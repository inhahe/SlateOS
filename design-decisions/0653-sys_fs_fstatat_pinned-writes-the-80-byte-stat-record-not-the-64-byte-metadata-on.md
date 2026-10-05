## 653. `SYS_FS_FSTATAT_PINNED` writes the 80-byte stat record, not the 64-byte metadata one — sharing an encoder is worth less than carrying the fields the caller needs

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous) — lane A, at lane B's
request (`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`)

**In short:** syscall 663 is the race-free `fstatat` — "tell me about this name
inside the directory *this handle* was opened on", so a directory swapped out
underneath you is refused rather than silently answered from the impostor. It
was written to fill the same 64-byte buffer that the plain `SYS_FS_METADATA`
call fills, so that userspace could reuse one decoder. But that 64-byte buffer
has no slot for the **inode number** — the small integer that identifies a file
on a disk, and the thing that tells you whether two names are the same file.
`fstatat`'s whole job is to fill a `struct stat`, which needs one. So 663 now
writes the wider 80-byte record instead. Nothing was calling 663 yet, which is
the only moment this is free.

### What was wrong with sharing the encoder

The original reasoning is in the constant's old doc comment, and it is a good
instinct in general: *one decoder in userspace should serve both, and a second
layout is a second thing to keep in step.* It fails here for a specific reason —
**the two callers do not want the same fields.**

| field | 80-byte `SYS_FS_STAT` record | 64-byte `FS_META_SIZE` record |
|---|---|---|
| size, type, timestamps, uid, gid, permissions, attributes | present | present |
| **inode number** (`st_ino`) | `[72..80]` | **absent** |
| **hard-link count** (`st_nlink`) | `[12..16]` | **absent** |
| **block count** (`st_blocks`) | `[32..40]` | **absent** |

`SYS_FS_METADATA` answers "describe this file"; 663 answers "fill a `struct
stat`". Those overlap in most fields and differ in exactly the three that
establish *identity* rather than content.

### Why a missing inode is worse than a missing field usually is

A zero `st_ino` does not fail. Nothing checks for it, nothing logs it, no call
returns an error. It is a **plausible** value, and it makes unrelated tools
quietly wrong — which is the failure mode that survives longest, because there
is no symptom to search for:

- **`cp src dst` where both name the same file.** The refusal that stops `cp`
  destroying your file is `st_dev`/`st_ino` equality. With every inode zero,
  *every* pair of files compares equal, so `cp` refuses copies it should
  perform — or, depending on the direction of the check, performs the one it
  should refuse.
- **`du` and `tar`** coalesce hard links by inode. All inodes equal means a
  whole tree counts as one file.
- **`find -samefile`** matches everything.
- **`ls -i`** prints a column of zeros — the only *visible* symptom, and the
  least harmful one.

`st_nlink` matters for the same reason at one remove: it is what `rm` consults
to decide whether removing a name destroys the data.

### The decision

663 writes the 80-byte record produced by `encode_fs_stat_result` — byte-for-byte
the record `SYS_FS_STAT` and `SYS_FS_LSTAT` already write, from the same
encoder. The "one decoder serves both" property is *preserved*, just against a
different partner: 663 now shares with 606/607, which is the pairing that
matches what the caller is building.

**Changed now because now is when it is free.** Lane B had not wired 663 into
`posix::fstatat` precisely because of this gap, so the call has no callers at
all. Widening a record that already has callers is not an edit — it is a new
syscall number, kept forever alongside the old one, with every caller needing to
know which it got. The cost of getting this right went from zero to permanent at
the moment someone called it.

### What was checked rather than assumed

The record being wider is worthless if the value is not there to put in it. The
pinned lookup ends at the same `guard.metadata()` the path-based route uses, so
it returns the same `FileMeta` from the same filesystem driver — but "so it
must be fine" is the reasoning that produced the bug in §652. There is now an
assertion in `Vfs::self_test` (the live-pin block that already existed for 662):
`metadata_at_pinned` must report a **non-zero** inode, and the **same** inode
and link count the path-based `metadata` reports for that one file. Compared
rather than merely checked non-zero, because two routes reaching *different*
inodes for one file is the same bug wearing a different mask.

### The filesystems that genuinely cannot supply an inode

Some report `ino == 0` because there is nothing to report — FAT has no inodes.
That is fine and is not what this entry is about: `stat` on those paths already
reports zero today by the same route, so 663 reports exactly what 606 would.
Lane B said the same in their request. The defect being fixed is a record with
**no field to put it in**, which loses the number even on the filesystems that
have one.

### How to reverse it

Point the handler back at `pack_fs_meta_to_user` and restore the
`FS_META_SIZE` validation. Do not do this once anything calls 663.
