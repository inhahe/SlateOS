## 62. `nft`/`iptables` compat tooling (Q21) — keep as an explicit parser/pretty-printer only; fix the docs; steer users to `fw` (option C)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended C).

**Context.** `userspace/nft` (also `iptables`/`ip6tables` via `argv[0]`) is
stateless: each invocation builds a fresh `Ruleset`, applies one command, prints,
and discards it — it never persists or touches the kernel, despite a module doc
claiming persistence. The native `fw` tool now fully configures the kernel
firewall (§53). The kernel firewall model is far narrower than nftables (no NAT,
no sets/maps, one src IP/prefix + one dst port, input/output only), so a faithful
`nft` is impossible and a lossy one risks silently under-applying a user's policy.

**Decision.** Keep `nft`/`iptables` as an **explicit parser/pretty-printer
only.** Correct the module doc to state it does not persist or apply; print a
clear "not applied — use `fw` to configure the kernel firewall" notice on
mutating commands; treat `fw` as the one true firewall front-end. Full/minimal
wiring (A/B) is deferred until a concrete need appears.

**Alternatives.** (A) Full-ish wiring, (B) minimal wiring — both deferred:
large, lossy, misleading against the narrow kernel model.

**Where it lives.** `userspace/nft/src/main.rs` (`run_nft`/`run_iptables` module
doc + mutating-command notice). Related: known-issues TD18 residual.
