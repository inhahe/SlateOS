//! `<linux/ioprio.h>` — I/O priority (scheduling class) constants.
//!
//! I/O priority determines how the block I/O scheduler prioritizes
//! requests from different processes. Priorities are organized into
//! classes (real-time, best-effort, idle) with per-class levels.
//! Set via ioprio_set(2) / ioprio_get(2).

// ---------------------------------------------------------------------------
// I/O priority classes
// ---------------------------------------------------------------------------

/// No priority set (inherit from process nice value).
pub const IOPRIO_CLASS_NONE: u32 = 0;
/// Real-time I/O (highest priority, starves others).
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort I/O (default, 8 priority levels).
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle I/O (only when no other I/O pending).
pub const IOPRIO_CLASS_IDLE: u32 = 3;

// ---------------------------------------------------------------------------
// Priority level range
// ---------------------------------------------------------------------------

/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;
/// Highest priority within a class (0 = highest).
pub const IOPRIO_LEVEL_HIGHEST: u32 = 0;
/// Lowest priority within a class.
pub const IOPRIO_LEVEL_LOWEST: u32 = 7;
/// Default best-effort level (4).
pub const IOPRIO_BE_DEFAULT_LEVEL: u32 = 4;

// ---------------------------------------------------------------------------
// Priority encoding (class << SHIFT | level)
// ---------------------------------------------------------------------------

/// Bit shift for class in priority value.
pub const IOPRIO_CLASS_SHIFT: u32 = 13;
/// Mask for extracting level from priority value.
pub const IOPRIO_LEVEL_MASK: u32 = (1 << IOPRIO_CLASS_SHIFT) - 1;
/// Mask for extracting class from priority value.
pub const IOPRIO_CLASS_MASK: u32 = 0x07;

// ---------------------------------------------------------------------------
// ioprio_set/get "who" argument
// ---------------------------------------------------------------------------

/// Set/get priority for a process.
pub const IOPRIO_WHO_PROCESS: u32 = 1;
/// Set/get priority for a process group.
pub const IOPRIO_WHO_PGRP: u32 = 2;
/// Set/get priority for a user.
pub const IOPRIO_WHO_USER: u32 = 3;

// ---------------------------------------------------------------------------
// Helper constants
// ---------------------------------------------------------------------------

/// Default best-effort priority value (class=BE, level=4).
pub const IOPRIO_DEFAULT: u32 = (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | IOPRIO_BE_DEFAULT_LEVEL;

/// Idle priority value (class=IDLE, level=0).
pub const IOPRIO_IDLE: u32 = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;

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
        assert!(IOPRIO_BE_DEFAULT_LEVEL <= IOPRIO_LEVEL_LOWEST);
    }

    #[test]
    fn test_encoding() {
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
        assert_eq!(IOPRIO_LEVEL_MASK, 0x1FFF);
        // Extract class from default priority
        assert_eq!((IOPRIO_DEFAULT >> IOPRIO_CLASS_SHIFT) & IOPRIO_CLASS_MASK, IOPRIO_CLASS_BE);
        // Extract level from default priority
        assert_eq!(IOPRIO_DEFAULT & IOPRIO_LEVEL_MASK, IOPRIO_BE_DEFAULT_LEVEL);
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
    fn test_idle_priority() {
        let class = (IOPRIO_IDLE >> IOPRIO_CLASS_SHIFT) & IOPRIO_CLASS_MASK;
        assert_eq!(class, IOPRIO_CLASS_IDLE);
    }
}
