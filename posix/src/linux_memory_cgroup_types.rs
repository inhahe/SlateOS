//! `<linux/memcontrol.h>` — Memory cgroup controller constants.
//!
//! The memory cgroup controller (memcg) limits and accounts memory
//! usage for a group of processes. It tracks RSS, page cache, kernel
//! memory (slab, stack, socket buffers), and swap. When a cgroup
//! exceeds its limit, the OOM killer targets processes within that
//! cgroup. In cgroup v2, memory accounting is comprehensive and
//! includes all kernel memory consumed on behalf of the cgroup's
//! processes.

// ---------------------------------------------------------------------------
// Memory cgroup stat types
// ---------------------------------------------------------------------------

/// Current memory usage in bytes.
pub const MEMCG_STAT_CURRENT: u32 = 0;
/// RSS (resident set size) in pages.
pub const MEMCG_STAT_RSS: u32 = 1;
/// Page cache (file-backed pages) in pages.
pub const MEMCG_STAT_CACHE: u32 = 2;
/// Kernel slab memory.
pub const MEMCG_STAT_SLAB: u32 = 3;
/// Swap usage in pages.
pub const MEMCG_STAT_SWAP: u32 = 4;
/// Anonymous huge pages.
pub const MEMCG_STAT_ANON_THPS: u32 = 5;
/// Kernel stack memory.
pub const MEMCG_STAT_KERNEL_STACK: u32 = 6;
/// Socket buffer memory.
pub const MEMCG_STAT_SOCK: u32 = 7;
/// Percpu memory.
pub const MEMCG_STAT_PERCPU: u32 = 8;

// ---------------------------------------------------------------------------
// Memory cgroup event types (memory.events)
// ---------------------------------------------------------------------------

/// Number of times memory limit was hit (reclaim triggered).
pub const MEMCG_EVENT_LOW: u32 = 0;
/// Number of times high watermark was exceeded.
pub const MEMCG_EVENT_HIGH: u32 = 1;
/// Number of times hard limit was hit (allocation throttled).
pub const MEMCG_EVENT_MAX: u32 = 2;
/// Number of OOM kills in this cgroup.
pub const MEMCG_EVENT_OOM: u32 = 3;
/// Number of OOM killer invocations.
pub const MEMCG_EVENT_OOM_KILL: u32 = 4;
/// Number of group OOM kills (v2).
pub const MEMCG_EVENT_OOM_GROUP_KILL: u32 = 5;

// ---------------------------------------------------------------------------
// Memory pressure levels
// ---------------------------------------------------------------------------

/// No memory pressure.
pub const MEMCG_PRESSURE_NONE: u32 = 0;
/// Low memory pressure (some reclaim happening).
pub const MEMCG_PRESSURE_LOW: u32 = 1;
/// Medium memory pressure (significant reclaim).
pub const MEMCG_PRESSURE_MEDIUM: u32 = 2;
/// Critical memory pressure (near OOM).
pub const MEMCG_PRESSURE_CRITICAL: u32 = 3;

// ---------------------------------------------------------------------------
// Memory cgroup protection types (memory.low/memory.min)
// ---------------------------------------------------------------------------

/// Effective protection means best-effort (can be violated under pressure).
pub const MEMCG_PROT_LOW: u32 = 0;
/// Hard protection (will not be reclaimed below this).
pub const MEMCG_PROT_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// OOM control flags
// ---------------------------------------------------------------------------

/// Disable OOM killer for this cgroup (pause processes instead).
pub const MEMCG_OOM_DISABLED: u32 = 0;
/// Enable OOM killer for this cgroup (default).
pub const MEMCG_OOM_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_types_distinct() {
        let stats = [
            MEMCG_STAT_CURRENT, MEMCG_STAT_RSS, MEMCG_STAT_CACHE,
            MEMCG_STAT_SLAB, MEMCG_STAT_SWAP, MEMCG_STAT_ANON_THPS,
            MEMCG_STAT_KERNEL_STACK, MEMCG_STAT_SOCK, MEMCG_STAT_PERCPU,
        ];
        for i in 0..stats.len() {
            for j in (i + 1)..stats.len() {
                assert_ne!(stats[i], stats[j]);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        let events = [
            MEMCG_EVENT_LOW, MEMCG_EVENT_HIGH, MEMCG_EVENT_MAX,
            MEMCG_EVENT_OOM, MEMCG_EVENT_OOM_KILL,
            MEMCG_EVENT_OOM_GROUP_KILL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_pressure_levels_ordered() {
        assert!(MEMCG_PRESSURE_NONE < MEMCG_PRESSURE_LOW);
        assert!(MEMCG_PRESSURE_LOW < MEMCG_PRESSURE_MEDIUM);
        assert!(MEMCG_PRESSURE_MEDIUM < MEMCG_PRESSURE_CRITICAL);
    }

    #[test]
    fn test_protection_types_distinct() {
        assert_ne!(MEMCG_PROT_LOW, MEMCG_PROT_MIN);
    }

    #[test]
    fn test_oom_control_distinct() {
        assert_ne!(MEMCG_OOM_DISABLED, MEMCG_OOM_ENABLED);
    }
}
