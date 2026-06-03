//! `<linux/dlm.h>` — Distributed Lock Manager userspace API.
//!
//! DLM is the kernel-side lock manager used by GFS2 and OCFS2 cluster
//! filesystems and by cluster-aware userspace daemons (dlm_controld).
//! Lock requests come in via libdlm and identify resources by name in
//! a per-lockspace namespace.

// ---------------------------------------------------------------------------
// Namespace sizes
// ---------------------------------------------------------------------------

/// Max lockspace name length (NUL-terminated).
pub const DLM_LOCKSPACE_LEN: usize = 64;
/// Max resource name length.
pub const DLM_RESNAME_MAXLEN: usize = 64;
/// Length of the lock value block (per AST data slot).
pub const DLM_LVB_LEN: usize = 32;
/// User-extra cookie size.
pub const DLM_USER_LVB_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Lock modes (compatible with VAX/VMS DLM convention)
// ---------------------------------------------------------------------------

/// `DLM_LOCK_IV` — invalid (placeholder for "no lock").
pub const DLM_LOCK_IV: i32 = -1;
/// `DLM_LOCK_NL` — null (placeholder grant).
pub const DLM_LOCK_NL: u32 = 0;
/// `DLM_LOCK_CR` — concurrent read.
pub const DLM_LOCK_CR: u32 = 1;
/// `DLM_LOCK_CW` — concurrent write.
pub const DLM_LOCK_CW: u32 = 2;
/// `DLM_LOCK_PR` — protected read.
pub const DLM_LOCK_PR: u32 = 3;
/// `DLM_LOCK_PW` — protected write.
pub const DLM_LOCK_PW: u32 = 4;
/// `DLM_LOCK_EX` — exclusive.
pub const DLM_LOCK_EX: u32 = 5;

// ---------------------------------------------------------------------------
// Lock-request flags (struct dlm_lksb.flags)
// ---------------------------------------------------------------------------

/// Block until lock is granted.
pub const DLM_LKF_NOQUEUE: u32 = 0x0000_0001;
/// Convert in place; do not re-queue.
pub const DLM_LKF_CONVERT: u32 = 0x0000_0004;
/// Operation will cancel a pending one.
pub const DLM_LKF_CANCEL: u32 = 0x0000_0080;
/// Persistent lock (survives client crash).
pub const DLM_LKF_PERSISTENT: u32 = 0x0000_0080;
/// Use LVB carry-through.
pub const DLM_LKF_VALBLK: u32 = 0x0000_0100;
/// Express directly, no demote.
pub const DLM_LKF_EXPEDITE: u32 = 0x0000_0400;
/// Force unlock.
pub const DLM_LKF_FORCEUNLOCK: u32 = 0x0000_2000;

// ---------------------------------------------------------------------------
// AST return codes
// ---------------------------------------------------------------------------

/// `DLM_SBF_DEMOTED` — lock was demoted.
pub const DLM_SBF_DEMOTED: u32 = 0x01;
/// `DLM_SBF_VALNOTVALID` — LVB is not valid.
pub const DLM_SBF_VALNOTVALID: u32 = 0x02;
/// `DLM_SBF_ALTMODE` — granted at alternate mode.
pub const DLM_SBF_ALTMODE: u32 = 0x04;

// ---------------------------------------------------------------------------
// User-side ioctls / device path
// ---------------------------------------------------------------------------

/// `/dev/misc/dlm-control` — control node for dlm_controld.
pub const DLM_CONTROL_NODE: &str = "/dev/misc/dlm-control";
/// `/dev/misc/dlm-monitor` — readonly state stream.
pub const DLM_MONITOR_NODE: &str = "/dev/misc/dlm-monitor";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_sizes() {
        // DLM names are limited to 64 bytes; cluster software relies on
        // this size for protocol packets.
        assert_eq!(DLM_LOCKSPACE_LEN, 64);
        assert_eq!(DLM_RESNAME_MAXLEN, 64);
        assert_eq!(DLM_LVB_LEN, 32);
        assert_eq!(DLM_USER_LVB_LEN, 32);
    }

    #[test]
    fn test_modes_dense_and_ordered() {
        // Lock modes NL..EX are 0..5, monotonically more restrictive.
        let m = [
            DLM_LOCK_NL,
            DLM_LOCK_CR,
            DLM_LOCK_CW,
            DLM_LOCK_PR,
            DLM_LOCK_PW,
            DLM_LOCK_EX,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(DLM_LOCK_IV, -1);
    }

    #[test]
    fn test_flag_bits_pow2() {
        // Single-bit flags except those that share a value by kernel
        // ABI (PERSISTENT and CANCEL overlap historically; documented).
        for &f in &[
            DLM_LKF_NOQUEUE,
            DLM_LKF_CONVERT,
            DLM_LKF_CANCEL,
            DLM_LKF_VALBLK,
            DLM_LKF_EXPEDITE,
            DLM_LKF_FORCEUNLOCK,
        ] {
            assert!(f.is_power_of_two());
        }
        // CANCEL and PERSISTENT share 0x80 because they apply to
        // different operation types and never co-occur.
        assert_eq!(DLM_LKF_CANCEL, DLM_LKF_PERSISTENT);
    }

    #[test]
    fn test_sbf_bits_distinct() {
        let s = [DLM_SBF_DEMOTED, DLM_SBF_VALNOTVALID, DLM_SBF_ALTMODE];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_device_node_paths() {
        assert!(DLM_CONTROL_NODE.starts_with("/dev/"));
        assert!(DLM_MONITOR_NODE.starts_with("/dev/"));
        assert_ne!(DLM_CONTROL_NODE, DLM_MONITOR_NODE);
    }
}
