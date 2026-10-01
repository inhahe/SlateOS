//! Unit tests of the pieces that do not read a system; what the program
//! prints is `scripts/lsblk-diff.sh`'s to compare with upstream's.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests unwrap and index what they built"
)]

use super::*;

#[test]
fn columns_are_upstreams_in_upstreams_order() {
    assert_eq!(COLS.len(), 69);
    assert_eq!(info(COLS[0]).name, "ALIGNMENT");
    assert_eq!(info(Col::Majmin).name, "MAJ:MIN");
    assert_eq!(info(Col::Targets).name, "MOUNTPOINTS");
    assert_eq!(info(COLS[68]).name, "ZONE-AMAX");
    // Every name is unique, so a name finds one column.
    for (i, &a) in COLS.iter().enumerate() {
        for &b in &COLS[i + 1..] {
            assert_ne!(info(a).name, info(b).name);
        }
    }
    assert_eq!(MAX_COLUMNS, 138);
}

#[test]
fn a_column_is_named_whole_in_any_case() {
    assert_eq!(
        column_name_to_id(b"maj:min", b"maj:min", b"lsblk"),
        Some(Col::Majmin)
    );
    assert_eq!(
        column_name_to_id(b"NAME", b"NAME", b"lsblk"),
        Some(Col::Name)
    );
    assert_eq!(column_name_to_id(b"NAM", b"NAM", b"lsblk"), None);
    assert_eq!(
        column_name_to_id(b"FSUSE%", b"FSUSE%", b"lsblk"),
        Some(Col::Fsuseperc)
    );
}

#[test]
fn major_lists_parse_as_strtoul_does() {
    let mut v = Vec::new();
    parse_majors(b"8,253", &mut v, "excluded", b"lsblk").unwrap();
    assert_eq!(v, [8, 253]);
    // A trailing comma ends the list; the rest is refused.
    let mut v = Vec::new();
    parse_majors(b"7,", &mut v, "excluded", b"lsblk").unwrap();
    assert_eq!(v, [7]);
    assert!(parse_majors(b"7,,8", &mut Vec::new(), "excluded", b"lsblk").is_err());
    assert!(parse_majors(b"x", &mut Vec::new(), "excluded", b"lsblk").is_err());
    assert!(parse_majors(b"8x", &mut Vec::new(), "excluded", b"lsblk").is_err());
    assert!(
        parse_majors(
            b"99999999999999999999",
            &mut Vec::new(),
            "excluded",
            b"lsblk"
        )
        .is_err()
    );
    // `-1` is ULONG_MAX, stored in an int: -1 again.
    let mut v = Vec::new();
    parse_majors(b"-1", &mut v, "excluded", b"lsblk").unwrap();
    assert_eq!(v, [-1]);
    // An empty list is no list, and no error.
    let mut v = Vec::new();
    parse_majors(b"", &mut v, "excluded", b"lsblk").unwrap();
    assert!(v.is_empty());
    // 256 entries is one too many.
    let long: Vec<u8> = (0..256)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
        .into_bytes();
    assert!(parse_majors(&long, &mut Vec::new(), "excluded", b"lsblk").is_err());
}

#[test]
fn sort_numbers_are_whole_decimals() {
    let mut d = 7u64;
    str2u64(Some(b"42"), &mut d);
    assert_eq!(d, 42);
    str2u64(Some(b"4x"), &mut d);
    assert_eq!(d, 42);
    str2u64(Some(b""), &mut d);
    str2u64(None, &mut d);
    assert_eq!(d, 42);
    str2u64(Some(b" 5"), &mut d);
    assert_eq!(d, 5);
    str2u64(Some(b"99999999999999999999"), &mut d);
    assert_eq!(d, 5);
}

#[test]
fn a_percentage_is_rounded_as_printf_rounds_it() {
    assert_eq!(percent_0f(12.4), "12");
    assert_eq!(percent_0f(12.6), "13");
    // Ties go to the even neighbour.
    assert_eq!(percent_0f(12.5), "12");
    assert_eq!(percent_0f(13.5), "14");
    assert_eq!(percent_0f(0.0), "0");
    assert_eq!(percent_0f(100.0), "100");
}

#[test]
fn modes_are_ls_letters() {
    assert_eq!(xstrmode(0o060_660), b"brw-rw----");
    assert_eq!(xstrmode(0o020_620), b"crw--w----");
    assert_eq!(xstrmode(0o104_755), b"-rwsr-xr-x");
    assert_eq!(xstrmode(0o041_777), b"drwxrwxrwt");
    assert_eq!(xstrmode(0o102_644), b"-rw-r-Sr--");
}

#[test]
fn numbered_names_scan_as_sscanf() {
    assert_eq!(scan_majmin(b"8:0"), Some((8, 0)));
    assert_eq!(scan_majmin(b"259:12"), Some((259, 12)));
    assert_eq!(scan_majmin(b"8"), None);
    assert_eq!(scan_majmin(b"x:1"), None);
}

#[test]
fn u64_cells_compare_with_no_number_first() {
    let none = CellView::default();
    assert_eq!(cmp_u64_cells(none, none), 0);
}

#[test]
fn usage_lists_every_column() {
    let u = String::from_utf8(usage(b"lsblk")).unwrap();
    assert!(u.starts_with("\nUsage:\n lsblk [options] [<device> ...]\n"));
    assert!(u.contains("\n    ALIGNMENT  alignment offset\n"));
    // `%-22s`: the 11-byte option padded with 11 blanks.
    assert!(u.contains(" -h, --help           display this help\n"));
    assert!(u.ends_with("\nFor more details see lsblk(8).\n"));
}

#[cfg(unix)]
#[test]
fn a_partition_links_name_its_disk() {
    let dir = scratchdir::ScratchDir::new("lsblk-holder");
    std::os::unix::fs::symlink(
        "../../../../devices/pci0000:00/block/sda/sda2",
        dir.path("sda2"),
    )
    .unwrap();
    let d = quoting::os_bytes(dir.path("").as_os_str()).into_owned();
    let d = d.strip_suffix(b"/").unwrap_or(&d).to_vec();
    assert_eq!(
        get_wholedisk_from_partition_dirent(&d, b"sda2"),
        Some(b"sda".to_vec())
    );
    assert_eq!(get_wholedisk_from_partition_dirent(&d, b"none"), None);
}
