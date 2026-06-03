//! `<linux/netpoll.h>` — netpoll / netconsole interface.
//!
//! netpoll is the kernel's "send a packet even when the network stack
//! is jammed or held inside a softirq" mechanism. It's the substrate
//! for `netconsole` (kernel printk over UDP), the in-kernel KGDBoE
//! debugger, and was historically used by `kgdb-light`. The control
//! plane is a sysfs configfs tree at `/sys/kernel/config/netconsole`.

// ---------------------------------------------------------------------------
// Compile-time constants
// ---------------------------------------------------------------------------

/// Maximum length of a target name in configfs.
pub const NETCONSOLE_PARAM_TARGET_MAXLEN: usize = 32;

/// Default UDP source port (chosen by historical default; admin overrides).
pub const NETCONSOLE_DEFAULT_PORT: u16 = 6665;

/// Default UDP destination port.
pub const NETCONSOLE_DEFAULT_REMOTE_PORT: u16 = 6666;

/// Hard cap on the textual loglevel field in `netconsole.cmdline`.
pub const NETCONSOLE_LOGLEVEL_MAX: u32 = 7;

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

pub const CONFIGFS_NETCONSOLE_ROOT: &str = "/sys/kernel/config/netconsole";
pub const SYSFS_NETCONSOLE_ENABLED: &str = "enabled";
pub const SYSFS_NETCONSOLE_RELEASE: &str = "release";
pub const SYSFS_NETCONSOLE_DEV_NAME: &str = "dev_name";
pub const SYSFS_NETCONSOLE_LOCAL_PORT: &str = "local_port";
pub const SYSFS_NETCONSOLE_REMOTE_PORT: &str = "remote_port";
pub const SYSFS_NETCONSOLE_LOCAL_IP: &str = "local_ip";
pub const SYSFS_NETCONSOLE_REMOTE_IP: &str = "remote_ip";
pub const SYSFS_NETCONSOLE_LOCAL_MAC: &str = "local_mac";
pub const SYSFS_NETCONSOLE_REMOTE_MAC: &str = "remote_mac";
pub const SYSFS_NETCONSOLE_USERDATA: &str = "userdata";

// ---------------------------------------------------------------------------
// netpoll target structure flags (`netpoll_targets.flags`)
// ---------------------------------------------------------------------------

pub const NETPOLL_F_SKB_RESERVE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// netpoll receive-callback return values
// ---------------------------------------------------------------------------

pub const NETPOLL_RX_DROP: u32 = 0;
pub const NETPOLL_RX_OK: u32 = 1;

// ---------------------------------------------------------------------------
// netconsole format markers in the v2 (extended) on-wire protocol
// ---------------------------------------------------------------------------

/// Magic two-byte continuation indicator at the start of every extended frame.
pub const NETCONSOLE_EXT_MAGIC: &[u8; 2] = b",c";

/// Maximum size of a single netconsole packet (Ethernet MTU minus headers).
pub const NETCONSOLE_MAX_PRINT_CHUNK: usize = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ports_distinct_and_adjacent() {
        assert_eq!(NETCONSOLE_DEFAULT_PORT, 6665);
        assert_eq!(NETCONSOLE_DEFAULT_REMOTE_PORT, 6666);
        assert_eq!(NETCONSOLE_DEFAULT_REMOTE_PORT - NETCONSOLE_DEFAULT_PORT, 1);
    }

    #[test]
    fn test_loglevel_max_in_range() {
        // Linux loglevels are 0..7 (KERN_EMERG..KERN_DEBUG).
        assert_eq!(NETCONSOLE_LOGLEVEL_MAX, 7);
    }

    #[test]
    fn test_configfs_root() {
        assert_eq!(CONFIGFS_NETCONSOLE_ROOT, "/sys/kernel/config/netconsole");
    }

    #[test]
    fn test_sysfs_attribute_names_distinct() {
        let n = [
            SYSFS_NETCONSOLE_ENABLED,
            SYSFS_NETCONSOLE_RELEASE,
            SYSFS_NETCONSOLE_DEV_NAME,
            SYSFS_NETCONSOLE_LOCAL_PORT,
            SYSFS_NETCONSOLE_REMOTE_PORT,
            SYSFS_NETCONSOLE_LOCAL_IP,
            SYSFS_NETCONSOLE_REMOTE_IP,
            SYSFS_NETCONSOLE_LOCAL_MAC,
            SYSFS_NETCONSOLE_REMOTE_MAC,
            SYSFS_NETCONSOLE_USERDATA,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
    }

    #[test]
    fn test_rx_return_values_dense_0_1() {
        assert_eq!(NETPOLL_RX_DROP, 0);
        assert_eq!(NETPOLL_RX_OK, 1);
    }

    #[test]
    fn test_ext_magic_and_chunk_size() {
        // Continuation marker is the literal ',c'.
        assert_eq!(NETCONSOLE_EXT_MAGIC, b",c");
        // 1000 bytes leaves headroom under the 1500-byte Ethernet MTU.
        assert_eq!(NETCONSOLE_MAX_PRINT_CHUNK, 1000);
        assert!(NETCONSOLE_MAX_PRINT_CHUNK < 1500);
    }

    #[test]
    fn test_skb_reserve_flag() {
        assert!(NETPOLL_F_SKB_RESERVE.is_power_of_two());
        assert_eq!(NETPOLL_F_SKB_RESERVE, 1);
    }
}
