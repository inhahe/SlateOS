## §382 — Every differential harness builds its subject for Linux and runs it inside WSL; `diff-subject.sh` is retired

**Date:** 2026-08-25
**Decided by:** Claude (autonomous) — finishing the sweep §376 opened

**In short:** The tree has 45 test scripts, one per command-line utility, that
check our version of a program by running it side by side with the original GNU
version and comparing the output byte for byte. Until now some of them built our
program for Windows and ran it on Windows, comparing it against a *Windows
imitation* of the GNU tool — so a "no differences" result sometimes only meant
that two wrong things agreed with each other. All 45 now build a Linux program
and run both sides inside WSL against the real GNU tools. The old
Windows-building helper, `scripts/diff-subject.sh`, no longer has a caller and is
deleted; the reasoning written in its header moves into `diff-wsl.sh` rather than
being lost with it.

### What was decided

Every `scripts/*-diff.sh` sources the shared preamble, with nothing left on the
old path:

```
$ for f in scripts/*-diff.sh; do
    [ "$f" = scripts/all-diff.sh ] && continue
    grep -q '^\. "\$(dirname "\$0")/diff-wsl.sh"' "$f" || echo "NOT MIGRATED: $f"
  done
$                      # 45 harnesses, no output
```

Concretely, in this change:

- `scripts/diff-subject.sh` is deleted. Its two failure-mode paragraphs — the
  **stale binary** (`cargo test` and `cargo clippy` do not refresh a binary; found
  in `printf-diff.sh`) and the **wrong program entirely** (forty-two binary names
  produced by two packages, which cost `calc-diff.sh` a false "95 passed, 105
  differed" on 2026-08-21) — are moved verbatim into `diff-wsl.sh`'s header, which
  previously cited them by reference.
- The two delegating citations inside `diff-wsl.sh` ("`diff-subject.sh` argues at
  length…", "for the reason `diff-subject.sh` spells out at length") become the
  local text they were pointing at.
- `all-diff.sh` and `.gitattributes` name `diff-wsl.sh` in the prose where they
  named `diff-subject.sh`.
- This file's two *earlier* mentions of `diff-subject.sh` are left untouched.
  `design-decisions.md` is append-only history; those sentences were true when
  written and describe a decision made at that time.
- `known-issues.md`'s two mentions get a dated follow-up rather than an edit, for
  the same reason — but they *do* get one, because unlike this file they are read
  as current state. One sits in the still-open
  `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`, where "all 27 `*-diff.sh`
  harnesses use it" would otherwise stand as a live claim naming a file that no
  longer exists and a count that is now 45.

### Why the whole harness had to move, not just the reference

Three separate reasons, each sufficient on its own.

- **The reference has to be glibc's.** The host's `/usr/bin` is MSYS2, a Cygwin
  derivative, and its coreutils are not GNU's on exactly the axes a differential
  test measures. Its getopt says ``unknown option -- x`` where glibc says
  ``invalid option -- 'x'``; a harness pointed at it certifies the Cygwin wording.
- **The subject has to be a Linux binary.** `coreutils::stdfd` is
  `#[cfg(target_os = "linux")]`, so the Windows build is not merely a different
  compilation of the same program — it is a different program, missing the exit
  path that decides whether a write error is reported at all.
- **The host path had no staleness guard, and the WSL path does.** On 2026-08-24
  this workspace reached a state where `cargo build` exited 0 while the
  `coreutils` library was three commits stale, and `interleave-diff.sh` reported
  sixteen differences that did not exist. `diff-wsl.sh`'s `diff_assert_fresh`
  catches that; `diff-subject.sh` could not, because it only asked whether the
  file was there.

The third reason is what settled `extfloat-diff.sh`, which was the last holdout
and the one with no system counterpart at all — nothing *forced* it across.
`extfloat-probe` is an example in the `coreutils` package, so it links the exact
library that was stale that day, and a float harness reporting "no differences"
against a stale `extfloat.rs` is the most confidently wrong answer in the
directory.

### What is given up

- **Nothing in the suite is measurable without WSL.** Previously a Windows-only
  checkout could at least run the host-built harnesses. Now every one of the 45
  skips. This is accepted because a harness that runs without a glibc reference
  was not measuring the thing it claimed to; a skip is an honest zero and a
  Cygwin comparison is not.
- **The suite no longer compiles the utilities for a second triple.** Building
  for `x86_64-pc-windows-gnu` was, incidentally, a check that the code was not
  quietly Linux-only. That check is gone, and a portability regression will now
  surface somewhere else or not at all. Judged a small loss: the shipping target
  is `x86_64-slateos`, which is Unix-shaped, and `stdfd` is already explicitly
  Linux-gated — the portability the old build was proving is not portability
  anyone intends to keep.
- **Each run pays a re-exec and a 9p-mounted source tree.** Measured at seconds
  per harness against a suite that takes half an hour, so it did not weigh.

### Alternatives rejected

- **Leave `extfloat-diff.sh` on the host path.** It compares against a C probe
  compiled by `gcc`, not against a program in `/usr/bin`, so the glibc argument
  did not reach it and the Linux-binary argument barely did — `extfloat` builds
  its 80-bit format out of integers and `f64` is SSE on both triples. Rejected on
  the staleness guard alone, above.
- **Move `extfloat-probe` into `coreutils/src/bin/`** so the existing `DIFF_BINS`
  machinery would build it. Rejected: everything in `src/bin/` is installed into
  the image, and this is a test instrument. `diff-wsl.sh` gained a `DIFF_EXAMPLES`
  knob instead, which is four lines and keeps the probe out of the shipped set.
- **Keep `diff-subject.sh` as documentation**, with its functions unused.
  Rejected: it is a file whose whole shape says "source me", sitting in the
  directory where new harnesses are written. Prose that is still worth having is
  worth having in the file that is actually sourced.
