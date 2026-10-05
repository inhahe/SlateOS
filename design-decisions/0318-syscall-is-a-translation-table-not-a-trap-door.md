## §318 — `syscall()` is a translation table, not a trap door

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** Unix has a function called `syscall()` that lets a program ask the
kernel for something directly, by number, bypassing the C library. CPython uses
it for a handful of things its libc might be too old to expose. The problem is
that the numbers are *Linux's* numbers, and SlateOS's own numbers are unrelated
— they happen to overlap, so forwarding one to the other would not error out,
it would call the wrong thing. Our `syscall()` therefore translates a small,
fixed set of Linux numbers into calls on functions in this crate, and refuses
everything else.

**The collision is not hypothetical.** SlateOS's `SYS_EXIT` is **1**, which is
Linux's `write`. A naive `syscall(n, …) → SYSCALL n` forwarder handed
`SYS_WRITE` (Linux 1) would have exited the process. A test now pins this:
`test_linux_numbers_are_linux_numbers` asserts both that our `SYS_EXIT` differs
from `crate::syscall::SYS_EXIT` and that `crate::syscall::SYS_EXIT` equals
Linux's `SYS_WRITE`, so the overlap cannot silently drift back.

**Three options were on the table.**

| | *What changes:* |
|---|---|
| Forward to the kernel | wrong function called, silently — rejected outright |
| Translate every Linux number | a second, parallel libc, maintained forever |
| Translate the numbers real callers issue; `ENOSYS` for the rest | `syscall(SYS_gettid)` works; `syscall(SYS_write, …)` fails cleanly |

We took the third. The table covers what CPython actually issues —
`sched_yield`, `getpid`, `getppid`, `gettid`, `get{,e}{u,g}id`,
`gettimeofday`, `getrandom`, `pidfd_open`, `pidfd_send_signal` — every one of
which is a thin query with an existing crate function behind it.

**Why the libc-bypass numbers are *deliberately* absent.** `read`, `write`,
`close`, `fork`, `execve` and `exit` are not missing by oversight; wrapping
them would be actively wrong. A caller reaching those through `syscall()` is
bypassing libc *on purpose* — to avoid our fd table, our `pthread_atfork`
handlers, our CWD resolution — and quietly routing it back through the very
layer it is evading would produce behaviour that matches neither what the
caller asked for nor what Linux does. `ENOSYS` is the honest answer, and
`test_bypass_syscalls_are_not_silently_wrapped` pins it.

**Against.** A program that legitimately needs a number we have not tabulated
gets `ENOSYS` and must be diagnosed, where a full forwarder would "just work"
— except that it would not; it would call the wrong function. The real cost is
that the table needs extending as new programs land, which is a small, visible,
test-covered edit rather than a hidden hazard.

**One implementation note worth keeping.** The seven arguments are typed
`isize`, **not** `core::ffi::c_long`. `c_long` is 32 bits on the LLP64 host the
tests run on and 64 bits on `x86_64-slateos` — declaring the signature with it
produces a *different function* in the tested build than in the shipped one,
truncating every pointer argument. The first compile caught it, and the type
alias carries the explanation so it cannot recur silently. Seven fixed
arguments rather than a variadic is ABI-identical on x86-64 SysV and avoids a
feature gate the crate does not enable.

**Where it lives.** `posix/src/sys_syscall.rs`, rewritten in `5531f816c`; the
old header claimed its `SYS_*` constants "map Linux syscall names to our native
syscall numbers" (they are verbatim Linux numbers) and globbed in
`crate::syscall::*`, which is what forced the `SYS_EXIT_LINUX` alias. Both are
gone; nothing consumed either.
