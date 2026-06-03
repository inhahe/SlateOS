//! Linux UTS namespace constants.
//!
//! UTS namespaces isolate the system hostname and NIS domain name.
//! Each UTS namespace has its own `uname` output, enabling
//! containers to report different hostnames independently.

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new UTS namespace.
pub const CLONE_NEWUTS: u64 = 0x04000000;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// UTS namespace proc link.
pub const PROC_NS_UTS: &str = "ns/uts";

// ---------------------------------------------------------------------------
// UTS field limits (from <linux/utsname.h>)
// ---------------------------------------------------------------------------

/// Maximum hostname length (including NUL).
pub const UTS_HOSTNAME_LEN: usize = 65;
/// Maximum domainname length (including NUL).
pub const UTS_DOMAINNAME_LEN: usize = 65;
/// Maximum sysname length.
pub const UTS_SYSNAME_LEN: usize = 65;
/// Maximum release string length.
pub const UTS_RELEASE_LEN: usize = 65;
/// Maximum version string length.
pub const UTS_VERSION_LEN: usize = 65;
/// Maximum machine string length.
pub const UTS_MACHINE_LEN: usize = 65;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

/// Default hostname for new UTS namespaces.
pub const UTS_DEFAULT_HOSTNAME: &str = "(none)";
/// Default NIS domain name.
pub const UTS_DEFAULT_DOMAINNAME: &str = "(none)";

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

/// Hostname sysctl.
pub const SYSCTL_HOSTNAME: &str = "kernel.hostname";
/// Domain name sysctl.
pub const SYSCTL_DOMAINNAME: &str = "kernel.domainname";
/// OS type sysctl.
pub const SYSCTL_OSTYPE: &str = "kernel.ostype";
/// OS release sysctl.
pub const SYSCTL_OSRELEASE: &str = "kernel.osrelease";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newuts() {
        assert_eq!(CLONE_NEWUTS, 0x04000000);
        assert!((CLONE_NEWUTS as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_no_overlap_with_other_ns() {
        let other_ns: &[u64] = &[
            0x10000000, // CLONE_NEWUSER
            0x20000000, // CLONE_NEWPID
            0x00020000, // CLONE_NEWNS
            0x40000000, // CLONE_NEWNET
            0x08000000, // CLONE_NEWIPC
        ];
        for flag in other_ns {
            assert_ne!(CLONE_NEWUTS, *flag);
        }
    }

    #[test]
    fn test_field_lengths() {
        // All UTS fields have the same length
        assert_eq!(UTS_HOSTNAME_LEN, 65);
        assert_eq!(UTS_DOMAINNAME_LEN, 65);
        assert_eq!(UTS_SYSNAME_LEN, 65);
        assert_eq!(UTS_RELEASE_LEN, 65);
        assert_eq!(UTS_VERSION_LEN, 65);
        assert_eq!(UTS_MACHINE_LEN, 65);
    }

    #[test]
    fn test_defaults_not_empty() {
        assert!(!UTS_DEFAULT_HOSTNAME.is_empty());
        assert!(!UTS_DEFAULT_DOMAINNAME.is_empty());
    }

    #[test]
    fn test_sysctl_paths_distinct() {
        let paths = [
            SYSCTL_HOSTNAME,
            SYSCTL_DOMAINNAME,
            SYSCTL_OSTYPE,
            SYSCTL_OSRELEASE,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_sysctl_paths_have_kernel_prefix() {
        let paths = [
            SYSCTL_HOSTNAME,
            SYSCTL_DOMAINNAME,
            SYSCTL_OSTYPE,
            SYSCTL_OSRELEASE,
        ];
        for path in &paths {
            assert!(path.starts_with("kernel."), "{}", path);
        }
    }
}
