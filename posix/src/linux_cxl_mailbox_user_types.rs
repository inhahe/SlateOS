//! `<uapi/linux/cxl_mem.h>` — CXL mailbox command interface (raw opcodes).
//!
//! CXL devices expose a mailbox at BAR0 + cap offset. The host writes a
//! command opcode + payload, polls for completion, then reads return
//! status + output payload. The Linux driver exposes these via
//! `/dev/cxl/memN` ioctls; the kernel validates opcodes against an
//! allowlist before forwarding to hardware.

// ---------------------------------------------------------------------------
// CXL mailbox command opcodes (CXL 3.0 spec §8.2.9)
// ---------------------------------------------------------------------------

/// Events log — get records.
pub const CXL_MBOX_OP_GET_EVENT_RECORDS: u16 = 0x0100;
/// Events log — clear records.
pub const CXL_MBOX_OP_CLEAR_EVENT_RECORDS: u16 = 0x0101;
/// Events log — get interrupt policy.
pub const CXL_MBOX_OP_GET_EVT_INT_POLICY: u16 = 0x0102;
/// Events log — set interrupt policy.
pub const CXL_MBOX_OP_SET_EVT_INT_POLICY: u16 = 0x0103;

/// Firmware update — get info.
pub const CXL_MBOX_OP_GET_FW_INFO: u16 = 0x0200;
/// Firmware update — transfer FW.
pub const CXL_MBOX_OP_TRANSFER_FW: u16 = 0x0201;
/// Firmware update — activate FW.
pub const CXL_MBOX_OP_ACTIVATE_FW: u16 = 0x0202;

/// Timestamp — get.
pub const CXL_MBOX_OP_GET_TIMESTAMP: u16 = 0x0300;
/// Timestamp — set.
pub const CXL_MBOX_OP_SET_TIMESTAMP: u16 = 0x0301;

/// Logs — get supported logs.
pub const CXL_MBOX_OP_GET_SUPPORTED_LOGS: u16 = 0x0400;
/// Logs — get log entries.
pub const CXL_MBOX_OP_GET_LOG: u16 = 0x0401;

/// Identify command — device info.
pub const CXL_MBOX_OP_IDENTIFY: u16 = 0x4000;
/// Get partition info — volatile/persistent split.
pub const CXL_MBOX_OP_GET_PARTITION_INFO: u16 = 0x4100;
/// Set partition info.
pub const CXL_MBOX_OP_SET_PARTITION_INFO: u16 = 0x4101;
/// Get LSA (label storage area).
pub const CXL_MBOX_OP_GET_LSA: u16 = 0x4102;
/// Set LSA.
pub const CXL_MBOX_OP_SET_LSA: u16 = 0x4103;
/// Get health info.
pub const CXL_MBOX_OP_GET_HEALTH_INFO: u16 = 0x4200;
/// Get alert config.
pub const CXL_MBOX_OP_GET_ALERT_CONFIG: u16 = 0x4201;
/// Set alert config.
pub const CXL_MBOX_OP_SET_ALERT_CONFIG: u16 = 0x4202;
/// Get shutdown state.
pub const CXL_MBOX_OP_GET_SHUTDOWN_STATE: u16 = 0x4203;
/// Set shutdown state.
pub const CXL_MBOX_OP_SET_SHUTDOWN_STATE: u16 = 0x4204;

/// Get poison list.
pub const CXL_MBOX_OP_GET_POISON: u16 = 0x4300;
/// Inject poison at address.
pub const CXL_MBOX_OP_INJECT_POISON: u16 = 0x4301;
/// Clear poison at address.
pub const CXL_MBOX_OP_CLEAR_POISON: u16 = 0x4302;
/// Get scan media capabilities.
pub const CXL_MBOX_OP_GET_SCAN_MEDIA_CAPS: u16 = 0x4303;
/// Scan media.
pub const CXL_MBOX_OP_SCAN_MEDIA: u16 = 0x4304;
/// Get scan media results.
pub const CXL_MBOX_OP_GET_SCAN_MEDIA: u16 = 0x4305;

/// Security — sanitize device.
pub const CXL_MBOX_OP_SANITIZE: u16 = 0x4400;
/// Security — secure erase.
pub const CXL_MBOX_OP_SECURE_ERASE: u16 = 0x4401;
/// Security — get state.
pub const CXL_MBOX_OP_GET_SECURITY_STATE: u16 = 0x4500;
/// Security — set passphrase.
pub const CXL_MBOX_OP_SET_PASSPHRASE: u16 = 0x4501;
/// Security — disable passphrase.
pub const CXL_MBOX_OP_DISABLE_PASSPHRASE: u16 = 0x4502;
/// Security — unlock.
pub const CXL_MBOX_OP_UNLOCK: u16 = 0x4503;
/// Security — freeze.
pub const CXL_MBOX_OP_FREEZE_SECURITY: u16 = 0x4504;
/// Security — passphrase secure erase.
pub const CXL_MBOX_OP_PASSPHRASE_SECURE_ERASE: u16 = 0x4505;

// ---------------------------------------------------------------------------
// Opcode category masks (high byte of the opcode = category)
// ---------------------------------------------------------------------------

pub const CXL_MBOX_CAT_EVENTS: u16 = 0x01;
pub const CXL_MBOX_CAT_FW: u16 = 0x02;
pub const CXL_MBOX_CAT_TIMESTAMP: u16 = 0x03;
pub const CXL_MBOX_CAT_LOGS: u16 = 0x04;
pub const CXL_MBOX_CAT_IDENTIFY: u16 = 0x40;
pub const CXL_MBOX_CAT_CCLS: u16 = 0x41;
pub const CXL_MBOX_CAT_HEALTH: u16 = 0x42;
pub const CXL_MBOX_CAT_POISON: u16 = 0x43;
pub const CXL_MBOX_CAT_SANITIZE: u16 = 0x44;
pub const CXL_MBOX_CAT_SECURITY: u16 = 0x45;

// ---------------------------------------------------------------------------
// Mailbox return codes (CXL 3.0 §8.2.8.4.5.1)
// ---------------------------------------------------------------------------

pub const CXL_MBOX_RET_SUCCESS: u16 = 0x00;
pub const CXL_MBOX_RET_BG_STARTED: u16 = 0x01;
pub const CXL_MBOX_RET_INVALID_INPUT: u16 = 0x02;
pub const CXL_MBOX_RET_UNSUPPORTED: u16 = 0x03;
pub const CXL_MBOX_RET_INTERNAL_ERROR: u16 = 0x04;
pub const CXL_MBOX_RET_RETRY: u16 = 0x05;
pub const CXL_MBOX_RET_BUSY: u16 = 0x06;
pub const CXL_MBOX_RET_MEDIA_DISABLED: u16 = 0x07;
pub const CXL_MBOX_RET_FW_TRANSFER_IN_PROGRESS: u16 = 0x08;
pub const CXL_MBOX_RET_FW_TRANSFER_OOO: u16 = 0x09;
pub const CXL_MBOX_RET_FW_AUTH_FAILED: u16 = 0x0a;
pub const CXL_MBOX_RET_INVALID_SLOT: u16 = 0x0b;

// ---------------------------------------------------------------------------
// Mailbox payload limits
// ---------------------------------------------------------------------------

/// Minimum payload size that all CXL mailboxes guarantee (bytes).
pub const CXL_MBOX_MIN_PAYLOAD: usize = 256;
/// Maximum mailbox payload size: 1 MiB per CXL 3.0.
pub const CXL_MBOX_MAX_PAYLOAD: usize = 1 << 20;
/// Default poll-for-completion timeout, milliseconds.
pub const CXL_MBOX_DEFAULT_TIMEOUT_MS: u32 = 2000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcode_categories_match_high_byte() {
        assert_eq!(CXL_MBOX_OP_GET_EVENT_RECORDS >> 8, CXL_MBOX_CAT_EVENTS);
        assert_eq!(CXL_MBOX_OP_GET_FW_INFO >> 8, CXL_MBOX_CAT_FW);
        assert_eq!(CXL_MBOX_OP_GET_TIMESTAMP >> 8, CXL_MBOX_CAT_TIMESTAMP);
        assert_eq!(CXL_MBOX_OP_IDENTIFY >> 8, CXL_MBOX_CAT_IDENTIFY);
        assert_eq!(CXL_MBOX_OP_GET_HEALTH_INFO >> 8, CXL_MBOX_CAT_HEALTH);
        assert_eq!(CXL_MBOX_OP_GET_POISON >> 8, CXL_MBOX_CAT_POISON);
        assert_eq!(CXL_MBOX_OP_SANITIZE >> 8, CXL_MBOX_CAT_SANITIZE);
        assert_eq!(CXL_MBOX_OP_GET_SECURITY_STATE >> 8, CXL_MBOX_CAT_SECURITY);
    }

    #[test]
    fn test_event_subops_consecutive() {
        assert_eq!(CXL_MBOX_OP_CLEAR_EVENT_RECORDS, CXL_MBOX_OP_GET_EVENT_RECORDS + 1);
        assert_eq!(CXL_MBOX_OP_GET_EVT_INT_POLICY, CXL_MBOX_OP_GET_EVENT_RECORDS + 2);
        assert_eq!(CXL_MBOX_OP_SET_EVT_INT_POLICY, CXL_MBOX_OP_GET_EVENT_RECORDS + 3);
    }

    #[test]
    fn test_fw_subops_consecutive() {
        assert_eq!(CXL_MBOX_OP_TRANSFER_FW, CXL_MBOX_OP_GET_FW_INFO + 1);
        assert_eq!(CXL_MBOX_OP_ACTIVATE_FW, CXL_MBOX_OP_GET_FW_INFO + 2);
    }

    #[test]
    fn test_get_set_pairs() {
        assert_eq!(CXL_MBOX_OP_SET_TIMESTAMP, CXL_MBOX_OP_GET_TIMESTAMP + 1);
        assert_eq!(CXL_MBOX_OP_SET_PARTITION_INFO, CXL_MBOX_OP_GET_PARTITION_INFO + 1);
        assert_eq!(CXL_MBOX_OP_SET_LSA, CXL_MBOX_OP_GET_LSA + 1);
        assert_eq!(CXL_MBOX_OP_SET_ALERT_CONFIG, CXL_MBOX_OP_GET_ALERT_CONFIG + 1);
        assert_eq!(CXL_MBOX_OP_SET_SHUTDOWN_STATE, CXL_MBOX_OP_GET_SHUTDOWN_STATE + 1);
    }

    #[test]
    fn test_poison_subops_dense() {
        let p = [
            CXL_MBOX_OP_GET_POISON,
            CXL_MBOX_OP_INJECT_POISON,
            CXL_MBOX_OP_CLEAR_POISON,
            CXL_MBOX_OP_GET_SCAN_MEDIA_CAPS,
            CXL_MBOX_OP_SCAN_MEDIA,
            CXL_MBOX_OP_GET_SCAN_MEDIA,
        ];
        for w in p.windows(2) {
            assert_eq!(w[1], w[0] + 1);
        }
    }

    #[test]
    fn test_return_codes_dense_0_to_b() {
        let r = [
            CXL_MBOX_RET_SUCCESS,
            CXL_MBOX_RET_BG_STARTED,
            CXL_MBOX_RET_INVALID_INPUT,
            CXL_MBOX_RET_UNSUPPORTED,
            CXL_MBOX_RET_INTERNAL_ERROR,
            CXL_MBOX_RET_RETRY,
            CXL_MBOX_RET_BUSY,
            CXL_MBOX_RET_MEDIA_DISABLED,
            CXL_MBOX_RET_FW_TRANSFER_IN_PROGRESS,
            CXL_MBOX_RET_FW_TRANSFER_OOO,
            CXL_MBOX_RET_FW_AUTH_FAILED,
            CXL_MBOX_RET_INVALID_SLOT,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_payload_size_bounds() {
        assert_eq!(CXL_MBOX_MIN_PAYLOAD, 256);
        assert_eq!(CXL_MBOX_MAX_PAYLOAD, 1024 * 1024);
        assert!(CXL_MBOX_MAX_PAYLOAD > CXL_MBOX_MIN_PAYLOAD);
        assert!(CXL_MBOX_MIN_PAYLOAD.is_power_of_two());
        assert!(CXL_MBOX_MAX_PAYLOAD.is_power_of_two());
    }

    #[test]
    fn test_default_timeout_positive() {
        assert!(CXL_MBOX_DEFAULT_TIMEOUT_MS > 0);
        assert_eq!(CXL_MBOX_DEFAULT_TIMEOUT_MS, 2000);
    }

    #[test]
    fn test_security_opcodes_in_cat_security() {
        for op in [
            CXL_MBOX_OP_GET_SECURITY_STATE,
            CXL_MBOX_OP_SET_PASSPHRASE,
            CXL_MBOX_OP_DISABLE_PASSPHRASE,
            CXL_MBOX_OP_UNLOCK,
            CXL_MBOX_OP_FREEZE_SECURITY,
            CXL_MBOX_OP_PASSPHRASE_SECURE_ERASE,
        ] {
            assert_eq!(op >> 8, CXL_MBOX_CAT_SECURITY);
        }
    }
}
