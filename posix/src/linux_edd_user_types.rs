//! `<linux/edd.h>` — BIOS Enhanced Disk Drive services userspace ABI.
//!
//! The kernel exposes EDD geometry data captured at boot under
//! `/sys/firmware/edd/intN/`. Userspace bootloaders (grub-mkconfig,
//! systemd-boot generators) read it to map BIOS disk numbers (0x80…)
//! to Linux block devices.

// ---------------------------------------------------------------------------
// EDD limits
// ---------------------------------------------------------------------------

/// Maximum number of EDD-described disks (BIOS allows 0x80..0x85).
pub const EDDMAXNR: u32 = 6;
/// Maximum size of the per-disk parameter table (INT13 fn 48h).
pub const EDD_DEVICE_PARAM_SIZE: u32 = 74;
/// Maximum signature blocks reported.
pub const EDD_MBR_SIG_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// EDD info "valid" flags
// ---------------------------------------------------------------------------

/// Fixed-disk extensions are valid (INT13 ext present).
pub const EDD_EXT_FIXED_DISK_ACCESS: u32 = 0x1;
/// Device-locking & ejection extensions valid.
pub const EDD_EXT_DEVICE_LOCKING_AND_EJECTING: u32 = 0x2;
/// Enhanced disk drive support valid.
pub const EDD_EXT_ENHANCED_DISK_DRIVE_SUPPORT: u32 = 0x4;
/// 64-bit extension valid (INT13 fn 48h with 64-bit LBA).
pub const EDD_EXT_64BIT_EXTENSIONS: u32 = 0x8;

// ---------------------------------------------------------------------------
// Interface types (legacy_max_head; encoded in info_flags)
// ---------------------------------------------------------------------------

/// Interface: ATA.
pub const EDD_INFO_USE_INT13_FN50: u32 = 0x0010;
/// DMA boundary errors transparent.
pub const EDD_INFO_DMA_BOUNDARY_ERRORS_TRANSPARENT: u32 = 0x0008;
/// Geometry valid.
pub const EDD_INFO_GEOMETRY_VALID: u32 = 0x0002;
/// Removable.
pub const EDD_INFO_REMOVABLE: u32 = 0x0004;
/// Write with verify supported.
pub const EDD_INFO_WRITE_VERIFY: u32 = 0x0040;
/// Media change notify supported.
pub const EDD_INFO_MEDIA_CHANGE_NOTIFICATION: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Magic numbers (host-bus types reported by EDD)
// ---------------------------------------------------------------------------

/// Magic for "ATA " host bus.
pub const EDD_HOST_BUS_ATA_MAGIC: u32 = 0x41_54_41_20;
/// Magic for "PCI " host bus.
pub const EDD_HOST_BUS_PCI_MAGIC: u32 = 0x50_43_49_20;

// ---------------------------------------------------------------------------
// Sysfs mountpoint
// ---------------------------------------------------------------------------

/// Path under which the kernel exposes EDD data.
pub const EDD_SYSFS_DIR: &str = "/sys/firmware/edd";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_limits() {
        // BIOS disk range 0x80..0x85 → 6 disks; param table is 74
        // bytes from INT13 fn 48h spec.
        assert_eq!(EDDMAXNR, 6);
        assert_eq!(EDD_DEVICE_PARAM_SIZE, 74);
        assert_eq!(EDD_MBR_SIG_MAX, 16);
    }

    #[test]
    fn test_ext_flags_pow2_distinct() {
        let f = [
            EDD_EXT_FIXED_DISK_ACCESS,
            EDD_EXT_DEVICE_LOCKING_AND_EJECTING,
            EDD_EXT_ENHANCED_DISK_DRIVE_SUPPORT,
            EDD_EXT_64BIT_EXTENSIONS,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_info_flags_distinct() {
        let f = [
            EDD_INFO_USE_INT13_FN50,
            EDD_INFO_DMA_BOUNDARY_ERRORS_TRANSPARENT,
            EDD_INFO_GEOMETRY_VALID,
            EDD_INFO_REMOVABLE,
            EDD_INFO_WRITE_VERIFY,
            EDD_INFO_MEDIA_CHANGE_NOTIFICATION,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_host_bus_magic_strings() {
        // The host-bus magic is the 4-char ASCII string seen big-endian.
        assert_eq!(EDD_HOST_BUS_ATA_MAGIC, u32::from_be_bytes(*b"ATA "));
        assert_eq!(EDD_HOST_BUS_PCI_MAGIC, u32::from_be_bytes(*b"PCI "));
    }

    #[test]
    fn test_sysfs_path() {
        assert_eq!(EDD_SYSFS_DIR, "/sys/firmware/edd");
    }
}
