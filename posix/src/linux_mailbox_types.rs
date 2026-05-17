//! `<linux/mailbox_controller.h>` — Mailbox framework constants.
//!
//! The mailbox framework provides a generic API for inter-processor
//! communication hardware (doorbell registers, shared memory FIFOs,
//! interrupt-based signaling). A mailbox client (driver needing IPC)
//! requests a channel from a mailbox controller (hardware-specific
//! driver). Messages are typically small (pointer-sized) used as
//! doorbell/signal; bulk data goes through shared memory separately.

// ---------------------------------------------------------------------------
// Mailbox channel states
// ---------------------------------------------------------------------------

/// Channel is free (not allocated).
pub const MBOX_CHAN_FREE: u32 = 0;
/// Channel is allocated to a client.
pub const MBOX_CHAN_ALLOCATED: u32 = 1;
/// Channel has a pending message.
pub const MBOX_CHAN_PENDING: u32 = 2;

// ---------------------------------------------------------------------------
// Mailbox transmit modes
// ---------------------------------------------------------------------------

/// Blocking transmit (wait for previous message to be consumed).
pub const MBOX_TX_BLOCK: u32 = 0;
/// Non-blocking transmit (fail if channel busy).
pub const MBOX_TX_NONBLOCK: u32 = 1;

// ---------------------------------------------------------------------------
// Mailbox signal types
// ---------------------------------------------------------------------------

/// Doorbell (no data, just a signal/interrupt).
pub const MBOX_SIGNAL_DOORBELL: u32 = 0;
/// Data message (small payload in registers).
pub const MBOX_SIGNAL_DATA: u32 = 1;

// ---------------------------------------------------------------------------
// Mailbox controller flags
// ---------------------------------------------------------------------------

/// Controller supports multiple concurrent channels.
pub const MBOX_FLAG_MULTI_CHANNEL: u32 = 1 << 0;
/// Controller supports bidirectional communication.
pub const MBOX_FLAG_BIDIRECTIONAL: u32 = 1 << 1;
/// Controller supports polling mode (no interrupt).
pub const MBOX_FLAG_POLLING: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Mailbox client callback events
// ---------------------------------------------------------------------------

/// Message was successfully sent.
pub const MBOX_EVENT_TX_DONE: u32 = 0;
/// Message was received (RX callback).
pub const MBOX_EVENT_RX: u32 = 1;
/// Error occurred on channel.
pub const MBOX_EVENT_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_states_distinct() {
        let states = [MBOX_CHAN_FREE, MBOX_CHAN_ALLOCATED, MBOX_CHAN_PENDING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tx_modes_distinct() {
        assert_ne!(MBOX_TX_BLOCK, MBOX_TX_NONBLOCK);
    }

    #[test]
    fn test_signal_types_distinct() {
        assert_ne!(MBOX_SIGNAL_DOORBELL, MBOX_SIGNAL_DATA);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            MBOX_FLAG_MULTI_CHANNEL, MBOX_FLAG_BIDIRECTIONAL,
            MBOX_FLAG_POLLING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [MBOX_EVENT_TX_DONE, MBOX_EVENT_RX, MBOX_EVENT_ERROR];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
