//! `modechange` against measured GNU `chmod` output.
//!
//! The unit tests in `src/lib.rs` check the rules the module is *supposed* to
//! implement, one rule at a time, with expectations written by the same hand
//! that wrote the code. This file checks something weaker and much harder to
//! fool: that over a cross product of mode strings, starting modes, umasks and
//! file kinds, the module returns bit for bit what GNU coreutils 9.4 actually
//! did to a real file.
//!
//! That distinction is not theoretical here. Three of the rules in
//! `modechange.c` are invisible to any test whose expectations come from
//! reading it:
//!
//! * `affected == 0` does not mean "all three" — it means "ask the umask", and
//!   the difference only shows up when the umask is not `000`. Every one of the
//!   four hand-written parsers this crate replaces got that wrong, and every
//!   one of them passed its own tests.
//! * A short octal (`755`) leaves setuid and setgid alone on a directory where
//!   a long one (`00755`) clears them. On a regular file the two are
//!   indistinguishable, so a fixture that measured only files would certify the
//!   bug.
//! * An unrecognised permission letter does not fail where it is found; it ends
//!   the clause, and the failure happens later, at the end of the string. Get
//!   that backwards and `u+r-w` — which is legal — is rejected, while nothing
//!   in a rule-by-rule test notices.
//!
//! The table is committed rather than measured live, for the reason recorded in
//! design-decisions.md §338: the machine that runs the tests is not the machine
//! that has GNU coreutils on it. See `scripts/gen-chmod-fixture.sh` for how it
//! is produced, and why it must run as root.

use modechange::{adjust, compile};

/// The measured table, embedded at compile time so the test does not care what
/// the working directory is and does not need a GNU userland present.
const FIXTURE: &str = include_str!("data/gnu-chmod.txt");

/// One row of the fixture, resolved into the call it stands for.
struct Case {
    /// The line as written, for the failure message.
    raw: &'static str,
    /// The mode the file actually had when `chmod` was invoked — read back with
    /// `stat`, not assumed, so a bit the kernel declined to store yields a
    /// different but still correct row.
    start: u32,
    umask: u32,
    dir: bool,
    /// `chmod`'s exit status: 0 accepted the mode string, 1 rejected it.
    status: u32,
    /// The mode the file had afterwards. Meaningless when `status` is 1, since
    /// `chmod` then changed nothing.
    result: u32,
    /// The mode string, as bytes — the fixture's last column, with the
    /// generator's `<empty>` sentinel resolved back to the empty string.
    spec: &'static [u8],
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
    let (start, rest) = token(line.trim_start())?;
    let (umask, rest) = token(rest.trim_start())?;
    let (kind, rest) = token(rest.trim_start())?;
    let (status, rest) = token(rest.trim_start())?;
    let (result, rest) = token(rest.trim_start())?;
    // The spec is the rest of the line rather than a sixth token only so that a
    // trailing space in the fixture would be preserved; the generator refuses
    // to emit one, since a mode string containing a space is invalid anyway.
    let spec = rest.trim_start();

    Some(Case {
        raw: line,
        start: u32::from_str_radix(start, 8).ok()?,
        umask: u32::from_str_radix(umask, 8).ok()?,
        dir: match kind {
            "d" => true,
            "f" => false,
            _ => return None,
        },
        status: status.parse().ok()?,
        result: u32::from_str_radix(result, 8).ok()?,
        spec: if spec == "<empty>" {
            b""
        } else {
            spec.as_bytes()
        },
    })
}

#[test]
fn matches_gnu_on_every_measured_row() {
    let mut checked = 0usize;
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut dirs = 0usize;
    let mut nonzero_umask = 0usize;
    let mut failures = Vec::new();

    for line in FIXTURE.lines() {
        let Some(case) = parse(line) else { continue };
        checked += 1;
        if case.status == 0 {
            accepted += 1;
        } else {
            rejected += 1;
        }
        if case.dir {
            dirs += 1;
        }
        if case.umask != 0 {
            nonzero_umask += 1;
        }

        let compiled = compile(case.spec);
        let complaint = match (&compiled, case.status) {
            (Some(_), 0) => {
                // Both agree the string is a mode. Now agree on what it means.
                let Some(changes) = compiled.as_ref() else {
                    unreachable!("matched Some")
                };
                let got = adjust(case.start, case.dir, case.umask, changes).mode;
                if got == case.result {
                    None
                } else {
                    Some(format!("GNU: {:04o}  ours: {got:04o}", case.result))
                }
            }
            (None, 0) => Some("GNU accepted this mode string; we rejected it".to_owned()),
            (Some(_), _) => Some("GNU rejected this mode string; we accepted it".to_owned()),
            (None, _) => None,
        };
        if let Some(complaint) = complaint {
            // Cap the report: a systematic error produces thousands of rows,
            // and the first handful say everything the rest would.
            if failures.len() < 25 {
                failures.push(format!("  {}\n      {complaint}", case.raw));
            }
        }
    }

    // A fixture that failed to generate is an empty file, and an empty file is
    // a passing test unless the count is asserted. The same statement is made
    // twice on purpose — once by the generator before it will install a
    // fixture, once here at read time — because the failure it guards against,
    // a sweep that silently measured almost nothing, looks like success from
    // both sides.
    assert!(
        checked >= 20_000,
        "fixture looks truncated: only {checked} rows parsed \
         (regenerate with scripts/gen-chmod-fixture.sh under a GNU userland)"
    );
    // Each of these four is a whole axis of the cross product, and each has a
    // rule that lives only on it: rejection has the trailing-garbage rule,
    // directories have the setuid/setgid preservation rule and `X`, and a
    // non-zero umask is the only place `affected == 0` differs from `a`. A
    // fixture missing one of them is a fixture that certifies the bug.
    assert!(
        accepted > 0 && rejected > 0 && dirs > 0 && nonzero_umask > 0,
        "fixture is missing a whole axis: accepted={accepted} rejected={rejected} \
         dirs={dirs} nonzero_umask={nonzero_umask}"
    );

    assert!(
        failures.is_empty(),
        "{} of {checked} measured rows disagree with GNU (showing at most 25):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
