//! `<linux/component.h>` — Component framework constants.
//!
//! The component framework enables aggregate devices where a single
//! logical device is composed of multiple independent sub-devices
//! that probe asynchronously. For example, a DRM display device
//! might consist of a display controller, HDMI encoder, DSI panel,
//! and DP bridge — each probing independently. The component
//! framework waits for all components to be available before
//! binding the aggregate device.

// ---------------------------------------------------------------------------
// Component match types
// ---------------------------------------------------------------------------

/// Match by device (struct device pointer).
pub const COMPONENT_MATCH_DEVICE: u32 = 0;
/// Match by device tree node (of_node).
pub const COMPONENT_MATCH_OF_NODE: u32 = 1;
/// Match by ACPI device.
pub const COMPONENT_MATCH_ACPI: u32 = 2;
/// Match by custom compare function.
pub const COMPONENT_MATCH_CUSTOM: u32 = 3;

// ---------------------------------------------------------------------------
// Component states
// ---------------------------------------------------------------------------

/// Component registered (waiting for aggregate bind).
pub const COMPONENT_STATE_REGISTERED: u32 = 0;
/// Component bound (part of active aggregate).
pub const COMPONENT_STATE_BOUND: u32 = 1;
/// Component being unbound.
pub const COMPONENT_STATE_UNBINDING: u32 = 2;

// ---------------------------------------------------------------------------
// Aggregate device states
// ---------------------------------------------------------------------------

/// Aggregate waiting for all components.
pub const AGGREGATE_STATE_WAITING: u32 = 0;
/// All components available, binding in progress.
pub const AGGREGATE_STATE_BINDING: u32 = 1;
/// Aggregate fully bound and operational.
pub const AGGREGATE_STATE_BOUND: u32 = 2;
/// Aggregate unbinding (teardown).
pub const AGGREGATE_STATE_UNBINDING: u32 = 3;

// ---------------------------------------------------------------------------
// Component bind order
// ---------------------------------------------------------------------------

/// Bind in registration order (first registered, first bound).
pub const COMPONENT_ORDER_REGISTRATION: u32 = 0;
/// Bind in match order (order specified by aggregate master).
pub const COMPONENT_ORDER_MATCH: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_types_distinct() {
        let types = [
            COMPONENT_MATCH_DEVICE,
            COMPONENT_MATCH_OF_NODE,
            COMPONENT_MATCH_ACPI,
            COMPONENT_MATCH_CUSTOM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_component_states_distinct() {
        let states = [
            COMPONENT_STATE_REGISTERED,
            COMPONENT_STATE_BOUND,
            COMPONENT_STATE_UNBINDING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_aggregate_states_distinct() {
        let states = [
            AGGREGATE_STATE_WAITING,
            AGGREGATE_STATE_BINDING,
            AGGREGATE_STATE_BOUND,
            AGGREGATE_STATE_UNBINDING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_bind_orders_distinct() {
        assert_ne!(COMPONENT_ORDER_REGISTRATION, COMPONENT_ORDER_MATCH);
    }
}
