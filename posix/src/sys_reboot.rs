//! `<sys/reboot.h>` — system reboot.
//!
//! Re-exports `reboot()` and reboot command constants from the
//! `process` module.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::process::reboot;
pub use crate::process::LINUX_REBOOT_MAGIC1;
pub use crate::process::LINUX_REBOOT_MAGIC2;
pub use crate::process::LINUX_REBOOT_CMD_RESTART;
pub use crate::process::LINUX_REBOOT_CMD_HALT;
pub use crate::process::LINUX_REBOOT_CMD_POWER_OFF;
pub use crate::process::LINUX_REBOOT_CMD_CAD_ON;
pub use crate::process::LINUX_REBOOT_CMD_CAD_OFF;

// ---------------------------------------------------------------------------
// BSD-style aliases
// ---------------------------------------------------------------------------

/// Restart the system (BSD alias).
pub const RB_AUTOBOOT: u32 = LINUX_REBOOT_CMD_RESTART;

/// Halt the system (BSD alias).
pub const RB_HALT_SYSTEM: u32 = LINUX_REBOOT_CMD_HALT;

/// Power off the system (BSD alias).
pub const RB_POWER_OFF: u32 = LINUX_REBOOT_CMD_POWER_OFF;

/// Enable Ctrl-Alt-Delete (BSD alias).
pub const RB_ENABLE_CAD: u32 = LINUX_REBOOT_CMD_CAD_ON;

/// Disable Ctrl-Alt-Delete (BSD alias).
pub const RB_DISABLE_CAD: u32 = LINUX_REBOOT_CMD_CAD_OFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reboot_magic() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
    }

    #[test]
    fn test_bsd_aliases() {
        assert_eq!(RB_AUTOBOOT, LINUX_REBOOT_CMD_RESTART);
        assert_eq!(RB_HALT_SYSTEM, LINUX_REBOOT_CMD_HALT);
        assert_eq!(RB_POWER_OFF, LINUX_REBOOT_CMD_POWER_OFF);
        assert_eq!(RB_ENABLE_CAD, LINUX_REBOOT_CMD_CAD_ON);
        assert_eq!(RB_DISABLE_CAD, LINUX_REBOOT_CMD_CAD_OFF);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF, LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_reboot_stub() {
        let ret = reboot(LINUX_REBOOT_CMD_RESTART as i32);
        assert_eq!(ret, -1);
    }
}
