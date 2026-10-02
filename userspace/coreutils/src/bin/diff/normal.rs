//! diffutils' `normal.c` and `ed.c`: the default format, `-e`, `-f` and `-n`.

use crate::Opts;
use crate::analyze::Change;
use crate::io::FileData;
use crate::util::{self, CHANGED, ColorContext, NEW, OLD, Out, change_letter};

/// `print_normal_script`.
pub fn print_normal_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    util::print_script(script, util::find_change, o, |hunk| {
        print_normal_hunk(out, o, files, hunk);
    });
}

/// `print_normal_hunk`: `RANGEcRANGE`, `< ` lines, `---`, `> ` lines.
fn print_normal_hunk(out: &mut Out, o: &Opts, files: &[FileData; 2], hunk: &[Change]) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    out.begin_output(o, files);

    out.set_color_context(ColorContext::LineNumber);
    util::print_number_range(out, b',', &files[0], r.first0, r.last0);
    out.putc(change_letter(changes));
    util::print_number_range(out, b',', &files[1], r.first1, r.last1);
    out.set_color_context(ColorContext::Reset);
    out.put(b"\n");

    if changes & OLD != 0 {
        let mut i = r.first0;
        while i <= r.last0 {
            out.set_color_context(ColorContext::Delete);
            let line = files[0].line(i);
            util::print_1_line_nl(out, o, Some(b"<"), line, true);
            out.set_color_context(ColorContext::Reset);
            if line.last() == Some(&b'\n') {
                out.put(b"\n");
            }
            i = i.saturating_add(1);
        }
    }
    if changes == CHANGED {
        out.put(b"---\n");
    }
    if changes & NEW != 0 {
        let mut i = r.first1;
        while i <= r.last1 {
            out.set_color_context(ColorContext::Add);
            let line = files[1].line(i);
            util::print_1_line_nl(out, o, Some(b">"), line, true);
            out.set_color_context(ColorContext::Reset);
            if line.last() == Some(&b'\n') {
                out.put(b"\n");
            }
            i = i.saturating_add(1);
        }
    }
}

/// `print_ed_script`: `-e`, the script in reverse so that each command's
/// line numbers are still right when it runs.
pub fn print_ed_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    util::print_script(script, util::find_change, o, |hunk| {
        print_ed_hunk(out, o, files, hunk);
    });
}

/// `print_ed_hunk`.
fn print_ed_hunk(out: &mut Out, o: &Opts, files: &[FileData; 2], hunk: &[Change]) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    out.begin_output(o, files);
    util::print_number_range(out, b',', &files[0], r.first0, r.last0);
    out.putc(change_letter(changes));
    out.put(b"\n");

    if changes != OLD {
        let mut insert_mode = true;
        let mut i = r.first1;
        while i <= r.last1 {
            if !insert_mode {
                out.put(b"a\n");
                insert_mode = true;
            }
            let line = files[1].line(i);
            if line == b".\n" {
                // A line that is just a dot would end the insert: write two
                // dots, end the insert, and take one away.
                out.put(b"..\n.\ns/.//\n");
                insert_mode = false;
            } else {
                util::print_1_line_nl(out, o, Some(b""), line, false);
            }
            i = i.saturating_add(1);
        }
        if insert_mode {
            out.put(b".\n");
        }
    }
}

/// `pr_forward_ed_script`: `-f`, the commands in file order.
pub fn pr_forward_ed_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    util::print_script(script, util::find_change, o, |hunk| {
        pr_forward_ed_hunk(out, o, files, hunk);
    });
}

/// `pr_forward_ed_hunk`.
fn pr_forward_ed_hunk(out: &mut Out, o: &Opts, files: &[FileData; 2], hunk: &[Change]) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    out.begin_output(o, files);
    out.putc(change_letter(changes));
    util::print_number_range(out, b' ', &files[0], r.first0, r.last0);
    out.put(b"\n");
    if changes == OLD {
        return;
    }
    let mut i = r.first1;
    while i <= r.last1 {
        util::print_1_line_nl(out, o, Some(b""), files[1].line(i), false);
        i = i.saturating_add(1);
    }
    out.put(b".\n");
}

/// `print_rcs_script`: `-n`, ed commands that count the lines they insert.
pub fn print_rcs_script(out: &mut Out, o: &Opts, files: &[FileData; 2], script: &[Change]) {
    util::print_script(script, util::find_change, o, |hunk| {
        print_rcs_hunk(out, o, files, hunk);
    });
}

/// `print_rcs_hunk`.
fn print_rcs_hunk(out: &mut Out, o: &Opts, files: &[FileData; 2], hunk: &[Change]) {
    let (changes, r) = util::analyze_hunk(hunk, files, o);
    if changes == 0 {
        return;
    }
    out.begin_output(o, files);
    let (tf0, tl0) = util::translate_range(&files[0], r.first0, r.last0);
    if changes & OLD != 0 {
        let n = if tf0 <= tl0 {
            tl0.saturating_sub(tf0).saturating_add(1)
        } else {
            1
        };
        out.put(format!("d{tf0} {n}\n").as_bytes());
    }
    if changes & NEW != 0 {
        let (tf1, tl1) = util::translate_range(&files[1], r.first1, r.last1);
        let n = if tf1 <= tl1 {
            tl1.saturating_sub(tf1).saturating_add(1)
        } else {
            1
        };
        out.put(format!("a{tl0} {n}\n").as_bytes());
        let mut i = r.first1;
        while i <= r.last1 {
            util::print_1_line_nl(out, o, Some(b""), files[1].line(i), false);
            i = i.saturating_add(1);
        }
    }
}
