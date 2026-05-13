//! POSIX filename pattern matching.
//!
//! Implements `fnmatch()` for shell-style wildcard matching per
//! POSIX.1-2024.
//!
//! ## Supported Patterns
//!
//! - `*` — matches any string (including empty)
//! - `?` — matches any single character
//! - `[...]` — matches any character in the set
//! - `[!...]` or `[^...]` — matches any character NOT in the set
//! - `\c` — matches literal character `c` (when `FNM_NOESCAPE` not set)
//!
//! ## Flags
//!
//! - `FNM_PATHNAME` (1): `*` and `?` don't match `/`
//! - `FNM_NOESCAPE` (2): treat `\` as ordinary character
//! - `FNM_PERIOD` (4): leading `.` must be matched explicitly

/// Returned when the pattern does not match.
pub const FNM_NOMATCH: i32 = 1;
/// Wildcards don't match `/` (glibc/musl value: 1).
pub const FNM_PATHNAME: i32 = 1;
/// Treat backslash as ordinary character (glibc/musl value: 2).
pub const FNM_NOESCAPE: i32 = 2;
/// Leading `.` must be matched explicitly.
pub const FNM_PERIOD: i32 = 4;

/// Match a filename against a pattern.
///
/// Returns 0 if `string` matches `pattern`, `FNM_NOMATCH` otherwise.
///
/// # Safety
///
/// Both `pattern` and `string` must be valid null-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fnmatch(
    pattern: *const u8,
    string: *const u8,
    flags: i32,
) -> i32 {
    if pattern.is_null() || string.is_null() {
        return FNM_NOMATCH;
    }

    if do_match(pattern, 0, string, 0, flags, true) {
        0
    } else {
        FNM_NOMATCH
    }
}

/// Match a bracket expression `[...]` against a character.
///
/// `ppos` should point to the character after `[`.
/// Returns `Some(new_ppos)` (pointing past `]`) on match, `None` on
/// no-match or malformed bracket.
fn match_bracket(
    pat: *const u8,
    mut ppos: usize,
    sc: u8,
    flags: i32,
) -> Option<usize> {
    let negate = unsafe { *pat.add(ppos) } == b'!' || unsafe { *pat.add(ppos) } == b'^';
    if negate {
        ppos = ppos.wrapping_add(1);
    }

    let mut matched = false;
    let mut first = true;

    loop {
        let ch = unsafe { *pat.add(ppos) };
        if ch == 0 {
            return None; // Unclosed bracket.
        }
        if ch == b']' && !first {
            break;
        }
        first = false;

        // Check for POSIX character class [:classname:].
        if ch == b'[' && unsafe { *pat.add(ppos.wrapping_add(1)) } == b':' {
            let name_start = ppos.wrapping_add(2);
            let mut end = name_start;
            while unsafe { *pat.add(end) } != 0 {
                if unsafe { *pat.add(end) } == b':'
                    && unsafe { *pat.add(end.wrapping_add(1)) } == b']'
                {
                    break;
                }
                end = end.wrapping_add(1);
            }
            if unsafe { *pat.add(end) } == b':'
                && unsafe { *pat.add(end.wrapping_add(1)) } == b']'
            {
                if posix_class_matches(pat, name_start, end.wrapping_sub(name_start), sc) {
                    matched = true;
                }
                ppos = end.wrapping_add(2); // Skip past ":]"
                continue;
            }
            // Not a valid class — treat '[' as literal and fall through.
        }

        let mut low = ch;
        ppos = ppos.wrapping_add(1);

        // Handle escape.
        if low == b'\\' && flags & FNM_NOESCAPE == 0 {
            low = unsafe { *pat.add(ppos) };
            if low == 0 {
                return None;
            }
            ppos = ppos.wrapping_add(1);
        }

        // Check for range: a-z.
        if unsafe { *pat.add(ppos) } == b'-'
            && unsafe { *pat.add(ppos.wrapping_add(1)) } != b']'
            && unsafe { *pat.add(ppos.wrapping_add(1)) } != 0
        {
            ppos = ppos.wrapping_add(1); // skip '-'
            let mut high = unsafe { *pat.add(ppos) };
            if high == b'\\' && flags & FNM_NOESCAPE == 0 {
                ppos = ppos.wrapping_add(1);
                high = unsafe { *pat.add(ppos) };
                if high == 0 {
                    return None;
                }
            }
            ppos = ppos.wrapping_add(1);

            if sc >= low && sc <= high {
                matched = true;
            }
        } else if sc == low {
            matched = true;
        }
    }

    ppos = ppos.wrapping_add(1); // skip ']'

    if matched == negate {
        None
    } else {
        Some(ppos)
    }
}

/// Handle `*` wildcard matching.
///
/// `ppos` points past all consecutive `*` characters.
/// Returns true if the rest of the pattern matches.
fn match_star(
    pat: *const u8,
    ppos: usize,
    str: *const u8,
    spos: usize,
    flags: i32,
) -> bool {
    // If pattern is exhausted after *, match rest of string.
    if unsafe { *pat.add(ppos) } == 0 {
        // If FNM_PATHNAME, rest must not contain '/'.
        if flags & FNM_PATHNAME != 0 {
            let mut check = spos;
            while unsafe { *str.add(check) } != 0 {
                if unsafe { *str.add(check) } == b'/' {
                    return false;
                }
                check = check.wrapping_add(1);
            }
        }
        return true;
    }

    // Try matching * against 0, 1, 2, ... characters.
    let mut try_pos = spos;
    while unsafe { *str.add(try_pos) } != 0 {
        if flags & FNM_PATHNAME != 0 && unsafe { *str.add(try_pos) } == b'/' {
            break; // * stops at /.
        }
        if do_match(pat, ppos, str, try_pos, flags, false) {
            return true;
        }
        try_pos = try_pos.wrapping_add(1);
    }
    // Also try matching with * consuming everything up to here.
    do_match(pat, ppos, str, try_pos, flags, false)
}

/// Recursive pattern matching engine.
///
/// `at_start` indicates whether `spos` is at the start of the string
/// (or the start of a path component, for `FNM_PATHNAME` + `FNM_PERIOD`).
fn do_match(
    pat: *const u8,
    mut ppos: usize,
    str: *const u8,
    mut spos: usize,
    flags: i32,
    mut at_start: bool,
) -> bool {
    loop {
        let pc = unsafe { *pat.add(ppos) };
        let sc = unsafe { *str.add(spos) };

        match pc {
            0 => return sc == 0,

            b'?' => {
                if sc == 0
                    || (flags & FNM_PATHNAME != 0 && sc == b'/')
                    || (flags & FNM_PERIOD != 0 && sc == b'.' && at_start)
                {
                    return false;
                }
                ppos = ppos.wrapping_add(1);
                spos = spos.wrapping_add(1);
                // Matched a non-'/' char; no longer at start of component.
                at_start = false;
            }

            b'*' => {
                // Skip consecutive stars.
                while unsafe { *pat.add(ppos) } == b'*' {
                    ppos = ppos.wrapping_add(1);
                }
                if flags & FNM_PERIOD != 0 && sc == b'.' && at_start {
                    return false;
                }
                return match_star(pat, ppos, str, spos, flags);
            }

            b'[' => {
                if sc == 0
                    || (flags & FNM_PERIOD != 0 && sc == b'.' && at_start)
                    || (flags & FNM_PATHNAME != 0 && sc == b'/')
                {
                    return false;
                }
                let Some(new_ppos) = match_bracket(pat, ppos.wrapping_add(1), sc, flags) else {
                    return false;
                };
                ppos = new_ppos;
                spos = spos.wrapping_add(1);
                at_start = false;
            }

            b'\\' if flags & FNM_NOESCAPE == 0 => {
                ppos = ppos.wrapping_add(1);
                let escaped = unsafe { *pat.add(ppos) };
                if escaped == 0 || sc != escaped {
                    return false;
                }
                ppos = ppos.wrapping_add(1);
                spos = spos.wrapping_add(1);
                // After '\/' the next char is at component start.
                at_start = sc == b'/';
            }

            _ => {
                if pc != sc {
                    return false;
                }
                ppos = ppos.wrapping_add(1);
                spos = spos.wrapping_add(1);
                // After passing a '/', next char is "at start" of component.
                // Otherwise we're no longer at start.
                at_start = pc == b'/';
            }
        }
    }
}

// ---------------------------------------------------------------------------
// POSIX character class matching for bracket expressions
// ---------------------------------------------------------------------------

/// Check if character `c` matches a POSIX character class by name.
///
/// `name_start` is the offset into `pat` where the class name begins,
/// `name_len` is the length of the name (between `[:` and `:]`).
fn posix_class_matches(pat: *const u8, name_start: usize, name_len: usize, c: u8) -> bool {
    let name_eq = |expected: &[u8]| -> bool {
        if name_len != expected.len() { return false; }
        let mut k = 0;
        while k < name_len {
            if unsafe { *pat.add(name_start.wrapping_add(k)) } != expected[k] {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    };

    if name_eq(b"alpha") {
        c.is_ascii_alphabetic()
    } else if name_eq(b"digit") {
        c.is_ascii_digit()
    } else if name_eq(b"alnum") {
        c.is_ascii_alphanumeric()
    } else if name_eq(b"space") {
        c.is_ascii_whitespace()
    } else if name_eq(b"upper") {
        c.is_ascii_uppercase()
    } else if name_eq(b"lower") {
        c.is_ascii_lowercase()
    } else if name_eq(b"punct") {
        c.is_ascii_punctuation()
    } else if name_eq(b"cntrl") {
        c.is_ascii_control()
    } else if name_eq(b"print") {
        (0x20..=0x7E).contains(&c)
    } else if name_eq(b"graph") {
        (0x21..=0x7E).contains(&c)
    } else if name_eq(b"xdigit") {
        c.is_ascii_hexdigit()
    } else if name_eq(b"blank") {
        c == b' ' || c == b'\t'
    } else {
        false // Unknown class — no match.
    }
}
