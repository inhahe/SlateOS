//! Tests for the glibc `tzset` port. The expected values are glibc 2.39's,
//! measured with GNU `date` under WSL where the comment says so.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
#![allow(clippy::arithmetic_side_effects)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::*;

/// Serialises the tests that read or set `rule_dstoff`, which is one per
/// process in glibc and so one per test binary here.
static PROCESS: Mutex<()> = Mutex::new(());

/// Take the process, as a fresh one: `rule_dstoff` back at 0.
fn fresh_process() -> MutexGuard<'static, ()> {
    let guard = PROCESS.lock().unwrap_or_else(PoisonError::into_inner);
    reset_rule_dstoff();
    guard
}

/// A POSIX rule with no `posixrules` to fall back on.
fn rules(tz: &str) -> Engine {
    parse_tz(tz.as_bytes(), &mut |_, _| None)
}

/// `(tm_gmtoff, tm_isdst, tm_zone)` at `t`.
fn at_utc(zone: &Engine, t: i64) -> (i32, bool, String) {
    let s = zone.state(t).expect("a representable year");
    (
        s.gmtoff,
        s.is_dst,
        String::from_utf8(s.name.to_vec()).unwrap(),
    )
}

fn st(gmtoff: i32, is_dst: bool, name: &str) -> (i32, bool, String) {
    (gmtoff, is_dst, name.to_string())
}

/// Seconds since the epoch of a UTC civil time.
fn utc(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    crate::days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s
}

// --- A TZif writer --------------------------------------------------------------

/// One local time type: offset, DST, name, standard/wall, UT/local.
type Ty = (i32, bool, &'static str, bool, bool);

/// A v2 TZif file: a v1 block with no transitions (as `zic -b slim` writes)
/// and the v2 block with the given ones.
fn tzif(transitions: &[(i64, u8)], types: &[Ty], footer: Option<&str>) -> Vec<u8> {
    let mut desig = Vec::new();
    let mut idx = Vec::new();
    for &(_, _, name, _, _) in types {
        idx.push(u8::try_from(desig.len()).unwrap());
        desig.extend_from_slice(name.as_bytes());
        desig.push(0);
    }
    let indicators = types.iter().any(|t| t.3 || t.4);
    let n_ind = if indicators { types.len() } else { 0 };
    let header = |out: &mut Vec<u8>, timecnt: usize| {
        out.extend_from_slice(b"TZif2");
        out.extend_from_slice(&[0; 15]);
        for count in [n_ind, n_ind, 0, timecnt, types.len(), desig.len()] {
            out.extend_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
        }
    };
    let block = |out: &mut Vec<u8>, times: &[(i64, u8)], wide: bool| {
        for &(t, _) in times {
            if wide {
                out.extend_from_slice(&t.to_be_bytes());
            } else {
                out.extend_from_slice(&i32::try_from(t).unwrap().to_be_bytes());
            }
        }
        out.extend(times.iter().map(|&(_, ty)| ty));
        for (i, &(off, dst, _, _, _)) in types.iter().enumerate() {
            out.extend_from_slice(&off.to_be_bytes());
            out.push(u8::from(dst));
            out.push(idx[i]);
        }
        out.extend_from_slice(&desig);
        if indicators {
            out.extend(types.iter().map(|t| u8::from(t.3)));
            out.extend(types.iter().map(|t| u8::from(t.4)));
        }
    };
    let mut out = Vec::new();
    header(&mut out, 0);
    block(&mut out, &[], false);
    header(&mut out, transitions.len());
    block(&mut out, transitions, true);
    out.push(b'\n');
    out.extend_from_slice(footer.unwrap_or("").as_bytes());
    out.push(b'\n');
    out
}

const EST: Ty = (-18_000, false, "EST", false, false);
const EDT: Ty = (-14_400, true, "EDT", false, false);

/// New York's 2020, as the only two transitions of a `posixrules`, with its
/// footer.
fn new_york_2020() -> Vec<u8> {
    tzif(
        &[(1_583_650_800, 1), (1_604_210_400, 0)],
        &[EST, EDT],
        Some("EST5EDT,M3.2.0,M11.1.0"),
    )
}

/// A scratch zoneinfo directory, removed on drop.
struct ZoneDir(PathBuf);

impl ZoneDir {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("localtime-tzset-{tag}-{}", std::process::id()));
        // A leftover from an aborted run: stale, and ours to remove.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, bytes: &[u8]) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    fn dir(&self) -> &Path {
        &self.0
    }
    fn missing(&self) -> PathBuf {
        self.0.join("no-such-localtime")
    }
}

impl Drop for ZoneDir {
    fn drop(&mut self) {
        // Scratch space; failing to remove it costs a directory in /tmp.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// --- sscanf ------------------------------------------------------------------------

#[test]
fn hu_skips_white_space_takes_a_sign_and_wraps() {
    assert_eq!(scan_hu(b"  12x", 0), Some((12, 4)));
    assert_eq!(scan_hu(b"+7", 0), Some((7, 2)));
    assert_eq!(scan_hu(b"-5", 0), Some((65_531, 2)));
    assert_eq!(scan_hu(b"65537", 0), Some((1, 5)));
    // `strtoul` saturates, and ULONG_MAX is all ones in the low 16 bits too.
    assert_eq!(scan_hu(b"99999999999999999999999", 0), Some((65_535, 23)));
    assert_eq!(scan_hu(b"-", 0), None);
    assert_eq!(scan_hu(b"  ", 0), None);
    assert_eq!(scan_hu(b"x", 0), None);
}

#[test]
fn sscanf_stops_at_the_first_failure_and_keeps_what_it_had() {
    let s = sscanf(b"5:3x", &HMS);
    assert_eq!(
        (s.count, s.values[0], s.values[1], s.consumed),
        (2, 5, 3, Some(3))
    );
    let s = sscanf(b"5:", &HMS);
    assert_eq!((s.count, s.consumed), (1, Some(1)));
    let s = sscanf(b"M3.2", &MND);
    assert_eq!(
        (s.count, s.values[0], s.values[1], s.consumed),
        (2, 3, 2, None)
    );
}

// --- The POSIX-rule engine ----------------------------------------------------------

#[test]
fn a_full_rule_changes_at_its_instants() {
    let z = rules("EST5EDT,M3.2.0,M11.1.0");
    assert_eq!(at_utc(&z, 1_583_650_799), st(-18_000, false, "EST"));
    assert_eq!(at_utc(&z, 1_583_650_800), st(-14_400, true, "EDT"));
    assert_eq!(at_utc(&z, 1_604_210_399), st(-14_400, true, "EDT"));
    assert_eq!(at_utc(&z, 1_604_210_400), st(-18_000, false, "EST"));
}

#[test]
fn a_name_without_an_offset_is_utc_under_that_name() {
    // glibc: `TZ=Foo/Bar date +%Z%z` is `Foo+0000`.
    assert_eq!(at_utc(&rules("Foo/Bar"), 0), st(0, false, "Foo"));
    assert_eq!(at_utc(&rules("Universal"), 0), st(0, false, "Universal"));
    // Fewer than three letters is no name at all.
    assert_eq!(at_utc(&rules("12"), 0), st(0, false, ""));
    assert_eq!(at_utc(&rules("Ab5"), 0), st(0, false, ""));
}

#[test]
fn angle_brackets_quote_a_name() {
    assert_eq!(at_utc(&rules("<+05>-5"), 0), st(18_000, false, "+05"));
    assert_eq!(at_utc(&rules("<-00>0"), 0), st(0, false, "-00"));
    assert_eq!(at_utc(&rules("<AB>5"), 0), st(0, false, ""));
    assert_eq!(at_utc(&rules("<ABC5"), 0), st(0, false, ""));
}

#[test]
fn an_offset_is_read_as_sscanf_reads_it_and_then_clamped() {
    assert_eq!(at_utc(&rules("EST-5"), 0).0, 18_000);
    assert_eq!(at_utc(&rules("EST+ 5"), 0).0, -18_000);
    assert_eq!(at_utc(&rules("EST5:30"), 0).0, -19_800);
    // Hours clamp at 24, minutes and seconds at 59.
    assert_eq!(at_utc(&rules("EST25"), 0).0, -86_400);
    assert_eq!(at_utc(&rules("EST99:99:99"), 0).0, -(86_400 + 59 * 60 + 59));
    // A second sign is `%hu`'s: -5 is 65531, which clamps to 24 hours.
    assert_eq!(at_utc(&rules("EST+-5"), 0).0, -86_400);
}

#[test]
fn a_dst_name_alone_is_an_hour_ahead_of_standard_time() {
    let z = rules("AAA3BBB,M3.2.0,M11.1.0");
    assert_eq!(at_utc(&z, utc(2020, 7, 1, 0, 0, 0)), st(-7200, true, "BBB"));
}

#[test]
fn anything_after_the_offset_that_is_not_a_name_makes_an_unnamed_dst_half() {
    // `EST5x`: no DST name, but not "no DST" either -- glibc goes on to parse
    // rules from `x`, fails, and is left with two zeroed rules whose change
    // times differ by the standard offset. That reads as a southern
    // hemisphere zone in DST for all but five hours of each year, and the DST
    // half is the `memset`'s: no name, offset zero.
    let z = rules("EST5x");
    assert_eq!(at_utc(&z, utc(2020, 7, 1, 0, 0, 0)), st(0, true, ""));
    assert_eq!(
        at_utc(&z, utc(2020, 1, 1, 4, 59, 59)),
        st(-18_000, false, "EST")
    );
    assert_eq!(at_utc(&z, utc(2020, 1, 1, 5, 0, 0)), st(0, true, ""));
}

#[test]
fn a_rule_with_no_dates_and_no_posixrules_takes_the_us_rules() {
    let z = rules("AAA3BBB");
    // M3.2.0 at 02:00 AAA is 05:00Z; M11.1.0 at 02:00 BBB is 04:00Z.
    assert_eq!(at_utc(&z, utc(2020, 3, 8, 4, 59, 59)).2, "AAA");
    assert_eq!(at_utc(&z, utc(2020, 3, 8, 5, 0, 0)).2, "BBB");
    assert_eq!(at_utc(&z, utc(2020, 11, 1, 3, 59, 59)).2, "BBB");
    assert_eq!(at_utc(&z, utc(2020, 11, 1, 4, 0, 0)).2, "AAA");
    // And so does a lone trailing comma.
    assert_eq!(
        at_utc(&rules("AAA3BBB,"), utc(2020, 3, 8, 5, 0, 0)).2,
        "BBB"
    );
}

#[test]
fn a_rule_that_fails_partway_keeps_the_fields_it_wrote() {
    let Engine::Rules(r) = rules("AAA3BBB,M3") else {
        panic!("a rule, not a table");
    };
    let std = &r.rules[0];
    assert_eq!(
        (std.kind, std.m, std.n, std.d, std.secs),
        (Kind::M, 3, 0, 0, 0)
    );
    assert!(!std.parsed);
    assert_eq!(r.rules[1].kind, Kind::J0);
    assert_eq!(r.rules[1].offset, -7200);

    // Failing after the date: the date stays, the time is never set.
    let Engine::Rules(r) = rules("EST5EDT,M3.2.0x,M11.1.0") else {
        panic!("a rule, not a table");
    };
    let std = &r.rules[0];
    assert_eq!(
        (std.m, std.n, std.d, std.secs, std.parsed),
        (3, 2, 0, 0, false)
    );
}

#[test]
fn every_year_up_to_1970_changes_on_1970s_dates() {
    // glibc: `TZ='CET-1CEST,M3.5.0,M10.5.0/3' date -d 0021-06-15` is CET.
    let north = rules("CET-1CEST,M3.5.0,M10.5.0/3");
    assert_eq!(at_utc(&north, utc(21, 6, 15, 12, 0, 0)).2, "CET");
    assert_eq!(at_utc(&north, utc(1969, 7, 1, 12, 0, 0)).2, "CET");
    assert_eq!(at_utc(&north, utc(1970, 7, 1, 12, 0, 0)).2, "CEST");
    // And the southern mirror image: every such instant is summer time.
    let south = rules("AEST-10AEDT,M10.1.0,M4.1.0/3");
    assert_eq!(at_utc(&south, utc(1960, 7, 1, 12, 0, 0)).2, "AEDT");
    assert_eq!(at_utc(&south, utc(1990, 7, 1, 12, 0, 0)).2, "AEST");
}

#[test]
fn the_rules_are_those_of_the_instants_utc_year() {
    // glibc: `TZ='AAA-12BBB,J1/0,J1/10' date -d @1609419600` is
    // `2021-01-01 01:00:00 AAA +1200`. By the local year (2021) this instant
    // would fall inside the DST window; by the UTC year (2020) it is past it.
    let z = rules("AAA-12BBB,J1/0,J1/10");
    assert_eq!(at_utc(&z, 1_609_419_600), st(43_200, false, "AAA"));
}

#[test]
fn julian_days_count_february_29th_only_without_the_j() {
    // J60 is March 1st even in a leap year; 59 (from zero) is February 29th.
    let j = rules("AAA0BBB,J60/0,J61/0");
    assert!(at_utc(&j, utc(2020, 3, 1, 12, 0, 0)).1);
    assert!(!at_utc(&j, utc(2020, 2, 29, 12, 0, 0)).1);
    let n = rules("AAA0BBB,59/0,60/0");
    assert!(at_utc(&n, utc(2020, 2, 29, 12, 0, 0)).1);
    assert!(!at_utc(&n, utc(2020, 3, 1, 12, 0, 0)).1);
}

#[test]
fn week_five_is_the_last_such_day_of_the_month() {
    // The last Sunday of February 2020 is the 23rd, the first of March the 1st.
    let z = rules("AAA0BBB,M2.5.0/0,M3.1.0/0");
    assert!(!at_utc(&z, utc(2020, 2, 22, 23, 59, 59)).1);
    assert!(at_utc(&z, utc(2020, 2, 23, 0, 0, 0)).1);
    assert!(at_utc(&z, utc(2020, 2, 29, 22, 59, 59)).1);
    assert!(!at_utc(&z, utc(2020, 2, 29, 23, 0, 0)).1);
}

#[test]
fn a_negative_or_long_time_of_day_is_taken_as_written() {
    // `/-1` is 23:00 the day before; `/26` is 02:00 the day after.
    let z = rules("AAA0BBB,M3.2.0/-1,M11.1.0/26");
    assert!(!at_utc(&z, utc(2020, 3, 7, 22, 59, 59)).1);
    assert!(at_utc(&z, utc(2020, 3, 7, 23, 0, 0)).1);
    // 26:00 BBB (UTC+1) on November 1st is 01:00Z on the 2nd.
    assert!(at_utc(&z, utc(2020, 11, 2, 0, 59, 59)).1);
    assert!(!at_utc(&z, utc(2020, 11, 2, 1, 0, 0)).1);
}

#[test]
fn a_year_tm_year_cannot_hold_has_no_answer() {
    let z = rules("EST5");
    assert!(z.state(i64::MAX).is_none());
    assert_eq!(z.state_or_standard(i64::MAX).name, b"EST");
}

#[test]
fn an_unparsed_half_keeps_its_zeroed_change_for_a_first_lookup_in_year_zero() {
    // `computed_for` starts at 0 for a half that did not parse, so the first
    // year-0 lookup uses the `memset`'s change time of 0 for both halves --
    // equal, so standard time. Once another year has been computed the cache
    // no longer says 0, and year 0 is computed like any other: the two zeroed
    // rules then differ by the offset, which reads as DST (see `EST5x` above).
    let z = rules("EST5x");
    let year_zero = utc(0, 6, 1, 0, 0, 0);
    assert_eq!(at_utc(&z, year_zero), st(-18_000, false, "EST"));
    at_utc(&z, utc(2020, 6, 1, 0, 0, 0));
    assert_eq!(at_utc(&z, year_zero), st(0, true, ""));
}

// --- tzfile ------------------------------------------------------------------------

#[test]
fn before_the_first_transition_is_the_first_standard_type() {
    let _p = fresh_process();
    let bytes = tzif(
        &[(1000, 0), (2000, 1)],
        &[
            (3600, true, "DDD", false, false),
            (0, false, "SSS", false, false),
        ],
        Some("SSS0"),
    );
    let z = Engine::Table(Table::from_tzif(&bytes).unwrap());
    assert_eq!(at_utc(&z, 999), st(0, false, "SSS"));
    assert_eq!(at_utc(&z, 1000), st(3600, true, "DDD"));
    assert_eq!(at_utc(&z, 1999), st(3600, true, "DDD"));
    // At and past the last transition, the footer.
    assert_eq!(at_utc(&z, 2000), st(0, false, "SSS"));
}

#[test]
fn a_file_with_no_transitions_never_reads_its_footer() {
    let _p = fresh_process();
    let bytes = tzif(&[], &[(0, false, "AAA", false, false)], Some("BBB5"));
    let z = Engine::Table(Table::from_tzif(&bytes).unwrap());
    assert_eq!(at_utc(&z, 1_600_000_000), st(0, false, "AAA"));
}

#[test]
fn past_the_last_transition_with_no_footer_the_last_type_stands() {
    let _p = fresh_process();
    let bytes = tzif(&[(1000, 1)], &[EST, EDT], None);
    let z = Engine::Table(Table::from_tzif(&bytes).unwrap());
    assert_eq!(at_utc(&z, 5_000_000_000), st(-14_400, true, "EDT"));
}

#[test]
fn the_footer_is_parsed_the_glibc_way() {
    let _p = fresh_process();
    let bytes = tzif(&[(1000, 0)], &[EST], Some("Foo/Bar"));
    let z = Engine::Table(Table::from_tzif(&bytes).unwrap());
    assert_eq!(at_utc(&z, 2000), st(0, false, "Foo"));
}

#[test]
fn the_search_agrees_with_a_plain_one_everywhere() {
    let _p = fresh_process();
    let transitions: Vec<(i64, u8)> = (0..300i64)
        .map(|i| (i * 10_000_000 - 1_000_000_000, u8::try_from(i % 2).unwrap()))
        .collect();
    let bytes = tzif(&transitions, &[EST, EDT], None);
    let table = Table::from_tzif(&bytes).unwrap();
    let tr = table.transitions.clone();
    for t in (tr[0]..tr[tr.len() - 1]).step_by(1_234_567) {
        let want = tr.iter().position(|&x| x > t).unwrap();
        assert_eq!(table.search(t), want, "at {t}");
    }
}

#[test]
fn reading_a_file_with_no_transitions_sets_rule_dstoff() {
    let _p = fresh_process();
    Table::from_tzif(&new_york_2020()).unwrap();
    assert_eq!(RULE_DSTOFF.load(Ordering::Relaxed), 0);
    Table::from_tzif(&tzif(&[], &[(-18_000, false, "EST", false, false)], None)).unwrap();
    assert_eq!(RULE_DSTOFF.load(Ordering::Relaxed), -18_000);
}

// --- __tzfile_default --------------------------------------------------------------

fn aaa3bbb() -> (Rule, Rule) {
    let std = Rule {
        name: b"AAA".to_vec(),
        offset: -10_800,
        ..Rule::default()
    };
    let dst = Rule {
        name: b"BBB".to_vec(),
        offset: -7200,
        ..Rule::default()
    };
    (std, dst)
}

#[test]
fn posixrules_is_re_anchored_as_glibc_does_it_in_a_fresh_process() {
    // Measured with glibc: under `TZ=AAA3BBB` New York's 2020 transitions
    // happen at 09:00Z (not 07:00Z) and 04:00Z (not 06:00Z) -- the first moved
    // by the standard offsets' difference, the second by the whole DST offset,
    // because `rule_dstoff` is still 0.
    let _p = fresh_process();
    let (std, dst) = aaa3bbb();
    let z = Engine::Table(Table::defaulted(&new_york_2020(), &std, &dst).unwrap());
    assert_eq!(at_utc(&z, 1_583_657_999), st(-10_800, false, "AAA"));
    assert_eq!(at_utc(&z, 1_583_658_000), st(-7200, true, "BBB"));
    assert_eq!(at_utc(&z, 1_604_203_199), st(-7200, true, "BBB"));
    // At the (moved) last transition the footer takes over, with its own
    // names and offsets: 04:00Z on November 1st is still EDT by the rule.
    assert_eq!(at_utc(&z, 1_604_203_200), st(-14_400, true, "EDT"));
    assert_eq!(RULE_DSTOFF.load(Ordering::Relaxed), -7200);
}

#[test]
fn a_second_default_in_the_same_process_sees_the_first_ones_rule_dstoff() {
    let _p = fresh_process();
    let (std, dst) = aaa3bbb();
    Table::defaulted(&new_york_2020(), &std, &dst).unwrap();
    // Now `rule_dstoff` is -7200, and the fall transition does not move.
    let z = Engine::Table(Table::defaulted(&new_york_2020(), &std, &dst).unwrap());
    assert_eq!(at_utc(&z, 1_604_210_399).2, "BBB");
}

#[test]
fn a_transition_given_in_ut_is_not_moved() {
    let _p = fresh_process();
    let bytes = tzif(
        &[(1_583_650_800, 1), (1_604_210_400, 0)],
        &[EST, (-14_400, true, "EDT", true, true)],
        None,
    );
    let (std, dst) = aaa3bbb();
    let z = Engine::Table(Table::defaulted(&bytes, &std, &dst).unwrap());
    assert_eq!(at_utc(&z, 1_583_650_800).2, "BBB");
}

#[test]
fn posixrules_with_one_type_cannot_stand_in() {
    let _p = fresh_process();
    let bytes = tzif(&[], &[EST], None);
    let (std, dst) = aaa3bbb();
    assert!(Table::defaulted(&bytes, &std, &dst).is_none());
    // ...but reading it still set `rule_dstoff`, as `__tzfile_read` does
    // before `__tzfile_default` looks at the type count.
    assert_eq!(RULE_DSTOFF.load(Ordering::Relaxed), -18_000);
}

// --- tzset_internal ------------------------------------------------------------------

fn name_at(zone: &Engine, t: i64) -> String {
    at_utc(zone, t).2
}

#[test]
fn unset_with_no_localtime_is_utc_named_utc() {
    let _p = fresh_process();
    let d = ZoneDir::new("unset");
    assert_eq!(name_at(&resolve(None, d.dir(), &d.missing()), 0), "UTC");
}

#[test]
fn empty_is_universal_and_a_bare_colon_is_utc() {
    let _p = fresh_process();
    let d = ZoneDir::new("empty");
    // No `Universal` file here (nor in WSL's Ubuntu), so it is the rule
    // `Universal`: UTC under that name. glibc: `TZ= date +%Z` is `Universal`.
    assert_eq!(
        name_at(&resolve(Some(b""), d.dir(), &d.missing()), 0),
        "Universal"
    );
    assert_eq!(
        name_at(&resolve(Some(b":"), d.dir(), &d.missing()), 0),
        "UTC"
    );
    // With the file, the file.
    d.put(
        "Universal",
        &tzif(&[], &[(0, false, "UTC", false, false)], Some("UTC0")),
    );
    assert_eq!(
        name_at(&resolve(Some(b""), d.dir(), &d.missing()), 0),
        "UTC"
    );
}

#[test]
fn a_file_is_tried_before_a_rule_of_the_same_name() {
    let _p = fresh_process();
    let d = ZoneDir::new("file-first");
    // A file named like a rule, whose content disagrees with the rule.
    d.put(
        "EST5EDT",
        &tzif(&[], &[(-18_000, false, "XST", false, false)], None),
    );
    let summer = utc(2020, 7, 1, 0, 0, 0);
    for tz in [&b"EST5EDT"[..], b":EST5EDT"] {
        assert_eq!(
            name_at(&resolve(Some(tz), d.dir(), &d.missing()), summer),
            "XST"
        );
    }
    // Without the file, the rule.
    let other = ZoneDir::new("file-first-absent");
    assert_eq!(
        name_at(
            &resolve(Some(b"EST5EDT"), other.dir(), &other.missing()),
            summer
        ),
        "EDT"
    );
}

#[test]
fn naming_the_default_file_that_is_missing_is_utc() {
    let _p = fresh_process();
    let d = ZoneDir::new("default-named");
    let missing = d.missing();
    let name = path_bytes(&missing);
    assert_eq!(name_at(&resolve(Some(&name), d.dir(), &missing), 0), "UTC");
}

#[test]
fn a_rule_with_no_dates_borrows_posixrules_from_the_zone_directory() {
    let _p = fresh_process();
    let d = ZoneDir::new("posixrules");
    d.put(TZDEFRULES, &new_york_2020());
    let z = resolve(Some(b"AAA3BBB"), d.dir(), &d.missing());
    assert!(matches!(z, Engine::Table(_)));
    assert_eq!(name_at(&z, 1_583_658_000), "BBB");
    assert_eq!(name_at(&z, 1_583_657_999), "AAA");
}

#[test]
fn a_dotdot_name_is_never_read_and_falls_through_to_the_rule() {
    let _p = fresh_process();
    let d = ZoneDir::new("dotdot");
    d.put(
        "zone",
        &tzif(&[], &[(3600, false, "ONE", false, false)], None),
    );
    let sub = d.0.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let z = resolve(Some(b"../zone"), &sub, &d.missing());
    assert_eq!(at_utc(&z, 0), st(0, false, ""));
}

#[cfg(unix)]
#[test]
fn an_absolute_name_is_read_as_given() {
    let _p = fresh_process();
    let d = ZoneDir::new("absolute");
    d.put(
        "zone",
        &tzif(&[], &[(3600, false, "ONE", false, false)], None),
    );
    let path = d.0.join("zone");
    let z = resolve(
        Some(&path_bytes(&path)),
        Path::new("/nonexistent"),
        &d.missing(),
    );
    assert_eq!(at_utc(&z, 0), st(3600, false, "ONE"));
}

#[test]
fn a_file_that_is_not_tzif_falls_through_to_the_rule() {
    let _p = fresh_process();
    let d = ZoneDir::new("not-tzif");
    d.put("EST5", b"not a zone file");
    assert_eq!(
        at_utc(&resolve(Some(b"EST5"), d.dir(), &d.missing()), 0),
        st(-18_000, false, "EST")
    );
}

#[test]
fn a_nul_ends_the_value_as_it_ends_a_c_string() {
    let _p = fresh_process();
    let d = ZoneDir::new("nul");
    assert_eq!(
        at_utc(&resolve(Some(b"EST5\0junk"), d.dir(), &d.missing()), 0),
        st(-18_000, false, "EST")
    );
}

#[test]
fn a_long_name_is_cut_where_it_meets_tzinfo() {
    let z = rules("<ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij>5");
    let s = z.state(0).unwrap();
    assert_eq!(s.name.len(), 36);
    assert_eq!(s.info().name.as_bytes().len(), tzrules::TZ_NAME_CAP);
}

// --- Reading again, as glibc does -------------------------------------------------

/// New York's 2020 transitions and 2021's spring one, so that the 2020 fall
/// transition is not the last and is looked up in the table.
fn new_york_2020_21() -> Vec<u8> {
    tzif(
        &[(1_583_650_800, 1), (1_604_210_400, 0), (1_615_705_200, 1)],
        &[EST, EDT],
        Some("EST5EDT,M3.2.0,M11.1.0"),
    )
}

fn zone_name(zone: &crate::Zone, t: i64) -> Vec<u8> {
    zone.lookup(t).name.as_bytes().to_vec()
}

#[test]
fn tzset_re_reads_a_posixrules_zone_and_moves_its_fall_transitions() {
    // glibc: the first conversion in a process anchors `posixrules` with
    // `rule_dstoff` still 0; `mktime`'s `tzset ()` then re-reads it, now with
    // the user's DST offset, and the fall transitions move back to 06:00Z.
    let _p = fresh_process();
    let d = ZoneDir::new("tzset-stale");
    d.put(TZDEFRULES, &new_york_2020_21());
    let zone = crate::Zone::resolve(Some(b"AAA3BBB"), d.dir(), &d.missing());
    assert_eq!(zone_name(&zone, 1_604_203_199), b"BBB");
    assert_eq!(zone_name(&zone, 1_604_203_200), b"AAA");
    zone.tzset();
    assert_eq!(zone_name(&zone, 1_604_203_200), b"BBB");
    assert_eq!(zone_name(&zone, 1_604_210_399), b"BBB");
    assert_eq!(zone_name(&zone, 1_604_210_400), b"AAA");
}

#[test]
fn tzset_leaves_a_zone_that_is_not_from_posixrules_alone() {
    // Year 0's first lookup answers from the `memset`'s change times (see the
    // `EST5x` test above); a `tzset ()` that re-read the rule would reset that
    // cache and give the same answer again. glibc's does not re-read, and
    // neither does this.
    let _p = fresh_process();
    let d = ZoneDir::new("tzset-fresh");
    let zone = crate::Zone::resolve(Some(b"EST5x"), d.dir(), &d.missing());
    let year_zero = utc(0, 6, 1, 0, 0, 0);
    assert_eq!(zone_name(&zone, year_zero), b"EST");
    zone_name(&zone, utc(2020, 6, 1, 0, 0, 0));
    zone.tzset();
    assert_eq!(zone_name(&zone, year_zero), b"");
    // A switch of `TZ` to it does re-read it, and the cache starts over.
    zone.reread();
    assert_eq!(zone_name(&zone, year_zero), b"EST");
}

#[test]
fn a_switch_reads_the_other_zone_then_the_processs_own_again() {
    // The process runs under `AAA3BBB`, and one conversion happens under
    // `CCC4DDD` (DST offset -3 hours): gnulib's `set_tz` reads CCC4DDD, then
    // `revert_tz` reads AAA3BBB again. Each read is anchored by the DST offset
    // the previous one left.
    let _p = fresh_process();
    let d = ZoneDir::new("switched");
    d.put(TZDEFRULES, &new_york_2020_21());
    let process = crate::Zone::resolve(Some(b"AAA3BBB"), d.dir(), &d.missing());
    let other = crate::Zone::resolve(Some(b"CCC4DDD"), d.dir(), &d.missing());
    // First read of the process zone: anchored by 0, leaves -7200.
    assert_eq!(zone_name(&process, 1_604_203_200), b"AAA");
    // CCC4DDD read with -7200: its fall transition moves by -10800 - -7200.
    let fall = other.switched(&process, || {
        (
            zone_name(&other, 1_604_206_799),
            zone_name(&other, 1_604_206_800),
        )
    });
    assert_eq!(fall, (b"DDD".to_vec(), b"CCC".to_vec()));
    // The process zone re-read with -10800: moved by -7200 - -10800.
    assert_eq!(zone_name(&process, 1_604_213_999), b"BBB");
    assert_eq!(zone_name(&process, 1_604_214_000), b"AAA");
    // The same value is no switch at all, and reads nothing.
    let same = crate::Zone::resolve(Some(b"AAA3BBB"), d.dir(), &d.missing());
    assert!(same.same_tz(&process));
    let before = RULE_DSTOFF.load(Ordering::Relaxed);
    same.switched(&process, || ());
    assert_eq!(RULE_DSTOFF.load(Ordering::Relaxed), before);
}
