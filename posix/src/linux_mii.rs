//! `<linux/mii.h>` — MII (Media Independent Interface) PHY register definitions.
//!
//! MII is the standard interface between Ethernet MAC controllers and
//! PHY transceivers. These register definitions are used by ethtool,
//! network drivers, and diagnostic tools to query and configure PHY
//! hardware (link speed, duplex, auto-negotiation, etc.).

// ---------------------------------------------------------------------------
// MII register addresses
// ---------------------------------------------------------------------------

/// Basic Mode Control Register.
pub const MII_BMCR: u8 = 0x00;
/// Basic Mode Status Register.
pub const MII_BMSR: u8 = 0x01;
/// PHY Identifier 1.
pub const MII_PHYSID1: u8 = 0x02;
/// PHY Identifier 2.
pub const MII_PHYSID2: u8 = 0x03;
/// Auto-Negotiation Advertisement.
pub const MII_ADVERTISE: u8 = 0x04;
/// Auto-Negotiation Link Partner Ability.
pub const MII_LPA: u8 = 0x05;
/// Auto-Negotiation Expansion.
pub const MII_EXPANSION: u8 = 0x06;
/// 1000BASE-T Control.
pub const MII_CTRL1000: u8 = 0x09;
/// 1000BASE-T Status.
pub const MII_STAT1000: u8 = 0x0A;
/// Extended Status.
pub const MII_ESTATUS: u8 = 0x0F;
/// Disconnect Counter.
pub const MII_DCOUNTER: u8 = 0x12;
/// False Carrier Counter.
pub const MII_FCSCOUNTER: u8 = 0x13;

// ---------------------------------------------------------------------------
// BMCR (Basic Mode Control Register) bits
// ---------------------------------------------------------------------------

/// Reset PHY.
pub const BMCR_RESET: u16 = 0x8000;
/// Enable loopback.
pub const BMCR_LOOPBACK: u16 = 0x4000;
/// Speed select (100 Mbps).
pub const BMCR_SPEED100: u16 = 0x2000;
/// Enable auto-negotiation.
pub const BMCR_ANENABLE: u16 = 0x1000;
/// Power down.
pub const BMCR_PDOWN: u16 = 0x0800;
/// Isolate PHY.
pub const BMCR_ISOLATE: u16 = 0x0400;
/// Restart auto-negotiation.
pub const BMCR_ANRESTART: u16 = 0x0200;
/// Full duplex.
pub const BMCR_FULLDPLX: u16 = 0x0100;
/// Collision test.
pub const BMCR_CTST: u16 = 0x0080;
/// Speed select (1000 Mbps).
pub const BMCR_SPEED1000: u16 = 0x0040;

// ---------------------------------------------------------------------------
// BMSR (Basic Mode Status Register) bits
// ---------------------------------------------------------------------------

/// Extended capability.
pub const BMSR_ERCAP: u16 = 0x0001;
/// Jabber detected.
pub const BMSR_JCD: u16 = 0x0002;
/// Link status.
pub const BMSR_LSTATUS: u16 = 0x0004;
/// Auto-negotiation capable.
pub const BMSR_ANEGCAPABLE: u16 = 0x0008;
/// Remote fault.
pub const BMSR_RFAULT: u16 = 0x0010;
/// Auto-negotiation complete.
pub const BMSR_ANEGCOMPLETE: u16 = 0x0020;
/// 10 Mbps half duplex.
pub const BMSR_10HALF: u16 = 0x0800;
/// 10 Mbps full duplex.
pub const BMSR_10FULL: u16 = 0x1000;
/// 100 Mbps half duplex.
pub const BMSR_100HALF: u16 = 0x2000;
/// 100 Mbps full duplex.
pub const BMSR_100FULL: u16 = 0x4000;
/// 100BASE-T4 capable.
pub const BMSR_100BASE4: u16 = 0x8000;

// ---------------------------------------------------------------------------
// Advertisement register bits
// ---------------------------------------------------------------------------

/// IEEE 802.3 selector.
pub const ADVERTISE_CSMA: u16 = 0x0001;
/// 10BASE-T half duplex.
pub const ADVERTISE_10HALF: u16 = 0x0020;
/// 10BASE-T full duplex.
pub const ADVERTISE_10FULL: u16 = 0x0040;
/// 100BASE-TX half duplex.
pub const ADVERTISE_100HALF: u16 = 0x0080;
/// 100BASE-TX full duplex.
pub const ADVERTISE_100FULL: u16 = 0x0100;
/// 100BASE-T4.
pub const ADVERTISE_100BASE4: u16 = 0x0200;
/// Pause capability.
pub const ADVERTISE_PAUSE_CAP: u16 = 0x0400;
/// Asymmetric pause.
pub const ADVERTISE_PAUSE_ASYM: u16 = 0x0800;

// ---------------------------------------------------------------------------
// 1000BASE-T control/status
// ---------------------------------------------------------------------------

/// Advertise 1000BASE-T half duplex.
pub const ADVERTISE_1000HALF: u16 = 0x0100;
/// Advertise 1000BASE-T full duplex.
pub const ADVERTISE_1000FULL: u16 = 0x0200;

/// Link partner 1000BASE-T half duplex.
pub const LPA_1000HALF: u16 = 0x0400;
/// Link partner 1000BASE-T full duplex.
pub const LPA_1000FULL: u16 = 0x0800;

// ---------------------------------------------------------------------------
// Generic MII ioctl structure
// ---------------------------------------------------------------------------

/// MII ioctl data structure (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MiiIoctlData {
    /// PHY address.
    pub phy_id: u16,
    /// Register number.
    pub reg_num: u16,
    /// Input value.
    pub val_in: u16,
    /// Output value.
    pub val_out: u16,
}

impl MiiIoctlData {
    /// Create a zeroed MII ioctl data.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_addresses() {
        assert_eq!(MII_BMCR, 0x00);
        assert_eq!(MII_BMSR, 0x01);
        assert_eq!(MII_PHYSID1, 0x02);
        assert_eq!(MII_PHYSID2, 0x03);
        assert_eq!(MII_ADVERTISE, 0x04);
        assert_eq!(MII_LPA, 0x05);
    }

    #[test]
    fn test_bmcr_bits_are_powers_of_two() {
        let bits = [
            BMCR_RESET,
            BMCR_LOOPBACK,
            BMCR_SPEED100,
            BMCR_ANENABLE,
            BMCR_PDOWN,
            BMCR_ISOLATE,
            BMCR_ANRESTART,
            BMCR_FULLDPLX,
            BMCR_CTST,
            BMCR_SPEED1000,
        ];
        for b in &bits {
            assert!(b.is_power_of_two(), "BMCR bit {b:#06x} not power of 2");
        }
    }

    #[test]
    fn test_bmsr_link_status() {
        assert_eq!(BMSR_LSTATUS, 0x0004);
        assert_eq!(BMSR_ANEGCOMPLETE, 0x0020);
    }

    #[test]
    fn test_mii_ioctl_data_size() {
        assert_eq!(core::mem::size_of::<MiiIoctlData>(), 8);
    }

    #[test]
    fn test_advertise_speeds_distinct() {
        let speeds = [
            ADVERTISE_CSMA,
            ADVERTISE_10HALF,
            ADVERTISE_10FULL,
            ADVERTISE_100HALF,
            ADVERTISE_100FULL,
            ADVERTISE_100BASE4,
            ADVERTISE_PAUSE_CAP,
            ADVERTISE_PAUSE_ASYM,
        ];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }

    #[test]
    fn test_registers_distinct() {
        let regs = [
            MII_BMCR,
            MII_BMSR,
            MII_PHYSID1,
            MII_PHYSID2,
            MII_ADVERTISE,
            MII_LPA,
            MII_EXPANSION,
            MII_CTRL1000,
            MII_STAT1000,
            MII_ESTATUS,
        ];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
    }
}
