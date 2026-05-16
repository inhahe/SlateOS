//! `<linux/vm_sockets.h>` — Virtio/VM socket constants.
//!
//! AF_VSOCK provides communication between a guest VM and
//! its host without requiring network configuration. Used for
//! guest agents, file sharing (virtiofsd), and container runtimes.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// VM sockets address family.
pub const AF_VSOCK: u16 = 40;

// ---------------------------------------------------------------------------
// Well-known CIDs
// ---------------------------------------------------------------------------

/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local loopback (same host/guest).
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID.
pub const VMADDR_CID_HOST: u32 = 2;
/// Any CID (bind wildcard).
pub const VMADDR_CID_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// Any port (bind wildcard).
pub const VMADDR_PORT_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Socket options (SOL_VSOCK)
// ---------------------------------------------------------------------------

/// Buffer size.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
/// Minimum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
/// Maximum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
/// Peer host shutdown.
pub const SO_VM_SOCKETS_PEER_HOST_VM_ID: u32 = 3;
/// Connect timeout.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT: u32 = 6;

// ---------------------------------------------------------------------------
// Socket types
// ---------------------------------------------------------------------------

/// Stream socket (SOCK_STREAM equivalent).
pub const VSOCK_TYPE_STREAM: u32 = 1;
/// Datagram socket (SOCK_DGRAM equivalent).
pub const VSOCK_TYPE_DGRAM: u32 = 2;
/// Sequence packet socket (SOCK_SEQPACKET).
pub const VSOCK_TYPE_SEQPACKET: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_vsock() {
        assert_eq!(AF_VSOCK, 40);
    }

    #[test]
    fn test_cids_distinct() {
        let cids = [
            VMADDR_CID_HYPERVISOR, VMADDR_CID_LOCAL,
            VMADDR_CID_HOST, VMADDR_CID_ANY,
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
    fn test_sock_opts_distinct() {
        let opts = [
            SO_VM_SOCKETS_BUFFER_SIZE,
            SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE,
            SO_VM_SOCKETS_PEER_HOST_VM_ID,
            SO_VM_SOCKETS_CONNECT_TIMEOUT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        let types = [VSOCK_TYPE_STREAM, VSOCK_TYPE_DGRAM, VSOCK_TYPE_SEQPACKET];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
