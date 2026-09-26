//! Tests for the parse-datetime port.
//!
//! The expectations are worked through from upstream's code — the grammar
//! action, then `parse_datetime_body` — and every one of them is also a row of
//! `scripts/parse-datetime-diff.sh`, which checks it against GNU 9.4.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use std::path::Path;

/// 2021-06-15 12:00:00 UTC, a Tuesday.
const NOW: Timespec = Timespec {
    tv_sec: 1_623_758_400,
    tv_nsec: 0,
};

/// 2021-06-15 00:00:00 UTC.
const MIDNIGHT: i64 = 1_623_715_200;

const DAY: i64 = 86_400;

/// New York's rules as a POSIX string, so no zoneinfo tree is needed.
fn new_york() -> Zone {
    Zone::resolve(
        Some(b"EST5EDT,M3.2.0,M11.1.0"),
        "/nonexistent",
        Path::new("/nonexistent"),
    )
}

/// Parse in `zone` as a fresh process would: `mktime`'s guess starts at 0.
fn parse_in(zone: &Zone, s: &str) -> Option<Timespec> {
    let mut offset = 0;
    parse_datetime_body(
        s.as_bytes(),
        Some(NOW),
        None,
        zone,
        Some(b"UTC0"),
        &mut offset,
    )
}

fn utc(s: &str) -> Option<i64> {
    parse_in(&Zone::utc(), s).map(|t| t.tv_sec)
}

/// The `--debug` output for `s`, in UTC.
fn debug_output(zone: &Zone, s: &str) -> (Option<Timespec>, String) {
    let mut out: Vec<u8> = Vec::new();
    let mut offset = 0;
    let result = parse_datetime_body(
        s.as_bytes(),
        Some(NOW),
        Some(&mut out),
        zone,
        Some(b"UTC0"),
        &mut offset,
    );
    (result, String::from_utf8(out).unwrap())
}

#[test]
fn the_tables_have_the_shape_bison_declared() {
    use tables::*;
    assert_eq!(YYPACT.len(), YYNSTATES);
    assert_eq!(YYDEFACT.len(), YYNSTATES);
    assert_eq!(YYPGOTO.len(), YYNNTS);
    assert_eq!(YYDEFGOTO.len(), YYNNTS);
    assert_eq!(YYTABLE.len(), YYLAST + 1);
    assert_eq!(YYCHECK.len(), YYLAST + 1);
    assert_eq!(YYR1.len(), YYNRULES + 1);
    assert_eq!(YYR2.len(), YYNRULES + 1);
    // Every rule's left side is a nonterminal.
    for &lhs in YYR1.iter().skip(1) {
        let lhs = usize::try_from(lhs).unwrap();
        assert!((YYNTOKENS..YYNTOKENS + YYNNTS).contains(&lhs));
    }
    // No state shifts `error`: no rule of this grammar mentions it, which is
    // why every syntax error aborts.
    for &pact in &YYPACT {
        if pact != YYPACT_NINF
            && let Ok(i) = usize::try_from(i32::from(pact) + 1)
            && i <= YYLAST
        {
            assert_ne!(YYCHECK[i], 1);
        }
    }
    let _ = YYTABLE_NINF;
}

#[test]
fn a_timespec_round_trips_through_system_time() {
    for t in [
        Timespec::default(),
        NOW,
        Timespec {
            tv_sec: 1_623_758_400,
            tv_nsec: 123_456_789,
        },
        Timespec {
            tv_sec: -1,
            tv_nsec: 999_999_999,
        },
        Timespec {
            tv_sec: -2,
            tv_nsec: 500_000_000,
        },
    ] {
        let st = t.to_system_time().unwrap();
        assert_eq!(Timespec::from_system_time(st), t);
    }
}

/// `@-9223372036854775808` parses, so its instant has to convert: `i64::MIN`
/// seconds, which has no positive `i64` to negate. Only a 64-bit `timespec`
/// `SystemTime` holds it.
#[cfg(unix)]
#[test]
fn the_most_negative_second_round_trips() {
    for t in [
        Timespec {
            tv_sec: i64::MIN,
            tv_nsec: 0,
        },
        Timespec {
            tv_sec: i64::MIN,
            tv_nsec: 1,
        },
        Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 999_999_999,
        },
    ] {
        let st = t.to_system_time().unwrap();
        assert_eq!(Timespec::from_system_time(st), t);
    }
    assert_eq!(
        parse_in(&Zone::utc(), "@-9223372036854775808"),
        Some(Timespec {
            tv_sec: i64::MIN,
            tv_nsec: 0
        })
    );
}

#[test]
fn iso_dates_and_times() {
    assert_eq!(utc("2021-06-15"), Some(MIDNIGHT));
    assert_eq!(utc("2021-06-15 12:00:00"), Some(NOW.tv_sec));
    assert_eq!(utc("2021-06-15T12:00:00Z"), Some(NOW.tv_sec));
    assert_eq!(utc("2021-06-15 12:00:00 +0530"), Some(NOW.tv_sec - 19_800));
    assert_eq!(utc("2021-06-15 12:00:00 -05:00"), Some(NOW.tv_sec + 18_000));
}

#[test]
fn the_other_date_orders() {
    assert_eq!(utc("17 Jun 1992"), Some(708_739_200));
    // Without the comma, 1992 is a bare number after a date -- which
    // digits_to_date_time takes as the year.
    assert_eq!(utc("Jun 17 1992"), Some(708_739_200));
    assert_eq!(utc("Jun 17, 1992"), Some(708_739_200));
    assert_eq!(utc("17-Jun-1992"), Some(708_739_200));
    assert_eq!(utc("06/15/21"), Some(MIDNIGHT));
    assert_eq!(utc("2021/06/15"), Some(MIDNIGHT));
    assert_eq!(utc("20210615"), Some(MIDNIGHT));
}

#[test]
fn a_bare_number_of_three_or_four_digits_is_a_time() {
    assert_eq!(utc("1230"), Some(MIDNIGHT + 12 * 3600 + 30 * 60));
    assert_eq!(utc("12"), Some(MIDNIGHT + 12 * 3600));
}

#[test]
fn the_empty_string_is_midnight_today() {
    assert_eq!(utc(""), Some(MIDNIGHT));
    assert_eq!(utc("   "), Some(MIDNIGHT));
}

#[test]
fn seconds_since_the_epoch() {
    assert_eq!(parse_in(&Zone::utc(), "@0"), Some(Timespec::default()));
    assert_eq!(
        parse_in(&Zone::utc(), "@1.5"),
        Some(Timespec {
            tv_sec: 1,
            tv_nsec: 500_000_000
        })
    );
    // Negative, and the nanoseconds stay positive.
    assert_eq!(
        parse_in(&Zone::utc(), "@-1.5"),
        Some(Timespec {
            tv_sec: -2,
            tv_nsec: 500_000_000
        })
    );
    // Excess digits of a negative fraction round toward -infinity.
    assert_eq!(
        parse_in(&Zone::utc(), "@-0.0000000001"),
        Some(Timespec {
            tv_sec: -1,
            tv_nsec: 999_999_999
        })
    );
    // `@` does not combine with anything else.
    assert_eq!(utc("@0 + 1 day"), None);
}

#[test]
fn relative_items() {
    // `now` and `today` keep the time of day; `tomorrow` too.
    assert_eq!(utc("now"), Some(NOW.tv_sec));
    assert_eq!(utc("today"), Some(NOW.tv_sec));
    assert_eq!(utc("tomorrow"), Some(NOW.tv_sec + DAY));
    assert_eq!(utc("yesterday"), Some(NOW.tv_sec - DAY));
    assert_eq!(utc("3 days ago"), Some(NOW.tv_sec - 3 * DAY));
    // `ago` negates only the item it follows.
    assert_eq!(utc("1 day 2 hours ago"), Some(NOW.tv_sec + DAY - 7200));
    assert_eq!(utc("+1 fortnight"), Some(NOW.tv_sec + 14 * DAY));
    assert_eq!(utc("next week"), Some(NOW.tv_sec + 7 * DAY));
    assert_eq!(utc("last year"), Some(NOW.tv_sec - 365 * DAY));
    // Month arithmetic carries: January 31st plus a month is March 3rd.
    assert_eq!(utc("2021-01-31 1 month"), Some(1_614_729_600));
    // A fractional second.
    assert_eq!(
        parse_in(&Zone::utc(), "1.25 seconds ago"),
        Some(Timespec {
            tv_sec: NOW.tv_sec - 2,
            tv_nsec: 750_000_000
        })
    );
}

#[test]
fn weekdays() {
    // Today is a Tuesday, and a bare weekday includes today; the result is at
    // midnight.
    assert_eq!(utc("tuesday"), Some(MIDNIGHT));
    assert_eq!(utc("wednesday"), Some(MIDNIGHT + DAY));
    assert_eq!(utc("next tuesday"), Some(MIDNIGHT + 7 * DAY));
    assert_eq!(utc("last tuesday"), Some(MIDNIGHT - 7 * DAY));
    assert_eq!(utc("Tue,"), Some(MIDNIGHT));
    // Beside an explicit date the weekday is ignored, not checked.
    assert_eq!(utc("Monday 2021-06-15"), Some(MIDNIGHT));
}

#[test]
fn the_twelve_hour_clock() {
    assert_eq!(utc("12am"), Some(MIDNIGHT));
    assert_eq!(utc("12pm"), Some(MIDNIGHT + 12 * 3600));
    assert_eq!(utc("1 p.m."), Some(MIDNIGHT + 13 * 3600));
    assert_eq!(utc("12:30:15 AM"), Some(MIDNIGHT + 30 * 60 + 15));
    assert_eq!(utc("13pm"), None);
    assert_eq!(utc("0am"), None);
}

#[test]
fn a_signed_number_after_a_time_is_a_zone() {
    // The rule the hand-written parser could not express: `-5` is UTC-5.
    assert_eq!(utc("12:30 -5"), Some(MIDNIGHT + 17 * 3600 + 30 * 60));
    // Three or more digits are HHMM.
    assert_eq!(utc("12:30 -0530"), Some(MIDNIGHT + 18 * 3600));
    // More than 24 hours is refused.
    assert_eq!(utc("12:30 +2401"), None);
}

#[test]
fn zone_names() {
    // A zone alone means midnight in that zone.
    assert_eq!(utc("EST"), Some(MIDNIGHT + 5 * 3600));
    assert_eq!(utc("2021-06-15 12:00 EDT"), Some(NOW.tv_sec + 4 * 3600));
    assert_eq!(utc("2021-06-15 12:00 UTC+2"), Some(NOW.tv_sec - 2 * 3600));
    // Military `Z`, and `J` (local time).
    assert_eq!(utc("2021-06-15 12:00 Z"), Some(NOW.tv_sec));
    assert_eq!(utc("2021-06-15 12:00 J"), Some(NOW.tv_sec));
    // Periods are dropped for a second look at the zone table.
    assert_eq!(utc("2021-06-15 12:00 E.S.T."), Some(NOW.tv_sec + 5 * 3600));
    // Two zones is one too many.
    assert_eq!(utc("2021-06-15 12:00 EST UTC"), None);
}

#[test]
fn a_tz_prefix_names_the_zone_for_the_rest_of_the_string() {
    assert_eq!(
        utc("TZ=\"EST5\" 2021-06-15 12:00"),
        Some(NOW.tv_sec + 5 * 3600)
    );
    assert_eq!(
        utc("  TZ=\"EST5\"2021-06-15 12:00"),
        Some(NOW.tv_sec + 5 * 3600)
    );
    // An unknown escape, or no closing quote, is no prefix at all -- and
    // then `TZ` is an unknown word.
    assert_eq!(utc("TZ=\"EST\\5\" 12:00"), None);
    assert_eq!(utc("TZ=\"EST5 12:00"), None);
}

#[test]
fn comments_are_skipped_and_may_nest() {
    assert_eq!(utc("(a (nested) comment) 2021-06-15"), Some(MIDNIGHT));
    // An unterminated comment ends the string.
    assert_eq!(utc("2021-06-15 (unterminated"), Some(MIDNIGHT));
}

#[test]
fn out_of_range_fields_are_refused_not_carried() {
    assert_eq!(utc("2021-02-29"), None);
    assert_eq!(utc("2021-06-15 24:00"), None);
    assert_eq!(utc("2021-13-01"), None);
    assert_eq!(utc("junk"), None);
    assert_eq!(utc("2021-06-15 2021-06-16"), None, "two dates");
}

#[test]
fn an_int_field_keeps_the_low_32_bits_as_gcc_does() {
    // 4294967326 minutes is 30 once narrowed into `int`.
    assert_eq!(utc("10:4294967326"), Some(MIDNIGHT + 10 * 3600 + 30 * 60));
}

#[test]
fn a_nul_ends_the_string() {
    let mut offset = 0;
    let got = parse_datetime_body(
        b"2021-06-15\0junk",
        Some(NOW),
        None,
        &Zone::utc(),
        None,
        &mut offset,
    );
    assert_eq!(got.map(|t| t.tv_sec), Some(MIDNIGHT));
}

#[test]
fn a_skipped_local_time_is_refused_unless_it_names_its_offset() {
    let ny = new_york();
    // 02:30 on 2021-03-14 does not exist in New York.
    assert_eq!(parse_in(&ny, "2021-03-14 02:30"), None);
    // With an explicit offset it is simply 07:30 UTC.
    assert_eq!(
        parse_in(&ny, "2021-03-14 02:30 -0500").map(|t| t.tv_sec),
        Some(1_615_707_000)
    );
}

#[test]
fn a_repeated_local_time_is_the_first_to_a_fresh_process() {
    let ny = new_york();
    assert_eq!(
        parse_in(&ny, "2021-11-07 01:30").map(|t| t.tv_sec),
        Some(1_636_263_000)
    );
}

#[test]
fn the_local_zones_own_abbreviations_set_dst_not_the_offset() {
    let ny = new_york();
    // EDT in June is the zone's own daylight time: 12:00 EDT.
    assert_eq!(
        parse_in(&ny, "2021-06-15 12:00 EDT").map(|t| t.tv_sec),
        Some(NOW.tv_sec + 4 * 3600)
    );
    // EST in June asks mktime for standard time, which moves the hour, which
    // mktime_ok refuses.
    assert_eq!(parse_in(&ny, "2021-06-15 12:00 EST"), None);
}

#[test]
fn debug_output_reports_each_part() {
    let (result, out) = debug_output(&Zone::utc(), "2021-06-15 12:00");
    assert_eq!(result.map(|t| t.tv_sec), Some(NOW.tv_sec));
    assert_eq!(
        out,
        "date: parsed date part: (Y-M-D) 2021-06-15\n\
         date: parsed time part: 12:00:00\n\
         date: input timezone: TZ=\"UTC0\" environment value or -u\n\
         date: using specified time as starting value: '12:00:00'\n\
         date: starting date/time: '(Y-M-D) 2021-06-15 12:00:00'\n\
         date: '(Y-M-D) 2021-06-15 12:00:00' = 1623758400 epoch-seconds\n\
         date: timezone: Universal Time\n\
         date: final: 1623758400.000000000 (epoch-seconds)\n\
         date: final: (Y-M-D) 2021-06-15 12:00:00 (UTC)\n\
         date: final: (Y-M-D) 2021-06-15 12:00:00 (UTC+00)\n"
    );
}

#[test]
fn debug_output_names_an_unknown_word_as_the_lexer_left_it() {
    let (result, out) = debug_output(&Zone::utc(), "2021-06-15 f.o.o bar");
    assert_eq!(result, None);
    // The order is the tables': the date cannot be reduced until the token
    // after it has been read, so the lexer's complaint comes first.
    assert_eq!(
        out,
        "date: error: unknown word 'FOO'\n\
         date: parsed date part: (Y-M-D) 2021-06-15\n\
         date: error: parsing failed, stopped at ' bar'\n"
    );
}
