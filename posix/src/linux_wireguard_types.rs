//! WireGuard timer and protocol constants.
//!
//! WireGuard is a modern VPN tunnel operating at layer 3.
//! These constants define protocol timing, message types,
//! and cryptographic parameters used by the WireGuard
//! kernel module (net/wireguard).

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Handshake initiation message.
pub const WG_MSG_HANDSHAKE_INITIATION: u32 = 1;
/// Handshake response message.
pub const WG_MSG_HANDSHAKE_RESPONSE: u32 = 2;
/// Cookie reply (under load).
pub const WG_MSG_HANDSHAKE_COOKIE: u32 = 3;
/// Transport data message.
pub const WG_MSG_TRANSPORT_DATA: u32 = 4;

// ---------------------------------------------------------------------------
// Timing constants (seconds unless noted)
// ---------------------------------------------------------------------------

/// Rekey after this many seconds.
pub const WG_REKEY_AFTER_TIME: u32 = 120;
/// Reject messages after this many seconds without a new handshake.
pub const WG_REJECT_AFTER_TIME: u32 = 180;
/// Rekey attempt timeout.
pub const WG_REKEY_TIMEOUT: u32 = 5;
/// Keepalive interval (0 = disabled).
pub const WG_KEEPALIVE_TIMEOUT: u32 = 10;

// ---------------------------------------------------------------------------
// Data limits
// ---------------------------------------------------------------------------

/// Rekey after this many messages.
pub const WG_REKEY_AFTER_MESSAGES: u64 = 1 << 60;
/// Reject after this many messages.
pub const WG_REJECT_AFTER_MESSAGES: u64 = u64::MAX - (1 << 13);

// ---------------------------------------------------------------------------
// Message sizes (bytes)
// ---------------------------------------------------------------------------

/// Handshake initiation message size.
pub const WG_HANDSHAKE_INITIATION_SIZE: usize = 148;
/// Handshake response message size.
pub const WG_HANDSHAKE_RESPONSE_SIZE: usize = 92;
/// Cookie reply message size.
pub const WG_COOKIE_REPLY_SIZE: usize = 64;
/// Transport header size (before encrypted payload).
pub const WG_TRANSPORT_HEADER_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Cryptographic constants
// ---------------------------------------------------------------------------

/// Noise protocol name.
pub const WG_NOISE_PROTOCOL: &str = "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";
/// Public key size (Curve25519).
pub const WG_PUBLIC_KEY_SIZE: usize = 32;
/// Private key size.
pub const WG_PRIVATE_KEY_SIZE: usize = 32;
/// Preshared key size.
pub const WG_PRESHARED_KEY_SIZE: usize = 32;
/// AEAD authentication tag size (ChaCha20Poly1305).
pub const WG_AEAD_TAG_SIZE: usize = 16;
/// MAC size (BLAKE2s).
pub const WG_MAC_SIZE: usize = 16;
/// Cookie size.
pub const WG_COOKIE_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            WG_MSG_HANDSHAKE_INITIATION,
            WG_MSG_HANDSHAKE_RESPONSE,
            WG_MSG_HANDSHAKE_COOKIE,
            WG_MSG_TRANSPORT_DATA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_timing_order() {
        assert!(WG_REKEY_AFTER_TIME < WG_REJECT_AFTER_TIME);
        assert!(WG_REKEY_TIMEOUT < WG_REKEY_AFTER_TIME);
    }

    #[test]
    fn test_message_limits() {
        assert!(WG_REKEY_AFTER_MESSAGES < WG_REJECT_AFTER_MESSAGES);
    }

    #[test]
    fn test_message_sizes() {
        assert!(WG_HANDSHAKE_INITIATION_SIZE > WG_HANDSHAKE_RESPONSE_SIZE);
        assert!(WG_HANDSHAKE_RESPONSE_SIZE > WG_COOKIE_REPLY_SIZE);
        assert!(WG_TRANSPORT_HEADER_SIZE > 0);
    }

    #[test]
    fn test_key_sizes() {
        assert_eq!(WG_PUBLIC_KEY_SIZE, 32);
        assert_eq!(WG_PRIVATE_KEY_SIZE, 32);
        assert_eq!(WG_PRESHARED_KEY_SIZE, 32);
    }

    #[test]
    fn test_crypto_sizes() {
        assert_eq!(WG_AEAD_TAG_SIZE, 16);
        assert_eq!(WG_MAC_SIZE, 16);
        assert_eq!(WG_COOKIE_SIZE, 16);
    }
}
