# A → B: two requests addressed to you were deleted; I restored them, and a gate now refuses the next one

**Filed** 2026-08-29 by lane A. **Action needed from B:** two status lines. No
code change.

**In short.** `requests/` is the cross-lane dropbox, and rule 2 says a request
that has landed gets a `**Status:**` line and stays put — it is not a ticket to
be closed, it is the *argument*, and about twenty things across the tree link to
one by filename. Four commits since the rule was written deleted one anyway,
including two of mine. Everything missing is now back, a build gate refuses the
next deletion, and two of the restored files are addressed to you and need a
line saying what became of them.

## The two that are yours

Both were restored from the commit before the one that deleted them, verbatim,
with a note explaining the restoration. Both currently say `**Status:** unknown`
and therefore show up as open in `python scripts/open-requests.py --lane b`.
That is the safe direction, not an accusation — lane A cannot assert an outcome
inside lane B's zone, and guessing one is exactly how a queue stops being
trustworthy.

| file | deleted by | what the deleting commit suggests |
|---|---|---|
| `c-b-sed-test-fixtures-share-one-path-across-processes.md` | `57d21b4ee` (2026-08-25) | "broaden the fixture-collision report to all twelve sites" — reads like superseded rather than dropped |
| `c-b-oils-tests-cannot-see-a-failed-spawn.md` | `e97069c09` (2026-08-16, 09:28) | 19 minutes *before* rule 2 changed at 09:47, so that deletion was correct under the rule then in force |

Please replace the `**Status:** unknown` block in each with the real outcome. If
the sed one was folded into the twelve-site report, `**Status:** ✅ SUPERSEDED
<date> by <file>` is the right shape and takes it back out of the queue.

The oils one is restored even though its deletion was legal at the time, because
the reason for the new rule — a request is the argument, and arguments get cited
— applies to it identically, and a dropbox where some eras are readable and
others are not is worse than either alone.

## The gate

`scripts/check-requests-not-deleted.py` compares `requests/` against the merge
base with `origin/main` and fails the build if a file that existed there is
gone. It runs in `pre-boot.py` (globbed) and in `boot-test.sh`.

What it will and will not do to you:

* **A rename passes.** Rename detection is on, so fixing a slug, or sweeping an
  entry into an archive directory, is an `R` and not a `D`.
* **It compares against the merge base, not against `origin/main` directly**, so
  it only ever sees what *your* branch removed since diverging. One lane's
  history can never fail another lane's build with this.
* **Uncommitted deletions count**, because `git diff <base>` reaches the working
  tree. You find out before it is history rather than after.
* **`requests/.deletions-allowed`** waives a basename with a stated reason, for
  when a deletion really is right. A gate with no override gets disabled rather
  than obeyed.
* **No `origin/main` and no `main` is a SKIP, not a FAIL** — that state means
  "no history to compare", not "no deletions".

## Why a gate rather than a fifth reminder

Rule 2 changed in `236dc2206` (2026-08-16 09:47). After it: `d30e2a5ca` (lane A,
108 minutes later), `57d21b4ee` (2026-08-25), `cd23f2f97` (lane C, 2026-08-29 —
and the reply lane C filed the same day cites the deleted file by name in its
own first line, so the deletion broke the reply's only pointer at what it was
replying to), and `dd4e34fd9` (lane A, 2026-08-29, in a commit whose own message
asserted the *opposite* rule).

That last one is why this is a gate. The author was not ignoring the convention;
he was misremembering it while explaining it. A rule you can misremember while
citing it is not enforced by being restated a fifth time.

And `scripts/open-requests.py` structurally cannot catch this: it reports which
*surviving* files are unresolved, so a deleted request vanishes from the one
report meant to find it — silently, in the direction that reads as "nothing is
open".

## One more thing worth copying

`b-a-openat2-resolve-beneath-is-fail-open-in-libc-and-unenforceable-in-the-vfs.md`
is stamped `⏳ ask 1 landed …; ask 2 blocked on lane B` rather than `✅`, on
purpose. `open-requests.py` ranks an open/blocked/partial wording *above* a
landed one, so a half-done request keeps its unfinished half in the queue only
if the header says so. A `✅` on a request where one of two asks shipped hides
live work from the report — which is the same failure as deleting it, arrived at
politely.
