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
//!   `wcschr`, `wcsrchr`, `wcsstr`, `wcsncat`, `wcsdup` — wide string operations
//! - `wcsspn`, `wcscspn`, `wcspbrk`, `wcstok` — wide string search/tokenize
//! - `wcstol`, `wcstoul`, `wcstoll`, `wcstoull` — wide string→integer
//! - `wcstod`, `wcstof` — wide string→float
//! - `wmemcpy`, `wmemset`, `wmemcmp`, `wmemmove`, `wmemchr`, `wmempcpy` — wide memory ops
//! - `mbsrtowcs`, `mbsnrtowcs`, `wcsrtombs`, `wcsnrtombs` — restartable string conversion
//! - `wcscasecmp`, `wcsncasecmp` — case-insensitive wide string comparison
//! - `fputwc`, `fgetwc`, `putwc`, `getwc`, `putwchar`, `getwchar` — wide char I/O
//! - `fputws`, `fgetws` — wide string I/O
//! - `ungetwc` — push back wide character (ASCII only; multi-byte is best-effort)
//! - `wcsftime` — format date/time as wide string (delegates to narrow strftime)

/// Wide character type (32-bit Unicode code point).
pub type WcharT = i32;

/// Wide-character EOF indicator.
///
/// Analogous to `EOF` for byte streams.  POSIX requires WEOF to be a
/// value of type `wint_t` that is distinct from any valid wide character.
/// On glibc/musl with 32-bit `wchar_t`, WEOF is 0xFFFFFFFF (i.e., -1
/// when interpreted as `i32`).
pub const WEOF: WcharT = -1;

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
    opaque: [u8; 8],
}

impl MbstateT {
    /// Create a zero-initialized (initial) shift state.
    const fn new() -> Self {
        Self { opaque: [0; 8] }
    }

    /// Check if this is the initial shift state.
    #[allow(clippy::indexing_slicing)] // indices 4,5 always in bounds for [u8; 8]
    fn is_initial(self) -> bool {
        self.opaque[4] == 0 && self.opaque[5] == 0
    }

    /// Get accumulated byte count.
    #[allow(clippy::indexing_slicing)] // index 4 always in bounds for [u8; 8]
    fn count(self) -> usize {
        self.opaque[4] as usize
    }

    /// Get expected total byte count (0 = initial).
    #[allow(clippy::indexing_slicing)] // index 5 always in bounds for [u8; 8]
    fn expected(self) -> usize {
        self.opaque[5] as usize
    }

    /// Store an accumulated byte.
    #[allow(clippy::indexing_slicing)] // idx < 4 is checked; indices 4 always in bounds for [u8; 8]
    fn push(&mut self, b: u8) {
        let idx = self.opaque[4] as usize;
        if idx < 4 {
            self.opaque[idx] = b;
            self.opaque[4] = self.opaque[4].wrapping_add(1);
        }
    }

    /// Set the expected byte count for the current character.
    #[allow(clippy::indexing_slicing)] // index 5 always in bounds for [u8; 8]
    fn set_expected(&mut self, n: u8) {
        self.opaque[5] = n;
    }

    /// Get the accumulated bytes.
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn bytes(self, n: usize) -> [u8; 4] {
        let mut out = [0u8; 4];
        let count = if n < 4 { n } else { 4 };
        let mut i = 0;
        while i < count {
            out[i] = self.opaque[i];
            i += 1;
        }
        out
    }

    /// Reset to initial state.
    fn reset(&mut self) {
        self.opaque = [0; 8];
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
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
            crate::errno::set_errno(crate::errno::EILSEQ);
            return usize::MAX; // EILSEQ.
        }

        let mut buf = [0u8; 4];
        buf[0] = lead;
        let mut i = 1;
        while i < seq_len {
            let b = unsafe { *src.add(src_off + i) };
            if !is_cont(b) {
                crate::errno::set_errno(crate::errno::EILSEQ);
                return usize::MAX;
            }
            buf[i] = b;
            i += 1;
        }

        if let Some(cp) = utf8_decode(&buf, seq_len) {
            if !dst.is_null() {
                unsafe { *dst.add(dst_count) = cp as WcharT; }
            }
            src_off += seq_len;
            dst_count += 1;
        } else {
            crate::errno::set_errno(crate::errno::EILSEQ);
            return usize::MAX;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
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
            crate::errno::set_errno(crate::errno::EILSEQ);
            return usize::MAX;
        }

        let mut buf = [0u8; 4];
        let enc_len = utf8_encode(wc as u32, &mut buf);
        if enc_len == 0 {
            crate::errno::set_errno(crate::errno::EILSEQ);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn btowc(c: i32) -> WcharT {
    if (0..=127).contains(&c) { c } else { -1 }
}

/// Convert a wide character to a byte.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wctob(wc: WcharT) -> i32 {
    if (0..=127).contains(&wc) { wc } else { -1 }
}

// ---------------------------------------------------------------------------
// Shift state
// ---------------------------------------------------------------------------

/// Check if `*ps` is the initial shift state.
///
/// Returns non-zero if `ps` is null or describes the initial shift
/// state.  Returns 0 if a partial multi-byte sequence is buffered
/// (e.g. mid-way through a `mbrtowc` call).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mbsinit(ps: *const MbstateT) -> i32 {
    if ps.is_null() {
        return 1; // Null pointer → initial state per POSIX.
    }
    // SAFETY: ps is non-null (checked above), caller guarantees validity.
    let state = unsafe { &*ps };
    i32::from(state.is_initial())
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            crate::errno::set_errno(crate::errno::EILSEQ);
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
                crate::errno::set_errno(crate::errno::EILSEQ);
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

        if let Some(cp) = utf8_decode(&buf, seq_len) {
            if !pwc.is_null() {
                unsafe { *pwc = cp as WcharT; }
            }
            if cp == 0 { 0 } else { consumed }
        } else {
            crate::errno::set_errno(crate::errno::EILSEQ);
            usize::MAX // -1: EILSEQ.
        }
    } else {
        // Continue from partial state.
        let mut consumed: usize = 0;
        while state.count() < state.expected() && consumed < n {
            let b = unsafe { *s.add(consumed) };
            if !is_cont(b) {
                state.reset();
                crate::errno::set_errno(crate::errno::EILSEQ);
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

        if let Some(cp) = utf8_decode(&buf, seq_len) {
            if !pwc.is_null() {
                unsafe { *pwc = cp as WcharT; }
            }
            if cp == 0 { 0 } else { consumed }
        } else {
            crate::errno::set_errno(crate::errno::EILSEQ);
            usize::MAX
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
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
        crate::errno::set_errno(crate::errno::EILSEQ);
        return usize::MAX;
    }

    let mut buf = [0u8; 4];
    let n = utf8_encode(wc as u32, &mut buf);
    if n == 0 {
        crate::errno::set_errno(crate::errno::EILSEQ);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wcwidth(wc: WcharT) -> i32 {
    if wc == 0 {
        return 0;
    }
    // C0 and C1 control characters (0x00-0x1F, 0x7F, 0x80-0x9F).
    if wc < 32 || wc == 0x7f || (wc >= 0x80 && wc <= 0x9f) {
        return -1;
    }
    // Zero-width characters: combining marks, joiners, soft hyphen, BOM, etc.
    #[allow(clippy::manual_range_contains)]
    if (wc >= 0x0300 && wc <= 0x036f)   // Combining Diacritical Marks
        || (wc >= 0x0483 && wc <= 0x0489) // Cyrillic combining marks
        || (wc >= 0x0591 && wc <= 0x05bd) // Hebrew combining marks
        || wc == 0x05bf
        || (wc >= 0x05c1 && wc <= 0x05c2)
        || (wc >= 0x05c4 && wc <= 0x05c5)
        || wc == 0x05c7
        || (wc >= 0x0600 && wc <= 0x0605) // Arabic marks
        || (wc >= 0x0610 && wc <= 0x061a)
        || (wc >= 0x064b && wc <= 0x065f)
        || wc == 0x0670
        || (wc >= 0x06d6 && wc <= 0x06dd)
        || (wc >= 0x06df && wc <= 0x06e4)
        || (wc >= 0x06e7 && wc <= 0x06e8)
        || (wc >= 0x06ea && wc <= 0x06ed)
        || wc == 0x070f
        || (wc >= 0x0730 && wc <= 0x074a)
        || (wc >= 0x07a6 && wc <= 0x07b0)
        || (wc >= 0x0900 && wc <= 0x0902) // Devanagari combining
        || wc == 0x093c || wc == 0x0941
        || (wc >= 0x0941 && wc <= 0x0948)
        || wc == 0x094d
        || (wc >= 0x0951 && wc <= 0x0957)
        || (wc >= 0x0962 && wc <= 0x0963)
        || (wc >= 0x1ab0 && wc <= 0x1aff) // Combining Diacritical Marks Extended
        || (wc >= 0x1dc0 && wc <= 0x1dff) // Combining Diacritical Marks Supplement
        || (wc >= 0x20d0 && wc <= 0x20ff) // Combining Marks for Symbols
        || (wc >= 0xfe00 && wc <= 0xfe0f) // Variation Selectors
        || (wc >= 0xfe20 && wc <= 0xfe2f) // Combining Half Marks
        || wc == 0x00ad   // Soft hyphen (zero-width in most renderers)
        || wc == 0x200b   // Zero-width space
        || wc == 0x200c   // Zero-width non-joiner
        || wc == 0x200d   // Zero-width joiner
        || wc == 0x200e   // Left-to-right mark
        || wc == 0x200f   // Right-to-left mark
        || wc == 0x2028   // Line separator (format char)
        || wc == 0x2029   // Paragraph separator (format char)
        || (wc >= 0x202a && wc <= 0x202e) // Bidi formatting
        || (wc >= 0x2060 && wc <= 0x2064) // Invisible operators
        || (wc >= 0x2066 && wc <= 0x206f) // Bidi isolates
        || wc == 0xfeff   // BOM / zero-width no-break space
        || (wc >= 0xe0100 && wc <= 0xe01ef) // Variation Selectors Supplement
    {
        return 0;
    }
    // CJK Unified Ideographs and common fullwidth / wide ranges.
    #[allow(clippy::manual_range_contains)]
    if (wc >= 0x1100 && wc <= 0x115f)   // Hangul Jamo
        || (wc >= 0x231a && wc <= 0x231b) // Watch, Hourglass (emoji)
        || wc == 0x2329 || wc == 0x232a  // Angle brackets
        || (wc >= 0x23e9 && wc <= 0x23ec) // Emoji
        || wc == 0x23f0 || wc == 0x23f3
        || (wc >= 0x25fd && wc <= 0x25fe) // Medium small squares
        || (wc >= 0x2614 && wc <= 0x2615) // Umbrella, Hot beverage
        || (wc >= 0x2648 && wc <= 0x2653) // Zodiac signs
        || wc == 0x267f || wc == 0x2693
        || wc == 0x26a1
        || (wc >= 0x26aa && wc <= 0x26ab)
        || (wc >= 0x26bd && wc <= 0x26be)
        || (wc >= 0x26c4 && wc <= 0x26c5)
        || wc == 0x26ce || wc == 0x26d4
        || wc == 0x26ea
        || (wc >= 0x26f2 && wc <= 0x26f3)
        || wc == 0x26f5 || wc == 0x26fa
        || wc == 0x26fd || wc == 0x2702
        || wc == 0x2705
        || (wc >= 0x2708 && wc <= 0x270d)
        || wc == 0x270f
        || (wc >= 0x2753 && wc <= 0x2755)
        || wc == 0x2757
        || (wc >= 0x2795 && wc <= 0x2797)
        || wc == 0x27b0 || wc == 0x27bf
        || (wc >= 0x2b1b && wc <= 0x2b1c)
        || wc == 0x2b50 || wc == 0x2b55
        || (wc >= 0x2e80 && wc <= 0xa4cf && wc != 0x303f) // CJK
        || (wc >= 0xac00 && wc <= 0xd7a3) // Hangul Syllables
        || (wc >= 0xf900 && wc <= 0xfaff) // CJK Compat Ideographs
        || (wc >= 0xfe10 && wc <= 0xfe19) // CJK Vertical Forms
        || (wc >= 0xfe30 && wc <= 0xfe6f) // CJK Compatibility Forms + Small Form Variants
        || (wc >= 0xff01 && wc <= 0xff60) // Fullwidth forms
        || (wc >= 0xffe0 && wc <= 0xffe6) // Fullwidth signs
        || (wc >= 0x1f004 && wc <= 0x1f004) // Mahjong Tile
        || wc == 0x1f0cf                 // Playing Card
        || (wc >= 0x1f18e && wc <= 0x1f18e)
        || (wc >= 0x1f191 && wc <= 0x1f19a)
        || (wc >= 0x1f200 && wc <= 0x1f202)
        || (wc >= 0x1f210 && wc <= 0x1f23b)
        || (wc >= 0x1f240 && wc <= 0x1f248)
        || (wc >= 0x1f250 && wc <= 0x1f251)
        || (wc >= 0x1f300 && wc <= 0x1f64f) // Misc Symbols & Emoticons
        || (wc >= 0x1f680 && wc <= 0x1f6ff) // Transport & Map Symbols
        || (wc >= 0x1f900 && wc <= 0x1f9ff) // Supplemental Symbols
        || (wc >= 0x1fa00 && wc <= 0x1fa6f)
        || (wc >= 0x1fa70 && wc <= 0x1faff)
        || (wc >= 0x20000 && wc <= 0x2fffd) // CJK Extension B+
        || (wc >= 0x30000 && wc <= 0x3fffd) // CJK Extension G+
    {
        return 2;
    }
    1
}

/// Return the display width of a wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalnum(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a))
}

/// Check if wide character is alphabetic.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalpha(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x41..=0x5a | 0x61..=0x7a))
}

/// Check if wide character is a digit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswdigit(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39))
}

/// Check if wide character is a hex digit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswxdigit(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66))
}

/// Check if wide character is whitespace.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswspace(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x09..=0x0d | 0x20))
}

/// Check if wide character is a blank (space or tab).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswblank(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x09 | 0x20))
}

/// Check if wide character is printable.
///
/// Returns nonzero for printable characters (0x20-0x7E and above 0x9F).
/// C1 control characters (0x80-0x9F) are NOT printable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswprint(wc: WcharT) -> i32 {
    i32::from(wc >= 0x20 && wc != 0x7f && !(wc >= 0x80 && wc <= 0x9f))
}

/// Check if wide character is a control character.
///
/// Returns nonzero for C0 controls (0x00-0x1F), DEL (0x7F),
/// and C1 controls (0x80-0x9F).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswcntrl(wc: WcharT) -> i32 {
    i32::from(wc < 0x20 || wc == 0x7f || (wc >= 0x80 && wc <= 0x9f))
}

/// Check if wide character is uppercase.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswupper(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x41..=0x5a))
}

/// Check if wide character is lowercase.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswlower(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x61..=0x7a))
}

/// Check if wide character is punctuation.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswpunct(wc: WcharT) -> i32 {
    i32::from(iswprint(wc) != 0 && iswspace(wc) == 0 && iswalnum(wc) == 0)
}

/// Check if wide character is a graph character (printable, not space).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswgraph(wc: WcharT) -> i32 {
    i32::from(iswprint(wc) != 0 && wc != 0x20)
}

/// Convert wide character to lowercase.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn towlower(wc: WcharT) -> WcharT {
    if iswupper(wc) != 0 { wc + 32 } else { wc }
}

/// Convert wide character to uppercase.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcslen(s: *const WcharT) -> usize {
    let mut i: usize = 0;
    while unsafe { *s.add(i) } != 0 {
        i = i.wrapping_add(1);
    }
    i
}

/// Compare two wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscat(dst: *mut WcharT, src: *const WcharT) -> *mut WcharT {
    let dlen = unsafe { wcslen(dst) };
    unsafe { wcscpy(dst.add(dlen), src) };
    dst
}

/// Find a wide character in a wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wmemset(dst: *mut WcharT, wc: WcharT, n: usize) -> *mut WcharT {
    let mut i: usize = 0;
    while i < n {
        unsafe { *dst.add(i) = wc; }
        i = i.wrapping_add(1);
    }
    dst
}

/// Compare wide character regions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
// Wide string → number conversion (wcstol, wcstoul, wcstoll, wcstoull, wcstod)
// ---------------------------------------------------------------------------

/// Check if a wide character is ASCII whitespace.
#[inline]
const fn wc_space(wc: WcharT) -> bool {
    matches!(wc, 0x20 | 0x09 | 0x0a | 0x0d | 0x0b | 0x0c)
}

/// Convert a wide character to its digit value in the given base.
/// Returns -1 if not a valid digit.
#[inline]
fn wc_digit(wc: WcharT, base: i32) -> i32 {
    let val = match wc {
        0x30..=0x39 => wc.wrapping_sub(0x30), // '0'..'9'
        0x61..=0x7a => wc.wrapping_sub(0x61).wrapping_add(10), // 'a'..'z'
        0x41..=0x5a => wc.wrapping_sub(0x41).wrapping_add(10), // 'A'..'Z'
        _ => return -1,
    };
    if val < base { val } else { -1 }
}

/// Skip whitespace in a wide string, returning the new index.
#[inline]
unsafe fn wc_skip_ws(nptr: *const WcharT, mut i: usize) -> usize {
    while unsafe { *nptr.add(i) } != 0 && wc_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }
    i
}

/// Detect base and skip prefix for wide integer parsing.
///
/// Returns `(actual_base, new_index, before_prefix_index)`.
///
/// For octal (`"0..."` with base 0), the leading '0' is left as a
/// parseable digit — it is NOT consumed as a prefix.  Only the `"0x"`
/// / `"0X"` hex prefix is consumed.
///
/// `before_prefix_index` is the index before any prefix was consumed.
/// The caller uses this to roll back if no digits follow a hex prefix
/// (e.g., `"0x"` without any hex digits should parse as `0` with the
/// leading '0' counted as a valid digit).
#[allow(clippy::arithmetic_side_effects)]
unsafe fn wc_detect_base(nptr: *const WcharT, mut i: usize, mut base: i32) -> (i32, usize, usize) {
    let before_prefix = i;
    if base == 0 {
        if unsafe { *nptr.add(i) } == 0x30 {
            let next = unsafe { *nptr.add(i.wrapping_add(1)) };
            if next == 0x78 || next == 0x58 {
                base = 16;
                i = i.wrapping_add(2);
            } else {
                // Octal: do NOT advance past the '0'.  It is a valid
                // digit that the main loop will consume.
                base = 8;
            }
        } else {
            base = 10;
        }
    } else if base == 16 && unsafe { *nptr.add(i) } == 0x30 {
        let next = unsafe { *nptr.add(i.wrapping_add(1)) };
        if next == 0x78 || next == 0x58 {
            i = i.wrapping_add(2);
        }
    }
    (base, i, before_prefix)
}

/// `wcstol` — convert a wide string to a `long` (`i64` on LP64).
///
/// Skips leading whitespace, handles optional sign, auto-detects base
/// when `base` is 0.  Stores end pointer through `endptr` if non-null.
/// Sets errno to ERANGE on overflow (returns LONG_MAX/LONG_MIN).
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstol(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
) -> i64 {
    if nptr.is_null() {
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0;
    }

    let mut i = unsafe { wc_skip_ws(nptr, 0) };

    let negative = unsafe { *nptr.add(i) } == 0x2d;
    if negative || unsafe { *nptr.add(i) } == 0x2b {
        i = i.wrapping_add(1);
    }

    let (actual_base, new_i, before_prefix) = unsafe { wc_detect_base(nptr, i, base) };
    i = new_i;
    let _start = i;

    // Accumulate in negative space to correctly handle LONG_MIN
    // (whose absolute value exceeds LONG_MAX by one).
    let base64 = i64::from(actual_base);
    let cutoff = i64::MIN / base64;       // Most-negative safe value before multiply.
    let cutlim = -(i64::MIN % base64);    // Maximum digit before overflow after multiply.
    let mut acc: i64 = 0;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 { break; }
        let d = wc_digit(wc, actual_base);
        if d < 0 { break; }
        any_digits = true;

        // Check for overflow before accumulating.
        if acc < cutoff || (acc == cutoff && i64::from(d) > cutlim) {
            overflow = true;
            // Continue parsing to set endptr correctly per POSIX.
        } else if !overflow {
            acc = acc * base64 - i64::from(d);
        }
        i = i.wrapping_add(1);
    }

    // If no digits were parsed after a "0x"/"0X" prefix, the leading
    // '0' is still a valid digit (octal/hex zero).  Roll back to just
    // past the '0' so endptr is correct.
    if !any_digits && i != before_prefix {
        i = before_prefix.wrapping_add(1);
        any_digits = true;
        // acc stays 0, result is 0.
    }

    if !endptr.is_null() {
        unsafe { *endptr = if !any_digits { nptr } else { nptr.add(i) }; }
    }

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return if negative { i64::MIN } else { i64::MAX };
    }

    // acc is non-positive; negate if the input was positive.
    if negative { acc } else { -acc }
}

/// `wcstoul` — convert a wide string to an `unsigned long` (`u64` on LP64).
///
/// Sets errno to ERANGE on overflow (returns ULONG_MAX).
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstoul(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
) -> u64 {
    if nptr.is_null() {
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0;
    }

    let mut i = unsafe { wc_skip_ws(nptr, 0) };

    // POSIX: strtoul/wcstoul accept an optional sign.  A '-' means
    // the result is the unsigned wrapping negation of the parsed value.
    let negative = unsafe { *nptr.add(i) } == 0x2d; // '-'
    if negative || unsafe { *nptr.add(i) } == 0x2b { // '+'
        i = i.wrapping_add(1);
    }

    let (actual_base, new_i, before_prefix) = unsafe { wc_detect_base(nptr, i, base) };
    i = new_i;
    let _start = i;
    let mut result: u64 = 0;
    let base_u64 = actual_base as u64;
    let cutoff = u64::MAX / base_u64;
    let cutlim = u64::MAX % base_u64;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 { break; }
        let d = wc_digit(wc, actual_base);
        if d < 0 { break; }
        any_digits = true;
        let d_u64 = d as u64;

        if result > cutoff || (result == cutoff && d_u64 > cutlim) {
            overflow = true;
            // Continue parsing to set endptr correctly per POSIX.
        } else if !overflow {
            result = result * base_u64 + d_u64;
        }
        i = i.wrapping_add(1);
    }

    // If no digits were parsed after a "0x"/"0X" prefix, the leading
    // '0' is still a valid digit.  Roll back to just past the '0'.
    if !any_digits && i != before_prefix {
        i = before_prefix.wrapping_add(1);
        any_digits = true;
    }

    if !endptr.is_null() {
        unsafe { *endptr = if !any_digits { nptr } else { nptr.add(i) }; }
    }

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return u64::MAX;
    }

    if negative { result.wrapping_neg() } else { result }
}

/// `wcstoll` — convert a wide string to `long long` (`i64`).
///
/// On LP64, identical to `wcstol`.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoll(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
) -> i64 {
    unsafe { wcstol(nptr, endptr, base) }
}

/// `wcstoull` — convert a wide string to `unsigned long long` (`u64`).
///
/// On LP64, identical to `wcstoul`.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoull(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
) -> u64 {
    unsafe { wcstoul(nptr, endptr, base) }
}

/// `wcstod` — convert a wide string to `f64`.
///
/// Parses `[sign] digits [. digits] [e [sign] digits]`.  Uses a small
/// byte buffer to convert the ASCII-range wide characters to a multibyte
/// string, then delegates to `strtod`.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
pub unsafe extern "C" fn wcstod(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
) -> f64 {
    if nptr.is_null() {
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0.0;
    }

    let mut i = unsafe { wc_skip_ws(nptr, 0) };
    let start = i;

    // Collect ASCII-range float characters into a byte buffer.
    let mut buf = [0u8; 64];
    let mut bi: usize = 0;

    while bi < 62 {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 { break; }
        match wc {
            // '+', '-', '.', '0'-'9', 'E', 'e', plus 'i','n','f','a','t','y'
            // for "inf"/"infinity"/"nan" parsing.
            0x2b | 0x2d | 0x2e | 0x30..=0x39 | 0x45 | 0x65
            | 0x49 | 0x69 | 0x4e | 0x6e | 0x46 | 0x66
            | 0x41 | 0x61 | 0x54 | 0x74 | 0x59 | 0x79
            | 0x28 | 0x29 // '(' and ')' for nan(payload)
            => {
                buf[bi] = wc as u8;
                bi = bi.wrapping_add(1);
                i = i.wrapping_add(1);
            }
            _ => break,
        }
    }
    buf[bi] = 0;

    if bi == 0 {
        if !endptr.is_null() { unsafe { *endptr = nptr; } }
        return 0.0;
    }

    let mut byte_end: *const u8 = core::ptr::null();
    let val = unsafe { crate::stdlib::strtod(buf.as_ptr(), &raw mut byte_end) };

    if !endptr.is_null() {
        let bytes_consumed = if byte_end.is_null() {
            0usize
        } else {
            unsafe { byte_end.offset_from(buf.as_ptr()) as usize }
        };
        unsafe { *endptr = nptr.add(start.wrapping_add(bytes_consumed)); }
    }

    val
}

/// `wcstof` — convert a wide string to `f32`.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstof(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
) -> f32 {
    unsafe { wcstod(nptr, endptr) as f32 }
}

// ---------------------------------------------------------------------------
// nl_langinfo
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
// Wide string search/span functions
// ---------------------------------------------------------------------------

/// Find the first wide character in `s` that is in `accept`.
///
/// Returns the length of the initial segment of `s` consisting
/// entirely of wide characters in `accept`.
///
/// # Safety
///
/// Both strings must be valid null-terminated wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsspn(s: *const WcharT, accept: *const WcharT) -> usize {
    let mut count: usize = 0;
    loop {
        let ch = unsafe { *s.add(count) };
        if ch == 0 {
            break;
        }
        // Check if ch is in accept.
        let mut found = false;
        let mut j: usize = 0;
        loop {
            let a = unsafe { *accept.add(j) };
            if a == 0 {
                break;
            }
            if a == ch {
                found = true;
                break;
            }
            j = j.wrapping_add(1);
        }
        if !found {
            break;
        }
        count = count.wrapping_add(1);
    }
    count
}

/// Find the length of the initial segment of `s` consisting
/// entirely of wide characters NOT in `reject`.
///
/// # Safety
///
/// Both strings must be valid null-terminated wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscspn(s: *const WcharT, reject: *const WcharT) -> usize {
    let mut count: usize = 0;
    loop {
        let ch = unsafe { *s.add(count) };
        if ch == 0 {
            break;
        }
        // Check if ch is in reject.
        let mut j: usize = 0;
        loop {
            let r = unsafe { *reject.add(j) };
            if r == 0 {
                break;
            }
            if r == ch {
                return count;
            }
            j = j.wrapping_add(1);
        }
        count = count.wrapping_add(1);
    }
    count
}

/// Find the first wide character in `s` that is in `accept`.
///
/// Returns a pointer to the first matching character, or NULL if none found.
///
/// # Safety
///
/// Both strings must be valid null-terminated wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcspbrk(s: *const WcharT, accept: *const WcharT) -> *const WcharT {
    let mut i: usize = 0;
    loop {
        let ch = unsafe { *s.add(i) };
        if ch == 0 {
            return core::ptr::null();
        }
        let mut j: usize = 0;
        loop {
            let a = unsafe { *accept.add(j) };
            if a == 0 {
                break;
            }
            if a == ch {
                return unsafe { s.add(i) };
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
}

/// Tokenize a wide string.
///
/// On the first call, `s` is the string to tokenize.  On subsequent
/// calls, pass NULL as `s` to continue tokenizing the same string.
/// The `saveptr` state is used (thread-safe version of strtok).
///
/// # Safety
///
/// `delim` must be a valid null-terminated wide string.
/// `saveptr` must point to a valid `*mut WcharT` (used as state).
/// On first call, `s` must be a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstok(
    s: *mut WcharT,
    delim: *const WcharT,
    saveptr: *mut *mut WcharT,
) -> *mut WcharT {
    // Determine start position.
    let mut ptr = if s.is_null() {
        let saved = unsafe { *saveptr };
        if saved.is_null() {
            return core::ptr::null_mut();
        }
        saved
    } else {
        s
    };

    // Skip leading delimiters.
    loop {
        let ch = unsafe { *ptr };
        if ch == 0 {
            unsafe { *saveptr = core::ptr::null_mut(); }
            return core::ptr::null_mut();
        }
        if !wchar_in_set(ch, delim) {
            break;
        }
        ptr = unsafe { ptr.add(1) };
    }

    // ptr now points to start of token.
    let token_start = ptr;

    // Find end of token.
    loop {
        let ch = unsafe { *ptr };
        if ch == 0 {
            unsafe { *saveptr = core::ptr::null_mut(); }
            break;
        }
        if wchar_in_set(ch, delim) {
            unsafe { *ptr = 0; }
            unsafe { *saveptr = ptr.add(1); }
            break;
        }
        ptr = unsafe { ptr.add(1) };
    }

    token_start
}

/// Helper: check if `wc` is in the null-terminated set.
fn wchar_in_set(wc: WcharT, set: *const WcharT) -> bool {
    let mut j: usize = 0;
    loop {
        let s = unsafe { *set.add(j) };
        if s == 0 {
            return false;
        }
        if s == wc {
            return true;
        }
        j = j.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// wcsdup
// ---------------------------------------------------------------------------

/// Duplicate a wide string.
///
/// Allocates memory for a copy of `s` using `malloc`.  The caller
/// must free the result with `free()`.
///
/// # Safety
///
/// `s` must be a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsdup(s: *const WcharT) -> *mut WcharT {
    if s.is_null() {
        return core::ptr::null_mut();
    }

    let len = unsafe { wcslen(s) };
    let size = len.wrapping_add(1).wrapping_mul(core::mem::size_of::<WcharT>());

    let raw_ptr = crate::malloc::malloc(size);
    if raw_ptr.is_null() {
        return core::ptr::null_mut();
    }

    // Copy including the null terminator.
    // SAFETY: malloc returned aligned memory of sufficient size for (len+1) WcharT values.
    // Our malloc implementation guarantees at least 8-byte alignment, which satisfies
    // WcharT (i32, 4-byte alignment).
    #[allow(clippy::cast_ptr_alignment)]
    let out_ptr = raw_ptr.cast::<WcharT>();
    let mut i: usize = 0;
    while i <= len {
        unsafe { *out_ptr.add(i) = *s.add(i); }
        i = i.wrapping_add(1);
    }

    out_ptr
}

// ---------------------------------------------------------------------------
// Wide string collation
// ---------------------------------------------------------------------------

/// Locale-aware wide string comparison.
///
/// Since we only support the C locale, this is identical to `wcscmp`.
///
/// # Safety
///
/// Both `s1` and `s2` must be valid null-terminated wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscoll(s1: *const WcharT, s2: *const WcharT) -> i32 {
    unsafe { wcscmp(s1, s2) }
}

/// Locale-aware wide string comparison (locale variant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscoll_l(s1: *const WcharT, s2: *const WcharT, _locale: usize) -> i32 {
    unsafe { wcscmp(s1, s2) }
}

/// Transform a wide string for locale-aware comparison.
///
/// Copies at most `n` wide characters of `src` into `dest`.  Since
/// we only support the C locale, this is just `wcsncpy` semantics.
/// Returns the length of `src` (not counting null).
///
/// # Safety
///
/// `dest` must be valid for `n` wide characters.  `src` must be
/// a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsxfrm(dest: *mut WcharT, src: *const WcharT, n: usize) -> usize {
    let len = unsafe { wcslen(src) };
    if n > 0 {
        unsafe { wcsncpy(dest, src, n); }
    }
    len
}

/// Transform a wide string for locale-aware comparison (locale variant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsxfrm_l(dest: *mut WcharT, src: *const WcharT, n: usize, _locale: usize) -> usize {
    unsafe { wcsxfrm(dest, src, n) }
}

// ---------------------------------------------------------------------------
// Restartable string conversions: mbsrtowcs / wcsrtombs
// ---------------------------------------------------------------------------

/// Convert a multibyte string to a wide string (restartable).
///
/// Converts at most `len` wide characters from the multibyte string
/// pointed to by `*src`.  On success, `*src` is updated to point past
/// the last byte consumed (or set to NULL if the entire string was
/// converted including the null terminator).
///
/// # Safety
///
/// `src` must point to a valid `*const u8` pointer to a multibyte string.
/// `dst` must be valid for `len` wide characters (or may be NULL for counting).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbsrtowcs(
    dst: *mut WcharT,
    src: *mut *const u8,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    // Delegate to mbsnrtowcs with nms = SIZE_MAX (no byte limit).
    unsafe { mbsnrtowcs(dst, src, usize::MAX, len, ps) }
}

/// Convert a multibyte string to a wide string (restartable, n-limited).
///
/// Converts at most `len` wide characters, consuming at most `nms` bytes
/// from the multibyte string pointed to by `*src`.
///
/// # Safety
///
/// Same as `mbsrtowcs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbsnrtowcs(
    dst: *mut WcharT,
    src: *mut *const u8,
    nms: usize,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    if src.is_null() || unsafe { (*src).is_null() } {
        return 0;
    }

    let mut s = unsafe { *src };
    let mut written: usize = 0;
    let mut bytes_consumed: usize = 0;

    while written < len && bytes_consumed < nms {
        let remaining_bytes = nms.saturating_sub(bytes_consumed);
        // Limit n to remaining bytes available.
        let n = if remaining_bytes > 4 { 4 } else { remaining_bytes };
        if n == 0 {
            break;
        }

        let mut wc: WcharT = 0;
        let pwc = if dst.is_null() {
            core::ptr::null_mut()
        } else {
            &raw mut wc
        };

        let ret = unsafe { mbrtowc(pwc, s, n, ps) };

        if ret == 0 {
            // Null terminator encountered.
            if !dst.is_null() {
                unsafe { *dst.add(written) = 0; }
            }
            unsafe { *src = core::ptr::null(); }
            return written;
        }
        if ret == usize::MAX {
            // Encoding error.
            crate::errno::set_errno(crate::errno::EILSEQ);
            return usize::MAX;
        }
        if ret == usize::MAX.wrapping_sub(1) {
            // Incomplete sequence and we've run out of bytes.
            break;
        }
        if !dst.is_null() {
            unsafe { *dst.add(written) = wc; }
        }
        s = unsafe { s.add(ret) };
        bytes_consumed = bytes_consumed.wrapping_add(ret);
        written = written.wrapping_add(1);
    }

    unsafe { *src = s; }
    written
}

/// Convert a wide string to a multibyte string (restartable).
///
/// Converts at most `len` bytes worth of wide characters from the
/// wide string pointed to by `*src`.
///
/// # Safety
///
/// `src` must point to a valid `*const WcharT` pointer to a wide string.
/// `dst` must be valid for `len` bytes (or may be NULL for counting).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsrtombs(
    dst: *mut u8,
    src: *mut *const WcharT,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    // Delegate to wcsnrtombs with nwc = SIZE_MAX (no character limit).
    unsafe { wcsnrtombs(dst, src, usize::MAX, len, ps) }
}

/// Convert a wide string to a multibyte string (restartable, n-limited).
///
/// Converts at most `nwc` wide characters, producing at most `len` bytes.
///
/// # Safety
///
/// Same as `wcsrtombs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing)]
pub unsafe extern "C" fn wcsnrtombs(
    dst: *mut u8,
    src: *mut *const WcharT,
    nwc: usize,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    if src.is_null() || unsafe { (*src).is_null() } {
        return 0;
    }

    let mut s = unsafe { *src };
    let mut written: usize = 0;
    let mut chars_consumed: usize = 0;
    let mut buf: [u8; 4] = [0; 4];

    while chars_consumed < nwc {
        let wc = unsafe { *s };

        // Encode into temporary buffer to know the byte count.
        let ret = unsafe { wcrtomb(buf.as_mut_ptr(), wc, ps) };

        if ret == usize::MAX {
            // Encoding error.
            crate::errno::set_errno(crate::errno::EILSEQ);
            return usize::MAX;
        }

        // Check if output would overflow.
        if written.wrapping_add(ret) > len {
            break;
        }

        // Copy to destination.
        if !dst.is_null() {
            let mut k: usize = 0;
            while k < ret {
                unsafe { *dst.add(written.wrapping_add(k)) = buf[k]; }
                k = k.wrapping_add(1);
            }
        }

        written = written.wrapping_add(ret);
        chars_consumed = chars_consumed.wrapping_add(1);

        if wc == 0 {
            // Null terminator — don't count it in written, set src to null.
            unsafe { *src = core::ptr::null(); }
            return written.wrapping_sub(ret); // Exclude the null byte from count.
        }

        s = unsafe { s.add(1) };
    }

    unsafe { *src = s; }
    written
}

// ---------------------------------------------------------------------------
// nl_langinfo stub
// ---------------------------------------------------------------------------

/// Query locale-dependent information.
///
/// Returns reasonable defaults for the C locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
// Wide string case-insensitive comparison
// ---------------------------------------------------------------------------

/// Case-insensitive wide string comparison.
///
/// Compares two null-terminated wide strings, converting each character
/// to lowercase before comparison.  Returns 0 if equal, negative if
/// `s1 < s2`, positive if `s1 > s2`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscasecmp(s1: *const WcharT, s2: *const WcharT) -> i32 {
    if s1.is_null() || s2.is_null() {
        // Defensive: POSIX says behaviour is undefined for null.
        return 0;
    }
    let mut i: usize = 0;
    loop {
        let c1 = towlower(unsafe { *s1.add(i) });
        let c2 = towlower(unsafe { *s2.add(i) });
        if c1 != c2 {
            return c1.wrapping_sub(c2);
        }
        // Both equal — if nul terminator, strings are identical.
        if c1 == 0 {
            return 0;
        }
        i = i.wrapping_add(1);
    }
}

/// Case-insensitive wide string comparison with length limit.
///
/// Compares at most `n` wide characters from `s1` and `s2`,
/// converting to lowercase before comparison.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsncasecmp(
    s1: *const WcharT,
    s2: *const WcharT,
    n: usize,
) -> i32 {
    if n == 0 || s1.is_null() || s2.is_null() {
        return 0;
    }
    let mut i: usize = 0;
    while i < n {
        let c1 = towlower(unsafe { *s1.add(i) });
        let c2 = towlower(unsafe { *s2.add(i) });
        if c1 != c2 {
            return c1.wrapping_sub(c2);
        }
        if c1 == 0 {
            return 0;
        }
        i = i.wrapping_add(1);
    }
    0
}

// ---------------------------------------------------------------------------
// Wide character I/O
// ---------------------------------------------------------------------------

/// Write a wide character to a stream.
///
/// Encodes `wc` as UTF-8 and writes the bytes to `stream`.
/// Returns `wc` on success, `WEOF` on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing)]
pub unsafe extern "C" fn fputwc(wc: WcharT, stream: *mut u8) -> WcharT {
    let cp = wc as u32;
    let mut buf = [0u8; 4];
    let len = if cp < 0x80 {
        buf[0] = cp as u8;
        1
    } else if cp < 0x800 {
        buf[0] = 0xC0 | (cp >> 6) as u8;
        buf[1] = 0x80 | (cp & 0x3F) as u8;
        2
    } else if cp < 0x1_0000 {
        buf[0] = 0xE0 | (cp >> 12) as u8;
        buf[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        buf[2] = 0x80 | (cp & 0x3F) as u8;
        3
    } else if cp <= 0x10_FFFF {
        buf[0] = 0xF0 | (cp >> 18) as u8;
        buf[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
        buf[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
        buf[3] = 0x80 | (cp & 0x3F) as u8;
        4
    } else {
        crate::errno::set_errno(crate::errno::EILSEQ);
        return WEOF;
    };

    let mut i: usize = 0;
    while i < len {
        if crate::stdio::fputc(i32::from(buf[i]), stream) < 0 {
            return WEOF;
        }
        i = i.wrapping_add(1);
    }
    wc
}

/// Read a wide character from a stream.
///
/// Reads one UTF-8 encoded character from `stream` and returns
/// the wide character value.  Returns `WEOF` on error or EOF.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetwc(stream: *mut u8) -> WcharT {
    let first = crate::stdio::fgetc(stream);
    if first < 0 {
        return WEOF;
    }
    let b0 = first as u8;

    // Single-byte ASCII.
    if b0 < 0x80 {
        return WcharT::from(b0);
    }

    // Determine expected length and initial bits.
    let (expected_len, mut cp): (usize, u32) = if b0 & 0xE0 == 0xC0 {
        (2, u32::from(b0 & 0x1F))
    } else if b0 & 0xF0 == 0xE0 {
        (3, u32::from(b0 & 0x0F))
    } else if b0 & 0xF8 == 0xF0 {
        (4, u32::from(b0 & 0x07))
    } else {
        crate::errno::set_errno(crate::errno::EILSEQ);
        return WEOF;
    };

    // Read continuation bytes.
    let mut i: usize = 1;
    while i < expected_len {
        let next = crate::stdio::fgetc(stream);
        if next < 0 {
            return WEOF;
        }
        let nb = next as u8;
        if nb & 0xC0 != 0x80 {
            crate::errno::set_errno(crate::errno::EILSEQ);
            return WEOF;
        }
        cp = (cp << 6) | u32::from(nb & 0x3F);
        i = i.wrapping_add(1);
    }

    cp as WcharT
}

/// Write a wide character to a stream (alias for `fputwc`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putwc(wc: WcharT, stream: *mut u8) -> WcharT {
    unsafe { fputwc(wc, stream) }
}

/// Read a wide character from a stream (alias for `fgetwc`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getwc(stream: *mut u8) -> WcharT {
    unsafe { fgetwc(stream) }
}

/// Write a wide character to stdout.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putwchar(wc: WcharT) -> WcharT {
    // Use STDOUT_SENTINEL explicitly — dangling_mut::<u8>() happens to
    // return the same value today but is not guaranteed to.
    unsafe { fputwc(wc, crate::stdio::STDOUT_SENTINEL as *mut u8) }
}

/// Read a wide character from stdin.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getwchar() -> WcharT {
    // SAFETY: STDIN_SENTINEL (null) is the stdio convention.
    unsafe { fgetwc(core::ptr::null_mut()) }
}

/// Push back a wide character onto a stream.
///
/// Only one character of pushback is guaranteed.  Returns `wc` on
/// success, `WEOF` if pushback fails.  We encode the wide character
/// back to UTF-8 and push the first byte via `ungetc`.  Since our
/// stdio only supports one byte of pushback, this only works reliably
/// for ASCII characters; multi-byte pushback is best-effort.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ungetwc(wc: WcharT, stream: *mut u8) -> WcharT {
    if wc == WEOF {
        return WEOF;
    }
    // For ASCII, push back directly.
    let cp = wc as u32;
    if cp < 0x80 {
        if crate::stdio::ungetc(cp as i32, stream) < 0 {
            return WEOF;
        }
        return wc;
    }
    // Multi-byte: best-effort single-byte pushback of first UTF-8 byte.
    // This is a known limitation — full multi-byte pushback would
    // require expanding the stdio ungetc buffer.
    crate::errno::set_errno(crate::errno::EILSEQ);
    WEOF
}

/// Write a wide string to a stream.
///
/// Writes each character of the null-terminated wide string `s` to
/// `stream` using `fputwc`.  Does NOT write the null terminator.
/// Returns a non-negative value on success, `WEOF` on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputws(s: *const WcharT, stream: *mut u8) -> i32 {
    if s.is_null() {
        return -1;
    }
    let mut i: usize = 0;
    loop {
        let wc = unsafe { *s.add(i) };
        if wc == 0 {
            return 0; // Success.
        }
        if unsafe { fputwc(wc, stream) } == WEOF {
            return -1;
        }
        i = i.wrapping_add(1);
    }
}

/// Read a wide string from a stream.
///
/// Reads at most `n-1` wide characters from `stream` into `ws`,
/// stopping at a newline (which is included) or EOF.  The resulting
/// string is null-terminated.  Returns `ws` on success, null on
/// error or EOF with no characters read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetws(
    ws: *mut WcharT,
    n: i32,
    stream: *mut u8,
) -> *mut WcharT {
    if ws.is_null() || n <= 0 {
        return core::ptr::null_mut();
    }
    let max = (n as usize).wrapping_sub(1); // Reserve space for null terminator.
    let mut count: usize = 0;
    while count < max {
        let wc = unsafe { fgetwc(stream) };
        if wc == WEOF {
            if count == 0 {
                return core::ptr::null_mut(); // EOF with nothing read.
            }
            break;
        }
        unsafe { *ws.add(count) = wc; }
        count = count.wrapping_add(1);
        // Stop after newline.
        if wc == WcharT::from(b'\n') {
            break;
        }
    }
    // Null-terminate.
    unsafe { *ws.add(count) = 0; }
    ws
}

/// Copy wide characters, returning pointer past last written.
///
/// Like `wmemcpy` but returns a pointer to the wide character after
/// the last one written (i.e., `dest + n`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wmempcpy(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    if !dest.is_null() && !src.is_null() {
        let mut i: usize = 0;
        while i < n {
            unsafe { *dest.add(i) = *src.add(i); }
            i = i.wrapping_add(1);
        }
    }
    // SAFETY: dest + n is one past the last element written.
    unsafe { dest.add(n) }
}

// ---------------------------------------------------------------------------
// Wide strftime
// ---------------------------------------------------------------------------

/// Format a date/time as a wide string.
///
/// Converts the wide format string to a narrow (UTF-8) format, calls
/// `strftime`, then widens the result.  This works correctly because
/// `strftime` format specifiers are all ASCII, and on our UTF-8 locale
/// the output of `strftime` is valid UTF-8.
///
/// Returns the number of wide characters written (excluding the null
/// terminator), or 0 if the result doesn't fit in `maxsize` wide
/// characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::indexing_slicing)]
pub unsafe extern "C" fn wcsftime(
    wcs: *mut WcharT,
    maxsize: usize,
    format: *const WcharT,
    tm: *const crate::time::Tm,
) -> usize {
    const FMT_BUF_SIZE: usize = 256;
    const OUT_BUF_SIZE: usize = 1024;

    if wcs.is_null() || format.is_null() || tm.is_null() || maxsize == 0 {
        return 0;
    }

    // Convert wide format to narrow.  strftime format specifiers are all
    // ASCII (%Y, %m, %d, etc.) so this is a simple byte-truncation.
    let mut fmt_buf = [0u8; FMT_BUF_SIZE];
    let mut fi: usize = 0;
    loop {
        let wc = unsafe { *format.add(fi) };
        if wc == 0 || fi >= FMT_BUF_SIZE.wrapping_sub(1) {
            break;
        }
        // Truncate to byte — format specifiers are ASCII.
        fmt_buf[fi] = (wc & 0x7F) as u8;
        fi = fi.wrapping_add(1);
    }
    fmt_buf[fi] = 0;

    // Call narrow strftime.  Use a buffer 4× maxsize since UTF-8 can
    // use up to 4 bytes per character.
    let mut out_buf = [0u8; OUT_BUF_SIZE];
    let narrow_max = maxsize.wrapping_mul(4).min(OUT_BUF_SIZE);
    let len = unsafe {
        crate::time::strftime(
            out_buf.as_mut_ptr(),
            narrow_max,
            fmt_buf.as_ptr(),
            tm,
        )
    };

    if len == 0 {
        // strftime failed or result doesn't fit.
        unsafe { *wcs = 0; }
        return 0;
    }

    // Widen the narrow output to wide characters.  strftime output
    // on our UTF-8 locale is valid UTF-8 (month/day names are ASCII).
    let mut wi: usize = 0;
    let mut bi: usize = 0;
    let limit = maxsize.wrapping_sub(1); // Leave room for null terminator.
    while bi < len && wi < limit {
        let b = out_buf[bi];
        // Determine UTF-8 sequence length from lead byte.
        let seq_len = if b < 0x80 { 1_usize }
            else if b & 0xE0 == 0xC0 { 2 }
            else if b & 0xF0 == 0xE0 { 3 }
            else if b & 0xF8 == 0xF0 { 4 }
            else { 1 }; // Invalid lead → treat as single byte.

        if bi.wrapping_add(seq_len) > len {
            break; // Incomplete sequence at end.
        }

        if seq_len == 1 {
            unsafe { *wcs.add(wi) = WcharT::from(b); }
        } else if let Some(cp) = utf8_decode(&out_buf[bi..bi.wrapping_add(seq_len)], seq_len) {
            unsafe { *wcs.add(wi) = cp as WcharT; }
        } else {
            // Invalid UTF-8 — emit replacement character.
            unsafe { *wcs.add(wi) = 0xFFFD; }
        }
        bi = bi.wrapping_add(seq_len);
        wi = wi.wrapping_add(1);
    }

    // Null-terminate.
    unsafe { *wcs.add(wi) = 0; }
    wi
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

    // -- wcstol base-detection bug regression tests --

    /// Helper: build a null-terminated wide string from ASCII bytes.
    fn wcs_from_ascii(s: &[u8]) -> [WcharT; 64] {
        let mut buf = [0i32; 64];
        for (i, &b) in s.iter().enumerate() {
            if i >= 63 { break; }
            buf[i] = b as WcharT;
        }
        buf
    }

    #[test]
    fn test_wcstol_zero_base_auto() {
        // wcstol(L"0", &end, 0) should return 0 with endptr past '0'.
        let s = wcs_from_ascii(b"0\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
        // endptr should point past the '0' (to the null terminator).
        assert_eq!(end, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcstol_octal_leading_zero() {
        // "077" in base 0 → octal 77 = decimal 63
        let s = wcs_from_ascii(b"077\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 63);
        assert_eq!(end, unsafe { s.as_ptr().add(3) });
    }

    #[test]
    fn test_wcstol_hex_prefix() {
        let s = wcs_from_ascii(b"0xFF\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 255);
        assert_eq!(end, unsafe { s.as_ptr().add(4) });
    }

    #[test]
    fn test_wcstol_hex_no_digits_after_0x() {
        // "0x" with no hex digits: should parse "0" and endptr at '0'+1.
        let s = wcs_from_ascii(b"0x\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
        // endptr should point just past the '0', not past the 'x'.
        assert_eq!(end, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcstol_hex_0x_then_non_hex() {
        // "0xG" → parse "0", endptr past '0'.
        let s = wcs_from_ascii(b"0xG\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
        assert_eq!(end, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcstol_base10() {
        let s = wcs_from_ascii(b"  -42\0");
        let val = unsafe { wcstol(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, -42);
    }

    #[test]
    fn test_wcstol_empty_string() {
        let s = wcs_from_ascii(b"\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 0);
        assert_eq!(end, s.as_ptr()); // No digits consumed.
    }

    #[test]
    fn test_wcstoul_zero_base_auto() {
        let s = wcs_from_ascii(b"0\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstoul(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
        assert_eq!(end, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcstoul_hex_no_digits_after_0x() {
        let s = wcs_from_ascii(b"0x\0");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstoul(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
        assert_eq!(end, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcstol_negative_sign() {
        let s = wcs_from_ascii(b"-123\0");
        let val = unsafe { wcstol(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, -123);
    }

    #[test]
    fn test_wcstol_positive_sign() {
        let s = wcs_from_ascii(b"+456\0");
        let val = unsafe { wcstol(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, 456);
    }

    #[test]
    fn test_wcstol_invalid_base() {
        let s = wcs_from_ascii(b"123\0");
        crate::errno::set_errno(0);
        let val = unsafe { wcstol(s.as_ptr(), core::ptr::null_mut(), 37) };
        assert_eq!(val, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // wcwidth
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcwidth_null() {
        assert_eq!(wcwidth(0), 0);
    }

    #[test]
    fn test_wcwidth_ascii_printable() {
        assert_eq!(wcwidth(b'A' as i32), 1);
        assert_eq!(wcwidth(b' ' as i32), 1);
        assert_eq!(wcwidth(b'~' as i32), 1);
    }

    #[test]
    fn test_wcwidth_c0_control() {
        assert_eq!(wcwidth(0x01), -1); // SOH
        assert_eq!(wcwidth(0x0a), -1); // LF
        assert_eq!(wcwidth(0x1f), -1); // US
        assert_eq!(wcwidth(0x7f), -1); // DEL
    }

    #[test]
    fn test_wcwidth_c1_control() {
        // C1 controls (0x80-0x9F) should return -1.
        assert_eq!(wcwidth(0x80), -1);
        assert_eq!(wcwidth(0x85), -1); // NEL
        assert_eq!(wcwidth(0x9f), -1);
    }

    #[test]
    fn test_wcwidth_cjk() {
        // CJK ideograph — fullwidth, width 2.
        assert_eq!(wcwidth(0x4e2d), 2); // 中
        assert_eq!(wcwidth(0x6587), 2); // 文
    }

    #[test]
    fn test_wcwidth_hangul() {
        assert_eq!(wcwidth(0xac00), 2); // 가
        assert_eq!(wcwidth(0xd7a3), 2); // Last Hangul syllable
    }

    #[test]
    fn test_wcwidth_fullwidth_forms() {
        assert_eq!(wcwidth(0xff01), 2); // ！ (fullwidth !)
        assert_eq!(wcwidth(0xff21), 2); // Ａ (fullwidth A)
    }

    #[test]
    fn test_wcwidth_combining_marks() {
        // Combining diacritical marks have width 0.
        assert_eq!(wcwidth(0x0300), 0); // Combining Grave Accent
        assert_eq!(wcwidth(0x0301), 0); // Combining Acute Accent
        assert_eq!(wcwidth(0x036f), 0); // End of combining marks block
    }

    #[test]
    fn test_wcwidth_zero_width_chars() {
        assert_eq!(wcwidth(0x200b), 0); // Zero-width space
        assert_eq!(wcwidth(0x200d), 0); // Zero-width joiner (used in emoji sequences)
        assert_eq!(wcwidth(0xfeff), 0); // BOM / ZWNBSP
    }

    #[test]
    fn test_wcwidth_variation_selectors() {
        assert_eq!(wcwidth(0xfe00), 0);  // VS1
        assert_eq!(wcwidth(0xfe0f), 0);  // VS16 (emoji presentation)
    }

    #[test]
    fn test_wcwidth_latin_above_c1() {
        // Latin characters above C1 range should be width 1.
        assert_eq!(wcwidth(0xa0), 1);  // NBSP
        assert_eq!(wcwidth(0xc0), 1);  // À
        assert_eq!(wcwidth(0xff), 1);  // ÿ
    }

    #[test]
    fn test_wcwidth_emoji() {
        // Emoji in SMP should be width 2.
        assert_eq!(wcwidth(0x1f600), 2); // 😀
        assert_eq!(wcwidth(0x1f680), 2); // 🚀
    }

    // -----------------------------------------------------------------------
    // iswcntrl / iswprint — C1 control handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_iswcntrl_c1_controls() {
        // C1 controls (0x80-0x9F) should be classified as control chars.
        assert_ne!(iswcntrl(0x80), 0);
        assert_ne!(iswcntrl(0x85), 0); // NEL
        assert_ne!(iswcntrl(0x9f), 0);
    }

    #[test]
    fn test_iswcntrl_not_above_c1() {
        // Characters at 0xA0 and above are NOT control characters.
        assert_eq!(iswcntrl(0xa0), 0);  // NBSP
        assert_eq!(iswcntrl(0xff), 0);  // ÿ
    }

    #[test]
    fn test_iswprint_c1_controls() {
        // C1 controls should NOT be printable.
        assert_eq!(iswprint(0x80), 0);
        assert_eq!(iswprint(0x85), 0);
        assert_eq!(iswprint(0x9f), 0);
    }

    #[test]
    fn test_iswprint_above_c1() {
        // Characters at 0xA0 and above should be printable.
        assert_ne!(iswprint(0xa0), 0);  // NBSP
        assert_ne!(iswprint(0xc0), 0);  // À
    }

    // -----------------------------------------------------------------------
    // wcstod
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcstod_basic() {
        let s: &[WcharT] = &[b'3' as i32, b'.' as i32, b'1' as i32, b'4' as i32, 0];
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert!((val - 3.14).abs() < 1e-10);
        assert_eq!(end, unsafe { s.as_ptr().add(4) });
    }

    #[test]
    fn test_wcstod_negative() {
        let s: &[WcharT] = &[b'-' as i32, b'2' as i32, b'.' as i32, b'5' as i32, 0];
        let val = unsafe { wcstod(s.as_ptr(), core::ptr::null_mut()) };
        assert!((val - (-2.5)).abs() < 1e-10);
    }

    #[test]
    fn test_wcstod_scientific() {
        // "1e3" = 1000.0
        let s: &[WcharT] = &[b'1' as i32, b'e' as i32, b'3' as i32, 0];
        let val = unsafe { wcstod(s.as_ptr(), core::ptr::null_mut()) };
        assert!((val - 1000.0).abs() < 1e-10);
    }

    #[test]
    fn test_wcstod_leading_whitespace() {
        // "  42" with leading wide spaces.
        let s: &[WcharT] = &[b' ' as i32, b' ' as i32, b'4' as i32, b'2' as i32, 0];
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert!((val - 42.0).abs() < 1e-10);
    }

    #[test]
    fn test_wcstod_no_digits() {
        // "abc" — no valid float.
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 0.0);
        assert_eq!(end, s.as_ptr()); // endptr set to start.
    }

    #[test]
    fn test_wcstod_infinity() {
        let s: &[WcharT] = &[b'i' as i32, b'n' as i32, b'f' as i32, 0];
        let val = unsafe { wcstod(s.as_ptr(), core::ptr::null_mut()) };
        assert!(val.is_infinite() && val > 0.0);
    }

    #[test]
    fn test_wcstod_nan() {
        let s: &[WcharT] = &[b'n' as i32, b'a' as i32, b'n' as i32, 0];
        let val = unsafe { wcstod(s.as_ptr(), core::ptr::null_mut()) };
        assert!(val.is_nan());
    }

    #[test]
    fn test_wcstod_trailing_text() {
        // "12.5xyz" — parse "12.5", endptr at 'x'.
        let s: &[WcharT] = &[
            b'1' as i32, b'2' as i32, b'.' as i32, b'5' as i32,
            b'x' as i32, b'y' as i32, b'z' as i32, 0,
        ];
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert!((val - 12.5).abs() < 1e-10);
        assert_eq!(end, unsafe { s.as_ptr().add(4) });
    }

    #[test]
    fn test_wcstod_null() {
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(core::ptr::null(), &raw mut end) };
        assert_eq!(val, 0.0);
    }

    // -----------------------------------------------------------------------
    // wcsstr
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcsstr_found() {
        let hay: &[WcharT] = &[b'h' as i32, b'e' as i32, b'l' as i32, b'l' as i32, b'o' as i32, 0];
        let needle: &[WcharT] = &[b'l' as i32, b'l' as i32, 0];
        let ret = unsafe { wcsstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { hay.as_ptr().add(2) });
    }

    #[test]
    fn test_wcsstr_not_found() {
        let hay: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let needle: &[WcharT] = &[b'x' as i32, b'y' as i32, 0];
        let ret = unsafe { wcsstr(hay.as_ptr(), needle.as_ptr()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_wcsstr_empty_needle() {
        let hay: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        let needle: &[WcharT] = &[0];
        let ret = unsafe { wcsstr(hay.as_ptr(), needle.as_ptr()) };
        assert_eq!(ret, hay.as_ptr());
    }

    // -----------------------------------------------------------------------
    // wcsspn / wcscspn / wcspbrk
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcsspn_basic() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, b'x' as i32, 0];
        let accept: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let ret = unsafe { wcsspn(s.as_ptr(), accept.as_ptr()) };
        assert_eq!(ret, 3);
    }

    #[test]
    fn test_wcsspn_no_match() {
        let s: &[WcharT] = &[b'x' as i32, b'y' as i32, 0];
        let accept: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        assert_eq!(unsafe { wcsspn(s.as_ptr(), accept.as_ptr()) }, 0);
    }

    #[test]
    fn test_wcscspn_basic() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, b',' as i32, 0];
        let reject: &[WcharT] = &[b',' as i32, b';' as i32, 0];
        assert_eq!(unsafe { wcscspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    #[test]
    fn test_wcscspn_no_reject() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let reject: &[WcharT] = &[b'x' as i32, 0];
        assert_eq!(unsafe { wcscspn(s.as_ptr(), reject.as_ptr()) }, 3);
    }

    #[test]
    fn test_wcspbrk_found() {
        let s: &[WcharT] = &[b'h' as i32, b'e' as i32, b'l' as i32, b'l' as i32, b'o' as i32, 0];
        let accept: &[WcharT] = &[b'l' as i32, b'o' as i32, 0];
        let ret = unsafe { wcspbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { s.as_ptr().add(2) }); // first 'l'
    }

    #[test]
    fn test_wcspbrk_not_found() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        let accept: &[WcharT] = &[b'x' as i32, 0];
        let ret = unsafe { wcspbrk(s.as_ptr(), accept.as_ptr()) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // wcstok
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcstok_basic() {
        let mut data: [WcharT; 14] = [
            b'o' as i32, b'n' as i32, b'e' as i32, b',' as i32,
            b't' as i32, b'w' as i32, b'o' as i32, b',' as i32,
            b't' as i32, b'h' as i32, b'r' as i32, b'e' as i32, b'e' as i32,
            0,
        ];
        let delim: &[WcharT] = &[b',' as i32, 0];
        let mut save: *mut WcharT = core::ptr::null_mut();

        let tok1 = unsafe { wcstok(data.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(!tok1.is_null());
        assert_eq!(unsafe { wcslen(tok1) }, 3);

        let tok2 = unsafe { wcstok(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!tok2.is_null());
        assert_eq!(unsafe { wcslen(tok2) }, 3);

        let tok3 = unsafe { wcstok(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(!tok3.is_null());
        assert_eq!(unsafe { wcslen(tok3) }, 5);

        let tok4 = unsafe { wcstok(core::ptr::null_mut(), delim.as_ptr(), &raw mut save) };
        assert!(tok4.is_null());
    }

    #[test]
    fn test_wcstok_all_delimiters() {
        let mut data: [WcharT; 4] = [b',' as i32, b',' as i32, b',' as i32, 0];
        let delim: &[WcharT] = &[b',' as i32, 0];
        let mut save: *mut WcharT = core::ptr::null_mut();

        let tok = unsafe { wcstok(data.as_mut_ptr(), delim.as_ptr(), &raw mut save) };
        assert!(tok.is_null());
    }

    // -----------------------------------------------------------------------
    // wcscasecmp / wcsncasecmp
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcscasecmp_equal() {
        let s1: &[WcharT] = &[b'H' as i32, b'E' as i32, b'L' as i32, b'L' as i32, b'O' as i32, 0];
        let s2: &[WcharT] = &[b'h' as i32, b'e' as i32, b'l' as i32, b'l' as i32, b'o' as i32, 0];
        assert_eq!(unsafe { wcscasecmp(s1.as_ptr(), s2.as_ptr()) }, 0);
    }

    #[test]
    fn test_wcscasecmp_different() {
        let s1: &[WcharT] = &[b'A' as i32, b'B' as i32, b'C' as i32, 0];
        let s2: &[WcharT] = &[b'a' as i32, b'b' as i32, b'd' as i32, 0];
        assert!(unsafe { wcscasecmp(s1.as_ptr(), s2.as_ptr()) } < 0);
    }

    #[test]
    fn test_wcsncasecmp_limited() {
        let s1: &[WcharT] = &[b'A' as i32, b'B' as i32, b'X' as i32, 0];
        let s2: &[WcharT] = &[b'a' as i32, b'b' as i32, b'Y' as i32, 0];
        // First 2 chars match case-insensitively.
        assert_eq!(unsafe { wcsncasecmp(s1.as_ptr(), s2.as_ptr(), 2) }, 0);
        // But all 3 chars differ at position 2.
        assert!(unsafe { wcsncasecmp(s1.as_ptr(), s2.as_ptr(), 3) } < 0);
    }

    #[test]
    fn test_wcsncasecmp_zero_n() {
        let s1: &[WcharT] = &[b'A' as i32, 0];
        let s2: &[WcharT] = &[b'Z' as i32, 0];
        assert_eq!(unsafe { wcsncasecmp(s1.as_ptr(), s2.as_ptr(), 0) }, 0);
    }

    // -----------------------------------------------------------------------
    // wcscpy / wcsncpy / wcslen / wcscmp / wcsncmp
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcscpy_basic() {
        let src: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let mut dst: [WcharT; 4] = [0; 4];
        let ret = unsafe { wcscpy(dst.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(dst, [b'a' as i32, b'b' as i32, b'c' as i32, 0]);
    }

    #[test]
    fn test_wcslen_basic() {
        let s: &[WcharT] = &[b'h' as i32, b'i' as i32, 0];
        assert_eq!(unsafe { wcslen(s.as_ptr()) }, 2);
    }

    #[test]
    fn test_wcslen_empty() {
        let s: &[WcharT] = &[0];
        assert_eq!(unsafe { wcslen(s.as_ptr()) }, 0);
    }

    #[test]
    fn test_wcscmp_equal() {
        let s1: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        let s2: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        assert_eq!(unsafe { wcscmp(s1.as_ptr(), s2.as_ptr()) }, 0);
    }

    #[test]
    fn test_wcscmp_less() {
        let s1: &[WcharT] = &[b'a' as i32, 0];
        let s2: &[WcharT] = &[b'b' as i32, 0];
        assert!(unsafe { wcscmp(s1.as_ptr(), s2.as_ptr()) } < 0);
    }

    #[test]
    fn test_wcscmp_greater() {
        let s1: &[WcharT] = &[b'z' as i32, 0];
        let s2: &[WcharT] = &[b'a' as i32, 0];
        assert!(unsafe { wcscmp(s1.as_ptr(), s2.as_ptr()) } > 0);
    }

    #[test]
    fn test_wcsncmp_limited() {
        let s1: &[WcharT] = &[b'a' as i32, b'b' as i32, b'x' as i32, 0];
        let s2: &[WcharT] = &[b'a' as i32, b'b' as i32, b'y' as i32, 0];
        assert_eq!(unsafe { wcsncmp(s1.as_ptr(), s2.as_ptr(), 2) }, 0);
        assert!(unsafe { wcsncmp(s1.as_ptr(), s2.as_ptr(), 3) } < 0);
    }

    // -----------------------------------------------------------------------
    // wcschr / wcsrchr
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcschr_found() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'c' as i32, 0];
        let ret = unsafe { wcschr(s.as_ptr(), b'b' as i32) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcschr_not_found() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        let ret = unsafe { wcschr(s.as_ptr(), b'x' as i32) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_wcschr_null_terminator() {
        // Searching for NUL should find the terminator.
        let s: &[WcharT] = &[b'a' as i32, 0];
        let ret = unsafe { wcschr(s.as_ptr(), 0) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { s.as_ptr().add(1) });
    }

    #[test]
    fn test_wcsrchr_found() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, b'a' as i32, 0];
        let ret = unsafe { wcsrchr(s.as_ptr(), b'a' as i32) };
        assert!(!ret.is_null());
        // Should find the LAST 'a' at index 2.
        assert_eq!(ret, unsafe { s.as_ptr().add(2) });
    }

    #[test]
    fn test_wcsrchr_not_found() {
        let s: &[WcharT] = &[b'a' as i32, b'b' as i32, 0];
        let ret = unsafe { wcsrchr(s.as_ptr(), b'x' as i32) };
        assert!(ret.is_null());
    }

    // -----------------------------------------------------------------------
    // wcsncat
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcsncat_basic() {
        let mut dst: [WcharT; 10] = [0; 10];
        dst[0] = b'A' as i32;
        dst[1] = b'B' as i32;
        dst[2] = 0;
        let src: &[WcharT] = &[b'C' as i32, b'D' as i32, b'E' as i32, 0];
        let ret = unsafe { wcsncat(dst.as_mut_ptr(), src.as_ptr(), 2) };
        assert_eq!(ret, dst.as_mut_ptr());
        // Should be "ABCD\0"
        assert_eq!(dst[0], b'A' as i32);
        assert_eq!(dst[1], b'B' as i32);
        assert_eq!(dst[2], b'C' as i32);
        assert_eq!(dst[3], b'D' as i32);
        assert_eq!(dst[4], 0);
    }

    #[test]
    fn test_wcsncat_zero_n() {
        let mut dst: [WcharT; 4] = [b'x' as i32, 0, 0, 0];
        let src: &[WcharT] = &[b'y' as i32, 0];
        unsafe { wcsncat(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst[0], b'x' as i32);
        assert_eq!(dst[1], 0); // Unchanged.
    }
}
