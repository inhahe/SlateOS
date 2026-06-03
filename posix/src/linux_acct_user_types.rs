//! `<sys/acct.h>` — BSD-style process accounting.
//!
//! Linux writes one fixed-size `struct acct` record per terminating
//! process when accounting is enabled with `acct(2)`. The format is
//! ancient (BSD 4.3) and parsed by `accton(8)`, `sa(8)`, `lastcomm(1)`.

// ---------------------------------------------------------------------------
// Magic / version byte at start of each record
// ---------------------------------------------------------------------------

pub const ACCT_VERSION_V0: u8 = 0;
pub const ACCT_VERSION_V1: u8 = 1;
pub const ACCT_VERSION_V2: u8 = 2;
/// Linux's extended ("v3") record layout — current default.
pub const ACCT_VERSION_V3: u8 = 3;

// ---------------------------------------------------------------------------
// `ac_flag` bits — encoded process exit conditions
// ---------------------------------------------------------------------------

/// `fork()`ed but did not `exec()` before terminating.
pub const AFORK: u8 = 0x01;
/// Used super-user privileges during its lifetime.
pub const ASU: u8 = 0x02;
/// Memory was compactified before accounting (historical, unused).
pub const ACOMPAT: u8 = 0x04;
/// Process was traced (`PTRACE_TRACEME`).
pub const ACORE: u8 = 0x08;
/// Process dumped core.
pub const AXSIG: u8 = 0x10;

// ---------------------------------------------------------------------------
// Field widths (`struct acct_v3` in `<linux/acct.h>`)
// ---------------------------------------------------------------------------

pub const ACCT_COMM_LEN: usize = 16;

// ---------------------------------------------------------------------------
// Frequency conversion — `ac_etime`, `ac_utime`, `ac_stime` are stored
// as `comp_t` (8-bit base-8 exponent, 13-bit mantissa) in units of 1/AHZ s
// ---------------------------------------------------------------------------

/// Historical accounting tick frequency (BSD compatibility).
pub const AHZ: u32 = 100;

// ---------------------------------------------------------------------------
// `acct(2)` syscall number on x86_64
// ---------------------------------------------------------------------------

pub const NR_ACCT: u32 = 163;

// ---------------------------------------------------------------------------
// Default file path (when an admin runs `accton /var/log/acct/pacct`)
// ---------------------------------------------------------------------------

pub const DEFAULT_ACCT_FILE: &str = "/var/log/account/pacct";

// ---------------------------------------------------------------------------
// `/proc/sys/kernel/acct` thresholds — high/low/check (percent free)
// ---------------------------------------------------------------------------

pub const SYSCTL_KERNEL_ACCT: &str = "/proc/sys/kernel/acct";
pub const ACCT_HIGH_WATER_DEFAULT: u32 = 4;
pub const ACCT_LOW_WATER_DEFAULT: u32 = 2;
pub const ACCT_CHECK_INTERVAL_DEFAULT_S: u32 = 30;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_dense_0_to_3() {
        let v = [ACCT_VERSION_V0, ACCT_VERSION_V1, ACCT_VERSION_V2, ACCT_VERSION_V3];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_flag_bits_single() {
        let f = [AFORK, ASU, ACOMPAT, ACORE, AXSIG];
        let mut or = 0u8;
        for v in f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // All five flags fit in the low 5 bits.
        assert_eq!(or, 0x1F);
    }

    #[test]
    fn test_comm_len_is_task_comm_len() {
        // Matches TASK_COMM_LEN — 16 is the kernel-wide command-name cap.
        assert_eq!(ACCT_COMM_LEN, 16);
    }

    #[test]
    fn test_ahz_is_classic_100() {
        assert_eq!(AHZ, 100);
    }

    #[test]
    fn test_nr_acct_is_163() {
        // x86_64 syscall number 163 (see arch/x86/entry/syscalls/syscall_64.tbl).
        assert_eq!(NR_ACCT, 163);
    }

    #[test]
    fn test_default_path_under_var_log() {
        assert!(DEFAULT_ACCT_FILE.starts_with("/var/log/"));
        assert!(DEFAULT_ACCT_FILE.ends_with("/pacct"));
    }

    #[test]
    fn test_water_marks_ordered() {
        // High > low > 0; both are percentages of free disk space.
        assert!(ACCT_LOW_WATER_DEFAULT > 0);
        assert!(ACCT_HIGH_WATER_DEFAULT > ACCT_LOW_WATER_DEFAULT);
        assert_eq!(ACCT_HIGH_WATER_DEFAULT, 4);
        assert_eq!(ACCT_LOW_WATER_DEFAULT, 2);
        assert_eq!(ACCT_CHECK_INTERVAL_DEFAULT_S, 30);
        assert_eq!(SYSCTL_KERNEL_ACCT, "/proc/sys/kernel/acct");
    }
}
