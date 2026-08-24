//! `expand` — convert tabs in each file to spaces.
//!
//! ```text
//! expand [-i] [-t LIST] [-LIST] [--] [FILE...]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-i`, `--initial` | convert only the tabs before the line's first non-blank |
//! | `-t LIST`, `--tabs=LIST` | tab stops, comma- or blank-separated; default 8 |
//! | `-LIST` | the same list written without the `-t`, e.g. `expand -4` |
//!
//! The grammar of `LIST` — the `/` and `+` prefixes, when a prefix is no prefix
//! at all, which errors abandon the rest of the argument — lives in
//! [`coreutils::tabstops`], because `unexpand -t` must accept exactly the same
//! thing and a second hand-written parser would drift from this one.
//!
//! ## What this used to be
//!
//! `expand [-t N] [FILE...]`, and the gaps were not all missing options:
//!
//! * **Input was decoded as UTF-8, a line at a time, via `BufRead::lines()`.**
//!   That corrupts three separate ways: a file that is not valid UTF-8 stopped
//!   at the first bad byte, a CRLF file silently lost its CRs (`lines()` strips
//!   them), and a final line with no newline *gained* one, because the output
//!   was written with `writeln!`. `expand` is a filter — a byte it was not
//!   asked to change must come out unchanged.
//! * **A bad `-t` argument was silently accepted.** `tab_width.parse()
//!   .unwrap_or(8)` turned `expand -t oops` into `expand -t 8`, so a typo
//!   produced plausible, wrong output and reported success.
//! * **`-t0` was defined to delete tabs**, guarding a division that GNU avoids
//!   by refusing the argument outright (`tab size cannot be 0`).
//! * **Every exit was `0`** — `main` returned `()` — and every write was
//!   `let _ = writeln!(…)`, so a full disk produced a truncated file and a
//!   success status. A missing file printed a diagnostic and also exited 0.
//! * **Only the separated `-t N` form was recognised.** `-t8`, `--tabs=8`,
//!   `-8`, `-i` and `--` were all read as *filenames*, which then did not exist
//!   — and, per the previous point, exited 0 anyway.
//!
//! ## Columns are bytes, not display cells
//!
//! `expand` counts one column per byte, and this is measured rather than
//! assumed: GNU 9.4 gives identical output under `LC_ALL=C` and
//! `LC_ALL=C.UTF-8` for a two-byte `é`, for a wide CJK character, and for a
//! combining accent. So `é\tx` puts the tab stop two columns along, not one.
//!
//! That looks wrong and is the compatible answer. It also falls out of the
//! only implementation that can never mangle its input: nothing here decodes,
//! so a file that is not valid UTF-8 passes through byte for byte.
//!
//! ## `-i` stops at the first non-blank, and a backspace is not blank
//!
//! Upstream's rule is one line — `convert &= convert_entire_line ||
//! isblank (c)` — with two consequences worth stating because neither is in
//! the manual:
//!
//! * It is evaluated on the **rewritten** character, and a converted tab has
//!   already become a space by then. So a run of leading tabs and spaces all
//!   converts, and only the first genuine non-blank switches conversion off.
//! * `\b` is not blank, so a backspace in the leading whitespace ends the
//!   conversion region even though nothing visible has been printed.
//!
//! ## Backspaces move the column back
//!
//! `\b` is passed through and decrements the column *and* the position in the
//! explicit stop list, so a tab after it is measured from where the cursor
//! actually is. Both decrements are clamped at zero (upstream's `column -=
//! !!column`), so a line that starts with backspaces does not wrap to a huge
//! column.
//!
//! ## Checked against GNU
//!
//! `scripts/expand-diff.sh` runs this and glibc's `expand` over the same
//! command lines and compares stdout, stderr and the exit status byte for
//! byte. The corners it pinned down:
//!
//! * **The operands are one character stream, not a sequence of files.** A
//!   file that does not end in a newline continues the same line into the next
//!   file: `expand a b` where `a` is `"x\t"` measures `b`'s first tab from the
//!   column `a` left it at. This is `while ((c = getc (fp)) < 0 && (fp =
//!   next_file (fp)))` upstream, and it is why the column state is not
//!   per-file.
//! * **An unterminated final line stays unterminated.** Nothing is appended.
//! * **A file that cannot be opened does not stop the run**; the rest are
//!   still converted and the status is 1 at the end.
//! * **The digit options are `getopt` options with *optional* arguments**
//!   (`"0::"`…`"9::"`), which has two visible effects: `expand -1,3` works,
//!   because upstream recovers the whole list with `parse_tab_stops (optarg -
//!   1)`; and, unlike `head -5`'s obsolete form, a digit option is **not**
//!   restricted to the first argument — `expand file -4` is accepted.
//! * **Each `-t` appends rather than replaces**, so `expand -t1 -t3` is
//!   `expand -t 1,3` — and `expand -t3 -t1` is an error, because ascending
//!   order is checked across the whole command line at the end.
//! * **A tab-stop diagnostic exits on the spot**, before the rest of the
//!   options are looked at: `expand -t x -q` reports the bad list and never
//!   mentions `-q`, while `expand -q -t x` reports only `-q`.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::quotef_os;
use coreutils::stdfd::{self, Stream};
use coreutils::tabstops::TabStops;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's
// `expand >&-` as the closed descriptor it is. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

const EXPAND: Program = Program::new("expand", 1);

const USAGE: &str = "usage: expand [-i] [-t LIST] [-LIST] [--] [FILE...]";

/// `\b`, which Rust has no escape for.
const BACKSPACE: u8 = 0x08;

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `expand --=x`, whose empty prefix matches every entry.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("tabs", Long::Tabs),
    ("initial", Long::Initial),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Tabs,
    Initial,
    Help,
    Version,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Convert these operands (`-` meaning standard input).
    Run(Settings, Vec<OsString>),
    Help,
    Version,
}

/// The two things the options actually decide.
#[derive(Debug, Default, PartialEq, Eq)]
struct Settings {
    tabs: TabStops,
    /// False under `-i`: stop converting after the line's first non-blank.
    entire_line: bool,
}

/// A command line that cannot be run.
///
/// The two variants are not interchangeable, because upstream produces them
/// from different code with different shapes. A getopt diagnostic ends in
/// `Try 'expand --help' for more information.` and carries the program's usage
/// status; a tab-stop diagnostic is `error (0, 0, …)` repeated up to twice and
/// then a bare `exit (EXIT_FAILURE)`, so it has no referral and is always 1.
#[derive(Debug)]
enum Refusal {
    Getopt(getopt::Error),
    Tabs(Vec<String>),
}

impl Refusal {
    fn report(&self) -> ExitCode {
        let status = match self {
            Self::Getopt(e) => {
                eprintln!("expand: {}", e.message());
                e.status
            }
            Self::Tabs(messages) => {
                for message in messages {
                    eprintln!("expand: {message}");
                }
                1
            }
        };
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(refusal) => return refusal.report(),
    };

    // `--help` and `--version` are writes like any other, so they fail like
    // any other: measured, `expand --help >&-` is
    // `expand: write error: Bad file descriptor` and exits 1.
    let mut out = Stream::stdout();
    let (settings, mut files) = match request {
        Request::Help => return say(out, format!("{USAGE}\n").as_bytes()),
        Request::Version => return say(out, b"expand (SlateOS coreutils)\n"),
        Request::Run(settings, files) => (settings, files),
    };

    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let mut input = Input::new(files);
    let mut expander = Expander::new(&settings.tabs, settings.entire_line);

    while let Some(byte) = input.next_byte() {
        if let Err(trouble) = expander.push(byte, &mut out) {
            return trouble.report();
        }
    }

    // Buffered output has to reach the OS before success can be claimed; a
    // flush that fails here is a truncated conversion reported as a complete
    // one. Upstream gets this from `atexit (close_stdout)`, which is also why
    // the failure surfaces here and not at the byte it happened on.
    let earned = if input.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    };
    stdfd::close_stdout("expand", out, earned)
}

/// A failure that ends the run rather than the file.
#[derive(Debug)]
enum Trouble {
    Write(io::Error),
    /// The column counter wrapped. Unreachable with 64-bit columns on any
    /// input that fits in memory, and ported anyway because the alternative is
    /// a silent wrap that mangles alignment rather than reporting it.
    TooLong,
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Write(e) => stdfd::write_error("expand", e),
            Self::TooLong => eprintln!("expand: input line is too long"),
        }
        ExitCode::FAILURE
    }
}

/// Say one thing and stop -- `--help` and `--version`.
///
/// The stream is closed here rather than left to the end of `main`, because
/// these two return without reaching it -- and closing it is what discovers
/// that there was nowhere to say it.
fn say(mut out: Stream, bytes: &[u8]) -> ExitCode {
    let _ = out.write_all(bytes);
    stdfd::close_stdout("expand", out, ExitCode::SUCCESS)
}

// ---------------------------------------------------------------- conversion

/// The running column state, which spans the whole command line rather than
/// one file, and resets at each newline.
struct Expander<'a> {
    tabs: &'a TabStops,
    entire_line: bool,
    /// Whether tabs on this line are still being converted; `-i` clears it at
    /// the first non-blank.
    convert: bool,
    column: u64,
    /// Position in the explicit stop list, rewound by a backspace in step with
    /// `column`.
    tab_index: usize,
}

impl<'a> Expander<'a> {
    fn new(tabs: &'a TabStops, entire_line: bool) -> Self {
        Self {
            tabs,
            entire_line,
            convert: true,
            column: 0,
            tab_index: 0,
        }
    }

    /// Feed one input byte and write its replacement.
    ///
    /// This is the whole of upstream's `expand()` inner loop, one character per
    /// call. The state it keeps between calls is what makes two files behave as
    /// one stream.
    fn push<W: Write>(&mut self, byte: u8, out: &mut W) -> Result<(), Trouble> {
        let mut c = byte;
        if self.convert {
            if c == b'\t' {
                // Past the last explicit stop with no `/` or `+` to continue
                // it, a tab is worth exactly one space.
                let next = match self.tabs.next_stop(self.column, &mut self.tab_index) {
                    Some(stop) => stop,
                    None => self.column.wrapping_add(1),
                };
                if next < self.column {
                    return Err(Trouble::TooLong);
                }
                // Upstream is `while (++column < next_tab_column) putchar (' ')`
                // — one space fewer than the gap, because the tab itself is
                // rewritten to the last of them just below.
                loop {
                    self.column = self.column.wrapping_add(1);
                    if self.column >= next {
                        break;
                    }
                    out.write_all(b" ").map_err(Trouble::Write)?;
                }
                c = b' ';
            } else if c == BACKSPACE {
                // Back one column, and force the next tab stop to be looked up
                // again from the rewound position.
                self.column = self.column.saturating_sub(1);
                self.tab_index = self.tab_index.saturating_sub(1);
            } else {
                self.column = self.column.wrapping_add(1);
                if self.column == 0 {
                    return Err(Trouble::TooLong);
                }
            }
            // Note this tests the *rewritten* character: a converted tab is a
            // space by now and so does not end `-i`'s region, while a backspace
            // — which is not blank — does.
            self.convert &= self.entire_line || c == b' ' || c == b'\t';
        }

        out.write_all(&[c]).map_err(Trouble::Write)?;
        if c == b'\n' {
            self.convert = true;
            self.column = 0;
            self.tab_index = 0;
        }
        Ok(())
    }
}

// --------------------------------------------------------------------- input

/// The operands as one byte stream.
///
/// Upstream's `while ((c = getc (fp)) < 0 && (fp = next_file (fp)))` makes the
/// file boundary invisible to the converter: a line that begins in one file and
/// ends in the next is one line, and its columns keep counting across the join.
struct Input {
    names: std::vec::IntoIter<OsString>,
    open: Option<Open>,
    /// Set by an operand that could not be opened or read; the run continues
    /// and the status is 1 at the end.
    failed: bool,
}

struct Open {
    name: OsString,
    reader: Box<dyn BufRead>,
}

/// What one attempt at a byte produced. Named rather than inlined so the
/// borrow of `open` ends before the stream is closed or replaced.
enum Step {
    Byte(u8),
    Eof,
    Retry,
    Failed(io::Error),
}

impl Input {
    fn new(names: Vec<OsString>) -> Self {
        Self {
            names: names.into_iter(),
            open: None,
            failed: false,
        }
    }

    /// Open operands until one succeeds; report and skip the ones that do not.
    fn advance(&mut self) -> bool {
        for name in self.names.by_ref() {
            let opened: io::Result<Box<dyn BufRead>> = if name == "-" {
                Ok(Box::new(BufReader::new(io::stdin())))
            } else {
                File::open(&name).map(|f| Box::new(BufReader::new(f)) as Box<dyn BufRead>)
            };
            match opened {
                Ok(reader) => {
                    self.open = Some(Open { name, reader });
                    return true;
                }
                Err(e) => {
                    eprintln!("expand: {}: {}", quotef_os(&name), strerror(&e));
                    self.failed = true;
                }
            }
        }
        false
    }

    fn next_byte(&mut self) -> Option<u8> {
        loop {
            if self.open.is_none() && !self.advance() {
                return None;
            }
            let step = {
                let open = self.open.as_mut()?;
                match open.reader.fill_buf() {
                    Ok(chunk) => match chunk.first().copied() {
                        Some(byte) => {
                            open.reader.consume(1);
                            Step::Byte(byte)
                        }
                        None => Step::Eof,
                    },
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => Step::Retry,
                    Err(e) => Step::Failed(e),
                }
            };
            match step {
                Step::Byte(byte) => return Some(byte),
                Step::Retry => {}
                Step::Eof => self.open = None,
                Step::Failed(e) => {
                    // Upstream notices this through `ferror` in `next_file`, so
                    // the file is named and the run moves on to the next one.
                    if let Some(open) = self.open.take() {
                        eprintln!("expand: {}: {}", quotef_os(&open.name), strerror(&e));
                    }
                    self.failed = true;
                }
            }
        }
    }
}

// ------------------------------------------------------------------- parsing

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut settings = Settings {
        tabs: TabStops::new(),
        entire_line: true,
    };
    let mut files: Vec<OsString> = Vec::new();
    let mut only_operands = false;
    let mut at = 0usize;

    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        if only_operands {
            files.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is standard input, which is an operand.
            files.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            if let Some(request) = long_option(body, &bytes, args, &mut at, &mut settings)? {
                return Ok(request);
            }
        } else {
            short_options(&bytes, args, &mut at, &mut settings)?;
        }
    }

    settings
        .tabs
        .finalize()
        .map_err(|message| Refusal::Tabs(vec![message]))?;

    Ok(Request::Run(settings, files))
}

/// One `--name`, `--name=value` or `--name value` argument.
///
/// `next` is the caller's position in `args`, advanced when `--tabs` has to
/// reach forward for a separated argument.
fn long_option(
    body: &[u8],
    whole: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<Option<Request>, Refusal> {
    // Split before resolving, so the *name* is what gets matched and the whole
    // argument is what gets echoed back when it resolves to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 matches none of
    // them and takes the unrecognised path, reported as the bytes typed.
    let typed = std::str::from_utf8(typed)
        .map_err(|_| Refusal::Getopt(EXPAND.unrecognized_option(whole)))?;
    let (name, which) = EXPAND
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    match which {
        Long::Tabs => {
            // A long option's *required* argument may be written either way:
            // `--tabs=8` or `--tabs 8`. (An *optional* one may not — it only
            // ever comes from the `=` form — but `expand` has none.) Only the
            // last argument on the line can leave it genuinely missing.
            let value = match inline {
                Some(value) => value.to_vec(),
                None => {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(EXPAND.long_missing_argument(name)));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                }
            };
            settings.tabs.parse(&value).map_err(Refusal::Tabs)?;
        }
        Long::Initial | Long::Help | Long::Version => {
            if inline.is_some() {
                return Err(Refusal::Getopt(EXPAND.long_unwanted_argument(name)));
            }
            match which {
                Long::Initial => settings.entire_line = false,
                Long::Help => return Ok(Some(Request::Help)),
                Long::Version => return Ok(Some(Request::Version)),
                Long::Tabs => {}
            }
        }
    }
    Ok(None)
}

/// One `-abc` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
///
/// `next` is the caller's position in `args`, advanced when `-t` has to reach
/// forward for a separated argument.
fn short_options(
    bytes: &[u8],
    args: &[OsString],
    next: &mut usize,
    settings: &mut Settings,
) -> Result<(), Refusal> {
    let cluster = bytes.get(1..).unwrap_or_default();
    let mut at = 0usize;
    while let Some(&c) = cluster.get(at) {
        match c {
            b'i' => settings.entire_line = false,
            b't' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(EXPAND.short_missing_argument(b't')));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                settings.tabs.parse(&value).map_err(Refusal::Tabs)?;
                return Ok(());
            }
            b'0'..=b'9' => {
                // The obsolete form. Upstream declares these as short options
                // with *optional* arguments and then recovers the list with
                // `parse_tab_stops (optarg - 1)` — pointer arithmetic back onto
                // the digit itself — so the value is this digit plus whatever
                // follows it in the same argument, and `-1,3` is a list.
                //
                // Optional arguments are never separated, which is why nothing
                // here reaches for the next argument: `expand -3 file` converts
                // `file` rather than looking for tab stops in its name.
                let value = cluster.get(at..).unwrap_or_default();
                settings.tabs.parse(value).map_err(Refusal::Tabs)?;
                return Ok(());
            }
            _ => return Err(Refusal::Getopt(EXPAND.invalid_option(c))),
        }
        at = at.saturating_add(1);
    }
    Ok(())
}

#[cfg(unix)]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    arg.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(arg: &OsString) -> Vec<u8> {
    arg.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Convert `input` under the given command line, ignoring operands.
    fn run(options: &[&str], input: &[u8]) -> Vec<u8> {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        let request = parse_args(&args).expect("command line should parse");
        let Request::Run(settings, _) = request else {
            panic!("expected a run");
        };
        let mut expander = Expander::new(&settings.tabs, settings.entire_line);
        let mut out: Vec<u8> = Vec::new();
        for &byte in input {
            expander.push(byte, &mut out).expect("no write can fail");
        }
        out
    }

    /// The diagnostics one command line produces, without the `expand: `
    /// prefix and without the referral.
    fn refuse(options: &[&str]) -> Vec<String> {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Ok(other) => panic!("expected a refusal, got {other:?}"),
            Err(Refusal::Getopt(e)) => vec![e.sentence],
            Err(Refusal::Tabs(messages)) => messages,
        }
    }

    #[test]
    fn the_default_is_a_stop_every_eight_columns() {
        assert_eq!(run(&[], b"a\tb\tc\n"), b"a       b       c\n".to_vec());
        assert_eq!(run(&[], b"12345678\tX\n"), b"12345678        X\n".to_vec());
    }

    #[test]
    fn the_five_spellings_of_a_tab_size_agree() {
        let expected = b"ab  c\n".to_vec();
        assert_eq!(run(&["-t4"], b"ab\tc\n"), expected);
        assert_eq!(run(&["-t", "4"], b"ab\tc\n"), expected);
        assert_eq!(run(&["--tabs=4"], b"ab\tc\n"), expected);
        // The separated long form. This was the one that was wrong: a long
        // option with a *required* argument takes the next word if there is no
        // `=`, exactly as the short form does, and only the last argument on
        // the line can leave it genuinely missing.
        assert_eq!(run(&["--tabs", "4"], b"ab\tc\n"), expected);
        assert_eq!(run(&["-4"], b"ab\tc\n"), expected);
    }

    #[test]
    fn a_separated_long_argument_is_consumed_rather_than_left_an_operand() {
        let args: Vec<OsString> = ["--tabs", "4", "file"].iter().map(OsString::from).collect();
        let Ok(Request::Run(_, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert_eq!(files, vec![OsString::from("file")]);
        // …and with nothing after it, it really is missing.
        assert_eq!(
            refuse(&["--tabs"]),
            vec!["option '--tabs' requires an argument".to_string()]
        );
    }

    #[test]
    fn an_obsolete_digit_option_carries_a_whole_list() {
        // `-1,3` is one option whose argument is recovered from the digit
        // onwards, so it is exactly `-t 1,3`.
        assert_eq!(run(&["-1,3"], b"a\tb\n"), run(&["-t", "1,3"], b"a\tb\n"));
        assert_eq!(run(&["-1,3"], b"a\tb\n"), b"a  b\n".to_vec());
    }

    #[test]
    fn explicit_stops_run_out_and_then_a_tab_is_one_space() {
        // Stops at 3 and 6; the third tab is past the end.
        assert_eq!(
            run(&["-t", "3,6"], b"a\tb\tc\td\n"),
            b"a  b  c d\n".to_vec()
        );
    }

    #[test]
    fn a_slash_continues_the_list_and_a_plus_continues_it_relatively() {
        // `/4` restarts at multiples of 4 after the stop at 2: 4, 8, 12.
        assert_eq!(
            run(&["-t", "2,/4"], b"\ta\tb\tc\n"),
            b"  a b   c\n".to_vec()
        );
        // `+4` counts from the last stop instead: 6, 10, 14.
        assert_eq!(
            run(&["-t", "2,+4"], b"\ta\tb\tc\n"),
            b"  a   b   c\n".to_vec()
        );
    }

    #[test]
    fn every_occurrence_of_the_option_appends() {
        assert_eq!(
            run(&["-t1", "-t3"], b"a\tb\n"),
            run(&["-t", "1,3"], b"a\tb\n")
        );
        // Which is why the reversed order is not merely redundant but an error.
        assert_eq!(refuse(&["-t3", "-t1"]), ["tab sizes must be ascending"]);
    }

    #[test]
    fn initial_stops_converting_at_the_first_nonblank() {
        // The leading tab converts; the one after `x` does not.
        assert_eq!(run(&["-i"], b"\tx\ty\n"), b"        x\ty\n".to_vec());
        // A space between two tabs keeps the region alive, because the test is
        // `isblank` on the rewritten character.
        assert_eq!(run(&["-i"], b" \t x\ty\n"), b"         x\ty\n".to_vec());
        // And it resets at every newline.
        assert_eq!(run(&["-i"], b"x\ty\n\tz\n"), b"x\ty\n        z\n".to_vec());
    }

    #[test]
    fn a_backspace_is_not_blank_so_it_ends_the_initial_region() {
        assert_eq!(run(&["-i"], b"\x08\tx\n"), b"\x08\tx\n".to_vec());
        // Without `-i` the same backspace merely rewinds the column.
        assert_eq!(run(&[], b"ab\x08\tc\n"), b"ab\x08       c\n".to_vec());
    }

    #[test]
    fn a_backspace_rewinds_the_position_in_the_explicit_list_too() {
        // Stops at 2 and 4. `abc` reaches column 3, the backspace puts it back
        // at 2, and the tab then runs to 4 — two spaces, not the one that the
        // un-rewound column 3 would have given. Measured against GNU.
        assert_eq!(
            run(&["-t", "2,4"], b"abc\x08\tx\n"),
            b"abc\x08  x\n".to_vec()
        );
    }

    #[test]
    fn nothing_is_added_to_a_line_that_had_no_newline() {
        assert_eq!(run(&[], b"a\tb"), b"a       b".to_vec());
    }

    #[test]
    fn input_is_bytes_and_a_column_is_a_byte() {
        // `é` is two bytes in UTF-8 and therefore two columns, which is what
        // GNU does under every locale. The bytes come out unchanged.
        assert_eq!(
            run(&[], "é\tx\n".as_bytes()),
            "é      x\n".as_bytes().to_vec()
        );
        // And a byte sequence that is not UTF-8 at all passes through.
        assert_eq!(run(&[], b"\xff\xfe\tx\n"), b"\xff\xfe      x\n".to_vec());
    }

    #[test]
    fn a_bad_tab_list_is_refused_rather_than_defaulted() {
        assert_eq!(
            refuse(&["-t", "oops"]),
            ["tab size contains invalid character(s): ‘oops’"]
        );
        assert_eq!(refuse(&["-t", "0"]), ["tab size cannot be 0"]);
        assert_eq!(
            refuse(&["-t", "8", "-t", "8"]),
            ["tab sizes must be ascending"]
        );
    }

    #[test]
    fn a_tab_list_can_report_twice_and_a_getopt_error_cannot() {
        // `parse_tab_stops` keeps going after a misplaced specifier.
        assert_eq!(
            refuse(&["-t", "1/2/3"]),
            [
                "'/' specifier not at start of number: ‘/2/3’",
                "'/' specifier not at start of number: ‘/3’",
            ]
        );
        // A getopt error stops the parse, so the second problem is unreported.
        assert_eq!(refuse(&["-q", "-z"]), ["invalid option -- 'q'"]);
    }

    #[test]
    fn a_tab_stop_diagnostic_preempts_the_options_after_it() {
        // Both of these are errors; which one is reported depends only on which
        // came first, because each exits where it is found.
        assert_eq!(
            refuse(&["-t", "x", "-q"]),
            ["tab size contains invalid character(s): ‘x’"]
        );
        assert_eq!(refuse(&["-q", "-t", "x"]), ["invalid option -- 'q'"]);
    }

    #[test]
    fn the_referral_belongs_to_the_getopt_errors_only() {
        let args: Vec<OsString> = ["-q"].iter().map(OsString::from).collect();
        let Err(Refusal::Getopt(e)) = parse_args(&args) else {
            panic!("expected a getopt refusal");
        };
        assert_eq!(e.referral, Some("expand"));
        assert_eq!(e.status, 1);
    }

    #[test]
    fn a_long_option_resolves_from_an_unambiguous_prefix() {
        assert_eq!(run(&["--in"], b"\tx\ty\n"), run(&["-i"], b"\tx\ty\n"));
        assert_eq!(run(&["--ta=4"], b"ab\tc\n"), b"ab  c\n".to_vec());
    }

    #[test]
    fn an_ambiguous_prefix_lists_the_table_in_declaration_order() {
        assert_eq!(
            refuse(&["--=x"]),
            [
                "option '--=x' is ambiguous; possibilities: '--tabs' '--initial' '--help' '--version'"
            ]
        );
    }

    #[test]
    fn a_long_option_is_missing_its_argument_only_when_nothing_follows() {
        // This test used to assert that `--tabs 4` *itself* was the missing
        // argument — the wrong belief that shipped a bug. A required argument
        // is missing only when there is no next word at all.
        assert_eq!(
            refuse(&["--tabs"]),
            ["option '--tabs' requires an argument"]
        );
        assert_eq!(
            refuse(&["-i", "--tabs"]),
            ["option '--tabs' requires an argument"]
        );
        // The converse still holds: an option that takes *no* argument rejects
        // one written with `=`.
        assert_eq!(
            refuse(&["--initial=4"]),
            ["option '--initial' doesn't allow an argument"]
        );
    }

    #[test]
    fn operands_and_options_may_be_interleaved() {
        let args: Vec<OsString> = ["a", "-i", "b", "--", "-c"]
            .iter()
            .map(OsString::from)
            .collect();
        let Ok(Request::Run(settings, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert!(!settings.entire_line);
        assert_eq!(files, ["a", "b", "-c"].map(OsString::from));
    }

    #[test]
    fn help_and_version_answer_immediately() {
        let args: Vec<OsString> = ["--help", "-t", "x"].iter().map(OsString::from).collect();
        assert_eq!(parse_args(&args).unwrap(), Request::Help);
        let args: Vec<OsString> = ["--version"].iter().map(OsString::from).collect();
        assert_eq!(parse_args(&args).unwrap(), Request::Version);
    }
}
