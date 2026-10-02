### A-SIGCHLD-IS-NOT-POSTED-WHEN-A-CHILD-STOPS-OR-CONTINUES -- 2026-10-01 -- OPEN (lane A, waiting on lane D's answer)

**In short:** when a child stops or continues, the parent is not sent
`SIGCHLD`. A parent that waits with `waitpid(..., WUNTRACED)` or
`WCONTINUED` still sees it. A parent that learns about its children from its
`SIGCHLD` handler sees only exits, so a job stopped with `^Z` is not
noticed until the parent next waits. Linux sends `SIGCHLD` with
`CLD_STOPPED` or `CLD_CONTINUED`, unless the parent asked not to
(`SA_NOCLDSTOP`).

**Where.** `syscall::handlers::stop_process_for_signal` and
`continue_process` record the change (`pcb::record_jc_stopped`,
`record_jc_continued`) and wake waiters. Neither posts anything to the
parent. The exit path in `proc::thread` does post. The codes are ready in
`pcb::JobControlEvent::sigchld_code_and_status`.

**Why it waits.** `SA_NOCLDSTOP` is the parent's choice, and it lives in
different places for the two ABIs:
- For a Linux-ABI parent it is in the kernel's disposition table, so the
  kernel can honour it.
- For a native parent it is in libc. libc can filter only once it reads
  `si_code` from the extended frame
  (`requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`). Before
  that, it cannot tell a stop from an exit.

The reply on that request asks lane D whether the kernel should post to
native parents and leave `SA_NOCLDSTOP` to libc.

**The proper fix.** After the record in each place:
- post `SigInfo::child(pid, uid, event.sigchld_code_and_status())` to the
  parent;
- for a Linux-ABI parent, skip the post when its `SIGCHLD` disposition has
  `SA_NOCLDSTOP`;
- for a native parent, post through `classify_post_info`, so that a parent
  with no handler drops it.

**Reproduce.** A parent with a `SIGCHLD` handler that counts calls forks a
child that runs `raise(SIGSTOP)`. On Linux the handler runs once, with
`CLD_STOPPED`; here it does not run.
