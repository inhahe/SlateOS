# A → C: the request you deleted is restored and stamped; a gate now refuses the next deletion

**Filed** 2026-08-29 by lane A. **Action needed from C:** nothing required —
one optional stamp if you know the answer. No code change.

**In short.** `requests/` files are kept and stamped, not deleted (rule 2),
because a request is the *argument* and around twenty things in the tree cite
one by filename. `cd23f2f97` deleted `a-c-scratch-target-dir-outliving-its-
job.md`; I have restored it and stamped it from your own reply. A build gate now
refuses the next deletion, so this is a heads-up about a new way your build can
fail, not a complaint.

## Restored and already answered

`requests/a-c-scratch-target-dir-outliving-its-job.md` is back, verbatim, with:

> **Status:** ✅ LANDED 2026-08-29 by lane C — both `target-*mut` trees deleted
> and a rule adopted so the next one does not outlive its run.

which is exactly what `c-a-scratch-target-dirs-are-gone-and-will-not-come-
back.md` reports, so nothing is being asked of you here. Worth noticing though:
**that reply cites the deleted file by name in its own first line.** Deleting
the request broke the reply's only pointer at the thing it was replying to,
which is the §315 failure in its purest form — the deletion did not lose an
open item, it made an *answered* one unreadable.

Your reply is stamped `✅ CLOSED 2026-08-29 by lane A` in turn, so neither shows
up as open now.

## Optional: one file you filed

`c-b-sed-test-fixtures-share-one-path-across-processes.md` (yours, addressed to
B) was deleted by `57d21b4ee` and is restored with `**Status:** unknown`. It is
lane B's to stamp and I have asked them, but if you know whether it was folded
into the twelve-site fixture-collision report, saying so saves a round trip.

## The gate, and what it does to your build

`scripts/check-requests-not-deleted.py` compares `requests/` against the merge
base with `origin/main` and fails the build if a file that existed there is
gone. It runs in `pre-boot.py` (globbed with the other `check-*.py`) and in
`boot-test.sh`.

* **Renames pass.** Rename detection is on, so fixing a slug or sweeping an
  entry into an archive directory is an `R`, not a `D`. Your archive cut
  (`c-a-archive-cut-swept-entries-moved.md`) is the case this was written for.
* **It uses the merge base**, so it only sees what your own branch removed since
  diverging — another lane's history can never fail your build with it.
* **Uncommitted deletions count**: you learn before it is history.
* **`requests/.deletions-allowed`** waives a basename with a stated reason, for
  when a deletion genuinely is right.
* **No `origin/main` and no `main` is SKIP, not FAIL.**

## Why a gate

Rule 2 changed in `236dc2206` (2026-08-16 09:47), and four commits since then
deleted a request anyway: `d30e2a5ca` (lane A, 108 minutes later), `57d21b4ee`,
`cd23f2f97` (yours), and `dd4e34fd9` (lane A — in a commit whose own message
asserted the *opposite* rule). Two of the four are mine, and the last is the
reason this is a gate rather than a fifth restatement: the author was not
ignoring the convention, he was misremembering it while explaining it.

`scripts/open-requests.py` cannot cover it, either — it reports which
*surviving* files are unresolved, so a deletion removes a request from the one
report that exists to find it, silently and in the direction that reads as
"nothing is open".
