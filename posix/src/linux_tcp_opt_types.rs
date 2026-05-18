//! `<netinet/tcp.h>` — TCP socket option constants.
//!
//! TCP socket options control connection behavior: Nagle algorithm
//! (TCP_NODELAY), keepalive parameters, congestion control, and
//! various performance tuning knobs. These are set via setsockopt()
//! at the IPPROTO_TCP level.

// ---------------------------------------------------------------------------
// TCP socket options (level IPPROTO_TCP)
// ---------------------------------------------------------------------------

/// Disable Nagle algorithm (send immediately).
pub const TCP_NODELAY: u32 = 1;
/// Maximum segment size.
pub const TCP_MAXSEG: u32 = 2;
/// Cork (hold data until uncork or buffer full).
pub const TCP_CORK: u32 = 3;
/// Keepalive idle time (seconds before first probe).
pub const TCP_KEEPIDLE: u32 = 4;
/// Keepalive interval (seconds between probes).
pub const TCP_KEEPINTVL: u32 = 5;
/// Keepalive probe count (probes before dropping).
pub const TCP_KEEPCNT: u32 = 6;
/// Number of SYN retransmits.
pub const TCP_SYNCNT: u32 = 7;
/// Linger timeout for close (seconds).
pub const TCP_LINGER2: u32 = 8;
/// Defer accept until data arrives.
pub const TCP_DEFER_ACCEPT: u32 = 9;
/// Window clamp (maximum advertised window).
pub const TCP_WINDOW_CLAMP: u32 = 10;
/// Get TCP connection info.
pub const TCP_INFO: u32 = 11;
/// Quick ACK mode.
pub const TCP_QUICKACK: u32 = 12;
/// Set congestion control algorithm name.
pub const TCP_CONGESTION: u32 = 13;
/// Set MD5 signature key (per-address).
pub const TCP_MD5SIG: u32 = 14;
/// Thin linear timeouts (for thin streams).
pub const TCP_THIN_LINEAR_TIMEOUTS: u32 = 16;
/// Thin dupack (thin stream duplicate ACK threshold).
pub const TCP_THIN_DUPACK: u32 = 17;
/// User timeout (ms before connection abort).
pub const TCP_USER_TIMEOUT: u32 = 18;
/// TCP Fast Open (TFO) — queue length for listen socket.
pub const TCP_FASTOPEN: u32 = 23;
/// TCP Fast Open connect (send data in SYN).
pub const TCP_FASTOPEN_CONNECT: u32 = 30;
/// Enable/disable timestamps.
pub const TCP_TIMESTAMPS: u32 = 24;
/// Do not send RST on close with data in queue.
pub const TCP_NOTSENT_LOWAT: u32 = 25;
/// Set send zerocopy.
pub const TCP_ZEROCOPY_RECEIVE: u32 = 35;

// ---------------------------------------------------------------------------
// TCP states (from tcp_info)
// ---------------------------------------------------------------------------

/// Established state.
pub const TCP_ESTABLISHED: u32 = 1;
/// SYN sent (active open).
pub const TCP_SYN_SENT: u32 = 2;
/// SYN received (passive open).
pub const TCP_SYN_RECV: u32 = 3;
/// FIN wait 1 (close initiated).
pub const TCP_FIN_WAIT1: u32 = 4;
/// FIN wait 2 (FIN ACKed, waiting for peer FIN).
pub const TCP_FIN_WAIT2: u32 = 5;
/// Time wait (2MSL timeout).
pub const TCP_TIME_WAIT: u32 = 6;
/// Close (connection closed).
pub const TCP_CLOSE: u32 = 7;
/// Close wait (peer sent FIN).
pub const TCP_CLOSE_WAIT: u32 = 8;
/// Last ACK (waiting for FIN ACK).
pub const TCP_LAST_ACK: u32 = 9;
/// Listen (server socket).
pub const TCP_LISTEN: u32 = 10;
/// Closing (both sides sent FIN simultaneously).
pub const TCP_CLOSING: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_options_distinct() {
        let opts = [
            TCP_NODELAY, TCP_MAXSEG, TCP_CORK, TCP_KEEPIDLE,
            TCP_KEEPINTVL, TCP_KEEPCNT, TCP_SYNCNT, TCP_LINGER2,
            TCP_DEFER_ACCEPT, TCP_WINDOW_CLAMP, TCP_INFO,
            TCP_QUICKACK, TCP_CONGESTION, TCP_MD5SIG,
            TCP_THIN_LINEAR_TIMEOUTS, TCP_THIN_DUPACK,
            TCP_USER_TIMEOUT, TCP_FASTOPEN, TCP_FASTOPEN_CONNECT,
            TCP_TIMESTAMPS, TCP_NOTSENT_LOWAT, TCP_ZEROCOPY_RECEIVE,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
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
    fn test_nodelay_is_one() {
        assert_eq!(TCP_NODELAY, 1);
    }
}
