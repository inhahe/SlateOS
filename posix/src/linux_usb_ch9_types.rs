//! `<linux/usb/ch9.h>` — USB Chapter 9 standard request constants.
//!
//! Chapter 9 of the USB specification defines the standard device
//! requests that all USB devices must support. These include
//! GET_DESCRIPTOR, SET_ADDRESS, SET_CONFIGURATION, and other
//! requests used during enumeration and configuration.

// ---------------------------------------------------------------------------
// Standard request codes (bRequest)
// ---------------------------------------------------------------------------

/// Get device/interface/endpoint status.
pub const USB_REQ_GET_STATUS: u8 = 0x00;
/// Clear a feature (e.g., endpoint halt).
pub const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
/// Set a feature (e.g., remote wakeup).
pub const USB_REQ_SET_FEATURE: u8 = 0x03;
/// Set device address.
pub const USB_REQ_SET_ADDRESS: u8 = 0x05;
/// Get a descriptor (device, config, string, etc.).
pub const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
/// Set/replace a descriptor.
pub const USB_REQ_SET_DESCRIPTOR: u8 = 0x07;
/// Get the current configuration value.
pub const USB_REQ_GET_CONFIGURATION: u8 = 0x08;
/// Set the device configuration.
pub const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
/// Get the alternate setting for an interface.
pub const USB_REQ_GET_INTERFACE: u8 = 0x0A;
/// Set the alternate setting for an interface.
pub const USB_REQ_SET_INTERFACE: u8 = 0x0B;
/// Sync frame (isochronous endpoints).
pub const USB_REQ_SYNCH_FRAME: u8 = 0x0C;
/// Set SEL (SuperSpeed Link PM).
pub const USB_REQ_SET_SEL: u8 = 0x30;
/// Set isochronous delay.
pub const USB_REQ_SET_ISOCH_DELAY: u8 = 0x31;

// ---------------------------------------------------------------------------
// Descriptor types (wValue high byte)
// ---------------------------------------------------------------------------

/// Device descriptor.
pub const USB_DT_DEVICE: u8 = 0x01;
/// Configuration descriptor.
pub const USB_DT_CONFIG: u8 = 0x02;
/// String descriptor.
pub const USB_DT_STRING: u8 = 0x03;
/// Interface descriptor.
pub const USB_DT_INTERFACE: u8 = 0x04;
/// Endpoint descriptor.
pub const USB_DT_ENDPOINT: u8 = 0x05;
/// Device qualifier (high-speed capable device at full-speed).
pub const USB_DT_DEVICE_QUALIFIER: u8 = 0x06;
/// Other speed configuration.
pub const USB_DT_OTHER_SPEED_CONFIG: u8 = 0x07;
/// Interface power descriptor.
pub const USB_DT_INTERFACE_POWER: u8 = 0x08;
/// BOS (Binary Object Store) descriptor.
pub const USB_DT_BOS: u8 = 0x0F;
/// SuperSpeed endpoint companion.
pub const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;
/// SuperSpeedPlus isochronous endpoint companion.
pub const USB_DT_SSP_ISOC_ENDPOINT_COMP: u8 = 0x31;

// ---------------------------------------------------------------------------
// Feature selectors
// ---------------------------------------------------------------------------

/// Endpoint halt (stall).
pub const USB_ENDPOINT_HALT: u8 = 0;
/// Device remote wakeup.
pub const USB_DEVICE_REMOTE_WAKEUP: u8 = 1;
/// Device test mode.
pub const USB_DEVICE_TEST_MODE: u8 = 2;
/// U1 enable (SuperSpeed link PM).
pub const USB_DEVICE_U1_ENABLE: u8 = 48;
/// U2 enable (SuperSpeed link PM).
pub const USB_DEVICE_U2_ENABLE: u8 = 49;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_requests_distinct() {
        let reqs = [
            USB_REQ_GET_STATUS, USB_REQ_CLEAR_FEATURE,
            USB_REQ_SET_FEATURE, USB_REQ_SET_ADDRESS,
            USB_REQ_GET_DESCRIPTOR, USB_REQ_SET_DESCRIPTOR,
            USB_REQ_GET_CONFIGURATION, USB_REQ_SET_CONFIGURATION,
            USB_REQ_GET_INTERFACE, USB_REQ_SET_INTERFACE,
            USB_REQ_SYNCH_FRAME, USB_REQ_SET_SEL,
            USB_REQ_SET_ISOCH_DELAY,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_descriptor_types_distinct() {
        let dts = [
            USB_DT_DEVICE, USB_DT_CONFIG, USB_DT_STRING,
            USB_DT_INTERFACE, USB_DT_ENDPOINT, USB_DT_DEVICE_QUALIFIER,
            USB_DT_OTHER_SPEED_CONFIG, USB_DT_INTERFACE_POWER,
            USB_DT_BOS, USB_DT_SS_ENDPOINT_COMP,
            USB_DT_SSP_ISOC_ENDPOINT_COMP,
        ];
        for i in 0..dts.len() {
            for j in (i + 1)..dts.len() {
                assert_ne!(dts[i], dts[j]);
            }
        }
    }

    #[test]
    fn test_feature_selectors_distinct() {
        let feats = [
            USB_ENDPOINT_HALT, USB_DEVICE_REMOTE_WAKEUP,
            USB_DEVICE_TEST_MODE, USB_DEVICE_U1_ENABLE,
            USB_DEVICE_U2_ENABLE,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }
}
