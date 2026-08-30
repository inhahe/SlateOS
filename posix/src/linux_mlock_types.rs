//! `<sys/mman.h>` — Memory locking constants.
//!
//! These constants control the `mlock2()` and `mlockall()` syscalls
//! which lock pages into physical memory, preventing them from being
//! paged out to swap. This is critical for real-time applications
//! and security-sensitive code that must avoid page faults.

// ---------------------------------------------------------------------------
// mlockall() flags
// ---------------------------------------------------------------------------

/// Lock all currently mapped pages.
pub const MCL_CURRENT: u32 = 1;
/// Lock all future mappings as well.
pub const MCL_FUTURE: u32 = 2;
/// Lock all pages faulted in after call.
pub const MCL_ONFAULT: u32 = 4;

// ---------------------------------------------------------------------------
// mlock2() flags
// ---------------------------------------------------------------------------

/// Lock pages only when faulted in (lazy lock).
pub const MLOCK_ONFAULT: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_mcl_power_of_two() {
        assert!(MCL_CURRENT.is_power_of_two());
        assert!(MCL_FUTURE.is_power_of_two());
        assert!(MCL_ONFAULT.is_power_of_two());
    }

    #[test]
    fn test_mcl_values() {
        assert_eq!(MCL_CURRENT, 1);
        assert_eq!(MCL_FUTURE, 2);
        assert_eq!(MCL_ONFAULT, 4);
    }

    #[test]
    fn test_mlock_onfault() {
        assert_eq!(MLOCK_ONFAULT, 0x01);
    }
}
