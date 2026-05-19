//! `<linux/rds.h>` — Additional RDS (Reliable Datagram Sockets) constants.
//!
//! Supplementary RDS constants covering socket options,
//! message flags, and RDMA operations.

// ---------------------------------------------------------------------------
// RDS socket options
// ---------------------------------------------------------------------------

/// Cancel sent to operation.
pub const RDS_CANCEL_SENT_TO: u32 = 1;
/// Get MR (Memory Region).
pub const RDS_GET_MR: u32 = 2;
/// Free MR.
pub const RDS_FREE_MR: u32 = 3;
/// Recycle MR.
pub const RDS_RECVERR: u32 = 5;
/// Connection info.
pub const RDS_CONG_MONITOR: u32 = 6;
/// Get MR for RDMA.
pub const RDS_GET_MR_FOR_DEST: u32 = 7;

// ---------------------------------------------------------------------------
// RDS message flags (RDS_RDMA_*)
// ---------------------------------------------------------------------------

/// Readwrite flag (allow write).
pub const RDS_RDMA_READWRITE: u32 = 1 << 0;
/// Fence flag.
pub const RDS_RDMA_FENCE: u32 = 1 << 1;
/// Invalidate flag.
pub const RDS_RDMA_INVALIDATE: u32 = 1 << 2;
/// Use once flag.
pub const RDS_RDMA_USE_ONCE: u32 = 1 << 3;
/// Dontwait flag.
pub const RDS_RDMA_DONTWAIT: u32 = 1 << 4;
/// Notify me flag.
pub const RDS_RDMA_NOTIFY_ME: u32 = 1 << 5;
/// Silent flag.
pub const RDS_RDMA_SILENT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// RDS CMSG types
// ---------------------------------------------------------------------------

/// RDMA arguments.
pub const RDS_CMSG_RDMA_ARGS: u32 = 1;
/// RDMA destination.
pub const RDS_CMSG_RDMA_DEST: u32 = 2;
/// RDMA map.
pub const RDS_CMSG_RDMA_MAP: u32 = 3;
/// RDMA status.
pub const RDS_CMSG_RDMA_STATUS: u32 = 4;
/// Congestion update.
pub const RDS_CMSG_CONG_UPDATE: u32 = 5;
/// Atomic FADD.
pub const RDS_CMSG_ATOMIC_FADD: u32 = 6;
/// Atomic CSWP.
pub const RDS_CMSG_ATOMIC_CSWP: u32 = 7;
/// Masked atomic FADD.
pub const RDS_CMSG_MASKED_ATOMIC_FADD: u32 = 8;
/// Masked atomic CSWP.
pub const RDS_CMSG_MASKED_ATOMIC_CSWP: u32 = 9;
/// Receive path latency.
pub const RDS_CMSG_RXPATH_LATENCY: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_opts_distinct() {
        let opts = [
            RDS_CANCEL_SENT_TO, RDS_GET_MR, RDS_FREE_MR,
            RDS_RECVERR, RDS_CONG_MONITOR, RDS_GET_MR_FOR_DEST,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_rdma_flags_power_of_two() {
        let flags = [
            RDS_RDMA_READWRITE, RDS_RDMA_FENCE, RDS_RDMA_INVALIDATE,
            RDS_RDMA_USE_ONCE, RDS_RDMA_DONTWAIT,
            RDS_RDMA_NOTIFY_ME, RDS_RDMA_SILENT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_rdma_flags_no_overlap() {
        let flags = [
            RDS_RDMA_READWRITE, RDS_RDMA_FENCE, RDS_RDMA_INVALIDATE,
            RDS_RDMA_USE_ONCE, RDS_RDMA_DONTWAIT,
            RDS_RDMA_NOTIFY_ME, RDS_RDMA_SILENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmsg_types_distinct() {
        let types = [
            RDS_CMSG_RDMA_ARGS, RDS_CMSG_RDMA_DEST,
            RDS_CMSG_RDMA_MAP, RDS_CMSG_RDMA_STATUS,
            RDS_CMSG_CONG_UPDATE, RDS_CMSG_ATOMIC_FADD,
            RDS_CMSG_ATOMIC_CSWP, RDS_CMSG_MASKED_ATOMIC_FADD,
            RDS_CMSG_MASKED_ATOMIC_CSWP, RDS_CMSG_RXPATH_LATENCY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
