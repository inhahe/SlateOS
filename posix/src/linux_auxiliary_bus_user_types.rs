//! `<linux/auxiliary_bus.h>` — auxiliary-bus device modaliases.
//!
//! The auxiliary bus carries virtual sub-devices of a parent driver
//! (e.g. an Ethernet NIC publishing its RDMA, PTP, or netdev port as
//! separate auxiliary devices). Userspace tooling (`udev`, `modprobe`)
//! matches them by modalias of the form
//! `auxiliary:<parent>.<func>`.

// ---------------------------------------------------------------------------
// Modalias bus tag and field separators
// ---------------------------------------------------------------------------

pub const AUXILIARY_MODALIAS_PREFIX: &str = "auxiliary:";

/// Length of the bus tag including the trailing colon.
pub const AUXILIARY_MODALIAS_PREFIX_LEN: usize = 10;

/// Separator between `<parent>` and the per-driver name in the modalias.
pub const AUXILIARY_MODALIAS_SEP: u8 = b'.';

// ---------------------------------------------------------------------------
// Identifier-length limits (kernel uapi)
// ---------------------------------------------------------------------------

/// Auxiliary device name (driver-chosen) up to this many bytes.
pub const AUXILIARY_NAME_SIZE: usize = 32;

/// Per-driver match-string buffer including the trailing NUL.
pub const AUXILIARY_MAX_NAME_LEN: usize = AUXILIARY_NAME_SIZE;

// ---------------------------------------------------------------------------
// sysfs layout
// ---------------------------------------------------------------------------

pub const SYS_BUS_AUXILIARY: &str = "/sys/bus/auxiliary";
pub const SYS_BUS_AUXILIARY_DEVICES: &str = "/sys/bus/auxiliary/devices";
pub const SYS_BUS_AUXILIARY_DRIVERS: &str = "/sys/bus/auxiliary/drivers";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modalias_prefix_consistent() {
        assert_eq!(AUXILIARY_MODALIAS_PREFIX, "auxiliary:");
        assert_eq!(
            AUXILIARY_MODALIAS_PREFIX.len(),
            AUXILIARY_MODALIAS_PREFIX_LEN
        );
        // Prefix ends with a colon, as required by the kernel's modalias
        // regex.
        assert!(AUXILIARY_MODALIAS_PREFIX.ends_with(':'));
    }

    #[test]
    fn test_separator_is_dot() {
        assert_eq!(AUXILIARY_MODALIAS_SEP, b'.');
        assert_eq!(AUXILIARY_MODALIAS_SEP, 0x2E);
    }

    #[test]
    fn test_name_size_is_32() {
        // Driver-name field is fixed at 32 bytes (including NUL).
        assert_eq!(AUXILIARY_NAME_SIZE, 32);
        assert_eq!(AUXILIARY_MAX_NAME_LEN, AUXILIARY_NAME_SIZE);
        // 32 is a tidy cache-line-friendly power of two.
        assert!(AUXILIARY_NAME_SIZE.is_power_of_two());
    }

    #[test]
    fn test_sysfs_paths() {
        assert!(SYS_BUS_AUXILIARY.starts_with("/sys/bus/"));
        assert!(
            SYS_BUS_AUXILIARY_DEVICES.starts_with(SYS_BUS_AUXILIARY)
        );
        assert!(
            SYS_BUS_AUXILIARY_DRIVERS.starts_with(SYS_BUS_AUXILIARY)
        );
        assert_eq!(
            SYS_BUS_AUXILIARY_DEVICES,
            "/sys/bus/auxiliary/devices"
        );
        assert_eq!(
            SYS_BUS_AUXILIARY_DRIVERS,
            "/sys/bus/auxiliary/drivers"
        );
    }

    #[test]
    fn test_modalias_round_trip_shape() {
        // A realistic modalias: parent driver "mlx5_core" exposing a
        // sub-device "rdma".
        let parent = "mlx5_core";
        let name = "rdma";
        let modalias = alloc_modalias(parent, name);
        assert!(modalias.starts_with(AUXILIARY_MODALIAS_PREFIX));
        // Find the separator after the prefix.
        let rest = &modalias[AUXILIARY_MODALIAS_PREFIX_LEN..];
        let dot = rest
            .as_bytes()
            .iter()
            .position(|&b| b == AUXILIARY_MODALIAS_SEP)
            .expect("dot separator");
        assert_eq!(&rest[..dot], parent);
        assert_eq!(&rest[dot + 1..], name);
    }

    // Helper that builds a modalias on the test heap; kept inside the
    // tests module so the production code remains `no_std`-friendly.
    fn alloc_modalias(parent: &str, name: &str) -> std::string::String {
        let mut s = std::string::String::from(AUXILIARY_MODALIAS_PREFIX);
        s.push_str(parent);
        s.push(char::from(AUXILIARY_MODALIAS_SEP));
        s.push_str(name);
        s
    }
}
