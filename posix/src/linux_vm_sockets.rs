//! `<linux/vm_sockets.h>` — VM sockets (vsock) constants.
//!
//! VM sockets (AF_VSOCK) provide zero-configuration communication
//! between a hypervisor and its guest VMs. Used by QEMU/KVM virtio
//! transport and VMware VMCI transport.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// AF_VSOCK address family.
pub const AF_VSOCK: i32 = 40;

// ---------------------------------------------------------------------------
// Well-known CIDs (Context Identifiers)
// ---------------------------------------------------------------------------

/// Any CID (wildcard).
pub const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;
/// Hypervisor CID.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// Well-known CID reserved for local communication.
pub const VMADDR_CID_LOCAL: u32 = 1;
/// Host CID.
pub const VMADDR_CID_HOST: u32 = 2;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// Any port (wildcard).
pub const VMADDR_PORT_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Socket options (SOL_VSOCK level)
// ---------------------------------------------------------------------------

/// Buffer size.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u64 = 0;
/// Buffer min size.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u64 = 1;
/// Buffer max size.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u64 = 2;
/// Connect timeout.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT: u64 = 6;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// Trusted transport flag.
pub const VMADDR_FLAG_TO_HOST: u32 = 0x01;

// ---------------------------------------------------------------------------
// Vsock socket address structure
// ---------------------------------------------------------------------------

/// VM socket address.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrVm {
    /// Address family (AF_VSOCK).
    pub svm_family: u16,
    /// Reserved (must be zero).
    pub svm_reserved1: u16,
    /// Port number.
    pub svm_port: u32,
    /// Context ID (CID).
    pub svm_cid: u32,
    /// Flags.
    pub svm_flags: u8,
    /// Padding to 16 bytes.
    pub svm_zero: [u8; 3],
}

impl SockaddrVm {
    /// Create a zeroed vsock address.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

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
    fn test_any_port() {
        assert_eq!(VMADDR_PORT_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_sock_opts_distinct() {
        let opts = [
            SO_VM_SOCKETS_BUFFER_SIZE,
            SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE,
            SO_VM_SOCKETS_CONNECT_TIMEOUT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_sockaddr_vm_size() {
        assert_eq!(core::mem::size_of::<SockaddrVm>(), 16);
    }

    #[test]
    fn test_sockaddr_vm_zeroed() {
        let addr = SockaddrVm::zeroed();
        assert_eq!(addr.svm_family, 0);
        assert_eq!(addr.svm_port, 0);
        assert_eq!(addr.svm_cid, 0);
    }
}
