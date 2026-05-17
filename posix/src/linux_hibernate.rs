//! `<linux/suspend.h>` (hibernate) — Hibernation (suspend-to-disk) constants.
//!
//! Hibernation saves the entire system state to a swap partition or
//! file, then powers off. On resume, the kernel restores the saved
//! image and continues execution. This module defines hibernation
//! image header constants, compression modes, and states.

// ---------------------------------------------------------------------------
// Hibernation states
// ---------------------------------------------------------------------------

/// Freezing processes.
pub const HIBERNATE_STATE_FREEZE: u8 = 0;
/// Creating snapshot image.
pub const HIBERNATE_STATE_SNAPSHOT: u8 = 1;
/// Writing image to storage.
pub const HIBERNATE_STATE_WRITE: u8 = 2;
/// Platform-specific power-off.
pub const HIBERNATE_STATE_PLATFORM: u8 = 3;
/// Entering S4 (ACPI S4).
pub const HIBERNATE_STATE_ENTER: u8 = 4;
/// Resuming from hibernation.
pub const HIBERNATE_STATE_RESUME: u8 = 5;

// ---------------------------------------------------------------------------
// Image header magic
// ---------------------------------------------------------------------------

/// Swap partition signature ("SWSUSP2").
pub const HIBERNATE_SIG: &str = "SWSUSP2";
/// Header version.
pub const HIBERNATE_HEADER_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Compression modes
// ---------------------------------------------------------------------------

/// No compression.
pub const HIBERNATE_COMP_NONE: u8 = 0;
/// LZO compression.
pub const HIBERNATE_COMP_LZO: u8 = 1;
/// LZ4 compression.
pub const HIBERNATE_COMP_LZ4: u8 = 2;
/// ZSTD compression.
pub const HIBERNATE_COMP_ZSTD: u8 = 3;

// ---------------------------------------------------------------------------
// Hibernation flags
// ---------------------------------------------------------------------------

/// Use platform mode (ACPI S4).
pub const HIBERNATE_F_PLATFORM: u32 = 1 << 0;
/// Write image to file (not swap).
pub const HIBERNATE_F_FILE: u32 = 1 << 1;
/// Encrypt image.
pub const HIBERNATE_F_ENCRYPT: u32 = 1 << 2;
/// Compress image.
pub const HIBERNATE_F_COMPRESS: u32 = 1 << 3;
/// Resume from file.
pub const HIBERNATE_F_RESUME_FILE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Page flags in hibernate image
// ---------------------------------------------------------------------------

/// Page contains data (not free).
pub const HIBERNATE_PF_DATA: u32 = 1 << 0;
/// Page is from highmem zone.
pub const HIBERNATE_PF_HIGHMEM: u32 = 1 << 1;
/// Page was dirty.
pub const HIBERNATE_PF_DIRTY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            HIBERNATE_STATE_FREEZE, HIBERNATE_STATE_SNAPSHOT,
            HIBERNATE_STATE_WRITE, HIBERNATE_STATE_PLATFORM,
            HIBERNATE_STATE_ENTER, HIBERNATE_STATE_RESUME,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_compression_modes_distinct() {
        let modes = [
            HIBERNATE_COMP_NONE, HIBERNATE_COMP_LZO,
            HIBERNATE_COMP_LZ4, HIBERNATE_COMP_ZSTD,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            HIBERNATE_F_PLATFORM, HIBERNATE_F_FILE,
            HIBERNATE_F_ENCRYPT, HIBERNATE_F_COMPRESS,
            HIBERNATE_F_RESUME_FILE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_page_flags_no_overlap() {
        let flags = [HIBERNATE_PF_DATA, HIBERNATE_PF_HIGHMEM, HIBERNATE_PF_DIRTY];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
