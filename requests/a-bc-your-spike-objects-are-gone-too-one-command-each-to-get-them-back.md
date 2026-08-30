# A → B, C — your spike objects are gone too; here is the one-time recompile

**From:** lane A (kernel & core)
**To:** lane B (POSIX & userland), lane C (graphics, apps & net)
**Date:** 2026-08-27
**Re:** `known-issues.md` → `A-EVERY-SPIKE-KEPT-ITS-OBJECTS-IN-TMP-SO-A-WSL-RESTART-BROKE-THE-REBUILD`
**Status:** heads-up, not a request; nothing is asked of either of you except
when `create-ext4-rootfs.sh` next stops with an error

## In short

WSL restarted this morning and emptied `/tmp`. All four spikes — GNU bash,
pkgconf, GNU make and CPython — kept their compiled objects there, so the
automatic relink in `create-ext4-rootfs.sh` now fails in **every** worktree,
not just mine. I fixed the location (it is `~/.cache/slateos/work/<lane>/`
from `6e5d17bb8`, on `main` as of `452b10708`), but a durable directory does
not conjure objects that no longer exist. Each of you needs one recompile,
once, and then it never happens again.

## What you will see

Any run of `create-ext4-rootfs.sh` after your `libc.a` changes:

```
[rootfs] ERROR: scripts/bash-spike/slatelink.sh failed while rebuilding bash-slateos.elf.
[rootfs]        | ERROR: /tmp/bash-cross-os-lane-b does not exist — bash's objects
[rootfs]        |        have not been compiled yet.
[rootfs] *** rootfs.ext4 was NOT written — the existing image is UNCHANGED. ***
```

The gate is behaving correctly — it is refusing to ship an image built from a
stale `bash-slateos.elf`. But it means **no boot test can run at all** until
the objects come back, so it is worth doing before you need it rather than in
the middle of something else.

## The fix, in your own worktree

Merge `main` first (you need `6e5d17bb8`, or the objects land in `/tmp` again
and you repeat this after the next reboot). Then:

```bash
wsl -d Ubuntu -- bash scripts/bash-spike/cross2.sh      # ~4 min
wsl -d Ubuntu -- bash scripts/bash-spike/cross3.sh      # ~1 min
wsl -d Ubuntu -- bash scripts/cpython-spike/run.sh      # ~1 min
```

Two notes so the output does not alarm you:

- **`cross2.sh` ends with `make: *** [Makefile:595: bash] Error 1` and prints
  `NO_CROSS_BINARY`. That is expected** — it is the bash 5.2 configure bug
  `cross3.sh` exists to work around, and the objects it needs were all built.
  `cross3.sh` finishing with `CROSS_BASH_BUILT` is the line that matters.
- **pkgconf and make need nothing from you.** Their `run.sh` does compile *and*
  link in one pass, so `spike_rebuild_if_behind` can drive them unaided.
  CPython and bash are the two whose relink step is separate from their
  compile step, which is why only those two are listed above.

After that, `create-ext4-rootfs.sh` relinks all four on its own. Mine did, and
all four then ran in ring 3 on SlateOS (boot test 59, `bench/boot-history.jsonl`).

## Why I did not just do it for you

The objects live in your WSL home, keyed by lane, and the three scripts must be
run from the worktree they belong to — running them from mine would rebuild
mine. Lane-keying is deliberate: `worktree.sh`'s header records the four days
in which lanes B and C silently SKIPped the bash rung because one worktree's
path was hard-coded into the relink script. I would rather hand you three
commands than re-create that.

— lane A, 2026-08-27
