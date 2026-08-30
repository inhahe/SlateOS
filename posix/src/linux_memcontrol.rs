//! `<linux/memcontrol.h>` — Memory cgroup controller constants.
//!
//! The memory cgroup (memcg) controller limits and tracks memory
//! usage per cgroup. It enforces hard/soft limits, tracks RSS,
//! cache, swap, and kernel memory, and triggers OOM kills when
//! a cgroup exceeds its limit.

// ---------------------------------------------------------------------------
// Memory cgroup stat types
// ---------------------------------------------------------------------------

/// Cache (page cache).
pub const MEMCG_CACHE: u32 = 0;
/// RSS (resident set size).
pub const MEMCG_RSS: u32 = 1;
/// RSS huge pages.
pub const MEMCG_RSS_HUGE: u32 = 2;
/// Shmem (shared memory).
pub const MEMCG_SHMEM: u32 = 3;
/// Mapped file.
pub const MEMCG_MAPPED_FILE: u32 = 4;
/// Dirty pages.
pub const MEMCG_DIRTY: u32 = 5;
/// Pages under writeback.
pub const MEMCG_WRITEBACK: u32 = 6;
/// Swap usage.
pub const MEMCG_SWAP: u32 = 7;

// ---------------------------------------------------------------------------
// Memory cgroup events
// ---------------------------------------------------------------------------

/// Low event (below low boundary).
pub const MEMCG_LOW: u32 = 0;
/// High event (above high boundary).
pub const MEMCG_HIGH: u32 = 1;
/// Max event (at hard limit).
pub const MEMCG_MAX: u32 = 2;
/// OOM event.
pub const MEMCG_OOM: u32 = 3;
/// OOM kill event.
pub const MEMCG_OOM_KILL: u32 = 4;
/// OOM group kill.
pub const MEMCG_OOM_GROUP_KILL: u32 = 5;

// ---------------------------------------------------------------------------
// Memory cgroup file names (cgroup v2)
// ---------------------------------------------------------------------------

/// Current memory usage.
pub const MEMCG_FILE_CURRENT: &str = "memory.current";
/// Memory high boundary.
pub const MEMCG_FILE_HIGH: &str = "memory.high";
/// Memory hard limit.
pub const MEMCG_FILE_MAX: &str = "memory.max";
/// Memory low boundary.
pub const MEMCG_FILE_LOW: &str = "memory.low";
/// Memory minimum guarantee.
pub const MEMCG_FILE_MIN: &str = "memory.min";
/// Memory stats.
pub const MEMCG_FILE_STAT: &str = "memory.stat";
/// Swap current usage.
pub const MEMCG_FILE_SWAP_CURRENT: &str = "memory.swap.current";
/// Swap hard limit.
pub const MEMCG_FILE_SWAP_MAX: &str = "memory.swap.max";
/// Events file.
pub const MEMCG_FILE_EVENTS: &str = "memory.events";
/// Pressure stall info.
pub const MEMCG_FILE_PRESSURE: &str = "memory.pressure";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum memory limit (no limit).
pub const MEMCG_LIMIT_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_types_distinct() {
        let types = [
            MEMCG_CACHE,
            MEMCG_RSS,
            MEMCG_RSS_HUGE,
            MEMCG_SHMEM,
            MEMCG_MAPPED_FILE,
            MEMCG_DIRTY,
            MEMCG_WRITEBACK,
            MEMCG_SWAP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            MEMCG_LOW,
            MEMCG_HIGH,
            MEMCG_MAX,
            MEMCG_OOM,
            MEMCG_OOM_KILL,
            MEMCG_OOM_GROUP_KILL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_file_names_distinct() {
        let files = [
            MEMCG_FILE_CURRENT,
            MEMCG_FILE_HIGH,
            MEMCG_FILE_MAX,
            MEMCG_FILE_LOW,
            MEMCG_FILE_MIN,
            MEMCG_FILE_STAT,
            MEMCG_FILE_SWAP_CURRENT,
            MEMCG_FILE_SWAP_MAX,
            MEMCG_FILE_EVENTS,
            MEMCG_FILE_PRESSURE,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_limit_max() {
        assert_eq!(MEMCG_LIMIT_MAX, u64::MAX);
    }
}
