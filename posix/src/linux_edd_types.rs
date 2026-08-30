//! `<linux/edd.h>` — Enhanced Disk Drive BIOS interface constants.
//!
//! Constants reported by the EDD (Enhanced Disk Drive) BIOS interface
//! that x86 firmware uses to describe boot disk geometry, host bus,
//! and interface type to the kernel.

// ---------------------------------------------------------------------------
// EDD signature & sysfs limits
// ---------------------------------------------------------------------------

/// Signature in the EDD info block ("EDDV").
pub const EDD_MBR_SIG_MAX: u32 = 16;
/// Maximum number of EDD-described disks.
pub const EDDMAXNR: u32 = 6;
/// Magic value for "no EDD info available".
pub const EDD_INFO_DMA_BOUNDARY_ERRORS: u32 = 0x0004;

// ---------------------------------------------------------------------------
// EDD info flags (edd_info.params.info_flags)
// ---------------------------------------------------------------------------

/// DMA boundary errors transparent.
pub const EDD_INFO_DMA_BOUNDARY_ERRORS_TRANSPARENT: u32 = 0x0001;
/// Geometry valid.
pub const EDD_INFO_GEOMETRY_VALID: u32 = 0x0002;
/// Removable.
pub const EDD_INFO_REMOVABLE: u32 = 0x0008;
/// Write verify supported.
pub const EDD_INFO_WRITE_VERIFY: u32 = 0x0010;
/// Media change notification.
pub const EDD_INFO_MEDIA_CHANGE_NOTIFICATION: u32 = 0x0020;
/// Lockable.
pub const EDD_INFO_LOCKABLE: u32 = 0x0040;
/// No media present.
pub const EDD_INFO_NO_MEDIA_PRESENT: u32 = 0x0080;
/// Use interrupt 13h extensions.
pub const EDD_INFO_USE_INT13_FN50: u32 = 0x0100;

// ---------------------------------------------------------------------------
// EDD interface versions (edd_info.version)
// ---------------------------------------------------------------------------

/// EDD spec version 1.0.
pub const EDD_VER_NONE: u32 = 0x00;
/// EDD 1.0.
pub const EDD_VER_1_0: u32 = 0x01;
/// EDD 1.1.
pub const EDD_VER_1_1: u32 = 0x10;
/// EDD 3.0.
pub const EDD_VER_3_0: u32 = 0x30;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_limit_sane() {
        assert!(EDD_MBR_SIG_MAX > 0);
        assert!(EDDMAXNR > 0);
    }

    #[test]
    fn test_info_flags_distinct() {
        let flags = [
            EDD_INFO_DMA_BOUNDARY_ERRORS_TRANSPARENT,
            EDD_INFO_GEOMETRY_VALID,
            EDD_INFO_DMA_BOUNDARY_ERRORS,
            EDD_INFO_REMOVABLE,
            EDD_INFO_WRITE_VERIFY,
            EDD_INFO_MEDIA_CHANGE_NOTIFICATION,
            EDD_INFO_LOCKABLE,
            EDD_INFO_NO_MEDIA_PRESENT,
            EDD_INFO_USE_INT13_FN50,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_info_flags_single_bit() {
        let single = [
            EDD_INFO_DMA_BOUNDARY_ERRORS_TRANSPARENT,
            EDD_INFO_GEOMETRY_VALID,
            EDD_INFO_DMA_BOUNDARY_ERRORS,
            EDD_INFO_REMOVABLE,
            EDD_INFO_WRITE_VERIFY,
            EDD_INFO_MEDIA_CHANGE_NOTIFICATION,
            EDD_INFO_LOCKABLE,
            EDD_INFO_NO_MEDIA_PRESENT,
            EDD_INFO_USE_INT13_FN50,
        ];
        for &f in &single {
            assert!(f.is_power_of_two(), "flag {f:#x} is not a single bit");
        }
    }

    #[test]
    fn test_versions_distinct() {
        let versions = [EDD_VER_NONE, EDD_VER_1_0, EDD_VER_1_1, EDD_VER_3_0];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }
}
