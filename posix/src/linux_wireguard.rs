//! `<linux/wireguard.h>` — WireGuard VPN constants.
//!
//! WireGuard is a modern VPN tunnel implemented in the kernel.
//! Configuration is done via Generic Netlink. This module defines
//! the netlink commands, attributes, and constants.

// ---------------------------------------------------------------------------
// Generic Netlink family
// ---------------------------------------------------------------------------

/// WireGuard Generic Netlink family name.
pub const WG_GENL_NAME: &str = "wireguard";
/// WireGuard Generic Netlink version.
pub const WG_GENL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Netlink commands
// ---------------------------------------------------------------------------

/// Get device.
pub const WG_CMD_GET_DEVICE: u8 = 0;
/// Set device.
pub const WG_CMD_SET_DEVICE: u8 = 1;

// ---------------------------------------------------------------------------
// Device attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const WGDEVICE_A_UNSPEC: u16 = 0;
/// Interface index.
pub const WGDEVICE_A_IFINDEX: u16 = 1;
/// Interface name.
pub const WGDEVICE_A_IFNAME: u16 = 2;
/// Private key.
pub const WGDEVICE_A_PRIVATE_KEY: u16 = 3;
/// Public key.
pub const WGDEVICE_A_PUBLIC_KEY: u16 = 4;
/// Flags.
pub const WGDEVICE_A_FLAGS: u16 = 5;
/// Listen port.
pub const WGDEVICE_A_LISTEN_PORT: u16 = 6;
/// Fwmark.
pub const WGDEVICE_A_FWMARK: u16 = 7;
/// Peers (nested).
pub const WGDEVICE_A_PEERS: u16 = 8;

// ---------------------------------------------------------------------------
// Peer attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const WGPEER_A_UNSPEC: u16 = 0;
/// Public key.
pub const WGPEER_A_PUBLIC_KEY: u16 = 1;
/// Preshared key.
pub const WGPEER_A_PRESHARED_KEY: u16 = 2;
/// Flags.
pub const WGPEER_A_FLAGS: u16 = 3;
/// Endpoint address.
pub const WGPEER_A_ENDPOINT: u16 = 4;
/// Persistent keepalive interval.
pub const WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL: u16 = 5;
/// Last handshake time.
pub const WGPEER_A_LAST_HANDSHAKE_TIME: u16 = 6;
/// RX bytes.
pub const WGPEER_A_RX_BYTES: u16 = 7;
/// TX bytes.
pub const WGPEER_A_TX_BYTES: u16 = 8;
/// Allowed IPs (nested).
pub const WGPEER_A_ALLOWEDIPS: u16 = 9;
/// Protocol version.
pub const WGPEER_A_PROTOCOL_VERSION: u16 = 10;

// ---------------------------------------------------------------------------
// Device flags
// ---------------------------------------------------------------------------

/// Replace peers list.
pub const WGDEVICE_F_REPLACE_PEERS: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Peer flags
// ---------------------------------------------------------------------------

/// Remove peer.
pub const WGPEER_F_REMOVE_ME: u32 = 1 << 0;
/// Replace allowed IPs.
pub const WGPEER_F_REPLACE_ALLOWEDIPS: u32 = 1 << 1;
/// Update only (don't create).
pub const WGPEER_F_UPDATE_ONLY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Key sizes
// ---------------------------------------------------------------------------

/// Key size (Curve25519, 32 bytes).
pub const WG_KEY_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_name() {
        assert_eq!(WG_GENL_NAME, "wireguard");
    }

    #[test]
    fn test_cmds_distinct() {
        assert_ne!(WG_CMD_GET_DEVICE, WG_CMD_SET_DEVICE);
    }

    #[test]
    fn test_device_attrs_distinct() {
        let attrs = [
            WGDEVICE_A_UNSPEC,
            WGDEVICE_A_IFINDEX,
            WGDEVICE_A_IFNAME,
            WGDEVICE_A_PRIVATE_KEY,
            WGDEVICE_A_PUBLIC_KEY,
            WGDEVICE_A_FLAGS,
            WGDEVICE_A_LISTEN_PORT,
            WGDEVICE_A_FWMARK,
            WGDEVICE_A_PEERS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_peer_attrs_distinct() {
        let attrs = [
            WGPEER_A_UNSPEC,
            WGPEER_A_PUBLIC_KEY,
            WGPEER_A_PRESHARED_KEY,
            WGPEER_A_FLAGS,
            WGPEER_A_ENDPOINT,
            WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL,
            WGPEER_A_LAST_HANDSHAKE_TIME,
            WGPEER_A_RX_BYTES,
            WGPEER_A_TX_BYTES,
            WGPEER_A_ALLOWEDIPS,
            WGPEER_A_PROTOCOL_VERSION,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_peer_flags_powers_of_two() {
        let flags = [
            WGPEER_F_REMOVE_ME,
            WGPEER_F_REPLACE_ALLOWEDIPS,
            WGPEER_F_UPDATE_ONLY,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_key_len() {
        assert_eq!(WG_KEY_LEN, 32);
    }
}
