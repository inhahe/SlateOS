//! `<linux/mei.h>` — Intel Management Engine Interface constants.
//!
//! The Intel MEI (Management Engine Interface) provides communication
//! between the host OS and Intel ME firmware. ME runs a separate
//! embedded processor for AMT (Active Management Technology), PTT
//! (Platform Trust Technology / fTPM), PAVP (Protected Audio Video
//! Path), and other firmware services. Host connects via /dev/mei0.

// ---------------------------------------------------------------------------
// MEI ioctl commands
// ---------------------------------------------------------------------------

/// Connect to a ME client by UUID.
pub const IOCTL_MEI_CONNECT_CLIENT: u32 = 0xC010_4801;
/// Notify the client of an event.
pub const IOCTL_MEI_NOTIFY_SET: u32 = 0x4004_4802;
/// Get notification event.
pub const IOCTL_MEI_NOTIFY_GET: u32 = 0x8004_4803;

// ---------------------------------------------------------------------------
// Well-known ME client UUIDs (as 16-byte arrays)
// ---------------------------------------------------------------------------

/// AMT (Active Management Technology) client UUID bytes.
pub const MEI_UUID_AMT: [u8; 16] = [
    0x12, 0xF8, 0x02, 0x28, 0x61, 0xD2, 0x11, 0xDD, 0xAD, 0x8B, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

/// AMTHI (AMT Host Interface) client UUID bytes.
pub const MEI_UUID_AMTHI: [u8; 16] = [
    0x12, 0xF8, 0x02, 0x28, 0x61, 0xD2, 0x11, 0xDD, 0xAD, 0x8B, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];

/// MEI bus enumeration UUID bytes.
pub const MEI_UUID_BUS_ENUM: [u8; 16] = [
    0xBB, 0x8F, 0xDC, 0x6F, 0x5C, 0xAA, 0x49, 0x21, 0x80, 0x12, 0x00, 0x38, 0x5A, 0x49, 0x86, 0x03,
];

// ---------------------------------------------------------------------------
// MEI connection states
// ---------------------------------------------------------------------------

/// Client is idle (not connected).
pub const MEI_CL_STATE_IDLE: u32 = 0;
/// Client is connecting.
pub const MEI_CL_STATE_CONNECTING: u32 = 1;
/// Client is connected.
pub const MEI_CL_STATE_CONNECTED: u32 = 2;
/// Client is disconnecting.
pub const MEI_CL_STATE_DISCONNECTING: u32 = 3;

// ---------------------------------------------------------------------------
// MEI max message size
// ---------------------------------------------------------------------------

/// Maximum MEI message payload size.
pub const MEI_MAX_MSG_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
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
    fn test_states_distinct() {
        let states = [
            MEI_CL_STATE_IDLE,
            MEI_CL_STATE_CONNECTING,
            MEI_CL_STATE_CONNECTED,
            MEI_CL_STATE_DISCONNECTING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_uuids_correct_length() {
        assert_eq!(MEI_UUID_AMT.len(), 16);
        assert_eq!(MEI_UUID_BUS_ENUM.len(), 16);
    }

    #[test]
    fn test_max_msg_size() {
        assert_eq!(MEI_MAX_MSG_SIZE, 4096);
    }
}
