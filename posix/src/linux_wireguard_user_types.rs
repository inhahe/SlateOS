//! `<linux/wireguard.h>` — WireGuard generic-netlink protocol.
//!
//! `wg`, `wg-quick`, and systemd-networkd's WireGuard support all
//! drive the kernel through the generic netlink family registered
//! under `WG_GENL_NAME`. This module enumerates the family magic,
//! commands, attributes, and peer/AllowedIP flags.

// ---------------------------------------------------------------------------
// Generic netlink family
// ---------------------------------------------------------------------------

/// Family name registered by the kernel module.
pub const WG_GENL_NAME: &str = "wireguard";
/// Family version (single API revision since 5.6).
pub const WG_GENL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Key / signature sizes (Curve25519 / ChaCha20-Poly1305)
// ---------------------------------------------------------------------------

/// Public-key length (Curve25519).
pub const WG_KEY_LEN: usize = 32;
/// Pre-shared-key length.
pub const WG_PSK_LEN: usize = 32;
/// HKDF/AEAD shared-secret length.
pub const WG_HASH_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Commands (genl cmd field)
// ---------------------------------------------------------------------------

/// `WG_CMD_GET_DEVICE` — query a configured device.
pub const WG_CMD_GET_DEVICE: u8 = 0;
/// `WG_CMD_SET_DEVICE` — install / update device configuration.
pub const WG_CMD_SET_DEVICE: u8 = 1;

// ---------------------------------------------------------------------------
// Device attributes (WGDEVICE_A_*)
// ---------------------------------------------------------------------------

/// `WGDEVICE_A_UNSPEC` — reserved.
pub const WGDEVICE_A_UNSPEC: u16 = 0;
/// `WGDEVICE_A_IFINDEX` — netdev ifindex (u32).
pub const WGDEVICE_A_IFINDEX: u16 = 1;
/// `WGDEVICE_A_IFNAME` — netdev name (string).
pub const WGDEVICE_A_IFNAME: u16 = 2;
/// `WGDEVICE_A_PRIVATE_KEY` — 32-byte privkey.
pub const WGDEVICE_A_PRIVATE_KEY: u16 = 3;
/// `WGDEVICE_A_PUBLIC_KEY` — 32-byte pubkey.
pub const WGDEVICE_A_PUBLIC_KEY: u16 = 4;
/// `WGDEVICE_A_FLAGS` — WGDEVICE_F_REPLACE_PEERS.
pub const WGDEVICE_A_FLAGS: u16 = 5;
/// `WGDEVICE_A_LISTEN_PORT` — UDP listen port (u16).
pub const WGDEVICE_A_LISTEN_PORT: u16 = 6;
/// `WGDEVICE_A_FWMARK` — fwmark on encapsulated packets (u32).
pub const WGDEVICE_A_FWMARK: u16 = 7;
/// `WGDEVICE_A_PEERS` — nested peer list.
pub const WGDEVICE_A_PEERS: u16 = 8;

/// `WGDEVICE_F_REPLACE_PEERS` — clear all peers before applying.
pub const WGDEVICE_F_REPLACE_PEERS: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Peer attributes (WGPEER_A_*)
// ---------------------------------------------------------------------------

/// `WGPEER_A_PUBLIC_KEY` — peer's pubkey.
pub const WGPEER_A_PUBLIC_KEY: u16 = 1;
/// `WGPEER_A_PRESHARED_KEY`.
pub const WGPEER_A_PRESHARED_KEY: u16 = 2;
/// `WGPEER_A_FLAGS`.
pub const WGPEER_A_FLAGS: u16 = 3;
/// `WGPEER_A_ENDPOINT` — sockaddr_in or sockaddr_in6.
pub const WGPEER_A_ENDPOINT: u16 = 4;
/// `WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL` — seconds (u16).
pub const WGPEER_A_PERSISTENT_KEEPALIVE_INTERVAL: u16 = 5;
/// `WGPEER_A_LAST_HANDSHAKE_TIME` — struct __kernel_timespec (out-only).
pub const WGPEER_A_LAST_HANDSHAKE_TIME: u16 = 6;
/// `WGPEER_A_RX_BYTES` — counter (u64).
pub const WGPEER_A_RX_BYTES: u16 = 7;
/// `WGPEER_A_TX_BYTES` — counter (u64).
pub const WGPEER_A_TX_BYTES: u16 = 8;
/// `WGPEER_A_ALLOWEDIPS` — nested list.
pub const WGPEER_A_ALLOWEDIPS: u16 = 9;
/// `WGPEER_A_PROTOCOL_VERSION` — always 1.
pub const WGPEER_A_PROTOCOL_VERSION: u16 = 10;

/// `WGPEER_F_REMOVE_ME` — delete this peer.
pub const WGPEER_F_REMOVE_ME: u32 = 1 << 0;
/// `WGPEER_F_REPLACE_ALLOWEDIPS` — clear AllowedIPs before applying.
pub const WGPEER_F_REPLACE_ALLOWEDIPS: u32 = 1 << 1;
/// `WGPEER_F_UPDATE_ONLY` — fail with -ENOENT if peer doesn't exist.
pub const WGPEER_F_UPDATE_ONLY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// AllowedIPs attributes (WGALLOWEDIP_A_*)
// ---------------------------------------------------------------------------

/// `WGALLOWEDIP_A_FAMILY` — AF_INET / AF_INET6 (u16).
pub const WGALLOWEDIP_A_FAMILY: u16 = 1;
/// `WGALLOWEDIP_A_IPADDR` — raw 4 or 16 bytes.
pub const WGALLOWEDIP_A_IPADDR: u16 = 2;
/// `WGALLOWEDIP_A_CIDR_MASK` — prefix length (u8).
pub const WGALLOWEDIP_A_CIDR_MASK: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_name_and_version() {
        assert_eq!(WG_GENL_NAME, "wireguard");
        assert_eq!(WG_GENL_VERSION, 1);
    }

    #[test]
    fn test_key_sizes() {
        // Curve25519 keys are always 32 bytes.
        assert_eq!(WG_KEY_LEN, 32);
        assert_eq!(WG_PSK_LEN, WG_KEY_LEN);
        assert_eq!(WG_HASH_LEN, WG_KEY_LEN);
    }

    #[test]
    fn test_commands_distinct() {
        assert_ne!(WG_CMD_GET_DEVICE, WG_CMD_SET_DEVICE);
        assert_eq!(WG_CMD_GET_DEVICE, 0);
    }

    #[test]
    fn test_device_attrs_dense() {
        let a = [
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
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert!(WGDEVICE_F_REPLACE_PEERS.is_power_of_two());
    }

    #[test]
    fn test_peer_attrs_dense() {
        let a = [
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
        // Peer attrs start at 1 (0 is UNSPEC reserved).
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_peer_flags_pow2_distinct() {
        let f = [
            WGPEER_F_REMOVE_ME,
            WGPEER_F_REPLACE_ALLOWEDIPS,
            WGPEER_F_UPDATE_ONLY,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_allowedip_attrs_dense() {
        assert_eq!(WGALLOWEDIP_A_FAMILY, 1);
        assert_eq!(WGALLOWEDIP_A_IPADDR, 2);
        assert_eq!(WGALLOWEDIP_A_CIDR_MASK, 3);
    }
}
