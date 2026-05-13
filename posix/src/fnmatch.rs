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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            let exp_byte = expected.get(k).copied().unwrap_or(0);
            if unsafe { *pat.add(name_start.wrapping_add(k)) } != exp_byte {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: call `fnmatch` with byte slices (must be null-terminated).
    fn matches(pat: &[u8], s: &[u8], flags: i32) -> bool {
        let result = unsafe { fnmatch(pat.as_ptr(), s.as_ptr(), flags) };
        result == 0
    }

    // -----------------------------------------------------------------------
    // 1. Basic wildcard matching (* and ?)
    // -----------------------------------------------------------------------

    #[test]
    fn star_matches_empty() {
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn star_matches_any_string() {
        assert!(matches(b"*\0", b"hello\0", 0));
    }

    #[test]
    fn star_matches_middle() {
        assert!(matches(b"he*lo\0", b"hello\0", 0));
        assert!(matches(b"he*lo\0", b"hemiddlelo\0", 0));
    }

    #[test]
    fn star_matches_beginning() {
        assert!(matches(b"*ello\0", b"hello\0", 0));
    }

    #[test]
    fn star_matches_end() {
        assert!(matches(b"hell*\0", b"hello\0", 0));
    }

    #[test]
    fn star_no_match() {
        assert!(!matches(b"he*lx\0", b"hello\0", 0));
    }

    #[test]
    fn consecutive_stars() {
        assert!(matches(b"**\0", b"abc\0", 0));
        assert!(matches(b"a***b\0", b"aXYZb\0", 0));
    }

    #[test]
    fn question_mark_single_char() {
        assert!(matches(b"?\0", b"a\0", 0));
        assert!(matches(b"h?llo\0", b"hello\0", 0));
    }

    #[test]
    fn question_mark_no_match_empty() {
        assert!(!matches(b"?\0", b"\0", 0));
    }

    #[test]
    fn question_mark_no_match_multiple() {
        assert!(!matches(b"?\0", b"ab\0", 0));
    }

    #[test]
    fn multiple_question_marks() {
        assert!(matches(b"???\0", b"abc\0", 0));
        assert!(!matches(b"???\0", b"ab\0", 0));
        assert!(!matches(b"???\0", b"abcd\0", 0));
    }

    #[test]
    fn literal_match() {
        assert!(matches(b"hello\0", b"hello\0", 0));
        assert!(!matches(b"hello\0", b"world\0", 0));
    }

    #[test]
    fn literal_empty() {
        assert!(matches(b"\0", b"\0", 0));
        assert!(!matches(b"\0", b"a\0", 0));
        assert!(!matches(b"a\0", b"\0", 0));
    }

    #[test]
    fn star_and_question_combined() {
        assert!(matches(b"*?*\0", b"x\0", 0));
        assert!(!matches(b"*?*\0", b"\0", 0));
        assert!(matches(b"?*?\0", b"ab\0", 0));
        assert!(matches(b"?*?\0", b"abc\0", 0));
        assert!(!matches(b"?*?\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 2. Character classes [abc], [a-z], [!abc]
    // -----------------------------------------------------------------------

    #[test]
    fn bracket_single_chars() {
        assert!(matches(b"[abc]\0", b"a\0", 0));
        assert!(matches(b"[abc]\0", b"b\0", 0));
        assert!(matches(b"[abc]\0", b"c\0", 0));
        assert!(!matches(b"[abc]\0", b"d\0", 0));
    }

    #[test]
    fn bracket_range() {
        assert!(matches(b"[a-z]\0", b"m\0", 0));
        assert!(matches(b"[a-z]\0", b"a\0", 0));
        assert!(matches(b"[a-z]\0", b"z\0", 0));
        assert!(!matches(b"[a-z]\0", b"A\0", 0));
        assert!(!matches(b"[a-z]\0", b"0\0", 0));
    }

    #[test]
    fn bracket_range_digits() {
        assert!(matches(b"[0-9]\0", b"5\0", 0));
        assert!(!matches(b"[0-9]\0", b"a\0", 0));
    }

    #[test]
    fn bracket_negation_excl() {
        assert!(!matches(b"[!abc]\0", b"a\0", 0));
        assert!(matches(b"[!abc]\0", b"d\0", 0));
    }

    #[test]
    fn bracket_negation_caret() {
        assert!(!matches(b"[^abc]\0", b"b\0", 0));
        assert!(matches(b"[^abc]\0", b"x\0", 0));
    }

    #[test]
    fn bracket_negated_range() {
        assert!(!matches(b"[!a-z]\0", b"m\0", 0));
        assert!(matches(b"[!a-z]\0", b"5\0", 0));
    }

    #[test]
    fn bracket_literal_close_bracket_first() {
        // ']' as first char in bracket is literal per POSIX.
        assert!(matches(b"[]abc]\0", b"]\0", 0));
        assert!(matches(b"[]abc]\0", b"a\0", 0));
    }

    #[test]
    fn bracket_in_pattern() {
        assert!(matches(b"file[0-9].txt\0", b"file3.txt\0", 0));
        assert!(!matches(b"file[0-9].txt\0", b"filea.txt\0", 0));
    }

    #[test]
    fn bracket_unclosed() {
        // Unclosed bracket: no match.
        assert!(!matches(b"[abc\0", b"a\0", 0));
    }

    #[test]
    fn bracket_mixed_chars_and_ranges() {
        // 'x' or digits 0-9
        assert!(matches(b"[x0-9]\0", b"x\0", 0));
        assert!(matches(b"[x0-9]\0", b"5\0", 0));
        assert!(!matches(b"[x0-9]\0", b"y\0", 0));
    }

    // -----------------------------------------------------------------------
    // 3. POSIX character classes [:alpha:], [:digit:], etc.
    // -----------------------------------------------------------------------

    #[test]
    fn posix_class_alpha() {
        assert!(matches(b"[[:alpha:]]\0", b"a\0", 0));
        assert!(matches(b"[[:alpha:]]\0", b"Z\0", 0));
        assert!(!matches(b"[[:alpha:]]\0", b"5\0", 0));
    }

    #[test]
    fn posix_class_digit() {
        assert!(matches(b"[[:digit:]]\0", b"7\0", 0));
        assert!(!matches(b"[[:digit:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_alnum() {
        assert!(matches(b"[[:alnum:]]\0", b"a\0", 0));
        assert!(matches(b"[[:alnum:]]\0", b"9\0", 0));
        assert!(!matches(b"[[:alnum:]]\0", b"!\0", 0));
    }

    #[test]
    fn posix_class_upper() {
        assert!(matches(b"[[:upper:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:upper:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_lower() {
        assert!(matches(b"[[:lower:]]\0", b"a\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"A\0", 0));
    }

    #[test]
    fn posix_class_space() {
        assert!(matches(b"[[:space:]]\0", b" \0", 0));
        assert!(matches(b"[[:space:]]\0", b"\t\0", 0));
        assert!(!matches(b"[[:space:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_punct() {
        assert!(matches(b"[[:punct:]]\0", b"!\0", 0));
        assert!(matches(b"[[:punct:]]\0", b".\0", 0));
        assert!(!matches(b"[[:punct:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_xdigit() {
        assert!(matches(b"[[:xdigit:]]\0", b"a\0", 0));
        assert!(matches(b"[[:xdigit:]]\0", b"F\0", 0));
        assert!(matches(b"[[:xdigit:]]\0", b"9\0", 0));
        assert!(!matches(b"[[:xdigit:]]\0", b"g\0", 0));
    }

    #[test]
    fn posix_class_blank() {
        assert!(matches(b"[[:blank:]]\0", b" \0", 0));
        assert!(matches(b"[[:blank:]]\0", b"\t\0", 0));
        assert!(!matches(b"[[:blank:]]\0", b"\n\0", 0));
    }

    #[test]
    fn posix_class_print() {
        assert!(matches(b"[[:print:]]\0", b" \0", 0));
        assert!(matches(b"[[:print:]]\0", b"~\0", 0));
        assert!(!matches(b"[[:print:]]\0", b"\x01\0", 0));
    }

    #[test]
    fn posix_class_graph() {
        assert!(matches(b"[[:graph:]]\0", b"!\0", 0));
        assert!(!matches(b"[[:graph:]]\0", b" \0", 0));
        assert!(!matches(b"[[:graph:]]\0", b"\x01\0", 0));
    }

    #[test]
    fn posix_class_cntrl() {
        assert!(matches(b"[[:cntrl:]]\0", b"\x01\0", 0));
        assert!(matches(b"[[:cntrl:]]\0", b"\x7f\0", 0));
        assert!(!matches(b"[[:cntrl:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_unknown() {
        // Unknown class name should not match.
        assert!(!matches(b"[[:bogus:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_in_context() {
        assert!(matches(b"file[[:digit:]][[:digit:]].txt\0", b"file42.txt\0", 0));
        assert!(!matches(b"file[[:digit:]][[:digit:]].txt\0", b"fileAB.txt\0", 0));
    }

    #[test]
    fn posix_class_negated() {
        assert!(!matches(b"[![:digit:]]\0", b"5\0", 0));
        assert!(matches(b"[![:digit:]]\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 4. FNM_PATHNAME flag (wildcards don't match /)
    // -----------------------------------------------------------------------

    #[test]
    fn pathname_star_does_not_cross_slash() {
        assert!(matches(b"*\0", b"a/b\0", 0)); // Without flag, * matches /
        assert!(!matches(b"*\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_question_does_not_match_slash() {
        assert!(matches(b"a?b\0", b"a/b\0", 0)); // Without flag
        assert!(!matches(b"a?b\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_explicit_slash_matches() {
        assert!(matches(b"a/b\0", b"a/b\0", FNM_PATHNAME));
        assert!(matches(b"*/b\0", b"a/b\0", FNM_PATHNAME));
        assert!(matches(b"a/*\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_star_per_component() {
        assert!(matches(b"*/*\0", b"a/b\0", FNM_PATHNAME));
        assert!(!matches(b"*/*\0", b"a/b/c\0", FNM_PATHNAME));
        assert!(matches(b"*/*/*\0", b"a/b/c\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_bracket_does_not_match_slash() {
        assert!(!matches(b"a[/]b\0", b"a/b\0", FNM_PATHNAME));
    }

    // -----------------------------------------------------------------------
    // 5. FNM_PERIOD flag (leading . must be explicit)
    // -----------------------------------------------------------------------

    #[test]
    fn period_star_does_not_match_leading_dot() {
        assert!(matches(b"*\0", b".hidden\0", 0)); // Without flag
        assert!(!matches(b"*\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_question_does_not_match_leading_dot() {
        assert!(matches(b"?hidden\0", b".hidden\0", 0));
        assert!(!matches(b"?hidden\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_explicit_dot_matches() {
        assert!(matches(b".hidden\0", b".hidden\0", FNM_PERIOD));
        assert!(matches(b".*\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_not_leading_dot_ok() {
        // Dot that is not leading should still match *.
        assert!(matches(b"*\0", b"file.txt\0", FNM_PERIOD));
    }

    #[test]
    fn period_bracket_does_not_match_leading_dot() {
        assert!(!matches(b"[.]\0", b".\0", FNM_PERIOD));
    }

    #[test]
    fn period_with_pathname_after_slash() {
        // With both FNM_PATHNAME and FNM_PERIOD, a dot at the start of a
        // path component (after /) should require an explicit match.
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(!matches(b"dir/*\0", b"dir/.hidden\0", flags));
        assert!(matches(b"dir/.*\0", b"dir/.hidden\0", flags));
    }

    // -----------------------------------------------------------------------
    // 6. FNM_NOESCAPE flag
    // -----------------------------------------------------------------------

    #[test]
    fn noescape_backslash_literal() {
        // With FNM_NOESCAPE, backslash is treated as ordinary character.
        assert!(matches(b"\\\0", b"\\\0", FNM_NOESCAPE));
        assert!(!matches(b"\\\0", b"a\0", FNM_NOESCAPE));
    }

    #[test]
    fn noescape_backslash_in_bracket() {
        // With FNM_NOESCAPE, backslash in bracket is literal.
        assert!(matches(b"[\\\\a]\0", b"\\\0", FNM_NOESCAPE));
    }

    // -----------------------------------------------------------------------
    // 7. Backslash escaping (when FNM_NOESCAPE is NOT set)
    // -----------------------------------------------------------------------

    #[test]
    fn escape_star() {
        // \* matches literal *
        assert!(matches(b"\\*\0", b"*\0", 0));
        assert!(!matches(b"\\*\0", b"abc\0", 0));
    }

    #[test]
    fn escape_question() {
        assert!(matches(b"\\?\0", b"?\0", 0));
        assert!(!matches(b"\\?\0", b"a\0", 0));
    }

    #[test]
    fn escape_bracket() {
        assert!(matches(b"\\[\0", b"[\0", 0));
    }

    #[test]
    fn escape_backslash() {
        assert!(matches(b"\\\\\0", b"\\\0", 0));
    }

    #[test]
    fn escape_in_bracket_range() {
        // Escaped character as a range endpoint.
        assert!(matches(b"[\\a-\\c]\0", b"b\0", 0));
    }

    #[test]
    fn escape_trailing_backslash_no_match() {
        // Pattern ending with lone backslash (without FNM_NOESCAPE) cannot
        // match because the escaped char is NUL.
        assert!(!matches(b"a\\\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 8. Edge cases: empty strings, null pointers, adjacent wildcards
    // -----------------------------------------------------------------------

    #[test]
    fn null_pattern() {
        let result = unsafe { fnmatch(core::ptr::null(), b"test\0".as_ptr(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn null_string() {
        let result = unsafe { fnmatch(b"*\0".as_ptr(), core::ptr::null(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn both_null() {
        let result = unsafe { fnmatch(core::ptr::null(), core::ptr::null(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn empty_pattern_empty_string() {
        assert!(matches(b"\0", b"\0", 0));
    }

    #[test]
    fn empty_pattern_nonempty_string() {
        assert!(!matches(b"\0", b"a\0", 0));
    }

    #[test]
    fn nonempty_pattern_empty_string() {
        assert!(!matches(b"a\0", b"\0", 0));
    }

    #[test]
    fn star_only_empty_string() {
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn question_only_single_char() {
        assert!(matches(b"?\0", b"x\0", 0));
    }

    #[test]
    fn adjacent_stars() {
        assert!(matches(b"***\0", b"anything\0", 0));
        assert!(matches(b"***\0", b"\0", 0));
    }

    #[test]
    fn star_question_star() {
        // At least one character required (due to ?).
        assert!(matches(b"*?*\0", b"a\0", 0));
        assert!(!matches(b"*?*\0", b"\0", 0));
    }

    #[test]
    fn long_string_star_prefix() {
        // Stress: * at beginning with long string.
        assert!(matches(b"*end\0", b"a]very]long]string]with]end\0", 0));
        assert!(!matches(b"*end\0", b"a]very]long]string]with]enD\0", 0));
    }

    #[test]
    fn pattern_longer_than_string() {
        assert!(!matches(b"abcdef\0", b"abc\0", 0));
    }

    #[test]
    fn string_longer_than_pattern() {
        assert!(!matches(b"abc\0", b"abcdef\0", 0));
    }

    // -----------------------------------------------------------------------
    // 9. Complex patterns combining features
    // -----------------------------------------------------------------------

    #[test]
    fn complex_glob_file_matching() {
        assert!(matches(b"*.txt\0", b"readme.txt\0", 0));
        assert!(!matches(b"*.txt\0", b"readme.md\0", 0));
        assert!(matches(b"*.tar.gz\0", b"archive.tar.gz\0", 0));
    }

    #[test]
    fn complex_directory_pattern() {
        assert!(matches(
            b"src/*/test_*.rs\0",
            b"src/module/test_foo.rs\0",
            0,
        ));
    }

    #[test]
    fn complex_bracket_and_star() {
        assert!(matches(b"[a-z]*.log\0", b"server.log\0", 0));
        assert!(!matches(b"[a-z]*.log\0", b"Server.log\0", 0));
        assert!(!matches(b"[a-z]*.log\0", b"1server.log\0", 0));
    }

    #[test]
    fn complex_multiple_brackets() {
        assert!(matches(b"[abc][def][ghi]\0", b"adg\0", 0));
        assert!(matches(b"[abc][def][ghi]\0", b"cfi\0", 0));
        assert!(!matches(b"[abc][def][ghi]\0", b"aaa\0", 0));
    }

    #[test]
    fn complex_escaped_in_pattern() {
        // Match literal "[test].txt"
        assert!(matches(b"\\[test\\].txt\0", b"[test].txt\0", 0));
    }

    #[test]
    fn complex_pathname_period_combined() {
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(matches(b"src/*/*.rs\0", b"src/mod/lib.rs\0", flags));
        assert!(!matches(b"src/*/*.rs\0", b"src/.hidden/lib.rs\0", flags));
        assert!(matches(b"src/.*/*.rs\0", b"src/.hidden/lib.rs\0", flags));
    }

    #[test]
    fn complex_question_in_extension() {
        assert!(matches(b"file.???\0", b"file.txt\0", 0));
        assert!(matches(b"file.???\0", b"file.htm\0", 0));
        assert!(!matches(b"file.???\0", b"file.html\0", 0));
        assert!(!matches(b"file.???\0", b"file.rs\0", 0));
    }

    #[test]
    fn complex_posix_class_with_star() {
        assert!(matches(b"[[:upper:]]*\0", b"Hello\0", 0));
        assert!(!matches(b"[[:upper:]]*\0", b"hello\0", 0));
    }

    #[test]
    fn complex_all_flags() {
        let flags = FNM_PATHNAME | FNM_PERIOD | FNM_NOESCAPE;
        // Backslash is literal (NOESCAPE), star stops at / (PATHNAME),
        // leading dot must be explicit (PERIOD).
        assert!(matches(b"dir/*.c\0", b"dir/main.c\0", flags));
        assert!(!matches(b"dir/*.c\0", b"dir/.hidden.c\0", flags));
        assert!(!matches(b"*\0", b"a/b\0", flags));
        // Backslash is literal in pattern when NOESCAPE.
        assert!(matches(b"a\\b\0", b"a\\b\0", flags));
    }

    #[test]
    fn complex_nested_path_components() {
        let flags = FNM_PATHNAME;
        assert!(matches(b"a/*/c\0", b"a/b/c\0", flags));
        assert!(!matches(b"a/*/c\0", b"a/b/d/c\0", flags));
    }

    #[test]
    fn complex_star_at_path_boundary() {
        let flags = FNM_PATHNAME;
        assert!(matches(b"*/file\0", b"dir/file\0", flags));
        assert!(!matches(b"*/file\0", b"dir/sub/file\0", flags));
    }

    #[test]
    fn return_values() {
        // fnmatch returns 0 on match, FNM_NOMATCH (1) on no-match.
        let result = unsafe { fnmatch(b"abc\0".as_ptr(), b"abc\0".as_ptr(), 0) };
        assert_eq!(result, 0);
        let result = unsafe { fnmatch(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    // -------------------------------------------------------------------
    // Stress tests — fnmatch additional edge cases
    // -------------------------------------------------------------------

    #[test]
    fn stress_star_empty_string() {
        // "*" should match empty string.
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn stress_question_single_char() {
        assert!(matches(b"?\0", b"a\0", 0));
        assert!(!matches(b"?\0", b"\0", 0)); // ? requires exactly one char
        assert!(!matches(b"?\0", b"ab\0", 0)); // ? matches one, not two
    }

    #[test]
    fn stress_multiple_stars() {
        // "**" should still work like "*".
        assert!(matches(b"**\0", b"hello\0", 0));
        assert!(matches(b"**\0", b"\0", 0));
    }

    #[test]
    fn stress_star_question_combo() {
        // "*?" matches at least one character.
        assert!(matches(b"*?\0", b"x\0", 0));
        assert!(matches(b"*?\0", b"hello\0", 0));
        assert!(!matches(b"*?\0", b"\0", 0)); // needs at least one char
    }

    #[test]
    fn stress_bracket_range_digits() {
        assert!(matches(b"[0-9]\0", b"5\0", 0));
        assert!(!matches(b"[0-9]\0", b"a\0", 0));
        assert!(matches(b"[0-9][0-9]\0", b"42\0", 0));
        assert!(!matches(b"[0-9][0-9]\0", b"4a\0", 0));
    }

    #[test]
    fn stress_bracket_literal_dash() {
        // Dash at start of bracket expr is literal.
        assert!(matches(b"[-abc]\0", b"-\0", 0));
        assert!(matches(b"[-abc]\0", b"a\0", 0));
        assert!(!matches(b"[-abc]\0", b"x\0", 0));
    }

    #[test]
    fn stress_bracket_literal_close_bracket() {
        // ] at start of bracket expr is literal.
        assert!(matches(b"[]abc]\0", b"]\0", 0));
        assert!(matches(b"[]abc]\0", b"a\0", 0));
    }

    #[test]
    fn stress_negated_bracket_caret() {
        // [^...] is synonym for [!...]
        assert!(matches(b"[^abc]\0", b"x\0", 0));
        assert!(!matches(b"[^abc]\0", b"a\0", 0));
    }

    #[test]
    fn stress_pathname_star_no_slash() {
        // With FNM_PATHNAME, * doesn't match /.
        assert!(!matches(b"a*c\0", b"a/c\0", FNM_PATHNAME));
        assert!(matches(b"a*c\0", b"abc\0", FNM_PATHNAME));
    }

    #[test]
    fn stress_pathname_question_no_slash() {
        // With FNM_PATHNAME, ? doesn't match /.
        assert!(!matches(b"a?c\0", b"a/c\0", FNM_PATHNAME));
        assert!(matches(b"a?c\0", b"abc\0", FNM_PATHNAME));
    }

    #[test]
    fn stress_period_leading_dot() {
        // With FNM_PERIOD, leading . must be matched explicitly.
        assert!(!matches(b"*\0", b".hidden\0", FNM_PERIOD));
        assert!(matches(b".*\0", b".hidden\0", FNM_PERIOD));
        assert!(!matches(b"?\0", b".\0", FNM_PERIOD));
        assert!(matches(b".\0", b".\0", FNM_PERIOD));
    }

    #[test]
    fn stress_period_after_slash_pathname() {
        // With FNM_PATHNAME | FNM_PERIOD, dot after / must be explicit.
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(!matches(b"dir/*\0", b"dir/.hidden\0", flags));
        assert!(matches(b"dir/.*\0", b"dir/.hidden\0", flags));
    }

    #[test]
    fn stress_escape_special_chars() {
        // Without NOESCAPE, backslash escapes *, ?, [.
        assert!(matches(b"\\*\0", b"*\0", 0));
        assert!(!matches(b"\\*\0", b"abc\0", 0));
        assert!(matches(b"\\?\0", b"?\0", 0));
        assert!(!matches(b"\\?\0", b"a\0", 0));
    }

    #[test]
    fn stress_noescape_backslash_literal() {
        // With FNM_NOESCAPE, backslash is literal.
        assert!(matches(b"a\\b\0", b"a\\b\0", FNM_NOESCAPE));
        assert!(!matches(b"a\\b\0", b"ab\0", FNM_NOESCAPE));
    }

    #[test]
    fn stress_exact_long_pattern() {
        // Exact match of a long string.
        let pattern = b"abcdefghijklmnopqrstuvwxyz\0";
        let text = b"abcdefghijklmnopqrstuvwxyz\0";
        assert!(matches(pattern, text, 0));
    }

    #[test]
    fn stress_star_in_middle() {
        assert!(matches(b"hello*world\0", b"hello beautiful world\0", 0));
        assert!(matches(b"hello*world\0", b"helloworld\0", 0));
        assert!(!matches(b"hello*world\0", b"hello beautiful place\0", 0));
    }

    #[test]
    fn stress_multiple_bracket_expressions() {
        assert!(matches(b"[abc][def][ghi]\0", b"adg\0", 0));
        assert!(matches(b"[abc][def][ghi]\0", b"beh\0", 0));
        assert!(!matches(b"[abc][def][ghi]\0", b"aaa\0", 0));
    }

    #[test]
    fn stress_posix_class_lower() {
        assert!(matches(b"[[:lower:]]\0", b"a\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"1\0", 0));
    }

    #[test]
    fn stress_posix_class_upper() {
        assert!(matches(b"[[:upper:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:upper:]]\0", b"a\0", 0));
    }

    #[test]
    fn stress_posix_class_digit_in_combo() {
        // Mix POSIX class with range.
        assert!(matches(b"[[:digit:]a-f]\0", b"0\0", 0));
        assert!(matches(b"[[:digit:]a-f]\0", b"a\0", 0));
        assert!(matches(b"[[:digit:]a-f]\0", b"f\0", 0));
        assert!(!matches(b"[[:digit:]a-f]\0", b"g\0", 0));
    }

    #[test]
    fn stress_pattern_literal_only() {
        // Pattern with no wildcards is exact match.
        assert!(matches(b"hello\0", b"hello\0", 0));
        assert!(!matches(b"hello\0", b"Hello\0", 0));
        assert!(!matches(b"hello\0", b"hello world\0", 0));
    }
}
