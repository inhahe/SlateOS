//! `<linux/reboot.h>` — System reboot command constants.
//!
//! The `reboot()` syscall performs system shutdown, restart, or
//! halt operations. It requires passing two magic numbers for
//! safety, plus a command specifying the desired action.

// ---------------------------------------------------------------------------
// Reboot magic numbers (safety check)
// ---------------------------------------------------------------------------

/// First magic number (Linus's birthday: Dec 30).
pub const LINUX_REBOOT_MAGIC1: u32 = 0xFEE1_DEAD;
/// Second magic number (first option).
pub const LINUX_REBOOT_MAGIC2: u32 = 672274793;
/// Second magic number (alternate 1).
pub const LINUX_REBOOT_MAGIC2A: u32 = 85072278;
/// Second magic number (alternate 2).
pub const LINUX_REBOOT_MAGIC2B: u32 = 369367448;
/// Second magic number (alternate 3).
pub const LINUX_REBOOT_MAGIC2C: u32 = 537993216;

// ---------------------------------------------------------------------------
// Reboot commands
// ---------------------------------------------------------------------------

/// Restart the system (normal reboot).
pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x0123_4567;
/// Halt the system (power stays on).
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xCDEF_0123;
/// Power off the system.
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321_FEDC;
/// Restart with command string.
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xA1B2_C3D4;
/// Suspend to disk (hibernate).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xD000_FCE2;
/// Enable Ctrl-Alt-Del warm reboot.
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89AB_CDEF;
/// Disable Ctrl-Alt-Del warm reboot.
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x0000_0000;
/// Load kexec kernel.
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x4558_4543;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic1() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xFEE1_DEAD);
    }

    #[test]
    fn test_magic2_values_distinct() {
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
    fn test_commands_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF, LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_SW_SUSPEND, LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF, LINUX_REBOOT_CMD_KEXEC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_cad_off_is_zero() {
        assert_eq!(LINUX_REBOOT_CMD_CAD_OFF, 0);
    }

    #[test]
    fn test_kexec_command() {
        // "EXEC" in ASCII-ish
        assert_eq!(LINUX_REBOOT_CMD_KEXEC, 0x4558_4543);
    }
}
