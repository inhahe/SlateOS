//! `<linux/handshake.h>` — Kernel TLS handshake upcall constants.
//!
//! The handshake netlink interface (Linux 6.3+) allows the kernel to
//! request TLS handshakes from a userspace daemon (tlshd). When a
//! kernel subsystem (NFS, SMB) needs a TLS-secured connection, it
//! sends a handshake request via netlink; the userspace TLS library
//! performs the handshake (certificate exchange, key negotiation),
//! then installs the negotiated keys back into the kernel via
//! kTLS (kernel TLS). Avoids putting TLS libraries in kernel space.

// ---------------------------------------------------------------------------
// Handshake netlink commands
// ---------------------------------------------------------------------------

/// Accept a pending handshake request.
pub const HANDSHAKE_CMD_READY: u32 = 1;
/// Notify that handshake is complete.
pub const HANDSHAKE_CMD_DONE: u32 = 2;
/// Request a new handshake.
pub const HANDSHAKE_CMD_ACCEPT: u32 = 3;

// ---------------------------------------------------------------------------
// Handshake protocol types
// ---------------------------------------------------------------------------

/// Unspecified protocol.
pub const HANDSHAKE_PROTO_UNSPEC: u32 = 0;
/// TLS handshake protocol.
pub const HANDSHAKE_PROTO_TLS: u32 = 1;

// ---------------------------------------------------------------------------
// TLS handshake types
// ---------------------------------------------------------------------------

/// TLS client handshake (connect).
pub const HANDSHAKE_TLS_TYPE_CLIENTHELLO: u32 = 0;
/// TLS server handshake (accept).
pub const HANDSHAKE_TLS_TYPE_SERVERHELLO: u32 = 1;

// ---------------------------------------------------------------------------
// Handshake auth modes
// ---------------------------------------------------------------------------

/// Unspecified authentication.
pub const HANDSHAKE_AUTH_UNSPEC: u32 = 0;
/// Pre-shared key (PSK) authentication.
pub const HANDSHAKE_AUTH_PSK: u32 = 1;
/// X.509 certificate authentication.
pub const HANDSHAKE_AUTH_X509: u32 = 2;

// ---------------------------------------------------------------------------
// Handshake netlink attributes
// ---------------------------------------------------------------------------

/// Socket file descriptor attribute.
pub const HANDSHAKE_A_ACCEPT_SOCKFD: u32 = 1;
/// Handler class attribute.
pub const HANDSHAKE_A_ACCEPT_HANDLER_CLASS: u32 = 2;
/// Message type attribute.
pub const HANDSHAKE_A_ACCEPT_MESSAGE_TYPE: u32 = 3;
/// Timeout (milliseconds) attribute.
pub const HANDSHAKE_A_ACCEPT_TIMEOUT: u32 = 4;
/// Auth mode attribute.
pub const HANDSHAKE_A_ACCEPT_AUTH_MODE: u32 = 5;
/// Peer identity (certificate) attribute.
pub const HANDSHAKE_A_ACCEPT_PEER_IDENTITY: u32 = 6;
/// Protocol attribute.
pub const HANDSHAKE_A_ACCEPT_CERTIFICATE: u32 = 7;
/// Peername (SNI) attribute.
pub const HANDSHAKE_A_ACCEPT_PEERNAME: u32 = 8;

// ---------------------------------------------------------------------------
// Done status codes
// ---------------------------------------------------------------------------

/// Handshake completed successfully.
pub const HANDSHAKE_DONE_SUCCESS: u32 = 0;
/// Handshake timed out.
pub const HANDSHAKE_DONE_TIMEOUT: u32 = 1;
/// Handshake failed (authentication or protocol error).
pub const HANDSHAKE_DONE_AUTH_FAIL: u32 = 2;
/// Handshake was aborted.
pub const HANDSHAKE_DONE_ABORTED: u32 = 3;

// ---------------------------------------------------------------------------
// Multicast groups
// ---------------------------------------------------------------------------

/// Netlink multicast group for handshake notifications.
pub const HANDSHAKE_MCGRP_NONE: u32 = 0;
/// Netlink multicast group for TLS handshake requests.
pub const HANDSHAKE_MCGRP_TLSHD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            HANDSHAKE_CMD_READY,
            HANDSHAKE_CMD_DONE,
            HANDSHAKE_CMD_ACCEPT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        assert_ne!(HANDSHAKE_PROTO_UNSPEC, HANDSHAKE_PROTO_TLS);
    }

    #[test]
    fn test_tls_types_distinct() {
        assert_ne!(
            HANDSHAKE_TLS_TYPE_CLIENTHELLO,
            HANDSHAKE_TLS_TYPE_SERVERHELLO
        );
    }

    #[test]
    fn test_auth_modes_distinct() {
        let auths = [
            HANDSHAKE_AUTH_UNSPEC,
            HANDSHAKE_AUTH_PSK,
            HANDSHAKE_AUTH_X509,
        ];
        for i in 0..auths.len() {
            for j in (i + 1)..auths.len() {
                assert_ne!(auths[i], auths[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            HANDSHAKE_A_ACCEPT_SOCKFD,
            HANDSHAKE_A_ACCEPT_HANDLER_CLASS,
            HANDSHAKE_A_ACCEPT_MESSAGE_TYPE,
            HANDSHAKE_A_ACCEPT_TIMEOUT,
            HANDSHAKE_A_ACCEPT_AUTH_MODE,
            HANDSHAKE_A_ACCEPT_PEER_IDENTITY,
            HANDSHAKE_A_ACCEPT_CERTIFICATE,
            HANDSHAKE_A_ACCEPT_PEERNAME,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_done_statuses_distinct() {
        let statuses = [
            HANDSHAKE_DONE_SUCCESS,
            HANDSHAKE_DONE_TIMEOUT,
            HANDSHAKE_DONE_AUTH_FAIL,
            HANDSHAKE_DONE_ABORTED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_mcgrp_distinct() {
        assert_ne!(HANDSHAKE_MCGRP_NONE, HANDSHAKE_MCGRP_TLSHD);
    }
}
