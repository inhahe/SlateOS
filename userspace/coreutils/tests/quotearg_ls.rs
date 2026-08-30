//! All ten of GNU's quoting styles, against `ls`, row for row.
//!
//! `quotearg.rs` next door checks the three styles a *diagnostic* can be made
//! to print, using `sort` and `head` as instruments. This checks all ten,
//! using `ls --quoting-style=` — the only program that will render a name in
//! a style of the caller's choosing — and it checks them against the same
//! corpus, so the two measurements are of the same questions asked of
//! different programs.
//!
//! That overlap is the point rather than a waste. Three of the ten are meant
//! to be the functions the other file already tests, and
//! [`agrees_with_the_diagnostic_styles_where_it_should`] asserts it: the two
//! fixtures were produced by different GNU programs on different code paths,
//! so a rule we had reconstructed rather than measured would have to be wrong
//! in the same way twice to survive both. It did not survive: the cross-check
//! is what found that [`quotef`] is *not* `shell-escape` but `shell-escape`
//! with `:` added to the set of bytes that force the quotes on, a difference
//! visible on exactly four names out of 2904.
//!
//! To refresh the fixture (a new GNU release, or a rule found to be
//! under-covered):
//!
//! ```text
//! wsl -e python3 scripts/ls-quote-probe.py \
//!     userspace/coreutils/tests/quotearg-ls-gnu.txt
//! ```

use coreutils::quote::{Style, quote, quoteaf, quotef};

const FIXTURE: &str = include_str!("quotearg-ls-gnu.txt");

/// The styles, in the order the fixture's columns are written.
const COLUMNS: &[Style] = &[
    Style::Literal,
    Style::Shell,
    Style::ShellAlways,
    Style::ShellEscape,
    Style::ShellEscapeAlways,
    Style::C,
    Style::CMaybe,
    Style::Escape,
    Style::Locale,
    Style::Clocale,
];

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd-length hex {s:?}");
    s.as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("hex is ascii");
            u8::from_str_radix(text, 16).expect("hex digit")
        })
        .collect()
}

/// A name and its ten recorded renderings.
struct Row {
    lineno: usize,
    name: Vec<u8>,
    want: Vec<Vec<u8>>,
}

fn rows() -> Vec<Row> {
    let mut out = Vec::new();
    for (index, line) in FIXTURE.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // Not `split_whitespace`: a style whose answer is the empty string
        // writes an empty field, and collapsing runs of spaces would shift
        // every column after it.
        let fields: Vec<&str> = line.split(' ').collect();
        assert_eq!(
            fields.len(),
            COLUMNS.len() + 1,
            "line {}: {} fields, expected {}",
            index + 1,
            fields.len(),
            COLUMNS.len() + 1
        );
        out.push(Row {
            lineno: index + 1,
            name: unhex(fields[0]),
            want: fields[1..].iter().map(|f| unhex(f)).collect(),
        });
    }
    assert!(out.len() > 2500, "fixture shrank to {} rows", out.len());
    out
}

/// Whether `input` holds `c`'s UTF-8 encoding.
///
/// Bytewise, because `input` need not be valid UTF-8 — and a false positive
/// is impossible anyway: a multi-byte encoding's bytes are all ≥ 0x80 and its
/// lead byte cannot appear as a continuation, so the sequence occurs only
/// where the character does.
fn contains_char(input: &[u8], c: char) -> bool {
    let mut buf = [0u8; 4];
    let needle = c.encode_utf8(&mut buf).as_bytes();
    input.windows(needle.len()).any(|w| w == needle)
}

/// The character we render differently from GNU on purpose, and where that
/// difference is allowed to show.
///
/// The reason is `quotearg.rs`'s and `design-decisions.md` §357's: GNU asks
/// glibc's `iswprint`, a 709-range table generated from a particular Unicode
/// release; we ask a rule that needs no table and so cannot drift. The two
/// agree on every assigned character and differ on unassigned ones, which
/// glibc escapes and we print.
///
/// *Which rows* that reaches is itself a fact worth pinning, and it is neither
/// "all of them" nor even "all rows of some styles" — see [`must_differ`].
const DIVERGENT: char = '\u{0378}';

/// Whether a name holding [`DIVERGENT`] is *required* to render differently
/// from GNU under `style`.
///
/// Three cases, and the middle one is the one that had to be measured rather
/// than assumed — this test asserted "the shell styles never consult
/// printability" until GNU disagreed on ten renderings:
///
/// * [`Style::Literal`] escapes nothing, so there is nothing to disagree
///   about. It never differs.
/// * [`Style::Shell`] and [`Style::ShellAlways`] escape nothing either, and
///   choose *whether* to quote bytewise — [`coreutils::quote`]'s
///   `shell_bare_ok` — so printability cannot reach that decision. But when a
///   name contains `'` there are two ways to write it, and printability
///   chooses between them: `"…"`, which needs every character printable, and
///   the `'…'\''…'` splice, which does not. So these two differ on exactly the
///   rows that hold both the character and a `'`.
/// * The remaining seven escape by printability throughout, so every row
///   holding the character differs.
///
/// A row that differs where this says it should not is a real bug; a row that
/// agrees where this says it should not is a recorded reason gone stale. Both
/// fail.
fn must_differ(style: Style, name: &[u8]) -> bool {
    match style {
        Style::Literal => false,
        Style::Shell | Style::ShellAlways => name.contains(&b'\''),
        _ => true,
    }
}

fn show(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// gnulib's `quote_these_too` as `ls` sets it for a **file name** — which is
/// what every row of the fixture is.
///
/// `decode_switches` calls `set_char_quoting (filename_quoting_options, ' ',
/// 1)` for the `escape` style and no other, so that a name holding a space
/// cannot end the word in a rendering that has no quotes to hold it together.
/// It is not part of the style: the *directory header* gets its own options,
/// with `:` in the set and no space, and `ls -b` prints a directory called
/// `d e` as `d e:` while printing a file called `a b` as `a\ b`.
///
/// The probe passes neither `-F` nor `--file-type`, so the indicator
/// characters are not in the set for any row here. See
/// [`coreutils::quote::Style::quote_with`].
fn ls_filename_set(style: Style) -> &'static [u8] {
    if style == Style::Escape { b" " } else { b"" }
}

#[test]
fn matches_gnu_ls_row_for_row() {
    let mut checked = 0usize;
    let mut wrong: Vec<String> = Vec::new();
    // Per style: how many rows exercised the divergent character, and how
    // many came out agreeing with GNU.
    let mut divergent: std::collections::HashMap<usize, (usize, usize)> =
        std::collections::HashMap::new();

    for row in rows() {
        let odd = contains_char(&row.name, DIVERGENT);
        for (column, (&style, want)) in COLUMNS.iter().zip(&row.want).enumerate() {
            let got = style.quote_with(&row.name, ls_filename_set(style));
            checked = checked.saturating_add(1);
            if odd && must_differ(style, &row.name) {
                let entry = divergent.entry(column).or_default();
                entry.0 = entry.0.saturating_add(1);
                if got == *want {
                    entry.1 = entry.1.saturating_add(1);
                }
                continue;
            }
            if got != *want && wrong.len() < 25 {
                wrong.push(format!(
                    "  line {} {style:?}({:?})\n    gnu:  {}\n    ours: {}",
                    row.lineno,
                    show(&row.name),
                    show(want),
                    show(&got)
                ));
            } else if got != *want && wrong.len() == 25 {
                wrong.push("  ...".to_string());
            }
        }
    }

    for (column, &style) in COLUMNS.iter().enumerate() {
        // The one style [`must_differ`] can never fire for, so there is no
        // coverage to demand of the fixture.
        if style == Style::Literal {
            continue;
        }
        let (rows, agreed) = divergent.remove(&column).unwrap_or_default();
        assert!(
            rows > 0,
            "U+{:04X} is recorded as an expected divergence but no fixture row \
             exercises it under {style:?} — widen scripts/quote-probe.py's \
             corpus or drop the entry",
            u32::from(DIVERGENT)
        );
        assert_eq!(
            agreed,
            0,
            "U+{:04X} was expected to differ from GNU under {style:?}, but {agreed} \
             of its {rows} rows now agree, so the recorded reason is stale",
            u32::from(DIVERGENT)
        );
    }

    assert!(
        wrong.is_empty(),
        "{} of {checked} renderings differ from GNU:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn agrees_with_the_diagnostic_styles_where_it_should() {
    // `quotef` is `shell-escape` *plus* `:` in gnulib's `quote_these_too`, so
    // the two disagree on exactly the names a colon would otherwise have let
    // through bare. Naming the four rather than counting them is the point:
    // a fifth would mean the rule is not the one recorded in `SAFE`'s doc.
    let colon_only: &[&[u8]] = &[b":", b"a:", b":z", b"a:z"];
    let mut surprises = Vec::new();

    for row in rows() {
        let escape = Style::ShellEscape.quote(&row.name);
        let expected_to_differ = colon_only.contains(&row.name.as_slice());
        let differs = escape != quotef(&row.name).into_bytes();
        if differs != expected_to_differ {
            surprises.push(format!(
                "  line {} {:?}: shell-escape {} quotef ({} vs {})",
                row.lineno,
                show(&row.name),
                if differs { "differs from" } else { "equals" },
                show(&escape),
                quotef(&row.name)
            ));
        }
        // These two are aliases outright. `quoteaf` always quotes, so the
        // colon never gets a chance to matter; `quote` has no colon rule at
        // all.
        assert_eq!(
            Style::ShellEscapeAlways.quote(&row.name),
            quoteaf(&row.name).into_bytes(),
            "line {}: shell-escape-always is not quoteaf for {:?}",
            row.lineno,
            show(&row.name)
        );
        assert_eq!(
            Style::Locale.quote(&row.name),
            quote(&row.name).into_bytes(),
            "line {}: locale is not quote for {:?}",
            row.lineno,
            show(&row.name)
        );
        assert_eq!(
            Style::Clocale.quote(&row.name),
            Style::Locale.quote(&row.name),
            "line {}: clocale and locale parted ways on {:?}",
            row.lineno,
            show(&row.name)
        );
    }

    assert!(
        surprises.is_empty(),
        "shell-escape and quotef differ on names other than the four the \
         colon rule predicts:\n{}",
        surprises.join("\n")
    );
}

#[test]
fn the_empty_name_is_rendered_by_hand_because_it_cannot_be_a_file() {
    // `ls` needs a file to list, and there is no file whose name is empty, so
    // this row cannot be measured and is asserted from the rules instead.
    // Every one of these is the style's own general case with no bytes in it:
    // `literal` and `escape` copy nothing, the quoting styles emit their
    // delimiters around nothing, and `shell` cannot elide the quotes because
    // an empty word without them is not a word at all. `quote_these_too` is
    // irrelevant here whatever it holds, since there are no bytes to match it.
    let cases: &[(Style, &[u8])] = &[
        (Style::Literal, b""),
        (Style::Shell, b"''"),
        (Style::ShellAlways, b"''"),
        (Style::ShellEscape, b"''"),
        (Style::ShellEscapeAlways, b"''"),
        (Style::C, b"\"\""),
        (Style::CMaybe, b""),
        (Style::Escape, b""),
        (Style::Locale, "‘’".as_bytes()),
        (Style::Clocale, "‘’".as_bytes()),
    ];
    for &(style, want) in cases {
        assert_eq!(style.quote(b""), want, "{style:?} of the empty name");
    }
}

#[test]
fn every_style_has_a_word_and_every_word_a_style() {
    // A variant with no spelling could never be selected, and a spelling with
    // no variant would be accepted and then ignored.
    for &style in COLUMNS {
        assert!(
            Style::WORDS.iter().any(|&(_, s)| s == style),
            "{style:?} has no --quoting-style word"
        );
    }
    assert_eq!(
        Style::WORDS.len(),
        COLUMNS.len(),
        "the word list and the fixture's columns disagree on how many styles there are"
    );
    // The order is GNU's, and it is the order the diagnostic lists them in.
    let words: Vec<&str> = Style::WORDS.iter().map(|&(w, _)| w).collect();
    assert_eq!(
        words,
        [
            "literal",
            "shell",
            "shell-always",
            "shell-escape",
            "shell-escape-always",
            "c",
            "c-maybe",
            "escape",
            "locale",
            "clocale"
        ]
    );
}

#[test]
fn the_fixture_records_which_gnu_produced_it() {
    // The version matters when a row moves: it is the difference between "we
    // regressed" and "GNU changed".
    let header: Vec<&str> = FIXTURE.lines().take_while(|l| l.starts_with('#')).collect();
    assert!(
        header.iter().any(|l| l.contains("GNU coreutils")),
        "fixture header does not name the GNU release it came from: {header:?}"
    );
}
