//! `<linux/sysctl.h>` — sysctl binary interface constants.
//!
//! While the sysctl binary interface (sys_sysctl) is deprecated in
//! favor of `/proc/sys`, these constants identify the top-level
//! sysctl categories and are still referenced by some applications.

// ---------------------------------------------------------------------------
// Top-level sysctl categories
// ---------------------------------------------------------------------------

/// Kernel parameters.
pub const CTL_KERN: i32 = 1;
/// VM parameters.
pub const CTL_VM: i32 = 2;
/// Network parameters.
pub const CTL_NET: i32 = 3;
/// /proc parameters.
pub const CTL_PROC: i32 = 4;
/// Filesystem parameters.
pub const CTL_FS: i32 = 5;
/// Debug parameters.
pub const CTL_DEBUG: i32 = 6;
/// Device parameters.
pub const CTL_DEV: i32 = 7;

// ---------------------------------------------------------------------------
// CTL_KERN subcategories
// ---------------------------------------------------------------------------

/// OS type.
pub const KERN_OSTYPE: i32 = 1;
/// OS release.
pub const KERN_OSRELEASE: i32 = 2;
/// OS revision.
pub const KERN_OSREV: i32 = 3;
/// Kernel version string.
pub const KERN_VERSION: i32 = 4;
/// Max threads.
pub const KERN_MAX_THREADS: i32 = 14;
/// Random entropy pool.
pub const KERN_RANDOM: i32 = 44;
/// Hostname.
pub const KERN_HOSTNAME: i32 = 10;
/// Domain name.
pub const KERN_DOMAINNAME: i32 = 22;

// ---------------------------------------------------------------------------
// CTL_VM subcategories
// ---------------------------------------------------------------------------

/// Overcommit memory policy.
pub const VM_OVERCOMMIT_MEMORY: i32 = 5;
/// Swappiness.
pub const VM_SWAPPINESS: i32 = 19;
/// Dirty ratio.
pub const VM_DIRTY_RATIO: i32 = 20;
/// Dirty background ratio.
pub const VM_DIRTY_BACKGROUND: i32 = 21;
/// Drop caches.
pub const VM_DROP_CACHES: i32 = 36;

// ---------------------------------------------------------------------------
// CTL_NET subcategories
// ---------------------------------------------------------------------------

/// Core networking.
pub const NET_CORE: i32 = 1;
/// IPv4.
pub const NET_IPV4: i32 = 5;
/// IPv6.
pub const NET_IPV6: i32 = 12;
/// Unix sockets.
pub const NET_UNIX: i32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_top_level_distinct() {
        let cats = [
            CTL_KERN, CTL_VM, CTL_NET, CTL_PROC,
            CTL_FS, CTL_DEBUG, CTL_DEV,
        ];
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_kern_subcats() {
        assert_eq!(KERN_OSTYPE, 1);
        assert_eq!(KERN_OSRELEASE, 2);
        assert_eq!(KERN_VERSION, 4);
    }

    #[test]
    fn test_vm_subcats() {
        assert_eq!(VM_OVERCOMMIT_MEMORY, 5);
        assert_ne!(VM_SWAPPINESS, VM_DIRTY_RATIO);
    }

    #[test]
    fn test_net_subcats_distinct() {
        let nets = [NET_CORE, NET_IPV4, NET_IPV6, NET_UNIX];
        for i in 0..nets.len() {
            for j in (i + 1)..nets.len() {
                assert_ne!(nets[i], nets[j]);
            }
        }
    }
}
