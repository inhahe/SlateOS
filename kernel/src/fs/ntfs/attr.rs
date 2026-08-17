//! NTFS attributes: the only thing an MFT record actually contains.
//!
//! NTFS has no inode with fixed fields. A file *is* a bag of typed,
//! optionally-named attributes — its name is a `$FILE_NAME` attribute, its
//! timestamps a `$STANDARD_INFORMATION`, its contents a `$DATA`, its
//! directory listing a `$INDEX_ROOT` plus `$INDEX_ALLOCATION`. Even the MFT
//! itself is a file whose `$DATA` is the MFT. Almost every structural
//! question in this driver therefore reduces to "find the attribute of type
//! T named N and read its value".
//!
//! ## Resident vs non-resident: the one distinction that matters
//!
//! A small attribute's value lives **inside** the MFT record (resident); a
//! large one lives in clusters elsewhere on disk, and the record holds only a
//! *runlist* describing where (non-resident). This is not an optimisation
//! detail the caller can ignore: a 100-byte file has no clusters allocated at
//! all, and a directory small enough to fit has no `$INDEX_ALLOCATION`. Code
//! that handles only one of the two forms works perfectly on whichever test
//! volume it was written against and fails on the other.
//!
//! ## Runlists
//!
//! A runlist is a sequence of variable-width entries. Each begins with a
//! header byte whose low nibble is the byte-width of a *length* field and
//! whose high nibble is the byte-width of an *offset* field; a zero header
//! terminates the list. The length is unsigned clusters; the offset is a
//! **signed delta** from the previous run's LCN, which is what lets a runlist
//! stay compact across a disk. A zero-width offset field means the run is
//! *sparse* — a hole, read as zeroes, occupying no clusters.
//!
//! The signed delta is the subtle part: it is sign-extended from its own
//! width, so a 1-byte offset of `0xFF` is `-1`, not `255`. Getting that wrong
//! yields runlists that appear to work until the first backward-seeking file.
//!
//! ## References
//!
//! - <https://flatcap.github.io/linux-ntfs/ntfs/concepts/attribute_header.html>
//! - <https://flatcap.github.io/linux-ntfs/ntfs/concepts/data_runs.html>
//! - Linux `fs/ntfs3/attrib.c`, `fs/ntfs3/run.c`

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::raw::{filetime_to_unix_ns, u8_at, u16_at, u32_at, u64_at, utf16le_at};

/// Attribute type codes.
///
/// Stored as a `u32` newtype rather than an enum: the set is open (Windows
/// has added types across versions), and an unknown type must be *skipped*,
/// not rejected — a volume written by a newer Windows must still be readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttributeType(pub u32);

impl AttributeType {
    /// `$STANDARD_INFORMATION` — timestamps, DOS attribute flags.
    pub const STANDARD_INFORMATION: Self = Self(0x10);
    /// `$ATTRIBUTE_LIST` — where to find attributes that did not fit.
    pub const ATTRIBUTE_LIST: Self = Self(0x20);
    /// `$FILE_NAME` — one name of the file, plus its parent directory.
    pub const FILE_NAME: Self = Self(0x30);
    /// `$OBJECT_ID`.
    pub const OBJECT_ID: Self = Self(0x40);
    /// `$SECURITY_DESCRIPTOR`.
    pub const SECURITY_DESCRIPTOR: Self = Self(0x50);
    /// `$VOLUME_NAME` — the volume label (on the `$Volume` record).
    pub const VOLUME_NAME: Self = Self(0x60);
    /// `$VOLUME_INFORMATION` — NTFS version and dirty flag.
    pub const VOLUME_INFORMATION: Self = Self(0x70);
    /// `$DATA` — file contents (unnamed) or an alternate stream (named).
    pub const DATA: Self = Self(0x80);
    /// `$INDEX_ROOT` — the resident root of a B+ tree index.
    pub const INDEX_ROOT: Self = Self(0x90);
    /// `$INDEX_ALLOCATION` — the non-resident nodes of a B+ tree index.
    pub const INDEX_ALLOCATION: Self = Self(0xA0);
    /// `$BITMAP` — allocation bitmap (for `$MFT` and for large indexes).
    pub const BITMAP: Self = Self(0xB0);
    /// `$REPARSE_POINT` — symlinks, junctions, mount points.
    pub const REPARSE_POINT: Self = Self(0xC0);
    /// Sentinel marking the end of a record's attribute list.
    pub const END: Self = Self(0xFFFF_FFFF);
}

/// Attribute flag: the data is compressed (LZNT1).
pub const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
/// Attribute flag: the data is encrypted (EFS).
pub const ATTR_FLAG_ENCRYPTED: u16 = 0x4000;
/// Attribute flag: the data is sparse.
pub const ATTR_FLAG_SPARSE: u16 = 0x8000;

/// Largest number of runs we will decode from one runlist.
///
/// A runlist is bounded by the attribute record that contains it, so this is
/// belt-and-braces against a self-referential decode bug rather than against
/// the disk; without it a zero-advance bug would spin forever holding the
/// filesystem lock.
const MAX_RUNS: usize = 65536;

/// Fields common to resident and non-resident attributes.
#[derive(Debug, Clone)]
pub struct AttributeHeader {
    /// Attribute type code.
    pub attr_type: AttributeType,
    /// Total length of the attribute record, in bytes.
    pub length: u32,
    /// Whether the value lives outside the MFT record.
    pub non_resident: bool,
    /// Attribute name (empty for the unnamed instance, `$I30` for a
    /// directory index, an alternate-stream name for a named `$DATA`).
    pub name: String,
    /// Attribute flags (compressed / encrypted / sparse).
    pub flags: u16,
    /// Instance id, unique within the base record and its extensions.
    pub id: u16,
}

impl AttributeHeader {
    /// Whether the attribute's data is compressed.
    pub fn is_compressed(&self) -> bool {
        self.flags & ATTR_FLAG_COMPRESSED != 0
    }

    /// Whether the attribute's data is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.flags & ATTR_FLAG_ENCRYPTED != 0
    }

    /// Whether the attribute's data is sparse.
    pub fn is_sparse(&self) -> bool {
        self.flags & ATTR_FLAG_SPARSE != 0
    }
}

/// One extent of a non-resident attribute's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRun {
    /// First virtual cluster this run covers.
    pub vcn: u64,
    /// First logical cluster on disk, or `None` for a sparse run (a hole).
    pub lcn: Option<u64>,
    /// Length of the run, in clusters.
    pub length: u64,
}

/// The parts of an attribute that differ between the two storage forms.
#[derive(Debug, Clone)]
pub enum AttributeBody {
    /// Value stored inline in the MFT record.
    Resident {
        /// The value bytes.
        value: Vec<u8>,
        /// Whether the attribute is indexed.
        indexed: bool,
    },
    /// Value stored in clusters described by a runlist.
    NonResident {
        /// First VCN this attribute record covers.
        start_vcn: u64,
        /// Last VCN this attribute record covers (inclusive).
        last_vcn: u64,
        /// Bytes allocated on disk.
        allocated_size: u64,
        /// Logical size of the data.
        data_size: u64,
        /// Bytes that have actually been written; the remainder up to
        /// `data_size` reads as zeroes.
        initialized_size: u64,
        /// Compression unit, as a power-of-two cluster count (0 = none).
        compression_unit: u16,
        /// Decoded runlist.
        runs: Vec<DataRun>,
    },
}

/// A parsed attribute.
#[derive(Debug, Clone)]
pub struct Attribute {
    /// Common header fields.
    pub header: AttributeHeader,
    /// Form-specific fields.
    pub body: AttributeBody,
}

impl Attribute {
    /// The resident value, if this attribute is resident.
    pub fn resident_value(&self) -> Option<&[u8]> {
        match &self.body {
            AttributeBody::Resident { value, .. } => Some(value),
            AttributeBody::NonResident { .. } => None,
        }
    }

    /// The logical size of the attribute's data, whichever form it takes.
    pub fn data_size(&self) -> u64 {
        match &self.body {
            AttributeBody::Resident { value, .. } => value.len() as u64,
            AttributeBody::NonResident { data_size, .. } => *data_size,
        }
    }
}

/// Parse every attribute in a record's attribute region.
///
/// Stops at the `0xFFFFFFFF` terminator or at the end of `region`.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if an attribute record is malformed or has
/// a length that would not advance the cursor (which would otherwise loop
/// forever).
pub fn parse_attributes(region: &[u8]) -> KernelResult<Vec<Attribute>> {
    let mut out = Vec::new();
    let mut offset = 0usize;

    // A terminator needs only 4 bytes; anything shorter ends the list.
    while let Some(type_code) = u32_at(region, offset) {
        if type_code == AttributeType::END.0 {
            break;
        }

        let length = u32_at(region, offset.saturating_add(4)).ok_or(KernelError::CorruptedData)?;
        let length = usize::try_from(length).map_err(|_| KernelError::CorruptedData)?;

        // A zero or unaligned length cannot be walked past.
        if length == 0 || length % 8 != 0 {
            return Err(KernelError::CorruptedData);
        }
        let end = offset
            .checked_add(length)
            .ok_or(KernelError::CorruptedData)?;
        if end > region.len() {
            return Err(KernelError::CorruptedData);
        }

        let record = region.get(offset..end).ok_or(KernelError::CorruptedData)?;
        out.push(parse_attribute(record)?);

        offset = end;
    }

    Ok(out)
}

/// Parse a single attribute record (`record.len()` is the record's length).
fn parse_attribute(record: &[u8]) -> KernelResult<Attribute> {
    let attr_type = AttributeType(u32_at(record, 0).ok_or(KernelError::CorruptedData)?);
    let length = u32_at(record, 4).ok_or(KernelError::CorruptedData)?;
    let non_resident = u8_at(record, 8).ok_or(KernelError::CorruptedData)? != 0;
    let name_len = usize::from(u8_at(record, 9).ok_or(KernelError::CorruptedData)?);
    let name_offset = usize::from(u16_at(record, 0x0A).ok_or(KernelError::CorruptedData)?);
    let flags = u16_at(record, 0x0C).ok_or(KernelError::CorruptedData)?;
    let id = u16_at(record, 0x0E).ok_or(KernelError::CorruptedData)?;

    let name = if name_len == 0 {
        String::new()
    } else {
        utf16le_at(record, name_offset, name_len).ok_or(KernelError::CorruptedData)?
    };

    let header = AttributeHeader {
        attr_type,
        length,
        non_resident,
        name,
        flags,
        id,
    };

    let body = if non_resident {
        parse_non_resident_body(record)?
    } else {
        parse_resident_body(record)?
    };

    Ok(Attribute { header, body })
}

/// Parse the resident-specific portion of an attribute record.
fn parse_resident_body(record: &[u8]) -> KernelResult<AttributeBody> {
    let value_len = usize::try_from(u32_at(record, 0x10).ok_or(KernelError::CorruptedData)?)
        .map_err(|_| KernelError::CorruptedData)?;
    let value_off = usize::from(u16_at(record, 0x14).ok_or(KernelError::CorruptedData)?);
    let indexed = u8_at(record, 0x16).ok_or(KernelError::CorruptedData)? != 0;

    let end = value_off
        .checked_add(value_len)
        .ok_or(KernelError::CorruptedData)?;
    let value = record
        .get(value_off..end)
        .ok_or(KernelError::CorruptedData)?
        .to_vec();

    Ok(AttributeBody::Resident { value, indexed })
}

/// Parse the non-resident-specific portion of an attribute record.
fn parse_non_resident_body(record: &[u8]) -> KernelResult<AttributeBody> {
    let start_vcn = u64_at(record, 0x10).ok_or(KernelError::CorruptedData)?;
    let last_vcn = u64_at(record, 0x18).ok_or(KernelError::CorruptedData)?;
    let runs_off = usize::from(u16_at(record, 0x20).ok_or(KernelError::CorruptedData)?);
    let compression_unit = u16_at(record, 0x22).ok_or(KernelError::CorruptedData)?;
    let allocated_size = u64_at(record, 0x28).ok_or(KernelError::CorruptedData)?;
    let data_size = u64_at(record, 0x30).ok_or(KernelError::CorruptedData)?;
    let initialized_size = u64_at(record, 0x38).ok_or(KernelError::CorruptedData)?;

    let runlist = record.get(runs_off..).ok_or(KernelError::CorruptedData)?;
    let runs = decode_runlist(runlist, start_vcn)?;

    Ok(AttributeBody::NonResident {
        start_vcn,
        last_vcn,
        allocated_size,
        data_size,
        initialized_size,
        compression_unit,
        runs,
    })
}

/// Decode a runlist into absolute (VCN, LCN, length) runs.
///
/// `start_vcn` is the attribute record's first VCN; the runlist's lengths are
/// relative to it, and its offsets are deltas relative to the previous run's
/// LCN (starting from 0).
///
/// # Errors
///
/// [`KernelError::CorruptedData`] for a malformed entry, a run that overflows
/// the VCN space, or a negative resulting LCN.
pub fn decode_runlist(data: &[u8], start_vcn: u64) -> KernelResult<Vec<DataRun>> {
    let mut runs = Vec::new();
    let mut offset = 0usize;
    let mut vcn = start_vcn;
    let mut prev_lcn: i64 = 0;

    loop {
        let header = match u8_at(data, offset) {
            Some(0) | None => break,
            Some(h) => h,
        };
        if runs.len() >= MAX_RUNS {
            return Err(KernelError::CorruptedData);
        }

        let len_size = usize::from(header & 0x0F);
        let off_size = usize::from(header >> 4);

        // A length field of zero width would make the run zero-length and
        // the cursor advance by one byte forever.
        if len_size == 0 || len_size > 8 || off_size > 8 {
            return Err(KernelError::CorruptedData);
        }

        let len_start = offset.checked_add(1).ok_or(KernelError::CorruptedData)?;
        let length =
            read_unsigned_le(data, len_start, len_size).ok_or(KernelError::CorruptedData)?;
        if length == 0 {
            return Err(KernelError::CorruptedData);
        }

        let off_start = len_start
            .checked_add(len_size)
            .ok_or(KernelError::CorruptedData)?;

        let lcn = if off_size == 0 {
            // Sparse run: no clusters allocated, reads as zeroes. The
            // previous LCN is *not* updated — a hole does not move the
            // cursor across the disk.
            None
        } else {
            let delta =
                read_signed_le(data, off_start, off_size).ok_or(KernelError::CorruptedData)?;
            let abs = prev_lcn
                .checked_add(delta)
                .ok_or(KernelError::CorruptedData)?;
            if abs < 0 {
                return Err(KernelError::CorruptedData);
            }
            prev_lcn = abs;
            Some(u64::try_from(abs).map_err(|_| KernelError::CorruptedData)?)
        };

        runs.push(DataRun { vcn, lcn, length });

        vcn = vcn.checked_add(length).ok_or(KernelError::CorruptedData)?;
        offset = off_start
            .checked_add(off_size)
            .ok_or(KernelError::CorruptedData)?;
    }

    Ok(runs)
}

/// Read a `size`-byte little-endian unsigned integer.
fn read_unsigned_le(data: &[u8], offset: usize, size: usize) -> Option<u64> {
    if size == 0 || size > 8 {
        return None;
    }
    let end = offset.checked_add(size)?;
    let slice = data.get(offset..end)?;
    let mut value = 0u64;
    for (i, byte) in slice.iter().enumerate() {
        let shift = u32::try_from(i).ok()?.checked_mul(8)?;
        value |= u64::from(*byte).checked_shl(shift)?;
    }
    Some(value)
}

/// Read a `size`-byte little-endian **sign-extended** integer.
///
/// The sign bit is the top bit of the highest byte *present*, not of a
/// 64-bit value: a 1-byte `0xFF` is `-1`.
fn read_signed_le(data: &[u8], offset: usize, size: usize) -> Option<i64> {
    let raw = read_unsigned_le(data, offset, size)?;
    let bits = u32::try_from(size).ok()?.checked_mul(8)?;
    if bits >= 64 {
        #[allow(clippy::cast_possible_wrap)] // Full width: the value *is* i64.
        return Some(raw as i64);
    }
    let sign_bit = 1u64.checked_shl(bits.checked_sub(1)?)?;
    #[allow(clippy::cast_possible_wrap)] // Two's-complement sign extension.
    if raw & sign_bit != 0 {
        // Sign-extend: set all bits above the field width.
        let mask = u64::MAX.checked_shl(bits)?;
        Some((raw | mask) as i64)
    } else {
        Some(raw as i64)
    }
}

// ---------------------------------------------------------------------------
// $STANDARD_INFORMATION
// ---------------------------------------------------------------------------

/// The timestamps and DOS attribute bits every file carries.
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardInformation {
    /// Creation time, ns since the Unix epoch.
    pub created_ns: u64,
    /// Last data modification, ns since the Unix epoch.
    pub modified_ns: u64,
    /// Last MFT-record change, ns since the Unix epoch.
    pub changed_ns: u64,
    /// Last access, ns since the Unix epoch.
    pub accessed_ns: u64,
    /// DOS file attribute bits (read-only, hidden, system, …).
    pub dos_flags: u32,
}

/// DOS attribute bit: read-only.
pub const DOS_READONLY: u32 = 0x0001;
/// DOS attribute bit: hidden.
pub const DOS_HIDDEN: u32 = 0x0002;
/// DOS attribute bit: system.
pub const DOS_SYSTEM: u32 = 0x0004;
/// DOS attribute bit: directory.
pub const DOS_DIRECTORY: u32 = 0x1000_0000;

impl StandardInformation {
    /// Parse a `$STANDARD_INFORMATION` value.
    ///
    /// # Errors
    ///
    /// [`KernelError::CorruptedData`] if the value is shorter than the
    /// 48-byte NTFS 1.2 form.
    pub fn parse(value: &[u8]) -> KernelResult<Self> {
        Ok(Self {
            created_ns: filetime_to_unix_ns(u64_at(value, 0x00).ok_or(KernelError::CorruptedData)?),
            modified_ns: filetime_to_unix_ns(
                u64_at(value, 0x08).ok_or(KernelError::CorruptedData)?,
            ),
            changed_ns: filetime_to_unix_ns(u64_at(value, 0x10).ok_or(KernelError::CorruptedData)?),
            accessed_ns: filetime_to_unix_ns(
                u64_at(value, 0x18).ok_or(KernelError::CorruptedData)?,
            ),
            dos_flags: u32_at(value, 0x20).ok_or(KernelError::CorruptedData)?,
        })
    }
}

// ---------------------------------------------------------------------------
// $FILE_NAME
// ---------------------------------------------------------------------------

/// Namespace of a `$FILE_NAME` attribute.
///
/// A file normally has **two** names — a long Win32 name and a short
/// 8.3 "DOS" name — stored as two separate `$FILE_NAME` attributes and two
/// separate index entries. Listing a directory without filtering therefore
/// shows every file twice, once as `LongFileName.txt` and once as
/// `LONGFI~1.TXT`. That is the single most visible NTFS-parser bug there is,
/// and the fix is to hide `Dos`-namespace names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSpace {
    /// Case-sensitive, any character except `/` and NUL.
    Posix,
    /// The long name.
    Win32,
    /// The short 8.3 alias.
    Dos,
    /// A name that is simultaneously a valid long and short name, so only
    /// one attribute exists.
    Win32AndDos,
    /// Something a future Windows invented.
    Unknown(u8),
}

impl NameSpace {
    /// Decode the namespace byte.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Posix,
            1 => Self::Win32,
            2 => Self::Dos,
            3 => Self::Win32AndDos,
            other => Self::Unknown(other),
        }
    }

    /// Whether a name in this namespace should appear in a directory
    /// listing. See the type-level note: DOS aliases must not.
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::Dos)
    }
}

/// A parsed `$FILE_NAME` attribute.
#[derive(Debug, Clone)]
pub struct FileNameAttr {
    /// MFT reference of the directory containing this name.
    pub parent: u64,
    /// Creation time, ns since the Unix epoch.
    pub created_ns: u64,
    /// Modification time, ns since the Unix epoch.
    pub modified_ns: u64,
    /// MFT-record change time, ns since the Unix epoch.
    pub changed_ns: u64,
    /// Access time, ns since the Unix epoch.
    pub accessed_ns: u64,
    /// Allocated size of the file's unnamed `$DATA`.
    pub allocated_size: u64,
    /// Logical size of the file's unnamed `$DATA`.
    ///
    /// Note this is a *cached* copy that NTFS updates lazily; the `$DATA`
    /// attribute itself is authoritative. It is used for directory listings
    /// (where it saves reading every child's MFT record) but never for a
    /// read.
    pub data_size: u64,
    /// DOS attribute flags.
    pub dos_flags: u32,
    /// Which namespace this name belongs to.
    pub namespace: NameSpace,
    /// The name itself.
    pub name: String,
}

/// Minimum size of a `$FILE_NAME` value (header + zero-length name).
pub const FILE_NAME_HEADER_LEN: usize = 0x42;

impl FileNameAttr {
    /// Parse a `$FILE_NAME` value.
    ///
    /// # Errors
    ///
    /// [`KernelError::CorruptedData`] if the value is truncated.
    pub fn parse(value: &[u8]) -> KernelResult<Self> {
        let parent = u64_at(value, 0x00).ok_or(KernelError::CorruptedData)?;
        let created_ns =
            filetime_to_unix_ns(u64_at(value, 0x08).ok_or(KernelError::CorruptedData)?);
        let modified_ns =
            filetime_to_unix_ns(u64_at(value, 0x10).ok_or(KernelError::CorruptedData)?);
        let changed_ns =
            filetime_to_unix_ns(u64_at(value, 0x18).ok_or(KernelError::CorruptedData)?);
        let accessed_ns =
            filetime_to_unix_ns(u64_at(value, 0x20).ok_or(KernelError::CorruptedData)?);
        let allocated_size = u64_at(value, 0x28).ok_or(KernelError::CorruptedData)?;
        let data_size = u64_at(value, 0x30).ok_or(KernelError::CorruptedData)?;
        let dos_flags = u32_at(value, 0x38).ok_or(KernelError::CorruptedData)?;
        let name_len = usize::from(u8_at(value, 0x40).ok_or(KernelError::CorruptedData)?);
        let namespace = NameSpace::from_raw(u8_at(value, 0x41).ok_or(KernelError::CorruptedData)?);
        let name =
            utf16le_at(value, FILE_NAME_HEADER_LEN, name_len).ok_or(KernelError::CorruptedData)?;

        Ok(Self {
            parent,
            created_ns,
            modified_ns,
            changed_ns,
            accessed_ns,
            allocated_size,
            data_size,
            dos_flags,
            namespace,
            name,
        })
    }
}

// ---------------------------------------------------------------------------
// $ATTRIBUTE_LIST
// ---------------------------------------------------------------------------

/// One entry of an `$ATTRIBUTE_LIST`: "attribute X lives in record Y".
///
/// A file acquires one of these when its attributes outgrow a single 1 KiB
/// MFT record — a heavily fragmented file whose runlist alone exceeds the
/// record, or a directory with many alternate streams. Ignoring it does not
/// produce an error; it produces a file that reads as empty, because its
/// `$DATA` is simply not in the base record. That silence is why this is
/// implemented rather than deferred.
#[derive(Debug, Clone)]
pub struct AttributeListEntry {
    /// Type of the referenced attribute.
    pub attr_type: AttributeType,
    /// First VCN the referenced attribute record covers.
    pub start_vcn: u64,
    /// MFT reference of the record holding it.
    pub reference: u64,
    /// Instance id of the referenced attribute.
    pub id: u16,
    /// Name of the referenced attribute.
    pub name: String,
}

/// Parse an `$ATTRIBUTE_LIST` value into its entries.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] for an entry whose length would not advance
/// the cursor or which runs past the end of the value.
pub fn parse_attribute_list(value: &[u8]) -> KernelResult<Vec<AttributeListEntry>> {
    let mut out = Vec::new();
    let mut offset = 0usize;

    while offset < value.len() {
        let Some(type_code) = u32_at(value, offset) else {
            break;
        };
        if type_code == AttributeType::END.0 {
            break;
        }

        let entry_len =
            usize::from(u16_at(value, offset.saturating_add(4)).ok_or(KernelError::CorruptedData)?);
        if entry_len < 0x18 {
            return Err(KernelError::CorruptedData);
        }
        let end = offset
            .checked_add(entry_len)
            .ok_or(KernelError::CorruptedData)?;
        if end > value.len() {
            return Err(KernelError::CorruptedData);
        }
        let entry = value.get(offset..end).ok_or(KernelError::CorruptedData)?;

        let name_len = usize::from(u8_at(entry, 6).ok_or(KernelError::CorruptedData)?);
        let name_off = usize::from(u8_at(entry, 7).ok_or(KernelError::CorruptedData)?);
        let start_vcn = u64_at(entry, 8).ok_or(KernelError::CorruptedData)?;
        let reference = u64_at(entry, 0x10).ok_or(KernelError::CorruptedData)?;
        let id = u16_at(entry, 0x18).unwrap_or(0);

        let name = if name_len == 0 {
            String::new()
        } else {
            utf16le_at(entry, name_off, name_len).ok_or(KernelError::CorruptedData)?
        };

        out.push(AttributeListEntry {
            attr_type: AttributeType(type_code),
            start_vcn,
            reference,
            id,
            name,
        });

        offset = end;
    }

    Ok(out)
}
