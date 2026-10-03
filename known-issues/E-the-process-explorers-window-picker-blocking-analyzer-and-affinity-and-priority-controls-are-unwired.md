### [E] The process explorer's window picker, blocking analyzer and affinity and priority controls are unwired -- 2026-09-26
**Status:** OPEN -- `apps/procexplorer/src/features.rs`, under an
`#![expect(dead_code)]` that goes when they are wired. Each waits on something
outside lane E's tree.

**In short:** `features.rs` held six panels written against invented data and
reachable from nothing (the orphan-modules scan's island). Two are real now:
the **Environment** and **Memory** tabs show the selected process's
`/proc/<pid>/environ` and `/proc/<pid>/maps` (2026-09-26). The other four are
not, and cannot be from here:

| Panel | What it needs | Whose |
|---|---|---|
| Window picker ("which process owns this window?") | the compositor to report the window under the pointer and its owner | lane F |
| Blocking analyzer (what a process waits on, deadlocks) | the kernel to publish a task's wait reason and what holds it (`/proc/<pid>/wchan` or better) | lane A |
| Affinity control | `sched_setaffinity` reachable from a native program | lanes A/D |
| Priority control | `setpriority` that acts on the process it names: libc's ignores `who` and renices the caller, and no native syscall can name another process (`requests/e-ad-renicing-another-process-renices-the-caller.md`) | lanes A/D |

Their invented fixtures (`with_demo_data`, `mock_pick`) are `#[cfg(test)]` or
test-only now, so no build can show them. The memory map does not show what is
*resident*: `/proc/<pid>/maps` does not say, and there is no `smaps`.
