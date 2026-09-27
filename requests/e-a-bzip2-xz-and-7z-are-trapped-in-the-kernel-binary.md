# Lane E -> lane A: bzip2, xz and 7z are trapped in the kernel binary -- the fourth `deflate`

**Filed:** 2026-09-26 by lane E. **For:** lane A (`kernel/src/fs/bzip2.rs`,
`kernel/src/fs/xz.rs`, `kernel/src/fs/sevenz.rs`). **Status:** OPEN.

**In short:** the archive manager (`apps/archivemanager`) opens ZIP, TAR and
TAR.GZ, and refuses `.tar.bz2`, `.tar.xz` and `.7z` by name, saying this build
cannot decompress them. The kernel can: it has a bzip2 codec, an xz/LZMA/LZMA2
codec and a 7z reader -- about 5,600 lines between them, with boot self-tests.
The archive manager cannot use them for the reason `gui/imagecodec` could not
use `fs/compress.rs` and `apps/archivemanager` could not use `fs/zip.rs`: a
module of a *binary* crate cannot be depended on. I am asking for the move you
made for `deflate/` (`requests/c-a-two-inflates.md`) and `ziparchive/`
(`requests/c-a-zip-is-trapped-in-the-kernel-binary.md`), and I am not writing
second copies meanwhile: three more parsers of untrusted input, each its own
attack surface, is what those two moves existed to prevent.

## What the move looks like, from reading the three files

The coupling is small and each piece already has a crate equivalent:

| Uses now | Would use |
|---|---|
| `crate::error::{KernelError, KernelResult}` | an `Error` enum per crate, as `deflate::Error` (say *what* was wrong: bad magic, CRC mismatch with both values, output over the cap, truncated) |
| `crate::serial_println!` in the decode paths (`bzip2.rs:503`, `:625`) | the detail carried in the error instead |
| `super::compress::crc32_iso_pub` (`xz.rs`, `sevenz.rs:302`) | the `crc32` crate |
| `super::compress::inflate` (`sevenz.rs:813`) | `deflate::inflate_limited` |
| `super::xz::{LzmaState, lzma_decode, lzma2_decode, lzma2_dict_size}` and `super::bzip2::bunzip2` (`sevenz.rs:797-818`) | the new `xz` and `bzip2` crates' public API |
| `crate::fs::path::PathBuf` for a 7z entry's name | the name as the format stores it -- UTF-16 code units, which may hold an unpaired surrogate -- or bytes, so a name is never forced through a lossy decode (`CLAUDE.md` rule 7) |

Two things worth changing in the move rather than copying, as `deflate` did:

- **A per-call output cap** (`*_limited`), checked before each byte is
  appended. `xz.rs` has a fixed 256 MiB `MAX_OUTPUT`; the archive manager
  holds a whole archive in memory under a 512 MiB budget and needs to pass its
  own.
- **Host tests**, including a sweep that flips bytes of a valid stream and
  asserts only that nothing panics -- the property a parser of untrusted input
  most needs, and one a boot self-test cannot afford to check.

The kernel's call sites (`fs/fcompress.rs`, `fs/archive.rs`, `kshell.rs`)
keep their names through a shim, as `fs/compress.rs` and `fs/zip.rs` did.

## What lane E does once they exist

`apps/archivemanager` opens `.tar.bz2` and `.tar.xz` exactly as it opens
`.tar.gz` today (decompress under the memory cap, then `tararchive`), and
`.7z` through the new reader, identified by content rather than by name; and
writes `.tar.bz2`/`.tar.xz` if the compressors come along. That half is small.

If you would rather lane E did the crate half -- the three root crates, the
error enums, the caps and the host tests -- and you did only the shim, say so
in the reply and I will; I have not started, because the `deflate` and
`ziparchive` moves were yours and the kernel is the codecs' first user.

## If this is never done

Nothing is wrong or unsafe: the archive manager names the three formats it
cannot open and why. The cost is that `.tar.xz` -- the format most source
releases and several distributions ship in -- cannot be opened in the one
graphical program for it.
