//! `<linux/virtio_vsock.h>` — Virtio vsock transport constants.
//!
//! Virtio vsock provides socket communication between a virtual
//! machine guest and its host without requiring network configuration.
//! Uses CID (Context ID) addressing instead of IP addresses. Supports
//! both stream (SOCK_STREAM) and datagram (SOCK_DGRAM) semantics.

// ---------------------------------------------------------------------------
// Well-known CIDs
// ---------------------------------------------------------------------------

/// Hypervisor CID (host side).
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local loopback CID (same VM).
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID (from guest's perspective).
pub const VMADDR_CID_HOST: u32 = 2;
/// Any CID (wildcard, for binding).
pub const VMADDR_CID_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Virtio vsock port numbers
// ---------------------------------------------------------------------------

/// Any port (wildcard, for binding).
pub const VMADDR_PORT_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Vsock packet operations
// ---------------------------------------------------------------------------

/// Invalid operation.
pub const VIRTIO_VSOCK_OP_INVALID: u32 = 0;
/// Connection request.
pub const VIRTIO_VSOCK_OP_REQUEST: u32 = 1;
/// Connection response.
pub const VIRTIO_VSOCK_OP_RESPONSE: u32 = 2;
/// Connection reset.
pub const VIRTIO_VSOCK_OP_RST: u32 = 3;
/// Connection shutdown.
pub const VIRTIO_VSOCK_OP_SHUTDOWN: u32 = 4;
/// Data payload (stream).
pub const VIRTIO_VSOCK_OP_RW: u32 = 5;
/// Credit update (flow control).
pub const VIRTIO_VSOCK_OP_CREDIT_UPDATE: u32 = 6;
/// Credit request.
pub const VIRTIO_VSOCK_OP_CREDIT_REQUEST: u32 = 7;

// ---------------------------------------------------------------------------
// Shutdown flags
// ---------------------------------------------------------------------------

/// Shut down receive side.
pub const VIRTIO_VSOCK_SHUTDOWN_RCV: u32 = 1;
/// Shut down send side.
pub const VIRTIO_VSOCK_SHUTDOWN_SEND: u32 = 2;

// ---------------------------------------------------------------------------
// Vsock socket types
// ---------------------------------------------------------------------------

/// Vsock stream (ordered, reliable byte stream).
pub const VIRTIO_VSOCK_TYPE_STREAM: u32 = 1;
/// Vsock seqpacket (ordered, reliable message boundaries).
pub const VIRTIO_VSOCK_TYPE_SEQPACKET: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_operations_distinct() {
        let ops = [
            VIRTIO_VSOCK_OP_INVALID, VIRTIO_VSOCK_OP_REQUEST,
            VIRTIO_VSOCK_OP_RESPONSE, VIRTIO_VSOCK_OP_RST,
            VIRTIO_VSOCK_OP_SHUTDOWN, VIRTIO_VSOCK_OP_RW,
            VIRTIO_VSOCK_OP_CREDIT_UPDATE, VIRTIO_VSOCK_OP_CREDIT_REQUEST,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_shutdown_flags_no_overlap() {
        assert_eq!(VIRTIO_VSOCK_SHUTDOWN_RCV & VIRTIO_VSOCK_SHUTDOWN_SEND, 0);
    }

    #[test]
    fn test_socket_types_distinct() {
        assert_ne!(VIRTIO_VSOCK_TYPE_STREAM, VIRTIO_VSOCK_TYPE_SEQPACKET);
    }

    #[test]
    fn test_any_values() {
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
        assert_eq!(VMADDR_PORT_ANY, u32::MAX);
    }
}
