//! `<linux/nfc.h>` — Near-Field Communication socket ABI.
//!
//! Linux added an NFC stack in 3.1 for mobile use cases (Android-style
//! tap-to-pair, contactless payments). The stack exposes raw NFC
//! sockets via `AF_NFC` and configures adapters through netlink. The
//! constants here come from `<linux/nfc.h>`.

// ---------------------------------------------------------------------------
// Address family and protocols
// ---------------------------------------------------------------------------

/// `AF_NFC` — Linux 3.1+.
pub const AF_NFC: u32 = 39;

pub const NFC_SOCKPROTO_RAW: u32 = 0;
pub const NFC_SOCKPROTO_LLCP: u32 = 1;
pub const NFC_SOCKPROTO_MAX: u32 = 2;

// ---------------------------------------------------------------------------
// `SOL_NFC` socket options
// ---------------------------------------------------------------------------

pub const NFC_LLCP_RW: u32 = 0;
pub const NFC_LLCP_MIUX: u32 = 1;
pub const NFC_LLCP_REMOTE_MIU: u32 = 2;
pub const NFC_LLCP_REMOTE_LTO: u32 = 3;
pub const NFC_LLCP_REMOTE_RW: u32 = 4;

// ---------------------------------------------------------------------------
// Genetlink family
// ---------------------------------------------------------------------------

pub const NFC_GENL_NAME: &str = "nfc";
pub const NFC_GENL_VERSION: u32 = 1;
pub const NFC_GENL_MCAST_EVENT_NAME: &str = "events";

// ---------------------------------------------------------------------------
// Commands (`enum nfc_commands`)
// ---------------------------------------------------------------------------

pub const NFC_CMD_UNSPEC: u32 = 0;
pub const NFC_CMD_GET_DEVICE: u32 = 1;
pub const NFC_CMD_DEV_UP: u32 = 2;
pub const NFC_CMD_DEV_DOWN: u32 = 3;
pub const NFC_CMD_DEP_LINK_UP: u32 = 4;
pub const NFC_CMD_DEP_LINK_DOWN: u32 = 5;
pub const NFC_CMD_START_POLL: u32 = 6;
pub const NFC_CMD_STOP_POLL: u32 = 7;
pub const NFC_CMD_GET_TARGET: u32 = 8;
pub const NFC_EVENT_TARGETS_FOUND: u32 = 9;
pub const NFC_EVENT_DEVICE_ADDED: u32 = 10;
pub const NFC_EVENT_DEVICE_REMOVED: u32 = 11;
pub const NFC_EVENT_TARGET_LOST: u32 = 12;
pub const NFC_EVENT_TM_ACTIVATED: u32 = 13;
pub const NFC_EVENT_TM_DEACTIVATED: u32 = 14;
pub const NFC_CMD_LLC_GET_PARAMS: u32 = 15;
pub const NFC_CMD_LLC_SET_PARAMS: u32 = 16;
pub const NFC_CMD_ENABLE_SE: u32 = 17;
pub const NFC_CMD_DISABLE_SE: u32 = 18;
pub const NFC_CMD_LLC_SDREQ: u32 = 19;
pub const NFC_EVENT_LLC_SDRES: u32 = 20;
pub const NFC_CMD_FW_DOWNLOAD: u32 = 21;
pub const NFC_EVENT_SE_ADDED: u32 = 22;
pub const NFC_EVENT_SE_REMOVED: u32 = 23;
pub const NFC_EVENT_SE_CONNECTIVITY: u32 = 24;
pub const NFC_EVENT_SE_TRANSACTION: u32 = 25;
pub const NFC_CMD_GET_SE: u32 = 26;
pub const NFC_CMD_SE_IO: u32 = 27;
pub const NFC_CMD_ACTIVATE_TARGET: u32 = 28;
pub const NFC_CMD_VENDOR: u32 = 29;
pub const NFC_CMD_DEACTIVATE_TARGET: u32 = 30;

// ---------------------------------------------------------------------------
// RF protocol bit values
// ---------------------------------------------------------------------------

pub const NFC_PROTO_JEWEL: u32 = 1;
pub const NFC_PROTO_MIFARE: u32 = 2;
pub const NFC_PROTO_FELICA: u32 = 3;
pub const NFC_PROTO_ISO14443: u32 = 4;
pub const NFC_PROTO_NFC_DEP: u32 = 5;
pub const NFC_PROTO_ISO14443_B: u32 = 6;
pub const NFC_PROTO_ISO15693: u32 = 7;
pub const NFC_PROTO_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// LLCP service-name maximum length
// ---------------------------------------------------------------------------

pub const NFC_LLCP_MAX_SERVICE_NAME: usize = 63;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_and_protos() {
        assert_eq!(AF_NFC, 39);
        assert_eq!(NFC_SOCKPROTO_RAW, 0);
        assert_eq!(NFC_SOCKPROTO_LLCP, 1);
        assert_eq!(NFC_SOCKPROTO_MAX, 2);
    }

    #[test]
    fn test_llcp_sockopts_dense_0_to_4() {
        let o = [
            NFC_LLCP_RW,
            NFC_LLCP_MIUX,
            NFC_LLCP_REMOTE_MIU,
            NFC_LLCP_REMOTE_LTO,
            NFC_LLCP_REMOTE_RW,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_genl_family_identity() {
        assert_eq!(NFC_GENL_NAME, "nfc");
        assert_eq!(NFC_GENL_VERSION, 1);
        assert_eq!(NFC_GENL_MCAST_EVENT_NAME, "events");
    }

    #[test]
    fn test_commands_dense_0_to_30() {
        let c = [
            NFC_CMD_UNSPEC,
            NFC_CMD_GET_DEVICE,
            NFC_CMD_DEV_UP,
            NFC_CMD_DEV_DOWN,
            NFC_CMD_DEP_LINK_UP,
            NFC_CMD_DEP_LINK_DOWN,
            NFC_CMD_START_POLL,
            NFC_CMD_STOP_POLL,
            NFC_CMD_GET_TARGET,
            NFC_EVENT_TARGETS_FOUND,
            NFC_EVENT_DEVICE_ADDED,
            NFC_EVENT_DEVICE_REMOVED,
            NFC_EVENT_TARGET_LOST,
            NFC_EVENT_TM_ACTIVATED,
            NFC_EVENT_TM_DEACTIVATED,
            NFC_CMD_LLC_GET_PARAMS,
            NFC_CMD_LLC_SET_PARAMS,
            NFC_CMD_ENABLE_SE,
            NFC_CMD_DISABLE_SE,
            NFC_CMD_LLC_SDREQ,
            NFC_EVENT_LLC_SDRES,
            NFC_CMD_FW_DOWNLOAD,
            NFC_EVENT_SE_ADDED,
            NFC_EVENT_SE_REMOVED,
            NFC_EVENT_SE_CONNECTIVITY,
            NFC_EVENT_SE_TRANSACTION,
            NFC_CMD_GET_SE,
            NFC_CMD_SE_IO,
            NFC_CMD_ACTIVATE_TARGET,
            NFC_CMD_VENDOR,
            NFC_CMD_DEACTIVATE_TARGET,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_protocols_dense_1_to_7() {
        let p = [
            NFC_PROTO_JEWEL,
            NFC_PROTO_MIFARE,
            NFC_PROTO_FELICA,
            NFC_PROTO_ISO14443,
            NFC_PROTO_NFC_DEP,
            NFC_PROTO_ISO14443_B,
            NFC_PROTO_ISO15693,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        assert_eq!(NFC_PROTO_MAX, 8);
    }

    #[test]
    fn test_llcp_service_name_cap() {
        // 63 bytes — one less than 64 so the name fits in a 64-byte struct
        // with a NUL terminator.
        assert_eq!(NFC_LLCP_MAX_SERVICE_NAME, 63);
    }
}
