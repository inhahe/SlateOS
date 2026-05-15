//! `<sys/mman.h>` — memory management declarations.
//!
//! Re-exports memory-mapping functions, protection/flag constants,
//! and shared-memory operations from the `mman` module.

// ---------------------------------------------------------------------------
// Protection flags
// ---------------------------------------------------------------------------

pub use crate::mman::PROT_NONE;
pub use crate::mman::PROT_READ;
pub use crate::mman::PROT_WRITE;
pub use crate::mman::PROT_EXEC;

// ---------------------------------------------------------------------------
// Mapping flags
// ---------------------------------------------------------------------------

pub use crate::mman::MAP_SHARED;
pub use crate::mman::MAP_PRIVATE;
pub use crate::mman::MAP_FIXED;
pub use crate::mman::MAP_ANONYMOUS;
pub use crate::mman::MAP_ANON;
pub use crate::mman::MAP_GROWSDOWN;
pub use crate::mman::MAP_FIXED_NOREPLACE;
pub use crate::mman::MAP_NORESERVE;
pub use crate::mman::MAP_POPULATE;
pub use crate::mman::MAP_NONBLOCK;
pub use crate::mman::MAP_FAILED;

// ---------------------------------------------------------------------------
// msync flags
// ---------------------------------------------------------------------------

pub use crate::mman::MS_ASYNC;
pub use crate::mman::MS_SYNC;
pub use crate::mman::MS_INVALIDATE;

// ---------------------------------------------------------------------------
// madvise advice
// ---------------------------------------------------------------------------

pub use crate::mman::MADV_NORMAL;
pub use crate::mman::MADV_RANDOM;
pub use crate::mman::MADV_SEQUENTIAL;
pub use crate::mman::MADV_WILLNEED;
pub use crate::mman::MADV_DONTNEED;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::mman::mmap;
pub use crate::mman::munmap;
pub use crate::mman::mprotect;
pub use crate::mman::mlock;
pub use crate::mman::mlockall;
pub use crate::mman::msync;
pub use crate::mman::madvise;
pub use crate::mman::shm_open;
pub use crate::mman::shm_unlink;
pub use crate::mman::memfd_create;
pub use crate::mman::mmap64;
pub use crate::mman::mremap;
pub use crate::mman::mlock2;

// ---------------------------------------------------------------------------
// POSIX advisory
// ---------------------------------------------------------------------------

/// Advise the kernel about expected memory use patterns (POSIX version).
pub use crate::mman::posix_madvise;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prot_values() {
        assert_eq!(PROT_NONE, 0);
        assert_eq!(PROT_READ, 1);
        assert_eq!(PROT_WRITE, 2);
        assert_eq!(PROT_EXEC, 4);
    }

    #[test]
    fn test_prot_combinable() {
        let rw = PROT_READ | PROT_WRITE;
        assert_eq!(rw, 3);
        let rwx = PROT_READ | PROT_WRITE | PROT_EXEC;
        assert_eq!(rwx, 7);
    }

    #[test]
    fn test_map_flags() {
        assert_eq!(MAP_SHARED, 0x01);
        assert_eq!(MAP_PRIVATE, 0x02);
        assert_eq!(MAP_FIXED, 0x10);
        assert_eq!(MAP_ANONYMOUS, 0x20);
        assert_eq!(MAP_ANON, MAP_ANONYMOUS);
    }

    #[test]
    fn test_map_flags_distinct() {
        let flags = [
            MAP_SHARED, MAP_PRIVATE, MAP_FIXED, MAP_ANONYMOUS,
            MAP_GROWSDOWN, MAP_FIXED_NOREPLACE, MAP_NORESERVE,
            MAP_POPULATE, MAP_NONBLOCK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "MAP flags must be distinct");
            }
        }
    }

    #[test]
    fn test_map_failed() {
        assert_eq!(MAP_FAILED as usize, usize::MAX);
    }

    #[test]
    fn test_msync_flags() {
        assert_eq!(MS_ASYNC, 1);
        assert_eq!(MS_SYNC, 4);
        assert_eq!(MS_INVALIDATE, 2);
    }

    #[test]
    fn test_madv_values() {
        assert_eq!(MADV_NORMAL, 0);
        assert_eq!(MADV_RANDOM, 1);
        assert_eq!(MADV_SEQUENTIAL, 2);
        assert_eq!(MADV_WILLNEED, 3);
        assert_eq!(MADV_DONTNEED, 4);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PROT_READ, crate::mman::PROT_READ);
        assert_eq!(MAP_SHARED, crate::mman::MAP_SHARED);
        assert_eq!(MAP_ANONYMOUS, crate::mman::MAP_ANONYMOUS);
    }
}
