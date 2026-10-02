### [A] The power-management family: four modules that claim to act, actuate nothing, and publish it through `/proc` -- 2026-09-17

**Status:** OPEN

**In short:** the kernel has four modules for saving power -- device power
states, power profiles, an energy saver and a game mode. Between them they
promise to set the CPU governor, dim the display, throttle apps and suppress
notifications. None of them does any of it. They store the setting, show it
in `/proc`, and stop there, so the system reports itself in power-saver mode
while running exactly as before.

**Found by dd-950's own prescription**, on the day it was written: instead of
triaging `kernel/`'s 130 unread fields one at a time, group them and ask what
single absent consumer would read a cluster. Grouping by file put 12 of the
130 in four power modules.

| module | dead fields | hardware accesses | callers outside `/proc`, `kshell`, own self-test |
|---|---|---|---|
| `devpower` | 1 (`target_state`) | 0 | 0 |
| `fs/power` | 5 | 0 | 2 |
| `fs/energysaver` | 3 | 0 | **0** |
| `fs/powerprofile` | 3 | 0 | **0** |
| `fs/gamemode` | (3, counted separately) | 0 | **0** |

**The claims, against the call graph.** Each module doc describes active
control; none of them calls the subsystem it names, and those subsystems
exist in this tree:

| module says | calls |
|---|---|
| `powerprofile`: "control CPU governor, display brightness, suspend timing" | `cpufreq::` 0, `brightness::` 0 |
| `energysaver`: "app throttling, display dimming schedules" | `sched::` 0, `brightness::` 0 |
| `gamemode`: "suppresses notifications, blocks background tasks" | `sched::` 0 |
| `devpower`: "PCI config space PM capability for hardware control" | no PCI/MMIO/port access anywhere |

`cpufreq.rs` and `brightness.rs` are both present. So this is lane C's
phrasing exactly -- **unwired, not unimplementable.** The dead fields are the
shape of the policy application that was never connected, which is why
deleting them would be the wrong move (dd-950).

**Why it is one entry and not twelve.** Twelve unread fields across four
files reads as twelve oversights. It is one gap: nothing in this kernel
applies power policy. The count is a symptom whose magnitude carries no
information, which is the whole of dd-950.

**The dd-945 exposure is the part that actively misleads.** All four publish
through `/proc`, so a reader is told a profile is active, a device is asleep,
or energy saving is on, and none of it is true. `devpower`'s `/proc` header
now discloses that its states are modelled; the other three do not yet, and
that is the next bounded piece of work here. A disclosure at the point of
reading is what dd-945 requires, and it is cheap; wiring the actuation is a
real feature and is not claimed to be in scope.
