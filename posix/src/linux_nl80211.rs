//! `<linux/nl80211.h>` — cfg80211/nl80211 WiFi configuration constants.
//!
//! nl80211 is the Generic Netlink interface for 802.11 (WiFi)
//! configuration. Used by iw, wpa_supplicant, NetworkManager,
//! and hostapd for scanning, connecting, AP management, etc.

// ---------------------------------------------------------------------------
// nl80211 commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NL80211_CMD_UNSPEC: u8 = 0;
/// Get wireless interface.
pub const NL80211_CMD_GET_WIPHY: u8 = 1;
/// Set wireless interface.
pub const NL80211_CMD_SET_WIPHY: u8 = 2;
/// New wireless interface.
pub const NL80211_CMD_NEW_WIPHY: u8 = 3;
/// Get interface.
pub const NL80211_CMD_GET_INTERFACE: u8 = 5;
/// Set interface.
pub const NL80211_CMD_SET_INTERFACE: u8 = 6;
/// New interface.
pub const NL80211_CMD_NEW_INTERFACE: u8 = 7;
/// Delete interface.
pub const NL80211_CMD_DEL_INTERFACE: u8 = 8;
/// Get scan results.
pub const NL80211_CMD_GET_SCAN: u8 = 32;
/// Trigger scan.
pub const NL80211_CMD_TRIGGER_SCAN: u8 = 33;
/// New scan results.
pub const NL80211_CMD_NEW_SCAN_RESULTS: u8 = 34;
/// Scan aborted.
pub const NL80211_CMD_SCAN_ABORTED: u8 = 35;
/// Connect.
pub const NL80211_CMD_CONNECT: u8 = 46;
/// Disconnect.
pub const NL80211_CMD_DISCONNECT: u8 = 48;
/// Authenticate.
pub const NL80211_CMD_AUTHENTICATE: u8 = 37;
/// Associate.
pub const NL80211_CMD_ASSOCIATE: u8 = 38;
/// Deauthenticate.
pub const NL80211_CMD_DEAUTHENTICATE: u8 = 39;
/// Disassociate.
pub const NL80211_CMD_DISASSOCIATE: u8 = 40;
/// Join mesh.
pub const NL80211_CMD_JOIN_MESH: u8 = 68;
/// Leave mesh.
pub const NL80211_CMD_LEAVE_MESH: u8 = 69;
/// Start AP.
pub const NL80211_CMD_START_AP: u8 = 15;
/// Stop AP.
pub const NL80211_CMD_STOP_AP: u8 = 16;
/// Get station info.
pub const NL80211_CMD_GET_STATION: u8 = 17;
/// Get regulatory domain.
pub const NL80211_CMD_GET_REG: u8 = 49;
/// Set regulatory domain.
pub const NL80211_CMD_SET_REG: u8 = 26;

// ---------------------------------------------------------------------------
// nl80211 attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NL80211_ATTR_UNSPEC: u16 = 0;
/// Wiphy index.
pub const NL80211_ATTR_WIPHY: u16 = 1;
/// Wiphy name.
pub const NL80211_ATTR_WIPHY_NAME: u16 = 2;
/// Interface index.
pub const NL80211_ATTR_IFINDEX: u16 = 3;
/// Interface name.
pub const NL80211_ATTR_IFNAME: u16 = 4;
/// Interface type.
pub const NL80211_ATTR_IFTYPE: u16 = 5;
/// MAC address.
pub const NL80211_ATTR_MAC: u16 = 6;
/// Frequency (MHz).
pub const NL80211_ATTR_WIPHY_FREQ: u16 = 38;
/// Channel type.
pub const NL80211_ATTR_WIPHY_CHANNEL_TYPE: u16 = 39;
/// SSID.
pub const NL80211_ATTR_SSID: u16 = 52;
/// Scan frequencies.
pub const NL80211_ATTR_SCAN_FREQUENCIES: u16 = 44;
/// Scan SSIDs.
pub const NL80211_ATTR_SCAN_SSIDS: u16 = 45;
/// BSS.
pub const NL80211_ATTR_BSS: u16 = 47;
/// Regulatory alpha2 country.
pub const NL80211_ATTR_REG_ALPHA2: u16 = 16;
/// Transmit power (mBm).
pub const NL80211_ATTR_WIPHY_TX_POWER_LEVEL: u16 = 98;

// ---------------------------------------------------------------------------
// Interface types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NL80211_IFTYPE_UNSPECIFIED: u32 = 0;
/// Ad-hoc (IBSS).
pub const NL80211_IFTYPE_ADHOC: u32 = 1;
/// Managed (station/client).
pub const NL80211_IFTYPE_STATION: u32 = 2;
/// Access Point.
pub const NL80211_IFTYPE_AP: u32 = 3;
/// AP VLAN.
pub const NL80211_IFTYPE_AP_VLAN: u32 = 4;
/// Monitor.
pub const NL80211_IFTYPE_MONITOR: u32 = 6;
/// Mesh point.
pub const NL80211_IFTYPE_MESH_POINT: u32 = 7;
/// P2P client.
pub const NL80211_IFTYPE_P2P_CLIENT: u32 = 8;
/// P2P GO.
pub const NL80211_IFTYPE_P2P_GO: u32 = 9;

// ---------------------------------------------------------------------------
// Band types
// ---------------------------------------------------------------------------

/// 2.4 GHz.
pub const NL80211_BAND_2GHZ: u32 = 0;
/// 5 GHz.
pub const NL80211_BAND_5GHZ: u32 = 1;
/// 6 GHz (WiFi 6E).
pub const NL80211_BAND_6GHZ: u32 = 2;
/// 60 GHz (WiGig).
pub const NL80211_BAND_60GHZ: u32 = 3;

// ---------------------------------------------------------------------------
// Channel widths
// ---------------------------------------------------------------------------

/// 20 MHz.
pub const NL80211_CHAN_WIDTH_20_NOHT: u32 = 0;
/// 20 MHz (HT).
pub const NL80211_CHAN_WIDTH_20: u32 = 1;
/// 40 MHz.
pub const NL80211_CHAN_WIDTH_40: u32 = 2;
/// 80 MHz.
pub const NL80211_CHAN_WIDTH_80: u32 = 3;
/// 80+80 MHz.
pub const NL80211_CHAN_WIDTH_80P80: u32 = 4;
/// 160 MHz.
pub const NL80211_CHAN_WIDTH_160: u32 = 5;
/// 320 MHz (WiFi 7).
pub const NL80211_CHAN_WIDTH_320: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            NL80211_CMD_UNSPEC, NL80211_CMD_GET_WIPHY,
            NL80211_CMD_SET_WIPHY, NL80211_CMD_GET_INTERFACE,
            NL80211_CMD_TRIGGER_SCAN, NL80211_CMD_CONNECT,
            NL80211_CMD_DISCONNECT, NL80211_CMD_START_AP,
            NL80211_CMD_STOP_AP, NL80211_CMD_GET_STATION,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_iftypes_distinct() {
        let types = [
            NL80211_IFTYPE_UNSPECIFIED, NL80211_IFTYPE_ADHOC,
            NL80211_IFTYPE_STATION, NL80211_IFTYPE_AP,
            NL80211_IFTYPE_AP_VLAN, NL80211_IFTYPE_MONITOR,
            NL80211_IFTYPE_MESH_POINT, NL80211_IFTYPE_P2P_CLIENT,
            NL80211_IFTYPE_P2P_GO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_bands() {
        assert_eq!(NL80211_BAND_2GHZ, 0);
        assert_eq!(NL80211_BAND_5GHZ, 1);
        assert_eq!(NL80211_BAND_6GHZ, 2);
    }

    #[test]
    fn test_chan_widths_distinct() {
        let widths = [
            NL80211_CHAN_WIDTH_20_NOHT, NL80211_CHAN_WIDTH_20,
            NL80211_CHAN_WIDTH_40, NL80211_CHAN_WIDTH_80,
            NL80211_CHAN_WIDTH_80P80, NL80211_CHAN_WIDTH_160,
            NL80211_CHAN_WIDTH_320,
        ];
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                assert_ne!(widths[i], widths[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            NL80211_ATTR_UNSPEC, NL80211_ATTR_WIPHY,
            NL80211_ATTR_WIPHY_NAME, NL80211_ATTR_IFINDEX,
            NL80211_ATTR_IFNAME, NL80211_ATTR_IFTYPE,
            NL80211_ATTR_MAC, NL80211_ATTR_SSID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
