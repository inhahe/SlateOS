## Two lanes merging up at once race in the shared `os` worktree (lane C)

**Status: OPEN 2026-08-16** (lane C). Observed, not theorised: a routine
`git merge lane-c` in `D:\visual studio projects\os` printed `Updating
239fd95f7..dc9a2caa9` alongside git's "a git process may have crashed in this
repository earlier" lock error, and when it settled `HEAD` was at `41d86dee2`
— *lane B's* merge, which had been running in the same directory at the same
moment. Neither merge's result survived intact.

`CLAUDE.md` gives every lane its own worktree precisely so that one lane's
`git checkout` cannot move another's `HEAD`, and then routes all three lanes
back through **one shared checkout** — `os` — for the merge-up step. That
reintroduces exactly the hazard the worktrees removed, just narrowed to the
window in which a merge runs. There is a lock that serialises QEMU; there is
none for this.

**The mitigation is available today and needs no lock, so prefer it:** merging
up does not require a working tree at all when the merge is a fast-forward,
which it is whenever you have already merged `origin/main` into your lane
(which the same rules require you to do first). From your *own* worktree:

```bash
git fetch origin
git merge origin/main          # resolve here, in your own tree, and test
git push origin lane-c         # publish the lane
git push origin lane-c:main    # fast-forward main; no shared checkout touched
```

This is strictly safer than the documented route: the merge and its conflict
resolution happen in the tree you already have exclusive use of and have just
run the tests in, and the step that touches `main` is a server-side
fast-forward that cannot interleave with another lane's — if a lane pushed in
between, the push is simply rejected as non-fast-forward and you merge again.
`main` still only ever advances to a commit whose tests were run.

**What the proper fix looks like:** amend `CLAUDE.md`'s "When You Finish a
Task" step 11 to prescribe the fast-forward push instead of a merge performed
in `os`, leaving `os` as a read-only integration *checkout* for inspecting
`main`. That file is operator-owned — this needs an explicit instruction
before it can be edited, so it is written up here rather than done. Until
then, any lane that does use `os` should treat it as exclusive and check
`git -C os status` first.
