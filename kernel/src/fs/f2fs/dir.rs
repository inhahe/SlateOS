//! F2FS directories: dentry blocks, inline dentries, lookup and readdir.
//!
//! A directory is an ordinary file whose contents are dentry blocks, plus a
//! hash table imposed on top of those blocks' *indices*. Each 4 KiB block is a
//! validity bitmap, a fixed array of 214 fixed-size entries, and a parallel
//! array of 214 eight-byte name slots. A name longer than eight bytes spills
//! into the following slots, and its entry describes all of them at once.
//!
//! # The bitmap is the only thing that says an entry exists
//!
//! Entries are not terminated, chained or ordered — the array is fixed and
//! every slot always has bytes in it. The bitmap is what distinguishes a live
//! entry from the remains of a deleted one, and it is also what marks the
//! *continuation* slots of a long name as occupied. So a reader that walks the
//! array without consulting the bitmap sees deleted files, and one that
//! consults it but does not skip a name's continuation slots re-reads the tail
//! of every long name as if it were an entry of its own.
//!
//! # Two ways to find a name
//!
//! `lookup` uses the hash table, which is what makes a large directory fast:
//! the name's hash picks a bucket, the bucket is a run of two or four blocks,
//! and only those are read. `readdir` ignores the hash entirely and walks
//! every block of the directory in order — which is also what makes the two
//! agree even when the hash is subtly wrong in one direction.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::node::{Inode, Nat, read_file_block};
use super::raw::{
    BLOCK_SIZE, DENTRY_BITMAP_SIZE, DENTRY_RESERVED_SIZE, DIR_ENTRY_LEN, F2FS_FT_BLKDEV,
    F2FS_FT_CHRDEV, F2FS_FT_DIR, F2FS_FT_FIFO, F2FS_FT_REG_FILE, F2FS_FT_SOCK, F2FS_FT_SYMLINK,
    F2FS_NAME_LEN, F2FS_SLOT_LEN, NR_DENTRY_IN_BLOCK, bucket_blocks, dentry_hash, dir_block_index,
    dir_buckets, read_u8, read_u16, read_u32, slots_for_name, test_bit,
};
use super::sb::SuperBlock;

/// One directory entry, as this driver hands it out.
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// The name, as raw bytes.
    ///
    /// Bytes, not `String`: an F2FS name is any byte sequence without `/` or
    /// NUL, and forcing UTF-8 on it would corrupt names this filesystem
    /// considers perfectly ordinary.
    pub name: Vec<u8>,
    /// The inode number the name refers to.
    pub ino: u32,
    /// The on-disk file-type byte.
    pub file_type: u8,
}

impl DirEntry {
    /// The name as a `String`, for the paths where the VFS demands one.
    pub fn name_string(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    /// Is this entry a directory?
    pub const fn is_dir(&self) -> bool {
        self.file_type == F2FS_FT_DIR
    }

    /// The POSIX mode type bits this entry's `file_type` corresponds to.
    ///
    /// Only a hint — the authoritative mode is in the inode. Used to fill in a
    /// `readdir` result without reading every child's inode block, which for a
    /// large directory would turn one sequential pass into thousands of
    /// scattered reads.
    pub const fn mode_bits(&self) -> u16 {
        match self.file_type {
            F2FS_FT_DIR => 0x4000,
            F2FS_FT_SYMLINK => 0xA000,
            F2FS_FT_CHRDEV => 0x2000,
            F2FS_FT_BLKDEV => 0x6000,
            F2FS_FT_FIFO => 0x1000,
            F2FS_FT_SOCK => 0xC000,
            _ => 0x8000,
        }
    }
}

/// The geometry of a dentry area, whether a whole block or an inline region.
///
/// Inline and block-backed directories have the same four-part layout —
/// bitmap, reserved padding, entry array, name array — at different sizes.
/// Describing it once and parameterising the sizes is what keeps the two from
/// drifting apart, which matters because the inline sizes are *derived* and
/// therefore easy to get wrong in only one of two copies.
#[derive(Debug, Clone, Copy)]
struct DentryLayout {
    /// Entries (and name slots) in the area.
    slots: usize,
    /// Byte offset of the entry array.
    entries_off: usize,
    /// Byte offset of the name array.
    names_off: usize,
}

impl DentryLayout {
    /// The layout of a full 4 KiB dentry block.
    const fn block() -> Self {
        Self {
            slots: NR_DENTRY_IN_BLOCK,
            entries_off: DENTRY_BITMAP_SIZE + DENTRY_RESERVED_SIZE,
            names_off: DENTRY_BITMAP_SIZE
                + DENTRY_RESERVED_SIZE
                + NR_DENTRY_IN_BLOCK * DIR_ENTRY_LEN,
        }
    }

    /// The layout of an inline dentry area of `size` bytes.
    ///
    /// The slot count is what `size` can hold once each slot's cost is counted
    /// as its entry, its name bytes, *and* its one bit of bitmap — hence the
    /// `* 8 ... + 1` shape, which is F2FS's own formula rearranged to avoid a
    /// division by a fraction.
    fn inline(size: usize) -> Self {
        let per_slot_bits = DIR_ENTRY_LEN
            .saturating_add(F2FS_SLOT_LEN)
            .saturating_mul(8)
            .saturating_add(1);
        // `per_slot_bits` is a constant expression that cannot be zero, but
        // `checked_div` says so to the compiler rather than to the reader.
        let slots = size
            .saturating_mul(8)
            .checked_div(per_slot_bits)
            .unwrap_or(0);
        let bitmap = slots.saturating_add(7) / 8;
        // Whatever the three arrays do not use is reserved padding, and it
        // sits between the bitmap and the entries exactly as in a full block.
        let used = bitmap
            .saturating_add(slots.saturating_mul(DIR_ENTRY_LEN))
            .saturating_add(slots.saturating_mul(F2FS_SLOT_LEN));
        let reserved = size.saturating_sub(used);
        let entries_off = bitmap.saturating_add(reserved);
        Self {
            slots,
            entries_off,
            names_off: entries_off.saturating_add(slots.saturating_mul(DIR_ENTRY_LEN)),
        }
    }
}

/// Walk the live entries of one dentry area, calling `f` for each.
///
/// `f` returning `Some` stops the walk and yields that value — which is how
/// `lookup` short-circuits without a separate traversal.
fn for_each_entry<T>(
    area: &[u8],
    layout: DentryLayout,
    mut f: impl FnMut(DirEntry) -> Option<T>,
) -> KernelResult<Option<T>> {
    let mut slot = 0usize;
    while slot < layout.slots {
        if !test_bit(area, slot) {
            slot = slot.saturating_add(1);
            continue;
        }

        let eoff = layout
            .entries_off
            .checked_add(
                slot.checked_mul(DIR_ENTRY_LEN)
                    .ok_or(KernelError::InternalError)?,
            )
            .ok_or(KernelError::InternalError)?;

        let ino = read_u32(area, eoff.saturating_add(4))?;
        let name_len = read_u16(area, eoff.saturating_add(8))? as usize;
        let file_type = read_u8(area, eoff.saturating_add(10))?;

        // A zero-length or over-long name means the entry array does not
        // describe what the bitmap claims. Advancing by one slot and carrying
        // on is the right recovery: it cannot loop, and it keeps the rest of
        // the block readable rather than discarding a directory over one bad
        // entry.
        if name_len == 0 || name_len > F2FS_NAME_LEN {
            slot = slot.saturating_add(1);
            continue;
        }

        let used = slots_for_name(name_len).max(1);
        let noff = layout
            .names_off
            .checked_add(
                slot.checked_mul(F2FS_SLOT_LEN)
                    .ok_or(KernelError::InternalError)?,
            )
            .ok_or(KernelError::InternalError)?;
        let nend = noff
            .checked_add(name_len)
            .ok_or(KernelError::InternalError)?;

        if let Some(name) = area.get(noff..nend) {
            if ino != 0 {
                let entry = DirEntry {
                    name: name.to_vec(),
                    ino,
                    file_type,
                };
                if let Some(v) = f(entry) {
                    return Ok(Some(v));
                }
            }
        }

        slot = slot.saturating_add(used);
    }
    Ok(None)
}

/// The inline dentry area of an inode, with its derived layout.
fn inline_area(inode: &Inode) -> KernelResult<(&[u8], DentryLayout)> {
    let area = inode.inline_area()?;
    let layout = DentryLayout::inline(area.len());
    Ok((area, layout))
}

/// How many blocks a directory's data occupies.
///
/// From `i_size`, rounded up — F2FS keeps a directory's size exact, and the
/// hash table's levels are laid out contiguously from block 0, so this is also
/// the end of the last level in use.
fn dir_blocks(inode: &Inode) -> u64 {
    let bs = u64::try_from(BLOCK_SIZE).unwrap_or(4096);
    inode
        .size
        .saturating_add(bs.saturating_sub(1))
        .checked_div(bs)
        .unwrap_or(0)
}

/// List every entry of `inode`.
///
/// Walks the directory's blocks in order rather than following the hash table.
/// That is what POSIX `readdir` wants — every entry exactly once — and it is
/// also robust to a hash table whose depth field disagrees with its contents.
pub fn read_dir(nat: &Nat<'_>, sb: &SuperBlock, inode: &Inode) -> KernelResult<Vec<DirEntry>> {
    if !inode.is_dir() {
        return Err(KernelError::NotADirectory);
    }

    let mut out = Vec::new();

    if inode.has_inline_dentry() {
        let (area, layout) = inline_area(inode)?;
        for_each_entry(area, layout, |e| {
            out.push(e);
            None::<()>
        })?;
        return Ok(out);
    }

    for b in 0..dir_blocks(inode) {
        let block = read_file_block(nat, sb, inode, b)?;
        // A hole inside a directory is a bucket that was never filled, not
        // corruption — the hash table's block indices are sparse by design.
        if block.iter().all(|&x| x == 0) {
            continue;
        }
        for_each_entry(&block, DentryLayout::block(), |e| {
            out.push(e);
            None::<()>
        })?;
    }

    Ok(out)
}

/// Find `name` in `inode`.
///
/// Uses the hash table: the name's hash selects a bucket at each level from 0
/// up to the directory's current depth, and only that bucket's two or four
/// blocks are read. A miss at one level continues to the next, because an
/// entry inserted before the directory grew a level is still in the shallower
/// one.
pub fn lookup(
    nat: &Nat<'_>,
    sb: &SuperBlock,
    inode: &Inode,
    name: &[u8],
) -> KernelResult<Option<DirEntry>> {
    if !inode.is_dir() {
        return Err(KernelError::NotADirectory);
    }
    if name.is_empty() || name.len() > F2FS_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    if inode.has_inline_dentry() {
        let (area, layout) = inline_area(inode)?;
        return for_each_entry(
            area,
            layout,
            |e| {
                if e.name == name { Some(e) } else { None }
            },
        );
    }

    let hash = dentry_hash(name);
    let total = dir_blocks(inode);

    // `current_depth` is what the inode claims; the loop is additionally
    // bounded by the directory's actual size, so a corrupt depth cannot make
    // this read thousands of blocks that are not there.
    let depth = inode.current_depth.min(super::raw::MAX_DIR_HASH_DEPTH);
    for level in 0..depth {
        let nbucket = dir_buckets(level, inode.dir_level);
        let nblock = bucket_blocks(level);
        let bucket = hash.checked_rem(nbucket).unwrap_or(0);
        let start = dir_block_index(level, inode.dir_level, bucket);

        for i in 0..nblock {
            let Some(idx) = start.checked_add(i) else {
                break;
            };
            if u64::from(idx) >= total {
                break;
            }
            let block = read_file_block(nat, sb, inode, u64::from(idx))?;
            let found = for_each_entry(&block, DentryLayout::block(), |e| {
                if e.name == name { Some(e) } else { None }
            })?;
            if found.is_some() {
                return Ok(found);
            }
        }
    }

    // A directory whose depth is zero still has its level-0 buckets; and a
    // lookup that missed every level is worth one honest linear sweep only if
    // the hash table could not have covered the whole directory. Rather than
    // guess, fall back to the same walk `read_dir` does: it is O(size) once,
    // on a path that is already about to report a miss, and it makes `lookup`
    // and `read_dir` incapable of disagreeing about what exists.
    for b in 0..total {
        let block = read_file_block(nat, sb, inode, b)?;
        let found = for_each_entry(&block, DentryLayout::block(), |e| {
            if e.name == name { Some(e) } else { None }
        })?;
        if found.is_some() {
            return Ok(found);
        }
    }

    Ok(None)
}

/// The file-type byte a directory entry should carry for `mode`.
///
/// Used by the self-test to build directories the same way `mkfs` would.
pub const fn file_type_for_mode(mode: u16) -> u8 {
    match mode & 0xF000 {
        0x4000 => F2FS_FT_DIR,
        0xA000 => F2FS_FT_SYMLINK,
        0x2000 => F2FS_FT_CHRDEV,
        0x6000 => F2FS_FT_BLKDEV,
        0x1000 => F2FS_FT_FIFO,
        0xC000 => F2FS_FT_SOCK,
        _ => F2FS_FT_REG_FILE,
    }
}
