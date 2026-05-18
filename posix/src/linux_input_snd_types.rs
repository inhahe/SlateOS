//! `<linux/input-event-codes.h>` (SND subset) — sound output event codes.
//!
//! Sound events control simple tone generators built into input
//! devices (PC speaker beep, keyboard click sounds). These are
//! distinct from the ALSA audio subsystem — SND events drive tiny
//! piezo speakers or buzzers, not full audio hardware.

// ---------------------------------------------------------------------------
// Sound codes
// ---------------------------------------------------------------------------

/// System click sound (keyboard feedback).
pub const SND_CLICK: u16 = 0x00;
/// System bell / beep.
pub const SND_BELL: u16 = 0x01;
/// Tone generator (frequency in Hz as value).
pub const SND_TONE: u16 = 0x02;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum sound code.
pub const SND_MAX: u16 = 0x07;
/// Number of sound codes (SND_MAX + 1).
pub const SND_CNT: u16 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snd_codes_distinct() {
        assert_ne!(SND_CLICK, SND_BELL);
        assert_ne!(SND_CLICK, SND_TONE);
        assert_ne!(SND_BELL, SND_TONE);
    }

    #[test]
    fn test_snd_sequential() {
        assert_eq!(SND_CLICK, 0);
        assert_eq!(SND_BELL, 1);
        assert_eq!(SND_TONE, 2);
    }

    #[test]
    fn test_all_within_max() {
        assert!(SND_CLICK <= SND_MAX);
        assert!(SND_BELL <= SND_MAX);
        assert!(SND_TONE <= SND_MAX);
    }

    #[test]
    fn test_snd_cnt() {
        assert_eq!(SND_CNT, SND_MAX + 1);
    }
}
