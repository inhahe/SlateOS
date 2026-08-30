//! `<linux/pwm.h>` — sysfs userspace interface for PWM channels.
//!
//! PWM (pulse-width modulation) chips are exposed at
//! `/sys/class/pwm/pwmchipN/` with `export`, `pwmN/period`,
//! `pwmN/duty_cycle`, `pwmN/polarity`, and `pwmN/enable` text files.
//! Userspace fan controllers, backlight tools, and DIY motor
//! drivers read/write these sysfs nodes.

// ---------------------------------------------------------------------------
// Sysfs root and file names
// ---------------------------------------------------------------------------

/// Root of the PWM sysfs class.
pub const PWM_SYSFS_ROOT: &str = "/sys/class/pwm";
/// Per-chip "export" file (write channel index to make pwmN appear).
pub const PWM_SYSFS_EXPORT: &str = "export";
/// Per-chip "unexport" file.
pub const PWM_SYSFS_UNEXPORT: &str = "unexport";
/// Per-chip "npwm" file — number of channels (ASCII decimal).
pub const PWM_SYSFS_NPWM: &str = "npwm";
/// Per-channel "period" file in nanoseconds.
pub const PWM_SYSFS_PERIOD: &str = "period";
/// Per-channel "duty_cycle" file in nanoseconds.
pub const PWM_SYSFS_DUTY_CYCLE: &str = "duty_cycle";
/// Per-channel "polarity" file ("normal" or "inversed").
pub const PWM_SYSFS_POLARITY: &str = "polarity";
/// Per-channel "enable" file ("0" or "1").
pub const PWM_SYSFS_ENABLE: &str = "enable";

// ---------------------------------------------------------------------------
// Polarity values
// ---------------------------------------------------------------------------

/// "normal" — high for duty_cycle, low for the rest of period.
pub const PWM_POLARITY_NORMAL: &str = "normal";
/// "inversed" — low for duty_cycle, high for the rest of period.
pub const PWM_POLARITY_INVERSED: &str = "inversed";

// ---------------------------------------------------------------------------
// Enable values
// ---------------------------------------------------------------------------

/// "1" — channel running.
pub const PWM_ENABLE_ON: &str = "1";
/// "0" — channel stopped (output undefined).
pub const PWM_ENABLE_OFF: &str = "0";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum period the kernel accepts (1 second in nanoseconds — kernel
/// max is 2^31 ns ≈ 2.15 s, but driver code rejects > 1 GHz / 1 Hz).
pub const PWM_MAX_PERIOD_NS: u64 = 1_000_000_000;
/// Minimum period (1 ns lower bound).
pub const PWM_MIN_PERIOD_NS: u64 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_root_under_sys_class() {
        // Must live under /sys/class so libgpiod-style discovery works.
        assert!(PWM_SYSFS_ROOT.starts_with("/sys/class/"));
        assert_eq!(PWM_SYSFS_ROOT, "/sys/class/pwm");
    }

    #[test]
    fn test_file_names_distinct() {
        let f = [
            PWM_SYSFS_EXPORT,
            PWM_SYSFS_UNEXPORT,
            PWM_SYSFS_NPWM,
            PWM_SYSFS_PERIOD,
            PWM_SYSFS_DUTY_CYCLE,
            PWM_SYSFS_POLARITY,
            PWM_SYSFS_ENABLE,
        ];
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
            // All sysfs names are lowercase ASCII without slashes —
            // they are appended to a directory path.
            assert!(!f[i].contains('/'));
            assert!(f[i].chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn test_polarity_values() {
        // Polarity is the only multi-word value — must match the
        // kernel's accept-list strings exactly.
        assert_eq!(PWM_POLARITY_NORMAL, "normal");
        assert_eq!(PWM_POLARITY_INVERSED, "inversed");
        assert_ne!(PWM_POLARITY_NORMAL, PWM_POLARITY_INVERSED);
    }

    #[test]
    fn test_enable_values() {
        assert_eq!(PWM_ENABLE_ON, "1");
        assert_eq!(PWM_ENABLE_OFF, "0");
    }

    #[test]
    fn test_period_bounds_sane() {
        assert!(PWM_MIN_PERIOD_NS < PWM_MAX_PERIOD_NS);
        assert_eq!(PWM_MAX_PERIOD_NS, 1_000_000_000);
        assert_eq!(PWM_MIN_PERIOD_NS, 1);
    }
}
