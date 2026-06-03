//! `<linux/capi.h>` — CAPI 2.0 ISDN userspace constants.
//!
//! CAPI (Common ISDN Application Programming Interface) is the
//! standard ISDN messaging API on Linux, used by AVM and other ISDN
//! card drivers. `capiinfo`, `capi-utils` and ISDN telephony stacks
//! consume these manufacturer-info and message-type constants.

// ---------------------------------------------------------------------------
// Major CAPI message types (CAPI message-header: cmd byte)
// ---------------------------------------------------------------------------

/// ALERT request/response.
pub const CAPI_ALERT: u32 = 0x01;
/// CONNECT — outbound call setup.
pub const CAPI_CONNECT: u32 = 0x02;
/// CONNECT_ACTIVE — call established.
pub const CAPI_CONNECT_ACTIVE: u32 = 0x03;
/// CONNECT_B3_ACTIVE — B-channel B3 protocol activated.
pub const CAPI_CONNECT_B3_ACTIVE: u32 = 0x83;
/// CONNECT_B3 — B-channel B3 protocol setup.
pub const CAPI_CONNECT_B3: u32 = 0x82;
/// DATA_B3 — user data on B-channel.
pub const CAPI_DATA_B3: u32 = 0x86;
/// DISCONNECT — call teardown.
pub const CAPI_DISCONNECT: u32 = 0x04;
/// DISCONNECT_B3 — B-channel teardown.
pub const CAPI_DISCONNECT_B3: u32 = 0x84;
/// FACILITY — supplementary services.
pub const CAPI_FACILITY: u32 = 0x80;
/// INFO — call progress / cause info.
pub const CAPI_INFO: u32 = 0x08;
/// LISTEN — register for incoming calls.
pub const CAPI_LISTEN: u32 = 0x05;
/// MANUFACTURER — vendor-specific message.
pub const CAPI_MANUFACTURER: u32 = 0xff;
/// RESET_B3 — protocol reset on B-channel.
pub const CAPI_RESET_B3: u32 = 0x87;
/// SELECT_B_PROTOCOL — choose layer-2 protocol on B-channel.
pub const CAPI_SELECT_B_PROTOCOL: u32 = 0x41;

// ---------------------------------------------------------------------------
// Subcommand kinds (low nibble of message header byte 5)
// ---------------------------------------------------------------------------

/// Request from application to CAPI.
pub const CAPI_REQ: u32 = 0x80;
/// Confirmation back from CAPI.
pub const CAPI_CONF: u32 = 0x81;
/// Indication from CAPI to application.
pub const CAPI_IND: u32 = 0x82;
/// Response from application.
pub const CAPI_RESP: u32 = 0x83;

// ---------------------------------------------------------------------------
// CAPI info codes (info field of CONF/IND messages)
// ---------------------------------------------------------------------------

/// No error.
pub const CAPI_NOERROR: u32 = 0x0000;
/// Too many applications.
pub const CAPI_TOOMANYAPPLS: u32 = 0x1001;
/// Application ID out of range.
pub const CAPI_LOGBLKSIZETOSMALL: u32 = 0x1002;
/// Buffer too small.
pub const CAPI_BUFFEXCECEEDS64K: u32 = 0x1003;
/// Message buffer size out of range.
pub const CAPI_MSGBUFSIZETOOSMALL: u32 = 0x1004;
/// Maximum logical applications exceeded.
pub const CAPI_ANZLOGCONNNOTSUPPORTED: u32 = 0x1005;
/// Internal busy condition.
pub const CAPI_REGRESERVED: u32 = 0x1006;

// ---------------------------------------------------------------------------
// Manufacturer-information lengths (used by CAPI_GET_MANUFACTURER ioctl)
// ---------------------------------------------------------------------------

/// Manufacturer name buffer length.
pub const CAPI_MANUFACTURER_LEN: u32 = 64;
/// Serial-number buffer length.
pub const CAPI_SERIAL_LEN: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types_distinct() {
        let msgs = [
            CAPI_ALERT,
            CAPI_CONNECT,
            CAPI_CONNECT_ACTIVE,
            CAPI_CONNECT_B3_ACTIVE,
            CAPI_CONNECT_B3,
            CAPI_DATA_B3,
            CAPI_DISCONNECT,
            CAPI_DISCONNECT_B3,
            CAPI_FACILITY,
            CAPI_INFO,
            CAPI_LISTEN,
            CAPI_MANUFACTURER,
            CAPI_RESET_B3,
            CAPI_SELECT_B_PROTOCOL,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
            assert!(msgs[i] <= 0xff);
        }
    }

    #[test]
    fn test_subcommand_kinds_distinct() {
        let kinds = [CAPI_REQ, CAPI_CONF, CAPI_IND, CAPI_RESP];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }

    #[test]
    fn test_info_codes_distinct() {
        let codes = [
            CAPI_NOERROR,
            CAPI_TOOMANYAPPLS,
            CAPI_LOGBLKSIZETOSMALL,
            CAPI_BUFFEXCECEEDS64K,
            CAPI_MSGBUFSIZETOOSMALL,
            CAPI_ANZLOGCONNNOTSUPPORTED,
            CAPI_REGRESERVED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
        // No-error must be zero so a freshly-zeroed info field signals
        // success.
        assert_eq!(CAPI_NOERROR, 0);
    }

    #[test]
    fn test_manufacturer_lens_sane() {
        assert!(CAPI_MANUFACTURER_LEN >= 16);
        assert!(CAPI_SERIAL_LEN >= 1);
    }
}
