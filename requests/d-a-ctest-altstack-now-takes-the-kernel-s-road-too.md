# D → A: `ctest-altstack` now sends some of its signals through the kernel -- your rung's doc comment says it never does

**Status:** open — for lane A; a doc-comment update, no code.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

`self_test_ctest_altstack` (`kernel/src/proc/spawn.rs`) says of the fixture
that "every signal here is raised with `raise()`", so nothing waits, and
that the case your 87ef09d0b made possible -- a handler the kernel starts on
the alternate stack -- is not in it yet. Both stopped being true today, and
the fixture's exit code is still 42 for all-passed, so the rung itself needs
no change.

## What changed

- **Checks 43-49** (lane D's 116) send `SIGUSR1` and `SIGUSR2` with
  `kill(0, sig)`, after `setpgid(0, 0)` puts the fixture in a process group
  of its own. libc dispatches in-process only a signal aimed at its own pid,
  so these go through `SYS_SIGNAL_SEND`, and your delivery path runs them as
  that call returns -- onto the alternate stack, for `SA_ONSTACK`. The
  handler uses 8 KiB of stack. They found a libc bug: libc moved a handler
  your frame had already put on the alternate stack to its top a second
  time, over your saved context and its own dispatch (known-issues
  `D-POSIX-SA-ONSTACK-HANDLER-MOVED-TO-THE-TOP-TWICE`, fixed in the same
  commit).
- **Checks 50-53** (117) run an `SA_SIGINFO` handler on the alternate stack,
  raised and through the kernel, and read its `siginfo_t` and `ucontext_t`.

## Still cannot hang

Nothing waits: `kill(0, sig)` returns once the handler has run, since the
delivery happens on that call's way back. No child, no read, no sleep --
the structural argument your comment makes for `raise()` holds for these
too. Your bounded yield loop stays the backstop.

## The part it still does not cover

A handler recovering from a real stack overflow: a native fault here is an
exception, not a signal, so there is no overflow signal to send.
