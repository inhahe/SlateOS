//! Tables printed as libsmartcols prints them. The expected outputs follow
//! upstream's rules line by line; the differential harnesses of the programs
//! built on this crate check the same against util-linux itself.

use super::*;

fn disks() -> Table {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, 0);
    let size = tb.new_column(b"SIZE", 0.0, FL_RIGHT);
    for (n, s) in [(&b"sda"[..], &b"10G"[..]), (b"sdb1", b"5G")] {
        let ln = tb.new_line(None).unwrap();
        tb.line_set_data(ln, name, n).unwrap();
        tb.line_set_data(ln, size, s).unwrap();
    }
    tb
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8(out).unwrap()
}

#[test]
fn off_a_terminal_each_column_is_as_wide_as_its_widest_cell() {
    let mut tb = disks();
    assert_eq!(
        text(tb.print().unwrap()),
        "NAME SIZE\nsda   10G\nsdb1   5G\n"
    );
}

#[test]
fn raw_export_and_json_are_upstreams() {
    let mut tb = disks();
    tb.enable_raw(true);
    assert_eq!(text(tb.print().unwrap()), "NAME SIZE\nsda 10G\nsdb1 5G\n");

    let mut tb = disks();
    tb.enable_export(true);
    assert_eq!(
        text(tb.print().unwrap()),
        "NAME=\"sda\" SIZE=\"10G\"\nNAME=\"sdb1\" SIZE=\"5G\"\n"
    );

    let mut tb = disks();
    tb.enable_json(true);
    tb.set_name(b"disks");
    assert_eq!(
        text(tb.print().unwrap()),
        "{\n   \"disks\": [\n      {\n         \"name\": \"sda\",\n         \"size\": \"10G\"\n      },{\n         \"name\": \"sdb1\",\n         \"size\": \"5G\"\n      }\n   ]\n}\n"
    );
}

#[test]
fn an_empty_table_prints_nothing_but_json_prints_an_empty_array() {
    let mut tb = Table::new();
    tb.new_column(b"A", 0.0, 0);
    assert_eq!(tb.print().unwrap(), b"");
    // Upstream's array always closes on a line of its own, so an empty one
    // holds a blank line -- `lslocks -J --pid 999999` prints the same.
    tb.enable_json(true);
    assert_eq!(text(tb.print().unwrap()), "{\n   \"\": [\n\n   ]\n}\n");
}

#[test]
fn a_table_without_columns_is_refused() {
    assert_eq!(Table::new().print(), Err(Error::Invalid));
}

#[test]
fn a_tree_is_drawn_with_the_ascii_symbols() {
    let mut tb = Table::new();
    tb.set_utf8(false);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, FL_TREE);
    let a = tb.new_line(None).unwrap();
    let b = tb.new_line(Some(a)).unwrap();
    let c = tb.new_line(Some(a)).unwrap();
    let d = tb.new_line(Some(c)).unwrap();
    for (ln, s) in [(a, &b"a"[..]), (b, b"b"), (c, b"c"), (d, b"d")] {
        tb.line_set_data(ln, name, s).unwrap();
    }
    assert_eq!(text(tb.print().unwrap()), "NAME\na\n|-b\n`-c\n  `-d\n");
}

#[test]
fn a_tree_in_utf8_uses_box_drawing() {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, FL_TREE);
    let a = tb.new_line(None).unwrap();
    let b = tb.new_line(Some(a)).unwrap();
    tb.line_set_data(a, name, b"a").unwrap();
    tb.line_set_data(b, name, b"b").unwrap();
    assert_eq!(text(tb.print().unwrap()), "NAME\na\n\u{2514}\u{2500}b\n");
}

#[test]
fn on_a_narrow_terminal_a_trunc_column_gives_way_first() {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Always);
    tb.set_termwidth(10);
    let a = tb.new_column(b"A", 0.0, FL_TRUNC);
    let b = tb.new_column(b"B", 0.0, 0);
    let ln = tb.new_line(None).unwrap();
    tb.line_set_data(ln, a, b"abcdefghij").unwrap();
    tb.line_set_data(ln, b, b"xyz").unwrap();
    assert_eq!(text(tb.print().unwrap()), "A    B\nabcd xyz\n");
}

#[test]
fn control_bytes_are_encoded_and_measured_encoded() {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let a = tb.new_column(b"A", 0.0, 0);
    let b = tb.new_column(b"B", 0.0, 0);
    let ln = tb.new_line(None).unwrap();
    tb.line_set_data(ln, a, b"x\ty").unwrap();
    tb.line_set_data(ln, b, b"z").unwrap();
    assert_eq!(text(tb.print().unwrap()), "A      B\nx\\x09y z\n");
}

#[test]
fn a_shell_variable_name_is_made_of_letters_digits_and_underscores() {
    let col = Column {
        header: Cell {
            data: Some(b"1FOO%".to_vec()),
            ..Cell::default()
        },
        ..Column::default()
    };
    assert_eq!(col.name_as_shellvar(), Some(b"_1FOO_PCT".to_vec()));
    let col = Column {
        header: Cell {
            data: Some(b"MAJ:MIN".to_vec()),
            ..Cell::default()
        },
        ..Column::default()
    };
    assert_eq!(col.name_as_shellvar(), Some(b"MAJ_MIN".to_vec()));
}

#[test]
fn maxout_and_minout_exclude_each_other() {
    let mut tb = Table::new();
    tb.enable_maxout(true).unwrap();
    assert_eq!(tb.enable_minout(true), Err(Error::Invalid));
    tb.enable_maxout(false).unwrap();
    tb.enable_minout(true).unwrap();
    assert_eq!(tb.enable_maxout(true), Err(Error::Invalid));
}

#[test]
fn every_terminal_width_finishes_printing() {
    // Upstream never finishes at some of these widths: its reductions wrap
    // a size_t to a width in the quintillions, or run out of stages with the
    // table still too wide and loop. Here every one ends, with the table as
    // wide as the columns' minimums allow.
    for width in 0..40 {
        let mut tb = Table::new();
        tb.set_utf8(true);
        tb.set_termforce(TermForce::Always);
        tb.set_termwidth(width);
        let a = tb.new_column(b"WRAPPED", 0.3, FL_WRAP);
        let b = tb.new_column(b"TRUNCATED", 0.1, FL_TRUNC);
        let c = tb.new_column(b"PLAIN", 0.0, 0);
        for _ in 0..3 {
            let ln = tb.new_line(None).unwrap();
            tb.line_set_data(ln, a, b"abcdefghijklmnopqrstuvwxyz")
                .unwrap();
            tb.line_set_data(ln, b, b"0123456789").unwrap();
            tb.line_set_data(ln, c, b"x").unwrap();
        }
        let mut out = Vec::new();
        tb.print_into(&mut out).unwrap();
        assert!(
            out.ends_with(
                b"
"
            ),
            "width {width}"
        );
    }
}

/// Columns A, B, C and one line `a b c`, off a terminal.
fn abc() -> (Table, [ColumnId; 3]) {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let ids = [
        tb.new_column(b"A", 0.0, 0),
        tb.new_column(b"B", 0.0, 0),
        tb.new_column(b"C", 0.0, 0),
    ];
    let ln = tb.new_line(None).unwrap();
    for (&cl, d) in ids.iter().zip([&b"a"[..], b"b", b"c"]) {
        tb.line_set_data(ln, cl, d).unwrap();
    }
    (tb, ids)
}

#[test]
fn a_moved_column_takes_its_cells_with_it() {
    let (mut tb, [a, b, c]) = abc();
    tb.move_column(None, c).unwrap();
    assert_eq!(text(tb.print().unwrap()), "C A B\nc a b\n");
    tb.move_column(Some(a), c).unwrap();
    assert_eq!(text(tb.print().unwrap()), "A C B\na c b\n");
    // Already after `pre`: nothing to do.
    tb.move_column(Some(c), b).unwrap();
    assert_eq!(text(tb.print().unwrap()), "A C B\na c b\n");
    assert_eq!(tb.column_ids(), vec![a, c, b]);
    assert_eq!(tb.column(1), Some(c));
    assert_eq!(tb.column_by_name(b"B"), Some(b));
    assert_eq!(tb.line_column_data(LineId(0), c), Some(&b"c"[..]));
}

#[test]
fn a_column_moved_behind_itself_leaves_the_table_and_its_cell_behind() {
    // `column -t -N A,B,C -O A,A`: upstream unlinks A and links it after
    // itself, in a list of its own. B and C are renumbered 0 and 1 while
    // the cells stay put, so each shows the cell before its own.
    let (mut tb, [a, b, c]) = abc();
    tb.move_column(Some(a), a).unwrap();
    assert_eq!(text(tb.print().unwrap()), "B C\na b\n");
    assert_eq!(tb.column_ids(), vec![b, c]);
    assert_eq!(tb.ncols(), 3);
    // Moved back after a column of the table, it rejoins it, and each
    // line's cell moves from its old number, 0, to its new one, 2 -- which
    // puts B's and C's own cells back under them.
    tb.move_column(Some(c), a).unwrap();
    assert_eq!(tb.column_ids(), vec![b, c, a]);
    assert_eq!(text(tb.print().unwrap()), "B C A\nb c a\n");
}

#[test]
fn a_column_moved_behind_a_detached_one_is_detached_too() {
    let (mut tb, [a, b, c]) = abc();
    tb.move_column(Some(a), a).unwrap();
    // A kept its number, 0, and C is now 1: "already after A", upstream's
    // test says, and nothing moves.
    tb.move_column(Some(a), c).unwrap();
    assert_eq!(tb.column_ids(), vec![b, c]);
    // B, now 0, is not "after" A's stale 0: it is linked after A, outside
    // the table, and C reads the first cell.
    tb.move_column(Some(a), b).unwrap();
    assert_eq!(tb.column_ids(), vec![c]);
    assert_eq!(text(tb.print().unwrap()), "C\na\n");
}

#[test]
fn a_line_changes_parents_and_its_new_parent_lists_it_last() {
    let mut tb = Table::new();
    tb.set_utf8(false);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"N", 0.0, FL_TREE);
    let l: Vec<LineId> = (0..4).map(|_| tb.new_line(None).unwrap()).collect();
    for (&ln, d) in l.iter().zip([&b"1"[..], b"2", b"3", b"4"]) {
        tb.line_set_data(ln, name, d).unwrap();
    }
    tb.line_add_child(l[0], l[2]).unwrap();
    tb.line_add_child(l[0], l[3]).unwrap();
    tb.line_add_child(l[1], l[2]).unwrap();
    assert_eq!(text(tb.print().unwrap()), "N\n1\n`-4\n2\n`-3\n");
    assert!(tb.line_is_ancestor(l[1], l[2]));
    assert!(tb.line_is_ancestor(l[2], l[2]));
    assert!(!tb.line_is_ancestor(l[0], l[2]));
    tb.line_remove_child(l[1], l[2]).unwrap();
    assert_eq!(text(tb.print().unwrap()), "N\n1\n`-4\n2\n3\n");
}

#[test]
fn a_line_no_root_leads_to_is_neither_measured_nor_printed() {
    // A line made its own child is its own parent: not a root, and under
    // none. Upstream's walk never reaches it; nor does this one.
    let mut tb = Table::new();
    tb.set_utf8(false);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"N", 0.0, FL_TREE);
    let a = tb.new_line(None).unwrap();
    let b = tb.new_line(None).unwrap();
    tb.line_set_data(a, name, b"a").unwrap();
    tb.line_set_data(b, name, b"a-much-wider-cell").unwrap();
    tb.line_add_child(b, b).unwrap();
    assert_eq!(text(tb.print().unwrap()), "N\na\n");
}

#[test]
fn an_unnamed_column_has_an_empty_header() {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let a = tb.new_unnamed_column(0.0, 0);
    let b = tb.new_column(b"B", 0.0, 0);
    let ln = tb.new_line(None).unwrap();
    tb.line_set_data(ln, a, b"xx").unwrap();
    tb.line_set_data(ln, b, b"y").unwrap();
    assert_eq!(tb.column_name(a), None);
    assert_eq!(text(tb.print().unwrap()), "   B\nxx y\n");
}

/// findmnt's SOURCES column: wrapped at its newlines.
fn sources(data: &[u8]) -> (Table, ColumnId) {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    let t = tb.new_column(b"TARGET", 0.0, 0);
    let s = tb.new_column(b"SOURCES", 0.0, FL_WRAP);
    tb.column_set_wrapnl(s).unwrap();
    tb.column_set_safechars(s, b"\n").unwrap();
    let ln = tb.new_line(None).unwrap();
    tb.line_set_data(ln, t, b"/x").unwrap();
    tb.line_set_data(ln, s, data).unwrap();
    (tb, s)
}

#[test]
fn a_newline_wrapped_cell_prints_a_piece_per_line() {
    let (mut tb, _) = sources(b"a\nbbb");
    assert_eq!(
        text(tb.print().unwrap()),
        "TARGET SOURCES\n/x     a\n       bbb\n"
    );
    // A trailing newline ends the last piece; it does not start another.
    let (mut tb, _) = sources(b"a\n");
    assert_eq!(text(tb.print().unwrap()), "TARGET SOURCES\n/x     a\n");
    // The column is as wide as its widest piece, not its whole text.
    let (mut tb, _) = sources(b"/dev/sda1\n/dev/sdb1");
    assert_eq!(
        text(tb.print().unwrap()),
        "TARGET SOURCES\n/x     /dev/sda1\n       /dev/sdb1\n"
    );
}

#[test]
fn a_newline_wrapped_json_array_has_an_element_per_piece() {
    let (mut tb, s) = sources(b"a\nbbb");
    tb.column_set_json_type(s, JsonType::ArrayString).unwrap();
    tb.enable_json(true);
    tb.set_name(b"t");
    assert_eq!(
        text(tb.print().unwrap()),
        "{\n   \"t\": [\n      {\n         \"target\": \"/x\",\n         \"sources\": [\n             \"a\", \"bbb\"\n         ]\n      }\n   ]\n}\n"
    );
}

#[test]
fn a_range_prints_its_header_once_and_lines_can_be_replaced() {
    let mut tb = disks();
    let mut out = Vec::new();
    tb.print_range_into(&mut out).unwrap();
    assert_eq!(text(out), "NAME SIZE\nsda   10G\nsdb1   5G");
    tb.remove_lines();
    assert_eq!(tb.nlines(), 0);
    let ln = tb.new_line(None).unwrap();
    let (name, size) = (tb.column(0).unwrap(), tb.column(1).unwrap());
    tb.line_set_data(ln, name, b"sdc").unwrap();
    tb.line_set_data(ln, size, b"1G").unwrap();
    let mut out = Vec::new();
    tb.print_range_into(&mut out).unwrap();
    assert_eq!(text(out), "sdc    1G");
}

#[test]
fn a_tree_cannot_be_printed_as_a_range() {
    let mut tb = Table::new();
    tb.set_termforce(TermForce::Never);
    tb.new_column(b"N", 0.0, FL_TREE);
    tb.new_line(None).unwrap();
    assert!(tb.print_range_into(&mut Vec::new()).is_err());
}

/// lsblk --merge's shape: sda1 and sdb1 grouped, md0 the group's child and
/// md0p1 md0's -- in table order as lsblk adds them.
fn merged(utf8: bool) -> Table {
    let mut tb = Table::new();
    tb.set_utf8(utf8);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, FL_TREE);
    let size = tb.new_column(b"SIZE", 0.0, FL_RIGHT);
    let rows: [(&[u8], &[u8], Option<usize>); 6] = [
        (b"sda", b"10G", None),
        (b"sda1", b"5G", Some(0)),
        (b"sdb", b"10G", None),
        (b"sdb1", b"5G", Some(2)),
        (b"md0", b"5G", None),
        (b"md0p1", b"1G", Some(4)),
    ];
    let mut lines = Vec::new();
    for (n, s, parent) in rows {
        let ln = tb.new_line(parent.map(|p| lines[p])).unwrap();
        tb.line_set_data(ln, name, n).unwrap();
        tb.line_set_data(ln, size, s).unwrap();
        lines.push(ln);
    }
    tb.group_lines(Some(lines[1]), lines[1]).unwrap();
    tb.group_lines(Some(lines[3]), lines[1]).unwrap();
    tb.line_link_group(lines[4], lines[1]).unwrap();
    tb
}

// The expected outputs from here on are util-linux 2.39.3's libsmartcols
// own, from `scripts/scols-probe.c` running the same calls.

#[test]
fn a_groups_chart_is_drawn_left_of_the_tree() {
    let mut tb = merged(false);
    tb.enable_ascii(true);
    assert_eq!(
        text(tb.print().unwrap()),
        "    NAME    SIZE\n    sda      10G\n,-> `-sda1    5G\n|   sdb      10G\n'-> `-sdb1    5G\n `--md0       5G\n    `-md0p1   1G\n"
    );
}

#[test]
fn a_groups_chart_in_utf8_is_drawn_with_dashed_lines() {
    let mut tb = merged(true);
    assert_eq!(
        text(tb.print().unwrap()),
        "    NAME    SIZE\n    sda      10G\n\u{250c}\u{2508}\u{25b6} \u{2514}\u{2500}sda1    5G\n\u{2506}   sdb      10G\n\u{2514}\u{252c}\u{25b6} \u{2514}\u{2500}sdb1    5G\n \u{2514}\u{2508}\u{2508}md0       5G\n    \u{2514}\u{2500}md0p1   1G\n"
    );
}

#[test]
fn in_json_a_groups_child_is_a_root() {
    let mut tb = merged(true);
    tb.enable_json(true);
    tb.set_name(b"blockdevices");
    let out = text(tb.print().unwrap());
    assert!(out.contains(
        "},{\n         \"name\": \"md0\",\n         \"size\": \"5G\",\n         \"children\": ["
    ));
    assert!(!out.contains('\u{2508}'));
}

#[test]
fn a_second_groups_chart_goes_left_of_the_first() {
    let mut tb = Table::new();
    tb.set_utf8(false);
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, FL_TREE);
    let l: Vec<LineId> = [&b"a"[..], b"b", b"c", b"d", b"x", b"y"]
        .iter()
        .map(|n| {
            let ln = tb.new_line(None).unwrap();
            tb.line_set_data(ln, name, n).unwrap();
            ln
        })
        .collect();
    tb.group_lines(Some(l[0]), l[0]).unwrap();
    tb.group_lines(Some(l[2]), l[0]).unwrap();
    tb.group_lines(Some(l[1]), l[1]).unwrap();
    tb.group_lines(Some(l[3]), l[1]).unwrap();
    tb.line_link_group(l[4], l[0]).unwrap();
    tb.line_link_group(l[5], l[1]).unwrap();
    tb.enable_ascii(true);
    // The horizontal line to a child runs through every empty slot to its
    // right -- and, as upstream counts them without resetting the count,
    // through the slots its earlier calls counted too.
    assert_eq!(
        text(tb.print().unwrap()),
        "       NAME\n   ,-> a\n,->|   b\n|  '-> c\n|   `--x\n'->    d\n `-----y\n"
    );
}

#[test]
fn grouping_refuses_what_upstream_refuses() {
    let mut tb = Table::new();
    tb.set_utf8(true);
    tb.set_termforce(TermForce::Never);
    tb.new_column(b"N", 0.0, FL_TREE);
    let l0 = tb.new_line(None).unwrap();
    let l1 = tb.new_line(None).unwrap();
    let l2 = tb.new_line(Some(l0)).unwrap();
    let l3 = tb.new_line(None).unwrap();
    tb.group_lines(Some(l0), l1).unwrap();
    // l1 is in a group, l2 in none.
    assert_eq!(tb.group_lines(Some(l1), l2), Err(Error::Invalid));
    // l2 has a parent.
    assert_eq!(tb.line_link_group(l2, l0), Err(Error::Invalid));
    // l2 is in no group.
    assert_eq!(tb.line_link_group(l3, l2), Err(Error::Invalid));
    tb.line_link_group(l3, l0).unwrap();
    // Already a group's child.
    assert_eq!(tb.line_link_group(l3, l0), Err(Error::Invalid));
    // A group of one, made without a second line.
    tb.group_lines(None, l3).unwrap();
    assert_eq!(
        text(tb.print().unwrap()),
        "       N\n   \u{250c}\u{2508}\u{25b6} \n   \u{2506}   \u{2514}\u{2500}\n   \u{2514}\u{252c}\u{25b6} \n\u{250c}\u{2508}\u{25b6} \u{2514}\u{2508}\u{2508}\n"
    );
}

#[test]
fn removing_the_lines_removes_the_groups() {
    let mut tb = merged(false);
    tb.remove_lines();
    let name = tb.column(0).unwrap();
    let ln = tb.new_line(None).unwrap();
    tb.line_set_data(ln, name, b"sdc").unwrap();
    tb.enable_ascii(true);
    assert_eq!(text(tb.print().unwrap()), "NAME SIZE\nsdc  \n");
}

/// lsblk's `cmp_u64_cells`.
fn cmp_u64(a: CellView<'_>, b: CellView<'_>) -> i32 {
    match (a.userdata(), b.userdata()) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => match x.cmp(&y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        },
    }
}

#[test]
fn a_list_is_sorted_stably_with_cells_without_data_first() {
    let mut tb = Table::new();
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, 0);
    let size = tb.new_column(b"SIZE", 0.0, FL_RIGHT);
    let rows: [(Option<&[u8]>, Option<u64>); 5] = [
        (Some(b"sdb"), Some(5)),
        (Some(b"sda"), Some(10)),
        (None, Some(5)),
        (Some(b"sdc"), None),
        (Some(b"loop0"), Some(0)),
    ];
    for (n, s) in rows {
        let ln = tb.new_line(None).unwrap();
        if let Some(n) = n {
            tb.line_set_data(ln, name, n).unwrap();
        }
        if let Some(s) = s {
            tb.line_set_data(ln, size, s.to_string().as_bytes()).unwrap();
            tb.cell_set_userdata(ln, 1, s).unwrap();
        }
    }
    // No comparison function yet.
    assert_eq!(tb.sort(Some(size)), Err(Error::Invalid));
    tb.column_set_cmpfunc(size, cmp_u64).unwrap();
    tb.sort(Some(size)).unwrap();
    assert_eq!(
        text(tb.print().unwrap()),
        "NAME  SIZE\nsdc   \nloop0    0\nsdb      5\n         5\nsda     10\n"
    );
    tb.column_set_cmpfunc(name, cmpstr_cells).unwrap();
    tb.sort(Some(name)).unwrap();
    let by_name = "NAME  SIZE\n         5\nloop0    0\nsda     10\nsdb      5\nsdc   \n";
    assert_eq!(text(tb.print().unwrap()), by_name);
    // Without a column: the one sorted by last.
    tb.sort(None).unwrap();
    assert_eq!(text(tb.print().unwrap()), by_name);
}

#[test]
fn sorting_by_tree_puts_each_line_before_its_children() {
    let mut tb = Table::new();
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"NAME", 0.0, 0);
    tb.column_set_cmpfunc(name, cmpstr_cells).unwrap();
    let mut l: Vec<LineId> = Vec::new();
    for (n, parent) in [
        (&b"z"[..], None),
        (b"b", None),
        (b"y", Some(0)),
        (b"a", Some(1)),
        (b"x", Some(0)),
        (b"c", Some(3)),
    ] {
        let ln = tb.new_line(parent.map(|p: usize| l[p])).unwrap();
        tb.line_set_data(ln, name, n).unwrap();
        l.push(ln);
    }
    tb.sort_by_tree();
    assert_eq!(text(tb.print().unwrap()), "NAME\nz\ny\nx\nb\na\nc\n");
    // A list is sorted as one: the tree is not consulted.
    tb.sort(Some(name)).unwrap();
    assert_eq!(text(tb.print().unwrap()), "NAME\na\nb\nc\nx\ny\nz\n");
    // By tree again, each line's children sorted by the name first.
    tb.sort_by_tree();
    assert_eq!(text(tb.print().unwrap()), "NAME\nb\na\nc\nz\nx\ny\n");
}

#[test]
fn a_line_made_its_own_groups_child_is_sorted_without_end_upstream_but_not_here() {
    // Upstream's sort recurses from the group's first member into the
    // group's children -- the member itself -- until the stack runs out.
    let mut tb = Table::new();
    tb.set_termforce(TermForce::Never);
    let name = tb.new_column(b"N", 0.0, FL_TREE);
    tb.column_set_cmpfunc(name, cmpstr_cells).unwrap();
    let a = tb.new_line(None).unwrap();
    let b = tb.new_line(None).unwrap();
    tb.line_set_data(a, name, b"a").unwrap();
    tb.line_set_data(b, name, b"b").unwrap();
    tb.group_lines(Some(b), a).unwrap();
    tb.line_link_group(a, b).unwrap();
    tb.sort(Some(name)).unwrap();
    tb.print().unwrap();
}
