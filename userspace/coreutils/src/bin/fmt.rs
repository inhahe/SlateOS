//! `fmt` — fill and join lines into paragraphs of a given width.
//!
//! A port of GNU coreutils 9.4's `src/fmt.c`, function by function: the same
//! options and messages, the same rules for where a paragraph starts and ends,
//! and above all the same line breaks -- which are chosen by a cost function
//! whose every constant shows in the output.
//!
//! # What this replaced
//!
//! `userspace/fmt`, written by hand. It read its arguments as `String` (a file
//! name that was not UTF-8 killed it before its first statement), and it
//! filled lines greedily, so on any paragraph of more than a line its breaks
//! were not GNU's.
//!
//! # How the breaks are chosen
//!
//! Not greedily. Each paragraph is formatted whole, by dynamic programming
//! over its suffixes ([`Fmt::fmt_paragraph`]): a line costs the square of its
//! distance from the goal width (93% of the width unless `-g` says otherwise)
//! plus half the square of its difference from the line after it, and each
//! break costs a constant adjusted by where it falls -- cheaper after a
//! sentence or other punctuation and before an opening parenthesis, dearer
//! after a word that ends in a period without ending a sentence, before the
//! last word of a sentence (an orphan) and after the first (a widow). The last
//! line of a paragraph costs nothing, so it may be as short as it likes.
//!
//! A sentence ends at a word ending in `.`, `?` or `!` -- closing `)]'"` after
//! it allowed -- that is followed by the end of its line or by two spaces:
//! Emacs's rule. On output such a word is followed by two spaces wherever the
//! spacing is rebuilt (at a line's end, and everywhere under `-u`), any other
//! word by one.
//!
//! # What no reading of `--help` suggests
//!
//! **Columns are bytes.** A multibyte character counts as several columns, as
//! it does upstream.
//!
//! **A paragraph holds 1000 words and 5000 bytes.** Past either, the part read
//! so far is formatted, a cheap break near its end is chosen, everything
//! before the break is printed, and the rest carries on as the start of the
//! paragraph -- printed again with the *first* line's indent. A single word
//! longer than 5000 bytes is printed raw, unindented, 5000 bytes at a time.
//!
//! **One tab anywhere turns tabs on everywhere.** Once a file has shown a tab
//! in white space, indentation and runs of spaces that reach a tab stop are
//! written with tabs, to the end of that file.
//!
//! **`fmt DIR` says `fmt: read error`, naming nothing.** Upstream hands the
//! file name to a format string that has no place for it.
//!
//! **A NUL byte in a word counts as an opening bracket, a closing one and a
//! period at once**, because upstream classifies with `strchr`, which finds
//! the terminator of the string it searches.
//!
//! # Reference
//!
//! Measured against GNU `fmt` (coreutils 9.4) through WSL; where measurement
//! could not settle a rule, `coreutils-9.4/src/fmt.c` settled it.
//! `scripts/fmt-diff.sh` is the executable form of every claim here.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::xdectoumax;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write as _};
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `fmt -Z; echo $?` is 1.
const FMT: Program = Program::new("fmt", 1);

/// Upstream's `getopt_long` string, verbatim. The digits are there so that a
/// `-WIDTH` anywhere but first is diagnosed in words of its own rather than
/// as an unknown option.
const SHORT_OPTIONS: &str = "0123456789cstuw:p:g:";

/// Upstream's `long_options`, in its order -- observable through the
/// ambiguity message, which names the entries a prefix matched in table
/// order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("crown-margin", Takes::Nothing),
    ("prefix", Takes::Required),
    ("split-only", Takes::Nothing),
    ("tagged-paragraph", Takes::Nothing),
    ("uniform-spacing", Takes::Nothing),
    ("width", Takes::Required),
    ("goal", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `WIDTH`: the default maximum line width.
const WIDTH: i64 = 75;

/// `LEEWAY`: the default goal is this many percent short of the width.
const LEEWAY: i64 = 7;

/// `DEF_INDENT`: the second-line indent `-t` gives a one-line paragraph whose
/// first line is not indented, before any multi-line paragraph has set one.
const DEF_INDENT: i64 = 3;

/// `MAXWORDS` and `MAXCHARS`: the most of a paragraph held at once. Past
/// either, the paragraph is printed up to a good break and continued
/// ([`Fmt::flush_paragraph`]).
const MAXWORDS: usize = 1000;
const MAXCHARS: usize = 5000;

/// `END_OF_WORD`: the word count at which a paragraph is flushed, which
/// leaves room for the word being read and the sentinel after it.
const END_OF_WORD: usize = MAXWORDS - 2;

/// `TABWIDTH`: tab stops on input and output.
const TABWIDTH: i64 = 8;

/// The default goal is `max_width * GOAL_NUMERATOR / 200`: 93% and a half
/// percent, rounded down.
const GOAL_NUMERATOR: i64 = 2 * (100 - LEEWAY) + 1;

/// stdio's buffer for a file or a pipe -- `st_blksize`, 4096 -- which decides
/// when output is written, and so which failures are noticed when.
const BUFFER: usize = 4096;

/// `COST`: upstream's `long int`.
type Cost = i64;

/// `MAXCOST`.
const MAXCOST: Cost = Cost::MAX;

/// `EQUIV (n)`: a cost measured in squared columns.
const fn equiv(n: i64) -> Cost {
    n.saturating_mul(n)
}

/// `SHORT_COST (n)`: a line `n` columns short of (or past) the goal.
const fn short_cost(n: i64) -> Cost {
    equiv(n.saturating_mul(10))
}

/// `RAGGED_COST (n)`: a line `n` columns longer or shorter than the next.
const fn ragged_cost(n: i64) -> Cost {
    short_cost(n) / 2
}

/// `LINE_COST`: the cost of a break.
const LINE_COST: Cost = equiv(70);

/// `WIDOW_COST (n)`: breaking after the first word of a sentence, `n` long.
fn widow_cost(n: i64) -> Cost {
    equiv(200)
        .checked_div(n.saturating_add(2))
        .unwrap_or(MAXCOST)
}

/// `ORPHAN_COST (n)`: breaking before the last word of a sentence, `n` long.
fn orphan_cost(n: i64) -> Cost {
    equiv(150)
        .checked_div(n.saturating_add(2))
        .unwrap_or(MAXCOST)
}

/// `SENTENCE_BONUS`: breaking at the end of a sentence.
const SENTENCE_BONUS: Cost = equiv(50);

/// `NOBREAK_COST`: breaking after a period that does not end a sentence --
/// which would read as though it did.
const NOBREAK_COST: Cost = equiv(600);

/// `PAREN_BONUS`: breaking before an opening parenthesis.
const PAREN_BONUS: Cost = equiv(40);

/// `PUNCT_BONUS`: breaking after other punctuation.
const PUNCT_BONUS: Cost = equiv(40);

/// `LINE_CREDIT`: the bias toward a later split point when a paragraph has to
/// be printed in parts.
const LINE_CREDIT: Cost = equiv(3);

/// `MAXCOST - LINE_CREDIT`: the most a credit can be added to.
const MAXCOST_LESS_CREDIT: Cost = MAXCOST - LINE_CREDIT;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    crown: bool,
    tagged: bool,
    split: bool,
    uniform: bool,
    /// `-p`, as given: [`Prefix::new`] trims it.
    prefix: Vec<u8>,
    max_width: i64,
    goal_width: i64,
    files: Vec<OsString>,
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let settings = match parse_args(&args) {
        Ok(Request::Run(settings)) => settings,
        Ok(Request::Help) => {
            let mut out = Stream::stdout();
            // A failed write is `close_stdout`'s to report.
            let _ = out.write_all(HELP.as_bytes());
            return stdfd::close_stdout("fmt", out, ExitCode::SUCCESS);
        }
        Ok(Request::Version) => {
            let mut out = Stream::stdout();
            let _ = out.write_all(b"fmt (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("fmt", out, ExitCode::SUCCESS);
        }
        Err(e) => {
            diag!("fmt: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut fmt = Fmt::new(&settings);
    let earned = fmt.run(&settings.files);
    let out = fmt.finish();
    stdfd::close_stdout("fmt", out, earned)
}

/// Read the command line: upstream's `main` up to the files.
///
/// # Errors
/// Any getopt diagnostic, a `-WIDTH` that is not first, or a width that is
/// not a number in range. The widths are checked after the options, as
/// upstream checks them, so `fmt -w x -Z` complains about `-Z`.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut crown = false;
    let mut tagged = false;
    let mut split = false;
    let mut uniform = false;
    let mut prefix = Vec::new();
    let mut max_width_option: Option<Vec<u8>> = None;
    let mut goal_width_option: Option<Vec<u8>> = None;
    let mut files = Vec::new();

    // The old syntax, a dash and digits, is recognised only as the very first
    // argument -- upstream checks `argv[1]` and then hides it from getopt.
    let mut rest = args;
    if let Some((first, after)) = args.split_first() {
        let word = os_bytes(first);
        if let [b'-', digit, ..] = word.as_ref()
            && digit.is_ascii_digit()
        {
            max_width_option = word.get(1..).map(<[u8]>::to_vec);
            rest = after;
        }
    }

    for item in FMT.parse(rest, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(digit, _) if digit.is_ascii_digit() => {
                return Err(FMT.usage_referring(format!(
                    "invalid option -- {}; -WIDTH is recognized only when it is the first\n\
                     option; use -w N instead",
                    char::from(digit)
                )));
            }
            Opt::Short(b'c', _) | Opt::Long("crown-margin", _) => crown = true,
            Opt::Short(b's', _) | Opt::Long("split-only", _) => split = true,
            Opt::Short(b't', _) | Opt::Long("tagged-paragraph", _) => tagged = true,
            Opt::Short(b'u', _) | Opt::Long("uniform-spacing", _) => uniform = true,
            Opt::Short(b'w', Some(v)) | Opt::Long("width", Some(v)) => {
                max_width_option = Some(os_bytes(&v).into_owned());
            }
            Opt::Short(b'g', Some(v)) | Opt::Long("goal", Some(v)) => {
                goal_width_option = Some(os_bytes(&v).into_owned());
            }
            Opt::Short(b'p', Some(v)) | Opt::Long("prefix", Some(v)) => {
                prefix = os_bytes(&v).into_owned();
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => files.push(word.clone()),
            // Every option in the two tables is handled above; an unknown one
            // arrives as an `Err`, and one missing its value cannot parse.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }

    let width = |text: &[u8], max: u64| -> Result<i64, getopt::Error> {
        xdectoumax(text, 0, max, Some(b""), "invalid width")
            .map(|n| i64::try_from(n).unwrap_or(i64::MAX))
            .map_err(|message| FMT.usage(message))
    };
    let mut max_width = match &max_width_option {
        // "Limit max_width to MAXCHARS / 2; otherwise, the resulting output
        // can be quite ugly."
        Some(text) => width(text, u64::try_from(MAXCHARS / 2).unwrap_or(u64::MAX))?,
        None => WIDTH,
    };
    let goal_width = match &goal_width_option {
        Some(text) => {
            // The goal is limited by the width as it stands -- the default, if
            // `-w` was not given, even though the width then follows the goal.
            let goal = width(text, u64::try_from(max_width).unwrap_or(0))?;
            if max_width_option.is_none() {
                max_width = goal.saturating_add(10);
            }
            goal
        }
        None => max_width
            .saturating_mul(GOAL_NUMERATOR)
            .checked_div(200)
            .unwrap_or(0),
    };

    Ok(Request::Run(Settings {
        crown,
        tagged,
        split,
        uniform,
        prefix,
        max_width,
        goal_width,
        files,
    }))
}

/// GNU's `--help`, minus the ancillary block of URLs, as every utility here
/// omits it.
const HELP: &str = "\
Usage: fmt [-WIDTH] [OPTION]... [FILE]...
Reformat each paragraph in the FILE(s), writing to standard output.
The option -WIDTH is an abbreviated form of --width=DIGITS.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -c, --crown-margin        preserve indentation of first two lines
  -p, --prefix=STRING       reformat only lines beginning with STRING,
                              reattaching the prefix to reformatted lines
  -s, --split-only          split long lines, but do not refill
  -t, --tagged-paragraph    indentation of first line different from second
  -u, --uniform-spacing     one space between words, two after sentences
  -w, --width=WIDTH         maximum line width (default of 75 columns)
  -g, --goal=WIDTH          goal width (default of 93% of width)
      --help        display this help and exit
      --version     output version information and exit
";

/// `-p` as upstream's `set_prefix` leaves it.
#[derive(Debug, PartialEq, Eq)]
struct Prefix {
    /// The text between the leading and the trailing spaces: what a line must
    /// begin with (after any indent) and what a reformatted line is given.
    text: Vec<u8>,
    /// `prefix_lead_space`: the spaces trimmed from the front. A line must be
    /// indented at least this far to be formatted.
    lead_space: i64,
    /// `prefix_full_length`: the length with the trailing spaces, which a
    /// line must reach to be formatted.
    full_length: i64,
    /// `prefix_length`: the length without them.
    length: i64,
}

impl Prefix {
    /// `set_prefix`. Only spaces are trimmed, not tabs.
    fn new(given: &[u8]) -> Prefix {
        let lead = given.iter().take_while(|&&b| b == b' ').count();
        let rest = given.get(lead..).unwrap_or_default();
        let kept = rest
            .len()
            .saturating_sub(rest.iter().rev().take_while(|&&b| b == b' ').count());
        Prefix {
            text: rest.get(..kept).unwrap_or_default().to_vec(),
            lead_space: as_i64(lead),
            full_length: as_i64(rest.len()),
            length: as_i64(kept),
        }
    }
}

/// A `usize` as a column count. Every caller's value is a length bounded by
/// an argument or a buffer, far below `i64::MAX`.
fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A column count as a `usize`, for the values upstream stores in an `int`
/// and uses as a length: never negative where this is called.
fn as_usize(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `struct Word`.
#[derive(Clone, Copy, Debug, Default)]
struct Word {
    /// `text`: where the word starts in [`Fmt::parabuf`].
    text: usize,
    /// `length`.
    length: i64,
    /// `space`: the white space after it -- as read, or as rebuilt.
    space: i64,
    /// `paren`: starts with an opening bracket or quote.
    paren: bool,
    /// `period`: ends in `[.?!][])"']*`.
    period: bool,
    /// `punct`: ends in punctuation.
    punct: bool,
    /// `final`: ends a sentence.
    fin: bool,
    /// `line_length`: the length of the best line starting here.
    line_length: i64,
    /// `best_cost`: the cost of the best paragraph starting here.
    best_cost: Cost,
    /// `next_break`: the word the best line starting here stops before.
    next_break: usize,
}

/// `c_isspace`: what ends a word. Only a space and a tab are white space
/// between words ([`Fmt::get_space`]); the other four begin the next one.
fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isopen`: `strchr ("(['`\"", c) != nullptr`, which is also true of NUL.
fn isopen(b: u8) -> bool {
    matches!(b, b'(' | b'[' | b'\'' | b'`' | b'"' | 0)
}

/// `isclose`, NUL included for the same reason.
fn isclose(b: u8) -> bool {
    matches!(b, b')' | b']' | b'\'' | b'"' | 0)
}

/// `isperiod`, NUL included for the same reason.
fn isperiod(b: u8) -> bool {
    matches!(b, b'.' | b'?' | b'!' | 0)
}

/// One input, and stdio's view of it: the buffer `getc` reads from and the
/// two flags, `feof` and `ferror`.
struct Input {
    source: Source,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    /// `feof`: sticky, as glibc has made it since 2.28 -- once set, `getc`
    /// answers EOF without reading.
    eof: bool,
    /// `ferror`. Not sticky in the same way: glibc's next `getc` reads again.
    error: bool,
}

/// Where an [`Input`] reads from.
enum Source {
    /// Descriptor 0, which is read through and closed only at the end.
    Stdin,
    File(std::fs::File),
    /// Between files: nothing to read.
    Nothing,
}

impl Input {
    fn new(source: Source) -> Input {
        Input {
            source,
            buf: vec![0; BUFFER],
            pos: 0,
            len: 0,
            eof: false,
            error: false,
        }
    }

    /// `getc`: the next byte, or `None` at the end of the input or on a read
    /// error -- which `error` then records.
    fn getc(&mut self) -> Option<u8> {
        if self.pos < self.len {
            let b = self.buf.get(self.pos).copied();
            self.pos = self.pos.saturating_add(1);
            return b;
        }
        if self.eof {
            return None;
        }
        let got = match &mut self.source {
            Source::Stdin => stdfd::read(0, &mut self.buf),
            Source::File(file) => read_once(file, &mut self.buf),
            Source::Nothing => Ok(0),
        };
        match got {
            Ok(0) => {
                self.eof = true;
                None
            }
            Ok(n) => {
                self.len = n.min(self.buf.len());
                self.pos = 1;
                self.buf.first().copied()
            }
            Err(_) => {
                self.error = true;
                None
            }
        }
    }
}

/// One `read(2)`, retrying only an interruption, as `stdfd::read` does.
fn read_once(file: &mut std::fs::File, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match file.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            got => return got,
        }
    }
}

/// Upstream's file-scope state. The settings never change; the rest is the
/// paragraph being read and where reading and writing have got to.
struct Fmt {
    crown: bool,
    tagged: bool,
    split: bool,
    uniform: bool,
    prefix: Prefix,
    /// `max_width`: no line is longer, unless one word is.
    max_width: i64,
    /// `goal_width`: the length lines aim for.
    goal_width: i64,

    /// `in_column`: the column reached on the line being read.
    in_column: i64,
    /// `out_column`: the column reached on the line being written.
    out_column: i64,
    /// `parabuf`: the text of the paragraph's words, end to end.
    parabuf: Vec<u8>,
    /// `wptr`: where the next byte of a word goes in `parabuf`.
    wptr: usize,
    /// `word`: the paragraph's words. Index [`Fmt::word_limit`] is one past
    /// the last complete word -- the sentinel while a paragraph is formatted.
    word: Vec<Word>,
    word_limit: usize,
    /// Where an out-of-range word index would land. Never reached: every
    /// index is below `MAXWORDS` by construction (`word_limit` is flushed at
    /// `MAXWORDS - 2`); this exists so that reaching it would be harmless.
    spare: Word,
    /// `tabs`: a tab has been read in this file, so output uses tabs.
    tabs: bool,
    /// `prefix_indent`: the paragraph's indent before the prefix.
    prefix_indent: i64,
    /// `first_indent` and `other_indent`: the paragraph's first line's
    /// indent, and the rest's.
    first_indent: i64,
    other_indent: i64,
    /// `next_char`: the first character of the next line after its prefix
    /// and indent, or the first that failed to match the prefix.
    next_char: Option<u8>,
    /// `next_prefix_indent`: the white space before the prefix on that line.
    next_prefix_indent: i64,
    /// `last_line_length`: the length of the last line printed of a
    /// paragraph being printed in parts.
    last_line_length: i64,

    input: Input,
    out: Stream,
    /// stdio's buffer in front of `out`: see [`BUFFER`].
    obuf: Vec<u8>,
    /// Standard output's reader went away. Upstream died of `SIGPIPE` at
    /// that write; here nothing more is read or written, and nothing more is
    /// said.
    gone: bool,
    /// A test's copy of everything printed, kept instead of written.
    #[cfg(test)]
    kept: Option<Vec<u8>>,
}

impl Fmt {
    fn new(settings: &Settings) -> Fmt {
        Fmt {
            crown: settings.crown,
            tagged: settings.tagged,
            split: settings.split,
            uniform: settings.uniform,
            prefix: Prefix::new(&settings.prefix),
            max_width: settings.max_width,
            goal_width: settings.goal_width,
            in_column: 0,
            out_column: 0,
            parabuf: vec![0; MAXCHARS],
            wptr: 0,
            word: vec![Word::default(); MAXWORDS],
            word_limit: 0,
            spare: Word::default(),
            tabs: false,
            prefix_indent: 0,
            first_indent: 0,
            other_indent: 0,
            next_char: None,
            next_prefix_indent: 0,
            last_line_length: 0,
            input: Input::new(Source::Nothing),
            out: Stream::stdout(),
            obuf: Vec::with_capacity(BUFFER),
            gone: false,
            #[cfg(test)]
            kept: None,
        }
    }

    /// Upstream's loop over the files in `main`, and its close of standard
    /// input after it.
    fn run(&mut self, files: &[OsString]) -> ExitCode {
        let mut ok = true;
        let mut have_read_stdin = false;
        if files.is_empty() {
            have_read_stdin = true;
            ok = self.file(Input::new(Source::Stdin), OsStr::new("-"));
        }
        for name in files {
            if self.gone {
                break;
            }
            if name == "-" {
                ok &= self.file(Input::new(Source::Stdin), name);
                have_read_stdin = true;
            } else {
                match std::fs::File::open(name) {
                    Ok(file) => ok &= self.file(Input::new(Source::File(file)), name),
                    Err(e) => {
                        self.say(&format!(
                            "fmt: cannot open {} for reading: {}",
                            quoteaf_os(name),
                            strerror(&e)
                        ));
                        ok = false;
                    }
                }
            }
        }
        if have_read_stdin
            && !self.gone
            && let Err(e) = stdfd::close_stdin()
        {
            self.say(&format!("fmt: closing standard input: {}", strerror(&e)));
            return ExitCode::FAILURE;
        }
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    /// Hand standard output over for the final flush.
    fn finish(mut self) -> Stream {
        self.spill();
        self.out
    }

    /// `fmt`: format one input. Returns whether it was read and closed
    /// without error.
    fn file(&mut self, input: Input, name: &OsStr) -> bool {
        self.input = input;
        self.tabs = false;
        self.other_indent = 0;
        self.next_char = self.get_prefix();
        while self.get_paragraph() {
            self.fmt_paragraph();
            self.put_paragraph(self.word_limit);
        }
        let input = std::mem::replace(&mut self.input, Input::new(Source::Nothing));
        // `ferror (f) ? 0 : -1`: a read error is reported with `errno` 0, and
        // a failed close only when nothing failed before it.
        let mut failure: Option<Option<io::Error>> = input.error.then_some(None);
        if let Source::File(file) = input.source
            && let Err(e) = stdfd::close(file)
            && failure.is_none()
        {
            failure = Some(Some(e));
        }
        match failure {
            None => true,
            // `error (0, 0, _("read error"), quotef (file))`: the format has
            // no conversion, so the name is never printed.
            Some(None) => {
                self.say("fmt: read error");
                false
            }
            Some(Some(e)) => {
                self.say(&format!("fmt: {}: {}", quotef_os(name), strerror(&e)));
                false
            }
        }
    }

    /// `error (0, ...)`: flush standard output first, as `error` does, and
    /// stay quiet if that finds its reader gone -- upstream would have died
    /// in the flush.
    fn say(&mut self, message: &str) {
        self.spill();
        if !self.gone {
            // Never fails; a failure is recorded and checked just below.
            let _ = self.out.flush();
            self.check_reader();
        }
        if !self.gone {
            stdfd::diag_line(message);
        }
    }

    /// Hand what stdio's buffer holds to standard output.
    fn spill(&mut self) {
        if self.obuf.is_empty() {
            return;
        }
        #[cfg(test)]
        if let Some(kept) = self.kept.as_mut() {
            kept.append(&mut self.obuf);
            return;
        }
        if !self.gone {
            // Never fails; a failure is recorded in the stream, for
            // `close_stdout`, and a reader that left is noticed here.
            let _ = self.out.write_all(&self.obuf);
            self.check_reader();
        }
        self.obuf.clear();
    }

    fn check_reader(&mut self) {
        if self.out.errored() && self.out.error().is_some_and(|e| stdfd::reader_gone(&e)) {
            self.gone = true;
        }
    }

    /// `putchar`.
    fn putchar(&mut self, b: u8) {
        if self.gone {
            return;
        }
        self.obuf.push(b);
        if self.obuf.len() >= BUFFER {
            self.spill();
        }
    }

    /// `getc` on the current input -- which ends where standard output's
    /// reader went away, because upstream ended there.
    fn getc(&mut self) -> Option<u8> {
        if self.gone {
            return None;
        }
        self.input.getc()
    }

    fn word(&self, i: usize) -> Word {
        self.word.get(i).copied().unwrap_or_default()
    }

    fn word_mut(&mut self, i: usize) -> &mut Word {
        match self.word.get_mut(i) {
            Some(w) => w,
            None => &mut self.spare,
        }
    }

    /// `set_other_indent`: the indent of the paragraph's lines after the
    /// first, by mode.
    fn set_other_indent(&mut self, same_paragraph: bool) {
        if self.split {
            self.other_indent = self.first_indent;
        } else if self.crown {
            self.other_indent = if same_paragraph {
                self.in_column
            } else {
                self.first_indent
            };
        } else if self.tagged {
            if same_paragraph && self.in_column != self.first_indent {
                self.other_indent = self.in_column;
            } else if self.other_indent == self.first_indent {
                // "Only one line: use the secondary indent from last time if
                // it splits, or 0 if there have been no multi-line paragraphs
                // in the input so far. But if these rules make the two
                // indents the same, pick a new secondary indent."
                self.other_indent = if self.first_indent == 0 {
                    DEF_INDENT
                } else {
                    0
                };
            }
        } else {
            self.other_indent = self.first_indent;
        }
    }

    /// `get_paragraph`: copy the lines that are not part of any paragraph,
    /// then read one paragraph. False at the end of the input.
    fn get_paragraph(&mut self) -> bool {
        self.last_line_length = 0;
        let mut c = self.next_char;

        // Blank lines, and lines without the prefix, are copied.
        while c == Some(b'\n')
            || c.is_none()
            || self.next_prefix_indent < self.prefix.lead_space
            || self.in_column
                < self
                    .next_prefix_indent
                    .saturating_add(self.prefix.full_length)
        {
            c = self.copy_rest(c);
            if c.is_none() {
                self.next_char = None;
                return false;
            }
            self.putchar(b'\n');
            c = self.get_prefix();
        }

        self.prefix_indent = self.next_prefix_indent;
        self.first_indent = self.in_column;
        self.wptr = 0;
        self.word_limit = 0;
        c = self.get_line(c);
        let same = self.same_para(c);
        self.set_other_indent(same);

        if self.split {
            // A line is a paragraph.
        } else if self.crown {
            if self.same_para(c) {
                loop {
                    c = self.get_line(c);
                    if !(self.same_para(c) && self.in_column == self.other_indent) {
                        break;
                    }
                }
            }
        } else if self.tagged {
            if self.same_para(c) && self.in_column != self.first_indent {
                loop {
                    c = self.get_line(c);
                    if !(self.same_para(c) && self.in_column == self.other_indent) {
                        break;
                    }
                }
            }
        } else {
            while self.same_para(c) && self.in_column == self.other_indent {
                c = self.get_line(c);
            }
        }

        // The paragraph's last word ends a sentence.
        if let Some(last) = self.word_limit.checked_sub(1) {
            let w = self.word_mut(last);
            w.period = true;
            w.fin = true;
        }
        self.next_char = c;
        true
    }

    /// `copy_rest`: copy a line that is not part of a paragraph -- `c` is
    /// the character that failed to match the prefix, or the newline or end
    /// of a line that was blank after it. Returns what ended the line.
    fn copy_rest(&mut self, mut c: Option<u8>) -> Option<u8> {
        self.out_column = 0;
        let line_ended = matches!(c, None | Some(b'\n'));
        if self.in_column > self.next_prefix_indent || !line_ended {
            self.put_space(self.next_prefix_indent);
            let mut s = 0;
            while self.out_column != self.in_column {
                let Some(&b) = self.prefix.text.get(s) else {
                    break;
                };
                self.putchar(b);
                s = s.saturating_add(1);
                self.out_column = self.out_column.saturating_add(1);
            }
            if !line_ended {
                self.put_space(self.in_column.saturating_sub(self.out_column));
            }
            if c.is_none()
                && self.in_column >= self.next_prefix_indent.saturating_add(self.prefix.length)
            {
                self.putchar(b'\n');
            }
        }
        while let Some(b) = c {
            if b == b'\n' {
                break;
            }
            self.putchar(b);
            c = self.getc();
        }
        c
    }

    /// `same_para`: whether a line whose first character after its prefix
    /// and indent is `c` can continue the paragraph.
    fn same_para(&self, c: Option<u8>) -> bool {
        self.next_prefix_indent == self.prefix_indent
            && self.in_column
                >= self
                    .next_prefix_indent
                    .saturating_add(self.prefix.full_length)
            && !matches!(c, None | Some(b'\n'))
    }

    /// `get_line`: read the rest of a line, starting at `c`, into words.
    /// Returns the first character of the next line after its prefix.
    fn get_line(&mut self, mut c: Option<u8>) -> Option<u8> {
        // A word: everything up to a space, a tab or a line's end. `c` is
        // never the end of the input on the first pass -- the callers see to
        // that -- and the end of a line stops the loop at its foot.
        while let Some(mut byte) = c {
            let limit = self.word_limit;
            let start_text = self.wptr;
            self.word_mut(limit).text = start_text;
            loop {
                if self.wptr == MAXCHARS {
                    self.set_other_indent(true);
                    self.flush_paragraph();
                }
                if let Some(slot) = self.parabuf.get_mut(self.wptr) {
                    *slot = byte;
                }
                self.wptr = self.wptr.saturating_add(1);
                c = self.getc();
                match c {
                    Some(b) if !c_isspace(b) => byte = b,
                    _ => break,
                }
            }
            // A flush moved the word down, so its start is read back.
            let limit = self.word_limit;
            let length = as_i64(self.wptr.saturating_sub(self.word(limit).text));
            self.word_mut(limit).length = length;
            self.in_column = self.in_column.saturating_add(length);
            self.check_punctuation(limit);

            // The space after it.
            let start = self.in_column;
            c = self.get_space(c);
            let uniform = self.uniform;
            let space = self.in_column.saturating_sub(start);
            let w = self.word_mut(limit);
            w.space = space;
            w.fin = c.is_none() || (w.period && (c == Some(b'\n') || w.space > 1));
            if matches!(c, None | Some(b'\n')) || uniform {
                w.space = if w.fin { 2 } else { 1 };
            }
            if self.word_limit == END_OF_WORD {
                self.set_other_indent(true);
                self.flush_paragraph();
            }
            self.word_limit = self.word_limit.saturating_add(1);
            if c == Some(b'\n') {
                break;
            }
        }
        self.get_prefix()
    }

    /// `get_prefix`: read a line's indent and prefix. Returns the first
    /// character after them, or the first that failed to match the prefix.
    fn get_prefix(&mut self) -> Option<u8> {
        self.in_column = 0;
        let first = self.getc();
        let mut c = self.get_space(first);
        if self.prefix.length == 0 {
            self.next_prefix_indent = self.prefix.lead_space.min(self.in_column);
        } else {
            self.next_prefix_indent = self.in_column;
            let mut i = 0;
            while let Some(&pc) = self.prefix.text.get(i) {
                if c != Some(pc) {
                    return c;
                }
                self.in_column = self.in_column.saturating_add(1);
                c = self.getc();
                i = i.saturating_add(1);
            }
            c = self.get_space(c);
        }
        c
    }

    /// `get_space`: read spaces and tabs, counting columns. Returns the first
    /// character that is neither.
    fn get_space(&mut self, mut c: Option<u8>) -> Option<u8> {
        loop {
            match c {
                Some(b' ') => self.in_column = self.in_column.saturating_add(1),
                Some(b'\t') => {
                    self.tabs = true;
                    self.in_column = next_tab_stop(self.in_column);
                }
                _ => return c,
            }
            c = self.getc();
        }
    }

    /// `check_punctuation`: what a word begins and ends with.
    fn check_punctuation(&mut self, i: usize) {
        let w = self.word(i);
        let text = self
            .parabuf
            .get(w.text..w.text.saturating_add(as_usize(w.length)))
            .unwrap_or_default();
        let (Some(&first), Some(&last)) = (text.first(), text.last()) else {
            return;
        };
        // `while (start < finish && isclose (*finish)) finish--;`
        let mut finish = text.len().saturating_sub(1);
        while finish > 0 && text.get(finish).is_some_and(|&b| isclose(b)) {
            finish = finish.saturating_sub(1);
        }
        let period = text.get(finish).is_some_and(|&b| isperiod(b));
        let w = self.word_mut(i);
        w.paren = isopen(first);
        // `ispunct`, which in the C and C.UTF-8 locales is ASCII's.
        w.punct = last.is_ascii_punctuation();
        w.period = period;
    }

    /// `flush_paragraph`: the paragraph has outgrown the buffers. Print it up
    /// to a good break near its end and keep the rest.
    fn flush_paragraph(&mut self) {
        // "In the special case where it's all one word, just flush it."
        if self.word_limit == 0 {
            let len = self.wptr.min(self.parabuf.len());
            for i in 0..len {
                let b = self.parabuf.get(i).copied().unwrap_or_default();
                self.putchar(b);
            }
            self.wptr = 0;
            return;
        }

        self.fmt_paragraph();

        // The break whose line costs least relative to the rest of the
        // paragraph after it, favouring later ones a little each line.
        let mut split_point = self.word_limit;
        let mut best_break = MAXCOST;
        let mut w = self.word(0).next_break;
        while w != self.word_limit {
            let next = self.word(w).next_break;
            let saving = self
                .word(w)
                .best_cost
                .saturating_sub(self.word(next).best_cost);
            if saving < best_break {
                split_point = w;
                best_break = saving;
            }
            if best_break <= MAXCOST_LESS_CREDIT {
                best_break = best_break.saturating_add(LINE_CREDIT);
            }
            // The chain only ever moves forward; a step back would loop
            // forever, so stop rather than trust it.
            if next <= w {
                break;
            }
            w = next;
        }
        self.put_paragraph(split_point);

        // Move what is left to the front of both buffers.
        let shift = self.word(split_point).text;
        let end = self.wptr.min(self.parabuf.len());
        if shift <= end {
            self.parabuf.copy_within(shift..end, 0);
        }
        self.wptr = self.wptr.saturating_sub(shift);
        for i in split_point..=self.word_limit {
            let w = self.word_mut(i);
            w.text = w.text.saturating_sub(shift);
        }
        let last = self.word_limit.min(self.word.len().saturating_sub(1));
        if split_point <= last {
            self.word.copy_within(split_point..=last, 0);
        }
        self.word_limit = self.word_limit.saturating_sub(split_point);
    }

    /// `fmt_paragraph`: the cheapest way to break the paragraph, found for
    /// every suffix of it from the shortest to the whole.
    fn fmt_paragraph(&mut self) {
        let limit = self.word_limit;
        self.word_mut(limit).best_cost = 0;
        let saved_length = self.word(limit).length;
        // The sentinel: a "word" no line can reach past.
        self.word_mut(limit).length = self.max_width;

        for start in (0..limit).rev() {
            let mut best = MAXCOST;
            let mut len = if start == 0 {
                self.first_indent
            } else {
                self.other_indent
            };
            // At least one word, however long, in the line.
            let mut w = start;
            len = len.saturating_add(self.word(w).length);
            loop {
                w = w.saturating_add(1);
                // Consider breaking before `w`.
                let mut wcost = self
                    .line_cost(w, len)
                    .saturating_add(self.word(w).best_cost);
                if start == 0 && self.last_line_length > 0 {
                    wcost = wcost
                        .saturating_add(ragged_cost(len.saturating_sub(self.last_line_length)));
                }
                if wcost < best {
                    best = wcost;
                    let s = self.word_mut(start);
                    s.next_break = w;
                    s.line_length = len;
                }
                if w == limit {
                    break;
                }
                len = len
                    .saturating_add(self.word(w.saturating_sub(1)).space)
                    .saturating_add(self.word(w).length);
                if len >= self.max_width {
                    break;
                }
            }
            let cost = best.saturating_add(self.base_cost(start));
            self.word_mut(start).best_cost = cost;
        }

        self.word_mut(limit).length = saved_length;
    }

    /// `base_cost`: the part of breaking before word `this` that does not
    /// depend on the line's length.
    fn base_cost(&self, this: usize) -> Cost {
        let mut cost = LINE_COST;
        if let Some(before) = this.checked_sub(1) {
            let prev = self.word(before);
            if prev.period {
                if prev.fin {
                    cost = cost.saturating_sub(SENTENCE_BONUS);
                } else {
                    cost = cost.saturating_add(NOBREAK_COST);
                }
            } else if prev.punct {
                cost = cost.saturating_sub(PUNCT_BONUS);
            } else if this > 1 && self.word(this.saturating_sub(2)).fin {
                cost = cost.saturating_add(widow_cost(prev.length));
            }
        }
        let w = self.word(this);
        if w.paren {
            cost = cost.saturating_sub(PAREN_BONUS);
        } else if w.fin {
            cost = cost.saturating_add(orphan_cost(w.length));
        }
        cost
    }

    /// `line_cost`: the part of breaking before word `next` that depends on
    /// `len`, the length of the line ending there.
    fn line_cost(&self, next: usize, len: i64) -> Cost {
        if next == self.word_limit {
            return 0;
        }
        let mut cost = short_cost(self.goal_width.saturating_sub(len));
        let w = self.word(next);
        if w.next_break != self.word_limit {
            cost = cost.saturating_add(ragged_cost(len.saturating_sub(w.line_length)));
        }
        cost
    }

    /// `put_paragraph`: print the paragraph's lines up to, not including,
    /// the one starting at `finish`, which is on the chain of breaks.
    fn put_paragraph(&mut self, finish: usize) {
        self.put_line(0, self.first_indent);
        let mut w = self.word(0).next_break;
        // The chain only moves forward, so `<` stops where `!=` would and
        // cannot run past a `finish` that is somehow not on it.
        while w < finish {
            self.put_line(w, self.other_indent);
            w = self.word(w).next_break;
        }
    }

    /// `put_line`: print the line starting at word `w`, indented to
    /// `indent`, prefix and all.
    fn put_line(&mut self, mut w: usize, indent: i64) {
        self.out_column = 0;
        self.put_space(self.prefix_indent);
        for i in 0..self.prefix.text.len() {
            let b = self.prefix.text.get(i).copied().unwrap_or_default();
            self.putchar(b);
        }
        self.out_column = self.out_column.saturating_add(self.prefix.length);
        self.put_space(indent.saturating_sub(self.out_column));

        let endline = self.word(w).next_break.saturating_sub(1);
        while w < endline {
            self.put_word(w);
            self.put_space(self.word(w).space);
            w = w.saturating_add(1);
        }
        self.put_word(w);
        self.last_line_length = self.out_column;
        self.putchar(b'\n');
    }

    /// `put_word`.
    fn put_word(&mut self, i: usize) {
        let w = self.word(i);
        let length = as_usize(w.length);
        for at in w.text..w.text.saturating_add(length) {
            let b = self.parabuf.get(at).copied().unwrap_or_default();
            self.putchar(b);
        }
        self.out_column = self.out_column.saturating_add(w.length);
    }

    /// `put_space`: `space` columns of white space -- as tabs where they
    /// reach a tab stop, if the input has shown a tab.
    fn put_space(&mut self, space: i64) {
        let space_target = self.out_column.saturating_add(space);
        if self.tabs {
            let tab_target = space_target
                .checked_div(TABWIDTH)
                .unwrap_or(0)
                .saturating_mul(TABWIDTH);
            if self.out_column.saturating_add(1) < tab_target {
                while self.out_column < tab_target {
                    self.putchar(b'\t');
                    self.out_column = next_tab_stop(self.out_column);
                }
            }
        }
        while self.out_column < space_target {
            self.putchar(b' ');
            self.out_column = self.out_column.saturating_add(1);
        }
    }
}

/// The tab stop after `column`: `(column / TABWIDTH + 1) * TABWIDTH`.
fn next_tab_stop(column: i64) -> i64 {
    column
        .checked_div(TABWIDTH)
        .unwrap_or(0)
        .saturating_add(1)
        .saturating_mul(TABWIDTH)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn settings(words: &[&str]) -> Settings {
        match parse_args(&argv(words)) {
            Ok(Request::Run(s)) => s,
            other => panic!("{words:?}: {other:?}"),
        }
    }

    fn refusal(words: &[&str]) -> String {
        match parse_args(&argv(words)) {
            Err(e) => e.message(),
            other => panic!("{words:?}: {other:?}"),
        }
    }

    /// Format `input` with `words` as the options, through everything but
    /// the descriptors: the input is read from memory and the output kept.
    fn fill(words: &[&str], input: &[u8]) -> Vec<u8> {
        let mut fmt = Fmt::new(&settings(words));
        fmt.kept = Some(Vec::new());
        fmt.input = Input::new(Source::Nothing);
        fmt.input.buf = input.to_vec();
        fmt.input.len = input.len();
        fmt.tabs = false;
        fmt.other_indent = 0;
        fmt.next_char = fmt.get_prefix();
        while fmt.get_paragraph() {
            fmt.fmt_paragraph();
            fmt.put_paragraph(fmt.word_limit);
        }
        fmt.spill();
        fmt.kept.take().unwrap()
    }

    fn text(words: &[&str], input: &str) -> String {
        String::from_utf8(fill(words, input.as_bytes())).unwrap()
    }

    #[test]
    fn the_widths_and_their_defaults() {
        let s = settings(&[]);
        assert_eq!((s.max_width, s.goal_width), (75, 70));
        let s = settings(&["-w", "40"]);
        assert_eq!((s.max_width, s.goal_width), (40, 37));
        let s = settings(&["-40"]);
        assert_eq!((s.max_width, s.goal_width), (40, 37));
        // The goal alone sets the width ten past it.
        let s = settings(&["-g", "50"]);
        assert_eq!((s.max_width, s.goal_width), (60, 50));
        let s = settings(&["-g", "50", "-w", "55"]);
        assert_eq!((s.max_width, s.goal_width), (55, 50));
        // A later -w overrides the old syntax.
        let s = settings(&["-30", "-w", "20"]);
        assert_eq!(s.max_width, 20);
    }

    #[test]
    fn the_width_diagnostics() {
        assert_eq!(refusal(&["-w", "x"]), "invalid width: \u{2018}x\u{2019}");
        assert_eq!(
            refusal(&["-w", "3000"]),
            "invalid width: \u{2018}3000\u{2019}: Numerical result out of range"
        );
        assert_eq!(
            refusal(&["-w", "99999999999999"]),
            "invalid width: \u{2018}99999999999999\u{2019}: Value too large for defined data type"
        );
        // The goal is limited by the width in force: the default 75.
        assert_eq!(
            refusal(&["-g", "80"]),
            "invalid width: \u{2018}80\u{2019}: Numerical result out of range"
        );
        assert_eq!(refusal(&["-72x"]), "invalid width: \u{2018}72x\u{2019}");
        // The width is checked after the options, so -Z is what is reported.
        assert!(refusal(&["-w", "x", "-Z"]).starts_with("invalid option -- 'Z'"));
    }

    #[test]
    fn a_width_that_is_not_first_is_explained() {
        assert_eq!(
            refusal(&["-c7"]),
            "invalid option -- 7; -WIDTH is recognized only when it is the first\n\
             option; use -w N instead\nTry 'fmt --help' for more information."
        );
        assert!(refusal(&["-c", "-72"]).starts_with("invalid option -- 7;"));
    }

    #[test]
    fn the_prefix_is_trimmed_of_spaces_only() {
        let p = Prefix::new(b"  # ");
        assert_eq!(p.text, b"#");
        assert_eq!((p.lead_space, p.full_length, p.length), (2, 2, 1));
        let p = Prefix::new(b"\t>");
        assert_eq!(p.text, b"\t>");
        assert_eq!((p.lead_space, p.full_length, p.length), (0, 2, 2));
        let p = Prefix::new(b"   ");
        assert_eq!(p.text, b"");
        assert_eq!((p.lead_space, p.full_length, p.length), (3, 0, 0));
    }

    /// Every expectation in these tests is GNU fmt 9.4's output for the same
    /// input.
    #[test]
    fn short_lines_are_joined_and_long_ones_broken() {
        assert_eq!(text(&[], "a\nb\nc\n"), "a b c\n");
        assert_eq!(
            text(&["-w", "10"], "one two three four\n"),
            "one two\nthree\nfour\n"
        );
        // A line is always shorter than the width: ten columns do not fit in
        // `-w 10`, and do in `-w 11`.
        assert_eq!(text(&["-w", "10"], "three four\n"), "three\nfour\n");
        assert_eq!(text(&["-w", "11"], "three four\n"), "three four\n");
    }

    #[test]
    fn blank_lines_separate_paragraphs() {
        assert_eq!(text(&[], "a\nb\n\nc\n"), "a b\n\nc\n");
        // A line of white space is blank, and printed empty.
        assert_eq!(text(&[], "a\n   \nb\n"), "a\n\nb\n");
    }

    #[test]
    fn the_last_line_ends_with_a_newline() {
        assert_eq!(text(&[], "a b"), "a b\n");
        // White space alone, unterminated, still makes a line.
        assert_eq!(text(&[], "  "), "\n");
    }

    #[test]
    fn a_change_of_indent_starts_a_paragraph() {
        assert_eq!(text(&[], "a\n  b\n  c\n"), "a\n  b c\n");
        // Unless -c or -t lets the first line's indent differ from the rest.
        assert_eq!(text(&["-c"], "a\n  b\n  c\n"), "a b c\n");
        assert_eq!(text(&["-t"], "a\n  b\n  c\n"), "a b c\n");
        // -t insists that it differ.
        assert_eq!(text(&["-t"], "a\nb\n"), "a\nb\n");
    }

    #[test]
    fn sentences_end_with_two_spaces_under_u() {
        assert_eq!(
            text(&["-u"], "One.  Two.\nThree   four\n"),
            "One.  Two.  Three four\n"
        );
        // A period mid-line followed by one space ends no sentence.
        assert_eq!(text(&["-u"], "Mr. Smith\n"), "Mr. Smith\n");
    }

    #[test]
    fn split_only_never_joins() {
        assert_eq!(text(&["-s"], "a\nb\n"), "a\nb\n");
        assert_eq!(text(&["-s", "-w", "5"], "aa bb cc\n"), "aa\nbb\ncc\n");
    }

    #[test]
    fn the_prefix_selects_lines_and_is_reattached() {
        assert_eq!(text(&["-p>"], "> a\n> b\nc\n"), "> a b\nc\n");
        assert_eq!(text(&["-p", "#"], "x\n#a\n#b\n"), "x\n#a b\n");
    }

    #[test]
    fn a_tab_turns_tabs_on() {
        assert_eq!(text(&[], "\ta\n\tb\n"), "\ta b\n");
        assert_eq!(text(&[], "        a\n        b\n"), "        a b\n");
    }

    #[test]
    fn a_nul_is_bracket_and_period_alike() {
        let mut fmt = Fmt::new(&settings(&[]));
        let mut classify = |text: &[u8]| {
            fmt.parabuf[..text.len()].copy_from_slice(text);
            fmt.word[0] = Word {
                length: as_i64(text.len()),
                ..Word::default()
            };
            fmt.check_punctuation(0);
            let w = fmt.word[0];
            (w.paren, w.period, w.punct)
        };
        // Alone, a NUL opens a word and ends it in a period -- but is not
        // punctuation, which `ispunct` decides.
        assert_eq!(classify(b"\0"), (true, true, false));
        // After a letter it is a closing bracket, skipped on the way to the
        // byte that decides the period.
        assert_eq!(classify(b"x\0"), (false, false, false));
        assert_eq!(classify(b"x.\0"), (false, true, false));
    }

    #[test]
    fn a_word_over_the_buffer_is_printed_raw() {
        let long = "x".repeat(MAXCHARS + 10);
        let out = text(&["-w", "20"], &format!("  {long}\n"));
        // The first MAXCHARS bytes go out unindented; the indent lands in the
        // middle of the word, before the rest of it.
        assert_eq!(
            out,
            format!("{}  {}\n", "x".repeat(MAXCHARS), "x".repeat(10))
        );
    }

    #[test]
    fn costs_are_upstreams() {
        assert_eq!(LINE_COST, 4900);
        assert_eq!(short_cost(-3), 900);
        assert_eq!(ragged_cost(3), 450);
        assert_eq!(widow_cost(3), 8000);
        assert_eq!(orphan_cost(3), 4500);
        assert_eq!(next_tab_stop(0), 8);
        assert_eq!(next_tab_stop(7), 8);
        assert_eq!(next_tab_stop(8), 16);
    }
}
