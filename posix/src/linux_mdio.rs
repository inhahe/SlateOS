//! `<linux/mdio.h>` — MDIO (Management Data I/O) bus constants.
//!
//! MDIO is the serial management interface (IEEE 802.3 clause 22/45)
//! used to configure and monitor Ethernet PHYs. It uses a two-wire
//! interface (MDC clock + MDIO data) with a 5-bit PHY address and
//! 5-bit register address (clause 22) or 16-bit register address
//! (clause 45).

// ---------------------------------------------------------------------------
// MDIO clause 22 registers (standard MII)
// ---------------------------------------------------------------------------

/// Control register.
pub const MII_BMCR: u8 = 0x00;
/// Status register.
pub const MII_BMSR: u8 = 0x01;
/// PHY identifier 1 (OUI bits 3:18).
pub const MII_PHYSID1: u8 = 0x02;
/// PHY identifier 2 (OUI bits 19:24 + model + rev).
pub const MII_PHYSID2: u8 = 0x03;
/// Auto-negotiation advertisement.
pub const MII_ADVERTISE: u8 = 0x04;
/// Link partner ability.
pub const MII_LPA: u8 = 0x05;
/// Auto-negotiation expansion.
pub const MII_EXPANSION: u8 = 0x06;
/// 1000BASE-T control.
pub const MII_CTRL1000: u8 = 0x09;
/// 1000BASE-T status.
pub const MII_STAT1000: u8 = 0x0A;
/// Extended status.
pub const MII_ESTATUS: u8 = 0x0F;

// ---------------------------------------------------------------------------
// BMCR (Basic Mode Control Register) bits
// ---------------------------------------------------------------------------

/// Software reset.
pub const BMCR_RESET: u16 = 1 << 15;
/// Loopback mode.
pub const BMCR_LOOPBACK: u16 = 1 << 14;
/// Speed select (100 Mbps).
pub const BMCR_SPEED100: u16 = 1 << 13;
/// Auto-negotiation enable.
pub const BMCR_ANENABLE: u16 = 1 << 12;
/// Power down.
pub const BMCR_PDOWN: u16 = 1 << 11;
/// Isolate PHY from MII.
pub const BMCR_ISOLATE: u16 = 1 << 10;
/// Restart auto-negotiation.
pub const BMCR_ANRESTART: u16 = 1 << 9;
/// Full duplex.
pub const BMCR_FULLDPLX: u16 = 1 << 8;
/// Speed select (1000 Mbps, with bit 13).
pub const BMCR_SPEED1000: u16 = 1 << 6;

// ---------------------------------------------------------------------------
// BMSR (Basic Mode Status Register) bits
// ---------------------------------------------------------------------------

/// Auto-negotiation complete.
pub const BMSR_ANEGCOMPLETE: u16 = 1 << 5;
/// Link status.
pub const BMSR_LSTATUS: u16 = 1 << 2;
/// Jabber detected.
pub const BMSR_JCD: u16 = 1 << 1;
/// Extended capability.
pub const BMSR_ERCAP: u16 = 1 << 0;

// ---------------------------------------------------------------------------
// Clause 45 device types (MMD)
// ---------------------------------------------------------------------------

/// PMA/PMD (Physical Medium Attachment).
pub const MDIO_MMD_PMAPMD: u8 = 1;
/// WIS (WAN Interface Sublayer).
pub const MDIO_MMD_WIS: u8 = 2;
/// PCS (Physical Coding Sublayer).
pub const MDIO_MMD_PCS: u8 = 3;
/// PHY XS (PHY Extended Sublayer).
pub const MDIO_MMD_PHYXS: u8 = 4;
/// DTE XS (DTE Extended Sublayer).
pub const MDIO_MMD_DTEXS: u8 = 5;
/// Auto-Negotiation.
pub const MDIO_MMD_AN: u8 = 7;
/// Vendor Specific 1.
pub const MDIO_MMD_VEND1: u8 = 30;
/// Vendor Specific 2.
pub const MDIO_MMD_VEND2: u8 = 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mii_regs_distinct() {
        let regs = [
            MII_BMCR, MII_BMSR, MII_PHYSID1, MII_PHYSID2,
            MII_ADVERTISE, MII_LPA, MII_EXPANSION,
            MII_CTRL1000, MII_STAT1000, MII_ESTATUS,
        ];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
    }

    #[test]
    fn test_bmcr_bits_selected_no_overlap() {
        let bits = [
            BMCR_RESET, BMCR_LOOPBACK, BMCR_SPEED100,
            BMCR_ANENABLE, BMCR_PDOWN, BMCR_ISOLATE,
            BMCR_ANRESTART, BMCR_FULLDPLX, BMCR_SPEED1000,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_mmd_types_distinct() {
        let mmds = [
            MDIO_MMD_PMAPMD, MDIO_MMD_WIS, MDIO_MMD_PCS,
            MDIO_MMD_PHYXS, MDIO_MMD_DTEXS, MDIO_MMD_AN,
            MDIO_MMD_VEND1, MDIO_MMD_VEND2,
        ];
        for i in 0..mmds.len() {
            for j in (i + 1)..mmds.len() {
                assert_ne!(mmds[i], mmds[j]);
            }
        }
    }
}
