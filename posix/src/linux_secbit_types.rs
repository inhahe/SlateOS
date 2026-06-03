//! `<linux/securebits.h>` — Secure bits constants.
//!
//! Securebits constants covering capability inheritance,
//! no-root mode, and keep-caps flags.

// ---------------------------------------------------------------------------
// Securebits flags (SECBIT_*)
// ---------------------------------------------------------------------------

/// No setuid fixup.
pub const SECBIT_NOROOT: u32 = 1 << 0;
/// Lock no-root.
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << 1;
/// No setuid fixup (suid bit does not grant caps).
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
/// Lock no-setuid-fixup.
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;
/// Keep capabilities across setuid.
pub const SECBIT_KEEP_CAPS: u32 = 1 << 4;
/// Lock keep-caps.
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;
/// No capability ambient raise.
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
/// Lock no-cap-ambient-raise.
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Secure bits masks
// ---------------------------------------------------------------------------

/// All non-locked bits.
pub const SECURE_ALL_BITS: u32 =
    SECBIT_NOROOT | SECBIT_NO_SETUID_FIXUP | SECBIT_KEEP_CAPS | SECBIT_NO_CAP_AMBIENT_RAISE;
/// All locked bits.
pub const SECURE_ALL_LOCKS: u32 = SECBIT_NOROOT_LOCKED
    | SECBIT_NO_SETUID_FIXUP_LOCKED
    | SECBIT_KEEP_CAPS_LOCKED
    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

// ---------------------------------------------------------------------------
// Securebits issue flags (prctl)
// ---------------------------------------------------------------------------

/// Issue: no new privs.
pub const SECURE_NOROOT: u32 = 0;
/// Issue: no setuid fixup.
pub const SECURE_NO_SETUID_FIXUP: u32 = 2;
/// Issue: keep caps.
pub const SECURE_KEEP_CAPS: u32 = 4;
/// Issue: no cap ambient raise.
pub const SECURE_NO_CAP_AMBIENT_RAISE: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secbits_power_of_two() {
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
            assert!(b.is_power_of_two(), "0x{:02x} not power of two", b);
        }
    }

    #[test]
    fn test_secbits_no_overlap() {
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
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_all_bits_mask() {
        assert_eq!(
            SECURE_ALL_BITS,
            SECBIT_NOROOT | SECBIT_NO_SETUID_FIXUP | SECBIT_KEEP_CAPS | SECBIT_NO_CAP_AMBIENT_RAISE
        );
    }

    #[test]
    fn test_all_locks_mask() {
        assert_eq!(
            SECURE_ALL_LOCKS,
            SECBIT_NOROOT_LOCKED
                | SECBIT_NO_SETUID_FIXUP_LOCKED
                | SECBIT_KEEP_CAPS_LOCKED
                | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED
        );
    }

    #[test]
    fn test_bits_and_locks_no_overlap() {
        assert_eq!(SECURE_ALL_BITS & SECURE_ALL_LOCKS, 0);
    }

    #[test]
    fn test_issue_flags_distinct() {
        let flags = [
            SECURE_NOROOT,
            SECURE_NO_SETUID_FIXUP,
            SECURE_KEEP_CAPS,
            SECURE_NO_CAP_AMBIENT_RAISE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
