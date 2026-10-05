//! `tz_plan`, case by case against glibc 2.39 -- where a comment gives what
//! `date` printed for a case, it was measured under WSL's glibc at
//! 1990-03-20 12:00 UTC.

use super::*;

fn plan(file: Option<ZoneFile<'static>>, rule: Option<&'static [u8]>) -> TzPlan<'static> {
    TzPlan { file, rule }
}

fn rule(text: &[u8]) -> Tz {
    Tz::parse(text).unwrap()
}

/// Unset is the machine's own zone, or UTC without it; naming that file
/// outright is the same thing.
#[test]
fn unset_is_the_machines_zone_and_so_is_naming_its_file() {
    let system = plan(Some(ZoneFile::System), None);
    assert_eq!(tz_plan(None), system);
    assert_eq!(tz_plan(Some(b"/etc/localtime")), system);
    assert_eq!(tz_plan(Some(b":/etc/localtime")), system);
    assert_eq!(system.fallback(), Tz::utc());
    // Only that path: another way of spelling it is just a path.
    assert_eq!(
        tz_plan(Some(b"/etc//localtime")).file(),
        Some(ZoneFile::Path(b"/etc//localtime"))
    );
}

/// Empty is the name `Universal` -- a file, then a rule that is not one, so
/// UTC (glibc: `Universal`) -- and a bare `:` is UTC with nothing to read
/// (glibc: `UTC`).
#[test]
fn empty_is_universal_and_a_bare_colon_is_utc() {
    let empty = tz_plan(Some(b""));
    assert_eq!(
        empty,
        plan(Some(ZoneFile::Named(b"Universal")), Some(b"Universal"))
    );
    assert_eq!(empty.fallback(), Tz::utc());
    let colon = tz_plan(Some(b":"));
    assert_eq!(colon, plan(None, None));
    assert_eq!(colon.fallback(), Tz::utc());
}

/// `EST5EDT` is a file and a rule: the file is tried first, and the rule
/// stands in only without it (glibc: `07:00 EST`, by the file; the rule says
/// `08:00 EDT`). A `:` changes nothing.
#[test]
fn a_file_is_tried_before_a_rule_of_the_same_name() {
    for tz in [&b"EST5EDT"[..], b":EST5EDT"] {
        let p = tz_plan(Some(tz));
        assert_eq!(
            p,
            plan(Some(ZoneFile::Named(b"EST5EDT")), Some(b"EST5EDT")),
            "{tz:?}"
        );
        assert_eq!(p.fallback(), rule(b"EST5EDT"), "{tz:?}");
    }
}

/// A rule with dates is looked for as a file too -- glibc looks, and finds
/// none -- and is the rule when there is none.
#[test]
fn a_rule_stands_in_for_a_file_that_is_not_there() {
    let text = b"CET-1CEST,M3.5.0,M10.5.0/3";
    let p = tz_plan(Some(text));
    assert_eq!(p.file(), Some(ZoneFile::Named(text)));
    assert_eq!(p.rule(), Some(&text[..]));
    assert_eq!(p.fallback(), rule(text));
}

/// One `:` is dropped and no more: in `::EST5EDT` the second is part of a
/// name no rule has (glibc: UTC, unnamed).
#[test]
fn only_one_colon_is_dropped() {
    let p = tz_plan(Some(b"::EST5EDT"));
    assert_eq!(
        p,
        plan(Some(ZoneFile::Named(b":EST5EDT")), Some(b":EST5EDT"))
    );
    assert_eq!(p.fallback(), Tz::utc());
}

/// A name is a file under the zoneinfo directory and a path is a path, `:`
/// or not; neither is a rule, so without the file each is UTC (glibc gives
/// `TZ=Foo/Bar` the name `Foo`, which this crate's engine does not).
#[test]
fn a_name_is_a_file_under_the_zoneinfo_directory_and_a_path_is_a_path() {
    let p = tz_plan(Some(b"America/New_York"));
    assert_eq!(p.file(), Some(ZoneFile::Named(b"America/New_York")));
    assert_eq!(p.fallback(), Tz::utc());
    for tz in [&b"/etc/zones/home"[..], b":/etc/zones/home"] {
        let p = tz_plan(Some(tz));
        assert_eq!(p.file(), Some(ZoneFile::Path(b"/etc/zones/home")), "{tz:?}");
        assert_eq!(p.rule(), Some(&b"/etc/zones/home"[..]), "{tz:?}");
        assert_eq!(p.fallback(), Tz::utc(), "{tz:?}");
    }
}

/// A name that would leave the zoneinfo tree, or that a NUL would cut short,
/// is never opened; it goes on to the rule as a missing file does, and is
/// not one.
#[test]
fn a_name_that_escapes_or_is_cut_short_is_not_opened() {
    for (tz, name) in [
        (&b"../../../etc/shadow"[..], &b"../../../etc/shadow"[..]),
        (b":../x", b"../x"),
        (
            b"/usr/share/zoneinfo/../../etc/shadow",
            b"/usr/share/zoneinfo/../../etc/shadow",
        ),
        (b"Europe/..", b"Europe/.."),
        (b"a\0b", b"a\0b"),
    ] {
        let p = tz_plan(Some(tz));
        assert_eq!(p.file(), None, "{tz:?}");
        assert_eq!(p.rule(), Some(name), "{tz:?}");
        assert_eq!(p.fallback(), Tz::utc(), "{tz:?}");
    }
    // Two dots inside a component are a name, not a step up.
    assert_eq!(
        tz_plan(Some(b"Europe/Bu..dapest")).file(),
        Some(ZoneFile::Named(b"Europe/Bu..dapest"))
    );
}

/// The rule [`TzPlan::rule`] offers is what [`TzPlan::fallback`] reads: one
/// with a NUL in it is not a rule, even if what comes before the NUL is.
#[test]
fn a_nul_ends_no_rule_early() {
    let p = tz_plan(Some(b"JST-9\0junk"));
    assert_eq!(p.file(), None);
    assert_eq!(p.fallback(), Tz::utc());
    assert_eq!(tz_plan(Some(b"JST-9")).fallback(), rule(b"JST-9"));
}
