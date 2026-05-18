//! `<sys/socket.h>` — Socket-level (SOL_SOCKET) option constants.
//!
//! Socket-level options apply regardless of protocol. They control
//! buffer sizes, timeouts, keepalive, broadcast, reuse, and various
//! ancillary data options. These are set via setsockopt() at the
//! SOL_SOCKET level.

// ---------------------------------------------------------------------------
// SOL_SOCKET options
// ---------------------------------------------------------------------------

/// Enable local address reuse.
pub const SO_REUSEADDR: u32 = 2;
/// Type of socket (SOCK_STREAM, SOCK_DGRAM, etc.).
pub const SO_TYPE: u32 = 3;
/// Get/clear pending error.
pub const SO_ERROR: u32 = 4;
/// Allow broadcast datagrams.
pub const SO_BROADCAST: u32 = 6;
/// Send buffer size.
pub const SO_SNDBUF: u32 = 7;
/// Receive buffer size.
pub const SO_RCVBUF: u32 = 8;
/// Keep connections alive.
pub const SO_KEEPALIVE: u32 = 9;
/// Don't route (send directly to interface).
pub const SO_DONTROUTE: u32 = 5;
/// Linger on close with data pending.
pub const SO_LINGER: u32 = 13;
/// Receive out-of-band data inline.
pub const SO_OOBINLINE: u32 = 10;
/// Enable port reuse (multiple listeners).
pub const SO_REUSEPORT: u32 = 15;
/// Send timeout.
pub const SO_SNDTIMEO: u32 = 21;
/// Receive timeout.
pub const SO_RCVTIMEO: u32 = 20;
/// Receive timestamp in ancillary data.
pub const SO_TIMESTAMP: u32 = 29;
/// Receive nanosecond timestamp.
pub const SO_TIMESTAMPNS: u32 = 35;
/// Bind to specific device.
pub const SO_BINDTODEVICE: u32 = 25;
/// Get socket credentials (ucred).
pub const SO_PEERCRED: u32 = 17;
/// Receive sender credentials via ancillary data.
pub const SO_PASSCRED: u32 = 16;
/// Pass security context.
pub const SO_PASSSEC: u32 = 34;
/// Set send buffer (force, override rmem_max).
pub const SO_SNDBUFFORCE: u32 = 32;
/// Set receive buffer (force, override rmem_max).
pub const SO_RCVBUFFORCE: u32 = 33;
/// Get receive low watermark.
pub const SO_RCVLOWAT: u32 = 18;
/// Get send low watermark.
pub const SO_SNDLOWAT: u32 = 19;
/// Mark socket for policy routing.
pub const SO_MARK: u32 = 36;
/// Attach BPF/cBPF filter.
pub const SO_ATTACH_FILTER: u32 = 26;
/// Detach BPF filter.
pub const SO_DETACH_FILTER: u32 = 27;
/// Set socket priority.
pub const SO_PRIORITY: u32 = 12;
/// Enable busy polling.
pub const SO_BUSY_POLL: u32 = 46;
/// Prefer busy polling budget.
pub const SO_PREFER_BUSY_POLL: u32 = 69;
/// Enable zerocopy sendmsg.
pub const SO_ZEROCOPY: u32 = 60;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_so_options_distinct() {
        let opts = [
            SO_REUSEADDR, SO_TYPE, SO_ERROR, SO_BROADCAST,
            SO_SNDBUF, SO_RCVBUF, SO_KEEPALIVE, SO_DONTROUTE,
            SO_LINGER, SO_OOBINLINE, SO_REUSEPORT, SO_SNDTIMEO,
            SO_RCVTIMEO, SO_TIMESTAMP, SO_TIMESTAMPNS,
            SO_BINDTODEVICE, SO_PEERCRED, SO_PASSCRED,
            SO_PASSSEC, SO_SNDBUFFORCE, SO_RCVBUFFORCE,
            SO_RCVLOWAT, SO_SNDLOWAT, SO_MARK,
            SO_ATTACH_FILTER, SO_DETACH_FILTER, SO_PRIORITY,
            SO_BUSY_POLL, SO_PREFER_BUSY_POLL, SO_ZEROCOPY,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_reuseaddr_value() {
        assert_eq!(SO_REUSEADDR, 2);
    }

    #[test]
    fn test_buf_options_related() {
        assert_ne!(SO_SNDBUF, SO_RCVBUF);
        assert_ne!(SO_SNDBUFFORCE, SO_RCVBUFFORCE);
    }
}
