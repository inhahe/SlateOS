//! `<linux/personality.h>` — Additional personality constants.
//!
//! Supplementary personality constants covering execution domain flags,
//! personality types, and bug emulation bits.

// ---------------------------------------------------------------------------
// Personality flags (upper byte)
// ---------------------------------------------------------------------------

/// Use UNAME26 (report kernel as 2.6.x).
pub const UNAME26: u32 = 0x0020000;
/// Address limit 3GB.
pub const ADDR_LIMIT_3GB: u32 = 0x8000000;
/// Set short inode.
pub const SHORT_INODE: u32 = 0x1000000;
/// Whole seconds only for timestamps.
pub const WHOLE_SECONDS: u32 = 0x2000000;
/// Sticky timeouts.
pub const STICKY_TIMEOUTS: u32 = 0x4000000;
/// No address space layout randomization.
pub const ADDR_NO_RANDOMIZE: u32 = 0x0040000;
/// FDPIC (function descriptors).
pub const FDPIC_FUNCPTRS: u32 = 0x0080000;
/// Mmap page zero.
pub const MMAP_PAGE_ZERO: u32 = 0x0100000;
/// Address compatibility layout.
pub const ADDR_COMPAT_LAYOUT: u32 = 0x0200000;
/// Read implies exec.
pub const READ_IMPLIES_EXEC: u32 = 0x0400000;
/// Limit 32-bit address space.
pub const ADDR_LIMIT_32BIT: u32 = 0x0800000;

// ---------------------------------------------------------------------------
// Personality types (execution domains)
// ---------------------------------------------------------------------------

/// Linux personality (default).
pub const PER_LINUX: u32 = 0x0000;
/// Linux 32-bit personality.
pub const PER_LINUX32: u32 = 0x0008;
/// Linux 32-bit on 64-bit.
pub const PER_LINUX32_3GB: u32 = 0x0008 | 0x8000000;
/// SVR4 personality.
pub const PER_SVR4: u32 = 0x0001 | 0x2000000 | 0x4000000;
/// SVR3 personality.
pub const PER_SVR3: u32 = 0x0002 | 0x2000000 | 0x4000000;
/// SCO personality.
pub const PER_SCOSVR3: u32 = 0x0003 | 0x2000000 | 0x4000000;
/// OSR5 personality.
pub const PER_OSR5: u32 = 0x0003 | 0x2000000 | 0x4000000 | 0x0100000;
/// BSD personality.
pub const PER_BSD: u32 = 0x0006;
/// Xenix personality.
pub const PER_XENIX: u32 = 0x0007 | 0x1000000 | 0x2000000;

// ---------------------------------------------------------------------------
// Personality mask
// ---------------------------------------------------------------------------

/// Mask to extract personality type from flags.
pub const PER_MASK: u32 = 0x00FF;
/// Mask to extract flag bits.
pub const PER_FLAG_MASK: u32 = 0xFFFFFF00;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_bits_distinct() {
        let flags = [
            UNAME26, ADDR_LIMIT_3GB, SHORT_INODE,
            WHOLE_SECONDS, STICKY_TIMEOUTS, ADDR_NO_RANDOMIZE,
            FDPIC_FUNCPTRS, MMAP_PAGE_ZERO, ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC, ADDR_LIMIT_32BIT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_linux_default_is_zero() {
        assert_eq!(PER_LINUX, 0);
    }

    #[test]
    fn test_personality_mask() {
        assert_eq!(PER_MASK & PER_FLAG_MASK, 0);
    }

    #[test]
    fn test_per_linux32_base() {
        assert_eq!(PER_LINUX32 & PER_MASK, 0x0008);
    }

    #[test]
    fn test_per_linux32_3gb_includes_addr_limit() {
        assert_ne!(PER_LINUX32_3GB & ADDR_LIMIT_3GB, 0);
    }
}
