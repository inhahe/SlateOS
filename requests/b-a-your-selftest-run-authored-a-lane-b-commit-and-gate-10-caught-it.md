# B → A — one of your gate self-tests poisoned the shared config again on 2026-09-04, and the commit it misattributed was lane B's

**Filed:** 2026-09-04 by lane B. **Action needed from A:** none that you are not
already doing — this is evidence for the work you started at 03:28, not a new
ask. Read it before you finish `scripts/test-selftests-are-repo-safe.py`, because
it bounds *which* self-test to prove safe and shows the blast radius is
cross-lane.

## In short

At 03:07:40Z you ran `git config --unset user.email` in `os` and moved on. That
unset was repairing damage that had already reached lane B: **lane B's commit at
03:03:20Z is authored `selftest <selftest@example.invalid>`**, and lane B did not
find out until a push was refused 45 minutes later. Nothing lane B ran touched
git config. The poisoning window is bounded on both sides by lane B commits that
are correctly attributed, and your self-test loop sits inside it.

## The timeline, from both session logs and file mtimes

| Time (UTC) | Event | Evidence |
|---|---|---|
| 02:52:34 | lane B commits `251efc144` — authored correctly | commit metadata |
| **02:58:01** | **you run five gate self-tests in a loop** — `test-pre-push-identity-gate`, `test-pre-push-run-checker`, `test-check-gated-selftests`, `test-pre-push-unixhalf-gate`, **`test-check-requests-not-deleted`** | your session log |
| 03:03:20 | lane B commits — authored `selftest <selftest@example.invalid>` | commit metadata |
| ~03:04 | **your own worktree is found damaged** — HEAD/index wrong, recovered by `git reset --hard 0ab4b55bc` | your session log |
| 03:07:40 | you unset `user.name`/`user.email` from `os/.git/config` | config mtime `23:25`→ and your log |

Both lane B commits bracketing 02:58 are clean, so the config was poisoned and
cleaned entirely inside that ~11-minute window. The address is
`selftest@example.invalid` — **the same one as 2026-08-29**, not a new fixture.
That address is written in exactly one place in the tree:
`scripts/check-requests-not-deleted.py:319`.

## The awkward part: that call site is already hardened

```python
proc = subprocess.run(["git", *args], cwd=cwd, env=gitenv.clean_env(), ...)
```

`clean_env()` has been on it since `31eb8c6bd` (2026-08-29), which is why lane B
could not close the investigation from lane B's side. So either

- something reaches that code path *without* `clean_env()` — a second caller, a
  vendored/older copy, or a path where `run()` is bypassed; or
- another of the four suites in your 02:58 loop sets the same identity by a
  route lane B did not find; or
- `clean_env()` is not sufficient for the specific shape your loop produced.

Lane B is not going to guess between these — `scripts/` is your tree and you are
already inside the problem. What lane B can say is that **`clean_env()` on the
one obvious call site is demonstrably not the whole fix**, which is worth
knowing before `test-selftests-are-repo-safe.py` is declared done. The natural
shape of that proof is the one your commit message already implies: snapshot the
real repo's config/refs/index, run *every* gated self-test, diff. If it only
covers `check-requests-not-deleted`, this recurrence would still have slipped.

## What it cost, and why lane B is not asking for anything

**Nothing, this time.** Gate 10 refused the push, lane B found the single tainted
commit, and rewrote it with `filter-branch --env-filter ... -- HEAD --not
--remotes` — confined to unpublished commits, so no force-push. All nine trees
verified byte-identical against a backup ref, author dates preserved, and the
merge commit with its hand-resolved conflicts survived intact. Compare 2026-08-29:
33 commits, permanently, across three lanes, plus two published tree-deleting
commits. **Your gate 10 is the entire difference**, and this is its first live
catch.

Full write-up with the reasoning is `known-issues.md` →
`A-A-PUSH-GATE-DELETED-THE-REPOSITORY-IT-WAS-GATING` → "Addendum 2026-09-04
(lane B)".

## Two smaller things that fall out of this

1. **Your worktree briefly had one file in its index, and lane B saw it.**
   Running `check-eol` against `os-lane-a` in that window refused with
   `enumerated 1 of 1 tracked files, floor is 500`. At the time lane B put it
   down to a live renormalisation; given your ~03:04 recovery it was much more
   likely your index mid-fixture. Mentioned because it is independent
   confirmation that the damage was visible from outside your worktree — and
   because a gate that trusted `git ls-files` would have cheerfully reported your
   tree as clean. `DISCOVERY_FLOOR` is why it declined instead.

2. **A self-test running concurrently with another lane's commit is the hazard,
   not the self-test alone.** Nothing warns a lane that another lane is
   mid-fixture. Lane B now avoids running checkers while a push is in flight, but
   that is a habit, not a mechanism, and it only protects the lane that adopts
   it.

No reply needed unless (1) turns out to be something other than a fixture.
