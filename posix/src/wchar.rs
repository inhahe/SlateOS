//! Wide character and multibyte string support (`<wchar.h>`, `<wctype.h>`).
//!
//! Full UTF-8 multibyte ↔ wchar_t (Unicode code point) conversion.
//! Our OS uses UTF-8 throughout, and these functions correctly decode
//! and encode UTF-8 sequences up to 4 bytes (U+10FFFF).
//!
//! ## Implemented
//!
//! - `mblen`, `mbtowc`, `wctomb` — multibyte ↔ wide character (UTF-8)
//! - `mbstowcs`, `wcstombs` — multibyte ↔ wide string (UTF-8)
//! - `mbrtowc`, `wcrtomb` — restartable multibyte conversion (UTF-8)
//! - `wcwidth`, `wcswidth` — character/string display width (Unicode)
//! - `btowc`, `wctob` — byte ↔ wide character
//! - `mbsinit` — check initial shift state
//! - `wctype`, `iswctype` — generic character class dispatch
//! - `wctrans`, `towctrans` — generic character transformation dispatch
//! - `towlower`, `towupper` — wide character case conversion
//! - `iswalpha`, `iswdigit`, `iswalnum`, `iswspace`, `iswprint`,
//!   `iswupper`, `iswlower`, `iswpunct`, `iswcntrl`, `iswgraph`,
//!   `iswxdigit`, `iswblank` — wide ctype
//! - `wcscpy`, `wcsncpy`, `wcslen`, `wcscmp`, `wcsncmp`, `wcscat`,
//!   `wcschr`, `wcsrchr` — wide string operations
//! - `wmemcpy`, `wmemset`, `wmemcmp` — wide memory operations

/// Wide character type (32-bit Unicode code point).
pub type WcharT = i32;

/// Multibyte conversion state for restartable functions (`mbrtowc`, `wcrtomb`).
///
/// Tracks a partially decoded UTF-8 sequence.  Layout:
/// - bytes 0..3: accumulated input bytes of the partial character
/// - byte 4: number of bytes accumulated so far
/// - byte 5: total bytes expected for this character (0 = initial state)
/// - bytes 6..7: reserved (zero)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MbstateT {
    _opaque: [u8; 8],
}

impl MbstateT {
    /// Create a zero-initialized (initial) shift state.
    const fn new() -> Self {
        Self { _opaque: [0; 8] }
    }

    /// Check if this is the initial shift state.
    fn is_initial(&self) -> bool {
        self._opaque[4] == 0 && self._opaque[5] == 0
    }

    /// Get accumulated byte count.
    fn count(&self) -> usize {
        self._opaque[4] as usize
    }

    /// Get expected total byte count (0 = initial).
    fn expected(&self) -> usize {
        self._opaque[5] as usize
    }

    /// Store an accumulated byte.
    fn push(&mut self, b: u8) {
        let idx = self._opaque[4] as usize;
        if idx < 4 {
            self._opaque[idx] = b;
            self._opaque[4] = self._opaque[4].wrapping_add(1);
        }
    }

    /// Set the expected byte count for the current character.
    fn set_expected(&mut self, n: u8) {
        self._opaque[5] = n;
    }

    /// Get the accumulated bytes.
    fn bytes(&self, n: usize) -> [u8; 4] {
        let mut out = [0u8; 4];
        let count = if n < 4 { n } else { 4 };
        let mut i = 0;
        while i < count {
            out[i] = self._opaque[i];
            i += 1;
        }
        out
    }

    /// Reset to initial state.
    fn reset(&mut self) {
        self._opaque = [0; 8];
    }
}

// ---------------------------------------------------------------------------
// Internal UTF-8 helpers
// ---------------------------------------------------------------------------

/// Determine the byte length of a UTF-8 sequence from its leading byte.
///
/// Returns 1..=4 for valid lead bytes, 0 for continuation or invalid bytes.
#[inline]
fn utf8_seq_len(lead: u8) -> usize {
    if lead < 0x80 { 1 }
    else if lead < 0xC2 { 0 }        // Overlong 2-byte or continuation.
    else if lead < 0xE0 { 2 }
    else if lead < 0xF0 { 3 }
    else if lead < 0xF5 { 4 }        // F5..FF are invalid lead bytes.
    else { 0 }
}

/// Check if a byte is a UTF-8 continuation byte (10xxxxxx).
#[inline]
fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// Decode a complete UTF-8 sequence from `bytes[..len]` into a code point.
///
/// Returns `None` for invalid sequences (overlong, surrogate, > U+10FFFF).
#[allow(clippy::arithmetic_side_effects)]
fn utf8_decode(bytes: &[u8], len: usize) -> Option<u32> {
    let cp = match len {
        1 => u32::from(bytes[0]),
        2 => {
            let b0 = u32::from(bytes[0] & 0x1F);
            let b1 = u32::from(bytes[1] & 0x3F);
            (b0 << 6) | b1
        }
        3 => {
            let b0 = u32::from(bytes[0] & 0x0F);
            let b1 = u32::from(bytes[1] & 0x3F);
            let b2 = u32::from(bytes[2] & 0x3F);
            (b0 << 12) | (b1 << 6) | b2
        }
        4 => {
            let b0 = u32::from(bytes[0] & 0x07);
            let b1 = u32::from(bytes[1] & 0x3F);
            let b2 = u32::from(bytes[2] & 0x3F);
            let b3 = u32::from(bytes[3] & 0x3F);
            (b0 << 18) | (b1 << 12) | (b2 << 6) | b3
        }
        _ => return None,
    };

    // Reject overlong encodings.
    match len {
        2 if cp < 0x80 => return None,
        3 if cp < 0x800 => return None,
        4 if cp < 0x1_0000 => return None,
        _ => {}
    }

    // Reject surrogates (U+D800..U+DFFF) and values > U+10FFFF.
    if (0xD800..=0xDFFF).contains(&cp) || cp > 0x10_FFFF {
        return None;
    }

    Some(cp)
}

/// Encode a Unicode code point as UTF-8 into `buf`.
///
/// Returns the number of bytes written (1..=4), or 0 if the code point
/// is invalid (> U+10FFFF or a surrogate).
#[allow(clippy::arithmetic_side_effects)]
fn utf8_encode(cp: u32, buf: &mut [u8; 4]) -> usize {
    if cp <= 0x7F {
        buf[0] = cp as u8;
        1
    } else if cp <= 0x7FF {
        buf[0] = (0xC0 | (cp >> 6)) as u8;
        buf[1] = (0x80 | (cp & 0x3F)) as u8;
        2
    } else if cp <= 0xFFFF {
        // Reject surrogates.
        if (0xD800..=0xDFFF).contains(&cp) {
            return 0;
        }
        buf[0] = (0xE0 | (cp >> 12)) as u8;
        buf[1] = (0x80 | ((cp >> 6) & 0x3F)) as u8;
        buf[2] = (0x80 | (cp & 0x3F)) as u8;
        3
    } else if cp <= 0x10_FFFF {
        buf[0] = (0xF0 | (cp >> 18)) as u8;
        buf[1] = (0x80 | ((cp >> 12) & 0x3F)) as u8;
        buf[2] = (0x80 | ((cp >> 6) & 0x3F)) as u8;
        buf[3] = (0x80 | (cp & 0x3F)) as u8;
        4
    } else {
        0 // Invalid code point.
    }
}

// ---------------------------------------------------------------------------
// Multibyte ↔ wide character
// ---------------------------------------------------------------------------

/// Determine the number of bytes in a UTF-8 multibyte character.
///
/// Returns 0 for null byte, 1..4 for valid UTF-8 lead bytes,
/// -1 for invalid (sets errno to EILSEQ).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn mblen(s: *const u8, n: usize) -> i32 {
    if s.is_null() {
        return 0; // No state-dependent encoding.
    }
    let lead = unsafe { *s };
    if lead == 0 {
        return 0;
    }

    let seq_len = utf8_seq_len(lead);
    if seq_len == 0 || seq_len > n {
        return -1;
    }

    // Verify continuation bytes.
    let mut i = 1;
    while i < seq_len {
        if !is_cont(unsafe { *s.add(i) }) {
            return -1;
        }
        i += 1;
    }

    // Build the byte slice and validate the code point.
    let mut buf = [0u8; 4];
    let mut j = 0;
    while j < seq_len {
        buf[j] = unsafe { *s.add(j) };
        j += 1;
    }
    if utf8_decode(&buf, seq_len).is_none() {
        return -1;
    }

    seq_len as i32
}

/// Convert a UTF-8 multibyte character to a wide character (code point).
///
/// Reads up to `n` bytes from `s`, decodes one UTF-8 character, and
/// stores the Unicode code point in `*pwc`.
///
/// Returns the number of bytes consumed (1..4), 0 for null character,
/// or -1 for invalid sequence.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn mbtowc(pwc: *mut WcharT, s: *const u8, n: usize) -> i32 {
    if s.is_null() {
        return 0;
    }
    let lead = unsafe { *s };
    if lead == 0 {
        if !pwc.is_null() {
            unsafe { *pwc = 0; }
        }
        return 0;
    }

    let seq_len = utf8_seq_len(lead);
    if seq_len == 0 || seq_len > n {
        return -1;
    }

    let mut buf = [0u8; 4];
    buf[0] = lead;
    let mut i = 1;
    while i < seq_len {
        let b = unsafe { *s.add(i) };
        if !is_cont(b) {
            return -1;
        }
        buf[i] = b;
        i += 1;
    }

    match utf8_decode(&buf, seq_len) {
        Some(cp) => {
            if !pwc.is_null() {
                unsafe { *pwc = cp as WcharT; }
            }
            seq_len as i32
        }
        None => -1,
    }
}

/// Convert a wide character (Unicode code point) to UTF-8.
///
/// Writes the UTF-8 encoding of `wc` into `s` (which must have room
/// for at least `MB_CUR_MAX` = 4 bytes).
///
/// Returns the number of bytes written (1..4), or -1 if the code
/// point is not valid Unicode.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wctomb(s: *mut u8, wc: WcharT) -> i32 {
    if s.is_null() {
        return 0; // No state-dependent encoding.
    }

    if wc < 0 {
        return -1;
    }
    let cp = wc as u32;
    let mut buf = [0u8; 4];
    let n = utf8_encode(cp, &mut buf);
    if n == 0 {
        return -1;
    }

    let mut i = 0;
    while i < n {
        // SAFETY: Caller guarantees s has room for MB_CUR_MAX bytes.
        unsafe { *s.add(i) = buf[i]; }
        i += 1;
    }
    n as i32
}

/// Convert a UTF-8 multibyte string to a wide string.
///
/// Decodes up to `n` wide characters from the UTF-8 string at `src`
/// and stores them in `dst`.  If `dst` is null, just counts characters.
///
/// Returns the number of wide characters written (not counting null),
/// or `(size_t)-1` on encoding error.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn mbstowcs(dst: *mut WcharT, src: *const u8, n: usize) -> usize {
    if src.is_null() {
        return 0;
    }

    let mut src_off: usize = 0;
    let mut dst_count: usize = 0;

    while dst_count < n {
        let lead = unsafe { *src.add(src_off) };
        if lead == 0 {
            if !dst.is_null() {
                unsafe { *dst.add(dst_count) = 0; }
            }
            return dst_count;
        }

        let seq_len = utf8_seq_len(lead);
        if seq_len == 0 {
            return usize::MAX; // EILSEQ.
        }

        let mut buf = [0u8; 4];
        buf[0] = lead;
        let mut i = 1;
        while i < seq_len {
            let b = unsafe { *src.add(src_off + i) };
            if !is_cont(b) {
                return usize::MAX;
            }
            buf[i] = b;
            i += 1;
        }

        match utf8_decode(&buf, seq_len) {
            Some(cp) => {
                if !dst.is_null() {
                    unsafe { *dst.add(dst_count) = cp as WcharT; }
                }
                src_off += seq_len;
                dst_count += 1;
            }
            None => return usize::MAX,
        }
    }
    dst_count
}

/// Convert a wide string to a UTF-8 multibyte string.
///
/// Encodes wide characters from `src` into the UTF-8 buffer at `dst`,
/// writing at most `n` bytes.  If `dst` is null, counts the total
/// bytes needed.
///
/// Returns the number of bytes written (not counting null terminator),
/// or `(size_t)-1` if a code point is invalid.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstombs(dst: *mut u8, src: *const WcharT, n: usize) -> usize {
    if src.is_null() {
        return 0;
    }

    let mut src_idx: usize = 0;
    let mut dst_off: usize = 0;

    loop {
        let wc = unsafe { *src.add(src_idx) };
        if wc == 0 {
            // Null-terminate if room.
            if !dst.is_null() && dst_off < n {
                unsafe { *dst.add(dst_off) = 0; }
            }
            return dst_off;
        }

        if wc < 0 {
            return usize::MAX;
        }

        let mut buf = [0u8; 4];
        let enc_len = utf8_encode(wc as u32, &mut buf);
        if enc_len == 0 {
            return usize::MAX; // Invalid code point.
        }

        // Check if there's room in the output buffer.
        if dst_off + enc_len > n {
            return dst_off; // Buffer full, stop.
        }

        if !dst.is_null() {
            let mut i = 0;
            while i < enc_len {
                unsafe { *dst.add(dst_off + i) = buf[i]; }
                i += 1;
            }
        }

        dst_off += enc_len;
        src_idx += 1;
    }
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

/// Internal static state for `mbrtowc`/`wcrtomb` when caller passes null `ps`.
static mut INTERNAL_MBSTATE: MbstateT = MbstateT::new();

/// Restartable multibyte (UTF-8) → wide character.
///
/// Reads up to `n` bytes from `s`, continuing from the partial state in
/// `*ps`.  Stores the decoded code point in `*pwc`.
///
/// Returns:
/// - 0 if the decoded character is null (U+0000)
/// - 1..4: number of bytes consumed to complete a character
/// - `(size_t)-2`: incomplete but valid so far (state updated)
/// - `(size_t)-1`: invalid byte sequence (errno = EILSEQ)
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn mbrtowc(
    pwc: *mut WcharT,
    s: *const u8,
    n: usize,
    ps: *mut MbstateT,
) -> usize {
    // Use internal state if ps is null.
    let state = if ps.is_null() {
        // SAFETY: Single-threaded access; matches POSIX spec for null ps.
        unsafe { &mut *core::ptr::addr_of_mut!(INTERNAL_MBSTATE) }
    } else {
        unsafe { &mut *ps }
    };

    // s == NULL: equivalent to mbrtowc(NULL, "", 1, ps) — reset state.
    if s.is_null() {
        state.reset();
        return 0;
    }

    if n == 0 {
        return usize::MAX.wrapping_sub(1); // -2: need more bytes.
    }

    // If we have no partial state, start fresh.
    if state.is_initial() {
        let lead = unsafe { *s };
        if lead == 0 {
            if !pwc.is_null() {
                unsafe { *pwc = 0; }
            }
            return 0;
        }

        let seq_len = utf8_seq_len(lead);
        if seq_len == 0 {
            return usize::MAX; // -1: EILSEQ.
        }

        // Start accumulating.
        state.reset();
        state.set_expected(seq_len as u8);
        state.push(lead);

        // Try to consume remaining bytes from input.
        let mut consumed: usize = 1;
        while state.count() < state.expected() && consumed < n {
            let b = unsafe { *s.add(consumed) };
            if !is_cont(b) {
                state.reset();
                return usize::MAX; // -1: EILSEQ.
            }
            state.push(b);
            consumed += 1;
        }

        if state.count() < state.expected() {
            return usize::MAX.wrapping_sub(1); // -2: incomplete.
        }

        // Decode the complete sequence.
        let buf = state.bytes(state.count());
        let seq_len = state.expected();
        state.reset();

        match utf8_decode(&buf, seq_len) {
            Some(cp) => {
                if !pwc.is_null() {
                    unsafe { *pwc = cp as WcharT; }
                }
                if cp == 0 { 0 } else { consumed }
            }
            None => usize::MAX, // -1: EILSEQ.
        }
    } else {
        // Continue from partial state.
        let mut consumed: usize = 0;
        while state.count() < state.expected() && consumed < n {
            let b = unsafe { *s.add(consumed) };
            if !is_cont(b) {
                state.reset();
                return usize::MAX; // -1: EILSEQ.
            }
            state.push(b);
            consumed += 1;
        }

        if state.count() < state.expected() {
            return usize::MAX.wrapping_sub(1); // -2: incomplete.
        }

        let buf = state.bytes(state.count());
        let seq_len = state.expected();
        state.reset();

        match utf8_decode(&buf, seq_len) {
            Some(cp) => {
                if !pwc.is_null() {
                    unsafe { *pwc = cp as WcharT; }
                }
                if cp == 0 { 0 } else { consumed }
            }
            None => usize::MAX,
        }
    }
}

/// Restartable wide character → multibyte (UTF-8).
///
/// Encodes `wc` as UTF-8 into `s` (which must have room for at least
/// `MB_CUR_MAX` = 4 bytes).  The state `ps` is currently unused since
/// UTF-8 encoding is stateless, but accepted for API compatibility.
///
/// Returns the number of bytes written, or `(size_t)-1` on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcrtomb(
    s: *mut u8,
    wc: WcharT,
    _ps: *mut MbstateT,
) -> usize {
    if s.is_null() {
        // "Reset to initial state" — no-op for UTF-8, returns 1
        // (the number of bytes to encode the null character).
        return 1;
    }

    if wc < 0 {
        return usize::MAX;
    }

    let mut buf = [0u8; 4];
    let n = utf8_encode(wc as u32, &mut buf);
    if n == 0 {
        return usize::MAX; // Invalid code point.
    }

    let mut i = 0;
    while i < n {
        // SAFETY: Caller guarantees s has room for MB_CUR_MAX bytes.
        unsafe { *s.add(i) = buf[i]; }
        i += 1;
    }
    n
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
// wctype / iswctype — generic classification dispatch (<wctype.h>)
// ---------------------------------------------------------------------------

/// Opaque handle for a character class (returned by `wctype()`).
///
/// POSIX defines `wctype_t` as a scalar.  We encode each class as
/// a small nonzero integer so `0` means "invalid."
pub type WctypeT = u32;

// Class IDs — keep in sync with wctype() and iswctype().
const WC_ALNUM:  WctypeT = 1;
const WC_ALPHA:  WctypeT = 2;
const WC_BLANK:  WctypeT = 3;
const WC_CNTRL:  WctypeT = 4;
const WC_DIGIT:  WctypeT = 5;
const WC_GRAPH:  WctypeT = 6;
const WC_LOWER:  WctypeT = 7;
const WC_PRINT:  WctypeT = 8;
const WC_PUNCT:  WctypeT = 9;
const WC_SPACE:  WctypeT = 10;
const WC_UPPER:  WctypeT = 11;
const WC_XDIGIT: WctypeT = 12;

/// Look up a character class by name.
///
/// Returns a nonzero `wctype_t` handle for the twelve standard POSIX
/// classes, or `0` for unrecognized names.
///
/// # Safety
///
/// `name` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wctype(name: *const u8) -> WctypeT {
    if name.is_null() {
        return 0;
    }

    // Read the name into a bounded buffer to avoid walking arbitrary memory.
    let mut buf = [0u8; 16];
    let mut i: usize = 0;
    while i < 15 {
        let c = unsafe { *name.add(i) };
        if c == 0 { break; }
        buf[i] = c;
        i = i.wrapping_add(1);
    }
    let len = i;

    match &buf[..len] {
        b"alnum"  => WC_ALNUM,
        b"alpha"  => WC_ALPHA,
        b"blank"  => WC_BLANK,
        b"cntrl"  => WC_CNTRL,
        b"digit"  => WC_DIGIT,
        b"graph"  => WC_GRAPH,
        b"lower"  => WC_LOWER,
        b"print"  => WC_PRINT,
        b"punct"  => WC_PUNCT,
        b"space"  => WC_SPACE,
        b"upper"  => WC_UPPER,
        b"xdigit" => WC_XDIGIT,
        _         => 0,
    }
}

/// Test a wide character against a class obtained from `wctype()`.
///
/// Returns nonzero if `wc` belongs to the class identified by `ct`.
#[unsafe(no_mangle)]
pub extern "C" fn iswctype(wc: WcharT, ct: WctypeT) -> i32 {
    match ct {
        WC_ALNUM  => iswalnum(wc),
        WC_ALPHA  => iswalpha(wc),
        WC_BLANK  => iswblank(wc),
        WC_CNTRL  => iswcntrl(wc),
        WC_DIGIT  => iswdigit(wc),
        WC_GRAPH  => iswgraph(wc),
        WC_LOWER  => iswlower(wc),
        WC_PRINT  => iswprint(wc),
        WC_PUNCT  => iswpunct(wc),
        WC_SPACE  => iswspace(wc),
        WC_UPPER  => iswupper(wc),
        WC_XDIGIT => iswxdigit(wc),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// wctrans / towctrans — generic transformation dispatch (<wctype.h>)
// ---------------------------------------------------------------------------

/// Opaque handle for a character transformation (returned by `wctrans()`).
pub type WctransT = u32;

const WT_TOLOWER: WctransT = 1;
const WT_TOUPPER: WctransT = 2;

/// Look up a character transformation by name.
///
/// POSIX requires `"tolower"` and `"toupper"`.  Returns `0` for
/// unrecognized names.
///
/// # Safety
///
/// `name` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wctrans(name: *const u8) -> WctransT {
    if name.is_null() {
        return 0;
    }

    let mut buf = [0u8; 16];
    let mut i: usize = 0;
    while i < 15 {
        let c = unsafe { *name.add(i) };
        if c == 0 { break; }
        buf[i] = c;
        i = i.wrapping_add(1);
    }
    let len = i;

    match &buf[..len] {
        b"tolower" => WT_TOLOWER,
        b"toupper" => WT_TOUPPER,
        _          => 0,
    }
}

/// Apply a transformation obtained from `wctrans()` to a wide character.
#[unsafe(no_mangle)]
pub extern "C" fn towctrans(wc: WcharT, tr: WctransT) -> WcharT {
    match tr {
        WT_TOLOWER => towlower(wc),
        WT_TOUPPER => towupper(wc),
        _ => wc,
    }
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

// ---------------------------------------------------------------------------
// MB_CUR_MAX / mbrlen
// ---------------------------------------------------------------------------

/// Maximum bytes per multibyte character in UTF-8.
pub const MB_CUR_MAX: usize = 4;

/// Determine the number of bytes in a restartable multibyte character.
///
/// Equivalent to `mbrtowc(NULL, s, n, ps)` but doesn't store the
/// decoded character.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrlen(s: *const u8, n: usize, ps: *mut MbstateT) -> usize {
    unsafe { mbrtowc(core::ptr::null_mut(), s, n, ps) }
}

/// Concatenate at most `n` wide characters from `src` to `dst`.
///
/// Appends up to `n` wide characters, always null-terminates.
///
/// # Safety
///
/// `dst` must have room for the existing string plus `n` + 1 wide chars.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsncat(
    dst: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    let dlen = unsafe { wcslen(dst) };
    let mut j: usize = 0;
    while j < n {
        let c = unsafe { *src.add(j) };
        unsafe { *dst.add(dlen.wrapping_add(j)) = c; }
        if c == 0 {
            return dst;
        }
        j = j.wrapping_add(1);
    }
    // Null-terminate.
    unsafe { *dst.add(dlen.wrapping_add(j)) = 0; }
    dst
}

/// Search for a wide character in a memory region.
///
/// # Safety
///
/// `s` must be valid for `n * sizeof(wchar_t)` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemchr(
    s: *const WcharT,
    wc: WcharT,
    n: usize,
) -> *const WcharT {
    let mut i: usize = 0;
    while i < n {
        if unsafe { *s.add(i) } == wc {
            return unsafe { s.add(i) };
        }
        i = i.wrapping_add(1);
    }
    core::ptr::null()
}

/// Move wide characters (overlapping regions safe).
///
/// # Safety
///
/// Both `dst` and `src` must be valid for `n * sizeof(wchar_t)` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmemmove(
    dst: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    if (dst as usize) < (src as usize) {
        let mut i: usize = 0;
        while i < n {
            unsafe { *dst.add(i) = *src.add(i); }
            i = i.wrapping_add(1);
        }
    } else if (dst as usize) > (src as usize) {
        let mut i = n;
        while i > 0 {
            i = i.wrapping_sub(1);
            unsafe { *dst.add(i) = *src.add(i); }
        }
    }
    dst
}

/// Find a wide substring in a wide string.
///
/// # Safety
///
/// Both strings must be valid null-terminated wide strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wcsstr(
    haystack: *const WcharT,
    needle: *const WcharT,
) -> *const WcharT {
    if unsafe { *needle } == 0 {
        return haystack;
    }

    let mut h: usize = 0;
    while unsafe { *haystack.add(h) } != 0 {
        let mut j: usize = 0;
        loop {
            let n_ch = unsafe { *needle.add(j) };
            if n_ch == 0 {
                return unsafe { haystack.add(h) };
            }
            let h_ch = unsafe { *haystack.add(h.wrapping_add(j)) };
            if h_ch != n_ch {
                break;
            }
            j = j.wrapping_add(1);
        }
        h = h.wrapping_add(1);
    }
    core::ptr::null()
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- UTF-8 internal helpers --

    #[test]
    fn test_utf8_seq_len() {
        // ASCII
        assert_eq!(utf8_seq_len(0x00), 1);
        assert_eq!(utf8_seq_len(0x41), 1); // 'A'
        assert_eq!(utf8_seq_len(0x7F), 1);
        // 2-byte
        assert_eq!(utf8_seq_len(0xC2), 2); // Smallest valid 2-byte lead.
        assert_eq!(utf8_seq_len(0xDF), 2);
        // 3-byte
        assert_eq!(utf8_seq_len(0xE0), 3);
        assert_eq!(utf8_seq_len(0xEF), 3);
        // 4-byte
        assert_eq!(utf8_seq_len(0xF0), 4);
        assert_eq!(utf8_seq_len(0xF4), 4);
        // Invalid
        assert_eq!(utf8_seq_len(0x80), 0); // Continuation byte.
        assert_eq!(utf8_seq_len(0xBF), 0);
        assert_eq!(utf8_seq_len(0xC0), 0); // Overlong.
        assert_eq!(utf8_seq_len(0xC1), 0);
        assert_eq!(utf8_seq_len(0xF5), 0); // > U+10FFFF.
        assert_eq!(utf8_seq_len(0xFF), 0);
    }

    #[test]
    fn test_utf8_decode_ascii() {
        assert_eq!(utf8_decode(&[0x41], 1), Some(0x41)); // 'A'
        assert_eq!(utf8_decode(&[0x00], 1), Some(0x00)); // NUL
        assert_eq!(utf8_decode(&[0x7F], 1), Some(0x7F)); // DEL
    }

    #[test]
    fn test_utf8_decode_2byte() {
        // U+00E9 = é = C3 A9
        assert_eq!(utf8_decode(&[0xC3, 0xA9], 2), Some(0xE9));
        // U+00A3 = £ = C2 A3
        assert_eq!(utf8_decode(&[0xC2, 0xA3], 2), Some(0xA3));
        // U+07FF = max 2-byte = DF BF
        assert_eq!(utf8_decode(&[0xDF, 0xBF], 2), Some(0x7FF));
    }

    #[test]
    fn test_utf8_decode_3byte() {
        // U+20AC = € = E2 82 AC
        assert_eq!(utf8_decode(&[0xE2, 0x82, 0xAC], 3), Some(0x20AC));
        // U+FFFF = max 3-byte = EF BF BF
        assert_eq!(utf8_decode(&[0xEF, 0xBF, 0xBF], 3), Some(0xFFFF));
    }

    #[test]
    fn test_utf8_decode_4byte() {
        // U+1F600 = 😀 = F0 9F 98 80
        assert_eq!(utf8_decode(&[0xF0, 0x9F, 0x98, 0x80], 4), Some(0x1F600));
        // U+10FFFF = max Unicode = F4 8F BF BF
        assert_eq!(utf8_decode(&[0xF4, 0x8F, 0xBF, 0xBF], 4), Some(0x10FFFF));
    }

    #[test]
    fn test_utf8_decode_rejects_surrogates() {
        // U+D800 would be ED A0 80
        assert_eq!(utf8_decode(&[0xED, 0xA0, 0x80], 3), None);
        // U+DFFF would be ED BF BF
        assert_eq!(utf8_decode(&[0xED, 0xBF, 0xBF], 3), None);
    }

    #[test]
    fn test_utf8_decode_rejects_overlong() {
        // U+0041 as 2 bytes (overlong): C1 81
        assert_eq!(utf8_decode(&[0xC1, 0x81], 2), None);
        // U+007F as 2 bytes (overlong): C1 BF
        assert_eq!(utf8_decode(&[0xC1, 0xBF], 2), None);
    }

    #[test]
    fn test_utf8_encode_roundtrip() {
        let test_cps = [0x00, 0x41, 0x7F, 0x80, 0xE9, 0x7FF, 0x800,
                        0x20AC, 0xFFFF, 0x10000, 0x1F600, 0x10FFFF];
        for &cp in &test_cps {
            let mut buf = [0u8; 4];
            let n = utf8_encode(cp, &mut buf);
            assert!(n > 0, "encode failed for U+{:04X}", cp);
            let decoded = utf8_decode(&buf, n);
            assert_eq!(decoded, Some(cp), "roundtrip failed for U+{:04X}", cp);
        }
    }

    #[test]
    fn test_utf8_encode_rejects_invalid() {
        let mut buf = [0u8; 4];
        assert_eq!(utf8_encode(0xD800, &mut buf), 0); // Surrogate.
        assert_eq!(utf8_encode(0xDFFF, &mut buf), 0);
        assert_eq!(utf8_encode(0x110000, &mut buf), 0); // > max.
    }

    // -- mblen --

    #[test]
    fn test_mblen_ascii() {
        let s = b"A";
        assert_eq!(unsafe { mblen(s.as_ptr(), 1) }, 1);
    }

    #[test]
    fn test_mblen_null() {
        assert_eq!(unsafe { mblen(core::ptr::null(), 0) }, 0);
        let s = b"\0";
        assert_eq!(unsafe { mblen(s.as_ptr(), 1) }, 0);
    }

    #[test]
    fn test_mblen_multibyte() {
        // é = C3 A9
        let s: &[u8] = &[0xC3, 0xA9];
        assert_eq!(unsafe { mblen(s.as_ptr(), 2) }, 2);
        // € = E2 82 AC
        let s: &[u8] = &[0xE2, 0x82, 0xAC];
        assert_eq!(unsafe { mblen(s.as_ptr(), 3) }, 3);
        // 😀 = F0 9F 98 80
        let s: &[u8] = &[0xF0, 0x9F, 0x98, 0x80];
        assert_eq!(unsafe { mblen(s.as_ptr(), 4) }, 4);
    }

    #[test]
    fn test_mblen_insufficient_bytes() {
        // 2-byte char but n=1
        let s: &[u8] = &[0xC3, 0xA9];
        assert_eq!(unsafe { mblen(s.as_ptr(), 1) }, -1);
    }

    // -- mbtowc --

    #[test]
    fn test_mbtowc_ascii() {
        let s = b"Z";
        let mut wc: WcharT = 0;
        assert_eq!(unsafe { mbtowc(&mut wc, s.as_ptr(), 1) }, 1);
        assert_eq!(wc, 0x5A);
    }

    #[test]
    fn test_mbtowc_euro() {
        // € = U+20AC = E2 82 AC
        let s: &[u8] = &[0xE2, 0x82, 0xAC];
        let mut wc: WcharT = 0;
        assert_eq!(unsafe { mbtowc(&mut wc, s.as_ptr(), 3) }, 3);
        assert_eq!(wc, 0x20AC);
    }

    #[test]
    fn test_mbtowc_emoji() {
        // 😀 = U+1F600 = F0 9F 98 80
        let s: &[u8] = &[0xF0, 0x9F, 0x98, 0x80];
        let mut wc: WcharT = 0;
        assert_eq!(unsafe { mbtowc(&mut wc, s.as_ptr(), 4) }, 4);
        assert_eq!(wc, 0x1F600);
    }

    // -- wctomb --

    #[test]
    fn test_wctomb_ascii() {
        let mut buf = [0u8; 4];
        assert_eq!(unsafe { wctomb(buf.as_mut_ptr(), 0x41) }, 1);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn test_wctomb_multibyte() {
        // U+20AC = € = E2 82 AC
        let mut buf = [0u8; 4];
        assert_eq!(unsafe { wctomb(buf.as_mut_ptr(), 0x20AC) }, 3);
        assert_eq!(&buf[..3], &[0xE2, 0x82, 0xAC]);
    }

    #[test]
    fn test_wctomb_emoji() {
        // U+1F600 = 😀 = F0 9F 98 80
        let mut buf = [0u8; 4];
        assert_eq!(unsafe { wctomb(buf.as_mut_ptr(), 0x1F600) }, 4);
        assert_eq!(&buf, &[0xF0, 0x9F, 0x98, 0x80]);
    }

    #[test]
    fn test_wctomb_invalid() {
        let mut buf = [0u8; 4];
        assert_eq!(unsafe { wctomb(buf.as_mut_ptr(), -1) }, -1);
    }

    // -- mbstowcs / wcstombs roundtrip --

    #[test]
    fn test_mbstowcs_ascii() {
        let src = b"Hello\0";
        let mut dst = [0i32; 16];
        let n = unsafe { mbstowcs(dst.as_mut_ptr(), src.as_ptr(), 16) };
        assert_eq!(n, 5);
        assert_eq!(dst[0], b'H' as i32);
        assert_eq!(dst[4], b'o' as i32);
        assert_eq!(dst[5], 0);
    }

    #[test]
    fn test_mbstowcs_utf8() {
        // "café" = 63 61 66 C3 A9 00
        let src: &[u8] = &[0x63, 0x61, 0x66, 0xC3, 0xA9, 0x00];
        let mut dst = [0i32; 16];
        let n = unsafe { mbstowcs(dst.as_mut_ptr(), src.as_ptr(), 16) };
        assert_eq!(n, 4); // c, a, f, é
        assert_eq!(dst[0], 0x63); // c
        assert_eq!(dst[3], 0xE9); // é
    }

    #[test]
    fn test_wcstombs_roundtrip() {
        // U+20AC (€), U+0041 (A)
        let src: &[i32] = &[0x20AC, 0x41, 0];
        let mut dst = [0u8; 16];
        let n = unsafe { wcstombs(dst.as_mut_ptr(), src.as_ptr(), 16) };
        assert_eq!(n, 4); // 3 bytes for €, 1 for A
        assert_eq!(&dst[..3], &[0xE2, 0x82, 0xAC]);
        assert_eq!(dst[3], b'A');
    }

    // -- MbstateT --

    #[test]
    fn test_mbstate_initial() {
        let st = MbstateT::new();
        assert!(st.is_initial());
        assert_eq!(st.count(), 0);
        assert_eq!(st.expected(), 0);
    }

    #[test]
    fn test_mbstate_push_reset() {
        let mut st = MbstateT::new();
        st.set_expected(3);
        st.push(0xE2);
        assert_eq!(st.count(), 1);
        assert!(!st.is_initial());
        st.push(0x82);
        st.push(0xAC);
        assert_eq!(st.count(), 3);
        let bytes = st.bytes(3);
        assert_eq!(&bytes[..3], &[0xE2, 0x82, 0xAC]);
        st.reset();
        assert!(st.is_initial());
    }

    // -- wctype / iswctype --

    #[test]
    fn test_wctype_known_classes() {
        let names: &[(&[u8], WctypeT)] = &[
            (b"alnum\0",  WC_ALNUM),
            (b"alpha\0",  WC_ALPHA),
            (b"blank\0",  WC_BLANK),
            (b"cntrl\0",  WC_CNTRL),
            (b"digit\0",  WC_DIGIT),
            (b"graph\0",  WC_GRAPH),
            (b"lower\0",  WC_LOWER),
            (b"print\0",  WC_PRINT),
            (b"punct\0",  WC_PUNCT),
            (b"space\0",  WC_SPACE),
            (b"upper\0",  WC_UPPER),
            (b"xdigit\0", WC_XDIGIT),
        ];
        for &(name, expected) in names {
            let ct = unsafe { wctype(name.as_ptr()) };
            assert_eq!(ct, expected, "wctype({:?}) failed", core::str::from_utf8(&name[..name.len()-1]).unwrap_or("?"));
        }
    }

    #[test]
    fn test_wctype_unknown() {
        assert_eq!(unsafe { wctype(b"bogus\0".as_ptr()) }, 0);
        assert_eq!(unsafe { wctype(b"\0".as_ptr()) }, 0);
        assert_eq!(unsafe { wctype(core::ptr::null()) }, 0);
    }

    #[test]
    fn test_iswctype_dispatch() {
        let digit_ct = unsafe { wctype(b"digit\0".as_ptr()) };
        assert_ne!(iswctype(b'5' as WcharT, digit_ct), 0);
        assert_eq!(iswctype(b'A' as WcharT, digit_ct), 0);

        let upper_ct = unsafe { wctype(b"upper\0".as_ptr()) };
        assert_ne!(iswctype(b'Z' as WcharT, upper_ct), 0);
        assert_eq!(iswctype(b'z' as WcharT, upper_ct), 0);

        let space_ct = unsafe { wctype(b"space\0".as_ptr()) };
        assert_ne!(iswctype(b' ' as WcharT, space_ct), 0);
        assert_eq!(iswctype(b'x' as WcharT, space_ct), 0);
    }

    #[test]
    fn test_iswctype_invalid_class() {
        // Class 0 (invalid) should always return 0.
        assert_eq!(iswctype(b'A' as WcharT, 0), 0);
        assert_eq!(iswctype(b'0' as WcharT, 99), 0);
    }

    // -- wctrans / towctrans --

    #[test]
    fn test_wctrans_known() {
        assert_eq!(unsafe { wctrans(b"tolower\0".as_ptr()) }, WT_TOLOWER);
        assert_eq!(unsafe { wctrans(b"toupper\0".as_ptr()) }, WT_TOUPPER);
    }

    #[test]
    fn test_wctrans_unknown() {
        assert_eq!(unsafe { wctrans(b"tostuff\0".as_ptr()) }, 0);
        assert_eq!(unsafe { wctrans(core::ptr::null()) }, 0);
    }

    #[test]
    fn test_towctrans_dispatch() {
        let to_lower = unsafe { wctrans(b"tolower\0".as_ptr()) };
        assert_eq!(towctrans(b'A' as WcharT, to_lower), b'a' as WcharT);
        assert_eq!(towctrans(b'z' as WcharT, to_lower), b'z' as WcharT);

        let to_upper = unsafe { wctrans(b"toupper\0".as_ptr()) };
        assert_eq!(towctrans(b'a' as WcharT, to_upper), b'A' as WcharT);
        assert_eq!(towctrans(b'Z' as WcharT, to_upper), b'Z' as WcharT);
    }

    #[test]
    fn test_towctrans_invalid() {
        // Invalid transform → return character unchanged.
        assert_eq!(towctrans(b'A' as WcharT, 0), b'A' as WcharT);
    }
}
