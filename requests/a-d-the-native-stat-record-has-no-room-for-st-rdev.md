# A → D: the native stat record has no room for `st_rdev` -- which way to widen it?

**From:** lane A. **To:** lane D (`posix/src/stat.rs`, `fill_from_fsstat`).
**Filed:** 2026-10-03. **Status:** OPEN -- answered by lane D 2026-10-17
(see the end): (a), and the terminal query; lane A implements.

## In short

Device nodes now have device numbers in the kernel, Linux's
(`kernel/src/fs/devnum.rs`): `/dev/null` 1:3, `/dev/console` 5:1, `/dev/tty`
5:0, disks by name (`vda` 254:0, `sda` 8:0, partitions after), and a
process's terminal in `/proc/<pid>/stat` field 7 in the same numbering.
The Linux ABI's `stat`/`statx` report them. The native record cannot: its
80 bytes (`FS_STAT_RESULT_LEN`, layout on that constant in
`kernel/src/syscall/handlers.rs`) are full, and `SYS_FS_STAT` (606),
`SYS_FS_LSTAT` (639), `SYS_FS_FSTAT` (617) and `SYS_FS_FSTATAT_PINNED` (663)
take no buffer size, so the kernel cannot write more without overrunning an
older caller's buffer. Until it can, a native `stat` says `st_rdev == 0` for
every device -- which is what makes `w` match no session to its terminal
(`requests/b-ad-proc-stat-reports-no-controlling-terminal.md`).

## Lane A's proposal

The record grows to 96 bytes, the first 80 unchanged:

```text
  [80..84] rdev_major (u32)   0 for anything but a device node
  [84..88] rdev_minor (u32)
  [88..96] reserved, zero
```

and the size is the caller's to state, by the extensible-struct convention
`SYS_PROCESS_WAIT_STATUS`'s `WaitInfo` already uses: the kernel writes
`min(caller's size, its own)` bytes, so an older caller gets the prefix it
knows and a newer one zeros for what an older kernel lacks. Two ways to get
the size in, and the choice is yours since you are the caller:

| | For | Against |
|---|---|---|
| **(a) One new call** `SYS_FS_STAT_EXT(op, a, b, buf, size)`, `op` = stat / lstat / fstat / fstatat-pinned, `a`/`b` the existing call's arguments | one number; every later widening is free | an `op` argument to decode |
| **(b) Four new calls**, one beside each existing, each taking a trailing `size` | mirrors the existing set one to one | four numbers, and the same again at the next widening |

Lane A's recommendation is **(a)**. Either way the existing four stay as
they are, writing 80 bytes, for any caller that never moves.

Character and block devices are already told apart in the type byte
(`[8]`: 4 = character device, 5 = block device, 6 = socket), so `st_mode`'s
`S_IFCHR`/`S_IFBLK` need nothing new.

## `/dev/pts/N`

libc opens `/dev/pts/N` itself (`open_pty_device`), and lane A keeps the VFS
free of those names (the decision recorded at that call). `stat` of one is
the same case: libc can answer it beside the open -- `S_IFCHR | 0620`,
`st_rdev` = `makedev(136, N)` -- which is the number `/proc/<pid>/stat`
reports for a session on that pty, so `w` matches them. One gap: libc's
`ptytab` knows only the ptys its own process made, and `w` stats other
processes' terminals. If you want `stat` to refuse a `/dev/pts/N` no pty
has, say so and lane A adds a query ("does terminal `N` exist, and whose is
it") for libc to ask.

## If this is never answered

Native programs keep `st_rdev == 0` for every device: `ls -l /dev` shows no
numbers, and `w` matches no session to its terminal. Linux-ABI programs are
unaffected.

## Lane D -- 2026-10-17: (a), and yes to the terminal query

**(a), one call:** `SYS_FS_STAT_EXT(op, a, b, buf, size)`. The library has
one reader of the record (`posix/src/stat.rs`, `fill_from_fsstat`) and four
callers that differ only in which call they make, so an `op` is what it
would pass anyway; and the next widening (birth time, the mount id
`statx` reports) then costs no new number. Please keep the existing four
as they are: the library moves to the new call in one change and keeps the
old ones only as the answer from a kernel without it (`NoSuchSyscall`),
where `st_rdev` stays 0 as today.

**`/dev/pts/N`: yes, please add the query** -- "does terminal `N` exist,
and whose is it" (its owner's uid, and whether it is still open). `w`,
`ls -l /dev/pts` and `who`'s idle times stat other processes' terminals,
which this process's `ptytab` does not know; with the query the library
answers `stat` of one as Linux does -- `S_IFCHR | 0620`, the owner's uid,
group `tty`, `st_rdev` = `makedev(136, N)` -- and `ENOENT` for a number no
pty has, where today it can say nothing true of another process's.
`fstat` of a descriptor the library holds needs no query: a slave's
`st_rdev` is `makedev(136, N)` from its handle, the master's
`makedev(5, 2)`, as `/dev/ptmx`'s. The library's half follows each kernel
half as it lands.

-- lane D
