//! ed — the standard line editor.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a file name holding a byte
//! that is not valid UTF-8 — which on this OS is a legal name, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
//!
//! For an *editor* the argv panic was only the visible half. The buffer was a
//! `Vec<String>` filled by `fs::read_to_string`, so a file holding one byte
//! that is not UTF-8 could not be opened at all — the read returned
//! `InvalidData`, which the old `main` could not tell from "no such file" and
//! answered by printing `0` and presenting an **empty buffer**. A subsequent
//! `w` would then have truncated the file to nothing. A line editor that
//! silently empties a file it cannot decode is a data-loss bug, not a
//! diagnostics one, and it is the reason this conversion goes all the way to
//! `Vec<Vec<u8>>` rather than swapping one argv call.
//!
//! Everything is bytes now: argv, the file name to the syscall, the buffer, the
//! substitution, and stdout. GNU `ed` is byte-clean throughout — measured, a
//! name and a content both holding `0x80` round-trip unchanged — so this is
//! fidelity, not local taste.
//!
//! # The nine further defects, in the lines this rewrite replaced
//!
//! Each was measured against GNU ed 1.20.1, and each had no test.
//!
//! 1. **No options at all**, including `--help` and `--version`. The first
//!    argument was taken as the file name whatever it looked like, so `ed -s f`
//!    tried to open a file called `-s`.
//! 2. **Every diagnostic went to stdout and the exit status was always 0.** GNU
//!    splits them: `?` and the `-v` explanation go to **stdout**, the operating
//!    system's own complaint about a file goes to **stderr**, and the status is
//!    0 normal / 1 a command failed / 2 a problem with the input file. A script
//!    that ran `ed` could not tell success from failure.
//! 3. **`,p` printed one line.** `,` is `1,$` and `;` is `.,$`; the old address
//!    parser treated a leading `,` as "no address", so the documented
//!    "`, p` print all lines" printed only the current line. `%` was not
//!    understood either.
//! 4. **`=` printed the line *count*, never the addressed line.** `2=` answered
//!    `3` on a three-line buffer where GNU answers `2`.
//! 5. **An out-of-range or reversed address was silent.** `0p`, `9p` and
//!    `4,2p` all printed nothing and continued; GNU answers `?` and, being
//!    driven from a file, stops.
//! 6. **`s` printed the line it changed.** GNU prints nothing unless the
//!    command carries a `p` suffix — so a script doing `1,$s/…/…/` got the
//!    whole file echoed back at it.
//! 7. **A trailing `\r` was stripped from every line unconditionally**, so a
//!    CRLF file was silently rewritten as LF by `w`. GNU strips it only under
//!    `--strip-trailing-cr`.
//! 8. **A file whose last line had no newline was miscounted** and got no
//!    `Newline appended` notice: `ed` on a 3-byte `abc` printed `3`, where GNU
//!    prints `Newline appended` and then `4`, because 4 bytes is what `w` will
//!    write back.
//! 9. **A command suffix was ignored rather than refused.** `1pX` printed the
//!    line; GNU answers `?` / `Invalid command suffix`.
//!
//! # The two rules that decide almost everything, and are not `isatty`
//!
//! GNU `ed` asks whether **standard input is a regular file** — not whether it
//! is a terminal — and the answer governs two visible behaviours at once:
//!
//! | stdin | `-v` explanation | after an error |
//! |---|---|---|
//! | regular file (`ed f < script`) | prefixed `script, line N: ` | **stops immediately** |
//! | pipe or terminal (`… \| ed f`) | bare sentence | carries on |
//!
//! Measured both ways with the same commands: `printf '9p\n1p\nq\n' \| ed f`
//! prints `?` then `alpha`, and the same bytes in a file print `?` and nothing
//! else. That is why this file uses [`coreutils::filekind`] rather than
//! `is_tty`, and it is also why a missing input file is fatal in one case and
//! not the other — see [`Editor::load`].
//!
//! # What this `ed` does not have
//!
//! The command language is a subset, and the subset is stated here rather than
//! left to be discovered:
//!
//! - No `m`, `t`, `j`, `k` and `'x`, `r`, `e`/`E`, `u`, `x`/`y`, `h`/`H`, `#`,
//!   no `+line` on the command line, no marks. See `known-issues.md` →
//!   `TD-B-ED-IS-MISSING-EIGHT-COMMANDS`. (Not `z` — GNU ed answers `?` to that
//!   one too, so it is not a gap. Not `!command` either: that one is a refusal
//!   rather than a gap — see below.)
//!
//! # Regular expressions
//!
//! `s`, the `/RE/` and `?RE?` addresses, and `g`/`v`/`G`/`V` all run POSIX
//! **basic** regular expressions through [`ere::bre`], the same engine `sed`
//! uses, over bytes rather than characters. `\1`…`\9` and `&` work in a
//! replacement; `//` and `??` repeat the last pattern, and are an error rather
//! than a match-everything when there is no last pattern.
//!
//! Two places where this file has to know about regex syntax itself, rather
//! than handing the text to the engine:
//!
//! - **Finding the closing delimiter is bracket-aware.** `/` inside `[...]` is
//!   an ordinary character, which is what makes `s/[/]/X/` a two-field command
//!   that replaces a slash. See [`read_pattern`], and [`EdError::UnbalancedBrackets`]
//!   for why an unclosed `[` is `ed`'s own diagnostic and not the engine's.
//! - **A match may be abandoned rather than answered** — the engine bounds
//!   backreference search. See [`Editor::matches`].
//!
//! # Deliberate differences from GNU
//!
//! - **A file name in a diagnostic is printed raw**, as GNU ed prints it, and
//!   *not* through [`coreutils::quote`] as the coreutils do. The name in
//!   question is one the user typed on `ed`'s own command line, so the forged-
//!   output risk that the quoting exists for does not arise here; matching the
//!   editor we are compared against is worth more.
//! - **An ambiguous long option lists its candidates**, because that is what
//!   [`coreutils::getopt`] does. GNU ed uses its own argument parser and says
//!   only `option '--=x' is ambiguous`. Every other wording — `invalid option
//!   -- 'Z'`, `unrecognized option '--zz'`, the `Try 'ed --help'` referral —
//!   matches.
//! - **`!command`, and a file name beginning with `!`, are refused**, where GNU
//!   hands the text to a shell and runs it. This is a deliberate omission, not
//!   a gap: see `design-decisions.md` §713. For the *name*, the alternative to
//!   refusing is to treat it as a literal name — which would *write to a file
//!   the user did not ask for*, and silently. All three answer
//!   `Shell access not implemented by this ed`.
//!
//! # How this is checked
//!
//! `scripts/ed-diff.sh` runs 239 cases against GNU ed 1.20.1 inside WSL and
//! compares four things, not the usual three: stdout, stderr, the exit status
//! **and the bytes left on disk**. The fourth is not belt-and-braces — the
//! data-loss bug above agreed with GNU on the first three and disagreed only on
//! the file. Every case appears in the two stdin kinds where the two kinds
//! differ. `OURS=/usr/bin/ed scripts/ed-diff.sh` checks the harness can still
//! tell the two apart: it turns all 8 deliberate differences into `XPASS` and
//! all 8 known-bug cases into `KFIXED`, and nothing else moves.

use coreutils::errmsg::strerror;
use coreutils::filekind;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd::{self, Stream};
use ere::{Regex, StartOfLine, bre};
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::process::ExitCode;

/// `ed`'s usage status is 1 — measured: `ed -Z; echo $?` prints 1.
const ED: Program = Program::new("ed", 1);

/// GNU `ed`'s short options. `-h` is help and `-V` is version, which is the
/// reverse of the coreutils convention and is `ed`'s own.
const SHORT_OPTIONS: &str = "EGhlp:qrsvV";

/// GNU `ed`'s long options, in `--help`'s order.
///
/// `--quiet` and `--silent` are two spellings of one option and are both
/// listed, which is what keeps `--s` ambiguous against `--script` and
/// `--strip-trailing-cr`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("extended-regexp", Takes::Nothing),
    ("traditional", Takes::Nothing),
    ("loose-exit-status", Takes::Nothing),
    ("prompt", Takes::Required),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("restricted", Takes::Nothing),
    ("script", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("strip-trailing-cr", Takes::Nothing),
    ("unsafe-names", Takes::Nothing),
];

/// The column at which `l` folds a line, GNU's `POS_MAX`-independent constant.
///
/// Measured, and it is *not* the terminal width: `COLUMNS=40` changes nothing,
/// and a line of 30 `0x80` bytes breaks after the eighteenth `\200` — 72
/// characters exactly. The break happens *after* a whole escape sequence, never
/// inside one, so a line of 71 `a`s followed by `\200` breaks at column 75.
const LIST_WIDTH: usize = 72;

// ------------------------------------------------------------------ options --

#[derive(Default, Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    /// `-p`: written before every command is read. Bytes, because a prompt may
    /// be any argument at all.
    prompt: Option<Vec<u8>>,
    /// `-l`: exit 0 even when a command failed.
    loose: bool,
    /// `-q`/`--silent`: suppress the diagnostics that go to **stderr**. It does
    /// not touch `?` or the `-v` explanation, which are stdout.
    quiet: bool,
    /// `-r`: a file name may not contain `/`, and there is no shell escape.
    restricted: bool,
    /// `-s`: suppress byte counts. Not `Newline appended`, which GNU prints
    /// under `-s` and under `-q` alike — measured.
    script: bool,
    /// `-v`: print the sentence explaining each `?`.
    verbose: bool,
    strip_cr: bool,
    /// `--unsafe-names`: permit bytes 1–31 in a file name.
    unsafe_names: bool,
    /// `-E`: patterns are POSIX *extended* regular expressions, so `a+`, `a|b`
    /// and `\(…\)`-without-backslashes mean what they do in `egrep`.
    extended: bool,
    /// `-G`/`--traditional`. GNU's compatibility mode touches `G`, `V`, `f`,
    /// `l`, `m`, `t` and `!!`; of those this `ed` has `G`, `V`, `f` and `l`, and
    /// measured against GNU 1.20.1 exactly one of them differs — `l` omits the
    /// trailing `$`. So that is what this flag does here, and the rest of
    /// traditional mode is a no-op because it is already the same.
    traditional: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The options, and the file to open if one was named.
    Run(Options, Option<OsString>),
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("ed (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(opts, file)) => {
            let loose = opts.loose;
            let mut editor = Editor::new(opts);
            let status = editor.load(file.as_deref());
            let status = match status {
                Some(fatal) => fatal,
                None => editor.run(),
            };
            let Editor { out, .. } = editor;
            let earned = if loose { 0 } else { status };
            stdfd::close_stdout("ed", out, ExitCode::from(earned))
        }
        Err(e) => {
            ED.report(&e);
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: ed [OPTION]... [FILE]
Edit FILE as a buffer of lines.

  -h, --help                 display this help and exit
  -V, --version              output version information and exit
  -E, --extended-regexp      use extended regular expressions
  -G, --traditional          run in compatibility mode
  -l, --loose-exit-status    exit with 0 status even if a command fails
  -p, --prompt=STRING        use STRING as an interactive prompt
  -q, --quiet, --silent      suppress diagnostics written to stderr
  -r, --restricted           run in restricted mode
  -s, --script               suppress byte counts
  -v, --verbose              be verbose
      --strip-trailing-cr    strip carriage returns at end of text lines
      --unsafe-names         allow control characters 1-31 in file names

Commands:
  (.)p / (.,.)p    print lines            (.)n   print with line numbers
  (.,.)l           print unambiguously    (.)a   append text after the line
  (.)i             insert text before     (.,.)c change lines
  (.,.)d           delete lines           (.)s/RE/REPL/ substitute
  (1,$)g/RE/CMDS   run CMDS on matches    (1,$)v/RE/CMDS on non-matches
  (1,$)G/RE/        as g, one at a time   (1,$)V/RE/      as v, one at a time
  (1,$)w [FILE]    write to FILE          f [FILE]  show or set the file name
  ($)=             print a line number    q / Q  quit, with or without a warning

Addresses may be a number, '.', '$', '+N', '-N', '/RE/', '?RE?', ',' (1,$),
';' (.,$) or '%'.

Exit status: 0 for a normal exit, 1 for a command that failed, 2 for a
problem with the input file.
"
    .to_string()
}

/// Parse `ed`'s argv.
///
/// # Errors
///
/// An unknown option, one this implementation does not have, or a missing
/// value.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut opts = Options::default();
    let mut file: Option<OsString> = None;

    for item in ED.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'h', _) | Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'l', _) | Opt::Long("loose-exit-status", _) => opts.loose = true,
            Opt::Short(b'p', value) | Opt::Long("prompt", value) => {
                opts.prompt = value.map(|v| os_bytes(&v).into_owned());
            }
            Opt::Short(b'q', _) | Opt::Long("quiet" | "silent", _) => opts.quiet = true,
            Opt::Short(b'r', _) | Opt::Long("restricted", _) => opts.restricted = true,
            Opt::Short(b's', _) | Opt::Long("script", _) => opts.script = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => opts.verbose = true,
            Opt::Long("strip-trailing-cr", _) => opts.strip_cr = true,
            Opt::Long("unsafe-names", _) => opts.unsafe_names = true,
            Opt::Short(b'E', _) | Opt::Long("extended-regexp", _) => opts.extended = true,
            Opt::Short(b'G', _) | Opt::Long("traditional", _) => opts.traditional = true,
            Opt::Short(other, _) => return Err(ED.invalid_option(other)),
            Opt::Long(other, _) => return Err(unimplemented_long(other)),
            // GNU takes the first operand and ignores the rest in silence —
            // measured, `ed a b` opens `a` and says nothing about `b`.
            Opt::Operand(name) => {
                if file.is_none() {
                    file = Some(name.clone());
                }
            }
        }
    }

    Ok(Request::Run(opts, file))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    ED.usage_referring(format!("option '--{name}' is not implemented by this ed"))
}

// ------------------------------------------------------------------- errors --

/// The sentence GNU `ed` prints after `?` when it is being verbose, and the
/// exit status that goes with it.
///
/// The sentences are GNU's, verbatim and measured, because they are the
/// program's whole diagnostic vocabulary: `?` alone says only that something
/// went wrong.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum EdError {
    InvalidAddress,
    InvalidCommandSuffix,
    UnexpectedAddress,
    UnknownCommand,
    NoCurrentFilename,
    NoMatch,
    CannotOpenOutputFile,
    /// A file that *opened* and then would not read — a directory is the
    /// everyday case. GNU distinguishes it from a file that never opened, and
    /// the distinction is visible: see [`Editor::load`].
    CannotReadInputFile,
    ControlCharsInName,
    DirectoryAccessRestricted,
    ShellAccessUnsupported,
    /// An empty pattern — `//`, `??`, or `s//repl/` — with nothing remembered
    /// to repeat.
    NoPreviousPattern,
    /// A `[` in a pattern that the delimiter scan never saw closed. This is
    /// GNU's *own* wording, not the regex library's: `ed` has to know where the
    /// brackets are before it can find the closing delimiter (a `/` inside
    /// `[...]` is an ordinary character, which is what makes `s/[/]/X/` a valid
    /// two-field command), so it detects this before `regcomp` is ever called
    /// and reports it in its own voice.
    UnbalancedBrackets,
    /// The regex engine refused the pattern. The sentence is glibc's, because
    /// that is what GNU `ed` prints here — it hands `regerror`'s text straight
    /// through rather than translating it into one of its own.
    BadPattern(&'static str),
    /// A match was abandoned rather than answered. Not a GNU sentence: GNU's
    /// backtracker has no budget and simply runs. See [`Editor::matches`] for
    /// why "did not match" would have been the dangerous answer.
    PatternTooCostly,
    /// `s///g` where the pattern can only match the empty string at the point
    /// the walk has reached, so no amount of substituting would ever consume a
    /// byte. GNU's sentence, and GNU's judgement: it refuses the command rather
    /// than inventing a rule for how far to skip. See [`substitute_line`].
    InfiniteSubstitutionLoop,
    /// A `g`, `v`, `G` or `V` inside another one's command list. Refused rather
    /// than supported, as GNU does: the inner command would clear and refill the
    /// outer one's marks, so the outer loop would resume against a selection
    /// that is no longer its own.
    NestedGlobal,
    /// `q` on a modified buffer. Status 1: the *command* failed.
    BufferModified,
    /// End of input on a modified buffer. Status 2 — measured: this is a
    /// problem with the input, not with a command, and GNU grades it as one.
    BufferModifiedAtEof,
}

impl EdError {
    fn sentence(self) -> &'static str {
        match self {
            EdError::InvalidAddress => "Invalid address",
            EdError::InvalidCommandSuffix => "Invalid command suffix",
            EdError::UnexpectedAddress => "Unexpected address",
            EdError::UnknownCommand => "Unknown command",
            EdError::NoCurrentFilename => "No current filename",
            EdError::NoMatch => "No match",
            EdError::CannotOpenOutputFile => "Cannot open output file",
            EdError::CannotReadInputFile => "Cannot read input file",
            EdError::ControlCharsInName => "Control characters 1-31 not allowed in file names",
            EdError::DirectoryAccessRestricted => "Directory access restricted",
            EdError::ShellAccessUnsupported => "Shell access not implemented by this ed",
            EdError::NoPreviousPattern => "No previous pattern",
            EdError::UnbalancedBrackets => "Unbalanced brackets ([])",
            EdError::BadPattern(text) => text,
            EdError::PatternTooCostly => "Regular expression match abandoned",
            EdError::InfiniteSubstitutionLoop => "Infinite substitution loop",
            EdError::NestedGlobal => "Cannot nest global commands",
            EdError::BufferModified | EdError::BufferModifiedAtEof => "Warning: buffer modified",
        }
    }

    fn status(self) -> u8 {
        match self {
            EdError::BufferModifiedAtEof => 2,
            _ => 1,
        }
    }
}

// ------------------------------------------------------------------ parsing --

/// How a line is rendered.
///
/// `p`, `n` and `l` are not three alternatives but three *independent* flags,
/// and this is not an implementation convenience — it is what GNU does and it
/// is observable. `1nl` and `1ln` both print `1\talpha$`: the number from `n`
/// and the escaping and the `$` from `l`, in one line. Modelling them as an
/// enum makes the second letter overwrite the first, which prints `alpha$` for
/// one of those two and `1\talpha` for the other, and gets both wrong.
///
/// The command letter contributes the same flags as a suffix letter would, so
/// `1pn` (command `p`, suffix `n`) and `1np` are the same thing. `p` alone
/// carries no flag: it means only "print", which any of the three implies.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(test, derive(Debug))]
struct PrintStyle {
    /// `n`: prefix the line number and a tab.
    numbered: bool,
    /// `l`: escape the unprintable bytes, fold long lines, end with `$`.
    listed: bool,
}

impl PrintStyle {
    /// Plain `p`: no number, no escaping.
    const PLAIN: Self = Self {
        numbered: false,
        listed: false,
    };

    /// The flags a `p`, `n` or `l` letter contributes, wherever it appears.
    fn of(letter: u8) -> Self {
        Self {
            numbered: letter == b'n',
            listed: letter == b'l',
        }
    }

    /// Both sets of flags at once, which is what a command letter and a print
    /// suffix add up to.
    fn with(self, other: Self) -> Self {
        Self {
            numbered: self.numbered || other.numbered,
            listed: self.listed || other.listed,
        }
    }
}

/// What an address counts *from*, before any `+N`/`-N` is applied.
///
/// This exists because of `/RE/`. Every other base — `.`, `$`, a number — can
/// be turned into a line number by a parser that knows only the current line
/// and the buffer's length, which is why the parser used to return an `i64`
/// directly. A search cannot: it has to read the buffer, and it can fail. So
/// the parser now hands back what was *written* and the editor resolves it.
///
/// The split has a second payoff that is not just plumbing. `addr1;addr2` is
/// specified to move `.` to `addr1` before `addr2` is evaluated, so
/// `/a/;/b/` finds the first `b` *after* the first `a`. That is impossible to
/// express when both addresses are resolved by the same pure call; with a
/// symbolic form the editor resolves them in order and the rule falls out.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum AddrBase {
    /// `.`
    Current,
    /// `$`
    Last,
    /// A literal line number.
    Line(i64),
    /// `/RE/` (forward) or `?RE?` (backward). An empty pattern means "the last
    /// one used", which is why this carries the text rather than a compiled
    /// regex — the compile has to be deferred to the editor, which is the only
    /// thing that knows what the last pattern was.
    Search { pattern: Vec<u8>, forward: bool },
}

/// One address expression: a base plus the sum of its `+N` / `-N` terms.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Addr {
    base: AddrBase,
    offset: i64,
}

impl Addr {
    /// An address that is just a line number, which is what most of the tests
    /// and all of the non-search paths produce.
    fn line(n: i64) -> Self {
        Self {
            base: AddrBase::Line(n),
            offset: 0,
        }
    }
}

/// One command line, cut into its parts but not yet validated.
///
/// The two addresses stay `Option` all the way here because the *default* is a
/// property of the command and not of the grammar: with no address `p` means
/// `.`, `=` means `$` and `w` means `1,$`. A parser that filled in one default
/// would make two of the three wrong.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Command {
    first: Option<Addr>,
    second: Option<Addr>,
    /// The range was written with `;` rather than `,`, so `.` moves to the
    /// first address before the second is evaluated.
    semi: bool,
    /// Whether any address text was present, which is what makes `1q` an
    /// `Unexpected address` while `q` is fine.
    addressed: bool,
    /// The command letter, or `b'\n'` for a line that was only an address.
    cmd: u8,
    /// Everything after the command letter.
    rest: Vec<u8>,
}

/// A [`Command`] whose addresses have been turned into line numbers.
///
/// Separate from `Command` because resolving is not a pure operation any more:
/// a `/RE/` address reads the buffer, can fail, and — through `;` — can depend
/// on the address before it. Keeping the two apart is what lets the parser stay
/// a pure function with pure tests.
struct Resolved {
    first: Option<i64>,
    second: Option<i64>,
    addressed: bool,
    cmd: u8,
    rest: Vec<u8>,
}

fn skip_blank(bytes: &[u8], pos: &mut usize) {
    while matches!(bytes.get(*pos), Some(b' ' | b'\t')) {
        *pos = pos.saturating_add(1);
    }
}

/// Read the decimal number at `pos`, or `None` when there is not one.
fn read_number(bytes: &[u8], pos: &mut usize) -> Option<Result<i64, EdError>> {
    if !bytes.get(*pos).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut value: i64 = 0;
    while let Some(&d) = bytes.get(*pos) {
        if !d.is_ascii_digit() {
            break;
        }
        let digit = i64::from(d.wrapping_sub(b'0'));
        value = match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => v,
            // A number too big to be a line is not a line: GNU answers
            // `Invalid address` rather than wrapping into one that exists.
            None => return Some(Err(EdError::InvalidAddress)),
        };
        *pos = pos.saturating_add(1);
    }
    Some(Ok(value))
}

/// Read a delimited pattern — the `RE` of `/RE/`, `?RE?` or `s/RE/…`.
///
/// `pos` is left just past the closing delimiter, or at end of input when the
/// pattern was not closed (which is legal: `1s/a` and `/a` both work).
///
/// Two rules make this more than "scan to the next delimiter", and both are
/// measured:
///
/// * **A `[…]` bracket expression swallows the delimiter.** `s/[/]/X/` turns
///   `a/b` into `aXb` in GNU: the pattern is `[/]`, not the empty string. So
///   this tracks bracket state, including the two positions where a `]` is an
///   ordinary character (`[]…]` and `[^]…]`) and the `[.` / `[=` / `[:`
///   sub-brackets that have their own terminators.
/// * **A `[` that is never closed is `ed`'s own error, not the regex
///   library's.** GNU says `Unbalanced brackets ([])`, which is not one of
///   glibc's sentences — it cannot be, because `ed` has to resolve the brackets
///   to find the end of the pattern in the first place, before there is
///   anything to hand to `regcomp`.
///
/// A `\` escapes the next byte. The backslash is kept unless it was hiding the
/// delimiter, since every other `\x` is the regex engine's to interpret.
///
/// The `bool` is whether a closing delimiter was actually found. It is reported
/// rather than inferred from the last byte because the two are not the same
/// thing: `s/a\/` ends *on* a `/` that was escaped, so the pattern is `a/` and
/// the command has no replacement field at all.
fn read_pattern(bytes: &[u8], pos: &mut usize, delim: u8) -> Result<(Vec<u8>, bool), EdError> {
    let mut out: Vec<u8> = Vec::new();
    // Where in a bracket expression the scan is. `Out` is the ordinary case;
    // `In` counts the members seen so far, because the first one (or the first
    // after a `^`) is the one position where `]` is a member rather than the
    // close; `Class` is inside `[.`, `[=` or `[:`, each of which ends only at
    // its own two-byte terminator.
    enum Br {
        Out,
        In {
            seen: usize,
            negated: bool,
        },
        Class {
            term: u8,
            seen: usize,
            negated: bool,
        },
    }
    let mut br = Br::Out;
    while let Some(&b) = bytes.get(*pos) {
        match br {
            Br::Class {
                term,
                seen,
                negated,
            } => {
                *pos = pos.saturating_add(1);
                out.push(b);
                if b == b']' && out.len() >= 2 && out.get(out.len().wrapping_sub(2)) == Some(&term)
                {
                    br = Br::In { seen, negated };
                }
                continue;
            }
            Br::In { seen, negated } => {
                *pos = pos.saturating_add(1);
                out.push(b);
                if b == b'^' && seen == 0 && !negated {
                    br = Br::In {
                        seen: 0,
                        negated: true,
                    };
                    continue;
                }
                // `[]a]` and `[^]a]`: the leading `]` is a member.
                if b == b']' && seen > 0 {
                    br = Br::Out;
                    continue;
                }
                if b == b'['
                    && matches!(bytes.get(*pos), Some(b'.' | b'=' | b':'))
                    && let Some(&term) = bytes.get(*pos)
                {
                    *pos = pos.saturating_add(1);
                    out.push(term);
                    br = Br::Class {
                        term,
                        seen: seen.saturating_add(1),
                        negated,
                    };
                    continue;
                }
                br = Br::In {
                    seen: seen.saturating_add(1),
                    negated,
                };
                continue;
            }
            Br::Out => {}
        }
        if b == b'\\' {
            *pos = pos.saturating_add(1);
            let Some(&next) = bytes.get(*pos) else {
                out.push(b'\\');
                break;
            };
            // The backslash hides the delimiter and nothing else: `s/a\/b/c/`
            // is a two-field command whose pattern is `a/b`. Every other `\x`
            // is the regex dialect's, so it is passed through intact.
            if next != delim {
                out.push(b'\\');
            }
            out.push(next);
            *pos = pos.saturating_add(1);
            continue;
        }
        if b == delim {
            *pos = pos.saturating_add(1);
            return Ok((out, true));
        }
        *pos = pos.saturating_add(1);
        out.push(b);
        if b == b'[' {
            br = Br::In {
                seen: 0,
                negated: false,
            };
        }
    }
    if !matches!(br, Br::Out) {
        return Err(EdError::UnbalancedBrackets);
    }
    Ok((out, false))
}

/// Whether a `g`/`v` command list continues onto the next input line.
///
/// It does when the line ends with an *odd* number of backslashes, so that
/// `s/a/\\/` — a replacement of one literal backslash — is a complete command
/// and `s/a/b/\` is not. Measured: `g/beta/s/e/E/\` followed by `s/t/T/` runs
/// both, turning `beta` into `bETa`.
fn has_trailing_continuation(body: &[u8]) -> bool {
    let mut run = 0usize;
    for &b in body.iter().rev() {
        if b == b'\\' {
            run = run.saturating_add(1);
        } else {
            break;
        }
    }
    run % 2 == 1
}

/// Split a `g`/`v` command list into the individual commands it runs.
///
/// The continuations have already been turned into newlines by the caller, so
/// this is a split on `\n` — with one exception that is not a formality: an
/// *empty* command in the list means `p`, and so does an empty list, which is
/// just the one-element case of the same rule. Measured against GNU 1.20.1:
/// `g/a/` on a three-line file prints every matching line rather than doing
/// nothing, and `g/a/\` followed by a blank line — a list of two empty
/// commands — prints every matching line *twice*.
///
/// The empty command therefore cannot be handed to `execute`, which would read
/// it as the bare-newline command (`.+1p`, a different thing entirely). It is
/// rewritten to `p` here, at the one place that knows the context.
fn split_command_list(body: &[u8]) -> Vec<Vec<u8>> {
    body.split(|&b| b == b'\n')
        .map(|step| if step.is_empty() { b"p" } else { step }.to_vec())
        .collect()
}

/// One address expression: a base (`.`, `$`, a number, `/RE/`, `?RE?`, or
/// nothing) followed by any number of `+N` / `-N` terms.
///
/// Returns `Ok(None)` when there was no address text at all, which is how the
/// caller knows to use the command's own default. A bare `+` or `-` counts as
/// address text and means ±1 — measured, `-p` on a five-line buffer at line 5
/// prints line 4.
///
/// Nothing here reads the buffer: a search is recorded, not performed. See
/// [`AddrBase`] for why.
fn parse_address(bytes: &[u8], pos: &mut usize) -> Result<Option<Addr>, EdError> {
    skip_blank(bytes, pos);

    let mut base: Option<AddrBase> = match bytes.get(*pos) {
        Some(b'.') => {
            *pos = pos.saturating_add(1);
            Some(AddrBase::Current)
        }
        Some(b'$') => {
            *pos = pos.saturating_add(1);
            Some(AddrBase::Last)
        }
        Some(&d @ (b'/' | b'?')) => {
            *pos = pos.saturating_add(1);
            // An address search need not be closed: `/beta` is a whole
            // command line and finds the next line holding `beta`.
            let (pattern, _closed) = read_pattern(bytes, pos, d)?;
            Some(AddrBase::Search {
                pattern,
                forward: d == b'/',
            })
        }
        _ => match read_number(bytes, pos) {
            Some(n) => Some(AddrBase::Line(n?)),
            None => None,
        },
    };

    let mut offset: i64 = 0;
    loop {
        let mark = *pos;
        skip_blank(bytes, pos);
        let op = match bytes.get(*pos) {
            Some(&op @ (b'+' | b'-')) => op,
            // Put back the blanks: they belong to whatever follows, and for a
            // command letter that is a suffix check that must see them.
            _ => {
                *pos = mark;
                break;
            }
        };
        *pos = pos.saturating_add(1);
        skip_blank(bytes, pos);
        let step = match read_number(bytes, pos) {
            Some(n) => n?,
            None => 1,
        };
        // `+2` with no base at all counts from `.`, and is address text in its
        // own right — which is why `base` is filled in here rather than left
        // `None` for the command's default to claim.
        base = Some(base.unwrap_or(AddrBase::Current));
        let signed = if op == b'+' {
            step
        } else {
            step.checked_neg().ok_or(EdError::InvalidAddress)?
        };
        offset = offset.checked_add(signed).ok_or(EdError::InvalidAddress)?;
    }

    Ok(base.map(|base| Addr { base, offset }))
}

/// The address part of a command line: one address, a range, or nothing.
///
/// Returns the two addresses, whether the separator was `;`, and whether any
/// address text was present at all.
fn parse_addresses(
    bytes: &[u8],
    pos: &mut usize,
) -> Result<(Option<Addr>, Option<Addr>, bool, bool), EdError> {
    skip_blank(bytes, pos);
    if bytes.get(*pos) == Some(&b'%') {
        *pos = pos.saturating_add(1);
        return Ok((
            Some(Addr::line(1)),
            Some(Addr {
                base: AddrBase::Last,
                offset: 0,
            }),
            false,
            true,
        ));
    }

    let first = parse_address(bytes, pos)?;
    skip_blank(bytes, pos);

    let Some(&sep @ (b',' | b';')) = bytes.get(*pos) else {
        let addressed = first.is_some();
        return Ok((first, None, false, addressed));
    };
    *pos = pos.saturating_add(1);
    let semi = sep == b';';

    // `,` runs from line 1, `;` runs from the current line. Both run to `$`
    // unless a second address says otherwise.
    let lo = first.unwrap_or(if semi {
        Addr {
            base: AddrBase::Current,
            offset: 0,
        }
    } else {
        Addr::line(1)
    });
    let hi = parse_address(bytes, pos)?.unwrap_or(Addr {
        base: AddrBase::Last,
        offset: 0,
    });
    Ok((Some(lo), Some(hi), semi, true))
}

fn parse_command(line: &[u8]) -> Result<Command, EdError> {
    let mut pos = 0usize;
    let (first, second, semi, addressed) = parse_addresses(line, &mut pos)?;
    skip_blank(line, &mut pos);

    let cmd = match line.get(pos) {
        Some(&c) => {
            pos = pos.saturating_add(1);
            c
        }
        None => b'\n',
    };
    let rest = line.get(pos..).unwrap_or(&[]).to_vec();

    Ok(Command {
        first,
        second,
        semi,
        addressed,
        cmd,
        rest,
    })
}

/// The optional `p`/`n`/`l` that may follow a command, and the requirement that
/// nothing else does.
///
/// The "nothing else" half is not pedantry: `1p ` — one trailing space — is
/// `Invalid command suffix` in GNU, which is why this takes the rest of the
/// line verbatim rather than trimming it.
fn print_suffix(rest: &[u8]) -> Result<Option<PrintStyle>, EdError> {
    match rest {
        [] => Ok(None),
        [c @ (b'p' | b'n' | b'l')] => Ok(Some(PrintStyle::of(*c))),
        _ => Err(EdError::InvalidCommandSuffix),
    }
}

/// The body of an `s` command: `/pattern/replacement/flags`.
///
/// Any byte may be the delimiter, and a `\` before one makes it literal.
/// Returns `None` when the text is not a substitution at all.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Substitution {
    pattern: Vec<u8>,
    replacement: Vec<u8>,
    global: bool,
    print: Option<PrintStyle>,
}

fn parse_substitute(arg: &[u8]) -> Result<Substitution, EdError> {
    let Some(&delim) = arg.first() else {
        return Err(EdError::InvalidCommandSuffix);
    };
    let mut pos = 1usize;

    // The pattern is scanned by the *bracket-aware* reader and the replacement
    // is not, because they are not the same language: `[` is a metacharacter on
    // the left of an `s` and an ordinary byte on the right. Scanning both the
    // same way would make `s/x/[/` — replace `x` with an open bracket — read
    // its closing delimiter as a bracket member and fail.
    let (pattern, closed) = read_pattern(arg, &mut pos, delim)?;
    if !closed {
        return Err(EdError::InvalidCommandSuffix);
    }

    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut field: Vec<u8> = Vec::new();
    while let Some(&b) = arg.get(pos) {
        if b == b'\\' {
            if let Some(&next) = arg.get(pos.saturating_add(1)) {
                // A backslash hides the delimiter and nothing else, which is
                // what keeps `s/a/b\/c/` a two-field command.
                if next != delim {
                    field.push(b'\\');
                }
                field.push(next);
                pos = pos.saturating_add(2);
                continue;
            }
            field.push(b'\\');
            pos = pos.saturating_add(1);
        } else if b == delim {
            parts.push(std::mem::take(&mut field));
            pos = pos.saturating_add(1);
        } else {
            field.push(b);
            pos = pos.saturating_add(1);
        }
    }
    parts.push(field);

    let replacement = parts.first().cloned().unwrap_or_default();
    let flags = parts.get(1).cloned().unwrap_or_default();
    // `parts` counts the fields *after* the pattern, so a closed replacement
    // leaves two of them and an unclosed one leaves a single field.
    let parts_len = parts.len().saturating_add(1);

    let mut global = false;
    // An `s` whose replacement is *not* closed by a delimiter prints the last
    // line it changed. POSIX puts it as "a <newline> may be used instead of the
    // final delimiter, in which case the last line affected shall be written",
    // and GNU obeys it: `1s/a/A` prints `Alpha` where `1s/a/A/` prints nothing.
    // It is a real convenience at a terminal and a real difference in a script,
    // so it is not something to leave out.
    let mut print = if parts_len < 3 {
        Some(PrintStyle::PLAIN)
    } else {
        None
    };
    for &f in &flags {
        match f {
            b'g' => global = true,
            c @ (b'p' | b'n' | b'l') => {
                print = Some(print.unwrap_or_default().with(PrintStyle::of(c)));
            }
            _ => return Err(EdError::InvalidCommandSuffix),
        }
    }

    Ok(Substitution {
        pattern,
        replacement,
        global,
        print,
    })
}

// ------------------------------------------------------------- pure helpers --

/// Cut a file's bytes into stored lines.
///
/// Returns the lines and whether a final newline had to be invented. Every line
/// is stored *without* its newline, so the file's byte count is the sum of the
/// lengths plus one per line — which is the number `ed` prints, and is why a
/// file with no trailing newline reports one byte more than it occupies.
fn split_lines(content: &[u8], strip_cr: bool) -> (Vec<Vec<u8>>, bool) {
    if content.is_empty() {
        return (Vec::new(), false);
    }
    let appended = content.last() != Some(&b'\n');
    let body = if appended {
        content
    } else {
        content
            .get(..content.len().saturating_sub(1))
            .unwrap_or(&[])
    };
    let mut lines: Vec<Vec<u8>> = body
        .split(|&b| b == b'\n')
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    if strip_cr {
        for line in &mut lines {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
    }
    (lines, appended)
}

/// The byte count `ed` reports for a run of lines: each line plus its newline.
fn byte_count(lines: &[Vec<u8>]) -> usize {
    lines.iter().fold(0usize, |acc, l| {
        acc.saturating_add(l.len()).saturating_add(1)
    })
}

/// Expand one match's worth of an `s` replacement into `out`.
///
/// `&` is the whole match and `\1`…`\9` are the parenthesised groups, both of
/// which are why this needs the spans rather than just the matched text. `\&`
/// is a literal ampersand and `\\` a literal backslash; every other `\x` is the
/// byte `x`, which is what makes `\/` inside a `/`-delimited replacement work.
///
/// A group that did not participate in the match expands to nothing rather than
/// failing. That is not leniency — POSIX specifies an unset group as the empty
/// string, and `s/\(a\)\|b/[\1]/` on a line matching `b` is the ordinary way to
/// reach it.
fn expand_replacement(
    line: &[u8],
    spans: &[Option<(usize, usize)>],
    repl: &[u8],
    out: &mut Vec<u8>,
) {
    let group = |n: usize, out: &mut Vec<u8>| {
        if let Some(&Some((s, e))) = spans.get(n)
            && let Some(text) = line.get(s..e)
        {
            out.extend_from_slice(text);
        }
    };
    let mut i = 0usize;
    while let Some(&b) = repl.get(i) {
        match b {
            b'&' => {
                group(0, out);
                i = i.saturating_add(1);
            }
            b'\\' => match repl.get(i.saturating_add(1)) {
                Some(&d @ b'1'..=b'9') => {
                    group(usize::from(d.wrapping_sub(b'0')), out);
                    i = i.saturating_add(2);
                }
                Some(&next) => {
                    out.push(next);
                    i = i.saturating_add(2);
                }
                // A replacement ending in a lone backslash keeps it. GNU uses a
                // trailing backslash to continue a `g` command list onto the
                // next line, so by the time one reaches here it is literal.
                None => {
                    out.push(b'\\');
                    i = i.saturating_add(1);
                }
            },
            _ => {
                out.push(b);
                i = i.saturating_add(1);
            }
        }
    }
}

/// Replace `re` in `line`, once or everywhere. `Ok(None)` when it never
/// matched, which is what makes `No match` an error rather than a no-op.
///
/// ## The empty-match rule, which is not `sed`'s
///
/// A pattern that can match nothing — `a*`, `^`, `x\?` — has to be handled
/// deliberately or `s///g` never terminates. `sed` answers by advancing one
/// character past an empty match, so `s/x*/-/g` on `abc` yields `-a-b-c-`.
/// **`ed` does not do that**, and copying `sed`'s rule here — which is what
/// this function used to do, via [`Regex::capture_spans_iter`], whose whole
/// contract is that rule — was wrong in a way no test had asked about:
///
/// ```text
/// GNU ed 1.20.1, buffer "alpha":   ,s/a*/X/g   →   ? / Infinite substitution loop
/// this ed, before this was fixed:  ,s/a*/X/g   →   XlXpXhX
/// ```
///
/// GNU's actual loop, measured rather than inferred: it substitutes, advances
/// past what the match consumed, and searches the remainder again with
/// `REG_NOTBOL` set — and if *that* search comes back with an empty match at
/// offset 0 of the remainder, it gives up on the whole command, because a
/// substitution that consumed nothing and left the position unmoved would
/// repeat for ever. The first search is exempt: an empty match at 0 on the
/// first pass is how `,s/^/> /` and `,s/x*/X/` do their jobs.
///
/// `REG_NOTBOL` — [`StartOfLine::No`] here — is the part that cannot be skipped.
/// Without it, `s/^x*/X/g` on `alpha` would find its second empty match at 0
/// and report the loop error, where GNU prints `Xalpha`: the point of the flag
/// is that `^` has already been passed and cannot match again.
///
/// Lines the walk already changed keep their change when a later line raises
/// the error — the caller writes each line back as it goes, which is GNU's
/// behaviour too (`aaa\nbbb` with `,s/a*/X/g` leaves `X` and `bbb`, and reports
/// the buffer as modified).
///
/// # Errors
/// [`EdError::InfiniteSubstitutionLoop`] as above;
/// [`EdError::PatternTooCostly`] if a backreference search ran out of budget.
fn substitute_line(
    line: &[u8],
    re: &Regex,
    replacement: &[u8],
    global: bool,
) -> Result<Option<Vec<u8>>, EdError> {
    let mut out: Vec<u8> = Vec::with_capacity(line.len());
    let mut at = 0usize;
    let mut first = true;
    // `Regex::search` rather than a `capture_spans_at` loop: the latter
    // re-decodes the whole line on every call, which turns one substitution
    // into a quadratic one on exactly the long lines where it would show.
    let subject = re.search(line);
    loop {
        // Byte 0 of the line is a line start only on the first search; after
        // that the walk has passed it. See the doc comment — this is the whole
        // difference between `s/^x*/X/g` working and reporting a loop.
        let bol = if first {
            StartOfLine::Yes
        } else {
            StartOfLine::No
        };
        let found = subject
            .capture_spans_from(at, bol)
            .map_err(|_| EdError::PatternTooCostly)?;
        let Some(spans) = found else { break };
        let Some(&Some((s, e))) = spans.first() else {
            break;
        };
        if !first && s == at && e == at {
            return Err(EdError::InfiniteSubstitutionLoop);
        }
        if let Some(gap) = line.get(at..s) {
            out.extend_from_slice(gap);
        }
        expand_replacement(line, &spans, replacement, &mut out);
        at = e;
        first = false;
        // `at >= line.len()` is GNU's "the remaining text is empty" — the walk
        // stops there rather than searching an empty remainder, which is why
        // `s/x*/X/g` on an empty line substitutes once instead of erroring.
        if !global || at >= line.len() {
            break;
        }
    }
    if first {
        return Ok(None);
    }
    if let Some(tail) = line.get(at..) {
        out.extend_from_slice(tail);
    }
    Ok(Some(out))
}

/// `l`'s rendering: every byte made visible, a `$` marking the end, and a fold
/// at [`LIST_WIDTH`] announced by a trailing `\`.
///
/// The fold is measured in the columns it *prints*, not in the bytes it reads:
/// a line of 36 tabs is 72 printed columns and folds, a line of 36 `a`s does
/// not. And the test is made **before** each escape and never after the last
/// one, with no look-ahead at how wide the next escape will be. That sounds
/// like a detail and is two observable behaviours:
///
/// * 71 `a`s followed by `\200` is 75 columns and does **not** fold — the
///   check that would have folded it happens before an escape that never
///   comes. Folding after the escape instead puts a `\` and a newline in
///   front of the `$` with nothing following them.
/// * 72 `a`s followed by `\200` *does* fold, and the `\200` lands whole on the
///   next line. Deciding the fold by whether the next escape would fit would
///   have folded the 71-`a` line too.
///
/// `COLUMNS` has no effect on any of it; the 72 is fixed. Measured against GNU
/// ed 1.20.1 at every length from 68 to 75, and with tabs and `\ooo` escapes to
/// separate the column rule from the byte rule.
fn list_line(line: &[u8], traditional: bool) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(line.len().saturating_add(2));
    let mut col = 0usize;
    for &b in line {
        if col >= LIST_WIDTH {
            out.extend_from_slice(b"\\\n");
            col = 0;
        }
        let escape: Vec<u8> = match b {
            b'\\' => b"\\\\".to_vec(),
            0x07 => b"\\a".to_vec(),
            0x08 => b"\\b".to_vec(),
            0x0c => b"\\f".to_vec(),
            b'\n' => b"\\n".to_vec(),
            b'\r' => b"\\r".to_vec(),
            b'\t' => b"\\t".to_vec(),
            0x0b => b"\\v".to_vec(),
            b'$' => b"\\$".to_vec(),
            0x20..=0x7e => vec![b],
            _ => format!("\\{b:03o}").into_bytes(),
        };
        out.extend_from_slice(&escape);
        col = col.saturating_add(escape.len());
    }
    // The `$` marks the end of the line so a trailing blank is visible. GNU's
    // `-G` compatibility mode drops it — measured, `ed -sG` on a 100-column line
    // folds it identically and ends without the `$`.
    out.extend_from_slice(if traditional { b"\n" } else { b"$\n" });
    out
}

/// Whether a file name may be used, given the two rules that can forbid one.
fn check_name(name: &[u8], opts: &Options) -> Result<(), EdError> {
    if name.first() == Some(&b'!') {
        return Err(EdError::ShellAccessUnsupported);
    }
    if opts.restricted && name.contains(&b'/') {
        return Err(EdError::DirectoryAccessRestricted);
    }
    if !opts.unsafe_names && name.iter().any(|&b| (1..=31).contains(&b)) {
        return Err(EdError::ControlCharsInName);
    }
    Ok(())
}

// -------------------------------------------------------------------- editor --

/// What a command asked the main loop to do next.
enum Action {
    Continue,
    Quit,
}

struct Editor {
    buffer: Vec<Vec<u8>>,
    /// 1-based; 0 means "before the first line", which is a legal address for
    /// `a` and `i` and for nothing else.
    current: usize,
    filename: Option<Vec<u8>>,
    modified: bool,
    /// Whether `q` has already refused once. Cleared by any other command, so
    /// that `q`, an edit, `q` warns twice.
    warned: bool,
    opts: Options,
    out: Stream,
    /// The exit status earned so far. Sticky: a later success does not clear
    /// an earlier failure — measured, three bad addresses then a good one over
    /// a pipe still exits 1.
    status: u8,
    line_no: u64,
    /// Standard input is a regular file. See the module docs: it decides both
    /// the shape of the `-v` explanation and whether an error is fatal.
    file_driven: bool,
    stdin: std::io::StdinLock<'static>,
    /// The last pattern compiled, which `//`, `??` and `s//repl/` all reuse.
    ///
    /// `Rc` because a caller needs to hold the pattern while it edits the
    /// buffer, and both live on this struct: `s` matches with the regex and
    /// writes back through `&mut self.buffer` in the same loop. Taking a cheap
    /// handle is the alternative to taking the regex out and remembering to put
    /// it back on every exit path, which is the sort of thing that survives
    /// review and then loses the pattern on an error return.
    last_re: Option<std::rc::Rc<Regex>>,
    /// One flag per buffer line, live only for the duration of a `g`/`v`/`G`/`V`
    /// command: "this line was selected and has not been visited yet".
    ///
    /// It is a parallel array rather than a set of line numbers because the
    /// command list renumbers the buffer as it runs. `insert` and `delete` shift
    /// the marks in step with the lines, so a mark keeps pointing at the line it
    /// was set on however much text is added or removed above it. A set of
    /// numbers would have to be rewritten by every edit, and the first one
    /// missed would run the list on the wrong line.
    marks: Vec<bool>,
    /// The command list of a running `g`/`v`/`G`/`V`, as lines still to be
    /// consumed, reversed so the next one is the last element.
    ///
    /// `Some` for exactly as long as a global is running, and it is the *whole*
    /// input source while it is: [`Editor::read_line`] takes from here and
    /// returns `None` at the end of the list rather than falling through to
    /// standard input. That is what makes an `a` inside a command list take its
    /// text from the list and stop when the list does, with no `.` — measured
    /// against GNU, where `g/beta/a\` + `inserted` appends one line.
    ///
    /// It doubles as the nesting guard: a `g` that finds this `Some` refuses.
    global_input: Option<Vec<Vec<u8>>>,
}

impl Editor {
    fn new(opts: Options) -> Self {
        let file_driven = filekind::borrowed_stdin().is_some_and(|f| filekind::is_regular(&f));
        Editor {
            buffer: Vec::new(),
            current: 0,
            filename: None,
            modified: false,
            warned: false,
            opts,
            out: Stream::stdout(),
            status: 0,
            line_no: 0,
            file_driven,
            stdin: std::io::stdin().lock(),
            last_re: None,
            marks: Vec::new(),
            global_input: None,
        }
    }

    /// The regex a command asked for: the one it spelled out, or — for an
    /// empty pattern — the last one used.
    ///
    /// An empty pattern is `//`, `??` or `s//repl/`, and it is an *error* when
    /// nothing has been searched for yet rather than a match-everything.
    /// Measured: GNU says `No previous pattern`.
    fn pattern(&mut self, text: &[u8]) -> Result<std::rc::Rc<Regex>, EdError> {
        if text.is_empty() {
            return self.last_re.clone().ok_or(EdError::NoPreviousPattern);
        }
        let re = if self.opts.extended {
            Regex::new_flags(text, false)
        } else {
            bre::compile(text, false)
        }
        .map_err(|e| EdError::BadPattern(e.message()))?;
        let re = std::rc::Rc::new(re);
        self.last_re = Some(std::rc::Rc::clone(&re));
        Ok(re)
    }

    /// Whether `line` matches, with an abandoned search reported rather than
    /// answered.
    ///
    /// The distinction earns its keep in `v/RE/d`, which deletes every line the
    /// pattern does *not* match. Reporting a search that ran out of budget as
    /// "did not match" would delete the user's lines on the strength of a
    /// question this `ed` declined to answer.
    fn matches(re: &Regex, line: &[u8]) -> Result<bool, EdError> {
        re.is_match(line).map_err(|_| EdError::PatternTooCostly)
    }

    /// The next line matching `re`, searching from `.` and wrapping past the
    /// end of the buffer — which is what GNU does, and is why `/alpha/` finds
    /// line 1 from line 2 of a four-line file rather than failing.
    fn search(&self, re: &Regex, forward: bool) -> Result<usize, EdError> {
        let total = self.total();
        if total == 0 {
            return Err(EdError::NoMatch);
        }
        let from = self.current.min(total);
        for step in 1..=total {
            let raw = if forward {
                from.saturating_add(step).saturating_sub(1)
            } else {
                from.saturating_add(total)
                    .saturating_sub(step)
                    .saturating_sub(1)
            };
            let n = raw.checked_rem(total).unwrap_or(0).saturating_add(1);
            let Some(line) = self.buffer.get(n.saturating_sub(1)) else {
                continue;
            };
            if Self::matches(re, line)? {
                return Ok(n);
            }
        }
        Err(EdError::NoMatch)
    }

    /// Turn one written address into a line number.
    fn resolve(&mut self, a: &Addr) -> Result<i64, EdError> {
        let base = match &a.base {
            AddrBase::Current => i64::try_from(self.current).unwrap_or(i64::MAX),
            AddrBase::Last => i64::try_from(self.total()).unwrap_or(i64::MAX),
            AddrBase::Line(n) => *n,
            AddrBase::Search { pattern, forward } => {
                let re = self.pattern(pattern)?;
                i64::try_from(self.search(&re, *forward)?).unwrap_or(i64::MAX)
            }
        };
        base.checked_add(a.offset).ok_or(EdError::InvalidAddress)
    }

    /// Resolve both of a command's addresses, in order.
    ///
    /// The order is the point. `addr1;addr2` moves `.` to `addr1` before
    /// `addr2` is evaluated, so `/a/;/b/` is "the first `b` at or after the
    /// first `a`" rather than two searches from the same place. `.` is put back
    /// afterwards — including on the error path, because a range whose second
    /// address does not exist must not leave the editor somewhere new.
    fn resolve_command(&mut self, c: Command) -> Result<Resolved, EdError> {
        let first = match &c.first {
            Some(a) => Some(self.resolve(a)?),
            None => None,
        };
        let saved = self.current;
        if c.semi
            && let Some(f) = first
            && let Ok(n) = usize::try_from(f)
        {
            self.current = n.min(self.total());
        }
        let second = match &c.second {
            Some(a) => self.resolve(a).map(Some),
            None => Ok(None),
        };
        self.current = saved;
        Ok(Resolved {
            first,
            second: second?,
            addressed: c.addressed,
            cmd: c.cmd,
            rest: c.rest,
        })
    }

    fn put(&mut self, bytes: &[u8]) {
        // `Stream::write` records rather than returns a failure; the verdict
        // comes from `close_stdout`.
        let _ = self.out.write_all(bytes);
    }

    /// Open the file named on the command line, if there was one.
    ///
    /// `Some(status)` means the session is over before it started, which is
    /// what a *file-driven* `ed` does with an unreadable operand. Over a pipe
    /// or a terminal GNU reports the same thing and carries on with an empty
    /// buffer, leaving the status at 0 — measured both ways.
    fn load(&mut self, file: Option<&std::ffi::OsStr>) -> Option<u8> {
        let file = file?;
        let name = os_bytes(file).into_owned();

        if let Err(e) = check_name(&name, &self.opts) {
            return Some(self.name_refused(&name, e.sentence()));
        }

        // Opening and reading are kept apart because GNU grades them
        // differently, and `fs::read` would fuse them into one error that
        // cannot be told apart afterwards. Measured against GNU ed 1.20.1:
        //
        // | what failed | over a pipe | from a script file |
        // |---|---|---|
        // | the open (`nosuch.txt`, an unreadable file) | one line on stderr, empty buffer, status **0** | one line, exit **2**, no editing |
        // | the read (`ed .`, a directory) | *two* lines — the errno's, then `Cannot read input file` — empty buffer, status **1** | one line, exit **2** |
        //
        // So an unreadable directory is a *command* failure and a missing file
        // is not, which is worth having right: a script that opens a path it
        // did not expect to be a directory otherwise reports success.
        // `opened` has to come from the open call itself, not from a later
        // `metadata` probe: a file with no read permission *exists*, so a
        // probe would call its EACCES a read failure and print a second line
        // GNU does not print.
        let mut opened = false;
        let read = std::fs::File::open(os_from_bytes(&name)).and_then(|mut f| {
            opened = true;
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut content).map(|_| content)
        });

        match read {
            Ok(content) => {
                let (lines, appended) = split_lines(&content, self.opts.strip_cr);
                self.buffer = lines;
                self.current = self.buffer.len();
                self.filename = Some(name);
                // GNU prints this under `-s` and under `-q` alike: it is not a
                // diagnostic, it is a statement about what the buffer now holds
                // and therefore about what `w` will write.
                if appended {
                    self.put(b"Newline appended\n");
                }
                if !self.opts.script {
                    self.put(format!("{}\n", byte_count(&self.buffer)).as_bytes());
                }
                None
            }
            Err(e) => {
                self.filename = Some(name.clone());
                self.complain(&name, &strerror(&e));
                if self.file_driven {
                    return Some(2);
                }
                // The path exists, so this was the read and not the open.
                if opened {
                    self.complain(&name, EdError::CannotReadInputFile.sentence());
                    self.status = self.status.max(EdError::CannotReadInputFile.status());
                }
                None
            }
        }
    }

    fn name_refused(&mut self, name: &[u8], sentence: &str) -> u8 {
        self.complain(name, sentence);
        2
    }

    /// GNU's stderr shape: `<name>: <sentence>`, the name raw, no `ed: `
    /// prefix, and nothing at all under `-q`.
    fn complain(&mut self, name: &[u8], sentence: &str) {
        if self.opts.quiet {
            return;
        }
        let mut line = Vec::with_capacity(name.len().saturating_add(sentence.len()));
        line.extend_from_slice(name);
        line.extend_from_slice(b": ");
        line.extend_from_slice(sentence.as_bytes());
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }

    /// Report an error. Returns whether the session must end.
    fn fail(&mut self, e: EdError) -> bool {
        self.put(b"?\n");
        if self.opts.verbose {
            let mut line = Vec::new();
            if self.file_driven {
                line.extend_from_slice(format!("script, line {}: ", self.line_no).as_bytes());
            }
            line.extend_from_slice(e.sentence().as_bytes());
            line.push(b'\n');
            self.put(&line);
        }
        self.status = e.status();
        self.file_driven
    }

    /// Read one line of input, without its newline. `None` at end of input.
    ///
    /// While a global command list is running that list *is* the input — see
    /// [`Editor::global_input`] — so this returns `None` at the end of the list
    /// and does not reach standard input.
    fn read_line(&mut self) -> Option<Vec<u8>> {
        if let Some(pending) = self.global_input.as_mut() {
            return pending.pop();
        }
        let mut raw: Vec<u8> = Vec::new();
        match self.stdin.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        if raw.last() == Some(&b'\n') {
            raw.pop();
        }
        Some(raw)
    }

    fn run(&mut self) -> u8 {
        loop {
            if let Some(prompt) = self.opts.prompt.clone() {
                self.put(&prompt);
                let _ = self.out.flush();
            }
            let Some(line) = self.read_line() else { break };
            self.line_no = self.line_no.saturating_add(1);
            match self.execute(&line) {
                Ok(Action::Continue) => {}
                Ok(Action::Quit) => return self.status,
                Err(e) => {
                    if self.fail(e) {
                        return self.status;
                    }
                }
            }
        }
        // End of input. An unsaved buffer is a problem with the *input*, not
        // with a command, which is why its status is 2 and `q`'s is 1.
        if self.modified && !self.warned {
            self.fail(EdError::BufferModifiedAtEof);
        }
        self.status
    }

    fn total(&self) -> usize {
        self.buffer.len()
    }

    /// Resolve a command's addresses to a `1..=total` range, or to `0..=total`
    /// where the command accepts "before the first line".
    fn range(
        &self,
        c: &Resolved,
        default: (usize, usize),
        allow_zero: bool,
    ) -> Result<(usize, usize), EdError> {
        let lo = c
            .first
            .unwrap_or(i64::try_from(default.0).unwrap_or(i64::MAX));
        let hi = c.second.unwrap_or(lo);
        let floor = i64::from(!allow_zero);
        let ceiling = i64::try_from(self.total()).unwrap_or(i64::MAX);
        if lo < floor || hi < lo || hi > ceiling {
            return Err(EdError::InvalidAddress);
        }
        let lo = usize::try_from(lo).map_err(|_| EdError::InvalidAddress)?;
        let hi = usize::try_from(hi).map_err(|_| EdError::InvalidAddress)?;
        Ok((lo, hi))
    }

    fn print_range(&mut self, lo: usize, hi: usize, style: PrintStyle) {
        let mut n = lo;
        while n <= hi {
            let Some(line) = self.buffer.get(n.saturating_sub(1)).cloned() else {
                break;
            };
            let mut bytes = Vec::with_capacity(line.len().saturating_add(8));
            // The two flags compose: `n` adds the number, `l` decides how the
            // body is written, and `nl` does both. The number is not part of
            // what `l` folds — GNU prints it separately, so a numbered listing
            // still folds its body at column 72.
            if style.numbered {
                bytes.extend_from_slice(format!("{n}\t").as_bytes());
            }
            if style.listed {
                bytes.extend_from_slice(&list_line(&line, self.opts.traditional));
            } else {
                bytes.extend_from_slice(&line);
                bytes.push(b'\n');
            }
            self.put(&bytes);
            n = n.saturating_add(1);
        }
        self.current = hi.min(self.total());
    }

    /// Read the text of an `a`, `i` or `c` command: lines up to a lone `.`, or
    /// to end of input.
    fn read_text(&mut self) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        while let Some(line) = self.read_line() {
            // Lines taken from a global command list are not input lines, so
            // they do not move the number `-v` reports an error against.
            if self.global_input.is_none() {
                self.line_no = self.line_no.saturating_add(1);
            }
            if line == b"." {
                break;
            }
            lines.push(line);
        }
        lines
    }

    /// The file a `w` or `f` command names, or the current one.
    fn resolve_name(&mut self, rest: &[u8], remember: bool) -> Result<Vec<u8>, EdError> {
        let mut pos = 0usize;
        skip_blank(rest, &mut pos);
        let given = rest.get(pos..).unwrap_or(&[]);
        if given.is_empty() {
            return self.filename.clone().ok_or(EdError::NoCurrentFilename);
        }
        check_name(given, &self.opts)?;
        // GNU sets the default file name from `w FILE` only when there is not
        // one already, so `ed f` then `w backup` still writes `f` next time.
        if remember && self.filename.is_none() {
            self.filename = Some(given.to_vec());
        }
        Ok(given.to_vec())
    }

    fn execute(&mut self, line: &[u8]) -> Result<Action, EdError> {
        let total = self.total();
        let parsed = parse_command(line)?;
        if parsed.cmd != b'q' && parsed.cmd != b'Q' {
            self.warned = false;
        }
        let c = self.resolve_command(parsed)?;

        match c.cmd {
            // A line that was only an address prints that line; an empty line
            // prints the one after the current one.
            b'\n' => {
                let default = self.current.saturating_add(1);
                let (_, hi) = self.range(&c, (default, default), false)?;
                self.print_range(hi, hi, PrintStyle::PLAIN);
                Ok(Action::Continue)
            }

            b'p' | b'n' | b'l' => {
                // The command letter and the suffix letter both contribute, so
                // `1nl` numbers *and* lists. See `PrintStyle`.
                let suffix = print_suffix(&c.rest)?.unwrap_or_default();
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                self.print_range(lo, hi, PrintStyle::of(c.cmd).with(suffix));
                Ok(Action::Continue)
            }

            b'a' | b'i' => {
                let suffix = print_suffix(&c.rest)?;
                let (_, hi) = self.range(&c, (self.current, self.current), true)?;
                // `i` inserts before the addressed line, which is `a` at one
                // less; `0i` and `1i` therefore mean the same thing.
                let at = if c.cmd == b'i' {
                    hi.saturating_sub(1)
                } else {
                    hi
                };
                let text = self.read_text();
                let added = text.len();
                self.insert(at, text);
                self.current = at.saturating_add(added);
                if added > 0 {
                    self.modified = true;
                }
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            b'c' => {
                let suffix = print_suffix(&c.rest)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let text = self.read_text();
                let added = text.len();
                self.delete(lo, hi);
                self.insert(lo.saturating_sub(1), text);
                self.current = lo.saturating_sub(1).saturating_add(added);
                self.modified = true;
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            b'd' => {
                let suffix = print_suffix(&c.rest)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                self.delete(lo, hi);
                self.current = lo.min(self.total());
                self.modified = true;
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            b's' => {
                let sub = parse_substitute(&c.rest)?;
                let re = self.pattern(&sub.pattern)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let mut hit = None;
                let mut n = lo;
                while n <= hi {
                    let idx = n.saturating_sub(1);
                    let replaced = match self.buffer.get(idx) {
                        Some(l) => substitute_line(l, &re, &sub.replacement, sub.global)?,
                        None => None,
                    };
                    if let Some(new_line) = replaced {
                        if let Some(slot) = self.buffer.get_mut(idx) {
                            *slot = new_line;
                        }
                        hit = Some(n);
                        // Marked here rather than after the loop because the
                        // loop can leave through the `?` above: a later line
                        // that raises `Infinite substitution loop` does not
                        // un-change the lines already rewritten, and GNU warns
                        // about the buffer on the way out. Measured: `aaa\nbbb`
                        // with `,s/a*/X/g` errors on line 2 and still refuses a
                        // bare `q`.
                        self.modified = true;
                    }
                    n = n.saturating_add(1);
                }
                let Some(last) = hit else {
                    return Err(EdError::NoMatch);
                };
                self.current = last;
                self.finish_suffix(sub.print);
                Ok(Action::Continue)
            }

            // `g`/`v` run a command list on every (non-)matching line; `G`/`V`
            // are the forms that ask at the terminal instead. All four share
            // one implementation because the only difference is where the
            // command text comes from and whether the match is inverted.
            b'g' | b'v' | b'G' | b'V' => {
                if self.global_input.is_some() {
                    return Err(EdError::NestedGlobal);
                }
                let invert = c.cmd == b'v' || c.cmd == b'V';
                let interactive = c.cmd == b'G' || c.cmd == b'V';
                // An unaddressed global covers the whole buffer, which `range`
                // cannot express on its own: it derives the upper bound from the
                // *lower* default (`hi = c.second.unwrap_or(lo)`), so passing
                // `(1, self.total())` still yields `(1, 1)`. `w` below carries
                // the same shape for the same reason.
                let (lo, hi) = if c.addressed {
                    self.range(&c, (1, self.total()), false)?
                } else {
                    (1, self.total())
                };
                let Some(&delim) = c.rest.first() else {
                    return Err(EdError::InvalidCommandSuffix);
                };
                // A delimiter that could end an address or a line would make the
                // command unparseable, so GNU refuses these outright.
                if delim == b'\n' || delim == b' ' {
                    return Err(EdError::InvalidCommandSuffix);
                }
                let mut pos = 1usize;
                let (pattern, _closed) = read_pattern(&c.rest, &mut pos, delim)?;
                let re = self.pattern(&pattern)?;
                let mut body = c.rest.get(pos..).unwrap_or(&[]).to_vec();
                // The list continues onto the next input line while this one
                // ends with an unescaped backslash. The backslash goes; the
                // newline it hid becomes the separator between two commands.
                while has_trailing_continuation(&body) {
                    body.pop();
                    body.push(b'\n');
                    let Some(next) = self.read_line() else { break };
                    self.line_no = self.line_no.saturating_add(1);
                    body.extend_from_slice(&next);
                }
                self.global(lo, hi, &re, invert, interactive, &body)
            }

            b'w' => {
                let (lo, hi) = if c.addressed {
                    self.range(&c, (1, self.total()), false)?
                } else {
                    (1, self.total())
                };
                let name = self.resolve_name(&c.rest, true)?;
                self.write(&name, lo, hi)?;
                Ok(Action::Continue)
            }

            b'f' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                let name = self.resolve_name(&c.rest, false)?;
                // `f NAME` always sets the name, unlike `w NAME`.
                self.filename = Some(name.clone());
                let mut out = name;
                out.push(b'\n');
                self.put(&out);
                Ok(Action::Continue)
            }

            b'=' => {
                print_suffix(&c.rest)?;
                // The default is `$`, not `.`: `=` on a three-line buffer is 3
                // whatever the current line is, and `2=` is 2.
                let hi = c
                    .second
                    .or(c.first)
                    .unwrap_or(i64::try_from(total).unwrap_or(i64::MAX));
                if hi < 0 || hi > i64::try_from(total).unwrap_or(i64::MAX) {
                    return Err(EdError::InvalidAddress);
                }
                self.put(format!("{hi}\n").as_bytes());
                Ok(Action::Continue)
            }

            b'q' => {
                print_suffix(&c.rest)?;
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                if self.modified && !self.warned {
                    self.warned = true;
                    return Err(EdError::BufferModified);
                }
                Ok(Action::Quit)
            }

            b'Q' => {
                print_suffix(&c.rest)?;
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                Ok(Action::Quit)
            }

            // `!CMD` hands the line to a shell. That is a deliberate omission,
            // not a missing feature — see the module docs and
            // `design-decisions.md` §713 — so it gets the sentence that says so
            // rather than falling through to `Unknown command`, which would
            // suggest the letter was merely unimplemented. `w !CMD` and
            // `r !CMD` already answer the same way via `resolve_name`.
            b'!' => Err(EdError::ShellAccessUnsupported),

            _ => Err(EdError::UnknownCommand),
        }
    }

    /// The `p`/`n`/`l` that may follow a command that is not itself a print.
    fn finish_suffix(&mut self, suffix: Option<PrintStyle>) {
        if let Some(style) = suffix {
            let at = self.current;
            if at >= 1 && at <= self.total() {
                self.print_range(at, at, style);
            }
        }
    }

    /// Insert `text` after 0-based offset `at`.
    fn insert(&mut self, at: usize, text: Vec<Vec<u8>>) {
        let mut at = at.min(self.buffer.len());
        for line in text {
            self.buffer.insert(at, line);
            // A line created during a `g` command list was not one of the lines
            // the pattern selected, so it is not visited. GNU behaves the same
            // way, and the alternative is a `g/x/a` that appends for ever.
            self.marks.insert(at.min(self.marks.len()), false);
            at = at.saturating_add(1);
        }
    }

    /// Delete the 1-based inclusive range `lo..=hi`.
    fn delete(&mut self, lo: usize, hi: usize) {
        let start = lo.saturating_sub(1).min(self.buffer.len());
        let end = hi.min(self.buffer.len());
        if start < end {
            self.buffer.drain(start..end);
            if start < self.marks.len() {
                self.marks.drain(start..end.min(self.marks.len()));
            }
        }
    }

    /// The engine behind `g`, `v`, `G` and `V`.
    ///
    /// The two-pass shape is the whole point and is not an optimisation. A
    /// command list can insert and delete lines, so the line numbers move
    /// underneath it; a one-pass loop that walked `lo..=hi` and tested each
    /// line as it arrived would visit lines the command list created, skip
    /// lines it pushed downwards, and run off the end of a buffer it had
    /// shortened. So every selected line is *marked* first — and the marks are
    /// carried through [`Editor::insert`] and [`Editor::delete`] so they follow
    /// their lines rather than their numbers — and only then is the list run,
    /// taking the first line still marked each time round.
    ///
    /// An error inside the list stops the whole `g`, which is GNU's behaviour
    /// and the safe one: a `g/x/s/a/b/` whose fifth line has no `a` should not
    /// go on quietly editing the sixth.
    fn global(
        &mut self,
        lo: usize,
        hi: usize,
        re: &Regex,
        invert: bool,
        interactive: bool,
        body: &[u8],
    ) -> Result<Action, EdError> {
        self.marks.clear();
        self.marks.resize(self.buffer.len(), false);
        let mut n = lo;
        while n <= hi {
            let idx = n.saturating_sub(1);
            if let Some(line) = self.buffer.get(idx) {
                let hit = Self::matches(re, line)?;
                if hit != invert
                    && let Some(slot) = self.marks.get_mut(idx)
                {
                    *slot = true;
                }
            }
            n = n.saturating_add(1);
        }

        let list = split_command_list(body);
        // The list `G`/`V` will repeat for a lone `&`. Starts as the body, so
        // that `G/RE/p` — a list given on the `G` line itself — is what an `&`
        // at the first prompt repeats.
        let mut remembered = list.clone();
        let mut outcome = Ok(Action::Continue);

        // Driven off the marks rather than off a line range: the command list
        // can insert and delete, so the *next* marked line has to be looked up
        // again after every step rather than counted to.
        while let Some(idx) = self.marks.iter().position(|&m| m) {
            if let Some(slot) = self.marks.get_mut(idx) {
                *slot = false;
            }
            self.current = idx.saturating_add(1);

            let steps = if interactive {
                // `G`/`V` print the line and read one command list for it, from
                // real input. End of input ends the loop, as GNU does.
                self.print_range(self.current, self.current, PrintStyle::PLAIN);
                let Some(reply) = self.read_line() else { break };
                self.line_no = self.line_no.saturating_add(1);
                if reply.is_empty() {
                    // An empty line leaves this line alone. It does *not*
                    // become the remembered list, so a later `&` still repeats
                    // the last list that did something.
                    continue;
                }
                if reply == b"&" {
                    remembered.clone()
                } else {
                    remembered = split_command_list(&reply);
                    remembered.clone()
                }
            } else {
                list.clone()
            };

            // The list is the input source while it runs, so an `a` inside it
            // takes its text from the list. Reversed because `read_line` pops.
            let mut queue = steps;
            queue.reverse();
            self.global_input = Some(queue);
            // Popped from `global_input` rather than iterated, because a step
            // may itself consume the rest of the list — `a` takes its text from
            // there, which is how GNU makes `g/RE/a\` work without a `.`.
            while let Some(step) = self.global_input.as_mut().and_then(Vec::pop) {
                match self.execute(&step) {
                    Ok(Action::Continue) => {}
                    Ok(Action::Quit) => {
                        outcome = Ok(Action::Quit);
                        break;
                    }
                    Err(e) => {
                        outcome = Err(e);
                        break;
                    }
                }
            }
            self.global_input = None;
            if !matches!(outcome, Ok(Action::Continue)) {
                break;
            }
        }
        self.marks.clear();
        outcome
    }

    fn write(&mut self, name: &[u8], lo: usize, hi: usize) -> Result<(), EdError> {
        let mut bytes: Vec<u8> = Vec::new();
        let mut n = lo;
        while n <= hi {
            let Some(line) = self.buffer.get(n.saturating_sub(1)) else {
                break;
            };
            bytes.extend_from_slice(line);
            bytes.push(b'\n');
            n = n.saturating_add(1);
        }
        match std::fs::write(os_from_bytes(name), &bytes) {
            Ok(()) => {
                self.modified = false;
                if !self.opts.script {
                    self.put(format!("{}\n", bytes.len()).as_bytes());
                }
                Ok(())
            }
            Err(e) => {
                let text = strerror(&e);
                let owned = name.to_vec();
                self.complain(&owned, &text);
                Err(EdError::CannotOpenOutputFile)
            }
        }
    }
}

// --------------------------------------------------------------------- tests --

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn lines(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|x| x.as_bytes().to_vec()).collect()
    }

    // ---------------- argv ----------------

    #[test]
    fn no_arguments_is_an_empty_session() {
        let Request::Run(opts, file) = parse_args(&s(&[])).unwrap() else {
            panic!("expected a run")
        };
        assert_eq!(opts, Options::default());
        assert!(file.is_none());
    }

    #[test]
    fn the_first_operand_is_the_file_and_the_rest_are_ignored() {
        // Measured: `ed a b` opens `a` and says nothing at all about `b`.
        let Request::Run(_, file) = parse_args(&s(&["a", "b"])).unwrap() else {
            panic!("expected a run")
        };
        assert_eq!(file, Some(OsString::from("a")));
    }

    #[test]
    fn options_may_follow_the_operand() {
        // `getopt_long` permutes, and so does GNU ed: `ed f -s` is script mode.
        let Request::Run(opts, file) = parse_args(&s(&["f", "-s"])).unwrap() else {
            panic!("expected a run")
        };
        assert!(opts.script);
        assert_eq!(file, Some(OsString::from("f")));
    }

    #[test]
    fn h_is_help_and_v_is_verbose_which_is_eds_own_spelling() {
        assert!(matches!(parse_args(&s(&["-h"])).unwrap(), Request::Help));
        assert!(matches!(parse_args(&s(&["-V"])).unwrap(), Request::Version));
        let Request::Run(opts, _) = parse_args(&s(&["-v"])).unwrap() else {
            panic!("expected a run")
        };
        assert!(opts.verbose, "-v is verbose in ed, not version");
    }

    #[test]
    fn quiet_and_silent_are_one_option() {
        for spell in ["-q", "--quiet", "--silent"] {
            let Request::Run(opts, _) = parse_args(&s(&[spell])).unwrap() else {
                panic!("expected a run")
            };
            assert!(opts.quiet, "{spell}");
        }
    }

    #[test]
    fn the_prompt_is_bytes_not_text() {
        let value = OsString::from("*>");
        let args = vec![OsString::from("-p"), value];
        let Request::Run(opts, _) = parse_args(&args).unwrap() else {
            panic!("expected a run")
        };
        assert_eq!(opts.prompt.as_deref(), Some(&b"*>"[..]));
    }

    #[test]
    fn the_regex_dialect_options_are_accepted() {
        // These were refused while this `ed` had no regex engine, on the
        // grounds that accepting `-E` and matching literally would edit the
        // wrong text in silence. Both now do what they say.
        for spell in ["-E", "--extended-regexp"] {
            let Request::Run(opts, _) = parse_args(&s(&[spell])).unwrap() else {
                panic!("expected a run")
            };
            assert!(opts.extended, "{spell}");
        }
        for spell in ["-G", "--traditional"] {
            let Request::Run(opts, _) = parse_args(&s(&[spell])).unwrap() else {
                panic!("expected a run")
            };
            assert!(opts.traditional, "{spell}");
        }
    }

    #[test]
    fn traditional_mode_drops_the_end_of_line_marker() {
        // The one difference `-G` makes among the commands this ed has.
        // Measured: `ed -sG` folds a 100-column line identically and ends it
        // without the `$`.
        assert_eq!(list_line(b"ab", false), b"ab$\n".to_vec());
        assert_eq!(list_line(b"ab", true), b"ab\n".to_vec());
        let folded = list_line(&[b'a'; 100], true);
        assert!(folded.windows(2).any(|w| w == b"\\\n"), "still folds");
        assert!(folded.ends_with(b"a\n") && !folded.ends_with(b"$\n"));
    }

    #[test]
    fn an_unknown_option_takes_getopts_wording() {
        let e = parse_args(&s(&["-Z"])).unwrap_err();
        assert_eq!(e.sentence, "invalid option -- 'Z'");
        assert_eq!(
            e.message(),
            "invalid option -- 'Z'\nTry 'ed --help' for more information."
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_utf8_survives_argv() {
        use std::os::unix::ffi::OsStringExt;
        let name = OsString::from_vec(vec![b'w', 0x80, b'x']);
        let Request::Run(_, file) = parse_args(&[name.clone()]).unwrap() else {
            panic!("expected a run")
        };
        assert_eq!(file, Some(name));
    }

    // ---------------- addresses ----------------

    /// Parse *and resolve* an address, against a buffer of `total` blank lines
    /// with `.` at `current`.
    ///
    /// Resolution needs a buffer now that `/RE/` is an address, so these tests
    /// go through an `Editor` rather than calling the parser alone. The lines
    /// are blank because no test here searches; the ones that do build their own
    /// buffer with `editor_with`.
    fn resolve_in(line: &str, current: usize, total: usize) -> Result<Resolved, EdError> {
        let mut e = Editor::new(Options::default());
        e.buffer = vec![Vec::new(); total];
        e.current = current;
        let parsed = parse_command(line.as_bytes())?;
        e.resolve_command(parsed)
    }

    fn addr(line: &str, current: usize, total: usize) -> (Option<i64>, Option<i64>, u8) {
        let c = resolve_in(line, current, total).unwrap();
        (c.first, c.second, c.cmd)
    }

    #[test]
    fn a_bare_command_carries_no_address() {
        assert_eq!(addr("p", 3, 9), (None, None, b'p'));
        assert_eq!(addr("", 3, 9), (None, None, b'\n'));
    }

    #[test]
    fn dot_is_the_current_line_and_dollar_is_the_last() {
        assert_eq!(addr(".p", 3, 9), (Some(3), None, b'p'));
        assert_eq!(addr("$p", 3, 9), (Some(9), None, b'p'));
    }

    #[test]
    fn comma_is_one_to_dollar_and_semicolon_is_dot_to_dollar() {
        // The defect this replaces: `,p` printed a single line, because a
        // leading `,` looked like "no address" to the old parser.
        assert_eq!(addr(",p", 3, 9), (Some(1), Some(9), b'p'));
        assert_eq!(addr(";p", 3, 9), (Some(3), Some(9), b'p'));
        assert_eq!(addr("%p", 3, 9), (Some(1), Some(9), b'p'));
        assert_eq!(addr("2,p", 3, 9), (Some(2), Some(9), b'p'));
        assert_eq!(addr(",5p", 3, 9), (Some(1), Some(5), b'p'));
    }

    #[test]
    fn plus_and_minus_are_arithmetic_and_a_bare_one_means_a_step_of_one() {
        // Measured on a five-line buffer at line 5: `-p` prints line 4,
        // `.-2p` prints line 3, `1+2p` prints line 3, `$-1p` prints line 4.
        assert_eq!(addr("-p", 5, 5), (Some(4), None, b'p'));
        assert_eq!(addr("-1p", 5, 5), (Some(4), None, b'p'));
        assert_eq!(addr(".-2p", 5, 5), (Some(3), None, b'p'));
        assert_eq!(addr("1+2p", 5, 5), (Some(3), None, b'p'));
        assert_eq!(addr("$-1p", 5, 5), (Some(4), None, b'p'));
        assert_eq!(addr("+p", 5, 5), (Some(6), None, b'p'));
    }

    #[test]
    fn a_space_before_the_command_is_allowed_and_one_after_is_not() {
        // `1 p` prints; `1p ` is `Invalid command suffix`. Both measured, and
        // together they are why the address parser puts its trailing blanks
        // back rather than eating them.
        assert_eq!(addr("1 p", 1, 9), (Some(1), None, b'p'));
        let c = parse_command(b"1p ").unwrap();
        assert_eq!(print_suffix(&c.rest), Err(EdError::InvalidCommandSuffix));
    }

    #[test]
    fn an_address_too_large_to_hold_is_refused_not_wrapped() {
        let huge = "99999999999999999999p";
        assert_eq!(
            parse_command(huge.as_bytes()).unwrap_err(),
            EdError::InvalidAddress
        );
    }

    #[test]
    fn a_print_suffix_is_one_letter_or_nothing() {
        assert_eq!(print_suffix(b""), Ok(None));
        assert_eq!(print_suffix(b"p"), Ok(Some(PrintStyle::PLAIN)));
        assert_eq!(print_suffix(b"n"), Ok(Some(PrintStyle::of(b'n'))));
        assert_eq!(print_suffix(b"l"), Ok(Some(PrintStyle::of(b'l'))));
        assert_eq!(print_suffix(b"X"), Err(EdError::InvalidCommandSuffix));
        assert_eq!(print_suffix(b"pp"), Err(EdError::InvalidCommandSuffix));
    }

    /// `p`, `n` and `l` are flags that add up, not styles that replace each
    /// other: `1nl` and `1ln` both print `1<tab>alpha$`. Measured against GNU.
    #[test]
    fn the_print_letters_combine_rather_than_override() {
        let n = PrintStyle::of(b'n');
        let l = PrintStyle::of(b'l');
        assert_eq!(n.with(l), l.with(n));
        assert!(n.with(l).numbered && n.with(l).listed);
        // `p` carries no flag of its own, so it neither adds nor removes one.
        assert_eq!(n.with(PrintStyle::of(b'p')), n);
        assert_eq!(PrintStyle::of(b'p'), PrintStyle::PLAIN);
    }

    // ---------------- reading a file ----------------

    #[test]
    fn a_file_ending_in_a_newline_gains_nothing() {
        let (lines, appended) = split_lines(b"a\nb\n", false);
        assert_eq!(lines, vec![b"a".to_vec(), b"b".to_vec()]);
        assert!(!appended);
        assert_eq!(byte_count(&lines), 4);
    }

    #[test]
    fn a_file_without_a_final_newline_gains_one_and_says_so() {
        // GNU on a 3-byte `abc` prints `Newline appended` and then `4` — the
        // count of what `w` would write, not of what is on disk.
        let (lines, appended) = split_lines(b"abc", false);
        assert_eq!(lines, vec![b"abc".to_vec()]);
        assert!(appended);
        assert_eq!(byte_count(&lines), 4);
    }

    #[test]
    fn an_empty_file_is_an_empty_buffer() {
        let (lines, appended) = split_lines(b"", false);
        assert!(lines.is_empty());
        assert!(!appended);
        assert_eq!(byte_count(&lines), 0);
    }

    #[test]
    fn a_trailing_cr_is_kept_unless_asked_for() {
        // The old code stripped it unconditionally, so `w` silently converted
        // a CRLF file to LF. Measured: GNU reports 6 bytes, and 4 under
        // `--strip-trailing-cr`.
        let (kept, _) = split_lines(b"a\r\nb\r\n", false);
        assert_eq!(byte_count(&kept), 6);
        let (stripped, _) = split_lines(b"a\r\nb\r\n", true);
        assert_eq!(stripped, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(byte_count(&stripped), 4);
    }

    #[test]
    fn a_line_may_hold_any_byte() {
        let (lines, _) = split_lines(b"al\x80pha\n", false);
        assert_eq!(lines, vec![b"al\x80pha".to_vec()]);
    }

    #[test]
    fn a_blank_line_between_two_lines_is_kept() {
        let (lines, _) = split_lines(b"a\n\nb\n", false);
        assert_eq!(lines.len(), 3);
        assert!(lines.get(1).unwrap().is_empty());
    }

    // ---------------- substitution ----------------

    /// `substitute_line` against a freshly compiled pattern.
    fn sub(line: &[u8], pattern: &[u8], repl: &[u8], global: bool) -> Option<Vec<u8>> {
        let re = bre::compile(pattern, false).unwrap();
        substitute_line(line, &re, repl, global).unwrap()
    }

    #[test]
    fn substitute_replaces_the_first_occurrence_only() {
        assert_eq!(
            sub(b"foo foo foo", b"foo", b"bar", false),
            Some(b"bar foo foo".to_vec())
        );
    }

    #[test]
    fn substitute_global_replaces_all_of_them() {
        assert_eq!(
            sub(b"foo foo foo", b"foo", b"bar", true),
            Some(b"bar bar bar".to_vec())
        );
    }

    #[test]
    fn substitute_reports_no_match_rather_than_returning_the_line() {
        // The caller turns this into GNU's `No match`, which the old code had
        // no way to produce: it wrote the unchanged line back and marked the
        // buffer modified.
        assert_eq!(sub(b"hello", b"X", b"Y", true), None);
    }

    #[test]
    fn substitute_works_on_bytes_that_are_not_text() {
        // The pattern is a byte, not a character: `\x80` is not valid UTF-8 and
        // has to match anyway, which is why the whole engine is byte-based.
        assert_eq!(sub(b"a\x80b", b"\x80", b"!", false), Some(b"a!b".to_vec()));
    }

    #[test]
    fn substitute_is_a_regular_expression_not_a_literal() {
        // The defect this closes: `s/./X/` replaced a literal full stop.
        assert_eq!(sub(b"abc", b".", b"X", false), Some(b"Xbc".to_vec()));
        assert_eq!(sub(b"abc", b"^a", b"X", false), Some(b"Xbc".to_vec()));
        assert_eq!(sub(b"aaab", b"a*", b"X", false), Some(b"Xb".to_vec()));
        assert_eq!(
            sub(b"ab", b"\\(a\\)b", b"[\\1]", false),
            Some(b"[a]".to_vec())
        );
        assert_eq!(sub(b"ab", b"a", b"<&>", false), Some(b"<a>b".to_vec()));
    }

    /// `substitute_line`'s error, for the cases that are meant to have one.
    fn sub_err(line: &[u8], pattern: &[u8], repl: &[u8], global: bool) -> EdError {
        let re = bre::compile(pattern, false).unwrap();
        substitute_line(line, &re, repl, global).unwrap_err()
    }

    #[test]
    fn a_global_substitution_applies_an_empty_match_once_and_then_stops() {
        // `,s/^/> /` prefixes the line exactly once: the empty match at 0 is
        // applied, and the second search cannot match `^` again because byte 0
        // is no longer a line start. Same for `$`, from the other end.
        assert_eq!(sub(b"ab", b"^", b"> ", true), Some(b"> ab".to_vec()));
        assert_eq!(sub(b"ab", b"$", b" <", true), Some(b"ab <".to_vec()));
        assert_eq!(sub(b"alpha", b"^x*", b"X", true), Some(b"Xalpha".to_vec()));
        // A pattern that consumes something on the first pass and can only
        // match empty afterwards is fine too, as long as the empty match is
        // ruled out by `^`.
        assert_eq!(sub(b"alpha", b"^a*", b"X", true), Some(b"Xlpha".to_vec()));
        // An empty line: one substitution, and the walk stops because there is
        // no remaining text to search.
        assert_eq!(sub(b"", b"x*", b"X", true), Some(b"X".to_vec()));
        // Consuming the whole line likewise ends the walk rather than searching
        // an empty remainder.
        assert_eq!(sub(b"aaa", b"a*", b"X", true), Some(b"X".to_vec()));
        assert_eq!(sub(b"b", b"b*", b"X", true), Some(b"X".to_vec()));
    }

    #[test]
    fn a_global_substitution_that_cannot_advance_is_refused() {
        // GNU 1.20.1's rule, measured: an empty match at offset 0 of the
        // *remaining* text, on any pass after the first, ends the command.
        // Copying `sed`'s rule instead — skip a character and carry on — gave
        // `,s/a*/X/g` on `alpha` the answer `XlXpXhX`, which GNU never prints.
        for (line, pat) in [
            (&b"alpha"[..], &b"a*"[..]),
            (b"alpha", b"b*"),
            (b"alpha", b"a*b*"),
            (b"alpha", b"\\(a\\)*"),
            (b"alpha", b"a\\{0,\\}"),
            // Leftmost-empty at 0 even though a longer match exists later, and
            // consumed-then-empty: both are the same refusal.
            (b"ab", b"b*"),
            (b"ba", b"b*"),
        ] {
            assert_eq!(
                sub_err(line, pat, b"X", true),
                EdError::InfiniteSubstitutionLoop,
                "{}",
                String::from_utf8_lossy(pat)
            );
        }
        // Without `g` there is only ever one pass, so none of them can loop.
        assert_eq!(sub(b"alpha", b"a*", b"X", false), Some(b"Xlpha".to_vec()));
        assert_eq!(sub(b"alpha", b"b*", b"X", false), Some(b"Xalpha".to_vec()));
    }

    #[test]
    fn parse_sub_takes_any_delimiter_and_an_escaped_one_is_literal() {
        let p = parse_substitute(b"/foo/bar/").unwrap();
        assert_eq!(p.pattern, b"foo".to_vec());
        assert_eq!(p.replacement, b"bar".to_vec());
        assert!(!p.global);
        let p = parse_substitute(b"|foo|bar|").unwrap();
        assert_eq!(p.pattern, b"foo".to_vec());
        let p = parse_substitute(br"/a\/b/c/").unwrap();
        assert_eq!(p.pattern, b"a/b".to_vec());
        assert_eq!(p.replacement, b"c".to_vec());
    }

    #[test]
    fn parse_sub_reads_the_g_and_print_flags() {
        assert!(parse_substitute(b"/x/y/g").unwrap().global);
        assert_eq!(
            parse_substitute(b"/x/y/p").unwrap().print,
            Some(PrintStyle::PLAIN)
        );
        assert_eq!(
            parse_substitute(b"/x/y/gl").unwrap().print,
            Some(PrintStyle::of(b'l'))
        );
        // A closed replacement prints nothing unless a flag says to.
        assert_eq!(parse_substitute(b"/x/y/").unwrap().print, None);
        // An *unclosed* one prints the last line it changed — POSIX's "a
        // <newline> may be used instead of the final delimiter". Measured:
        // `1s/a/A` prints `Alpha` where `1s/a/A/` prints nothing.
        assert_eq!(
            parse_substitute(b"/x/y").unwrap().print,
            Some(PrintStyle::PLAIN)
        );
        assert_eq!(
            parse_substitute(b"/x/y/z"),
            Err(EdError::InvalidCommandSuffix)
        );
    }

    #[test]
    fn parse_sub_needs_two_fields() {
        assert_eq!(parse_substitute(b""), Err(EdError::InvalidCommandSuffix));
        assert_eq!(
            parse_substitute(b"/foo"),
            Err(EdError::InvalidCommandSuffix)
        );
    }

    // ---------------- l ----------------

    #[test]
    fn list_escapes_the_bytes_a_terminal_would_eat() {
        // Measured: `a\tb\\c$d\200e` lists as `a\tb\\c\$d\200e$`.
        assert_eq!(
            list_line(b"a\tb\\c$d\x80e", false),
            br"a\tb\\c\$d\200e$"
                .to_vec()
                .into_iter()
                .chain([b'\n'])
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn list_folds_at_seventy_two_columns() {
        // Eighteen four-character escapes make exactly 72, and the break comes
        // after the eighteenth.
        let out = list_line(&[0x80u8; 30], false);
        let text = String::from_utf8(out).unwrap();
        let first = text.lines().next().unwrap();
        assert_eq!(first.len(), 73, "72 columns then the announcing backslash");
        assert!(first.ends_with('\\'));
    }

    #[test]
    fn list_never_folds_inside_an_escape() {
        // 71 `a`s then `\200` is 75 columns: the escape goes out whole, past
        // the margin, because the fold is decided before an escape and there
        // is no escape after this one. Measured — GNU prints this on one line.
        let mut line = vec![b'a'; 71];
        line.push(0x80);
        let out = String::from_utf8(list_line(&line, false)).unwrap();
        assert_eq!(out, format!("{}\\200$\n", "a".repeat(71)));

        // One more `a` in front and the fold does happen — after 72 columns,
        // with the escape landing whole on the second line.
        let mut line = vec![b'a'; 72];
        line.push(0x80);
        let out = String::from_utf8(list_line(&line, false)).unwrap();
        assert_eq!(out, format!("{}\\\n\\200$\n", "a".repeat(72)));

        // And the margin is columns, not bytes: 36 tabs are 36 bytes and 72
        // printed columns, so a 37th tab starts a new line.
        let out = String::from_utf8(list_line(&[b'\t'; 37], false)).unwrap();
        assert_eq!(out, format!("{}\\\n\\t$\n", r"\t".repeat(36)));
    }

    /// The exact lengths at which GNU starts and stops folding a plain run.
    #[test]
    fn the_fold_margin_is_exactly_seventy_two() {
        for n in [68usize, 69, 70, 71, 72] {
            let out = String::from_utf8(list_line(&vec![b'a'; n], false)).unwrap();
            assert_eq!(
                out,
                format!("{}$\n", "a".repeat(n)),
                "n={n} should not fold"
            );
        }
        for n in [73usize, 74, 75] {
            let out = String::from_utf8(list_line(&vec![b'a'; n], false)).unwrap();
            let tail = "a".repeat(n.saturating_sub(72));
            assert_eq!(out, format!("{}\\\n{tail}$\n", "a".repeat(72)), "n={n}");
        }
    }

    #[test]
    fn list_marks_the_end_of_every_line() {
        assert_eq!(list_line(b"", false), b"$\n".to_vec());
    }

    // ---------------- file names ----------------

    #[test]
    fn a_control_character_in_a_name_is_refused_by_default() {
        let plain = Options::default();
        assert_eq!(
            check_name(b"ct\x01rl", &plain),
            Err(EdError::ControlCharsInName)
        );
        let unsafe_ok = Options {
            unsafe_names: true,
            ..Options::default()
        };
        assert_eq!(check_name(b"ct\x01rl", &unsafe_ok), Ok(()));
    }

    #[test]
    fn a_byte_that_is_not_text_is_a_perfectly_good_name() {
        // The whole point of the rewrite: 0x80 is not a control character and
        // not a separator, so nothing may refuse it.
        assert_eq!(check_name(b"we\x80ird", &Options::default()), Ok(()));
    }

    #[test]
    fn restricted_mode_refuses_a_name_with_a_separator() {
        let r = Options {
            restricted: true,
            ..Options::default()
        };
        assert_eq!(
            check_name(b"./f", &r),
            Err(EdError::DirectoryAccessRestricted)
        );
        assert_eq!(check_name(b"f", &r), Ok(()));
    }

    #[test]
    fn a_name_beginning_with_a_bang_is_a_shell_command_we_do_not_run() {
        assert_eq!(
            check_name(b"!wc -l", &Options::default()),
            Err(EdError::ShellAccessUnsupported)
        );
    }

    // ---------------- statuses ----------------

    #[test]
    fn an_unsaved_buffer_at_end_of_input_is_graded_worse_than_a_failed_quit() {
        // Both print `Warning: buffer modified`; GNU exits 1 for the command
        // and 2 for the end of input, because the second is a statement about
        // the input rather than about a command.
        assert_eq!(EdError::BufferModified.status(), 1);
        assert_eq!(EdError::BufferModifiedAtEof.status(), 2);
        assert_eq!(
            EdError::BufferModified.sentence(),
            EdError::BufferModifiedAtEof.sentence()
        );
    }

    #[test]
    fn every_sentence_is_gnus() {
        assert_eq!(EdError::InvalidAddress.sentence(), "Invalid address");
        assert_eq!(
            EdError::InvalidCommandSuffix.sentence(),
            "Invalid command suffix"
        );
        assert_eq!(EdError::UnexpectedAddress.sentence(), "Unexpected address");
        assert_eq!(EdError::UnknownCommand.sentence(), "Unknown command");
        assert_eq!(EdError::NoCurrentFilename.sentence(), "No current filename");
        assert_eq!(EdError::NoMatch.sentence(), "No match");
        assert_eq!(
            EdError::ControlCharsInName.sentence(),
            "Control characters 1-31 not allowed in file names"
        );
        assert_eq!(
            EdError::DirectoryAccessRestricted.sentence(),
            "Directory access restricted"
        );
        assert_eq!(
            EdError::InfiniteSubstitutionLoop.sentence(),
            "Infinite substitution loop"
        );
        assert_eq!(
            EdError::NestedGlobal.sentence(),
            "Cannot nest global commands"
        );
        assert_eq!(EdError::NoPreviousPattern.sentence(), "No previous pattern");
        assert_eq!(
            EdError::UnbalancedBrackets.sentence(),
            "Unbalanced brackets ([])"
        );
    }

    // ---------------- buffer edits ----------------

    fn editor_with(lines_in: &[&str]) -> Editor {
        let mut e = Editor::new(Options::default());
        e.buffer = lines(lines_in);
        e.current = e.buffer.len();
        e
    }

    #[test]
    fn insert_at_zero_is_a_prepend_and_at_the_end_an_append() {
        let mut e = editor_with(&["a", "b"]);
        e.insert(0, lines(&["X", "Y"]));
        assert_eq!(e.buffer, lines(&["X", "Y", "a", "b"]));
        e.insert(4, lines(&["Z"]));
        assert_eq!(e.buffer, lines(&["X", "Y", "a", "b", "Z"]));
    }

    #[test]
    fn delete_removes_an_inclusive_range() {
        let mut e = editor_with(&["a", "b", "c", "d"]);
        e.delete(2, 3);
        assert_eq!(e.buffer, lines(&["a", "d"]));
        e.delete(1, 2);
        assert!(e.buffer.is_empty());
    }

    #[test]
    fn a_range_is_checked_against_the_buffer() {
        let mut e = editor_with(&["a", "b", "c"]);
        let ok = e.resolve_command(parse_command(b"1,2p").unwrap()).unwrap();
        assert_eq!(e.range(&ok, (3, 3), false), Ok((1, 2)));
        // Reversed, past the end, and zero: all three were silent before.
        let rev = e.resolve_command(parse_command(b"2,1p").unwrap()).unwrap();
        assert_eq!(e.range(&rev, (3, 3), false), Err(EdError::InvalidAddress));
        let past = e.resolve_command(parse_command(b"9p").unwrap()).unwrap();
        assert_eq!(e.range(&past, (3, 3), false), Err(EdError::InvalidAddress));
        let zero = e.resolve_command(parse_command(b"0p").unwrap()).unwrap();
        assert_eq!(e.range(&zero, (3, 3), false), Err(EdError::InvalidAddress));
        // …but zero is where `a` and `i` put text before the first line.
        assert_eq!(e.range(&zero, (3, 3), true), Ok((0, 0)));
    }

    /// Run one command line against a fresh buffer and hand back what is left.
    ///
    /// The editor must not outlive the call: `Editor::new` takes `stdin().lock()`,
    /// so two live editors in one thread deadlock — and `let e = …` twice in a
    /// row *is* two live editors, because shadowing does not drop.
    fn after(lines_in: &[&str], cmd: &[u8]) -> Vec<Vec<u8>> {
        let mut e = editor_with(lines_in);
        assert!(e.execute(cmd).is_ok());
        std::mem::take(&mut e.buffer)
    }

    #[test]
    fn an_unaddressed_global_covers_the_whole_buffer() {
        // The bug this guards against: `range` derives its upper bound from the
        // *lower* default, so a `g` that handed it `(1, total)` still came back
        // with `(1, 1)` and only ever visited the first line.
        let all = ["alpha", "beta", "gamma"];
        assert!(after(&all, b"g/a/d").is_empty());
        // `v` is the same loop with the match inverted, so — every line here
        // containing an `a` — nothing is selected and the buffer survives.
        assert_eq!(after(&all, b"v/a/d"), lines(&all));
        // An explicit range still narrows it.
        assert_eq!(after(&all, b"2,3g/a/d"), lines(&["alpha"]));
        // And the marks travel with the lines rather than with their numbers:
        // deleting line 1 must not turn line 2 into an unvisited line 1 again.
        assert_eq!(
            after(&all, b"g/a/s/a/X/"),
            lines(&["Xlpha", "betX", "gXmma"])
        );
    }

    #[test]
    fn a_global_inside_a_global_is_refused() {
        let mut e = editor_with(&["alpha", "beta"]);
        assert!(matches!(
            e.execute(b"g/a/g/b/d"),
            Err(EdError::NestedGlobal)
        ));
    }

    #[test]
    fn an_empty_global_command_list_prints() {
        // Measured against GNU: `g/a/` with nothing after the delimiter runs
        // `p`, and `g/a/\` plus a blank line — two empty commands — runs it
        // twice. So an empty command is `p`, not the bare-newline command.
        assert_eq!(split_command_list(b""), vec![b"p".to_vec()]);
        assert_eq!(
            split_command_list(b"\n"),
            vec![b"p".to_vec(), b"p".to_vec()]
        );
        assert_eq!(
            split_command_list(b"s/a/X/\n"),
            vec![b"s/a/X/".to_vec(), b"p".to_vec()]
        );
        assert_eq!(
            split_command_list(b"s/a/X/\ns/b/Y/"),
            vec![b"s/a/X/".to_vec(), b"s/b/Y/".to_vec()]
        );
    }

    #[test]
    fn a_command_list_continues_on_an_odd_number_of_backslashes() {
        // `s/a/b/\` is unfinished; `s/a/\\/` is a complete command whose
        // replacement happens to be a backslash.
        assert!(has_trailing_continuation(b"s/a/b/\\"));
        assert!(!has_trailing_continuation(b"s/a/\\\\/"));
        assert!(!has_trailing_continuation(b"s/a/b/\\\\"));
        assert!(!has_trailing_continuation(b"p"));
        assert!(has_trailing_continuation(b"\\\\\\"));
    }
}
