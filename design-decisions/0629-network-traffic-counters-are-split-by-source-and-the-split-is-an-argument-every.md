## 629. Network traffic counters are split by source, and the split is an argument every recorder must pass

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the kernel counted every Ethernet frame it moved in one set of six
numbers and printed them under the heading `eth0` — the network card. But two
different things move frames: the card, and virtual cables (`veth`) that carry
traffic between containers on this machine and never reach the card at all. So a
machine busy only between its own containers reported a busy *card*, and nothing
in the output let you tell how much, if any, had actually gone out on the wire.
There are now two sets of numbers, one per source, and each place that counts a
frame has to say which it is.

**The alternatives considered.**

| option | why not |
|---|---|
| Leave it and document the conflation | This is what was done on 2026-08-26 — `fs::netdev` named its projected row `kernel` instead of `eth0` precisely to avoid asserting something false. It works, but it pushes the problem onto every future reader: the honest name is one nobody is looking for, and the number still cannot answer the question an operator is asking ("is the wire the bottleneck?"). |
| Stop counting veth frames in `net::interface` | Trades an over-count for an under-count. The veth traffic is real work the stack did; deleting it would make the machine's total frame rate wrong, and the known-issue entry called this out in advance. |
| A `bool from_nic` flag on the existing counters | A flag decides *this* case and nothing after it. The moment a second NIC, a bridge or a tun device appears there is no place to put it. |
| A name-keyed map of counter groups | The natural shape, and the wrong one *here*: `record_rx` is on the path of every single frame, so a map means a lock acquisition and a string comparison per packet. That per-frame cost is exactly what `fs::netdev` refused when it chose to project at read time rather than call `record_rx` per frame (§ the `netdev` entry). A fixed set of groups keeps the hot path a single relaxed `fetch_add` on a static. |

**What was chosen:** `enum Source { Nic, Veth }`, one `static CounterGroup` per
variant, and a `const fn group(Source) -> &'static CounterGroup` that is a
`match`, not an index. Two consequences were the point rather than side-effects:

- **The `match` makes adding a variant a compile error** until its storage
  exists. An indexed array would have compiled and silently gone out of bounds
  (or, with `get`, silently fallen back to the NIC's column — a wrong number
  that looks like a right one, which is the failure mode this project keeps
  finding).
- **`Source` is a required argument, not a defaulted one.** `record_rx(bytes)`
  with a `Source::Nic` default would let a future frame path be wired up without
  anyone deciding what it counts as — which is exactly how the original bug
  happened: `veth::poll` reached for the counters that existed. Making it an
  argument means the compiler asks the question at every call site.

**Cost accepted:** eleven call sites had to change, and every reader had to
decide whether it wanted the NIC, the veth aggregate, or the sum — there is no
longer a single number to reach for by default. That is the intended cost; the
decision each reader now has to make is one it was previously making by accident.
Where the answer was "the sum", it is written down with the reason
(`netstat -s`'s IP section, because a veth frame really is a packet IP received).

**Deliberately not per-endpoint.** The plan allowed "one group per veth endpoint
or at least a `veth` aggregate". `net::veth` already keeps exact per-endpoint
counts in `VethEndStats`; a second per-endpoint copy at this level would be a
second thing to keep in step and would answer no question the first cannot. The
information that was missing was the wire/not-wire split, and that is one bit.

**Where it is:** `kernel/src/net/interface.rs` (the groups, `Source`,
`stats`/`stats_for`/`stats_total`, self-test case 7); recorders in
`kernel/src/net/mod.rs` and `kernel/src/net/veth.rs`; readers in
`kernel/src/net/netstat.rs`, `kernel/src/net/dashboard.rs`,
`kernel/src/fs/netdev.rs` and `sys_net_stat` in
`kernel/src/syscall/handlers.rs`. Full account in `known-issues.md` →
`A-NET-INTERFACE-COUNTERS-CONFLATE-THE-NIC-WITH-EVERY-VETH-PAIR`.
