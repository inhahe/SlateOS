## TD-C-SYSINFO-INVENTS-A-WHOLE-MACHINE-WHILE-THE-REAL-QUERY-LAYER-SITS-UNUSED -- FIXED 2026-09-15

**In short:** the System Information app tells you your machine has a
GenuineIntel processor, an Intel I225-V network adapter, Intel Wi-Fi 6E AX211
and an AMD Radeon RX 7900 XTX. It is describing no machine in particular — the
values are written into `main.rs` as constants. Meanwhile a complete, layered
hardware query module sits in the same crate with **no callers at all**.

**Date:** 2026-09-15. **Lane:** C. First of the ten self-declared stubs on two
independent grounds, which is why it is first rather than merely early.

*Most harmful:* a system information tool is read precisely when someone wants
to know what hardware they have, so a plausible invention there is worse than
anywhere else on the list.

*Cheapest — and this was wrong when I wrote it; see the correction below:* it
is the only one of the ten whose real **client** already exists.
Checked rather than assumed — every other app on that list is a single
`main.rs` with no sibling module at all, so `devicemanager`, `netmanager`,
`partmanager`, `remotedesktop`, `vpnmanager`, `netscan`, `speedtest`,
`sysmonitor` and `videoplayer` each need a data *source* built before there is
anything to wire, and most of those are blocked on the OS not exposing the data
yet. `procexplorer` does have a second module, `features.rs`, but it is an
unreached *feature* set — a window picker, a blocking analyser, affinity
control — not a provider, so it belongs to the island ledger's "a widget
becomes reachable when an application draws it" category rather than to this
one.

So the ten are not one task repeated ten times. Nine are blocked on the system;
one is a wiring.

**The two halves.** `apps/sysinfo/src/main.rs` builds everything in its
constructor: `populate_cpu`, `populate_memory`, `populate_storage`,
`populate_network`, `populate_display`, `populate_pci`, `populate_services`,
`populate_processes`, `populate_drivers`, `populate_env_vars` and more, each
returning hardcoded values. `apps/sysinfo/src/hwquery.rs` is 2,152 lines
implementing a `HardwareProvider` trait with seventeen query methods, a
`SyscallProvider` that reads `/sys/hardware/*`, a `StubProvider`, a
`FallbackProvider`, and a `RefreshManager` with a TTL cache. `main.rs`
references `hwquery::` **zero times**. It is on the island ledger as
`apps/sysinfo/hwquery.rs`.

So this is one defect wearing two of this tree's recurring shapes at once: an
island, and a fabrication, each of which is the other's fix.

**The trap in the obvious wiring, which is why this is a note and not a
one-liner.** `FallbackProvider` tries the syscall provider and falls back to
`StubProvider` on error. Wiring `main` to it would compile, run, and display
exactly the same fictional machine — because the syscall path fails on any host
without `/sys/hardware`, which is every host today. That is the fabrication with
more steps and a longer call stack, and it would look like a fix.

**What the fix is.** Wire `main` to `SyscallProvider` directly, and render
`HwQueryError::NotAvailable { path }` as a row saying the value could not be
read and from where. The error type already carries the path, so the honest
message is available without inventing one. `StubProvider` becomes
`#[cfg(test)]` — it is a perfectly good fixture, and the same "a fixture
production can reach is a fixture that eventually ships" rule that moved
`ExifData::sample` applies. `FallbackProvider` goes: falling back to fabricated
data is the defect, not a feature.

The app's own types will need `Option` where a category can be absent, the same
move `apps/benchmark`'s `SubTestResult::score` needed today and for the same
reason: a zero-filled `CpuInfo` reads as a processor with no cores rather than
as an unanswered question.

**AND THE FIRST THING THE RECOUNT FOUND WAS MY OWN.**

`apps/benchmark` is on the module-level list — after being fixed. Its `//!`
doc still reads:

> simulated with representative computation; on real Slate OS hardware the
> stubs would be replaced with timed kernel/driver calls.

Thirteen of its sixteen tests measure the machine now and the other three say
why they do not. So the code was corrected and the documentation that described
the old behaviour was left, which is the same defect as the one being fixed,
pointing the other way: before, the source admitted a fabrication the window
denied; now the source claims a fabrication the code has stopped committing.

Both directions mislead the next reader, and the second is the one more likely
to survive — a doc that *understates* what the code does attracts no complaints
from anyone. Fixed in the same commit as this note.

The general form, for the checklist: **a fix is not finished until the prose
that described the defect has been re-read.** Every one of the six settings
pages got this right because rewriting the page forced the comment to be
rewritten with it. `benchmark` got it wrong because the stale claim lived in a
module header twelve hundred lines away from anything I edited.

**CORRECTION: `hwquery` is a client, and the interface it reads has no server.**

Lane B asked the right question — is the query layer dead because it is broken,
or dead because nobody connected it? — and the answer is neither. It is dead
because **the thing it reads does not exist**.

`SyscallProvider` reads `/sys/hardware/cpu`, `/sys/hardware/memory`,
`/sys/hardware/block`, `/sys/hardware/net` and so on. Grepping `kernel/`,
`services/` and `userspace/` for `sys/hardware` returns **nothing**. No
component in this tree has ever produced those files. The module is 2,152 lines
and 33 tests of a well-built client for an interface with no server, which is
the same defect as the fifteen private clipboards and the service with no
clients — inverted.

So my "nine are blocked on the system; one is a wiring" was wrong, and wrong in
the flattering direction: sysinfo is blocked too, just one layer further along
than the other nine. What it has that they lack is the *client* half already
written and tested, so when a producer appears the app needs no new parsing
code.

**The wiring is still worth doing now, and this is why.** Pointing `main` at
`SyscallProvider` today produces an application that says it cannot read the
hardware — which is true, and is better than one that says you own a Radeon RX
7900 XTX. It also means that on the day `/sys/hardware` gains a producer the
app starts working with no further change. The alternative, waiting, leaves the
invented machine on screen for the whole of that wait.

**What the producer needs**, for whoever writes it: the format is already
pinned by `hwquery`'s parser and its 33 tests — flat `key=value` files, one per
category, with the field names `SyscallProvider::field` looks up. A producer
written against those tests cannot disagree with the consumer, which is the one
piece of luck in this arrangement.

**FIXED, and the correction above needs one of its own.**

The application no longer invents anything: `main` queries
`hwquery::SyscallProvider` directly, 642 lines of constants are gone, and the
window says it cannot read the hardware. `hwquery` left the island ledger.
`FallbackProvider` was deleted rather than wired — it dropped to `StubProvider`
whenever the syscall path failed, which is every host without the tree, so its
purpose in practice was to display hardware nobody had.

**The `/sys/hardware` premise in the correction above was itself stale, and the
decision it contradicted is lane C's own.** `design-decisions.md` §850, dated
2026-09-14: hardware facts are served under `/sys/devices`, not a second
`/sys/hardware` tree, because the kernel already publishes `core_id`,
`physical_package_id` and cache geometry there. Lane A proposed that and
withdrew their own first choice; **lane C made the final call**, and lane C then
spent an hour writing a request asking lane A to build the tree §850 had
retired.

Two things in that request were wrong and the second was dangerous. It scoped
the change as "thirteen constants and one macro", when the trees differ in
*data model* — `/sys/devices` is scalar-per-file, `hwquery` read one
`key=value` file — so the reader changes, not the constants. And it said *"take
the tests as the specification, not my field list"*, which would have pointed a
producer author at 33 green tests pinning the very format the decision moved
away from. Written against them, a producer would have satisfied its consumer
perfectly, contradicted `main`, and passed everything.

**The general form, which lane A recorded as §937 in these words:** tests pin
the format the consumer currently parses, which is only the specification if the
format is not the thing under decision. Where a format has been decided
against, its tests are the strongest available argument for keeping it — green,
executable, and evidence of intent — and they are wrong. Pointing at them feels
like rigour rather than inertia, and thirty-three of them are harder to argue
with than one sentence in `design-decisions.md`.

**The reader is rewritten** for `/sys/devices`, scalar-per-file: `cpuid/` for
the CPUID leaf 1 identity, `present` for the logical count, `cpuN/topology/`
for distinct `(socket, core)` pairs, `cpu0/cache/indexN/` for geometry. Four
`CpuInfo` fields are `Option` and always `None` on this kernel — there is no
brand or vendor (CPUID leaves 0 and 0x8000_0002..4 are not served) and no
`cpufreq/` — and the panel says "Not reported by this system" rather than
drawing an empty name or a stopped clock.

**Still outstanding:** lane A serves `cpu` and `memory` today and has offered
`block` and `net` next. Everything else the window lists — PCI, USB, sound,
IRQs, I/O ports, the memory map, DMA — has no producer, and the window says so
per category rather than as one banner, which is right: those are separate
facts and will arrive separately.
