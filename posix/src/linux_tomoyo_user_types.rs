//! TOMOYO Linux — path-based MAC LSM.
//!
//! TOMOYO is a path-name-based MAC (unlike SELinux's label-based MAC),
//! configured by writing policy files under `/sys/kernel/security/tomoyo/`.
//! The "learning mode" auto-generates policy from observed accesses.

// ---------------------------------------------------------------------------
// securityfs paths
// ---------------------------------------------------------------------------

pub const TOMOYO_ROOT: &str = "/sys/kernel/security/tomoyo";
pub const TOMOYO_DOMAIN_POLICY: &str = "/sys/kernel/security/tomoyo/domain_policy";
pub const TOMOYO_EXCEPTION_POLICY: &str = "/sys/kernel/security/tomoyo/exception_policy";
pub const TOMOYO_PROFILE: &str = "/sys/kernel/security/tomoyo/profile";
pub const TOMOYO_MANAGER: &str = "/sys/kernel/security/tomoyo/manager";
pub const TOMOYO_VERSION: &str = "/sys/kernel/security/tomoyo/version";
pub const TOMOYO_AUDIT: &str = "/sys/kernel/security/tomoyo/audit";
pub const TOMOYO_QUERY: &str = "/sys/kernel/security/tomoyo/query";
pub const TOMOYO_SELF_DOMAIN: &str = "/sys/kernel/security/tomoyo/self_domain";

// ---------------------------------------------------------------------------
// Profile enforcement modes
// ---------------------------------------------------------------------------

pub const TOMOYO_CONFIG_DISABLED: u32 = 0;
pub const TOMOYO_CONFIG_LEARNING: u32 = 1;
pub const TOMOYO_CONFIG_PERMISSIVE: u32 = 2;
pub const TOMOYO_CONFIG_ENFORCING: u32 = 3;

// ---------------------------------------------------------------------------
// Operations covered by TOMOYO domain rules
// ---------------------------------------------------------------------------

pub const TOMOYO_KW_FILE_EXECUTE: &str = "file execute";
pub const TOMOYO_KW_FILE_READ: &str = "file read";
pub const TOMOYO_KW_FILE_WRITE: &str = "file write";
pub const TOMOYO_KW_FILE_APPEND: &str = "file append";
pub const TOMOYO_KW_FILE_CREATE: &str = "file create";
pub const TOMOYO_KW_FILE_UNLINK: &str = "file unlink";
pub const TOMOYO_KW_FILE_MKDIR: &str = "file mkdir";
pub const TOMOYO_KW_FILE_RMDIR: &str = "file rmdir";
pub const TOMOYO_KW_FILE_TRUNCATE: &str = "file truncate";
pub const TOMOYO_KW_FILE_SYMLINK: &str = "file symlink";
pub const TOMOYO_KW_FILE_MKBLOCK: &str = "file mkblock";
pub const TOMOYO_KW_FILE_MKCHAR: &str = "file mkchar";
pub const TOMOYO_KW_FILE_MKFIFO: &str = "file mkfifo";
pub const TOMOYO_KW_FILE_MKSOCK: &str = "file mksock";
pub const TOMOYO_KW_FILE_LINK: &str = "file link";
pub const TOMOYO_KW_FILE_RENAME: &str = "file rename";
pub const TOMOYO_KW_FILE_CHMOD: &str = "file chmod";
pub const TOMOYO_KW_FILE_CHOWN: &str = "file chown";
pub const TOMOYO_KW_FILE_CHGRP: &str = "file chgrp";
pub const TOMOYO_KW_FILE_IOCTL: &str = "file ioctl";

// ---------------------------------------------------------------------------
// Network keywords
// ---------------------------------------------------------------------------

pub const TOMOYO_KW_NETWORK_INET_STREAM_BIND: &str = "network inet stream bind";
pub const TOMOYO_KW_NETWORK_INET_STREAM_LISTEN: &str = "network inet stream listen";
pub const TOMOYO_KW_NETWORK_INET_STREAM_CONNECT: &str = "network inet stream connect";
pub const TOMOYO_KW_NETWORK_INET_DGRAM_BIND: &str = "network inet dgram bind";
pub const TOMOYO_KW_NETWORK_INET_DGRAM_SEND: &str = "network inet dgram send";
pub const TOMOYO_KW_NETWORK_UNIX_STREAM_BIND: &str = "network unix stream bind";
pub const TOMOYO_KW_NETWORK_UNIX_STREAM_CONNECT: &str = "network unix stream connect";

// ---------------------------------------------------------------------------
// Profile-line keywords
// ---------------------------------------------------------------------------

pub const TOMOYO_KW_COMMENT: &str = "COMMENT=";
pub const TOMOYO_KW_PROFILE_CONFIG: &str = "PROFILE_CONFIG::";
pub const TOMOYO_KW_PREFERENCE: &str = "PREFERENCE::";

/// Default profile (index 0) is normally `disabled`.
pub const TOMOYO_DEFAULT_PROFILE: u32 = 0;
/// TOMOYO supports up to 256 profiles per system.
pub const TOMOYO_MAX_PROFILES: u32 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_securityfs_paths_under_root() {
        let p = [
            TOMOYO_DOMAIN_POLICY,
            TOMOYO_EXCEPTION_POLICY,
            TOMOYO_PROFILE,
            TOMOYO_MANAGER,
            TOMOYO_VERSION,
            TOMOYO_AUDIT,
            TOMOYO_QUERY,
            TOMOYO_SELF_DOMAIN,
        ];
        for path in p {
            assert!(path.starts_with(TOMOYO_ROOT));
        }
    }

    #[test]
    fn test_config_modes_dense_0_to_3() {
        assert_eq!(TOMOYO_CONFIG_DISABLED, 0);
        assert_eq!(TOMOYO_CONFIG_LEARNING, 1);
        assert_eq!(TOMOYO_CONFIG_PERMISSIVE, 2);
        assert_eq!(TOMOYO_CONFIG_ENFORCING, 3);
    }

    #[test]
    fn test_file_keywords_start_with_file() {
        let k = [
            TOMOYO_KW_FILE_EXECUTE,
            TOMOYO_KW_FILE_READ,
            TOMOYO_KW_FILE_WRITE,
            TOMOYO_KW_FILE_APPEND,
            TOMOYO_KW_FILE_CREATE,
            TOMOYO_KW_FILE_UNLINK,
            TOMOYO_KW_FILE_MKDIR,
            TOMOYO_KW_FILE_RMDIR,
            TOMOYO_KW_FILE_TRUNCATE,
            TOMOYO_KW_FILE_SYMLINK,
            TOMOYO_KW_FILE_MKBLOCK,
            TOMOYO_KW_FILE_MKCHAR,
            TOMOYO_KW_FILE_MKFIFO,
            TOMOYO_KW_FILE_MKSOCK,
            TOMOYO_KW_FILE_LINK,
            TOMOYO_KW_FILE_RENAME,
            TOMOYO_KW_FILE_CHMOD,
            TOMOYO_KW_FILE_CHOWN,
            TOMOYO_KW_FILE_CHGRP,
            TOMOYO_KW_FILE_IOCTL,
        ];
        for kw in k {
            assert!(kw.starts_with("file "));
        }
    }

    #[test]
    fn test_network_keywords_start_with_network() {
        let k = [
            TOMOYO_KW_NETWORK_INET_STREAM_BIND,
            TOMOYO_KW_NETWORK_INET_STREAM_LISTEN,
            TOMOYO_KW_NETWORK_INET_STREAM_CONNECT,
            TOMOYO_KW_NETWORK_INET_DGRAM_BIND,
            TOMOYO_KW_NETWORK_INET_DGRAM_SEND,
            TOMOYO_KW_NETWORK_UNIX_STREAM_BIND,
            TOMOYO_KW_NETWORK_UNIX_STREAM_CONNECT,
        ];
        for kw in k {
            assert!(kw.starts_with("network "));
        }
    }

    #[test]
    fn test_profile_limits() {
        assert_eq!(TOMOYO_DEFAULT_PROFILE, 0);
        // The profile index is an 8-bit unsigned, so 256 distinct slots.
        assert_eq!(TOMOYO_MAX_PROFILES, 1 << 8);
    }
}
