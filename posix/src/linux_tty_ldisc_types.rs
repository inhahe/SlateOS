//! `<linux/tty_ldisc.h>` — TTY line discipline constants.
//!
//! Line disciplines sit between the TTY driver (hardware) and
//! userspace, processing data in both directions. The default N_TTY
//! provides line editing and signal generation. Other disciplines
//! handle specialized protocols: PPP for dial-up networking, SLIP
//! for serial IP, HDLC for synchronous links, and Bluetooth HCI
//! for Bluetooth over UART. Line disciplines are set per-TTY via
//! ioctl(TIOCSETD).

// ---------------------------------------------------------------------------
// Line discipline numbers
// ---------------------------------------------------------------------------

/// Normal TTY line discipline (line editing, canonical mode).
pub const N_TTY: u32 = 0;
/// SLIP (Serial Line IP) discipline.
pub const N_SLIP: u32 = 1;
/// Mouse discipline (serial mice).
pub const N_MOUSE: u32 = 2;
/// PPP (Point-to-Point Protocol) discipline.
pub const N_PPP: u32 = 3;
/// STRIP (Starmode Radio IP) discipline.
pub const N_STRIP: u32 = 4;
/// AX.25 (amateur radio) discipline.
pub const N_AX25: u32 = 5;
/// X.25 discipline.
pub const N_X25: u32 = 6;
/// 6-pack (amateur radio) discipline.
pub const N_6PACK: u32 = 7;
/// Multipoint HDLC.
pub const N_MASC: u32 = 8;
/// HDLC (synchronous serial) discipline.
pub const N_HDLC: u32 = 13;
/// SYNC PPP discipline.
pub const N_SYNC_PPP: u32 = 14;
/// Bluetooth HCI UART discipline.
pub const N_HCI: u32 = 15;
/// IrDA discipline.
pub const N_IRDA: u32 = 16;
/// Profibus discipline.
pub const N_PROFIBUS: u32 = 20;
/// CAN (Controller Area Network) over serial.
pub const N_SLCAN: u32 = 17;
/// GPS NMEA discipline.
pub const N_GPS: u32 = 22;
/// GSM MUX (multiplexing over serial for modems).
pub const N_GSM0710: u32 = 21;
/// NULL discipline (discard all data).
pub const N_NULL: u32 = 27;

// ---------------------------------------------------------------------------
// Line discipline ioctl commands
// ---------------------------------------------------------------------------

/// Get current line discipline number.
pub const TIOCGETD: u32 = 0x5424;
/// Set line discipline number.
pub const TIOCSETD: u32 = 0x5423;

// ---------------------------------------------------------------------------
// Maximum line discipline number
// ---------------------------------------------------------------------------

/// Maximum line discipline ID.
pub const NR_LDISCS: u32 = 30;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ldiscs_distinct() {
        let ldiscs = [
            N_TTY, N_SLIP, N_MOUSE, N_PPP, N_STRIP, N_AX25, N_X25, N_6PACK, N_MASC, N_HDLC,
            N_SYNC_PPP, N_HCI, N_IRDA, N_SLCAN, N_PROFIBUS, N_GSM0710, N_GPS, N_NULL,
        ];
        for i in 0..ldiscs.len() {
            for j in (i + 1)..ldiscs.len() {
                assert_ne!(ldiscs[i], ldiscs[j]);
            }
        }
    }

    #[test]
    fn test_all_within_max() {
        let all = [
            N_TTY, N_SLIP, N_MOUSE, N_PPP, N_STRIP, N_AX25, N_X25, N_6PACK, N_MASC, N_HDLC,
            N_SYNC_PPP, N_HCI, N_IRDA, N_SLCAN, N_PROFIBUS, N_GSM0710, N_GPS, N_NULL,
        ];
        for &ldisc in &all {
            assert!(ldisc < NR_LDISCS);
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(TIOCGETD, TIOCSETD);
    }
}
