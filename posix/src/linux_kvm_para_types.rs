//! `<linux/kvm_para.h>` — KVM paravirtualization constants.
//!
//! KVM paravirtualization allows guests to cooperate with the
//! hypervisor for better performance. Instead of trapping on every
//! privileged operation, the guest uses hypercalls to request services
//! directly. Features include stolen time accounting, async page
//! faults (guest can schedule other work while a page is being
//! swapped in), PV TLB flush (batch TLB shootdowns), PV spinlocks
//! (yield to hypervisor instead of spinning), and clock source.

// ---------------------------------------------------------------------------
// KVM hypercall numbers
// ---------------------------------------------------------------------------

/// Hypercall not implemented (returns -ENOSYS equivalent).
pub const KVM_HC_VAPIC_POLL_IRQ: u32 = 1;
/// MMU operation (deprecated).
pub const KVM_HC_MMU_OP: u32 = 2;
/// Get feature flags.
pub const KVM_HC_FEATURES: u32 = 3;
/// PV EOI (end-of-interrupt).
pub const KVM_HC_PPC_MAP_MAGIC_PAGE: u32 = 4;
/// Kick a vCPU out of halt.
pub const KVM_HC_KICK_CPU: u32 = 5;
/// Clock pairing for precision time.
pub const KVM_HC_CLOCK_PAIRING: u32 = 9;
/// Send IPI (inter-processor interrupt).
pub const KVM_HC_SEND_IPI: u32 = 10;
/// Schedule yield to hypervisor.
pub const KVM_HC_SCHED_YIELD: u32 = 11;
/// Map guest GPA range.
pub const KVM_HC_MAP_GPA_RANGE: u32 = 12;

// ---------------------------------------------------------------------------
// KVM feature bits (CPUID leaf 0x40000001)
// ---------------------------------------------------------------------------

/// Guest can use clocksource (kvmclock).
pub const KVM_FEATURE_CLOCKSOURCE: u32 = 0;
/// NOP I/O delay (don't use port 0x80 for timing).
pub const KVM_FEATURE_NOP_IO_DELAY: u32 = 1;
/// MMU operation (deprecated).
pub const KVM_FEATURE_MMU_OP: u32 = 2;
/// Kvmclock v2 (more precise).
pub const KVM_FEATURE_CLOCKSOURCE2: u32 = 3;
/// Async page fault.
pub const KVM_FEATURE_ASYNC_PF: u32 = 4;
/// Steal time accounting.
pub const KVM_FEATURE_STEAL_TIME: u32 = 5;
/// PV end-of-interrupt.
pub const KVM_FEATURE_PV_EOI: u32 = 6;
/// PV unhalt (wake idle vCPUs efficiently).
pub const KVM_FEATURE_PV_UNHALT: u32 = 7;
/// PV TLB flush.
pub const KVM_FEATURE_PV_TLB_FLUSH: u32 = 9;
/// Async page fault v2.
pub const KVM_FEATURE_ASYNC_PF_VMEXIT: u32 = 10;
/// PV send IPI.
pub const KVM_FEATURE_PV_SEND_IPI: u32 = 11;
/// PV sched yield.
pub const KVM_FEATURE_PV_SCHED_YIELD: u32 = 13;
/// Clocksource stable bit.
pub const KVM_FEATURE_CLOCKSOURCE_STABLE_BIT: u32 = 24;

// ---------------------------------------------------------------------------
// KVM PV async page fault
// ---------------------------------------------------------------------------

/// Async page fault — page not present (guest should schedule other work).
pub const KVM_PV_REASON_PAGE_NOT_PRESENT: u32 = 1;
/// Async page fault — page ready (page has been swapped in).
pub const KVM_PV_REASON_PAGE_READY: u32 = 2;

// ---------------------------------------------------------------------------
// KVM PV EOI
// ---------------------------------------------------------------------------

/// PV EOI enabled flag.
pub const KVM_PV_EOI_ENABLED: u32 = 1 << 0;
/// PV EOI — interrupt needs EOI.
pub const KVM_PV_EOI_NEED_EOI: u32 = 0;
/// PV EOI — no EOI needed (already handled).
pub const KVM_PV_EOI_NO_EOI: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hypercalls_distinct() {
        let hcs = [
            KVM_HC_VAPIC_POLL_IRQ,
            KVM_HC_MMU_OP,
            KVM_HC_FEATURES,
            KVM_HC_PPC_MAP_MAGIC_PAGE,
            KVM_HC_KICK_CPU,
            KVM_HC_CLOCK_PAIRING,
            KVM_HC_SEND_IPI,
            KVM_HC_SCHED_YIELD,
            KVM_HC_MAP_GPA_RANGE,
        ];
        for i in 0..hcs.len() {
            for j in (i + 1)..hcs.len() {
                assert_ne!(hcs[i], hcs[j]);
            }
        }
    }

    #[test]
    fn test_features_distinct() {
        let feats = [
            KVM_FEATURE_CLOCKSOURCE,
            KVM_FEATURE_NOP_IO_DELAY,
            KVM_FEATURE_MMU_OP,
            KVM_FEATURE_CLOCKSOURCE2,
            KVM_FEATURE_ASYNC_PF,
            KVM_FEATURE_STEAL_TIME,
            KVM_FEATURE_PV_EOI,
            KVM_FEATURE_PV_UNHALT,
            KVM_FEATURE_PV_TLB_FLUSH,
            KVM_FEATURE_ASYNC_PF_VMEXIT,
            KVM_FEATURE_PV_SEND_IPI,
            KVM_FEATURE_PV_SCHED_YIELD,
            KVM_FEATURE_CLOCKSOURCE_STABLE_BIT,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_async_pf_reasons_distinct() {
        assert_ne!(KVM_PV_REASON_PAGE_NOT_PRESENT, KVM_PV_REASON_PAGE_READY);
    }

    #[test]
    fn test_pv_eoi_values() {
        assert_ne!(KVM_PV_EOI_NEED_EOI, KVM_PV_EOI_NO_EOI);
        assert_eq!(KVM_PV_EOI_ENABLED, 1);
    }
}
