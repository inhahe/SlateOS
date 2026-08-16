//! The Btrfs superblock: the one structure whose location is fixed.
//!
//! Everything else in Btrfs is found by following a logical address through
//! the chunk tree, and the chunk tree itself is found through the superblock.
//! So this is the bootstrap: 4096 bytes at a known byte offset containing the
//! geometry (`nodesize`, `sectorsize`), the roots of the two trees that matter
//! (`chunk_root` and `root`), and — embedded directly in it — enough of the
//! chunk tree's *contents* to map the chunk tree's own address. That last part
//! is `sys_chunk_array`, and it exists precisely because the alternative is a
//! circular dependency.
//!
//! # Why mirrors are read rather than just the primary
//!
//! Btrfs writes the superblock to up to four fixed offsets (64 KiB, 64 MiB,
//! 256 GiB, 1 PiB — whichever the device is large enough to hold). They are
//! not redundant copies of one write: a transaction commit updates them in
//! order, so a crash mid-commit can leave them at *different generations*,
//! and the newest one that still checksums correctly is the newest consistent
//! view of the filesystem. Reading only the primary would mean a torn commit
//! presents as a corrupt filesystem when a perfectly good older tree is
//! sitting 64 MiB away.
//!
//! We therefore read every mirror the device is big enough to contain, discard
//! the ones that fail magic or checksum, and keep the highest `generation`
//! among the survivors. A short device (the synthetic test image, or a small
//! partition) simply has fewer mirrors, which is why a read failure past the
//! end is treated as "this mirror does not exist" rather than as an error.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::raw::{CSUM_LEN, MAGIC, read_u8, read_u16, read_u32, read_u64};

/// Bytes of superblock read and checksummed, regardless of `sectorsize`.
pub const SUPER_INFO_SIZE: usize = 4096;

/// Byte offset of `sys_chunk_array` within the superblock.
///
/// Derived, not guessed: csum(32), fsid(16), 13 × u64, 5 × u32, 4 × u64,
/// u16, 3 × u8, `dev_item`(98), label(256), 2 × u64, `metadata_uuid`(16),
/// u64, reserved(27 × 8) — summing to 0x32B. The constant is what the code
/// uses; the arithmetic is here so a future reader can check it against the
/// format without a Linux tree to hand.
pub const SYS_CHUNK_ARRAY_OFFSET: usize = 0x32B;

/// Maximum bytes of chunk items embedded in the superblock.
pub const SYS_CHUNK_ARRAY_SIZE: usize = 2048;

/// `csum_type` value for CRC32C, the only algorithm this driver implements.
///
/// Btrfs also defines xxhash64 (1), SHA-256 (2) and BLAKE2b (3). A volume
/// using one of those is rejected outright rather than mounted with checksums
/// unverified — an unverified checksum is worse than no checksum, because it
/// reports success.
pub const CSUM_TYPE_CRC32C: u16 = 0;

// ---------------------------------------------------------------------------
// Incompat feature flags
// ---------------------------------------------------------------------------

/// Backrefs may be shared between extents (universal since 2010).
pub const FEATURE_INCOMPAT_MIXED_BACKREF: u64 = 1 << 0;
/// The default subvolume is set explicitly rather than being FS_TREE.
pub const FEATURE_INCOMPAT_DEFAULT_SUBVOL: u64 = 1 << 1;
/// Data and metadata share block groups (small filesystems).
pub const FEATURE_INCOMPAT_MIXED_GROUPS: u64 = 1 << 2;
/// Extents may be LZO-compressed.
pub const FEATURE_INCOMPAT_COMPRESS_LZO: u64 = 1 << 3;
/// Extents may be zstd-compressed.
pub const FEATURE_INCOMPAT_COMPRESS_ZSTD: u64 = 1 << 4;
/// Metadata nodes may be larger than one page.
pub const FEATURE_INCOMPAT_BIG_METADATA: u64 = 1 << 5;
/// `INODE_REF` items may use the extended form.
pub const FEATURE_INCOMPAT_EXTENDED_IREF: u64 = 1 << 6;
/// RAID5/RAID6 chunk profiles are present.
pub const FEATURE_INCOMPAT_RAID56: u64 = 1 << 7;
/// Metadata extent items omit the level/refs preamble.
pub const FEATURE_INCOMPAT_SKINNY_METADATA: u64 = 1 << 8;
/// File holes have no explicit `EXTENT_DATA` item.
pub const FEATURE_INCOMPAT_NO_HOLES: u64 = 1 << 9;
/// The filesystem UUID is carried in `metadata_uuid` instead of `fsid`.
pub const FEATURE_INCOMPAT_METADATA_UUID: u64 = 1 << 10;
/// RAID1 with 3 or 4 copies.
pub const FEATURE_INCOMPAT_RAID1C34: u64 = 1 << 11;
/// Zoned device layout.
pub const FEATURE_INCOMPAT_ZONED: u64 = 1 << 12;
/// The v2 extent tree format.
pub const FEATURE_INCOMPAT_EXTENT_TREE_V2: u64 = 1 << 13;

/// Incompat flags this read-only driver understands.
///
/// The rule for what belongs here is narrow and worth stating, because
/// "incompat" means the filesystem author asserted that a driver which does
/// not know the flag will *misread* the volume — not merely miss a feature.
/// So a flag goes in this mask only if either (a) it changes nothing about
/// how bytes are interpreted on the read path, or (b) the read path here
/// actually implements the change.
///
/// - `MIXED_BACKREF`, `DEFAULT_SUBVOL`, `BIG_METADATA` — allocation and
///   subvolume-selection concerns; the on-disk item formats we read are
///   unaffected.
/// - `EXTENDED_IREF` — affects `INODE_REF`, which the directory walk parses
///   defensively either way.
/// - `SKINNY_METADATA` — changes `EXTENT_ITEM` keys in the extent tree, which
///   a read-only driver never consults (it needs the FS tree, not the
///   allocator's view of it).
/// - `NO_HOLES` — a hole is the *absence* of an `EXTENT_DATA` item; the file
///   reader must zero-fill gaps, which it does unconditionally.
/// - `METADATA_UUID` — only changes which UUID field is authoritative, and
///   this driver does not gate on the UUID.
///
/// Everything else is refused. Compression is the notable omission: mounting
/// a compressed volume without decompressing would hand the caller ciphered
/// bytes that *look* like file contents, which is exactly the failure mode
/// that silently corrupts data instead of reporting an error.
pub const SUPPORTED_INCOMPAT: u64 = FEATURE_INCOMPAT_MIXED_BACKREF
    | FEATURE_INCOMPAT_DEFAULT_SUBVOL
    | FEATURE_INCOMPAT_BIG_METADATA
    | FEATURE_INCOMPAT_EXTENDED_IREF
    | FEATURE_INCOMPAT_SKINNY_METADATA
    | FEATURE_INCOMPAT_NO_HOLES
    | FEATURE_INCOMPAT_METADATA_UUID;

/// Byte offsets of the superblock mirrors, primary first.
///
/// `btrfs_sb_offset()` in the Linux tree: mirror 0 is 64 KiB, and mirror `n`
/// thereafter is `16 KiB << (12 * n)` — 64 MiB, 256 GiB, 1 PiB.
pub const SB_MIRROR_OFFSETS: [u64; 4] = [
    0x1_0000,           // 64 KiB
    0x400_0000,         // 64 MiB
    0x40_0000_0000,     // 256 GiB
    0x4_0000_0000_0000, // 1 PiB
];

/// A parsed and validated Btrfs superblock.
#[derive(Debug, Clone)]
pub struct Superblock {
    /// Filesystem UUID.
    pub fsid: [u8; 16],
    /// Physical byte offset this copy claims to live at.
    pub bytenr: u64,
    /// Transaction id of the commit that wrote this copy.
    pub generation: u64,
    /// Logical address of the root tree (the tree of tree roots).
    pub root: u64,
    /// Logical address of the chunk tree.
    pub chunk_root: u64,
    /// Logical address of the log tree; non-zero means an unclean shutdown.
    pub log_root: u64,
    /// Size of the filesystem in bytes.
    pub total_bytes: u64,
    /// Bytes currently allocated.
    pub bytes_used: u64,
    /// Objectid of the tree holding the default subvolume's root directory.
    pub root_dir_objectid: u64,
    /// How many devices this filesystem spans.
    ///
    /// Kept because it is the cheapest possible rejection of a multi-device
    /// volume: this driver is handed one [`SectorSource`], so a chunk whose
    /// stripes live on a second device is unreadable no matter how correct the
    /// chunk parser is, and finding that out at mount time gives a clear error
    /// instead of a puzzling one during a tree descent.
    pub num_devices: u64,
    /// Minimum addressable unit for data (typically 4096).
    pub sectorsize: u32,
    /// Size of a metadata tree block (typically 16384).
    pub nodesize: u32,
    /// Generation of the commit that wrote the chunk tree root.
    pub chunk_root_generation: u64,
    /// Feature flags that make the volume unreadable if unrecognised.
    pub incompat_flags: u64,
    /// Checksum algorithm; only [`CSUM_TYPE_CRC32C`] is accepted.
    pub csum_type: u16,
    /// Height of the root tree (0 = the root block is a leaf).
    pub root_level: u8,
    /// Height of the chunk tree.
    pub chunk_root_level: u8,
    /// The embedded chunk items that bootstrap the chunk tree, exactly
    /// `sys_chunk_array_size` bytes long.
    pub sys_chunk_array: Vec<u8>,
    /// Volume label, NUL-padded on disk and trimmed here.
    pub label: Vec<u8>,
}

impl Superblock {
    /// Parse and fully validate a 4096-byte superblock image.
    ///
    /// Validation is deliberately done here rather than by the caller, because
    /// every field below is used as a length or an address by code that has no
    /// way to re-check it: `nodesize` sizes a heap allocation, `chunk_root` is
    /// dereferenced through the chunk map, and `sys_chunk_array_size` bounds a
    /// slice. A superblock that reaches a caller has already been established
    /// to be self-consistent.
    pub fn parse(buf: &[u8]) -> KernelResult<Self> {
        if buf.len() < SUPER_INFO_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        if read_u64(buf, 0x40)? != MAGIC {
            return Err(KernelError::InvalidArgument);
        }

        let csum_type = read_u16(buf, 0xC4)?;
        if csum_type != CSUM_TYPE_CRC32C {
            return Err(KernelError::NotSupported);
        }
        verify_csum(buf)?;

        let incompat_flags = read_u64(buf, 0xBC)?;
        if incompat_flags & !SUPPORTED_INCOMPAT != 0 {
            return Err(KernelError::NotSupported);
        }

        let sectorsize = read_u32(buf, 0x90)?;
        let nodesize = read_u32(buf, 0x94)?;
        // Both must be powers of two, and `nodesize` bounds a per-node
        // allocation on every tree descent — an absurd value here is an OOM
        // on a hostile image, so it is capped at the 64 KiB the format allows.
        if !sectorsize.is_power_of_two() || !(512..=65536).contains(&sectorsize) {
            return Err(KernelError::InvalidArgument);
        }
        if !nodesize.is_power_of_two() || !(512..=65536).contains(&nodesize) {
            return Err(KernelError::InvalidArgument);
        }

        let sys_chunk_array_size = read_u32(buf, 0xA0)?;
        let sys_len = usize::try_from(sys_chunk_array_size)
            .ok()
            .filter(|n| *n <= SYS_CHUNK_ARRAY_SIZE)
            .ok_or(KernelError::InvalidArgument)?;
        let sys_end = SYS_CHUNK_ARRAY_OFFSET
            .checked_add(sys_len)
            .ok_or(KernelError::InvalidArgument)?;
        let sys_chunk_array = buf
            .get(SYS_CHUNK_ARRAY_OFFSET..sys_end)
            .ok_or(KernelError::InvalidArgument)?
            .to_vec();

        let mut fsid = [0u8; 16];
        let fsid_src = buf.get(0x20..0x30).ok_or(KernelError::InvalidArgument)?;
        fsid.copy_from_slice(fsid_src);

        let label_src = buf.get(0x12B..0x22B).ok_or(KernelError::InvalidArgument)?;
        let label_len = label_src.iter().position(|b| *b == 0).unwrap_or(0x100);
        let label = label_src
            .get(..label_len)
            .ok_or(KernelError::InternalError)?
            .to_vec();

        Ok(Self {
            fsid,
            bytenr: read_u64(buf, 0x30)?,
            generation: read_u64(buf, 0x48)?,
            root: read_u64(buf, 0x50)?,
            chunk_root: read_u64(buf, 0x58)?,
            log_root: read_u64(buf, 0x60)?,
            total_bytes: read_u64(buf, 0x70)?,
            bytes_used: read_u64(buf, 0x78)?,
            root_dir_objectid: read_u64(buf, 0x80)?,
            num_devices: read_u64(buf, 0x88)?,
            sectorsize,
            nodesize,
            chunk_root_generation: read_u64(buf, 0xA4)?,
            incompat_flags,
            csum_type,
            root_level: read_u8(buf, 0xC6)?,
            chunk_root_level: read_u8(buf, 0xC7)?,
            sys_chunk_array,
            label,
        })
    }

    /// `nodesize` as a `usize`, which is what every buffer allocation wants.
    pub fn nodesize_usize(&self) -> KernelResult<usize> {
        usize::try_from(self.nodesize).map_err(|_| KernelError::InvalidArgument)
    }
}

/// Verify the CRC32C over a superblock or tree block.
///
/// Btrfs checksums the block *excluding* its own 32-byte checksum field, and
/// stores the result in the low bytes of that field with the remainder zeroed.
/// Only the first four bytes are meaningful for CRC32C; the field is 32 bytes
/// because the same slot holds a SHA-256 or BLAKE2b digest on volumes using
/// those.
pub fn verify_csum(block: &[u8]) -> KernelResult<()> {
    let stored = read_u32(block, 0)?;
    let body = block.get(CSUM_LEN..).ok_or(KernelError::InvalidArgument)?;
    if crate::crypto::crc32c(body) == stored {
        Ok(())
    } else {
        Err(KernelError::IoError)
    }
}

/// Write the CRC32C of `block` into its checksum field.
///
/// Only used when synthesising an image (the self-test), but it lives beside
/// [`verify_csum`] on purpose: a test that computed the checksum its own way
/// would pass even if both sides were wrong in the same direction, which is
/// the classic way a checksum test proves nothing.
pub fn seal_csum(block: &mut [u8]) -> KernelResult<()> {
    let body = block.get(CSUM_LEN..).ok_or(KernelError::InvalidArgument)?;
    let csum = crate::crypto::crc32c(body).to_le_bytes();
    let field = block
        .get_mut(..CSUM_LEN)
        .ok_or(KernelError::InvalidArgument)?;
    field.fill(0);
    field
        .get_mut(..4)
        .ok_or(KernelError::InternalError)?
        .copy_from_slice(&csum);
    Ok(())
}

/// Read every superblock mirror the device can hold and return the newest
/// one that validates.
///
/// A mirror that cannot be read (past the end of a short device) or that
/// fails magic/checksum/feature validation is skipped silently — the point of
/// having four is that some of them are expected to be absent or stale. Only
/// if *none* survives is this a failure, and then the error is the one from
/// the primary, which is the copy whose absence a caller most wants explained.
pub fn read_superblock(src: &dyn SectorSource) -> KernelResult<Superblock> {
    let mut best: Option<Superblock> = None;
    let mut primary_err = KernelError::IoError;

    for (i, off) in SB_MIRROR_OFFSETS.iter().enumerate() {
        let parsed = read_bytes(src, *off, SUPER_INFO_SIZE).and_then(|buf| Superblock::parse(&buf));
        match parsed {
            Ok(sb) => {
                // A copy that disagrees about where it lives is not a
                // superblock we followed a pointer to — it is whatever else
                // happens to sit at that offset with a plausible magic.
                if sb.bytenr != *off {
                    continue;
                }
                if best.as_ref().is_none_or(|b| sb.generation > b.generation) {
                    best = Some(sb);
                }
            }
            Err(e) => {
                if i == 0 {
                    primary_err = e;
                }
            }
        }
    }

    best.ok_or(primary_err)
}
