//! `<linux/mei.h>` — Intel AMT/MEI client-protocol identifiers.
//!
//! Intel Active Management Technology talks to userspace through the
//! Management Engine Interface character device. Userspace selects
//! a firmware-side client by connecting to a UUID, then exchanges
//! messages on that connection. This module gathers the well-known
//! identifiers AMT-aware tooling needs.

// ---------------------------------------------------------------------------
// MEI character-device path
// ---------------------------------------------------------------------------

pub const DEV_MEI: &str = "/dev/mei0";

// ---------------------------------------------------------------------------
// Standard MEI client GUIDs — 16 bytes, mixed-endian (Microsoft format)
// ---------------------------------------------------------------------------

/// AMT host-interface client: `12f80028-b4b7-4b2d-aca8-46e0ff65814c`.
pub const MEI_AMTHIF_GUID: [u8; 16] = [
    0x28, 0x00, 0xf8, 0x12, 0xb7, 0xb4, 0x2d, 0x4b, 0xac, 0xa8, 0x46, 0xe0, 0xff, 0x65, 0x81, 0x4c,
];

/// Manageability application: `309dcde8-ccb1-4062-8f78-600115a34327`.
pub const MEI_WATCHDOG_GUID: [u8; 16] = [
    0xe8, 0xcd, 0x9d, 0x30, 0xb1, 0xcc, 0x62, 0x40, 0x8f, 0x78, 0x60, 0x01, 0x15, 0xa3, 0x43, 0x27,
];

/// Wireless-manager client.
pub const MEI_WLAN_GUID: [u8; 16] = [
    0x46, 0xb2, 0xcd, 0x82, 0x77, 0x47, 0xbe, 0x4b, 0x80, 0xc3, 0xbf, 0xed, 0xbd, 0x39, 0xff, 0xa8,
];

// ---------------------------------------------------------------------------
// AMT default network ports (TCP)
// ---------------------------------------------------------------------------

pub const AMT_PORT_HTTP: u16 = 16_992;
pub const AMT_PORT_HTTPS: u16 = 16_993;
pub const AMT_PORT_REDIR: u16 = 16_994;
pub const AMT_PORT_REDIR_TLS: u16 = 16_995;
pub const AMT_PORT_SOL_LMS: u16 = 16_998;

// ---------------------------------------------------------------------------
// Watchdog defaults
// ---------------------------------------------------------------------------

pub const MEI_WATCHDOG_DEFAULT_TIMEOUT_S: u32 = 120;
pub const MEI_WATCHDOG_MIN_TIMEOUT_S: u32 = 120;

// ---------------------------------------------------------------------------
// Sysfs
// ---------------------------------------------------------------------------

pub const SYS_CLASS_MEI: &str = "/sys/class/mei";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_path() {
        assert_eq!(DEV_MEI, "/dev/mei0");
        assert!(DEV_MEI.starts_with("/dev/"));
    }

    #[test]
    fn test_known_guids_distinct_and_16_bytes() {
        let g = [MEI_AMTHIF_GUID, MEI_WATCHDOG_GUID, MEI_WLAN_GUID];
        for guid in g {
            assert_eq!(guid.len(), 16);
        }
        assert_ne!(MEI_AMTHIF_GUID, MEI_WATCHDOG_GUID);
        assert_ne!(MEI_WATCHDOG_GUID, MEI_WLAN_GUID);
        assert_ne!(MEI_AMTHIF_GUID, MEI_WLAN_GUID);
    }

    #[test]
    fn test_amthif_guid_microsoft_layout() {
        // First 32-bit block stored little-endian: 0x12f80028.
        let block = u32::from_le_bytes([
            MEI_AMTHIF_GUID[0],
            MEI_AMTHIF_GUID[1],
            MEI_AMTHIF_GUID[2],
            MEI_AMTHIF_GUID[3],
        ]);
        assert_eq!(block, 0x12f8_0028);
    }

    #[test]
    fn test_amt_ports_cluster_16990s() {
        let ports = [
            AMT_PORT_HTTP,
            AMT_PORT_HTTPS,
            AMT_PORT_REDIR,
            AMT_PORT_REDIR_TLS,
            AMT_PORT_SOL_LMS,
        ];
        for &p in &ports {
            assert!(p >= 16_990);
            assert!(p < 17_000);
        }
        // HTTPS is HTTP + 1 (matches the 80/443-style sibling layout AMT uses).
        assert_eq!(AMT_PORT_HTTPS, AMT_PORT_HTTP + 1);
        assert_eq!(AMT_PORT_REDIR_TLS, AMT_PORT_REDIR + 1);
    }

    #[test]
    fn test_watchdog_timeout_bounds() {
        // The MEI watchdog enforces a 120-second floor.
        assert_eq!(MEI_WATCHDOG_MIN_TIMEOUT_S, 120);
        assert!(MEI_WATCHDOG_DEFAULT_TIMEOUT_S >= MEI_WATCHDOG_MIN_TIMEOUT_S);
    }

    #[test]
    fn test_sysfs_path() {
        assert_eq!(SYS_CLASS_MEI, "/sys/class/mei");
        assert!(SYS_CLASS_MEI.starts_with("/sys/class/"));
    }
}
