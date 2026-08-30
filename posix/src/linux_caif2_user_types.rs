//! `<linux/caif/if_caif.h>` — CAIF netlink link configuration.
//!
//! CAIF physical-link interfaces (`cfhsi*`, `cfusb*`, `cfspi*`) are
//! created and tuned through rtnetlink with the `IFLA_CAIF_*`
//! attributes. This module captures those attribute IDs and the
//! link-flag bits.

// ---------------------------------------------------------------------------
// IFLA_CAIF_* attribute IDs
// ---------------------------------------------------------------------------

pub const IFLA_CAIF_UNSPEC: u32 = 0;
pub const IFLA_CAIF_IPV4_CONNID: u32 = 1;
pub const IFLA_CAIF_IPV6_CONNID: u32 = 2;
pub const IFLA_CAIF_LOOPBACK: u32 = 3;

// ---------------------------------------------------------------------------
// Link-layer phy-driver identifiers (`enum cfcnfg_phy_preference`)
// ---------------------------------------------------------------------------

pub const CFPHYPREF_UNSPECIFIED: u32 = 0;
pub const CFPHYPREF_LOW_LAT: u32 = 1;
pub const CFPHYPREF_HIGH_BW: u32 = 2;
pub const CFPHYPREF_LOOP: u32 = 3;

// ---------------------------------------------------------------------------
// HSI link parameters
// ---------------------------------------------------------------------------

/// Default CAIF-HSI MTU (4 KiB).
pub const CFHSI_DEFAULT_MTU: u32 = 4_096;

/// Default inactivity timeout before tearing down a HSI link (ms).
pub const CFHSI_DEFAULT_INACTIVITY_TIMEOUT_MS: u32 = 1_000;

/// Default Q-high-watermark — pause transmit when queue exceeds this.
pub const CFHSI_DEFAULT_Q_HIGH_WATERMARK: u32 = 100;

/// Default Q-low-watermark — resume transmit when queue drops below.
pub const CFHSI_DEFAULT_Q_LOW_WATERMARK: u32 = 50;

// ---------------------------------------------------------------------------
// Channel priority classes (1..7, 0 reserved for "system")
// ---------------------------------------------------------------------------

pub const CAIF_PRIO_MIN: u32 = 1;
pub const CAIF_PRIO_LOW: u32 = 2;
pub const CAIF_PRIO_NORMAL: u32 = 3;
pub const CAIF_PRIO_HIGH: u32 = 4;
pub const CAIF_PRIO_MAX: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_caif_attrs_dense_0_to_3() {
        let a = [
            IFLA_CAIF_UNSPEC,
            IFLA_CAIF_IPV4_CONNID,
            IFLA_CAIF_IPV6_CONNID,
            IFLA_CAIF_LOOPBACK,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_phy_pref_dense_0_to_3() {
        let p = [
            CFPHYPREF_UNSPECIFIED,
            CFPHYPREF_LOW_LAT,
            CFPHYPREF_HIGH_BW,
            CFPHYPREF_LOOP,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_hsi_defaults_sensible() {
        // 4 KiB matches a standard memory page on many ARM/ST platforms.
        assert_eq!(CFHSI_DEFAULT_MTU, 4_096);
        assert!(CFHSI_DEFAULT_MTU.is_power_of_two());
        // 1 second inactivity timeout.
        assert_eq!(CFHSI_DEFAULT_INACTIVITY_TIMEOUT_MS, 1_000);
        // High > Low watermark.
        assert!(CFHSI_DEFAULT_Q_HIGH_WATERMARK > CFHSI_DEFAULT_Q_LOW_WATERMARK);
        // Low watermark is exactly half the high one.
        assert_eq!(
            CFHSI_DEFAULT_Q_HIGH_WATERMARK / CFHSI_DEFAULT_Q_LOW_WATERMARK,
            2
        );
    }

    #[test]
    fn test_priority_classes_ordered() {
        assert!(CAIF_PRIO_MIN < CAIF_PRIO_LOW);
        assert!(CAIF_PRIO_LOW < CAIF_PRIO_NORMAL);
        assert!(CAIF_PRIO_NORMAL < CAIF_PRIO_HIGH);
        assert!(CAIF_PRIO_HIGH < CAIF_PRIO_MAX);
        assert_eq!(CAIF_PRIO_MIN, 1);
        assert_eq!(CAIF_PRIO_MAX, 7);
    }

    #[test]
    fn test_priority_classes_fit_in_3_bits() {
        // CAIF carries the priority class as a 3-bit field.
        for v in [
            CAIF_PRIO_MIN,
            CAIF_PRIO_LOW,
            CAIF_PRIO_NORMAL,
            CAIF_PRIO_HIGH,
            CAIF_PRIO_MAX,
        ] {
            assert!(v < (1 << 3));
        }
    }
}
