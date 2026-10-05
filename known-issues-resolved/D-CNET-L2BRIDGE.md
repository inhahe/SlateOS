### D-CNET-L2BRIDGE. User-defined container networks now provide a shared layer-2 bridge (same-network peers reach each other directly) — RESOLVED 2026-07-01

**Resolution (2026-07-01):** each named network now stands up one
`net::bridge` instance and switches frames at L2 between its members'
veth host-ends, so two containers on the same named network reach each
other directly by their allocated IPs. The prior IPAM-only behaviour is
now backed by real inter-container reachability.

**What landed:**
1. **veth bridged flag** (`kernel/src/net/veth.rs`): `VethEnd` gained a
   `bridged: bool`; `poll_all()` skips bridged ends (the bridge owns their
   frames, not the global host stack). New `set_bridged`/`is_bridged`.
2. **Bridge veth ports** (`kernel/src/net/bridge.rs`): `BridgePort` gained
   `veth_pair: Option<usize>`; `MAX_BRIDGES` raised to 16.
   `attach_veth`/`detach_veth` register a veth pair's host-end (end A) as a
   bridge port (idempotent; port id = slot index), toggling the veth
   bridged flag outside the BRIDGES lock. `forward(bridge_idx)` drains each
   ingress port's `veth::recv(pair, A)`, learns src→port + resolves dst in
   one BRIDGES-locked step (MACs parsed via `get`+`try_from`, no slicing),
   then delivers: known unicast → `veth::send(out_pair, A, frame)`;
   broadcast/multicast/unknown → flood-clone to all other members **and**
   `ethernet::process_frame(&frame)` into the host stack (this preserves
   the pre-existing external-NAT path — no regression). `forward_all()`
   snapshots active bridges and forwards each.
3. **net::poll wiring** (`kernel/src/net/mod.rs`): `bridge::forward_all()`
   runs immediately before `veth::poll_all()`, so bridged host-ends are
   consumed by the bridge rather than the generic drain.
4. **Lazy per-network bridge lifecycle** (`kernel/src/cnetwork.rs`):
   `Network` gained `bridge_idx: Option<usize>`; `Allocation` gained
   `veth_pair: Option<usize>`. `attach_container_veth(name, cid, pair)`
   creates the bridge lazily on first attach, attaches the veth, and
   records the pair on the owning lease. `release`/`release_container`
   detach their veth pairs; `detach_and_maybe_teardown` deletes the bridge
   when its last port leaves (`veth_port_count == 0`).
5. **run-path wiring** (`kernel/src/kshell.rs`): the `oci run --network
   NAME` path calls `attach_container_veth` after taking the IPAM lease,
   printing `L2 bridge: NAME (N members)` (non-fatal warning if the veth
   is missing).

**Lock ordering:** `TABLE (cnetwork) → BRIDGES (bridge) → veth`; no reverse
edge, and `bridge::forward` never holds BRIDGES across veth I/O.

**Boot self-test:** `cnetwork::self_test()` builds a two-member network,
asserts the bridge is created lazily, exercises broadcast-flood and
learned-unicast forwarding, then verifies teardown on last detach —
serial `[cnetwork]   L2 bridge forward/learn: OK` and
`[cnetwork]   L2 bridge lifecycle: OK`.

**Follow-up (unchanged from before):** `poll_all` still dispatches
non-bridged veth frames into the *global* `ethernet::process_frame`;
per-namespace RX dispatch remains a separate TODO independent of this L2
switching work.

**Discovered/documented:** 2026-07-01 (while landing the `docker network`
IPAM feature, increments 60–61). **Resolved:** 2026-07-01.
