//! `<linux/usb/ch9.h>` (descriptor subset) — USB descriptor type constants.
//!
//! USB descriptors are structured data returned by devices during
//! enumeration. They describe the device's identity (vendor/product),
//! capabilities (speed, power), configuration (interfaces, endpoints),
//! and class-specific features. The host reads descriptors to select
//! the correct driver and configure the device properly.

// ---------------------------------------------------------------------------
// Descriptor types (bDescriptorType values)
// ---------------------------------------------------------------------------

/// Device descriptor.
pub const USB_DT_DEVICE: u32 = 0x01;
/// Configuration descriptor.
pub const USB_DT_CONFIG: u32 = 0x02;
/// String descriptor.
pub const USB_DT_STRING: u32 = 0x03;
/// Interface descriptor.
pub const USB_DT_INTERFACE: u32 = 0x04;
/// Endpoint descriptor.
pub const USB_DT_ENDPOINT: u32 = 0x05;
/// Device qualifier (for USB 2.0 dual-speed).
pub const USB_DT_DEVICE_QUALIFIER: u32 = 0x06;
/// Other speed configuration.
pub const USB_DT_OTHER_SPEED_CONFIG: u32 = 0x07;
/// Interface power descriptor.
pub const USB_DT_INTERFACE_POWER: u32 = 0x08;
/// OTG (On-The-Go) descriptor.
pub const USB_DT_OTG: u32 = 0x09;
/// Interface association descriptor.
pub const USB_DT_INTERFACE_ASSOCIATION: u32 = 0x0B;
/// BOS (Binary Object Store) descriptor.
pub const USB_DT_BOS: u32 = 0x0F;
/// Device capability descriptor.
pub const USB_DT_DEVICE_CAPABILITY: u32 = 0x10;
/// SuperSpeed endpoint companion.
pub const USB_DT_SS_ENDPOINT_COMP: u32 = 0x30;
/// SuperSpeedPlus isochronous endpoint companion.
pub const USB_DT_SSP_ISOC_ENDPOINT_COMP: u32 = 0x31;

// ---------------------------------------------------------------------------
// Descriptor sizes (standard, bytes)
// ---------------------------------------------------------------------------

/// Device descriptor size.
pub const USB_DT_DEVICE_SIZE: u32 = 18;
/// Configuration descriptor size (header only).
pub const USB_DT_CONFIG_SIZE: u32 = 9;
/// Interface descriptor size.
pub const USB_DT_INTERFACE_SIZE: u32 = 9;
/// Endpoint descriptor size.
pub const USB_DT_ENDPOINT_SIZE: u32 = 7;

// ---------------------------------------------------------------------------
// USB speeds
// ---------------------------------------------------------------------------

/// Low speed (1.5 Mbps, USB 1.0).
pub const USB_SPEED_LOW: u32 = 1;
/// Full speed (12 Mbps, USB 1.1).
pub const USB_SPEED_FULL: u32 = 2;
/// High speed (480 Mbps, USB 2.0).
pub const USB_SPEED_HIGH: u32 = 3;
/// SuperSpeed (5 Gbps, USB 3.0).
pub const USB_SPEED_SUPER: u32 = 5;
/// SuperSpeed+ (10/20 Gbps, USB 3.1/3.2).
pub const USB_SPEED_SUPER_PLUS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_types_distinct() {
        let types = [
            USB_DT_DEVICE, USB_DT_CONFIG, USB_DT_STRING,
            USB_DT_INTERFACE, USB_DT_ENDPOINT, USB_DT_DEVICE_QUALIFIER,
            USB_DT_OTHER_SPEED_CONFIG, USB_DT_INTERFACE_POWER,
            USB_DT_OTG, USB_DT_INTERFACE_ASSOCIATION,
            USB_DT_BOS, USB_DT_DEVICE_CAPABILITY,
            USB_DT_SS_ENDPOINT_COMP, USB_DT_SSP_ISOC_ENDPOINT_COMP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_speeds_ordered() {
        assert!(USB_SPEED_LOW < USB_SPEED_FULL);
        assert!(USB_SPEED_FULL < USB_SPEED_HIGH);
        assert!(USB_SPEED_HIGH < USB_SPEED_SUPER);
        assert!(USB_SPEED_SUPER < USB_SPEED_SUPER_PLUS);
    }

    #[test]
    fn test_descriptor_sizes() {
        assert!(USB_DT_DEVICE_SIZE > 0);
        assert!(USB_DT_CONFIG_SIZE > 0);
        assert!(USB_DT_ENDPOINT_SIZE > 0);
    }
}
