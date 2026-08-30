//! `<linux/nvme.h>` (feature subset) — NVMe feature identifiers.
//!
//! NVMe features are tuneable parameters of the controller and its
//! namespaces. They are read with Get Features and written with Set
//! Features admin commands. Feature IDs are standardised by the NVMe
//! specification.

// ---------------------------------------------------------------------------
// Feature identifiers (FID)
// ---------------------------------------------------------------------------

/// Arbitration: weighted round-robin parameters.
pub const NVME_FEAT_ARBITRATION: u32 = 0x01;
/// Power management: power state selection.
pub const NVME_FEAT_POWER_MGMT: u32 = 0x02;
/// LBA range type: namespace LBA ranges.
pub const NVME_FEAT_LBA_RANGE: u32 = 0x03;
/// Temperature threshold: thermal alert.
pub const NVME_FEAT_TEMP_THRESH: u32 = 0x04;
/// Error recovery: timeout and retry settings.
pub const NVME_FEAT_ERR_RECOVERY: u32 = 0x05;
/// Volatile write cache: enable/disable.
pub const NVME_FEAT_VOLATILE_WC: u32 = 0x06;
/// Number of queues: set I/O queue count.
pub const NVME_FEAT_NUM_QUEUES: u32 = 0x07;
/// Interrupt coalescing: aggregation settings.
pub const NVME_FEAT_IRQ_COALESCE: u32 = 0x08;
/// Interrupt vector configuration.
pub const NVME_FEAT_IRQ_CONFIG: u32 = 0x09;
/// Write atomicity: normal/extended.
pub const NVME_FEAT_WRITE_ATOMIC: u32 = 0x0A;
/// Asynchronous event configuration.
pub const NVME_FEAT_ASYNC_EVENT: u32 = 0x0B;
/// Autonomous power state transition.
pub const NVME_FEAT_AUTO_PST: u32 = 0x0C;
/// Host memory buffer: allocation.
pub const NVME_FEAT_HOST_MEM_BUF: u32 = 0x0D;
/// Timestamp: set controller time.
pub const NVME_FEAT_TIMESTAMP: u32 = 0x0E;
/// Keep alive timer: timeout value.
pub const NVME_FEAT_KATO: u32 = 0x0F;
/// Host controlled thermal management.
pub const NVME_FEAT_HCTM: u32 = 0x10;
/// Non-operational power state configuration.
pub const NVME_FEAT_NOPSC: u32 = 0x11;
/// Host behaviour support.
pub const NVME_FEAT_HOST_BEHAVIOR: u32 = 0x16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_distinct() {
        let feats = [
            NVME_FEAT_ARBITRATION,
            NVME_FEAT_POWER_MGMT,
            NVME_FEAT_LBA_RANGE,
            NVME_FEAT_TEMP_THRESH,
            NVME_FEAT_ERR_RECOVERY,
            NVME_FEAT_VOLATILE_WC,
            NVME_FEAT_NUM_QUEUES,
            NVME_FEAT_IRQ_COALESCE,
            NVME_FEAT_IRQ_CONFIG,
            NVME_FEAT_WRITE_ATOMIC,
            NVME_FEAT_ASYNC_EVENT,
            NVME_FEAT_AUTO_PST,
            NVME_FEAT_HOST_MEM_BUF,
            NVME_FEAT_TIMESTAMP,
            NVME_FEAT_KATO,
            NVME_FEAT_HCTM,
            NVME_FEAT_NOPSC,
            NVME_FEAT_HOST_BEHAVIOR,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_common_features() {
        assert_eq!(NVME_FEAT_NUM_QUEUES, 0x07);
        assert_eq!(NVME_FEAT_VOLATILE_WC, 0x06);
    }

    #[test]
    fn test_features_nonzero() {
        assert!(NVME_FEAT_ARBITRATION > 0);
    }
}
