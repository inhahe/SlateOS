//! `<linux/mei.h>` — Intel Management Engine Interface constants.
//!
//! MEI provides communication between the host CPU and the Intel
//! Management Engine (ME/CSME). Used for AMT, PTT (fTPM),
//! PAVP (protected audio/video), and other ME services.

// ---------------------------------------------------------------------------
// MEI ioctl commands
// ---------------------------------------------------------------------------

/// Connect to ME client by UUID.
pub const IOCTL_MEI_CONNECT_CLIENT: u32 = 0xC010_4801;

/// Notify ME client.
pub const IOCTL_MEI_NOTIFY_SET: u32 = 0x4004_4802;

/// Get notification.
pub const IOCTL_MEI_NOTIFY_GET: u32 = 0x8004_4803;

// ---------------------------------------------------------------------------
// MEI client UUIDs (well-known services)
// ---------------------------------------------------------------------------

/// AMT HECI client UUID bytes.
pub const MEI_IAMTHIF_UUID: [u8; 16] = [
    0x12, 0xf8, 0x00, 0x28, 0xb4, 0xb7, 0x4b, 0x2d, 0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
];

/// Watchdog client UUID bytes.
pub const MEI_WD_UUID: [u8; 16] = [
    0x05, 0xb7, 0x9a, 0x6f, 0x45, 0x97, 0x4c, 0x62, 0xa1, 0x4c, 0x55, 0x41, 0xf1, 0xf0, 0xa6, 0xad,
];

/// MKHI (Management Kernel Host Interface) UUID bytes.
pub const MEI_MKHI_UUID: [u8; 16] = [
    0x8e, 0x6a, 0x6e, 0x72, 0x08, 0x86, 0xa4, 0x49, 0x9c, 0x5f, 0xb3, 0xb0, 0x5d, 0xee, 0xa9, 0x4e,
];

// ---------------------------------------------------------------------------
// MEI bus message types
// ---------------------------------------------------------------------------

/// Host start request.
pub const MEI_HBM_HOST_START_REQ: u8 = 0x01;
/// Host start response.
pub const MEI_HBM_HOST_START_RES: u8 = 0x81;
/// Host stop request.
pub const MEI_HBM_HOST_STOP_REQ: u8 = 0x02;
/// Host stop response.
pub const MEI_HBM_HOST_STOP_RES: u8 = 0x82;
/// ME stop request.
pub const MEI_HBM_ME_STOP_REQ: u8 = 0x03;
/// Host enumerate request.
pub const MEI_HBM_HOST_ENUM_REQ: u8 = 0x04;
/// Host enumerate response.
pub const MEI_HBM_HOST_ENUM_RES: u8 = 0x84;
/// Client properties request.
pub const MEI_HBM_CLIENT_PROP_REQ: u8 = 0x05;
/// Client properties response.
pub const MEI_HBM_CLIENT_PROP_RES: u8 = 0x85;
/// Client connect request.
pub const MEI_HBM_CLIENT_CONNECT_REQ: u8 = 0x06;
/// Client connect response.
pub const MEI_HBM_CLIENT_CONNECT_RES: u8 = 0x86;
/// Client disconnect request.
pub const MEI_HBM_CLIENT_DISCONNECT_REQ: u8 = 0x07;
/// Client disconnect response.
pub const MEI_HBM_CLIENT_DISCONNECT_RES: u8 = 0x87;
/// Flow control.
pub const MEI_HBM_FLOW_CONTROL: u8 = 0x08;
/// Notification request.
pub const MEI_HBM_NOTIFICATION_REQ: u8 = 0x09;
/// Notification response.
pub const MEI_HBM_NOTIFICATION_RES: u8 = 0x89;

// ---------------------------------------------------------------------------
// Connection status codes
// ---------------------------------------------------------------------------

/// Success.
pub const MEI_CL_CONN_SUCCESS: u8 = 0;
/// Not found.
pub const MEI_CL_CONN_NOT_FOUND: u8 = 1;
/// Already connected.
pub const MEI_CL_CONN_ALREADY_STARTED: u8 = 2;
/// Out of resources.
pub const MEI_CL_CONN_OUT_OF_RESOURCES: u8 = 3;
/// Invalid message.
pub const MEI_CL_CONN_MESSAGE_SMALL: u8 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            IOCTL_MEI_CONNECT_CLIENT,
            IOCTL_MEI_NOTIFY_SET,
            IOCTL_MEI_NOTIFY_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_hbm_types_distinct() {
        let types = [
            MEI_HBM_HOST_START_REQ,
            MEI_HBM_HOST_START_RES,
            MEI_HBM_HOST_STOP_REQ,
            MEI_HBM_HOST_STOP_RES,
            MEI_HBM_ME_STOP_REQ,
            MEI_HBM_HOST_ENUM_REQ,
            MEI_HBM_HOST_ENUM_RES,
            MEI_HBM_CLIENT_PROP_REQ,
            MEI_HBM_CLIENT_PROP_RES,
            MEI_HBM_CLIENT_CONNECT_REQ,
            MEI_HBM_CLIENT_CONNECT_RES,
            MEI_HBM_CLIENT_DISCONNECT_REQ,
            MEI_HBM_CLIENT_DISCONNECT_RES,
            MEI_HBM_FLOW_CONTROL,
            MEI_HBM_NOTIFICATION_REQ,
            MEI_HBM_NOTIFICATION_RES,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_req_res_pattern() {
        // Response is always request | 0x80
        assert_eq!(MEI_HBM_HOST_START_RES, MEI_HBM_HOST_START_REQ | 0x80);
        assert_eq!(MEI_HBM_HOST_STOP_RES, MEI_HBM_HOST_STOP_REQ | 0x80);
        assert_eq!(MEI_HBM_HOST_ENUM_RES, MEI_HBM_HOST_ENUM_REQ | 0x80);
        assert_eq!(MEI_HBM_CLIENT_PROP_RES, MEI_HBM_CLIENT_PROP_REQ | 0x80);
    }

    #[test]
    fn test_conn_status_distinct() {
        let statuses = [
            MEI_CL_CONN_SUCCESS,
            MEI_CL_CONN_NOT_FOUND,
            MEI_CL_CONN_ALREADY_STARTED,
            MEI_CL_CONN_OUT_OF_RESOURCES,
            MEI_CL_CONN_MESSAGE_SMALL,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_uuid_lengths() {
        assert_eq!(MEI_IAMTHIF_UUID.len(), 16);
        assert_eq!(MEI_WD_UUID.len(), 16);
        assert_eq!(MEI_MKHI_UUID.len(), 16);
    }

    #[test]
    fn test_uuids_distinct() {
        assert_ne!(MEI_IAMTHIF_UUID, MEI_WD_UUID);
        assert_ne!(MEI_WD_UUID, MEI_MKHI_UUID);
        assert_ne!(MEI_IAMTHIF_UUID, MEI_MKHI_UUID);
    }
}
