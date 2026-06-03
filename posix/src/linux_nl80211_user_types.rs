//! `<linux/nl80211.h>` — 802.11 (WiFi) genetlink ABI.
//!
//! nl80211 is the modern WiFi control plane — `iw(8)`, `wpa_supplicant`,
//! `hostapd`, and NetworkManager talk to the kernel's `cfg80211` layer
//! through it. The constants here are the most commonly used subset:
//! commands, interface types, and frame-management codes.

// ---------------------------------------------------------------------------
// Genetlink family
// ---------------------------------------------------------------------------

pub const NL80211_GENL_NAME: &str = "nl80211";
pub const NL80211_MULTICAST_GROUP_CONFIG: &str = "config";
pub const NL80211_MULTICAST_GROUP_SCAN: &str = "scan";
pub const NL80211_MULTICAST_GROUP_REG: &str = "regulatory";
pub const NL80211_MULTICAST_GROUP_MLME: &str = "mlme";
pub const NL80211_MULTICAST_GROUP_VENDOR: &str = "vendor";
pub const NL80211_MULTICAST_GROUP_NAN: &str = "nan";
pub const NL80211_MULTICAST_GROUP_TESTMODE: &str = "testmode";

// ---------------------------------------------------------------------------
// Commands (subset of `enum nl80211_commands`)
// ---------------------------------------------------------------------------

pub const NL80211_CMD_UNSPEC: u32 = 0;
pub const NL80211_CMD_GET_WIPHY: u32 = 1;
pub const NL80211_CMD_SET_WIPHY: u32 = 2;
pub const NL80211_CMD_NEW_WIPHY: u32 = 3;
pub const NL80211_CMD_DEL_WIPHY: u32 = 4;
pub const NL80211_CMD_GET_INTERFACE: u32 = 5;
pub const NL80211_CMD_SET_INTERFACE: u32 = 6;
pub const NL80211_CMD_NEW_INTERFACE: u32 = 7;
pub const NL80211_CMD_DEL_INTERFACE: u32 = 8;
pub const NL80211_CMD_TRIGGER_SCAN: u32 = 33;
pub const NL80211_CMD_NEW_SCAN_RESULTS: u32 = 34;
pub const NL80211_CMD_SCAN_ABORTED: u32 = 35;
pub const NL80211_CMD_AUTHENTICATE: u32 = 37;
pub const NL80211_CMD_ASSOCIATE: u32 = 38;
pub const NL80211_CMD_DEAUTHENTICATE: u32 = 39;
pub const NL80211_CMD_DISASSOCIATE: u32 = 40;
pub const NL80211_CMD_CONNECT: u32 = 46;
pub const NL80211_CMD_ROAM: u32 = 47;
pub const NL80211_CMD_DISCONNECT: u32 = 48;

// ---------------------------------------------------------------------------
// Interface types (`enum nl80211_iftype`)
// ---------------------------------------------------------------------------

pub const NL80211_IFTYPE_UNSPECIFIED: u32 = 0;
pub const NL80211_IFTYPE_ADHOC: u32 = 1;
pub const NL80211_IFTYPE_STATION: u32 = 2;
pub const NL80211_IFTYPE_AP: u32 = 3;
pub const NL80211_IFTYPE_AP_VLAN: u32 = 4;
pub const NL80211_IFTYPE_WDS: u32 = 5;
pub const NL80211_IFTYPE_MONITOR: u32 = 6;
pub const NL80211_IFTYPE_MESH_POINT: u32 = 7;
pub const NL80211_IFTYPE_P2P_CLIENT: u32 = 8;
pub const NL80211_IFTYPE_P2P_GO: u32 = 9;
pub const NL80211_IFTYPE_P2P_DEVICE: u32 = 10;
pub const NL80211_IFTYPE_OCB: u32 = 11;
pub const NL80211_IFTYPE_NAN: u32 = 12;

// ---------------------------------------------------------------------------
// Channel widths (`enum nl80211_chan_width`)
// ---------------------------------------------------------------------------

pub const NL80211_CHAN_WIDTH_20_NOHT: u32 = 0;
pub const NL80211_CHAN_WIDTH_20: u32 = 1;
pub const NL80211_CHAN_WIDTH_40: u32 = 2;
pub const NL80211_CHAN_WIDTH_80: u32 = 3;
pub const NL80211_CHAN_WIDTH_80P80: u32 = 4;
pub const NL80211_CHAN_WIDTH_160: u32 = 5;
pub const NL80211_CHAN_WIDTH_5: u32 = 6;
pub const NL80211_CHAN_WIDTH_10: u32 = 7;
pub const NL80211_CHAN_WIDTH_1: u32 = 8;
pub const NL80211_CHAN_WIDTH_2: u32 = 9;
pub const NL80211_CHAN_WIDTH_4: u32 = 10;
pub const NL80211_CHAN_WIDTH_8: u32 = 11;
pub const NL80211_CHAN_WIDTH_16: u32 = 12;
pub const NL80211_CHAN_WIDTH_320: u32 = 13;

// ---------------------------------------------------------------------------
// 802.11 frequency-band identifiers
// ---------------------------------------------------------------------------

pub const NL80211_BAND_2GHZ: u32 = 0;
pub const NL80211_BAND_5GHZ: u32 = 1;
pub const NL80211_BAND_60GHZ: u32 = 2;
pub const NL80211_BAND_6GHZ: u32 = 3;
pub const NL80211_BAND_S1GHZ: u32 = 4;
pub const NL80211_BAND_LC: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_identity() {
        assert_eq!(NL80211_GENL_NAME, "nl80211");
    }

    #[test]
    fn test_multicast_groups_distinct() {
        let g = [
            NL80211_MULTICAST_GROUP_CONFIG,
            NL80211_MULTICAST_GROUP_SCAN,
            NL80211_MULTICAST_GROUP_REG,
            NL80211_MULTICAST_GROUP_MLME,
            NL80211_MULTICAST_GROUP_VENDOR,
            NL80211_MULTICAST_GROUP_NAN,
            NL80211_MULTICAST_GROUP_TESTMODE,
        ];
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                assert_ne!(g[i], g[j]);
            }
        }
    }

    #[test]
    fn test_wiphy_block_dense_1_to_8() {
        // WIPHY/INTERFACE management commands cluster densely.
        let c = [
            NL80211_CMD_GET_WIPHY,
            NL80211_CMD_SET_WIPHY,
            NL80211_CMD_NEW_WIPHY,
            NL80211_CMD_DEL_WIPHY,
            NL80211_CMD_GET_INTERFACE,
            NL80211_CMD_SET_INTERFACE,
            NL80211_CMD_NEW_INTERFACE,
            NL80211_CMD_DEL_INTERFACE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_iftype_dense_0_to_12() {
        let i = [
            NL80211_IFTYPE_UNSPECIFIED,
            NL80211_IFTYPE_ADHOC,
            NL80211_IFTYPE_STATION,
            NL80211_IFTYPE_AP,
            NL80211_IFTYPE_AP_VLAN,
            NL80211_IFTYPE_WDS,
            NL80211_IFTYPE_MONITOR,
            NL80211_IFTYPE_MESH_POINT,
            NL80211_IFTYPE_P2P_CLIENT,
            NL80211_IFTYPE_P2P_GO,
            NL80211_IFTYPE_P2P_DEVICE,
            NL80211_IFTYPE_OCB,
            NL80211_IFTYPE_NAN,
        ];
        for (idx, &v) in i.iter().enumerate() {
            assert_eq!(v as usize, idx);
        }
    }

    #[test]
    fn test_channel_widths_dense_0_to_13() {
        let w = [
            NL80211_CHAN_WIDTH_20_NOHT,
            NL80211_CHAN_WIDTH_20,
            NL80211_CHAN_WIDTH_40,
            NL80211_CHAN_WIDTH_80,
            NL80211_CHAN_WIDTH_80P80,
            NL80211_CHAN_WIDTH_160,
            NL80211_CHAN_WIDTH_5,
            NL80211_CHAN_WIDTH_10,
            NL80211_CHAN_WIDTH_1,
            NL80211_CHAN_WIDTH_2,
            NL80211_CHAN_WIDTH_4,
            NL80211_CHAN_WIDTH_8,
            NL80211_CHAN_WIDTH_16,
            NL80211_CHAN_WIDTH_320,
        ];
        for (i, &v) in w.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_bands_dense_0_to_5() {
        let b = [
            NL80211_BAND_2GHZ,
            NL80211_BAND_5GHZ,
            NL80211_BAND_60GHZ,
            NL80211_BAND_6GHZ,
            NL80211_BAND_S1GHZ,
            NL80211_BAND_LC,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
