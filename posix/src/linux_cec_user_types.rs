//! `<linux/cec.h>` — Consumer-Electronics Control (HDMI-CEC).
//!
//! CEC is the 1-wire bus inside HDMI that lets a TV remote control
//! the AVR, the Blu-ray player, and the games console. Linux exposes
//! it through `/dev/cecN` with a structured message API.

// ---------------------------------------------------------------------------
// Device naming
// ---------------------------------------------------------------------------

pub const CEC_DEVICE_PREFIX: &str = "/dev/cec";

// ---------------------------------------------------------------------------
// CEC frame layout limits
// ---------------------------------------------------------------------------

/// Maximum bytes in a CEC message (15 data bytes + 1 header).
pub const CEC_MAX_MSG_SIZE: usize = 16;

/// Header byte: low nibble = destination, high nibble = source.
pub const CEC_HEADER_SIZE: usize = 1;

/// Maximum 4-bit logical address (15).
pub const CEC_LOG_ADDR_MASK: u8 = 0x0F;

// ---------------------------------------------------------------------------
// Logical addresses (`enum cec_logical_address`)
// ---------------------------------------------------------------------------

pub const CEC_LOG_ADDR_TV: u8 = 0;
pub const CEC_LOG_ADDR_RECORD_1: u8 = 1;
pub const CEC_LOG_ADDR_RECORD_2: u8 = 2;
pub const CEC_LOG_ADDR_TUNER_1: u8 = 3;
pub const CEC_LOG_ADDR_PLAYBACK_1: u8 = 4;
pub const CEC_LOG_ADDR_AUDIOSYSTEM: u8 = 5;
pub const CEC_LOG_ADDR_TUNER_2: u8 = 6;
pub const CEC_LOG_ADDR_TUNER_3: u8 = 7;
pub const CEC_LOG_ADDR_PLAYBACK_2: u8 = 8;
pub const CEC_LOG_ADDR_RECORD_3: u8 = 9;
pub const CEC_LOG_ADDR_TUNER_4: u8 = 10;
pub const CEC_LOG_ADDR_PLAYBACK_3: u8 = 11;
pub const CEC_LOG_ADDR_BACKUP_1: u8 = 12;
pub const CEC_LOG_ADDR_BACKUP_2: u8 = 13;
pub const CEC_LOG_ADDR_SPECIFIC: u8 = 14;
pub const CEC_LOG_ADDR_UNREGISTERED: u8 = 15;

// ---------------------------------------------------------------------------
// Capability flag bits (`struct cec_caps.capabilities`)
// ---------------------------------------------------------------------------

pub const CEC_CAP_PHYS_ADDR: u32 = 1 << 0;
pub const CEC_CAP_LOG_ADDRS: u32 = 1 << 1;
pub const CEC_CAP_TRANSMIT: u32 = 1 << 2;
pub const CEC_CAP_PASSTHROUGH: u32 = 1 << 3;
pub const CEC_CAP_RC: u32 = 1 << 4;
pub const CEC_CAP_MONITOR_ALL: u32 = 1 << 5;
pub const CEC_CAP_NEEDS_HPD: u32 = 1 << 6;
pub const CEC_CAP_MONITOR_PIN: u32 = 1 << 7;
pub const CEC_CAP_CONNECTOR_INFO: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Default physical address (no device).
// ---------------------------------------------------------------------------

/// Invalid / unset physical address (4 hex nibbles all-ones).
pub const CEC_PHYS_ADDR_INVALID: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_size_layout() {
        // 16 bytes total: 1 header + 15 data bytes.
        assert_eq!(CEC_MAX_MSG_SIZE, 16);
        assert_eq!(CEC_HEADER_SIZE, 1);
        assert!(CEC_MAX_MSG_SIZE.is_power_of_two());
    }

    #[test]
    fn test_logical_addresses_dense_0_to_15() {
        let a = [
            CEC_LOG_ADDR_TV,
            CEC_LOG_ADDR_RECORD_1,
            CEC_LOG_ADDR_RECORD_2,
            CEC_LOG_ADDR_TUNER_1,
            CEC_LOG_ADDR_PLAYBACK_1,
            CEC_LOG_ADDR_AUDIOSYSTEM,
            CEC_LOG_ADDR_TUNER_2,
            CEC_LOG_ADDR_TUNER_3,
            CEC_LOG_ADDR_PLAYBACK_2,
            CEC_LOG_ADDR_RECORD_3,
            CEC_LOG_ADDR_TUNER_4,
            CEC_LOG_ADDR_PLAYBACK_3,
            CEC_LOG_ADDR_BACKUP_1,
            CEC_LOG_ADDR_BACKUP_2,
            CEC_LOG_ADDR_SPECIFIC,
            CEC_LOG_ADDR_UNREGISTERED,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Fits in 4 bits.
        assert_eq!(CEC_LOG_ADDR_UNREGISTERED, CEC_LOG_ADDR_MASK);
    }

    #[test]
    fn test_capability_bits_distinct_single_bits() {
        let f = [
            CEC_CAP_PHYS_ADDR,
            CEC_CAP_LOG_ADDRS,
            CEC_CAP_TRANSMIT,
            CEC_CAP_PASSTHROUGH,
            CEC_CAP_RC,
            CEC_CAP_MONITOR_ALL,
            CEC_CAP_NEEDS_HPD,
            CEC_CAP_MONITOR_PIN,
            CEC_CAP_CONNECTOR_INFO,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // 9 bits set in OR of all flags.
        let all: u32 = f.iter().fold(0, |acc, &v| acc | v);
        assert_eq!(all.count_ones(), 9);
    }

    #[test]
    fn test_phys_addr_invalid_is_all_ones() {
        assert_eq!(CEC_PHYS_ADDR_INVALID, 0xFFFF);
        assert_eq!(CEC_PHYS_ADDR_INVALID.count_ones(), 16);
    }

    #[test]
    fn test_audio_and_playback_addresses_correct() {
        // The AVR sits at logical address 5 per the spec.
        assert_eq!(CEC_LOG_ADDR_AUDIOSYSTEM, 5);
        // Three independent playback devices.
        assert_eq!(CEC_LOG_ADDR_PLAYBACK_1, 4);
        assert_eq!(CEC_LOG_ADDR_PLAYBACK_2, 8);
        assert_eq!(CEC_LOG_ADDR_PLAYBACK_3, 11);
    }

    #[test]
    fn test_device_prefix_under_dev() {
        assert!(CEC_DEVICE_PREFIX.starts_with("/dev/"));
        assert!(CEC_DEVICE_PREFIX.ends_with("cec"));
    }
}
