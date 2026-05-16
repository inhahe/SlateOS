//! `<linux/handshake.h>` — Kernel TLS handshake upcall constants.
//!
//! The handshake subsystem delegates TLS handshakes from the kernel
//! to a userspace handler. When the kernel needs to establish a TLS
//! session (e.g., for NFS-over-TLS), it sends a netlink request to
//! a userspace daemon that completes the handshake.

// ---------------------------------------------------------------------------
// Handshake netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const HANDSHAKE_CMD_UNSPEC: u8 = 0;
/// Ready (userspace handler registering).
pub const HANDSHAKE_CMD_READY: u8 = 1;
/// Accept (kernel requesting handshake).
pub const HANDSHAKE_CMD_ACCEPT: u8 = 2;
/// Done (handshake completed).
pub const HANDSHAKE_CMD_DONE: u8 = 3;

// ---------------------------------------------------------------------------
// Handshake message types
// ---------------------------------------------------------------------------

/// Unspecified type.
pub const HANDSHAKE_MSG_TYPE_UNSPEC: u32 = 0;
/// Client hello.
pub const HANDSHAKE_MSG_TYPE_CLIENTHELLO: u32 = 1;
/// Server hello.
pub const HANDSHAKE_MSG_TYPE_SERVERHELLO: u32 = 2;

// ---------------------------------------------------------------------------
// Handshake auth modes
// ---------------------------------------------------------------------------

/// Unspecified authentication.
pub const HANDSHAKE_AUTH_UNSPEC: u32 = 0;
/// Unauthenticated (anonymous).
pub const HANDSHAKE_AUTH_UNAUTH: u32 = 1;
/// Pre-shared key authentication.
pub const HANDSHAKE_AUTH_PSK: u32 = 2;
/// X.509 certificate authentication.
pub const HANDSHAKE_AUTH_X509: u32 = 3;

// ---------------------------------------------------------------------------
// Handshake handler types
// ---------------------------------------------------------------------------

/// Unspecified handler type.
pub const HANDSHAKE_HANDLER_UNSPEC: u32 = 0;
/// TLS client handler.
pub const HANDSHAKE_HANDLER_TLSHD: u32 = 1;
/// Maximum handler type.
pub const HANDSHAKE_HANDLER_MAX: u32 = 2;

// ---------------------------------------------------------------------------
// Handshake netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const HANDSHAKE_A_ACCEPT_UNSPEC: u16 = 0;
/// Socket FD.
pub const HANDSHAKE_A_ACCEPT_SOCKFD: u16 = 1;
/// Handler class.
pub const HANDSHAKE_A_ACCEPT_HANDLER_CLASS: u16 = 2;
/// Message type.
pub const HANDSHAKE_A_ACCEPT_MESSAGE_TYPE: u16 = 3;
/// Timeout (ms).
pub const HANDSHAKE_A_ACCEPT_TIMEOUT: u16 = 4;
/// Auth mode.
pub const HANDSHAKE_A_ACCEPT_AUTH_MODE: u16 = 5;
/// Peer identity.
pub const HANDSHAKE_A_ACCEPT_PEER_IDENTITY: u16 = 6;
/// Certificate.
pub const HANDSHAKE_A_ACCEPT_CERTIFICATE: u16 = 7;
/// Peer name.
pub const HANDSHAKE_A_ACCEPT_PEERNAME: u16 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            HANDSHAKE_CMD_UNSPEC, HANDSHAKE_CMD_READY,
            HANDSHAKE_CMD_ACCEPT, HANDSHAKE_CMD_DONE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            HANDSHAKE_MSG_TYPE_UNSPEC, HANDSHAKE_MSG_TYPE_CLIENTHELLO,
            HANDSHAKE_MSG_TYPE_SERVERHELLO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_auth_modes_distinct() {
        let modes = [
            HANDSHAKE_AUTH_UNSPEC, HANDSHAKE_AUTH_UNAUTH,
            HANDSHAKE_AUTH_PSK, HANDSHAKE_AUTH_X509,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            HANDSHAKE_A_ACCEPT_UNSPEC, HANDSHAKE_A_ACCEPT_SOCKFD,
            HANDSHAKE_A_ACCEPT_HANDLER_CLASS, HANDSHAKE_A_ACCEPT_MESSAGE_TYPE,
            HANDSHAKE_A_ACCEPT_TIMEOUT, HANDSHAKE_A_ACCEPT_AUTH_MODE,
            HANDSHAKE_A_ACCEPT_PEER_IDENTITY, HANDSHAKE_A_ACCEPT_CERTIFICATE,
            HANDSHAKE_A_ACCEPT_PEERNAME,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
