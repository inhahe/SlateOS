//! `<crypto/algapi.h>` — base `crypto_alg` structure flags and priority.
//!
//! Every registered kernel cipher/hash/etc. is a `struct crypto_alg`
//! with a name, type/mask, and a numeric priority. Higher-priority
//! variants (assembly, hardware-accelerated) override slower ones.

// ---------------------------------------------------------------------------
// Algorithm priority hints
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_PRIORITY_MIN: i32 = 0;
/// Generic C reference implementation.
pub const CRYPTO_ALG_PRIORITY_GENERIC: i32 = 100;
/// SSE / NEON / VSX assembly fast paths.
pub const CRYPTO_ALG_PRIORITY_SIMD: i32 = 200;
/// AES-NI, SHA-NI etc. (CPU-specific instruction).
pub const CRYPTO_ALG_PRIORITY_AESNI: i32 = 300;
/// Off-CPU hardware accelerator.
pub const CRYPTO_ALG_PRIORITY_HARDWARE: i32 = 400;
pub const CRYPTO_ALG_PRIORITY_MAX: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Type / mask (low nibble of cra_flags)
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000_000F;
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x01;
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x02;
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x03;
pub const CRYPTO_ALG_TYPE_LSKCIPHER: u32 = 0x04;
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x06;
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x08;
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0C;
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0E;
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0F;

// ---------------------------------------------------------------------------
// Algorithm flag bits (above the type nibble)
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_LARVAL: u32 = 1 << 4;
pub const CRYPTO_ALG_DEAD: u32 = 1 << 5;
pub const CRYPTO_ALG_DYING: u32 = 1 << 6;
pub const CRYPTO_ALG_ASYNC: u32 = 1 << 7;
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 1 << 8;
pub const CRYPTO_ALG_GENIV: u32 = 1 << 9;
pub const CRYPTO_ALG_TESTED: u32 = 1 << 10;
pub const CRYPTO_ALG_INSTANCE: u32 = 1 << 11;
pub const CRYPTO_ALG_KERN_DRIVER_ONLY: u32 = 1 << 12;
pub const CRYPTO_ALG_OPTIONAL_KEY: u32 = 1 << 13;
pub const CRYPTO_ALG_INTERNAL: u32 = 1 << 14;
pub const CRYPTO_ALG_ALLOCATES_MEMORY: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Request/transform flags (CRYPTO_TFM_REQ_*)
// ---------------------------------------------------------------------------

pub const CRYPTO_TFM_REQ_MAY_BACKLOG: u32 = 1 << 0;
pub const CRYPTO_TFM_REQ_MAY_SLEEP: u32 = 1 << 1;
pub const CRYPTO_TFM_REQ_FORBID_WEAK_KEYS: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Common error codes returned by alg ops
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_EINVAL: i32 = 22;
pub const CRYPTO_ALG_EBUSY: i32 = 16;
pub const CRYPTO_ALG_EINPROGRESS: i32 = 115;
pub const CRYPTO_ALG_ENOKEY: i32 = 126;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priorities_strictly_ordered() {
        let p = [
            CRYPTO_ALG_PRIORITY_MIN,
            CRYPTO_ALG_PRIORITY_GENERIC,
            CRYPTO_ALG_PRIORITY_SIMD,
            CRYPTO_ALG_PRIORITY_AESNI,
            CRYPTO_ALG_PRIORITY_HARDWARE,
            CRYPTO_ALG_PRIORITY_MAX,
        ];
        for w in p.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn test_type_mask_covers_low_nibble() {
        assert_eq!(CRYPTO_ALG_TYPE_MASK, 0x0F);
        let t = [
            CRYPTO_ALG_TYPE_CIPHER,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_LSKCIPHER,
            CRYPTO_ALG_TYPE_AKCIPHER,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_RNG,
            CRYPTO_ALG_TYPE_SHASH,
            CRYPTO_ALG_TYPE_AHASH,
        ];
        for &v in &t {
            assert_eq!(v & CRYPTO_ALG_TYPE_MASK, v);
        }
    }

    #[test]
    fn test_alg_flag_bits_distinct_single_above_nibble() {
        let f = [
            CRYPTO_ALG_LARVAL,
            CRYPTO_ALG_DEAD,
            CRYPTO_ALG_DYING,
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_GENIV,
            CRYPTO_ALG_TESTED,
            CRYPTO_ALG_INSTANCE,
            CRYPTO_ALG_KERN_DRIVER_ONLY,
            CRYPTO_ALG_OPTIONAL_KEY,
            CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_ALLOCATES_MEMORY,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            // Above the type nibble.
            assert!(x > CRYPTO_ALG_TYPE_MASK);
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_tfm_req_flags_distinct_single_bit() {
        for x in [
            CRYPTO_TFM_REQ_MAY_BACKLOG,
            CRYPTO_TFM_REQ_MAY_SLEEP,
            CRYPTO_TFM_REQ_FORBID_WEAK_KEYS,
        ] {
            assert!(x.is_power_of_two());
        }
        assert_eq!(
            CRYPTO_TFM_REQ_MAY_BACKLOG
                | CRYPTO_TFM_REQ_MAY_SLEEP
                | CRYPTO_TFM_REQ_FORBID_WEAK_KEYS,
            0x07
        );
    }

    #[test]
    fn test_errno_values_distinct() {
        let e = [
            CRYPTO_ALG_EINVAL,
            CRYPTO_ALG_EBUSY,
            CRYPTO_ALG_EINPROGRESS,
            CRYPTO_ALG_ENOKEY,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(CRYPTO_ALG_EINVAL, 22);
        assert_eq!(CRYPTO_ALG_EBUSY, 16);
        assert_eq!(CRYPTO_ALG_EINPROGRESS, 115);
        assert_eq!(CRYPTO_ALG_ENOKEY, 126);
    }
}
