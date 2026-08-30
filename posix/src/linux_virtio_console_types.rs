//! `<linux/virtio_console.h>` — VirtIO console/serial device constants.
//!
//! virtio-console provides virtual serial ports and console access
//! for guest VMs. It supports multiple ports (beyond the primary
//! console), hot-plug of ports, and port naming. The guest
//! communicates via virtqueues; the host presents each port as a
//! chardev (for QEMU) or connects it to a pty, socket, or file.
//! Used for VM serial console, guest agent communication channels,
//! and debugging.

// ---------------------------------------------------------------------------
// VirtIO console feature bits (VIRTIO_CONSOLE_F_*)
// ---------------------------------------------------------------------------

/// Device supports multiple ports.
pub const VIRTIO_CONSOLE_F_MULTIPORT: u32 = 1;
/// Device supports emergency write (crash console).
pub const VIRTIO_CONSOLE_F_EMERG_WRITE: u32 = 2;
/// Port has a name.
pub const VIRTIO_CONSOLE_F_SIZE: u32 = 0;

// ---------------------------------------------------------------------------
// VirtIO console control message IDs
// ---------------------------------------------------------------------------

/// Device ready notification.
pub const VIRTIO_CONSOLE_DEVICE_READY: u32 = 0;
/// Add a new port.
pub const VIRTIO_CONSOLE_DEVICE_ADD: u32 = 1;
/// Remove a port.
pub const VIRTIO_CONSOLE_DEVICE_REMOVE: u32 = 2;
/// Port ready (driver ready to use port).
pub const VIRTIO_CONSOLE_PORT_READY: u32 = 3;
/// Port is a console (primary serial).
pub const VIRTIO_CONSOLE_CONSOLE_PORT: u32 = 4;
/// Resize notification (terminal size change).
pub const VIRTIO_CONSOLE_RESIZE: u32 = 5;
/// Port open notification.
pub const VIRTIO_CONSOLE_PORT_OPEN: u32 = 6;
/// Port name.
pub const VIRTIO_CONSOLE_PORT_NAME: u32 = 7;

// ---------------------------------------------------------------------------
// Port states
// ---------------------------------------------------------------------------

/// Port is closed.
pub const VIRTIO_CONSOLE_PORT_CLOSED: u32 = 0;
/// Port is open.
pub const VIRTIO_CONSOLE_PORT_OPENED: u32 = 1;

// ---------------------------------------------------------------------------
// Maximum values
// ---------------------------------------------------------------------------

/// Maximum number of ports.
pub const VIRTIO_CONSOLE_MAX_PORTS: u32 = 64;
/// Maximum port name length.
pub const VIRTIO_CONSOLE_MAX_NAME_LEN: u32 = 128;

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
    fn test_control_messages_distinct() {
        let msgs = [
            VIRTIO_CONSOLE_DEVICE_READY,
            VIRTIO_CONSOLE_DEVICE_ADD,
            VIRTIO_CONSOLE_DEVICE_REMOVE,
            VIRTIO_CONSOLE_PORT_READY,
            VIRTIO_CONSOLE_CONSOLE_PORT,
            VIRTIO_CONSOLE_RESIZE,
            VIRTIO_CONSOLE_PORT_OPEN,
            VIRTIO_CONSOLE_PORT_NAME,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_port_states_distinct() {
        assert_ne!(VIRTIO_CONSOLE_PORT_CLOSED, VIRTIO_CONSOLE_PORT_OPENED);
    }

    #[test]
    fn test_max_ports() {
        assert_eq!(VIRTIO_CONSOLE_MAX_PORTS, 64);
        assert!(VIRTIO_CONSOLE_MAX_PORTS.is_power_of_two());
    }

    #[test]
    fn test_max_name_len() {
        assert_eq!(VIRTIO_CONSOLE_MAX_NAME_LEN, 128);
    }
}
