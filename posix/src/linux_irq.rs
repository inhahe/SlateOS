//! `<linux/irq.h>` / `<linux/interrupt.h>` — IRQ management constants.
//!
//! The interrupt subsystem manages hardware and software interrupts.
//! IRQ descriptors track per-IRQ state; request_irq flags control
//! sharing, edge/level triggering, and handler behavior. Softirqs
//! and tasklets provide deferred interrupt processing.

// ---------------------------------------------------------------------------
// IRQ request flags (IRQF_*)
// ---------------------------------------------------------------------------

/// Shared IRQ line.
pub const IRQF_SHARED: u32 = 0x0000_0080;
/// Probe shared IRQ.
pub const IRQF_PROBE_SHARED: u32 = 0x0000_0100;
/// Timer interrupt.
pub const IRQF_TIMER: u32 = 0x0000_0200;
/// Per-CPU interrupt.
pub const IRQF_PERCPU: u32 = 0x0000_0400;
/// No balancing.
pub const IRQF_NOBALANCING: u32 = 0x0000_0800;
/// IRQ is a high-priority interrupt.
pub const IRQF_IRQPOLL: u32 = 0x0000_1000;
/// One-shot (mask until handler finishes).
pub const IRQF_ONESHOT: u32 = 0x0000_2000;
/// No suspend (keep during S3).
pub const IRQF_NO_SUSPEND: u32 = 0x0000_4000;
/// Force resume.
pub const IRQF_FORCE_RESUME: u32 = 0x0000_8000;
/// No threaded handler.
pub const IRQF_NO_THREAD: u32 = 0x0001_0000;
/// Early resume.
pub const IRQF_EARLY_RESUME: u32 = 0x0002_0000;
/// Context saver.
pub const IRQF_COND_SUSPEND: u32 = 0x0004_0000;
/// Cond-oneshot.
pub const IRQF_COND_ONESHOT: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// IRQ trigger types (IRQF_TRIGGER_*)
// ---------------------------------------------------------------------------

/// No trigger type specified.
pub const IRQF_TRIGGER_NONE: u32 = 0x0000_0000;
/// Rising edge triggered.
pub const IRQF_TRIGGER_RISING: u32 = 0x0000_0001;
/// Falling edge triggered.
pub const IRQF_TRIGGER_FALLING: u32 = 0x0000_0002;
/// Active high level triggered.
pub const IRQF_TRIGGER_HIGH: u32 = 0x0000_0004;
/// Active low level triggered.
pub const IRQF_TRIGGER_LOW: u32 = 0x0000_0008;
/// Mask of all trigger bits.
pub const IRQF_TRIGGER_MASK: u32 = IRQF_TRIGGER_RISING
    | IRQF_TRIGGER_FALLING
    | IRQF_TRIGGER_HIGH
    | IRQF_TRIGGER_LOW;

// ---------------------------------------------------------------------------
// IRQ return values
// ---------------------------------------------------------------------------

/// IRQ not handled.
pub const IRQ_NONE: u32 = 0;
/// IRQ handled.
pub const IRQ_HANDLED: u32 = 1;
/// IRQ handled, wake thread.
pub const IRQ_WAKE_THREAD: u32 = 2;

// ---------------------------------------------------------------------------
// Softirq vectors
// ---------------------------------------------------------------------------

/// High-priority tasklet.
pub const HI_SOFTIRQ: u32 = 0;
/// Timer softirq.
pub const TIMER_SOFTIRQ: u32 = 1;
/// Network TX softirq.
pub const NET_TX_SOFTIRQ: u32 = 2;
/// Network RX softirq.
pub const NET_RX_SOFTIRQ: u32 = 3;
/// Block I/O softirq.
pub const BLOCK_SOFTIRQ: u32 = 4;
/// IRQ poll softirq.
pub const IRQ_POLL_SOFTIRQ: u32 = 5;
/// Tasklet softirq.
pub const TASKLET_SOFTIRQ: u32 = 6;
/// Scheduler softirq.
pub const SCHED_SOFTIRQ: u32 = 7;
/// High-res timer softirq.
pub const HRTIMER_SOFTIRQ: u32 = 8;
/// RCU softirq.
pub const RCU_SOFTIRQ: u32 = 9;
/// Number of softirq vectors.
pub const NR_SOFTIRQS: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_types_powers_of_two() {
        let triggers = [
            IRQF_TRIGGER_RISING, IRQF_TRIGGER_FALLING,
            IRQF_TRIGGER_HIGH, IRQF_TRIGGER_LOW,
        ];
        for t in &triggers {
            assert!(t.is_power_of_two(), "0x{:x}", t);
        }
    }

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
        assert_eq!(IRQF_TRIGGER_MASK, 0x0F);
    }

    #[test]
    fn test_irq_return_distinct() {
        let returns = [IRQ_NONE, IRQ_HANDLED, IRQ_WAKE_THREAD];
        for i in 0..returns.len() {
            for j in (i + 1)..returns.len() {
                assert_ne!(returns[i], returns[j]);
            }
        }
    }

    #[test]
    fn test_softirq_vectors_distinct() {
        let vectors = [
            HI_SOFTIRQ, TIMER_SOFTIRQ, NET_TX_SOFTIRQ,
            NET_RX_SOFTIRQ, BLOCK_SOFTIRQ, IRQ_POLL_SOFTIRQ,
            TASKLET_SOFTIRQ, SCHED_SOFTIRQ, HRTIMER_SOFTIRQ,
            RCU_SOFTIRQ,
        ];
        for i in 0..vectors.len() {
            for j in (i + 1)..vectors.len() {
                assert_ne!(vectors[i], vectors[j]);
            }
        }
    }

    #[test]
    fn test_nr_softirqs() {
        assert_eq!(NR_SOFTIRQS, 10);
    }

    #[test]
    fn test_all_softirqs_below_nr() {
        let vectors = [
            HI_SOFTIRQ, TIMER_SOFTIRQ, NET_TX_SOFTIRQ,
            NET_RX_SOFTIRQ, BLOCK_SOFTIRQ, IRQ_POLL_SOFTIRQ,
            TASKLET_SOFTIRQ, SCHED_SOFTIRQ, HRTIMER_SOFTIRQ,
            RCU_SOFTIRQ,
        ];
        for v in &vectors {
            assert!(*v < NR_SOFTIRQS);
        }
    }

    #[test]
    fn test_request_flags_distinct() {
        let flags = [
            IRQF_SHARED, IRQF_PROBE_SHARED, IRQF_TIMER,
            IRQF_PERCPU, IRQF_NOBALANCING, IRQF_IRQPOLL,
            IRQF_ONESHOT, IRQF_NO_SUSPEND, IRQF_FORCE_RESUME,
            IRQF_NO_THREAD, IRQF_EARLY_RESUME,
            IRQF_COND_SUSPEND, IRQF_COND_ONESHOT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
