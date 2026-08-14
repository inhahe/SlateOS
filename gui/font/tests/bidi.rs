//! The Unicode bidi conformance suite, run against `osfont::bidi`.
//!
//! `BidiCharacterTest.txt` is the oracle for UAX #9: each line is a string,
//! a requested paragraph direction, and the paragraph level, per-character
//! embedding levels and visual order that a conforming implementation must
//! produce. There is no judgement to exercise here and no partial credit — a
//! rule is either implemented or it is not, and the suite says which.
//!
//! Two tests, from two files, both written by
//! `gui/font/tools/gen_bidi_conformance.py`:
//!
//! * [`the_sampled_conformance_suite_passes`] runs the checked-in sample on
//!   every `cargo test`. It is stratified over the distinct *shapes* of case
//!   in the suite, so it covers every rule rather than every string.
//! * [`the_whole_conformance_suite_passes`] runs all ninety thousand, and is
//!   `#[ignore]`d because the file it needs is too big to check in. Run the
//!   generator first, then `cargo test -- --ignored`.
//!
//! Failures are reported with the case that produced them, decoded, rather
//! than as a count — a bidi bug is diagnosed by looking at the string.

// The oracle file is machine-generated and its format is fixed, so a field
// that will not parse means the fixture is corrupt — which a panic naming
// the line reports far better than a `Result` threaded through the parser.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use osfont::bidi::{Base, Level, resolve};

/// One line of `BidiCharacterTest.txt`.
struct Case {
    line: usize,
    text: Vec<char>,
    base: Base,
    /// The paragraph level the suite expects, whatever `base` asked for.
    para: Level,
    /// One entry per input character; `None` where rule X9 removed it and the
    /// suite therefore does not check its level.
    levels: Vec<Option<Level>>,
    /// The indices of the surviving characters, in the order they are drawn.
    order: Vec<usize>,
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

/// Parse the suite's five semicolon-separated fields.
fn parse(text: &str) -> Vec<Case> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split(';').collect();
        assert_eq!(
            f.len(),
            5,
            "line {}: expected five fields, got {}: {line}",
            n + 1,
            f.len()
        );
        let chars: Vec<char> = f[0]
            .split_whitespace()
            .map(|c| {
                let cp = u32::from_str_radix(c, 16).expect("hex code point");
                char::from_u32(cp).expect("a code point the suite lists must be a char")
            })
            .collect();
        let base = match f[1].trim() {
            "0" => Base::Ltr,
            "1" => Base::Rtl,
            "2" => Base::Auto,
            other => panic!("line {}: unknown direction {other:?}", n + 1),
        };
        let levels = f[3]
            .split_whitespace()
            .map(|v| {
                if v == "x" {
                    None
                } else {
                    Some(v.parse().expect("a level or x"))
                }
            })
            .collect();
        out.push(Case {
            line: n + 1,
            text: chars,
            base,
            para: f[2].trim().parse().expect("a paragraph level"),
            levels,
            order: f[4]
                .split_whitespace()
                .map(|v| v.parse().expect("an index"))
                .collect(),
        });
    }
    out
}

/// Run every case, returning the first few failures rendered for a human.
fn run(cases: &[Case]) -> (usize, String) {
    let mut failed = 0usize;
    let mut report = String::new();
    for case in cases {
        let got = resolve(&case.text, case.base);

        let mut problem = None;
        if got.level() != case.para {
            problem = Some(format!(
                "paragraph level {}, expected {}",
                got.level(),
                case.para
            ));
        } else if got.levels().len() != case.levels.len() {
            problem = Some(format!(
                "{} levels, expected {}",
                got.levels().len(),
                case.levels.len()
            ));
        } else {
            // Only the characters the suite gives a level for are checked:
            // rule X9 removes the rest, and their levels are unobservable.
            for (i, want) in case.levels.iter().enumerate() {
                let Some(want) = want else { continue };
                let have = got.levels().get(i).copied().unwrap_or(u8::MAX);
                if have != *want {
                    problem = Some(format!("level[{i}] = {have}, expected {want}"));
                    break;
                }
            }
            if problem.is_none() {
                // `reorder` already drops what rule X9 removed, which is
                // exactly the set the suite left without a level.
                let visual = got.reorder();
                if visual != case.order {
                    problem = Some(format!(
                        "visual order {visual:?}, expected {:?}",
                        case.order
                    ));
                }
            }
        }

        if let Some(problem) = problem {
            failed = failed.saturating_add(1);
            if failed <= 10 {
                let _ = writeln!(
                    report,
                    "line {}: {problem}\n    chars {:04X?}\n    base {:?}, levels {:?}",
                    case.line,
                    case.text.iter().map(|&c| c as u32).collect::<Vec<_>>(),
                    case.base,
                    got.levels()
                );
            }
        }
    }
    (failed, report)
}

#[test]
fn the_sampled_conformance_suite_passes() {
    let path = data_dir().join("bidi_conformance.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — the sample is checked in, so this should always be \
             readable; regenerate it with gui/font/tools/gen_bidi_conformance.py",
            path.display()
        )
    });
    let cases = parse(&text);
    assert!(
        cases.len() > 1000,
        "only {} cases in the sample — the fixture is truncated",
        cases.len()
    );
    let (failed, report) = run(&cases);
    assert_eq!(
        failed,
        0,
        "{failed} of {} conformance cases failed:\n{report}",
        cases.len()
    );
    println!("{} cases, all passing", cases.len());
}

#[test]
#[ignore = "needs the 6.8 MB BidiCharacterTest.txt, which is not checked in"]
fn the_whole_conformance_suite_passes() {
    let path = data_dir().join("BidiCharacterTest.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} — run `python gui/font/tools/gen_bidi_conformance.py` to \
             fetch it",
            path.display()
        )
    });
    let cases = parse(&text);
    assert!(
        cases.len() > 90_000,
        "only {} cases — the suite is truncated",
        cases.len()
    );
    let (failed, report) = run(&cases);
    assert_eq!(
        failed,
        0,
        "{failed} of {} conformance cases failed:\n{report}",
        cases.len()
    );
    println!("{} cases, all passing", cases.len());
}
