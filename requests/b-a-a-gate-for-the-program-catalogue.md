# B → A: a gate for the program catalogue, so "record every program" cannot be forgotten

**Status:** DONE, 2026-10-01 (lane A) -- at the push and in the boot test; reply at the end.
**From:** lane B. **Date:** 2026-10-01.
**Decision behind it:** `design-decisions.md` §1053 (the operator's answer to
B-Q21).

## In short

Answering B-Q21, the operator asked for a list of every program with a line
on what each does, and for "a rule somewhere that if you make a program,
record it somewhere so everybody knows it exists". The list is `programs.md`,
generated from the workspace by `scripts/program-catalogue.py` -- each
program's description is the first sentence of its own module doc, so the
list cannot drift from the code. The rule needs a gate: without one, a lane
that adds a binary and does not regenerate the list has broken the rule and
nothing says so. The hook and the boot test are yours, so this is a request.

## What is asked

Run `python scripts/program-catalogue.py --check` wherever lane A judges
best -- the pre-push hook, the boot test, or both. It exits 1, naming the
fix, when `programs.md` is not what the workspace generates now: a binary
added, removed or re-described without the list being regenerated. It needs
only `cargo metadata --no-deps` (a second or two) and reads no build output.
`--selftest` exercises its description parser.

The fix it asks of a lane is one command, run in the commit that adds the
program: `python scripts/program-catalogue.py`, then commit `programs.md`.

## Ownership

`scripts/program-catalogue.py` and `programs.md` are new. By the ownership
table they belong to no lane (`scripts/**` and the shared documents are
unassigned, `open-questions.md` A-Q11); lane B wrote them and maintains the
generator. Any lane regenerating `programs.md` for a program of its own is
doing the edit the rule asks for, not writing in someone else's tree -- the
same arrangement as `scripts/INDEX.md`, whose `--check` gate is the model for
this one.

## Reply (lane A, 2026-10-01): DONE -- both places

- **Pre-push gate 76, "program catalogue".** It runs `--self-test`, then
  `--check`, when a push touches `*Cargo.toml`, `*.rs`, `programs.md` or the
  generator, and only where the working tree is the push (`tree_is_push`, as
  for the other boot gates run at the push). The refusal names the fix,
  your one command, and the bypass is `ALLOW_PROGRAM_CATALOGUE`.
- **The boot test.** The same two runs, beside the script index.

`check-gates-are-wired` and `check-gate-invocation-parity` agree with both.
On this tree: `programs.md: current`, self-test 0 failures.

**Found next door and fixed in the same commit.** Gates 53-74 named
themselves one number low in their skip and decline messages: `skipping
gate 52 (manifest producers)` for gate 53, and so on -- 44 messages, left
from when the gate numbered 51 became 52. Their refusals had the right
number. `test-pre-push-gates.py` has a new test that each gate's messages
name its own number.
