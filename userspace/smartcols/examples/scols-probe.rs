//! The port's side of `scripts/smartcols-diff.sh`: a table built from a
//! script on stdin and printed, with the answer of every call that can
//! refuse written to stderr -- byte for byte what `scripts/scols-probe.c`
//! writes when it runs the same script through util-linux's libsmartcols.
//! The script's commands are described there.

use std::io::{Read, Write};

use smartcols::{CellView, ColumnId, Error, JsonType, LineId, Table, TermForce};

/// A call's answer as upstream's: 0, or `-EINVAL`.
fn rc(r: Result<(), Error>) -> &'static str {
    match r {
        Ok(()) => "0",
        Err(Error::Invalid) => "-22",
    }
}

/// Hex to bytes; `-` is none. Like the C side, the bytes end at a NUL.
fn unhex(s: &[u8]) -> Option<Vec<u8>> {
    if s == b"-" {
        return None;
    }
    let mut out = Vec::new();
    for pair in s.as_chunks::<2>().0 {
        match std::str::from_utf8(pair)
            .ok()
            .and_then(|p| u8::from_str_radix(p, 16).ok())
        {
            Some(b) => out.push(b),
            None => std::process::exit(98),
        }
    }
    if let Some(nul) = out.iter().position(|&b| b == 0) {
        out.truncate(nul);
    }
    Some(out)
}

/// `parts`, then a newline, onto the answers written to stderr.
fn say(err: &mut Vec<u8>, parts: &[&[u8]]) {
    for p in parts {
        err.extend_from_slice(p);
    }
    err.push(b'\n');
}

/// `strtol`'s answer for a small decimal, as an index; -1 or garbage is none.
fn index(s: &[u8]) -> Option<usize> {
    std::str::from_utf8(s).ok()?.parse::<usize>().ok()
}

/// lsblk's `cmp_u64_cells`.
fn cmp_u64_cells(a: CellView<'_>, b: CellView<'_>) -> i32 {
    match (a.userdata(), b.userdata()) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => {
            if x == y {
                0
            } else if x >= y {
                1
            } else {
                -1
            }
        }
    }
}

fn json_type(s: &[u8]) -> JsonType {
    match s {
        b"number" => JsonType::Number,
        b"boolean" => JsonType::Boolean,
        b"array-string" => JsonType::ArrayString,
        b"array-number" => JsonType::ArrayNumber,
        b"boolean-optional" => JsonType::BooleanOptional,
        _ => JsonType::String,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per command, as the C side's one chain of ifs"
)]
fn main() {
    let mut script = Vec::new();
    if std::io::stdin().read_to_end(&mut script).is_err() {
        std::process::exit(99);
    }
    let mut tb = Table::new();
    tb.set_termforce(TermForce::Never);
    let mut cols: Vec<ColumnId> = Vec::new();
    let mut lines: Vec<LineId> = Vec::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let col_at = |cols: &[ColumnId], s: &[u8]| index(s).and_then(|i| cols.get(i).copied());
    let line_at = |lines: &[LineId], s: &[u8]| index(s).and_then(|i| lines.get(i).copied());

    for cmd in script.split(|&b| b == b'\n') {
        let f: Vec<&[u8]> = cmd
            .split(|&b| b == b' ')
            .filter(|w| !w.is_empty())
            .collect();
        let (Some(&name), n) = (f.first(), f.len()) else {
            continue;
        };
        let arg = |i: usize| f.get(i).copied().unwrap_or_default();
        match (name, n) {
            (b"col", 4) => {
                let mut flags = 0;
                let mut wrapnl = false;
                if arg(2) != b"-" {
                    for &c in arg(2) {
                        flags |= match c {
                            b't' => smartcols::FL_TREE,
                            b'r' => smartcols::FL_RIGHT,
                            b'c' => smartcols::FL_TRUNC,
                            b'w' => smartcols::FL_WRAP,
                            b'n' => smartcols::FL_NOEXTREMES,
                            b's' => smartcols::FL_STRICTWIDTH,
                            b'h' => smartcols::FL_HIDDEN,
                            b'N' => {
                                wrapnl = true;
                                0
                            }
                            _ => 0,
                        };
                    }
                }
                let whint = std::str::from_utf8(arg(3))
                    .ok()
                    .and_then(|w| w.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let cl = match unhex(arg(1)) {
                    Some(name) => tb.new_column(&name, whint, flags),
                    None => tb.new_unnamed_column(whint, flags),
                };
                if wrapnl {
                    // Both refuse only a column not the table's, which `cl`
                    // is.
                    let _ = tb.column_set_wrapnl(cl);
                    let _ = tb.column_set_safechars(cl, b"\n");
                }
                cols.push(cl);
            }
            (b"jtype", 3) => {
                if let Some(cl) = col_at(&cols, arg(1)) {
                    // The column is the table's.
                    let _ = tb.column_set_json_type(cl, json_type(arg(2)));
                }
            }
            (b"line", 2) => match tb.new_line(line_at(&lines, arg(1))) {
                Ok(ln) => lines.push(ln),
                Err(_) => std::process::exit(97),
            },
            (b"data", 4) => {
                if let (Some(ln), Some(cl), Some(d)) = (
                    line_at(&lines, arg(1)),
                    col_at(&cols, arg(2)),
                    unhex(arg(3)),
                ) {
                    // Both are the table's.
                    let _ = tb.line_set_data(ln, cl, &d);
                }
            }
            (b"udata", 4) => {
                let x = std::str::from_utf8(arg(3))
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                if let (Some(ln), Some(cell)) = (line_at(&lines, arg(1)), index(arg(2))) {
                    // The C side crashes on a cell that is not there; the
                    // scripts never name one.
                    let _ = tb.cell_set_userdata(ln, cell, x);
                }
            }
            (b"group", 3) => {
                let r = match line_at(&lines, arg(2)) {
                    Some(member) => tb.group_lines(line_at(&lines, arg(1)), member),
                    None => Err(Error::Invalid),
                };
                say(
                    &mut err,
                    &[b"group ", arg(1), b" ", arg(2), b": ", rc(r).as_bytes()],
                );
            }
            (b"link", 3) => {
                let r = match (line_at(&lines, arg(1)), line_at(&lines, arg(2))) {
                    (Some(ln), Some(member)) => tb.line_link_group(ln, member),
                    _ => Err(Error::Invalid),
                };
                say(
                    &mut err,
                    &[b"link ", arg(1), b" ", arg(2), b": ", rc(r).as_bytes()],
                );
            }
            (b"cmp", 3) => {
                if let Some(cl) = col_at(&cols, arg(1)) {
                    // Refused only for a column not the table's.
                    let f: smartcols::CmpFunc = if arg(2) == b"u64" {
                        cmp_u64_cells
                    } else {
                        smartcols::cmpstr_cells
                    };
                    let _ = tb.column_set_cmpfunc(cl, f);
                }
            }
            (b"sort", 2) => {
                let r = tb.sort(col_at(&cols, arg(1)));
                say(&mut err, &[b"sort ", arg(1), b": ", rc(r).as_bytes()]);
            }
            (b"sorttree", 1) => {
                tb.sort_by_tree();
                say(&mut err, &[b"sorttree: 0"]);
            }
            (b"ascii", _) => tb.enable_ascii(true),
            (b"json", _) => tb.enable_json(true),
            (b"raw", _) => tb.enable_raw(true),
            (b"export", _) => tb.enable_export(true),
            (b"noheadings", _) => tb.enable_noheadings(true),
            (b"maxout", _) => {
                // Refused, as upstream's is, when minout is on.
                let _ = tb.enable_maxout(true);
            }
            (b"minout", _) => {
                // Refused, as upstream's is, when maxout is on.
                let _ = tb.enable_minout(true);
            }
            (b"nowrap", _) => tb.enable_nowrap(true),
            (b"noencoding", _) => tb.enable_noencoding(true),
            (b"nolinesep", _) => tb.enable_nolinesep(true),
            (b"shellvar", _) => tb.enable_shellvar(true),
            (b"name", 2) => {
                if let Some(d) = unhex(arg(1)) {
                    tb.set_name(&d);
                }
            }
            (b"term", 2) => {
                tb.set_termforce(TermForce::Always);
                tb.set_termwidth(index(arg(1)).unwrap_or(0));
            }
            (b"print", _) => {
                let r = tb.print_into(&mut out);
                say(&mut err, &[b"print: ", rc(r).as_bytes()]);
            }
            _ => {
                say(&mut err, &[b"bad command: ", arg(0)]);
                // Exiting either way; a failed write has nowhere to go.
                let _ = std::io::stderr().write_all(&err);
                std::process::exit(2);
            }
        }
        // Upstream's stderr is unbuffered: each answer is out before the next
        // command runs -- and before an abort. A failed write is the
        // harness's to notice, as a difference.
        let _ = std::io::stderr().write_all(&err);
        err.clear();
    }
    // Upstream's stdout is a pipe's, flushed at exit (and lost on an abort,
    // as this is).
    if std::io::stdout().write_all(&out).is_err() {
        std::process::exit(1);
    }
}
