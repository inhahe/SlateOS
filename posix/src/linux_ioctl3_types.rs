//! `<linux/ioctl.h>` — Additional ioctl constants (part 3).
//!
//! Supplementary ioctl constants covering direction flags,
//! size encoding, and type/number macros.

// ---------------------------------------------------------------------------
// ioctl direction bits
// ---------------------------------------------------------------------------

/// No direction (command only).
pub const IOC_NONE: u32 = 0;
/// Write data to driver.
pub const IOC_WRITE: u32 = 1;
/// Read data from driver.
pub const IOC_READ: u32 = 2;

// ---------------------------------------------------------------------------
// ioctl number field widths
// ---------------------------------------------------------------------------

/// Number of bits in the NR field.
pub const IOC_NRBITS: u32 = 8;
/// Number of bits in the TYPE field.
pub const IOC_TYPEBITS: u32 = 8;
/// Number of bits in the SIZE field.
pub const IOC_SIZEBITS: u32 = 14;
/// Number of bits in the DIR field.
pub const IOC_DIRBITS: u32 = 2;

// ---------------------------------------------------------------------------
// ioctl field masks
// ---------------------------------------------------------------------------

/// NR field mask.
pub const IOC_NRMASK: u32 = (1 << IOC_NRBITS) - 1;
/// TYPE field mask.
pub const IOC_TYPEMASK: u32 = (1 << IOC_TYPEBITS) - 1;
/// SIZE field mask.
pub const IOC_SIZEMASK: u32 = (1 << IOC_SIZEBITS) - 1;
/// DIR field mask.
pub const IOC_DIRMASK: u32 = (1 << IOC_DIRBITS) - 1;

// ---------------------------------------------------------------------------
// ioctl field shifts
// ---------------------------------------------------------------------------

/// NR field shift.
pub const IOC_NRSHIFT: u32 = 0;
/// TYPE field shift.
pub const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
/// SIZE field shift.
pub const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
/// DIR field shift.
pub const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

// ---------------------------------------------------------------------------
// ioctl maximum size
// ---------------------------------------------------------------------------

/// Maximum ioctl data size.
pub const IOC_IN: u32 = IOC_WRITE << IOC_DIRSHIFT;
/// Maximum ioctl data size (out).
pub const IOC_OUT: u32 = IOC_READ << IOC_DIRSHIFT;
/// Maximum ioctl data size (in/out).
pub const IOC_INOUT: u32 = (IOC_WRITE | IOC_READ) << IOC_DIRSHIFT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directions_distinct() {
        let dirs = [IOC_NONE, IOC_WRITE, IOC_READ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_bit_widths_sum() {
        assert_eq!(
            IOC_NRBITS + IOC_TYPEBITS + IOC_SIZEBITS + IOC_DIRBITS,
            32
        );
    }

    #[test]
    fn test_shifts_ordered() {
        assert!(IOC_NRSHIFT < IOC_TYPESHIFT);
        assert!(IOC_TYPESHIFT < IOC_SIZESHIFT);
        assert!(IOC_SIZESHIFT < IOC_DIRSHIFT);
    }

    #[test]
    fn test_masks_width() {
        assert_eq!(IOC_NRMASK, 0xFF);
        assert_eq!(IOC_TYPEMASK, 0xFF);
        assert_eq!(IOC_DIRMASK, 0x3);
    }

    #[test]
    fn test_dir_macros_distinct() {
        let dirs = [IOC_IN, IOC_OUT, IOC_INOUT];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }
}
