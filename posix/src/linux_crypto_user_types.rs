//! `<linux/if_alg.h>` — AF_ALG socket family for kernel-crypto offload.
//!
//! AF_ALG lets userspace push data through the kernel crypto API
//! without re-implementing AES/SHA/HMAC. systemd-cryptsetup, fscrypt's
//! `keyctl`, and openssh use it for hardware-accelerated paths.

// ---------------------------------------------------------------------------
// Address family and protocol level
// ---------------------------------------------------------------------------

/// `AF_ALG` socket family.
pub const AF_ALG: u32 = 38;
/// SOL_ALG socket option level.
pub const SOL_ALG: u32 = 279;

// ---------------------------------------------------------------------------
// Socket options (level=SOL_ALG, name=)
// ---------------------------------------------------------------------------

/// `ALG_SET_KEY` — install symmetric key.
pub const ALG_SET_KEY: u32 = 1;
/// `ALG_SET_IV` — install AEAD/CBC IV via CMSG.
pub const ALG_SET_IV: u32 = 2;
/// `ALG_SET_OP` — encrypt vs decrypt direction via CMSG.
pub const ALG_SET_OP: u32 = 3;
/// `ALG_SET_AEAD_ASSOCLEN` — AAD length.
pub const ALG_SET_AEAD_ASSOCLEN: u32 = 4;
/// `ALG_SET_AEAD_AUTHSIZE` — tag length.
pub const ALG_SET_AEAD_AUTHSIZE: u32 = 5;
/// `ALG_SET_DRBG_ENTROPY` — install DRBG seed.
pub const ALG_SET_DRBG_ENTROPY: u32 = 6;
/// `ALG_SET_KEY_BY_KEY_SERIAL` — install key from keyring serial.
pub const ALG_SET_KEY_BY_KEY_SERIAL: u32 = 7;

// ---------------------------------------------------------------------------
// ALG_SET_OP values (passed in CMSG data)
// ---------------------------------------------------------------------------

/// `ALG_OP_DECRYPT`.
pub const ALG_OP_DECRYPT: u32 = 0;
/// `ALG_OP_ENCRYPT`.
pub const ALG_OP_ENCRYPT: u32 = 1;

// ---------------------------------------------------------------------------
// Type/name strings (struct sockaddr_alg)
// ---------------------------------------------------------------------------

/// `salg_type = "hash"` — message digest / HMAC.
pub const ALG_TYPE_HASH: &str = "hash";
/// `salg_type = "skcipher"` — symmetric cipher.
pub const ALG_TYPE_SKCIPHER: &str = "skcipher";
/// `salg_type = "aead"` — authenticated cipher.
pub const ALG_TYPE_AEAD: &str = "aead";
/// `salg_type = "rng"` — random number generator.
pub const ALG_TYPE_RNG: &str = "rng";
/// `salg_type = "akcipher"` — asymmetric cipher.
pub const ALG_TYPE_AKCIPHER: &str = "akcipher";
/// `salg_type = "kpp"` — key agreement.
pub const ALG_TYPE_KPP: &str = "kpp";

/// Max length of `salg_type` (NUL-terminated buffer).
pub const ALG_MAX_TYPE_NAME: usize = 14;
/// Max length of `salg_name`.
pub const ALG_MAX_ALG_NAME: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_and_level() {
        // AF_ALG = 38 since v2.6.38.
        assert_eq!(AF_ALG, 38);
        assert_eq!(SOL_ALG, 279);
    }

    #[test]
    fn test_setopts_dense_starting_from_1() {
        let o = [
            ALG_SET_KEY,
            ALG_SET_IV,
            ALG_SET_OP,
            ALG_SET_AEAD_ASSOCLEN,
            ALG_SET_AEAD_AUTHSIZE,
            ALG_SET_DRBG_ENTROPY,
            ALG_SET_KEY_BY_KEY_SERIAL,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_op_values() {
        // Encrypt = 1 lets sendmsg() drive direction without a separate
        // ioctl. Decrypt = 0 must remain zero (default) for clarity.
        assert_eq!(ALG_OP_DECRYPT, 0);
        assert_eq!(ALG_OP_ENCRYPT, 1);
    }

    #[test]
    fn test_type_strings_distinct_and_short() {
        let t = [
            ALG_TYPE_HASH,
            ALG_TYPE_SKCIPHER,
            ALG_TYPE_AEAD,
            ALG_TYPE_RNG,
            ALG_TYPE_AKCIPHER,
            ALG_TYPE_KPP,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
            // Every type string must fit (with NUL) in the 14-byte
            // salg_type buffer.
            assert!(t[i].len() < ALG_MAX_TYPE_NAME);
        }
    }

    #[test]
    fn test_name_size_limits() {
        // The 14/64 sizes are part of the on-the-wire sockaddr_alg
        // layout and cannot change without ABI breakage.
        assert_eq!(ALG_MAX_TYPE_NAME, 14);
        assert_eq!(ALG_MAX_ALG_NAME, 64);
    }
}
