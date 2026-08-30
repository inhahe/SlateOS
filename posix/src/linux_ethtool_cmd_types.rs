//! `<linux/ethtool.h>` — Ethtool ioctl command constants.
//!
//! Ethtool is the standard interface for querying and configuring
//! network interface card (NIC) parameters such as speed, duplex,
//! auto-negotiation, offload features, and ring buffer sizes.

// ---------------------------------------------------------------------------
// Ethtool ioctl commands
// ---------------------------------------------------------------------------

/// Get driver info.
pub const ETHTOOL_GDRVINFO: u32 = 0x0000_0003;
/// Get settings (speed, duplex, autoneg).
pub const ETHTOOL_GSET: u32 = 0x0000_0001;
/// Set settings.
pub const ETHTOOL_SSET: u32 = 0x0000_0002;
/// Get link status.
pub const ETHTOOL_GLINK: u32 = 0x0000_000A;
/// Get message level.
pub const ETHTOOL_GMSGLVL: u32 = 0x0000_0007;
/// Set message level.
pub const ETHTOOL_SMSGLVL: u32 = 0x0000_0008;
/// NIC self-test.
pub const ETHTOOL_TEST: u32 = 0x0000_001A;
/// Get statistics.
pub const ETHTOOL_GSTATS: u32 = 0x0000_001D;
/// Get string set.
pub const ETHTOOL_GSTRINGS: u32 = 0x0000_001B;
/// Get ring buffer parameters.
pub const ETHTOOL_GRINGPARAM: u32 = 0x0000_0010;
/// Set ring buffer parameters.
pub const ETHTOOL_SRINGPARAM: u32 = 0x0000_0011;
/// Get coalesce parameters.
pub const ETHTOOL_GCOALESCE: u32 = 0x0000_000E;
/// Set coalesce parameters.
pub const ETHTOOL_SCOALESCE: u32 = 0x0000_000F;
/// Get pause parameters.
pub const ETHTOOL_GPAUSEPARAM: u32 = 0x0000_0012;
/// Set pause parameters.
pub const ETHTOOL_SPAUSEPARAM: u32 = 0x0000_0013;
/// Get channel count.
pub const ETHTOOL_GCHANNELS: u32 = 0x0000_003C;
/// Set channel count.
pub const ETHTOOL_SCHANNELS: u32 = 0x0000_003D;
/// Get TSO (TCP segmentation offload) status.
pub const ETHTOOL_GTSO: u32 = 0x0000_001E;
/// Set TSO status.
pub const ETHTOOL_STSO: u32 = 0x0000_001F;
/// Get receive checksum offload.
pub const ETHTOOL_GRXCSUM: u32 = 0x0000_0014;
/// Get transmit checksum offload.
pub const ETHTOOL_GTXCSUM: u32 = 0x0000_0016;
/// Get generic receive offload.
pub const ETHTOOL_GGRO: u32 = 0x0000_002B;
/// Get features (combined).
pub const ETHTOOL_GFEATURES: u32 = 0x0000_003A;
/// Set features.
pub const ETHTOOL_SFEATURES: u32 = 0x0000_003B;

// ---------------------------------------------------------------------------
// Ethtool SIOCETHTOOL ioctl number
// ---------------------------------------------------------------------------

/// The ioctl request code for ethtool operations.
pub const SIOCETHTOOL: u32 = 0x8946;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            ETHTOOL_GSET,
            ETHTOOL_SSET,
            ETHTOOL_GDRVINFO,
            ETHTOOL_GLINK,
            ETHTOOL_GMSGLVL,
            ETHTOOL_SMSGLVL,
            ETHTOOL_GRINGPARAM,
            ETHTOOL_SRINGPARAM,
            ETHTOOL_GCOALESCE,
            ETHTOOL_SCOALESCE,
            ETHTOOL_GPAUSEPARAM,
            ETHTOOL_SPAUSEPARAM,
            ETHTOOL_GRXCSUM,
            ETHTOOL_GTXCSUM,
            ETHTOOL_TEST,
            ETHTOOL_GSTRINGS,
            ETHTOOL_GSTATS,
            ETHTOOL_GTSO,
            ETHTOOL_STSO,
            ETHTOOL_GGRO,
            ETHTOOL_GFEATURES,
            ETHTOOL_SFEATURES,
            ETHTOOL_GCHANNELS,
            ETHTOOL_SCHANNELS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_gset() {
        assert_eq!(ETHTOOL_GSET, 1);
    }

    #[test]
    fn test_siocethtool() {
        assert_eq!(SIOCETHTOOL, 0x8946);
    }
}
