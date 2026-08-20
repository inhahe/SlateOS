//! The ZFS Attribute Processor: every name-to-value map in the pool.
//!
//! A ZAP is what ZFS uses wherever another filesystem would use a directory
//! block, a superblock field with a name, or a small key/value table. The MOS
//! object directory is a ZAP, a DSL directory's child list is a ZAP, a ZPL
//! directory is a ZAP, and the System Attribute registry is two more. One
//! parser therefore unlocks most of the pool.
//!
//! # Two encodings, chosen by size
//!
//! A ZAP that fits in one block and whose names are short lives as a
//! **microzap**: a flat array of 64-byte slots, each an 8-byte value and a
//! NUL-terminated name of at most 49 characters. Lookup is a linear scan, and
//! that is fine because the whole thing is one block.
//!
//! Anything larger is a **fatzap**: a hash table whose buckets are separate
//! blocks ("leaves"), indexed by a pointer table that maps the top bits of a
//! CRC-64 of the name to a leaf block id. Names and values inside a leaf are
//! stored in chained 24-byte chunks, because a leaf holds variable-length data
//! and ZFS refuses to have two allocators.
//!
//! The two are distinguished by the first eight bytes of block 0, and a caller
//! never has to know which it got.
//!
//! # What is deliberately not supported
//!
//! An **external pointer table** (`zt_numblks != 0`) — a fatzap so large its
//! pointer table no longer fits in the header block. Reaching that takes on
//! the order of a hundred thousand entries in one directory. It is rejected
//! explicitly rather than misread, and [`lookup`] says `NotSupported` so the
//! failure names itself.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::dmu::{Dnode, Reader};
use super::raw::{read_u8, read_u16, read_u32, read_u64, read_u64_be};

/// `ZBT_LEAF`: a fatzap leaf block.
pub const ZBT_LEAF: u64 = 1u64 << 63;
/// `ZBT_HEADER`: the first block of a fatzap.
pub const ZBT_HEADER: u64 = (1u64 << 63) + 1;
/// `ZBT_MICRO`: the single block of a microzap.
pub const ZBT_MICRO: u64 = (1u64 << 63) + 3;

/// `ZAP_MAGIC`, in the fatzap header.
pub const ZAP_MAGIC: u64 = 0x0002_f52a_b2ab;
/// `ZAP_LEAF_MAGIC`, in every fatzap leaf.
pub const ZAP_LEAF_MAGIC: u32 = 0x02AB_1EAF;

/// `MZAP_ENT_LEN`: bytes per microzap slot.
pub(super) const MZAP_ENT_LEN: usize = 64;
/// `MZAP_NAME_LEN`: bytes of name in a microzap slot, NUL included.
const MZAP_NAME_LEN: usize = 50;

/// `ZAP_LEAF_CHUNKSIZE`.
pub(super) const ZAP_LEAF_CHUNKSIZE: usize = 24;
/// `ZAP_LEAF_HDRSIZE`: two chunks' worth of leaf header.
pub(super) const ZAP_LEAF_HDRSIZE: usize = 2 * ZAP_LEAF_CHUNKSIZE;
/// `ZAP_LEAF_ARRAY_BYTES`: payload bytes in an array chunk.
pub(super) const ZAP_LEAF_ARRAY_BYTES: usize = ZAP_LEAF_CHUNKSIZE - 3;

/// `ZAP_CHUNK_FREE`.
pub(super) const ZAP_CHUNK_FREE: u8 = 253;
/// `ZAP_CHUNK_ENTRY`.
pub(super) const ZAP_CHUNK_ENTRY: u8 = 252;
/// `ZAP_CHUNK_ARRAY`.
pub(super) const ZAP_CHUNK_ARRAY: u8 = 251;

/// `CHAIN_END`: the terminator of a leaf's hash chain and of an array chain.
pub(super) const CHAIN_END: u16 = 0xffff;

/// `ZAP_HASHBITS`: how many of the CRC-64's high bits index the hash table.
/// The remaining low bits are left to the collision differentiator.
pub(super) const ZAP_HASHBITS: u32 = 28;

/// `ZFS_CRC64_POLY`, the reflected polynomial behind `zfs_crc64_table`.
const ZFS_CRC64_POLY: u64 = 0xD800_0000_0000_0000;

/// Refuse a ZAP with more chained array chunks than this while assembling one
/// name. A leaf holds at most a few hundred chunks, so any chain longer than
/// this is a cycle in corrupt data, and following it would hang the kernel.
const MAX_ARRAY_CHUNKS: usize = 4096;

/// The CRC-64 byte table ZFS stirs names through, built at compile time.
///
/// Generating it rather than transcribing 256 constants means it cannot be
/// transcribed *wrongly*, and the generator is the four lines from
/// `zfs_fletcher.c` that produced the published table.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(super) const ZFS_CRC64_TABLE: [u64; 256] = {
    // A const initialiser cannot use iterators or `get()`, and every index
    // here is a loop counter bounded by the array length, so the lint has
    // nothing to catch.
    let mut table = [0u64; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut ct = i as u64;
        let mut j = 0;
        while j < 8 {
            ct = (ct >> 1) ^ (0u64.wrapping_sub(ct & 1) & ZFS_CRC64_POLY);
            j += 1;
        }
        table[i] = ct;
        i += 1;
    }
    table
};

/// One entry of a ZAP, as returned by [`entries`].
#[derive(Debug, Clone)]
pub struct ZapEntry {
    /// The name, without its terminating NUL.
    pub name: Vec<u8>,
    /// The value. ZAPs can hold arrays, but every consumer in this driver
    /// wants a single 64-bit integer, and an entry that is not one is skipped.
    pub value: u64,
}

/// Hash a name the way a fatzap does: CRC-64 seeded with the ZAP's salt,
/// keeping only the top [`ZAP_HASHBITS`] bits.
///
/// The salt is per-ZAP, which is what stops an attacker who can choose file
/// names from forcing every name in a directory into one hash bucket.
#[must_use]
pub fn zap_hash(salt: u64, name: &[u8]) -> u64 {
    let mut h = salt;
    for &c in name {
        let idx = usize::from(((h ^ u64::from(c)) & 0xFF) as u8);
        // The index is masked to a byte, so it is always in range; the
        // fallback keeps this free of panicking indexing.
        let t = ZFS_CRC64_TABLE.get(idx).copied().unwrap_or(0);
        h = (h >> 8) ^ t;
    }
    // Keep only the high bits; the low ones belong to the collision
    // differentiator and are not part of the bucket index.
    h & !((1u64 << (64 - ZAP_HASHBITS)) - 1)
}

/// Look up `name` in the ZAP object described by `dn`.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if the name is absent.
/// - [`KernelError::NotSupported`] for an external pointer table.
/// - [`KernelError::CorruptedData`] if the block is neither a microzap nor a
///   fatzap header, or a leaf fails its own magic check.
pub fn lookup(reader: &Reader<'_>, dn: &Dnode, name: &[u8]) -> KernelResult<u64> {
    let block0 = reader.read_object_block(dn, 0)?;
    match read_u64(&block0, 0)? {
        ZBT_MICRO => micro_lookup(&block0, name),
        ZBT_HEADER => fat_lookup(reader, dn, &block0, name),
        _ => Err(KernelError::CorruptedData),
    }
}

/// Look up `name` and return its value as a raw integer array.
///
/// Used for the System Attribute `LAYOUTS` ZAP, whose values are `u16` arrays
/// of attribute numbers rather than single objects. A microzap answers here
/// too, as a one-element array of 8-byte integers: a microzap value is always
/// exactly one `u64` by construction, so this is a widening, not a guess.
///
/// # Errors
///
/// As [`lookup`], plus [`KernelError::CorruptedData`] if the entry declares an
/// integer width that is not 1, 2, 4 or 8.
pub fn lookup_array(reader: &Reader<'_>, dn: &Dnode, name: &[u8]) -> KernelResult<ZapArray> {
    let block0 = reader.read_object_block(dn, 0)?;
    match read_u64(&block0, 0)? {
        ZBT_MICRO => {
            let value = micro_lookup(&block0, name)?;
            Ok(ZapArray {
                intlen: 8,
                numints: 1,
                bytes: value.to_be_bytes().to_vec(),
            })
        }
        ZBT_HEADER => fat_lookup_array(reader, dn, &block0, name),
        _ => Err(KernelError::CorruptedData),
    }
}

/// Enumerate every entry of the ZAP object described by `dn`.
///
/// # Errors
///
/// As [`lookup`], minus [`KernelError::NotFound`] — an empty ZAP yields an
/// empty vector rather than an error.
pub fn entries(reader: &Reader<'_>, dn: &Dnode) -> KernelResult<Vec<ZapEntry>> {
    let block0 = reader.read_object_block(dn, 0)?;
    match read_u64(&block0, 0)? {
        ZBT_MICRO => Ok(micro_entries(&block0)),
        ZBT_HEADER => fat_entries(reader, dn, &block0),
        _ => Err(KernelError::CorruptedData),
    }
}

// ---------------------------------------------------------------------------
// Microzap
// ---------------------------------------------------------------------------

/// The name in a microzap slot, up to its NUL.
fn micro_name(block: &[u8], slot_off: usize) -> &[u8] {
    let start = slot_off.wrapping_add(14);
    let end = start.wrapping_add(MZAP_NAME_LEN);
    let raw = block.get(start..end).unwrap_or(&[]);
    let n = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    raw.get(..n).unwrap_or(&[])
}

pub(super) fn micro_lookup(block: &[u8], name: &[u8]) -> KernelResult<u64> {
    let mut off = MZAP_ENT_LEN; // The 64-byte header is one slot's worth.
    while off.saturating_add(MZAP_ENT_LEN) <= block.len() {
        let value = read_u64(block, off)?;
        let ent_name = micro_name(block, off);
        // An empty name marks an unused slot. A slot with a name is live
        // regardless of its value, so the name is what is tested.
        if !ent_name.is_empty() && ent_name == name {
            return Ok(value);
        }
        off = off.wrapping_add(MZAP_ENT_LEN);
    }
    Err(KernelError::NotFound)
}

pub(super) fn micro_entries(block: &[u8]) -> Vec<ZapEntry> {
    let mut out = Vec::new();
    let mut off = MZAP_ENT_LEN;
    while off.saturating_add(MZAP_ENT_LEN) <= block.len() {
        let name = micro_name(block, off);
        if !name.is_empty() {
            if let Ok(value) = read_u64(block, off) {
                out.push(ZapEntry {
                    name: name.to_vec(),
                    value,
                });
            }
        }
        off = off.wrapping_add(MZAP_ENT_LEN);
    }
    out
}

// ---------------------------------------------------------------------------
// Fatzap
// ---------------------------------------------------------------------------

/// The parts of `zap_phys_t` a reader needs.
struct FatHeader {
    /// `zap_ptrtbl.zt_blk` — non-zero means the table is external.
    ptrtbl_blk: u64,
    /// `zap_ptrtbl.zt_numblks`.
    ptrtbl_numblks: u64,
    /// `zap_ptrtbl.zt_shift` — how many hash bits index the table.
    ptrtbl_shift: u64,
    /// `zap_salt`.
    salt: u64,
    /// `log2` of the ZAP's block size.
    blkshift: u32,
}

fn parse_fat_header(dn: &Dnode, block0: &[u8]) -> KernelResult<FatHeader> {
    if read_u64(block0, 8)? != ZAP_MAGIC {
        return Err(KernelError::CorruptedData);
    }
    let blkshift = log2_exact(dn.datablksz).ok_or(KernelError::CorruptedData)?;
    // The embedded pointer table lives in the second half of the block, so a
    // block smaller than 32 bytes would put it inside the header.
    if blkshift < 9 {
        return Err(KernelError::CorruptedData);
    }
    Ok(FatHeader {
        ptrtbl_blk: read_u64(block0, 16)?,
        ptrtbl_numblks: read_u64(block0, 24)?,
        ptrtbl_shift: read_u64(block0, 32)?,
        salt: read_u64(block0, 72)?,
        blkshift,
    })
}

/// `ZAP_HASH_IDX`: the top `shift` bits of the hash.
const fn zap_hash_idx(hash: u64, shift: u64) -> u64 {
    if shift == 0 || shift > 64 {
        0
    } else {
        // `1 <= shift <= 64` here, so the subtraction cannot wrap and the
        // shift is in range. Written with the checked form so the guard above
        // and this line cannot drift apart.
        hash >> 64u64.wrapping_sub(shift)
    }
}

/// Read entry `idx` of the embedded pointer table, which occupies the second
/// half of the header block as an array of `u64` block ids.
fn embedded_ptrtbl(block0: &[u8], blkshift: u32, idx: u64) -> KernelResult<u64> {
    // The table starts halfway through the block: `1 << (blkshift - 4)` u64s
    // in, which is `1 << (blkshift - 1)` bytes.
    let base = 1usize
        .checked_shl(blkshift.checked_sub(1).ok_or(KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;
    let off = usize::try_from(idx)
        .ok()
        .and_then(|i| i.checked_mul(8))
        .and_then(|d| base.checked_add(d))
        .ok_or(KernelError::CorruptedData)?;
    read_u64(block0, off)
}

/// Read the leaf block that would hold `name`, and the hash used to find it.
fn fat_leaf_for(
    reader: &Reader<'_>,
    dn: &Dnode,
    block0: &[u8],
    name: &[u8],
) -> KernelResult<(Vec<u8>, u64, u32)> {
    let hdr = parse_fat_header(dn, block0)?;
    if hdr.ptrtbl_numblks != 0 || hdr.ptrtbl_blk != 0 {
        return Err(KernelError::NotSupported);
    }
    let hash = zap_hash(hdr.salt, name);
    let idx = zap_hash_idx(hash, hdr.ptrtbl_shift);
    let blkid = embedded_ptrtbl(block0, hdr.blkshift, idx)?;
    let leaf = reader.read_object_block(dn, blkid)?;
    Ok((leaf, hash, hdr.blkshift))
}

fn fat_lookup(reader: &Reader<'_>, dn: &Dnode, block0: &[u8], name: &[u8]) -> KernelResult<u64> {
    let (leaf, hash, blkshift) = fat_leaf_for(reader, dn, block0, name)?;
    leaf_lookup(&leaf, blkshift, hash, name)
}

fn fat_lookup_array(
    reader: &Reader<'_>,
    dn: &Dnode,
    block0: &[u8],
    name: &[u8],
) -> KernelResult<ZapArray> {
    let (leaf, hash, blkshift) = fat_leaf_for(reader, dn, block0, name)?;
    leaf_lookup_array(&leaf, blkshift, hash, name)
}

fn fat_entries(reader: &Reader<'_>, dn: &Dnode, block0: &[u8]) -> KernelResult<Vec<ZapEntry>> {
    let hdr = parse_fat_header(dn, block0)?;
    if hdr.ptrtbl_numblks != 0 || hdr.ptrtbl_blk != 0 {
        return Err(KernelError::NotSupported);
    }
    if hdr.ptrtbl_shift > 24 {
        // 2^24 pointer-table entries in an *embedded* table is impossible —
        // the table would be 128 MiB inside one block — so this is corruption
        // and iterating it would allocate unboundedly.
        return Err(KernelError::CorruptedData);
    }
    let count = 1u64
        .checked_shl(u32::try_from(hdr.ptrtbl_shift).map_err(|_| KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;

    // The pointer table maps 2^shift hash prefixes onto far fewer leaves, so
    // the same block id appears many times over. Collecting the distinct ones
    // first turns "read a leaf per table slot" into "read a leaf per leaf".
    let mut blkids: Vec<u64> = Vec::new();
    for i in 0..count {
        let blkid = embedded_ptrtbl(block0, hdr.blkshift, i)?;
        if blkid != 0 && !blkids.contains(&blkid) {
            blkids.push(blkid);
        }
    }

    let mut out = Vec::new();
    for blkid in blkids {
        let Ok(leaf) = reader.read_object_block(dn, blkid) else {
            continue;
        };
        leaf_entries(&leaf, hdr.blkshift, &mut out);
    }
    Ok(out)
}

/// Byte offset of leaf chunk `idx`.
pub(super) fn leaf_chunk_off(blkshift: u32, idx: usize) -> Option<usize> {
    // Header, then the hash table of `1 << (blkshift - 5)` u16 entries.
    let hash_entries = 1usize.checked_shl(blkshift.checked_sub(5)?)?;
    let base = ZAP_LEAF_HDRSIZE.checked_add(hash_entries.checked_mul(2)?)?;
    base.checked_add(idx.checked_mul(ZAP_LEAF_CHUNKSIZE)?)
}

/// Number of chunks in a leaf of `1 << blkshift` bytes.
///
/// Upstream states this as "block size, less two bytes per hash-table entry,
/// less two chunks of header, divided by the chunk size" — and the result is
/// exact: for a 16 KiB leaf, `1072 + 638 * 24 == 16384`, so the last chunk
/// ends precisely at the end of the block.
pub(super) fn leaf_numchunks(blkshift: u32) -> Option<usize> {
    let size = 1usize.checked_shl(blkshift)?;
    let hash_entries = 1usize.checked_shl(blkshift.checked_sub(5)?)?;
    size.checked_sub(hash_entries.checked_mul(2)?)?
        .checked_div(ZAP_LEAF_CHUNKSIZE)?
        .checked_sub(2)
}

fn leaf_validate(leaf: &[u8]) -> KernelResult<u16> {
    if read_u64(leaf, 0)? != ZBT_LEAF {
        return Err(KernelError::CorruptedData);
    }
    if read_u32(leaf, 24)? != ZAP_LEAF_MAGIC {
        return Err(KernelError::CorruptedData);
    }
    // `lh_prefix_len`: how many hash bits this leaf's prefix already consumed.
    read_u16(leaf, 32)
}

/// Assemble `total` bytes from the chunk chain starting at `first`.
///
/// A ZAP array is a linked list of 21-byte payload fragments; the caller knows
/// how many bytes it wants because the entry header records `intlen` and
/// `numints`. Everything else here is bounds- and cycle-checking: the chain is
/// on-disk data, so a corrupt `le_next` must terminate rather than spin.
fn leaf_array_bytes(leaf: &[u8], blkshift: u32, first: u16, total: usize) -> KernelResult<Vec<u8>> {
    let mut out = Vec::with_capacity(total.min(1 << 12));
    let mut chunk = first;
    let mut guard = 0usize;
    while chunk != CHAIN_END && out.len() < total {
        guard = guard.wrapping_add(1);
        if guard > MAX_ARRAY_CHUNKS {
            return Err(KernelError::CorruptedData);
        }
        let off = leaf_chunk_off(blkshift, usize::from(chunk)).ok_or(KernelError::CorruptedData)?;
        if read_u8(leaf, off)? != ZAP_CHUNK_ARRAY {
            return Err(KernelError::CorruptedData);
        }
        let want = total.saturating_sub(out.len()).min(ZAP_LEAF_ARRAY_BYTES);
        let start = off.wrapping_add(1);
        out.extend_from_slice(
            leaf.get(start..start.wrapping_add(want))
                .ok_or(KernelError::CorruptedData)?,
        );
        chunk = read_u16(leaf, off.wrapping_add(1).wrapping_add(ZAP_LEAF_ARRAY_BYTES))?;
    }
    Ok(out)
}

/// [`leaf_array_bytes`] for a name: `numints` single-byte integers.
fn leaf_array(leaf: &[u8], blkshift: u32, first: u16, numints: u16) -> KernelResult<Vec<u8>> {
    leaf_array_bytes(leaf, blkshift, first, usize::from(numints))
}

/// A ZAP value that is not a single 64-bit integer: the width of its
/// integers, how many there are, and the raw bytes.
#[derive(Debug, Clone)]
pub struct ZapArray {
    /// Bytes per integer: 1, 2, 4 or 8.
    pub intlen: u8,
    /// Number of integers.
    pub numints: u16,
    /// `intlen * numints` bytes, in the order stored.
    pub bytes: Vec<u8>,
}

impl ZapArray {
    /// Interpret the array as `u16`s.
    ///
    /// The SA layout table is exactly this: a list of attribute numbers. ZAP
    /// integers are stored big-endian whatever the pool's byte order, so this
    /// is not a host-order reinterpretation.
    #[must_use]
    pub fn as_u16s(&self) -> Vec<u16> {
        if self.intlen != 2 {
            return Vec::new();
        }
        self.bytes
            .chunks_exact(2)
            .map(|c| {
                let mut b = [0u8; 2];
                b.copy_from_slice(c);
                u16::from_be_bytes(b)
            })
            .collect()
    }
}

/// Read a leaf entry's value as raw integers.
fn leaf_value_raw(leaf: &[u8], blkshift: u32, entry_off: usize) -> KernelResult<ZapArray> {
    let intlen = read_u8(leaf, entry_off.wrapping_add(1))?;
    let numints = read_u16(leaf, entry_off.wrapping_add(10))?;
    if intlen == 0 || !matches!(intlen, 1 | 2 | 4 | 8) {
        return Err(KernelError::CorruptedData);
    }
    let total = usize::from(intlen)
        .checked_mul(usize::from(numints))
        .ok_or(KernelError::CorruptedData)?;
    let value_chunk = read_u16(leaf, entry_off.wrapping_add(8))?;
    let bytes = leaf_array_bytes(leaf, blkshift, value_chunk, total)?;
    Ok(ZapArray {
        intlen,
        numints,
        bytes,
    })
}

/// Read a leaf entry's single 64-bit value.
///
/// ZAP array integers are stored big-endian regardless of the pool's byte
/// order — an oddity of the format, not of this reader.
fn leaf_value(leaf: &[u8], blkshift: u32, entry_off: usize) -> KernelResult<u64> {
    let intlen = read_u8(leaf, entry_off.wrapping_add(1))?;
    let numints = read_u16(leaf, entry_off.wrapping_add(10))?;
    if intlen != 8 || numints != 1 {
        // Every name-to-object consumer in this driver wants one 64-bit
        // value. An entry that is a byte array (an extended attribute, say)
        // is not an error in the ZAP — it is simply not something these
        // callers can use, and `lookup_array` exists for the ones that can.
        return Err(KernelError::NotSupported);
    }
    let value_chunk = read_u16(leaf, entry_off.wrapping_add(8))?;
    let off =
        leaf_chunk_off(blkshift, usize::from(value_chunk)).ok_or(KernelError::CorruptedData)?;
    if read_u8(leaf, off)? != ZAP_CHUNK_ARRAY {
        return Err(KernelError::CorruptedData);
    }
    read_u64_be(leaf, off.wrapping_add(1))
}

/// Byte offset of the entry chunk for `name`, found by walking its hash chain.
fn leaf_find(leaf: &[u8], blkshift: u32, hash: u64, name: &[u8]) -> KernelResult<usize> {
    let prefix_len = leaf_validate(leaf)?;
    let numchunks = leaf_numchunks(blkshift).ok_or(KernelError::CorruptedData)?;

    // LEAF_HASH: the hash bits just below the ones the pointer table used.
    let hash_shift = blkshift
        .checked_sub(5)
        .and_then(|b| b.checked_add(u32::from(prefix_len)))
        .ok_or(KernelError::CorruptedData)?;
    if hash_shift > 64 {
        return Err(KernelError::CorruptedData);
    }
    let hash_entries = 1usize
        .checked_shl(blkshift.checked_sub(5).ok_or(KernelError::CorruptedData)?)
        .ok_or(KernelError::CorruptedData)?;
    let bucket = 64u32
        .checked_sub(hash_shift)
        .and_then(|s| hash.checked_shr(s))
        .and_then(|h| usize::try_from(h).ok())
        .ok_or(KernelError::CorruptedData)?
        & hash_entries.saturating_sub(1);

    let mut chunk = read_u16(leaf, ZAP_LEAF_HDRSIZE.wrapping_add(bucket.wrapping_mul(2)))?;
    let mut guard = 0usize;
    while chunk != CHAIN_END {
        guard = guard.wrapping_add(1);
        if guard > numchunks {
            // A hash chain longer than the leaf has chunks is a cycle.
            return Err(KernelError::CorruptedData);
        }
        if usize::from(chunk) >= numchunks {
            return Err(KernelError::CorruptedData);
        }
        let off = leaf_chunk_off(blkshift, usize::from(chunk)).ok_or(KernelError::CorruptedData)?;
        if read_u8(leaf, off)? != ZAP_CHUNK_ENTRY {
            return Err(KernelError::CorruptedData);
        }
        let le_hash = read_u64(leaf, off.wrapping_add(16))?;
        if le_hash == hash {
            let name_chunk = read_u16(leaf, off.wrapping_add(4))?;
            let name_numints = read_u16(leaf, off.wrapping_add(6))?;
            let stored = leaf_array(leaf, blkshift, name_chunk, name_numints)?;
            // `name_numints` counts the trailing NUL for a byte-array name.
            let stored = strip_nul(&stored);
            if stored == name {
                return Ok(off);
            }
        }
        chunk = read_u16(leaf, off.wrapping_add(2))?;
    }
    Err(KernelError::NotFound)
}

pub(super) fn leaf_lookup(leaf: &[u8], blkshift: u32, hash: u64, name: &[u8]) -> KernelResult<u64> {
    let off = leaf_find(leaf, blkshift, hash, name)?;
    leaf_value(leaf, blkshift, off)
}

pub(super) fn leaf_lookup_array(
    leaf: &[u8],
    blkshift: u32,
    hash: u64,
    name: &[u8],
) -> KernelResult<ZapArray> {
    let off = leaf_find(leaf, blkshift, hash, name)?;
    leaf_value_raw(leaf, blkshift, off)
}

/// Append every entry in `leaf` to `out`, skipping anything malformed.
///
/// Iteration walks the chunk array directly rather than the hash chains: the
/// chains partition the same entries, and scanning linearly cannot loop.
pub(super) fn leaf_entries(leaf: &[u8], blkshift: u32, out: &mut Vec<ZapEntry>) {
    if leaf_validate(leaf).is_err() {
        return;
    }
    let Some(numchunks) = leaf_numchunks(blkshift) else {
        return;
    };
    for idx in 0..numchunks {
        let Some(off) = leaf_chunk_off(blkshift, idx) else {
            continue;
        };
        match read_u8(leaf, off) {
            Ok(ZAP_CHUNK_ENTRY) => {}
            Ok(ZAP_CHUNK_ARRAY | ZAP_CHUNK_FREE) | Err(_) => continue,
            Ok(_) => continue,
        }
        let (Ok(name_chunk), Ok(name_numints)) = (
            read_u16(leaf, off.wrapping_add(4)),
            read_u16(leaf, off.wrapping_add(6)),
        ) else {
            continue;
        };
        let Ok(stored) = leaf_array(leaf, blkshift, name_chunk, name_numints) else {
            continue;
        };
        let Ok(value) = leaf_value(leaf, blkshift, off) else {
            continue;
        };
        let name = strip_nul(&stored);
        if !name.is_empty() {
            out.push(ZapEntry {
                name: name.to_vec(),
                value,
            });
        }
    }
}

fn strip_nul(bytes: &[u8]) -> &[u8] {
    let n = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes.get(..n).unwrap_or(&[])
}

/// `log2` of `v`, but only when `v` is an exact power of two.
pub(super) fn log2_exact(v: u64) -> Option<u32> {
    if v == 0 || !v.is_power_of_two() {
        return None;
    }
    Some(v.trailing_zeros())
}
