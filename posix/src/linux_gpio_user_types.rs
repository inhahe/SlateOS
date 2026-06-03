//! `<linux/gpio.h>` (v1 ABI) — `/dev/gpiochipN` ioctls and labels.
//!
//! Although libgpiod has migrated to the v2 `<linux/gpio.h>` API,
//! lots of userspace (Yocto recipes, embedded test scripts,
//! pre-libgpiod-2.0 binaries) still uses the v1 character-device
//! ioctls. The constants below cover that v1 surface — chip info,
//! line info, line handles, and line events.

// ---------------------------------------------------------------------------
// Field-length constants
// ---------------------------------------------------------------------------

/// Maximum line label length (excluding NUL).
pub const GPIO_MAX_NAME_SIZE: u32 = 32;
/// Maximum number of lines per chip request.
pub const GPIOHANDLES_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// Chip-level info (struct gpiochip_info.flags)
// ---------------------------------------------------------------------------

/// Chip is locked open by another consumer.
pub const GPIOCHIP_INFO_FLAG_BUSY: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Line info / request flags (struct gpioline_info.flags &
// gpiohandle_request.flags)
// ---------------------------------------------------------------------------

/// Line is reserved by the kernel or a consumer.
pub const GPIOLINE_FLAG_KERNEL: u32 = 1 << 0;
/// Line is configured as output.
pub const GPIOLINE_FLAG_IS_OUT: u32 = 1 << 1;
/// Line is active-low.
pub const GPIOLINE_FLAG_ACTIVE_LOW: u32 = 1 << 2;
/// Line is open-drain.
pub const GPIOLINE_FLAG_OPEN_DRAIN: u32 = 1 << 3;
/// Line is open-source.
pub const GPIOLINE_FLAG_OPEN_SOURCE: u32 = 1 << 4;
/// Line has bias-pull-up enabled.
pub const GPIOLINE_FLAG_BIAS_PULL_UP: u32 = 1 << 5;
/// Line has bias-pull-down enabled.
pub const GPIOLINE_FLAG_BIAS_PULL_DOWN: u32 = 1 << 6;
/// Line bias is explicitly disabled.
pub const GPIOLINE_FLAG_BIAS_DISABLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Event-request flags (struct gpioevent_request.eventflags)
// ---------------------------------------------------------------------------

/// Watch rising edges.
pub const GPIOEVENT_REQUEST_RISING_EDGE: u32 = 1 << 0;
/// Watch falling edges.
pub const GPIOEVENT_REQUEST_FALLING_EDGE: u32 = 1 << 1;
/// Watch both edges.
pub const GPIOEVENT_REQUEST_BOTH_EDGES: u32 =
    GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE;

// ---------------------------------------------------------------------------
// Event-fd payload event ids (struct gpioevent_data.id)
// ---------------------------------------------------------------------------

/// Rising-edge event.
pub const GPIOEVENT_EVENT_RISING_EDGE: u32 = 0x01;
/// Falling-edge event.
pub const GPIOEVENT_EVENT_FALLING_EDGE: u32 = 0x02;

// ---------------------------------------------------------------------------
// ioctl numbers (group 0xb4 — 'GPIO')
// ---------------------------------------------------------------------------

/// `GPIO_GET_CHIPINFO_IOCTL` — query chip info.
pub const GPIO_GET_CHIPINFO_IOCTL: u32 = 0x8044_b401;
/// `GPIO_GET_LINEINFO_IOCTL` — query one line.
pub const GPIO_GET_LINEINFO_IOCTL: u32 = 0xc048_b402;
/// `GPIO_GET_LINEHANDLE_IOCTL` — request a handle to multiple lines.
pub const GPIO_GET_LINEHANDLE_IOCTL: u32 = 0xc16c_b403;
/// `GPIO_GET_LINEEVENT_IOCTL` — request an event fd for a line.
pub const GPIO_GET_LINEEVENT_IOCTL: u32 = 0xc030_b404;
/// `GPIOHANDLE_GET_LINE_VALUES_IOCTL` — read current values.
pub const GPIOHANDLE_GET_LINE_VALUES_IOCTL: u32 = 0xc040_b408;
/// `GPIOHANDLE_SET_LINE_VALUES_IOCTL` — drive new values.
pub const GPIOHANDLE_SET_LINE_VALUES_IOCTL: u32 = 0xc040_b409;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_size_and_max_lines() {
        // 32-byte labels and 64-line requests are the v1 ABI defaults.
        assert_eq!(GPIO_MAX_NAME_SIZE, 32);
        assert_eq!(GPIOHANDLES_MAX, 64);
        assert!(GPIO_MAX_NAME_SIZE.is_power_of_two());
        assert!(GPIOHANDLES_MAX.is_power_of_two());
    }

    #[test]
    fn test_line_flags_distinct_pow2() {
        let f = [
            GPIOLINE_FLAG_KERNEL,
            GPIOLINE_FLAG_IS_OUT,
            GPIOLINE_FLAG_ACTIVE_LOW,
            GPIOLINE_FLAG_OPEN_DRAIN,
            GPIOLINE_FLAG_OPEN_SOURCE,
            GPIOLINE_FLAG_BIAS_PULL_UP,
            GPIOLINE_FLAG_BIAS_PULL_DOWN,
            GPIOLINE_FLAG_BIAS_DISABLE,
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

    #[test]
    fn test_event_request_both_edges_is_or() {
        assert_eq!(
            GPIOEVENT_REQUEST_BOTH_EDGES,
            GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE
        );
        assert!(GPIOEVENT_REQUEST_RISING_EDGE.is_power_of_two());
        assert!(GPIOEVENT_REQUEST_FALLING_EDGE.is_power_of_two());
    }

    #[test]
    fn test_event_payload_ids_distinct() {
        assert_eq!(GPIOEVENT_EVENT_RISING_EDGE, 1);
        assert_eq!(GPIOEVENT_EVENT_FALLING_EDGE, 2);
        assert_ne!(GPIOEVENT_EVENT_RISING_EDGE, GPIOEVENT_EVENT_FALLING_EDGE);
    }

    #[test]
    fn test_ioctls_distinct_and_use_magic_0xb4() {
        let ops = [
            GPIO_GET_CHIPINFO_IOCTL,
            GPIO_GET_LINEINFO_IOCTL,
            GPIO_GET_LINEHANDLE_IOCTL,
            GPIO_GET_LINEEVENT_IOCTL,
            GPIOHANDLE_GET_LINE_VALUES_IOCTL,
            GPIOHANDLE_SET_LINE_VALUES_IOCTL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // GPIO ioctl group is 0xb4.
            assert_eq!((ops[i] >> 8) & 0xff, 0xb4);
        }
    }
}
