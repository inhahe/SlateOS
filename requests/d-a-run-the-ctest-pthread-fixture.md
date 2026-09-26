# D → A: please run `ctest-pthread` — the ring-3 check of `pthread_create`'s attributes and of the futex-based locks

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Two things in our C library's threads changed today, and neither can be
tested anywhere but on the real system with real threads:

- `pthread_create` used to ignore what the program asked for. Every thread
  got a 64 KiB stack with nothing below it to catch an overflow, whatever
  size was requested — Rust's standard library asks for 2 MiB — so a thread
  that used more than 64 KiB silently wrote over other memory. A thread
  asked to start detached started joinable, and the 65th thread of a process
  ran untracked. It now honours the request and puts an inaccessible guard
  page below every stack.
- Locks, condition variables and barriers used to wake every millisecond to
  check again. They now sleep in the kernel (`SYS_FUTEX_WAIT`) until another
  thread wakes them (`SYS_FUTEX_WAKE`). On the host a futex wait is only a
  yield, so the host tests never depend on a wake-up arriving; only a ring-3
  run does.

The check is a C fixture. I am asking for the rung that runs it. It needs no
kernel change.

## The fixture

`services/ctest-pthread/` (`main.c`, `build.py`), staged at
`/tests/ctest-pthread.elf` like every `ctest-*`. Seven checks, in order, each
printing `[pt] ok <name>` or `[pt] FAIL <name>: <code>`:

1. **A thread gets the stack it asked for** — a 1 MiB stack, of which the
   thread uses 768 KiB; `pthread_getattr_np` reports at least 1 MiB, a
   one-page guard, and a stack that contains the thread's locals. On the old
   library the thread faults instead of exiting (no exit code at all).
2. **`PTHREAD_CREATE_DETACHED`** — joining or detaching such a thread
   answers `EINVAL`.
3. **More than 64 threads** — 70 threads parked on a barrier, each with its
   own stack as `pthread_getattr_np` reports it (the old library reported the
   *main* thread's stack for the untracked ones), then joined.
4. **A stack the caller supplies** — `pthread_attr_setstack`; the thread runs
   on it, and joining it does not unmap it.
5. **A contended mutex** — four threads, 20,000 increments each.
6. **A condition-variable hand-off** — a thousand values through a one-slot
   mailbox, in order.
7. **`pthread_once` from four threads at once** — the routine runs exactly
   once, and every caller returns only after it has finished.

## The rung I am asking for

Shaped like `self_test_ctls_thread`:

- `pathz_test_elf("ctest-pthread", "ctest-pthread")`.
- No capability grants. It opens no files.
- `EXPECTED = 42`.
- **A larger budget than `self_test_ctls_thread`'s 2000 yields.** This one
  creates about 85 threads, and checks 5 and 6 are thousands of lock
  hand-offs, each a sleep and a wake. Every wait in it is on a thread that is
  making progress, so a budget measured in time (say 60 seconds) fits it
  better than a count of yields. A timeout here would itself be a finding: a
  lost futex wake-up leaves the fixture asleep forever.

**The legend** (also at the top of `main.c`):

- `50`–`57` the big stack: `50` attribute setup, `51` create, `52` join,
  `53` `pthread_getattr_np` in the thread, **`54` the stack is smaller than
  asked**, `55` the thread's buffer is not on its own stack, `56` the guard
  is not one 16 KiB page, `57` the buffer did not hold what was written.
- `60`–`63` detached: `60` create, **`61`/`62` join/detach did not answer
  `EINVAL`**, `63` the thread never finished.
- `70`–`74` many threads: `70` barrier init, `71` a create failed,
  `72` `pthread_getattr_np` failed, **`73` a thread reported the main
  thread's stack (it is untracked)**, `74` a join failed or returned the
  wrong value.
- `80`–`83` the caller's stack: `80` create, `81` join, **`82` the thread did
  not run on the supplied stack**, `83` a second thread on the same stack
  failed.
- `90`/`91` the mutex: `90` create, **`91` an increment was lost**.
- `100`/`101` the condition variable: `100` create, **`101` a value arrived
  out of order**.
- `110`/`111` `pthread_once`: `110` setup, **`111` the routine ran other
  than exactly once**.

A hang rather than an exit code means a sleeper was never woken: a futex
wake-up is being lost, in `posix/src/lowlevellock.rs` or in the kernel's
futex.

I have not touched `kernel/**`.
