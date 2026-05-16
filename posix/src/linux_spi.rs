//! `<linux/spi/spidev.h>` — SPI bus userspace interface.
//!
//! Provides ioctl constants and structures for SPI communication
//! via `/dev/spidevX.Y` devices.

// ---------------------------------------------------------------------------
// SPI mode bits
// ---------------------------------------------------------------------------

/// Clock polarity (CPOL = 0, CPHA = 0).
pub const SPI_MODE_0: u32 = 0x00;
/// CPHA = 1.
pub const SPI_MODE_1: u32 = 0x01;
/// CPOL = 1.
pub const SPI_MODE_2: u32 = 0x02;
/// CPOL = 1, CPHA = 1.
pub const SPI_MODE_3: u32 = 0x03;

/// Clock phase.
pub const SPI_CPHA: u32 = 0x01;
/// Clock polarity.
pub const SPI_CPOL: u32 = 0x02;
/// Chip select active high.
pub const SPI_CS_HIGH: u32 = 0x04;
/// LSB first (vs MSB first).
pub const SPI_LSB_FIRST: u32 = 0x08;
/// 3-wire mode (SI/SO shared).
pub const SPI_3WIRE: u32 = 0x10;
/// Loopback mode.
pub const SPI_LOOP: u32 = 0x20;
/// No chip select.
pub const SPI_NO_CS: u32 = 0x40;
/// One wire (slave pull, bidirectional data).
pub const SPI_READY: u32 = 0x80;
/// TX: double data rate.
pub const SPI_TX_DUAL: u32 = 0x100;
/// TX: quad data rate.
pub const SPI_TX_QUAD: u32 = 0x200;
/// RX: double data rate.
pub const SPI_RX_DUAL: u32 = 0x400;
/// RX: quad data rate.
pub const SPI_RX_QUAD: u32 = 0x800;

// ---------------------------------------------------------------------------
// SPI ioctl commands
// ---------------------------------------------------------------------------

/// Read SPI mode.
pub const SPI_IOC_RD_MODE: u64 = 0x8001_6B01;
/// Write SPI mode.
pub const SPI_IOC_WR_MODE: u64 = 0x4001_6B01;
/// Read LSB first setting.
pub const SPI_IOC_RD_LSB_FIRST: u64 = 0x8001_6B02;
/// Write LSB first setting.
pub const SPI_IOC_WR_LSB_FIRST: u64 = 0x4001_6B02;
/// Read bits per word.
pub const SPI_IOC_RD_BITS_PER_WORD: u64 = 0x8001_6B03;
/// Write bits per word.
pub const SPI_IOC_WR_BITS_PER_WORD: u64 = 0x4001_6B03;
/// Read max speed (Hz).
pub const SPI_IOC_RD_MAX_SPEED_HZ: u64 = 0x8004_6B04;
/// Write max speed (Hz).
pub const SPI_IOC_WR_MAX_SPEED_HZ: u64 = 0x4004_6B04;
/// Read 32-bit mode.
pub const SPI_IOC_RD_MODE32: u64 = 0x8004_6B05;
/// Write 32-bit mode.
pub const SPI_IOC_WR_MODE32: u64 = 0x4004_6B05;

// ---------------------------------------------------------------------------
// SPI transfer struct
// ---------------------------------------------------------------------------

/// A single SPI transfer.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SpiIocTransfer {
    /// TX buffer pointer (or 0).
    pub tx_buf: u64,
    /// RX buffer pointer (or 0).
    pub rx_buf: u64,
    /// Transfer length (bytes).
    pub len: u32,
    /// Speed for this transfer (Hz, or 0 for default).
    pub speed_hz: u32,
    /// Delay after transfer (usec).
    pub delay_usecs: u16,
    /// Bits per word for this transfer (or 0 for default).
    pub bits_per_word: u8,
    /// Deassert CS after this transfer.
    pub cs_change: u8,
    /// Number of TX bits.
    pub tx_nbits: u8,
    /// Number of RX bits.
    pub rx_nbits: u8,
    /// Word delay (usec).
    pub word_delay_usecs: u8,
    /// Padding.
    _pad: u8,
}

impl SpiIocTransfer {
    /// Create a zeroed transfer struct.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spi_transfer_size() {
        // 8 + 8 + 4 + 4 + 2 + 1 + 1 + 1 + 1 + 1 + 1 = 32 bytes.
        assert_eq!(core::mem::size_of::<SpiIocTransfer>(), 32);
    }

    #[test]
    fn test_spi_modes() {
        assert_eq!(SPI_MODE_0, 0);
        assert_eq!(SPI_MODE_1, SPI_CPHA);
        assert_eq!(SPI_MODE_2, SPI_CPOL);
        assert_eq!(SPI_MODE_3, SPI_CPHA | SPI_CPOL);
    }

    #[test]
    fn test_mode_flags_are_bits() {
        let flags = [
            SPI_CPHA, SPI_CPOL, SPI_CS_HIGH, SPI_LSB_FIRST,
            SPI_3WIRE, SPI_LOOP, SPI_NO_CS, SPI_READY,
            SPI_TX_DUAL, SPI_TX_QUAD, SPI_RX_DUAL, SPI_RX_QUAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "SPI mode flags must not overlap");
            }
        }
    }

    #[test]
    fn test_ioctl_rd_wr_pairs() {
        // Read and write variants should differ.
        assert_ne!(SPI_IOC_RD_MODE, SPI_IOC_WR_MODE);
        assert_ne!(SPI_IOC_RD_BITS_PER_WORD, SPI_IOC_WR_BITS_PER_WORD);
        assert_ne!(SPI_IOC_RD_MAX_SPEED_HZ, SPI_IOC_WR_MAX_SPEED_HZ);
    }

    #[test]
    fn test_transfer_zeroed() {
        let t = SpiIocTransfer::zeroed();
        assert_eq!(t.tx_buf, 0);
        assert_eq!(t.rx_buf, 0);
        assert_eq!(t.len, 0);
        assert_eq!(t.speed_hz, 0);
    }
}
