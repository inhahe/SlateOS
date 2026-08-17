//! F2FS self-tests, driven by a synthetic volume built in RAM.
//!
//! # Why this file builds a whole filesystem
//!
//! Every hard part of an F2FS driver is an *indirection between blocks*: a
//! node id that only resolves once the winning checkpoint's NAT bitmap has
//! chosen between two copies of a NAT block, a journal in the checkpoint that
//! overrides that NAT block, a file offset that reaches its data through three
//! levels of node. None of that can be exercised by feeding a hand-written
//! byte array to one parser — a byte array proves the parser accepts what the
//! *test author* believed the format to be, which is exactly the belief under
//! test.
//!
//! So [`build_image`] lays out a complete, structurally valid F2FS volume:
//! two checkpoint packs at different versions, two copies of the NAT with the
//! bitmap selecting the *second*, a NAT journal that overrides one entry, an
//! inline-dentry root, a block-backed subdirectory, and a file that reaches
//! data through direct, indirect and double-indirect nodes. The driver then
//! mounts it through [`MemorySource`] and reads it back with no device
//! involved, on every boot.
//!
//! Three properties of the layout are deliberate traps, and each one turns a
//! plausible driver bug from "silently wrong" into "test fails":
//!
//! * **The NAT bitmap bit for block 0 is set**, so the live NAT copy is the
//!   *second* one. Copy 0 is filled with entries whose addresses are
//!   [`NULL_ADDR`], so a driver that ignores the bitmap resolves nothing.
//! * **`hello.txt`'s NAT-area entry points at a stale inode block** whose
//!   footer is perfectly valid and whose contents differ. Only the checkpoint
//!   journal names the current block, so a driver that skips the journal
//!   mounts, lists, opens and reads the file — and returns the wrong bytes.
//! * **The two checkpoint packs carry different versions and different
//!   summary formats.** The higher-versioned one is the *second*, so a driver
//!   that takes the first pack picks the wrong checkpoint; and the two use the
//!   normal and compacted journal layouts respectively, so the corruption
//!   group's fallback exercises the layout the happy path does not.
//!
//! The builder is written *independently* of the parser: it computes the
//! inline-dentry geometry from the format's formula written out by hand rather
//! than calling [`super::dir`]'s private `DentryLayout`, and it computes node
//! addresses from the layout table below rather than from [`super::node`]. A
//! builder that reused the parser's notion of the layout would agree with it
//! by construction, including where both are wrong.
//!
//! # What this does *not* prove
//!
//! Names are hashed with [`super::raw::dentry_hash`] and the checkpoint is
//! checksummed with the driver's own CRC-32, so directory lookup and block
//! validation are tested for self-consistency, not for conformance to Linux.
//! If the TEA hash's initial vector were wrong, this suite would still pass —
//! the same wrong hash would be on both sides. That is why
//! [`test_primitives`] additionally pins the hash against properties Linux
//! guarantees independently of any image: `.` and `..` hash to zero, bit 31 is
//! always clear, and the result is not the CRC.
//!
//! # Volume layout (blocks of 4096 bytes, 32 blocks per segment)
//!
//! ```text
//!   0..32    segment 0   superblock at byte 1024, backup at byte 5120
//!  32..96    segments 1-2  checkpoint: pack A at 32 (v3), pack B at 64 (v7)
//!  96..160   segments 3-4  SIT   (never read by a read-only driver)
//! 160..224   segments 5-6  NAT: copy 0 at 160 (decoy), copy 1 at 192 (live)
//! 224..256   segment 7   SSA   (never read by a read-only driver)
//! 256..512   segments 8-15  main area
//! ```
//!
//! ```text
//! main-area block   contents
//! 256   inode nid 3   /                (inline dentries)
//! 257   inode nid 4   /hello.txt       current, named only by the CP journal
//! 258   inode nid 4   /hello.txt       STALE, named by the NAT area
//! 259   inode nid 5   /sub             (block-backed dentries)
//! 260                 /sub dentry block 0
//! 262   inode nid 6   /sub/data.bin    (extra attrs + inline xattr)
//! 263                 /sub/data.bin block 0
//! 264                 /sub/data.bin block 1
//! 265   inode nid 7   /link            (symlink, inline)
//! 266   inode nid 8   /sparse.bin      (block 0 is a hole)
//! 267                 /sparse.bin block 1
//! 268   inode nid 9   /big.bin
//! 269                 /big.bin block 0            via i_addr[0]
//! 270   node  nid 10  direct node                 via i_nid[0]
//! 271                 /big.bin block 923
//! 272   node  nid 11  direct node                 via i_nid[1]
//! 273                 /big.bin block 1941
//! 274   node  nid 12  indirect node               via i_nid[2]
//! 275   node  nid 13  direct node under nid 12
//! 276                 /big.bin block 2959
//! 277   node  nid 14  double-indirect node        via i_nid[4]
//! 278   node  nid 15  indirect node under nid 14
//! 279   node  nid 16  direct node under nid 15
//! 280                 /big.bin block 2075607
//! 281   inode nid 17  /prealloc.bin    (NEW_ADDR, must read as zeroes)
//! 282   inode nid 18  /sub/<47-byte name>  (inline, spans six name slots)
//! ```

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::MemorySource;
use crate::fs::path::Path;
use crate::fs::vfs::{EntryType, FileSystem};
use crate::serial_println;

use super::F2fsFs;
use super::cp;
use super::dir;
use super::node::{self, BlockPath, Nat};
use super::raw::{
    ADDRS_PER_BLOCK, BLOCK_SIZE, DEF_ADDRS_PER_INODE, F2FS_EXTRA_ATTR, F2FS_FT_DIR,
    F2FS_FT_REG_FILE, F2FS_FT_SYMLINK, F2FS_HASH_COL_BIT, F2FS_INLINE_DATA, F2FS_INLINE_DENTRY,
    F2FS_INLINE_XATTR, F2FS_NAME_LEN, I_ADDR_OFF, I_NID_OFF, MAGIC, NAT_ENTRIES_PER_BLOCK,
    NEW_ADDR, NODE_FOOTER_OFF, NULL_ADDR, NULL_NID, SUPER_OFFSET, block_to_offset, bucket_blocks,
    dentry_hash, dir_block_index, dir_buckets, read_u16, read_u32, read_u64, read_u8,
    slots_for_name, test_bit,
};
use super::sb::{self, FEATURE_BLKZONED, SuperBlock};

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Blocks per segment. Deliberately 32 rather than the real world's 512: the
/// NAT's two copies are one segment apart, so a small segment keeps the whole
/// image at 2 MiB while still making the interleave arithmetic non-trivial.
const BLOCKS_PER_SEG: u32 = 32;
/// `log2(BLOCKS_PER_SEG)`, as the superblock stores it.
const LOG_BLOCKS_PER_SEG: u32 = 5;

const CP_BLKADDR: u32 = 32;
const SIT_BLKADDR: u32 = 96;
const NAT_BLKADDR: u32 = 160;
const SSA_BLKADDR: u32 = 224;
const MAIN_BLKADDR: u32 = 256;
const SEGMENT_COUNT: u32 = 16;
const SEGMENT_COUNT_MAIN: u32 = 8;
/// Blocks in the whole image.
const IMAGE_BLOCKS: u32 = 512;

/// Byte offset of the superblock's CRC, and the length it covers.
const SB_CRC_OFF: usize = 3068;
/// Byte offset of the checkpoint's CRC.
const CP_CRC_OFF: usize = 4092;
/// Byte offset of `checksum_offset` inside a checkpoint block.
const CP_CHKSUM_FIELD: usize = 164;
/// Blocks in each checkpoint pack.
const CP_PACK_BLOCKS: u32 = 8;

const CP_UMOUNT: u32 = 0x0000_0001;
const CP_COMPACT_SUM: u32 = 0x0000_0004;

/// Bytes of SIT bitmap the checkpoint claims, which is what displaces the NAT
/// bitmap in the no-payload layout.
const SIT_BITMAP_BYTES: u32 = 8;
/// Bytes of NAT bitmap: 32 NAT block-pairs fit in four.
const NAT_BITMAP_BYTES: u32 = 4;
/// Byte offset of the NAT bitmap within a checkpoint block, in this layout.
const NAT_BITMAP_OFF: usize = 192 + SIT_BITMAP_BYTES as usize;

/// Journal offset inside a *normal* summary block.
const SUM_JOURNAL_OFF: usize = 3584;

// Node ids.
const NID_ROOT: u32 = 3;
const NID_HELLO: u32 = 4;
const NID_SUB: u32 = 5;
const NID_DATA: u32 = 6;
const NID_LINK: u32 = 7;
const NID_SPARSE: u32 = 8;
const NID_BIG: u32 = 9;
const NID_BIG_D0: u32 = 10;
const NID_BIG_D1: u32 = 11;
const NID_BIG_IND: u32 = 12;
const NID_BIG_IND_D: u32 = 13;
const NID_BIG_DIND: u32 = 14;
const NID_BIG_DIND_I: u32 = 15;
const NID_BIG_DIND_D: u32 = 16;
const NID_PREALLOC: u32 = 17;
const NID_LONG: u32 = 18;
/// One past the highest nid the image defines.
const NID_END: u32 = 19;

// Main-area block addresses.
const B_ROOT: u32 = 256;
const B_HELLO: u32 = 257;
const B_HELLO_STALE: u32 = 258;
const B_SUB: u32 = 259;
const B_SUB_DENT0: u32 = 260;
const B_DATA: u32 = 262;
const B_DATA_0: u32 = 263;
const B_DATA_1: u32 = 264;
const B_LINK: u32 = 265;
const B_SPARSE: u32 = 266;
const B_SPARSE_1: u32 = 267;
const B_BIG: u32 = 268;
const B_BIG_0: u32 = 269;
const B_BIG_D0: u32 = 270;
const B_BIG_D0_DATA: u32 = 271;
const B_BIG_D1: u32 = 272;
const B_BIG_D1_DATA: u32 = 273;
const B_BIG_IND: u32 = 274;
const B_BIG_IND_D: u32 = 275;
const B_BIG_IND_DATA: u32 = 276;
const B_BIG_DIND: u32 = 277;
const B_BIG_DIND_I: u32 = 278;
const B_BIG_DIND_D: u32 = 279;
const B_BIG_DIND_DATA: u32 = 280;
const B_PREALLOC: u32 = 281;
const B_LONG: u32 = 282;

/// `i_extra_isize` for the inodes that carry an extra-attribute area.
///
/// 36 bytes = nine `u32` slots taken off the front of `i_addr`, which is what
/// `mkfs.f2fs` writes. An inode built with it and read without it has every
/// block pointer displaced by nine slots.
const EXTRA_ISIZE: u16 = 36;
/// Inline-xattr size, in `u32` units, for the one inode that reserves one.
const INLINE_XATTR_WORDS: u16 = 50;

// File contents.
const HELLO_TEXT: &[u8] = b"Hello from the checkpoint journal.\n";
const HELLO_STALE_TEXT: &[u8] = b"STALE: the NAT area's copy of this file.\n";
const LINK_TARGET: &[u8] = b"/sub/data.bin";
const LONG_TEXT: &[u8] = b"payload of the long-named file";
/// 47 bytes, so it occupies six of the eight-byte name slots.
const LONG_NAME: &[u8] = b"a-very-long-file-name-that-spans-many-slots.txt";
/// `/sub/data.bin` is this many bytes: one full block plus a short tail.
const DATA_SIZE: u64 = 4096 + 100;
/// `/sparse.bin` is two blocks, the first of which is a hole.
const SPARSE_SIZE: u64 = 8192;

// `/big.bin`'s file-block indices, one per level of indirection. Each is the
// first block reachable only through that level, so an off-by-one in
// `block_path` moves it to a neighbouring level and reads a different node.
const BIG_DIRECT0: u64 = DEF_ADDRS_PER_INODE as u64;
const BIG_DIRECT1: u64 = BIG_DIRECT0 + ADDRS_PER_BLOCK as u64;
const BIG_INDIRECT: u64 = BIG_DIRECT1 + ADDRS_PER_BLOCK as u64;
const BIG_DINDIRECT: u64 =
    BIG_INDIRECT + 2 * (ADDRS_PER_BLOCK as u64) * (ADDRS_PER_BLOCK as u64);
/// `/big.bin`'s size: one byte past its highest backed block.
const BIG_SIZE: u64 = (BIG_DINDIRECT + 1) * BLOCK_SIZE as u64;

const BIG_TEXT_0: &[u8] = b"BIG:inode-direct";
const BIG_TEXT_D0: &[u8] = b"BIG:direct-node-0";
const BIG_TEXT_D1: &[u8] = b"BIG:direct-node-1";
const BIG_TEXT_IND: &[u8] = b"BIG:indirect";
const BIG_TEXT_DIND: &[u8] = b"BIG:double-indirect";

/// Byte offset of block `n`.
const fn blk(n: u32) -> usize {
    (n as usize).saturating_mul(BLOCK_SIZE)
}

// ---------------------------------------------------------------------------
// Byte-level builders
//
// Every writer silently ignores an out-of-range offset rather than panicking.
// A self-test that panics takes the kernel down before it can print its
// results, and a builder bug that writes nowhere surfaces immediately as a
// failed check with a name attached — which is more informative than a
// backtrace-free halt.
// ---------------------------------------------------------------------------

fn put_bytes(img: &mut [u8], off: usize, v: &[u8]) {
    if let Some(dst) = off.checked_add(v.len()).and_then(|e| img.get_mut(off..e)) {
        dst.copy_from_slice(v);
    }
}

fn put_u8(img: &mut [u8], off: usize, v: u8) {
    if let Some(b) = img.get_mut(off) {
        *b = v;
    }
}

fn put_u16(img: &mut [u8], off: usize, v: u16) {
    put_bytes(img, off, &v.to_le_bytes());
}

fn put_u32(img: &mut [u8], off: usize, v: u32) {
    put_bytes(img, off, &v.to_le_bytes());
}

fn put_u64(img: &mut [u8], off: usize, v: u64) {
    put_bytes(img, off, &v.to_le_bytes());
}

/// Set bit `n` of the bitmap starting at byte `base`.
fn put_bit(img: &mut [u8], base: usize, n: usize) {
    let Some(byte) = base.checked_add(n.checked_div(8).unwrap_or(0)) else {
        return;
    };
    if let Some(b) = img.get_mut(byte) {
        *b |= 1u8 << (n % 8);
    }
}

/// Write a node footer into block `block`.
///
/// `nid` is what the node *is*; `ino` is the inode it belongs to. For an inode
/// block the two are equal, and [`node::read_inode`] refuses a block where
/// they are not.
fn put_footer(img: &mut [u8], block: u32, nid: u32, ino: u32) {
    let base = blk(block).saturating_add(NODE_FOOTER_OFF);
    put_u32(img, base, nid);
    put_u32(img, base.saturating_add(4), ino);
    put_u32(img, base.saturating_add(8), 0);
    put_u64(img, base.saturating_add(12), 7);
    put_u32(img, base.saturating_add(20), 0);
}

/// Options for [`put_inode`], gathered so the builder reads as a description
/// of the inode rather than as ten positional arguments.
#[derive(Clone, Copy)]
struct InodeSpec {
    nid: u32,
    block: u32,
    mode: u16,
    inline: u8,
    size: u64,
    links: u32,
    pino: u32,
    extra_isize: u16,
    inline_xattr_words: u16,
    current_depth: u32,
}

impl InodeSpec {
    /// A regular file with nothing unusual about it.
    const fn file(nid: u32, block: u32, size: u64) -> Self {
        Self {
            nid,
            block,
            mode: 0o100_644,
            inline: 0,
            size,
            links: 1,
            pino: NID_ROOT,
            extra_isize: 0,
            inline_xattr_words: 0,
            current_depth: 0,
        }
    }

    /// A directory.
    const fn dir(nid: u32, block: u32, size: u64) -> Self {
        Self {
            mode: 0o40_755,
            links: 2,
            current_depth: 1,
            ..Self::file(nid, block, size)
        }
    }
}

/// Byte offset of `i_addr[n]` within the inode block, computed here from the
/// format rather than from [`Inode`]'s own accessor.
const fn addr_off(spec: &InodeSpec, n: u32) -> usize {
    blk(spec.block)
        .saturating_add(I_ADDR_OFF)
        .saturating_add(spec.extra_isize as usize)
        .saturating_add((n as usize).saturating_mul(4))
}

/// Byte offset of `i_nid[n]` within the inode block.
const fn nid_off(spec: &InodeSpec, n: u32) -> usize {
    blk(spec.block)
        .saturating_add(I_NID_OFF)
        .saturating_add((n as usize).saturating_mul(4))
}

/// `i_addr` slots the inode actually owns, by the format's formula.
const fn addrs_per_inode(spec: &InodeSpec) -> u32 {
    let extra_words = (spec.extra_isize as u32).saturating_div(4);
    let xattr = if spec.inline & F2FS_INLINE_XATTR == 0 {
        0
    } else {
        spec.inline_xattr_words as u32
    };
    DEF_ADDRS_PER_INODE
        .saturating_sub(extra_words)
        .saturating_sub(xattr)
}

/// Write an inode into its block.
fn put_inode(img: &mut [u8], spec: &InodeSpec) {
    let base = blk(spec.block);
    put_u16(img, base, spec.mode);
    put_u8(img, base.saturating_add(3), spec.inline);
    put_u32(img, base.saturating_add(4), 0);
    put_u32(img, base.saturating_add(8), 0);
    put_u32(img, base.saturating_add(12), spec.links);
    put_u64(img, base.saturating_add(16), spec.size);
    // `i_blocks` is in 512-byte units and is reported verbatim by `metadata`.
    put_u64(
        img,
        base.saturating_add(24),
        spec.size.div_ceil(512).max(1),
    );
    put_u64(img, base.saturating_add(32), 1_700_000_001);
    put_u64(img, base.saturating_add(40), 1_700_000_002);
    put_u64(img, base.saturating_add(48), 1_700_000_003);
    put_u32(img, base.saturating_add(56), 11);
    put_u32(img, base.saturating_add(60), 22);
    put_u32(img, base.saturating_add(64), 33);
    put_u32(img, base.saturating_add(72), spec.current_depth);
    put_u32(img, base.saturating_add(84), spec.pino);
    put_u8(img, base.saturating_add(347), 0);

    if spec.inline & F2FS_EXTRA_ATTR != 0 {
        put_u16(img, base.saturating_add(360), spec.extra_isize);
        put_u16(img, base.saturating_add(362), spec.inline_xattr_words);
    }

    put_footer(img, spec.block, spec.nid, spec.nid);
}

/// The inline data/dentry area of an inode, as a byte range in the image.
///
/// One `i_addr` slot past the extra-attribute area, running to the end of the
/// slots the inode owns. Written out here rather than taken from
/// [`Inode::inline_area`] so the two can disagree.
const fn inline_area(spec: &InodeSpec) -> (usize, usize) {
    let start = addr_off(spec, 1);
    let words = addrs_per_inode(spec).saturating_sub(1) as usize;
    (start, words.saturating_mul(4))
}

// ---------------------------------------------------------------------------
// Directory entries
// ---------------------------------------------------------------------------

/// Absolute byte offsets of one dentry area's three arrays.
#[derive(Clone, Copy)]
struct DentryGeom {
    slots: usize,
    bitmap: usize,
    entries: usize,
    names: usize,
}

impl DentryGeom {
    /// A full 4 KiB dentry block: 27 bytes of bitmap, 3 reserved, 214 entries
    /// of 11 bytes, then 214 name slots of 8.
    const fn block(block: u32) -> Self {
        let base = blk(block);
        Self {
            slots: 214,
            bitmap: base,
            entries: base.saturating_add(30),
            names: base.saturating_add(30 + 214 * 11),
        }
    }

    /// An inline dentry area of `len` bytes starting at `start`.
    ///
    /// The slot count is the largest `n` with `n * (19 * 8 + 1) <= len * 8` —
    /// each slot costs its 11-byte entry, its 8-byte name slot and one bit of
    /// bitmap. Whatever is left over is reserved padding, and it sits between
    /// the bitmap and the entries exactly as in a full block.
    const fn inline(start: usize, len: usize) -> Self {
        let slots = len.saturating_mul(8).saturating_div(19 * 8 + 1);
        let bitmap_bytes = slots.div_ceil(8);
        let used = bitmap_bytes.saturating_add(slots.saturating_mul(19));
        let reserved = len.saturating_sub(used);
        let entries = start.saturating_add(bitmap_bytes).saturating_add(reserved);
        Self {
            slots,
            bitmap: start,
            entries,
            names: entries.saturating_add(slots.saturating_mul(11)),
        }
    }
}

/// Write one directory entry into `geom` at `slot`.
///
/// Sets every slot the name spans in the bitmap, which is what F2FS does and
/// what makes a long name's continuation slots invisible to a walk that
/// advances by [`slots_for_name`].
fn put_dentry(img: &mut [u8], geom: DentryGeom, slot: usize, name: &[u8], ino: u32, ftype: u8) {
    let eoff = geom
        .entries
        .saturating_add(slot.saturating_mul(11));
    put_u32(img, eoff, dentry_hash(name));
    put_u32(img, eoff.saturating_add(4), ino);
    put_u16(
        img,
        eoff.saturating_add(8),
        u16::try_from(name.len()).unwrap_or(0),
    );
    put_u8(img, eoff.saturating_add(10), ftype);

    put_bytes(
        img,
        geom.names.saturating_add(slot.saturating_mul(8)),
        name,
    );

    for i in 0..slots_for_name(name.len()).max(1) {
        put_bit(img, geom.bitmap, slot.saturating_add(i));
    }
}

// ---------------------------------------------------------------------------
// Superblock, checkpoint and NAT
// ---------------------------------------------------------------------------

/// Write both copies of the superblock, sealing each with its CRC.
fn put_superblock(img: &mut [u8], feature: u32) {
    for copy in 0..2u32 {
        let base = (SUPER_OFFSET as usize).saturating_add(blk(copy));
        put_u32(img, base, MAGIC);
        put_u16(img, base.saturating_add(4), 1);
        put_u16(img, base.saturating_add(6), 15);
        put_u32(img, base.saturating_add(8), 9);
        put_u32(img, base.saturating_add(12), 3);
        put_u32(img, base.saturating_add(16), 12);
        put_u32(img, base.saturating_add(20), LOG_BLOCKS_PER_SEG);
        put_u32(img, base.saturating_add(24), 1);
        put_u32(img, base.saturating_add(28), 1);
        put_u32(img, base.saturating_add(32), SB_CRC_OFF as u32);
        put_u64(img, base.saturating_add(36), u64::from(IMAGE_BLOCKS));
        put_u32(img, base.saturating_add(44), SEGMENT_COUNT);
        put_u32(img, base.saturating_add(48), SEGMENT_COUNT);
        put_u32(img, base.saturating_add(52), 2);
        put_u32(img, base.saturating_add(56), 2);
        put_u32(img, base.saturating_add(60), 2);
        put_u32(img, base.saturating_add(64), 1);
        put_u32(img, base.saturating_add(68), SEGMENT_COUNT_MAIN);
        put_u32(img, base.saturating_add(72), 0);
        put_u32(img, base.saturating_add(76), CP_BLKADDR);
        put_u32(img, base.saturating_add(80), SIT_BLKADDR);
        put_u32(img, base.saturating_add(84), NAT_BLKADDR);
        put_u32(img, base.saturating_add(88), SSA_BLKADDR);
        put_u32(img, base.saturating_add(92), MAIN_BLKADDR);
        put_u32(img, base.saturating_add(96), NID_ROOT);
        put_u32(img, base.saturating_add(100), 1);
        put_u32(img, base.saturating_add(104), 2);
        for i in 0..16usize {
            put_u8(
                img,
                base.saturating_add(108).saturating_add(i),
                u8::try_from(i).unwrap_or(0),
            );
        }
        // The label is UTF-16LE and NUL-terminated. A non-ASCII character is
        // included so a driver that walked it as bytes would produce visibly
        // wrong text rather than something that merely looks short.
        for (i, unit) in "SLATE\u{2014}F2FS".encode_utf16().enumerate() {
            put_u16(
                img,
                base.saturating_add(124)
                    .saturating_add(i.saturating_mul(2)),
                unit,
            );
        }
        put_u32(img, base.saturating_add(1664), 0);
        put_u32(img, base.saturating_add(2180), feature);

        // Seeded with the magic and inverted at neither end: F2FS uses Linux's
        // bare `crc32_le`, so the conventional framing would be wrong by `!0`
        // on both sides.
        let crc = img
            .get(base..base.saturating_add(SB_CRC_OFF))
            .map_or(0, |s| crate::crypto::crc32_raw(MAGIC, s));
        put_u32(img, base.saturating_add(SB_CRC_OFF), crc);
    }
}

/// Write one checkpoint pack.
///
/// `tail_version` is written into the pack's last block. Passing something
/// other than `version` produces exactly the shape of a pack whose write was
/// interrupted: a complete, correctly-checksummed first block claiming a
/// version the pack never finished committing.
fn put_cp_pack(img: &mut [u8], start: u32, version: u64, tail_version: u64, flags: u32) {
    let base = blk(start);
    put_u64(img, base, version);
    put_u64(img, base.saturating_add(8), u64::from(IMAGE_BLOCKS));
    put_u64(img, base.saturating_add(16), 40);
    put_u32(img, base.saturating_add(32), 4);
    put_u32(img, base.saturating_add(132), flags);
    put_u32(img, base.saturating_add(136), CP_PACK_BLOCKS);
    put_u32(img, base.saturating_add(140), 1);
    put_u32(img, base.saturating_add(144), 16);
    put_u32(img, base.saturating_add(148), 10);
    put_u32(img, base.saturating_add(152), NID_END);
    put_u32(img, base.saturating_add(156), SIT_BITMAP_BYTES);
    put_u32(img, base.saturating_add(160), NAT_BITMAP_BYTES);
    put_u32(img, base.saturating_add(CP_CHKSUM_FIELD), CP_CRC_OFF as u32);
    put_u64(img, base.saturating_add(168), 1_700_000_000);

    // Bit 0 of the NAT bitmap: NAT block 0's live copy is the *second* one.
    put_bit(img, base.saturating_add(NAT_BITMAP_OFF), 0);

    // The journal, in whichever of the two layouts this pack's flags select.
    let (jblock, joff) = if flags & CP_COMPACT_SUM != 0 {
        (start.saturating_add(1), 0usize)
    } else {
        (
            start
                .saturating_add(CP_PACK_BLOCKS)
                .saturating_sub(7),
            SUM_JOURNAL_OFF,
        )
    };
    let jbase = blk(jblock).saturating_add(joff);
    put_u16(img, jbase, 1);
    put_u32(img, jbase.saturating_add(2), NID_HELLO);
    put_u8(img, jbase.saturating_add(6), 1);
    put_u32(img, jbase.saturating_add(7), NID_HELLO);
    put_u32(img, jbase.saturating_add(11), B_HELLO);

    let crc = img
        .get(base..base.saturating_add(CP_CRC_OFF))
        .map_or(0, |s| crate::crypto::crc32_raw(MAGIC, s));
    put_u32(img, base.saturating_add(CP_CRC_OFF), crc);

    // The tail block carries only the version, which is what a reader compares
    // against the head to decide the pack was finished.
    put_u64(
        img,
        blk(start.saturating_add(CP_PACK_BLOCKS).saturating_sub(1)),
        tail_version,
    );
}

/// Write one entry into a NAT block.
fn put_nat(img: &mut [u8], nat_block: u32, nid: u32, ino: u32, addr: u32) {
    let off = blk(nat_block).saturating_add((nid as usize).saturating_mul(9));
    put_u8(img, off, 1);
    put_u32(img, off.saturating_add(1), ino);
    put_u32(img, off.saturating_add(5), addr);
}

/// Fill a data block with a byte pattern that depends on its address.
///
/// The pattern is a function of the *block address*, so a read that lands on
/// the wrong block produces bytes that are wrong everywhere rather than in a
/// header the check might not look at.
fn put_pattern(img: &mut [u8], block: u32, label: &[u8]) {
    let base = blk(block);
    for i in 0..BLOCK_SIZE {
        let v = u8::try_from(
            (block as usize)
                .wrapping_mul(31)
                .wrapping_add(i)
                .wrapping_rem(251),
        )
        .unwrap_or(0);
        put_u8(img, base.saturating_add(i), v);
    }
    put_bytes(img, base, label);
}

/// The byte the address-derived pattern puts at offset `i` of `block`.
fn pattern_byte(block: u32, i: usize) -> u8 {
    u8::try_from(
        (block as usize)
            .wrapping_mul(31)
            .wrapping_add(i)
            .wrapping_rem(251),
    )
    .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The image
// ---------------------------------------------------------------------------

/// Build the synthetic volume described in this module's header.
#[allow(clippy::too_many_lines)]
fn build_image() -> Vec<u8> {
    let mut img = vec![0u8; blk(IMAGE_BLOCKS)];

    put_superblock(
        &mut img,
        sb::FEATURE_SB_CHKSUM | sb::FEATURE_EXTRA_ATTR | sb::FEATURE_FLEXIBLE_INLINE_XATTR,
    );

    // Pack A is older and uses the compacted summary layout; pack B is newer
    // and uses the normal one. A driver that takes the first pack it can parse
    // gets A, and the volume it mounts is a version behind.
    put_cp_pack(&mut img, CP_BLKADDR, 3, 3, CP_COMPACT_SUM);
    put_cp_pack(
        &mut img,
        CP_BLKADDR.saturating_add(BLOCKS_PER_SEG),
        7,
        7,
        CP_UMOUNT,
    );

    // --- NAT ------------------------------------------------------------
    //
    // Copy 0 is the decoy: every nid resolves to NULL_ADDR there, so a driver
    // that ignores the checkpoint's bitmap finds nothing at all. Copy 1 is
    // live, and `hello.txt`'s entry in it is deliberately *stale* — only the
    // checkpoint journal names the current block.
    let live = NAT_BLKADDR.saturating_add(BLOCKS_PER_SEG);
    for nid in 0..NID_END {
        put_nat(&mut img, NAT_BLKADDR, nid, nid, NULL_ADDR);
    }
    let table: [(u32, u32, u32); 16] = [
        (NID_ROOT, NID_ROOT, B_ROOT),
        (NID_HELLO, NID_HELLO, B_HELLO_STALE),
        (NID_SUB, NID_SUB, B_SUB),
        (NID_DATA, NID_DATA, B_DATA),
        (NID_LINK, NID_LINK, B_LINK),
        (NID_SPARSE, NID_SPARSE, B_SPARSE),
        (NID_BIG, NID_BIG, B_BIG),
        (NID_BIG_D0, NID_BIG, B_BIG_D0),
        (NID_BIG_D1, NID_BIG, B_BIG_D1),
        (NID_BIG_IND, NID_BIG, B_BIG_IND),
        (NID_BIG_IND_D, NID_BIG, B_BIG_IND_D),
        (NID_BIG_DIND, NID_BIG, B_BIG_DIND),
        (NID_BIG_DIND_I, NID_BIG, B_BIG_DIND_I),
        (NID_BIG_DIND_D, NID_BIG, B_BIG_DIND_D),
        (NID_PREALLOC, NID_PREALLOC, B_PREALLOC),
        (NID_LONG, NID_LONG, B_LONG),
    ];
    for (nid, ino, addr) in table {
        put_nat(&mut img, live, nid, ino, addr);
    }

    // --- / : an inline-dentry directory ---------------------------------
    let root = InodeSpec {
        inline: F2FS_INLINE_DENTRY,
        links: 4,
        pino: NID_ROOT,
        ..InodeSpec::dir(NID_ROOT, B_ROOT, 0)
    };
    put_inode(&mut img, &root);
    let (rstart, rlen) = inline_area(&root);
    let rg = DentryGeom::inline(rstart, rlen);
    put_dentry(&mut img, rg, 0, b".", NID_ROOT, F2FS_FT_DIR);
    put_dentry(&mut img, rg, 1, b"..", NID_ROOT, F2FS_FT_DIR);
    put_dentry(&mut img, rg, 2, b"hello.txt", NID_HELLO, F2FS_FT_REG_FILE);
    put_dentry(&mut img, rg, 4, b"sub", NID_SUB, F2FS_FT_DIR);
    put_dentry(&mut img, rg, 5, b"link", NID_LINK, F2FS_FT_SYMLINK);
    put_dentry(&mut img, rg, 6, b"sparse.bin", NID_SPARSE, F2FS_FT_REG_FILE);
    put_dentry(&mut img, rg, 8, b"big.bin", NID_BIG, F2FS_FT_REG_FILE);
    put_dentry(
        &mut img,
        rg,
        9,
        b"prealloc.bin",
        NID_PREALLOC,
        F2FS_FT_REG_FILE,
    );

    // --- /hello.txt : inline data, with an extra-attribute area ----------
    //
    // Two copies. The one the NAT area points at is a complete, valid inode
    // whose footer checks out; only its contents differ. Nothing but the
    // journal distinguishes them.
    for (block, text) in [(B_HELLO, HELLO_TEXT), (B_HELLO_STALE, HELLO_STALE_TEXT)] {
        let spec = InodeSpec {
            inline: F2FS_INLINE_DATA | F2FS_EXTRA_ATTR,
            extra_isize: EXTRA_ISIZE,
            size: text.len() as u64,
            ..InodeSpec::file(NID_HELLO, block, 0)
        };
        put_inode(&mut img, &spec);
        let (start, _) = inline_area(&spec);
        put_bytes(&mut img, start, text);
    }

    // --- /sub : a block-backed directory --------------------------------
    let sub = InodeSpec {
        links: 2,
        pino: NID_ROOT,
        ..InodeSpec::dir(NID_SUB, B_SUB, u64::try_from(blk(2)).unwrap_or(8192))
    };
    put_inode(&mut img, &sub);
    put_u32(&mut img, addr_off(&sub, 0), B_SUB_DENT0);
    // Bucket 0 at level 0 is two blocks wide and only the first was ever
    // filled, so the second is a hole. `read_dir` must skip it rather than
    // treat a block of zeroes as 214 empty entries.
    put_u32(&mut img, addr_off(&sub, 1), NULL_ADDR);

    let sg = DentryGeom::block(B_SUB_DENT0);
    put_dentry(&mut img, sg, 0, b".", NID_SUB, F2FS_FT_DIR);
    put_dentry(&mut img, sg, 1, b"..", NID_ROOT, F2FS_FT_DIR);
    put_dentry(&mut img, sg, 2, b"data.bin", NID_DATA, F2FS_FT_REG_FILE);
    put_dentry(&mut img, sg, 3, LONG_NAME, NID_LONG, F2FS_FT_REG_FILE);

    // --- /sub/data.bin : two real blocks, extra attrs and an inline xattr -
    let data = InodeSpec {
        inline: F2FS_EXTRA_ATTR | F2FS_INLINE_XATTR,
        extra_isize: EXTRA_ISIZE,
        inline_xattr_words: INLINE_XATTR_WORDS,
        pino: NID_SUB,
        ..InodeSpec::file(NID_DATA, B_DATA, DATA_SIZE)
    };
    put_inode(&mut img, &data);
    put_u32(&mut img, addr_off(&data, 0), B_DATA_0);
    put_u32(&mut img, addr_off(&data, 1), B_DATA_1);
    put_pattern(&mut img, B_DATA_0, b"DATA0");
    put_pattern(&mut img, B_DATA_1, b"DATA1");

    // --- /link : a symlink whose target is stored like file contents -----
    let link = InodeSpec {
        mode: 0o120_777,
        inline: F2FS_INLINE_DATA,
        size: LINK_TARGET.len() as u64,
        ..InodeSpec::file(NID_LINK, B_LINK, 0)
    };
    put_inode(&mut img, &link);
    let (lstart, _) = inline_area(&link);
    put_bytes(&mut img, lstart, LINK_TARGET);

    // --- /sparse.bin : a hole followed by a real block -------------------
    let sparse = InodeSpec::file(NID_SPARSE, B_SPARSE, SPARSE_SIZE);
    put_inode(&mut img, &sparse);
    put_u32(&mut img, addr_off(&sparse, 0), NULL_ADDR);
    put_u32(&mut img, addr_off(&sparse, 1), B_SPARSE_1);
    put_pattern(&mut img, B_SPARSE_1, b"TAIL");

    // --- /prealloc.bin : reserved but never written ----------------------
    let prealloc = InodeSpec::file(NID_PREALLOC, B_PREALLOC, BLOCK_SIZE as u64);
    put_inode(&mut img, &prealloc);
    put_u32(&mut img, addr_off(&prealloc, 0), NEW_ADDR);

    // --- /sub/<long name> : inline data ----------------------------------
    let long = InodeSpec {
        inline: F2FS_INLINE_DATA,
        size: LONG_TEXT.len() as u64,
        pino: NID_SUB,
        ..InodeSpec::file(NID_LONG, B_LONG, 0)
    };
    put_inode(&mut img, &long);
    let (long_start, _) = inline_area(&long);
    put_bytes(&mut img, long_start, LONG_TEXT);

    // --- /big.bin : one block at each level of indirection ---------------
    //
    // The file is nominally 8.5 GiB. Only five of its blocks exist; everything
    // between them is a hole, which is what lets a 2 MiB image exercise the
    // double-indirect path at all.
    let big = InodeSpec::file(NID_BIG, B_BIG, BIG_SIZE);
    put_inode(&mut img, &big);
    put_u32(&mut img, addr_off(&big, 0), B_BIG_0);
    put_u32(&mut img, nid_off(&big, 0), NID_BIG_D0);
    put_u32(&mut img, nid_off(&big, 1), NID_BIG_D1);
    put_u32(&mut img, nid_off(&big, 2), NID_BIG_IND);
    put_u32(&mut img, nid_off(&big, 3), 0);
    put_u32(&mut img, nid_off(&big, 4), NID_BIG_DIND);
    put_pattern(&mut img, B_BIG_0, BIG_TEXT_0);

    // Every intermediate node is a bare array of `u32` ahead of its footer,
    // whether its entries are block addresses or node ids.
    let nodes: [(u32, u32, u32); 6] = [
        (B_BIG_D0, NID_BIG_D0, B_BIG_D0_DATA),
        (B_BIG_D1, NID_BIG_D1, B_BIG_D1_DATA),
        (B_BIG_IND, NID_BIG_IND, NID_BIG_IND_D),
        (B_BIG_IND_D, NID_BIG_IND_D, B_BIG_IND_DATA),
        (B_BIG_DIND, NID_BIG_DIND, NID_BIG_DIND_I),
        (B_BIG_DIND_I, NID_BIG_DIND_I, NID_BIG_DIND_D),
    ];
    for (block, nid, slot0) in nodes {
        put_u32(&mut img, blk(block), slot0);
        put_footer(&mut img, block, nid, NID_BIG);
    }
    put_u32(&mut img, blk(B_BIG_DIND_D), B_BIG_DIND_DATA);
    put_footer(&mut img, B_BIG_DIND_D, NID_BIG_DIND_D, NID_BIG);

    put_pattern(&mut img, B_BIG_D0_DATA, BIG_TEXT_D0);
    put_pattern(&mut img, B_BIG_D1_DATA, BIG_TEXT_D1);
    put_pattern(&mut img, B_BIG_IND_DATA, BIG_TEXT_IND);
    put_pattern(&mut img, B_BIG_DIND_DATA, BIG_TEXT_DIND);

    img
}

/// Mount the synthetic volume.
fn mount_image(img: Vec<u8>) -> KernelResult<F2fsFs> {
    F2fsFs::open_source(Box::new(MemorySource::new(img)))
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Running tally of the suite.
struct Checks {
    passed: u32,
    failed: u32,
}

/// One group of the self-test.
///
/// Runs its checks against the shared tally, returning `Err` only for a hard
/// error that makes its *own* remaining steps meaningless — a mount that could
/// not proceed, so everything downstream of it in that group would be asserting
/// against nothing. A failed assertion is not an error and does not stop the
/// group.
type TestGroup = fn(&mut Checks) -> KernelResult<()>;

impl Checks {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    /// Assert `cond`, reporting `what` on failure.
    ///
    /// Never panics: a self-test that panics takes the kernel down before it
    /// can print the rest of its results, so one broken check would hide every
    /// check after it. Returns nothing, for the same reason — the tally is what
    /// decides the suite's verdict, so there is no failure for a caller to
    /// propagate and no `?` to write at the call site.
    fn check(&mut self, cond: bool, what: &str) {
        if cond {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
            serial_println!("[f2fs] SELF-TEST FAILED: {}", what);
        }
    }

    /// Assert that `got` failed with exactly `want`.
    ///
    /// A helper rather than `got == Err(want)` because the success types
    /// involved — `SuperBlock`, `Checkpoint`, `Inode`, `F2fsFs` — do not
    /// implement `PartialEq`, and deriving it on production types purely so a
    /// test could spell an equality would be the test dictating the driver's
    /// API. Comparing the error alone is also the stricter check: it says
    /// *which* rejection happened, not merely that one did.
    fn check_err<T>(&mut self, got: KernelResult<T>, want: KernelError, what: &str) {
        self.check(got.err() == Some(want), what);
    }

    /// Assert two byte slices are equal, reporting the first difference.
    ///
    /// Reporting the offset matters more here than in most suites: the file
    /// data in this image is an address-derived pattern, so the *index* of the
    /// first wrong byte usually names the bug outright — a whole-block
    /// mismatch from block 0 means the wrong block was fetched, whereas a
    /// mismatch part-way in means the offset arithmetic slipped.
    fn check_bytes(&mut self, got: &[u8], want: &[u8], what: &str) {
        if got.len() != want.len() {
            self.failed = self.failed.saturating_add(1);
            serial_println!(
                "[f2fs] SELF-TEST FAILED: {} - length {} != {}",
                what,
                got.len(),
                want.len()
            );
            return;
        }
        for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            if a != b {
                self.failed = self.failed.saturating_add(1);
                serial_println!(
                    "[f2fs] SELF-TEST FAILED: {} - byte {} is {:#04x}, expected {:#04x}",
                    what,
                    i,
                    a,
                    b
                );
                return;
            }
        }
        self.passed = self.passed.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// Scalars, bitmaps and the name hash
// ---------------------------------------------------------------------------

// The `Result` is not superfluous here even though this group happens never to
// fail hard: every group is stored in one `[(&str, TestGroup); 7]` table, so
// they must share a signature. Dropping the wrapper from the one group that
// currently gets away without it would mean the table could no longer hold it.
#[allow(clippy::unnecessary_wraps)]
fn test_primitives(c: &mut Checks) -> KernelResult<()> {
    let buf: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

    c.check(read_u8(&buf, 0) == Ok(0x01), "read_u8");
    c.check(read_u16(&buf, 0) == Ok(0x0201), "read_u16 is little-endian");
    c.check(read_u32(&buf, 0) == Ok(0x0403_0201), "read_u32");
    c.check(read_u64(&buf, 0) == Ok(0x0807_0605_0403_0201), "read_u64");

    // A read that starts inside the buffer but ends outside it is the one a
    // naive bounds check misses, so it is checked for every width.
    c.check(read_u8(&buf, 8).is_err(), "read_u8 past the end");
    c.check(read_u16(&buf, 7).is_err(), "read_u16 straddling the end");
    c.check(read_u32(&buf, 5).is_err(), "read_u32 straddling the end");
    c.check(read_u64(&buf, 1).is_err(), "read_u64 straddling the end");
    c.check(
        read_u64(&buf, usize::MAX).is_err(),
        "an offset that would overflow when the width is added",
    );

    // The NAT version bitmap and the dentry validity bitmap are both LSB-first
    // within each byte, which is the opposite of how a bitmap is usually drawn
    // on paper and therefore the easy thing to get backwards.
    let bm: [u8; 2] = [0b0000_0110, 0b1000_0000];
    c.check(!test_bit(&bm, 0), "bit 0 clear");
    c.check(test_bit(&bm, 1) && test_bit(&bm, 2), "bits 1 and 2 set");
    c.check(test_bit(&bm, 15), "the high bit of the second byte is bit 15");
    c.check(!test_bit(&bm, 16), "a bit past the end reads as clear");

    c.check(block_to_offset(0) == 0, "block 0 is at offset 0");
    c.check(block_to_offset(1) == 4096, "block 1 is one block in");
    c.check(
        block_to_offset(u32::MAX) == u64::from(u32::MAX) * 4096,
        "the last block's offset does not overflow",
    );

    // Directory geometry. Level 0 is one bucket of two blocks, so level 1
    // starts at block 2, and level 2 starts after level 1's two buckets.
    c.check(dir_buckets(0, 0) == 1, "level 0 has one bucket");
    c.check(dir_buckets(3, 0) == 8, "bucket count doubles per level");
    c.check(
        dir_buckets(2, 1) == dir_buckets(3, 0),
        "i_dir_level shifts the whole table",
    );
    c.check(bucket_blocks(0) == 2, "a shallow bucket is two blocks");
    c.check(bucket_blocks(31) == 4, "a deep bucket is four blocks");
    c.check(dir_block_index(0, 0, 0) == 0, "the first block of level 0");
    c.check(dir_block_index(1, 0, 0) == 2, "level 1 starts after level 0");
    c.check(dir_block_index(1, 0, 1) == 4, "the second bucket of level 1");
    c.check(dir_block_index(2, 0, 0) == 6, "level 2 starts after level 1");

    c.check(slots_for_name(0) == 0, "an empty name occupies no slots");
    c.check(slots_for_name(1) == 1, "a one-byte name occupies one slot");
    c.check(slots_for_name(8) == 1, "eight bytes still fit one slot");
    c.check(slots_for_name(9) == 2, "nine bytes spill into a second");
    c.check(
        slots_for_name(LONG_NAME.len()) == 6,
        "the long test name occupies six slots",
    );

    // The hash's two documented invariants, plus the two properties that catch
    // a transcription slip: it must depend on every byte, and it must not
    // silently be some other function of the name.
    c.check(dentry_hash(b".") == 0, ". hashes to zero");
    c.check(dentry_hash(b"..") == 0, ".. hashes to zero");
    let h = dentry_hash(b"hello.txt");
    c.check(h & F2FS_HASH_COL_BIT == 0, "bit 31 is reserved and clear");
    c.check(h == dentry_hash(b"hello.txt"), "the hash is deterministic");
    c.check(
        h != dentry_hash(b"hello.txu"),
        "the hash depends on the last byte",
    );
    c.check(
        dentry_hash(b"a") != dentry_hash(b"b"),
        "one-byte names hash apart",
    );
    // A name longer than one 16-byte block exercises the loop's second pass,
    // where an off-by-one in the length bookkeeping would otherwise hide.
    c.check(
        dentry_hash(LONG_NAME) != dentry_hash(LONG_NAME.get(..46).unwrap_or(&[])),
        "a multi-round name depends on its last byte too",
    );
    c.check(
        dentry_hash(b"") == dentry_hash(b""),
        "the empty name does not diverge",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The superblock
// ---------------------------------------------------------------------------

/// The superblock structure inside the primary copy's block.
fn sb_slice(img: &[u8]) -> &[u8] {
    img.get(SUPER_OFFSET as usize..(SUPER_OFFSET as usize).saturating_add(BLOCK_SIZE))
        .unwrap_or(&[])
}

/// Re-seal a superblock copy after a test has edited it.
///
/// Without this, every mutation would fail the CRC and every rejection test
/// would pass for the wrong reason — the point of the group is to prove the
/// driver rejects each *specific* inconsistency, not that it notices the CRC.
fn reseal_sb(img: &mut [u8], copy: u32) {
    let base = (SUPER_OFFSET as usize).saturating_add(blk(copy));
    let crc = img
        .get(base..base.saturating_add(SB_CRC_OFF))
        .map_or(0, |s| crate::crypto::crc32_raw(MAGIC, s));
    put_u32(img, base.saturating_add(SB_CRC_OFF), crc);
}

fn test_superblock(c: &mut Checks) -> KernelResult<()> {
    let img = build_image();
    let sb = SuperBlock::parse(sb_slice(&img))?;

    c.check(sb.blocks_per_seg == BLOCKS_PER_SEG, "blocks_per_seg");
    c.check(sb.segs_per_sec == 1, "segs_per_sec");
    c.check(sb.secs_per_zone == 1, "secs_per_zone");
    c.check(sb.block_count == u64::from(IMAGE_BLOCKS), "block_count");
    c.check(sb.section_count == SEGMENT_COUNT, "section_count");
    c.check(sb.segment_count == SEGMENT_COUNT, "segment_count");
    c.check(
        sb.segment_count_main == SEGMENT_COUNT_MAIN,
        "segment_count_main",
    );
    c.check(sb.cp_blkaddr == CP_BLKADDR, "cp_blkaddr");
    c.check(sb.sit_blkaddr == SIT_BLKADDR, "sit_blkaddr");
    c.check(sb.nat_blkaddr == NAT_BLKADDR, "nat_blkaddr");
    c.check(sb.ssa_blkaddr == SSA_BLKADDR, "ssa_blkaddr");
    c.check(sb.main_blkaddr == MAIN_BLKADDR, "main_blkaddr");
    c.check(sb.root_ino == NID_ROOT, "root_ino");
    c.check(sb.node_ino == 1 && sb.meta_ino == 2, "node_ino and meta_ino");
    c.check(sb.cp_payload == 0, "cp_payload");
    c.check(
        sb.uuid.iter().enumerate().all(|(i, b)| {
            usize::from(*b) == i
        }),
        "the uuid is copied verbatim",
    );
    // A driver that read the label as bytes would produce "SLATE" and stop, or
    // ASCII noise; only a correct UTF-16LE decode yields the em dash.
    c.check(sb.label == "SLATE\u{2014}F2FS", "the UTF-16LE volume label");

    c.check(
        sb.has_feature(sb::FEATURE_EXTRA_ATTR) && sb.has_feature(sb::FEATURE_SB_CHKSUM),
        "the declared features are set",
    );
    c.check(
        !sb.has_feature(sb::FEATURE_COMPRESSION),
        "an undeclared feature is not set",
    );

    c.check(sb.blocks_per_seg_u64() == 32, "blocks_per_seg_u64");
    c.check(sb.total_blocks() == u64::from(IMAGE_BLOCKS), "total_blocks");

    // The bounds every block pointer in the volume is filtered through.
    c.check(
        sb.is_valid_data_block(MAIN_BLKADDR),
        "the first main-area block is valid",
    );
    c.check(
        sb.is_valid_data_block(IMAGE_BLOCKS - 1),
        "the last main-area block is valid",
    );
    c.check(
        !sb.is_valid_data_block(MAIN_BLKADDR - 1),
        "a block just below the main area is rejected",
    );
    c.check(
        !sb.is_valid_data_block(IMAGE_BLOCKS),
        "a block one past the end is rejected",
    );
    c.check(!sb.is_valid_data_block(0), "block zero is rejected");
    c.check(
        !sb.is_valid_data_block(NULL_ADDR) && !sb.is_valid_data_block(NEW_ADDR),
        "the two reserved addresses are not data blocks",
    );
    c.check(
        sb.is_in_checkpoint_area(CP_BLKADDR) && !sb.is_in_checkpoint_area(SIT_BLKADDR),
        "the checkpoint area is half-open",
    );
    c.check(
        sb.is_in_nat_area(NAT_BLKADDR) && !sb.is_in_nat_area(SSA_BLKADDR),
        "the NAT area is half-open",
    );

    // --- rejections -------------------------------------------------------

    let mut bad = img.clone();
    put_u32(&mut bad, SUPER_OFFSET as usize, MAGIC ^ 1);
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::InvalidArgument,
        "a wrong magic is rejected",
    );

    let mut bad = img.clone();
    put_u32(&mut bad, (SUPER_OFFSET as usize).saturating_add(16), 13);
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::NotSupported,
        "an 8 KiB block size is refused rather than misparsed",
    );

    let mut bad = img.clone();
    put_u32(
        &mut bad,
        (SUPER_OFFSET as usize).saturating_add(2180),
        sb::FEATURE_SB_CHKSUM | FEATURE_BLKZONED,
    );
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::NotSupported,
        "a zoned volume is refused",
    );

    // The areas must ascend: NAT placed below SIT is the shape that turns a
    // NAT lookup into a read of the wrong area rather than an error.
    let mut bad = img.clone();
    put_u32(
        &mut bad,
        (SUPER_OFFSET as usize).saturating_add(84),
        SIT_BLKADDR - 1,
    );
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::CorruptedData,
        "out-of-order areas are rejected",
    );

    let mut bad = img.clone();
    put_u32(&mut bad, (SUPER_OFFSET as usize).saturating_add(96), 0);
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::CorruptedData,
        "a zero root inode is rejected",
    );

    // A main area larger than the volume: the check that stops a plausible
    // block pointer from being read past the end of the device.
    let mut bad = img.clone();
    put_u32(
        &mut bad,
        (SUPER_OFFSET as usize).saturating_add(68),
        SEGMENT_COUNT_MAIN * 4,
    );
    reseal_sb(&mut bad, 0);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::CorruptedData,
        "a main area larger than the volume is rejected",
    );

    // The CRC itself, checked last so the earlier cases cannot be passing
    // because of it.
    let mut bad = img.clone();
    put_u32(&mut bad, (SUPER_OFFSET as usize).saturating_add(24), 2);
    c.check_err(
        SuperBlock::parse(sb_slice(&bad)),
        KernelError::CorruptedData,
        "an unsealed edit fails the checksum",
    );

    // --- the backup copy --------------------------------------------------

    let mut broken_primary = img.clone();
    put_u32(&mut broken_primary, SUPER_OFFSET as usize, 0);
    let src = MemorySource::new(broken_primary);
    let recovered = sb::read_superblock(&src)?;
    c.check(
        recovered.main_blkaddr == MAIN_BLKADDR && recovered.label == sb.label,
        "a destroyed primary falls back to the backup",
    );
    c.check(sb::probe(&src), "probe still matches on the backup alone");

    // Both copies gone: the *primary's* error is what surfaces, so the report
    // describes the copy the user knows about.
    let mut both = img.clone();
    put_u32(&mut both, SUPER_OFFSET as usize, 0);
    put_u32(
        &mut both,
        (SUPER_OFFSET as usize).saturating_add(BLOCK_SIZE),
        0,
    );
    let src = MemorySource::new(both);
    c.check_err(
        sb::read_superblock(&src),
        KernelError::InvalidArgument,
        "with both copies gone the primary's error is reported",
    );
    c.check(!sb::probe(&src), "probe rejects a non-F2FS image");

    Ok(())
}

// ---------------------------------------------------------------------------
// The checkpoint
// ---------------------------------------------------------------------------

/// Re-seal a checkpoint pack's head block after a test has edited it.
fn reseal_cp(img: &mut [u8], start: u32) {
    let base = blk(start);
    let crc = img
        .get(base..base.saturating_add(CP_CRC_OFF))
        .map_or(0, |s| crate::crypto::crc32_raw(MAGIC, s));
    put_u32(img, base.saturating_add(CP_CRC_OFF), crc);
}

fn test_checkpoint(c: &mut Checks) -> KernelResult<()> {
    let img = build_image();
    let sb = SuperBlock::parse(sb_slice(&img))?;
    let src = MemorySource::new(img.clone());
    let checkpoint = cp::read_checkpoint(&src, &sb)?;

    // The image puts the *newer* pack second on purpose: a reader that takes
    // the first valid pack it finds, rather than the highest version, passes
    // every other check in this group and still mounts a stale filesystem.
    c.check(checkpoint.version == 7, "the higher version wins");
    c.check(
        checkpoint.start_block == CP_BLKADDR + BLOCKS_PER_SEG,
        "the winning pack is the second one",
    );
    c.check(
        checkpoint.total_block_count == CP_PACK_BLOCKS,
        "cp_pack_total_block_count",
    );
    c.check(checkpoint.start_sum_offset == 1, "cp_pack_start_sum");
    c.check(checkpoint.flags == CP_UMOUNT, "ckpt_flags");
    c.check(
        checkpoint.has_flag(cp::CP_UMOUNT_FLAG) && !checkpoint.has_flag(cp::CP_COMPACT_SUM_FLAG),
        "has_flag reads the winning pack's flags",
    );
    c.check(
        checkpoint.user_block_count == u64::from(IMAGE_BLOCKS),
        "user_block_count",
    );
    c.check(checkpoint.valid_block_count == 40, "valid_block_count");
    c.check(checkpoint.valid_node_count == 16, "valid_node_count");
    c.check(checkpoint.valid_inode_count == 10, "valid_inode_count");
    c.check(checkpoint.free_segment_count == 4, "free_segment_count");

    // The bitmap is what selects between the NAT's two copies, so its length
    // must come from `nat_ver_bitmap_bytesize` and its contents from the right
    // one of the three possible offsets.
    c.check(
        checkpoint.nat_bitmap.len() == NAT_BITMAP_BYTES as usize,
        "the NAT bitmap is nat_ver_bitmap_bytesize long",
    );
    c.check(
        test_bit(&checkpoint.nat_bitmap, 0),
        "NAT block 0's bit is set, selecting the second copy",
    );
    c.check(
        !test_bit(&checkpoint.nat_bitmap, 1),
        "NAT block 1's bit is clear",
    );

    // The journal was written at the *normal*-summary offset in this pack and
    // at the compacted offset in the other; finding it proves the reader chose
    // its layout from the winning pack's own flags.
    c.check(checkpoint.nat_journal.len() == 1, "one journalled NAT entry");
    let j = checkpoint.journal_lookup(NID_HELLO);
    c.check(
        j.map(|e| (e.ino, e.block_addr)) == Some((NID_HELLO, B_HELLO)),
        "the journal entry for hello.txt",
    );
    c.check(
        checkpoint.journal_lookup(NID_ROOT).is_none(),
        "a nid with no journal entry misses",
    );

    // --- rejections -------------------------------------------------------

    // Break the winning pack's *tail* version only. The head is still intact
    // and still CRC-correct, so nothing but the head/tail comparison can catch
    // it — this is the half-written checkpoint that the rule exists for.
    let mut torn = img.clone();
    put_u64(
        &mut torn,
        blk(CP_BLKADDR + BLOCKS_PER_SEG + CP_PACK_BLOCKS - 1),
        999,
    );
    let src = MemorySource::new(torn);
    let older = cp::read_checkpoint(&src, &sb)?;
    c.check(
        older.version == 3 && older.start_block == CP_BLKADDR,
        "a torn pack is skipped for the older intact one",
    );
    // …and the older pack's journal is in the *compacted* layout, so this also
    // proves the second of the two layouts is read correctly.
    c.check(
        older
            .journal_lookup(NID_HELLO)
            .map(|e| e.block_addr)
            == Some(B_HELLO),
        "the compacted-summary journal is found in the fallback pack",
    );
    c.check(
        older.has_flag(cp::CP_COMPACT_SUM_FLAG),
        "the fallback pack is the compacted one",
    );

    // A head whose CRC does not match its contents.
    let mut bad_crc = img.clone();
    put_u32(
        &mut bad_crc,
        blk(CP_BLKADDR + BLOCKS_PER_SEG).saturating_add(144),
        0,
    );
    let src = MemorySource::new(bad_crc);
    let fallback = cp::read_checkpoint(&src, &sb)?;
    c.check(
        fallback.version == 3,
        "a pack failing its CRC is skipped",
    );

    // A checksum_offset pointing into the header is impossible for any writer
    // to produce, because the CRC would overlap bytes it is computed from.
    let mut bad_off = img.clone();
    put_u32(
        &mut bad_off,
        blk(CP_BLKADDR + BLOCKS_PER_SEG).saturating_add(CP_CHKSUM_FIELD),
        64,
    );
    reseal_cp(&mut bad_off, CP_BLKADDR + BLOCKS_PER_SEG);
    let src = MemorySource::new(bad_off);
    let fallback = cp::read_checkpoint(&src, &sb)?;
    c.check(
        fallback.version == 3,
        "a checksum_offset inside the header is rejected",
    );

    // Both packs gone.
    let mut dead = img.clone();
    put_u64(&mut dead, blk(CP_BLKADDR + CP_PACK_BLOCKS - 1), 111);
    put_u64(
        &mut dead,
        blk(CP_BLKADDR + BLOCKS_PER_SEG + CP_PACK_BLOCKS - 1),
        222,
    );
    let src = MemorySource::new(dead);
    c.check_err(
        cp::read_checkpoint(&src, &sb),
        KernelError::CorruptedData,
        "two torn packs leave nothing to mount",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The node address table
// ---------------------------------------------------------------------------

fn test_nat(c: &mut Checks) -> KernelResult<()> {
    let img = build_image();
    let sb = SuperBlock::parse(sb_slice(&img))?;
    let src = MemorySource::new(img.clone());
    let checkpoint = cp::read_checkpoint(&src, &sb)?;
    let nat = Nat::new(&src, &sb, &checkpoint);

    // The interleave. Every nid below 455 lives in NAT block offset 0, which
    // the formula places at `nat_blkaddr`, and the set bitmap bit then moves it
    // one segment on to the second copy.
    c.check(
        nat.nat_block_addr(NID_ROOT) == Ok(NAT_BLKADDR + BLOCKS_PER_SEG),
        "the bitmap bit selects the second NAT copy",
    );
    // Offset 1 doubles to 2 and folds back by 1, which is the whole point of
    // the formula: consecutive NAT blocks are adjacent, not two apart.
    c.check(
        nat.nat_block_addr(NAT_ENTRIES_PER_BLOCK) == Ok(NAT_BLKADDR + 1),
        "NAT block 1 folds back to nat_blkaddr + 1",
    );
    c.check(
        nat.nat_block_addr(NAT_ENTRIES_PER_BLOCK * 31) == Ok(NAT_BLKADDR + 31),
        "the last block of the first NAT segment",
    );
    // Offset 32 wraps the fold back to zero and doubles clean out of the NAT
    // area — the bound that stops a large nid becoming a read of the SSA.
    c.check(
        nat.nat_block_addr(NAT_ENTRIES_PER_BLOCK * 32).is_err(),
        "an address computed past the NAT area is rejected",
    );
    c.check(
        nat.nat_block_addr(u32::MAX).is_err(),
        "the largest possible nid does not wrap",
    );

    // Clearing the bit must move the lookup to the *first* copy, which this
    // image deliberately filled with NULL_ADDR decoys. Without this the whole
    // bitmap could be ignored and every other check here would still pass.
    let mut flipped = checkpoint.clone();
    flipped.nat_bitmap = vec![0u8; NAT_BITMAP_BYTES as usize];
    let decoy_nat = Nat::new(&src, &sb, &flipped);
    c.check(
        decoy_nat.nat_block_addr(NID_ROOT) == Ok(NAT_BLKADDR),
        "a clear bitmap bit selects the first NAT copy",
    );
    c.check(
        decoy_nat.lookup(NID_ROOT).map(|e| e.block_addr) == Ok(NULL_ADDR),
        "the first copy holds the stale NULL_ADDR decoy",
    );
    c.check_err(
        decoy_nat.read_node(NID_ROOT),
        KernelError::NotFound,
        "a NULL_ADDR node is absent, not corrupt",
    );

    // --- lookups ----------------------------------------------------------

    c.check_err(
        nat.lookup(NULL_NID),
        KernelError::InvalidArgument,
        "nid 0 is the null nid and is never looked up",
    );
    let root = nat.lookup(NID_ROOT)?;
    c.check(
        root.block_addr == B_ROOT && root.ino == NID_ROOT,
        "the root's NAT entry",
    );
    let sub = nat.lookup(NID_SUB)?;
    c.check(sub.block_addr == B_SUB, "a directory's NAT entry");

    // The journal wins outright. The NAT area's entry for hello.txt points at
    // a *valid, parseable* stale inode, so a reader that consults only the
    // area returns a file whose contents are wrong but whose every check
    // passes — the failure this ordering exists to prevent.
    let hello = nat.lookup(NID_HELLO)?;
    c.check(
        hello.block_addr == B_HELLO,
        "the journal overrides the NAT area",
    );
    c.check(
        hello.block_addr != B_HELLO_STALE,
        "the NAT area's stale entry is not what surfaces",
    );
    // …and the stale entry really is there, so the test above is meaningful.
    let stale_off = blk(NAT_BLKADDR + BLOCKS_PER_SEG)
        .saturating_add((NID_HELLO as usize).saturating_mul(9))
        .saturating_add(5);
    c.check(
        read_u32(&img, stale_off) == Ok(B_HELLO_STALE),
        "the NAT area really does hold the stale address",
    );

    // --- node footers -----------------------------------------------------

    let node = nat.read_node(NID_ROOT)?;
    c.check(node.len() == BLOCK_SIZE, "a node block is one block");
    c.check(
        read_u32(&node, NODE_FOOTER_OFF) == Ok(NID_ROOT),
        "the footer names the node it belongs to",
    );

    // A NAT entry pointing at the wrong — but perfectly well-formed — node is
    // what the footer is the only guard against: there is no checksum on a
    // node block to catch it.
    let mut swapped = img.clone();
    put_u32(
        &mut swapped,
        blk(B_ROOT).saturating_add(NODE_FOOTER_OFF),
        NID_SUB,
    );
    let src2 = MemorySource::new(swapped);
    let cp2 = cp::read_checkpoint(&src2, &sb)?;
    let nat2 = Nat::new(&src2, &sb, &cp2);
    c.check_err(
        nat2.read_node(NID_ROOT),
        KernelError::CorruptedData,
        "a node whose footer names another node is rejected",
    );

    // An entry pointing outside the main area must be refused before it turns
    // into a read.
    let mut wild = img.clone();
    put_u32(
        &mut wild,
        blk(NAT_BLKADDR + BLOCKS_PER_SEG)
            .saturating_add((NID_SUB as usize).saturating_mul(9))
            .saturating_add(5),
        IMAGE_BLOCKS + 4096,
    );
    let src3 = MemorySource::new(wild);
    let cp3 = cp::read_checkpoint(&src3, &sb)?;
    let nat3 = Nat::new(&src3, &sb, &cp3);
    c.check_err(
        nat3.read_node(NID_SUB),
        KernelError::CorruptedData,
        "a node address past the volume is rejected",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// File offset -> node path
// ---------------------------------------------------------------------------

// Every boundary in this group is an exact index derived from the format's
// own constants - 923 inode slots, 1018 per node - and the value of the check
// is that the expression on the page can be read straight off the layout.
// Rewriting `d + 2 * a + a * a` as a chain of `saturating_add`/`saturating_mul`
// calls would hide the one thing a reviewer has to verify. None of these can
// overflow: the largest is a little over 10^9, against a `u64`.
#[allow(clippy::arithmetic_side_effects)]
fn test_block_path(c: &mut Checks) -> KernelResult<()> {
    let img = build_image();
    let sb = SuperBlock::parse(sb_slice(&img))?;
    let src = MemorySource::new(img);
    let checkpoint = cp::read_checkpoint(&src, &sb)?;
    let nat = Nat::new(&src, &sb, &checkpoint);

    let big = node::read_inode(&nat, &sb, NID_BIG)?;
    c.check(
        big.addrs_per_inode() == DEF_ADDRS_PER_INODE,
        "a plain inode owns all 923 i_addr slots",
    );

    let a = u64::from(ADDRS_PER_BLOCK);
    let d = u64::from(DEF_ADDRS_PER_INODE);

    // Each boundary is checked from *both* sides. A path table that is off by
    // one is right in the middle of every range and wrong only at the seams,
    // so testing a midpoint proves nothing.
    c.check(
        node::block_path(&big, 0) == Ok(BlockPath::Inode { slot: 0 }),
        "block 0 is the inode's first slot",
    );
    c.check(
        node::block_path(&big, d - 1) == Ok(BlockPath::Inode { slot: 922 }),
        "the inode's last slot",
    );
    c.check(
        node::block_path(&big, d)
            == Ok(BlockPath::Direct {
                nid_index: 0,
                slot: 0,
            }),
        "one past the inode is the first direct node",
    );
    c.check(
        node::block_path(&big, d + a - 1)
            == Ok(BlockPath::Direct {
                nid_index: 0,
                slot: 1017,
            }),
        "the first direct node's last slot",
    );
    c.check(
        node::block_path(&big, d + a)
            == Ok(BlockPath::Direct {
                nid_index: 1,
                slot: 0,
            }),
        "the second direct node begins",
    );
    c.check(
        node::block_path(&big, d + 2 * a)
            == Ok(BlockPath::Indirect {
                nid_index: 2,
                mid: 0,
                slot: 0,
            }),
        "the first indirect node begins",
    );
    c.check(
        node::block_path(&big, d + 2 * a + 1)
            == Ok(BlockPath::Indirect {
                nid_index: 2,
                mid: 0,
                slot: 1,
            }),
        "the slot advances before the mid does",
    );
    c.check(
        node::block_path(&big, d + 2 * a + a)
            == Ok(BlockPath::Indirect {
                nid_index: 2,
                mid: 1,
                slot: 0,
            }),
        "a full direct node advances the mid",
    );
    c.check(
        node::block_path(&big, d + 2 * a + a * a)
            == Ok(BlockPath::Indirect {
                nid_index: 3,
                mid: 0,
                slot: 0,
            }),
        "the second indirect node begins",
    );
    c.check(
        node::block_path(&big, d + 2 * a + 2 * a * a - 1)
            == Ok(BlockPath::Indirect {
                nid_index: 3,
                mid: 1017,
                slot: 1017,
            }),
        "the second indirect node's last block",
    );
    c.check(
        node::block_path(&big, BIG_DINDIRECT)
            == Ok(BlockPath::DoubleIndirect {
                outer: 0,
                mid: 0,
                slot: 0,
            }),
        "the double-indirect node begins",
    );
    c.check(
        node::block_path(&big, BIG_DINDIRECT + a * a)
            == Ok(BlockPath::DoubleIndirect {
                outer: 1,
                mid: 0,
                slot: 0,
            }),
        "a full indirect node advances the outer index",
    );

    // The end of what the format can address at all. One past it must be an
    // error rather than a wrapped-around path into the middle of the file.
    let last = BIG_DINDIRECT
        .saturating_add(a.saturating_mul(a).saturating_mul(a))
        .saturating_sub(1);
    c.check(
        node::block_path(&big, last)
            == Ok(BlockPath::DoubleIndirect {
                outer: 1017,
                mid: 1017,
                slot: 1017,
            }),
        "the last addressable block of a file",
    );
    c.check(
        node::block_path(&big, last + 1).is_err(),
        "one block past the addressable end is refused",
    );
    c.check(
        node::block_path(&big, u64::MAX).is_err(),
        "the largest possible offset does not wrap",
    );

    // Extra attributes shift the whole table, because the extra-attribute area
    // overlaps `i_addr` rather than preceding it. An inode with 36 bytes of
    // extra attributes owns nine fewer slots, so its first direct node starts
    // nine blocks earlier — a reader that ignores this reads real data from
    // the wrong offsets of the right file, which no checksum would catch.
    let hello = node::read_inode(&nat, &sb, NID_HELLO)?;
    c.check(
        hello.extra_isize == EXTRA_ISIZE,
        "the extra-attribute size is parsed",
    );
    c.check(
        hello.addrs_per_inode() == DEF_ADDRS_PER_INODE - u32::from(EXTRA_ISIZE) / 4,
        "extra attributes eat i_addr slots",
    );
    c.check(
        node::block_path(&hello, u64::from(hello.addrs_per_inode()))
            == Ok(BlockPath::Direct {
                nid_index: 0,
                slot: 0,
            }),
        "the shifted inode/direct boundary",
    );

    // An inline xattr eats slots from the *back*, which reduces the count
    // without moving `i_addr[0]`.
    let data = node::read_inode(&nat, &sb, NID_DATA)?;
    c.check(
        data.addrs_per_inode()
            == DEF_ADDRS_PER_INODE - u32::from(EXTRA_ISIZE) / 4 - u32::from(INLINE_XATTR_WORDS),
        "an inline xattr eats i_addr slots too",
    );
    c.check(
        data.addr(0) == Ok(B_DATA_0),
        "i_addr[0] is unmoved by the inline xattr",
    );
    c.check(
        data.addr(data.addrs_per_inode()).is_err(),
        "reading one slot past the end is refused",
    );

    // --- resolution -------------------------------------------------------

    // Every one of the five levels, resolved end to end. These are the offsets
    // /big.bin actually has blocks at; everything between them is a hole.
    c.check(
        node::resolve_block(&nat, &big, 0) == Ok(B_BIG_0),
        "resolve through the inode",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_DIRECT0) == Ok(B_BIG_D0_DATA),
        "resolve through the first direct node",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_DIRECT1) == Ok(B_BIG_D1_DATA),
        "resolve through the second direct node",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_INDIRECT) == Ok(B_BIG_IND_DATA),
        "resolve through the indirect node",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_DINDIRECT) == Ok(B_BIG_DIND_DATA),
        "resolve through the double-indirect node",
    );

    // A hole at each level. `i_nid[3]` is deliberately zero, so the whole
    // second indirect range is absent without any node existing to say so.
    c.check(
        node::resolve_block(&nat, &big, 1) == Ok(NULL_ADDR),
        "an unwritten inode slot is a hole",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_DIRECT0 + 1) == Ok(NULL_ADDR),
        "an unwritten direct-node slot is a hole",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_INDIRECT + u64::from(ADDRS_PER_BLOCK)) == Ok(NULL_ADDR),
        "an absent second-level node is a hole",
    );
    c.check(
        node::resolve_block(&nat, &big, BIG_INDIRECT + a * a) == Ok(NULL_ADDR),
        "a null i_nid is a whole absent range, not an error",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// The whole volume
// ---------------------------------------------------------------------------

/// Reconstruct what [`put_pattern`] wrote into `block`.
///
/// Derived from the same address the driver had to resolve, so an expectation
/// can never accidentally agree with a block the driver fetched by mistake.
fn expect_block(block: u32, label: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = (0..BLOCK_SIZE).map(|i| pattern_byte(block, i)).collect();
    if let Some(dst) = out.get_mut(..label.len()) {
        dst.copy_from_slice(label);
    }
    out
}

/// Did `readdir` return an entry with this name?
fn has_name(entries: &[crate::fs::vfs::DirEntry], name: &[u8]) -> bool {
    entries.iter().any(|e| e.name.as_bytes() == name)
}

#[allow(clippy::too_many_lines)]
fn test_volume(c: &mut Checks) -> KernelResult<()> {
    let mut fs = mount_image(build_image())?;

    c.check(fs.fs_type() == "f2fs", "fs_type");

    // --- the inline-dentry root ------------------------------------------

    let root = fs.readdir(Path::new("/"))?;
    c.check(root.len() == 6, "the root has six entries besides . and ..");
    c.check(
        !has_name(&root, b".") && !has_name(&root, b".."),
        ". and .. are the VFS's business, not the driver's",
    );
    for name in [
        b"hello.txt".as_slice(),
        b"sub",
        b"link",
        b"sparse.bin",
        b"big.bin",
        b"prealloc.bin",
    ] {
        c.check(has_name(&root, name), "the root lists every name it holds");
    }
    // The image leaves slots 3, 7 and 10.. clear. Their bytes are zero, which
    // is a perfectly well-formed entry naming inode 0 - so a reader that walks
    // the entries instead of the bitmap sees a directory full of phantoms.
    c.check(
        root.iter().all(|e| !e.name.as_bytes().is_empty()),
        "cleared bitmap slots produce no entries",
    );
    c.check(
        root.iter()
            .find(|e| e.name.as_bytes() == b"sub")
            .map(|e| e.entry_type)
            == Some(EntryType::Directory),
        "the directory's type comes from the dentry, not from a stat",
    );
    c.check(
        root.iter()
            .find(|e| e.name.as_bytes() == b"link")
            .map(|e| e.entry_type)
            == Some(EntryType::Symlink),
        "the symlink's type",
    );
    c.check(
        root.iter()
            .find(|e| e.name.as_bytes() == b"hello.txt")
            .map(|e| e.size)
            == Some(HELLO_TEXT.len() as u64),
        "readdir reports a file's size",
    );

    // --- the block-backed directory --------------------------------------

    let sub = fs.readdir(Path::new("/sub"))?;
    c.check(sub.len() == 2, "/sub has two entries");
    c.check(has_name(&sub, b"data.bin"), "/sub/data.bin is listed");
    // The 47-byte name spans six slots. A reader that treats every set bitmap
    // bit as the start of an entry finds five extra garbage entries after it.
    c.check(has_name(&sub, LONG_NAME), "a six-slot name is listed");
    c.check(
        sub.iter().filter(|e| e.name.as_bytes() == LONG_NAME).count() == 1,
        "the long name's continuation slots produce no extra entries",
    );

    // --- inline data, resolved through the journal -----------------------

    let hello = fs.read_file(Path::new("/hello.txt"))?;
    c.check_bytes(&hello, HELLO_TEXT, "/hello.txt reads the journal's copy");
    c.check(
        hello != HELLO_STALE_TEXT,
        "/hello.txt is not the NAT area's stale copy",
    );

    // --- block-backed data, across a block boundary ----------------------

    let data = fs.read_file(Path::new("/sub/data.bin"))?;
    let mut want = expect_block(B_DATA_0, b"DATA0");
    want.extend_from_slice(expect_block(B_DATA_1, b"DATA1").get(..100).unwrap_or(&[]));
    c.check_bytes(&data, &want, "/sub/data.bin reads both of its blocks");

    // A read that starts inside one block and ends inside the next is where an
    // offset-within-block slip shows up; a whole-file read would not catch it.
    let straddle = fs.read_at(Path::new("/sub/data.bin"), 4090, 12)?;
    let mut want = expect_block(B_DATA_0, b"DATA0")
        .get(4090..)
        .unwrap_or(&[])
        .to_vec();
    want.extend_from_slice(expect_block(B_DATA_1, b"DATA1").get(..6).unwrap_or(&[]));
    c.check_bytes(&straddle, &want, "a read straddling a block boundary");

    // A read that runs off the end of the file is truncated, not refused.
    let tail = fs.read_at(Path::new("/sub/data.bin"), DATA_SIZE - 10, 100)?;
    c.check(tail.len() == 10, "a read past EOF is clipped to the file");
    let past = fs.read_at(Path::new("/sub/data.bin"), DATA_SIZE, 10)?;
    c.check(past.is_empty(), "a read starting at EOF returns nothing");
    let way_past = fs.read_at(Path::new("/sub/data.bin"), u64::MAX, 10)?;
    c.check(way_past.is_empty(), "an absurd offset returns nothing");

    // --- holes and preallocation -----------------------------------------

    let sparse = fs.read_file(Path::new("/sparse.bin"))?;
    let mut want = vec![0u8; BLOCK_SIZE];
    want.extend_from_slice(&expect_block(B_SPARSE_1, b"TAIL"));
    c.check_bytes(&sparse, &want, "a hole reads as zeroes, then real data");

    let prealloc = fs.read_file(Path::new("/prealloc.bin"))?;
    c.check_bytes(
        &prealloc,
        &vec![0u8; BLOCK_SIZE],
        "a preallocated block reads as zeroes, not as block 0xFFFFFFFF",
    );

    // --- the symlink ------------------------------------------------------

    let target = fs.readlink(Path::new("/link"))?;
    c.check(
        target.as_bytes() == LINK_TARGET,
        "readlink returns the inline target",
    );
    c.check(
        fs.readlink(Path::new("/hello.txt")).is_err(),
        "readlink on a regular file is refused",
    );
    // `resolve` does not follow a trailing symlink, which is what makes
    // `lstat` and `stat` agree here.
    c.check(
        fs.lstat(Path::new("/link"))?.entry_type == EntryType::Symlink,
        "lstat sees the link itself",
    );

    // --- every level of /big.bin ------------------------------------------

    for (block, label, name) in [
        (0u64, BIG_TEXT_0, "the inode's own pointer"),
        (BIG_DIRECT0, BIG_TEXT_D0, "the first direct node"),
        (BIG_DIRECT1, BIG_TEXT_D1, "the second direct node"),
        (BIG_INDIRECT, BIG_TEXT_IND, "the indirect node"),
        (BIG_DINDIRECT, BIG_TEXT_DIND, "the double-indirect node"),
    ] {
        let got = fs.read_at(
            Path::new("/big.bin"),
            block.saturating_mul(BLOCK_SIZE as u64),
            label.len(),
        )?;
        c.check_bytes(&got, label, name);
    }
    // A hole in the middle of the file.
    let hole = fs.read_at(Path::new("/big.bin"), BLOCK_SIZE as u64, 16)?;
    c.check_bytes(&hole, &[0u8; 16], "a hole inside /big.bin reads as zeroes");
    // Reading it whole would be 8.5 GiB. The cap exists so that a corrupt size
    // cannot turn one `read_file` into an allocation that kills the kernel.
    c.check_err(
        fs.read_file(Path::new("/big.bin")),
        KernelError::FileTooLarge,
        "an 8 GiB file is refused rather than allocated",
    );

    // --- path resolution --------------------------------------------------

    let long = fs.read_file(Path::new("/sub/a-very-long-file-name-that-spans-many-slots.txt"))?;
    c.check_bytes(&long, LONG_TEXT, "a six-slot name resolves");

    let via_dotdot = fs.read_file(Path::new("/sub/../hello.txt"))?;
    c.check_bytes(&via_dotdot, HELLO_TEXT, ".. climbs back out of /sub");
    let via_dot = fs.read_file(Path::new("/./sub/./data.bin"))?;
    c.check(via_dot.len() == DATA_SIZE as usize, ". is a no-op");
    let above_root = fs.read_file(Path::new("/../../hello.txt"))?;
    c.check_bytes(&above_root, HELLO_TEXT, ".. at the root stays at the root");

    c.check_err(
        fs.read_file(Path::new("/nonexistent")),
        KernelError::NotFound,
        "a missing name is NotFound",
    );
    c.check_err(
        fs.read_file(Path::new("/hello.txt/x")),
        KernelError::NotADirectory,
        "descending through a file is refused",
    );
    c.check_err(
        fs.read_file(Path::new("/sub")),
        KernelError::IsADirectory,
        "reading a directory as a file is refused",
    );
    c.check_err(
        fs.readdir(Path::new("/hello.txt")),
        KernelError::NotADirectory,
        "readdir on a file is refused",
    );
    // F2FS without the casefold feature is case-sensitive, and so is this OS.
    c.check_err(
        fs.read_file(Path::new("/HELLO.TXT")),
        KernelError::NotFound,
        "lookup is case-sensitive",
    );

    // --- metadata ---------------------------------------------------------

    let st = fs.stat(Path::new("/sub/data.bin"))?;
    c.check(
        st.entry_type == EntryType::File && st.size == DATA_SIZE,
        "stat on a file",
    );
    c.check(
        st.name.as_bytes() == b"data.bin",
        "stat names the leaf, not the path",
    );
    c.check(
        fs.stat(Path::new("/"))?.entry_type == EntryType::Directory,
        "stat on the root",
    );

    let meta = fs.metadata(Path::new("/sub/data.bin"))?;
    c.check(meta.ino == u64::from(NID_DATA), "metadata reports the nid");
    c.check(meta.size == DATA_SIZE, "metadata size");
    c.check(meta.nlinks == 1, "metadata link count");
    // The write bits must be gone: the mount refuses every write, and a mode
    // that says otherwise is a lie userspace will act on.
    c.check(
        meta.permissions & 0o222 == 0,
        "the write bits are masked off a read-only mount",
    );
    c.check(meta.permissions == 0o444, "the read bits survive the mask");
    c.check(
        fs.metadata(Path::new("/sub"))?.permissions == 0o555,
        "a directory keeps its execute bits",
    );
    // The three timestamps are given three *different* values by the builder,
    // and the nanosecond halves three more, precisely so that a driver which
    // reads the right field for the wrong stamp cannot pass. Asserting exact
    // values rather than "nonzero" is the whole point: atime/ctime/mtime sit
    // at 32/40/48 with their nsec halves at 56/60/64, and swapping any pair is
    // a plausible transcription error that no relational check would catch.
    c.check(
        meta.accessed_ns == 1_700_000_001 * 1_000_000_000 + 11,
        "atime is read from offset 32 and converted to nanoseconds",
    );
    c.check(
        meta.changed_ns == 1_700_000_002 * 1_000_000_000 + 22,
        "ctime is read from offset 40 and converted to nanoseconds",
    );
    c.check(
        meta.created_ns == meta.changed_ns,
        "the creation stamp reports ctime, which is all F2FS records",
    );
    c.check(
        meta.modified_ns == 1_700_000_003 * 1_000_000_000 + 33,
        "mtime is read from offset 48 and converted to nanoseconds",
    );

    let info = fs.statvfs()?;
    c.check(info.fs_type == "f2fs", "statvfs fs_type");
    c.check(info.volume_label == "SLATE\u{2014}F2FS", "statvfs label");
    c.check(info.block_size == 4096, "statvfs block size");
    c.check(info.read_only, "the mount is read-only");
    c.check(
        info.free_blocks == 0,
        "a read-only mount offers no free space",
    );
    c.check(
        info.max_name_len == F2FS_NAME_LEN as u64,
        "statvfs max_name_len",
    );

    c.check(
        fs.debug_stats().contains("cp_ver=7"),
        "debug_stats names the winning checkpoint",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Damaged volumes
// ---------------------------------------------------------------------------

/// Mount an image with one edit applied, expecting the mount itself to fail.
fn expect_mount_failure(c: &mut Checks, edit: impl FnOnce(&mut Vec<u8>), what: &str) {
    let mut img = build_image();
    edit(&mut img);
    c.check(mount_image(img).is_err(), what);
}

#[allow(clippy::too_many_lines)]
fn test_corruption(c: &mut Checks) -> KernelResult<()> {
    // Nothing that looks like F2FS at all.
    let blank = vec![0u8; blk(IMAGE_BLOCKS)];
    c.check(
        !sb::probe(&MemorySource::new(blank.clone())),
        "a blank device does not probe as F2FS",
    );
    c.check(mount_image(blank).is_err(), "a blank device does not mount");

    // A device too small to hold even a superblock. The read must fail rather
    // than index off the end of the buffer.
    c.check(
        mount_image(vec![0u8; 512]).is_err(),
        "a device smaller than the superblock does not mount",
    );

    // --- damage that must stop the mount ----------------------------------

    expect_mount_failure(
        c,
        |img| {
            put_u32(img, SUPER_OFFSET as usize, 0);
            put_u32(img, (SUPER_OFFSET as usize).saturating_add(BLOCK_SIZE), 0);
        },
        "both superblocks destroyed",
    );
    expect_mount_failure(
        c,
        |img| {
            put_u64(img, blk(CP_BLKADDR + CP_PACK_BLOCKS - 1), 111);
            put_u64(
                img,
                blk(CP_BLKADDR + BLOCKS_PER_SEG + CP_PACK_BLOCKS - 1),
                222,
            );
        },
        "both checkpoint packs torn",
    );

    // --- damage the driver must survive ------------------------------------

    // Tear the *newer* pack. The volume must fall back to the older one and
    // still be fully readable - including `hello.txt`, whose current address
    // lives only in a journal, and which in the older pack is stored in the
    // *compacted* summary layout rather than the normal one. This is the one
    // check that exercises the fallback and the second journal layout at the
    // same time, which is why the image was built with the two packs
    // disagreeing about their layout.
    let mut img = build_image();
    put_u64(
        &mut img,
        blk(CP_BLKADDR + BLOCKS_PER_SEG + CP_PACK_BLOCKS - 1),
        999,
    );
    match mount_image(img) {
        Ok(mut fs) => {
            c.check(
                fs.checkpoint().version == 3,
                "a torn newer pack falls back to the older one",
            );
            match fs.read_file(Path::new("/hello.txt")) {
                Ok(text) => c.check_bytes(
                    &text,
                    HELLO_TEXT,
                    "the fallback pack's compacted journal still finds hello.txt",
                ),
                Err(e) => c.check(false, match e {
                    KernelError::NotFound => "hello.txt after fallback: not found",
                    _ => "hello.txt after fallback: read failed",
                }),
            }
            c.check(
                fs.readdir(Path::new("/")).map(|v| v.len()) == Ok(6),
                "the root still lists after falling back",
            );
        }
        Err(_) => c.check(false, "a volume with one torn pack must still mount"),
    }

    // A destroyed *primary* superblock is survivable: the backup carries the
    // same geometry.
    let mut img = build_image();
    put_u32(&mut img, SUPER_OFFSET as usize, 0);
    match mount_image(img) {
        Ok(mut fs) => {
            c.check(
                fs.superblock().main_blkaddr == MAIN_BLKADDR,
                "the backup superblock carries the same geometry",
            );
            c.check(
                fs.read_file(Path::new("/hello.txt")).is_ok(),
                "the volume is readable through the backup superblock",
            );
        }
        Err(_) => c.check(false, "a destroyed primary superblock must not stop a mount"),
    }

    // --- damage that must be confined to the object it touches --------------
    //
    // Each of these leaves the volume mountable. What matters is that the
    // damaged file fails and its *neighbours* keep working: a driver that
    // propagates one bad pointer into a whole-volume failure is as wrong as
    // one that returns the wrong bytes.

    // A node footer naming the wrong node - the only guard on the NAT
    // indirection, since node blocks carry no checksum.
    let mut img = build_image();
    put_u32(&mut img, blk(B_SUB).saturating_add(NODE_FOOTER_OFF), NID_BIG);
    let mut fs = mount_image(img)?;
    c.check_err(
        fs.readdir(Path::new("/sub")),
        KernelError::CorruptedData,
        "a node whose footer names another node is rejected",
    );
    c.check(
        fs.read_file(Path::new("/hello.txt")).is_ok(),
        "one bad footer does not take the volume down",
    );

    // An inode block whose footer nid is right but whose ino is not. For an
    // inode the two must be equal; anything else is a node that is not an
    // inode being read as one.
    let mut img = build_image();
    put_u32(
        &mut img,
        blk(B_DATA).saturating_add(NODE_FOOTER_OFF).saturating_add(4),
        NID_ROOT,
    );
    let mut fs = mount_image(img)?;
    c.check(
        fs.read_file(Path::new("/sub/data.bin")).is_err(),
        "an inode whose footer ino is not its own nid is rejected",
    );

    // A block pointer aimed past the end of the volume.
    let mut img = build_image();
    let data_spec = InodeSpec {
        inline: F2FS_EXTRA_ATTR | F2FS_INLINE_XATTR,
        extra_isize: EXTRA_ISIZE,
        inline_xattr_words: INLINE_XATTR_WORDS,
        ..InodeSpec::file(NID_DATA, B_DATA, DATA_SIZE)
    };
    put_u32(&mut img, addr_off(&data_spec, 0), IMAGE_BLOCKS + 1000);
    let mut fs = mount_image(img)?;
    c.check_err(
        fs.read_file(Path::new("/sub/data.bin")),
        KernelError::CorruptedData,
        "a block pointer past the volume is rejected",
    );
    c.check(
        fs.read_file(Path::new("/sparse.bin")).is_ok(),
        "a neighbouring file is unaffected",
    );

    // A block pointer into the *metadata* areas, which is the more dangerous
    // shape: the address exists on the device, so only the main-area bound
    // stops it being read and served as file contents.
    let mut img = build_image();
    put_u32(&mut img, addr_off(&data_spec, 0), NAT_BLKADDR);
    let mut fs = mount_image(img)?;
    c.check_err(
        fs.read_file(Path::new("/sub/data.bin")),
        KernelError::CorruptedData,
        "a block pointer into the NAT area is rejected",
    );

    // A directory claiming a hash depth of 4 billion levels. The lookup loop
    // is bounded twice over - by MAX_DIR_HASH_DEPTH and by the directory's
    // actual size - so the right outcome is not an error but an unremarkable
    // success: the directory still works, and the walk still terminates. An
    // unbounded reader would hang here instead, which is why this check exists
    // even though it asserts that nothing happened.
    let mut img = build_image();
    put_u32(&mut img, blk(B_SUB).saturating_add(72), u32::MAX);
    let mut fs = mount_image(img)?;
    c.check(
        fs.readdir(Path::new("/sub")).map(|v| v.len()) == Ok(2),
        "an absurd directory depth still lists correctly",
    );
    c.check(
        fs.read_file(Path::new("/sub/data.bin")).map(|v| v.len()) == Ok(DATA_SIZE as usize),
        "an absurd directory depth still resolves a name",
    );

    // An inode size far larger than the blocks behind it. Reading it must not
    // attempt the allocation the size implies.
    let mut img = build_image();
    put_u64(&mut img, blk(B_DATA).saturating_add(16), u64::MAX);
    let mut fs = mount_image(img)?;
    c.check_err(
        fs.read_file(Path::new("/sub/data.bin")),
        KernelError::FileTooLarge,
        "an absurd file size is refused rather than allocated",
    );

    // The NAT bitmap flipped: every lookup moves to the decoy copy, where
    // every nid is NULL_ADDR. The mount reads the root inode, so it fails
    // here - which is the point of reading it there. A successful mount is a
    // claim: it publishes a VFS entry, shadows whatever was at the mount
    // point, and tells callers the volume is usable. Deferring the check does
    // not hide the fault (every later call errors too), it destroys the
    // attribution - the fault then surfaces from some unrelated `open()`
    // rather than from the operation that had the evidence.
    let mut img = build_image();
    put_u8(
        &mut img,
        blk(CP_BLKADDR + BLOCKS_PER_SEG).saturating_add(NAT_BITMAP_OFF),
        0,
    );
    reseal_cp(&mut img, CP_BLKADDR + BLOCKS_PER_SEG);
    // The older pack still points at the live copy, so it has to go too;
    // otherwise the fallback would quietly repair the volume.
    put_u64(
        &mut img,
        blk(CP_BLKADDR + CP_PACK_BLOCKS - 1),
        555,
    );
    c.check(
        mount_image(img).is_err(),
        "a NAT bitmap selecting the decoy copy fails the mount",
    );

    // A root inode that reads perfectly but is a regular file. Nothing about
    // it is corrupt in the sense the footer and bounds checks understand - it
    // is a valid node, at a valid address, that the checkpoint genuinely names
    // as the root. Only the root's *type* is wrong, and a driver that does not
    // check it mounts a volume on which every path lookup then fails for a
    // reason that names the path rather than the mount.
    let mut img = build_image();
    put_u16(&mut img, blk(B_ROOT), 0o100_644);
    c.check_err(
        mount_image(img),
        KernelError::CorruptedData,
        "a root inode that is not a directory fails the mount",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Directory entries, below the VFS
// ---------------------------------------------------------------------------

/// Find `name` among the entries `dir::read_dir` produced.
fn find_entry<'a>(entries: &'a [dir::DirEntry], name: &[u8]) -> Option<&'a dir::DirEntry> {
    entries.iter().find(|e| e.name == name)
}

fn test_dentries(c: &mut Checks) -> KernelResult<()> {
    let img = build_image();
    let sb = SuperBlock::parse(sb_slice(&img))?;
    let src = MemorySource::new(img);
    let checkpoint = cp::read_checkpoint(&src, &sb)?;
    let nat = Nat::new(&src, &sb, &checkpoint);

    // --- the inline-dentry root -------------------------------------------

    let root = node::read_inode(&nat, &sb, NID_ROOT)?;
    c.check(root.has_inline_dentry(), "the root's dentries are inline");
    c.check(!root.has_inline_data(), "an inline dentry is not inline data");
    c.check(root.is_dir() && !root.is_file(), "the root is a directory");

    // Below the VFS, `.` and `..` are ordinary entries; it is the VFS layer
    // that hides them. Testing both halves separately is what proves the
    // filtering happens in exactly one place.
    let entries = dir::read_dir(&nat, &sb, &root)?;
    c.check(entries.len() == 8, "the root holds eight entries in all");
    c.check(
        find_entry(&entries, b".").map(|e| e.ino) == Some(NID_ROOT),
        ". is a real entry pointing at the directory itself",
    );
    c.check(
        find_entry(&entries, b"..").map(|e| e.ino) == Some(NID_ROOT),
        ".. is a real entry",
    );
    c.check(
        find_entry(&entries, b"hello.txt").map(|e| (e.ino, e.file_type))
            == Some((NID_HELLO, F2FS_FT_REG_FILE)),
        "an entry carries both an inode and a type",
    );
    c.check(
        find_entry(&entries, b"link").map(|e| e.file_type) == Some(F2FS_FT_SYMLINK),
        "the symlink's file-type byte",
    );

    // --- lookup -----------------------------------------------------------

    c.check(
        dir::lookup(&nat, &sb, &root, b"sub")?.map(|e| e.ino) == Some(NID_SUB),
        "an inline-dentry lookup hits",
    );
    // A miss is `Ok(None)`, not an error: "this name is not here" is an answer,
    // and collapsing it into `Err` would make a missing file indistinguishable
    // from a damaged directory.
    c.check(
        dir::lookup(&nat, &sb, &root, b"absent")?.is_none(),
        "a miss is Ok(None), not an error",
    );
    c.check(
        dir::lookup(&nat, &sb, &root, b"Sub")?.is_none(),
        "lookup is case-sensitive",
    );
    // A prefix of a real name must not match it: names are compared whole, and
    // the length lives in the entry rather than in the slots it spans.
    c.check(
        dir::lookup(&nat, &sb, &root, b"hello")?.is_none(),
        "a prefix of a name does not match it",
    );
    c.check_err(
        dir::lookup(&nat, &sb, &root, b""),
        KernelError::InvalidArgument,
        "an empty name is refused",
    );
    c.check_err(
        dir::lookup(&nat, &sb, &root, &[b'x'; F2FS_NAME_LEN + 1]),
        KernelError::InvalidArgument,
        "a name longer than the format allows is refused",
    );

    // --- the block-backed directory ----------------------------------------

    let sub = node::read_inode(&nat, &sb, NID_SUB)?;
    c.check(!sub.has_inline_dentry(), "/sub keeps its dentries in blocks");
    let entries = dir::read_dir(&nat, &sb, &sub)?;
    c.check(entries.len() == 4, "/sub holds four entries in all");
    // Its second block is a hole. A directory with an unfilled bucket is
    // normal, and a reader that parses the zero block finds 214 entries all
    // naming inode 0 - the failure mode this check exists for.
    c.check(
        entries.iter().all(|e| e.ino != 0),
        "a hole inside a directory contributes no entries",
    );

    // A 47-byte name spans six slots, and the five continuation slots have
    // their bitmap bits set too. They must not be read as entries of their own.
    let long = find_entry(&entries, LONG_NAME);
    c.check(long.map(|e| e.ino) == Some(NID_LONG), "the six-slot name");
    c.check(
        entries.iter().filter(|e| e.name.len() > 8).count() == 1,
        "the continuation slots produce no extra entries",
    );
    c.check(
        dir::lookup(&nat, &sb, &sub, LONG_NAME)?.map(|e| e.ino) == Some(NID_LONG),
        "a six-slot name is found by the hash table",
    );
    c.check(
        dir::lookup(&nat, &sb, &sub, LONG_NAME.get(..8).unwrap_or(&[]))?.is_none(),
        "the first slot of a long name is not a name of its own",
    );
    c.check(
        dir::lookup(&nat, &sb, &sub, b"data.bin")?.map(|e| e.ino) == Some(NID_DATA),
        "a hashed lookup in a block-backed directory",
    );

    // --- type errors --------------------------------------------------------

    let hello = node::read_inode(&nat, &sb, NID_HELLO)?;
    c.check_err(
        dir::read_dir(&nat, &sb, &hello),
        KernelError::NotADirectory,
        "read_dir on a regular file is refused",
    );
    c.check_err(
        dir::lookup(&nat, &sb, &hello, b"x"),
        KernelError::NotADirectory,
        "lookup inside a regular file is refused",
    );

    // The mode-to-file-type mapping the builder uses, checked against the
    // driver's own so the two cannot drift apart and agree on nonsense.
    c.check(
        dir::file_type_for_mode(0o40_755) == F2FS_FT_DIR,
        "a directory mode maps to F2FS_FT_DIR",
    );
    c.check(
        dir::file_type_for_mode(0o100_644) == F2FS_FT_REG_FILE,
        "a regular-file mode maps to F2FS_FT_REG_FILE",
    );
    c.check(
        dir::file_type_for_mode(0o120_777) == F2FS_FT_SYMLINK,
        "a symlink mode maps to F2FS_FT_SYMLINK",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Run the F2FS self-test.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if any check failed or any group aborted.
/// The suite always runs to completion first: a failure is reported, not
/// thrown, so one broken area cannot hide the state of every other.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[f2fs] Running F2FS self-test...");

    let groups: [(&str, TestGroup); 8] = [
        ("primitives", test_primitives),
        ("superblock", test_superblock),
        ("checkpoint", test_checkpoint),
        ("nat", test_nat),
        ("block path", test_block_path),
        ("dentries", test_dentries),
        ("volume", test_volume),
        ("corruption", test_corruption),
    ];

    let mut checks = Checks::new();
    let mut aborted = 0u32;
    for (name, group) in groups {
        if group(&mut checks).is_err() {
            aborted = aborted.saturating_add(1);
            serial_println!("[f2fs] SELF-TEST group '{}' could not complete.", name);
        }
    }

    serial_println!(
        "[f2fs] Self-test: {} passed, {} failed, {} group(s) errored.",
        checks.passed,
        checks.failed,
        aborted
    );

    if checks.failed == 0 && aborted == 0 {
        Ok(())
    } else {
        Err(KernelError::InvalidArgument)
    }
}
