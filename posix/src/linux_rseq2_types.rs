//! `<linux/rseq.h>` — Additional restartable sequences constants.
//!
//! Supplementary rseq constants covering flags,
//! CPU ID states, and signature values.

// ---------------------------------------------------------------------------
// RSEQ flags (RSEQ_FLAG_*)
// ---------------------------------------------------------------------------

/// Unregister.
pub const RSEQ_FLAG_UNREGISTER: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// RSEQ CS flags (RSEQ_CS_FLAG_*)
// ---------------------------------------------------------------------------

/// No restart on preempt.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT: u32 = 1 << 0;
/// No restart on signal.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL: u32 = 1 << 1;
/// No restart on migrate.
pub const RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// RSEQ CPU ID special values
// ---------------------------------------------------------------------------

/// Uninitialized CPU ID.
pub const RSEQ_CPU_ID_UNINITIALIZED: i32 = -1;
/// Registration failed.
pub const RSEQ_CPU_ID_REGISTRATION_FAILED: i32 = -2;

// ---------------------------------------------------------------------------
// RSEQ signature (per-architecture)
// ---------------------------------------------------------------------------

/// x86-64 rseq signature.
pub const RSEQ_SIG_X86_64: u32 = 0x53053053;

// ---------------------------------------------------------------------------
// RSEQ node/mm flags
// ---------------------------------------------------------------------------

/// Node ID valid.
pub const RSEQ_MM_CID_VALID: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// RSEQ structure sizes
// ---------------------------------------------------------------------------

/// Original rseq struct size (v1).
pub const RSEQ_SIZE_V1: u32 = 32;
/// Extended rseq struct size (v2 with mm_cid).
pub const RSEQ_SIZE_V2: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_unregister() {
        assert!(RSEQ_FLAG_UNREGISTER.is_power_of_two());
    }

    #[test]
    fn test_cs_flags_power_of_two() {
        let flags = [
            RSEQ_CS_FLAG_NO_RESTART_ON_PREEMPT,
            RSEQ_CS_FLAG_NO_RESTART_ON_SIGNAL,
            RSEQ_CS_FLAG_NO_RESTART_ON_MIGRATE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
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
    fn test_cpu_id_special() {
        assert!(RSEQ_CPU_ID_UNINITIALIZED < 0);
        assert!(RSEQ_CPU_ID_REGISTRATION_FAILED < 0);
        assert_ne!(RSEQ_CPU_ID_UNINITIALIZED, RSEQ_CPU_ID_REGISTRATION_FAILED);
    }

    #[test]
    fn test_sizes() {
        assert!(RSEQ_SIZE_V1 < RSEQ_SIZE_V2);
        assert_eq!(RSEQ_SIZE_V1, 32);
        assert_eq!(RSEQ_SIZE_V2, 64);
    }

    #[test]
    fn test_signature() {
        assert_eq!(RSEQ_SIG_X86_64, 0x53053053);
    }
}
