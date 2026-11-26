//! Policy Pack queues and scheduler core (ADR-0048, TP-2.1 / #231).
//!
//! Compiled only with the `policy-pack` cargo feature (off by default). **Nothing calls this yet:**
//! the relay still uses the fixed 16 KiB pump, so the data path with the pack off is unchanged.
//! This module provides the bounded per-flow queues and the deficit round-robin scheduler that
//! TP-2.1 specifies, plus the properties TP-2.2 must hold: every flow is visited once per round
//! (bounded wait), a quantum bounds one visit, and a full queue is refused rather than growing.
//!
//! Class weights (TP-3.x) are not set here: with the Balanced profile every class has the same
//! quantum, which is the documented default and the fail-safe. No benefit is claimed.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use crate::policy::{Class, FlowId};

/// How the scheduler bounds one flow and one visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Most bytes one flow may be granted per visit (deficit round-robin quantum).
    pub quantum_bytes: usize,
    /// Hard bound on bytes queued for one flow. A larger `enqueue` is refused, never truncated.
    pub max_queued_bytes: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            quantum_bytes: 16 * 1024,
            max_queued_bytes: 64 * 1024,
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
    overflow_refusals: u64,
}

impl Scheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            flows: HashMap::new(),
            rotation: VecDeque::new(),
            grants: 0,
            overflow_refusals: 0,
        }
    }

    pub fn config(&self) -> SchedulerConfig {
        self.config
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

    /// Offers `bytes` to a flow's queue. Refuses rather than growing past the bound.
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

    /// Next flow to serve and how many bytes it may move now, bounded by `max_bytes` and the
    /// quantum. `None` means every queue is empty.
    pub fn next_grant(&mut self, max_bytes: usize) -> Option<(FlowId, usize)> {
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
            return Some((flow, grant));
        }
        None
    }

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

    pub fn overflow_refusals(&self) -> u64 {
        self.overflow_refusals
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
}
