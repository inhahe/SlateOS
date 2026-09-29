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
//!
//! A few examples test what the grammar as published does not have -- an
//! opt-in extension its `parser.c` was generated without -- and are named,
//! with the reason, beside the test that leaves them out; a name that
//! matches no example fails the test, so the list cannot outlive them.

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

/// Whether `line` is a header's `===` line -- in a file with CRLF endings,
/// its CR aside.
fn is_equals(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    line.len() >= 3 && line.bytes().all(|b| b == b'=')
}

/// Whether `line` is a divider's `---` line, its CR aside.
fn is_dashes(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    line.len() >= 3 && line.bytes().all(|b| b == b'-')
}

/// The examples in one corpus file.
fn examples(text: &str) -> Vec<Example> {
    // Lines keep their CRs: in a corpus of CRLF input (`crlf.txt`) the CRs
    // are the test, and `tree-sitter test` hands the parser the bytes as
    // they are.
    let lines: Vec<&str> = text.split('\n').collect();
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

/// Run every example in `grammars/<dir>/corpus/` but those `not_built`
/// names (`(file, example)`), answering how many ran and the failures -- a
/// `not_built` name no example has among them.
fn run(language: &str, dir: &str, not_built: &[(&str, &str)]) -> (usize, Vec<String>) {
    let lang = Language::for_injection(language).expect("a language");
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
    let mut left_out = vec![false; not_built.len()];
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a corpus file");
        // Kept as it is: compared as a name, rendered only in messages. A
        // file's name need not be text.
        let name = file.file_name().unwrap_or_default();
        let short = name.display();
        for example in examples(&text) {
            if example.skip {
                continue;
            }
            if let Some(at) = not_built
                .iter()
                .position(|&(f, title)| name == f && title == example.name)
            {
                left_out[at] = true;
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
    for (&(file, name), found) in not_built.iter().zip(left_out) {
        if !found {
            failures.push(format!(
                "{file}: {name}: left out as not built, but there is no such example"
            ));
        }
    }
    (ran, failures)
}

/// Every example of `language`'s corpus but those `not_built` parses as
/// upstream's grammar parses it, and at least `at_least` of them ran.
fn check(language: &str, dir: &str, at_least: usize, not_built: &[(&str, &str)]) {
    let (ran, failures) = run(language, dir, not_built);
    assert!(ran >= at_least, "{language}: only {ran} examples ran");
    assert!(
        failures.is_empty(),
        "{language}: {} of {ran} examples failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// **The Bash grammar -- tables, lexers and its large ported scanner --
/// parses its whole corpus as upstream's does.** (Its `crlf.txt` has no
/// carriage returns: upstream's repository normalises every text file to
/// LF, that one included.)
#[test]
fn bash_passes_its_corpus() {
    check("Bash", "bash", 100, &[]);
}

/// **The HTML grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does**: end tags left out, raw text, custom
/// elements.
#[test]
fn html_passes_its_corpus() {
    check("HTML", "html", 20, &[]);
}

/// **The JavaScript grammar -- tables, lexers and ported scanner -- parses
/// its whole corpus as upstream's does**: the semicolons a line leaves out,
/// template strings, regular expressions, JSX.
#[test]
fn javascript_passes_its_corpus() {
    check("JavaScript", "javascript", 115, &[]);
}

/// **The C grammar parses its whole corpus as upstream's does.**
#[test]
fn c_passes_its_corpus() {
    check("C", "c", 85, &[]);
}

/// **The CSS grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn css_passes_its_corpus() {
    check("CSS", "css", 40, &[]);
}

/// **The TOML grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn toml_passes_its_corpus() {
    check("TOML", "toml", 17, &[]);
}

/// **The YAML grammar -- tables, lexers and its large ported scanner --
/// parses its whole corpus as upstream's does.**
#[test]
fn yaml_passes_its_corpus() {
    check("YAML", "yaml", 95, &[]);
}

/// **Markdown's block grammar -- tables, lexers and its large ported scanner
/// -- parses its whole corpus as upstream's does.**
#[test]
fn markdown_passes_its_corpus() {
    check("Markdown", "markdown", 322, &[]);
}

/// **Markdown's inline grammar parses its whole corpus as upstream's does**
/// -- but for the examples of the two extensions a user must opt into when
/// generating it, tags (`#tag`) and wiki links (`[[page]]`). Its published
/// `parser.c`, the one vendored, is generated without them (it has no
/// `tag` or `wiki_link` node), which is what a README wants: GitHub reads
/// neither. Upstream runs its corpus against a grammar generated with
/// `ALL_EXTENSIONS=1`, which is also why two of the spec's examples are
/// here: their expected trees were written by it, and it reads `&#x;` as
/// holding the tag `#x`.
#[test]
fn markdown_inline_passes_its_corpus() {
    check(
        "markdown_inline",
        "markdown_inline",
        337,
        &[
            ("extension_wikilink.txt", "Basic Wiki-link parsing."),
            ("extension_wikilink.txt", "Wiki-link to a file"),
            ("extension_wikilink.txt", "Wiki-link to a heading in a note"),
            ("extension_wikilink.txt", "Wiki-link with title"),
            ("extension_wikilink.txt", "Wiki-link version of Example 556"),
            (
                "spec.txt",
                "Example 324 - https://github.github.com/gfm/#example-324",
            ),
            (
                "spec.txt",
                "Example 638 - https://github.github.com/gfm/#example-638",
            ),
            ("tags.txt", "Tags are working"),
        ],
    );
}

/// **The JSON grammar parses its whole corpus as upstream's does.**
#[test]
fn json_passes_its_corpus() {
    check("JSON", "json", 6, &[]);
}

/// **The Rust grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn rust_passes_its_corpus() {
    check("Rust", "rust", 150, &[]);
}

/// **The Python grammar -- tables, lexers and ported scanner -- parses its
/// whole corpus as upstream's does.**
#[test]
fn python_passes_its_corpus() {
    check("Python", "python", 110, &[]);
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

/// **An example left out must exist**: a name in a `not_built` list that no
/// example of the corpus has is a failure, so the list cannot outlive the
/// examples it names -- and one that does exist is not run.
#[test]
fn a_left_out_example_must_exist() {
    let (_, failures) = run("JSON", "json", &[("main.txt", "No such example")]);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("no such example"), "{failures:?}");
    let (all, _) = run("JSON", "json", &[]);
    let (fewer, failures) = run("JSON", "json", &[("main.txt", "Arrays")]);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(fewer, all - 1);
}
