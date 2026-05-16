//! `<linux/energy_model.h>` — Energy model constants.
//!
//! The energy model (EM) framework provides per-device power
//! consumption data at each performance state. Used by the
//! Energy Aware Scheduler (EAS), thermal governors, and devfreq
//! to make power-optimal placement and frequency decisions.

// ---------------------------------------------------------------------------
// EM flags
// ---------------------------------------------------------------------------

/// CPU device type.
pub const EM_PERF_DOMAIN_CPU: u32 = 1 << 0;
/// Milliwatt precision power values.
pub const EM_PERF_DOMAIN_MILLIWATTS: u32 = 1 << 1;
/// Skip inefficient states.
pub const EM_PERF_DOMAIN_SKIP_INEFFICIENCIES: u32 = 1 << 2;
/// Artificial (software-defined) performance states.
pub const EM_PERF_DOMAIN_ARTIFICIAL: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// EM device types
// ---------------------------------------------------------------------------

/// CPU energy model.
pub const EM_DEV_TYPE_CPU: u32 = 0;
/// GPU energy model.
pub const EM_DEV_TYPE_GPU: u32 = 1;
/// DSP energy model.
pub const EM_DEV_TYPE_DSP: u32 = 2;
/// Generic device energy model.
pub const EM_DEV_TYPE_OTHER: u32 = 3;

// ---------------------------------------------------------------------------
// EM callback actions
// ---------------------------------------------------------------------------

/// Get cost (from driver).
pub const EM_GET_COST: u32 = 0;
/// Update power data.
pub const EM_UPDATE_POWER: u32 = 1;

// ---------------------------------------------------------------------------
// EM limits
// ---------------------------------------------------------------------------

/// Maximum number of performance states per domain.
pub const EM_MAX_NUM_PERF_STATES: u32 = 64;

/// Maximum number of performance domains.
pub const EM_MAX_PERF_DOMAINS: u32 = 256;

// ---------------------------------------------------------------------------
// EM efficiency state
// ---------------------------------------------------------------------------

/// State is efficient (Pareto-optimal in the power-performance curve).
pub const EM_STATE_EFFICIENT: u32 = 0;
/// State is inefficient (dominated by another state).
pub const EM_STATE_INEFFICIENT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_flags_powers_of_two() {
        let flags = [
            EM_PERF_DOMAIN_CPU, EM_PERF_DOMAIN_MILLIWATTS,
            EM_PERF_DOMAIN_SKIP_INEFFICIENCIES, EM_PERF_DOMAIN_ARTIFICIAL,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_domain_flags_no_overlap() {
        let flags = [
            EM_PERF_DOMAIN_CPU, EM_PERF_DOMAIN_MILLIWATTS,
            EM_PERF_DOMAIN_SKIP_INEFFICIENCIES, EM_PERF_DOMAIN_ARTIFICIAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dev_types_distinct() {
        let types = [
            EM_DEV_TYPE_CPU, EM_DEV_TYPE_GPU,
            EM_DEV_TYPE_DSP, EM_DEV_TYPE_OTHER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_callback_actions_distinct() {
        assert_ne!(EM_GET_COST, EM_UPDATE_POWER);
    }

    #[test]
    fn test_limits() {
        assert_eq!(EM_MAX_NUM_PERF_STATES, 64);
        assert_eq!(EM_MAX_PERF_DOMAINS, 256);
    }

    #[test]
    fn test_efficiency_states_distinct() {
        assert_ne!(EM_STATE_EFFICIENT, EM_STATE_INEFFICIENT);
    }
}
