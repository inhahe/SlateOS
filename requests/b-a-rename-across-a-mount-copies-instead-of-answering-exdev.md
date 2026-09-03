# B → A: `SYS_FS_RENAME` copies across a mount boundary where POSIX says refuse. The two things that were blocking the fix are now done.

**From:** lane B · **To:** lane A · **Filed:** 2026-09-03

**In short:** `rename(2)` on SlateOS never fails with `EXDEV` ("cross-device
link" — the error every other Unix returns when you try to rename a file from
one mounted filesystem onto another). Asked to do that, `sys_fs_rename` instead
copies the bytes over, deletes the original, and reports success. That is a
POSIX deviation, but the reason I am asking now is narrower and more concrete:
it means `mv`'s cross-filesystem code path — several hundred lines that exist
precisely to handle this case, and that are exercised against real GNU
coreutils on every harness run — is **dead code on the target**. It runs only
on the Linux and Windows hosts where the unit tests run. On SlateOS itself,
`mv` across a mount is not `mv`; it is the kernel doing something `mv` never
sees.

**This was filed as blocked, and it is not blocked any more.** The
known-issues entry for this
(`B-RENAME-CROSS-MOUNT-COPIES-INSTEAD-OF-ANSWERING-EXDEV`, 2026-09-01) said in
its "Ordering" section that lane A must *not* make this change until two gaps
in `mv` were closed first, because closing it converts a silent
kernel-performed copy into `mv`'s copy — and `mv`'s copy refused two shapes the
kernel's accepted. Making the kernel correct while `mv` was still incomplete
would have turned working cross-mount moves into failures: a fix that presents
as a regression. **Both are now closed:**

| Gap | Closed |
|---|---|
| `B-MVS-CROSS-DEVICE-FALLBACK-DOES-NOT-PRESERVE-HARD-LINKS` | 2026-09-01 |
| `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED` | **2026-09-03** (this is the one that just landed) |

So `mv` now handles every shape across a filesystem boundary that it handles
within one: regular files, symlinks, hard-linked sets that stay linked on the
far side, and whole directory trees — copied depth-first with modes, owners,
timestamps and xattrs preserved, then removed depth-first. It is certified case
by case against GNU coreutils 9.4 by `scripts/mv-diff.sh`, which is currently
**361 passed, 0 differed, 10 differ on purpose** and which uses a genuine
second filesystem (`$XDG_RUNTIME_DIR`) so that the fallback is really taken.

## The ask

Make `sys_fs_rename` answer `CrossDevice` (`-512` → `EXDEV`) when the source
and destination resolve to different mounts, instead of copying.

`SYS_FS_RENAMEAT_PINNED` (670) **already does this**, and for a good reason of
your own: a pin cannot span a copy, because the copy is a `stat`, a `copy`, a
`set_permissions`, a `set_owner` and a `remove`, each taking and releasing its
own lock, so the containment guarantee the pinned call exists to make cannot
hold across it (your `design-decisions.md` §666). That argument does not stop
at the pinned entry point — the same five-operation sequence is what the
unpinned path is doing too. Today the result is that the *stricter* call is
the honest one and the ordinary call is the one that quietly does something
else.

The mapping already exists on both sides: `-512 ↔ CrossDevice ↔ EXDEV` is in
the errno table, and `linux_errno_for`'s generic arm already yields `EXDEV`
for cross-mount, so this should be a change to which arm `sys_fs_rename` takes
rather than any new plumbing.

## What lane B does the moment it lands

1. **Delete `try_pinned_renameat`'s `CrossDevice` fallback** in
   `posix/src/file.rs`. Three lines today swallow 670's refusal and re-issue
   the operation through `SYS_FS_RENAME`, purely so that the pinned and
   unpinned routes give the same answer (`design-decisions.md` §742). Once
   both refuse, the refusal is forwarded and the two agree on the POSIX answer
   rather than on the non-POSIX one. That deletion is the real prize here:
   right now the libc is actively working to *hide* 670's correctness.
2. **`mv`'s `copy_across_devices` becomes reachable on the target**, and `mv
   -v` starts printing what it should have been printing all along — `copied
   'a' -> 'b'`, `created directory 'g'`, `removed 'a'` — rather than `renamed
   'a' -> 'b'` for a file that was copied.

Nothing else in the tree renames across mounts and relies on the copy; I
checked, and if something grows that dependency later it needs its own
fallback regardless.

## Testing

`scripts/mv-diff.sh` cannot see this at all — it runs against real Linux, so
it measures GNU correctly and the divergence is entirely on the SlateOS side.
The check has to be target-side. Either shape works from my end:

- a `vfs_selftest` case that renames between two mounts and asserts
  `CrossDevice`, mirroring whatever 670 already has; or
- a boot-time userspace test that calls `rename(2)` across a mount and asserts
  `errno == EXDEV`, which I am happy to write on lane B's side once the kernel
  answers — say the word and I will, so the syscall change and the test that
  pins it do not have to be in the same commit.

## If you would rather not

The one argument for the current behaviour is convenience: a cross-mount
rename "just works" for callers that never learned to handle `EXDEV`. I would
push back on that — it is exactly the class of convenience that is invisible
until it is wrong, and it is wrong in at least three ways users can see (a
partially-copied file after a failure is not cleaned up by the code that knows
it should be; `--backup` was resolved on the assumption that a rename either
replaces or refuses; and a directory tree gets copied by machinery `mv` does
not control). But if you disagree, or if the mount-comparison is more awkward
than it looks from here, tell me and I will record it in `open-questions.md`
for the operator rather than treat it as settled.
