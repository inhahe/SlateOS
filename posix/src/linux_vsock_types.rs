//! `<linux/vm_sockets.h>` — VM sockets (AF_VSOCK) constants.
//!
//! VM sockets (virtio-vsock) enable communication between a hypervisor
//! host and its guest VMs without requiring network configuration.
//! They use a CID (Context ID) addressing scheme. Used by container
//! runtimes (Firecracker, Kata), guest agents, and cloud-init for
//! fast host-guest communication.

// ---------------------------------------------------------------------------
// Well-known CIDs
// ---------------------------------------------------------------------------

/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Local (loopback) CID.
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID.
pub const VMADDR_CID_HOST: u32 = 2;
/// Any CID (bind to all).
pub const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// Any port (kernel assigns).
pub const VMADDR_PORT_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// VSOCK socket options (SOL_VSOCK level)
// ---------------------------------------------------------------------------

/// VSOCK socket option level.
pub const SOL_VSOCK: u32 = 287;
/// Get/set buffer size.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
/// Get/set minimum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
/// Get/set maximum buffer size.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
/// Connection timeout (seconds).
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT: u32 = 6;

// ---------------------------------------------------------------------------
// VSOCK flags
// ---------------------------------------------------------------------------

/// Use SEQPACKET instead of STREAM.
pub const VSOCK_RECVERR: u32 = 1;

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
    fn test_any_is_max() {
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
        assert_eq!(VMADDR_PORT_ANY, u32::MAX);
    }

    #[test]
    fn test_options_distinct() {
        let opts = [
            SO_VM_SOCKETS_BUFFER_SIZE, SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE, SO_VM_SOCKETS_CONNECT_TIMEOUT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
