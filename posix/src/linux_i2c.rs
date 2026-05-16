//! `<linux/i2c.h>` + `<linux/i2c-dev.h>` — I2C bus interface.
//!
//! Provides ioctl constants and structures for userspace I2C
//! communication via `/dev/i2c-*` devices.

// ---------------------------------------------------------------------------
// I2C ioctl commands
// ---------------------------------------------------------------------------

/// Set slave address.
pub const I2C_SLAVE: u64 = 0x0703;
/// Set slave address (even if it's already in use by a driver).
pub const I2C_SLAVE_FORCE: u64 = 0x0706;
/// 10-bit addressing mode.
pub const I2C_TENBIT: u64 = 0x0704;
/// Get adapter functionality bits.
pub const I2C_FUNCS: u64 = 0x0705;
/// Combined read/write transfer.
pub const I2C_RDWR: u64 = 0x0707;
/// Packet error checking.
pub const I2C_PEC: u64 = 0x0708;
/// SMBus-level access.
pub const I2C_SMBUS: u64 = 0x0720;

// ---------------------------------------------------------------------------
// I2C message flags
// ---------------------------------------------------------------------------

/// Read (vs write).
pub const I2C_M_RD: u16 = 0x0001;
/// 10-bit slave address.
pub const I2C_M_TEN: u16 = 0x0010;
/// DMA-safe buffers.
pub const I2C_M_DMA_SAFE: u16 = 0x0200;
/// Use RECV_LEN for first byte.
pub const I2C_M_RECV_LEN: u16 = 0x0400;
/// No start condition.
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;
/// Do not emit STOP at end.
pub const I2C_M_NOSTART: u16 = 0x4000;
/// Treat NACK as ACK.
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// Ignore NACK.
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;
/// Skip repeated start.
pub const I2C_M_STOP: u16 = 0x8000;

// ---------------------------------------------------------------------------
// I2C adapter functionality bits
// ---------------------------------------------------------------------------

/// Plain I2C-level commands.
pub const I2C_FUNC_I2C: u32 = 0x0000_0001;
/// 10-bit addressing.
pub const I2C_FUNC_10BIT_ADDR: u32 = 0x0000_0002;
/// Use SMBUS-like PEC.
pub const I2C_FUNC_PROTOCOL_MANGLING: u32 = 0x0000_0004;
/// SMBus PEC.
pub const I2C_FUNC_SMBUS_PEC: u32 = 0x0000_0008;
/// No start.
pub const I2C_FUNC_NOSTART: u32 = 0x0000_0010;
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
/// SMBus block process call.
pub const I2C_FUNC_SMBUS_READ_BLOCK_DATA: u32 = 0x0100_0000;
/// SMBus write block data.
pub const I2C_FUNC_SMBUS_WRITE_BLOCK_DATA: u32 = 0x0200_0000;

// ---------------------------------------------------------------------------
// I2C message struct
// ---------------------------------------------------------------------------

/// Single I2C message for I2C_RDWR ioctl.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct I2cMsg {
    /// Slave address.
    pub addr: u16,
    /// Flags (I2C_M_*).
    pub flags: u16,
    /// Message length (bytes).
    pub len: u16,
    /// Pointer to data buffer.
    pub buf: *mut u8,
}

/// I2C RDWR ioctl data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct I2cRdwrIoctlData {
    /// Pointer to array of messages.
    pub msgs: *mut I2cMsg,
    /// Number of messages.
    pub nmsgs: u32,
}

/// SMBus ioctl data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct I2cSmbusIoctlData {
    /// Read (1) or write (0).
    pub read_write: u8,
    /// SMBus command byte.
    pub command: u8,
    /// Transaction type.
    pub size: u32,
    /// Pointer to data.
    pub data: *mut u8,
}

// ---------------------------------------------------------------------------
// SMBus transaction types
// ---------------------------------------------------------------------------

/// Quick transaction.
pub const I2C_SMBUS_QUICK: u32 = 0;
/// Byte transaction.
pub const I2C_SMBUS_BYTE: u32 = 1;
/// Byte data transaction.
pub const I2C_SMBUS_BYTE_DATA: u32 = 2;
/// Word data transaction.
pub const I2C_SMBUS_WORD_DATA: u32 = 3;
/// Block data transaction.
pub const I2C_SMBUS_BLOCK_DATA: u32 = 5;
/// I2C block data.
pub const I2C_SMBUS_I2C_BLOCK_DATA: u32 = 8;

/// Read direction.
pub const I2C_SMBUS_READ: u8 = 1;
/// Write direction.
pub const I2C_SMBUS_WRITE: u8 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i2c_msg_size() {
        // 2 + 2 + 2 + padding + 8 (pointer) = 16 on 64-bit.
        assert!(core::mem::size_of::<I2cMsg>() >= 14);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            I2C_SLAVE, I2C_SLAVE_FORCE, I2C_TENBIT,
            I2C_FUNCS, I2C_RDWR, I2C_PEC, I2C_SMBUS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_msg_flags_are_bits() {
        let flags = [
            I2C_M_RD, I2C_M_TEN, I2C_M_DMA_SAFE,
            I2C_M_RECV_LEN, I2C_M_NO_RD_ACK, I2C_M_NOSTART,
            I2C_M_IGNORE_NAK, I2C_M_STOP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "I2C_M flags must not overlap");
            }
        }
    }

    #[test]
    fn test_func_bits_are_bits() {
        assert_eq!(I2C_FUNC_I2C & I2C_FUNC_10BIT_ADDR, 0);
        assert_eq!(I2C_FUNC_SMBUS_QUICK & I2C_FUNC_SMBUS_READ_BYTE, 0);
    }

    #[test]
    fn test_smbus_transaction_types() {
        assert_eq!(I2C_SMBUS_QUICK, 0);
        assert_ne!(I2C_SMBUS_BYTE, I2C_SMBUS_BYTE_DATA);
        assert_ne!(I2C_SMBUS_WORD_DATA, I2C_SMBUS_BLOCK_DATA);
    }

    #[test]
    fn test_smbus_direction() {
        assert_eq!(I2C_SMBUS_READ, 1);
        assert_eq!(I2C_SMBUS_WRITE, 0);
    }
}
