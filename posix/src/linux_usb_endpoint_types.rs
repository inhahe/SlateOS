//! `<linux/usb/ch9.h>` (endpoint subset) — USB endpoint constants.
//!
//! USB endpoints are the addressable units within a USB device that
//! send or receive data. Each endpoint has a direction (IN/OUT),
//! transfer type (control, bulk, interrupt, isochronous), and a
//! maximum packet size. Endpoint 0 is the default control endpoint
//! used for device enumeration and configuration.

// ---------------------------------------------------------------------------
// Endpoint direction
// ---------------------------------------------------------------------------

/// OUT endpoint (host → device).
pub const USB_DIR_OUT: u32 = 0x00;
/// IN endpoint (device → host).
pub const USB_DIR_IN: u32 = 0x80;
/// Direction mask (bit 7 of endpoint address).
pub const USB_ENDPOINT_DIR_MASK: u32 = 0x80;
/// Endpoint number mask (bits 0-3).
pub const USB_ENDPOINT_NUMBER_MASK: u32 = 0x0F;

// ---------------------------------------------------------------------------
// Endpoint transfer types (bmAttributes bits 0-1)
// ---------------------------------------------------------------------------

/// Control transfer (setup, data, status phases).
pub const USB_ENDPOINT_XFER_CONTROL: u32 = 0;
/// Isochronous transfer (guaranteed bandwidth, no retries).
pub const USB_ENDPOINT_XFER_ISOC: u32 = 1;
/// Bulk transfer (reliable, variable bandwidth).
pub const USB_ENDPOINT_XFER_BULK: u32 = 2;
/// Interrupt transfer (polled, bounded latency).
pub const USB_ENDPOINT_XFER_INT: u32 = 3;
/// Transfer type mask.
pub const USB_ENDPOINT_XFERTYPE_MASK: u32 = 0x03;

// ---------------------------------------------------------------------------
// Isochronous synchronization types (bmAttributes bits 2-3)
// ---------------------------------------------------------------------------

/// No synchronization.
pub const USB_ENDPOINT_SYNC_NONE: u32 = 0x00;
/// Asynchronous (device provides clock).
pub const USB_ENDPOINT_SYNC_ASYNC: u32 = 0x04;
/// Adaptive (device adapts to host clock).
pub const USB_ENDPOINT_SYNC_ADAPTIVE: u32 = 0x08;
/// Synchronous (locked to SOF/USB frame).
pub const USB_ENDPOINT_SYNC_SYNC: u32 = 0x0C;
/// Sync type mask.
pub const USB_ENDPOINT_SYNCTYPE_MASK: u32 = 0x0C;

// ---------------------------------------------------------------------------
// Isochronous usage types (bmAttributes bits 4-5)
// ---------------------------------------------------------------------------

/// Data endpoint.
pub const USB_ENDPOINT_USAGE_DATA: u32 = 0x00;
/// Feedback endpoint.
pub const USB_ENDPOINT_USAGE_FEEDBACK: u32 = 0x10;
/// Implicit feedback data endpoint.
pub const USB_ENDPOINT_USAGE_IMPLICIT_FB: u32 = 0x20;
/// Usage type mask.
pub const USB_ENDPOINT_USAGE_MASK: u32 = 0x30;

// ---------------------------------------------------------------------------
// Maximum packet sizes
// ---------------------------------------------------------------------------

/// Max packet for low-speed interrupt (8 bytes).
pub const USB_MAXPACKET_LS_INT: u32 = 8;
/// Max packet for full-speed bulk (64 bytes).
pub const USB_MAXPACKET_FS_BULK: u32 = 64;
/// Max packet for high-speed bulk (512 bytes).
pub const USB_MAXPACKET_HS_BULK: u32 = 512;
/// Max packet for SuperSpeed bulk (1024 bytes).
pub const USB_MAXPACKET_SS_BULK: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction() {
        assert_eq!(USB_DIR_OUT & USB_ENDPOINT_DIR_MASK, 0);
        assert_eq!(USB_DIR_IN & USB_ENDPOINT_DIR_MASK, USB_DIR_IN);
    }

    #[test]
    fn test_xfer_types_distinct() {
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
    fn test_sync_types_in_mask() {
        let syncs = [
            USB_ENDPOINT_SYNC_NONE,
            USB_ENDPOINT_SYNC_ASYNC,
            USB_ENDPOINT_SYNC_ADAPTIVE,
            USB_ENDPOINT_SYNC_SYNC,
        ];
        for s in &syncs {
            assert_eq!(*s & !USB_ENDPOINT_SYNCTYPE_MASK, 0);
        }
    }

    #[test]
    fn test_max_packets_ordered() {
        assert!(USB_MAXPACKET_LS_INT < USB_MAXPACKET_FS_BULK);
        assert!(USB_MAXPACKET_FS_BULK < USB_MAXPACKET_HS_BULK);
        assert!(USB_MAXPACKET_HS_BULK < USB_MAXPACKET_SS_BULK);
    }
}
