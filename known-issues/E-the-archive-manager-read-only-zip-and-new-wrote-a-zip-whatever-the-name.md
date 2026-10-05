### [E] The archive manager read only ZIP, and "New" wrote a ZIP whatever the name -- 2026-09-26
**Status:** FIXED for every format the program knows. TAR and TAR.GZ (lane E, 2026-09-26), TAR.BZ2 (2026-10-03: read and written through the new `bzip2` crate, a port of libbzip2 1.0.8), TAR.XZ (2026-10-03, through the new `xz` crate, a port of liblzma 5.2.5; it writes what `xz -6` writes, byte for byte), and 7z (2026-10-03, through the new `sevenz` crate, 7-Zip 26.00's own reader ported: listed, extracted and tested, each file given 7-Zip's verdict). 7z is read-only: Add and Delete are greyed out on a 7z and say why, and New does not offer it -- a 7z writer would be new work (`xz` has the LZMA2 encoder; the 7z header writer does not exist). An encrypted 7z is listed and its files refused until the program asks for a password. What remains open is the TAR-parser note at the end.

**Also fixed 2026-09-26: the Open dialog showed only ZIP files.** Its filter
was `*.zip` alone, written when ZIP was all the program read, so once TAR and
TAR.GZ opened, the dialog still hid every one of them. The filter and the
program's own name detection now read one table, `ArchiveFormat::patterns`,
so a name the program recognises is always one the dialog shows. A `.tar.xz`
was not recognised at all ("does not end in an archive extension I know");
it was then named and refused as TAR.XZ, until it could be read and written
(2026-10-03).

**In short:** the archive manager opened ZIP files and nothing else: a `.tar`
or `.tar.gz` -- the commonest archives on a Unix-like system -- was refused
with "this build reads ZIP only". And "New archive" wrote an empty ZIP
whatever the user named it, so a new `backup.tar` held ZIP bytes under a TAR's
name, which nothing then opened as either.

**Fixed.** A new crate, `apps/tararchive`, lists and writes TAR: ustar, GNU
(long names and links, base-256 numbers) and PAX (path, linkpath, size, mtime
records), every header's checksum checked, a damaged archive listed as far as
it reads with where it stopped. There were three TAR parsers in the tree --
the kernel's, coreutils' `tar`, `undelete`'s -- and none a crate could use.
The archive manager opens TAR in place and TAR.GZ inflated (under the same
512 MiB cap as everything else), by what the bytes are rather than the name;
lists members (a `./` root is not a member, no name keeps its `./`); extracts
files and folders, a hard link as a copy of its target, and refuses symbolic
links (a link can point outside the destination) and devices by name; Test
reads every member back and says where a damaged archive stops; Add, Delete
and New write TAR and TAR.GZ in their own format (a TAR streamed, a TAR.GZ
built and compressed, both refused up front past the memory budget). The CRC
column is blank for TAR, which keeps no checksum of a member, rather than a
column of zeros.

**Where.** `apps/tararchive`; `apps/archivemanager/src/backend.rs`:
`parse_tar`, `extract_tar`, `verify_tar`, `save_tar`, `create_empty`,
`SeekReader`, `copy_bytes`; `ArchiveEntry::crc32` is an `Option`;
`ArchiveModel::damage`, `ArchiveTestResults::damage`.

**Still open.** Other copies of TAR parsing -- `kernel/src/fs/tar.rs` (lane
A), `userspace/coreutils/src/bin/tar.rs` (lane B), `apps/undelete` -- could
use `tararchive`; not filed as requests yet, since the kernel's is `no_std`
and `tararchive` reads through `std::io`.
