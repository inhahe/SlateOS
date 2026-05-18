//! `<linux/usb/ch9.h>` (class subset) — USB device class constants.
//!
//! USB device classes identify what type of device is connected,
//! allowing the host to load the correct generic driver without
//! needing vendor-specific knowledge. Classes are specified at the
//! device level (bDeviceClass) or interface level (bInterfaceClass).
//! Interface-level class codes allow composite devices (e.g., a
//! keyboard with a built-in hub).

// ---------------------------------------------------------------------------
// USB device/interface class codes
// ---------------------------------------------------------------------------

/// Per-interface class (check each interface's bInterfaceClass).
pub const USB_CLASS_PER_INTERFACE: u32 = 0x00;
/// Audio device (speakers, microphones, MIDI).
pub const USB_CLASS_AUDIO: u32 = 0x01;
/// Communications device (modems, Ethernet adapters).
pub const USB_CLASS_COMM: u32 = 0x02;
/// Human Interface Device (keyboard, mouse, gamepad).
pub const USB_CLASS_HID: u32 = 0x03;
/// Physical device (force feedback).
pub const USB_CLASS_PHYSICAL: u32 = 0x05;
/// Still image (cameras, scanners).
pub const USB_CLASS_STILL_IMAGE: u32 = 0x06;
/// Printer.
pub const USB_CLASS_PRINTER: u32 = 0x07;
/// Mass storage (USB drives, card readers).
pub const USB_CLASS_MASS_STORAGE: u32 = 0x08;
/// Hub.
pub const USB_CLASS_HUB: u32 = 0x09;
/// CDC data (data interface for comm devices).
pub const USB_CLASS_CDC_DATA: u32 = 0x0A;
/// Smart card (CCID).
pub const USB_CLASS_CSCID: u32 = 0x0B;
/// Content security.
pub const USB_CLASS_CONTENT_SEC: u32 = 0x0D;
/// Video (webcams).
pub const USB_CLASS_VIDEO: u32 = 0x0E;
/// Personal healthcare (pulse oximeter, blood pressure).
pub const USB_CLASS_PERSONAL_HEALTHCARE: u32 = 0x0F;
/// Audio/video (A/V streaming).
pub const USB_CLASS_AV: u32 = 0x10;
/// Billboard (USB-C alternate mode advertising).
pub const USB_CLASS_BILLBOARD: u32 = 0x11;
/// USB Type-C bridge.
pub const USB_CLASS_TYPE_C_BRIDGE: u32 = 0x12;
/// Wireless controller (Bluetooth, WiFi adapter).
pub const USB_CLASS_WIRELESS: u32 = 0xE0;
/// Miscellaneous (IAD-using composite devices).
pub const USB_CLASS_MISC: u32 = 0xEF;
/// Application-specific.
pub const USB_CLASS_APP_SPEC: u32 = 0xFE;
/// Vendor-specific.
pub const USB_CLASS_VENDOR_SPEC: u32 = 0xFF;

// ---------------------------------------------------------------------------
// USB mass storage subclass codes
// ---------------------------------------------------------------------------

/// SCSI transparent command set.
pub const USB_SC_SCSI: u32 = 0x06;
/// UFI (USB Floppy Interface).
pub const USB_SC_UFI: u32 = 0x04;
/// RBC (Reduced Block Commands).
pub const USB_SC_RBC: u32 = 0x01;

// ---------------------------------------------------------------------------
// USB mass storage protocol codes
// ---------------------------------------------------------------------------

/// Bulk-Only Transport (BOT).
pub const USB_PR_BULK: u32 = 0x50;
/// USB Attached SCSI (UAS).
pub const USB_PR_UAS: u32 = 0x62;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [
            USB_CLASS_PER_INTERFACE, USB_CLASS_AUDIO, USB_CLASS_COMM,
            USB_CLASS_HID, USB_CLASS_PHYSICAL, USB_CLASS_STILL_IMAGE,
            USB_CLASS_PRINTER, USB_CLASS_MASS_STORAGE, USB_CLASS_HUB,
            USB_CLASS_CDC_DATA, USB_CLASS_CSCID, USB_CLASS_CONTENT_SEC,
            USB_CLASS_VIDEO, USB_CLASS_PERSONAL_HEALTHCARE,
            USB_CLASS_AV, USB_CLASS_BILLBOARD, USB_CLASS_TYPE_C_BRIDGE,
            USB_CLASS_WIRELESS, USB_CLASS_MISC,
            USB_CLASS_APP_SPEC, USB_CLASS_VENDOR_SPEC,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_mass_storage_protocols_distinct() {
        assert_ne!(USB_PR_BULK, USB_PR_UAS);
    }

    #[test]
    fn test_subclasses_distinct() {
        let scs = [USB_SC_SCSI, USB_SC_UFI, USB_SC_RBC];
        for i in 0..scs.len() {
            for j in (i + 1)..scs.len() {
                assert_ne!(scs[i], scs[j]);
            }
        }
    }
}
