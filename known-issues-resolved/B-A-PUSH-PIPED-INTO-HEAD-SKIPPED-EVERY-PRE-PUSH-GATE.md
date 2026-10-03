## B-A-PUSH-PIPED-INTO-HEAD-SKIPPED-EVERY-PRE-PUSH-GATE -- on Git for Windows a hook killed by SIGPIPE reads as a pass (lane B, 2026-10-02) — **FIXED** 2026-10-02

**In short:** a `git push` whose output was piped into something that stops
reading early -- `git push origin lane-b 2>&1 | head -60` -- went through even
when a pre-push gate refused it. When `head` exits, the hook's next write to
the pipe kills its shell with SIGPIPE, and `git.exe`, a native Windows program,
reads that death as exit 0. Every gate in `scripts/hooks/pre-push` could be
skipped this way without anyone choosing to skip it. Fixed by having the hook
ignore SIGPIPE (`trap '' PIPE`) before it writes anything.

**Found by** a push of lane B's that should have been refused. The
text-mode-writes gate logged a refusal at 03:13:23
(`pre-push-text-mode-writes.776805.log` in the worktree's git dir) and
`refs/remotes/origin/lane-b` records "update by push" at 03:13:32 -- the push
that had been piped into `head -60`. (The finding itself was the gate's false
positive on `tarfile.open(..., mode="w")`, fixed in the same change; the
bypass was real either way.)

**Reproduced in isolation:** a scratch repository whose pre-push hook prints a
line, sleeps, prints ten more and exits 1. `git push` refused; `git push 2>&1 |
head -1` published the commit. The same hook with `trap '' PIPE` as its second
line refused both ways, and a passing hook still passed. Then against the real
hook: with a text-mode finding planted in the worktree, a push to a throwaway
bare clone through `| head -5` succeeded.

**The fix:** `scripts/hooks/pre-push` ignores SIGPIPE before its first write, so
a write to a closed pipe fails with EPIPE and the hook reaches the exit it
decided on. No pipeline in the hook needs SIGPIPE to stop a producer (each is
finite, and git exits quietly on EPIPE regardless).
`scripts/test-pre-push-gates.py` asserts the trap is in code rather than prose,
precedes the first write, and is never reset; it fails against a mutant without
it.

**What it may have let through before.** Any push made from a Windows host with
its output piped into `head`, `grep -q`, `sed ...q` or anything else that stops
reading. A refusal leaves a `pre-push-<gate>.<pid>.log` in the pushing
worktree's git dir; one followed within seconds by "update by push" in
`git reflog show refs/remotes/origin/<branch>` is the sign. Most gates grade the
whole working tree rather than the pushed range, so the next push made under
the fixed hook re-asks most of what was skipped.
