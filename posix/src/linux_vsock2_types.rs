//! `<linux/vm_sockets.h>` — Additional vsock constants.
//!
//! Supplementary vsock constants covering socket options,
//! transport types, and CID values.

// ---------------------------------------------------------------------------
// vsock CID (Context ID) special values
// ---------------------------------------------------------------------------

/// Any CID (wildcard).
pub const VMADDR_CID_ANY: u32 = 0xFFFFFFFF;
/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local CID (loopback).
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID.
pub const VMADDR_CID_HOST: u32 = 2;

// ---------------------------------------------------------------------------
// vsock port special values
// ---------------------------------------------------------------------------

/// Any port (wildcard).
pub const VMADDR_PORT_ANY: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// vsock socket options
// ---------------------------------------------------------------------------

/// Buffer size.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
/// Minimum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
/// Maximum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
/// Peer host VM CID.
pub const SO_VM_SOCKETS_PEER_HOST_VM_ID: u32 = 3;
/// Trusted (peer is in the same security domain).
pub const SO_VM_SOCKETS_TRUSTED: u32 = 5;
/// Connect timeout.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD: u32 = 6;
/// Non-blocking connect timeout.
pub const SO_VM_SOCKETS_NONBLOCK_TXRX: u32 = 7;
/// New connect timeout.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW: u32 = 8;

// ---------------------------------------------------------------------------
// vsock transport flags
// ---------------------------------------------------------------------------

/// DGRAM transport available.
pub const VSOCK_TRANSPORT_F_DGRAM: u32 = 1 << 0;
/// STREAM transport available.
pub const VSOCK_TRANSPORT_F_STREAM: u32 = 1 << 1;
/// SEQPACKET transport available.
pub const VSOCK_TRANSPORT_F_SEQPACKET: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cids_distinct() {
        let cids = [
            VMADDR_CID_ANY,
            VMADDR_CID_HYPERVISOR,
            VMADDR_CID_LOCAL,
            VMADDR_CID_HOST,
        ];
        for i in 0..cids.len() {
            for j in (i + 1)..cids.len() {
                assert_ne!(cids[i], cids[j]);
            }
        }
    }

    #[test]
    fn test_any_is_max() {
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
        assert_eq!(VMADDR_PORT_ANY, u32::MAX);
    }

    #[test]
    fn test_socket_opts_distinct() {
        let opts = [
            SO_VM_SOCKETS_BUFFER_SIZE,
            SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE,
            SO_VM_SOCKETS_PEER_HOST_VM_ID,
            SO_VM_SOCKETS_TRUSTED,
            SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD,
            SO_VM_SOCKETS_NONBLOCK_TXRX,
            SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_transport_flags_power_of_two() {
        assert!(VSOCK_TRANSPORT_F_DGRAM.is_power_of_two());
        assert!(VSOCK_TRANSPORT_F_STREAM.is_power_of_two());
        assert!(VSOCK_TRANSPORT_F_SEQPACKET.is_power_of_two());
    }

    #[test]
    fn test_transport_flags_no_overlap() {
        let flags = [
            VSOCK_TRANSPORT_F_DGRAM,
            VSOCK_TRANSPORT_F_STREAM,
            VSOCK_TRANSPORT_F_SEQPACKET,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
