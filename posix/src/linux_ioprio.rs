//! `<linux/ioprio.h>` — I/O priority definitions.
//!
//! Defines I/O scheduling class constants used by `ioprio_get()`
//! and `ioprio_set()`.

// Re-export the syscall wrappers.
pub use crate::process::ioprio_get;
pub use crate::process::ioprio_set;

// ---------------------------------------------------------------------------
// I/O priority classes
// ---------------------------------------------------------------------------

/// No I/O priority class (use CFQ default).
pub const IOPRIO_CLASS_NONE: i32 = 0;

/// Real-time I/O (highest priority, 0-7 levels).
pub const IOPRIO_CLASS_RT: i32 = 1;

/// Best-effort I/O (normal, 0-7 levels).
pub const IOPRIO_CLASS_BE: i32 = 2;

/// Idle I/O (only when disk is otherwise idle).
pub const IOPRIO_CLASS_IDLE: i32 = 3;

// ---------------------------------------------------------------------------
// "Who" values for ioprio_get/set
// ---------------------------------------------------------------------------

/// Set I/O priority for a process.
pub const IOPRIO_WHO_PROCESS: i32 = 1;

/// Set I/O priority for a process group.
pub const IOPRIO_WHO_PGRP: i32 = 2;

/// Set I/O priority for a user.
pub const IOPRIO_WHO_USER: i32 = 3;

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Bits used for class in the ioprio value.
pub const IOPRIO_CLASS_SHIFT: i32 = 13;

/// Extract the I/O class from an ioprio value.
#[inline]
pub const fn ioprio_prio_class(ioprio: i32) -> i32 {
    (ioprio >> IOPRIO_CLASS_SHIFT) & 0x7
}

/// Extract the priority data (0-7) from an ioprio value.
#[inline]
pub const fn ioprio_prio_data(ioprio: i32) -> i32 {
    ioprio & 0x1FFF
}

/// Build an ioprio value from class and data.
#[inline]
pub const fn ioprio_prio_value(class: i32, data: i32) -> i32 {
    (class << IOPRIO_CLASS_SHIFT) | (data & 0x1FFF)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [
            IOPRIO_CLASS_NONE, IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_who_distinct() {
        assert_ne!(IOPRIO_WHO_PROCESS, IOPRIO_WHO_PGRP);
        assert_ne!(IOPRIO_WHO_PGRP, IOPRIO_WHO_USER);
    }

    #[test]
    fn test_prio_value_roundtrip() {
        let val = ioprio_prio_value(IOPRIO_CLASS_BE, 4);
        assert_eq!(ioprio_prio_class(val), IOPRIO_CLASS_BE);
        assert_eq!(ioprio_prio_data(val), 4);
    }

    #[test]
    fn test_prio_value_rt() {
        let val = ioprio_prio_value(IOPRIO_CLASS_RT, 0);
        assert_eq!(ioprio_prio_class(val), IOPRIO_CLASS_RT);
        assert_eq!(ioprio_prio_data(val), 0);
    }

    #[test]
    fn test_prio_value_idle() {
        let val = ioprio_prio_value(IOPRIO_CLASS_IDLE, 0);
        assert_eq!(ioprio_prio_class(val), IOPRIO_CLASS_IDLE);
    }

    #[test]
    fn test_class_shift() {
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
    }
}
