//! `<linux/nfc.h>` — Additional NFC constants.
//!
//! Supplementary NFC constants covering protocol types,
//! communication modes, and SE (Secure Element) types.

// ---------------------------------------------------------------------------
// NFC protocols (NFC_PROTO_*)
// ---------------------------------------------------------------------------

/// Jewel protocol.
pub const NFC_PROTO_JEWEL: u32 = 1;
/// MIFARE protocol.
pub const NFC_PROTO_MIFARE: u32 = 2;
/// Felica protocol.
pub const NFC_PROTO_FELICA: u32 = 3;
/// ISO 14443 Type A.
pub const NFC_PROTO_ISO14443: u32 = 4;
/// NFC-DEP (Data Exchange Protocol).
pub const NFC_PROTO_NFC_DEP: u32 = 5;
/// ISO 14443 Type B.
pub const NFC_PROTO_ISO14443_B: u32 = 6;
/// ISO 15693 (vicinity cards).
pub const NFC_PROTO_ISO15693: u32 = 7;

/// Maximum protocol number.
pub const NFC_PROTO_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// NFC communication modes
// ---------------------------------------------------------------------------

/// Passive initiator.
pub const NFC_COMM_PASSIVE: u32 = 0;
/// Active initiator.
pub const NFC_COMM_ACTIVE: u32 = 1;

// ---------------------------------------------------------------------------
// NFC RF technology types
// ---------------------------------------------------------------------------

/// NFC-A (Type A, 106 kbps).
pub const NFC_RF_TECH_A: u32 = 0;
/// NFC-B (Type B, 106 kbps).
pub const NFC_RF_TECH_B: u32 = 1;
/// NFC-F (FeliCa, 212/424 kbps).
pub const NFC_RF_TECH_F: u32 = 2;
/// NFC-V (ISO 15693).
pub const NFC_RF_TECH_V: u32 = 3;

// ---------------------------------------------------------------------------
// NFC Secure Element types
// ---------------------------------------------------------------------------

/// Embedded SE.
pub const NFC_SE_EMBEDDED: u32 = 0x01;
/// UICC (SIM card) SE.
pub const NFC_SE_UICC: u32 = 0x02;

// ---------------------------------------------------------------------------
// NFC genetlink commands
// ---------------------------------------------------------------------------

/// Unspec.
pub const NFC_CMD_UNSPEC: u32 = 0;
/// Get device info.
pub const NFC_CMD_GET_DEVICE: u32 = 1;
/// Start poll.
pub const NFC_CMD_START_POLL: u32 = 2;
/// Stop poll.
pub const NFC_CMD_STOP_POLL: u32 = 3;
/// DEP link up.
pub const NFC_CMD_DEP_LINK_UP: u32 = 4;
/// DEP link down.
pub const NFC_CMD_DEP_LINK_DOWN: u32 = 5;
/// Get target info.
pub const NFC_CMD_GET_TARGET: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            NFC_PROTO_JEWEL, NFC_PROTO_MIFARE, NFC_PROTO_FELICA,
            NFC_PROTO_ISO14443, NFC_PROTO_NFC_DEP,
            NFC_PROTO_ISO14443_B, NFC_PROTO_ISO15693,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_comm_modes_distinct() {
        assert_ne!(NFC_COMM_PASSIVE, NFC_COMM_ACTIVE);
    }

    #[test]
    fn test_rf_techs_distinct() {
        let techs = [NFC_RF_TECH_A, NFC_RF_TECH_B, NFC_RF_TECH_F, NFC_RF_TECH_V];
        for i in 0..techs.len() {
            for j in (i + 1)..techs.len() {
                assert_ne!(techs[i], techs[j]);
            }
        }
    }

    #[test]
    fn test_se_types_distinct() {
        assert_ne!(NFC_SE_EMBEDDED, NFC_SE_UICC);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            NFC_CMD_UNSPEC, NFC_CMD_GET_DEVICE,
            NFC_CMD_START_POLL, NFC_CMD_STOP_POLL,
            NFC_CMD_DEP_LINK_UP, NFC_CMD_DEP_LINK_DOWN,
            NFC_CMD_GET_TARGET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
