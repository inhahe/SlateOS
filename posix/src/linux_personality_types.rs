//! `<linux/personality.h>` — Process execution domain/personality constants.
//!
//! The personality system call sets per-process flags that modify
//! how the kernel handles certain system calls (e.g., address space
//! layout, signal behavior). This allows binary compatibility with
//! other Unix systems and controlled mmap/brk behavior.

// ---------------------------------------------------------------------------
// Personality base types (low byte)
// ---------------------------------------------------------------------------

/// Standard Linux personality.
pub const PER_LINUX: u32 = 0x0000;
/// SVR4 Unix personality.
pub const PER_SVR4: u32 = 0x0001;
/// SVR3 Unix personality.
pub const PER_SVR3: u32 = 0x0002;
/// SCO OpenServer personality.
pub const PER_SCOSVR3: u32 = 0x0003;
/// OSR5 personality.
pub const PER_OSR5: u32 = 0x0003;
/// BSD personality (FreeBSD-like).
pub const PER_BSD: u32 = 0x0006;
/// Linux 32-bit on 64-bit.
pub const PER_LINUX32: u32 = 0x0008;
/// IRIX5 personality.
pub const PER_IRIX32: u32 = 0x0009;
/// Solaris personality.
pub const PER_SOLARIS: u32 = 0x000D;
/// UW7 (UnixWare 7) personality.
pub const PER_UW7: u32 = 0x000E;
/// HP-UX personality.
pub const PER_HPUX: u32 = 0x000F;

// ---------------------------------------------------------------------------
// Personality flag bits (high bytes, OR'd with base)
// ---------------------------------------------------------------------------

/// Disable ASLR (Address Space Layout Randomization).
pub const ADDR_NO_RANDOMIZE: u32 = 0x0004_0000;
/// Limit mmap base to low 32-bit address space.
pub const MMAP_PAGE_ZERO: u32 = 0x0010_0000;
/// Use legacy virtual address space layout.
pub const ADDR_COMPAT_LAYOUT: u32 = 0x0020_0000;
/// Set read implies exec (legacy behavior).
pub const READ_IMPLIES_EXEC: u32 = 0x0040_0000;
/// Limit address space (brk randomization off).
pub const ADDR_LIMIT_32BIT: u32 = 0x0080_0000;
/// Short inode numbers (for old stat calls).
pub const SHORT_INODE: u32 = 0x0100_0000;
/// Use whole seconds for stat timestamps.
pub const WHOLE_SECONDS: u32 = 0x0200_0000;
/// Sticky bit grants delete permission.
pub const STICKY_TIMEOUTS: u32 = 0x0400_0000;
/// Limit to 3GB address space.
pub const ADDR_LIMIT_3GB: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Query current personality without changing it.
pub const PERSONALITY_QUERY: u32 = 0xFFFF_FFFF;
/// Personality type mask (low byte).
pub const PER_MASK: u32 = 0x00FF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_types_fit_mask() {
        let bases = [
            PER_LINUX,
            PER_SVR4,
            PER_SVR3,
            PER_BSD,
            PER_LINUX32,
            PER_IRIX32,
            PER_SOLARIS,
            PER_UW7,
            PER_HPUX,
        ];
        for b in bases {
            assert_eq!(b & !PER_MASK, 0);
        }
    }

    #[test]
    fn test_flags_above_mask() {
        let flags = [
            ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
            SHORT_INODE,
            WHOLE_SECONDS,
            STICKY_TIMEOUTS,
            ADDR_LIMIT_3GB,
        ];
        for f in flags {
            assert_eq!(f & PER_MASK, 0);
            assert_ne!(f, 0);
        }
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
            SHORT_INODE,
            WHOLE_SECONDS,
            STICKY_TIMEOUTS,
            ADDR_LIMIT_3GB,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_linux_is_zero() {
        assert_eq!(PER_LINUX, 0);
    }
}
