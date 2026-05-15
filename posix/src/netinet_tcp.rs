//! `<netinet/tcp.h>` — TCP protocol definitions.
//!
//! Re-exports TCP socket option constants from `socket` and `netinet`.

// ---------------------------------------------------------------------------
// TCP socket options (from socket.rs)
// ---------------------------------------------------------------------------

pub use crate::socket::TCP_NODELAY;
pub use crate::socket::TCP_KEEPIDLE;
pub use crate::socket::TCP_KEEPINTVL;
pub use crate::socket::TCP_KEEPCNT;
pub use crate::socket::TCP_MAXSEG;
pub use crate::socket::TCP_CORK;
pub use crate::socket::TCP_USER_TIMEOUT;
pub use crate::socket::TCP_INFO;
pub use crate::socket::SOL_TCP;

// ---------------------------------------------------------------------------
// Additional TCP options (from netinet.rs)
// ---------------------------------------------------------------------------

pub use crate::netinet::TCP_FASTOPEN;
pub use crate::netinet::TCP_QUICKACK;
pub use crate::netinet::TCP_CONGESTION;
pub use crate::netinet::TCP_TIMESTAMP;

// ---------------------------------------------------------------------------
// TCP states (informational)
// ---------------------------------------------------------------------------

/// Connection established.
pub const TCP_ESTABLISHED: i32 = 1;

/// Sent SYN, waiting for SYN-ACK.
pub const TCP_SYN_SENT: i32 = 2;

/// Received SYN, sent SYN-ACK.
pub const TCP_SYN_RECV: i32 = 3;

/// Received FIN, sent ACK.
pub const TCP_FIN_WAIT1: i32 = 4;

/// Sent FIN, received ACK.
pub const TCP_FIN_WAIT2: i32 = 5;

/// Waiting for enough time to pass (2MSL).
pub const TCP_TIME_WAIT: i32 = 6;

/// Connection closed.
pub const TCP_CLOSE: i32 = 7;

/// Received FIN, waiting for close.
pub const TCP_CLOSE_WAIT: i32 = 8;

/// Sent FIN and ACK, waiting for FIN.
pub const TCP_LAST_ACK: i32 = 9;

/// Listening for connections.
pub const TCP_LISTEN: i32 = 10;

/// Both sides trying to close simultaneously.
pub const TCP_CLOSING: i32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_nodelay_value() {
        assert_eq!(TCP_NODELAY, 1);
    }

    #[test]
    fn test_tcp_options_distinct() {
        let opts = [
            TCP_NODELAY, TCP_MAXSEG, TCP_CORK,
            TCP_KEEPIDLE, TCP_KEEPINTVL, TCP_KEEPCNT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_tcp_states_distinct() {
        let states = [
            TCP_ESTABLISHED, TCP_SYN_SENT, TCP_SYN_RECV,
            TCP_FIN_WAIT1, TCP_FIN_WAIT2, TCP_TIME_WAIT,
            TCP_CLOSE, TCP_CLOSE_WAIT, TCP_LAST_ACK,
            TCP_LISTEN, TCP_CLOSING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tcp_states_sequential() {
        assert_eq!(TCP_ESTABLISHED, 1);
        assert_eq!(TCP_CLOSING, 11);
    }

    #[test]
    fn test_sol_tcp_value() {
        assert_eq!(SOL_TCP, 6);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(TCP_NODELAY, crate::socket::TCP_NODELAY);
        assert_eq!(TCP_CORK, crate::socket::TCP_CORK);
        assert_eq!(TCP_FASTOPEN, crate::netinet::TCP_FASTOPEN);
    }
}
