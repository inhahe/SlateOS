//! `<linux/bsg.h>` — Block-SCSI-Generic (`/dev/bsg/*`) user-facing
//! constants.
//!
//! `bsg` is the kernel's character-device interface for sending
//! arbitrary SCSI/SAS commands to a target. It replaces the legacy
//! `sg` driver with a richer 4-byte-aligned `sg_io_v4` request
//! structure used by `sg3_utils`, `smartctl`, and the storcli stack.

// ---------------------------------------------------------------------------
// Sub-protocol identifiers (`bsg_protocol_id`)
// ---------------------------------------------------------------------------

pub const BSG_PROTOCOL_SCSI: u32 = 0;
pub const BSG_PROTOCOL_TRANSPORT: u32 = 1;

// ---------------------------------------------------------------------------
// SCSI subprotocols (`bsg_subprotocol_id`)
// ---------------------------------------------------------------------------

pub const BSG_SUB_PROTOCOL_SCSI_CMD: u32 = 0;
pub const BSG_SUB_PROTOCOL_SCSI_TMF: u32 = 1;
pub const BSG_SUB_PROTOCOL_SCSI_TRANSPORT: u32 = 2;

// ---------------------------------------------------------------------------
// Transport subprotocols
// ---------------------------------------------------------------------------

pub const BSG_SUB_PROTOCOL_SAS_SMP: u32 = 0;
pub const BSG_SUB_PROTOCOL_FC_BSG: u32 = 1;

// ---------------------------------------------------------------------------
// `bsg_set_command_q` ioctl
// ---------------------------------------------------------------------------

/// `_IOW('b', 100, int)` — set command-queue depth.
pub const SG_IO: u32 = 0x2285;
pub const SG_GET_VERSION_NUM: u32 = 0x2282;
pub const SG_GET_RESERVED_SIZE: u32 = 0x2272;
pub const SG_SET_RESERVED_SIZE: u32 = 0x2275;
pub const SG_GET_COMMAND_Q: u32 = 0x2270;
pub const SG_SET_COMMAND_Q: u32 = 0x2271;

// ---------------------------------------------------------------------------
// Reply / status flag bits in `sg_io_v4.driver_status`
// ---------------------------------------------------------------------------

pub const BSG_DRV_STATUS_OK: u32 = 0;
pub const BSG_DRV_STATUS_SENSE: u32 = 0x8;
pub const BSG_DRV_STATUS_TIMEOUT: u32 = 0x6;
pub const BSG_DRV_STATUS_HARD_ERROR: u32 = 0x7;

// ---------------------------------------------------------------------------
// Sizes / limits
// ---------------------------------------------------------------------------

/// Maximum CDB length carried in a `sg_io_v4` request.
pub const BSG_MAX_CDB_LEN: usize = 252;

/// Maximum sense-data payload returned.
pub const BSG_MAX_SENSE_LEN: usize = 96;

/// Default per-request timeout (ms).
pub const BSG_DEFAULT_TIMEOUT_MS: u32 = 60_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_ids_pair() {
        assert_eq!(BSG_PROTOCOL_SCSI, 0);
        assert_eq!(BSG_PROTOCOL_TRANSPORT, 1);
        assert!(BSG_PROTOCOL_SCSI < BSG_PROTOCOL_TRANSPORT);
    }

    #[test]
    fn test_scsi_subprotocols_dense_0_to_2() {
        let s = [
            BSG_SUB_PROTOCOL_SCSI_CMD,
            BSG_SUB_PROTOCOL_SCSI_TMF,
            BSG_SUB_PROTOCOL_SCSI_TRANSPORT,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_transport_subprotocols() {
        assert_eq!(BSG_SUB_PROTOCOL_SAS_SMP, 0);
        assert_eq!(BSG_SUB_PROTOCOL_FC_BSG, 1);
    }

    #[test]
    fn test_sg_ioctls_in_sg_namespace() {
        for v in [
            SG_IO,
            SG_GET_VERSION_NUM,
            SG_GET_RESERVED_SIZE,
            SG_SET_RESERVED_SIZE,
            SG_GET_COMMAND_Q,
            SG_SET_COMMAND_Q,
        ] {
            // The SG family lives in the 0x22 type byte.
            assert_eq!((v >> 8) & 0xFF, 0x22);
        }
        // GET/SET reserved-size form a pair (0x2272/0x2275).
        // GET/SET command-q form a pair (0x2270/0x2271).
        assert_eq!(SG_SET_COMMAND_Q - SG_GET_COMMAND_Q, 1);
    }

    #[test]
    fn test_driver_status_codes_distinct() {
        let s = [
            BSG_DRV_STATUS_OK,
            BSG_DRV_STATUS_SENSE,
            BSG_DRV_STATUS_TIMEOUT,
            BSG_DRV_STATUS_HARD_ERROR,
        ];
        for (i, &a) in s.iter().enumerate() {
            for &b in &s[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // OK is zero.
        assert_eq!(BSG_DRV_STATUS_OK, 0);
    }

    #[test]
    fn test_request_size_limits() {
        assert_eq!(BSG_MAX_CDB_LEN, 252);
        assert_eq!(BSG_MAX_SENSE_LEN, 96);
        assert_eq!(BSG_DEFAULT_TIMEOUT_MS, 60_000);
        // 60 seconds = 1 minute default.
        assert_eq!(BSG_DEFAULT_TIMEOUT_MS / 1000, 60);
        // 252 = 256 - 4 (matches max-CDB minus a small fixed header).
        assert!(BSG_MAX_CDB_LEN < 256);
    }
}
