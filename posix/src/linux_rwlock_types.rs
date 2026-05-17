//! `<linux/rwlock.h>` — Reader-writer spinlock constants.
//!
//! Reader-writer spinlocks allow multiple concurrent readers or one
//! exclusive writer. Readers don't block each other, improving
//! parallelism for read-heavy data structures. However, writers can
//! starve if readers continuously hold the lock. rwlocks are used
//! in the kernel for data like routing tables, mount tree, and
//! other structures that are read far more often than written.

// ---------------------------------------------------------------------------
// rwlock state encoding
// ---------------------------------------------------------------------------

/// Lock is free (no readers or writers).
pub const RWLOCK_FREE: u32 = 0;
/// Write-locked value (typically 0x8000_0000 or -1 in the counter).
pub const RWLOCK_WRITE_LOCKED: u32 = 0x8000_0000;
/// One reader bit (each reader increments by 1).
pub const RWLOCK_READER_BIAS: u32 = 1;
/// Maximum concurrent readers.
pub const RWLOCK_MAX_READERS: u32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// rwlock types (variants)
// ---------------------------------------------------------------------------

/// Standard rwlock (writer-preferring on some architectures).
pub const RWLOCK_TYPE_NORMAL: u32 = 0;
/// Reader-preferring rwlock (writers may starve).
pub const RWLOCK_TYPE_READER_PREF: u32 = 1;
/// Writer-preferring rwlock (readers may starve).
pub const RWLOCK_TYPE_WRITER_PREF: u32 = 2;
/// Fair rwlock (FIFO ordering, no starvation).
pub const RWLOCK_TYPE_FAIR: u32 = 3;

// ---------------------------------------------------------------------------
// Lock debugging constants
// ---------------------------------------------------------------------------

/// Lock class: no nesting allowed.
pub const LOCK_CLASS_UNNESTED: u32 = 0;
/// Lock class: single level nesting allowed.
pub const LOCK_CLASS_NESTED: u32 = 1;
/// Maximum lock nesting depth for lockdep.
pub const LOCKDEP_MAX_DEPTH: u32 = 48;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_locked_is_high_bit() {
        assert!(RWLOCK_WRITE_LOCKED.is_power_of_two());
        assert_eq!(RWLOCK_WRITE_LOCKED, 1 << 31);
    }

    #[test]
    fn test_max_readers_complement() {
        assert_eq!(RWLOCK_WRITE_LOCKED | RWLOCK_MAX_READERS, u32::MAX);
    }

    #[test]
    fn test_rwlock_types_distinct() {
        let types = [
            RWLOCK_TYPE_NORMAL, RWLOCK_TYPE_READER_PREF,
            RWLOCK_TYPE_WRITER_PREF, RWLOCK_TYPE_FAIR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_lock_classes_distinct() {
        assert_ne!(LOCK_CLASS_UNNESTED, LOCK_CLASS_NESTED);
    }

    #[test]
    fn test_lockdep_depth() {
        assert!(LOCKDEP_MAX_DEPTH > 0);
    }
}
