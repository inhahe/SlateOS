//! `<linux/kdb.h>` / `kgdb` — kernel debugger user-facing constants.
//!
//! KGDB and KDB (the "built-in" debugger) communicate with userspace
//! via a small set of sysfs knobs and module parameters. The names
//! and values below match the kernel's `kernel/debug/` subsystem.

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

/// Trigger KGDB break by writing "g" here.
pub const KDB_SYSRQ_TRIGGER: &str = "/proc/sysrq-trigger";
/// Tunable to choose the serial console for KGDB.
pub const KDB_KGDBOC_PATH: &str = "/sys/module/kgdboc/parameters/kgdboc";
/// Tunable enabling early KGDB.
pub const KDB_KGDB_EARLY_PATH: &str = "/sys/module/kgdb/parameters/kgdb_early";

// ---------------------------------------------------------------------------
// KDB initialization status (`kdb_initial_cpu`)
// ---------------------------------------------------------------------------

/// KDB not yet initialized.
pub const KDB_INIT_EARLY: i32 = 0;
/// KDB initialized on the boot CPU.
pub const KDB_INIT_FULL: i32 = 1;

// ---------------------------------------------------------------------------
// KDB return codes (`enum kdb_state`)
// ---------------------------------------------------------------------------

pub const KDB_CMD_GO: i32 = -1001;
pub const KDB_CMD_CPU: i32 = -1002;
pub const KDB_CMD_SS: i32 = -1003;
pub const KDB_CMD_KGDB: i32 = -1005;

// ---------------------------------------------------------------------------
// Reason codes for entering KDB/KGDB
// ---------------------------------------------------------------------------

pub const KDB_REASON_OOPS: u32 = 1;
pub const KDB_REASON_FAULT: u32 = 2;
pub const KDB_REASON_BREAK: u32 = 3;
pub const KDB_REASON_DEBUG: u32 = 4;
pub const KDB_REASON_NMI: u32 = 5;
pub const KDB_REASON_SWITCH: u32 = 6;
pub const KDB_REASON_KEYBOARD: u32 = 7;
pub const KDB_REASON_ENTER: u32 = 8;
pub const KDB_REASON_ENTER_SLAVE: u32 = 9;
pub const KDB_REASON_SSTEP: u32 = 10;
pub const KDB_REASON_SYSTEM_NMI: u32 = 11;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum command line length passed to a kdb command.
pub const KDB_CMD_BUF_LEN: usize = 200;
/// Maximum length of a kdb command name.
pub const KDB_CMD_NAME_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_well_formed() {
        // sysrq trigger is in /proc, not /sys.
        assert!(KDB_SYSRQ_TRIGGER.starts_with("/proc/"));
        // module-parameter paths live under /sys/module.
        assert!(KDB_KGDBOC_PATH.starts_with("/sys/module/"));
        assert!(KDB_KGDB_EARLY_PATH.starts_with("/sys/module/"));
    }

    #[test]
    fn test_init_status_distinct() {
        assert_ne!(KDB_INIT_EARLY, KDB_INIT_FULL);
        assert_eq!(KDB_INIT_EARLY, 0);
        assert_eq!(KDB_INIT_FULL, 1);
    }

    #[test]
    fn test_cmd_codes_all_negative_distinct() {
        let c = [KDB_CMD_GO, KDB_CMD_CPU, KDB_CMD_SS, KDB_CMD_KGDB];
        for &x in &c {
            assert!(x < 0);
        }
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_reason_codes_dense_1_to_11() {
        let r = [
            KDB_REASON_OOPS,
            KDB_REASON_FAULT,
            KDB_REASON_BREAK,
            KDB_REASON_DEBUG,
            KDB_REASON_NMI,
            KDB_REASON_SWITCH,
            KDB_REASON_KEYBOARD,
            KDB_REASON_ENTER,
            KDB_REASON_ENTER_SLAVE,
            KDB_REASON_SSTEP,
            KDB_REASON_SYSTEM_NMI,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_size_constants() {
        assert_eq!(KDB_CMD_BUF_LEN, 200);
        assert_eq!(KDB_CMD_NAME_LEN, 32);
        // Command name fits inside the full command buffer.
        assert!(KDB_CMD_NAME_LEN < KDB_CMD_BUF_LEN);
    }
}
