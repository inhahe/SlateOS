//! `tabs`: ncurses 6.4's (`progs/tabs.c`, 20240113), ported.
//!
//! Clears the terminal's hardware tab stops and sets new ones: every eighth
//! column by default, every `n`th for `-n`, one of the canned lists (`-a`,
//! `-c`, `-f`, `-p`, `-s`, `-u` and their variants) or a list of columns,
//! `1,11,21` or `1,+10,+10`. `+m` sets a left margin first, where the
//! terminal can; `-d` draws a ruler under the stops, `-n` sets nothing.
//! The terminal's description is `terminfo`'s; the shared machinery is
//! [`coreutils::ncurses`].
//!
//! The options are upstream's own hand-made walk, not `getopt`'s, and its
//! quirks are kept, measured against Ubuntu 24.04's `tabs`:
//!
//! - `-T` takes the next word even when its value is attached, so
//!   `-Tvt100 4` loses the `4` and sets the default, every eighth column.
//! - Stops are checked for order only where a comma follows one: `5,1` is
//!   taken, `5,1,2` refused.
//! - `-a` and `-c` read the letter after them as their variant, and give it
//!   back when it is none of theirs.
//! - Operands gather into one list, and a canned option replaces it; an
//!   operand after a canned option starts the gathering afresh -- unless it
//!   begins with `+`, which is added to what the earlier operands gathered:
//!   `tabs 1 -a 9` is `9`, `tabs 1 -a +5` is `1,+5`.
//!
//! Deliberately different: `-V` names SlateOS's coreutils rather than the
//! ncurses version, and a name echoed in a complaint has its unprintable
//! bytes escaped (design-decisions §370).

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::ncurses::{self as nc, Legacy, PutChar, TtySettings, cap};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd;
use terminfo::{Entry, Padding, Tparm};

coreutils::guard_std_fds!();

/// `usage`'s text, all of it: the program's name is spelled out.
const USAGE: &[u8] = b"Usage: tabs [options] [tabstop-list]\n\
\n\
Options:\n\
\x20 -0       reset tabs\n\
\x20 -8       set tabs to standard interval\n\
\x20 -a       Assembler, IBM S/370, first format\n\
\x20 -a2      Assembler, IBM S/370, second format\n\
\x20 -c       COBOL, normal format\n\
\x20 -c2      COBOL compact format\n\
\x20 -c3      COBOL compact format extended\n\
\x20 -d       debug (show ruler with expected/actual tab positions)\n\
\x20 -f       FORTRAN\n\
\x20 -n       no-op (do not modify terminal settings)\n\
\x20 -p       PL/I\n\
\x20 -s       SNOBOL\n\
\x20 -u       UNIVAC 1100 Assembler\n\
\x20 -T name  use terminal type 'name'\n\
\x20 -V       print version\n\
\n\
A tabstop-list is an ordered list of column numbers, e.g., 1,11,21\n\
or 1,+10,+10 which is the same.\n";

/// What ends the program early, its complaint already written: the status
/// to exit with.
struct Exit(u8);

/// `usage`: the text on standard error -- standard output flushed first --
/// and status 1.
fn usage(out: &mut ulclosestream::Stdout) -> Exit {
    out.flush();
    ulclosestream::stderr_write(USAGE);
    Exit(1)
}

/// C's `isspace` in the C locale.
fn isspace(c: u8) -> bool {
    cstrtol::isspace(c)
}

/// `skip_list`: past the digits, blanks, `+` and `,` a list is made of.
fn skip_list(value: &[u8]) -> usize {
    value
        .iter()
        .position(|&c| !(c.is_ascii_digit() || isspace(c) || c == b'+' || c == b','))
        .unwrap_or(value.len())
}

/// `trimmed_tab_list`: blanks gone from the ends, a blank after a digit or a
/// comma made a comma, and a run of commas one -- written only before what
/// follows it, so none trails.
fn trimmed_tab_list(source: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(source.len());
    let mut last: u8 = 0;
    for &c in source {
        let mut ch = c;
        if isspace(ch) {
            if last == 0 {
                continue;
            } else if last.is_ascii_digit() || last == b',' {
                ch = b',';
            }
        } else if ch == b',' {
            // Held: written before what comes next, if anything does.
        } else {
            if last == b',' {
                result.push(last);
            }
            result.push(ch);
        }
        last = ch;
    }
    result
}

/// `comma_is_needed`: a list that does not already end in one.
fn comma_is_needed(source: Option<&[u8]>) -> bool {
    source.is_some_and(|s| s.last().is_some_and(|&c| c != b','))
}

/// `add_to_tab_list (&append, value)`: `value`, trimmed, added to what was
/// gathered -- a comma between where neither has one. The answer is what
/// is gathered afterwards, which for a `value` with nothing in it is what
/// was gathered before (perhaps nothing).
fn add_to_tab_list(append: &mut Option<Vec<u8>>, value: &[u8]) -> Option<Vec<u8>> {
    let copied = trimmed_tab_list(value);
    if !copied.is_empty() {
        let comma: &[u8] = if copied.first() == Some(&b',') || !comma_is_needed(append.as_deref()) {
            b""
        } else {
            b","
        };
        let mut result = append.take().unwrap_or_default();
        result.extend_from_slice(comma);
        result.extend_from_slice(&copied);
        *append = Some(result);
    }
    append.clone()
}

/// What the program was told.
#[derive(Default)]
struct Options {
    debug: bool,
    no_op: bool,
    /// `term_name`: `None` once `-T` ended the command line with no name.
    term_name: Option<Vec<u8>>,
    /// `tab_list`.
    tab_list: Option<Vec<u8>>,
    /// Whether `tab_list` is `append`, gathered from the operands, rather
    /// than a canned list.
    tab_list_is_append: bool,
    /// `append`.
    append: Option<Vec<u8>>,
    /// `+m`: -1 for none.
    margin: i32,
}

impl Options {
    /// A canned list, or one given as an option.
    fn set_list(&mut self, list: &[u8]) {
        self.tab_list = Some(list.to_vec());
        self.tab_list_is_append = false;
    }

    /// `tab_list = add_to_tab_list (&append, value)`.
    fn add(&mut self, value: &[u8]) {
        self.tab_list = add_to_tab_list(&mut self.append, value);
        self.tab_list_is_append = true;
    }
}

/// `main`'s walk over the arguments.
fn parse(
    argv: &[Vec<u8>],
    progname: &[u8],
    o: &mut Options,
    out: &mut ulclosestream::Stdout,
) -> Result<Option<u8>, Exit> {
    let mut n = 1usize;
    while let Some(word) = argv.get(n) {
        match word.first() {
            Some(b'-') => {
                let mut i = 0usize;
                loop {
                    i = i.saturating_add(1);
                    let Some(&ch) = word.get(i) else { break };
                    match ch {
                        b'a' => {
                            i = i.saturating_add(1);
                            if word.get(i) == Some(&b'2') {
                                o.set_list(b"1,10,16,40,72");
                            } else {
                                // Given back, to be read as an option.
                                o.set_list(b"1,10,16,36,72");
                                i = i.saturating_sub(1);
                            }
                        }
                        b'c' => {
                            i = i.saturating_add(1);
                            match word.get(i) {
                                Some(b'2') => o.set_list(b"1,6,10,14,49"),
                                Some(b'3') => {
                                    o.set_list(b"1,6,10,14,18,22,26,30,34,38,42,46,50,54,58,62,67");
                                }
                                _ => {
                                    o.set_list(b"1,8,12,16,20,55");
                                    i = i.saturating_sub(1);
                                }
                            }
                        }
                        b'd' => o.debug = true,
                        b'f' => o.set_list(b"1,7,11,15,19,23"),
                        b'n' => o.no_op = true,
                        b'p' => o.set_list(b"1,5,9,13,17,21,25,29,33,37,41,45,49,53,57,61"),
                        b's' => o.set_list(b"1,10,55"),
                        b'u' => o.set_list(b"1,12,20,44"),
                        b'T' => {
                            // The next word is taken whether or not the name
                            // is attached to this one.
                            n = n.saturating_add(1);
                            let attached = word.get(i.saturating_add(1)..).unwrap_or_default();
                            o.term_name = if attached.is_empty() {
                                argv.get(n).cloned()
                            } else {
                                Some(attached.to_vec())
                            };
                            break;
                        }
                        b'V' => {
                            out.write(&nc::version_line(progname));
                            return Ok(Some(0));
                        }
                        c if c.is_ascii_digit() => {
                            let rest = word.get(i..).unwrap_or_default();
                            let len = skip_list(rest);
                            o.set_list(rest.get(..len).unwrap_or_default());
                            i = i.saturating_add(len).saturating_sub(1);
                        }
                        _ => return Err(usage(out)),
                    }
                }
            }
            Some(b'+') => match word.get(1) {
                Some(b'm') => {
                    let digits = word.get(2..).unwrap_or_default();
                    if digits.iter().any(|c| !c.is_ascii_digit()) {
                        return Err(usage(out));
                    }
                    o.margin = if digits.is_empty() {
                        10
                    } else {
                        digits.iter().fold(0i32, |n, &d| {
                            n.wrapping_mul(10)
                                .wrapping_add(i32::from(d.wrapping_sub(b'0')))
                        })
                    };
                }
                // "special case of relative stops separated by spaces?"
                Some(_) => o.add(word),
                None => {}
            },
            _ => {
                // A canned list chosen since: what was gathered goes.
                if o.append.is_some() && !o.tab_list_is_append {
                    o.append = None;
                }
                o.add(word);
            }
        }
        n = n.saturating_add(1);
    }
    Ok(None)
}

/// The program's state once it has set up.
struct Tabs<'o> {
    out: PutChar<'o>,
    progname: Vec<u8>,
    /// The terminal, as `tigetstr` reads it, for `tparm`'s checks.
    entry: Entry,
    /// Its legacy copy, as the macros read it.
    strings: Entry,
    tparm: Tparm,
    padding: Padding,
    /// `max_cols`.
    max_cols: i32,
}

impl Tabs<'_> {
    /// `putchar (c)`.
    fn putch(&mut self, c: u8) {
        self.out.0.write(&[c]);
    }

    /// `printf`'s and `fputs`'s bytes.
    fn print(&mut self, bytes: &[u8]) {
        self.out.0.write(bytes);
    }

    /// `tputs (s, 1, putch)`, for a string there is.
    fn tputs(&mut self, s: Option<&[u8]>) {
        if let Some(s) = s {
            let padding = self.padding;
            terminfo::tputs_to(s, 1, &padding, &mut self.out);
        }
    }

    /// The macro `name`: the legacy copy's.
    fn cap(&self, name: &str) -> Option<Vec<u8>> {
        cap(&self.strings, name).map(<[u8]>::to_vec)
    }

    /// `TIPARM_n (s, ...)`.
    fn tiparm(&mut self, expected: usize, s: &[u8], params: &[i32]) -> Option<Vec<u8>> {
        self.tparm.nc_tiparm(&self.entry, expected, s, params)
    }

    /// `ansi_clear_tabs`: whether `tbc` is the ANSI one, `CSI 3 g`, which
    /// needs no move to the left margin first.
    fn ansi_clear_tabs(&self) -> bool {
        self.cap("tbc").is_some_and(|tbc| {
            let param = if tbc.first() == Some(&0x9b) {
                tbc.get(1..)
            } else {
                tbc.strip_prefix(b"\x1b[")
            };
            param.unwrap_or(&tbc) == b"3g"
        })
    }

    /// `do_tabs (list)`.
    fn do_tabs(&mut self, list: &[i32]) {
        let mut last: i32 = 1;
        let mut first = true;
        let hts = self.cap("hts");
        for &stop in list.iter().take_while(|&&s| s > 0) {
            if first {
                first = false;
                self.putch(b'\r');
            }
            if last < stop {
                // `while (last++ < stop)`: compared, then counted.
                loop {
                    let before = last;
                    last = last.saturating_add(1);
                    if before >= stop || last > self.max_cols {
                        break;
                    }
                    self.putch(b' ');
                }
            }
            if stop <= self.max_cols {
                self.tputs(hts.as_deref());
                last = stop;
            } else {
                break;
            }
        }
        self.putch(b'\r');
    }

    /// `decode_tabs (tab_list, margin)`: the stops, each moved right by a
    /// margin the terminal could not set; `None` for stops that do not
    /// increase, after saying so.
    fn decode_tabs(&self, tab_list: &[u8], margin: i32) -> Option<Vec<i32>> {
        let margin = margin.max(0);
        let mut result: Vec<i32> = Vec::new();
        let mut value: i32 = 0;
        let mut prior: i32 = 0;
        for &ch in tab_list {
            if ch.is_ascii_digit() {
                value = value
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(ch.wrapping_sub(b'0')));
                if value > self.max_cols {
                    value = self.max_cols;
                }
            } else if ch == b',' {
                let stop = value.wrapping_add(prior).wrapping_add(margin);
                if let Some(&before) = result.last()
                    && stop <= before
                {
                    let mut m = escape_unprintable(&self.progname).into_bytes();
                    m.extend_from_slice(
                        format!(": tab-stops are not in increasing order: {value} {before}\n")
                            .as_bytes(),
                    );
                    ulclosestream::stderr_write(&m);
                    return None;
                }
                result.push(stop);
                value = 0;
                prior = 0;
            } else if ch == b'+'
                && let Some(&before) = result.last()
            {
                prior = before;
            }
        }
        // "If there is only one value, then it is an option such as "-8"."
        if result.is_empty() && value > 0 {
            let step = value;
            value = 1;
            while i32::try_from(result.len()).unwrap_or(i32::MAX) < self.max_cols.saturating_sub(1)
            {
                result.push(value.wrapping_add(margin));
                value = value.wrapping_add(step);
            }
        }
        // "Add the last value, if any."
        result.push(value.wrapping_add(prior).wrapping_add(margin));
        Some(result)
    }

    /// `print_ruler (list, new_line)`: a ruler, then a `*` at each stop.
    fn print_ruler(&mut self, list: &[i32], new_line: &[u8]) {
        let max_cols = self.max_cols;
        let mut n = 0i32;
        while n < max_cols {
            let ch = n.checked_div(10).unwrap_or(0).saturating_add(1);
            let mark = if ch < 10 {
                nc::low_byte(ch.wrapping_add(i32::from(b'0')))
            } else {
                nc::low_byte(ch.wrapping_add(i32::from(b'A')).wrapping_sub(10))
            };
            let mut buffer = b"----+----".to_vec();
            buffer.push(mark);
            let shown = usize::try_from(max_cols.saturating_sub(n).min(10)).unwrap_or(0);
            self.print(buffer.get(..shown).unwrap_or(&buffer));
            n = n.saturating_add(10);
        }
        self.print(new_line);

        let mut last = 0i32;
        for &stop in list {
            if stop <= 0 || last >= max_cols {
                break;
            }
            loop {
                last = last.saturating_add(1);
                if last >= stop {
                    break;
                }
                if last <= max_cols {
                    self.putch(b'-');
                } else {
                    break;
                }
            }
            if last <= max_cols {
                self.putch(b'*');
                last = stop;
            } else {
                break;
            }
        }
        loop {
            last = last.saturating_add(1);
            if last > max_cols {
                break;
            }
            self.putch(b'-');
        }
        self.print(new_line);
    }

    /// `write_tabs (list, new_line)`: a `*` at each stop by tabbing to it,
    /// and a `+` one tab past them.
    fn write_tabs(&mut self, list: &[i32], new_line: &[u8]) {
        let mut stop = 0;
        for &s in list.iter().chain(std::iter::once(&0)) {
            stop = s;
            if !(s > 0 && s <= self.max_cols) {
                break;
            }
            self.print(if s == 1 { b"*" } else { b"\t*" });
        }
        if stop < self.max_cols {
            self.print(b"\t+");
        }
        self.print(new_line);
    }

    /// `do_set_margin (margin, no_op)`: whether the terminal can set it --
    /// 0 being clearing one -- and, unless `no_op`, set.
    fn do_set_margin(&mut self, margin: i32, no_op: bool) -> bool {
        if margin == 0 {
            // "0 is special case for resetting"
            let mgc = self.cap("mgc");
            if mgc.is_some() && !no_op {
                self.tputs(mgc.as_deref());
            }
            return mgc.is_some();
        }
        if margin < 0 {
            return true;
        }
        // 0-based from here on.
        let mut margin = margin.wrapping_sub(1);
        let max_cols = self.max_cols;
        if let Some(smgl) = self.cap("smgl") {
            if !no_op {
                // "assuming we're on the first column of the line, move the
                // cursor to the column at which we will set a margin."
                if let Some(hpa) = self.cap("hpa") {
                    let s = self.tiparm(1, &hpa, &[margin]);
                    self.tputs(s.as_deref());
                } else if margin >= 1 {
                    if let Some(cuf) = self.cap("cuf") {
                        let s = self.tiparm(1, &cuf, &[margin]);
                        self.tputs(s.as_deref());
                    } else {
                        while margin > 0 {
                            margin = margin.saturating_sub(1);
                            self.putch(b' ');
                        }
                    }
                }
                self.tputs(Some(&smgl));
            }
            return true;
        }
        if let Some(smglp) = self.cap("smglp") {
            if !no_op {
                let s = if self.cap("smgrp").is_some() {
                    self.tiparm(1, &smglp, &[margin])
                } else {
                    self.tiparm(2, &smglp, &[margin, max_cols])
                };
                self.tputs(s.as_deref());
            }
            return true;
        }
        if let Some(smglr) = self.cap("smglr") {
            if !no_op {
                let s = self.tiparm(2, &smglr, &[margin, max_cols]);
                self.tputs(s.as_deref());
            }
            return true;
        }
        false
    }
}

/// `legal_tab_list (tab_list)`: no list, or one of digits, commas and `+`
/// with no comma at its end -- saying which character, or that comma, is
/// wrong.
fn legal_tab_list(progname: &[u8], tab_list: Option<&[u8]>) -> bool {
    let Some(list) = tab_list.filter(|l| !l.is_empty()) else {
        // "if no list given, default to "tabs -8""
        return true;
    };
    let mut m = escape_unprintable(progname).into_bytes();
    if comma_is_needed(Some(list)) {
        let Some(&bad) = list
            .iter()
            .find(|&&c| !(c.is_ascii_digit() || c == b',' || c == b'+'))
        else {
            return true;
        };
        m.extend_from_slice(b": unexpected character found '");
        m.extend_from_slice(escape_unprintable(&[bad]).as_bytes());
        m.extend_from_slice(b"'\n");
    } else {
        m.extend_from_slice(b": trailing comma found '");
        m.extend_from_slice(escape_unprintable(list).as_bytes());
        m.extend_from_slice(b"'\n");
    }
    ulclosestream::stderr_write(&m);
    false
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(&argv, &mut out) {
        Ok(code) | Err(Exit(code)) => code,
    };
    // `exit`'s own flush, whose failure nobody hears of: there is no
    // `close_stdout` here.
    out.flush_at_exit();
    ExitCode::from(status)
}

/// Upstream's `main`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> Result<u8, Exit> {
    let words: Vec<Vec<u8>> = argv.iter().map(|a| os_bytes(a).into_owned()).collect();
    let progname = nc::rootname(words.first().map(Vec::as_slice).unwrap_or_default()).to_vec();
    let mut o = Options {
        term_name: Some(
            std::env::var_os("TERM")
                .map_or_else(|| b"ansi+tabs".to_vec(), |t| os_bytes(&t).into_owned()),
        ),
        margin: -1,
        ..Options::default()
    };
    if let Some(code) = parse(&words, &progname, &mut o, out)? {
        return Ok(code);
    }

    let (tty, saved) = match TtySettings::save(false, &progname) {
        Ok(s) => s,
        Err(code) => return Err(Exit(code)),
    };
    // `setupterm (term_name, fd, (int *) 0)`: with nowhere to put its
    // verdict it prints it and exits.
    let opts = terminfo::Options {
        fd: tty.fd(),
        ..terminfo::Options::default()
    };
    let tenv = terminfo::Env::from_process();
    // A name, or -- after `-T` at the very end -- none, which sends
    // `setupterm` to `TERM`.
    let term = std::env::var_os("TERM").map(|t| os_bytes(&t).into_owned());
    let setup = terminfo::setupterm_with(o.term_name.as_deref(), term.as_deref(), &tenv, &opts);
    if let Some(complaint) = &setup.complaint {
        ulclosestream::stderr_write(&nc::setupterm_complaint(complaint));
        return Err(Exit(1));
    }
    let padding = setup.padding();
    let Some(entry) = setup.entry else {
        return Err(Exit(1));
    };
    let columns = Legacy::of(&entry).columns;
    let mut max_cols = if columns > 0 { columns } else { 80 };
    if o.margin > 0 {
        max_cols = max_cols.saturating_sub(o.margin);
    }
    let strings = terminfo::legacy_copy(&entry, &tenv);
    let mut t = Tabs {
        out: PutChar(out),
        progname,
        entry,
        strings,
        tparm: Tparm::new(),
        padding,
        max_cols,
    };

    // `PROGNAME: terminal type 'NAME' cannot WHAT` -- the name as upstream
    // prints it, `(null)` for none, which `-T` at the end of the command
    // line leaves.
    let cannot = |what: &[u8]| {
        let mut m = escape_unprintable(&t.progname).into_bytes();
        m.extend_from_slice(b": terminal type '");
        match o.term_name.as_deref() {
            Some(name) => m.extend_from_slice(escape_unprintable(name).as_bytes()),
            None => m.extend_from_slice(b"(null)"),
        }
        m.extend_from_slice(b"' cannot ");
        m.extend_from_slice(what);
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
    };
    if t.cap("tbc").is_none() {
        cannot(b"reset tabs");
        return Ok(1);
    }
    if t.cap("hts").is_none() {
        cannot(b"set tabs");
        return Ok(1);
    }
    if !legal_tab_list(&t.progname, o.tab_list.as_deref()) {
        return Ok(1);
    }
    // No list: `add_to_tab_list (&append, "8")`, and with no list there was
    // nothing gathered for it to be added to.
    let tab_list = match o.tab_list.clone() {
        Some(l) => l,
        None => add_to_tab_list(&mut o.append, b"8").unwrap_or_else(|| b"8".to_vec()),
    };

    let mut new_line: &[u8] = b"\n";
    let mut change_tty = false;
    if !o.no_op {
        // "set tty modes to -ocrnl to allow \r"
        if libcall::termios::get_attr(1).is_ok() {
            let old = saved.unwrap_or_default();
            let mut new_settings = old;
            new_settings.c_oflag &= !libcall::termios::OCRNL;
            tty.update(&old, &new_settings);
            change_tty = true;
            new_line = b"\r\n";
        }
        if !t.ansi_clear_tabs() {
            t.putch(b'\r');
        }
        let tbc = t.cap("tbc");
        t.tputs(tbc.as_deref());
    }

    let mut margin = o.margin;
    if margin >= 0 {
        t.putch(b'\r');
        // "reset existing margin before setting margin, to reduce problems
        // moving left of the current margin."
        if margin > 0 && t.do_set_margin(0, o.no_op) {
            t.putch(b'\r');
        }
        if t.do_set_margin(margin, o.no_op) {
            margin = -1;
        }
    }

    let list = t.decode_tabs(&tab_list, margin);
    let mut header = b"tabs ".to_vec();
    header.extend_from_slice(&tab_list);
    header.extend_from_slice(new_line);
    match list {
        Some(list) => {
            if !o.no_op {
                t.do_tabs(&list);
            }
            if o.debug {
                t.print(&header);
                t.print_ruler(&list, new_line);
                t.write_tabs(&list, new_line);
            }
        }
        None if o.debug => t.print(&header),
        None => {}
    }
    if !o.no_op && change_tty {
        tty.restore();
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_trimmed_as_upstream_trims_it() {
        assert_eq!(trimmed_tab_list(b"  1 5  9 "), b"1,5,9");
        assert_eq!(trimmed_tab_list(b"1,,6,11,"), b"1,6,11");
        assert_eq!(trimmed_tab_list(b",5"), b",5");
        assert_eq!(trimmed_tab_list(b"+ 5"), b"+5");
        assert_eq!(trimmed_tab_list(b"   "), b"");
    }

    #[test]
    fn lists_are_gathered_with_a_comma_between() {
        let mut append = None;
        assert_eq!(add_to_tab_list(&mut append, b"1"), Some(b"1".to_vec()));
        assert_eq!(
            add_to_tab_list(&mut append, b"5 9"),
            Some(b"1,5,9".to_vec())
        );
        assert_eq!(
            add_to_tab_list(&mut append, b",12"),
            Some(b"1,5,9,12".to_vec())
        );
        // Nothing in it: what was gathered stands.
        assert_eq!(
            add_to_tab_list(&mut append, b"  "),
            Some(b"1,5,9,12".to_vec())
        );
        let mut none = None;
        assert_eq!(add_to_tab_list(&mut none, b" "), None);
    }

    #[test]
    fn skip_list_stops_at_what_no_list_holds() {
        assert_eq!(skip_list(b"8x"), 1);
        assert_eq!(skip_list(b"1, +5"), 5);
        assert_eq!(skip_list(b""), 0);
    }
}
