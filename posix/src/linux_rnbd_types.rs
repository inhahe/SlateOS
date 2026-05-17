//! `<linux/rnbd.h>` — RDMA Network Block Device (RNBD) constants.
//!
//! RNBD provides high-performance remote block storage over RDMA
//! (InfiniBand, RoCE). Unlike NBD (which uses TCP sockets), RNBD
//! uses RDMA transport for zero-copy data transfer with kernel-bypass
//! I/O, achieving near-local-disk latency and throughput. The
//! architecture has a client (rnbd-client) that creates block devices
//! mapped to exports on a server (rnbd-server). Used in HPC clusters
//! and disaggregated storage architectures.

// ---------------------------------------------------------------------------
// RNBD message types (client ↔ server protocol)
// ---------------------------------------------------------------------------

/// Open (map) a remote device.
pub const RNBD_MSG_OPEN: u32 = 0;
/// Close (unmap) a remote device.
pub const RNBD_MSG_CLOSE: u32 = 1;
/// Read I/O request.
pub const RNBD_MSG_READ: u32 = 2;
/// Write I/O request.
pub const RNBD_MSG_WRITE: u32 = 3;
/// Identification/handshake.
pub const RNBD_MSG_IDENT: u32 = 4;

// ---------------------------------------------------------------------------
// RNBD access modes
// ---------------------------------------------------------------------------

/// Read-only access.
pub const RNBD_ACCESS_RO: u32 = 0;
/// Read-write access.
pub const RNBD_ACCESS_RW: u32 = 1;
/// Exclusive read-write (only one client).
pub const RNBD_ACCESS_RW_EXCLUSIVE: u32 = 2;

// ---------------------------------------------------------------------------
// RNBD device flags
// ---------------------------------------------------------------------------

/// Device supports discard (TRIM/unmap).
pub const RNBD_FLAG_DISCARD: u32 = 1 << 0;
/// Device supports secure erase.
pub const RNBD_FLAG_SECURE_ERASE: u32 = 1 << 1;
/// Device supports write zeroes.
pub const RNBD_FLAG_WRITE_ZEROES: u32 = 1 << 2;
/// Device is rotational (HDD).
pub const RNBD_FLAG_ROTATIONAL: u32 = 1 << 3;
/// Device supports FUA.
pub const RNBD_FLAG_FUA: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// RNBD session states
// ---------------------------------------------------------------------------

/// Session connected.
pub const RNBD_SESSION_CONNECTED: u32 = 0;
/// Session reconnecting (transport error, retrying).
pub const RNBD_SESSION_RECONNECTING: u32 = 1;
/// Session closed.
pub const RNBD_SESSION_CLOSED: u32 = 2;

// ---------------------------------------------------------------------------
// RNBD I/O flags
// ---------------------------------------------------------------------------

/// I/O is synchronous.
pub const RNBD_IO_FLAG_SYNC: u32 = 1 << 0;
/// I/O with FUA.
pub const RNBD_IO_FLAG_FUA: u32 = 1 << 1;
/// I/O is discard/trim.
pub const RNBD_IO_FLAG_DISCARD: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messages_distinct() {
        let msgs = [
            RNBD_MSG_OPEN, RNBD_MSG_CLOSE,
            RNBD_MSG_READ, RNBD_MSG_WRITE, RNBD_MSG_IDENT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_access_modes_distinct() {
        let modes = [RNBD_ACCESS_RO, RNBD_ACCESS_RW, RNBD_ACCESS_RW_EXCLUSIVE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_device_flags_no_overlap() {
        let flags = [
            RNBD_FLAG_DISCARD, RNBD_FLAG_SECURE_ERASE,
            RNBD_FLAG_WRITE_ZEROES, RNBD_FLAG_ROTATIONAL,
            RNBD_FLAG_FUA,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_session_states_distinct() {
        let states = [
            RNBD_SESSION_CONNECTED, RNBD_SESSION_RECONNECTING,
            RNBD_SESSION_CLOSED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_io_flags_no_overlap() {
        let flags = [
            RNBD_IO_FLAG_SYNC, RNBD_IO_FLAG_FUA, RNBD_IO_FLAG_DISCARD,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
