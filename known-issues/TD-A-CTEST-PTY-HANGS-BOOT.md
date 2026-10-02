## TD-A-CTEST-PTY-HANGS-BOOT — PtySlave reads dispatch through SYS_TTY_READ which targets the console, not the pty (lane A, 2026-09-08) — KERNEL SIDE FIXED

**Root cause.** `posix/src/file.rs` routes `HandleKind::PtySlave` reads
through `SYS_TTY_READ(buf, count)`, which calls `tty_read_into_user` with
`current_tty()` — hardcoded to the console.  The console's default termios
is canonical (`ICANON`, `VMIN=1`), so `canonical_read()` blocks forever
waiting for keyboard input.  The pty slave's own termios (set to raw by
the `ctest-pty` fixture) is never consulted.

This is the missing `SYS_PTY_SLAVE_READ` half of an asymmetry: the master
side has had `SYS_PTY_MASTER_READ` (546) and `SYS_PTY_MASTER_TRY_READ`
(547) since the pty was built; the slave side has `SYS_PTY_SLAVE_WRITE`
(548) but no read.

**Fix (kernel, done).** `SYS_PTY_SLAVE_READ` (872) and
`SYS_PTY_SLAVE_TRY_READ` (873) added.  Both use `resolve_tty_arg` to
target the correct pty and call `tty::read` / `tty::try_read`.

**Fix (POSIX, done -- lane B, 2026-09-09).** `posix/src/file.rs`'s PtySlave
read arm now reads `fdtable::get_status_flags(fd)` and dispatches
`SYS_PTY_SLAVE_TRY_READ` (873) when `O_NONBLOCK` is set and
`SYS_PTY_SLAVE_READ` (872) otherwise, passing `entry.handle` as `arg0` -- the
shape the neighbouring `SYS_PTY_SLAVE_WRITE` arm already used. Constants added
to `posix/src/syscall.rs` and pinned in its number-assertion test.

**The rung is still disabled** in `kernel/src/main.rs` and needs re-enabling by
lane A for any of this to be worth anything; asked for in the request file.

**Why lane B's "cannot hang" guarantee did not hold**, recorded because the
shape recurs. The fixture set `O_NONBLOCK` and bounded every retry loop, and
both of those were true. But `O_NONBLOCK` is a flag in libc's own descriptor
table that each read arm must *consult* and turn into a `TRY_` syscall. The
slave arm could not consult it -- `SYS_TTY_READ` takes no handle, so there was
no descriptor whose flags it could honour. The flag was set, read by nobody,
and dropped. A guarantee resting on a flag is only as strong as the narrowest
path that flag has to survive, and "I set the flag" is a different claim from
"the flag is honoured" -- only the second one bounds anything.

**It hung at check 14**, the first slave read. The fixture's own exit-code
legend would have named it in one line had it been able to fail instead of
hang; that, rather than the bug, is what the wrong claim cost.

**The "scheduler doesn't preempt" concern was a false alarm.**
`schedule_inner` correctly returns without context-switching when the
spinning task is the only runnable one (`picked_id == current_id`).  The
liveness check's "zero context switches" is technically correct but
misleading — the scheduler *is* running, it just picks the same task
because nothing else is runnable.

**Where.** `kernel/src/syscall/number.rs` (872–873),
`kernel/src/syscall/handlers.rs` (`sys_pty_slave_read`,
`sys_pty_slave_try_read`), `kernel/src/tty/mod.rs` (`try_read`,
`canonical_try_read`, `raw_try_read`).
