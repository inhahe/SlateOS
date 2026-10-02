## `A-NET-INTERFACE-COUNTERS-CONFLATE-THE-NIC-WITH-EVERY-VETH-PAIR` (lane A, 2026-08-26)

**Status:** ✅ FIXED 2026-08-27 — `net::interface` now keeps one counter group
per `Source` (`Nic`, `Veth`); every recorder names its source; `stats()` is the
NIC's group; `stats_for`/`stats_total` expose the rest.

**What landed, against the plan below:**

| plan | landed |
|---|---|
| counter groups keyed by source | `enum Source { Nic, Veth }` + one `static CounterGroup` each, selected by a `match` in `group()`. Not an indexed array: the `match` makes a new variant a compile error until its storage exists, and leaves the frame path with no bounds check. |
| each recorder names its source | `record_tx`/`record_tx_error`/`record_rx`/`record_rx_drop` all take `Source` as their first argument. Not a defaulted flag, so a new frame path cannot be added without someone deciding which column it belongs in. |
| `stats()` returns the NIC's group | done; `stats_for(Source)` and `stats_total()` added beside it. |
| `netstat -i` grows a row per source | done — the config block still describes the NIC, and the traffic block below it is now a table with one row per `Source::ALL` plus a `total` row, iterated so a new variant adds a row without touching the reader. |
| `netdev::kernel_row` projects the NIC group, row renamed `eth0` | done — `nic_row`, `NIC_IFACE = "eth0"`. The name stays reserved against `register_iface` for the original reason: two rows called `eth0` would be worse than one badly-named one. |

**Three things the plan did not anticipate.**

1. **There was a third conflated site, and it was a *transmit*.**
   `net::send_frame_ns` (`net/mod.rs:201`) calls `veth::send` and then recorded
   the frame as NIC transmit. The entry below lists only `veth::poll`'s two
   receive sites, because it was written from `veth.rs`. So the old `eth0` TX
   column over-reported too, not just RX. Now `Source::Veth`.
2. **`SYS_NET_STAT` (825) had to grow a selector rather than change meaning.**
   Its 48-byte record has no source field, so making it return the NIC's group
   would have silently narrowed what an existing caller sees. `arg1` now selects:
   `0` = every source summed (exactly what the call returned before, so no caller
   changes), `1` = NIC, `2` = veth. Anything else is `InvalidArgument` rather
   than a fallback to `0`, so a caller written against a future source cannot be
   handed the total and told it is that source's traffic. No in-tree caller
   invokes it yet — `posix/src/syscall.rs`'s two references are capability
   allow-lists — but the ABI is published and was treated as such.
3. **`/metrics` needed a label, not a second metric name.** `dashboard`'s four
   `os_net_*_total` counters are now emitted by a new `prom_counter_labeled`
   with `source="eth0"` / `source="veth"`. A scraper can sum the label to
   recover the old single number; it could never have split a merged one. The
   JSON `/api/system` and `/api/network` views keep `stats()` — those numbers sit
   beside that interface's IP and MAC, so the NIC's column is the correct one and
   they simply became true.

4. **Renaming the projected row collided with `netdev`'s own test fixture, and
   only a boot caught it.** `netdev::self_test` built its recorded fixtures on
   an interface it called `"eth0"`. The moment `NIC_IFACE` became `"eth0"`,
   `register_iface` correctly refused it and case 2 panicked the kernel —
   `!!! KERNEL PANIC !!! register: AlreadyExists` at `netdev.rs:510`, boot test
   60, eighteen minutes in. The reservation was doing its job; two string
   constants in one file had simply come to agree by accident. Fixed by naming
   the fixture for what it is (`const DEV: &str = "testnic0"`) **and** by making
   the collision a build error rather than a boot panic:

   ```rust
   const _: () = assert!(
       !const_str_eq(DEV, NIC_IFACE),
       "netdev self-test fixture must not use the reserved NIC_IFACE name"
   );
   ```

   Verified by temporarily setting `DEV` back to `"eth0"` and confirming
   `cargo check` fails with `E0080` — a guard nobody has watched fail is a guard
   nobody knows works. Case 1 additionally now asserts the reserved name is
   refused on an *empty* table, which is what distinguishes a reservation from
   ordinary duplicate detection: with a row present, `AlreadyExists` could in
   principle have come from the row.

**`netstat -s`'s IP section reads `stats_total()`, deliberately.** A frame
drained from a veth pair is handed to `ethernet::process_frame` exactly like one
off the wire, so it genuinely *is* a packet the IP layer received. What veth
traffic lacks is the wire attribution, not the processing — so it belongs in an
IP-level count and not under an `eth0` heading. Same reasoning for the boot-time
`[net] Traffic` line, which instead prints one line per source.

**Why the veth group is an aggregate and not per-endpoint.** `net::veth` already
keeps exact per-endpoint byte and frame counts in its own `VethEndStats`. A
second per-endpoint copy here would be a second thing to keep in step for no new
information; what was missing at *this* level was only the wire/not-wire split.

**Test coverage** is `interface::self_test` case 7 (7 tests, was 6). It
deliberately does **not** record into the live counters: `record_rx(Source::Veth,
100)` followed by "check the NIC group did not move" would leave 100 bytes of
traffic that never happened permanently visible in `netstat -i` and
`/proc/netdev`, and the counters are monotonic by contract so there is no honest
way to take it back — `test_write_primitives` can mutate live config only because
it can restore it. Instead it exercises the same `CounterGroup` methods on a
local group, asserts every `Source` maps to distinct storage (`core::ptr::eq`
over all pairs — if two variants shared a group the split would be cosmetic and
every other assertion would still pass), and asserts `stats() == stats_for(Nic)`
and that the total is never below its own rows.

**If a third source appears** (a second NIC, a bridge, a tun/tap device), add a
`Source` variant: the `match` in `group()`, `Source::name`, and
`check-variant-lists.py` on `Source::ALL` will between them force the storage,
the label and the readers to be updated. The one thing that will *not* prompt
you is `SYS_NET_STAT`'s selector — add the number there too.

**Original report follows.**

**In short:** the kernel counts network traffic in one set of counters and
labels them `eth0`. Traffic between two containers on a virtual cable — which
never reaches the network card — is added to the same counters. So a machine
that is busy only between its own containers reports a busy *NIC*, and there is
no way from the output to tell how much of it, if any, actually went out on the
wire.

`kernel/src/net/interface.rs` keeps six relaxed atomics — `TX_BYTES`,
`RX_BYTES`, `TX_PACKETS`, `RX_PACKETS`, `TX_ERRORS`, `RX_DROPS` — and exposes
them through `stats()`. They are fed from two genuinely different places:

| caller | what it is | what it should count as |
|---|---|---|
| `net::mod::poll` / `net::mod::send_frame*` (lines ~110, ~201–239) | frames in and out of the real NIC (virtio-net, e1000 or rtl8139) | the NIC's traffic |
| `net::veth::poll` (lines 577, 584) | frames drained from a virtual-ethernet endpoint, delivered inside the kernel | a veth pair's traffic |

`netstat -i` then prints `Interface: eth0` above the sum
(`net/netstat.rs:240`), and `netstat -s` prints the same numbers as "packets
received" / "packets sent" (lines 365–368). Neither says the total includes
frames that never touched the wire.

**Why it is not merely cosmetic.** These counters are what an operator uses to
answer "is the network the bottleneck?" A conflated total answers that question
wrongly in a specific direction — always *over*-reporting NIC load — and the
error grows with exactly the workload (containers talking to each other) where
the NIC is least involved. It also cannot be corrected after the fact, because
the two contributions are summed at record time and nothing retains the split.

**The proper fix** is per-source counters rather than a flag: give
`net::interface` a small set of counter groups keyed by source (the NIC, and
one per veth endpoint or at least a `veth` aggregate), have each recorder name
its source, and have `stats()` return the NIC's group. `netstat -i` should then
grow a row per source rather than one row labelled `eth0`. Do **not** fix it by
removing the `record_rx` calls from `veth::poll` — the veth traffic is real and
someone will want it; it is the *labelling* that is wrong, and deleting the
calls would replace an over-count with a silent under-count of the machine's
actual frame rate.

**Where it bites, beyond netstat.** `fs::netdev` projects these counters into
`/proc/netdev` as the row named `kernel` — deliberately *not* named `eth0`,
precisely because of this conflation; see that module's docs. When this is
fixed, `netdev::kernel_row` should project the NIC group and the name can
become `eth0` honestly.

**How it was found.** Reading `net::interface` while wiring the `/proc/netdev`
projection, to decide what the projected row could truthfully be called.

**Not a regression.** True since `veth::poll` was written.
