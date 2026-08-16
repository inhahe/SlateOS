//! Where an NTFS volume's bytes come from.
//!
//! Every read the NTFS driver performs goes through [`SectorSource`] rather
//! than calling [`crate::fs::cache::read_sector`] directly. That indirection
//! exists for one reason: **the driver has to be testable without a disk.**
//!
//! The other read-only driver in this tree, `iso9660`, calls the block cache
//! by name from a dozen places, and the price is visible in its `self_test()`
//! — the on-disk half of the driver (volume descriptors, directory records,
//! extents) is only exercised when a real ISO happens to be mounted, which in
//! the boot test it is not, so that half runs untested on every boot. The
//! same shape here would be worse, because NTFS's on-disk structures are
//! where all the difficulty is: fixups, runlists, resident-vs-non-resident
//! attributes, B-tree indexes. A driver whose hard parts are only tested when
//! someone attaches a Windows partition is a driver whose hard parts are
//! never tested.
//!
//! With this trait, [`MemorySource`] lets `tests.rs` synthesise a complete,
//! byte-exact NTFS volume in RAM and drive the *entire* parser over it during
//! every boot. The device path is then a thin wrapper whose only job is to
//! call the block cache.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::blkdev::SECTOR_SIZE;
use crate::error::{KernelError, KernelResult};

/// A source of 512-byte sectors backing an NTFS volume.
///
/// Implementations must treat `lba` as an absolute sector index from the
/// start of the volume (the NTFS boot sector is LBA 0).
pub trait SectorSource: Send + Sync {
    /// Read `buf.len() / SECTOR_SIZE` consecutive sectors starting at `lba`.
    ///
    /// `buf.len()` must be a non-zero multiple of [`SECTOR_SIZE`]; an
    /// implementation may return [`KernelError::InvalidArgument`] otherwise.
    /// On success the whole buffer is filled; on error its contents are
    /// unspecified.
    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> KernelResult<()>;

    /// A human-readable name for diagnostics (a device name, or `"<memory>"`).
    fn source_name(&self) -> &str;
}

/// Read a byte range that need not be sector-aligned.
///
/// NTFS asks for byte ranges constantly — an MFT record is 1024 bytes at an
/// arbitrary offset into the `$MFT` data stream, an index entry straddles
/// whatever it straddles — so every caller would otherwise open-code the same
/// round-down/round-up dance. Doing it once here also means the bounds
/// arithmetic is checked in one place instead of a dozen.
pub fn read_bytes(src: &dyn SectorSource, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }

    let sector_size = SECTOR_SIZE as u64;
    let first_lba = offset
        .checked_div(sector_size)
        .ok_or(KernelError::InvalidArgument)?;
    let skip = offset
        .checked_rem(sector_size)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(KernelError::InvalidArgument)?;

    let span = skip.checked_add(len).ok_or(KernelError::InvalidArgument)?;
    // Round up to a whole number of sectors.
    let sectors = span
        .checked_add(SECTOR_SIZE.saturating_sub(1))
        .ok_or(KernelError::InvalidArgument)?
        / SECTOR_SIZE;
    let alloc = sectors
        .checked_mul(SECTOR_SIZE)
        .ok_or(KernelError::InvalidArgument)?;

    let mut raw = vec![0u8; alloc];
    src.read_sectors(first_lba, &mut raw)?;

    raw.get(skip..span)
        .map(<[u8]>::to_vec)
        .ok_or(KernelError::InternalError)
}

/// A [`SectorSource`] backed by a real block device, through the block cache.
pub struct DeviceSource {
    device: String,
}

impl DeviceSource {
    /// Wrap a named block device.
    pub fn new(device: &str) -> Self {
        Self {
            device: String::from(device),
        }
    }
}

impl SectorSource for DeviceSource {
    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> KernelResult<()> {
        if buf.is_empty() || !buf.len().is_multiple_of(SECTOR_SIZE) {
            return Err(KernelError::InvalidArgument);
        }

        let mut sector = [0u8; SECTOR_SIZE];
        for (i, chunk) in buf.chunks_mut(SECTOR_SIZE).enumerate() {
            let this_lba = lba
                .checked_add(i as u64)
                .ok_or(KernelError::InvalidArgument)?;
            crate::fs::cache::read_sector(&self.device, this_lba, &mut sector)?;
            chunk.copy_from_slice(&sector);
        }
        Ok(())
    }

    fn source_name(&self) -> &str {
        &self.device
    }
}

/// A [`SectorSource`] backed by an in-RAM image.
///
/// Used by the self-test to drive the parser over a synthetic volume. Reads
/// past the end of the image are an error rather than zero-fill: a parser bug
/// that walks off the end of the volume should surface as a failure, not as a
/// plausible-looking run of zeroes.
pub struct MemorySource {
    image: Vec<u8>,
}

impl MemorySource {
    /// Wrap an in-memory volume image.
    pub fn new(image: Vec<u8>) -> Self {
        Self { image }
    }

    /// The image's size in bytes.
    pub fn len(&self) -> usize {
        self.image.len()
    }

    /// Whether the image is empty.
    pub fn is_empty(&self) -> bool {
        self.image.is_empty()
    }
}

impl SectorSource for MemorySource {
    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> KernelResult<()> {
        if buf.is_empty() || !buf.len().is_multiple_of(SECTOR_SIZE) {
            return Err(KernelError::InvalidArgument);
        }

        let start = lba
            .checked_mul(SECTOR_SIZE as u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(KernelError::InvalidArgument)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(KernelError::InvalidArgument)?;

        let slice = self.image.get(start..end).ok_or(KernelError::IoError)?;
        buf.copy_from_slice(slice);
        Ok(())
    }

    // The trait fixes the signature as `&self -> &str`; narrowing this one
    // impl to `&'static str` would no longer implement the trait.
    #[allow(clippy::unnecessary_literal_bound)]
    fn source_name(&self) -> &str {
        "<memory>"
    }
}
