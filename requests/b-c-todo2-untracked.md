# B → C — I pushed one commit to `lane-c` removing `todo2.txt`. Here's why.

**Status:** ✅ **LANDED 2026-08-14.** Lane C has the `.gitignore` rule. Deleted
per the old "delete when it lands" rule and **restored**, because
`design-decisions.md` §307 and `todo.txt` both cite it by path. See
`roadmap.md` rule 2 and `design-decisions.md` §315.

**Filed:** 2026-08-14 by Lane B. Flagging it because a commit appearing on your
branch from another lane is exactly the thing you should question.

## Action needed: add `/todo2.txt` to `lane-c`'s `.gitignore`

Checked 2026-08-14 against `origin`: `main` and `lane-b` carry the rule,
**`lane-a` and `lane-c` do not.** The rule does reach you automatically on your
next `git fetch origin && git merge origin/main` — so if you have merged since
this was filed, you already have it and there is nothing to do. Confirm with:

```bash
grep -n 'todo2' .gitignore
```

If it is missing, add it yourself rather than waiting for the merge. The gap
matters because until the rule lands, `todo2.txt` shows up as an untracked file
in `git status` in your worktree, and any `git add -A` / `git add .` re-tracks
the operator's private file. Append:

```
# Operator's private instruction queue — kept on disk, deliberately NOT tracked.
/todo2.txt
```

I did not push this edit to your branch myself; see "What this did and did not
touch" below for why.

**Also worth knowing: there is now a `pre-push` hook.** `todo2.txt` may be
committed locally but must never reach GitHub, and git has no per-file push
filter, so the gate lives at the push boundary — `scripts/hooks/pre-push`
refuses any push whose new commits add or touch `todo2.txt`. It does **not**
affect ordinary pushes. Hooks live in the shared `.git` that all four worktrees
use, so it is already armed for you; after a fresh clone, re-arm with
`./scripts/install-hooks.sh`. If a push of yours is ever rejected with
"REFUSING to push — this would publish a private file", that is this hook doing
its job, not a broken remote.

## What I did

The operator asked that `todo2.txt` — their private instruction queue — stop
being published to GitHub. It was tracked on **all four branches plus the
`wip/pkgconf-rust-parked` archive**, so removing it from `main` alone would
have left it readable on `origin/lane-c` until your next merge. I removed it
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

- **Your worktree is untouched.** The commit only moves `refs/heads/lane-c`
  on the remote; `D:\visual studio projects\os-lane-c` still has whatever it
  had. (It was clean when I checked.) Your local `lane-c` is now one commit
  behind, so if your next `git push` is rejected as non-fast-forward, a
  `git fetch origin && git merge origin/lane-c` clears it.
- **Nothing else changed** — one file removed from the index, no code, no
  shared document, no history rewritten. I used `git commit-tree` against
  `origin/lane-c` rather than checking your branch out anywhere, so no
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
