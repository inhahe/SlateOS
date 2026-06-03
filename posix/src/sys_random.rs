//! `<sys/random.h>` — random number generation.
//!
//! Provides constants and wrapper functions for the `getrandom`
//! system call interface.
//!
//! ## Implementation
//!
//! The actual `getrandom()` and `getentropy()` functions live in the
//! `unistd` module (matching glibc, which declares them in
//! `<unistd.h>` with constants in `<sys/random.h>`).  This module
//! provides the `GRND_*` flag constants and re-exports.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Read from `/dev/random` instead of `/dev/urandom`.
///
/// When set, `getrandom()` draws from the blocking entropy pool.
/// Without this flag it draws from the non-blocking pool (urandom).
pub const GRND_RANDOM: u32 = 0x0002;

/// Don't block if insufficient entropy is available.
///
/// `getrandom()` returns -1 with `EAGAIN` instead of blocking when
/// the entropy pool is not yet initialized.
pub const GRND_NONBLOCK: u32 = 0x0001;

/// Don't draw from the kernel RNG.
///
/// Linux 5.17+ flag: causes `getrandom()` to use the vDSO-based
/// chacha20 CPRNG seeded from kernel entropy, avoiding a syscall.
pub const GRND_INSECURE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_grnd_constants_nonzero() {
        assert_ne!(GRND_RANDOM, 0);
        assert_ne!(GRND_NONBLOCK, 0);
        assert_ne!(GRND_INSECURE, 0);
    }

    #[test]
    fn test_grnd_constants_distinct() {
        assert_ne!(GRND_RANDOM, GRND_NONBLOCK);
        assert_ne!(GRND_RANDOM, GRND_INSECURE);
        assert_ne!(GRND_NONBLOCK, GRND_INSECURE);
    }

    #[test]
    fn test_grnd_constants_are_power_of_two() {
        // Each flag should be a single bit.
        assert!(GRND_RANDOM.is_power_of_two());
        assert!(GRND_NONBLOCK.is_power_of_two());
        assert!(GRND_INSECURE.is_power_of_two());
    }

    #[test]
    fn test_grnd_values() {
        assert_eq!(GRND_NONBLOCK, 0x0001);
        assert_eq!(GRND_RANDOM, 0x0002);
        assert_eq!(GRND_INSECURE, 0x0004);
    }

    #[test]
    fn test_grnd_flags_combinable() {
        // Flags should be OR-able without collision.
        let combined = GRND_RANDOM | GRND_NONBLOCK;
        assert_eq!(combined, 0x0003);
        assert_ne!(combined & GRND_RANDOM, 0);
        assert_ne!(combined & GRND_NONBLOCK, 0);
        assert_eq!(combined & GRND_INSECURE, 0);
    }

    #[test]
    fn test_grnd_all_flags() {
        let all = GRND_RANDOM | GRND_NONBLOCK | GRND_INSECURE;
        assert_eq!(all, 0x0007);
    }

    // -----------------------------------------------------------------------
    // Integration: getrandom from unistd uses these flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_getrandom_with_zero_flags() {
        let mut buf = [0u8; 16];
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), 0);
        // Our stub returns buflen on success.
        assert_eq!(ret, 16);
    }

    #[test]
    fn test_getrandom_with_grnd_random() {
        let mut buf = [0u8; 8];
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_RANDOM);
        assert_eq!(ret, 8);
    }

    #[test]
    fn test_getrandom_with_grnd_nonblock() {
        let mut buf = [0u8; 4];
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_NONBLOCK);
        assert_eq!(ret, 4);
    }

    #[test]
    fn test_getrandom_null_buffer_zero_len() {
        // Null buffer with zero length is a no-op success — Linux short-
        // circuits copy_to_user for zero length and returns 0.
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 0, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_getentropy_basic() {
        let mut buf = [0u8; 32];
        let ret = crate::unistd::getentropy(buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, 0); // getentropy returns 0 on success.
    }

    #[test]
    fn test_getentropy_max_256() {
        // getentropy should fail for buffers > 256 bytes.
        let mut buf = [0u8; 257];
        let ret = crate::unistd::getentropy(buf.as_mut_ptr(), buf.len());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Phase 80 — getrandom flag-validation tests
    //
    // These mirror the Linux `SYSCALL_DEFINE3(getrandom, ...)` prologue:
    //   1. flags & ~GRND_VALID_FLAGS  → EINVAL
    //   2. GRND_RANDOM | GRND_INSECURE simultaneously → EINVAL
    //   3. null buf + non-zero len    → EFAULT
    //   4. buflen > isize::MAX        → EINVAL
    // and the success cases for each individual valid flag.
    // -----------------------------------------------------------------------

    /// Convenience: a small valid buffer to share across success cases.
    fn small_buf() -> [u8; 16] {
        [0u8; 16]
    }

    // ---- (a) Helper / constant invariants --------------------------------

    #[test]
    fn test_grnd_valid_flags_mask_equals_union() {
        // The exported mask must equal the union of every documented flag.
        assert_eq!(
            crate::unistd::GRND_VALID_FLAGS,
            GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE
        );
    }

    #[test]
    fn test_grnd_valid_flags_excludes_unknown_bits() {
        // No high bits should be in the mask.
        assert_eq!(crate::unistd::GRND_VALID_FLAGS & 0xFFFF_FFF8, 0);
    }

    #[test]
    fn test_grnd_insecure_reexported_value() {
        // unistd and sys_random must agree on the GRND_INSECURE value.
        assert_eq!(crate::unistd::GRND_INSECURE, GRND_INSECURE);
    }

    // ---- (b) EINVAL — unknown flag bits ----------------------------------

    #[test]
    fn test_getrandom_rejects_high_bit_flag() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), 0x8000_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_rejects_one_unknown_bit_with_valid_bits() {
        // Valid flags ORed with one unknown bit must still be rejected —
        // Linux validates the entire flags word, not just whether *any*
        // valid bit is present.
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_NONBLOCK | 0x0008);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_rejects_all_high_bits() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), !0u32);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- (c) EINVAL — mutually exclusive GRND_RANDOM + GRND_INSECURE -----

    #[test]
    fn test_getrandom_rejects_random_plus_insecure() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret =
            crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_RANDOM | GRND_INSECURE);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_rejects_random_plus_insecure_plus_nonblock() {
        // Adding GRND_NONBLOCK to the mutually-exclusive pair must still
        // fail — the conflict check runs regardless of other valid bits.
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(
            buf.as_mut_ptr(),
            buf.len(),
            GRND_RANDOM | GRND_INSECURE | GRND_NONBLOCK,
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---- (d) Validation order -------------------------------------------

    #[test]
    fn test_getrandom_flag_check_precedes_null_check() {
        // Linux returns EINVAL for invalid flags before ever dereferencing
        // the buffer.  A null buffer plus an invalid flag must report
        // EINVAL (the flag error), not EFAULT.
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 16, 0xDEAD_BEEF);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_conflict_check_precedes_null_check() {
        // Same idea for the GRND_RANDOM|GRND_INSECURE conflict — it must
        // surface as EINVAL before the null-buf EFAULT path runs.
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 16, GRND_RANDOM | GRND_INSECURE);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_null_check_precedes_overflow_check() {
        // With invalid (null) buf AND overflowing length but valid flags,
        // the null check is reached first → EFAULT, not EINVAL.
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), usize::MAX, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ---- (e) EFAULT — null buffer with non-zero length ------------------

    #[test]
    fn test_getrandom_null_nonzero_len_efault() {
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // ---- (f) Success — each valid flag combination ----------------------

    #[test]
    fn test_getrandom_insecure_only_succeeds() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_INSECURE);
        assert_eq!(ret, buf.len() as isize);
    }

    #[test]
    fn test_getrandom_nonblock_plus_random_succeeds() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret =
            crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_NONBLOCK | GRND_RANDOM);
        assert_eq!(ret, buf.len() as isize);
    }

    #[test]
    fn test_getrandom_nonblock_plus_insecure_succeeds() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret =
            crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_NONBLOCK | GRND_INSECURE);
        assert_eq!(ret, buf.len() as isize);
    }

    #[test]
    fn test_getrandom_all_valid_flags_succeed_individually() {
        // GRND_RANDOM and GRND_INSECURE alone (each) are valid; only their
        // *combination* is rejected.  Exercise both individually.
        let mut buf = small_buf();
        for f in [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE] {
            crate::errno::set_errno(0);
            let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), f);
            assert_eq!(ret, buf.len() as isize, "flag {f:#x} should succeed");
        }
    }

    // ---- (g) Workflow / buggy-caller patterns ---------------------------

    #[test]
    fn test_getrandom_zero_len_with_valid_flag_succeeds() {
        let mut buf = small_buf();
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), 0, GRND_NONBLOCK);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_getrandom_null_zero_len_with_valid_flag_succeeds() {
        // Null + zero len + any valid flag = no-op success.
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 0, GRND_NONBLOCK);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_getrandom_null_zero_len_with_invalid_flag_einval() {
        // Even a no-op zero-length call must reject unknown flag bits.
        crate::errno::set_errno(0);
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 0, 0x1_0000);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getrandom_errno_unchanged_on_success() {
        // Sentinel value: success must NOT clobber a pre-existing errno.
        let mut buf = small_buf();
        crate::errno::set_errno(0xBEEF);
        let ret = crate::unistd::getrandom(buf.as_mut_ptr(), buf.len(), GRND_NONBLOCK);
        assert_eq!(ret, buf.len() as isize);
        assert_eq!(crate::errno::get_errno(), 0xBEEF);
    }

    #[test]
    fn test_getrandom_returns_full_buflen_on_success() {
        // Sanity: we never short-read.  Whatever length the caller asks
        // for (within range), they get back on success.
        let mut buf = [0u8; 64];
        for &len in &[1usize, 7, 16, 33, 64] {
            crate::errno::set_errno(0);
            let ret = crate::unistd::getrandom(buf.as_mut_ptr(), len, 0);
            assert_eq!(ret, len as isize);
        }
    }
}
