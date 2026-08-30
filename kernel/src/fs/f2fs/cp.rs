//! The F2FS checkpoint: which of the two packs is current, and what it carries.
//!
//! F2FS is a log-structured filesystem, so nothing is overwritten in place and
//! the "current state of the filesystem" is not a place on disk — it is a
//! *version*. The checkpoint is what pins it down: two packs alternate at the
//! start of the checkpoint area, and the one with the higher version that is
//! also intact is the filesystem. Mounting the other one is mounting the
//! filesystem as it was one checkpoint ago.
//!
//! # Why a pack is validated at both ends
//!
//! A pack is several blocks long, written first block to last. The version
//! number appears in *both* the first and the last block, and a pack counts as
//! intact only if the two agree. That is the whole crash-consistency
//! mechanism: a machine that died mid-checkpoint leaves a pack whose header
//! carries the new version and whose tail still carries the old one, and the
//! mismatch is what makes a reader fall back to the other pack rather than
//! trusting a half-written one. A CRC over the first block cannot substitute —
//! the first block is complete and checksums perfectly; it is the *rest* of
//! the pack that never arrived.
//!
//! # The NAT journal
//!
//! Recent node-address updates are not written to the NAT area at all; they
//! are carried inside the checkpoint pack as a small journal, and they
//! **override** the NAT area for the nids they mention. A reader that skips
//! the journal does not fail loudly — it reads the *previous* location of any
//! recently-written node, which is a block that still parses and still has a
//! valid footer, and so returns stale file contents. That is why this is here
//! rather than deferred as an optimisation.

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::raw::{BLOCK_SIZE, MAGIC, NAT_ENTRY_LEN, block_to_offset, read_u16, read_u32, read_u64};
use super::sb::SuperBlock;

// ---------------------------------------------------------------------------
// Checkpoint flags
// ---------------------------------------------------------------------------

/// The filesystem was cleanly unmounted.
pub const CP_UMOUNT_FLAG: u32 = 0x0000_0001;
/// Orphan inodes are listed in this pack.
pub const CP_ORPHAN_PRESENT_FLAG: u32 = 0x0000_0002;
/// Summaries are stored in the compacted layout.
pub const CP_COMPACT_SUM_FLAG: u32 = 0x0000_0004;
/// The filesystem was flagged for `fsck`.
pub const CP_FSCK_FLAG: u32 = 0x0000_0010;
/// Fastboot checkpoint (node summaries present).
pub const CP_FASTBOOT_FLAG: u32 = 0x0000_0020;
/// NAT bits are present after the pack.
pub const CP_NAT_BITS_FLAG: u32 = 0x0000_0080;
/// The NAT/SIT bitmaps are stored in the large layout.
pub const CP_LARGE_NAT_BITMAP_FLAG: u32 = 0x0000_0400;

/// Byte offset of `sit_nat_version_bitmap` within a checkpoint block.
const SIT_NAT_BITMAP_OFF: usize = 192;

/// Byte offset of `checksum_offset` within a checkpoint block.
///
/// Reached by counting the header out: three `u64`s and three `u32`s (36),
/// then `cur_node_segno[8]` + `cur_node_blkoff[8]` + `cur_data_segno[8]` +
/// `cur_data_blkoff[8]` (96), then seven `u32`s.
const CP_CHKSUM_OFFSET_FIELD: usize = 164;

/// Lowest byte offset the checkpoint CRC may sit at.
///
/// The bitmap area, because everything before it is a fixed header field: a
/// `checksum_offset` pointing into the header would have the CRC overlapping a
/// value the CRC is computed from, which no writer can produce and which would
/// make the verification depend on the order the two were written in.
const CP_MIN_CHKSUM_OFFSET: u32 = SIT_NAT_BITMAP_OFF as u32;
/// Highest: the last four bytes of the block.
const CP_CHKSUM_OFFSET: u32 = BLOCK_SIZE as u32 - 4;

/// Bytes of journal area in a summary block.
///
/// `4096 - 5 (footer) - 3584 (512 seven-byte summary entries) = 507`.
const SUM_JOURNAL_SIZE: usize = BLOCK_SIZE - 5 - SUM_ENTRY_SIZE;

/// Bytes of summary entries preceding the journal in a normal summary block.
const SUM_ENTRY_SIZE: usize = 7 * 512;

/// Length of one `nat_journal_entry`: nid(4) + `f2fs_nat_entry`(9).
const NAT_JOURNAL_ENTRY_LEN: usize = 4 + NAT_ENTRY_LEN;

/// Journal slots available: `(507 - 2) / 13`.
const NAT_JOURNAL_ENTRIES: usize = (SUM_JOURNAL_SIZE - 2) / NAT_JOURNAL_ENTRY_LEN;

/// Persistent current-segment types, used to locate a summary block.
const NR_CURSEG_DATA_TYPE: u32 = 3;
/// Data types plus the three node types.
const NR_CURSEG_PERSIST_TYPE: u32 = 6;

// ---------------------------------------------------------------------------
// The parsed checkpoint
// ---------------------------------------------------------------------------

/// One entry recovered from the checkpoint's NAT journal.
#[derive(Debug, Clone, Copy)]
pub struct NatJournalEntry {
    /// The node id this entry describes.
    pub nid: u32,
    /// The inode the node belongs to.
    pub ino: u32,
    /// Where the node block currently lives.
    pub block_addr: u32,
}

/// The current checkpoint, and everything a reader needs out of it.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Monotonically increasing version of this pack.
    pub version: u64,
    /// First block of the winning pack.
    pub start_block: u32,
    /// Blocks in the pack.
    pub total_block_count: u32,
    /// Block offset of the first summary block within the pack.
    pub start_sum_offset: u32,
    /// Flags; see the `CP_*` constants.
    pub flags: u32,
    /// Valid blocks in the main area.
    pub valid_block_count: u64,
    /// Valid nodes.
    pub valid_node_count: u32,
    /// Valid inodes.
    pub valid_inode_count: u32,
    /// Free segments in the main area.
    pub free_segment_count: u32,
    /// User-visible block count from the checkpoint's point of view.
    pub user_block_count: u64,
    /// The NAT version bitmap: one bit per NAT block, selecting which of the
    /// two copies is current.
    pub nat_bitmap: Vec<u8>,
    /// NAT overrides recovered from the pack's journal.
    pub nat_journal: Vec<NatJournalEntry>,
}

impl Checkpoint {
    /// Is `flag` set?
    pub const fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// Look up `nid` in the journal, if it is there.
    ///
    /// Linear, and deliberately so: the journal holds at most
    /// [`NAT_JOURNAL_ENTRIES`] (38) entries, so a hash map would cost an
    /// allocation and a hash to beat a scan that fits in a cache line or two.
    pub fn journal_lookup(&self, nid: u32) -> Option<NatJournalEntry> {
        self.nat_journal.iter().copied().find(|e| e.nid == nid)
    }
}

/// The checksum of a checkpoint block, computed the way F2FS defines it.
///
/// The CRC covers the block *except* the four bytes holding the CRC itself:
/// bytes `[0, off)` then `[off + 4, 4096)`. The second half is skipped
/// entirely when the CRC sits in the last four bytes, which is where modern
/// `mkfs.f2fs` puts it — so the two-region form only ever runs on older
/// volumes, and getting it wrong would go unnoticed on a freshly-made one.
fn checkpoint_crc(block: &[u8], crc_offset: u32) -> KernelResult<u32> {
    let off = crc_offset as usize;
    let head = block.get(..off).ok_or(KernelError::InvalidArgument)?;
    let mut crc = crate::crypto::crc32_raw(MAGIC, head);

    if crc_offset < CP_CHKSUM_OFFSET {
        let tail_start = off.checked_add(4).ok_or(KernelError::InvalidArgument)?;
        let tail = block
            .get(tail_start..BLOCK_SIZE)
            .ok_or(KernelError::InvalidArgument)?;
        crc = crate::crypto::crc32_raw(crc, tail);
    }

    Ok(crc)
}

/// Read one pack's version, verifying its CRC and its head/tail agreement.
///
/// Returns the version and the first block on success. `None` means the pack
/// is absent or damaged, which is an ordinary outcome — one of the two packs
/// is routinely mid-write — and so is not an error.
fn read_pack(src: &dyn SectorSource, start: u32) -> Option<(u64, Vec<u8>)> {
    let head = read_bytes(src, block_to_offset(start), BLOCK_SIZE).ok()?;

    let crc_offset = read_u32(&head, CP_CHKSUM_OFFSET_FIELD).ok()?;
    if !(CP_MIN_CHKSUM_OFFSET..=CP_CHKSUM_OFFSET).contains(&crc_offset) {
        return None;
    }
    let want = read_u32(&head, crc_offset as usize).ok()?;
    if checkpoint_crc(&head, crc_offset).ok()? != want {
        return None;
    }

    let version = read_u64(&head, 0).ok()?;
    let total = read_u32(&head, 136).ok()?;
    if total == 0 {
        return None;
    }

    // The last block of the pack. If the version there does not match, the
    // pack was never finished, whatever its first block says.
    let last = start.checked_add(total.checked_sub(1)?)?;
    let tail = read_bytes(src, block_to_offset(last), BLOCK_SIZE).ok()?;
    if read_u64(&tail, 0).ok()? != version {
        return None;
    }

    Some((version, head))
}

/// Extract the NAT version bitmap from a checkpoint block.
///
/// Three layouts exist and the flags pick between them. The `cp_payload` case
/// is the awkward one: the NAT bitmap is too large for the checkpoint block,
/// so it starts here and continues into the payload blocks that follow, while
/// the SIT bitmap is displaced into a block of its own. Only the NAT half
/// matters to a reader.
fn nat_bitmap_of(sb: &SuperBlock, block: &[u8], flags: u32) -> KernelResult<Vec<u8>> {
    let nat_bytes = read_u32(block, 160)? as usize;
    let sit_bytes = read_u32(block, 156)? as usize;

    let start = if flags & CP_LARGE_NAT_BITMAP_FLAG != 0 {
        // The large layout puts a checksum over the bitmaps immediately before
        // them, so the NAT bitmap begins four bytes later than it otherwise
        // would.
        SIT_NAT_BITMAP_OFF.checked_add(4)
    } else if sb.cp_payload > 0 {
        Some(SIT_NAT_BITMAP_OFF)
    } else {
        // Without a payload the two bitmaps are packed one after the other in
        // this block, SIT first.
        SIT_NAT_BITMAP_OFF.checked_add(sit_bytes)
    }
    .ok_or(KernelError::CorruptedData)?;

    let end = start
        .checked_add(nat_bytes)
        .ok_or(KernelError::CorruptedData)?;

    // A bitmap that would run past the end of the block is corruption in the
    // header, not a short read: truncating it silently would make every NAT
    // block past the cut resolve to copy 0 and read stale nodes.
    let bytes = block.get(start..end).ok_or(KernelError::CorruptedData)?;
    Ok(bytes.to_vec())
}

/// Locate the summary block holding the NAT journal and read the journal out.
///
/// The journal lives at a different place depending on how the summaries were
/// written. Both forms are handled because both occur in the field: the
/// compacted form after an ordinary checkpoint, the normal form after a clean
/// unmount.
fn read_nat_journal(
    src: &dyn SectorSource,
    sb: &SuperBlock,
    start_block: u32,
    total_block_count: u32,
    start_sum_offset: u32,
    flags: u32,
) -> Vec<NatJournalEntry> {
    let (block, offset) = if flags & CP_COMPACT_SUM_FLAG != 0 {
        // Compacted: the journals lead the first summary block, NAT first.
        (start_block.checked_add(start_sum_offset), 0usize)
    } else {
        // Normal: the hot-data summary block, counted back from the end of the
        // pack. Whether the node summaries are present shifts the base — a
        // clean unmount writes them, an ordinary checkpoint does not.
        let base = if flags & (CP_UMOUNT_FLAG | CP_FASTBOOT_FLAG) != 0 {
            NR_CURSEG_PERSIST_TYPE
        } else {
            NR_CURSEG_DATA_TYPE
        };
        let back = base.saturating_add(1);
        let blk = start_block
            .checked_add(total_block_count)
            .and_then(|v| v.checked_sub(back));
        (blk, SUM_ENTRY_SIZE)
    };

    let Some(block) = block else {
        return Vec::new();
    };
    // The summary block is inside the checkpoint area by construction, so an
    // address outside it came from a corrupt header. Check before reading, not
    // after: the point is to not issue the read at all.
    if !sb.is_in_checkpoint_area(block) {
        crate::serial_println!(
            "[f2fs] NAT journal block {} is outside the checkpoint area; ignoring.",
            block
        );
        return Vec::new();
    }
    // A journal that cannot be read is not a mount failure: at worst it costs
    // freshness on recently-written nodes, and refusing the whole volume over
    // it would be a strictly worse outcome than mounting a slightly older view.
    let Ok(buf) = read_bytes(src, block_to_offset(block), BLOCK_SIZE) else {
        crate::serial_println!(
            "[f2fs] NAT journal unreadable at block {}; ignoring.",
            block
        );
        return Vec::new();
    };

    let Ok(n) = read_u16(&buf, offset) else {
        return Vec::new();
    };
    let count = core::cmp::min(n as usize, NAT_JOURNAL_ENTRIES);

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = offset
            .saturating_add(2)
            .saturating_add(i.saturating_mul(NAT_JOURNAL_ENTRY_LEN));
        let (Ok(nid), Ok(ino), Ok(addr)) = (
            read_u32(&buf, base),
            read_u32(&buf, base.saturating_add(5)),
            read_u32(&buf, base.saturating_add(9)),
        ) else {
            break;
        };
        out.push(NatJournalEntry {
            nid,
            ino,
            block_addr: addr,
        });
    }
    out
}

/// Read the current checkpoint.
///
/// # Errors
///
/// [`KernelError::CorruptedData`] if neither pack is intact — which, unlike a
/// single damaged pack, genuinely means the filesystem cannot be mounted.
pub fn read_checkpoint(src: &dyn SectorSource, sb: &SuperBlock) -> KernelResult<Checkpoint> {
    let first = sb.cp_blkaddr;
    let second = sb
        .cp_blkaddr
        .checked_add(sb.blocks_per_seg)
        .ok_or(KernelError::CorruptedData)?;

    let a = read_pack(src, first);
    let b = read_pack(src, second);

    // Higher version wins; a tie cannot happen on a sane volume, and if it did
    // either pack would describe the same filesystem.
    let (start_block, block) = match (a, b) {
        (Some((va, ba)), Some((vb, bb))) => {
            if vb > va {
                (second, bb)
            } else {
                (first, ba)
            }
        }
        (Some((_, ba)), None) => (first, ba),
        (None, Some((_, bb))) => (second, bb),
        (None, None) => {
            crate::serial_println!("[f2fs] Both checkpoint packs are damaged.");
            return Err(KernelError::CorruptedData);
        }
    };

    let flags = read_u32(&block, 132)?;
    let total_block_count = read_u32(&block, 136)?;
    let start_sum_offset = read_u32(&block, 140)?;

    let nat_bitmap = nat_bitmap_of(sb, &block, flags)?;
    let nat_journal = read_nat_journal(
        src,
        sb,
        start_block,
        total_block_count,
        start_sum_offset,
        flags,
    );

    Ok(Checkpoint {
        version: read_u64(&block, 0)?,
        start_block,
        total_block_count,
        start_sum_offset,
        flags,
        valid_block_count: read_u64(&block, 16)?,
        valid_node_count: read_u32(&block, 144)?,
        valid_inode_count: read_u32(&block, 148)?,
        free_segment_count: read_u32(&block, 32)?,
        user_block_count: read_u64(&block, 8)?,
        nat_bitmap,
        nat_journal,
    })
}
