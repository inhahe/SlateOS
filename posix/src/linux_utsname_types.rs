//! `<sys/utsname.h>` — System identification constants.
//!
//! `uname()` returns system identification information in a
//! `struct utsname`.  These constants define the field sizes,
//! offsets, and the total structure size.

// ---------------------------------------------------------------------------
// Field sizes (bytes, including NUL terminator)
// ---------------------------------------------------------------------------

/// Length of each field in struct utsname (Linux uses 65 bytes).
pub const UTSNAME_FIELD_LEN: u32 = 65;
/// Length of the domainname field (Linux extension, also 65 bytes).
pub const UTSNAME_DOMAIN_LEN: u32 = 65;

// ---------------------------------------------------------------------------
// Field offsets in struct utsname (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of sysname (OS name, e.g. "Linux").
pub const UTSNAME_OFF_SYSNAME: u32 = 0;
/// Offset of nodename (network hostname).
pub const UTSNAME_OFF_NODENAME: u32 = 65;
/// Offset of release (kernel release string).
pub const UTSNAME_OFF_RELEASE: u32 = 130;
/// Offset of version (kernel version/build info).
pub const UTSNAME_OFF_VERSION: u32 = 195;
/// Offset of machine (hardware architecture, e.g. "x86_64").
pub const UTSNAME_OFF_MACHINE: u32 = 260;
/// Offset of domainname (NIS domain name, Linux extension).
pub const UTSNAME_OFF_DOMAINNAME: u32 = 325;

/// Total size of struct utsname on Linux (6 fields × 65 bytes).
pub const UTSNAME_SIZE: u32 = 390;

// ---------------------------------------------------------------------------
// Field indices (for array-style access)
// ---------------------------------------------------------------------------

/// Index of sysname field.
pub const UTSNAME_IDX_SYSNAME: u32 = 0;
/// Index of nodename field.
pub const UTSNAME_IDX_NODENAME: u32 = 1;
/// Index of release field.
pub const UTSNAME_IDX_RELEASE: u32 = 2;
/// Index of version field.
pub const UTSNAME_IDX_VERSION: u32 = 3;
/// Index of machine field.
pub const UTSNAME_IDX_MACHINE: u32 = 4;
/// Index of domainname field.
pub const UTSNAME_IDX_DOMAINNAME: u32 = 5;

/// Number of fields in struct utsname.
pub const UTSNAME_FIELD_COUNT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_len() {
        assert_eq!(UTSNAME_FIELD_LEN, 65);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            UTSNAME_OFF_SYSNAME, UTSNAME_OFF_NODENAME,
            UTSNAME_OFF_RELEASE, UTSNAME_OFF_VERSION,
            UTSNAME_OFF_MACHINE, UTSNAME_OFF_DOMAINNAME,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_are_field_multiples() {
        assert_eq!(UTSNAME_OFF_SYSNAME, 0 * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_OFF_NODENAME, 1 * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_OFF_RELEASE, 2 * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_OFF_VERSION, 3 * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_OFF_MACHINE, 4 * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_OFF_DOMAINNAME, 5 * UTSNAME_FIELD_LEN);
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(UTSNAME_SIZE, UTSNAME_FIELD_COUNT * UTSNAME_FIELD_LEN);
        assert_eq!(UTSNAME_SIZE, 390);
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(UTSNAME_OFF_DOMAINNAME < UTSNAME_SIZE);
    }

    #[test]
    fn test_indices_sequential() {
        assert_eq!(UTSNAME_IDX_SYSNAME, 0);
        assert_eq!(UTSNAME_IDX_DOMAINNAME, 5);
    }

    #[test]
    fn test_field_count() {
        assert_eq!(UTSNAME_FIELD_COUNT, 6);
    }

    #[test]
    fn test_domain_len_matches_field_len() {
        assert_eq!(UTSNAME_DOMAIN_LEN, UTSNAME_FIELD_LEN);
    }
}
