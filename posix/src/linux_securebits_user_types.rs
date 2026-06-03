//! `<linux/securebits.h>` — per-process `securebits` flags.
//!
//! Linux capability inheritance is governed by a per-process
//! `securebits` word. Setuid-root binaries can opt out of legacy
//! Unix uid-zero special-casing by setting these bits via
//! `prctl(PR_SET_SECUREBITS, …)`. systemd uses this to run
//! services as root *without* the historic "uid 0 = bypass
//! capabilities" semantics.

// ---------------------------------------------------------------------------
// Securebits field layout
// ---------------------------------------------------------------------------

// Each effective bit comes paired with a "locked" bit. The "locked"
// bit prevents the effective bit from being cleared again. The
// bit-pair pattern is `(EFFECTIVE | LOCKED)` per "issue".

pub const SECURE_NOROOT: u32 = 0;
pub const SECURE_NOROOT_LOCKED: u32 = 1;
pub const SECURE_NO_SETUID_FIXUP: u32 = 2;
pub const SECURE_NO_SETUID_FIXUP_LOCKED: u32 = 3;
pub const SECURE_KEEP_CAPS: u32 = 4;
pub const SECURE_KEEP_CAPS_LOCKED: u32 = 5;
pub const SECURE_NO_CAP_AMBIENT_RAISE: u32 = 6;
pub const SECURE_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 7;

// ---------------------------------------------------------------------------
// Convenience masks (`SECBIT_*` form: 1 << bit)
// ---------------------------------------------------------------------------

pub const SECBIT_NOROOT: u32 = 1 << SECURE_NOROOT;
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << SECURE_NOROOT_LOCKED;
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << SECURE_NO_SETUID_FIXUP;
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << SECURE_NO_SETUID_FIXUP_LOCKED;
pub const SECBIT_KEEP_CAPS: u32 = 1 << SECURE_KEEP_CAPS;
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << SECURE_KEEP_CAPS_LOCKED;
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << SECURE_NO_CAP_AMBIENT_RAISE;
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << SECURE_NO_CAP_AMBIENT_RAISE_LOCKED;

/// All bits currently defined — used by the kernel to reject
/// unknown bits on `PR_SET_SECUREBITS`.
pub const SECURE_ALL_BITS: u32 = SECBIT_NOROOT
    | SECBIT_NO_SETUID_FIXUP
    | SECBIT_KEEP_CAPS
    | SECBIT_NO_CAP_AMBIENT_RAISE;
pub const SECURE_ALL_LOCKS: u32 = SECBIT_NOROOT_LOCKED
    | SECBIT_NO_SETUID_FIXUP_LOCKED
    | SECBIT_KEEP_CAPS_LOCKED
    | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

/// Combined mask of every defined bit (effective + locked).
pub const SECURE_ALL_UNPRIVILEGED: u32 = SECURE_ALL_BITS | SECURE_ALL_LOCKS;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_indices_dense_0_to_7() {
        let b = [
            SECURE_NOROOT,
            SECURE_NOROOT_LOCKED,
            SECURE_NO_SETUID_FIXUP,
            SECURE_NO_SETUID_FIXUP_LOCKED,
            SECURE_KEEP_CAPS,
            SECURE_KEEP_CAPS_LOCKED,
            SECURE_NO_CAP_AMBIENT_RAISE,
            SECURE_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_locked_pairs_at_odd_indices() {
        // Each LOCKED bit sits at the index immediately after its
        // effective bit.
        assert_eq!(SECURE_NOROOT_LOCKED, SECURE_NOROOT + 1);
        assert_eq!(SECURE_NO_SETUID_FIXUP_LOCKED, SECURE_NO_SETUID_FIXUP + 1);
        assert_eq!(SECURE_KEEP_CAPS_LOCKED, SECURE_KEEP_CAPS + 1);
        assert_eq!(
            SECURE_NO_CAP_AMBIENT_RAISE_LOCKED,
            SECURE_NO_CAP_AMBIENT_RAISE + 1
        );
    }

    #[test]
    fn test_secbit_masks_are_single_bits() {
        let m = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        let mut or = 0u32;
        for v in m {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // Eight dense bits → low byte covered.
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_all_unprivileged_covers_byte() {
        // ALL_UNPRIVILEGED is the union of effective + locked bits.
        assert_eq!(SECURE_ALL_UNPRIVILEGED, 0xFF);
        // Effective and locked masks are disjoint.
        assert_eq!(SECURE_ALL_BITS & SECURE_ALL_LOCKS, 0);
        // Effective bits sit at even positions (0, 2, 4, 6).
        assert_eq!(SECURE_ALL_BITS, (1 << 0) | (1 << 2) | (1 << 4) | (1 << 6));
        assert_eq!(SECURE_ALL_BITS, 0x55);
        assert_eq!(SECURE_ALL_LOCKS, 0xAA);
    }
}
