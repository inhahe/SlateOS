//! `<linux/handshake.h>` — TLS handshake upcall (generic netlink family).
//!
//! Added in Linux 6.5 to let the kernel delegate TLS/QUIC handshakes
//! to a userspace daemon (`tlshd`, part of ktls-utils). The NFS, SMB,
//! and AF_TLS subsystems use this to set up kTLS sockets without
//! linking OpenSSL into the kernel.

// ---------------------------------------------------------------------------
// Generic netlink family identity
// ---------------------------------------------------------------------------

/// Family name registered via `genl_register_family`.
pub const HANDSHAKE_FAMILY_NAME: &str = "handshake";
/// Wire ABI version.
pub const HANDSHAKE_FAMILY_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Multicast group
// ---------------------------------------------------------------------------

/// Name of the multicast group that distributes pending handshakes.
pub const HANDSHAKE_MCGRP_NONE: &str = "none";
/// Multicast group for TLS-handshake requests.
pub const HANDSHAKE_MCGRP_TLSHD: &str = "tlshd";

// ---------------------------------------------------------------------------
// enum handshake_msg_type
// ---------------------------------------------------------------------------

/// Sentinel (unused).
pub const HANDSHAKE_MSG_TYPE_UNSPEC: u32 = 0;
/// Client-side handshake request.
pub const HANDSHAKE_MSG_TYPE_CLIENTHELLO: u32 = 1;
/// Server-side handshake request.
pub const HANDSHAKE_MSG_TYPE_SERVERHELLO: u32 = 2;

// ---------------------------------------------------------------------------
// enum handshake_auth (negotiated authentication mode)
// ---------------------------------------------------------------------------

/// Sentinel.
pub const HANDSHAKE_AUTH_UNSPEC: u32 = 0;
/// Unauthenticated (anon).
pub const HANDSHAKE_AUTH_UNAUTH: u32 = 1;
/// Pre-shared key.
pub const HANDSHAKE_AUTH_PSK: u32 = 2;
/// X.509 certificate.
pub const HANDSHAKE_AUTH_X509: u32 = 3;

// ---------------------------------------------------------------------------
// enum handshake_cmd
// ---------------------------------------------------------------------------

/// Sentinel.
pub const HANDSHAKE_CMD_UNSPEC: u32 = 0;
/// `HANDSHAKE_CMD_READY` — daemon advertises readiness.
pub const HANDSHAKE_CMD_READY: u32 = 1;
/// `HANDSHAKE_CMD_ACCEPT` — daemon claims a pending request.
pub const HANDSHAKE_CMD_ACCEPT: u32 = 2;
/// `HANDSHAKE_CMD_DONE` — daemon returns the handshake outcome.
pub const HANDSHAKE_CMD_DONE: u32 = 3;

// ---------------------------------------------------------------------------
// Netlink attribute IDs (enum handshake_a_x509 / handshake_a_done)
// ---------------------------------------------------------------------------

/// `HANDSHAKE_A_ACCEPT_SOCKFD` — socket fd to upgrade.
pub const HANDSHAKE_A_ACCEPT_SOCKFD: u32 = 1;
/// `HANDSHAKE_A_ACCEPT_HANDLER_CLASS` — protocol family.
pub const HANDSHAKE_A_ACCEPT_HANDLER_CLASS: u32 = 2;
/// `HANDSHAKE_A_ACCEPT_MESSAGE_TYPE` — client vs server hello.
pub const HANDSHAKE_A_ACCEPT_MESSAGE_TYPE: u32 = 3;
/// `HANDSHAKE_A_ACCEPT_TIMEOUT` — kernel-side timeout (ms).
pub const HANDSHAKE_A_ACCEPT_TIMEOUT: u32 = 4;
/// `HANDSHAKE_A_ACCEPT_AUTH_MODE` — allowed auth modes.
pub const HANDSHAKE_A_ACCEPT_AUTH_MODE: u32 = 5;
/// `HANDSHAKE_A_DONE_STATUS` — final errno result.
pub const HANDSHAKE_A_DONE_STATUS: u32 = 1;
/// `HANDSHAKE_A_DONE_SOCKFD` — handshake-completed fd.
pub const HANDSHAKE_A_DONE_SOCKFD: u32 = 2;

// ---------------------------------------------------------------------------
// enum handshake_handler_class
// ---------------------------------------------------------------------------

/// Sentinel.
pub const HANDSHAKE_HANDLER_CLASS_NONE: u32 = 0;
/// TLS handshake (handler is `tlshd`).
pub const HANDSHAKE_HANDLER_CLASS_TLSHD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_metadata() {
        assert_eq!(HANDSHAKE_FAMILY_NAME, "handshake");
        assert_eq!(HANDSHAKE_FAMILY_VERSION, 1);
        // GENL_NAMSIZ (16) must accommodate the family name + NUL.
        assert!(HANDSHAKE_FAMILY_NAME.len() < 16);
        assert!(HANDSHAKE_MCGRP_TLSHD.len() < 16);
    }

    #[test]
    fn test_msg_type_dense() {
        let m = [
            HANDSHAKE_MSG_TYPE_UNSPEC,
            HANDSHAKE_MSG_TYPE_CLIENTHELLO,
            HANDSHAKE_MSG_TYPE_SERVERHELLO,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_auth_modes_dense() {
        let a = [
            HANDSHAKE_AUTH_UNSPEC,
            HANDSHAKE_AUTH_UNAUTH,
            HANDSHAKE_AUTH_PSK,
            HANDSHAKE_AUTH_X509,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_commands_dense() {
        let c = [
            HANDSHAKE_CMD_UNSPEC,
            HANDSHAKE_CMD_READY,
            HANDSHAKE_CMD_ACCEPT,
            HANDSHAKE_CMD_DONE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_accept_attributes_dense() {
        let a = [
            HANDSHAKE_A_ACCEPT_SOCKFD,
            HANDSHAKE_A_ACCEPT_HANDLER_CLASS,
            HANDSHAKE_A_ACCEPT_MESSAGE_TYPE,
            HANDSHAKE_A_ACCEPT_TIMEOUT,
            HANDSHAKE_A_ACCEPT_AUTH_MODE,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_handler_class_distinct() {
        assert_ne!(HANDSHAKE_HANDLER_CLASS_NONE, HANDSHAKE_HANDLER_CLASS_TLSHD);
        assert_eq!(HANDSHAKE_HANDLER_CLASS_NONE, 0);
    }
}
