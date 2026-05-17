//! `<linux/sfp.h>` — SFP (Small Form-factor Pluggable) module constants.
//!
//! SFP modules are hot-pluggable optical/electrical transceivers for
//! Ethernet. The kernel's SFP subsystem handles module detection,
//! I2C EEPROM reading (SFF-8472), and notification to phylink/MAC
//! drivers about module insertion/removal.

// ---------------------------------------------------------------------------
// SFP module types (from SFF-8024 connector type)
// ---------------------------------------------------------------------------

/// Unknown module.
pub const SFP_CONNECTOR_UNKNOWN: u8 = 0x00;
/// SC connector.
pub const SFP_CONNECTOR_SC: u8 = 0x01;
/// LC connector.
pub const SFP_CONNECTOR_LC: u8 = 0x07;
/// Optical pigtail.
pub const SFP_CONNECTOR_OPTICAL: u8 = 0x0B;
/// Copper pigtail.
pub const SFP_CONNECTOR_COPPER: u8 = 0x21;
/// RJ45 connector (copper SFP).
pub const SFP_CONNECTOR_RJ45: u8 = 0x22;
/// No separable connector.
pub const SFP_CONNECTOR_NOSEP: u8 = 0x23;

// ---------------------------------------------------------------------------
// SFP interface types
// ---------------------------------------------------------------------------

/// 1000BASE-SX (multimode fiber).
pub const SFP_IF_1000BASE_SX: u8 = 0;
/// 1000BASE-LX (single-mode fiber).
pub const SFP_IF_1000BASE_LX: u8 = 1;
/// 1000BASE-T (copper).
pub const SFP_IF_1000BASE_T: u8 = 2;
/// 10GBASE-SR (short-reach multimode).
pub const SFP_IF_10GBASE_SR: u8 = 3;
/// 10GBASE-LR (long-reach single-mode).
pub const SFP_IF_10GBASE_LR: u8 = 4;
/// 10GBASE-ER (extended-reach).
pub const SFP_IF_10GBASE_ER: u8 = 5;
/// SFP+ Direct Attach (copper twinax).
pub const SFP_IF_10G_DAC: u8 = 6;
/// 25GBASE-SR.
pub const SFP_IF_25GBASE_SR: u8 = 7;
/// 25GBASE-CR (copper).
pub const SFP_IF_25GBASE_CR: u8 = 8;

// ---------------------------------------------------------------------------
// SFP events
// ---------------------------------------------------------------------------

/// Module inserted.
pub const SFP_EVENT_INSERT: u8 = 0;
/// Module removed.
pub const SFP_EVENT_REMOVE: u8 = 1;
/// Module ready (I2C accessible).
pub const SFP_EVENT_READY: u8 = 2;
/// TX fault detected.
pub const SFP_EVENT_TX_FAULT: u8 = 3;
/// Loss of signal.
pub const SFP_EVENT_LOS: u8 = 4;

// ---------------------------------------------------------------------------
// SFP I2C addresses (SFF-8472)
// ---------------------------------------------------------------------------

/// Base EEPROM (module info, A0h).
pub const SFP_I2C_ADDR_A0: u8 = 0x50;
/// Diagnostic monitoring (A2h).
pub const SFP_I2C_ADDR_A2: u8 = 0x51;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            SFP_CONNECTOR_UNKNOWN, SFP_CONNECTOR_SC, SFP_CONNECTOR_LC,
            SFP_CONNECTOR_OPTICAL, SFP_CONNECTOR_COPPER,
            SFP_CONNECTOR_RJ45, SFP_CONNECTOR_NOSEP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_interface_types_distinct() {
        let ifs = [
            SFP_IF_1000BASE_SX, SFP_IF_1000BASE_LX, SFP_IF_1000BASE_T,
            SFP_IF_10GBASE_SR, SFP_IF_10GBASE_LR, SFP_IF_10GBASE_ER,
            SFP_IF_10G_DAC, SFP_IF_25GBASE_SR, SFP_IF_25GBASE_CR,
        ];
        for i in 0..ifs.len() {
            for j in (i + 1)..ifs.len() {
                assert_ne!(ifs[i], ifs[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            SFP_EVENT_INSERT, SFP_EVENT_REMOVE, SFP_EVENT_READY,
            SFP_EVENT_TX_FAULT, SFP_EVENT_LOS,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_i2c_addresses_distinct() {
        assert_ne!(SFP_I2C_ADDR_A0, SFP_I2C_ADDR_A2);
    }
}
