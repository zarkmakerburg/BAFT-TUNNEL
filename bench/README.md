# bench/

`harness.py` is the E1 (loopback) integrity harness from docs/ADVISOR_ASSESSMENT.md §5. Stdlib Python, no root, no firewall/route/sysctl change.

Dry run 2026-11-17 on a cloud VM (`null-control`, plain relay, **not any tunnel**):
- 256 MiB, no fault: sha match, verdict `correct`, 191 MiB/s (harness self-check only).
- 64 MiB, cut at 20 MiB for 1 s, app-level retry: verdict `truncated`, sha mismatch (expected: a plain relay has no resume; shows the harness detects data loss).

These numbers validate the harness, not any project. No Rakhsh, Rathole, FRP or BAFT-TUNNEL result exists yet. Versions/commits are recorded at run time.
