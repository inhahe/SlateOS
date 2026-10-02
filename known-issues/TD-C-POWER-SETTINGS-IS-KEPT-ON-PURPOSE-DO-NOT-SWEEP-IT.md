## TD-C-POWER-SETTINGS-IS-KEPT-ON-PURPOSE-DO-NOT-SWEEP-IT

**Date:** 2026-09-16. **Lane:** C.
**Where:** `gui/desktop/src/power_settings.rs` (1,855 lines).

**In short:** this file looks exactly like two others that were deleted today —
it is a settings screen nobody can open, it saves nothing, and no code refers
to it. It is being kept anyway. The two that went were duplicates of programs
that already work; this one is the only place several ideas are written down at
all, and the work that would make it real belongs to another team.

**What is genuinely only here.** Measured against the live `power.rs`:

| Concept | `power.rs` (live) | `power_settings.rs` |
|---|---|---|
| `PowerAction` | 61 mentions — live | 13 — duplicate |
| `BatteryInfo` | live, and honest | second copy |
| `PowerPlan` | absent | 19 — **only here** |
| `BatteryHealth` | absent | 29 — **only here** |
| `ChargeState` | absent | 23 — **only here** |
| `ChargeHistorySample` | absent | 4 — **only here** |

**Why it cannot simply be finished either.** Every unique thing in it needs
hardware this lane does not own. A power plan needs CPU frequency control;
battery health, charge state and charge history need a battery driver that
calls `register_source`. Both are lane A's. So it is neither deletable (the
knowledge has no other home) nor completable (the dependency is not ours) —
which is exactly the state the sibling entry kept `remote.rs` in.

**The live module is not the problem, and was checked rather than assumed.**
`power.rs` defaults to `present: false, state: NoBattery` and nothing populates
it from hardware, which looks like the fabrication design-decisions 856 is
about until you read the comment at `lib.rs:6874`: that is *the true answer*,
because `/proc/battery` reports zero sources until an ACPI driver registers
one, and the line that changes when it does is named. The disk meter beside it
does the same thing — `disk_fraction: None` with "the meter says so rather than
showing a plausible fraction of a number we do have". Absent data reported as
absent is the opposite of the defect.

**What would change this entry.** Lane A landing either CPU frequency control
or a battery source. At that point the *screens* go to `apps/settings` under
815 and this file goes with them — but until then, deleting it loses the only
written description of what those screens should do.
