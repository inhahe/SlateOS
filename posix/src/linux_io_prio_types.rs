//! `<linux/ioprio.h>` — I/O priority (ioprio) constants.
//!
//! Linux I/O priority determines the scheduling class and priority
//! level for block I/O operations. The BFQ and mq-deadline schedulers
//! use these to differentiate between real-time, best-effort, and
//! idle I/O, similar to CPU scheduling classes.

// ---------------------------------------------------------------------------
// I/O priority classes
// ---------------------------------------------------------------------------

/// No class set (inherit from CPU priority).
pub const IOPRIO_CLASS_NONE: u8 = 0;
/// Real-time I/O class (highest priority, 8 levels).
pub const IOPRIO_CLASS_RT: u8 = 1;
/// Best-effort I/O class (default, 8 levels).
pub const IOPRIO_CLASS_BE: u8 = 2;
/// Idle I/O class (only when no other I/O).
pub const IOPRIO_CLASS_IDLE: u8 = 3;

// ---------------------------------------------------------------------------
// Priority levels (within a class)
// ---------------------------------------------------------------------------

/// Highest priority within class.
pub const IOPRIO_LEVEL_HIGH: u8 = 0;
/// Default priority within best-effort class.
pub const IOPRIO_LEVEL_DEFAULT: u8 = 4;
/// Lowest priority within class.
pub const IOPRIO_LEVEL_LOW: u8 = 7;
/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u8 = 8;

// ---------------------------------------------------------------------------
// ioprio encoding (class << 13 | level)
// ---------------------------------------------------------------------------

/// Bit shift for class field.
pub const IOPRIO_CLASS_SHIFT: u8 = 13;
/// Mask for level field.
pub const IOPRIO_LEVEL_MASK: u16 = 0x1FFF;
/// Mask for class field (after shift).
pub const IOPRIO_CLASS_MASK: u16 = 0x07;

// ---------------------------------------------------------------------------
// Who specifier for ioprio_set/get
// ---------------------------------------------------------------------------

/// Set/get ioprio for process.
pub const IOPRIO_WHO_PROCESS: u32 = 1;
/// Set/get ioprio for process group.
pub const IOPRIO_WHO_PGRP: u32 = 2;
/// Set/get ioprio for user.
pub const IOPRIO_WHO_USER: u32 = 3;

// ---------------------------------------------------------------------------
// Hint flags (Linux 6.x+)
// ---------------------------------------------------------------------------

/// No hint.
pub const IOPRIO_HINT_NONE: u16 = 0;
/// Short-duration I/O.
pub const IOPRIO_HINT_DEV_DURATION_LIMIT_1: u16 = 1;
/// Medium-duration I/O.
pub const IOPRIO_HINT_DEV_DURATION_LIMIT_2: u16 = 2;
/// Long-duration I/O.
pub const IOPRIO_HINT_DEV_DURATION_LIMIT_3: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [
            IOPRIO_CLASS_NONE,
            IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE,
            IOPRIO_CLASS_IDLE,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_levels() {
        assert!(IOPRIO_LEVEL_HIGH < IOPRIO_LEVEL_DEFAULT);
        assert!(IOPRIO_LEVEL_DEFAULT < IOPRIO_LEVEL_LOW);
        assert_eq!(IOPRIO_NR_LEVELS, 8);
        assert!(IOPRIO_LEVEL_LOW < IOPRIO_NR_LEVELS);
    }

    #[test]
    fn test_encoding() {
        // Best-effort, level 4 = (2 << 13) | 4 = 0x4004
        let prio = (IOPRIO_CLASS_BE as u16) << IOPRIO_CLASS_SHIFT | (IOPRIO_LEVEL_DEFAULT as u16);
        assert_eq!(prio & IOPRIO_LEVEL_MASK, IOPRIO_LEVEL_DEFAULT as u16);
        assert_eq!(
            (prio >> IOPRIO_CLASS_SHIFT) & IOPRIO_CLASS_MASK,
            IOPRIO_CLASS_BE as u16
        );
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

    #[test]
    fn test_hints_distinct() {
        let hints = [
            IOPRIO_HINT_NONE,
            IOPRIO_HINT_DEV_DURATION_LIMIT_1,
            IOPRIO_HINT_DEV_DURATION_LIMIT_2,
            IOPRIO_HINT_DEV_DURATION_LIMIT_3,
        ];
        for i in 0..hints.len() {
            for j in (i + 1)..hints.len() {
                assert_ne!(hints[i], hints[j]);
            }
        }
    }
}
