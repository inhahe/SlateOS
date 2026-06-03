//! `<linux/tcp.h>` — TCP protocol header and extended options.
//!
//! Re-exports TCP socket options and state constants from `socket`,
//! `netinet`, and `netinet_tcp`, and adds the TCP header struct plus
//! additional Linux-specific TCP options not in the POSIX headers.

// ---------------------------------------------------------------------------
// Re-exports: socket options
// ---------------------------------------------------------------------------

pub use crate::socket::SOL_TCP;
pub use crate::socket::TCP_CORK;
pub use crate::socket::TCP_INFO;
pub use crate::socket::TCP_KEEPCNT;
pub use crate::socket::TCP_KEEPIDLE;
pub use crate::socket::TCP_KEEPINTVL;
pub use crate::socket::TCP_MAXSEG;
pub use crate::socket::TCP_NODELAY;
pub use crate::socket::TCP_USER_TIMEOUT;

pub use crate::netinet::TCP_CONGESTION;
pub use crate::netinet::TCP_FASTOPEN;
pub use crate::netinet::TCP_QUICKACK;
pub use crate::netinet::TCP_TIMESTAMP;

// ---------------------------------------------------------------------------
// Re-exports: TCP states
// ---------------------------------------------------------------------------

pub use crate::netinet_tcp::TCP_CLOSE;
pub use crate::netinet_tcp::TCP_CLOSE_WAIT;
pub use crate::netinet_tcp::TCP_CLOSING;
pub use crate::netinet_tcp::TCP_ESTABLISHED;
pub use crate::netinet_tcp::TCP_FIN_WAIT1;
pub use crate::netinet_tcp::TCP_FIN_WAIT2;
pub use crate::netinet_tcp::TCP_LAST_ACK;
pub use crate::netinet_tcp::TCP_LISTEN;
pub use crate::netinet_tcp::TCP_SYN_RECV;
pub use crate::netinet_tcp::TCP_SYN_SENT;
pub use crate::netinet_tcp::TCP_TIME_WAIT;

// ---------------------------------------------------------------------------
// Additional Linux-specific TCP options
// ---------------------------------------------------------------------------

/// Defer accept: wait for data before returning from accept().
pub const TCP_DEFER_ACCEPT: i32 = 9;
/// TCP window clamp.
pub const TCP_WINDOW_CLAMP: i32 = 10;
/// Linger2 timeout (FIN_WAIT2).
pub const TCP_LINGER2: i32 = 8;
/// Number of SYN retransmits.
pub const TCP_SYNCNT: i32 = 7;
/// Thin-stream linear timeouts.
pub const TCP_THIN_LINEAR_TIMEOUTS: i32 = 16;
/// Thin-stream fast retransmit.
pub const TCP_THIN_DUPACK: i32 = 17;
/// TCP repair mode.
pub const TCP_REPAIR: i32 = 19;
/// TCP repair queue selection.
pub const TCP_REPAIR_QUEUE: i32 = 20;
/// TCP queue sequence number (for repair).
pub const TCP_QUEUE_SEQ: i32 = 21;
/// TCP repair options.
pub const TCP_REPAIR_OPTIONS: i32 = 22;
/// Save TCP connection state.
pub const TCP_SAVED_SYN: i32 = 28;
/// TCP zero-copy receive.
pub const TCP_ZEROCOPY_RECEIVE: i32 = 35;
/// TCP no-delay ACK.
pub const TCP_NOTSENT_LOWAT: i32 = 25;

// ---------------------------------------------------------------------------
// TCP header flags
// ---------------------------------------------------------------------------

/// FIN flag.
pub const TH_FIN: u8 = 0x01;
/// SYN flag.
pub const TH_SYN: u8 = 0x02;
/// RST flag.
pub const TH_RST: u8 = 0x04;
/// PSH flag.
pub const TH_PUSH: u8 = 0x08;
/// ACK flag.
pub const TH_ACK: u8 = 0x10;
/// URG flag.
pub const TH_URG: u8 = 0x20;
/// ECE flag.
pub const TH_ECE: u8 = 0x40;
/// CWR flag.
pub const TH_CWR: u8 = 0x80;

// ---------------------------------------------------------------------------
// TCP header struct
// ---------------------------------------------------------------------------

/// TCP packet header (20 bytes without options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tcphdr {
    /// Source port.
    pub source: u16,
    /// Destination port.
    pub dest: u16,
    /// Sequence number.
    pub seq: u32,
    /// Acknowledgement number.
    pub ack_seq: u32,
    /// Data offset (high 4 bits) and flags (low 4+8 bits).
    ///
    /// Layout (network byte order):
    ///   bits [15:12] = data offset (header length in 32-bit words)
    ///   bits [11:6]  = reserved
    ///   bits [5:0]   = flags (URG,ACK,PSH,RST,SYN,FIN)
    pub doff_flags: u16,
    /// Window size.
    pub window: u16,
    /// Checksum.
    pub check: u16,
    /// Urgent pointer.
    pub urg_ptr: u16,
}

impl Tcphdr {
    /// Create a zeroed TCP header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// TCP repair queue identifiers
// ---------------------------------------------------------------------------

/// No queue.
pub const TCP_NO_QUEUE: i32 = 0;
/// Receive queue.
pub const TCP_RECV_QUEUE: i32 = 1;
/// Send queue.
pub const TCP_SEND_QUEUE: i32 = 2;
/// Queues count.
pub const TCP_QUEUES_NR: i32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcphdr_size() {
        assert_eq!(core::mem::size_of::<Tcphdr>(), 20);
    }

    #[test]
    fn test_tcphdr_zeroed() {
        let hdr = Tcphdr::zeroed();
        assert_eq!(hdr.source, 0);
        assert_eq!(hdr.dest, 0);
        assert_eq!(hdr.seq, 0);
        assert_eq!(hdr.ack_seq, 0);
        assert_eq!(hdr.window, 0);
    }

    #[test]
    fn test_tcp_flags_distinct() {
        let flags = [
            TH_FIN, TH_SYN, TH_RST, TH_PUSH, TH_ACK, TH_URG, TH_ECE, TH_CWR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tcp_flags_are_powers_of_two() {
        let flags = [
            TH_FIN, TH_SYN, TH_RST, TH_PUSH, TH_ACK, TH_URG, TH_ECE, TH_CWR,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} is not a power of two");
        }
    }

    #[test]
    fn test_linux_tcp_options_distinct() {
        let opts = [
            TCP_DEFER_ACCEPT,
            TCP_WINDOW_CLAMP,
            TCP_LINGER2,
            TCP_SYNCNT,
            TCP_THIN_LINEAR_TIMEOUTS,
            TCP_THIN_DUPACK,
            TCP_REPAIR,
            TCP_REPAIR_QUEUE,
            TCP_QUEUE_SEQ,
            TCP_REPAIR_OPTIONS,
            TCP_SAVED_SYN,
            TCP_ZEROCOPY_RECEIVE,
            TCP_NOTSENT_LOWAT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_tcp_repair_queues() {
        assert_eq!(TCP_NO_QUEUE, 0);
        assert_eq!(TCP_RECV_QUEUE, 1);
        assert_eq!(TCP_SEND_QUEUE, 2);
        assert_eq!(TCP_QUEUES_NR, 3);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(TCP_NODELAY, crate::socket::TCP_NODELAY);
        assert_eq!(TCP_CORK, crate::socket::TCP_CORK);
        assert_eq!(TCP_FASTOPEN, crate::netinet::TCP_FASTOPEN);
        assert_eq!(TCP_ESTABLISHED, crate::netinet_tcp::TCP_ESTABLISHED);
        assert_eq!(TCP_CLOSING, crate::netinet_tcp::TCP_CLOSING);
    }
}
