//! `<linux/wireless.h>` — legacy Wireless Extensions (WEXT) ioctls.
//!
//! WEXT is the deprecated-but-still-supported interface that `iwconfig`,
//! `iwlist`, and old `wireless_tools` use. Modern code uses nl80211
//! over netlink instead, but kernel drivers must still answer WEXT
//! ioctls for compatibility.

// ---------------------------------------------------------------------------
// `SIOCxIW*` ioctl numbers — set/get pairs in 0x8B00..0x8B30
// ---------------------------------------------------------------------------

pub const SIOCSIWCOMMIT: u32 = 0x8B00;
pub const SIOCGIWNAME: u32 = 0x8B01;
pub const SIOCSIWNWID: u32 = 0x8B02;
pub const SIOCGIWNWID: u32 = 0x8B03;
pub const SIOCSIWFREQ: u32 = 0x8B04;
pub const SIOCGIWFREQ: u32 = 0x8B05;
pub const SIOCSIWMODE: u32 = 0x8B06;
pub const SIOCGIWMODE: u32 = 0x8B07;
pub const SIOCSIWSENS: u32 = 0x8B08;
pub const SIOCGIWSENS: u32 = 0x8B09;
pub const SIOCSIWRANGE: u32 = 0x8B0A;
pub const SIOCGIWRANGE: u32 = 0x8B0B;
pub const SIOCSIWPRIV: u32 = 0x8B0C;
pub const SIOCGIWPRIV: u32 = 0x8B0D;
pub const SIOCSIWSTATS: u32 = 0x8B0E;
pub const SIOCGIWSTATS: u32 = 0x8B0F;

// ---------------------------------------------------------------------------
// `iw_mode` values (`SIOC?IWMODE`)
// ---------------------------------------------------------------------------

pub const IW_MODE_AUTO: u32 = 0;
pub const IW_MODE_ADHOC: u32 = 1;
pub const IW_MODE_INFRA: u32 = 2;
pub const IW_MODE_MASTER: u32 = 3;
pub const IW_MODE_REPEAT: u32 = 4;
pub const IW_MODE_SECOND: u32 = 5;
pub const IW_MODE_MONITOR: u32 = 6;
pub const IW_MODE_MESH: u32 = 7;

// ---------------------------------------------------------------------------
// IEEE-2.4-GHz channel/frequency mapping (Wi-Fi b/g/n channels 1..14)
// ---------------------------------------------------------------------------

/// Channel 1 center frequency in MHz.
pub const IW_FREQ_24_CH1_MHZ: u32 = 2412;
/// Spacing between adjacent channels (channels 1..13).
pub const IW_FREQ_24_SPACING_MHZ: u32 = 5;
/// Channel 14 is offset (Japan-only): 2484 MHz, not 2477.
pub const IW_FREQ_24_CH14_MHZ: u32 = 2484;

/// 5 GHz UNII-1 base channel (channel 36 = 5180 MHz, 20-MHz wide).
pub const IW_FREQ_5_CH36_MHZ: u32 = 5180;
/// 5 GHz channel spacing (4 MHz per channel number).
pub const IW_FREQ_5_SPACING_MHZ: u32 = 5;

// ---------------------------------------------------------------------------
// Encryption-key indices
// ---------------------------------------------------------------------------

pub const IW_ENCODE_INDEX_MASK: u32 = 0x00FF;
pub const IW_ENCODE_FLAGS_MASK: u32 = 0xFF00;
pub const IW_ENCODE_DISABLED: u32 = 0x8000;
pub const IW_ENCODE_ENABLED: u32 = 0x0000;
pub const IW_ENCODE_RESTRICTED: u32 = 0x4000;
pub const IW_ENCODE_OPEN: u32 = 0x2000;
pub const IW_ENCODE_NOKEY: u32 = 0x0800;
pub const IW_ENCODE_TEMP: u32 = 0x0400;

/// Maximum number of WEP keys per device.
pub const IW_ENCODE_INDEX_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sioc_pairs_adjacent_in_0x8B0x() {
        // Every Get/Set pair sits on adjacent numbers; whole block is
        // in the 0x8B00 range.
        let pairs = [
            (SIOCSIWNWID, SIOCGIWNWID),
            (SIOCSIWFREQ, SIOCGIWFREQ),
            (SIOCSIWMODE, SIOCGIWMODE),
            (SIOCSIWSENS, SIOCGIWSENS),
            (SIOCSIWRANGE, SIOCGIWRANGE),
            (SIOCSIWPRIV, SIOCGIWPRIV),
            (SIOCSIWSTATS, SIOCGIWSTATS),
        ];
        for (s, g) in pairs {
            assert_eq!(g, s + 1);
            assert_eq!(s & 0xFF00, 0x8B00);
        }
        assert_eq!(SIOCSIWCOMMIT, 0x8B00);
        assert_eq!(SIOCGIWNAME, 0x8B01);
    }

    #[test]
    fn test_iw_modes_dense_0_to_7() {
        let m = [
            IW_MODE_AUTO,
            IW_MODE_ADHOC,
            IW_MODE_INFRA,
            IW_MODE_MASTER,
            IW_MODE_REPEAT,
            IW_MODE_SECOND,
            IW_MODE_MONITOR,
            IW_MODE_MESH,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_24ghz_channel_math() {
        // Channel n in 2.4 GHz band: 2412 + 5*(n-1) MHz for n=1..13.
        assert_eq!(IW_FREQ_24_CH1_MHZ, 2412);
        // Channel 6 center freq should be 2437.
        let ch6 = IW_FREQ_24_CH1_MHZ + 5 * IW_FREQ_24_SPACING_MHZ;
        assert_eq!(ch6, 2437);
        // Channel 14 is the odd one (Japan only).
        assert_eq!(IW_FREQ_24_CH14_MHZ, 2484);
        assert!(IW_FREQ_24_CH14_MHZ > IW_FREQ_24_CH1_MHZ + 12 * IW_FREQ_24_SPACING_MHZ);
    }

    #[test]
    fn test_5ghz_channel_36_is_5180() {
        // 5 GHz UNII-1 starts at channel 36 = 5180 MHz.
        assert_eq!(IW_FREQ_5_CH36_MHZ, 5180);
        // Channel 40 should be 5200.
        let ch40 = IW_FREQ_5_CH36_MHZ + 4 * IW_FREQ_5_SPACING_MHZ;
        assert_eq!(ch40, 5200);
    }

    #[test]
    fn test_encode_masks_disjoint() {
        // Index bits (low byte) and flag bits (high byte) don't overlap.
        assert_eq!(IW_ENCODE_INDEX_MASK & IW_ENCODE_FLAGS_MASK, 0);
        assert_eq!(IW_ENCODE_INDEX_MASK | IW_ENCODE_FLAGS_MASK, 0xFFFF);
        // The flag bits all sit in the high byte.
        for v in [
            IW_ENCODE_DISABLED,
            IW_ENCODE_RESTRICTED,
            IW_ENCODE_OPEN,
            IW_ENCODE_NOKEY,
            IW_ENCODE_TEMP,
        ] {
            assert_eq!(v & IW_ENCODE_INDEX_MASK, 0);
        }
    }

    #[test]
    fn test_max_keys_is_8() {
        // The historical WEP design supported 8 keys (3-bit index).
        assert_eq!(IW_ENCODE_INDEX_MAX, 8);
    }
}
