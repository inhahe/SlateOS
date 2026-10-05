## D-EXEC-CLEARS-THE-SIGNAL-MASK — a program started by `exec` begins with no signals blocked, whatever the program that exec'd it had blocked; POSIX and Linux keep the mask (lane D, 2026-09-30) — **Status: OPEN (waiting on lane A)**

**In short:** shells and supervisors block a signal -- `SIGCHLD`, `SIGINT`
-- around starting a command, and the command is meant to start with it
still blocked until it says otherwise. Here every program that `exec`
starts, native or Linux, starts with nothing blocked, so a signal the
parent meant to hold back reaches the new program's default action at once.

**Why:** `proc::signal::on_exec` (`kernel/src/proc/signal.rs`) sets
`state.blocked = 0` under a comment saying POSIX asks for it; POSIX, and
Linux, keep the mask across `exec` and reset only the dispositions.

**The proper fix:** keep `blocked` in `on_exec` -- requested of lane A in
`requests/d-a-exec-must-keep-the-signal-mask.md`. libc's half is done: a
native image takes the mask it inherited at start-up (`init_signals`,
`adopt_inherited_mask`), which reads 0 until the kernel keeps it.

**Where:** `kernel/src/proc/signal.rs` (`on_exec`); `posix/src/signal.rs`
(`init_signals`).
