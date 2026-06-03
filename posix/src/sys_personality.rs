//! `<sys/personality.h>` — process execution domain.
//!
//! The `personality()` system call controls the execution domain
//! of a process.  This affects signal handling, system call
//! behavior, and virtual address space layout.  It is Linux-specific.

// ---------------------------------------------------------------------------
// Re-export the function from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::PERSONALITY_QUERY;
pub use crate::unistd::current_personality;
pub use crate::unistd::personality;

// ---------------------------------------------------------------------------
// Personality flags
// ---------------------------------------------------------------------------

/// Default Linux execution domain.
pub const PER_LINUX: u32 = 0x0000;

/// Linux with 32-bit compatibility.
pub const PER_LINUX32: u32 = 0x0008;

/// SVR4 execution domain.
pub const PER_SVR4: u32 = 0x0001;

/// SVR3 execution domain.
pub const PER_SVR3: u32 = 0x0002;

/// SCO Unix execution domain.
pub const PER_SCOSVR3: u32 = 0x0003;

/// OSR5 execution domain.
pub const PER_OSR5: u32 = 0x0003;

/// BSD execution domain.
pub const PER_BSD: u32 = 0x0006;

/// FreeBSD execution domain.
pub const PER_FREEBSD: u32 = 0x0006;

/// Xenix execution domain.
pub const PER_XENIX: u32 = 0x0007;

/// Linux with 32-bit emulation.
pub const PER_LINUX32_3GB: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Personality modification bits
// ---------------------------------------------------------------------------

/// Use short inode numbers.
pub const SHORT_INODE: u32 = 0x1000000;

/// Use sticky bit for executables.
pub const STICKY_TIMEOUTS: u32 = 0x4000000;

/// Disable address space layout randomization.
pub const ADDR_NO_RANDOMIZE: u32 = 0x0040000;

/// Disable ASLR mmap randomization.
pub const MMAP_PAGE_ZERO: u32 = 0x0100000;

/// Limit address space to 3 GB (32-bit compat).
pub const ADDR_COMPAT_LAYOUT: u32 = 0x0200000;

/// Read implies exec (legacy behavior).
pub const READ_IMPLIES_EXEC: u32 = 0x0400000;

/// Limit stack to 32-bit address range.
pub const ADDR_LIMIT_32BIT: u32 = 0x0800000;

/// Limit stack to 3 GB.
pub const ADDR_LIMIT_3GB: u32 = 0x8000000;

/// Whole address space (no limits).
pub const WHOLE_SECONDS: u32 = 0x2000000;

// personality() function is re-exported from unistd above.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_linux() {
        assert_eq!(PER_LINUX, 0);
    }

    #[test]
    fn test_per_linux32() {
        assert_eq!(PER_LINUX32, 0x0008);
    }

    /// Restore the personality to PER_LINUX so tests don't bleed
    /// state into each other.  Returns the previous value in case the
    /// test wants to assert what it was.
    fn reset_personality() -> i32 {
        personality(PER_LINUX as u64)
    }

    #[test]
    fn test_personality_query() {
        reset_personality();
        let result = personality(PERSONALITY_QUERY as u64);
        assert_eq!(result, PER_LINUX as i32);
    }

    #[test]
    fn test_personality_set() {
        reset_personality();
        let result = personality(PER_LINUX as u64);
        assert_eq!(result, PER_LINUX as i32);
    }

    #[test]
    fn test_addr_no_randomize() {
        assert_eq!(ADDR_NO_RANDOMIZE, 0x0040000);
    }

    #[test]
    fn test_modification_bits_distinct() {
        let bits = [
            SHORT_INODE,
            STICKY_TIMEOUTS,
            ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j], "personality bits must be distinct");
            }
        }
    }

    #[test]
    fn test_modification_bits_are_flags() {
        let bits = [
            SHORT_INODE,
            STICKY_TIMEOUTS,
            ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
        ];
        for &b in &bits {
            assert_ne!(b, 0);
            assert_eq!(b & (b - 1), 0, "bit 0x{b:X} is not a power of two");
        }
    }

    // -----------------------------------------------------------------------
    // Phase 78 — personality state-tracking and truncation parity
    //
    // Linux's `sys_personality` is a thin "remember last value, return
    // previous" helper.  Our previous stub always returned 0 (PER_LINUX)
    // regardless of what was set, and compared `persona == 0xFFFFFFFF`
    // against a u64 — so `personality(0xFFFF_FFFF_FFFF_FFFF)` would
    // overwrite the state with garbage instead of being treated as the
    // query sentinel.  Phase 78 fixes both behaviours.
    // -----------------------------------------------------------------------

    /// RAII guard restoring the personality on drop.
    struct PersonalityGuard {
        saved: u32,
    }
    impl PersonalityGuard {
        fn snapshot() -> Self {
            Self {
                saved: current_personality(),
            }
        }
    }
    impl Drop for PersonalityGuard {
        fn drop(&mut self) {
            let _ = personality(self.saved as u64);
        }
    }

    #[test]
    fn test_phase78_query_constant_value() {
        assert_eq!(PERSONALITY_QUERY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_phase78_set_returns_previous_value() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        // Set to PER_LINUX32, observe previous (PER_LINUX = 0).
        let old = personality(PER_LINUX32 as u64);
        assert_eq!(old, PER_LINUX as i32);
        // Set back to PER_LINUX, observe previous (PER_LINUX32).
        let old2 = personality(PER_LINUX as u64);
        assert_eq!(old2, PER_LINUX32 as i32);
    }

    #[test]
    fn test_phase78_query_returns_current_after_set() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(PER_LINUX32 as u64);
        let q = personality(PERSONALITY_QUERY as u64);
        assert_eq!(q, PER_LINUX32 as i32);
        // Query is non-destructive: a second query returns the same.
        let q2 = personality(PERSONALITY_QUERY as u64);
        assert_eq!(q2, PER_LINUX32 as i32);
    }

    #[test]
    fn test_phase78_query_does_not_clobber_state() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality((ADDR_NO_RANDOMIZE | PER_LINUX) as u64);
        let _ = personality(PERSONALITY_QUERY as u64); // query (should not change)
        let _ = personality(PERSONALITY_QUERY as u64);
        // After two queries the underlying state must still equal the
        // pre-query value.
        assert_eq!(current_personality(), ADDR_NO_RANDOMIZE | PER_LINUX);
    }

    #[test]
    fn test_phase78_current_personality_helper_matches_query() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(PER_BSD as u64);
        assert_eq!(current_personality(), PER_BSD);
        let queried = personality(PERSONALITY_QUERY as u64) as u32;
        assert_eq!(queried, PER_BSD);
    }

    // -- Truncation parity --------------------------------------------------

    #[test]
    fn test_phase78_high_bits_of_u64_truncated_for_query() {
        // Linux's syscall is `unsigned int`; bits above 32 are dropped.
        // 0xFFFF_FFFF_FFFF_FFFF must be treated as the query sentinel,
        // not as a set with garbage high bits.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(PER_LINUX32 as u64);
        let q = personality(0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(q, PER_LINUX32 as i32);
        // State must not have changed — still PER_LINUX32.
        assert_eq!(current_personality(), PER_LINUX32);
    }

    #[test]
    fn test_phase78_high_bits_of_u64_truncated_for_set() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        // Set with high bits that should be discarded.  Only the low
        // 32 bits become the stored personality.
        let _ = personality(0xDEAD_BEEF_0000_0008);
        assert_eq!(current_personality(), 0x0000_0008);
    }

    #[test]
    fn test_phase78_zero_set_returns_to_per_linux() {
        let _g = PersonalityGuard::snapshot();
        let _ = personality(PER_LINUX32 as u64);
        let old = personality(0);
        // Could be PER_LINUX32 if just set, but generally the previous
        // value.  After this call the state is PER_LINUX (0).
        let _ = old;
        assert_eq!(current_personality(), 0);
    }

    // -- Round-trip and bit-preservation -----------------------------------

    #[test]
    fn test_phase78_unknown_bits_are_preserved() {
        // Linux does NOT validate the bits; whatever you store comes
        // back out.  This mirrors `proc/self/personality` behaviour.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let weird: u32 = 0x1234_5678;
        let _ = personality(weird as u64);
        assert_eq!(current_personality(), weird);
        let q = personality(PERSONALITY_QUERY as u64) as u32;
        assert_eq!(q, weird);
    }

    #[test]
    fn test_phase78_combined_flags_round_trip() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let combined = PER_LINUX | ADDR_NO_RANDOMIZE | MMAP_PAGE_ZERO | READ_IMPLIES_EXEC;
        let _ = personality(combined as u64);
        assert_eq!(current_personality(), combined);
    }

    #[test]
    fn test_phase78_max_u32_minus_one_is_set_not_query() {
        // 0xFFFFFFFE is *not* the query sentinel — only 0xFFFFFFFF is.
        // It must be stored.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(0xFFFF_FFFE);
        assert_eq!(current_personality(), 0xFFFF_FFFE);
        // A subsequent real query reads it back.
        let q = personality(PERSONALITY_QUERY as u64) as u32;
        assert_eq!(q, 0xFFFF_FFFE);
    }

    // -- Workflow / sequence parity ----------------------------------------

    #[test]
    fn test_phase78_workflow_set_query_set_returns_chain() {
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        // After each `personality(new)` the *previous* is returned.
        let a = personality(PER_LINUX as u64); // prev: whatever, now 0
        let _ = a;
        let b = personality(PER_LINUX32 as u64); // prev: 0 → expect 0
        assert_eq!(b, PER_LINUX as i32);
        let c = personality(PER_BSD as u64); // prev: PER_LINUX32 → 8
        assert_eq!(c, PER_LINUX32 as i32);
        let d = personality(PERSONALITY_QUERY as u64); // query → PER_BSD
        assert_eq!(d, PER_BSD as i32);
        let e = personality(PERSONALITY_QUERY as u64); // query again, unchanged
        assert_eq!(e, PER_BSD as i32);
    }

    #[test]
    fn test_phase78_addr_no_randomize_set_then_query() {
        // Common real-world use: a debugger setting ADDR_NO_RANDOMIZE
        // before exec'ing the target.  Verify the bit survives a query.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality((PER_LINUX | ADDR_NO_RANDOMIZE) as u64);
        let q = personality(PERSONALITY_QUERY as u64) as u32;
        assert_eq!(q & ADDR_NO_RANDOMIZE, ADDR_NO_RANDOMIZE);
        assert_eq!(q & 0xFF, PER_LINUX); // domain bits
    }

    // -- Buggy-caller cases -------------------------------------------------

    #[test]
    fn test_phase78_buggy_caller_sign_extended_minus_one() {
        // C code that does `personality((unsigned long)(-1))` on a
        // 64-bit platform passes 0xFFFFFFFFFFFFFFFF as u64.  This must
        // be the query, not a corrupt set.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(PER_LINUX32 as u64);
        let neg_one_as_ulong: u64 = !0u64;
        let q = personality(neg_one_as_ulong);
        assert_eq!(q, PER_LINUX32 as i32);
        assert_eq!(current_personality(), PER_LINUX32);
    }

    #[test]
    fn test_phase78_buggy_caller_passes_int_min() {
        // i32::MIN bit pattern (0x80000000) cast to u64 — top bit set.
        // After truncation it's 0x80000000, a valid personality value
        // (no Linux validation).  Must be stored.
        let _g = PersonalityGuard::snapshot();
        reset_personality();
        let _ = personality(0x8000_0000_u64);
        assert_eq!(current_personality(), 0x8000_0000);
    }
}
