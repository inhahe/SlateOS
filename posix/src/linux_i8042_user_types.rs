//! `<linux/i8042.h>` / `drivers/input/serio/i8042.h` — PS/2 controller.
//!
//! Even on modern desktops the i8042 controller is still emulated by
//! the chipset to deliver legacy PS/2 keyboard and mouse data (and on
//! laptops with no internal USB HID, the embedded controller speaks
//! PS/2 to the same I/O ports). The kernel `i8042` driver and the
//! `serio` framework drive it via the I/O port and command set below.

// ---------------------------------------------------------------------------
// Legacy I/O ports
// ---------------------------------------------------------------------------

/// Data port (read/write).
pub const I8042_DATA_REG: u16 = 0x60;
/// Status (read) / Command (write) port.
pub const I8042_COMMAND_REG: u16 = 0x64;
/// Alias used in some kernel sources.
pub const I8042_STATUS_REG: u16 = 0x64;

// ---------------------------------------------------------------------------
// Status register bits (read from port 0x64)
// ---------------------------------------------------------------------------

/// Output buffer full (data available to read).
pub const I8042_STR_OBF: u8 = 0x01;
/// Input buffer full (controller still processing command).
pub const I8042_STR_IBF: u8 = 0x02;
/// System flag (set after POST).
pub const I8042_STR_SYSFLAG: u8 = 0x04;
/// Last write was a command (1) vs data (0).
pub const I8042_STR_CMDDAT: u8 = 0x08;
/// Keyboard inhibit (1 = keyboard enabled).
pub const I8042_STR_KBDINH: u8 = 0x10;
/// Auxiliary device output buffer full (mouse byte).
pub const I8042_STR_AUXDATA: u8 = 0x20;
/// Timeout error.
pub const I8042_STR_TIMEOUT: u8 = 0x40;
/// Parity error.
pub const I8042_STR_PARITY: u8 = 0x80;

// ---------------------------------------------------------------------------
// Controller commands (written to port 0x64)
// ---------------------------------------------------------------------------

/// Read configuration byte.
pub const I8042_CMD_CTL_RCTR: u32 = 0x20;
/// Write configuration byte.
pub const I8042_CMD_CTL_WCTR: u32 = 0x60;
/// Self-test (returns 0x55 on success).
pub const I8042_CMD_CTL_TEST: u32 = 0xAA;
/// Test keyboard interface.
pub const I8042_CMD_KBD_TEST: u32 = 0xAB;
/// Disable keyboard interface.
pub const I8042_CMD_KBD_DISABLE: u32 = 0xAD;
/// Enable keyboard interface.
pub const I8042_CMD_KBD_ENABLE: u32 = 0xAE;
/// Test auxiliary (mouse) interface.
pub const I8042_CMD_AUX_TEST: u32 = 0xA9;
/// Disable auxiliary interface.
pub const I8042_CMD_AUX_DISABLE: u32 = 0xA7;
/// Enable auxiliary interface.
pub const I8042_CMD_AUX_ENABLE: u32 = 0xA8;
/// Write next byte to auxiliary device.
pub const I8042_CMD_AUX_SEND: u32 = 0xD4;

// ---------------------------------------------------------------------------
// Controller configuration-byte bits (data byte for RCTR/WCTR)
// ---------------------------------------------------------------------------

/// Keyboard interrupt enable.
pub const I8042_CTR_KBDINT: u8 = 0x01;
/// Auxiliary interrupt enable.
pub const I8042_CTR_AUXINT: u8 = 0x02;
/// System flag.
pub const I8042_CTR_SYSFLAG: u8 = 0x04;
/// Keyboard disable.
pub const I8042_CTR_KBDDIS: u8 = 0x10;
/// Auxiliary disable.
pub const I8042_CTR_AUXDIS: u8 = 0x20;
/// Translate scan-code-set 2 → 1.
pub const I8042_CTR_XLATE: u8 = 0x40;

// ---------------------------------------------------------------------------
// Self-test reply
// ---------------------------------------------------------------------------

/// Controller passes self-test by returning 0x55.
pub const I8042_RET_CTL_TEST: u8 = 0x55;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_io_ports() {
        // IBM AT used 0x60 (data) and 0x64 (status/command).
        assert_eq!(I8042_DATA_REG, 0x60);
        assert_eq!(I8042_COMMAND_REG, 0x64);
        assert_eq!(I8042_STATUS_REG, I8042_COMMAND_REG);
    }

    #[test]
    fn test_status_bits_cover_full_byte() {
        let s = [
            I8042_STR_OBF,
            I8042_STR_IBF,
            I8042_STR_SYSFLAG,
            I8042_STR_CMDDAT,
            I8042_STR_KBDINH,
            I8042_STR_AUXDATA,
            I8042_STR_TIMEOUT,
            I8042_STR_PARITY,
        ];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        assert_eq!(s.iter().fold(0u8, |a, &b| a | b), 0xFF);
    }

    #[test]
    fn test_commands_distinct() {
        let c = [
            I8042_CMD_CTL_RCTR,
            I8042_CMD_CTL_WCTR,
            I8042_CMD_CTL_TEST,
            I8042_CMD_KBD_TEST,
            I8042_CMD_KBD_DISABLE,
            I8042_CMD_KBD_ENABLE,
            I8042_CMD_AUX_TEST,
            I8042_CMD_AUX_DISABLE,
            I8042_CMD_AUX_ENABLE,
            I8042_CMD_AUX_SEND,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // ENABLE = DISABLE + 1 for both keyboard and aux.
        assert_eq!(I8042_CMD_KBD_ENABLE, I8042_CMD_KBD_DISABLE + 1);
        assert_eq!(I8042_CMD_AUX_ENABLE, I8042_CMD_AUX_DISABLE + 1);
    }

    #[test]
    fn test_config_bits_distinct() {
        let c = [
            I8042_CTR_KBDINT,
            I8042_CTR_AUXINT,
            I8042_CTR_SYSFLAG,
            I8042_CTR_KBDDIS,
            I8042_CTR_AUXDIS,
            I8042_CTR_XLATE,
        ];
        for &b in &c {
            assert!(b.is_power_of_two());
        }
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_self_test_magic() {
        // Controller's "I'm OK" reply is the documented 0x55.
        assert_eq!(I8042_RET_CTL_TEST, 0x55);
    }
}
