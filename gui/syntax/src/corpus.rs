//! Every grammar's own test corpus, run against the converted grammar.
//!
//! A grammar's authors test it with `tree-sitter test`: files of examples,
//! each an input and the tree it must parse to (`test/corpus/*.txt`,
//! vendored as `grammars/<name>/corpus/`). Running the same examples here is
//! what says the grammar survived its conversion to Rust -- tables, lexers
//! and hand-ported scanner -- and parses as the C one does. The format and the
//! comparison are `tree-sitter test`'s: whitespace is not significant in the
//! expected tree, field names are compared only when the expected tree has
//! them, and an example marked `:error` needs only to fail to parse cleanly.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::Language;

/// How long one example may take to parse before it is a failure: far past
/// any example's real time (milliseconds), and short enough that a scanner
/// that sends the parser round in circles fails the test rather than hanging
/// it.
const EXAMPLE_LIMIT: Duration = Duration::from_secs(20);

/// One example.
#[derive(Debug)]
struct Example {
    name: String,
    input: String,
    expected: String,
    /// `:error`: the input must parse with an error, whatever the tree.
    error: bool,
    /// `:skip`.
    skip: bool,
}

/// Whether `line` is a header's `===` line.
fn is_equals(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'=')
}

/// Whether `line` is a divider's `---` line.
fn is_dashes(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'-')
}

/// The examples in one corpus file.
fn examples(text: &str) -> Vec<Example> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_equals(lines[i]) {
            i += 1;
            continue;
        }
        // The header: a name, attributes, and the closing `===`.
        let mut name = String::new();
        let mut error = false;
        let mut skip = false;
        i += 1;
        while i < lines.len() && !is_equals(lines[i]) {
            match lines[i].trim() {
                ":error" => error = true,
                ":skip" => skip = true,
                attribute if attribute.starts_with(':') => {}
                title => {
                    if !name.is_empty() {
                        name.push(' ');
                    }
                    name.push_str(title);
                }
            }
            i += 1;
        }
        i += 1;
        // The input, to the divider -- the longest line of dashes before
        // the next header (the last of equals), so an input may itself hold
        // a shorter one.
        let start = i;
        let mut divider: Option<usize> = None;
        while i < lines.len() && !is_equals(lines[i]) {
            if is_dashes(lines[i]) && divider.is_none_or(|d| lines[i].len() >= lines[d].len()) {
                divider = Some(i);
            }
            i += 1;
        }
        let Some(divider) = divider else {
            continue;
        };
        // `tree-sitter test` keeps the blank line after the header and drops
        // the newline before the divider.
        let input = lines[start..divider].join("\n");
        // Comment lines (`; ...`) in the expected tree are not part of it.
        let expected = lines[divider + 1..i]
            .iter()
            .filter(|l| !l.trim_start().starts_with(';'))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        out.push(Example {
            name,
            input,
            expected,
            error,
            skip,
        });
    }
    out
}

/// A tree written out, as `tree-sitter test` compares them: its whitespace
/// made single blanks, none before a `)`.
fn normalize(sexp: &str) -> String {
    sexp.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" )", ")")
}

/// Whether `word` is a field's name and its colon: `name:`.
fn is_field(word: &str) -> bool {
    word.strip_suffix(':').is_some_and(|name| {
        !name.is_empty() && name.chars().all(|c| c == '_' || c.is_alphanumeric())
    })
}

/// A tree with its field names taken out -- ` name: (` made ` (`, as
/// `tree-sitter test` does.
fn without_fields(sexp: &str) -> String {
    let words: Vec<&str> = sexp.split(' ').collect();
    let mut out = Vec::with_capacity(words.len());
    for (i, word) in words.iter().enumerate() {
        let next_opens = words.get(i + 1).is_some_and(|w| w.starts_with('('));
        if i > 0 && is_field(word) && next_opens {
            continue;
        }
        out.push(*word);
    }
    out.join(" ")
}

/// Whether `sexp` names any fields: ` name: (`.
fn has_fields(sexp: &str) -> bool {
    sexp.split(' ')
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| is_field(pair[0]) && pair[1].starts_with('('))
}

/// Run every example in `grammars/<dir>/corpus/`, answering the failures.
fn run(language: &str, dir: &str) -> (usize, Vec<String>) {
    let lang = Language::named(language).expect("a language");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.ts_language())
        .expect("the grammar loads");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("grammars")
        .join(dir)
        .join("corpus");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the corpus")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    let mut ran = 0;
    let mut failures = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a corpus file");
        let short = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for example in examples(&text) {
            if example.skip {
                continue;
            }
            ran += 1;
            let deadline = Instant::now() + EXAMPLE_LIMIT;
            let mut too_long = |_: &tree_sitter::ParseState| Instant::now() >= deadline;
            let input = example.input.as_bytes();
            let parsed = parser.parse_with_options(
                &mut |at: usize, _: tree_sitter::Point| input.get(at..).unwrap_or_default(),
                None,
                Some(tree_sitter::ParseOptions::new().progress_callback(&mut too_long)),
            );
            let Some(tree) = parsed else {
                failures.push(format!(
                    "{short}: {}: no tree within {} s -- the parser went round in circles",
                    example.name,
                    EXAMPLE_LIMIT.as_secs()
                ));
                parser.reset();
                continue;
            };
            let root = tree.root_node();
            if example.error {
                if !root.has_error() {
                    failures.push(format!(
                        "{short}: {}: parsed without the error it should have",
                        example.name
                    ));
                }
                continue;
            }
            let expected = normalize(&example.expected);
            let mut actual = normalize(&root.to_sexp());
            if !has_fields(&expected) {
                actual = without_fields(&actual);
            }
            if actual != expected {
                failures.push(format!(
                    "{short}: {}\n  expected: {expected}\n  actual:   {actual}",
                    example.name
                ));
            }
        }
    }
    (ran, failures)
}

fn check(language: &str, dir: &str, at_least: usize) {
    let (ran, failures) = run(language, dir);
    assert!(ran >= at_least, "{language}: only {ran} examples ran");
    assert!(
        failures.is_empty(),
        "{language}: {} of {ran} examples failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// **The C grammar parses its whole corpus as upstream's does.**
#[test]
fn c_passes_its_corpus() {
    check("C", "c", 85);
}

/// **The CSS grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn css_passes_its_corpus() {
    check("CSS", "css", 40);
}

/// **The TOML grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn toml_passes_its_corpus() {
    check("TOML", "toml", 17);
}

/// **The JSON grammar parses its whole corpus as upstream's does.**
#[test]
fn json_passes_its_corpus() {
    check("JSON", "json", 6);
}

/// **The Rust grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn rust_passes_its_corpus() {
    check("Rust", "rust", 150);
}

/// **The Python grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn python_passes_its_corpus() {
    check("Python", "python", 110);
}

/// **The corpus reader reads the format**: the name, `:error`, the input
/// with its leading blank line, the tree after the divider.
#[test]
fn the_corpus_format_is_read() {
    let e = examples(
        "=====\nOne\n:error\n=====\n\nx\n---\n\n(a (b))\n=====\nTwo\n=====\ny\n---\n(c)\n",
    );
    assert_eq!(e.len(), 2);
    assert_eq!(
        (e[0].name.as_str(), e[0].error, e[0].input.as_str()),
        ("One", true, "\nx")
    );
    assert_eq!(normalize(&e[0].expected), "(a (b))");
    assert_eq!(
        (e[1].input.as_str(), normalize(&e[1].expected).as_str()),
        ("y", "(c)")
    );
    assert_eq!(normalize("(a\n  x: (b)\n  (c)\n)"), "(a x: (b) (c))");
    assert!(has_fields("(a x: (b))") && !has_fields("(a (b))"));
    let e = examples("=====\nD\n=====\na\n---\nb\n------\n(d)\n; a note\n");
    assert_eq!(
        (e[0].input.as_str(), normalize(&e[0].expected).as_str()),
        ("a\n---\nb", "(d)")
    );
    assert_eq!(without_fields("(a x: (b) (c))"), "(a (b) (c))");
}
