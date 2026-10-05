//! diffutils' `side.c`: `-y`, the two files side by side.

use crate::Opts;
use crate::analyze::Change;
use crate::io::{FileData, Lin};
use crate::util::{self, CHANGED, ColorContext, NEW, OLD, Out};

/// `next0` and `next1`: the next line of each file to print.
pub struct Side {
    next0: Lin,
    next1: Lin,
    /// The output line being built, kept between lines for its allocation:
    /// see [`LineOut`].
    line: Vec<u8>,
}

/// `print_sdiff_script`.
pub fn print_sdiff_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    out.begin_output(o, files);
    let start = files[0].prefix_lines.saturating_neg();
    let mut s = Side {
        next0: start,
        next1: start,
        line: Vec::new(),
    };
    util::print_script(script, util::find_change, o, |hunk| {
        print_sdiff_hunk(&mut s, out, o, files, hunk);
    });
    print_sdiff_common_lines(
        &mut s,
        out,
        o,
        files,
        files[0].valid_lines,
        files[1].valid_lines,
    );
}

/// One line of side-by-side output, built before it is written.
///
/// Upstream writes the line a character at a time with `putc`, which costs it
/// a store; a write here costs a lock on standard output, many times what the
/// rest of the line costs. Building the line first changes no destination's
/// bytes and no order among them: under `-l`, where `outfile` is the pipe to
/// `pr` and the characters upstream writes to standard output go around it,
/// what is built goes out ahead of each of those, and of each colour change.
struct LineOut<'a> {
    out: &'a mut Out,
    buf: Vec<u8>,
}

impl LineOut<'_> {
    /// `putc` to `outfile`.
    fn putc(&mut self, b: u8) {
        self.buf.push(b);
    }

    /// Bytes upstream writes to `stdout` rather than to `outfile`.
    fn put_stdout(&mut self, bytes: &[u8]) {
        if self.out.outfile_is_stdout() {
            self.buf.extend_from_slice(bytes);
        } else {
            self.flush();
            self.out.put_stdout(bytes);
        }
    }

    fn set_color_context(&mut self, ctx: ColorContext) {
        self.flush();
        self.out.set_color_context(ctx);
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            self.out.put(&self.buf);
            self.buf.clear();
        }
    }
}

/// `tab_from_to`: tab (or space) from column `from` to column `to`.
fn tab_from_to(w: &mut LineOut, o: &Opts, mut from: usize, to: usize) -> usize {
    let tab_size = o.tabsize.max(1);
    if !o.expand_tabs {
        let mut tab = from
            .saturating_add(tab_size)
            .saturating_sub(from.checked_rem(tab_size).unwrap_or(0));
        while tab <= to {
            w.putc(b'\t');
            from = tab;
            tab = tab.saturating_add(tab_size);
        }
    }
    while from < to {
        w.putc(b' ');
        from = from.saturating_add(1);
    }
    to
}

/// `print_half_line`: one side of a line, cut to `out_bound` columns with
/// tabs observed and the newline dropped. Returns the last column written.
fn print_half_line(
    w: &mut LineOut,
    o: &Opts,
    line: &[u8],
    indent: usize,
    out_bound: usize,
) -> usize {
    let tabsize = o.tabsize.max(1);
    let mut in_position = 0usize;
    let mut out_position = 0usize;
    let mut rest = line;
    while let [c, tail @ ..] = rest {
        let c = *c;
        let here = rest;
        rest = tail;
        match c {
            b'\t' => {
                let spaces = tabsize.saturating_sub(in_position.checked_rem(tabsize).unwrap_or(0));
                if in_position == out_position {
                    let mut tabstop = out_position.saturating_add(spaces);
                    if o.expand_tabs {
                        if out_bound < tabstop {
                            tabstop = out_bound;
                        }
                        while out_position < tabstop {
                            w.putc(b' ');
                            out_position = out_position.saturating_add(1);
                        }
                    } else if tabstop < out_bound {
                        out_position = tabstop;
                        w.putc(b'\t');
                    }
                }
                in_position = in_position.saturating_add(spaces);
            }
            b'\r' => {
                w.putc(b'\r');
                tab_from_to(w, o, 0, indent);
                in_position = 0;
                out_position = 0;
            }
            0x08 => {
                if in_position != 0 {
                    in_position = in_position.saturating_sub(1);
                    if in_position < out_bound {
                        if out_position <= in_position {
                            // Spaces for a tab suppressed past the bound.
                            while out_position < in_position {
                                w.putc(b' ');
                                out_position = out_position.saturating_add(1);
                            }
                        } else {
                            out_position = in_position;
                            w.putc(c);
                        }
                    }
                }
            }
            b'\n' => return out_position,
            0x20..=0x7e if !matches!(c, b'$' | b'@' | b'`') => {
                // `in_position++ < out_bound`.
                let was = in_position;
                in_position = in_position.saturating_add(1);
                if was < out_bound {
                    out_position = in_position;
                    w.putc(c);
                }
            }
            0x0c | 0x0b => {
                if in_position < out_bound {
                    w.putc(c);
                }
            }
            _ => {
                // `mbrtowc`: one character, if the bytes here make one.
                match decode(here) {
                    Some((ch, bytes)) => {
                        if let Some(width) = charwidth::char_width(ch).filter(|&w| w > 0) {
                            in_position = in_position.saturating_add(width);
                        }
                        if in_position <= out_bound {
                            out_position = in_position;
                            // Upstream writes these to `stdout`, not to
                            // `outfile`: under `-l` they bypass `pr`.
                            w.put_stdout(here.get(..bytes).unwrap_or_default());
                        }
                        rest = here.get(bytes..).unwrap_or_default();
                    }
                    None => {
                        if in_position < out_bound {
                            w.putc(c);
                        }
                    }
                }
            }
        }
    }
    out_position
}

/// One UTF-8 character at the start of `s` and its length; `None` for an
/// invalid or incomplete sequence, and for NUL (`mbrtowc`'s 0).
fn decode(s: &[u8]) -> Option<(char, usize)> {
    let first = *s.first()?;
    if first == 0 {
        return None;
    }
    let len = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let bytes = s.get(..len)?;
    let text = std::str::from_utf8(bytes).ok()?;
    text.chars().next().map(|c| (c, len))
}

/// `print_1sdiff_line`: one output line, either side possibly absent, built
/// in `line` and written whole (see [`LineOut`]).
fn print_1sdiff_line(
    line: &mut Vec<u8>,
    out: &mut Out,
    o: &Opts,
    left: Option<&[u8]>,
    mut sep: u8,
    right: Option<&[u8]>,
) {
    let mut w = LineOut {
        out,
        buf: std::mem::take(line),
    };
    let hw = o.sdiff_half_width;
    let c2o = o.sdiff_column2_offset;
    let mut col = 0usize;
    let mut put_newline = false;
    let mut color_to_reset = false;
    if sep == b'<' {
        w.set_color_context(ColorContext::Delete);
        color_to_reset = true;
    } else if sep == b'>' {
        w.set_color_context(ColorContext::Add);
        color_to_reset = true;
    }
    if let Some(l) = left {
        put_newline |= l.last() == Some(&b'\n');
        col = print_half_line(&mut w, o, l, 0, hw);
    }
    if sep != b' ' {
        col = tab_from_to(&mut w, o, col, hw.saturating_add(c2o).saturating_sub(1) / 2)
            .saturating_add(1);
        if sep == b'|' && put_newline != right.is_some_and(|r| r.last() == Some(&b'\n')) {
            sep = if put_newline { b'/' } else { b'\\' };
        }
        w.putc(sep);
    }
    if let Some(r) = right {
        put_newline |= r.last() == Some(&b'\n');
        if r.first() != Some(&b'\n') {
            col = tab_from_to(&mut w, o, col, c2o);
            print_half_line(&mut w, o, r, col, hw);
        }
    }
    if put_newline {
        w.putc(b'\n');
    }
    if color_to_reset {
        w.set_color_context(ColorContext::Reset);
    }
    w.flush();
    *line = w.buf;
}

/// `print_sdiff_common_lines`: the lines both files share up to the limits.
fn print_sdiff_common_lines(
    s: &mut Side,
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    limit0: Lin,
    limit1: Lin,
) {
    let mut i0 = s.next0;
    let mut i1 = s.next1;
    if !o.suppress_common_lines && (i0 != limit0 || i1 != limit1) {
        if o.sdiff_merge_assist {
            out.put(
                format!(
                    "i{},{}\n",
                    limit0.saturating_sub(i0),
                    limit1.saturating_sub(i1)
                )
                .as_bytes(),
            );
        }
        if !o.left_column {
            while i0 != limit0 && i1 != limit1 {
                print_1sdiff_line(
                    &mut s.line,
                    out,
                    o,
                    Some(files[0].line(i0)),
                    b' ',
                    Some(files[1].line(i1)),
                );
                i0 = i0.saturating_add(1);
                i1 = i1.saturating_add(1);
            }
            while i1 != limit1 {
                print_1sdiff_line(&mut s.line, out, o, None, b')', Some(files[1].line(i1)));
                i1 = i1.saturating_add(1);
            }
        }
        while i0 != limit0 {
            print_1sdiff_line(&mut s.line, out, o, Some(files[0].line(i0)), b'(', None);
            i0 = i0.saturating_add(1);
        }
    }
    s.next0 = limit0;
    s.next1 = limit1;
}

/// `print_sdiff_hunk`.
fn print_sdiff_hunk(s: &mut Side, out: &mut Out, o: &Opts, files: &[FileData; 2], hunk: &[Change]) {
    let (mut changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    let (mut first0, last0, mut first1, last1) = (r.first0, r.last0, r.first1, r.last1);
    print_sdiff_common_lines(s, out, o, files, first0, first1);
    if o.sdiff_merge_assist {
        out.put(
            format!(
                "c{},{}\n",
                last0.saturating_sub(first0).saturating_add(1),
                last1.saturating_sub(first1).saturating_add(1)
            )
            .as_bytes(),
        );
    }
    if changes == CHANGED {
        let mut i = first0;
        let mut j = first1;
        while i <= last0 && j <= last1 {
            print_1sdiff_line(
                &mut s.line,
                out,
                o,
                Some(files[0].line(i)),
                b'|',
                Some(files[1].line(j)),
            );
            i = i.saturating_add(1);
            j = j.saturating_add(1);
        }
        changes = (if i <= last0 { OLD } else { 0 }) | (if j <= last1 { NEW } else { 0 });
        s.next0 = i;
        first0 = i;
        s.next1 = j;
        first1 = j;
    }
    if changes & NEW != 0 {
        let mut j = first1;
        while j <= last1 {
            print_1sdiff_line(&mut s.line, out, o, None, b'>', Some(files[1].line(j)));
            j = j.saturating_add(1);
        }
        s.next1 = j;
    }
    if changes & OLD != 0 {
        let mut i = first0;
        while i <= last0 {
            print_1sdiff_line(&mut s.line, out, o, Some(files[0].line(i)), b'<', None);
            i = i.saturating_add(1);
        }
        s.next0 = i;
    }
}
