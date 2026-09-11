//! How long a terminal has been idle — or that we cannot tell.
//!
//! # Why this crate exists
//!
//! `w`, `finger`, `pinky` and `who -u` all print an IDLE column, and between
//! them there were two answers to how it is computed:
//!
//! | Program | Idle time |
//! |---|---|
//! | `who` | the mtime of `/dev/<tty>`, subtracted from now |
//! | `userspace/w` (and its `finger`, `pinky`) | **the literal `0`, always** |
//!
//! `w`'s `UtmpEntry` set `idle_secs: 0` where it was built and nothing ever
//! assigned it again. Its formatter renders 0 as `"  .  "`, which in `w` means
//! *active right now* — so every user was reported as active, always,
//! including one who had been away for three hours. A column that reports a
//! value it never measured is the defect `design-decisions.md` §1006 is
//! about; it is just wearing a table.
//!
//! # Why the answer is an `Option`
//!
//! `who`'s version returned `0` for "the terminal was touched this second"
//! AND for "there is no such device", "the mtime is unreadable" and "the name
//! is not something I can turn into a path". Those are a measurement and three
//! admissions of ignorance sharing one value, and the display renders that
//! value as *active now* — the most reassuring of the four.
//!
//! `None` means we could not find out. Callers print `?`, which is what every
//! other unknown in these programs prints, rather than the most flattering
//! number available. Same rule as the read-defaults and absent-operand
//! ledgers: for a value a person reads to make a decision, "I do not know" and
//! "nothing to report" must not be the same answer.

#![forbid(unsafe_code)]

use std::fs;
use std::time::SystemTime;

/// Seconds since `tty` was last written to, or `None` if that cannot be found
/// out.
///
/// The terminal's device node carries the time of the last write, which is the
/// standard way to measure this and what `w` has always done.
///
/// `None` is returned when the name is empty or `?` (utmp's own "unknown"),
/// when the name is not UTF-8 and so cannot be made into a path on this build,
/// when there is no such device, and when its mtime cannot be read. All four
/// are "we cannot tell", and none of them is "zero seconds".
///
/// A device whose mtime is in the future gives `Some(0)` rather than `None`:
/// the file was found and read, so this is a measurement — of a clock that
/// disagrees with itself — and not an absence of one.
#[must_use]
pub fn idle_secs(tty: &[u8], now: u64) -> Option<u64> {
    if tty.is_empty() || tty == b"?" {
        return None;
    }
    // A terminal we cannot name cannot be stat'd. On the real target every
    // name is bytes; this build makes a path from a `str`, so a name outside
    // UTF-8 is one we cannot ask about rather than one that is idle for zero
    // seconds.
    let name = str::from_utf8(tty).ok()?;

    // `Path::is_absolute` rather than `starts_with('/')`: the latter is a
    // POSIX assumption, and it made this prepend `/dev/` to an already-absolute
    // host path, which the crate's own test caught on the first run. utmp
    // records a bare name like `pts/0`, and `who`'s other sources can record a
    // full path, so both forms really do arrive here.
    let dev_path = if std::path::Path::new(name).is_absolute() {
        name.to_string()
    } else {
        format!("/dev/{name}")
    };

    let mtime = fs::metadata(&dev_path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();

    Some(now.saturating_sub(mtime))
}

#[cfg(test)]
// `expect` on a path the test itself just obtained: if the test binary has no
// path, the test cannot run at all, and panicking says so. CLAUDE.md allows
// these in test modules for exactly this case.
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_name_utmp_could_not_supply_is_unknown_not_zero() {
        // utmp writes "?" when it has no line for a session, and `who`'s
        // fallback source writes it too. Reporting that as zero seconds idle
        // renders as "active now" -- a claim about somebody we know nothing
        // about.
        assert_eq!(idle_secs(b"", 1000), None);
        assert_eq!(idle_secs(b"?", 1000), None);
    }

    #[test]
    fn a_name_that_will_not_make_a_path_is_unknown() {
        assert_eq!(idle_secs(b"tty\xff\xfe1", 1000), None);
    }

    #[test]
    fn a_device_that_does_not_exist_is_unknown() {
        // The important half: this used to be 0, which the formatter prints
        // as "." -- active now -- for a terminal that is not there at all.
        assert_eq!(idle_secs(b"definitely-not-a-tty-93f1", 1000), None);
        assert_eq!(idle_secs(b"/dev/definitely-not-a-tty-93f1", 1000), None);
    }

    #[test]
    fn a_real_file_gives_a_measurement() {
        // Uses a file that exists on every platform this builds for, named
        // absolutely so the /dev prefix is not applied. The value is whatever
        // its mtime implies; the point is that it is Some.
        let exe = std::env::current_exe().expect("the test binary exists");
        let path = exe.to_str().expect("the test binary path is UTF-8");
        assert!(
            idle_secs(path.as_bytes(), u64::MAX).is_some(),
            "an existing file must yield a measurement, not None"
        );
    }

    #[test]
    fn a_clock_that_disagrees_with_itself_is_still_a_measurement() {
        // `now` before the mtime used to underflow or be special-cased to 0.
        // Saturating gives 0, and it is Some(0) -- we looked, and the answer
        // is "not idle" -- rather than None, which would claim we could not
        // look. The distinction is the whole point of this crate.
        let exe = std::env::current_exe().expect("the test binary exists");
        let path = exe.to_str().expect("the test binary path is UTF-8");
        assert_eq!(idle_secs(path.as_bytes(), 0), Some(0));
    }
}
