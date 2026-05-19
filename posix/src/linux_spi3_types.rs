//! `<linux/spi/spidev.h>` — Additional SPI constants (part 3).
//!
//! Supplementary SPI constants covering mode bits,
//! transfer flags, and ioctl commands.

// ---------------------------------------------------------------------------
// SPI mode bits
// ---------------------------------------------------------------------------

/// Clock phase.
pub const SPI_CPHA: u32 = 0x01;
/// Clock polarity.
pub const SPI_CPOL: u32 = 0x02;
/// Mode 0: CPOL=0, CPHA=0.
pub const SPI_MODE_0: u32 = 0;
/// Mode 1: CPOL=0, CPHA=1.
pub const SPI_MODE_1: u32 = SPI_CPHA;
/// Mode 2: CPOL=1, CPHA=0.
pub const SPI_MODE_2: u32 = SPI_CPOL;
/// Mode 3: CPOL=1, CPHA=1.
pub const SPI_MODE_3: u32 = SPI_CPOL | SPI_CPHA;
/// Chip select active high.
pub const SPI_CS_HIGH: u32 = 0x04;
/// LSB first.
pub const SPI_LSB_FIRST: u32 = 0x08;
/// 3-wire mode.
pub const SPI_3WIRE: u32 = 0x10;
/// Loopback mode.
pub const SPI_LOOP: u32 = 0x20;
/// No chip select.
pub const SPI_NO_CS: u32 = 0x40;
/// Ready signal.
pub const SPI_READY: u32 = 0x80;
/// Transmit dual.
pub const SPI_TX_DUAL: u32 = 0x100;
/// Transmit quad.
pub const SPI_TX_QUAD: u32 = 0x200;
/// Receive dual.
pub const SPI_RX_DUAL: u32 = 0x400;
/// Receive quad.
pub const SPI_RX_QUAD: u32 = 0x800;
/// CS word.
pub const SPI_CS_WORD: u32 = 0x1000;
/// TX octal.
pub const SPI_TX_OCTAL: u32 = 0x2000;
/// RX octal.
pub const SPI_RX_OCTAL: u32 = 0x4000;
/// 3-wire HIZ.
pub const SPI_3WIRE_HIZ: u32 = 0x8000;

// ---------------------------------------------------------------------------
// SPI transfer flags
// ---------------------------------------------------------------------------

/// Use speed hz.
pub const SPI_TRANS_SPEED_HZ: u32 = 1 << 0;
/// Use delay.
pub const SPI_TRANS_DELAY: u32 = 1 << 1;
/// Use bits per word.
pub const SPI_TRANS_BPW: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes() {
        assert_eq!(SPI_MODE_0, 0);
        assert_eq!(SPI_MODE_1, 1);
        assert_eq!(SPI_MODE_2, 2);
        assert_eq!(SPI_MODE_3, 3);
    }

    #[test]
    fn test_mode_bits_distinct() {
        let bits = [
            SPI_CPHA, SPI_CPOL, SPI_CS_HIGH, SPI_LSB_FIRST,
            SPI_3WIRE, SPI_LOOP, SPI_NO_CS, SPI_READY,
            SPI_TX_DUAL, SPI_TX_QUAD, SPI_RX_DUAL, SPI_RX_QUAD,
            SPI_CS_WORD, SPI_TX_OCTAL, SPI_RX_OCTAL, SPI_3WIRE_HIZ,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                // Some are compositions but individual flag bits should not overlap
                // beyond CPHA/CPOL composing modes
                if bits[i].is_power_of_two() && bits[j].is_power_of_two() {
                    assert_eq!(bits[i] & bits[j], 0);
                }
            }
        }
    }

    #[test]
    fn test_transfer_flags_no_overlap() {
        let flags = [SPI_TRANS_SPEED_HZ, SPI_TRANS_DELAY, SPI_TRANS_BPW];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
