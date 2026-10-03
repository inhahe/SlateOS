## 723. `rm` walks the tree itself instead of calling `remove_dir_all`, and answers each entry with a three-way verdict rather than a boolean

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B
**Where:** `userspace/coreutils/src/bin/rm.rs` — `Rm::operand`, `Rm::entry`,
`Rm::remove_tree`, the `Verdict` enum, `is_write_protected`; and
`scripts/rm-diff.sh`.

**In short:** `rm` deletes files. Ours previously handed a whole directory to
the Rust standard library's `remove_dir_all` and let it do the work. That is
one function call, and it is why `rm -rf .` emptied the current directory and
`rm -rf /` was not refused: a library function that deletes a tree has no
opinion about *which* trees a user should be stopped from deleting, cannot ask
before each file, cannot print each file as it goes, and cannot report which
particular file it failed on. Everything `rm` is expected to do beyond
"delete", it could not do. So the walk is now written out by hand. This entry
records that choice, the three-way answer each step of the walk returns, and
the one place where the hand-written version is *worse* than what it replaced.

### Decision 1: the recursive walk is written here, not delegated

*What changes:* `rm -rv dir` names every file as it removes it, deepest first;
`rm -ir dir` asks before descending and again before removing; a failure deep
in the tree names the file that failed and the rest of the tree still goes.
None of that was possible before.

- **For:** every behaviour that distinguishes `rm` from "delete this tree"
  lives *between* the steps of the walk — the `.`/`..` refusal and the root
  failsafe before the first step, the prompt and the `-v` line at each step,
  `--one-file-system`'s device check on each descent, and the ancestor
  bookkeeping after a step declines. A delegated walk offers no such seam. The
  two data-loss defects were not oversights on top of `remove_dir_all`; they
  were the direct consequence of using it.
- **Against:** it is roughly 300 lines where there was one call, and it
  re-implements things the standard library had already got right — including
  gnulib's trailing-slash arithmetic, which is fiddly enough to have its own
  tests (`joining_drops_one_trailing_slash_only`,
  `trailing_slashes_collapse_to_one_but_not_to_none`).
- **And one real loss, not merely more code:** `remove_dir_all` on Linux walks
  with `openat`/`unlinkat` relative to a directory *descriptor*, so a component
  of the path cannot be swapped for a symlink underneath it mid-walk. This walk
  is path-based — it builds `parent/child` and calls `remove_file` on the whole
  string — so it is open to that race, which is a real one with a long history
  in `rm` specifically. GNU avoids it with `fts`'s `FTS_CWDFD`. Logged as
  `TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`, with the
  fix (a descriptor-relative descent) described there.
- **Why the "for" wins anyway:** the race needs a local attacker with write
  access to a directory inside the tree being removed *while* it is being
  removed. The defects it replaces needed a user to type `rm -rf .`. Trading a
  conditional, contested vulnerability for two unconditional ones is the right
  direction, and the descriptor-relative rewrite is an improvement to this walk
  rather than a reason to go back to not having one.

### Decision 2: an entry's removal answers `Removed`, `Declined` or `Abandoned`

*What changes:* declining to descend into a directory exits **0** and says
nothing further; declining a single file inside it exits **1**, because the
parent then fails its `rmdir` with `Directory not empty`. A boolean would make
those two the same, and one of them would be wrong.

`Verdict` is gnulib's `mark_ancestor_dirs` in a different shape. When a
subtree is not fully removed, the enclosing directories must be skipped — but
*how* they are skipped depends on why:

| why the subtree survived | ancestors | status |
|---|---|---|
| the user said no to descending | skipped in silence, no `rmdir` attempted | 0 |
| something failed and was reported | skipped in silence, no second message | 1 (already earned) |
| the user said no to one child | `rmdir` still attempted, and fails aloud | 1 |

- **For:** it is measured behaviour, all three rows of it, and no simpler model
  reproduces the table. The third row is the surprising one and is exactly what
  a boolean gets wrong.
- **Against:** three states where two would compile, and the distinction
  between `Declined` and `Abandoned` is invisible in the code that consumes it
  until you read the table above.
- **Why the "for" wins:** the alternative is not "simpler", it is "wrong in one
  of three cases", and the wrong case is a silent exit-status bug — the kind
  that a script notices and a human does not.

### Decision 3: a `euidaccess` failure that is not `EACCES` means "not write-protected"

*What changes:* in a corner where GNU prints a second diagnostic and skips the
file, ours proceeds to remove it and reports whatever the removal itself hits.

The prompt says `remove write-protected regular file 'x'?` when the file cannot
be written. GNU distinguishes `EACCES` (write-protected — ask) from any other
`faccessat` failure (an error in its own right — report it and skip the entry).
Ours treats everything that is not a plain success as "not write-protected".

- **For:** the removal is about to run anyway and will report the real problem
  with the real `errno`. Inventing a diagnostic here can only turn a file that
  `rm` would have removed into one it refuses — a failure in the direction of
  not doing the job.
- **Against:** it is a divergence from GNU, and an unmeasured one: the harness
  has no way to make `faccessat` fail with something other than `EACCES` on a
  file that then removes cleanly, so this is reasoned rather than certified.
- **Why the "for" wins:** the divergence is confined to a case nobody could
  construct, and it errs toward doing what was asked. The opposite error —
  refusing to remove a file because a *permission query* failed oddly — is
  worse and harder to explain.

### Decision 4: the differential harness never recursively removes `/`

*What changes:* `scripts/rm-diff.sh` carries `rm /` and `rm -d /` but no
`rm -rf /`. The recursive failsafe is certified by a unit test instead.

- **For:** the harness runs as a normal user inside WSL, where `/mnt/d` is the
  repository and the whole of the D: drive. A `rm -rf /` case is safe exactly
  as long as the failsafe works — which is the thing it is meant to test — so
  its failure mode is destroying the tree that contains the evidence. The unit
  test `a_recursive_operand_that_is_the_root_is_refused` points `Rm::root` at a
  scratch directory and tests the same comparison with nothing at stake.
- **Against:** the unit test can only certify *our* behaviour. The exact
  wording of the two-line refusal (`it is dangerous to operate recursively on
  '/'` / `use --no-preserve-root to override this failsafe`) and the `(same as
  '/')` clause for `//` are therefore transcribed from measurement and frozen
  in a unit test, not diffed against GNU on every run like everything else.
- **Why the "for" wins:** a test that can delete the operator's disk is not
  made acceptable by a high probability of passing. If the wording is ever
  worth certifying properly, the way to do it is a `chroot` under
  `unshare --map-root-user`, where `/` is a scratch directory with six copied
  files in it — not by pointing the real thing at the real root.
