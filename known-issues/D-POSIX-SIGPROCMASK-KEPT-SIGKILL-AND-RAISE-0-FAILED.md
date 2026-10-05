## D-POSIX-SIGPROCMASK-KEPT-SIGKILL-AND-RAISE-0-FAILED — `sigprocmask` put `SIGKILL` and `SIGSTOP` in the blocked mask and reported them blocked; `raise(0)` was an error (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/signal.rs`)**

**In short:** two small ways the signal functions answered differently
from what POSIX requires, found replaying glibc's answers for the old BSD
calls built on them. A program asking to block every signal was told that
`SIGKILL` and `SIGSTOP` -- which can never be blocked -- were blocked; and
`raise(0)`, which a program may use to check that it can signal itself,
failed where it should succeed.

- `sigprocmask`: POSIX -- "It is not possible to block those signals which
  cannot be ignored. This shall be enforced by the system without causing
  an error to be indicated." Both were kept in the mask this library
  stores and reports back (the kernel ignored them). Now dropped, by every
  `how`; `sigblock(sigmask(SIGKILL) | sigmask(SIGUSR1))` reports `SIGUSR1`
  alone, as glibc's does.
- `raise(0)`: POSIX makes `raise(sig)` `pthread_kill(pthread_self(), sig)`,
  and for that "if sig is zero, error checking shall be performed but no
  signal shall actually be sent". It was `EINVAL` -- two tests pinned
  that, on the premise that 0 is out of range -- and is 0 now, as glibc's.

**Where:** `posix/src/signal.rs`: `sigprocmask`, `raise`, and their tests.
