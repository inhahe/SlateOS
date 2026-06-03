//! `<linux/nvme.h>` (NVM command subset) — NVMe I/O command opcodes.
//!
//! NVMe I/O commands operate on namespaces (logical storage units).
//! They are submitted to I/O submission queues and completed via
//! completion queues. Unlike SCSI, NVMe commands are fixed 64-byte
//! structures with a simple opcode-based dispatch.

// ---------------------------------------------------------------------------
// NVM I/O command opcodes (nvme_opcode)
// ---------------------------------------------------------------------------

/// Flush volatile write cache.
pub const NVME_CMD_FLUSH: u8 = 0x00;
/// Write data to namespace.
pub const NVME_CMD_WRITE: u8 = 0x01;
/// Read data from namespace.
pub const NVME_CMD_READ: u8 = 0x02;
/// Write uncorrectable (mark LBAs as unreadable).
pub const NVME_CMD_WRITE_UNCOR: u8 = 0x04;
/// Compare data in namespace.
pub const NVME_CMD_COMPARE: u8 = 0x05;
/// Write zeroes to namespace.
pub const NVME_CMD_WRITE_ZEROES: u8 = 0x08;
/// Dataset management (TRIM/deallocate).
pub const NVME_CMD_DSM: u8 = 0x09;
/// Verify data integrity without transfer.
pub const NVME_CMD_VERIFY: u8 = 0x0C;
/// Reservation register.
pub const NVME_CMD_RESV_REGISTER: u8 = 0x0D;
/// Reservation report.
pub const NVME_CMD_RESV_REPORT: u8 = 0x0E;
/// Reservation acquire.
pub const NVME_CMD_RESV_ACQUIRE: u8 = 0x11;
/// Reservation release.
pub const NVME_CMD_RESV_RELEASE: u8 = 0x15;
/// Zone management send (ZNS).
pub const NVME_CMD_ZONE_MGMT_SEND: u8 = 0x79;
/// Zone management receive (ZNS).
pub const NVME_CMD_ZONE_MGMT_RECV: u8 = 0x7A;
/// Zone append (ZNS).
pub const NVME_CMD_ZONE_APPEND: u8 = 0x7D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            NVME_CMD_FLUSH,
            NVME_CMD_WRITE,
            NVME_CMD_READ,
            NVME_CMD_WRITE_UNCOR,
            NVME_CMD_COMPARE,
            NVME_CMD_WRITE_ZEROES,
            NVME_CMD_DSM,
            NVME_CMD_VERIFY,
            NVME_CMD_RESV_REGISTER,
            NVME_CMD_RESV_REPORT,
            NVME_CMD_RESV_ACQUIRE,
            NVME_CMD_RESV_RELEASE,
            NVME_CMD_ZONE_MGMT_SEND,
            NVME_CMD_ZONE_MGMT_RECV,
            NVME_CMD_ZONE_APPEND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_common_opcodes() {
        assert_eq!(NVME_CMD_READ, 0x02);
        assert_eq!(NVME_CMD_WRITE, 0x01);
        assert_eq!(NVME_CMD_FLUSH, 0x00);
    }

    #[test]
    fn test_dsm_is_trim() {
        assert_eq!(NVME_CMD_DSM, 0x09);
    }
}
