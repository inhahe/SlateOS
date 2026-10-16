# D → F: `review-batch.py`'s mutation testing never starts on `posix`

**Status:** OPEN · **Filed:** 2026-10-16 by lane D · **For:** lane F
(`scripts/review-batch.py`, its mutation-testing step).

## In short

On lane D's round-202 batch the mutation step tested no mutant at all:
cargo-mutants first runs the crate's tests unmutated, and `posix`'s take
longer than the `--timeout 300` the script gives every run -- 285 seconds
of library tests (9425 of them) and then the doctests -- so the baseline is
called a timeout, cargo-mutants stops with "cargo test failed in an
unmutated tree, so no mutants were tested", and the trailer reads
`Review-mutants: 0 missed of 0 tested (1243 found; not finished: posix)`.
Every lane D batch touching `posix` will end the same way.

Even with the baseline passing, the half hour the step has would test five
or six of `posix`'s mutants at five minutes a run, out of 1243 -- too few
to mean anything.

## What would make it work

Either of these, and preferably both:

1. **Run only what the mutated code is tested by.** A mutant in
   `posix/src/time.rs` is caught, if at all, by `time::tests` (and a few
   neighbours'). cargo-mutants passes arguments after `--` to `cargo test`,
   so `-- --lib -- time::` (the mutated files' module paths as libtest
   filters) cuts each run from five minutes to seconds, and leaves the
   doctests out.
2. **Size the timeout from the baseline**, not a constant: cargo-mutants
   takes `--timeout-multiplier` against the measured baseline, which is what
   it is for; a fixed 300 s cannot fit both a small crate and this one.

## Where

`scripts/review-batch.py`, the `cargo mutants` command (`"--timeout",
"300"`, around line 757). Results of the failed run:
`E:\visual studio projects\os\.git\reviews\20261016T174651-491682b8daeb-mutants\posix\output.txt`.

## If this is never done

Nothing breaks: the reviews still run. But no `posix` batch is ever
mutation-tested, and every follow-up commit carries a trailer saying so.
