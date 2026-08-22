//! `quote` / `quotef` / `quoteaf` against GNU coreutils, row for row.
//!
//! The expectations are not written by hand and are not derived from reading
//! gnulib. They were *measured*: `scripts/quote-probe.py` runs GNU `sort` and
//! `head` under `LC_ALL=C.UTF-8` over every byte in every position that
//! matters, over a set of whole multi-byte characters chosen to straddle every
//! edge of "printable", over sequences that are not valid UTF-8 at all, and
//! over an adversarial and a random corpus, and records what GNU printed. This
//! test replays that recording.
//!
//! The locale is `C.UTF-8` and not `C` because since `design-decisions.md`
//! §351 `quote()` prints curly marks in every locale, so `C` — where GNU's are
//! straight — is the setting in which the *reference* would be wrong.
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
/// GNU asks glibc's `iswprint`, which under `C.UTF-8` is a 709-range table
/// generated from that release's `UnicodeData.txt`. We ask a rule — "not a
/// Unicode control, not a line/paragraph separator" — that needs no table and
/// so cannot drift when Unicode assigns a code point. See
/// `design-decisions.md` §357 and `coreutils::quote::printable_char`.
///
/// The two answers coincide everywhere except **unassigned** code points
/// (`Cn`), which glibc escapes and we print as themselves. A name holding one
/// is a name holding a character no font can draw and no standard has given a
/// meaning, so neither answer is wrong; ours is the one that does not need a
/// table.
///
/// Every row whose input holds one of these must *keep* differing. A recorded
/// reason that has quietly stopped applying is worse than a difference — it
/// trains the next reader to trust a stale note — so an agreement here fails
/// the test just as a disagreement elsewhere does.
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
            "quote" => quote(&input),
            "quotef" => quotef(&input),
            "quoteaf" => quoteaf(&input),
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
             row exercises it -- widen scripts/quote-probe.py or drop the entry",
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
    // A fixture that silently shrank to nothing would let this pass while
    // testing nothing at all. The floor is a little under the real count
    // (8719) so that adding a probe case is not also a test edit, while a
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
