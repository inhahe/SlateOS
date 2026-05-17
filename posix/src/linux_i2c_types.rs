//! `<linux/i2c.h>` — I2C (Inter-Integrated Circuit) bus constants.
//!
//! I2C is a multi-master, multi-slave serial bus used for attaching
//! low-speed peripherals to a processor: EEPROMs, sensors, RTCs,
//! touch controllers, PMICs, and display adapters. It uses two wires
//! (SDA data, SCL clock) with 7-bit or 10-bit addressing.

// ---------------------------------------------------------------------------
// I2C transfer flags
// ---------------------------------------------------------------------------

/// Read transfer (default is write).
pub const I2C_M_RD: u16 = 0x0001;
/// 10-bit slave address.
pub const I2C_M_TEN: u16 = 0x0010;
/// Combined format (no repeated start).
pub const I2C_M_DMA_SAFE: u16 = 0x0200;
/// Length is first received byte.
pub const I2C_M_RECV_LEN: u16 = 0x0400;
/// Don't generate start condition.
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
/// Treat NACK as ACK.
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
/// Skip repeated start.
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// No start/stop framing.
pub const I2C_M_NOSTART: u16 = 0x4000;
/// Force stop after this message.
pub const I2C_M_STOP: u16 = 0x8000u16;

// ---------------------------------------------------------------------------
// I2C bus speeds
// ---------------------------------------------------------------------------

/// Standard mode (100 kHz).
pub const I2C_SPEED_STANDARD: u32 = 100_000;
/// Fast mode (400 kHz).
pub const I2C_SPEED_FAST: u32 = 400_000;
/// Fast mode plus (1 MHz).
pub const I2C_SPEED_FAST_PLUS: u32 = 1_000_000;
/// High speed mode (3.4 MHz).
pub const I2C_SPEED_HIGH: u32 = 3_400_000;
/// Ultra-fast mode (5 MHz, unidirectional).
pub const I2C_SPEED_ULTRA: u32 = 5_000_000;

// ---------------------------------------------------------------------------
// I2C functionality flags
// ---------------------------------------------------------------------------

/// Basic I2C (write/read).
pub const I2C_FUNC_I2C: u32 = 0x0000_0001;
/// 10-bit addressing.
pub const I2C_FUNC_10BIT_ADDR: u32 = 0x0000_0002;
/// SMBus quick command.
pub const I2C_FUNC_SMBUS_QUICK: u32 = 0x0001_0000;
/// SMBus read byte.
pub const I2C_FUNC_SMBUS_READ_BYTE: u32 = 0x0002_0000;
/// SMBus write byte.
pub const I2C_FUNC_SMBUS_WRITE_BYTE: u32 = 0x0004_0000;
/// SMBus read byte data.
pub const I2C_FUNC_SMBUS_READ_BYTE_DATA: u32 = 0x0008_0000;
/// SMBus write byte data.
pub const I2C_FUNC_SMBUS_WRITE_BYTE_DATA: u32 = 0x0010_0000;
/// SMBus read word data.
pub const I2C_FUNC_SMBUS_READ_WORD_DATA: u32 = 0x0020_0000;
/// SMBus write word data.
pub const I2C_FUNC_SMBUS_WRITE_WORD_DATA: u32 = 0x0040_0000;
/// SMBus block read.
pub const I2C_FUNC_SMBUS_READ_BLOCK_DATA: u32 = 0x0080_0000;
/// SMBus block write.
pub const I2C_FUNC_SMBUS_WRITE_BLOCK_DATA: u32 = 0x0100_0000;

// ---------------------------------------------------------------------------
// I2C address limits
// ---------------------------------------------------------------------------

/// Minimum 7-bit address.
pub const I2C_ADDR_MIN: u8 = 0x03;
/// Maximum 7-bit address.
pub const I2C_ADDR_MAX: u8 = 0x77;
/// 10-bit address maximum.
pub const I2C_ADDR_10BIT_MAX: u16 = 0x3FF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_flags_no_overlap() {
        let flags = [
            I2C_M_RD, I2C_M_TEN, I2C_M_DMA_SAFE, I2C_M_RECV_LEN,
            I2C_M_NO_RD_ACK, I2C_M_IGNORE_NAK, I2C_M_REV_DIR_ADDR,
            I2C_M_NOSTART, I2C_M_STOP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_speeds_ordering() {
        assert!(I2C_SPEED_STANDARD < I2C_SPEED_FAST);
        assert!(I2C_SPEED_FAST < I2C_SPEED_FAST_PLUS);
        assert!(I2C_SPEED_FAST_PLUS < I2C_SPEED_HIGH);
        assert!(I2C_SPEED_HIGH < I2C_SPEED_ULTRA);
    }

    #[test]
    fn test_func_flags_no_overlap() {
        let funcs = [
            I2C_FUNC_I2C, I2C_FUNC_10BIT_ADDR, I2C_FUNC_SMBUS_QUICK,
            I2C_FUNC_SMBUS_READ_BYTE, I2C_FUNC_SMBUS_WRITE_BYTE,
            I2C_FUNC_SMBUS_READ_BYTE_DATA, I2C_FUNC_SMBUS_WRITE_BYTE_DATA,
            I2C_FUNC_SMBUS_READ_WORD_DATA, I2C_FUNC_SMBUS_WRITE_WORD_DATA,
            I2C_FUNC_SMBUS_READ_BLOCK_DATA, I2C_FUNC_SMBUS_WRITE_BLOCK_DATA,
        ];
        for i in 0..funcs.len() {
            assert!(funcs[i].is_power_of_two());
            for j in (i + 1)..funcs.len() {
                assert_eq!(funcs[i] & funcs[j], 0);
            }
        }
    }

    #[test]
    fn test_address_limits() {
        assert!(I2C_ADDR_MIN < I2C_ADDR_MAX);
        assert!((I2C_ADDR_MAX as u16) < I2C_ADDR_10BIT_MAX);
    }
}
