//! The archive back end: real bytes, on and off the disk.
//!
//! Everything in `main.rs` above this module is a *model* of an archive — a
//! list of entries, a tree, a selection, a set of columns. This module is the
//! only place that knows those entries came from a file, and the only place
//! that can put them back into one. The split is worth keeping: the window is
//! tested by driving it with a model, and the model is tested by round-tripping
//! it through here, so neither test needs the other's machinery.
//!
//! # What this build can read
//!
//! ZIP, via the workspace's [`ziparchive`] crate — the same parser the kernel
//! links, promoted out of `kernel/src/fs/zip.rs` at lane C's request precisely
//! so that this program would not have to grow a second one.
//!
//! TAR and TAR.GZ, via [`tararchive`] and the workspace's `deflate`: listed,
//! extracted, tested, and written back -- a member added or deleted, a new
//! archive created. A TAR is read in place like a ZIP; a TAR.GZ is one gzip
//! stream over the whole archive, so it is inflated into memory (under the
//! same [`MAX_ARCHIVE_BYTES`] as everything else) and rewritten whole. What
//! the bytes are decides, not the name: a gzipped `.tar` opens as a TAR.GZ.
//!
//! TAR.BZ2 and 7z are named by [`ArchiveFormat`] and refused here in words
//! rather than silently mis-parsed: `ArchiveError::NotYetReadable` says which
//! format it was. Each needs a decompressor this tree does not have.
//!
//! # An entry name is not a path
//!
//! A ZIP member name is an attacker-controlled byte string that lives inside a
//! file someone else wrote. It becomes a path only after [`safe_destination`]
//! has confined it under the directory the user chose. `../../etc/passwd` is a
//! legal member name and `..\..\etc\passwd` is one a Windows tool will happily
//! write; both are refused, and the refusal is reported rather than skipped
//! quietly, because an extraction that silently drops members is worse than one
//! that says what it would not do.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::{ArchiveEntry, ArchiveFormat, ArchiveModel, ArchiveTestResults, TestResult};

/// The largest archive this program will read into memory.
///
/// [`ziparchive`] is a whole-buffer API — `parse` takes `&[u8]` and every
/// entry is located by an offset into it — so opening an archive means holding
/// all of it. That is fine for the archives a person opens by hand and wrong
/// for a 40 GB backup set, so the limit is explicit and the refusal names the
/// size rather than letting the allocator decide. Recorded in
/// `known-issues.md`; the proper fix is a seeking reader on the crate side.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// The most this program will allocate to *rewrite* an archive.
///
/// [`MAX_ARCHIVE_BYTES`] bounds what an open costs, and for a long time it was
/// the only bound there was -- which left the rewrite unbounded, because the
/// two cost different things. A save holds three of that order at once: the old
/// archive it is reading members out of, every reproduced member's *plaintext*,
/// and the new archive being built. So the budget is three times the open
/// budget. It is a ceiling on this program, not a property of ZIP.
///
/// The gap this closes is compression ratio, and it is not a small one.
/// `MAX_ARCHIVE_BYTES` is checked against the file on disk, and a 500 MB
/// archive of highly compressible data holds *far* more than 500 MB of
/// plaintext -- a ZIP of zeroes expands about a thousandfold. Such an archive
/// passed the open check, then exhausted memory during a save, which is the
/// failure the open check exists to turn into a message. See known-issues.md
/// -> `TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY`.
pub const MAX_SAVE_BYTES: u64 = 3 * MAX_ARCHIVE_BYTES;

/// Why an archive operation could not be carried out.
#[derive(Debug)]
pub enum ArchiveError {
    /// The file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// The archive is larger than this program will hold in memory.
    TooLarge { bytes: u64 },
    /// The name does not end in any extension this program recognises.
    UnknownFormat { name: String },
    /// A format this program knows about but cannot yet read.
    NotYetReadable { format: ArchiveFormat },
    /// The ZIP parser refused the bytes.
    Zip(ziparchive::Error),
    /// A `.tar` whose first block is not a TAR header.
    NotTar { why: tararchive::Damage },
    /// A `.tar.gz` whose gzip stream will not inflate.
    Gzip(deflate::Error),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::TooLarge { bytes } => write!(
                f,
                "the archive is {} and this program reads archives up to {}",
                guitk::bytes::iec(*bytes),
                guitk::bytes::iec(MAX_ARCHIVE_BYTES)
            ),
            Self::UnknownFormat { name } => {
                write!(f, "{name} does not end in an archive extension I know")
            }
            Self::NotYetReadable { format } => write!(
                f,
                "{} — this build reads ZIP, TAR and TAR.GZ",
                format.display_name()
            ),
            Self::Zip(e) => write!(f, "{e}"),
            Self::NotTar { why } => write!(f, "it is not a TAR archive: {why}"),
            Self::Gzip(deflate::Error::OutputTooLarge) => write!(
                f,
                "it inflates to more than {}, the most this program reads",
                guitk::bytes::iec(MAX_ARCHIVE_BYTES)
            ),
            Self::Gzip(e) => write!(f, "its gzip stream will not inflate: {e}"),
        }
    }
}

/// The bytes an [`ArchiveModel`] was built from, and the central-directory
/// record each of its entries came from.
///
/// Held on the model because extraction and verification need the compressed
/// bytes, and the model is the only thing that outlives the click that opened
/// the file.
///
/// **Keyed by [`ArchiveEntry::id`], not by index.** The list is sorted in place
/// every time the user clicks a column header, so a parallel `Vec` would name a
/// different member after the first click — and the member is what decides
/// where the bytes are, so the mistake would extract one file's contents under
/// another's name rather than merely showing the wrong row.
// Not `Clone`, since 2026-09-16: this holds an open file, and a derived clone
// of one is either a second handle with its own cursor or a panic, neither of
// which is what a caller writing `.clone()` expects. Nothing cloned it -- the
// derive predates the file and outlived the bytes it was cheap for.
pub struct ArchiveSource {
    /// Where the archive's bytes are, and how to reach them.
    bytes: ArchiveBytes,
    /// Each member's record, by the id the model gave its entry.
    members: Members,
}

/// The parser's own record of each member: a ZIP's central directory, or a
/// TAR's headers.
enum Members {
    Zip(HashMap<u64, ziparchive::ZipEntry>),
    Tar(HashMap<u64, tararchive::Entry>),
}

impl Members {
    fn len(&self) -> usize {
        match self {
            Self::Zip(m) => m.len(),
            Self::Tar(m) => m.len(),
        }
    }
}

/// Where an archive's bytes live.
///
/// The application holds a *file*, not its contents. `ziparchive`'s ranged
/// entry points (`parse_at`, `entry_data_at`) read the central directory from
/// the tail and then one member at a time, so opening a 900 MB archive to look
/// at its listing costs a handle and a `Vec<ZipEntry>` rather than 900 MB --
/// which is the measurement that
/// `requests/c-a-ziparchive-wants-a-ranged-reader-and-a-streaming-writer.md`
/// was filed on.
///
/// `Memory` is not a second code path for the application: nothing in `open`
/// can produce one. It exists because the tests build fixtures with
/// `ziparchive::create` and read them straight back, and making each of them
/// write a temporary file would test the filesystem rather than the parser.
pub enum ArchiveBytes {
    /// A file on disk, read at offsets.
    ///
    /// `RefCell` because a positional read is logically not a mutation -- it
    /// answers "what is at this offset" without changing what the archive is
    /// -- while the portable way to perform one moves a cursor. Without it,
    /// `&ArchiveSource` would have to become `&mut ArchiveSource` through
    /// every caller in `main.rs`, which would spread a detail of how reading
    /// is implemented across code that only wants to look at an entry.
    File {
        /// The open archive.
        file: std::cell::RefCell<fs::File>,
        /// Its length, asked once at open time.
        len: u64,
    },
    /// Bytes already in hand. The tests' fixture source.
    Memory(Vec<u8>),
}

impl ArchiveBytes {
    /// Read at `offset`, without needing a mutable borrow of the archive.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::File { file, .. } => {
                use io::{Read as _, Seek as _};
                let mut file = file.borrow_mut();
                file.seek(io::SeekFrom::Start(offset))?;
                // `read` may return short for reasons that are not EOF, and
                // `ReadAt`'s contract says short means end of source. Looping
                // here keeps that promise rather than reporting a truncated
                // archive because a read was split.
                let mut done = 0;
                while done < buf.len() {
                    let Some(rest) = buf.get_mut(done..) else {
                        break;
                    };
                    match file.read(rest) {
                        Ok(0) => break,
                        Ok(n) => done = done.saturating_add(n),
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(done)
            }
            Self::Memory(bytes) => {
                let start = usize::try_from(offset).unwrap_or(usize::MAX);
                let Some(rest) = bytes.get(start..) else {
                    return Ok(0);
                };
                let n = rest.len().min(buf.len());
                let (Some(dst), Some(src)) = (buf.get_mut(..n), rest.get(..n)) else {
                    return Ok(0);
                };
                dst.copy_from_slice(src);
                Ok(n)
            }
        }
    }

    /// The archive's length in bytes.
    const fn len(&self) -> u64 {
        match self {
            Self::File { len, .. } => *len,
            Self::Memory(bytes) => bytes.len() as u64,
        }
    }
}

/// A borrowing view of [`ArchiveBytes`] that satisfies `ziparchive::ReadAt`.
///
/// The trait takes `&mut self`, which is right for a source that really is
/// consumed as it is read. This one is not, so the mutability stops here
/// rather than being pushed out to every caller.
struct Reader<'a>(&'a ArchiveBytes);

impl ziparchive::ReadAt for Reader<'_> {
    type Error = io::Error;

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read_at(offset, buf)
    }

    fn len(&mut self) -> io::Result<u64> {
        Ok(self.0.len())
    }
}

impl fmt::Debug for ArchiveSource {
    /// Length, not contents. `ArchiveModel` derives `Debug`, and an archive is
    /// megabytes: a derived `Debug` here would turn one `dbg!` or one failing
    /// `assert_eq!` on a model into a screenful of hex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchiveSource")
            .field("bytes", &self.bytes.len())
            .field("members", &self.members.len())
            .finish()
    }
}

impl ArchiveSource {
    /// A reader over the archive, for `ziparchive`'s ranged entry points.
    ///
    /// Replaces a `bytes() -> &[u8]` that handed out the whole file. Every
    /// caller of that took the slice only to pass it straight to
    /// `ziparchive`, so nothing lost a capability -- what went is the
    /// requirement that the file be in memory to have one.
    fn reader(&self) -> Reader<'_> {
        Reader(&self.bytes)
    }

    /// The archive's size in bytes.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.bytes.len()
    }

    /// The central-directory record the entry with this id came from, for a
    /// ZIP.
    #[must_use]
    pub fn member(&self, id: u64) -> Option<&ziparchive::ZipEntry> {
        match &self.members {
            Members::Zip(m) => m.get(&id),
            Members::Tar(_) => None,
        }
    }

    /// The header the entry with this id came from, for a TAR.
    #[must_use]
    pub fn tar_member(&self, id: u64) -> Option<&tararchive::Entry> {
        match &self.members {
            Members::Tar(m) => m.get(&id),
            Members::Zip(_) => None,
        }
    }

    /// How many members the archive declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }
}

/// Read `path` and parse it into a model.
///
/// # Errors
///
/// [`ArchiveError`] for a name with no known extension, a format this build
/// cannot read, a file that will not read or is too big to hold, or bytes the
/// ZIP parser refuses.
pub fn open(path: &Path) -> Result<ArchiveModel, ArchiveError> {
    let Some(format) = ArchiveFormat::from_path(path) else {
        return Err(ArchiveError::UnknownFormat {
            name: path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
        });
    };
    if matches!(format, ArchiveFormat::TarBz2 | ArchiveFormat::SevenZip) {
        return Err(ArchiveError::NotYetReadable { format });
    }
    // Ask the size before reading, so an archive too big to hold is refused by
    // name instead of by running the machine out of memory first.
    let size = fs::metadata(path)
        .map_err(|source| ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if size > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge { bytes: size });
    }
    // The handle, not the contents. `parse_zip` reads the central directory
    // through it and each member is read when it is actually extracted, so
    // opening a large archive to look at its listing no longer costs its size
    // in memory.
    let file = fs::File::open(path).map_err(|source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = ArchiveBytes::File {
        file: std::cell::RefCell::new(file),
        len: size,
    };
    if format == ArchiveFormat::Zip {
        return parse_zip(path, bytes);
    }
    // A TAR or a TAR.GZ, by what its bytes are rather than its name: gzip's
    // magic, or not.
    let mut magic = [0_u8; 2];
    let read = bytes
        .read_at(0, &mut magic)
        .map_err(|source| ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if read == 2 && magic == [0x1F, 0x8B] {
        let compressed = read_all(&bytes).map_err(|source| ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let limit = usize::try_from(MAX_ARCHIVE_BYTES).unwrap_or(usize::MAX);
        let tar = deflate::gunzip_limited(&compressed, limit).map_err(ArchiveError::Gzip)?;
        drop(compressed);
        parse_tar(path, ArchiveBytes::Memory(tar), ArchiveFormat::TarGz, size)
    } else {
        parse_tar(path, bytes, ArchiveFormat::Tar, size)
    }
}

/// All of `bytes`, read into memory -- a gzipped archive, to inflate.
fn read_all(bytes: &ArchiveBytes) -> io::Result<Vec<u8>> {
    let len = usize::try_from(bytes.len()).unwrap_or(usize::MAX);
    let mut buf = vec![0; len];
    let got = bytes.read_at(0, &mut buf)?;
    buf.truncate(got);
    Ok(buf)
}

/// [`ArchiveBytes`] as a `Read + Seek`, for [`tararchive::list`].
struct SeekReader<'a> {
    bytes: &'a ArchiveBytes,
    pos: u64,
}

impl io::Read for SeekReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.bytes.read_at(self.pos, buf)?;
        self.pos = self.pos.saturating_add(n as u64);
        Ok(n)
    }
}

impl io::Seek for SeekReader<'_> {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        let (base, offset) = match to {
            io::SeekFrom::Start(at) => {
                self.pos = at;
                return Ok(at);
            }
            io::SeekFrom::End(off) => (self.bytes.len(), off),
            io::SeekFrom::Current(off) => (self.pos, off),
        };
        self.pos = base.checked_add_signed(offset).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.pos)
    }
}

/// Parse `bytes` as a TAR -- inflated already, for a `.tar.gz` -- under the
/// name `path`. `on_disk` is the file's own size: a TAR.GZ's compressed size
/// is the whole stream's, since gzip compresses the archive at once and not
/// each member.
///
/// # Errors
///
/// [`ArchiveError::NotTar`] if the first block is not a TAR header, and
/// [`ArchiveError::Io`] if the bytes will not read. An archive damaged further
/// in is listed as far as it reads, and [`ArchiveModel::damage`] says where.
pub fn parse_tar(
    path: &Path,
    bytes: ArchiveBytes,
    format: ArchiveFormat,
    on_disk: u64,
) -> Result<ArchiveModel, ArchiveError> {
    let listing = tararchive::list(&mut SeekReader {
        bytes: &bytes,
        pos: 0,
    })
    .map_err(|source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if let tararchive::End::Damaged { at: 0, why } = listing.end {
        return Err(ArchiveError::NotTar { why });
    }
    let mut model = ArchiveModel::new(path, format);
    let mut by_id = HashMap::with_capacity(listing.entries.len());
    for member in listing.entries {
        let display = tar_display_path(&member.name);
        // `./` is the archive's own root, which `tar -C dir .` writes first.
        if display.is_empty() || display == "." {
            continue;
        }
        let name = display
            .rsplit_once('/')
            .map_or(display.as_str(), |(_, last)| last)
            .to_string();
        let carries = carries_bytes(member.kind);
        let size = if carries { member.size } else { 0 };
        let id = model.add_entry(ArchiveEntry {
            depth: u32::try_from(display.matches('/').count()).unwrap_or(u32::MAX),
            name,
            is_dir: member.kind == tararchive::Kind::Directory,
            size,
            compressed_size: size,
            modified: u64::try_from(member.mtime).unwrap_or(0),
            crc32: None,
            encrypted: false,
            method: tar_method(member.kind, format),
            path: display,
            expanded: false,
            selected: false,
            id: 0, // assigned by add_entry
        });
        by_id.insert(id, member);
    }
    if format == ArchiveFormat::TarGz {
        model.total_compressed = on_disk;
    }
    model.damage = match listing.end {
        tararchive::End::Damaged { at, why } => Some(format!(
            "the archive is damaged {} in -- {why}; nothing after that is listed",
            guitk::bytes::iec(at)
        )),
        tararchive::End::TooManyMembers => Some(format!(
            "only the first {} members are listed",
            tararchive::MAX_MEMBERS
        )),
        tararchive::End::Marker | tararchive::End::EndOfFile => None,
    };
    model.source = Some(ArchiveSource {
        bytes,
        members: Members::Tar(by_id),
    });
    model.rebuild_tree();
    Ok(model)
}

/// Whether a member of `kind` has bytes of its own in the archive. A kind
/// this does not know keeps whatever bytes its header says it has, so a
/// rewrite does not drop them.
fn carries_bytes(kind: tararchive::Kind) -> bool {
    matches!(kind, tararchive::Kind::File | tararchive::Kind::Other(_))
}

/// What the Method column says for a TAR member: how its bytes are stored,
/// or what it is when it has none.
fn tar_method(kind: tararchive::Kind, format: ArchiveFormat) -> String {
    match kind {
        tararchive::Kind::File if format == ArchiveFormat::TarGz => String::from("Gzip"),
        tararchive::Kind::File => String::from("Stored"),
        tararchive::Kind::Directory => String::new(),
        tararchive::Kind::Symlink => String::from("Symbolic link"),
        tararchive::Kind::HardLink => String::from("Hard link"),
        tararchive::Kind::CharDevice | tararchive::Kind::BlockDevice => String::from("Device"),
        tararchive::Kind::Fifo => String::from("Pipe"),
        tararchive::Kind::Other(flag) => format!("Type {}", char::from(flag)),
    }
}

/// A TAR member's name to show: [`display_path`], less the `./` that `tar -C
/// dir .` puts in front of every name.
fn tar_display_path(raw: &[u8]) -> String {
    display_path(without_dot_slash(raw))
}

/// Whether two member names are the same file: equal once each has lost the
/// `./` a TAR may put in front.
fn same_name(a: &[u8], b: &[u8]) -> bool {
    without_dot_slash(a) == without_dot_slash(b)
}

/// `name` less any `./` in front of it.
fn without_dot_slash(mut name: &[u8]) -> &[u8] {
    while let Some(rest) = name.strip_prefix(b"./") {
        name = rest;
    }
    name
}

/// How much of a member is copied at once: a member of a gigabyte is not
/// held whole to be written out.
const COPY_CHUNK: usize = 1024 * 1024;

/// Copy `size` bytes at `offset` in the archive to `out`, a chunk at a time.
fn copy_bytes(
    bytes: &ArchiveBytes,
    offset: u64,
    size: u64,
    out: &mut (impl io::Write + ?Sized),
) -> Result<(), SkipReason> {
    let mut buf = vec![0; COPY_CHUNK];
    let mut done = 0_u64;
    while done < size {
        let want = usize::try_from(size.saturating_sub(done))
            .unwrap_or(usize::MAX)
            .min(COPY_CHUNK);
        let chunk = buf.get_mut(..want).unwrap_or(&mut []);
        let got = bytes
            .read_at(offset.saturating_add(done), chunk)
            .map_err(SkipReason::Unreadable)?;
        if got < want {
            return Err(SkipReason::Unreadable(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the archive ends inside this member",
            )));
        }
        out.write_all(chunk).map_err(SkipReason::Io)?;
        done = done.saturating_add(want as u64);
    }
    Ok(())
}

/// Parse bytes already in hand as a ZIP, under the name `path`.
///
/// Separate from [`open`] because it is also how the tests build their fixture:
/// they write a real archive with `ziparchive::create` and read it back through
/// here, so the entry list a test asserts against came out of the same parser
/// the program uses rather than being typed in beside it.
///
/// # Errors
///
/// [`ArchiveError::Zip`] if the bytes are not a well-formed archive.
pub fn parse_zip(path: &Path, bytes: ArchiveBytes) -> Result<ArchiveModel, ArchiveError> {
    // `RangedError` keeps "the archive is malformed" and "the source could not
    // be read" apart, and this is where that distinction earns its keep: a
    // disk that stops answering must not be reported as a damaged archive.
    let members = ziparchive::parse_at(&mut Reader(&bytes)).map_err(|e| match e {
        ziparchive::RangedError::Zip(e) => ArchiveError::Zip(e),
        ziparchive::RangedError::Read(source) => ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        },
    })?;
    let mut model = ArchiveModel::new(path, ArchiveFormat::Zip);
    let mut by_id = HashMap::with_capacity(members.len());
    for member in members {
        let display = display_path(&member.name);
        let name = display
            .rsplit_once('/')
            .map_or(display.as_str(), |(_, last)| last)
            .to_string();
        let id = model.add_entry(ArchiveEntry {
            depth: u32::try_from(display.matches('/').count()).unwrap_or(u32::MAX),
            name,
            is_dir: member.is_dir,
            size: member.uncompressed_size,
            compressed_size: member.compressed_size,
            modified: dos_datetime_to_unix(member.dos_datetime),
            crc32: Some(member.crc32),
            // General-purpose bit 0, read rather than assumed. This column used
            // to be a hardcoded `false`, which was the right answer for every
            // archive SlateOS writes and the wrong one for every archive that
            // needs a password — and the two are indistinguishable to a user
            // looking at a column that always says the same thing.
            encrypted: member.is_encrypted(),
            method: method_name(member.method),
            path: display,
            expanded: false,
            selected: false,
            id: 0, // assigned by add_entry
        });
        by_id.insert(id, member);
    }
    model.source = Some(ArchiveSource {
        bytes,
        members: Members::Zip(by_id),
    });
    model.rebuild_tree();
    Ok(model)
}

/// Seconds since the Unix epoch for the MS-DOS date/time pair a ZIP central
/// directory stores, or `0` if it recorded no usable time.
///
/// `ziparchive` hands the pair over raw and undecoded, deliberately: it is
/// `no_std` and will not own a calendar. This is the caller that wants a date,
/// so the *rendering* decision lives here and the *calendar* decision does not:
/// [`guitk::tzrules::unix_from_dos_datetime`] answers "does this pair name a
/// real instant", and everything this function adds is the translation of its
/// `None` into the `0` that `ArchiveEntry::format_date` renders as `-`.
///
/// Two facts about the format shape that translation:
///
/// * **Zero is "not recorded", not an instant.** A zero pair is day 0 of month
///   0, which is not a date at all, so it maps to `0` — which
///   `ArchiveEntry::format_date` renders as `-`. Rendering it as 1980-01-01
///   would put a measurement where there is none. Archives SlateOS itself
///   writes carry zero (design-decisions.md §618), so this is the common case,
///   not an edge one. The same reasoning maps every *malformed* pair to `0`
///   too: a malformed date is an unknown date, not a guessed one.
///
///   Conflating the two is safe here for a reason worth stating, because it is
///   not safe in general: a DOS pair cannot name 1970 — the format's epoch is
///   1980 — so a `0` out of this function is never a real timestamp that
///   happens to be the Unix epoch. In a format that stored Unix seconds
///   directly it would be, and this shape would be wrong.
/// * **A DOS timestamp has no zone.** The format records wall-clock time as the
///   writer's machine saw it and stores nothing about where that machine was.
///   There is no correct conversion to an absolute instant, so `tzrules` reads
///   it as UTC — which is also what `format_date` renders in
///   (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`), making the round trip show back
///   exactly the digits the archive stored.
///
/// # Why both directions are shared and neither is written here
///
/// The encoder is [`guitk::tzrules::dos_datetime_from_unix`] (lane A, `37c04848e`)
/// and the decoder is [`guitk::tzrules::unix_from_dos_datetime`]. They pack and
/// unpack the same `(date << 16) | time` layout, and both take and return
/// **seconds**, not nanoseconds.
///
/// This app used to decode the pair itself, on the argument that the range
/// check was a rendering decision rather than a calendar one and so belonged
/// next to the column it fed. That argument was wrong, and it was wrong in a
/// way that shipped a bug: the local check tested `day` against a constant
/// `1..=31`, which is the *widest* month rather than *this* month, so
/// 2026-02-30 passed it and reached `days_from_civil` — the Hinnant algorithm,
/// which normalises rather than refusing — and came back rendering as
/// **2026-03-02**. A plausible date in roughly the right place, with nothing to
/// suggest the archive had said something impossible. 2026-02-29 in a common
/// year became March 1 the same way.
///
/// "Is 30 a day in February" is a question about the calendar, and only the
/// calendar crate knows the leap rule, so only the calendar crate can answer
/// it. What is genuinely a rendering decision — that an unanswerable pair
/// becomes a `-` in a column rather than an `Option` a caller must handle — is
/// the part that stayed, and it is these four lines.
///
/// Reported in `requests/a-c-the-dos-decoder-exists-now-and-yours-invents-a-date-in-february.md`;
/// the refusing-versus-normalising constraint both directions implement is
/// design-decisions.md §618, and where the pair lives is §621.
#[must_use]
pub fn dos_datetime_to_unix(pair: u32) -> u64 {
    guitk::tzrules::unix_from_dos_datetime(pair)
        .and_then(|secs| u64::try_from(secs).ok())
        .unwrap_or(0)
}

/// What to show in the Method column for a ZIP compression method number.
///
/// The two this build can decompress are named; anything else is shown by its
/// number, because "unknown" for methods 9, 12 and 14 alike would hide the one
/// fact that tells the user which tool wrote the archive.
fn method_name(method: u16) -> String {
    match method {
        0 => String::from("Stored"),
        8 => String::from("Deflate"),
        other => format!("Method {other}"),
    }
}

/// The path to *show* for a member, from the bytes the archive stored.
///
/// Lossy, deliberately and only here: a listing is the one place a name with no
/// UTF-8 reading still has to appear on screen, and a row that cannot be drawn
/// is worse than one drawn with a replacement character in it. Nothing is ever
/// *written* to this name — [`safe_destination`] works from the raw bytes and
/// refuses what it cannot render faithfully — so a substituted character can
/// mislabel a row but can never misplace a file.
///
/// The trailing `/` a directory member carries is dropped, because every other
/// part of the model addresses `src` rather than `src/`.
fn display_path(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .trim_end_matches('/')
        .to_string()
}

/// Why a member was not extracted.
#[derive(Debug)]
pub enum SkipReason {
    /// The name would put the file outside the chosen directory.
    Escapes,
    /// The name has no UTF-8 reading, so this host cannot name the file.
    UnnameableHere,
    /// The name has no components at all — `/`, or `././`.
    Empty,
    /// The member needs a password, and this build has no decryption at all.
    ///
    /// Separate from [`Self::Zip`] because the two say opposite things about the
    /// archive. Handing an encrypted member to the inflater does not fail
    /// cleanly: it decompresses ciphertext into whatever that happens to expand
    /// to, and the size or CRC check at the end rejects it as corrupt. That
    /// report is wrong twice over — it blames the archive for a file that is
    /// perfectly intact, and it tells a user whose only real problem is a
    /// missing password to go and find an undamaged copy.
    Encrypted,
    /// The member would not decompress.
    Zip(ziparchive::Error),
    /// The file or its directory could not be written.
    Io(io::Error),
    /// A symbolic link. Not made: a link can point outside the destination,
    /// and a member extracted after it could then be written through it.
    SymbolicLink,
    /// A device or a pipe, which is not a file to write.
    NotAFile,
    /// A hard link to a member the archive does not hold.
    LinkTargetMissing,
    /// The archive itself could not be read at that member's offset.
    ///
    /// Separate from [`Self::Io`], which is the *output* failing, and from
    /// [`Self::Zip`], which is the archive being malformed -- for the reason
    /// [`Self::Encrypted`] is separate from `Zip`. All three would otherwise
    /// print something false: "could not be written" about a file nothing
    /// tried to write, or a damaged archive about an intact one on a disk that
    /// stopped answering. `ziparchive::RangedError` keeps the last two apart
    /// at the source; this is where that distinction survives into what the
    /// user is told.
    Unreadable(io::Error),
}

impl From<ziparchive::RangedError<io::Error>> for SkipReason {
    /// Keeps the crate's distinction instead of flattening it: a malformed
    /// archive stays `Zip`, a source that would not answer becomes
    /// `Unreadable`.
    fn from(e: ziparchive::RangedError<io::Error>) -> Self {
        match e {
            ziparchive::RangedError::Zip(e) => Self::Zip(e),
            ziparchive::RangedError::Read(e) => Self::Unreadable(e),
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escapes => f.write_str("its name points outside the destination"),
            Self::UnnameableHere => f.write_str("its name is not text this system can write"),
            Self::Empty => f.write_str("its name is empty"),
            Self::Encrypted => f.write_str("it is encrypted and this build cannot decrypt"),
            Self::Zip(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Unreadable(e) => write!(f, "the archive could not be read: {e}"),
            Self::SymbolicLink => f.write_str(
                "it is a symbolic link, which this build does not make: a link can point outside the destination",
            ),
            Self::NotAFile => f.write_str("it is a device or a pipe, not a file"),
            Self::LinkTargetMissing => {
                f.write_str("it is a hard link to a member this archive does not hold")
            }
        }
    }
}

/// What an extraction did.
///
/// Every refusal is kept, not just the last one: a run that wrote 900 files and
/// refused 3 has to be able to say *which* 3, and a status line reporting only
/// the final error would report whichever happened to sort last.
#[derive(Debug, Default)]
pub struct ExtractReport {
    /// Files written.
    pub written: usize,
    /// Directories created because a member named one.
    pub directories: usize,
    /// Total uncompressed bytes written.
    pub bytes: u64,
    /// Members not extracted, in the order they were met.
    pub skipped: Vec<(String, SkipReason)>,
}

impl ExtractReport {
    /// A one-line summary for the status bar.
    #[must_use]
    pub fn summary(&self, dest: &Path) -> String {
        let files = format!(
            "{} file{}",
            self.written,
            if self.written == 1 { "" } else { "s" }
        );
        match self.skipped.first() {
            None => format!(
                "Extracted {files} ({}) to {}",
                guitk::bytes::iec(self.bytes),
                dest.display()
            ),
            // The first refusal is named in full and the rest are counted. A
            // status line that said "3 problems" would send the user looking
            // for a log this program does not keep; one that tried to name all
            // three would not fit.
            Some((name, why)) => {
                let more = self.skipped.len().saturating_sub(1);
                let tail = if more == 0 {
                    String::new()
                } else {
                    format!(" (and {more} more)")
                };
                format!("Extracted {files}; skipped {name} — {why}{tail}")
            }
        }
    }
}

/// Where `raw` may be written under `dest`.
///
/// The whole of the Zip Slip defence. A member name is refused unless every one
/// of its components is an *ordinary* name — which excludes `..`, absolute
/// roots, and Windows drive prefixes, the last of which matters because
/// `PathBuf::push("C:")` replaces the path it is pushed onto rather than
/// extending it, so a member called `C:evil` would land on the C drive and not
/// under `dest` at all.
///
/// Split on **both** separators. The format says `/`, but archives written by
/// Windows tools carry `\`, and a check that only knew about `/` would read
/// `..\..\etc\passwd` as one harmless component and then hand it to a
/// filesystem that does know about `\`.
///
/// # Errors
///
/// [`SkipReason`] naming why the member cannot be written.
fn safe_destination(dest: &Path, raw: &[u8]) -> Result<PathBuf, SkipReason> {
    // Strict, not lossy. A lossy conversion here would write the member out
    // under a name that is not its own — and, worse, two members differing only
    // in bytes with no UTF-8 reading would collapse onto one path and the
    // second would overwrite the first.
    let name = std::str::from_utf8(raw).map_err(|_| SkipReason::UnnameableHere)?;
    let mut out = dest.to_path_buf();
    let mut components = 0_usize;
    for part in name.split(['/', '\\']) {
        // A leading `/`, a doubled `//`, and a trailing `/` on a directory
        // member all show up here as an empty part. `unzip` strips a leading
        // separator rather than refusing the member, and so do we.
        if part.is_empty() || part == "." {
            continue;
        }
        let mut walk = Path::new(part).components();
        match (walk.next(), walk.next()) {
            (Some(Component::Normal(c)), None) => out.push(c),
            _ => return Err(SkipReason::Escapes),
        }
        components = components.saturating_add(1);
    }
    if components == 0 {
        return Err(SkipReason::Empty);
    }
    Ok(out)
}

/// Extract `members` of `source` under `dest`.
///
/// Never fails as a whole: a member that cannot be written is recorded in the
/// report and the rest are still extracted, because a user who asked for 900
/// files and can have 897 of them wants the 897.
pub fn extract(source: &ArchiveSource, members: &[&ArchiveEntry], dest: &Path) -> ExtractReport {
    match &source.members {
        Members::Zip(_) => extract_zip(source, members, dest),
        Members::Tar(map) => extract_tar(&source.bytes, map, members, dest),
    }
}

/// [`extract`] for a TAR: a file's bytes copied out a chunk at a time, a
/// directory made, a hard link written as a copy of its target. A symbolic
/// link is not made -- see [`SkipReason::SymbolicLink`] -- and neither is a
/// device or a pipe.
fn extract_tar(
    bytes: &ArchiveBytes,
    map: &HashMap<u64, tararchive::Entry>,
    members: &[&ArchiveEntry],
    dest: &Path,
) -> ExtractReport {
    let mut report = ExtractReport::default();
    for entry in members {
        let Some(member) = map.get(&entry.id) else {
            report
                .skipped
                .push((entry.path.clone(), SkipReason::Escapes));
            continue;
        };
        let target = match safe_destination(dest, &member.name) {
            Ok(p) => p,
            Err(why) => {
                report.skipped.push((entry.path.clone(), why));
                continue;
            }
        };
        // What to copy: the member's own bytes, or for a hard link its
        // target's -- the first file of that name, which a TAR writes before
        // any link to it.
        let from = match member.kind {
            tararchive::Kind::Directory => {
                match fs::create_dir_all(&target) {
                    Ok(()) => report.directories = report.directories.saturating_add(1),
                    Err(e) => report.skipped.push((entry.path.clone(), SkipReason::Io(e))),
                }
                continue;
            }
            tararchive::Kind::File | tararchive::Kind::Other(_) => member,
            tararchive::Kind::HardLink => {
                let original = member.link.as_ref().and_then(|link| {
                    map.values()
                        .find(|m| m.kind == tararchive::Kind::File && same_name(&m.name, link))
                });
                match original {
                    Some(original) => original,
                    None => {
                        report
                            .skipped
                            .push((entry.path.clone(), SkipReason::LinkTargetMissing));
                        continue;
                    }
                }
            }
            tararchive::Kind::Symlink => {
                report
                    .skipped
                    .push((entry.path.clone(), SkipReason::SymbolicLink));
                continue;
            }
            tararchive::Kind::CharDevice
            | tararchive::Kind::BlockDevice
            | tararchive::Kind::Fifo => {
                report
                    .skipped
                    .push((entry.path.clone(), SkipReason::NotAFile));
                continue;
            }
        };
        if let Some(parent) = target.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report.skipped.push((entry.path.clone(), SkipReason::Io(e)));
            continue;
        }
        let written = fs::File::create(&target)
            .map_err(SkipReason::Io)
            .and_then(|mut file| copy_bytes(bytes, from.offset, from.size, &mut file));
        match written {
            Ok(()) => {
                report.written = report.written.saturating_add(1);
                report.bytes = report.bytes.saturating_add(from.size);
            }
            Err(why) => {
                // A file cut off part-way is not left looking like the member.
                let _ = fs::remove_file(&target);
                report.skipped.push((entry.path.clone(), why));
            }
        }
    }
    report
}

/// [`extract`] for a ZIP.
fn extract_zip(source: &ArchiveSource, members: &[&ArchiveEntry], dest: &Path) -> ExtractReport {
    let mut report = ExtractReport::default();
    for entry in members {
        let Some(member) = source.member(entry.id) else {
            // The model and its source disagree, which can only happen if an
            // entry was added to the model by something other than the parser.
            report
                .skipped
                .push((entry.path.clone(), SkipReason::Escapes));
            continue;
        };
        let target = match safe_destination(dest, &member.name) {
            Ok(p) => p,
            Err(why) => {
                report.skipped.push((entry.path.clone(), why));
                continue;
            }
        };
        if member.is_dir {
            match fs::create_dir_all(&target) {
                Ok(()) => report.directories = report.directories.saturating_add(1),
                Err(e) => report.skipped.push((entry.path.clone(), SkipReason::Io(e))),
            }
            continue;
        }
        // Checked before the inflater is asked, not after it fails. The
        // difference is what the user is told: refused here, the report names a
        // missing password; left to `extract_entry`, the ciphertext inflates to
        // rubbish and the size/CRC check calls an intact archive corrupt. It is
        // also the only place the Encrypted column and the error message can be
        // made to agree, since both now read the same bit.
        if member.is_encrypted() {
            report
                .skipped
                .push((entry.path.clone(), SkipReason::Encrypted));
            continue;
        }
        let data = match ziparchive::extract_entry_at(&mut source.reader(), member) {
            Ok(d) => d,
            Err(e) => {
                report
                    .skipped
                    .push((entry.path.clone(), SkipReason::from(e)));
                continue;
            }
        };
        // A member called `a/b/c.txt` in an archive with no `a/` member of its
        // own still has to land in a directory that exists. `create_dir_all` on
        // the parent is what makes an archive written by a tool that omits
        // directory entries extract at all.
        if let Some(parent) = target.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report.skipped.push((entry.path.clone(), SkipReason::Io(e)));
            continue;
        }
        match fs::write(&target, &data) {
            Ok(()) => {
                report.written = report.written.saturating_add(1);
                report.bytes = report.bytes.saturating_add(data.len() as u64);
            }
            Err(e) => report.skipped.push((entry.path.clone(), SkipReason::Io(e))),
        }
    }
    report
}

/// Decompress every member and check it against what the archive declared.
///
/// This is what the Test button does, and it is a real test: `extract_entry`
/// caps the inflater at the entry's declared size, then requires the result to
/// be exactly that size and to match the stored CRC-32. Nothing is written to
/// disk.
///
/// Directory members are not tested — they have no data — and are not counted,
/// so a pass rate is a pass rate over the things that could have failed.
#[must_use]
pub fn verify(model: &ArchiveModel) -> ArchiveTestResults {
    let files: Vec<&ArchiveEntry> = model.entries.iter().filter(|e| !e.is_dir).collect();
    let mut results = ArchiveTestResults::new(files.len());
    results.damage.clone_from(&model.damage);
    let Some(source) = model.source.as_ref() else {
        return results;
    };
    if let Members::Tar(map) = &source.members {
        verify_tar(&source.bytes, map, &files, &mut results);
        return results;
    }
    for entry in files {
        let result = match source.member(entry.id) {
            None => TestResult::Corrupted(String::from("no record of this entry in the archive")),
            // An encrypted member is not tested rather than tested and failed.
            // It counts against the pass rate either way — the Test button
            // cannot vouch for data it cannot read — but "Decrypt Failed" sends
            // the user to look for a password, which is the thing that would
            // actually help, whereas "Corrupted" sends them to look for another
            // copy of an archive that is not damaged.
            Some(member) if member.is_encrypted() => TestResult::DecryptionFailed,
            Some(member) => match ziparchive::extract_entry_at(&mut source.reader(), member) {
                Ok(_) => TestResult::Ok,
                // The parser checks the declared size first and the CRC second
                // but reports one error for both, so this message names both
                // rather than picking one and being wrong half the time. Asked
                // for a finer error in
                // `requests/c-a-ziparchive-drops-the-one-field-a-date-column-needs.md`.
                Err(ziparchive::RangedError::Zip(ziparchive::Error::CorruptedData)) => {
                    TestResult::Corrupted(String::from(
                        "its contents do not match the size or checksum the archive declared",
                    ))
                }
                Err(ziparchive::RangedError::Zip(ziparchive::Error::UnsupportedMethod)) => {
                    TestResult::Corrupted(format!("{} is not a codec this build has", entry.method))
                }
                // Says nothing about the archive -- see `TestResult::Unreadable`.
                Err(ziparchive::RangedError::Read(e)) => TestResult::Unreadable(format!("{e}")),
            },
        };
        results.record(&entry.path, result);
    }
    results
}

/// [`verify`] for a TAR: every member's bytes read back from the archive.
/// A TAR keeps no checksum of a member's contents -- only of its header,
/// which the listing already checked -- so what can fail is the archive
/// ending inside a member, or the disk not answering.
fn verify_tar(
    bytes: &ArchiveBytes,
    map: &HashMap<u64, tararchive::Entry>,
    files: &[&ArchiveEntry],
    results: &mut ArchiveTestResults,
) {
    for entry in files {
        let result = match map.get(&entry.id) {
            None => TestResult::Corrupted(String::from("no record of this entry in the archive")),
            Some(member) if carries_bytes(member.kind) => {
                match copy_bytes(bytes, member.offset, member.size, &mut io::sink()) {
                    Ok(()) => TestResult::Ok,
                    Err(SkipReason::Unreadable(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                        TestResult::Corrupted(String::from("the archive ends inside it"))
                    }
                    Err(why) => TestResult::Unreadable(why.to_string()),
                }
            }
            Some(member) if member.kind == tararchive::Kind::HardLink => {
                let target = member.link.as_ref().and_then(|link| {
                    map.values()
                        .find(|m| m.kind == tararchive::Kind::File && same_name(&m.name, link))
                });
                match target {
                    Some(_) => TestResult::Ok,
                    None => TestResult::Corrupted(String::from(
                        "it links to a member the archive does not hold",
                    )),
                }
            }
            // A link or a device: its header is all there is, and the listing
            // checked it.
            Some(_) => TestResult::Ok,
        };
        results.record(&entry.path, result);
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// A `fs::File` as a `ziparchive` sink.
///
/// The writer hands this one member at a time, so the new archive is never
/// held whole. Before this, `save` built every member's plaintext into a
/// `Vec<ZipWriteEntry>` and then asked `ziparchive::create` for the finished
/// bytes, so a rewrite held the old archive, every member's plaintext and the
/// entire new archive at once -- "old + plaintext + new", which
/// `requests/c-a-ziparchive-wants-a-ranged-reader-and-a-streaming-writer.md`
/// called the part nobody anticipated.
struct FileSink<'a>(&'a mut fs::File);

impl ziparchive::WriteStream for FileSink<'_> {
    type Error = io::Error;

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        use io::Write as _;
        self.0.write_all(buf)
    }
}

/// A file the user has asked to put into the archive, already read.
///
/// Read *before* the rewrite starts rather than during it, because a rewrite
/// that discovers halfway through that one of its inputs has gone away has
/// already decided the shape of the new archive. Everything that can fail on
/// the way in fails before a single byte of the old archive is at risk.
#[derive(Debug)]
pub struct PendingAdd {
    /// The name the member will carry, as bytes.
    pub name: Vec<u8>,
    /// The file's contents.
    pub data: Vec<u8>,
    /// The file's mtime as the DOS pair, or `0` if it had none this build
    /// could express.
    pub dos_datetime: u32,
    /// The file's mtime in seconds since 1970 -- what a TAR records -- or `0`
    /// if it had none.
    pub mtime: i64,
}

/// Why an archive could not be rewritten.
///
/// Every variant fails the operation *whole*. That is the point of the type:
/// a rewrite drops every member it cannot reproduce, so a partial success here
/// is not a partial success at all — it is silent data loss inside a file the
/// user still believes contains what it used to.
#[derive(Debug)]
pub enum SaveError {
    /// The model was not read from a file, so there are no bytes to rebuild
    /// its existing members from.
    NoSource,
    /// A member of the open archive could not be reproduced.
    ///
    /// Carries the member's displayable name and what went wrong. This is the
    /// variant that stops the destructive case: an archive holding one
    /// encrypted member cannot be rewritten by a build with no decryption
    /// without losing that member, so it is not rewritten at all.
    CannotReproduce { name: String, why: SkipReason },
    /// The rewrite would allocate more than [`MAX_SAVE_BYTES`].
    ///
    /// Refused up front, from the sizes in the central directory, rather than
    /// discovered by being killed part way through: an archive rewrite that
    /// dies mid-flight is the one case where this program could lose a file the
    /// user already had.
    WouldExhaustMemory { projected: u64, limit: u64 },
    /// A row in the list names a member the source archive does not contain.
    ///
    /// Separate from [`Self::CannotReproduce`] because it is not a fact about
    /// the archive — it means the list and the bytes it was built from have
    /// come apart, which is this program's bug and not the user's file's. The
    /// only correct response is still to refuse: the rewrite would silently
    /// drop the row, and a bug that eats a member is worse than one that
    /// reports itself.
    UnknownMember { name: String },
    /// The new archive could not be written, or could not replace the old one.
    Io { path: PathBuf, source: io::Error },
    /// A format this build cannot write.
    Unwritable { format: ArchiveFormat },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSource => f.write_str("this archive was not read from a file"),
            Self::WouldExhaustMemory { projected, limit } => write!(
                f,
                "rewriting this archive needs about {} of memory and this program will use up to {}; nothing was changed",
                guitk::bytes::iec(*projected),
                guitk::bytes::iec(*limit)
            ),
            Self::CannotReproduce { name, why } => write!(
                f,
                "{name} cannot be rewritten — {why}; nothing was changed, \
                 because saving would have dropped it"
            ),
            Self::UnknownMember { name } => write!(
                f,
                "the list has come apart from the file it was read from \
                 ({name} is listed but not in it); nothing was changed. \
                 Re-open the archive"
            ),
            Self::Io { path, source } => write!(f, "cannot write {}: {source}", path.display()),
            Self::Unwritable { format } => write!(
                f,
                "this build writes ZIP, TAR and TAR.GZ, and not {}",
                format.display_name()
            ),
        }
    }
}

/// What a rewrite did.
#[derive(Debug, Default)]
pub struct SaveReport {
    /// Members in the archive afterwards.
    pub members: usize,
    /// Members that came from the files just added.
    pub added: usize,
    /// Members an added file displaced because it had the same name.
    pub replaced: usize,
    /// The size of the archive on disk afterwards.
    pub bytes: u64,
}

impl SaveReport {
    /// A one-line summary for the status bar.
    #[must_use]
    pub fn summary(&self, path: &Path) -> String {
        // Both counts, not just the total, because "added 3" and "replaced 1"
        // are the two things the user cannot see by looking at the list: the
        // list after a save that replaced a member looks exactly like the list
        // after one that did not, and replacing is the case where something
        // that was in the archive is now gone.
        let added = match (self.added, self.replaced) {
            (0, _) => String::new(),
            (n, 0) => format!(", {n} added"),
            (n, r) => format!(", {n} added (replacing {r})"),
        };
        format!(
            "Saved {} — {} member{}{added}, {}",
            path.display(),
            self.members,
            if self.members == 1 { "" } else { "s" },
            guitk::bytes::iec(self.bytes),
        )
    }
}

/// Read `path` so it can be added to an archive.
///
/// The member name is the file's own name, not its path: dropping a file into
/// an archive puts it at the archive's root, which is what every other archive
/// manager does and what the file list on screen will then show.
///
/// # Errors
///
/// [`ArchiveError::Io`] if the file will not read, and [`ArchiveError::TooLarge`]
/// for one bigger than this program is willing to hold — the same ceiling
/// reading uses, since the rewrite holds the whole new archive in memory too.
pub fn read_for_add(path: &Path) -> Result<PendingAdd, ArchiveError> {
    let meta = fs::metadata(path).map_err(|source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if meta.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge { bytes: meta.len() });
    }
    let data = fs::read(path).map_err(|source| ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // Bytes, not text, and taken from the OS string rather than a lossy
    // rendering of it: this name is about to be written into a file, and the
    // rule the module doc states in the other direction holds in this one too.
    // `to_string_lossy` would turn a name the platform accepts but UTF-8 does
    // not into U+FFFD, so the member would go in under a name that can never
    // be extracted back onto the name it came from — the same silent
    // corruption `explorer`'s `fileops.rs` documents at its own encoder.
    let name = path
        .file_name()
        .map_or_else(Vec::new, |n| n.as_encoded_bytes().to_vec());
    Ok(PendingAdd {
        name,
        data,
        dos_datetime: dos_datetime_of(&meta),
        mtime: unix_mtime_of(&meta),
    })
}

/// A file's mtime in seconds since 1970, or `0` when it has none this build
/// can read.
fn unix_mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// A file's mtime as the DOS pair, or `0` when there is not one this build can
/// express.
///
/// `0` rather than the DOS minimum for the reason design-decisions.md §618
/// gives: 1980-01-01 is a date, and "no time was recorded" is not one. The
/// encoder already returns `0` for anything outside the DOS window, so a file
/// dated 1970 or 2110 lands there rather than being clamped to a wrong year.
fn dos_datetime_of(meta: &fs::Metadata) -> u32 {
    let Ok(modified) = meta.modified() else {
        return 0;
    };
    let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return 0;
    };
    let Ok(secs) = i64::try_from(since_epoch.as_secs()) else {
        return 0;
    };
    guitk::tzrules::dos_datetime_from_unix(secs)
}

/// Rebuild `model`'s archive with `adding` folded in, and replace the file.
///
/// The model's entry list is the archive's contents afterwards, in its own
/// order, plus the added files. Anything the user removed from the list is
/// therefore removed from the file — which is what makes Delete mean something
/// — and anything they added is written with its real mtime.
///
/// **Nothing is written until every member has been reproduced.** An existing
/// member's bytes come back out of the old archive through the same
/// `extract_entry` the Test button uses, so it is checked against its declared
/// size and CRC on the way through; if any member will not come back, the whole
/// save is refused and the file on disk is untouched. The alternative — write
/// what can be written — turns one unreadable member into a permanently lost
/// one.
///
/// The replacement itself is a write to a neighbouring temporary file followed
/// by a rename, so a crash or a full disk halfway through leaves the original
/// archive intact rather than truncated.
///
/// # Errors
///
/// [`SaveError`], and in every case the archive on disk is exactly as it was.
/// What a rewrite of `model` with `adding` will hold in memory at its peak.
///
/// Computed from the central directory, which carries both the compressed and
/// the uncompressed size of every member, so this is an arithmetic projection
/// rather than a guess. The four terms are the four things alive at once:
///
/// | term | why it is held |
/// |---|---|
/// | the bytes of every added file | already in `PendingAdd::data` by the time we are called |
/// | one reproduced member | the buffer `entry_data_at` fills, dropped before the next |
///
/// **Three of the four costs this used to sum are gone**, and the table above
/// is what is left rather than a tightened estimate of the same thing. The
/// archive is no longer read whole at open (a handle, since `ArchiveBytes`);
/// the plaintext of every reproduced member is no longer built, because a
/// member is copied across compressed and never inflated; and the new archive
/// is no longer assembled in a buffer, because it is written to the file as it
/// is produced. What remains is what the caller already holds plus the largest
/// single member -- see `TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY`.
///
/// The largest member rather than their sum, because the buffers do not
/// coexist: each is dropped at the end of its iteration.
///
/// Directory members contribute nothing: they have no data, which is why they
/// are skipped in the rewrite loop too.
fn projected_save_bytes(
    source: &ArchiveSource,
    model: &ArchiveModel,
    adding: &[PendingAdd],
) -> u64 {
    // Everything the caller is already holding. `read_for_add` reads each file
    // whole before `save` is called, so this is spent whatever happens here.
    let mut total: u64 = 0;
    for add in adding {
        total = total.saturating_add(add.data.len() as u64);
    }

    // Plus the largest single member, which is all the rewrite adds: members
    // are copied one at a time and each buffer is dropped before the next is
    // read. The *compressed* size, because a copied member is never inflated.
    let mut largest = 0_u64;
    for entry in &model.entries {
        let Some(member) = source.member(entry.id) else {
            continue;
        };
        if member.is_dir || adding.iter().any(|add| add.name == member.name) {
            continue;
        }
        largest = largest.max(member.compressed_size);
    }
    total.saturating_add(largest)
}

pub fn save(model: &ArchiveModel, adding: Vec<PendingAdd>) -> Result<SaveReport, SaveError> {
    save_within(model, adding, MAX_SAVE_BYTES)
}

/// [`save`], with the memory budget as a parameter.
///
/// The budget is a parameter for one reason: so a test can reach the refusal on
/// the *real* path. Tripping [`MAX_SAVE_BYTES`] honestly would mean building an
/// archive claiming more than 1.5 GiB of plaintext, which costs 1.5 GiB to
/// write -- so the alternative was to test the projection arithmetic on its own
/// and leave nothing at all covering "and `save` acts on it", which is the
/// shape of bug this crate keeps finding in others (an argument the program
/// cannot produce is an argument no test result means anything about).
///
/// `save` is the only non-test caller and passes the constant.
fn save_within(
    model: &ArchiveModel,
    adding: Vec<PendingAdd>,
    limit: u64,
) -> Result<SaveReport, SaveError> {
    let source = model.source.as_ref().ok_or(SaveError::NoSource)?;
    if let Members::Tar(map) = &source.members {
        return save_tar(model, source, map, adding, limit);
    }

    // Before anything is allocated, and before the old archive is touched.
    let projected = projected_save_bytes(source, model, &adding);
    if projected > limit {
        return Err(SaveError::WouldExhaustMemory { projected, limit });
    }

    // The names being added, so an existing member with the same name can be
    // dropped rather than written twice. A ZIP with two members of one name is
    // legal to write and ambiguous to read: which one a tool extracts is its
    // own business, so writing one is a way of not deciding.
    let added = adding.len();

    // Streamed, not assembled. Each member is written as it is read, so what
    // is held at once is one member rather than the whole new archive plus
    // every member's plaintext.
    let ((written, replaced), total) = replace_file_with(&model.path, move |file| {
        let mut zip = ziparchive::ZipWriter::new(FileSink(file));
        let mut written = 0_usize;
        let mut replaced = 0_usize;
        // Writing to the temporary failed. The path named is the archive the
        // user asked to save, not the temporary, which is this module's
        // business and not theirs.
        let failed = |source: io::Error| SaveError::Io {
            path: model.path.clone(),
            source,
        };

        for entry in &model.entries {
            let Some(member) = source.member(entry.id) else {
                return Err(SaveError::UnknownMember {
                    name: entry.path.clone(),
                });
            };
            // A member being replaced is simply not written; the new one
            // arrives in the loop below. A ZIP with two members of one name is
            // legal to write and ambiguous to read, so writing one is a way of
            // not deciding.
            if adding.iter().any(|add| add.name == member.name) {
                replaced = replaced.saturating_add(1);
                continue;
            }
            if member.is_dir {
                zip.add_entry(&ziparchive::ZipWriteEntry {
                    name: member.name.clone(),
                    data: Vec::new(),
                    store_only: true,
                    dos_datetime: member.dos_datetime,
                })
                .map_err(failed)?;
                written = written.saturating_add(1);
                continue;
            }
            if member.is_encrypted() {
                return Err(SaveError::CannotReproduce {
                    name: entry.path.clone(),
                    why: SkipReason::Encrypted,
                });
            }
            // The member's *stored* bytes, handed straight to `copy_entry`:
            // the member crosses into the new archive without being inflated
            // and without being deflated again. It therefore keeps the method,
            // CRC and sizes it arrived with, which the previous version could
            // not -- rebuilding from plaintext meant re-compressing everything
            // and storing uncompressed whatever failed to shrink.
            let raw = ziparchive::entry_data_at(&mut source.reader(), member).map_err(|e| {
                SaveError::CannotReproduce {
                    name: entry.path.clone(),
                    why: SkipReason::from(e),
                }
            })?;
            zip.copy_entry(member, &raw).map_err(failed)?;
            written = written.saturating_add(1);
        }

        for add in adding {
            zip.add_entry(&ziparchive::ZipWriteEntry {
                name: add.name,
                data: add.data,
                store_only: false,
                dos_datetime: add.dos_datetime,
            })
            .map_err(failed)?;
            written = written.saturating_add(1);
        }

        zip.finish().map_err(failed)?;
        Ok((written, replaced))
    })?;

    Ok(SaveReport {
        members: written,
        added,
        replaced,
        bytes: total,
    })
}

/// [`save`] for a TAR or a TAR.GZ: the model's members, in its order, less
/// those an added file of the same name displaces, then the added files.
///
/// A TAR is written to the file as it is produced, a member at a time, its
/// bytes copied across a chunk at a time. A TAR.GZ is one gzip stream over
/// the whole archive, so the archive is built in memory and compressed --
/// which is what its projection counts, and refuses past `limit`.
fn save_tar(
    model: &ArchiveModel,
    source: &ArchiveSource,
    map: &HashMap<u64, tararchive::Entry>,
    adding: Vec<PendingAdd>,
    limit: u64,
) -> Result<SaveReport, SaveError> {
    let gzipped = model.format == ArchiveFormat::TarGz;
    let added_bytes: u64 = adding.iter().map(|a| a.data.len() as u64).sum();
    let projected = if gzipped {
        // The archive being built -- every member padded, two headers each
        // at most -- and its compressed copy beside it.
        let members: u64 = model
            .entries
            .iter()
            .filter_map(|e| map.get(&e.id))
            .map(|m| {
                m.size
                    .div_ceil(512)
                    .saturating_mul(512)
                    .saturating_add(1024)
            })
            .sum();
        let tar = members
            .saturating_add(
                added_bytes.saturating_add(1024_u64.saturating_mul(adding.len() as u64)),
            )
            .saturating_add(1024);
        tar.saturating_mul(2).saturating_add(added_bytes)
    } else {
        added_bytes.saturating_add(COPY_CHUNK as u64)
    };
    if projected > limit {
        return Err(SaveError::WouldExhaustMemory { projected, limit });
    }
    let added = adding.len();
    let write = |out: &mut dyn io::Write| -> Result<(usize, usize), SaveError> {
        let failed = |source: io::Error| SaveError::Io {
            path: model.path.clone(),
            source,
        };
        let (mut written, mut replaced) = (0_usize, 0_usize);
        for entry in &model.entries {
            let Some(member) = map.get(&entry.id) else {
                return Err(SaveError::UnknownMember {
                    name: entry.path.clone(),
                });
            };
            if adding.iter().any(|add| same_name(&add.name, &member.name)) {
                replaced = replaced.saturating_add(1);
                continue;
            }
            let size = if carries_bytes(member.kind) {
                member.size
            } else {
                0
            };
            tararchive::write_header(
                &mut *out,
                &tararchive::NewMember {
                    name: &member.name,
                    kind: member.kind,
                    mode: member.mode,
                    mtime: member.mtime,
                    size,
                    link: member.link.as_deref(),
                },
            )
            .map_err(failed)?;
            copy_bytes(&source.bytes, member.offset, size, &mut *out).map_err(|why| match why {
                SkipReason::Io(e) => failed(e),
                why => SaveError::CannotReproduce {
                    name: entry.path.clone(),
                    why,
                },
            })?;
            tararchive::write_padding(&mut *out, size).map_err(failed)?;
            written = written.saturating_add(1);
        }
        for add in &adding {
            let size = add.data.len() as u64;
            tararchive::write_header(
                &mut *out,
                &tararchive::NewMember {
                    name: &add.name,
                    kind: tararchive::Kind::File,
                    mode: 0o644,
                    mtime: add.mtime,
                    size,
                    link: None,
                },
            )
            .map_err(failed)?;
            out.write_all(&add.data).map_err(failed)?;
            tararchive::write_padding(&mut *out, size).map_err(failed)?;
            written = written.saturating_add(1);
        }
        tararchive::write_end(&mut *out).map_err(failed)?;
        Ok((written, replaced))
    };
    let ((written, replaced), bytes) = if gzipped {
        let mut tar = Vec::new();
        let counts = write(&mut tar)?;
        let gz = deflate::gzip(&tar);
        drop(tar);
        replace_file(&model.path, &gz)?;
        (counts, gz.len() as u64)
    } else {
        replace_file_with(&model.path, |file| write(file))?
    };
    Ok(SaveReport {
        members: written,
        added,
        replaced,
        bytes,
    })
}

/// Write an empty archive at `path`, in the format its name says -- a ZIP
/// when it names none. An empty TAR is its two closing blocks; an empty
/// TAR.GZ, those gzipped.
///
/// It wrote an empty ZIP whatever the name, so a new `backup.tar` was a ZIP
/// under a TAR's name, which nothing then opened as either.
///
/// # Errors
///
/// [`SaveError::Io`] if the file cannot be written, and
/// [`SaveError::Unwritable`] for a format this build does not write.
pub fn create_empty(path: &Path) -> Result<(), SaveError> {
    let empty_tar = [0_u8; 1024];
    let bytes = match ArchiveFormat::from_path(path) {
        None | Some(ArchiveFormat::Zip) => ziparchive::create(&[]),
        Some(ArchiveFormat::Tar) => empty_tar.to_vec(),
        Some(ArchiveFormat::TarGz) => deflate::gzip(&empty_tar),
        Some(format @ (ArchiveFormat::TarBz2 | ArchiveFormat::SevenZip)) => {
            return Err(SaveError::Unwritable { format });
        }
    };
    replace_file(path, &bytes)
}

/// Put `bytes` at `path` without ever leaving `path` half-written.
///
/// Write beside, then rename over. `fs::write` straight onto the target would
/// truncate the user's archive as its first action, so a disk that fills up or
/// a process that dies during the write destroys the original and leaves a
/// fragment wearing its name. Rename is the only step that touches the target,
/// and on both Windows and Unix it replaces atomically.
fn replace_file_with<T>(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> Result<T, SaveError>,
) -> Result<(T, u64), SaveError> {
    // The same temporary-then-rename dance as `replace_file`, and for the same
    // reasons -- beside the target so the rename stays within one filesystem,
    // and the temporary removed on every failing path. What differs is that
    // the caller writes *into* the file rather than handing over finished
    // bytes, which is what lets an archive be rewritten without existing in
    // memory first.
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", std::process::id()));
    let temp = path.with_file_name(name);

    let outcome = (|| {
        let mut file = fs::File::create(&temp).map_err(|source| SaveError::Io {
            path: temp.clone(),
            source,
        })?;
        let value = write(&mut file)?;
        // Asked of the file rather than counted while writing: the number
        // reported is then the size of the thing on disk, not a tally that
        // could disagree with it.
        let len = file
            .metadata()
            .map_err(|source| SaveError::Io {
                path: temp.clone(),
                source,
            })?
            .len();
        Ok((value, len))
    })();

    let (value, len) = match outcome {
        Ok(v) => v,
        Err(e) => {
            // A half-written archive beside the user's real one is litter that
            // looks like a backup.
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
    };

    if let Err(source) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(SaveError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok((value, len))
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    // Beside the target, so the rename stays within one filesystem — across a
    // mount boundary it would degrade into copy-then-delete and lose the
    // atomicity that is the whole reason for the dance.
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", std::process::id()));
    let temp = path.with_file_name(name);
    fs::write(&temp, bytes).map_err(|source| SaveError::Io {
        path: temp.clone(),
        source,
    })?;
    if let Err(source) = fs::rename(&temp, path) {
        // The temporary is this function's litter, and leaving it next to the
        // user's archive would be a second failure reported as none.
        let _ = fs::remove_file(&temp);
        return Err(SaveError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use ziparchive::ZipWriteEntry;

    /// A member, compressed if it helps.
    fn member(name: &str, data: &[u8]) -> ZipWriteEntry {
        ZipWriteEntry {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            store_only: false,
            // Left unrecorded on purpose: this helper backs
            // `an_archive_we_wrote_ourselves_reports_no_time_rather_than_1980`,
            // whose whole point is that an absent time reads as absent.
            dos_datetime: 0,
        }
    }

    /// A scratch directory nothing else is using, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "archivemanager-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the scratch directory");
        dir
    }

    /// A one-member archive whose member claims to be encrypted.
    ///
    /// The bit is patched into the bytes rather than asked for, because
    /// `ziparchive::create` cannot *write* an encrypted member — nothing in
    /// SlateOS can. The only honest fixture for reading one is therefore the
    /// bytes a real archiver would have produced, which for the purpose of every
    /// assertion below is an ordinary archive with general-purpose bit 0 set.
    ///
    /// Only the central header is patched, which is exactly where `parse` reads
    /// the field. That leaves the local header disagreeing with it — a real
    /// encrypted archive would set both — but the member's *data* is real
    /// deflate either way, which is the point: it makes the test able to tell
    /// "refused because the bit is set" from "failed because ciphertext does not
    /// inflate". Only the first is a correct refusal, and only this fixture can
    /// distinguish them.
    fn encrypted_archive(name: &str, data: &[u8]) -> Vec<u8> {
        let mut bytes = ziparchive::create(&[member(name, data)]);
        let central = bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4B, 0x01, 0x02])
            .expect("a central directory header");
        bytes[central + 8..central + 10].copy_from_slice(&1u16.to_le_bytes());
        bytes
    }

    /// `bytes` with general-purpose bit 0 set on the member called `name`.
    ///
    /// The same trick as [`encrypted_archive`] and for the same reason — nothing
    /// in SlateOS can *write* an encrypted member — but aimed at one member of
    /// several rather than at the only one, so a test can assert that an
    /// archive is refused *whole* rather than merely that an archive of one
    /// unreadable member produces nothing.
    fn with_encrypted_bit(bytes: &[u8], name: &[u8]) -> Vec<u8> {
        // Central-directory header: signature at 0, general-purpose flags at 8,
        // name length at 28, extra at 30, comment at 32, name text at 46.
        let mut out = bytes.to_vec();
        let mut at = 0;
        while at + 46 <= out.len() {
            if out[at..at + 4] != [0x50, 0x4B, 0x01, 0x02] {
                at += 1;
                continue;
            }
            let n = u16::from_le_bytes([out[at + 28], out[at + 29]]) as usize;
            if out.get(at + 46..at + 46 + n) == Some(name) {
                out[at + 8..at + 10].copy_from_slice(&1u16.to_le_bytes());
                return out;
            }
            at += 1;
        }
        panic!("no central header for {}", String::from_utf8_lossy(name));
    }

    /// The MS-DOS pair for a wall-clock date and time, as a ZIP stores it.
    ///
    /// The encoder is written out here rather than reusing anything from
    /// production, so a test asserts the decode against an independently
    /// spelled-out format rather than against itself.
    fn dos(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u32 {
        let date = ((year - 1980) << 9) | (month << 5) | day;
        let time = (hour << 11) | (minute << 5) | (second / 2);
        (date << 16) | time
    }

    #[test]
    fn a_dos_pair_decodes_to_the_instant_it_names() {
        // 1980-01-01 00:00:00 UTC is 315532800, which is worth pinning as a
        // literal: it is the DOS *minimum* date, and getting the epoch offset
        // wrong would shift every date in the program by a decade while still
        // looking plausible.
        assert_eq!(dos_datetime_to_unix(dos(1980, 1, 1, 0, 0, 0)), 315_532_800);

        // A date far from the epoch, to catch a leap-year rule that is only
        // right near 1980. 2026-08-26 14:30:52 UTC.
        assert_eq!(
            dos_datetime_to_unix(dos(2026, 8, 26, 14, 30, 52)),
            1_787_754_652
        );

        // And it renders, rather than being a number only this test sees.
        assert_ne!(
            ArchiveEntry::format_date(dos_datetime_to_unix(dos(2026, 8, 26, 14, 30, 52))),
            "-",
            "a decoded time must reach the Date column as a date"
        );
    }

    #[test]
    fn a_pair_that_names_no_instant_stays_unknown() {
        // Zero is the case that actually occurs: `ziparchive::create` writes it,
        // deliberately, so that archives SlateOS produces say "no time recorded"
        // instead of claiming 1980-01-01.
        assert_eq!(dos_datetime_to_unix(0), 0);
        assert_eq!(
            ArchiveEntry::format_date(0),
            "-",
            "zero must reach the Date column as `-`"
        );

        // Month and day are 1-based in the format, so a zero in either is a
        // malformed pair rather than a date near the start of the year. An
        // out-of-range field is refused rather than clamped: an invented date
        // is worse than an admitted gap.
        for bad in [
            dos(2026, 0, 26, 12, 0, 0),
            dos(2026, 8, 0, 12, 0, 0),
            dos(2026, 13, 1, 12, 0, 0),
            dos(2026, 8, 26, 24, 0, 0),
            dos(2026, 8, 26, 12, 60, 0),
        ] {
            assert_eq!(
                dos_datetime_to_unix(bad),
                0,
                "a malformed pair {bad:#010x} must be unknown, not guessed"
            );
        }
    }

    /// Regression: the day check used to be a constant `1..=31`, which is the
    /// widest month rather than *this* month, so February 30 passed it and was
    /// then normalised by `days_from_civil` into a perfectly ordinary March 2.
    /// The user saw a plausible date in roughly the right region with nothing
    /// to suggest the archive had said something impossible — the one outcome
    /// this function's contract rules out. Reported by lane A in
    /// `requests/a-c-the-dos-decoder-exists-now-and-yours-invents-a-date-in-february.md`.
    #[test]
    fn a_day_that_month_does_not_have_is_unknown_and_not_the_next_month() {
        for (bad, would_have_been) in [
            (dos(2026, 2, 30, 12, 0, 0), "2026-03-02"),
            // 2026 is not a leap year, so the 29th is as impossible as the 30th
            // — and this is the case a rule that only knows "February is short"
            // would still get wrong.
            (dos(2026, 2, 29, 12, 0, 0), "2026-03-01"),
            (dos(2026, 4, 31, 12, 0, 0), "2026-05-01"),
            (dos(2026, 6, 31, 12, 0, 0), "2026-07-01"),
            (dos(2026, 9, 31, 12, 0, 0), "2026-10-01"),
            (dos(2026, 11, 31, 12, 0, 0), "2026-12-01"),
        ] {
            let secs = dos_datetime_to_unix(bad);
            assert_eq!(
                secs, 0,
                "{bad:#010x} names no day, so it must be unknown rather than {would_have_been}"
            );
            assert_eq!(
                ArchiveEntry::format_date(secs),
                "-",
                "an impossible day must reach the Date column as `-`"
            );
        }
    }

    /// The other half of the same rule: a leap day is a real day, and refusing
    /// it would be the opposite bug — an archive that recorded a true time
    /// showing `-`. This is why the check has to consult the calendar rather
    /// than shorten February by a constant.
    #[test]
    fn the_twenty_ninth_of_february_in_a_leap_year_still_decodes() {
        // 2024-02-29 12:00:00 UTC.
        assert_eq!(
            dos_datetime_to_unix(dos(2024, 2, 29, 12, 0, 0)),
            1_709_208_000
        );
        // 2000 is the century that *is* a leap year, which a rule missing the
        // 400-year clause would refuse.
        assert_ne!(dos_datetime_to_unix(dos(2000, 2, 29, 0, 0, 0)), 0);
        // 2100 is the century that is not, which the same missing clause would
        // wrongly accept.
        assert_eq!(dos_datetime_to_unix(dos(2100, 2, 29, 0, 0, 0)), 0);
    }

    /// The rendering decision — an unanswerable pair becomes `-` — is the part
    /// that stayed in this app when the decoding moved to `tzrules`. That makes
    /// the mapping from `None` to `0` load-bearing, so it is asserted directly
    /// against the shared function rather than only through its consequences.
    #[test]
    fn the_shared_decoder_and_this_wrapper_agree_on_every_pair_they_are_given() {
        for pair in [
            0,
            dos(1980, 1, 1, 0, 0, 0),
            dos(2026, 8, 26, 14, 30, 52),
            dos(2026, 2, 30, 12, 0, 0),
            dos(2024, 2, 29, 12, 0, 0),
            dos(2026, 13, 1, 12, 0, 0),
            dos(2026, 8, 26, 24, 0, 0),
        ] {
            let expected = guitk::tzrules::unix_from_dos_datetime(pair)
                .and_then(|s| u64::try_from(s).ok())
                .unwrap_or(0);
            assert_eq!(
                dos_datetime_to_unix(pair),
                expected,
                "the wrapper must add nothing to {pair:#010x} but the sentinel"
            );
        }
    }

    #[test]
    fn an_odd_second_is_the_formats_loss_and_not_a_wrong_answer() {
        // DOS stores seconds halved, so 53 and 52 are the same stored value.
        // The decode must land on the representable one rather than rounding up
        // into a second the archive did not record.
        assert_eq!(
            dos_datetime_to_unix(dos(2026, 8, 26, 14, 30, 53)),
            dos_datetime_to_unix(dos(2026, 8, 26, 14, 30, 52)),
        );
    }

    #[test]
    fn an_archive_we_wrote_ourselves_reports_no_time_rather_than_1980() {
        // The end-to-end shape of the two rules above: our own writer records no
        // time, the parser hands the zero through, and the model says so.
        let bytes = ziparchive::create(&[member("a.txt", b"a")]);
        let model = parse_zip(Path::new("/tmp/dates.zip"), ArchiveBytes::Memory(bytes))
            .expect("a well-formed archive");
        let entry = &model.entries[0];
        assert_eq!(entry.modified, 0);
        assert_eq!(ArchiveEntry::format_date(entry.modified), "-");
    }

    #[test]
    fn a_real_archive_round_trips_through_the_model() {
        // The fixture is a ZIP this test wrote, read back by the parser the
        // program uses. Nothing about the entry list is typed in beside it, so
        // a change in the parser shows up here rather than in production.
        let bytes = ziparchive::create(&[
            member("src/main.rs", b"fn main() {}\n"),
            member("README.md", &b"read me, and me, and me, and me\n".repeat(8)),
        ]);
        let model = parse_zip(Path::new("/tmp/fixture.zip"), ArchiveBytes::Memory(bytes))
            .expect("a well-formed archive");

        assert_eq!(model.format, ArchiveFormat::Zip);
        assert_eq!(model.file_count, 2);
        let main = model
            .entries
            .iter()
            .find(|e| e.path == "src/main.rs")
            .expect("the member is in the model");
        assert_eq!(
            main.name, "main.rs",
            "the display name is the last component"
        );
        assert_eq!(main.depth, 1);
        assert_eq!(main.size, 13, "the size is the archive's, not a guess");
        assert!(!main.is_dir);

        let readme = model
            .entries
            .iter()
            .find(|e| e.path == "README.md")
            .expect("the member is in the model");
        assert_eq!(readme.method, "Deflate", "eight repeats compress");
        assert!(
            readme.compressed_size < readme.size,
            "packed {} is not smaller than {}",
            readme.compressed_size,
            readme.size
        );
    }

    #[test]
    fn every_member_carries_its_own_bytes_after_the_list_is_sorted() {
        // The reason the source is keyed by id. Sorting reorders `entries`, and
        // a parallel vector would then extract one member's contents under
        // another member's name.
        let bytes =
            ziparchive::create(&[member("z.txt", b"this is z"), member("a.txt", b"this is a")]);
        let mut model = parse_zip(Path::new("/tmp/order.zip"), ArchiveBytes::Memory(bytes))
            .expect("well-formed");
        model.sort_entries(&crate::SortState {
            column: crate::Column::Name,
            direction: crate::SortDirection::Ascending,
        });
        assert_eq!(model.entries[0].path, "a.txt", "the sort ran");

        let source = model
            .source
            .as_ref()
            .expect("parsed archives have a source");
        for entry in &model.entries {
            let m = source.member(entry.id).expect("every entry has its member");
            let data =
                ziparchive::extract_entry_at(&mut source.reader(), m).expect("it decompresses");
            assert_eq!(
                String::from_utf8_lossy(&data),
                format!("this is {}", entry.path.trim_end_matches(".txt")),
                "{} got another member's bytes",
                entry.path
            );
        }
    }

    #[test]
    fn verifying_a_good_archive_passes_every_file_and_counts_no_directories() {
        let bytes = ziparchive::create(&[
            ZipWriteEntry {
                name: b"docs/".to_vec(),
                data: Vec::new(),
                store_only: true,
                dos_datetime: 0,
            },
            member("docs/guide.md", b"# Guide\n"),
            member("LICENSE", b"do what you like\n"),
        ]);
        let model = parse_zip(Path::new("/tmp/good.zip"), ArchiveBytes::Memory(bytes))
            .expect("well-formed");
        let results = verify(&model);
        assert_eq!(
            results.total_entries, 2,
            "a directory has nothing to verify"
        );
        assert_eq!(results.tested, 2);
        assert!(results.all_passed(), "{:?}", results.results);
        assert!((results.pass_rate() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_flipped_byte_is_caught_by_the_test_button_and_named() {
        // A checksum nobody checks is decoration. Corrupt one member's
        // compressed bytes and the Test button must fail that member and only
        // that member.
        let good = ziparchive::create(&[
            member("intact.txt", b"this member is fine and stays fine"),
            member("broken.txt", b"this member is about to be damaged"),
        ]);
        let model = parse_zip(Path::new("/tmp/x.zip"), ArchiveBytes::Memory(good.clone()))
            .expect("well-formed");
        let broken_entry = model
            .entries
            .iter()
            .find(|e| e.path == "broken.txt")
            .expect("it is there");
        let member = model
            .source
            .as_ref()
            .expect("a source")
            .member(broken_entry.id)
            .expect("its record");
        // Find the member's compressed bytes in the file and flip a bit in the
        // middle of them, which damages the data without disturbing any header.
        let start = good
            .windows(4)
            .position(|w| w == b"this")
            .expect("a stored-or-deflated body to damage");
        let mut damaged = good;
        damaged[start + 2] ^= 0xFF;
        assert!(
            member.uncompressed_size > 0,
            "the fixture member must have data"
        );

        let model = parse_zip(Path::new("/tmp/x.zip"), ArchiveBytes::Memory(damaged))
            .expect("the directory is intact");
        let results = verify(&model);
        assert_eq!(results.tested, 2);
        assert_eq!(results.failed, 1, "{:?}", results.results);
        assert!(!results.all_passed());
        let (bad, why) = results
            .results
            .iter()
            .find(|(_, r)| **r != TestResult::Ok)
            .expect("one member failed");
        assert!(
            matches!(why, TestResult::Corrupted(msg) if msg.contains("checksum")),
            "{why:?} does not say what went wrong"
        );
        assert_eq!(
            bad, "intact.txt",
            "the damaged bytes are the first member's"
        );
    }

    #[test]
    fn extraction_writes_the_files_and_the_directories_they_need() {
        let dir = scratch("extract");
        let bytes = ziparchive::create(&[
            member("deep/down/here.txt", b"found me"),
            member("top.txt", b"at the top"),
        ]);
        let model =
            parse_zip(Path::new("/tmp/e.zip"), ArchiveBytes::Memory(bytes)).expect("well-formed");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(model.source.as_ref().expect("a source"), &all, &dir);

        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(report.written, 2);
        assert_eq!(report.bytes, 18);
        assert_eq!(
            fs::read_to_string(dir.join("deep").join("down").join("here.txt")).unwrap(),
            "found me",
            "a nested member makes its own directories"
        );
        assert_eq!(
            fs::read_to_string(dir.join("top.txt")).unwrap(),
            "at the top"
        );
        assert!(
            report.summary(&dir).starts_with("Extracted 2 files"),
            "{}",
            report.summary(&dir)
        );

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn an_encrypted_member_is_shown_as_encrypted() {
        let model = parse_zip(
            Path::new("/tmp/locked.zip"),
            ArchiveBytes::Memory(encrypted_archive(
                "secret.txt",
                b"pretend this is ciphertext",
            )),
        )
        .expect("well-formed");
        assert!(
            model.entries[0].encrypted,
            "the padlock column must read the bit, not a constant"
        );

        // And the ordinary case still reads false, from the same bit rather than
        // from the hardcoded answer it used to be.
        let plain = parse_zip(
            Path::new("/tmp/plain.zip"),
            ArchiveBytes::Memory(ziparchive::create(&[member(
                "open.txt",
                b"no password needed",
            )])),
        )
        .expect("well-formed");
        assert!(!plain.entries[0].encrypted);
    }

    #[test]
    fn an_encrypted_member_is_refused_by_name_rather_than_called_corrupt() {
        // The whole point of the change. Handed to the inflater, an encrypted
        // member fails the size or CRC check and gets reported as damaged — an
        // answer that is wrong about the archive and useless to the user, who
        // needs a password and is being told to find another copy.
        let dir = scratch("encrypted-extract");
        let model = parse_zip(
            Path::new("/tmp/locked.zip"),
            ArchiveBytes::Memory(encrypted_archive(
                "secret.txt",
                b"pretend this is ciphertext",
            )),
        )
        .expect("well-formed");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(model.source.as_ref().expect("a source"), &all, &dir);

        assert_eq!(report.written, 0, "nothing readable was in there");
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        let (name, why) = &report.skipped[0];
        assert_eq!(name, "secret.txt");
        assert!(
            matches!(why, SkipReason::Encrypted),
            "refused for the wrong reason: {why}"
        );
        assert!(
            why.to_string().contains("encrypted"),
            "the reason a user reads must name the password, not the checksum: {why}"
        );
        assert!(
            !dir.join("secret.txt").exists(),
            "a member we cannot decrypt must not leave ciphertext on disk under its own name"
        );

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn testing_an_encrypted_member_reports_a_password_not_damage() {
        // `verify` backs the Test button, and it is the other place the two
        // could disagree: a row showing a padlock and a result reading
        // "Corrupted" are two different explanations of one fact.
        let model = parse_zip(
            Path::new("/tmp/locked.zip"),
            ArchiveBytes::Memory(encrypted_archive(
                "secret.txt",
                b"pretend this is ciphertext",
            )),
        )
        .expect("well-formed");
        let results = verify(&model);

        assert!(matches!(
            results.results.get("secret.txt"),
            Some(TestResult::DecryptionFailed)
        ));
        assert_eq!(
            results.failed, 1,
            "a member we cannot read must not count as passing"
        );
        assert!(!results.all_passed());
    }

    #[test]
    fn a_member_that_climbs_out_of_the_destination_is_refused_and_says_so() {
        // Zip Slip. `../../` is a legal member name and this is the only thing
        // standing between it and the user's home directory.
        let dir = scratch("slip");
        let inside = dir.join("inside");
        fs::create_dir_all(&inside).expect("make the destination");

        let bytes = ziparchive::create(&[
            member("../escaped.txt", b"should not be written"),
            member("..\\also-escaped.txt", b"nor this"),
            member("keeps.txt", b"but this one is fine"),
        ]);
        let model = parse_zip(Path::new("/tmp/slip.zip"), ArchiveBytes::Memory(bytes))
            .expect("well-formed");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(model.source.as_ref().expect("a source"), &all, &inside);

        assert_eq!(report.written, 1, "only the honest member");
        assert_eq!(report.skipped.len(), 2, "{:?}", report.skipped);
        for (name, why) in &report.skipped {
            assert!(
                matches!(why, SkipReason::Escapes),
                "{name} was refused for the wrong reason: {why}"
            );
        }
        assert!(
            !dir.join("escaped.txt").exists() && !dir.join("also-escaped.txt").exists(),
            "a refused member reached the disk anyway"
        );
        assert!(inside.join("keeps.txt").exists());
        assert!(
            report.summary(&inside).contains("and 1 more"),
            "{}",
            report.summary(&inside)
        );

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn a_name_shaped_like_a_drive_or_a_root_is_refused_too() {
        // `PathBuf::push("C:")` replaces the path rather than extending it, so
        // a drive prefix escapes without ever containing `..`. A leading
        // separator is stripped instead, which is what `unzip` does.
        let dest = Path::new("/dest");
        assert!(matches!(
            safe_destination(dest, b"C:evil.txt"),
            Err(SkipReason::Escapes)
        ));
        assert!(matches!(
            safe_destination(dest, b"a/../../b"),
            Err(SkipReason::Escapes)
        ));
        assert!(matches!(
            safe_destination(dest, b"/"),
            Err(SkipReason::Empty)
        ));
        assert!(matches!(
            safe_destination(dest, b"./././"),
            Err(SkipReason::Empty)
        ));
        assert!(matches!(
            safe_destination(dest, &[0xFF, 0xFE]),
            Err(SkipReason::UnnameableHere)
        ));
        assert_eq!(
            safe_destination(dest, b"/leading/slash.txt").expect("stripped, not refused"),
            Path::new("/dest").join("leading").join("slash.txt")
        );
        assert_eq!(
            safe_destination(dest, b"double//slash.txt").expect("an empty part is skipped"),
            Path::new("/dest").join("double").join("slash.txt")
        );
    }

    #[test]
    fn opening_something_that_is_not_an_archive_says_which_thing_it_is_not() {
        let dir = scratch("open");
        let not_zip = dir.join("notes.txt");
        fs::write(&not_zip, b"hello").expect("write it");
        assert!(matches!(
            open(&not_zip),
            Err(ArchiveError::UnknownFormat { .. })
        ));

        let tar = dir.join("bundle.tar.gz");
        fs::write(&tar, b"not really").expect("write it");
        match open(&tar) {
            Err(e @ ArchiveError::NotTar { .. }) => {
                assert!(e.to_string().contains("not a TAR archive"), "{e}");
            }
            other => panic!("expected it to be said not to be a TAR, got {other:?}"),
        }
        let seven = dir.join("bundle.7z");
        fs::write(&seven, b"7z\xBC\xAF\x27\x1C").expect("write it");
        match open(&seven) {
            Err(e @ ArchiveError::NotYetReadable { .. }) => {
                assert!(e.to_string().contains("reads ZIP, TAR and TAR.GZ"), "{e}");
            }
            other => panic!("expected a refusal naming the format, got {other:?}"),
        }

        let missing = dir.join("gone.zip");
        assert!(matches!(open(&missing), Err(ArchiveError::Io { .. })));

        let garbage = dir.join("garbage.zip");
        fs::write(&garbage, b"PK\x03\x04 and then nothing that makes sense").expect("write it");
        match open(&garbage) {
            Err(ArchiveError::Zip(e)) => assert_eq!(e, ziparchive::Error::CorruptedData),
            other => panic!("expected the parser to refuse it, got {other:?}"),
        }

        fs::remove_dir_all(&dir).expect("remove the scratch directory");
    }

    #[test]
    fn an_error_says_something_a_user_can_act_on() {
        // Every arm, because a status line is the only place these are ever
        // seen and an empty or a debug-shaped one reads as a crash.
        for e in [
            ArchiveError::TooLarge {
                bytes: 900 * 1024 * 1024,
            },
            ArchiveError::UnknownFormat {
                name: String::from("notes.txt"),
            },
            ArchiveError::NotYetReadable {
                format: ArchiveFormat::SevenZip,
            },
            ArchiveError::Zip(ziparchive::Error::UnsupportedMethod),
            ArchiveError::Io {
                path: PathBuf::from("/tmp/x.zip"),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            },
        ] {
            let text = e.to_string();
            assert!(text.len() > 12, "{e:?} says {text:?}");
            assert!(
                !text.starts_with(char::is_uppercase) || text.starts_with("PK"),
                "{text:?} is a sentence fragment and gets pasted after a colon"
            );
        }
        assert!(
            ArchiveError::TooLarge {
                bytes: 900 * 1024 * 1024
            }
            .to_string()
            .contains("512"),
            "the refusal must name the limit it is enforcing"
        );
    }

    #[test]
    fn a_source_prints_its_size_rather_than_its_contents() {
        let bytes = ziparchive::create(&[member("a.txt", b"x")]);
        let model =
            parse_zip(Path::new("/tmp/d.zip"), ArchiveBytes::Memory(bytes)).expect("well-formed");
        let text = format!("{:?}", model.source.as_ref().expect("a source"));
        assert!(text.contains("members: 1"), "{text}");
        assert!(
            !text.contains("120, 0") && text.len() < 120,
            "a Debug that dumps the archive turns one failing assert into a screenful: {text}"
        );
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    /// Write `entries` to a real file in `dir` and open it the way the program
    /// does.
    ///
    /// Through the file rather than through `parse_zip` directly, because every
    /// assertion below is about what ends up *on disk*, and a model whose
    /// `path` names a file that does not exist cannot be saved.
    fn on_disk(dir: &Path, name: &str, entries: &[ZipWriteEntry]) -> ArchiveModel {
        let path = dir.join(name);
        fs::write(&path, ziparchive::create(entries)).expect("write the fixture");
        open(&path).expect("the fixture is a well-formed archive")
    }

    /// Every member name in an archive file, in the order it stores them.
    fn names_in(path: &Path) -> Vec<Vec<u8>> {
        let bytes = fs::read(path).expect("read the archive back");
        ziparchive::parse(&bytes)
            .expect("what we wrote must parse")
            .into_iter()
            .map(|m| m.name)
            .collect()
    }

    /// A rewrite that would not fit in memory is refused before it starts.
    ///
    /// `MAX_ARCHIVE_BYTES` is checked against the file *on disk*, which does
    /// not bound the rewrite at all: a save additionally holds every member's
    /// plaintext, and compression ratio is exactly the gap between those two
    /// numbers. A 500 MB archive of zeroes holds hundreds of gigabytes of
    /// plaintext, passed the open check, and then exhausted memory during a
    /// save -- which is the failure the open check exists to turn into a
    /// message. See known-issues.md ->
    /// `TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY`.
    ///
    /// The budget is passed in rather than tripped honestly: reaching
    /// `MAX_SAVE_BYTES` for real needs an archive claiming 1.5 GiB of
    /// plaintext, which costs 1.5 GiB to write. `save_within` is the body
    /// `save` runs, so this exercises the real path.
    #[test]
    fn a_rewrite_too_big_to_hold_is_refused_and_the_file_is_untouched() {
        let dir = scratch("save-budget");
        // Compressible on purpose: the point is that the archive on disk is
        // small while its plaintext is not, which is the case the on-disk
        // check cannot see.
        let model = on_disk(
            &dir,
            "big.zip",
            &[member("zeroes.bin", &b"\0".repeat(64 * 1024))],
        );
        let on_disk_len = fs::metadata(&model.path).expect("the archive exists").len();

        let projected = {
            let source = model.source.as_ref().expect("a source");
            projected_save_bytes(source, &model, &[])
        };
        // This used to assert the opposite -- that the projection *exceeds*
        // the file on disk, "or it is measuring the wrong thing". That was
        // right while a save held the archive, every member's plaintext and
        // the whole new archive at once: a ZIP of zeroes expands about a
        // thousandfold, so the on-disk check could pass and the save still
        // exhaust memory. Copying members across compressed removed that, so
        // the projection is now below the file on disk -- and the old
        // assertion would fail because the defect it guarded is gone.
        assert!(
            projected < on_disk_len,
            "a compressible archive still costs more to rewrite ({projected}) than it occupies ({on_disk_len}); members are not being copied"
        );
        // And the archive that used to be refused now saves.
        save(&model, Vec::new()).expect("a compressible archive rewrites within budget");
        let model = open(&model.path).expect("and reopens");
        let before = fs::read(&model.path).expect("readable");
        let projected = {
            let source = model.source.as_ref().expect("a source");
            projected_save_bytes(source, &model, &[])
        };

        // A budget under the projection: the case the constant is meant to
        // catch, reached without allocating a gigabyte to do it.
        let err = save_within(&model, Vec::new(), projected - 1)
            .expect_err("a rewrite over budget must be refused");
        match err {
            SaveError::WouldExhaustMemory {
                projected: p,
                limit,
            } => {
                assert_eq!(p, projected, "the refusal must report what it measured");
                assert_eq!(limit, projected - 1);
                // The message names both numbers, because "not enough memory"
                // without them tells the user nothing they can act on.
                let text = SaveError::WouldExhaustMemory {
                    projected: p,
                    limit,
                }
                .to_string();
                assert!(text.contains("memory"), "unhelpful message: {text}");
            }
            other => panic!("wrong refusal: {other:?}"),
        }

        assert_eq!(
            fs::read(&model.path).expect("still readable"),
            before,
            "a refused rewrite must not have touched the file"
        );

        // And exactly at the budget it goes through, so the comparison is not
        // off by one in the direction that refuses work it could do.
        save_within(&model, Vec::new(), projected).expect("a rewrite within budget proceeds");

        fs::remove_dir_all(&dir).ok();
    }

    /// The projection counts the four things that are alive at once, and an
    /// ordinary archive is nowhere near the budget.
    ///
    /// The risk on this side is a false refusal: a projection that over-counts
    /// would refuse rewrites the program can perfectly well do, which is worse
    /// than the bug it fixes because it happens to everybody rather than to
    /// someone opening a zip bomb.
    #[test]
    fn an_ordinary_archive_is_nowhere_near_the_save_budget() {
        let dir = scratch("save-budget-ok");
        let model = on_disk(
            &dir,
            "ordinary.zip",
            &[
                member("src/main.rs", b"fn main() {}\n"),
                member("README.md", &b"read me\n".repeat(40)),
            ],
        );
        let source = model.source.as_ref().expect("a source");
        let projected = projected_save_bytes(source, &model, &[]);
        assert!(
            projected < MAX_SAVE_BYTES,
            "a two-file archive projected {projected}, over the budget"
        );
        // It must still be a real measurement rather than zero.
        assert!(projected > 0, "the projection is not measuring anything");

        save(&model, Vec::new()).expect("an ordinary rewrite is not refused");

        fs::remove_dir_all(&dir).ok();
    }

    /// A directory member contributes nothing to the projection, because it
    /// carries no data -- the same reason the rewrite loop skips it.
    #[test]
    fn a_directory_member_costs_nothing_in_the_projection() {
        let dir = scratch("save-budget-dir");
        let with_dir = on_disk(
            &dir,
            "d.zip",
            &[member("keep/", b""), member("keep/f.txt", b"hello")],
        );
        let source = with_dir.source.as_ref().expect("a source");
        let projected = projected_save_bytes(source, &with_dir, &[]);

        // Asserted as a property rather than as arithmetic: the same archive
        // without the directory member must project the same number. The
        // previous version spelled out a formula, which made it a test of the
        // projection's implementation rather than of the claim in its name --
        // it failed when the formula changed, though directories still cost
        // nothing.
        let without = on_disk(&dir, "nodir.zip", &[member("keep/f.txt", b"hello")]);
        let plain = without.source.as_ref().expect("a source");
        assert_eq!(
            projected,
            projected_save_bytes(plain, &without, &[]),
            "a directory member was charged for data it does not have"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_save_that_changes_nothing_still_produces_a_readable_archive() {
        let dir = scratch("save-roundtrip");
        let model = on_disk(
            &dir,
            "a.zip",
            &[
                member("src/main.rs", b"fn main() {}\n"),
                member("README.md", &b"read me\n".repeat(40)),
            ],
        );

        let report = save(&model, Vec::new()).expect("a rewrite of an archive we just wrote");
        assert_eq!(report.members, 2);
        assert_eq!(report.added, 0);
        assert_eq!(report.replaced, 0);

        // Re-opened rather than compared byte-for-byte: the rewrite is allowed
        // to produce different bytes (it re-compresses), and what has to
        // survive is the contents, not the encoding.
        let after = open(&model.path).expect("the rewritten archive still opens");
        assert_eq!(after.file_count, 2);
        let main = after
            .entries
            .iter()
            .find(|e| e.path == "src/main.rs")
            .expect("the member survived the rewrite");
        let source = after.source.as_ref().expect("a source");
        let data = ziparchive::extract_entry_at(
            &mut source.reader(),
            source.member(main.id).expect("its member"),
        )
        .expect("its bytes come back");
        assert_eq!(data, b"fn main() {}\n", "a rewrite must not alter contents");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_row_removed_from_the_list_is_removed_from_the_file() {
        // The whole point of the writer: Delete edits the list, and without a
        // save the file still holds everything it always did.
        let dir = scratch("save-delete");
        let mut model = on_disk(
            &dir,
            "a.zip",
            &[
                member("keep.txt", b"keep"),
                member("drop.txt", b"drop"),
                member("also-keep.txt", b"keep too"),
            ],
        );
        model.entries.retain(|e| e.path != "drop.txt");

        let report = save(&model, Vec::new()).expect("the rewrite succeeds");
        assert_eq!(report.members, 2);

        let names = names_in(&model.path);
        assert_eq!(names.len(), 2, "the dropped member is gone from the file");
        assert!(
            !names.iter().any(|n| n == b"drop.txt"),
            "the deleted member is still in the file"
        );
        assert!(names.iter().any(|n| n == b"keep.txt"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_added_file_lands_in_the_archive_with_its_own_bytes() {
        let dir = scratch("save-add");
        let model = on_disk(&dir, "a.zip", &[member("old.txt", b"old")]);

        let report = save(
            &model,
            vec![PendingAdd {
                name: b"new.txt".to_vec(),
                data: b"brand new".to_vec(),
                dos_datetime: dos(2026, 8, 26, 14, 30, 52),
                mtime: 0,
            }],
        )
        .expect("the rewrite succeeds");
        assert_eq!(report.members, 2);
        assert_eq!(report.added, 1);
        assert_eq!(report.replaced, 0);

        let after = open(&model.path).expect("the archive still opens");
        let new = after
            .entries
            .iter()
            .find(|e| e.path == "new.txt")
            .expect("the added member is in the archive");
        let source = after.source.as_ref().expect("a source");
        let data = ziparchive::extract_entry_at(
            &mut source.reader(),
            source.member(new.id).expect("its member"),
        )
        .expect("its bytes come back");
        assert_eq!(data, b"brand new");
        assert_ne!(
            ArchiveEntry::format_date(new.modified),
            "-",
            "an added file's mtime must reach the Date column, not be dropped \
             by the rewrite"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adding_a_name_the_archive_already_has_displaces_it_rather_than_doubling_it() {
        // A ZIP with two members of one name is legal to write and ambiguous to
        // read, so writing one is a way of not deciding. The new bytes win, and
        // the count says so.
        let dir = scratch("save-replace");
        let model = on_disk(
            &dir,
            "a.zip",
            &[member("dup.txt", b"the old one"), member("other.txt", b"o")],
        );

        let report = save(
            &model,
            vec![PendingAdd {
                name: b"dup.txt".to_vec(),
                data: b"the new one".to_vec(),
                dos_datetime: 0,
                mtime: 0,
            }],
        )
        .expect("the rewrite succeeds");
        assert_eq!(report.replaced, 1);
        assert_eq!(report.members, 2, "displaced, not appended alongside");

        let names = names_in(&model.path);
        assert_eq!(
            names.iter().filter(|n| n.as_slice() == b"dup.txt").count(),
            1,
            "the archive must not end up with two members of one name"
        );

        let after = open(&model.path).expect("the archive still opens");
        let dup = after
            .entries
            .iter()
            .find(|e| e.path == "dup.txt")
            .expect("the member is there");
        let source = after.source.as_ref().expect("a source");
        let data = ziparchive::extract_entry_at(
            &mut source.reader(),
            source.member(dup.id).expect("its member"),
        )
        .expect("its bytes come back");
        assert_eq!(data, b"the new one", "the added file's bytes must win");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_member_that_cannot_be_reproduced_stops_the_whole_save_and_the_file_is_untouched() {
        // The assertion the writer exists to make safe. This build cannot
        // decrypt, so rewriting would silently drop the encrypted member — and
        // the user would be left with a file they still believe is complete.
        let dir = scratch("save-refuse");
        let path = dir.join("a.zip");
        // A perfectly ordinary member alongside the encrypted one, so the
        // refusal is a refusal to *drop* something and not merely a run that
        // found nothing to write.
        let bytes = with_encrypted_bit(
            &ziparchive::create(&[
                member("plain.txt", b"fine"),
                member("secret.txt", b"cannot be reproduced"),
            ]),
            b"secret.txt",
        );
        fs::write(&path, &bytes).expect("write the fixture");
        let before = fs::read(&path).expect("read the fixture back");

        let model = open(&path).expect("an encrypted archive still opens");
        let error = save(&model, Vec::new()).expect_err("a save that would lose a member");
        match &error {
            SaveError::CannotReproduce { name, why } => {
                assert_eq!(name, "secret.txt");
                assert!(
                    matches!(why, SkipReason::Encrypted),
                    "an encrypted member must be named as such, not called corrupt: {why}"
                );
            }
            other => panic!("wrong refusal: {other}"),
        }
        assert!(
            error.to_string().contains("nothing was changed"),
            "the message must say the file is intact: {error}"
        );

        assert_eq!(
            fs::read(&path).expect("read the archive back"),
            before,
            "a refused save must leave the archive byte-identical"
        );
        assert!(
            fs::read_dir(&dir)
                .expect("list the scratch directory")
                .flatten()
                .all(|e| e.file_name() == "a.zip"),
            "a refused save must not leave a temporary file behind"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_model_with_no_source_is_refused_rather_than_written_as_an_empty_archive() {
        // The dangerous shape: a model built by hand has rows but no bytes to
        // rebuild them from, so a writer that just skipped what it could not
        // find would replace the user's archive with an empty one.
        let dir = scratch("save-nosource");
        let mut model = on_disk(&dir, "a.zip", &[member("a.txt", b"a")]);
        let before = fs::read(&model.path).expect("read the fixture");
        model.source = None;

        let error = save(&model, Vec::new()).expect_err("no source, no rewrite");
        assert!(matches!(error, SaveError::NoSource), "{error}");
        assert_eq!(
            fs::read(&model.path).expect("read the archive back"),
            before,
            "the file must be untouched"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_row_the_source_does_not_know_is_refused_by_its_own_error() {
        // Not `CannotReproduce`: this says the list and the bytes have come
        // apart, which is this program's bug, and the message has to send the
        // user somewhere useful rather than blaming their archive.
        let dir = scratch("save-unknown");
        let mut model = on_disk(&dir, "a.zip", &[member("a.txt", b"a")]);
        let before = fs::read(&model.path).expect("read the fixture");
        model.entries[0].id = 9_999;

        let error = save(&model, Vec::new()).expect_err("an id no member carries");
        assert!(matches!(error, SaveError::UnknownMember { .. }), "{error}");
        assert!(
            error.to_string().contains("Re-open"),
            "the message must say what to do: {error}"
        );
        assert_eq!(
            fs::read(&model.path).expect("read the archive back"),
            before,
            "the file must be untouched"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_name_with_no_utf8_reading_survives_a_rewrite_unchanged() {
        // The rewrite writes `member.name`, not `entry.path`. The path is a
        // lossy rendering built for a column: writing it back would rename the
        // member to one containing U+FFFD, and collapse two members onto one
        // name if they differed only in those bytes.
        let dir = scratch("save-nonutf8");
        let raw = b"caf\xE9.txt".to_vec();
        let path = dir.join("a.zip");
        fs::write(
            &path,
            ziparchive::create(&[ZipWriteEntry {
                name: raw.clone(),
                data: b"latin-1 name".to_vec(),
                store_only: false,
                dos_datetime: 0,
            }]),
        )
        .expect("write the fixture");

        let model = open(&path).expect("a member with a non-UTF-8 name still opens");
        save(&model, Vec::new()).expect("the rewrite succeeds");

        assert_eq!(
            names_in(&path),
            vec![raw],
            "the rewrite must carry the raw name through, not a rendering of it"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_directory_member_survives_a_rewrite() {
        // Directories have no data, so there is nothing to reproduce — and a
        // writer that ran them through the extractor anyway would refuse an
        // archive that is perfectly fine.
        let dir = scratch("save-dirs");
        let path = dir.join("a.zip");
        fs::write(
            &path,
            ziparchive::create(&[
                ZipWriteEntry {
                    name: b"docs/".to_vec(),
                    data: Vec::new(),
                    store_only: true,
                    dos_datetime: 0,
                },
                member("docs/a.txt", b"a"),
            ]),
        )
        .expect("write the fixture");

        let model = open(&path).expect("the fixture opens");
        save(&model, Vec::new()).expect("a rewrite that includes a directory member");

        let names = names_in(&path);
        assert!(
            names.iter().any(|n| n == b"docs/"),
            "the explicit directory member must survive: {names:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_archive_is_an_empty_one_the_program_can_open() {
        // `create_empty` writes the end-of-central-directory record and nothing
        // else. An empty ZIP that will not parse would leave New producing a
        // file the program itself refuses to open.
        let dir = scratch("create-empty");
        let path = dir.join("new.zip");
        create_empty(&path).expect("write an empty archive");

        let model = open(&path).expect("an empty archive must open");
        assert_eq!(model.file_count, 0);
        assert!(model.entries.is_empty());
        assert!(
            model.source.is_some(),
            "a new archive must be writable straight away, or Add is dead on arrival"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_rewrite_never_truncates_the_original_before_it_has_the_replacement() {
        // Not a crash test — nothing here can crash mid-write — but the
        // property that makes one survivable: the target is only ever touched
        // by the rename, so at no point does a partly-written archive wear the
        // user's filename. Asserted by checking the temporary is a sibling
        // (same rename target filesystem) and is gone afterwards.
        let dir = scratch("save-atomic");
        let model = on_disk(&dir, "a.zip", &[member("a.txt", b"a")]);
        save(&model, Vec::new()).expect("the rewrite succeeds");

        let left: Vec<_> = fs::read_dir(&dir)
            .expect("list the scratch directory")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            left.len(),
            1,
            "the temporary must be renamed away, not left beside the archive: {left:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ------------------------------------------------------------------
    // TAR and TAR.GZ
    // ------------------------------------------------------------------

    /// A TAR archive of a folder, a file in it, a file at the root, a hard
    /// link to the first file and a symbolic link -- written by the same
    /// writer `save` uses.
    fn tar_fixture() -> Vec<u8> {
        use tararchive::{Kind, NewMember};
        let mut out = Vec::new();
        let mut put = |name: &str, kind: Kind, data: &[u8], link: Option<&str>| {
            tararchive::write_header(
                &mut out,
                &NewMember {
                    name: name.as_bytes(),
                    kind,
                    mode: 0o644,
                    mtime: 1_700_000_000,
                    size: data.len() as u64,
                    link: link.map(str::as_bytes),
                },
            )
            .unwrap();
            out.extend_from_slice(data);
            tararchive::write_padding(&mut out, data.len() as u64).unwrap();
        };
        put("./", Kind::Directory, b"", None);
        put("./docs/", Kind::Directory, b"", None);
        put("./docs/readme.txt", Kind::File, b"read me first", None);
        put("./top.bin", Kind::File, &[7; 1300], None);
        put(
            "./again.txt",
            Kind::HardLink,
            b"",
            Some("./docs/readme.txt"),
        );
        put("./shortcut", Kind::Symlink, b"", Some("/etc/passwd"));
        tararchive::write_end(&mut out).unwrap();
        out
    }

    #[test]
    fn a_tar_is_listed_as_it_holds() {
        let dir = scratch("tar-list");
        let path = dir.join("bundle.tar");
        fs::write(&path, tar_fixture()).unwrap();
        let model = open(&path).expect("a TAR opens");
        assert_eq!(model.format, ArchiveFormat::Tar);
        let paths: Vec<&str> = model.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "docs",
                "docs/readme.txt",
                "top.bin",
                "again.txt",
                "shortcut"
            ],
            "`./` is the root, not a member, and no name keeps its `./`"
        );
        let readme = &model.entries[1];
        assert_eq!(
            (readme.size, readme.crc32),
            (13, None),
            "no checksum is invented"
        );
        assert_eq!(readme.method, "Stored");
        assert_eq!(readme.modified, 1_700_000_000);
        assert!(model.entries[0].is_dir);
        assert_eq!(model.entries[3].method, "Hard link");
        assert_eq!(model.entries[4].method, "Symbolic link");
        assert_eq!(model.damage, None);
        assert_eq!(model.total_size, 13 + 1300);
        fs::remove_dir_all(&dir).ok();
    }

    /// A TAR.GZ is the same archive, gzipped: inflated, listed, its
    /// compressed size the file's. A gzipped `.tar` is one too, whatever its
    /// name says.
    #[test]
    fn a_gzipped_tar_is_inflated_and_listed_whatever_it_is_called() {
        let dir = scratch("tgz-list");
        let gz = deflate::gzip(&tar_fixture());
        for name in ["bundle.tar.gz", "bundle.tgz", "misnamed.tar"] {
            let path = dir.join(name);
            fs::write(&path, &gz).unwrap();
            let model = open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(model.format, ArchiveFormat::TarGz, "{name}");
            assert_eq!(model.entries.len(), 5, "{name}");
            assert_eq!(model.total_compressed, gz.len() as u64, "{name}");
            assert_eq!(model.entries[1].method, "Gzip");
        }
        // A `.tgz` that is not gzipped is a plain TAR.
        let plain = dir.join("plain.tgz");
        fs::write(&plain, tar_fixture()).unwrap();
        assert_eq!(open(&plain).unwrap().format, ArchiveFormat::Tar);
        // A gzip stream that is damaged says so.
        let mut broken = gz.clone();
        let middle = broken.len() / 2;
        broken.truncate(middle);
        let cut = dir.join("cut.tar.gz");
        fs::write(&cut, &broken).unwrap();
        match open(&cut) {
            Err(e @ ArchiveError::Gzip(_)) => assert!(e.to_string().contains("gzip"), "{e}"),
            other => panic!("expected the gzip to be refused, got {other:?}"),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tar_extracts_its_files_folders_and_hard_links_and_not_its_symbolic_links() {
        let dir = scratch("tar-extract");
        let path = dir.join("bundle.tar");
        fs::write(&path, tar_fixture()).unwrap();
        let model = open(&path).unwrap();
        let source = model.source.as_ref().unwrap();
        let dest = dir.join("out");
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let report = extract(source, &all, &dest);
        assert_eq!(
            fs::read(dest.join("docs/readme.txt")).unwrap(),
            b"read me first"
        );
        assert_eq!(fs::read(dest.join("top.bin")).unwrap(), vec![7; 1300]);
        assert_eq!(
            fs::read(dest.join("again.txt")).unwrap(),
            b"read me first",
            "a hard link is written as a copy of its target"
        );
        assert!(!dest.join("shortcut").exists(), "a symbolic link was made");
        assert_eq!(report.written, 3);
        assert_eq!(report.directories, 1);
        assert_eq!(report.skipped.len(), 1);
        assert!(matches!(report.skipped[0].1, SkipReason::SymbolicLink));
        assert!(
            report.summary(&dest).contains("symbolic link"),
            "{}",
            report.summary(&dest)
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tar_member_named_outside_the_destination_is_refused() {
        let dir = scratch("tar-slip");
        let path = dir.join("evil.tar");
        fs::write(
            &path,
            tararchive::testing::tar(&[("../escape.txt", b"out".as_slice())]),
        )
        .unwrap();
        let model = open(&path).unwrap();
        let all: Vec<&ArchiveEntry> = model.entries.iter().collect();
        let dest = dir.join("out");
        let report = extract(model.source.as_ref().unwrap(), &all, &dest);
        assert_eq!(report.written, 0);
        assert!(matches!(report.skipped[0].1, SkipReason::Escapes));
        assert!(!dir.join("escape.txt").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tar_test_reads_every_member_and_says_where_an_archive_is_damaged() {
        let dir = scratch("tar-test");
        let path = dir.join("bundle.tar");
        fs::write(&path, tar_fixture()).unwrap();
        let results = verify(&open(&path).unwrap());
        assert!(results.all_passed(), "{}", results.summary());
        assert_eq!(results.tested, 4, "every member that is not a folder");
        // A header past the first two that does not add up: the members
        // before it list and pass, and the archive is still said to be damaged.
        let mut bytes = tar_fixture();
        let second_file = 512 * 3; // "./", "./docs/", "./docs/readme.txt" + its block
        bytes[second_file + 512] = b'X';
        let damaged = dir.join("damaged.tar");
        fs::write(&damaged, &bytes).unwrap();
        let model = open(&damaged).unwrap();
        assert!(model.damage.is_some(), "the damage was not noticed");
        assert!(model.entries.len() < 5);
        let results = verify(&model);
        assert!(!results.all_passed());
        assert!(
            results.summary().contains("damaged"),
            "{}",
            results.summary()
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// Adding to a TAR, deleting from it, and the same for a TAR.GZ: the file
    /// is rewritten in its own format, and reads back as the list said.
    #[test]
    fn a_tar_and_a_tar_gz_are_rewritten_in_their_own_format() {
        let dir = scratch("tar-save");
        for (name, gzipped) in [("bundle.tar", false), ("bundle.tar.gz", true)] {
            let path = dir.join(name);
            let bytes = tar_fixture();
            fs::write(
                &path,
                if gzipped {
                    deflate::gzip(&bytes)
                } else {
                    bytes
                },
            )
            .unwrap();
            let mut model = open(&path).unwrap();
            // Delete top.bin, add new.txt, replace docs/readme.txt's name? No:
            // add a file of a name already there, which displaces it.
            model.entries.retain(|e| e.path != "top.bin");
            let report = save(
                &model,
                vec![
                    PendingAdd {
                        name: b"new.txt".to_vec(),
                        data: b"fresh".to_vec(),
                        dos_datetime: 0,
                        mtime: 1_600_000_000,
                    },
                    PendingAdd {
                        name: b"docs/readme.txt".to_vec(),
                        data: b"read me second".to_vec(),
                        dos_datetime: 0,
                        mtime: 0,
                    },
                ],
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((report.added, report.replaced), (2, 1), "{name}");
            let after = open(&path).unwrap();
            assert_eq!(
                after.format,
                if gzipped {
                    ArchiveFormat::TarGz
                } else {
                    ArchiveFormat::Tar
                }
            );
            let paths: Vec<&str> = after.entries.iter().map(|e| e.path.as_str()).collect();
            assert_eq!(
                paths,
                [
                    "docs",
                    "again.txt",
                    "shortcut",
                    "new.txt",
                    "docs/readme.txt"
                ],
                "{name}"
            );
            let new = after.entries.iter().find(|e| e.path == "new.txt").unwrap();
            assert_eq!(new.modified, 1_600_000_000, "{name}");
            let dest = dir.join(format!("{name}-out"));
            let all: Vec<&ArchiveEntry> = after.entries.iter().collect();
            extract(after.source.as_ref().unwrap(), &all, &dest);
            assert_eq!(fs::read(dest.join("new.txt")).unwrap(), b"fresh");
            assert_eq!(
                fs::read(dest.join("docs/readme.txt")).unwrap(),
                b"read me second"
            );
            let link = after.entries.iter().find(|e| e.path == "shortcut").unwrap();
            assert_eq!(link.method, "Symbolic link", "a link is kept as a link");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_archive_is_written_in_the_format_its_name_says() {
        let dir = scratch("tar-create");
        for (name, format) in [
            ("new.tar", ArchiveFormat::Tar),
            ("new.tar.gz", ArchiveFormat::TarGz),
            ("new.zip", ArchiveFormat::Zip),
        ] {
            let path = dir.join(name);
            create_empty(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let model = open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(model.format, format, "{name}");
            assert!(model.entries.is_empty(), "{name}");
        }
        match create_empty(&dir.join("new.7z")) {
            Err(e @ SaveError::Unwritable { .. }) => {
                assert!(e.to_string().contains("7-Zip"), "{e}");
            }
            other => panic!("expected 7z to be refused, got {other:?}"),
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tar_gz_too_large_to_rebuild_in_memory_is_refused_before_anything_is_touched() {
        let dir = scratch("tgz-limit");
        let path = dir.join("big.tar.gz");
        let tar = tararchive::testing::tar(&[("a.bin", [1; 4000].as_slice())]);
        fs::write(&path, deflate::gzip(&tar)).unwrap();
        let before = fs::read(&path).unwrap();
        let model = open(&path).unwrap();
        match save_within(&model, Vec::new(), 1000) {
            Err(SaveError::WouldExhaustMemory { .. }) => {}
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(fs::read(&path).unwrap(), before, "the archive changed");
        fs::remove_dir_all(&dir).ok();
    }
}
