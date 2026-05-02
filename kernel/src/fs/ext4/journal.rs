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
//! Based on Linux `fs/jbd2/` (simplified — no revoke blocks, no async commit,
//! single-threaded transactions).

#![allow(dead_code)] // Infrastructure for upcoming integration.

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
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalBlockHeader {
    h_magic: u32,
    h_blocktype: u32,
    h_sequence: u32,
}

/// Descriptor block tag (v1, 8 bytes).
///
/// Each tag describes one block being logged.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct JournalBlockTag {
    /// Filesystem block number being logged (low 32 bits).
    t_blocknr: u32,
    /// Flags (ESCAPE, SAME_UUID, LAST_TAG).
    t_flags: u32,
}

// ---------------------------------------------------------------------------
// In-memory journal state
// ---------------------------------------------------------------------------

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

        // Parse the journal superblock.
        let jsb = super::driver::read_struct_pub::<JournalSuperblock>(&jsb_buf)?;

        // Validate magic.
        if jsb.s_header_magic != JBD2_MAGIC {
            crate::serial_println!(
                "[ext4-journal] Bad journal magic: {:#x} (expected {:#x})",
                jsb.s_header_magic, JBD2_MAGIC
            );
            return Err(KernelError::IoError);
        }

        let blocktype = jsb.s_header_blocktype;
        if blocktype != block_type::SUPERBLOCK_V1 && blocktype != block_type::SUPERBLOCK_V2 {
            crate::serial_println!(
                "[ext4-journal] Bad journal superblock type: {}",
                blocktype
            );
            return Err(KernelError::IoError);
        }

        let state = JournalState {
            block_size: jsb.s_blocksize,
            max_len: jsb.s_maxlen,
            first_block: jsb.s_first,
            next_sequence: jsb.s_sequence,
            write_pos: jsb.s_start,
            journal_ino,
            journal_blocks,
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

        // Write tags for each block in the transaction.
        let header_size = core::mem::size_of::<JournalBlockHeader>();
        let tag_size = core::mem::size_of::<JournalBlockTag>();
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

            // Write tag.
            if let Some(dest) = desc_buf.get_mut(tag_offset..tag_offset + 4) {
                dest.copy_from_slice(&(*fs_block as u32).to_le_bytes());
            }
            if let Some(dest) = desc_buf.get_mut(tag_offset + 4..tag_offset + 8) {
                dest.copy_from_slice(&flags.to_le_bytes());
            }

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
    /// Scans the journal for committed transactions and replays their
    /// data blocks to the final filesystem locations.
    pub fn replay(&mut self) -> KernelResult<u32> {
        let block_size = self.state.block_size as usize;
        let header_size = core::mem::size_of::<JournalBlockHeader>();
        let tag_size = core::mem::size_of::<JournalBlockTag>();
        let mut replayed = 0u32;
        let mut pos = self.state.write_pos;
        let mut expected_seq = self.state.next_sequence;

        crate::serial_println!(
            "[ext4-journal] Starting replay from pos={}, seq={}",
            pos, expected_seq
        );

        // Scan for transactions.
        let max_scan = self.state.max_len;
        for _ in 0..max_scan {
            // Read block at current position.
            let phys = self.journal_phys_block(pos)?;
            let mut buf = vec![0u8; block_size];
            self.reader.read_block(phys, &mut buf)?;

            // Check for a valid header.
            let header = super::driver::read_struct_pub::<JournalBlockHeader>(&buf)?;
            if header.h_magic != JBD2_MAGIC {
                break; // End of journal log.
            }
            if header.h_sequence != expected_seq {
                break; // Not the transaction we're looking for.
            }

            match header.h_blocktype {
                block_type::DESCRIPTOR => {
                    // Parse tags to find which blocks are in this transaction.
                    let mut tag_offset = header_size;
                    let mut block_positions = Vec::new();

                    while tag_offset.saturating_add(tag_size) <= block_size {
                        let tag = super::driver::read_struct_pub::<JournalBlockTag>(
                            buf.get(tag_offset..).ok_or(KernelError::IoError)?
                        )?;

                        block_positions.push(u64::from(tag.t_blocknr));

                        let is_last = (tag.t_flags & tag_flags::LAST_TAG) != 0;
                        tag_offset = tag_offset.saturating_add(tag_size);

                        if is_last {
                            break;
                        }
                    }

                    // Read and replay each data block.
                    pos = self.advance_pos(pos);
                    for fs_block in &block_positions {
                        let data_phys = self.journal_phys_block(pos)?;
                        let mut data_buf = vec![0u8; block_size];
                        self.reader.read_block(data_phys, &mut data_buf)?;

                        // Write to final filesystem location.
                        self.reader.write_block(*fs_block, &data_buf)?;
                        replayed = replayed.saturating_add(1);

                        pos = self.advance_pos(pos);
                    }
                }
                block_type::COMMIT => {
                    // Transaction committed — advance to next.
                    expected_seq = expected_seq.wrapping_add(1);
                    pos = self.advance_pos(pos);
                }
                block_type::REVOKE => {
                    // Skip revoke blocks (not yet implemented).
                    pos = self.advance_pos(pos);
                }
                _ => {
                    break; // Unknown block type — stop.
                }
            }
        }

        if replayed > 0 {
            crate::serial_println!(
                "[ext4-journal] Replayed {} blocks from journal",
                replayed
            );
            self.reader.flush()?;
        }

        // Update journal state.
        self.state.next_sequence = expected_seq;
        self.state.write_pos = pos;
        self.write_journal_superblock()?;

        Ok(replayed)
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

    /// Write a block header (magic + type + sequence).
    fn write_block_header(&self, buf: &mut [u8], blocktype: u32, sequence: u32) {
        if let Some(dest) = buf.get_mut(0..4) {
            dest.copy_from_slice(&JBD2_MAGIC.to_le_bytes());
        }
        if let Some(dest) = buf.get_mut(4..8) {
            dest.copy_from_slice(&blocktype.to_le_bytes());
        }
        if let Some(dest) = buf.get_mut(8..12) {
            dest.copy_from_slice(&sequence.to_le_bytes());
        }
    }

    /// Write the journal superblock back to disk.
    fn write_journal_superblock(&self) -> KernelResult<()> {
        let block_size = self.state.block_size as usize;
        let jsb_phys = self.journal_phys_block(0)?;

        // Read the existing superblock.
        let mut buf = vec![0u8; block_size];
        self.reader.read_block(jsb_phys, &mut buf)?;

        // Update sequence and start position.
        if let Some(dest) = buf.get_mut(0x18..0x1C) {
            dest.copy_from_slice(&self.state.next_sequence.to_le_bytes());
        }
        if let Some(dest) = buf.get_mut(0x1C..0x20) {
            dest.copy_from_slice(&self.state.write_pos.to_le_bytes());
        }

        self.reader.write_block(jsb_phys, &buf)?;
        self.reader.flush()
    }
}
