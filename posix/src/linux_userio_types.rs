//! `<linux/userio.h>` — userio (in-userspace serio) constants.
//!
//! `/dev/userio` lets userspace programs implement a virtual serio
//! port (PS/2 keyboard or mouse) — used by `psmouse-alps` userspace
//! drivers and protocol fuzzers. These constants describe the
//! single-byte wire protocol exchanged over the device.

// ---------------------------------------------------------------------------
// Userio commands (sent from userspace to /dev/userio)
// ---------------------------------------------------------------------------

/// Register a virtual serio port.
pub const USERIO_CMD_REGISTER: u8 = 0;
/// Set the serio type (PS/2 keyboard vs. mouse).
pub const USERIO_CMD_SET_PORT_TYPE: u8 = 1;
/// Send a byte from the virtual device to the host.
pub const USERIO_CMD_SEND_INTERRUPT: u8 = 2;

// ---------------------------------------------------------------------------
// Userio events (read from /dev/userio)
// ---------------------------------------------------------------------------

/// No event in this slot.
pub const USERIO_EVENT_NONE: u8 = 0;
/// Host wrote a byte to the virtual port (TX from kernel side).
pub const USERIO_EVENT_TX: u8 = 1;
/// Host requested a port-control change (reset etc.).
pub const USERIO_EVENT_CMD: u8 = 2;

// ---------------------------------------------------------------------------
// Serio port-type tags (used as payload of USERIO_CMD_SET_PORT_TYPE)
// ---------------------------------------------------------------------------

/// 8042 KBD port (PS/2 keyboard).
pub const SERIO_8042_KBD: u8 = 0x01;
/// 8042 AUX port (PS/2 mouse).
pub const SERIO_8042_AUX: u8 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            USERIO_CMD_REGISTER,
            USERIO_CMD_SET_PORT_TYPE,
            USERIO_CMD_SEND_INTERRUPT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [USERIO_EVENT_NONE, USERIO_EVENT_TX, USERIO_EVENT_CMD];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
        // NONE must be zero so an unwritten buffer reads as no-event.
        assert_eq!(USERIO_EVENT_NONE, 0);
    }

    #[test]
    fn test_port_types_distinct() {
        assert_ne!(SERIO_8042_KBD, SERIO_8042_AUX);
        // Port types must be non-zero so an uninitialised payload
        // cannot accidentally select KBD or AUX.
        assert_ne!(SERIO_8042_KBD, 0);
        assert_ne!(SERIO_8042_AUX, 0);
    }
}
