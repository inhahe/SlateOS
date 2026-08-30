//! `<linux/rds_rdma.h>` — RDS RDMA extension constants.
//!
//! RDS (Reliable Datagram Sockets, `AF_RDS`) extension for RDMA
//! offload — used by Oracle clusters over InfiniBand. These
//! constants name the cmsg types, opcode flags, and status codes
//! exposed to userspace.

// ---------------------------------------------------------------------------
// Control-message types (cmsg_type for AF_RDS)
// ---------------------------------------------------------------------------

/// MR (memory region) cookie to be reused by following requests.
pub const RDS_CMSG_RDMA_ARGS: u32 = 1;
/// Source cookie returned to caller for completion correlation.
pub const RDS_CMSG_RDMA_DEST: u32 = 2;
/// Caller-supplied MR-map cookie.
pub const RDS_CMSG_RDMA_MAP: u32 = 3;
/// RDMA completion notification.
pub const RDS_CMSG_RDMA_STATUS: u32 = 4;
/// Congestion-update event.
pub const RDS_CMSG_CONG_UPDATE: u32 = 5;
/// Atomic FADD args.
pub const RDS_CMSG_ATOMIC_FADD: u32 = 6;
/// Atomic CSWP args.
pub const RDS_CMSG_ATOMIC_CSWP: u32 = 7;
/// Atomic completion notification.
pub const RDS_CMSG_ATOMIC_NOTIFY: u32 = 8;
/// Atomic status.
pub const RDS_CMSG_ATOMIC_STATUS: u32 = 9;
/// Masked atomic CSWP.
pub const RDS_CMSG_MASKED_ATOMIC_CSWP: u32 = 10;
/// Masked atomic FADD.
pub const RDS_CMSG_MASKED_ATOMIC_FADD: u32 = 11;

// ---------------------------------------------------------------------------
// RDMA-args flag bits (rds_rdma_args.flags)
// ---------------------------------------------------------------------------

/// Operation is a write (else read).
pub const RDS_RDMA_READWRITE: u32 = 0x0001;
/// Issue a completion notify on success.
pub const RDS_RDMA_NOTIFY_ME: u32 = 0x0002;
/// FENCE — wait for prior ops to complete.
pub const RDS_RDMA_FENCE: u32 = 0x0004;
/// Free the local memory region after this op.
pub const RDS_RDMA_INVALIDATE: u32 = 0x0008;
/// Op uses the local memory key.
pub const RDS_RDMA_USE_ONCE: u32 = 0x0010;
/// Send an RDMA-RDMA-DEST cookie back.
pub const RDS_RDMA_DONTWAIT: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Completion status codes (rds_rdma_notify.status)
// ---------------------------------------------------------------------------

/// Op completed successfully.
pub const RDS_RDMA_SUCCESS: u32 = 0;
/// Remote-side error.
pub const RDS_RDMA_REMOTE_ERROR: u32 = 1;
/// Op cancelled (peer reset).
pub const RDS_RDMA_CANCELED: u32 = 2;
/// Op dropped due to local failure.
pub const RDS_RDMA_DROPPED: u32 = 3;
/// Other unspecified error.
pub const RDS_RDMA_OTHER_ERROR: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmsg_types_distinct() {
        let cmsgs = [
            RDS_CMSG_RDMA_ARGS,
            RDS_CMSG_RDMA_DEST,
            RDS_CMSG_RDMA_MAP,
            RDS_CMSG_RDMA_STATUS,
            RDS_CMSG_CONG_UPDATE,
            RDS_CMSG_ATOMIC_FADD,
            RDS_CMSG_ATOMIC_CSWP,
            RDS_CMSG_ATOMIC_NOTIFY,
            RDS_CMSG_ATOMIC_STATUS,
            RDS_CMSG_MASKED_ATOMIC_CSWP,
            RDS_CMSG_MASKED_ATOMIC_FADD,
        ];
        for i in 0..cmsgs.len() {
            for j in (i + 1)..cmsgs.len() {
                assert_ne!(cmsgs[i], cmsgs[j]);
            }
        }
    }

    #[test]
    fn test_rdma_flag_bits_distinct() {
        let flags = [
            RDS_RDMA_READWRITE,
            RDS_RDMA_NOTIFY_ME,
            RDS_RDMA_FENCE,
            RDS_RDMA_INVALIDATE,
            RDS_RDMA_USE_ONCE,
            RDS_RDMA_DONTWAIT,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        // Caller uses a non-zero status to indicate failure.
        assert_eq!(RDS_RDMA_SUCCESS, 0);
        let errs = [
            RDS_RDMA_REMOTE_ERROR,
            RDS_RDMA_CANCELED,
            RDS_RDMA_DROPPED,
            RDS_RDMA_OTHER_ERROR,
        ];
        for &e in &errs {
            assert_ne!(e, 0);
        }
    }
}
