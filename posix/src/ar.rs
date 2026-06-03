//! `<ar.h>` — archive file format constants.
//!
//! Defines the magic string and header format constants for the
//! Unix archive format used by `ar(1)` and consumed by linkers.

// ---------------------------------------------------------------------------
// Magic string
// ---------------------------------------------------------------------------

/// Archive magic string at the beginning of every ar file.
///
/// "!<arch>\n" — exactly 8 bytes.
pub const ARMAG: &[u8] = b"!<arch>\n";

/// Length of `ARMAG`.
pub const SARMAG: usize = 8;

/// Magic string at the end of each member header.
///
/// "`\n" — the 2-byte fmag field.
pub const ARFMAG: &[u8] = b"`\n";

// ---------------------------------------------------------------------------
// Header field sizes
// ---------------------------------------------------------------------------

/// Size of the `ar_name` field (member name).
pub const AR_NAME_SIZE: usize = 16;

/// Size of the `ar_date` field (modification time, decimal seconds).
pub const AR_DATE_SIZE: usize = 12;

/// Size of the `ar_uid` field (owner user ID, decimal).
pub const AR_UID_SIZE: usize = 6;

/// Size of the `ar_gid` field (owner group ID, decimal).
pub const AR_GID_SIZE: usize = 6;

/// Size of the `ar_mode` field (file mode, octal).
pub const AR_MODE_SIZE: usize = 8;

/// Size of the `ar_size` field (file size in bytes, decimal).
pub const AR_SIZE_SIZE: usize = 10;

/// Size of the `ar_fmag` field (header magic).
pub const AR_FMAG_SIZE: usize = 2;

/// Total size of an archive member header.
///
/// ar_name(16) + ar_date(12) + ar_uid(6) + ar_gid(6) + ar_mode(8)
/// + ar_size(10) + ar_fmag(2) = 60 bytes.
pub const AR_HDR_SIZE: usize = AR_NAME_SIZE
    + AR_DATE_SIZE
    + AR_UID_SIZE
    + AR_GID_SIZE
    + AR_MODE_SIZE
    + AR_SIZE_SIZE
    + AR_FMAG_SIZE;

/// Archive member header (struct ar_hdr).
///
/// All fields are ASCII text, space-padded.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArHdr {
    /// Member name, terminated by '/' and padded with spaces.
    pub ar_name: [u8; AR_NAME_SIZE],
    /// Modification time (decimal seconds since epoch).
    pub ar_date: [u8; AR_DATE_SIZE],
    /// Owner UID (decimal ASCII).
    pub ar_uid: [u8; AR_UID_SIZE],
    /// Owner GID (decimal ASCII).
    pub ar_gid: [u8; AR_GID_SIZE],
    /// File mode (octal ASCII).
    pub ar_mode: [u8; AR_MODE_SIZE],
    /// File size in bytes (decimal ASCII).
    pub ar_size: [u8; AR_SIZE_SIZE],
    /// Header magic: "`\n".
    pub ar_fmag: [u8; AR_FMAG_SIZE],
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Magic strings
    // -----------------------------------------------------------------------

    #[test]
    fn test_armag() {
        assert_eq!(ARMAG, b"!<arch>\n");
    }

    #[test]
    fn test_sarmag() {
        assert_eq!(SARMAG, 8);
        assert_eq!(ARMAG.len(), SARMAG);
    }

    #[test]
    fn test_arfmag() {
        assert_eq!(ARFMAG, b"`\n");
        assert_eq!(ARFMAG.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Field sizes
    // -----------------------------------------------------------------------

    #[test]
    fn test_field_sizes() {
        assert_eq!(AR_NAME_SIZE, 16);
        assert_eq!(AR_DATE_SIZE, 12);
        assert_eq!(AR_UID_SIZE, 6);
        assert_eq!(AR_GID_SIZE, 6);
        assert_eq!(AR_MODE_SIZE, 8);
        assert_eq!(AR_SIZE_SIZE, 10);
        assert_eq!(AR_FMAG_SIZE, 2);
    }

    #[test]
    fn test_hdr_size() {
        assert_eq!(AR_HDR_SIZE, 60);
    }

    #[test]
    fn test_field_sizes_sum() {
        let sum = AR_NAME_SIZE
            + AR_DATE_SIZE
            + AR_UID_SIZE
            + AR_GID_SIZE
            + AR_MODE_SIZE
            + AR_SIZE_SIZE
            + AR_FMAG_SIZE;
        assert_eq!(sum, AR_HDR_SIZE);
    }

    // -----------------------------------------------------------------------
    // ArHdr struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_ar_hdr_size() {
        assert_eq!(core::mem::size_of::<ArHdr>(), AR_HDR_SIZE);
    }

    #[test]
    fn test_ar_hdr_alignment() {
        // Byte-aligned (all fields are byte arrays).
        assert_eq!(core::mem::align_of::<ArHdr>(), 1);
    }

    #[test]
    fn test_ar_hdr_zeroed() {
        let hdr = ArHdr {
            ar_name: [0; AR_NAME_SIZE],
            ar_date: [0; AR_DATE_SIZE],
            ar_uid: [0; AR_UID_SIZE],
            ar_gid: [0; AR_GID_SIZE],
            ar_mode: [0; AR_MODE_SIZE],
            ar_size: [0; AR_SIZE_SIZE],
            ar_fmag: [0; AR_FMAG_SIZE],
        };
        assert_eq!(hdr.ar_name[0], 0);
        assert_eq!(hdr.ar_fmag[0], 0);
    }

    #[test]
    fn test_ar_hdr_typical() {
        // A typical header for a file named "hello.o".
        let mut hdr = ArHdr {
            ar_name: [b' '; AR_NAME_SIZE],
            ar_date: [b' '; AR_DATE_SIZE],
            ar_uid: [b' '; AR_UID_SIZE],
            ar_gid: [b' '; AR_GID_SIZE],
            ar_mode: [b' '; AR_MODE_SIZE],
            ar_size: [b' '; AR_SIZE_SIZE],
            ar_fmag: [b'`', b'\n'],
        };
        // Write name "hello.o/"
        let name = b"hello.o/";
        hdr.ar_name[..name.len()].copy_from_slice(name);

        assert_eq!(&hdr.ar_name[..8], b"hello.o/");
        assert_eq!(&hdr.ar_fmag, ARFMAG);
    }

    // -----------------------------------------------------------------------
    // Magic recognition
    // -----------------------------------------------------------------------

    #[test]
    fn test_armag_starts_with_exclamation() {
        assert_eq!(ARMAG[0], b'!');
    }

    #[test]
    fn test_armag_ends_with_newline() {
        assert_eq!(ARMAG[SARMAG - 1], b'\n');
    }
}
