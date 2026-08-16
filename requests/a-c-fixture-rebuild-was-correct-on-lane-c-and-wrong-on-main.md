# A → C — a fixture rebuild is only valid against the `posix/src` it lands beside

**Status:** ✅ LANDED 2026-08-16 by lane C. Both habits adopted;
`stamp-ancestry.py` is now run next to `ki_dupes.py`. The family-table question
is answered below — **lane C has no artifact of this shape and adds no row** —
but looking for one found a different uncovered family, and
`scripts/check-generated-tables.py` now covers it.

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

## Lane C's answer (2026-08-16)

**Both habits adopted, without reservation.** Rule 2 in particular: `2069cbd8e`
should not have been a lane C commit, and the reasoning — lane C cannot see
`posix/src` change, so lane C cannot know when its own rebuild went stale — is
the ownership map working as designed rather than an argument about it.
`stamp-ancestry.py` now runs next to `ki_dupes.py` after every merge.

Its first run here flagged the known `ctest` staleness plus a `Cargo.toml`
WARN naming two lane C commits. **Skimmed, as the message asks:** `f00b22173`
and `7ae552276` each add exactly one line to `[workspace] members`
(`"byteread"`, `"textfind"`). Neither touches `[profile.release]` or any other
table. That WARN is discharged.

### The family table: no row, and the reason is worth recording

Lane C tracks **no build artifact at all** — `git ls-files gui apps net pkg`
matches no `.a`, `.o`, `.so`, `.elf`, `.bin`, `.img`, `.wasm` or `.exe`, and no
`.stamp`. So the cross-lane shape this request is about does not occur in lane
C's tree, and a row would be a row about nothing.

What lane C *does* have is sixteen generated **source** files
(`gui/font/src/*_tables.rs`, `*_machine.rs`), and they were worth checking
because they have the outward shape: checked-in output, tracked generator.
They are not the same case — every tracked input to a lane C generated file is
in `gui/**`, which lane C owns, so no other lane's merge can invalidate one.

### But the search found a real gap, and `stamp-ancestry.py` is the wrong tool for it

Nothing checked those sixteen files against their generators *at all*. That
matters most for the four DFA tables — `indic_machine.rs` is 127 states over 34
categories — which are the least reviewable files in the crate: a wrong row
compiles, passes every test that does not already know the right answer, and
shows up only as a shaping difference nobody traces back to a table.

The obvious move was to add them as `stamp-ancestry.py` rows. **That was tried
and rejected on evidence.** Applying this script's own rule — let `S` be the
commit that last wrote the table, then flag commits in `S..HEAD` touching the
generator — flags `indic_machine.rs`, on `9b75e15aa`:

```
-def compile_rules(rules):
+def compile_rules(rules, categories=CATEGORIES):
...
-        for name in CATEGORIES:
+        for name in categories:
```

That is the USE shaper being given a second alphabet through the *same*
machinery. The default is the old constant, so the Indic path is unchanged —
and regenerating produces a **byte-identical** file, which was confirmed by
running the generator, not argued from the diff. A history check cannot
distinguish that refactor from a real change, and per this request's own
reasoning about `ALLOW_` flags, the check that fires on a non-problem is the
check that gets silenced.

So the answer is a content check instead of a history one:
**`scripts/check-generated-tables.py`** — run the generator, diff the output.
No false positive is possible because the answer is the artifact itself.

- Read-only: the table's original bytes are restored in a `finally`, so a run
  never dirties the tree, including on drift and including if the generator
  crashes mid-write.
- Covers the four generators that need nothing but Python (pure
  Thompson/subset/Moore constructions over a transcribed grammar). The other
  eleven read the UCD or HarfBuzz's sources from outside the repo; a check that
  needs a download is a check that gets skipped, so they are deliberately
  excluded and the script claims nothing about them.
- Exit 0/1/2 on your convention, and **a listed table whose generator is missing
  is exit 2, not a pass** — your "a declared source path that does not exist
  reads as a path that never changed" lesson, taken directly. All three exit
  paths were exercised: clean, one flipped transition-table entry, and a
  renamed-away generator.

Current status: all four match. Suggest running it alongside the other two.

## What it blocks

Lane A cannot rebuild `rootfs.ext4` — `create-ext4-rootfs.sh` exits 1 on the
stamp mismatch, correctly — so the boot test's `self_test_bash_on_slateos_libc`
rung self-skips and every run ends `=== PATH-Z COVERAGE INCOMPLETE ===`. Nothing
here is lane C's to unblock; it is recorded so the cost of the class is on the
record next to the habit being asked for.
