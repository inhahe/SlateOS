//! RFC 5424 structured data, as util-linux 2.39.3's `logger` validates and
//! renders it (`misc-utils/logger.c`, the `*_structured_data*` functions).

/// One SD-ELEMENT: `[ID PARAM PARAM ...]`. Upstream's `struct structured_data`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub id: Vec<u8>,
    pub params: Vec<Vec<u8>>,
}

/// `ul_strchr_escaped`: the first `c` in `s` not preceded by an unescaped
/// backslash. Returns its index.
///
/// Upstream (lib/strutils.c):
///
/// ```c
/// char *ul_strchr_escaped(const char *s, int c)
/// {
///     char *p;
///     int esc = 0;
///
///     for (p = (char *) s; p && *p; p++) {
///         if (!esc && *p == '\\') {
///             esc = 1;
///             continue;
///         }
///         if (*p == c && (!esc || c == '\\'))
///             return p;
///         esc = 0;
///     }
///     return NULL;
/// }
/// ```
#[must_use]
pub fn strchr_escaped(s: &[u8], c: u8) -> Option<usize> {
    let mut esc = false;
    for (i, &b) in s.iter().enumerate() {
        if b == 0 {
            break;
        }
        if !esc && b == b'\\' {
            esc = true;
            continue;
        }
        if b == c && (!esc || c == b'\\') {
            return Some(i);
        }
        esc = false;
    }
    None
}

/// The absolute index of the first `b` in `s` at or after `at`.
fn find_from(s: &[u8], at: usize, b: u8) -> Option<usize> {
    let rel = s.get(at..)?.iter().position(|&x| x == b)?;
    at.checked_add(rel)
}

/// [`strchr_escaped`] from `at`, as an absolute index.
fn escaped_from(s: &[u8], at: usize, c: u8) -> Option<usize> {
    let rel = strchr_escaped(s.get(at..)?, c)?;
    at.checked_add(rel)
}

/// `valid_structured_data_param`: `name="value"`, with `]` escaped inside
/// the value and `\` allowed only before one of `[]"\`.
///
/// Ported branch for branch, including the parts that read oddly: the `]`
/// scan compares against `ul_strchr_escaped(s, ']')` from each position, the
/// backslash scan skips a doubled `\\` as a unit, and the name's own bytes are
/// never checked -- `a ="v"` passes, because only the `=` right before the
/// quote is.
///
/// Index steps saturate: every index is below `s.len()`, so they are exact.
#[must_use]
pub fn valid_param(s: &[u8]) -> bool {
    let Some(eq) = find_from(s, 0, b'=') else {
        return false;
    };
    let Some(qm1) = find_from(s, 0, b'"') else {
        return false;
    };
    let after_qm1 = qm1.saturating_add(1);
    let Some(qm2) = escaped_from(s, after_qm1, b'"') else {
        return false;
    };

    // ']' needs to be escaped.
    let mut at = after_qm1;
    while let Some(p) = find_from(s, at, b']') {
        if p > qm2 || Some(p) == escaped_from(s, at, b']') {
            return false;
        }
        at = p.saturating_add(1);
    }

    // '\' is allowed only before '[]"\' chars.
    let mut at = after_qm1;
    while let Some(p) = find_from(s, at, b'\\') {
        // `strchr("[]\"\\", *(p + 1))` is true for the NUL terminator too:
        // strchr finds the string's own terminator. So a trailing backslash
        // passes this test.
        let next = s.get(p.saturating_add(1)).copied().unwrap_or(0);
        if !(next == 0 || b"[]\"\\".contains(&next)) {
            return false;
        }
        at = p.saturating_add(1);
        if s.get(at) == Some(&b'\\') {
            at = at.saturating_add(1);
        }
    }

    // foo="bar"
    eq > 0
        && eq < qm1
        && eq.saturating_add(1) == qm1
        && qm1 < qm2
        && s.get(qm2.saturating_add(1)).is_none()
}

/// `valid_structured_data_id`: one of the three IANA IDs, or
/// `name@<digits>[.<digits>...]` with no `[`, `=`, `"`, `@`, blank or
/// control byte in the name.
#[must_use]
pub fn valid_id(s: &[u8]) -> bool {
    let Some(at) = find_from(s, 0, b'@') else {
        return matches!(s, b"timeQuality" | b"origin" | b"meta");
    };
    let digits_from = at.saturating_add(1);
    if at == 0 || s.get(digits_from).is_none() {
        return false;
    }
    // <digits> or <digits>.<digits>[...]: `isdigit_strend` from each point.
    let mut p = digits_from;
    while let Some(rest) = s.get(p..).filter(|r| !r.is_empty()) {
        let run = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if run == rest.len() {
            break; // only digits to the end
        }
        // What follows the digits must be a `.` with something after it, and
        // there must have been digits.
        let end = p.saturating_add(run);
        if run == 0 || s.get(end) != Some(&b'.') || s.get(end.saturating_add(1)).is_none() {
            return false;
        }
        p = end.saturating_add(1);
    }
    // Forbidden bytes in the name.
    s.get(..at).is_some_and(|name| {
        name.iter().all(|&b| {
            !(b == b'['
                || b == b'='
                || b == b'"'
                || b == b'@'
                || b == b' '
                || b == b'\t'
                || b.is_ascii_control())
        })
    })
}

/// `strdup_structured_data_list` over the reserved then the user elements:
/// each non-empty element as `[ID P1 P2]`, concatenated; `None` when there is
/// nothing to print (the header then carries the NILVALUE).
#[must_use]
pub fn render(reserved: &[Element], user: &[Element]) -> Option<Vec<u8>> {
    let mut out: Option<Vec<u8>> = None;
    for e in reserved.iter().chain(user) {
        if e.params.is_empty() {
            continue;
        }
        let buf = out.get_or_insert_with(Vec::new);
        buf.push(b'[');
        buf.extend_from_slice(&e.id);
        buf.push(b' ');
        buf.extend_from_slice(&e.params.join(&b' '));
        buf.push(b']');
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::needless_raw_string_hashes)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_iana_names_or_name_at_enterprise_number() {
        for ok in [
            &b"timeQuality"[..],
            b"origin",
            b"meta",
            b"zoo@123",
            b"x@1.2.3",
            b"ourSDID@32473",
        ] {
            assert!(valid_id(ok), "{}", ok.escape_ascii());
        }
        for bad in [
            &b"bad"[..],
            b"@123",
            b"x@",
            b"x@1.",
            b"x@1..2",
            b"x@a",
            b"z o@1",
            b"z=o@1",
            b"z\"o@1",
            b"z[o@1",
        ] {
            assert!(!valid_id(bad), "{}", bad.escape_ascii());
        }
    }

    #[test]
    fn params_are_name_equals_quoted_value() {
        // `a ="v"` passes too: upstream checks only that `=` sits right
        // before the opening quote, never the name's own bytes.
        for ok in [
            &br#"tiger="hungry""#[..],
            br#"a="""#,
            br#"a="b\]""#,
            br#"a="b\"c""#,
            br#"a="b\\""#,
            br#"a ="v""#,
        ] {
            assert!(valid_param(ok), "{}", ok.escape_ascii());
        }
        for bad in [
            &br#"x=y"#[..], // no quotes
            br#"="v""#,     // no name
            br#"a="b]""#,   // `]` must be escaped
            br#"a="b\x""#,  // `\` only before []"\
            br#"a="v"x"#,   // bytes after the closing quote
            br#"a="v"#,     // unterminated
        ] {
            assert!(!valid_param(bad), "{}", bad.escape_ascii());
        }
    }

    #[test]
    fn elements_render_in_order_and_empty_ones_vanish() {
        let tq = Element {
            id: b"timeQuality".to_vec(),
            params: vec![b"tzKnown=\"1\"".to_vec(), b"isSynced=\"0\"".to_vec()],
        };
        let bare = Element {
            id: b"meta".to_vec(),
            params: Vec::new(),
        };
        let zoo = Element {
            id: b"zoo@123".to_vec(),
            params: vec![b"tiger=\"hungry\"".to_vec()],
        };
        assert_eq!(
            render(&[tq], &[bare, zoo]).as_deref(),
            Some(&br#"[timeQuality tzKnown="1" isSynced="0"][zoo@123 tiger="hungry"]"#[..])
        );
        assert_eq!(render(&[], &[]), None);
    }
}
