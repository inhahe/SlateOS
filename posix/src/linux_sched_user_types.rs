//! `<sched.h>` — `sched_getscheduler(2)` / `sched_setscheduler(2)` policies.
//!
//! The scheduler-policy enum controls whether a task is dispatched by
//! the fair scheduler (`SCHED_OTHER`/`SCHED_BATCH`/`SCHED_IDLE`), the
//! real-time scheduler (`SCHED_FIFO`/`SCHED_RR`), or the deadline
//! scheduler (`SCHED_DEADLINE`). These constants are used by
//! `chrt`, audio servers (PulseAudio, PipeWire), and any realtime
//! daemon.

// ---------------------------------------------------------------------------
// Scheduling policies
// ---------------------------------------------------------------------------

pub const SCHED_OTHER: u32 = 0;
pub const SCHED_FIFO: u32 = 1;
pub const SCHED_RR: u32 = 2;
pub const SCHED_BATCH: u32 = 3;
pub const SCHED_ISO: u32 = 4;
pub const SCHED_IDLE: u32 = 5;
pub const SCHED_DEADLINE: u32 = 6;
pub const SCHED_EXT: u32 = 7;

/// "Normal" — synonym for `SCHED_OTHER`.
pub const SCHED_NORMAL: u32 = SCHED_OTHER;

/// `SCHED_RESET_ON_FORK` — high bit OR'd into the policy to drop the
/// real-time policy on fork.
pub const SCHED_RESET_ON_FORK: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// `sched_setattr` flags (`SCHED_FLAG_*`)
// ---------------------------------------------------------------------------

pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
pub const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u64 = 0x20;
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;

pub const SCHED_FLAG_KEEP_ALL: u64 = SCHED_FLAG_KEEP_POLICY | SCHED_FLAG_KEEP_PARAMS;
pub const SCHED_FLAG_UTIL_CLAMP: u64 = SCHED_FLAG_UTIL_CLAMP_MIN | SCHED_FLAG_UTIL_CLAMP_MAX;
pub const SCHED_FLAG_ALL: u64 = SCHED_FLAG_RESET_ON_FORK
    | SCHED_FLAG_RECLAIM
    | SCHED_FLAG_DL_OVERRUN
    | SCHED_FLAG_KEEP_ALL
    | SCHED_FLAG_UTIL_CLAMP;

// ---------------------------------------------------------------------------
// Realtime priority bounds for FIFO/RR
// ---------------------------------------------------------------------------

pub const SCHED_PRIORITY_MIN: i32 = 1;
pub const SCHED_PRIORITY_MAX: i32 = 99;

// ---------------------------------------------------------------------------
// Util-clamp range (0..=1024 maps to 0%..100%)
// ---------------------------------------------------------------------------

pub const SCHED_CAPACITY_SHIFT: u32 = 10;
pub const SCHED_CAPACITY_SCALE: u32 = 1 << SCHED_CAPACITY_SHIFT;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_SCHED_SETSCHEDULER: u32 = 144;
pub const NR_SCHED_GETSCHEDULER: u32 = 145;
pub const NR_SCHED_SETPARAM: u32 = 142;
pub const NR_SCHED_GETPARAM: u32 = 143;
pub const NR_SCHED_SETATTR: u32 = 314;
pub const NR_SCHED_GETATTR: u32 = 315;
pub const NR_SCHED_YIELD: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_dense_0_to_7() {
        let p = [
            SCHED_OTHER,
            SCHED_FIFO,
            SCHED_RR,
            SCHED_BATCH,
            SCHED_ISO,
            SCHED_IDLE,
            SCHED_DEADLINE,
            SCHED_EXT,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // SCHED_NORMAL is just an alias for OTHER.
        assert_eq!(SCHED_NORMAL, SCHED_OTHER);
    }

    #[test]
    fn test_reset_on_fork_high_bit() {
        // The high bit toggles "drop RT on fork" when OR'd into the policy.
        assert_eq!(SCHED_RESET_ON_FORK, 0x4000_0000);
        assert!(SCHED_RESET_ON_FORK.is_power_of_two());
        // It must not collide with any actual policy number.
        for p in 0..=SCHED_EXT {
            assert_eq!(p & SCHED_RESET_ON_FORK, 0);
        }
    }

    #[test]
    fn test_setattr_flags_single_bit_and_compose() {
        let f = [
            SCHED_FLAG_RESET_ON_FORK,
            SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN,
            SCHED_FLAG_KEEP_POLICY,
            SCHED_FLAG_KEEP_PARAMS,
            SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Composite masks.
        assert_eq!(
            SCHED_FLAG_KEEP_ALL,
            SCHED_FLAG_KEEP_POLICY | SCHED_FLAG_KEEP_PARAMS
        );
        assert_eq!(
            SCHED_FLAG_UTIL_CLAMP,
            SCHED_FLAG_UTIL_CLAMP_MIN | SCHED_FLAG_UTIL_CLAMP_MAX
        );
        assert_eq!(SCHED_FLAG_ALL, 0x7F);
    }

    #[test]
    fn test_rt_priority_range() {
        // POSIX guarantees at least 32 priorities; Linux exposes 1..=99.
        assert_eq!(SCHED_PRIORITY_MIN, 1);
        assert_eq!(SCHED_PRIORITY_MAX, 99);
        assert!(SCHED_PRIORITY_MAX - SCHED_PRIORITY_MIN >= 31);
    }

    #[test]
    fn test_capacity_scale_is_1024() {
        // util-clamp uses a 0..=1024 range (10-bit fixed point).
        assert_eq!(SCHED_CAPACITY_SCALE, 1024);
        assert_eq!(SCHED_CAPACITY_SHIFT, 10);
        assert_eq!(SCHED_CAPACITY_SCALE, 1 << SCHED_CAPACITY_SHIFT);
    }

    #[test]
    fn test_syscall_numbers_distinct() {
        let n = [
            NR_SCHED_SETSCHEDULER,
            NR_SCHED_GETSCHEDULER,
            NR_SCHED_SETPARAM,
            NR_SCHED_GETPARAM,
            NR_SCHED_SETATTR,
            NR_SCHED_GETATTR,
            NR_SCHED_YIELD,
        ];
        for a in 0..n.len() {
            for b in (a + 1)..n.len() {
                assert_ne!(n[a], n[b]);
            }
        }
        // sched_yield is the famous "24".
        assert_eq!(NR_SCHED_YIELD, 24);
        // sched_getattr lives right after sched_setattr.
        assert_eq!(NR_SCHED_GETATTR, NR_SCHED_SETATTR + 1);
    }
}
