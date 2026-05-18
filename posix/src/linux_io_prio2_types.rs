//! `<linux/ioprio.h>` — I/O priority constants (extended).
//!
//! Extended I/O priority constants covering I/O scheduling
//! classes, priority levels, encoding/decoding macros, and
//! who-type selectors.

// ---------------------------------------------------------------------------
// I/O priority classes (IOPRIO_CLASS_*)
// ---------------------------------------------------------------------------

/// No class set (inherit from parent).
pub const IOPRIO_CLASS_NONE: u32 = 0;
/// Real-time I/O class.
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort I/O class.
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle I/O class (lowest priority).
pub const IOPRIO_CLASS_IDLE: u32 = 3;

// ---------------------------------------------------------------------------
// I/O priority class encoding
// ---------------------------------------------------------------------------

/// Class shift (in priority word).
pub const IOPRIO_CLASS_SHIFT: u32 = 13;
/// Class mask (3 bits).
pub const IOPRIO_CLASS_MASK: u32 = 0x07;
/// Data mask (priority level, 13 bits).
pub const IOPRIO_PRIO_MASK: u32 = 0x1FFF;

// ---------------------------------------------------------------------------
// I/O priority who types
// ---------------------------------------------------------------------------

/// Process (by PID).
pub const IOPRIO_WHO_PROCESS: u32 = 1;
/// Process group.
pub const IOPRIO_WHO_PGRP: u32 = 2;
/// User (by UID).
pub const IOPRIO_WHO_USER: u32 = 3;

// ---------------------------------------------------------------------------
// I/O priority levels (within class)
// ---------------------------------------------------------------------------

/// Highest priority within class.
pub const IOPRIO_LEVEL_HIGHEST: u32 = 0;
/// Normal priority within class.
pub const IOPRIO_LEVEL_NORMAL: u32 = 4;
/// Lowest priority within class.
pub const IOPRIO_LEVEL_LOWEST: u32 = 7;
/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// I/O priority hints
// ---------------------------------------------------------------------------

/// No hint.
pub const IOPRIO_HINT_NONE: u32 = 0;
/// Duration short.
pub const IOPRIO_HINT_DEV_DURATION_SHORT: u32 = 1;
/// Duration medium.
pub const IOPRIO_HINT_DEV_DURATION_MEDIUM: u32 = 2;
/// Duration long.
pub const IOPRIO_HINT_DEV_DURATION_LONG: u32 = 3;

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
    fn test_who_types_distinct() {
        let whos = [IOPRIO_WHO_PROCESS, IOPRIO_WHO_PGRP, IOPRIO_WHO_USER];
        for i in 0..whos.len() {
            for j in (i + 1)..whos.len() {
                assert_ne!(whos[i], whos[j]);
            }
        }
    }

    #[test]
    fn test_levels() {
        assert!(IOPRIO_LEVEL_HIGHEST < IOPRIO_LEVEL_NORMAL);
        assert!(IOPRIO_LEVEL_NORMAL < IOPRIO_LEVEL_LOWEST);
        assert!(IOPRIO_LEVEL_LOWEST < IOPRIO_NR_LEVELS);
    }

    #[test]
    fn test_class_shift() {
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
    }

    #[test]
    fn test_prio_mask() {
        assert_eq!(IOPRIO_PRIO_MASK, 0x1FFF);
    }

    #[test]
    fn test_nr_levels() {
        assert_eq!(IOPRIO_NR_LEVELS, 8);
    }

    #[test]
    fn test_hints_distinct() {
        let hints = [
            IOPRIO_HINT_NONE, IOPRIO_HINT_DEV_DURATION_SHORT,
            IOPRIO_HINT_DEV_DURATION_MEDIUM,
            IOPRIO_HINT_DEV_DURATION_LONG,
        ];
        for i in 0..hints.len() {
            for j in (i + 1)..hints.len() {
                assert_ne!(hints[i], hints[j]);
            }
        }
    }

    #[test]
    fn test_none_class_is_zero() {
        assert_eq!(IOPRIO_CLASS_NONE, 0);
    }
}
