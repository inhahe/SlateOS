### [A] `devpower` reports device power states it never applies, and `/proc` published them undisclosed -- 2026-09-17

**Status:** OPEN

**In short:** a kernel module says it manages the power state of PCI
devices. It keeps a table of which device is in which power state, shows
that table in `/proc`, and never writes a single power register. So a reader
is told a device is asleep while it is awake and drawing full power. Nothing
said the numbers were make-believe.

**How it was found: one dead field.** `target_state` (devpower.rs:187) is
assigned at 397 and 479 and read nowhere -- one of the 130 flagged when lane
C's field gate was pointed at `kernel/` for the first time (entry above).
The field turned out to be the least of it.

**The measurement.** `devpower.rs` contains **zero** hardware accesses: no
`outl`/`outb`/`inl`/`inb`, no `write_volatile`/`read_volatile`, no `pci::`.
Its module doc describes a working subsystem, and all four of its stated
integrations are absent:

| the doc claims | the tree says |
|---|---|
| `crate::power` coordinates system sleep/wake | `power.rs` never calls `devpower::` at all |
| `crate::udriver` notified to save/restore state | only `DeviceAddr` is imported, as a type |
| `crate::devhotplug` emits power-change events | never called |
| PCI config space PM capability drives hardware | no PCI, MMIO or port access anywhere |

The entire caller set is `fs/procfs.rs` (`procfs_content`), `kshell.rs`
(`procfs_content`, `stats`, `all_devices`, `system_suspend`,
`system_resume`, `self_test`) and `main.rs` (`self_test`). So
`system_suspend`/`system_resume` are reachable only from an interactive
debug shell, never from the real system power path.

**The author knew, and nothing tracked it.** `set_state` carries "For now,
record the state transition immediately" above an inline comment listing the
PMCSR write-and-settle sequence that is not implemented. `devpower` appeared
in neither `todo.txt` nor `known-issues.md` -- a knowingly incomplete
subsystem with no tracking entry, which `CLAUDE.md` asks for explicitly.

**Why this is dd-945 and not merely unfinished.** `procfs_content` is wired
into `/proc`. dd-945's rule is that a simulated action must be disclosed
*where its result is read*, not where the code is written -- and the read is
a `/proc` file that disclosed nothing. A fabricated action, per dd-945's own
distinction, cannot be un-said by deleting the fabrication later; what it
needs is the disclosure at the point of consumption.

**Fixed.** The `/proc` header now says the states are modelled and no power
register is written, so the file cannot be mistaken for a working power
manager. The module doc's "Integration" list is replaced by an
intended-vs-today table, because a doc listing integrations reads as a claim
that they exist, and it is the first thing anyone extending the module sees.

**Deliberately NOT done: `target_state` stays.** It is scaffolding for the
asynchronous transition the PMCSR sequence needs, where `current != target`
while a transition is in flight. Removing it would delete the shape of the
real fix, which is the opposite of useful. The field was the thread, not the
defect -- worth recording as a correction to "130 presumptively dead": some
of that 130 is scaffolding whose real problem is untracked incompleteness,
not dead state.
