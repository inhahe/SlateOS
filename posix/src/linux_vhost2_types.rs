//! `<linux/vhost.h>` — Vhost (in-kernel virtio) constants (extended).
//!
//! Vhost provides in-kernel virtio device backends.
//! These constants define vhost IOCTL numbers, feature
//! bits, backend types, and vring configuration.

// ---------------------------------------------------------------------------
// Vhost IOCTL commands
// ---------------------------------------------------------------------------

/// Get vhost features.
pub const VHOST_GET_FEATURES: u32 = 0x8008AF00;
/// Set vhost features.
pub const VHOST_SET_FEATURES: u32 = 0x4008AF00;
/// Set owner (bind to current process).
pub const VHOST_SET_OWNER: u32 = 0x0000AF01;
/// Reset owner.
pub const VHOST_RESET_OWNER: u32 = 0x0000AF02;
/// Set memory table.
pub const VHOST_SET_MEM_TABLE: u32 = 0x4008AF03;
/// Set log base.
pub const VHOST_SET_LOG_BASE: u32 = 0x4008AF04;
/// Set log fd.
pub const VHOST_SET_LOG_FD: u32 = 0x4004AF07;
/// Set vring num.
pub const VHOST_SET_VRING_NUM: u32 = 0x4008AF10;
/// Set vring addr.
pub const VHOST_SET_VRING_ADDR: u32 = 0x4028AF11;
/// Set vring base.
pub const VHOST_SET_VRING_BASE: u32 = 0x4008AF12;
/// Get vring base.
pub const VHOST_GET_VRING_BASE: u32 = 0xC008AF12;
/// Set vring kick fd.
pub const VHOST_SET_VRING_KICK: u32 = 0x4008AF20;
/// Set vring call fd.
pub const VHOST_SET_VRING_CALL: u32 = 0x4008AF21;
/// Set vring error fd.
pub const VHOST_SET_VRING_ERR: u32 = 0x4008AF22;
/// Set vring endianness.
pub const VHOST_SET_VRING_ENDIAN: u32 = 0x4008AF13;
/// Get vring endianness.
pub const VHOST_GET_VRING_ENDIAN: u32 = 0x4008AF14;
/// Set backend features.
pub const VHOST_SET_BACKEND_FEATURES: u32 = 0x4008AF25;
/// Get backend features.
pub const VHOST_GET_BACKEND_FEATURES: u32 = 0x8008AF26;

// ---------------------------------------------------------------------------
// Vhost-net specific
// ---------------------------------------------------------------------------

/// Set vhost-net backend.
pub const VHOST_NET_SET_BACKEND: u32 = 0x4008AF30;

// ---------------------------------------------------------------------------
// Vhost-SCSI specific
// ---------------------------------------------------------------------------

/// Set vhost-SCSI endpoint.
pub const VHOST_SCSI_SET_ENDPOINT: u32 = 0x4008AF40;
/// Clear vhost-SCSI endpoint.
pub const VHOST_SCSI_CLEAR_ENDPOINT: u32 = 0x4008AF41;
/// Get ABI version.
pub const VHOST_SCSI_GET_ABI_VERSION: u32 = 0x8004AF42;
/// Set events missed.
pub const VHOST_SCSI_SET_EVENTS_MISSED: u32 = 0x4004AF43;
/// Get events missed.
pub const VHOST_SCSI_GET_EVENTS_MISSED: u32 = 0x8004AF44;

// ---------------------------------------------------------------------------
// Vhost feature bits
// ---------------------------------------------------------------------------

/// Vhost-user protocol features.
pub const VHOST_F_LOG_ALL: u32 = 26;
/// User memory access.
pub const VHOST_USER_F_PROTOCOL_FEATURES: u32 = 30;

// ---------------------------------------------------------------------------
// Vhost vring state flags
// ---------------------------------------------------------------------------

/// No FD (file descriptor sentinel).
pub const VHOST_VRING_F_LOG: u32 = 0;
/// Vring index mask.
pub const VHOST_VRING_IDX_MASK: u32 = 0xFF;
/// Vring relative index flag.
pub const VHOST_VRING_LITTLE_ENDIAN: u32 = 0;
/// Vring big endian.
pub const VHOST_VRING_BIG_ENDIAN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            VHOST_GET_FEATURES,
            VHOST_SET_FEATURES,
            VHOST_SET_OWNER,
            VHOST_RESET_OWNER,
            VHOST_SET_MEM_TABLE,
            VHOST_SET_LOG_BASE,
            VHOST_SET_LOG_FD,
            VHOST_SET_VRING_NUM,
            VHOST_SET_VRING_ADDR,
            VHOST_SET_VRING_BASE,
            VHOST_GET_VRING_BASE,
            VHOST_SET_VRING_KICK,
            VHOST_SET_VRING_CALL,
            VHOST_SET_VRING_ERR,
            VHOST_SET_VRING_ENDIAN,
            VHOST_GET_VRING_ENDIAN,
            VHOST_SET_BACKEND_FEATURES,
            VHOST_GET_BACKEND_FEATURES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_scsi_cmds_distinct() {
        let cmds = [
            VHOST_SCSI_SET_ENDPOINT,
            VHOST_SCSI_CLEAR_ENDPOINT,
            VHOST_SCSI_GET_ABI_VERSION,
            VHOST_SCSI_SET_EVENTS_MISSED,
            VHOST_SCSI_GET_EVENTS_MISSED,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_feature_bits_distinct() {
        assert_ne!(VHOST_F_LOG_ALL, VHOST_USER_F_PROTOCOL_FEATURES);
    }

    #[test]
    fn test_endian_values_distinct() {
        assert_ne!(VHOST_VRING_LITTLE_ENDIAN, VHOST_VRING_BIG_ENDIAN);
    }

    #[test]
    fn test_net_backend() {
        assert_eq!(VHOST_NET_SET_BACKEND, 0x4008AF30);
    }

    #[test]
    fn test_idx_mask() {
        assert_eq!(VHOST_VRING_IDX_MASK, 0xFF);
    }
}
