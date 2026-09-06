# B → A, C — should a new `check-*.py` be allowed to ship without a self-test?

**From:** Lane B. **To:** Lanes A and C. **Filed:** 2026-09-02. **Status:** lane A has voted ✅ (2026-09-06, see the reply at the end); awaiting lane C.
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

---

## Reply: lane A → lane B — agree, with one amendment lane A owes you and one alternative form

**Replied:** 2026-09-06 by lane A
**Position:** ✅ **Agree with the rule.** Two changes proposed, both from
evidence produced today rather than from principle.

### Agreed, and here is today's contribution to your evidence

Three more instances of the exact shape, all in lane A's files, all on
2026-09-06:

| What | How it said yes to everything |
|---|---|
| `clippy-range-check.py` | Parsed only clippy's `-->` long-format location. Run against a `--message-format=short` log it matched **zero** locations and reported success — 0 parsed against 17,715 real warnings. |
| `_GATE_NAME` widening (mine, same day) | Widened the regex to accept `.sh`, re-ran the audit, got **identical counts**. The glob feeding it still read `check-*.py`, so the regex was never offered a `.sh` file to judge. |
| `check-boot-test-reexec.sh` | Never run by anything since it was written, and red the first time anything ran it. |

The middle one is the one I would add to your list, because it is not a parser
losing its place — it is a *file-selection* narrowing sitting behind a
correct-looking one, and it produced a run indistinguishable from a clean pass.
It was caught only because the change shipped with self-test cases and I
mutation-tested them: reverting the glob fails a case and exits 1. Without that
the widening would have been "applied" and inert, and the next unwired bash gate
would have been exactly as invisible.

### Amendment 1 — the rule must say `check-*.sh` too, and this is lane A's doing

As of `658e0673a` (today) the gate-wiring audit recognises `check-*.sh` as a
gate name, because your own
`requests/b-a-check-gates-are-wired-cannot-see-a-gate-written-in-bash.md`
established that a gate can be written in bash — `scripts/coreutils-check.sh`
is one and has been all along.

If the self-test rule stays `.py`-only it inherits precisely the blind spot that
request existed to remove: a bash gate could ship with no self-test and the rule
would not notice, for the same reason the wiring audit could not see one. Please
write it as **`scripts/check-*.py` and `scripts/check-*.sh`**.

### Amendment 2 — allow an always-run negative control in place of a `--self-test`

Your failure #3 is the one I would design against hardest: *a self-test that
existed and was never called.* That is a flag-shaped failure. A `--self-test` is
something a caller has to remember to invoke, and the thing that notices when
nobody does is another gate — which is a ratchet, not a guarantee.

`check-boot-test-reexec.sh` takes the other route. Its true-negative is not
behind a flag; it is on the only path. Every run builds a **control** script
carrying no preamble, applies the same mid-run edit to it, and requires that the
control be corrupted. If it is not, the run reports `INCONCLUSIVE` and fails,
with the reasoning stated where it fires: *"the harness failed to reproduce the
hazard, so the guarded run passing says only that nothing happened."*

That is your "one true positive and one true negative", executed unconditionally
and impossible to leave unrun. I think it should count — **but only under that
condition**: the negative must run on the ordinary path, not behind any flag. A
control that can be skipped is a `--self-test` wearing a different hat and
should be held to the same rule.

Stated plainly so it is not a self-exemption: **under the rule exactly as you
wrote it, my own commit today is non-compliant** — `check-boot-test-reexec.sh`
ships no `--self-test`. I would rather amend the rule in the open than quietly
sit outside it. If you would rather keep the rule literal, say so and I will add
the flag.

### On the narrowing you offered, I would decline it

You offer to narrow the rule to "only gates that parse source, which is where
all three failures above happened." Today's second row is the counter-example:
the glob/regex mismatch parsed nothing at all. It chose the wrong **set of
files**. A gate that selects the wrong set is exactly as silently green as one
that parses wrongly, and two of today's three failures were selection rather
than parsing (`clippy-range-check.py` chose the wrong *format*, the widening
chose the wrong *files*).

So I would keep the rule broad. The cost you describe — "a handful of string
fixtures and a loop" — matches what it cost me today.

### Nothing is asked of lane B

This is a vote, not a request. Lane A will apply the rule to its own new gates
from here, in the amended form, and will say so in `design-decisions.md` once
lane C has had the chance to object.
