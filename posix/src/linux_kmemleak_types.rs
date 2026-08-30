//! `<linux/kmemleak.h>` — Kernel memory-leak detector constants.
//!
//! Constants used to drive `kmemleak` from userspace via
//! `/sys/kernel/debug/kmemleak`. The kernel writes leak reports
//! there; userspace can configure scan behaviour by writing
//! single-word commands.

// ---------------------------------------------------------------------------
// Userspace write-commands accepted on /sys/kernel/debug/kmemleak
// ---------------------------------------------------------------------------

/// "off" — disable kmemleak entirely.
pub const KMEMLEAK_CMD_OFF: &str = "off";
/// "stack=on" — enable stack scanning.
pub const KMEMLEAK_CMD_STACK_ON: &str = "stack=on";
/// "stack=off" — disable stack scanning.
pub const KMEMLEAK_CMD_STACK_OFF: &str = "stack=off";
/// "scan=on" — enable periodic scanning.
pub const KMEMLEAK_CMD_SCAN_ON: &str = "scan=on";
/// "scan=off" — disable periodic scanning.
pub const KMEMLEAK_CMD_SCAN_OFF: &str = "scan=off";
/// "scan" — trigger one immediate scan.
pub const KMEMLEAK_CMD_SCAN_NOW: &str = "scan";
/// "scan=NNN" prefix — set periodic scan interval (seconds).
pub const KMEMLEAK_CMD_SCAN_INTERVAL_PREFIX: &str = "scan=";
/// "clear" — clear the list of reported leaks.
pub const KMEMLEAK_CMD_CLEAR: &str = "clear";
/// "dump=ADDR" prefix — dump the object containing the given address.
pub const KMEMLEAK_CMD_DUMP_PREFIX: &str = "dump=";

// ---------------------------------------------------------------------------
// Default scan / report tuning (matches kernel defaults)
// ---------------------------------------------------------------------------

/// Default scan interval (seconds) — once every 10 minutes.
pub const KMEMLEAK_DEFAULT_SCAN_INTERVAL: u32 = 600;
/// Maximum number of leaks reported in a single read.
pub const KMEMLEAK_MAX_REPORTED_LEAKS: u32 = 10000;

// ---------------------------------------------------------------------------
// Object-flag bits (kmemleak_object.flags)
// ---------------------------------------------------------------------------

/// Object is referenced.
pub const KMEMLEAK_OBJECT_ALLOCATED: u32 = 0x01;
/// Object has been reported as a leak.
pub const KMEMLEAK_OBJECT_REPORTED: u32 = 0x02;
/// Scan has not touched this object yet.
pub const KMEMLEAK_OBJECT_NOT_SCANNED: u32 = 0x04;
/// Object is on the gray list (referenced but not via root).
pub const KMEMLEAK_OBJECT_NO_SCAN: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            KMEMLEAK_CMD_OFF,
            KMEMLEAK_CMD_STACK_ON,
            KMEMLEAK_CMD_STACK_OFF,
            KMEMLEAK_CMD_SCAN_ON,
            KMEMLEAK_CMD_SCAN_OFF,
            KMEMLEAK_CMD_SCAN_NOW,
            KMEMLEAK_CMD_CLEAR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_scan_interval_prefix_matches() {
        assert!(KMEMLEAK_CMD_SCAN_INTERVAL_PREFIX.starts_with("scan"));
        assert!(KMEMLEAK_CMD_DUMP_PREFIX.starts_with("dump"));
        // A user-formed command "scan=600" must start with the prefix.
        let cmd = format!("{}{}", KMEMLEAK_CMD_SCAN_INTERVAL_PREFIX, 600);
        assert!(cmd.starts_with(KMEMLEAK_CMD_SCAN_INTERVAL_PREFIX));
    }

    #[test]
    fn test_default_scan_interval_sane() {
        assert!(KMEMLEAK_DEFAULT_SCAN_INTERVAL >= 60);
        assert!(KMEMLEAK_DEFAULT_SCAN_INTERVAL <= 86400);
    }

    #[test]
    fn test_object_flags_distinct_bits() {
        let flags = [
            KMEMLEAK_OBJECT_ALLOCATED,
            KMEMLEAK_OBJECT_REPORTED,
            KMEMLEAK_OBJECT_NOT_SCANNED,
            KMEMLEAK_OBJECT_NO_SCAN,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
