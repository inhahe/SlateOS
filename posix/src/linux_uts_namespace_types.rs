//! `<linux/utsname.h>` — UTS namespace constants.
//!
//! UTS (Unix Timesharing System) namespaces isolate hostname and
//! domain name. Each UTS namespace has its own values for
//! `uname()` fields: sysname, nodename (hostname), release,
//! version, machine, and domainname. Containers use UTS namespaces
//! so each container can have its own hostname without affecting
//! the host or other containers.

// ---------------------------------------------------------------------------
// UTS field length limits
// ---------------------------------------------------------------------------

/// Maximum length of nodename (hostname) including null terminator.
pub const UTS_NODENAME_LEN: u32 = 65;
/// Maximum length of sysname field.
pub const UTS_SYSNAME_LEN: u32 = 65;
/// Maximum length of release field.
pub const UTS_RELEASE_LEN: u32 = 65;
/// Maximum length of version field.
pub const UTS_VERSION_LEN: u32 = 65;
/// Maximum length of machine field.
pub const UTS_MACHINE_LEN: u32 = 65;
/// Maximum length of domainname field.
pub const UTS_DOMAINNAME_LEN: u32 = 65;

// ---------------------------------------------------------------------------
// sethostname/setdomainname limits
// ---------------------------------------------------------------------------

/// Maximum hostname length (HOST_NAME_MAX, POSIX limit).
pub const HOST_NAME_MAX: u32 = 64;
/// Maximum NIS domain name length.
pub const DOMAIN_NAME_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// UTS namespace clone flag
// ---------------------------------------------------------------------------

/// Clone flag for creating a new UTS namespace.
pub const CLONE_NEWUTS: u32 = 0x0400_0000;

// ---------------------------------------------------------------------------
// uname() sysname values (for reference)
// ---------------------------------------------------------------------------

/// Linux sysname value (what uname -s returns).
pub const UTS_SYSNAME_LINUX: u32 = 0;

// ---------------------------------------------------------------------------
// UTS namespace states
// ---------------------------------------------------------------------------

/// Namespace is active.
pub const UTSNS_STATE_ACTIVE: u32 = 0;
/// Namespace is being destroyed.
pub const UTSNS_STATE_DYING: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_lengths_consistent() {
        // All UTS fields have the same max length in Linux
        assert_eq!(UTS_NODENAME_LEN, UTS_SYSNAME_LEN);
        assert_eq!(UTS_SYSNAME_LEN, UTS_RELEASE_LEN);
        assert_eq!(UTS_RELEASE_LEN, UTS_VERSION_LEN);
        assert_eq!(UTS_VERSION_LEN, UTS_MACHINE_LEN);
        assert_eq!(UTS_MACHINE_LEN, UTS_DOMAINNAME_LEN);
    }

    #[test]
    fn test_host_name_max_fits_in_field() {
        // hostname must fit in nodename field (with null terminator)
        assert!(HOST_NAME_MAX < UTS_NODENAME_LEN);
    }

    #[test]
    fn test_domain_name_max_fits_in_field() {
        assert!(DOMAIN_NAME_MAX < UTS_DOMAINNAME_LEN);
    }

    #[test]
    fn test_clone_flag_is_power_of_two() {
        assert!(CLONE_NEWUTS.is_power_of_two());
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(UTSNS_STATE_ACTIVE, UTSNS_STATE_DYING);
    }
}
