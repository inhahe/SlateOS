//! `<linux/tcp_states.h>` — TCP connection state constants.
//!
//! TCP connections progress through a well-defined state machine:
//! CLOSED → SYN_SENT → ESTABLISHED → FIN_WAIT_1 → etc. These
//! states are visible via /proc/net/tcp, ss(8), and getsockopt().
//! The state determines what operations are valid on the socket
//! and how incoming packets are processed. Understanding states is
//! critical for debugging connection issues and implementing
//! health checks.

// ---------------------------------------------------------------------------
// TCP states (matching kernel enum tcp_state)
// ---------------------------------------------------------------------------

/// Socket is not connected (initial/final state).
pub const TCP_ESTABLISHED: u32 = 1;
/// SYN sent, waiting for SYN-ACK (active open).
pub const TCP_SYN_SENT: u32 = 2;
/// SYN received, waiting for ACK (passive open).
pub const TCP_SYN_RECV: u32 = 3;
/// FIN sent, waiting for ACK (active close step 1).
pub const TCP_FIN_WAIT1: u32 = 4;
/// Received ACK of FIN, waiting for peer's FIN.
pub const TCP_FIN_WAIT2: u32 = 5;
/// Waiting for enough time to pass (2*MSL) before CLOSED.
pub const TCP_TIME_WAIT: u32 = 6;
/// Socket is closed.
pub const TCP_CLOSE: u32 = 7;
/// Waiting for remote connection termination request.
pub const TCP_CLOSE_WAIT: u32 = 8;
/// Waiting for ACK of sent FIN (simultaneous close).
pub const TCP_LAST_ACK: u32 = 9;
/// Socket is listening for incoming connections.
pub const TCP_LISTEN: u32 = 10;
/// Both sides sent FIN simultaneously.
pub const TCP_CLOSING: u32 = 11;
/// New SYN received on existing connection (rare).
pub const TCP_NEW_SYN_RECV: u32 = 12;
/// Bound but not listening or connected.
pub const TCP_BOUND_INACTIVE: u32 = 13;
/// Maximum state value.
pub const TCP_MAX_STATES: u32 = 14;

// ---------------------------------------------------------------------------
// TCP connection flags (for internal state tracking)
// ---------------------------------------------------------------------------

/// Connection is using timestamps.
pub const TCP_FLAG_TIMESTAMPS: u32 = 0x01;
/// Connection is using window scaling.
pub const TCP_FLAG_WSCALE: u32 = 0x02;
/// Connection supports SACK.
pub const TCP_FLAG_SACK: u32 = 0x04;
/// Connection is using ECN.
pub const TCP_FLAG_ECN: u32 = 0x08;
/// Fast open cookie is valid.
pub const TCP_FLAG_FASTOPEN: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            TCP_ESTABLISHED, TCP_SYN_SENT, TCP_SYN_RECV,
            TCP_FIN_WAIT1, TCP_FIN_WAIT2, TCP_TIME_WAIT,
            TCP_CLOSE, TCP_CLOSE_WAIT, TCP_LAST_ACK,
            TCP_LISTEN, TCP_CLOSING, TCP_NEW_SYN_RECV,
            TCP_BOUND_INACTIVE,
        ];
        for i in 0..states.len() {
            assert!(states[i] < TCP_MAX_STATES);
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TCP_FLAG_TIMESTAMPS, TCP_FLAG_WSCALE, TCP_FLAG_SACK,
            TCP_FLAG_ECN, TCP_FLAG_FASTOPEN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
