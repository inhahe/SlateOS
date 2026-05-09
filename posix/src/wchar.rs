//! Wide character and multibyte string stubs (`<wchar.h>`, `<wctype.h>`).
//!
//! Our OS uses UTF-8 throughout and doesn't need wide character support.
//! These stubs exist so programs that reference `<wchar.h>` or `<wctype.h>`
//! symbols can link.  Actual wide character processing should use UTF-8
//! natively.
//!
//! ## Implemented (stubs)
//!
//! - `mblen`, `mbtowc`, `wctomb` — multibyte ↔ wide character
//! - `mbstowcs`, `wcstombs` — multibyte ↔ wide string
//! - `wcwidth`, `wcswidth` — character/string display width
//! - `btowc`, `wctob` — byte ↔ wide character
//! - `mbsinit` — check initial shift state
//! - `iswctype`, `towlower`, `towupper` — wide character classification
//! - `iswalpha`, `iswdigit`, `iswalnum`, `iswspace`, `iswprint`,
//!   `iswupper`, `iswlower`, `iswpunct`, `iswcntrl`, `iswgraph`,
//!   `iswxdigit`, `iswblank` — wide ctype
//! - `wcscpy`, `wcsncpy`, `wcslen`, `wcscmp`, `wcsncmp`, `wcscat`,
//!   `wcschr`, `wcsrchr` — wide string operations
//! - `wmemcpy`, `wmemset`, `wmemcmp` — wide memory operations
//! - `mbrtowc`, `wcrtomb` — restartable multibyte conversion

/// Wide character type (32-bit Unicode code point).
pub type WcharT = i32;

/// Multibyte conversion state (opaque — we only support stateless UTF-8).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MbstateT {
    _opaque: [u8; 8],
}

// ---------------------------------------------------------------------------
// Multibyte ↔ wide character
// ---------------------------------------------------------------------------

/// Determine the number of bytes in a multibyte character.
///
/// For ASCII (our only encoding), returns 1 for valid characters,
/// 0 for null, -1 for invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mblen(s: *const u8, _n: usize) -> i32 {
    if s.is_null() {
        return 0; // No state-dependent encoding.
    }
    let c = unsafe { *s };
    i32::from(c != 0)
}

/// Convert a multibyte character to a wide character.
///
/// ASCII-only: copies the byte value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbtowc(pwc: *mut WcharT, s: *const u8, _n: usize) -> i32 {
    if s.is_null() {
        return 0;
    }
    let c = unsafe { *s };
    if c == 0 {
        if !pwc.is_null() {
            unsafe { *pwc = 0; }
        }
        return 0;
    }
    if !pwc.is_null() {
        unsafe { *pwc = WcharT::from(c); }
    }
    1
}

/// Convert a wide character to a multibyte character.
///
/// ASCII-only: stores the low byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wctomb(s: *mut u8, wc: WcharT) -> i32 {
    if s.is_null() {
        return 0;
    }
    if !(0..=127).contains(&wc) {
        return -1; // Not representable in ASCII.
    }
    unsafe { *s = wc as u8; }
    1
}

/// Convert a multibyte string to a wide string.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn mbstowcs(dst: *mut WcharT, src: *const u8, n: usize) -> usize {
    if src.is_null() {
        return 0;
    }
    let mut i: usize = 0;
    while i < n {
        let c = unsafe { *src.add(i) };
        if !dst.is_null() {
            unsafe { *dst.add(i) = WcharT::from(c); }
        }
        if c == 0 {
            return i;
        }
        i = i.wrapping_add(1);
    }
    i
}

/// Convert a wide string to a multibyte string.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstombs(dst: *mut u8, src: *const WcharT, n: usize) -> usize {
    if src.is_null() {
        return 0;
    }
    let mut i: usize = 0;
    while i < n {
        let wc = unsafe { *src.add(i) };
        if !(0..=127).contains(&wc) {
            return usize::MAX; // Error.
        }
        if !dst.is_null() {
            unsafe { *dst.add(i) = wc as u8; }
        }
        if wc == 0 {
            return i;
        }
        i = i.wrapping_add(1);
    }
    i
}

// ---------------------------------------------------------------------------
// Byte ↔ wide character
// ---------------------------------------------------------------------------

/// Convert a byte to a wide character.
#[unsafe(no_mangle)]
pub extern "C" fn btowc(c: i32) -> WcharT {
    if (0..=127).contains(&c) { c } else { -1 }
}

/// Convert a wide character to a byte.
#[unsafe(no_mangle)]
pub extern "C" fn wctob(wc: WcharT) -> i32 {
    if (0..=127).contains(&wc) { wc } else { -1 }
}

// ---------------------------------------------------------------------------
// Shift state
// ---------------------------------------------------------------------------

/// Check if `*ps` is the initial shift state.
///
/// Always returns 1 (we only support stateless encoding).
#[unsafe(no_mangle)]
pub extern "C" fn mbsinit(_ps: *const MbstateT) -> i32 {
    1 // Stateless.
}

// ---------------------------------------------------------------------------
// Restartable multibyte conversion
// ---------------------------------------------------------------------------

/// Restartable multibyte → wide character.
///
/// ASCII-only implementation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrtowc(
    pwc: *mut WcharT,
    s: *const u8,
    _n: usize,
    _ps: *mut MbstateT,
) -> usize {
    if s.is_null() {
        return 0;
    }
    let c = unsafe { *s };
    if c == 0 {
        if !pwc.is_null() {
            unsafe { *pwc = 0; }
        }
        return 0;
    }
    if !pwc.is_null() {
        unsafe { *pwc = WcharT::from(c); }
    }
    1
}

/// Restartable wide → multibyte character.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcrtomb(
    s: *mut u8,
    wc: WcharT,
    _ps: *mut MbstateT,
) -> usize {
    if s.is_null() {
        return 1; // Needed to reset state (no-op for stateless).
    }
    if !(0..=127).contains(&wc) {
        return usize::MAX; // Error.
    }
    unsafe { *s = wc as u8; }
    1
}

// ---------------------------------------------------------------------------
// Display width
// ---------------------------------------------------------------------------

/// Return the display width of a wide character.
///
/// Returns -1 for non-printable, 0 for null, 1 for printable ASCII,
/// 2 for CJK (basic heuristic using Unicode block ranges).
#[unsafe(no_mangle)]
pub extern "C" fn wcwidth(wc: WcharT) -> i32 {
    if wc == 0 {
        return 0;
    }
    if wc < 32 || wc == 0x7f {
        return -1; // Control character.
    }
    // CJK Unified Ideographs and common fullwidth ranges.
    #[allow(clippy::manual_range_contains)]
    if (wc >= 0x1100 && wc <= 0x115f)   // Hangul Jamo
        || (wc >= 0x2e80 && wc <= 0xa4cf && wc != 0x303f) // CJK
        || (wc >= 0xac00 && wc <= 0xd7a3) // Hangul Syllables
        || (wc >= 0xf900 && wc <= 0xfaff) // CJK Compat Ideographs
        || (wc >= 0xfe10 && wc <= 0xfe6f) // CJK forms
        || (wc >= 0xff01 && wc <= 0xff60) // Fullwidth forms
        || (wc >= 0xffe0 && wc <= 0xffe6) // Fullwidth signs
        || (wc >= 0x20000 && wc <= 0x2fffd) // CJK Extension B+
        || (wc >= 0x30000 && wc <= 0x3fffd) // CJK Extension G+
    {
        return 2;
    }
    1
}

/// Return the display width of a wide string.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcswidth(s: *const WcharT, n: usize) -> i32 {
    if s.is_null() {
        return -1;
    }
    let mut width: i32 = 0;
    let mut i: usize = 0;
    while i < n {
        let wc = unsafe { *s.add(i) };
        if wc == 0 {
            break;
        }
        let w = wcwidth(wc);
        if w < 0 {
            return -1;
        }
        width += w;
        i = i.wrapping_add(1);
    }
    width
}

// ---------------------------------------------------------------------------
// Wide character classification (wctype.h)
// ---------------------------------------------------------------------------

/// Check if wide character is alphanumeric.
#[unsafe(no_mangle)]
pub extern "C" fn iswalnum(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a))
}

/// Check if wide character is alphabetic.
#[unsafe(no_mangle)]
pub extern "C" fn iswalpha(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x41..=0x5a | 0x61..=0x7a))
}

/// Check if wide character is a digit.
#[unsafe(no_mangle)]
pub extern "C" fn iswdigit(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39))
}

/// Check if wide character is a hex digit.
#[unsafe(no_mangle)]
pub extern "C" fn iswxdigit(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66))
}

/// Check if wide character is whitespace.
#[unsafe(no_mangle)]
pub extern "C" fn iswspace(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x09..=0x0d | 0x20))
}

/// Check if wide character is a blank (space or tab).
#[unsafe(no_mangle)]
pub extern "C" fn iswblank(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x09 | 0x20))
}

/// Check if wide character is printable.
#[unsafe(no_mangle)]
pub extern "C" fn iswprint(wc: WcharT) -> i32 {
    i32::from(wc >= 0x20 && wc != 0x7f)
}

/// Check if wide character is a control character.
#[unsafe(no_mangle)]
pub extern "C" fn iswcntrl(wc: WcharT) -> i32 {
    i32::from(wc < 0x20 || wc == 0x7f)
}

/// Check if wide character is uppercase.
#[unsafe(no_mangle)]
pub extern "C" fn iswupper(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x41..=0x5a))
}

/// Check if wide character is lowercase.
#[unsafe(no_mangle)]
pub extern "C" fn iswlower(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x61..=0x7a))
}

/// Check if wide character is punctuation.
#[unsafe(no_mangle)]
pub extern "C" fn iswpunct(wc: WcharT) -> i32 {
    i32::from(iswprint(wc) != 0 && iswspace(wc) == 0 && iswalnum(wc) == 0)
}

/// Check if wide character is a graph character (printable, not space).
#[unsafe(no_mangle)]
pub extern "C" fn iswgraph(wc: WcharT) -> i32 {
    i32::from(iswprint(wc) != 0 && wc != 0x20)
}

/// Convert wide character to lowercase.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn towlower(wc: WcharT) -> WcharT {
    if iswupper(wc) != 0 { wc + 32 } else { wc }
}

/// Convert wide character to uppercase.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn towupper(wc: WcharT) -> WcharT {
    if iswlower(wc) != 0 { wc - 32 } else { wc }
}

// ---------------------------------------------------------------------------
// Wide string operations
// ---------------------------------------------------------------------------

/// Copy a wide string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscpy(dst: *mut WcharT, src: *const WcharT) -> *mut WcharT {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *src.add(i) };
        unsafe { *dst.add(i) = c; }
        if c == 0 {
            return dst;
        }
        i = i.wrapping_add(1);
    }
}

/// Copy at most `n` wide characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncpy(
    dst: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    let mut i: usize = 0;
    let mut done = false;
    while i < n {
        if done {
            unsafe { *dst.add(i) = 0; }
        } else {
            let c = unsafe { *src.add(i) };
            unsafe { *dst.add(i) = c; }
            if c == 0 {
                done = true;
            }
        }
        i = i.wrapping_add(1);
    }
    dst
}

/// Return the length of a wide string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcslen(s: *const WcharT) -> usize {
    let mut i: usize = 0;
    while unsafe { *s.add(i) } != 0 {
        i = i.wrapping_add(1);
    }
    i
}

/// Compare two wide strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscmp(s1: *const WcharT, s2: *const WcharT) -> i32 {
    let mut i: usize = 0;
    loop {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b || a == 0 {
            return match a.cmp(&b) {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Greater => 1,
                core::cmp::Ordering::Equal => 0,
            };
        }
        i = i.wrapping_add(1);
    }
}

/// Compare at most `n` wide characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncmp(
    s1: *const WcharT,
    s2: *const WcharT,
    n: usize,
) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b || a == 0 {
            return match a.cmp(&b) {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Greater => 1,
                core::cmp::Ordering::Equal => 0,
            };
        }
        i = i.wrapping_add(1);
    }
    0
}

/// Concatenate wide strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcscat(dst: *mut WcharT, src: *const WcharT) -> *mut WcharT {
    let dlen = unsafe { wcslen(dst) };
    unsafe { wcscpy(dst.add(dlen), src) };
    dst
}

/// Find a wide character in a wide string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcschr(s: *const WcharT, wc: WcharT) -> *const WcharT {
    let mut i: usize = 0;
    loop {
        let c = unsafe { *s.add(i) };
        if c == wc {
            return unsafe { s.add(i) };
        }
        if c == 0 {
            return core::ptr::null();
        }
        i = i.wrapping_add(1);
    }
}

/// Find the last occurrence of a wide character.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsrchr(s: *const WcharT, wc: WcharT) -> *const WcharT {
    let len = unsafe { wcslen(s) };
    let mut i = len;
    // Include position `len` to check for searching null terminator.
    loop {
        if unsafe { *s.add(i) } == wc {
            return unsafe { s.add(i) };
        }
        if i == 0 {
            break;
        }
        i = i.wrapping_sub(1);
    }
    core::ptr::null()
}

// ---------------------------------------------------------------------------
// Wide memory operations
// ---------------------------------------------------------------------------

/// Copy wide characters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemcpy(
    dst: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    let mut i: usize = 0;
    while i < n {
        unsafe { *dst.add(i) = *src.add(i); }
        i = i.wrapping_add(1);
    }
    dst
}

/// Set wide characters to a value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemset(dst: *mut WcharT, wc: WcharT, n: usize) -> *mut WcharT {
    let mut i: usize = 0;
    while i < n {
        unsafe { *dst.add(i) = wc; }
        i = i.wrapping_add(1);
    }
    dst
}

/// Compare wide character regions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemcmp(s1: *const WcharT, s2: *const WcharT, n: usize) -> i32 {
    let mut i: usize = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b {
            return if a < b { -1 } else { 1 };
        }
        i = i.wrapping_add(1);
    }
    0
}

// ---------------------------------------------------------------------------
// nl_langinfo stub
// ---------------------------------------------------------------------------

/// Query locale-dependent information.
///
/// Returns reasonable defaults for the C locale.
#[unsafe(no_mangle)]
pub extern "C" fn nl_langinfo(item: i32) -> *const u8 {
    match item {
        // CODESET
        14 => c"UTF-8".as_ptr().cast::<u8>(),
        // D_T_FMT
        1 => c"%a %b %e %H:%M:%S %Y".as_ptr().cast::<u8>(),
        // D_FMT
        2 => c"%m/%d/%y".as_ptr().cast::<u8>(),
        // T_FMT
        3 => c"%H:%M:%S".as_ptr().cast::<u8>(),
        // RADIXCHAR
        4 => c".".as_ptr().cast::<u8>(),
        // YESEXPR
        6 => c"^[yY]".as_ptr().cast::<u8>(),
        // NOEXPR
        7 => c"^[nN]".as_ptr().cast::<u8>(),
        // THOUSEP and everything else
        _ => c"".as_ptr().cast::<u8>(),
    }
}
