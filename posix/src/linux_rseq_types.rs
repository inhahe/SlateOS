//! `<linux/rseq.h>` — Restartable sequences (rseq) constants.
//!
//! Restartable sequences allow userspace to perform per-CPU atomic
//! operations without syscalls. A thread registers an rseq area; the
//! kernel restarts the critical section if the thread migrates CPUs
//! or is preempted. Used by glibc and high-performance allocators
//! (e.g., tcmalloc) for per-CPU data access.

// ---------------------------------------------------------------------------
// rseq flags
// ---------------------------------------------------------------------------

/// Unregister the rseq area.
pub const RSEQ_FLAG_UNREGISTER: u32 = 1;

// ---------------------------------------------------------------------------
// rseq CPU ID special values
// ---------------------------------------------------------------------------

/// CPU ID is uninitialized (registration not yet completed).
pub const RSEQ_CPU_ID_UNINITIALIZED: i32 = -1;
/// Registration failed (rseq not supported or error).
pub const RSEQ_CPU_ID_REGISTRATION_FAILED: i32 = -2;

// ---------------------------------------------------------------------------
// rseq_cs (critical section descriptor) flags
// ---------------------------------------------------------------------------

/// No migration during critical section.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT: u32 = 1;
/// No restart on signal delivery.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL: u32 = 2;
/// No restart on CPU migration.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE: u32 = 4;

// ---------------------------------------------------------------------------
// rseq structure sizes
// ---------------------------------------------------------------------------

/// Size of the original rseq ABI structure (v1).
pub const RSEQ_AREA_SIZE_V1: u32 = 32;
/// Alignment requirement for the rseq area.
pub const RSEQ_AREA_ALIGN: u32 = 32;
/// Size of the rseq_cs descriptor.
pub const RSEQ_CS_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// rseq field offsets (within struct rseq)
// ---------------------------------------------------------------------------

/// Offset of cpu_id_start field.
pub const RSEQ_OFFSET_CPU_ID_START: u32 = 4;
/// Offset of cpu_id field.
pub const RSEQ_OFFSET_CPU_ID: u32 = 8;
/// Offset of rseq_cs pointer field.
pub const RSEQ_OFFSET_RSEQ_CS: u32 = 16;
/// Offset of flags field.
pub const RSEQ_OFFSET_FLAGS: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_unregister() {
        assert_eq!(RSEQ_FLAG_UNREGISTER, 1);
    }

    #[test]
    fn test_cpu_id_specials_distinct() {
        assert_ne!(RSEQ_CPU_ID_UNINITIALIZED, RSEQ_CPU_ID_REGISTRATION_FAILED);
        assert!(RSEQ_CPU_ID_UNINITIALIZED < 0);
        assert!(RSEQ_CPU_ID_REGISTRATION_FAILED < 0);
    }

    #[test]
    fn test_cs_flags_no_overlap() {
        let flags = [
            RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT,
            RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL,
            RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_area_alignment() {
        assert!(RSEQ_AREA_ALIGN.is_power_of_two());
        assert!(RSEQ_AREA_SIZE_V1 <= RSEQ_AREA_ALIGN * 4);
    }

    #[test]
    fn test_field_offsets_ordered() {
        assert!(RSEQ_OFFSET_CPU_ID_START < RSEQ_OFFSET_CPU_ID);
        assert!(RSEQ_OFFSET_CPU_ID < RSEQ_OFFSET_RSEQ_CS);
        assert!(RSEQ_OFFSET_RSEQ_CS < RSEQ_OFFSET_FLAGS);
    }

    #[test]
    fn test_offsets_within_area() {
        assert!(RSEQ_OFFSET_FLAGS < RSEQ_AREA_SIZE_V1);
    }
}
