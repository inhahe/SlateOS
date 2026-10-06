## D-POSIX-A-NATIVE-FILE-MAPPING-IS-A-COPY — a native program's `mmap` of a file is a copy read in at once, and a writable shared one is refused (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A (`requests/d-a-a-native-program-cannot-map-a-file.md`).

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
