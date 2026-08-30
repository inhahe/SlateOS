//! `<linux/auxiliary_bus.h>` — Auxiliary bus constants.
//!
//! The auxiliary bus allows a device driver to create child devices
//! that are serviced by different subsystems. For example, a NIC
//! driver can expose an auxiliary device for RDMA, another for
//! devlink, and another for IPsec offload — each handled by its
//! own driver without tight coupling.

// ---------------------------------------------------------------------------
// Auxiliary device naming
// ---------------------------------------------------------------------------

/// Maximum auxiliary device name length.
pub const AUXILIARY_NAME_SIZE: u32 = 32;
/// Name separator between parent and auxiliary (dot).
pub const AUXILIARY_NAME_SEP: char = '.';

// ---------------------------------------------------------------------------
// Auxiliary device IDs (well-known subsystem suffixes)
// ---------------------------------------------------------------------------

/// RDMA auxiliary device.
pub const AUXILIARY_ID_RDMA: &str = "rdma";
/// Devlink auxiliary device.
pub const AUXILIARY_ID_DEVLINK: &str = "devlink";
/// Ethernet auxiliary device.
pub const AUXILIARY_ID_ETH: &str = "eth";
/// Crypto offload auxiliary device.
pub const AUXILIARY_ID_CRYPTO: &str = "crypto";
/// Compression offload auxiliary device.
pub const AUXILIARY_ID_COMPRESS: &str = "compress";

// ---------------------------------------------------------------------------
// Auxiliary bus flags
// ---------------------------------------------------------------------------

/// Device is initialized.
pub const AUXILIARY_FLAG_INITIALIZED: u32 = 1 << 0;
/// Device probe deferred.
pub const AUXILIARY_FLAG_PROBE_DEFERRED: u32 = 1 << 1;
/// Device is in error state.
pub const AUXILIARY_FLAG_ERROR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_size() {
        assert_eq!(AUXILIARY_NAME_SIZE, 32);
    }

    #[test]
    fn test_separator() {
        assert_eq!(AUXILIARY_NAME_SEP, '.');
    }

    #[test]
    fn test_ids_distinct() {
        let ids = [
            AUXILIARY_ID_RDMA,
            AUXILIARY_ID_DEVLINK,
            AUXILIARY_ID_ETH,
            AUXILIARY_ID_CRYPTO,
            AUXILIARY_ID_COMPRESS,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            AUXILIARY_FLAG_INITIALIZED,
            AUXILIARY_FLAG_PROBE_DEFERRED,
            AUXILIARY_FLAG_ERROR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
