//! `<linux/cred.h>` — task credential constants.
//!
//! Every Linux task carries a `cred` structure recording UIDs/GIDs,
//! supplementary groups, and a capability bitmap. POSIX setresuid()
//! plus the Linux-specific KEEPCAPS securebits govern transitions.

// ---------------------------------------------------------------------------
// Reserved UIDs / GIDs
// ---------------------------------------------------------------------------

pub const UID_ROOT: u32 = 0;
pub const GID_ROOT: u32 = 0;
/// Sentinel meaning "leave this field unchanged" in setresuid/setresgid.
pub const UID_UNCHANGED: u32 = u32::MAX;
pub const GID_UNCHANGED: u32 = u32::MAX;
/// Highest valid UID on a typical 32-bit-UID system (also NOBODY conv.).
pub const UID_OVERFLOW: u32 = 65534;
pub const GID_OVERFLOW: u32 = 65534;
/// Sentinel used by 16-bit-uid syscalls to mean "no change".
pub const UID16_UNCHANGED: u16 = u16::MAX;

// ---------------------------------------------------------------------------
// Supplementary groups
// ---------------------------------------------------------------------------

/// Default kernel maximum (NGROUPS_MAX).
pub const NGROUPS_MAX: u32 = 65_536;
/// Small static buffer historically used by libc.
pub const NGROUPS_SMALL: usize = 32;

// ---------------------------------------------------------------------------
// securebits flags (prctl(PR_SET_SECUREBITS))
// ---------------------------------------------------------------------------

pub const SECBIT_NOROOT: u32 = 1 << 0;
pub const SECBIT_NOROOT_LOCKED: u32 = 1 << 1;
pub const SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;
pub const SECBIT_KEEP_CAPS: u32 = 1 << 4;
pub const SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;
pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

/// All securebits OR'd together (low 8 bits).
pub const SECBITS_ALL: u32 = 0xFF;

// ---------------------------------------------------------------------------
// Capability set sizes
// ---------------------------------------------------------------------------

/// Number of capabilities supported by current kernels (CAP_LAST_CAP+1).
pub const CAP_NCAPS: u32 = 41;
/// Bytes per capability bitmap (rounded up).
pub const CAP_BITMAP_BYTES: usize = ((CAP_NCAPS as usize) + 7) / 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_ids_are_zero() {
        assert_eq!(UID_ROOT, 0);
        assert_eq!(GID_ROOT, 0);
    }

    #[test]
    fn test_unchanged_sentinels_are_all_ones() {
        assert_eq!(UID_UNCHANGED, u32::MAX);
        assert_eq!(GID_UNCHANGED, u32::MAX);
        assert_eq!(UID16_UNCHANGED, u16::MAX);
    }

    #[test]
    fn test_overflow_ids_are_65534() {
        assert_eq!(UID_OVERFLOW, 65534);
        assert_eq!(GID_OVERFLOW, 65534);
    }

    #[test]
    fn test_ngroups_geometry() {
        assert_eq!(NGROUPS_MAX, 65_536);
        assert!(NGROUPS_MAX.is_power_of_two());
        assert!(NGROUPS_SMALL < (NGROUPS_MAX as usize));
    }

    #[test]
    fn test_securebits_distinct_single_bit() {
        let s = [
            SECBIT_NOROOT,
            SECBIT_NOROOT_LOCKED,
            SECBIT_NO_SETUID_FIXUP,
            SECBIT_NO_SETUID_FIXUP_LOCKED,
            SECBIT_KEEP_CAPS,
            SECBIT_KEEP_CAPS_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE,
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
        ];
        for (i, &x) in s.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &s[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        let or = s.iter().copied().fold(0u32, |a, b| a | b);
        assert_eq!(or, SECBITS_ALL);
    }

    #[test]
    fn test_securebits_lock_bits_paired() {
        // Each LOCKED bit is exactly 2x the corresponding feature bit.
        assert_eq!(SECBIT_NOROOT_LOCKED, SECBIT_NOROOT << 1);
        assert_eq!(SECBIT_NO_SETUID_FIXUP_LOCKED, SECBIT_NO_SETUID_FIXUP << 1);
        assert_eq!(SECBIT_KEEP_CAPS_LOCKED, SECBIT_KEEP_CAPS << 1);
        assert_eq!(
            SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED,
            SECBIT_NO_CAP_AMBIENT_RAISE << 1
        );
    }

    #[test]
    fn test_cap_bitmap_size() {
        assert_eq!(CAP_NCAPS, 41);
        // 41 bits → 6 bytes (rounding up).
        assert_eq!(CAP_BITMAP_BYTES, 6);
    }
}
