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
            pos = pos.wrapping_add(2);
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
    let mut rp = pos;
    let braced = rp < end && input.get(rp).copied().unwrap_or(0) == b'{';
    if braced {
        rp = rp.wrapping_add(1);
    }

    // Read variable name.
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

    rp.wrapping_sub(pos)
}
