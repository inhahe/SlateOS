//! `<linux/i2c-dev.h>` — userspace I²C bus access.
//!
//! The `i2c-dev` driver exposes each bus as `/dev/i2c-N`; `i2cdetect`,
//! `i2cget/set`, libsensors, and any custom firmware-flashing tool
//! issues SMBus transactions through the ioctls below. Same API used
//! by EEPROM dump utilities, BME280 sensor drivers in userspace, and
//! HDMI-DDC display probing.

// ---------------------------------------------------------------------------
// ioctl numbers (raw, not _IO-encoded)
// ---------------------------------------------------------------------------

/// `I2C_RETRIES` — number of retries on NACK.
pub const I2C_RETRIES: u32 = 0x0701;
/// `I2C_TIMEOUT` — set timeout in jiffies/10ms units.
pub const I2C_TIMEOUT: u32 = 0x0702;
/// `I2C_SLAVE` — set the 7-bit slave address.
pub const I2C_SLAVE: u32 = 0x0703;
/// `I2C_SLAVE_FORCE` — like SLAVE, but allow a driver-claimed address.
pub const I2C_SLAVE_FORCE: u32 = 0x0706;
/// `I2C_TENBIT` — switch to 10-bit addressing.
pub const I2C_TENBIT: u32 = 0x0704;
/// `I2C_FUNCS` — query adapter capabilities.
pub const I2C_FUNCS: u32 = 0x0705;
/// `I2C_RDWR` — combined read/write transaction (struct i2c_rdwr_ioctl_data).
pub const I2C_RDWR: u32 = 0x0707;
/// `I2C_PEC` — enable SMBus Packet Error Checking.
pub const I2C_PEC: u32 = 0x0708;
/// `I2C_SMBUS` — issue an SMBus transaction (struct i2c_smbus_ioctl_data).
pub const I2C_SMBUS: u32 = 0x0720;

// ---------------------------------------------------------------------------
// SMBus transaction-direction flag (struct i2c_smbus_ioctl_data.read_write)
// ---------------------------------------------------------------------------

/// Direction: write.
pub const I2C_SMBUS_WRITE: u8 = 0;
/// Direction: read.
pub const I2C_SMBUS_READ: u8 = 1;

// ---------------------------------------------------------------------------
// SMBus transaction sizes (.size)
// ---------------------------------------------------------------------------

pub const I2C_SMBUS_QUICK: u32 = 0;
pub const I2C_SMBUS_BYTE: u32 = 1;
pub const I2C_SMBUS_BYTE_DATA: u32 = 2;
pub const I2C_SMBUS_WORD_DATA: u32 = 3;
pub const I2C_SMBUS_PROC_CALL: u32 = 4;
pub const I2C_SMBUS_BLOCK_DATA: u32 = 5;
pub const I2C_SMBUS_I2C_BLOCK_BROKEN: u32 = 6;
pub const I2C_SMBUS_BLOCK_PROC_CALL: u32 = 7;
pub const I2C_SMBUS_I2C_BLOCK_DATA: u32 = 8;

// ---------------------------------------------------------------------------
// SMBus payload limits
// ---------------------------------------------------------------------------

/// Maximum block-data payload length (SMBus 2.0).
pub const I2C_SMBUS_BLOCK_MAX: u32 = 32;
/// Buffer size including the length byte and PEC.
pub const I2C_SMBUS_BLOCK_BUF_LEN: u32 = I2C_SMBUS_BLOCK_MAX + 2;

// ---------------------------------------------------------------------------
// Adapter functionality bits (returned by I2C_FUNCS as u32 bitmask)
// ---------------------------------------------------------------------------

pub const I2C_FUNC_I2C: u32 = 1 << 0;
pub const I2C_FUNC_10BIT_ADDR: u32 = 1 << 1;
pub const I2C_FUNC_PROTOCOL_MANGLING: u32 = 1 << 2;
pub const I2C_FUNC_SMBUS_PEC: u32 = 1 << 3;
pub const I2C_FUNC_NOSTART: u32 = 1 << 4;
pub const I2C_FUNC_SMBUS_BLOCK_PROC_CALL: u32 = 1 << 15;
pub const I2C_FUNC_SMBUS_QUICK: u32 = 1 << 16;
pub const I2C_FUNC_SMBUS_READ_BYTE: u32 = 1 << 17;
pub const I2C_FUNC_SMBUS_WRITE_BYTE: u32 = 1 << 18;
pub const I2C_FUNC_SMBUS_READ_BYTE_DATA: u32 = 1 << 19;
pub const I2C_FUNC_SMBUS_WRITE_BYTE_DATA: u32 = 1 << 20;
pub const I2C_FUNC_SMBUS_READ_WORD_DATA: u32 = 1 << 21;
pub const I2C_FUNC_SMBUS_WRITE_WORD_DATA: u32 = 1 << 22;
pub const I2C_FUNC_SMBUS_PROC_CALL: u32 = 1 << 23;
pub const I2C_FUNC_SMBUS_READ_BLOCK_DATA: u32 = 1 << 24;
pub const I2C_FUNC_SMBUS_WRITE_BLOCK_DATA: u32 = 1 << 25;
pub const I2C_FUNC_SMBUS_READ_I2C_BLOCK: u32 = 1 << 26;
pub const I2C_FUNC_SMBUS_WRITE_I2C_BLOCK: u32 = 1 << 27;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_in_0x07xx_range() {
        for &io in &[
            I2C_RETRIES,
            I2C_TIMEOUT,
            I2C_SLAVE,
            I2C_SLAVE_FORCE,
            I2C_TENBIT,
            I2C_FUNCS,
            I2C_RDWR,
            I2C_PEC,
            I2C_SMBUS,
        ] {
            assert_eq!(io & 0xFF00, 0x0700);
        }
    }

    #[test]
    fn test_smbus_direction_distinct() {
        assert_eq!(I2C_SMBUS_WRITE, 0);
        assert_eq!(I2C_SMBUS_READ, 1);
    }

    #[test]
    fn test_smbus_sizes_dense() {
        let s = [
            I2C_SMBUS_QUICK,
            I2C_SMBUS_BYTE,
            I2C_SMBUS_BYTE_DATA,
            I2C_SMBUS_WORD_DATA,
            I2C_SMBUS_PROC_CALL,
            I2C_SMBUS_BLOCK_DATA,
            I2C_SMBUS_I2C_BLOCK_BROKEN,
            I2C_SMBUS_BLOCK_PROC_CALL,
            I2C_SMBUS_I2C_BLOCK_DATA,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_block_payload_sizes() {
        assert_eq!(I2C_SMBUS_BLOCK_MAX, 32);
        // +1 length, +1 PEC.
        assert_eq!(I2C_SMBUS_BLOCK_BUF_LEN, 34);
    }

    #[test]
    fn test_func_bits_pow2_and_distinct() {
        let f = [
            I2C_FUNC_I2C,
            I2C_FUNC_10BIT_ADDR,
            I2C_FUNC_PROTOCOL_MANGLING,
            I2C_FUNC_SMBUS_PEC,
            I2C_FUNC_NOSTART,
            I2C_FUNC_SMBUS_BLOCK_PROC_CALL,
            I2C_FUNC_SMBUS_QUICK,
            I2C_FUNC_SMBUS_READ_BYTE,
            I2C_FUNC_SMBUS_WRITE_BYTE,
            I2C_FUNC_SMBUS_READ_BYTE_DATA,
            I2C_FUNC_SMBUS_WRITE_BYTE_DATA,
            I2C_FUNC_SMBUS_READ_WORD_DATA,
            I2C_FUNC_SMBUS_WRITE_WORD_DATA,
            I2C_FUNC_SMBUS_PROC_CALL,
            I2C_FUNC_SMBUS_READ_BLOCK_DATA,
            I2C_FUNC_SMBUS_WRITE_BLOCK_DATA,
            I2C_FUNC_SMBUS_READ_I2C_BLOCK,
            I2C_FUNC_SMBUS_WRITE_I2C_BLOCK,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }
}
