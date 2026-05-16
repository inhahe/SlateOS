//! `<linux/serial.h>` — serial port interface.
//!
//! Provides structures and constants for configuring serial ports
//! via ioctl (TIOCGSERIAL / TIOCSSERIAL).

// ---------------------------------------------------------------------------
// Serial port types
// ---------------------------------------------------------------------------

/// Unknown type.
pub const PORT_UNKNOWN: i32 = 0;
/// 8250/16450 UART.
pub const PORT_8250: i32 = 1;
/// 16550 UART.
pub const PORT_16550: i32 = 2;
/// 16550A UART (with FIFO).
pub const PORT_16550A: i32 = 3;
/// Cirrus Logic CL-CD1400.
pub const PORT_CIRRUS: i32 = 4;
/// 16650 UART.
pub const PORT_16650: i32 = 5;
/// 16650V2 UART.
pub const PORT_16650V2: i32 = 6;
/// 16750 UART.
pub const PORT_16750: i32 = 7;
/// 16850 UART.
pub const PORT_16850: i32 = 8;
/// RSA (iP-Serial) UART.
pub const PORT_RSA: i32 = 9;

// ---------------------------------------------------------------------------
// Serial flags
// ---------------------------------------------------------------------------

/// Hardware flow control (CTS/RTS).
pub const ASYNC_HUP_NOTIFY: u32 = 0x0001;
/// Four-port board.
pub const ASYNC_FOURPORT: u32 = 0x0002;
/// SAK (Secure Attention Key).
pub const ASYNC_SAK: u32 = 0x0004;
/// Split irq.
pub const ASYNC_SPLIT_TERMIOS: u32 = 0x0008;
/// SPD_HI — use 56000 instead of 38400.
pub const ASYNC_SPD_HI: u32 = 0x0010;
/// SPD_VHI — use 115200.
pub const ASYNC_SPD_VHI: u32 = 0x0020;
/// Skip test during autoconfig.
pub const ASYNC_SKIP_TEST: u32 = 0x0040;
/// Auto interrupt.
pub const ASYNC_AUTO_IRQ: u32 = 0x0080;
/// Session lockout.
pub const ASYNC_SESSION_LOCKOUT: u32 = 0x0100;
/// Pgrp lockout.
pub const ASYNC_PGRP_LOCKOUT: u32 = 0x0200;
/// Callout nohup.
pub const ASYNC_CALLOUT_NOHUP: u32 = 0x0400;
/// Hardware flow control.
pub const ASYNC_HARDPPS_CD: u32 = 0x0800;
/// SPD_SHI — use 230400.
pub const ASYNC_SPD_SHI: u32 = 0x1000;
/// Low latency mode.
pub const ASYNC_LOW_LATENCY: u32 = 0x2000;
/// Bug-compatible with 16450 (no FIFO).
pub const ASYNC_BUGGY_UART: u32 = 0x4000;

/// Mask for user-settable flags.
pub const ASYNC_FLAGS: u32 = 0x7FFF;
/// SPD mask.
pub const ASYNC_SPD_MASK: u32 = ASYNC_SPD_HI | ASYNC_SPD_VHI | ASYNC_SPD_SHI;

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Get serial port info.
pub const TIOCGSERIAL: u64 = 0x541E;
/// Set serial port info.
pub const TIOCSSERIAL: u64 = 0x541F;
/// Get line status register.
pub const TIOCSERGETLSR: u64 = 0x5459;
/// Get multiport info.
pub const TIOCSERGETMULTI: u64 = 0x545A;
/// Set multiport info.
pub const TIOCSERSETMULTI: u64 = 0x545B;

// ---------------------------------------------------------------------------
// Serial struct
// ---------------------------------------------------------------------------

/// Serial port configuration (matches `struct serial_struct`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SerialStruct {
    /// Serial port type (PORT_*).
    pub type_: i32,
    /// Serial line number.
    pub line: i32,
    /// I/O port base address.
    pub port: u32,
    /// IRQ number.
    pub irq: i32,
    /// Flags (ASYNC_*).
    pub flags: i32,
    /// Transfer FIFO size.
    pub xmit_fifo_size: i32,
    /// Custom divisor.
    pub custom_divisor: i32,
    /// Baud base.
    pub baud_base: i32,
    /// Close delay (ticks).
    pub close_delay: u16,
    /// Unused.
    pub io_type: u8,
    /// Reserved.
    pub reserved_char: u8,
    /// Hub6 flag.
    pub hub6: i32,
    /// Closing wait (ticks).
    pub closing_wait: u16,
    /// Closing wait 2.
    pub closing_wait2: u16,
    /// Pointer to I/O memory.
    pub iomem_base: *mut u8,
    /// I/O memory register shift.
    pub iomem_reg_shift: u16,
    /// Port high bits.
    pub port_high: u32,
    /// I/O memory type.
    pub iomap_base: u64,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_struct_size() {
        assert!(core::mem::size_of::<SerialStruct>() >= 40);
    }

    #[test]
    fn test_port_types_sequential() {
        assert_eq!(PORT_UNKNOWN, 0);
        assert_eq!(PORT_8250, 1);
        assert_eq!(PORT_16550, 2);
        assert_eq!(PORT_16550A, 3);
    }

    #[test]
    fn test_async_flags_are_bits() {
        let flags = [
            ASYNC_HUP_NOTIFY, ASYNC_FOURPORT, ASYNC_SAK,
            ASYNC_SPLIT_TERMIOS, ASYNC_SPD_HI, ASYNC_SPD_VHI,
            ASYNC_SKIP_TEST, ASYNC_AUTO_IRQ,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "Flags must not overlap");
            }
        }
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            TIOCGSERIAL, TIOCSSERIAL, TIOCSERGETLSR,
            TIOCSERGETMULTI, TIOCSERSETMULTI,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_spd_mask() {
        assert_eq!(ASYNC_SPD_MASK, 0x1030);
    }
}
