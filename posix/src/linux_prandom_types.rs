//! `<linux/random.h>` — Pseudo-random number generator constants.
//!
//! Constants for the Linux kernel random number subsystem covering
//! entropy pool sizes, getrandom flags, and RNDCTL ioctl commands.

// ---------------------------------------------------------------------------
// getrandom flags (GRND_*)
// ---------------------------------------------------------------------------

/// Block until entropy pool is initialized.
pub const GRND_NONBLOCK: u32 = 0x0001;
/// Use /dev/random (blocking pool).
pub const GRND_RANDOM: u32 = 0x0002;
/// Seed from init-phase entropy only.
pub const GRND_INSECURE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Entropy pool constants
// ---------------------------------------------------------------------------

/// Input pool size in bits.
pub const POOL_INPUT_BITS: u32 = 4096;
/// Blocking pool size in bits.
pub const POOL_BLOCKING_BITS: u32 = 4096;
/// Output pool size (bytes returned per read).
pub const POOL_OUTPUT_BYTES: u32 = 256;

// ---------------------------------------------------------------------------
// RNDCTL ioctl commands
// ---------------------------------------------------------------------------

/// Get entropy count.
pub const RNDGETENTCNT: u32 = 0x80045200;
/// Add to entropy count.
pub const RNDADDTOENTCNT: u32 = 0x40045201;
/// Get pool size.
pub const RNDGETPOOL: u32 = 0x80085202;
/// Add entropy.
pub const RNDADDENTROPY: u32 = 0x40085203;
/// Clear pool.
pub const RNDZAPENTCNT: u32 = 0x5204;
/// Clear pool and reseed.
pub const RNDCLEARPOOL: u32 = 0x5206;
/// Reseed CRNG.
pub const RNDRESEEDCRNG: u32 = 0x5207;

// ---------------------------------------------------------------------------
// Random source types
// ---------------------------------------------------------------------------

/// Hardware RNG source.
pub const RANDOM_SOURCE_HW: u32 = 0;
/// Interrupt timing source.
pub const RANDOM_SOURCE_IRQ: u32 = 1;
/// Disk timing source.
pub const RANDOM_SOURCE_DISK: u32 = 2;
/// Input event source.
pub const RANDOM_SOURCE_INPUT: u32 = 3;
/// Jitter entropy source.
pub const RANDOM_SOURCE_JITTER: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grnd_flags_distinct() {
        let flags = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_grnd_flags_no_overlap() {
        let flags = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pool_sizes() {
        assert_eq!(POOL_INPUT_BITS, 4096);
        assert!(POOL_OUTPUT_BYTES > 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            RNDGETENTCNT,
            RNDADDTOENTCNT,
            RNDGETPOOL,
            RNDADDENTROPY,
            RNDZAPENTCNT,
            RNDCLEARPOOL,
            RNDRESEEDCRNG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_source_types_distinct() {
        let sources = [
            RANDOM_SOURCE_HW,
            RANDOM_SOURCE_IRQ,
            RANDOM_SOURCE_DISK,
            RANDOM_SOURCE_INPUT,
            RANDOM_SOURCE_JITTER,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }
}
