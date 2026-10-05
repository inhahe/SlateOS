## §662 — `DirEntry` carries the filesystem's inode, and 647/664's record grows a `u64 ino`

**Date:** 2026-08-31 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** Every operating system reports two numbers that are supposed to
be the same one: the identity a file has when you *list* the directory it is
in, and the identity it has when you *ask about the file directly*. Ours were
never the same number, because listing did not report an identity at all and
the compatibility layer made one up from the file's name. Programs compare
those two numbers to decide whether two names are the same file — that is how
`cp` refuses to copy a file over itself, and how `tar`, `du` and `rsync` avoid
counting one file twice — and when the numbers disagree, none of those
programs report an error. They just get the answer wrong. The directory-listing
record now carries the real number, so the two agree.

### The problem

`fs::vfs::DirEntry` had three fields — name, type, size — and no identity.
Every consumer that needed one had to invent it, and two of them did, in
different ways:

- `posix`'s `Dirent::d_ino` used the entry's *position in the listing*, which
  changes when a neighbour is created or removed.
- The Linux-ABI `getdents64` used `synth_inode`, an FNV-1a hash of
  `dir_path + "/" + name` (`kernel/src/syscall/linux.rs:43243`). Stable and
  collision-resistant — and, being a hash of the path, *guaranteed* never to
  equal the inode `stat` reports for the same file.

The second is the interesting failure, because it is the one that looks
correct. `fill_stat_from_meta` writes `meta.ino` to `st_ino`
(`linux.rs:19927`), which for ext4, btrfs, f2fs, zfs, ntfs and memfs is a real
number from the backing filesystem. So `ls -i` and `stat` printed different
inodes for the same file, `find -inum "$(stat -c %i f)"` matched nothing, and
`tar`, `rsync`, `du` and `cp --preserve=links` — all of which detect hard
links by comparing `(st_dev, st_ino)` pairs against `d_ino` — silently drew
the wrong conclusion. No syscall returned an error in any of those cases. The
values were plausible; only their relationship was wrong.

Lane B independently reached the same conclusion from the other side while
wiring `SYS_FS_GETDENTS_PINNED` (664), and asked for the field
(`requests/b-a-*`). Their framing is the sharper one: *an invented `d_ino` is
worse than an absent one*, because an absent one is a value userspace can test
for, and an invented one is a value userspace believes.

### The decision

1. **`DirEntry` gains `pub ino: u64`, bound at construction by every
   backend.** All 91 construction sites across 13 filesystems now pass the
   identity that filesystem already had in hand — ext4's `child_ino` straight
   out of the directory block, btrfs's `location.objectid`, zfs's ZAP object
   number, f2fs's node number, NTFS's MFT record number, memfs's per-node
   counter, FAT's first cluster. In every case the value is the same one that
   filesystem's `metadata()` already reports as `st_ino`, so the two agree by
   construction rather than by coincidence.

2. **`0` means "this filesystem has no stable per-object identity", not
   "unknown" and not "deleted".** It is the convention `FileMeta::ino` already
   used, and the filesystems that report it here are exactly the ones that
   report it there: `procfs`, `sysfs`, `devfs`, `iso9660`, and FAT files with
   no allocated cluster. A caller reading 0 from a listing and 0 from a stat
   is being told one consistent thing.

3. **647 and 664's shared record grows a trailing `u64 ino`**, making it
   `u8 type | u32 name_len | name | u64 size | u64 ino` — 21 + `name_len`
   bytes. Both handlers now compute the length with the one
   `entry_record_len`; 647 had carried its own inline copy of the formula,
   which is precisely the drift that helper's own doc comment warns about, and
   664's ABI note defines its record as "the same encoding 647 uses", so
   agreement had better not depend on two authors remembering to edit both.

4. **`getdents64` prefers `ent.ino` and keeps the hash only as a fallback**
   for the filesystems with no identity at all.

### Alternatives rejected

**Let userspace stat each entry.** Correct, and it is what a caller must do
anyway when `d_type` comes back `DT_UNKNOWN` — but it is one syscall per
entry, which is the cost `readdir` exists to avoid, and it would not have
fixed `d_ino` itself. The field would have stayed a lie that a diligent caller
worked around and an ordinary one believed.

**A new syscall number rather than a wider record.** The usual reason to add a
number instead of widening is that widening breaks existing decoders. There
are none: 664 has never had a caller, 647's only consumer is `posix`, which
decodes 603's fixed 264-byte format and not this one, and lane B confirmed
they had not yet written the decoder. Changed now precisely *because* that
window is open; once B wires 664 it closes permanently and the same change
costs a second syscall number forever. Same reasoning as §653.

**Omit the field on filesystems that have no inode.** A record whose shape
depends on its contents cannot be decoded by a reader that has not already
decoded it. A fixed layout with a documented sentinel is strictly better.

**Emit a literal `0` for `d_ino` in `getdents64` on identity-less
filesystems, instead of the hash.** Rejected, but it is the closest call here.
In favour: it would make `d_ino` and `st_ino` agree *everywhere*, since
`st_ino` is also 0 on those filesystems, which is the entire point of this
change. Against: `getdents64` is the Linux ABI, and a Linux readdir loop is
entitled to read `d_ino == 0` as a deleted entry and skip the name — glibc's
`readdir` does not, but enough ported code does that emitting 0 would risk
losing entries from `/proc` and `/dev` listings outright. Losing a name is a
worse failure than reporting an identity that a stat will not confirm, so the
hash stays. The residual disagreement is real and is logged in
`known-issues.md` as `A-GETDENTS64-D-INO-DISAGREES-WITH-ST-INO-ON-PSEUDO-FILESYSTEMS`.

### What this does not fix

`posix`'s `Dirent::d_ino` is lane B's, and still carries the position index
until they decode the new field. That is theirs to change and the request they
filed says they will.

### Testing

The VFS pagination selftest now asserts `entry.ino == Vfs::metadata(path).ino`
for every entry of a ten-file directory, and separately that none of them is 0
— memfs assigns every node a number at creation, so a 0 there would mean the
listing dropped it rather than that the filesystem had none. That second
assertion is the one that matters: a listing reporting synthesised numbers
would pass every other check in that test, including the first one, if the
same synthesis were used on both sides.
