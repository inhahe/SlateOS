//! `<linux/vt.h>` — Additional virtual terminal constants.
//!
//! Supplementary VT constants covering ioctl commands,
//! VT modes, and VT states.

// ---------------------------------------------------------------------------
// VT ioctl commands
// ---------------------------------------------------------------------------

/// Open a VT.
pub const VT_OPENQRY: u32 = 0x5600;
/// Get VT mode.
pub const VT_GETMODE: u32 = 0x5601;
/// Set VT mode.
pub const VT_SETMODE: u32 = 0x5602;
/// Get VT state.
pub const VT_GETSTATE: u32 = 0x5603;
/// Send signal.
pub const VT_SENDSIG: u32 = 0x5604;
/// Release display.
pub const VT_RELDISP: u32 = 0x5605;
/// Activate VT.
pub const VT_ACTIVATE: u32 = 0x5606;
/// Wait active.
pub const VT_WAITACTIVE: u32 = 0x5607;
/// Disallocate VT.
pub const VT_DISALLOCATE: u32 = 0x5608;
/// Resize VT.
pub const VT_RESIZE: u32 = 0x5609;
/// Resize extended.
pub const VT_RESIZEX: u32 = 0x560A;
/// Lock switch.
pub const VT_LOCKSWITCH: u32 = 0x560B;
/// Unlock switch.
pub const VT_UNLOCKSWITCH: u32 = 0x560C;
/// Get HiFontMask.
pub const VT_GETHIFONTMASK: u32 = 0x560D;
/// Wait for release.
pub const VT_WAITEVENT: u32 = 0x560E;
/// Set activates.
pub const VT_SETACTIVATE: u32 = 0x560F;

// ---------------------------------------------------------------------------
// VT modes
// ---------------------------------------------------------------------------

/// Auto mode.
pub const VT_AUTO: u32 = 0x00;
/// Process mode.
pub const VT_PROCESS: u32 = 0x01;
/// Ack-acquire mode.
pub const VT_ACKACQ: u32 = 0x02;

// ---------------------------------------------------------------------------
// VT event flags
// ---------------------------------------------------------------------------

/// VT opened event.
pub const VT_EVENT_SWITCH: u32 = 0x01;
/// Blank event.
pub const VT_EVENT_BLANK: u32 = 0x02;
/// Unblank event.
pub const VT_EVENT_UNBLANK: u32 = 0x04;
/// Resize event.
pub const VT_EVENT_RESIZE: u32 = 0x08;

// ---------------------------------------------------------------------------
// Maximum VT constants
// ---------------------------------------------------------------------------

/// Maximum number of consoles.
pub const MAX_NR_CONSOLES: u32 = 63;
/// Maximum number of user-defined keys.
pub const MAX_NR_USER_CONSOLES: u32 = 63;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            VT_OPENQRY,
            VT_GETMODE,
            VT_SETMODE,
            VT_GETSTATE,
            VT_SENDSIG,
            VT_RELDISP,
            VT_ACTIVATE,
            VT_WAITACTIVE,
            VT_DISALLOCATE,
            VT_RESIZE,
            VT_RESIZEX,
            VT_LOCKSWITCH,
            VT_UNLOCKSWITCH,
            VT_GETHIFONTMASK,
            VT_WAITEVENT,
            VT_SETACTIVATE,
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
    fn test_event_flags_power_of_two() {
        let events = [
            VT_EVENT_SWITCH,
            VT_EVENT_BLANK,
            VT_EVENT_UNBLANK,
            VT_EVENT_RESIZE,
        ];
        for e in &events {
            assert!(e.is_power_of_two(), "0x{:02x} not power of two", e);
        }
    }

    #[test]
    fn test_event_flags_no_overlap() {
        let events = [
            VT_EVENT_SWITCH,
            VT_EVENT_BLANK,
            VT_EVENT_UNBLANK,
            VT_EVENT_RESIZE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_max_consoles() {
        assert_eq!(MAX_NR_CONSOLES, 63);
        assert_eq!(MAX_NR_USER_CONSOLES, 63);
    }
}
