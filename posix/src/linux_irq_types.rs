//! `<linux/irq.h>` — Interrupt type and flag constants.
//!
//! The Linux IRQ subsystem provides a generic abstraction for
//! interrupt handling across architectures. Interrupts can be
//! edge or level triggered, shared or exclusive, and can be
//! configured for various hardware behaviors.

// ---------------------------------------------------------------------------
// IRQ trigger types (IRQF_TRIGGER_*)
// ---------------------------------------------------------------------------

/// Rising edge triggered.
pub const IRQF_TRIGGER_RISING: u32 = 1 << 0;
/// Falling edge triggered.
pub const IRQF_TRIGGER_FALLING: u32 = 1 << 1;
/// Active high level triggered.
pub const IRQF_TRIGGER_HIGH: u32 = 1 << 2;
/// Active low level triggered.
pub const IRQF_TRIGGER_LOW: u32 = 1 << 3;
/// Mask for all trigger type bits.
pub const IRQF_TRIGGER_MASK: u32 = 0x0F;

// ---------------------------------------------------------------------------
// IRQ handler flags (IRQF_*)
// ---------------------------------------------------------------------------

/// IRQ can be shared between devices.
pub const IRQF_SHARED: u32 = 1 << 7;
/// IRQ is for timer/clock.
pub const IRQF_TIMER: u32 = 1 << 9;
/// Disable balancing for this IRQ.
pub const IRQF_NOBALANCING: u32 = 1 << 11;
/// IRQ is per-CPU.
pub const IRQF_PERCPU: u32 = 1 << 10;
/// Don't create /proc/irq entry.
pub const IRQF_NO_SUSPEND: u32 = 1 << 14;
/// IRQ safe for forced threading.
pub const IRQF_FORCE_RESUME: u32 = 1 << 15;
/// Oneshot — IRQ not re-enabled after hardirq handler.
pub const IRQF_ONESHOT: u32 = 1 << 13;
/// Don't disable IRQ during handler.
pub const IRQF_NO_THREAD: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// IRQ flow types
// ---------------------------------------------------------------------------

/// Level flow handler.
pub const IRQ_TYPE_LEVEL: u8 = 0;
/// Edge flow handler.
pub const IRQ_TYPE_EDGE: u8 = 1;
/// Simple flow handler.
pub const IRQ_TYPE_SIMPLE: u8 = 2;
/// Per-CPU flow handler.
pub const IRQ_TYPE_PERCPU: u8 = 3;
/// Fasteoi flow handler.
pub const IRQ_TYPE_FASTEOI: u8 = 4;

// ---------------------------------------------------------------------------
// IRQ return values
// ---------------------------------------------------------------------------

/// IRQ not handled (not for this device).
pub const IRQ_NONE: u8 = 0;
/// IRQ handled.
pub const IRQ_HANDLED: u8 = 1;
/// IRQ handled, wake thread.
pub const IRQ_WAKE_THREAD: u8 = 2;

// ---------------------------------------------------------------------------
// IRQ chip flags
// ---------------------------------------------------------------------------

/// Chip can set affinity.
pub const IRQCHIP_SET_TYPE_MASKED: u32 = 1 << 0;
/// Skip set_wake.
pub const IRQCHIP_SKIP_SET_WAKE: u32 = 1 << 1;
/// Support nested threading.
pub const IRQCHIP_ONESHOT_SAFE: u32 = 1 << 2;
/// EOI on unmask.
pub const IRQCHIP_EOI_IF_HANDLED: u32 = 1 << 3;
/// Threaded EOI.
pub const IRQCHIP_EOI_THREADED: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_types_no_overlap() {
        let triggers = [
            IRQF_TRIGGER_RISING, IRQF_TRIGGER_FALLING,
            IRQF_TRIGGER_HIGH, IRQF_TRIGGER_LOW,
        ];
        for i in 0..triggers.len() {
            for j in (i + 1)..triggers.len() {
                assert_eq!(triggers[i] & triggers[j], 0);
            }
        }
    }

    #[test]
    fn test_trigger_mask() {
        assert_eq!(
            IRQF_TRIGGER_MASK,
            IRQF_TRIGGER_RISING | IRQF_TRIGGER_FALLING |
            IRQF_TRIGGER_HIGH | IRQF_TRIGGER_LOW
        );
    }

    #[test]
    fn test_handler_flags_no_overlap_selected() {
        // Check a subset that should not overlap
        let flags = [IRQF_SHARED, IRQF_TIMER, IRQF_PERCPU, IRQF_ONESHOT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_return_values_distinct() {
        let vals = [IRQ_NONE, IRQ_HANDLED, IRQ_WAKE_THREAD];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_flow_types_distinct() {
        let types = [
            IRQ_TYPE_LEVEL, IRQ_TYPE_EDGE, IRQ_TYPE_SIMPLE,
            IRQ_TYPE_PERCPU, IRQ_TYPE_FASTEOI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_chip_flags_no_overlap() {
        let flags = [
            IRQCHIP_SET_TYPE_MASKED, IRQCHIP_SKIP_SET_WAKE,
            IRQCHIP_ONESHOT_SAFE, IRQCHIP_EOI_IF_HANDLED,
            IRQCHIP_EOI_THREADED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
