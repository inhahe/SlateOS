### A-REAPED-PROCESS-KEPT-ITS-FILE-MAPPINGS-AND-TERMINAL -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a file mapped into a process (`mmap` of a file -- a
dynamically linked program maps every library it loads) holds a reference on
the open file. When the process's parent collected it with `wait`, those
references were never dropped, so the file's last close never came: an
unlinked file's space was not freed, and a `flock` held through such a file
was never released. The session's claim on its terminal was likewise kept
when the session ended by `wait`. Only the other way a process ends,
`pcb::destroy`, released either.

**Where:** `kernel/src/proc/pcb.rs`, `try_reap`.

**Fixed:** `destroy`, `try_reap` and the new `release_autoreaped` end a
process through one function, `finish_process`.

**Still different from Linux:** Linux drops a process's mappings when it
exits (`exit_mm`), not when it is reaped; here a zombie keeps its file
references until its parent waits.
