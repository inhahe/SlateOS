//! `<linux/vm_sockets.h>` — `AF_VSOCK` host↔guest sockets.
//!
//! VSOCK is the transport between hypervisors (KVM/virtio-vsock,
//! VMware, Hyper-V, Firecracker) and the guest. Container runtimes
//! (Kata, Firecracker microVMs) use it for the management channel.

// ---------------------------------------------------------------------------
// Address family and SOL level
// ---------------------------------------------------------------------------

pub const AF_VSOCK: u32 = 40;
pub const PF_VSOCK: u32 = AF_VSOCK;
pub const SOL_VSOCK: u32 = 287;

// ---------------------------------------------------------------------------
// Well-known CIDs (`sockaddr_vm.svm_cid`)
// ---------------------------------------------------------------------------

/// "Any CID" — used for binding wildcards.
pub const VMADDR_CID_ANY: u32 = u32::MAX;

/// CID 0 is reserved by the spec for the hypervisor itself.
pub const VMADDR_CID_HYPERVISOR: u32 = 0;

/// CID 1 is reserved (was VMADDR_CID_RESERVED in older kernels).
pub const VMADDR_CID_LOCAL: u32 = 1;

/// CID 2 is the host (kernel-side endpoint visible to the guest).
pub const VMADDR_CID_HOST: u32 = 2;

/// First CID a guest may use.
pub const VMADDR_CID_GUEST_MIN: u32 = 3;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

pub const VMADDR_PORT_ANY: u32 = u32::MAX;

/// Privileged port range (require `CAP_NET_BIND_SERVICE`).
pub const VMADDR_PRIV_PORT_MAX: u32 = 1023;

// ---------------------------------------------------------------------------
// Socket options (`setsockopt(SOL_VSOCK, …)`)
// ---------------------------------------------------------------------------

pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
pub const SO_VM_SOCKETS_PEER_HOST_VM_ID: u32 = 3;
pub const SO_VM_SOCKETS_TRUSTED: u32 = 5;
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD: u32 = 6;
pub const SO_VM_SOCKETS_NONBLOCK_TXRX: u32 = 7;
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW: u32 = 8;

// ---------------------------------------------------------------------------
// Buffer-size defaults (`virtio_vsock`)
// ---------------------------------------------------------------------------

pub const VM_SOCKETS_BUFFER_MIN: u32 = 128;
pub const VM_SOCKETS_BUFFER_DEFAULT: u32 = 262_144; // 256 KiB
pub const VM_SOCKETS_BUFFER_MAX: u32 = 262_144;

// ---------------------------------------------------------------------------
// `transport=` mount-like attribute exposed via getsockopt
// ---------------------------------------------------------------------------

pub const VSOCK_TRANSPORT_NAME_VIRTIO: &str = "virtio";
pub const VSOCK_TRANSPORT_NAME_VMCI: &str = "vmci";
pub const VSOCK_TRANSPORT_NAME_HYPERV: &str = "hyperv";
pub const VSOCK_TRANSPORT_NAME_LOOPBACK: &str = "loopback";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_and_sol() {
        // AF_VSOCK = 40, SOL_VSOCK = 287 (Linux net/vmw_vsock).
        assert_eq!(AF_VSOCK, 40);
        assert_eq!(PF_VSOCK, AF_VSOCK);
        assert_eq!(SOL_VSOCK, 287);
    }

    #[test]
    fn test_well_known_cids_assigned() {
        // CID 0/1/2 are reserved; guests start at 3.
        assert_eq!(VMADDR_CID_HYPERVISOR, 0);
        assert_eq!(VMADDR_CID_LOCAL, 1);
        assert_eq!(VMADDR_CID_HOST, 2);
        assert_eq!(VMADDR_CID_GUEST_MIN, 3);
        // "Any" is all-ones.
        assert_eq!(VMADDR_CID_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_privileged_port_boundary() {
        // 1023 matches the IPv4 privileged port range.
        assert_eq!(VMADDR_PRIV_PORT_MAX, 1023);
        assert_eq!(VMADDR_PORT_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_sockopts_distinct() {
        let o = [
            SO_VM_SOCKETS_BUFFER_SIZE,
            SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE,
            SO_VM_SOCKETS_PEER_HOST_VM_ID,
            SO_VM_SOCKETS_TRUSTED,
            SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD,
            SO_VM_SOCKETS_NONBLOCK_TXRX,
            SO_VM_SOCKETS_CONNECT_TIMEOUT_NEW,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_buffer_size_defaults_ordered() {
        assert!(VM_SOCKETS_BUFFER_MIN < VM_SOCKETS_BUFFER_DEFAULT);
        assert!(VM_SOCKETS_BUFFER_DEFAULT <= VM_SOCKETS_BUFFER_MAX);
        // 256 KiB by default.
        assert_eq!(VM_SOCKETS_BUFFER_DEFAULT, 256 * 1024);
        // Buffer max is a power of two (good for ring buffers).
        assert!(VM_SOCKETS_BUFFER_MAX.is_power_of_two());
    }

    #[test]
    fn test_transport_names_distinct() {
        let t = [
            VSOCK_TRANSPORT_NAME_VIRTIO,
            VSOCK_TRANSPORT_NAME_VMCI,
            VSOCK_TRANSPORT_NAME_HYPERV,
            VSOCK_TRANSPORT_NAME_LOOPBACK,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }
}
