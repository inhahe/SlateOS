## B-THE-BASH-RELINK-SCRIPT-HARD-CODED-ONE-WORKTREE-SO-ONLY-`main`-EVER-RAN-BASH

**Status: FIXED 2026-08-16** (`scripts/bash-spike/slatelink.sh` now derives the
repo root from its own location; lane B relinked and boot-verified.)

**In short:** the script that builds our GNU bash binary had the path of one
particular working copy typed into it. Whichever of the four working copies you
ran it from, it read that one copy's library and wrote the result into that one
copy's output directory. So two of the three development lanes never had a bash
binary at all, their boot tests quietly skipped the bash test for four days, and
nobody noticed because "skipped" and "passed" both end with the boot test
passing.

**What it looked like.** `slatelink.sh` opened with two absolute paths:

```sh
SPIKE="/mnt/d/visual studio projects/os/build/spike"
SYSROOT="/mnt/d/visual studio projects/os/toolchain/sysroot/lib"
```

There are four checkouts (`os`, `os-lane-a`, `os-lane-b`, `os-lane-c`), so those
two lines are wrong in three separate ways at once, depending on where the
script is invoked from:

1. **It links against another lane's `libc.a`.** The artifact then proves
   nothing about the libc of the tree you ran it in — which is the entire point
   of the exercise, since `self_test_bash_on_slateos_libc` exists to assert that
   *our* libc satisfies bash.
2. **It writes into another lane's `build/spike/`,** clobbering whatever that
   lane had there without that lane's knowledge.
3. **It leaves the invoking worktree with no `bash-slateos.elf`,** so that
   lane's rootfs build logs `bash-slateos.elf not found — the bash self-test
   will no-op` and the image ships without `/bin/bash`.

**The observable consequence**, straight out of lane B's boot log before the
fix:

```
=== PATH-Z COVERAGE INCOMPLETE ===
  Path-Z prerequisites: 1 rung(s) SKIPPED — coverage is INCOMPLETE
  [spawn]   SKIP: GNU bash 5.2 linked against OUR libc.a (ring 3) — prerequisite missing: /mnt/bin/bash
=== Boot test PASSED ===
```

Bash ships beside osh by operator decision (design-decisions.md §305), which
makes it a product artifact, not a spike leftover. Two of three lanes were
merging work to `main` having never executed it once.

**Fix.** `ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"`, with
`SPIKE` and `SYSROOT` derived from it, so the script always operates on the
worktree it physically lives in. The `/tmp` scratch directories are now keyed by
worktree name (`/tmp/slate-sysroot-$LANE`, `/tmp/slate_missing-$LANE.txt`) so
two lanes relinking concurrently cannot overwrite each other's copy of the
sysroot or each other's missing-symbol report — the boot lock serialises QEMU,
not this. The script also now fails with an explanatory message when
`/tmp/bash-cross` is absent, instead of `cd`-ing into nothing and linking an
empty object list.

**Standing lesson, and it is the same one as three other entries today.** A
hard-coded path is not merely unportable; in a multi-worktree repo it is a
*silent cross-lane write*. Any script that reads or writes repo-relative files
must locate the repo from `BASH_SOURCE`/`__file__`, never from a literal. The
reason this survived is the familiar one: the failure mode was a SKIP, and a
skipped rung inside a passing boot test reads as success
(`B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`). The Path-Z coverage banner is what
eventually made it visible, which is an argument for that banner existing.
