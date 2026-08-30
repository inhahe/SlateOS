//! `<linux/vm_sockets.h>` — AF_VSOCK socket constants.
//!
//! AF_VSOCK is the guest↔host socket family backed by virtio-vsock
//! (and on macOS, Hyper-V VMBUS). systemd-vsockd, VS Code Remote-
//! Tunnels, and cloud-init dial known CIDs to talk to the hypervisor.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// `AF_VSOCK` — socket family value.
pub const AF_VSOCK: u32 = 40;
/// `PF_VSOCK` — protocol family (alias).
pub const PF_VSOCK: u32 = AF_VSOCK;

// ---------------------------------------------------------------------------
// Reserved CIDs
// ---------------------------------------------------------------------------

/// `VMADDR_CID_ANY` — wildcard for bind().
pub const VMADDR_CID_ANY: u32 = 0xffff_ffff;
/// `VMADDR_CID_HYPERVISOR` — reserved (hypervisor itself).
pub const VMADDR_CID_HYPERVISOR: u32 = 0;
/// `VMADDR_CID_LOCAL` — loopback within the same partition.
pub const VMADDR_CID_LOCAL: u32 = 1;
/// `VMADDR_CID_HOST` — the host kernel (Linux side of guest↔host vsock).
pub const VMADDR_CID_HOST: u32 = 2;

// ---------------------------------------------------------------------------
// Reserved ports
// ---------------------------------------------------------------------------

/// `VMADDR_PORT_ANY` — wildcard for bind().
pub const VMADDR_PORT_ANY: u32 = 0xffff_ffff;
/// Highest port reserved for the kernel — userspace must bind > this
/// when using SO_VM_SOCKETS_PEER_HOST_VM_ID.
pub const VSOCK_HOST_RESERVED_PORTS: u32 = 1023;

// ---------------------------------------------------------------------------
// SOL_VSOCK socket options (vm_sockets.h)
// ---------------------------------------------------------------------------

/// `SO_VM_SOCKETS_BUFFER_SIZE` — set/get buffer size hint.
pub const SO_VM_SOCKETS_BUFFER_SIZE: u32 = 0;
/// `SO_VM_SOCKETS_BUFFER_MIN_SIZE`.
pub const SO_VM_SOCKETS_BUFFER_MIN_SIZE: u32 = 1;
/// `SO_VM_SOCKETS_BUFFER_MAX_SIZE`.
pub const SO_VM_SOCKETS_BUFFER_MAX_SIZE: u32 = 2;
/// `SO_VM_SOCKETS_PEER_HOST_VM_ID` — query peer host VM ID (Hyper-V).
pub const SO_VM_SOCKETS_PEER_HOST_VM_ID: u32 = 3;
/// `SO_VM_SOCKETS_TRUSTED` — query whether peer is trusted.
pub const SO_VM_SOCKETS_TRUSTED: u32 = 5;
/// `SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD` — historic name for the new
/// 64-bit option.
pub const SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD: u32 = 6;
/// `SO_VM_SOCKETS_NONBLOCK_TXRX` — async tx/rx flag.
pub const SO_VM_SOCKETS_NONBLOCK_TXRX: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_values() {
        // Linux assigned AF_VSOCK=40 in v3.9 — never changes.
        assert_eq!(AF_VSOCK, 40);
        assert_eq!(PF_VSOCK, AF_VSOCK);
    }

    #[test]
    fn test_reserved_cids_distinct() {
        let c = [
            VMADDR_CID_ANY,
            VMADDR_CID_HYPERVISOR,
            VMADDR_CID_LOCAL,
            VMADDR_CID_HOST,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // ANY uses all-ones (u32 ::MAX), HYPERVISOR is 0.
        assert_eq!(VMADDR_CID_ANY, u32::MAX);
        assert_eq!(VMADDR_CID_HYPERVISOR, 0);
    }

    #[test]
    fn test_port_any_is_max() {
        assert_eq!(VMADDR_PORT_ANY, u32::MAX);
        // Privileged range matches the AF_INET convention.
        assert_eq!(VSOCK_HOST_RESERVED_PORTS, 1023);
    }

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            SO_VM_SOCKETS_BUFFER_SIZE,
            SO_VM_SOCKETS_BUFFER_MIN_SIZE,
            SO_VM_SOCKETS_BUFFER_MAX_SIZE,
            SO_VM_SOCKETS_PEER_HOST_VM_ID,
            SO_VM_SOCKETS_TRUSTED,
            SO_VM_SOCKETS_CONNECT_TIMEOUT_OLD,
            SO_VM_SOCKETS_NONBLOCK_TXRX,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
