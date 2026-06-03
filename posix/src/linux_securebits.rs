//! `<linux/securebits.h>` — securebits constants.
//!
//! Securebits control the inheritance and retention of capabilities
//! across setuid transitions, privilege changes, and execve calls.
//! Used by `prctl(PR_SET_SECUREBITS, ...)` and `prctl(PR_GET_SECUREBITS)`.

// ---------------------------------------------------------------------------
// Securebits
// ---------------------------------------------------------------------------

/// When set, UID 0 does not grant capabilities on setuid transitions.
pub const SECBIT_NOROOT: u32 = 1 << 0;
/// Lock NOROOT bit (cannot be changed once set).
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << 1;

/// When set, keep capabilities on UID change from 0 to non-0.
pub const SECBIT_KEEP_CAPS: u32 = 1 << 4;
/// Lock KEEP_CAPS.
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;

/// When set, don't grant ambient capabilities.
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
/// Lock NO_SETUID_FIXUP.
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;

/// When set, ambient capabilities are disabled entirely.
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
/// Lock NO_CAP_AMBIENT_RAISE.
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Convenience masks
// ---------------------------------------------------------------------------

/// All securebits (for setting/querying).
pub const SECURE_ALL_BITS: u32 = SECBIT_NOROOT
    | SECBIT_NOROOT_LOCKED
    | SECBIT_NO_SETUID_FIXUP
    | SECBIT_NO_SETUID_FIXUP_LOCKED
    | SECBIT_KEEP_CAPS
    | SECBIT_KEEP_CAPS_LOCKED
    | SECBIT_NO_CAP_AMBIENT_RAISE
    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

/// All lock bits.
pub const SECURE_ALL_LOCKS: u32 = SECBIT_NOROOT_LOCKED
    | SECBIT_NO_SETUID_FIXUP_LOCKED
    | SECBIT_KEEP_CAPS_LOCKED
    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secbits_are_powers_of_two() {
        let bits = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for b in &bits {
            assert!(b.is_power_of_two(), "secbit {b:#x} not power of 2");
        }
    }

    #[test]
    fn test_secbits_distinct() {
        let bits = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_lock_bits_paired() {
        // Each lock bit is one position above its corresponding feature bit.
        assert_eq!(SECBIT_NOROOT_LOCKED, SECBIT_NOROOT << 1);
        assert_eq!(SECBIT_NO_SETUID_FIXUP_LOCKED, SECBIT_NO_SETUID_FIXUP << 1);
        assert_eq!(SECBIT_KEEP_CAPS_LOCKED, SECBIT_KEEP_CAPS << 1);
        assert_eq!(
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE << 1
        );
    }

    #[test]
    fn test_all_bits_mask() {
        assert_eq!(SECURE_ALL_BITS, 0xFF);
    }

    #[test]
    fn test_all_locks_in_all_bits() {
        assert_eq!(SECURE_ALL_LOCKS & SECURE_ALL_BITS, SECURE_ALL_LOCKS);
    }
}
