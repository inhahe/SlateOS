### A-SIGCHLD-IS-NOT-POSTED-WHEN-A-CHILD-STOPS-OR-CONTINUES -- 2026-10-01 -- OPEN for native parents (lane A, waiting on lane D's answer)

**In short:** when a child stops or continues, a *native* parent is not sent
`SIGCHLD`. A parent that waits with `waitpid(..., WUNTRACED)` or
`WCONTINUED` still sees the change. A native parent that learns about its
children from its `SIGCHLD` handler sees only exits, so a job stopped with
`^Z` is not noticed until the parent next waits. A Linux-ABI parent has been
sent it since 2026-10-08, as Linux sends it: `CLD_STOPPED` or
`CLD_CONTINUED`, unless its `SIGCHLD` is ignored or has `SA_NOCLDSTOP`.

**Where.** `syscall::handlers::notify_parent_of_job_control`, called by
`stop_process_for_signal` and `continue_process` after the change is
recorded (`pcb::record_jc_stopped`, `record_jc_continued`). It returns early
for a parent that is not Linux-ABI. The exit path in `proc::thread` posts to
both kinds of parent.

**Why it waits.** `SA_NOCLDSTOP` is the parent's choice, and it lives in
different places for the two ABIs:
- For a Linux-ABI parent it is in the kernel's disposition table, so the
  kernel honours it (`syscall::linux::linux_wants_cldstop`).
- For a native parent it is in libc. libc can filter only once it reads
  `si_code` from the extended frame
  (`requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`). Before
  that, it cannot tell a stop from an exit.

The reply on that request (item 3) asks lane D whether the kernel should post
to native parents and leave `SA_NOCLDSTOP` to libc.

**The proper fix,** once lane D says yes: in
`notify_parent_of_job_control`, post to a native parent through
`signal::classify_post_info`, so that a parent with no handler drops it, and
let libc keep it back under `SA_NOCLDSTOP`.

**Reproduce.** A native parent with a `SIGCHLD` handler that counts calls
forks a child that runs `raise(SIGSTOP)`. On Linux the handler runs once,
with `CLD_STOPPED`; here it does not run. The Linux-ABI version of this is
`build/sigchldstoptest.c` (`spawn::self_test_linux_sigchld_stop`), which
passes.
