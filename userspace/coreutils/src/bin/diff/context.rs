//! diffutils' `context.c`: `-c` and `-u`.

use crate::Opts;
use crate::analyze::Change;
use crate::io::{FileData, Lin};
use crate::util::{self, ColorContext, NEW, OLD, Out, Range};

/// `print_context_label`: `*** NAME\tTIME`, or the `--label` given.
fn print_context_label(
    out: &mut Out,
    o: &Opts,
    mark: &[u8],
    file: &FileData,
    name: &[u8],
    label: Option<&[u8]>,
) {
    out.set_color_context(ColorContext::Header);
    out.put(mark);
    out.put(b" ");
    match label {
        Some(l) => out.put(l),
        None => {
            out.put(name);
            out.put(b"\t");
            let tm = o.zone.localtime(file.stat.mtime_sec, file.stat.mtime_nsec);
            out.put(&localtime::nstrftime(o.time_format, &tm));
        }
    }
    out.set_color_context(ColorContext::Reset);
    out.put(b"\n");
}

/// `print_context_header`.
pub fn print_context_header(
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    names: &[Vec<u8>; 2],
    unidiff: bool,
) {
    let (m0, m1): (&[u8], &[u8]) = if unidiff {
        (b"---", b"+++")
    } else {
        (b"***", b"---")
    };
    print_context_label(out, o, m0, &files[0], &names[0], o.file_label[0].as_deref());
    print_context_label(out, o, m1, &files[1], &names[1], o.file_label[1].as_deref());
}

/// The state `find_function` keeps between hunks.
pub struct FindFunction {
    last_search: Lin,
    last_match: Option<Lin>,
}

/// `print_context_script`.
pub fn print_context_script(
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    script: &mut [Change],
    unidiff: bool,
) {
    if o.ignore_blank_lines || o.ignore_regexp.is_some() {
        mark_ignorable(script, files, o);
    } else {
        for c in script.iter_mut() {
            c.ignore = false;
        }
    }
    let mut ff = FindFunction {
        last_search: files[0].prefix_lines.saturating_neg(),
        last_match: None,
    };
    let script: &[Change] = script;
    util::print_script(script, find_hunk, o, |hunk| {
        if unidiff {
            pr_unidiff_hunk(out, o, files, hunk, &mut ff);
        } else {
            pr_context_hunk(out, o, files, hunk, &mut ff);
        }
    });
}

/// `print_context_number_range`: `A,B`, or one number.
fn print_context_number_range(out: &mut Out, file: &FileData, a: Lin, b: Lin) {
    let (ta, tb) = util::translate_range(file, a, b);
    if tb <= ta {
        out.put(tb.to_string().as_bytes());
    } else {
        out.put(format!("{ta},{tb}").as_bytes());
    }
}

/// `print_context_function`: the matching line after a hunk header, without
/// leading white space and cut at 40 bytes.
fn print_context_function(out: &mut Out, function: &[u8]) {
    out.put(b" ");
    // The buffer holds a newline after every line, even a last one whose
    // newline was supplied; the scans stop there.
    let text = function.split(|&b| b == b'\n').next().unwrap_or_default();
    let i = text.iter().take_while(|&&c| crate::io::is_space(c)).count();
    let mut j = i.saturating_add(40).min(text.len());
    while i < j
        && text
            .get(j.saturating_sub(1))
            .is_some_and(|&c| crate::io::is_space(c))
    {
        j = j.saturating_sub(1);
    }
    out.put(text.get(i..j).unwrap_or_default());
}

/// The hunk's ranges widened by the context, clipped to the file.
fn widen(r: Range, files: &[FileData; 2], o: &Opts) -> Range {
    let i = files[0].prefix_lines.saturating_neg();
    let ctx = o.context;
    let last = |l: Lin, f: &FileData| {
        if l < f.valid_lines.saturating_sub(ctx) {
            l.saturating_add(ctx)
        } else {
            f.valid_lines.saturating_sub(1)
        }
    };
    Range {
        first0: r.first0.saturating_sub(ctx).max(i),
        first1: r.first1.saturating_sub(ctx).max(i),
        last0: last(r.last0, &files[0]),
        last1: last(r.last1, &files[1]),
    }
}

/// `pr_context_hunk`.
fn pr_context_hunk(
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    hunk: &[Change],
    ff: &mut FindFunction,
) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    let r = widen(r, files, o);
    let function = if o.function_regexp.is_some() {
        find_function(files, r.first0, ff, o)
    } else {
        None
    };

    out.begin_output(o, files);
    out.put(b"***************");
    if let Some(f) = function {
        print_context_function(out, files[0].line(f));
    }
    out.put(b"\n");
    out.set_color_context(ColorContext::LineNumber);
    out.put(b"*** ");
    print_context_number_range(out, &files[0], r.first0, r.last0);
    out.put(b" ****");
    out.set_color_context(ColorContext::Reset);
    out.put(b"\n");

    if changes & OLD != 0 {
        let mut next = hunk.iter().peekable();
        let mut i = r.first0;
        while i <= r.last0 {
            out.set_color_context(ColorContext::Delete);
            while next
                .peek()
                .is_some_and(|c| c.line0.saturating_add(c.deleted) <= i)
            {
                next.next();
            }
            let prefix: &[u8] = match next.peek() {
                Some(c) if c.line0 <= i => {
                    if c.inserted > 0 {
                        b"!"
                    } else {
                        b"-"
                    }
                }
                _ => b" ",
            };
            let line = files[0].line(i);
            util::print_1_line_nl(out, o, Some(prefix), line, true);
            out.set_color_context(ColorContext::Reset);
            if line.last() == Some(&b'\n') {
                out.put(b"\n");
            }
            i = i.saturating_add(1);
        }
    }

    out.set_color_context(ColorContext::LineNumber);
    out.put(b"--- ");
    print_context_number_range(out, &files[1], r.first1, r.last1);
    out.put(b" ----");
    out.set_color_context(ColorContext::Reset);
    out.put(b"\n");

    if changes & NEW != 0 {
        let mut next = hunk.iter().peekable();
        let mut i = r.first1;
        while i <= r.last1 {
            out.set_color_context(ColorContext::Add);
            while next
                .peek()
                .is_some_and(|c| c.line1.saturating_add(c.inserted) <= i)
            {
                next.next();
            }
            let prefix: &[u8] = match next.peek() {
                Some(c) if c.line1 <= i => {
                    if c.deleted > 0 {
                        b"!"
                    } else {
                        b"+"
                    }
                }
                _ => b" ",
            };
            let line = files[1].line(i);
            util::print_1_line_nl(out, o, Some(prefix), line, true);
            out.set_color_context(ColorContext::Reset);
            if line.last() == Some(&b'\n') {
                out.put(b"\n");
            }
            i = i.saturating_add(1);
        }
    }
}

/// `print_unidiff_number_range`: `A,N`; `A` alone for one line, `B,0` for
/// none.
fn print_unidiff_number_range(out: &mut Out, file: &FileData, a: Lin, b: Lin) {
    let (ta, tb) = util::translate_range(file, a, b);
    if tb <= ta {
        if tb < ta {
            out.put(format!("{tb},0").as_bytes());
        } else {
            out.put(tb.to_string().as_bytes());
        }
    } else {
        out.put(format!("{ta},{}", tb.saturating_sub(ta).saturating_add(1)).as_bytes());
    }
}

/// `pr_unidiff_hunk`.
fn pr_unidiff_hunk(
    out: &mut Out,
    o: &Opts,
    files: &[FileData; 2],
    hunk: &[Change],
    ff: &mut FindFunction,
) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    let r = widen(r, files, o);
    let function = if o.function_regexp.is_some() {
        find_function(files, r.first0, ff, o)
    } else {
        None
    };

    out.begin_output(o, files);
    out.set_color_context(ColorContext::LineNumber);
    out.put(b"@@ -");
    print_unidiff_number_range(out, &files[0], r.first0, r.last0);
    out.put(b" +");
    print_unidiff_number_range(out, &files[1], r.first1, r.last1);
    out.put(b" @@");
    out.set_color_context(ColorContext::Reset);
    if let Some(f) = function {
        print_context_function(out, files[0].line(f));
    }
    out.put(b"\n");

    let mut next = hunk.iter().peekable();
    let mut i = r.first0;
    let mut j = r.first1;
    while i <= r.last0 || j <= r.last1 {
        match next.peek().copied() {
            Some(c) if i >= c.line0 => {
                // A difference: the deleted part, then the inserted part.
                for _ in 0..c.deleted.max(0) {
                    let line = files[0].line(i);
                    i = i.saturating_add(1);
                    out.set_color_context(ColorContext::Delete);
                    out.put(b"-");
                    if o.initial_tab && !(o.suppress_blank_empty && line.first() == Some(&b'\n')) {
                        out.put(b"\t");
                    }
                    util::print_1_line_nl(out, o, None, line, true);
                    out.set_color_context(ColorContext::Reset);
                    if line.last() == Some(&b'\n') {
                        out.put(b"\n");
                    }
                }
                for _ in 0..c.inserted.max(0) {
                    let line = files[1].line(j);
                    j = j.saturating_add(1);
                    out.set_color_context(ColorContext::Add);
                    out.put(b"+");
                    if o.initial_tab && !(o.suppress_blank_empty && line.first() == Some(&b'\n')) {
                        out.put(b"\t");
                    }
                    util::print_1_line_nl(out, o, None, line, true);
                    out.set_color_context(ColorContext::Reset);
                    if line.last() == Some(&b'\n') {
                        out.put(b"\n");
                    }
                }
                next.next();
            }
            _ => {
                // Context, from file 0.
                let line = files[0].line(i);
                i = i.saturating_add(1);
                if !(o.suppress_blank_empty && line.first() == Some(&b'\n')) {
                    out.put(if o.initial_tab { b"\t" } else { b" " });
                }
                util::print_1_line_nl(out, o, None, line, false);
                j = j.saturating_add(1);
            }
        }
    }
}

/// `find_hunk`: the changes that belong to one hunk -- until more than
/// twice the context (or, before an ignorable change, more than the context)
/// unchanged lines separate two.
pub fn find_hunk(script: &[Change], o: &Opts) -> usize {
    let ignorable_threshold = o.context;
    let non_ignorable_threshold = o.context.saturating_mul(2).saturating_add(1);
    let mut k = 0usize;
    loop {
        let Some(cur) = script.get(k) else {
            return k;
        };
        let top0 = cur.line0.saturating_add(cur.deleted);
        k = k.saturating_add(1);
        let Some(next) = script.get(k) else {
            return k;
        };
        let thresh = if next.ignore {
            ignorable_threshold
        } else {
            non_ignorable_threshold
        };
        if next.line0.saturating_sub(top0) >= thresh {
            return k;
        }
    }
}

/// `mark_ignorable`: set each change's `ignore`.
fn mark_ignorable(script: &mut [Change], files: &[FileData; 2], o: &Opts) {
    for c in script.iter_mut() {
        let (changes, _) = util::analyze_hunk(std::slice::from_ref(c), files, o);
        c.ignore = changes == 0;
    }
}

/// `find_function`: the last line before `linenum` that the `-F` expression
/// matches, searching back no further than the last search began.
fn find_function(
    files: &[FileData; 2],
    linenum: Lin,
    ff: &mut FindFunction,
    o: &Opts,
) -> Option<Lin> {
    let last = ff.last_search;
    ff.last_search = linenum;
    let re = o.function_regexp.as_ref()?;
    let mut i = linenum;
    while last <= i.saturating_sub(1) {
        i = i.saturating_sub(1);
        let line = files[0].line(i);
        // `linelen = linbuf[i + 1] - line - 1`: the last byte is taken to be a
        // newline, so a last line without one loses its final character here,
        // as it does upstream.
        let text = line.get(..line.len().saturating_sub(1)).unwrap_or_default();
        if crate::regex_matches(re, text) {
            ff.last_match = Some(i);
            return Some(i);
        }
    }
    ff.last_match
}
