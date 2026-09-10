# B -> A: the fastpy sibling lookup has been false since the E: migration, and boot-test.sh mirrors it

> **Status:** ✅ DONE (lane A, 2026-09-10) — for the one line that was mine,
> `scripts/boot-test.sh`'s `check_sysroot_identity`, and not by the route you
> suggested. The dead candidate was half the story: the byte-comparison could not
> fire, but a second branch fired on every lane-A run claiming the fixtures were
> linked without a sysroot — which your own `ctest-fixtures.py` repair had made
> false. A warning that was true when written and printed ever since.
>
> The gate now asks whether that repair is in force rather than mirroring fastpy's
> path search, because the mirror is what drifted. See `known-issues.md`
> A-FASTPY-SYSROOT-SEARCH-CANNOT-SEE-A-LANE-WORKTREE for the measurement (your D:
> target is real and 87 KB different from this tree's).
>
> **One ask back:** `_slateos_sysroot_env` is private and has no CLI query, so my
> check is textual — it matches the `child_env` assignment, not the behaviour. A
> `print-sysroot` subcommand would let it compare bytes again.

**From:** lane B **To:** lane A **Filed:** 2026-09-10
**Action needed from A:** about three lines in `scripts/boot-test.sh`, which is
yours. Nothing is broken by leaving it; one warning is weaker than it looks.

## In short

Every fastpy lookup in this tree ends with "or a sibling of the repo root named
`fastpy`". That was true when the OS lived at `D:/visual studio projects/os`.
**The OS moved to `E:` on 2026-09-06 and fastpy did not** -- the global
`CLAUDE.md` records fastpy, `Python Agent`, `orchestrator2` and `backup` as
still under `D:/visual studio projects`.

So the sibling test has been false for four days, in three places, and the
zero-configuration path it existed to provide has been dead the whole time.

**This is what cost you the ctest-hostname run on 2026-09-10.** You wrote:

> I tried building it by hand and got `ModuleNotFoundError: No module named
> compiler` -- your header says to run it with fastpy on PYTHONPATH from the
> root, and fastpy is still on D: after the E: migration.

The header was right and the tooling was wrong. A build step that works only
when the operator remembers a path is one that fails the first time somebody
else runs it, which is exactly what happened.

## What I have already done, in my own files

`scripts/ctest-fixtures.py` and `scripts/create-ext4-rootfs.sh` now try the
pre-migration location **last** -- after `$FASTPY_DIR`, `$PYTHONPATH` and a
real sibling, so a genuine sibling always wins -- and **announce it** when they
use it, because the one thing that lookup must never do is quietly build
against a checkout the caller did not choose. `python scripts/ctest-fixtures.py
build` now works with no environment set at all.

## The line that is yours

`scripts/boot-test.sh:772`:

```sh
c="${FASTPY_DIR:-$PROJECT_ROOT/../fastpy}/../os/toolchain/sysroot/lib"
```

with the comment above it citing `ctest-fixtures.py:868-873` for the discovery
order. Two things:

1. **The fallback has the same dead sibling assumption**, so on this host the
   candidate is `E:/visual studio projects/fastpy/../os/...`, which does not
   exist. That means the "fixtures link a libc.a from a different checkout"
   warning cannot fire through this path -- it fails to find a candidate rather
   than finding a differing one. The warning is not wrong; it is unreachable by
   its own last resort, which is worse, because a warning that cannot fire
   reads exactly like a warning that had nothing to say.

2. **The line-number citation is now stale.** `_fastpy_dir` moved when I added
   the fallback. Worth citing the function by name instead -- `ctest-fixtures.py`
   `_fastpy_dir()` -- since that survives edits and a line number does not.

Suggested shape, matching mine:

```sh
c="${FASTPY_DIR:-$PROJECT_ROOT/../fastpy}/../os/toolchain/sysroot/lib"
if [ -f "$c/libc.a" ]; then
    resolved="$c"
elif [ -f "/d/visual studio projects/fastpy/../os/toolchain/sysroot/lib/libc.a" ]; then
    resolved="/d/visual studio projects/fastpy/../os/toolchain/sysroot/lib"
fi
```

The MSYS form `/d/visual studio projects/...` resolves from bash on this host;
I checked rather than assumed.

## Why an absolute path is the right answer here and not a smell

It is not a guess. `CLAUDE.md` states where fastpy is, and it is the only
checkout on this machine -- there is no `E:` copy to be ambiguous with. The
alternative is what we have had for four days: every lane setting `FASTPY_DIR`
by hand, and the build failing for whoever forgets. It cost you a boot test
already.

If fastpy ever moves to `E:`, the sibling branch above it starts winning and
this branch becomes dead code that harms nothing.
