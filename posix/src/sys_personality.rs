//! `<sys/personality.h>` — process execution domain.
//!
//! The `personality()` system call controls the execution domain
//! of a process.  This affects signal handling, system call
//! behavior, and virtual address space layout.  It is Linux-specific.

// ---------------------------------------------------------------------------
// Re-export the function from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::personality;

// ---------------------------------------------------------------------------
// Personality flags
// ---------------------------------------------------------------------------

/// Default Linux execution domain.
pub const PER_LINUX: u32 = 0x0000;

/// Linux with 32-bit compatibility.
pub const PER_LINUX32: u32 = 0x0008;

/// SVR4 execution domain.
pub const PER_SVR4: u32 = 0x0001;

/// SVR3 execution domain.
pub const PER_SVR3: u32 = 0x0002;

/// SCO Unix execution domain.
pub const PER_SCOSVR3: u32 = 0x0003;

/// OSR5 execution domain.
pub const PER_OSR5: u32 = 0x0003;

/// BSD execution domain.
pub const PER_BSD: u32 = 0x0006;

/// FreeBSD execution domain.
pub const PER_FREEBSD: u32 = 0x0006;

/// Xenix execution domain.
pub const PER_XENIX: u32 = 0x0007;

/// Linux with 32-bit emulation.
pub const PER_LINUX32_3GB: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Personality modification bits
// ---------------------------------------------------------------------------

/// Use short inode numbers.
pub const SHORT_INODE: u32 = 0x1000000;

/// Use sticky bit for executables.
pub const STICKY_TIMEOUTS: u32 = 0x4000000;

/// Disable address space layout randomization.
pub const ADDR_NO_RANDOMIZE: u32 = 0x0040000;

/// Disable ASLR mmap randomization.
pub const MMAP_PAGE_ZERO: u32 = 0x0100000;

/// Limit address space to 3 GB (32-bit compat).
pub const ADDR_COMPAT_LAYOUT: u32 = 0x0200000;

/// Read implies exec (legacy behavior).
pub const READ_IMPLIES_EXEC: u32 = 0x0400000;

/// Limit stack to 32-bit address range.
pub const ADDR_LIMIT_32BIT: u32 = 0x0800000;

/// Limit stack to 3 GB.
pub const ADDR_LIMIT_3GB: u32 = 0x8000000;

/// Whole address space (no limits).
pub const WHOLE_SECONDS: u32 = 0x2000000;

// personality() function is re-exported from unistd above.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_linux() {
        assert_eq!(PER_LINUX, 0);
    }

    #[test]
    fn test_per_linux32() {
        assert_eq!(PER_LINUX32, 0x0008);
    }

    #[test]
    fn test_personality_query() {
        let result = personality(0xFFFFFFFF);
        assert_eq!(result, PER_LINUX as i32);
    }

    #[test]
    fn test_personality_set() {
        let result = personality(PER_LINUX as u64);
        assert_eq!(result, PER_LINUX as i32);
    }

    #[test]
    fn test_addr_no_randomize() {
        assert_eq!(ADDR_NO_RANDOMIZE, 0x0040000);
    }

    #[test]
    fn test_modification_bits_distinct() {
        let bits = [
            SHORT_INODE, STICKY_TIMEOUTS, ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO, ADDR_COMPAT_LAYOUT, READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(
                    bits[i], bits[j],
                    "personality bits must be distinct"
                );
            }
        }
    }

    #[test]
    fn test_modification_bits_are_flags() {
        let bits = [
            SHORT_INODE, STICKY_TIMEOUTS, ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO, ADDR_COMPAT_LAYOUT, READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
        ];
        for &b in &bits {
            assert_ne!(b, 0);
            assert_eq!(b & (b - 1), 0, "bit 0x{b:X} is not a power of two");
        }
    }
}
