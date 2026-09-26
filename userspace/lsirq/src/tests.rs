//! The pieces of `lsirq.c` and `irq-common.c`, each against what
//! util-linux 2.39.3 does with the same input. The program as a whole is
//! compared with upstream's by `scripts/lsirq-diff.sh`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

fn line(s: &str, cpus: usize) -> IrqInfo {
    parse_line(s.as_bytes(), cpus, false).unwrap()
}

#[test]
fn a_line_is_an_irq_counters_and_a_name() {
    let got = line(
        "  0:         10          5   IO-APIC   2-edge      timer\n",
        2,
    );
    assert_eq!(got.irq, b"0");
    assert_eq!(got.total, 15);
    assert_eq!(got.name, b"IO-APIC 2-edge timer");
    // Only as many counters as the header has CPUs are read; the rest is
    // the name.
    let got = line("NMI:          3          4   Non-maskable interrupts\n", 1);
    assert_eq!(got.total, 3);
    assert_eq!(got.name, b"4 Non-maskable interrupts");
    // Nothing after the counters is an empty name.
    assert_eq!(line("ERR:          7\n", 2).name, b"");
    // No colon, no line.
    assert_eq!(parse_line(b"no colon here\n", 2, false), None);
}

#[test]
fn counters_are_eleven_bytes_apart_whatever_they_hold() {
    // `5` then eleven bytes on: `7` is inside the jump, and what is read
    // next is ` 8`.
    let got = line("X: 5 7         8 name\n", 3);
    assert_eq!(got.total, 13);
    // The first that does not parse stops the rest.
    let got = line("X:          1       abc          2 tail\n", 3);
    assert_eq!(got.total, 1);
    assert_eq!(got.name, b"abc 2 tail");
    // At most ten characters a counter, sign included.
    let got = line("X: 12345678901\n", 1);
    assert_eq!(got.total, 1_234_567_890);
    // A minus sign wraps, as strtoul's does.
    assert_eq!(line("X:         -5\n", 1).total, 5u64.wrapping_neg());
}

#[test]
fn the_irq_loses_only_its_leading_blanks() {
    assert_eq!(line(" \t LOC :   1\n", 1).irq, b"LOC ");
}

#[test]
fn a_softirq_is_named_from_upstreams_table() {
    let got = parse_line(b"    NET_RX:          4          5\n", 2, true).unwrap();
    assert_eq!(got.name, b"network receive softirq");
    assert_eq!(got.total, 9);
    // Two counters inside one eleven-byte field are one counter.
    assert_eq!(parse_line(b"NET_RX:  4  5\n", 2, true).unwrap().total, 4);
    assert_eq!(parse_line(b"  OTHER:  1\n", 1, true).unwrap().name, b"");
}

#[test]
fn a_line_is_a_c_string() {
    let got = parse_line(b"A:          3 na\0me\n", 1, false).unwrap();
    assert_eq!(got.name, b"na");
    // A colon after the NUL is not seen.
    assert_eq!(parse_line(b"A\0:  3\n", 1, false), None);
}

fn irq(name: &str, total: u64) -> IrqInfo {
    IrqInfo {
        irq: name.as_bytes().to_vec(),
        name: name.as_bytes().to_vec(),
        total,
        delta: 0,
    }
}

fn order(sort: SortBy, mut v: Vec<IrqInfo>) -> Vec<String> {
    sort_result(sort, &mut v);
    v.into_iter()
        .map(|i| String::from_utf8(i.irq).unwrap())
        .collect()
}

#[test]
fn the_default_order_is_descending_total_and_stable() {
    let v = vec![irq("a", 1), irq("b", 5), irq("c", 1), irq("d", 5)];
    assert_eq!(order(SortBy::Total, v), ["b", "d", "a", "c"]);
}

#[test]
fn irqs_sort_as_versions_and_names_as_bytes() {
    let v = vec![irq("10", 0), irq("9", 0), irq("NMI", 0), irq("1", 0)];
    assert_eq!(
        order(SortBy::Interrupts, v.clone()),
        ["1", "9", "10", "NMI"]
    );
    assert_eq!(order(SortBy::Name, v.clone()), ["1", "10", "9", "NMI"]);
    // Every delta is 0 here, so DELTA is NAME.
    assert_eq!(order(SortBy::Delta, v), ["1", "10", "9", "NMI"]);
}

#[test]
fn sort_and_column_names_are_any_case() {
    assert_eq!(sort_func_by_name(b"irq"), Some(SortBy::Interrupts));
    assert_eq!(sort_func_by_name(b"Total"), Some(SortBy::Total));
    assert_eq!(sort_func_by_name(b"TOT"), None);
    assert_eq!(
        irq_column_name_to_id(b"delta", b"delta", b"lsirq"),
        Some(Col::Delta)
    );
    assert_eq!(irq_column_name_to_id(b"NAM", b"NAM", b"lsirq"), None);
}

#[test]
fn the_help_lists_every_column_but_delta() {
    let text = String::from_utf8(usage(b"lsirq")).unwrap();
    assert!(text.starts_with(
        "\nUsage:\n lsirq [options]\n\nUtility to display kernel interrupt information.\n"
    ));
    assert!(text.contains("\n -h, --help           display this help\n"));
    assert!(text.contains(
        "\nAvailable output columns:\n  IRQ    interrupts\n  TOTAL  total count\n  NAME   name\n"
    ));
    assert!(!text.contains("DELTA"));
    assert!(text.ends_with("\nFor more details see lsirq(1).\n"));
}

#[test]
fn a_total_is_printed_signed() {
    let out = IrqOutput {
        columns: vec![Col::Irq, Col::Total, Col::Delta],
        sort: SortBy::Total,
        json: false,
        pairs: false,
        no_headings: true,
    };
    let mut table = get_scols_table(&out, &[irq("X", 5u64.wrapping_neg())]);
    table.set_termforce(smartcols::TermForce::Never);
    let text = String::from_utf8(table.print().unwrap()).unwrap();
    // Each column at least as wide as its header, hidden or not.
    assert_eq!(text, "  X    -5     0\n");
}
