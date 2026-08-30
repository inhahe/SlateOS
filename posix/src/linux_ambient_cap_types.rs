//! `<linux/capability.h>` — Ambient and inherited capability constants.
//!
//! Ambient capabilities are capabilities that are preserved across
//! execve() for non-root processes, allowing privilege to flow to
//! child processes without setuid binaries. These constants define
//! the capability set types and version information.

// ---------------------------------------------------------------------------
// Capability set types (for capget/capset)
// ---------------------------------------------------------------------------

/// Effective capability set (currently active).
pub const CAP_EFFECTIVE: u32 = 0;
/// Permitted capability set (upper bound).
pub const CAP_PERMITTED: u32 = 1;
/// Inheritable capability set (preserved across exec).
pub const CAP_INHERITABLE: u32 = 2;

// ---------------------------------------------------------------------------
// Capability header version magic
// ---------------------------------------------------------------------------

/// Capability version 1 (Linux 2.6.25-).
pub const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
/// Capability version 2 (Linux 2.6.25+).
pub const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_1026;
/// Capability version 3 (Linux 2.6.26+, 64-bit).
pub const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

// ---------------------------------------------------------------------------
// Capability data sizes
// ---------------------------------------------------------------------------

/// Number of u32 words in v1 capability data.
pub const CAP_V1_DATA_WORDS: u32 = 1;
/// Number of u32 words in v3 capability data.
pub const CAP_V3_DATA_WORDS: u32 = 2;

// ---------------------------------------------------------------------------
// File capability constants
// ---------------------------------------------------------------------------

/// File capability version 2 (xattr format).
pub const VFS_CAP_REVISION_2: u32 = 0x0200_0000;
/// File capability version 3 (namespace-aware).
pub const VFS_CAP_REVISION_3: u32 = 0x0300_0000;
/// File capability flags: effective bit.
pub const VFS_CAP_FLAGS_EFFECTIVE: u32 = 0x000001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_types_distinct() {
        let sets = [CAP_EFFECTIVE, CAP_PERMITTED, CAP_INHERITABLE];
        for i in 0..sets.len() {
            for j in (i + 1)..sets.len() {
                assert_ne!(sets[i], sets[j]);
            }
        }
    }

    #[test]
    fn test_versions_distinct() {
        let versions = [
            LINUX_CAPABILITY_VERSION_1,
            LINUX_CAPABILITY_VERSION_2,
            LINUX_CAPABILITY_VERSION_3,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_version_3_is_current() {
        assert_eq!(LINUX_CAPABILITY_VERSION_3, 0x2008_0522);
    }

    #[test]
    fn test_data_words() {
        assert_eq!(CAP_V1_DATA_WORDS, 1);
        assert_eq!(CAP_V3_DATA_WORDS, 2);
    }

    #[test]
    fn test_vfs_cap_revisions() {
        assert_ne!(VFS_CAP_REVISION_2, VFS_CAP_REVISION_3);
    }

    #[test]
    fn test_effective_is_zero() {
        assert_eq!(CAP_EFFECTIVE, 0);
    }
}
