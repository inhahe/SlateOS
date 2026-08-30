//! `<linux/genwqe/genwqe_card.h>` — GenWQE accelerator card constants.
//!
//! Constants for the IBM GenWQE (Generic Work Queue Engine) PCIe
//! accelerator card userspace interface. Used by zlib-style compression
//! offload on POWER hosts.

// ---------------------------------------------------------------------------
// GenWQE card capability flags
// ---------------------------------------------------------------------------

/// Card supports zlib compression.
pub const GENWQE_CARD_TYPE_GENWQE5: u32 = 0;
/// Card type 4 (zEDC).
pub const GENWQE_CARD_TYPE_GENWQE4: u32 = 1;

// ---------------------------------------------------------------------------
// File-handle flags (struct genwqe_file.flags)
// ---------------------------------------------------------------------------

/// Card is busy (in flush state).
pub const GENWQE_FLAG_MSI_ENABLED: u32 = 0x0001;
/// IRQ active.
pub const GENWQE_FLAG_IRQ_ENABLED: u32 = 0x0002;

// ---------------------------------------------------------------------------
// DDCB (Device Driver Control Block) error codes
// ---------------------------------------------------------------------------

/// DDCB completed OK.
pub const DDCB_OK: u32 = 0x00;
/// Hardware reported execution error.
pub const DDCB_ERR_EXEC: u32 = 0x01;
/// Hardware error invariant.
pub const DDCB_ERR_INVAL: u32 = 0x02;
/// Hardware reported timeout.
pub const DDCB_ERR_TIMEOUT: u32 = 0x03;
/// Card was reset while the DDCB was in flight.
pub const DDCB_ERR_RESET: u32 = 0x04;
/// Pagefault during DMA.
pub const DDCB_ERR_PAGEFAULT: u32 = 0x05;

// ---------------------------------------------------------------------------
// Application IDs
// ---------------------------------------------------------------------------

/// Application ID for zlib operations.
pub const GENWQE_APPL_ID_GZIP: u32 = 0x4750_0000;
/// Application ID for ECRC verification.
pub const GENWQE_APPL_ID_ECRC: u32 = 0x4543_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_types_distinct() {
        assert_ne!(GENWQE_CARD_TYPE_GENWQE5, GENWQE_CARD_TYPE_GENWQE4);
    }

    #[test]
    fn test_file_flags_distinct_bits() {
        assert!(GENWQE_FLAG_MSI_ENABLED.is_power_of_two());
        assert!(GENWQE_FLAG_IRQ_ENABLED.is_power_of_two());
        assert_ne!(GENWQE_FLAG_MSI_ENABLED, GENWQE_FLAG_IRQ_ENABLED);
    }

    #[test]
    fn test_ddcb_codes_distinct() {
        let codes = [
            DDCB_OK,
            DDCB_ERR_EXEC,
            DDCB_ERR_INVAL,
            DDCB_ERR_TIMEOUT,
            DDCB_ERR_RESET,
            DDCB_ERR_PAGEFAULT,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
        assert_eq!(DDCB_OK, 0);
    }

    #[test]
    fn test_appl_ids_distinct() {
        assert_ne!(GENWQE_APPL_ID_GZIP, GENWQE_APPL_ID_ECRC);
        // Application IDs use the upper half of the 32-bit word — verify
        // the low 16 bits are reserved-zero.
        assert_eq!(GENWQE_APPL_ID_GZIP & 0xFFFF, 0);
        assert_eq!(GENWQE_APPL_ID_ECRC & 0xFFFF, 0);
    }
}
