//! diffutils' `ifdef.c`: `-D` and the `--*-group-format`/`--*-line-format`
//! options -- a merged file, each group of lines printed through a format.

use crate::Opts;
use crate::analyze::Change;
use crate::io::{FileData, Lin};
use crate::util::{self, Out, UNCHANGED};

/// A group of lines: `from` up to `upto` of `file`.
#[derive(Clone, Copy)]
struct Group {
    file: usize,
    from: Lin,
    upto: Lin,
}

/// `next_line0`, `next_line1`.
struct Next {
    line0: Lin,
    line1: Lin,
}

/// `group_format[k]`, for `k` one of `UNCHANGED`, `OLD`, `NEW`, `CHANGED`.
fn group_format(o: &Opts, k: u8) -> &[u8] {
    o.group_format
        .get(usize::from(k))
        .map_or(&[][..], Vec::as_slice)
}

/// `line_format[k]`, for `k` one of `UNCHANGED`, `OLD`, `NEW`.
fn line_format(o: &Opts, k: u8) -> &[u8] {
    o.line_format
        .get(usize::from(k))
        .map_or(&[][..], Vec::as_slice)
}

/// `print_ifdef_script`.
pub fn print_ifdef_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    let start = files[0].prefix_lines.saturating_neg();
    let mut next = Next {
        line0: start,
        line1: start,
    };
    util::print_script(script, util::find_change, o, |hunk| {
        print_ifdef_hunk(&mut next, out, o, files, hunk);
    });
    if next.line0 < files[0].valid_lines || next.line1 < files[1].valid_lines {
        out.begin_output(o, files);
        format_ifdef(
            out,
            o,
            files,
            group_format(o, UNCHANGED),
            next.line0,
            files[0].valid_lines,
            next.line1,
            files[1].valid_lines,
        );
    }
}

/// `print_ifdef_hunk`.
fn print_ifdef_hunk(
    next: &mut Next,
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    hunk: &[Change],
) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    out.begin_output(o, files);
    if next.line0 < r.first0 || next.line1 < r.first1 {
        format_ifdef(
            out,
            o,
            files,
            group_format(o, UNCHANGED),
            next.line0,
            r.first0,
            next.line1,
            r.first1,
        );
    }
    next.line0 = r.last0.saturating_add(1);
    next.line1 = r.last1.saturating_add(1);
    format_ifdef(
        out,
        o,
        files,
        group_format(o, changes),
        r.first0,
        next.line0,
        r.first1,
        next.line1,
    );
}

/// `format_ifdef`.
#[allow(clippy::too_many_arguments)]
fn format_ifdef(
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    format: &[u8],
    beg0: Lin,
    end0: Lin,
    beg1: Lin,
    end1: Lin,
) {
    let groups = [
        Group {
            file: 0,
            from: beg0,
            upto: end0,
        },
        Group {
            file: 1,
            from: beg1,
            upto: end1,
        },
    ];
    format_group(Some(out), o, files, format, 0, 0, &groups);
}

/// `format_group`: print `format` from `f` up to the first unnested
/// `endchar` (or the end), and return where it stopped. With no `out`, only
/// scan.
fn format_group(
    mut out: Option<&mut Out>,
    o: &Opts,
    files: &[FileData; 2],
    format: &[u8],
    mut f: usize,
    endchar: u8,
    groups: &[Group; 2],
) -> usize {
    while let Some(&c0) = format.get(f) {
        if c0 == endchar || c0 == 0 {
            break;
        }
        f = f.saturating_add(1);
        let f1 = f;
        let mut c = c0;
        if c0 == b'%' {
            let spec = format.get(f).copied().unwrap_or(0);
            f = f.saturating_add(1);
            match spec {
                b'%' => c = b'%',
                b'(' => {
                    // `%(A=B?THEN:ELSE)`.
                    match if_then_else(out.as_deref_mut(), o, files, format, f, groups) {
                        Some(next) => {
                            f = next;
                            continue;
                        }
                        None => {
                            c = b'%';
                            f = f1;
                        }
                    }
                }
                b'<' => {
                    print_ifdef_lines(
                        out.as_deref_mut(),
                        o,
                        files,
                        line_format(o, util::OLD),
                        &groups[0],
                    );
                    continue;
                }
                b'=' => {
                    print_ifdef_lines(
                        out.as_deref_mut(),
                        o,
                        files,
                        line_format(o, UNCHANGED),
                        &groups[0],
                    );
                    continue;
                }
                b'>' => {
                    print_ifdef_lines(
                        out.as_deref_mut(),
                        o,
                        files,
                        line_format(o, util::NEW),
                        &groups[1],
                    );
                    continue;
                }
                _ => match do_printf_spec(
                    out.as_deref_mut(),
                    format,
                    f1.saturating_sub(1),
                    files,
                    None,
                    Some(groups),
                ) {
                    Some(next) => {
                        f = next;
                        continue;
                    }
                    None => {
                        c = b'%';
                        f = f1;
                    }
                },
            }
        }
        if let Some(o2) = out.as_deref_mut() {
            o2.putc(c);
        }
    }
    f
}

/// The `%(` arm of `format_group`; `None` for a malformed condition.
fn if_then_else(
    out: Option<&mut Out>,
    o: &Opts,
    files: &[FileData; 2],
    format: &[u8],
    mut f: usize,
    groups: &[Group; 2],
) -> Option<usize> {
    let mut value = [0i64; 2];
    for (i, v) in value.iter_mut().enumerate() {
        let c = format.get(f).copied().unwrap_or(0);
        if c.is_ascii_digit() {
            // `strtoimax`, failing on overflow.
            let digits = format
                .get(f..)
                .unwrap_or_default()
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            let text = format.get(f..f.saturating_add(digits)).unwrap_or_default();
            *v = std::str::from_utf8(text).ok()?.parse::<i64>().ok()?;
            f = f.saturating_add(digits);
        } else {
            *v = i64::try_from(groups_letter_value(files, groups, c)).ok()?;
            if *v < 0 {
                return None;
            }
            f = f.saturating_add(1);
        }
        let want = if i == 0 { b'=' } else { b'?' };
        let got = format.get(f).copied().unwrap_or(0);
        f = f.saturating_add(1);
        if got != want {
            return None;
        }
    }
    let (thenout, elseout) = if value[0] == value[1] {
        (out, None)
    } else {
        (None, out)
    };
    f = format_group(thenout, o, files, format, f, b':', groups);
    if format.get(f).is_some_and(|&b| b != 0) {
        f = format_group(elseout, o, files, format, f.saturating_add(1), b')', groups);
        if format.get(f).is_some_and(|&b| b != 0) {
            f = f.saturating_add(1);
        }
    }
    Some(f)
}

/// `groups_letter_value`: `%e %f %l %m %n` for the old group, capitals for
/// the new; -1 for any other letter.
fn groups_letter_value(files: &[FileData; 2], groups: &[Group; 2], letter: u8) -> Lin {
    let (g, letter) = match letter {
        b'E' | b'F' | b'L' | b'M' | b'N' => (&groups[1], letter.to_ascii_lowercase()),
        _ => (&groups[0], letter),
    };
    let Some(file) = files.get(g.file) else {
        return -1;
    };
    match letter {
        b'e' => util::translate_line_number(file, g.from).saturating_sub(1),
        b'f' => util::translate_line_number(file, g.from),
        b'l' => util::translate_line_number(file, g.upto).saturating_sub(1),
        b'm' => util::translate_line_number(file, g.upto),
        b'n' => g.upto.saturating_sub(g.from),
        _ => -1,
    }
}

/// `print_ifdef_lines`: each line of the group through `format`.
fn print_ifdef_lines(
    out: Option<&mut Out>,
    o: &Opts,
    files: &[FileData; 2],
    format: &[u8],
    group: &Group,
) {
    let Some(out) = out else {
        return;
    };
    let Some(file) = files.get(group.file) else {
        return;
    };
    let (from, upto) = (group.from, group.upto);

    // The single-write shortcuts upstream takes for `%l\n` and `%L`.
    if !o.expand_tabs && format.first() == Some(&b'%') {
        if format.get(1..) == Some(b"l\n".as_slice()) && from < upto {
            let a = file.line_start(from);
            let mut b = file.line_start(upto);
            let last_has_nl = file.buffer.get(b.saturating_sub(1)) == Some(&b'\n');
            if !last_has_nl {
                b = b.saturating_add(1);
            }
            out.put(file.buffer.get(a..b).unwrap_or_default());
            return;
        }
        if format.get(1..) == Some(b"L".as_slice()) {
            let a = file.line_start(from);
            let b = file.line_start(upto);
            out.put(file.buffer.get(a..b.max(a)).unwrap_or_default());
            return;
        }
    }

    let mut i = from;
    while i < upto {
        let mut f = 0usize;
        while let Some(&c0) = format.get(f) {
            f = f.saturating_add(1);
            let f1 = f;
            let mut c = c0;
            if c0 == b'%' {
                let spec = format.get(f).copied().unwrap_or(0);
                f = f.saturating_add(1);
                match spec {
                    b'%' => c = b'%',
                    b'l' => {
                        let line = file.line(i);
                        let text = line.strip_suffix(b"\n").unwrap_or(line);
                        util::output_1_line(out, o, text, None);
                        continue;
                    }
                    b'L' => {
                        util::output_1_line(out, o, file.line(i), None);
                        continue;
                    }
                    _ => match do_printf_spec(
                        Some(out),
                        format,
                        f1.saturating_sub(1),
                        files,
                        Some((group.file, i)),
                        None,
                    ) {
                        Some(next) => {
                            f = next;
                            continue;
                        }
                        None => {
                            c = b'%';
                            f = f1;
                        }
                    },
                }
            }
            out.putc(c);
        }
        i = i.saturating_add(1);
    }
}

/// `do_printf_spec`: `%[-'0]*[0-9]*(.[0-9]*)?[cdoxX]` followed by the
/// letter it formats; `None` if `spec` is not one.
fn do_printf_spec(
    out: Option<&mut Out>,
    format: &[u8],
    spec: usize,
    files: &[FileData; 2],
    line: Option<(usize, Lin)>,
    groups: Option<&[Group; 2]>,
) -> Option<usize> {
    let at = |i: usize| format.get(i).copied().unwrap_or(0);
    let mut f = spec.saturating_add(1);
    let mut minus = false;
    let mut zero = false;
    let mut c;
    loop {
        c = at(f);
        f = f.saturating_add(1);
        match c {
            b'-' => minus = true,
            b'\'' => {}
            b'0' => zero = true,
            _ => break,
        }
    }
    let mut width = 0usize;
    while c.is_ascii_digit() {
        width = width
            .saturating_mul(10)
            .saturating_add(usize::from(c.saturating_sub(b'0')));
        c = at(f);
        f = f.saturating_add(1);
    }
    let mut precision = None;
    if c == b'.' {
        let mut p = 0usize;
        loop {
            c = at(f);
            f = f.saturating_add(1);
            if !c.is_ascii_digit() {
                break;
            }
            p = p
                .saturating_mul(10)
                .saturating_add(usize::from(c.saturating_sub(b'0')));
        }
        precision = Some(p);
    }
    let c1 = at(f);
    f = f.saturating_add(1);

    match c {
        b'c' => {
            if c1 != b'\'' {
                return None;
            }
            let (value, next) = scan_char_literal(format, f)?;
            if let Some(out) = out {
                out.putc(value);
            }
            Some(next)
        }
        b'd' | b'o' | b'x' | b'X' => {
            let value = match (line, groups) {
                (Some((file, n)), _) => {
                    if c1 != b'n' {
                        return None;
                    }
                    util::translate_line_number(files.get(file)?, n)
                }
                (None, Some(g)) => {
                    let v = groups_letter_value(files, g, c1);
                    if v < 0 {
                        return None;
                    }
                    v
                }
                (None, None) => return None,
            };
            if let Some(out) = out {
                let spec = cprintf::cfmt::Spec {
                    minus,
                    plus: false,
                    space: false,
                    hash: false,
                    zero,
                    width,
                    precision,
                    conv: c,
                };
                let v = if c == b'd' {
                    cprintf::cfmt::Value::Signed(i64::try_from(value).unwrap_or(i64::MAX))
                } else {
                    // `%lo`/`%lx` of a negative `long` print its two's
                    // complement, as the unsigned reading does.
                    cprintf::cfmt::Value::Unsigned(
                        i64::try_from(value).unwrap_or(0).cast_unsigned(),
                    )
                };
                out.put(&cprintf::cfmt::render(&spec, v));
            }
            Some(f)
        }
        _ => None,
    }
}

/// `scan_char_literal`: `C'` or `\OOO'` after `%c'`; the byte and where the
/// literal ends.
fn scan_char_literal(format: &[u8], lit: usize) -> Option<(u8, usize)> {
    let at = |i: usize| format.get(i).copied().unwrap_or(0);
    let mut p = lit;
    let c = at(p);
    p = p.saturating_add(1);
    match c {
        0 | b'\'' => None,
        b'\\' => {
            let mut value: u8 = 0;
            loop {
                let c = at(p);
                p = p.saturating_add(1);
                if c == b'\'' {
                    break;
                }
                let digit = c.wrapping_sub(b'0');
                if 8 <= digit {
                    return None;
                }
                value = value.wrapping_mul(8).wrapping_add(digit);
            }
            let digits = p.saturating_sub(lit).saturating_sub(2);
            if !(1..=3).contains(&digits) {
                return None;
            }
            Some((value, p))
        }
        _ => {
            if at(p) != b'\'' {
                return None;
            }
            Some((c, p.saturating_add(1)))
        }
    }
}
