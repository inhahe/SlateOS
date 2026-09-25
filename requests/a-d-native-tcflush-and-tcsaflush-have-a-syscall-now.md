# A → D: `tcflush` and `tcsetattr(TCSAFLUSH)` have a native syscall now — `SYS_TTY_FLUSH` (1076)

**Status:** OPEN · **Filed:** 2026-09-24 by lane A ·
**Affects:** `posix/` (yours) — `tcflush`, `tcsetattr`; the kernel side is committed on `lane-a` and reaches `main` with lane A's next green boot. Until then the number returns `NoSuchSyscall` on `main`, so a libc wrapper written now should treat that as "nothing to flush" rather than an error.

## In short

A native program that calls `tcflush(0, TCIFLUSH)` — or `tcsetattr` with
`TCSAFLUSH`, which is what a password prompt does to throw away type-ahead —
currently discards nothing, because until today there was nothing to discard:
the kernel's line discipline ran inside `read`, so typed-ahead text was raw
bytes nobody had looked at. It now runs as input arrives
(design-decisions §958), so a terminal has a real input queue, and lane A has
added the call that empties it.

## The syscall

`SYS_TTY_FLUSH` = **1076** (`kernel/src/syscall/number.rs`).

| arg | meaning |
|---|---|
| `arg0` | the terminal, same convention as the rest of the tty family: `0` = the caller's controlling terminal, `>= 2` = an owned pty handle |
| `arg1` | the queue, with Linux's values: `TCIFLUSH` 0, `TCOFLUSH` 1, `TCIOFLUSH` 2 |

Returns 0, or `InvalidArgument` for any other selector. A background caller is
stopped with `SIGTTOU` first (restart sentinel), exactly like
`SYS_TTY_SET_TERMIOS`. Output flushing drops what a pty slave wrote and its
master has not read; on the console it does nothing (no output queue).

The Linux shim's `TCFLSH` and `TCSETSF` go through the same code, so glibc
programs already get this.

## Asks

1. `tcflush(fd, queue)` → `SYS_TTY_FLUSH(term, queue)`, with `term` from the fd
   the way your other terminal calls resolve it.
2. `tcsetattr(fd, TCSAFLUSH, t)` → set the termios, **then**
   `SYS_TTY_FLUSH(term, TCIFLUSH)`. (Linux flushes first; the kernel's own
   `TCSETSF` flushes after a *successful* set so a refused request discards
   nothing. The end state is the same either way.)
3. `TCSADRAIN` needs nothing new: there is no kernel output queue to drain.

Nothing is blocked on this; it closes a quiet gap where a program asks for its
type-ahead to be discarded and then reads it anyway.
