//! `<linux/ethtool.h>` — Ethernet tool ioctls and commands.
//!
//! ethtool provides a standard interface for querying and configuring
//! Ethernet device settings (speed, duplex, wake-on-LAN, ring buffers,
//! offloads, etc.). Used by the `ethtool` command and NetworkManager.

// ---------------------------------------------------------------------------
// Ethtool ioctl
// ---------------------------------------------------------------------------

/// Ethtool ioctl command number (SIOCETHTOOL).
pub const SIOCETHTOOL: u64 = 0x8946;

// ---------------------------------------------------------------------------
// Ethtool commands (ethtool_cmd.cmd)
// ---------------------------------------------------------------------------

/// Get settings.
pub const ETHTOOL_GSET: u32 = 0x00000001;
/// Set settings.
pub const ETHTOOL_SSET: u32 = 0x00000002;
/// Get driver info.
pub const ETHTOOL_GDRVINFO: u32 = 0x00000003;
/// Get register dump.
pub const ETHTOOL_GREGS: u32 = 0x00000004;
/// Get Wake-on-LAN settings.
pub const ETHTOOL_GWOL: u32 = 0x00000005;
/// Set Wake-on-LAN settings.
pub const ETHTOOL_SWOL: u32 = 0x00000006;
/// Get message level.
pub const ETHTOOL_GMSGLVL: u32 = 0x00000007;
/// Set message level.
pub const ETHTOOL_SMSGLVL: u32 = 0x00000008;
/// Restart autonegotiation.
pub const ETHTOOL_NWAY_RST: u32 = 0x00000009;
/// Get link status.
pub const ETHTOOL_GLINK: u32 = 0x0000000A;
/// Get EEPROM data.
pub const ETHTOOL_GEEPROM: u32 = 0x0000000B;
/// Set EEPROM data.
pub const ETHTOOL_SEEPROM: u32 = 0x0000000C;
/// Get coalesce parameters.
pub const ETHTOOL_GCOALESCE: u32 = 0x0000000E;
/// Set coalesce parameters.
pub const ETHTOOL_SCOALESCE: u32 = 0x0000000F;
/// Get ring parameters.
pub const ETHTOOL_GRINGPARAM: u32 = 0x00000010;
/// Set ring parameters.
pub const ETHTOOL_SRINGPARAM: u32 = 0x00000011;
/// Get pause parameters.
pub const ETHTOOL_GPAUSEPARAM: u32 = 0x00000012;
/// Set pause parameters.
pub const ETHTOOL_SPAUSEPARAM: u32 = 0x00000013;
/// Get string set info.
pub const ETHTOOL_GSSET_INFO: u32 = 0x00000037;
/// Get statistics.
pub const ETHTOOL_GSTATS: u32 = 0x0000001D;
/// Get features.
pub const ETHTOOL_GFEATURES: u32 = 0x0000003A;
/// Set features.
pub const ETHTOOL_SFEATURES: u32 = 0x0000003B;
/// Get channels.
pub const ETHTOOL_GCHANNELS: u32 = 0x0000003C;
/// Set channels.
pub const ETHTOOL_SCHANNELS: u32 = 0x0000003D;
/// Get timestamp info.
pub const ETHTOOL_GET_TS_INFO: u32 = 0x00000041;
/// Get link settings (new API).
pub const ETHTOOL_GLINKSETTINGS: u32 = 0x0000004C;
/// Set link settings (new API).
pub const ETHTOOL_SLINKSETTINGS: u32 = 0x0000004D;

// ---------------------------------------------------------------------------
// Link speeds (Mbps)
// ---------------------------------------------------------------------------

/// 10 Mbps.
pub const SPEED_10: u32 = 10;
/// 100 Mbps.
pub const SPEED_100: u32 = 100;
/// 1 Gbps.
pub const SPEED_1000: u32 = 1000;
/// 2.5 Gbps.
pub const SPEED_2500: u32 = 2500;
/// 5 Gbps.
pub const SPEED_5000: u32 = 5000;
/// 10 Gbps.
pub const SPEED_10000: u32 = 10000;
/// 25 Gbps.
pub const SPEED_25000: u32 = 25000;
/// 40 Gbps.
pub const SPEED_40000: u32 = 40000;
/// 50 Gbps.
pub const SPEED_50000: u32 = 50000;
/// 100 Gbps.
pub const SPEED_100000: u32 = 100000;
/// Unknown speed.
pub const SPEED_UNKNOWN: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Duplex modes
// ---------------------------------------------------------------------------

/// Half duplex.
pub const DUPLEX_HALF: u8 = 0;
/// Full duplex.
pub const DUPLEX_FULL: u8 = 1;
/// Unknown duplex.
pub const DUPLEX_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Wake-on-LAN modes
// ---------------------------------------------------------------------------

/// Wake on PHY activity.
pub const WAKE_PHY: u32 = 1 << 0;
/// Wake on unicast frame.
pub const WAKE_UCAST: u32 = 1 << 1;
/// Wake on multicast frame.
pub const WAKE_MCAST: u32 = 1 << 2;
/// Wake on broadcast frame.
pub const WAKE_BCAST: u32 = 1 << 3;
/// Wake on ARP.
pub const WAKE_ARP: u32 = 1 << 4;
/// Wake on magic packet.
pub const WAKE_MAGIC: u32 = 1 << 5;
/// Wake on magic packet with password.
pub const WAKE_MAGICSECURE: u32 = 1 << 6;
/// Wake on filter.
pub const WAKE_FILTER: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            ETHTOOL_GSET, ETHTOOL_SSET, ETHTOOL_GDRVINFO,
            ETHTOOL_GWOL, ETHTOOL_SWOL, ETHTOOL_GLINK,
            ETHTOOL_GCOALESCE, ETHTOOL_SCOALESCE,
            ETHTOOL_GRINGPARAM, ETHTOOL_SRINGPARAM,
            ETHTOOL_GSTATS, ETHTOOL_GFEATURES, ETHTOOL_SFEATURES,
            ETHTOOL_GLINKSETTINGS, ETHTOOL_SLINKSETTINGS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_speeds() {
        assert_eq!(SPEED_10, 10);
        assert_eq!(SPEED_1000, 1000);
        assert_eq!(SPEED_10000, 10000);
        assert_eq!(SPEED_100000, 100000);
    }

    #[test]
    fn test_duplex() {
        assert_eq!(DUPLEX_HALF, 0);
        assert_eq!(DUPLEX_FULL, 1);
        assert_eq!(DUPLEX_UNKNOWN, 0xFF);
    }

    #[test]
    fn test_wol_flags_are_powers_of_two() {
        let flags = [
            WAKE_PHY, WAKE_UCAST, WAKE_MCAST, WAKE_BCAST,
            WAKE_ARP, WAKE_MAGIC, WAKE_MAGICSECURE, WAKE_FILTER,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "WOL flag {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_siocethtool() {
        assert_eq!(SIOCETHTOOL, 0x8946);
    }
}
