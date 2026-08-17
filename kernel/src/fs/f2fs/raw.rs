//! F2FS on-disk primitives: little-endian scalars, geometry, and the name hash.
//!
//! As in the Btrfs and NTFS ports, nothing here is reinterpreted as a
//! `#[repr(C)]` struct via a pointer cast. Every field is read by explicit
//! offset out of a `Vec<u8>` that came from a device, because the on-disk
//! structures are packed to byte granularity and the buffer's alignment is the
//! allocator's business. F2FS makes the point more sharply than most: a
//! `f2fs_nat_entry` is **nine** bytes — one `u8` and two `u32` — so in a NAT
//! block every entry after the first is at a misaligned offset by
//! construction, and three out of every four hold both of their `u32`s
//! unaligned.
//!
//! # A note on units
//!
//! F2FS counts in three units and the code below is careful to say which:
//!
//! | unit | size | what it addresses |
//! |---|---|---|
//! | sector | 512 B | the `SectorSource` underneath us |
//! | block | 4096 B | everything in the format (`block_t` is a block index) |
//! | segment | 2 MiB (512 blocks) | the allocation/GC unit |
//!
//! A `block_t` in an inode, a NAT entry or the superblock is an index into the
//! whole *volume* in 4 KiB units, not an offset into any area — so turning one
//! into a byte offset is a single multiply and never involves a base address.

use crate::error::{KernelError, KernelResult};

/// Byte offset of the primary superblock from the start of the volume.
///
/// F2FS leaves the first 1 KiB alone for the same reason ext4 does: a
/// bootloader or partition table can live there without the filesystem
/// caring.
pub const SUPER_OFFSET: u64 = 1024;

/// `F2FS_SUPER_MAGIC`, at offset 0 of the superblock.
///
/// Also the seed for every metadata CRC in the format, which is why it is a
/// `u32` used in two places rather than only a signature.
pub const MAGIC: u32 = 0xF2F5_2010;

/// F2FS block size. Fixed by the format at 4 KiB; there is no log field to
/// honour beyond checking that the volume agrees.
pub const BLOCK_SIZE: usize = 4096;

/// `log2(BLOCK_SIZE)`, as stored in the superblock's `log_blocksize`.
pub const LOG_BLOCK_SIZE: u32 = 12;

/// Sectors per block: 4096 / 512.
pub const SECTORS_PER_BLOCK: u64 = 8;

/// Length of a `node_footer`, which sits at the end of *every* node block.
///
/// nid(4) + ino(4) + flag(4) + cp_ver(8) + next_blkaddr(4) = 24.
pub const NODE_FOOTER_LEN: usize = 24;

/// Byte offset of the `node_footer` within a 4 KiB node block.
pub const NODE_FOOTER_OFF: usize = BLOCK_SIZE - NODE_FOOTER_LEN;

/// Length of a `f2fs_nat_entry`: version(1) + ino(4) + block_addr(4).
///
/// Nine bytes, not twelve: the structure is `__packed` and the format really
/// does place 455 of them in a 4096-byte block with 1 byte left over.
pub const NAT_ENTRY_LEN: usize = 9;

/// NAT entries in one 4 KiB block: `4096 / 9`.
pub const NAT_ENTRIES_PER_BLOCK: u32 = 455;

/// Block address meaning "this block does not exist" — a hole.
pub const NULL_ADDR: u32 = 0;

/// Block address meaning "reserved but never written" — reads as zeroes.
///
/// Distinct from [`NULL_ADDR`] on disk and identical to it for a reader; both
/// are kept named because conflating them is how a driver ends up reading
/// block `0xFFFFFFFF` off the end of the device.
pub const NEW_ADDR: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Well-known inode numbers
// ---------------------------------------------------------------------------

/// Node-id 0 is the "no node" sentinel; a real nid is never zero.
pub const NULL_NID: u32 = 0;

// ---------------------------------------------------------------------------
// Inode geometry
// ---------------------------------------------------------------------------

/// Direct block pointers stored inside an inode block, before any subtraction
/// for the extra-isize area or an inline xattr.
pub const DEF_ADDRS_PER_INODE: u32 = 923;

/// Node ids stored inside an inode block: 2 direct, 2 indirect, 1 double.
pub const DEF_NIDS_PER_INODE: usize = 5;

/// Block pointers in a direct node block: `(4096 - 24) / 4`.
pub const ADDRS_PER_BLOCK: u32 = 1018;

/// Node ids in an indirect node block; the same count, for the same reason.
pub const NIDS_PER_BLOCK: u32 = 1018;

/// Byte offset of `i_addr[0]` within an inode block.
///
/// Everything before it is fixed-size: the mode/uid/gid/timestamps block, the
/// 255-byte `i_name`, `i_dir_level`, and the 12-byte cached `i_ext`.
pub const I_ADDR_OFF: usize = 360;

/// Byte offset of `i_nid[0]` within an inode block: `I_ADDR_OFF + 923 * 4`.
pub const I_NID_OFF: usize = 4052;

// ---------------------------------------------------------------------------
// `i_inline` flags
// ---------------------------------------------------------------------------

/// The inode carries its extended attributes inline.
pub const F2FS_INLINE_XATTR: u8 = 0x01;
/// The inode's file data is stored inside the inode block itself.
pub const F2FS_INLINE_DATA: u8 = 0x02;
/// The inode is a directory whose entries are stored inside the inode block.
pub const F2FS_INLINE_DENTRY: u8 = 0x04;
/// The inode carries the `i_extra_isize` field (and everything after it).
pub const F2FS_EXTRA_ATTR: u8 = 0x20;

/// `i_addr` slots reserved before the inline-data area begins.
pub const DEF_INLINE_RESERVED_SIZE: u32 = 1;

// ---------------------------------------------------------------------------
// Directory entry types (`f2fs_dir_entry::file_type`)
// ---------------------------------------------------------------------------

/// A regular file.
pub const F2FS_FT_REG_FILE: u8 = 1;
/// A directory.
pub const F2FS_FT_DIR: u8 = 2;
/// A character device.
pub const F2FS_FT_CHRDEV: u8 = 3;
/// A block device.
pub const F2FS_FT_BLKDEV: u8 = 4;
/// A FIFO.
pub const F2FS_FT_FIFO: u8 = 5;
/// A socket.
pub const F2FS_FT_SOCK: u8 = 6;
/// A symbolic link.
pub const F2FS_FT_SYMLINK: u8 = 7;

// ---------------------------------------------------------------------------
// Directory block geometry
// ---------------------------------------------------------------------------

/// Length of a `f2fs_dir_entry`: hash(4) + ino(4) + name_len(2) + type(1).
pub const DIR_ENTRY_LEN: usize = 11;

/// Bytes of filename storage per directory slot.
///
/// A name longer than this occupies several consecutive slots, and the entry
/// describing it lives in the *first* of them; the rest are marked used in the
/// bitmap but have no entry of their own.
pub const F2FS_SLOT_LEN: usize = 8;

/// Directory entries in one 4 KiB dentry block.
pub const NR_DENTRY_IN_BLOCK: usize = 214;

/// Bytes of validity bitmap in a dentry block: `ceil(214 / 8)`.
pub const DENTRY_BITMAP_SIZE: usize = 27;

/// Padding between the bitmap and the entry array in a dentry block.
///
/// `4096 - (11 + 8) * 214 - 27 = 3`. Not a design choice, just what is left
/// over — but a reader that omits it lands three bytes early on every entry.
pub const DENTRY_RESERVED_SIZE: usize = 3;

/// Maximum filename length F2FS stores.
pub const F2FS_NAME_LEN: usize = 255;

/// Deepest hash level a directory may grow to.
pub const MAX_DIR_HASH_DEPTH: u32 = 63;

/// Ceiling on buckets per level once the depth passes `MAX_DIR_HASH_DEPTH / 2`.
pub const MAX_DIR_BUCKETS: u32 = 1 << ((MAX_DIR_HASH_DEPTH / 2) - 1);

/// Bit the directory hash never sets, reserved to mark a colliding entry.
pub const F2FS_HASH_COL_BIT: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Little-endian scalar reads
// ---------------------------------------------------------------------------

/// Read a `u8` at `off`, or [`KernelError::InvalidArgument`] if out of range.
pub fn read_u8(buf: &[u8], off: usize) -> KernelResult<u8> {
    buf.get(off).copied().ok_or(KernelError::InvalidArgument)
}

/// Read a little-endian `u16` at `off`.
pub fn read_u16(buf: &[u8], off: usize) -> KernelResult<u16> {
    let end = off.checked_add(2).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 2] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u16::from_le_bytes(arr))
}

/// Read a little-endian `u32` at `off`.
pub fn read_u32(buf: &[u8], off: usize) -> KernelResult<u32> {
    let end = off.checked_add(4).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 4] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u32::from_le_bytes(arr))
}

/// Read a little-endian `u64` at `off`.
pub fn read_u64(buf: &[u8], off: usize) -> KernelResult<u64> {
    let end = off.checked_add(8).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let arr: [u8; 8] = s.try_into().map_err(|_| KernelError::InternalError)?;
    Ok(u64::from_le_bytes(arr))
}

/// Test bit `n` of a little-endian bitmap laid out LSB-first within each byte.
///
/// Returns `false` for an index past the end rather than erroring. Both
/// callers — the NAT version bitmap and the dentry validity bitmap — are
/// asking "is this slot in use?", and a slot that does not exist is not in
/// use; the bounds are checked by the caller that knows the real limit.
pub fn test_bit(bitmap: &[u8], n: usize) -> bool {
    let byte = n / 8;
    let bit = n % 8;
    bitmap.get(byte).is_some_and(|b| (b >> bit) & 1 == 1)
}

/// Convert a block index to a byte offset within the volume.
pub fn block_to_offset(block: u32) -> u64 {
    u64::from(block).saturating_mul(BLOCK_SIZE as u64)
}

// ---------------------------------------------------------------------------
// The directory name hash
// ---------------------------------------------------------------------------

/// The TEA round constant.
const DELTA: u32 = 0x9E37_79B9;

/// One TEA transform round-set over four input words.
///
/// Wrapping arithmetic throughout: TEA is *defined* modulo 2^32, so an
/// overflow here is the algorithm working, not a bug — which is why the
/// crate-wide `arithmetic_side_effects` lint is suppressed rather than
/// satisfied with `checked_add`.
#[allow(clippy::arithmetic_side_effects)]
fn tea_transform(buf: &mut [u32; 4], input: &[u32; 4]) {
    let mut sum: u32 = 0;
    let mut b0 = buf[0];
    let mut b1 = buf[1];
    let (a, b, c, d) = (input[0], input[1], input[2], input[3]);

    for _ in 0..16 {
        sum = sum.wrapping_add(DELTA);
        b0 = b0.wrapping_add(
            ((b1 << 4).wrapping_add(a)) ^ b1.wrapping_add(sum) ^ ((b1 >> 5).wrapping_add(b)),
        );
        b1 = b1.wrapping_add(
            ((b0 << 4).wrapping_add(c)) ^ b0.wrapping_add(sum) ^ ((b0 >> 5).wrapping_add(d)),
        );
    }

    buf[0] = buf[0].wrapping_add(b0);
    buf[1] = buf[1].wrapping_add(b1);
}

/// Pack up to `num * 4` bytes of `msg` into `num` big-endian-ish words.
///
/// The padding word is the name's *length* smeared across all four bytes, so
/// two names that differ only by trailing bytes the packer would otherwise
/// drop still hash differently. Faithful to Linux's `str2hashbuf`, including
/// its quirk of truncating `len` to `num * 4` after the pad has already been
/// computed from the full length.
#[allow(clippy::arithmetic_side_effects)]
fn str2hashbuf(msg: &[u8], len: usize, out: &mut [u32; 4], num: usize) {
    let pad_byte = (len & 0xFF) as u32;
    let pad = pad_byte | (pad_byte << 8) | (pad_byte << 16) | (pad_byte << 24);

    let take = core::cmp::min(len, num.saturating_mul(4));
    let mut val = pad;
    let mut slot = 0usize;
    let mut remaining = num;

    for i in 0..take {
        if i % 4 == 0 {
            val = pad;
        }
        let byte = msg.get(i).copied().unwrap_or(0);
        val = u32::from(byte).wrapping_add(val << 8);
        if i % 4 == 3 {
            if let Some(o) = out.get_mut(slot) {
                *o = val;
            }
            slot += 1;
            remaining = remaining.saturating_sub(1);
            val = pad;
        }
    }

    // Linux writes the partial word unconditionally once, then pads the rest.
    if remaining > 0 {
        if let Some(o) = out.get_mut(slot) {
            *o = val;
        }
        slot += 1;
        remaining -= 1;
    }
    while remaining > 0 {
        if let Some(o) = out.get_mut(slot) {
            *o = pad;
        }
        slot += 1;
        remaining -= 1;
    }
}

/// The F2FS directory name hash.
///
/// A TEA (Tiny Encryption Algorithm) hash with the MD4 initial vector, not any
/// kind of CRC — F2FS inherited it from ext3's legacy hash. Two properties
/// matter to a reader:
///
/// * **`.` and `..` hash to zero.** Linux special-cases them before hashing,
///   and a driver that does not will fail to find the parent of any directory
///   deep enough to have more than one hash level.
/// * **Bit 31 is always clear** ([`F2FS_HASH_COL_BIT`]). The filesystem
///   reserves it, so a hash that has it set can only have come from a reader
///   that forgot to mask.
///
/// Getting this wrong is quiet in exactly the way the Btrfs name hash is: every
/// checksum still verifies, every block still parses, and lookups simply miss —
/// a filesystem that mounts and appears empty.
pub fn dentry_hash(name: &[u8]) -> u32 {
    if name == b"." || name == b".." {
        return 0;
    }

    // The MD4 initial vector, which is where F2FS's hash gets its avalanche.
    let mut buf: [u32; 4] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    let mut off = 0usize;
    let mut len = name.len();

    loop {
        let mut input = [0u32; 4];
        let chunk = name.get(off..).unwrap_or(&[]);
        str2hashbuf(chunk, len, &mut input, 4);
        tea_transform(&mut buf, &input);
        off = off.saturating_add(16);
        if len <= 16 {
            break;
        }
        len = len.saturating_sub(16);
    }

    buf.first().copied().unwrap_or(0) & !F2FS_HASH_COL_BIT
}

// ---------------------------------------------------------------------------
// Directory hash-table geometry
// ---------------------------------------------------------------------------

/// Buckets at hash `level` for a directory whose `i_dir_level` is `dir_level`.
pub fn dir_buckets(level: u32, dir_level: u8) -> u32 {
    let combined = level.saturating_add(u32::from(dir_level));
    if combined < MAX_DIR_HASH_DEPTH / 2 {
        1u32.checked_shl(combined).unwrap_or(MAX_DIR_BUCKETS)
    } else {
        MAX_DIR_BUCKETS
    }
}

/// Blocks per bucket at hash `level`.
///
/// Two until the table is deep, then four — F2FS widens the bucket rather than
/// the table once doubling the bucket count stops paying.
pub fn bucket_blocks(level: u32) -> u32 {
    if level < MAX_DIR_HASH_DEPTH / 2 { 2 } else { 4 }
}

/// The first directory-relative block index of bucket `idx` at `level`.
///
/// The levels are laid out end to end in the directory's own address space, so
/// this is the sum of every shallower level's total size plus the offset of
/// the bucket within this one.
pub fn dir_block_index(level: u32, dir_level: u8, idx: u32) -> u32 {
    let mut bidx: u32 = 0;
    for i in 0..level {
        let per_level = dir_buckets(i, dir_level).saturating_mul(bucket_blocks(i));
        bidx = bidx.saturating_add(per_level);
    }
    bidx.saturating_add(idx.saturating_mul(bucket_blocks(level)))
}

/// How many consecutive slots a name of `len` bytes occupies.
pub fn slots_for_name(len: usize) -> usize {
    len.saturating_add(F2FS_SLOT_LEN.saturating_sub(1)) / F2FS_SLOT_LEN
}
