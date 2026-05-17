//! `<linux/cxl_mem.h>` — CXL mailbox command constants.
//!
//! CXL devices communicate with the host via a mailbox interface.
//! The host writes a command opcode and payload to the mailbox
//! registers, the device processes the command, and the host reads
//! the result. Commands cover device identification, health info,
//! firmware management, security, media/poison tracking, and
//! persistent memory label storage (LSA).

// ---------------------------------------------------------------------------
// CXL mailbox command opcodes
// ---------------------------------------------------------------------------

/// Identify device (get device capabilities).
pub const CXL_MBOX_OP_IDENTIFY: u32 = 0x0001;
/// Get partition info (volatile/persistent capacity).
pub const CXL_MBOX_OP_GET_PARTITION_INFO: u32 = 0x0100;
/// Set partition info (resize volatile/persistent).
pub const CXL_MBOX_OP_SET_PARTITION_INFO: u32 = 0x0101;
/// Get LSA (Label Storage Area, for namespaces).
pub const CXL_MBOX_OP_GET_LSA: u32 = 0x0401;
/// Set LSA.
pub const CXL_MBOX_OP_SET_LSA: u32 = 0x0402;
/// Get health info (temperature, spare, errors).
pub const CXL_MBOX_OP_GET_HEALTH_INFO: u32 = 0x0200;
/// Get alert configuration.
pub const CXL_MBOX_OP_GET_ALERT_CONFIG: u32 = 0x0201;
/// Set alert configuration.
pub const CXL_MBOX_OP_SET_ALERT_CONFIG: u32 = 0x0202;
/// Get event records (error/info/warning events).
pub const CXL_MBOX_OP_GET_EVENT_RECORD: u32 = 0x0100;
/// Clear event records.
pub const CXL_MBOX_OP_CLEAR_EVENT_RECORD: u32 = 0x0101;
/// Get poison list (media errors).
pub const CXL_MBOX_OP_GET_POISON: u32 = 0x0300;
/// Inject poison (for testing).
pub const CXL_MBOX_OP_INJECT_POISON: u32 = 0x0301;
/// Clear poison.
pub const CXL_MBOX_OP_CLEAR_POISON: u32 = 0x0302;
/// Get firmware info.
pub const CXL_MBOX_OP_GET_FW_INFO: u32 = 0x0200;
/// Transfer firmware image.
pub const CXL_MBOX_OP_TRANSFER_FW: u32 = 0x0201;
/// Activate firmware.
pub const CXL_MBOX_OP_ACTIVATE_FW: u32 = 0x0202;

// ---------------------------------------------------------------------------
// CXL mailbox return codes
// ---------------------------------------------------------------------------

/// Command completed successfully.
pub const CXL_MBOX_SUCCESS: u32 = 0x0000;
/// Background command started.
pub const CXL_MBOX_BG_STARTED: u32 = 0x0001;
/// Invalid input.
pub const CXL_MBOX_INVALID_INPUT: u32 = 0x0002;
/// Unsupported command.
pub const CXL_MBOX_UNSUPPORTED: u32 = 0x0003;
/// Internal error.
pub const CXL_MBOX_INTERNAL_ERROR: u32 = 0x0004;
/// Retry required.
pub const CXL_MBOX_RETRY: u32 = 0x0005;
/// Busy (command already in progress).
pub const CXL_MBOX_BUSY: u32 = 0x0006;
/// Media disabled.
pub const CXL_MBOX_MEDIA_DISABLED: u32 = 0x0007;
/// FW transfer in progress.
pub const CXL_MBOX_FW_XFER_IN_PROGRESS: u32 = 0x0008;

// ---------------------------------------------------------------------------
// CXL event log types
// ---------------------------------------------------------------------------

/// Information events.
pub const CXL_EVENT_LOG_INFO: u32 = 0;
/// Warning events.
pub const CXL_EVENT_LOG_WARN: u32 = 1;
/// Failure events.
pub const CXL_EVENT_LOG_FAILURE: u32 = 2;
/// Fatal events.
pub const CXL_EVENT_LOG_FATAL: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_codes_distinct() {
        let codes = [
            CXL_MBOX_SUCCESS, CXL_MBOX_BG_STARTED,
            CXL_MBOX_INVALID_INPUT, CXL_MBOX_UNSUPPORTED,
            CXL_MBOX_INTERNAL_ERROR, CXL_MBOX_RETRY,
            CXL_MBOX_BUSY, CXL_MBOX_MEDIA_DISABLED,
            CXL_MBOX_FW_XFER_IN_PROGRESS,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_event_logs_distinct() {
        let logs = [
            CXL_EVENT_LOG_INFO, CXL_EVENT_LOG_WARN,
            CXL_EVENT_LOG_FAILURE, CXL_EVENT_LOG_FATAL,
        ];
        for i in 0..logs.len() {
            for j in (i + 1)..logs.len() {
                assert_ne!(logs[i], logs[j]);
            }
        }
    }

    #[test]
    fn test_event_logs_ordered() {
        assert!(CXL_EVENT_LOG_INFO < CXL_EVENT_LOG_WARN);
        assert!(CXL_EVENT_LOG_WARN < CXL_EVENT_LOG_FAILURE);
        assert!(CXL_EVENT_LOG_FAILURE < CXL_EVENT_LOG_FATAL);
    }
}
