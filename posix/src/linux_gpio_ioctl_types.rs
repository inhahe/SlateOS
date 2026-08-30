//! `<linux/gpio.h>` (ioctl subset) — GPIO chardev ioctl request numbers.
//!
//! The GPIO character device (`/dev/gpiochipN`) supports ioctls for
//! querying chip information, line information, requesting lines for
//! I/O, and setting up event monitoring. The v2 API adds multi-line
//! and line configuration change support.

// ---------------------------------------------------------------------------
// GPIO ioctl magic number
// ---------------------------------------------------------------------------

/// GPIO ioctl magic byte (0xB4).
pub const GPIO_IOCTL_MAGIC: u8 = 0xB4;

// ---------------------------------------------------------------------------
// V1 ioctl request numbers (offset within magic)
// ---------------------------------------------------------------------------

/// Get chip info (struct gpiochip_info).
pub const GPIO_GET_CHIPINFO_IOCTL_NR: u32 = 0x01;
/// Get line info (struct gpioline_info).
pub const GPIO_GET_LINEINFO_IOCTL_NR: u32 = 0x02;
/// Request line handle (struct gpiohandle_request).
pub const GPIO_GET_LINEHANDLE_IOCTL_NR: u32 = 0x03;
/// Request line events (struct gpioevent_request).
pub const GPIO_GET_LINEEVENT_IOCTL_NR: u32 = 0x04;

// ---------------------------------------------------------------------------
// Handle ioctls (on the fd returned by GET_LINEHANDLE)
// ---------------------------------------------------------------------------

/// Get line values.
pub const GPIOHANDLE_GET_LINE_VALUES_IOCTL_NR: u32 = 0x08;
/// Set line values.
pub const GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR: u32 = 0x09;
/// Set line config (change direction, flags).
pub const GPIOHANDLE_SET_CONFIG_IOCTL_NR: u32 = 0x0A;

// ---------------------------------------------------------------------------
// V2 ioctl request numbers
// ---------------------------------------------------------------------------

/// Get line info (v2 struct).
pub const GPIO_V2_GET_LINEINFO_IOCTL_NR: u32 = 0x05;
/// Watch line info changes (v2).
pub const GPIO_V2_GET_LINEINFO_WATCH_IOCTL_NR: u32 = 0x06;
/// Request lines (v2, multi-line).
pub const GPIO_V2_GET_LINE_IOCTL_NR: u32 = 0x07;
/// Get line values (v2).
pub const GPIO_V2_LINE_GET_VALUES_IOCTL_NR: u32 = 0x0E;
/// Set line values (v2).
pub const GPIO_V2_LINE_SET_VALUES_IOCTL_NR: u32 = 0x0F;
/// Set line config (v2).
pub const GPIO_V2_LINE_SET_CONFIG_IOCTL_NR: u32 = 0x0D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(GPIO_IOCTL_MAGIC, 0xB4);
    }

    #[test]
    fn test_v1_ioctls_distinct() {
        let nrs = [
            GPIO_GET_CHIPINFO_IOCTL_NR,
            GPIO_GET_LINEINFO_IOCTL_NR,
            GPIO_GET_LINEHANDLE_IOCTL_NR,
            GPIO_GET_LINEEVENT_IOCTL_NR,
        ];
        for i in 0..nrs.len() {
            for j in (i + 1)..nrs.len() {
                assert_ne!(nrs[i], nrs[j]);
            }
        }
    }

    #[test]
    fn test_v2_ioctls_distinct() {
        let nrs = [
            GPIO_V2_GET_LINEINFO_IOCTL_NR,
            GPIO_V2_GET_LINEINFO_WATCH_IOCTL_NR,
            GPIO_V2_GET_LINE_IOCTL_NR,
            GPIO_V2_LINE_GET_VALUES_IOCTL_NR,
            GPIO_V2_LINE_SET_VALUES_IOCTL_NR,
            GPIO_V2_LINE_SET_CONFIG_IOCTL_NR,
        ];
        for i in 0..nrs.len() {
            for j in (i + 1)..nrs.len() {
                assert_ne!(nrs[i], nrs[j]);
            }
        }
    }

    #[test]
    fn test_handle_ioctls_distinct() {
        assert_ne!(
            GPIOHANDLE_GET_LINE_VALUES_IOCTL_NR,
            GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR
        );
        assert_ne!(
            GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR,
            GPIOHANDLE_SET_CONFIG_IOCTL_NR
        );
    }
}
