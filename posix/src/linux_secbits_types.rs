//! `<linux/securebits.h>` — Secure bits flag constants.
//!
//! Securebits control how the kernel handles capabilities for
//! processes that change UIDs. They determine whether capabilities
//! are preserved or dropped on UID transitions, and whether the
//! root user retains ambient authority.

// ---------------------------------------------------------------------------
// Securebits flags
// ---------------------------------------------------------------------------

/// Don't grant capabilities to root processes.
pub const SECBIT_NOROOT: u32 = 1 << 0;
/// Lock NOROOT setting (irreversible).
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << 1;
/// Don't drop capabilities on setuid(non-0).
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
/// Lock NO_SETUID_FIXUP (irreversible).
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;
/// Keep capabilities on uid=0 → uid≠0 transition.
pub const SECBIT_KEEP_CAPS: u32 = 1 << 4;
/// Lock KEEP_CAPS (irreversible).
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;
/// Don't raise ambient capabilities on exec.
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
/// Lock NO_CAP_AMBIENT_RAISE (irreversible).
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Combined lock masks
// ---------------------------------------------------------------------------

/// All base flags (without locks).
pub const SECURE_ALL_BITS: u32 = SECBIT_NOROOT
    | SECBIT_NO_SETUID_FIXUP
    | SECBIT_KEEP_CAPS
    | SECBIT_NO_CAP_AMBIENT_RAISE;

/// All lock flags.
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
    fn test_secbits_power_of_two() {
        let bits = [
            SECBIT_NOROOT, SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP, SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS, SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE, SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for b in &bits {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_secbits_no_overlap() {
        let bits = [
            SECBIT_NOROOT, SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP, SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS, SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE, SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_all_bits_and_locks_no_overlap() {
        assert_eq!(SECURE_ALL_BITS & SECURE_ALL_LOCKS, 0);
    }

    #[test]
    fn test_lock_is_adjacent_bit() {
        assert_eq!(SECBIT_NOROOT_LOCKED, SECBIT_NOROOT << 1);
        assert_eq!(SECBIT_NO_SETUID_FIXUP_LOCKED, SECBIT_NO_SETUID_FIXUP << 1);
        assert_eq!(SECBIT_KEEP_CAPS_LOCKED, SECBIT_KEEP_CAPS << 1);
    }

    #[test]
    fn test_noroot_is_bit0() {
        assert_eq!(SECBIT_NOROOT, 1);
    }
}
