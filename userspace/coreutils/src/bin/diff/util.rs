//! diffutils' `util.c`: the output stream and everything every format shares
//! -- messages, line printing, line numbers, hunk analysis, colours -- and
//! `lines_differ`, the line comparison the white-space options define.

use crate::analyze::Change;
use crate::io::{FileData, Lin, is_space};
use crate::{ColorsStyle, Opts, OutputStyle, WhiteSpace};
use coreutils::stdfd::{self, Stream};
use std::io::Write as _;

/// `enum changes`: what a hunk holds.
pub const UNCHANGED: u8 = 0;
pub const OLD: u8 = 1;
pub const NEW: u8 = 2;
pub const CHANGED: u8 = 3;

/// `change_letter`.
pub fn change_letter(changes: u8) -> u8 {
    match changes {
        OLD => b'd',
        NEW => b'a',
        CHANGED => b'c',
        _ => 0,
    }
}

/// `enum color_context`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorContext {
    Header,
    Add,
    Delete,
    Reset,
    LineNumber,
}

/// `color_indicator`'s eight entries: `lc rc ec rs hd ad de ln`.
#[derive(Clone, Debug)]
pub struct Palette {
    pub ind: [Option<Vec<u8>>; 8],
}

const C_LEFT: usize = 0;
const C_RIGHT: usize = 1;
const C_RESET: usize = 3;
const C_HEADER: usize = 4;
const C_ADD: usize = 5;
const C_DELETE: usize = 6;
const C_LINE: usize = 7;
const INDICATOR_NAME: [&[u8]; 8] = [b"lc", b"rc", b"ec", b"rs", b"hd", b"ad", b"de", b"ln"];

impl Default for Palette {
    fn default() -> Palette {
        Palette {
            ind: [
                Some(b"\x1b[".to_vec()),
                Some(b"m".to_vec()),
                None,
                Some(b"0".to_vec()),
                Some(b"1".to_vec()),
                Some(b"32".to_vec()),
                Some(b"31".to_vec()),
                Some(b"36".to_vec()),
            ],
        }
    }
}

/// Upstream's `PR_PROGRAM`, a constant of the build: where `-l` finds `pr`.
/// A macro as well as a constant so that the messages naming it can be
/// literals -- see [`Out::finish_output`].
macro_rules! pr_program {
    () => {
        "/usr/bin/pr"
    };
}
pub const PR_PROGRAM: &str = pr_program!();

/// A pipe to `pr`, for `-l`.
struct Pr {
    child: std::process::Child,
}

/// The output side of a run: `outfile`, the queue of messages `-l` defers,
/// and the colour state.
pub struct Out {
    stdout: Stream,
    pr: Option<Pr>,
    /// `outfile != 0`: output has begun for the current comparison.
    pub begun: bool,
    msg_queue: Vec<Vec<u8>>,
    current_names: [Vec<u8>; 2],
    currently_recursive: bool,
    colors_enabled: bool,
    last_context: ColorContext,
    palette: Palette,
    palette_spec: Option<Vec<u8>>,
}

impl Out {
    pub fn new(palette_spec: Option<Vec<u8>>) -> Out {
        Out {
            stdout: Stream::stdout(),
            pr: None,
            begun: false,
            msg_queue: Vec::new(),
            current_names: [Vec::new(), Vec::new()],
            currently_recursive: false,
            colors_enabled: false,
            last_context: ColorContext::Reset,
            palette: Palette::default(),
            palette_spec,
        }
    }

    /// Write to `outfile`.
    pub fn put(&mut self, bytes: &[u8]) {
        if let Some(pr) = self.pr.as_mut() {
            if let Some(stdin) = pr.child.stdin.as_mut() {
                // A failed write is noticed when the pipe is closed, as
                // upstream's `ferror (outfile)` notices it.
                let _ = stdin.write_all(bytes);
            }
        } else {
            let _ = self.stdout.write_all(bytes);
        }
    }

    /// Write to standard output, whatever `outfile` is.
    pub fn put_stdout(&mut self, bytes: &[u8]) {
        let _ = self.stdout.write_all(bytes);
    }

    /// Whether `outfile` is standard output: it is, except under `-l`.
    pub fn outfile_is_stdout(&self) -> bool {
        self.pr.is_none()
    }

    pub fn putc(&mut self, b: u8) {
        self.put(&[b]);
    }

    /// `message`: a line about the comparison rather than of it, printed on
    /// standard output now -- or at the end, under `-l`.
    pub fn message(&mut self, o: &Opts, text: &[u8]) {
        if o.paginate {
            self.msg_queue.push(text.to_vec());
        } else {
            if o.sdiff_merge_assist {
                self.put_stdout(b" ");
            }
            self.put_stdout(text);
        }
    }

    /// `print_message_queue`.
    pub fn print_message_queue(&mut self) {
        for m in std::mem::take(&mut self.msg_queue) {
            self.put_stdout(&m);
        }
    }

    /// Flush standard output, as `fflush (stdout)` does.
    pub fn flush(&mut self) {
        let _ = self.stdout.flush();
    }

    /// The failure standard output has recorded, if any: `ferror (stdout)`.
    pub fn stdout_failure(&self) -> Option<std::io::Error> {
        if self.stdout.errored() {
            self.stdout.error()
        } else {
            None
        }
    }

    /// The stream, handed back for `close_stdout`.
    pub fn into_stdout(self) -> Stream {
        self.stdout
    }

    /// `setup_output`.
    pub fn setup_output(&mut self, name0: &[u8], name1: &[u8], recursive: bool) {
        self.current_names = [name0.to_vec(), name1.to_vec()];
        self.currently_recursive = recursive;
        self.begun = false;
    }

    /// `check_color_output`.
    fn check_color_output(&mut self, o: &Opts, is_pipe: bool) {
        if o.colors_style == ColorsStyle::Never {
            return;
        }
        let output_is_tty = o.presume_output_tty || (!is_pipe && stdfd::is_tty(1));
        self.colors_enabled = o.colors_style == ColorsStyle::Always
            || (o.colors_style == ColorsStyle::Auto && output_is_tty);
        if self.colors_enabled {
            self.parse_diff_color();
        }
    }

    /// `parse_diff_color`: `--palette`, in `dircolors`' notation.
    fn parse_diff_color(&mut self) {
        let Some(spec) = self.palette_spec.clone() else {
            return;
        };
        if spec.is_empty() {
            return;
        }
        let mut p = 0usize;
        let failed = loop {
            match spec.get(p) {
                None => break false,
                Some(b':') => p = p.saturating_add(1),
                Some(b'*') => {
                    // An extension: parsed and kept by upstream, never used.
                    p = p.saturating_add(1);
                    let Some((_, used)) =
                        coreutils::ls::get_funky_string(spec.get(p..).unwrap_or_default(), true)
                    else {
                        break true;
                    };
                    p = p.saturating_add(used);
                    if spec.get(p) != Some(&b'=') {
                        break true;
                    }
                    p = p.saturating_add(1);
                    let Some((_, used)) =
                        coreutils::ls::get_funky_string(spec.get(p..).unwrap_or_default(), false)
                    else {
                        break true;
                    };
                    p = p.saturating_add(used);
                }
                Some(&a) => {
                    let Some(&b) = spec.get(p.saturating_add(1)) else {
                        break true;
                    };
                    let label = [a, b];
                    p = p.saturating_add(2);
                    if spec.get(p) != Some(&b'=') {
                        break true;
                    }
                    p = p.saturating_add(1);
                    let Some(ind) = INDICATOR_NAME.iter().position(|n| *n == label) else {
                        stdfd::diag_bytes(
                            &[b"diff: unrecognized prefix: ".as_slice(), &label, b"\n"].concat(),
                        );
                        break true;
                    };
                    let Some((value, used)) =
                        coreutils::ls::get_funky_string(spec.get(p..).unwrap_or_default(), false)
                    else {
                        break true;
                    };
                    if let Some(slot) = self.palette.ind.get_mut(ind) {
                        *slot = Some(value);
                    }
                    p = p.saturating_add(used);
                }
            }
        };
        if failed {
            stdfd::diag_line("diff: unparsable value for --palette");
            self.colors_enabled = false;
        }
    }

    /// `set_color_context`.
    pub fn set_color_context(&mut self, ctx: ColorContext) {
        if self.colors_enabled && self.last_context != ctx {
            let which = match ctx {
                ColorContext::Header => C_HEADER,
                ColorContext::LineNumber => C_LINE,
                ColorContext::Add => C_ADD,
                ColorContext::Delete => C_DELETE,
                ColorContext::Reset => C_RESET,
            };
            for i in [C_LEFT, which, C_RIGHT] {
                let s = self
                    .palette
                    .ind
                    .get(i)
                    .cloned()
                    .flatten()
                    .unwrap_or_default();
                self.put(&s);
            }
            self.last_context = ctx;
        }
    }

    /// `begin_output`: on the first output of a comparison, start the pipe to
    /// `pr` or announce the files under `-r`, and print the context header.
    pub fn begin_output(&mut self, o: &Opts, files: &[FileData; 2]) {
        if self.begun {
            return;
        }
        self.begun = true;
        let names = [
            c_escape(&self.current_names[0]),
            c_escape(&self.current_names[1]),
        ];
        let mut name = b"diff".to_vec();
        name.extend_from_slice(&o.switch_string);
        name.push(b' ');
        name.extend_from_slice(&names[0]);
        name.push(b' ');
        name.extend_from_slice(&names[1]);

        if o.paginate {
            self.flush();
            match spawn_pr(&name) {
                Ok(child) => {
                    self.pr = Some(Pr { child });
                    self.check_color_output(o, true);
                }
                Err(e) => {
                    self.print_message_queue();
                    stdfd::diag_line(&format!("diff: fork: {}", coreutils::errmsg::strerror(&e)));
                    crate::exit_trouble(self);
                }
            }
        } else {
            self.check_color_output(o, false);
            if self.currently_recursive {
                self.put_stdout(&name);
                self.put_stdout(b"\n");
            }
        }

        match o.output_style {
            OutputStyle::Context => {
                crate::context::print_context_header(self, o, files, &names, false);
            }
            OutputStyle::Unified => {
                crate::context::print_context_header(self, o, files, &names, true);
            }
            _ => {}
        }
    }

    /// `finish_output`: close the pipe to `pr` and wait for it.
    pub fn finish_output(&mut self) {
        if let Some(mut pr) = self.pr.take() {
            drop(pr.child.stdin.take());
            let status = pr.child.wait();
            let code = match status {
                Ok(s) => s.code().unwrap_or(i32::MAX),
                Err(_) => i32::MAX,
            };
            if code != 0 {
                // Upstream's apostrophes, around a path fixed at build time:
                // literals, because nothing here was read from anywhere.
                let msg = match code {
                    126 => concat!(
                        "subsidiary program '",
                        pr_program!(),
                        "' could not be invoked"
                    )
                    .to_owned(),
                    127 => concat!("subsidiary program '", pr_program!(), "' not found").to_owned(),
                    i32::MAX => {
                        concat!("subsidiary program '", pr_program!(), "' failed").to_owned()
                    }
                    n => format!(
                        concat!(
                            "subsidiary program '",
                            pr_program!(),
                            "' failed (exit status {})"
                        ),
                        n
                    ),
                };
                stdfd::diag_line(&format!("diff: {msg}"));
                crate::exit_trouble(self);
            }
        }
        self.begun = false;
    }
}

fn spawn_pr(name: &[u8]) -> std::io::Result<std::process::Child> {
    std::process::Command::new(PR_PROGRAM)
        .arg("-h")
        .arg(coreutils::quote::os_from_bytes(name))
        .stdin(std::process::Stdio::piped())
        .spawn()
}

/// `c_escape`: a file name for the `diff ... A B` line, in double quotes with
/// C escapes when it holds a space or a control character.
pub fn c_escape(s: &[u8]) -> Vec<u8> {
    fn esc(c: u8) -> u8 {
        match c {
            0x07 => b'a',
            0x08 => b'b',
            b'\t' => b't',
            b'\n' => b'n',
            0x0b => b'v',
            0x0c => b'f',
            b'\r' => b'r',
            b'"' => b'"',
            b'\\' => b'\\',
            // `c < 32` on a signed char: true for controls and for every
            // byte above 0x7f.
            _ => u8::from(c < 32 || c >= 0x80),
        }
    }
    let must_quote = s.contains(&b' ');
    if !must_quote && s.iter().all(|&c| esc(c) == 0) {
        return s.to_vec();
    }
    let mut out = vec![b'"'];
    for &c in s {
        match esc(c) {
            0 => out.push(c),
            1 => {
                out.push(b'\\');
                out.push(((c >> 6) & 3).saturating_add(b'0'));
                out.push(((c >> 3) & 7).saturating_add(b'0'));
                out.push((c & 7).saturating_add(b'0'));
            }
            e => {
                out.push(b'\\');
                out.push(e);
            }
        }
    }
    out.push(b'"');
    out
}

/// `lines_differ`: whether two newline-terminated lines differ under the
/// white-space and case rules.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn lines_differ(s1: &[u8], s2: &[u8], o: &Opts) -> bool {
    // Each line ends in a newline, which both loops stop at; reading past it
    // cannot happen, but a missing one reads as a newline rather than panics.
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(b'\n');
    let mut t1 = 0usize;
    let mut t2 = 0usize;
    let mut column = 0usize;
    let tabsize = o.tabsize.max(1);
    loop {
        let mut c1 = at(s1, t1);
        t1 += 1;
        let mut c2 = at(s2, t2);
        t2 += 1;

        if c1 != c2 {
            match o.ignore_white_space {
                WhiteSpace::AllSpace => {
                    while is_space(c1) && c1 != b'\n' {
                        c1 = at(s1, t1);
                        t1 += 1;
                    }
                    while is_space(c2) && c2 != b'\n' {
                        c2 = at(s2, t2);
                        t2 += 1;
                    }
                }
                WhiteSpace::SpaceChange => {
                    if is_space(c1) {
                        while c1 != b'\n' {
                            c1 = at(s1, t1);
                            t1 += 1;
                            if !is_space(c1) {
                                t1 -= 1;
                                c1 = b' ';
                                break;
                            }
                        }
                    }
                    if is_space(c2) {
                        while c2 != b'\n' {
                            c2 = at(s2, t2);
                            t2 += 1;
                            if !is_space(c2) {
                                t2 -= 1;
                                c2 = b' ';
                                break;
                            }
                        }
                    }
                    if c1 != c2 {
                        // Went too far on the simple test: back up to the
                        // first non-blank on the side that ran ahead.
                        if c2 == b' ' && c1 != b'\n' && 1 < t1 && is_space(at(s1, t1 - 2)) {
                            t1 -= 1;
                            continue;
                        }
                        if c1 == b' ' && c2 != b'\n' && 1 < t2 && is_space(at(s2, t2 - 2)) {
                            t2 -= 1;
                            continue;
                        }
                    }
                }
                WhiteSpace::TrailingSpace | WhiteSpace::TabExpansionAndTrailingSpace => {
                    let mut handled = false;
                    if is_space(c1) && is_space(c2) {
                        let rest_blank = |s: &[u8], mut p: usize| loop {
                            let c = at(s, p);
                            if c == b'\n' {
                                break true;
                            }
                            if !is_space(c) {
                                break false;
                            }
                            p += 1;
                        };
                        let blank1 = c1 == b'\n' || rest_blank(s1, t1);
                        if blank1 {
                            let blank2 = c2 == b'\n' || rest_blank(s2, t2);
                            if blank2 {
                                // Both lines have nothing but white space left.
                                return false;
                            }
                        }
                        handled = true;
                    }
                    if !handled && o.ignore_white_space == WhiteSpace::TabExpansionAndTrailingSpace
                    {
                        if let Some(differ) = tab_expansion(
                            &mut c1,
                            &mut c2,
                            s1,
                            s2,
                            &mut t1,
                            &mut t2,
                            &mut column,
                            tabsize,
                        ) {
                            return differ;
                        }
                    }
                }
                WhiteSpace::TabExpansion => {
                    if let Some(differ) = tab_expansion(
                        &mut c1,
                        &mut c2,
                        s1,
                        s2,
                        &mut t1,
                        &mut t2,
                        &mut column,
                        tabsize,
                    ) {
                        return differ;
                    }
                }
                WhiteSpace::None => {}
            }

            if o.ignore_case {
                c1 = c1.to_ascii_lowercase();
                c2 = c2.to_ascii_lowercase();
            }
            if c1 != c2 {
                break;
            }
        }
        if c1 == b'\n' {
            return false;
        }
        column += if c1 == b'\t' {
            tabsize - column % tabsize
        } else {
            1
        };
    }
    true
}

/// `lines_differ`'s `IGNORE_TAB_EXPANSION` arm: a run of spaces and tabs on
/// one side against a run on the other is equal when both end at one column.
/// `Some(true)` when they do not.
#[allow(clippy::too_many_arguments, clippy::arithmetic_side_effects)]
fn tab_expansion(
    c1: &mut u8,
    c2: &mut u8,
    s1: &[u8],
    s2: &[u8],
    t1: &mut usize,
    t2: &mut usize,
    column: &mut usize,
    tabsize: usize,
) -> Option<bool> {
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(b'\n');
    if (*c1 == b' ' && *c2 == b'\t') || (*c1 == b'\t' && *c2 == b' ') {
        let mut column2 = *column;
        loop {
            if *c1 == b' ' {
                *column += 1;
            } else if *c1 == b'\t' {
                *column += tabsize - *column % tabsize;
            } else {
                break;
            }
            *c1 = at(s1, *t1);
            *t1 += 1;
        }
        loop {
            if *c2 == b' ' {
                column2 += 1;
            } else if *c2 == b'\t' {
                column2 += tabsize - column2 % tabsize;
            } else {
                break;
            }
            *c2 = at(s2, *t2);
            *t2 += 1;
        }
        if *column != column2 {
            return Some(true);
        }
    }
    None
}

/// `translate_line_number`: an internal line number as the file's own,
/// counting from 1.
pub fn translate_line_number(file: &FileData, i: Lin) -> Lin {
    i.saturating_add(file.prefix_lines).saturating_add(1)
}

/// `translate_range`.
pub fn translate_range(file: &FileData, a: Lin, b: Lin) -> (Lin, Lin) {
    (
        translate_line_number(file, a.saturating_sub(1)).saturating_add(1),
        translate_line_number(file, b.saturating_add(1)).saturating_sub(1),
    )
}

/// `print_number_range`: `A,B`, or one number when the range has one line or
/// none.
pub fn print_number_range(out: &mut Out, sepchar: u8, file: &FileData, a: Lin, b: Lin) {
    let (ta, tb) = translate_range(file, a, b);
    if tb > ta {
        out.put(format!("{ta}{}{tb}", char::from(sepchar)).as_bytes());
    } else {
        out.put(tb.to_string().as_bytes());
    }
}

/// The first and last line numbers a hunk touches in each file.
#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub first0: Lin,
    pub last0: Lin,
    pub first1: Lin,
    pub last1: Lin,
}

/// `analyze_hunk`: the hunk's ranges, and whether it deletes, inserts, both,
/// or -- every line being ignorable under `-B`/`-I` -- neither.
pub fn analyze_hunk(hunk: &[Change], files: &[FileData; 2], o: &Opts) -> (u8, Range) {
    let mut trivial = o.ignore_blank_lines || o.ignore_regexp.is_some();
    // 0: ignore empty lines; usize::MAX: length does not make a line trivial.
    let trivial_length: usize = if o.ignore_blank_lines { 0 } else { usize::MAX };
    let skip_white_space =
        o.ignore_blank_lines && WhiteSpace::TrailingSpace <= o.ignore_white_space;
    let skip_leading_white_space =
        skip_white_space && WhiteSpace::SpaceChange <= o.ignore_white_space;

    let mut show_from: Lin = 0;
    let mut show_to: Lin = 0;
    let first = hunk.first().copied().unwrap_or(Change {
        line0: 0,
        line1: 0,
        deleted: 0,
        inserted: 0,
        ignore: false,
    });
    let mut l0 = first.line0;
    let mut l1 = first.line1;

    for next in hunk {
        l0 = next.line0.saturating_add(next.deleted).saturating_sub(1);
        l1 = next.line1.saturating_add(next.inserted).saturating_sub(1);
        show_from = show_from.saturating_add(next.deleted);
        show_to = show_to.saturating_add(next.inserted);

        for (file, from, to) in [(&files[0], next.line0, l0), (&files[1], next.line1, l1)] {
            let mut i = from;
            while i <= to && trivial {
                let line = file.line(i);
                // The text before the newline, which a last line may lack.
                let text = line.strip_suffix(b"\n").unwrap_or(line);
                let mut p = 0usize;
                if skip_white_space {
                    for (k, &c) in text.iter().enumerate() {
                        if !is_space(c) {
                            p = if skip_leading_white_space { k } else { 0 };
                            break;
                        }
                        p = k.saturating_add(1);
                    }
                }
                let len_after = text.len().saturating_sub(p);
                if len_after != trivial_length
                    && o.ignore_regexp
                        .as_ref()
                        .is_none_or(|re| !crate::regex_matches(re, text))
                {
                    trivial = false;
                }
                i = i.saturating_add(1);
            }
        }
    }

    let range = Range {
        first0: first.line0,
        last0: l0,
        first1: first.line1,
        last1: l1,
    };
    if trivial {
        return (UNCHANGED, range);
    }
    let mut changes = UNCHANGED;
    if show_from != 0 {
        changes |= OLD;
    }
    if show_to != 0 {
        changes |= NEW;
    }
    (changes, range)
}

/// `print_1_line_nl`: a line flagged with `line_flag`, its newline printed
/// unless `skip_nl`, and a `\ No newline at end of file` after it when it
/// has none.
pub fn print_1_line_nl(
    out: &mut Out,
    o: &Opts,
    line_flag: Option<&[u8]>,
    line: &[u8],
    skip_nl: bool,
) {
    let mut flag_format: Option<(&[u8], &[u8])> = None;
    if let Some(flag) = line_flag.filter(|f| !f.is_empty()) {
        let sep: &[u8] = if o.initial_tab { b"\t" } else { b" " };
        flag_format = Some((flag, sep));
        if o.suppress_blank_empty && line.first() == Some(&b'\n') {
            // A single blank flag disappears; any other prints without its
            // separator.
            let shown = if flag == b" " {
                flag.get(1..).unwrap_or_default()
            } else {
                flag
            };
            out.put(shown);
        } else {
            out.put(flag);
            out.put(sep);
        }
    }
    let ends_nl = line.last() == Some(&b'\n');
    let limit = if skip_nl && ends_nl {
        line.len().saturating_sub(1)
    } else {
        line.len()
    };
    output_1_line(out, o, line.get(..limit).unwrap_or_default(), flag_format);
    if line_flag.is_none_or(|f| !f.is_empty()) && !ends_nl {
        out.set_color_context(ColorContext::Reset);
        out.put(b"\n\\ No newline at end of file\n");
    }
}

/// `output_1_line`: the text of a line; with `-t`, tabs expanded and the flag
/// repeated after each carriage return so the stops keep lining up.
pub fn output_1_line(out: &mut Out, o: &Opts, text: &[u8], flag_format: Option<(&[u8], &[u8])>) {
    if !o.expand_tabs {
        out.put(text);
        return;
    }
    let tab_size = o.tabsize.max(1);
    let mut column = 0usize;
    let mut buf = Vec::with_capacity(text.len());
    let mut t = 0usize;
    while let Some(&c) = text.get(t) {
        t = t.saturating_add(1);
        match c {
            b'\t' => {
                let spaces = tab_size.saturating_sub(column.checked_rem(tab_size).unwrap_or(0));
                column = column.saturating_add(spaces);
                buf.extend(std::iter::repeat_n(b' ', spaces));
            }
            b'\r' => {
                buf.push(c);
                if let Some((flag, sep)) = flag_format
                    && t < text.len()
                    && text.get(t) != Some(&b'\n')
                {
                    buf.extend_from_slice(flag);
                    buf.extend_from_slice(sep);
                }
                column = 0;
            }
            0x08 => {
                if column == 0 {
                    continue;
                }
                column = column.saturating_sub(1);
                buf.push(c);
            }
            _ => {
                // `isprint` in C.UTF-8: ASCII's printable characters only.
                column = column.saturating_add(usize::from((0x20..0x7f).contains(&c)));
                buf.push(c);
            }
        }
    }
    out.put(&buf);
}

/// `print_script`: cut the script into hunks with `hunkfun` and print each
/// with `printfun`.
pub fn print_script(
    script: &[Change],
    hunkfun: fn(&[Change], &Opts) -> usize,
    o: &Opts,
    mut printfun: impl FnMut(&[Change]),
) {
    let mut start = 0usize;
    while start < script.len() {
        let rest = script.get(start..).unwrap_or_default();
        let len = hunkfun(rest, o).max(1).min(rest.len());
        printfun(rest.get(..len).unwrap_or_default());
        start = start.saturating_add(len);
    }
}

/// `find_change` and `find_reverse_change`: every change is a hunk of its own.
pub fn find_change(_script: &[Change], _o: &Opts) -> usize {
    1
}
