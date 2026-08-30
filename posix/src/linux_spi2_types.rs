//! `<linux/spi/spidev.h>` — Additional SPI constants.
//!
//! Supplementary SPI constants covering mode flags,
//! transfer options, and IOCTL commands.

// ---------------------------------------------------------------------------
// SPI mode bits
// ---------------------------------------------------------------------------

/// Clock phase.
pub const SPI_CPHA: u32 = 0x01;
/// Clock polarity.
pub const SPI_CPOL: u32 = 0x02;
/// Mode 0 (CPOL=0, CPHA=0).
pub const SPI_MODE_0: u32 = 0;
/// Mode 1 (CPOL=0, CPHA=1).
pub const SPI_MODE_1: u32 = SPI_CPHA;
/// Mode 2 (CPOL=1, CPHA=0).
pub const SPI_MODE_2: u32 = SPI_CPOL;
/// Mode 3 (CPOL=1, CPHA=1).
pub const SPI_MODE_3: u32 = SPI_CPOL | SPI_CPHA;
/// Chip select active high.
pub const SPI_CS_HIGH: u32 = 0x04;
/// LSB first.
pub const SPI_LSB_FIRST: u32 = 0x08;
/// 3-wire mode.
pub const SPI_3WIRE: u32 = 0x10;
/// Loopback.
pub const SPI_LOOP: u32 = 0x20;
/// No chip select.
pub const SPI_NO_CS: u32 = 0x40;
/// Ready signal.
pub const SPI_READY: u32 = 0x80;
/// Dual SPI.
pub const SPI_TX_DUAL: u32 = 0x100;
/// Dual RX.
pub const SPI_TX_QUAD: u32 = 0x200;
/// Octal TX.
pub const SPI_TX_OCTAL: u32 = 0x2000;
/// Dual RX.
pub const SPI_RX_DUAL: u32 = 0x400;
/// Quad RX.
pub const SPI_RX_QUAD: u32 = 0x800;
/// Octal RX.
pub const SPI_RX_OCTAL: u32 = 0x4000;
/// RX CPHA flip.
pub const SPI_RX_CPHA_FLIP: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// SPI IOCTL commands
// ---------------------------------------------------------------------------

/// Read mode.
pub const SPI_IOC_RD_MODE: u32 = 0x80016B01;
/// Write mode.
pub const SPI_IOC_WR_MODE: u32 = 0x40016B01;
/// Read LSB first.
pub const SPI_IOC_RD_LSB_FIRST: u32 = 0x80016B02;
/// Write LSB first.
pub const SPI_IOC_WR_LSB_FIRST: u32 = 0x40016B02;
/// Read bits per word.
pub const SPI_IOC_RD_BITS_PER_WORD: u32 = 0x80016B03;
/// Write bits per word.
pub const SPI_IOC_WR_BITS_PER_WORD: u32 = 0x40016B03;
/// Read max speed.
pub const SPI_IOC_RD_MAX_SPEED_HZ: u32 = 0x80046B04;
/// Write max speed.
pub const SPI_IOC_WR_MAX_SPEED_HZ: u32 = 0x40046B04;
/// Read mode32.
pub const SPI_IOC_RD_MODE32: u32 = 0x80046B05;
/// Write mode32.
pub const SPI_IOC_WR_MODE32: u32 = 0x40046B05;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes() {
        assert_eq!(SPI_MODE_0, 0);
        assert_eq!(SPI_MODE_1, SPI_CPHA);
        assert_eq!(SPI_MODE_2, SPI_CPOL);
        assert_eq!(SPI_MODE_3, SPI_CPOL | SPI_CPHA);
    }

    #[test]
    fn test_mode_bits_distinct() {
        let bits = [
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
            SPI_RX_DUAL,
            SPI_RX_QUAD,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_rd_wr_pairs() {
        assert_ne!(SPI_IOC_RD_MODE, SPI_IOC_WR_MODE);
        assert_ne!(SPI_IOC_RD_MAX_SPEED_HZ, SPI_IOC_WR_MAX_SPEED_HZ);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SPI_IOC_RD_MODE,
            SPI_IOC_WR_MODE,
            SPI_IOC_RD_LSB_FIRST,
            SPI_IOC_WR_LSB_FIRST,
            SPI_IOC_RD_BITS_PER_WORD,
            SPI_IOC_WR_BITS_PER_WORD,
            SPI_IOC_RD_MAX_SPEED_HZ,
            SPI_IOC_WR_MAX_SPEED_HZ,
            SPI_IOC_RD_MODE32,
            SPI_IOC_WR_MODE32,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
