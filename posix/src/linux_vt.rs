//! `<linux/vt.h>` — virtual terminal (VT) control.
//!
//! Provides ioctl constants and structures for managing Linux
//! virtual terminals (VT switching, VT modes, etc.).

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Open a new virtual terminal.
pub const VT_OPENQRY: u64 = 0x5600;
/// Get VT mode info.
pub const VT_GETMODE: u64 = 0x5601;
/// Set VT mode info.
pub const VT_SETMODE: u64 = 0x5602;
/// Get VT state.
pub const VT_GETSTATE: u64 = 0x5603;
/// Send release signal.
pub const VT_RELDISP: u64 = 0x5605;
/// Activate a VT.
pub const VT_ACTIVATE: u64 = 0x5606;
/// Wait for VT activation.
pub const VT_WAITACTIVE: u64 = 0x5607;
/// Disallocate a VT.
pub const VT_DISALLOCATE: u64 = 0x5608;
/// Resize VT.
pub const VT_RESIZE: u64 = 0x5609;
/// Resize VT (with extended info).
pub const VT_RESIZEX: u64 = 0x560A;
/// Lock VT switching.
pub const VT_LOCKSWITCH: u64 = 0x560B;
/// Unlock VT switching.
pub const VT_UNLOCKSWITCH: u64 = 0x560C;

// ---------------------------------------------------------------------------
// VT mode settings
// ---------------------------------------------------------------------------

/// Auto VT switching (kernel handles it).
pub const VT_AUTO: i32 = 0x00;
/// Process VT switching (application handles it).
pub const VT_PROCESS: i32 = 0x01;
/// Acknowledge VT switch.
pub const VT_ACKACQ: i32 = 0x02;

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// VT mode parameters.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VtMode {
    /// Mode (VT_AUTO or VT_PROCESS).
    pub mode: i8,
    /// Wait for completion (unused).
    pub waitv: i8,
    /// Signal for release.
    pub relsig: i16,
    /// Signal for acquire.
    pub acqsig: i16,
    /// Filler (unused).
    pub frsig: i16,
}

/// VT state information.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VtStat {
    /// Active VT number.
    pub v_active: u16,
    /// Signal to send on release (unused).
    pub v_signal: u16,
    /// Bitmask of active VTs.
    pub v_state: u16,
}

/// VT size information.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VtSizes {
    /// Number of rows.
    pub v_rows: u16,
    /// Number of columns.
    pub v_cols: u16,
    /// Scroll size.
    pub v_scrollsize: u16,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vt_mode_struct_size() {
        assert_eq!(core::mem::size_of::<VtMode>(), 8);
    }

    #[test]
    fn test_vt_stat_struct_size() {
        assert_eq!(core::mem::size_of::<VtStat>(), 6);
    }

    #[test]
    fn test_vt_sizes_struct_size() {
        assert_eq!(core::mem::size_of::<VtSizes>(), 6);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            VT_OPENQRY,
            VT_GETMODE,
            VT_SETMODE,
            VT_GETSTATE,
            VT_RELDISP,
            VT_ACTIVATE,
            VT_WAITACTIVE,
            VT_DISALLOCATE,
            VT_RESIZE,
            VT_RESIZEX,
            VT_LOCKSWITCH,
            VT_UNLOCKSWITCH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_vt_modes() {
        assert_eq!(VT_AUTO, 0);
        assert_eq!(VT_PROCESS, 1);
        assert_eq!(VT_ACKACQ, 2);
    }

    #[test]
    fn test_vt_mode_process() {
        let mode = VtMode {
            mode: VT_PROCESS as i8,
            waitv: 0,
            relsig: 10, // SIGUSR1
            acqsig: 12, // SIGUSR2
            frsig: 0,
        };
        assert_eq!(mode.mode, 1);
        assert_ne!(mode.relsig, mode.acqsig);
    }
}
