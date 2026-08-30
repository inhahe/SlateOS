//! `<linux/mdio.h>` — MDIO/MII PHY register constants.
//!
//! MDIO (Management Data Input/Output) is the management bus for
//! Ethernet PHY chips. It provides standardized register access
//! (IEEE 802.3 clause 22/45) for configuring link speed, duplex,
//! auto-negotiation, and reading link status.

// ---------------------------------------------------------------------------
// MII register addresses (clause 22, 5-bit)
// ---------------------------------------------------------------------------

/// Basic Mode Control Register.
pub const MII_BMCR: u8 = 0x00;
/// Basic Mode Status Register.
pub const MII_BMSR: u8 = 0x01;
/// PHY Identifier 1 (OUI bits 3-18).
pub const MII_PHYSID1: u8 = 0x02;
/// PHY Identifier 2 (OUI bits 19-24, model, revision).
pub const MII_PHYSID2: u8 = 0x03;
/// Auto-Negotiation Advertisement Register.
pub const MII_ADVERTISE: u8 = 0x04;
/// Auto-Negotiation Link Partner Ability Register.
pub const MII_LPA: u8 = 0x05;
/// Auto-Negotiation Expansion Register.
pub const MII_EXPANSION: u8 = 0x06;
/// 1000BASE-T Control Register.
pub const MII_CTRL1000: u8 = 0x09;
/// 1000BASE-T Status Register.
pub const MII_STAT1000: u8 = 0x0A;
/// Extended Status Register.
pub const MII_ESTATUS: u8 = 0x0F;

// ---------------------------------------------------------------------------
// BMCR (Basic Mode Control Register) bits
// ---------------------------------------------------------------------------

/// Software reset.
pub const BMCR_RESET: u16 = 0x8000;
/// Loopback mode.
pub const BMCR_LOOPBACK: u16 = 0x4000;
/// Speed select (100 Mbps when set with bit 6 clear).
pub const BMCR_SPEED100: u16 = 0x2000;
/// Enable auto-negotiation.
pub const BMCR_ANENABLE: u16 = 0x1000;
/// Power down PHY.
pub const BMCR_PDOWN: u16 = 0x0800;
/// Isolate PHY from MII.
pub const BMCR_ISOLATE: u16 = 0x0400;
/// Restart auto-negotiation.
pub const BMCR_ANRESTART: u16 = 0x0200;
/// Full duplex mode.
pub const BMCR_FULLDPLX: u16 = 0x0100;
/// Speed select MSB (1000 Mbps when set).
pub const BMCR_SPEED1000: u16 = 0x0040;

// ---------------------------------------------------------------------------
// BMSR (Basic Mode Status Register) bits
// ---------------------------------------------------------------------------

/// Link is up.
pub const BMSR_LSTATUS: u16 = 0x0004;
/// Auto-negotiation complete.
pub const BMSR_ANEGCOMPLETE: u16 = 0x0020;
/// Can do 10 Mbps half-duplex.
pub const BMSR_10HALF: u16 = 0x0800;
/// Can do 10 Mbps full-duplex.
pub const BMSR_10FULL: u16 = 0x1000;
/// Can do 100 Mbps half-duplex.
pub const BMSR_100HALF: u16 = 0x2000;
/// Can do 100 Mbps full-duplex.
pub const BMSR_100FULL: u16 = 0x4000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mii_regs_distinct() {
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

    #[test]
    fn test_bmcr_bits_distinct() {
        let bits = [
            BMCR_RESET,
            BMCR_LOOPBACK,
            BMCR_SPEED100,
            BMCR_ANENABLE,
            BMCR_PDOWN,
            BMCR_ISOLATE,
            BMCR_ANRESTART,
            BMCR_FULLDPLX,
            BMCR_SPEED1000,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_link_status_bit() {
        assert_eq!(BMSR_LSTATUS, 0x0004);
    }
}
