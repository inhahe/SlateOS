//! `<linux/vt.h>` — Virtual terminal (VT) constants.
//!
//! Linux virtual terminals (VTs) provide multiple independent text
//! consoles on a single physical display. Users switch between them
//! with Ctrl+Alt+Fn keys. VTs support text mode (VGA text), framebuffer
//! mode, and KMS mode. The kernel provides up to 63 VTs (typically
//! 6 are configured by default).

// ---------------------------------------------------------------------------
// VT ioctl commands
// ---------------------------------------------------------------------------

/// Open a new VT.
pub const VT_OPENQRY: u32 = 0x5600;
/// Get VT mode (text/graphics).
pub const VT_GETMODE: u32 = 0x5601;
/// Set VT mode.
pub const VT_SETMODE: u32 = 0x5602;
/// Get VT state (active VT number).
pub const VT_GETSTATE: u32 = 0x5603;
/// Activate (switch to) a VT.
pub const VT_ACTIVATE: u32 = 0x5606;
/// Wait for VT activation to complete.
pub const VT_WAITACTIVE: u32 = 0x5607;
/// Disallocate (free) a VT.
pub const VT_DISALLOCATE: u32 = 0x5608;
/// Resize VT.
pub const VT_RESIZE: u32 = 0x5609;
/// Lock VT switching.
pub const VT_LOCKSWITCH: u32 = 0x560B;
/// Unlock VT switching.
pub const VT_UNLOCKSWITCH: u32 = 0x560C;

// ---------------------------------------------------------------------------
// VT modes
// ---------------------------------------------------------------------------

/// Auto mode (kernel handles VT switching).
pub const VT_AUTO: u32 = 0;
/// Process mode (process controls VT switching).
pub const VT_PROCESS: u32 = 1;
/// Acknowledge mode.
pub const VT_ACKACQ: u32 = 2;

// ---------------------------------------------------------------------------
// VT limits
// ---------------------------------------------------------------------------

/// Maximum number of virtual terminals.
pub const MAX_NR_CONSOLES: u32 = 63;
/// Minimum VT number (VT 0 is special).
pub const MIN_NR_CONSOLES: u32 = 1;

// ---------------------------------------------------------------------------
// KDSETMODE modes (text vs graphics)
// ---------------------------------------------------------------------------

/// Text mode.
pub const KD_TEXT: u32 = 0x00;
/// Graphics mode (disable kernel text output).
pub const KD_GRAPHICS: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            VT_OPENQRY, VT_GETMODE, VT_SETMODE, VT_GETSTATE,
            VT_ACTIVATE, VT_WAITACTIVE, VT_DISALLOCATE,
            VT_RESIZE, VT_LOCKSWITCH, VT_UNLOCKSWITCH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [VT_AUTO, VT_PROCESS, VT_ACKACQ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_kd_modes_distinct() {
        assert_ne!(KD_TEXT, KD_GRAPHICS);
    }

    #[test]
    fn test_console_limits() {
        assert!(MIN_NR_CONSOLES < MAX_NR_CONSOLES);
    }
}
