//! Block I/O helpers for ext4.
//!
//! Reads ext4 blocks from the underlying block device through the buffer
//! cache.  An ext4 "block" is typically 4096 bytes (8 x 512-byte sectors).
//! This module abstracts that translation.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::blkdev::SECTOR_SIZE;
use crate::error::{KernelError, KernelResult};

/// Block reader tied to a specific device and block size.
///
/// Wraps the buffer cache with ext4 block-level addressing.
pub struct BlockReader {
    /// Block device name (e.g., "vda").
    device: String,
    /// ext4 block size in bytes (1024, 2048, 4096, ...).
    block_size: u32,
    /// Sectors per ext4 block.
    sectors_per_block: u32,
}

impl BlockReader {
    /// Create a new block reader for the given device and block size.
    pub fn new(device: &str, block_size: u32) -> KernelResult<Self> {
        if block_size == 0 || !block_size.is_power_of_two() {
            return Err(KernelError::InvalidArgument);
        }
        let sector_size = SECTOR_SIZE as u32;
        if block_size < sector_size {
            return Err(KernelError::InvalidArgument);
        }
        let sectors_per_block = block_size / sector_size;

        Ok(Self {
            device: String::from(device),
            block_size,
            sectors_per_block,
        })
    }

    /// Read a single ext4 block into `buf`.
    ///
    /// `buf` must be at least `block_size` bytes.
    pub fn read_block(&self, block_nr: u64, buf: &mut [u8]) -> KernelResult<()> {
        let bs = self.block_size as usize;
        if buf.len() < bs {
            return Err(KernelError::InvalidArgument);
        }

        let start_lba = block_nr.saturating_mul(u64::from(self.sectors_per_block));
        let mut sector_buf = [0u8; SECTOR_SIZE];

        for i in 0..self.sectors_per_block {
            let lba = start_lba.saturating_add(u64::from(i));
            crate::fs::cache::read_sector(&self.device, lba, &mut sector_buf)?;

            let offset = (i as usize).saturating_mul(SECTOR_SIZE);
            if let Some(dest) = buf.get_mut(offset..offset.saturating_add(SECTOR_SIZE)) {
                dest.copy_from_slice(&sector_buf);
            }
        }

        Ok(())
    }

    /// Read a range of bytes from the device at an absolute byte offset.
    ///
    /// This is useful for reading structures that don't align to block
    /// boundaries (e.g., the superblock at byte offset 1024).
    pub fn read_bytes(&self, byte_offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }

        let sector_size = SECTOR_SIZE as u64;
        let start_lba = byte_offset / sector_size;
        let offset_in_sector = (byte_offset % sector_size) as usize;

        // Calculate how many sectors we need to read.
        let total_bytes_from_sector_start = offset_in_sector.saturating_add(len);
        let sectors_needed = total_bytes_from_sector_start
            .saturating_add(SECTOR_SIZE)
            .saturating_sub(1)
            / SECTOR_SIZE;

        let mut raw = vec![0u8; sectors_needed.saturating_mul(SECTOR_SIZE)];
        let mut sector_buf = [0u8; SECTOR_SIZE];

        for i in 0..sectors_needed {
            let lba = start_lba.saturating_add(i as u64);
            crate::fs::cache::read_sector(&self.device, lba, &mut sector_buf)?;

            let offset = i.saturating_mul(SECTOR_SIZE);
            if let Some(dest) = raw.get_mut(offset..offset.saturating_add(SECTOR_SIZE)) {
                dest.copy_from_slice(&sector_buf);
            }
        }

        // Extract the requested range.
        let end = offset_in_sector.saturating_add(len);
        raw.get(offset_in_sector..end)
            .map(|s| s.to_vec())
            .ok_or(KernelError::IoError)
    }

    /// Write a single ext4 block from `buf`.
    ///
    /// `buf` must be at least `block_size` bytes.
    /// Writes go through the buffer cache (write-back).
    pub fn write_block(&self, block_nr: u64, buf: &[u8]) -> KernelResult<()> {
        let bs = self.block_size as usize;
        if buf.len() < bs {
            return Err(KernelError::InvalidArgument);
        }

        let start_lba = block_nr.saturating_mul(u64::from(self.sectors_per_block));

        for i in 0..self.sectors_per_block {
            let lba = start_lba.saturating_add(u64::from(i));
            let offset = (i as usize).saturating_mul(SECTOR_SIZE);
            let end = offset.saturating_add(SECTOR_SIZE);
            let src = buf.get(offset..end).ok_or(KernelError::IoError)?;

            let mut sector_buf = [0u8; SECTOR_SIZE];
            sector_buf.copy_from_slice(src);
            crate::fs::cache::write_sector(&self.device, lba, &sector_buf)?;
        }

        Ok(())
    }

    /// Write a range of bytes to the device at an absolute byte offset.
    ///
    /// Uses read-modify-write for sectors that are partially overwritten.
    /// Writes go through the buffer cache (write-back).
    pub fn write_bytes(&self, byte_offset: u64, data: &[u8]) -> KernelResult<()> {
        if data.is_empty() {
            return Ok(());
        }

        let sector_size = SECTOR_SIZE as u64;
        let start_lba = byte_offset / sector_size;
        let offset_in_sector = (byte_offset % sector_size) as usize;

        let mut remaining = data;
        let mut lba = start_lba;
        let mut pos_in_sector = offset_in_sector;

        while !remaining.is_empty() {
            let mut sector_buf = [0u8; SECTOR_SIZE];

            // If we're writing a partial sector, read the existing content first.
            let write_start = pos_in_sector;
            let write_len = remaining.len().min(SECTOR_SIZE.saturating_sub(pos_in_sector));

            if write_start > 0 || write_len < SECTOR_SIZE {
                // Partial sector — read-modify-write.
                crate::fs::cache::read_sector(&self.device, lba, &mut sector_buf)?;
            }

            if let (Some(dest), Some(src)) = (
                sector_buf.get_mut(write_start..write_start.saturating_add(write_len)),
                remaining.get(..write_len),
            ) {
                dest.copy_from_slice(src);
            }

            crate::fs::cache::write_sector(&self.device, lba, &sector_buf)?;

            remaining = remaining.get(write_len..).unwrap_or(&[]);
            lba = lba.saturating_add(1);
            pos_in_sector = 0; // Subsequent sectors start at offset 0.
        }

        Ok(())
    }

    /// Flush all cached writes for this device to disk.
    pub fn flush(&self) -> KernelResult<()> {
        crate::fs::cache::flush(&self.device)
    }

    /// The ext4 block size in bytes.
    #[must_use]
    #[allow(dead_code)]
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// The device name.
    #[must_use]
    #[allow(dead_code)]
    pub fn device(&self) -> &str {
        &self.device
    }
}
