//! `<sys/mman.h>` — `mlock(2)` / `mlockall(2)` / `mlock2(2)` constants.
//!
//! Memory locking pins pages so they cannot be paged out. Real-time
//! systems (audio servers like JACK and PipeWire, low-latency trading
//! daemons) and cryptographic libraries (libsodium pins key memory to
//! keep it out of swap) are the primary users.

// ---------------------------------------------------------------------------
// `mlockall(2)` flags
// ---------------------------------------------------------------------------

/// Lock all currently mapped pages.
pub const MCL_CURRENT: u32 = 1;
/// Lock all pages mapped in the future.
pub const MCL_FUTURE: u32 = 2;
/// Defer the lock to page-fault time (Linux 4.4+).
pub const MCL_ONFAULT: u32 = 4;

// ---------------------------------------------------------------------------
// `mlock2(2)` flags (Linux 4.4+)
// ---------------------------------------------------------------------------

/// Lock pages only when faulted in.
pub const MLOCK_ONFAULT: u32 = 0x01;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_MLOCK: u32 = 149;
pub const NR_MUNLOCK: u32 = 150;
pub const NR_MLOCKALL: u32 = 151;
pub const NR_MUNLOCKALL: u32 = 152;
/// Added in Linux 4.4.
pub const NR_MLOCK2: u32 = 325;

// ---------------------------------------------------------------------------
// `RLIMIT_MEMLOCK` defaults (kernel default before unlimited for root)
// ---------------------------------------------------------------------------

/// Historical default soft limit, 64 KiB.
pub const RLIMIT_MEMLOCK_DEFAULT: u64 = 64 * 1024;
/// Modern systemd default, 8 MiB.
pub const RLIMIT_MEMLOCK_SYSTEMD_DEFAULT: u64 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mlockall_flags_single_bit_and_dense() {
        for f in [MCL_CURRENT, MCL_FUTURE, MCL_ONFAULT] {
            assert!(f.is_power_of_two());
        }
        // Three dense bits.
        assert_eq!(MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT, 0x7);
    }

    #[test]
    fn test_mlock2_flag() {
        // mlock2's only flag value matches MCL_ONFAULT-style semantics
        // but the encoding is a low bit (0x01), not MCL's 0x04.
        assert_eq!(MLOCK_ONFAULT, 1);
        assert!(MLOCK_ONFAULT.is_power_of_two());
    }

    #[test]
    fn test_syscall_numbers_monotone_and_clustered() {
        // The mlock family is densely numbered.
        assert_eq!(NR_MUNLOCK, NR_MLOCK + 1);
        assert_eq!(NR_MLOCKALL, NR_MLOCK + 2);
        assert_eq!(NR_MUNLOCKALL, NR_MLOCK + 3);
        // mlock2 added much later — different range.
        assert!(NR_MLOCK2 > NR_MUNLOCKALL);
    }

    #[test]
    fn test_rlimit_defaults() {
        assert_eq!(RLIMIT_MEMLOCK_DEFAULT, 65_536);
        assert_eq!(RLIMIT_MEMLOCK_SYSTEMD_DEFAULT, 8_388_608);
        assert!(RLIMIT_MEMLOCK_SYSTEMD_DEFAULT > RLIMIT_MEMLOCK_DEFAULT);
    }
}
