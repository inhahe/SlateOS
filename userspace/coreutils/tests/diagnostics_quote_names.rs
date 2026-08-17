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

/// `eprintln!("prog: {ident}: ...")` — the shape where `ident` ends up as a
/// bare name in the message, which is what `quotef` exists to prevent.
fn bare_interpolated_name(line: &str) -> Option<&str> {
    let after_prog = line.split_once("eprintln!(\"")?.1.split_once(": {")?;
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
fn no_diagnostic_interpolates_a_bare_file_name() {
    let mut found = Vec::new();
    for (file, text) in sources() {
        for (i, line) in text.lines().enumerate() {
            if let Some(ident) = bare_interpolated_name(line) {
                found.push(format!(
                    "  {file}:{}: {{{ident}}} goes into the message unquoted\n    {}",
                    i + 1,
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
        for (i, line) in text.lines().enumerate() {
            if !line.contains("eprintln!(") && !line.contains("println!(") {
                continue;
            }
            if line.contains("'{") && line.contains("}'") {
                found.push(format!("  {file}:{}: {}", i + 1, line.trim()));
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
