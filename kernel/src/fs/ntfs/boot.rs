//! The NTFS boot sector (`$Boot`, LBA 0).
//!
//! Everything else in the volume is located relative to the geometry this
//! sector declares, so it is also the volume's only unvalidatable input: a
//! wrong `bytes_per_sector` here does not produce a wrong answer later, it
//! produces reads at wrong addresses. Hence the validation below is
//! deliberately strict — a field that is out of range is a rejected volume,
//! not a clamped one.
//!
//! ## Layout (offsets into the 512-byte sector)
//!
//! ```text
//! 0x00  3   jump instruction
//! 0x03  8   OEM ID — "NTFS    "
//! 0x0B  2   bytes per sector
//! 0x0D  1   sectors per cluster (see below)
//! 0x0E  2   reserved sectors (0 on NTFS)
//! 0x15  1   media descriptor
//! 0x18  2   sectors per track
//! 0x1A  2   number of heads
//! 0x28  8   total sectors in the volume
//! 0x30  8   cluster of the $MFT
//! 0x38  8   cluster of the $MFTMirr
//! 0x40  1   clusters per MFT record (signed, see below)
//! 0x44  1   clusters per index buffer (signed, see below)
//! 0x48  8   volume serial number
//! 0x1FE 2   0xAA55 signature
//! ```
//!
//! ## The signed size fields
//!
//! `clusters_per_mft_record` and `clusters_per_index_buffer` are `i8`. A
//! positive value is a count of clusters. A *negative* value `v` means the
//! structure is `2^(-v)` **bytes**, which is how NTFS expresses a record
//! smaller than one cluster — the usual case, since MFT records are 1024
//! bytes and clusters are typically 4096. Reading these as unsigned is the
//! classic NTFS parser bug: `-10` reads as `246`, and the driver then tries
//! to read a one-megabyte MFT record.
//!
//! ## References
//!
//! - <https://flatcap.github.io/linux-ntfs/ntfs/files/boot.html>
//! - Linux `fs/ntfs3/ntfs.h`, `fs/ntfs3/super.c`

use crate::error::{KernelError, KernelResult};

use super::raw::{i8_at, u8_at, u16_at, u64_at};

/// OEM identifier every NTFS volume carries at offset 3.
pub const NTFS_OEM_ID: &[u8; 8] = b"NTFS    ";

/// Offset of the OEM identifier.
pub const OEM_ID_OFFSET: usize = 0x03;

/// Largest sector size we accept. 4096 covers 4Kn drives; anything larger
/// is not a real disk and is far more likely to be a corrupt field.
const MAX_BYTES_PER_SECTOR: u32 = 4096;

/// Largest cluster size we accept (2 MiB — the NTFS maximum since Windows
/// 10 1709; older volumes top out at 64 KiB).
const MAX_CLUSTER_SIZE: u32 = 2 * 1024 * 1024;

/// Largest MFT record / index buffer we accept.
const MAX_RECORD_SIZE: u32 = 1024 * 1024;

/// Parsed NTFS boot sector: the volume's geometry.
#[derive(Debug, Clone, Copy)]
pub struct BootSector {
    /// Bytes per sector, as declared by the volume (not necessarily the
    /// device's own sector size).
    pub bytes_per_sector: u32,
    /// Sectors per cluster.
    pub sectors_per_cluster: u32,
    /// Cluster size in bytes.
    pub cluster_size: u32,
    /// Total sectors in the volume.
    pub total_sectors: u64,
    /// Logical cluster number of the `$MFT`.
    pub mft_lcn: u64,
    /// Logical cluster number of the `$MFTMirr` (the first four MFT records,
    /// mirrored for recovery).
    pub mftmirr_lcn: u64,
    /// Size of one MFT record in bytes.
    pub mft_record_size: u32,
    /// Size of one index allocation buffer in bytes.
    pub index_record_size: u32,
    /// Volume serial number.
    pub serial: u64,
}

impl BootSector {
    /// Parse and validate a boot sector.
    ///
    /// # Errors
    ///
    /// [`KernelError::InvalidArgument`] if `data` is shorter than a sector,
    /// [`KernelError::NotSupported`] if the OEM ID is not NTFS, and
    /// [`KernelError::CorruptedData`] if a geometry field is out of range.
    pub fn parse(data: &[u8]) -> KernelResult<Self> {
        if data.len() < 512 {
            return Err(KernelError::InvalidArgument);
        }

        let oem = data
            .get(OEM_ID_OFFSET..OEM_ID_OFFSET.saturating_add(8))
            .ok_or(KernelError::InvalidArgument)?;
        if oem != NTFS_OEM_ID.as_slice() {
            return Err(KernelError::NotSupported);
        }

        let bytes_per_sector = u32::from(u16_at(data, 0x0B).ok_or(KernelError::CorruptedData)?);
        // Must be a power of two in [512, MAX]. NTFS itself requires 512
        // minimum; a non-power-of-two would break every offset computation
        // downstream, so reject rather than round.
        if bytes_per_sector < 512
            || bytes_per_sector > MAX_BYTES_PER_SECTOR
            || !bytes_per_sector.is_power_of_two()
        {
            return Err(KernelError::CorruptedData);
        }

        let sectors_per_cluster =
            decode_sectors_per_cluster(u8_at(data, 0x0D).ok_or(KernelError::CorruptedData)?)?;

        let cluster_size = bytes_per_sector
            .checked_mul(sectors_per_cluster)
            .ok_or(KernelError::CorruptedData)?;
        if cluster_size > MAX_CLUSTER_SIZE {
            return Err(KernelError::CorruptedData);
        }

        let total_sectors = u64_at(data, 0x28).ok_or(KernelError::CorruptedData)?;
        let mft_lcn = u64_at(data, 0x30).ok_or(KernelError::CorruptedData)?;
        let mftmirr_lcn = u64_at(data, 0x38).ok_or(KernelError::CorruptedData)?;

        let mft_record_size = decode_sized_field(
            i8_at(data, 0x40).ok_or(KernelError::CorruptedData)?,
            cluster_size,
        )?;
        let index_record_size = decode_sized_field(
            i8_at(data, 0x44).ok_or(KernelError::CorruptedData)?,
            cluster_size,
        )?;

        // An MFT record must hold at least a record header plus a terminator,
        // and must be a multiple of the sector size because fixups are applied
        // per sector.
        if mft_record_size < bytes_per_sector || !mft_record_size.is_multiple_of(bytes_per_sector) {
            return Err(KernelError::CorruptedData);
        }
        if index_record_size < bytes_per_sector
            || !index_record_size.is_multiple_of(bytes_per_sector)
        {
            return Err(KernelError::CorruptedData);
        }

        // The MFT must live inside the volume.
        let mft_first_sector = mft_lcn
            .checked_mul(u64::from(sectors_per_cluster))
            .ok_or(KernelError::CorruptedData)?;
        if total_sectors == 0 || mft_first_sector >= total_sectors {
            return Err(KernelError::CorruptedData);
        }

        let serial = u64_at(data, 0x48).ok_or(KernelError::CorruptedData)?;

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            cluster_size,
            total_sectors,
            mft_lcn,
            mftmirr_lcn,
            mft_record_size,
            index_record_size,
            serial,
        })
    }

    /// Byte offset of a logical cluster from the start of the volume.
    pub fn cluster_offset(&self, lcn: u64) -> KernelResult<u64> {
        lcn.checked_mul(u64::from(self.cluster_size))
            .ok_or(KernelError::CorruptedData)
    }

    /// Total volume size in bytes.
    pub fn volume_bytes(&self) -> u64 {
        self.total_sectors
            .saturating_mul(u64::from(self.bytes_per_sector))
    }
}

/// Decode the `sectors_per_cluster` byte at offset 0x0D.
///
/// Values above 0x80 use the same power-of-two encoding as the signed size
/// fields (Windows 10 1709 added this to allow clusters above 64 KiB), so
/// `0xF4` (= -12 as `i8`) means `2^12` sectors per cluster.
fn decode_sectors_per_cluster(raw: u8) -> KernelResult<u32> {
    if raw == 0 {
        return Err(KernelError::CorruptedData);
    }
    if raw <= 0x80 {
        let v = u32::from(raw);
        if !v.is_power_of_two() {
            return Err(KernelError::CorruptedData);
        }
        return Ok(v);
    }
    // 0x81..=0xFF: 2^(256 - raw) sectors.
    let shift = 256u32.saturating_sub(u32::from(raw));
    if shift >= 32 {
        return Err(KernelError::CorruptedData);
    }
    1u32.checked_shl(shift).ok_or(KernelError::CorruptedData)
}

/// Decode a signed cluster-or-power-of-two size field into bytes.
///
/// See the module docs: positive is a cluster count, negative `v` is
/// `2^(-v)` bytes.
fn decode_sized_field(raw: i8, cluster_size: u32) -> KernelResult<u32> {
    if raw > 0 {
        let clusters = u32::try_from(raw).map_err(|_| KernelError::CorruptedData)?;
        let size = cluster_size
            .checked_mul(clusters)
            .ok_or(KernelError::CorruptedData)?;
        if size > MAX_RECORD_SIZE {
            return Err(KernelError::CorruptedData);
        }
        return Ok(size);
    }

    // `raw <= 0`. A shift of 0 (raw == 0) would mean a 1-byte record, which
    // is rejected by the caller's sector-multiple check; reject it here too
    // so the arithmetic below never produces a nonsense size.
    let shift = raw.checked_neg().ok_or(KernelError::CorruptedData)?;
    let shift = u32::try_from(shift).map_err(|_| KernelError::CorruptedData)?;
    if shift == 0 || shift >= 32 {
        return Err(KernelError::CorruptedData);
    }
    let size = 1u32.checked_shl(shift).ok_or(KernelError::CorruptedData)?;
    if size > MAX_RECORD_SIZE {
        return Err(KernelError::CorruptedData);
    }
    Ok(size)
}

/// Whether a buffer looks like an NTFS boot sector.
///
/// Checks only the OEM ID, deliberately: `probe` is called on every device to
/// pick a driver, and a volume whose geometry is corrupt should still be
/// *recognised* as NTFS so the mount fails with a filesystem-specific error
/// rather than being silently handed to the next driver in the list.
pub fn looks_like_ntfs(data: &[u8]) -> bool {
    data.get(OEM_ID_OFFSET..OEM_ID_OFFSET.saturating_add(8)) == Some(NTFS_OEM_ID.as_slice())
}
