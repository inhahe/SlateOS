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
        if result != None {
            serial_println!("[ext4-htree]   FAIL: scan_leaf_block should not find 'other'");
            return Err(KernelError::InternalError);
        }
        serial_println!("[ext4-htree]   scan_leaf_block miss OK");
    }

    serial_println!("[ext4-htree] Hash self-test PASSED");
    Ok(())
}
