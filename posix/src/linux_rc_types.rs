//! `<linux/lirc.h>` / `<linux/input.h>` — Remote control constants.
//!
//! Constants for IR remote control protocols,
//! key map types, and RC device parameters.

// ---------------------------------------------------------------------------
// RC protocol types (RC_PROTO_*)
// ---------------------------------------------------------------------------

/// Unknown protocol.
pub const RC_PROTO_UNKNOWN: u32 = 0;
/// Other protocol.
pub const RC_PROTO_OTHER: u32 = 1;
/// RC-5.
pub const RC_PROTO_RC5: u32 = 2;
/// RC-5x (20 bits).
pub const RC_PROTO_RC5X_20: u32 = 3;
/// RC-5 SZ.
pub const RC_PROTO_RC5_SZ: u32 = 4;
/// JVC.
pub const RC_PROTO_JVC: u32 = 5;
/// Sony 12-bit.
pub const RC_PROTO_SONY12: u32 = 6;
/// Sony 15-bit.
pub const RC_PROTO_SONY15: u32 = 7;
/// Sony 20-bit.
pub const RC_PROTO_SONY20: u32 = 8;
/// NEC.
pub const RC_PROTO_NEC: u32 = 9;
/// NEC extended (NECX).
pub const RC_PROTO_NECX: u32 = 10;
/// NEC 32-bit.
pub const RC_PROTO_NEC32: u32 = 11;
/// Sanyo.
pub const RC_PROTO_SANYO: u32 = 12;
/// MCE keyboard.
pub const RC_PROTO_MCIR2_KBD: u32 = 13;
/// MCE mouse.
pub const RC_PROTO_MCIR2_MSE: u32 = 14;
/// RC-6 mode 0.
pub const RC_PROTO_RC6_0: u32 = 15;
/// RC-6 mode 6A (20 bits).
pub const RC_PROTO_RC6_6A_20: u32 = 16;
/// RC-6 mode 6A (24 bits).
pub const RC_PROTO_RC6_6A_24: u32 = 17;
/// RC-6 mode 6A (32 bits).
pub const RC_PROTO_RC6_6A_32: u32 = 18;
/// RC-6 MCE.
pub const RC_PROTO_RC6_MCE: u32 = 19;
/// Sharp.
pub const RC_PROTO_SHARP: u32 = 20;
/// Xbox DVD.
pub const RC_PROTO_XMP: u32 = 21;
/// CEC.
pub const RC_PROTO_CEC: u32 = 22;
/// Imon.
pub const RC_PROTO_IMON: u32 = 23;
/// RCMM 12.
pub const RC_PROTO_RCMM12: u32 = 24;
/// RCMM 24.
pub const RC_PROTO_RCMM24: u32 = 25;
/// RCMM 32.
pub const RC_PROTO_RCMM32: u32 = 26;

// ---------------------------------------------------------------------------
// RC driver types
// ---------------------------------------------------------------------------

/// Raw IR driver.
pub const RC_DRIVER_IR_RAW: u32 = 0;
/// Scancode driver.
pub const RC_DRIVER_SCANCODE: u32 = 1;
/// Raw IR + scancode.
pub const RC_DRIVER_IR_RAW_TX: u32 = 2;

// ---------------------------------------------------------------------------
// Key map table types
// ---------------------------------------------------------------------------

/// RC-5 key map.
pub const RC_MAP_RC5: u32 = 0;
/// RC-6 key map.
pub const RC_MAP_RC6: u32 = 1;
/// NEC key map.
pub const RC_MAP_NEC: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_sequential() {
        assert_eq!(RC_PROTO_UNKNOWN, 0);
        assert_eq!(RC_PROTO_OTHER, 1);
        assert_eq!(RC_PROTO_RCMM32, 26);
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            RC_PROTO_UNKNOWN, RC_PROTO_OTHER, RC_PROTO_RC5,
            RC_PROTO_RC5X_20, RC_PROTO_RC5_SZ, RC_PROTO_JVC,
            RC_PROTO_SONY12, RC_PROTO_SONY15, RC_PROTO_SONY20,
            RC_PROTO_NEC, RC_PROTO_NECX, RC_PROTO_NEC32,
            RC_PROTO_SANYO, RC_PROTO_MCIR2_KBD, RC_PROTO_MCIR2_MSE,
            RC_PROTO_RC6_0, RC_PROTO_RC6_6A_20, RC_PROTO_RC6_6A_24,
            RC_PROTO_RC6_6A_32, RC_PROTO_RC6_MCE, RC_PROTO_SHARP,
            RC_PROTO_XMP, RC_PROTO_CEC, RC_PROTO_IMON,
            RC_PROTO_RCMM12, RC_PROTO_RCMM24, RC_PROTO_RCMM32,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_driver_types_distinct() {
        let types = [RC_DRIVER_IR_RAW, RC_DRIVER_SCANCODE, RC_DRIVER_IR_RAW_TX];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_map_types_sequential() {
        assert_eq!(RC_MAP_RC5, 0);
        assert_eq!(RC_MAP_RC6, 1);
        assert_eq!(RC_MAP_NEC, 2);
    }
}
