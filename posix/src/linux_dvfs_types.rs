//! `<linux/pm_opp.h>` — Dynamic Voltage and Frequency Scaling constants.
//!
//! DVFS allows the kernel to adjust CPU/GPU voltage and frequency
//! operating points (OPPs) to balance performance and power
//! consumption. These constants define OPP table flags, transition
//! latency limits, and efficiency classes.

// ---------------------------------------------------------------------------
// OPP availability flags
// ---------------------------------------------------------------------------

/// OPP is available for use.
pub const OPP_AVAILABLE: u32 = 1;
/// OPP is not available (disabled by hardware or firmware).
pub const OPP_UNAVAILABLE: u32 = 0;

// ---------------------------------------------------------------------------
// OPP type flags
// ---------------------------------------------------------------------------

/// OPP is a turbo/boost frequency.
pub const OPP_TURBO: u32 = 1 << 0;
/// OPP requires special voltage handling.
pub const OPP_SUSPEND: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Transition latency constants (nanoseconds)
// ---------------------------------------------------------------------------

/// Unknown/unspecified transition latency.
pub const CPUFREQ_ETERNAL: u32 = u32::MAX;
/// Typical fast transition (10 microseconds in ns).
pub const DVFS_LATENCY_FAST_NS: u32 = 10_000;
/// Typical slow transition (100 microseconds in ns).
pub const DVFS_LATENCY_SLOW_NS: u32 = 100_000;
/// Maximum acceptable latency (1 millisecond in ns).
pub const DVFS_LATENCY_MAX_NS: u32 = 1_000_000;

// ---------------------------------------------------------------------------
// Energy/efficiency model constants
// ---------------------------------------------------------------------------

/// Default efficiency class (no class assigned).
pub const EM_PERF_CLASS_DEFAULT: u32 = 0;
/// High performance class.
pub const EM_PERF_CLASS_HIGH: u32 = 1;
/// Energy efficient class.
pub const EM_PERF_CLASS_EFFICIENT: u32 = 2;

// ---------------------------------------------------------------------------
// Voltage domains
// ---------------------------------------------------------------------------

/// No voltage domain specified.
pub const VOLTAGE_DOMAIN_NONE: u32 = 0;
/// CPU core voltage domain.
pub const VOLTAGE_DOMAIN_CPU: u32 = 1;
/// GPU voltage domain.
pub const VOLTAGE_DOMAIN_GPU: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opp_availability() {
        assert_eq!(OPP_AVAILABLE, 1);
        assert_eq!(OPP_UNAVAILABLE, 0);
        assert_ne!(OPP_AVAILABLE, OPP_UNAVAILABLE);
    }

    #[test]
    fn test_opp_type_flags() {
        assert_eq!(OPP_TURBO & OPP_SUSPEND, 0);
        assert!(OPP_TURBO.is_power_of_two());
        assert!(OPP_SUSPEND.is_power_of_two());
    }

    #[test]
    fn test_latency_ordering() {
        assert!(DVFS_LATENCY_FAST_NS < DVFS_LATENCY_SLOW_NS);
        assert!(DVFS_LATENCY_SLOW_NS < DVFS_LATENCY_MAX_NS);
        assert!(DVFS_LATENCY_MAX_NS < CPUFREQ_ETERNAL);
    }

    #[test]
    fn test_eternal() {
        assert_eq!(CPUFREQ_ETERNAL, u32::MAX);
    }

    #[test]
    fn test_perf_classes_distinct() {
        let classes = [
            EM_PERF_CLASS_DEFAULT, EM_PERF_CLASS_HIGH,
            EM_PERF_CLASS_EFFICIENT,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_voltage_domains_distinct() {
        let domains = [
            VOLTAGE_DOMAIN_NONE, VOLTAGE_DOMAIN_CPU,
            VOLTAGE_DOMAIN_GPU,
        ];
        for i in 0..domains.len() {
            for j in (i + 1)..domains.len() {
                assert_ne!(domains[i], domains[j]);
            }
        }
    }
}
