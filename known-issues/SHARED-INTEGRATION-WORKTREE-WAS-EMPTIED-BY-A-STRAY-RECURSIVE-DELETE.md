### SHARED-INTEGRATION-WORKTREE-WAS-EMPTIED-BY-A-STRAY-RECURSIVE-DELETE

**Status:** RECOVERED (no work lost), **cause not identified, hazard still
live**. **Lane:** found by A; the action that caused it was **not** lane A's.
**Found:** 2026-09-01 ~05:15, incidentally, while preparing a routine
`lane-a` → `main` merge.

**In short:** the shared integration checkout at `D:\visual studio projects\os`
— the one every lane merges through — had **every one of its 13,769 tracked
files deleted from disk** at about 04:23. Git still had all the content, so
nothing was permanently lost and the tree has been restored byte-for-byte. But
nobody noticed for ~50 minutes, and the failure mode this was one command away
from is severe.

**What was found.** `git status` in `os` reported 13,769 paths as ` D`
(deleted-in-worktree, unstaged). `ls` showed the directory held only two
entries, both untracked and both stamped 04:23:

```
d/sym -> real      (a relative symlink)
real/              (empty)
```

That pair is a **symlink-resolution test fixture** — the canonical shape for
exercising `realpath`/`readlink`. It appears in no script in the tree
(`grep -rn 'd/sym'` finds nothing outside prose), so it was almost certainly
created ad-hoc by an agent's shell command, in the integration worktree, which
`CLAUDE.md` says plainly is a merge tree and must not be edited.

**The most probable mechanism**, on that evidence: a recursive delete run to
clean the fixture up, rooted one directory too high — at `os` itself rather
than at the scratch directory — which then walked the tree deleting everything
and bailed out on the one entry it could not handle, the symlink. That is
exactly the residue observed: the whole tree gone, `d/sym` and its target left
standing. It also matches the shape of the `rd /s /q` cleanup that project
convention encourages for build output.

**Why nothing was lost.** `os` is a pure checkout of `main`: it had no
local-only commits (`git log origin/main..HEAD` empty), nothing staged, and no
modified tracked file other than `.claude/scheduled_tasks.lock`. So every
deleted path's content was in `HEAD`, and

```bash
git ls-files --deleted -z | xargs -0 -n 500 git checkout --
```

restored all 13,769 with `git diff HEAD` then clean. Only the deletions were
discarded, which is the point.

**The near miss, and why this is logged rather than shrugged off.** The next
thing any lane does in this worktree is merge. A merge preceded by a habitual
`git add -A` would have **staged 13,769 deletions and committed them to
`main`** as an ordinary-looking commit — and it would have been pushed, because
pushing after a merge is standing policy here. Recovering *that* means a revert
on shared history with three lanes building on top of it. The window between
the delete and someone merging was about 50 minutes and closed only because the
merge happened to start with `git status`.

**What should change** (not done — the fix belongs to whoever owns the
convention, and one of these is a cross-lane policy question):

1. **Never create scratch fixtures inside a git worktree**, least of all the
   shared one. `os` is not a scratch directory. A symlink fixture belongs in a
   temp dir that no recursive delete can reach a repository from.
2. **A recursive delete should never be rooted at a path that contains a
   `.git`.** A three-line guard in whatever cleanup script did this — refuse if
   the target or any ancestor is a work tree — would have turned a 50-minute
   silent data loss into an error message.
3. **The merge procedure should refuse a dirty integration tree.** `os` should
   be verified clean *before* a merge starts, and a bulk deletion should abort
   it rather than be swept into it. This is the cheapest of the three and the
   one that actually caught the incident, by accident, this time.

**Left in place deliberately:** `d/sym` and `real/` still exist and are still
untracked. They are another lane's, they are harmless (the only symptom is
`git status` printing `warning: could not open directory 'd/sym/'`), and
deleting another lane's files is the class of action that caused this. Whoever
recognises them should remove them — from a shell whose working directory is
not a repository.
