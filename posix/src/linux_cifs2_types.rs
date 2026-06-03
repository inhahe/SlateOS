//! `<linux/cifs/cifs_mount.h>` — Additional CIFS/SMB constants.
//!
//! Supplementary CIFS constants covering mount flags,
//! security modes, and SMB protocol versions.

// ---------------------------------------------------------------------------
// CIFS mount flags
// ---------------------------------------------------------------------------

/// Soft mount.
pub const CIFS_MOUNT_NO_PERM: u32 = 1 << 0;
/// Set UID from server.
pub const CIFS_MOUNT_SET_UID: u32 = 1 << 1;
/// Map special chars.
pub const CIFS_MOUNT_MAP_SPECIAL_CHR: u32 = 1 << 3;
/// Direct I/O.
pub const CIFS_MOUNT_DIRECT_IO: u32 = 1 << 4;
/// No xattr.
pub const CIFS_MOUNT_NO_XATTR: u32 = 1 << 5;
/// Map POSIX ACLs.
pub const CIFS_MOUNT_POSIX_PATHS: u32 = 1 << 6;
/// CIFS oplock.
pub const CIFS_MOUNT_NO_BRL: u32 = 1 << 7;
/// Force new SMB session.
pub const CIFS_MOUNT_CIFS_ACL: u32 = 1 << 8;
/// Overwrite remap.
pub const CIFS_MOUNT_OVERR_SNAME: u32 = 1 << 9;
/// Server inode numbers.
pub const CIFS_MOUNT_SERVER_INUM: u32 = 1 << 10;
/// Strict cache mode.
pub const CIFS_MOUNT_STRICT_IO: u32 = 1 << 11;
/// Multi-user mount.
pub const CIFS_MOUNT_MULTIUSER: u32 = 1 << 13;
/// FSCACHE support.
pub const CIFS_MOUNT_FSCACHE: u32 = 1 << 14;
/// Read-only.
pub const CIFS_MOUNT_RO: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// CIFS security modes
// ---------------------------------------------------------------------------

/// NTLM security.
pub const CIFS_SECMODE_NTLM: u32 = 0;
/// NTLMv2 security.
pub const CIFS_SECMODE_NTLMV2: u32 = 1;
/// Kerberos security.
pub const CIFS_SECMODE_KRB5: u32 = 2;
/// Negotiate default.
pub const CIFS_SECMODE_NTLMSSP: u32 = 3;

// ---------------------------------------------------------------------------
// SMB protocol versions
// ---------------------------------------------------------------------------

/// SMB 1.0.
pub const SMB_PROTOCOL_SMB1: u32 = 0;
/// SMB 2.0.
pub const SMB_PROTOCOL_SMB2: u32 = 1;
/// SMB 2.1.
pub const SMB_PROTOCOL_SMB21: u32 = 2;
/// SMB 3.0.
pub const SMB_PROTOCOL_SMB30: u32 = 3;
/// SMB 3.0.2.
pub const SMB_PROTOCOL_SMB302: u32 = 4;
/// SMB 3.1.1.
pub const SMB_PROTOCOL_SMB311: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_flags_power_of_two() {
        let flags = [
            CIFS_MOUNT_NO_PERM,
            CIFS_MOUNT_SET_UID,
            CIFS_MOUNT_MAP_SPECIAL_CHR,
            CIFS_MOUNT_DIRECT_IO,
            CIFS_MOUNT_NO_XATTR,
            CIFS_MOUNT_POSIX_PATHS,
            CIFS_MOUNT_NO_BRL,
            CIFS_MOUNT_CIFS_ACL,
            CIFS_MOUNT_OVERR_SNAME,
            CIFS_MOUNT_SERVER_INUM,
            CIFS_MOUNT_STRICT_IO,
            CIFS_MOUNT_MULTIUSER,
            CIFS_MOUNT_FSCACHE,
            CIFS_MOUNT_RO,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_mount_flags_no_overlap() {
        let flags = [
            CIFS_MOUNT_NO_PERM,
            CIFS_MOUNT_SET_UID,
            CIFS_MOUNT_MAP_SPECIAL_CHR,
            CIFS_MOUNT_DIRECT_IO,
            CIFS_MOUNT_NO_XATTR,
            CIFS_MOUNT_POSIX_PATHS,
            CIFS_MOUNT_NO_BRL,
            CIFS_MOUNT_CIFS_ACL,
            CIFS_MOUNT_OVERR_SNAME,
            CIFS_MOUNT_SERVER_INUM,
            CIFS_MOUNT_STRICT_IO,
            CIFS_MOUNT_MULTIUSER,
            CIFS_MOUNT_FSCACHE,
            CIFS_MOUNT_RO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sec_modes_distinct() {
        let modes = [
            CIFS_SECMODE_NTLM,
            CIFS_SECMODE_NTLMV2,
            CIFS_SECMODE_KRB5,
            CIFS_SECMODE_NTLMSSP,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_protocol_versions_distinct() {
        let vers = [
            SMB_PROTOCOL_SMB1,
            SMB_PROTOCOL_SMB2,
            SMB_PROTOCOL_SMB21,
            SMB_PROTOCOL_SMB30,
            SMB_PROTOCOL_SMB302,
            SMB_PROTOCOL_SMB311,
        ];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
            }
        }
    }

    #[test]
    fn test_protocol_versions_ordered() {
        assert!(SMB_PROTOCOL_SMB1 < SMB_PROTOCOL_SMB2);
        assert!(SMB_PROTOCOL_SMB2 < SMB_PROTOCOL_SMB21);
        assert!(SMB_PROTOCOL_SMB21 < SMB_PROTOCOL_SMB30);
        assert!(SMB_PROTOCOL_SMB30 < SMB_PROTOCOL_SMB302);
        assert!(SMB_PROTOCOL_SMB302 < SMB_PROTOCOL_SMB311);
    }
}
