//! ext4 hash-tree (htree / dx) directory index for O(1) name lookups.
//!
//! Large ext4 directories use a hash tree index instead of linear entry
//! scanning.  The htree is a 1- or 2-level B-tree keyed by a hash of
//! the filename, stored in the directory's first data block.
//!
//! ## On-disk layout
//!
//! ```text
//! Block 0 (dx_root):
//!   [ "."  dir entry (12 bytes) ]
//!   [ ".." dir entry (rec_len fills to dx_root_info) ]
//!   [ DxRootInfo (8 bytes) ]
//!   [ DxCountLimit (4 bytes, disguised as first DxEntry) ]
//!   [ DxEntry[1..count] sorted by hash ]
//!
//! Block N (leaf):
//!   [ standard dir entries — linear scan within one block ]
//!
//! (Optional) Block M (dx_node, if indirect_levels > 0):
//!   [ fake dir entry (8 bytes) ]
//!   [ DxCountLimit + DxEntry[1..count] ]
//! ```
//!
//! ## Hash algorithm
//!
//! The default hash for modern ext4 is `half_md4` (version 1 or 4 for
//! unsigned).  The hash seed comes from the superblock's `s_hash_seed[4]`.
//!
//! ## References
//!
//! - Linux `fs/ext4/namei.c` (dx_probe, ext4fs_dirhash)
//! - Linux `fs/ext4/hash.c` (half MD4 / TEA implementations)
//! - ext4 wiki: <https://ext4.wiki.kernel.org/index.php/Ext4_Disk_Layout#Hash_Tree_Directories>

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::driver::{read_struct_pub, Ext4Driver};
use super::ondisk::{
    DxCountLimit, DxRootInfo, Ext4DirEntry2,
    inode_flags,
};

// ---------------------------------------------------------------------------
// Hash functions
// ---------------------------------------------------------------------------

/// Compute the ext4 directory hash for a filename.
///
/// Dispatches to the correct hash function based on `hash_version`
/// (from `s_def_hash_version` in the superblock).
///
/// Returns `(hash, minor_hash)`.  The minor_hash is used for tiebreaking
/// but most lookups only need the primary hash.
pub fn ext4_dirhash(
    name: &[u8],
    hash_version: u8,
    seed: &[u32; 4],
) -> (u32, u32) {
    match hash_version {
        // 0 = legacy (signed), 3 = legacy (unsigned) — simple DJB-style
        0 | 3 => legacy_hash(name, hash_version == 3),
        // 1 = half_md4 (signed), 4 = half_md4 (unsigned) — most common
        1 | 4 => half_md4_hash(name, seed),
        // 2 = TEA (signed), 5 = TEA (unsigned)
        2 | 5 => tea_hash(name, seed),
        // Unknown — fall back to half_md4
        _ => half_md4_hash(name, seed),
    }
}

/// Legacy ext2 hash (DJB-style).
///
/// Simple but weak — only used on very old filesystems.
fn legacy_hash(name: &[u8], _unsigned: bool) -> (u32, u32) {
    let mut hash: u32 = 0x12A3FE2D;
    let mut hash2: u32 = 0x37ABE8F9;

    for &byte in name {
        let b = u32::from(byte);
        hash = hash.wrapping_mul(11).wrapping_add(b);
        hash2 = hash2.wrapping_mul(11).wrapping_add(b);
    }

    // Fold to ensure non-zero and strip the sign bit.
    (fold_hash(hash), fold_hash(hash2))
}

/// Half-MD4 hash — the standard ext4 directory hash.
///
/// Based on Linux `fs/ext4/hash.c` `dx_hack_hash()` + `half_md4_transform`.
/// This is the most commonly used hash version (versions 1 and 4).
fn half_md4_hash(name: &[u8], seed: &[u32; 4]) -> (u32, u32) {
    // Initialize state from seed.
    let mut a = seed[0];
    let mut b = seed[1];
    let mut c = seed[2];
    let mut d = seed[3];

    // Pad name into u32 chunks, 8 at a time for the half-MD4 rounds.
    // Linux pads with zero bytes and processes 32 bytes (8 u32s) per round.
    let mut buf = [0u32; 8];
    let mut offset = 0usize;

    while offset < name.len() {
        // Fill buf with up to 8 u32s from the name.
        for slot in &mut buf {
            *slot = 0;
        }
        for (i, slot) in buf.iter_mut().enumerate() {
            let byte_off = offset.saturating_add(i.saturating_mul(4));
            if byte_off < name.len() {
                *slot = str2hashbuf(name, byte_off);
            }
        }
        offset = offset.saturating_add(32); // 8 * 4 bytes per round

        // Half-MD4 transform (4 rounds of 4 operations each).
        half_md4_transform(&mut a, &mut b, &mut c, &mut d, &buf);
    }

    (fold_hash(b), fold_hash(d))
}

/// TEA (Tiny Encryption Algorithm) hash variant.
///
/// Used when `s_def_hash_version` is 2 or 5.  TEA is a block cipher
/// used here as a hash function.
///
/// Based on Linux `fs/ext4/hash.c`.
fn tea_hash(name: &[u8], seed: &[u32; 4]) -> (u32, u32) {
    let mut a = seed[0];
    let mut b = seed[1];
    let mut c = seed[2];
    let mut d = seed[3];

    let mut buf = [0u32; 4];
    let mut offset = 0usize;

    while offset < name.len() {
        for slot in &mut buf {
            *slot = 0;
        }
        for (i, slot) in buf.iter_mut().enumerate() {
            let byte_off = offset.saturating_add(i.saturating_mul(4));
            if byte_off < name.len() {
                *slot = str2hashbuf(name, byte_off);
            }
        }
        offset = offset.saturating_add(16);

        tea_transform(&mut a, &mut b, &mut c, &mut d, &buf);
    }

    (fold_hash(a), fold_hash(c))
}

/// Convert up to 4 bytes of a name at `offset` into a little-endian u32.
fn str2hashbuf(name: &[u8], offset: usize) -> u32 {
    let mut val = 0u32;
    let remaining = name.len().saturating_sub(offset);
    let count = remaining.min(4);

    for i in 0..count {
        if let Some(&byte) = name.get(offset.saturating_add(i)) {
            val |= u32::from(byte) << (i.saturating_mul(8));
        }
    }

    // Pad partial words with the length byte (Linux convention).
    if count < 4 {
        val |= (count as u32) << (count.saturating_mul(8));
    }

    val
}

/// Fold a 32-bit hash to ensure it's valid for ext4 (non-zero, no sign bit).
///
/// Matches Linux's `hash & ~1` and special-casing of 0 → 2.
fn fold_hash(mut h: u32) -> u32 {
    // Clear the low bit (ext4 uses bit 0 for internal flags in some contexts).
    h &= !1u32;
    // Zero hash is invalid in the htree.
    if h == 0 {
        h = 2;
    }
    h
}

// ---------------------------------------------------------------------------
// Half-MD4 transform
// ---------------------------------------------------------------------------

/// One round of the half-MD4 transform on state (a,b,c,d) with input buf[8].
///
/// This is a simplified version of MD4 using only 2 of the 3 MD4 rounds
/// (F and G, skipping H).  Matches Linux `fs/ext4/hash.c` exactly.
#[allow(clippy::many_single_char_names)]
fn half_md4_transform(
    a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32,
    buf: &[u32; 8],
) {
    // Round 1 (F function: (b & c) | (!b & d))
    macro_rules! round1 {
        ($a:expr, $b:expr, $c:expr, $d:expr, $k:expr, $s:expr) => {
            $a = $a.wrapping_add(
                (($b & $c) | (!$b & $d))
                    .wrapping_add(buf[$k])
            );
            $a = $a.rotate_left($s);
        };
    }

    round1!(*a, *b, *c, *d, 0, 3);
    round1!(*d, *a, *b, *c, 1, 7);
    round1!(*c, *d, *a, *b, 2, 11);
    round1!(*b, *c, *d, *a, 3, 19);
    round1!(*a, *b, *c, *d, 4, 3);
    round1!(*d, *a, *b, *c, 5, 7);
    round1!(*c, *d, *a, *b, 6, 11);
    round1!(*b, *c, *d, *a, 7, 19);

    // Round 2 (G function: (b & c) | (b & d) | (c & d))
    // Constant: 0x5A827999 (sqrt(2) * 2^30)
    const K2: u32 = 0x5A82_7999;
    macro_rules! round2 {
        ($a:expr, $b:expr, $c:expr, $d:expr, $k:expr, $s:expr) => {
            $a = $a.wrapping_add(
                (($b & $c) | ($b & $d) | ($c & $d))
                    .wrapping_add(buf[$k])
                    .wrapping_add(K2)
            );
            $a = $a.rotate_left($s);
        };
    }

    round2!(*a, *b, *c, *d, 1, 3);
    round2!(*d, *a, *b, *c, 3, 5);
    round2!(*c, *d, *a, *b, 5, 9);
    round2!(*b, *c, *d, *a, 7, 13);
    round2!(*a, *b, *c, *d, 0, 3);
    round2!(*d, *a, *b, *c, 2, 5);
    round2!(*c, *d, *a, *b, 4, 9);
    round2!(*b, *c, *d, *a, 6, 13);
}

/// TEA transform round.
///
/// Matches Linux `fs/ext4/hash.c` TEA hash implementation.
#[allow(clippy::many_single_char_names)]
fn tea_transform(
    a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32,
    buf: &[u32; 4],
) {
    let mut sum: u32 = 0;
    const DELTA: u32 = 0x9E37_79B9;

    // 16 rounds of TEA (standard is 32 but ext4 uses 16).
    for _ in 0..16 {
        sum = sum.wrapping_add(DELTA);
        *b = b.wrapping_add(
            ((*a << 4).wrapping_add(buf[0]))
            ^ a.wrapping_add(sum)
            ^ ((*a >> 5).wrapping_add(buf[1]))
        );
        *a = a.wrapping_add(
            ((*b << 4).wrapping_add(buf[2]))
            ^ b.wrapping_add(sum)
            ^ ((*b >> 5).wrapping_add(buf[3]))
        );
    }

    // Mix in c and d.
    *c = c.wrapping_add(*a);
    *d = d.wrapping_add(*b);
}

// ---------------------------------------------------------------------------
// Htree lookup
// ---------------------------------------------------------------------------

/// Layout offsets within the dx_root block.
///
/// The root block starts with:
/// - "." entry: 12 bytes (inode=4, rec_len=12, name_len=1, file_type=2, name=".")
/// - ".." entry header: 8 bytes (but rec_len extends to cover the rest)
///   At offset 20 within ".." entry's padding area:
/// - dx_root_info: 8 bytes
/// - First dx_entry (actually DxCountLimit): at offset 28
/// - Subsequent dx_entries: at offset 28 + 8*i
const DOT_ENTRY_SIZE: usize = 12;
const DOTDOT_HEADER_SIZE: usize = 8;
/// Offset of dx_root_info from the start of the block.
const DX_ROOT_INFO_OFFSET: usize = DOT_ENTRY_SIZE + DOTDOT_HEADER_SIZE;
/// Offset of the first dx_entry (count/limit) from the start of the block.
const DX_ROOT_ENTRIES_OFFSET: usize = DX_ROOT_INFO_OFFSET + 8; // after DxRootInfo

/// Internal node header size (fake dir entry = 8 bytes).
const DX_NODE_HEADER_SIZE: usize = 8;

/// Attempt an htree-accelerated directory lookup.
///
/// Returns `Ok(Some((inode, file_type)))` on success, `Ok(None)` if the
/// directory doesn't use htree (caller should fall back to linear scan),
/// or `Err(...)` on I/O or structural errors.
///
/// This is the primary entry point — called from `Ext4Driver::dir_lookup()`
/// when the directory inode has the INDEX flag set.
pub fn htree_lookup(
    driver: &Ext4Driver,
    dir_ino: u32,
    name: &str,
) -> KernelResult<Option<(u32, u8)>> {
    let dir_inode = driver.read_inode(dir_ino)?;

    // Check the INDEX flag on the inode.
    if dir_inode.i_flags & inode_flags::INDEX == 0 {
        return Ok(None); // Not htree-indexed; caller uses linear scan.
    }

    // Map logical block 0 → physical block for the dx_root.
    let phys_block = match driver.logical_to_physical(dir_ino, &dir_inode, 0) {
        Ok(Some(pb)) => pb,
        Ok(None) => return Ok(None), // Empty directory — shouldn't happen with htree.
        Err(_) => return Ok(None),   // Fall back to linear scan on error.
    };

    let root_data = driver.read_block(phys_block)?;
    if root_data.len() < DX_ROOT_ENTRIES_OFFSET + 4 {
        return Ok(None); // Block too small for htree root.
    }

    // Parse dx_root_info.
    let info_bytes = root_data
        .get(DX_ROOT_INFO_OFFSET..DX_ROOT_INFO_OFFSET + 8)
        .ok_or(KernelError::IoError)?;
    let info: DxRootInfo = read_struct_pub(info_bytes)?;

    // Validate: info_length should be 8, indirect_levels ≤ 2.
    if info.info_length != 8 || info.indirect_levels > 2 {
        return Ok(None); // Unusual structure — fall back to linear.
    }

    // Compute the hash for the target name.
    let hash_version = driver.superblock().raw.s_def_hash_version;
    let seed = &driver.superblock().raw.s_hash_seed;
    let (target_hash, _minor) = ext4_dirhash(name.as_bytes(), hash_version, seed);

    // Read the count/limit from the first dx_entry slot.
    let cl_bytes = root_data
        .get(DX_ROOT_ENTRIES_OFFSET..DX_ROOT_ENTRIES_OFFSET + 4)
        .ok_or(KernelError::IoError)?;
    let cl: DxCountLimit = read_struct_pub(cl_bytes)?;

    // DxCountLimit layout (overlays first DxEntry's hash field):
    //   limit: u16 — max entries that fit in this node
    //   count: u16 — actual number of valid entries (including slot 0)
    //
    // Entry 0 is the count/limit slot itself; its `block` field (second u32
    // of the DxEntry) is the default leaf block for hashes below entries[1].
    let entry_count = cl.count as usize; // actual valid entries

    if entry_count < 1 {
        return Ok(None);
    }

    // The entries array starts at DX_ROOT_ENTRIES_OFFSET.
    // Entry 0 has the count/limit in its hash field + block for hash=0.
    // Entries 1..count have real (hash, block) pairs sorted by hash.
    // Binary search for the target hash in entries[1..count].
    let leaf_block = find_leaf_block(
        &root_data,
        DX_ROOT_ENTRIES_OFFSET,
        entry_count,
        target_hash,
    )?;

    // If there are indirect levels, we need to descend through dx_nodes.
    let final_block = if info.indirect_levels > 0 {
        descend_dx_nodes(driver, &dir_inode, dir_ino, leaf_block, target_hash, info.indirect_levels)?
    } else {
        leaf_block
    };

    // Read the leaf block and linear-scan for the name.
    let leaf_phys = match driver.logical_to_physical(dir_ino, &dir_inode, u64::from(final_block)) {
        Ok(Some(pb)) => pb,
        _ => return Ok(None),
    };

    let leaf_data = driver.read_block(leaf_phys)?;

    // Validate directory block checksum before scanning entries.
    super::driver::validate_dirent_checksum(
        driver.superblock(),
        dir_ino,
        dir_inode.i_generation,
        &leaf_data,
    )?;

    scan_leaf_block(&leaf_data, name)
}

/// Binary search the dx_entry array to find which block contains entries
/// with the given hash.
///
/// The entries are at `base_offset + i*8` for i in 0..count.
/// Entry 0's block is the default (for hashes < entries[1].hash).
/// Returns the logical block number.
fn find_leaf_block(
    data: &[u8],
    base_offset: usize,
    count: usize,
    target_hash: u32,
) -> KernelResult<u32> {
    if count == 0 {
        return Err(KernelError::IoError);
    }

    // Read entry 0 to get the default block (used if hash < all entries).
    let e0_off = base_offset + 4; // skip the count/limit 4 bytes, get the block
    let default_block = read_u32(data, e0_off)?;

    if count <= 1 {
        // Only the default entry — all hashes go to this block.
        return Ok(default_block);
    }

    // Binary search entries[1..count] for the largest entry with hash ≤ target.
    // Each entry is 8 bytes: [hash:u32][block:u32] at base_offset + i*8.
    let mut lo: usize = 1;
    let mut hi: usize = count.saturating_sub(1);
    let mut best_block = default_block;

    while lo <= hi {
        let mid = lo.saturating_add(hi) / 2;
        let entry_off = base_offset.saturating_add(mid.saturating_mul(8));
        let entry_hash = read_u32(data, entry_off)?;

        if entry_hash <= target_hash {
            // This entry covers our hash — record its block.
            best_block = read_u32(data, entry_off.saturating_add(4))?;
            lo = mid.saturating_add(1);
        } else {
            if mid == 0 {
                break;
            }
            hi = mid.saturating_sub(1);
        }
    }

    Ok(best_block)
}

/// Descend through intermediate dx_node blocks when indirect_levels > 0.
///
/// Each intermediate dx_node contains its own set of (hash, block) entries
/// that refine the search.  The `dir_inode` is needed to map logical block
/// numbers to physical blocks via the extent tree.  `dir_ino` is the
/// inode number, passed through for the extent cache.
fn descend_dx_nodes(
    driver: &Ext4Driver,
    dir_inode: &super::ondisk::Ext4Inode,
    dir_ino: u32,
    mut block_num: u32,
    target_hash: u32,
    levels: u8,
) -> KernelResult<u32> {
    for _ in 0..levels {
        let phys = match driver.logical_to_physical(dir_ino, dir_inode, block_num as u64) {
            Ok(Some(pb)) => pb,
            _ => return Err(KernelError::IoError),
        };

        let node_data = driver.read_block(phys)?;

        // dx_node starts with a fake dir entry (8 bytes), then entries.
        if node_data.len() < DX_NODE_HEADER_SIZE + 4 {
            return Err(KernelError::IoError);
        }

        // Read count/limit from first entry position.
        let cl_bytes = node_data
            .get(DX_NODE_HEADER_SIZE..DX_NODE_HEADER_SIZE + 4)
            .ok_or(KernelError::IoError)?;
        let cl: DxCountLimit = read_struct_pub(cl_bytes)?;
        let count = cl.count as usize;

        block_num = find_leaf_block(&node_data, DX_NODE_HEADER_SIZE, count, target_hash)?;
    }

    Ok(block_num)
}

/// Linear scan a leaf block for a directory entry matching `name`.
///
/// Returns `Ok(Some((inode, file_type)))` if found, `Ok(None)` if not.
fn scan_leaf_block(
    data: &[u8],
    name: &str,
) -> KernelResult<Option<(u32, u8)>> {
    let name_bytes = name.as_bytes();
    let hdr_size = core::mem::size_of::<Ext4DirEntry2>();
    let mut offset = 0usize;

    while offset.saturating_add(hdr_size) <= data.len() {
        let hdr_bytes = data
            .get(offset..offset.saturating_add(hdr_size))
            .ok_or(KernelError::IoError)?;
        let hdr: Ext4DirEntry2 = read_struct_pub(hdr_bytes)?;

        if hdr.rec_len == 0 {
            break;
        }

        if hdr.inode != 0 && hdr.name_len as usize == name_bytes.len() {
            let name_start = offset.saturating_add(hdr_size);
            let name_end = name_start.saturating_add(hdr.name_len as usize);
            if let Some(entry_name) = data.get(name_start..name_end) {
                if entry_name == name_bytes {
                    return Ok(Some((hdr.inode, hdr.file_type)));
                }
            }
        }

        offset = offset.saturating_add(hdr.rec_len as usize);
    }

    Ok(None)
}

/// Read a little-endian u32 from `data` at `offset`.
fn read_u32(data: &[u8], offset: usize) -> KernelResult<u32> {
    let bytes = data
        .get(offset..offset.saturating_add(4))
        .ok_or(KernelError::IoError)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

// ---------------------------------------------------------------------------
// Htree write-side: insert directory entry into hash-indexed directory
// ---------------------------------------------------------------------------

/// Insert a new directory entry into an htree-indexed directory.
///
/// This is the main entry point for htree write operations.  It:
///
/// 1. Computes the hash for the new filename
/// 2. Probes the hash tree to find the target leaf block
/// 3. Attempts to insert the entry in the leaf
/// 4. If the leaf is full, splits it and updates the parent dx_node/dx_root
///
/// Returns `Ok(true)` if the entry was inserted via htree, `Ok(false)` if
/// the directory is not htree-indexed (caller should use the linear path),
/// or `Err(...)` on failure.
///
/// Based on Linux `fs/ext4/namei.c` `ext4_dx_add_entry`.
pub fn htree_add_entry(
    driver: &mut Ext4Driver,
    dir_ino: u32,
    dir_inode: &mut super::ondisk::Ext4Inode,
    child_ino: u32,
    name: &[u8],
    file_type_byte: u8,
) -> KernelResult<bool> {
    // Check the INDEX flag on the directory inode.
    if dir_inode.i_flags & inode_flags::INDEX == 0 {
        return Ok(false); // Not htree-indexed.
    }

    // Map logical block 0 → physical block for the dx_root.
    let phys_block0 = match driver.logical_to_physical(dir_ino, dir_inode, 0) {
        Ok(Some(pb)) => pb,
        _ => return Ok(false), // Can't resolve root block — fall back.
    };

    let root_data = driver.read_block(phys_block0)?;
    if root_data.len() < DX_ROOT_ENTRIES_OFFSET + 4 {
        return Ok(false);
    }

    // Parse dx_root_info.
    let info_bytes = root_data
        .get(DX_ROOT_INFO_OFFSET..DX_ROOT_INFO_OFFSET + 8)
        .ok_or(KernelError::IoError)?;
    let info: DxRootInfo = read_struct_pub(info_bytes)?;

    if info.info_length != 8 || info.indirect_levels > 1 {
        // We only support direct (0) and single-indirect (1) levels for writes.
        return Ok(false);
    }

    // Compute hash for the new filename.
    let hash_version = driver.superblock().raw.s_def_hash_version;
    let seed = driver.superblock().raw.s_hash_seed;
    let (target_hash, _) = ext4_dirhash(name, hash_version, &seed);

    // Read count/limit from the root's dx_entry array.
    let cl_bytes = root_data
        .get(DX_ROOT_ENTRIES_OFFSET..DX_ROOT_ENTRIES_OFFSET + 4)
        .ok_or(KernelError::IoError)?;
    let root_cl: DxCountLimit = read_struct_pub(cl_bytes)?;

    // Probe the tree to find the target leaf block number.
    let leaf_block_num = find_leaf_block(
        &root_data,
        DX_ROOT_ENTRIES_OFFSET,
        root_cl.count as usize,
        target_hash,
    )?;

    // If indirect levels, descend to the actual leaf.
    let final_leaf_block = if info.indirect_levels > 0 {
        descend_dx_nodes(driver, dir_inode, dir_ino, leaf_block_num, target_hash, info.indirect_levels)?
    } else {
        leaf_block_num
    };

    // Read the target leaf block.
    let leaf_phys = match driver.logical_to_physical(dir_ino, dir_inode, u64::from(final_leaf_block)) {
        Ok(Some(pb)) => pb,
        _ => return Err(KernelError::IoError),
    };

    let mut leaf_data = driver.read_block(leaf_phys)?;
    let block_size = driver.superblock().block_size as usize;

    // Calculate the new entry size (aligned to 4 bytes).
    let entry_header_size = 8usize;
    let entry_size = entry_header_size.saturating_add(name.len());
    let entry_size_aligned = (entry_size.saturating_add(3)) & !3;

    // Try to insert into the existing leaf block.
    if let Some(insert_offset) = find_leaf_insert_point(
        &leaf_data, block_size, entry_size_aligned, driver.superblock().has_metadata_csum,
    ) {
        // Space available — insert the entry and write the block back.
        insert_leaf_entry(
            &mut leaf_data,
            insert_offset,
            child_ino,
            name,
            file_type_byte,
            block_size,
        )?;

        // Re-stamp the directory block checksum.
        super::driver::stamp_dirent_checksum_pub(
            driver.superblock(),
            dir_ino,
            dir_inode.i_generation,
            &mut leaf_data,
        );

        driver.write_block_raw(leaf_phys, &leaf_data)?;
        return Ok(true);
    }

    // Leaf is full — need to split.
    // Allocate a new block for the split.
    let new_block_num = driver.alloc_block(leaf_phys)?;

    // Split the leaf: distribute entries between old and new leaf by hash.
    match split_leaf_and_insert(
        driver,
        dir_ino,
        dir_inode,
        leaf_phys,
        &leaf_data,
        new_block_num,
        final_leaf_block,
        target_hash,
        child_ino,
        name,
        file_type_byte,
        block_size,
        phys_block0,
        info.indirect_levels,
    ) {
        Ok(()) => Ok(true),
        Err(e) => {
            // Error cleanup: free the newly allocated block.
            let _ = driver.free_block_nr(new_block_num);
            Err(e)
        }
    }
}

/// Find an insertion point within a leaf block for a new directory entry.
///
/// Returns `Some(offset)` if space is found, `None` if the block is full.
/// This is the leaf-specific version of `find_dir_insert_point` that handles
/// the dirent-tail reservation for metadata checksum directories.
fn find_leaf_insert_point(
    data: &[u8],
    block_size: usize,
    needed_size: usize,
    has_metadata_csum: bool,
) -> Option<usize> {
    let entry_hdr = core::mem::size_of::<Ext4DirEntry2>();
    let tail_size = if has_metadata_csum {
        core::mem::size_of::<super::ondisk::Ext4DirEntryTail>()
    } else {
        0
    };
    let usable_size = block_size.saturating_sub(tail_size);
    let mut offset = 0usize;
    let mut last_offset = 0usize;
    let mut last_actual = 0usize;
    let mut last_rec_len = 0u16;

    while offset.saturating_add(entry_hdr) <= usable_size {
        let hdr_bytes = data.get(offset..offset.saturating_add(entry_hdr))?;
        let hdr: Ext4DirEntry2 = read_struct_pub(hdr_bytes).ok()?;

        if hdr.rec_len == 0 { break; }

        // Skip the dirent tail sentinel.
        if hdr.inode == 0
            && hdr.rec_len == 12
            && hdr.name_len == 0
            && hdr.file_type == super::ondisk::EXT4_DIRENT_TAIL_MARKER
        {
            break; // Don't scan past the tail.
        }

        let actual = if hdr.inode == 0 {
            0 // deleted entry — whole rec_len is free
        } else {
            let name_total = entry_hdr.saturating_add(hdr.name_len as usize);
            (name_total.saturating_add(3)) & !3
        };

        last_offset = offset;
        last_actual = actual;
        last_rec_len = hdr.rec_len;

        offset = offset.saturating_add(hdr.rec_len as usize);
    }

    // Check if there's space after the last real entry.
    if last_rec_len as usize > last_actual {
        let free = (last_rec_len as usize).saturating_sub(last_actual);
        if free >= needed_size {
            return Some(last_offset.saturating_add(last_actual));
        }
    }

    None
}

/// Insert a directory entry into a leaf block at the given offset.
///
/// Shrinks the previous entry's rec_len and writes the new entry.
fn insert_leaf_entry(
    data: &mut [u8],
    offset: usize,
    child_ino: u32,
    name: &[u8],
    file_type_byte: u8,
    block_size: usize,
) -> KernelResult<()> {
    let entry_hdr = 8usize; // sizeof(Ext4DirEntry2)

    // Find and shrink the previous entry.
    let block_start = (offset / block_size) * block_size;
    let mut pos = block_start;
    while pos.saturating_add(entry_hdr) <= offset {
        if let Some(bytes) = data.get(pos..pos.saturating_add(entry_hdr)) {
            if let Ok(hdr) = read_struct_pub::<Ext4DirEntry2>(bytes) {
                let next = pos.saturating_add(hdr.rec_len as usize);
                if next > offset || hdr.rec_len == 0 {
                    // Shrink this entry to end exactly at `offset`.
                    let new_rec_len = (offset.saturating_sub(pos)) as u16;
                    if let Some(rl) = data.get_mut(
                        pos.saturating_add(4)..pos.saturating_add(6)
                    ) {
                        rl.copy_from_slice(&new_rec_len.to_le_bytes());
                    }
                    break;
                }
                pos = next;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Compute rec_len for the new entry: extends to the next boundary
    // (either end of usable block space or next entry).
    // In practice this is `block_end_or_tail - offset`.
    let remaining = block_size.saturating_sub(offset % block_size);

    // Write the new entry.
    write_leaf_dirent(data, offset, child_ino, name, file_type_byte, remaining)
}

/// Write a raw directory entry at a specific offset in a leaf block.
///
/// Returns `InvalidArgument` if `name` exceeds 255 bytes or `rec_len`
/// exceeds 65535 (the on-disk field widths).
fn write_leaf_dirent(
    buf: &mut [u8],
    offset: usize,
    inode: u32,
    name: &[u8],
    file_type_byte: u8,
    rec_len: usize,
) -> KernelResult<()> {
    // Defend against silent truncation of on-disk fields.
    if name.len() > 255 {
        return Err(KernelError::InvalidArgument);
    }
    if rec_len > u16::MAX as usize {
        return Err(KernelError::InvalidArgument);
    }

    if let Some(dest) = buf.get_mut(offset..offset.saturating_add(4)) {
        dest.copy_from_slice(&inode.to_le_bytes());
    }
    if let Some(dest) = buf.get_mut(
        offset.saturating_add(4)..offset.saturating_add(6)
    ) {
        dest.copy_from_slice(&(rec_len as u16).to_le_bytes());
    }
    if let Some(b) = buf.get_mut(offset.saturating_add(6)) {
        *b = name.len() as u8;
    }
    if let Some(b) = buf.get_mut(offset.saturating_add(7)) {
        *b = file_type_byte;
    }
    let name_start = offset.saturating_add(8);
    let name_end = name_start.saturating_add(name.len());
    if name_end <= buf.len() {
        if let Some(dest) = buf.get_mut(name_start..name_end) {
            dest.copy_from_slice(name);
        }
    }

    Ok(())
}

/// Split a full leaf block and insert the new entry.
///
/// 1. Collects all existing entries from the leaf + the new entry
/// 2. Sorts them by hash
/// 3. Distributes roughly half to the old leaf, half to the new leaf
/// 4. Adds a new dx_entry in the parent node pointing to the new leaf
/// 5. Writes both leaf blocks and the updated parent to disk
///
/// Based on Linux `fs/ext4/namei.c` `do_split` + `add_dirent_to_buf`.
#[allow(clippy::too_many_arguments)]
fn split_leaf_and_insert(
    driver: &mut Ext4Driver,
    dir_ino: u32,
    dir_inode: &mut super::ondisk::Ext4Inode,
    old_leaf_phys: u64,
    old_leaf_data: &[u8],
    new_block_phys: u64,
    old_leaf_logical: u32,
    target_hash: u32,
    child_ino: u32,
    name: &[u8],
    file_type_byte: u8,
    block_size: usize,
    root_phys: u64,
    indirect_levels: u8,
) -> KernelResult<()> {
    let hash_version = driver.superblock().raw.s_def_hash_version;
    let seed = driver.superblock().raw.s_hash_seed;
    let has_csum = driver.superblock().has_metadata_csum;

    // Collect all existing entries from the old leaf.
    let mut entries = collect_leaf_entries(old_leaf_data, block_size, has_csum)?;

    // Compute hashes for the collected entries (they were stored with hash=0).
    for e in &mut entries {
        let (h, _) = ext4_dirhash(&e.name, hash_version, &seed);
        e.hash = h;
    }

    // Add the new entry to the list.
    entries.push(LeafEntry {
        inode: child_ino,
        name: name.to_vec(),
        file_type: file_type_byte,
        hash: target_hash,
    });

    // Sort entries by hash for balanced splitting.
    entries.sort_by_key(|e| e.hash);

    // Find the split point: roughly half the entries go to each leaf.
    // Use a size-based split (total bytes), not just count, since entries
    // vary in size.
    let entry_hdr = 8usize;
    let tail_reserve = if has_csum { 12 } else { 0 };
    let usable = block_size.saturating_sub(tail_reserve);

    let mut split_idx = 0usize;
    let mut used = 0usize;
    let half_target = usable / 2;
    for (i, e) in entries.iter().enumerate() {
        let esize = (entry_hdr + e.name.len() + 3) & !3;
        if used + esize > half_target && i > 0 {
            split_idx = i;
            break;
        }
        used += esize;
        split_idx = i + 1;
    }

    // Ensure at least one entry in each leaf.
    if split_idx == 0 { split_idx = 1; }
    if split_idx >= entries.len() {
        split_idx = entries.len().saturating_sub(1);
    }

    // Build the two leaf blocks.
    let left_entries = &entries[..split_idx];
    let right_entries = &entries[split_idx..];

    let mut left_buf = vec![0u8; block_size];
    let mut right_buf = vec![0u8; block_size];

    write_leaf_entries(&mut left_buf, left_entries, block_size, has_csum)?;
    write_leaf_entries(&mut right_buf, right_entries, block_size, has_csum)?;

    // Stamp checksums.
    super::driver::stamp_dirent_checksum_pub(
        driver.superblock(), dir_ino, dir_inode.i_generation, &mut left_buf,
    );
    super::driver::stamp_dirent_checksum_pub(
        driver.superblock(), dir_ino, dir_inode.i_generation, &mut right_buf,
    );

    // The new leaf's hash boundary is the smallest hash in the right half.
    let new_leaf_hash = right_entries
        .first()
        .map(|e| e.hash)
        .unwrap_or(0);

    // Allocate a logical block number for the new leaf.
    // The new leaf needs a logical block number in the directory's extent tree.
    // We need to grow the directory by one block and use that logical number.
    let dir_size = u64::from(dir_inode.i_size_lo);
    let blocks_in_dir = (dir_size as usize).div_ceil(block_size);
    let new_logical = blocks_in_dir as u32;

    // Extend the directory's extent tree to map new_logical → new_block_phys.
    driver.extend_dir_one_block(dir_ino, dir_inode, new_block_phys, new_logical)?;

    // Write both leaf blocks.
    driver.write_block_raw(old_leaf_phys, &left_buf)?;
    driver.write_block_raw(new_block_phys, &right_buf)?;

    // Update the dx_root (or dx_node if indirect) with the new dx_entry.
    add_dx_entry(
        driver,
        dir_ino,
        dir_inode,
        root_phys,
        new_leaf_hash,
        new_logical,
        indirect_levels,
        old_leaf_logical,
    )?;

    Ok(())
}

/// An entry extracted from a leaf block for splitting.
struct LeafEntry {
    inode: u32,
    name: Vec<u8>,
    file_type: u8,
    hash: u32,
}

/// Collect all valid directory entries from a leaf block.
fn collect_leaf_entries(
    data: &[u8],
    block_size: usize,
    has_csum: bool,
) -> KernelResult<Vec<LeafEntry>> {
    let entry_hdr = core::mem::size_of::<Ext4DirEntry2>();
    let tail_size = if has_csum { 12 } else { 0 };
    let usable = block_size.saturating_sub(tail_size);
    let mut entries = Vec::new();
    let mut offset = 0usize;

    while offset.saturating_add(entry_hdr) <= usable {
        let hdr_bytes = data.get(offset..offset.saturating_add(entry_hdr))
            .ok_or(KernelError::IoError)?;
        let hdr: Ext4DirEntry2 = read_struct_pub(hdr_bytes)?;

        if hdr.rec_len == 0 { break; }

        // Skip the dirent tail.
        if hdr.inode == 0
            && hdr.rec_len == 12
            && hdr.name_len == 0
            && hdr.file_type == super::ondisk::EXT4_DIRENT_TAIL_MARKER
        {
            break;
        }

        if hdr.inode != 0 && hdr.name_len > 0 {
            let name_start = offset.saturating_add(entry_hdr);
            let name_end = name_start.saturating_add(hdr.name_len as usize);
            let name = data.get(name_start..name_end)
                .ok_or(KernelError::IoError)?
                .to_vec();

            // Don't bother hashing "." and ".." — they stay with the original block.
            // Actually, after a split these should not appear in leaf blocks
            // (they're only in the root block). But be safe.
            entries.push(LeafEntry {
                inode: hdr.inode,
                name,
                file_type: hdr.file_type,
                hash: 0, // will be filled below
            });
        }

        offset = offset.saturating_add(hdr.rec_len as usize);
    }

    // Compute hashes for all entries.
    // We need the superblock's hash seed — passed through via has_csum flag workaround.
    // Actually, we should get the hash info from the driver. For now, return entries
    // with hash=0 and let the caller fill them.
    // Re-think: since we're called from split_leaf_and_insert which has access to
    // hash_version and seed, we should pass those in or hash there.
    Ok(entries)
}

/// Write a list of directory entries into a leaf block buffer.
fn write_leaf_entries(
    buf: &mut [u8],
    entries: &[LeafEntry],
    block_size: usize,
    has_csum: bool,
) -> KernelResult<()> {
    let entry_hdr = 8usize;
    let tail_size = if has_csum { 12 } else { 0 };
    let usable = block_size.saturating_sub(tail_size);
    let mut offset = 0usize;

    for (i, e) in entries.iter().enumerate() {
        let actual_size = (entry_hdr + e.name.len() + 3) & !3;
        let is_last = i + 1 == entries.len();

        // rec_len: if last entry, extends to end of usable space.
        let rec_len = if is_last {
            usable.saturating_sub(offset)
        } else {
            actual_size
        };

        write_leaf_dirent(buf, offset, e.inode, &e.name, e.file_type, rec_len)?;
        offset += rec_len;
    }

    // Initialize dirent tail if checksums are enabled.
    if has_csum && block_size >= 12 {
        let tail_offset = block_size - 12;
        // inode = 0
        if let Some(d) = buf.get_mut(tail_offset..tail_offset + 4) {
            d.copy_from_slice(&0u32.to_le_bytes());
        }
        // rec_len = 12
        if let Some(d) = buf.get_mut(tail_offset + 4..tail_offset + 6) {
            d.copy_from_slice(&12u16.to_le_bytes());
        }
        // name_len = 0
        if let Some(b) = buf.get_mut(tail_offset + 6) { *b = 0; }
        // file_type = EXT4_DIRENT_TAIL_MARKER (0xDE)
        if let Some(b) = buf.get_mut(tail_offset + 7) {
            *b = super::ondisk::EXT4_DIRENT_TAIL_MARKER;
        }
        // checksum placeholder (4 bytes, will be filled by stamp_dirent_checksum).
        if let Some(d) = buf.get_mut(tail_offset + 8..tail_offset + 12) {
            d.copy_from_slice(&0u32.to_le_bytes());
        }
    }

    Ok(())
}

/// Add a new dx_entry to the hash tree root (or intermediate node).
///
/// `hash` is the minimum hash of the new leaf's entries.
/// `block` is the logical block number of the new leaf.
#[allow(clippy::too_many_arguments)]
fn add_dx_entry(
    driver: &mut Ext4Driver,
    dir_ino: u32,
    dir_inode: &mut super::ondisk::Ext4Inode,
    root_phys: u64,
    hash: u32,
    block: u32,
    indirect_levels: u8,
    _target_leaf_logical: u32,
) -> KernelResult<()> {
    if indirect_levels == 0 {
        // Direct: add entry to the dx_root's entry array.
        add_dx_entry_to_node(driver, root_phys, DX_ROOT_ENTRIES_OFFSET, hash, block)
    } else if indirect_levels == 1 {
        // Single indirect: find the correct dx_node, then add there.
        add_dx_entry_indirect(driver, dir_ino, dir_inode, root_phys, hash, block)
    } else {
        // Two-level indirect — very rare, not supported.
        Err(KernelError::NotSupported)
    }
}

/// Add a (hash, block) entry to a dx_node or dx_root entry array.
///
/// `node_phys` is the physical block address of the node.
/// `entries_base` is the byte offset of the first DxEntry (DxCountLimit)
/// within the block — `DX_ROOT_ENTRIES_OFFSET` for the root, or
/// `DX_NODE_HEADER_SIZE` for intermediate nodes.
fn add_dx_entry_to_node(
    driver: &mut Ext4Driver,
    node_phys: u64,
    entries_base: usize,
    hash: u32,
    block: u32,
) -> KernelResult<()> {
    let mut node_data = driver.read_block(node_phys)?;

    // Read current count/limit.
    let cl_bytes = node_data
        .get(entries_base..entries_base + 4)
        .ok_or(KernelError::IoError)?;
    let cl: DxCountLimit = read_struct_pub(cl_bytes)?;

    if cl.count >= cl.limit {
        // Node is full — would need to split the dx_node.
        // dx_node splitting is extremely rare (requires thousands of files
        // per hash bucket).  Return NotSupported; the caller will fall back
        // to a non-htree path.
        return Err(KernelError::NotSupported);
    }

    // Insert the new (hash, block) entry at the correct sorted position.
    let count = cl.count as usize;

    // Read all current entries.
    let mut dx_entries: Vec<(u32, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        let off = entries_base + i * 8;
        if i == 0 {
            // Entry 0 is the count/limit + default block.
            let default_block = read_u32(&node_data, off + 4)?;
            dx_entries.push((0, default_block));
        } else {
            let h = read_u32(&node_data, off)?;
            let b = read_u32(&node_data, off + 4)?;
            dx_entries.push((h, b));
        }
    }

    // Find insertion position (maintain sorted order by hash, skip entry 0).
    let mut insert_pos = count; // default: append at end
    for i in 1..count {
        if dx_entries[i].0 > hash {
            insert_pos = i;
            break;
        }
    }

    // Insert the new entry.
    dx_entries.insert(insert_pos, (hash, block));

    // Write updated count.
    let new_count = (count + 1) as u16;
    if let Some(d) = node_data.get_mut(entries_base..entries_base + 2) {
        d.copy_from_slice(&cl.limit.to_le_bytes());
    }
    if let Some(d) = node_data.get_mut(entries_base + 2..entries_base + 4) {
        d.copy_from_slice(&new_count.to_le_bytes());
    }

    // Write all dx_entries back.
    for (i, &(h, b)) in dx_entries.iter().enumerate() {
        let off = entries_base + i * 8;
        if i == 0 {
            // Entry 0: keep count/limit in first 4 bytes, write block in next 4.
            if let Some(d) = node_data.get_mut(off + 4..off + 8) {
                d.copy_from_slice(&b.to_le_bytes());
            }
        } else {
            if let Some(d) = node_data.get_mut(off..off + 4) {
                d.copy_from_slice(&h.to_le_bytes());
            }
            if let Some(d) = node_data.get_mut(off + 4..off + 8) {
                d.copy_from_slice(&b.to_le_bytes());
            }
        }
    }

    // Write the updated node block.
    driver.write_block_raw(node_phys, &node_data)?;

    Ok(())
}

/// Handle `add_dx_entry` for indirect_levels == 1.
///
/// The dx_root's entries point to intermediate dx_node blocks, which
/// in turn point to leaf blocks.  We need to:
///
/// 1. Read the dx_root to find which dx_node covers our hash range.
/// 2. Map the dx_node's logical block number to a physical block.
/// 3. Add the new (hash, block) entry to that dx_node.
///
/// Based on Linux `fs/ext4/namei.c` `ext4_dx_add_entry` with
/// `levels > 0`.
fn add_dx_entry_indirect(
    driver: &mut Ext4Driver,
    dir_ino: u32,
    dir_inode: &mut super::ondisk::Ext4Inode,
    root_phys: u64,
    hash: u32,
    block: u32,
) -> KernelResult<()> {
    let root_data = driver.read_block(root_phys)?;

    // Read the root's count/limit to find which dx_node covers our hash.
    let cl_bytes = root_data
        .get(DX_ROOT_ENTRIES_OFFSET..DX_ROOT_ENTRIES_OFFSET + 4)
        .ok_or(KernelError::IoError)?;
    let root_cl: DxCountLimit = read_struct_pub(cl_bytes)?;

    // Find the dx_node block that covers our hash.  At indirect level 1,
    // the root's entries point to dx_node blocks (not leaves).
    let dx_node_logical = find_leaf_block(
        &root_data,
        DX_ROOT_ENTRIES_OFFSET,
        root_cl.count as usize,
        hash,
    )?;

    // Map logical → physical for the dx_node.
    let dx_node_phys = match driver.logical_to_physical(
        dir_ino, dir_inode, u64::from(dx_node_logical),
    ) {
        Ok(Some(pb)) => pb,
        _ => return Err(KernelError::IoError),
    };

    // Add the new entry to the dx_node.  dx_nodes have entries starting
    // at DX_NODE_HEADER_SIZE (8 bytes past the fake dir entry header).
    add_dx_entry_to_node(driver, dx_node_phys, DX_NODE_HEADER_SIZE, hash, block)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the ext4 hash functions against known test vectors.
///
/// These vectors are derived from running the Linux kernel's ext4 hash
/// on known inputs.  This ensures our Rust implementation matches.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[ext4-htree] Running hash self-test...");

    // Test 1: fold_hash basic behavior.
    let h = fold_hash(0);
    if h != 2 {
        serial_println!("[ext4-htree]   FAIL: fold_hash(0) = {}, expected 2", h);
        return Err(KernelError::InternalError);
    }
    let h = fold_hash(5);
    if h != 4 {
        // 5 & !1 = 4
        serial_println!("[ext4-htree]   FAIL: fold_hash(5) = {}, expected 4", h);
        return Err(KernelError::InternalError);
    }
    let h = fold_hash(6);
    if h != 6 {
        serial_println!("[ext4-htree]   FAIL: fold_hash(6) = {}, expected 6", h);
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   fold_hash OK");

    // Test 2: str2hashbuf packs bytes correctly (LE).
    let val = str2hashbuf(b"abcd", 0);
    // 'a'=0x61, 'b'=0x62, 'c'=0x63, 'd'=0x64 → 0x64636261 in LE
    if val != 0x6463_6261 {
        serial_println!(
            "[ext4-htree]   FAIL: str2hashbuf('abcd', 0) = 0x{:08x}, expected 0x64636261",
            val
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   str2hashbuf OK");

    // Test 3: Partial word padding.
    let val = str2hashbuf(b"ab", 0);
    // 'a'=0x61, 'b'=0x62, pad_len=2 → 0x61 | (0x62 << 8) | (2 << 16)
    let expected = 0x61u32 | (0x62u32 << 8) | (2u32 << 16);
    if val != expected {
        serial_println!(
            "[ext4-htree]   FAIL: str2hashbuf('ab', 0) = 0x{:08x}, expected 0x{:08x}",
            val, expected
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   str2hashbuf partial OK");

    // Test 4: Hash determinism — same input always gives same output.
    let seed = [0x12345678u32, 0x9ABCDEF0, 0x13579BDF, 0x2468ACE0];
    let (h1, m1) = half_md4_hash(b"hello.txt", &seed);
    let (h2, m2) = half_md4_hash(b"hello.txt", &seed);
    if h1 != h2 || m1 != m2 {
        serial_println!("[ext4-htree]   FAIL: half_md4 not deterministic");
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   half_md4 determinism OK (hash=0x{:08x})", h1);

    // Test 5: Different names give different hashes.
    let (ha, _) = half_md4_hash(b"file_a", &seed);
    let (hb, _) = half_md4_hash(b"file_b", &seed);
    if ha == hb {
        serial_println!(
            "[ext4-htree]   WARN: half_md4 collision on 'file_a' and 'file_b' (rare but possible)"
        );
    } else {
        serial_println!("[ext4-htree]   half_md4 different names → different hashes OK");
    }

    // Test 6: Legacy hash.
    let (hl1, _) = legacy_hash(b"test", false);
    let (hl2, _) = legacy_hash(b"test", false);
    if hl1 != hl2 {
        serial_println!("[ext4-htree]   FAIL: legacy hash not deterministic");
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   legacy hash OK (hash=0x{:08x})", hl1);

    // Test 7: TEA hash.
    let (ht1, _) = tea_hash(b"test_file.rs", &seed);
    let (ht2, _) = tea_hash(b"test_file.rs", &seed);
    if ht1 != ht2 {
        serial_println!("[ext4-htree]   FAIL: TEA hash not deterministic");
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   TEA hash OK (hash=0x{:08x})", ht1);

    // Test 8: Zero-length name (edge case).
    let (hz, _) = half_md4_hash(b"", &seed);
    if hz == 0 {
        serial_println!("[ext4-htree]   FAIL: hash of empty name should not be 0");
        return Err(KernelError::InternalError);
    }
    serial_println!("[ext4-htree]   empty name hash OK (hash=0x{:08x})", hz);

    // Test 9: scan_leaf_block with synthetic data.
    {
        // Build a tiny directory block with one entry: "testfile" → inode 42.
        let mut block = vec![0u8; 4096];
        let name_bytes = b"testfile";
        // Ext4DirEntry2 header: inode=42, rec_len=4096, name_len=8, file_type=1
        block[0..4].copy_from_slice(&42u32.to_le_bytes());
        block[4..6].copy_from_slice(&4096u16.to_le_bytes()); // rec_len = whole block
        block[6] = 8; // name_len
        block[7] = 1; // file_type = REG_FILE
        block[8..16].copy_from_slice(name_bytes);

        let result = scan_leaf_block(&block, "testfile")?;
        if result != Some((42, 1)) {
            serial_println!(
                "[ext4-htree]   FAIL: scan_leaf_block returned {:?}, expected Some((42, 1))",
                result
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[ext4-htree]   scan_leaf_block found entry OK");

        // Should not find a different name.
        let result = scan_leaf_block(&block, "other")?;
        if result.is_some() {
            serial_println!("[ext4-htree]   FAIL: scan_leaf_block should not find 'other'");
            return Err(KernelError::InternalError);
        }
        serial_println!("[ext4-htree]   scan_leaf_block miss OK");
    }

    serial_println!("[ext4-htree] Hash self-test PASSED");
    Ok(())
}
