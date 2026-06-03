//! `<linux/workqueue.h>` (kernel-internal, surfaced to userspace via
//! `/sys/devices/virtual/workqueue/`) — kernel workqueue control.
//!
//! Workqueues are how kernel code defers work to a thread context.
//! Userspace can tune their CPU affinity and concurrency via sysfs;
//! `tuned` and large server installs poke these knobs to pin
//! workqueues off latency-sensitive CPUs.

// ---------------------------------------------------------------------------
// sysfs paths
// ---------------------------------------------------------------------------

pub const SYS_WORKQUEUE_ROOT: &str = "/sys/devices/virtual/workqueue";
pub const SYS_WORKQUEUE_CPUMASK: &str = "/sys/devices/virtual/workqueue/cpumask";
pub const SYS_WORKQUEUE_DEBUG: &str = "/sys/kernel/debug/workqueue";

// ---------------------------------------------------------------------------
// Per-workqueue attribute file names (relative to
// `/sys/devices/virtual/workqueue/<name>/`)
// ---------------------------------------------------------------------------

pub const WQ_ATTR_CPUMASK: &str = "cpumask";
pub const WQ_ATTR_NICE: &str = "nice";
pub const WQ_ATTR_NUMA: &str = "numa";
pub const WQ_ATTR_PER_CPU: &str = "per_cpu";
pub const WQ_ATTR_MAX_ACTIVE: &str = "max_active";
pub const WQ_ATTR_AFFINITY_SCOPE: &str = "affinity_scope";
pub const WQ_ATTR_AFFINITY_STRICT: &str = "affinity_strict";

// ---------------------------------------------------------------------------
// `WQ_*` flags accepted by `alloc_workqueue()` (kernel-internal API
// but surfaced via debugfs)
// ---------------------------------------------------------------------------

pub const WQ_UNBOUND: u32 = 1 << 1;
pub const WQ_FREEZABLE: u32 = 1 << 2;
pub const WQ_MEM_RECLAIM: u32 = 1 << 3;
pub const WQ_HIGHPRI: u32 = 1 << 4;
pub const WQ_CPU_INTENSIVE: u32 = 1 << 5;
pub const WQ_SYSFS: u32 = 1 << 6;
pub const WQ_POWER_EFFICIENT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Defaults & limits
// ---------------------------------------------------------------------------

/// 0 = "use the kernel default" for `max_active`.
pub const WQ_DFL_ACTIVE: u32 = 0;
/// Max value for `max_active` (`WQ_MAX_ACTIVE` in kernel).
pub const WQ_MAX_ACTIVE: u32 = 512;
/// Bound workqueues are capped per CPU at this many runnable items.
pub const WQ_DFL_PER_CPU: u32 = 256;

// ---------------------------------------------------------------------------
// Affinity-scope string values
// ---------------------------------------------------------------------------

pub const WQ_AFFINITY_CPU: &str = "cpu";
pub const WQ_AFFINITY_SMT: &str = "smt";
pub const WQ_AFFINITY_CACHE: &str = "cache";
pub const WQ_AFFINITY_NUMA: &str = "numa";
pub const WQ_AFFINITY_SYSTEM: &str = "system";

// ---------------------------------------------------------------------------
// Common kernel workqueue names (visible in /sys/devices/virtual/workqueue)
// ---------------------------------------------------------------------------

pub const WQ_NAME_EVENTS: &str = "events";
pub const WQ_NAME_EVENTS_LONG: &str = "events_long";
pub const WQ_NAME_EVENTS_UNBOUND: &str = "events_unbound";
pub const WQ_NAME_EVENTS_FREEZABLE: &str = "events_freezable";
pub const WQ_NAME_EVENTS_HIGHPRI: &str = "events_highpri";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_workqueue_root() {
        assert!(SYS_WORKQUEUE_CPUMASK.starts_with(SYS_WORKQUEUE_ROOT));
        // debug is a separate root.
        assert!(SYS_WORKQUEUE_DEBUG.starts_with("/sys/kernel/debug"));
    }

    #[test]
    fn test_attr_names_distinct() {
        let a = [
            WQ_ATTR_CPUMASK,
            WQ_ATTR_NICE,
            WQ_ATTR_NUMA,
            WQ_ATTR_PER_CPU,
            WQ_ATTR_MAX_ACTIVE,
            WQ_ATTR_AFFINITY_SCOPE,
            WQ_ATTR_AFFINITY_STRICT,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_wq_flags_dense_bits_1_to_7() {
        let f = [
            WQ_UNBOUND, WQ_FREEZABLE, WQ_MEM_RECLAIM, WQ_HIGHPRI, WQ_CPU_INTENSIVE, WQ_SYSFS,
            WQ_POWER_EFFICIENT,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << (i + 1));
        }
    }

    #[test]
    fn test_defaults_and_limits() {
        assert_eq!(WQ_DFL_ACTIVE, 0);
        // WQ_MAX_ACTIVE is a power of two.
        assert!(WQ_MAX_ACTIVE.is_power_of_two());
        assert_eq!(WQ_MAX_ACTIVE, 512);
        assert!(WQ_DFL_PER_CPU < WQ_MAX_ACTIVE);
    }

    #[test]
    fn test_affinity_scopes_distinct() {
        let s = [
            WQ_AFFINITY_CPU,
            WQ_AFFINITY_SMT,
            WQ_AFFINITY_CACHE,
            WQ_AFFINITY_NUMA,
            WQ_AFFINITY_SYSTEM,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_common_wq_names_start_with_events() {
        for n in [
            WQ_NAME_EVENTS,
            WQ_NAME_EVENTS_LONG,
            WQ_NAME_EVENTS_UNBOUND,
            WQ_NAME_EVENTS_FREEZABLE,
            WQ_NAME_EVENTS_HIGHPRI,
        ] {
            assert!(n.starts_with("events"));
        }
    }
}
