//! `<linux/vm_sockets.h>` — VM sockets (vsock) constants.
//!
//! VM sockets provide a communication channel between a hypervisor
//! and its guest virtual machines, or between VMs on the same host.
//! They use a CID (Context ID) + port addressing scheme instead
//! of IP addresses.

// ---------------------------------------------------------------------------
// Well-known CIDs
// ---------------------------------------------------------------------------

/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local (loopback) CID — connect within the same context.
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID (from guest perspective).
pub const VMADDR_CID_HOST: u32 = 2;
/// Any CID (wildcard for bind).
pub const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// Any port (wildcard for bind).
pub const VMADDR_PORT_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Socket options (SOL_VSOCK level)
// ---------------------------------------------------------------------------

/// Buffer size for the socket.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
/// Minimum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
/// Maximum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
/// Peer host shutdown notification.
pub const SO_VM_SOCKETS_PEER_HOST_VM_ID: u32 = 3;
/// Connect timeout in milliseconds.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT: u32 = 6;

// ---------------------------------------------------------------------------
// vsock transport flags
// ---------------------------------------------------------------------------

/// Stream transport (reliable, connection-oriented).
pub const VSOCK_TYPE_STREAM: u32 = 1;
/// Datagram transport (unreliable, connectionless).
pub const VSOCK_TYPE_DGRAM: u32 = 2;
/// Sequential packet transport (message boundaries preserved).
pub const VSOCK_TYPE_SEQPACKET: u32 = 5;

// ---------------------------------------------------------------------------
// vsock flags
// ---------------------------------------------------------------------------

/// Flag: connection is from trusted host.
pub const VSOCK_FLAG_TRUSTED: u32 = 1 << 0;
/// Flag: connection request.
pub const VSOCK_FLAG_REQUEST: u32 = 1 << 1;
/// Flag: connection shutdown.
pub const VSOCK_FLAG_SHUTDOWN: u32 = 1 << 2;
/// Flag: reset.
pub const VSOCK_FLAG_RST: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cids_distinct() {
        let cids = [
            VMADDR_CID_HYPERVISOR,
            VMADDR_CID_LOCAL,
            VMADDR_CID_HOST,
            VMADDR_CID_ANY,
        ];
        for i in 0..cids.len() {
            for j in (i + 1)..cids.len() {
                assert_ne!(cids[i], cids[j]);
            }
        }
    }

    #[test]
    fn test_cid_any() {
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
    }

    #[test]
    fn test_port_any() {
        assert_eq!(VMADDR_PORT_ANY, u32::MAX);
    }

    #[test]
    fn test_sockopt_distinct() {
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
    fn test_transport_types_distinct() {
        let types = [VSOCK_TYPE_STREAM, VSOCK_TYPE_DGRAM, VSOCK_TYPE_SEQPACKET];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            VSOCK_FLAG_TRUSTED,
            VSOCK_FLAG_REQUEST,
            VSOCK_FLAG_SHUTDOWN,
            VSOCK_FLAG_RST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_hypervisor_cid() {
        assert_eq!(VMADDR_CID_HYPERVISOR, 0);
    }
}
