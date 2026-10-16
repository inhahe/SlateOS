### A-BLOCKED-FATAL-SIGNAL-KILLED-AT-ONCE -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a signal whose default action ends the process (`SIGTERM`,
`SIGHUP`), sent to a process with no handler trampoline while it had the
signal *blocked*, ended it on the spot. A blocked signal must wait, pending,
until it is unblocked. The blocked check was there for the stop signals and
missing for the fatal ones.

**Where:** `kernel/src/proc/signal.rs`, `classify_post_info`.

**Fixed:** a blocked fatal signal is kept pending; the syscall-return
checkpoint takes the default action once it is unblocked. Tested in
`test_ignored_set`.
