## TD-A-BOOT-HISTORY-IS-A-TRACKED-FILE-EVERY-BOOT-DIRTIES (lane A, 2026-08-24) — **open**

**Still open 2026-09-10, with fresh first-hand evidence of the smaller cost.**
In one session lane A ran five boot tests and had to make *two separate commits*
whose entire content was `bench/boot-history.jsonl` -- one of them literally
titled "bench: boot-history row from the run that validated 1072/1073" -- plus
a third where the ledger rode along inside an unrelated kernel commit because it
was dirty when that commit was made. None of those is the data-loss hazard this
entry was filed for; they are the everyday version of it, and they are why the
hazard keeps being reachable: the file's normal state is dirty, so clearing it
never looks destructive, and committing it is a chore that attaches itself to
whatever commit happens to be next.

The first proposed fix below -- have `boot-test.sh` commit the row itself, as its
own single-file commit -- removes all three. Noting also that the harness already
works around the symptom: `boot-test.sh` excludes this path from its own
dirty-tree check (`':(exclude)bench/boot-history.jsonl'`), which keeps the run
from complaining but leaves every other git command exposed.

`bench/boot-history.jsonl` is committed to git *and* appended to by every run of
`scripts/boot-test.sh`. So a boot always leaves the worktree dirty in a tracked
file that all three lanes write, and the record of a run exists only in the
working tree until someone remembers to commit it.

That is structurally exposed to ordinary git hygiene. On 2026-08-24 a
`git checkout -- bench/boot-history.jsonl`, run to clear the dirty file before a
merge, discarded two unrecorded boots — including the **passing** one that had
just turned the tree green. `git checkout --` restores from the index, and for a
file whose only copy of the new lines is the working tree, that is a delete.
Nothing warned, because a dirty ledger is the file's normal state.

The entries were not reconstructed. `boot-history.py` can re-read a surviving
serial log, but it computes `src_digest` from the tree *as it is now* — which by
then was mid-merge and 26 commits removed from what was booted. A ledger entry
whose digest describes a different tree is worse than an absent one: it is
evidence that reads as true. Two data points out of 387 are missing instead.

### What the proper fix looks like

Either of these removes the hazard; the first is smaller and probably right:

- **Have `boot-test.sh` commit the record itself**, as its own single-file
  commit, immediately after appending. The ledger is append-only and per-run, so
  such commits never conflict textually — this is the same reasoning that makes
  the shared docs' append convention merge cleanly. The worktree then returns to
  clean on its own and no later git command can eat the entry.
- Or **untrack it** and keep the history outside the repo. Rejected on sight:
  the streak and wall-time medians are cross-lane and cross-machine, which is
  exactly what a tracked file gives for free.

### Until then

Never run `git checkout --`, `git restore`, or `git stash` against
`bench/boot-history.jsonl`. If it is dirty before a merge, **commit it**, do not
clear it.
