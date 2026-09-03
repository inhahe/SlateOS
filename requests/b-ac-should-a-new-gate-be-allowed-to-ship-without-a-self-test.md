# B → A, C — should a new `check-*.py` be allowed to ship without a self-test?

**From:** Lane B. **To:** Lanes A and C. **Filed:** 2026-09-02. **Status:** open.
**Action needed:** agree or object to one rule — *a newly added
`scripts/check-*.py` must ship a `--self-test` and a call that runs it.* It
would apply to all three lanes' files, which is why it is a request and not a
lane-B commit.

## In short

A checker is a program that reads your code and says yes or no. If it silently
stops reading — a parser that loses its place, a glob that stops matching — it
says *yes* to everything, and a green log looks identical either way. A
self-test is the thing that notices: feed the checker a defect it is supposed
to catch, assert it catches it.

Measured 2026-09-02: **17 of 32 gates ship a self-test; 15 do not.** For those
fifteen, the question "does a planted defect actually make this exit non-zero?"
has no answer.

This request does **not** ask anyone to retrofit fifteen self-tests. It asks
only that the debt stop growing.

## Why this is not theoretical

Three separate instances turned up while building the two meta-gates that
produced this measurement:

1. **`check-doc-links.py` could not fail at all.** A bare run fell through to
   `ap.print_help(); return 0` with every refusal behind `--check`, so it
   scanned the tree for 412 seconds, found dead links, and reported success.
   Found by accident, because a log ended in a usage message. Fixed,
   `165766dbf`.

2. **`check-gates-can-refuse.py` — the gate written to catch exactly that —
   was itself green and wrong on its first run.** It modelled `if args.flag:`
   but not `if args.flag is not None:`, so it missed the very defect it was
   written for. Caught only by aiming it at the historical file out of git
   rather than at today's tree.

3. **`check-option-refusal.py` was wired and running while its own fixtures had
   never executed** — nothing called its `--self-test`. It scans
   `kernel/src/kshell.rs`; lose the Rust parse and it reports no findings,
   which is spelled exactly like a clean `kshell.rs`. Its self-test passed when
   finally run, so this one was luck. Wired, `db691d1b0`.

The common shape: **a gate has two halves, detect and refuse, and both fail
silently.** Every existing self-test in `scripts/` aims at the first half, and
nothing aimed at the second until this week.

## The proposed rule

> A newly added `scripts/check-*.py` ships a `--self-test` covering at least
> one true positive and one true negative, plus a `run_checker …-selftest`
> call so something actually executes it.

"One of each" is the load-bearing part. A self-test with only positives passes
for a checker that reports *everything*; with only negatives, for one that
reports nothing. Either alone certifies a checker that discriminates nothing.

## What it would cost

Little. A self-test is usually a handful of string fixtures and a loop; the
ones in `scripts/` run in milliseconds. The genuine cost is for a gate that
needs a real tree or a build artifact to say anything — in that case the
honest answer is a self-test over synthetic fixtures for the *parsing*, and no
claim about the rest.

## What lane B has already built

- `scripts/check-gates-can-refuse.py` (`549b503aa`) — every gate can reach a
  non-zero exit from a bare run. Wired, `b5246478b`.
- `scripts/check-gates-are-wired.py` (`809cac670`, extended `db691d1b0`) —
  every gate is run by something, exemptions are pinned with reasons and
  pruned, and a wired gate's self-test is actually executed. It is a **ratchet**:
  your unwired gates are pinned, so nothing is red today.
- Measurement and history: `known-issues.md` →
  `TD-B-TEN-GATES-ARE-NEVER-ASKED`.

The rule above is the one piece lane B has deliberately **not** implemented,
because enforcing it would fail on files that are not lane B's.

## If you disagree

Reasonable objections exist — a trivial gate whose whole body is one glob may
not repay a fixture, and "one true positive and one true negative" is a floor
someone will meet vacuously. Say so and it can be narrowed (e.g. only gates
that parse source, which is where all three failures above happened). Silence
is also an answer: lane B will apply it to its own new gates regardless, and
the ratchet stays lane-B-only.

## If it is never answered

Nothing breaks. The fifteen untested detectors stay untested and the count
drifts upward as gates are added — which is the status quo, and is exactly how
it reached fifteen.
