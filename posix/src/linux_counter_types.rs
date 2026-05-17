//! `<linux/counter.h>` — Counter subsystem constants.
//!
//! The Linux counter subsystem provides a generic interface for
//! hardware counter devices (quadrature encoders, frequency counters,
//! pulse counters, tachometers). Counters track events via signals
//! and report count values. Used in industrial automation, motor
//! control, and precision measurement applications.

// ---------------------------------------------------------------------------
// Counter scope
// ---------------------------------------------------------------------------

/// Device-level scope.
pub const COUNTER_SCOPE_DEVICE: u32 = 0;
/// Signal-level scope.
pub const COUNTER_SCOPE_SIGNAL: u32 = 1;
/// Count-level scope.
pub const COUNTER_SCOPE_COUNT: u32 = 2;

// ---------------------------------------------------------------------------
// Counter count direction
// ---------------------------------------------------------------------------

/// Counting forward (incrementing).
pub const COUNTER_COUNT_DIRECTION_FORWARD: u32 = 0;
/// Counting backward (decrementing).
pub const COUNTER_COUNT_DIRECTION_BACKWARD: u32 = 1;

// ---------------------------------------------------------------------------
// Counter count mode
// ---------------------------------------------------------------------------

/// Normal counting (wrap at max/min).
pub const COUNTER_COUNT_MODE_NORMAL: u32 = 0;
/// Range limit (saturate at max/min).
pub const COUNTER_COUNT_MODE_RANGE_LIMIT: u32 = 1;
/// Non-recycle (stop at max/min).
pub const COUNTER_COUNT_MODE_NON_RECYCLE: u32 = 2;
/// Modulo-N counting.
pub const COUNTER_COUNT_MODE_MODULO_N: u32 = 3;

// ---------------------------------------------------------------------------
// Counter signal level
// ---------------------------------------------------------------------------

/// Signal is low.
pub const COUNTER_SIGNAL_LEVEL_LOW: u32 = 0;
/// Signal is high.
pub const COUNTER_SIGNAL_LEVEL_HIGH: u32 = 1;

// ---------------------------------------------------------------------------
// Counter function (counting mode)
// ---------------------------------------------------------------------------

/// Increase count on rising edge.
pub const COUNTER_FUNCTION_INCREASE: u32 = 0;
/// Decrease count on rising edge.
pub const COUNTER_FUNCTION_DECREASE: u32 = 1;
/// Quadrature x1 (count on A edge).
pub const COUNTER_FUNCTION_QUADRATURE_X1_A: u32 = 4;
/// Quadrature x1 (count on B edge).
pub const COUNTER_FUNCTION_QUADRATURE_X1_B: u32 = 5;
/// Quadrature x2 (count on A edges).
pub const COUNTER_FUNCTION_QUADRATURE_X2_A: u32 = 6;
/// Quadrature x2 (count on B edges).
pub const COUNTER_FUNCTION_QUADRATURE_X2_B: u32 = 7;
/// Quadrature x4 (count on all edges).
pub const COUNTER_FUNCTION_QUADRATURE_X4: u32 = 8;
/// Pulse-direction mode.
pub const COUNTER_FUNCTION_PULSE_DIRECTION: u32 = 9;

// ---------------------------------------------------------------------------
// Counter event types
// ---------------------------------------------------------------------------

/// Overflow event (counter reached maximum).
pub const COUNTER_EVENT_OVERFLOW: u32 = 0;
/// Underflow event (counter reached minimum).
pub const COUNTER_EVENT_UNDERFLOW: u32 = 1;
/// Threshold event.
pub const COUNTER_EVENT_THRESHOLD: u32 = 2;
/// Index/marker signal detected.
pub const COUNTER_EVENT_INDEX: u32 = 3;
/// Change of state.
pub const COUNTER_EVENT_CHANGE_OF_STATE: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scopes_distinct() {
        let scopes = [
            COUNTER_SCOPE_DEVICE, COUNTER_SCOPE_SIGNAL,
            COUNTER_SCOPE_COUNT,
        ];
        for i in 0..scopes.len() {
            for j in (i + 1)..scopes.len() {
                assert_ne!(scopes[i], scopes[j]);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(
            COUNTER_COUNT_DIRECTION_FORWARD,
            COUNTER_COUNT_DIRECTION_BACKWARD
        );
    }

    #[test]
    fn test_count_modes_distinct() {
        let modes = [
            COUNTER_COUNT_MODE_NORMAL, COUNTER_COUNT_MODE_RANGE_LIMIT,
            COUNTER_COUNT_MODE_NON_RECYCLE, COUNTER_COUNT_MODE_MODULO_N,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_functions_distinct() {
        let funcs = [
            COUNTER_FUNCTION_INCREASE, COUNTER_FUNCTION_DECREASE,
            COUNTER_FUNCTION_QUADRATURE_X1_A, COUNTER_FUNCTION_QUADRATURE_X1_B,
            COUNTER_FUNCTION_QUADRATURE_X2_A, COUNTER_FUNCTION_QUADRATURE_X2_B,
            COUNTER_FUNCTION_QUADRATURE_X4, COUNTER_FUNCTION_PULSE_DIRECTION,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            COUNTER_EVENT_OVERFLOW, COUNTER_EVENT_UNDERFLOW,
            COUNTER_EVENT_THRESHOLD, COUNTER_EVENT_INDEX,
            COUNTER_EVENT_CHANGE_OF_STATE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
