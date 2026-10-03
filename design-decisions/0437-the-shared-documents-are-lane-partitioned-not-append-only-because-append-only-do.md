## §437 — The shared documents are lane-*partitioned*, not append-only, because append-only does not prevent the conflict it was adopted to prevent

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Operator (operator's own challenge to the rule; Claude proposed this replacement)

**In short:** Four documents that all three lanes write — the bug list, the
open-questions queue, this file, and `todo.txt` — carried a rule saying you
may only *add* to them, never change or delete a line. That rule was adopted
to stop three agents' edits colliding when they merge their work together.
It does not do that, and it made the files worse to read: answered questions
piled up at the top of the questions file where they were most in the way,
and a bug that had been fixed could not say so. The rule is replaced with:
each lane owns a *region* of each file and may do anything it likes inside
its own region, including deleting.

**The rule that was there.** "Shared documents are append-only, with
per-lane sections": new entries go at the very end, prefixed with your lane
letter; editing an existing entry is allowed only if its heading carries
your letter.

**Why it does not work.** Append-only was a proxy for the actual goal —
*three lanes writing one file must never produce a merge conflict* — and it
is a proxy that fails in the ordinary case. Git merges by line region. Two
lanes that both append at end-of-file are writing the same region, so
append-only produces the conflict rather than preventing it. This is
measured, not predicted: merging `origin/main` into `lane-c` on 2026-08-16
conflicted in *this file*, because lane A had appended §203 at EOF and lane
C had appended §435 and §436 at EOF. Both lanes had followed the rule
exactly.

The per-lane numbering (A §200–299, B §300–399, C §400–499) was believed to
be what made this file merge cleanly. It is not — or rather, it would be, if
the file were ordered by number. It is ordered *chronologically*: §424 sits
between §308 and §309. Numbering that does not correspond to physical
position partitions nothing. `CLAUDE.md`'s claim that the numbering
"auto-merged across a 72-commit divergence with zero conflicts" describes a
window in which only one lane happened to append.

**What actually prevents conflicts: partitioning.** If each lane writes a
different *region* of the file, two lanes' edits land at different line
offsets and git merges them without inspecting content. Partitioning
subsumes append-only — appending is just the special case where the region
is "the end" — and it is strictly stronger, because it also covers two lanes
appending at once, which append-only does not.

And partitioning permits, at no cost to the goal, everything append-only
forbade: in-place edits, status stamps, restructuring, and **deletion**
within your own region.

**What the old rule cost, concretely.**

| Symptom | Measured 2026-08-16 |
|---|---|
| `open-questions.md` is cluttered | 7 answered questions occupy lines 52–219; the 3 actually-open ones begin at line 220. The file's entire purpose is to be scanned for open questions, and they are last. |
| The rule is not followed anyway | All 7 answered entries are stamped `Status: **RESOLVED**` *in place*, which append-only forbids. |
| The rule contradicted `CLAUDE.md` | `os/CLAUDE.md` says an answered question moves to `design-decisions.md` and is *removed* from `open-questions.md`; `roadmap.md` rule 3 said "Append only." Agents followed the useful one, so the written rule had already lost. |
| The lane-letter carve-out is undecidable | `known-issues.md` has 999 headings; **34** carry a lane letter. For the other ~965, "editing is allowed only if the heading carries your lane letter" answers *no* for every lane — including whoever fixes the bug. |

That last row is the one that matters most. A bug list that structurally
cannot record "this is fixed" degrades into a list of things that *were once*
wrong, which is the one thing a bug list must not be. 518 of those entries
say FIXED — all of them written in violation of the rule, and all of them
correct to have been written.

**The alternative that was rejected: keep append-only and add a separate
resolution log.** Status would live in a second file keyed by entry ID.
This preserves append-only literally, and it was rejected because it doubles
the number of places a reader must look while making neither of them
authoritative — and it still conflicts, since the resolution log is itself
appended to by all three lanes at EOF. It satisfies the letter of the rule
and none of its purpose.

**The one genuinely cross-lane edit, and why it is allowed.** Any lane may
add or update a single `**Status:**` line at a fixed position under any
`known-issues.md` heading, without a request. It is a one-line edit at a
known offset, so a collision is trivial to resolve; the alternative is that
an issue you fixed stays open forever in the file whose job is knowing what
is open; and two lanes stamping the same entry is *information* — two lanes
believed they fixed the same bug — that should be surfaced rather than
designed away.

**If it is never revisited:** the documents stay readable and the merge
behaviour strictly improves, since numeric ordering in this file turns three
EOF-appenders into three disjoint insertion points.

**Where:** `roadmap.md` → "Three-Agent Parallel Execution" rule 3;
`open-questions.md` (`## Resolved` index); `deferred-questions.md`
(`## Closed` index); `known-issues.md` and the new
`known-issues-resolved.md`; this file's numeric ordering.
