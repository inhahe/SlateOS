//! `<linux/mman.h>` — `madvise(2)` advice codes.
//!
//! `madvise(2)` is the hint channel between userspace and the kernel's
//! VM. glibc malloc uses MADV_DONTNEED to return freed pages, jemalloc
//! and TCMalloc use MADV_FREE, JVMs use MADV_HUGEPAGE for large heaps,
//! and CRIU uses MADV_DODUMP/DONTDUMP to control core-dump contents.

// ---------------------------------------------------------------------------
// Classic POSIX-style advice (0..6)
// ---------------------------------------------------------------------------

pub const MADV_NORMAL: u32 = 0;
pub const MADV_RANDOM: u32 = 1;
pub const MADV_SEQUENTIAL: u32 = 2;
pub const MADV_WILLNEED: u32 = 3;
pub const MADV_DONTNEED: u32 = 4;

// ---------------------------------------------------------------------------
// Linux-specific advice
// ---------------------------------------------------------------------------

pub const MADV_FREE: u32 = 8;
pub const MADV_REMOVE: u32 = 9;
pub const MADV_DONTFORK: u32 = 10;
pub const MADV_DOFORK: u32 = 11;
pub const MADV_MERGEABLE: u32 = 12;
pub const MADV_UNMERGEABLE: u32 = 13;
pub const MADV_HUGEPAGE: u32 = 14;
pub const MADV_NOHUGEPAGE: u32 = 15;
pub const MADV_DONTDUMP: u32 = 16;
pub const MADV_DODUMP: u32 = 17;
pub const MADV_WIPEONFORK: u32 = 18;
pub const MADV_KEEPONFORK: u32 = 19;
pub const MADV_COLD: u32 = 20;
pub const MADV_PAGEOUT: u32 = 21;
pub const MADV_POPULATE_READ: u32 = 22;
pub const MADV_POPULATE_WRITE: u32 = 23;
pub const MADV_DONTNEED_LOCKED: u32 = 24;
pub const MADV_COLLAPSE: u32 = 25;

// ---------------------------------------------------------------------------
// Hardware-poison injection (CONFIG_MEMORY_FAILURE)
// ---------------------------------------------------------------------------

pub const MADV_HWPOISON: u32 = 100;
pub const MADV_SOFT_OFFLINE: u32 = 101;

// ---------------------------------------------------------------------------
// `process_madvise(2)` flags (Linux 5.10+)
// ---------------------------------------------------------------------------

/// PIDFD selector for `process_madvise`.
pub const PROCESS_MADV_PIDFD: u32 = 0;

/// `__NR_process_madvise` on x86_64.
pub const NR_PROCESS_MADVISE: u32 = 440;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_posix_advice_dense_0_to_4() {
        let p = [
            MADV_NORMAL,
            MADV_RANDOM,
            MADV_SEQUENTIAL,
            MADV_WILLNEED,
            MADV_DONTNEED,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_linux_advice_dense_8_to_25() {
        let l = [
            MADV_FREE,
            MADV_REMOVE,
            MADV_DONTFORK,
            MADV_DOFORK,
            MADV_MERGEABLE,
            MADV_UNMERGEABLE,
            MADV_HUGEPAGE,
            MADV_NOHUGEPAGE,
            MADV_DONTDUMP,
            MADV_DODUMP,
            MADV_WIPEONFORK,
            MADV_KEEPONFORK,
            MADV_COLD,
            MADV_PAGEOUT,
            MADV_POPULATE_READ,
            MADV_POPULATE_WRITE,
            MADV_DONTNEED_LOCKED,
            MADV_COLLAPSE,
        ];
        for (i, &v) in l.iter().enumerate() {
            assert_eq!(v as usize, 8 + i);
        }
    }

    #[test]
    fn test_hwpoison_codes() {
        // Reserved high-numbered block for memory-failure testing.
        assert_eq!(MADV_HWPOISON, 100);
        assert_eq!(MADV_SOFT_OFFLINE, 101);
        // Don't collide with normal advice.
        assert!(MADV_HWPOISON > MADV_COLLAPSE);
    }

    #[test]
    fn test_inverse_pairs_adjacent() {
        // The "do/dont" pairs are deliberately consecutive.
        assert_eq!(MADV_DOFORK - MADV_DONTFORK, 1);
        assert_eq!(MADV_UNMERGEABLE - MADV_MERGEABLE, 1);
        assert_eq!(MADV_NOHUGEPAGE - MADV_HUGEPAGE, 1);
        assert_eq!(MADV_DODUMP - MADV_DONTDUMP, 1);
        assert_eq!(MADV_KEEPONFORK - MADV_WIPEONFORK, 1);
        assert_eq!(MADV_POPULATE_WRITE - MADV_POPULATE_READ, 1);
    }

    #[test]
    fn test_process_madvise_syscall() {
        // PIDFD is the only selector value defined for process_madvise.
        assert_eq!(PROCESS_MADV_PIDFD, 0);
        // Linux 5.10 x86_64 syscall number.
        assert_eq!(NR_PROCESS_MADVISE, 440);
    }
}
