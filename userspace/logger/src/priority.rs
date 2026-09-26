//! `-p facility.level`: util-linux 2.39.3's `pencode` and `decode`
//! (`misc-utils/logger.c`) over glibc 2.39's `facilitynames` and
//! `prioritynames` (`<syslog.h>` under `SYSLOG_NAMES`) -- the tables the
//! reference is actually built with, copied rather than recalled.

/// `LOG_PRIMASK`.
pub const LOG_PRIMASK: i32 = 0x07;
/// `LOG_FACMASK`.
pub const LOG_FACMASK: i32 = 0x03f8;
/// `LOG_KERN`.
pub const LOG_KERN: i32 = 0;
/// `LOG_USER`.
pub const LOG_USER: i32 = 1 << 3;

/// glibc's `prioritynames`, in glibc's order. `none` is `INTERNAL_NOPRI`
/// (0x10), which `pencode` then masks to 0: `-p none` is `user.emerg`.
pub const PRIORITYNAMES: [(&str, i32); 12] = [
    ("alert", 1),
    ("crit", 2),
    ("debug", 7),
    ("emerg", 0),
    ("err", 3),
    ("error", 3),
    ("info", 6),
    ("none", 0x10),
    ("notice", 5),
    ("panic", 0),
    ("warn", 4),
    ("warning", 4),
];

/// glibc's `facilitynames`, in glibc's order. `mark` is `INTERNAL_MARK`,
/// `LOG_MAKEPRI(LOG_NFACILITIES << 3, 0)` = 192; `security` is `auth`.
pub const FACILITYNAMES: [(&str, i32); 22] = [
    ("auth", 4 << 3),
    ("authpriv", 10 << 3),
    ("cron", 9 << 3),
    ("daemon", 3 << 3),
    ("ftp", 11 << 3),
    ("kern", 0),
    ("lpr", 6 << 3),
    ("mail", 2 << 3),
    ("mark", 24 << 3),
    ("news", 7 << 3),
    ("security", 4 << 3),
    ("syslog", 5 << 3),
    ("user", 1 << 3),
    ("uucp", 8 << 3),
    ("local0", 16 << 3),
    ("local1", 17 << 3),
    ("local2", 18 << 3),
    ("local3", 19 << 3),
    ("local4", 20 << 3),
    ("local5", 21 << 3),
    ("local6", 22 << 3),
    ("local7", 23 << 3),
];

/// The name glibc's table gives facility value `fac` -- the first, so `auth`
/// rather than its deprecated alias `security` -- for a journal record.
#[must_use]
pub fn facility_name(fac: i32) -> Option<&'static str> {
    FACILITYNAMES
        .iter()
        .find(|&&(_, v)| v == fac)
        .map(|&(n, _)| n)
}

/// Why `-p` was refused: `errx(EXIT_FAILURE, ...)` with the half that failed,
/// unquoted, as upstream prints it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PriorityError {
    /// `unknown facility name: %s`
    Facility(Vec<u8>),
    /// `unknown priority name: %s`
    Priority(Vec<u8>),
}

/// `strtol(name, &end, 10)` then the store into an `int`: `None` for
/// `errno` (a value outside `long`), an empty conversion, or trailing bytes.
/// The `int` store WRAPS, so `4294967304` becomes 8.
fn strtol_int(name: &[u8]) -> Option<i32> {
    // `decode` only calls this when the first byte is a digit, so there is no
    // sign or leading space to skip.
    if name.is_empty() || !name.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: i64 = 0;
    for &d in name {
        // `d` is an ASCII digit (checked above), so the subtraction is exact.
        v = v
            .checked_mul(10)?
            .checked_add(i64::from(d.wrapping_sub(b'0')))?; // ERANGE
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "upstream stores strtol's long into an int; the wrap is the behaviour"
    )]
    Some(v as i32)
}

/// `decode`: a name looked up case-insensitively, or -- when it starts with a
/// digit -- a number accepted only if the table holds that value.
fn decode(name: &[u8], table: &[(&str, i32)]) -> Option<i32> {
    let first = *name.first()?;
    if first.is_ascii_digit() {
        let num = strtol_int(name)?;
        return table.iter().any(|&(_, v)| v == num).then_some(num);
    }
    table
        .iter()
        .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(name))
        .map(|&(_, v)| v)
}

/// `pencode`: `facility.level` or a bare level (facility `user`), `kern`
/// silently becoming `user` ("kern is forbidden").
///
/// # Errors
///
/// The facility or the level is not in its table.
pub fn pencode(s: &[u8]) -> Result<i32, PriorityError> {
    let (facility, level_name) = match s.iter().position(|&b| b == b'.') {
        Some(dot) => {
            let (fac, rest) = s.split_at(dot);
            let facility =
                decode(fac, &FACILITYNAMES).ok_or_else(|| PriorityError::Facility(fac.to_vec()))?;
            (facility, rest.get(1..).unwrap_or_default())
        }
        None => (LOG_USER, s),
    };
    let level = decode(level_name, &PRIORITYNAMES)
        .ok_or_else(|| PriorityError::Priority(level_name.to_vec()))?;
    let facility = if facility == LOG_KERN {
        LOG_USER
    } else {
        facility
    };
    Ok((level & LOG_PRIMASK) | (facility & LOG_FACMASK))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn names_and_numbers() {
        assert_eq!(pencode(b"user.notice"), Ok(13));
        assert_eq!(pencode(b"notice"), Ok(13));
        assert_eq!(pencode(b"LOCAL3.ERR"), Ok((19 << 3) | 3));
        assert_eq!(pencode(b"8.5"), Ok(13));
        assert_eq!(pencode(b"5"), Ok(13));
    }

    #[test]
    fn glibcs_internal_entries_are_accepted() {
        assert_eq!(pencode(b"none"), Ok(8)); // 0x10 & 7 = 0 -> user.emerg
        assert_eq!(pencode(b"16"), Ok(8));
        assert_eq!(pencode(b"mark.info"), Ok(192 | 6));
        assert_eq!(pencode(b"192.1"), Ok(193));
        assert_eq!(pencode(b"security.err"), Ok((4 << 3) | 3));
    }

    #[test]
    fn spellings_glibc_does_not_have_are_refused() {
        assert_eq!(
            pencode(b"kernel.err"),
            Err(PriorityError::Facility(b"kernel".to_vec()))
        );
        assert_eq!(
            pencode(b"critical"),
            Err(PriorityError::Priority(b"critical".to_vec()))
        );
    }

    #[test]
    fn kern_is_forbidden_and_becomes_user() {
        assert_eq!(pencode(b"kern.err"), Ok(8 | 3));
        assert_eq!(pencode(b"0.3"), Ok(8 | 3));
    }

    #[test]
    fn a_number_must_be_a_value_the_table_holds() {
        assert_eq!(
            pencode(b"96.1"),
            Err(PriorityError::Facility(b"96".to_vec()))
        );
        assert_eq!(pencode(b"8"), Err(PriorityError::Priority(b"8".to_vec())));
        assert_eq!(
            pencode(b"12x"),
            Err(PriorityError::Priority(b"12x".to_vec()))
        );
    }

    #[test]
    fn the_int_store_wraps() {
        // 4294967304 = 2^32 + 8 -> (int) 8 = LOG_USER.
        assert_eq!(pencode(b"4294967304.5"), Ok(13));
    }

    #[test]
    fn empty_halves_are_unknown_names() {
        assert_eq!(pencode(b".info"), Err(PriorityError::Facility(Vec::new())));
        assert_eq!(pencode(b"user."), Err(PriorityError::Priority(Vec::new())));
        assert_eq!(
            pencode(b"user.info.x"),
            Err(PriorityError::Priority(b"info.x".to_vec()))
        );
    }
}
