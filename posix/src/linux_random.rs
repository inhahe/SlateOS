//! `<linux/random.h>` — kernel random number generator.
//!
//! Re-exports GRND_* flags from `sys_random` and adds kernel-internal
//! RNG ioctl constants for `/dev/random` and `/dev/urandom`.

pub use crate::sys_random::GRND_RANDOM;
pub use crate::sys_random::GRND_NONBLOCK;
pub use crate::sys_random::GRND_INSECURE;
pub use crate::unistd::getrandom;

// ---------------------------------------------------------------------------
// RNG ioctl commands (/dev/random, /dev/urandom)
// ---------------------------------------------------------------------------

/// Get entropy count (bits available).
pub const RNDGETENTCNT: u64 = 0x8004_5200;
/// Add entropy to the pool.
pub const RNDADDTOENTCNT: u64 = 0x4004_5201;
/// Get pool size (in bits).
pub const RNDGETPOOL: u64 = 0x8002_5202;
/// Add entropy data.
pub const RNDADDENTROPY: u64 = 0x4008_5203;
/// Clear the entropy pool.
pub const RNDZAPENTCNT: u64 = 0x5204;
/// Clear pool and reseed CRNG.
pub const RNDCLEARPOOL: u64 = 0x5206;
/// Reseed CRNG from entropy pool.
pub const RNDRESEEDCRNG: u64 = 0x5207;

// ---------------------------------------------------------------------------
// Entropy pool info struct
// ---------------------------------------------------------------------------

/// Data passed with RNDADDENTROPY.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RandPoolInfo {
    /// Entropy count (bits of randomness claimed).
    pub entropy_count: i32,
    /// Size of the data buffer that follows.
    pub buf_size: i32,
    // Followed by `buf_size` bytes of entropy data.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grnd_flags() {
        assert_eq!(GRND_NONBLOCK, 1);
        assert_eq!(GRND_RANDOM, 2);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            RNDGETENTCNT, RNDADDTOENTCNT, RNDGETPOOL,
            RNDADDENTROPY, RNDZAPENTCNT, RNDCLEARPOOL, RNDRESEEDCRNG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rand_pool_info_size() {
        assert_eq!(core::mem::size_of::<RandPoolInfo>(), 8);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(GRND_RANDOM, crate::sys_random::GRND_RANDOM);
        assert_eq!(GRND_NONBLOCK, crate::sys_random::GRND_NONBLOCK);
    }
}
