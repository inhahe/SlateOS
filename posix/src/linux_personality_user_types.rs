//! `<sys/personality.h>` — execution-domain ABI selector.
//!
//! `personality(2)` lets a process opt into legacy execution domains
//! (SCO, BSD, SunOS, etc.) and toggle quirks like ASLR, brk-region
//! placement, and "read implies exec". Real-world use today is mostly
//! the quirk bits: `setarch -R foo` flips `ADDR_NO_RANDOMIZE` to make
//! older programs deterministic.

// ---------------------------------------------------------------------------
// Personality domains (`enum personality`)
// ---------------------------------------------------------------------------

pub const PER_LINUX: u32 = 0x0000;
pub const PER_LINUX_32BIT: u32 = 0x0008;
pub const PER_LINUX_FDPIC: u32 = 0x0008_0000;
pub const PER_SVR4: u32 = 0x0001 | 0x0040_0000;
pub const PER_SVR3: u32 = 0x0002 | 0x0040_0000;
pub const PER_SCOSVR3: u32 = 0x0003 | 0x0080_0000 | 0x0040_0000;
pub const PER_OSR5: u32 = 0x0003 | 0x0080_0000 | 0x0100_0000;
pub const PER_WYSEV386: u32 = 0x0004 | 0x0040_0000;
pub const PER_ISCR4: u32 = 0x0005 | 0x0040_0000;
pub const PER_BSD: u32 = 0x0006;
pub const PER_SUNOS: u32 = 0x0006 | 0x0080_0000;
pub const PER_XENIX: u32 = 0x0007 | 0x0040_0000;
pub const PER_LINUX32: u32 = 0x0008;
pub const PER_LINUX32_3GB: u32 = 0x0008 | 0x8000_0000;
pub const PER_IRIX32: u32 = 0x0009 | 0x0080_0000;
pub const PER_IRIXN32: u32 = 0x000A | 0x0080_0000;
pub const PER_IRIX64: u32 = 0x000B | 0x0080_0000;
pub const PER_RISCOS: u32 = 0x000C;
pub const PER_SOLARIS: u32 = 0x000D | 0x0080_0000;
pub const PER_UW7: u32 = 0x000E | 0x0040_0000;
pub const PER_OSF4: u32 = 0x000F;
pub const PER_HPUX: u32 = 0x0010;
pub const PER_MASK: u32 = 0x00FF;

// ---------------------------------------------------------------------------
// Quirk bits (OR'd with the domain)
// ---------------------------------------------------------------------------

pub const UNAME26: u32 = 0x0020_0000;
pub const ADDR_NO_RANDOMIZE: u32 = 0x0004_0000;
pub const FDPIC_FUNCPTRS: u32 = 0x0008_0000;
pub const MMAP_PAGE_ZERO: u32 = 0x0010_0000;
pub const ADDR_COMPAT_LAYOUT: u32 = 0x0200_0000;
pub const READ_IMPLIES_EXEC: u32 = 0x0040_0000;
pub const ADDR_LIMIT_32BIT: u32 = 0x0080_0000;
pub const SHORT_INODE: u32 = 0x0100_0000;
pub const WHOLE_SECONDS: u32 = 0x0400_0000;
pub const STICKY_TIMEOUTS: u32 = 0x0800_0000;
pub const ADDR_LIMIT_3GB: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Sentinel returned by `personality(0xFFFFFFFF)` to read without setting.
// ---------------------------------------------------------------------------

pub const PERSONALITY_READ: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Syscall
// ---------------------------------------------------------------------------

pub const NR_PERSONALITY: u32 = 135;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_linux_personality_zero() {
        // The most common case: a plain Linux process has personality 0.
        assert_eq!(PER_LINUX, 0);
    }

    #[test]
    fn test_per_mask_covers_low_byte() {
        // The domain selector lives in the low 8 bits; quirk flags occupy
        // higher bits.
        assert_eq!(PER_MASK, 0xFF);
    }

    #[test]
    fn test_quirk_bits_single_bit() {
        let q = [
            UNAME26,
            ADDR_NO_RANDOMIZE,
            FDPIC_FUNCPTRS,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
            SHORT_INODE,
            WHOLE_SECONDS,
            STICKY_TIMEOUTS,
            ADDR_LIMIT_3GB,
        ];
        for v in q {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_quirk_bits_distinct() {
        let q = [
            UNAME26,
            ADDR_NO_RANDOMIZE,
            FDPIC_FUNCPTRS,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
            SHORT_INODE,
            WHOLE_SECONDS,
            STICKY_TIMEOUTS,
            ADDR_LIMIT_3GB,
        ];
        for i in 0..q.len() {
            for j in (i + 1)..q.len() {
                assert_ne!(q[i], q[j]);
            }
        }
    }

    #[test]
    fn test_addr_limit_3gb_is_high_bit() {
        // ADDR_LIMIT_3GB picks bit 31 — distinct from the rest, which
        // cluster between bits 17 and 27.
        assert_eq!(ADDR_LIMIT_3GB, 1 << 31);
    }

    #[test]
    fn test_compound_domains_share_bit_patterns() {
        // PER_SUNOS = PER_BSD | ADDR_LIMIT_32BIT — the BSD compat layer
        // with 32-bit address limits set.
        assert_eq!(PER_SUNOS, PER_BSD | ADDR_LIMIT_32BIT);
        // PER_LINUX32_3GB = PER_LINUX32 with the 3 GiB limit bit.
        assert_eq!(PER_LINUX32_3GB, PER_LINUX32 | ADDR_LIMIT_3GB);
    }

    #[test]
    fn test_read_sentinel_is_all_ones() {
        // Passing 0xFFFFFFFF reads the current personality without
        // changing it.
        assert_eq!(PERSONALITY_READ, u32::MAX);
    }

    #[test]
    fn test_syscall_number() {
        assert_eq!(NR_PERSONALITY, 135);
    }
}
