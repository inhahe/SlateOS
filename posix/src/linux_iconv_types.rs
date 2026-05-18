//! `<iconv.h>` — Character set conversion constants.
//!
//! `iconv()` converts text between character encodings.  These
//! constants define common encoding IDs and control flags used
//! by the glibc iconv implementation.

// ---------------------------------------------------------------------------
// iconv() return values
// ---------------------------------------------------------------------------

/// Conversion error (returned by iconv on failure, cast from (size_t)-1).
pub const ICONV_ERROR: usize = usize::MAX;

// ---------------------------------------------------------------------------
// Common encoding identifiers (numeric IDs as used by glibc internals)
// ---------------------------------------------------------------------------

/// ASCII / US-ASCII encoding.
pub const ICONV_ENC_ASCII: u32 = 0;
/// UTF-8 encoding.
pub const ICONV_ENC_UTF8: u32 = 1;
/// UTF-16 little-endian encoding.
pub const ICONV_ENC_UTF16LE: u32 = 2;
/// UTF-16 big-endian encoding.
pub const ICONV_ENC_UTF16BE: u32 = 3;
/// UTF-32 little-endian encoding.
pub const ICONV_ENC_UTF32LE: u32 = 4;
/// UTF-32 big-endian encoding.
pub const ICONV_ENC_UTF32BE: u32 = 5;
/// ISO-8859-1 (Latin-1) encoding.
pub const ICONV_ENC_ISO8859_1: u32 = 6;
/// ISO-8859-15 (Latin-9) encoding.
pub const ICONV_ENC_ISO8859_15: u32 = 7;
/// Shift-JIS encoding (Japanese).
pub const ICONV_ENC_SJIS: u32 = 8;
/// EUC-JP encoding (Japanese).
pub const ICONV_ENC_EUCJP: u32 = 9;
/// EUC-KR encoding (Korean).
pub const ICONV_ENC_EUCKR: u32 = 10;
/// GB2312 encoding (Simplified Chinese).
pub const ICONV_ENC_GB2312: u32 = 11;
/// Big5 encoding (Traditional Chinese).
pub const ICONV_ENC_BIG5: u32 = 12;
/// KOI8-R encoding (Russian).
pub const ICONV_ENC_KOI8R: u32 = 13;
/// Windows-1252 encoding (Western European).
pub const ICONV_ENC_CP1252: u32 = 14;
/// Windows-1251 encoding (Cyrillic).
pub const ICONV_ENC_CP1251: u32 = 15;

// ---------------------------------------------------------------------------
// iconv_open() flags (glibc extensions via //suffix)
// ---------------------------------------------------------------------------

/// Transliterate characters that cannot be represented.
pub const ICONV_FLAG_TRANSLIT: u32 = 1 << 0;
/// Ignore characters that cannot be converted.
pub const ICONV_FLAG_IGNORE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_is_max() {
        assert_eq!(ICONV_ERROR, usize::MAX);
    }

    #[test]
    fn test_encodings_distinct() {
        let encs = [
            ICONV_ENC_ASCII, ICONV_ENC_UTF8, ICONV_ENC_UTF16LE,
            ICONV_ENC_UTF16BE, ICONV_ENC_UTF32LE, ICONV_ENC_UTF32BE,
            ICONV_ENC_ISO8859_1, ICONV_ENC_ISO8859_15,
            ICONV_ENC_SJIS, ICONV_ENC_EUCJP, ICONV_ENC_EUCKR,
            ICONV_ENC_GB2312, ICONV_ENC_BIG5, ICONV_ENC_KOI8R,
            ICONV_ENC_CP1252, ICONV_ENC_CP1251,
        ];
        for i in 0..encs.len() {
            for j in (i + 1)..encs.len() {
                assert_ne!(encs[i], encs[j]);
            }
        }
    }

    #[test]
    fn test_ascii_is_zero() {
        assert_eq!(ICONV_ENC_ASCII, 0);
    }

    #[test]
    fn test_utf8_is_one() {
        assert_eq!(ICONV_ENC_UTF8, 1);
    }

    #[test]
    fn test_flags_powers_of_two() {
        assert!(ICONV_FLAG_TRANSLIT.is_power_of_two());
        assert!(ICONV_FLAG_IGNORE.is_power_of_two());
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(ICONV_FLAG_TRANSLIT & ICONV_FLAG_IGNORE, 0);
    }
}
