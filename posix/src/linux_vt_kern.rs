//! `<linux/vt_kern.h>` — Virtual terminal kernel-side constants.
//!
//! Extends the VT ioctl constants from `linux_vt` with kernel-internal
//! virtual terminal state definitions used by the console subsystem.

pub use crate::linux_vt::VT_ACTIVATE;
pub use crate::linux_vt::VT_GETMODE;
pub use crate::linux_vt::VT_OPENQRY;
pub use crate::linux_vt::VT_SETMODE;
pub use crate::linux_vt::VT_WAITACTIVE;

// ---------------------------------------------------------------------------
// VT modes
// ---------------------------------------------------------------------------

/// Auto VT switching (kernel handles it).
pub const VT_AUTO: u8 = 0;
/// Process-controlled VT switching.
pub const VT_PROCESS: u8 = 1;
/// Acknowledge-based switching.
pub const VT_ACKACQ: u8 = 2;

// ---------------------------------------------------------------------------
// VT states
// ---------------------------------------------------------------------------

/// VT is active (displayed).
pub const VT_IS_ACTIVE: u32 = 0;
/// VT is in use (has processes).
pub const VT_IS_IN_USE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vt_modes() {
        assert_eq!(VT_AUTO, 0);
        assert_eq!(VT_PROCESS, 1);
        assert_eq!(VT_ACKACQ, 2);
    }

    #[test]
    fn test_reexports() {
        assert_ne!(VT_OPENQRY, VT_GETMODE);
        assert_ne!(VT_ACTIVATE, VT_WAITACTIVE);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(VT_OPENQRY, crate::linux_vt::VT_OPENQRY);
    }
}
