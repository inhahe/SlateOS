//! `<linux/dlm.h>` — Distributed Lock Manager userspace constants.
//!
//! The DLM provides cluster-wide locking on Linux (GFS2, OCFS2,
//! Pacemaker fencing). Userspace clients via `libdlm` invoke the
//! kernel using these lock modes, flags, and status codes, and the
//! lockspace lifecycle ioctls on `/dev/dlm-control` and
//! `/dev/dlm_<name>`.

// ---------------------------------------------------------------------------
// Lock modes (DLM_LOCK_*)
// ---------------------------------------------------------------------------

/// "I-of-V" mode (invalid, used as a sentinel).
pub const DLM_LOCK_IV: i32 = -1;
/// Null (no access).
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
// Lock-request flags (DLM_LKF_*)
// ---------------------------------------------------------------------------

/// Don't queue if the request can't be granted now.
pub const DLM_LKF_NOQUEUE: u32 = 0x00000001;
/// Conversion: don't requeue if blocked.
pub const DLM_LKF_CANCEL: u32 = 0x00000002;
/// Convert lock without losing it first.
pub const DLM_LKF_CONVERT: u32 = 0x00000004;
/// Receive value block from the LVB.
pub const DLM_LKF_VALBLK: u32 = 0x00000008;
/// Set LVB after grant.
pub const DLM_LKF_QUECVT: u32 = 0x00000010;
/// Invalid LVB (mark lockspace as needing recovery).
pub const DLM_LKF_IVVALBLK: u32 = 0x00000020;
/// Submit non-blocking lock request.
pub const DLM_LKF_PERSISTENT: u32 = 0x00000080;
/// Force-unlock (used during recovery).
pub const DLM_LKF_FORCEUNLOCK: u32 = 0x00000100;
/// Submit lock to be granted only on the master.
pub const DLM_LKF_TIMEOUT: u32 = 0x00000200;
/// Wait until conversion to a higher mode is granted.
pub const DLM_LKF_HEADQUE: u32 = 0x00000400;
/// Wait until conversion is at the head of the queue.
pub const DLM_LKF_NOORDER: u32 = 0x00000800;
/// Don't wait for replies from other nodes.
pub const DLM_LKF_ORPHAN: u32 = 0x00001000;

// ---------------------------------------------------------------------------
// Status / errno-style return codes used by DLM
// ---------------------------------------------------------------------------

/// Lock request succeeded.
pub const DLM_ECANCEL: i32 = 0x10001;
/// Lock request unlocked.
pub const DLM_EUNLOCK: i32 = 0x10002;

// ---------------------------------------------------------------------------
// Size limits
// ---------------------------------------------------------------------------

/// Maximum lockspace name length.
pub const DLM_LOCKSPACE_LEN: u32 = 64;
/// Maximum resource name length.
pub const DLM_RESNAME_MAXLEN: u32 = 64;
/// LVB (Lock Value Block) length.
pub const DLM_LVB_LEN: u32 = 32;
/// Maximum length of a user message attached to a request.
pub const DLM_USER_LVB_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// /dev/dlm-control device ioctls (minor numbers)
// ---------------------------------------------------------------------------

/// dlm-control ioctl group letter ('D').
pub const DLM_IOCTL_LETTER: u8 = b'D';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_modes_distinct_and_ordered() {
        // DLM modes must be ordered NL < CR < CW < PR < PW < EX so a
        // simple integer compare answers "is mode A weaker than B?".
        assert!(DLM_LOCK_NL < DLM_LOCK_CR);
        assert!(DLM_LOCK_CR < DLM_LOCK_CW);
        assert!(DLM_LOCK_CW < DLM_LOCK_PR);
        assert!(DLM_LOCK_PR < DLM_LOCK_PW);
        assert!(DLM_LOCK_PW < DLM_LOCK_EX);
        // IV (-1) is the sentinel below all real modes.
        assert!(DLM_LOCK_IV < DLM_LOCK_NL);
    }

    #[test]
    fn test_lock_flag_bits_distinct_pow2() {
        let f = [
            DLM_LKF_NOQUEUE,
            DLM_LKF_CANCEL,
            DLM_LKF_CONVERT,
            DLM_LKF_VALBLK,
            DLM_LKF_QUECVT,
            DLM_LKF_IVVALBLK,
            DLM_LKF_PERSISTENT,
            DLM_LKF_FORCEUNLOCK,
            DLM_LKF_TIMEOUT,
            DLM_LKF_HEADQUE,
            DLM_LKF_NOORDER,
            DLM_LKF_ORPHAN,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        assert_ne!(DLM_ECANCEL, DLM_EUNLOCK);
    }

    #[test]
    fn test_size_limits_sane() {
        assert!(DLM_LOCKSPACE_LEN.is_power_of_two());
        assert!(DLM_RESNAME_MAXLEN.is_power_of_two());
        assert!(DLM_LVB_LEN.is_power_of_two());
        assert_eq!(DLM_LVB_LEN, DLM_USER_LVB_LEN);
        assert_eq!(DLM_IOCTL_LETTER, b'D');
    }
}
