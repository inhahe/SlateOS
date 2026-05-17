//! `<linux/seqlock.h>` — Sequence lock constants.
//!
//! Sequence locks (seqlocks) provide fast lockless reads of rarely-
//! modified data. The writer increments a sequence counter (odd =
//! write in progress, even = stable). Readers read the counter
//! before and after accessing data; if it changed or was odd, they
//! retry. No locks for readers means zero contention on read-heavy
//! paths. Used for jiffies, xtime (wall clock), and networking
//! statistics where reads vastly outnumber writes.

// ---------------------------------------------------------------------------
// Sequence counter states
// ---------------------------------------------------------------------------

/// Initial sequence value (unlocked, stable).
pub const SEQCOUNT_INIT: u32 = 0;
/// Write in progress bit (sequence is odd during writes).
pub const SEQCOUNT_WRITE_BIT: u32 = 1;

// ---------------------------------------------------------------------------
// Seqlock types
// ---------------------------------------------------------------------------

/// Plain seqcount (no associated lock, writer must use external sync).
pub const SEQCOUNT_TYPE_PLAIN: u32 = 0;
/// seqcount_spinlock_t (associated spinlock for writer).
pub const SEQCOUNT_TYPE_SPINLOCK: u32 = 1;
/// seqcount_rwlock_t (associated rwlock for writer).
pub const SEQCOUNT_TYPE_RWLOCK: u32 = 2;
/// seqcount_mutex_t (associated mutex for writer).
pub const SEQCOUNT_TYPE_MUTEX: u32 = 3;
/// seqcount_ww_mutex_t (associated ww_mutex).
pub const SEQCOUNT_TYPE_WW_MUTEX: u32 = 4;

// ---------------------------------------------------------------------------
// Seqlock read retry thresholds
// ---------------------------------------------------------------------------

/// Maximum read retries before yielding CPU.
pub const SEQLOCK_MAX_RETRIES: u32 = 100;

// ---------------------------------------------------------------------------
// Sequence counter operations
// ---------------------------------------------------------------------------

/// Read sequence number before accessing data.
pub const SEQOP_READ_BEGIN: u32 = 0;
/// Read sequence number after accessing data (compare with begin).
pub const SEQOP_READ_RETRY: u32 = 1;
/// Increment sequence to start write.
pub const SEQOP_WRITE_BEGIN: u32 = 2;
/// Increment sequence to end write.
pub const SEQOP_WRITE_END: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_is_even() {
        assert_eq!(SEQCOUNT_INIT % 2, 0);
    }

    #[test]
    fn test_write_bit() {
        assert_eq!(SEQCOUNT_WRITE_BIT, 1);
        // During write, sequence is odd
        assert_eq!((SEQCOUNT_INIT + SEQCOUNT_WRITE_BIT) % 2, 1);
    }

    #[test]
    fn test_types_distinct() {
        let types = [
            SEQCOUNT_TYPE_PLAIN, SEQCOUNT_TYPE_SPINLOCK,
            SEQCOUNT_TYPE_RWLOCK, SEQCOUNT_TYPE_MUTEX,
            SEQCOUNT_TYPE_WW_MUTEX,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ops_distinct() {
        let ops = [
            SEQOP_READ_BEGIN, SEQOP_READ_RETRY,
            SEQOP_WRITE_BEGIN, SEQOP_WRITE_END,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_max_retries() {
        assert!(SEQLOCK_MAX_RETRIES > 0);
    }
}
