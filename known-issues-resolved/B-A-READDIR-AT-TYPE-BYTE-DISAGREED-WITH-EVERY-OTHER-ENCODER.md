## B-A-READDIR-AT-TYPE-BYTE-DISAGREED-WITH-EVERY-OTHER-ENCODER (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** the kernel tells a program what kind of thing each directory entry
is with a single number — 0 for a file, 1 for a folder, and so on. Four places
in the kernel write that number, and one of them had two of the values swapped:
it called a shortcut (symlink) 3 where everything else called it 2. Anything
reading a directory through that one call would have seen shortcuts reported as
disk labels and vice versa.

**Where:** `kernel/src/syscall/handlers.rs`, `sys_fs_readdir_at` (~line 10537).
It encoded `Symlink => 2, VolumeLabel => 3`, while `sys_fs_readdir` (~8333),
`encode_fs_stat_result` (~8509) and the second stat encoder (~9659) all use
`VolumeLabel => 2, Symlink => 3`. All four already agreed on `File => 0`,
`Directory => 1` and (as of `40404447a`) `CharDevice => 4`, which is the most
dangerous shape an ABI byte can take: a decoder written against either syscall
looks correct on every ordinary file and is wrong only on the two rare kinds.

**Fix:** aligned `sys_fs_readdir_at` to the other three, and documented the
encoding on the syscall's doc comment so the next encoder has something to copy
from. Safe to change silently because nothing outside the kernel decodes this
record yet — `userspace/strace` only names the syscall, and no libc or app path
reaches it.

**Not fixed, and deliberately:** `sys_fs_readdir_at` still emits a FAT volume
label as a record, where `sys_fs_readdir` skips it. It is paginated by an offset
into the directory, so dropping an entry would make `entries_written` disagree
with how far the offset actually advanced and the caller's next page would step
over a real neighbour. Filtering belongs where the offset is computed, in
`Vfs::readdir_at_resolved`, not where the record is packed. Low priority: FAT
volume labels appear only in the root of a FAT volume.
