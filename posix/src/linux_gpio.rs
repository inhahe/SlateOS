//! `<linux/gpio.h>` — GPIO character device interface.
//!
//! The GPIO character device (`/dev/gpiochipN`) provides a modern
//! alternative to the deprecated sysfs GPIO interface. Used by
//! libgpiod and applications needing GPIO access.

// ---------------------------------------------------------------------------
// GPIO ioctl commands
// ---------------------------------------------------------------------------

/// Get chip info.
pub const GPIO_GET_CHIPINFO_IOCTL: u64 = 0x8044B401;
/// Get line info (v2).
pub const GPIO_V2_GET_LINEINFO_IOCTL: u64 = 0xC100B405;
/// Request lines (v2).
pub const GPIO_V2_GET_LINE_IOCTL: u64 = 0xC250B407;
/// Get line values (v2).
pub const GPIO_V2_LINE_GET_VALUES_IOCTL: u64 = 0xC010B40E;
/// Set line values (v2).
pub const GPIO_V2_LINE_SET_VALUES_IOCTL: u64 = 0xC010B40D;
/// Set line config (v2).
pub const GPIO_V2_LINE_SET_CONFIG_IOCTL: u64 = 0xC110B40F;

// ---------------------------------------------------------------------------
// GPIO line flags (v2)
// ---------------------------------------------------------------------------

/// Line is used/requested.
pub const GPIO_V2_LINE_FLAG_USED: u64 = 1 << 0;
/// Active low.
pub const GPIO_V2_LINE_FLAG_ACTIVE_LOW: u64 = 1 << 1;
/// Input direction.
pub const GPIO_V2_LINE_FLAG_INPUT: u64 = 1 << 2;
/// Output direction.
pub const GPIO_V2_LINE_FLAG_OUTPUT: u64 = 1 << 3;
/// Rising edge event detection.
pub const GPIO_V2_LINE_FLAG_EDGE_RISING: u64 = 1 << 4;
/// Falling edge event detection.
pub const GPIO_V2_LINE_FLAG_EDGE_FALLING: u64 = 1 << 5;
/// Open drain output.
pub const GPIO_V2_LINE_FLAG_OPEN_DRAIN: u64 = 1 << 6;
/// Open source output.
pub const GPIO_V2_LINE_FLAG_OPEN_SOURCE: u64 = 1 << 7;
/// Pull-up bias.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_UP: u64 = 1 << 8;
/// Pull-down bias.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
/// Disable bias.
pub const GPIO_V2_LINE_FLAG_BIAS_DISABLED: u64 = 1 << 10;
/// Event clock: realtime.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME: u64 = 1 << 11;
/// Event clock: hardware timestamp engine.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// GPIO event types
// ---------------------------------------------------------------------------

/// Rising edge event.
pub const GPIO_V2_LINE_EVENT_RISING_EDGE: u32 = 1;
/// Falling edge event.
pub const GPIO_V2_LINE_EVENT_FALLING_EDGE: u32 = 2;

// ---------------------------------------------------------------------------
// GPIO constants
// ---------------------------------------------------------------------------

/// Maximum number of lines per request.
pub const GPIO_V2_LINES_MAX: usize = 64;
/// Maximum chip name length.
pub const GPIO_MAX_NAME_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            GPIO_GET_CHIPINFO_IOCTL,
            GPIO_V2_GET_LINEINFO_IOCTL,
            GPIO_V2_GET_LINE_IOCTL,
            GPIO_V2_LINE_GET_VALUES_IOCTL,
            GPIO_V2_LINE_SET_VALUES_IOCTL,
            GPIO_V2_LINE_SET_CONFIG_IOCTL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            GPIO_V2_LINE_FLAG_USED,
            GPIO_V2_LINE_FLAG_ACTIVE_LOW,
            GPIO_V2_LINE_FLAG_INPUT,
            GPIO_V2_LINE_FLAG_OUTPUT,
            GPIO_V2_LINE_FLAG_EDGE_RISING,
            GPIO_V2_LINE_FLAG_EDGE_FALLING,
            GPIO_V2_LINE_FLAG_OPEN_DRAIN,
            GPIO_V2_LINE_FLAG_OPEN_SOURCE,
            GPIO_V2_LINE_FLAG_BIAS_PULL_UP,
            GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN,
            GPIO_V2_LINE_FLAG_BIAS_DISABLED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_event_types() {
        assert_eq!(GPIO_V2_LINE_EVENT_RISING_EDGE, 1);
        assert_eq!(GPIO_V2_LINE_EVENT_FALLING_EDGE, 2);
    }

    #[test]
    fn test_constants() {
        assert_eq!(GPIO_V2_LINES_MAX, 64);
        assert_eq!(GPIO_MAX_NAME_SIZE, 32);
    }
}
