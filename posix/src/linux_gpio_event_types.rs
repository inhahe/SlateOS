//! `<linux/gpio.h>` (event subset) — GPIO event request and types.
//!
//! GPIO events report edge transitions (rising, falling, or both)
//! on input lines. Userspace requests events via the chardev
//! interface and reads them from a file descriptor. Each event
//! includes a high-resolution timestamp and the edge type.

// ---------------------------------------------------------------------------
// Event request flags (GPIOEVENT_REQUEST_*)
// ---------------------------------------------------------------------------

/// Report rising edge events (low → high).
pub const GPIOEVENT_REQUEST_RISING_EDGE: u32 = 1 << 0;
/// Report falling edge events (high → low).
pub const GPIOEVENT_REQUEST_FALLING_EDGE: u32 = 1 << 1;
/// Report both edges.
pub const GPIOEVENT_REQUEST_BOTH_EDGES: u32 =
    GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE;

// ---------------------------------------------------------------------------
// Event types (gpioevent_data.id)
// ---------------------------------------------------------------------------

/// Rising edge event occurred.
pub const GPIOEVENT_EVENT_RISING_EDGE: u32 = 0x01;
/// Falling edge event occurred.
pub const GPIOEVENT_EVENT_FALLING_EDGE: u32 = 0x02;

// ---------------------------------------------------------------------------
// GPIO event timestamp clock sources
// ---------------------------------------------------------------------------

/// Monotonic clock (CLOCK_MONOTONIC).
pub const GPIO_EVENT_CLOCK_MONOTONIC: u32 = 1;
/// Realtime clock (CLOCK_REALTIME).
pub const GPIO_EVENT_CLOCK_REALTIME: u32 = 2;
/// HTE (Hardware Timestamp Engine) clock.
pub const GPIO_EVENT_CLOCK_HTE: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_flags_composable() {
        assert_eq!(GPIOEVENT_REQUEST_BOTH_EDGES,
            GPIOEVENT_REQUEST_RISING_EDGE | GPIOEVENT_REQUEST_FALLING_EDGE);
    }

    #[test]
    fn test_request_flags_no_overlap() {
        assert!(GPIOEVENT_REQUEST_RISING_EDGE.is_power_of_two());
        assert!(GPIOEVENT_REQUEST_FALLING_EDGE.is_power_of_two());
        assert_eq!(GPIOEVENT_REQUEST_RISING_EDGE & GPIOEVENT_REQUEST_FALLING_EDGE, 0);
    }

    #[test]
    fn test_event_types_distinct() {
        assert_ne!(GPIOEVENT_EVENT_RISING_EDGE, GPIOEVENT_EVENT_FALLING_EDGE);
    }

    #[test]
    fn test_clock_sources_distinct() {
        let clocks = [
            GPIO_EVENT_CLOCK_MONOTONIC,
            GPIO_EVENT_CLOCK_REALTIME,
            GPIO_EVENT_CLOCK_HTE,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }
}
