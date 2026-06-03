//! `<linux/ppdev.h>` — Parallel port device (ppdev) constants.
//!
//! ppdev provides userspace access to parallel ports via /dev/parportN.
//! Unlike the kernel printer driver (lp), ppdev gives direct control
//! over data, status, and control lines. Used by EPROM programmers,
//! JTAG adapters, CNC controllers, and other hardware interfacing
//! applications that need bit-banging access to the parallel port.

// ---------------------------------------------------------------------------
// ppdev ioctl commands
// ---------------------------------------------------------------------------

/// Set data direction (0=out, 1=in).
pub const PPDATADIR: u32 = 0x4004_7090;
/// Read status register.
pub const PPRSTATUS: u32 = 0x8001_7081;
/// Read control register.
pub const PPRCONTROL: u32 = 0x8001_7083;
/// Write control register.
pub const PPWCONTROL: u32 = 0x4001_7084;
/// Read data register.
pub const PPRDATA: u32 = 0x8001_7085;
/// Write data register.
pub const PPWDATA: u32 = 0x4001_7086;
/// Claim the port (acquire exclusive access).
pub const PPCLAIM: u32 = 0x708B;
/// Release the port.
pub const PPRELEASE: u32 = 0x708C;
/// Set port mode (negotiate IEEE 1284 mode).
pub const PPSETMODE: u32 = 0x4004_7080;
/// Get port mode.
pub const PPGETMODE: u32 = 0x8004_7081;
/// Set timeout for operations.
pub const PPSETTIME: u32 = 0x4008_7096;
/// Get current timeout.
pub const PPGETTIME: u32 = 0x8008_7095;
/// Frob (modify) control register bits.
pub const PPFCONTROL: u32 = 0x4002_708E;
/// Yield the port (allow other users briefly).
pub const PPYIELD: u32 = 0x708D;
/// Negotiate to a specific IEEE 1284 phase.
pub const PPNEGOT: u32 = 0x4004_7091;
/// Set flags (e.g., exclusive access).
pub const PPSETFLAGS: u32 = 0x4004_7093;
/// Clear IRQ count.
pub const PPCLRIRQ: u32 = 0x8004_7093;

// ---------------------------------------------------------------------------
// Data direction
// ---------------------------------------------------------------------------

/// Data lines are output (host → device).
pub const PP_DATADIR_OUT: u32 = 0;
/// Data lines are input (device → host).
pub const PP_DATADIR_IN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            PPDATADIR, PPRSTATUS, PPRCONTROL, PPWCONTROL, PPRDATA, PPWDATA, PPCLAIM, PPRELEASE,
            PPSETMODE, PPGETMODE, PPSETTIME, PPGETTIME, PPFCONTROL, PPYIELD, PPNEGOT, PPSETFLAGS,
            PPCLRIRQ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_data_directions_distinct() {
        assert_ne!(PP_DATADIR_OUT, PP_DATADIR_IN);
        assert_eq!(PP_DATADIR_OUT, 0);
        assert_eq!(PP_DATADIR_IN, 1);
    }
}
