//! `<linux/vhost.h>` — vhost device interface.
//!
//! vhost is the kernel-side component of virtio that moves data-plane
//! processing into the kernel for better performance. Used by QEMU,
//! OVS, and other VM/container networking tools.

// ---------------------------------------------------------------------------
// vhost ioctl commands
// ---------------------------------------------------------------------------

/// Get vhost features.
pub const VHOST_GET_FEATURES: u64 = 0x8008AF00;
/// Set vhost features.
pub const VHOST_SET_FEATURES: u64 = 0x4008AF00;
/// Set the owner of the vhost fd.
pub const VHOST_SET_OWNER: u64 = 0xAF01;
/// Reset the owner.
pub const VHOST_RESET_OWNER: u64 = 0xAF02;
/// Set memory table.
pub const VHOST_SET_MEM_TABLE: u64 = 0x4008AF03;
/// Set log base.
pub const VHOST_SET_LOG_BASE: u64 = 0x4008AF04;
/// Set log fd.
pub const VHOST_SET_LOG_FD: u64 = 0x4004AF07;

// ---------------------------------------------------------------------------
// vhost vring ioctls
// ---------------------------------------------------------------------------

/// Set vring number of descriptors.
pub const VHOST_SET_VRING_NUM: u64 = 0x4008AF10;
/// Set vring addresses.
pub const VHOST_SET_VRING_ADDR: u64 = 0x4028AF11;
/// Set vring base index.
pub const VHOST_SET_VRING_BASE: u64 = 0x4008AF12;
/// Get vring base index.
pub const VHOST_GET_VRING_BASE: u64 = 0xC008AF12;
/// Set vring kick fd.
pub const VHOST_SET_VRING_KICK: u64 = 0x4008AF20;
/// Set vring call fd.
pub const VHOST_SET_VRING_CALL: u64 = 0x4008AF21;
/// Set vring error fd.
pub const VHOST_SET_VRING_ERR: u64 = 0x4008AF22;
/// Set vring endianness.
pub const VHOST_SET_VRING_ENDIAN: u64 = 0x4008AF13;
/// Get vring endianness.
pub const VHOST_GET_VRING_ENDIAN: u64 = 0x4008AF14;

// ---------------------------------------------------------------------------
// vhost-net ioctls
// ---------------------------------------------------------------------------

/// Set backend fd (vhost-net).
pub const VHOST_NET_SET_BACKEND: u64 = 0x4008AF30;

// ---------------------------------------------------------------------------
// vhost-vsock ioctls
// ---------------------------------------------------------------------------

/// Set guest CID (vhost-vsock).
pub const VHOST_VSOCK_SET_GUEST_CID: u64 = 0x4008AF60;
/// Set running state (vhost-vsock).
pub const VHOST_VSOCK_SET_RUNNING: u64 = 0x4004AF61;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_ioctls_distinct() {
        let cmds = [
            VHOST_GET_FEATURES, VHOST_SET_FEATURES,
            VHOST_SET_OWNER, VHOST_RESET_OWNER,
            VHOST_SET_MEM_TABLE, VHOST_SET_LOG_BASE,
            VHOST_SET_LOG_FD,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_vring_ioctls_distinct() {
        let cmds = [
            VHOST_SET_VRING_NUM, VHOST_SET_VRING_ADDR,
            VHOST_SET_VRING_BASE, VHOST_SET_VRING_KICK,
            VHOST_SET_VRING_CALL, VHOST_SET_VRING_ERR,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_net_ioctl() {
        assert_ne!(VHOST_NET_SET_BACKEND, VHOST_SET_VRING_KICK);
    }

    #[test]
    fn test_vsock_ioctls() {
        assert_ne!(VHOST_VSOCK_SET_GUEST_CID, VHOST_VSOCK_SET_RUNNING);
    }
}
