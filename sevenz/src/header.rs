//! The archive's database: 7-Zip's `7zIn.cpp` (`CInArchive`), ported.
//!
//! A 7z archive is a 32-byte start header, packed data, and a "next header"
//! -- itself often packed (`kEncodedHeader`) -- describing the folders (each
//! a graph of coders over some packed streams), how each folder's output
//! splits into files, and the files' names, times and attributes. This reads
//! it as 7-Zip does: every check 7-Zip makes is made, and each failure is
//! the kind 7-Zip reports -- a header error ([`Error::Header`]: "Headers
//! Error"), a feature it does not support ([`Error::Unsupported`]), or not an
//! archive at all ([`Error::NotSevenZip`]). The faults 7-Zip only warns about
//! ("There are some data after the end of the payload data", a name list
//! longer than the names) are recorded in [`Warnings`] and the archive opens.

// Counts and offsets here are bounded by the header they are read from --
// every item takes at least a byte of it -- and sizes the header supplies
// are summed with `checked_add`.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Result};

/// The signature: `7z\xBC\xAF\x27\x1C`.
pub(crate) const SIGNATURE: [u8; 6] = [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c];

/// `kHeaderSize`: signature, version, start header CRC, start header.
pub(crate) const HEADER_SIZE: usize = 32;

/// `k_Scan_NumCoders_MAX`, `k_Scan_NumCodersStreams_in_Folder_MAX`
const NUM_CODERS_MAX: u32 = 64;
const NUM_STREAMS_MAX: u32 = 64;

/// `kNumMax`: a count above this is unsupported.
const NUM_MAX: u64 = 0x7fff_ffff;

/// The property IDs (`NID`).
pub(crate) mod id {
    pub(crate) const END: u64 = 0;
    pub(crate) const HEADER: u64 = 1;
    pub(crate) const ARCHIVE_PROPERTIES: u64 = 2;
    pub(crate) const ADDITIONAL_STREAMS_INFO: u64 = 3;
    pub(crate) const MAIN_STREAMS_INFO: u64 = 4;
    pub(crate) const FILES_INFO: u64 = 5;
    pub(crate) const PACK_INFO: u64 = 6;
    pub(crate) const UNPACK_INFO: u64 = 7;
    pub(crate) const SUB_STREAMS_INFO: u64 = 8;
    pub(crate) const SIZE: u64 = 9;
    pub(crate) const CRC: u64 = 10;
    pub(crate) const FOLDER: u64 = 11;
    pub(crate) const CODERS_UNPACK_SIZE: u64 = 12;
    pub(crate) const NUM_UNPACK_STREAM: u64 = 13;
    pub(crate) const EMPTY_STREAM: u64 = 14;
    pub(crate) const EMPTY_FILE: u64 = 15;
    pub(crate) const ANTI: u64 = 16;
    pub(crate) const NAME: u64 = 17;
    pub(crate) const CTIME: u64 = 18;
    pub(crate) const ATIME: u64 = 19;
    pub(crate) const MTIME: u64 = 20;
    pub(crate) const WIN_ATTRIB: u64 = 21;
    pub(crate) const ENCODED_HEADER: u64 = 23;
    pub(crate) const START_POS: u64 = 24;
    pub(crate) const DUMMY: u64 = 25;
}

/// What 7-Zip reports about a damaged header without refusing the archive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Warnings {
    /// `ThereIsHeaderError` set without a failure: bytes left over in a part
    /// of the header, a name list longer than the names, a file list that
    /// does not cover the folders.
    pub header: bool,
    /// `UnsupportedFeatureWarning`: a file property or header part this
    /// reader does not know, skipped.
    pub unsupported_feature: bool,
    /// The packed header's data continued past its end
    /// (`dataAfterEnd_Error` while decoding it).
    pub data_after_end: bool,
}

/// One coder of a folder (`CCoderInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Coder {
    pub(crate) method: u64,
    pub(crate) props: Vec<u8>,
    pub(crate) num_streams: u32,
}

/// A bond (`CBond`): coder input `pack_index` reads coder `unpack_index`'s
/// output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bond {
    pub(crate) pack_index: u32,
    pub(crate) unpack_index: u32,
}

/// A folder (`CFolderEx`): its coders, the bonds between them, which coder
/// inputs are packed streams, and which coder's output is the folder's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Folder {
    pub(crate) coders: Vec<Coder>,
    pub(crate) bonds: Vec<Bond>,
    pub(crate) pack_streams: Vec<u32>,
    pub(crate) unpack_coder: u32,
}

/// `CFolders`: the folders, their packed streams, and their sizes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Folders {
    pub(crate) num_pack_streams: u32,
    /// `PackPositions`: `num_pack_streams + 1` offsets from the data start.
    pub(crate) pack_positions: Vec<u64>,
    /// `FolderCRCs`, per folder.
    pub(crate) folder_crcs: Vec<Option<u32>>,
    /// `NumUnpackStreamsVector`: how many files' data each folder holds.
    pub(crate) num_unpack_streams: Vec<u32>,
    /// `CoderUnpackSizes`, every coder's output size, folder after folder.
    pub(crate) coder_unpack_sizes: Vec<u64>,
    /// `FoToCoderUnpackSizes`: where each folder's sizes start.
    pub(crate) fo_to_coder_unpack_sizes: Vec<u32>,
    /// `FoStartPackStreamIndex`: each folder's first packed stream.
    pub(crate) fo_start_pack_stream_index: Vec<u32>,
    pub(crate) folders: Vec<Folder>,
}

impl Folders {
    /// `GetFolderUnpackSize`: the folder's output size, its unpack coder's.
    pub(crate) fn unpack_size(&self, folder: usize) -> u64 {
        let start = self
            .fo_to_coder_unpack_sizes
            .get(folder)
            .copied()
            .unwrap_or(0) as usize;
        let main = self
            .folders
            .get(folder)
            .map_or(0, |f| f.unpack_coder as usize);
        self.coder_unpack_sizes
            .get(start.wrapping_add(main))
            .copied()
            .unwrap_or(0)
    }

    /// A coder's output size within folder `folder`.
    pub(crate) fn coder_unpack_size(&self, folder: usize, coder: usize) -> u64 {
        let start = self
            .fo_to_coder_unpack_sizes
            .get(folder)
            .copied()
            .unwrap_or(0) as usize;
        self.coder_unpack_sizes
            .get(start.wrapping_add(coder))
            .copied()
            .unwrap_or(0)
    }

    /// The offset (from the data start) and size of the folder's `j`th
    /// packed stream.
    pub(crate) fn pack_stream(&self, folder: usize, j: usize) -> (u64, u64) {
        let i = (self
            .fo_start_pack_stream_index
            .get(folder)
            .copied()
            .unwrap_or(0) as usize)
            .wrapping_add(j);
        let start = self.pack_positions.get(i).copied().unwrap_or(0);
        let end = self
            .pack_positions
            .get(i.wrapping_add(1))
            .copied()
            .unwrap_or(start);
        (start, end.saturating_sub(start))
    }
}

/// One file of the archive (`CFileItem` and the database's per-file vectors).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileRecord {
    pub(crate) size: u64,
    pub(crate) crc: Option<u32>,
    pub(crate) has_stream: bool,
    pub(crate) is_dir: bool,
    pub(crate) is_anti: bool,
    pub(crate) name: Vec<u16>,
    pub(crate) ctime: Option<u64>,
    pub(crate) atime: Option<u64>,
    pub(crate) mtime: Option<u64>,
    pub(crate) attrib: Option<u32>,
    pub(crate) start_pos: Option<u64>,
}

/// A file record for nothing, for an index out of range.
pub(crate) static EMPTY_RECORD: FileRecord = FileRecord {
    size: 0,
    crc: None,
    has_stream: false,
    is_dir: false,
    is_anti: false,
    name: Vec::new(),
    ctime: None,
    atime: None,
    mtime: None,
    attrib: None,
    start_pos: None,
};

/// `CDbEx`: everything the header says.
#[derive(Debug, Clone, Default)]
pub(crate) struct Database {
    pub(crate) folders: Folders,
    pub(crate) files: Vec<FileRecord>,
    /// Where the main streams' packed data starts, in the archive.
    pub(crate) data_start: u64,
    /// `FolderStartFileIndex`, `FileIndexToFolderIndexMap`.
    pub(crate) folder_start_file: Vec<u32>,
    pub(crate) file_folder: Vec<Option<u32>>,
    pub(crate) warnings: Warnings,
}

/// `CInByte2`: a cursor over one buffer of the header.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) const fn rem(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or(&[])
    }

    /// `ReadByte`
    pub(crate) fn byte(&mut self) -> Result<u8> {
        let &b = self.buf.get(self.pos).ok_or(Error::Header)?;
        self.pos = self.pos.wrapping_add(1);
        Ok(b)
    }

    /// `ReadBytes`
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.rem() {
            return Err(Error::Header);
        }
        let out = self
            .buf
            .get(self.pos..self.pos.wrapping_add(n))
            .unwrap_or(&[]);
        self.pos = self.pos.wrapping_add(n);
        Ok(out)
    }

    /// `SkipData(size)`
    fn skip(&mut self, n: u64) -> Result<()> {
        if n > self.rem() as u64 {
            return Err(Error::Header);
        }
        self.pos = self.pos.wrapping_add(n as usize);
        Ok(())
    }

    /// `SkipData()`: a size, then that many bytes.
    fn skip_data(&mut self) -> Result<()> {
        let n = self.number()?;
        self.skip(n)
    }

    /// `ReadNumber`: 7z's variable-length integer -- the first byte's top
    /// bits count the bytes that follow, little-endian, and its remaining
    /// bits are the value's highest.
    pub(crate) fn number(&mut self) -> Result<u64> {
        let b = u32::from(self.byte()?);
        if b & 0x80 == 0 {
            return Ok(u64::from(b));
        }
        let mut value = u64::from(self.byte()?);
        for i in 1..8u32 {
            let mask = 0x80u32 >> i;
            if b & mask == 0 {
                let high = u64::from(b & (mask - 1));
                return Ok(value | (high << (i * 8)));
            }
            value |= u64::from(self.byte()?) << (i * 8);
        }
        Ok(value)
    }

    /// `ReadNum`: a count, at most `kNumMax`.
    pub(crate) fn num(&mut self) -> Result<u32> {
        let v = self.number()?;
        if v > NUM_MAX {
            return Err(Error::Unsupported);
        }
        Ok(v as u32)
    }

    /// `ReadUInt32`
    fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(b);
        Ok(u32::from_le_bytes(a))
    }

    /// `ReadUInt64`
    fn u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }

    /// `ReadID`
    pub(crate) fn id(&mut self) -> Result<u64> {
        self.number()
    }

    /// `WaitId`: skips properties until `want`; reaching `kEnd` first is a
    /// header error.
    fn wait_id(&mut self, want: u64) -> Result<()> {
        loop {
            let t = self.id()?;
            if t == want {
                return Ok(());
            }
            if t == id::END {
                return Err(Error::Header);
            }
            self.skip_data()?;
        }
    }

    /// `ReadBoolVector`: `n` bits, high bit first.
    fn bool_vector(&mut self, n: usize) -> Result<Vec<bool>> {
        let mut v = Vec::with_capacity(n.min(self.rem().saturating_mul(8)));
        let mut b = 0u8;
        let mut mask = 0u8;
        for _ in 0..n {
            if mask == 0 {
                b = self.byte()?;
                mask = 0x80;
            }
            v.push(b & mask != 0);
            mask >>= 1;
        }
        Ok(v)
    }

    /// `ReadBoolVector2`: an "all defined" byte, or the vector.
    fn bool_vector2(&mut self, n: usize) -> Result<Vec<bool>> {
        if self.byte()? == 0 {
            self.bool_vector(n)
        } else {
            Ok(vec![true; n])
        }
    }

    /// `ReadHashDigests`
    fn digests(&mut self, n: usize) -> Result<Vec<Option<u32>>> {
        let defs = self.bool_vector2(n)?;
        defs.into_iter()
            .map(|d| if d { self.u32().map(Some) } else { Ok(None) })
            .collect()
    }
}

/// `CStreamSwitch::Set(archive, dataVector)`: an "external" byte; when set,
/// the data that follows is in the decoded additional stream it names.
/// Returns that stream's reader, or `None` to go on in `r`.
fn external<'d>(r: &mut Reader<'_>, data: Option<&'d [Vec<u8>]>) -> Result<Option<Reader<'d>>> {
    if r.byte()? == 0 {
        return Ok(None);
    }
    let data = data.ok_or(Error::Header)?;
    let index = r.num()? as usize;
    let buf = data.get(index).ok_or(Error::Header)?;
    Ok(Some(Reader::new(buf)))
}

/// `CStreamSwitch::Remove`: a switched-to buffer not read to its end is a
/// header warning.
fn done(r: &Reader<'_>, warnings: &mut Warnings) {
    if r.rem() != 0 {
        warnings.header = true;
    }
}

/// `ReadPackInfo`
fn pack_info(r: &mut Reader<'_>, f: &mut Folders) -> Result<()> {
    let n = r.num()?;
    r.wait_id(id::SIZE)?;
    let mut positions = Vec::with_capacity((n as usize).saturating_add(1).min(r.rem()));
    let mut sum = 0u64;
    for _ in 0..n {
        positions.push(sum);
        let size = r.number()?;
        sum = sum.checked_add(size).ok_or(Error::Header)?;
    }
    positions.push(sum);
    f.num_pack_streams = n;
    f.pack_positions = positions;
    loop {
        let t = r.id()?;
        if t == id::END {
            return Ok(());
        }
        if t == id::CRC {
            // The packed streams' CRCs: read, and not used (as 7-Zip).
            r.digests(n as usize)?;
            continue;
        }
        r.skip_data()?;
    }
}

/// `ReadUnpackInfo`: the folders, from `r` or from an additional stream.
fn unpack_info(
    r: &mut Reader<'_>,
    data: Option<&[Vec<u8>]>,
    f: &mut Folders,
    warnings: &mut Warnings,
) -> Result<()> {
    r.wait_id(id::FOLDER)?;
    let num_folders = r.num()?;
    let mut ext = external(r, data)?;
    let num_coders_out = match ext.as_mut() {
        Some(e) => read_folders(e, num_folders, f)?,
        None => read_folders(r, num_folders, f)?,
    };
    if let Some(e) = ext.as_ref() {
        done(e, warnings);
    }

    r.wait_id(id::CODERS_UNPACK_SIZE)?;
    let mut sizes = Vec::with_capacity((num_coders_out as usize).min(r.rem()));
    for _ in 0..num_coders_out {
        sizes.push(r.number()?);
    }
    f.coder_unpack_sizes = sizes;
    f.folder_crcs = vec![None; num_folders as usize];
    loop {
        let t = r.id()?;
        if t == id::END {
            return Ok(());
        }
        if t == id::CRC {
            f.folder_crcs = r.digests(num_folders as usize)?;
            continue;
        }
        r.skip_data()?;
    }
}

/// The folders themselves (`ReadUnpackInfo`'s loop), from whichever buffer
/// holds them; returns how many coder outputs they have.
fn read_folders(src: &mut Reader<'_>, num_folders: u32, f: &mut Folders) -> Result<u32> {
    let mut num_coders_out = 0u32;
    let mut pack_index = 0u32;
    let mut folders = Vec::new();
    let mut fo_to_sizes = Vec::new();
    let mut fo_start_pack = Vec::new();
    for _ in 0..num_folders {
        let folder = parse_folder(src)?;
        fo_to_sizes.push(num_coders_out);
        num_coders_out = num_coders_out.saturating_add(folder.coders.len() as u32);
        fo_start_pack.push(pack_index);
        let n = folder.pack_streams.len() as u32;
        if n > f.num_pack_streams.saturating_sub(pack_index) {
            return Err(Error::Header);
        }
        pack_index += n;
        folders.push(folder);
    }
    fo_to_sizes.push(num_coders_out);
    fo_start_pack.push(pack_index);
    f.folders = folders;
    f.fo_to_coder_unpack_sizes = fo_to_sizes;
    f.fo_start_pack_stream_index = fo_start_pack;
    Ok(num_coders_out)
}

/// One folder's coders, bonds and packed streams, with the checks
/// `ReadUnpackInfo` makes as it reads them.
fn parse_folder(r: &mut Reader<'_>) -> Result<Folder> {
    let num_coders = r.num()?;
    if num_coders == 0 || num_coders > NUM_CODERS_MAX {
        return Err(Error::Unsupported);
    }
    let mut coders = Vec::with_capacity(num_coders as usize);
    let mut num_in_streams = 0u32;
    for _ in 0..num_coders {
        let main = r.byte()?;
        if main & 0xc0 != 0 {
            return Err(Error::Unsupported);
        }
        let id_size = usize::from(main & 0x0f);
        if id_size > 8 {
            return Err(Error::Unsupported);
        }
        if id_size > r.rem() {
            return Err(Error::Header);
        }
        let mut method = 0u64;
        for &b in r.bytes(id_size)? {
            method = (method << 8) | u64::from(b);
        }
        let mut num_streams = 1u32;
        if main & 0x10 != 0 {
            num_streams = r.num()?;
            if num_streams > NUM_STREAMS_MAX {
                return Err(Error::Unsupported);
            }
            if r.num()? != 1 {
                return Err(Error::Unsupported);
            }
        }
        num_in_streams = num_in_streams.saturating_add(num_streams);
        if num_in_streams > NUM_STREAMS_MAX {
            return Err(Error::Unsupported);
        }
        let mut props = Vec::new();
        if main & 0x20 != 0 {
            let size = r.num()? as usize;
            if size > r.rem() {
                return Err(Error::Header);
            }
            props = r.bytes(size)?.to_vec();
        }
        coders.push(Coder {
            method,
            props,
            num_streams,
        });
    }

    let mut bonds = Vec::new();
    let mut pack_streams = Vec::new();
    let unpack_coder;
    if num_coders == 1 && num_in_streams == 1 {
        unpack_coder = 0;
        pack_streams.push(0);
    } else {
        let num_bonds = num_coders - 1;
        if num_in_streams < num_bonds {
            return Err(Error::Unsupported);
        }
        let mut stream_used = vec![false; num_in_streams as usize];
        let mut coder_used = vec![false; num_coders as usize];
        for _ in 0..num_bonds {
            let pack_index = r.num()?;
            match stream_used.get_mut(pack_index as usize) {
                Some(u) if !*u => *u = true,
                _ => return Err(Error::Unsupported),
            }
            let unpack_index = r.num()?;
            match coder_used.get_mut(unpack_index as usize) {
                Some(u) if !*u => *u = true,
                _ => return Err(Error::Unsupported),
            }
            bonds.push(Bond {
                pack_index,
                unpack_index,
            });
        }
        let num_pack_streams = num_in_streams - num_bonds;
        if num_pack_streams == 1 {
            // `ParseFolder`: the one input no bond feeds.
            let free = (0..num_in_streams).find(|i| !bonds.iter().any(|b| b.pack_index == *i));
            pack_streams.push(free.ok_or(Error::Unsupported)?);
        } else {
            for _ in 0..num_pack_streams {
                let index = r.num()?;
                match stream_used.get_mut(index as usize) {
                    Some(u) if !*u => *u = true,
                    _ => return Err(Error::Unsupported),
                }
                pack_streams.push(index);
            }
        }
        unpack_coder = coder_used
            .iter()
            .position(|u| !u)
            .ok_or(Error::Unsupported)? as u32;
    }
    Ok(Folder {
        coders,
        bonds,
        pack_streams,
        unpack_coder,
    })
}

/// `ReadSubStreamsInfo`: how each folder's output divides into files, and
/// their CRCs. Returns the unpack sizes and digests, one per file with data.
fn sub_streams_info(r: &mut Reader<'_>, f: &mut Folders) -> Result<(Vec<u64>, Vec<Option<u32>>)> {
    let num_folders = f.folders.len();
    f.num_unpack_streams = vec![1; num_folders];
    let mut t;
    loop {
        t = r.id()?;
        if t == id::NUM_UNPACK_STREAM {
            for n in &mut f.num_unpack_streams {
                *n = r.num()?;
            }
            continue;
        }
        if t == id::CRC || t == id::SIZE || t == id::END {
            break;
        }
        r.skip_data()?;
    }

    let mut sizes = Vec::new();
    if t == id::SIZE {
        for i in 0..num_folders {
            let n = f.num_unpack_streams.get(i).copied().unwrap_or(0);
            if n == 0 {
                continue;
            }
            let mut sum = 0u64;
            for _ in 1..n {
                let size = r.number()?;
                sizes.push(size);
                sum = sum.checked_add(size).ok_or(Error::Header)?;
            }
            let folder_size = f.unpack_size(i);
            if folder_size < sum {
                return Err(Error::Header);
            }
            sizes.push(folder_size - sum);
        }
        t = r.id()?;
    } else {
        for i in 0..num_folders {
            let n = f.num_unpack_streams.get(i).copied().unwrap_or(0);
            if n > 1 {
                return Err(Error::Header);
            }
            if n == 1 {
                sizes.push(f.unpack_size(i));
            }
        }
    }

    let folder_crc = |i: usize| f.folder_crcs.get(i).copied().flatten();
    let num_digests: usize = (0..num_folders)
        .map(|i| {
            let n = f.num_unpack_streams.get(i).copied().unwrap_or(0);
            if n != 1 || folder_crc(i).is_none() {
                n as usize
            } else {
                0
            }
        })
        .sum();

    let mut digests: Option<Vec<Option<u32>>> = None;
    loop {
        if t == id::END {
            break;
        }
        if t == id::CRC {
            let defs = r.bool_vector2(num_digests)?;
            let mut out = Vec::with_capacity(sizes.len());
            let mut k2 = 0usize;
            for i in 0..num_folders {
                let n = f.num_unpack_streams.get(i).copied().unwrap_or(0);
                if n == 1 && folder_crc(i).is_some() {
                    out.push(folder_crc(i));
                } else {
                    for _ in 0..n {
                        let defined = defs.get(k2).copied().unwrap_or(false);
                        k2 += 1;
                        out.push(if defined { Some(r.u32()?) } else { None });
                    }
                }
            }
            digests = Some(out);
        } else {
            r.skip_data()?;
        }
        t = r.id()?;
    }

    // Digests not given, or given for another count: the folders' own CRCs
    // where a folder holds one file, none otherwise.
    let digests = match digests {
        Some(d) if d.len() == sizes.len() => d,
        _ => {
            let mut out = Vec::with_capacity(sizes.len());
            for i in 0..num_folders {
                let n = f.num_unpack_streams.get(i).copied().unwrap_or(0);
                if n == 1 && folder_crc(i).is_some() {
                    out.push(folder_crc(i));
                } else {
                    out.extend(core::iter::repeat_n(None, n as usize));
                }
            }
            out
        }
    };
    Ok((sizes, digests))
}

/// `ReadStreamsInfo`: packed streams, folders, and their division into
/// files. `range_limit` is where the packed data must end (the next
/// header's offset).
fn streams_info(
    r: &mut Reader<'_>,
    data: Option<&[Vec<u8>]>,
    range_limit: u64,
    f: &mut Folders,
    warnings: &mut Warnings,
) -> Result<(u64, Vec<u64>, Vec<Option<u32>>)> {
    let mut data_offset = 0u64;
    let mut t = r.id()?;
    if t == id::PACK_INFO {
        data_offset = r.number()?;
        if data_offset > range_limit {
            return Err(Error::Header);
        }
        pack_info(r, f)?;
        let total = f.pack_positions.last().copied().unwrap_or(0);
        if total > range_limit - data_offset {
            return Err(Error::Header);
        }
        t = r.id()?;
    }
    if t == id::UNPACK_INFO {
        unpack_info(r, data, f, warnings)?;
        t = r.id()?;
    }
    if !f.folders.is_empty() && f.pack_positions.is_empty() {
        f.pack_positions = vec![0];
    }
    let (sizes, digests);
    if t == id::SUB_STREAMS_INFO {
        (sizes, digests) = sub_streams_info(r, f)?;
        t = r.id()?;
    } else {
        f.num_unpack_streams = vec![1; f.folders.len()];
        sizes = (0..f.folders.len()).map(|i| f.unpack_size(i)).collect();
        digests = Vec::new();
    }
    if t != id::END {
        return Err(Error::Header);
    }
    Ok((data_offset, sizes, digests))
}

/// What reading a header needs from outside it: the archive, to decode the
/// packed parts of the header.
pub(crate) trait Unpacker {
    /// Decodes folder `index` of `folders`, whose packed data starts at
    /// `base`, to exactly its unpack size, or fails as decoding a file
    /// would; returns the data and whether data followed the end.
    fn unpack(&self, folders: &Folders, index: usize, base: u64) -> Result<(Vec<u8>, bool)>;
}

/// `ReadAndDecodePackedStreams`: the streams that hold packed header
/// parts, decoded. A folder that does not decode to its size, or fails its
/// CRC, is a header error.
fn decode_packed_streams(
    r: &mut Reader<'_>,
    base: u64,
    range_limit: u64,
    unpacker: &dyn Unpacker,
    warnings: &mut Warnings,
) -> Result<Vec<Vec<u8>>> {
    let mut f = Folders::default();
    let (data_offset, _, _) = streams_info(r, None, range_limit, &mut f, warnings)?;
    let start = base.checked_add(data_offset).ok_or(Error::Header)?;
    let mut out = Vec::with_capacity(f.folders.len());
    for i in 0..f.folders.len() {
        let (data, after_end) = unpacker.unpack(&f, i, start)?;
        if after_end {
            warnings.header = true;
            warnings.data_after_end = true;
        }
        if data.len() as u64 != f.unpack_size(i) {
            return Err(Error::Header);
        }
        if let Some(crc) = f.folder_crcs.get(i).copied().flatten() {
            if crc32::crc32(&data) != crc {
                return Err(Error::Header);
            }
        }
        out.push(data);
    }
    Ok(out)
}

/// Reads the archive's database from the next header `buf` (already
/// checked against its CRC). `base` is the position after the start
/// header, where packed data offsets count from; `range_limit` the next
/// header's offset from there.
pub(crate) fn read_database(
    buf: &[u8],
    base: u64,
    range_limit: u64,
    unpacker: &dyn Unpacker,
) -> Result<Database> {
    let mut warnings = Warnings::default();
    let mut r = Reader::new(buf);
    let t = r.id()?;
    if t == id::HEADER {
        let db = read_header(&mut r, base, range_limit, unpacker, &mut warnings)?;
        done(&r, &mut warnings);
        return Ok(finish(db, warnings));
    }
    if t != id::ENCODED_HEADER {
        return Err(Error::Header);
    }
    let decoded = decode_packed_streams(&mut r, base, range_limit, unpacker, &mut warnings)?;
    done(&r, &mut warnings);
    let header = match decoded.as_slice() {
        // An encoded header of no streams: an archive with nothing in it.
        [] => {
            return Ok(finish(Database::default(), warnings));
        }
        [one] => one,
        _ => return Err(Error::Header),
    };
    let mut r = Reader::new(header);
    if r.id()? != id::HEADER {
        return Err(Error::Header);
    }
    let db = read_header(&mut r, base, range_limit, unpacker, &mut warnings)?;
    done(&r, &mut warnings);
    Ok(finish(db, warnings))
}

fn finish(mut db: Database, warnings: Warnings) -> Database {
    db.warnings.header |= warnings.header;
    db.warnings.unsupported_feature |= warnings.unsupported_feature;
    db.warnings.data_after_end |= warnings.data_after_end;
    db
}

/// `ReadHeader`
fn read_header(
    r: &mut Reader<'_>,
    base: u64,
    range_limit: u64,
    unpacker: &dyn Unpacker,
    warnings: &mut Warnings,
) -> Result<Database> {
    let mut db = Database::default();
    let mut t = r.id()?;
    if t == id::ARCHIVE_PROPERTIES {
        loop {
            if r.id()? == id::END {
                break;
            }
            r.skip_data()?;
        }
        t = r.id()?;
    }

    let mut data_vector = Vec::new();
    if t == id::ADDITIONAL_STREAMS_INFO {
        data_vector = decode_packed_streams(r, base, range_limit, unpacker, warnings)?;
        t = r.id()?;
    }

    let mut unpack_sizes = Vec::new();
    let mut digests = Vec::new();
    if t == id::MAIN_STREAMS_INFO {
        let (offset, sizes, d) = streams_info(
            r,
            Some(&data_vector),
            range_limit,
            &mut db.folders,
            warnings,
        )?;
        db.data_start = base.checked_add(offset).ok_or(Error::Header)?;
        unpack_sizes = sizes;
        digests = d;
        t = r.id()?;
    }

    if t == id::FILES_INFO {
        read_files(r, &data_vector, &unpack_sizes, &digests, &mut db, warnings)?;
        t = r.id()?;
    }

    fill_links(&mut db, warnings)?;

    if t != id::END || r.rem() != 0 {
        warnings.unsupported_feature = true;
    }
    Ok(db)
}

/// `kName`: the rest of `src` is the names, UTF-16LE, each ended by a zero
/// unit. A name the data ends inside is a header error; data left after the
/// last name, a header warning.
fn read_names(
    src: &mut Reader<'_>,
    num_files: usize,
    warnings: &mut Warnings,
) -> Result<Vec<Vec<u16>>> {
    let all = src.rest();
    src.pos = src.buf.len();
    let mut names = Vec::with_capacity(num_files.min(all.len() / 2));
    let mut units = all
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap_or([0, 0])));
    let mut consumed = 0usize;
    for _ in 0..num_files {
        let mut name = Vec::new();
        loop {
            // 7-Zip's ThrowEndOfData.
            let u = units.next().ok_or(Error::Header)?;
            consumed += 2;
            if u == 0 {
                break;
            }
            name.push(u);
        }
        names.push(name);
    }
    if consumed != all.len() {
        warnings.header = true;
    }
    Ok(names)
}

/// `Read_UInt32_Vector`: a value for each defined entry.
fn read_u32s(src: &mut Reader<'_>, defs: &[bool]) -> Result<Vec<Option<u32>>> {
    defs.iter()
        .map(|&d| if d { src.u32().map(Some) } else { Ok(None) })
        .collect()
}

/// `ReadUInt64DefVector`'s values: one for each defined entry.
fn read_u64s(src: &mut Reader<'_>, defs: &[bool]) -> Result<Vec<Option<u64>>> {
    defs.iter()
        .map(|&d| if d { src.u64().map(Some) } else { Ok(None) })
        .collect()
}

/// The `kFilesInfo` part of `ReadHeader`.
fn read_files(
    r: &mut Reader<'_>,
    data_vector: &[Vec<u8>],
    unpack_sizes: &[u64],
    digests: &[Option<u32>],
    db: &mut Database,
    warnings: &mut Warnings,
) -> Result<()> {
    let num_files = r.num()? as usize;
    let mut empty_stream: Vec<bool> = Vec::new();
    let mut empty_file: Vec<bool> = Vec::new();
    let mut anti: Vec<bool> = Vec::new();
    let mut num_empty_streams = 0usize;
    let mut names: Vec<Vec<u16>> = Vec::new();
    let mut attrib: Vec<Option<u32>> = Vec::new();
    let mut times: [Vec<Option<u64>>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    loop {
        let t = r.id()?;
        if t == id::END {
            break;
        }
        let size = r.number()?;
        if size > r.rem() as u64 {
            return Err(Error::Header);
        }
        let mut prop = Reader::new(r.bytes(size as usize)?);
        match t {
            id::NAME => {
                let mut ext = external(&mut prop, Some(data_vector))?;
                names = match ext.as_mut() {
                    Some(e) => read_names(e, num_files, warnings)?,
                    None => read_names(&mut prop, num_files, warnings)?,
                };
                if let Some(e) = ext.as_ref() {
                    done(e, warnings);
                }
            }
            id::WIN_ATTRIB => {
                let defs = prop.bool_vector2(num_files)?;
                let mut ext = external(&mut prop, Some(data_vector))?;
                attrib = match ext.as_mut() {
                    Some(e) => read_u32s(e, &defs)?,
                    None => read_u32s(&mut prop, &defs)?,
                };
                if let Some(e) = ext.as_ref() {
                    done(e, warnings);
                }
            }
            id::EMPTY_STREAM => {
                empty_stream = prop.bool_vector(num_files)?;
                num_empty_streams = empty_stream.iter().filter(|&&e| e).count();
                empty_file.clear();
                anti.clear();
            }
            id::EMPTY_FILE => empty_file = prop.bool_vector(num_empty_streams)?,
            id::ANTI => anti = prop.bool_vector(num_empty_streams)?,
            id::START_POS | id::CTIME | id::ATIME | id::MTIME => {
                let defs = prop.bool_vector2(num_files)?;
                let mut ext = external(&mut prop, Some(data_vector))?;
                let values = match ext.as_mut() {
                    Some(e) => read_u64s(e, &defs)?,
                    None => read_u64s(&mut prop, &defs)?,
                };
                if let Some(e) = ext.as_ref() {
                    done(e, warnings);
                }
                let slot = match t {
                    id::START_POS => 0,
                    id::CTIME => 1,
                    id::ATIME => 2,
                    _ => 3,
                };
                if let Some(v) = times.get_mut(slot) {
                    *v = values;
                }
            }
            id::DUMMY => {
                for _ in 0..size {
                    if prop.byte()? != 0 {
                        warnings.header = true;
                    }
                }
            }
            _ => {
                warnings.unsupported_feature = true;
                prop.pos = prop.buf.len();
            }
        }
        // A property not read to its end: 7-Zip's ThrowIncorrect.
        if prop.rem() != 0 {
            return Err(Error::Header);
        }
    }

    if num_files - num_empty_streams != unpack_sizes.len() {
        return Err(Error::Unsupported);
    }

    let [start_pos, ctime, atime, mtime] = times;
    let at = |v: &[Option<u64>], i: usize| v.get(i).copied().flatten();
    let mut files = Vec::with_capacity(num_files);
    let mut size_index = 0usize;
    let mut empty_index = 0usize;
    for i in 0..num_files {
        let mut file = FileRecord::default();
        if empty_stream.get(i).copied().unwrap_or(false) {
            file.has_stream = false;
            file.is_dir = !empty_file.get(empty_index).copied().unwrap_or(false);
            file.is_anti = anti.get(empty_index).copied().unwrap_or(false);
            empty_index += 1;
        } else {
            file.has_stream = true;
            file.size = unpack_sizes.get(size_index).copied().unwrap_or(0);
            file.crc = digests.get(size_index).copied().flatten();
            size_index += 1;
        }
        file.name = names.get(i).cloned().unwrap_or_default();
        file.attrib = attrib.get(i).copied().flatten();
        file.start_pos = at(&start_pos, i);
        file.ctime = at(&ctime, i);
        file.atime = at(&atime, i);
        file.mtime = at(&mtime, i);
        files.push(file);
    }
    db.files = files;
    Ok(())
}

/// `CDbEx::FillLinks`: which folder each file's data is in. Files left over
/// for no folder, or folders left over with files declared, are header
/// warnings (7-Zip 18.06 on); a file with data and no folder left is an error.
fn fill_links(db: &mut Database, warnings: &mut Warnings) -> Result<()> {
    let num_folders = db.folders.folders.len();
    let mut start = vec![0u32; num_folders];
    let mut map = Vec::with_capacity(db.files.len());
    let mut folder = 0usize;
    let mut in_folder = 0u32;
    let streams = |k: usize| db.folders.num_unpack_streams.get(k).copied().unwrap_or(0);
    for (i, file) in db.files.iter().enumerate() {
        let empty = !file.has_stream;
        if in_folder == 0 {
            if empty {
                map.push(None);
                continue;
            }
            loop {
                if folder >= num_folders {
                    return Err(Error::Header);
                }
                if let Some(s) = start.get_mut(folder) {
                    *s = i as u32;
                }
                if streams(folder) != 0 {
                    break;
                }
                folder += 1;
            }
        }
        map.push(Some(folder as u32));
        if empty {
            continue;
        }
        in_folder += 1;
        if in_folder >= streams(folder) {
            folder += 1;
            in_folder = 0;
        }
    }
    if in_folder != 0 {
        folder += 1;
        warnings.header = true;
    }
    while folder < num_folders {
        if let Some(s) = start.get_mut(folder) {
            *s = db.files.len() as u32;
        }
        if streams(folder) != 0 {
            warnings.header = true;
        }
        folder += 1;
    }
    db.folder_start_file = start;
    db.file_folder = map;
    Ok(())
}
