## TD-B-TAR-AND-RM-CARRY-TWO-DESCRIPTOR-WALKS (lane B, 2026-09-03)

**Status: FIXED, 2026-09-03**, the day after it was filed — which is what it
said it expected. `tar.rs` now holds no syscalls of its own: its `Dir` is a
newtype over `coreutils::dirfd::Dir`, its `Located` holds one, and the private
`CStat`, `CTimespec`, `oflag`, `c_path`, `c_name`, `embedded_nul`, `AT_*` and
the eleven-entry `extern "C"` block are gone. What stayed is what the entry
said would stay: `locate`, `Located`'s wording layer, `components`,
`MAX_SYMLINK_HOPS`, `EXDEV`/`ELOOP`.

**Certified.** `scripts/tar-diff.sh` — the case-by-case comparison against GNU
tar 1.35 that this entry named as the thing making the swap "provably a
behavioural no-op" — reports **245 passed, 0 differed, 2 differ on purpose**.
`scripts/coreutils-check.sh` is green on both halves: host clippy and a host
`check --bin tar` (the only build in the tree that compiles this file's
`cfg(not(unix))` arm), and on Linux 3755 tests across 87 binaries with the
`coreutils` lib at 421 and `tar` at 53.

Three things came out of the conversion that were not in the plan, plus one
defect it introduced and the harness caught:

- **The `locate` loop is now stat-first**, where it used to try the descent and
  read the failure. That is what lets it call `dirfd::Dir::open_child`, which
  is where the identity check lives — so the exposure this entry was filed
  *for* (`tar` correct on Linux, exposed on SlateOS for an ancestor swapped
  between the lookup and the open) is closed rather than merely relocated. It
  also stops `readlinkat` being asked of every component that failed to open,
  which the old code's own comment admitted was a guess at an errno that "is
  not promised to be anywhere else".
- **`ENOTDIR` is now raised by hand**, because it used to arrive from
  `O_DIRECTORY` and nothing opens with that flag on this path any more.
- **`Located::is_real_dir` stopped being an open** and became a stat, which
  fixes a case nobody had noticed: a directory that cannot be *opened* is still
  a directory, and answering "no" for it made a directory member over an
  existing unreadable directory report `EEXIST` instead of being accepted.
- **The conversion introduced exactly one behaviour change, and it was a bug.**
  `dirfd::c_target` refused an empty symlink target with `InvalidInput`, on the
  written grounds that `symlinkat` "would answer `EINVAL` anyway"; it answers
  `ENOENT`, and `tar.rs`'s deleted code had passed the empty target straight
  through. `tar-diff.sh`'s `emptysym` case is what found it — ours said
  "Invalid argument" where GNU said "No such file or directory". `c_target` now
  refuses only the NUL, and `dirfd`'s new test asserts *which layer* rejected
  the empty target rather than that it was rejected. Written up as Lesson 110 (lane B).

`dirfd::Stat` gained an `mtime` for this — see `design-decisions.md` §756 for
why a shared type was widened for one caller — and `dirfd::Dir` gained
`reopen()`, which `locate` needs to keep its root while a walk consumes a
handle.

**In short:** two utilities now know how to walk a directory tree safely, and
they know it in two places. `tar` learned it first, privately, when its own
symlink hole was fixed; `rm` learned it second, in a shared module built for
the purpose. Until `tar` is moved onto the shared one, a fix or a mistake in
either copy does not reach the other — and the thing being duplicated is a
security invariant, which is the worst kind of thing to have two versions of,
because reading one file no longer tells you whether the other is safe.

**Where.** `userspace/coreutils/src/bin/tar.rs` carries its own `Dir`, `CStat`,
`oflag` module and `extern` block. `userspace/coreutils/src/dirfd.rs` is the
shared layer, currently used only by `rm`.

**The difference that matters.** The shared module's `open_child` verifies the
descriptor it got — `fstat` and compare `(dev, ino)` against the `fstatat` that
classified the entry — because `openat` is still textual on SlateOS
(`requests/b-a-openat-is-the-one-at-call-left-unpinned.md`). `tar`'s private
copy does not, so **`tar` is correct on Linux and exposed on SlateOS** for the
ancestor-swap case. That is the concrete cost of the duplication, not a
hypothetical one.

**Proper fix.** Delete `tar.rs`'s `Dir`, `CStat`, `oflag` and externs; use
`coreutils::dirfd`. The tar-specific parts around it — `locate`, `Located`,
`components`, `MAX_SYMLINK_HOPS`, the `EXDEV` handling — stay where they are;
they are about what `tar` does with a resolved location, not about how it
resolves one. Certify with `scripts/tar-diff.sh`, which compares against GNU
tar case by case and is what makes the swap provably a behavioural no-op.

Filed the same day the shared module landed, and expected to be short-lived:
it exists to name the window, not to justify it.
