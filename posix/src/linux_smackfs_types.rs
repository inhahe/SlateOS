//! SMACK LSM filesystem interface constants.
//!
//! Constants for the SMACK (Simplified Mandatory Access Control
//! Kernel) Linux Security Module's `smackfs` interface, mounted at
//! `/sys/fs/smackfs`. Userspace policy tools (`smackload`, `chsmack`)
//! consume these.

// ---------------------------------------------------------------------------
// Special SMACK labels
// ---------------------------------------------------------------------------

/// Floor label — least privileged, matches anything.
pub const SMACK_LABEL_FLOOR: &str = "_";
/// Star label — wildcard, matches anything in either direction.
pub const SMACK_LABEL_STAR: &str = "*";
/// Hat label — most privileged.
pub const SMACK_LABEL_HAT: &str = "^";
/// Huh label — placeholder for unknown subject.
pub const SMACK_LABEL_HUH: &str = "?";
/// Web label — long-form web origin label.
pub const SMACK_LABEL_WEB: &str = "@";

// ---------------------------------------------------------------------------
// Label length limits
// ---------------------------------------------------------------------------

/// Maximum length of a SMACK label (bytes, NUL-terminated).
pub const SMACK_LABEL_LEN: u32 = 256;
/// Long-form label maximum (older smackfs path).
pub const SMACK_LONGLABEL: u32 = 256;

// ---------------------------------------------------------------------------
// Access-rule mode characters (in /smack/load2 rule strings)
// ---------------------------------------------------------------------------

/// "r" — read.
pub const SMACK_ACC_R: u8 = b'r';
/// "w" — write.
pub const SMACK_ACC_W: u8 = b'w';
/// "x" — execute.
pub const SMACK_ACC_X: u8 = b'x';
/// "a" — append.
pub const SMACK_ACC_A: u8 = b'a';
/// "t" — transmute (label propagates to created object).
pub const SMACK_ACC_T: u8 = b't';
/// "l" — lock (POSIX locks).
pub const SMACK_ACC_L: u8 = b'l';
/// "b" — bring up (audit-only mode).
pub const SMACK_ACC_B: u8 = b'b';
/// "-" — placeholder for absent permission in a rule string.
pub const SMACK_ACC_NONE: u8 = b'-';

// ---------------------------------------------------------------------------
// CIPSO option ranges (for /smack/cipso)
// ---------------------------------------------------------------------------

/// Maximum CIPSO category number.
pub const SMACK_CIPSO_MAXCATNUM: u32 = 240;
/// Maximum CIPSO level.
pub const SMACK_CIPSO_MAXLEVEL: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_labels_single_char() {
        // The five special labels are all single ASCII chars so they
        // can never collide with a regular textual label.
        for s in [
            SMACK_LABEL_FLOOR,
            SMACK_LABEL_STAR,
            SMACK_LABEL_HAT,
            SMACK_LABEL_HUH,
            SMACK_LABEL_WEB,
        ] {
            assert_eq!(s.len(), 1);
            assert!(s.is_ascii());
        }
    }

    #[test]
    fn test_special_labels_distinct() {
        let lbls = [
            SMACK_LABEL_FLOOR,
            SMACK_LABEL_STAR,
            SMACK_LABEL_HAT,
            SMACK_LABEL_HUH,
            SMACK_LABEL_WEB,
        ];
        for i in 0..lbls.len() {
            for j in (i + 1)..lbls.len() {
                assert_ne!(lbls[i], lbls[j]);
            }
        }
    }

    #[test]
    fn test_label_len_reasonable() {
        // SMACK labels are short; 256 bytes is the documented cap and
        // both LABEL_LEN and LONGLABEL must agree.
        assert_eq!(SMACK_LABEL_LEN, 256);
        assert_eq!(SMACK_LONGLABEL, SMACK_LABEL_LEN);
    }

    #[test]
    fn test_access_chars_distinct() {
        let chars = [
            SMACK_ACC_R,
            SMACK_ACC_W,
            SMACK_ACC_X,
            SMACK_ACC_A,
            SMACK_ACC_T,
            SMACK_ACC_L,
            SMACK_ACC_B,
            SMACK_ACC_NONE,
        ];
        for &c in &chars {
            assert!(c.is_ascii());
        }
        for i in 0..chars.len() {
            for j in (i + 1)..chars.len() {
                assert_ne!(chars[i], chars[j]);
            }
        }
    }

    #[test]
    fn test_cipso_ranges_in_byte_space() {
        // Both CIPSO sub-fields must fit a single byte (the on-wire
        // CIPSO option encodes them as octets).
        assert!(SMACK_CIPSO_MAXCATNUM <= 255);
        assert!(SMACK_CIPSO_MAXLEVEL <= 255);
    }
}
