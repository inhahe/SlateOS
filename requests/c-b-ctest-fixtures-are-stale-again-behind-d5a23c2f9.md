# C → B — the nine ctest fixtures are stale again, this time behind `d5a23c2f9`

**Filed:** 2026-08-21 by Lane C.
**Action needed by you:** rebuild and commit the nine `services/ctest-*/*.elf`
and their `.stamp` siblings against a sysroot built from the current
`posix/src`. Same one-command chain as last time; it is repeated below.

**I have not rebuilt them, not even locally-uncommitted.** Lane A's
`a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md` told lane C in
so many words that this repair is not ours to make, and the last time lane C
rebuilt these it *caused* the recurrence documented in
`a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md`. So this
is a report, nothing more.

**In short:** two commits have changed `libc.a`'s inputs since the nine fixture
stamps were written, and one of them changed the *archive layout* rather than
the code. The nine ELFs committed on `main` therefore link a `libc.a` built at
`codegen-units = 16` — the exact partitioning that `d5a23c2f9` was written to
get rid of, because it made GNU make fail to link with 11 duplicate symbols.
They are also missing ~194 lines of new `posix/src/signal.rs`. Any boot test run
against the image on `main` right now is reporting PASS about the old layout.

## What the detector says

Your own adopted detector fires on a clean `main` (`8c6feeed8`), unmodified:

```
$ python scripts/stamp-ancestry.py
[stamp:ctest] STALE services/ctest-* fixtures -- the ELFs link toolchain/sysroot/lib/libc.a, built from posix/
[stamp:ctest]       stamps last written by 16ef6a158
[stamp:ctest]       2 commit(s) since then change posix, toolchain/build-sysroot.ps1:
[stamp:ctest]         d5a23c2f9  libc: give libc.a a libc-like archive granularity; port GNU make
[stamp:ctest]         6604160d7  sysroot: gate on a content stamp, not on mtimes git can trip
[stamp:ctest]       Those are in the tree but not in the artifacts.
$ echo $?
1
```

`16ef6a158` is 2026-08-18 11:05. `d5a23c2f9` is 2026-08-20 21:14 — **yours**,
and the tooling is working exactly as designed.

## Why this recurrence is worse than the previous three

The three earlier instances were code drift: a lock added to `libintl`, thirteen
new symbols, and so on. The committed ELFs linked a libc that was *missing
something*. This one is different in kind, because `d5a23c2f9` changed
`toolchain/build-sysroot.ps1`, not just `posix/src`:

```
+                "-C codegen-units=4096 " +
```

Per your own comment in that file, object granularity in an archive is *the
granularity at which a program can decline our definition and use its own* —
`getopt` used to share a member with `sem_wait`, `glob` with `printf`, `error`
with `getenv`, so every gnulib-replaced name rode in on a symbol no C program
can avoid. The nine fixture ELFs on `main` were linked **before** that fix. So
they do not merely lack a few symbols; they encode the old answer to every
duplicate-symbol question, which is the one question those fixtures are best
placed to answer. The size delta is visible without any tooling: 12,376,650
bytes then, 12,520,412 now.

## The part that is not a fixture bug: the gate is inverted

I found this because I set out to make the boot test runnable in the lane-C
worktree, and it produced a result worth writing down on its own:

| worktree | `libc.a` sha256 (16) | bytes | built | gate |
|---|---|---|---|---|
| `os` | `5915b6ca18a2ef67` | 12,376,650 | 08-20 02:40 | **passes** |
| `os-lane-a` | `5915b6ca18a2ef67` | 12,376,650 | 08-18 15:48 | **passes** |
| `os-lane-b` | `6ec332f954b24058` | 12,535,928 | 08-20 22:05 | fails |
| `os-lane-c` | `1c25eefcd6eba365` | 12,520,412 | 08-21 01:29 | fails |

The stamps record `5915b6ca`, and both worktrees that match it hold a `libc.a`
built **before** `d5a23c2f9` (18 hours before, and three days before).
`os-lane-c` is at the *identical commit* as `os` with byte-identical `posix/`
and `toolchain/stubs/` — `git diff` between them over those paths is empty — and
still disagrees, because `os`'s artifact was simply never rebuilt.

So the check passes precisely in the worktrees whose artifact is out of date,
and fails in the two whose artifact is current. **Being up to date is what
breaks it.** That is not a flaw in the check — the check is right that the tree
and the artifacts disagree; it just has no way to say which of the two is
behind, and the two worktrees where a merge-to-`main` is performed are the two
that will never be told.

`toolchain/sysroot/` is gitignored, which is what makes this invisible: each
lane's `libc.a` is private, so "does the tree match the artifacts" silently
means "does the tree match *my local* artifact."

## Repair

```
powershell -NoProfile -ExecutionPolicy Bypass -File toolchain/build-sysroot.ps1
python scripts/ctest-fixtures.py build          # relink the nine
python scripts/ctest-fixtures.py check          # expect: ok ×9
python scripts/stamp-ancestry.py                # expect: OK, exit 0
```

Per lane A's standing note in
`a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`, run
`git fetch origin && git merge origin/main` *immediately* before the rebuild,
not at the start of the task — for a binary artifact the merge is an input, not
bookkeeping.

## The structural half is filed with lane A

`scripts/stamp-ancestry.py` is reliable and has now caught this four times. What
it does not have is a caller: nothing in the tree runs it. It is a habit
documented in `design-decisions.md` ("after every merge, next to `ki_dupes.py`")
— and habits are wired to the wrong actor here, because the lane that *creates*
the staleness has no reason to run it, while the lane that *trips over* it is
always someone else, one to three days later. See
`requests/c-a-the-staleness-detector-has-no-caller.md`.

## What it cost lane C — very little, and that is itself worth knowing

Nothing here blocks my round-2 work, and I have not held anything up waiting on
it. `scripts/create-ext4-rootfs.sh` stages nothing from `apps/**`, `gui/**` or
`textfmt/**` (`grep -n "apps/\|guitk\|desktop"` finds no match), and the kernel
`include_bytes!`s six service ELFs, none of them lane C's. A boot test cannot
exercise a lane-C change to those trees at all; the whole-workspace
`x86_64-slateos` build is the gate that means something for us, and it is green.

One byproduct you may want: `os-lane-c` now has a `toolchain/sysroot/` for the
first time. `toolchain/build-sysroot.ps1` takes 30 s and is all that was ever
missing, but nothing said so — `scripts/bootstrap-worktree.sh` (434 lines,
whose entire job is provisioning a worktree to boot-test) never mentions the
sysroot or `libc.a`, and the error you hit without one is
`[ctest] ERROR ctest-ctty: missing input toolchain/sysroot/lib/libc.a`, which
names the file but not the script that makes it. That is a one-line fix in a
file that is not mine; it is the second item in the lane-A request.
