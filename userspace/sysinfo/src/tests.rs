//! Tests for the formatting half of `sysinfo`.
//!
//! The reading and parsing are `procinfo`'s and are tested there. What is
//! tested here is what this program actually decides: how wide a column is,
//! whether a value is ever cut to fit one, what a missing field prints, what
//! JSON is emitted for bytes that are not text, and whether a read *error*
//! ends up distinguishable from a read that found nothing.
//!
//! Every one of those was a defect in the version this file replaced, so each
//! test below names the behaviour it pins rather than the function it calls.

// Tests are allowed to panic on bad data: that is what a failing test is.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{
    Row, Status, column_width, display_width, interface_rows, json_bytes, json_number, mount_table,
    push_json_char,
};
use procinfo::{Mount, NetDevice};
use std::io;

/// The bytes of a row, for comparison against an expected layout.
fn bytes(row: &Row) -> &[u8] {
    &row.0
}

/// The bytes of a row, when the test's expectation is text.
fn text(row: &Row) -> String {
    String::from_utf8(row.0.clone()).expect("row was expected to be text")
}

fn mount(device: &[u8], point: &[u8], fstype: &[u8], options: &[u8]) -> Mount {
    Mount {
        device: device.to_vec(),
        mount_point: point.to_vec(),
        fstype: fstype.to_vec(),
        options: options.to_vec(),
        dump: 0,
        pass: 0,
    }
}

// ============================================================================
// Widths
// ============================================================================

#[test]
fn width_of_utf8_is_counted_in_characters_not_bytes() {
    // Two characters, five bytes. Padding by byte count would push every later
    // column three places left on any row containing this device name.
    assert_eq!(display_width("é€".as_bytes()), 2);
    assert_eq!("é€".len(), 5);
}

#[test]
fn width_of_bytes_that_are_not_utf8_falls_back_to_byte_count() {
    // There is no character count for bytes that are not characters. The byte
    // count is wrong for display but is finite and monotonic, which is all the
    // column arithmetic needs; the alternative is refusing to print the row.
    assert_eq!(display_width(b"\xff\xfe"), 2);
}

#[test]
fn a_column_is_at_least_as_wide_as_its_heading() {
    let values: Vec<&[u8]> = vec![b"a", b"bb"];
    assert_eq!(column_width("Device", values.into_iter()), "Device".len());
}

#[test]
fn a_column_widens_to_its_widest_value() {
    let values: Vec<&[u8]> = vec![b"/", b"/very/long/mount/point", b"/tmp"];
    assert_eq!(
        column_width("Mount", values.into_iter()),
        "/very/long/mount/point".len()
    );
}

#[test]
fn a_column_with_no_values_is_the_heading_width() {
    let values: Vec<&[u8]> = Vec::new();
    assert_eq!(column_width("Type", values.into_iter()), 4);
}

// ============================================================================
// Rows
// ============================================================================

#[test]
fn padding_never_truncates_a_value_wider_than_its_column() {
    // The rule that replaces `&parts[3][..20]`. A row one column too wide is a
    // cosmetic defect; a mount point cut at byte 20 is a wrong answer, and if
    // byte 20 is inside a multi-byte character the old code did not print a
    // wrong answer at all -- it panicked.
    let mut row = Row::new();
    row.padded(b"/mnt/considerably-longer-than-the-column", 4);
    assert_eq!(text(&row), "/mnt/considerably-longer-than-the-column");
}

#[test]
fn padding_pads_to_the_column_width() {
    let mut row = Row::new();
    row.padded(b"ext4", 8).text("|");
    assert_eq!(text(&row), "ext4    |");
}

#[test]
fn a_row_carries_bytes_that_are_not_utf8_through_unchanged() {
    // A mount point is a path, and a SlateOS path is any bytes but `/` and NUL.
    // Nothing between reading it and writing it may assume otherwise.
    let mut row = Row::new();
    row.text("  ").raw(b"/mnt/\xff\xfe").text("!");
    assert_eq!(bytes(&row), b"  /mnt/\xff\xfe!");
}

#[test]
fn padding_a_non_utf8_value_uses_its_byte_width() {
    let mut row = Row::new();
    row.padded(b"\xff\xfe", 5).text("|");
    assert_eq!(bytes(&row), b"\xff\xfe   |");
}

// ============================================================================
// The mount table
// ============================================================================

#[test]
fn the_mount_table_aligns_every_column_against_the_widest_row() {
    let mounts = vec![
        mount(b"/dev/sda1", b"/", b"ext4", b"rw,relatime"),
        mount(
            b"/dev/disk/by-uuid/0123456789abcdef",
            b"/home",
            b"ext4",
            b"rw",
        ),
    ];
    let rows = mount_table(&mounts);
    assert_eq!(rows.len(), 3);

    // Every row's `Type` column must begin at the same offset -- which is the
    // property the old code lost by padding the device field to a constant 20
    // while letting a longer device name run past it.
    let type_offsets: Vec<usize> = rows
        .iter()
        .map(|row| {
            let line = text(row);
            line.find("ext4").or_else(|| line.find("Type")).unwrap()
        })
        .collect();
    assert_eq!(type_offsets[0], type_offsets[1]);
    assert_eq!(type_offsets[1], type_offsets[2]);
}

#[test]
fn the_mount_table_never_cuts_the_options() {
    // Options are last and unbounded on purpose: they are the field with no
    // useful upper bound, and a reader scans them rather than aligning them.
    let long = b"rw,relatime,seclabel,attr2,inode64,logbufs=8,logbsize=32k,noquota";
    let mounts = vec![mount(b"/dev/sda1", b"/", b"xfs", long)];
    let rows = mount_table(&mounts);
    assert!(text(&rows[1]).ends_with(std::str::from_utf8(long).unwrap()));
}

#[test]
fn a_mount_point_that_is_not_utf8_still_produces_a_row() {
    let mounts = vec![mount(b"/dev/sdb1", b"/mnt/\xff\xfe", b"ext4", b"rw")];
    let rows = mount_table(&mounts);
    assert!(
        bytes(&rows[1])
            .windows(2)
            .any(|pair| pair == b"\xff\xfe".as_slice())
    );
}

#[test]
fn a_mount_table_with_no_mounts_is_still_a_heading() {
    let rows = mount_table(&[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0]), "  Mount Device Type Options");
}

// ============================================================================
// Interfaces
// ============================================================================

fn device(name: &[u8], rx: Option<u64>, tx: Option<u64>) -> NetDevice {
    NetDevice {
        name: name.to_vec(),
        rx_bytes: rx,
        rx_packets: None,
        tx_bytes: tx,
        tx_packets: None,
    }
}

#[test]
fn interface_names_are_padded_to_the_longest_name() {
    let devices = vec![
        device(b"lo", Some(1), Some(2)),
        device(b"enp0s31f6", Some(3), Some(4)),
    ];
    let rows = interface_rows(&devices);
    let first = text(&rows[0]);
    let second = text(&rows[1]);
    assert_eq!(first.find("RX:"), second.find("RX:"));
}

#[test]
fn an_interface_with_no_counters_says_so_rather_than_reporting_zero() {
    // `None` is "the kernel did not write this column", which is not the same
    // claim as "this interface has moved no bytes".
    let devices = vec![device(b"eth0", None, None)];
    let rows = interface_rows(&devices);
    assert!(text(&rows[0]).contains("(counters not reported)"));
    // No counter line at all, rather than a counter line reading zero.
    assert!(!text(&rows[0]).contains("RX:"));
}

// ============================================================================
// JSON
// ============================================================================

#[test]
fn a_missing_number_is_null_and_not_zero() {
    assert_eq!(json_number(None), "null");
    assert_eq!(json_number(Some(0)), "0");
    // The two must not collide: a report saying the machine has 0 kB of memory
    // is a false statement, where `null` is a true one.
    assert_ne!(json_number(None), json_number(Some(0)));
}

#[test]
fn a_missing_string_is_null_and_not_an_empty_string() {
    assert_eq!(json_bytes(None), "null");
    assert_eq!(json_bytes(Some(b"")), "\"\"");
}

#[test]
fn json_escapes_the_two_characters_that_would_end_the_string() {
    assert_eq!(json_bytes(Some(br#"a"b\c"#)), r#""a\"b\\c""#);
}

#[test]
fn json_escapes_every_control_character_not_just_the_familiar_three() {
    // RFC 8259 section 7: U+0000 through U+001F must be escaped. The escaper
    // this replaces handled \n, \r and \t and passed the rest through raw,
    // which produced output no strict parser would accept.
    let value = json_bytes(Some(b"a\x01b\x1fc"));
    assert_eq!(value, "\"a\\u0001b\\u001fc\"");
    assert_eq!(json_bytes(Some(b"\n\r\t")), "\"\\n\\r\\t\"");
}

#[test]
fn json_escapes_bytes_that_are_not_utf8_one_byte_at_a_time() {
    // Not a faithful round-trip -- JSON has none for arbitrary bytes -- but
    // every byte is present and distinguishable, where `from_utf8_lossy` would
    // collapse the pair into a single U+FFFD and lose the count.
    assert_eq!(json_bytes(Some(b"x\xff\xfe")), "\"x\\u00ff\\u00fe\"");
}

#[test]
fn json_leaves_ordinary_text_alone() {
    assert_eq!(
        json_bytes(Some("Intel Core i7 é".as_bytes())),
        "\"Intel Core i7 é\""
    );
}

#[test]
fn push_json_char_is_the_one_place_escaping_is_decided() {
    // Both branches of `json_bytes` route through it, so the UTF-8 and the
    // non-UTF-8 path cannot disagree about how a quote is escaped.
    let mut out = String::new();
    push_json_char(&mut out, '"');
    push_json_char(&mut out, 'a');
    push_json_char(&mut out, '\u{7}');
    assert_eq!(out, "\\\"a\\u0007");
}

// ============================================================================
// Absence versus failure
// ============================================================================

#[test]
fn a_value_is_returned_and_nothing_is_recorded() {
    let mut status = Status::default();
    let got = status.take("/proc/meminfo", io::Result::Ok(Some(7u32)));
    assert_eq!(got, Some(7));
    assert!(!status.failed);
}

#[test]
fn a_file_the_kernel_does_not_export_is_an_answer_not_a_fault() {
    // `Ok(None)` means "this kernel has no such file", which `sysinfo` prints
    // as "(not available)" and exits 0 for. Nothing failed.
    let mut status = Status::default();
    let got: Option<u32> = status.take("/proc/pressure/cpu", io::Result::Ok(None));
    assert_eq!(got, None);
    assert!(!status.failed);
}

#[test]
fn a_file_that_could_not_be_read_makes_the_process_exit_non_zero() {
    // The defect this pins: `fs::read_to_string(p).ok()` turned a permission
    // error into the same `None` as an absent file, and the program exited 0
    // either way -- so a sysinfo run that could read nothing at all was
    // indistinguishable, to a script, from one that described the machine.
    let mut status = Status::default();
    let denied = io::Error::from(io::ErrorKind::PermissionDenied);
    let got: Option<u32> = status.take("/proc/meminfo", Err(denied));
    assert_eq!(got, None);
    assert!(status.failed);
}

#[test]
fn one_failure_among_many_reads_is_enough_to_fail_the_run() {
    let mut status = Status::default();
    let _ = status.take("a", io::Result::Ok(Some(1u32)));
    let _ = status.take(
        "b",
        io::Result::<Option<u32>>::Err(io::Error::other("boom")),
    );
    let _ = status.take("c", io::Result::Ok(Some(3u32)));
    assert!(
        status.failed,
        "a later success must not clear an earlier failure"
    );
}
