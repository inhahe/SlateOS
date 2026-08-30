//! `<sound/asequencer.h>` — ALSA sequencer well-known clients and queues.
//!
//! The ALSA sequencer routes MIDI-like events between clients (which
//! may be applications, kernel synths, or hardware ports). A small
//! number of client IDs are reserved by the kernel and protocol.

// ---------------------------------------------------------------------------
// Reserved client IDs
// ---------------------------------------------------------------------------

/// System client (announce, timer, queue control).
pub const SNDRV_SEQ_CLIENT_SYSTEM: u8 = 0;
/// First user-allocatable client ID.
pub const SNDRV_SEQ_CLIENT_FIRST_USER: u8 = 16;
/// Dummy MIDI through-port for testing/loopback.
pub const SNDRV_SEQ_CLIENT_DUMMY: u8 = 14;
/// OSS-emulation client (Linux-specific).
pub const SNDRV_SEQ_CLIENT_OSS: u8 = 15;
/// Broadcast destination (all subscribed clients).
pub const SNDRV_SEQ_ADDRESS_BROADCAST: u8 = 255;
/// Unknown / unsubscribed address.
pub const SNDRV_SEQ_ADDRESS_UNKNOWN: u8 = 253;
/// Self-address subscription.
pub const SNDRV_SEQ_ADDRESS_SUBSCRIBERS: u8 = 254;

// ---------------------------------------------------------------------------
// System-client port IDs
// ---------------------------------------------------------------------------

pub const SNDRV_SEQ_PORT_SYSTEM_TIMER: u8 = 0;
pub const SNDRV_SEQ_PORT_SYSTEM_ANNOUNCE: u8 = 1;

// ---------------------------------------------------------------------------
// Sequencer limits
// ---------------------------------------------------------------------------

pub const SNDRV_SEQ_MAX_CLIENTS: u32 = 256;
pub const SNDRV_SEQ_MAX_PORTS: u32 = 256;
pub const SNDRV_SEQ_MAX_QUEUES: u32 = 32;
pub const SNDRV_SEQ_MAX_EVENTS: u32 = 2000;

// ---------------------------------------------------------------------------
// Queue tempo / resolution defaults
// ---------------------------------------------------------------------------

/// MIDI standard PPQ (pulses per quarter note).
pub const SNDRV_SEQ_DEFAULT_PPQ: u32 = 96;
/// Default tempo in microseconds per quarter note (120 BPM).
pub const SNDRV_SEQ_DEFAULT_TEMPO_US: u32 = 500_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reserved_clients_distinct_and_below_first_user() {
        let reserved = [
            SNDRV_SEQ_CLIENT_SYSTEM,
            SNDRV_SEQ_CLIENT_DUMMY,
            SNDRV_SEQ_CLIENT_OSS,
        ];
        for &id in &reserved {
            assert!(id < SNDRV_SEQ_CLIENT_FIRST_USER);
        }
        assert_eq!(SNDRV_SEQ_CLIENT_FIRST_USER, 16);
    }

    #[test]
    fn test_special_addresses_high_byte() {
        // The kernel reserves 253..=255 as special addresses.
        assert_eq!(SNDRV_SEQ_ADDRESS_UNKNOWN, 253);
        assert_eq!(SNDRV_SEQ_ADDRESS_SUBSCRIBERS, 254);
        assert_eq!(SNDRV_SEQ_ADDRESS_BROADCAST, 255);
        // Ordered: unknown < subscribers < broadcast.
        assert!(SNDRV_SEQ_ADDRESS_UNKNOWN < SNDRV_SEQ_ADDRESS_SUBSCRIBERS);
        assert!(SNDRV_SEQ_ADDRESS_SUBSCRIBERS < SNDRV_SEQ_ADDRESS_BROADCAST);
    }

    #[test]
    fn test_system_ports_dense_low() {
        assert_eq!(SNDRV_SEQ_PORT_SYSTEM_TIMER, 0);
        assert_eq!(SNDRV_SEQ_PORT_SYSTEM_ANNOUNCE, 1);
    }

    #[test]
    fn test_seq_limits_powers_of_two_except_events() {
        assert!(SNDRV_SEQ_MAX_CLIENTS.is_power_of_two());
        assert!(SNDRV_SEQ_MAX_PORTS.is_power_of_two());
        assert!(SNDRV_SEQ_MAX_QUEUES.is_power_of_two());
        assert_eq!(SNDRV_SEQ_MAX_CLIENTS, 256);
        assert_eq!(SNDRV_SEQ_MAX_QUEUES, 32);
        // 2000 is the historical event limit — not a power of two.
        assert_eq!(SNDRV_SEQ_MAX_EVENTS, 2000);
    }

    #[test]
    fn test_default_tempo_is_120_bpm() {
        // 60_000_000 us/min / 500_000 us/qn = 120 BPM.
        assert_eq!(60_000_000 / SNDRV_SEQ_DEFAULT_TEMPO_US, 120);
        // 96 PPQ is the SMF default.
        assert_eq!(SNDRV_SEQ_DEFAULT_PPQ, 96);
    }
}
