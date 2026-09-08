# A → B — `SYS_PTY_SLAVE_READ` (872) and `SYS_PTY_SLAVE_TRY_READ` (873) now exist; route PtySlave reads through them

**From:** Lane A.  **To:** Lane B.  **Filed:** 2026-09-08.  **Status:** open.  
**Blocking:** ctest-pty fixture (disabled rung in `kernel/src/main.rs`).

## In short

PtySlave reads in `posix/src/file.rs` currently dispatch through
`SYS_TTY_READ(buf, count)`, which reads from `current_tty()` — the console.
When the caller's controlling terminal is not the pty slave (e.g. a process
that called `openpty` but not yet `login_tty`), the read targets the wrong
device.  The console's default termios is canonical with `VMIN=1`, so
`canonical_read()` blocks forever on keyboard input that never arrives.  This
is the root cause of the `ctest-pty` hang.

## What Lane A did

Added two new kernel syscalls that mirror `SYS_PTY_MASTER_READ` (546) /
`SYS_PTY_MASTER_TRY_READ` (547) for the slave side:

| Syscall | Number | Description |
|---|---|---|
| `SYS_PTY_SLAVE_READ` | 872 | Blocking read from a pty slave |
| `SYS_PTY_SLAVE_TRY_READ` | 873 | Non-blocking variant (returns `WouldBlock`/`EAGAIN`) |

Both take `(arg0: tty, arg1: buf, arg2: count)` under the `resolve_tty_arg`
convention: `0` = the caller's controlling terminal, `>= 2` = an owned pty
handle.  They call `tty::read()` / `tty::try_read()` on the resolved
terminal, which uses the pty's own termios — not the console's.

The ownership gate self-test already covers them (unowned handle → refused).

## What Lane B needs to do

In `posix/src/file.rs`, the `HandleKind::PtySlave` read dispatch (around
line 535–551) currently does:

```rust
syscall2(SYS_TTY_READ, buf as u64, count as u64)
```

Change it to:

```rust
// For O_NONBLOCK:
syscall3(SYS_PTY_SLAVE_TRY_READ, handle_or_0, buf as u64, count as u64)
// For blocking:
syscall3(SYS_PTY_SLAVE_READ, handle_or_0, buf as u64, count as u64)
```

following the same O_NONBLOCK → try / blocking → normal pattern that
`HandleKind::PtyMaster` reads already use (lines 509–533).

The `handle_or_0` argument: if the POSIX fd table entry for a PtySlave
carries the pty handle, pass it.  If it only carries the kind and the
process's controlling terminal is the slave (which it always is after
`login_tty`), passing `0` works — `resolve_tty_arg(0)` returns
`current_tty()`, which after `login_tty` is the pty slave.  Either is
correct; the handle form is more precise.

The `SYS_PTY_SLAVE_TRY_READ` returns `WouldBlock` → `EAGAIN` when no data
is available, matching the existing `SYS_PTY_MASTER_TRY_READ` behaviour.

## What this unblocks

Once the routing is in place, the `ctest-pty` boot rung can be re-enabled
(the comment in `kernel/src/main.rs` documents this dependency).  The
fixture's PtySlave reads will target the correct pty device, canonical mode
will use the pty's own termios (set to raw by the fixture), and the ^C
signal-delivery test can run.

## Technical debt this closes

The asymmetry in the pty syscall family — master has READ/TRY_READ, slave
only had WRITE — is now gone.  `TD-B-PTY-SLAVE-READ-IS-CTTY-ONLY` (if
tracked anywhere) is resolved on the kernel side.
