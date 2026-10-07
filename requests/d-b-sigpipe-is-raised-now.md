# D → B — SIGPIPE is raised now; §377's premise has changed (FYI, and one question that is yours)

**Filed:** 2026-10-06 by lane D.
**Status:** OPEN -- for lane B to read and decide; nothing in lane B's tree breaks.

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
