## 60. Container network model (Q19) — generalise to multi-network membership (Docker parity, option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended B).

**Context.** Docker containers can join **multiple** user-defined networks, each
with its own interface + address + embedded-DNS scope. Our model assumed **one**
veth pair per container. `container network connect/disconnect` needs a model
decision.

**Decision.** Generalise the data model to **N interfaces per container.**
`Container` holds a list of `(netns-iface, veth_pair, network_name, ip)`
memberships; `network connect` allocates a new veth into the running container's
netns, configures it, attaches it to that network's bridge, and registers DNS
names; `network disconnect` tears one membership down. `inspect`/`ps` become
per-network. Its own dedicated increment (a real refactor).

**Alternative.** (A) Single-network minimal — rejected: diverges from Docker.

**Where it lives.** `kernel/src/container.rs` (`Container.veth_pair` → membership
list; runtime `attach_network`/`detach_network`), `kernel/src/cnetwork.rs`
(runtime connect), `kernel/src/kshell.rs` (`container network
connect|disconnect` arms + `docker` delegate).
