//! `<linux/counter.h>` — Generic counter character device interface.
//!
//! The counter subsystem exposes hardware counters (quadrature encoders,
//! pulse counters, frequency counters) through /dev/counterN with a
//! poll/read protocol plus per-counter sysfs attributes.

// ---------------------------------------------------------------------------
// Device path
// ---------------------------------------------------------------------------

pub const COUNTER_DEV_PREFIX: &str = "/dev/counter";
pub const COUNTER_SYSFS_BUS: &str = "/sys/bus/counter";

// ---------------------------------------------------------------------------
// Count direction (struct counter_count_read_value::value field meaning)
// ---------------------------------------------------------------------------

pub const COUNTER_COUNT_DIRECTION_FORWARD: u8 = 0;
pub const COUNTER_COUNT_DIRECTION_BACKWARD: u8 = 1;

// ---------------------------------------------------------------------------
// Count modes
// ---------------------------------------------------------------------------

pub const COUNTER_COUNT_MODE_NORMAL: u8 = 0;
pub const COUNTER_COUNT_MODE_RANGE_LIMIT: u8 = 1;
pub const COUNTER_COUNT_MODE_NON_RECYCLE: u8 = 2;
pub const COUNTER_COUNT_MODE_MODULO_N: u8 = 3;

// ---------------------------------------------------------------------------
// Signal level
// ---------------------------------------------------------------------------

pub const COUNTER_SIGNAL_LEVEL_LOW: u8 = 0;
pub const COUNTER_SIGNAL_LEVEL_HIGH: u8 = 1;

// ---------------------------------------------------------------------------
// Function modes (quadrature, pulse-direction, increase, decrease)
// ---------------------------------------------------------------------------

pub const COUNTER_FUNCTION_INCREASE: u8 = 0;
pub const COUNTER_FUNCTION_DECREASE: u8 = 1;
pub const COUNTER_FUNCTION_PULSE_DIRECTION: u8 = 2;
pub const COUNTER_FUNCTION_QUADRATURE_X1_A: u8 = 3;
pub const COUNTER_FUNCTION_QUADRATURE_X1_B: u8 = 4;
pub const COUNTER_FUNCTION_QUADRATURE_X2_A: u8 = 5;
pub const COUNTER_FUNCTION_QUADRATURE_X2_B: u8 = 6;
pub const COUNTER_FUNCTION_QUADRATURE_X4: u8 = 7;

// ---------------------------------------------------------------------------
// Event types (counter_event::watch.event)
// ---------------------------------------------------------------------------

pub const COUNTER_EVENT_OVERFLOW: u8 = 0;
pub const COUNTER_EVENT_UNDERFLOW: u8 = 1;
pub const COUNTER_EVENT_OVERFLOW_UNDERFLOW: u8 = 2;
pub const COUNTER_EVENT_THRESHOLD: u8 = 3;
pub const COUNTER_EVENT_INDEX: u8 = 4;
pub const COUNTER_EVENT_CHANGE_OF_STATE: u8 = 5;
pub const COUNTER_EVENT_CAPTURE: u8 = 6;

// ---------------------------------------------------------------------------
// Component types (counter_component::type)
// ---------------------------------------------------------------------------

pub const COUNTER_COMPONENT_NONE: u8 = 0;
pub const COUNTER_COMPONENT_SIGNAL: u8 = 1;
pub const COUNTER_COMPONENT_COUNT: u8 = 2;
pub const COUNTER_COMPONENT_FUNCTION: u8 = 3;
pub const COUNTER_COMPONENT_SYNAPSE_ACTION: u8 = 4;
pub const COUNTER_COMPONENT_EXTENSION: u8 = 5;

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

pub const COUNTER_SCOPE_DEVICE: u8 = 0;
pub const COUNTER_SCOPE_SIGNAL: u8 = 1;
pub const COUNTER_SCOPE_COUNT: u8 = 2;

// ---------------------------------------------------------------------------
// Ioctl numbers
// ---------------------------------------------------------------------------

pub const COUNTER_IOC_MAGIC: u8 = 0x3E;
pub const COUNTER_ADD_WATCH_IOCTL_NR: u8 = 0x60;
pub const COUNTER_ENABLE_EVENTS_IOCTL_NR: u8 = 0x61;
pub const COUNTER_DISABLE_EVENTS_IOCTL_NR: u8 = 0x62;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_paths_well_formed() {
        assert!(COUNTER_DEV_PREFIX.starts_with("/dev/"));
        assert!(COUNTER_SYSFS_BUS.starts_with("/sys/bus/"));
    }

    #[test]
    fn test_direction_binary() {
        assert_eq!(COUNTER_COUNT_DIRECTION_FORWARD, 0);
        assert_eq!(COUNTER_COUNT_DIRECTION_BACKWARD, 1);
    }

    #[test]
    fn test_count_modes_dense_0_to_3() {
        let m = [
            COUNTER_COUNT_MODE_NORMAL,
            COUNTER_COUNT_MODE_RANGE_LIMIT,
            COUNTER_COUNT_MODE_NON_RECYCLE,
            COUNTER_COUNT_MODE_MODULO_N,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_signal_levels_binary() {
        assert_eq!(COUNTER_SIGNAL_LEVEL_LOW, 0);
        assert_eq!(COUNTER_SIGNAL_LEVEL_HIGH, 1);
    }

    #[test]
    fn test_functions_dense_0_to_7() {
        let f = [
            COUNTER_FUNCTION_INCREASE,
            COUNTER_FUNCTION_DECREASE,
            COUNTER_FUNCTION_PULSE_DIRECTION,
            COUNTER_FUNCTION_QUADRATURE_X1_A,
            COUNTER_FUNCTION_QUADRATURE_X1_B,
            COUNTER_FUNCTION_QUADRATURE_X2_A,
            COUNTER_FUNCTION_QUADRATURE_X2_B,
            COUNTER_FUNCTION_QUADRATURE_X4,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_events_dense_0_to_6() {
        let e = [
            COUNTER_EVENT_OVERFLOW,
            COUNTER_EVENT_UNDERFLOW,
            COUNTER_EVENT_OVERFLOW_UNDERFLOW,
            COUNTER_EVENT_THRESHOLD,
            COUNTER_EVENT_INDEX,
            COUNTER_EVENT_CHANGE_OF_STATE,
            COUNTER_EVENT_CAPTURE,
        ];
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_components_dense_0_to_5() {
        let c = [
            COUNTER_COMPONENT_NONE,
            COUNTER_COMPONENT_SIGNAL,
            COUNTER_COMPONENT_COUNT,
            COUNTER_COMPONENT_FUNCTION,
            COUNTER_COMPONENT_SYNAPSE_ACTION,
            COUNTER_COMPONENT_EXTENSION,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_scopes_dense_0_to_2() {
        assert_eq!(COUNTER_SCOPE_DEVICE, 0);
        assert_eq!(COUNTER_SCOPE_SIGNAL, 1);
        assert_eq!(COUNTER_SCOPE_COUNT, 2);
    }

    #[test]
    fn test_ioctl_magic_and_nrs() {
        // '>' is 0x3E in ASCII.
        assert_eq!(COUNTER_IOC_MAGIC, b'>');
        assert_eq!(COUNTER_ADD_WATCH_IOCTL_NR, 0x60);
        assert_eq!(COUNTER_ENABLE_EVENTS_IOCTL_NR, 0x61);
        assert_eq!(COUNTER_DISABLE_EVENTS_IOCTL_NR, 0x62);
    }
}
