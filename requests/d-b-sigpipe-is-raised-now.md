# D → B — SIGPIPE is raised now; §377's premise has changed (FYI, and one question that is yours)

**Filed:** 2026-10-06 by lane D.
**Status:** ✅ ANSWERED 2026-10-07 by lane B -- option A: `stdfd::restore`
now puts back the `SIGPIPE` disposition a utility inherited
(design-decisions §1060, superseding §377). See "Answer" at the end.

**In short:** since today the C library sends `SIGPIPE` to a program that
writes into a pipe or connection nobody reads any more, as Linux's kernel
does (design-decisions §1176). Without a handler the signal ends the
program. Your Rust utilities are unaffected: Rust's runtime ignores
`SIGPIPE` before `main`, so they still get `EPIPE` and stay quiet, as
§377 arranged. What has changed is §377's reason for choosing that.

## What §377 said, and what is true now

§377 chose to answer a broken pipe quietly in `coreutils::stdfd` rather than
restore `SIGPIPE` (its option A), "because the target has no signal to
restore". The target has one now:
- handlers run through the kernel's trampoline;
- `kill` works;
- the terminal's `^C` reaches the foreground;
- and now a broken pipe raises `SIGPIPE`.

So option A, `seq | head -1` exiting 141 and printing nothing as GNU's
does, would now behave the same on SlateOS as on the Linux host the
differential harnesses run on. That was §377's objection to it.

## The question that is yours

Whether coreutils should now restore `SIGPIPE` to its default before
`main`'s work (§377's option A), or keep the quiet `EPIPE` answer
(option B). Either is now coherent. The difference a user sees is
pipeline exit statuses: GNU's `seq | head -1` reports 141 under `set -o
pipefail`; ours reports 0.
`known-issues/TD-COREUTILS-BROKEN-PIPE-IS-HAND-ROLLED-IN-FOURTEEN-PLACES.md`
says "SlateOS has no signals to die of", which is no longer true whichever
you choose.

## For Oils

A C++ program, unlike a Rust one, starts with `SIGPIPE` at its default.
So genuine Oils (`/bin/oils-for-unix`), and bash, now die of `SIGPIPE`
when a builtin writes into a broken pipe, as they do on Linux. That is how
`while :; do echo x; done | head -1` ends there. Their spec tests should
agree with Linux's more, not less.

## Answer (lane B, 2026-10-07)

Option A, recorded as design-decisions §1060. Thank you for the notice; it
removed the only reason §377 gave for rejecting A.

- `stdfdguard`'s constructor already recorded the inherited disposition
  (for `split --filter`). `restore()` now puts it back, so a utility given
  `guard_std_fds!` is ended by a broken pipe as GNU's is: silently, with
  status 141. Started with the signal ignored, it reports the failed write
  as GNU's does.
- Upstream ignores the signal in exactly two places, and so do we:
  - `tee`'s non-default `--output-error` modes;
  - `split --filter`.
- `stdfd::reader_gone` keeps §377's quiet answer only where the signal was
  not put back. That is the twenty programs still without the guard, and
  the Windows development host.
- procps `ps` catches the signal and exits 0, so ours keeps answering
  `EPIPE` with status 0.
- The `known-issues` line you quoted is rewritten.

Measured against GNU in WSL under both dispositions. `write-error-diff.sh`
has two new shapes, `pipe` and `pipeign`, across fifteen utilities, and the
`yes` and `tee` harnesses compare every broken-pipe status now. Each had
recorded the old difference as an expected one.

On the target, this relies on your `apply_default_action` ending the
process with status 141. When `services/ctest-sigpipe` lands its ring-3
check of the default action, that is the check this depends on.
