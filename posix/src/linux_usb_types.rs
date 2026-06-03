//! `<linux/usb.h>` — Core USB framework constants.
//!
//! The USB subsystem manages device enumeration, configuration,
//! and driver binding. These constants cover URB (USB Request Block)
//! transfer types, pipe directions, and core framework state values
//! used throughout the USB stack.

// ---------------------------------------------------------------------------
// USB pipe direction
// ---------------------------------------------------------------------------

/// USB pipe direction: host to device (OUT).
pub const USB_DIR_OUT: u8 = 0x00;
/// USB pipe direction: device to host (IN).
pub const USB_DIR_IN: u8 = 0x80;

// ---------------------------------------------------------------------------
// USB transfer types
// ---------------------------------------------------------------------------

/// Control transfer (setup/status/data stages).
pub const USB_ENDPOINT_XFER_CONTROL: u8 = 0;
/// Isochronous transfer (periodic, no retry).
pub const USB_ENDPOINT_XFER_ISOC: u8 = 1;
/// Bulk transfer (large data, best-effort).
pub const USB_ENDPOINT_XFER_BULK: u8 = 2;
/// Interrupt transfer (small, periodic, guaranteed latency).
pub const USB_ENDPOINT_XFER_INT: u8 = 3;

// ---------------------------------------------------------------------------
// USB device states
// ---------------------------------------------------------------------------

/// Device not yet attached.
pub const USB_STATE_NOTATTACHED: u32 = 0;
/// Device attached, not yet powered.
pub const USB_STATE_ATTACHED: u32 = 1;
/// Device powered (Vbus applied).
pub const USB_STATE_POWERED: u32 = 2;
/// Device connected at full/high speed (not yet addressed).
pub const USB_STATE_RECONNECTING: u32 = 3;
/// Device assigned a unique address.
pub const USB_STATE_UNAUTHENTICATED: u32 = 4;
/// Device authenticated (wireless USB).
pub const USB_STATE_DEFAULT: u32 = 5;
/// Device addressed.
pub const USB_STATE_ADDRESS: u32 = 6;
/// Device configured (ready to use).
pub const USB_STATE_CONFIGURED: u32 = 7;
/// Device suspended (low power).
pub const USB_STATE_SUSPENDED: u32 = 8;

// ---------------------------------------------------------------------------
// URB transfer flags
// ---------------------------------------------------------------------------

/// Short packet is not an error.
pub const URB_SHORT_NOT_OK: u32 = 0x0001;
/// Use ISO frame-based scheduling.
pub const URB_ISO_ASAP: u32 = 0x0002;
/// Do not use DMA mapping.
pub const URB_NO_TRANSFER_DMA_MAP: u32 = 0x0004;
/// Zero-length packet termination for bulk OUT.
pub const URB_ZERO_PACKET: u32 = 0x0040;
/// Do not interrupt on completion.
pub const URB_NO_INTERRUPT: u32 = 0x0080;
/// Free transfer buffer on completion.
pub const URB_FREE_BUFFER: u32 = 0x0100;

// ---------------------------------------------------------------------------
// USB request type fields (bmRequestType)
// ---------------------------------------------------------------------------

/// Request type: standard.
pub const USB_TYPE_STANDARD: u8 = 0x00;
/// Request type: class-specific.
pub const USB_TYPE_CLASS: u8 = 0x20;
/// Request type: vendor-specific.
pub const USB_TYPE_VENDOR: u8 = 0x40;

/// Request recipient: device.
pub const USB_RECIP_DEVICE: u8 = 0x00;
/// Request recipient: interface.
pub const USB_RECIP_INTERFACE: u8 = 0x01;
/// Request recipient: endpoint.
pub const USB_RECIP_ENDPOINT: u8 = 0x02;
/// Request recipient: other.
pub const USB_RECIP_OTHER: u8 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_distinct() {
        assert_ne!(USB_DIR_OUT, USB_DIR_IN);
        assert_eq!(USB_DIR_OUT, 0);
    }

    #[test]
    fn test_transfer_types_distinct() {
        let types = [
            USB_ENDPOINT_XFER_CONTROL,
            USB_ENDPOINT_XFER_ISOC,
            USB_ENDPOINT_XFER_BULK,
            USB_ENDPOINT_XFER_INT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_device_states_distinct() {
        let states = [
            USB_STATE_NOTATTACHED,
            USB_STATE_ATTACHED,
            USB_STATE_POWERED,
            USB_STATE_RECONNECTING,
            USB_STATE_UNAUTHENTICATED,
            USB_STATE_DEFAULT,
            USB_STATE_ADDRESS,
            USB_STATE_CONFIGURED,
            USB_STATE_SUSPENDED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_urb_flags_distinct() {
        let flags = [
            URB_SHORT_NOT_OK,
            URB_ISO_ASAP,
            URB_NO_TRANSFER_DMA_MAP,
            URB_ZERO_PACKET,
            URB_NO_INTERRUPT,
            URB_FREE_BUFFER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_request_types_distinct() {
        assert_ne!(USB_TYPE_STANDARD, USB_TYPE_CLASS);
        assert_ne!(USB_TYPE_CLASS, USB_TYPE_VENDOR);
    }
}
