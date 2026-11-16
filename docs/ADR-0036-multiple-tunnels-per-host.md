# ADR-0036: more than one tunnel per host (one agent per instance, host list)

Status: accepted (2026-11-16, owner's delegated decision authority). Roadmap: 0.3.0 O2. Builds on ADR-0023, ADR-0026, ADR-0033.

## Context

The units are already templated (`baft-tunnel@NAME`, `baft-tunnel-agent@NAME`, per-instance config, runtime dir, state dir and root
control helpers), and `install.sh --register NAME --agent-listen 127.0.0.1:PORT` can register a second instance. What is missing is
the operator view: each agent and its panel know only their own instance, so a host with two tunnels has two unrelated panels and
no way to see that.

Two designs were weighed. **One agent serving every instance** would merge instance trust boundaries: one token, one process able to
read every key path and drive every helper, and a large rewrite of a 5.9k-line agent bound to `--instance`. **One agent per instance
plus a read-only host list** keeps today's isolation (ADR-0023/0026) and adds no privilege.

## Decision

1. Instance ownership is unchanged: one agent per instance, bound to its own config, runtime dir, state dir and helper requests.
   An agent never starts, stops, configures or reads secrets of another instance.
2. Each agent writes `agent.json` (`instance`, loopback `listen`; no secret) into its own state directory at startup.
3. `GET /v1/host/tunnels` (bearer-protected, read-only; also answered in setup mode) lists instances on the host: every valid
   `NAME.json` name in the config directory plus siblings that published `agent.json`. Sibling directories are read only when this
   agent's state dir is `PARENT/<its instance>` (installed layout). Per instance it returns name, `current`, `configured`,
   `agent_listen` (only loopback addresses), and the unit's `active`/`sub`/`enabled`. It returns no config contents, keys or tokens,
   and is capped at 32 entries with a `truncated` flag.
4. The panel's Tunnels page shows "Tunnels on this host". A sibling gets an open-panel link only when the page itself is served from a
   loopback address (SSH tunnel). Otherwise the agent address is shown as text. Per-instance paths behind the HTTPS front end
   (ADR-0033) and a "register another tunnel" helper from the panel are a follow-up, not part of this change.
5. The agents share the existing 0600 token file, so one bearer works for every local agent; ports must differ (`--agent-listen`).

## Not claimed

CI only (unit test with a fake units backend). No LAB run with two real instances yet; a host with two tunnels is not verified until
the B5 LAB thread runs it.
