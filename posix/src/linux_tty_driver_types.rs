//! `<linux/tty_driver.h>` — TTY driver framework constants.
//!
//! TTY drivers are the hardware-facing component of the TTY layer.
//! Each driver manages one or more TTY ports (serial UARTs, USB
//! serial adapters, virtual consoles, PTY masters, etc.). The driver
//! provides open/close/write/ioctl operations while the TTY core
//! handles common buffering, locking, and line discipline integration.
//! Drivers register with tty_register_driver() and receive callbacks
//! when userspace interacts with their devices.

// ---------------------------------------------------------------------------
// TTY driver types
// ---------------------------------------------------------------------------

/// Serial port driver.
pub const TTY_DRIVER_TYPE_SERIAL: u32 = 1;
/// Console driver (VT, framebuffer).
pub const TTY_DRIVER_TYPE_CONSOLE: u32 = 2;
/// PTY master driver.
pub const TTY_DRIVER_TYPE_PTY: u32 = 3;
/// System console (printk output device).
pub const TTY_DRIVER_TYPE_SYSTEM: u32 = 4;

// ---------------------------------------------------------------------------
// TTY driver subtypes
// ---------------------------------------------------------------------------

/// Normal PTY pair.
pub const PTY_TYPE_MASTER: u32 = 1;
/// PTY slave.
pub const PTY_TYPE_SLAVE: u32 = 2;
/// System console subtype.
pub const SYSTEM_TYPE_CONSOLE: u32 = 1;
/// System TTY subtype (/dev/tty).
pub const SYSTEM_TYPE_TTY: u32 = 2;
/// Serial normal subtype.
pub const SERIAL_TYPE_NORMAL: u32 = 1;
/// Serial callout subtype (deprecated).
pub const SERIAL_TYPE_CALLOUT: u32 = 2;

// ---------------------------------------------------------------------------
// TTY driver flags
// ---------------------------------------------------------------------------

/// Driver is installed (has ports allocated).
pub const TTY_DRIVER_INSTALLED: u32 = 0x0001;
/// Driver does not require an open count (always available).
pub const TTY_DRIVER_RESET_TERMIOS: u32 = 0x0002;
/// Driver uses devpts filesystem (Unix98 PTYs).
pub const TTY_DRIVER_DEVPTS_MEM: u32 = 0x0008;
/// Driver handles its own hardware flow control.
pub const TTY_DRIVER_HARDWARE_BREAK: u32 = 0x0010;
/// Driver is dynamically allocated.
pub const TTY_DRIVER_DYNAMIC_DEV: u32 = 0x0020;
/// Driver manages its own alloc/free.
pub const TTY_DRIVER_DYNAMIC_ALLOC: u32 = 0x0040;
/// Driver's TTYs don't have an associated device.
pub const TTY_DRIVER_UNNUMBERED_NODE: u32 = 0x0080;

// ---------------------------------------------------------------------------
// TTY port flags
// ---------------------------------------------------------------------------

/// Port is active (device open).
pub const TTY_PORT_ACTIVE: u32 = 0x01;
/// Port hardware is initialized.
pub const TTY_PORT_INITIALIZED: u32 = 0x02;
/// Port is being closed.
pub const TTY_PORT_CLOSING: u32 = 0x04;
/// Port carrier detect is asserted.
pub const TTY_PORT_CTS_FLOW: u32 = 0x08;
/// Port has console attached.
pub const TTY_PORT_CONSOLE: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_types_distinct() {
        let types = [
            TTY_DRIVER_TYPE_SERIAL, TTY_DRIVER_TYPE_CONSOLE,
            TTY_DRIVER_TYPE_PTY, TTY_DRIVER_TYPE_SYSTEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_driver_flags_no_overlap() {
        let flags = [
            TTY_DRIVER_INSTALLED, TTY_DRIVER_RESET_TERMIOS,
            TTY_DRIVER_DEVPTS_MEM, TTY_DRIVER_HARDWARE_BREAK,
            TTY_DRIVER_DYNAMIC_DEV, TTY_DRIVER_DYNAMIC_ALLOC,
            TTY_DRIVER_UNNUMBERED_NODE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_port_flags_no_overlap() {
        let flags = [
            TTY_PORT_ACTIVE, TTY_PORT_INITIALIZED,
            TTY_PORT_CLOSING, TTY_PORT_CTS_FLOW, TTY_PORT_CONSOLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
