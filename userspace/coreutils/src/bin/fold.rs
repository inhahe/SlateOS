//! `fold` — wrap each input line to fit in a given width.
//!
//! ```text
//! fold [-bs] [-w WIDTH] [-WIDTH] [--] [FILE...]
//! ```
//!
//! | option | effect |
//! |---|---|
//! | `-b`, `--bytes` | count bytes rather than columns |
//! | `-s`, `--spaces` | break at a blank rather than mid-word, when there is one |
//! | `-w WIDTH`, `--width=WIDTH` | wrap at `WIDTH` instead of 80 |
//! | `-WIDTH` | the same width written without the `-w`, e.g. `fold -40` |
//!
//! ## What this used to be
//!
//! `fold [-w N] [-s] [FILE...]`, wrong in six ways a one-line command would
//! show:
//!
//! * **A column was a byte, always.** Without `-b` GNU counts *columns*: a tab
//!   advances to the next multiple of eight, a carriage return returns to zero,
//!   and a backspace goes back one. So `printf 'a\tb\n' | fold -w 4` breaks
//!   before the tab, which the old code could not do at any width — and `-b`,
//!   the option that asks for the byte counting it always did, did not exist.
//! * **Input was decoded as UTF-8, a line at a time, through `lines()`.** A
//!   file that is not valid UTF-8 stopped at the first bad byte, a CRLF file
//!   silently lost its CRs, and a final line with no newline *gained* one —
//!   three separate ways for a filter to damage what it was only asked to wrap.
//! * **`-w bogus` silently became `-w 80`.** `parse().unwrap_or(80)` turns a
//!   typo into plausible, wrong output and a success status.
//! * **`-w 0` was defined here as "don't wrap".** GNU refuses it: the floor is
//!   1, and the message is `invalid number of columns: '0': Numerical result
//!   out of range`.
//! * **Every exit was 0** — `main` returned `()`, every write was `let _ = …` —
//!   so a missing file, a read error and a full disk all reported success.
//! * **`-b`, `--width=N`, `-40`, `--` and every long option were read as
//!   *filenames***, which then did not exist, and (per the previous point)
//!   exited 0 anyway.
//!
//! ## The column model
//!
//! Upstream's `adjust_column` is four cases, and only the last is the obvious
//! one:
//!
//! | byte | next column |
//! |---|---|
//! | `\b` | one back, floored at 0 |
//! | `\r` | 0 |
//! | `\t` | up to the next multiple of 8 |
//! | anything else | one on |
//!
//! "Anything else" includes every non-printing byte and every byte of a
//! multi-byte character — the `isprint` test in upstream is commented out — so
//! `é` costs two columns under a UTF-8 locale just as it costs two bytes. The
//! tab width is a hard-coded 8; `fold` has no `-t`.
//!
//! Under `-b` all four collapse into "one on", which is what makes `-b` mean
//! something other than "the input is ASCII".
//!
//! ## `-s` and the rescan
//!
//! When a character would overflow the width, `-s` looks *backwards* through
//! what is already buffered for the last blank — a space or a tab — and breaks
//! after it. What follows the blank moves to the next line, its column is
//! recomputed from scratch, and the character that triggered all this is
//! measured again against the new column. That last step is upstream's `goto
//! rescan`, and it is load-bearing: the moved remainder may itself be wider
//! than the width, in which case the whole thing happens again, and eventually
//! falls through to a hard break once no blank is left.
//!
//! A line with no blank at all is folded hard, exactly as without `-s`, and a
//! single character wider than the whole width is emitted alone on its line
//! rather than looping forever.
//!
//! ## Each file is its own stream
//!
//! Unlike `expand`, `fold` resets its column and its buffer at every operand:
//! upstream's `column` and `offset_out` are locals of `fold_file`. So a file
//! that does not end in a newline has its last partial line flushed *without*
//! one before the next file starts at column zero — the join is visible, and
//! visible in the opposite way to `expand`'s.
//!
//! ## The width is a gnulib number
//!
//! `-w` goes through [`coreutils::xnum::xdectoumax`] with a floor of 1, a
//! ceiling of `SIZE_MAX - 9`, and *no* suffixes — `fold -w 1K` is an error
//! though `head -c 1K` is not, because the two pass different suffix lists to
//! the same function. Out of range produces one of two different `strerror`
//! sentences, chosen by the value rather than by the bound; see that module.
//!
//! Being an `error (EXIT_FAILURE, …)` inside the option loop, a bad width
//! **preempts every option after it**: `fold -w 0 -Z` reports the width and
//! never mentions `-Z`, while `fold -Z -w 0` reports only `-Z`.
//!
//! ## Checked against GNU
//!
//! `scripts/fold-diff.sh` runs this and glibc's `fold` over the same command
//! lines and compares stdout, stderr and the exit status byte for byte.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program};
use coreutils::quote::quotef_os;
use coreutils::xnum::xdectoumax;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

const FOLD: Program = Program::new("fold", 1);

const USAGE: &str = "usage: fold [-bs] [-w WIDTH] [-WIDTH] [--] [FILE...]";

/// `\b`, which Rust has no escape for.
const BACKSPACE: u8 = 0x08;

/// The tab width `adjust_column` assumes. Hard-coded upstream as `TAB_WIDTH`;
/// `fold` has no option to change it.
const TAB_WIDTH: u64 = 8;

/// Upstream's ceiling: `SIZE_MAX - TAB_WIDTH - 1`. The slack is what keeps the
/// column counter from wrapping — a tab may carry the column up to seven past
/// the width before the overflow is noticed.
const MAX_WIDTH: u64 = u64::MAX - TAB_WIDTH - 1;

/// The long options, **in GNU's declaration order** — which is observable,
/// because `getopt_long` lists an ambiguous prefix's candidates in it.
/// Measured with `fold --=x`, whose empty prefix matches every entry.
const LONG_OPTIONS: &[(&str, Long)] = &[
    ("bytes", Long::Bytes),
    ("spaces", Long::Spaces),
    ("width", Long::Width),
    ("help", Long::Help),
    ("version", Long::Version),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Long {
    Bytes,
    Spaces,
    Width,
    Help,
    Version,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Fold these operands (`-` meaning standard input).
    Run(Settings, Vec<OsString>),
    Help,
    Version,
}

/// The three things the options decide.
#[derive(Debug, PartialEq, Eq)]
struct Settings {
    width: u64,
    /// `-s`: prefer to break after a blank.
    break_spaces: bool,
    /// `-b`: one column per byte, ignoring tabs, returns and backspaces.
    count_bytes: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            width: 80,
            break_spaces: false,
            count_bytes: false,
        }
    }
}

/// A command line that cannot be run.
///
/// The two variants are shaped by the two different places upstream refuses
/// one. A getopt diagnostic ends in `Try 'fold --help' for more information.`;
/// a bad width comes from `xdectoumax`, which has no referral and exits on the
/// spot, so it can never be joined by a second message.
#[derive(Debug)]
enum Refusal {
    Getopt(getopt::Error),
    Width(String),
}

impl Refusal {
    fn report(&self) -> ExitCode {
        let status = match self {
            Self::Getopt(e) => {
                eprintln!("fold: {}", e.message());
                e.status
            }
            Self::Width(message) => {
                eprintln!("fold: {message}");
                1
            }
        };
        ExitCode::from(u8::try_from(status).unwrap_or(1))
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(refusal) => return refusal.report(),
    };

    let (settings, mut files) = match request {
        Request::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Request::Version => {
            println!("fold (SlateOS coreutils)");
            return ExitCode::SUCCESS;
        }
        Request::Run(settings, files) => (settings, files),
    };

    if files.is_empty() {
        files.push(OsString::from("-"));
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut ok = true;

    for name in &files {
        match fold_file(name, &settings, &mut out) {
            Ok(file_ok) => ok &= file_ok,
            Err(trouble) => return trouble.report(),
        }
    }

    // Buffered output has to reach the OS before success can be claimed; a
    // flush that fails here is a truncated file reported as a complete one.
    // Upstream gets this from `atexit (close_stdout)`.
    if let Err(e) = out.flush() {
        return Trouble::Write(e).report();
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A failure that ends the run rather than the file.
#[derive(Debug)]
enum Trouble {
    Write(io::Error),
}

impl Trouble {
    fn report(&self) -> ExitCode {
        match self {
            Self::Write(e) => eprintln!("fold: write error: {}", strerror(e)),
        }
        ExitCode::FAILURE
    }
}

// ----------------------------------------------------------------- conversion

/// Where printing `c` from `column` leaves the cursor.
///
/// Upstream's `adjust_column`, including the branch it comments out: every byte
/// that is not one of the three control characters counts one column, printable
/// or not.
fn adjust_column(column: u64, c: u8, count_bytes: bool) -> u64 {
    if count_bytes {
        return column.saturating_add(1);
    }
    match c {
        BACKSPACE => column.saturating_sub(1),
        b'\r' => 0,
        b'\t' => column.saturating_add(TAB_WIDTH.saturating_sub(column % TAB_WIDTH)),
        _ => column.saturating_add(1),
    }
}

/// One file's worth of folding state.
///
/// Per-file rather than per-run because upstream's are locals of `fold_file` —
/// see the module docs.
struct Folder<'a> {
    settings: &'a Settings,
    /// The column the next character will occupy.
    column: u64,
    /// The current output line, not yet terminated. Upstream's `line_out`.
    line: Vec<u8>,
}

impl<'a> Folder<'a> {
    fn new(settings: &'a Settings) -> Self {
        Self {
            settings,
            column: 0,
            line: Vec::new(),
        }
    }

    /// Feed one input byte.
    ///
    /// This is the whole of upstream's `fold_file` inner loop, one character
    /// per call, with `goto rescan` written as the loop it is.
    fn push<W: Write>(&mut self, c: u8, out: &mut W) -> Result<(), Trouble> {
        if c == b'\n' {
            self.line.push(c);
            out.write_all(&self.line).map_err(Trouble::Write)?;
            self.line.clear();
            self.column = 0;
            return Ok(());
        }

        loop {
            self.column = adjust_column(self.column, c, self.settings.count_bytes);
            if self.column <= self.settings.width {
                break;
            }

            // Too wide. `-s` gets first refusal: break after the last blank
            // already buffered, if there is one.
            if self.settings.break_spaces
                && let Some(blank) = self.line.iter().rposition(|&b| b == b' ' || b == b'\t')
            {
                let end = blank.saturating_add(1);
                out.write_all(self.line.get(..end).unwrap_or_default())
                    .map_err(Trouble::Write)?;
                out.write_all(b"\n").map_err(Trouble::Write)?;
                self.line.drain(..end);
                // The remainder starts the next line, so its width is measured
                // again from zero — not as `column` minus something, because a
                // tab inside it lands on different stops now.
                let mut column = 0u64;
                for &b in &self.line {
                    column = adjust_column(column, b, self.settings.count_bytes);
                }
                self.column = column;
                continue;
            }

            if self.line.is_empty() {
                // A single character wider than the whole width: emit it alone
                // rather than looping. The column is deliberately *not* reset,
                // so the next character is measured from where this one left
                // the cursor.
                self.line.push(c);
                return Ok(());
            }

            self.line.push(b'\n');
            out.write_all(&self.line).map_err(Trouble::Write)?;
            self.line.clear();
            self.column = 0;
        }

        self.line.push(c);
        Ok(())
    }

    /// Flush a final line that had no newline of its own — without adding one.
    fn finish<W: Write>(&mut self, out: &mut W) -> Result<(), Trouble> {
        if !self.line.is_empty() {
            out.write_all(&self.line).map_err(Trouble::Write)?;
            self.line.clear();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------- input

/// Fold one operand, returning whether it succeeded.
///
/// An operand that cannot be opened or read is reported and skipped; only a
/// *write* failure ends the run, because there is nowhere left to put the rest.
fn fold_file<W: Write>(name: &OsString, settings: &Settings, out: &mut W) -> Result<bool, Trouble> {
    let opened: io::Result<Box<dyn BufRead>> = if name == "-" {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        File::open(name).map(|f| Box::new(BufReader::new(f)) as Box<dyn BufRead>)
    };
    let mut reader = match opened {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("fold: {}: {}", quotef_os(name), strerror(&e));
            return Ok(false);
        }
    };

    let mut folder = Folder::new(settings);
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                // Upstream notices this through `ferror` *after* the loop, so
                // whatever was already folded is still written and only then is
                // the file named.
                folder.finish(out)?;
                eprintln!("fold: {}: {}", quotef_os(name), strerror(&e));
                return Ok(false);
            }
        };
        if chunk.is_empty() {
            break;
        }
        // Copied because `push` writes through `out` while the reader's buffer
        // is borrowed; a chunk is bounded by the reader's capacity.
        let bytes = chunk.to_vec();
        reader.consume(bytes.len());
        for &byte in &bytes {
            folder.push(byte, out)?;
        }
    }

    folder.finish(out)?;
    Ok(true)
}

// -------------------------------------------------------------------- parsing

fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut settings = Settings::default();
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

    Ok(Request::Run(settings, files))
}

/// `-w`'s argument, or an obsolete digit cluster's.
fn parse_width(value: &[u8]) -> Result<u64, Refusal> {
    // `""` rather than a suffix list: `fold -w 1K` is an error. The floor of 1
    // is what makes `-w 0` "Numerical result out of range" rather than a
    // silent do-nothing.
    xdectoumax(value, 1, MAX_WIDTH, Some(b""), "invalid number of columns").map_err(Refusal::Width)
}

/// One `--name`, `--name=value` or `--name value` argument.
///
/// `next` is the caller's position in `args`, advanced when `--width` has to
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
    let typed =
        std::str::from_utf8(typed).map_err(|_| Refusal::Getopt(FOLD.unrecognized_option(whole)))?;
    let (name, which) = FOLD
        .resolve_long(typed, whole, LONG_OPTIONS)
        .map_err(Refusal::Getopt)?;

    match which {
        Long::Width => {
            // A required argument may be written either way — `--width=5` or
            // `--width 5` — so only the last argument on the line can leave it
            // genuinely missing.
            let value = match inline {
                Some(value) => value.to_vec(),
                None => {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(FOLD.long_missing_argument(name)));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                }
            };
            settings.width = parse_width(&value)?;
        }
        Long::Bytes | Long::Spaces | Long::Help | Long::Version => {
            if inline.is_some() {
                return Err(Refusal::Getopt(FOLD.long_unwanted_argument(name)));
            }
            match which {
                Long::Bytes => settings.count_bytes = true,
                Long::Spaces => settings.break_spaces = true,
                Long::Help => return Ok(Some(Request::Help)),
                Long::Version => return Ok(Some(Request::Version)),
                Long::Width => {}
            }
        }
    }
    Ok(None)
}

/// One `-abc` cluster.
///
/// Bytes, not `char`s: `-é` is two bytes, and iterating `char`s would report
/// `invalid option -- 'é'`, an option nobody typed.
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
            b'b' => settings.count_bytes = true,
            b's' => settings.break_spaces = true,
            b'w' => {
                // A *required* argument: the rest of the cluster if there is
                // one, otherwise the whole of the next argument.
                let rest = cluster.get(at.saturating_add(1)..).unwrap_or_default();
                let value = if rest.is_empty() {
                    let Some(separate) = args.get(*next) else {
                        return Err(Refusal::Getopt(FOLD.short_missing_argument(b'w')));
                    };
                    *next = next.saturating_add(1);
                    arg_bytes(separate)
                } else {
                    rest.to_vec()
                };
                settings.width = parse_width(&value)?;
                return Ok(());
            }
            b'0'..=b'9' => {
                // The obsolete form. Upstream declares these as short options
                // with *optional* arguments and recovers the digit with
                // `optarg--`, so the value is this digit plus whatever follows
                // it in the same argument — `fold -12` is a width of twelve.
                //
                // Unlike `unexpand`'s digits, which accumulate across the whole
                // command line, each of these *replaces* the width: `fold -1
                // -2` wraps at two. And unlike `head -5`'s obsolete form, a
                // digit option is not restricted to the first argument.
                let value = cluster.get(at..).unwrap_or_default();
                settings.width = parse_width(value)?;
                return Ok(());
            }
            _ => return Err(Refusal::Getopt(FOLD.invalid_option(c))),
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

    /// Fold `input` under the given command line, ignoring operands.
    fn run(options: &[&str], input: &[u8]) -> Vec<u8> {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        let request = parse_args(&args).expect("command line should parse");
        let Request::Run(settings, _) = request else {
            panic!("expected a run");
        };
        let mut folder = Folder::new(&settings);
        let mut out: Vec<u8> = Vec::new();
        for &byte in input {
            folder.push(byte, &mut out).expect("no write can fail");
        }
        folder.finish(&mut out).expect("no write can fail");
        out
    }

    fn text(options: &[&str], input: &str) -> String {
        String::from_utf8(run(options, input.as_bytes())).expect("ascii in, ascii out")
    }

    /// The diagnostic one command line produces, without the `fold: ` prefix
    /// and without the referral.
    fn refuse(options: &[&str]) -> String {
        let args: Vec<OsString> = options.iter().map(OsString::from).collect();
        match parse_args(&args) {
            Ok(other) => panic!("expected a refusal, got {other:?}"),
            Err(Refusal::Getopt(e)) => e.sentence,
            Err(Refusal::Width(message)) => message,
        }
    }

    #[test]
    fn the_default_width_is_eighty() {
        let long = "x".repeat(200);
        let folded = text(&[], &format!("{long}\n"));
        let lines: Vec<&str> = folded.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), 80);
        assert_eq!(lines[1].len(), 80);
        assert_eq!(lines[2].len(), 40);
    }

    #[test]
    fn a_short_line_is_untouched() {
        assert_eq!(text(&["-w", "5"], "abc\n"), "abc\n");
        assert_eq!(text(&["-w", "3"], "abc\n"), "abc\n");
    }

    #[test]
    fn a_hard_break_happens_at_the_width() {
        assert_eq!(text(&["-w", "5"], "abcdefghij\n"), "abcde\nfghij\n");
        assert_eq!(text(&["-w", "1"], "abc\n"), "a\nb\nc\n");
    }

    #[test]
    fn an_unterminated_final_line_stays_unterminated() {
        assert_eq!(text(&["-w", "3"], "abcdef"), "abc\ndef");
        assert_eq!(text(&["-w", "3"], "ab"), "ab");
    }

    #[test]
    fn a_tab_advances_to_the_next_multiple_of_eight() {
        // The tab takes the column from 1 to 8, which is past a width of 4, so
        // the break lands before it — and then the tab is *still* past the
        // width on a line of its own, which is the case that only the first
        // clause of upstream's `if (offset_out == 0)` covers: the character is
        // emitted alone and the column is deliberately *not* reset, so the
        // following `b` is at column 9 and breaks again. Measured, because the
        // obvious guess ("a\n\tb\n") is wrong.
        assert_eq!(text(&["-w", "4"], "a\tb\n"), "a\n\t\nb\n");
        // At width 8 the tab exactly reaches the stop and stays — but `b` is
        // then column 9 and starts a new line.
        assert_eq!(text(&["-w", "8"], "a\tb\n"), "a\t\nb\n");
    }

    #[test]
    fn dash_b_counts_the_tab_as_one_byte() {
        assert_eq!(text(&["-b", "-w", "4"], "a\tb\n"), "a\tb\n");
        // The difference is exactly the column model: three bytes fit where
        // three columns do not.
        assert_eq!(text(&["-b", "-w", "3"], "a\tbcd\n"), "a\tb\ncd\n");
    }

    #[test]
    fn a_carriage_return_returns_to_column_zero() {
        assert_eq!(text(&["-w", "3"], "abc\rdef\n"), "abc\rdef\n");
        // Under `-b` it is just another byte, so the same line folds.
        assert_eq!(text(&["-b", "-w", "3"], "abc\rdef\n"), "abc\n\rde\nf\n");
    }

    #[test]
    fn a_backspace_moves_the_column_back() {
        assert_eq!(text(&["-w", "3"], "ab\u{8}cd\n"), "ab\u{8}cd\n");
        assert_eq!(text(&["-b", "-w", "3"], "ab\u{8}cd\n"), "ab\u{8}\ncd\n");
    }

    #[test]
    fn dash_s_breaks_after_the_last_blank() {
        assert_eq!(
            text(&["-s", "-w", "8"], "hello world!\n"),
            "hello \nworld!\n"
        );
        assert_eq!(
            text(&["-s", "-w", "10"], "the quick brown fox\n"),
            "the quick \nbrown fox\n"
        );
    }

    #[test]
    fn dash_s_falls_back_to_a_hard_break_when_there_is_no_blank() {
        assert_eq!(
            text(&["-s", "-w", "5"], "supercalifragilistic\n"),
            text(&["-w", "5"], "supercalifragilistic\n")
        );
    }

    #[test]
    fn dash_s_rescans_a_remainder_that_is_still_too_wide() {
        // The break after "ab " leaves "cdefghij", itself too wide and with no
        // blank in it, so that folds hard.
        assert_eq!(
            text(&["-s", "-w", "5"], "ab cdefghij\n"),
            "ab \ncdefg\nhij\n"
        );
    }

    #[test]
    fn a_blank_at_the_start_is_still_a_break_point() {
        // The blank is at index 0, so the first output line is just the blank.
        // Upstream does not special-case it — a `logical_end` of 0 still counts
        // as found.
        assert_eq!(text(&["-s", "-w", "5"], " abcdefghi\n"), " \nabcde\nfghi\n");
    }

    #[test]
    fn a_character_wider_than_the_width_gets_a_line_of_its_own() {
        // A tab at width 1 cannot fit anywhere, and must not loop.
        assert_eq!(text(&["-w", "1"], "\tx\n"), "\t\nx\n");
    }

    #[test]
    fn an_empty_line_stays_empty() {
        assert_eq!(text(&["-w", "1"], "\n\n"), "\n\n");
    }

    #[test]
    fn every_spelling_of_a_width_agrees() {
        let expected = "abcd\nefgh\n";
        for options in [
            vec!["-w4"],
            vec!["-w", "4"],
            vec!["--width=4"],
            vec!["--width", "4"],
            vec!["-4"],
        ] {
            assert_eq!(text(&options, "abcdefgh\n"), expected, "for {options:?}");
        }
    }

    #[test]
    fn an_obsolete_digit_run_is_one_number_and_the_last_one_wins() {
        // `-12` is twelve, not one and then two…
        assert_eq!(text(&["-12"], "abcdefghijklmn\n"), "abcdefghijkl\nmn\n");
        // …and unlike `unexpand`'s accumulating digits, a second option
        // replaces the first rather than extending it: `-1 -2` is two.
        assert_eq!(text(&["-1", "-2"], "abcdef\n"), "ab\ncd\nef\n");
        // A digit option is not restricted to the first argument.
        let args: Vec<OsString> = ["file", "-2"].iter().map(OsString::from).collect();
        let Ok(Request::Run(settings, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert_eq!(settings.width, 2);
        assert_eq!(files, vec![OsString::from("file")]);
    }

    #[test]
    fn a_digit_cluster_that_is_not_a_number_is_reported_whole() {
        // The digit's argument is recovered from the digit onwards, so the
        // whole cluster is what gets quoted back.
        assert_eq!(refuse(&["-1,2"]), "invalid number of columns: '1,2'");
        assert_eq!(refuse(&["-1x"]), "invalid number of columns: '1x'");
        // …and a digit after another option still starts its own number.
        assert_eq!(text(&["-s2"], "abcd\n"), "ab\ncd\n");
    }

    #[test]
    fn the_width_diagnostics() {
        assert_eq!(
            refuse(&["-w", "0"]),
            "invalid number of columns: '0': Numerical result out of range"
        );
        assert_eq!(
            refuse(&["-w", "bogus"]),
            "invalid number of columns: 'bogus'"
        );
        assert_eq!(refuse(&["-w", "1K"]), "invalid number of columns: '1K'");
        assert_eq!(refuse(&["-w", "-3"]), "invalid number of columns: '-3'");
        assert_eq!(
            refuse(&["-w", "18446744073709551607"]),
            "invalid number of columns: '18446744073709551607': \
             Value too large for defined data type"
        );
    }

    #[test]
    fn a_bad_width_preempts_every_option_after_it() {
        // Upstream's `xdectoumax` exits from inside the option loop, so `-Z` is
        // never reached — while the other order reports `-Z`.
        assert_eq!(
            refuse(&["-w", "0", "-Z"]),
            "invalid number of columns: '0': Numerical result out of range"
        );
        assert_eq!(refuse(&["-Z", "-w", "0"]), "invalid option -- 'Z'");
    }

    #[test]
    fn the_getopt_diagnostics() {
        assert_eq!(refuse(&["-Z"]), "invalid option -- 'Z'");
        assert_eq!(refuse(&["--nope"]), "unrecognized option '--nope'");
        assert_eq!(refuse(&["-w"]), "option requires an argument -- 'w'");
        assert_eq!(
            refuse(&["--width"]),
            "option '--width' requires an argument"
        );
        assert_eq!(
            refuse(&["--bytes=x"]),
            "option '--bytes' doesn't allow an argument"
        );
    }

    #[test]
    fn long_options_abbreviate_and_report_ambiguity_in_declaration_order() {
        assert_eq!(text(&["--wid=4"], "abcdefgh\n"), "abcd\nefgh\n");
        assert_eq!(
            refuse(&["--=x"]),
            "option '--=x' is ambiguous; possibilities: '--bytes' '--spaces' '--width' \
             '--help' '--version'"
        );
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        let args: Vec<OsString> = ["--", "-w"].iter().map(OsString::from).collect();
        let Ok(Request::Run(settings, files)) = parse_args(&args) else {
            panic!("expected a run");
        };
        assert_eq!(settings.width, 80);
        assert_eq!(files, vec![OsString::from("-w")]);
    }

    #[test]
    fn bytes_that_are_not_text_pass_through_untouched() {
        assert_eq!(
            run(&["-w", "4"], b"\xff\xfe\xfd\xfc\xfb\n"),
            b"\xff\xfe\xfd\xfc\n\xfb\n".to_vec()
        );
    }
}
