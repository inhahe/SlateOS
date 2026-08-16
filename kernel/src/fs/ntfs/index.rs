//! NTFS directory indexes: the B+ tree that makes a directory a directory.
//!
//! A directory's children are not a list. They are a B+ tree keyed on the
//! filename, whose root is a resident `$INDEX_ROOT` attribute named `$I30`
//! and whose interior and leaf nodes — if the tree outgrows the MFT record —
//! live in a non-resident `$INDEX_ALLOCATION` attribute of the same name, as
//! fixed-size `INDX` blocks addressed by VCN.
//!
//! ## The trap: a small directory has no `$INDEX_ALLOCATION` at all
//!
//! Below roughly a dozen entries the whole tree fits inside `$INDEX_ROOT` and
//! there is no `$INDEX_ALLOCATION` attribute on the record. Above it, the
//! root usually holds *almost nothing* — often a single "end" entry whose
//! only content is a pointer to a child node — and every real name is out in
//! the `INDX` blocks. A driver that reads only `$INDEX_ROOT` therefore lists
//! small directories perfectly and large ones as empty; a driver that assumes
//! `$INDEX_ALLOCATION` exists fails on every small one. Both halves are
//! mandatory, which is why `tests.rs` synthesises a volume containing one of
//! each.
//!
//! ## Node layout
//!
//! Every node — the one inside `$INDEX_ROOT` and every `INDX` block — begins
//! with the same 16-byte header, whose offsets are relative to **the header
//! itself**, not to the start of the attribute or block:
//!
//! ```text
//! 0x00  4  offset to the first entry
//! 0x04  4  total size of the entries
//! 0x08  4  allocated size of the node
//! 0x0C  4  flags (bit 0: this node has children)
//! ```
//!
//! ## Entry layout
//!
//! ```text
//! 0x00  8  MFT reference of the indexed file
//! 0x08  2  length of this entry
//! 0x0A  2  length of the key ($FILE_NAME) that follows
//! 0x0C  2  flags (bit 0: has a subnode, bit 1: is the last entry)
//! 0x10  …  the key
//! …        (last 8 bytes of the entry, if it has a subnode: child VCN)
//! ```
//!
//! The last entry of every node carries no key — it is a terminator whose
//! purpose is to hold the rightmost child pointer. Treating it as a file is
//! how a phantom zero-length entry appears in a listing.
//!
//! ## References
//!
//! - <https://flatcap.github.io/linux-ntfs/ntfs/concepts/index_record.html>
//! - Linux `fs/ntfs3/index.c`

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::attr::FileNameAttr;
use super::raw::{u16_at, u32_at, u64_at};

/// Magic at the start of an index allocation block.
pub const INDX_MAGIC: &[u8; 4] = b"INDX";

/// The name every directory index carries.
pub const DIR_INDEX_NAME: &str = "$I30";

/// Index entry flag: the entry points at a child node.
pub const ENTRY_HAS_SUBNODE: u16 = 0x0001;
/// Index entry flag: the entry is the node's terminator.
pub const ENTRY_IS_LAST: u16 = 0x0002;

/// Node header flag: this node has children.
pub const NODE_HAS_CHILDREN: u32 = 0x0001;

/// Size of the node header.
pub const NODE_HEADER_LEN: usize = 0x10;

/// Offset of the node header within an `$INDEX_ROOT` value.
pub const INDEX_ROOT_NODE_OFFSET: usize = 0x10;

/// Offset of the node header within an `INDX` block.
pub const INDX_NODE_OFFSET: usize = 0x18;

/// Minimum length of an index entry (header only, no key, no child VCN).
const MIN_ENTRY_LEN: usize = 0x10;

/// The fixed header of an `$INDEX_ROOT` value.
#[derive(Debug, Clone, Copy)]
pub struct IndexRoot {
    /// Attribute type that this index is keyed on (`0x30` = `$FILE_NAME`
    /// for a directory).
    pub indexed_type: u32,
    /// Collation rule (`1` = filename, case-insensitive Unicode).
    pub collation: u32,
    /// Size in bytes of each `INDX` block in the matching
    /// `$INDEX_ALLOCATION`.
    pub index_block_size: u32,
    /// Clusters per `INDX` block, or (when a block is smaller than a
    /// cluster) the log2 of its size in bytes.
    pub clusters_per_block: u8,
}

impl IndexRoot {
    /// Parse the fixed header of an `$INDEX_ROOT` value.
    ///
    /// # Errors
    ///
    /// [`KernelError::CorruptedData`] if the value is truncated.
    pub fn parse(value: &[u8]) -> KernelResult<Self> {
        Ok(Self {
            indexed_type: u32_at(value, 0x00).ok_or(KernelError::CorruptedData)?,
            collation: u32_at(value, 0x04).ok_or(KernelError::CorruptedData)?,
            index_block_size: u32_at(value, 0x08).ok_or(KernelError::CorruptedData)?,
            clusters_per_block: super::raw::u8_at(value, 0x0C).ok_or(KernelError::CorruptedData)?,
        })
    }
}

/// A parsed index node header.
#[derive(Debug, Clone, Copy)]
pub struct NodeHeader {
    /// Offset of the first entry, relative to the header.
    pub entries_offset: u32,
    /// Total size of the entries, relative to the header.
    pub entries_size: u32,
    /// Allocated size of the node, relative to the header.
    pub allocated_size: u32,
    /// Node flags.
    pub flags: u32,
}

impl NodeHeader {
    /// Parse a node header from `buf` at `offset`.
    ///
    /// # Errors
    ///
    /// [`KernelError::CorruptedData`] if the header is truncated.
    pub fn parse(buf: &[u8], offset: usize) -> KernelResult<Self> {
        Ok(Self {
            entries_offset: u32_at(buf, offset).ok_or(KernelError::CorruptedData)?,
            entries_size: u32_at(buf, offset.saturating_add(4)).ok_or(KernelError::CorruptedData)?,
            allocated_size: u32_at(buf, offset.saturating_add(8))
                .ok_or(KernelError::CorruptedData)?,
            flags: u32_at(buf, offset.saturating_add(12)).ok_or(KernelError::CorruptedData)?,
        })
    }

    /// Whether entries in this node may point at child nodes.
    pub fn has_children(&self) -> bool {
        self.flags & NODE_HAS_CHILDREN != 0
    }
}

/// One entry of a directory index.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// MFT reference of the file this entry names.
    pub reference: u64,
    /// The `$FILE_NAME` key, or `None` for the node's terminator entry.
    pub key: Option<FileNameAttr>,
    /// VCN of the child node, if this entry has one.
    pub child_vcn: Option<u64>,
    /// Whether this is the node's terminator entry.
    pub is_last: bool,
}

/// Parse every entry of the node whose header sits at `node_offset`.
///
/// Entries are returned in on-disk (collation) order, including the
/// terminator entry — callers need it, because it carries the rightmost
/// child pointer.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if the node header, an entry length, or a
/// key is malformed. An entry whose declared length would not advance the
/// cursor is rejected rather than skipped: silently advancing past it would
/// resynchronise on arbitrary bytes and invent entries.
pub fn parse_node(buf: &[u8], node_offset: usize) -> KernelResult<Vec<IndexEntry>> {
    let header = NodeHeader::parse(buf, node_offset)?;

    let first = node_offset
        .checked_add(usize::try_from(header.entries_offset).map_err(|_| KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;
    let end = node_offset
        .checked_add(usize::try_from(header.entries_size).map_err(|_| KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;

    if first < node_offset.saturating_add(NODE_HEADER_LEN) || end > buf.len() || first > end {
        return Err(KernelError::CorruptedData);
    }

    let mut out = Vec::new();
    let mut offset = first;

    while offset < end {
        let entry_len = usize::from(
            u16_at(buf, offset.saturating_add(0x08)).ok_or(KernelError::CorruptedData)?,
        );
        if entry_len < MIN_ENTRY_LEN || entry_len % 8 != 0 {
            return Err(KernelError::CorruptedData);
        }
        let entry_end = offset
            .checked_add(entry_len)
            .ok_or(KernelError::CorruptedData)?;
        if entry_end > end {
            return Err(KernelError::CorruptedData);
        }
        let entry = buf.get(offset..entry_end).ok_or(KernelError::CorruptedData)?;

        let reference = u64_at(entry, 0x00).ok_or(KernelError::CorruptedData)?;
        let key_len = usize::from(u16_at(entry, 0x0A).ok_or(KernelError::CorruptedData)?);
        let flags = u16_at(entry, 0x0C).ok_or(KernelError::CorruptedData)?;

        let is_last = flags & ENTRY_IS_LAST != 0;
        let has_subnode = flags & ENTRY_HAS_SUBNODE != 0;

        let child_vcn = if has_subnode {
            // The child VCN is the last 8 bytes of the entry, wherever the
            // key happens to end.
            let vcn_off = entry_len.checked_sub(8).ok_or(KernelError::CorruptedData)?;
            Some(u64_at(entry, vcn_off).ok_or(KernelError::CorruptedData)?)
        } else {
            None
        };

        let key = if is_last || key_len == 0 {
            None
        } else {
            let key_end = MIN_ENTRY_LEN
                .checked_add(key_len)
                .ok_or(KernelError::CorruptedData)?;
            let key_bytes = entry
                .get(MIN_ENTRY_LEN..key_end)
                .ok_or(KernelError::CorruptedData)?;
            Some(FileNameAttr::parse(key_bytes)?)
        };

        out.push(IndexEntry {
            reference,
            key,
            child_vcn,
            is_last,
        });

        if is_last {
            break;
        }
        offset = entry_end;
    }

    Ok(out)
}

/// Verify an `INDX` block's magic and VCN, after fixups have been applied.
///
/// The VCN check is not redundant with the fixup check: fixups prove the
/// block was written atomically, but not that it is the block we asked for.
/// A runlist bug that reads the wrong cluster yields a perfectly valid `INDX`
/// block from elsewhere in the same directory, whose entries are real names
/// in the wrong place — a corruption that would otherwise pass every check.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] on a bad magic or a VCN mismatch.
pub fn verify_indx(buf: &[u8], expected_vcn: u64) -> KernelResult<()> {
    if buf.get(0..4) != Some(INDX_MAGIC.as_slice()) {
        return Err(KernelError::CorruptedData);
    }
    let vcn = u64_at(buf, 0x10).ok_or(KernelError::CorruptedData)?;
    if vcn != expected_vcn {
        return Err(KernelError::CorruptedData);
    }
    Ok(())
}
