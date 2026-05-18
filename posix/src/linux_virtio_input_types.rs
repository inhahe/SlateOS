//! `<linux/virtio_input.h>` — VirtIO input device constants.
//!
//! virtio-input passes input events (keyboard, mouse, touchscreen)
//! from host to guest. It wraps Linux evdev events in virtio
//! transport, providing the same event codes and types used by
//! the native input subsystem.

// ---------------------------------------------------------------------------
// Config select values (virtio_input_config.select)
// ---------------------------------------------------------------------------

/// Unset (no info requested).
pub const VIRTIO_INPUT_CFG_UNSET: u8 = 0x00;
/// Select ID string.
pub const VIRTIO_INPUT_CFG_ID_NAME: u8 = 0x01;
/// Select serial number string.
pub const VIRTIO_INPUT_CFG_ID_SERIAL: u8 = 0x02;
/// Select device ID (bustype, vendor, product, version).
pub const VIRTIO_INPUT_CFG_ID_DEVIDS: u8 = 0x03;
/// Select supported properties.
pub const VIRTIO_INPUT_CFG_PROP_BITS: u8 = 0x10;
/// Select supported event types.
pub const VIRTIO_INPUT_CFG_EV_BITS: u8 = 0x11;
/// Select supported absolute axes info.
pub const VIRTIO_INPUT_CFG_ABS_INFO: u8 = 0x12;

// ---------------------------------------------------------------------------
// Event sizes
// ---------------------------------------------------------------------------

/// Size of one virtio_input_event (type + code + value = 8 bytes).
pub const VIRTIO_INPUT_EVENT_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// VirtIO input virtqueue indices
// ---------------------------------------------------------------------------

/// Event virtqueue (device → driver, events).
pub const VIRTIO_INPUT_VQ_EVENTS: u32 = 0;
/// Status virtqueue (driver → device, LED/FF status).
pub const VIRTIO_INPUT_VQ_STATUS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_selects_distinct() {
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
    fn test_unset_is_zero() {
        assert_eq!(VIRTIO_INPUT_CFG_UNSET, 0);
    }

    #[test]
    fn test_vq_indices() {
        assert_ne!(VIRTIO_INPUT_VQ_EVENTS, VIRTIO_INPUT_VQ_STATUS);
    }

    #[test]
    fn test_event_size() {
        assert_eq!(VIRTIO_INPUT_EVENT_SIZE, 8);
    }
}
