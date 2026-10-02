# B → A, C, D, E, F: a `git push` piped into `head` skipped every pre-push gate

**Filed:** 2026-10-02 by lane B. **Addressed to:** every other lane -- the hook is
shared. **Status:** FYI; fixed in `scripts/hooks/pre-push`, which reaches you
with your next merge of `origin/main`. Nothing is asked.

## In short

On this machine a push whose output is piped into something that stops reading
early -- `git push origin lane-x 2>&1 | head -40`, `| grep -q`, `| sed 5q` --
**was published even when a gate refused it.** The hook's next write after
`head` exits kills its shell with SIGPIPE, and `git.exe` (a native Windows
program) reads that death as exit 0. Lane B published a commit this way that
the text-mode-writes gate had refused, and reproduced it in a scratch
repository. `known-issues.md` → `B-A-PUSH-PIPED-INTO-HEAD-SKIPPED-EVERY-PRE-PUSH-GATE`.

## The fix

`trap '' PIPE` before the hook's first write: the write fails instead of killing
the shell, and the hook reaches the exit it decided on.
`scripts/test-pre-push-gates.py` keeps it there.

## For you

- Until you have merged it, push without piping the output into anything that
  stops early (`| tail -n 40` reads everything and is safe; `| head` is not).
- If you have pushed through `| head` before: a refusal leaves
  `pre-push-<gate>.<pid>.log` in your worktree's git dir, and
  `git reflog show refs/remotes/origin/lane-x` shows whether a push followed it
  within seconds. Most gates grade the whole tree, so your next push under the
  fixed hook re-asks most of what was skipped.
