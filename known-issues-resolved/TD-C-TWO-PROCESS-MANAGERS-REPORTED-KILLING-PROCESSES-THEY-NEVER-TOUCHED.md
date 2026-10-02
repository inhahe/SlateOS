## TD-C-TWO-PROCESS-MANAGERS-REPORTED-KILLING-PROCESSES-THEY-NEVER-TOUCHED -- FIXED 2026-09-15

**In short:** the process explorer and the system monitor each had Kill, Pause
and Resume buttons. Pressing Kill said "Killed process firefox (PID 4821)" and
removed the row from the list. Nothing was signalled — neither program can send
a signal at all. The process explorer additionally opened on six invented
processes and an invented 8 GiB machine, even though it had been reading the
real `/proc` since 2026-09-13.

**Date:** 2026-09-15. **Lane:** C. Found by
`scripts/find-claimed-acts.py`, written the same afternoon for this class.

**Why this is the worst form the defect takes.** The claim was not merely
false; **the program then made it come true in the display.** Kill removed the
row, Pause set the row to Stopped. So the window agreed with its own status
line — the process vanished from the list exactly as it would have if it had
died — and *nothing inside the program could tell the user otherwise.* Every
other fabrication in this sweep can in principle be caught by looking harder at
the screen. This one cannot.

What believing it costs: a person who thinks a runaway process is dead stops
trying to kill it. The machine stays slow, and the reason has been removed
from the list of suspects.

| control | claimed | did |
|---|---|---|
| `procexplorer` Kill (toolbar + menu) | "Killed process X (PID n)" | removed the row |
| `procexplorer` Pause / Resume (both routes) | "Paused X" / "Resumed X" | set the row's status |
| `sysmonitor` Kill (toolbar + menu) | "Killed process X (PID n)" | removed the row |
| `sysmonitor` Stop / Continue | "Stopped X" / "Resumed X" | set the row's status |

**Two routes each, and that is the part worth remembering.** Both apps had the
toolbar action *and* a right-click menu action, implemented separately, saying
the same false thing two hundred lines apart. Repairing `kill_selected` alone
left `ContextAction::Kill` untouched in both — and I did exactly that, and the
scanner caught it on the re-run. **A defect found by reading one call site is
a defect half-fixed by default.**

**The tests were the other half, and they differed instructively.**
`procexplorer` had *no* test for any of the three controls, which is part of
why the claim survived. `sysmonitor` had four, and they were worse than none:

```rust
s.kill_selected();
assert_eq!(s.processes.len(), initial - 1);
```

That assertion **pins the fabrication.** It does not test that a process was
killed — nothing could, from in here — it tests that the row was removed, which
is precisely the mechanism that made the lie self-consistent. A test over a
claim nothing performs makes the defect look deliberate and protects it from
the next reader. A fifth test, `test_continue_selected`, then passed
*vacuously* once the repair landed: it stopped and continued a process that was
already Running and asserted it was Running, so it would have held against two
methods that did nothing whatever.

**`procexplorer`'s invented machine, and the shape of how it survived.** Two
changes, each defensible alone:

* `refresh()` reads the real `/proc` through `procinfo`, and **keeps the
  previous list when it cannot read** — right, because an explorer that empties
  itself when `/proc` is briefly unavailable is worse than one that holds.
* `main()` called `load_demo_data()` first, under a comment reading *"Until a
  real process source exists this is what there is to show"*.

Together: on every host without `/proc`, the invented list loaded first and the
real reader then declined to replace it. **The careful reader was shadowed at
startup by the thing it replaced**, and the comment saying why had been false
for two days. `load_demo_data` set more than processes — 8 GiB of memory, 33%
CPU, load `[1.23, 0.98, 0.87]`, 86472 seconds of uptime, and a table of network
connections. An entire plausible machine, none of it this one.

Startup now calls `refresh()`; the demo builders are `#[cfg(test)]`; memory,
load and uptime are read from the same `ProcFs`; and when `/proc` cannot be
read at all the status bar says so rather than leaving a window that looks like
a working one.

**What is still missing, and whose it is.** Sending a signal needs `kill(2)`,
which is stateful and therefore reachable only through the C ABI per
`design-decisions.md` §768 — `posix::signal::kill` as a Rust dependency is the
route that looks right and resolves to a stub answering `-ENOSYS`. That makes
it `libcall`'s to expose and `libcall` is lane B's:
`requests/c-b-a-process-manager-needs-a-way-to-send-a-signal.md`. Until it is
answered the three controls are honest and inert, which is the correct state
for a control that cannot act.

`/proc/net/tcp` is likewise parsed by nothing in this tree, so both Network
tabs now show an empty list rather than invented sockets.

**Still open here:** `sysmonitor` has no `procinfo` dependency and still
invents its process list and system figures from `load_demo_data` at startup.
Its *claims* are repaired; its *data* is not. That is the next commit, and it
is the same edit `procexplorer` just took.

**Verified by sabotage**, five claims, each broken with an edit that still
compiles: the kill claimed again, the row removed to match it, an unreadable
`/proc` passed over in silence, and the system figures left unread. All five
went red.
