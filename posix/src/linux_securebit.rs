//! `<linux/securebits.h>` — Secure bits for capability handling.
//!
//! Secure bits control how the kernel handles capabilities during
//! UID transitions (setuid). They allow disabling the legacy
//! set-user-ID-root mechanism so capabilities are the sole
//! privilege model.

// ---------------------------------------------------------------------------
// Secure bit indices
// ---------------------------------------------------------------------------

/// Don't grant capabilities when executing setuid-root programs.
pub const SECUREBITS_NOROOT: u32 = 0;
/// Lock the NOROOT setting.
pub const SECUREBITS_NOROOT_LOCKED: u32 = 1;
/// Don't drop capabilities when switching from root to non-root UID.
pub const SECUREBITS_NO_SETUID_FIXUP: u32 = 2;
/// Lock the NO_SETUID_FIXUP setting.
pub const SECUREBITS_NO_SETUID_FIXUP_LOCKED: u32 = 3;
/// Keep capabilities across exec (ambient capabilities).
pub const SECUREBITS_KEEP_CAPS: u32 = 4;
/// Lock the KEEP_CAPS setting.
pub const SECUREBITS_KEEP_CAPS_LOCKED: u32 = 5;
/// Prevent privilege escalation via no_new_privs.
pub const SECUREBITS_NO_CAP_AMBIENT_RAISE: u32 = 6;
/// Lock the NO_CAP_AMBIENT_RAISE setting.
pub const SECUREBITS_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 7;

// ---------------------------------------------------------------------------
// Secure bit flags (1 << index)
// ---------------------------------------------------------------------------

/// NOROOT flag value.
pub const SECBIT_NOROOT: u32 = 1 << SECUREBITS_NOROOT;
/// NOROOT_LOCKED flag value.
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << SECUREBITS_NOROOT_LOCKED;
/// NO_SETUID_FIXUP flag value.
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << SECUREBITS_NO_SETUID_FIXUP;
/// NO_SETUID_FIXUP_LOCKED flag value.
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << SECUREBITS_NO_SETUID_FIXUP_LOCKED;
/// KEEP_CAPS flag value.
pub const SECBIT_KEEP_CAPS: u32 = 1 << SECUREBITS_KEEP_CAPS;
/// KEEP_CAPS_LOCKED flag value.
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << SECUREBITS_KEEP_CAPS_LOCKED;
/// NO_CAP_AMBIENT_RAISE flag value.
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << SECUREBITS_NO_CAP_AMBIENT_RAISE;
/// NO_CAP_AMBIENT_RAISE_LOCKED flag value.
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << SECUREBITS_NO_CAP_AMBIENT_RAISE_LOCKED;

// ---------------------------------------------------------------------------
// Combined masks
// ---------------------------------------------------------------------------

/// All settable secure bits.
pub const SECURE_ALL_BITS: u32 =
    SECBIT_NOROOT | SECBIT_NO_SETUID_FIXUP | SECBIT_KEEP_CAPS | SECBIT_NO_CAP_AMBIENT_RAISE;

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
    fn test_bit_indices_distinct() {
        let indices = [
            SECUREBITS_NOROOT,
            SECUREBITS_NOROOT_LOCKED,
            SECUREBITS_NO_SETUID_FIXUP,
            SECUREBITS_NO_SETUID_FIXUP_LOCKED,
            SECUREBITS_KEEP_CAPS,
            SECUREBITS_KEEP_CAPS_LOCKED,
            SECUREBITS_NO_CAP_AMBIENT_RAISE,
            SECUREBITS_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_lock_follows_setting() {
        // Each lock bit is one position above its setting bit
        assert_eq!(SECUREBITS_NOROOT_LOCKED, SECUREBITS_NOROOT + 1);
        assert_eq!(
            SECUREBITS_NO_SETUID_FIXUP_LOCKED,
            SECUREBITS_NO_SETUID_FIXUP + 1
        );
        assert_eq!(SECUREBITS_KEEP_CAPS_LOCKED, SECUREBITS_KEEP_CAPS + 1);
        assert_eq!(
            SECUREBITS_NO_CAP_AMBIENT_RAISE_LOCKED,
            SECUREBITS_NO_CAP_AMBIENT_RAISE + 1
        );
    }

    #[test]
    fn test_all_bits_mask() {
        assert_eq!(SECURE_ALL_BITS & SECURE_ALL_LOCKS, 0);
    }

    #[test]
    fn test_combined_coverage() {
        let all = SECURE_ALL_BITS | SECURE_ALL_LOCKS;
        assert_eq!(all, 0xFF);
    }
}
