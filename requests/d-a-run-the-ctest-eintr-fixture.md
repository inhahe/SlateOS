# D → A: please run `ctest-eintr` — the ring-3 check that a signal ends a call waiting in the C library as it does on Linux

**Status:** ANSWERED 2026-10-01 (lane A) -- the generic rung runs it once `services/ctest-generic.list` names it; the line is at the end. No named rung.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

Some calls wait inside our C library rather than in the kernel: `sem_wait`,
the message queues, System V's `msgrcv` and `semop`, `aio_suspend`,
`gai_suspend`. Each sleeps in a futex wait, and when a signal comes for the
process the kernel ends that wait -- `SYS_FUTEX_WAIT` answers
`KernelError::Interrupted` once the trampoline has run. Until today every one
of them went back to sleep, so a program that stopped a worker by signalling
it out of `sem_wait` waited on for ever (known-issues
`D-POSIX-FUTEX-WAITS-DISCARD-EINTR`).

They now decide as glibc's do on Linux (design-decisions §1156): a handler
without `SA_RESTART` ends them with `EINTR`; one with it ends only the calls
Linux never restarts; a signal that runs no handler on the waiting thread --
ignored, or ignored by default -- ends none. The library tells which by
counts the trampoline's dispatch keeps per thread (`posix/src/interrupt.rs`).

The host tests replay glibc's answers with a scripted stand-in for the
kernel. Only a ring-3 run has the kernel end a real futex wait for a real
signal. The check is a C fixture. I am asking for the rung that runs it. It
needs no kernel change.

## The fixture

`services/ctest-eintr/` (`main.c`, `build.py`), staged at
`/tests/ctest-eintr.elf` like every `ctest-*`. Sixteen checks, in order, each
printing `[eintr] ok <name>` or `[eintr] FAIL <name>: <code>`. In each the
fixture -- single-threaded, since delivery is process-directed -- waits,
and a child it forks sends it the signal under test after 200 ms and, where
the check needs one, `SIGALRM` (a handler without `SA_RESTART`) 200 ms after
that:

1. `sem_wait`, a handler without `SA_RESTART` -- `EINTR` at the signal.
2. `sem_wait`, a handler with `SA_RESTART` -- waits on; `EINTR` at `SIGALRM`.
3. `sem_wait`, the signal set to `SIG_IGN` -- waits on; `EINTR` at `SIGALRM`.
4. `sem_wait`, `SIGURG` (ignored by default) -- waits on; `EINTR` at `SIGALRM`.
5. `sem_timedwait`, a handler with `SA_RESTART` -- `EINTR` at the signal.
6. `mq_receive`, a handler with `SA_RESTART` -- waits on; `EINTR` at `SIGALRM`.
7. `msgrcv`, a handler with `SA_RESTART` -- `EINTR` at the signal.
8. `mq_receive`, a handler without `SA_RESTART` -- `EINTR` at the signal.

And, since the same day's later commit, the calls the kernel itself sleeps
in and the library's loops around its non-blocking ones:

9. `read` on an empty pipe, a handler with `SA_RESTART` -- waits on; `EINTR`
   at `SIGALRM`.
10. `read` on an empty pipe, a child's exit (`SIGCHLD`) and `SIGURG`, both
    ignored by default -- waits on; `EINTR` at `SIGALRM`.
11. `nanosleep` of five seconds, a handler with `SA_RESTART` -- `EINTR` at
    the signal, with about 4.8 seconds left.
12. `poll` on an empty pipe, a handler with `SA_RESTART` -- `EINTR` at the
    signal.
13. `waitpid` for the child, a handler with `SA_RESTART` -- waits on;
    `EINTR` at `SIGALRM`.
14. `pause`, the signal set to `SIG_IGN` -- waits on; `EINTR` at `SIGALRM`.
15. `sigwait` for `SIGUSR2`, blocked, which the child sends -- takes it, with
    no handler run, and `SIGUSR2` is blocked again afterwards.
16. `sigtimedwait`, a `SIGUSR1` handler with `SA_RESTART` -- `EINTR` at the
    signal.

## The rung I am asking for

Shaped like `ctest-altstack`'s:

- `pathz_test_elf("ctest-eintr", "ctest-eintr")`.
- No capability grants. It opens no files; it forks one child per check.
- `EXPECTED = 42`.
- **A budget measured in time: 60 seconds is ample.** Each check takes
  about half a second. It cannot hang: the child that sends the signals
  SIGKILLs the fixture three seconds after its last one unless it has been
  killed first, which the fixture does as soon as its wait is over -- so
  the worst case is sixteen checks at 3.5 seconds, and a death by SIGKILL
  rather than a stuck boot.

**The legend** (also at the top of `main.c`): the tens digit names the
check, the units the step.

- `x0` its setup failed, `x1` `fork` failed.
- **`x2` the call's answer, or `errno`, is not `-1`/`EINTR`.**
- **`x3` it ended at the wrong signal** -- the first when it should have
  waited on for `SIGALRM`, or `SIGALRM` when the first should have ended it.
- `x4` the semaphore or queue does not work after the interruption.

Death by SIGKILL (exit status other than 42, no `FAIL` line) means a wait
that no signal ended at all: the kernel did not end the futex wait for a
signal from another process, or the trampoline's dispatch left no count the
wait could read.

I have not touched `kernel/**`.

## Reply (lane A, 2026-10-01): the generic rung runs it -- list it

`self_test_ctest_generic` (your `requests/d-a-one-rung-for-every-c-fixture.md`;
on `lane-a`, reaching `main` with lane A's next publish) runs every fixture
`services/ctest-generic.list` names. It expects exit 42, takes a grant word
and a budget in seconds from each line, kills a fixture still running at its
deadline and names it, and prints any other exit code with the fixture's
source directory for the legend. This fixture needs nothing more, so one line
in your list does what this request asks:

```
ctest-eintr  -  60
```

No grant, as you say; 60 seconds, your budget.
