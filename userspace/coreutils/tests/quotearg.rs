//! `quote` / `quotef` / `quoteaf` against GNU coreutils, row for row.
//!
//! The expectations are not written by hand and are not derived from reading
//! gnulib. They were *measured*: `scripts/quote-probe.py` runs GNU `sort` and
//! `head` under `LC_ALL=C` over every byte in every position that matters,
//! plus an adversarial and a random corpus, and records what GNU printed.
//! This test replays that recording.
//!
//! Recording rather than shelling out to GNU at test time is deliberate. The
//! development host is Windows, where the only GNU coreutils available run
//! under MSYS and re-encode `argv` — so a name holding a byte that is not
//! valid UTF-8 never reaches the program intact, and a live comparison would
//! be measuring the translation layer. It also means this test needs nothing
//! installed and cannot go quiet when a tool is missing, which is the usual
//! way a differential test stops testing anything.
//!
//! To refresh it (a new GNU release, or a rule found to be under-covered):
//!
//! ```text
//! wsl -e python3 scripts/quote-probe.py userspace/coreutils/tests/quotearg-gnu.txt
//! ```
//!
//! A row that changes is a finding, not a nuisance: look at what moved before
//! adjusting anything.

use coreutils::quote::{quote, quoteaf, quotef};

const FIXTURE: &str = include_str!("quotearg-gnu.txt");

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
            "quote" => quote(&input),
            "quotef" => quotef(&input),
            "quoteaf" => quoteaf(&input),
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
    // A fixture that silently shrank to nothing would let this pass while
    // testing nothing at all. The floor is a little under the real count
    // (8333) so that adding a probe case is not also a test edit, while a
    // fixture that lost a whole style still trips it.
    assert!(checked > 8000, "fixture only had {checked} rows");
    // Each style must be present: a dispatch arm that silently matched
    // nothing would look exactly like a passing test.
    for style in ["quote", "quotef", "quoteaf"] {
        assert!(
            *seen.get(style).unwrap_or(&0) > 500,
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
fn the_fixture_records_which_gnu_produced_it() {
    // The version matters when a row moves: it is the difference between "we
    // regressed" and "GNU changed".
    let header: Vec<&str> = FIXTURE.lines().take_while(|l| l.starts_with('#')).collect();
    assert!(
        header.iter().any(|l| l.contains("GNU coreutils")),
        "fixture header does not name the GNU release it came from: {header:?}"
    );
}
