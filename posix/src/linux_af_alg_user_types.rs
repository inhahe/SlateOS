//! `<linux/if_alg.h>` — userspace access to in-kernel crypto algorithms.
//!
//! `AF_ALG` sockets let userspace use the kernel's `crypto_api`
//! (skcipher, aead, hash, rng, akcipher) without linking a userspace
//! crypto library. `cryptsetup` and `iwd` use it; so do containers
//! that strip libcrypto from the rootfs.

// ---------------------------------------------------------------------------
// Address family / SOL level
// ---------------------------------------------------------------------------

pub const AF_ALG: u32 = 38;
pub const PF_ALG: u32 = AF_ALG;
pub const SOL_ALG: u32 = 279;

// ---------------------------------------------------------------------------
// `sockaddr_alg.salg_type` strings (chosen at `bind()` time)
// ---------------------------------------------------------------------------

pub const ALG_TYPE_SKCIPHER: &str = "skcipher";
pub const ALG_TYPE_AEAD: &str = "aead";
pub const ALG_TYPE_HASH: &str = "hash";
pub const ALG_TYPE_RNG: &str = "rng";
pub const ALG_TYPE_AKCIPHER: &str = "akcipher";
pub const ALG_TYPE_KPP: &str = "kpp";
pub const ALG_TYPE_SHASH: &str = "shash";
pub const ALG_TYPE_AHASH: &str = "ahash";

// ---------------------------------------------------------------------------
// Field sizes
// ---------------------------------------------------------------------------

pub const ALG_TYPE_LEN: usize = 14;
pub const ALG_NAME_LEN: usize = 64;

// ---------------------------------------------------------------------------
// `setsockopt(SOL_ALG, …)` levels
// ---------------------------------------------------------------------------

pub const ALG_SET_KEY: u32 = 1;
pub const ALG_SET_IV: u32 = 2;
pub const ALG_SET_OP: u32 = 3;
pub const ALG_SET_AEAD_ASSOCLEN: u32 = 4;
pub const ALG_SET_AEAD_AUTHSIZE: u32 = 5;
pub const ALG_SET_DRBG_ENTROPY: u32 = 6;
pub const ALG_SET_KEY_BY_KEY_SERIAL: u32 = 7;

// ---------------------------------------------------------------------------
// `ALG_OP_*` — sent via `cmsg` to select encrypt vs. decrypt
// ---------------------------------------------------------------------------

pub const ALG_OP_DECRYPT: u32 = 0;
pub const ALG_OP_ENCRYPT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_alg_constants() {
        assert_eq!(AF_ALG, 38);
        assert_eq!(PF_ALG, AF_ALG);
        assert_eq!(SOL_ALG, 279);
    }

    #[test]
    fn test_type_strings_distinct_short() {
        let t = [
            ALG_TYPE_SKCIPHER,
            ALG_TYPE_AEAD,
            ALG_TYPE_HASH,
            ALG_TYPE_RNG,
            ALG_TYPE_AKCIPHER,
            ALG_TYPE_KPP,
            ALG_TYPE_SHASH,
            ALG_TYPE_AHASH,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
            // Every type string fits in the salg_type field with room
            // for a NUL terminator.
            assert!(t[i].len() < ALG_TYPE_LEN);
        }
    }

    #[test]
    fn test_field_sizes() {
        // salg_type is 14 bytes (in include/uapi/linux/if_alg.h).
        assert_eq!(ALG_TYPE_LEN, 14);
        // salg_name is 64 bytes (long enough for cbc(aes-ni-aes-asm)).
        assert_eq!(ALG_NAME_LEN, 64);
        assert!(ALG_NAME_LEN > ALG_TYPE_LEN);
    }

    #[test]
    fn test_setsockopt_ops_dense_1_to_7() {
        let s = [
            ALG_SET_KEY,
            ALG_SET_IV,
            ALG_SET_OP,
            ALG_SET_AEAD_ASSOCLEN,
            ALG_SET_AEAD_AUTHSIZE,
            ALG_SET_DRBG_ENTROPY,
            ALG_SET_KEY_BY_KEY_SERIAL,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_op_encrypt_decrypt_binary() {
        // 0 = decrypt, 1 = encrypt — matches `enum`.
        assert_eq!(ALG_OP_DECRYPT, 0);
        assert_eq!(ALG_OP_ENCRYPT, 1);
        assert_ne!(ALG_OP_DECRYPT, ALG_OP_ENCRYPT);
    }
}
