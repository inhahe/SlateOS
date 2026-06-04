//! `<linux/cper.h>` — UEFI Common Platform Error Record format.
//!
//! CPER is the binary record format firmware uses to report hardware
//! errors (machine checks, PCIe AER events, memory errors, etc.) to
//! the OS. Records are exposed via /sys/firmware/efi/efivars and
//! parsed by the EDAC and APEI subsystems.

// ---------------------------------------------------------------------------
// CPER record header signature
// ---------------------------------------------------------------------------

/// "CPER" magic at the start of each record header.
pub const CPER_SIG_RECORD: &[u8; 4] = b"CPER";
/// "PE" placed at the *end* of a record (signature_end).
pub const CPER_SIG_END: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Record header revision
// ---------------------------------------------------------------------------

pub const CPER_RECORD_REV_MAJOR: u8 = 0x01;
pub const CPER_RECORD_REV_MINOR: u8 = 0x02;
pub const CPER_RECORD_REV: u16 =
    ((CPER_RECORD_REV_MAJOR as u16) << 8) | (CPER_RECORD_REV_MINOR as u16);

// ---------------------------------------------------------------------------
// Error severity values (header.error_severity)
// ---------------------------------------------------------------------------

pub const CPER_SEV_RECOVERABLE: u32 = 0;
pub const CPER_SEV_FATAL: u32 = 1;
pub const CPER_SEV_CORRECTED: u32 = 2;
pub const CPER_SEV_INFORMATIONAL: u32 = 3;

// ---------------------------------------------------------------------------
// Record header validation bits
// ---------------------------------------------------------------------------

pub const CPER_VALID_PLATFORM_ID: u32 = 1 << 0;
pub const CPER_VALID_TIMESTAMP: u32 = 1 << 1;
pub const CPER_VALID_PARTITION_ID: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Section descriptor flags
// ---------------------------------------------------------------------------

pub const CPER_SEC_PRIMARY: u32 = 1 << 0;
pub const CPER_SEC_CONTAINMENT_WARNING: u32 = 1 << 1;
pub const CPER_SEC_RESET: u32 = 1 << 2;
pub const CPER_SEC_ERROR_THRESHOLD_EXCEEDED: u32 = 1 << 3;
pub const CPER_SEC_RESOURCE_NOT_ACCESSIBLE: u32 = 1 << 4;
pub const CPER_SEC_LATENT_ERROR: u32 = 1 << 5;
pub const CPER_SEC_PROPAGATED: u32 = 1 << 6;
pub const CPER_SEC_OVERFLOW: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Header field sizes (struct cper_record_header)
// ---------------------------------------------------------------------------

pub const CPER_RECORD_HEADER_SIZE: usize = 128;
pub const CPER_SECTION_DESCRIPTOR_SIZE: usize = 72;
/// Maximum reasonable record (Linux clamp).
pub const CPER_MAX_RECORD_BYTES: usize = 16 * 1024;

// ---------------------------------------------------------------------------
// Notification types ("notification_type" field; first 4 bytes of common GUIDs)
// ---------------------------------------------------------------------------

/// CMC (corrected machine check) — first 4 bytes of the GUID.
pub const CPER_NOTIFY_CMC_GUID_PREFIX: u32 = 0x2DCE_8BB1;
/// MCE (uncorrectable machine check).
pub const CPER_NOTIFY_MCE_GUID_PREFIX: u32 = 0xE8F56FFE;
/// PCIE.
pub const CPER_NOTIFY_PCIE_GUID_PREFIX: u32 = 0xCF93C01F;

// ---------------------------------------------------------------------------
// sysfs / pstore paths exposing CPER records
// ---------------------------------------------------------------------------

pub const CPER_SYSFS_EFIVARS: &str = "/sys/firmware/efi/efivars";
pub const CPER_PSTORE_MOUNT: &str = "/sys/fs/pstore";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_is_ascii_cper() {
        assert_eq!(CPER_SIG_RECORD, b"CPER");
    }

    #[test]
    fn test_sig_end_is_all_ones() {
        assert_eq!(CPER_SIG_END, 0xFFFF_FFFF);
    }

    #[test]
    fn test_revision_pack_2_1() {
        assert_eq!(CPER_RECORD_REV_MAJOR, 1);
        assert_eq!(CPER_RECORD_REV_MINOR, 2);
        assert_eq!(CPER_RECORD_REV, 0x0102);
    }

    #[test]
    fn test_severity_dense_0_to_3() {
        let s = [
            CPER_SEV_RECOVERABLE,
            CPER_SEV_FATAL,
            CPER_SEV_CORRECTED,
            CPER_SEV_INFORMATIONAL,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_valid_bits_single() {
        for b in [CPER_VALID_PLATFORM_ID, CPER_VALID_TIMESTAMP, CPER_VALID_PARTITION_ID] {
            assert!(b.is_power_of_two());
        }
        assert_eq!(
            CPER_VALID_PLATFORM_ID | CPER_VALID_TIMESTAMP | CPER_VALID_PARTITION_ID,
            0x07
        );
    }

    #[test]
    fn test_section_flags_distinct_single_bit() {
        let f = [
            CPER_SEC_PRIMARY,
            CPER_SEC_CONTAINMENT_WARNING,
            CPER_SEC_RESET,
            CPER_SEC_ERROR_THRESHOLD_EXCEEDED,
            CPER_SEC_RESOURCE_NOT_ACCESSIBLE,
            CPER_SEC_LATENT_ERROR,
            CPER_SEC_PROPAGATED,
            CPER_SEC_OVERFLOW,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        let or = f.iter().copied().fold(0u32, |a, b| a | b);
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_record_header_sizes_match_spec() {
        assert_eq!(CPER_RECORD_HEADER_SIZE, 128);
        assert_eq!(CPER_SECTION_DESCRIPTOR_SIZE, 72);
        // Header + at least one section descriptor must fit in 16 KiB cap.
        assert!(CPER_RECORD_HEADER_SIZE + CPER_SECTION_DESCRIPTOR_SIZE < CPER_MAX_RECORD_BYTES);
    }

    #[test]
    fn test_notification_guid_prefixes_distinct() {
        let g = [
            CPER_NOTIFY_CMC_GUID_PREFIX,
            CPER_NOTIFY_MCE_GUID_PREFIX,
            CPER_NOTIFY_PCIE_GUID_PREFIX,
        ];
        for (i, &x) in g.iter().enumerate() {
            for &y in &g[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_sysfs_paths_well_formed() {
        assert!(CPER_SYSFS_EFIVARS.starts_with("/sys/firmware/efi/"));
        assert!(CPER_PSTORE_MOUNT.starts_with("/sys/fs/"));
    }
}
