## B-A-A-MERGE-OF-TWO-CORRECT-COMMITS-LEFT-NINE-FIXTURES-LINKING-A-LIBC-`main`-NO-LONGER-BUILDS (lane B's tree; filed by lane A, 2026-08-16)

**Status: ✅ FIXED 2026-08-16 — both halves.** Detection is
`scripts/stamp-ancestry.py` (lane A, described below); the repair is lane B's
`3ad5c98aa`, which relinks all nine ELFs and rewrites all nine `.stamp` files
against a sysroot built from the merged `posix/src`. On lane B's tree today:
`ctest-fixtures.py check` → `ok` ×9, `image-check` → `ok rootfs.ext4 (74 staged
ELFs match the tree)`, `stamp-ancestry.py` → `OK … no source commit outranks
stamp commit 3ad5c98aa` (exit 0), and the boot test passes with
`self_test_bash_on_slateos_libc` no longer self-skipping.

**Not archived yet, deliberately.** `3ad5c98aa` is on `lane-b` and reaches
`main` only with the merge that carries this line; the archiving rule is "fixed
*and* the fix has been on `main` for a full boot test". Move it to
`known-issues-resolved.md` after the next green boot test on a tree that
contains it via `main`.

**The judgement call the detector left open is answered:
`design-decisions.md` §321 — no.** The stamp will not carry a `posix/` tree
hash. Reasons in full there; in one line, a source tree hash is wrong in both
directions (it moves on formatting-only commits, and it misses `tzrules/`,
`toolchain/build-sysroot.ps1` and `[profile.release]`), and a stamp mismatch is
*fatal* to `create-ext4-rootfs.sh`, so the inaccuracy would land where the only
escape hatch also disables the exact check beside it.

Original report follows, unedited, because the history is the content.

**Was: OPEN 2026-08-16.** Blocked `rootfs.ext4` rebuilds in every worktree.
Requests filed: `requests/a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md`
(the repair, lane B's) and `requests/a-c-fixture-rebuild-was-correct-on-lane-c-and-wrong-on-main.md`
(the habit, lane C's). Recorded here as well as there because a request is read
once by one lane, and this is a live defect on `main` that must not depend on
that happening.

**In short:** nine compiled test programs in `services/ctest-*/` are checked into
git alongside a small file recording which library they were linked against.
That record is honest and still wrong: the library it names is not the one
`main`'s source code produces any more. Two commits caused it, and *each one was
correct in the tree it was made in* — one added thirteen functions to the
library, the other rebuilt the nine programs, and they were made on different
branches within two hours of each other. Only their merge is broken, so neither
author's checkout could have shown it, and no check we have looks at a merge.

**Reproduce from a clean `main` checkout, two commands:**

```
$ powershell -File toolchain/build-sysroot.ps1
$ python scripts/ctest-fixtures.py check
[ctest] ERROR ctest-ctty: STALE - the ELF does not match its inputs.
[ctest]          input toolchain/sysroot/lib/libc.a: recorded 4b14549d0295552e... but on disk f7f9356c0bad29c2...
   … identically for all nine …
```

`llvm-nm --defined-only`, diffed across the two archives, says exactly what
moved. **Seventeen public, unmangled symbols** are in the archive `main` builds
and absent from the one all nine stamps name:

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
moved, which is what makes the delta unambiguous rather than noise. Defined
symbol counts 4379 → 4397; archive size 12,344,478 → 12,322,086 bytes.

**The history, which is the entry's actual content:**

```
                 c23cc33c0  merge origin/main into lane-b   11:11
                    /   \
   lane-b 12:22  5531f816c   2069cbd8e  lane-c: rebuild + re-stamp   13:09
   +13 libc symbols     \   /           (built against 11:11's libc.a)
                      b807390ff  main   ← both, and they disagree
```

`git merge-base 5531f816c 2069cbd8e` is `c23cc33c0`, and
`git merge-base --is-ancestor 5531f816c 2069cbd8e` answers **no**. So:

- **On `lane-c` at 13:09 the rebuild was correct.** The `libc.a` lane C linked
  against *was* what lane C's `posix/src` produced. Every gate passed honestly.
- **On `lane-b` at 12:22 the posix change was correct.** Lane B had no rebuilt
  `services/ctest-*` ELFs to invalidate, because on `lane-b` they had not been
  rebuilt.
- **On `main` the pair is wrong**, and no author's tree could show it.

**Why every existing gate is structurally the wrong instrument.**
`create-ext4-rootfs.sh` has three, and this passes cleanly through the reasoning
of all three:

| gate | what it asks | why it misses this |
|---|---|---|
| mtime (lines ~1213–1246) | was the ELF rebuilt after `libc.a` changed? | The script says so itself at line 1213: *"a fresh checkout stamps every file with one time, leaving no ordering to compare."* And here nobody rebuilt against a known-stale libc — the libc went stale *underneath* a correct rebuild, in a different worktree, afterwards. |
| content stamp (`ctest-fixtures.py`) | does the ELF match the `libc.a` it was built against? | Yes, it does. The stamp is **self-consistent and still wrong**: nothing verifies that *that* `libc.a` matches the `posix/src` in the tree. |
| `SYSROOT_STALE` | is `libc.a` older than `posix/src`? | Fires only at image-build time, in whichever lane happens to build an image, on a gitignored file that may not exist. |

The reason lane A saw it at all is that `posix/src/crt.rs` happened to receive a
merge-fresh mtime while the gitignored `libc.a` kept its older build time. That
is luck, not detection — and the initial hypothesis it produced ("the mtime gate
is a merge false positive") was *wrong*, which is only known because it was
tested by actually rebuilding rather than reasoned about.

**This is a defect class, not an incident.** Prior art in this file —
`B-THE-TRACKED-FIXTURE-BINARIES-DRIFT-FROM-THEIR-SOURCES` — established that a
tracked build product carries an invariant version control does not enforce, and
fixed it with content stamps. The stamp closed *within-tree* drift completely.
What remains open is one level up: **a merge can invalidate an artifact without
any commit in either parent being wrong.** Any tracked binary with recorded
inputs owned by one lane and derived from another lane's source has this shape.
Today that is `services/ctest-*` (lane B's, derived from `posix/src`); lane C's
`gui/**` binaries with recorded inputs are the next candidates.

**Detection half: FIXED 2026-08-16 — `scripts/stamp-ancestry.py`.** A pure-git
check, no toolchain, ~40 ms, same answer in a fresh clone as on the machine that
merged. **Run it after every merge, beside `python scripts/ki_dupes.py`;** the
two are the same kind of check — both catch things that are wrong only in a
merge — and belong in the same habit.

> Let `S` = the commit that last touched any of a family's stamps
> (`git log --format=%H -1 -- ':(glob)services/ctest-*/*.stamp'`). Any commit
> reachable from HEAD but not from `S` that touches the family's sources
> (`git log S..HEAD -- posix tzrules toolchain/build-sysroot.ps1`) is a commit
> whose effect on the artifact was never recorded.

It is *exactly* sensitive to the merge-induced case, because it asks a question
about **history** rather than about the working tree, which is the property both
mtime and the content stamp lack. It belongs at **merge time**, not image-build
time: merge is when the defect is created, and an image build is a lane-local
event that may not happen for hours.

Four design points worth keeping if it is ever rewritten, each because the
one-line rule as first stated was wrong in a way that mattered:

- **The source set is `posix/` + `tzrules/` + `toolchain/build-sysroot.ps1`, not
  `posix/src/**`.** `libc.a` is the `posix` staticlib; `posix` path-depends on
  `tzrules`; and the `RUSTFLAGS` that pick the float ABI — the ones
  `BUG-SYSROOT-SOFT-FLOAT-ABI` is about — live in the build script and nowhere
  else. A `tzrules` change would have walked straight through the rule as
  originally written.
- **Named commits are confirmed against trees.** A source touched and then
  reverted produces commits in `S..HEAD` but no change to the artifact's inputs.
  Listing commits gives a useful message; comparing `S:posix` against
  `HEAD:posix` gives the correct verdict; neither alone does both.
- **The root `Cargo.toml` warns rather than fails.** Its `[profile.release]`
  genuinely can change `libc.a`, but it far more often just gains a workspace
  member from an unrelated lane (`byteread` did, the same day). A check that
  cries wolf gets `ALLOW_`-flagged into silence, so this is a deliberate,
  documented hole — an opt-level change passes as a warning — and not an
  oversight. The alternative was parsing the manifest to see which table moved,
  which is fragile in a way that fails silently.
- **A family whose stamps match nothing, and a typo in a declared source path,
  both exit 2 rather than reporting clean.** A path that does not exist reads as
  a path that never changed, cheerfully and in green — so a typo in the config
  would have *widened* the silence the script exists to end. That is the fifth
  instance in this file of "the check did not run" wearing the costume of "the
  check passed", and it was written in from the start for that reason.

**Verified in both directions.** Positive: on the merged tree it names
`5531f816c` and nothing else. Negative: `--rev 2069cbd8e` — lane C's tree, where
the rebuild *was* correct — reports `OK`, so it is not a check that simply always
fires. The stale-config and missing-family branches were exercised directly and
both exit 2. The `:(glob)` pathspec magic is deliberate and load-bearing: plain
git globbing lets `*` cross `/`, which would silently widen the stamp pathspec.

**Repair half: FIXED 2026-08-16 — `3ad5c98aa`** (see the status block at the
top). The judgement call the detector did not settle — whether to also record
the libc's **source** identity in the stamp (the tree hash of `posix/`, alongside
the existing `libc.a` content hash) — was **declined**, in `design-decisions.md`
§321. It would have made the stamp answer the right question directly rather
than have a second script answer it alongside; what ruled it out is that a tree
hash cannot answer it *accurately* in either direction, while a stamp mismatch
is fatal to the image build. The coupling cost lane A flagged (a file under
`services/` naming a path outside it) turned out to be the smaller objection —
the stamp already names `toolchain/sysroot/lib/libc.a`.

**What it blocked while open.** `create-ext4-rootfs.sh` exits 1 on the stamp
mismatch, correctly, so no worktree could rebuild `rootfs.ext4`. That left the
boot test running an image with no `/bin/bash`, so
`self_test_bash_on_slateos_libc` self-skips and every run ends:

```
=== PATH-Z COVERAGE INCOMPLETE ===
  [spawn]   SKIP: GNU bash 5.2 linked against OUR libc.a (ring 3) — prerequisite missing: /mnt/bin/bash
=== Boot test PASSED ===
```

**`ALLOW_STALE_FIXTURES=1` is deliberately not being used**, and that decision
should survive whoever reads this next. It builds the image immediately, but the
nine fixtures then run against a libc missing `pthread_kill` and the entire
`posix_spawnattr_*` family — and `ctest-jobctl` is precisely the fixture that
exited 101 the last time a fixture ran against a libc that predated a fix it
needed (the anecdote in `create-ext4-rootfs.sh`'s own comment at 1247–1256). A
green boot test asserting that is worse than no boot test.

Lane A's own artifacts are already relinked against the fresh `libc.a`
(`bash-slateos.elf`, `pkgconf-slateos.elf`), so once the nine ELFs land the
image builds with no further action from anyone.
