//! `<linux/ethtool.h>` — Ethernet NIC management userspace ABI.
//!
//! `ethtool`, `mii-tool`, ifupdown, NetworkManager and similar tools
//! drive NIC link state, ring-buffer sizes, offloads, and flow
//! steering via SIOCETHTOOL on a packet socket. The opcode names
//! below are the kernel-stable subset every userspace tool knows.

// ---------------------------------------------------------------------------
// ioctl glue
// ---------------------------------------------------------------------------

/// `SIOCETHTOOL` socket ioctl that wraps every ethtool command.
pub const SIOCETHTOOL: u32 = 0x8946;

// ---------------------------------------------------------------------------
// Common commands (struct ethtool_cmd / ethtool_value)
// ---------------------------------------------------------------------------

/// `ETHTOOL_GSET` — get settings (deprecated, use _GLINKSETTINGS).
pub const ETHTOOL_GSET: u32 = 0x0000_0001;
/// `ETHTOOL_SSET` — set settings.
pub const ETHTOOL_SSET: u32 = 0x0000_0002;
/// `ETHTOOL_GDRVINFO` — driver name and version.
pub const ETHTOOL_GDRVINFO: u32 = 0x0000_0003;
/// `ETHTOOL_GREGS` — register dump.
pub const ETHTOOL_GREGS: u32 = 0x0000_0004;
/// `ETHTOOL_GWOL` — wake-on-lan settings.
pub const ETHTOOL_GWOL: u32 = 0x0000_0005;
/// `ETHTOOL_SWOL` — set wake-on-lan settings.
pub const ETHTOOL_SWOL: u32 = 0x0000_0006;
/// `ETHTOOL_GMSGLVL` — debug message level.
pub const ETHTOOL_GMSGLVL: u32 = 0x0000_0007;
/// `ETHTOOL_SMSGLVL`.
pub const ETHTOOL_SMSGLVL: u32 = 0x0000_0008;
/// `ETHTOOL_NWAY_RST` — restart autonegotiation.
pub const ETHTOOL_NWAY_RST: u32 = 0x0000_0009;
/// `ETHTOOL_GLINK` — link up/down.
pub const ETHTOOL_GLINK: u32 = 0x0000_000a;
/// `ETHTOOL_GRINGPARAM` — ring sizes.
pub const ETHTOOL_GRINGPARAM: u32 = 0x0000_0010;
/// `ETHTOOL_SRINGPARAM`.
pub const ETHTOOL_SRINGPARAM: u32 = 0x0000_0011;
/// `ETHTOOL_GPAUSEPARAM`.
pub const ETHTOOL_GPAUSEPARAM: u32 = 0x0000_0012;
/// `ETHTOOL_SPAUSEPARAM`.
pub const ETHTOOL_SPAUSEPARAM: u32 = 0x0000_0013;
/// `ETHTOOL_GSTATS` — driver stats array.
pub const ETHTOOL_GSTATS: u32 = 0x0000_001d;
/// `ETHTOOL_GPERMADDR` — permanent hardware MAC.
pub const ETHTOOL_GPERMADDR: u32 = 0x0000_0020;
/// `ETHTOOL_GFEATURES` — feature/offload bitmap.
pub const ETHTOOL_GFEATURES: u32 = 0x0000_003a;
/// `ETHTOOL_SFEATURES` — set offloads.
pub const ETHTOOL_SFEATURES: u32 = 0x0000_003b;
/// `ETHTOOL_GCHANNELS` — combined/rx/tx channel counts.
pub const ETHTOOL_GCHANNELS: u32 = 0x0000_003c;
/// `ETHTOOL_SCHANNELS` — set channel counts.
pub const ETHTOOL_SCHANNELS: u32 = 0x0000_003d;
/// `ETHTOOL_GLINKSETTINGS` — modern speed/duplex query (cmd 0x4c).
pub const ETHTOOL_GLINKSETTINGS: u32 = 0x0000_004c;
/// `ETHTOOL_SLINKSETTINGS` — modern speed/duplex set.
pub const ETHTOOL_SLINKSETTINGS: u32 = 0x0000_004d;

// ---------------------------------------------------------------------------
// Link speeds (struct ethtool_link_settings.speed, Mbps)
// ---------------------------------------------------------------------------

/// 10 Mbit/s.
pub const SPEED_10: u32 = 10;
/// 100 Mbit/s.
pub const SPEED_100: u32 = 100;
/// 1 Gbit/s.
pub const SPEED_1000: u32 = 1000;
/// 2.5 Gbit/s.
pub const SPEED_2500: u32 = 2500;
/// 10 Gbit/s.
pub const SPEED_10000: u32 = 10_000;
/// 25 Gbit/s.
pub const SPEED_25000: u32 = 25_000;
/// 40 Gbit/s.
pub const SPEED_40000: u32 = 40_000;
/// 100 Gbit/s.
pub const SPEED_100000: u32 = 100_000;
/// 400 Gbit/s.
pub const SPEED_400000: u32 = 400_000;
/// Link speed unknown.
pub const SPEED_UNKNOWN: i32 = -1;

// ---------------------------------------------------------------------------
// Duplex
// ---------------------------------------------------------------------------

/// Half duplex.
pub const DUPLEX_HALF: u32 = 0x00;
/// Full duplex.
pub const DUPLEX_FULL: u32 = 0x01;
/// Duplex unknown.
pub const DUPLEX_UNKNOWN: u32 = 0xff;

// ---------------------------------------------------------------------------
// Wake-on-LAN modes
// ---------------------------------------------------------------------------

/// WoL on PHY activity.
pub const WAKE_PHY: u32 = 1 << 0;
/// WoL on unicast frame.
pub const WAKE_UCAST: u32 = 1 << 1;
/// WoL on multicast frame.
pub const WAKE_MCAST: u32 = 1 << 2;
/// WoL on broadcast frame.
pub const WAKE_BCAST: u32 = 1 << 3;
/// WoL on ARP request.
pub const WAKE_ARP: u32 = 1 << 4;
/// WoL on magic packet.
pub const WAKE_MAGIC: u32 = 1 << 5;
/// WoL with secure-magic password.
pub const WAKE_MAGICSECURE: u32 = 1 << 6;
/// WoL on filter match.
pub const WAKE_FILTER: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_siocethtool() {
        assert_eq!(SIOCETHTOOL, 0x8946);
    }

    #[test]
    fn test_commands_distinct() {
        let c = [
            ETHTOOL_GSET,
            ETHTOOL_SSET,
            ETHTOOL_GDRVINFO,
            ETHTOOL_GREGS,
            ETHTOOL_GWOL,
            ETHTOOL_SWOL,
            ETHTOOL_GMSGLVL,
            ETHTOOL_SMSGLVL,
            ETHTOOL_NWAY_RST,
            ETHTOOL_GLINK,
            ETHTOOL_GRINGPARAM,
            ETHTOOL_SRINGPARAM,
            ETHTOOL_GPAUSEPARAM,
            ETHTOOL_SPAUSEPARAM,
            ETHTOOL_GSTATS,
            ETHTOOL_GPERMADDR,
            ETHTOOL_GFEATURES,
            ETHTOOL_SFEATURES,
            ETHTOOL_GCHANNELS,
            ETHTOOL_SCHANNELS,
            ETHTOOL_GLINKSETTINGS,
            ETHTOOL_SLINKSETTINGS,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // GLINKSETTINGS is the modern speed/duplex query.
        assert_eq!(ETHTOOL_GLINKSETTINGS, 0x4c);
    }

    #[test]
    fn test_speeds_monotonic() {
        // Speeds must form an increasing series so userspace can pick
        // the highest negotiable rate.
        let s = [
            SPEED_10,
            SPEED_100,
            SPEED_1000,
            SPEED_2500,
            SPEED_10000,
            SPEED_25000,
            SPEED_40000,
            SPEED_100000,
            SPEED_400000,
        ];
        for w in s.windows(2) {
            assert!(w[0] < w[1]);
        }
        // -1 is the sentinel for "unknown" in i32 form.
        assert_eq!(SPEED_UNKNOWN, -1);
    }

    #[test]
    fn test_duplex_values() {
        assert_eq!(DUPLEX_HALF, 0);
        assert_eq!(DUPLEX_FULL, 1);
        // 0xff is the explicit "unknown" sentinel.
        assert_eq!(DUPLEX_UNKNOWN, 0xff);
    }

    #[test]
    fn test_wake_flags_pow2_distinct() {
        let w = [
            WAKE_PHY,
            WAKE_UCAST,
            WAKE_MCAST,
            WAKE_BCAST,
            WAKE_ARP,
            WAKE_MAGIC,
            WAKE_MAGICSECURE,
            WAKE_FILTER,
        ];
        for &b in &w {
            assert!(b.is_power_of_two());
        }
        for i in 0..w.len() {
            for j in (i + 1)..w.len() {
                assert_ne!(w[i], w[j]);
            }
        }
    }
}
