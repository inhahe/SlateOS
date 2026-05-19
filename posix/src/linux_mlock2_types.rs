//! `<linux/mman.h>` — Additional mlock constants.
//!
//! Supplementary mlock constants covering mlock2 flags,
//! mlockall flags, and memory locking limits.

// ---------------------------------------------------------------------------
// mlock2 flags (MLOCK_*)
// ---------------------------------------------------------------------------

/// Lock pages on fault (lazy locking).
pub const MLOCK_ONFAULT: u32 = 0x01;

// ---------------------------------------------------------------------------
// mlockall flags (MCL_*)
// ---------------------------------------------------------------------------

/// Lock all current pages.
pub const MCL_CURRENT: u32 = 1;
/// Lock all future pages.
pub const MCL_FUTURE: u32 = 2;
/// Lock pages on fault (mlockall).
pub const MCL_ONFAULT: u32 = 4;

// ---------------------------------------------------------------------------
// Memory protection flags (PROT_*)
// ---------------------------------------------------------------------------

/// No access.
pub const PROT_NONE_ML: u32 = 0x0;
/// Read access.
pub const PROT_READ_ML: u32 = 0x1;
/// Write access.
pub const PROT_WRITE_ML: u32 = 0x2;
/// Execute access.
pub const PROT_EXEC_ML: u32 = 0x4;
/// Pages can grow downward.
pub const PROT_GROWSDOWN_ML: u32 = 0x01000000;
/// Pages can grow upward.
pub const PROT_GROWSUP_ML: u32 = 0x02000000;

// ---------------------------------------------------------------------------
// msync flags
// ---------------------------------------------------------------------------

/// Sync asynchronously.
pub const MS_ASYNC_ML: u32 = 1;
/// Invalidate cached data.
pub const MS_INVALIDATE_ML: u32 = 2;
/// Sync synchronously.
pub const MS_SYNC_ML: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mlock_onfault() {
        assert_eq!(MLOCK_ONFAULT, 1);
    }

    #[test]
    fn test_mcl_flags_power_of_two() {
        assert!(MCL_CURRENT.is_power_of_two());
        assert!(MCL_FUTURE.is_power_of_two());
        assert!(MCL_ONFAULT.is_power_of_two());
    }

    #[test]
    fn test_mcl_flags_no_overlap() {
        let flags = [MCL_CURRENT, MCL_FUTURE, MCL_ONFAULT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_prot_flags_distinct() {
        let flags = [
            PROT_NONE_ML, PROT_READ_ML, PROT_WRITE_ML,
            PROT_EXEC_ML, PROT_GROWSDOWN_ML, PROT_GROWSUP_ML,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_prot_basic_no_overlap() {
        let flags = [PROT_READ_ML, PROT_WRITE_ML, PROT_EXEC_ML];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_msync_flags_distinct() {
        let flags = [MS_ASYNC_ML, MS_INVALIDATE_ML, MS_SYNC_ML];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
