//! `<linux/rseq.h>` — restartable sequences ABI.
//!
//! rseq lets userspace mark a short critical section that will be
//! restarted (PC rewound) if the thread is preempted, migrated, or
//! signalled inside it. glibc registers an rseq area for every
//! pthread on supported kernels; per-CPU allocators like tcmalloc
//! use it to avoid atomics on hot paths.

// ---------------------------------------------------------------------------
// Magic value for the `rseq_cs.version` field
// ---------------------------------------------------------------------------

pub const RSEQ_SIG_VERSION: u32 = 0;

// ---------------------------------------------------------------------------
// Flags passed to `rseq(2)`
// ---------------------------------------------------------------------------

pub const RSEQ_FLAG_UNREGISTER: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// `rseq_cs.flags` — per critical-section restart-suppression hints
// ---------------------------------------------------------------------------

pub const RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT: u32 = 1 << 0;
pub const RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL: u32 = 1 << 1;
pub const RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Sizes / alignment
// ---------------------------------------------------------------------------

/// The kernel ABI requires the rseq area to be 32-byte aligned and
/// at least 32 bytes long.
pub const RSEQ_ALIGN: usize = 32;
/// Original ABI size (`struct rseq` was 32 bytes before node_id was added).
pub const RSEQ_SIZE_V0: usize = 32;

/// Magic signature stored at the start of the rseq critical-section
/// descriptor — used to defeat cross-application ROP gadgets that
/// point at a rseq_cs structure.
pub const RSEQ_SIG_DEFAULT: u32 = 0x5331_2553;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_RSEQ: u32 = 334;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_unregister_bit_zero() {
        // UNREGISTER is the only flag accepted on the syscall.
        assert_eq!(RSEQ_FLAG_UNREGISTER, 1);
        assert!(RSEQ_FLAG_UNREGISTER.is_power_of_two());
    }

    #[test]
    fn test_cs_flags_low_3_bits_distinct() {
        let f = [
            RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT,
            RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL,
            RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE,
        ];
        let mut or = 0u32;
        for (i, v) in f.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0x7);
    }

    #[test]
    fn test_align_and_size_are_32() {
        // The rseq area is 32-byte aligned and at least 32 bytes long
        // in the original ABI.
        assert_eq!(RSEQ_ALIGN, 32);
        assert_eq!(RSEQ_SIZE_V0, 32);
        assert!(RSEQ_ALIGN.is_power_of_two());
    }

    #[test]
    fn test_default_signature_distinct() {
        // The default signature must be non-zero so it's a meaningful
        // ROP-mitigation tag.
        assert_ne!(RSEQ_SIG_DEFAULT, 0);
        // Its ASCII reads as "S3!S" — the historical libc default.
        assert_eq!(RSEQ_SIG_DEFAULT, 0x53312553);
    }

    #[test]
    fn test_syscall_number() {
        // rseq(2) was added in Linux 4.18.
        assert_eq!(NR_RSEQ, 334);
    }
}
