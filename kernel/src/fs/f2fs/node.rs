//! Nodes: the NAT, node blocks, inodes, and file-offset → block resolution.
//!
//! F2FS separates *identity* from *location*. Everything that points at a node
//! — an inode's `i_nid[]`, an indirect node's slots, a directory entry's `ino`
//! — names it by **node id**, never by block address. The Node Address Table
//! translates a nid into the block the node currently occupies.
//!
//! That indirection is the whole reason F2FS can be log-structured without
//! wandering-tree updates. When a node is rewritten it lands somewhere new,
//! and only its one NAT entry changes; every pointer to it, at every depth,
//! still says the same nid and needs no update. The cost is that **no node can
//! be read without a NAT lookup first**, which is why every function here
//! takes the NAT along with the source.
//!
//! # The two NAT copies
//!
//! The NAT area is written twice over, and the checkpoint's NAT bitmap says,
//! per NAT block, which copy is current. This is not redundancy for repair —
//! it is how a NAT update becomes atomic: the new copy is written while the
//! old one is still the live one, and the bitmap bit flips as part of the
//! checkpoint. A reader that ignores the bitmap gets a NAT block that is
//! internally consistent and one checkpoint stale, so its lookups succeed and
//! return the previous location of every recently-written node.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::cp::Checkpoint;
use super::raw::{
    ADDRS_PER_BLOCK, BLOCK_SIZE, DEF_ADDRS_PER_INODE, DEF_INLINE_RESERVED_SIZE, F2FS_EXTRA_ATTR,
    F2FS_INLINE_DATA, F2FS_INLINE_DENTRY, F2FS_INLINE_XATTR, I_ADDR_OFF, I_NID_OFF,
    NAT_ENTRIES_PER_BLOCK, NAT_ENTRY_LEN, NEW_ADDR, NIDS_PER_BLOCK, NODE_FOOTER_OFF, NULL_ADDR,
    NULL_NID, block_to_offset, read_u16, read_u32, read_u64, test_bit,
};
use super::sb::{FEATURE_FLEXIBLE_INLINE_XATTR, SuperBlock};

// ---------------------------------------------------------------------------
// The NAT
// ---------------------------------------------------------------------------

/// Where a node currently lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NatEntry {
    /// Version counter; not meaningful to a reader, kept for diagnostics.
    pub version: u8,
    /// The inode this node belongs to (equal to `nid` for an inode itself).
    pub ino: u32,
    /// Block address of the node, or [`NULL_ADDR`] if it does not exist.
    pub block_addr: u32,
}

/// Resolves node ids to block addresses.
///
/// Holds the checkpoint's NAT bitmap and journal, and reads NAT blocks from
/// the device on demand. Kept separate from the filesystem object so the
/// self-test can exercise the address arithmetic — which is the part with the
/// non-obvious formula in it — without a mounted volume.
pub struct Nat<'a> {
    src: &'a dyn SectorSource,
    sb: &'a SuperBlock,
    cp: &'a Checkpoint,
}

impl<'a> Nat<'a> {
    /// Bind a NAT to its source, superblock and checkpoint.
    pub const fn new(src: &'a dyn SectorSource, sb: &'a SuperBlock, cp: &'a Checkpoint) -> Self {
        Self { src, sb, cp }
    }

    /// The block address of the NAT block that would hold `nid`.
    ///
    /// The formula is F2FS's, and it is not the obvious one:
    ///
    /// ```text
    /// addr = nat_blkaddr + (off << 1) - (off & (blocks_per_seg - 1))
    /// ```
    ///
    /// where `off` is `nid / NAT_ENTRIES_PER_BLOCK`. The doubling interleaves
    /// the two copies, and the subtraction folds them so that a NAT block and
    /// its alternate sit in the *same pair of segments* rather than half the
    /// NAT area apart. The point is locality: flipping one bitmap bit then
    /// rewrites a block that is a short seek from the one it replaces, and a
    /// sequential NAT scan touches each segment once instead of twice.
    ///
    /// Then the bitmap bit selects between the pair by adding one segment.
    pub fn nat_block_addr(&self, nid: u32) -> KernelResult<u32> {
        let off = nid
            .checked_div(NAT_ENTRIES_PER_BLOCK)
            .ok_or(KernelError::InternalError)?;

        let doubled = off.checked_mul(2).ok_or(KernelError::CorruptedData)?;
        let fold = off & self.sb.blocks_per_seg.saturating_sub(1);
        let mut addr = self
            .sb
            .nat_blkaddr
            .checked_add(doubled)
            .and_then(|v| v.checked_sub(fold))
            .ok_or(KernelError::CorruptedData)?;

        if test_bit(&self.cp.nat_bitmap, off as usize) {
            addr = addr
                .checked_add(self.sb.blocks_per_seg)
                .ok_or(KernelError::CorruptedData)?;
        }

        if !self.sb.is_in_nat_area(addr) {
            return Err(KernelError::CorruptedData);
        }
        Ok(addr)
    }

    /// Look up `nid`.
    ///
    /// The journal is consulted first and wins outright: an entry there is by
    /// definition newer than anything in the NAT area, because it is exactly
    /// the update that has not been written back yet.
    pub fn lookup(&self, nid: u32) -> KernelResult<NatEntry> {
        if nid == NULL_NID {
            return Err(KernelError::InvalidArgument);
        }

        if let Some(j) = self.cp.journal_lookup(nid) {
            return Ok(NatEntry {
                version: 0,
                ino: j.ino,
                block_addr: j.block_addr,
            });
        }

        let block = self.nat_block_addr(nid)?;
        let buf = read_bytes(self.src, block_to_offset(block), BLOCK_SIZE)?;

        let index = nid
            .checked_rem(NAT_ENTRIES_PER_BLOCK)
            .ok_or(KernelError::InternalError)? as usize;
        let off = index
            .checked_mul(NAT_ENTRY_LEN)
            .ok_or(KernelError::CorruptedData)?;

        Ok(NatEntry {
            version: read_u8_at(&buf, off)?,
            ino: read_u32(&buf, off.saturating_add(1))?,
            block_addr: read_u32(&buf, off.saturating_add(5))?,
        })
    }

    /// Read the node block for `nid`, validating its footer.
    ///
    /// The footer check is the equivalent of Btrfs's `bytenr` check: a node
    /// block carries the nid it *is*, so a NAT entry pointing at the wrong
    /// block yields a structure that parses perfectly and describes some other
    /// file. There is no checksum on a node block to catch it — F2FS relies on
    /// the footer — so skipping this leaves nothing at all guarding the
    /// indirection.
    pub fn read_node(&self, nid: u32) -> KernelResult<Vec<u8>> {
        let entry = self.lookup(nid)?;
        if entry.block_addr == NULL_ADDR || entry.block_addr == NEW_ADDR {
            return Err(KernelError::NotFound);
        }
        if !self.sb.is_valid_data_block(entry.block_addr) {
            return Err(KernelError::CorruptedData);
        }

        let buf = read_bytes(self.src, block_to_offset(entry.block_addr), BLOCK_SIZE)?;
        let footer_nid = read_u32(&buf, NODE_FOOTER_OFF)?;
        if footer_nid != nid {
            crate::serial_println!(
                "[f2fs] Node {} at block {} claims to be node {}.",
                nid,
                entry.block_addr,
                footer_nid
            );
            return Err(KernelError::CorruptedData);
        }
        Ok(buf)
    }
}

/// Read a `u8`, with the same error shape as the other scalar readers.
fn read_u8_at(buf: &[u8], off: usize) -> KernelResult<u8> {
    buf.get(off).copied().ok_or(KernelError::InvalidArgument)
}

// ---------------------------------------------------------------------------
// Inodes
// ---------------------------------------------------------------------------

/// A parsed F2FS inode.
#[derive(Debug, Clone)]
pub struct Inode {
    /// Node id, which for an inode is also its inode number.
    pub nid: u32,
    /// POSIX mode, type bits included.
    pub mode: u16,
    /// `i_inline` flags; see the `F2FS_INLINE_*` constants.
    pub inline: u8,
    /// Owning user.
    pub uid: u32,
    /// Owning group.
    pub gid: u32,
    /// Hard-link count.
    pub links: u32,
    /// Size in bytes.
    pub size: u64,
    /// Size in 512-byte units, as `stat` reports it.
    pub blocks: u64,
    /// Access time, seconds and nanoseconds.
    pub atime: (u64, u32),
    /// Change time.
    pub ctime: (u64, u32),
    /// Modification time.
    pub mtime: (u64, u32),
    /// Hash-table depth, for directories.
    pub current_depth: u32,
    /// `i_dir_level`, the directory's starting hash level.
    pub dir_level: u8,
    /// Parent inode number.
    pub pino: u32,
    /// Size of the extra-attribute area, in bytes.
    pub extra_isize: u16,
    /// Inline xattr size in `u32` units, when the flexible feature is on.
    pub inline_xattr_size: u16,
    /// The inode's own block, kept whole so inline data and inline dentries
    /// can be served without re-reading it.
    pub block: Vec<u8>,
}

impl Inode {
    /// Does this inode store its data inside its own block?
    pub const fn has_inline_data(&self) -> bool {
        self.inline & F2FS_INLINE_DATA != 0
    }

    /// Is this a directory whose entries live inside its own block?
    pub const fn has_inline_dentry(&self) -> bool {
        self.inline & F2FS_INLINE_DENTRY != 0
    }

    /// Is this a regular file?
    pub const fn is_file(&self) -> bool {
        self.mode & 0xF000 == 0x8000
    }

    /// Is this a directory?
    pub const fn is_dir(&self) -> bool {
        self.mode & 0xF000 == 0x4000
    }

    /// Is this a symbolic link?
    pub const fn is_symlink(&self) -> bool {
        self.mode & 0xF000 == 0xA000
    }

    /// `i_addr` slots this inode actually uses for block pointers.
    ///
    /// Three things eat into the nominal 923: the extra-attribute area at the
    /// front (which overlaps `i_addr`, hence the subtraction rather than an
    /// offset), and an inline xattr area at the back. Getting this count wrong
    /// does not fail — it shifts every direct pointer by a slot or two, which
    /// reads real blocks belonging to the wrong offsets of the same file.
    pub fn addrs_per_inode(&self) -> u32 {
        let extra_words = u32::from(self.extra_isize) / 4;
        let base = DEF_ADDRS_PER_INODE.saturating_sub(extra_words);
        base.saturating_sub(self.inline_xattr_words())
    }

    /// `u32` slots reserved at the end of `i_addr` for an inline xattr.
    fn inline_xattr_words(&self) -> u32 {
        if self.inline & F2FS_INLINE_XATTR == 0 {
            0
        } else {
            u32::from(self.inline_xattr_size)
        }
    }

    /// Byte offset of `i_addr[n]` within the inode block.
    fn addr_slot_off(&self, n: u32) -> KernelResult<usize> {
        let extra = usize::from(self.extra_isize);
        let base = I_ADDR_OFF.checked_add(extra).ok_or(KernelError::InternalError)?;
        let idx = usize::try_from(n).map_err(|_| KernelError::InternalError)?;
        base.checked_add(idx.checked_mul(4).ok_or(KernelError::InternalError)?)
            .ok_or(KernelError::InternalError)
    }

    /// Read `i_addr[n]`.
    pub fn addr(&self, n: u32) -> KernelResult<u32> {
        if n >= self.addrs_per_inode() {
            return Err(KernelError::InvalidArgument);
        }
        read_u32(&self.block, self.addr_slot_off(n)?)
    }

    /// Read `i_nid[n]` (`n` in 0..5).
    pub fn nid(&self, n: usize) -> KernelResult<u32> {
        if n >= super::raw::DEF_NIDS_PER_INODE {
            return Err(KernelError::InvalidArgument);
        }
        read_u32(
            &self.block,
            I_NID_OFF
                .checked_add(n.checked_mul(4).ok_or(KernelError::InternalError)?)
                .ok_or(KernelError::InternalError)?,
        )
    }

    /// The inline data/dentry area inside the inode block.
    ///
    /// Starts one `i_addr` slot past the extra-attribute area — that reserved
    /// slot is not decoration, it is where `i_addr[0]` would be, and F2FS
    /// keeps it clear so that code which blindly reads a "first block pointer"
    /// on an inline inode reads zero rather than the first bytes of the file's
    /// own contents.
    pub fn inline_area(&self) -> KernelResult<&[u8]> {
        let start = self.addr_slot_off(DEF_INLINE_RESERVED_SIZE)?;
        let words = self
            .addrs_per_inode()
            .saturating_sub(DEF_INLINE_RESERVED_SIZE);
        let len = usize::try_from(words)
            .ok()
            .and_then(|w| w.checked_mul(4))
            .ok_or(KernelError::InternalError)?;
        let end = start.checked_add(len).ok_or(KernelError::InternalError)?;
        self.block.get(start..end).ok_or(KernelError::CorruptedData)
    }

    /// Parse an inode from its node block.
    pub fn parse(nid: u32, sb: &SuperBlock, block: Vec<u8>) -> KernelResult<Self> {
        if block.len() < BLOCK_SIZE {
            return Err(KernelError::CorruptedData);
        }

        let inline = read_u8_at(&block, 3)?;

        // `i_extra_isize` only exists when the inode says so. Reading it
        // unconditionally would interpret the first four bytes of `i_addr` —
        // a block pointer — as a size, and then shift every subsequent field
        // by that pointer's low bits.
        let (extra_isize, inline_xattr_size) = if inline & F2FS_EXTRA_ATTR != 0 {
            let extra = read_u16(&block, 360)?;
            let xattr = if sb.has_feature(FEATURE_FLEXIBLE_INLINE_XATTR) {
                read_u16(&block, 362)?
            } else {
                0
            };
            (extra, xattr)
        } else {
            (0, 0)
        };

        // An extra-attribute area larger than the inode's pointer array would
        // make `addrs_per_inode` underflow to a huge number via saturation and
        // then index anywhere in the block.
        if u32::from(extra_isize) / 4 >= DEF_ADDRS_PER_INODE {
            return Err(KernelError::CorruptedData);
        }

        let inode = Self {
            nid,
            mode: read_u16(&block, 0)?,
            inline,
            uid: read_u32(&block, 4)?,
            gid: read_u32(&block, 8)?,
            links: read_u32(&block, 12)?,
            size: read_u64(&block, 16)?,
            blocks: read_u64(&block, 24)?,
            atime: (read_u64(&block, 32)?, read_u32(&block, 56)?),
            ctime: (read_u64(&block, 40)?, read_u32(&block, 60)?),
            mtime: (read_u64(&block, 48)?, read_u32(&block, 64)?),
            current_depth: read_u32(&block, 72)?,
            dir_level: read_u8_at(&block, 347)?,
            pino: read_u32(&block, 84)?,
            extra_isize,
            inline_xattr_size,
            block,
        };

        if u32::from(inode.inline_xattr_size) >= inode.addrs_per_inode().saturating_add(1) {
            return Err(KernelError::CorruptedData);
        }

        Ok(inode)
    }
}

/// Read and parse the inode `ino`.
pub fn read_inode(nat: &Nat<'_>, sb: &SuperBlock, ino: u32) -> KernelResult<Inode> {
    let block = nat.read_node(ino)?;

    // A node block's footer names the inode it belongs to. For an inode block
    // that must be the inode itself; anything else means the nid resolved to a
    // direct or indirect node, which would otherwise be parsed as a plausible
    // inode made of block pointers.
    let footer_ino = read_u32(&block, NODE_FOOTER_OFF.saturating_add(4))?;
    if footer_ino != ino {
        crate::serial_println!(
            "[f2fs] Node {} is not an inode (footer ino {}).",
            ino,
            footer_ino
        );
        return Err(KernelError::CorruptedData);
    }

    Inode::parse(ino, sb, block)
}

// ---------------------------------------------------------------------------
// File offset -> block address
// ---------------------------------------------------------------------------

/// Which node holds the pointer for a given file block, and where in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPath {
    /// Directly in the inode, at `i_addr[slot]`.
    Inode { slot: u32 },
    /// In the direct node at `i_nid[nid_index]`, at slot `slot`.
    Direct { nid_index: usize, slot: u32 },
    /// Via the indirect node at `i_nid[nid_index]`: slot `mid` there names a
    /// direct node, whose slot `slot` holds the address.
    Indirect {
        nid_index: usize,
        mid: u32,
        slot: u32,
    },
    /// Via the double-indirect node at `i_nid[4]`.
    DoubleIndirect { outer: u32, mid: u32, slot: u32 },
}

/// Decide where the pointer for file block `block` lives.
///
/// This mirrors Linux's `get_node_path`. The layout is the classic UFS shape —
/// some pointers in the inode, then two direct nodes, two indirect, one double
/// indirect — with F2FS's twist that the "pointers" at every level above the
/// last are node *ids*, not block addresses.
///
/// # Errors
///
/// [`KernelError::InvalidArgument`] for an offset past what the format can
/// address at all, which for 4 KiB blocks is a little over 3.9 TiB.
pub fn block_path(inode: &Inode, block: u64) -> KernelResult<BlockPath> {
    let direct_index = u64::from(inode.addrs_per_inode());
    let direct_blks = u64::from(ADDRS_PER_BLOCK);
    let nids_per_blk = u64::from(NIDS_PER_BLOCK);
    let indirect_blks = direct_blks.saturating_mul(nids_per_blk);
    let dindirect_blks = indirect_blks.saturating_mul(nids_per_blk);

    let mut b = block;

    if b < direct_index {
        return Ok(BlockPath::Inode {
            slot: u32::try_from(b).map_err(|_| KernelError::InternalError)?,
        });
    }
    b = b.saturating_sub(direct_index);

    for nid_index in 0..2usize {
        if b < direct_blks {
            return Ok(BlockPath::Direct {
                nid_index,
                slot: u32::try_from(b).map_err(|_| KernelError::InternalError)?,
            });
        }
        b = b.saturating_sub(direct_blks);
    }

    for nid_index in 2..4usize {
        if b < indirect_blks {
            let mid = b
                .checked_div(direct_blks)
                .ok_or(KernelError::InternalError)?;
            let slot = b
                .checked_rem(direct_blks)
                .ok_or(KernelError::InternalError)?;
            return Ok(BlockPath::Indirect {
                nid_index,
                mid: u32::try_from(mid).map_err(|_| KernelError::InternalError)?,
                slot: u32::try_from(slot).map_err(|_| KernelError::InternalError)?,
            });
        }
        b = b.saturating_sub(indirect_blks);
    }

    if b < dindirect_blks {
        let outer = b
            .checked_div(indirect_blks)
            .ok_or(KernelError::InternalError)?;
        let rem = b
            .checked_rem(indirect_blks)
            .ok_or(KernelError::InternalError)?;
        let mid = rem
            .checked_div(direct_blks)
            .ok_or(KernelError::InternalError)?;
        let slot = rem
            .checked_rem(direct_blks)
            .ok_or(KernelError::InternalError)?;
        return Ok(BlockPath::DoubleIndirect {
            outer: u32::try_from(outer).map_err(|_| KernelError::InternalError)?,
            mid: u32::try_from(mid).map_err(|_| KernelError::InternalError)?,
            slot: u32::try_from(slot).map_err(|_| KernelError::InternalError)?,
        });
    }

    Err(KernelError::InvalidArgument)
}

/// Read slot `slot` of the node `nid`, treating it as an array of `u32`.
///
/// Both direct nodes (block addresses) and indirect nodes (node ids) are
/// `u32` arrays occupying the whole block ahead of the footer, so one reader
/// serves both. A nid of zero short-circuits to zero, which is the sparse
/// case: F2FS does not allocate a direct node for a range of a file that was
/// never written.
fn node_slot(nat: &Nat<'_>, nid: u32, slot: u32) -> KernelResult<u32> {
    if nid == NULL_NID {
        return Ok(0);
    }
    if slot >= ADDRS_PER_BLOCK {
        return Err(KernelError::CorruptedData);
    }
    let block = nat.read_node(nid)?;
    let off = usize::try_from(slot)
        .ok()
        .and_then(|s| s.checked_mul(4))
        .ok_or(KernelError::InternalError)?;
    read_u32(&block, off)
}

/// Resolve file block `block` of `inode` to a device block address.
///
/// Returns [`NULL_ADDR`] for a hole and [`NEW_ADDR`] for a preallocated block;
/// both read as zeroes, and both are deliberately distinguished from a real
/// address rather than collapsed to `Option`, so a caller that forgets to
/// handle them fails a bounds check instead of reading block 0.
pub fn resolve_block(nat: &Nat<'_>, inode: &Inode, block: u64) -> KernelResult<u32> {
    match block_path(inode, block)? {
        BlockPath::Inode { slot } => inode.addr(slot),
        BlockPath::Direct { nid_index, slot } => {
            let nid = inode.nid(nid_index)?;
            node_slot(nat, nid, slot)
        }
        BlockPath::Indirect {
            nid_index,
            mid,
            slot,
        } => {
            let ind = inode.nid(nid_index)?;
            let direct = node_slot(nat, ind, mid)?;
            node_slot(nat, direct, slot)
        }
        BlockPath::DoubleIndirect { outer, mid, slot } => {
            let dind = inode.nid(4)?;
            let ind = node_slot(nat, dind, outer)?;
            let direct = node_slot(nat, ind, mid)?;
            node_slot(nat, direct, slot)
        }
    }
}

/// Read file block `block` of `inode`, zero-filling holes.
///
/// Always returns exactly [`BLOCK_SIZE`] bytes.
pub fn read_file_block(nat: &Nat<'_>, sb: &SuperBlock, inode: &Inode, block: u64) -> KernelResult<Vec<u8>> {
    let addr = resolve_block(nat, inode, block)?;
    if addr == NULL_ADDR || addr == NEW_ADDR {
        return Ok(vec![0u8; BLOCK_SIZE]);
    }
    if !sb.is_valid_data_block(addr) {
        crate::serial_println!(
            "[f2fs] Inode {} block {} points outside the main area ({}).",
            inode.nid,
            block,
            addr
        );
        return Err(KernelError::CorruptedData);
    }
    read_bytes(nat.src, block_to_offset(addr), BLOCK_SIZE)
}
