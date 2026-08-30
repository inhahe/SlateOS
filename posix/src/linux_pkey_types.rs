//! `<linux/pkeys.h>` — Memory protection keys (pkeys) constants.
//!
//! Protection keys (Intel MPK on x86-64, ARM PKEY) allow per-thread
//! access restriction on memory pages without modifying page tables.
//! Each page can be tagged with a 4-bit key (0-15), and a per-thread
//! register (PKRU on x86) controls read/write access to each key's pages.
//! This enables fast intra-process isolation without syscall overhead.

// ---------------------------------------------------------------------------
// Protection key limits
// ---------------------------------------------------------------------------

/// Maximum number of protection keys (hardware limit on x86-64).
pub const PKEY_MAX: u32 = 16;
/// Default protection key (key 0, always accessible).
pub const PKEY_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// pkey_alloc access rights flags
// ---------------------------------------------------------------------------

/// Disable access (read and write) to pages with this key.
pub const PKEY_DISABLE_ACCESS: u32 = 0x1;
/// Disable write access to pages with this key.
pub const PKEY_DISABLE_WRITE: u32 = 0x2;

// ---------------------------------------------------------------------------
// PKRU register bit layout (x86-64)
// ---------------------------------------------------------------------------

/// Bits per key in PKRU register.
pub const PKRU_BITS_PER_KEY: u32 = 2;
/// Access disable bit offset within key's PKRU bits.
pub const PKRU_AD_BIT: u32 = 0;
/// Write disable bit offset within key's PKRU bits.
pub const PKRU_WD_BIT: u32 = 1;
/// Mask for one key's worth of PKRU bits.
pub const PKRU_KEY_MASK: u32 = 0x3;

// ---------------------------------------------------------------------------
// pkey_mprotect flags (combined with mprotect PROT_* values)
// ---------------------------------------------------------------------------

/// No access.
pub const PROT_NONE: u32 = 0x0;
/// Read access.
pub const PROT_READ: u32 = 0x1;
/// Write access.
pub const PROT_WRITE: u32 = 0x2;
/// Execute access.
pub const PROT_EXEC: u32 = 0x4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkey_range() {
        assert_eq!(PKEY_DEFAULT, 0);
        assert_eq!(PKEY_MAX, 16);
    }

    #[test]
    fn test_disable_flags_no_overlap() {
        assert_eq!(PKEY_DISABLE_ACCESS & PKEY_DISABLE_WRITE, 0);
        assert!(PKEY_DISABLE_ACCESS.is_power_of_two());
        assert!(PKEY_DISABLE_WRITE.is_power_of_two());
    }

    #[test]
    fn test_pkru_layout() {
        assert_eq!(PKRU_BITS_PER_KEY, 2);
        assert_eq!(PKRU_KEY_MASK, (1 << PKRU_BITS_PER_KEY) - 1);
    }

    #[test]
    fn test_prot_flags_no_overlap() {
        let flags = [PROT_READ, PROT_WRITE, PROT_EXEC];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_prot_none_is_zero() {
        assert_eq!(PROT_NONE, 0);
    }
}
