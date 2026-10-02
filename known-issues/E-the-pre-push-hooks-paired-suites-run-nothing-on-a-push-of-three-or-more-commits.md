### [E] The pre-push hook's paired suites run nothing on a push of three or more commits -- 2026-09-25
**Status:** OPEN -- `scripts/hooks/pre-push` (gate 20 and the hook's own-suites
block, lines ~1154 and ~1208). Filed as
`requests/e-ab-pre-push-suites-never-run-on-a-multi-commit-push.md`; lane E has
not edited the hook, because the hook's ownership is open question A-Q11.

**In short:** when a script in `scripts/` is pushed, the hook is meant to run
that script's own test suite, and when the hook itself is pushed, its six
suites. Both steps list the pushed files by handing every pushed commit to a
single `git diff-tree`, which takes at most two trees and reads the rest as
path filters. On a push of three or more commits -- nearly every lane push --
the list comes back empty, the hook prints "no scripts/*.py in this push has a
paired test suite", and no suite runs.

**How it was found.** A lane-E push of ten commits, three touching `scripts/`
and one adding `scripts/test-check-cp-diff-sees-nul.py` beside the checker it
tests, logged that sentence. Reproduced by running the hook's pipeline by hand
against the same commits: nothing with `xargs -r git diff-tree`, the five
`scripts/` files with `git diff-tree --stdin`.

**The fix** is `git diff-tree --stdin` (or `xargs -r -n1`) in both places,
given in the request with a regression case for `test-pre-push-gates.py`.
Until then, run a script's `test-<stem>.py` by hand before pushing it.
