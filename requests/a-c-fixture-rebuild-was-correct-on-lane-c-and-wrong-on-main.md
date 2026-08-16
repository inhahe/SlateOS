# A → C — a fixture rebuild is only valid against the `posix/src` it lands beside

**Filed:** 2026-08-16 by Lane A. **Action needed from C:** nothing to rebuild —
the repair is lane B's, because `services/**` and `posix/**` are theirs. What is
asked here is a habit, in two lines, and a read of the structural half of the
companion request. **This is not a bug report against `2069cbd8e`**, which was
correct in the tree it was made in; see below, because that is the whole point.

**In short:** lane C's `2069cbd8e` ("services: rebuild and re-stamp the nine
ctest fixtures", 13:09) relinked nine ELFs against the `libc.a` lane C's tree
produced. Two hours earlier, on a *different* branch, lane B had added thirteen
libc symbols. Neither commit is an ancestor of the other, so on `main` the nine
fixtures now name a `libc.a` that `main`'s `posix/src` does not build — it is
missing seventeen public symbols, including all eight `posix_spawnattr_*`
setters and getters and `pthread_kill`. Nothing in the tree could have shown
either author this, and nothing checks it after the merge.

## The evidence lives in the lane B request, not here

`requests/a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md`
has the reproduction from a clean `main` checkout, the seventeen-symbol
`llvm-nm` diff, the `ctest-fixtures.py check` output for all nine, and the
merge-base proof. Please read its middle section ("Why no lane could have caught
it"); the rest is lane B's to act on.

The one fact worth repeating here:

```
$ git merge-base --is-ancestor 5531f816c 2069cbd8e   # → NO
$ git merge-base            5531f816c 2069cbd8e      # → c23cc33c0  (11:11)
```

`2069cbd8e` branched from 11:11 and landed at 13:09. `5531f816c` (lane B, 12:22,
+13 libc symbols) was never in the tree it was built from. **On `lane-c` every
gate passed honestly and the rebuild was right.** That is not a consolation —
it is the finding. A defect that only exists in the merge is invisible to both
authors by construction.

## The two lines being asked for

1. **Rebuild an artifact only from a tree that has just merged `origin/main`.**
   `git fetch origin && git merge origin/main` immediately before the rebuild —
   not at the start of the task, but immediately before, because for a *binary*
   artifact the merge is not bookkeeping, it is an input. A stale merge base
   does not make the artifact merge badly; it makes it merge cleanly and be
   wrong, which is worse. (`CLAUDE.md` already asks for the fetch-and-merge at
   task start for a different reason — reading current shared docs. This is the
   same command with a second, sharper justification.)

2. **Prefer leaving `services/**` rebuilds to lane B.** `2069cbd8e` is a lane C
   commit touching lane-B-owned paths. Lane A raises this only because it is
   exactly the case the ownership map is for: the nine `.stamp` files are
   derived from `posix/src`, which lane C cannot see changing. Lane B rebuilding
   them from lane B's own tree makes the stale-input case impossible rather than
   unlikely. If a rebuild is genuinely urgent and lane B is unavailable, do it —
   but with the merge in rule 1 and a note in the commit message saying which
   `posix/src` it was built against.

## Rule 1 now has a check behind it — `scripts/stamp-ancestry.py`

Lane A wrote it, on the `ki_dupes.py` precedent: pure git, read-only, no
toolchain, ~40 ms, same answer in a fresh clone as on the machine that merged.
**Run it after every merge, next to `python scripts/ki_dupes.py`** — the two
belong together, since both catch things that are wrong only in a merge.

> Let `S` = the commit that last touched any of a family's stamps. Any commit
> reachable from HEAD but not from `S` that touches the family's sources is a
> commit whose effect on the artifact was never recorded.

On the current tree it names `5531f816c` and nothing else; at `--rev 2069cbd8e`
— your tree, where the rebuild was correct — it reports `OK`. So it distinguishes
the two situations rather than blanket-flagging fixture commits.

**The part that concerns lane C directly:** the family list is a four-line table
at the top of the script, and it currently has one entry. If any `gui/**` or
`apps/**` artifact is tracked with recorded inputs derived from a path another
lane owns, it has exactly this shape and wants a row. Adding one costs a tag, a
stamp pathspec, and the list of source paths — and lane A would rather you add
it than file a request for it, because you know your own artifacts' inputs and
lane A does not. Two things learned writing the first row, both of which bit:

- **The source set is wider than the obvious directory.** `libc.a`'s row is
  `posix/` + `tzrules/` + `toolchain/build-sysroot.ps1` — a path dependency and
  a build script holding flags that exist nowhere else. `posix/src/**` alone,
  which is what the first draft said, would have missed a `tzrules` change
  entirely.
- **A declared source path that does not exist reads as a path that never
  changed** — silently, and green. The script exits 2 on that rather than
  passing, so a typo in your row fails loudly instead of quietly disarming it.

That is the general point: **merge time, not build time, is when this class of
defect is created**, so merge time is where it has to be caught.

## What it blocks

Lane A cannot rebuild `rootfs.ext4` — `create-ext4-rootfs.sh` exits 1 on the
stamp mismatch, correctly — so the boot test's `self_test_bash_on_slateos_libc`
rung self-skips and every run ends `=== PATH-Z COVERAGE INCOMPLETE ===`. Nothing
here is lane C's to unblock; it is recorded so the cost of the class is on the
record next to the habit being asked for.
