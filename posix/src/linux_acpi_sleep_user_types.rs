//! ACPI sleep — `/sys/power/state`, `s2idle`, hibernate modes.
//!
//! Linux exposes ACPI S-state and s2idle (modern standby) selection
//! via sysfs. `systemd-sleep`, `pm-utils`, and `swsusp`/`uswsusp`
//! drive these.

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

pub const SYS_POWER_STATE: &str = "/sys/power/state";
pub const SYS_POWER_MEM_SLEEP: &str = "/sys/power/mem_sleep";
pub const SYS_POWER_DISK: &str = "/sys/power/disk";
pub const SYS_POWER_RESUME: &str = "/sys/power/resume";
pub const SYS_POWER_RESUME_OFFSET: &str = "/sys/power/resume_offset";
pub const SYS_POWER_PM_TRACE: &str = "/sys/power/pm_trace";
pub const SYS_POWER_IMAGE_SIZE: &str = "/sys/power/image_size";
pub const SYS_POWER_RESERVED_SIZE: &str = "/sys/power/reserved_size";
pub const SYS_POWER_WAKEUP_COUNT: &str = "/sys/power/wakeup_count";

// ---------------------------------------------------------------------------
// `/sys/power/state` accepted values
// ---------------------------------------------------------------------------

pub const PM_STATE_FREEZE: &str = "freeze"; // s2idle
pub const PM_STATE_STANDBY: &str = "standby"; // S1
pub const PM_STATE_MEM: &str = "mem"; // S3 (or s2idle if mem_sleep=s2idle)
pub const PM_STATE_DISK: &str = "disk"; // S4 (hibernate)

// ---------------------------------------------------------------------------
// `/sys/power/mem_sleep` modifier values (chooses what `mem` means)
// ---------------------------------------------------------------------------

pub const PM_MEM_SLEEP_S2IDLE: &str = "s2idle";
pub const PM_MEM_SLEEP_SHALLOW: &str = "shallow"; // S1
pub const PM_MEM_SLEEP_DEEP: &str = "deep"; // S3

// ---------------------------------------------------------------------------
// `/sys/power/disk` accepted values (hibernate mode)
// ---------------------------------------------------------------------------

pub const PM_DISK_PLATFORM: &str = "platform"; // call into firmware after image
pub const PM_DISK_SHUTDOWN: &str = "shutdown"; // power off after image
pub const PM_DISK_REBOOT: &str = "reboot";
pub const PM_DISK_SUSPEND: &str = "suspend"; // do an S3 instead
pub const PM_DISK_TEST_RESUME: &str = "test_resume";
pub const PM_DISK_TEST: &str = "test";

// ---------------------------------------------------------------------------
// Sleep-state enum (mirror of kernel `suspend_state_t`)
// ---------------------------------------------------------------------------

pub const PM_SUSPEND_ON: u32 = 0; // working
pub const PM_SUSPEND_TO_IDLE: u32 = 1; // s2idle
pub const PM_SUSPEND_STANDBY: u32 = 2;
pub const PM_SUSPEND_MEM: u32 = 3;
pub const PM_SUSPEND_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Default hibernate image-size fraction (kernel boots with 2/5)
// ---------------------------------------------------------------------------

pub const PM_IMAGE_SIZE_DEFAULT_NUM: u64 = 2;
pub const PM_IMAGE_SIZE_DEFAULT_DEN: u64 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_sys_power() {
        for p in [
            SYS_POWER_STATE,
            SYS_POWER_MEM_SLEEP,
            SYS_POWER_DISK,
            SYS_POWER_RESUME,
            SYS_POWER_RESUME_OFFSET,
            SYS_POWER_PM_TRACE,
            SYS_POWER_IMAGE_SIZE,
            SYS_POWER_RESERVED_SIZE,
            SYS_POWER_WAKEUP_COUNT,
        ] {
            assert!(p.starts_with("/sys/power/"));
        }
    }

    #[test]
    fn test_state_strings_distinct_short() {
        let s = [PM_STATE_FREEZE, PM_STATE_STANDBY, PM_STATE_MEM, PM_STATE_DISK];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
            assert!(s[i].len() <= 7);
        }
    }

    #[test]
    fn test_mem_sleep_modifier_strings() {
        let m = [PM_MEM_SLEEP_S2IDLE, PM_MEM_SLEEP_SHALLOW, PM_MEM_SLEEP_DEEP];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // "deep" is what most desktops use today.
        assert_eq!(PM_MEM_SLEEP_DEEP, "deep");
    }

    #[test]
    fn test_disk_mode_strings_distinct() {
        let d = [
            PM_DISK_PLATFORM,
            PM_DISK_SHUTDOWN,
            PM_DISK_REBOOT,
            PM_DISK_SUSPEND,
            PM_DISK_TEST_RESUME,
            PM_DISK_TEST,
        ];
        for i in 0..d.len() {
            for j in (i + 1)..d.len() {
                assert_ne!(d[i], d[j]);
            }
        }
    }

    #[test]
    fn test_suspend_state_enum_dense_0_to_3() {
        let s = [
            PM_SUSPEND_ON,
            PM_SUSPEND_TO_IDLE,
            PM_SUSPEND_STANDBY,
            PM_SUSPEND_MEM,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(PM_SUSPEND_MAX as usize, s.len());
    }

    #[test]
    fn test_image_size_fraction_is_2_5() {
        // 2/5 = 40 % of RAM by default for the hibernate image.
        assert_eq!(PM_IMAGE_SIZE_DEFAULT_NUM, 2);
        assert_eq!(PM_IMAGE_SIZE_DEFAULT_DEN, 5);
        assert!(PM_IMAGE_SIZE_DEFAULT_NUM < PM_IMAGE_SIZE_DEFAULT_DEN);
    }
}
