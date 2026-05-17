//! `<linux/mmc/sdio.h>` — SDIO (Secure Digital I/O) constants.
//!
//! SDIO extends the SD card interface to support I/O devices like
//! WiFi cards, Bluetooth adapters, GPS receivers, and cameras. SDIO
//! cards share the physical interface with SD memory cards but use
//! a different command set for streaming I/O operations.

// ---------------------------------------------------------------------------
// SDIO commands (CMD52, CMD53)
// ---------------------------------------------------------------------------

/// Direct I/O read/write (single byte, CMD52).
pub const SDIO_CMD_IO_RW_DIRECT: u8 = 52;
/// Extended I/O read/write (multi-byte, CMD53).
pub const SDIO_CMD_IO_RW_EXTENDED: u8 = 53;
/// Send operation condition (CMD5).
pub const SDIO_CMD_SEND_OP_COND: u8 = 5;

// ---------------------------------------------------------------------------
// SDIO CCCR (Card Common Control Registers) addresses
// ---------------------------------------------------------------------------

/// CCCR/SDIO revision.
pub const SDIO_CCCR_REV: u8 = 0x00;
/// SD specification revision.
pub const SDIO_CCCR_SD_REV: u8 = 0x01;
/// I/O enable.
pub const SDIO_CCCR_IO_EN: u8 = 0x02;
/// I/O ready.
pub const SDIO_CCCR_IO_RDY: u8 = 0x03;
/// Interrupt enable.
pub const SDIO_CCCR_INT_EN: u8 = 0x04;
/// Interrupt pending.
pub const SDIO_CCCR_INT_PENDING: u8 = 0x05;
/// I/O abort.
pub const SDIO_CCCR_ABORT: u8 = 0x06;
/// Bus interface control.
pub const SDIO_CCCR_BUS_IF: u8 = 0x07;
/// Card capability.
pub const SDIO_CCCR_CAPS: u8 = 0x08;
/// Common CIS pointer.
pub const SDIO_CCCR_CIS_PTR: u8 = 0x09;
/// Bus suspend.
pub const SDIO_CCCR_BUS_SUSPEND: u8 = 0x0C;
/// Function select.
pub const SDIO_CCCR_FN_SEL: u8 = 0x0D;
/// Power control.
pub const SDIO_CCCR_POWER: u8 = 0x12;
/// High speed.
pub const SDIO_CCCR_SPEED: u8 = 0x13;

// ---------------------------------------------------------------------------
// SDIO bus width
// ---------------------------------------------------------------------------

/// 1-bit data bus.
pub const SDIO_BUS_WIDTH_1BIT: u8 = 0x00;
/// 4-bit data bus.
pub const SDIO_BUS_WIDTH_4BIT: u8 = 0x02;
/// 8-bit data bus (embedded SDIO).
pub const SDIO_BUS_WIDTH_8BIT: u8 = 0x03;

// ---------------------------------------------------------------------------
// SDIO function class codes
// ---------------------------------------------------------------------------

/// No standard interface.
pub const SDIO_CLASS_NONE: u8 = 0x00;
/// UART (serial).
pub const SDIO_CLASS_UART: u8 = 0x01;
/// Bluetooth Type-A.
pub const SDIO_CLASS_BT_A: u8 = 0x02;
/// Bluetooth Type-B.
pub const SDIO_CLASS_BT_B: u8 = 0x03;
/// GPS.
pub const SDIO_CLASS_GPS: u8 = 0x04;
/// Camera.
pub const SDIO_CLASS_CAMERA: u8 = 0x05;
/// PHS (Personal Handy-phone).
pub const SDIO_CLASS_PHS: u8 = 0x06;
/// WLAN (802.11).
pub const SDIO_CLASS_WLAN: u8 = 0x07;
/// Embedded SDIO-ATA.
pub const SDIO_CLASS_ATA: u8 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [SDIO_CMD_IO_RW_DIRECT, SDIO_CMD_IO_RW_EXTENDED, SDIO_CMD_SEND_OP_COND];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_cccr_addresses_distinct() {
        let addrs = [
            SDIO_CCCR_REV, SDIO_CCCR_SD_REV, SDIO_CCCR_IO_EN,
            SDIO_CCCR_IO_RDY, SDIO_CCCR_INT_EN, SDIO_CCCR_INT_PENDING,
            SDIO_CCCR_ABORT, SDIO_CCCR_BUS_IF, SDIO_CCCR_CAPS,
            SDIO_CCCR_CIS_PTR, SDIO_CCCR_BUS_SUSPEND, SDIO_CCCR_FN_SEL,
            SDIO_CCCR_POWER, SDIO_CCCR_SPEED,
        ];
        for i in 0..addrs.len() {
            for j in (i + 1)..addrs.len() {
                assert_ne!(addrs[i], addrs[j]);
            }
        }
    }

    #[test]
    fn test_bus_widths_distinct() {
        let widths = [SDIO_BUS_WIDTH_1BIT, SDIO_BUS_WIDTH_4BIT, SDIO_BUS_WIDTH_8BIT];
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                assert_ne!(widths[i], widths[j]);
            }
        }
    }

    #[test]
    fn test_class_codes_distinct() {
        let classes = [
            SDIO_CLASS_NONE, SDIO_CLASS_UART, SDIO_CLASS_BT_A,
            SDIO_CLASS_BT_B, SDIO_CLASS_GPS, SDIO_CLASS_CAMERA,
            SDIO_CLASS_PHS, SDIO_CLASS_WLAN, SDIO_CLASS_ATA,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }
}
