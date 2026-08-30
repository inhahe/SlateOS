//! `<linux/rseq.h>` — Restartable sequences constants.
//!
//! Restartable sequences (rseq) allow userspace to perform
//! per-CPU atomic operations without kernel transitions.
//! If a thread is preempted or migrated during a critical
//! section, the kernel restarts it. Used by glibc for fast
//! per-CPU memory allocation.

// ---------------------------------------------------------------------------
// rseq flags
// ---------------------------------------------------------------------------

/// Register rseq for this thread.
pub const RSEQ_FLAG_UNREGISTER: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// rseq cs flags (critical section descriptor)
// ---------------------------------------------------------------------------

/// No migration check (per-CPU only, no preemption restart).
pub const RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT: u32 = 1 << 0;
/// No signal delivery during critical section.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL: u32 = 1 << 1;
/// No migration restart.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// rseq ABI constants
// ---------------------------------------------------------------------------

/// Size of struct rseq (as registered via syscall).
pub const RSEQ_STRUCT_SIZE_V1: u32 = 32;

/// Alignment requirement for struct rseq.
pub const RSEQ_STRUCT_ALIGN: u32 = 32;

/// Signature value placed before abort handler for validation.
/// x86_64 uses 0x53053053 by convention.
pub const RSEQ_SIG_X86_64: u32 = 0x53053053;

// ---------------------------------------------------------------------------
// CPU ID special values
// ---------------------------------------------------------------------------

/// CPU ID indicating uninitialized/unregistered state.
pub const RSEQ_CPU_ID_UNINITIALIZED: i32 = -1;
/// CPU ID indicating registration in progress.
pub const RSEQ_CPU_ID_REGISTRATION_FAILED: i32 = -2;

// ---------------------------------------------------------------------------
// Memory model constants
// ---------------------------------------------------------------------------

/// rseq uses relaxed memory ordering within critical sections.
pub const RSEQ_MEMORY_ORDER_RELAXED: u32 = 0;
/// Acquire semantics.
pub const RSEQ_MEMORY_ORDER_ACQUIRE: u32 = 1;
/// Release semantics.
pub const RSEQ_MEMORY_ORDER_RELEASE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cs_flags_powers_of_two() {
        let flags = [
            RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT,
            RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL,
            RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_cs_flags_no_overlap() {
        let flags = [
            RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT,
            RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL,
            RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_struct_size_and_align() {
        assert_eq!(RSEQ_STRUCT_SIZE_V1, 32);
        assert_eq!(RSEQ_STRUCT_ALIGN, 32);
        // Align should be power of two
        assert!(RSEQ_STRUCT_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_cpu_id_specials() {
        assert_ne!(RSEQ_CPU_ID_UNINITIALIZED, RSEQ_CPU_ID_REGISTRATION_FAILED);
        assert!(RSEQ_CPU_ID_UNINITIALIZED < 0);
        assert!(RSEQ_CPU_ID_REGISTRATION_FAILED < 0);
    }

    #[test]
    fn test_memory_orders_distinct() {
        let orders = [
            RSEQ_MEMORY_ORDER_RELAXED,
            RSEQ_MEMORY_ORDER_ACQUIRE,
            RSEQ_MEMORY_ORDER_RELEASE,
        ];
        for i in 0..orders.len() {
            for j in (i + 1)..orders.len() {
                assert_ne!(orders[i], orders[j]);
            }
        }
    }

    #[test]
    fn test_rseq_sig() {
        assert_eq!(RSEQ_SIG_X86_64, 0x53053053);
    }
}
