//! `<linux/nfc.h>` — NFC (Near Field Communication) constants.
//!
//! The Linux NFC subsystem provides a unified interface for NFC
//! controllers (NCI, HCI, NFC-DEP). Applications use AF_NFC sockets
//! or the netlink interface to discover NFC tags, read/write NDEF
//! messages, and establish peer-to-peer connections. The netlink
//! interface (NFC_CMD_*) manages device discovery, target activation,
//! and secure element access. Used for contactless payments (NFC-A/B),
//! tag reading (MIFARE, NTAG), and device pairing.

// ---------------------------------------------------------------------------
// NFC netlink commands (NFC_CMD_*)
// ---------------------------------------------------------------------------

/// Get NFC device info.
pub const NFC_CMD_GET_DEVICE: u32 = 1;
/// Start device poll (discovery).
pub const NFC_CMD_DEV_UP: u32 = 2;
/// Stop device poll.
pub const NFC_CMD_DEV_DOWN: u32 = 3;
/// Start NFC target polling.
pub const NFC_CMD_START_POLL: u32 = 4;
/// Stop NFC target polling.
pub const NFC_CMD_STOP_POLL: u32 = 5;
/// Get discovered targets.
pub const NFC_CMD_GET_TARGET: u32 = 6;
/// Activate an NFC-DEP (peer-to-peer) target.
pub const NFC_CMD_DEP_LINK_UP: u32 = 7;
/// Deactivate NFC-DEP link.
pub const NFC_CMD_DEP_LINK_DOWN: u32 = 8;
/// Target discovered event.
pub const NFC_CMD_TARGETS_FOUND: u32 = 9;
/// Target lost event.
pub const NFC_CMD_TARGET_LOST: u32 = 10;
/// LLC (Logical Link Control) get parameters.
pub const NFC_CMD_LLC_GET_PARAMS: u32 = 11;
/// LLC set parameters.
pub const NFC_CMD_LLC_SET_PARAMS: u32 = 12;
/// Enable secure element.
pub const NFC_CMD_ENABLE_SE: u32 = 13;
/// Disable secure element.
pub const NFC_CMD_DISABLE_SE: u32 = 14;
/// Get secure element list.
pub const NFC_CMD_GET_SE: u32 = 15;
/// SE (secure element) I/O transaction.
pub const NFC_CMD_SE_IO: u32 = 16;
/// Activate target.
pub const NFC_CMD_ACTIVATE_TARGET: u32 = 17;
/// Deactivate target.
pub const NFC_CMD_DEACTIVATE_TARGET: u32 = 18;
/// Vendor-specific command.
pub const NFC_CMD_VENDOR: u32 = 19;

// ---------------------------------------------------------------------------
// NFC protocols
// ---------------------------------------------------------------------------

/// NFC-A (ISO 14443A) — MIFARE, NTAG.
pub const NFC_PROTO_MIFARE: u32 = 0;
/// NFC-A/4A (ISO 14443-4A).
pub const NFC_PROTO_ISO14443: u32 = 1;
/// NFC-B (ISO 14443B).
pub const NFC_PROTO_ISO14443_B: u32 = 2;
/// NFC-F (FeliCa, JIS X 6319-4).
pub const NFC_PROTO_FELICA: u32 = 3;
/// NFC-V (ISO 15693, vicinity cards).
pub const NFC_PROTO_ISO15693: u32 = 4;
/// NFC-DEP (peer-to-peer protocol).
pub const NFC_PROTO_NFC_DEP: u32 = 5;
/// ISO 14443-4B (NFC-B with ISO-DEP).
pub const NFC_PROTO_ISO14443_B2: u32 = 6;

// ---------------------------------------------------------------------------
// NFC poll modes (bitmask)
// ---------------------------------------------------------------------------

/// Poll for NFC-A targets (initiator mode).
pub const NFC_POLL_NFC_A: u32 = 1 << 0;
/// Poll for NFC-B targets.
pub const NFC_POLL_NFC_B: u32 = 1 << 1;
/// Poll for NFC-F targets.
pub const NFC_POLL_NFC_F: u32 = 1 << 2;
/// Poll for NFC-V targets.
pub const NFC_POLL_NFC_V: u32 = 1 << 3;
/// Listen for NFC-DEP (target mode).
pub const NFC_POLL_NFC_DEP: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// NFC communication modes
// ---------------------------------------------------------------------------

/// Passive communication mode (target powered by field).
pub const NFC_COMM_PASSIVE: u32 = 0;
/// Active communication mode (both generate field).
pub const NFC_COMM_ACTIVE: u32 = 1;

// ---------------------------------------------------------------------------
// Secure element types
// ---------------------------------------------------------------------------

/// UICC (SIM card) secure element.
pub const NFC_SE_UICC: u32 = 1;
/// Embedded secure element.
pub const NFC_SE_EMBEDDED: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            NFC_CMD_GET_DEVICE, NFC_CMD_DEV_UP, NFC_CMD_DEV_DOWN,
            NFC_CMD_START_POLL, NFC_CMD_STOP_POLL, NFC_CMD_GET_TARGET,
            NFC_CMD_DEP_LINK_UP, NFC_CMD_DEP_LINK_DOWN,
            NFC_CMD_TARGETS_FOUND, NFC_CMD_TARGET_LOST,
            NFC_CMD_LLC_GET_PARAMS, NFC_CMD_LLC_SET_PARAMS,
            NFC_CMD_ENABLE_SE, NFC_CMD_DISABLE_SE, NFC_CMD_GET_SE,
            NFC_CMD_SE_IO, NFC_CMD_ACTIVATE_TARGET,
            NFC_CMD_DEACTIVATE_TARGET, NFC_CMD_VENDOR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            NFC_PROTO_MIFARE, NFC_PROTO_ISO14443,
            NFC_PROTO_ISO14443_B, NFC_PROTO_FELICA,
            NFC_PROTO_ISO15693, NFC_PROTO_NFC_DEP,
            NFC_PROTO_ISO14443_B2,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_poll_modes_no_overlap() {
        let modes = [
            NFC_POLL_NFC_A, NFC_POLL_NFC_B, NFC_POLL_NFC_F,
            NFC_POLL_NFC_V, NFC_POLL_NFC_DEP,
        ];
        for i in 0..modes.len() {
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_comm_modes_distinct() {
        assert_ne!(NFC_COMM_PASSIVE, NFC_COMM_ACTIVE);
    }

    #[test]
    fn test_se_types_distinct() {
        assert_ne!(NFC_SE_UICC, NFC_SE_EMBEDDED);
    }
}
