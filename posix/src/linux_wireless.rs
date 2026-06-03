//! `<linux/wireless.h>` — wireless extensions (WEXT) interface.
//!
//! Provides ioctl constants for the legacy wireless extensions API.
//! Modern programs use nl80211 (netlink) instead, but many existing
//! tools still use WEXT for basic operations.

// ---------------------------------------------------------------------------
// Wireless ioctl commands
// ---------------------------------------------------------------------------

/// Get wireless name/type.
pub const SIOCGIWNAME: u64 = 0x8B01;
/// Set wireless mode.
pub const SIOCSIWMODE: u64 = 0x8B06;
/// Get wireless mode.
pub const SIOCGIWMODE: u64 = 0x8B07;
/// Set frequency/channel.
pub const SIOCSIWFREQ: u64 = 0x8B04;
/// Get frequency/channel.
pub const SIOCGIWFREQ: u64 = 0x8B05;
/// Set ESSID (network name).
pub const SIOCSIWESSID: u64 = 0x8B1A;
/// Get ESSID.
pub const SIOCGIWESSID: u64 = 0x8B1B;
/// Set access point MAC address.
pub const SIOCSIWAP: u64 = 0x8B14;
/// Get access point MAC address.
pub const SIOCGIWAP: u64 = 0x8B15;
/// Scan for networks.
pub const SIOCSIWSCAN: u64 = 0x8B18;
/// Get scan results.
pub const SIOCGIWSCAN: u64 = 0x8B19;
/// Set transmit power.
pub const SIOCSIWTXPOW: u64 = 0x8B26;
/// Get transmit power.
pub const SIOCGIWTXPOW: u64 = 0x8B27;
/// Set sensitivity.
pub const SIOCSIWSENS: u64 = 0x8B08;
/// Get sensitivity.
pub const SIOCGIWSENS: u64 = 0x8B09;
/// Get link quality range.
pub const SIOCGIWRANGE: u64 = 0x8B0B;
/// Get statistics.
pub const SIOCGIWSTATS: u64 = 0x8B0F;
/// Set encryption key.
pub const SIOCSIWENCODE: u64 = 0x8B2A;
/// Get encryption key.
pub const SIOCGIWENCODE: u64 = 0x8B2B;
/// Set bit rate.
pub const SIOCSIWRATE: u64 = 0x8B20;
/// Get bit rate.
pub const SIOCGIWRATE: u64 = 0x8B21;
/// Set RTS threshold.
pub const SIOCSIWRTS: u64 = 0x8B22;
/// Get RTS threshold.
pub const SIOCGIWRTS: u64 = 0x8B23;

// ---------------------------------------------------------------------------
// Wireless modes
// ---------------------------------------------------------------------------

/// Auto (driver picks best mode).
pub const IW_MODE_AUTO: i32 = 0;
/// Ad-hoc (IBSS).
pub const IW_MODE_ADHOC: i32 = 1;
/// Managed (infrastructure/client).
pub const IW_MODE_INFRA: i32 = 2;
/// Master (access point).
pub const IW_MODE_MASTER: i32 = 3;
/// Repeater.
pub const IW_MODE_REPEAT: i32 = 4;
/// Secondary repeater.
pub const IW_MODE_SECOND: i32 = 5;
/// Monitor (promiscuous).
pub const IW_MODE_MONITOR: i32 = 6;
/// Mesh network.
pub const IW_MODE_MESH: i32 = 7;

// ---------------------------------------------------------------------------
// Wireless event types (for WEXT events via netlink)
// ---------------------------------------------------------------------------

/// Wireless event flag in netlink.
pub const IWEVCUSTOM: u16 = 0x8C02;
/// Scan completed event.
pub const SIOCGIWSCAN_EVENT: u16 = 0x8C01;
/// New AP event.
pub const IWEVREGISTERED: u16 = 0x8C03;
/// AP lost event.
pub const IWEVEXPIRED: u16 = 0x8C04;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum ESSID length.
pub const IW_ESSID_MAX_SIZE: usize = 32;
/// Maximum encoding key size.
pub const IW_ENCODING_TOKEN_MAX: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            SIOCGIWNAME,
            SIOCSIWMODE,
            SIOCGIWMODE,
            SIOCSIWFREQ,
            SIOCGIWFREQ,
            SIOCSIWESSID,
            SIOCGIWESSID,
            SIOCSIWAP,
            SIOCGIWAP,
            SIOCSIWSCAN,
            SIOCGIWSCAN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_modes_sequential() {
        assert_eq!(IW_MODE_AUTO, 0);
        assert_eq!(IW_MODE_ADHOC, 1);
        assert_eq!(IW_MODE_INFRA, 2);
        assert_eq!(IW_MODE_MONITOR, 6);
        assert_eq!(IW_MODE_MESH, 7);
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            IW_MODE_AUTO,
            IW_MODE_ADHOC,
            IW_MODE_INFRA,
            IW_MODE_MASTER,
            IW_MODE_REPEAT,
            IW_MODE_SECOND,
            IW_MODE_MONITOR,
            IW_MODE_MESH,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_essid_max() {
        assert_eq!(IW_ESSID_MAX_SIZE, 32);
    }

    #[test]
    fn test_get_set_pairs() {
        assert_ne!(SIOCSIWMODE, SIOCGIWMODE);
        assert_ne!(SIOCSIWESSID, SIOCGIWESSID);
        assert_ne!(SIOCSIWAP, SIOCGIWAP);
    }
}
