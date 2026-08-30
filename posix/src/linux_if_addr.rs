//! `<linux/if_addr.h>` — interface address attributes for rtnetlink.
//!
//! IFA_* attributes are used in RTM_NEWADDR / RTM_GETADDR netlink
//! messages to describe and configure network interface addresses.

// ---------------------------------------------------------------------------
// IFA_* attribute types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFA_UNSPEC: u16 = 0;
/// Interface address.
pub const IFA_ADDRESS: u16 = 1;
/// Local address.
pub const IFA_LOCAL: u16 = 2;
/// Interface name label.
pub const IFA_LABEL: u16 = 3;
/// Broadcast address.
pub const IFA_BROADCAST: u16 = 4;
/// Anycast address.
pub const IFA_ANYCAST: u16 = 5;
/// Address cache info.
pub const IFA_CACHEINFO: u16 = 6;
/// Multicast address.
pub const IFA_MULTICAST: u16 = 7;
/// Address flags (extended, u32).
pub const IFA_FLAGS: u16 = 8;
/// Route priority / metric.
pub const IFA_RT_PRIORITY: u16 = 9;
/// Target network namespace ID.
pub const IFA_TARGET_NETNSID: u16 = 10;
/// Protocol that installed the address.
pub const IFA_PROTO: u16 = 11;

// ---------------------------------------------------------------------------
// IFA_F_* address flags
// ---------------------------------------------------------------------------

/// Address is secondary/backup.
pub const IFA_F_SECONDARY: u32 = 0x01;
/// Alias for IFA_F_SECONDARY.
pub const IFA_F_TEMPORARY: u32 = IFA_F_SECONDARY;
/// Do not perform duplicate address detection.
pub const IFA_F_NODAD: u32 = 0x02;
/// Optimistic duplicate address detection.
pub const IFA_F_OPTIMISTIC: u32 = 0x04;
/// Duplicate address detected.
pub const IFA_F_DADFAILED: u32 = 0x08;
/// Home address (MIPv6).
pub const IFA_F_HOMEADDRESS: u32 = 0x10;
/// Address is deprecated.
pub const IFA_F_DEPRECATED: u32 = 0x20;
/// Address is tentative.
pub const IFA_F_TENTATIVE: u32 = 0x40;
/// Permanent address (no lifetime).
pub const IFA_F_PERMANENT: u32 = 0x80;
/// Manage temporary addresses.
pub const IFA_F_MANAGETEMPADDR: u32 = 0x100;
/// Don't create prefix route.
pub const IFA_F_NOPREFIXROUTE: u32 = 0x200;
/// Autoconfigured address.
pub const IFA_F_MCAUTOJOIN: u32 = 0x400;
/// Stable privacy address.
pub const IFA_F_STABLE_PRIVACY: u32 = 0x800;

// ---------------------------------------------------------------------------
// ifaddrmsg struct (netlink message payload)
// ---------------------------------------------------------------------------

/// Interface address message (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ifaddrmsg {
    /// Address family (AF_INET, AF_INET6, etc.).
    pub ifa_family: u8,
    /// Prefix length.
    pub ifa_prefixlen: u8,
    /// Address flags (IFA_F_*).
    pub ifa_flags: u8,
    /// Address scope.
    pub ifa_scope: u8,
    /// Interface index.
    pub ifa_index: u32,
}

impl Ifaddrmsg {
    /// Create a zeroed interface address message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Address cache info (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IfaCacheinfo {
    /// Preferred lifetime.
    pub ifa_prefered: u32,
    /// Valid lifetime.
    pub ifa_valid: u32,
    /// Time since creation (jiffies).
    pub cstamp: u32,
    /// Time since last update (jiffies).
    pub tstamp: u32,
}

impl IfaCacheinfo {
    /// Create a zeroed cache info.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Address scopes
// ---------------------------------------------------------------------------

/// Global scope.
pub const RT_SCOPE_UNIVERSE: u8 = 0;
/// Site scope (deprecated for IPv6).
pub const RT_SCOPE_SITE: u8 = 200;
/// Link scope.
pub const RT_SCOPE_LINK: u8 = 253;
/// Host scope.
pub const RT_SCOPE_HOST: u8 = 254;
/// Nowhere scope.
pub const RT_SCOPE_NOWHERE: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifa_attrs_sequential() {
        assert_eq!(IFA_UNSPEC, 0);
        assert_eq!(IFA_ADDRESS, 1);
        assert_eq!(IFA_LOCAL, 2);
        assert_eq!(IFA_LABEL, 3);
        assert_eq!(IFA_BROADCAST, 4);
        assert_eq!(IFA_ANYCAST, 5);
        assert_eq!(IFA_CACHEINFO, 6);
    }

    #[test]
    fn test_ifa_flags_distinct() {
        let flags = [
            IFA_F_SECONDARY,
            IFA_F_NODAD,
            IFA_F_OPTIMISTIC,
            IFA_F_DADFAILED,
            IFA_F_HOMEADDRESS,
            IFA_F_DEPRECATED,
            IFA_F_TENTATIVE,
            IFA_F_PERMANENT,
            IFA_F_MANAGETEMPADDR,
            IFA_F_NOPREFIXROUTE,
            IFA_F_MCAUTOJOIN,
            IFA_F_STABLE_PRIVACY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ifaddrmsg_size() {
        assert_eq!(core::mem::size_of::<Ifaddrmsg>(), 8);
    }

    #[test]
    fn test_ifa_cacheinfo_size() {
        assert_eq!(core::mem::size_of::<IfaCacheinfo>(), 16);
    }

    #[test]
    fn test_scopes() {
        assert_eq!(RT_SCOPE_UNIVERSE, 0);
        assert!(RT_SCOPE_LINK > RT_SCOPE_SITE);
        assert!(RT_SCOPE_HOST > RT_SCOPE_LINK);
        assert!(RT_SCOPE_NOWHERE > RT_SCOPE_HOST);
    }

    #[test]
    fn test_temporary_alias() {
        assert_eq!(IFA_F_TEMPORARY, IFA_F_SECONDARY);
    }
}
