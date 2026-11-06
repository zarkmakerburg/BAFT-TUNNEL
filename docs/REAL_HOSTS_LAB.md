# Optional six-host LAB matrix — not currently active

This document preserves the original expansion plan for a future six-host matrix. It is **not** the active LAB configuration and no six-host workflow is currently enabled.

The active LAB is the two-host Iran/abroad environment described in [PHASE0_TWO_HOST_LAB.md](PHASE0_TWO_HOST_LAB.md). Do not add placeholder addresses or assume that the additional four hosts exist.

## Future matrix shape

If the owner later provisions the additional dedicated test hosts, the optional matrix can use:

| Role | Future environment variable |
|---|---|
| Iran 1 | `IR_1_HOST` |
| Iran 2 | `IR_2_HOST` |
| Abroad 1 | `EX_1_HOST` |
| Abroad 2 | `EX_2_HOST` |
| Abroad 3 | `EX_3_HOST` |
| Abroad 4 | `EX_4_HOST` |

The intended matrix is eight directed Iran-to-abroad TCP paths. The existing `lab/real-hosts/preflight-matrix.sh` and its mock tests remain available as future groundwork, but there is no active six-host Actions workflow. The former six-host `real-host-lab.yml` workflow has been removed to avoid an unusable workflow that requires unconfigured hosts.

## Security and activation gate

Before reintroducing a six-host workflow, require owner-provided dedicated test hosts, independently verified host keys, a reviewed environment-variable contract, a read-only/manual confirmation gate, strict SSH host-key verification, masked host values, and cleanup proof. Never reuse BAFT credentials or use production/customer hosts.

The mock matrix test uses fake SSH and documentation-only addresses; it is not real-host evidence. A successful future preflight would prove only SSH inspection and observed TCP reachability, not tunnel functionality, throughput, failover, censorship resistance, or production readiness.
