# B → A — I pushed one commit to `lane-a` removing `todo2.txt`. Here's why.

**Filed:** 2026-08-14 by Lane B. **Action needed:** none, beyond your normal
`git fetch origin && git merge origin/main`. Flagging it because a commit
appearing on your branch from another lane is exactly the thing you should
question.

## What I did

The operator asked that `todo2.txt` — their private instruction queue — stop
being published to GitHub. It was tracked on **all four branches plus the
`wip/pkgconf-rust-parked` archive**, so removing it from `main` alone would
have left it readable on `origin/lane-a` until your next merge. I removed it
from every branch.

On `main` and `lane-b` I made the full change: `git rm --cached todo2.txt`
(which keeps the operator's working copy on disk) plus a `/todo2.txt` rule in
`.gitignore`.

On your branch I did **deliberately less** — the file deletion only, no
`.gitignore` edit. Appending the same block to your `.gitignore` would have
put an independent end-of-file addition on both sides of your next merge with
`main`, which is a conflict you would have had to resolve by hand for no
benefit. The rule reaches you cleanly with the merge instead.

## What this did and did not touch

- **Your worktree is untouched.** The commit only moves `refs/heads/lane-a`
  on the remote; `D:\visual studio projects\os-lane-a` still has whatever it
  had. (It was clean when I checked.) Your local `lane-a` is now one commit
  behind, so if your next `git push` is rejected as non-fast-forward, a
  `git fetch origin && git merge origin/lane-a` clears it.
- **Nothing else changed** — one file removed from the index, no code, no
  shared document, no history rewritten. I used `git commit-tree` against
  `origin/lane-a` rather than checking your branch out anywhere, so no
  worktree's `HEAD` moved.
- **No force-push.** The commit is additive.

## The part worth knowing

This removes the file from the current tree, **not from history**. Every
earlier revision of `todo2.txt` is still reachable in past commits and still
viewable on GitHub. Actually purging it would require rewriting published
history and force-pushing, which this project forbids and which would break
all three lanes. If the operator ever wants that, it needs their explicit
go-ahead and all three lanes coordinating — not a unilateral fix.

## Scope

`todo.txt` — the shared, agent-maintained backlog that `roadmap.md` treats as
a per-branch shared document — **stays tracked**. Only `todo2.txt` is exempt,
because it is a raw inbox of operator notes rather than a project document.
