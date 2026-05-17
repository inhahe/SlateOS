//! `<linux/usb/pd.h>` — USB Power Delivery (USB-PD) constants.
//!
//! USB Power Delivery is a protocol for negotiating power supply
//! over USB Type-C cables. It supports up to 240W (48V/5A) with
//! Extended Power Range, role swapping (source/sink, host/device),
//! and alternate modes (DisplayPort, Thunderbolt over USB-C).

// ---------------------------------------------------------------------------
// PD specification revisions
// ---------------------------------------------------------------------------

/// PD Revision 1.0.
pub const PD_REV10: u8 = 0;
/// PD Revision 2.0.
pub const PD_REV20: u8 = 1;
/// PD Revision 3.0.
pub const PD_REV30: u8 = 2;
/// PD Revision 3.1.
pub const PD_REV31: u8 = 3;

// ---------------------------------------------------------------------------
// PD message types (control messages)
// ---------------------------------------------------------------------------

/// GoodCRC.
pub const PD_CTRL_GOODCRC: u8 = 1;
/// GotoMin.
pub const PD_CTRL_GOTOMIN: u8 = 2;
/// Accept.
pub const PD_CTRL_ACCEPT: u8 = 3;
/// Reject.
pub const PD_CTRL_REJECT: u8 = 4;
/// Ping.
pub const PD_CTRL_PING: u8 = 5;
/// PS_RDY (power supply ready).
pub const PD_CTRL_PS_RDY: u8 = 6;
/// Get Source Cap.
pub const PD_CTRL_GET_SOURCE_CAP: u8 = 7;
/// Get Sink Cap.
pub const PD_CTRL_GET_SINK_CAP: u8 = 8;
/// DR_SWAP (data role swap).
pub const PD_CTRL_DR_SWAP: u8 = 9;
/// PR_SWAP (power role swap).
pub const PD_CTRL_PR_SWAP: u8 = 10;
/// VCONN_SWAP.
pub const PD_CTRL_VCONN_SWAP: u8 = 11;
/// Wait.
pub const PD_CTRL_WAIT: u8 = 12;
/// Soft Reset.
pub const PD_CTRL_SOFT_RESET: u8 = 13;
/// Not Supported.
pub const PD_CTRL_NOT_SUPPORTED: u8 = 16;

// ---------------------------------------------------------------------------
// PD data message types
// ---------------------------------------------------------------------------

/// Source Capabilities.
pub const PD_DATA_SOURCE_CAP: u8 = 1;
/// Request.
pub const PD_DATA_REQUEST: u8 = 2;
/// BIST.
pub const PD_DATA_BIST: u8 = 3;
/// Sink Capabilities.
pub const PD_DATA_SINK_CAP: u8 = 4;
/// Vendor Defined.
pub const PD_DATA_VENDOR_DEF: u8 = 15;

// ---------------------------------------------------------------------------
// Power supply types (in source PDOs)
// ---------------------------------------------------------------------------

/// Fixed supply.
pub const PD_PDO_TYPE_FIXED: u8 = 0;
/// Battery.
pub const PD_PDO_TYPE_BATTERY: u8 = 1;
/// Variable supply.
pub const PD_PDO_TYPE_VARIABLE: u8 = 2;
/// Augmented PDO (PPS).
pub const PD_PDO_TYPE_APDO: u8 = 3;

// ---------------------------------------------------------------------------
// Standard voltages (millivolts)
// ---------------------------------------------------------------------------

/// USB default 5V.
pub const PD_VOLTAGE_5V: u32 = 5000;
/// USB PD 9V.
pub const PD_VOLTAGE_9V: u32 = 9000;
/// USB PD 15V.
pub const PD_VOLTAGE_15V: u32 = 15000;
/// USB PD 20V.
pub const PD_VOLTAGE_20V: u32 = 20000;
/// USB PD 28V (EPR).
pub const PD_VOLTAGE_28V: u32 = 28000;
/// USB PD 36V (EPR).
pub const PD_VOLTAGE_36V: u32 = 36000;
/// USB PD 48V (EPR).
pub const PD_VOLTAGE_48V: u32 = 48000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revisions_distinct() {
        let revs = [PD_REV10, PD_REV20, PD_REV30, PD_REV31];
        for i in 0..revs.len() {
            for j in (i + 1)..revs.len() {
                assert_ne!(revs[i], revs[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_messages_distinct() {
        let msgs = [
            PD_CTRL_GOODCRC, PD_CTRL_GOTOMIN, PD_CTRL_ACCEPT,
            PD_CTRL_REJECT, PD_CTRL_PING, PD_CTRL_PS_RDY,
            PD_CTRL_GET_SOURCE_CAP, PD_CTRL_GET_SINK_CAP,
            PD_CTRL_DR_SWAP, PD_CTRL_PR_SWAP, PD_CTRL_VCONN_SWAP,
            PD_CTRL_WAIT, PD_CTRL_SOFT_RESET, PD_CTRL_NOT_SUPPORTED,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_data_messages_distinct() {
        let msgs = [
            PD_DATA_SOURCE_CAP, PD_DATA_REQUEST, PD_DATA_BIST,
            PD_DATA_SINK_CAP, PD_DATA_VENDOR_DEF,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_pdo_types_distinct() {
        let types = [PD_PDO_TYPE_FIXED, PD_PDO_TYPE_BATTERY, PD_PDO_TYPE_VARIABLE, PD_PDO_TYPE_APDO];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_voltages_ascending() {
        let volts = [
            PD_VOLTAGE_5V, PD_VOLTAGE_9V, PD_VOLTAGE_15V,
            PD_VOLTAGE_20V, PD_VOLTAGE_28V, PD_VOLTAGE_36V,
            PD_VOLTAGE_48V,
        ];
        for i in 0..(volts.len() - 1) {
            assert!(volts[i] < volts[i + 1]);
        }
    }
}
