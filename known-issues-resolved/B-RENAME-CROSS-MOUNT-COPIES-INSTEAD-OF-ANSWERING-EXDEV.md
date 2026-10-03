## B-RENAME-CROSS-MOUNT-COPIES-INSTEAD-OF-ANSWERING-EXDEV — **FIXED 2026-09-12**

**Both conditions this entry named for its own closure are met.** It said it
would stay open "until the kernel actually answers `CrossDevice`, because until
then `mv`'s fallback remains unreachable on the target and the three-line
`CrossDevice` deletion in `try_pinned_renameat` cannot be made".

1. **The kernel answers.** Lane A made `Vfs::rename` return
   `KernelError::CrossDevice` for a cross-mount rename, with a `vfs_selftest`
   case that fails if a cross-mount rename succeeds. `sys_fs_rename` forwards
   it, so the path-based route refuses exactly as `SYS_FS_RENAMEAT_PINNED`
   (670) always did.
2. **The deletion is made.** `try_pinned_renameat` no longer declines 670's
   `CrossDevice` answer. Both routes now give the same answer, so `renameat`
   has one contract again rather than two chosen by the shape of its arguments.

**Why the exception existed and why removing it is not a reversal.** It was the
family's one deliberate departure from "a pinned call that answers has
answered", and the reason was sound: the two routes did *different operations*.
670 refused; the path call copied and deleted. Declining gave up nothing,
because a pin could never have covered a copy. What changed is not the
judgement but the premise — there is no second operation left to fall back to.

**The more expensive half of leaving it would have been the comment.** The
fallback cost one extra syscall to reach the same answer; the twenty lines
explaining it would have gone on describing a copy-then-delete that no longer
happens, to every reader who came after. Stale reasoning outlives stale code,
because code gets exercised.

**A test was renamed rather than deleted.**
`a_cross_device_answer_is_final_everywhere_but_the_rename` is now
`a_cross_device_answer_is_final_for_every_pinned_call`. Its assertion did not
change — `pinned_answer` always called `CrossDevice` final, and that is still
the thing worth pinning, because the cheap way to have written the original
exception was a line in `pinned_answer` that would have silently handed it to
all seven pinned calls. The test is what would have caught that. Only its name
was a claim about the world, and only the name was wrong.

`cargo test -p posix --target x86_64-pc-windows-gnu`: 20702 passed, 0 failed.

Found by `scripts/check-stale-blockers.py`, which flagged this entry for citing
a request that had reported itself `DONE`.

The description below is kept in the tense it was written in.

**In short:** `rename(2)` on this system never returns `EXDEV`. Asked to rename
across a mount boundary, the kernel's `SYS_FS_RENAME` copies the file and deletes
the original, and reports success. Every other Unix refuses, and POSIX says it
must; `mv`'s entire cross-device fallback — the one made attribute-preserving on
2026-09-01 — is therefore unreachable on the target, and reachable only on the
Linux and Windows hosts the unit tests run on.

**Why it matters more than a spec deviation.** The kernel's copy is not `mv`'s
copy, and the differences are user-visible:

- **It is not `mv`'s.** `-v` prints `renamed 'a' -> 'b'` for something that was
  copied; `--backup` has already been resolved on the assumption that a rename
  either replaces or refuses; a partially-copied file left by a failure is not
  cleaned up by the code that knows it should be.
- **It moves directories silently.** `mv` refuses a cross-device directory move
  (`B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`) because it has no recursive
  walk yet. If the kernel's rename copies a directory tree instead, the refusal
  never fires and the tree is copied by machinery `mv` does not control.
- **It cannot be pinned.** `SYS_FS_RENAMEAT_PINNED` (670) refuses a cross-mount
  rename precisely because a pin cannot span a copy — a `stat`, a `copy`, a
  `set_permissions`, a `set_owner` and a `remove`, each taking and releasing its
  own lock (lane A's `design-decisions.md` §666). So the operation that most
  wants the containment guarantee is the one shape that cannot have it, and the
  reason is that the kernel is doing something other than a rename.

**Where.** Kernel side: `sys_fs_rename`'s cross-mount arm — lane A's, not
writable from here. libc side: `posix/src/file.rs`, `rename_ex` forwards to
`SYS_FS_RENAME` and reports whatever it says, and `try_pinned_renameat` deletes
670's `CrossDevice` answer so the two routes agree (`design-decisions.md` §742).

**The proper fix.** A request to lane A to make `SYS_FS_RENAME` answer
`CrossDevice` for a cross-mount rename, in one commit with 670 already doing so,
after which:

1. `try_pinned_renameat`'s three-line `CrossDevice` fallback is deleted and the
   refusal is forwarded — both routes then agree on the POSIX answer.
2. `mv`'s `copy_across_devices` becomes live on the target for the first time,
   which is where the ordering matters: it must not be switched on before
   `B-MVS-CROSS-DEVICE-FALLBACK-DOES-NOT-PRESERVE-HARD-LINKS` and
   `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED` are closed, or a
   cross-mount `mv -r`-shaped move that silently worked would start failing.
3. Anything else in the tree that renames across mounts and relied on the copy
   has to grow its own fallback. Nothing does today.

**Ordering, therefore.** This is not a change to make first. The two `mv` gaps
above are the prerequisites, because closing this one converts a silent,
kernel-performed copy into `mv`'s copy, and `mv`'s copy currently refuses two
shapes the kernel's accepts. Doing them in the other order is a regression that
looks like a fix.

**UNBLOCKED 2026-09-03 — request filed.** Both prerequisites are now closed:
`B-MVS-CROSS-DEVICE-FALLBACK-DOES-NOT-PRESERVE-HARD-LINKS` on 2026-09-01 and
`B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED` on 2026-09-03. `mv` now
handles every shape across a filesystem boundary that it handles within one,
certified at 361/0/10 by `scripts/mv-diff.sh`, so the ordering hazard above no
longer applies and the change is safe for lane A to make. Asked for in
`requests/b-a-rename-across-a-mount-copies-instead-of-answering-exdev.md`.
This entry stays **OPEN** until the kernel actually answers `CrossDevice`,
because until then `mv`'s fallback remains unreachable on the target and the
three-line `CrossDevice` deletion in `try_pinned_renameat` cannot be made.

**How it would be caught.** `scripts/mv-diff.sh` runs against a real Linux and
already has a second filesystem (`@FAR@`, `$XDG_RUNTIME_DIR`), so it measures
GNU's behaviour correctly and cannot see this at all — the divergence is in the
SlateOS kernel, which the harness does not run. It needs a `vfs_selftest` case,
or a target-side test that renames between two mounts and asserts `EXDEV`.
