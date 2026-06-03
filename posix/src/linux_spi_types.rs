//! `<linux/spi/spi.h>` — SPI (Serial Peripheral Interface) constants.
//!
//! SPI is a synchronous serial bus used for short-distance communication
//! between a master and one or more slave devices. It's used for flash
//! memory, sensors, display controllers, ADCs, and SD cards. The Linux
//! SPI subsystem provides both controller drivers and protocol drivers.

// ---------------------------------------------------------------------------
// SPI mode flags (CPOL, CPHA, etc.)
// ---------------------------------------------------------------------------

/// Clock phase: data sampled on leading edge.
pub const SPI_CPHA: u32 = 1 << 0;
/// Clock polarity: idle high.
pub const SPI_CPOL: u32 = 1 << 1;
/// Mode 0 (CPOL=0, CPHA=0).
pub const SPI_MODE_0: u32 = 0;
/// Mode 1 (CPOL=0, CPHA=1).
pub const SPI_MODE_1: u32 = SPI_CPHA;
/// Mode 2 (CPOL=1, CPHA=0).
pub const SPI_MODE_2: u32 = SPI_CPOL;
/// Mode 3 (CPOL=1, CPHA=1).
pub const SPI_MODE_3: u32 = SPI_CPOL | SPI_CPHA;
/// Chip select active high (instead of default low).
pub const SPI_CS_HIGH: u32 = 1 << 2;
/// LSB first (instead of default MSB first).
pub const SPI_LSB_FIRST: u32 = 1 << 3;
/// 3-wire SPI (SI/SO shared).
pub const SPI_3WIRE: u32 = 1 << 4;
/// Loopback mode.
pub const SPI_LOOP: u32 = 1 << 5;
/// No chip select.
pub const SPI_NO_CS: u32 = 1 << 6;
/// Slave mode (device is slave).
pub const SPI_READY: u32 = 1 << 7;
/// Dual SPI (2 data lines).
pub const SPI_TX_DUAL: u32 = 1 << 8;
/// Quad SPI (4 data lines).
pub const SPI_TX_QUAD: u32 = 1 << 9;
/// Octal SPI (8 data lines).
pub const SPI_TX_OCTAL: u32 = 1 << 10;
/// Dual receive.
pub const SPI_RX_DUAL: u32 = 1 << 11;
/// Quad receive.
pub const SPI_RX_QUAD: u32 = 1 << 12;
/// Octal receive.
pub const SPI_RX_OCTAL: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// SPI transfer flags
// ---------------------------------------------------------------------------

/// Use 8 bits per word (override device default).
pub const SPI_TRANS_BITS_PER_WORD_8: u8 = 8;
/// Use 16 bits per word.
pub const SPI_TRANS_BITS_PER_WORD_16: u8 = 16;
/// Use 32 bits per word.
pub const SPI_TRANS_BITS_PER_WORD_32: u8 = 32;

// ---------------------------------------------------------------------------
// Common SPI speeds
// ---------------------------------------------------------------------------

/// Maximum SPI speed for most flash (50 MHz).
pub const SPI_SPEED_50MHZ: u32 = 50_000_000;
/// High-speed SPI flash (100 MHz).
pub const SPI_SPEED_100MHZ: u32 = 100_000_000;
/// Maximum typical SPI speed (200 MHz).
pub const SPI_SPEED_200MHZ: u32 = 200_000_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_values() {
        assert_eq!(SPI_MODE_0, 0);
        assert_eq!(SPI_MODE_1, 1);
        assert_eq!(SPI_MODE_2, 2);
        assert_eq!(SPI_MODE_3, 3);
    }

    #[test]
    fn test_mode_flags_no_overlap() {
        let flags = [
            SPI_CPHA,
            SPI_CPOL,
            SPI_CS_HIGH,
            SPI_LSB_FIRST,
            SPI_3WIRE,
            SPI_LOOP,
            SPI_NO_CS,
            SPI_READY,
            SPI_TX_DUAL,
            SPI_TX_QUAD,
            SPI_TX_OCTAL,
            SPI_RX_DUAL,
            SPI_RX_QUAD,
            SPI_RX_OCTAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bits_per_word_distinct() {
        let bpw = [
            SPI_TRANS_BITS_PER_WORD_8,
            SPI_TRANS_BITS_PER_WORD_16,
            SPI_TRANS_BITS_PER_WORD_32,
        ];
        for i in 0..bpw.len() {
            for j in (i + 1)..bpw.len() {
                assert_ne!(bpw[i], bpw[j]);
            }
        }
    }

    #[test]
    fn test_speeds() {
        assert!(SPI_SPEED_50MHZ < SPI_SPEED_100MHZ);
        assert!(SPI_SPEED_100MHZ < SPI_SPEED_200MHZ);
    }
}
