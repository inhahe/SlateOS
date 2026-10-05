## B-PATCH-DRY-RUN-NAMES-A-FILE-IT-DID-NOT-WRITE (lane B, 2026-09-12) — FIXED

Two differences in `--dry-run`, both in what it says rather than what it does.

**It named a reject file that does not exist.** Under `--dry-run` nothing is
written, but the failure summary still said
`1 out of 1 hunk FAILED -- saving rejects to file a/base.txt.rej`. GNU prints
the bare `1 out of 1 hunk FAILED` there. **Naming a file that was never created
sends the reader looking for it**, which is worse than saying less.

**And the progress line carried an ellipsis nothing upstream produces.** GNU
prints `checking file a/base.txt`; this build printed `checking file
a/base.txt...`. That decoration predates tonight's work — it was in the original
`diag!` call — and survived because every dry-run case was already failing for
the stream reason, so nothing had ever compared the text.

That is the same shape as the whole `patch` effort: a defect that only becomes
visible once the bigger one in front of it is gone. Nine fixes in, the count has
gone 3 passed / 62 differed to **49 / 16**, and most steps uncovered the next.
