//! `quote_c_maybe` / `quote_c_maybe_colon` against GNU coreutils, row for row.
//!
//! Same method as `quotearg.rs`, and for the same reason: the expectations are
//! not written from intuition and not derived by reading gnulib. They were
//! *measured* — `scripts/c-maybe-probe.py` drives `ls --quoting-style=c-maybe`
//! and `paste -d 'ARG\'` under `LC_ALL=C.UTF-8` over every byte in three
//! positions, over whole multi-byte characters chosen to straddle every edge
//! of "printable", over sequences that are not valid UTF-8 at all, and over a
//! corpus of interacting shapes, and records what GNU printed. This test
//! replays the recording.
//!
//! The locale is `C.UTF-8` and not `C` because since `design-decisions.md`
//! §351 osh's string layer is UTF-8 in every locale; under plain `C` gnulib
//! does not decode at all, so the *reference* would be answering a byte
//! question where ours answers a character one.
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

/// Whether `input` holds `c`'s UTF-8 encoding.
///
/// Bytewise, because `input` need not be valid UTF-8 — and a false positive is
/// impossible anyway: a multi-byte encoding's bytes are all ≥ 0x80 and its
/// lead byte cannot appear as a continuation, so the sequence occurs only
/// where the character does.
fn contains_char(input: &[u8], c: char) -> bool {
    let mut buf = [0u8; 4];
    let needle = c.encode_utf8(&mut buf).as_bytes();
    input.windows(needle.len()).any(|w| w == needle)
}

/// Characters on which we differ from GNU **on purpose**, and the reason.
///
/// The same single entry `tests/quotearg.rs` carries, and for the same reason:
/// GNU asks glibc's `iswprint`, a table generated from that release's
/// `UnicodeData.txt`, while we ask a rule that needs no table — see
/// `design-decisions.md` §357 and `coreutils::quote::printable_char`. The two
/// coincide everywhere except **unassigned** code points, which glibc escapes
/// and we print as themselves.
///
/// Every row whose input holds one of these must *keep* differing: a recorded
/// reason that has quietly stopped applying is worse than a difference,
/// because it trains the next reader to trust a stale note.
const EXPECTED_DIVERGENCE: &[(char, &str)] = &[(
    '\u{0378}',
    "unassigned (Cn); glibc's table escapes it, our rule prints it",
)];

#[test]
fn matches_gnu_coreutils_row_for_row() {
    let mut checked = 0usize;
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut wrong = Vec::new();
    // Per divergent character: how many rows exercised it, and any that came
    // out *agreeing* with GNU, which means the note above has gone stale.
    let mut divergent: std::collections::HashMap<char, (usize, Vec<String>)> =
        std::collections::HashMap::new();
    for (lineno, line) in FIXTURE.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut fields = line.split(' ');
        let style = fields.next().expect("style");
        let input = unhex(fields.next().expect("input"));
        let want = String::from_utf8(unhex(fields.next().expect("output"))).expect("utf-8");
        assert!(fields.next().is_none(), "line {}: extra field", lineno + 1);

        let got = match style {
            "c-maybe" => quote_c_maybe(&input),
            "c-maybe-colon" => quote_c_maybe_colon(&input),
            other => panic!("line {}: unknown style {other:?}", lineno + 1),
        };
        checked += 1;
        *seen.entry(style).or_default() += 1;

        // A row about a deliberately-divergent character is judged by the
        // opposite rule from every other row.
        if let Some(&(c, _)) = EXPECTED_DIVERGENCE
            .iter()
            .find(|(c, _)| contains_char(&input, *c))
        {
            let entry = divergent.entry(c).or_default();
            entry.0 += 1;
            if got == want && entry.1.len() < 5 {
                entry.1.push(format!(
                    "  line {}: {style}({input:?}) now agrees: {got}",
                    lineno + 1
                ));
            }
            continue;
        }

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
    // An expected divergence that no row exercises is a claim with no evidence
    // behind it, and one that has stopped happening is a stale reason. Both
    // fail, for the same purpose: the list must describe reality.
    for &(c, why) in EXPECTED_DIVERGENCE {
        let (rows, agreed) = divergent.remove(&c).unwrap_or_default();
        assert!(
            rows > 0,
            "U+{:04X} is listed as an expected divergence ({why}) but no fixture \
             row exercises it -- widen scripts/c-maybe-probe.py or drop the entry",
            u32::from(c)
        );
        assert!(
            agreed.is_empty(),
            "U+{:04X} was expected to differ from GNU ({why}) but {} of its {rows} rows \
             now agree, so the recorded reason is stale:\n{}",
            u32::from(c),
            agreed.len(),
            agreed.join("\n")
        );
    }
    // A fixture that silently shrank would let this pass while testing
    // nothing. The floor sits under the real count (1828) so that adding a
    // probe case is not also a test edit.
    assert!(checked > 1700, "fixture only had {checked} rows");
    for style in ["c-maybe", "c-maybe-colon"] {
        assert!(
            *seen.get(style).unwrap_or(&0) > 800,
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
