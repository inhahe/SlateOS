//! `<linux/mailbox_controller.h>` — Mailbox framework constants.
//!
//! The mailbox framework provides inter-processor communication (IPC)
//! between the main CPU and co-processors (DSPs, MCUs, secure
//! enclaves, GPU command processors). Messages are typically small
//! (a few words) and signal the co-processor to act on shared memory.

// ---------------------------------------------------------------------------
// Mailbox channel states
// ---------------------------------------------------------------------------

/// Channel is free (idle).
pub const MBOX_STATE_FREE: u8 = 0;
/// Channel is active (in use).
pub const MBOX_STATE_ACTIVE: u8 = 1;
/// Channel request pending.
pub const MBOX_STATE_PENDING: u8 = 2;
/// Channel error.
pub const MBOX_STATE_ERROR: u8 = 3;

// ---------------------------------------------------------------------------
// Mailbox transfer modes
// ---------------------------------------------------------------------------

/// Blocking send (wait for ACK).
pub const MBOX_TX_BLOCK: u8 = 0;
/// Non-blocking send (fire and forget).
pub const MBOX_TX_NON_BLOCK: u8 = 1;
/// Queued send (multiple messages).
pub const MBOX_TX_QUEUE: u8 = 2;

// ---------------------------------------------------------------------------
// Mailbox signal types
// ---------------------------------------------------------------------------

/// Doorbell (interrupt-based notification).
pub const MBOX_SIGNAL_DOORBELL: u8 = 0;
/// Data message (payload in registers).
pub const MBOX_SIGNAL_DATA: u8 = 1;
/// Shared memory (pointer/offset in message).
pub const MBOX_SIGNAL_SHMEM: u8 = 2;

// ---------------------------------------------------------------------------
// Mailbox controller flags
// ---------------------------------------------------------------------------

/// Controller has TX done IRQ.
pub const MBOX_F_TXDONE_IRQ: u32 = 1 << 0;
/// Controller polls TX done status.
pub const MBOX_F_TXDONE_POLL: u32 = 1 << 1;
/// Controller has RX IRQ.
pub const MBOX_F_RX_IRQ: u32 = 1 << 2;
/// Controller supports flush.
pub const MBOX_F_FLUSH: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Common SCMI (System Control and Management Interface) channels
// ---------------------------------------------------------------------------

/// SCMI power domain protocol.
pub const MBOX_SCMI_POWER: u8 = 0x11;
/// SCMI performance protocol.
pub const MBOX_SCMI_PERF: u8 = 0x13;
/// SCMI clock protocol.
pub const MBOX_SCMI_CLOCK: u8 = 0x14;
/// SCMI sensor protocol.
pub const MBOX_SCMI_SENSOR: u8 = 0x15;
/// SCMI reset protocol.
pub const MBOX_SCMI_RESET: u8 = 0x16;
/// SCMI voltage protocol.
pub const MBOX_SCMI_VOLTAGE: u8 = 0x17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            MBOX_STATE_FREE, MBOX_STATE_ACTIVE,
            MBOX_STATE_PENDING, MBOX_STATE_ERROR,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tx_modes_distinct() {
        let modes = [MBOX_TX_BLOCK, MBOX_TX_NON_BLOCK, MBOX_TX_QUEUE];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_signal_types_distinct() {
        let signals = [MBOX_SIGNAL_DOORBELL, MBOX_SIGNAL_DATA, MBOX_SIGNAL_SHMEM];
        for i in 0..signals.len() {
            for j in (i + 1)..signals.len() {
                assert_ne!(signals[i], signals[j]);
            }
        }
    }

    #[test]
    fn test_controller_flags_no_overlap() {
        let flags = [
            MBOX_F_TXDONE_IRQ, MBOX_F_TXDONE_POLL,
            MBOX_F_RX_IRQ, MBOX_F_FLUSH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_scmi_protocols_distinct() {
        let protos = [
            MBOX_SCMI_POWER, MBOX_SCMI_PERF, MBOX_SCMI_CLOCK,
            MBOX_SCMI_SENSOR, MBOX_SCMI_RESET, MBOX_SCMI_VOLTAGE,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }
}
