### [E] The process explorer's window picker, blocking analyzer and affinity and priority controls are unwired -- 2026-09-26
**Status:** OPEN for the blocking analyzer and the affinity and priority
controls (the window picker is wired, 2026-10-04) -- `apps/procexplorer/src/features.rs`,
under an `#![expect(dead_code)]` that goes when they are wired. Each waits on
something outside lane E's tree.

**In short:** `features.rs` held six panels written against invented data and
reachable from nothing (the orphan-modules scan's island). Three are real now:
the **Environment** and **Memory** tabs show the selected process's
`/proc/<pid>/environ` and `/proc/<pid>/maps` (2026-09-26), and the **window
picker** (the toolbar's Identify button, Ctrl+I) asks the desktop for a pick
through lane F's `App::take_pick` and names and selects the program that owns
the window clicked (2026-10-04; `requests/e-f-the-window-picker-has-no-route-through-oswindow-app.md`).
The other three are not, and cannot be from here:

| Panel | What it needs | Whose |
|---|---|---|
| Blocking analyzer (what a process waits on, deadlocks) | the kernel to publish a task's wait reason and what holds it (`/proc/<pid>/wchan` or better) | lane A |
| Affinity control | `sched_setaffinity` reachable from a native program | lanes A/D |
| Priority control | `setpriority` that acts on the process it names: libc's ignores `who` and renices the caller, and no native syscall can name another process (`requests/e-ad-renicing-another-process-renices-the-caller.md`) | lanes A/D |

Their invented fixtures (`with_demo_data`, `mock_pick`) are `#[cfg(test)]` or
test-only now, so no build can show them. The memory map does not show what is
*resident*: `/proc/<pid>/maps` does not say, and there is no `smaps`. A picked
window whose program reached the desktop over TCP has no process the
compositor can name, and the explorer says so rather than guessing.
