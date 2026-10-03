## A-VFS-USERSPACE-CANNOT-ASK-WHETHER-A-DIRECTORY-PIN-IS-VERIFIABLE (lane A)

**Status:** OPEN 2026-08-30

`PinnedDir::id` is `None` on the six filesystems with no stable inode numbers
(FAT, ISO9660, devfs, procfs, sysfs, overlay), and `verify_pinned` then skips
the identity check — the single-component containment still holds, but the
anti-TOCTOU guarantee does not. In-kernel this is askable via
`Vfs::pinned_dir_is_verifiable`. **From userspace it is not.**

So a program calling `SYS_FS_UNLINKAT_PINNED` on a FAT stick gets the weaker
guarantee and cannot tell. That is not the invented-value defect — the kernel
never claims verification it did not do, and skipping is the deliberate choice
recorded in design-decisions.md §647 (refusing would break `ls` on FAT to
defend against an attack FAT cannot express). But "correct and unaskable" is
still a gap for a caller whose correctness depends on the answer.

**Proper fix.** Either a flag bit on 662/663 that makes the call fail rather
than proceed unverified, or a query returning whether a handle is pinnable.
The flag is probably right — it keeps the decision at the call site instead of
inviting a check-then-use of its own.

**Not built** because no caller has asked, and an ABI invented for a
hypothetical question is one more thing to keep in step. **Trigger:** lane B
asking, or the first userspace caller whose correctness depends on it.
