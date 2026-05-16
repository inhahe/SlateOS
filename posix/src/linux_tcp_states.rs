//! TCP connection state constants.
//!
//! TCP state machine states as used by the kernel networking
//! stack. These values appear in /proc/net/tcp, sock_diag
//! responses, and the tcp_info structure.

// ---------------------------------------------------------------------------
// TCP states (enum tcp_state in kernel)
// ---------------------------------------------------------------------------

/// Connection established (data transfer).
pub const TCP_ESTABLISHED: u8 = 1;
/// SYN sent (active open, waiting for SYN-ACK).
pub const TCP_SYN_SENT: u8 = 2;
/// SYN received (passive open, waiting for ACK).
pub const TCP_SYN_RECV: u8 = 3;
/// FIN-WAIT-1 (sent FIN, waiting for ACK or FIN).
pub const TCP_FIN_WAIT1: u8 = 4;
/// FIN-WAIT-2 (our FIN acknowledged, waiting for peer's FIN).
pub const TCP_FIN_WAIT2: u8 = 5;
/// TIME-WAIT (both FINs acknowledged, waiting for stale packets).
pub const TCP_TIME_WAIT: u8 = 6;
/// Closed (no connection).
pub const TCP_CLOSE: u8 = 7;
/// Close-wait (received FIN, waiting for local close).
pub const TCP_CLOSE_WAIT: u8 = 8;
/// Last-ACK (sent FIN after receiving FIN, waiting for final ACK).
pub const TCP_LAST_ACK: u8 = 9;
/// Listen (waiting for connections).
pub const TCP_LISTEN: u8 = 10;
/// Closing (both sides sent FIN simultaneously).
pub const TCP_CLOSING: u8 = 11;
/// New SYN received (used in SYN cookies / TFO).
pub const TCP_NEW_SYN_RECV: u8 = 12;

/// Maximum valid TCP state value.
pub const TCP_MAX_STATES: u8 = 13;

// ---------------------------------------------------------------------------
// TCP state names (for /proc display)
// ---------------------------------------------------------------------------

/// State name for ESTABLISHED.
pub const TCP_STATE_NAME_ESTABLISHED: &str = "ESTABLISHED";
/// State name for SYN_SENT.
pub const TCP_STATE_NAME_SYN_SENT: &str = "SYN-SENT";
/// State name for SYN_RECV.
pub const TCP_STATE_NAME_SYN_RECV: &str = "SYN-RECV";
/// State name for FIN_WAIT1.
pub const TCP_STATE_NAME_FIN_WAIT1: &str = "FIN-WAIT-1";
/// State name for FIN_WAIT2.
pub const TCP_STATE_NAME_FIN_WAIT2: &str = "FIN-WAIT-2";
/// State name for TIME_WAIT.
pub const TCP_STATE_NAME_TIME_WAIT: &str = "TIME-WAIT";
/// State name for CLOSE.
pub const TCP_STATE_NAME_CLOSE: &str = "CLOSE";
/// State name for CLOSE_WAIT.
pub const TCP_STATE_NAME_CLOSE_WAIT: &str = "CLOSE-WAIT";
/// State name for LAST_ACK.
pub const TCP_STATE_NAME_LAST_ACK: &str = "LAST-ACK";
/// State name for LISTEN.
pub const TCP_STATE_NAME_LISTEN: &str = "LISTEN";
/// State name for CLOSING.
pub const TCP_STATE_NAME_CLOSING: &str = "CLOSING";

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
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_states_in_range() {
        let states = [
            TCP_ESTABLISHED, TCP_SYN_SENT, TCP_SYN_RECV,
            TCP_FIN_WAIT1, TCP_FIN_WAIT2, TCP_TIME_WAIT,
            TCP_CLOSE, TCP_CLOSE_WAIT, TCP_LAST_ACK,
            TCP_LISTEN, TCP_CLOSING, TCP_NEW_SYN_RECV,
        ];
        for state in &states {
            assert!(*state > 0);
            assert!(*state < TCP_MAX_STATES);
        }
    }

    #[test]
    fn test_state_names_distinct() {
        let names = [
            TCP_STATE_NAME_ESTABLISHED, TCP_STATE_NAME_SYN_SENT,
            TCP_STATE_NAME_SYN_RECV, TCP_STATE_NAME_FIN_WAIT1,
            TCP_STATE_NAME_FIN_WAIT2, TCP_STATE_NAME_TIME_WAIT,
            TCP_STATE_NAME_CLOSE, TCP_STATE_NAME_CLOSE_WAIT,
            TCP_STATE_NAME_LAST_ACK, TCP_STATE_NAME_LISTEN,
            TCP_STATE_NAME_CLOSING,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_max_states() {
        assert_eq!(TCP_MAX_STATES, 13);
    }
}
