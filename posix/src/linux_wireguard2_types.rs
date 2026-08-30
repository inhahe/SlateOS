//! `<linux/wireguard.h>` — Additional WireGuard constants.
//!
//! Supplementary WireGuard constants covering peer flags,
//! allowed IP flags, and message types.

// ---------------------------------------------------------------------------
// WireGuard device flags (WGDEVICE_F_*)
// ---------------------------------------------------------------------------

/// Replace peers (remove all existing).
pub const WGDEVICE_F_REPLACE_PEERS: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// WireGuard peer flags (WGPEER_F_*)
// ---------------------------------------------------------------------------

/// Remove this peer.
pub const WGPEER_F_REMOVE_ME: u32 = 1 << 0;
/// Replace allowed IPs.
pub const WGPEER_F_REPLACE_ALLOWEDIPS: u32 = 1 << 1;
/// Update only (don't create).
pub const WGPEER_F_UPDATE_ONLY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// WireGuard message types
// ---------------------------------------------------------------------------

/// Handshake initiation.
pub const WG_MSG_HANDSHAKE_INITIATION: u32 = 1;
/// Handshake response.
pub const WG_MSG_HANDSHAKE_RESPONSE: u32 = 2;
/// Handshake cookie.
pub const WG_MSG_HANDSHAKE_COOKIE: u32 = 3;
/// Transport data.
pub const WG_MSG_TRANSPORT_DATA: u32 = 4;

// ---------------------------------------------------------------------------
// WireGuard key/nonce sizes
// ---------------------------------------------------------------------------

/// Public/private key length (Curve25519).
pub const WG_KEY_LEN: u32 = 32;
/// Nonce/counter length (ChaCha20).
pub const WG_NONCE_LEN: u32 = 8;
/// MAC length (Poly1305).
pub const WG_MAC_LEN: u32 = 16;
/// Cookie length.
pub const WG_COOKIE_LEN: u32 = 16;
/// Hash length (BLAKE2s).
pub const WG_HASH_LEN: u32 = 32;
/// Timestamp length (TAI64N).
pub const WG_TIMESTAMP_LEN: u32 = 12;

// ---------------------------------------------------------------------------
// WireGuard timing constants (seconds)
// ---------------------------------------------------------------------------

/// Rekey after (seconds).
pub const WG_REKEY_AFTER_TIME: u32 = 120;
/// Reject after (seconds).
pub const WG_REJECT_AFTER_TIME: u32 = 180;
/// Keepalive timeout default.
pub const WG_KEEPALIVE_TIMEOUT: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_flag() {
        assert_eq!(WGDEVICE_F_REPLACE_PEERS, 1);
    }

    #[test]
    fn test_peer_flags_power_of_two() {
        assert!(WGPEER_F_REMOVE_ME.is_power_of_two());
        assert!(WGPEER_F_REPLACE_ALLOWEDIPS.is_power_of_two());
        assert!(WGPEER_F_UPDATE_ONLY.is_power_of_two());
    }

    #[test]
    fn test_peer_flags_no_overlap() {
        let flags = [
            WGPEER_F_REMOVE_ME,
            WGPEER_F_REPLACE_ALLOWEDIPS,
            WGPEER_F_UPDATE_ONLY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

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
    fn test_key_sizes() {
        assert_eq!(WG_KEY_LEN, 32);
        assert_eq!(WG_MAC_LEN, 16);
        assert_eq!(WG_HASH_LEN, 32);
    }

    #[test]
    fn test_timing_ordering() {
        assert!(WG_KEEPALIVE_TIMEOUT < WG_REKEY_AFTER_TIME);
        assert!(WG_REKEY_AFTER_TIME < WG_REJECT_AFTER_TIME);
    }
}
