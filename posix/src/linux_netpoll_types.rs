//! `<linux/netpoll.h>` — Netpoll and netconsole constants.
//!
//! Netpoll provides low-level network I/O for critical subsystems
//! that need to send messages even when the normal network stack is
//! unavailable (panics, debugging). Netconsole uses netpoll to send
//! kernel log messages over UDP to a remote collector — essential for
//! debugging kernel crashes on headless systems. Netpoll operates at
//! the driver level, bypassing the full network stack, so it works
//! even when the kernel is in a broken state.

// ---------------------------------------------------------------------------
// Netpoll states
// ---------------------------------------------------------------------------

/// Netpoll target is not configured.
pub const NETPOLL_STATE_UNCONFIGURED: u32 = 0;
/// Netpoll target is configured and ready.
pub const NETPOLL_STATE_READY: u32 = 1;
/// Netpoll is actively transmitting.
pub const NETPOLL_STATE_BUSY: u32 = 2;
/// Netpoll target has error (link down, etc.).
pub const NETPOLL_STATE_ERROR: u32 = 3;

// ---------------------------------------------------------------------------
// Netconsole options
// ---------------------------------------------------------------------------

/// Extended netconsole (include metadata: timestamp, CPU, etc.).
pub const NETCON_OPT_EXTENDED: u32 = 0x01;
/// Prepend log level to messages.
pub const NETCON_OPT_LOGLEVEL: u32 = 0x02;
/// Release prepend (include release string).
pub const NETCON_OPT_RELEASE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Netconsole transport
// ---------------------------------------------------------------------------

/// Default netconsole UDP port (source).
pub const NETCONSOLE_PORT_SRC: u32 = 6665;
/// Default netconsole UDP port (destination).
pub const NETCONSOLE_PORT_DST: u32 = 6666;
/// Maximum netconsole message size.
pub const NETCONSOLE_MAX_MSG: u32 = 1000;

// ---------------------------------------------------------------------------
// Netpoll poll flags
// ---------------------------------------------------------------------------

/// Poll for RX (receive path).
pub const NETPOLL_POLL_RX: u32 = 0x01;
/// Poll for TX (transmit path).
pub const NETPOLL_POLL_TX: u32 = 0x02;
/// Poll completed (no more work).
pub const NETPOLL_POLL_DONE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            NETPOLL_STATE_UNCONFIGURED, NETPOLL_STATE_READY,
            NETPOLL_STATE_BUSY, NETPOLL_STATE_ERROR,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_netcon_opts_no_overlap() {
        let opts = [NETCON_OPT_EXTENDED, NETCON_OPT_LOGLEVEL, NETCON_OPT_RELEASE];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }

    #[test]
    fn test_ports() {
        assert_ne!(NETCONSOLE_PORT_SRC, NETCONSOLE_PORT_DST);
        assert!(NETCONSOLE_PORT_SRC > 0);
        assert!(NETCONSOLE_PORT_DST > 0);
    }

    #[test]
    fn test_poll_flags_no_overlap() {
        let flags = [NETPOLL_POLL_RX, NETPOLL_POLL_TX, NETPOLL_POLL_DONE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_msg_limit() {
        assert!(NETCONSOLE_MAX_MSG > 0);
    }
}
