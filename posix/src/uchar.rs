//! `<uchar.h>` — Unicode character conversion.
//!
//! Provides `c16rtomb`, `mbrtoc16`, `c32rtomb`, `mbrtoc32` for
//! converting between multibyte (UTF-8) sequences and fixed-width
//! Unicode code points (char16_t / char32_t).
//!
//! ## Implementation
//!
//! Since our locale uses ANSI_X3.4-1968 (ASCII), multibyte sequences
//! are single-byte ASCII characters.  These functions handle the
//! ASCII subset correctly and reject anything outside 0x00–0x7F.
//!
//! The `mbstate_t` conversion state is currently unused (ASCII has no
//! multi-byte sequences), but the functions accept and ignore it for
//! API compatibility.

use crate::errno;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `char16_t` — 16-bit character type (UTF-16 code unit).
pub type Char16T = u16;

/// `char32_t` — 32-bit character type (Unicode code point).
pub type Char32T = u32;

/// `mbstate_t` — multibyte conversion state.
///
/// For ASCII/C locale this is unused; we keep a single byte to track
/// partial state (always zero in practice).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MbstateT {
    _state: [u8; 8],
}

impl MbstateT {
    /// Create a zeroed (initial) conversion state.
    pub const fn new() -> Self {
        Self { _state: [0; 8] }
    }
}

impl Default for MbstateT {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// c32rtomb — char32_t → multibyte
// ---------------------------------------------------------------------------

/// Convert a 32-bit character to a multibyte sequence.
///
/// Writes the multibyte representation of `c32` into `s` (which must
/// have room for at least `MB_CUR_MAX` bytes).
///
/// Returns the number of bytes written, or `(size_t)(-1)` on error
/// (EILSEQ).
///
/// If `s` is null, resets the conversion state and returns 1 (the
/// length of the NUL character in the C locale).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn c32rtomb(s: *mut u8, c32: Char32T, _ps: *mut MbstateT) -> usize {
    if s.is_null() {
        // Reset state; return length of NUL in multibyte encoding (1).
        return 1;
    }

    // In our ASCII locale, only 0x00–0x7F are valid.
    if c32 > 0x7F {
        errno::set_errno(errno::EILSEQ);
        return usize::MAX; // (size_t)(-1)
    }

    // SAFETY: caller guarantees s has room for at least 1 byte.
    unsafe {
        *s = c32 as u8;
    }
    1
}

// ---------------------------------------------------------------------------
// mbrtoc32 — multibyte → char32_t
// ---------------------------------------------------------------------------

/// Convert a multibyte sequence to a 32-bit character.
///
/// Examines up to `n` bytes from `s` to form a complete multibyte
/// character, storing the result in `*pc32`.
///
/// Returns:
/// - 0 if the character is NUL
/// - 1..n for a valid character (number of bytes consumed)
/// - `(size_t)(-1)` for an invalid sequence (EILSEQ)
/// - `(size_t)(-2)` for an incomplete sequence
///
/// If `s` is null, equivalent to `mbrtoc32(NULL, "", 1, ps)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mbrtoc32(
    pc32: *mut Char32T,
    s: *const u8,
    n: usize,
    _ps: *mut MbstateT,
) -> usize {
    if s.is_null() {
        // Reset state; equivalent to mbrtoc32(NULL, "", 1, ps).
        return 0;
    }

    if n == 0 {
        // Incomplete sequence (no bytes to examine).
        return usize::MAX - 1; // (size_t)(-2)
    }

    // SAFETY: s is non-null and n >= 1.
    let byte = unsafe { *s };

    if byte > 0x7F {
        errno::set_errno(errno::EILSEQ);
        return usize::MAX; // (size_t)(-1)
    }

    if !pc32.is_null() {
        // SAFETY: pc32 is non-null.
        unsafe {
            *pc32 = Char32T::from(byte);
        }
    }

    // POSIX: return 0 on NUL byte, else 1 (one byte consumed).
    usize::from(byte != 0)
}

// ---------------------------------------------------------------------------
// c16rtomb — char16_t → multibyte
// ---------------------------------------------------------------------------

/// Convert a 16-bit character to a multibyte sequence.
///
/// Same semantics as `c32rtomb` but for `char16_t`.  Surrogate pairs
/// (0xD800–0xDFFF) are rejected with EILSEQ.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn c16rtomb(s: *mut u8, c16: Char16T, _ps: *mut MbstateT) -> usize {
    if s.is_null() {
        return 1;
    }

    // Reject surrogates and non-ASCII in our locale.
    if c16 > 0x7F {
        errno::set_errno(errno::EILSEQ);
        return usize::MAX;
    }

    // SAFETY: caller guarantees s has room.
    unsafe {
        *s = c16 as u8;
    }
    1
}

// ---------------------------------------------------------------------------
// mbrtoc16 — multibyte → char16_t
// ---------------------------------------------------------------------------

/// Convert a multibyte sequence to a 16-bit character.
///
/// Same semantics as `mbrtoc32` but stores into `*pc16`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mbrtoc16(
    pc16: *mut Char16T,
    s: *const u8,
    n: usize,
    _ps: *mut MbstateT,
) -> usize {
    if s.is_null() {
        return 0;
    }

    if n == 0 {
        return usize::MAX - 1; // (size_t)(-2)
    }

    let byte = unsafe { *s };

    if byte > 0x7F {
        errno::set_errno(errno::EILSEQ);
        return usize::MAX;
    }

    if !pc16.is_null() {
        unsafe {
            *pc16 = Char16T::from(byte);
        }
    }

    usize::from(byte != 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    #[test]
    fn test_char16_t_size() {
        assert_eq!(core::mem::size_of::<Char16T>(), 2);
    }

    #[test]
    fn test_char32_t_size() {
        assert_eq!(core::mem::size_of::<Char32T>(), 4);
    }

    #[test]
    fn test_mbstate_t_size() {
        assert_eq!(core::mem::size_of::<MbstateT>(), 8);
    }

    #[test]
    fn test_mbstate_t_new() {
        let st = MbstateT::new();
        for &b in &st._state {
            assert_eq!(b, 0);
        }
    }

    // -----------------------------------------------------------------------
    // c32rtomb
    // -----------------------------------------------------------------------

    #[test]
    fn test_c32rtomb_ascii() {
        let mut buf = [0u8; 4];
        let mut st = MbstateT::new();
        let ret = c32rtomb(buf.as_mut_ptr(), b'A' as u32, &mut st);
        assert_eq!(ret, 1);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn test_c32rtomb_nul() {
        let mut buf = [0xFFu8; 4];
        let ret = c32rtomb(buf.as_mut_ptr(), 0, core::ptr::null_mut());
        assert_eq!(ret, 1);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_c32rtomb_all_ascii() {
        let mut buf = [0u8; 1];
        for c in 0..=0x7Fu32 {
            let ret = c32rtomb(buf.as_mut_ptr(), c, core::ptr::null_mut());
            assert_eq!(ret, 1);
            assert_eq!(buf[0], c as u8);
        }
    }

    #[test]
    fn test_c32rtomb_non_ascii_eilseq() {
        let mut buf = [0u8; 4];
        errno::set_errno(0);
        let ret = c32rtomb(buf.as_mut_ptr(), 0x80, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX);
        assert_eq!(errno::get_errno(), errno::EILSEQ);
    }

    #[test]
    fn test_c32rtomb_high_codepoint_eilseq() {
        let mut buf = [0u8; 4];
        let ret = c32rtomb(buf.as_mut_ptr(), 0x1F600, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX);
    }

    #[test]
    fn test_c32rtomb_null_s_resets() {
        let ret = c32rtomb(core::ptr::null_mut(), b'X' as u32, core::ptr::null_mut());
        assert_eq!(ret, 1); // length of NUL in encoding
    }

    // -----------------------------------------------------------------------
    // mbrtoc32
    // -----------------------------------------------------------------------

    #[test]
    fn test_mbrtoc32_ascii() {
        let s = b"H";
        let mut c32: Char32T = 0;
        let ret = mbrtoc32(&mut c32, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, 1);
        assert_eq!(c32, b'H' as u32);
    }

    #[test]
    fn test_mbrtoc32_nul() {
        let s = b"\0";
        let mut c32: Char32T = 0xFF;
        let ret = mbrtoc32(&mut c32, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(c32, 0);
    }

    #[test]
    fn test_mbrtoc32_non_ascii_eilseq() {
        let s = [0x80u8];
        let mut c32: Char32T = 0;
        errno::set_errno(0);
        let ret = mbrtoc32(&mut c32, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX); // (size_t)(-1)
        assert_eq!(errno::get_errno(), errno::EILSEQ);
    }

    #[test]
    fn test_mbrtoc32_zero_n_incomplete() {
        let s = b"A";
        let mut c32: Char32T = 0;
        let ret = mbrtoc32(&mut c32, s.as_ptr(), 0, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX - 1); // (size_t)(-2)
    }

    #[test]
    fn test_mbrtoc32_null_s_resets() {
        let ret = mbrtoc32(
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_mbrtoc32_null_pc32() {
        let s = b"Z";
        let ret = mbrtoc32(core::ptr::null_mut(), s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, 1); // still consumes the byte
    }

    // -----------------------------------------------------------------------
    // c16rtomb
    // -----------------------------------------------------------------------

    #[test]
    fn test_c16rtomb_ascii() {
        let mut buf = [0u8; 2];
        let ret = c16rtomb(buf.as_mut_ptr(), b'x' as u16, core::ptr::null_mut());
        assert_eq!(ret, 1);
        assert_eq!(buf[0], b'x');
    }

    #[test]
    fn test_c16rtomb_nul() {
        let mut buf = [0xFFu8; 2];
        let ret = c16rtomb(buf.as_mut_ptr(), 0, core::ptr::null_mut());
        assert_eq!(ret, 1);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_c16rtomb_non_ascii_eilseq() {
        let mut buf = [0u8; 2];
        errno::set_errno(0);
        let ret = c16rtomb(buf.as_mut_ptr(), 0x0100, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX);
        assert_eq!(errno::get_errno(), errno::EILSEQ);
    }

    #[test]
    fn test_c16rtomb_surrogate_eilseq() {
        let mut buf = [0u8; 2];
        // High surrogate 0xD800.
        let ret = c16rtomb(buf.as_mut_ptr(), 0xD800, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX);
    }

    #[test]
    fn test_c16rtomb_null_s_resets() {
        let ret = c16rtomb(core::ptr::null_mut(), b'A' as u16, core::ptr::null_mut());
        assert_eq!(ret, 1);
    }

    // -----------------------------------------------------------------------
    // mbrtoc16
    // -----------------------------------------------------------------------

    #[test]
    fn test_mbrtoc16_ascii() {
        let s = b"Q";
        let mut c16: Char16T = 0;
        let ret = mbrtoc16(&mut c16, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, 1);
        assert_eq!(c16, b'Q' as u16);
    }

    #[test]
    fn test_mbrtoc16_nul() {
        let s = b"\0";
        let mut c16: Char16T = 0xFF;
        let ret = mbrtoc16(&mut c16, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(c16, 0);
    }

    #[test]
    fn test_mbrtoc16_non_ascii_eilseq() {
        let s = [0xC0u8]; // invalid lead byte
        let mut c16: Char16T = 0;
        errno::set_errno(0);
        let ret = mbrtoc16(&mut c16, s.as_ptr(), 1, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX);
        assert_eq!(errno::get_errno(), errno::EILSEQ);
    }

    #[test]
    fn test_mbrtoc16_zero_n_incomplete() {
        let s = b"A";
        let ret = mbrtoc16(core::ptr::null_mut(), s.as_ptr(), 0, core::ptr::null_mut());
        assert_eq!(ret, usize::MAX - 1);
    }

    #[test]
    fn test_mbrtoc16_null_s_resets() {
        let ret = mbrtoc16(
            core::ptr::null_mut(),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // Round-trip: c32rtomb → mbrtoc32
    // -----------------------------------------------------------------------

    #[test]
    fn test_roundtrip_c32() {
        for c in 0..=0x7Fu32 {
            let mut buf = [0u8; 4];
            let n = c32rtomb(buf.as_mut_ptr(), c, core::ptr::null_mut());
            assert_eq!(n, 1);

            let mut out: Char32T = 0xDEAD;
            let m = mbrtoc32(&mut out, buf.as_ptr(), n, core::ptr::null_mut());
            if c == 0 {
                assert_eq!(m, 0);
            } else {
                assert_eq!(m, 1);
            }
            assert_eq!(out, c);
        }
    }

    // -----------------------------------------------------------------------
    // Round-trip: c16rtomb → mbrtoc16
    // -----------------------------------------------------------------------

    #[test]
    fn test_roundtrip_c16() {
        for c in 0..=0x7Fu16 {
            let mut buf = [0u8; 2];
            let n = c16rtomb(buf.as_mut_ptr(), c, core::ptr::null_mut());
            assert_eq!(n, 1);

            let mut out: Char16T = 0xDEAD;
            let m = mbrtoc16(&mut out, buf.as_ptr(), n, core::ptr::null_mut());
            if c == 0 {
                assert_eq!(m, 0);
            } else {
                assert_eq!(m, 1);
            }
            assert_eq!(out, c);
        }
    }
}
