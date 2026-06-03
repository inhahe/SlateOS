//! `<linux/i2c-dev.h>` — I2C userspace device interface constants.
//!
//! The I2C dev interface (/dev/i2c-N) provides userspace access to
//! I2C buses for communicating with slave devices (sensors, EEPROMs,
//! RTCs, etc.). The ioctl commands control addressing, timeouts,
//! and transfer modes.

// ---------------------------------------------------------------------------
// I2C ioctl commands
// ---------------------------------------------------------------------------

/// Set slave address (7 or 10 bit).
pub const I2C_SLAVE: u32 = 0x0703;
/// Set slave address (force, even if in use by kernel driver).
pub const I2C_SLAVE_FORCE: u32 = 0x0706;
/// Use 10-bit addressing (vs default 7-bit).
pub const I2C_TENBIT: u32 = 0x0704;
/// Get adapter functionality bitmask.
pub const I2C_FUNCS: u32 = 0x0705;
/// Combined read/write transfer (struct i2c_rdwr_ioctl_data).
pub const I2C_RDWR: u32 = 0x0707;
/// Enable/disable PEC (Packet Error Checking for SMBus).
pub const I2C_PEC: u32 = 0x0708;
/// SMBus-level access (struct i2c_smbus_ioctl_data).
pub const I2C_SMBUS: u32 = 0x0720;
/// Set bus timeout (in units of 10ms).
pub const I2C_TIMEOUT: u32 = 0x0702;
/// Set number of retries on NAK.
pub const I2C_RETRIES: u32 = 0x0701;

// ---------------------------------------------------------------------------
// I2C adapter functionality bits (from I2C_FUNCS)
// ---------------------------------------------------------------------------

/// Adapter supports plain I2C transfers.
pub const I2C_FUNC_I2C: u32 = 0x0000_0001;
/// Adapter supports 10-bit addresses.
pub const I2C_FUNC_10BIT_ADDR: u32 = 0x0000_0002;
/// Adapter supports SMBus byte/word data.
pub const I2C_FUNC_SMBUS_BLOCK_DATA: u32 = 0x0200_0000;
/// Adapter supports SMBus quick command.
pub const I2C_FUNC_SMBUS_QUICK: u32 = 0x0001_0000;
/// Adapter supports SMBus read byte.
pub const I2C_FUNC_SMBUS_READ_BYTE: u32 = 0x0002_0000;
/// Adapter supports SMBus write byte.
pub const I2C_FUNC_SMBUS_WRITE_BYTE: u32 = 0x0004_0000;
/// Adapter supports SMBus read byte data.
pub const I2C_FUNC_SMBUS_READ_BYTE_DATA: u32 = 0x0008_0000;
/// Adapter supports SMBus write byte data.
pub const I2C_FUNC_SMBUS_WRITE_BYTE_DATA: u32 = 0x0010_0000;
/// Adapter supports SMBus read word data.
pub const I2C_FUNC_SMBUS_READ_WORD_DATA: u32 = 0x0020_0000;
/// Adapter supports SMBus write word data.
pub const I2C_FUNC_SMBUS_WRITE_WORD_DATA: u32 = 0x0040_0000;
/// Adapter supports PEC.
pub const I2C_FUNC_SMBUS_PEC: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// I2C message flags (for I2C_RDWR)
// ---------------------------------------------------------------------------

/// Read message (slave → master).
pub const I2C_M_RD: u16 = 0x0001;
/// 10-bit address in this message.
pub const I2C_M_TEN: u16 = 0x0010;
/// Don't generate STOP condition.
pub const I2C_M_NOSTART: u16 = 0x4000;
/// Reversed direction (for certain protocols).
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;
/// Message length will be first received byte.
pub const I2C_M_RECV_LEN: u16 = 0x0400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            I2C_SLAVE,
            I2C_SLAVE_FORCE,
            I2C_TENBIT,
            I2C_FUNCS,
            I2C_RDWR,
            I2C_PEC,
            I2C_SMBUS,
            I2C_TIMEOUT,
            I2C_RETRIES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_func_bits_distinct() {
        let funcs = [
            I2C_FUNC_I2C,
            I2C_FUNC_10BIT_ADDR,
            I2C_FUNC_SMBUS_PEC,
            I2C_FUNC_SMBUS_QUICK,
            I2C_FUNC_SMBUS_READ_BYTE,
            I2C_FUNC_SMBUS_WRITE_BYTE,
            I2C_FUNC_SMBUS_READ_BYTE_DATA,
            I2C_FUNC_SMBUS_WRITE_BYTE_DATA,
            I2C_FUNC_SMBUS_READ_WORD_DATA,
            I2C_FUNC_SMBUS_WRITE_WORD_DATA,
            I2C_FUNC_SMBUS_BLOCK_DATA,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_msg_flags_distinct() {
        let flags = [
            I2C_M_RD,
            I2C_M_TEN,
            I2C_M_NOSTART,
            I2C_M_REV_DIR_ADDR,
            I2C_M_RECV_LEN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
