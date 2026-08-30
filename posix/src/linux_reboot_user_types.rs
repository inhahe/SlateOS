//! `<sys/reboot.h>` — `reboot(2)` magic numbers and commands.
//!
//! The `reboot` syscall requires two magic numbers as the first two
//! arguments to guard against accidental invocation. The command in
//! the third argument selects power-off vs reboot vs halt vs kexec.
//! systemd, sysvinit, and busybox all dispatch through these.

// ---------------------------------------------------------------------------
// Magic numbers (`LINUX_REBOOT_MAGIC*`)
// ---------------------------------------------------------------------------

/// First magic — Torvalds' birthday in BCD.
pub const LINUX_REBOOT_MAGIC1: u32 = 0xfee1_dead;

/// Second magic candidates — any of these is accepted as a second
/// magic number.
pub const LINUX_REBOOT_MAGIC2: u32 = 672_274_793;
pub const LINUX_REBOOT_MAGIC2A: u32 = 85_072_278;
pub const LINUX_REBOOT_MAGIC2B: u32 = 369_367_448;
pub const LINUX_REBOOT_MAGIC2C: u32 = 537_993_216;

// ---------------------------------------------------------------------------
// Command codes (`LINUX_REBOOT_CMD_*`)
// ---------------------------------------------------------------------------

pub const LINUX_REBOOT_CMD_RESTART: u32 = 0x0123_4567;
pub const LINUX_REBOOT_CMD_HALT: u32 = 0xCDEF_0123;
pub const LINUX_REBOOT_CMD_CAD_ON: u32 = 0x89AB_CDEF;
pub const LINUX_REBOOT_CMD_CAD_OFF: u32 = 0x0000_0000;
pub const LINUX_REBOOT_CMD_POWER_OFF: u32 = 0x4321_FEDC;
pub const LINUX_REBOOT_CMD_RESTART2: u32 = 0xA1B2_C3D4;
pub const LINUX_REBOOT_CMD_SW_SUSPEND: u32 = 0xD000_FCE2;
pub const LINUX_REBOOT_CMD_KEXEC: u32 = 0x4584_2742;

// ---------------------------------------------------------------------------
// `RESTART2` argument size
// ---------------------------------------------------------------------------

/// Maximum length of the bootloader command string in `RESTART2`.
pub const RESTART2_CMD_MAX: usize = 256;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_REBOOT: u32 = 169;
pub const NR_KEXEC_LOAD: u32 = 246;
pub const NR_KEXEC_FILE_LOAD: u32 = 320;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic1_is_feeldead() {
        // The famous magic — `fee1dead`.
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
    }

    #[test]
    fn test_magic2_values_distinct() {
        let m = [
            LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_MAGIC2A,
            LINUX_REBOOT_MAGIC2B,
            LINUX_REBOOT_MAGIC2C,
        ];
        for a in 0..m.len() {
            for b in (a + 1)..m.len() {
                assert_ne!(m[a], m[b]);
            }
        }
        // MAGIC2 is the decimal form of "672274793" (Linus' wife's birthday).
        assert_eq!(LINUX_REBOOT_MAGIC2, 672_274_793);
    }

    #[test]
    fn test_cmd_codes_distinct() {
        let c = [
            LINUX_REBOOT_CMD_RESTART,
            LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF,
            LINUX_REBOOT_CMD_POWER_OFF,
            LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_SW_SUSPEND,
            LINUX_REBOOT_CMD_KEXEC,
        ];
        for a in 0..c.len() {
            for b in (a + 1)..c.len() {
                assert_ne!(c[a], c[b]);
            }
        }
        // CAD_OFF is the only one that is plain zero (so that a zeroed
        // argument disables Ctrl-Alt-Del by default).
        assert_eq!(LINUX_REBOOT_CMD_CAD_OFF, 0);
    }

    #[test]
    fn test_restart2_buffer_cap() {
        // The bootloader command buffer is 256 bytes.
        assert_eq!(RESTART2_CMD_MAX, 256);
    }

    #[test]
    fn test_syscall_numbers_distinct() {
        assert_ne!(NR_REBOOT, NR_KEXEC_LOAD);
        assert_ne!(NR_KEXEC_LOAD, NR_KEXEC_FILE_LOAD);
        // reboot(2) sits at 169 on x86_64.
        assert_eq!(NR_REBOOT, 169);
    }
}
