## D-POSIX-A-NATIVE-FILE-MAPPING-IS-A-COPY — a native program's `mmap` of a file is a copy read in at once, and a writable shared one is refused (lane D, 2026-10-06)

**Status:** OPEN -- fixed on lane-d (`mmap` of a file is the kernel's,
`SYS_MMAP_FILE`, demand-paged), awaiting a boot on main. See "Fixed" at the end.

**In short:** programs read files by mapping them into memory (`mmap`). For
a native SlateOS program, that used to give memory full of zeros instead of
the file's bytes, silently, because the native kernel call cannot map a
file and the C library passed the request through anyway. Since 2026-10-06
the C library copies the file into fresh memory instead. The bytes are now
right, but it is a copy, made all at once. And a mapping meant to write the
file back (`MAP_SHARED` with `PROT_WRITE`) is refused with `ENODEV`, because
the writes would never reach the file. So SQLite's WAL mode and LMDB cannot
work until the kernel maps files for native programs.

**Where:** `posix/src/mman/file_map.rs` (the copy, and the refusals in
Linux's order); `posix/src/mman.rs` (`mmap` sends a file mapping there).

**How it differs from Linux, until the kernel maps files:**

| What | Linux | Here |
|---|---|---|
| When the file is read | each page at its first touch | all of it, at `mmap` |
| Memory it costs | the pages touched | the whole range, at once |
| A read-only `MAP_SHARED` mapping, after another writes the file | shows the new bytes | keeps the old ones |
| A page wholly past the end of the file | SIGBUS when touched | zeros |
| `MAP_SHARED` with `PROT_WRITE` | writes reach the file | `ENODEV` |
| `madvise(MADV_DONTNEED)` on it | reads the file again | `EINVAL`, as for all memory here |
| `mremap` growing it | maps more of the file | `ENOSYS`, as for all memory here |
| `/proc/self/maps` | names the file | anonymous memory |

**What was wrong before** (fixed 2026-10-06): `mmap` passed `prot` as the
native flags, the Linux `flags` as the native physical address, and the
descriptor and offset where `SYS_MMAP` reads nothing. A file mapping was
anonymous memory -- zeros -- with no error. No fixture mapped a file, so
nothing noticed; `services/ctest-mmap-file` does now.

**The proper fix:** native file mappings in the kernel, demand-paged
(`VmaKind::FileBacked` already serves the Linux ABI), `MAP_PRIVATE` first,
then a writable `MAP_SHARED`. Then `mmap` hands a file mapping to the kernel,
and `file_map.rs` goes.

**Fixed (2026-10-17, lane D):** lane A's kernel has mapped files for native
programs since 2026-10-07 (`SYS_MMAP_FILE`, 1144), and `file_map.rs` now
hands it every mapping that passes this library's refusals -- the ones only
it can make, from the descriptor's kind and access mode, in Linux's order --
instead of copying. A private mapping is demand-paged, a shared one maps the
page cache and shows others' writes, and the mapping keeps its own
reference to the file. A writable shared mapping is the kernel's to grant:
main's kernel answers `ENOSYS`, still reported as `ENODEV`, and lane A's next
(lane-a-wip `9b218f242`, design-decisions §1572) saves its stores, which
`msync` now saves at once through `SYS_MSYNC` (1177; a kernel without it has
nothing to save). `services/ctest-mmap-file` takes either answer at 62 and,
when mapped, checks a store reaches the file after `msync(MS_SYNC)`; it
passes on Linux that way. Of the table above, the rows that remain this
library's are none; `madvise(MADV_DONTNEED)` and `mremap` are the kernel's.
