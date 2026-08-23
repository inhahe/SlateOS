//! Every diagnostic that names a file must route the name through `quote`.
//!
//! The sweep that made this true was one commit; keeping it true is the hard
//! part, because the natural way to write the next diagnostic is
//!
//! ```text
//! eprintln!("cut: {path}: {e}");
//! ```
//!
//! which is correct-looking, compiles, and reintroduces the hole: a file
//! called `x\ncut: /etc/shadow: Permission denied` writes a second line into
//! `cut`'s error stream that `cut` never wrote. Nothing in review catches that
//! reliably — it looks like every other `eprintln!` in the tree — so this test
//! reads the source and catches it mechanically.
//!
//! It is deliberately a *source* test rather than a behavioural one. Checking
//! it by running each of the 85 binaries against a hostile name would need
//! them built and a filesystem that accepts the names, and would still only
//! cover the paths a test happened to drive. Reading the source covers every
//! call site including the ones on rare error paths, which is where an
//! unquoted name would actually hide.

use std::path::{Path, PathBuf};

fn bin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin")
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![bin_dir()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src/bin is readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(bin_dir())
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.push((name, std::fs::read_to_string(&path).expect("utf-8 source")));
            }
        }
    }
    out.sort();
    assert!(out.len() > 50, "only found {} sources", out.len());
    out
}

/// Identifiers that hold a *message*, not a name — these are already-rendered
/// text and quoting them again would be wrong.
const NOT_A_NAME: &[&str] = &["msg", "e", "err", "error", "message", "reason"];

/// How many physical lines one wrapped macro call may span before
/// [`logical_lines`] gives up on it. A bound is required because the scanner
/// does not model every Rust literal form — a raw string would defeat it — and
/// an unbounded join would then swallow the rest of the file, turning one
/// unrecognised line into a silent hole over everything below it. 40 is far
/// above the longest real call in `src/bin`.
const MAX_JOIN_LINES: usize = 40;

/// Net bracket depth of `src`, ignoring brackets inside string and char
/// literals; `None` if a string literal is still open at the end.
///
/// Skipping literals is not an optimisation. A format string is full of `{`
/// and `}` and often contains a paren of its own — `purged {n} job(s)` — so
/// counting those would leave every such call permanently unbalanced and it
/// would never be joined.
fn bracket_delta(src: &str) -> Option<isize> {
    let b = src.as_bytes();
    let mut depth: isize = 0;
    let mut k = 0usize;
    while k < b.len() {
        match b[k] {
            b'"' => {
                k += 1;
                while k < b.len() && b[k] != b'"' {
                    k += if b[k] == b'\\' { 2 } else { 1 };
                }
                if k >= b.len() {
                    return None;
                }
            }
            // `'x'` and `'\n'` are char literals and can hide a bracket; `'a`
            // is a lifetime and must be stepped over one byte at a time.
            b'\'' => {
                if k + 2 < b.len() && b[k + 1] == b'\\' {
                    if let Some(end) = b[k + 2..].iter().position(|&c| c == b'\'') {
                        k += 2 + end;
                    }
                } else if k + 2 < b.len() && b[k + 2] == b'\'' {
                    k += 2;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    Some(depth)
}

/// Do hand-written single quotes in `line` wrap an actual format
/// *placeholder*?
///
/// The naive test — "contains `'{` and contains `}'`" — cannot tell a
/// placeholder from a doubled brace. `{{` and `}}` are Rust's escapes for a
/// literal `{` and `}`, so a usage line reading
///
/// ```text
/// aws events put-rule --event-pattern '{{"source":["aws.s3"]}}'
/// ```
///
/// prints a JSON example with no interpolation at all, yet contains both
/// substrings. Reporting it is not merely noise: there is no name in it, so
/// no edit can ever resolve the report — it is a permanent false positive
/// that a reader must re-dismiss on every run. `eventbridge-cli`'s help text
/// in the wider tree has two.
///
/// The scan starts at the macro's opening quote so that a brace belonging to
/// enclosing *code* — `if x { println!(…) }` joined onto one logical line —
/// cannot be mistaken for a placeholder and swallow the real one after it.
fn quotes_around_placeholder(line: &str) -> bool {
    let Some(open) = line.find("println!") else {
        return false;
    };
    let Some(quote) = line[open..].find('"') else {
        return false;
    };
    let fmt: Vec<char> = line[open + quote + 1..].chars().collect();
    let mut i = 0;
    while i < fmt.len() {
        match fmt[i] {
            '{' if fmt.get(i + 1) == Some(&'{') => i += 2,
            '{' => {
                let Some(off) = fmt[i..].iter().position(|&c| c == '}') else {
                    return false;
                };
                let close = i + off;
                if i > 0 && fmt[i - 1] == '\'' && fmt.get(close + 1) == Some(&'\'') {
                    return true;
                }
                i = close + 1;
            }
            '}' if fmt.get(i + 1) == Some(&'}') => i += 2,
            _ => i += 1,
        }
    }
    false
}

/// The lines of `text`, but with a `println!`/`eprintln!` that rustfmt split
/// across several of them rejoined into one.
///
/// Both detectors below match a *line*, and rustfmt routinely breaks a long
/// call so that the name lands on a line with no macro name on it:
///
/// ```text
/// eprintln!(
///     "tar: {}: unsupported entry type '{}'; skipped",
///     quotef_os(&name),
///     char::from(other)
/// );
/// ```
///
/// Without this, that call is invisible to both — and it was: the line above
/// is the real `tar.rs` site that this test passed over for as long as it
/// existed, while a crafted archive could set the type byte to `\n` and forge
/// a line of `tar`'s stderr. A checker that a formatter can silence reports a
/// clean tree for a dirty one.
///
/// A physical line ending in an odd number of backslashes is Rust's *line
/// continuation* inside a string literal: the backslash, the newline and the
/// next line's leading whitespace all vanish from the string's value. That
/// boundary is therefore joined with nothing rather than a space, so the text
/// this test reads is the text the compiler sees. `diskutil` in the wider tree
/// is written that way, and joining it with a space put a `\ ` in the middle of
/// its message — which is not a valid escape at all.
///
/// Returns `(first physical line number, source)`, so a report still points at
/// the line a reader would go to.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // `eprintln!(` contains `println!(`, so take the earliest match rather
        // than whichever is looked for first.
        let start = match (line.find("eprintln!("), line.find("println!(")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        let Some(start) = start else {
            out.push((i + 1, line.to_string()));
            i += 1;
            continue;
        };
        let mut joined = line.to_string();
        let mut j = i;
        while !matches!(bracket_delta(&joined[start..]), Some(d) if d <= 0) {
            j += 1;
            if j >= lines.len() || j - i >= MAX_JOIN_LINES {
                j = i;
                joined = line.to_string();
                break;
            }
            if is_continuation(&joined) {
                joined.pop();
                joined.push_str(lines[j].trim_start());
            } else {
                joined.push(' ');
                joined.push_str(lines[j].trim());
            }
        }
        out.push((i + 1, joined));
        i = j + 1;
    }
    out
}

/// Does `src` end in a string-literal line continuation? An *odd* number of
/// trailing backslashes: `"a\` continues, `"a\\` is an escaped backslash and
/// ends the line for real.
fn is_continuation(src: &str) -> bool {
    (src.len() - src.trim_end_matches('\\').len()) % 2 == 1
}

/// `eprintln!("prog: {ident}: ...")` — the shape where `ident` ends up as a
/// bare name in the message, which is what `quotef` exists to prevent.
fn bare_interpolated_name(line: &str) -> Option<&str> {
    // Whitespace is allowed between `(` and `"` because `line` may be a call
    // that [`logical_lines`] reassembled, which leaves the space rustfmt's
    // line break used to be.
    let after_prog = line
        .split_once("eprintln!(")?
        .1
        .trim_start()
        .strip_prefix('"')?
        .split_once(": {")?;
    // The program name must be a plain word: `eprintln!("cut: {path}: ...")`.
    if !after_prog
        .0
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || after_prog.0.is_empty()
    {
        return None;
    }
    let (ident, rest) = after_prog.1.split_once('}')?;
    // `{}` is a positional argument, which the arguments themselves quote;
    // and a `{name:?}` or `{n}` counter is not a file name either.
    if ident.is_empty()
        || !ident
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    // Only when the interpolation is followed by `: `, which is the shape
    // `prog: NAME: reason`. `eprintln!("prog: {msg}")` is a whole message.
    if !rest.starts_with(": ") {
        return None;
    }
    if NOT_A_NAME.contains(&ident) {
        return None;
    }
    Some(ident)
}

#[test]
fn the_detector_finds_what_the_sweep_removed() {
    // Both tests above pass, and a detector that matched nothing would pass
    // exactly the same way. These are real lines from before the sweep.
    for line in [
        r#"                eprintln!("wc: {path}: {e}");"#,
        r#"                    eprintln!("md5sum: {path}: read error");"#,
        r#"                eprintln!("grep: {pf}: {}", strerror(&e));"#,
        r#"            eprintln!("time: {cmd}: {e}");"#,
    ] {
        assert!(
            bare_interpolated_name(line).is_some(),
            "detector missed {line:?}"
        );
    }
    // ...and does not fire on a whole message, on a positional argument that
    // the call site is already quoting, or on ordinary text.
    for line in [
        r#"    eprintln!("awk: {msg}");"#,
        r#"            eprintln!("split: read error: {e}");"#,
        r#"                eprintln!("wc: {}: {e}", quotef_os(path));"#,
        r#"            eprintln!("chmod: {e}");"#,
        r#"    let x = format!("{path}: {e}");"#,
    ] {
        assert_eq!(
            bare_interpolated_name(line),
            None,
            "detector fired on {line:?}"
        );
    }
}

#[test]
fn a_doubled_brace_is_a_literal_brace_not_a_name() {
    // The quote detector's whole job is the shape on the left; the shape on
    // the right merely *contains the same two substrings*, and firing on it
    // would emit a report no edit can satisfy.
    for line in [
        r#"    eprintln!("cut: cannot open '{path}': {e}");"#,
        r#"        println!("{}: removed '{}'", personality, file);"#,
        r#"    eprintln!("cut: {{literal}} '{path}': {e}");"#,
    ] {
        assert!(quotes_around_placeholder(line), "detector missed {line:?}");
    }
    for line in [
        r#"    println!("  aws events put-rule --event-pattern '{{\"source\":[\"aws.s3\"]}}'");"#,
        r#"    println!("  aws scheduler create-schedule --target '{{\"Arn\":\"<a>\"}}'");"#,
        r#"    println!("usage: prog '{{...}}'");"#,
    ] {
        assert!(!quotes_around_placeholder(line), "detector fired on {line:?}");
    }
}

#[test]
fn the_detector_sees_a_call_rustfmt_wrapped() {
    // The regression this exists for. `tar.rs` really did contain the call
    // below, and both detectors walked straight past it for as long as they
    // read physical lines — while the byte it interpolates comes out of the
    // archive header, so `\n` there forged a line of `tar`'s stderr.
    let wrapped = "            eprintln!(\n\
                   \x20               \"tar: {}: unsupported entry type '{}'; skipped\",\n\
                   \x20               quotef_os(&name),\n\
                   \x20               char::from(other)\n\
                   \x20           );\n";
    let joined = logical_lines(wrapped);
    assert_eq!(joined.len(), 1, "call not joined: {joined:?}");
    assert_eq!(joined[0].0, 1, "must report the first physical line");
    assert!(
        joined[0].1.contains("'{") && joined[0].1.contains("}'"),
        "hand-written quotes lost in the join: {:?}",
        joined[0].1
    );

    // The bare-name shape, wrapped so the format string is alone on its line.
    let bare = "eprintln!(\n    \"cut: {path}: {e}\"\n);\n";
    let joined = logical_lines(bare);
    assert_eq!(joined.len(), 1);
    assert_eq!(bare_interpolated_name(&joined[0].1), Some("path"));

    // A paren inside the format string is text, not structure: counting it
    // would leave the call unbalanced and the join would run away.
    let parens = "println!(\"cancel: purged {n} job(s) on '{p}'\");\nlet y = 2;\n";
    let joined = logical_lines(parens);
    assert_eq!(joined.len(), 2, "over-joined past a paren in a literal");

    // A literal broken with a trailing `\` is a line continuation: the
    // backslash, the newline and the next line's indent are all absent from
    // the string's value, so joining that boundary with a space reads a
    // message the compiler never compiled. `diskutil` in the wider tree is
    // written this way.
    let continued = "eprintln!(\n\
                     \x20   \"diskutil: cannot format '{other}' yet -- the FAT \\\n\
                     \x20    family only\"\n\
                     );\n";
    let joined = logical_lines(continued);
    assert_eq!(joined.len(), 1, "continuation not joined: {joined:?}");
    assert!(
        joined[0].1.contains("the FAT family only"),
        "continuation joined with a stray space or backslash: {:?}",
        joined[0].1
    );

    // An *escaped* backslash is a real backslash and ends the line for real,
    // so that boundary keeps its separator.
    let escaped = "eprintln!(\n    \"a\\\\\",\n    x\n);\n";
    let joined = logical_lines(escaped);
    assert_eq!(joined.len(), 1);
    assert!(
        joined[0].1.contains("\"a\\\\\", x"),
        "escaped backslash was treated as a continuation: {:?}",
        joined[0].1
    );

    // And an unparseable line must not swallow what follows it, or one odd
    // line would blind the test to every call below it in the file.
    let unterminated = "let s = \"oops;\neprintln!(\"cut: {path}: {e}\");\n";
    let joined = logical_lines(unterminated);
    assert!(
        joined
            .iter()
            .any(|(_, l)| bare_interpolated_name(l).is_some()),
        "a call after an unterminated literal was swallowed: {joined:?}"
    );
}

#[test]
fn no_diagnostic_interpolates_a_bare_file_name() {
    let mut found = Vec::new();
    for (file, text) in sources() {
        for (line_no, line) in logical_lines(&text) {
            if let Some(ident) = bare_interpolated_name(&line) {
                found.push(format!(
                    "  {file}:{line_no}: {{{ident}}} goes into the message unquoted\n    {}",
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} diagnostic(s) name a file without quoting it.\n\
         Wrap the name: eprintln!(\"prog: {{}}: {{e}}\", quotef_os(name)).\n\
         If the value really is a message rather than a name, add its \
         identifier to NOT_A_NAME in this file.\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn no_diagnostic_hand_writes_quotes_around_a_name() {
    // `'{name}'` is the other half of the same bug, and the more tempting one
    // because it *looks* quoted. It is not: a name containing a quote, a
    // newline or a control byte walks straight out through it. That shape is
    // what `quoteaf` is for.
    let mut found = Vec::new();
    for (file, text) in sources() {
        for (line_no, line) in logical_lines(&text) {
            if !line.contains("eprintln!(") && !line.contains("println!(") {
                continue;
            }
            if quotes_around_placeholder(&line) {
                found.push(format!("  {file}:{line_no}: {}", line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} diagnostic(s) hand-write quotes around a name.\n\
         Hand-written quotes do not survive a name that contains one; use \
         quoteaf_os(name), which always quotes and escapes what it must.\n{}",
        found.len(),
        found.join("\n")
    );
}
