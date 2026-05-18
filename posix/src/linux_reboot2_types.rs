//! `<linux/reboot.h>` — Reboot command constants (extended).
//!
//! Extended reboot constants covering magic numbers, reboot
//! commands, and power management actions.

// ---------------------------------------------------------------------------
// Reboot magic numbers
// ---------------------------------------------------------------------------

/// First magic number (required).
pub const LINUX_REBOOT_MAGIC1: u32 = 0xFEE1DEAD;
/// Second magic number (one of these required).
pub const LINUX_REBOOT_MAGIC2: u32 = 672274793;
/// Alternative second magic number.
pub const LINUX_REBOOT_MAGIC2A: u32 = 85072278;
/// Alternative second magic number.
pub const LINUX_REBOOT_MAGIC2B: u32 = 369367448;
/// Alternative second magic number.
pub const LINUX_REBOOT_MAGIC2C: u32 = 537993216;

// ---------------------------------------------------------------------------
// Reboot commands
// ---------------------------------------------------------------------------

/// Restart the system.
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x01234567;
/// Halt the system (stop, don't power off).
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xCDEF0123;
/// Power off the system.
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321FEDC;
/// Restart with command string.
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xA1B2C3D4;
/// Ctrl-Alt-Del enable.
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89ABCDEF;
/// Ctrl-Alt-Del disable.
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x00000000;
/// Suspend (software suspend / hibernation).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xD000FCE2;
/// Kexec (load and execute a new kernel).
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x45584543;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic1() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xFEE1DEAD);
    }

    #[test]
    fn test_magic2_variants_distinct() {
        let magics = [
            LINUX_REBOOT_MAGIC2, LINUX_REBOOT_MAGIC2A,
            LINUX_REBOOT_MAGIC2B, LINUX_REBOOT_MAGIC2C,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF, LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_CAD_ON, LINUX_REBOOT_CMD_CAD_OFF,
            LINUX_REBOOT_CMD_SW_SUSPEND, LINUX_REBOOT_CMD_KEXEC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_restart_value() {
        assert_eq!(LINUX_REBOOT_CMD_RESTART, 0x01234567);
    }

    #[test]
    fn test_cad_off_is_zero() {
        assert_eq!(LINUX_REBOOT_CMD_CAD_OFF, 0);
    }
}
