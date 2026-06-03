//! `<sys/random.h>` and `<linux/random.h>` — RNG ABI.
//!
//! `getrandom(2)` is the modern entry point; `/dev/random` and
//! `/dev/urandom` are the legacy character devices kept for
//! compatibility. `getentropy(3)` is a libc wrapper. The ioctls
//! below are used by `rngd` and friends to feed entropy into the
//! kernel pool and to inspect its state.

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

pub const DEV_RANDOM: &str = "/dev/random";
pub const DEV_URANDOM: &str = "/dev/urandom";

/// Misc-class minor for `/dev/random`.
pub const RANDOM_MINOR: u32 = 8;
/// Misc-class minor for `/dev/urandom`.
pub const URANDOM_MINOR: u32 = 9;

// ---------------------------------------------------------------------------
// `getrandom(2)` flags
// ---------------------------------------------------------------------------

pub const GRND_NONBLOCK: u32 = 1 << 0;
pub const GRND_RANDOM: u32 = 1 << 1;
pub const GRND_INSECURE: u32 = 1 << 2;

/// Mask of currently-defined flags — anything else is `EINVAL`.
pub const GRND_VALID_FLAGS: u32 = GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE;

// ---------------------------------------------------------------------------
// `/dev/{u}random` ioctls (`RNDGETENTCNT`, `RNDADDENTROPY`, …)
// ---------------------------------------------------------------------------

pub const RNDGETENTCNT: u32 = 0x8004_5200;
pub const RNDADDTOENTCNT: u32 = 0x4004_5201;
pub const RNDGETPOOL: u32 = 0x8008_5202;
pub const RNDADDENTROPY: u32 = 0x4008_5203;
pub const RNDZAPENTCNT: u32 = 0x5204;
pub const RNDCLEARPOOL: u32 = 0x5206;
pub const RNDRESEEDCRNG: u32 = 0x5207;

// ---------------------------------------------------------------------------
// Pool / read sizes
// ---------------------------------------------------------------------------

/// Maximum bytes returned by a single `getrandom(2)` call. Beyond
/// this the call may short-read.
pub const GETRANDOM_MAX_RETURN: usize = 256;
/// Pool size (CRNG output buffer). 256 bits = 32 bytes.
pub const CRNG_RESEED_INTERVAL_S: u32 = 60;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_GETRANDOM: u32 = 318;
pub const NR_VGETRANDOM: u32 = 451;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_paths() {
        assert_eq!(DEV_RANDOM, "/dev/random");
        assert_eq!(DEV_URANDOM, "/dev/urandom");
        assert_ne!(DEV_RANDOM, DEV_URANDOM);
    }

    #[test]
    fn test_misc_minors_adjacent() {
        assert_eq!(RANDOM_MINOR, 8);
        assert_eq!(URANDOM_MINOR, 9);
        assert_eq!(URANDOM_MINOR, RANDOM_MINOR + 1);
    }

    #[test]
    fn test_grnd_flags_single_bit_and_mask() {
        let f = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for v in f {
            assert!(v.is_power_of_two());
        }
        assert_eq!(GRND_VALID_FLAGS, 0x7);
        // GRND_INSECURE was added in Linux 5.6.
        assert_eq!(GRND_INSECURE, 0x4);
    }

    #[test]
    fn test_rnd_ioctls_distinct() {
        let i = [
            RNDGETENTCNT,
            RNDADDTOENTCNT,
            RNDGETPOOL,
            RNDADDENTROPY,
            RNDZAPENTCNT,
            RNDCLEARPOOL,
            RNDRESEEDCRNG,
        ];
        for a in 0..i.len() {
            for b in (a + 1)..i.len() {
                assert_ne!(i[a], i[b]);
            }
        }
    }

    #[test]
    fn test_getrandom_short_read_cap() {
        // The kernel won't atomically return more than 256 bytes.
        assert_eq!(GETRANDOM_MAX_RETURN, 256);
        assert!(GETRANDOM_MAX_RETURN.is_power_of_two());
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_GETRANDOM, 318);
        // vgetrandom (vDSO-style) is much newer.
        assert!(NR_VGETRANDOM > NR_GETRANDOM);
    }
}
