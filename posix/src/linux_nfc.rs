//! `<linux/nfc.h>` — Near Field Communication (NFC) constants.
//!
//! NFC is a short-range wireless technology for contactless payments,
//! access control, and data exchange. The Linux NFC subsystem uses
//! Generic Netlink for device management and socket-based I/O.

// ---------------------------------------------------------------------------
// NFC protocols
// ---------------------------------------------------------------------------

/// NFC-A (ISO 14443-3A, Mifare).
pub const NFC_PROTO_JEWEL: u32 = 1;
/// NFC-A Mifare.
pub const NFC_PROTO_MIFARE: u32 = 2;
/// Felica (NFC-F).
pub const NFC_PROTO_FELICA: u32 = 3;
/// ISO 14443-4 (Type A).
pub const NFC_PROTO_ISO14443: u32 = 4;
/// NFC-DEP (peer-to-peer).
pub const NFC_PROTO_NFC_DEP: u32 = 5;
/// ISO 14443-4 (Type B).
pub const NFC_PROTO_ISO14443_B: u32 = 6;
/// ISO 15693.
pub const NFC_PROTO_ISO15693: u32 = 7;

/// Maximum protocol number.
pub const NFC_PROTO_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// NFC commands (via Generic Netlink)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFC_CMD_UNSPEC: u8 = 0;
/// Get device.
pub const NFC_CMD_GET_DEVICE: u8 = 1;
/// Enable device.
pub const NFC_CMD_DEV_UP: u8 = 2;
/// Disable device.
pub const NFC_CMD_DEV_DOWN: u8 = 3;
/// Start poll.
pub const NFC_CMD_START_POLL: u8 = 5;
/// Stop poll.
pub const NFC_CMD_STOP_POLL: u8 = 6;
/// Get target.
pub const NFC_CMD_GET_TARGET: u8 = 9;
/// LLC get parameters.
pub const NFC_CMD_LLC_GET_PARAMS: u8 = 14;
/// LLC set parameters.
pub const NFC_CMD_LLC_SET_PARAMS: u8 = 15;
/// Activate target.
pub const NFC_CMD_ACTIVATE_TARGET: u8 = 18;

// ---------------------------------------------------------------------------
// NFC attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFC_ATTR_UNSPEC: u16 = 0;
/// Device index.
pub const NFC_ATTR_DEVICE_INDEX: u16 = 1;
/// Device name.
pub const NFC_ATTR_DEVICE_NAME: u16 = 2;
/// Protocols.
pub const NFC_ATTR_PROTOCOLS: u16 = 3;
/// Target index.
pub const NFC_ATTR_TARGET_INDEX: u16 = 4;
/// Target sensor serial.
pub const NFC_ATTR_TARGET_SENS_RES: u16 = 5;
/// Target selection response.
pub const NFC_ATTR_TARGET_SEL_RES: u16 = 6;

// ---------------------------------------------------------------------------
// NFC socket constants
// ---------------------------------------------------------------------------

/// NFC raw socket protocol.
pub const NFC_SOCKPROTO_RAW: u32 = 0;
/// NFC LLCP socket protocol.
pub const NFC_SOCKPROTO_LLCP: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            NFC_PROTO_JEWEL,
            NFC_PROTO_MIFARE,
            NFC_PROTO_FELICA,
            NFC_PROTO_ISO14443,
            NFC_PROTO_NFC_DEP,
            NFC_PROTO_ISO14443_B,
            NFC_PROTO_ISO15693,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            NFC_CMD_UNSPEC,
            NFC_CMD_GET_DEVICE,
            NFC_CMD_DEV_UP,
            NFC_CMD_DEV_DOWN,
            NFC_CMD_START_POLL,
            NFC_CMD_STOP_POLL,
            NFC_CMD_GET_TARGET,
            NFC_CMD_ACTIVATE_TARGET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_socket_protos() {
        assert_eq!(NFC_SOCKPROTO_RAW, 0);
        assert_eq!(NFC_SOCKPROTO_LLCP, 1);
    }
}
