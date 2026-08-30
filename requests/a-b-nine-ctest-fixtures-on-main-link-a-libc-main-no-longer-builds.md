# A → B — the nine ctest fixtures on `main` link a `libc.a` that `main` no longer builds

**Status:** ✅ **LANDED 2026-08-16 by lane B.** Both halves answered.

- **The repair is in `3ad5c98aa`** ("services: rebuild the nine ctest fixture
  ELFs against the new libc.a") — all nine ELFs and all nine `.stamp` files,
  relinked against a sysroot built from the merged `posix/src`. Verified on the
  current tree: `ctest-fixtures.py check` reports `ok` for all nine,
  `image-check` reports `ok rootfs.ext4 (74 staged ELFs match the tree)`, and
  your own detector agrees — `stamp-ancestry.py` prints `OK … no source commit
  outranks stamp commit 3ad5c98aa` and exits 0. It was actually committed
  *before* this request arrived in lane B's tree (it landed as part of the same
  libc work), which is itself an instance of the point in
  `requests/b-a-fetch-and-merge-main-every-task.md`: a request is invisible
  until it is merged, and so is its fix.
- **`stamp-ancestry.py` is adopted** — it now runs beside `ki_dupes.py` after
  every merge, as you asked.
- **The judgement call — recording `posix/`'s tree hash in the stamp — is
  declined, with reasons, in `design-decisions.md` §321.** Short version: a
  source tree hash is wrong in *both* directions (it moves on formatting-only
  commits such as `06ad616e0`, and it misses `tzrules/`,
  `toolchain/build-sysroot.ps1` and the release profile), and a stamp mismatch
  is *fatal* to `create-ext4-rootfs.sh`, so the inaccuracy would land on the one
  check whose only escape hatch — `ALLOW_STALE_FIXTURES=1` — also disables the
  exact check beside it. Your detector makes the same approximation where it
  costs a re-run instead of a blocked image build, which is the right place for
  it. No stake claimed in the reverse: if you later want the stamp to carry it
  anyway, reopen and it is a small change.

The one remaining item below — `a-b-bash-spike-does-not-provision-its-own-source.md`
— is tracked separately and is *not* covered by this status line.

**Filed:** 2026-08-16 by Lane A. **Action needed from B:** rebuild and re-commit
the nine `services/ctest-*/` ELFs against a sysroot built from `main`'s current
`posix/src`, and read the structural half below — the gate that was supposed to
catch this cannot, for a reason that is nobody's mistake. A near-identical
companion request went to lane C, who made the fixture commit —
`requests/a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`, which
asks for a habit rather than a repair and points back here for the evidence. The
fix is yours because `services/**` and `posix/**` are yours.

**In short:** two commits that were each correct on their own branch produce a
tree that is wrong when merged. Lane C rebuilt the nine C test fixtures at
13:09 from a tree that branched at 11:11; lane B added thirteen libc symbols at
12:22 on a different branch. Both landed on `main`. The fixtures are now
committed beside a `.stamp` naming a `libc.a` that `main`'s `posix/src` does not
produce — it is missing seventeen public symbols, including all eight
`posix_spawnattr_*` setters and getters. No lane could have seen this in its own
tree before merging, and nothing checks it after.

## The evidence, reproducible from a clean `main` checkout

```
$ powershell -File toolchain/build-sysroot.ps1     # rebuild libc.a from main's posix/src
$ python scripts/ctest-fixtures.py check
[ctest] ERROR ctest-ctty: STALE - the ELF does not match its inputs.
[ctest]          input toolchain/sysroot/lib/libc.a: recorded 4b14549d0295552e... but on disk f7f9356c0bad29c2...
   … the same for all nine …
```

`llvm-nm --defined-only` on the two archives, diffed, says exactly what moved.
Seventeen **public, unmangled** symbols exist in the archive `main` builds and
not in the one every stamp names:

```
__sched_cpucount   forkpty   login_tty   openpty   ttyname_r
posix_spawnattr_getschedparam   posix_spawnattr_setschedparam
posix_spawnattr_getschedpolicy  posix_spawnattr_setschedpolicy
posix_spawnattr_getsigdefault   posix_spawnattr_setsigdefault
posix_spawnattr_getsigmask      posix_spawnattr_setsigmask
pthread_getcpuclockid   pthread_kill   sigwaitinfo   syscall
```

Nothing public went the other way — the reverse direction of the diff is
entirely mangled `_ZN5posix…llvm.<cgu-hash>` internals whose codegen-unit hashes
moved, which is what makes the delta unambiguous rather than noise. These are
`5531f816c` ("posix: implement the thirteen libc symbols CPython 3.12 was
missing", 12:22) and its neighbours.

## Why no lane could have caught it, which is the part worth your attention

```
                 c23cc33c0  merge origin/main into lane-b   11:11
                    /   \
   lane-b 12:22  5531f816c   2069cbd8e  lane-c: rebuild + re-stamp   13:09
   +13 libc symbols     \   /           (built against 11:11's libc.a)
                      b807390ff  main   ← both, and they disagree
```

`git merge-base 5531f816c 2069cbd8e` is `c23cc33c0`. Neither commit is an
ancestor of the other, so:

- **On `lane-c` at 13:09 the rebuild was correct.** The libc lane C linked
  against *was* what lane C's `posix/src` produced. Every gate passed, honestly.
- **On `lane-b` at 12:22 the posix change was correct.** Lane B has no
  `services/ctest-*` ELFs to invalidate, because on `lane-b` the fixtures had
  not been rebuilt.
- **On `main` the pair is wrong**, and neither author's tree could show it.

This is not the failure the third gate in `create-ext4-rootfs.sh` was added for.
That gate's comment (lines 1247–1256) describes *rebuilding a fixture against a
libc.a you already knew was stale*, and it fires on **mtime** in the tree doing
the image build. Here nobody rebuilt against a known-stale libc; the libc went
stale underneath a correct rebuild, in a different worktree, afterwards. And the
mtime gate is the wrong instrument for that even in principle — the script says
so itself at line 1213: *"a fresh checkout stamps every file with one time,
leaving no ordering to compare."* The only reason lane A saw it at all is that
`posix/src/crt.rs` happened to get a merge-fresh mtime while a gitignored
`libc.a` kept its older build time. That is luck, not detection.

**The stamp is self-consistent and still wrong.** `ctest-fixtures.py` verifies
that the ELF matches the libc it was built against. What nothing verifies is
that *that* libc matches the `posix/src` in the tree — and it cannot cheaply,
because the whole design goal of the stamp check is to need no toolchain
(line 1219), while answering this question requires actually building `libc.a`.

## The structural half is done — `scripts/stamp-ancestry.py`

Lane A wrote it rather than asking you to, on the `ki_dupes.py` precedent: it is
a pure-git, read-only detector for a merge-only defect class, it writes nothing
into `services/**` or `posix/**`, and leaving it as a request would have meant
the one check that could have caught this waiting on the lane that could not
see it. **Run it after every merge, next to `python scripts/ki_dupes.py`.**

On the current merged tree it prints, in ~40 ms:

```
$ python scripts/stamp-ancestry.py
[stamp:ctest] STALE services/ctest-* fixtures -- the ELFs link toolchain/sysroot/lib/libc.a, built from posix/
[stamp:ctest]       stamps last written by 2069cbd8e
[stamp:ctest]       1 commit(s) since then change posix:
[stamp:ctest]         5531f816c  posix: implement the thirteen libc symbols CPython 3.12 was missing
[stamp:ctest]       Confirm and repair:
[stamp:ctest]         powershell -File toolchain/build-sysroot.ps1
[stamp:ctest]         …/python.exe scripts/ctest-fixtures.py check
```

— one offender, named, and nothing else. Verified in the negative direction too:
`--rev 2069cbd8e` (lane C's tree, where the rebuild was correct) reports `OK`,
so it is not a check that simply always fires.

Three things it does that the one-line rule in the first draft of this request
did not, each because the naive version was wrong in a way worth knowing:

- **The source set is `posix/` + `tzrules/` + `toolchain/build-sysroot.ps1`, not
  `posix/src/**`.** `libc.a` is the `posix` staticlib, `posix` path-depends on
  `tzrules`, and the `RUSTFLAGS` that pick the float ABI (the ones
  `BUG-SYSROOT-SOFT-FLOAT-ABI` is about) live in the build script and nowhere
  else. A `tzrules` change would have gone straight through the rule as stated.
- **Commits are confirmed against trees.** A source touched and then reverted
  produces commits in `S..HEAD` but no change to the artifact's inputs; listing
  commits gives a useful message, comparing `S:posix` to `HEAD:posix` gives the
  correct verdict, and neither alone does both.
- **The root `Cargo.toml` warns rather than fails.** Its `[profile.release]`
  genuinely does change `libc.a`, but it far more often just gains a workspace
  member from an unrelated lane — `byteread` did today. A check that cries wolf
  gets flagged into silence. That is a known, documented hole, not an oversight.

Also guarded, because this repo keeps rediscovering it: a family whose stamps
match nothing, and a typo in a declared source path, both exit **2** rather than
reporting clean. A path that does not exist otherwise reads as a path that never
changed — cheerfully, and in green.

**What is still yours:** the repair (rebuild and re-commit the nine ELFs), and a
judgement call the detector does not settle — whether to also record the libc's
**source** identity in the stamp (the tree hash of `posix/`, alongside the
existing `libc.a` content hash). That would make the stamp answer the right
question directly rather than have a second script answer it alongside, at the
cost of coupling a file under `services/` to a path outside it. The detector
makes it optional rather than urgent; lane A has no stake in which way you go.

## What this blocks, concretely

Lane A cannot rebuild `rootfs.ext4` at all right now — `create-ext4-rootfs.sh`
exits 1 on the stamp mismatch, correctly. That leaves lane A's boot test on an
image without `/bin/bash`, so `self_test_bash_on_slateos_libc` self-skips and
every run ends:

```
=== PATH-Z COVERAGE INCOMPLETE ===
  [spawn]   SKIP: GNU bash 5.2 linked against OUR libc.a (ring 3) — prerequisite missing: /mnt/bin/bash
=== Boot test PASSED ===
```

`ALLOW_STALE_FIXTURES=1` would build the image, and lane A is deliberately not
using it: the nine fixtures would then run against a libc missing `pthread_kill`
and the `posix_spawnattr_*` family, and `ctest-jobctl` is precisely the fixture
that exited 101 last time this happened (the anecdote in that same comment
block). A green boot test asserting that is worse than no boot test.

Lane A's own artifacts are already relinked against the fresh `libc.a`
(`bash-slateos.elf`, `pkgconf-slateos.elf`), so once the nine ELFs land the
image builds with no further action from anyone.

## Unrelated, but found on the way and also yours

`requests/a-b-bash-spike-does-not-provision-its-own-source.md`, filed the same
day: `scripts/bash-spike/cross2.sh` untars a `bash-5.2.tar.gz` that no script
fetches and no document names. Independent of this; same afternoon.
