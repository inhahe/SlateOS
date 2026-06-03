//! `acpid` event protocol — netlink and `/proc/acpi/event`.
//!
//! Userspace daemons (`acpid`, `upowerd`, `systemd-logind`) read ACPI
//! events as space-separated `type bus id data` lines. Modern kernels
//! also expose them through the `acpi_event` generic netlink family.

// ---------------------------------------------------------------------------
// Legacy /proc text-protocol path
// ---------------------------------------------------------------------------

pub const PROC_ACPI_EVENT: &str = "/proc/acpi/event";

// ---------------------------------------------------------------------------
// Generic-netlink family name and groups
// ---------------------------------------------------------------------------

pub const ACPI_GENL_FAMILY_NAME: &str = "acpi_event";
pub const ACPI_GENL_MCAST_GROUP: &str = "acpi_mc_group";
pub const ACPI_GENL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Generic-netlink command IDs (`enum`)
// ---------------------------------------------------------------------------

pub const ACPI_GENL_CMD_UNSPEC: u8 = 0;
pub const ACPI_GENL_CMD_EVENT: u8 = 1;
pub const ACPI_GENL_CMD_MAX: u8 = 2;

// ---------------------------------------------------------------------------
// Generic-netlink attributes for an event message
// ---------------------------------------------------------------------------

pub const ACPI_GENL_ATTR_UNSPEC: u16 = 0;
pub const ACPI_GENL_ATTR_EVENT: u16 = 1;
pub const ACPI_GENL_ATTR_MAX: u16 = 2;

// ---------------------------------------------------------------------------
// Maximum sizes
// ---------------------------------------------------------------------------

pub const ACPI_GENL_BUS_ID_LEN: usize = 15;
pub const ACPI_GENL_DEVICE_CLASS_LEN: usize = 20;

// ---------------------------------------------------------------------------
// Well-known event-type strings as emitted by the kernel
// ---------------------------------------------------------------------------

pub const ACPI_EVENT_BUTTON_POWER: &str = "button/power";
pub const ACPI_EVENT_BUTTON_SLEEP: &str = "button/sleep";
pub const ACPI_EVENT_BUTTON_LID: &str = "button/lid";
pub const ACPI_EVENT_AC_ADAPTER: &str = "ac_adapter";
pub const ACPI_EVENT_BATTERY: &str = "battery";
pub const ACPI_EVENT_THERMAL_ZONE: &str = "thermal_zone";
pub const ACPI_EVENT_PROCESSOR: &str = "processor";
pub const ACPI_EVENT_FAN: &str = "fan";
pub const ACPI_EVENT_VIDEO: &str = "video";

// ---------------------------------------------------------------------------
// Standard event codes (numeric `event` field) per the ACPI spec §5.6
// ---------------------------------------------------------------------------

pub const ACPI_NOTIFY_BUS_CHECK: u32 = 0x00;
pub const ACPI_NOTIFY_DEVICE_CHECK: u32 = 0x01;
pub const ACPI_NOTIFY_DEVICE_WAKE: u32 = 0x02;
pub const ACPI_NOTIFY_EJECT_REQUEST: u32 = 0x03;
pub const ACPI_NOTIFY_DEVICE_CHECK_LIGHT: u32 = 0x04;
pub const ACPI_NOTIFY_FREQUENCY_MISMATCH: u32 = 0x05;
pub const ACPI_NOTIFY_BUS_MODE_MISMATCH: u32 = 0x06;
pub const ACPI_NOTIFY_POWER_FAULT: u32 = 0x07;
pub const ACPI_NOTIFY_CAPABILITIES_CHECK: u32 = 0x08;
pub const ACPI_NOTIFY_DEVICE_PLD_CHECK: u32 = 0x09;
pub const ACPI_NOTIFY_RESERVED: u32 = 0x0A;
pub const ACPI_NOTIFY_LOCALITY_UPDATE: u32 = 0x0B;
pub const ACPI_NOTIFY_SHUTDOWN_REQUEST: u32 = 0x0C;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_family_strings() {
        assert_eq!(ACPI_GENL_FAMILY_NAME, "acpi_event");
        assert_eq!(ACPI_GENL_MCAST_GROUP, "acpi_mc_group");
        // Both fit comfortably in the 16-byte genl name field.
        assert!(ACPI_GENL_FAMILY_NAME.len() < 16);
        assert!(ACPI_GENL_MCAST_GROUP.len() < 16);
    }

    #[test]
    fn test_cmd_and_attr_dense() {
        let c = [ACPI_GENL_CMD_UNSPEC, ACPI_GENL_CMD_EVENT];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(ACPI_GENL_CMD_MAX as usize, c.len());

        let a = [ACPI_GENL_ATTR_UNSPEC, ACPI_GENL_ATTR_EVENT];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(ACPI_GENL_ATTR_MAX as usize, a.len());
    }

    #[test]
    fn test_bus_and_class_lengths() {
        // bus_id 15 + NUL = 16; device_class 20 + NUL = 21.
        assert_eq!(ACPI_GENL_BUS_ID_LEN, 15);
        assert_eq!(ACPI_GENL_DEVICE_CLASS_LEN, 20);
    }

    #[test]
    fn test_event_strings_have_no_whitespace() {
        let e = [
            ACPI_EVENT_BUTTON_POWER,
            ACPI_EVENT_BUTTON_SLEEP,
            ACPI_EVENT_BUTTON_LID,
            ACPI_EVENT_AC_ADAPTER,
            ACPI_EVENT_BATTERY,
            ACPI_EVENT_THERMAL_ZONE,
            ACPI_EVENT_PROCESSOR,
            ACPI_EVENT_FAN,
            ACPI_EVENT_VIDEO,
        ];
        for s in e {
            assert!(!s.is_empty());
            assert!(!s.contains(' '));
            // The event protocol uses '/' as a subtype separator only
            // for button events.
            if s.starts_with("button/") {
                assert!(s.contains('/'));
            }
        }
    }

    #[test]
    fn test_notify_codes_dense_0_to_c() {
        let n = [
            ACPI_NOTIFY_BUS_CHECK,
            ACPI_NOTIFY_DEVICE_CHECK,
            ACPI_NOTIFY_DEVICE_WAKE,
            ACPI_NOTIFY_EJECT_REQUEST,
            ACPI_NOTIFY_DEVICE_CHECK_LIGHT,
            ACPI_NOTIFY_FREQUENCY_MISMATCH,
            ACPI_NOTIFY_BUS_MODE_MISMATCH,
            ACPI_NOTIFY_POWER_FAULT,
            ACPI_NOTIFY_CAPABILITIES_CHECK,
            ACPI_NOTIFY_DEVICE_PLD_CHECK,
            ACPI_NOTIFY_RESERVED,
            ACPI_NOTIFY_LOCALITY_UPDATE,
            ACPI_NOTIFY_SHUTDOWN_REQUEST,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
