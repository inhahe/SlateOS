//! ACPI power-management — D-states, S-states, C-states, P-states.
//!
//! These are the four families of ACPI power tiers exposed to Linux
//! via sysfs and `/proc/acpi/`. CPU idle/freq governors and userspace
//! tools (`powertop`, `cpupower`, `tlp`) read and act on them.

// ---------------------------------------------------------------------------
// Device power states (`_PR0`/`_PSx` levels)
// ---------------------------------------------------------------------------

pub const ACPI_D_STATE_D0: u8 = 0; // fully on
pub const ACPI_D_STATE_D1: u8 = 1;
pub const ACPI_D_STATE_D2: u8 = 2;
pub const ACPI_D_STATE_D3_HOT: u8 = 3; // memory preserved
pub const ACPI_D_STATE_D3_COLD: u8 = 4; // power removed
pub const ACPI_D_STATE_MAX: u8 = ACPI_D_STATE_D3_COLD;

// ---------------------------------------------------------------------------
// System sleep states
// ---------------------------------------------------------------------------

pub const ACPI_STATE_S0: u8 = 0; // working
pub const ACPI_STATE_S1: u8 = 1; // power on, CPU stopped
pub const ACPI_STATE_S2: u8 = 2; // CPU off, cache lost
pub const ACPI_STATE_S3: u8 = 3; // suspend-to-RAM (STR)
pub const ACPI_STATE_S4: u8 = 4; // hibernate
pub const ACPI_STATE_S5: u8 = 5; // soft off
pub const ACPI_S_STATE_MAX: u8 = ACPI_STATE_S5;

// ---------------------------------------------------------------------------
// Processor C-states (`_CST`)
// ---------------------------------------------------------------------------

pub const ACPI_C_STATE_C0: u8 = 0; // running
pub const ACPI_C_STATE_C1: u8 = 1; // halt
pub const ACPI_C_STATE_C2: u8 = 2; // stop clock
pub const ACPI_C_STATE_C3: u8 = 3; // sleep
pub const ACPI_C_STATE_C4: u8 = 4;
pub const ACPI_C_STATE_C5: u8 = 5;
pub const ACPI_C_STATE_C6: u8 = 6;
pub const ACPI_C_STATE_C7: u8 = 7;
pub const ACPI_C_STATE_C8: u8 = 8;
pub const ACPI_C_STATE_C9: u8 = 9;
pub const ACPI_C_STATE_C10: u8 = 10;

// ---------------------------------------------------------------------------
// Battery-status flags
// ---------------------------------------------------------------------------

pub const ACPI_BATTERY_STATE_DISCHARGING: u32 = 0x01;
pub const ACPI_BATTERY_STATE_CHARGING: u32 = 0x02;
pub const ACPI_BATTERY_STATE_CRITICAL: u32 = 0x04;
pub const ACPI_BATTERY_STATE_CHARGE_LIMITING: u32 = 0x08;

// ---------------------------------------------------------------------------
// Sysfs paths used by powertop / acpid
// ---------------------------------------------------------------------------

pub const SYS_POWER_STATE: &str = "/sys/power/state";
pub const SYS_POWER_DISK: &str = "/sys/power/disk";
pub const SYS_POWER_MEM_SLEEP: &str = "/sys/power/mem_sleep";
pub const SYS_CLASS_POWER_SUPPLY: &str = "/sys/class/power_supply";
pub const SYS_DEVICES_SYSTEM_CPU: &str = "/sys/devices/system/cpu";

// ---------------------------------------------------------------------------
// Strings written to /sys/power/state
// ---------------------------------------------------------------------------

pub const ACPI_SLEEP_STR_FREEZE: &str = "freeze"; // s2idle
pub const ACPI_SLEEP_STR_STANDBY: &str = "standby"; // S1
pub const ACPI_SLEEP_STR_MEM: &str = "mem"; // S3
pub const ACPI_SLEEP_STR_DISK: &str = "disk"; // S4

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d_states_dense_with_d3_split() {
        // D0..D3hot is dense 0..3; D3cold is the fifth value 4.
        assert_eq!(ACPI_D_STATE_D0, 0);
        assert_eq!(ACPI_D_STATE_D1, 1);
        assert_eq!(ACPI_D_STATE_D2, 2);
        assert_eq!(ACPI_D_STATE_D3_HOT, 3);
        assert_eq!(ACPI_D_STATE_D3_COLD, 4);
        assert_eq!(ACPI_D_STATE_MAX, 4);
    }

    #[test]
    fn test_s_states_dense_0_to_5() {
        let s = [
            ACPI_STATE_S0,
            ACPI_STATE_S1,
            ACPI_STATE_S2,
            ACPI_STATE_S3,
            ACPI_STATE_S4,
            ACPI_STATE_S5,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(ACPI_S_STATE_MAX, 5);
    }

    #[test]
    fn test_c_states_dense_0_to_10() {
        let c = [
            ACPI_C_STATE_C0,
            ACPI_C_STATE_C1,
            ACPI_C_STATE_C2,
            ACPI_C_STATE_C3,
            ACPI_C_STATE_C4,
            ACPI_C_STATE_C5,
            ACPI_C_STATE_C6,
            ACPI_C_STATE_C7,
            ACPI_C_STATE_C8,
            ACPI_C_STATE_C9,
            ACPI_C_STATE_C10,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_battery_state_bits_disjoint() {
        let b = [
            ACPI_BATTERY_STATE_DISCHARGING,
            ACPI_BATTERY_STATE_CHARGING,
            ACPI_BATTERY_STATE_CRITICAL,
            ACPI_BATTERY_STATE_CHARGE_LIMITING,
        ];
        let mut or = 0u32;
        for v in b {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x0F);
        // Discharging and charging are mutually exclusive but both can
        // co-occur with critical (firmware quirks); the bitmask still
        // has each flag as a single bit.
    }

    #[test]
    fn test_sysfs_paths_in_sys_namespace() {
        for p in [
            SYS_POWER_STATE,
            SYS_POWER_DISK,
            SYS_POWER_MEM_SLEEP,
            SYS_CLASS_POWER_SUPPLY,
            SYS_DEVICES_SYSTEM_CPU,
        ] {
            assert!(p.starts_with("/sys/"));
        }
    }

    #[test]
    fn test_sleep_strings_short_and_distinct() {
        let s = [
            ACPI_SLEEP_STR_FREEZE,
            ACPI_SLEEP_STR_STANDBY,
            ACPI_SLEEP_STR_MEM,
            ACPI_SLEEP_STR_DISK,
        ];
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
            assert!(s[i].len() <= 7);
        }
        // "mem" is the canonical S3 trigger string.
        assert_eq!(ACPI_SLEEP_STR_MEM, "mem");
    }
}
