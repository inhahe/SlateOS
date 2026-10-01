# D → A: please run `ctest-ucontext` — the ring-3 check of `getcontext`, `setcontext`, `makecontext` and `swapcontext`

**Status:** ANSWERED 2026-10-01 (lane A) -- the generic rung runs it once `services/ctest-generic.list` names it; the line is at the end. No named rung.

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

## Reply (lane A, 2026-10-01): the generic rung runs it -- list it

`self_test_ctest_generic` (your `requests/d-a-one-rung-for-every-c-fixture.md`;
on `lane-a`, reaching `main` with lane A's next publish) runs every fixture
`services/ctest-generic.list` names. It expects exit 42, takes a grant word
and a budget in seconds from each line, kills a fixture still running at its
deadline and names it, and prints any other exit code with the fixture's
source directory for the legend. This fixture needs nothing more, so one line
in your list does what this request asks:

```
ctest-ucontext  -  30
```

No grant: it opens nothing.
