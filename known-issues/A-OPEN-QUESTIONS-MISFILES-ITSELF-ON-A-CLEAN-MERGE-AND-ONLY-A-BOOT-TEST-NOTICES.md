## A-OPEN-QUESTIONS-MISFILES-ITSELF-ON-A-CLEAN-MERGE-AND-ONLY-A-BOOT-TEST-NOTICES (lane A, 2026-09-09)

**In short:** `open-questions.md` has one structural rule — an `OPEN` question
belongs above the `# Resolved` header, because the operator reads the queue from
the top and a question filed below it was never asked. Concurrent additions near
that header **merge cleanly into the wrong section**, and nothing looks until a
boot test does, nineteen minutes in. It happened twice on 2026-09-09.

**Both occurrences, both from conflict-free auto-merges:**

| when | what landed below `# Resolved` | how it was found |
|---|---|---|
| merging `origin/main` into `lane-a` | `C-Q12` (lane C) and `B-Q9` (lane B), both `Status: OPEN` | `check-open-questions.py`, run by hand after the merge |
| merging `origin/main` again, ~2 h later | `B-Q10` (lane B), `Status: OPEN` | same |

A third instance the same day was mine by hand, filing A-Q9: I anchored an
insert on `## Resolved — lane A` (the per-lane index) instead of `# Resolved`
(the section boundary, forty lines earlier), and **that** one cost a full boot
test before the gate caught it.

**Why it merges cleanly and comes out wrong.** `design-decisions.md` survives
the same traffic because its per-lane numbering bands give each lane a *distinct
insertion offset* — that is the whole point of the band split, and it works.
`open-questions.md` has no such split: every lane inserts immediately above the
one `# Resolved` header. Two lanes doing that touch different lines, so git has
no conflict to report, and whichever side is applied second can land on the far
side of the header. Neither lane did anything wrong and no marker appears.

**Why it is expensive out of proportion to its size.** The repair is seconds —
move the block, touching none of its text. But:

- `check-open-questions.py` is **not in `scripts/hooks/pre-push`**. The hook has
  gate 13 for `design-decisions.md`'s bands and gate 14 for stray Markdown
  headings; the structure of `open-questions.md` is the missing sibling.
- So the first thing that notices is `boot-test.sh`, where the gate sits at
  position **214 of 220** — see
  `A-KERNEL-CLIPPY-IS-CHECKED-ONLY-INSIDE-A-19-MINUTE-BOOT-TEST`. A 0.0-second
  check is the last thing between a nineteen-minute wait and the build.
- Pushed unnoticed, it breaks `main` for all three lanes, since every boot test
  runs that gate.

**The obvious fix, and the argument against it that I could not answer.**
Adding it as pre-push gate 16 costs **0.0 s** (measured, from the gate timing
TSVs) against a failure that occurred twice in one day. That is the exact
inverse of the pre-push kernel-clippy gate costed and rejected the same
afternoon (~100 s per kernel push, ~150 min/fortnight, to prevent a roughly
fortnightly break) — cheap where that was dear.

What stopped me is gate 14's own stated principle, which is about this file's
neighbours:

> *"A gate refuses to publish a new defect; blocking lane A's push on a
> pre-existing `---` in a lane C document it must not edit would be a gate that
> can only be satisfied by violating the ownership rules in roadmap.md."*

Gate 14 answers that with `--changed-only --head <sha>`, so it judges only what
the push introduces. **`check-open-questions.py` has no such scoping** — no
`--head`, no `--changed-only`, no argument parsing at all. Unscoped, the gate
could block lane A for a question lane C misfiled in its own commit, which is
not lane A's to move.

My rebuttal, for whoever picks this up: the observed trigger is *merging*, not
authoring. A lane adding a question to its own tree puts it above the header,
where it is obvious; all three of today's instances arose at a merge or an
insert against the boundary. The merger is both the cause and the right fixer,
and the fix relocates a block without editing its text. So an unscoped gate
would, in practice, fire on the person who can fix it.

That is reasoning, not evidence, and the counter-principle is load-bearing for
the gate family it would join — so this is written down rather than built.
**Two ways forward, either fine:**

1. **Teach `check-open-questions.py` `--head`/`--changed-only`**, then add it as
   gate 16 exactly as gate 14 is wired. Satisfies the principle outright; costs
   the argument-parsing the script has never had.
2. **Add it unscoped with an `ALLOW_` escape hatch**, on the reasoning above.
   Cheaper, and weaker precisely where gate 14 is careful.

**If it is never fixed:** nothing rots, but every concurrent pair of new
questions is a coin-flip on a wasted boot test, and the tree can reach `main`
in a state where all three lanes' boot tests refuse to build — found only by
whoever next waits nineteen minutes for it.
