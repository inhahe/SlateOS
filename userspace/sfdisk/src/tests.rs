#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

#[test]
fn a_reply_is_moved_over_its_blanks_by_only_as_many_bytes() {
    assert_eq!(reply_from_line(b"yes\n"), b"yes");
    // Two blanks: two bytes moved, the rest left where it was.
    assert_eq!(reply_from_line(b"  yes\n"), b"yeyes");
    // More blanks than text: the moved bytes take the string's end with
    // them, and the newline then goes as usual.
    assert_eq!(reply_from_line(b"     y\n"), b"y");
    assert_eq!(reply_from_line(b"\n"), b"");
    assert_eq!(reply_from_line(b"n"), b"n");
}

#[test]
fn replies_are_yes_no_or_neither() {
    assert_eq!(rpmatch(b"y"), 1);
    assert_eq!(rpmatch(b"No"), 0);
    assert_eq!(rpmatch(b"x"), -1);
    assert_eq!(rpmatch(b""), -1);
}

#[test]
fn basename_is_libgens() {
    assert_eq!(basename(b"/dev/sda"), b"sda");
    assert_eq!(basename(b"img"), b"img");
    assert_eq!(basename(b"dir/"), b"dir");
    assert_eq!(basename(b"/"), b"/");
    assert_eq!(basename(b""), b".");
}

#[test]
fn an_incomplete_or_empty_line_is_ignored() {
    let mut pa = Partition::new();
    // Neither a start nor a default for it.
    assert!(ignore_partition(&pa));
    pa.set_start_follow_default(true);
    pa.end_follow_default = true;
    assert!(!ignore_partition(&pa));
    // An old sfdisk's `start=0 size=0`.
    let mut old = Partition::new();
    old.set_start(0);
    old.set_size(0);
    assert!(ignore_partition(&old));
}

#[test]
fn usage_lists_the_commands_and_ends_with_the_manual() {
    let text = String::from_utf8(usage(b"sfdisk")).unwrap();
    assert!(text.starts_with("\nUsage:\n sfdisk [options] <dev> [[-N] <part>]\n"));
    assert!(text.contains(" -v, --version             display version\n"));
    assert!(text.contains("\nAvailable output columns:\n"));
    assert!(text.ends_with("\nFor more details see sfdisk(8).\n"));
}
