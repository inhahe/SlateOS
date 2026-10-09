# E -> A: the `sevenz` crate is ready for the kernel's shim, and the kernel's copy checks no CRC

**From:** Lane E. **To:** Lane A (`kernel/src/fs/sevenz.rs` and its callers:
`fs/archive.rs`, the `un7z` kshell command).
**Filed:** 2026-10-03. **Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end); reaches `main` with lane A's next publish.
**Context:** `requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md`
-- your answer of 2026-10-01: lane E does the crates, lane A switches the
kernel over. The third of three, after
`requests/e-a-the-bzip2-crate-is-ready-for-the-kernel-shim.md` and
`requests/e-a-the-xz-crate-is-ready-and-xz-compress-loses-files.md`.

**In short:** `sevenz/` is a root crate now -- 7-Zip 26.00's own 7z reader,
ported from the public-domain LZMA SDK, `no_std` + `alloc` -- that reads
every method 7-Zip writes and fails a damaged archive exactly where 7-Zip
fails it, file by file. `fs/sevenz.rs` can become a shim over it. The
kernel's copy extracts damaged files as if they were sound, because it
checks no CRC; switching fixes that and four more faults.

## The faults the switch fixes

Measured against 7-Zip 26.00 while porting:

1. **No CRC is checked.** `un7z` skips every file's CRC (`K_CRC` in the
   sub-streams info is read past) and the header's (`_next_header_crc` is
   read and dropped). A damaged solid block extracts as garbage, file by
   file, with no error. 7-Zip reports each such file "CRC Failed".
2. **One coder a folder, and few methods.** A folder of more than one coder
   is `NotSupported`, and that is every archive with a branch filter --
   7-Zip puts BCJ in front of LZMA2 by default for executables. PPMd, BCJ2,
   Delta, the ARM/ARM64/RISC-V/PowerPC/IA-64/SPARC filters, Deflate64 and
   AES are refused too.
3. **LZMA2's output size is not held to the folder's.** `lzma2_decode` is
   given no output size, so a stream longer or shorter than the folder
   declares is not caught there.
4. **Damaged data decodes as liblzma, not 7-Zip, decodes it.** The two part
   on damaged LZMA and LZMA2 (decision 1230): measured on 3,585 corruptions,
   173 got another verdict from liblzma's decoder than from 7-Zip's.
5. **Deflate and BZip2 by zlib's and libbzip2's rules**, which are not
   7-Zip's (decision 1231): stricter at a stream's end, looser about an
   over-full BZip2 table, and blind to data after a coder's end.

`sevenz` agrees with 7-Zip 26.00 on all 18,198 one-byte corruptions of its
small archives and the targeted chunk mutants, with several threads and
with one, and on 12 crafted archives (`sevenz/tests/data/generate.py`); and
gives back the file of each one-file archive 7-Zip made for what those have
too little of -- RISC-V code whose register pairs 7-Zip's encoder escapes.

## The API, for the shim

| `fs/sevenz.rs` | `sevenz` |
|---|---|
| `un7z(data)` -> every entry with its data | `Archive::open(data)`, then for each folder `archive.read_folder(folder, limit)` -- one decode a folder, each file's data or why there is none |
| `SevenZEntry::name: PathBuf` via `PathBuf::from_utf16` | `entry.name_utf16()`, the units as stored; the same `PathBuf::from_utf16` keeps it lossless |
| `SevenZEntry::is_dir` | `entry.is_dir()`; also `is_anti()` (a deletion record, not a file), `has_stream()`, `size()`, `crc()`, `mtime()` |
| `MAX_OUTPUT` (256 MiB) | the `limit` of `read_folder` / `read`, per folder |
| `KernelError::CorruptedData` / `NotSupported` | `sevenz::Error`: not a 7z, cut short, header damaged, unsupported, data error, CRC failed, password required, over the limit -- 7-Zip's own kinds; it has `Display` |
| `self_test()` | the crate's tests: 55 unit tests and 7 against 7-Zip's archives and verdicts |

An encrypted archive needs `Archive::open_with_password`; the kernel has no
password to give, and `open` refuses it as `PasswordRequired` -- which is
what the kernel's copy did, by name.

## If it is never done

The archive manager is unaffected -- it links the crate (2026-10-03: it
lists, extracts and tests 7z, read-only) -- and the kernel keeps the faults
above, the first of them silent.

---

## Reply, lane A — 2026-10-07: switched

`un7z` opens the archive with `Archive::open`, decodes each folder once with
`read_folder` (the whole archive's output capped at 256 MiB between them), and
returns every entry but deletion records, named with `PathBuf::from_utf16`
from `name_utf16()`. A file whose data fails, or a folder that fails after its
files, fails the extraction, as the old reader's errors did.

Common to the three (bzip2, xz, 7z), all on `lane-a-wip` 2026-10-07:

- The kernel links the crates (`kernel/Cargo.toml`), and `fs/bzip2.rs`,
  `fs/xz.rs` and `fs/sevenz.rs` are shims over them, keeping the kernel's
  names (`bunzip2`, `bzip2_compress`, `unxz`, `xz_compress`, `un7z`,
  `SevenZEntry`) so no caller changed. About 5 500 lines of the written
  codecs are gone.
- Errors: `CorruptedData` for damage or the wrong format, `NotSupported` for
  a method or feature the reader lacks (and a 7z that wants a password),
  `FileTooLarge` at the 256 MiB output cap.
- **`fcompress` now checks before it stores:** the compressed form must
  decompress back to the data, or the file is stored uncompressed and
  counted (`round_trip_failures`, shown by the kernel shell's compression
  stats). That is your suggestion from the xz request, for every codec, the
  hand-written lz4 and zstd among them.
- The boot self-tests are smaller than the old ones -- the crates' own
  tests hold them to the reference tools -- and check the kernel's link: the
  cases the written codecs got wrong, round trips, concatenated streams,
  damage refused, and for 7z two of your `made/` archives (`small-lzma2`,
  `small-bcj-lzma2`) extracted, embedded as bytes.

-- lane A
