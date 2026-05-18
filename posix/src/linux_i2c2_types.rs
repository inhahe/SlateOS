//! `<linux/i2c-dev.h>` — Additional I2C constants.
//!
//! Supplementary I2C constants covering bus functionality,
//! SMBUS protocol types, and transfer flags.

// ---------------------------------------------------------------------------
// I2C functionality bits (I2C_FUNC_*)
// ---------------------------------------------------------------------------

/// Basic I2C.
pub const I2C_FUNC_I2C: u32 = 0x00000001;
/// 10-bit addresses.
pub const I2C_FUNC_10BIT_ADDR: u32 = 0x00000002;
/// Protocol mangling.
pub const I2C_FUNC_PROTOCOL_MANGLING: u32 = 0x00000004;
/// SMBUS PEC.
pub const I2C_FUNC_SMBUS_PEC: u32 = 0x00000008;
/// No start.
pub const I2C_FUNC_NOSTART: u32 = 0x00000010;
/// Slave.
pub const I2C_FUNC_SLAVE: u32 = 0x00000020;
/// SMBUS quick.
pub const I2C_FUNC_SMBUS_QUICK: u32 = 0x00010000;
/// SMBUS read byte.
pub const I2C_FUNC_SMBUS_READ_BYTE: u32 = 0x00020000;
/// SMBUS write byte.
pub const I2C_FUNC_SMBUS_WRITE_BYTE: u32 = 0x00040000;
/// SMBUS read byte data.
pub const I2C_FUNC_SMBUS_READ_BYTE_DATA: u32 = 0x00080000;
/// SMBUS write byte data.
pub const I2C_FUNC_SMBUS_WRITE_BYTE_DATA: u32 = 0x00100000;
/// SMBUS read word data.
pub const I2C_FUNC_SMBUS_READ_WORD_DATA: u32 = 0x00200000;
/// SMBUS write word data.
pub const I2C_FUNC_SMBUS_WRITE_WORD_DATA: u32 = 0x00400000;
/// SMBUS read block data.
pub const I2C_FUNC_SMBUS_READ_BLOCK_DATA: u32 = 0x01000000;
/// SMBUS write block data.
pub const I2C_FUNC_SMBUS_WRITE_BLOCK_DATA: u32 = 0x02000000;
/// SMBUS host notify.
pub const I2C_FUNC_SMBUS_HOST_NOTIFY: u32 = 0x10000000;

// ---------------------------------------------------------------------------
// SMBUS transfer types
// ---------------------------------------------------------------------------

/// SMBUS quick.
pub const I2C_SMBUS_QUICK: u32 = 0;
/// SMBUS byte.
pub const I2C_SMBUS_BYTE: u32 = 1;
/// SMBUS byte data.
pub const I2C_SMBUS_BYTE_DATA: u32 = 2;
/// SMBUS word data.
pub const I2C_SMBUS_WORD_DATA: u32 = 3;
/// SMBUS process call.
pub const I2C_SMBUS_PROC_CALL: u32 = 4;
/// SMBUS block data.
pub const I2C_SMBUS_BLOCK_DATA: u32 = 5;
/// SMBUS I2C block broken.
pub const I2C_SMBUS_I2C_BLOCK_BROKEN: u32 = 6;
/// SMBUS block process call.
pub const I2C_SMBUS_BLOCK_PROC_CALL: u32 = 7;
/// SMBUS I2C block data.
pub const I2C_SMBUS_I2C_BLOCK_DATA: u32 = 8;

// ---------------------------------------------------------------------------
// I2C transfer flags
// ---------------------------------------------------------------------------

/// Read from device.
pub const I2C_M_RD: u16 = 0x0001;
/// Ten-bit address.
pub const I2C_M_TEN: u16 = 0x0010;
/// DMA safe buffer.
pub const I2C_M_DMA_SAFE: u16 = 0x0200;
/// Receive length first.
pub const I2C_M_RECV_LEN: u16 = 0x0400;
/// No start.
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
/// Ignore NACK.
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
/// Reverse direction.
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// No start condition.
pub const I2C_M_NOSTART: u16 = 0x4000;
/// Stop condition.
pub const I2C_M_STOP: u16 = 0x8000u16;

// ---------------------------------------------------------------------------
// SMBUS block size
// ---------------------------------------------------------------------------

/// Maximum SMBUS block size.
pub const I2C_SMBUS_BLOCK_MAX: u32 = 32;
/// SMBUS read direction.
pub const I2C_SMBUS_READ: u8 = 1;
/// SMBUS write direction.
pub const I2C_SMBUS_WRITE: u8 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_func_bits_power_of_two() {
        let funcs = [
            I2C_FUNC_I2C, I2C_FUNC_10BIT_ADDR,
            I2C_FUNC_PROTOCOL_MANGLING, I2C_FUNC_SMBUS_PEC,
            I2C_FUNC_NOSTART, I2C_FUNC_SLAVE,
            I2C_FUNC_SMBUS_QUICK, I2C_FUNC_SMBUS_READ_BYTE,
            I2C_FUNC_SMBUS_WRITE_BYTE,
        ];
        for f in &funcs {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_smbus_types_sequential() {
        assert_eq!(I2C_SMBUS_QUICK, 0);
        assert_eq!(I2C_SMBUS_BYTE, 1);
        assert_eq!(I2C_SMBUS_I2C_BLOCK_DATA, 8);
    }

    #[test]
    fn test_transfer_flags_distinct() {
        let flags: [u16; 9] = [
            I2C_M_RD, I2C_M_TEN, I2C_M_DMA_SAFE,
            I2C_M_RECV_LEN, I2C_M_NO_RD_ACK, I2C_M_IGNORE_NAK,
            I2C_M_REV_DIR_ADDR, I2C_M_NOSTART, I2C_M_STOP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_smbus_direction() {
        assert_eq!(I2C_SMBUS_WRITE, 0);
        assert_eq!(I2C_SMBUS_READ, 1);
    }

    #[test]
    fn test_block_max() {
        assert_eq!(I2C_SMBUS_BLOCK_MAX, 32);
    }
}
