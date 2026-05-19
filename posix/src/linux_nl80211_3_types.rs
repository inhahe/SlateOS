//! `<linux/nl80211.h>` — Additional nl80211 constants (batch 3).
//!
//! Supplementary nl80211 constants covering channel widths,
//! BSS selection types, and SAE (WPA3) parameters.

// ---------------------------------------------------------------------------
// Channel widths (NL80211_CHAN_WIDTH_*)
// ---------------------------------------------------------------------------

/// 20 MHz channel width.
pub const NL80211_CHAN_WIDTH_20_NOHT: u32 = 0;
/// 20 MHz HT channel.
pub const NL80211_CHAN_WIDTH_20: u32 = 1;
/// 40 MHz channel.
pub const NL80211_CHAN_WIDTH_40: u32 = 2;
/// 80 MHz channel.
pub const NL80211_CHAN_WIDTH_80: u32 = 3;
/// 80+80 MHz channel.
pub const NL80211_CHAN_WIDTH_80P80: u32 = 4;
/// 160 MHz channel.
pub const NL80211_CHAN_WIDTH_160: u32 = 5;
/// 5 MHz channel.
pub const NL80211_CHAN_WIDTH_5: u32 = 6;
/// 10 MHz channel.
pub const NL80211_CHAN_WIDTH_10: u32 = 7;
/// 1 MHz channel (S1G).
pub const NL80211_CHAN_WIDTH_1: u32 = 8;
/// 2 MHz channel (S1G).
pub const NL80211_CHAN_WIDTH_2: u32 = 9;
/// 4 MHz channel (S1G).
pub const NL80211_CHAN_WIDTH_4: u32 = 10;
/// 8 MHz channel (S1G).
pub const NL80211_CHAN_WIDTH_8: u32 = 11;
/// 16 MHz channel (S1G).
pub const NL80211_CHAN_WIDTH_16: u32 = 12;
/// 320 MHz channel (Wi-Fi 7).
pub const NL80211_CHAN_WIDTH_320: u32 = 13;

// ---------------------------------------------------------------------------
// BSS selection types
// ---------------------------------------------------------------------------

/// Band preference.
pub const NL80211_BSS_SELECT_ATTR_BAND_PREF: u32 = 1;
/// RSSI adjust.
pub const NL80211_BSS_SELECT_ATTR_RSSI_ADJUST: u32 = 2;
/// RSSI.
pub const NL80211_BSS_SELECT_ATTR_RSSI: u32 = 3;

// ---------------------------------------------------------------------------
// SAE (WPA3) auth mechanism
// ---------------------------------------------------------------------------

/// SAE: hunting and pecking (H2E disabled).
pub const NL80211_SAE_PWE_HUNT_AND_PECK: u32 = 0;
/// SAE: hash to element.
pub const NL80211_SAE_PWE_HASH_TO_ELEMENT: u32 = 1;
/// SAE: both methods.
pub const NL80211_SAE_PWE_BOTH: u32 = 2;
/// SAE: unspecified.
pub const NL80211_SAE_PWE_UNSPECIFIED: u32 = 3;

// ---------------------------------------------------------------------------
// TID (Traffic Identifier) configuration
// ---------------------------------------------------------------------------

/// Maximum TID number (0-7 for QoS, 8-15 extended).
pub const NL80211_TID_MAX: u32 = 15;
/// Number of QoS TIDs (0-7).
pub const NL80211_TID_QOS_COUNT: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chan_widths_distinct() {
        let widths = [
            NL80211_CHAN_WIDTH_20_NOHT, NL80211_CHAN_WIDTH_20,
            NL80211_CHAN_WIDTH_40, NL80211_CHAN_WIDTH_80,
            NL80211_CHAN_WIDTH_80P80, NL80211_CHAN_WIDTH_160,
            NL80211_CHAN_WIDTH_5, NL80211_CHAN_WIDTH_10,
            NL80211_CHAN_WIDTH_1, NL80211_CHAN_WIDTH_2,
            NL80211_CHAN_WIDTH_4, NL80211_CHAN_WIDTH_8,
            NL80211_CHAN_WIDTH_16, NL80211_CHAN_WIDTH_320,
        ];
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                assert_ne!(widths[i], widths[j]);
            }
        }
    }

    #[test]
    fn test_bss_select_attrs_distinct() {
        let attrs = [
            NL80211_BSS_SELECT_ATTR_BAND_PREF,
            NL80211_BSS_SELECT_ATTR_RSSI_ADJUST,
            NL80211_BSS_SELECT_ATTR_RSSI,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_sae_pwe_distinct() {
        let modes = [
            NL80211_SAE_PWE_HUNT_AND_PECK,
            NL80211_SAE_PWE_HASH_TO_ELEMENT,
            NL80211_SAE_PWE_BOTH,
            NL80211_SAE_PWE_UNSPECIFIED,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_tid_values() {
        assert_eq!(NL80211_TID_MAX, 15);
        assert_eq!(NL80211_TID_QOS_COUNT, 8);
    }
}
