//! `<linux/reboot.h>` — system reboot commands (kernel view).
//!
//! Re-exports from `sys_reboot` and `process`.

// ---------------------------------------------------------------------------
// Re-exports: magic values
// ---------------------------------------------------------------------------

pub use crate::process::LINUX_REBOOT_MAGIC1;
pub use crate::process::LINUX_REBOOT_MAGIC2;
pub use crate::process::reboot;

// ---------------------------------------------------------------------------
// Re-exports: commands from sys_reboot
// ---------------------------------------------------------------------------

pub use crate::sys_reboot::LINUX_REBOOT_CMD_RESTART;
pub use crate::sys_reboot::LINUX_REBOOT_CMD_HALT;
pub use crate::sys_reboot::LINUX_REBOOT_CMD_POWER_OFF;
pub use crate::sys_reboot::LINUX_REBOOT_CMD_CAD_ON;
pub use crate::sys_reboot::LINUX_REBOOT_CMD_CAD_OFF;
pub use crate::sys_reboot::RB_AUTOBOOT;
pub use crate::sys_reboot::RB_HALT_SYSTEM;
pub use crate::sys_reboot::RB_POWER_OFF;

// ---------------------------------------------------------------------------
// Additional reboot commands
// ---------------------------------------------------------------------------

/// Restart with command string.
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xA1B2C3D4;
/// Suspend to disk (hibernate).
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xD000FCE2;
/// kexec: load a new kernel.
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x45584543;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_values() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART, LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF, LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_SW_SUSPEND, LINUX_REBOOT_CMD_KEXEC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rb_aliases() {
        assert_eq!(RB_AUTOBOOT, LINUX_REBOOT_CMD_RESTART);
        assert_eq!(RB_HALT_SYSTEM, LINUX_REBOOT_CMD_HALT);
        assert_eq!(RB_POWER_OFF, LINUX_REBOOT_CMD_POWER_OFF);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(LINUX_REBOOT_MAGIC1, crate::process::LINUX_REBOOT_MAGIC1);
        assert_eq!(LINUX_REBOOT_CMD_RESTART, crate::sys_reboot::LINUX_REBOOT_CMD_RESTART);
    }
}
