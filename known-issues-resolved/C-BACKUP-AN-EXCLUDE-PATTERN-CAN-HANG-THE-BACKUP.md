## `C-BACKUP-AN-EXCLUDE-PATTERN-CAN-HANG-THE-BACKUP` (lane C, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** `backup create --exclude '**a'` never finished. It did not fail
with a message, it did not skip the pattern — it recursed until the stack ran
out and the process died, part-way through a backup. Any exclude pattern
containing `**` that was not a whole path segment did this: `**.tmp`,
`**cache`, `***`. The fix makes such a pattern behave as a plain `*`, which is
what bash's `globstar` and gitignore already do.

**Where it lived:** `apps/backup/src/main.rs`, `glob_match_recursive` and
`glob_match_simple`.

**The defect.** The two functions disagreed about which `**` was a globstar.
`glob_match_simple` delegated to `glob_match_recursive` on *any* `**`;
`glob_match_recursive` accepted only a `**` that was a whole path segment
(`**` at the end, or `**/`) and handed anything else straight back to
`glob_match_simple`. Neither consumed a byte before delegating, so the two
bounced the identical `(pattern, text)` pair back and forth forever:

    glob_match_simple("**a", "b")  -> glob_match_recursive("**a", "b")
    glob_match_recursive("**a", "b") -> glob_match_simple("**a", "b")   // no progress

Each side was locally reasonable. The bug lived in the gap between them, which
is why reading either function alone did not show it.

**Why it went unseen.** The five existing glob tests all used well-formed
patterns (`*.txt`, `**/*.rs`, `a?c`). A malformed one was never tried, and the
failure mode is a hang rather than a wrong answer, so no assertion could have
caught it in passing.

**How it was found and verified.** Not by reading — by differential fuzzing.
The rewrite that removed this file's `indexing_slicing` warnings was checked
against the previous implementation over an exhaustive space of patterns and
texts up to four bytes (alphabets `*?/ab` and `/ab`). The old implementation
was depth-limited so non-termination could be *counted* rather than hang the
harness. Result over 94,501 pairs:

| | count |
|---|---|
| agree exactly | 90,841 |
| differ | **0** |
| old code did not terminate | **3,660** (3.9%) |

So the rewrite is behaviour-preserving everywhere the old code produced an
answer at all, and the 3.9% is the bug's true reach — this was not an exotic
corner.

**The fix.** `glob_match_simple` now delegates only when the `**` is a whole
path segment — exactly the condition `glob_match_recursive` accepts. The two
agree, so the loop cannot form. A `**` that is not a whole segment falls
through to the ordinary single-`*` arm, which always advances `pi`, so progress
is structural rather than argued. Regression tests
`a_double_star_that_is_not_a_path_segment_terminates` and
`a_double_star_that_is_a_path_segment_still_spans_directories` pin both halves;
the first hangs the suite rather than failing it if the bug returns, which is
the loudest signal this shape of defect allows.

**Lesson worth keeping.** Two functions that delegate to each other must agree
on the condition that decides *which* of them handles a case, and the agreement
has to be written down in both places or it will drift. Mutual recursion with
no argument shrinking on some path is a hang waiting for the right input.
