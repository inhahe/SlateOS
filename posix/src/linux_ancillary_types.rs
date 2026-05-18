//! `<sys/socket.h>` — Ancillary data alignment and size constants.
//!
//! Ancillary data in control messages must be properly aligned in
//! the `msg_control` buffer. These constants define the alignment
//! requirements and size computation helpers used by the CMSG_*
//! macros in userspace.

// ---------------------------------------------------------------------------
// CMSG alignment and size constants
// ---------------------------------------------------------------------------

/// Alignment of cmsghdr structures (platform-dependent, 8 on x86_64).
pub const CMSG_ALIGN_SIZE: usize = 8;

/// Size of cmsghdr header (cmsg_len, cmsg_level, cmsg_type).
/// On x86_64 Linux this is 16 bytes (with padding).
pub const CMSGHDR_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Maximum ancillary data sizes
// ---------------------------------------------------------------------------

/// Maximum number of file descriptors per SCM_RIGHTS message.
pub const SCM_MAX_FD: u32 = 253;

/// Maximum total ancillary buffer size (sysctl default).
pub const OPTMEM_MAX_DEFAULT: u32 = 20480;

// ---------------------------------------------------------------------------
// Credential structure field count
// ---------------------------------------------------------------------------

/// Number of fields in ucred structure (pid, uid, gid).
pub const UCRED_FIELDS: u32 = 3;

/// Size of ucred structure in bytes (3 × 4 bytes on Linux).
pub const UCRED_SIZE: usize = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmsg_alignment() {
        assert_eq!(CMSG_ALIGN_SIZE, 8);
        assert!(CMSG_ALIGN_SIZE.is_power_of_two());
    }

    #[test]
    fn test_cmsghdr_size() {
        assert_eq!(CMSGHDR_SIZE, 16);
        // Must be aligned to CMSG_ALIGN_SIZE
        assert_eq!(CMSGHDR_SIZE % CMSG_ALIGN_SIZE, 0);
    }

    #[test]
    fn test_scm_max_fd() {
        assert_eq!(SCM_MAX_FD, 253);
    }

    #[test]
    fn test_optmem_max() {
        assert_eq!(OPTMEM_MAX_DEFAULT, 20480);
    }

    #[test]
    fn test_ucred() {
        assert_eq!(UCRED_FIELDS, 3);
        assert_eq!(UCRED_SIZE, 12);
    }
}
