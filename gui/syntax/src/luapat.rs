//! Neovim's `#lua-match?`, as tree-sitter's `#match?`.
//!
//! Queries written for Neovim test a capture's text against a Lua pattern
//! -- `(#lua-match? @constant "^[%u_][%u%d_]+$")` -- where tree-sitter tests
//! it against a regular expression, `#match?`. The runtime evaluates
//! `#match?` itself and hands `#lua-match?` back unread, so a pattern
//! carrying one matched every node its shape allows: in the linker-script
//! query, `^%.` (a name starting with a dot) made every name a label. So a
//! query's `#lua-match?` and `#not-lua-match?` are rewritten, before it is
//! compiled, as `#match?` and `#not-match?` with each Lua pattern translated
//! into the regular expression that matches the same texts.
//!
//! Lua's classes are ASCII's (`%a`, `%d`, `%s`...), as the C library's are
//! in the C locale Neovim's Lua runs in; `.` takes any character, a line
//! feed too. A pattern using what a regular expression cannot say -- a
//! balanced match (`%b`), a frontier (`%f`), a back reference (`%1`) -- is
//! refused, and the query with it.

use std::borrow::Cow;

/// The regular expression for Lua's class `%c` -- with no brackets, to go
/// inside a set -- if `c` names one, and whether it is negated (upper case).
fn class(c: char) -> Option<(&'static str, bool)> {
    let body = match c.to_ascii_lowercase() {
        'a' => "A-Za-z",
        'd' => "0-9",
        'l' => "a-z",
        's' => "\\t\\n\\x0B\\x0C\\r ",
        'u' => "A-Z",
        'w' => "A-Za-z0-9",
        'x' => "0-9A-Fa-f",
        'p' => "!-/:-@\\[-`{-~",
        'c' => "\\x00-\\x1F\\x7F",
        'g' => "!-~",
        _ => return None,
    };
    Some((body, c.is_ascii_uppercase()))
}

/// `c` as a regular expression's literal, escaped where it is special.
fn literal(c: char, out: &mut String) {
    if "\\.+*?()|[]{}^$#&-~".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// The regular expression matching what the Lua pattern `pattern` matches.
///
/// # Errors
///
/// A pattern using a balanced match, a frontier or a back reference, or one
/// cut short (a `%` or a `[` at its end).
pub(crate) fn to_regex(pattern: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut first = true;
    while let Some(c) = chars.next() {
        // A repetition with nothing before it to repeat is its character,
        // as Lua reads it.
        let nothing_before = out.is_empty() || out == "^" || out.ends_with("(?:");
        match c {
            '^' if first => out.push('^'),
            '$' if chars.peek().is_none() => out.push('$'),
            '.' => out.push_str("(?s:.)"),
            '*' | '+' | '?' | '-' if nothing_before => literal(c, &mut out),
            '*' | '+' | '?' => out.push(c),
            // Lua's lazy repetition.
            '-' => out.push_str("*?"),
            '(' => out.push_str("(?:"),
            ')' => out.push(')'),
            '%' => {
                let Some(next) = chars.next() else {
                    return Err(format!("`{pattern}` ends with `%`"));
                };
                match class(next) {
                    Some((body, negated)) => {
                        out.push_str(if negated { "[^" } else { "[" });
                        out.push_str(body);
                        out.push(']');
                    }
                    None if next.is_ascii_alphanumeric() => {
                        return Err(format!(
                            "`{pattern}` uses `%{next}`, which a regular expression cannot say"
                        ));
                    }
                    None => literal(next, &mut out),
                }
            }
            '[' => set(pattern, &mut chars, &mut out)?,
            other => literal(other, &mut out),
        }
        first = false;
    }
    Ok(out)
}

/// A set, its `[` taken: its members up to its `]`, as a regular
/// expression's set -- a `]` first a member, a `-` between two a range, a
/// `%` class its characters (negated, a set of its own nested in it).
fn set(
    pattern: &str,
    chars: &mut core::iter::Peekable<core::str::Chars<'_>>,
    out: &mut String,
) -> Result<(), String> {
    out.push('[');
    if chars.next_if_eq(&'^').is_some() {
        out.push('^');
    }
    let mut first = true;
    loop {
        let Some(m) = chars.next() else {
            return Err(format!("`{pattern}` has a set never closed"));
        };
        match m {
            ']' if !first => break,
            '%' => {
                let Some(next) = chars.next() else {
                    return Err(format!("`{pattern}` ends with `%`"));
                };
                match class(next) {
                    Some((body, false)) => out.push_str(body),
                    Some((body, true)) => {
                        out.push_str("[^");
                        out.push_str(body);
                        out.push(']');
                    }
                    None => literal(next, out),
                }
            }
            '-' if !first && chars.peek().is_some_and(|&n| n != ']') => out.push('-'),
            other => literal(other, out),
        }
        first = false;
    }
    out.push(']');
    Ok(())
}

/// `query` with each `#lua-match?` and `#not-lua-match?` rewritten as a
/// `#match?` or `#not-match?` of the same texts; the query itself where it
/// has none.
///
/// # Errors
///
/// A Lua pattern [`to_regex`] refuses, or a `#lua-match?` not followed by a
/// capture and a string.
pub(crate) fn rewrite(query: &str) -> Result<Cow<'_, str>, String> {
    if !query.contains("lua-match?") {
        return Ok(Cow::Borrowed(query));
    }
    let mut out = String::with_capacity(query.len());
    let mut rest = query;
    while let Some(c) = rest.chars().next() {
        match c {
            ';' => {
                let end = rest.find('\n').map_or(rest.len(), |n| n.saturating_add(1));
                out.push_str(rest.get(..end).unwrap_or(rest));
                rest = rest.get(end..).unwrap_or_default();
            }
            '"' => {
                let (literal, after) = string(rest)?;
                out.push_str(literal);
                rest = after;
            }
            '#' if rest.starts_with("#lua-match?") || rest.starts_with("#not-lua-match?") => {
                let negated = rest.starts_with("#not-");
                let name_len = if negated {
                    "#not-lua-match?".len()
                } else {
                    "#lua-match?".len()
                };
                rest = rest.get(name_len..).unwrap_or_default();
                out.push_str(if negated { "#not-match?" } else { "#match?" });
                // The capture, as it is.
                let blanks = rest.len().saturating_sub(rest.trim_start().len());
                out.push_str(rest.get(..blanks).unwrap_or_default());
                rest = rest.get(blanks..).unwrap_or_default();
                if !rest.starts_with('@') {
                    return Err("a `#lua-match?` with no capture".to_owned());
                }
                let capture_len = rest
                    .find(|c: char| c.is_whitespace() || c == ')')
                    .unwrap_or(rest.len());
                out.push_str(rest.get(..capture_len).unwrap_or_default());
                rest = rest.get(capture_len..).unwrap_or_default();
                let blanks = rest.len().saturating_sub(rest.trim_start().len());
                out.push_str(rest.get(..blanks).unwrap_or_default());
                rest = rest.get(blanks..).unwrap_or_default();
                if !rest.starts_with('"') {
                    return Err("a `#lua-match?` with no pattern".to_owned());
                }
                let (literal, after) = string(rest)?;
                let pattern = unescape(
                    literal
                        .get(1..literal.len().saturating_sub(1))
                        .unwrap_or_default(),
                );
                let regex = to_regex(&pattern)?;
                out.push('"');
                out.push_str(&regex.replace('\\', "\\\\").replace('"', "\\\""));
                out.push('"');
                rest = after;
            }
            _ => {
                out.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
            }
        }
    }
    Ok(Cow::Owned(out))
}

/// The string literal `text` starts with, quotes included, and what follows.
fn string(text: &str) -> Result<(&str, &str), String> {
    let mut escaped = false;
    for (at, c) in text.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => {
                let end = at.saturating_add(1);
                return Ok((
                    text.get(..end).unwrap_or(text),
                    text.get(end..).unwrap_or_default(),
                ));
            }
            _ => {}
        }
    }
    Err("a string never closed in a query".to_owned())
}

/// A query string's text, its escapes read: `\\`, `\"`, `\n`, `\t`, `\r`,
/// `\0`, and a backslash before anything else is that thing.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Whether the Lua pattern `pattern`, as a regular expression, matches
    /// `text`.
    fn matches(pattern: &str, text: &str) -> bool {
        regex::Regex::new(&to_regex(pattern).unwrap())
            .unwrap()
            .is_match(text)
    }

    /// **A Lua pattern matches as a regular expression what it matches in
    /// Lua**: classes, sets with classes and ranges, anchors, the four
    /// repetitions, `%` before punctuation, and `.` a line feed too.
    #[test]
    fn a_lua_pattern_matches_what_it_does_in_lua() {
        assert!(matches("^%.", ".text"));
        assert!(!matches("^%.", "text"));
        assert!(matches("^[%u_][%u%d_]+$", "STACK_SIZE_2"));
        assert!(!matches("^[%u_][%u%d_]+$", "Stack_size"));
        assert!(!matches("^[%u_][%u%d_]+$", "A"));
        assert!(matches("^%a+$", "Word") && !matches("^%a+$", "w0rd"));
        assert!(matches("^%A+$", "123 _") && !matches("^%A+$", "a1"));
        assert!(matches("^[a-f]%d-$", "b") && matches("^[a-f]%d-$", "b12"));
        assert!(matches("^[^%s]+$", "no-blanks") && !matches("^[^%s]+$", "a b"));
        assert!(matches("^a.b$", "a\nb"));
        assert!(matches("x?y", "y") && matches("^%(%)$", "()"));
        assert!(matches("^[]x]$", "]"));
        assert!(matches("^%x+$", "DeadBeef") && !matches("^%x+$", "xyz"));
        assert!(matches("^[%w_]+$", "a_1") && matches("a$b", "a$b"));
        // A class negated inside a set; a repetition with nothing to repeat.
        assert!(matches("^[%A0]+$", "0 -") && !matches("^[%A0]+$", "a"));
        assert!(matches("^-a$", "-a") && matches("^*$", "*"));
    }

    /// **What a regular expression cannot say is refused**: a balanced
    /// match, a frontier, a back reference, a pattern cut short.
    #[test]
    fn what_a_regex_cannot_say_is_refused() {
        for bad in ["%b()", "%f[%w]", "(a)%1", "abc%", "[abc"] {
            assert!(to_regex(bad).is_err(), "{bad}");
        }
    }

    /// **A query's `#lua-match?` is rewritten, and nothing else**: the
    /// predicate, its negation, its string's escapes; one in a comment or a
    /// string is left alone.
    #[test]
    fn a_querys_lua_match_is_rewritten_and_nothing_else() {
        let query = "; not this #lua-match? one\n((symbol) @label (#lua-match? @label \"^%.\"))\n((symbol) @c (#not-lua-match? @c \"^[%u_]+$\"))\n(\"#lua-match?\" @string)\n";
        let out = rewrite(query).unwrap();
        assert!(out.starts_with("; not this #lua-match? one\n"), "{out}");
        assert!(out.contains(r#"(#match? @label "^\\.")"#), "{out}");
        assert!(out.contains(r#"(#not-match? @c "^[A-Z_]+$")"#), "{out}");
        assert!(out.contains("(\"#lua-match?\" @string)"), "{out}");
        let plain = "((symbol) @x (#match? @x \"a\"))";
        assert!(matches!(rewrite(plain).unwrap(), Cow::Borrowed(_)));
        assert!(rewrite("(#lua-match? \"x\")").is_err());
        assert!(rewrite("(#lua-match? @x \"%b()\")").is_err());
    }
}
