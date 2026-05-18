//! `<linux/xattr.h>` — Security namespace extended attribute constants.
//!
//! The "security." xattr namespace stores LSM labels (SELinux,
//! Smack, AppArmor), IMA/EVM digests, and file capabilities.
//! Only the kernel and privileged processes can set these.

// ---------------------------------------------------------------------------
// SELinux attributes
// ---------------------------------------------------------------------------

/// SELinux file context label.
pub const XATTR_SELINUX_SUFFIX: &[u8] = b"selinux";
/// SELinux default context for new files.
pub const SELINUX_CONTEXT_LEN_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// AppArmor attributes
// ---------------------------------------------------------------------------

/// AppArmor profile label attribute suffix.
pub const XATTR_APPARMOR_SUFFIX: &[u8] = b"apparmor";
/// Maximum AppArmor label length.
pub const APPARMOR_LABEL_LEN_MAX: u32 = 1024;

// ---------------------------------------------------------------------------
// Smack attributes
// ---------------------------------------------------------------------------

/// Smack64 label suffix.
pub const XATTR_SMACK_SUFFIX: &[u8] = b"SMACK64";
/// Smack64 exec label suffix.
pub const XATTR_SMACK_EXEC_SUFFIX: &[u8] = b"SMACK64EXEC";
/// Smack64 transmute suffix.
pub const XATTR_SMACK_TRANSMUTE_SUFFIX: &[u8] = b"SMACK64TRANSMUTE";
/// Smack64 mmap suffix.
pub const XATTR_SMACK_MMAP_SUFFIX: &[u8] = b"SMACK64MMAP";
/// Maximum Smack label length.
pub const SMACK_LABEL_LEN_MAX: u32 = 256;

// ---------------------------------------------------------------------------
// File capabilities (security.capability)
// ---------------------------------------------------------------------------

/// File capability attribute suffix.
pub const XATTR_CAPS_SUFFIX: &[u8] = b"capability";
/// VFS capability version 1 (deprecated).
pub const VFS_CAP_REVISION_1: u32 = 0x01000001;
/// VFS capability version 2.
pub const VFS_CAP_REVISION_2: u32 = 0x02000002;
/// VFS capability version 3 (namespace-aware).
pub const VFS_CAP_REVISION_3: u32 = 0x02000003;
/// Size of v2 capability data.
pub const VFS_CAP_U32_2: u32 = 2;

// ---------------------------------------------------------------------------
// IMA digest
// ---------------------------------------------------------------------------

/// IMA attribute suffix.
pub const XATTR_IMA_SUFFIX: &[u8] = b"ima";
/// IMA digest type: SHA256.
pub const IMA_HASH_SHA256: u32 = 4;
/// IMA digest type: SHA512.
pub const IMA_HASH_SHA512: u32 = 6;

// ---------------------------------------------------------------------------
// EVM signature
// ---------------------------------------------------------------------------

/// EVM attribute suffix.
pub const XATTR_EVM_SUFFIX: &[u8] = b"evm";
/// EVM HMAC type.
pub const EVM_XATTR_HMAC: u8 = 0x01;
/// EVM digital signature type.
pub const EVM_XATTR_PORTABLE_DIGSIG: u8 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suffixes_distinct() {
        let suffixes: [&[u8]; 5] = [
            XATTR_SELINUX_SUFFIX,
            XATTR_APPARMOR_SUFFIX,
            XATTR_SMACK_SUFFIX,
            XATTR_CAPS_SUFFIX,
            XATTR_IMA_SUFFIX,
        ];
        for i in 0..suffixes.len() {
            for j in (i + 1)..suffixes.len() {
                assert_ne!(suffixes[i], suffixes[j]);
            }
        }
    }

    #[test]
    fn test_smack_suffixes_distinct() {
        let suffixes: [&[u8]; 4] = [
            XATTR_SMACK_SUFFIX,
            XATTR_SMACK_EXEC_SUFFIX,
            XATTR_SMACK_TRANSMUTE_SUFFIX,
            XATTR_SMACK_MMAP_SUFFIX,
        ];
        for i in 0..suffixes.len() {
            for j in (i + 1)..suffixes.len() {
                assert_ne!(suffixes[i], suffixes[j]);
            }
        }
    }

    #[test]
    fn test_vfs_cap_revisions_distinct() {
        let revs = [VFS_CAP_REVISION_1, VFS_CAP_REVISION_2, VFS_CAP_REVISION_3];
        for i in 0..revs.len() {
            for j in (i + 1)..revs.len() {
                assert_ne!(revs[i], revs[j]);
            }
        }
    }

    #[test]
    fn test_ima_hashes_distinct() {
        assert_ne!(IMA_HASH_SHA256, IMA_HASH_SHA512);
    }

    #[test]
    fn test_evm_types_distinct() {
        assert_ne!(EVM_XATTR_HMAC, EVM_XATTR_PORTABLE_DIGSIG);
    }

    #[test]
    fn test_selinux_context_max() {
        assert_eq!(SELINUX_CONTEXT_LEN_MAX, 4096);
    }

    #[test]
    fn test_vfs_cap_v3() {
        assert_eq!(VFS_CAP_REVISION_3, 0x02000003);
    }
}
