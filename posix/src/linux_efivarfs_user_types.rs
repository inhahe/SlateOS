//! `<linux/efivarfs.h>` / `<linux/efi.h>` — EFI variable filesystem ABI.
//!
//! Tools like `efibootmgr`, `efivar`, and systemd's bootctl interact
//! with EFI NVRAM by reading and writing `/sys/firmware/efi/efivars/<name>`.
//! The attribute prefix at the start of each value is what makes the
//! file system writable/persistent semantics work.

// ---------------------------------------------------------------------------
// Filesystem magic
// ---------------------------------------------------------------------------

/// `EFIVARFS_MAGIC` — statfs f_type for efivarfs.
pub const EFIVARFS_MAGIC: u64 = 0xde5e_81e4;

// ---------------------------------------------------------------------------
// Mount points
// ---------------------------------------------------------------------------

/// Standard mount point.
pub const EFIVARFS_MOUNT: &str = "/sys/firmware/efi/efivars";

// ---------------------------------------------------------------------------
// Variable attributes (prepended to data)
// ---------------------------------------------------------------------------

/// Variable persists across reboots.
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 0x0000_0001;
/// Variable is accessible in boot services.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 0x0000_0002;
/// Variable is accessible at runtime.
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 0x0000_0004;
/// Variable holds a hardware-error record.
pub const EFI_VARIABLE_HARDWARE_ERROR_RECORD: u32 = 0x0000_0008;
/// Variable is authenticated with an AuthInfo header.
pub const EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS: u32 = 0x0000_0010;
/// Variable uses time-based authenticated writes (Secure Boot DBs).
pub const EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS: u32 = 0x0000_0020;
/// Variable is to be appended on write.
pub const EFI_VARIABLE_APPEND_WRITE: u32 = 0x0000_0040;
/// Enhanced authenticated access (UEFI 2.7+).
pub const EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS: u32 = 0x0000_0080;

/// Mask of all defined attribute bits.
pub const EFI_VARIABLE_MASK: u32 = EFI_VARIABLE_NON_VOLATILE
    | EFI_VARIABLE_BOOTSERVICE_ACCESS
    | EFI_VARIABLE_RUNTIME_ACCESS
    | EFI_VARIABLE_HARDWARE_ERROR_RECORD
    | EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_APPEND_WRITE
    | EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS;

// ---------------------------------------------------------------------------
// Size limits
// ---------------------------------------------------------------------------

/// Max variable name length (16-bit UCS-2 chars).
pub const EFI_VAR_NAME_LEN: usize = 1024;
/// Standard GUID byte length.
pub const EFI_GUID_LEN: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        // efivarfs magic is documented in the kernel as 0xde5e81e4.
        assert_eq!(EFIVARFS_MAGIC, 0xde5e_81e4);
    }

    #[test]
    fn test_mount_path() {
        assert_eq!(EFIVARFS_MOUNT, "/sys/firmware/efi/efivars");
    }

    #[test]
    fn test_attribute_bits_pow2_distinct() {
        let a = [
            EFI_VARIABLE_NON_VOLATILE,
            EFI_VARIABLE_BOOTSERVICE_ACCESS,
            EFI_VARIABLE_RUNTIME_ACCESS,
            EFI_VARIABLE_HARDWARE_ERROR_RECORD,
            EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS,
            EFI_VARIABLE_APPEND_WRITE,
            EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS,
        ];
        for &b in &a {
            assert!(b.is_power_of_two());
        }
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        // The mask must equal the OR of all defined bits.
        let or = a.iter().fold(0u32, |x, &y| x | y);
        assert_eq!(EFI_VARIABLE_MASK, or);
    }

    #[test]
    fn test_size_limits() {
        // EFI names are UCS-2; 1024 is the kernel's per-call limit.
        assert_eq!(EFI_VAR_NAME_LEN, 1024);
        // EFI_GUID is exactly 16 bytes on every UEFI revision.
        assert_eq!(EFI_GUID_LEN, 16);
    }
}
