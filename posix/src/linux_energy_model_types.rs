//! `<linux/energy_model.h>` — Energy Model (EM) framework constants.
//!
//! The Energy Model provides a per-device table of performance states
//! and their associated power costs. The EAS (Energy Aware Scheduler)
//! uses the EM to make task placement decisions that minimize energy
//! consumption while meeting performance requirements. Each
//! performance domain (group of CPUs sharing frequency) has a table
//! mapping frequency → power, allowing the scheduler to compute the
//! energy cost of different task placements.

// ---------------------------------------------------------------------------
// Energy model performance domain flags
// ---------------------------------------------------------------------------

/// Domain uses milliwatts (default is abstract cost units).
pub const EM_PERF_DOMAIN_MILLIWATTS: u32 = 1 << 0;
/// Domain uses artificial (estimated) power values.
pub const EM_PERF_DOMAIN_ARTIFICIAL: u32 = 1 << 1;
/// Domain should skip for EAS (not usable by scheduler).
pub const EM_PERF_DOMAIN_SKIP_INEFFICIENCIES: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Energy model CPU flags (per performance state)
// ---------------------------------------------------------------------------

/// This performance state is inefficient (dominated by another).
pub const EM_PERF_STATE_INEFFICIENT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Energy model registration sources
// ---------------------------------------------------------------------------

/// Registered by cpufreq driver.
pub const EM_SOURCE_CPUFREQ: u32 = 0;
/// Registered by device tree.
pub const EM_SOURCE_DT: u32 = 1;
/// Registered by platform driver.
pub const EM_SOURCE_PLATFORM: u32 = 2;
/// Registered by user (sysfs override).
pub const EM_SOURCE_USER: u32 = 3;

// ---------------------------------------------------------------------------
// Energy model capacity scale
// ---------------------------------------------------------------------------

/// Maximum capacity value (1024, matching scheduler capacity).
pub const EM_MAX_CAPACITY: u32 = 1024;
/// Minimum capacity value.
pub const EM_MIN_CAPACITY: u32 = 1;

// ---------------------------------------------------------------------------
// Energy model update flags
// ---------------------------------------------------------------------------

/// EM data has been updated (notify scheduler to recompute).
pub const EM_UPDATE_PERF_DOMAIN: u32 = 1 << 0;
/// EM inefficiency markers have been updated.
pub const EM_UPDATE_INEFFICIENCY: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_flags_no_overlap() {
        let flags = [
            EM_PERF_DOMAIN_MILLIWATTS,
            EM_PERF_DOMAIN_ARTIFICIAL,
            EM_PERF_DOMAIN_SKIP_INEFFICIENCIES,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_state_flags() {
        assert_eq!(EM_PERF_STATE_INEFFICIENT, 1);
        assert!(EM_PERF_STATE_INEFFICIENT.is_power_of_two());
    }

    #[test]
    fn test_sources_distinct() {
        let sources = [
            EM_SOURCE_CPUFREQ,
            EM_SOURCE_DT,
            EM_SOURCE_PLATFORM,
            EM_SOURCE_USER,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_capacity_bounds() {
        assert!(EM_MIN_CAPACITY < EM_MAX_CAPACITY);
        assert_eq!(EM_MAX_CAPACITY, 1024);
        assert!(EM_MAX_CAPACITY.is_power_of_two());
    }

    #[test]
    fn test_update_flags_no_overlap() {
        let flags = [EM_UPDATE_PERF_DOMAIN, EM_UPDATE_INEFFICIENCY];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }
}
