//! `<crypto/rng.h>` — cryptographic random-number generator interface.
//!
//! The kernel's CRYPTO_ALG_TYPE_RNG family exposes DRBGs (deterministic
//! random bit generators) per NIST SP 800-90A. Userspace can pull bytes
//! via AF_ALG sockets or via /dev/random / getrandom().

// ---------------------------------------------------------------------------
// Algorithm type identifier
// ---------------------------------------------------------------------------

pub const ALG_TYPE_RNG: &str = "rng";

// ---------------------------------------------------------------------------
// Common RNG algorithm names
// ---------------------------------------------------------------------------

pub const RNG_NAME_DRBG_NOPR_HMAC_SHA256: &str = "drbg_nopr_hmac_sha256";
pub const RNG_NAME_DRBG_NOPR_HMAC_SHA512: &str = "drbg_nopr_hmac_sha512";
pub const RNG_NAME_DRBG_NOPR_CTR_AES256: &str = "drbg_nopr_ctr_aes256";
pub const RNG_NAME_DRBG_PR_HMAC_SHA256: &str = "drbg_pr_hmac_sha256";
pub const RNG_NAME_DRBG_PR_CTR_AES256: &str = "drbg_pr_ctr_aes256";
pub const RNG_NAME_STDRNG: &str = "stdrng";
pub const RNG_NAME_JITTERENTROPY: &str = "jitterentropy_rng";
pub const RNG_NAME_KRNG: &str = "krng";

// ---------------------------------------------------------------------------
// DRBG seed lengths (NIST SP 800-90A §10)
// ---------------------------------------------------------------------------

pub const DRBG_HMAC_SHA256_SEED_LEN: usize = 32;
pub const DRBG_HMAC_SHA512_SEED_LEN: usize = 64;
pub const DRBG_CTR_AES256_SEED_LEN: usize = 48;

// ---------------------------------------------------------------------------
// DRBG max bytes per request (Linux drbg limits)
// ---------------------------------------------------------------------------

/// Maximum bytes per drbg_generate() request (DRBG_MAX_REQUEST_BYTES).
pub const DRBG_MAX_REQUEST_BYTES: usize = 1 << 16;
/// Maximum bytes between mandatory reseeds.
pub const DRBG_MAX_BYTES_PER_RESEED: u64 = 1 << 48;

// ---------------------------------------------------------------------------
// /dev/random + getrandom() interface
// ---------------------------------------------------------------------------

pub const DEV_RANDOM_PATH: &str = "/dev/random";
pub const DEV_URANDOM_PATH: &str = "/dev/urandom";
pub const DEV_HW_RANDOM_PATH: &str = "/dev/hwrng";

/// getrandom() flag — read from /dev/random pool (block if low).
pub const GRND_RANDOM: u32 = 1 << 1;
/// getrandom() flag — return EAGAIN instead of blocking.
pub const GRND_NONBLOCK: u32 = 1 << 0;
/// getrandom() flag — insecure mode (early-boot, no entropy required).
pub const GRND_INSECURE: u32 = 1 << 2;

pub const NR_GETRANDOM_X86_64: u32 = 318;
pub const NR_GETRANDOM_AARCH64: u32 = 278;
pub const NR_GETRANDOM_I386: u32 = 355;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name_is_rng() {
        assert_eq!(ALG_TYPE_RNG, "rng");
    }

    #[test]
    fn test_drbg_name_pairs_pr_vs_nopr() {
        // "_pr_" indicates prediction-resistance; "_nopr_" doesn't.
        assert!(RNG_NAME_DRBG_NOPR_HMAC_SHA256.contains("_nopr_"));
        assert!(RNG_NAME_DRBG_PR_HMAC_SHA256.contains("_pr_"));
        assert!(!RNG_NAME_DRBG_PR_HMAC_SHA256.contains("_nopr_"));
    }

    #[test]
    fn test_drbg_seed_lengths_match_underlying_primitives() {
        // HMAC-SHA256 has 256-bit security.
        assert_eq!(DRBG_HMAC_SHA256_SEED_LEN, 32);
        // HMAC-SHA512 has 512-bit security.
        assert_eq!(DRBG_HMAC_SHA512_SEED_LEN, 64);
        // CTR-AES256 seed = key (32) + V (16) = 48.
        assert_eq!(DRBG_CTR_AES256_SEED_LEN, 32 + 16);
    }

    #[test]
    fn test_drbg_request_bounds() {
        assert_eq!(DRBG_MAX_REQUEST_BYTES, 65_536);
        assert!(DRBG_MAX_REQUEST_BYTES.is_power_of_two());
        // Reseed interval is much larger than per-request.
        assert!((DRBG_MAX_BYTES_PER_RESEED as u128) > DRBG_MAX_REQUEST_BYTES as u128);
    }

    #[test]
    fn test_dev_paths_under_dev() {
        for p in [DEV_RANDOM_PATH, DEV_URANDOM_PATH, DEV_HW_RANDOM_PATH] {
            assert!(p.starts_with("/dev/"));
        }
    }

    #[test]
    fn test_grnd_flags_single_bit_distinct() {
        for f in [GRND_RANDOM, GRND_NONBLOCK, GRND_INSECURE] {
            assert!(f.is_power_of_two());
        }
        assert_ne!(GRND_RANDOM, GRND_NONBLOCK);
        assert_ne!(GRND_NONBLOCK, GRND_INSECURE);
    }

    #[test]
    fn test_getrandom_syscall_numbers_distinct() {
        let n = [
            NR_GETRANDOM_X86_64,
            NR_GETRANDOM_AARCH64,
            NR_GETRANDOM_I386,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_all_rng_names_distinct() {
        let n = [
            RNG_NAME_DRBG_NOPR_HMAC_SHA256,
            RNG_NAME_DRBG_NOPR_HMAC_SHA512,
            RNG_NAME_DRBG_NOPR_CTR_AES256,
            RNG_NAME_DRBG_PR_HMAC_SHA256,
            RNG_NAME_DRBG_PR_CTR_AES256,
            RNG_NAME_STDRNG,
            RNG_NAME_JITTERENTROPY,
            RNG_NAME_KRNG,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }
}
