//! `<linux/dlmconstants.h>` — DLM lock-manager protocol constants.
//!
//! Lock modes, flags, and error codes shared between the kernel
//! Distributed Lock Manager and userspace clients
//! (typically GFS2 / OCFS2 cluster nodes and `dlm_controld`).

// ---------------------------------------------------------------------------
// Lock modes
// ---------------------------------------------------------------------------

/// Invalid / unlocked.
pub const DLM_LOCK_IV: i32 = -1;
/// Null mode.
pub const DLM_LOCK_NL: i32 = 0;
/// Concurrent read.
pub const DLM_LOCK_CR: i32 = 1;
/// Concurrent write.
pub const DLM_LOCK_CW: i32 = 2;
/// Protected read.
pub const DLM_LOCK_PR: i32 = 3;
/// Protected write.
pub const DLM_LOCK_PW: i32 = 4;
/// Exclusive.
pub const DLM_LOCK_EX: i32 = 5;

// ---------------------------------------------------------------------------
// Lock-status / async-request flags (uint32)
// ---------------------------------------------------------------------------

/// Don't queue the request — fail with EAGAIN if not immediately grantable.
pub const DLM_LKF_NOQUEUE: u32 = 0x0000_0001;
/// Cancel a pending or convert in progress.
pub const DLM_LKF_CANCEL: u32 = 0x0000_0002;
/// Mark request as converting (internal).
pub const DLM_LKF_CONVERT: u32 = 0x0000_0004;
/// Apply value-block transfer.
pub const DLM_LKF_VALBLK: u32 = 0x0000_0008;
/// Notify on conversion done.
pub const DLM_LKF_QUECVT: u32 = 0x0000_0010;
/// Don't go through the deadlock detector.
pub const DLM_LKF_EXPEDITE: u32 = 0x0000_0020;
/// Persistent — keep across normal lock-resource teardown.
pub const DLM_LKF_PERSISTENT: u32 = 0x0000_0080;
/// AST callbacks not desired (BAST/CAST suppressed).
pub const DLM_LKF_NODLCKWT: u32 = 0x0000_0100;
/// Try to convert without deadlock notification.
pub const DLM_LKF_NODLCKBLK: u32 = 0x0000_0200;
/// Orphaned lock — kept after owner exit.
pub const DLM_LKF_ORPHAN: u32 = 0x0000_0400;
/// Allow conversion deadlock.
pub const DLM_LKF_ALTPR: u32 = 0x0000_0800;
/// Allow conversion-deadlock workaround (CW substitution).
pub const DLM_LKF_ALTCW: u32 = 0x0000_1000;
/// Force unlock — used by recovery.
pub const DLM_LKF_FORCEUNLOCK: u32 = 0x0000_2000;
/// Time-out queued request after specified ms.
pub const DLM_LKF_TIMEOUT: u32 = 0x0000_4000;

// ---------------------------------------------------------------------------
// Lock-status / error codes returned via the AST
// ---------------------------------------------------------------------------

/// Request was queued and is grantable.
pub const DLM_ECANCEL: i32 = 0x10001;
/// Lock converted/upgraded successfully.
pub const DLM_EUNLOCK: i32 = 0x10002;

// ---------------------------------------------------------------------------
// Misc structural caps
// ---------------------------------------------------------------------------

/// Maximum lockspace name length (bytes).
pub const DLM_LOCKSPACE_LEN: u32 = 64;
/// Maximum resource name length (bytes).
pub const DLM_RESNAME_MAXLEN: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_modes_distinct_and_ordered() {
        // Modes are strictly increasing in "strength" — relied on by the
        // DLM compatibility matrix.
        let modes = [
            DLM_LOCK_NL,
            DLM_LOCK_CR,
            DLM_LOCK_CW,
            DLM_LOCK_PR,
            DLM_LOCK_PW,
            DLM_LOCK_EX,
        ];
        for w in modes.windows(2) {
            assert!(w[0] < w[1]);
        }
        assert!(DLM_LOCK_IV < DLM_LOCK_NL);
    }

    #[test]
    fn test_lkf_flags_distinct() {
        let flags = [
            DLM_LKF_NOQUEUE,
            DLM_LKF_CANCEL,
            DLM_LKF_CONVERT,
            DLM_LKF_VALBLK,
            DLM_LKF_QUECVT,
            DLM_LKF_EXPEDITE,
            DLM_LKF_PERSISTENT,
            DLM_LKF_NODLCKWT,
            DLM_LKF_NODLCKBLK,
            DLM_LKF_ORPHAN,
            DLM_LKF_ALTPR,
            DLM_LKF_ALTCW,
            DLM_LKF_FORCEUNLOCK,
            DLM_LKF_TIMEOUT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
        for &f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} is not a single bit");
        }
    }

    #[test]
    fn test_codes_distinct() {
        assert_ne!(DLM_ECANCEL, DLM_EUNLOCK);
    }

    #[test]
    fn test_name_lengths_match_kernel() {
        assert_eq!(DLM_LOCKSPACE_LEN, 64);
        assert_eq!(DLM_RESNAME_MAXLEN, 64);
    }
}
