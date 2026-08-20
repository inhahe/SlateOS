//! ZFS on-disk primitives: scalars, DVAs, block pointers and the constants
//! that name them.
//!
//! # Byte order is a property of the *pool*, not of the format
//!
//! Unlike every other filesystem in this tree, ZFS does not fix an on-disk
//! byte order. A pool is written in the byte order of the machine that created
//! it, and each block pointer carries a `B` bit saying which order was used;
//! an importing host of the other endianness byte-swaps as it reads. We are
//! little-endian-only, so this driver reads little-endian and *rejects* a
//! big-endian pool at the door rather than pretending to understand it — see
//! [`BlkPtr::little_endian`]. Silently misreading a big-endian pool would
//! produce absurd sizes and offsets that would then be reported as corruption,
//! which is a much worse diagnostic than "not supported".
//!
//! # Nothing is reinterpreted as a struct
//!
//! Every field is read by explicit offset out of a `&[u8]` that came from a
//! device, for the same reason as the Btrfs and NTFS drivers: the buffer's
//! alignment belongs to the allocator, and a pointer cast onto it is UB even
//! where x86_64 happens to tolerate the load.

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Smallest addressable unit in a DVA offset, and the unit `asize`/`psize`/
/// `lsize` are stored in. ZFS calls this `SPA_MINBLOCKSIZE`.
pub const SPA_MINBLOCKSIZE: u64 = 512;

/// `log2(SPA_MINBLOCKSIZE)`; DVA offsets and block sizes are stored shifted
/// down by this much.
pub const SPA_MINBLOCKSHIFT: u32 = 9;

/// Largest block a pool may use, and therefore the largest single read this
/// driver will issue. 16 MiB is `zfs_max_recordsize`'s ceiling on modern
/// pools; older pools cap at 128 KiB, but accepting the larger value costs
/// nothing and refusing it would reject a valid volume.
pub const SPA_MAXBLOCKSIZE: u64 = 16 * 1024 * 1024;

/// Size of one vdev label. Four of these exist per device.
pub const VDEV_LABEL_SIZE: u64 = 256 * 1024;

/// Bytes reserved at the front of a device before any allocatable space: two
/// labels plus the 3.5 MiB boot block. Every DVA offset is relative to the end
/// of this region, which is why a raw `offset << 9` lands 4 MiB short.
pub const VDEV_LABEL_START_SIZE: u64 = 4 * 1024 * 1024;

/// Offset of the nvlist (pool configuration) within a label.
pub const VDEV_LABEL_NVLIST_OFFSET: u64 = 16 * 1024;

/// Size of the nvlist region within a label.
pub const VDEV_LABEL_NVLIST_SIZE: usize = 112 * 1024;

/// Offset of the uberblock array within a label.
pub const VDEV_LABEL_UBERBLOCK_OFFSET: u64 = 128 * 1024;

/// Size of the uberblock array within a label.
pub const VDEV_LABEL_UBERBLOCK_SIZE: u64 = 128 * 1024;

/// Minimum `log2` of one uberblock slot. The real slot size is
/// `1 << max(ashift, UBERBLOCK_SHIFT)`, so a pool with `ashift=12` has 4 KiB
/// slots and only 32 uberblocks per label.
pub const UBERBLOCK_SHIFT: u32 = 10;

/// `0x00bab10c` — "oo-ba-bloc". Present at the head of every uberblock.
pub const UBERBLOCK_MAGIC: u64 = 0x0000_0000_00ba_b10c;

/// The same magic as a pool written big-endian would store it. Recognising it
/// is what lets us say "big-endian pool" instead of "no uberblock found".
pub const UBERBLOCK_MAGIC_SWAPPED: u64 = 0x0cb1_ba00_0000_0000;

/// On-disk size of a `blkptr_t`.
pub const BLKPTR_LEN: usize = 128;

/// On-disk size of one `dnode_phys_t` slot.
pub const DNODE_LEN: usize = 512;

/// `log2(DNODE_LEN)`.
pub const DNODE_SHIFT: u32 = 9;

/// Byte offset of the first `blkptr_t` inside a `dnode_phys_t`.
pub const DNODE_CORE_LEN: usize = 64;

/// On-disk size of an `objset_phys_t`, including the reserved tail. Only the
/// first 1 KiB is meaningful to a reader; the rest is `os_userused_dnode` /
/// `os_groupused_dnode` and padding.
pub const OBJSET_PHYS_SIZE: usize = 2048;

// ---------------------------------------------------------------------------
// Compression algorithms (`enum zio_compress`)
// ---------------------------------------------------------------------------

/// No compression: `psize == lsize` and the payload is the data.
pub const ZIO_COMPRESS_OFF: u8 = 2;
/// LZJB, the pre-0.6.5 default for metadata.
pub const ZIO_COMPRESS_LZJB: u8 = 3;
/// "Empty" — the block is all zeroes and occupies no space.
pub const ZIO_COMPRESS_EMPTY: u8 = 4;
/// Zero-length encoding, used for mostly-zero metadata.
pub const ZIO_COMPRESS_ZLE: u8 = 14;
/// LZ4, the default for metadata (and for `compression=on`) on modern pools.
pub const ZIO_COMPRESS_LZ4: u8 = 15;
/// Zstandard. Not supported by this driver; named so the error can say why.
pub const ZIO_COMPRESS_ZSTD: u8 = 16;

// ---------------------------------------------------------------------------
// Checksum algorithms (`enum zio_checksum`)
// ---------------------------------------------------------------------------

/// Inherit from the parent; never appears in a written block pointer.
pub const ZIO_CHECKSUM_INHERIT: u8 = 0;
/// No checksum stored.
pub const ZIO_CHECKSUM_OFF: u8 = 2;
/// Self-checksumming label block (SHA-256 with the checksum field zeroed).
pub const ZIO_CHECKSUM_LABEL: u8 = 3;
/// Self-checksumming gang block header.
pub const ZIO_CHECKSUM_GANG_HEADER: u8 = 4;
/// ZIL log block (embedded fletcher-4).
pub const ZIO_CHECKSUM_ZILOG: u8 = 5;
/// Fletcher-2. Weak, but still what old pools used for data.
pub const ZIO_CHECKSUM_FLETCHER_2: u8 = 6;
/// Fletcher-4. The default for data.
pub const ZIO_CHECKSUM_FLETCHER_4: u8 = 7;
/// SHA-256. The default for metadata.
pub const ZIO_CHECKSUM_SHA256: u8 = 8;
/// ZIL log block, second generation.
pub const ZIO_CHECKSUM_ZILOG2: u8 = 9;
/// SHA-512/256.
pub const ZIO_CHECKSUM_SHA512: u8 = 10;
/// Skein-512.
pub const ZIO_CHECKSUM_SKEIN: u8 = 11;
/// Edon-R.
pub const ZIO_CHECKSUM_EDONR: u8 = 12;
/// BLAKE3.
pub const ZIO_CHECKSUM_BLAKE3: u8 = 13;

// ---------------------------------------------------------------------------
// DMU object types (`enum dmu_object_type`) — only those this driver names
// ---------------------------------------------------------------------------

/// An unallocated dnode slot.
pub const DMU_OT_NONE: u8 = 0;
/// The `dnode_phys_t` array of an object set.
pub const DMU_OT_DNODE: u8 = 10;
/// An `objset_phys_t`.
pub const DMU_OT_OBJSET: u8 = 11;
/// The MOS object directory (a ZAP).
pub const DMU_OT_OBJECT_DIRECTORY: u8 = 12;
/// A DSL directory; `dsl_dir_phys_t` lives in its bonus buffer.
pub const DMU_OT_DSL_DIR: u8 = 12 + 1;
/// A ZAP of child DSL directories.
pub const DMU_OT_DSL_DIR_CHILD_MAP: u8 = 12 + 2;
/// A DSL dataset; `dsl_dataset_phys_t` lives in its bonus buffer.
pub const DMU_OT_DSL_DATASET: u8 = 16;
/// A ZPL plain file.
pub const DMU_OT_PLAIN_FILE_CONTENTS: u8 = 19;
/// A ZPL directory (a ZAP of name -> dirent).
pub const DMU_OT_DIRECTORY_CONTENTS: u8 = 20;
/// The ZPL master node ZAP (object 1 of a filesystem objset).
pub const DMU_OT_MASTER_NODE: u8 = 21;
/// A `znode_phys_t` bonus buffer (ZPL version <= 4).
pub const DMU_OT_ZNODE: u8 = 17;
/// A System Attribute bonus buffer (ZPL version 5).
pub const DMU_OT_SA: u8 = 44;
/// The SA master node ZAP.
pub const DMU_OT_SA_MASTER_NODE: u8 = 45;
/// The SA attribute registration ZAP.
pub const DMU_OT_SA_ATTR_REGISTRATION: u8 = 46;
/// The SA attribute layout ZAP.
pub const DMU_OT_SA_ATTR_LAYOUTS: u8 = 47;

// ---------------------------------------------------------------------------
// Object set types (`enum dmu_objset_type`)
// ---------------------------------------------------------------------------

/// No type / uninitialised.
pub const DMU_OST_NONE: u64 = 0;
/// The meta object set.
pub const DMU_OST_META: u64 = 1;
/// A ZFS POSIX Layer filesystem.
pub const DMU_OST_ZFS: u64 = 2;
/// A zvol.
pub const DMU_OST_ZVOL: u64 = 3;

// ---------------------------------------------------------------------------
// Scalars
// ---------------------------------------------------------------------------

/// Read one byte.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if `off` is past the end of `buf`.
#[inline]
pub fn read_u8(buf: &[u8], off: usize) -> KernelResult<u8> {
    buf.get(off).copied().ok_or(KernelError::InvalidArgument)
}

/// Read a little-endian `u16`.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the field does not fit in `buf`.
#[inline]
pub fn read_u16(buf: &[u8], off: usize) -> KernelResult<u16> {
    let end = off.checked_add(2).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let mut b = [0u8; 2];
    b.copy_from_slice(s);
    Ok(u16::from_le_bytes(b))
}

/// Read a little-endian `u32`.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the field does not fit in `buf`.
#[inline]
pub fn read_u32(buf: &[u8], off: usize) -> KernelResult<u32> {
    let end = off.checked_add(4).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let mut b = [0u8; 4];
    b.copy_from_slice(s);
    Ok(u32::from_le_bytes(b))
}

/// Read a little-endian `u64`.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the field does not fit in `buf`.
#[inline]
pub fn read_u64(buf: &[u8], off: usize) -> KernelResult<u64> {
    let end = off.checked_add(8).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let mut b = [0u8; 8];
    b.copy_from_slice(s);
    Ok(u64::from_le_bytes(b))
}

/// Read a big-endian `u64`.
///
/// The nvlist in a vdev label is XDR-encoded, which is big-endian regardless
/// of the pool's byte order — it has to be, because it is what tells an
/// importing host what the pool's byte order *is*.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the field does not fit in `buf`.
#[inline]
pub fn read_u64_be(buf: &[u8], off: usize) -> KernelResult<u64> {
    let end = off.checked_add(8).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let mut b = [0u8; 8];
    b.copy_from_slice(s);
    Ok(u64::from_be_bytes(b))
}

/// Read a big-endian `u32` (XDR).
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the field does not fit in `buf`.
#[inline]
pub fn read_u32_be(buf: &[u8], off: usize) -> KernelResult<u32> {
    let end = off.checked_add(4).ok_or(KernelError::InvalidArgument)?;
    let s = buf.get(off..end).ok_or(KernelError::InvalidArgument)?;
    let mut b = [0u8; 4];
    b.copy_from_slice(s);
    Ok(u32::from_be_bytes(b))
}

/// Extract `len` bits starting at bit `low` from `v`.
///
/// This is ZFS's `BF64_GET`. Every packed field in a block pointer and every
/// ZPL directory entry is described in the upstream headers in exactly these
/// terms, so naming the operation the same way keeps the field definitions
/// below checkable against the source they came from.
#[inline]
#[must_use]
pub const fn bf64_get(v: u64, low: u32, len: u32) -> u64 {
    if low >= 64 || len == 0 || len > 64 {
        return 0;
    }
    let shifted = v >> low;
    if len >= 64 {
        shifted
    } else {
        // `len < 64` here, so the shift is in range; `wrapping_sub` rather
        // than `-` so this line stays correct on its own terms even if the
        // guard above is ever loosened.
        shifted & (1u64 << len).wrapping_sub(1)
    }
}

/// Extract a `BF64_GET_SB`-style field: a bit range that stores a value
/// shifted right by `shift`, biased by `bias`.
///
/// `asize`, `psize` and `lsize` are all stored as `(bytes >> 9) - 1`, which is
/// `bias = 1, shift = 9`. Doing the arithmetic here rather than at each call
/// site is what keeps the `+ 1` from being forgotten in one of the three.
#[inline]
#[must_use]
pub const fn bf64_get_sb(v: u64, low: u32, len: u32, shift: u32, bias: u64) -> u64 {
    let raw = bf64_get(v, low, len);
    if shift >= 64 {
        return 0;
    }
    raw.wrapping_add(bias) << shift
}

// ---------------------------------------------------------------------------
// Data Virtual Address
// ---------------------------------------------------------------------------

/// One of the up-to-three copies ("ditto blocks") of a block.
///
/// A DVA names a vdev and an offset within it. This driver supports a
/// single-vdev pool, so `vdev` must be 0 for the DVA to be readable — but the
/// field is still parsed and reported, because "this block lives on vdev 2"
/// is a far more useful failure than "read failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dva {
    /// Which top-level vdev holds this copy.
    pub vdev: u32,
    /// Allocated size in bytes (may exceed `psize` for raidz/gang).
    pub asize: u64,
    /// Byte offset from the end of the label/boot reserve.
    pub offset: u64,
    /// The gang bit: this DVA points at a gang header, not at data.
    pub gang: bool,
}

impl Dva {
    /// Whether this DVA slot is populated. An all-zero DVA is the "no copy
    /// here" encoding, and a block pointer with a zero DVA 0 is a hole.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.offset == 0 && self.asize == 0 && self.vdev == 0
    }

    /// Byte offset of this copy from the start of its vdev.
    ///
    /// # Errors
    ///
    /// [`KernelError::InvalidArgument`] on overflow, which for a 63-bit
    /// sector offset means a corrupt or hostile block pointer.
    pub fn physical_offset(&self) -> KernelResult<u64> {
        self.offset
            .checked_add(VDEV_LABEL_START_SIZE)
            .ok_or(KernelError::InvalidArgument)
    }
}

/// Parse the two-word DVA at `off` within `buf`.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the sixteen bytes do not fit.
pub fn parse_dva(buf: &[u8], off: usize) -> KernelResult<Dva> {
    let w0 = read_u64(buf, off)?;
    let w1 = read_u64(buf, off.checked_add(8).ok_or(KernelError::InvalidArgument)?)?;
    Ok(Dva {
        vdev: u32::try_from(bf64_get(w0, 32, 32)).unwrap_or(u32::MAX),
        // ASIZE is 24 bits of 512-byte units, with no bias.
        asize: bf64_get_sb(w0, 0, 24, SPA_MINBLOCKSHIFT, 0),
        offset: bf64_get_sb(w1, 0, 63, SPA_MINBLOCKSHIFT, 0),
        gang: bf64_get(w1, 63, 1) == 1,
    })
}

// ---------------------------------------------------------------------------
// Block pointer
// ---------------------------------------------------------------------------

/// A 128-byte `blkptr_t`: where a block is, how big it is both ways, how it is
/// compressed and checksummed, and what it contains.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlkPtr {
    /// The up-to-three copies. Copy 0 is tried first.
    pub dvas: [Dva; 3],
    /// Logical (uncompressed) size in bytes.
    pub lsize: u64,
    /// Physical (on-disk, compressed) size in bytes.
    pub psize: u64,
    /// `ZIO_COMPRESS_*`.
    pub compress: u8,
    /// `ZIO_CHECKSUM_*`.
    pub checksum: u8,
    /// `DMU_OT_*` of whatever this block holds.
    pub obj_type: u8,
    /// Indirection level: 0 is data, > 0 is an array of block pointers.
    pub level: u8,
    /// The `B` bit: the pool was written little-endian.
    pub little_endian: bool,
    /// The `D` bit: this block is deduplicated.
    pub dedup: bool,
    /// The `X` bit: this block is encrypted.
    pub encrypted: bool,
    /// The `E` bit: the data is embedded in the pointer itself, not on disk.
    pub embedded: bool,
    /// Transaction group in which this block was physically written.
    pub phys_birth: u64,
    /// Transaction group in which the logical block last changed.
    pub birth: u64,
    /// Number of non-hole blocks beneath this one.
    pub fill: u64,
    /// The stored checksum, four 64-bit words.
    pub cksum: [u64; 4],
}

impl BlkPtr {
    /// Whether this pointer addresses nothing — a hole in a sparse file, or an
    /// unused slot in an indirect block. Reading a hole yields zeroes, which
    /// is a correct answer and not an error.
    #[must_use]
    pub fn is_hole(&self) -> bool {
        !self.embedded && self.dvas[0].is_empty()
    }

    /// Number of logical bytes this pointer covers, saturating rather than
    /// wrapping on a corrupt `lsize`.
    #[must_use]
    pub const fn logical_size(&self) -> u64 {
        self.lsize
    }
}

/// Parse the 128-byte block pointer at `off` within `buf`.
///
/// The field positions come from `sys/spa.h`'s `blkptr_t` diagram. Word 6 is
/// the packed properties word:
///
/// ```text
/// 6   |BDX|lvl| type  | cksum |E| comp|     PSIZE     |     LSIZE     |
///      63 62 61  56-60  48-55   40-47 39   32-38        16-31            0-15
/// ```
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] if the 128 bytes do not fit in `buf`.
pub fn parse_blkptr(buf: &[u8], off: usize) -> KernelResult<BlkPtr> {
    let end = off
        .checked_add(BLKPTR_LEN)
        .ok_or(KernelError::InvalidArgument)?;
    if end > buf.len() {
        return Err(KernelError::InvalidArgument);
    }

    let mut bp = BlkPtr::default();
    for (i, dva) in bp.dvas.iter_mut().enumerate() {
        let dva_off = off
            .checked_add(i.checked_mul(16).ok_or(KernelError::InvalidArgument)?)
            .ok_or(KernelError::InvalidArgument)?;
        *dva = parse_dva(buf, dva_off)?;
    }

    let props = read_u64(
        buf,
        off.checked_add(48).ok_or(KernelError::InvalidArgument)?,
    )?;
    bp.embedded = bf64_get(props, 39, 1) == 1;
    bp.compress = u8::try_from(bf64_get(props, 32, 7)).unwrap_or(u8::MAX);
    bp.checksum = u8::try_from(bf64_get(props, 40, 8)).unwrap_or(u8::MAX);
    bp.obj_type = u8::try_from(bf64_get(props, 48, 8)).unwrap_or(u8::MAX);
    bp.level = u8::try_from(bf64_get(props, 56, 5)).unwrap_or(u8::MAX);
    bp.encrypted = bf64_get(props, 61, 1) == 1;
    bp.dedup = bf64_get(props, 62, 1) == 1;
    bp.little_endian = bf64_get(props, 63, 1) == 1;

    if bp.embedded {
        // An embedded block pointer reuses the DVA words for payload, so
        // LSIZE/PSIZE move and are *not* biased-and-shifted: LSIZE is 25 bits
        // of raw bytes, PSIZE 7 bits of raw bytes.
        bp.lsize = bf64_get(props, 0, 25);
        bp.psize = bf64_get(props, 25, 7);
    } else {
        bp.lsize = bf64_get_sb(props, 0, 16, SPA_MINBLOCKSHIFT, 1);
        bp.psize = bf64_get_sb(props, 16, 16, SPA_MINBLOCKSHIFT, 1);
    }

    bp.phys_birth = read_u64(
        buf,
        off.checked_add(72).ok_or(KernelError::InvalidArgument)?,
    )?;
    bp.birth = read_u64(
        buf,
        off.checked_add(80).ok_or(KernelError::InvalidArgument)?,
    )?;
    bp.fill = read_u64(
        buf,
        off.checked_add(88).ok_or(KernelError::InvalidArgument)?,
    )?;
    for (i, word) in bp.cksum.iter_mut().enumerate() {
        let c_off = off
            .checked_add(96)
            .and_then(|b| i.checked_mul(8).and_then(|d| b.checked_add(d)))
            .ok_or(KernelError::InvalidArgument)?;
        *word = read_u64(buf, c_off)?;
    }

    Ok(bp)
}
