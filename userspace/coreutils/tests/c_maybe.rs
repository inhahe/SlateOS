//! `quote_c_maybe` / `quote_c_maybe_colon` against GNU coreutils, row for row.
//!
//! Same method as `quotearg.rs`, and for the same reason: the expectations are
//! not written from intuition and not derived by reading gnulib. They were
//! *measured* — `scripts/c-maybe-probe.py` drives `ls --quoting-style=c-maybe`
//! and `paste -d 'ARG\'` under `LC_ALL=C` over every byte in three positions
//! plus a corpus of interacting shapes, and records what GNU printed. This
//! test replays the recording.
//!
//! Recording rather than shelling out at test time keeps the suite runnable on
//! the Windows development host, where the available GNU build runs under MSYS
//! and re-encodes `argv`, so a byte that is not valid UTF-8 never arrives
//! intact and a live comparison would be measuring the translation layer.
//!
//! To refresh it:
//!
//! ```text
//! wsl -e python3 scripts/c-maybe-probe.py userspace/coreutils/tests/c-maybe-gnu.txt
//! ```
//!
//! Three inputs are outside both oracles' reach and are covered by unit tests
//! in `quote.rs` instead, marked there as unmeasured: NUL (no argument and no
//! file name may hold it), the empty string, and — for the plain style only —
//! anything containing `/`.

use coreutils::quote::{quote_c_maybe, quote_c_maybe_colon};

const FIXTURE: &str = include_str!("c-maybe-gnu.txt");

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

#[test]
fn matches_gnu_coreutils_row_for_row() {
    let mut checked = 0usize;
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut wrong = Vec::new();
    for (lineno, line) in FIXTURE.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut fields = line.split(' ');
        let style = fields.next().expect("style");
        let input = unhex(fields.next().expect("input"));
        let want = String::from_utf8(unhex(fields.next().expect("output"))).expect("ascii");
        assert!(fields.next().is_none(), "line {}: extra field", lineno + 1);

        let got = match style {
            "c-maybe" => quote_c_maybe(&input),
            "c-maybe-colon" => quote_c_maybe_colon(&input),
            other => panic!("line {}: unknown style {other:?}", lineno + 1),
        };
        checked += 1;
        *seen.entry(style).or_default() += 1;
        if got != want {
            if wrong.len() < 20 {
                wrong.push(format!(
                    "  line {}: {style}({input:?})\n    gnu:  {want}\n    ours: {got}",
                    lineno + 1
                ));
            } else if wrong.len() == 20 {
                wrong.push("  ...".to_string());
            }
        }
    }
    // A fixture that silently shrank would let this pass while testing
    // nothing. The floor sits under the real count (1590) so that adding a
    // probe case is not also a test edit.
    assert!(checked > 1500, "fixture only had {checked} rows");
    for style in ["c-maybe", "c-maybe-colon"] {
        assert!(
            *seen.get(style).unwrap_or(&0) > 700,
            "fixture has only {:?} rows for {style}",
            seen.get(style)
        );
    }
    assert!(
        wrong.is_empty(),
        "{} of {checked} rows differ from GNU:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_two_styles_differ_only_over_the_colon() {
    // The claim the `_colon` variant rests on: `quotearg_n_style_colon` adds
    // `:` to the set of bytes that force the quotes on, and changes nothing
    // else. If that were wrong, the fixture above would still pass — each
    // style is checked against its own oracle — so it is asserted separately.
    for input in [
        &b"abc"[..],
        b"a b",
        b"a'b",
        b"a\"b",
        b"a\\b",
        b"a\tb",
        b"\xff",
        b"{",
        b"#a",
        b"?",
    ] {
        assert_eq!(
            quote_c_maybe(input),
            quote_c_maybe_colon(input),
            "{input:?}"
        );
    }
    for input in [&b":"[..], b"a:b", b"::", b"a:"] {
        assert_ne!(
            quote_c_maybe(input),
            quote_c_maybe_colon(input),
            "{input:?}"
        );
    }
    // ...and once the quotes are on for some other reason, the colon inside
    // them is a plain colon in both, because gnulib drops `quote_these_too`
    // when it restarts.
    assert_eq!(quote_c_maybe(b"a:\tb"), quote_c_maybe_colon(b"a:\tb"));
    assert_eq!(quote_c_maybe_colon(b"a:\tb"), r#""a:\tb""#);
}

#[test]
fn the_fixture_records_which_gnu_produced_it() {
    let header: Vec<&str> = FIXTURE.lines().take_while(|l| l.starts_with('#')).collect();
    assert!(
        header.iter().any(|l| l.contains("GNU coreutils")),
        "fixture header does not name the GNU release it came from: {header:?}"
    );
}
