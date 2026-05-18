//! `<linux/dm-ioctl.h>` — Device-mapper target type constants.
//!
//! DM targets are the building blocks of device-mapper tables.
//! Each target type implements a specific storage transformation
//! (linear mapping, striping, encryption, caching, etc.).

// ---------------------------------------------------------------------------
// Target type name constants (as byte slices for no_std)
// ---------------------------------------------------------------------------

/// Linear mapping (1:1 offset into underlying device).
pub const DM_TARGET_LINEAR: &[u8] = b"linear";
/// Striped mapping (RAID-0 across multiple devices).
pub const DM_TARGET_STRIPED: &[u8] = b"striped";
/// Mirror (RAID-1 synchronous copy).
pub const DM_TARGET_MIRROR: &[u8] = b"mirror";
/// Snapshot (CoW copy of a device).
pub const DM_TARGET_SNAPSHOT: &[u8] = b"snapshot";
/// Snapshot origin (the source device for snapshots).
pub const DM_TARGET_SNAPSHOT_ORIGIN: &[u8] = b"snapshot-origin";
/// Error target (returns I/O errors).
pub const DM_TARGET_ERROR: &[u8] = b"error";
/// Zero target (returns zeroes on read, discards writes).
pub const DM_TARGET_ZERO: &[u8] = b"zero";
/// Crypt target (dm-crypt: block-level encryption).
pub const DM_TARGET_CRYPT: &[u8] = b"crypt";
/// Thin provisioning pool.
pub const DM_TARGET_THIN_POOL: &[u8] = b"thin-pool";
/// Thin provisioned device.
pub const DM_TARGET_THIN: &[u8] = b"thin";
/// Cache target (dm-cache: SSD caching).
pub const DM_TARGET_CACHE: &[u8] = b"cache";
/// Writecache target.
pub const DM_TARGET_WRITECACHE: &[u8] = b"writecache";
/// Integrity target (dm-integrity: data integrity).
pub const DM_TARGET_INTEGRITY: &[u8] = b"integrity";
/// Delay target (adds I/O latency for testing).
pub const DM_TARGET_DELAY: &[u8] = b"delay";
/// Flakey target (simulates I/O failures for testing).
pub const DM_TARGET_FLAKEY: &[u8] = b"flakey";
/// Dust target (simulates bad sectors).
pub const DM_TARGET_DUST: &[u8] = b"dust";
/// RAID target (md-raid via device-mapper).
pub const DM_TARGET_RAID: &[u8] = b"raid";
/// Verity target (dm-verity: read-only integrity verification).
pub const DM_TARGET_VERITY: &[u8] = b"verity";

// ---------------------------------------------------------------------------
// dm-crypt cipher modes
// ---------------------------------------------------------------------------

/// AES-XTS cipher (standard for disk encryption).
pub const DM_CRYPT_AES_XTS: &[u8] = b"aes-xts-plain64";
/// AES-CBC cipher with ESSIV.
pub const DM_CRYPT_AES_CBC_ESSIV: &[u8] = b"aes-cbc-essiv:sha256";

// ---------------------------------------------------------------------------
// dm-crypt flags
// ---------------------------------------------------------------------------

/// Allow discards to pass through.
pub const DM_CRYPT_ALLOW_DISCARDS: u32 = 1 << 0;
/// Use same IV generation for reads and writes.
pub const DM_CRYPT_SAME_CPU_CRYPT: u32 = 1 << 1;
/// Submit I/O from a dedicated thread.
pub const DM_CRYPT_NO_OFFLOAD: u32 = 1 << 2;
/// No read workqueue (inline decrypt).
pub const DM_CRYPT_NO_READ_WORKQUEUE: u32 = 1 << 3;
/// No write workqueue (inline encrypt).
pub const DM_CRYPT_NO_WRITE_WORKQUEUE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_targets_distinct() {
        let targets: [&[u8]; 18] = [
            DM_TARGET_LINEAR, DM_TARGET_STRIPED, DM_TARGET_MIRROR,
            DM_TARGET_SNAPSHOT, DM_TARGET_SNAPSHOT_ORIGIN,
            DM_TARGET_ERROR, DM_TARGET_ZERO, DM_TARGET_CRYPT,
            DM_TARGET_THIN_POOL, DM_TARGET_THIN, DM_TARGET_CACHE,
            DM_TARGET_WRITECACHE, DM_TARGET_INTEGRITY,
            DM_TARGET_DELAY, DM_TARGET_FLAKEY, DM_TARGET_DUST,
            DM_TARGET_RAID, DM_TARGET_VERITY,
        ];
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                assert_ne!(targets[i], targets[j]);
            }
        }
    }

    #[test]
    fn test_cipher_modes_distinct() {
        assert_ne!(DM_CRYPT_AES_XTS, DM_CRYPT_AES_CBC_ESSIV);
    }

    #[test]
    fn test_crypt_flags_no_overlap() {
        let flags = [
            DM_CRYPT_ALLOW_DISCARDS, DM_CRYPT_SAME_CPU_CRYPT,
            DM_CRYPT_NO_OFFLOAD, DM_CRYPT_NO_READ_WORKQUEUE,
            DM_CRYPT_NO_WRITE_WORKQUEUE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_crypt_flags_power_of_two() {
        assert!(DM_CRYPT_ALLOW_DISCARDS.is_power_of_two());
        assert!(DM_CRYPT_SAME_CPU_CRYPT.is_power_of_two());
        assert!(DM_CRYPT_NO_OFFLOAD.is_power_of_two());
        assert!(DM_CRYPT_NO_READ_WORKQUEUE.is_power_of_two());
        assert!(DM_CRYPT_NO_WRITE_WORKQUEUE.is_power_of_two());
    }

    #[test]
    fn test_linear_name() {
        assert_eq!(DM_TARGET_LINEAR, b"linear");
    }
}
