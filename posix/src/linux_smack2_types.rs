//! `<linux/smack.h>` — Additional SMACK LSM constants.
//!
//! Supplementary SMACK constants covering access modes,
//! label lengths, and CIPSO/CALIPSO configuration.

// ---------------------------------------------------------------------------
// SMACK access modes
// ---------------------------------------------------------------------------

/// Read access.
pub const MAY_READ: u32 = 0x01;
/// Write access.
pub const MAY_WRITE: u32 = 0x02;
/// Execute access.
pub const MAY_EXEC: u32 = 0x04;
/// Append access.
pub const MAY_APPEND: u32 = 0x08;
/// Transmute access.
pub const MAY_TRANSMUTE: u32 = 0x10;
/// Lock access.
pub const MAY_LOCK: u32 = 0x20;
/// Bring-up access.
pub const MAY_BRINGUP: u32 = 0x40;

// ---------------------------------------------------------------------------
// SMACK label lengths
// ---------------------------------------------------------------------------

/// Maximum SMACK label length.
pub const SMACK_LABEL_LEN_MAX: u32 = 255;
/// Minimum SMACK label length.
pub const SMACK_LABEL_LEN_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// SMACK special labels
// ---------------------------------------------------------------------------

/// Floor label ("_").
pub const SMACK_LABEL_FLOOR: u8 = b'_';
/// Star label ("*").
pub const SMACK_LABEL_STAR: u8 = b'*';
/// Hat label ("^").
pub const SMACK_LABEL_HAT: u8 = b'^';
/// Web label ("@").
pub const SMACK_LABEL_WEB: u8 = b'@';

// ---------------------------------------------------------------------------
// SMACK CIPSO constants
// ---------------------------------------------------------------------------

/// Maximum CIPSO domain length.
pub const SMACK_CIPSO_MAXDOMAIN: u32 = 255;
/// Maximum CIPSO level.
pub const SMACK_CIPSO_MAXLEVEL: u32 = 255;
/// Maximum CIPSO categories.
pub const SMACK_CIPSO_MAXCATNUM: u32 = 239;
/// CIPSO option tag for SMACK.
pub const SMACK_CIPSO_DOI_DEFAULT: u32 = 3;

// ---------------------------------------------------------------------------
// SMACK magic numbers (smackfs)
// ---------------------------------------------------------------------------

/// smackfs magic number.
pub const SMACK_MAGIC: u32 = 0x43415D53;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_modes_power_of_two() {
        let modes = [
            MAY_READ, MAY_WRITE, MAY_EXEC, MAY_APPEND,
            MAY_TRANSMUTE, MAY_LOCK, MAY_BRINGUP,
        ];
        for m in &modes {
            assert!(m.is_power_of_two(), "0x{:02x} not power of two", m);
        }
    }

    #[test]
    fn test_access_modes_no_overlap() {
        let modes = [
            MAY_READ, MAY_WRITE, MAY_EXEC, MAY_APPEND,
            MAY_TRANSMUTE, MAY_LOCK, MAY_BRINGUP,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_label_lengths() {
        assert_eq!(SMACK_LABEL_LEN_MIN, 1);
        assert_eq!(SMACK_LABEL_LEN_MAX, 255);
        assert!(SMACK_LABEL_LEN_MIN < SMACK_LABEL_LEN_MAX);
    }

    #[test]
    fn test_special_labels_distinct() {
        let labels = [
            SMACK_LABEL_FLOOR, SMACK_LABEL_STAR,
            SMACK_LABEL_HAT, SMACK_LABEL_WEB,
        ];
        for i in 0..labels.len() {
            for j in (i + 1)..labels.len() {
                assert_ne!(labels[i], labels[j]);
            }
        }
    }

    #[test]
    fn test_cipso_bounds() {
        assert_eq!(SMACK_CIPSO_MAXDOMAIN, 255);
        assert_eq!(SMACK_CIPSO_MAXLEVEL, 255);
        assert!(SMACK_CIPSO_MAXCATNUM < SMACK_CIPSO_MAXLEVEL);
    }

    #[test]
    fn test_magic() {
        assert_eq!(SMACK_MAGIC, 0x43415D53);
    }
}
