//! `<linux/vm_sockets_diag.h>` — vsock socket diagnostics constants.
//!
//! The vsock diagnostics netlink interface allows tools like `ss` to
//! inspect vsock socket state. vsock sockets provide communication
//! between virtual machines and their hypervisor hosts. The diag
//! interface reports socket state, connection details, and buffer
//! usage for both VMCI-backed and virtio-backed vsock transports.

// ---------------------------------------------------------------------------
// vsock diag attributes
// ---------------------------------------------------------------------------

/// vsock info attribute.
pub const VSOCK_DIAG_INFO: u32 = 1;

// ---------------------------------------------------------------------------
// vsock transport types
// ---------------------------------------------------------------------------

/// VMCI transport (VMware).
pub const VSOCK_TRANSPORT_VMCI: u32 = 0;
/// virtio transport (KVM/QEMU).
pub const VSOCK_TRANSPORT_VIRTIO: u32 = 1;
/// Loopback transport (same host).
pub const VSOCK_TRANSPORT_LOOPBACK: u32 = 2;

// ---------------------------------------------------------------------------
// vsock socket states (matching TCP-like states)
// ---------------------------------------------------------------------------

/// Socket is free/unused.
pub const VSOCK_SS_FREE: u32 = 0;
/// Socket is unconnected.
pub const VSOCK_SS_UNCONNECTED: u32 = 1;
/// Socket is connecting.
pub const VSOCK_SS_CONNECTING: u32 = 2;
/// Socket is connected.
pub const VSOCK_SS_CONNECTED: u32 = 3;
/// Socket is disconnecting.
pub const VSOCK_SS_DISCONNECTING: u32 = 4;
/// Socket is listening.
pub const VSOCK_SS_LISTEN: u32 = 5;

// ---------------------------------------------------------------------------
// Well-known CIDs
// ---------------------------------------------------------------------------

/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local (loopback) CID.
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID.
pub const VMADDR_CID_HOST: u32 = 2;
/// Any CID (wildcard).
pub const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_types_distinct() {
        let types = [
            VSOCK_TRANSPORT_VMCI,
            VSOCK_TRANSPORT_VIRTIO,
            VSOCK_TRANSPORT_LOOPBACK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            VSOCK_SS_FREE,
            VSOCK_SS_UNCONNECTED,
            VSOCK_SS_CONNECTING,
            VSOCK_SS_CONNECTED,
            VSOCK_SS_DISCONNECTING,
            VSOCK_SS_LISTEN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_well_known_cids_distinct() {
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
    fn test_cid_any_is_max() {
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
    }
}
