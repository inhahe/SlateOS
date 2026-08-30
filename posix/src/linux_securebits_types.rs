//! `<linux/securebits.h>` — Secure bits (credential security flags).
//!
//! Securebits control how the kernel handles credential transitions
//! for a process. They restrict or allow certain privilege escalation
//! mechanisms: whether capabilities are kept across uid changes,
//! whether setuid-root programs gain capabilities, and whether the
//! root user gets special treatment. Each securebit has a corresponding
//! "locked" bit that prevents it from being changed (even by root).
//! These are per-thread settings inherited across fork/exec.

// ---------------------------------------------------------------------------
// Securebit flags
// ---------------------------------------------------------------------------

/// NOROOT: don't grant capabilities to setuid-root programs.
pub const SECBIT_NOROOT: u32 = 0x0000_0001;
/// NOROOT_LOCKED: lock the NOROOT bit (can't be cleared).
pub const SECBIT_NOROOT_LOCKED: u32 = 0x0000_0002;
/// NO_SETUID_FIXUP: don't adjust capabilities on uid change.
pub const SECBIT_NO_SETUID_FIXUP: u32 = 0x0000_0004;
/// NO_SETUID_FIXUP_LOCKED: lock the NO_SETUID_FIXUP bit.
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 0x0000_0008;
/// KEEP_CAPS: retain capabilities across setuid (deprecated, use PR_SET_KEEPCAPS).
pub const SECBIT_KEEP_CAPS: u32 = 0x0000_0010;
/// KEEP_CAPS_LOCKED: lock the KEEP_CAPS bit.
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 0x0000_0020;
/// NO_CAP_AMBIENT_RAISE: disallow raising ambient capabilities.
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 0x0000_0040;
/// NO_CAP_AMBIENT_RAISE_LOCKED: lock the NO_CAP_AMBIENT_RAISE bit.
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Composite masks
// ---------------------------------------------------------------------------

/// All securebits (all flags OR'd together).
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
    fn test_securebits_no_overlap() {
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
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_lock_bits_are_adjacent() {
        // Each lock bit is one position above its corresponding flag
        assert_eq!(SECBIT_NOROOT_LOCKED, SECBIT_NOROOT << 1);
        assert_eq!(SECBIT_NO_SETUID_FIXUP_LOCKED, SECBIT_NO_SETUID_FIXUP << 1);
        assert_eq!(SECBIT_KEEP_CAPS_LOCKED, SECBIT_KEEP_CAPS << 1);
        assert_eq!(
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE << 1
        );
    }

    #[test]
    fn test_all_bits_covers_all() {
        assert_eq!(SECURE_ALL_BITS, 0xFF);
    }

    #[test]
    fn test_all_locks_subset_of_all_bits() {
        assert_eq!(SECURE_ALL_LOCKS & SECURE_ALL_BITS, SECURE_ALL_LOCKS);
    }
}
