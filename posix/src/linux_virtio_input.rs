//! `<linux/virtio_input.h>` — Virtio input device constants.
//!
//! Virtio-input provides keyboard, mouse, and tablet input in VMs.
//! Carries Linux input events (struct input_event) over virtqueues.

pub use crate::linux_virtio_types::VIRTIO_ID_INPUT;

// ---------------------------------------------------------------------------
// Config select values
// ---------------------------------------------------------------------------

/// Unset/undefined.
pub const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
/// Device name string.
pub const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
/// Device serial number.
pub const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
/// Device ID (bustype, vendor, product, version).
pub const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
/// Properties bitmap.
pub const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
/// Event type bitmap.
pub const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
/// Absolute axis info.
pub const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

// ---------------------------------------------------------------------------
// Input event structure
// ---------------------------------------------------------------------------

/// Virtio input event (matches Linux struct input_event fields).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputEvent {
    /// Event type (EV_KEY, EV_REL, etc.).
    pub event_type: u16,
    /// Event code (KEY_A, REL_X, etc.).
    pub code: u16,
    /// Event value.
    pub value: u32,
}

impl VirtioInputEvent {
    /// Create a zeroed input event.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Virtio input device ID.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputDevids {
    /// Bus type.
    pub bustype: u16,
    /// Vendor ID.
    pub vendor: u16,
    /// Product ID.
    pub product: u16,
    /// Version.
    pub version: u16,
}

impl VirtioInputDevids {
    /// Create a zeroed device ID.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Virtio input absolute axis info.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputAbsinfo {
    /// Minimum value.
    pub min: u32,
    /// Maximum value.
    pub max: u32,
    /// Fuzz (noise threshold).
    pub fuzz: u32,
    /// Flat (dead zone).
    pub flat: u32,
    /// Resolution.
    pub res: u32,
}

impl VirtioInputAbsinfo {
    /// Create a zeroed absinfo.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cfg_select_distinct() {
        let cfgs = [
            VIRTIO_INPUT_CFG_UNSET, VIRTIO_INPUT_CFG_ID_NAME,
            VIRTIO_INPUT_CFG_ID_SERIAL, VIRTIO_INPUT_CFG_ID_DEVIDS,
            VIRTIO_INPUT_CFG_PROP_BITS, VIRTIO_INPUT_CFG_EV_BITS,
            VIRTIO_INPUT_CFG_ABS_INFO,
        ];
        for i in 0..cfgs.len() {
            for j in (i + 1)..cfgs.len() {
                assert_ne!(cfgs[i], cfgs[j]);
            }
        }
    }

    #[test]
    fn test_event_size() {
        assert_eq!(core::mem::size_of::<VirtioInputEvent>(), 8);
    }

    #[test]
    fn test_devids_size() {
        assert_eq!(core::mem::size_of::<VirtioInputDevids>(), 8);
    }

    #[test]
    fn test_absinfo_size() {
        assert_eq!(core::mem::size_of::<VirtioInputAbsinfo>(), 20);
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_INPUT, 18);
    }

    #[test]
    fn test_event_zeroed() {
        let ev = VirtioInputEvent::zeroed();
        assert_eq!(ev.event_type, 0);
        assert_eq!(ev.code, 0);
        assert_eq!(ev.value, 0);
    }
}
