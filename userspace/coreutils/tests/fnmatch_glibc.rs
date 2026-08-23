//! [`coreutils::fnmatch`] against glibc's `fnmatch(3)`, case for case.
//!
//! The expectations are not written by hand and are not derived from reading
//! glibc's source. They were *measured*: `scripts/fnmatch-probe.c` calls the
//! real `fnmatch(3)` over a cross product of 137 patterns, 70 names and 11 flag
//! sets — about 105,000 cases — and records what it answered. This test replays
//! that recording.
//!
//! The patterns are chosen to reach the parts of the grammar that a hand-written
//! matcher gets wrong: every bracket oddity (a leading `]` as a member, `-` at
//! either end, an inverted range, a class beside a literal, an unknown class, a
//! collating symbol, an unterminated `[`), escapes including a trailing
//! backslash and a backslash inside a bracket, and the interactions between `*`,
//! `/` and a leading `.` that [`Flags::PATHNAME`] and [`Flags::PERIOD`] govern.
//! The names include bytes that are not valid UTF-8, which is the case our own
//! filesystem makes ordinary and which no `char`-based matcher can even be
//! handed.
//!
//! Recording rather than shelling out to glibc at test time is the same
//! decision `quotearg.rs` documents: the development host is Windows, the test
//! must not go quiet when a tool is missing, and a fixture is diffable — a row
//! that changes is a finding to look at, not a nuisance to re-baseline.
//!
//! To refresh it (a new glibc, or a rule found to be under-covered):
//!
//! ```text
//! wsl -e sh -c 'gcc -O2 -o /tmp/fnprobe scripts/fnmatch-probe.c && /tmp/fnprobe' \
//!     > userspace/coreutils/tests/fnmatch-glibc.txt
//! ```
//!
//! Where we differ from glibc on purpose, [`DELIBERATE`] says so and why; every
//! other difference is a bug in [`coreutils::fnmatch`].

use coreutils::fnmatch::{Flags, fnmatch};

const FIXTURE: &str = include_str!("fnmatch-glibc.txt");

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

/// Patterns whose glibc answer we do not reproduce, and the reason.
///
/// Both entries are the same decision, documented in the module docs of
/// [`coreutils::fnmatch`]: a collating symbol `[.x.]` and an equivalence class
/// `[=x=]` are rejected as a malformed bracket rather than parsed. glibc parses
/// them and, in the C locale, reduces each to the single character between the
/// delimiters. No caller in this tree writes one, and refusing is the safer of
/// the two ways to be wrong — a pattern that uses one fails to match rather than
/// matching a set of punctuation the author did not intend.
///
/// The test asserts these rows *do* differ, so deleting the feature's absence
/// without deleting this list fails rather than passing quietly.
const DELIBERATE: &[&str] = &["[[.a.]]", "[[=a=]]", "[[.hyphen.]]", "[[.a.]b]"];

struct Case {
    flags: u32,
    pattern: Vec<u8>,
    name: Vec<u8>,
    glibc: bool,
}

fn cases() -> Vec<Case> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::new();

    for line in FIXTURE.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut field = line.split('\t');
        match field.next() {
            Some("N") => names.push(unhex(field.next().expect("N row has a name"))),
            Some("M") => {
                let flags: u32 = field
                    .next()
                    .expect("M row has flags")
                    .parse()
                    .expect("flags are a number");
                let pattern = unhex(field.next().expect("M row has a pattern"));
                let bitmap = field.next().expect("M row has a bitmap");
                assert_eq!(
                    bitmap.len(),
                    names.len().div_ceil(4),
                    "bitmap does not cover the name list"
                );
                for (index, name) in names.iter().enumerate() {
                    let nibble = bitmap
                        .as_bytes()
                        .get(index / 4)
                        .map(|&b| char::from(b).to_digit(16).expect("hex nibble"))
                        .expect("bitmap covers every name");
                    out.push(Case {
                        flags,
                        pattern: pattern.clone(),
                        name: name.clone(),
                        glibc: nibble & (1 << (index % 4)) != 0,
                    });
                }
            }
            other => panic!("unknown fixture row {other:?}"),
        }
    }
    assert!(out.len() > 100_000, "fixture shrank to {} cases", out.len());
    out
}

fn show(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn matches_glibc_on_every_recorded_case() {
    let mut differed = Vec::new();
    let mut deliberate_seen = 0_usize;

    for case in cases() {
        let ours = fnmatch(&case.pattern, &case.name, Flags::from_bits(case.flags));
        let excused = DELIBERATE
            .iter()
            .any(|&text| text.as_bytes() == case.pattern.as_slice());
        if ours == case.glibc {
            assert!(
                !excused || ours == case.glibc,
                "unreachable: agreement is never a failure"
            );
            continue;
        }
        if excused {
            deliberate_seen = deliberate_seen.saturating_add(1);
            continue;
        }
        differed.push(format!(
            "flags {} pattern {:?} name {:?}: glibc {}, ours {}",
            case.flags,
            show(&case.pattern),
            show(&case.name),
            case.glibc,
            ours
        ));
    }

    assert!(
        deliberate_seen > 0,
        "no row differed on a collating symbol or equivalence class — either \
         they are now parsed (delete them from DELIBERATE) or the fixture no \
         longer covers them"
    );
    assert!(
        differed.is_empty(),
        "{} of the recorded cases disagree with glibc:\n{}",
        differed.len(),
        differed.join("\n")
    );
}
