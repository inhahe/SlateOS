//! `<linux/ioprio.h>` — I/O priority (scheduling class) constants.
//!
//! I/O priority determines how the block layer scheduler orders
//! requests from a process. Higher-priority I/O is serviced first.
//! The priority is encoded as class (upper 3 bits) + level (lower
//! 13 bits) in a u16 value.

// ---------------------------------------------------------------------------
// I/O scheduling classes
// ---------------------------------------------------------------------------

/// No class set (use default).
pub const IOPRIO_CLASS_NONE: u32 = 0;
/// Real-time: guaranteed I/O bandwidth.
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort: fair scheduling.
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle: only when no other I/O is pending.
pub const IOPRIO_CLASS_IDLE: u32 = 3;

// ---------------------------------------------------------------------------
// I/O priority levels (within a class)
// ---------------------------------------------------------------------------

/// Highest priority within a class.
pub const IOPRIO_LEVEL_HIGHEST: u32 = 0;
/// Default priority level.
pub const IOPRIO_LEVEL_DEFAULT: u32 = 4;
/// Lowest priority within a class.
pub const IOPRIO_LEVEL_LOWEST: u32 = 7;
/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// I/O priority encoding helpers
// ---------------------------------------------------------------------------

/// Bit shift for class field.
pub const IOPRIO_CLASS_SHIFT: u32 = 13;
/// Mask for priority level field.
pub const IOPRIO_PRIO_MASK: u32 = (1 << IOPRIO_CLASS_SHIFT) - 1;

// ---------------------------------------------------------------------------
// ioprio_get/ioprio_set "who" argument
// ---------------------------------------------------------------------------

/// Priority for a specific process.
pub const IOPRIO_WHO_PROCESS: u32 = 1;
/// Priority for a process group.
pub const IOPRIO_WHO_PGRP: u32 = 2;
/// Priority for a user.
pub const IOPRIO_WHO_USER: u32 = 3;

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
    fn test_level_range() {
        assert_eq!(IOPRIO_LEVEL_HIGHEST, 0);
        assert_eq!(IOPRIO_LEVEL_LOWEST, 7);
        assert_eq!(IOPRIO_NR_LEVELS, 8);
    }

    #[test]
    fn test_encoding() {
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
        assert_eq!(IOPRIO_PRIO_MASK, 0x1FFF);
    }

    #[test]
    fn test_who_values_distinct() {
        let whos = [IOPRIO_WHO_PROCESS, IOPRIO_WHO_PGRP, IOPRIO_WHO_USER];
        for i in 0..whos.len() {
            for j in (i + 1)..whos.len() {
                assert_ne!(whos[i], whos[j]);
            }
        }
    }
}
