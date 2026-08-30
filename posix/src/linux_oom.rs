//! `<linux/oom.h>` — Out-of-memory killer constants.
//!
//! The OOM killer selects and kills processes when the system runs
//! out of memory. It uses an OOM score (based on memory usage,
//! oom_score_adj, and other factors) to choose victims. Processes
//! can adjust their OOM priority via /proc/<pid>/oom_score_adj.

// ---------------------------------------------------------------------------
// OOM score adjustment
// ---------------------------------------------------------------------------

/// Minimum OOM score adjustment (most protected).
pub const OOM_SCORE_ADJ_MIN: i16 = -1000;
/// Maximum OOM score adjustment (least protected).
pub const OOM_SCORE_ADJ_MAX: i16 = 1000;
/// Default OOM score adjustment.
pub const OOM_SCORE_ADJ_DEFAULT: i16 = 0;

// ---------------------------------------------------------------------------
// OOM score (legacy /proc/<pid>/oom_adj, deprecated)
// ---------------------------------------------------------------------------

/// Legacy OOM disable value.
pub const OOM_DISABLE: i32 = -17;
/// Legacy OOM adjust minimum.
pub const OOM_ADJUST_MIN: i32 = -16;
/// Legacy OOM adjust maximum.
pub const OOM_ADJUST_MAX: i32 = 15;

// ---------------------------------------------------------------------------
// OOM control flags
// ---------------------------------------------------------------------------

/// OOM killer enabled.
pub const OOM_CONTROL_ENABLE: u32 = 0;
/// OOM killer disabled (for cgroup).
pub const OOM_CONTROL_DISABLE: u32 = 1;

// ---------------------------------------------------------------------------
// OOM victim selection
// ---------------------------------------------------------------------------

/// Kill process and all threads.
pub const OOM_KILL_PROCESS: u32 = 0;
/// Kill single thread only.
pub const OOM_KILL_THREAD: u32 = 1;

// ---------------------------------------------------------------------------
// OOM events (for cgroup memory.events)
// ---------------------------------------------------------------------------

/// OOM event occurred.
pub const OOM_EVENT_OOM: u32 = 0;
/// OOM kill event.
pub const OOM_EVENT_KILL: u32 = 1;
/// OOM group kill event.
pub const OOM_EVENT_GROUP_KILL: u32 = 2;

// ---------------------------------------------------------------------------
// Special OOM values
// ---------------------------------------------------------------------------

/// Maximum possible OOM score.
pub const OOM_SCORE_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_adj_range() {
        assert!(OOM_SCORE_ADJ_MIN < OOM_SCORE_ADJ_DEFAULT);
        assert!(OOM_SCORE_ADJ_DEFAULT < OOM_SCORE_ADJ_MAX);
        assert_eq!(OOM_SCORE_ADJ_MIN, -1000);
        assert_eq!(OOM_SCORE_ADJ_MAX, 1000);
    }

    #[test]
    fn test_legacy_adj_range() {
        assert!(OOM_DISABLE < OOM_ADJUST_MIN);
        assert!(OOM_ADJUST_MIN < OOM_ADJUST_MAX);
    }

    #[test]
    fn test_control_values() {
        assert_ne!(OOM_CONTROL_ENABLE, OOM_CONTROL_DISABLE);
    }

    #[test]
    fn test_kill_modes() {
        assert_ne!(OOM_KILL_PROCESS, OOM_KILL_THREAD);
    }

    #[test]
    fn test_events_distinct() {
        let events = [OOM_EVENT_OOM, OOM_EVENT_KILL, OOM_EVENT_GROUP_KILL];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_score_max() {
        assert_eq!(OOM_SCORE_MAX, 1000);
    }
}
