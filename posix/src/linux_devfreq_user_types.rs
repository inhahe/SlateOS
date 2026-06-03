//! `<linux/devfreq.h>` — userspace-visible devfreq governor constants.
//!
//! devfreq is the Linux DVFS framework for non-CPU devices (GPU, DDR
//! controller, ISP). Userspace tunes it via sysfs at
//! `/sys/class/devfreq/<dev>/`. These constants name the in-tree
//! governors, the available transition flags, and the limits exposed
//! by the driver.

// ---------------------------------------------------------------------------
// Devfreq governor names (in-tree, as exposed via "available_governors")
// ---------------------------------------------------------------------------

/// Tracks `userspace`-set frequency in `min_freq`/`max_freq`.
pub const DEVFREQ_GOV_USERSPACE: &str = "userspace";
/// Pin the device to the maximum frequency the driver supports.
pub const DEVFREQ_GOV_PERFORMANCE: &str = "performance";
/// Pin the device to the minimum frequency the driver supports.
pub const DEVFREQ_GOV_POWERSAVE: &str = "powersave";
/// On-demand-style governor (default for most platform GPU drivers).
pub const DEVFREQ_GOV_SIMPLE_ONDEMAND: &str = "simple_ondemand";
/// Passive governor — frequency follows another devfreq device.
pub const DEVFREQ_GOV_PASSIVE: &str = "passive";

// ---------------------------------------------------------------------------
// "trans_stat" header flags / transition reasons
// ---------------------------------------------------------------------------

/// Frequency change requested by user space.
pub const DEVFREQ_PRECHANGE: u32 = 0;
/// Frequency change finished and committed.
pub const DEVFREQ_POSTCHANGE: u32 = 1;

// ---------------------------------------------------------------------------
// Polling intervals (milliseconds)
// ---------------------------------------------------------------------------

/// Default polling interval used by simple_ondemand if none set.
pub const DEVFREQ_DEFAULT_POLL_MS: u32 = 100;
/// Minimum sane polling interval.
pub const DEVFREQ_MIN_POLL_MS: u32 = 10;
/// Maximum polling interval the framework accepts.
pub const DEVFREQ_MAX_POLL_MS: u32 = 60_000;

// ---------------------------------------------------------------------------
// Governor name buffer length (writes to sysfs "governor" are bounded)
// ---------------------------------------------------------------------------

/// Maximum bytes in a governor name string (matches `DEVFREQ_NAME_LEN`).
pub const DEVFREQ_NAME_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_names_distinct() {
        let g = [
            DEVFREQ_GOV_USERSPACE,
            DEVFREQ_GOV_PERFORMANCE,
            DEVFREQ_GOV_POWERSAVE,
            DEVFREQ_GOV_SIMPLE_ONDEMAND,
            DEVFREQ_GOV_PASSIVE,
        ];
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                assert_ne!(g[i], g[j]);
            }
            // Sysfs governor strings must fit in the kernel's fixed buffer.
            assert!((g[i].len() as u32) < DEVFREQ_NAME_LEN);
            for b in g[i].bytes() {
                assert!(b.is_ascii_lowercase() || b == b'_');
            }
        }
    }

    #[test]
    fn test_transition_phases_distinct() {
        assert_ne!(DEVFREQ_PRECHANGE, DEVFREQ_POSTCHANGE);
    }

    #[test]
    fn test_poll_bounds_ordered() {
        assert!(DEVFREQ_MIN_POLL_MS < DEVFREQ_DEFAULT_POLL_MS);
        assert!(DEVFREQ_DEFAULT_POLL_MS < DEVFREQ_MAX_POLL_MS);
    }
}
