//! `<linux/uuid.h>` — UUID/GUID constants and types.
//!
//! UUIDs (Universally Unique Identifiers) are used throughout the
//! Linux kernel for identifying devices, partitions (GPT), MEI
//! clients, ACPI objects, and more.

// ---------------------------------------------------------------------------
// UUID size
// ---------------------------------------------------------------------------

/// UUID size in bytes.
pub const UUID_SIZE: usize = 16;

/// UUID string length (with hyphens, without NUL).
pub const UUID_STRING_LEN: usize = 36;

// ---------------------------------------------------------------------------
// UUID type
// ---------------------------------------------------------------------------

/// A 128-bit UUID.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uuid {
    /// Raw UUID bytes.
    pub b: [u8; UUID_SIZE],
}

impl Uuid {
    /// Create a nil UUID (all zeros).
    pub const fn nil() -> Self {
        Self {
            b: [0u8; UUID_SIZE],
        }
    }

    /// Check if this UUID is nil (all zeros).
    pub fn is_nil(&self) -> bool {
        self.b == [0u8; UUID_SIZE]
    }

    /// Create a UUID from raw bytes.
    pub const fn from_bytes(bytes: [u8; UUID_SIZE]) -> Self {
        Self { b: bytes }
    }
}

/// A GUID (same storage as UUID, different byte order convention).
/// Microsoft-style: first three fields are little-endian.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    /// Raw GUID bytes.
    pub b: [u8; UUID_SIZE],
}

impl Guid {
    /// Create a nil GUID (all zeros).
    pub const fn nil() -> Self {
        Self {
            b: [0u8; UUID_SIZE],
        }
    }

    /// Check if this GUID is nil (all zeros).
    pub fn is_nil(&self) -> bool {
        self.b == [0u8; UUID_SIZE]
    }

    /// Create a GUID from raw bytes.
    pub const fn from_bytes(bytes: [u8; UUID_SIZE]) -> Self {
        Self { b: bytes }
    }
}

// ---------------------------------------------------------------------------
// Well-known UUIDs (GPT partition types)
// ---------------------------------------------------------------------------

/// EFI System Partition GUID bytes (C12A7328-F81F-11D2-BA4B-00A0C93EC93B).
pub const EFI_SYSTEM_PARTITION_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];

/// Linux filesystem partition GUID (0FC63DAF-8483-4772-8E79-3D69D8477DE4).
pub const LINUX_FS_GUID: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];

/// Linux swap partition GUID (0657FD6D-A4AB-43C4-84E5-0933C84B4F4F).
pub const LINUX_SWAP_GUID: [u8; 16] = [
    0x6d, 0xfd, 0x57, 0x06, 0xab, 0xa4, 0xc4, 0x43, 0x84, 0xe5, 0x09, 0x33, 0xc8, 0x4b, 0x4f, 0x4f,
];

// ---------------------------------------------------------------------------
// UUID versions (extracted from version nibble)
// ---------------------------------------------------------------------------

/// Version 1: time-based.
pub const UUID_VERSION_1: u8 = 1;
/// Version 3: MD5 hash.
pub const UUID_VERSION_3: u8 = 3;
/// Version 4: random.
pub const UUID_VERSION_4: u8 = 4;
/// Version 5: SHA-1 hash.
pub const UUID_VERSION_5: u8 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_size() {
        assert_eq!(UUID_SIZE, 16);
        assert_eq!(core::mem::size_of::<Uuid>(), 16);
    }

    #[test]
    fn test_guid_size() {
        assert_eq!(core::mem::size_of::<Guid>(), 16);
    }

    #[test]
    fn test_nil_uuid() {
        let u = Uuid::nil();
        assert!(u.is_nil());
        assert_eq!(u.b, [0u8; 16]);
    }

    #[test]
    fn test_nil_guid() {
        let g = Guid::nil();
        assert!(g.is_nil());
    }

    #[test]
    fn test_non_nil() {
        let u = Uuid::from_bytes([1; 16]);
        assert!(!u.is_nil());
    }

    #[test]
    fn test_uuid_eq() {
        let a = Uuid::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let b = Uuid::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(a, b);
    }

    #[test]
    fn test_uuid_ne() {
        let a = Uuid::nil();
        let b = Uuid::from_bytes([1; 16]);
        assert_ne!(a, b);
    }

    #[test]
    fn test_string_len() {
        assert_eq!(UUID_STRING_LEN, 36);
    }

    #[test]
    fn test_versions_distinct() {
        let versions = [
            UUID_VERSION_1,
            UUID_VERSION_3,
            UUID_VERSION_4,
            UUID_VERSION_5,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_well_known_guids_distinct() {
        assert_ne!(EFI_SYSTEM_PARTITION_GUID, LINUX_FS_GUID);
        assert_ne!(LINUX_FS_GUID, LINUX_SWAP_GUID);
        assert_ne!(EFI_SYSTEM_PARTITION_GUID, LINUX_SWAP_GUID);
    }
}
