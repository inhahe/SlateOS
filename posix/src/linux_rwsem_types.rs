//! `<linux/rwsem.h>` — Reader-writer semaphore constants.
//!
//! Reader-writer semaphores (rwsem) are sleeping locks that allow
//! concurrent readers or exclusive writers. Unlike rwlock (spinlock-
//! based, non-sleeping), rwsem tasks sleep when they can't acquire
//! the lock. rwsems are writer-preferring (to prevent writer
//! starvation) and support optimistic spinning on the owner field.
//! Used for protecting mmap_lock (process address space), filesystem
//! superblock, and other data accessed in process context.

// ---------------------------------------------------------------------------
// rwsem count encoding
// ---------------------------------------------------------------------------

/// Read-locked bias (one reader holds it).
pub const RWSEM_READER_BIAS: u64 = 0x0000_0000_0000_0100;
/// Write-locked value (in count field).
pub const RWSEM_WRITER_LOCKED: u64 = 0xFFFF_FFFF_FFFF_FF01;
/// Unlocked value.
pub const RWSEM_UNLOCKED: u64 = 0;
/// Flag: waiters are present.
pub const RWSEM_FLAG_WAITERS: u64 = 0x0000_0000_0000_0002;
/// Flag: handoff requested (pass lock directly to waiter).
pub const RWSEM_FLAG_HANDOFF: u64 = 0x0000_0000_0000_0004;

// ---------------------------------------------------------------------------
// rwsem waiter types
// ---------------------------------------------------------------------------

/// Waiter wants read access.
pub const RWSEM_WAITER_READER: u32 = 0;
/// Waiter wants write access.
pub const RWSEM_WAITER_WRITER: u32 = 1;

// ---------------------------------------------------------------------------
// rwsem optimistic spin thresholds
// ---------------------------------------------------------------------------

/// Maximum spin iterations before sleeping.
pub const RWSEM_SPIN_MAX: u32 = 256;
/// Spin pause iterations (CPU relax).
pub const RWSEM_SPIN_PAUSE: u32 = 4;

// ---------------------------------------------------------------------------
// rwsem downgrade modes
// ---------------------------------------------------------------------------

/// Downgrade write lock to read lock.
pub const RWSEM_DOWNGRADE_READ: u32 = 0;
/// Release write lock completely.
pub const RWSEM_DOWNGRADE_RELEASE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_values_distinct() {
        assert_ne!(RWSEM_READER_BIAS, RWSEM_UNLOCKED);
        assert_ne!(RWSEM_WRITER_LOCKED, RWSEM_UNLOCKED);
        assert_ne!(RWSEM_READER_BIAS, RWSEM_WRITER_LOCKED);
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(RWSEM_FLAG_WAITERS & RWSEM_FLAG_HANDOFF, 0);
    }

    #[test]
    fn test_waiter_types_distinct() {
        assert_ne!(RWSEM_WAITER_READER, RWSEM_WAITER_WRITER);
    }

    #[test]
    fn test_spin_thresholds() {
        assert!(RWSEM_SPIN_MAX > 0);
        assert!(RWSEM_SPIN_PAUSE > 0);
        assert!(RWSEM_SPIN_MAX > RWSEM_SPIN_PAUSE);
    }

    #[test]
    fn test_downgrade_modes_distinct() {
        assert_ne!(RWSEM_DOWNGRADE_READ, RWSEM_DOWNGRADE_RELEASE);
    }
}
