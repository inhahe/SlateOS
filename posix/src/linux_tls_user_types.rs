//! `<linux/tls.h>` — kernel TLS (kTLS) offload.
//!
//! After a userspace library (OpenSSL/GnuTLS) finishes the TLS
//! handshake, it can hand the negotiated keys to the kernel via
//! `setsockopt(SOL_TLS, TLS_TX/RX)`. The kernel then encrypts and
//! decrypts records on the socket directly, enabling zero-copy
//! `sendfile(2)` of encrypted data and CPU offload to NICs.

// ---------------------------------------------------------------------------
// `setsockopt` level
// ---------------------------------------------------------------------------

pub const SOL_TLS: u32 = 282;

// ---------------------------------------------------------------------------
// `TCP_ULP` setsockopt value to flip a TCP socket into TLS mode
// ---------------------------------------------------------------------------

pub const TLS_ULP_NAME: &str = "tls";

// ---------------------------------------------------------------------------
// `TLS_*` socket options
// ---------------------------------------------------------------------------

pub const TLS_TX: u32 = 1;
pub const TLS_RX: u32 = 2;
pub const TLS_TX_ZEROCOPY_RO: u32 = 3;
pub const TLS_RX_EXPECT_NO_PAD: u32 = 4;

// ---------------------------------------------------------------------------
// Protocol versions (`tls_crypto_info.version`)
// ---------------------------------------------------------------------------

pub const TLS_1_2_VERSION: u16 = (3 << 8) | 3; // 0x0303
pub const TLS_1_3_VERSION: u16 = (3 << 8) | 4; // 0x0304

// ---------------------------------------------------------------------------
// Cipher suites (`tls_crypto_info.cipher_type`)
// ---------------------------------------------------------------------------

pub const TLS_CIPHER_AES_GCM_128: u16 = 51;
pub const TLS_CIPHER_AES_GCM_256: u16 = 52;
pub const TLS_CIPHER_AES_CCM_128: u16 = 53;
pub const TLS_CIPHER_CHACHA20_POLY1305: u16 = 54;
pub const TLS_CIPHER_SM4_GCM: u16 = 55;
pub const TLS_CIPHER_SM4_CCM: u16 = 56;
pub const TLS_CIPHER_ARIA_GCM_128: u16 = 57;
pub const TLS_CIPHER_ARIA_GCM_256: u16 = 58;

// ---------------------------------------------------------------------------
// Key / IV / salt sizes for AES-GCM-128 (`struct tls12_crypto_info_aes_gcm_128`)
// ---------------------------------------------------------------------------

pub const TLS_CIPHER_AES_GCM_128_IV_SIZE: usize = 8;
pub const TLS_CIPHER_AES_GCM_128_KEY_SIZE: usize = 16;
pub const TLS_CIPHER_AES_GCM_128_SALT_SIZE: usize = 4;
pub const TLS_CIPHER_AES_GCM_128_TAG_SIZE: usize = 16;
pub const TLS_CIPHER_AES_GCM_128_REC_SEQ_SIZE: usize = 8;

// Key / IV / salt sizes for AES-GCM-256.
pub const TLS_CIPHER_AES_GCM_256_IV_SIZE: usize = 8;
pub const TLS_CIPHER_AES_GCM_256_KEY_SIZE: usize = 32;
pub const TLS_CIPHER_AES_GCM_256_SALT_SIZE: usize = 4;
pub const TLS_CIPHER_AES_GCM_256_TAG_SIZE: usize = 16;
pub const TLS_CIPHER_AES_GCM_256_REC_SEQ_SIZE: usize = 8;

// Key / IV / salt sizes for ChaCha20-Poly1305 (RFC 7539).
pub const TLS_CIPHER_CHACHA20_POLY1305_IV_SIZE: usize = 12;
pub const TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE: usize = 32;
pub const TLS_CIPHER_CHACHA20_POLY1305_SALT_SIZE: usize = 0;
pub const TLS_CIPHER_CHACHA20_POLY1305_TAG_SIZE: usize = 16;
pub const TLS_CIPHER_CHACHA20_POLY1305_REC_SEQ_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Control message types (`cmsg_type` for TLS record metadata)
// ---------------------------------------------------------------------------

pub const TLS_SET_RECORD_TYPE: u32 = 1;
pub const TLS_GET_RECORD_TYPE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sol_tls_is_282() {
        assert_eq!(SOL_TLS, 282);
        assert_eq!(TLS_ULP_NAME, "tls");
    }

    #[test]
    fn test_tx_rx_sockopts_dense() {
        // TLS_TX and TLS_RX are 1 and 2; the zerocopy/no-pad extensions
        // were added after at 3 and 4.
        assert_eq!(TLS_TX, 1);
        assert_eq!(TLS_RX, 2);
        assert_eq!(TLS_TX_ZEROCOPY_RO, 3);
        assert_eq!(TLS_RX_EXPECT_NO_PAD, 4);
    }

    #[test]
    fn test_protocol_versions_match_record_layer_bytes() {
        // The on-the-wire TLS version bytes: 0x0303 for TLS 1.2, 0x0304
        // for TLS 1.3 (which still advertises 1.2 in the record header,
        // but the kernel knows the real version).
        assert_eq!(TLS_1_2_VERSION, 0x0303);
        assert_eq!(TLS_1_3_VERSION, 0x0304);
        assert_eq!(TLS_1_3_VERSION, TLS_1_2_VERSION + 1);
    }

    #[test]
    fn test_cipher_ids_dense_51_to_58() {
        let c = [
            TLS_CIPHER_AES_GCM_128,
            TLS_CIPHER_AES_GCM_256,
            TLS_CIPHER_AES_CCM_128,
            TLS_CIPHER_CHACHA20_POLY1305,
            TLS_CIPHER_SM4_GCM,
            TLS_CIPHER_SM4_CCM,
            TLS_CIPHER_ARIA_GCM_128,
            TLS_CIPHER_ARIA_GCM_256,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, 51 + i);
        }
    }

    #[test]
    fn test_aes_gcm_sizes_match_rfc_5288() {
        // AES-GCM-128: 16B key, 8B IV (explicit), 4B salt (implicit),
        // 16B tag, 8B sequence. Total nonce is salt||iv = 12B.
        assert_eq!(TLS_CIPHER_AES_GCM_128_KEY_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_128_IV_SIZE, 8);
        assert_eq!(TLS_CIPHER_AES_GCM_128_SALT_SIZE, 4);
        assert_eq!(
            TLS_CIPHER_AES_GCM_128_IV_SIZE + TLS_CIPHER_AES_GCM_128_SALT_SIZE,
            12
        );
        // AES-GCM-256: only the key size differs.
        assert_eq!(TLS_CIPHER_AES_GCM_256_KEY_SIZE, 32);
        assert_eq!(TLS_CIPHER_AES_GCM_256_IV_SIZE, TLS_CIPHER_AES_GCM_128_IV_SIZE);
        assert_eq!(
            TLS_CIPHER_AES_GCM_256_SALT_SIZE,
            TLS_CIPHER_AES_GCM_128_SALT_SIZE
        );
        // Tag is always 16B (full GCM tag).
        assert_eq!(TLS_CIPHER_AES_GCM_128_TAG_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_256_TAG_SIZE, 16);
    }

    #[test]
    fn test_chacha_uses_12b_iv_and_no_explicit_salt() {
        // RFC 7539 ChaCha20-Poly1305 uses a single 12B nonce — kTLS
        // models that as "12B IV, 0B salt" so the union layout still
        // works.
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_IV_SIZE, 12);
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_SALT_SIZE, 0);
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE, 32);
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_TAG_SIZE, 16);
    }

    #[test]
    fn test_cmsg_record_type_pair() {
        // SET/GET record-type cmsgs are 1/2 — used to peek/set
        // the TLS record type (handshake/alert/app-data) on a
        // per-message basis.
        assert_eq!(TLS_SET_RECORD_TYPE, 1);
        assert_eq!(TLS_GET_RECORD_TYPE, 2);
    }
}
