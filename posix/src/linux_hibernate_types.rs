//! `<linux/suspend.h>` — Hibernation (S4) constants.
//!
//! Hibernation-specific constants covering swap types,
//! snapshot flags, and compression modes.

// ---------------------------------------------------------------------------
// Hibernation snapshot ioctl commands
// ---------------------------------------------------------------------------

/// Freeze processes.
pub const SNAPSHOT_FREEZE: u32 = 0x5301;
/// Unfreeze processes.
pub const SNAPSHOT_UNFREEZE: u32 = 0x5302;
/// Atomic snapshot.
pub const SNAPSHOT_ATOMIC_RESTORE: u32 = 0x5304;
/// Free memory.
pub const SNAPSHOT_FREE: u32 = 0x5305;
/// Get free swap.
pub const SNAPSHOT_FREE_SWAP_PAGES: u32 = 0x5309;
/// Set swap area.
pub const SNAPSHOT_SET_SWAP_AREA: u32 = 0x530C;
/// Get image size.
pub const SNAPSHOT_GET_IMAGE_SIZE: u32 = 0x530E;
/// Platform support.
pub const SNAPSHOT_PLATFORM_SUPPORT: u32 = 0x530F;
/// Power off.
pub const SNAPSHOT_POWER_OFF: u32 = 0x5310;
/// Create image.
pub const SNAPSHOT_CREATE_IMAGE: u32 = 0x5311;
/// Prefer minimal.
pub const SNAPSHOT_PREF_IMAGE_SIZE: u32 = 0x5312;
/// Allocate pages.
pub const SNAPSHOT_ALLOC_SWAP_PAGE: u32 = 0x5313;
/// Available swap.
pub const SNAPSHOT_AVAIL_SWAP_SIZE: u32 = 0x5316;

// ---------------------------------------------------------------------------
// Swap header constants
// ---------------------------------------------------------------------------

/// Swap header magic (new format).
pub const SWAP_HEADER_MAGIC: u32 = 0x5741_5053;
/// Swap header magic v2.
pub const SWAP_HEADER_MAGIC_V2: u32 = 0x4F53_5741;
/// Max swap bad pages.
pub const MAX_SWAP_BADPAGES: u32 = 1;

// ---------------------------------------------------------------------------
// Hibernation flags
// ---------------------------------------------------------------------------

/// Test mode (no actual hibernate).
pub const HIBERNATION_TEST: u32 = 1 << 0;
/// Test-resume mode.
pub const HIBERNATION_TESTPROC: u32 = 1 << 1;
/// Platform mode.
pub const HIBERNATION_PLATFORM: u32 = 1 << 2;
/// Shutdown (instead of platform).
pub const HIBERNATION_SHUTDOWN: u32 = 1 << 3;
/// Reboot after hibernate.
pub const HIBERNATION_REBOOT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Compression modes
// ---------------------------------------------------------------------------

/// No compression.
pub const SWAP_COMPRESS_NONE: u32 = 0;
/// LZO compression.
pub const SWAP_COMPRESS_LZO: u32 = 1;
/// LZ4 compression.
pub const SWAP_COMPRESS_LZ4: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_ioctls_distinct() {
        let cmds = [
            SNAPSHOT_FREEZE, SNAPSHOT_UNFREEZE,
            SNAPSHOT_ATOMIC_RESTORE, SNAPSHOT_FREE,
            SNAPSHOT_FREE_SWAP_PAGES, SNAPSHOT_SET_SWAP_AREA,
            SNAPSHOT_GET_IMAGE_SIZE, SNAPSHOT_PLATFORM_SUPPORT,
            SNAPSHOT_POWER_OFF, SNAPSHOT_CREATE_IMAGE,
            SNAPSHOT_PREF_IMAGE_SIZE, SNAPSHOT_ALLOC_SWAP_PAGE,
            SNAPSHOT_AVAIL_SWAP_SIZE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_swap_magics_distinct() {
        assert_ne!(SWAP_HEADER_MAGIC, SWAP_HEADER_MAGIC_V2);
    }

    #[test]
    fn test_hibernation_flags_power_of_two() {
        let flags = [
            HIBERNATION_TEST, HIBERNATION_TESTPROC,
            HIBERNATION_PLATFORM, HIBERNATION_SHUTDOWN,
            HIBERNATION_REBOOT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_hibernation_flags_no_overlap() {
        let flags = [
            HIBERNATION_TEST, HIBERNATION_TESTPROC,
            HIBERNATION_PLATFORM, HIBERNATION_SHUTDOWN,
            HIBERNATION_REBOOT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_compress_distinct() {
        let modes = [
            SWAP_COMPRESS_NONE, SWAP_COMPRESS_LZO, SWAP_COMPRESS_LZ4,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
