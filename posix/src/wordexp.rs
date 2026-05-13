//! POSIX word expansion (`<wordexp.h>`).
//!
//! Provides `wordexp` and `wordfree` for shell-like word expansion
//! (tilde expansion, variable substitution, command substitution,
//! field splitting, pathname expansion).
//!
//! ## Limitations
//!
//! This is a minimal implementation:
//! - Tilde expansion (`~` → home directory) is not supported (returns
//!   the literal `~`).
//! - Variable expansion (`$VAR`) is supported for environment variables.
//! - Command substitution (`` `cmd` `` or `$(cmd)`) is not supported
//!   (returns `WRDE_CMDSUB` if `WRDE_NOCMD` is set, otherwise the
//!   literal text).
//! - Quote removal is handled (single quotes, double quotes, backslash).
//!
//! Programs that use `wordexp` for simple word splitting and variable
//! expansion will work.  Complex shell expansions will return literal text.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Append to previous result (not supported — ignored).
pub const WRDE_APPEND: i32 = 2;
/// Do not run commands (`` `cmd` `` and `$(cmd)` cause `WRDE_CMDSUB`).
pub const WRDE_NOCMD: i32 = 4;
/// Reuse a previous `wordexp_t` result.
pub const WRDE_REUSE: i32 = 8;
/// Report errors on stderr (ignored — we set errno instead).
pub const WRDE_SHOWERR: i32 = 16;
/// Treat undefined variables as errors.
pub const WRDE_UNDEF: i32 = 32;

// Error return codes (values match glibc/musl).
/// Out of memory.
pub const WRDE_NOSPACE: i32 = 1;
/// Illegal NUL byte in word.
pub const WRDE_BADCHAR: i32 = 2;
/// Undefined shell variable (with `WRDE_UNDEF`).
pub const WRDE_BADVAL: i32 = 3;
/// Command substitution requested with `WRDE_NOCMD`.
pub const WRDE_CMDSUB: i32 = 4;
/// Shell syntax error.
#[allow(dead_code)]
pub const WRDE_SYNTAX: i32 = 5;

// ---------------------------------------------------------------------------
// wordexp_t structure
// ---------------------------------------------------------------------------

/// Result of word expansion.
#[repr(C)]
pub struct WordexpT {
    /// Number of words in `we_wordv`.
    pub we_wordc: usize,
    /// Array of pointers to expanded words (null-terminated).
    pub we_wordv: *mut *mut u8,
    /// Number of NUL slots reserved at the start of `we_wordv`
    /// (for `WRDE_DOOFFS` — not supported, always 0).
    pub we_offs: usize,
}

/// Maximum word length (stack buffer for expansion).
const MAX_WORD_LEN: usize = 4096;

/// Maximum number of expanded words.
const MAX_WORDS: usize = 256;

/// Word boundary (start, end) pair.
struct WordBound {
    start: usize,
    end: usize,
}

// ---------------------------------------------------------------------------
// wordexp / wordfree
// ---------------------------------------------------------------------------

/// Perform word expansion on a string.
///
/// Splits `words` on whitespace, expands `$VAR` references from the
/// environment, and returns the result in `pwordexp`.
///
/// Returns 0 on success, or a `WRDE_*` error code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wordexp(
    words: *const u8,
    pwordexp: *mut WordexpT,
    flags: i32,
) -> i32 {
    if words.is_null() || pwordexp.is_null() {
        return WRDE_BADCHAR;
    }

    // Read input into a stack buffer.
    let input = read_input(words);

    // Split on whitespace.
    let (bounds, word_count) = split_words(&input.buf, input.len);

    if word_count == 0 {
        let we = unsafe { &mut *pwordexp };
        we.we_wordc = 0;
        we.we_wordv = core::ptr::null_mut();
        we.we_offs = 0;
        return 0;
    }

    // Allocate word pointer array.
    let array_size = word_count.wrapping_add(1).wrapping_mul(core::mem::size_of::<*mut u8>());
    // malloc returns memory aligned to at least 16 bytes, so casting to
    // *mut *mut u8 (align 8) is safe.
    #[allow(clippy::cast_ptr_alignment)]
    let array_ptr = crate::malloc::malloc(array_size).cast::<*mut u8>();
    if array_ptr.is_null() {
        return WRDE_NOSPACE;
    }

    // Expand each word.
    let result = expand_all_words(
        &input.buf, &bounds, word_count, flags, array_ptr,
    );

    match result {
        Ok(actual_count) => {
            // Null-terminate the array.
            // SAFETY: array_ptr has word_count+1 slots; actual_count <= word_count.
            unsafe { *array_ptr.add(actual_count) = core::ptr::null_mut(); }
            let we = unsafe { &mut *pwordexp };
            we.we_wordc = actual_count;
            we.we_wordv = array_ptr;
            we.we_offs = 0;
            0
        }
        Err(code) => code,
    }
}

/// Free the result of a `wordexp` call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn wordfree(pwordexp: *mut WordexpT) {
    if pwordexp.is_null() {
        return;
    }

    let we = unsafe { &mut *pwordexp };
    if !we.we_wordv.is_null() {
        for i in 0..we.we_wordc {
            // SAFETY: we_wordv has we_wordc valid pointers.
            let word = unsafe { *we.we_wordv.add(i) };
            if !word.is_null() {
                // SAFETY: word was allocated by malloc in wordexp.
                unsafe { crate::malloc::free(word.cast()); }
            }
        }
        // SAFETY: we_wordv was allocated by malloc in wordexp.
        unsafe { crate::malloc::free(we.we_wordv.cast()); }
    }

    we.we_wordc = 0;
    we.we_wordv = core::ptr::null_mut();
    we.we_offs = 0;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Input buffer read from a C string.
struct InputBuf {
    buf: [u8; MAX_WORD_LEN],
    len: usize,
}

/// Read a C string into a stack buffer.
fn read_input(words: *const u8) -> InputBuf {
    let mut result = InputBuf {
        buf: [0u8; MAX_WORD_LEN],
        len: 0,
    };
    loop {
        if result.len >= MAX_WORD_LEN {
            break;
        }
        // SAFETY: caller guarantees words is a valid C string.
        let byte = unsafe { *words.add(result.len) };
        if byte == 0 {
            break;
        }
        if let Some(slot) = result.buf.get_mut(result.len) {
            *slot = byte;
        }
        result.len = result.len.wrapping_add(1);
    }
    result
}

/// Split input on whitespace, returning word boundaries.
fn split_words(input: &[u8], input_len: usize) -> ([WordBound; MAX_WORDS], usize) {
    // Initialize with Default-like values.
    let mut bounds: [WordBound; MAX_WORDS] = unsafe { core::mem::zeroed() };
    let mut count: usize = 0;
    let mut pos: usize = 0;

    while pos < input_len && count < MAX_WORDS {
        // Skip whitespace.
        while pos < input_len {
            let c = input.get(pos).copied().unwrap_or(0);
            if c != b' ' && c != b'\t' && c != b'\n' {
                break;
            }
            pos = pos.wrapping_add(1);
        }
        if pos >= input_len {
            break;
        }

        let start = pos;
        pos = scan_word_end(input, pos, input_len);

        if let Some(b) = bounds.get_mut(count) {
            b.start = start;
            b.end = pos;
        }
        count = count.wrapping_add(1);
    }

    (bounds, count)
}

/// Scan forward from `pos` to find the end of a word, handling quotes.
fn scan_word_end(input: &[u8], mut pos: usize, input_len: usize) -> usize {
    while pos < input_len {
        let c = input.get(pos).copied().unwrap_or(0);
        if c == b' ' || c == b'\t' || c == b'\n' {
            break;
        }
        if c == b'\'' {
            pos = pos.wrapping_add(1);
            while pos < input_len && input.get(pos).copied().unwrap_or(0) != b'\'' {
                pos = pos.wrapping_add(1);
            }
            if pos < input_len {
                pos = pos.wrapping_add(1);
            }
        } else if c == b'"' {
            pos = pos.wrapping_add(1);
            while pos < input_len && input.get(pos).copied().unwrap_or(0) != b'"' {
                if input.get(pos).copied().unwrap_or(0) == b'\\' {
                    pos = pos.wrapping_add(1);
                }
                pos = pos.wrapping_add(1);
            }
            if pos < input_len {
                pos = pos.wrapping_add(1);
            }
        } else if c == b'\\' {
            // Skip backslash + the escaped character.  If the
            // backslash is the last character, clamp to input_len
            // so the returned end-position never exceeds the buffer.
            pos = pos.wrapping_add(2).min(input_len);
        } else {
            pos = pos.wrapping_add(1);
        }
    }
    pos
}

/// Expand all words, storing results in `array_ptr`.
///
/// Returns `Ok(actual_count)` or `Err(WRDE_* error code)`.
fn expand_all_words(
    input: &[u8],
    bounds: &[WordBound; MAX_WORDS],
    word_count: usize,
    flags: i32,
    array_ptr: *mut *mut u8,
) -> Result<usize, i32> {
    let mut actual_count: usize = 0;

    for wi in 0..word_count {
        let start = bounds.get(wi).map_or(0, |b| b.start);
        let end = bounds.get(wi).map_or(0, |b| b.end);

        let result = expand_single_word(input, start, end, flags);
        match result {
            Ok(exp) => {
                let word_ptr = alloc_word(&exp.buf, exp.len);
                if word_ptr.is_null() {
                    free_words(array_ptr, actual_count);
                    return Err(WRDE_NOSPACE);
                }
                // SAFETY: actual_count < word_count <= MAX_WORDS.
                unsafe { *array_ptr.add(actual_count) = word_ptr; }
                actual_count = actual_count.wrapping_add(1);
            }
            Err(code) => {
                free_words(array_ptr, actual_count);
                return Err(code);
            }
        }
    }

    Ok(actual_count)
}

/// Expanded word buffer.
struct ExpandedWord {
    buf: [u8; MAX_WORD_LEN],
    len: usize,
}

/// Expand a single word: strip quotes, expand `$VAR`, handle command
/// substitution.
fn expand_single_word(
    input: &[u8],
    start: usize,
    end: usize,
    flags: i32,
) -> Result<ExpandedWord, i32> {
    let mut exp = ExpandedWord {
        buf: [0u8; MAX_WORD_LEN],
        len: 0,
    };
    let mut rp = start;

    while rp < end {
        let c = input.get(rp).copied().unwrap_or(0);

        if c == b'\'' {
            rp = expand_single_quoted(input, rp, end, &mut exp);
        } else if c == b'"' {
            rp = expand_double_quoted(input, rp, end, &mut exp, flags)?;
        } else if c == b'\\' {
            rp = rp.wrapping_add(1);
            if rp < end {
                emit_byte(&mut exp, input.get(rp).copied().unwrap_or(0));
                rp = rp.wrapping_add(1);
            }
        } else if c == b'$' {
            rp = expand_dollar(input, rp, end, &mut exp, flags)?;
        } else if c == b'`' {
            rp = expand_backtick(input, rp, end, flags)?;
        } else {
            emit_byte(&mut exp, c);
            rp = rp.wrapping_add(1);
        }
    }

    Ok(exp)
}

/// Emit a byte into the expanded word buffer.
fn emit_byte(exp: &mut ExpandedWord, byte: u8) {
    if let Some(slot) = exp.buf.get_mut(exp.len) {
        *slot = byte;
    }
    exp.len = exp.len.wrapping_add(1);
}

/// Expand a single-quoted region: copy literally until closing `'`.
fn expand_single_quoted(
    input: &[u8],
    pos: usize,
    end: usize,
    exp: &mut ExpandedWord,
) -> usize {
    let mut rp = pos.wrapping_add(1); // Skip opening quote.
    while rp < end && input.get(rp).copied().unwrap_or(0) != b'\'' {
        emit_byte(exp, input.get(rp).copied().unwrap_or(0));
        rp = rp.wrapping_add(1);
    }
    if rp < end {
        rp = rp.wrapping_add(1); // Skip closing quote.
    }
    rp
}

/// Expand a double-quoted region: process `$VAR` and backslash escapes.
fn expand_double_quoted(
    input: &[u8],
    pos: usize,
    end: usize,
    exp: &mut ExpandedWord,
    flags: i32,
) -> Result<usize, i32> {
    let mut rp = pos.wrapping_add(1); // Skip opening quote.
    while rp < end && input.get(rp).copied().unwrap_or(0) != b'"' {
        let dc = input.get(rp).copied().unwrap_or(0);
        if dc == b'\\' && rp.wrapping_add(1) < end {
            rp = rp.wrapping_add(1);
            emit_byte(exp, input.get(rp).copied().unwrap_or(0));
            rp = rp.wrapping_add(1);
        } else if dc == b'$' {
            rp = rp.wrapping_add(1);
            let vlen = expand_var(input, rp, end, &mut exp.buf, &mut exp.len, flags);
            if vlen == usize::MAX {
                return Err(WRDE_BADVAL);
            }
            rp = rp.wrapping_add(vlen);
        } else {
            emit_byte(exp, dc);
            rp = rp.wrapping_add(1);
        }
    }
    if rp < end {
        rp = rp.wrapping_add(1); // Skip closing quote.
    }
    Ok(rp)
}

/// Expand a `$` reference (variable or command substitution).
fn expand_dollar(
    input: &[u8],
    pos: usize,
    end: usize,
    exp: &mut ExpandedWord,
    flags: i32,
) -> Result<usize, i32> {
    let mut rp = pos.wrapping_add(1); // Skip '$'.
    // Check for $(...) command substitution.
    if rp < end && input.get(rp).copied().unwrap_or(0) == b'(' {
        if flags & WRDE_NOCMD != 0 {
            return Err(WRDE_CMDSUB);
        }
        // Skip balanced parens — can't actually run commands.
        let mut depth: i32 = 1;
        rp = rp.wrapping_add(1);
        while rp < end && depth > 0 {
            let pc = input.get(rp).copied().unwrap_or(0);
            if pc == b'(' {
                depth = depth.wrapping_add(1);
            } else if pc == b')' {
                depth = depth.wrapping_sub(1);
            }
            rp = rp.wrapping_add(1);
        }
        Ok(rp)
    } else {
        let vlen = expand_var(input, rp, end, &mut exp.buf, &mut exp.len, flags);
        if vlen == usize::MAX {
            return Err(WRDE_BADVAL);
        }
        Ok(rp.wrapping_add(vlen))
    }
}

/// Expand a backtick command substitution.
fn expand_backtick(
    input: &[u8],
    pos: usize,
    end: usize,
    flags: i32,
) -> Result<usize, i32> {
    if flags & WRDE_NOCMD != 0 {
        return Err(WRDE_CMDSUB);
    }
    let mut rp = pos.wrapping_add(1);
    while rp < end && input.get(rp).copied().unwrap_or(0) != b'`' {
        rp = rp.wrapping_add(1);
    }
    if rp < end {
        rp = rp.wrapping_add(1);
    }
    Ok(rp)
}

/// Allocate and copy a word from a buffer.
fn alloc_word(buf: &[u8], len: usize) -> *mut u8 {
    let word_ptr = crate::malloc::malloc(len.wrapping_add(1));
    if word_ptr.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: word_ptr is non-null with len+1 bytes.
    let mut ci: usize = 0;
    while ci < len {
        let byte = buf.get(ci).copied().unwrap_or(0);
        unsafe { *word_ptr.add(ci) = byte; }
        ci = ci.wrapping_add(1);
    }
    unsafe { *word_ptr.add(len) = 0; } // NUL terminator.
    word_ptr
}

/// Free already-allocated words on error.
fn free_words(array_ptr: *mut *mut u8, count: usize) {
    for i in 0..count {
        // SAFETY: array_ptr has count valid entries.
        let word = unsafe { *array_ptr.add(i) };
        if !word.is_null() {
            unsafe { crate::malloc::free(word.cast()); }
        }
    }
    unsafe { crate::malloc::free(array_ptr.cast()); }
}

/// Extract a variable name from `input[pos..]`, returning
/// `(name_start, name_end, consumed)` where `consumed` is the number
/// of input bytes used (including braces if present).
///
/// This is the pure parsing logic factored out of `expand_var` so it
/// can be tested without calling `getenv`.
fn parse_var_name(input: &[u8], pos: usize, end: usize) -> (usize, usize, usize) {
    let mut rp = pos;
    let braced = rp < end && input.get(rp).copied().unwrap_or(0) == b'{';
    if braced {
        rp = rp.wrapping_add(1);
    }

    let name_start = rp;
    while rp < end {
        let c = input.get(rp).copied().unwrap_or(0);
        if braced {
            if c == b'}' {
                break;
            }
        } else if !c.is_ascii_alphanumeric() && c != b'_' {
            break;
        }
        rp = rp.wrapping_add(1);
    }
    let name_end = rp;

    if braced && rp < end {
        rp = rp.wrapping_add(1); // Skip closing '}'.
    }

    (name_start, name_end, rp.wrapping_sub(pos))
}

/// Expand a `$VAR` or `${VAR}` reference from `input[pos..]`.
///
/// Appends the variable's value to `expanded[*elen..]`.
/// Returns the number of input bytes consumed, or `usize::MAX` if
/// `WRDE_UNDEF` is set and the variable is undefined.
fn expand_var(
    input: &[u8],
    pos: usize,
    end: usize,
    expanded: &mut [u8],
    elen: &mut usize,
    flags: i32,
) -> usize {
    let (name_start, name_end, consumed) = parse_var_name(input, pos, end);

    let name_len = name_end.wrapping_sub(name_start);
    if name_len == 0 {
        // Bare '$' — emit literal.
        if let Some(slot) = expanded.get_mut(*elen) {
            *slot = b'$';
        }
        *elen = elen.wrapping_add(1);
        return 0;
    }

    // Build a NUL-terminated name on the stack.
    let mut name_buf = [0u8; 256];
    let copy_len = name_len.min(255);
    for i in 0..copy_len {
        if let Some(slot) = name_buf.get_mut(i) {
            *slot = input.get(name_start.wrapping_add(i)).copied().unwrap_or(0);
        }
    }

    // Look up the variable.
    // SAFETY: name_buf is a NUL-terminated stack-allocated string.
    let val = unsafe { crate::environ::getenv(name_buf.as_ptr()) };
    if val.is_null() {
        if flags & WRDE_UNDEF != 0 {
            return usize::MAX; // Error: undefined variable.
        }
        // Undefined variable → empty string.
    } else {
        // Append value to expanded.
        let mut vi: usize = 0;
        loop {
            // SAFETY: getenv returned a non-null C string.
            let c = unsafe { *val.add(vi) };
            if c == 0 {
                break;
            }
            if let Some(slot) = expanded.get_mut(*elen) {
                *slot = c;
            }
            *elen = elen.wrapping_add(1);
            vi = vi.wrapping_add(1);
        }
    }

    consumed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // Constants — verify values match glibc/musl
    // =======================================================================

    #[test]
    fn flag_constants_match_glibc() {
        assert_eq!(WRDE_APPEND, 2);
        assert_eq!(WRDE_NOCMD, 4);
        assert_eq!(WRDE_REUSE, 8);
        assert_eq!(WRDE_SHOWERR, 16);
        assert_eq!(WRDE_UNDEF, 32);
    }

    #[test]
    fn error_constants_match_glibc() {
        assert_eq!(WRDE_NOSPACE, 1);
        assert_eq!(WRDE_BADCHAR, 2);
        assert_eq!(WRDE_BADVAL, 3);
        assert_eq!(WRDE_CMDSUB, 4);
        assert_eq!(WRDE_SYNTAX, 5);
    }

    #[test]
    fn flags_are_distinct_powers_of_two() {
        // Each flag should be a distinct bit so they can be OR'd.
        let flags = [WRDE_APPEND, WRDE_NOCMD, WRDE_REUSE, WRDE_SHOWERR, WRDE_UNDEF];
        for (i, &a) in flags.iter().enumerate() {
            assert!(a.count_ones() == 1, "flag {a} is not a power of two");
            for &b in &flags[i + 1..] {
                assert_eq!(a & b, 0, "flags {a} and {b} overlap");
            }
        }
    }

    // =======================================================================
    // WordexpT layout
    // =======================================================================

    #[test]
    fn wordexp_t_size_and_alignment() {
        // On 64-bit, WordexpT should be 3 pointers/usizes = 24 bytes.
        assert_eq!(
            core::mem::size_of::<WordexpT>(),
            3 * core::mem::size_of::<usize>(),
        );
        assert_eq!(
            core::mem::align_of::<WordexpT>(),
            core::mem::align_of::<usize>(),
        );
    }

    #[test]
    fn wordexp_t_field_offsets() {
        // Verify the repr(C) layout: wordc at 0, wordv at 1*ptr, offs at 2*ptr.
        let ps = core::mem::size_of::<usize>();
        assert_eq!(core::mem::offset_of!(WordexpT, we_wordc), 0);
        assert_eq!(core::mem::offset_of!(WordexpT, we_wordv), ps);
        assert_eq!(core::mem::offset_of!(WordexpT, we_offs), 2 * ps);
    }

    // =======================================================================
    // Internal constants
    // =======================================================================

    #[test]
    fn max_word_len_is_reasonable() {
        assert!(MAX_WORD_LEN >= 1024, "MAX_WORD_LEN too small for practical use");
        assert!(MAX_WORD_LEN <= 65536, "MAX_WORD_LEN wastefully large for stack");
    }

    #[test]
    fn max_words_is_reasonable() {
        assert!(MAX_WORDS >= 64, "MAX_WORDS too small");
        assert!(MAX_WORDS <= 4096, "MAX_WORDS too large for stack arrays");
    }

    // =======================================================================
    // emit_byte
    // =======================================================================

    #[test]
    fn emit_byte_writes_and_advances() {
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        emit_byte(&mut exp, b'A');
        assert_eq!(exp.len, 1);
        assert_eq!(exp.buf[0], b'A');

        emit_byte(&mut exp, b'B');
        assert_eq!(exp.len, 2);
        assert_eq!(exp.buf[1], b'B');
    }

    #[test]
    fn emit_byte_advances_len_past_buffer_end() {
        // When the buffer is full, emit_byte should still increment len
        // (tracking the "would-have-been" length) but not write out of bounds.
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: MAX_WORD_LEN,
        };
        emit_byte(&mut exp, b'X');
        assert_eq!(exp.len, MAX_WORD_LEN + 1);
    }

    // =======================================================================
    // split_words — word splitting on whitespace
    // =======================================================================

    /// Helper: split a byte string and return a Vec of (start, end) pairs.
    fn split(input: &[u8]) -> Vec<(usize, usize)> {
        let mut buf = [0u8; MAX_WORD_LEN];
        let len = input.len().min(MAX_WORD_LEN);
        buf[..len].copy_from_slice(&input[..len]);
        let (bounds, count) = split_words(&buf, len);
        (0..count).map(|i| (bounds[i].start, bounds[i].end)).collect()
    }

    /// Helper: extract the word text from a split result.
    fn split_texts(input: &[u8]) -> Vec<Vec<u8>> {
        split(input)
            .iter()
            .map(|&(s, e)| input[s..e].to_vec())
            .collect()
    }

    #[test]
    fn split_empty_input() {
        assert!(split(b"").is_empty());
    }

    #[test]
    fn split_only_whitespace() {
        assert!(split(b"   \t  \n  ").is_empty());
    }

    #[test]
    fn split_single_word() {
        let words = split_texts(b"hello");
        assert_eq!(words, vec![b"hello".to_vec()]);
    }

    #[test]
    fn split_multiple_words() {
        let words = split_texts(b"hello world foo");
        assert_eq!(
            words,
            vec![b"hello".to_vec(), b"world".to_vec(), b"foo".to_vec()],
        );
    }

    #[test]
    fn split_leading_trailing_whitespace() {
        let words = split_texts(b"  hello  ");
        assert_eq!(words, vec![b"hello".to_vec()]);
    }

    #[test]
    fn split_tabs_and_newlines() {
        let words = split_texts(b"a\tb\nc");
        assert_eq!(
            words,
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()],
        );
    }

    #[test]
    fn split_preserves_quoted_spaces() {
        // Single-quoted word containing a space should be one word.
        let words = split_texts(b"'hello world'");
        assert_eq!(words, vec![b"'hello world'".to_vec()]);
    }

    #[test]
    fn split_preserves_double_quoted_spaces() {
        let words = split_texts(b"\"hello world\"");
        assert_eq!(words, vec![b"\"hello world\"".to_vec()]);
    }

    #[test]
    fn split_backslash_escapes_space() {
        let words = split_texts(b"hello\\ world");
        assert_eq!(words, vec![b"hello\\ world".to_vec()]);
    }

    #[test]
    fn split_mixed_quoted_and_unquoted() {
        let words = split_texts(b"a 'b c' d");
        assert_eq!(
            words,
            vec![b"a".to_vec(), b"'b c'".to_vec(), b"d".to_vec()],
        );
    }

    #[test]
    fn split_adjacent_quoted_regions() {
        // "hello"'world' should be a single word.
        let words = split_texts(b"\"hello\"'world'");
        assert_eq!(words, vec![b"\"hello\"'world'".to_vec()]);
    }

    // =======================================================================
    // scan_word_end — quote-aware word boundary scanning
    // =======================================================================

    #[test]
    fn scan_plain_word() {
        let input = b"hello world";
        assert_eq!(scan_word_end(input, 0, input.len()), 5);
    }

    #[test]
    fn scan_from_start_of_second_word() {
        let input = b"hello world";
        assert_eq!(scan_word_end(input, 6, input.len()), 11);
    }

    #[test]
    fn scan_single_quoted_word() {
        let input = b"'hello world' next";
        assert_eq!(scan_word_end(input, 0, input.len()), 13);
    }

    #[test]
    fn scan_double_quoted_word() {
        let input = b"\"hello world\" next";
        assert_eq!(scan_word_end(input, 0, input.len()), 13);
    }

    #[test]
    fn scan_backslash_escape_in_word() {
        let input = b"hello\\ world next";
        assert_eq!(scan_word_end(input, 0, input.len()), 12);
    }

    #[test]
    fn scan_double_quote_with_backslash_inside() {
        // "he\"llo" should be one word (escaped quote inside double quotes).
        let input = b"\"he\\\"llo\" next";
        assert_eq!(scan_word_end(input, 0, input.len()), 9);
    }

    #[test]
    fn scan_unclosed_single_quote() {
        // Unclosed quote: scans to end of input.
        let input = b"'hello";
        assert_eq!(scan_word_end(input, 0, input.len()), 6);
    }

    #[test]
    fn scan_unclosed_double_quote() {
        let input = b"\"hello";
        assert_eq!(scan_word_end(input, 0, input.len()), 6);
    }

    #[test]
    fn scan_empty_quotes() {
        let input = b"'' next";
        assert_eq!(scan_word_end(input, 0, input.len()), 2);
    }

    // =======================================================================
    // expand_single_quoted — single-quote removal
    // =======================================================================

    #[test]
    fn single_quoted_basic() {
        let input = b"'hello' rest";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        let end_pos = expand_single_quoted(input, 0, 7, &mut exp);
        assert_eq!(end_pos, 7); // past closing quote
        assert_eq!(&exp.buf[..exp.len], b"hello");
    }

    #[test]
    fn single_quoted_preserves_special_chars() {
        // Single quotes preserve everything literally, including $ and \.
        let input = b"'$HOME\\n'";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        let end_pos = expand_single_quoted(input, 0, input.len(), &mut exp);
        assert_eq!(end_pos, 9);
        assert_eq!(&exp.buf[..exp.len], b"$HOME\\n");
    }

    #[test]
    fn single_quoted_empty() {
        let input = b"''";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        let end_pos = expand_single_quoted(input, 0, 2, &mut exp);
        assert_eq!(end_pos, 2);
        assert_eq!(exp.len, 0);
    }

    #[test]
    fn single_quoted_unclosed() {
        // Unclosed single quote: copies to end of range.
        let input = b"'hello";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        let end_pos = expand_single_quoted(input, 0, input.len(), &mut exp);
        assert_eq!(end_pos, 6); // scanned to end
        assert_eq!(&exp.buf[..exp.len], b"hello");
    }

    #[test]
    fn single_quoted_with_spaces() {
        let input = b"'hello world'";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        let end_pos = expand_single_quoted(input, 0, input.len(), &mut exp);
        assert_eq!(end_pos, 13);
        assert_eq!(&exp.buf[..exp.len], b"hello world");
    }

    // =======================================================================
    // expand_backtick — backtick command substitution detection
    // =======================================================================

    #[test]
    fn backtick_with_nocmd_returns_cmdsub() {
        let input = b"`ls`";
        let result = expand_backtick(input, 0, input.len(), WRDE_NOCMD);
        assert_eq!(result, Err(WRDE_CMDSUB));
    }

    #[test]
    fn backtick_without_nocmd_skips_content() {
        let input = b"`ls -la` rest";
        let result = expand_backtick(input, 0, 8, 0);
        assert_eq!(result, Ok(8)); // past closing backtick
    }

    #[test]
    fn backtick_unclosed_scans_to_end() {
        let input = b"`ls -la";
        let result = expand_backtick(input, 0, input.len(), 0);
        assert_eq!(result, Ok(7));
    }

    // =======================================================================
    // expand_dollar — command substitution branch ($(...))
    // =======================================================================
    //
    // Only the $(...) branch is testable without getenv. The $VAR branch
    // calls expand_var which calls getenv.

    #[test]
    fn dollar_paren_with_nocmd_returns_cmdsub() {
        let input = b"$(echo hi)";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        // expand_dollar starts after reading '$', so the '$' is at pos 0.
        let result = expand_dollar(input, 0, input.len(), &mut exp, WRDE_NOCMD);
        assert_eq!(result, Err(WRDE_CMDSUB));
    }

    #[test]
    fn dollar_paren_without_nocmd_skips_balanced() {
        let input = b"$(echo $(nested))rest";
        let mut exp = ExpandedWord {
            buf: [0u8; MAX_WORD_LEN],
            len: 0,
        };
        // expand_dollar is called with pos pointing at '$', so the '(' is at pos+1.
        let result = expand_dollar(input, 0, input.len(), &mut exp, 0);
        // Should skip past the balanced $(echo $(nested)) which ends at index 17.
        assert_eq!(result, Ok(17));
    }

    // =======================================================================
    // parse_var_name — variable name extraction
    // =======================================================================

    #[test]
    fn parse_var_name_simple() {
        let input = b"HOME/bin";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"HOME");
        assert_eq!(consumed, 4);
    }

    #[test]
    fn parse_var_name_with_underscore() {
        let input = b"MY_VAR=rest";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"MY_VAR");
        assert_eq!(consumed, 6);
    }

    #[test]
    fn parse_var_name_braced() {
        let input = b"{HOME}/bin";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"HOME");
        // consumed includes the braces: { + HOME + } = 6
        assert_eq!(consumed, 6);
    }

    #[test]
    fn parse_var_name_braced_special_chars() {
        // Braced names can contain characters that unbraced cannot.
        let input = b"{FOO.BAR}rest";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"FOO.BAR");
        assert_eq!(consumed, 9);
    }

    #[test]
    fn parse_var_name_empty() {
        // Bare '$' with no valid name chars following.
        let input = b" rest";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(start, end); // empty name
        assert_eq!(consumed, 0);
    }

    #[test]
    fn parse_var_name_digits_and_letters() {
        let input = b"VAR123_x!";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"VAR123_x");
        assert_eq!(consumed, 8);
    }

    #[test]
    fn parse_var_name_at_offset() {
        // The variable name starts at a non-zero offset.
        let input = b"xx HOME rest";
        let (start, end, consumed) = parse_var_name(input, 3, input.len());
        assert_eq!(&input[start..end], b"HOME");
        assert_eq!(consumed, 4);
    }

    #[test]
    fn parse_var_name_empty_braces() {
        let input = b"{}rest";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(start, end); // empty name
        assert_eq!(consumed, 2); // consumed both braces
    }

    #[test]
    fn parse_var_name_unclosed_brace() {
        // Unclosed brace: scans to end of range.
        let input = b"{HOME";
        let (start, end, consumed) = parse_var_name(input, 0, input.len());
        assert_eq!(&input[start..end], b"HOME");
        // No closing brace found, so consumed = end - pos = 5.
        assert_eq!(consumed, 5);
    }

    // =======================================================================
    // Interaction tests: split_words + scan_word_end consistency
    // =======================================================================

    #[test]
    fn split_and_scan_agree_on_boundaries() {
        let input = b"alpha 'beta gamma' delta";
        let pairs = split(input);
        // Should find 3 words: alpha, 'beta gamma', delta
        assert_eq!(pairs.len(), 3);
        assert_eq!(&input[pairs[0].0..pairs[0].1], b"alpha");
        assert_eq!(&input[pairs[1].0..pairs[1].1], b"'beta gamma'");
        assert_eq!(&input[pairs[2].0..pairs[2].1], b"delta");
    }

    #[test]
    fn split_handles_many_words() {
        // Build a string with exactly MAX_WORDS words.
        let mut input = Vec::new();
        for i in 0..MAX_WORDS {
            if i > 0 {
                input.push(b' ');
            }
            input.push(b'w');
        }
        let pairs = split(&input);
        assert_eq!(pairs.len(), MAX_WORDS);
    }

    #[test]
    fn split_caps_at_max_words() {
        // Build a string with MAX_WORDS + 10 words.
        let mut input = Vec::new();
        for i in 0..MAX_WORDS + 10 {
            if i > 0 {
                input.push(b' ');
            }
            input.push(b'w');
        }
        let pairs = split(&input);
        // Should be capped at MAX_WORDS.
        assert_eq!(pairs.len(), MAX_WORDS);
    }

    #[test]
    fn split_backslash_at_end_of_input() {
        // Trailing backslash with no following character.
        let input = b"hello\\";
        let words = split_texts(input);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0], b"hello\\");
    }

    #[test]
    fn split_multiple_quote_styles_in_one_word() {
        // A word that combines single and double quotes: he'l'"l"o
        let input = b"he'l'\"l\"o next";
        let words = split_texts(input);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0], b"he'l'\"l\"o");
    }
}
