//! `<linux/usb.h>` (URB subset) — USB Request Block constants.
//!
//! URBs (USB Request Blocks) are the kernel's data structure for
//! submitting USB transfers. A driver allocates a URB, fills in the
//! endpoint, transfer buffer, and completion callback, then submits
//! it to the USB core. The host controller driver (xHCI, EHCI, etc.)
//! processes it asynchronously and calls the completion handler when
//! done. URB flags control transfer behavior.

// ---------------------------------------------------------------------------
// URB transfer flags
// ---------------------------------------------------------------------------

/// Short read is not an error (allow less data than requested).
pub const URB_SHORT_NOT_OK: u32 = 0x0001;
/// Transfer is isochronous.
pub const URB_ISO_ASAP: u32 = 0x0002;
/// Don't use DMA mapping (buffer is already DMA-mapped).
pub const URB_NO_TRANSFER_DMA_MAP: u32 = 0x0004;
/// Don't use DMA for setup packet.
pub const URB_NO_SETUP_DMA_MAP: u32 = 0x0008;
/// Zero-length packet at end of transfer.
pub const URB_ZERO_PACKET: u32 = 0x0040;
/// Don't interrupt on completion (batch completions).
pub const URB_NO_INTERRUPT: u32 = 0x0080;
/// Free buffer on completion.
pub const URB_FREE_BUFFER: u32 = 0x0100;
/// URB was unlinked (cancelled).
pub const URB_DIR_IN: u32 = 0x0200;

// ---------------------------------------------------------------------------
// URB status codes
// ---------------------------------------------------------------------------

/// Transfer completed successfully.
pub const URB_STATUS_SUCCESS: i32 = 0;
/// Transfer cancelled (unlinked).
pub const URB_STATUS_UNLINKED: i32 = -2;
/// Endpoint stalled (protocol error).
pub const URB_STATUS_STALL: i32 = -32;
/// Device not responding.
pub const URB_STATUS_NODEV: i32 = -19;
/// Data overrun.
pub const URB_STATUS_OVERFLOW: i32 = -75;
/// Transfer in progress.
pub const URB_STATUS_INPROGRESS: i32 = -115;
/// CRC or bitstuff error.
pub const URB_STATUS_CRC: i32 = -84;
/// Babble detected (device sent too much data).
pub const URB_STATUS_BABBLE: i32 = -71;

// ---------------------------------------------------------------------------
// USB control request types (bmRequestType)
// ---------------------------------------------------------------------------

/// Standard request.
pub const USB_TYPE_STANDARD: u32 = 0x00;
/// Class-specific request.
pub const USB_TYPE_CLASS: u32 = 0x20;
/// Vendor-specific request.
pub const USB_TYPE_VENDOR: u32 = 0x40;
/// Request type mask.
pub const USB_TYPE_MASK: u32 = 0x60;
/// Recipient: device.
pub const USB_RECIP_DEVICE: u32 = 0x00;
/// Recipient: interface.
pub const USB_RECIP_INTERFACE: u32 = 0x01;
/// Recipient: endpoint.
pub const USB_RECIP_ENDPOINT: u32 = 0x02;
/// Recipient: other.
pub const USB_RECIP_OTHER: u32 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_types_in_mask() {
        let types = [USB_TYPE_STANDARD, USB_TYPE_CLASS, USB_TYPE_VENDOR];
        for t in &types {
            assert_eq!(*t & !USB_TYPE_MASK, 0);
        }
    }

    #[test]
    fn test_recipients_distinct() {
        let recips = [
            USB_RECIP_DEVICE, USB_RECIP_INTERFACE,
            USB_RECIP_ENDPOINT, USB_RECIP_OTHER,
        ];
        for i in 0..recips.len() {
            for j in (i + 1)..recips.len() {
                assert_ne!(recips[i], recips[j]);
            }
        }
    }

    #[test]
    fn test_status_codes() {
        assert_eq!(URB_STATUS_SUCCESS, 0);
        assert!(URB_STATUS_STALL < 0);
        assert!(URB_STATUS_NODEV < 0);
    }
}
