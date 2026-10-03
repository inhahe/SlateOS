//! procps-ng 4.0.4's `local/signals.c`: signal names to numbers, as `pkill`
//! reads them.
//!
//! procps keeps its own table rather than asking the C library, so its
//! programs accept the names *it* knows, spelled the ways it allows: with or
//! without `SIG`, in any case, the three aliases `CLD`, `IO` and `IOT`, the
//! two names for nothing (`EXIT` and `NULL`, both 0), `RTMIN` and `RTMIN+n`,
//! and a plain number. That is a different set of spellings from GNU's
//! `sig2str` (which `coreutils::sig2str` is) -- `pkill -IO` works and
//! `kill -IO` does not -- so it is a separate module rather than a second
//! caller of that one.
//!
//! Only `signal_name_to_number` is here, because it is the one function of
//! that file a ported program calls. The rest of `signals.c` -- the listings
//! `kill -l` and `skill -l` print -- comes with the first port that prints
//! one.

/// The signal table, for Linux on x86-64: `sigtable[]` with `SIGEMT`
/// undefined (so `STKFLT` is present) and `SIGPWR` defined.
///
/// Sorted by `strcasecmp`, as upstream's `bsearch` requires; a lookup here
/// is a linear search, which finds exactly what the binary search finds in a
/// sorted table.
const SIGTABLE: [(&str, i32); 31] = [
    ("ABRT", 6),
    ("ALRM", 14),
    ("BUS", 7),
    ("CHLD", 17),
    ("CONT", 18),
    ("FPE", 8),
    ("HUP", 1),
    ("ILL", 4),
    ("INT", 2),
    ("KILL", 9),
    ("PIPE", 13),
    ("POLL", 29),
    ("PROF", 27),
    ("PWR", 30),
    ("QUIT", 3),
    ("SEGV", 11),
    ("STKFLT", 16),
    ("STOP", 19),
    ("SYS", 31),
    ("TERM", 15),
    ("TRAP", 5),
    ("TSTP", 20),
    ("TTIN", 21),
    ("TTOU", 22),
    ("URG", 23),
    ("USR1", 10),
    ("USR2", 12),
    ("VTALRM", 26),
    ("WINCH", 28),
    ("XCPU", 24),
    ("XFSZ", 25),
];

/// `SIGCHLD`, which `CLD` names.
const SIGCHLD: i32 = 17;
/// `SIGPOLL`, which `IO` names.
const SIGPOLL: i32 = 29;
/// `SIGABRT`, which `IOT` names.
const SIGABRT: i32 = 6;

/// `strncasecmp(name, prefix, prefix.len()) == 0`, in the C locale -- which
/// is what it is in every locale for these ASCII prefixes.
fn starts_with_ignore_case(name: &[u8], prefix: &[u8]) -> bool {
    name.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `signal_name_to_number`: the number `name` spells, or -1 when it spells
/// none.
///
/// `rtmin` is the C library's `SIGRTMIN` -- 34 under glibc, 32 under
/// SlateOS's library -- which upstream reads through the macro, so the same
/// spelling can be a different number on two systems. `RTMIN+n` is
/// `rtmin + n`; and a plain number is itself, provided it is not negative and
/// `n + rtmin` is at most 127, a bound that mixes the two up exactly as
/// upstream's does.
///
/// The number after `RTMIN+`, and a plain one, are read by `strtol`: leading
/// spaces and a sign are allowed, and nothing may follow. A number beyond
/// `long` saturates, and the sum with `rtmin` then wraps as the C addition
/// does on x86-64; the result is cut to `int`. So `RTMIN+9223372036854775807`
/// passes the bound and is signal 33 under glibc -- reproduced, not endorsed:
/// it is what the program the harness compares against does.
#[must_use]
pub fn signal_name_to_number(name: &[u8], rtmin: i32) -> i32 {
    let mut name = name;
    if starts_with_ignore_case(name, b"SIG") {
        name = name.get(3..).unwrap_or_default();
    }
    if name.eq_ignore_ascii_case(b"CLD") {
        return SIGCHLD;
    }
    if name.eq_ignore_ascii_case(b"IO") {
        return SIGPOLL;
    }
    if name.eq_ignore_ascii_case(b"IOT") {
        return SIGABRT;
    }
    if let Some(&(_, num)) = SIGTABLE
        .iter()
        .find(|(n, _)| name.eq_ignore_ascii_case(n.as_bytes()))
    {
        return num;
    }
    if name.eq_ignore_ascii_case(b"RTMIN") {
        return rtmin;
    }
    if name.eq_ignore_ascii_case(b"EXIT") || name.eq_ignore_ascii_case(b"NULL") {
        return 0;
    }
    let mut offset: i64 = 0;
    if starts_with_ignore_case(name, b"RTMIN+") {
        name = name.get(6..).unwrap_or_default();
        offset = i64::from(rtmin);
    }
    // `strtol (name, &endp, 10)`, which must use all of it.
    let (val, used) = super::scanf::strtol(name);
    if used == 0 || used != name.len() {
        return -1;
    }
    if val < 0 || val.wrapping_add(i64::from(rtmin)) > 127 {
        return -1;
    }
    super::scanf::low_i32(val.wrapping_add(offset))
}

#[cfg(test)]
mod tests {
    use super::signal_name_to_number as num;

    #[test]
    fn names_with_and_without_sig_in_any_case() {
        assert_eq!(num(b"TERM", 34), 15);
        assert_eq!(num(b"term", 34), 15);
        assert_eq!(num(b"SIGkill", 34), 9);
        assert_eq!(num(b"sigHUP", 34), 1);
        assert_eq!(num(b"STKFLT", 34), 16);
        assert_eq!(num(b"POLL", 34), 29);
        assert_eq!(num(b"PWR", 34), 30);
    }

    #[test]
    fn the_aliases_and_the_names_for_nothing() {
        assert_eq!(num(b"CLD", 34), 17);
        assert_eq!(num(b"io", 34), 29);
        assert_eq!(num(b"SIGIOT", 34), 6);
        assert_eq!(num(b"EXIT", 34), 0);
        assert_eq!(num(b"null", 34), 0);
    }

    #[test]
    fn real_time_signals_follow_the_library() {
        assert_eq!(num(b"RTMIN", 34), 34);
        assert_eq!(num(b"RTMIN", 32), 32);
        assert_eq!(num(b"rtmin+3", 34), 37);
        assert_eq!(num(b"RTMIN+3", 32), 35);
        assert_eq!(num(b"RTMIN+ 3", 34), 37);
        assert_eq!(num(b"RTMIN+-1", 34), -1);
        assert_eq!(num(b"RTMIN+93", 34), 127);
        assert_eq!(num(b"RTMIN+94", 34), -1);
        assert_eq!(num(b"RTMIN+", 34), -1);
    }

    #[test]
    fn numbers_are_bounded_by_the_real_time_range() {
        assert_eq!(num(b"9", 34), 9);
        assert_eq!(num(b"0", 34), 0);
        assert_eq!(num(b"+5", 34), 5);
        assert_eq!(num(b" 5", 34), 5);
        assert_eq!(num(b"93", 34), 93);
        assert_eq!(num(b"94", 34), -1);
        assert_eq!(num(b"95", 32), 95);
        assert_eq!(num(b"-1", 34), -1);
        assert_eq!(num(b"5x", 34), -1);
        assert_eq!(num(b"", 34), -1);
        assert_eq!(num(b"SIG", 34), -1);
    }

    #[test]
    fn a_number_beyond_long_wraps_as_c_does() {
        // `strtol` saturates at LONG_MAX; adding SIGRTMIN wraps negative and
        // passes the bound; the result is cut to `int`.
        assert_eq!(num(b"RTMIN+9223372036854775807", 34), 33);
        assert_eq!(num(b"99999999999999999999", 34), -1);
    }

    #[test]
    fn unknown_names_are_minus_one() {
        assert_eq!(num(b"f", 34), -1);
        assert_eq!(num(b"HU", 34), -1);
        assert_eq!(num(b"-signal", 34), -1);
        assert_eq!(num(b"USR3", 34), -1);
    }
}
