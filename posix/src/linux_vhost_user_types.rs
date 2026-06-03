//! `<linux/vhost.h>` — `/dev/vhost-net`, `/dev/vhost-vsock`, … ioctls.
//!
//! vhost moves virtio device emulation out of the qemu process and
//! into the host kernel. QEMU opens `/dev/vhost-net`, ties it to a
//! tap fd, and uses the ioctls below to set memory, configure rings,
//! and bind eventfds for kick/call notifications.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for vhost ioctls (0xAF, same as virtio).
pub const VHOST_VIRTIO: u8 = 0xAF;

// ---------------------------------------------------------------------------
// Core ioctls — owner, features, memory
// ---------------------------------------------------------------------------

/// `VHOST_GET_FEATURES` — read the supported feature bitmap (u64).
pub const VHOST_GET_FEATURES: u32 = 0x8008_AF00;
/// `VHOST_SET_FEATURES` — set the negotiated feature bitmap.
pub const VHOST_SET_FEATURES: u32 = 0x4008_AF00;
/// `VHOST_SET_OWNER` — take ownership of the vhost fd.
pub const VHOST_SET_OWNER: u32 = 0x0000_AF01;
/// `VHOST_RESET_OWNER` — release ownership.
pub const VHOST_RESET_OWNER: u32 = 0x0000_AF02;
/// `VHOST_SET_MEM_TABLE` — install the guest-memory map.
pub const VHOST_SET_MEM_TABLE: u32 = 0x4008_AF03;
/// `VHOST_SET_LOG_BASE` — install the dirty-log virtual address.
pub const VHOST_SET_LOG_BASE: u32 = 0x4008_AF04;
/// `VHOST_SET_LOG_FD` — install the eventfd for log notifications.
pub const VHOST_SET_LOG_FD: u32 = 0x4004_AF07;

// ---------------------------------------------------------------------------
// Virtqueue ioctls
// ---------------------------------------------------------------------------

/// `VHOST_SET_VRING_NUM` — set ring size.
pub const VHOST_SET_VRING_NUM: u32 = 0x4008_AF10;
/// `VHOST_SET_VRING_ADDR` — set descriptor / avail / used ring addrs.
pub const VHOST_SET_VRING_ADDR: u32 = 0x4028_AF11;
/// `VHOST_SET_VRING_BASE` — set last avail index.
pub const VHOST_SET_VRING_BASE: u32 = 0x4008_AF12;
/// `VHOST_GET_VRING_BASE` — read last avail index.
pub const VHOST_GET_VRING_BASE: u32 = 0xC008_AF12;
/// `VHOST_SET_VRING_KICK` — eventfd that the guest writes to kick host.
pub const VHOST_SET_VRING_KICK: u32 = 0x4008_AF20;
/// `VHOST_SET_VRING_CALL` — eventfd the host signals back to the guest.
pub const VHOST_SET_VRING_CALL: u32 = 0x4008_AF21;
/// `VHOST_SET_VRING_ERR` — eventfd for fatal errors.
pub const VHOST_SET_VRING_ERR: u32 = 0x4008_AF22;
/// `VHOST_SET_VRING_BUSYLOOP_TIMEOUT` — spin polling timeout (µs).
pub const VHOST_SET_VRING_BUSYLOOP_TIMEOUT: u32 = 0x4008_AF23;
/// `VHOST_GET_VRING_BUSYLOOP_TIMEOUT`.
pub const VHOST_GET_VRING_BUSYLOOP_TIMEOUT: u32 = 0xC008_AF24;

// ---------------------------------------------------------------------------
// vhost-net specific
// ---------------------------------------------------------------------------

/// `VHOST_NET_SET_BACKEND` — bind a tap/socket fd to a queue.
pub const VHOST_NET_SET_BACKEND: u32 = 0x4008_AF30;

// ---------------------------------------------------------------------------
// vhost-vsock specific
// ---------------------------------------------------------------------------

/// `VHOST_VSOCK_SET_GUEST_CID` — set the guest CID.
pub const VHOST_VSOCK_SET_GUEST_CID: u32 = 0x4008_AF60;
/// `VHOST_VSOCK_SET_RUNNING` — start/stop vsock.
pub const VHOST_VSOCK_SET_RUNNING: u32 = 0x4004_AF61;

// ---------------------------------------------------------------------------
// Feature flags (subset — VHOST_F_*)
// ---------------------------------------------------------------------------

/// vhost log-all is supported.
pub const VHOST_F_LOG_ALL: u32 = 26;
/// vhost-net mergeable receive buffers.
pub const VHOST_NET_F_VIRTIO_NET_HDR: u32 = 27;

// ---------------------------------------------------------------------------
// Device-node paths
// ---------------------------------------------------------------------------

/// `/dev/vhost-net`.
pub const VHOST_NET_PATH: &str = "/dev/vhost-net";
/// `/dev/vhost-vsock`.
pub const VHOST_VSOCK_PATH: &str = "/dev/vhost-vsock";
/// `/dev/vhost-scsi`.
pub const VHOST_SCSI_PATH: &str = "/dev/vhost-scsi";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_af() {
        assert_eq!(VHOST_VIRTIO, 0xAF);
    }

    #[test]
    fn test_core_ioctls_distinct_and_use_af() {
        let ops = [
            VHOST_GET_FEATURES,
            VHOST_SET_FEATURES,
            VHOST_SET_OWNER,
            VHOST_RESET_OWNER,
            VHOST_SET_MEM_TABLE,
            VHOST_SET_LOG_BASE,
            VHOST_SET_LOG_FD,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            assert_eq!((ops[i] >> 8) & 0xff, VHOST_VIRTIO as u32);
        }
    }

    #[test]
    fn test_vring_ioctls_distinct_and_use_af() {
        let ops = [
            VHOST_SET_VRING_NUM,
            VHOST_SET_VRING_ADDR,
            VHOST_SET_VRING_BASE,
            VHOST_GET_VRING_BASE,
            VHOST_SET_VRING_KICK,
            VHOST_SET_VRING_CALL,
            VHOST_SET_VRING_ERR,
            VHOST_SET_VRING_BUSYLOOP_TIMEOUT,
            VHOST_GET_VRING_BUSYLOOP_TIMEOUT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            assert_eq!((ops[i] >> 8) & 0xff, VHOST_VIRTIO as u32);
        }
    }

    #[test]
    fn test_transport_ioctls_distinct() {
        let ops = [
            VHOST_NET_SET_BACKEND,
            VHOST_VSOCK_SET_GUEST_CID,
            VHOST_VSOCK_SET_RUNNING,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            assert_eq!((ops[i] >> 8) & 0xff, VHOST_VIRTIO as u32);
        }
    }

    #[test]
    fn test_feature_bit_numbers() {
        // These are bit numbers (not masks) since the feature word is
        // u64. The two we expose must remain distinct.
        assert_ne!(VHOST_F_LOG_ALL, VHOST_NET_F_VIRTIO_NET_HDR);
        assert!(VHOST_F_LOG_ALL < 64);
        assert!(VHOST_NET_F_VIRTIO_NET_HDR < 64);
    }

    #[test]
    fn test_device_paths() {
        assert_eq!(VHOST_NET_PATH, "/dev/vhost-net");
        assert_eq!(VHOST_VSOCK_PATH, "/dev/vhost-vsock");
        assert_eq!(VHOST_SCSI_PATH, "/dev/vhost-scsi");
        // All start with /dev/vhost- prefix.
        for p in [VHOST_NET_PATH, VHOST_VSOCK_PATH, VHOST_SCSI_PATH] {
            assert!(p.starts_with("/dev/vhost-"));
        }
    }
}
