//! `<linux/tcp.h>` — TCP connection info state and field constants.
//!
//! The `TCP_INFO` socket option returns detailed statistics about a
//! TCP connection including state, congestion window, RTT, and
//! retransmission counts. These constants identify connection states
//! and info field indices.

// ---------------------------------------------------------------------------
// TCP connection states (from tcp_info.tcpi_state)
// ---------------------------------------------------------------------------

/// Connection established.
pub const TCP_ESTABLISHED: u8 = 1;
/// Sent SYN, awaiting SYN-ACK.
pub const TCP_SYN_SENT: u8 = 2;
/// Received SYN, sent SYN-ACK.
pub const TCP_SYN_RECV: u8 = 3;
/// Sent FIN, awaiting ACK.
pub const TCP_FIN_WAIT1: u8 = 4;
/// Received ACK of FIN.
pub const TCP_FIN_WAIT2: u8 = 5;
/// Awaiting remote FIN after close.
pub const TCP_TIME_WAIT: u8 = 6;
/// Socket is closed.
pub const TCP_CLOSE: u8 = 7;
/// Awaiting ACK of FIN.
pub const TCP_CLOSE_WAIT: u8 = 8;
/// Sent FIN after CLOSE_WAIT.
pub const TCP_LAST_ACK: u8 = 9;
/// Listening for connections.
pub const TCP_LISTEN: u8 = 10;
/// Both sides sent FIN simultaneously.
pub const TCP_CLOSING: u8 = 11;
/// New SYN received on TIME_WAIT socket.
pub const TCP_NEW_SYN_RECV: u8 = 12;

// ---------------------------------------------------------------------------
// TCP congestion algorithm notification events
// ---------------------------------------------------------------------------

/// Congestion window reduced.
pub const CA_EVENT_CWND_RESTART: u8 = 0;
/// Fast retransmit triggered.
pub const CA_EVENT_FAST_ACK: u8 = 1;
/// Loss detected.
pub const CA_EVENT_LOSS: u8 = 2;
/// ECN notification received.
pub const CA_EVENT_ECN_NO_CE: u8 = 3;
/// ECN CE mark received.
pub const CA_EVENT_ECN_IS_CE: u8 = 4;

// ---------------------------------------------------------------------------
// TCP info options (socket option numbers)
// ---------------------------------------------------------------------------

/// Get TCP connection info struct.
pub const TCP_INFO: u32 = 11;
/// Get congestion control algorithm name.
pub const TCP_CONGESTION: u32 = 13;
/// Get connection timeout value.
pub const TCP_USER_TIMEOUT: u32 = 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_states_distinct() {
        let states = [
            TCP_ESTABLISHED,
            TCP_SYN_SENT,
            TCP_SYN_RECV,
            TCP_FIN_WAIT1,
            TCP_FIN_WAIT2,
            TCP_TIME_WAIT,
            TCP_CLOSE,
            TCP_CLOSE_WAIT,
            TCP_LAST_ACK,
            TCP_LISTEN,
            TCP_CLOSING,
            TCP_NEW_SYN_RECV,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_established_is_one() {
        assert_eq!(TCP_ESTABLISHED, 1);
    }

    #[test]
    fn test_listen_is_ten() {
        assert_eq!(TCP_LISTEN, 10);
    }

    #[test]
    fn test_ca_events_distinct() {
        let events = [
            CA_EVENT_CWND_RESTART,
            CA_EVENT_FAST_ACK,
            CA_EVENT_LOSS,
            CA_EVENT_ECN_NO_CE,
            CA_EVENT_ECN_IS_CE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_tcp_info_options_distinct() {
        let opts = [TCP_INFO, TCP_CONGESTION, TCP_USER_TIMEOUT];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
