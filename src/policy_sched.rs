//! Policy Pack queues and scheduler core (ADR-0048, TP-2.1 / #231, TP-2.2 wiring).
//!
//! Compiled only with the `policy-pack` cargo feature (off by default). `relay_with_policy`
//! (`policy_relay`) creates one [`Arbiter`] per relay direction per session and only when
//! `BAFT_TUNNEL_POLICY_PACK=sched` is set; with the switch off nothing here runs, so the data path
//! is unchanged.
//!
//! Two layers:
//! * [`Scheduler`] is the pure deficit round-robin bookkeeping: bounded per-flow queues, one
//!   quantum per visit, a full queue refused instead of grown.
//! * [`Arbiter`] is the runtime: one task per direction owns the scheduler and hands each granted
//!   flow its turn through a [`FlowHandle`]. A flow asks before it writes, so a granted flow is the
//!   only writer for its turn and no payload is copied into a second queue.
//!
//! Class weights (TP-3.x) are not set here: with the Balanced profile every class has the same
//! quantum, which is the documented default and the fail-safe. No benefit is claimed.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, oneshot};

use crate::policy::{Class, FlowId};

/// How the scheduler bounds one flow and one visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Most bytes one flow may be granted per visit (deficit round-robin quantum).
    pub quantum_bytes: usize,
    /// Hard bound on bytes queued for one flow. A larger `enqueue` is refused, never truncated.
    pub max_queued_bytes: usize,
}

/// Bytes one visit may move when the switch names no size (one 16 KiB read, as today's pump).
pub(crate) const DEFAULT_QUANTUM_BYTES: usize = 16 * 1024;

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            quantum_bytes: DEFAULT_QUANTUM_BYTES,
            max_queued_bytes: DEFAULT_QUANTUM_BYTES * 4,
        }
    }
}

/// Outcome of offering bytes to a flow's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enqueue {
    /// Bytes are queued.
    Accepted,
    /// The flow's queue is already at `max_queued_bytes`; nothing was queued.
    Full,
    /// The flow is not registered, so it has no queue and no class.
    NotScheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlowState {
    class: Class,
    queued: usize,
    deficit: usize,
}

/// Bounded per-flow queues with one deficit round-robin rotation.
///
/// `next_grant` visits flows in rotation order and returns at most `quantum_bytes`, so a flow with
/// data present is guaranteed a visit within one full rotation (ADR-0031 rule 5, ADR-0048
/// decision 3). A flow that drains to zero leaves the rotation and re-enters on the next enqueue.
#[derive(Debug)]
pub struct Scheduler {
    config: SchedulerConfig,
    flows: HashMap<FlowId, FlowState>,
    rotation: VecDeque<FlowId>,
    grants: u64,
    granted_bytes: u64,
    overflow_refusals: u64,
}

impl Scheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            flows: HashMap::new(),
            rotation: VecDeque::new(),
            grants: 0,
            granted_bytes: 0,
            overflow_refusals: 0,
        }
    }

    pub fn config(&self) -> SchedulerConfig {
        self.config
    }

    /// Replaces the bounds in force. Only a ratified ADR-0031 row is ever passed here (ADR-0056
    /// decision 1), so the new pair is already bounds-checked. Queued bytes are left alone: they
    /// drain under the new quantum on the next visits.
    pub fn set_config(&mut self, config: SchedulerConfig) {
        self.config = config;
    }

    /// Registers a flow with its configured class. Re-registering keeps any queued bytes.
    pub fn register(&mut self, flow: FlowId, class: Class) {
        let entry = self.flows.entry(flow).or_insert(FlowState {
            class,
            queued: 0,
            deficit: 0,
        });
        entry.class = class;
        if !self.rotation.contains(&flow) {
            self.rotation.push_back(flow);
        }
    }

    /// Removes a flow and its queue. Bytes already granted are not affected; nothing is reordered.
    pub fn close(&mut self, flow: FlowId) {
        self.flows.remove(&flow);
        self.rotation.retain(|candidate| *candidate != flow);
    }

    /// Offers `bytes` to a flow's queue. Refuses rather than growing past the bound. The relay
    /// uses [`Self::offer`] instead, because it re-offers a span it already queued; this additive
    /// form is what the scheduler's own tests model several flows with.
    #[cfg(test)]
    pub fn enqueue(&mut self, flow: FlowId, bytes: usize) -> Enqueue {
        let Some(state) = self.flows.get(&flow) else {
            return Enqueue::NotScheduled;
        };
        let Some(total) = state.queued.checked_add(bytes) else {
            self.overflow_refusals += 1;
            return Enqueue::Full;
        };
        if total > self.config.max_queued_bytes {
            self.overflow_refusals += 1;
            return Enqueue::Full;
        }
        if let Some(state) = self.flows.get_mut(&flow) {
            state.queued = total;
        }
        if !self.rotation.contains(&flow) {
            self.rotation.push_back(flow);
        }
        Enqueue::Accepted
    }

    /// Records the span a flow's single in-flight request covers, replacing whatever it offered
    /// before. The relay re-offers the bytes a grant left unwritten, and those bytes are already
    /// in this queue, so adding them again would count them twice and could answer a later
    /// request with more bytes than it offered. A span past the cap is refused exactly as
    /// [`Self::enqueue`] refuses it.
    pub fn offer(&mut self, flow: FlowId, bytes: usize) -> Enqueue {
        if !self.flows.contains_key(&flow) {
            return Enqueue::NotScheduled;
        }
        if bytes > self.config.max_queued_bytes {
            self.overflow_refusals += 1;
            return Enqueue::Full;
        }
        if let Some(state) = self.flows.get_mut(&flow) {
            state.queued = bytes;
        }
        if bytes > 0 && !self.rotation.contains(&flow) {
            self.rotation.push_back(flow);
        }
        Enqueue::Accepted
    }

    /// Next flow to serve and how many bytes it may move now, bounded by `max_bytes` and the
    /// quantum. `None` means every queue is empty.
    #[cfg(test)]
    pub fn next_grant(&mut self, max_bytes: usize) -> Option<(FlowId, usize)> {
        self.next_grant_for(max_bytes, |_| true)
    }

    /// A grant for the next flow that both has queued bytes and is `ready` for them. The arbiter
    /// passes the set of flows whose acquire is still waiting: granting bytes to a flow that has
    /// no request outstanding would count bytes nobody writes and inflate the granted-byte rate
    /// the adaptive loop divides by (ADR-0056 decision 4).
    pub fn next_grant_for(
        &mut self,
        max_bytes: usize,
        mut ready: impl FnMut(FlowId) -> bool,
    ) -> Option<(FlowId, usize)> {
        if max_bytes == 0 {
            return None;
        }
        for _ in 0..self.rotation.len() {
            let flow = self.rotation.pop_front()?;
            let Some(state) = self.flows.get_mut(&flow) else {
                continue;
            };
            if state.queued == 0 {
                state.deficit = 0;
                continue;
            }
            if !ready(flow) {
                // Keep the flow's place and its queue; it is granted when its request arrives.
                self.rotation.push_back(flow);
                continue;
            }
            if state.deficit == 0 {
                state.deficit = self.config.quantum_bytes;
            }
            let grant = state
                .queued
                .min(state.deficit)
                .min(max_bytes)
                .min(self.config.quantum_bytes);
            if grant == 0 {
                // A zero quantum cannot make progress; keep the flow and report nothing.
                self.rotation.push_back(flow);
                continue;
            }
            state.queued -= grant;
            state.deficit -= grant;
            if state.queued == 0 {
                state.deficit = 0;
            } else {
                self.rotation.push_back(flow);
            }
            self.grants += 1;
            self.granted_bytes = self.granted_bytes.saturating_add(grant as u64);
            return Some((flow, grant));
        }
        None
    }

    #[cfg(test)]
    pub fn queued_bytes(&self, flow: FlowId) -> usize {
        self.flows.get(&flow).map_or(0, |state| state.queued)
    }

    pub fn class_of(&self, flow: FlowId) -> Option<Class> {
        self.flows.get(&flow).map(|state| state.class)
    }

    pub fn total_queued_bytes(&self) -> usize {
        self.flows.values().map(|state| state.queued).sum()
    }

    pub fn flows(&self) -> usize {
        self.flows.len()
    }

    pub fn grants(&self) -> u64 {
        self.grants
    }

    /// Bytes handed to flows so far; the rate half of ADR-0056 decision 4's estimate.
    pub fn granted_bytes(&self) -> u64 {
        self.granted_bytes
    }

    pub fn overflow_refusals(&self) -> u64 {
        self.overflow_refusals
    }
}

/// Requests one arbiter accepts before senders block. A flow holds at most one request at a time,
/// so this bounds how many flows can be contending without queueing unbounded work in the channel.
const COMMAND_BUFFER: usize = 64;

/// Snapshot of an arbiter's counters, used by tests and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ArbiterStats {
    pub grants: u64,
    pub granted_bytes: u64,
    pub overflow_refusals: u64,
    pub queued_bytes: usize,
    pub flows: usize,
}

#[allow(dead_code)] // `Stats` is only reached from tests and diagnostics.
enum Command {
    Request {
        flow: FlowId,
        bytes: usize,
        reply: oneshot::Sender<usize>,
    },
    Close(FlowId),
    Configure(SchedulerConfig),
    Stats(oneshot::Sender<ArbiterStats>),
}

/// The single arbitration point for one relay direction (ADR-0048 decision 5).
///
/// One task owns the [`Scheduler`] and is the only thing that decides who writes next, so the
/// rotation is global to the direction instead of per stream. The task ends when every
/// [`FlowHandle`] and the [`Arbiter`] are dropped, so it cannot outlive its session.
#[derive(Debug)]
pub(crate) struct Arbiter {
    commands: mpsc::Sender<Command>,
}

impl Arbiter {
    /// Starts the arbiter task. Must be called from a Tokio runtime.
    pub(crate) fn spawn(config: SchedulerConfig) -> Arc<Self> {
        let (commands, inbox) = mpsc::channel(COMMAND_BUFFER);
        tokio::spawn(run(config, inbox));
        Arc::new(Self { commands })
    }

    /// Handle for one flow in this direction. Dropping the handle closes the flow's queue.
    pub(crate) fn flow(self: &Arc<Self>, flow: FlowId) -> FlowHandle {
        FlowHandle {
            flow,
            arbiter: Arc::clone(self),
            closed: AtomicBool::new(false),
        }
    }

    /// Applies a new scheduler pair to a running direction (ADR-0056 decision 2). `false` means
    /// the arbiter has already stopped, so the caller must not assume the change took effect.
    pub(crate) async fn configure(&self, config: SchedulerConfig) -> bool {
        self.commands.send(Command::Configure(config)).await.is_ok()
    }

    #[allow(dead_code)] // tests and diagnostics ask the arbiter what it is holding.
    pub(crate) async fn stats(&self) -> Option<ArbiterStats> {
        let (reply, answer) = oneshot::channel();
        self.commands.send(Command::Stats(reply)).await.ok()?;
        answer.await.ok()
    }
}

/// One flow's handle to a direction's arbiter.
#[derive(Debug)]
pub(crate) struct FlowHandle {
    flow: FlowId,
    arbiter: Arc<Arbiter>,
    closed: AtomicBool,
}

impl FlowHandle {
    /// Waits until this flow's turn in the rotation, then returns how many of the `bytes` offered
    /// may be written now. The caller writes exactly that many and offers the rest again.
    /// `Err` means the flow was closed or the arbiter stopped, and the relay must end.
    pub(crate) async fn acquire(&self, bytes: usize) -> io::Result<usize> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "flow closed"));
        }
        if bytes == 0 {
            return Ok(0);
        }
        let (reply, answer) = oneshot::channel();
        self.arbiter
            .commands
            .send(Command::Request {
                flow: self.flow,
                bytes,
                reply,
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "flow scheduler stopped"))?;
        let grant = answer
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "flow closed while queued"))?;
        if grant == 0 {
            return Err(io::Error::other(
                "flow scheduler refused bytes over the queue cap",
            ));
        }
        Ok(grant)
    }

    /// Closes the flow: its queue, its rotation slot and any acquire still waiting are released.
    /// Later acquires on this handle fail instead of re-opening the flow.
    pub(crate) async fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.arbiter.commands.send(Command::Close(self.flow)).await;
    }
}

impl Drop for FlowHandle {
    fn drop(&mut self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // A relay task that is aborted never runs its `close`; the queue must still go away, or the
        // rotation would keep granting bytes nobody writes. `try_send` keeps `Drop` non-blocking;
        // if the channel is full the flow's pending bytes drain on their own and a drained flow
        // leaves the rotation, so nothing wedges either way.
        let _ = self.arbiter.commands.try_send(Command::Close(self.flow));
    }
}

/// The arbiter task: own the scheduler, accept requests, hand out grants in rotation order.
///
/// A request that would push a flow past `max_queued_bytes` is answered with a zero grant, which
/// `acquire` turns into an error: the relay ends that flow instead of buffering past the cap. With
/// one chunk in flight per flow (the relay asks after each granted write, never before) a queue holds
/// at most one quantum, so the cap only bites on a misconfigured quantum and the configuration
/// refuses that case up front.
async fn run(config: SchedulerConfig, mut commands: mpsc::Receiver<Command>) {
    let mut scheduler = Scheduler::new(config);
    let mut waiting: HashMap<FlowId, oneshot::Sender<usize>> = HashMap::new();
    loop {
        serve(&mut scheduler, &mut waiting);
        match commands.recv().await {
            Some(Command::Request { flow, bytes, reply }) => {
                if scheduler.class_of(flow).is_none() {
                    scheduler.register(flow, Class::Balanced);
                }
                match scheduler.offer(flow, bytes) {
                    Enqueue::Accepted => {
                        waiting.insert(flow, reply);
                    }
                    Enqueue::Full => {
                        let _ = reply.send(0);
                    }
                    Enqueue::NotScheduled => {
                        let _ = reply.send(0);
                    }
                }
            }
            Some(Command::Close(flow)) => {
                scheduler.close(flow);
                // Dropping the sender ends any acquire still waiting on this flow.
                waiting.remove(&flow);
            }
            Some(Command::Configure(config)) => scheduler.set_config(config),
            Some(Command::Stats(reply)) => {
                let _ = reply.send(ArbiterStats {
                    grants: scheduler.grants(),
                    granted_bytes: scheduler.granted_bytes(),
                    overflow_refusals: scheduler.overflow_refusals(),
                    queued_bytes: scheduler.total_queued_bytes(),
                    flows: scheduler.flows(),
                });
            }
            None => break,
        }
    }
    // Every handle is gone: dropping `waiting` releases the remaining acquires as closed.
}

/// Grants every queue that can move now. A queued flow without a waiter still has its bytes
/// granted and discarded, which lets a stale queue drain instead of blocking the rotation.
fn serve(scheduler: &mut Scheduler, waiting: &mut HashMap<FlowId, oneshot::Sender<usize>>) {
    while let Some((flow, grant)) = scheduler
        .next_grant_for(scheduler.config().quantum_bytes, |flow| {
            waiting.contains_key(&flow)
        })
    {
        let Some(reply) = waiting.remove(&flow) else {
            // Unreachable: `ready` selected this flow, so a request is waiting.
            break;
        };
        let _ = reply.send(grant);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler() -> Scheduler {
        Scheduler::new(SchedulerConfig {
            quantum_bytes: 1_000,
            max_queued_bytes: 4_000,
        })
    }

    fn drained(scheduler: &mut Scheduler) -> Vec<(FlowId, usize)> {
        let mut grants = Vec::new();
        while let Some(grant) = scheduler.next_grant(4_096) {
            grants.push(grant);
        }
        grants
    }

    #[test]
    fn balanced_flows_take_turns_in_rotation_order() {
        let mut scheduler = scheduler();
        let (a, b) = (FlowId(1), FlowId(2));
        scheduler.register(a, Class::Balanced);
        scheduler.register(b, Class::Balanced);
        assert_eq!(scheduler.enqueue(a, 3_000), Enqueue::Accepted);
        assert_eq!(scheduler.enqueue(b, 3_000), Enqueue::Accepted);

        let grants = drained(&mut scheduler);
        let order: Vec<FlowId> = grants.iter().map(|(flow, _)| *flow).collect();
        assert_eq!(
            order,
            vec![a, b, a, b, a, b],
            "equal quanta must alternate between equal-class flows"
        );
        assert!(grants.iter().all(|(_, bytes)| *bytes == 1_000));
        assert_eq!(scheduler.total_queued_bytes(), 0);
    }

    #[test]
    fn no_flow_starves_under_sustained_load() {
        let mut scheduler = scheduler();
        let flows = [FlowId(1), FlowId(2), FlowId(3), FlowId(4)];
        for flow in flows {
            scheduler.register(flow, Class::Balanced);
        }
        let mut visits = HashMap::new();
        for _ in 0..40 {
            // Keep every queue non-empty so fairness, not arrival order, decides.
            for flow in flows {
                let _ = scheduler.enqueue(flow, 500);
            }
            if let Some((flow, _)) = scheduler.next_grant(1_000) {
                *visits.entry(flow).or_insert(0_u64) += 1;
            }
        }
        for flow in flows {
            let count = visits.get(&flow).copied().unwrap_or(0);
            assert_eq!(
                count, 10,
                "equal classes must serve {flow:?} once per four grants, saw {count} of 40"
            );
        }
    }

    #[test]
    fn a_flow_is_visited_at_least_once_per_rotation() {
        let mut scheduler = scheduler();
        let flows = [FlowId(10), FlowId(11), FlowId(12)];
        for flow in flows {
            scheduler.register(flow, Class::Balanced);
            assert_eq!(scheduler.enqueue(flow, 4_000), Enqueue::Accepted);
        }
        // Three flows, quantum 1000: the first three grants are one from each.
        let mut first_round = Vec::new();
        for _ in 0..flows.len() {
            first_round.push(scheduler.next_grant(4_096).expect("grant").0);
        }
        first_round.sort_by_key(|flow| flow.0);
        assert_eq!(first_round, flows.to_vec());
    }

    #[test]
    fn a_grant_never_exceeds_the_quantum_or_the_request() {
        let mut scheduler = scheduler();
        let flow = FlowId(7);
        scheduler.register(flow, Class::Bulk);
        assert_eq!(scheduler.enqueue(flow, 4_000), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(64), Some((flow, 64)));
        // The remaining deficit (1000 - 64) bounds the next visit before a new round starts.
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 936)));
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 1_000)));
        assert_eq!(scheduler.next_grant(0), None);
    }

    #[test]
    fn queue_overflow_is_refused_and_counted_not_truncated() {
        let mut scheduler = scheduler();
        let flow = FlowId(3);
        scheduler.register(flow, Class::Balanced);
        assert_eq!(scheduler.enqueue(flow, 4_000), Enqueue::Accepted);
        assert_eq!(scheduler.enqueue(flow, 1), Enqueue::Full);
        assert_eq!(scheduler.queued_bytes(flow), 4_000);
        assert_eq!(scheduler.overflow_refusals(), 1);
        assert_eq!(scheduler.enqueue(flow, usize::MAX), Enqueue::Full);
        assert_eq!(scheduler.overflow_refusals(), 2);
    }

    #[test]
    fn a_reoffered_remainder_is_not_counted_twice_and_drains_to_zero() {
        let mut scheduler = scheduler();
        let flow = FlowId(5);
        scheduler.register(flow, Class::Balanced);
        // The relay offers the span its request covers; a grant of one quantum leaves the rest
        // queued, and the relay's next offer repeats that span rather than adding to it.
        assert_eq!(scheduler.offer(flow, 2_500), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 1_000)));
        assert_eq!(scheduler.queued_bytes(flow), 1_500);
        assert_eq!(scheduler.offer(flow, 1_500), Enqueue::Accepted);
        assert_eq!(
            scheduler.queued_bytes(flow),
            1_500,
            "re-offering the remainder must not add it again"
        );
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 1_000)));
        assert_eq!(scheduler.queued_bytes(flow), 500);
        assert_eq!(scheduler.offer(flow, 500), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 500)));
        assert_eq!(
            scheduler.queued_bytes(flow),
            0,
            "a complete read leaves no queued bytes"
        );
        // A read smaller than the quantum is granted whole, and also leaves the queue empty.
        assert_eq!(scheduler.offer(flow, 300), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(4_096), Some((flow, 300)));
        assert_eq!(scheduler.queued_bytes(flow), 0);
        // The cap bounds the span one request covers, not a sum of past offers.
        assert_eq!(scheduler.offer(flow, 4_001), Enqueue::Full);
        assert_eq!(scheduler.overflow_refusals(), 1);
        assert_eq!(scheduler.offer(flow, 0), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(4_096), None);
    }

    #[test]
    fn unknown_and_closed_flows_have_no_queue() {
        let mut scheduler = scheduler();
        let flow = FlowId(4);
        assert_eq!(scheduler.enqueue(flow, 10), Enqueue::NotScheduled);
        scheduler.register(flow, Class::Gaming);
        assert_eq!(scheduler.class_of(flow), Some(Class::Gaming));
        assert_eq!(scheduler.enqueue(flow, 500), Enqueue::Accepted);
        scheduler.close(flow);
        assert_eq!(scheduler.flows(), 0);
        assert_eq!(scheduler.queued_bytes(flow), 0);
        assert_eq!(scheduler.next_grant(1_000), None);
        assert_eq!(scheduler.enqueue(flow, 10), Enqueue::NotScheduled);
    }

    #[test]
    fn a_drained_flow_leaves_the_rotation() {
        let mut scheduler = scheduler();
        let flow = FlowId(9);
        scheduler.register(flow, Class::Streaming);
        assert_eq!(scheduler.enqueue(flow, 500), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(1_000), Some((flow, 500)));
        assert_eq!(scheduler.next_grant(1_000), None);
        assert_eq!(scheduler.enqueue(flow, 500), Enqueue::Accepted);
        assert_eq!(scheduler.next_grant(1_000), Some((flow, 500)));
    }

    fn arbiter_config() -> SchedulerConfig {
        SchedulerConfig {
            quantum_bytes: 1_024,
            max_queued_bytes: 4_096,
        }
    }

    #[tokio::test]
    async fn arbiter_grants_every_contending_flow_equally() {
        let arbiter = Arbiter::spawn(arbiter_config());
        let flows = [FlowId(1), FlowId(2), FlowId(3)];
        let mut tasks = Vec::new();
        for flow in flows {
            let handle = arbiter.flow(flow);
            tasks.push(tokio::spawn(async move {
                let mut bytes = 0;
                for _ in 0..6 {
                    bytes += handle.acquire(1_024).await.expect("grant");
                }
                bytes
            }));
        }
        for task in tasks {
            assert_eq!(task.await.expect("task"), 6 * 1_024);
        }
        let stats = arbiter.stats().await.expect("stats");
        assert_eq!(
            stats.grants, 18,
            "one grant per visit, three flows, six visits"
        );
        assert_eq!(stats.queued_bytes, 0, "every granted byte left the queue");
        assert_eq!(stats.overflow_refusals, 0);
    }

    #[tokio::test]
    async fn arbiter_refuses_bytes_over_the_cap_instead_of_queueing_them() {
        // A quantum larger than the cap is a misconfiguration the relay's parser refuses; the
        // scheduler must still refuse rather than grow the queue past the bound.
        let arbiter = Arbiter::spawn(SchedulerConfig {
            quantum_bytes: 4_096,
            max_queued_bytes: 2_048,
        });
        let handle = arbiter.flow(FlowId(5));
        let error = handle
            .acquire(4_096)
            .await
            .expect_err("over-cap bytes must be refused");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        let stats = arbiter.stats().await.expect("stats");
        assert_eq!(stats.queued_bytes, 0, "nothing was buffered past the cap");
        assert_eq!(stats.overflow_refusals, 1);
        assert_eq!(stats.grants, 0);
    }

    #[tokio::test]
    async fn a_closed_flow_ends_its_acquire_and_leaves_no_queue() {
        let arbiter = Arbiter::spawn(arbiter_config());
        let handle = arbiter.flow(FlowId(6));
        assert_eq!(handle.acquire(1_024).await.expect("grant"), 1_024);
        handle.close().await;
        let error = handle.acquire(1_024).await.expect_err("closed flow");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        let mut stats = arbiter.stats().await.expect("stats");
        for _ in 0..100 {
            if stats.flows == 0 && stats.queued_bytes == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            stats = arbiter.stats().await.expect("stats");
        }
        assert_eq!(stats.flows, 0, "a closed flow must leave the rotation");
        assert_eq!(stats.queued_bytes, 0);
    }

    #[tokio::test]
    async fn a_dropped_flow_handle_closes_its_queue() {
        let arbiter = Arbiter::spawn(arbiter_config());
        {
            let handle = arbiter.flow(FlowId(7));
            assert_eq!(handle.acquire(512).await.expect("grant"), 512);
        }
        let mut stats = arbiter.stats().await.expect("stats");
        for _ in 0..100 {
            if stats.flows == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            stats = arbiter.stats().await.expect("stats");
        }
        assert_eq!(
            stats.flows, 0,
            "drop must close the flow, not leak its slot"
        );
    }

    /// ADR-0056 decision 4 divides queued bytes by the granted bytes over the interval, so the
    /// arbiter must report bytes, not only a grant count.
    #[tokio::test]
    async fn arbiter_counts_the_bytes_it_grants() {
        let arbiter = Arbiter::spawn(arbiter_config());
        let handle = arbiter.flow(FlowId(8));
        assert_eq!(handle.acquire(1_024).await.expect("grant"), 1_024);
        let stats = arbiter.stats().await.expect("stats");
        assert_eq!(stats.grants, 1);
        assert_eq!(stats.granted_bytes, 1_024);
    }

    /// ADR-0056 decision 2: a change applies to a running direction, with no session restart. The
    /// next grant uses the new pair.
    #[tokio::test]
    async fn arbiter_applies_a_new_pair_to_a_running_direction() {
        let arbiter = Arbiter::spawn(arbiter_config());
        let handle = arbiter.flow(FlowId(9));
        assert_eq!(handle.acquire(1_024).await.expect("grant"), 1_024);
        assert!(
            arbiter
                .configure(SchedulerConfig {
                    quantum_bytes: 512,
                    max_queued_bytes: 2_048,
                })
                .await
        );
        assert_eq!(
            handle.acquire(1_024).await.expect("grant"),
            512,
            "the shrunk quantum must bound the very next grant"
        );
        let stats = arbiter.stats().await.expect("stats");
        assert_eq!(stats.granted_bytes, 1_536);
        assert_eq!(stats.overflow_refusals, 0);
    }
}
