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
    fn test_getrandom_null_buffer() {
        let ret = crate::unistd::getrandom(core::ptr::null_mut(), 0, 0);
        // Null buffer → EFAULT, returns -1.
        assert_eq!(ret, -1);
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
}
