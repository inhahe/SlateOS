### A-IGNORED-SIGNALS-DID-NOT-SURVIVE-EXEC-OR-SPAWN -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** "ignore this signal" was known only to the C library, so a
program started by another forgot it: `nohup cmd` ignores the hang-up signal
and becomes `cmd`, and `cmd` died when its terminal closed. A parent that
ignored `SIGCHLD` still collected zombies, and a process with no signal
handler yet (before its libc started, or one with no libc) was killed by an
ignored `SIGHUP` -- the kernel took the default action. Lane D's
`requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`.

**Fixed:** the kernel keeps the ignored set (`proc::signal`), for both ABIs,
across `exec`, `fork` and spawn, and discards an ignored signal when it is
sent; a parent ignoring `SIGCHLD` (or with `SA_NOCLDWAIT`) leaves no zombies
(`pcb::ExitNotice`). New native calls `SYS_SIGNAL_SET_IGNORED` (1098) and
`SYS_SIGNAL_GET_IGNORED` (1099). Design-decisions §1512. Lane D's half --
reporting each change to or from `SIG_IGN`, seeding the table at start-up --
is theirs to do.

`/proc/<pid>/status` now prints Linux's `SigQ`/`SigPnd`/`ShdPnd`/`SigBlk`/`SigIgn`/`SigCgt` lines and `/proc/<pid>/stat` fields 31-34 are real. **Still short of Linux:** `SigCgt` (and field 34) is known only for a Linux-ABI process -- a native process's handlers are its libc's, so it reports none caught.

Tests: `proc::signal`'s `test_ignored_set` and `test_ignored_across_images`;
`pcb`'s `test_exit_notice`; `spawn`'s
`test_spawn_child_of_sigchld_ignorer_is_reaped`, through a real exit;
`dispatch`'s `test_dispatch_signal_ignored` (the two calls, and an ignored
`SIGHUP` that no longer kills) and the ignored case of
`test_dispatch_tty_job_control`; the Linux table's
`self_test_sigaction_table`.
