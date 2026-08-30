//! `<linux/virtio_vsock.h>` — Virtio vsock transport constants.
//!
//! Virtio-vsock provides the transport for AF_VSOCK sockets in
//! virtualized environments. It defines the packet format and
//! operations for communication between guest and host via
//! the virtio transport layer.

// ---------------------------------------------------------------------------
// Packet operations
// ---------------------------------------------------------------------------

/// Invalid operation.
pub const VIRTIO_VSOCK_OP_INVALID: u16 = 0;
/// Request connection.
pub const VIRTIO_VSOCK_OP_REQUEST: u16 = 1;
/// Response to connection request.
pub const VIRTIO_VSOCK_OP_RESPONSE: u16 = 2;
/// Connection reset.
pub const VIRTIO_VSOCK_OP_RST: u16 = 3;
/// Shutdown connection.
pub const VIRTIO_VSOCK_OP_SHUTDOWN: u16 = 4;
/// Data read/write.
pub const VIRTIO_VSOCK_OP_RW: u16 = 5;
/// Credit update.
pub const VIRTIO_VSOCK_OP_CREDIT_UPDATE: u16 = 6;
/// Credit request.
pub const VIRTIO_VSOCK_OP_CREDIT_REQUEST: u16 = 7;

// ---------------------------------------------------------------------------
// Shutdown flags
// ---------------------------------------------------------------------------

/// Shutdown receive side.
pub const VIRTIO_VSOCK_SHUTDOWN_RCV: u32 = 1;
/// Shutdown send side.
pub const VIRTIO_VSOCK_SHUTDOWN_SEND: u32 = 2;
/// Shutdown both sides.
pub const VIRTIO_VSOCK_SHUTDOWN_BOTH: u32 = 3;

// ---------------------------------------------------------------------------
// Packet types
// ---------------------------------------------------------------------------

/// Stream packet.
pub const VIRTIO_VSOCK_TYPE_STREAM: u16 = 1;
/// Seqpacket.
pub const VIRTIO_VSOCK_TYPE_SEQPACKET: u16 = 2;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Device supports SEQPACKET type.
pub const VIRTIO_VSOCK_F_SEQPACKET: u32 = 1;

// ---------------------------------------------------------------------------
// Flags in packet header
// ---------------------------------------------------------------------------

/// Packet carries data.
pub const VIRTIO_VSOCK_SEQ_EOM: u32 = 1;
/// End of record marker.
pub const VIRTIO_VSOCK_SEQ_EOR: u32 = 2;

// ---------------------------------------------------------------------------
// Virtqueue indices
// ---------------------------------------------------------------------------

/// RX virtqueue.
pub const VIRTIO_VSOCK_VQ_RX: u32 = 0;
/// TX virtqueue.
pub const VIRTIO_VSOCK_VQ_TX: u32 = 1;
/// Event virtqueue.
pub const VIRTIO_VSOCK_VQ_EVENT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            VIRTIO_VSOCK_OP_INVALID,
            VIRTIO_VSOCK_OP_REQUEST,
            VIRTIO_VSOCK_OP_RESPONSE,
            VIRTIO_VSOCK_OP_RST,
            VIRTIO_VSOCK_OP_SHUTDOWN,
            VIRTIO_VSOCK_OP_RW,
            VIRTIO_VSOCK_OP_CREDIT_UPDATE,
            VIRTIO_VSOCK_OP_CREDIT_REQUEST,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_shutdown_flags() {
        assert_eq!(
            VIRTIO_VSOCK_SHUTDOWN_BOTH,
            VIRTIO_VSOCK_SHUTDOWN_RCV | VIRTIO_VSOCK_SHUTDOWN_SEND
        );
    }

    #[test]
    fn test_types_distinct() {
        assert_ne!(VIRTIO_VSOCK_TYPE_STREAM, VIRTIO_VSOCK_TYPE_SEQPACKET);
    }

    #[test]
    fn test_seq_flags_distinct() {
        assert_ne!(VIRTIO_VSOCK_SEQ_EOM, VIRTIO_VSOCK_SEQ_EOR);
    }

    #[test]
    fn test_vq_indices_distinct() {
        let vqs = [
            VIRTIO_VSOCK_VQ_RX,
            VIRTIO_VSOCK_VQ_TX,
            VIRTIO_VSOCK_VQ_EVENT,
        ];
        for i in 0..vqs.len() {
            for j in (i + 1)..vqs.len() {
                assert_ne!(vqs[i], vqs[j]);
            }
        }
    }
}
