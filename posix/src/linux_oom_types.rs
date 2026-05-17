//! `<linux/oom.h>` — OOM (Out-of-Memory) killer constants.
//!
//! When the system runs out of memory and cannot reclaim pages, the
//! OOM killer selects a process to terminate. Each process has an
//! oom_score (0-1000) influenced by its oom_score_adj (-1000 to +1000).
//! Lower scores mean less likely to be killed. Setting oom_score_adj
//! to -1000 disables OOM killing for that process.

// ---------------------------------------------------------------------------
// oom_score_adj range
// ---------------------------------------------------------------------------

/// Minimum oom_score_adj (OOM immune).
pub const OOM_SCORE_ADJ_MIN: i32 = -1000;
/// Maximum oom_score_adj (most likely to be killed).
pub const OOM_SCORE_ADJ_MAX: i32 = 1000;

// ---------------------------------------------------------------------------
// Legacy oom_adj range (deprecated, use oom_score_adj)
// ---------------------------------------------------------------------------

/// Minimum legacy oom_adj (OOM immune).
pub const OOM_ADJ_MIN: i32 = -17;
/// Maximum legacy oom_adj.
pub const OOM_ADJ_MAX: i32 = 15;
/// Legacy OOM disable value.
pub const OOM_DISABLE: i32 = -17;

// ---------------------------------------------------------------------------
// OOM control cgroup values
// ---------------------------------------------------------------------------

/// OOM killer enabled for this cgroup.
pub const OOM_CONTROL_ENABLED: u32 = 0;
/// OOM killer disabled for this cgroup (processes pause instead).
pub const OOM_CONTROL_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// OOM policy flags (memory.oom.group in cgroup v2)
// ---------------------------------------------------------------------------

/// Kill individual process (default).
pub const OOM_POLICY_PROCESS: u32 = 0;
/// Kill entire cgroup (all processes in group).
pub const OOM_POLICY_GROUP: u32 = 1;

// ---------------------------------------------------------------------------
// OOM kill scores
// ---------------------------------------------------------------------------

/// Minimum OOM score (never killed in normal circumstances).
pub const OOM_SCORE_MIN: u32 = 0;
/// Maximum OOM score.
pub const OOM_SCORE_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_adj_range() {
        assert!(OOM_SCORE_ADJ_MIN < 0);
        assert!(OOM_SCORE_ADJ_MAX > 0);
        assert_eq!(OOM_SCORE_ADJ_MIN, -1000);
        assert_eq!(OOM_SCORE_ADJ_MAX, 1000);
    }

    #[test]
    fn test_legacy_adj_range() {
        assert!(OOM_ADJ_MIN < OOM_ADJ_MAX);
        assert_eq!(OOM_DISABLE, OOM_ADJ_MIN);
    }

    #[test]
    fn test_control_distinct() {
        assert_ne!(OOM_CONTROL_ENABLED, OOM_CONTROL_DISABLED);
    }

    #[test]
    fn test_policy_distinct() {
        assert_ne!(OOM_POLICY_PROCESS, OOM_POLICY_GROUP);
    }

    #[test]
    fn test_score_range() {
        assert_eq!(OOM_SCORE_MIN, 0);
        assert_eq!(OOM_SCORE_MAX, 1000);
    }
}
