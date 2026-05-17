//! `<linux/cifs/cifs_mount.h>` — CIFS/SMB filesystem mount constants.
//!
//! CIFS (Common Internet File System) / SMB (Server Message Block)
//! provides network file sharing, primarily with Windows servers.
//! The Linux cifs.ko module mounts remote SMB shares. These constants
//! define mount options, security modes, and protocol versions.

// ---------------------------------------------------------------------------
// SMB protocol versions
// ---------------------------------------------------------------------------

/// SMB 1.0 / CIFS (legacy, insecure).
pub const SMB_PROTOCOL_SMB1: u32 = 1;
/// SMB 2.0.
pub const SMB_PROTOCOL_SMB2: u32 = 0x0200;
/// SMB 2.1.
pub const SMB_PROTOCOL_SMB21: u32 = 0x0210;
/// SMB 3.0.
pub const SMB_PROTOCOL_SMB30: u32 = 0x0300;
/// SMB 3.0.2.
pub const SMB_PROTOCOL_SMB302: u32 = 0x0302;
/// SMB 3.1.1 (latest, mandatory encryption support).
pub const SMB_PROTOCOL_SMB311: u32 = 0x0311;

// ---------------------------------------------------------------------------
// Security modes
// ---------------------------------------------------------------------------

/// User-level security (user+password authentication).
pub const CIFS_SEC_USER: u32 = 1;
/// Kerberos (KRB5) authentication.
pub const CIFS_SEC_KRB5: u32 = 2;
/// NTLMv2 (default modern authentication).
pub const CIFS_SEC_NTLMV2: u32 = 4;
/// NTLMSSP (NT LAN Manager Security Support Provider).
pub const CIFS_SEC_NTLMSSP: u32 = 8;

// ---------------------------------------------------------------------------
// Mount flags
// ---------------------------------------------------------------------------

/// Enable POSIX extensions (if server supports them).
pub const CIFS_MOUNT_POSIX: u32 = 1 << 0;
/// Don't follow DFS referrals.
pub const CIFS_MOUNT_NO_DFS: u32 = 1 << 1;
/// Read-only mount.
pub const CIFS_MOUNT_RO: u32 = 1 << 2;
/// Allow setuid bits.
pub const CIFS_MOUNT_SUID: u32 = 1 << 3;
/// Use soft mount (return errors on timeout).
pub const CIFS_MOUNT_SOFT: u32 = 1 << 4;
/// Server inode numbers (don't generate locally).
pub const CIFS_MOUNT_SERVER_INUM: u32 = 1 << 5;
/// Use Unix extensions.
pub const CIFS_MOUNT_UNIX_EXT: u32 = 1 << 6;
/// Send mandatory byte-range locks.
pub const CIFS_MOUNT_MAND_LOCK: u32 = 1 << 7;
/// Enable SMB signing.
pub const CIFS_MOUNT_SIGN: u32 = 1 << 8;
/// Require encryption.
pub const CIFS_MOUNT_SEAL: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Cache modes
// ---------------------------------------------------------------------------

/// No client-side caching.
pub const CIFS_CACHE_NONE: u32 = 0;
/// Strict caching (close-to-open semantics).
pub const CIFS_CACHE_STRICT: u32 = 1;
/// Loose caching (local cache without revalidation).
pub const CIFS_CACHE_LOOSE: u32 = 2;
/// Read-only caching.
pub const CIFS_CACHE_RO: u32 = 3;
/// Read-write caching (oplock/lease delegation).
pub const CIFS_CACHE_RW: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_versions_ordered() {
        assert!(SMB_PROTOCOL_SMB1 < SMB_PROTOCOL_SMB2);
        assert!(SMB_PROTOCOL_SMB2 < SMB_PROTOCOL_SMB21);
        assert!(SMB_PROTOCOL_SMB21 < SMB_PROTOCOL_SMB30);
        assert!(SMB_PROTOCOL_SMB30 < SMB_PROTOCOL_SMB302);
        assert!(SMB_PROTOCOL_SMB302 < SMB_PROTOCOL_SMB311);
    }

    #[test]
    fn test_security_modes_no_overlap() {
        let modes = [CIFS_SEC_USER, CIFS_SEC_KRB5, CIFS_SEC_NTLMV2, CIFS_SEC_NTLMSSP];
        for i in 0..modes.len() {
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_mount_flags_no_overlap() {
        let flags = [
            CIFS_MOUNT_POSIX, CIFS_MOUNT_NO_DFS, CIFS_MOUNT_RO,
            CIFS_MOUNT_SUID, CIFS_MOUNT_SOFT, CIFS_MOUNT_SERVER_INUM,
            CIFS_MOUNT_UNIX_EXT, CIFS_MOUNT_MAND_LOCK,
            CIFS_MOUNT_SIGN, CIFS_MOUNT_SEAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cache_modes_distinct() {
        let modes = [
            CIFS_CACHE_NONE, CIFS_CACHE_STRICT,
            CIFS_CACHE_LOOSE, CIFS_CACHE_RO, CIFS_CACHE_RW,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
