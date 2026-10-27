#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

#[test]
fn a_dump_shows_sixteen_bytes_a_line_and_a_run_of_equal_lines_as_a_star() {
    let zeros = [0u8; 32];
    assert_eq!(
        String::from_utf8(dump_buffer(0, &zeros)).unwrap(),
        "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00\n*\n"
    );
    let mut buf = [1u8; 48];
    buf[..16].fill(0);
    buf[47] = 0x55;
    assert_eq!(
        String::from_utf8(dump_buffer(0x1d0, &buf)).unwrap(),
        "000001d0  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00\n\
         000001e0  01 01 01 01 01 01 01 01  01 01 01 01 01 01 01 01\n\
         000001f0  01 01 01 01 01 01 01 01  01 01 01 01 01 01 01 55\n"
    );
    // A partial last line still ends its line.
    assert_eq!(
        String::from_utf8(dump_buffer(0, &[0xab, 0xcd])).unwrap(),
        "00000000  ab cd\n"
    );
}

#[test]
fn a_run_of_equal_lines_is_skipped_to_its_end() {
    let mut buf = [0u8; 64];
    assert_eq!(skip_empty(&buf, 0, 64), 64);
    buf[40] = 1;
    // Lines 0 and 1 are equal, line 2 is not: the run ends at line 2.
    assert_eq!(skip_empty(&buf, 0, 64), 32);
    // Nothing equal after it: no run.
    assert_eq!(skip_empty(&buf, 32, 64), 32);
}

#[test]
fn replies_are_yes_no_or_neither_by_their_first_letter() {
    assert_eq!(rpmatch(b"yes"), 1);
    assert_eq!(rpmatch(b"Y"), 1);
    assert_eq!(rpmatch(b"no"), 0);
    assert_eq!(rpmatch(b"N"), 0);
    assert_eq!(rpmatch(b""), -1);
    assert_eq!(rpmatch(b"maybe"), -1);
}

#[test]
fn a_prompt_is_cut_to_upstreams_buffer() {
    let long = vec![b'x'; 200];
    assert_eq!(prompt128(&long).len(), 127);
    assert_eq!(prompt128(b"short"), b"short");
    assert_eq!(tochar(1), 'a');
    assert_eq!(tochar(16), 'p');
}

#[test]
fn a_reply_loses_its_leading_blanks() {
    assert_eq!(ltrim_whitespace(b"  \t n\n"), b"n\n");
    assert_eq!(ltrim_whitespace(b""), b"");
}

#[test]
fn a_device_without_a_label_offers_the_generic_and_create_commands() {
    let cxt = Context::new();
    let mut mc = menu::tests_support::walk(&cxt);
    let keys: String = mc.drain(..).map(char::from).collect();
    assert_eq!(keys, "dFlnptvimuxIOwqgGos");
}

#[test]
fn usage_ends_with_the_columns_and_the_manual() {
    let text = String::from_utf8(usage(b"fdisk")).unwrap();
    assert!(text.starts_with("\nUsage:\n fdisk [options] <disk>         change partition table\n"));
    assert!(text.contains(&format!("{:<31}display this help\n", " -h, --help")));
    assert!(text.contains("\nAvailable output columns:\n gpt: Device Start End Sectors Size Type"));
    assert!(text.ends_with("\nFor more details see fdisk(8).\n"));
}
