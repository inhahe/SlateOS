## A-SYSROOT-STALENESS-GATE-IS-WEDGED-BY-GIT-TOUCHING-A-FILE-IT-WATCHES

**Status:** FIXED 2026-08-18, the same day it was diagnosed. Lane A
(`scripts/`, `toolchain/`). The diagnosis below is kept in full because the
*reasoning* it corrects — "mtime answers an ordering question that a hash
cannot" — is plausible enough that someone will propose it again. See
"How it was fixed" at the end.

**Symptom.** `scripts/create-ext4-rootfs.sh` and `scripts/ctest-fixtures.py`
both refuse to proceed with

```
[ctest] ERROR: toolchain/sysroot/lib/libc.a is OLDER than 1 tracked source(s).
[ctest]          newer: toolchain/build-sysroot.ps1
```

and the remedy they print — re-run `toolchain/build-sysroot.ps1` — **does not
clear it.** The gate is unsatisfiable by its own instructions, which is worse
than a gate that over-reports: the reader follows the advice, sees no change,
and has no next move except `ALLOW_STALE_FIXTURES=1`, which disables a real
check for an unrelated reason.

**Why re-running does not help.** `build-sysroot.ps1` assembles the sysroot
with `Copy-Item`, and PowerShell's `Copy-Item` **preserves the source file's
timestamp**. So `libc.a`'s mtime is not the assembly time — it is the mtime of
`target/…/libposix.a`, i.e. when cargo last *linked* it. If posix has not
changed, cargo does not relink, the source mtime does not move, and the copy
faithfully reproduces the old timestamp. `libc.a` can therefore never become
newer than an input that was touched after the last real posix build.

**What touches an input without changing it: git.** `checkout`, `merge` and
`stash` all write mtimes. `sysroot_staleness()` watches `posix/src`,
`posix/Cargo.toml`, `toolchain/stubs` and `toolchain/build-sysroot.ps1`; any
git operation that writes one of those wedges the gate. This is not an edge
case here — `CLAUDE.md` requires `git fetch origin && git merge origin/main` at
the start of *every* task, and the three lanes merge constantly. It was
triggered here by a plain `git checkout --` that restored a file to
byte-identical content.

**Why the current design chose mtime.** Deliberately, and the reasoning is in
`scripts/ctest-fixtures.py` → `sysroot_staleness()`: *"mtime, not a hash,
because the question is 'was this built after that was edited' — an ordering,
which a content hash of a file the stamps do not track cannot answer."* That is
correct as far as it goes. The hole is the premise that mtime records when a
file was **edited**; it records when a file was **written**, and git writes
files it has not edited.

**The proper fix.** Give the sysroot the same treatment the ctest fixtures
already got when this exact class of bug hit them (their stamps are at
`version 2` for the same reason): have `toolchain/build-sysroot.ps1` write a
`toolchain/sysroot/.sysroot.stamp` recording the SHA-256 of every input it was
built from — the same four roots `sysroot_staleness()` walks — and have the
checker compare **hashes against that stamp** instead of comparing mtimes. That
answers the docstring's objection directly: the inputs stop being "files the
stamps do not track". It also makes the gate satisfiable, because re-running
the build script always rewrites the stamp. Keep the mtime comparison as a
fallback for when the stamp is absent, exactly as the fixture gate now keeps it
as a fallback for when python is absent.

**Workaround used on 2026-08-18** (recorded so it is not mistaken for a fix):
`touch -d <time before libc.a> toolchain/build-sysroot.ps1`, legitimate only
because `git diff HEAD` proved the file byte-identical to the commit it was
built from — i.e. the restored mtime asserts something true. Do not reach for
`ALLOW_STALE_FIXTURES=1` instead; that suppresses the fixture content check too.

**Related:** the sibling defect in the same run — the mtime gate `exit 1`ing
before the content-stamp gate could run — *was* fixed, in the commit that adds
this entry. This entry is the remaining half, one level up, in the check the
fixtures' own stamps cannot reach.

### How it was fixed

As described above: the sysroot now carries a content stamp.

- `toolchain/build-sysroot.ps1` calls `scripts/ctest-fixtures.py sysroot-stamp`
  after assembling, writing `toolchain/sysroot/.sysroot.stamp` — the SHA-256 of
  every file under the four input roots, text hashed with CRLF folded to LF per
  the `version 2` rule the fixture stamps already use.
- `sysroot_staleness()` returns `(mode, findings)`: hash drift when the stamp is
  present, the old `find -newer` ordering when it is not, exactly as the fixture
  gate falls back when python is absent. It reports which mode it used, so a
  weaker answer never reads as the stronger one.
- `scripts/create-ext4-rootfs.sh` no longer reimplements the scan; it shells out
  to `ctest-fixtures.py sysroot-check` and keeps its `find` loop only for the
  no-python path. The input list and the folding rule now exist once.

**The gate is satisfiable now**, which was the actual complaint: the stamp is
rewritten on every run of `build-sysroot.ps1` whether or not cargo relinked, so
following the printed remedy always clears it.

Three things worth keeping in mind:

- **The stamp is gitignored, deliberately** (`/toolchain/sysroot/` already
  covered it). Unlike the fixture stamps, which sit beside tracked ELFs and must
  survive a fresh clone, this one describes *this machine's* `libc.a`. A tracked
  copy would arrive in a checkout asserting a libc you have not built yet was
  built from those sources — the precise false-green the stamp exists to
  prevent. A missing stamp truthfully means "nobody has built the sysroot here".
- **`sysroot-stamp` run by hand is a silencer.** It records the sources as they
  are now and claims `libc.a` came from them, without verifying it — the same
  hazard `ctest-fixtures.py stamp` carries for fixtures, and the reason both are
  separate commands rather than something `check` does implicitly.
- **`target/` is excluded from the input walk.** It holds the output of building
  those sources, so including it would make the stamp self-referential and no
  two consecutive runs could ever agree. The mtime scan this replaces had the
  same exposure and got away with it only because nobody had run cargo inside
  `toolchain/stubs`.

**The workaround recorded above is no longer needed** and should not be
repeated: `touch`ing an input to defeat the check now defeats nothing, because
the check no longer looks at timestamps when a stamp exists.
