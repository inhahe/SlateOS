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
//! - `ungetwc` — push back a wide character, whatever its UTF-8 length
//! - `wcsftime` — format date/time as wide string (`crate::strftime`'s, re-exported)

/// Wide character type (32-bit Unicode code point).
pub type WcharT = i32;

/// Wide-character EOF indicator.
///
/// Analogous to `EOF` for byte streams.  POSIX requires WEOF to be a
/// value of type `wint_t` that is distinct from any valid wide character.
/// On glibc/musl with 32-bit `wchar_t`, WEOF is 0xFFFFFFFF (i.e., -1
/// when interpreted as `i32`).
pub const WEOF: WcharT = -1;

/// Multibyte conversion state, `mbstate_t`: for the restartable conversions
/// here -- `mbrtowc`, `wcrtomb` and their kin -- and `<uchar.h>`'s
/// ([`crate::uchar`]). Eight bytes, musl's size and glibc's:
///
/// | bytes | what |
/// |---|---|
/// | 0..4 | the bytes of a character begun and not finished -- `mbrtowc`'s input so far, `c8rtomb`'s code units so far -- or, with byte 6 set, what is still to be handed out |
/// | 4 | how many bytes of the character have come |
/// | 5 | how many it needs; 0, none begun |
/// | 6 | what is still to be handed out: 0, nothing; [`MbstateT::LOW_SURROGATE`], [`MbstateT::UTF8_UNITS`] or [`MbstateT::HIGH_SURROGATE`] |
/// | 7 | for `UTF8_UNITS`, how many are left |
///
/// All zeros is the initial state, as C requires of a zeroed `mbstate_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MbstateT {
    opaque: [u8; 8],
}

impl MbstateT {
    /// `mbrtoc16` has the second unit of a surrogate pair to hand out, in
    /// bytes 0 and 1.
    pub(crate) const LOW_SURROGATE: u8 = 1;
    /// `mbrtoc8` has a character's code units to hand out, in bytes 0 to 2.
    pub(crate) const UTF8_UNITS: u8 = 2;
    /// `c16rtomb` has the first unit of a surrogate pair, in bytes 0 and 1,
    /// and waits for the second.
    pub(crate) const HIGH_SURROGATE: u8 = 3;

    /// The initial conversion state.
    #[must_use]
    pub const fn new() -> Self {
        Self { opaque: [0; 8] }
    }

    /// Whether this is the initial state: no character begun, nothing to
    /// hand out.
    pub(crate) fn is_initial(self) -> bool {
        let [_, _, _, _, count, expected, pending, left] = self.opaque;
        count == 0 && expected == 0 && pending == 0 && left == 0
    }

    /// How many bytes of the character begun have come.
    pub(crate) fn count(self) -> usize {
        usize::from(self.opaque[4])
    }

    /// How many bytes the character begun needs; 0, none begun.
    pub(crate) fn expected(self) -> usize {
        usize::from(self.opaque[5])
    }

    /// The first byte of the character begun.
    pub(crate) fn lead(self) -> u8 {
        self.opaque[0]
    }

    /// Begin a character of `len` bytes (2 to 4) with `lead`.
    pub(crate) fn begin(&mut self, len: usize, lead: u8) {
        self.reset();
        // At most 4, from `utf8_seq_len`.
        self.opaque[5] = len.min(4) as u8;
        self.push(lead);
    }

    /// The character begun's next byte. A fifth is never kept: none needs it.
    pub(crate) fn push(&mut self, b: u8) {
        let at = self.count();
        if let Some(slot) = self.opaque.get_mut(at).filter(|_| at < 4) {
            *slot = b;
            self.opaque[4] = self.opaque[4].wrapping_add(1);
        }
    }

    /// The bytes of the character begun, and how many there are.
    pub(crate) fn bytes(self) -> ([u8; 4], usize) {
        let [a, b, c, d, ..] = self.opaque;
        ([a, b, c, d], self.count().min(4))
    }

    /// What is to be handed out: 0, or one of the three kinds above.
    pub(crate) fn pending(self) -> u8 {
        self.opaque[6]
    }

    /// Keep the surrogate `unit`, of kind [`Self::LOW_SURROGATE`] or
    /// [`Self::HIGH_SURROGATE`].
    pub(crate) fn keep_surrogate(&mut self, kind: u8, unit: u16) {
        self.reset();
        let [lo, hi] = unit.to_le_bytes();
        self.opaque[0] = lo;
        self.opaque[1] = hi;
        self.opaque[6] = kind;
    }

    /// The surrogate kept.
    pub(crate) fn surrogate(self) -> u16 {
        u16::from_le_bytes([self.opaque[0], self.opaque[1]])
    }

    /// Keep `units`, at most three of a character's code units, to hand out
    /// in order.
    pub(crate) fn keep_units(&mut self, units: &[u8]) {
        self.reset();
        for (slot, &u) in self.opaque.iter_mut().zip(units.iter().take(3)) {
            *slot = u;
        }
        self.opaque[6] = Self::UTF8_UNITS;
        self.opaque[7] = units.len().min(3) as u8;
    }

    /// The next unit kept, taken; after the last the state is initial again.
    pub(crate) fn next_unit(&mut self) -> u8 {
        let [unit, b, c, ..] = self.opaque;
        self.opaque[0] = b;
        self.opaque[1] = c;
        self.opaque[2] = 0;
        self.opaque[7] = self.opaque[7].saturating_sub(1);
        if self.opaque[7] == 0 {
            self.reset();
        }
        unit
    }

    /// Back to the initial state.
    pub(crate) fn reset(&mut self) {
        self.opaque = [0; 8];
    }
}

impl Default for MbstateT {
    /// The initial state.
    fn default() -> Self {
        Self::new()
    }
}

/// Which internal state a restartable function uses for a NULL `ps`: its
/// own, as C requires ("each function uses its own internal mbstate_t
/// object"), in the calling thread's block ([`crate::perthread`]), so that
/// two threads each passing NULL do not share one character half-read. Until
/// 2026-09-29 `mbrtowc` had one process-wide state, which `mbrlen` and the
/// string forms borrowed, and `<uchar.h>`'s functions none.
pub(crate) mod internal {
    pub(crate) const MBRTOWC: usize = 0;
    pub(crate) const MBRLEN: usize = 1;
    pub(crate) const WCRTOMB: usize = 2;
    pub(crate) const MBSRTOWCS: usize = 3;
    pub(crate) const MBSNRTOWCS: usize = 4;
    pub(crate) const WCSRTOMBS: usize = 5;
    pub(crate) const WCSNRTOMBS: usize = 6;
    pub(crate) const MBRTOC8: usize = 7;
    pub(crate) const MBRTOC16: usize = 8;
    pub(crate) const MBRTOC32: usize = 9;
    pub(crate) const C8RTOMB: usize = 10;
    pub(crate) const C16RTOMB: usize = 11;
    pub(crate) const C32RTOMB: usize = 12;
    /// How many there are.
    pub(crate) const COUNT: usize = 13;
}

/// `ps`, or for a NULL `ps` the calling thread's internal state for the
/// function `which` names ([`internal`]).
pub(crate) fn state_for(ps: *mut MbstateT, which: usize) -> *mut MbstateT {
    if !ps.is_null() {
        return ps;
    }
    // SAFETY: the calling thread's block; only the address is formed.
    let states = unsafe { core::ptr::addr_of_mut!((*crate::perthread::current()).mbstate) };
    states
        .cast::<MbstateT>()
        .wrapping_add(which.min(internal::COUNT.wrapping_sub(1)))
}

// ---------------------------------------------------------------------------
// Internal UTF-8 helpers
// ---------------------------------------------------------------------------

/// Determine the byte length of a UTF-8 sequence from its leading byte.
///
/// Returns 1..=4 for valid lead bytes, 0 for continuation or invalid bytes.
#[inline]
fn utf8_seq_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    } else if lead < 0xC2 {
        0
    }
    // Overlong 2-byte or continuation.
    else if lead < 0xE0 {
        2
    } else if lead < 0xF0 {
        3
    } else if lead < 0xF5 {
        4
    }
    // F5..FF are invalid lead bytes.
    else {
        0
    }
}

/// Check if a byte is a UTF-8 continuation byte (10xxxxxx).
#[inline]
fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// Whether `b` may follow `lead` as the second byte of a well-formed
/// sequence: Unicode's table 3-7, which rules out the overlong forms, the
/// surrogates and what is past U+10FFFF at the second byte already. So a
/// sequence no completion could make valid is refused at the byte that makes
/// it so -- C's "(size_t)(-2)" is for an incomplete "but potentially valid"
/// character -- where glibc's `mbrtowc` refuses it at its last byte.
fn second_byte_ok(lead: u8, b: u8) -> bool {
    let (low, high) = match lead {
        0xE0 => (0xA0, 0xBF),
        0xED => (0x80, 0x9F),
        0xF0 => (0x90, 0xBF),
        0xF4 => (0x80, 0x8F),
        _ => (0x80, 0xBF),
    };
    (low..=high).contains(&b)
}

/// What [`decode`] made of the bytes it was given.
pub(crate) enum Decoded {
    /// A whole character, and how many of the bytes given it took.
    Char { cp: u32, took: usize },
    /// The bytes begin a character without finishing it; the state keeps
    /// them all.
    Incomplete,
    /// No character begins so. The state is initial again.
    Invalid,
}

/// Decode the character `state` has begun, or the next one, from up to `n`
/// bytes at `s`: the heart of [`mbrtowc`], its kin and `<uchar.h>`'s
/// `mbrtoc8`, `mbrtoc16` and `mbrtoc32`. Strict UTF-8 -- RFC 3629, Unicode's
/// table 3-7 -- each byte checked as it comes ([`second_byte_ok`]).
///
/// # Safety
///
/// `s` has `n` readable bytes.
pub(crate) unsafe fn decode(state: &mut MbstateT, s: *const u8, n: usize) -> Decoded {
    let mut took: usize = 0;
    if state.expected() == 0 {
        if n == 0 {
            return Decoded::Incomplete;
        }
        // SAFETY: n >= 1.
        let lead = unsafe { s.read() };
        took = 1;
        match utf8_seq_len(lead) {
            0 => {
                state.reset();
                return Decoded::Invalid;
            }
            1 => {
                state.reset();
                return Decoded::Char {
                    cp: u32::from(lead),
                    took,
                };
            }
            len => state.begin(len, lead),
        }
    }
    while state.count() < state.expected() {
        if took >= n {
            return Decoded::Incomplete;
        }
        // SAFETY: took < n.
        let b = unsafe { s.add(took).read() };
        let fits = if state.count() == 1 {
            second_byte_ok(state.lead(), b)
        } else {
            is_cont(b)
        };
        if !fits {
            state.reset();
            return Decoded::Invalid;
        }
        state.push(b);
        took = took.wrapping_add(1);
    }
    let (bytes, len) = state.bytes();
    state.reset();
    // The checks above leave nothing for this to refuse; it has the last word
    // all the same.
    match utf8_decode(&bytes, len) {
        Some(cp) => Decoded::Char { cp, took },
        None => Decoded::Invalid,
    }
}

/// `(size_t)(-2)`: incomplete, all given bytes kept.
pub(crate) const INCOMPLETE: usize = usize::MAX.wrapping_sub(1);
/// `(size_t)(-1)`: an encoding error, `errno` `EILSEQ`.
pub(crate) const ILSEQ: usize = usize::MAX;

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
/// is invalid (> U+10FFFF or a surrogate). `<uchar.h>`'s encoder too.
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn utf8_encode(cp: u32, buf: &mut [u8; 4]) -> usize {
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
            unsafe {
                *pwc = 0;
            }
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
                unsafe {
                    *pwc = cp as WcharT;
                }
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
        unsafe {
            *s.add(i) = buf[i];
        }
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

    // `n` bounds the *writing*, not the counting. C says that when `dst` is
    // null the function returns the length the conversion would need and `n`
    // is ignored -- and this loop used to be bounded by `n` regardless, so
    // the idiomatic `mbstowcs(NULL, s, 0)` measurement answered 0 for every
    // input. The doc comment above has always claimed the correct behaviour;
    // only the code disagreed. Found by `printf::vswprintf`, which measures
    // exactly that way and silently formatted nothing.
    while dst.is_null() || dst_count < n {
        let lead = unsafe { *src.add(src_off) };
        if lead == 0 {
            if !dst.is_null() {
                unsafe {
                    *dst.add(dst_count) = 0;
                }
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
                unsafe {
                    *dst.add(dst_count) = cp as WcharT;
                }
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
                unsafe {
                    *dst.add(dst_off) = 0;
                }
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

        // Check if there's room in the output buffer -- but only when there
        // *is* an output buffer. With `dst` null this is a measurement and
        // `n` is ignored, which is what C specifies and what the doc comment
        // above has always said; the check used to run unconditionally, so
        // `wcstombs(NULL, s, 0)` reported 0 bytes needed for every input.
        if !dst.is_null() && dst_off + enc_len > n {
            return dst_off; // Buffer full, stop.
        }

        if !dst.is_null() {
            let mut i = 0;
            while i < enc_len {
                unsafe {
                    *dst.add(dst_off + i) = buf[i];
                }
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

/// Restartable multibyte (UTF-8) → wide character.
///
/// Reads up to `n` bytes from `s`, continuing from the partial state in
/// `*ps` (this function's own per-thread state for a NULL `ps`), and stores
/// the decoded code point in `*pwc` unless it is NULL.
///
/// Returns:
/// - 0 if the decoded character is null (U+0000)
/// - 1..4: number of bytes consumed to complete a character
/// - `(size_t)-2`: incomplete but valid so far (state updated)
/// - `(size_t)-1`: invalid byte sequence (errno = EILSEQ), found at the first
///   byte no completion could make valid ([`decode`])
///
/// A NULL `s` is `mbrtowc(NULL, "", 1, ps)`, as C says: the initial state
/// back, or `EILSEQ` in the middle of a character.
///
/// # Safety
///
/// `s` is NULL or has `n` readable bytes; `pwc` NULL or writable; `ps` NULL or
/// a valid `mbstate_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbrtowc(
    pwc: *mut WcharT,
    s: *const u8,
    n: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::MBRTOWC) };
    let (pwc, s, n) = if s.is_null() {
        (core::ptr::null_mut(), b"\0".as_ptr(), 1)
    } else {
        (pwc, s, n)
    };
    // SAFETY: `s` has `n` readable bytes, by the contract or as the literal.
    match unsafe { decode(state, s, n) } {
        Decoded::Char { cp, took } => {
            if !pwc.is_null() {
                // SAFETY: the caller's writable `wchar_t`. A code point is
                // at most 0x10FFFF, so it fits.
                unsafe { pwc.write(cp as WcharT) };
            }
            if cp == 0 { 0 } else { took }
        }
        Decoded::Incomplete => INCOMPLETE,
        Decoded::Invalid => {
            crate::errno::set_errno(crate::errno::EILSEQ);
            ILSEQ
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
pub unsafe extern "C" fn wcrtomb(s: *mut u8, wc: WcharT, ps: *mut MbstateT) -> usize {
    if s.is_null() {
        // `wcrtomb(buf, L'\0', ps)`: the state initial again -- UTF-8 has no
        // shift states, so there is nothing else to undo -- and the NUL's
        // one byte.
        // SAFETY: the caller's state, or this thread's own for this function.
        unsafe { (*state_for(ps, internal::WCRTOMB)).reset() };
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
        unsafe {
            *s.add(i) = buf[i];
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// Display width
// ---------------------------------------------------------------------------

/// The number of terminal columns `wc` takes: `charwidth`'s answer, the one
/// table SlateOS measures text with -- the terminal draws by it, and every
/// Rust program lays text out by it, so a C program must agree or the same
/// line is aligned two ways on one screen (lane B's
/// `requests/b-d-libc-wcwidth-should-answer-from-the-one-width-table.md`).
///
/// - 0 for `L'\0'`, as every C library returns;
/// - -1 for a control character (C0, DEL, C1), and for a `wchar_t` that is
///   not a Unicode scalar value -- a surrogate, a value past U+10FFFF, a
///   negative one -- as glibc returns;
/// - otherwise 0, 1 or 2, by design-decisions §1042's policy: gnulib's where
///   gnulib and glibc disagree (the soft hyphen takes no column; the prepended
///   concatenation marks take one; Hangul Jamo Extended-B's conjoining letters
///   take none), and Unicode 18.0's data, newer than glibc 2.39's.
///
/// The libc decodes UTF-8 whatever `setlocale` reports, so a width per
/// Unicode is what every character `mbrtowc` can produce needs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wcwidth(wc: WcharT) -> i32 {
    if wc == 0 {
        return 0;
    }
    let Some(c) = u32::try_from(wc).ok().and_then(char::from_u32) else {
        return -1;
    };
    charwidth::char_width(c).map_or(-1, |n| i32::try_from(n).unwrap_or(-1))
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
//
// glibc's `C.UTF-8`: glibc's rules, applied to the system's Unicode data,
// generated into `wctype_tables.rs` by `posix/tools/wctype_gen.py`
// (design-decisions §1167). The library decodes UTF-8 whatever `setlocale`
// reports, so every character `mbrtowc` can produce has its classes and its
// case. Eight classes are stored; the other four follow from them and from
// ASCII, as glibc's rules make them: `digit` and `xdigit` are ASCII's alone,
// as C requires; `alnum` is `alpha` or `digit`; `punct` is `graph` and
// neither.
// ---------------------------------------------------------------------------

use crate::wctype_tables as tables;

/// The stored class bits of `wc`, from the run that holds it. A `wchar_t`
/// that is not a code point -- negative, `WEOF`, past U+10FFFF -- is in no
/// class.
fn class_bits(wc: WcharT) -> u32 {
    let Ok(cp) = u32::try_from(wc) else {
        return 0;
    };
    if cp > 0x10_FFFF {
        return 0;
    }
    let runs = &tables::CLASS_RUNS;
    let i = runs.partition_point(|&run| run >> 8 <= cp);
    i.checked_sub(1)
        .and_then(|i| runs.get(i))
        .map_or(0, |&run| run & 0xff)
}

/// Whether `wc` is one of C's ten decimal digits, the only ones `iswdigit`
/// may answer for.
fn is_ascii_digit(wc: WcharT) -> bool {
    (0x30..=0x39).contains(&wc)
}

/// Check if wide character is alphanumeric: alphabetic, or a digit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalnum(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::ALPHA != 0 || is_ascii_digit(wc))
}

/// Check if wide character is alphabetic: Unicode's `Alphabetic`, and the
/// decimal digits of every other script, which C forbids `iswdigit` to own.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalpha(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::ALPHA != 0)
}

/// Check if wide character is a digit: `0` to `9` and nothing else, as C
/// requires.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswdigit(wc: WcharT) -> i32 {
    i32::from(is_ascii_digit(wc))
}

/// Check if wide character is a hex digit: `[0-9A-Fa-f]`, as C requires.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswxdigit(wc: WcharT) -> i32 {
    i32::from(matches!(wc, 0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66))
}

/// Check if wide character is whitespace: C's six, the line and paragraph
/// separators, and the spaces that may break a line (not U+00A0 and its
/// no-break kin).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswspace(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::SPACE != 0)
}

/// Check if wide character is a blank: tab, and the spaces that may break a
/// line.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswblank(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::BLANK != 0)
}

/// Check if wide character is printable: assigned, and not a control, a
/// surrogate, or the line or paragraph separator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswprint(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::PRINT != 0)
}

/// Check if wide character is a control character: C0, DEL, C1, and the
/// line and paragraph separators.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswcntrl(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::CNTRL != 0)
}

/// Check if wide character is uppercase: it has a lower case, or Unicode
/// calls it `Uppercase`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswupper(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::UPPER != 0)
}

/// Check if wide character is lowercase: it has an upper case, or Unicode
/// calls it `Lowercase`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswlower(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::LOWER != 0)
}

/// Check if wide character is punctuation: graphic, and neither alphabetic
/// nor a digit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswpunct(wc: WcharT) -> i32 {
    let bits = class_bits(wc);
    i32::from(bits & tables::GRAPH != 0 && bits & tables::ALPHA == 0 && !is_ascii_digit(wc))
}

/// Check if wide character is graphic: printable, and not a space.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswgraph(wc: WcharT) -> i32 {
    i32::from(class_bits(wc) & tables::GRAPH != 0)
}

/// `wc` moved by the run of `runs` that covers it, or `wc` itself.
fn map_case(wc: WcharT, runs: &[(u32, u32, i32, bool)]) -> WcharT {
    let Ok(cp) = u32::try_from(wc) else {
        return wc;
    };
    let i = runs.partition_point(|&(first, ..)| first <= cp);
    let Some(&(first, last, delta, every_other)) = i.checked_sub(1).and_then(|i| runs.get(i))
    else {
        return wc;
    };
    // `first <= cp` by the search, so the difference is the offset in the run.
    if cp > last || (every_other && cp.wrapping_sub(first) % 2 != 0) {
        return wc;
    }
    cp.checked_add_signed(delta)
        .and_then(|to| WcharT::try_from(to).ok())
        .unwrap_or(wc)
}

/// Convert wide character to lowercase: Unicode's simple lowercase mapping.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn towlower(wc: WcharT) -> WcharT {
    map_case(wc, &tables::TO_LOWER)
}

/// Convert wide character to uppercase: Unicode's simple uppercase mapping.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn towupper(wc: WcharT) -> WcharT {
    map_case(wc, &tables::TO_UPPER)
}

// ---------------------------------------------------------------------------
// wctype / iswctype — generic classification dispatch (<wctype.h>)
// ---------------------------------------------------------------------------

/// Opaque handle for a character class (returned by `wctype()`).
///
/// POSIX defines `wctype_t` as a scalar; musl's `<wctype.h>` makes it an
/// `unsigned long`, so it is 64 bits wide here too -- a caller's
/// `wctype_t` holds all of what `wctype` returns, and `wctype("x") == 0`
/// tests all of it. (It was a `u32` until 2026-09-29: the caller read the
/// return register's upper half, which nothing had set.) We encode each
/// class as a small nonzero integer so `0` means "invalid."
pub type WctypeT = usize;

// Class IDs — keep in sync with wctype() and iswctype().
const WC_ALNUM: WctypeT = 1;
const WC_ALPHA: WctypeT = 2;
const WC_BLANK: WctypeT = 3;
const WC_CNTRL: WctypeT = 4;
const WC_DIGIT: WctypeT = 5;
const WC_GRAPH: WctypeT = 6;
const WC_LOWER: WctypeT = 7;
const WC_PRINT: WctypeT = 8;
const WC_PUNCT: WctypeT = 9;
const WC_SPACE: WctypeT = 10;
const WC_UPPER: WctypeT = 11;
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
        if c == 0 {
            break;
        }
        buf[i] = c;
        i = i.wrapping_add(1);
    }
    let len = i;

    match &buf[..len] {
        b"alnum" => WC_ALNUM,
        b"alpha" => WC_ALPHA,
        b"blank" => WC_BLANK,
        b"cntrl" => WC_CNTRL,
        b"digit" => WC_DIGIT,
        b"graph" => WC_GRAPH,
        b"lower" => WC_LOWER,
        b"print" => WC_PRINT,
        b"punct" => WC_PUNCT,
        b"space" => WC_SPACE,
        b"upper" => WC_UPPER,
        b"xdigit" => WC_XDIGIT,
        _ => 0,
    }
}

/// Test a wide character against a class obtained from `wctype()`.
///
/// Returns nonzero if `wc` belongs to the class identified by `ct`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswctype(wc: WcharT, ct: WctypeT) -> i32 {
    match ct {
        WC_ALNUM => iswalnum(wc),
        WC_ALPHA => iswalpha(wc),
        WC_BLANK => iswblank(wc),
        WC_CNTRL => iswcntrl(wc),
        WC_DIGIT => iswdigit(wc),
        WC_GRAPH => iswgraph(wc),
        WC_LOWER => iswlower(wc),
        WC_PRINT => iswprint(wc),
        WC_PUNCT => iswpunct(wc),
        WC_SPACE => iswspace(wc),
        WC_UPPER => iswupper(wc),
        WC_XDIGIT => iswxdigit(wc),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// wctrans / towctrans — generic transformation dispatch (<wctype.h>)
// ---------------------------------------------------------------------------

/// Opaque handle for a character transformation (returned by `wctrans()`):
/// pointer-wide, as musl's `<wctype.h>` makes `wctrans_t` a `const int *`
/// (a `u32` until 2026-09-29, `wctype_t`'s fault too).
pub type WctransT = usize;

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
        if c == 0 {
            break;
        }
        buf[i] = c;
        i = i.wrapping_add(1);
    }
    let len = i;

    match &buf[..len] {
        b"tolower" => WT_TOLOWER,
        b"toupper" => WT_TOUPPER,
        _ => 0,
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
        unsafe {
            *dst.add(i) = c;
        }
        if c == 0 {
            return dst;
        }
        i = i.wrapping_add(1);
    }
}

/// Copy at most `n` wide characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsncpy(dst: *mut WcharT, src: *const WcharT, n: usize) -> *mut WcharT {
    let mut i: usize = 0;
    let mut done = false;
    while i < n {
        if done {
            unsafe {
                *dst.add(i) = 0;
            }
        } else {
            let c = unsafe { *src.add(i) };
            unsafe {
                *dst.add(i) = c;
            }
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

/// The length of the wide string at `s`, counting at most `maxlen`
/// characters (POSIX.1-2008): `wcslen` for a string that may have no
/// terminator within `maxlen`.
///
/// # Safety
///
/// `s` must be readable up to its first NUL or `maxlen` wide characters,
/// whichever comes first.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsnlen(s: *const WcharT, maxlen: usize) -> usize {
    // SAFETY: this function's contract is the helper's.
    unsafe { wcsnlen_bounded(s, maxlen) }
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
pub unsafe extern "C" fn wcsncmp(s1: *const WcharT, s2: *const WcharT, n: usize) -> i32 {
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

/// `wcschrnul(s, wc)` -- [`wcschr`], but the terminating NUL rather than NULL
/// where `wc` is not in `s` (a GNU extension).
///
/// # Safety
///
/// `s` must be a valid NUL-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcschrnul(s: *const WcharT, wc: WcharT) -> *const WcharT {
    let mut i: usize = 0;
    loop {
        // SAFETY: within the caller's string, up to and including its NUL.
        let c = unsafe { *s.add(i) };
        if c == wc || c == 0 {
            // SAFETY: as above.
            return unsafe { s.add(i) };
        }
        i = i.wrapping_add(1);
    }
}

/// `wcslcpy(dst, src, size)` -- [`crate::string::strlcpy`] for wide strings:
/// at most `size - 1` characters copied, and a NUL after them if `size` is
/// not 0. Returns `wcslen(src)`; a result not below `size` means the copy was
/// cut short (glibc 2.38's, and the BSDs').
///
/// # Safety
///
/// `dst` must be valid for `size` wide characters, and `src` a valid
/// NUL-terminated wide string not overlapping it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcslcpy(dst: *mut WcharT, src: *const WcharT, size: usize) -> usize {
    // SAFETY: the caller's NUL-terminated string.
    let len = unsafe { wcslen(src) };
    if size > 0 {
        let n = len.min(size.wrapping_sub(1));
        // SAFETY: `n < size`, within `dst`; `src` has `len >= n` characters.
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, n);
            *dst.add(n) = 0;
        }
    }
    len
}

/// `wcslcat(dst, src, size)` -- [`crate::string::strlcat`] for wide strings:
/// `src` appended to the wide string in `dst`'s `size` characters, cut short
/// to fit with its NUL. Returns the length the whole would have had; where
/// `dst` holds no NUL within `size`, `size + wcslen(src)`, and nothing is
/// written (glibc 2.38's, and the BSDs').
///
/// # Safety
///
/// `dst` must be valid for `size` wide characters, and `src` a valid
/// NUL-terminated wide string not overlapping it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcslcat(dst: *mut WcharT, src: *const WcharT, size: usize) -> usize {
    // SAFETY: at most `size` characters of `dst` are read.
    let dlen = unsafe { wcsnlen(dst, size) };
    // SAFETY: the caller's NUL-terminated string.
    let slen = unsafe { wcslen(src) };
    if dlen == size {
        return size.wrapping_add(slen);
    }
    // SAFETY: `dlen < size`: the room left is `size - dlen`, one of it the NUL.
    unsafe { wcslcpy(dst.add(dlen), src, size.wrapping_sub(dlen)) };
    dlen.wrapping_add(slen)
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
pub unsafe extern "C" fn wmemcpy(dst: *mut WcharT, src: *const WcharT, n: usize) -> *mut WcharT {
    let mut i: usize = 0;
    while i < n {
        unsafe {
            *dst.add(i) = *src.add(i);
        }
        i = i.wrapping_add(1);
    }
    dst
}

/// Set wide characters to a value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wmemset(dst: *mut WcharT, wc: WcharT, n: usize) -> *mut WcharT {
    let mut i: usize = 0;
    while i < n {
        unsafe {
            *dst.add(i) = wc;
        }
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
pub unsafe extern "C" fn wcstol(nptr: *const WcharT, endptr: *mut *const WcharT, base: i32) -> i64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    let mut i = unsafe { wc_skip_ws(nptr, 0) };

    let negative = unsafe { *nptr.add(i) } == 0x2d;
    if negative || unsafe { *nptr.add(i) } == 0x2b {
        i = i.wrapping_add(1);
    }

    let (actual_base, new_i, before_prefix) = unsafe { wc_detect_base(nptr, i, base) };
    i = new_i;

    // Accumulate in negative space to correctly handle LONG_MIN
    // (whose absolute value exceeds LONG_MAX by one).
    let base64 = i64::from(actual_base);
    let cutoff = i64::MIN / base64; // Most-negative safe value before multiply.
    let cutlim = -(i64::MIN % base64); // Maximum digit before overflow after multiply.
    let mut acc: i64 = 0;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, actual_base);
        if d < 0 {
            break;
        }
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
        unsafe {
            *endptr = if any_digits { nptr.add(i) } else { nptr };
        }
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
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    let mut i = unsafe { wc_skip_ws(nptr, 0) };

    // POSIX: strtoul/wcstoul accept an optional sign.  A '-' means
    // the result is the unsigned wrapping negation of the parsed value.
    let negative = unsafe { *nptr.add(i) } == 0x2d; // '-'
    if negative || unsafe { *nptr.add(i) } == 0x2b {
        // '+'
        i = i.wrapping_add(1);
    }

    let (actual_base, new_i, before_prefix) = unsafe { wc_detect_base(nptr, i, base) };
    i = new_i;
    let mut result: u64 = 0;
    let base_u64 = actual_base as u64;
    let cutoff = u64::MAX / base_u64;
    let cutlim = u64::MAX % base_u64;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, actual_base);
        if d < 0 {
            break;
        }
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
        unsafe {
            *endptr = if any_digits { nptr.add(i) } else { nptr };
        }
    }

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return u64::MAX;
    }

    if negative {
        result.wrapping_neg()
    } else {
        result
    }
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

// The conversions in an explicit locale (GNU), which is always C's here --
// see `locale.rs` -- and the 4.4BSD names for the `long long` ones.

/// `wcstol_l` -- [`wcstol`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstol_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> i64 {
    // SAFETY: forwarded.
    unsafe { wcstol(nptr, endptr, base) }
}

/// `wcstoul_l` -- [`wcstoul`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoul_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> u64 {
    // SAFETY: forwarded.
    unsafe { wcstoul(nptr, endptr, base) }
}

/// `wcstoll_l` -- [`wcstoll`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoll_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> i64 {
    // SAFETY: forwarded.
    unsafe { wcstoll(nptr, endptr, base) }
}

/// `wcstoull_l` -- [`wcstoull`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoull_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> u64 {
    // SAFETY: forwarded.
    unsafe { wcstoull(nptr, endptr, base) }
}

/// `wcstoq` -- 4.4BSD's name for [`wcstoll`].
///
/// # Safety
///
/// As [`wcstol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstoq(nptr: *const WcharT, endptr: *mut *const WcharT, base: i32) -> i64 {
    // SAFETY: forwarded.
    unsafe { wcstoll(nptr, endptr, base) }
}

/// `wcstouq` -- 4.4BSD's name for [`wcstoull`].
///
/// # Safety
///
/// As [`wcstoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstouq(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    base: i32,
) -> u64 {
    // SAFETY: forwarded.
    unsafe { wcstoull(nptr, endptr, base) }
}

/// A wide string as a source of bytes for the shared float scanner.
///
/// Every character a float literal can contain is ASCII, so a wide character
/// outside that range can only end the subject sequence — reporting it as 0
/// makes the scanner stop there, which is what it does for a terminator too.
/// Because each accepted byte is exactly one wide character, an index into
/// this source doubles as an index into the wide string.
struct WideSource(*const WcharT);

impl crate::decfloat::ByteSource for WideSource {
    fn byte_at(&self, i: usize) -> u8 {
        // SAFETY: the constructor's contract is that `self.0` is a valid
        // null-terminated wide string, and the scanner stops at the first 0 it
        // sees, so `i` never runs past the terminator.
        let wc = unsafe { *self.0.add(i) };
        if (1..0x80).contains(&wc) {
            // Truncation is exact: the range check keeps this under 0x80.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                wc as u8
            }
        } else {
            0
        }
    }
}

/// `wcstod` — convert a wide string to `f64`.
///
/// Parses the same subject sequence as [`crate::stdlib::strtod`] — including
/// `inf`, `infinity` and `nan(chars)` — by driving the same scanner over the
/// wide characters, so the two cannot drift apart. The digits are collected
/// exactly and rounded once, and there is no intermediate buffer to overflow:
/// this used to copy at most 62 characters into a fixed array and silently
/// truncate anything longer, turning `1.<70 digits>e300` into a wildly wrong
/// value (BUG-POSIX-WCSTOD-TRUNCATED).
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstod(nptr: *const WcharT, endptr: *mut *const WcharT) -> f64 {
    let mut acc = crate::decfloat::DigitCollector::new();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_wide_float(nptr, endptr, &mut acc) };
    let value = match token {
        crate::decfloat::FloatToken::None => return 0.0,
        crate::decfloat::FloatToken::Nan(p) => return crate::decfloat::nan_f64(p, negative),
        crate::decfloat::FloatToken::Infinity => f64::INFINITY,
        crate::decfloat::FloatToken::Number => {
            let (v, out_of_range) = acc.to_f64(negative);
            if out_of_range {
                crate::errno::set_errno(crate::errno::ERANGE);
            }
            v
        }
    };
    if negative { -value } else { value }
}

/// `wcstof` — convert a wide string to `f32`.
///
/// Rounds to `f32` directly rather than narrowing a `wcstod` result: two
/// roundings are not one, and a value a hair above an `f32` midpoint can land
/// exactly on that midpoint in `f64` and then be sent the wrong way by
/// ties-to-even.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstof(nptr: *const WcharT, endptr: *mut *const WcharT) -> f32 {
    let mut acc = crate::decfloat::DigitCollector::new();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_wide_float(nptr, endptr, &mut acc) };
    let value = match token {
        crate::decfloat::FloatToken::None => return 0.0,
        crate::decfloat::FloatToken::Nan(p) => return crate::decfloat::nan_f32(p, negative),
        crate::decfloat::FloatToken::Infinity => f32::INFINITY,
        crate::decfloat::FloatToken::Number => {
            let (v, out_of_range) = acc.to_f32(negative);
            if out_of_range {
                crate::errno::set_errno(crate::errno::ERANGE);
            }
            v
        }
    };
    if negative { -value } else { value }
}

/// `wcstod_l` -- [`wcstod`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstod`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstod_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    _loc: crate::locale::LocaleT,
) -> f64 {
    // SAFETY: forwarded.
    unsafe { wcstod(nptr, endptr) }
}

/// `wcstof_l` -- [`wcstof`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstof`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcstof_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    _loc: crate::locale::LocaleT,
) -> f32 {
    // SAFETY: forwarded.
    unsafe { wcstof(nptr, endptr) }
}

/// `wcstold` — convert a wide string to `long double`.
///
/// The wide sibling of [`crate::stdlib::strtold`], over the same scanner and
/// the same conversion, so the two cannot disagree: all 64 bits of the
/// significand, rounded in the current direction, and -- when the digits
/// need more memory than there is -- nothing converted, 0 and `ENOMEM`.
///
/// Found missing by linking a C++ program against this libc: libc++'s
/// `<locale>` needs `wcstold`. Until 2026-09-28 it was `wcstod` widened
/// (`TD-POSIX-LONG-DOUBLE-PRECISION`); the C symbol is a thunk
/// ([`crate::ld_c`]) into `__slate_ld_wcstold`.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string, and `endptr`
/// must be null or writable.
pub unsafe fn wcstold(nptr: *const WcharT, endptr: *mut *const WcharT) -> crate::x87::LongDouble {
    let mut acc = crate::decfloat::DigitCollector::for_long_double();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_wide_float(nptr, endptr, &mut acc) };
    let Some((value, out_of_range)) = crate::decfloat::ld80_of(token, negative, &acc) else {
        if !endptr.is_null() {
            // SAFETY: the caller promises `endptr` is writable.
            unsafe { *endptr = nptr };
        }
        crate::errno::set_errno(crate::errno::ENOMEM);
        return crate::x87::LongDouble::POS_ZERO;
    };
    if out_of_range {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
    value
}

/// `wcstold` for C, through the thunk: the result into `out`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_wcstold(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    out: *mut crate::x87::LongDouble,
) {
    // SAFETY: `wcstold`'s contract is the C caller's; `out` is the thunk's
    // result slot.
    unsafe { out.write(wcstold(nptr, endptr)) }
}
crate::ld_c!(l_pp "wcstold" => __slate_ld_wcstold);

/// `wcstold_l` -- [`wcstold`] in a locale, C's.
///
/// # Safety
///
/// As [`wcstold`].
pub unsafe fn wcstold_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    _loc: crate::locale::LocaleT,
) -> crate::x87::LongDouble {
    // SAFETY: forwarded.
    unsafe { wcstold(nptr, endptr) }
}

/// `wcstold_l` for C, through the thunk: the result into `out`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_wcstold_l(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    loc: crate::locale::LocaleT,
    out: *mut crate::x87::LongDouble,
) {
    // SAFETY: as in `__slate_ld_wcstold`.
    unsafe { out.write(wcstold_l(nptr, endptr, loc)) }
}
crate::ld_c!(l_ppp "wcstold_l" => __slate_ld_wcstold_l);

/// Scan a float subject sequence from a wide string and set `*endptr`.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated wide string, and `endptr` either
/// null or writable.
unsafe fn scan_wide_float(
    nptr: *const WcharT,
    endptr: *mut *const WcharT,
    acc: &mut crate::decfloat::DigitCollector,
) -> (crate::decfloat::FloatToken, bool) {
    if nptr.is_null() {
        if !endptr.is_null() {
            // SAFETY: the caller promises `endptr` is writable.
            unsafe {
                *endptr = nptr;
            }
        }
        return (crate::decfloat::FloatToken::None, false);
    }

    let (token, negative, consumed) = crate::decfloat::scan_float_token(&WideSource(nptr), acc);

    if !endptr.is_null() {
        // SAFETY: the caller promises `endptr` is writable.  One accepted byte
        // is one wide character, so `consumed` is a wide-character count, and
        // it never passes the terminator because the scanner stops there.
        unsafe {
            *endptr = nptr.add(consumed);
        }
    }

    (token, negative)
}

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
    // SAFETY: forwarded; for a NULL `ps`, mbrlen's own state, as C requires --
    // it was mbrtowc's until 2026-09-29.
    unsafe { mbrtowc(core::ptr::null_mut(), s, n, state_for(ps, internal::MBRLEN)) }
}

/// Concatenate at most `n` wide characters from `src` to `dst`.
///
/// Appends up to `n` wide characters, always null-terminates.
///
/// # Safety
///
/// `dst` must have room for the existing string plus `n` + 1 wide chars.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsncat(dst: *mut WcharT, src: *const WcharT, n: usize) -> *mut WcharT {
    let dlen = unsafe { wcslen(dst) };
    let mut j: usize = 0;
    while j < n {
        let c = unsafe { *src.add(j) };
        unsafe {
            *dst.add(dlen.wrapping_add(j)) = c;
        }
        if c == 0 {
            return dst;
        }
        j = j.wrapping_add(1);
    }
    // Null-terminate.
    unsafe {
        *dst.add(dlen.wrapping_add(j)) = 0;
    }
    dst
}

/// Search for a wide character in a memory region.
///
/// # Safety
///
/// `s` must be valid for `n * sizeof(wchar_t)` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wmemchr(s: *const WcharT, wc: WcharT, n: usize) -> *const WcharT {
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
pub unsafe extern "C" fn wmemmove(dst: *mut WcharT, src: *const WcharT, n: usize) -> *mut WcharT {
    if (dst as usize) < (src as usize) {
        let mut i: usize = 0;
        while i < n {
            unsafe {
                *dst.add(i) = *src.add(i);
            }
            i = i.wrapping_add(1);
        }
    } else if (dst as usize) > (src as usize) {
        let mut i = n;
        while i > 0 {
            i = i.wrapping_sub(1);
            unsafe {
                *dst.add(i) = *src.add(i);
            }
        }
    }
    dst
}

/// X/Open's old name for [`wcsstr`], which musl's `<wchar.h>` still declares.
///
/// # Safety
///
/// As for [`wcsstr`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcswcs(haystack: *const WcharT, needle: *const WcharT) -> *const WcharT {
    // SAFETY: this function's contract.
    unsafe { wcsstr(haystack, needle) }
}

/// Find a wide substring in a wide string.
///
/// The Two-Way search `strstr` uses (`string::TwoWay`): linear time,
/// constant space, where the loop it replaced took the product of the
/// lengths.  The haystack's length is found only as far as the search goes.
///
/// # Safety
///
/// Both strings must be valid null-terminated wide strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsstr(haystack: *const WcharT, needle: *const WcharT) -> *const WcharT {
    // SAFETY: the caller's strings.  The haystack is read one character
    // past `known` only while `known` is before its terminator.
    unsafe {
        let nlen = wcslen(needle);
        if nlen == 0 {
            return haystack;
        }
        // How many of the haystack's characters precede its terminator, as
        // far as anyone has looked.
        let mut known = 0usize;
        let mut fits = |end: usize| {
            while known < end && haystack.add(known).read() != 0 {
                known = known.wrapping_add(1);
            }
            end <= known
        };
        // A haystack shorter than the needle holds no match, which is
        // cheaper to see than the needle's factorization is to make.
        if !fits(nlen) {
            return core::ptr::null();
        }
        crate::string::TwoWay::new(needle, nlen, |wc: WcharT| wc).find(haystack, fits)
    }
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
            unsafe {
                *saveptr = core::ptr::null_mut();
            }
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
            unsafe {
                *saveptr = core::ptr::null_mut();
            }
            break;
        }
        if wchar_in_set(ch, delim) {
            unsafe {
                *ptr = 0;
            }
            unsafe {
                *saveptr = ptr.add(1);
            }
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
    let size = len
        .wrapping_add(1)
        .wrapping_mul(core::mem::size_of::<WcharT>());

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
        unsafe {
            *out_ptr.add(i) = *s.add(i);
        }
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
        unsafe {
            wcsncpy(dest, src, n);
        }
    }
    len
}

/// Transform a wide string for locale-aware comparison (locale variant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsxfrm_l(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
    _locale: usize,
) -> usize {
    unsafe { wcsxfrm(dest, src, n) }
}

// ---------------------------------------------------------------------------
// Restartable string conversions: mbsrtowcs / wcsrtombs
// ---------------------------------------------------------------------------

/// Convert a multibyte string to a wide string (restartable).
///
/// Converts at most `len` wide characters from the multibyte string
/// pointed to by `*src` -- see [`mbs_to_wcs`] for the rules.
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
    // SAFETY: forwarded, with this function's own state for a NULL `ps`.
    unsafe {
        mbs_to_wcs(
            dst,
            src,
            usize::MAX,
            len,
            state_for(ps, internal::MBSRTOWCS),
        )
    }
}

/// Convert a multibyte string to a wide string (restartable, n-limited):
/// at most `nms` bytes of it, and at most `len` wide characters -- see
/// [`mbs_to_wcs`].
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
    // SAFETY: forwarded, with this function's own state for a NULL `ps`.
    unsafe { mbs_to_wcs(dst, src, nms, len, state_for(ps, internal::MBSNRTOWCS)) }
}

/// What `mbsrtowcs` and `mbsnrtowcs` do, as POSIX and glibc 2.39 do it
/// (`posix/tools/oracle/multibyte_harness.py`):
///
/// - Convert from `*src`, reading at most `nms` bytes, writing at most `len`
///   wide characters to `dst`; the count converted, the terminator not
///   counted.
/// - With a NULL `dst` only count: `len` limits nothing, and `*src` and the
///   state are left as they were. (Until 2026-09-29 `len` limited the count,
///   so `mbsrtowcs(NULL, &src, 0, &st)` -- the usual way to ask how long the
///   result will be -- answered 0, and `*src` moved.)
/// - Otherwise `*src` is left past the last character converted, or NULL
///   after the terminator (the state then initial); and when the `nms` bytes
///   end in the middle of a character, past them, the character's bytes kept
///   in the state for the next call. (It stopped before them, and the next
///   call read them a second time, as an encoding error.)
/// - An encoding error is -1, `errno` `EILSEQ`, with `*src` at the character
///   that is not one.
///
/// # Safety
///
/// `src` NULL or pointing to a NULL or NUL-terminated string's pointer (or
/// one with `nms` readable bytes); `dst` NULL or writable for `len`; `ps` a
/// valid state.
unsafe fn mbs_to_wcs(
    dst: *mut WcharT,
    src: *mut *const u8,
    nms: usize,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's pointers, checked before each use.
    if src.is_null() || unsafe { (*src).is_null() } {
        return 0;
    }
    let counting = dst.is_null();
    // SAFETY: the caller's source pointer and state.
    let (start, mut state) = unsafe { (*src, *ps) };
    let mut at = start;
    let mut used: usize = 0;
    let mut written: usize = 0;
    let result = loop {
        if !counting && written >= len {
            break written;
        }
        let left = nms.saturating_sub(used);
        if left == 0 {
            break written;
        }
        let n = left.min(MB_CUR_MAX);
        // SAFETY: the caller's string: at most `n` bytes read, none past its
        // terminator, which ends every character begun.
        match unsafe { decode(&mut state, at, n) } {
            Decoded::Char { cp, took } => {
                if !counting {
                    // SAFETY: written < len, within the caller's array.
                    unsafe { dst.add(written).write(cp as WcharT) };
                }
                if cp == 0 {
                    if !counting {
                        // SAFETY: the caller's pointers.
                        unsafe {
                            *src = core::ptr::null();
                            (*ps).reset();
                        }
                    }
                    return written;
                }
                written = written.wrapping_add(1);
                at = at.wrapping_add(took);
                used = used.wrapping_add(took);
            }
            Decoded::Incomplete => {
                // The bytes ran out inside a character: all of them are in
                // the state now.
                at = at.wrapping_add(n);
                used = used.wrapping_add(n);
            }
            Decoded::Invalid => {
                crate::errno::set_errno(crate::errno::EILSEQ);
                break ILSEQ;
            }
        }
    };
    if !counting {
        // SAFETY: the caller's pointers.
        unsafe {
            *src = at;
            *ps = state;
        }
    }
    result
}

/// Convert a wide string to a multibyte string (restartable): at most `len`
/// bytes of it -- see [`wcs_to_mbs`].
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
    // SAFETY: forwarded, with this function's own state for a NULL `ps`.
    unsafe {
        wcs_to_mbs(
            dst,
            src,
            usize::MAX,
            len,
            state_for(ps, internal::WCSRTOMBS),
        )
    }
}

/// Convert a wide string to a multibyte string (restartable, n-limited): at
/// most `nwc` wide characters, producing at most `len` bytes -- see
/// [`wcs_to_mbs`].
///
/// # Safety
///
/// Same as `wcsrtombs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsnrtombs(
    dst: *mut u8,
    src: *mut *const WcharT,
    nwc: usize,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: forwarded, with this function's own state for a NULL `ps`.
    unsafe { wcs_to_mbs(dst, src, nwc, len, state_for(ps, internal::WCSNRTOMBS)) }
}

/// What `wcsrtombs` and `wcsnrtombs` do, by [`mbs_to_wcs`]'s rules the other
/// way: at most `nwc` characters, at most `len` bytes -- a character that
/// would not fit whole stops it, the terminator included -- `*src` past the
/// last converted or NULL after the terminator; with a NULL `dst` only
/// counting, `len` ignored and `*src` left alone; an unencodable character
/// (a surrogate, past U+10FFFF, negative) -1 `EILSEQ` with `*src` at it.
///
/// # Safety
///
/// `src` NULL or pointing to a NULL or NUL-terminated wide string's pointer
/// (or one with `nwc` readable characters); `dst` NULL or writable for `len`;
/// `ps` a valid state.
unsafe fn wcs_to_mbs(
    dst: *mut u8,
    src: *mut *const WcharT,
    nwc: usize,
    len: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's pointers, checked before each use.
    if src.is_null() || unsafe { (*src).is_null() } {
        return 0;
    }
    let counting = dst.is_null();
    // SAFETY: the caller's source pointer.
    let mut at = unsafe { *src };
    let mut written: usize = 0;
    let mut done: usize = 0;
    while done < nwc {
        // SAFETY: the caller's wide string, not read past its terminator.
        let wc = unsafe { at.read() };
        let mut buf = [0u8; 4];
        let k = if wc == 0 {
            1
        } else {
            match u32::try_from(wc).map(|cp| utf8_encode(cp, &mut buf)) {
                Ok(k @ 1..=4) => k,
                _ => {
                    crate::errno::set_errno(crate::errno::EILSEQ);
                    if !counting {
                        // SAFETY: the caller's pointer.
                        unsafe { *src = at };
                    }
                    return ILSEQ;
                }
            }
        };
        if !counting {
            if written.saturating_add(k) > len {
                break;
            }
            for (i, &b) in buf.iter().take(k).enumerate() {
                // SAFETY: written + k <= len, within the caller's array.
                unsafe { dst.add(written.wrapping_add(i)).write(b) };
            }
        }
        if wc == 0 {
            if !counting {
                // SAFETY: the caller's pointers.
                unsafe {
                    *src = core::ptr::null();
                    (*ps).reset();
                }
            }
            return written;
        }
        written = written.wrapping_add(k);
        at = at.wrapping_add(1);
        done = done.wrapping_add(1);
    }
    if !counting {
        // SAFETY: the caller's pointer.
        unsafe { *src = at };
    }
    written
}

// ---------------------------------------------------------------------------
// nl_langinfo — delegated to langinfo module
// ---------------------------------------------------------------------------
// The full implementation with all POSIX items (day/month names, era,
// codeset, etc.) lives in langinfo.rs.  No `no_mangle` here — the
// langinfo module owns the exported symbol.

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
pub unsafe extern "C" fn wcsncasecmp(s1: *const WcharT, s2: *const WcharT, n: usize) -> i32 {
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
//
// A stream holds the multibyte form of what the wide calls read and write --
// UTF-8 here, the only encoding this library has -- and these convert at the
// edge, through `stdio::WideStream`: the stream is held for the call and
// claimed wide, as glibc's `_IO_fwide(fp, 1)` claims it, so a wide call on a
// byte stream fails (and a byte call on a wide one; `stdio.rs` has that half).
// A byte sequence that is no character is `EILSEQ` and a stream error, as
// glibc's converter makes it.

/// `wc` as UTF-8 in `buf`: the length, or `None` for no character (a
/// surrogate, or above U+10FFFF -- glibc's UTF-8 converter refuses both).
pub(crate) fn encode_wide(wc: WcharT, buf: &mut [u8; 4]) -> Option<usize> {
    let cp = u32::try_from(wc).ok()?;
    match utf8_encode(cp, buf) {
        0 => None,
        n => Some(n),
    }
}

/// The next character from `ws`: `Ok(Some(wc))`, `Ok(None)` at end of file
/// or on a read error (the stream says which), or `Err(())` for a sequence
/// that is no character -- over-long, a surrogate, out of range, cut short
/// -- after which the stream is in error and `errno` is `EILSEQ`
/// ([`BadSequence`]).  A byte
/// that cannot continue the sequence is left to be read again.
pub(crate) fn read_wide(ws: &crate::stdio::WideStream) -> Result<Option<WcharT>, BadSequence> {
    let first = ws.getc();
    let Ok(b0) = u8::try_from(first) else {
        return Ok(None);
    };
    let (len, init, min) = match b0 {
        0x00..=0x7f => return Ok(Some(WcharT::from(b0))),
        0xc2..=0xdf => (2, u32::from(b0 & 0x1f), 0x80),
        0xe0..=0xef => (3, u32::from(b0 & 0x0f), 0x800),
        0xf0..=0xf4 => (4, u32::from(b0 & 0x07), 0x1_0000),
        _ => return Err(bad_sequence(ws)),
    };
    let mut cp = init;
    for _ in 1..len {
        let next = ws.getc();
        let Ok(b) = u8::try_from(next) else {
            // Cut short by end of file or an error: not a character.
            return Err(bad_sequence(ws));
        };
        if b & 0xc0 != 0x80 {
            let _ = ws.unget(b); // there is room: one byte was just read
            return Err(bad_sequence(ws));
        }
        cp = (cp << 6) | u32::from(b & 0x3f);
    }
    if cp < min || char::from_u32(cp).is_none() {
        return Err(bad_sequence(ws));
    }
    Ok(WcharT::try_from(cp).ok())
}

/// A byte sequence that was no character, already reported: `EILSEQ` and a
/// stream error.
pub(crate) struct BadSequence;

/// Report a byte sequence that is no character: `EILSEQ` and a stream error,
/// as glibc's converter reports it.
fn bad_sequence(ws: &crate::stdio::WideStream) -> BadSequence {
    crate::errno::set_errno(crate::errno::EILSEQ);
    ws.set_error();
    BadSequence
}

/// Write `wc` to `ws`: `false` on a write error or for no character
/// (`EILSEQ`, and a stream error).
fn write_wide(ws: &crate::stdio::WideStream, wc: WcharT) -> bool {
    let mut buf = [0u8; 4];
    let Some(len) = encode_wide(wc, &mut buf) else {
        let _ = bad_sequence(ws); // reported; the caller says WEOF
        return false;
    };
    buf.get(..len).unwrap_or(&[]).iter().all(|&b| ws.putc(b))
}

/// Write a wide character: `wc`, or `WEOF` on an error, for a character
/// UTF-8 cannot encode (`EILSEQ`), or on a byte stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputwc(wc: WcharT, stream: *mut u8) -> WcharT {
    let Some(ws) = crate::stdio::lock_wide_stream(stream) else {
        return WEOF;
    };
    if write_wide(&ws, wc) { wc } else { WEOF }
}

/// Read a wide character: `WEOF` at end of file, on an error, for a byte
/// sequence that is no character (`EILSEQ`), or on a byte stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetwc(stream: *mut u8) -> WcharT {
    let Some(ws) = crate::stdio::lock_wide_stream(stream) else {
        return WEOF;
    };
    match read_wide(&ws) {
        Ok(Some(wc)) => wc,
        _ => WEOF,
    }
}

/// `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putwc(wc: WcharT, stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fputwc(wc, stream) }
}

/// `fgetwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getwc(stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fgetwc(stream) }
}

/// `fputwc(wc, stdout)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putwchar(wc: WcharT) -> WcharT {
    // SAFETY: `stdout` is a stream.
    unsafe { fputwc(wc, crate::stdio::stdout_stream()) }
}

/// `fgetwc(stdin)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getwchar() -> WcharT {
    // SAFETY: `stdin` is a stream.
    unsafe { fgetwc(crate::stdio::stdin_stream()) }
}

/// `fputwc`: the stream's lock is recursive, so taking it again for a caller
/// that holds it costs a count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputwc_unlocked(wc: WcharT, stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fputwc(wc, stream) }
}

/// `fgetwc`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetwc_unlocked(stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fgetwc(stream) }
}

/// `putwc`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putwc_unlocked(wc: WcharT, stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fputwc(wc, stream) }
}

/// `getwc`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getwc_unlocked(stream: *mut u8) -> WcharT {
    // SAFETY: forwarded.
    unsafe { fgetwc(stream) }
}

/// `putwchar`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putwchar_unlocked(wc: WcharT) -> WcharT {
    putwchar(wc)
}

/// `getwchar`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getwchar_unlocked() -> WcharT {
    getwchar()
}

/// Push a wide character back: its UTF-8 goes back into the stream, so the
/// next `fgetwc` reads it again, whatever its length.  `wc`, or `WEOF` for
/// `WEOF`, a character UTF-8 cannot encode (`EILSEQ`), a byte stream, or no
/// room.  Clears end of file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ungetwc(wc: WcharT, stream: *mut u8) -> WcharT {
    if wc == WEOF {
        return WEOF;
    }
    let Some(ws) = crate::stdio::lock_wide_stream(stream) else {
        return WEOF;
    };
    if unget_wide(&ws, wc) { wc } else { WEOF }
}

/// `ungetwc`'s body, for a stream already held: push `wc`'s UTF-8 back.
/// `false` for a character UTF-8 cannot encode (`EILSEQ`) or no room.
pub(crate) fn unget_wide(ws: &crate::stdio::WideStream, wc: WcharT) -> bool {
    let mut buf = [0u8; 4];
    let Some(len) = encode_wide(wc, &mut buf) else {
        crate::errno::set_errno(crate::errno::EILSEQ);
        return false;
    };
    buf.get(..len)
        .unwrap_or(&[])
        .iter()
        .rev()
        .all(|&b| ws.unget(b))
}

/// `fputws`'s body.
///
/// # Safety
///
/// `s` is a wide C string or NULL.
unsafe fn fputws_raw(s: *const WcharT, stream: *mut u8) -> i32 {
    if s.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    let Some(ws) = crate::stdio::lock_wide_stream(stream) else {
        return -1;
    };
    let mut i = 0usize;
    loop {
        // SAFETY: a wide C string; the loop stops at its terminator.
        let wc = unsafe { *s.add(i) };
        if wc == 0 {
            return 1;
        }
        if !write_wide(&ws, wc) {
            return -1;
        }
        i = i.wrapping_add(1);
    }
}

/// Write a wide string: 1 (glibc's answer), or -1 on an error, for a
/// character UTF-8 cannot encode, or on a byte stream.  A NULL `s` is
/// `EFAULT` (§1115).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputws(s: *const WcharT, stream: *mut u8) -> i32 {
    // SAFETY: forwarded.
    unsafe { fputws_raw(s, stream) }
}

/// `fputws`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fputws_unlocked(s: *const WcharT, stream: *mut u8) -> i32 {
    // SAFETY: forwarded.
    unsafe { fputws_raw(s, stream) }
}

/// Read a wide line of at most `n - 1` characters, through its newline:
/// glibc's `fgetws`, whose NULL means nothing was read, or an error new to
/// this call stopped it (not `EAGAIN`); an error already on the stream does
/// not count.  An invalid sequence is such an error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetws(buf: *mut WcharT, n: i32, stream: *mut u8) -> *mut WcharT {
    if n <= 0 {
        return core::ptr::null_mut();
    }
    if buf.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return core::ptr::null_mut();
    }
    if n == 1 {
        // SAFETY: `buf` holds one character.
        unsafe { *buf = 0 };
        return buf;
    }
    let Some(ws) = crate::stdio::lock_wide_stream(stream) else {
        return core::ptr::null_mut();
    };
    let old_error = ws.error();
    ws.clear_error();
    let room = usize::try_from(n).unwrap_or(0).wrapping_sub(1);
    let mut count = 0usize;
    while count < room {
        match read_wide(&ws) {
            Ok(Some(wc)) => {
                // SAFETY: `count < room < n` characters fit in `buf`.
                unsafe { *buf.add(count) = wc };
                count = count.wrapping_add(1);
                if wc == WcharT::from(b'\n') {
                    break;
                }
            }
            _ => break,
        }
    }
    let new_error = ws.error();
    if old_error {
        ws.set_error();
    }
    if count == 0 || (new_error && crate::errno::get_errno() != crate::errno::EAGAIN) {
        return core::ptr::null_mut();
    }
    // SAFETY: `count <= n - 1`, so the terminator fits.
    unsafe { *buf.add(count) = 0 };
    buf
}

/// `fgetws`, as [`fputwc_unlocked`] is `fputwc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetws_unlocked(buf: *mut WcharT, n: i32, stream: *mut u8) -> *mut WcharT {
    // SAFETY: forwarded.
    unsafe { fgetws(buf, n, stream) }
}

/// Own archive member — gnulib replaces `wmempcpy`.
///
/// This module is otherwise one Rust module and therefore one codegen unit and
/// one archive member holding 78 externally visible symbols, `mbrtowc` among
/// them. Any program that calls `mbrtowc` extracts that whole member. gnulib
/// supplies its own `wmempcpy` (musl has none), so without this split the two
/// definitions collide and the program cannot decline half a member.
///
/// Measured, not theorised: `scripts/coreutils-spike/run.sh` failed `ls`,
/// `dir`, `vdir`, `du` and `dircolors` on exactly this symbol and no other.
///
/// Note that `-C codegen-units=4096` does not help here and never could —
/// `codegen-units` is a ceiling, not a splitter, and rustc's partitioner does
/// not divide a single module. Only a nested `mod` creates a new unit. See
/// `string.rs`'s module header and `design-decisions.md` §339/§340.
mod gnu_wmempcpy {
    use super::*;

    /// Copy wide characters, returning pointer past last written.
    ///
    /// Like `wmemcpy` but returns a pointer to the wide character after
    /// the last one written (i.e., `dest + n`).
    ///
    /// # Safety
    ///
    /// `dest` and `src` must be valid for `n` wide characters.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn wmempcpy(
        dest: *mut WcharT,
        src: *const WcharT,
        n: usize,
    ) -> *mut WcharT {
        if !dest.is_null() && !src.is_null() {
            let mut i: usize = 0;
            while i < n {
                unsafe {
                    *dest.add(i) = *src.add(i);
                }
                i = i.wrapping_add(1);
            }
        }
        // SAFETY: dest + n is one past the last element written.
        unsafe { dest.add(n) }
    }
}
pub use gnu_wmempcpy::wmempcpy;

/// Own archive member — gnulib replaces `wcpcpy`.  See [`gnu_wmempcpy`] for
/// why a function gnulib may define itself must not share a member.
mod gnu_wcpcpy {
    use super::*;

    /// Copy a wide string, returning a pointer to the terminator written.
    ///
    /// POSIX.1-2008 `wcpcpy`: the wide twin of `stpcpy`.
    ///
    /// # Safety
    ///
    /// `src` must be a NUL-terminated wide string and `dst` writable for
    /// `wcslen(src) + 1` wide characters; the two must not overlap.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn wcpcpy(dst: *mut WcharT, src: *const WcharT) -> *mut WcharT {
        let mut i: usize = 0;
        loop {
            // SAFETY: caller contract -- `src` is readable to its NUL and
            // `dst` writable for as many wide characters.
            let c = unsafe { *src.add(i) };
            // SAFETY: as above.
            unsafe { *dst.add(i) = c };
            if c == 0 {
                // SAFETY: `i` is within the string just written.
                return unsafe { dst.add(i) };
            }
            i = i.wrapping_add(1);
        }
    }
}
pub use gnu_wcpcpy::wcpcpy;

/// Own archive member — gnulib replaces `wcpncpy`.  See [`gnu_wmempcpy`].
mod gnu_wcpncpy {
    use super::*;

    /// Copy at most `n` wide characters, padding with NULs to `n`, and return
    /// a pointer to the first NUL written — or `dst + n` if none was.
    ///
    /// POSIX.1-2008 `wcpncpy`: the wide twin of `stpncpy`.
    ///
    /// # Safety
    ///
    /// `dst` must be writable for `n` wide characters and `src` readable for
    /// `n` of them or up to its NUL, whichever comes first.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn wcpncpy(
        dst: *mut WcharT,
        src: *const WcharT,
        n: usize,
    ) -> *mut WcharT {
        let mut end = n;
        let mut i: usize = 0;
        while i < n {
            let c = if end == n {
                // SAFETY: caller contract -- `src` is readable up to its NUL
                // or `n` characters, and no NUL has been read yet.
                unsafe { *src.add(i) }
            } else {
                0
            };
            if c == 0 && end == n {
                end = i;
            }
            // SAFETY: `i < n` and `dst` is writable for `n` characters.
            unsafe { *dst.add(i) = c };
            i = i.wrapping_add(1);
        }
        // SAFETY: `end <= n`, so this is within, or one past, `dst`.
        unsafe { dst.add(end) }
    }
}
pub use gnu_wcpncpy::wcpncpy;

// ---------------------------------------------------------------------------
// `_FORTIFY_SOURCE`: the wide-character and multibyte entry points
//
// glibc's wchar2.h passes these the destination's size *in wide characters*
// (`__glibc_objsize (s) / sizeof (wchar_t)`) for a `wchar_t` destination, and
// in bytes for a `char` one -- so each size below is in its destination's own
// unit, and `(size_t)-1`, "unknown", still fits everything.
//
// Every one of them aborts, as glibc's do (glibc 2.39 debug/*_chk.c), except
// `__fgetws_chk`, which clamps as `__fgets_chk` does -- design-decisions.md
// §1105 -- the rule, and the rows of its table for these, which say why a
// conversion is treated as a copy.
// ---------------------------------------------------------------------------

/// The length of the wide string at `s`, reading at most `max` characters.
///
/// # Safety
///
/// `s` must be readable up to its first NUL or `max` wide characters,
/// whichever comes first.
unsafe fn wcsnlen_bounded(s: *const WcharT, max: usize) -> usize {
    let mut i: usize = 0;
    // SAFETY: caller contract; `i < max` at every read.
    while i < max && unsafe { *s.add(i) } != 0 {
        i = i.wrapping_add(1);
    }
    i
}

/// [`crate::fortify::concatenation_fits`] for wide strings: the string
/// already in `dest` (found without reading past `objsize` characters of it),
/// plus `append` characters and a terminator, within `objsize`.  `false` when
/// `dest` has no terminator inside the object, where glibc's loop aborts too.
///
/// # Safety
///
/// `dest` must be readable up to its first NUL or `objsize` wide characters.
unsafe fn wide_concatenation_fits(dest: *const WcharT, append: usize, objsize: usize) -> bool {
    // SAFETY: this function's contract.
    let have = unsafe { wcsnlen_bounded(dest, objsize) };
    if have >= objsize {
        return false;
    }
    have.checked_add(append)
        .is_some_and(|total| crate::fortify::fits_with_terminator(total, objsize))
}

/// `__wmemcpy_chk` — fortified `wmemcpy`; aborts when `n > ns1`
/// (debug/wmemcpy_chk.c).
///
/// # Safety
///
/// As `wmemcpy`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wmemcpy_chk(
    s1: *mut WcharT,
    s2: *const WcharT,
    n: usize,
    ns1: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, ns1) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wmemcpy(s1, s2, n) }
}

/// `__wmemmove_chk` — fortified `wmemmove`; aborts when `n > ns1`.
///
/// # Safety
///
/// As `wmemmove`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wmemmove_chk(
    s1: *mut WcharT,
    s2: *const WcharT,
    n: usize,
    ns1: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, ns1) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the move fits the object.
    unsafe { wmemmove(s1, s2, n) }
}

/// `__wmempcpy_chk` — fortified `wmempcpy`; aborts when `n > ns1`.
///
/// # Safety
///
/// As `wmempcpy`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wmempcpy_chk(
    s1: *mut WcharT,
    s2: *const WcharT,
    n: usize,
    ns1: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, ns1) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wmempcpy(s1, s2, n) }
}

/// `__wmemset_chk` — fortified `wmemset`; aborts when `n > dstlen`.
///
/// # Safety
///
/// As `wmemset`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wmemset_chk(
    s: *mut WcharT,
    c: WcharT,
    n: usize,
    dstlen: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the fill fits the object.
    unsafe { wmemset(s, c, n) }
}

/// `__wcscpy_chk` — fortified `wcscpy`; aborts when `src` and its
/// terminator do not fit `n` wide characters.
///
/// # Safety
///
/// As `wcscpy`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcscpy_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
) -> *mut WcharT {
    // SAFETY: `src` is a NUL-terminated wide string (caller contract).
    if !crate::fortify::fits_with_terminator(unsafe { wcslen(src) }, n) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wcscpy(dest, src) }
}

/// `__wcpcpy_chk` — fortified [`wcpcpy`]; aborts as [`__wcscpy_chk`] does.
///
/// # Safety
///
/// As [`wcpcpy`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcpcpy_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    destlen: usize,
) -> *mut WcharT {
    // SAFETY: `src` is a NUL-terminated wide string (caller contract).
    if !crate::fortify::fits_with_terminator(unsafe { wcslen(src) }, destlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wcpcpy(dest, src) }
}

/// `__wcsncpy_chk` — fortified `wcsncpy`; aborts when `n > destlen`, since
/// `wcsncpy` writes exactly `n` wide characters.
///
/// # Safety
///
/// As `wcsncpy`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcsncpy_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
    destlen: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wcsncpy(dest, src, n) }
}

/// `__wcpncpy_chk` — fortified [`wcpncpy`]; aborts when `n > destlen`.
///
/// # Safety
///
/// As [`wcpncpy`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcpncpy_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
    destlen: usize,
) -> *mut WcharT {
    if !crate::fortify::fits(n, destlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the copy fits the object.
    unsafe { wcpncpy(dest, src, n) }
}

/// `__wcscat_chk` — fortified `wcscat`; aborts when the string already in
/// `dest`, `src` and a terminator do not fit `destlen`.
///
/// # Safety
///
/// As `wcscat`; `dest` must be readable up to its NUL or `destlen` wide
/// characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcscat_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    destlen: usize,
) -> *mut WcharT {
    // SAFETY: caller contract for both strings.
    if !unsafe { wide_concatenation_fits(dest, wcslen(src), destlen) } {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the concatenation fits the object.
    unsafe { wcscat(dest, src) }
}

/// `__wcsncat_chk` — fortified `wcsncat`; aborts when the string already in
/// `dest`, at most `n` characters of `src` and a terminator do not fit
/// `destlen`.
///
/// # Safety
///
/// As `wcsncat`; `dest` must be readable up to its NUL or `destlen` wide
/// characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcsncat_chk(
    dest: *mut WcharT,
    src: *const WcharT,
    n: usize,
    destlen: usize,
) -> *mut WcharT {
    // SAFETY: caller contract for both strings; `src` is read at most `n`.
    if !unsafe { wide_concatenation_fits(dest, wcsnlen_bounded(src, n), destlen) } {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and the concatenation fits the object.
    unsafe { wcsncat(dest, src, n) }
}

/// `__wcrtomb_chk` — fortified `wcrtomb`.
///
/// glibc 2.39's is `__wcrtomb_internal (s, wchar, ps, buflen)`
/// (wcsmbs/wcrtomb.c:38): it converts into a buffer of its own and aborts only
/// if the *encoding* is longer than `buflen` (:105), so a four-byte buffer is
/// enough for an ASCII character and a two-byte one is not enough for `€`.
/// An unencodable character is still `EILSEQ`, whatever the buffer.
///
/// # Safety
///
/// As `wcrtomb`; `s`, when non-NULL, must be writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcrtomb_chk(
    s: *mut u8,
    wc: WcharT,
    ps: *mut MbstateT,
    buflen: usize,
) -> usize {
    if s.is_null() {
        // The reset form writes nothing the caller sized.
        // SAFETY: caller contract.
        return unsafe { wcrtomb(s, wc, ps) };
    }
    let mut local = [0u8; MB_CUR_MAX];
    // SAFETY: `local` holds MB_CUR_MAX bytes, all `wcrtomb` ever writes.
    let written = unsafe { wcrtomb(local.as_mut_ptr(), wc, ps) };
    if written == usize::MAX {
        return written; // EILSEQ, set by wcrtomb.
    }
    if !crate::fortify::fits(written, buflen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: `written <= buflen`, and `s` is writable for `buflen` bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(local.as_ptr(), s, written);
    }
    written
}

/// `__wctomb_chk` — fortified `wctomb`.
///
/// Unlike `__wcrtomb_chk`, glibc's keeps the conservative test
/// (debug/wctomb_chk.c): the buffer must hold `MB_CUR_MAX` bytes, whatever the
/// character, or it aborts.  This libc's `MB_CUR_MAX` is 4 (UTF-8).
///
/// # Safety
///
/// As `wctomb`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wctomb_chk(s: *mut u8, wc: WcharT, buflen: usize) -> i32 {
    if !crate::fortify::fits(MB_CUR_MAX, buflen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and `s` holds MB_CUR_MAX bytes.
    unsafe { wctomb(s, wc) }
}

/// `__mbstowcs_chk` — fortified `mbstowcs`; aborts when `len > dstlen`
/// (debug/mbstowcs_chk.c).  `dstlen` is in wide characters.
///
/// # Safety
///
/// As `mbstowcs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __mbstowcs_chk(
    dst: *mut WcharT,
    src: *const u8,
    len: usize,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` are written.
    unsafe { mbstowcs(dst, src, len) }
}

/// `__mbsrtowcs_chk` — fortified `mbsrtowcs`; aborts when `len > dstlen`.
///
/// # Safety
///
/// As `mbsrtowcs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __mbsrtowcs_chk(
    dst: *mut WcharT,
    src: *mut *const u8,
    len: usize,
    ps: *mut MbstateT,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` are written.
    unsafe { mbsrtowcs(dst, src, len, ps) }
}

/// `__mbsnrtowcs_chk` — fortified `mbsnrtowcs`; aborts when `len > dstlen`.
///
/// # Safety
///
/// As `mbsnrtowcs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __mbsnrtowcs_chk(
    dst: *mut WcharT,
    src: *mut *const u8,
    nmc: usize,
    len: usize,
    ps: *mut MbstateT,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` are written.
    unsafe { mbsnrtowcs(dst, src, nmc, len, ps) }
}

/// `__wcstombs_chk` — fortified `wcstombs`; aborts when `len > dstlen`
/// (debug/wcstombs_chk.c).  `dstlen` is in bytes.
///
/// # Safety
///
/// As `wcstombs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcstombs_chk(
    dst: *mut u8,
    src: *const WcharT,
    len: usize,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` bytes are written.
    unsafe { wcstombs(dst, src, len) }
}

/// `__wcsrtombs_chk` — fortified `wcsrtombs`; aborts when `len > dstlen`.
///
/// # Safety
///
/// As `wcsrtombs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcsrtombs_chk(
    dst: *mut u8,
    src: *mut *const WcharT,
    len: usize,
    ps: *mut MbstateT,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` bytes are written.
    unsafe { wcsrtombs(dst, src, len, ps) }
}

/// `__wcsnrtombs_chk` — fortified `wcsnrtombs`; aborts when `len > dstlen`.
///
/// # Safety
///
/// As `wcsnrtombs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __wcsnrtombs_chk(
    dst: *mut u8,
    src: *mut *const WcharT,
    nwc: usize,
    len: usize,
    ps: *mut MbstateT,
    dstlen: usize,
) -> usize {
    if !crate::fortify::fits(len, dstlen) {
        crate::fortify::__chk_fail();
    }
    // SAFETY: caller contract, and at most `len <= dstlen` bytes are written.
    unsafe { wcsnrtombs(dst, src, nwc, len, ps) }
}

/// `__fgetws_chk` — fortified `fgetws`, clamped as `__fgets_chk` is: it reads
/// with at most `size` wide characters of room, so a line longer than the
/// object comes back in pieces instead of overflowing it.  A short read is
/// part of `fgetws`'s contract already.  (glibc aborts instead, once the line
/// has filled the object -- debug/fgetws_chk.c; the clamp is §1105's rule.)
///
/// # Safety
///
/// As `fgetws`; `buf` must be writable for `size` wide characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __fgetws_chk(
    buf: *mut WcharT,
    size: usize,
    n: i32,
    stream: *mut u8,
) -> *mut WcharT {
    if n <= 0 || size == 0 {
        return core::ptr::null_mut();
    }
    // `n > 0`, so the cast is exact; `min` keeps the result within `i32`.
    let bound = (n as usize).min(size);
    // SAFETY: caller contract, and `fgetws` writes at most `bound <= size`
    // wide characters (its line and the terminator).
    unsafe { fgetws(buf, i32::try_from(bound).unwrap_or(n), stream) }
}

// ---------------------------------------------------------------------------
// Wide strftime
// ---------------------------------------------------------------------------

// `wcsftime` and `wcsftime_l` are `crate::strftime`'s: the engine
// `strftime` uses, instantiated for `wchar_t`, as glibc compiles its one
// source twice. Until 2026-10-06 `wcsftime` here narrowed the format by
// masking each unit to seven bits, so any character past ASCII in it came
// out as another one, and an answer too long for the buffer came back cut
// short rather than as 0.
pub use crate::strftime::{wcsftime, wcsftime_l};

// ---------------------------------------------------------------------------
// POSIX 2008 locale-parameterised classification
// ---------------------------------------------------------------------------
//
// `iswalpha_l(wc, loc)` is `iswalpha(wc)` evaluated in an explicit locale
// rather than in the thread's current one. **We have exactly one locale**, so
// the two are the same function and the argument is ignored — which is not a
// shortcut but what musl does, for the same reason: musl supports only the C
// locale, so its `_l` forms are wrappers of one line each.
//
// Ignoring the argument is honest here in a way it would not be elsewhere in
// this tree. `newlocale` already returns a single tag for every request
// (`locale.rs`), so there is no second locale a caller could have obtained and
// no distinction being discarded. If a real locale ever lands, these become
// the twenty-one places that must learn about it, which is why they are
// together and why this comment names them as a set. (Fourteen until
// 2026-09-28, when `wctype_l`, `iswctype_l`, `wctrans_l`, `towctrans_l`,
// `wcscasecmp_l`, `wcsncasecmp_l` and `wcsftime_l` joined them: musl's headers
// declare all seven, and a program calling one did not link. Since
// 2026-10-06 `wcsftime_l` is `crate::strftime`'s, beside `strftime_l`, and
// reads its names from `crate::langinfo` -- the place a locale would change
// them.)
//
// Measured need: upstream CMake 4.4.3 links against our libc with exactly
// twenty undefined symbols and these are fourteen of them — the single largest
// group, and the cheapest. See `scripts/cmake-spike/README.md`.

/// `iswalnum` in an explicit locale. See the note above on why `_loc` is unused.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalnum_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswalnum(wc)
}

/// `iswalpha` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswalpha_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswalpha(wc)
}

/// `iswblank` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswblank_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswblank(wc)
}

/// `iswcntrl` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswcntrl_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswcntrl(wc)
}

/// `iswdigit` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswdigit_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswdigit(wc)
}

/// `iswgraph` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswgraph_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswgraph(wc)
}

/// `iswlower` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswlower_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswlower(wc)
}

/// `iswprint` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswprint_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswprint(wc)
}

/// `iswpunct` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswpunct_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswpunct(wc)
}

/// `iswspace` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswspace_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswspace(wc)
}

/// `iswupper` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswupper_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswupper(wc)
}

/// `iswxdigit` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswxdigit_l(wc: WcharT, _loc: crate::locale::LocaleT) -> i32 {
    iswxdigit(wc)
}

/// `towlower` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn towlower_l(wc: WcharT, _loc: crate::locale::LocaleT) -> WcharT {
    towlower(wc)
}

/// `towupper` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn towupper_l(wc: WcharT, _loc: crate::locale::LocaleT) -> WcharT {
    towupper(wc)
}

/// `wctype` in an explicit locale.
///
/// # Safety
///
/// As for [`wctype`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wctype_l(name: *const u8, _loc: crate::locale::LocaleT) -> WctypeT {
    // SAFETY: this function's contract.
    unsafe { wctype(name) }
}

/// `iswctype` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iswctype_l(wc: WcharT, ct: WctypeT, _loc: crate::locale::LocaleT) -> i32 {
    iswctype(wc, ct)
}

/// `wctrans` in an explicit locale.
///
/// # Safety
///
/// As for [`wctrans`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wctrans_l(name: *const u8, _loc: crate::locale::LocaleT) -> WctransT {
    // SAFETY: this function's contract.
    unsafe { wctrans(name) }
}

/// `towctrans` in an explicit locale.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn towctrans_l(wc: WcharT, tr: WctransT, _loc: crate::locale::LocaleT) -> WcharT {
    towctrans(wc, tr)
}

/// `wcscasecmp` in an explicit locale.
///
/// # Safety
///
/// As for [`wcscasecmp`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcscasecmp_l(
    s1: *const WcharT,
    s2: *const WcharT,
    _loc: crate::locale::LocaleT,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { wcscasecmp(s1, s2) }
}

/// `wcsncasecmp` in an explicit locale.
///
/// # Safety
///
/// As for [`wcsncasecmp`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsncasecmp_l(
    s1: *const WcharT,
    s2: *const WcharT,
    n: usize,
    _loc: crate::locale::LocaleT,
) -> i32 {
    // SAFETY: this function's contract.
    unsafe { wcsncasecmp(s1, s2, n) }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    use super::*;

    // -- ungetwc ------------------------------------------------------------

    /// glibc's libio/bug-wgenops-bz33998.c (CVE-2026-5928): after `getwc`
    /// reads `L'A'` from `"A\0"`, `ungetwc(L'\0')` pushes back the wide
    /// character asked for -- read next, and once -- and leaves the bytes
    /// still to come as they were: the stream's own NUL, then its end.
    /// glibc's matched the character against the byte stream instead.
    #[test]
    fn ungetwc_pushes_back_the_character_not_a_byte_already_read() {
        let mut bytes = *b"A\0";
        // SAFETY: `bytes` outlives the stream, closed below.
        let fp = unsafe {
            crate::stdio_mem::fmemopen(bytes.as_mut_ptr().cast(), bytes.len(), c"r".as_ptr().cast())
        };
        assert!(!fp.is_null());
        // SAFETY: `fp` is a live stream until the `fclose`.
        unsafe {
            assert_eq!(getwc(fp), WcharT::from(b'A'));
            assert_eq!(ungetwc(0, fp), 0);
            assert_eq!(getwc(fp), 0, "the character pushed back");
            assert_eq!(getwc(fp), 0, "the stream's own NUL");
            assert_eq!(getwc(fp), WEOF);
            assert_eq!(ungetwc(WcharT::from(b'z'), fp), WcharT::from(b'z'));
            assert_eq!(
                getwc(fp),
                WcharT::from(b'z'),
                "a pushback after the end reads"
            );
            assert_eq!(getwc(fp), WEOF);
            assert_eq!(crate::stdio::fclose(fp), 0);
        }
    }

    // -- wcpcpy / wcpncpy -------------------------------------------------

    fn w(s: &str) -> std::vec::Vec<WcharT> {
        s.chars()
            .map(|c| c as WcharT)
            .chain(core::iter::once(0))
            .collect()
    }

    #[test]
    fn test_wcpcpy_returns_the_terminator() {
        let src = w("héllo");
        let mut dst = [7 as WcharT; 8];
        let end = unsafe { wcpcpy(dst.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(&dst[..6], &src[..]);
        assert_eq!(end, dst.as_mut_ptr().wrapping_add(5));
        assert_eq!(dst[6], 7, "nothing past the terminator is written");
    }

    #[test]
    fn test_wcpncpy_pads_and_returns_the_first_nul() {
        let src = w("ab");
        let mut dst = [7 as WcharT; 6];
        let end = unsafe { wcpncpy(dst.as_mut_ptr(), src.as_ptr(), 5) };
        assert_eq!(&dst[..5], &['a' as WcharT, 'b' as WcharT, 0, 0, 0]);
        assert_eq!(dst[5], 7, "exactly n are written");
        assert_eq!(end, dst.as_mut_ptr().wrapping_add(2));
    }

    #[test]
    fn test_wcpncpy_without_room_for_a_nul_returns_dst_plus_n() {
        let src = w("abcdef");
        let mut dst = [0 as WcharT; 3];
        let end = unsafe { wcpncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(dst, ['a' as WcharT, 'b' as WcharT, 'c' as WcharT]);
        assert_eq!(end, dst.as_mut_ptr().wrapping_add(3));
    }

    // -- _FORTIFY_SOURCE wide entry points: the in-bounds paths ------------
    //
    // An overflow aborts the process, which a host test cannot survive; the
    // aborting half is exercised at ring 3 by services/ctest-fortify-abort.

    #[test]
    fn test_wide_copy_chk_delegates_at_the_boundary() {
        let src = w("abc"); // 4 with the terminator
        let mut dst = [0 as WcharT; 4];
        unsafe {
            assert_eq!(
                __wmemcpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4),
                dst.as_mut_ptr()
            );
            assert_eq!(&dst[..], &src[..]);
            __wmemset_chk(dst.as_mut_ptr(), 'z' as WcharT, 4, 4);
            assert_eq!(dst, ['z' as WcharT; 4]);
            __wmemmove_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4);
            assert_eq!(&dst[..], &src[..]);
            let end = __wmempcpy_chk(dst.as_mut_ptr(), src.as_ptr(), 2, 4);
            assert_eq!(end, dst.as_mut_ptr().wrapping_add(2));
            // wcscpy: three characters and the terminator fill exactly four.
            __wcscpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4);
            assert_eq!(&dst[..], &src[..]);
            let end = __wcpcpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4);
            assert_eq!(end, dst.as_mut_ptr().wrapping_add(3));
            __wcsncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4);
            assert_eq!(&dst[..], &src[..]);
            let end = __wcpncpy_chk(dst.as_mut_ptr(), src.as_ptr(), 4, 4);
            assert_eq!(end, dst.as_mut_ptr().wrapping_add(3));
        }
    }

    #[test]
    fn test_wide_concatenation_fits() {
        let mut dst = [0 as WcharT; 6];
        dst[0] = 'a' as WcharT; // "a"
        unsafe {
            assert!(
                wide_concatenation_fits(dst.as_ptr(), 4, 6),
                "a + 4 + NUL is 6"
            );
            assert!(!wide_concatenation_fits(dst.as_ptr(), 5, 6));
            assert!(
                !wide_concatenation_fits(dst.as_ptr(), usize::MAX, 6),
                "no overflow"
            );
            let full = ['x' as WcharT; 6]; // no terminator inside the object
            assert!(!wide_concatenation_fits(full.as_ptr(), 0, 6));
            let bcd = w("bcd");
            let ret = __wcscat_chk(dst.as_mut_ptr(), bcd.as_ptr(), 6);
            assert_eq!(ret, dst.as_mut_ptr());
            assert_eq!(
                &dst[..5],
                &[
                    'a' as WcharT,
                    'b' as WcharT,
                    'c' as WcharT,
                    'd' as WcharT,
                    0
                ]
            );
            // wcsncat counts at most n of src: "abcd" + "e" + NUL is 6.
            let efg = w("efg");
            __wcsncat_chk(dst.as_mut_ptr(), efg.as_ptr(), 1, 6);
            assert_eq!(dst[4], 'e' as WcharT);
            assert_eq!(dst[5], 0);
        }
    }

    #[test]
    fn test_wcrtomb_chk_checks_the_encoding_not_mb_cur_max() {
        let mut out = [0u8; 4];
        unsafe {
            // 'A' is one byte: a one-byte buffer is enough.
            assert_eq!(
                __wcrtomb_chk(out.as_mut_ptr(), 'A' as WcharT, core::ptr::null_mut(), 1),
                1
            );
            assert_eq!(out[0], b'A');
            // U+20AC is three bytes, and three bytes of room fit it exactly.
            assert_eq!(
                __wcrtomb_chk(out.as_mut_ptr(), 0x20AC, core::ptr::null_mut(), 3),
                3
            );
            assert_eq!(&out[..3], "€".as_bytes());
            // An unencodable character is EILSEQ, not an abort, however small
            // the buffer.
            crate::errno::set_errno(0);
            assert_eq!(
                __wcrtomb_chk(out.as_mut_ptr(), 0xD800, core::ptr::null_mut(), 1),
                usize::MAX
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EILSEQ);
        }
    }

    #[test]
    fn test_wctomb_chk_with_mb_cur_max_of_room() {
        let mut out = [0u8; MB_CUR_MAX];
        let n = unsafe { __wctomb_chk(out.as_mut_ptr(), 0x20AC, MB_CUR_MAX) };
        assert_eq!(n, 3);
        assert_eq!(&out[..3], "€".as_bytes());
    }

    #[test]
    fn test_conversion_chk_delegates_at_the_boundary() {
        let mb = "héllo\0".as_bytes();
        let mut wide = [0 as WcharT; 5];
        unsafe {
            // Five characters into five slots: fits, and stops without a NUL.
            assert_eq!(__mbstowcs_chk(wide.as_mut_ptr(), mb.as_ptr(), 5, 5), 5);
            assert_eq!(wide[1], 'é' as WcharT);
            let mut src = mb.as_ptr();
            assert_eq!(
                __mbsrtowcs_chk(wide.as_mut_ptr(), &raw mut src, 5, core::ptr::null_mut(), 5),
                5
            );
            // Two bytes of input hold 'h' and half of 'é': one character.
            let mut src = mb.as_ptr();
            assert_eq!(
                __mbsnrtowcs_chk(
                    wide.as_mut_ptr(),
                    &raw mut src,
                    2,
                    5,
                    core::ptr::null_mut(),
                    5
                ),
                1
            );
            let back = w("hé");
            let mut out = [0u8; 3];
            assert_eq!(__wcstombs_chk(out.as_mut_ptr(), back.as_ptr(), 3, 3), 3);
            assert_eq!(&out, "hé".as_bytes());
            let mut ws = back.as_ptr();
            assert_eq!(
                __wcsrtombs_chk(out.as_mut_ptr(), &raw mut ws, 3, core::ptr::null_mut(), 3),
                3
            );
            let mut ws = back.as_ptr();
            assert_eq!(
                __wcsnrtombs_chk(
                    out.as_mut_ptr(),
                    &raw mut ws,
                    1,
                    3,
                    core::ptr::null_mut(),
                    3
                ),
                1
            );
        }
    }

    #[test]
    fn test_fgetws_chk_early_returns() {
        let mut buf = [0 as WcharT; 4];
        unsafe {
            assert!(__fgetws_chk(buf.as_mut_ptr(), 4, 0, core::ptr::null_mut()).is_null());
            assert!(__fgetws_chk(buf.as_mut_ptr(), 4, -1, core::ptr::null_mut()).is_null());
            assert!(__fgetws_chk(buf.as_mut_ptr(), 0, 8, core::ptr::null_mut()).is_null());
        }
    }

    // -- POSIX 2008 locale-parameterised classification --

    /// Code points chosen to hit every branch the classifiers have: a letter,
    /// a capital, a digit, a space, a tab, punctuation, a control character,
    /// a hex letter, a Latin-1 accent, a CJK ideograph and NUL. Written as
    /// numbers rather than character literals so nothing here depends on this
    /// file's own encoding.
    const CLASSIFY_PROBES: [WcharT; 11] = [
        0x61, 0x5A, 0x37, 0x20, 0x09, 0x2E, 0x07, 0x66, 0x00E9, 0x4E2D, 0x00,
    ];

    #[test]
    fn locale_variants_agree_with_their_counterparts() {
        // The `_l` forms exist because POSIX spells them that way, not because
        // they do anything different here. If one ever stops agreeing with its
        // counterpart that is a typo, not a locale.
        let l: crate::locale::LocaleT = 1; // the only handle newlocale returns
        for wc in CLASSIFY_PROBES {
            assert_eq!(iswalnum_l(wc, l), iswalnum(wc), "iswalnum {wc:#x}");
            assert_eq!(iswalpha_l(wc, l), iswalpha(wc), "iswalpha {wc:#x}");
            assert_eq!(iswblank_l(wc, l), iswblank(wc), "iswblank {wc:#x}");
            assert_eq!(iswcntrl_l(wc, l), iswcntrl(wc), "iswcntrl {wc:#x}");
            assert_eq!(iswdigit_l(wc, l), iswdigit(wc), "iswdigit {wc:#x}");
            assert_eq!(iswgraph_l(wc, l), iswgraph(wc), "iswgraph {wc:#x}");
            assert_eq!(iswlower_l(wc, l), iswlower(wc), "iswlower {wc:#x}");
            assert_eq!(iswprint_l(wc, l), iswprint(wc), "iswprint {wc:#x}");
            assert_eq!(iswpunct_l(wc, l), iswpunct(wc), "iswpunct {wc:#x}");
            assert_eq!(iswspace_l(wc, l), iswspace(wc), "iswspace {wc:#x}");
            assert_eq!(iswupper_l(wc, l), iswupper(wc), "iswupper {wc:#x}");
            assert_eq!(iswxdigit_l(wc, l), iswxdigit(wc), "iswxdigit {wc:#x}");
            assert_eq!(towlower_l(wc, l), towlower(wc), "towlower {wc:#x}");
            assert_eq!(towupper_l(wc, l), towupper(wc), "towupper {wc:#x}");
        }
    }

    #[test]
    fn the_locale_handle_is_ignored_rather_than_dereferenced() {
        // A caller may pass LC_GLOBAL_LOCALE, a handle from newlocale, or --
        // wrongly -- anything at all. None of those may change the answer and
        // none may crash: there is no locale object behind the handle to
        // dereference, which is exactly why ignoring it is safe here and
        // would not be in a libc that had one.
        for l in [0usize, 1, usize::MAX] {
            assert_eq!(iswalpha_l(0x71, l), iswalpha(0x71));
            assert_eq!(towupper_l(0x71, l), towupper(0x71));
        }
    }

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
        let test_cps = [
            0x00, 0x41, 0x7F, 0x80, 0xE9, 0x7FF, 0x800, 0x20AC, 0xFFFF, 0x10000, 0x1F600, 0x10FFFF,
        ];
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
        st.begin(3, 0xE2);
        assert_eq!(st.count(), 1);
        assert!(!st.is_initial());
        st.push(0x82);
        st.push(0xAC);
        assert_eq!(st.count(), 3);
        let (bytes, n) = st.bytes();
        assert_eq!(&bytes[..n], &[0xE2, 0x82, 0xAC]);
        st.reset();
        assert!(st.is_initial());
    }

    /// The units kept to hand out, and a surrogate kept, make a state that
    /// is not initial; the last unit taken makes it initial again.
    #[test]
    fn test_mbstate_pending() {
        let mut st = MbstateT::new();
        st.keep_units(&[0x9F, 0x98, 0x80]);
        assert!(!st.is_initial());
        assert_eq!(st.pending(), MbstateT::UTF8_UNITS);
        assert_eq!([st.next_unit(), st.next_unit()], [0x9F, 0x98]);
        assert!(!st.is_initial());
        assert_eq!(st.next_unit(), 0x80);
        assert!(st.is_initial());
        st.keep_surrogate(MbstateT::HIGH_SURROGATE, 0xD83D);
        assert_eq!(
            (st.pending(), st.surrogate()),
            (MbstateT::HIGH_SURROGATE, 0xD83D)
        );
        assert_eq!(mbsinit(&raw const st), 0);
    }

    // -- wctype / iswctype --

    #[test]
    fn test_wctype_known_classes() {
        let names: &[(&[u8], WctypeT)] = &[
            (b"alnum\0", WC_ALNUM),
            (b"alpha\0", WC_ALPHA),
            (b"blank\0", WC_BLANK),
            (b"cntrl\0", WC_CNTRL),
            (b"digit\0", WC_DIGIT),
            (b"graph\0", WC_GRAPH),
            (b"lower\0", WC_LOWER),
            (b"print\0", WC_PRINT),
            (b"punct\0", WC_PUNCT),
            (b"space\0", WC_SPACE),
            (b"upper\0", WC_UPPER),
            (b"xdigit\0", WC_XDIGIT),
        ];
        for &(name, expected) in names {
            let ct = unsafe { wctype(name.as_ptr()) };
            assert_eq!(
                ct,
                expected,
                "wctype({:?}) failed",
                core::str::from_utf8(&name[..name.len() - 1]).unwrap_or("?")
            );
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
            if i >= 63 {
                break;
            }
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
        assert_eq!(wcwidth(0xfe00), 0); // VS1
        assert_eq!(wcwidth(0xfe0f), 0); // VS16 (emoji presentation)
    }

    #[test]
    fn test_wcwidth_latin_above_c1() {
        // Latin characters above C1 range should be width 1.
        assert_eq!(wcwidth(0xa0), 1); // NBSP
        assert_eq!(wcwidth(0xc0), 1); // À
        assert_eq!(wcwidth(0xff), 1); // ÿ
    }

    #[test]
    fn test_wcwidth_emoji() {
        // Emoji in SMP should be width 2.
        assert_eq!(wcwidth(0x1f600), 2); // 😀
        assert_eq!(wcwidth(0x1f680), 2); // 🚀
    }

    // -----------------------------------------------------------------------
    // Classes and case: glibc's C.UTF-8 (wctype_oracle.txt)
    // -----------------------------------------------------------------------

    /// What `posix/tools/oracle/wctype_harness.py` saw glibc 2.39 answer
    /// under `C.UTF-8`, for every code point: the class runs (bits in the
    /// harness's order), and the lower and upper mappings that move.
    static WCTYPE_ORACLE: &str = include_str!("wctype_oracle.txt");

    /// The twelve classifiers, in the oracle's bit order.
    const CLASSIFIERS: [(&str, extern "C" fn(WcharT) -> i32); 12] = [
        ("alnum", iswalnum),
        ("alpha", iswalpha),
        ("blank", iswblank),
        ("cntrl", iswcntrl),
        ("digit", iswdigit),
        ("graph", iswgraph),
        ("lower", iswlower),
        ("print", iswprint),
        ("punct", iswpunct),
        ("space", iswspace),
        ("upper", iswupper),
        ("xdigit", iswxdigit),
    ];

    /// Whether Unicode assigned `cp` after glibc's data was made.
    fn assigned_since_glibc(cp: u32) -> bool {
        crate::wctype_tables::ASSIGNED_SINCE_GLIBC
            .iter()
            .any(|&(lo, hi)| (lo..=hi).contains(&cp))
    }

    /// Every code point's twelve classes and both mappings are glibc's,
    /// but where Unicode has moved on since glibc's data: a character
    /// assigned since -- which glibc must then give no class and no case,
    /// checked here -- or one of the few dozen the generator lists by name
    /// as changed.
    #[test]
    fn every_class_and_case_is_glibcs_but_where_unicode_moved_on() {
        let mut masks = std::vec![0u32; 0x11_0000];
        let mut lower = std::collections::HashMap::new();
        let mut upper = std::collections::HashMap::new();
        let mut lines = WCTYPE_ORACLE.lines();
        assert_eq!(lines.next(), Some("glibc 2.39 C.UTF-8"));
        for line in lines {
            let words: std::vec::Vec<&str> = line.split(' ').collect();
            let hex = |i: usize| u32::from_str_radix(words[i], 16).expect("hex");
            match words[0] {
                "class" => masks[hex(1) as usize..=hex(2) as usize].fill(hex(3)),
                "lower" => {
                    lower.insert(hex(1), hex(2));
                }
                "upper" => {
                    upper.insert(hex(1), hex(2));
                }
                other => panic!("an oracle line of kind {other}"),
            }
        }
        let (mut compared, mut assigned, mut changed) = (0u32, 0u32, 0u32);
        for cp in 0..=0x10_FFFFu32 {
            if assigned_since_glibc(cp) {
                // A character here, nothing in glibc's data.
                assert_eq!(masks[cp as usize], 0, "glibc classifies U+{cp:04X}");
                assert!(
                    !lower.contains_key(&cp) && !upper.contains_key(&cp),
                    "U+{cp:04X}"
                );
                assigned += 1;
                continue;
            }
            if crate::wctype_tables::CHANGED_SINCE_GLIBC.contains(&cp) {
                changed += 1;
                continue;
            }
            let wc = cp as WcharT;
            for (bit, (name, test)) in CLASSIFIERS.iter().enumerate() {
                let want = masks[cp as usize] & (1 << bit) != 0;
                assert_eq!(test(wc) != 0, want, "isw{name}(U+{cp:04X})");
            }
            assert_eq!(
                towlower(wc) as u32,
                *lower.get(&cp).unwrap_or(&cp),
                "towlower(U+{cp:04X})"
            );
            assert_eq!(
                towupper(wc) as u32,
                *upper.get(&cp).unwrap_or(&cp),
                "towupper(U+{cp:04X})"
            );
            compared += 1;
        }
        assert_eq!(compared + assigned + changed, 0x11_0000);
        assert_eq!(
            changed as usize,
            crate::wctype_tables::CHANGED_SINCE_GLIBC.len()
        );
    }

    /// Unicode 18.0 where glibc's data is 15.1: a letter assigned since is a
    /// letter here, and has its case.
    #[test]
    fn a_letter_newer_than_glibcs_data_is_a_letter() {
        // U+1C89 CYRILLIC CAPITAL LETTER TJE, Unicode 16.0, lower U+1C8A.
        assert_ne!(iswalpha(0x1c89), 0);
        assert_ne!(iswupper(0x1c89), 0);
        assert_eq!(towlower(0x1c89), 0x1c8a);
        assert_eq!(towupper(0x1c8a), 0x1c89);
    }

    /// Letters, cases and spaces past ASCII: what a ported C program needs
    /// to compare words case-insensitively and to split them.
    #[test]
    fn classes_and_case_past_ascii() {
        assert_ne!(iswalpha(0xe9), 0); // é
        assert_ne!(iswlower(0xe9), 0);
        assert_eq!(towupper(0xe9), 0xc9);
        assert_eq!(towlower(0xc9), 0xe9);
        assert_eq!(towupper(0xdf), 0xdf); // ß has no single upper case
        assert_ne!(iswlower(0xdf), 0);
        assert_eq!(towlower(0x130), 0x69); // İ: Unicode's simple mapping
        assert_eq!(towupper(0x3c2), 0x3a3); // final sigma
        assert_ne!(iswalpha(0x4e00), 0); // 一
        assert_eq!(iswupper(0x4e00) | iswlower(0x4e00), 0);
        assert_ne!(iswalpha(0x0660), 0); // ARABIC-INDIC DIGIT ZERO: alpha, not digit
        assert_eq!(iswdigit(0x0660), 0);
        assert_ne!(iswalnum(0x0660), 0);
        assert_ne!(iswspace(0x3000), 0); // IDEOGRAPHIC SPACE
        assert_ne!(iswblank(0x3000), 0);
        assert_eq!(iswspace(0xa0), 0); // NO-BREAK SPACE: not a space
        assert_ne!(iswpunct(0xa0), 0);
        assert_ne!(iswspace(0x2028), 0); // LINE SEPARATOR
        assert_ne!(iswcntrl(0x2028), 0);
        assert_eq!(iswprint(0x2028), 0);
        assert_ne!(iswpunct(0x2014), 0); // EM DASH
        assert_ne!(iswprint(0xe000), 0); // private use
    }

    /// Nothing that is not a code point is in a class or has a case:
    /// `WEOF`, a negative value, a surrogate, a value past U+10FFFF.
    #[test]
    fn what_is_not_a_character_has_no_class_and_no_case() {
        for wc in [-1, WcharT::MIN, 0xd800, 0xdfff, 0x11_0000, WcharT::MAX] {
            for (name, test) in CLASSIFIERS {
                assert_eq!(test(wc), 0, "isw{name}({wc:#x})");
            }
            assert_eq!(towlower(wc), wc);
            assert_eq!(towupper(wc), wc);
        }
    }

    /// `iswctype` and `towctrans` answer as the functions they name, and the
    /// `_l` forms as the plain ones.
    #[test]
    fn the_generic_and_locale_forms_agree_with_the_functions() {
        for cp in [
            0x41u32, 0xe9, 0x3c3, 0x4e00, 0x3000, 0x2028, 0x1c89, 0x10ffff,
        ] {
            let wc = cp as WcharT;
            for (name, test) in CLASSIFIERS {
                let mut cname = name.as_bytes().to_vec();
                cname.push(0);
                // SAFETY: `cname` is NUL-terminated.
                let class = unsafe { wctype(cname.as_ptr()) };
                assert_ne!(class, 0, "{name}");
                assert_eq!(iswctype(wc, class) != 0, test(wc) != 0, "{name} U+{cp:04X}");
            }
            // SAFETY: both names are NUL-terminated.
            let (lo, up) = unsafe {
                (
                    wctrans(b"tolower\0".as_ptr()),
                    wctrans(b"toupper\0".as_ptr()),
                )
            };
            assert_eq!(towctrans(wc, lo), towlower(wc));
            assert_eq!(towctrans(wc, up), towupper(wc));
            assert_eq!(iswalpha_l(wc, 0), iswalpha(wc));
            assert_eq!(towlower_l(wc, 0), towlower(wc));
            assert_eq!(towupper_l(wc, 0), towupper(wc));
        }
    }

    /// The case runs' every-other-code-point form: Latin Extended-A
    /// alternates capital and small, and only the capitals move down.
    #[test]
    fn a_strided_case_run_moves_only_its_own_code_points() {
        assert_eq!(towlower(0x100), 0x101); // Ā -> ā
        assert_eq!(towlower(0x101), 0x101); // ā stays
        assert_eq!(towupper(0x101), 0x100);
        assert_eq!(towupper(0x100), 0x100);
        assert_eq!(towlower(0x17d), 0x17e); // Ž -> ž
    }

    /// Every character's width is `charwidth`'s, the table the terminal and
    /// every Rust program measure with -- all 1,112,064 of them, so a range
    /// the libc reads differently cannot hide between examples.
    #[test]
    fn wcwidth_is_charwidths_for_every_character() {
        let mut checked = 0u32;
        for cp in 1..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let want = charwidth::char_width(c).map_or(-1, |n| n as i32);
            assert_eq!(wcwidth(cp as WcharT), want, "U+{cp:04X}");
            checked += 1;
        }
        assert_eq!(checked, 0x10_FFFF - 0x800, "every scalar value but U+0000");
        assert_eq!(wcwidth(0), 0);
    }

    /// The places §1042 leaves glibc 2.39 on purpose: the soft hyphen takes
    /// no column, a prepended concatenation mark takes one, a conjoining
    /// letter of Hangul Jamo Extended-B takes none, and a character newer
    /// than glibc's Unicode is measured by its own data.
    #[test]
    fn wcwidth_departs_from_glibc_where_the_one_table_does() {
        assert_eq!(wcwidth(0x00ad), 0); // soft hyphen: glibc 1
        assert_eq!(wcwidth(0x0600), 1); // ARABIC NUMBER SIGN: a visible sign
        assert_eq!(wcwidth(0xd7b0), 0); // HANGUL JUNGSEONG O-YEO: glibc 1
        assert_eq!(wcwidth(0x1fa8a), 2); // an emoji since Unicode 16
    }

    /// What is not a Unicode scalar value has no width: the surrogates, a
    /// value past U+10FFFF, a negative one -- as glibc answers.
    #[test]
    fn wcwidth_of_what_is_not_a_character_is_minus_one() {
        for wc in [
            0xd800,
            0xdbff,
            0xdc00,
            0xdfff,
            0x11_0000,
            0x7fff_ffff,
            -1,
            WcharT::MIN,
        ] {
            assert_eq!(wcwidth(wc), -1, "{wc:#x}");
        }
    }

    /// `wcswidth` sums the widths of at most `n` characters, stopping at a
    /// NUL, and is -1 when any of them has none.
    #[test]
    fn wcswidth_sums_and_refuses_a_widthless_character() {
        let text: [WcharT; 6] = [0x61, 0x4e00, 0x0301, 0x1f600, 0x00ad, 0];
        // SAFETY: `text` holds six characters, the last a NUL.
        unsafe {
            assert_eq!(wcswidth(text.as_ptr(), 6), 1 + 2 + 0 + 2 + 0);
            assert_eq!(wcswidth(text.as_ptr(), 2), 1 + 2);
            assert_eq!(wcswidth(text.as_ptr(), 0), 0);
        }
        let control: [WcharT; 3] = [0x61, 0x1b, 0];
        // SAFETY: as above.
        unsafe {
            assert_eq!(wcswidth(control.as_ptr(), 3), -1);
            assert_eq!(wcswidth(control.as_ptr(), 1), 1);
        }
        let surrogate: [WcharT; 2] = [0xd800, 0];
        // SAFETY: as above.
        unsafe { assert_eq!(wcswidth(surrogate.as_ptr(), 2), -1) };
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
        assert_eq!(iswcntrl(0xa0), 0); // NBSP
        assert_eq!(iswcntrl(0xff), 0); // ÿ
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
        assert_ne!(iswprint(0xa0), 0); // NBSP
        assert_ne!(iswprint(0xc0), 0); // À
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
            b'1' as i32,
            b'2' as i32,
            b'.' as i32,
            b'5' as i32,
            b'x' as i32,
            b'y' as i32,
            b'z' as i32,
            0,
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

    /// An ASCII string as a null-terminated wide string.
    fn wide(text: &str) -> Vec<WcharT> {
        let mut v: Vec<WcharT> = text.bytes().map(WcharT::from).collect();
        v.push(0);
        v
    }

    #[test]
    fn wcsnlen_stops_at_the_bound_or_the_terminator() {
        let s = wide("hello");
        // SAFETY: a terminated wide string.
        unsafe {
            assert_eq!(wcsnlen(s.as_ptr(), 10), 5);
            assert_eq!(wcsnlen(s.as_ptr(), 3), 3);
            assert_eq!(wcsnlen(s.as_ptr(), 0), 0);
        }
        // No terminator within the bound: nothing past it is read.
        let unterminated: [WcharT; 3] = [0x61, 0x62, 0x63];
        // SAFETY: three readable characters.
        assert_eq!(unsafe { wcsnlen(unterminated.as_ptr(), 3) }, 3);
    }

    #[test]
    fn wcswcs_is_wcsstr() {
        let (h, n, none) = (wide("abcabd"), wide("abd"), wide("xyz"));
        // SAFETY: terminated wide strings; the offset stays inside `h`.
        unsafe {
            assert_eq!(wcswcs(h.as_ptr(), n.as_ptr()), h.as_ptr().add(3));
            assert!(wcswcs(h.as_ptr(), none.as_ptr()).is_null());
        }
    }

    #[test]
    fn the_new_l_forms_are_the_one_locales_functions() {
        let (a, b) = (wide("HeLLo"), wide("hello!"));
        // SAFETY: terminated strings and names.
        unsafe {
            let alpha = wctype_l(b"alpha\0".as_ptr(), 1);
            assert_eq!(alpha, wctype(b"alpha\0".as_ptr()));
            assert_ne!(iswctype_l(0x41, alpha, 1), 0);
            assert_eq!(iswctype_l(0x31, alpha, 1), 0);
            let up = wctrans_l(b"toupper\0".as_ptr(), 1);
            assert_eq!(towctrans_l(0x61, up, 1), 0x41);
            assert!(wcscasecmp_l(a.as_ptr(), b.as_ptr(), 1) < 0);
            assert_eq!(wcsncasecmp_l(a.as_ptr(), b.as_ptr(), 5, 1), 0);
        }
    }

    fn parse_wide(text: &str) -> f64 {
        let s = wide(text);
        unsafe { wcstod(s.as_ptr(), core::ptr::null_mut()) }
    }

    /// The old implementation copied the subject sequence into a `[u8; 64]`
    /// and stopped after 62 characters, so a long literal lost its exponent
    /// entirely and came back off by hundreds of orders of magnitude.
    #[test]
    fn wcstod_reads_a_number_longer_than_any_buffer() {
        let text = "1.234567890123456789012345678901234567890123456789012345678901234567890e300";
        assert!(text.len() > 62, "the test must exceed the old buffer");
        assert_eq!(parse_wide(text), text.parse::<f64>().unwrap());

        // 800 digits — past MAX_PARSE_DIGITS, so the sticky path runs too.
        let mut long = String::from("1.");
        for i in 0..800 {
            long.push(char::from(b'0' + (i % 10) as u8));
        }
        long.push_str("e-5");
        assert_eq!(parse_wide(&long), long.parse::<f64>().unwrap());
    }

    /// `endptr` must land after the whole number, however long it is.
    #[test]
    fn wcstod_places_endptr_after_a_long_number() {
        let digits = "3.14159265358979323846264338327950288419716939937510582097e-40";
        let s = wide(&format!("   {digits}xyz"));
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, digits.parse::<f64>().unwrap());
        // Three spaces plus the digits; the trailing "xyz" is not consumed.
        assert_eq!(end, unsafe { s.as_ptr().add(3 + digits.len()) });
    }

    /// A non-ASCII wide character ends the subject sequence rather than being
    /// truncated into some unrelated byte.
    #[test]
    fn wcstod_stops_at_a_non_ascii_character() {
        let s: &[WcharT] = &[b'1' as i32, b'2' as i32, 0x00e9, b'3' as i32, 0];
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 12.0);
        assert_eq!(end, unsafe { s.as_ptr().add(2) });
    }

    /// The named values go through the shared scanner, payload and all.
    #[test]
    fn wcstod_accepts_inf_and_nan() {
        assert!(parse_wide("INFINITY").is_infinite() && parse_wide("INFINITY") > 0.0);
        assert!(parse_wide("-inf").is_infinite() && parse_wide("-inf") < 0.0);
        assert!(parse_wide("nan(0x7)").is_nan());

        let s = wide("nan(0x7)tail");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstod(s.as_ptr(), &raw mut end) };
        assert!(val.is_nan());
        assert_eq!(end, unsafe { s.as_ptr().add(8) });
    }

    // -- the null-destination measuring form -----------------------------
    //
    // Both functions have always documented "if `dst` is null, just counts",
    // and until 2026-09-09 neither did: `mbstowcs`'s loop was bounded by `n`
    // and `wcstombs`'s capacity check ran unconditionally, so the idiomatic
    // `f(NULL, src, 0)` measurement answered 0 for every input. Nothing in
    // the tree called them that way, so nothing noticed until `vswprintf`
    // did -- and its symptom was formatting an empty string, not an error.

    #[test]
    fn mbstowcs_with_a_null_destination_counts_and_ignores_n() {
        let src = b"hello\0";
        let got = unsafe { mbstowcs(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(got, 5, "n must be ignored when dst is null");
        // A large n must give the same answer.
        let same = unsafe { mbstowcs(core::ptr::null_mut(), src.as_ptr(), 999) };
        assert_eq!(same, 5);
    }

    /// Counting is in *characters*, so a multibyte string counts short.
    #[test]
    fn mbstowcs_counts_characters_not_bytes() {
        // U+00E9, U+20AC, U+1F642 -- 2 + 3 + 4 = 9 bytes, 3 characters.
        // Written as byte values rather than escapes: this literal reached
        // the file through a shell heredoc once and came back as the decoded
        // characters, which the compiler then rejected.
        let src: &[u8] = &[0xc3, 0xa9, 0xe2, 0x82, 0xac, 0xf0, 0x9f, 0x99, 0x82, 0x00];
        let got = unsafe { mbstowcs(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(got, 3);
    }

    #[test]
    fn wcstombs_with_a_null_destination_counts_and_ignores_n() {
        let src = wide("hello");
        let got = unsafe { wcstombs(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(got, 5, "n must be ignored when dst is null");
        let same = unsafe { wcstombs(core::ptr::null_mut(), src.as_ptr(), 999) };
        assert_eq!(same, 5);
    }

    /// Counting is in *bytes*, so a multibyte string counts long -- the
    /// mirror image of `mbstowcs`, and the reason a caller sizing a buffer
    /// must ask the right one.
    #[test]
    fn wcstombs_counts_bytes_not_characters() {
        let src: &[WcharT] = &[0x00e9, 0x20ac, 0x1f642, 0];
        let got = unsafe { wcstombs(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(got, 9, "2 + 3 + 4 bytes");
    }

    /// The bounded, writing form is unchanged: `n` still stops it.
    #[test]
    fn a_real_destination_still_respects_n() {
        let src = wide("hello");
        let mut out = [0u8; 8];
        let written = unsafe { wcstombs(out.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(written, 3, "n still bounds a real write");

        let msrc = b"hello\0";
        let mut wout = [0 as WcharT; 8];
        let got = unsafe { mbstowcs(wout.as_mut_ptr(), msrc.as_ptr(), 2) };
        assert_eq!(got, 2);
    }

    /// `wcstold` is `strtold` over wide characters: the same value to the
    /// bit, the same end, for every kind of subject sequence.
    #[test]
    fn wcstold_agrees_with_strtold() {
        for text in [
            "0",
            "-0",
            "3.14159265358979",
            "  \t+2.5e10tail",
            "1e400",
            "-1e-400",
            "INFINITY",
            "-inf",
            "not a number",
            "0.1",
            "-0x1.23456789abcdef01p-16390",
            "nan(0x1234)x",
            "1.18973149535723176502e+4932",
        ] {
            let s = wide(text);
            let mut narrow = text.as_bytes().to_vec();
            narrow.push(0);
            let (mut e1, mut e2): (*const u8, *const WcharT) =
                (core::ptr::null(), core::ptr::null());
            let a = unsafe { crate::stdlib::strtold(narrow.as_ptr(), &raw mut e1) };
            let b = unsafe { wcstold(s.as_ptr(), &raw mut e2) };
            assert_eq!(
                (a.sign_exp, a.significand),
                (b.sign_exp, b.significand),
                "{text:?}"
            );
            assert_eq!(
                e1 as usize - narrow.as_ptr() as usize,
                (e2 as usize - s.as_ptr() as usize) / core::mem::size_of::<WcharT>(),
                "endptr for {text:?}"
            );
        }
    }

    /// The `nan(...)` form goes through the same scanner, payload and all.
    #[test]
    fn wcstold_accepts_nan_with_a_payload() {
        let s = wide("nan(0x7)tail");
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstold(s.as_ptr(), &raw mut end) };
        assert_eq!(
            (val.sign_exp, val.significand),
            (0x7FFF, 0xC000_0000_0000_0007)
        );
        assert_eq!(end, unsafe { s.as_ptr().add(8) });
    }

    /// A null `endptr` is accepted, as POSIX requires.
    #[test]
    fn wcstold_tolerates_a_null_endptr() {
        let s = wide("2.5");
        let v = unsafe { wcstold(s.as_ptr(), core::ptr::null_mut()) };
        assert_eq!((v.sign_exp, v.significand), (0x4000, 0xA000_0000_0000_0000));
    }

    /// Delegating to `wcstod` and narrowing rounded twice: this value sits a
    /// quarter of an `f64` ulp above the midpoint between `1.0f32` and its
    /// successor, so it must round *up*, but in `f64` it lands exactly on the
    /// midpoint and ties-to-even then sends it back down to `1.0f32`.
    #[test]
    fn wcstof_rounds_once_not_twice() {
        let text = "1.000000059604644830901776231257827021181583404541015625";
        let s = wide(text);
        let val = unsafe { wcstof(s.as_ptr(), core::ptr::null_mut()) };
        assert_eq!(
            val.to_bits(),
            0x3f80_0001,
            "expected the successor of 1.0f32"
        );
        assert_eq!(val, text.parse::<f32>().unwrap());
    }

    /// The extremes of the range, which the old buffer could not even spell.
    #[test]
    fn wcstod_spans_the_whole_range() {
        let dbl_max = "1.7976931348623157e308";
        assert_eq!(parse_wide(dbl_max), f64::MAX);
        // The smallest subnormal, written out in full: 751 characters.
        let denorm_min = format!("{:.1074}", f64::from_bits(1));
        assert!(denorm_min.len() > 700);
        assert_eq!(parse_wide(&denorm_min), f64::from_bits(1));
        assert_eq!(parse_wide("1e309"), f64::INFINITY);
        assert_eq!(parse_wide("1e-400"), 0.0);
    }

    /// Rust's parser is correctly rounded, so it is the oracle.
    #[test]
    fn wcstod_matches_rusts_parser_over_a_sweep() {
        let mut seed: u64 = 0x5eed_1234_9abc_def0;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..4000 {
            let digits = 1 + (next() % 40) as usize;
            let mut text = String::new();
            for _ in 0..digits {
                text.push(char::from(b'0' + (next() % 10) as u8));
            }
            text.push('.');
            for _ in 0..digits {
                text.push(char::from(b'0' + (next() % 10) as u8));
            }
            let exp = (next() % 61) as i32 - 30;
            text.push_str(&format!("e{exp}"));

            let expected: f64 = text.parse().unwrap();
            assert_eq!(parse_wide(&text), expected, "wcstod({text})");

            let s = wide(&text);
            let got32 = unsafe { wcstof(s.as_ptr(), core::ptr::null_mut()) };
            assert_eq!(got32, text.parse::<f32>().unwrap(), "wcstof({text})");
        }
    }

    // -----------------------------------------------------------------------
    // wcsstr
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcsstr_found() {
        let hay: &[WcharT] = &[
            b'h' as i32,
            b'e' as i32,
            b'l' as i32,
            b'l' as i32,
            b'o' as i32,
            0,
        ];
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

    /// `wcsstr` (the Two-Way search) against trying every place: every
    /// needle and haystack over two alphabets, one of them with characters
    /// past the BMP and a negative `wchar_t`, which the search orders as
    /// signed numbers.
    #[test]
    fn wcsstr_agrees_with_trying_every_place() {
        /// Every string over `alphabet` up to `max_len` long.
        fn all(alphabet: &[WcharT], max_len: usize) -> Vec<Vec<WcharT>> {
            let mut all = vec![Vec::new()];
            let mut longest: Vec<Vec<WcharT>> = vec![Vec::new()];
            for _ in 0..max_len {
                longest = longest
                    .iter()
                    .flat_map(|s| {
                        alphabet.iter().map(move |&c| {
                            let mut t = s.clone();
                            t.push(c);
                            t
                        })
                    })
                    .collect();
                all.extend(longest.iter().cloned());
            }
            all
        }
        for (alphabet, needles, hays) in
            [(&[0x61, 0x62][..], 6, 10), (&[0x41, 0x1F600, -5][..], 4, 7)]
        {
            let needles = all(alphabet, needles);
            let hays = all(alphabet, hays);
            for needle in &needles {
                let mut n = needle.clone();
                n.push(0);
                for hay in &hays {
                    let want = if needle.is_empty() {
                        Some(0)
                    } else {
                        hay.windows(needle.len()).position(|w| w == &needle[..])
                    };
                    let mut h = hay.clone();
                    h.push(0);
                    // SAFETY: terminated wide strings.
                    let got = unsafe { wcsstr(h.as_ptr(), n.as_ptr()) };
                    let got = (!got.is_null()).then(|| (got as usize - h.as_ptr() as usize) / 4);
                    assert_eq!(got, want, "wcsstr({hay:?}, {needle:?})");
                }
            }
        }
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
        let s: &[WcharT] = &[
            b'h' as i32,
            b'e' as i32,
            b'l' as i32,
            b'l' as i32,
            b'o' as i32,
            0,
        ];
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
            b'o' as i32,
            b'n' as i32,
            b'e' as i32,
            b',' as i32,
            b't' as i32,
            b'w' as i32,
            b'o' as i32,
            b',' as i32,
            b't' as i32,
            b'h' as i32,
            b'r' as i32,
            b'e' as i32,
            b'e' as i32,
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
        let s1: &[WcharT] = &[
            b'H' as i32,
            b'E' as i32,
            b'L' as i32,
            b'L' as i32,
            b'O' as i32,
            0,
        ];
        let s2: &[WcharT] = &[
            b'h' as i32,
            b'e' as i32,
            b'l' as i32,
            b'l' as i32,
            b'o' as i32,
            0,
        ];
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

    // -------------------------------------------------------------------
    // Wide character classification (isw* functions)
    // -------------------------------------------------------------------

    #[test]
    fn test_iswalnum_alpha() {
        assert_ne!(iswalnum(b'A' as i32), 0);
        assert_ne!(iswalnum(b'z' as i32), 0);
        assert_ne!(iswalnum(b'5' as i32), 0);
        assert_eq!(iswalnum(b' ' as i32), 0);
        assert_eq!(iswalnum(b'!' as i32), 0);
    }

    #[test]
    fn test_iswalpha_letters_only() {
        assert_ne!(iswalpha(b'A' as i32), 0);
        assert_ne!(iswalpha(b'Z' as i32), 0);
        assert_ne!(iswalpha(b'a' as i32), 0);
        assert_ne!(iswalpha(b'z' as i32), 0);
        assert_eq!(iswalpha(b'0' as i32), 0);
        assert_eq!(iswalpha(b' ' as i32), 0);
    }

    #[test]
    fn test_iswdigit_digits() {
        assert_ne!(iswdigit(b'0' as i32), 0);
        assert_ne!(iswdigit(b'9' as i32), 0);
        assert_eq!(iswdigit(b'a' as i32), 0);
        assert_eq!(iswdigit(b'/' as i32), 0);
        assert_eq!(iswdigit(b':' as i32), 0); // just past '9'
    }

    #[test]
    fn test_iswxdigit_hex() {
        assert_ne!(iswxdigit(b'0' as i32), 0);
        assert_ne!(iswxdigit(b'9' as i32), 0);
        assert_ne!(iswxdigit(b'a' as i32), 0);
        assert_ne!(iswxdigit(b'f' as i32), 0);
        assert_ne!(iswxdigit(b'A' as i32), 0);
        assert_ne!(iswxdigit(b'F' as i32), 0);
        assert_eq!(iswxdigit(b'g' as i32), 0);
        assert_eq!(iswxdigit(b'G' as i32), 0);
    }

    #[test]
    fn test_iswspace_whitespace() {
        assert_ne!(iswspace(b' ' as i32), 0);
        assert_ne!(iswspace(b'\t' as i32), 0);
        assert_ne!(iswspace(b'\n' as i32), 0);
        assert_ne!(iswspace(b'\r' as i32), 0);
        assert_ne!(iswspace(0x0b), 0); // vertical tab
        assert_ne!(iswspace(0x0c), 0); // form feed
        assert_eq!(iswspace(b'a' as i32), 0);
    }

    #[test]
    fn test_iswblank_tab_space() {
        assert_ne!(iswblank(b' ' as i32), 0);
        assert_ne!(iswblank(b'\t' as i32), 0);
        assert_eq!(iswblank(b'\n' as i32), 0);
        assert_eq!(iswblank(b'a' as i32), 0);
    }

    #[test]
    fn test_iswupper_lowercase() {
        assert_ne!(iswupper(b'A' as i32), 0);
        assert_ne!(iswupper(b'Z' as i32), 0);
        assert_eq!(iswupper(b'a' as i32), 0);
        assert_eq!(iswupper(b'0' as i32), 0);
    }

    #[test]
    fn test_iswlower_uppercase() {
        assert_ne!(iswlower(b'a' as i32), 0);
        assert_ne!(iswlower(b'z' as i32), 0);
        assert_eq!(iswlower(b'A' as i32), 0);
        assert_eq!(iswlower(b'0' as i32), 0);
    }

    #[test]
    fn test_iswpunct_punctuation() {
        assert_ne!(iswpunct(b'!' as i32), 0);
        assert_ne!(iswpunct(b'.' as i32), 0);
        assert_ne!(iswpunct(b',' as i32), 0);
        assert_ne!(iswpunct(b'@' as i32), 0);
        assert_eq!(iswpunct(b'A' as i32), 0);
        assert_eq!(iswpunct(b'0' as i32), 0);
        assert_eq!(iswpunct(b' ' as i32), 0);
    }

    #[test]
    fn test_iswgraph_printable_not_space() {
        assert_ne!(iswgraph(b'A' as i32), 0);
        assert_ne!(iswgraph(b'!' as i32), 0);
        assert_ne!(iswgraph(b'0' as i32), 0);
        assert_eq!(iswgraph(b' ' as i32), 0); // space is not graph
        assert_eq!(iswgraph(0x00), 0); // NUL is not graph
    }

    // -------------------------------------------------------------------
    // Wide character case conversion (towlower/towupper)
    // -------------------------------------------------------------------

    #[test]
    fn test_towlower_conversion() {
        assert_eq!(towlower(b'A' as i32), b'a' as i32);
        assert_eq!(towlower(b'Z' as i32), b'z' as i32);
        // Already lowercase — no change.
        assert_eq!(towlower(b'a' as i32), b'a' as i32);
        // Non-alpha — no change.
        assert_eq!(towlower(b'5' as i32), b'5' as i32);
    }

    #[test]
    fn test_towupper_conversion() {
        assert_eq!(towupper(b'a' as i32), b'A' as i32);
        assert_eq!(towupper(b'z' as i32), b'Z' as i32);
        // Already uppercase — no change.
        assert_eq!(towupper(b'A' as i32), b'A' as i32);
        // Non-alpha — no change.
        assert_eq!(towupper(b'!' as i32), b'!' as i32);
    }

    // -------------------------------------------------------------------
    // btowc / wctob — byte ↔ wide character
    // -------------------------------------------------------------------

    #[test]
    fn test_btowc_ascii() {
        assert_eq!(btowc(b'A' as i32), b'A' as i32);
        assert_eq!(btowc(0), 0); // NUL
        assert_eq!(btowc(127), 127); // DEL
    }

    #[test]
    fn test_btowc_non_ascii() {
        // Values > 127 return WEOF (-1).
        assert_eq!(btowc(128), -1);
        assert_eq!(btowc(255), -1);
        assert_eq!(btowc(-1), -1); // EOF
    }

    #[test]
    fn test_wctob_ascii() {
        assert_eq!(wctob(b'A' as i32), b'A' as i32);
        assert_eq!(wctob(0), 0);
        assert_eq!(wctob(127), 127);
    }

    #[test]
    fn test_wctob_non_ascii() {
        assert_eq!(wctob(128), -1);
        assert_eq!(wctob(0x1000), -1);
        assert_eq!(wctob(-1), -1); // WEOF
    }

    // -------------------------------------------------------------------
    // mbsinit
    // -------------------------------------------------------------------

    #[test]
    fn test_mbsinit_null_is_initial() {
        assert_ne!(mbsinit(core::ptr::null()), 0);
    }

    #[test]
    fn test_mbsinit_fresh_state() {
        let state = MbstateT::new();
        assert_ne!(mbsinit(&raw const state), 0);
    }

    // -------------------------------------------------------------------
    // nl_langinfo (delegated to crate::langinfo)
    // -------------------------------------------------------------------

    #[test]
    fn test_nl_langinfo_codeset() {
        let s = crate::langinfo::nl_langinfo(crate::langinfo::CODESET);
        assert!(!s.is_null());
        // Should return "ANSI_X3.4-1968" (C locale).
        assert_eq!(unsafe { *s }, b'A');
    }

    #[test]
    fn test_nl_langinfo_radixchar() {
        let s = crate::langinfo::nl_langinfo(crate::langinfo::RADIXCHAR);
        assert!(!s.is_null());
        assert_eq!(unsafe { *s }, b'.');
    }

    #[test]
    fn test_nl_langinfo_unknown_item() {
        let s = crate::langinfo::nl_langinfo(9999);
        assert!(!s.is_null());
        // Should return empty string.
        assert_eq!(unsafe { *s }, 0);
    }

    // -- wmemcpy --

    #[test]
    fn test_wmemcpy_basic() {
        let src: [WcharT; 4] = [0x41, 0x42, 0x43, 0x44]; // ABCD
        let mut dst: [WcharT; 4] = [0; 4];
        let ret = unsafe { wmemcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(dst, src);
    }

    #[test]
    fn test_wmemcpy_zero_length() {
        let src: [WcharT; 2] = [0x41, 0x42];
        let mut dst: [WcharT; 2] = [0; 2];
        let ret = unsafe { wmemcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(dst, [0, 0]); // unchanged
    }

    #[test]
    fn test_wmemcpy_unicode() {
        let src: [WcharT; 3] = [0x1F600, 0x2764, 0x1F4A9]; // 😀❤💩
        let mut dst: [WcharT; 3] = [0; 3];
        unsafe { wmemcpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(dst, src);
    }

    // -- wmemset --

    #[test]
    fn test_wmemset_basic() {
        let mut buf: [WcharT; 4] = [0; 4];
        let ret = unsafe { wmemset(buf.as_mut_ptr(), 0x58, 4) }; // 'X'
        assert_eq!(ret, buf.as_mut_ptr());
        assert_eq!(buf, [0x58, 0x58, 0x58, 0x58]);
    }

    #[test]
    fn test_wmemset_zero_length() {
        let mut buf: [WcharT; 2] = [0x41, 0x42];
        unsafe { wmemset(buf.as_mut_ptr(), 0x58, 0) };
        assert_eq!(buf, [0x41, 0x42]); // unchanged
    }

    #[test]
    fn test_wmemset_unicode_value() {
        let mut buf: [WcharT; 3] = [0; 3];
        unsafe { wmemset(buf.as_mut_ptr(), 0x1F600, 3) }; // 😀
        assert_eq!(buf, [0x1F600, 0x1F600, 0x1F600]);
    }

    // -- wmemcmp --

    #[test]
    fn test_wmemcmp_equal() {
        let a: [WcharT; 3] = [0x41, 0x42, 0x43];
        let b: [WcharT; 3] = [0x41, 0x42, 0x43];
        assert_eq!(unsafe { wmemcmp(a.as_ptr(), b.as_ptr(), 3) }, 0);
    }

    #[test]
    fn test_wmemcmp_less() {
        let a: [WcharT; 3] = [0x41, 0x42, 0x43];
        let b: [WcharT; 3] = [0x41, 0x42, 0x44];
        assert!(unsafe { wmemcmp(a.as_ptr(), b.as_ptr(), 3) } < 0);
    }

    #[test]
    fn test_wmemcmp_greater() {
        let a: [WcharT; 3] = [0x41, 0x42, 0x45];
        let b: [WcharT; 3] = [0x41, 0x42, 0x43];
        assert!(unsafe { wmemcmp(a.as_ptr(), b.as_ptr(), 3) } > 0);
    }

    #[test]
    fn test_wmemcmp_zero_length() {
        let a: [WcharT; 2] = [0x41, 0x42];
        let b: [WcharT; 2] = [0x43, 0x44];
        assert_eq!(unsafe { wmemcmp(a.as_ptr(), b.as_ptr(), 0) }, 0);
    }

    // -- wmemchr --

    #[test]
    fn test_wmemchr_found() {
        let data: [WcharT; 4] = [0x41, 0x42, 0x43, 0x44];
        let ret = unsafe { wmemchr(data.as_ptr(), 0x43, 4) };
        assert!(!ret.is_null());
        assert_eq!(ret, unsafe { data.as_ptr().add(2) });
    }

    #[test]
    fn test_wmemchr_not_found() {
        let data: [WcharT; 3] = [0x41, 0x42, 0x43];
        let ret = unsafe { wmemchr(data.as_ptr(), 0x58, 3) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_wmemchr_zero_length() {
        let data: [WcharT; 3] = [0x41, 0x42, 0x43];
        let ret = unsafe { wmemchr(data.as_ptr(), 0x41, 0) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_wmemchr_first_element() {
        let data: [WcharT; 3] = [0x58, 0x41, 0x42];
        let ret = unsafe { wmemchr(data.as_ptr(), 0x58, 3) };
        assert_eq!(ret, data.as_ptr());
    }

    // -- wmemmove --

    #[test]
    fn test_wmemmove_no_overlap() {
        let src: [WcharT; 3] = [0x41, 0x42, 0x43];
        let mut dst: [WcharT; 3] = [0; 3];
        let ret = unsafe { wmemmove(dst.as_mut_ptr(), src.as_ptr(), 3) };
        assert_eq!(ret, dst.as_mut_ptr());
        assert_eq!(dst, src);
    }

    #[test]
    fn test_wmemmove_overlap_forward() {
        let mut buf: [WcharT; 5] = [1, 2, 3, 4, 5];
        // Move [1,2,3] to position [1..4], overlapping.
        unsafe { wmemmove(buf.as_mut_ptr().add(1), buf.as_ptr(), 3) };
        assert_eq!(buf, [1, 1, 2, 3, 5]);
    }

    #[test]
    fn test_wmemmove_overlap_backward() {
        let mut buf: [WcharT; 5] = [1, 2, 3, 4, 5];
        // Move [2,3,4] to position [0..3], overlapping.
        unsafe { wmemmove(buf.as_mut_ptr(), buf.as_ptr().add(1), 3) };
        assert_eq!(buf, [2, 3, 4, 4, 5]);
    }

    #[test]
    fn test_wmemmove_zero_length() {
        let mut buf: [WcharT; 3] = [1, 2, 3];
        unsafe { wmemmove(buf.as_mut_ptr(), buf.as_ptr().add(1), 0) };
        assert_eq!(buf, [1, 2, 3]); // unchanged
    }

    #[test]
    fn test_wmemmove_same_pointer() {
        let mut buf: [WcharT; 3] = [0x41, 0x42, 0x43];
        unsafe { wmemmove(buf.as_mut_ptr(), buf.as_ptr(), 3) };
        assert_eq!(buf, [0x41, 0x42, 0x43]); // unchanged
    }

    // -- wmempcpy --

    #[test]
    fn test_wmempcpy_returns_past_end() {
        let src: [WcharT; 3] = [0x41, 0x42, 0x43];
        let mut dst: [WcharT; 3] = [0; 3];
        let ret = unsafe { wmempcpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        // Should return pointer past the last element written.
        assert_eq!(ret, unsafe { dst.as_mut_ptr().add(3) });
        assert_eq!(dst, src);
    }

    #[test]
    fn test_wmempcpy_zero_length() {
        let src: [WcharT; 2] = [0x41, 0x42];
        let mut dst: [WcharT; 2] = [0; 2];
        let ret = unsafe { wmempcpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(ret, dst.as_mut_ptr()); // base pointer (0 advance)
        assert_eq!(dst, [0, 0]); // unchanged
    }

    // -- wcscat --

    #[test]
    fn test_wcscat_basic() {
        let mut buf: [WcharT; 8] = [0; 8];
        buf[0] = 0x41; // 'A'
        buf[1] = 0x42; // 'B'
        buf[2] = 0;
        let src: [WcharT; 4] = [0x43, 0x44, 0x45, 0]; // "CDE"
        let ret = unsafe { wcscat(buf.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(ret, buf.as_mut_ptr());
        assert_eq!(&buf[..6], &[0x41, 0x42, 0x43, 0x44, 0x45, 0]);
    }

    #[test]
    fn test_wcscat_empty_src() {
        let mut buf: [WcharT; 4] = [0x41, 0x42, 0, 0];
        let src: [WcharT; 1] = [0]; // empty
        unsafe { wcscat(buf.as_mut_ptr(), src.as_ptr()) };
        assert_eq!(&buf[..3], &[0x41, 0x42, 0]);
    }

    // -- wcsncpy --

    #[test]
    fn test_wcsncpy_basic() {
        let src: [WcharT; 4] = [0x41, 0x42, 0x43, 0]; // "ABC"
        let mut dst: [WcharT; 5] = [0x99; 5];
        let ret = unsafe { wcsncpy(dst.as_mut_ptr(), src.as_ptr(), 5) };
        assert_eq!(ret, dst.as_mut_ptr());
        // Should copy "ABC\0" then pad with null.
        assert_eq!(dst, [0x41, 0x42, 0x43, 0, 0]);
    }

    #[test]
    fn test_wcsncpy_truncation() {
        let src: [WcharT; 6] = [0x41, 0x42, 0x43, 0x44, 0x45, 0]; // "ABCDE"
        let mut dst: [WcharT; 3] = [0; 3];
        unsafe { wcsncpy(dst.as_mut_ptr(), src.as_ptr(), 3) };
        // Copies exactly 3 chars, no null terminator.
        assert_eq!(dst, [0x41, 0x42, 0x43]);
    }

    #[test]
    fn test_wcsncpy_zero_n() {
        let src: [WcharT; 2] = [0x41, 0];
        let mut dst: [WcharT; 2] = [0x99, 0x99];
        unsafe { wcsncpy(dst.as_mut_ptr(), src.as_ptr(), 0) };
        assert_eq!(dst, [0x99, 0x99]); // unchanged
    }

    // -- wcscoll / wcscoll_l --

    #[test]
    fn test_wcscoll_equal() {
        let a: [WcharT; 4] = [0x41, 0x42, 0x43, 0];
        let b: [WcharT; 4] = [0x41, 0x42, 0x43, 0];
        assert_eq!(unsafe { wcscoll(a.as_ptr(), b.as_ptr()) }, 0);
    }

    #[test]
    fn test_wcscoll_less() {
        let a: [WcharT; 2] = [0x41, 0]; // "A"
        let b: [WcharT; 2] = [0x42, 0]; // "B"
        assert!(unsafe { wcscoll(a.as_ptr(), b.as_ptr()) } < 0);
    }

    #[test]
    fn test_wcscoll_l_delegates() {
        let a: [WcharT; 3] = [0x58, 0x59, 0];
        let b: [WcharT; 3] = [0x58, 0x59, 0];
        assert_eq!(unsafe { wcscoll_l(a.as_ptr(), b.as_ptr(), 0) }, 0);
    }

    // -- wcsxfrm / wcsxfrm_l --

    #[test]
    fn test_wcsxfrm_basic() {
        let src: [WcharT; 4] = [0x41, 0x42, 0x43, 0]; // "ABC"
        let mut dst: [WcharT; 8] = [0; 8];
        let len = unsafe { wcsxfrm(dst.as_mut_ptr(), src.as_ptr(), 8) };
        assert_eq!(len, 3); // length of src
        assert_eq!(&dst[..4], &[0x41, 0x42, 0x43, 0]);
    }

    #[test]
    fn test_wcsxfrm_zero_n() {
        let src: [WcharT; 3] = [0x41, 0x42, 0];
        let len = unsafe { wcsxfrm(core::ptr::null_mut(), src.as_ptr(), 0) };
        assert_eq!(len, 2); // Returns length without writing.
    }

    #[test]
    fn test_wcsxfrm_l_delegates() {
        let src: [WcharT; 3] = [0x41, 0x42, 0];
        let len = unsafe { wcsxfrm_l(core::ptr::null_mut(), src.as_ptr(), 0, 0) };
        assert_eq!(len, 2);
    }

    // -- mbrtowc --
    // Each test uses its own MbstateT to avoid polluting shared global state.

    #[test]
    fn test_mbrtowc_ascii() {
        let mut wc: WcharT = 0;
        let mut state = MbstateT::new();
        let ret = unsafe { mbrtowc(&mut wc, b"A".as_ptr(), 1, &mut state) };
        assert_eq!(ret, 1);
        assert_eq!(wc, 0x41);
    }

    #[test]
    fn test_mbrtowc_null_byte() {
        let mut wc: WcharT = 0x99;
        let mut state = MbstateT::new();
        let ret = unsafe { mbrtowc(&mut wc, b"\0".as_ptr(), 1, &mut state) };
        assert_eq!(ret, 0);
        assert_eq!(wc, 0);
    }

    #[test]
    fn test_mbrtowc_two_byte_utf8() {
        let mut wc: WcharT = 0;
        let mut state = MbstateT::new();
        // é = U+00E9 = 0xC3 0xA9
        let ret = unsafe { mbrtowc(&mut wc, b"\xC3\xA9".as_ptr(), 2, &mut state) };
        assert_eq!(ret, 2);
        assert_eq!(wc, 0xE9);
    }

    #[test]
    fn test_mbrtowc_three_byte_utf8() {
        let mut wc: WcharT = 0;
        let mut state = MbstateT::new();
        // ❤ = U+2764 = 0xE2 0x9D 0xA4
        let ret = unsafe { mbrtowc(&mut wc, b"\xE2\x9D\xA4".as_ptr(), 3, &mut state) };
        assert_eq!(ret, 3);
        assert_eq!(wc, 0x2764);
    }

    #[test]
    fn test_mbrtowc_four_byte_utf8() {
        let mut wc: WcharT = 0;
        let mut state = MbstateT::new();
        // 😀 = U+1F600 = 0xF0 0x9F 0x98 0x80
        let ret = unsafe { mbrtowc(&mut wc, b"\xF0\x9F\x98\x80".as_ptr(), 4, &mut state) };
        assert_eq!(ret, 4);
        assert_eq!(wc, 0x1F600);
    }

    #[test]
    fn test_mbrtowc_incomplete() {
        let mut wc: WcharT = 0;
        let mut state = MbstateT::new();
        // Only 1 byte of a 2-byte sequence.
        let ret = unsafe { mbrtowc(&mut wc, b"\xC3".as_ptr(), 1, &mut state) };
        // -2 (incomplete sequence)
        assert_eq!(ret, usize::MAX.wrapping_sub(1));
    }

    #[test]
    fn test_mbrtowc_null_s_resets_state() {
        let mut state = MbstateT::new();
        let ret = unsafe { mbrtowc(core::ptr::null_mut(), core::ptr::null(), 0, &mut state) };
        assert_eq!(ret, 0);
    }

    // -- wcrtomb --

    #[test]
    fn test_wcrtomb_ascii() {
        let mut buf = [0u8; 4];
        let ret = unsafe { wcrtomb(buf.as_mut_ptr(), 0x41, core::ptr::null_mut()) };
        assert_eq!(ret, 1);
        assert_eq!(buf[0], b'A');
    }

    #[test]
    fn test_wcrtomb_two_byte() {
        let mut buf = [0u8; 4];
        // é = U+00E9
        let ret = unsafe { wcrtomb(buf.as_mut_ptr(), 0xE9, core::ptr::null_mut()) };
        assert_eq!(ret, 2);
        assert_eq!(&buf[..2], b"\xC3\xA9");
    }

    #[test]
    fn test_wcrtomb_four_byte() {
        let mut buf = [0u8; 4];
        // 😀 = U+1F600
        let ret = unsafe { wcrtomb(buf.as_mut_ptr(), 0x1F600, core::ptr::null_mut()) };
        assert_eq!(ret, 4);
        assert_eq!(&buf, b"\xF0\x9F\x98\x80");
    }

    #[test]
    fn test_wcrtomb_null_s_returns_one() {
        let ret = unsafe { wcrtomb(core::ptr::null_mut(), 0x41, core::ptr::null_mut()) };
        assert_eq!(ret, 1); // Reset to initial state.
    }

    #[test]
    fn test_wcrtomb_invalid_negative() {
        let mut buf = [0u8; 4];
        let ret = unsafe { wcrtomb(buf.as_mut_ptr(), -1, core::ptr::null_mut()) };
        assert_eq!(ret, usize::MAX); // EILSEQ
    }

    // -- mbrlen --

    #[test]
    fn test_mbrlen_ascii() {
        let mut state = MbstateT::new();
        let ret = unsafe { mbrlen(b"A".as_ptr(), 1, &mut state) };
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_mbrlen_multibyte() {
        let mut state = MbstateT::new();
        // é = 0xC3 0xA9
        let ret = unsafe { mbrlen(b"\xC3\xA9".as_ptr(), 2, &mut state) };
        assert_eq!(ret, 2);
    }

    #[test]
    fn test_mbrlen_null_byte() {
        let mut state = MbstateT::new();
        let ret = unsafe { mbrlen(b"\0".as_ptr(), 1, &mut state) };
        assert_eq!(ret, 0);
    }

    // -- mbsrtowcs --

    #[test]
    fn test_mbsrtowcs_basic() {
        let input = b"ABC\0";
        let mut src: *const u8 = input.as_ptr();
        let mut dst: [WcharT; 4] = [0; 4];
        let mut state = MbstateT::new();
        let ret = unsafe { mbsrtowcs(dst.as_mut_ptr(), &mut src, 4, &mut state) };
        assert_eq!(ret, 3);
        assert_eq!(&dst[..4], &[0x41, 0x42, 0x43, 0]);
        assert!(src.is_null()); // Entire string consumed.
    }

    #[test]
    fn test_mbsrtowcs_utf8() {
        // "é" (U+00E9) = 0xC3 0xA9
        let input = b"\xC3\xA9\0";
        let mut src: *const u8 = input.as_ptr();
        let mut dst: [WcharT; 4] = [0; 4];
        let mut state = MbstateT::new();
        let ret = unsafe { mbsrtowcs(dst.as_mut_ptr(), &mut src, 4, &mut state) };
        assert_eq!(ret, 1);
        assert_eq!(dst[0], 0xE9);
    }

    // -- wcsrtombs --

    #[test]
    fn test_wcsrtombs_basic() {
        let input: [WcharT; 4] = [0x41, 0x42, 0x43, 0]; // "ABC"
        let mut src: *const WcharT = input.as_ptr();
        let mut dst = [0u8; 8];
        let mut state = MbstateT::new();
        let ret = unsafe { wcsrtombs(dst.as_mut_ptr(), &mut src, 8, &mut state) };
        assert_eq!(ret, 3);
        assert_eq!(&dst[..4], b"ABC\0");
        assert!(src.is_null()); // Entire string consumed.
    }

    #[test]
    fn test_wcsrtombs_utf8() {
        let input: [WcharT; 2] = [0xE9, 0]; // "é"
        let mut src: *const WcharT = input.as_ptr();
        let mut dst = [0u8; 8];
        let mut state = MbstateT::new();
        let ret = unsafe { wcsrtombs(dst.as_mut_ptr(), &mut src, 8, &mut state) };
        assert_eq!(ret, 2);
        assert_eq!(&dst[..2], b"\xC3\xA9");
    }

    // -- wcstof --

    #[test]
    fn test_wcstof_basic() {
        let s: [WcharT; 5] = [0x33, 0x2E, 0x31, 0x34, 0]; // "3.14"
        let mut end: *const WcharT = core::ptr::null();
        let val = unsafe { wcstof(s.as_ptr(), &mut end) };
        let diff = (val - 3.14_f32).abs();
        assert!(diff < 0.001);
    }

    // -- wcstoll --

    #[test]
    fn test_wcstoll_basic() {
        let s: [WcharT; 4] = [0x31, 0x32, 0x33, 0]; // "123"
        let val = unsafe { wcstoll(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, 123);
    }

    #[test]
    fn test_wcstoll_negative() {
        let s: [WcharT; 5] = [0x2D, 0x34, 0x32, 0, 0]; // "-42"
        let val = unsafe { wcstoll(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, -42);
    }

    // -- wcstoull --

    #[test]
    fn test_wcstoull_basic() {
        let s: [WcharT; 4] = [0x39, 0x39, 0x39, 0]; // "999"
        let val = unsafe { wcstoull(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(val, 999);
    }

    #[test]
    fn test_wcstoull_hex() {
        let s: [WcharT; 5] = [0x30, 0x78, 0x46, 0x46, 0]; // "0xFF"
        let val = unsafe { wcstoull(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(val, 255);
    }

    // -- wcsdup --

    #[test]
    fn test_wcsdup_null() {
        let ret = unsafe { wcsdup(core::ptr::null()) };
        assert!(ret.is_null());
    }

    // -- mbsnrtowcs --

    #[test]
    fn test_mbsnrtowcs_byte_limited() {
        let input = b"ABCDEF\0";
        let mut src: *const u8 = input.as_ptr();
        let mut dst: [WcharT; 8] = [0; 8];
        let mut state = MbstateT::new();
        // Limit to 3 bytes.
        let ret = unsafe { mbsnrtowcs(dst.as_mut_ptr(), &mut src, 3, 8, &mut state) };
        assert_eq!(ret, 3);
        assert_eq!(&dst[..3], &[0x41, 0x42, 0x43]);
    }

    // -- wcsnrtombs --

    #[test]
    fn test_wcsnrtombs_wchar_limited() {
        let input: [WcharT; 6] = [0x41, 0x42, 0x43, 0x44, 0x45, 0]; // "ABCDE"
        let mut src: *const WcharT = input.as_ptr();
        let mut dst = [0u8; 16];
        let mut state = MbstateT::new();
        // Limit to 3 wide chars.
        let ret = unsafe { wcsnrtombs(dst.as_mut_ptr(), &mut src, 3, 16, &mut state) };
        assert_eq!(ret, 3);
        assert_eq!(&dst[..3], b"ABC");
    }

    // The wide stream calls (fputwc, fgetwc, ungetwc, fputws, fgetws) are
    // tested in stdio.rs, over in-memory streams whose bytes can be checked.

    // -- wcswidth --

    #[test]
    fn test_wcswidth_null_returns_negative() {
        let ret = unsafe { wcswidth(core::ptr::null(), 10) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_wcswidth_empty_string() {
        let s: [WcharT; 1] = [0];
        let ret = unsafe { wcswidth(s.as_ptr(), 1) };
        assert_eq!(ret, 0, "empty string has width 0");
    }

    #[test]
    fn test_wcswidth_ascii_string() {
        // "Hello" — 5 printable ASCII chars, each width 1.
        let s: [WcharT; 6] = [0x48, 0x65, 0x6C, 0x6C, 0x6F, 0];
        let ret = unsafe { wcswidth(s.as_ptr(), 6) };
        assert_eq!(ret, 5);
    }

    #[test]
    fn test_wcswidth_with_n_limit() {
        // "Hello" but only measure first 3 chars.
        let s: [WcharT; 6] = [0x48, 0x65, 0x6C, 0x6C, 0x6F, 0];
        let ret = unsafe { wcswidth(s.as_ptr(), 3) };
        assert_eq!(ret, 3);
    }

    #[test]
    fn test_wcswidth_control_char_returns_negative() {
        // A control character (U+0001) has width -1 → wcswidth returns -1.
        let s: [WcharT; 2] = [0x01, 0];
        let ret = unsafe { wcswidth(s.as_ptr(), 2) };
        assert_eq!(ret, -1, "control char makes wcswidth return -1");
    }

    // -- wcsftime --

    /// A NULL buffer is written nowhere and answers the length, as glibc's
    /// does -- when the length and the NUL fit `maxsize`, and 0 when not.
    #[test]
    fn test_wcsftime_null_wcs_counts() {
        let tm = crate::time::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 100,
            tm_wday: 6,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        };
        let fmt: [WcharT; 3] = [b'%' as WcharT, b'Y' as WcharT, 0];
        let ret = unsafe { wcsftime(core::ptr::null_mut(), 64, fmt.as_ptr(), &tm) };
        assert_eq!(ret, 4);
        let ret = unsafe { wcsftime(core::ptr::null_mut(), 4, fmt.as_ptr(), &tm) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_wcsftime_null_format_returns_zero() {
        let tm = crate::time::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 100,
            tm_wday: 6,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        };
        let mut buf: [WcharT; 64] = [0; 64];
        let ret = unsafe { wcsftime(buf.as_mut_ptr(), 64, core::ptr::null(), &tm) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_wcsftime_null_tm_returns_zero() {
        let fmt: [WcharT; 3] = [b'%' as WcharT, b'Y' as WcharT, 0];
        let mut buf: [WcharT; 64] = [0; 64];
        let ret = unsafe { wcsftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), core::ptr::null()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_wcsftime_zero_maxsize_returns_zero() {
        let tm = crate::time::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 100,
            tm_wday: 6,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        };
        let fmt: [WcharT; 3] = [b'%' as WcharT, b'Y' as WcharT, 0];
        let mut buf: [WcharT; 64] = [0; 64];
        let ret = unsafe { wcsftime(buf.as_mut_ptr(), 0, fmt.as_ptr(), &tm) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_wcsftime_year_format() {
        // Format "%Y" for year 2000 (tm_year = 100 = 1900+100).
        let tm = crate::time::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 100,
            tm_wday: 6,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        };
        let fmt: [WcharT; 3] = [b'%' as WcharT, b'Y' as WcharT, 0];
        let mut buf: [WcharT; 64] = [0; 64];
        let ret = unsafe { wcsftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm) };
        assert_eq!(ret, 4, "year 2000 is 4 chars");
        assert_eq!(buf[0], b'2' as WcharT);
        assert_eq!(buf[1], b'0' as WcharT);
        assert_eq!(buf[2], b'0' as WcharT);
        assert_eq!(buf[3], b'0' as WcharT);
        assert_eq!(buf[4], 0, "null terminated");
    }

    #[test]
    fn test_wcsftime_literal_text() {
        // Format "hi" (no % specifiers) should output "hi".
        let tm = crate::time::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 1,
            tm_mon: 0,
            tm_year: 100,
            tm_wday: 6,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        };
        let fmt: [WcharT; 3] = [b'h' as WcharT, b'i' as WcharT, 0];
        let mut buf: [WcharT; 64] = [0; 64];
        let ret = unsafe { wcsftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm) };
        assert_eq!(ret, 2);
        assert_eq!(buf[0], b'h' as WcharT);
        assert_eq!(buf[1], b'i' as WcharT);
    }

    /// `wcschrnul` finds as `wcschr` does, and the NUL where `wcschr` finds
    /// nothing; searching for NUL finds the NUL.
    #[test]
    fn wcschrnul_ends_at_the_nul() {
        let s: [WcharT; 4] = [b'a' as WcharT, b'b' as WcharT, b'c' as WcharT, 0];
        let p = s.as_ptr();
        // SAFETY: a NUL-terminated wide string.
        unsafe {
            assert_eq!(wcschrnul(p, b'b' as WcharT), p.add(1));
            assert_eq!(wcschrnul(p, b'z' as WcharT), p.add(3));
            assert_eq!(wcschrnul(p, 0), p.add(3));
            assert!(wcschr(p, b'z' as WcharT).is_null());
        }
    }

    /// `wcslcpy` and `wcslcat` as the BSDs' and glibc 2.38's: what fits, a NUL
    /// always, and the length the whole would have had.
    #[test]
    fn wcslcpy_and_wcslcat_cut_short_and_say_so() {
        let w = |s: &str| -> std::vec::Vec<WcharT> {
            s.chars()
                .map(|c| c as WcharT)
                .chain(core::iter::once(0))
                .collect()
        };
        let src = w("hello");
        let mut dst: [WcharT; 4] = [7; 4];
        // SAFETY: `dst` holds 4; `src` is NUL-terminated.
        unsafe {
            assert_eq!(wcslcpy(dst.as_mut_ptr(), src.as_ptr(), 4), 5, "cut short");
            assert_eq!(&dst, &w("hel")[..]);
            assert_eq!(
                wcslcpy(dst.as_mut_ptr(), src.as_ptr(), 0),
                5,
                "size 0: nothing written"
            );
            assert_eq!(&dst, &w("hel")[..]);
        }
        let mut buf: [WcharT; 8] = [0; 8];
        // SAFETY: `buf` holds 8.
        unsafe {
            assert_eq!(wcslcpy(buf.as_mut_ptr(), w("ab").as_ptr(), 8), 2);
            assert_eq!(wcslcat(buf.as_mut_ptr(), w("cdef").as_ptr(), 8), 6);
            assert_eq!(&buf[..7], &w("abcdef")[..]);
            assert_eq!(
                wcslcat(buf.as_mut_ptr(), w("ghij").as_ptr(), 8),
                10,
                "cut short"
            );
            assert_eq!(&buf, &w("abcdefg")[..]);
            // No NUL within size: nothing written, size + wcslen(src).
            let mut full: [WcharT; 3] = [b'x' as WcharT; 3];
            assert_eq!(wcslcat(full.as_mut_ptr(), w("yz").as_ptr(), 3), 5);
            assert_eq!(full, [b'x' as WcharT; 3]);
        }
    }

    /// The `_l` conversions and the BSD `q` names answer as the functions
    /// they stand for.
    #[test]
    fn the_locale_and_q_forms_are_their_functions() {
        let num: std::vec::Vec<WcharT> = "-0x7fz".chars().map(|c| c as WcharT).chain([0]).collect();
        let p = num.as_ptr();
        let mut end_a: *const WcharT = core::ptr::null();
        let mut end_b: *const WcharT = core::ptr::null();
        // SAFETY: a NUL-terminated wide string; the end pointers are locals.
        unsafe {
            assert_eq!(
                wcstol_l(p, &raw mut end_a, 16, 0),
                wcstol(p, &raw mut end_b, 16)
            );
            assert_eq!(end_a, end_b);
            assert_eq!(wcstoll_l(p, core::ptr::null_mut(), 0, 0), -0x7f);
            assert_eq!(wcstoq(p, core::ptr::null_mut(), 0), -0x7f);
            assert_eq!(
                wcstoul_l(p, core::ptr::null_mut(), 16, 0),
                wcstoul(p, core::ptr::null_mut(), 16)
            );
            assert_eq!(
                wcstoull_l(p, core::ptr::null_mut(), 16, 0),
                wcstouq(p, core::ptr::null_mut(), 16)
            );
        }
        let f: std::vec::Vec<WcharT> = "2.5e3".chars().map(|c| c as WcharT).chain([0]).collect();
        // SAFETY: as above.
        unsafe {
            assert_eq!(
                wcstod_l(f.as_ptr(), core::ptr::null_mut(), 0).to_bits(),
                2500.0f64.to_bits()
            );
            assert_eq!(
                wcstof_l(f.as_ptr(), core::ptr::null_mut(), 0).to_bits(),
                2500.0f32.to_bits()
            );
            assert_eq!(
                wcstold_l(f.as_ptr(), core::ptr::null_mut(), 0),
                wcstold(f.as_ptr(), core::ptr::null_mut())
            );
        }
    }

    /// `mbsnrtowcs` and `wcsnrtombs` against glibc 2.39's answers
    /// (`posix/tools/oracle/multibyte_harness.py`): a character cut in two by
    /// `nms`, carried to the next call; counting that leaves `*src` and the
    /// state alone and ignores `len`; `*src` at an invalid character.
    #[test]
    fn the_string_forms_are_glibcs() {
        let oracle = include_str!("multibyte_oracle.txt");
        let unhex = |h: &str| -> Vec<u8> {
            (0..h.len() / 2)
                .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
                .collect()
        };
        // Owns the bytes `base` and `src` point into.
        let mut input: Vec<u8>;
        let mut base: *const u8 = core::ptr::null();
        let mut src: *const u8 = core::ptr::null();
        let mut st = MbstateT::new();
        let mut calls = 0;
        for line in oracle.lines().filter(|l| l.starts_with("mbsnrtowcs ")) {
            let (head, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = head.split(' ').collect();
            if f[1] != "then" {
                input = unhex(f[1]);
                base = input.as_ptr();
                src = base;
                st = MbstateT::new();
            }
            let (nms, len): (usize, usize) = (f[2].parse().unwrap(), f[3].parse().unwrap());
            let dst = f[4] == "buf";
            let mut out = [0x5555 as WcharT; 16];
            crate::errno::set_errno(0);
            let r = unsafe {
                mbsnrtowcs(
                    if dst {
                        out.as_mut_ptr()
                    } else {
                        core::ptr::null_mut()
                    },
                    &raw mut src,
                    nms,
                    len,
                    &raw mut st,
                )
            };
            let mut got = format!("{}", r as isize);
            if r == usize::MAX {
                got += if crate::errno::get_errno() == crate::errno::EILSEQ {
                    "!EILSEQ"
                } else {
                    "!other"
                };
            }
            got += &if src.is_null() {
                " src=NULL ".to_string()
            } else {
                format!(" src+{} ", unsafe { src.offset_from(base) })
            };
            if dst && r != usize::MAX {
                for c in &out[..r] {
                    got += &format!("{c:04x}.");
                }
            }
            got += &format!("- {}", mbsinit(&raw const st));
            assert_eq!(got, want, "{line}");
            calls += 1;
        }
        assert_eq!(calls, 12);
        let wide: [WcharT; 4] = [0x61, 0xE9, 0x20AC, 0];
        let mut calls = 0;
        for line in oracle
            .lines()
            .filter(|l| l.starts_with("wcsnrtombs ") && !l.contains("surrogate"))
        {
            let (head, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = head.split(' ').collect();
            let (nwc, len): (usize, usize) = (f[2].parse().unwrap(), f[3].parse().unwrap());
            let dst = f[4] == "buf";
            let mut src: *const WcharT = wide.as_ptr();
            let mut st = MbstateT::new();
            let mut out = [0x55u8; 32];
            let r = unsafe {
                wcsnrtombs(
                    if dst {
                        out.as_mut_ptr()
                    } else {
                        core::ptr::null_mut()
                    },
                    &raw mut src,
                    nwc,
                    len,
                    &raw mut st,
                )
            };
            let mut got = format!("{}", r as isize);
            got += &if src.is_null() {
                " src=NULL ".to_string()
            } else {
                format!(" src+{} ", unsafe { src.offset_from(wide.as_ptr()) })
            };
            if dst && r != usize::MAX {
                for b in &out[..r] {
                    got += &format!("{b:02x}");
                }
            }
            got += &format!("- {}", mbsinit(&raw const st));
            assert_eq!(got, want, "{line}");
            calls += 1;
        }
        assert_eq!(calls, 5);
        let bad: [WcharT; 4] = [0x61, 0xD800, 0x62, 0];
        let mut src: *const WcharT = bad.as_ptr();
        let mut out = [0u8; 16];
        crate::errno::set_errno(0);
        let r = unsafe {
            wcsnrtombs(
                out.as_mut_ptr(),
                &raw mut src,
                10,
                10,
                core::ptr::null_mut(),
            )
        };
        let got = format!(
            "{}{} src+{}",
            r as isize,
            if crate::errno::get_errno() == crate::errno::EILSEQ {
                "!EILSEQ"
            } else {
                ""
            },
            unsafe { src.offset_from(bad.as_ptr()) }
        );
        let want = oracle
            .lines()
            .find_map(|l| l.strip_prefix("wcsnrtombs surrogate = "))
            .unwrap();
        assert_eq!(got, want);
    }

    /// The usual way to ask how long a conversion will be -- a NULL `dst`
    /// and a `len` of 0 -- answers the length: `len` limited it, and it was 0.
    #[test]
    fn counting_ignores_len() {
        let text = "a\u{e9}\u{20ac}\u{1f600}\0";
        let mut src = text.as_ptr();
        let n = unsafe {
            mbsrtowcs(
                core::ptr::null_mut(),
                &raw mut src,
                0,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(n, 4);
        assert_eq!(src, text.as_ptr(), "*src untouched");
        let wide: [WcharT; 5] = [0x61, 0xE9, 0x20AC, 0x1F600, 0];
        let mut wsrc = wide.as_ptr();
        let n = unsafe {
            wcsrtombs(
                core::ptr::null_mut(),
                &raw mut wsrc,
                0,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(n, 10);
        assert_eq!(wsrc, wide.as_ptr());
    }
}
