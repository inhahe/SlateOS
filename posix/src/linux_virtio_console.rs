//! `<linux/virtio_console.h>` — Virtio console device constants.
//!
//! Virtio-console provides serial/console ports in VMs. Supports
//! multiple ports with hot-plug capability and console resize.

pub use crate::linux_virtio_types::VIRTIO_ID_CONSOLE;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Device supports console size.
pub const VIRTIO_CONSOLE_F_SIZE: u32 = 0;
/// Device supports multiple ports.
pub const VIRTIO_CONSOLE_F_MULTIPORT: u32 = 1;
/// Device supports emergency write.
pub const VIRTIO_CONSOLE_F_EMERG_WRITE: u32 = 2;

// ---------------------------------------------------------------------------
// Control messages
// ---------------------------------------------------------------------------

/// Device ready.
pub const VIRTIO_CONSOLE_DEVICE_READY: u32 = 0;
/// Device add (new port).
pub const VIRTIO_CONSOLE_DEVICE_ADD: u32 = 1;
/// Device remove (port removed).
pub const VIRTIO_CONSOLE_DEVICE_REMOVE: u32 = 2;
/// Port ready.
pub const VIRTIO_CONSOLE_PORT_READY: u32 = 3;
/// Console port designation.
pub const VIRTIO_CONSOLE_CONSOLE_PORT: u32 = 4;
/// Console resize.
pub const VIRTIO_CONSOLE_RESIZE: u32 = 5;
/// Port open.
pub const VIRTIO_CONSOLE_PORT_OPEN: u32 = 6;
/// Port name.
pub const VIRTIO_CONSOLE_PORT_NAME: u32 = 7;

// ---------------------------------------------------------------------------
// Control struct
// ---------------------------------------------------------------------------

/// Virtio console control message.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioConsoleControl {
    /// Port number.
    pub id: u32,
    /// Control event type.
    pub event: u16,
    /// Event value.
    pub value: u16,
}

impl VirtioConsoleControl {
    /// Create a zeroed control message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Console resize message.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioConsoleResize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

impl VirtioConsoleResize {
    /// Create a zeroed resize message.
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
    fn test_features_distinct() {
        let feats = [
            VIRTIO_CONSOLE_F_SIZE,
            VIRTIO_CONSOLE_F_MULTIPORT,
            VIRTIO_CONSOLE_F_EMERG_WRITE,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_events_distinct() {
        let events = [
            VIRTIO_CONSOLE_DEVICE_READY,
            VIRTIO_CONSOLE_DEVICE_ADD,
            VIRTIO_CONSOLE_DEVICE_REMOVE,
            VIRTIO_CONSOLE_PORT_READY,
            VIRTIO_CONSOLE_CONSOLE_PORT,
            VIRTIO_CONSOLE_RESIZE,
            VIRTIO_CONSOLE_PORT_OPEN,
            VIRTIO_CONSOLE_PORT_NAME,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_control_size() {
        assert_eq!(core::mem::size_of::<VirtioConsoleControl>(), 8);
    }

    #[test]
    fn test_resize_size() {
        assert_eq!(core::mem::size_of::<VirtioConsoleResize>(), 4);
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_CONSOLE, 3);
    }
}
