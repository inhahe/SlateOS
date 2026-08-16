//! The leaf items a read-only driver actually reads.
//!
//! Btrfs defines dozens of item types; a driver that only reads files needs
//! four of them — the root item (to find a subvolume's tree), the inode item
//! (metadata), the directory item (names), and the file extent item (data).
//! Everything else in the tree is the allocator's bookkeeping, and reading it
//! would tell us nothing we cannot get from the FS tree directly.
//!
//! # Names are bytes
//!
//! A Btrfs name is an arbitrary byte string with only `/` and NUL excluded —
//! the same rule this OS uses — so nothing here decodes to UTF-8. That is not
//! pedantry: a lossy decode would map two distinct on-disk names onto one
//! path, and the file the caller then opened would be whichever of them the
//! lookup happened to reach first.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::raw::{Key, read_u8, read_u16, read_u32, read_u64};

/// On-disk length of a `btrfs_inode_item`.
pub const INODE_ITEM_LEN: usize = 160;
/// On-disk length of a `btrfs_dir_item` header, before name and data.
pub const DIR_ITEM_HEADER_LEN: usize = 30;
/// On-disk length of a `btrfs_file_extent_item` describing a real extent.
pub const FILE_EXTENT_REG_LEN: usize = 53;
/// Offset within a `btrfs_file_extent_item` at which inline data begins.
pub const FILE_EXTENT_INLINE_DATA_OFF: usize = 21;

// Directory entry types (`BTRFS_FT_*`).

/// Regular file.
pub const FT_REG_FILE: u8 = 1;
/// Directory.
pub const FT_DIR: u8 = 2;
/// Symbolic link.
pub const FT_SYMLINK: u8 = 7;
/// Extended attribute (stored as a dir item, but never a directory entry).
pub const FT_XATTR: u8 = 8;

// File extent kinds (the `type` byte of a `btrfs_file_extent_item`).

/// Data stored inline in the item itself.
pub const FILE_EXTENT_INLINE: u8 = 0;
/// Data stored in an extent elsewhere on the volume.
pub const FILE_EXTENT_REG: u8 = 1;
/// Space reserved but never written; reads as zeroes.
pub const FILE_EXTENT_PREALLOC: u8 = 2;

/// The Btrfs name hash, which is the `offset` of a `DIR_ITEM` key.
///
/// This is CRC32C seeded with `~1` and — the part that is easy to get wrong —
/// **without the final inversion**. Btrfs calls Linux's `crc32c()`, which
/// applies neither a pre- nor a post-inversion itself; both are the caller's
/// business, and btrfs supplies only the odd seed. Using the fully-conditioned
/// CRC here (the one that checksums tree blocks) would produce a hash that is
/// wrong for every name, so directory lookup would find nothing while the
/// checksums all still verified — a failure that looks like an empty
/// filesystem rather than like a bug in a hash.
pub fn name_hash(name: &[u8]) -> u64 {
    u64::from(crate::crypto::crc32c_raw(!1u32, name))
}

/// A `btrfs_inode_item`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InodeItem {
    /// Logical size of the file's contents in bytes.
    pub size: u64,
    /// Bytes actually allocated.
    pub nbytes: u64,
    /// Hard-link count.
    pub nlink: u32,
    /// Owning user id.
    pub uid: u32,
    /// Owning group id.
    pub gid: u32,
    /// POSIX mode, including the file-type bits.
    pub mode: u32,
    /// Device number for a device node.
    pub rdev: u64,
    /// Access time, nanoseconds since the epoch.
    pub atime_ns: u64,
    /// Inode-change time.
    pub ctime_ns: u64,
    /// Modification time.
    pub mtime_ns: u64,
    /// Creation time (Btrfs records this; most Unix filesystems do not).
    pub otime_ns: u64,
}

/// Convert a `btrfs_timespec` at `off` into nanoseconds since the epoch.
///
/// Saturating rather than wrapping: a corrupt or absurd timestamp should
/// clamp to the end of the representable range, not wrap around to 1970 and
/// present a file from the future as ancient.
fn timespec_ns(buf: &[u8], off: usize) -> KernelResult<u64> {
    let secs = read_u64(buf, off)?;
    let nsec = read_u32(buf, off.checked_add(8).ok_or(KernelError::InvalidArgument)?)?;
    Ok(secs
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(nsec)))
}

impl InodeItem {
    /// Parse an inode item body.
    pub fn parse(buf: &[u8]) -> KernelResult<Self> {
        if buf.len() < INODE_ITEM_LEN {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            size: read_u64(buf, 16)?,
            nbytes: read_u64(buf, 24)?,
            nlink: read_u32(buf, 40)?,
            uid: read_u32(buf, 44)?,
            gid: read_u32(buf, 48)?,
            mode: read_u32(buf, 52)?,
            rdev: read_u64(buf, 56)?,
            atime_ns: timespec_ns(buf, 112)?,
            ctime_ns: timespec_ns(buf, 124)?,
            mtime_ns: timespec_ns(buf, 136)?,
            otime_ns: timespec_ns(buf, 148)?,
        })
    }

    /// Whether the mode's type bits say directory.
    pub fn is_dir(&self) -> bool {
        self.mode & 0o170_000 == 0o040_000
    }

    /// Whether the mode's type bits say symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.mode & 0o170_000 == 0o120_000
    }
}

/// One directory entry, as stored in a `DIR_ITEM` or `DIR_INDEX` item.
#[derive(Debug, Clone)]
pub struct DirItem {
    /// Key of the thing named: an `INODE_ITEM` key for a file in this tree,
    /// or a `ROOT_ITEM` key for a nested subvolume.
    pub location: Key,
    /// The name, as raw bytes.
    pub name: Vec<u8>,
    /// `BTRFS_FT_*` type byte.
    pub ftype: u8,
}

/// Parse every `btrfs_dir_item` packed into one leaf item.
///
/// A single `DIR_ITEM` key can hold more than one entry, because the key's
/// `offset` is a *hash* of the name and two names in the same directory may
/// collide. The entries are then concatenated in the item body and told apart
/// by their names — which is why a lookup must compare the name even after
/// the key matched exactly, and why this returns a list rather than one entry.
pub fn parse_dir_items(buf: &[u8]) -> KernelResult<Vec<DirItem>> {
    let mut out = Vec::new();
    let mut off = 0usize;

    while off < buf.len() {
        let location = Key::parse(buf, off)?;
        let data_len = read_u16(
            buf,
            off.checked_add(25).ok_or(KernelError::InvalidArgument)?,
        )?;
        let name_len = read_u16(
            buf,
            off.checked_add(27).ok_or(KernelError::InvalidArgument)?,
        )?;
        let ftype = read_u8(
            buf,
            off.checked_add(29).ok_or(KernelError::InvalidArgument)?,
        )?;

        let name_start = off
            .checked_add(DIR_ITEM_HEADER_LEN)
            .ok_or(KernelError::InvalidArgument)?;
        let name_end = name_start
            .checked_add(usize::from(name_len))
            .ok_or(KernelError::InvalidArgument)?;
        let name = buf
            .get(name_start..name_end)
            .ok_or(KernelError::InvalidArgument)?
            .to_vec();

        out.push(DirItem {
            location,
            name,
            ftype,
        });

        off = name_end
            .checked_add(usize::from(data_len))
            .ok_or(KernelError::InvalidArgument)?;
    }

    Ok(out)
}

/// A `btrfs_file_extent_item`: where one range of a file's bytes lives.
#[derive(Debug, Clone)]
pub enum FileExtent {
    /// Bytes stored directly in the item.
    Inline {
        /// The uncompressed length the file sees.
        ram_bytes: u64,
        /// The bytes themselves.
        data: Vec<u8>,
    },
    /// Bytes stored in an extent at a logical address.
    Regular {
        /// Logical address of the extent, or 0 for a hole.
        disk_bytenr: u64,
        /// Offset of this file range within that extent — non-zero when a
        /// later overwrite split one extent into several file ranges that
        /// still share it.
        offset: u64,
        /// Length of this file range.
        num_bytes: u64,
        /// True for `PREALLOC`: space reserved and never written, which must
        /// read as zeroes rather than as whatever the allocator left there.
        prealloc: bool,
    },
}

/// Parse a `btrfs_file_extent_item` body.
///
/// Compression and encryption are refused rather than ignored. An
/// encoded extent returned as-is would be handed to the caller as file
/// contents, and the caller has no way to tell that what it received is
/// compressed — so the read would "succeed" and the data would be wrong.
pub fn parse_file_extent(buf: &[u8]) -> KernelResult<FileExtent> {
    let ram_bytes = read_u64(buf, 8)?;
    let compression = read_u8(buf, 16)?;
    let encryption = read_u8(buf, 17)?;
    let other_encoding = read_u16(buf, 18)?;
    let kind = read_u8(buf, 20)?;

    if compression != 0 || encryption != 0 || other_encoding != 0 {
        return Err(KernelError::NotSupported);
    }

    match kind {
        FILE_EXTENT_INLINE => {
            let data = buf
                .get(FILE_EXTENT_INLINE_DATA_OFF..)
                .ok_or(KernelError::InvalidArgument)?
                .to_vec();
            Ok(FileExtent::Inline { ram_bytes, data })
        }
        FILE_EXTENT_REG | FILE_EXTENT_PREALLOC => {
            if buf.len() < FILE_EXTENT_REG_LEN {
                return Err(KernelError::InvalidArgument);
            }
            Ok(FileExtent::Regular {
                disk_bytenr: read_u64(buf, 21)?,
                offset: read_u64(buf, 37)?,
                num_bytes: read_u64(buf, 45)?,
                prealloc: kind == FILE_EXTENT_PREALLOC,
            })
        }
        _ => Err(KernelError::InvalidArgument),
    }
}

/// The parts of a `btrfs_root_item` needed to open the tree it names.
#[derive(Debug, Clone, Copy)]
pub struct RootItem {
    /// Logical address of the tree's root block.
    pub bytenr: u64,
    /// Generation the root block was written in.
    pub generation: u64,
    /// Height of the tree.
    pub level: u8,
    /// Inode number of this subvolume's root directory.
    pub root_dirid: u64,
}

impl RootItem {
    /// Parse a root item body.
    ///
    /// Only the first 239 bytes are read. Btrfs extended this item with a
    /// second copy of the generation and four UUIDs, and older filesystems
    /// have the shorter form — requiring the longer one would refuse to mount
    /// volumes that are perfectly readable.
    pub fn parse(buf: &[u8]) -> KernelResult<Self> {
        if buf.len() < 239 {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self {
            generation: read_u64(buf, 160)?,
            root_dirid: read_u64(buf, 168)?,
            bytenr: read_u64(buf, 176)?,
            level: read_u8(buf, 238)?,
        })
    }
}
