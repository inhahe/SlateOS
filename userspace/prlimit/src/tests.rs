//! The pieces of `prlimit.c`, each against what util-linux 2.39.3 does with
//! the same input. The program as a whole is compared with upstream's by
//! `scripts/prlimit-diff.sh`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

const BOTH: u8 = PRLIMIT_SOFT | PRLIMIT_HARD;

#[test]
fn a_limit_string_is_read_as_get_range_reads_it() {
    assert_eq!(
        get_range(b"unlimited"),
        Ok((RLIM_INFINITY, RLIM_INFINITY, BOTH))
    );
    assert_eq!(get_range(b"10"), Ok((10, 10, BOTH)));
    assert_eq!(get_range(b"10:20"), Ok((10, 20, BOTH)));
    assert_eq!(get_range(b"10:"), Ok((10, 10, PRLIMIT_SOFT)));
    assert_eq!(get_range(b":20"), Ok((RLIM_INFINITY, 20, PRLIMIT_HARD)));
    assert_eq!(
        get_range(b":unlimited"),
        Ok((RLIM_INFINITY, RLIM_INFINITY, PRLIMIT_HARD))
    );
    assert_eq!(get_range(b"5:unlimited"), Ok((5, RLIM_INFINITY, BOTH)));
    assert_eq!(
        get_range(b"unlimited:"),
        Ok((RLIM_INFINITY, RLIM_INFINITY, PRLIMIT_SOFT))
    );
    assert_eq!(get_range(b"unlimited:7"), Ok((RLIM_INFINITY, 7, BOTH)));
}

#[test]
fn upstreams_quirks_are_kept() {
    // Nothing checks what follows a lone value...
    assert_eq!(get_range(b"10abc"), Ok((10, 10, BOTH)));
    // ...or what follows "unlimited" unless it is a colon.
    assert_eq!(
        get_range(b"unlimitedfoo"),
        Ok((RLIM_INFINITY, RLIM_INFINITY, BOTH))
    );
    // strtoull: a minus sign wraps, so -1 is unlimited.
    assert_eq!(get_range(b"-1"), Ok((RLIM_INFINITY, RLIM_INFINITY, BOTH)));
    assert_eq!(get_range(b" +5"), Ok((5, 5, BOTH)));
}

#[test]
fn a_limit_string_is_refused_where_upstream_refuses_it() {
    for bad in [
        &b""[..],
        b"abc",
        b":",
        b":x",
        b":5x",
        b"5:x",
        b"5:5x",
        b"99999999999999999999",
    ] {
        assert_eq!(get_range(bad), Err(()), "{bad:?}");
    }
}

#[test]
fn an_option_value_loses_one_leading_equals_sign() {
    let mut lim = Prlimit {
        desc: 9,
        cur: 0,
        max: 0,
        modify: 0,
    };
    parse_prlim(&mut lim, b"=5:6", b"prlimit").unwrap();
    assert_eq!((lim.cur, lim.max, lim.modify), (5, 6, BOTH));
    // Only one: `--nofile==5` is `=5`, then `5`.
    parse_prlim(&mut lim, b"=:7", b"prlimit").unwrap();
    assert_eq!((lim.max, lim.modify), (7, PRLIMIT_HARD));
    assert_eq!(parse_prlim(&mut lim, b"==5", b"prlimit"), Err(1));
}

#[test]
fn columns_are_named_in_any_case_and_listed_in_enum_order() {
    assert_eq!(
        column_name_to_id(b"soft", b"soft", b"prlimit"),
        Some(Col::Soft)
    );
    assert_eq!(
        column_name_to_id(b"Description", b"Description", b"prlimit"),
        Some(Col::Help)
    );
    assert_eq!(column_name_to_id(b"RES", b"RES", b"prlimit"), None);
    let text = String::from_utf8(usage(b"prlimit")).unwrap();
    let desc = text.find(" DESCRIPTION  resource description\n").unwrap();
    let res = text.find("    RESOURCE  resource name\n").unwrap();
    assert!(desc < res, "DESCRIPTION is COL_HELP, first in infos[]");
    assert!(text.starts_with("\nUsage:\n prlimit [options] [--<resource>=<limit>] [-p PID]\n"));
    assert!(text.ends_with("\nFor more details see prlimit(1).\n"));
}

#[test]
fn every_resource_has_its_short_option_and_linux_number() {
    let flags: Vec<u8> = PRLIMIT_DESC.iter().map(|d| d.flag).collect();
    assert_eq!(flags, b"vctdfxlqenumryis");
    let mut numbers: Vec<i32> = PRLIMIT_DESC.iter().map(|d| d.resource).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, (0..16).collect::<Vec<i32>>());
}

#[test]
fn a_limit_prints_as_a_number_or_unlimited() {
    assert_eq!(limit_text(0), "0");
    assert_eq!(limit_text(1_048_576), "1048576");
    assert_eq!(limit_text(RLIM_INFINITY), "unlimited");
}

#[test]
fn the_table_has_upstreams_columns() {
    let lims = [
        Prlimit {
            desc: 9,
            cur: 1024,
            max: 4096,
            modify: 0,
        },
        Prlimit {
            desc: 8,
            cur: 0,
            max: 0,
            modify: 0,
        },
    ];
    let cols = [Col::Res, Col::Help, Col::Soft, Col::Hard, Col::Units];
    let text = String::from_utf8(show_limits(&lims, &cols, true, false)).unwrap();
    assert_eq!(
        text,
        "RESOURCE DESCRIPTION SOFT HARD UNITS\n\
         NOFILE max\\x20number\\x20of\\x20open\\x20files 1024 4096 files\n\
         NICE max\\x20nice\\x20prio\\x20allowed\\x20to\\x20raise 0 0 \n"
    );
}
