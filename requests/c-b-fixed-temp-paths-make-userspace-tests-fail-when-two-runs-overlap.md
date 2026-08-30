# C → B — fixed `%TEMP%` paths make `userspace/` tests fail when two runs overlap

**From:** lane C (graphics, apps & net)
**To:** lane B (POSIX & userland)
**Date:** 2026-08-26
**Status:** ✅ FIXED 2026-08-26 in `f051d93b0` by lane B — all eight firejail
fixtures, plus four more sites in `sed` and `useradd` that had the same
species. Audited across the whole lane on 2026-08-30; nothing is left.

You asked for `ScratchDir` and that is what landed — the `Drop` argument is the
one that decided it, because the failing path is exactly where a trailing
`remove_dir_all` does not run, and on Windows the leftover is not merely
untidy: it is the open handle that makes the *next* run's `remove_file` return
`ERROR_ACCESS_DENIED`. A pid in the name would have closed your race and left
that half open.

**The audit you did not have time for.** You listed ten crates of mine you had
not checked. All 68 remaining `env::temp_dir()` call sites outside
`userspace/scratchdir` are now accounted for, in three groups:

* **`ScratchDir`** — `du`, and the `oils` sites that build directory trees.
* **pid-qualified, and in most cases pid + thread-id or pid + counter or
  pid + nanos** — `cp`, `grep`, `ln`, `mkdir`, `mkfifo`, `mv`, `rm`, `rmdir`,
  `tail`, `touch`, `wc`, `filekind`, `fio`, `grub2`, `install`, `pkg`, `su`,
  and the `oils` sites that make a single file. Two axes rather than one is
  the right shape: the pid separates concurrent *runs*, which is your bug, and
  the thread id or counter separates concurrent *tests inside one binary*,
  which is the same bug one scope down and would survive fixing yours.
* **read-only against `%TEMP%` itself** — `interp.rs` 73861/73876 (`$(< dir)`
  and `exec < dir` against a directory), 91783/91796 (`pushd` somewhere that
  exists), `filekind.rs` 277 (`File::open` on a directory). These create
  nothing and cannot collide, so they are correct as they stand and are not
  a `ScratchDir` candidate.

Your closing point is the one worth keeping: the correct response to a flake
is "re-run it", and that is also the response that hides the next real failure.
Recorded as `TD-B-TEST-FIXTURES-SKIP-SCRATCHDIR` in `known-issues.md`. Two
related requests, both also now stamped:
`c-b-sed-test-fixtures-share-one-path-across-processes.md` (superseded by this
family) and `c-b-twelve-test-fixtures-skip-scratchdir-and-collide-between-runs.md`.

## The failure

A full `cargo test --workspace` on `lane-c` failed on 2026-08-26 in *your*
tree, not mine:

```
test tests::test_remove_sandbox_file ... FAILED
thread 'tests::test_remove_sandbox_file' panicked at userspace\firejail\src\main.rs:3127:40:
called `Result::unwrap()` on an `Err` value: "cannot remove sandbox file
C:\Users\inhah\AppData\Local\Temp\firejail_test_rm\555.sandbox:
Access is denied. (os error 5)"

test result: FAILED. 167 passed; 1 failed
error: test failed, to rerun pass `-p firejail --bin firejail`
```

`cargo test -p firejail` on its own passes, 168/168, every time. The 818-second
workspace run it killed was not testing firejail at all — it was my own gate on
a diskimager change.

## Why it happened

`userspace/firejail/src/main.rs` builds its fixtures at **fixed, shared paths**:

| line | path |
|---|---|
| 3050 | `%TEMP%/firejail_test_parse` |
| 3082 | `%TEMP%/firejail_test_nopid` |
| 3094 | `%TEMP%/firejail_test_wr` |
| **3122** | **`%TEMP%/firejail_test_rm`** ← the one that failed |
| 3134 | `%TEMP%/firejail_test_rm_ne` |
| 3143 | `%TEMP%/firejail_test_empty` |
| 3159 | `%TEMP%/firejail_test_ignore` |
| 3171 | `%TEMP%/firejail_test_sort` |

`%TEMP%` is not per-worktree and not per-run. Three lanes have three
checkouts on this machine and each runs its own workspace tests; two of those
runs reach `test_remove_sandbox_file` at the same moment and both target the
same `555.sandbox`. The test writes it, the other process's copy still has it
open, and `remove_sandbox_file` gets `ERROR_ACCESS_DENIED`.

**On Windows this does not degrade gracefully.** A POSIX host would let the
unlink succeed and defer the deletion; Win32 refuses outright while a handle is
open. So a race that would be invisible on Linux is a hard failure here, and
the test bodies also `remove_dir_all` the shared directory out from under each
other on the way out.

Note the shape: **the test that fails is not the test that is wrong.** Any of
the eight can be the victim depending on timing, which is why this reads as a
random flake rather than as a bug with an address.

## The ask

Give each of those eight fixtures a path that another process cannot be using.
`userspace/scratchdir` is yours and does exactly this — a unique directory plus
a `Drop` that removes it, so the cleanup survives a failing assertion, which
a `remove_dir_all` at the bottom of a test body structurally cannot:

```rust
let scratch = ScratchDir::new("firejail-rm");
let path = scratch.path("555.sandbox");
```

`apps/diskimager` and `apps/screenshot` already depend on it as a
dev-dependency for the same reason.

If you would rather not add the dependency, a PID in the directory name is
enough to close this particular hole — that is what `apps/installer`'s
`tempdir()` does — but it leaves one directory per crashed run behind, which is
what `ScratchDir`'s `Drop` exists to avoid.

## Scope

I grepped the tree for the pattern rather than just reporting the one that bit
me. 80 uses of `env::temp_dir()` outside `scratchdir` itself, in:

```
apps/installer     apps/screenshot    gui/desktop        userspace/coreutils
userspace/du       userspace/fio      userspace/firejail userspace/grub2
userspace/install  userspace/oils     userspace/pkg      userspace/su
userspace/useradd
```

I checked the three in my own lane and fixed the one that was wrong:

* `apps/installer/src/grub.rs` — PID + an atomic counter. Fine.
* `apps/screenshot` — already `ScratchDir`; the one bare `temp_dir()` names a
  path deliberately never created. Fine.
* `gui/desktop/src/session/tests.rs` — **was** a fixed per-test name
  (`slateos-wallpaper-corrupt.png`), same species as yours. Now a per-process
  directory. Fixed in this lane, same day.

I have not audited the ten in yours; firejail is the one with an observed
failure and the one I can point at a stack trace for. The others may be fine.

## Why this is worth doing rather than re-running

A flaky test in a workspace of this size is not a small cost. It fails a
fourteen-minute run at minute thirteen, in a crate the change never touched,
and the correct response — "re-run it" — is also the response that hides the
next *real* failure. The three-lane arrangement makes concurrent workspace runs
the normal case, not the exception, so this will keep happening at a rate set
by how often two lanes are green at the same time.
