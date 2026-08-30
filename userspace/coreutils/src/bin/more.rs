//! more — file perusal filter for viewing text one screen at a time.
//!
//! Usage: more [FILE...]
//!   Displays text one screen at a time.
//!   Press Enter for next line, Space for next page, q to quit.
//!   Without files, reads from stdin.
//!
//! # A pager may not decide what its input means
//!
//! Everything here is bytes: the file names from argv, and the file contents
//! on the way to stdout. That is not stylistic. This program used to read its
//! input with [`BufRead::lines`], which yields `String` and therefore *fails*
//! on a line that is not valid UTF-8 — and the failure was handled with
//! `Err(_) => break`, so `more` on a file holding one stray byte printed the
//! lines before it, stopped, and **exited 0**. A pager that silently shows you
//! part of a file is worse than one that refuses: nothing on screen says the
//! rest exists. Reconstructing each line into a `String` and printing it with
//! `writeln!` also appended a newline the file did not have.
//!
//! The loop is now `read_until(b'\n')` and `write_all`, which copies the file's
//! bytes through unexamined and reproduces a missing final newline.
//!
//! # When it pages, and when it labels
//!
//! Both of those are decided by whether a *terminal* is on the other end, and
//! this program used to decide neither — it paged unconditionally and labelled
//! on operand count alone. Both rules below were measured against util-linux
//! `more` 2.39.3 over the full `{1,2 files} × {stdin tty, pipe} × {stdout tty,
//! pipe}` matrix; each is the only rule that fits every cell.
//!
//! - **Page only when stdout is a terminal.** `more big.txt | cat` is a copy,
//!   not a conversation. Paging into a pipe was not merely cosmetic here: the
//!   keystroke read hit EOF immediately, `read_key` mapped that to `Quit`, and
//!   the pipeline received *one screen* of a file the user asked for all of —
//!   silently, with status 0. That is the same class of bug as the UTF-8 one
//!   above, arrived at from the other direction.
//! - **Print the `::::` banner when there is more than one operand, or when
//!   stdin is not a terminal.** The second half is what makes `more f > out`
//!   label its output while `more f` on a terminal does not, and it is keyed
//!   on stdin because that is what tells `more` nobody is going to answer a
//!   prompt.
//!
//! See `known-issues.md` → `B-more-STOPPED-PAGING-AT-THE-FIRST-NON-UTF8-BYTE`.
//!
//! # The options describe a screen, so with no screen they do nothing
//!
//! This program used to have no option parsing at all: `more --help` opened a
//! file called `--help`, `more -5 f` opened one called `-5`, and `more +20 f`
//! opened one called `+20`. The whole util-linux 2.39.3 set is now accepted.
//!
//! What each does was measured rather than assumed, and the single governing
//! fact is this: **with stdout on a pipe, util-linux `more` ignores every
//! display option and copies the file verbatim.** `more -n 3 f | wc -l`,
//! `more +5 f | wc -l`, `more +/17 f | wc -l` and `more -s f | cat -A` are all
//! byte-identical to `more f` (measured, 2026-08-30) — `-s` does not squeeze,
//! `+5` does not skip. That is not an oversight: `-n` sets a screenful, `+5`
//! sets what is at the top of the first screen, `-s` saves screen space. With
//! no screen there is nothing for any of them to mean. So every option below is
//! consulted only on the paging path, which is the path taken exactly when
//! stdout is a terminal — the same condition [`command_source`] already used.
//!
//! Two of them are accepted and then change nothing, and it is worth saying
//! why each is *already* what we do rather than leaving a reader to wonder:
//!
//! * **`-e`, `--exit-on-eof`** — we have always returned at end of file rather
//!   than prompting there, so this is the standing behaviour. Measured:
//!   util-linux's own output for a short file is the same with and without it.
//! * **`-u`, `--plain`** — suppresses the backspace-overstrike underlining
//!   (`_\bx`) that util-linux renders. We never interpret those sequences; the
//!   bytes go through as they are, which is what `-u` asks for.
//!
//! **`-f`, `--logical`** is the *inverse* case: it asks for logical lines, and
//! until now that was all we could count. The default is now screen rows — a
//! line wider than the terminal costs more than one — computed with
//! [`charwidth`], the same `wcwidth` the rest of the tree lays text out with.
//! `-f` restores the old counting.
//!
//! # The prompt is not a constant
//!
//! `--More--` was, here. Measured, util-linux's is in reverse video, carries
//! `(NN%)` when the input has a length to be a fraction of — `more f` reports
//! a percentage and `more < f` does not — and appends `-d`'s hint *inside* the
//! highlighted run. The percentage counts bytes taken from the file including
//! any `+NUM` skipped past, which is why `more +5 f` opens at 19% and not 9%.
//! It is erased with the terminal's clear-to-end-of-line rather than by
//! overwriting it with spaces, because it no longer has a fixed width.
//!
//! # Two places this deliberately does not match util-linux
//!
//! Both are its `-NUM` pre-scan not knowing what the rest of its parser knows,
//! and in both the reference loses information the user supplied:
//!
//! | typed | util-linux | here |
//! |---|---|---|
//! | `more -n -3 f` | `argument error: 'f'` — `-3` was lifted out as a height, so `-n` ate the file name | `argument error: '-3'`, naming what was actually wrong |
//! | `more -- -5 f` | pages `f` with a 5-line screen; `--` is not honoured | `-5` is a file name, as `--` promised |
//!
//! Both still fail, or not, in the same way as the reference — the statuses
//! agree — so nothing that scripts `more` can tell the difference. What differs
//! is which token an error names, and whether `--` means what it says.
//!
//! The interactive command set is *not* implemented beyond space, Enter and
//! `q`: util-linux also takes `h`, `/`, `n`, `b`, `=`, `:f`, `!`, `v` and digit
//! prefixes. See `known-issues.md`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf, quotef};
use ere::bre;
use std::collections::VecDeque;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::num::IntErrorKind;
use std::process::ExitCode;

/// The banner printed above each file's name. Fourteen colons, measured
/// against util-linux `more` 2.39.3; it was thirteen here.
const BANNER: &[u8] = b"::::::::::::::";

/// util-linux exits 1 on a bad option — measured for `--zzz`, `--s`,
/// `-n abc` and `--lines=abc`, all of which print to stderr and exit 1.
const USAGE_STATUS: i32 = 1;

const MORE: Program = Program::new("more", USAGE_STATUS);

const SHORTS: &str = "dflcpesun:hV";

const LONGS: &[(&str, Takes)] = &[
    ("silent", Takes::Nothing),
    ("logical", Takes::Nothing),
    ("no-pause", Takes::Nothing),
    ("print-over", Takes::Nothing),
    ("clean-print", Takes::Nothing),
    ("exit-on-eof", Takes::Nothing),
    ("squeeze", Takes::Nothing),
    ("plain", Takes::Nothing),
    ("lines", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

const HELP: &[u8] = b"\
Usage: more [OPTION]... [FILE]...
Display the contents of a file a screen at a time.

  -d, --silent          display help instead of ringing bell
  -f, --logical         count logical rather than screen lines
  -l, --no-pause        suppress pause after form feed
  -c, --print-over      do not scroll, display text and clean line ends
  -p, --clean-print     do not scroll, clean screen and display text
  -e, --exit-on-eof     exit on end-of-file
  -s, --squeeze         squeeze multiple blank lines into one
  -u, --plain           suppress underlining and bold
  -n, --lines <number>  the number of lines per screenful
  -<number>             same as --lines
  +<number>             display file beginning from line number
  +/<pattern>           display file beginning from pattern match
  -h, --help            display this help
  -V, --version         display version

With no FILE, or when FILE is -, read standard input.
";

const VERSION: &[u8] = b"more (SlateOS coreutils) 0.1.0\n";

/// How many lines above a `+/pattern` match are shown with it.
///
/// Two, measured: `more -n 5 +/17 n.txt` on a terminal shows 15 through 19,
/// not 17 through 21. A match with no context around it is a worse answer to
/// "show me where this is" than one line of scrollback would have been.
const CONTEXT_LINES: usize = 2;

/// Where the first screen starts, from `+<number>` or `+/<pattern>`.
#[derive(Debug)]
enum Start {
    Top,
    /// 1-based line number; the named line is the first one shown.
    Line(usize),
    /// The first line matching goes near the top of the screen.
    ///
    /// Held as the pattern's **bytes**, compiled later in [`seek_start`], and
    /// that delay is measured rather than incidental: `more '+/[' f | cat`
    /// prints the whole file and exits 0, with no complaint about the unclosed
    /// bracket, because with stdout on a pipe util-linux never looks at the
    /// pattern at all. Compiling here would turn that into an error.
    ///
    /// Basic regular expressions, because util-linux compiles this with
    /// `regcomp` and no `REG_EXTENDED` — `more '+/a\+b'` searches for a literal
    /// `+`, not for a repeat.
    Pattern(Vec<u8>),
}

/// Everything the command line says, already validated.
#[derive(Debug)]
struct Options {
    /// `None` means "ask the terminal", i.e. the `LINES` fallback.
    lines: Option<usize>,
    squeeze: bool,
    silent: bool,
    logical: bool,
    no_pause: bool,
    print_over: bool,
    clean_print: bool,
    start: Start,
    files: Vec<OsString>,
}

impl Options {
    fn new() -> Self {
        Self {
            lines: None,
            squeeze: false,
            silent: false,
            logical: false,
            no_pause: false,
            print_over: false,
            clean_print: false,
            start: Start::Top,
            files: Vec::new(),
        }
    }
}

/// What `parse_args` decided to do, since two of the three do not page.
#[derive(Debug)]
enum Outcome {
    Run(Box<Options>),
    Help,
    Version,
}

/// Read the command line.
///
/// `Err` carries the whole diagnostic, already worded, because the two sources
/// of one word it differently: `getopt` supplies glibc's sentence *and* the
/// `Try 'more --help'` line, while util-linux's own `more: argument error:
/// 'abc'` (measured) has neither.
fn parse_args(args: &[OsString]) -> Result<Outcome, String> {
    let mut opts = Options::new();

    // Two spellings have to come out of argv before `getopt` sees it, because
    // `getopt` cannot represent either. `-5` would be read as the five option
    // letters `-5`, and `+5` would be handed back as an operand — which is
    // *right* for `+abc` (measured: `more: cannot open +abc: No such file or
    // directory`) and wrong for `+5`. Scanning first, and stopping at `--`
    // exactly as `getopt` does, keeps the two rules in agreement about which
    // words are still options.
    let mut rest: Vec<OsString> = Vec::with_capacity(args.len());
    let mut only_operands = false;
    let mut at = 0usize;
    while let Some(arg) = args.get(at) {
        at = at.saturating_add(1);
        let bytes = os_bytes(arg);
        let word: &[u8] = bytes.as_ref();
        if only_operands {
            rest.push(arg.clone());
            continue;
        }
        if word == b"--" {
            only_operands = true;
            rest.push(arg.clone());
            continue;
        }
        // A value in the *next* word has to be stepped over here, or it is
        // read twice and differently. `more -n -3 f` gives `-3` to `-n`; a
        // scan that did not know that would take `-3` for the `-NUM` spelling,
        // leaving `getopt` to hand `-n` the file name and the file to nobody.
        if takes_next_word(word) {
            rest.push(arg.clone());
            if let Some(value) = args.get(at) {
                rest.push(value.clone());
                at = at.saturating_add(1);
            }
            continue;
        }
        if let Some(tail) = word.strip_prefix(b"+") {
            if let Some(pattern) = tail.strip_prefix(b"/") {
                opts.start = Start::Pattern(pattern.to_vec());
                continue;
            }
            if !tail.is_empty() && tail.iter().all(u8::is_ascii_digit) {
                // `+0` and `+1` both mean the top; util-linux accepts `+0`.
                opts.start = Start::Line(parse_count(tail)?.max(1));
                continue;
            }
            rest.push(arg.clone());
            continue;
        }
        // `-` alone is stdin, not an empty run of digits, so the emptiness
        // check is what keeps this from swallowing it.
        if let Some(tail) = word.strip_prefix(b"-")
            && !tail.is_empty()
            && tail.iter().all(u8::is_ascii_digit)
        {
            opts.lines = Some(parse_count(tail)?.max(1));
            continue;
        }
        rest.push(arg.clone());
    }

    for item in MORE.parse(&rest, SHORTS, LONGS) {
        // `message`, not `sentence`: the referral is a second line that
        // `getopt` keeps separate because `nl` prints several sentences under
        // one referral. `more` stops at the first error, so it wants both. The
        // status is not carried along because it cannot vary — `MORE`'s usage
        // status is 1, and the one rule that overrides it (a bad argument to an
        // option) is 1 as well.
        match item.map_err(|why| format!("more: {}", why.message()))? {
            Opt::Short(b'd', _) | Opt::Long("silent", _) => opts.silent = true,
            Opt::Short(b'f', _) | Opt::Long("logical", _) => opts.logical = true,
            Opt::Short(b'l', _) | Opt::Long("no-pause", _) => opts.no_pause = true,
            Opt::Short(b'c', _) | Opt::Long("print-over", _) => opts.print_over = true,
            Opt::Short(b'p', _) | Opt::Long("clean-print", _) => opts.clean_print = true,
            Opt::Short(b's', _) | Opt::Long("squeeze", _) => opts.squeeze = true,
            // Accepted and deliberately inert; see the module docs for why each
            // is already what this pager does.
            Opt::Short(b'e' | b'u', _) | Opt::Long("exit-on-eof" | "plain", _) => {}
            Opt::Short(b'n', value) | Opt::Long("lines", value) => {
                let raw = value.unwrap_or_default();
                opts.lines = Some(parse_count(os_bytes(&raw).as_ref())?.max(1));
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => return Ok(Outcome::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Outcome::Version),
            // Every letter in SHORTS and every name in LONGS is handled above,
            // so this arm is unreachable; it is a `{}` rather than a panic
            // because a pager is not the place to abort over a parser bug.
            Opt::Short(_, _) | Opt::Long(_, _) => {}
            Opt::Operand(operand) => opts.files.push(operand.clone()),
        }
    }

    Ok(Outcome::Run(Box::new(opts)))
}

/// Whether `word` is an option whose value is the word after it.
///
/// The long form is resolved through `getopt`'s own prefix rule rather than by
/// comparing against `"lines"`, so that `--lin 5` is read the same way by both
/// scanners. Anything that does not resolve is `false`: an unknown option is
/// `getopt`'s to complain about, and swallowing the word after it would lose a
/// file name from the message.
fn takes_next_word(word: &[u8]) -> bool {
    if let Some(name) = word.strip_prefix(b"--") {
        // `--lines=5` carries its own value; `--lines 5` does not.
        if name.contains(&b'=') {
            return false;
        }
        let Ok(typed) = std::str::from_utf8(name) else {
            return false;
        };
        return matches!(
            MORE.resolve_long(typed, word, LONGS),
            Ok((_, Takes::Required))
        );
    }
    let Some(cluster) = word.strip_prefix(b"-") else {
        return false;
    };
    // `-n` is the only short option with a value, and it claims the next word
    // only when nothing follows it in this one: `-n5` and `-sn5` carry theirs.
    match cluster.iter().position(|letter| *letter == b'n') {
        Some(index) => index.saturating_add(1) == cluster.len(),
        None => false,
    }
}

/// Read a screenful count, in util-linux's wording for a bad one.
///
/// Overflow is an argument error rather than a saturation: a number too large
/// for a `usize` is a number the user did not mean, and silently treating
/// `-99999999999999999999` as "one enormous screen" would hide the typo behind
/// behaviour indistinguishable from `--no-pause`.
fn parse_count(digits: &[u8]) -> Result<usize, String> {
    // `quoteaf`, not `quotef`: the quotes are always on in util-linux's
    // wording, and the value is embedded in a sentence rather than ending it.
    let bad = |suffix: &str| format!("more: argument error: {}{suffix}", quoteaf(digits));
    let Ok(text) = std::str::from_utf8(digits) else {
        return Err(bad(""));
    };
    match text.parse::<usize>() {
        Ok(count) => Ok(count),
        // util-linux calls `strtoul` and then reports whatever it left in
        // `errno`, so the two failures are worded differently: a value too
        // large to hold gets `ERANGE`'s text appended and a value that was
        // never a number gets nothing. Both measured. The string is written
        // out rather than fetched from `strerror`, because this is the errno
        // `strtoul` *would* have set and not one the OS handed us.
        Err(why) if *why.kind() == IntErrorKind::PosOverflow => {
            Err(bad(": Numerical result out of range"))
        }
        Err(_) => Err(bad("")),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(Outcome::Run(o)) => *o,
        Ok(Outcome::Help) => return say(HELP),
        Ok(Outcome::Version) => return say(VERSION),
        Err(sentence) => {
            diag!("{sentence}");
            // `getopt`'s own sentences already carry the `Try 'more --help'`
            // line; ours (a bad `-n` value) do not, and util-linux's do not
            // either -- `more: argument error: 'abc'` is the whole message.
            return ExitCode::from(1);
        }
    };
    run(opts)
}

/// Print `text` on stdout, for `--help` and `--version`.
///
/// Unlike the paging path this one does *not* swallow a write error: `--help`
/// into a full disk that reports nothing is the case where a caller has no way
/// to tell it failed. See [`write_ignoring_errors`] for why the pager itself
/// takes the opposite view.
fn say(text: &[u8]) -> ExitCode {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if out.write_all(text).and_then(|()| out.flush()).is_err() {
        diag!("more: write error");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run(opts: Options) -> ExitCode {
    let stdin_is_tty = io::stdin().is_terminal();
    let banners = wants_banners(opts.files.len(), stdin_is_tty);

    // Computed once, before any output: `keys` being `None` is what "do not
    // page" means, so it must not be re-derived per file and drift. It is also
    // what every option in `opts` is conditioned on -- see the module docs.
    let mut keys = command_source(stdin_is_tty);
    let screen = Screen {
        lines_per_page: opts
            .lines
            .unwrap_or_else(|| terminal_lines(env::var("LINES").ok().as_deref()).saturating_sub(1))
            .max(1),
        columns: terminal_columns(env::var("COLUMNS").ok().as_deref()),
        opts: &opts,
    };

    let files = if opts.files.is_empty() {
        vec![OsString::from("-")]
    } else {
        opts.files.clone()
    };

    // One lock for the whole run, so the headers and the file bodies cannot
    // interleave and there is a single place to flush before returning.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // `+<number>` and `+/<pattern>` place the top of the *first* screen, so
    // they apply to the first file shown and to nothing after it -- and, like
    // every other option, only when there is a screen at all.
    let mut start = if keys.is_some() {
        Some(&opts.start)
    } else {
        None
    };

    for path in &files {
        let name = os_bytes(path);

        // The size behind the `(NN%)` in the prompt. `None` for stdin and for
        // anything that is not a regular file, which is why util-linux's
        // `more < f` prompts with a bare `--More--` (measured) while `more f`
        // reports a percentage: a pipe has no length to be a fraction of.
        let mut total = None;

        let reader: Box<dyn Read> = if name.as_ref() == b"-" {
            // A deliberate divergence, and the only one: util-linux has no
            // `-` convention and reports `cannot open -: No such file or
            // directory`. Every other utility in this tree spells stdin `-`,
            // and a pager that alone refused it would be the surprise. Note
            // that stdin gets no banner even when `banners` is set — there is
            // no file name to put in one, and util-linux's own stdin copy is
            // likewise unlabelled.
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => {
                    // Opening a directory succeeds; it is the *read* that
                    // fails, with EISDIR, after the banner has already
                    // claimed a file is about to appear. util-linux stats
                    // first and says so in-band on stdout, where the reader
                    // is looking, then carries on with status 0.
                    match f.metadata() {
                        Ok(meta) if meta.is_dir() => {
                            write_ignoring_errors(&mut out, &directory_marker(&name));
                            continue;
                        }
                        Ok(meta) if meta.is_file() => total = Some(meta.len()),
                        // A character device, a fifo, or a stat that failed:
                        // no length, so no percentage, which is the same
                        // answer as for stdin rather than a wrong fraction.
                        Ok(_) | Err(_) => {}
                    }
                    // After the open, never before: a banner printed first
                    // names a file that a failed open then never shows.
                    if banners {
                        // The name goes in as bytes rather than formatted: it
                        // is a filename, and on this OS that is any byte but
                        // `/` and NUL. util-linux prints it raw here too.
                        write_ignoring_errors(&mut out, &file_header(&name));
                    }
                    Box::new(f)
                }
                Err(e) => {
                    diag!("more: cannot open {}: {}", quotef(&name), strerror(&e));
                    continue;
                }
            }
        };

        // `-p` clears the screen before each file and `-c` homes the cursor
        // without clearing, which is the whole of "do not scroll" that can be
        // done without a terminal database. Both are guarded by `keys`, so
        // neither can inject an escape sequence into a pipe or a file. The
        // exact bytes are measured: `\e[H\e[2J` for `-p`, `\e[H` for `-c`.
        if keys.is_some() {
            if opts.clean_print {
                write_ignoring_errors(&mut out, b"\x1b[H\x1b[2J");
            } else if opts.print_over {
                write_ignoring_errors(&mut out, b"\x1b[H");
            }
        }

        // One `BufReader` per file, made here and handed to both the seek and
        // the pager. Wrapping the same reader twice would lose whatever the
        // first wrapper had read ahead into its buffer and not yet returned —
        // `+2` on a small file would have shown nothing at all.
        let mut buf = BufReader::new(reader);
        let mut from = Started::top();

        if let Some(where_to_start) = start.take() {
            match seek_start(&mut buf, where_to_start) {
                Ok(Sought::At(at)) => from = at,
                // Both failures say so on *stdout*, in reverse video, and then
                // show the file from the top with status 0 — all four of those
                // are measured, and the last is the surprising one: util-linux
                // treats a search that found nothing as a completed request to
                // look, not as an error. Re-opening is how the input gets back
                // to the top; stdin cannot, so it is shown from where the
                // search stopped rather than not at all.
                Ok(other) => {
                    write_ignoring_errors(&mut out, &highlight(other.notice()));
                    if let Some(fresh) = reopen(path, &name) {
                        buf = BufReader::new(fresh);
                    }
                }
                Err(e) => {
                    diag!("more: {}: {}", quotef(&name), strerror(&e));
                    continue;
                }
            }
        }

        if !page(&mut buf, &name, &mut out, &mut keys, &screen, total, from) {
            break;
        }
    }

    let _ = out.flush(); // see write_ignoring_errors
    ExitCode::SUCCESS
}

/// The parts of the command line that describe the screen rather than the
/// files, bundled so that [`page`] takes one argument for them and cannot be
/// called with two of them transposed.
struct Screen<'a> {
    opts: &'a Options,
    /// Already resolved against `-n`, `LINES` and the 24-line fallback, and
    /// never zero.
    lines_per_page: usize,
    /// Terminal width, for counting how many rows a long line costs. Zero
    /// means "unknown", which makes every line cost one row.
    columns: usize,
}

/// Re-open a named file after a `+/pattern` search consumed it.
///
/// `None` for stdin, which cannot be rewound, and for a file that has gone
/// away between the two opens.
fn reopen(path: &OsString, name: &[u8]) -> Option<Box<dyn Read>> {
    if name == b"-" {
        return None;
    }
    File::open(path).ok().map(|f| Box::new(f) as Box<dyn Read>)
}

/// Where a file is to be shown from, and what has already been read to get
/// there.
#[derive(Debug, Default, PartialEq, Eq)]
struct Started {
    /// Lines already taken out of the input that must be written before any
    /// further read: the `+/pattern` match and the [`CONTEXT_LINES`] before
    /// it. A `BufRead` has no way to put them back.
    lines: Vec<Vec<u8>>,
    /// Bytes taken from the file so far, including `lines` and everything
    /// skipped. This is the numerator of the prompt's percentage, which is why
    /// `more +5 f` reports 19% and not 9% on the first screen (measured).
    consumed: u64,
    /// Whether at least one line was passed over on the way, which is what
    /// util-linux marks with `...skipping`.
    skipped: bool,
}

impl Started {
    fn top() -> Self {
        Self::default()
    }
}

/// Where `seek_start` left the input.
#[derive(Debug, PartialEq, Eq)]
enum Sought {
    /// Positioned.
    At(Started),
    /// `+/pattern` reached end of input without matching.
    NotFound,
    /// `+/pattern` was not a regular expression.
    BadPattern,
}

impl Sought {
    /// What to put on the screen about this outcome. Both wordings are
    /// util-linux's, measured on a terminal.
    fn notice(&self) -> &'static [u8] {
        match *self {
            Sought::NotFound => b"Pattern not found",
            Sought::BadPattern => b"Invalid regular expression",
            // `At` never reaches here — `run` matches it first — and an empty
            // notice is the harmless answer if that ever stops being true.
            Sought::At(_) => b"",
        }
    }
}

/// Consume input up to the line that `start` says should be at the top.
///
/// Nothing is written here: the lines skipped are the ones that would have
/// scrolled off before the user saw a screen, which is what `+20` asks for.
fn seek_start<R: BufRead>(buf: &mut R, start: &Start) -> io::Result<Sought> {
    match start {
        Start::Top => Ok(Sought::At(Started::top())),
        Start::Line(n) => {
            // `+1` is the top, so `n - 1` lines are dropped. `+0` is accepted
            // and means the same as `+1`; util-linux takes it too. No
            // `...skipping` here — measured, `more +5 f` prints none, and only
            // the pattern form does.
            let mut line = Vec::new();
            let mut consumed = 0u64;
            for _ in 1..*n {
                line.clear();
                // Past the end is not an error: `more +999 short.txt` shows a
                // blank screen rather than complaining.
                let got = buf.read_until(b'\n', &mut line)?;
                if got == 0 {
                    break;
                }
                consumed = consumed.saturating_add(u64::try_from(got).unwrap_or(u64::MAX));
            }
            Ok(Sought::At(Started {
                lines: Vec::new(),
                consumed,
                skipped: false,
            }))
        }
        Start::Pattern(pattern) => {
            // Compiled here rather than at parse time; see [`Start::Pattern`].
            // Case-sensitive: util-linux passes no `REG_ICASE`.
            let Ok(re) = bre::compile(pattern, false) else {
                return Ok(Sought::BadPattern);
            };
            // The match is shown with the lines above it, so those have to be
            // kept as they go by rather than found again afterwards — the
            // input may be a pipe, which cannot be re-read.
            let mut context: VecDeque<Vec<u8>> = VecDeque::with_capacity(CONTEXT_LINES);
            let mut consumed = 0u64;
            let mut skipped = false;
            loop {
                let mut line = Vec::new();
                let got = buf.read_until(b'\n', &mut line)?;
                if got == 0 {
                    return Ok(Sought::NotFound);
                }
                consumed = consumed.saturating_add(u64::try_from(got).unwrap_or(u64::MAX));
                // Matched against the line without its newline, so `pat$`
                // means what it says. `find` can only fail by exhausting the
                // engine's step budget on a pathological backreference;
                // treating that as "no match on this line" keeps the search
                // going rather than aborting the file.
                if re.find(strip_newline(&line)).unwrap_or(None).is_some() {
                    let mut lines: Vec<Vec<u8>> = context.into_iter().collect();
                    lines.push(line);
                    return Ok(Sought::At(Started {
                        lines,
                        consumed,
                        skipped,
                    }));
                }
                context.push_back(line);
                if context.len() > CONTEXT_LINES {
                    context.pop_front();
                    skipped = true;
                }
            }
        }
    }
}

/// Wrap a notice in the terminal's reverse video, as util-linux does.
///
/// The trailing newline is ours: util-linux leaves the cursor mid-line and
/// repaints, which needs a cursor-addressable screen model this pager does not
/// have. Without it the first line of the file would run into the message.
fn highlight(notice: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(notice.len().saturating_add(10));
    v.extend_from_slice(b"\x1b[7m");
    v.extend_from_slice(notice);
    v.extend_from_slice(b"\x1b[27m\n");
    v
}

/// The line without its trailing `\n`, and without a `\r` before it.
fn strip_newline(line: &[u8]) -> &[u8] {
    let end = line.strip_suffix(b"\n").unwrap_or(line);
    end.strip_suffix(b"\r").unwrap_or(end)
}

/// Copy one input to `out`, pausing every screenful if there is somewhere to
/// read a keystroke from.
///
/// Returns `false` when the user asked to quit, which ends the whole run and
/// not just this file.
///
/// Every option in `screen.opts` is consulted only on the branch guarded by
/// `keys`, which is the whole of the rule in the module docs: with stdout on a
/// pipe this is a byte-for-byte copy and nothing below can change that.
fn page<R: BufRead>(
    buf: &mut R,
    name: &[u8],
    out: &mut impl Write,
    keys: &mut Option<Box<dyn Read>>,
    screen: &Screen<'_>,
    total: Option<u64>,
    from: Started,
) -> bool {
    let paging = keys.is_some();
    let mut consumed = from.consumed;
    let mut rows: usize = 0;
    // `-s` collapses a *run* of blank lines, so it needs to know whether the
    // line just written was itself blank.
    let mut after_blank = false;
    let mut replay = 0usize;
    let mut buffer: Vec<u8> = Vec::new();

    if paging && from.skipped {
        write_ignoring_errors(out, b"...skipping\n");
    }

    loop {
        // The lines the `+/pattern` search had to read come out first, in
        // order, before the reader is touched again.
        let line: &[u8] = match from.lines.get(replay) {
            Some(held) => {
                replay = replay.saturating_add(1);
                held
            }
            None => {
                buffer.clear();
                match buf.read_until(b'\n', &mut buffer) {
                    // 0 bytes is end of input. A short read is not:
                    // `read_until` has already looped for us and only stops at
                    // the delimiter or at EOF, so a final line with no newline
                    // arrives here intact and is written back without one —
                    // which is what the file holds and what util-linux prints.
                    Ok(0) => return true,
                    Ok(got) => {
                        consumed = consumed.saturating_add(u64::try_from(got).unwrap_or(u64::MAX));
                    }
                    // A read error is the one case where stopping is right, but
                    // it must not be silent: this is where the UTF-8 failure
                    // used to land, mislabelled as end of file.
                    Err(e) => {
                        diag!("more: {}: {}", quotef(name), strerror(&e));
                        return true;
                    }
                }
                &buffer
            }
        };

        // `-s`: a second consecutive blank line is dropped rather than
        // written, so a run of any length shows as one.
        let blank = strip_newline(line).is_empty();
        if paging && screen.opts.squeeze && blank && after_blank {
            continue;
        }
        after_blank = blank;

        // `-c`: clear to end of line before each line, so text painted over
        // longer text does not leave the old tail behind.
        if paging && screen.opts.print_over {
            write_ignoring_errors(out, b"\x1b[K");
        }
        write_ignoring_errors(out, line);

        // No keystroke source means stdout is not a terminal, so there is no
        // screen to fill and nothing to wait for.
        let Some(src) = keys.as_mut() else { continue };

        rows = rows.saturating_add(row_cost(line, screen));
        // A form feed ends the page it is on, which is what a form feed is
        // for; `-l` is the option that says to treat it as an ordinary byte.
        let form_feed = !screen.opts.no_pause && line.contains(&FORM_FEED);
        if rows < screen.lines_per_page && !form_feed {
            continue;
        }

        // The prompt goes to stdout, not stderr: paging happens only when
        // stdout *is* the terminal, so that is where the pager's screen is.
        // On descriptor 2 it would vanish under `more f 2>/dev/null` and the
        // pager would look hung.
        write_ignoring_errors(out, &prompt(total, consumed, screen.opts.silent));
        let _ = out.flush(); // see write_ignoring_errors

        match read_key(src.as_mut()) {
            Key::Quit => return false,
            Key::Line => rows = screen.lines_per_page.saturating_sub(1),
            Key::Page => rows = 0,
        }

        // Erase the prompt with the terminal's own clear-to-end-of-line rather
        // than by overwriting it with spaces: the prompt's width is not fixed
        // once it carries a percentage and `-d`'s hint.
        write_ignoring_errors(out, b"\r\x1b[K");
    }
}

/// The byte that ends a page whatever else is on the line. `-l` disables it.
const FORM_FEED: u8 = 0x0c;

/// Build the `--More--` prompt.
///
/// Three measured details: it is in reverse video; it carries `(NN%)` when the
/// input has a length and nothing when it does not (`more < f` prompts bare);
/// and `-d` appends its hint *inside* the highlighted run rather than after it.
fn prompt(total: Option<u64>, consumed: u64, silent: bool) -> Vec<u8> {
    let mut v = Vec::from(&b"\x1b[7m--More--"[..]);
    if let Some(size) = total.filter(|&size| size > 0) {
        // Truncated, not rounded: three lines of an 81-byte file report 7%,
        // and 6 * 100 / 81 is 7.4. Clamped because a file can grow while it is
        // being read, and `(120%)` would be a bug report.
        let percent = consumed
            .saturating_mul(100)
            .checked_div(size)
            .unwrap_or(0)
            .min(100);
        v.extend_from_slice(format!("({percent}%)").as_bytes());
    }
    if silent {
        v.extend_from_slice(b"[Press space to continue, 'q' to quit.]");
    }
    v.extend_from_slice(b"\x1b[27m");
    v
}

/// How many rows of the screen one line of the file occupies.
///
/// One, under `-f`, or when the width is unknown. Otherwise the line's display
/// width divided by the terminal's, rounded up: a 200-column line on an
/// 80-column terminal fills three rows, and counting it as one is what makes a
/// pager scroll text off the top of the screen it just promised to stop at.
fn row_cost(line: &[u8], screen: &Screen<'_>) -> usize {
    if screen.opts.logical || screen.columns == 0 {
        return 1;
    }
    let width = display_width(strip_newline(line));
    width
        .saturating_add(screen.columns.saturating_sub(1))
        .checked_div(screen.columns)
        .unwrap_or(1)
        .max(1)
}

/// The number of terminal columns `body` occupies.
///
/// Bytes that are not valid UTF-8 count one column each: they go to the
/// terminal unaltered, and one is the least wrong guess about what it will do
/// with them. A zero-width character — a combining mark — costs nothing, which
/// is the whole reason this does not count bytes or `char`s.
fn display_width(body: &[u8]) -> usize {
    let mut width = 0usize;
    let mut rest = body;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => return width.saturating_add(str_width(text)),
            Err(bad) => {
                let good = bad.valid_up_to();
                if let Some(text) = rest.get(..good).and_then(|b| std::str::from_utf8(b).ok()) {
                    width = width.saturating_add(str_width(text));
                }
                width = width.saturating_add(1);
                match rest.get(good.saturating_add(1)..) {
                    Some(tail) => rest = tail,
                    None => return width,
                }
            }
        }
    }
}

fn str_width(text: &str) -> usize {
    text.chars().fold(0usize, |total, c| {
        total.saturating_add(charwidth::char_width(c).unwrap_or(0))
    })
}

/// Where the pager reads keystrokes — and therefore whether it pages at all.
///
/// `None` means "copy straight through": stdout is not a terminal, so there is
/// no screen to fill. Returning it is the fix for a pager that used to page
/// into a pipe, read EOF where a keystroke should have been, and treat that as
/// the user pressing `q`.
///
/// When stdin has been redirected but stdout is still a terminal, commands
/// cannot come from stdin — that descriptor is somebody's data, and reading it
/// for keystrokes would eat it. util-linux reopens the controlling terminal
/// for this and so do we; if there is none, we fall back to not paging, which
/// shows the whole file rather than a truncated one.
fn command_source(stdin_is_tty: bool) -> Option<Box<dyn Read>> {
    if !io::stdout().is_terminal() {
        return None;
    }
    if stdin_is_tty {
        return Some(Box::new(io::stdin()));
    }
    File::open("/dev/tty")
        .ok()
        .map(|f| Box::new(f) as Box<dyn Read>)
}

/// Whether each file's name is announced in a `::::` banner.
///
/// More than one operand is the obvious half. The other half — stdin not being
/// a terminal — is measured, not guessed: util-linux labels a lone file for
/// `more f < /dev/null` and leaves it unlabelled for `more f` at a prompt,
/// with stdout a pipe in both cases. Keying it on stdin rather than stdout is
/// what makes `more f | cat` from a terminal come out clean.
fn wants_banners(operands: usize, stdin_is_tty: bool) -> bool {
    operands > 1 || !stdin_is_tty
}

/// Write to stdout and discard the result, deliberately.
///
/// Discarding a write error is normally a defect in this tree, so the reason is
/// worth stating: `more plain.txt > /dev/full` on util-linux 2.39.3 writes no
/// diagnostic and **exits 0** (measured). More importantly, the common way a
/// pager's stdout ends is `more f | head`, where the pipe closing is the
/// pipeline working, not a failure — and unlike `cat`, `more` has no caller
/// that is going to treat its output as data to be checked. Matching the
/// reference is the right call here; the exit status stays what it was.
fn write_ignoring_errors(out: &mut impl Write, bytes: &[u8]) {
    let _ = out.write_all(bytes);
}

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Page, // space
    Line, // enter
    Quit, // q
}

fn read_key(src: &mut dyn Read) -> Key {
    let mut buf = [0u8; 1];
    match src.read(&mut buf) {
        Ok(0) | Err(_) => Key::Quit,
        Ok(_) => parse_key_byte(buf.first().copied().unwrap_or(b' ')),
    }
}

/// Translate one byte of user input into a `Key` action.
fn parse_key_byte(b: u8) -> Key {
    match b {
        b'q' | b'Q' => Key::Quit,
        b' ' => Key::Page,
        b'\n' | b'\r' => Key::Line,
        _ => Key::Page,
    }
}

/// Compute the terminal line count from a `LINES` env value; falls back to 24.
fn terminal_lines(env_value: Option<&str>) -> usize {
    if let Some(val) = env_value
        && let Ok(n) = val.parse::<usize>()
        && n > 0
    {
        return n;
    }
    24
}

/// Compute the terminal width from a `COLUMNS` env value; falls back to 80.
///
/// The fallback matters more than the lines one does, because it is only used
/// to decide how many rows a long line costs: 80 is the width every terminal
/// is at least, so a wrong guess here shows a slightly short screen rather
/// than scrolling text away unseen.
fn terminal_columns(env_value: Option<&str>) -> usize {
    if let Some(val) = env_value
        && let Ok(n) = val.parse::<usize>()
        && n > 0
    {
        return n;
    }
    80
}

/// Build the three header lines printed before a file, as bytes, with the
/// trailing newline on each.
fn file_header(path: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(path.len().saturating_add(BANNER.len() * 2 + 3));
    v.extend_from_slice(BANNER);
    v.push(b'\n');
    v.extend_from_slice(path);
    v.push(b'\n');
    v.extend_from_slice(BANNER);
    v.push(b'\n');
    v
}

/// What is printed in place of a directory's contents.
///
/// On stdout, not stderr, and byte-for-byte util-linux's: `\n*** NAME:
/// directory ***\n\n`. A pager's user is reading stdout; a note about why one
/// of the requested files produced nothing belongs in the same stream as the
/// files that did.
fn directory_marker(path: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(path.len().saturating_add(22));
    v.extend_from_slice(b"\n*** ");
    v.extend_from_slice(path);
    v.extend_from_slice(b": directory ***\n\n");
    v
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn quit_keys() {
        assert_eq!(parse_key_byte(b'q'), Key::Quit);
        assert_eq!(parse_key_byte(b'Q'), Key::Quit);
    }

    #[test]
    fn space_is_page() {
        assert_eq!(parse_key_byte(b' '), Key::Page);
    }

    #[test]
    fn newline_is_line() {
        assert_eq!(parse_key_byte(b'\n'), Key::Line);
        assert_eq!(parse_key_byte(b'\r'), Key::Line);
    }

    #[test]
    fn unknown_byte_defaults_to_page() {
        assert_eq!(parse_key_byte(b'x'), Key::Page);
        assert_eq!(parse_key_byte(0), Key::Page);
        assert_eq!(parse_key_byte(255), Key::Page);
    }

    #[test]
    fn terminal_lines_default_when_unset() {
        assert_eq!(terminal_lines(None), 24);
    }

    #[test]
    fn terminal_lines_parses_env() {
        assert_eq!(terminal_lines(Some("40")), 40);
    }

    #[test]
    fn terminal_lines_falls_back_on_garbage() {
        assert_eq!(terminal_lines(Some("notanumber")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_zero() {
        assert_eq!(terminal_lines(Some("0")), 24);
    }

    #[test]
    fn terminal_lines_falls_back_on_empty() {
        assert_eq!(terminal_lines(Some("")), 24);
    }

    #[test]
    fn file_header_contains_path() {
        assert_eq!(
            file_header(b"data.txt"),
            b"::::::::::::::\ndata.txt\n::::::::::::::\n"
        );
    }

    #[test]
    fn file_header_banner_is_fourteen_colons() {
        // Measured against util-linux more 2.39.3, which prints fourteen.
        // This was thirteen, which is the sort of difference nothing catches
        // by eye and every diff catches immediately.
        assert_eq!(BANNER.len(), 14);
        assert!(BANNER.iter().all(|&b| b == b':'));
    }

    #[test]
    fn file_header_with_unusual_chars() {
        let h = file_header(b"a b/c.txt");
        assert!(h.windows(9).any(|w| w == b"a b/c.txt"));
    }

    #[test]
    fn file_header_passes_a_non_utf8_name_through() {
        // The banner names the file being paged, so a lossy copy here would
        // name a file the user does not have. util-linux prints the bytes.
        let h = file_header(b"caf\xe9.txt");
        assert_eq!(h, b"::::::::::::::\ncaf\xe9.txt\n::::::::::::::\n");
    }

    #[test]
    fn directory_marker_matches_util_linux() {
        assert_eq!(directory_marker(b"dir"), b"\n*** dir: directory ***\n\n");
    }

    #[test]
    fn directory_marker_passes_a_non_utf8_name_through() {
        assert_eq!(
            directory_marker(b"caf\xe9"),
            b"\n*** caf\xe9: directory ***\n\n"
        );
    }

    #[test]
    fn banner_rule_matches_util_linux() {
        // Every cell of the measured matrix. The interesting ones are the
        // two single-file rows: the banner appears with stdin redirected and
        // not at an interactive prompt.
        assert!(!wants_banners(1, true));
        assert!(wants_banners(1, false));
        assert!(wants_banners(2, true));
        assert!(wants_banners(2, false));
        // No operands means stdin, which is never banner-ed; the count is
        // still zero here rather than one, because `-` is substituted after
        // this decision is made.
        assert!(!wants_banners(0, true));
    }

    /// A `Screen` over freshly-defaulted options, with the two numbers named.
    ///
    /// Every `page` test needs one and almost none of them care about the
    /// options, so building it here keeps the interesting line of each test
    /// the assertion rather than the setup.
    fn screen_of(opts: &Options, lines_per_page: usize, columns: usize) -> Screen<'_> {
        Screen {
            opts,
            lines_per_page,
            columns,
        }
    }

    /// Run `page` over `input` with no file size, so the prompt has no
    /// percentage and the expected output stays readable.
    fn run_page(input: &[u8], keys: Option<&'static [u8]>, screen: &Screen<'_>) -> (bool, Vec<u8>) {
        let mut buf = BufReader::new(input);
        let mut out: Vec<u8> = Vec::new();
        let mut src: Option<Box<dyn Read>> = keys.map(|k| Box::new(k) as Box<dyn Read>);
        let more = page(
            &mut buf,
            b"t",
            &mut out,
            &mut src,
            screen,
            None,
            Started::top(),
        );
        (more, out)
    }

    #[test]
    fn page_copies_everything_when_there_is_no_keystroke_source() {
        // The regression this whole rework exists for: with `keys` at `None`
        // -- stdout not a terminal -- a file longer than a screen must come
        // out whole, not one page of it.
        let body: Vec<u8> = (1..=60)
            .flat_map(|n| format!("{n}\n").into_bytes())
            .collect();
        let opts = Options::new();
        let (more, out) = run_page(&body, None, &screen_of(&opts, 23, 80));
        assert!(more);
        assert_eq!(out, body);
    }

    #[test]
    fn page_reproduces_a_missing_final_newline() {
        let opts = Options::new();
        let (more, out) = run_page(b"no-newline", None, &screen_of(&opts, 23, 80));
        assert!(more);
        assert_eq!(out, b"no-newline");
    }

    #[test]
    fn page_copies_bytes_that_are_not_utf8() {
        let opts = Options::new();
        let (more, out) = run_page(b"one\ntw\xffo\nthree\n", None, &screen_of(&opts, 23, 80));
        assert!(more);
        assert_eq!(out, b"one\ntw\xffo\nthree\n");
    }

    #[test]
    fn page_ignores_every_display_option_when_there_is_no_screen() {
        // The governing rule, as a test: with `keys` at `None` the output is
        // the file, whatever the options said. Measured against util-linux,
        // where `more -s -c -d f | cat` is byte-identical to `cat f`.
        let mut opts = Options::new();
        opts.squeeze = true;
        opts.print_over = true;
        opts.clean_print = true;
        opts.silent = true;
        let body: &[u8] = b"a\n\n\n\nb\x0cc\n";
        let (more, out) = run_page(body, None, &screen_of(&opts, 1, 80));
        assert!(more);
        assert_eq!(out, body);
    }

    #[test]
    fn page_stops_the_run_when_the_key_is_q() {
        let opts = Options::new();
        let (more, out) = run_page(b"a\nb\nc\n", Some(b"q"), &screen_of(&opts, 1, 80));
        assert!(!more);
        assert_eq!(out, b"a\n\x1b[7m--More--\x1b[27m");
    }

    #[test]
    fn page_continues_on_space() {
        let opts = Options::new();
        let (more, out) = run_page(b"a\nb\n", Some(b"  "), &screen_of(&opts, 1, 80));
        assert!(more);
        assert_eq!(
            out,
            b"a\n\x1b[7m--More--\x1b[27m\r\x1b[Kb\n\x1b[7m--More--\x1b[27m\r\x1b[K"
        );
    }

    #[test]
    fn squeeze_collapses_a_run_of_blank_lines_to_one() {
        let mut opts = Options::new();
        opts.squeeze = true;
        // Measured: `a`, one blank, `b`, one blank, `c`.
        let (more, out) = run_page(
            b"a\n\n\n\n\nb\n\n\n\nc\n",
            Some(b"     "),
            &screen_of(&opts, 99, 80),
        );
        assert!(more);
        assert_eq!(out, b"a\n\nb\n\nc\n");
    }

    #[test]
    fn without_squeeze_every_blank_line_is_kept() {
        let opts = Options::new();
        let body: &[u8] = b"a\n\n\n\n\nb\n";
        let (more, out) = run_page(body, Some(b"     "), &screen_of(&opts, 99, 80));
        assert!(more);
        assert_eq!(out, body);
    }

    #[test]
    fn a_form_feed_ends_the_page_and_minus_l_stops_it() {
        // Measured: `p1\fp2\np3\n` pauses after the first line by default and
        // does not with `-l`.
        let body: &[u8] = b"p1\x0cp2\np3\n";
        let opts = Options::new();
        let (_, paused) = run_page(body, Some(b"  "), &screen_of(&opts, 99, 80));
        assert!(paused.starts_with(b"p1\x0cp2\n\x1b[7m--More--"));

        let mut plain = Options::new();
        plain.no_pause = true;
        let (_, whole) = run_page(body, Some(b"  "), &screen_of(&plain, 99, 80));
        assert_eq!(whole, body);
    }

    #[test]
    fn a_long_line_costs_more_than_one_row() {
        let opts = Options::new();
        // Three rows of a ten-column terminal, so a two-row screen pauses
        // after the first line rather than after the second.
        let body: Vec<u8> = [b"x".repeat(25), b"\nshort\n".to_vec()].concat();
        let (_, out) = run_page(&body, Some(b" "), &screen_of(&opts, 2, 10));
        assert!(out.starts_with(&[b"x".repeat(25), b"\n\x1b[7m--More--".to_vec()].concat()));
    }

    #[test]
    fn logical_lines_count_one_apiece_however_long() {
        let mut opts = Options::new();
        opts.logical = true;
        let body: Vec<u8> = [b"x".repeat(25), b"\nshort\n".to_vec()].concat();
        let (_, out) = run_page(&body, Some(b" "), &screen_of(&opts, 2, 10));
        assert!(out.starts_with(&[b"x".repeat(25), b"\nshort\n\x1b[7m--More--".to_vec()].concat()));
    }

    #[test]
    fn prompt_is_bare_without_a_size_and_carries_a_percentage_with_one() {
        assert_eq!(prompt(None, 6, false), b"\x1b[7m--More--\x1b[27m");
        // Measured: three lines of `seq 1 30` -- 6 of 81 bytes -- reads 7%.
        assert_eq!(prompt(Some(81), 6, false), b"\x1b[7m--More--(7%)\x1b[27m");
    }

    #[test]
    fn silent_appends_the_hint_inside_the_highlight() {
        assert_eq!(
            prompt(Some(81), 6, true),
            b"\x1b[7m--More--(7%)[Press space to continue, 'q' to quit.]\x1b[27m".as_slice()
        );
    }

    #[test]
    fn a_percentage_never_exceeds_a_hundred() {
        // A file that grew while it was being read. `(120%)` is a bug report.
        assert_eq!(
            prompt(Some(10), 12, false),
            b"\x1b[7m--More--(100%)\x1b[27m"
        );
        // A zero-length file has no fraction to report at all.
        assert_eq!(prompt(Some(0), 0, false), b"\x1b[7m--More--\x1b[27m");
    }

    #[test]
    fn display_width_counts_columns_not_bytes() {
        assert_eq!(display_width(b"abc"), 3);
        // Two columns for one three-byte character.
        assert_eq!(display_width("\u{4e00}".as_bytes()), 2);
        // A combining mark is free.
        assert_eq!(display_width("e\u{0301}".as_bytes()), 1);
        // A byte that decodes to nothing still occupies the terminal.
        assert_eq!(display_width(b"a\xffb"), 3);
        assert_eq!(display_width(b""), 0);
    }

    #[test]
    fn terminal_columns_default_and_env() {
        assert_eq!(terminal_columns(None), 80);
        assert_eq!(terminal_columns(Some("100")), 100);
        assert_eq!(terminal_columns(Some("0")), 80);
        assert_eq!(terminal_columns(Some("wide")), 80);
    }

    /// `parse_args` over `&str`s, which every command line in these tests is.
    fn parse(words: &[&str]) -> Result<Outcome, String> {
        let argv: Vec<OsString> = words.iter().map(OsString::from).collect();
        parse_args(&argv)
    }

    fn options(words: &[&str]) -> Options {
        match parse(words) {
            Ok(Outcome::Run(opts)) => *opts,
            Ok(_) => panic!("expected a run, got help or version"),
            Err(why) => panic!("expected a run, got {why}"),
        }
    }

    #[test]
    fn bare_operands_are_files() {
        let opts = options(&["a.txt", "b.txt"]);
        assert_eq!(
            opts.files,
            vec![OsString::from("a.txt"), OsString::from("b.txt")]
        );
        assert!(matches!(opts.start, Start::Top));
        assert_eq!(opts.lines, None);
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert!(matches!(parse(&["--help"]), Ok(Outcome::Help)));
        assert!(matches!(parse(&["-h"]), Ok(Outcome::Help)));
        assert!(matches!(parse(&["--version"]), Ok(Outcome::Version)));
        assert!(matches!(parse(&["-V"]), Ok(Outcome::Version)));
    }

    #[test]
    fn a_bare_number_option_sets_the_screenful() {
        // The spelling `getopt` cannot represent: `-5` is five option letters
        // to it, and a screen height to `more`.
        assert_eq!(options(&["-5", "f"]).lines, Some(5));
        assert_eq!(options(&["-n", "5", "f"]).lines, Some(5));
        assert_eq!(options(&["-n5", "f"]).lines, Some(5));
        assert_eq!(options(&["--lines=5", "f"]).lines, Some(5));
        assert_eq!(options(&["--lines", "5", "f"]).lines, Some(5));
        // Zero would mean a screen that can hold nothing and prompts for ever.
        assert_eq!(options(&["-0", "f"]).lines, Some(1));
    }

    #[test]
    fn a_bad_screenful_is_util_linuxs_wording() {
        // Measured: `more -n abc f` prints exactly this and exits 1.
        assert_eq!(
            parse(&["-n", "abc", "f"]).err(),
            Some("more: argument error: 'abc'".to_string())
        );
        assert_eq!(
            parse(&["--lines=abc", "f"]).err(),
            Some("more: argument error: 'abc'".to_string())
        );
        // Too large for a `usize` is a typo, not an enormous screen -- and is
        // worded differently, because util-linux reports what `strtoul` left
        // in `errno`. Measured.
        assert_eq!(
            parse(&["-n", "999999999999999999999", "f"]).err(),
            Some(
                "more: argument error: '999999999999999999999': Numerical result out of range"
                    .to_string()
            )
        );
    }

    #[test]
    fn plus_number_sets_the_first_line() {
        assert!(matches!(options(&["+5", "f"]).start, Start::Line(5)));
        // `+0` and `+1` are both the top.
        assert!(matches!(options(&["+0", "f"]).start, Start::Line(1)));
        assert!(matches!(options(&["+1", "f"]).start, Start::Line(1)));
    }

    #[test]
    fn plus_slash_sets_the_first_pattern() {
        match options(&["+/17", "f"]).start {
            Start::Pattern(p) => assert_eq!(p, b"17"),
            _ => panic!("expected a pattern"),
        }
        // An empty pattern is a pattern, and matches the first line.
        match options(&["+/", "f"]).start {
            Start::Pattern(p) => assert!(p.is_empty()),
            _ => panic!("expected a pattern"),
        }
    }

    #[test]
    fn plus_anything_else_is_a_file_name() {
        // Measured: `more +abc n.txt` reports `cannot open +abc`, so `+abc` is
        // an operand and not a malformed option.
        let opts = options(&["+abc", "f"]);
        assert!(matches!(opts.start, Start::Top));
        assert_eq!(
            opts.files,
            vec![OsString::from("+abc"), OsString::from("f")]
        );
    }

    #[test]
    fn a_lone_dash_is_stdin_and_not_an_empty_number() {
        let opts = options(&["-"]);
        assert_eq!(opts.files, vec![OsString::from("-")]);
        assert_eq!(opts.lines, None);
    }

    #[test]
    fn double_dash_ends_the_options_for_both_scanners() {
        // The pre-scan and `getopt` have to agree about where options stop, or
        // `more -- -5` would set a screen height *and* fail to open a file.
        let opts = options(&["--", "-5", "+3"]);
        assert_eq!(opts.lines, None);
        assert!(matches!(opts.start, Start::Top));
        assert_eq!(opts.files, vec![OsString::from("-5"), OsString::from("+3")]);
    }

    #[test]
    fn a_value_in_the_next_word_is_not_read_as_an_option() {
        // The first of two deliberate divergences from util-linux, both of
        // them places where its own `-NUM` pre-scan does not know what the
        // rest of the parser knows.
        //
        // Measured: `more -n -3 n.txt` reports `argument error: 'n.txt'`,
        // because util-linux's scan lifted `-3` out as a screen height and
        // left `-n` to take the file name. Ours gives `-3` to `-n`, where it
        // was typed, and names it. Both refuse the command and exit 1; only
        // the token named differs, and naming the one the user actually got
        // wrong is the point of the message.
        assert_eq!(
            parse(&["-n", "-3", "n.txt"]).err(),
            Some("more: argument error: '-3'".to_string())
        );
        // Same rule for the long spelling, and through `getopt`'s abbreviation
        // rule so the two scanners cannot disagree about what `--lin` is.
        assert_eq!(options(&["--lines", "5", "f"]).lines, Some(5));
        assert_eq!(options(&["--lin", "5", "f"]).lines, Some(5));
        // A value already attached does not claim the next word.
        let opts = options(&["-n5", "f"]);
        assert_eq!(opts.lines, Some(5));
        assert_eq!(opts.files, vec![OsString::from("f")]);
        let bundled = options(&["-sn5", "f"]);
        assert_eq!(bundled.lines, Some(5));
        assert!(bundled.squeeze);
        assert_eq!(bundled.files, vec![OsString::from("f")]);
    }

    #[test]
    fn double_dash_protects_a_file_named_like_an_option() {
        // The second divergence. Measured: `more -- -5 n.txt` pages `n.txt`
        // with a five-line screen and never mentions `-5`, because
        // util-linux's `-NUM` scan does not honour `--`. That defeats the one
        // thing `--` is for, so ours treats `-5` as the file name it was
        // promised to be — and then says it cannot open it, which is the
        // answer to what was asked.
        let opts = options(&["--", "-5", "n.txt"]);
        assert_eq!(opts.lines, None);
        assert_eq!(
            opts.files,
            vec![OsString::from("-5"), OsString::from("n.txt")]
        );
    }

    #[test]
    fn every_flag_is_accepted_in_both_spellings() {
        assert!(options(&["-d", "f"]).silent);
        assert!(options(&["--silent", "f"]).silent);
        assert!(options(&["-f", "f"]).logical);
        assert!(options(&["--logical", "f"]).logical);
        assert!(options(&["-l", "f"]).no_pause);
        assert!(options(&["--no-pause", "f"]).no_pause);
        assert!(options(&["-c", "f"]).print_over);
        assert!(options(&["--print-over", "f"]).print_over);
        assert!(options(&["-p", "f"]).clean_print);
        assert!(options(&["--clean-print", "f"]).clean_print);
        assert!(options(&["-s", "f"]).squeeze);
        assert!(options(&["--squeeze", "f"]).squeeze);
        // Accepted and inert, which still has to parse.
        assert!(parse(&["-e", "-u", "--exit-on-eof", "--plain", "f"]).is_ok());
    }

    #[test]
    fn flags_bundle() {
        let opts = options(&["-sdl", "f"]);
        assert!(opts.squeeze && opts.silent && opts.no_pause);
        assert_eq!(opts.files, vec![OsString::from("f")]);
    }

    #[test]
    fn an_unknown_option_is_refused_with_the_try_line() {
        let why = parse(&["--zzz", "f"]).expect_err("--zzz is not an option");
        assert!(why.contains("--zzz"), "{why}");
        assert!(why.contains("more --help"), "{why}");
    }

    #[test]
    fn an_ambiguous_long_option_names_the_candidates() {
        // `--s` could be `--silent` or `--squeeze`; measured, util-linux lists
        // both rather than picking one.
        let why = parse(&["--s", "f"]).expect_err("--s is ambiguous");
        assert!(why.contains("ambiguous"), "{why}");
        assert!(why.contains("silent") && why.contains("squeeze"), "{why}");
    }

    #[test]
    fn seek_to_a_line_drops_the_ones_above_it() {
        let mut buf = BufReader::new(&b"1\n2\n3\n4\n"[..]);
        let got = seek_start(&mut buf, &Start::Line(3)).unwrap();
        assert_eq!(
            got,
            Sought::At(Started {
                lines: Vec::new(),
                consumed: 4,
                skipped: false,
            })
        );
        let mut rest = Vec::new();
        buf.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"3\n4\n");
    }

    #[test]
    fn seeking_past_the_end_is_not_an_error() {
        let mut buf = BufReader::new(&b"1\n2\n"[..]);
        assert!(matches!(
            seek_start(&mut buf, &Start::Line(999)),
            Ok(Sought::At(_))
        ));
    }

    #[test]
    fn seek_to_a_pattern_keeps_two_lines_of_context() {
        // Measured: `more -n 5 +/17` on `seq 1 30` shows 15 through 19, so the
        // match arrives with the two lines above it.
        let body: Vec<u8> = (1..=30)
            .flat_map(|n| format!("{n}\n").into_bytes())
            .collect();
        let mut buf = BufReader::new(body.as_slice());
        match seek_start(&mut buf, &Start::Pattern(b"^17$".to_vec())).unwrap() {
            Sought::At(at) => {
                assert_eq!(
                    at.lines,
                    vec![b"15\n".to_vec(), b"16\n".to_vec(), b"17\n".to_vec()]
                );
                // Lines 1..=17 of `seq 1 30`: nine of two bytes, eight of three.
                assert_eq!(at.consumed, 9 * 2 + 8 * 3);
                assert!(at.skipped);
            }
            other => panic!("expected a match, got {other:?}"),
        }
    }

    #[test]
    fn a_match_near_the_top_is_not_announced_as_skipping() {
        let mut buf = BufReader::new(&b"a\nb\nc\n"[..]);
        match seek_start(&mut buf, &Start::Pattern(b"b".to_vec())).unwrap() {
            Sought::At(at) => {
                assert_eq!(at.lines, vec![b"a\n".to_vec(), b"b\n".to_vec()]);
                assert!(!at.skipped, "nothing was passed over");
            }
            other => panic!("expected a match, got {other:?}"),
        }
    }

    #[test]
    fn a_pattern_matches_a_line_without_its_newline() {
        // `x$` must match the line `x`, which in the buffer is `x\n`.
        let mut buf = BufReader::new(&b"a\nx\n"[..]);
        assert!(matches!(
            seek_start(&mut buf, &Start::Pattern(b"^x$".to_vec())),
            Ok(Sought::At(_))
        ));
    }

    #[test]
    fn the_pattern_dialect_is_basic() {
        // In a BRE `a+b` is three literal characters; in an ERE it is a
        // repeat. util-linux uses `regcomp` without `REG_EXTENDED`.
        let mut buf = BufReader::new(&b"aab\na+b\n"[..]);
        match seek_start(&mut buf, &Start::Pattern(br"a+b".to_vec())).unwrap() {
            Sought::At(at) => assert_eq!(at.lines.last().map(Vec::as_slice), Some(&b"a+b\n"[..])),
            other => panic!("expected a match, got {other:?}"),
        }
    }

    #[test]
    fn a_pattern_that_matches_nothing_says_so() {
        let mut buf = BufReader::new(&b"a\nb\n"[..]);
        let got = seek_start(&mut buf, &Start::Pattern(b"zzz".to_vec())).unwrap();
        assert_eq!(got, Sought::NotFound);
        assert_eq!(got.notice(), b"Pattern not found");
    }

    #[test]
    fn a_pattern_that_does_not_compile_says_so() {
        // Measured: an unclosed bracket reports this on the screen, then shows
        // the file from the top with status 0.
        let mut buf = BufReader::new(&b"a\n"[..]);
        let got = seek_start(&mut buf, &Start::Pattern(b"[".to_vec())).unwrap();
        assert_eq!(got, Sought::BadPattern);
        assert_eq!(got.notice(), b"Invalid regular expression");
    }

    #[test]
    fn the_held_lines_are_written_before_anything_is_read() {
        let mut buf = BufReader::new(&b"c\nd\n"[..]);
        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = None;
        let opts = Options::new();
        assert!(page(
            &mut buf,
            b"t",
            &mut out,
            &mut keys,
            &screen_of(&opts, 99, 80),
            None,
            Started {
                lines: vec![b"a\n".to_vec(), b"b\n".to_vec()],
                consumed: 4,
                skipped: false,
            },
        ));
        assert_eq!(out, b"a\nb\nc\nd\n");
    }

    #[test]
    fn skipping_is_announced_only_when_there_is_a_screen() {
        let opts = Options::new();
        let held = || Started {
            lines: vec![b"a\n".to_vec()],
            consumed: 2,
            skipped: true,
        };

        let mut out: Vec<u8> = Vec::new();
        let mut keys: Option<Box<dyn Read>> = Some(Box::new(&b" "[..]));
        page(
            &mut BufReader::new(&b""[..]),
            b"t",
            &mut out,
            &mut keys,
            &screen_of(&opts, 99, 80),
            None,
            held(),
        );
        assert_eq!(out, b"...skipping\na\n");

        let mut piped: Vec<u8> = Vec::new();
        let mut none: Option<Box<dyn Read>> = None;
        page(
            &mut BufReader::new(&b""[..]),
            b"t",
            &mut piped,
            &mut none,
            &screen_of(&opts, 99, 80),
            None,
            held(),
        );
        assert_eq!(piped, b"a\n");
    }

    #[test]
    fn highlight_wraps_a_notice_in_reverse_video() {
        assert_eq!(
            highlight(b"Pattern not found"),
            b"\x1b[7mPattern not found\x1b[27m\n"
        );
    }

    #[test]
    fn help_and_version_end_in_a_newline() {
        // A caller that pipes `more --help` into `head` gets whole lines.
        assert!(HELP.ends_with(b"\n"));
        assert!(VERSION.ends_with(b"\n"));
    }

    #[test]
    fn help_names_every_option_the_parser_takes() {
        let text = std::str::from_utf8(HELP).unwrap();
        for (name, _) in LONGS {
            assert!(
                text.contains(&format!("--{name}")),
                "--{name} is undocumented"
            );
        }
        for letter in SHORTS.chars().filter(|c| c.is_ascii_alphanumeric()) {
            assert!(
                text.contains(&format!("-{letter}")),
                "-{letter} is undocumented"
            );
        }
    }
}
