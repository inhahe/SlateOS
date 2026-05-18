//! `<linux/gpio.h>` (flag subset) — GPIO line request and configuration flags.
//!
//! GPIO lines are requested from userspace via the chardev interface
//! (`/dev/gpiochipN`). Request flags control the line's direction
//! (input/output), drive mode (push-pull, open-drain), bias (pull-up,
//! pull-down), and active level (active-high, active-low).

// ---------------------------------------------------------------------------
// GPIO line flags (v1 API, GPIOHANDLE_REQUEST_*)
// ---------------------------------------------------------------------------

/// Line is an input.
pub const GPIOHANDLE_REQUEST_INPUT: u32 = 1 << 0;
/// Line is an output.
pub const GPIOHANDLE_REQUEST_OUTPUT: u32 = 1 << 1;
/// Line is active-low (inverted logic).
pub const GPIOHANDLE_REQUEST_ACTIVE_LOW: u32 = 1 << 2;
/// Line uses open-drain drive.
pub const GPIOHANDLE_REQUEST_OPEN_DRAIN: u32 = 1 << 3;
/// Line uses open-source drive.
pub const GPIOHANDLE_REQUEST_OPEN_SOURCE: u32 = 1 << 4;
/// Line has internal pull-up bias.
pub const GPIOHANDLE_REQUEST_BIAS_PULL_UP: u32 = 1 << 5;
/// Line has internal pull-down bias.
pub const GPIOHANDLE_REQUEST_BIAS_PULL_DOWN: u32 = 1 << 6;
/// Line has bias disabled (floating).
pub const GPIOHANDLE_REQUEST_BIAS_DISABLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// GPIO line info flags (GPIOLINE_FLAG_*)
// ---------------------------------------------------------------------------

/// Line is in use by the kernel.
pub const GPIOLINE_FLAG_KERNEL: u32 = 1 << 0;
/// Line is an output.
pub const GPIOLINE_FLAG_IS_OUT: u32 = 1 << 1;
/// Line is active-low.
pub const GPIOLINE_FLAG_ACTIVE_LOW: u32 = 1 << 2;
/// Line is open-drain.
pub const GPIOLINE_FLAG_OPEN_DRAIN: u32 = 1 << 3;
/// Line is open-source.
pub const GPIOLINE_FLAG_OPEN_SOURCE: u32 = 1 << 4;
/// Line has pull-up bias.
pub const GPIOLINE_FLAG_BIAS_PULL_UP: u32 = 1 << 5;
/// Line has pull-down bias.
pub const GPIOLINE_FLAG_BIAS_PULL_DOWN: u32 = 1 << 6;
/// Line has bias disabled.
pub const GPIOLINE_FLAG_BIAS_DISABLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_flags_no_overlap() {
        let flags = [
            GPIOHANDLE_REQUEST_INPUT, GPIOHANDLE_REQUEST_OUTPUT,
            GPIOHANDLE_REQUEST_ACTIVE_LOW, GPIOHANDLE_REQUEST_OPEN_DRAIN,
            GPIOHANDLE_REQUEST_OPEN_SOURCE,
            GPIOHANDLE_REQUEST_BIAS_PULL_UP, GPIOHANDLE_REQUEST_BIAS_PULL_DOWN,
            GPIOHANDLE_REQUEST_BIAS_DISABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_info_flags_no_overlap() {
        let flags = [
            GPIOLINE_FLAG_KERNEL, GPIOLINE_FLAG_IS_OUT,
            GPIOLINE_FLAG_ACTIVE_LOW, GPIOLINE_FLAG_OPEN_DRAIN,
            GPIOLINE_FLAG_OPEN_SOURCE,
            GPIOLINE_FLAG_BIAS_PULL_UP, GPIOLINE_FLAG_BIAS_PULL_DOWN,
            GPIOLINE_FLAG_BIAS_DISABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
