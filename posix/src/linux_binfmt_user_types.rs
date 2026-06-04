//! `binfmt_misc` registration / control surface.
//!
//! Linux exposes a wildcard binary-format handler at
//! `/proc/sys/fs/binfmt_misc/`. Userspace writes a colon-delimited
//! rule string to `register`; the kernel parses it and adds a new
//! entry that maps a magic/extension match to an interpreter.

// ---------------------------------------------------------------------------
// procfs control file paths
// ---------------------------------------------------------------------------

pub const PROC_BINFMT_MISC_REGISTER: &str = "/proc/sys/fs/binfmt_misc/register";
pub const PROC_BINFMT_MISC_STATUS: &str = "/proc/sys/fs/binfmt_misc/status";
pub const PROC_BINFMT_MISC: &str = "/proc/sys/fs/binfmt_misc";

// ---------------------------------------------------------------------------
// Registration rule field separator
// ---------------------------------------------------------------------------

/// Default field separator used in registration strings (e.g. `:M::magic:`).
pub const BINFMT_MISC_DEFAULT_DELIM: u8 = b':';

/// Maximum length of a `name`, `magic`, or `interpreter` field per
/// kernel uapi.
pub const BINFMT_MISC_FIELD_MAX: usize = 4096;

// ---------------------------------------------------------------------------
// Rule-type tags (first non-empty token of a rule)
// ---------------------------------------------------------------------------

/// `:name:E::ext::/path/to/interp::flags` — match by file extension.
pub const BINFMT_MISC_TYPE_EXT: u8 = b'E';
/// `:name:M:offset:magic:mask:/path/to/interp:flags` — match by magic.
pub const BINFMT_MISC_TYPE_MAGIC: u8 = b'M';

// ---------------------------------------------------------------------------
// Flag characters (final field of a rule)
// ---------------------------------------------------------------------------

/// `P` — preserve argv[0].
pub const BINFMT_MISC_FLAG_P: u8 = b'P';
/// `O` — open the binary for reading (kernel passes the fd).
pub const BINFMT_MISC_FLAG_O: u8 = b'O';
/// `C` — credentials are recomputed from the interpreter (not the binary).
pub const BINFMT_MISC_FLAG_C: u8 = b'C';
/// `F` — fix-binary: open and pin the interpreter at registration time.
pub const BINFMT_MISC_FLAG_F: u8 = b'F';

// ---------------------------------------------------------------------------
// Per-entry control file names (sysfs-style under the entry's directory)
// ---------------------------------------------------------------------------

pub const BINFMT_MISC_ENTRY_ENABLE: &[u8] = b"1";
pub const BINFMT_MISC_ENTRY_DISABLE: &[u8] = b"0";
pub const BINFMT_MISC_ENTRY_REMOVE: &[u8] = b"-1";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_procfs_paths_consistent() {
        assert_eq!(PROC_BINFMT_MISC, "/proc/sys/fs/binfmt_misc");
        assert!(
            PROC_BINFMT_MISC_REGISTER.starts_with(PROC_BINFMT_MISC)
        );
        assert!(PROC_BINFMT_MISC_STATUS.starts_with(PROC_BINFMT_MISC));
        assert!(PROC_BINFMT_MISC_REGISTER.ends_with("/register"));
        assert!(PROC_BINFMT_MISC_STATUS.ends_with("/status"));
    }

    #[test]
    fn test_default_delimiter_is_colon() {
        assert_eq!(BINFMT_MISC_DEFAULT_DELIM, b':');
        assert_eq!(BINFMT_MISC_DEFAULT_DELIM, 0x3A);
    }

    #[test]
    fn test_field_max_is_4096() {
        assert_eq!(BINFMT_MISC_FIELD_MAX, 4096);
        assert!(BINFMT_MISC_FIELD_MAX.is_power_of_two());
    }

    #[test]
    fn test_rule_type_tags_distinct_ascii() {
        assert_eq!(BINFMT_MISC_TYPE_EXT, b'E');
        assert_eq!(BINFMT_MISC_TYPE_MAGIC, b'M');
        assert_ne!(BINFMT_MISC_TYPE_EXT, BINFMT_MISC_TYPE_MAGIC);
        // Both are uppercase ASCII letters.
        assert!(BINFMT_MISC_TYPE_EXT.is_ascii_uppercase());
        assert!(BINFMT_MISC_TYPE_MAGIC.is_ascii_uppercase());
    }

    #[test]
    fn test_flag_letters_distinct_uppercase() {
        let f = [
            BINFMT_MISC_FLAG_P,
            BINFMT_MISC_FLAG_O,
            BINFMT_MISC_FLAG_C,
            BINFMT_MISC_FLAG_F,
        ];
        for &v in &f {
            assert!(v.is_ascii_uppercase());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_entry_control_byte_strings() {
        // Userspace writes "1" / "0" / "-1" to toggle entries.
        assert_eq!(BINFMT_MISC_ENTRY_ENABLE, b"1");
        assert_eq!(BINFMT_MISC_ENTRY_DISABLE, b"0");
        assert_eq!(BINFMT_MISC_ENTRY_REMOVE, b"-1");
        // The negative-one removal string is two bytes; the
        // single-digit toggles are one byte each.
        assert_eq!(BINFMT_MISC_ENTRY_ENABLE.len(), 1);
        assert_eq!(BINFMT_MISC_ENTRY_DISABLE.len(), 1);
        assert_eq!(BINFMT_MISC_ENTRY_REMOVE.len(), 2);
    }
}
