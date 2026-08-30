//! `<linux/cpuidle.h>` — CPU idle subsystem sysfs interface.
//!
//! cpuidle exposes each logical CPU's available C-states (deep sleep
//! modes) and their entry/exit overheads. The current governor (menu,
//! teo, ladder, haltpoll) picks which state to enter on each idle
//! period to balance power vs latency.

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

pub const CPUIDLE_SYSFS_ROOT: &str = "/sys/devices/system/cpu/cpuidle";
pub const CPUIDLE_SYSFS_CPU_PREFIX: &str = "/sys/devices/system/cpu/cpu";
pub const CPUIDLE_SYSFS_PER_CPU_DIR: &str = "cpuidle";
pub const CPUIDLE_SYSFS_STATE_PREFIX: &str = "state";

// ---------------------------------------------------------------------------
// Per-state attribute files
// ---------------------------------------------------------------------------

pub const CPUIDLE_ATTR_NAME: &str = "name";
pub const CPUIDLE_ATTR_DESC: &str = "desc";
pub const CPUIDLE_ATTR_LATENCY: &str = "latency";
pub const CPUIDLE_ATTR_RESIDENCY: &str = "residency";
pub const CPUIDLE_ATTR_POWER: &str = "power";
pub const CPUIDLE_ATTR_USAGE: &str = "usage";
pub const CPUIDLE_ATTR_TIME: &str = "time";
pub const CPUIDLE_ATTR_DISABLE: &str = "disable";
pub const CPUIDLE_ATTR_ABOVE: &str = "above";
pub const CPUIDLE_ATTR_BELOW: &str = "below";

// ---------------------------------------------------------------------------
// Top-level cpuidle attributes
// ---------------------------------------------------------------------------

pub const CPUIDLE_ATTR_CURRENT_DRIVER: &str = "current_driver";
pub const CPUIDLE_ATTR_CURRENT_GOVERNOR: &str = "current_governor";
pub const CPUIDLE_ATTR_AVAILABLE_GOVERNORS: &str = "available_governors";
pub const CPUIDLE_ATTR_CURRENT_GOVERNOR_RO: &str = "current_governor_ro";

// ---------------------------------------------------------------------------
// State flags (struct cpuidle_state::flags)
// ---------------------------------------------------------------------------

pub const CPUIDLE_FLAG_NONE: u32 = 0x0000;
pub const CPUIDLE_FLAG_POLLING: u32 = 0x0001;
pub const CPUIDLE_FLAG_COUPLED: u32 = 0x0002;
pub const CPUIDLE_FLAG_TIMER_STOP: u32 = 0x0004;
pub const CPUIDLE_FLAG_UNUSABLE: u32 = 0x0008;
pub const CPUIDLE_FLAG_OFF: u32 = 0x0010;
pub const CPUIDLE_FLAG_RCU_IDLE: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Maximum states per cpu (kernel limit)
// ---------------------------------------------------------------------------

pub const CPUIDLE_STATE_MAX: u32 = 10;
/// Sentinel name used by polling pseudo-state.
pub const CPUIDLE_POLL_STATE_NAME: &str = "POLL";

// ---------------------------------------------------------------------------
// Common governor names
// ---------------------------------------------------------------------------

pub const CPUIDLE_GOV_MENU: &str = "menu";
pub const CPUIDLE_GOV_LADDER: &str = "ladder";
pub const CPUIDLE_GOV_TEO: &str = "teo";
pub const CPUIDLE_GOV_HALTPOLL: &str = "haltpoll";

// ---------------------------------------------------------------------------
// Latency hint via /dev/cpu_dma_latency (pm_qos)
// ---------------------------------------------------------------------------

pub const CPU_DMA_LATENCY_DEV: &str = "/dev/cpu_dma_latency";
/// Value (in microseconds) that "pin to C0" — i.e., 0 → never go idle.
pub const CPU_DMA_LATENCY_NO_IDLE: u32 = 0;
pub const CPU_DMA_LATENCY_MAX: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_devices_system_cpu() {
        assert!(CPUIDLE_SYSFS_ROOT.starts_with("/sys/devices/system/cpu/"));
        assert!(CPUIDLE_SYSFS_CPU_PREFIX.starts_with("/sys/devices/system/cpu/"));
    }

    #[test]
    fn test_per_state_attrs_distinct() {
        let a = [
            CPUIDLE_ATTR_NAME,
            CPUIDLE_ATTR_DESC,
            CPUIDLE_ATTR_LATENCY,
            CPUIDLE_ATTR_RESIDENCY,
            CPUIDLE_ATTR_POWER,
            CPUIDLE_ATTR_USAGE,
            CPUIDLE_ATTR_TIME,
            CPUIDLE_ATTR_DISABLE,
            CPUIDLE_ATTR_ABOVE,
            CPUIDLE_ATTR_BELOW,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_top_level_attrs_distinct() {
        let a = [
            CPUIDLE_ATTR_CURRENT_DRIVER,
            CPUIDLE_ATTR_CURRENT_GOVERNOR,
            CPUIDLE_ATTR_AVAILABLE_GOVERNORS,
            CPUIDLE_ATTR_CURRENT_GOVERNOR_RO,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_state_flags_distinct_single_bit() {
        let f = [
            CPUIDLE_FLAG_POLLING,
            CPUIDLE_FLAG_COUPLED,
            CPUIDLE_FLAG_TIMER_STOP,
            CPUIDLE_FLAG_UNUSABLE,
            CPUIDLE_FLAG_OFF,
            CPUIDLE_FLAG_RCU_IDLE,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        assert_eq!(CPUIDLE_FLAG_NONE, 0);
    }

    #[test]
    fn test_state_max_and_poll() {
        assert_eq!(CPUIDLE_STATE_MAX, 10);
        assert_eq!(CPUIDLE_POLL_STATE_NAME, "POLL");
    }

    #[test]
    fn test_governor_names_distinct_lowercase() {
        let g = [
            CPUIDLE_GOV_MENU,
            CPUIDLE_GOV_LADDER,
            CPUIDLE_GOV_TEO,
            CPUIDLE_GOV_HALTPOLL,
        ];
        for (i, &x) in g.iter().enumerate() {
            for &y in &g[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase());
            }
        }
    }

    #[test]
    fn test_cpu_dma_latency_constants() {
        assert_eq!(CPU_DMA_LATENCY_DEV, "/dev/cpu_dma_latency");
        assert_eq!(CPU_DMA_LATENCY_NO_IDLE, 0);
        assert_eq!(CPU_DMA_LATENCY_MAX, i32::MAX);
    }
}
