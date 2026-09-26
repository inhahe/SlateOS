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
