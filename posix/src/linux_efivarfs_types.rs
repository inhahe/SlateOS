//! `<linux/efi.h>` — efivarfs (`/sys/firmware/efi/efivars/`) constants.
//!
//! efivarfs exposes UEFI runtime variables as files. The first four
//! bytes of every variable are the attribute bitmask; the
//! `FS_IOC_*FLAGS` ioctls flip the immutable bit. efibootmgr,
//! systemd-boot, and shim use the constants below.

// ---------------------------------------------------------------------------
// efivarfs magic and special files
// ---------------------------------------------------------------------------

/// `statfs(2)` magic for an efivarfs mount.
pub const EFIVARFS_MAGIC: u32 = 0xde5e_81e4;

/// Maximum length of a variable name (in UCS-2 chars).
pub const EFI_VAR_NAME_LEN: u32 = 1024;

/// Length of an EFI variable's GUID in bytes.
pub const EFI_GUID_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// EFI variable attributes (first 4 bytes of the file contents)
// ---------------------------------------------------------------------------

/// Variable is in non-volatile storage.
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 1 << 0;
/// Variable is accessible at boot-time only.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 1 << 1;
/// Variable is accessible at runtime.
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 1 << 2;
/// Variable indicates a hardware-error record.
pub const EFI_VARIABLE_HARDWARE_ERROR_RECORD: u32 = 1 << 3;
/// Variable uses authenticated writes (RSA2048+SHA256 — deprecated).
pub const EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS: u32 = 1 << 4;
/// Variable uses time-based authenticated writes.
pub const EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS: u32 = 1 << 5;
/// Append, don't overwrite.
pub const EFI_VARIABLE_APPEND_WRITE: u32 = 1 << 6;
/// Enhanced authenticated access (UEFI 2.8).
pub const EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS: u32 = 1 << 7;

/// Mask of every defined attribute bit.
pub const EFI_VARIABLE_MASK: u32 = EFI_VARIABLE_NON_VOLATILE
    | EFI_VARIABLE_BOOTSERVICE_ACCESS
    | EFI_VARIABLE_RUNTIME_ACCESS
    | EFI_VARIABLE_HARDWARE_ERROR_RECORD
    | EFI_VARIABLE_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_TIME_BASED_AUTHENTICATED_WRITE_ACCESS
    | EFI_VARIABLE_APPEND_WRITE
    | EFI_VARIABLE_ENHANCED_AUTHENTICATED_ACCESS;

// ---------------------------------------------------------------------------
// Well-known GUIDs (used to namespace variable names)
// ---------------------------------------------------------------------------

/// EFI_GLOBAL_VARIABLE GUID (Boot####, BootOrder, etc.).
pub const EFI_GLOBAL_VARIABLE_GUID: [u8; 16] = [
    0x61, 0xdf, 0xe4, 0x8b, 0xca, 0x93, 0xd2, 0x11,
    0xaa, 0x0d, 0x00, 0xe0, 0x98, 0x03, 0x2b, 0x8c,
];
/// EFI_IMAGE_SECURITY_DATABASE_GUID (db, dbx, KEK).
pub const EFI_IMAGE_SECURITY_DATABASE_GUID: [u8; 16] = [
    0xcb, 0xb2, 0x19, 0xd7, 0x3a, 0x3d, 0x96, 0x45,
    0xa3, 0xbc, 0xda, 0xd0, 0x0e, 0x67, 0x65, 0x6f,
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_and_sizes() {
        // efivarfs magic — comes from the kernel's `EFIVARFS_MAGIC`.
        assert_eq!(EFIVARFS_MAGIC, 0xde5e_81e4);
        assert_eq!(EFI_GUID_LEN, 16);
        assert!(EFI_VAR_NAME_LEN.is_power_of_two());
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
    }

    #[test]
    fn test_mask_covers_all_attrs() {
        // Mask must include every defined bit; otherwise userspace
        // writing the mask would silently strip unrecognised flags.
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
            assert_eq!(EFI_VARIABLE_MASK & b, b);
        }
    }

    #[test]
    fn test_guids_have_correct_length() {
        // Every EFI GUID is exactly 16 bytes.
        assert_eq!(EFI_GLOBAL_VARIABLE_GUID.len(), EFI_GUID_LEN as usize);
        assert_eq!(
            EFI_IMAGE_SECURITY_DATABASE_GUID.len(),
            EFI_GUID_LEN as usize
        );
        // The two well-known GUIDs must differ.
        assert_ne!(
            EFI_GLOBAL_VARIABLE_GUID,
            EFI_IMAGE_SECURITY_DATABASE_GUID
        );
    }
}
