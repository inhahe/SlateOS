//! Every grammar's own highlight tests, run against this crate's
//! highlighter.
//!
//! A grammar's authors test its highlight query with `tree-sitter test` as
//! well as its trees: source files whose comments point at the line above
//! and name the capture that colours it (`test/highlight/`, vendored as
//! `grammars/<name>/highlight/`):
//!
//! ```text
//! int main(void) {
//! // <- type
//! //  ^ function
//! ```
//!
//! `<-` points at the comment's own first column, `^` at its column and
//! `^^^` at three; `!name` says the colour there is not that one. A comment
//! points at the nearest line above it that is neither another such comment
//! nor too short to reach the column. The assertions are read as
//! `tree-sitter test` reads them and checked against what
//! [`SyntaxHighlighter`](crate::SyntaxHighlighter) paints, which is what
//! says it settles captures as tree-sitter's highlighter does -- of one
//! node's, the last pattern's wins; inside a node, a node within it wins.
//! A name is compared as the kind it paints ([`Highlight::for_capture`]):
//! `function.method` and `function` are both a function here, and an
//! assertion of a name that paints nothing (`@spell`) is not checked.

use std::path::PathBuf;
use std::time::Duration;

use guitk::highlight::{Highlight, Highlighter};
use guitk::textbuffer::TextBuffer;

use crate::{Language, Paint};

/// One assertion.
#[derive(Debug)]
struct Assertion {
    /// The row it points at: its comment's, until moved to the line above.
    row: usize,
    /// The column it points at, in characters.
    column: usize,
    /// How many columns: the number of `^`s.
    length: usize,
    /// `!name`: the colour there must not be `name`'s.
    negative: bool,
    /// The capture's name.
    name: String,
}

/// The assertion in the comment `node`, if it holds one: an arrow, perhaps
/// a `!`, and a name -- `tree-sitter test`'s reading, byte for byte.
fn assertion_in(node: tree_sitter::Node<'_>, source: &str) -> Option<Assertion> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let start = node.start_position();
    if start.row == 0 {
        return None;
    }
    let mut column = start.column;
    let mut has_left_caret = false;
    let mut has_arrow = false;
    let mut arrow_end = 0;
    let mut arrow_count = 1;
    for (i, c) in text.char_indices() {
        arrow_end = i + 1;
        if c == '-' && has_left_caret {
            has_arrow = true;
            break;
        }
        if c == '^' {
            has_arrow = true;
            column += i;
            for c in text[arrow_end..].chars() {
                if c != '^' {
                    arrow_end += arrow_count - 1;
                    break;
                }
                arrow_count += 1;
            }
            break;
        }
        has_left_caret = c == '<';
    }
    if !has_arrow {
        return None;
    }
    let mut negative = false;
    for (i, c) in text[arrow_end..].char_indices() {
        if c == '!' {
            negative = true;
            arrow_end += i + 1;
            break;
        } else if !c.is_whitespace() {
            break;
        }
    }
    // The name: the first run of word characters, `-` and `.` (`[\w_\-.]+`).
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
    let rest = &text[arrow_end..];
    let begin = rest.find(is_name)?;
    let name: String = rest[begin..].chars().take_while(|&c| is_name(c)).collect();
    // The column in characters, as the lines above are compared.
    let line = source.split_inclusive('\n').nth(start.row)?;
    let column = line.get(..column).map_or(column, |l| l.chars().count());
    Some(Assertion {
        row: start.row,
        column,
        length: arrow_count,
        negative,
        name,
    })
}

/// Every assertion in `source`, parsed as `language`, each pointed at the
/// line it is about; in order.
fn assertions(language: &Language, source: &str) -> Vec<Assertion> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.ts_language())
        .expect("the grammar loads");
    let tree = parser.parse(source, None).expect("a tree");
    let mut found = Vec::new();
    // The row each assertion's comment starts on.
    let mut comment_rows = Vec::new();
    // Every node, children before their parent, as `tree-sitter test`
    // walks them.
    let mut cursor = tree.root_node().walk();
    let mut ascending = false;
    loop {
        if ascending {
            let node = cursor.node();
            if node.kind().to_lowercase().contains("comment") {
                if let Some(assertion) = assertion_in(node, source) {
                    comment_rows.push(node.start_position().row);
                    found.push(assertion);
                }
            }
            if cursor.goto_next_sibling() {
                ascending = false;
            } else if !cursor.goto_parent() {
                break;
            }
        } else if !cursor.goto_first_child() {
            ascending = true;
        }
    }
    // Up to the line each is about: past other assertions' lines, and
    // lines too short to have the column.
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut i = 0;
    for assertion in &mut found {
        loop {
            let on_assertion_line = comment_rows[i..].contains(&assertion.row);
            let on_empty_line = lines
                .get(assertion.row)
                .is_none_or(|l| l.len() <= assertion.column);
            if on_assertion_line || on_empty_line {
                assert!(
                    assertion.row > 0,
                    "`{}`: no line above it to point at",
                    assertion.name
                );
                assertion.row -= 1;
            } else {
                while comment_rows.get(i).is_some_and(|&r| r < assertion.row) {
                    i += 1;
                }
                break;
            }
        }
    }
    found.sort_by_key(|a| (a.row, a.column));
    found
}

/// A row and a column in characters, as assertions point.
type Position = (usize, usize);

/// A byte offset in `source` as a row and a column in characters.
fn point(source: &str, line_starts: &[usize], byte: usize) -> Position {
    let row = line_starts
        .partition_point(|&s| s <= byte)
        .saturating_sub(1);
    let start = line_starts[row];
    (row, source[start..byte].chars().count())
}

/// Check every file in `grammars/<dir>/highlight/`: how many assertions
/// were checked, and the failures.
fn run(language: &str, dir: &str) -> (usize, Vec<String>) {
    let lang = Language::named(language).expect("a language");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("grammars")
        .join(dir)
        .join("highlight");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the highlight tests")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut checked = 0;
    let mut failures = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("a test file");
        let short = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let buffer = TextBuffer::from_text(&source);
        let mut h = lang.highlighter().expect("a highlighter");
        h.reset(&buffer);
        while h.work(&buffer, Duration::from_secs(5)) {}
        let line_starts: Vec<usize> = core::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let spans: Vec<(Position, Position, Highlight)> = h
            .highlights(&buffer, 0..buffer.len())
            .into_iter()
            .map(|s| {
                (
                    point(&source, &line_starts, s.range.start),
                    point(&source, &line_starts, s.range.end),
                    s.highlight,
                )
            })
            .collect();
        for a in assertions(lang, &source) {
            let Paint::Kind(want) = Paint::for_capture(&a.name) else {
                continue;
            };
            checked += 1;
            let (from, to) = ((a.row, a.column), (a.row, a.column + a.length));
            let over: Vec<Highlight> = spans
                .iter()
                .filter(|(start, end, _)| *start < to && *end > from)
                .map(|&(_, _, kind)| kind)
                .collect();
            let passed = over.contains(&want) != a.negative;
            if !passed {
                failures.push(format!(
                    "{short}:{}:{}: expected {}{} ({want:?}), painted {over:?}",
                    a.row + 1,
                    a.column + 1,
                    if a.negative { "not " } else { "" },
                    a.name,
                ));
            }
        }
    }
    (checked, failures)
}

/// `language`'s highlight tests all pass, and at least `at_least` of their
/// assertions were checked.
fn check(language: &str, dir: &str, at_least: usize) {
    let (checked, failures) = run(language, dir);
    assert!(
        checked >= at_least,
        "{language}: only {checked} assertions checked"
    );
    assert!(
        failures.is_empty(),
        "{language}: {} of {checked} assertions failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// **C is coloured as its grammar's highlight tests say.**
#[test]
fn c_is_coloured_as_its_tests_say() {
    check("C", "c", 23);
}

/// **CSS is coloured as its grammar's highlight tests say.**
#[test]
fn css_is_coloured_as_its_tests_say() {
    check("CSS", "css", 37);
}

/// **Python is coloured as its grammar's highlight tests say.**
#[test]
fn python_is_coloured_as_its_tests_say() {
    check("Python", "python", 34);
}

/// **TOML is coloured as its grammar's highlight tests say.**
#[test]
fn toml_is_coloured_as_its_tests_say() {
    check("TOML", "toml", 16);
}

/// **YAML is coloured as its grammar's highlight tests say.**
#[test]
fn yaml_is_coloured_as_its_tests_say() {
    check("YAML", "yaml", 25);
}

/// **Assertions are read as `tree-sitter test` reads them**: `<-` at the
/// comment's column, `^` at its own and `^^` over two, `!` negating, the
/// name after `@` or not, and each moved up past the assertion lines and
/// the lines too short to reach it.
#[test]
fn assertions_are_read_as_tree_sitter_reads_them() {
    let python = Language::named("python").unwrap();
    // The first points past the end of `x`'s line, so at the line above.
    let source = "long_name = 1\nx\n#           ^ number\n# <- variable\n#   ^^ ! @number\n";
    let found = assertions(python, source);
    let got: Vec<(usize, usize, usize, bool, &str)> = found
        .iter()
        .map(|a| (a.row, a.column, a.length, a.negative, a.name.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (0, 4, 2, true, "number"),
            (0, 12, 1, false, "number"),
            (1, 0, 1, false, "variable"),
        ]
    );
}
