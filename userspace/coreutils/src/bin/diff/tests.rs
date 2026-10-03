//! Unit tests for the pieces of the port that are functions of their inputs.
//! The program as a whole is `scripts/diff-diff.sh`'s, against GNU diff 3.10;
//! every expectation below that is a diff was produced by that GNU diff.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

fn argv(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn opts(words: &[&str]) -> Opts {
    match parse_args(&argv(words)) {
        Ok(Parsed::Run(p)) => p.0,
        _ => panic!("{words:?} did not parse"),
    }
}

fn refusal(words: &[&str]) -> (String, bool) {
    match parse_args(&argv(words)) {
        Err(Refusal { message, referral }) => (
            String::from_utf8(message.unwrap_or_default()).unwrap(),
            referral,
        ),
        _ => panic!("{words:?} parsed"),
    }
}

/// Run the comparison and the printer on two texts, as `diff_2_files` does,
/// capturing nothing but the edit script.
fn script(a: &[u8], b: &[u8], words: &[&str]) -> Vec<(Lin, Lin, Lin, Lin)> {
    let o = opts(words);
    let mut files = [
        FileData::new(Desc::Nonexistent, b"a".to_vec()),
        FileData::new(Desc::Nonexistent, b"b".to_vec()),
    ];
    io::test_load(&mut files, a, b, &o);
    analyze::compare(&mut files, o.minimal, o.speed_large_files);
    // In the files' own line numbers, from 1, as the output prints them.
    analyze::build_script(&files)
        .into_iter()
        .map(|c| {
            (
                util::translate_line_number(&files[0], c.line0),
                c.deleted,
                util::translate_line_number(&files[1], c.line1),
                c.inserted,
            )
        })
        .collect()
}

#[test]
fn strtoimax_whole_reads_as_upstream_does() {
    assert_eq!(strtoimax_whole(b""), Some(0));
    assert_eq!(strtoimax_whole(b"5"), Some(5));
    assert_eq!(strtoimax_whole(b" +5"), Some(5));
    assert_eq!(strtoimax_whole(b"-1"), Some(-1));
    assert_eq!(strtoimax_whole(b"5x"), None);
    assert_eq!(strtoimax_whole(b"  "), None);
    assert_eq!(strtoimax_whole(b"99999999999999999999999"), Some(i64::MAX));
}

#[test]
fn digit_options_accumulate_across_words() {
    let o = opts(&["-u", "-1", "-2", "a", "b"]);
    assert_eq!(o.context, 12);
    let o = opts(&["-u", "-1", "-a", "-2", "a", "b"]);
    assert_eq!(o.context, 2);
}

#[test]
fn style_conflicts_and_bad_values_are_refused() {
    assert_eq!(
        refusal(&["-u", "-c", "a", "b"]),
        ("conflicting output style options".to_string(), true)
    );
    assert_eq!(
        refusal(&["-U", "x", "a", "b"]),
        ("invalid context length 'x'".to_string(), true)
    );
    assert_eq!(
        refusal(&["-W", "0", "a", "b"]),
        ("invalid width '0'".to_string(), true)
    );
    assert_eq!(
        refusal(&["-W", "5", "-W", "6", "a", "b"]),
        ("conflicting width options".to_string(), false)
    );
    assert_eq!(
        refusal(&["--tabsize=x", "a", "b"]),
        ("invalid tabsize 'x'".to_string(), true)
    );
    assert_eq!(
        refusal(&["-L", "1", "-L", "2", "-L", "3", "a", "b"]),
        ("too many file label options".to_string(), false)
    );
}

#[test]
fn the_sdiff_columns_are_upstreams() {
    let o = opts(&["-y", "a", "b"]);
    assert_eq!((o.sdiff_half_width, o.sdiff_column2_offset), (61, 64));
    let o = opts(&["-y", "-W", "60", "a", "b"]);
    assert_eq!((o.sdiff_half_width, o.sdiff_column2_offset), (28, 32));
    let o = opts(&["-y", "-t", "-W", "60", "a", "b"]);
    assert_eq!((o.sdiff_half_width, o.sdiff_column2_offset), (28, 32));
}

#[test]
fn lines_differ_follows_each_white_space_rule() {
    let o = opts(&["a", "b"]);
    assert!(util::lines_differ(b"a b\n", b"a  b\n", &o));
    let o = opts(&["-b", "a", "b"]);
    assert!(!util::lines_differ(b"a b\n", b"a  b\n", &o));
    assert!(!util::lines_differ(b"a b \n", b"a b\n", &o));
    assert!(util::lines_differ(b"ab\n", b"a b\n", &o));
    let o = opts(&["-w", "a", "b"]);
    assert!(!util::lines_differ(b"ab\n", b" a b \n", &o));
    let o = opts(&["-Z", "a", "b"]);
    assert!(!util::lines_differ(b"a  \n", b"a\n", &o));
    assert!(util::lines_differ(b" a\n", b"a\n", &o));
    let o = opts(&["-E", "a", "b"]);
    assert!(!util::lines_differ(b"\tx\n", b"        x\n", &o));
    assert!(util::lines_differ(b"\tx\n", b"       x\n", &o));
    let o = opts(&["-i", "a", "b"]);
    assert!(!util::lines_differ(b"ABC\n", b"abc\n", &o));
}

#[test]
fn c_escape_quotes_what_a_shell_would_misread() {
    assert_eq!(util::c_escape(b"plain"), b"plain");
    assert_eq!(util::c_escape(b"a b"), b"\"a b\"");
    assert_eq!(util::c_escape(b"a\tb"), b"\"a\\tb\"");
    assert_eq!(util::c_escape(b"caf\xc3\xa9"), b"\"caf\\303\\251\"");
}

/// GNU's choice where an LCS would choose differently: `a b a b b` against
/// `a b` deletes lines 3 to 5, which `shift_boundaries` slides there.
#[test]
fn the_edit_script_is_gnus() {
    // GNU: `3,5d2`.
    assert_eq!(
        script(b"a\nb\na\nb\nb\n", b"a\nb\n", &[]),
        vec![(3, 3, 3, 0)]
    );
    assert_eq!(script(b"a\nb\nc\n", b"a\nb\nc\n", &[]), vec![]);
    assert_eq!(script(b"a\n", b"b\n", &[]), vec![(1, 1, 1, 1)]);
}

/// Short lines outgrow the bucket table's first guess, of 32 bytes a line,
/// and it grows -- here from 4096 buckets twice, the second time while the
/// second file is being classed. Equal lines must still share a class across
/// the files, distinct
/// ones must not, and an incomplete last line must still stay apart from the
/// complete line it would otherwise equal.
#[test]
fn classes_survive_the_bucket_table_growing() {
    use std::collections::HashMap;
    let mut a = Vec::new();
    for i in 0..5000 {
        a.extend_from_slice(format!("{i}\n").as_bytes());
    }
    a.extend_from_slice(b"y\nx");
    let mut b = Vec::new();
    for i in (0..5000).rev() {
        b.extend_from_slice(format!("{i}\nn{i}\n").as_bytes());
    }
    b.extend_from_slice(b"x\ny");

    let o = opts(&["a", "b"]);
    let mut files = [
        FileData::new(Desc::Nonexistent, b"a".to_vec()),
        FileData::new(Desc::Nonexistent, b"b".to_vec()),
    ];
    io::test_load(&mut files, &a, &b, &o);
    let classes = |f: usize| -> Vec<(Vec<u8>, Lin)> {
        (0..files[f].buffered_lines)
            .map(|i| (files[f].line(i).to_vec(), files[f].equiv(i)))
            .collect()
    };
    let (in_a, in_b) = (classes(0), classes(1));
    assert_eq!((in_a.len(), in_b.len()), (5002, 10002));
    // Every class number is the class of one text only, in either file.
    let mut text_of: HashMap<Lin, &[u8]> = HashMap::new();
    for (text, class) in in_a.iter().chain(&in_b) {
        assert_ne!(*class, 0);
        assert_eq!(*text_of.entry(*class).or_insert(text), text.as_slice());
    }
    // Every text in both files has one class.
    let a_class: HashMap<&[u8], Lin> = in_a.iter().map(|(t, c)| (t.as_slice(), *c)).collect();
    for (text, class) in &in_b {
        if let Some(&c) = a_class.get(text.as_slice()) {
            assert_eq!(c, *class, "{:?}", String::from_utf8_lossy(text));
        }
    }
    // 5000 numbers, 5000 `n` lines, `x` and `y` complete and incomplete.
    assert_eq!(text_of.len(), 10004);
    assert_eq!(files[0].equiv_max, 10005);
    assert_ne!(a_class[b"x".as_slice()], in_b[10000].1);
    assert_ne!(a_class[b"y\n".as_slice()], in_b[10001].1);
}

#[test]
fn find_hunk_joins_changes_within_twice_the_context() {
    let o = opts(&["-u", "a", "b"]);
    let ch = |line0: Lin| analyze::Change {
        line0,
        line1: line0,
        deleted: 1,
        inserted: 1,
        ignore: false,
    };
    // Six unchanged lines between is within 2 * 3 + 1.
    assert_eq!(context::find_hunk(&[ch(0), ch(7)], &o), 2);
    assert_eq!(context::find_hunk(&[ch(0), ch(8)], &o), 1);
}
