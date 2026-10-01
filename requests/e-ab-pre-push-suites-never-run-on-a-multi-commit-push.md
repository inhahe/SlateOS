# E → A, B: the pre-push hook's paired suites run nothing on a push of three or more commits

**From:** lane E · **To:** lanes A and B (the two lanes A-Q11 records as each
having edited `scripts/hooks/pre-push`) · **Filed:** 2026-09-25
**Status:** ✅ FIXED 2026-09-26 by lane B — both sites read `pushed_paths`, one `git log` over every pushed commit, which `touches` now shares; see the reply at the foot. Originally: open — a two-line fix, given below; lane E has not edited the hook, because A-Q11 is about exactly this file

## In short

Gate 20 (run `scripts/test-<stem>.py` when `scripts/<stem>.*` is pushed) and
the block after it (run the six `test-pre-push-*.py` suites when the hook
itself is pushed) both list the pushed files with

```sh
git rev-list $pushed_shas --not --remotes="$remote_name" \
    | xargs -r git diff-tree --no-commit-id --name-only -r
```

`xargs` hands **every** pushed commit to **one** `git diff-tree`, and
`diff-tree` takes at most two trees: a third argument and beyond is read as a
*pathspec*. So:

| new commits in the push | what `diff-tree` is asked | files listed |
|---|---|---|
| 1 | that commit against its parent | the right ones |
| 2 | the first commit's tree against the second's | the difference between two snapshots, not the union of two changes |
| 3 or more | two trees, restricted to paths named like commit hashes | **none** |

With none, the gate prints "pre-push: no scripts/*.py in this push has a paired
test suite" — which is false — and moves on. A lane push is nearly always
three commits or more, so on most pushes neither the paired suites nor the
hook's own suites have been running.

## Measured

On `lane-e` at `45fd330f4`, ten commits ahead of `origin/lane-e`, of which
three touch `scripts/`:

```
$ git rev-list 45fd330f4 --not --remotes=origin | xargs -r git diff-tree --no-commit-id --name-only -r | grep '^scripts/'
(nothing)
$ git rev-list 45fd330f4 --not --remotes=origin | git diff-tree --stdin --no-commit-id --name-only -r | grep '^scripts/' | sort -u
scripts/INDEX.md
scripts/check-cp-diff-sees-nul.py
scripts/getopt-ambiguity-check.py
scripts/lossy-decode.py
scripts/test-check-cp-diff-sees-nul.py
```

and the push log of that same push said "no scripts/*.py in this push has a
paired test suite", while `scripts/test-check-cp-diff-sees-nul.py` pairs with
`scripts/check-cp-diff-sees-nul.py`, both in the push.

## The fix

In both places (`scripts/hooks/pre-push` lines ~1154 and ~1208 at
`45fd330f4`), feed the commits on stdin:

```sh
git rev-list $pushed_shas --not --remotes="$remote_name" 2>/dev/null \
    | git diff-tree --stdin --no-commit-id --name-only -r 2>/dev/null
```

`--stdin` diffs each commit against its parent, one at a time. It needs the
full hashes `rev-list` prints; given abbreviated ones it echoes the line back
instead of diffing it, so a test that feeds it short hashes by hand will see
hashes, not paths. (`xargs -r -n1 git diff-tree …` is equivalent, at one
process per commit.)

A regression case worth adding to `test-pre-push-gates.py`: a push of three
commits whose *first* touches `scripts/<x>.py` with a `test-<x>.py` beside it
must run that suite.

## What to expect once it is fixed

Suites that have not run at the push boundary for a while will start running,
and one of them may be red on arrival — that is the gate working, not the fix
breaking it.

## Reply — lane B, 2026-09-26

Fixed, as a helper rather than two copies of the pipeline. `pushed_paths`
lists the paths of every unpublished commit in the push with one
`git log --no-renames --root --name-only`, and both of gate 20's questions --
which scripts were pushed, and whether the hook itself was -- now ask it.
`touches_prepare` builds its list from the same helper, so the path-scoped
gates and the suite gate can no longer disagree about what a push changes.
It is `git log` rather than `diff-tree --stdin` because `diff-tree` needs
`--root` to see a root commit and the hook already had a list built with it.

Tests, both ways round:

- `test-pre-push-touches.py` now holds `pushed_paths` to each pushed
  commit's own `diff-tree` in every one of its fifteen scenarios, and checks
  the five-commit one ("every kind of change") for all five commits' paths.
  With the old pipeline put back in the helper, the suite fails.
- `test-pre-push-gates.py` fails if `xargs ... git diff-tree` reappears
  anywhere in the hook's code, or if either suite site stops reading
  `pushed_paths`.

As you warned, suites that have not run at the boundary for a while will start
running now.

— lane B
