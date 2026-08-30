//! `<linux/sched.h>` — SCHED_DEADLINE scheduling-class user interface.
//!
//! SCHED_DEADLINE implements EDF (Earliest Deadline First) + CBS
//! (Constant Bandwidth Server). Each task declares
//! (runtime, deadline, period) in nanoseconds; the kernel admits the
//! task only if the system has spare bandwidth.

// ---------------------------------------------------------------------------
// Policy number (passed to sched_setscheduler / sched_setattr)
// ---------------------------------------------------------------------------

pub const SCHED_DEADLINE: u32 = 6;

// ---------------------------------------------------------------------------
// struct sched_attr layout (see sched_setattr(2))
// ---------------------------------------------------------------------------

/// Size of struct sched_attr (kernel ABI).
pub const SCHED_ATTR_SIZE: usize = 48;

pub const SCHED_ATTR_OFF_SIZE: usize = 0;
pub const SCHED_ATTR_OFF_SCHED_POLICY: usize = 4;
pub const SCHED_ATTR_OFF_SCHED_FLAGS: usize = 8;
pub const SCHED_ATTR_OFF_SCHED_NICE: usize = 16;
pub const SCHED_ATTR_OFF_SCHED_PRIORITY: usize = 20;
pub const SCHED_ATTR_OFF_SCHED_RUNTIME: usize = 24;
pub const SCHED_ATTR_OFF_SCHED_DEADLINE: usize = 32;
pub const SCHED_ATTR_OFF_SCHED_PERIOD: usize = 40;

// ---------------------------------------------------------------------------
// sched_flags bits (sched_attr::sched_flags, SCHED_FLAG_*)
// ---------------------------------------------------------------------------

pub const SCHED_FLAG_RESET_ON_FORK: u64 = 0x01;
pub const SCHED_FLAG_RECLAIM: u64 = 0x02;
pub const SCHED_FLAG_DL_OVERRUN: u64 = 0x04;
pub const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
pub const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
pub const SCHED_FLAG_UTIL_CLAMP_MIN: u64 = 0x20;
pub const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;

pub const SCHED_FLAG_ALL: u64 = SCHED_FLAG_RESET_ON_FORK
    | SCHED_FLAG_RECLAIM
    | SCHED_FLAG_DL_OVERRUN
    | SCHED_FLAG_KEEP_POLICY
    | SCHED_FLAG_KEEP_PARAMS
    | SCHED_FLAG_UTIL_CLAMP_MIN
    | SCHED_FLAG_UTIL_CLAMP_MAX;

// ---------------------------------------------------------------------------
// Util-clamp value range
// ---------------------------------------------------------------------------

pub const SCHED_CAPACITY_SHIFT: u32 = 10;
pub const SCHED_CAPACITY_SCALE: u32 = 1 << SCHED_CAPACITY_SHIFT;

// ---------------------------------------------------------------------------
// Practical runtime/deadline/period bounds (nanoseconds)
// ---------------------------------------------------------------------------

/// Minimum admissible runtime — 1 microsecond.
pub const DL_RUNTIME_MIN_NS: u64 = 1_000;
/// Maximum admissible runtime — clamped to fit in i64 (kernel uses signed).
pub const DL_RUNTIME_MAX_NS: u64 = i64::MAX as u64;
/// Minimum admissible period — equal to RUNTIME_MIN.
pub const DL_PERIOD_MIN_NS: u64 = DL_RUNTIME_MIN_NS;

// ---------------------------------------------------------------------------
// Syscall numbers for sched_setattr / sched_getattr
// ---------------------------------------------------------------------------

pub const NR_SCHED_SETATTR_X86_64: u32 = 314;
pub const NR_SCHED_GETATTR_X86_64: u32 = 315;
pub const NR_SCHED_SETATTR_AARCH64: u32 = 274;
pub const NR_SCHED_GETATTR_AARCH64: u32 = 275;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_value_is_6() {
        assert_eq!(SCHED_DEADLINE, 6);
    }

    #[test]
    fn test_sched_attr_size_48() {
        assert_eq!(SCHED_ATTR_SIZE, 48);
    }

    #[test]
    fn test_sched_attr_offsets_strictly_increasing() {
        let off = [
            SCHED_ATTR_OFF_SIZE,
            SCHED_ATTR_OFF_SCHED_POLICY,
            SCHED_ATTR_OFF_SCHED_FLAGS,
            SCHED_ATTR_OFF_SCHED_NICE,
            SCHED_ATTR_OFF_SCHED_PRIORITY,
            SCHED_ATTR_OFF_SCHED_RUNTIME,
            SCHED_ATTR_OFF_SCHED_DEADLINE,
            SCHED_ATTR_OFF_SCHED_PERIOD,
        ];
        for w in off.windows(2) {
            assert!(w[1] > w[0]);
        }
        // Period field is the last u64, followed by 0 padding to 48.
        assert_eq!(SCHED_ATTR_OFF_SCHED_PERIOD + 8, SCHED_ATTR_SIZE);
    }

    #[test]
    fn test_sched_flags_single_bit_distinct() {
        let f = [
            SCHED_FLAG_RESET_ON_FORK,
            SCHED_FLAG_RECLAIM,
            SCHED_FLAG_DL_OVERRUN,
            SCHED_FLAG_KEEP_POLICY,
            SCHED_FLAG_KEEP_PARAMS,
            SCHED_FLAG_UTIL_CLAMP_MIN,
            SCHED_FLAG_UTIL_CLAMP_MAX,
        ];
        let mut or_all = 0u64;
        for &v in &f {
            assert!(v.is_power_of_two());
            or_all |= v;
        }
        assert_eq!(or_all, 0x7F);
        assert_eq!(SCHED_FLAG_ALL, or_all);
    }

    #[test]
    fn test_capacity_scale_is_1024() {
        assert_eq!(SCHED_CAPACITY_SCALE, 1024);
        assert!(SCHED_CAPACITY_SCALE.is_power_of_two());
    }

    #[test]
    fn test_runtime_bounds_sane() {
        assert!(DL_RUNTIME_MAX_NS > DL_RUNTIME_MIN_NS);
        assert_eq!(DL_RUNTIME_MIN_NS, 1_000);
        // Minimum period equals minimum runtime — task can declare
        // runtime == deadline == period.
        assert_eq!(DL_PERIOD_MIN_NS, DL_RUNTIME_MIN_NS);
    }

    #[test]
    fn test_setattr_getattr_pair_consecutive() {
        // On every arch, getattr immediately follows setattr.
        assert_eq!(NR_SCHED_GETATTR_X86_64, NR_SCHED_SETATTR_X86_64 + 1);
        assert_eq!(NR_SCHED_GETATTR_AARCH64, NR_SCHED_SETATTR_AARCH64 + 1);
    }

    #[test]
    fn test_clamp_flags_consecutive_bits() {
        assert_eq!(SCHED_FLAG_UTIL_CLAMP_MAX, SCHED_FLAG_UTIL_CLAMP_MIN << 1);
    }
}
