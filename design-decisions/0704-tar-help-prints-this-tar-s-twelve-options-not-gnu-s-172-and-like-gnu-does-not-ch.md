## 704. `tar --help` prints this tar's twelve options, not GNU's 172 — and, like GNU, does not check that the write landed

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** After 703 gave `tar` long options, its own error messages ended
with `Try 'tar --help' or 'tar --usage' for more information.` — while both of
those commands answered "recognised but not implemented". A program that
directs you to two commands it refuses is worse than one with no help at all,
so `-?`/`--help`, `--usage` and `--version` are now implemented. Two things
about *how* were genuine choices rather than obvious: the help text is written
for this tar instead of copied from GNU's, and a failed write of it is ignored
rather than reported.

**Decision 1: the help text describes this tar.**

| Answer | *What changes* |
|---|---|
| **Write our own (chosen)** | `tar --help` lists twelve long options. A user who has read GNU's help gets a shorter, unfamiliar page — but everything on it works. |
| Reproduce GNU's help verbatim | `tar --help` lists 172 options, 160 of which the same program then refuses. The page is instantly familiar and is a lie in 159 places. |

Copying GNU's page is the more *compatible* answer and the wrong one, for the
reason 703 already settled about options generally: help text is read
specifically to find out what is available, so an over-broad page fails at the
one job it has. It would also read as a bug rather than a policy — someone who
follows the program's own help into `--exclude` and is told it is not
implemented has been sent there by the program.

The page keeps argp's *shape* — the `Usage:` line, an examples block, operation
modes separated from modifiers, informational options last — because that is
what a `tar` user recognises, and closes with the two facts that distinguish
this tar from the one the reader has used before: ustar only, and unimplemented
names refuse rather than being ignored. Without the second sentence a refusal
looks like breakage. A unit test asserts both directions: every performed
option appears, and a sample of refused ones does not.

**Decision 2: the write is unchecked, which diverges from the house rule.**

Measured: `tar --help >&-` prints nothing and exits **0**, where
`wc --help >&-` reports `write error: Bad file descriptor` and exits 1. argp
writes help to a `FILE*` and never asks whether it landed; coreutils utilities
run `close_stdout` at exit and do.

| Answer | *What changes* |
|---|---|
| **Match GNU — ignore (chosen)** | `tar --help \| head -20` exits 0 quietly, as it does everywhere else. A genuinely undeliverable help page fails silently. |
| Apply the coreutils rule — report | `tar --help \| head -20` can print `tar: write error: Broken pipe` and exit 1, which no other `tar` does. |

Help is most often read through a pager or `head`, so the second column's
failure mode is common and the first's is nearly unreachable — nobody is
depending on the exit status of a help page. This tar has no `close_stdout`
discipline on any of its other stdout paths either, so matching GNU here is also
the internally consistent choice; adopting the rule would mean adopting it for
listing output too, which is a separate change with its own measurements to do.

**Precedence was measured, not assumed,** and is the reason `parse_args`
returns its answer from *inside* the parse loop rather than validating argv
first:

| Command line | Result | Why |
|---|---|---|
| `tar --help --frobnicate` | help, exit 0 | `--help` came first |
| `tar --frobnicate --help` | `unrecognized option`, exit 64 | the bad one came first |
| `tar -c --help` | help, exit 0 | `--help` wins over doing the work |
| `tar --file --help` | `You must specify one of…`, exit 2 | `--help` was eaten as `--file`'s value |

All four are GNU's, and all four fall out of first-one-wins iteration. A parser
that checked the whole command line before returning would fail row 1; one that
scanned for `--help` up front would fail rows 2 and 4. A 19-case shape
differential against the real binary — exit status, which stream, which class of
message — found zero differences.
