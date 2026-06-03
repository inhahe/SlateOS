//! `<linux/mei.h>` — Intel ME (Management Engine) userspace API.
//!
//! `/dev/mei0` exposes Intel CSME firmware services (AMT, IPTS,
//! HDCP, fTPM, EC-fw). intel_amt-tools, fwupd, and OEM diagnostics
//! issue the constants below to connect to a specific ME client by
//! UUID and discover the negotiated buffer size.

// ---------------------------------------------------------------------------
// Magic letter for /dev/mei* ioctls
// ---------------------------------------------------------------------------

/// Magic byte for MEI ioctls.
pub const MEI_IOCTL_MAGIC: u8 = 0x48;

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `IOCTL_MEI_CONNECT_CLIENT` — connect to a client by UUID,
/// receive a `mei_client` structure with max_msg_length and protocol_version.
pub const IOCTL_MEI_CONNECT_CLIENT: u32 = 0xc010_4801;
/// `IOCTL_MEI_NOTIFY_SET` — enable async client-side notifications.
pub const IOCTL_MEI_NOTIFY_SET: u32 = 0x4004_4802;
/// `IOCTL_MEI_NOTIFY_GET` — block until a notification arrives.
pub const IOCTL_MEI_NOTIFY_GET: u32 = 0x8004_4803;
/// `IOCTL_MEI_CONNECT_CLIENT_VTAG` — connect with a "virtual tag"
/// (multi-instance protocol).
pub const IOCTL_MEI_CONNECT_CLIENT_VTAG: u32 = 0xc014_4804;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Length of a client GUID/UUID in bytes.
pub const MEI_CLIENT_GUID_LEN: u32 = 16;
/// Maximum number of clients tracked per MEI device.
pub const MEI_MAX_CLIENTS: u32 = 256;

// ---------------------------------------------------------------------------
// Well-known client UUIDs (low/high u64s for byte arrays of 16 bytes)
// ---------------------------------------------------------------------------

/// AMT-HID (host-initiated diagnostics) client UUID.
pub const MEI_UUID_AMT_HID: [u8; 16] = [
    0xe2, 0xfd, 0x82, 0x05, 0x66, 0xb6, 0x95, 0x4e,
    0xae, 0x5c, 0xed, 0x46, 0xea, 0x91, 0xf3, 0x37,
];
/// HECI heartbeat (firmware-version) client UUID — the canonical
/// "is the ME up" client.
pub const MEI_UUID_HBM: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// MKHI (Management Kernel Host Interface) client UUID.
pub const MEI_UUID_MKHI: [u8; 16] = [
    0xf2, 0x40, 0x35, 0x55, 0x66, 0x53, 0x67, 0x65,
    0xa3, 0xa5, 0xed, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5,
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_byte() {
        // 0x48 ('H' for HECI) — the historical MEI ioctl group.
        assert_eq!(MEI_IOCTL_MAGIC, b'H');
    }

    #[test]
    fn test_ioctls_distinct_and_use_magic_h() {
        let ops = [
            IOCTL_MEI_CONNECT_CLIENT,
            IOCTL_MEI_NOTIFY_SET,
            IOCTL_MEI_NOTIFY_GET,
            IOCTL_MEI_CONNECT_CLIENT_VTAG,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'H' (0x48) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'H' as u32);
        }
    }

    #[test]
    fn test_limits() {
        // GUID length is fixed at 16 (DCE / RFC 4122).
        assert_eq!(MEI_CLIENT_GUID_LEN, 16);
        assert!(MEI_MAX_CLIENTS.is_power_of_two());
    }

    #[test]
    fn test_guids_have_correct_length() {
        assert_eq!(MEI_UUID_AMT_HID.len(), MEI_CLIENT_GUID_LEN as usize);
        assert_eq!(MEI_UUID_HBM.len(), MEI_CLIENT_GUID_LEN as usize);
        assert_eq!(MEI_UUID_MKHI.len(), MEI_CLIENT_GUID_LEN as usize);
        // Non-trivial UUIDs must differ from each other.
        assert_ne!(MEI_UUID_AMT_HID, MEI_UUID_MKHI);
    }
}
