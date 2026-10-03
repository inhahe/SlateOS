//! sevenz: 7z archives, read as 7-Zip reads them.
//!
//! A port of 7-Zip's own 7z reader (`CPP/7zip/Archive/7z/7zIn.cpp`,
//! `7zDecode.cpp`) from the LZMA SDK 26.00, which Igor Pavlov placed in the
//! public domain, with its codecs: LZMA and LZMA2 and the branch converters
//! through the workspace's `xz`, BZip2 through `bzip2`, Deflate through
//! `deflate`, and BCJ2, PPMd and 7z's AES ported here from the same SDK.
//!
//! The whole archive is in memory (`&[u8]`), as the archive manager holds
//! it; a folder -- a solid block of several files -- is decoded whole when
//! any file in it is read.
//!
//! # Names
//!
//! A 7z name is UTF-16, and not always valid UTF-16. [`Entry::name_utf16`]
//! gives the units as stored; [`Entry::name`] gives them as text when they
//! are valid, and nothing is lost either way.
//!
//! # Refusals
//!
//! Each failure is the kind 7-Zip reports for the same archive: not a 7z
//! archive, cut short, a header error ("Headers Error"), something 7-Zip
//! does not support, damaged data ("Data Error"), a CRC that does not match
//! ("CRC Failed"). What 7-Zip only warns about is in [`Warnings`].

#![no_std]

extern crate alloc;

mod decode;
mod header;

use alloc::vec::Vec;

pub use header::Warnings;
use header::{Database, HEADER_SIZE, SIGNATURE};

/// Everything that can go wrong reading a 7z archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The data is not a 7z archive: no signature, a start header that fails
    /// its CRC, or a version 7-Zip does not read.
    NotSevenZip,
    /// The archive is cut short.
    UnexpectedEnd,
    /// The archive's header is damaged (7-Zip's "Headers Error").
    Header,
    /// A feature or compression method this reader does not support
    /// (7-Zip's "Unsupported Method" or "Unsupported feature").
    Unsupported,
    /// A file's compressed data is damaged (7-Zip's "Data Error").
    Data,
    /// A file decompresses, but not to the data its CRC says (7-Zip's "CRC
    /// Failed").
    Crc,
    /// The archive is encrypted and no password was given.
    PasswordRequired,
    /// Decompressing would pass the caller's limit.
    OutputTooLarge,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::NotSevenZip => "not a 7z archive",
            Self::UnexpectedEnd => "the archive ends unexpectedly",
            Self::Header => "the archive's header is damaged",
            Self::Unsupported => {
                "the archive uses a feature or method this reader does not support"
            }
            Self::Data => "the compressed data is damaged",
            Self::Crc => "a file does not match its CRC",
            Self::PasswordRequired => "the archive is encrypted",
            Self::OutputTooLarge => "decompressed size exceeds the caller's limit",
        })
    }
}

/// Shorthand for this crate's fallible operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Whether `data` begins with the 7z signature.
#[must_use]
pub fn looks_like_7z(data: &[u8]) -> bool {
    data.starts_with(&SIGNATURE)
}

/// `N` bytes of `buf` from `o`, if there are.
fn le<const N: usize>(buf: &[u8], o: usize) -> Option<[u8; N]> {
    buf.get(o..o.checked_add(N)?)?.try_into().ok()
}

/// 7-Zip's recovery of an archive whose start header was never written:
/// the header is the end of the file -- its last byte `kEnd`, and its start
/// the last of `kEncodedHeader kPackInfo` or `kHeader kMainStreamsInfo` in
/// the final 512 bytes. Returns its offset after the start header, its size
/// and its CRC.
fn recover(data: &[u8]) -> Option<(u64, u64, u32)> {
    let rem = data.len().saturating_sub(HEADER_SIZE);
    let check = rem.min(512);
    if check < 3 {
        return None;
    }
    let tail = data.get(data.len().saturating_sub(check)..)?;
    if tail.last() != Some(&0) {
        return None;
    }
    let encoded = [
        header::id::ENCODED_HEADER as u8,
        header::id::PACK_INFO as u8,
    ];
    let plain = [
        header::id::HEADER as u8,
        header::id::MAIN_STREAMS_INFO as u8,
    ];
    let start = tail.windows(2).rposition(|w| w == encoded || w == plain)?;
    let size = check.saturating_sub(start) as u64;
    let offset = (rem as u64).saturating_sub(size);
    Some((offset, size, crc32::crc32(tail.get(start..)?)))
}

/// A 7z archive, opened.
pub struct Archive<'a> {
    data: &'a [u8],
    db: Database,
    recovered: bool,
}

/// What an archive is, without its bytes.
impl core::fmt::Debug for Archive<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Archive")
            .field("bytes", &self.data.len())
            .field("entries", &self.db.files.len())
            .field("folders", &self.db.folders.folders.len())
            .field("warnings", &self.db.warnings)
            .finish_non_exhaustive()
    }
}

/// One file or directory of an archive.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    archive: &'a Archive<'a>,
    index: usize,
}

impl<'a> Archive<'a> {
    /// Opens `data` as a 7z archive, as 7-Zip does: the signature at its
    /// start, the start header's CRC, the next header's CRC, and every check
    /// 7-Zip makes of the header.
    ///
    /// # Errors
    ///
    /// [`Error::NotSevenZip`], [`Error::UnexpectedEnd`], [`Error::Header`] or
    /// [`Error::Unsupported`] as 7-Zip refuses the archive; a packed header
    /// that will not decode fails as its data does.
    pub fn open(data: &'a [u8]) -> Result<Self> {
        if !data.starts_with(&SIGNATURE) {
            return Err(Error::NotSevenZip);
        }
        let head = data.get(..HEADER_SIZE).ok_or(Error::UnexpectedEnd)?;
        let byte = |o: usize| head.get(o).copied().unwrap_or(0);
        let le32 = |o: usize| le::<4>(head, o).map_or(0, u32::from_le_bytes);
        let le64 = |o: usize| le::<8>(head, o).map_or(0, u64::from_le_bytes);
        let start_header = head.get(12..HEADER_SIZE).unwrap_or(&[]);
        // `TestSignature2`: the start header's CRC, or -- 7-Zip's recovery of
        // an archive whose writing was cut off -- a start header of zeros
        // under a version that is not.
        let start_crc_ok = crc32::crc32(start_header) == le32(8);
        let recovery_shape = head
            .get(8..HEADER_SIZE)
            .is_some_and(|z| z.iter().all(|&b| b == 0))
            && (byte(6) != 0 || byte(7) != 0);
        if !start_crc_ok && !recovery_shape {
            return Err(Error::NotSevenZip);
        }
        // `ReadDatabase2`: only major version 0 is read.
        if byte(6) != 0 {
            return Err(Error::NotSevenZip);
        }
        let mut next_offset = le64(12);
        let mut next_size = le64(20);
        let mut next_crc = le32(28);
        let after_header = HEADER_SIZE as u64;
        let mut recovered = false;

        if le32(8) == 0 && next_offset == 0 && next_size == 0 && next_crc == 0 {
            let (offset, size, crc) = recover(data).ok_or(Error::NotSevenZip)?;
            (next_offset, next_size, next_crc) = (offset, size, crc);
            recovered = true;
        }

        if next_offset > i64::MAX as u64 || next_size > 1 << 62 {
            return Err(Error::NotSevenZip);
        }
        if next_size == 0 {
            if next_offset != 0 || next_crc != 0 {
                return Err(Error::NotSevenZip);
            }
            return Ok(Self {
                data,
                db: Database::default(),
                recovered,
            });
        }
        let available = (data.len() as u64).saturating_sub(after_header);
        if available < next_offset.saturating_add(next_size) {
            return Err(Error::UnexpectedEnd);
        }
        // Within the data, by the check above.
        let start = after_header.saturating_add(next_offset) as usize;
        let next = data
            .get(start..start.saturating_add(next_size as usize))
            .ok_or(Error::UnexpectedEnd)?;
        if crc32::crc32(next) != next_crc {
            return Err(Error::Header);
        }
        let unpacker = decode::ArchiveUnpacker { data };
        let db = header::read_database(next, after_header, next_offset, &unpacker)?;
        Ok(Self {
            data,
            db,
            recovered,
        })
    }

    /// How many files and directories the archive lists.
    #[must_use]
    pub fn len(&self) -> usize {
        self.db.files.len()
    }

    /// Whether the archive lists nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.db.files.is_empty()
    }

    /// The entry at `index`.
    #[must_use]
    pub fn entry(&'a self, index: usize) -> Option<Entry<'a>> {
        (index < self.db.files.len()).then_some(Entry {
            archive: self,
            index,
        })
    }

    /// Every entry, in the archive's order.
    pub fn entries(&'a self) -> impl Iterator<Item = Entry<'a>> + 'a {
        (0..self.db.files.len()).map(move |index| Entry {
            archive: self,
            index,
        })
    }

    /// What 7-Zip would have warned about while opening the archive.
    #[must_use]
    pub fn warnings(&self) -> Warnings {
        self.db.warnings
    }

    /// Whether the start header was missing and the header found by
    /// searching the end of the file (7-Zip's recovery of an interrupted
    /// archive).
    #[must_use]
    pub fn was_recovered(&self) -> bool {
        self.recovered
    }

    /// Whether any folder holds more than one file.
    #[must_use]
    pub fn is_solid(&self) -> bool {
        self.db.folders.num_unpack_streams.iter().any(|&n| n > 1)
    }

    /// Decodes folder `index` whole, refusing more than `limit` bytes.
    fn folder(&self, index: usize, limit: usize) -> Result<Vec<u8>> {
        let (out, _) = decode::decode_folder(
            self.data,
            &self.db.folders,
            index,
            self.db.data_start,
            limit,
        )?;
        if let Some(crc) = self.db.folders.folder_crcs.get(index).copied().flatten() {
            if crc32::crc32(&out) != crc {
                return Err(Error::Crc);
            }
        }
        Ok(out)
    }

    /// The data of entry `index`, refusing to decode more than `limit`
    /// bytes (the whole folder it is in counts, as it must be decoded).
    ///
    /// # Errors
    ///
    /// [`Error::Data`], [`Error::Crc`] or [`Error::Unsupported`] as 7-Zip
    /// fails to extract the file, and [`Error::OutputTooLarge`] at the cap.
    pub fn read(&self, index: usize, limit: usize) -> Result<Vec<u8>> {
        let file = self.db.files.get(index).ok_or(Error::Header)?;
        if !file.has_stream {
            return Ok(Vec::new());
        }
        let folder = self
            .db
            .file_folder
            .get(index)
            .copied()
            .flatten()
            .ok_or(Error::Header)? as usize;
        let first = self.db.folder_start_file.get(folder).copied().unwrap_or(0) as usize;
        let out = self.folder(folder, limit)?;
        // The file's data starts after the data of the files before it in
        // its folder.
        let offset: u64 = self
            .db
            .files
            .get(first..index)
            .ok_or(Error::Header)?
            .iter()
            .filter(|f| f.has_stream)
            .map(|f| f.size)
            .sum();
        let start = usize::try_from(offset).map_err(|_| Error::Header)?;
        let end = start
            .checked_add(usize::try_from(file.size).map_err(|_| Error::Header)?)
            .ok_or(Error::Header)?;
        let data = out.get(start..end).ok_or(Error::Header)?.to_vec();
        if let Some(crc) = file.crc {
            if crc32::crc32(&data) != crc {
                return Err(Error::Crc);
            }
        }
        Ok(data)
    }
}

impl<'a> Entry<'a> {
    fn record(&self) -> &'a header::FileRecord {
        // An entry is made only for an index the archive has.
        self.archive
            .db
            .files
            .get(self.index)
            .unwrap_or(&header::EMPTY_RECORD)
    }

    /// The entry's position in the archive.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The name as stored: UTF-16 code units, `/` between path components.
    #[must_use]
    pub fn name_utf16(&self) -> &'a [u16] {
        &self.record().name
    }

    /// The name as text, if its units are valid UTF-16.
    #[must_use]
    pub fn name(&self) -> Option<alloc::string::String> {
        alloc::string::String::from_utf16(&self.record().name).ok()
    }

    /// The size of the entry's data.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.record().size
    }

    /// Whether the entry is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.record().is_dir
    }

    /// Whether the entry has data in a folder (an empty file has none).
    #[must_use]
    pub fn has_stream(&self) -> bool {
        self.record().has_stream
    }

    /// Whether the entry is an "anti-item": a deletion recorded by an
    /// update.
    #[must_use]
    pub fn is_anti(&self) -> bool {
        self.record().is_anti
    }

    /// The CRC-32 of the data, if the archive records one.
    #[must_use]
    pub fn crc(&self) -> Option<u32> {
        self.record().crc
    }

    /// Modification, creation and access times, as Windows `FILETIME`s
    /// (100 ns since 1601), when recorded.
    #[must_use]
    pub fn mtime(&self) -> Option<u64> {
        self.record().mtime
    }

    /// See [`Entry::mtime`].
    #[must_use]
    pub fn ctime(&self) -> Option<u64> {
        self.record().ctime
    }

    /// See [`Entry::mtime`].
    #[must_use]
    pub fn atime(&self) -> Option<u64> {
        self.record().atime
    }

    /// The attributes, when recorded: Windows attributes in the low 16 bits,
    /// and -- when bit 15 (`FILE_ATTRIBUTE_UNIX_EXTENSION`) is set -- a Unix
    /// mode in the high 16.
    #[must_use]
    pub fn attributes(&self) -> Option<u32> {
        self.record().attrib
    }

    /// The entry's data, as [`Archive::read`].
    ///
    /// # Errors
    ///
    /// As [`Archive::read`].
    pub fn read(&self, limit: usize) -> Result<Vec<u8>> {
        self.archive.read(self.index, limit)
    }
}
