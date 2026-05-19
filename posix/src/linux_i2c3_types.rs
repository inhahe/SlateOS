//! `<linux/i2c.h>` — Additional I2C constants (part 3).
//!
//! Supplementary I2C constants covering functionality bits,
//! message flags, and SMBUS protocol types.

// ---------------------------------------------------------------------------
// I2C functionality bits
// ---------------------------------------------------------------------------

/// Plain I2C.
pub const I2C_FUNC_I2C: u32 = 0x00000001;
/// 10-bit addressing.
pub const I2C_FUNC_10BIT_ADDR: u32 = 0x00000002;
/// Protocol mangling.
pub const I2C_FUNC_PROTOCOL_MANGLING: u32 = 0x00000004;
/// SMBus PEC.
pub const I2C_FUNC_SMBUS_PEC: u32 = 0x00000008;
/// No start.
pub const I2C_FUNC_NOSTART: u32 = 0x00000010;
/// Slave.
pub const I2C_FUNC_SLAVE: u32 = 0x00000020;
/// SMBus quick command.
pub const I2C_FUNC_SMBUS_QUICK: u32 = 0x00010000;
/// SMBus read byte.
pub const I2C_FUNC_SMBUS_READ_BYTE: u32 = 0x00020000;
/// SMBus write byte.
pub const I2C_FUNC_SMBUS_WRITE_BYTE: u32 = 0x00040000;
/// SMBus read byte data.
pub const I2C_FUNC_SMBUS_READ_BYTE_DATA: u32 = 0x00080000;
/// SMBus write byte data.
pub const I2C_FUNC_SMBUS_WRITE_BYTE_DATA: u32 = 0x00100000;
/// SMBus read word data.
pub const I2C_FUNC_SMBUS_READ_WORD_DATA: u32 = 0x00200000;
/// SMBus write word data.
pub const I2C_FUNC_SMBUS_WRITE_WORD_DATA: u32 = 0x00400000;
/// SMBus proc call.
pub const I2C_FUNC_SMBUS_PROC_CALL: u32 = 0x00800000;
/// SMBus read block data.
pub const I2C_FUNC_SMBUS_READ_BLOCK_DATA: u32 = 0x01000000;
/// SMBus write block data.
pub const I2C_FUNC_SMBUS_WRITE_BLOCK_DATA: u32 = 0x02000000;
/// SMBus read I2C block.
pub const I2C_FUNC_SMBUS_READ_I2C_BLOCK: u32 = 0x04000000;
/// SMBus write I2C block.
pub const I2C_FUNC_SMBUS_WRITE_I2C_BLOCK: u32 = 0x08000000;
/// SMBus host notify.
pub const I2C_FUNC_SMBUS_HOST_NOTIFY: u32 = 0x10000000;

// ---------------------------------------------------------------------------
// I2C message flags
// ---------------------------------------------------------------------------

/// Read from slave.
pub const I2C_M_RD: u16 = 0x0001;
/// 10-bit address.
pub const I2C_M_TEN: u16 = 0x0010;
/// DMA safe buffer.
pub const I2C_M_DMA_SAFE: u16 = 0x0200;
/// Receive length first.
pub const I2C_M_RECV_LEN: u16 = 0x0400;
/// No read ACK.
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
/// Ignore NACK.
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
/// Reverse direction.
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// No start.
pub const I2C_M_NOSTART: u16 = 0x4000;
/// Stop condition.
pub const I2C_M_STOP: u16 = 0x8000u16;

// ---------------------------------------------------------------------------
// SMBUS block size
// ---------------------------------------------------------------------------

/// Maximum SMBus block data size.
pub const I2C_SMBUS_BLOCK_MAX: u32 = 32;

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
            I2C_FUNC_SMBUS_WRITE_BYTE, I2C_FUNC_SMBUS_READ_BYTE_DATA,
            I2C_FUNC_SMBUS_WRITE_BYTE_DATA, I2C_FUNC_SMBUS_READ_WORD_DATA,
            I2C_FUNC_SMBUS_WRITE_WORD_DATA, I2C_FUNC_SMBUS_PROC_CALL,
            I2C_FUNC_SMBUS_READ_BLOCK_DATA, I2C_FUNC_SMBUS_WRITE_BLOCK_DATA,
            I2C_FUNC_SMBUS_READ_I2C_BLOCK, I2C_FUNC_SMBUS_WRITE_I2C_BLOCK,
            I2C_FUNC_SMBUS_HOST_NOTIFY,
        ];
        for f in &funcs {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_msg_flags_no_overlap() {
        let flags = [
            I2C_M_RD, I2C_M_TEN, I2C_M_DMA_SAFE,
            I2C_M_RECV_LEN, I2C_M_NO_RD_ACK, I2C_M_IGNORE_NAK,
            I2C_M_REV_DIR_ADDR, I2C_M_NOSTART, I2C_M_STOP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_block_max() {
        assert_eq!(I2C_SMBUS_BLOCK_MAX, 32);
    }
}
