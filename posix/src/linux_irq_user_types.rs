//! `<linux/interrupt.h>` — IRQ flags and SMP affinity user ABI.
//!
//! The flags below are the `IRQF_*` bits passed to `request_irq()`
//! in kernel space, but they are also exposed verbatim to userspace
//! through `/proc/interrupts`, `irqbalance(8)`, and the
//! `/proc/irq/N/smp_affinity*` interfaces every NUMA-aware service
//! reads. The numeric values match `include/uapi/linux/interrupt.h`.

// ---------------------------------------------------------------------------
// IRQF_* flags (request_irq flags exposed to userspace)
// ---------------------------------------------------------------------------

/// Shareable interrupt (multiple handlers).
pub const IRQF_SHARED: u32 = 0x0000_0080;
/// Sample randomness pool from this IRQ.
pub const IRQF_SAMPLE_RANDOM: u32 = 0x0000_0040;
/// Always probe; never auto-disable.
pub const IRQF_PROBE_SHARED: u32 = 0x0000_0100;
/// Interrupt is per-CPU.
pub const IRQF_PERCPU: u32 = 0x0000_0400;
/// Do not allow this IRQ to be moved between CPUs.
pub const IRQF_NOBALANCING: u32 = 0x0000_0800;
/// Interrupt is used for polling (IRQF_IRQPOLL in newer kernels).
pub const IRQF_IRQPOLL: u32 = 0x0000_1000;
/// One-shot: mask line until threaded handler completes.
pub const IRQF_ONESHOT: u32 = 0x0000_2000;
/// Do not suspend this IRQ during system sleep.
pub const IRQF_NO_SUSPEND: u32 = 0x0000_4000;
/// Force-resume even if NOSUSPEND would have skipped it.
pub const IRQF_FORCE_RESUME: u32 = 0x0000_8000;
/// Run handler in hardirq context (do not thread).
pub const IRQF_NO_THREAD: u32 = 0x0001_0000;
/// Resume early in suspend exit path.
pub const IRQF_EARLY_RESUME: u32 = 0x0002_0000;
/// Mark hardirq as timer (for tickless accounting).
pub const IRQF_TIMER: u32 = IRQF_NOBALANCING | IRQF_NO_SUSPEND;

// ---------------------------------------------------------------------------
// IRQ trigger types (`enum irq_trigger`)
// ---------------------------------------------------------------------------

pub const IRQ_TYPE_NONE: u32 = 0x0000_0000;
pub const IRQ_TYPE_EDGE_RISING: u32 = 0x0000_0001;
pub const IRQ_TYPE_EDGE_FALLING: u32 = 0x0000_0002;
pub const IRQ_TYPE_EDGE_BOTH: u32 = IRQ_TYPE_EDGE_RISING | IRQ_TYPE_EDGE_FALLING;
pub const IRQ_TYPE_LEVEL_HIGH: u32 = 0x0000_0004;
pub const IRQ_TYPE_LEVEL_LOW: u32 = 0x0000_0008;
pub const IRQ_TYPE_LEVEL_MASK: u32 = IRQ_TYPE_LEVEL_HIGH | IRQ_TYPE_LEVEL_LOW;

// ---------------------------------------------------------------------------
// Architectural vector numbers (x86)
// ---------------------------------------------------------------------------

/// Highest user-controllable IRQ on legacy 8259 PIC.
pub const NR_IRQS_LEGACY: u32 = 16;
/// PIT timer on the legacy ISA bus.
pub const ISA_IRQ_TIMER: u32 = 0;
/// PS/2 keyboard.
pub const ISA_IRQ_KEYBOARD: u32 = 1;
/// PS/2 mouse (cascade through PIC2).
pub const ISA_IRQ_MOUSE: u32 = 12;
/// First user vector after the architectural exceptions on x86.
pub const FIRST_EXTERNAL_VECTOR: u32 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_irqf_single_bit_flags_are_pow2() {
        let b = [
            IRQF_SHARED,
            IRQF_SAMPLE_RANDOM,
            IRQF_PROBE_SHARED,
            IRQF_PERCPU,
            IRQF_NOBALANCING,
            IRQF_IRQPOLL,
            IRQF_ONESHOT,
            IRQF_NO_SUSPEND,
            IRQF_FORCE_RESUME,
            IRQF_NO_THREAD,
            IRQF_EARLY_RESUME,
        ];
        for &v in &b {
            assert!(v.is_power_of_two());
        }
        // Pairwise distinct.
        for i in 0..b.len() {
            for j in (i + 1)..b.len() {
                assert_ne!(b[i], b[j]);
            }
        }
    }

    #[test]
    fn test_irqf_timer_is_composite() {
        assert_eq!(IRQF_TIMER, IRQF_NOBALANCING | IRQF_NO_SUSPEND);
        assert!(!IRQF_TIMER.is_power_of_two());
    }

    #[test]
    fn test_trigger_types_layout() {
        // 0 is "no trigger configured".
        assert_eq!(IRQ_TYPE_NONE, 0);
        // EDGE_BOTH is OR of rising+falling.
        assert_eq!(IRQ_TYPE_EDGE_BOTH, 0b0011);
        // LEVEL bits don't collide with EDGE bits.
        assert_eq!(IRQ_TYPE_EDGE_BOTH & IRQ_TYPE_LEVEL_MASK, 0);
        // MASK covers HIGH and LOW.
        assert_eq!(IRQ_TYPE_LEVEL_MASK, IRQ_TYPE_LEVEL_HIGH | IRQ_TYPE_LEVEL_LOW);
    }

    #[test]
    fn test_legacy_isa_vectors() {
        assert_eq!(NR_IRQS_LEGACY, 16);
        assert_eq!(ISA_IRQ_TIMER, 0);
        assert_eq!(ISA_IRQ_KEYBOARD, 1);
        // PS/2 mouse traditionally on IRQ12.
        assert_eq!(ISA_IRQ_MOUSE, 12);
        // x86 reserves 0..0x1F for CPU exceptions.
        assert_eq!(FIRST_EXTERNAL_VECTOR, 0x20);
    }
}
