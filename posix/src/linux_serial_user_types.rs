//! `<linux/serial.h>` — TIOCSSERIAL / TIOCGSERIAL userspace ABI.
//!
//! Programs that twiddle low-level UART parameters (setserial,
//! systemd-tty-ask-password-agent, irda/IrLPT bridges) use the
//! TIOCGSERIAL / TIOCSSERIAL ioctls to read or set the port type,
//! IRQ, base address, divisor, and a grab-bag of legacy flags.

// ---------------------------------------------------------------------------
// Port types (serial_struct.type)
// ---------------------------------------------------------------------------

/// Unknown / no port.
pub const PORT_UNKNOWN: u32 = 0;
/// 8250 (no FIFO).
pub const PORT_8250: u32 = 1;
/// 16450.
pub const PORT_16450: u32 = 2;
/// 16550 (broken FIFO).
pub const PORT_16550: u32 = 3;
/// 16550A (working FIFO).
pub const PORT_16550A: u32 = 4;
/// Cirrus 6526.
pub const PORT_CIRRUS: u32 = 5;
/// 16650.
pub const PORT_16650: u32 = 6;
/// 16650 V2.
pub const PORT_16650V2: u32 = 7;
/// 16750.
pub const PORT_16750: u32 = 8;
/// 16850 ("Startech-style") — flagged broken in many kernels.
pub const PORT_STARTECH: u32 = 9;
/// 16C950/954.
pub const PORT_16C950: u32 = 10;

// ---------------------------------------------------------------------------
// serial_struct.flags
// ---------------------------------------------------------------------------

/// Spread interrupts (8250 SCI quirk).
pub const ASYNC_HUP_NOTIFY: u32 = 1 << 0;
/// `fourport` mode — share an IRQ across 4 ports.
pub const ASYNC_FOURPORT: u32 = 1 << 1;
/// Skip UART test on probe.
pub const ASYNC_SAK: u32 = 1 << 2;
/// Use the spread-spectrum quirk.
pub const ASYNC_SPLIT_TERMIOS: u32 = 1 << 3;
/// Spd is high (38400 -> 57600).
pub const ASYNC_SPD_HI: u32 = 1 << 4;
/// Spd is very high (38400 -> 115200).
pub const ASYNC_SPD_VHI: u32 = 1 << 5;
/// Skip self-test of the UART.
pub const ASYNC_SKIP_TEST: u32 = 1 << 6;
/// Auto-IRQ on probe.
pub const ASYNC_AUTO_IRQ: u32 = 1 << 7;
/// Session lockout when carrier drops.
pub const ASYNC_SESSION_LOCKOUT: u32 = 1 << 8;
/// PGRP lockout.
pub const ASYNC_PGRP_LOCKOUT: u32 = 1 << 9;
/// Callout port — historical /dev/cua* semantics.
pub const ASYNC_CALLOUT_NOHUP: u32 = 1 << 10;
/// Hardware flow control (RTS/CTS).
pub const ASYNC_HARDPPS_CD: u32 = 1 << 11;
/// Spd is shortcut (38400 -> 230400).
pub const ASYNC_SPD_SHI: u32 = 1 << 12;
/// Low-latency mode (skip tty buffer batching).
pub const ASYNC_LOW_LATENCY: u32 = 1 << 13;
/// Buggy UART — apply workarounds.
pub const ASYNC_BUGGY_UART: u32 = 1 << 14;
/// Spd is warp (38400 -> 460800).
pub const ASYNC_SPD_WARP: u32 = 1 << 15;

/// Mask of speed-override bits.
pub const ASYNC_SPD_MASK: u32 =
    ASYNC_SPD_HI | ASYNC_SPD_VHI | ASYNC_SPD_SHI | ASYNC_SPD_WARP;

// ---------------------------------------------------------------------------
// xmit_fifo_size defaults
// ---------------------------------------------------------------------------

/// Default xmit-FIFO size for 16550A (16 bytes).
pub const SERIAL_XMIT_SIZE_16550A: u32 = 16;
/// Default xmit-FIFO size for 16750 (64 bytes).
pub const SERIAL_XMIT_SIZE_16750: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_dense() {
        let p = [
            PORT_UNKNOWN,
            PORT_8250,
            PORT_16450,
            PORT_16550,
            PORT_16550A,
            PORT_CIRRUS,
            PORT_16650,
            PORT_16650V2,
            PORT_16750,
            PORT_STARTECH,
            PORT_16C950,
        ];
        for (i, &t) in p.iter().enumerate() {
            assert_eq!(t as usize, i);
        }
    }

    #[test]
    fn test_async_flags_pow2_distinct() {
        let f = [
            ASYNC_HUP_NOTIFY,
            ASYNC_FOURPORT,
            ASYNC_SAK,
            ASYNC_SPLIT_TERMIOS,
            ASYNC_SPD_HI,
            ASYNC_SPD_VHI,
            ASYNC_SKIP_TEST,
            ASYNC_AUTO_IRQ,
            ASYNC_SESSION_LOCKOUT,
            ASYNC_PGRP_LOCKOUT,
            ASYNC_CALLOUT_NOHUP,
            ASYNC_HARDPPS_CD,
            ASYNC_SPD_SHI,
            ASYNC_LOW_LATENCY,
            ASYNC_BUGGY_UART,
            ASYNC_SPD_WARP,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_speed_mask_includes_all_speed_flags() {
        assert_eq!(
            ASYNC_SPD_MASK,
            ASYNC_SPD_HI | ASYNC_SPD_VHI | ASYNC_SPD_SHI | ASYNC_SPD_WARP
        );
        // None of the non-speed bits leaked into the mask.
        assert_eq!(ASYNC_SPD_MASK & ASYNC_HUP_NOTIFY, 0);
        assert_eq!(ASYNC_SPD_MASK & ASYNC_LOW_LATENCY, 0);
    }

    #[test]
    fn test_fifo_sizes() {
        // 16550A: 16-byte FIFO is THE defining feature vs 16450.
        assert_eq!(SERIAL_XMIT_SIZE_16550A, 16);
        // 16750: 64-byte FIFO.
        assert_eq!(SERIAL_XMIT_SIZE_16750, 64);
        assert!(SERIAL_XMIT_SIZE_16550A < SERIAL_XMIT_SIZE_16750);
    }
}
