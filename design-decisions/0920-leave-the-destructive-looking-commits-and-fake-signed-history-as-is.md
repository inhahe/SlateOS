## §920 — Leave the destructive-looking commits and fake-signed history as-is

**Date:** 2026-09-07. **Decided by:** Operator. **Lane:** A.

**In short:** two commits that appear to delete the whole OS and 33 commits
signed by a name the operator did not authorise are permanently in the
published git history. The operator chose **option A**: leave the history
as-is. The commits are harmless (the "deletions" were artifacts of the
migration, and the signatures were from a misconfigured account), and
rewriting published history would break all three lanes' worktrees and
require force-pushes on shared branches.
