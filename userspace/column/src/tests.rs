//! The pieces of `column.c`, each against what util-linux 2.39.3 does with
//! the same input. The program as a whole is compared with upstream's by
//! `scripts/column-diff.sh`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn wide(s: &str) -> Vec<char> {
    s.chars().collect()
}

#[test]
fn a_broken_byte_is_written_in_hex_and_a_literal_backslash_x_with_it() {
    assert_eq!(mbs_invalid_encode(b"a\xe9b \\x", true), b"a\\xe9b \\x5cx");
    // Each byte of a sequence cut short is broken alone, as mbrtowc finds
    // the string's NUL where a continuation should be.
    assert_eq!(mbs_invalid_encode(b"a\xe2\x82", true), b"a\\xe2\\x82");
    // Valid text around them passes.
    assert_eq!(
        mbs_invalid_encode("\u{e9}\u{4e00}\u{ff}".as_bytes(), true),
        "\u{e9}\u{4e00}\u{ff}".as_bytes()
    );
    // In the C locale every byte above 0x7f is broken.
    assert_eq!(
        mbs_invalid_encode("\u{e9}".as_bytes(), false),
        b"\\xc3\\xa9"
    );
}

#[test]
fn only_ascii_decodes_in_the_c_locale() {
    assert_eq!(mbs_to_wcs(b"ab", false), Some(wide("ab")));
    assert_eq!(mbs_to_wcs("\u{e9}".as_bytes(), false), None);
    assert_eq!(mbs_to_wcs("\u{e9}".as_bytes(), true), Some(wide("\u{e9}")));
    assert_eq!(mbs_to_wcs(b"\xff", true), None);
}

/// Every field `local_wcstok` finds in `line`.
fn fields(greedy: bool, seps: &str, line: &str) -> Vec<String> {
    let (seps, line) = (wide(seps), wide(line));
    let mut state = Tok::Start;
    let mut out = Vec::new();
    while let Some((a, b)) = local_wcstok(greedy, &seps, &line, &mut state) {
        out.push(line[a..b].iter().collect());
    }
    out
}

#[test]
fn the_default_separators_are_greedy() {
    assert_eq!(fields(true, "\t ", "  a  b\tc  "), ["a", "b", "c"]);
    assert_eq!(fields(true, "\t ", "abc"), ["abc"]);
    assert!(fields(true, "\t ", "   ").is_empty());
}

#[test]
fn a_given_separator_splits_every_time() {
    assert_eq!(fields(false, ":", "a::b:"), ["a", "", "b", ""]);
    // Leading blanks are part of the first field.
    assert_eq!(fields(false, ":", " a"), [" a"]);
    // Any of the separators; none at all is one field.
    assert_eq!(fields(false, ":,", "a,b:c"), ["a", "b", "c"]);
    assert_eq!(fields(false, "", "a:b"), ["a:b"]);
}

#[test]
fn a_list_of_names_drops_its_empty_words() {
    assert_eq!(strv_split(b",a,,b,"), [b"a".to_vec(), b"b".to_vec()]);
    assert!(strv_split(b"").is_empty());
    assert!(strv_split(b",").is_empty());
}

#[test]
fn unnamed_is_a_lone_dash_word() {
    assert!(has_unnamed(b"-"));
    assert!(has_unnamed(b"1,-"));
    assert!(has_unnamed(b"-,"));
    assert!(!has_unnamed(b"1-2"));
    assert!(!has_unnamed(b"a,b"));
}

/// The list modes' output for `ents` on a terminal `termwidth` wide.
fn listed(ents: &[&str], termwidth: usize, rows: bool) -> String {
    let mut ctl = Ctl::new(true);
    ctl.termwidth = Some(termwidth);
    for e in ents {
        let w = wide(e);
        ctl.maxlength = ctl.maxlength.max(width(&w));
        ctl.ents.push(w);
    }
    let mut out = Vec::new();
    if rows {
        ctl.columnate_fillrows(&mut out);
    } else {
        ctl.columnate_fillcols(&mut out);
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn entries_fill_columns_then_rows_at_tab_stops() {
    // The widest is 5, so each column is 8 wide, and 30 holds 3 of them.
    assert_eq!(
        listed(&["a", "bb", "ccc", "ddddd"], 30, false),
        "a\tccc\nbb\tddddd\n"
    );
    assert_eq!(
        listed(&["a", "bb", "ccc", "ddddd"], 30, true),
        "a\tbb\tccc\nddddd\n"
    );
    // A width of exactly 8 rounds up to 16: one tab, from 8 to 16.
    assert_eq!(listed(&["abcdefgh", "x"], 40, true), "abcdefgh\tx\n");
}

#[test]
fn filling_rows_with_room_for_no_column_never_breaks_a_line() {
    // 9 cells round to 16, and a 10-cell terminal holds none: upstream's
    // row never reaches its column count.
    assert_eq!(
        listed(&["abcdefghi", "b", "c"], 10, true),
        "abcdefghi\tb\t\tc\n"
    );
}

#[test]
fn a_line_is_a_c_string_and_keeps_its_blanks() {
    let mut ctl = Ctl::new(true);
    ctl.read_line(b"  ab \x00cd\n", b"column").unwrap();
    ctl.read_line(b" \t\x0b\r\n", b"column").unwrap();
    assert_eq!(ctl.ents, [wide("  ab ")]);
    ctl.keep_empty_lines = true;
    ctl.read_line(b"\n", b"column").unwrap();
    assert_eq!(ctl.ents, [wide("  ab "), wide("")]);
    assert_eq!(ctl.maxlength, 5);
    assert_eq!(ctl.errno, 0);
    ctl.read_line(b"a\xffb\n", b"column").unwrap();
    assert_eq!(ctl.ents[2], wide("a\\xffb"));
    assert_eq!(ctl.errno, EILSEQ);
}

/// A table-mode control block.
fn table_ctl() -> Ctl {
    let mut ctl = Ctl::new(true);
    ctl.mode = Mode::Table;
    ctl
}

/// Every cell of line `ln`, as text.
fn cells(tb: &Table, ln: usize) -> Vec<Option<String>> {
    let ln = tb.line_ids().nth(ln).unwrap();
    tb.column_ids()
        .into_iter()
        .map(|cl| {
            tb.line_column_data(ln, cl)
                .map(|d| String::from_utf8(d.to_vec()).unwrap())
        })
        .collect()
}

#[test]
fn fields_become_cells_and_columns_are_made_for_them() {
    let mut ctl = table_ctl();
    ctl.read_line(b"a b\n", b"column").unwrap();
    ctl.read_line(b"c d e\n", b"column").unwrap();
    let tb = ctl.tab.as_ref().unwrap();
    assert_eq!(tb.ncols(), 3);
    assert_eq!(cells(tb, 0), [Some("a".into()), Some("b".into()), None]);
    assert_eq!(
        cells(tb, 1),
        [Some("c".into()), Some("d".into()), Some("e".into())]
    );
}

#[test]
fn at_the_limit_the_rest_of_the_line_is_the_last_field() {
    let mut ctl = table_ctl();
    ctl.maxncols = 2;
    ctl.read_line(b"a  b  c  \n", b"column").unwrap();
    let tb = ctl.tab.as_ref().unwrap();
    assert_eq!(cells(tb, 0), [Some("a".into()), Some("b  c  ".into())]);
}

#[test]
fn json_needs_a_name_for_every_column() {
    let mut ctl = table_ctl();
    ctl.json = true;
    ctl.tab_colnames = Some(vec![b"A".to_vec()]);
    ctl.read_line(b"x\n", b"column").unwrap();
    // The second field has no column name. Upstream counts the line just
    // made, so it names the next one.
    assert_eq!(ctl.read_line(b"y z\n", b"column"), Err(1));
    // Hiding the unnamed ones makes them hidden columns instead.
    let mut ctl = table_ctl();
    ctl.json = true;
    ctl.hide_unnamed = true;
    ctl.tab_colnames = Some(vec![b"A".to_vec()]);
    ctl.read_line(b"y z\n", b"column").unwrap();
    let tb = ctl.tab.as_ref().unwrap();
    let last = tb.column_ids()[1];
    assert_eq!(tb.column_flags(last), Some(FL_HIDDEN));
}

/// A table of columns A, B, C and one without a name.
fn four() -> Table {
    let mut tb = Table::new();
    tb.new_column(b"A", 0.0, 0);
    tb.new_column(b"B", 0.0, 0);
    tb.new_column(b"C", 0.0, 0);
    tb.new_unnamed_column(0.0, 0);
    tb
}

/// Which columns, by position, carry `flag`.
fn flagged(tb: &Table, flag: u32) -> Vec<usize> {
    tb.column_ids()
        .into_iter()
        .enumerate()
        .filter(|&(_, cl)| tb.column_flags(cl).unwrap() & flag != 0)
        .map(|(i, _)| i)
        .collect()
}

fn apply(list: &str, flag: u32) -> (Table, Result<(), u8>) {
    let mut tb = four();
    let rc = apply_columnflag_from_list(&mut tb, list.as_bytes(), flag, b"column");
    (tb, rc)
}

#[test]
fn a_list_names_columns_by_number_name_and_range() {
    let (tb, rc) = apply("1,C", FL_RIGHT);
    assert_eq!((flagged(&tb, FL_RIGHT), rc), (vec![0, 2], Ok(())));
    assert_eq!(flagged(&apply("2-3", FL_RIGHT).0, FL_RIGHT), [1, 2]);
    // `0` is every column, `-` every one without a name.
    assert_eq!(flagged(&apply("0", FL_RIGHT).0, FL_RIGHT), [0, 1, 2, 3]);
    assert_eq!(flagged(&apply("-", FL_RIGHT).0, FL_RIGHT), [3]);
    // Negative numbers count visible columns from the end.
    assert_eq!(flagged(&apply("-2--1", FL_RIGHT).0, FL_RIGHT), [2, 3]);
    // Nothing checks what follows a lone number: `5x-3` is column 5, and
    // there is none.
    assert!(flagged(&apply("5x-3", FL_RIGHT).0, FL_RIGHT).is_empty());
    // A huge range ends where the columns do.
    assert_eq!(
        flagged(&apply("-2147483648-2147483647", FL_RIGHT).0, FL_RIGHT),
        [0, 1, 2, 3]
    );
    // An unknown name is refused, whatever came before it.
    assert_eq!(apply("1,X", FL_RIGHT).1, Err(1));
}

#[test]
fn hiding_from_the_end_counts_what_is_still_visible() {
    // `-2` hides C, the second visible from the end; then `-1` hides the
    // unnamed one, still the last visible.
    assert_eq!(flagged(&apply("-2--1", FL_HIDDEN).0, FL_HIDDEN), [2, 3]);
    // `-1` first, then `-1` again: each finds the last still visible.
    assert_eq!(flagged(&apply("-1,-1", FL_HIDDEN).0, FL_HIDDEN), [2, 3]);
}

#[test]
fn a_column_is_a_number_a_name_or_the_last_visible() {
    let tb = four();
    let ids = tb.column_ids();
    assert_eq!(string_to_column(&tb, b"2", b"column"), Ok(ids[1]));
    assert_eq!(string_to_column(&tb, b"B", b"column"), Ok(ids[1]));
    assert_eq!(string_to_column(&tb, b"-1", b"column"), Ok(ids[3]));
    // Column 0 is uint32_t's 0 - 1: no column.
    assert_eq!(string_to_column(&tb, b"0", b"column"), Err(1));
    assert_eq!(string_to_column(&tb, b"99999999999", b"column"), Err(1));
    assert_eq!(string_to_column(&tb, b"D", b"column"), Err(1));
}

#[test]
fn a_tree_is_made_from_ids_and_parents_without_loops() {
    let mut ctl = table_ctl();
    ctl.tab_colnames = Some(vec![b"ID".to_vec(), b"PARENT".to_vec(), b"NAME".to_vec()]);
    for line in [&b"1 0 a\n"[..], b"2 1 b\n", b"3 2 c\n", b"1 3 d\n"] {
        ctl.read_line(line, b"column").unwrap();
    }
    let tab = ctl.tab.as_mut().unwrap();
    tab.set_utf8(false);
    tab.set_termforce(TermForce::Never);
    create_tree(tab, b"NAME", b"PARENT", b"ID", b"column").unwrap();
    let mut out = Vec::new();
    tab.print_into(&mut out).unwrap();
    // Taking each line's ID in turn: `b` goes under `a` (ID 1), `c` under
    // `b`, `d` under `c`. Then `d`'s ID is 1 too, but `b` under `d` would
    // close a loop -- `b` is `d`'s ancestor -- so it stays where it is.
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "ID  PARENT  NAME\n\
         1   0       a\n\
         2   1       `-b\n\
         3   2         `-c\n\
         1   3           `-d\n"
    );
}

#[test]
fn the_help_ends_as_upstreams_does() {
    let text = String::from_utf8(usage(b"column")).unwrap();
    assert!(text.starts_with("\nUsage:\n column [options] [<file>...]\n\nColumnate lists.\n"));
    assert!(text.contains("\n -h, --help                       display this help\n"));
    assert!(text.ends_with(
        " -V, --version                    display version\n\nFor more details see column(1).\n"
    ));
}
