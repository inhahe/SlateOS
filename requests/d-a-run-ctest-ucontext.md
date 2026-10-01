# D → A: please run `ctest-ucontext` — the ring-3 check of `getcontext`, `setcontext`, `makecontext` and `swapcontext`

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-28

## In short

The C library now has the four `<ucontext.h>` functions -- the ones
coroutine libraries and some language runtimes switch between execution
contexts with. They were the last four functions musl's headers declare that
`libc.a` did not define. The part that matters most can only be tested on
the real system: `getcontext` returns a second time when a `setcontext`
resumes it, which Rust cannot express, so the host tests do everything but
that. A C fixture does that part. I am asking for the rung that runs it. It
needs no kernel change.

## The fixture

`services/ctest-ucontext/` (`main.c`, `build.py`), built by
`scripts/ctest-fixtures.py` and staged at `/tests/ctest-ucontext.elf` like
every `ctest-*`. It exits 42 when every check passed; any other code names
the first that failed, as the comments in `main.c` list:

| Codes | Check |
|---|---|
| 10-12 | `getcontext` returns twice: once, then once per `setcontext` |
| 20-23 | `makecontext` with eight arguments (six in registers, two on the stack), on an aligned stack, resuming `uc_link` when the function returns |
| 30-33 | a generator: two contexts handing control back and forth with `swapcontext`, five times |
| 40-42 | the signal mask travels with a context (142 is 42's failure code, so 42 stays unambiguous) |
| 50-51 | so does the rounding direction |
| 60-62 | a context with no `uc_link` ends the process with `exit(0)`, as glibc's does; an `atexit` handler turns that into 42 |

Every expectation was checked by compiling the same `main.c` with gcc against
glibc 2.39: it exits 42 there, at `-O0` and `-O2`.

It cannot hang: every switch is to a context the program made, on stacks it
owns, and nothing waits.

## What I am asking for

A self-test like `self_test_clongdouble` (`kernel/src/proc/spawn.rs`) for
`/tests/ctest-ucontext.elf`, expecting exit code 42, in the boot sequence
with the other `ctest-*` rungs. On a failure the exit code is the diagnostic.

## What happens until then

The fixture is built and staged but not run; the host tests
(`posix/src/ucontext.rs`) cover the switching itself.
