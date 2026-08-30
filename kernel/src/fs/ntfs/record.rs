//! MFT file records, and the update-sequence fixups every NTFS
//! multi-sector structure carries.
//!
//! ## Why fixups exist, and why they are not optional
//!
//! An MFT record (1024 bytes) and an index buffer (typically 4096) both span
//! several 512-byte sectors, so a power loss mid-write can leave a structure
//! whose first sector is new and whose last sector is old — *torn*, but with
//! every individual field still self-consistent. NTFS detects that by
//! stealing the **last two bytes of every sector** and writing one shared
//! "update sequence number" (USN) into all of them. The displaced originals
//! live in the update sequence array (USA) in the record header. On read, if
//! any sector's last two bytes disagree with the USN, that sector was not
//! part of the same write and the structure is torn.
//!
//! Two consequences the parser must respect:
//!
//! 1. **The check is the only torn-write detector NTFS has.** Skipping it
//!    does not make the driver more permissive, it makes it silently return
//!    half-old data. So a mismatch is [`KernelError::CorruptedData`], never a
//!    warning.
//! 2. **Fixups must be undone before any other field is read.** The two
//!    stolen bytes are at a fixed position inside each sector, which lands in
//!    the *middle* of whatever structure occupies that region — an attribute
//!    length, half a filename, anything. A parser that reads a field before
//!    applying fixups reads a value that is correct except for two bytes,
//!    which is the worst kind of wrong.
//!
//! ## Record layout
//!
//! ```text
//! 0x00  4  magic "FILE"
//! 0x04  2  offset to the update sequence array
//! 0x06  2  count of u16 entries in the USA (1 USN + one per sector)
//! 0x08  8  $LogFile sequence number
//! 0x10  2  sequence number (bumped on each reuse of this record)
//! 0x12  2  hard link count
//! 0x14  2  offset to the first attribute
//! 0x16  2  flags (0x01 in use, 0x02 directory)
//! 0x18  4  bytes used
//! 0x1C  4  bytes allocated
//! 0x20  8  base record reference (0 unless this is an extension record)
//! 0x28  2  next attribute id
//! 0x2C  4  this record's own number (NTFS 3.1+)
//! ```
//!
//! ## References
//!
//! - <https://flatcap.github.io/linux-ntfs/ntfs/concepts/file_record.html>
//! - Linux `fs/ntfs3/record.c`

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::attr::{Attribute, AttributeType, parse_attributes};
use super::raw::{u16_at, u32_at, u64_at};

/// Magic at the start of an MFT file record.
pub const FILE_MAGIC: &[u8; 4] = b"FILE";

/// Record flag: the record describes a file that exists (as opposed to a
/// deleted one whose slot has not been reused).
pub const FLAG_IN_USE: u16 = 0x0001;

/// Record flag: the record describes a directory.
pub const FLAG_DIRECTORY: u16 = 0x0002;

/// MFT record number of the volume root directory. Fixed by the format.
pub const ROOT_RECORD: u64 = 5;

/// Extract the record number from a 64-bit MFT reference.
///
/// The low 48 bits are the record number; the high 16 are a sequence number
/// used to detect a reference to a since-deleted file. We keep the two apart
/// rather than masking at each use site, because a reference used unmasked as
/// an index reads as an absurdly large record number and produces a confusing
/// out-of-range error instead of an obvious one.
pub fn mft_ref_record(reference: u64) -> u64 {
    reference & 0x0000_FFFF_FFFF_FFFF
}

/// Extract the sequence number from a 64-bit MFT reference.
pub fn mft_ref_sequence(reference: u64) -> u16 {
    #[allow(clippy::cast_possible_truncation)] // Deliberate: the top 16 bits.
    {
        (reference >> 48) as u16
    }
}

/// Undo the update-sequence fixups in a multi-sector structure, in place.
///
/// `magic` is the expected 4-byte signature (`FILE` or `INDX`).
/// `bytes_per_sector` comes from the boot sector.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the magic is wrong, the USA is
/// malformed, or any sector's tail does not carry the USN (a torn write).
pub fn apply_fixups(buf: &mut [u8], magic: &[u8; 4], bytes_per_sector: u32) -> KernelResult<()> {
    if buf.get(0..4) != Some(magic.as_slice()) {
        return Err(KernelError::CorruptedData);
    }

    let sector = usize::try_from(bytes_per_sector).map_err(|_| KernelError::CorruptedData)?;
    if sector < 4 || !buf.len().is_multiple_of(sector) {
        return Err(KernelError::CorruptedData);
    }
    let sectors = buf
        .len()
        .checked_div(sector)
        .ok_or(KernelError::CorruptedData)?;

    let usa_offset = usize::from(u16_at(buf, 0x04).ok_or(KernelError::CorruptedData)?);
    let usa_count = usize::from(u16_at(buf, 0x06).ok_or(KernelError::CorruptedData)?);

    // usa_count counts the USN plus one entry per sector.
    let fixup_entries = usa_count.checked_sub(1).ok_or(KernelError::CorruptedData)?;
    if fixup_entries != sectors {
        return Err(KernelError::CorruptedData);
    }

    // The USA must lie entirely within the structure, and must not overlap
    // the region the fixups themselves protect in a way that would make the
    // read order matter.
    let usa_end = usa_offset
        .checked_add(usa_count.checked_mul(2).ok_or(KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;
    if usa_end > buf.len() {
        return Err(KernelError::CorruptedData);
    }

    let usn = u16_at(buf, usa_offset).ok_or(KernelError::CorruptedData)?;

    for i in 0..sectors {
        // Last two bytes of sector i.
        let tail = i
            .checked_add(1)
            .and_then(|n| n.checked_mul(sector))
            .and_then(|n| n.checked_sub(2))
            .ok_or(KernelError::CorruptedData)?;

        let present = u16_at(buf, tail).ok_or(KernelError::CorruptedData)?;
        if present != usn {
            // Torn write: this sector was not written by the same
            // transaction as the record header.
            return Err(KernelError::CorruptedData);
        }

        let entry_off = usa_offset
            .checked_add(
                i.checked_add(1)
                    .and_then(|n| n.checked_mul(2))
                    .ok_or(KernelError::CorruptedData)?,
            )
            .ok_or(KernelError::CorruptedData)?;
        let original = u16_at(buf, entry_off).ok_or(KernelError::CorruptedData)?;

        let bytes = original.to_le_bytes();
        let dst = buf
            .get_mut(tail..tail.saturating_add(2))
            .ok_or(KernelError::CorruptedData)?;
        dst.copy_from_slice(&bytes);
    }

    Ok(())
}

/// A parsed MFT file record.
pub struct FileRecord {
    /// The record's own number, if the format version records it.
    pub record_number: u64,
    /// Sequence number (incremented each time the slot is reused).
    pub sequence: u16,
    /// Hard link count.
    pub hard_links: u16,
    /// Record flags (see [`FLAG_IN_USE`], [`FLAG_DIRECTORY`]).
    pub flags: u16,
    /// Base record reference — zero for a base record, non-zero for an
    /// extension record that holds overflow attributes.
    pub base_reference: u64,
    /// All attributes in this record, in on-disk order.
    pub attributes: Vec<Attribute>,
}

impl FileRecord {
    /// Parse a record whose fixups have **already** been applied.
    ///
    /// # Errors
    ///
    /// [`KernelError::CorruptedData`] for a malformed header or attribute
    /// list.
    pub fn parse(buf: &[u8], record_number: u64) -> KernelResult<Self> {
        if buf.get(0..4) != Some(FILE_MAGIC.as_slice()) {
            return Err(KernelError::CorruptedData);
        }

        let sequence = u16_at(buf, 0x10).ok_or(KernelError::CorruptedData)?;
        let hard_links = u16_at(buf, 0x12).ok_or(KernelError::CorruptedData)?;
        let attrs_offset = usize::from(u16_at(buf, 0x14).ok_or(KernelError::CorruptedData)?);
        let flags = u16_at(buf, 0x16).ok_or(KernelError::CorruptedData)?;
        let bytes_used =
            usize::try_from(u32_at(buf, 0x18).ok_or(KernelError::CorruptedData)?).unwrap_or(0);
        let base_reference = u64_at(buf, 0x20).ok_or(KernelError::CorruptedData)?;

        if attrs_offset >= buf.len() {
            return Err(KernelError::CorruptedData);
        }

        // `bytes_used` bounds the attribute region. Trust it only as far as
        // the buffer: a record claiming to use more than it allocates is
        // corrupt, and clamping (rather than rejecting) keeps a single bad
        // record from making the rest of the volume unreadable.
        let end = if bytes_used > attrs_offset && bytes_used <= buf.len() {
            bytes_used
        } else {
            buf.len()
        };

        let region = buf
            .get(attrs_offset..end)
            .ok_or(KernelError::CorruptedData)?;
        let attributes = parse_attributes(region)?;

        Ok(Self {
            record_number,
            sequence,
            hard_links,
            flags,
            base_reference,
            attributes,
        })
    }

    /// Whether the record describes a live file (as opposed to a deleted
    /// slot).
    pub fn is_in_use(&self) -> bool {
        self.flags & FLAG_IN_USE != 0
    }

    /// Whether the record describes a directory.
    pub fn is_directory(&self) -> bool {
        self.flags & FLAG_DIRECTORY != 0
    }

    /// Whether this is an extension record holding another file's overflow
    /// attributes.
    pub fn is_extension(&self) -> bool {
        self.base_reference != 0
    }

    /// The first attribute of the given type with the given name.
    ///
    /// `name` is compared case-**sensitively**. NTFS attribute names
    /// (`$I30`, `$SDH`, …) are fixed identifiers from the format, not user
    /// data, so there is no case-folding question here; user-visible
    /// filenames are a separate matter handled in `index.rs`.
    pub fn find_attribute(&self, ty: AttributeType, name: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|a| a.header.attr_type == ty && a.header.name == name)
    }

    /// All attributes of the given type with the given name, in order.
    ///
    /// A large non-resident attribute can be split across several records;
    /// each piece is a separate attribute covering a distinct VCN range.
    pub fn find_attributes(
        &self,
        ty: AttributeType,
        name: &str,
    ) -> impl Iterator<Item = &Attribute> {
        self.attributes
            .iter()
            .filter(move |a| a.header.attr_type == ty && a.header.name == name)
    }
}
