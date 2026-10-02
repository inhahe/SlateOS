//! Signal names and numbers, converted the way gnulib's `sig2str.c` converts
//! them.
//!
//! Every GNU utility that names a signal -- `split --filter` reporting the
//! signal that ended its command, `timeout -s`, `env --list-signal-handling`,
//! coreutils' `kill` -- goes through that one gnulib file, so they agree with
//! each other about spelling, and the agreement is observable in three ways
//! that a hand-written table gets wrong:
//!
//! * **The first name in the table wins.** Several names share a number on
//!   Linux, and gnulib lists POSIX's base signals first, then XSI's, then
//!   older Unix's, then Linux's own. So 29 is `POLL` (XSI), not `IO` (Linux);
//!   6 is `ABRT`, not `IOT`; 17 is `CHLD`, not `CLD`. Every one of those names
//!   still parses to its number.
//! * **Real-time signals are named from both ends.** The lower half of the
//!   range counts up from `RTMIN` and the upper half down from `RTMAX`, so
//!   with glibc's range of 34 to 64 the names run `RTMIN`, `RTMIN+1`, ...,
//!   `RTMIN+15`, `RTMAX-14`, ..., `RTMAX-1`, `RTMAX`. The range is the C
//!   library's, asked at run time through [`libcall::sigrtmin`]: glibc keeps
//!   32 and 33 for its threads and SlateOS's library keeps none, so one number
//!   has a different name on the two systems -- exactly as it does for a GNU
//!   binary built for each.
//! * **0 is `EXIT`**, the shells' name for the trap that runs at exit.
//!
//! Measured against GNU `kill -l NAME` and `kill -l NUMBER` (coreutils 9.4 on
//! glibc 2.39), which are this file's two functions with nothing in between.
//!
//! # Which names exist
//!
//! gnulib compiles an entry only `#ifdef SIGxxx`, so the table is the
//! platform's. These are the names glibc defines on x86-64 Linux, whose
//! numbering SlateOS's POSIX layer follows (`posix/src/signal.rs`). Measured,
//! not assumed: `LOST`, `UNUSED`, `EMT`, `INFO`, `CANCEL`, `THR` and `BREAK`
//! are all refused there as invalid signals.

/// gnulib's `numname_table`, in its order -- which is the point: see the module
/// docs for why the first entry for a number is the one that names it.
const TABLE: &[(i32, &str)] = &[
    // POSIX 1003.1-2001 base, "in traditional numeric order where possible".
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (4, "ILL"),
    (5, "TRAP"),
    (6, "ABRT"),
    (8, "FPE"),
    (9, "KILL"),
    (11, "SEGV"),
    // After SEGV on purpose: on Haiku the two are one number, and gnulib
    // prefers SEGV there.
    (7, "BUS"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (10, "USR1"),
    (12, "USR2"),
    (17, "CHLD"),
    (23, "URG"),
    (19, "STOP"),
    (20, "TSTP"),
    (18, "CONT"),
    (21, "TTIN"),
    (22, "TTOU"),
    // POSIX 1003.1-2001 with the XSI extension.
    (31, "SYS"),
    (29, "POLL"),
    (26, "VTALRM"),
    (27, "PROF"),
    (24, "XCPU"),
    (25, "XFSZ"),
    // Unix Version 7: the older name for ABRT.
    (6, "IOT"),
    // Unix System V.
    (17, "CLD"),
    (30, "PWR"),
    // GNU/Linux 2.2 and Solaris 8.
    (28, "WINCH"),
    // GNU/Linux 2.2.
    (29, "IO"),
    (16, "STKFLT"),
    // "Korn shell and Bash, of uncertain vintage."
    (0, "EXIT"),
];

/// The largest number a signal can have here: gnulib's `SIGNUM_BOUND`, which is
/// `NSIG - 1` -- 64 under glibc and under SlateOS's library alike.
pub const SIGNUM_BOUND: i32 = 64;

/// The name of signal `signum`, without the `SIG`: gnulib's `sig2str`.
///
/// `None` for a number that is no signal's here, which a caller turns into its
/// own wording -- `split` prints the number instead, `kill` refuses it.
#[must_use]
pub fn sig2str(signum: i32) -> Option<String> {
    if let Some((_, name)) = TABLE.iter().find(|(n, _)| *n == signum) {
        return Some((*name).to_string());
    }
    real_time_name(signum, libcall::sigrtmin(), libcall::sigrtmax())
}

/// The number signal `name` stands for: gnulib's `str2sig`.
///
/// `name` is matched as it is, so it must already be upper case and without a
/// `SIG` prefix; a caller that accepts `sigterm` or `SIGTERM` (`kill`,
/// `timeout`) folds and strips first, as coreutils' `operand2sig.c` does.
/// Digits are a number in their own right, up to [`SIGNUM_BOUND`].
#[must_use]
pub fn str2sig(name: &[u8]) -> Option<i32> {
    if name.first().is_some_and(u8::is_ascii_digit) {
        let (n, used) = strtol(name);
        return if used == name.len() && n <= i64::from(SIGNUM_BOUND) {
            i32::try_from(n).ok()
        } else {
            None
        };
    }
    if let Some((num, _)) = TABLE.iter().find(|(_, n)| n.as_bytes() == name) {
        return Some(*num);
    }
    real_time_number(name, libcall::sigrtmin(), libcall::sigrtmax())
}

/// [`sig2str`] for a real-time signal, against the range `rtmin..=rtmax`.
///
/// The halves split at `rtmin + (rtmax - rtmin) / 2`, which belongs to the
/// lower half: with 34..=64 that is 49, `RTMIN+15`, and 50 is `RTMAX-14`.
fn real_time_name(signum: i32, rtmin: i32, rtmax: i32) -> Option<String> {
    if !(rtmin <= signum && signum <= rtmax) {
        return None;
    }
    let middle = rtmin.checked_add(rtmax.checked_sub(rtmin)?.checked_div(2)?)?;
    let (stem, base) = if signum <= middle {
        ("RTMIN", rtmin)
    } else {
        ("RTMAX", rtmax)
    };
    Some(match signum.checked_sub(base)? {
        0 => stem.to_string(),
        delta => format!("{stem}{delta:+}"),
    })
}

/// [`str2sig`] for `RTMIN[n]` and `RTMAX[n]`, against the range
/// `rtmin..=rtmax`.
///
/// The offset is read by C's `strtol`, so it may be signed, may follow white
/// space, and may be absent -- `RTMIN` is `RTMIN+0`, and, measured, `RTMIN1`
/// is `RTMIN+1`. It must lie inside the range: `RTMIN-1` and `RTMAX+1` are
/// refused. A system with no real-time signals has `rtmin` 0 and
/// `rtmax` -1, and refuses both stems outright.
fn real_time_number(name: &[u8], rtmin: i32, rtmax: i32) -> Option<i32> {
    let (rtmin64, rtmax64) = (i64::from(rtmin), i64::from(rtmax));
    let (offset, lowest, highest, base) = if 0 < rtmin
        && let Some(rest) = name.strip_prefix(b"RTMIN")
    {
        (rest, 0, rtmax64.checked_sub(rtmin64)?, rtmin64)
    } else if 0 < rtmax
        && let Some(rest) = name.strip_prefix(b"RTMAX")
    {
        (rest, rtmin64.checked_sub(rtmax64)?, 0, rtmax64)
    } else {
        return None;
    };
    let (n, used) = strtol(offset);
    if used != offset.len() || n < lowest || n > highest {
        return None;
    }
    i32::try_from(base.checked_add(n)?).ok()
}

/// C's `strtol (s, &end, 10)`, returning the value and how many bytes of `s`
/// it used.
///
/// Leading white space (C's `isspace`, vertical tab and form feed included)
/// and one sign are allowed; with no digits after them nothing is used at
/// all, so `end` is `s` itself, as C has it. Out-of-range values saturate, as
/// C's do -- every caller here compares the result against a small bound, so
/// where in the saturated region a value lands cannot matter.
fn strtol(s: &[u8]) -> (i64, usize) {
    let space = |b: &&u8| matches!(**b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let mut at = s.iter().take_while(space).count();
    let negative = match s.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };
    let digits = s
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 {
        return (0, 0);
    }
    let end = at.saturating_add(digits);
    let magnitude = s
        .get(at..end)
        .unwrap_or_default()
        .iter()
        .fold(0_i64, |n, d| {
            n.saturating_mul(10)
                .saturating_add(i64::from(d.saturating_sub(b'0')))
        });
    (
        if negative {
            magnitude.saturating_neg()
        } else {
            magnitude
        },
        end,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// glibc's range, which the measurements in this file were taken against.
    const GLIBC: (i32, i32) = (34, 64);

    #[test]
    fn every_number_below_thirty_two_has_gnu_s_name() {
        // `kill -l 1` ... `kill -l 31` from GNU coreutils 9.4, in order.
        let gnu = "HUP INT QUIT ILL TRAP ABRT BUS FPE KILL USR1 SEGV USR2 PIPE \
                   ALRM TERM STKFLT CHLD CONT STOP TSTP TTIN TTOU URG XCPU XFSZ \
                   VTALRM PROF WINCH POLL PWR SYS";
        let ours: Vec<String> = (1..=31).map(|n| sig2str(n).unwrap()).collect();
        assert_eq!(ours.join(" "), gnu);
    }

    #[test]
    fn the_first_name_for_a_number_is_the_one_it_is_called() {
        assert_eq!(sig2str(29).as_deref(), Some("POLL"));
        assert_eq!(sig2str(6).as_deref(), Some("ABRT"));
        assert_eq!(sig2str(17).as_deref(), Some("CHLD"));
        assert_eq!(sig2str(0).as_deref(), Some("EXIT"));
    }

    #[test]
    fn every_alias_still_parses() {
        for (name, num) in [
            ("IO", 29),
            ("POLL", 29),
            ("IOT", 6),
            ("ABRT", 6),
            ("CLD", 17),
            ("CHLD", 17),
            ("STKFLT", 16),
            ("PWR", 30),
            ("EXIT", 0),
        ] {
            assert_eq!(str2sig(name.as_bytes()), Some(num), "{name}");
        }
    }

    #[test]
    fn a_name_glibc_does_not_define_is_no_signal() {
        for name in ["LOST", "UNUSED", "EMT", "INFO", "CANCEL", "THR", "BREAK"] {
            assert_eq!(str2sig(name.as_bytes()), None, "{name}");
        }
    }

    #[test]
    fn the_table_is_matched_exactly_case_and_all() {
        // Folding case and stripping `SIG` is the caller's job.
        assert_eq!(str2sig(b"term"), None);
        assert_eq!(str2sig(b"SIGTERM"), None);
        assert_eq!(str2sig(b"TERM "), None);
        assert_eq!(str2sig(b""), None);
    }

    #[test]
    fn digits_are_a_number_up_to_the_bound() {
        assert_eq!(str2sig(b"9"), Some(9));
        assert_eq!(str2sig(b"007"), Some(7));
        assert_eq!(str2sig(b"0"), Some(0));
        assert_eq!(str2sig(b"64"), Some(64));
        assert_eq!(str2sig(b"65"), None);
        assert_eq!(str2sig(b"9x"), None);
        assert_eq!(str2sig(b"99999999999999999999999"), None);
    }

    #[test]
    fn real_time_names_count_from_both_ends() {
        let (lo, hi) = GLIBC;
        let name = |n| real_time_name(n, lo, hi);
        // `kill -l 34`, `35`, `49`, `50`, `64` under glibc.
        assert_eq!(name(34).as_deref(), Some("RTMIN"));
        assert_eq!(name(35).as_deref(), Some("RTMIN+1"));
        assert_eq!(name(49).as_deref(), Some("RTMIN+15"));
        assert_eq!(name(50).as_deref(), Some("RTMAX-14"));
        assert_eq!(name(63).as_deref(), Some("RTMAX-1"));
        assert_eq!(name(64).as_deref(), Some("RTMAX"));
        assert_eq!(name(33), None);
        assert_eq!(name(65), None);
    }

    #[test]
    fn slateos_s_range_starts_two_lower() {
        // SlateOS's library reserves no real-time signals, so 32 is RTMIN and
        // the middle moves with it: 32 + (64 - 32) / 2 = 48.
        assert_eq!(real_time_name(32, 32, 64).as_deref(), Some("RTMIN"));
        assert_eq!(real_time_name(48, 32, 64).as_deref(), Some("RTMIN+16"));
        assert_eq!(real_time_name(49, 32, 64).as_deref(), Some("RTMAX-15"));
    }

    #[test]
    fn real_time_offsets_are_read_as_strtol_reads_them() {
        let (lo, hi) = GLIBC;
        let number = |s: &str| real_time_number(s.as_bytes(), lo, hi);
        // Each measured with GNU `kill -l`.
        assert_eq!(number("RTMIN"), Some(34));
        assert_eq!(number("RTMIN+0"), Some(34));
        assert_eq!(number("RTMIN1"), Some(35));
        assert_eq!(number("RTMIN+30"), Some(64));
        assert_eq!(number("RTMIN+31"), None);
        assert_eq!(number("RTMIN-1"), None);
        assert_eq!(number("RTMIN+"), None);
        assert_eq!(number("RTMAX-0"), Some(64));
        assert_eq!(number("RTMAX-30"), Some(34));
        assert_eq!(number("RTMAX-31"), None);
        assert_eq!(number("RTMAX+1"), None);
        // `strtol` skips leading white space, but not trailing.
        assert_eq!(number("RTMIN 2"), Some(36));
        assert_eq!(number("RTMIN 2 "), None);
        assert_eq!(number("RTMIN "), None);
    }

    #[test]
    fn a_system_without_real_time_signals_names_none() {
        // gnulib's fallback where `SIGRTMIN` is not defined: 0 and -1.
        assert_eq!(real_time_name(34, 0, -1), None);
        assert_eq!(real_time_number(b"RTMIN", 0, -1), None);
        assert_eq!(real_time_number(b"RTMAX", 0, -1), None);
    }

    #[test]
    fn the_running_library_s_range_round_trips() {
        let (lo, hi) = (libcall::sigrtmin(), libcall::sigrtmax());
        for n in lo..=hi {
            let name = sig2str(n).unwrap();
            assert_eq!(str2sig(name.as_bytes()), Some(n), "{name}");
        }
    }

    #[test]
    fn strtol_matches_c_at_its_edges() {
        assert_eq!(strtol(b""), (0, 0));
        assert_eq!(strtol(b"+"), (0, 0));
        assert_eq!(strtol(b" -"), (0, 0));
        assert_eq!(strtol(b"12x"), (12, 2));
        assert_eq!(strtol(b"\x0b\x0c-3"), (-3, 4));
        assert_eq!(strtol(b"99999999999999999999").0, i64::MAX);
    }
}
