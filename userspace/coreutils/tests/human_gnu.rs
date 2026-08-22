//! `coreutils::human` against measured GNU output.
//!
//! The unit tests in `src/human.rs` check the rules the module is *supposed*
//! to implement — the decimal disappearing at ten, the carry out of the top of
//! a scale, the space belonging to the unit. This file checks something
//! weaker and more useful: that on several thousand byte counts the module
//! agrees, character for character, with what GNU coreutils actually printed.
//!
//! That distinction earned its keep. gnulib's `human_readable` makes two
//! rounding decisions twelve lines apart (`human.c:301` and `human.c:327`)
//! which are written almost identically and are *not* the same test — one
//! compares against 2 and the other against 0. Transcribing the first over the
//! second passes every hand-written rule test above, and moves the SI carry by
//! 450 bytes: 999500 renders as `999 kB` instead of `1.0 MB`. Only a sweep
//! finds that, and only if the sweep's expectations come from GNU rather than
//! from the same misreading that produced the code.
//!
//! The table is committed rather than measured live; see
//! `scripts/gen-human-fixture.sh` for how it is produced and why it takes
//! three different GNU utilities to produce it.

use coreutils::human::{Opts, human_readable};

/// The measured table, embedded at compile time so the test does not care
/// what the working directory is and does not need GNU coreutils present.
const FIXTURE: &str = include_str!("data/human-gnu.txt");

/// One row of the fixture, resolved into the call it stands for.
struct Case {
    /// The line as written, for the failure message.
    raw: &'static str,
    /// Which instrument produced the row — kept so the test can assert that
    /// all three are represented rather than re-splitting `raw` to find out.
    style: &'static str,
    n: u64,
    opts: Opts,
    to_block_size: u64,
    expected: &'static str,
}

/// Peel one whitespace-delimited token off the front, returning it and the
/// unconsumed remainder.
fn token(s: &str) -> Option<(&str, &str)> {
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((s.get(..end)?, s.get(end..)?))
}

fn parse(line: &'static str) -> Option<Case> {
    let line = line.trim_end_matches('\r');
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // The rendering is *the rest of the line*, not a fourth field: `dd`'s
    // columns contain a space (`1.0 MB`). So the first three fields are peeled
    // off positionally and whatever follows is taken whole. Searching the line
    // for the fourth token instead would be wrong — `find` returns the first
    // match, and a rendering like `1` occurs inside the byte count that
    // precedes it.
    let (style, rest) = token(line.trim_start())?;
    let (base, rest) = token(rest.trim_start())?;
    let (n, rest) = token(rest.trim_start())?;
    let base: u64 = base.parse().ok()?;
    let n: u64 = n.parse().ok()?;
    let expected = rest.trim();
    if expected.is_empty() {
        return None;
    }

    let (opts, to_block_size) = match style {
        // `ls -h` / `ls --si`: autoscale, ceiling, suffix letters, from=to=1.
        "ceil" => {
            let mut o = Opts::AUTOSCALE | Opts::CEILING | Opts::SI;
            if base == 1024 {
                o = o | Opts::BASE_1024;
            }
            (o, 1)
        }
        // `du --apparent-size -B N`: no autoscale, ceiling, and `base` is
        // standing in for the output block size rather than for a radix.
        "ceilblk" => (Opts::CEILING, base),
        // `dd`: the only round-to-nearest caller in coreutils.
        "near" => {
            let mut o = Opts::AUTOSCALE
                | Opts::ROUND_TO_NEAREST
                | Opts::SPACE_BEFORE_UNIT
                | Opts::SI
                | Opts::B;
            if base == 1024 {
                o = o | Opts::BASE_1024;
            }
            (o, 1)
        }
        _ => return None,
    };

    Some(Case {
        raw: line,
        style,
        n,
        opts,
        to_block_size,
        expected,
    })
}

#[test]
fn matches_gnu_on_every_measured_row() {
    let mut checked = 0usize;
    let mut styles = [0usize; 3];
    let mut failures = Vec::new();

    for line in FIXTURE.lines() {
        let Some(case) = parse(line) else { continue };
        match case.style {
            "ceil" => styles[0] += 1,
            "ceilblk" => styles[1] += 1,
            _ => styles[2] += 1,
        }
        let got = human_readable(case.n, case.opts, 1, case.to_block_size);
        if got != case.expected {
            // Cap the report: a systematic error produces thousands of rows,
            // and the first handful say everything the rest would.
            if failures.len() < 25 {
                failures.push(format!(
                    "  {}\n      GNU: {:?}\n      ours: {:?}",
                    case.raw, case.expected, got
                ));
            }
        }
        checked += 1;
    }

    // A fixture that failed to generate is an empty file, and an empty file is
    // a passing test unless the count is asserted. The floor matches the one
    // the generator enforces before it will install a fixture at all; the two
    // are deliberately the same statement made twice, once at write time and
    // once at read time, because the failure they guard against — a sweep that
    // silently measured almost nothing — looks like success from both sides.
    assert!(
        checked >= 20_000,
        "fixture looks truncated: only {checked} rows parsed \
         (regenerate with scripts/gen-human-fixture.sh)"
    );
    assert!(
        styles[0] > 0 && styles[1] > 0 && styles[2] > 0,
        "fixture is missing a whole style: ceil={} ceilblk={} near={}",
        styles[0],
        styles[1],
        styles[2]
    );

    assert!(
        failures.is_empty(),
        "{} of {checked} measured renderings disagree with GNU \
         (showing at most 25):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
