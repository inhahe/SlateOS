//! `<linux/fcntl.h>` — File seal constants for memfd/shmem.
//!
//! File seals prevent certain operations on memfd/shmem file
//! descriptors, enabling safe shared-memory scenarios. Once a seal
//! is set, the corresponding operation is permanently forbidden.
//! This allows a process to share a memfd while guaranteeing the
//! contents won't change.

// ---------------------------------------------------------------------------
// File seal flags (fcntl F_ADD_SEALS / F_GET_SEALS)
// ---------------------------------------------------------------------------

/// Prevent further seal changes.
pub const F_SEAL_SEAL: u32 = 0x0001;
/// Prevent file from shrinking.
pub const F_SEAL_SHRINK: u32 = 0x0002;
/// Prevent file from growing.
pub const F_SEAL_GROW: u32 = 0x0004;
/// Prevent writes to the file.
pub const F_SEAL_WRITE: u32 = 0x0008;
/// Prevent writes while mapped (allows mmapped reads).
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
/// Execute seal (for memfd_create MFD_EXEC semantics).
pub const F_SEAL_EXEC: u32 = 0x0020;

// ---------------------------------------------------------------------------
// fcntl seal commands
// ---------------------------------------------------------------------------

/// Add seals to a file descriptor.
pub const F_ADD_SEALS: u32 = 1033;
/// Get the current set of seals.
pub const F_GET_SEALS: u32 = 1034;

// ---------------------------------------------------------------------------
// memfd_create flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the memfd.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// Allow sealing (without this, seals cannot be added).
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
/// Create file with hugetlb pages.
pub const MFD_HUGETLB: u32 = 0x0004;
/// Do not allow writes after creation (for W^X).
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;
/// Allow execution.
pub const MFD_EXEC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seal_flags_no_overlap() {
        let seals = [
            F_SEAL_SEAL,
            F_SEAL_SHRINK,
            F_SEAL_GROW,
            F_SEAL_WRITE,
            F_SEAL_FUTURE_WRITE,
            F_SEAL_EXEC,
        ];
        for i in 0..seals.len() {
            assert!(seals[i].is_power_of_two());
            for j in (i + 1)..seals.len() {
                assert_eq!(seals[i] & seals[j], 0);
            }
        }
    }

    #[test]
    fn test_seal_commands_distinct() {
        assert_ne!(F_ADD_SEALS, F_GET_SEALS);
    }

    #[test]
    fn test_memfd_flags_no_overlap() {
        let flags = [
            MFD_CLOEXEC,
            MFD_ALLOW_SEALING,
            MFD_HUGETLB,
            MFD_NOEXEC_SEAL,
            MFD_EXEC,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_seal_seal_value() {
        assert_eq!(F_SEAL_SEAL, 1);
    }
}
