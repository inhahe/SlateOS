//! ext4 journal (jbd2) — write-ahead logging for crash recovery.
//!
//! The journal records block-level changes in a circular log before
//! they are committed to their final on-disk locations.  If the system
//! crashes mid-write, the journal can replay committed transactions
//! to restore consistency.
//!
//! ## On-disk format
//!
//! The journal is stored in inode 8 (EXT4_JOURNAL_INO).  Its data blocks
//! form a circular log with these block types:
//!
//! - **Superblock** (block 0): journal metadata (sequence numbers, size)
//! - **Descriptor**: lists which filesystem blocks are being modified
//! - **Data**: copies of the modified blocks (before they go to final location)
//! - **Commit**: marks the end of a transaction
//! - **Revoke**: marks blocks that should NOT be replayed
//!
//! ## Transaction flow
//!
//! 1. `begin()` — start a new transaction
//! 2. `log_block(block_nr, data)` — record a block modification
//! 3. `commit()` — write commit record, then write blocks to final locations
//!
//! ## Implementation
//!
//! Based on Linux `fs/jbd2/` (simplified — no async commit, single-threaded
//! transactions).
//!
//! ## Revoke blocks
//!
//! Revoke blocks list filesystem block numbers that must NOT be replayed
//! during recovery, even if a prior transaction logged them.  This
//! prevents stale data from overwriting blocks that were freed and
//! reallocated between the transaction that logged them and the crash.
//!
//! Recovery uses a two-pass approach (matching Linux jbd2):
//!
//! - **Pass 1 (SCAN)**: walk the journal collecting all revoke records
//!   into a revoke table: `BTreeMap<u64, u32>` mapping filesystem block
//!   number → highest revoking sequence number.
//! - **Pass 2 (REPLAY)**: replay data blocks from descriptor transactions,
//!   skipping any block whose revoke sequence is ≥ the descriptor's
//!   transaction sequence.

#![allow(dead_code)] // Infrastructure for upcoming integration.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::io::BlockReader;

// ---------------------------------------------------------------------------
// Journal on-disk structures
// ---------------------------------------------------------------------------

/// Journal magic number (same as jbd/jbd2).
const JBD2_MAGIC: u32 = 0xC03B_3998;

/// Journal block types.
mod block_type {
    /// Descriptor block (lists modified blocks).
    pub const DESCRIPTOR: u32 = 1;
    /// Commit block (end of transaction).
    pub const COMMIT: u32 = 2;
    /// Journal superblock v1.
    pub const SUPERBLOCK_V1: u32 = 3;
    /// Journal superblock v2.
    pub const SUPERBLOCK_V2: u32 = 4;
    /// Revoke block.
    pub const REVOKE: u32 = 5;
}

/// Flags in descriptor block tags.
mod tag_flags {
    /// This tag has the UUID field (only first tag or after SAME_UUID is clear).
    pub const ESCAPE: u32 = 1;
    /// Same UUID as previous tag.
    pub const SAME_UUID: u32 = 2;
    /// Last tag in this descriptor block.
    pub const LAST_TAG: u32 = 8;
}

/// Journal superblock (on-disk format, first block of journal).
///
/// **Layout documentation only** — all fields are big-endian on disk,
/// so we parse them manually with `read_be32` instead of using
/// `read_struct_pub` (which assumes native byte order).
///
/// 1024 bytes (padded to block size).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalSuperblock {
    /// Block type header.
    s_header_magic: u32,        // 0x00
    s_header_blocktype: u32,    // 0x04
    s_header_sequence: u32,     // 0x08

    /// Journal device block size (usually same as fs block size).
    s_blocksize: u32,           // 0x0C
    /// Total number of blocks in the journal.
    s_maxlen: u32,              // 0x10
    /// First usable block in the journal (after superblock).
    s_first: u32,               // 0x14

    /// First expected commit ID in the log.
    s_sequence: u32,            // 0x18
    /// First block of the log that still needs replaying.
    s_start: u32,               // 0x1C

    /// Error value (non-zero if the journal has been aborted).
    s_errno: u32,               // 0x20

    // --- V2 fields ---
    /// Feature compat flags.
    s_feature_compat: u32,      // 0x24
    /// Feature incompat flags.
    s_feature_incompat: u32,    // 0x28
    /// Feature ro_compat flags.
    s_feature_ro_compat: u32,   // 0x2C

    /// Journal UUID.
    s_uuid: [u8; 16],          // 0x30

    /// Number of filesystems using this journal.
    s_nr_users: u32,            // 0x40

    /// Location of superblock copy (dynsuper).
    s_dynsuper: u32,            // 0x44

    /// Max journal blocks per transaction.
    s_max_transaction: u32,     // 0x48
    /// Max data blocks per transaction.
    s_max_trans_data: u32,      // 0x4C

    /// Padding to 1024 bytes.
    _padding: [u32; 44],       // 0x50 - 0xFF (176 bytes)

    /// Filesystem UUID(s) that use this journal.
    s_users: [u8; 768],        // 0x100 - 0x3FF
}

// Compile-time size check.
const _: () = assert!(core::mem::size_of::<JournalSuperblock>() == 1024);

/// Block header (common to descriptor, commit, revoke blocks).
///
/// **Layout documentation only** — parsed manually via `read_be32`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalBlockHeader {
    h_magic: u32,
    h_blocktype: u32,
    h_sequence: u32,
}

/// Descriptor block tag (v1, 8 bytes).
///
/// **Layout documentation only** — parsed manually via `read_be32`.
/// Each tag describes one block being logged.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalBlockTag {
    /// Filesystem block number being logged (low 32 bits).
    t_blocknr: u32,
    /// Flags (ESCAPE, SAME_UUID, LAST_TAG).
    t_flags: u32,
}

/// Revoke block header (jbd2 format).
///
/// Follows the standard `JournalBlockHeader` in a revoke block.  The
/// `r_count` field indicates the total number of bytes used in the block
/// (including the header), so the number of revoked block entries is
/// `(r_count - 16) / 4` for 32-bit entries or `(r_count - 16) / 8` for
/// 64-bit entries.  Our implementation supports 32-bit block numbers
/// (sufficient for < 16 TiB filesystems with 4k blocks).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalRevokeHeader {
    /// Standard journal block header (12 bytes).
    header: JournalBlockHeader,
    /// Total bytes used in this revoke block (including this header).
    r_count: u32,
}

// ---------------------------------------------------------------------------
// Big-endian helpers — jbd2 on-disk format is network byte order
// ---------------------------------------------------------------------------

/// Read a big-endian u32 from `buf` at the given byte offset.
///
/// Returns 0 if the slice is too short, which is safe for journal
/// parsing (0 is never a valid magic or meaningful field value).
#[inline]
fn read_be32(buf: &[u8], offset: usize) -> u32 {
    buf.get(offset..offset.wrapping_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map_or(0, u32::from_be_bytes)
}

/// Read a big-endian u64 from `buf` at the given byte offset.
///
/// For 64-bit block numbers in revoke blocks.  Returns 0 on short
/// reads.
#[inline]
fn read_be64(buf: &[u8], offset: usize) -> u64 {
    buf.get(offset..offset.wrapping_add(8))
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map_or(0, u64::from_be_bytes)
}

/// Write a big-endian u32 to `buf` at the given byte offset.
#[inline]
fn write_be32(buf: &mut [u8], offset: usize, val: u32) {
    if let Some(dest) = buf.get_mut(offset..offset.wrapping_add(4)) {
        dest.copy_from_slice(&val.to_be_bytes());
    }
}

// ---------------------------------------------------------------------------
// In-memory journal state
// ---------------------------------------------------------------------------

/// JBD2 incompat feature flag for 64-bit block numbers.
///
/// When set, descriptor tags use 12-byte format (with high 32-bit
/// block number) and revoke entries are 8 bytes instead of 4.
const JBD2_FEATURE_INCOMPAT_64BIT: u32 = 0x0000_0002;

/// Parsed journal metadata.
#[derive(Debug)]
pub struct JournalState {
    /// Block size of the journal (same as filesystem).
    block_size: u32,
    /// Total blocks in the journal.
    max_len: u32,
    /// First usable block (after superblock).
    first_block: u32,
    /// Next sequence number for new transactions.
    next_sequence: u32,
    /// Current write position in the circular log.
    write_pos: u32,
    /// Journal inode number (usually 8).
    journal_ino: u32,
    /// Physical block numbers of the journal's data blocks.
    /// Mapped from the journal inode's extent tree.
    journal_blocks: Vec<u64>,
    /// Whether the journal uses 64-bit block numbers in descriptor
    /// tags and revoke entries.
    has_64bit: bool,
}

/// A pending transaction — blocks that will be committed together.
#[derive(Debug)]
pub struct Transaction {
    /// Sequence number for this transaction.
    sequence: u32,
    /// List of (filesystem_block_nr, block_data) pairs.
    blocks: Vec<(u64, Vec<u8>)>,
}

/// The journal subsystem for an ext4 filesystem.
pub struct Journal {
    /// Reader for the underlying block device.
    reader: BlockReader,
    /// Journal state.
    state: JournalState,
    /// Currently open transaction (None if no transaction in progress).
    active: Option<Transaction>,
}

impl Journal {
    /// Open the journal from the filesystem's journal inode.
    ///
    /// Reads the journal superblock, validates it, and prepares for
    /// transaction processing.
    pub fn open(
        reader: &BlockReader,
        journal_ino: u32,
        journal_blocks: Vec<u64>,
        fs_block_size: u32,
    ) -> KernelResult<Self> {
        if journal_blocks.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        // Read the journal superblock (first block of the journal).
        let jsb_block = *journal_blocks.first().ok_or(KernelError::IoError)?;
        let mut jsb_buf = vec![0u8; fs_block_size as usize];
        reader.read_block(jsb_block, &mut jsb_buf)?;

        // Parse the journal superblock manually — jbd2 uses big-endian.
        // (Cannot use read_struct_pub which assumes native/LE byte order.)
        let magic = read_be32(&jsb_buf, 0x00);
        let blocktype = read_be32(&jsb_buf, 0x04);
        let _header_seq = read_be32(&jsb_buf, 0x08);
        let s_blocksize = read_be32(&jsb_buf, 0x0C);
        let s_maxlen = read_be32(&jsb_buf, 0x10);
        let s_first = read_be32(&jsb_buf, 0x14);
        let s_sequence = read_be32(&jsb_buf, 0x18);
        let s_start = read_be32(&jsb_buf, 0x1C);
        let _s_errno = read_be32(&jsb_buf, 0x20);
        let _s_feature_compat = read_be32(&jsb_buf, 0x24);
        let s_feature_incompat = read_be32(&jsb_buf, 0x28);

        // Validate magic.
        if magic != JBD2_MAGIC {
            crate::serial_println!(
                "[ext4-journal] Bad journal magic: {:#x} (expected {:#x})",
                magic, JBD2_MAGIC
            );
            return Err(KernelError::IoError);
        }

        if blocktype != block_type::SUPERBLOCK_V1 && blocktype != block_type::SUPERBLOCK_V2 {
            crate::serial_println!(
                "[ext4-journal] Bad journal superblock type: {}",
                blocktype
            );
            return Err(KernelError::IoError);
        }

        let has_64bit = (s_feature_incompat & JBD2_FEATURE_INCOMPAT_64BIT) != 0;
        let state = JournalState {
            block_size: s_blocksize,
            max_len: s_maxlen,
            first_block: s_first,
            next_sequence: s_sequence,
            write_pos: s_start,
            journal_ino,
            journal_blocks,
            has_64bit,
        };

        crate::serial_println!(
            "[ext4-journal] Opened: {} blocks, seq={}, start={}",
            state.max_len, state.next_sequence, state.write_pos
        );

        // Create reader for the journal device (same device as fs).
        let journal_reader = BlockReader::new(reader.device(), fs_block_size)?;

        Ok(Self {
            reader: journal_reader,
            state,
            active: None,
        })
    }

    /// Begin a new transaction.
    ///
    /// Only one transaction can be active at a time.
    pub fn begin(&mut self) -> KernelResult<()> {
        if self.active.is_some() {
            return Err(KernelError::InvalidArgument);
        }

        let seq = self.state.next_sequence;
        self.active = Some(Transaction {
            sequence: seq,
            blocks: Vec::new(),
        });

        Ok(())
    }

    /// Log a block modification in the current transaction.
    ///
    /// `block_nr` is the filesystem block number.
    /// `data` is the new content of the block.
    pub fn log_block(&mut self, block_nr: u64, data: &[u8]) -> KernelResult<()> {
        let txn = self.active.as_mut()
            .ok_or(KernelError::InvalidArgument)?;

        if data.len() != self.state.block_size as usize {
            return Err(KernelError::InvalidArgument);
        }

        txn.blocks.push((block_nr, data.to_vec()));
        Ok(())
    }

    /// Commit the current transaction.
    ///
    /// Writes:
    /// 1. Descriptor block (lists which fs blocks are being modified)
    /// 2. Data blocks (copies of the modified blocks)
    /// 3. Commit block (marks transaction complete)
    ///
    /// Then writes the actual blocks to their final filesystem locations.
    pub fn commit(&mut self) -> KernelResult<()> {
        let txn = self.active.take()
            .ok_or(KernelError::InvalidArgument)?;

        if txn.blocks.is_empty() {
            // Empty transaction — nothing to do.
            return Ok(());
        }

        let block_size = self.state.block_size as usize;
        let mut pos = self.state.write_pos;

        // 1. Write descriptor block.
        let mut desc_buf = vec![0u8; block_size];
        self.write_block_header(&mut desc_buf, block_type::DESCRIPTOR, txn.sequence);

        // Write tags for each block in the transaction (big-endian).
        let header_size: usize = 12;
        let tag_size: usize = 8;
        let mut tag_offset = header_size;

        for (i, (fs_block, _)) in txn.blocks.iter().enumerate() {
            if tag_offset.saturating_add(tag_size) > block_size {
                // Descriptor block is full — would need a continuation.
                // For simplicity, limit transactions to fit in one descriptor.
                break;
            }

            let mut flags = tag_flags::SAME_UUID;
            if i == txn.blocks.len().saturating_sub(1) {
                flags |= tag_flags::LAST_TAG;
            }

            // Write tag (big-endian per jbd2 spec).
            write_be32(&mut desc_buf, tag_offset, *fs_block as u32);
            write_be32(&mut desc_buf, tag_offset.wrapping_add(4), flags);

            tag_offset = tag_offset.saturating_add(tag_size);
        }

        // Write descriptor block to journal.
        let desc_phys = self.journal_phys_block(pos)?;
        self.reader.write_block(desc_phys, &desc_buf)?;
        pos = self.advance_pos(pos);

        // 2. Write data blocks to journal.
        for (_, data) in &txn.blocks {
            let data_phys = self.journal_phys_block(pos)?;
            self.reader.write_block(data_phys, data)?;
            pos = self.advance_pos(pos);
        }

        // 3. Write commit block.
        let mut commit_buf = vec![0u8; block_size];
        self.write_block_header(&mut commit_buf, block_type::COMMIT, txn.sequence);
        let commit_phys = self.journal_phys_block(pos)?;
        self.reader.write_block(commit_phys, &commit_buf)?;
        pos = self.advance_pos(pos);

        // Flush the journal writes.
        self.reader.flush()?;

        // 4. Write blocks to their final filesystem locations.
        for (fs_block, data) in &txn.blocks {
            self.reader.write_block(*fs_block, data)?;
        }

        // Flush final writes.
        self.reader.flush()?;

        // 5. Update journal superblock to advance the sequence and start.
        self.state.next_sequence = txn.sequence.wrapping_add(1);
        self.state.write_pos = pos;
        self.write_journal_superblock()?;

        Ok(())
    }

    /// Replay committed transactions from the journal.
    ///
    /// Called during mount when the RECOVER incompat flag is set.
    /// Uses a two-pass approach (matching Linux jbd2):
    ///
    /// - **Pass 1 (SCAN)**: walk the journal collecting all revoke
    ///   records into a table: block_nr → highest revoking sequence.
    /// - **Pass 2 (REPLAY)**: replay data blocks from descriptor
    ///   transactions, skipping any whose revoke sequence ≥ the
    ///   descriptor's transaction sequence.
    pub fn replay(&mut self) -> KernelResult<u32> {
        let start_pos = self.state.write_pos;
        let start_seq = self.state.next_sequence;

        crate::serial_println!(
            "[ext4-journal] Starting recovery from pos={}, seq={}",
            start_pos, start_seq,
        );

        // Pass 1: scan for revoke records.
        let (revoke_table, end_seq) = self.scan_revokes(start_pos, start_seq)?;

        if !revoke_table.is_empty() {
            crate::serial_println!(
                "[ext4-journal] Revoke table: {} entries (blocks protected from stale replay)",
                revoke_table.len(),
            );
        }

        // Pass 2: replay data blocks, respecting revokes.
        let (replayed, final_pos, final_seq) =
            self.replay_with_revokes(start_pos, start_seq, &revoke_table)?;

        if replayed > 0 {
            crate::serial_println!(
                "[ext4-journal] Replayed {} blocks from journal",
                replayed,
            );
            self.reader.flush()?;
        }

        // Update journal state to past the last replayed transaction.
        self.state.next_sequence = final_seq;
        self.state.write_pos = final_pos;
        self.write_journal_superblock()?;

        // Sanity check: both passes should agree on where the journal ends.
        if final_seq != end_seq {
            crate::serial_println!(
                "[ext4-journal] WARNING: scan ended at seq={} but replay at seq={} (journal may be inconsistent)",
                end_seq, final_seq,
            );
        }

        Ok(replayed)
    }

    /// Pass 1: scan the journal collecting revoke records.
    ///
    /// Returns (revoke_table, end_sequence) where the revoke table maps
    /// filesystem block number → highest revoking transaction sequence.
    fn scan_revokes(
        &self,
        start_pos: u32,
        start_seq: u32,
    ) -> KernelResult<(BTreeMap<u64, u32>, u32)> {
        let block_size = self.state.block_size as usize;
        // jbd2 block header: 12 bytes (magic + type + seq), all big-endian.
        let header_size: usize = 12;
        // jbd2 v1 descriptor tag: 8 bytes (blocknr_lo + flags), big-endian.
        let tag_size: usize = 8;
        let revoke_entry_size: usize = if self.state.has_64bit { 8 } else { 4 };

        let mut revoke_table: BTreeMap<u64, u32> = BTreeMap::new();
        let mut pos = start_pos;
        let mut expected_seq = start_seq;
        let max_scan = self.state.max_len;

        for _ in 0..max_scan {
            let phys = self.journal_phys_block(pos)?;
            let mut buf = vec![0u8; block_size];
            self.reader.read_block(phys, &mut buf)?;

            // Parse header as big-endian.
            let h_magic = read_be32(&buf, 0);
            let h_blocktype = read_be32(&buf, 4);
            let h_sequence = read_be32(&buf, 8);
            if h_magic != JBD2_MAGIC || h_sequence != expected_seq {
                break;
            }

            match h_blocktype {
                block_type::DESCRIPTOR => {
                    // Skip past the descriptor and its data blocks.
                    let mut tag_offset = header_size;
                    let mut data_block_count = 0u32;

                    while tag_offset.saturating_add(tag_size) <= block_size {
                        let t_flags = read_be32(&buf, tag_offset.wrapping_add(4));
                        data_block_count = data_block_count.saturating_add(1);

                        let is_last = (t_flags & tag_flags::LAST_TAG) != 0;
                        tag_offset = tag_offset.saturating_add(tag_size);

                        if is_last {
                            break;
                        }
                    }

                    // Advance past the descriptor block + data blocks.
                    pos = self.advance_pos(pos);
                    for _ in 0..data_block_count {
                        pos = self.advance_pos(pos);
                    }
                }
                block_type::COMMIT => {
                    expected_seq = expected_seq.wrapping_add(1);
                    pos = self.advance_pos(pos);
                }
                block_type::REVOKE => {
                    // Parse revoke block: extract filesystem block numbers.
                    // Revoke header is block header (12 bytes) + r_count (4 bytes) = 16 bytes.
                    let revoke_hdr_size: usize = 16;

                    // r_count is at offset 12, big-endian.
                    let r_count = (read_be32(&buf, 12) as usize).min(block_size);

                    // Parse revoked block numbers (big-endian, per jbd2 spec).
                    let mut offset = revoke_hdr_size;
                    while offset.saturating_add(revoke_entry_size) <= r_count {
                        let block_nr = if self.state.has_64bit {
                            read_be64(&buf, offset)
                        } else {
                            u64::from(read_be32(&buf, offset))
                        };

                        // Record the highest revoking sequence for this block.
                        let seq = expected_seq;
                        revoke_table
                            .entry(block_nr)
                            .and_modify(|existing| {
                                if seq > *existing {
                                    *existing = seq;
                                }
                            })
                            .or_insert(seq);

                        offset = offset.saturating_add(revoke_entry_size);
                    }

                    pos = self.advance_pos(pos);
                }
                _ => break,
            }
        }

        Ok((revoke_table, expected_seq))
    }

    /// Pass 2: replay data blocks, skipping revoked ones.
    ///
    /// Returns (replayed_count, final_pos, final_seq).
    fn replay_with_revokes(
        &self,
        start_pos: u32,
        start_seq: u32,
        revoke_table: &BTreeMap<u64, u32>,
    ) -> KernelResult<(u32, u32, u32)> {
        let block_size = self.state.block_size as usize;
        let header_size: usize = 12;
        let tag_size: usize = 8;

        let mut replayed = 0u32;
        let mut revoked = 0u32;
        let mut pos = start_pos;
        let mut expected_seq = start_seq;
        let max_scan = self.state.max_len;

        for _ in 0..max_scan {
            let phys = self.journal_phys_block(pos)?;
            let mut buf = vec![0u8; block_size];
            self.reader.read_block(phys, &mut buf)?;

            // Parse header as big-endian.
            let h_magic = read_be32(&buf, 0);
            let h_blocktype = read_be32(&buf, 4);
            let h_sequence = read_be32(&buf, 8);
            if h_magic != JBD2_MAGIC || h_sequence != expected_seq {
                break;
            }

            match h_blocktype {
                block_type::DESCRIPTOR => {
                    let txn_seq = expected_seq;

                    // Parse tags to find which blocks are in this transaction.
                    let mut tag_offset = header_size;
                    let mut block_positions = Vec::new();

                    while tag_offset.saturating_add(tag_size) <= block_size {
                        let t_blocknr = read_be32(&buf, tag_offset);
                        let t_flags = read_be32(&buf, tag_offset.wrapping_add(4));

                        block_positions.push(u64::from(t_blocknr));

                        let is_last = (t_flags & tag_flags::LAST_TAG) != 0;
                        tag_offset = tag_offset.saturating_add(tag_size);

                        if is_last {
                            break;
                        }
                    }

                    // Read and replay each data block, checking revokes.
                    pos = self.advance_pos(pos);
                    for fs_block in &block_positions {
                        let data_phys = self.journal_phys_block(pos)?;

                        // Check if this block was revoked by a later transaction.
                        // If revoke_seq >= txn_seq, the block was freed/reallocated
                        // AFTER this transaction, so replaying would corrupt data.
                        let is_revoked = revoke_table
                            .get(fs_block)
                            .map_or(false, |&revoke_seq| revoke_seq >= txn_seq);

                        if is_revoked {
                            revoked = revoked.saturating_add(1);
                        } else {
                            let mut data_buf = vec![0u8; block_size];
                            self.reader.read_block(data_phys, &mut data_buf)?;
                            self.reader.write_block(*fs_block, &data_buf)?;
                            replayed = replayed.saturating_add(1);
                        }

                        pos = self.advance_pos(pos);
                    }
                }
                block_type::COMMIT => {
                    expected_seq = expected_seq.wrapping_add(1);
                    pos = self.advance_pos(pos);
                }
                block_type::REVOKE => {
                    // Already processed in pass 1 — skip.
                    pos = self.advance_pos(pos);
                }
                _ => break,
            }
        }

        if revoked > 0 {
            crate::serial_println!(
                "[ext4-journal] Skipped {} revoked blocks during replay",
                revoked,
            );
        }

        Ok((replayed, pos, expected_seq))
    }

    /// Check if the journal needs recovery.
    pub fn needs_recovery(&self) -> bool {
        // If write_pos > first_block and sequence > 0, there may be
        // uncommitted data.  A full check would scan for valid transactions.
        self.state.write_pos != 0
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Map a journal-relative block index to a physical block number.
    fn journal_phys_block(&self, journal_block: u32) -> KernelResult<u64> {
        self.state.journal_blocks
            .get(journal_block as usize)
            .copied()
            .ok_or(KernelError::IoError)
    }

    /// Advance the write position, wrapping around the circular log.
    fn advance_pos(&self, pos: u32) -> u32 {
        let next = pos.saturating_add(1);
        if next >= self.state.max_len {
            self.state.first_block
        } else {
            next
        }
    }

    /// Write a block header (magic + type + sequence) in big-endian.
    fn write_block_header(&self, buf: &mut [u8], blocktype: u32, sequence: u32) {
        write_be32(buf, 0, JBD2_MAGIC);
        write_be32(buf, 4, blocktype);
        write_be32(buf, 8, sequence);
    }

    /// Write the journal superblock back to disk.
    fn write_journal_superblock(&self) -> KernelResult<()> {
        let block_size = self.state.block_size as usize;
        let jsb_phys = self.journal_phys_block(0)?;

        // Read the existing superblock.
        let mut buf = vec![0u8; block_size];
        self.reader.read_block(jsb_phys, &mut buf)?;

        // Update sequence and start position (big-endian).
        write_be32(&mut buf, 0x18, self.state.next_sequence);
        write_be32(&mut buf, 0x1C, self.state.write_pos);

        self.reader.write_block(jsb_phys, &buf)?;
        self.reader.flush()
    }
}
