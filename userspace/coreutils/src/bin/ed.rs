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
//! The command language is now complete — every letter GNU ed 1.20.1 accepts,
//!
//! ```text
//! a c d e E f g G h H i j k l m n p P q Q r s t u v V w W x y z = # !
//! ```
//!
//! is implemented, except `!`, which is a *refusal* rather than a gap (see
//! below). What is left is one command-line feature: no `+line` operand.
//!
//! Getting there took two passes, and the lesson from the second is worth
//! keeping. The first worked from the list of commands *this file* was missing,
//! which is not the same as the list GNU *has*; the second swept GNU's own set
//! a letter at a time and turned up seven more. One of those seven, `z`, had
//! actually been tried and written off as "GNU answers `?` to that one too" —
//! because it was tried with `.` at the end of the buffer, where `z`'s default
//! address `.+1` is out of range and `?` is an entirely ordinary `Invalid
//! address`. A `?` from a command that does not exist and a `?` from one that
//! does are the same `?`, which is the whole reason `h` and `H` are worth
//! having, and is an argument for measuring a command from more than one
//! starting state.
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
//! `scripts/ed-diff.sh` runs 507 cases against GNU ed 1.20.1 inside WSL and
//! compares four things, not the usual three: stdout, stderr, the exit status
//! **and the bytes left on disk**. The fourth is not belt-and-braces — the
//! data-loss bug above agreed with GNU on the first three and disagreed only on
//! the file. Every case appears in the two stdin kinds where the two kinds
//! differ. `OURS=/usr/bin/ed scripts/ed-diff.sh` checks the harness can still
//! tell the two apart: it turns all 8 deliberate differences into `XPASS`, and
//! nothing else moves.
//!
//! It is the harness, not the unit tests, that has found every substantive
//! disagreement here. Five rules in this file were *wrong in a way that reads
//! correctly* until it ran: a line that `m` moves keeps its `g` selection (it
//! does not), a global that changes nothing leaves the previous undo record
//! alone (it does not — the global clears it on entry), `u` restores only the
//! buffer (it restores `.` too, to where it was before the whole global), a
//! file name may run straight onto its command letter (`efive.txt` is
//! `Unexpected command suffix`), and `z` pages from `.+1` (everywhere except
//! inside a `g` list, where it pages from `.`). None of the five is visible
//! from GNU's documentation, and three of them are invisible from GNU's source
//! unless you already know which function to read.

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
    /// A file-naming command — `e`, `E`, `f`, `r`, `w`, `W` — whose command
    /// letter is followed immediately by something other than whitespace, so
    /// `efive.txt` and `$rf.txt` are refused where `e five.txt` and `$r f.txt`
    /// work. A separate sentence from [`EdError::InvalidCommandSuffix`], and
    /// GNU means the difference: this one says "that is a suffix and I did not
    /// expect one", the other "that is a suffix and it is not one I know".
    UnexpectedCommandSuffix,
    UnexpectedAddress,
    UnknownCommand,
    NoCurrentFilename,
    NoMatch,
    CannotOpenOutputFile,
    /// A file that *opened* and then would not read — a directory is the
    /// everyday case. GNU distinguishes it from a file that never opened, and
    /// the distinction is visible: see [`Editor::load`].
    CannotReadInputFile,
    /// A file that would not open at all, which is what `r nosuch.txt` and
    /// `e nosuch.txt` report. At *startup* the same failure prints only the
    /// OS's own line and no sentence — see [`Editor::load`] — which is why the
    /// two are separate variants rather than one.
    CannotOpenInputFile,
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
    /// `m` asked to move a range to a line inside itself. GNU's test is
    /// `lo <= dest < hi`, so `1,2m1` is refused and `1,2m2` — which is where the
    /// range already ends — is a permitted no-op. There is no such error for
    /// `t`: copying a range into itself is well defined, and GNU does it.
    InvalidDestination,
    /// A `k` whose mark is not one of `a`–`z`. Note that `k` with *no* mark at
    /// all is `Invalid command suffix` instead, because there is no character
    /// there to call invalid — measured both ways.
    InvalidMarkCharacter,
    /// `u` with no change recorded. Every buffer-modifying command replaces the
    /// record, so this is "the last command did not change anything", not "the
    /// session has changed nothing".
    NothingToUndo,
    /// `x` with an empty cut buffer. Only `d`, `c`, `j`, `s` and `y` fill it,
    /// so this is what `1m$` then `x` says in a fresh session.
    NothingToPut,
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
            EdError::UnexpectedCommandSuffix => "Unexpected command suffix",
            EdError::UnexpectedAddress => "Unexpected address",
            EdError::UnknownCommand => "Unknown command",
            EdError::NoCurrentFilename => "No current filename",
            EdError::NoMatch => "No match",
            EdError::CannotOpenOutputFile => "Cannot open output file",
            EdError::CannotReadInputFile => "Cannot read input file",
            EdError::CannotOpenInputFile => "Cannot open input file",
            EdError::ControlCharsInName => "Control characters 1-31 not allowed in file names",
            EdError::DirectoryAccessRestricted => "Directory access restricted",
            EdError::ShellAccessUnsupported => "Shell access not implemented by this ed",
            EdError::NoPreviousPattern => "No previous pattern",
            EdError::UnbalancedBrackets => "Unbalanced brackets ([])",
            EdError::BadPattern(text) => text,
            EdError::PatternTooCostly => "Regular expression match abandoned",
            EdError::InfiniteSubstitutionLoop => "Infinite substitution loop",
            EdError::NestedGlobal => "Cannot nest global commands",
            EdError::InvalidDestination => "Invalid destination",
            EdError::InvalidMarkCharacter => "Invalid mark character",
            EdError::NothingToUndo => "Nothing to undo",
            EdError::NothingToPut => "Nothing to put",
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
    /// `'x` — wherever the line marked `x` by a `k` command has got to. Carries
    /// the letter, not a line number: the marked line moves as text is inserted
    /// and deleted above it, so only the editor can say where it is now.
    Mark(u8),
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
        Some(b'\'') => {
            *pos = pos.saturating_add(1);
            let Some(&letter) = bytes.get(*pos) else {
                return Err(EdError::InvalidMarkCharacter);
            };
            if !letter.is_ascii_lowercase() {
                return Err(EdError::InvalidMarkCharacter);
            }
            *pos = pos.saturating_add(1);
            Some(AddrBase::Mark(letter))
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

/// One line in transit between [`Editor::cut`] and [`Editor::paste`], carrying
/// the `k` marks that have to travel with it.
///
/// GNU keeps the buffer as a linked list and the `k` marks as pointers *into*
/// it, so a move that relinks a node carries its marks for free. Here the
/// buffer is a `Vec` and the marks are a parallel array, which is the right
/// shape for every other operation and the wrong one for exactly this — so a
/// move has to lift the two apart and put them back together, and this is the
/// value that keeps them together in between.
///
/// The `g`/`v` *selection* mark deliberately does **not** travel:
/// [`Editor::cut`] drops it and [`Editor::paste`] inserts a clear one. GNU does
/// the same by a different route — its `move_lines` calls `unset_active_nodes`
/// over the moved range — and the difference is observable. In GNU,
/// `g/^\(one\|four\)$/4m0p` over `one two three four five` runs its list
/// **once**: the move deselects `four`, which was the second selected line.
/// Carrying the mark would run it twice.
struct Taken {
    text: Vec<u8>,
    /// The 26-bit set of `k` marks on the line, `a` in bit 0.
    kmark: u32,
}

/// Everything `u` puts back.
///
/// The marks travel with the buffer because they are *part* of it: `1ka`, `1d`,
/// `u`, `'ap` prints `alpha` in GNU, so undoing a delete has to bring back the
/// mark the delete cleared. `modified` travels too — `1d`, `w`, `u`, `q` exits
/// 0 rather than warning, because the state `u` restored is one that had been
/// written.
struct Snapshot {
    buffer: Vec<Vec<u8>>,
    marks: Vec<bool>,
    kmarks: Vec<u32>,
    current: usize,
    modified: bool,
}

struct Editor {
    buffer: Vec<Vec<u8>>,
    /// 1-based; 0 means "before the first line", which is a legal address for
    /// `a` and `i` and for nothing else.
    current: usize,
    filename: Option<Vec<u8>>,
    modified: bool,
    /// Whether a `q` or an `e` has already been refused with `Warning: buffer
    /// modified` and nothing has happened since to make the refusal worth
    /// repeating. Set by the run loop when either is refused; cleared by any
    /// command that *errors* and by any command that *changes the buffer*, and
    /// by nothing else — so `1d`, `q`, `1p`, `q` quits on the second `q` while
    /// `1d`, `q`, `1d`, `q` warns again. All measured; see [`Editor::touch`].
    warned: bool,
    /// Whether the command immediately before this one was that refusal. The
    /// *end-of-input* warning asks this instead of [`Editor::warned`], and the
    /// difference is visible: `1d`, `q`, EOF exits 1 with one warning, while
    /// `1d`, `q`, `1p`, EOF exits 2 with two — the `1p` neither cleared
    /// `warned` (so a `q` there would still have quit) nor kept the run of
    /// refusals going. Two rules, so two flags.
    warned_last: bool,
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
    /// The `k` marks: one 26-bit set per buffer line, bit `n` meaning "this line
    /// is marked `a + n`".
    ///
    /// Parallel to the buffer for the same reason [`Editor::marks`] is, and it
    /// matters more here because a `k` mark outlives the command that set it:
    /// `2kb`, then `1d`, then `'bp` prints `beta` in GNU — the mark followed its
    /// line up by one. A table of line numbers would have to be renumbered by
    /// every insert, delete, move and join, and the one that was forgotten would
    /// silently address the wrong line rather than fail.
    ///
    /// A set per line rather than a letter per line because a line may carry
    /// several: `1ka` then `1kb` leaves both live. Setting a letter clears it
    /// wherever it was — measured, `1ka` then `2ka` leaves only line 2 marked.
    kmarks: Vec<u32>,
    /// The one-deep undo record: the state as it was just before the last
    /// buffer-modifying command.
    ///
    /// One deep, and swapped rather than popped, because that is what GNU has:
    /// `u` after `u` *redoes*. A stack would be a different editor, not a better
    /// one. See [`Editor::snapshot`].
    undo: Option<Snapshot>,
    /// A `g`/`v`/`G`/`V` is running and has already taken its undo snapshot, so
    /// the next modifying command inside it must not take another.
    ///
    /// This is what makes one `g` one undo unit: `g/alpha/d` then `u` restores
    /// every line the global deleted, not just the last.
    global_undo_taken: bool,
    /// The state as it was just before the running global started, waiting to
    /// become the undo record if any command inside it modifies the buffer.
    ///
    /// Held here rather than installed as [`Editor::undo`] — which the global
    /// clears outright — because a `g` that modifies nothing must leave `u`
    /// with *nothing to do*, not with a no-op to do: GNU answers `Nothing to
    /// undo` to `1d`, `g/beta/p`, `u`. It is taken — moved out — by the first
    /// modifying command inside the global.
    global_before: Option<Snapshot>,
    /// The lines `x` will put back, filled by `d`, `c`, `j`, `s` and `y`.
    ///
    /// Filled by exactly those five and by nothing else — measured: `m`, `t`,
    /// `r`, `a`, `i` and `u` all leave it as it was, so `1m$` then `x` answers
    /// `Nothing to put` on a fresh session. It is *not* part of
    /// [`Snapshot`] either: `1d`, `u`, `x` still puts back the deleted line,
    /// because undoing a delete does not un-cut it.
    ///
    /// Plain text, no marks: a line put back by `x` carries neither the `k`
    /// marks the original had nor a `g` selection.
    cut_buffer: Vec<Vec<u8>>,
    /// The last error reported, for `h` and `H` to print.
    ///
    /// Survives any number of successful commands — `9p`, `h`, `1p`, `h`
    /// prints the same sentence twice — and is only ever replaced, never
    /// cleared, which is why it is set in [`Editor::fail`] and nowhere else.
    last_error: Option<EdError>,
    /// Whether a sentence follows the `?`. Starts as `-v` and is toggled by
    /// `H`, which is why it lives here rather than staying on [`Options`].
    verbose: bool,
    /// Whether the prompt is written before each command is read, toggled by
    /// `P`. Starts on exactly when `-p` was given.
    prompt_on: bool,
    /// The prompt itself. GNU's default is `*`, which is what `P` turns on when
    /// no `-p` set one.
    prompt: Vec<u8>,
    /// How many lines a bare `z` prints. GNU starts at 22, and a count given to
    /// `z` *persists* as the new window: `1z3` then `z` prints lines 4 to 6.
    window: usize,
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
        let verbose = opts.verbose;
        // `-p` both sets the string and turns the prompt on; `P` alone turns on
        // GNU's default `*`. Measured: `P`, `1p` writes `*one`.
        let prompt_on = opts.prompt.is_some();
        let prompt = opts.prompt.clone().unwrap_or_else(|| b"*".to_vec());
        Editor {
            buffer: Vec::new(),
            current: 0,
            filename: None,
            modified: false,
            warned: false,
            warned_last: false,
            opts,
            out: Stream::stdout(),
            status: 0,
            line_no: 0,
            file_driven,
            stdin: std::io::stdin().lock(),
            last_re: None,
            marks: Vec::new(),
            kmarks: Vec::new(),
            undo: None,
            global_undo_taken: false,
            global_before: None,
            cut_buffer: Vec::new(),
            last_error: None,
            verbose,
            prompt_on,
            prompt,
            // GNU's default window, and the number `z` prints with no count.
            window: 22,
            global_input: None,
        }
    }

    /// Begin a change: forget the old undo record, and hand back the state to
    /// restore if this command turns out to change something.
    ///
    /// Called *after* a command's addresses have been validated and before it
    /// writes anything. That placement is measured on both sides: `1d`, then
    /// `1m5` (an invalid destination), then `u` still undoes the `1d` — the
    /// address failed before this ran — while `1d`, then a `s` that matches
    /// nothing, then `u` answers `Nothing to undo`.
    ///
    /// Clearing and recording are two steps rather than one because they are
    /// separately observable. GNU clears the undo stack when a modifying
    /// command *starts* and pushes to it only as lines actually move, so a
    /// command that clears and then changes nothing leaves `u` with nothing to
    /// undo: `1d`, then `r` on an empty file, then `u` says `Nothing to undo`
    /// rather than bringing the deleted line back. A single "replace the
    /// record" step would get that case backwards.
    /// Inside a `g`/`v`/`G`/`V` this takes nothing and hands back `None`: the
    /// unit's "before" was captured by [`Editor::global`] at the top of the
    /// global, and it has to be, because `u` restores the *current line* along
    /// with the buffer and the global has already moved `.` onto the selected
    /// line by the time an inner command runs. Measured: with `.` at 2,
    /// `g/a/d` then `u` leaves `.` at 2, not at the first line the global
    /// matched. Not taking one per inner command also stops the snapshot being
    /// quadratic in the buffer for a global that touches every line.
    fn begin_change(&mut self) -> Option<Snapshot> {
        if self.global_input.is_some() {
            return None;
        }
        let before = self.snapshot();
        self.undo = None;
        Some(before)
    }

    /// The whole editor state that one `u` swaps.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            buffer: self.buffer.clone(),
            marks: self.marks.clone(),
            kmarks: self.kmarks.clone(),
            current: self.current,
            modified: self.modified,
        }
    }

    /// Record `before` as the state `u` will restore.
    ///
    /// Inside a global the first call takes the snapshot [`Editor::global`] set
    /// aside and every later one is ignored, which is what makes a whole global
    /// one undo unit: `g/alpha/d` then `u` brings back every line the global
    /// deleted, and puts `.` back where it was before the `g`.
    fn record_change(&mut self, before: Option<Snapshot>) {
        if self.global_input.is_some() {
            if !self.global_undo_taken {
                self.global_undo_taken = true;
                self.undo = self.global_before.take();
            }
            return;
        }
        self.undo = before;
    }

    /// The destination address of `m` and `t`, with the print suffix that may
    /// follow it.
    ///
    /// The destination is an ordinary address expression, so `1m/gamma/` and
    /// `1m.-1` both work. With nothing written there at all it is `.` — which
    /// is why a bare `m` is legal and moves the addressed line to just after the
    /// current one rather than being a syntax error.
    ///
    /// A blank between the destination and its suffix is allowed here where it
    /// is not after a plain command: `1p ` is `Invalid command suffix` but
    /// `1m$ p` prints. That is not an inconsistency to smooth over — GNU's
    /// address scanner eats trailing blanks, and this is the one place the
    /// difference shows.
    fn third_address(&mut self, rest: &[u8]) -> Result<(usize, Option<PrintStyle>), EdError> {
        let mut pos = 0usize;
        let addr = parse_address(rest, &mut pos)?;
        skip_blank(rest, &mut pos);
        let dest = match addr {
            Some(a) => self.resolve(&a)?,
            None => i64::try_from(self.current).unwrap_or(i64::MAX),
        };
        if dest < 0 || dest > i64::try_from(self.total()).unwrap_or(i64::MAX) {
            return Err(EdError::InvalidAddress);
        }
        let dest = usize::try_from(dest).map_err(|_| EdError::InvalidAddress)?;
        let suffix = print_suffix(rest.get(pos..).unwrap_or(&[]))?;
        Ok((dest, suffix))
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
            AddrBase::Mark(letter) => i64::try_from(self.mark_line(*letter)?).unwrap_or(i64::MAX),
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
    /// Read a file's bytes, printing the OS's own complaint on the way out and
    /// telling "would not open" from "opened and would not read".
    ///
    /// Opening and reading are kept apart because GNU grades them differently,
    /// and `fs::read` would fuse them into one error that cannot be told apart
    /// afterwards. Measured against GNU ed 1.20.1, at *startup*:
    ///
    /// | what failed | over a pipe | from a script file |
    /// |---|---|---|
    /// | the open (`nosuch.txt`, an unreadable file) | one line on stderr, empty buffer, status **0** | one line, exit **2**, no editing |
    /// | the read (`ed .`, a directory) | *two* lines — the errno's, then `Cannot read input file` — empty buffer, status **1** | one line, exit **2** |
    ///
    /// So an unreadable directory is a *command* failure and a missing file is
    /// not, which is worth having right: a script that opens a path it did not
    /// expect to be a directory otherwise reports success. From `e` and `r`,
    /// where there *is* a command to fail, both are failures and both print
    /// their sentence — `Cannot open input file` and `Cannot read input file`
    /// respectively. That difference in reporting is why this returns the error
    /// rather than printing it, and why [`Editor::load`] still owns the startup
    /// half of the table.
    ///
    /// `opened` has to come from the open call itself, not from a later
    /// `metadata` probe: a file with no read permission *exists*, so a probe
    /// would call its EACCES a read failure and print a second line GNU does
    /// not print.
    fn read_file(&mut self, name: &[u8]) -> Result<Vec<u8>, EdError> {
        let mut opened = false;
        let read = std::fs::File::open(os_from_bytes(name)).and_then(|mut f| {
            opened = true;
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut content).map(|_| content)
        });
        match read {
            Ok(content) => Ok(content),
            Err(e) => {
                self.complain(name, &strerror(&e));
                Err(if opened {
                    EdError::CannotReadInputFile
                } else {
                    EdError::CannotOpenInputFile
                })
            }
        }
    }

    /// What `e`, `r` and the startup load all print about a successful read.
    ///
    /// `Newline appended` goes out under `-s` and under `-q` alike: it is not a
    /// diagnostic, it is a statement about what the buffer now holds and
    /// therefore about what `w` will write. The byte count is the ordinary
    /// chatter that `-s` exists to silence.
    fn report_read(&mut self, bytes: usize, appended: bool) {
        if appended {
            self.put(b"Newline appended\n");
        }
        if !self.opts.script {
            self.put(format!("{bytes}\n").as_bytes());
        }
    }

    fn load(&mut self, file: Option<&std::ffi::OsStr>) -> Option<u8> {
        let file = file?;
        let name = os_bytes(file).into_owned();

        if let Err(e) = check_name(&name, &self.opts) {
            return Some(self.name_refused(&name, e.sentence()));
        }

        match self.read_file(&name) {
            Ok(content) => {
                let (lines, appended) = split_lines(&content, self.opts.strip_cr);
                self.buffer = lines;
                self.kmarks = vec![0; self.buffer.len()];
                self.current = self.buffer.len();
                self.filename = Some(name);
                let bytes = byte_count(&self.buffer);
                self.report_read(bytes, appended);
                None
            }
            Err(e) => {
                self.filename = Some(name.clone());
                if self.file_driven {
                    return Some(2);
                }
                // Only the *read* failure earns a second sentence at startup;
                // a file that never opened gets the OS's line and nothing else.
                if e == EdError::CannotReadInputFile {
                    self.complain(&name, e.sentence());
                    self.status = self.status.max(e.status());
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
        // Remembered for `h` and `H`, which is the only way to see it after the
        // fact when the session did not start with `-v`.
        self.last_error = Some(e);
        if self.verbose {
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
            if self.prompt_on {
                let prompt = self.prompt.clone();
                self.put(&prompt);
                let _ = self.out.flush();
            }
            let Some(line) = self.read_line() else { break };
            self.line_no = self.line_no.saturating_add(1);
            let result = self.execute(&line);
            // Both flags are maintained here rather than inside `execute`, so
            // that a `g` list counts as the one command it is spelled as.
            self.warned_last = matches!(result, Err(EdError::BufferModified));
            match &result {
                // The refusal itself is what sets the flag, for `q` and `e`
                // alike — which is why `1d`, `q`, `e f` proceeds, and so does
                // the pair the other way round.
                Err(EdError::BufferModified) => self.warned = true,
                // Any *other* error puts the warning back: `1d`, `q`, `zzz`,
                // `q` warns twice. Measured, and not a rule anyone would guess.
                Err(_) => self.warned = false,
                // A successful command clears it only if it changed something,
                // which `Editor::touch` does at the point of the change.
                Ok(_) => {}
            }
            match result {
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
        if self.modified && !self.warned_last {
            self.fail(EdError::BufferModifiedAtEof);
        }
        self.status
    }

    fn total(&self) -> usize {
        self.buffer.len()
    }

    /// Record that the buffer just changed.
    ///
    /// The second half is the part that is not obvious: a change *retracts* an
    /// outstanding `Warning: buffer modified`, so `1d`, `q`, `1d`, `q` warns
    /// twice while `1d`, `q`, `1p`, `q` warns once and then quits. Reading it
    /// as a rule about the user rather than about the flag makes it sensible —
    /// the warning means "you are about to lose work", and after further work
    /// is done it is a different, unheeded warning about different work.
    fn touch(&mut self) {
        self.modified = true;
        self.warned = false;
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

    /// The file an `e`, `E`, `f`, `r`, `w` or `W` command names, or the current
    /// one.
    ///
    /// The command letter must be followed by whitespace or by nothing at all:
    /// `efive.txt` and `$rf.txt` are `Unexpected command suffix`, where
    /// `e five.txt` and `$r f.txt` work. That is GNU's rule — its
    /// `unexpected_command_suffix` runs on the byte after the letter for every
    /// one of these six commands — and it is why a file name can never be run
    /// straight onto the letter the way `1p` runs a suffix onto `p`. Any number
    /// of blanks is fine after that, and a tab counts as one.
    fn resolve_name(&mut self, rest: &[u8], remember: bool) -> Result<Vec<u8>, EdError> {
        if !matches!(rest.first(), None | Some(b' ' | b'\t')) {
            return Err(EdError::UnexpectedCommandSuffix);
        }
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
                let before = self.begin_change();
                self.insert(at, text);
                self.current = at.saturating_add(added);
                if added > 0 {
                    self.touch();
                    self.record_change(before);
                }
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            b'c' => {
                let suffix = print_suffix(&c.rest)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let text = self.read_text();
                let added = text.len();
                let before = self.begin_change();
                self.yank(lo, hi);
                self.delete(lo, hi);
                self.insert(lo.saturating_sub(1), text);
                self.current = lo.saturating_sub(1).saturating_add(added);
                self.touch();
                self.record_change(before);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // Move a range. The destination is "after line N", so `0` is
            // "before line 1" and `$` is the end.
            b'm' => {
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let (dest, suffix) = self.third_address(&c.rest)?;
                // GNU's test, verbatim: `lo <= dest < hi`. It refuses a
                // destination *strictly inside* the range — there is no place
                // among the lines being moved for them to go — while allowing
                // the two no-ops at the edges, `1,2m2` and `1m1`. Both of those
                // still count as changes: `1m1` then `q` warns about the buffer.
                if lo <= dest && dest < hi {
                    return Err(EdError::InvalidDestination);
                }
                let before = self.begin_change();
                let lines = self.cut(lo, hi);
                let count = lines.len();
                // The cut removed `count` lines, all of them either wholly above
                // the destination or wholly below it — which is exactly what the
                // check above guarantees — so the destination shifts by the whole
                // count or not at all, never by part of it.
                let at = if dest >= hi {
                    dest.saturating_sub(count)
                } else {
                    dest
                };
                self.paste(at, lines);
                self.current = at.saturating_add(count);
                self.touch();
                self.record_change(before);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // Copy a range. Unlike `m` there is no forbidden destination:
            // copying a range into the middle of itself is well defined, and
            // `1,2t1` duplicates both lines where they stand.
            b't' => {
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let (dest, suffix) = self.third_address(&c.rest)?;
                let copied: Vec<Vec<u8>> = self
                    .buffer
                    .get(lo.saturating_sub(1)..hi)
                    .unwrap_or_default()
                    .to_vec();
                let count = copied.len();
                let before = self.begin_change();
                // Through `insert` rather than `paste`: a copy is new text and
                // carries no marks, so `1ka` then `1t$` leaves the mark on line
                // 1 and not on the copy.
                self.insert(dest, copied);
                self.current = dest.saturating_add(count);
                self.touch();
                self.record_change(before);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // Join a range into one line, with nothing between the pieces.
            b'j' => {
                let suffix = print_suffix(&c.rest)?;
                // The default range is `.,.+1`, which `range` cannot express:
                // it derives the upper bound from the *lower* default. An
                // unaddressed `j` on the last line is therefore `Invalid
                // address`, which is measured and is the reason for the
                // explicit bounds check here.
                let (lo, hi) = if c.addressed {
                    self.range(&c, (self.current, self.current), false)?
                } else {
                    let lo = self.current;
                    let hi = lo.saturating_add(1);
                    if lo < 1 || hi > self.total() {
                        return Err(EdError::InvalidAddress);
                    }
                    (lo, hi)
                };
                // One line is already joined. GNU treats this as a success that
                // changes nothing — not even the current line — and still
                // honours the print suffix.
                if hi > lo {
                    let before = self.begin_change();
                    self.yank(lo, hi);
                    let taken = self.cut(lo, hi);
                    let mut joined: Vec<u8> = Vec::new();
                    for line in &taken {
                        joined.extend_from_slice(&line.text);
                    }
                    // The joined line carries neither mark: it is a new line,
                    // and the lines that had them no longer exist. Measured —
                    // a `k` mark set on either of two joined lines answers
                    // `Invalid address` afterwards, and a `g` list that joins
                    // does not revisit the line it produced.
                    self.paste(
                        lo.saturating_sub(1),
                        vec![Taken {
                            text: joined,
                            kmark: 0,
                        }],
                    );
                    self.current = lo;
                    self.touch();
                    self.record_change(before);
                }
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `kx` names the addressed line `x`, for `'x` to address later.
            b'k' => {
                let (_, hi) = self.range(&c, (self.current, self.current), false)?;
                // No letter at all is `Invalid command suffix`, not `Invalid
                // mark character`: there is no character there to call invalid.
                // Measured both ways.
                let Some(&letter) = c.rest.first() else {
                    return Err(EdError::InvalidCommandSuffix);
                };
                if !letter.is_ascii_lowercase() {
                    return Err(EdError::InvalidMarkCharacter);
                }
                let suffix = print_suffix(c.rest.get(1..).unwrap_or(&[]))?;
                let bit = 1u32 << u32::from(letter.wrapping_sub(b'a'));
                // A letter names one line: `1ka` then `2ka` leaves only line 2
                // marked, so the old placement has to be cleared first.
                for m in &mut self.kmarks {
                    *m &= !bit;
                }
                if let Some(m) = self.kmarks.get_mut(hi.saturating_sub(1)) {
                    *m |= bit;
                }
                // `k` does not move `.`, and does not modify the buffer — a `k`
                // alone still lets `q` quit.
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `(.)r FILE` reads a file in after the addressed line. The default
            // address is `$`, not `.`.
            b'r' => {
                let at = match c.second.or(c.first) {
                    Some(n) => n,
                    None => i64::try_from(total).unwrap_or(i64::MAX),
                };
                if at < 0 || at > i64::try_from(total).unwrap_or(i64::MAX) {
                    return Err(EdError::InvalidAddress);
                }
                let at = usize::try_from(at).map_err(|_| EdError::InvalidAddress)?;
                let name = self.resolve_name(&c.rest, true)?;
                // Cleared here, before the read, and recorded only if the read
                // produced lines — so `r` on an empty file, and `r` on a file
                // that will not open, both leave `u` with nothing to undo. Both
                // measured.
                let before = self.begin_change();
                let content = self.read_file(&name)?;
                let (lines, appended) = split_lines(&content, self.opts.strip_cr);
                let bytes = byte_count(&lines);
                let added = lines.len();
                self.insert(at, lines);
                self.report_read(bytes, appended);
                // `.` becomes the last line read, or the address itself when
                // nothing was: `/beta/r` on an empty file leaves `.` at 2.
                self.current = at.saturating_add(added);
                if added > 0 {
                    self.touch();
                    self.record_change(before);
                }
                Ok(Action::Continue)
            }

            // `e FILE` replaces the buffer; `E FILE` does it without asking.
            b'e' | b'E' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                if c.cmd == b'e' && self.modified && !self.warned {
                    return Err(EdError::BufferModified);
                }
                let name = self.resolve_name(&c.rest, true)?;
                // GNU empties the buffer *before* it reads, so a failed `e`
                // leaves no buffer at all — measured: `e nosuch.txt` then `,p`
                // answers `Invalid address`. Surprising, and deliberate here:
                // an `e` that half-worked would be worse than one that plainly
                // left nothing behind.
                self.buffer.clear();
                self.kmarks.clear();
                self.marks.clear();
                self.current = 0;
                self.modified = false;
                self.undo = None;
                // The name is remembered even when the read fails, which is
                // what makes a later bare `e` retry the same file.
                self.filename = Some(name.clone());
                let content = self.read_file(&name)?;
                let (lines, appended) = split_lines(&content, self.opts.strip_cr);
                self.buffer = lines;
                self.kmarks = vec![0; self.buffer.len()];
                self.current = self.buffer.len();
                let bytes = byte_count(&self.buffer);
                self.report_read(bytes, appended);
                Ok(Action::Continue)
            }

            // One level of undo, and `u` after `u` redoes — see [`Snapshot`].
            b'u' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                let suffix = print_suffix(&c.rest)?;
                let Some(prev) = self.undo.take() else {
                    return Err(EdError::NothingToUndo);
                };
                // Swapped, not popped: what `u` puts back becomes what the next
                // `u` would put back, which is the redo.
                let redo = Snapshot {
                    buffer: std::mem::replace(&mut self.buffer, prev.buffer),
                    marks: std::mem::replace(&mut self.marks, prev.marks),
                    kmarks: std::mem::replace(&mut self.kmarks, prev.kmarks),
                    current: std::mem::replace(&mut self.current, prev.current),
                    modified: std::mem::replace(&mut self.modified, prev.modified),
                };
                self.undo = Some(redo);
                // `u` changes the buffer like any other command, so it retracts
                // an outstanding warning: `1d`, `q`, `u`, `u` — the second `u`
                // redoing the delete — warns again at the next `q`. When the
                // state it restored was unmodified this is unobservable, since
                // there is then nothing to warn about at all.
                self.warned = false;
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // A comment. The rest of the line is text, not a suffix — `#p`
            // prints nothing — and an address is accepted and then ignored,
            // though it is still *resolved*, so `/zzz/#` is `No match`.
            b'#' => Ok(Action::Continue),

            // `h` explains the last `?`. It is how a person at a terminal finds
            // out *why* something was refused, which they cannot do any other
            // way: `-v` has to be decided before the session starts, and by the
            // time you want the reason it is too late to have asked for it.
            //
            // It does not clear the record — `9p`, `h`, `1p`, `h` prints the
            // sentence twice — so it can be asked more than once, and it prints
            // nothing at all when nothing has failed yet.
            b'h' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                let suffix = print_suffix(&c.rest)?;
                if let Some(e) = self.last_error {
                    let mut line = e.sentence().as_bytes().to_vec();
                    line.push(b'\n');
                    self.put(&line);
                }
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `H` is `h` made automatic, and `-v` is exactly "start with `H`
            // already on" — which is why the flag it toggles is an `Editor`
            // field and not the immutable `Options` one, and why `-v` plus an
            // `H` turns the sentences *off*.
            b'H' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                let suffix = print_suffix(&c.rest)?;
                self.verbose = !self.verbose;
                // Turning it on prints the pending sentence, so `1d`, `q`, `H`
                // prints `Warning: buffer modified` twice: once for the `q` the
                // `H` is explaining, and once because the `H` itself is now
                // verbose about it. Measured, and it is the useful behaviour —
                // you type `H` *because* something just failed.
                if self.verbose
                    && let Some(e) = self.last_error
                {
                    let mut line = e.sentence().as_bytes().to_vec();
                    line.push(b'\n');
                    self.put(&line);
                }
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `P` toggles the prompt. GNU writes it before every command it
            // reads, whether or not input is a terminal, so a script that turns
            // it on gets a `*` interleaved with its output — which is what
            // makes it worth having a harness case.
            b'P' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                let suffix = print_suffix(&c.rest)?;
                self.prompt_on = !self.prompt_on;
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `(.,.)y` copies the addressed lines into the cut buffer. It is
            // the one command here that neither moves `.` nor modifies the
            // buffer: `1,2yp` prints the line `.` was already on.
            b'y' => {
                let suffix = print_suffix(&c.rest)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                self.yank(lo, hi);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `(.)x` puts the cut buffer back after the addressed line. `0x` is
            // legal — it puts at the front — which is why the range allows zero.
            b'x' => {
                let suffix = print_suffix(&c.rest)?;
                let (_, hi) = self.range(&c, (self.current, self.current), true)?;
                // Checked after the address, as GNU does: `9x` on a five-line
                // buffer is `Invalid address` even with nothing to put.
                if self.cut_buffer.is_empty() {
                    return Err(EdError::NothingToPut);
                }
                let lines = self.cut_buffer.clone();
                let added = lines.len();
                let before = self.begin_change();
                self.insert(hi, lines);
                self.current = hi.saturating_add(added);
                self.touch();
                self.record_change(before);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            // `(.+1)z(N)` prints a window of lines and remembers `N` as the
            // window size for next time — the paging command, and the reason a
            // bare `z` at the end of a buffer answers `Invalid address` rather
            // than `Unknown command`.
            b'z' => {
                // The address is the *first* line printed, not the last, and
                // when two are given it is the second that counts: `1z3` prints
                // 1 to 3, and `1,3z2` prints 3 to 4. So the pair collapses to
                // "the second address, defaulting to `.+1`" — which is exactly
                // what `range` cannot express, since it derives its upper bound
                // from the lower default.
                //
                // Inside a `g` list the default is `.` rather than `.+1` —
                // GNU's own `current_addr + !isglobal`, and the only place in
                // ed where a default address depends on *where the command came
                // from*. It is the right quirk: the global has just put `.` on
                // the selected line, and a `g/RE/z` that skipped past it would
                // page from the line after every match. Measured: `g/line0/z2`
                // on thirty numbered lines starts each window on the match.
                let skip = usize::from(self.global_input.is_none());
                let start = c.second.or(c.first).unwrap_or_else(|| {
                    i64::try_from(self.current.saturating_add(skip)).unwrap_or(1)
                });
                if start < 1 || start > i64::try_from(self.total()).unwrap_or(i64::MAX) {
                    return Err(EdError::InvalidAddress);
                }
                let start = usize::try_from(start).map_err(|_| EdError::InvalidAddress)?;
                let mut pos = 0usize;
                while matches!(c.rest.get(pos), Some(b'0'..=b'9')) {
                    pos = pos.saturating_add(1);
                }
                if pos > 0 {
                    let digits = c.rest.get(..pos).unwrap_or_default();
                    // A count of zero, and one too large to hold, are both
                    // `Invalid command suffix` — as is `1z-1`, which never
                    // reaches here because `-` is not a digit and so falls to
                    // `print_suffix` below. Measured, all three.
                    let n: usize = std::str::from_utf8(digits)
                        .ok()
                        .and_then(|t| t.parse().ok())
                        .filter(|n| *n > 0)
                        .ok_or(EdError::InvalidCommandSuffix)?;
                    self.window = n;
                }
                let style =
                    print_suffix(c.rest.get(pos..).unwrap_or_default())?.unwrap_or_default();
                // Clamped at the end of the buffer rather than refused: `4z9`
                // on a five-line buffer prints lines 4 and 5 and is not an
                // error, which is what makes `z` usable for walking to the end.
                let end = start
                    .saturating_add(self.window)
                    .saturating_sub(1)
                    .min(self.total());
                // `print_range` leaves `.` on the last line printed.
                self.print_range(start, end, style);
                Ok(Action::Continue)
            }

            b'd' => {
                let suffix = print_suffix(&c.rest)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                let before = self.begin_change();
                self.yank(lo, hi);
                self.delete(lo, hi);
                self.current = lo.min(self.total());
                self.touch();
                self.record_change(before);
                self.finish_suffix(suffix);
                Ok(Action::Continue)
            }

            b's' => {
                let sub = parse_substitute(&c.rest)?;
                let re = self.pattern(&sub.pattern)?;
                let (lo, hi) = self.range(&c, (self.current, self.current), false)?;
                // Cleared before the first line is looked at and recorded only
                // once something changes, so a `s` that matches nothing leaves
                // `u` with nothing to undo — measured, and not the same as
                // leaving the *previous* command's record in place.
                let mut before = Some(self.begin_change());
                let mut hit = None;
                let mut n = lo;
                while n <= hi {
                    let idx = n.saturating_sub(1);
                    let replaced = match self.buffer.get(idx) {
                        Some(l) => substitute_line(l, &re, &sub.replacement, sub.global)?,
                        None => None,
                    };
                    if let Some(new_line) = replaced {
                        // The cut buffer ends up holding the *last* changed
                        // line's original, not all of them: GNU rewrites a line
                        // by deleting the old one and inserting the new, and
                        // each of those deletes yanks — clearing what the
                        // previous line left. So `,s/a/X/` over three matching
                        // lines leaves one line in the cut buffer, and a
                        // following `x` puts back only the third line's before.
                        // Measured.
                        self.yank(n, n);
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
                        self.touch();
                        // Recorded on the first line that changes, for the same
                        // reason and with the same consequence: a `s` that dies
                        // half-way is still undoable back to where it started.
                        if let Some(state) = before.take() {
                            self.record_change(state);
                        }
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

            // `w` truncates the file and writes the range; `W` appends to it.
            // Everything else about them is the same, down to `W` clearing the
            // modified flag even when the name is not the default one — which
            // is arguably wrong of GNU, since the buffer is then saved in no
            // single place, but it is what GNU does and is measured.
            b'w' | b'W' => {
                let (lo, hi) = if c.addressed {
                    self.range(&c, (1, self.total()), false)?
                } else {
                    (1, self.total())
                };
                let name = self.resolve_name(&c.rest, true)?;
                self.write(&name, lo, hi, c.cmd == b'W')?;
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
                // Address first, then suffix: `1q9` is `Unexpected address`,
                // not `Invalid command suffix`. GNU checks them in that order
                // for every command that refuses an address, and now that `h`
                // exists the difference between the two sentences is visible.
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                print_suffix(&c.rest)?;
                if self.modified && !self.warned {
                    return Err(EdError::BufferModified);
                }
                Ok(Action::Quit)
            }

            b'Q' => {
                if c.addressed {
                    return Err(EdError::UnexpectedAddress);
                }
                print_suffix(&c.rest)?;
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
            // A new line carries no `k` mark, but the array still has to grow
            // *here* so every mark below the insertion point moves down with its
            // line rather than staying on a number.
            self.kmarks.insert(at.min(self.kmarks.len()), 0);
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
            // A `k` mark on a deleted line is gone, not moved: GNU answers
            // `Invalid address` for `'b` after the line `b` was on is deleted.
            // Draining the parallel entries is what says so.
            if start < self.kmarks.len() {
                self.kmarks.drain(start..end.min(self.kmarks.len()));
            }
        }
    }

    /// Copy the 1-based inclusive range `lo..=hi` into the cut buffer, for a
    /// later `x` to put back.
    ///
    /// Replaces what was there rather than adding to it, which is what makes
    /// the `s` case come out right: GNU yanks once per line it rewrites, so
    /// after a multi-line `s` only the last line's original survives.
    ///
    /// No marks travel. The cut buffer is plain text — a `k` mark set on a line
    /// that is deleted and then `x`-ed back does not come back with it, which
    /// is measured and is also the only sane answer, since `x` may put the
    /// lines back several times.
    fn yank(&mut self, lo: usize, hi: usize) {
        self.cut_buffer = self
            .buffer
            .get(lo.saturating_sub(1)..hi.min(self.buffer.len()))
            .unwrap_or_default()
            .to_vec();
    }

    /// Cut the 1-based inclusive range `lo..=hi` out of the buffer, marks and
    /// all, so that `m` and `j` can put it back somewhere else.
    ///
    /// Returns each line paired with *both* of its marks, because that pairing
    /// is the whole reason this exists rather than a `delete` and an `insert`.
    fn cut(&mut self, lo: usize, hi: usize) -> Vec<Taken> {
        let start = lo.saturating_sub(1).min(self.buffer.len());
        let end = hi.min(self.buffer.len());
        if start >= end {
            return Vec::new();
        }
        let lines: Vec<Vec<u8>> = self.buffer.drain(start..end).collect();
        // Dropped, not carried: a moved line loses its `g`/`v` selection. See
        // [`Taken`] for the measurement that says so.
        if start < self.marks.len() {
            drop(self.marks.drain(start..end.min(self.marks.len())));
        }
        let mut kmarks: Vec<u32> = Vec::with_capacity(lines.len());
        if start < self.kmarks.len() {
            kmarks.extend(self.kmarks.drain(start..end.min(self.kmarks.len())));
        }
        kmarks.resize(lines.len(), 0);
        lines
            .into_iter()
            .zip(kmarks)
            .map(|(text, kmark)| Taken { text, kmark })
            .collect()
    }

    /// Put lines cut by [`Editor::cut`] back after 0-based offset `at`, `k`
    /// marks and all — but never selected, per [`Taken`].
    fn paste(&mut self, at: usize, lines: Vec<Taken>) {
        let mut at = at.min(self.buffer.len());
        for line in lines {
            self.buffer.insert(at, line.text);
            self.marks.insert(at.min(self.marks.len()), false);
            self.kmarks.insert(at.min(self.kmarks.len()), line.kmark);
            at = at.saturating_add(1);
        }
    }

    /// The 1-based line carrying mark `letter`, or `Invalid address` when
    /// nothing does — which is what GNU says both for a mark never set and for
    /// one whose line has since been deleted.
    fn mark_line(&self, letter: u8) -> Result<usize, EdError> {
        let bit = 1u32 << u32::from(letter.wrapping_sub(b'a') & 31);
        self.kmarks
            .iter()
            .position(|m| m & bit != 0)
            .map(|i| i.saturating_add(1))
            .ok_or(EdError::InvalidAddress)
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
        // The whole global is one undo unit, so its "before" is taken once,
        // here, before the selection marks are laid down and before `.` starts
        // walking the selected lines — `u` restores the current line too, and
        // with `.` at 2, `g/a/d` then `u` puts it back at 2 rather than at the
        // first line the global matched.
        //
        // Set aside rather than installed as the undo record, and the old
        // record dropped: GNU's `exec_global` clears the undo stack the moment
        // the global starts and pushes to it only as lines actually move, so a
        // `g` that modifies nothing leaves `u` with nothing at all to do —
        // `1d`, `g/beta/p`, `u` answers `Nothing to undo` rather than bringing
        // the deleted line back. Installing the snapshot here instead would
        // make that `u` a silent no-op, which is a different answer.
        // `global_undo_taken` is the flag that says the unit has not opened yet.
        self.undo = None;
        self.global_before = Some(self.snapshot());
        self.global_undo_taken = false;
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

    /// Write `lo..=hi` to `name`, truncating it or — for `W` — appending.
    ///
    /// The byte count reported is the count *this* write produced, so a `W`
    /// onto a file that already had lines in it reports only what it added,
    /// not the file's new size. Measured.
    fn write(&mut self, name: &[u8], lo: usize, hi: usize, append: bool) -> Result<(), EdError> {
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
        let written = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(os_from_bytes(name))
                .and_then(|mut f| f.write_all(&bytes))
        } else {
            std::fs::write(os_from_bytes(name), &bytes)
        };
        match written {
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
        let Request::Run(_, file) = parse_args(std::slice::from_ref(&name)).unwrap() else {
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
                .chain(*b"\n")
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
        // Two neighbouring sentences that are not the same sentence: `1p!` is
        // `Invalid command suffix`, `efive.txt` is `Unexpected command suffix`.
        assert_eq!(
            EdError::UnexpectedCommandSuffix.sentence(),
            "Unexpected command suffix"
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
        // Parallel to the buffer, exactly as `load` leaves it: the `k` marks are
        // addressed by position, so an array a different length from the buffer
        // would put every mark on the wrong line.
        e.kmarks = vec![0; e.buffer.len()];
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

    /// Run a list of command lines against a fresh buffer and hand back the
    /// buffer, the current line, and the modified flag.
    ///
    /// One editor for the whole list, because the commands under test here are
    /// about what one leaves for the next: a `k` and the `'x` that reads it, a
    /// `d` and the `u` that puts it back. Only one editor may be alive at a
    /// time — see [`after`].
    fn drive(lines_in: &[&str], cmds: &[&[u8]]) -> (Vec<Vec<u8>>, usize, bool) {
        let mut e = editor_with(lines_in);
        for cmd in cmds {
            assert!(e.execute(cmd).is_ok(), "command failed: {:?}", *cmd);
        }
        (std::mem::take(&mut e.buffer), e.current, e.modified)
    }

    /// The error a list of commands ends with. Every command before the last
    /// must succeed.
    fn drive_err(lines_in: &[&str], cmds: &[&[u8]]) -> EdError {
        let mut e = editor_with(lines_in);
        let Some((last, rest)) = cmds.split_last() else {
            panic!("no commands");
        };
        for cmd in rest {
            assert!(e.execute(cmd).is_ok(), "command failed: {:?}", *cmd);
        }
        match e.execute(last) {
            Err(err) => err,
            Ok(_) => panic!("expected an error from {:?}", *last),
        }
    }

    /// The error a list of commands ends with, where the commands before the
    /// last are allowed to fail as well.
    ///
    /// Separate from [`drive_err`], which asserts that they all succeed,
    /// because a middle command's *failure* is sometimes the thing under test:
    /// a `s` that matches nothing is what clears the undo record.
    fn drive_last_err(lines_in: &[&str], cmds: &[&[u8]]) -> EdError {
        let mut e = editor_with(lines_in);
        let Some((last, rest)) = cmds.split_last() else {
            panic!("no commands");
        };
        for cmd in rest {
            drop(e.execute(cmd));
        }
        match e.execute(last) {
            Err(err) => err,
            Ok(_) => panic!("expected an error from {:?}", *last),
        }
    }

    #[test]
    fn move_takes_a_range_out_and_puts_it_back_after_the_destination() {
        let all = ["alpha", "beta", "gamma"];
        // Every one of these is measured against GNU ed 1.20.1, buffer and
        // current line together — the current line is the half a move is
        // easiest to get wrong, because the destination shifts when the lines
        // come out from above it.
        assert_eq!(
            drive(&all, &[b"1m$"]),
            (lines(&["beta", "gamma", "alpha"]), 3, true)
        );
        assert_eq!(
            drive(&all, &[b"2m0"]),
            (lines(&["beta", "alpha", "gamma"]), 1, true)
        );
        assert_eq!(
            drive(&all, &[b"1,2m$"]),
            (lines(&["gamma", "alpha", "beta"]), 3, true)
        );
        assert_eq!(drive(&all, &[b"1,3m0"]), (lines(&all), 3, true));
        // A destination written as anything an address can be.
        assert_eq!(
            drive(&all, &[b"1m/gamma/"]),
            (lines(&["beta", "gamma", "alpha"]), 3, true)
        );
        assert_eq!(
            drive(&all, &[b"1m.-1"]),
            (lines(&["beta", "alpha", "gamma"]), 2, true)
        );
        // No destination at all means `.`, which is why a bare `m` is legal.
        assert_eq!(
            drive(&all, &[b"1m"]),
            (lines(&["beta", "gamma", "alpha"]), 3, true)
        );
    }

    #[test]
    fn a_move_into_its_own_range_is_refused_but_the_edges_are_not() {
        let all = ["alpha", "beta", "gamma"];
        // `lo <= dest < hi`, GNU's test. The two edges are no-ops rather than
        // errors — and they still mark the buffer modified, which is why the
        // flag is asserted rather than the buffer alone.
        assert_eq!(drive_err(&all, &[b"1,2m1"]), EdError::InvalidDestination);
        assert_eq!(drive(&all, &[b"1,2m2"]), (lines(&all), 2, true));
        assert_eq!(drive(&all, &[b"1m1"]), (lines(&all), 1, true));
        // Out of range at either end is an address error, not a destination one.
        assert_eq!(drive_err(&all, &[b"1m5"]), EdError::InvalidAddress);
        assert_eq!(drive_err(&all, &[b"0m$"]), EdError::InvalidAddress);
        assert_eq!(drive_err(&["a"], &[b"1m$x"]), EdError::InvalidCommandSuffix);
    }

    #[test]
    fn copy_duplicates_a_range_and_may_copy_into_itself() {
        let all = ["alpha", "beta", "gamma"];
        assert_eq!(
            drive(&all, &[b"1t$"]),
            (lines(&["alpha", "beta", "gamma", "alpha"]), 4, true)
        );
        assert_eq!(
            drive(&all, &[b"1t0"]),
            (lines(&["alpha", "alpha", "beta", "gamma"]), 1, true)
        );
        // The destination `m` refuses is fine for `t`: nothing is being taken
        // away, so there is a well-defined answer.
        assert_eq!(
            drive(&all, &[b"1,2t1"]),
            (lines(&["alpha", "alpha", "beta", "beta", "gamma"]), 3, true)
        );
        assert_eq!(
            drive(&all, &[b"1,3t2"]),
            (
                lines(&["alpha", "beta", "alpha", "beta", "gamma", "gamma"]),
                5,
                true
            )
        );
        assert_eq!(drive_err(&all, &[b"1t5"]), EdError::InvalidAddress);
        assert_eq!(drive_err(&all, &[b"0t$"]), EdError::InvalidAddress);
    }

    #[test]
    fn join_glues_a_range_into_one_line_with_nothing_between() {
        let all = ["alpha", "beta", "gamma"];
        assert_eq!(
            drive(&all, &[b"1,2j"]),
            (lines(&["alphabeta", "gamma"]), 1, true)
        );
        assert_eq!(drive(&all, &[b",j"]), (lines(&["alphabetagamma"]), 1, true));
        // One line is already joined: not an error, and not a change either —
        // the current line does not move and the buffer is not marked modified.
        assert_eq!(drive(&all, &[b"1,1j"]), (lines(&all), 3, false));
        // The default range is `.,.+1`, which is off the end when `.` is `$`.
        assert_eq!(drive_err(&all, &[b"j"]), EdError::InvalidAddress);
        assert_eq!(
            drive(&all, &[b"1", b"j"]),
            (lines(&["alphabeta", "gamma"]), 1, true)
        );
        assert_eq!(drive_err(&all, &[b"0,1j"]), EdError::InvalidAddress);
    }

    #[test]
    fn a_mark_follows_its_line_wherever_the_line_goes() {
        let all = ["alpha", "beta", "gamma"];
        // This is the property the parallel array exists for. Each case is a
        // different way of moving text above the marked line; in every one of
        // them `'c` still names `gamma`.
        for shuffle in [
            &b"1d"[..],
            &b"1m$"[..],
            &b"1t0"[..],
            &b"1,2j"[..],
            &b"0a"[..],
        ] {
            let mut e = editor_with(&all);
            assert!(e.execute(b"3kc").is_ok());
            assert!(e.execute(shuffle).is_ok());
            let at = e.mark_line(b'c').expect("the mark survived");
            assert_eq!(
                e.buffer.get(at.saturating_sub(1)).map(Vec::as_slice),
                Some(&b"gamma"[..]),
                "after {shuffle:?}"
            );
        }
    }

    #[test]
    fn a_mark_is_one_line_per_letter_and_dies_with_its_line() {
        let all = ["alpha", "beta", "gamma"];
        // Setting a letter moves it: only one line answers to `a` at a time.
        let mut e = editor_with(&all);
        assert!(e.execute(b"1ka").is_ok());
        assert!(e.execute(b"2ka").is_ok());
        assert_eq!(e.mark_line(b'a'), Ok(2));
        // Several letters may share a line, though.
        assert!(e.execute(b"2kb").is_ok());
        assert_eq!(e.mark_line(b'a'), Ok(2));
        assert_eq!(e.mark_line(b'b'), Ok(2));
        // `k` moves nothing and changes nothing: `.` stays where it was and the
        // buffer is not modified, so a `q` after a `k` alone still quits.
        assert_eq!(e.current, 3);
        assert!(!e.modified);
        // Deleting the line takes the marks with it, and `'a` then has no line.
        assert!(e.execute(b"2d").is_ok());
        assert_eq!(e.mark_line(b'a'), Err(EdError::InvalidAddress));
        assert_eq!(e.mark_line(b'b'), Err(EdError::InvalidAddress));
        // A copy is new text and carries no mark; the original keeps it.
        assert!(e.execute(b"1ka").is_ok());
        assert!(e.execute(b"1t$").is_ok());
        assert_eq!(e.mark_line(b'a'), Ok(1));
    }

    #[test]
    fn a_mark_name_is_one_lowercase_letter_and_nothing_else() {
        let all = ["alpha", "beta", "gamma"];
        assert_eq!(drive_err(&all, &[b"1kA"]), EdError::InvalidMarkCharacter);
        assert_eq!(drive_err(&all, &[b"1k1"]), EdError::InvalidMarkCharacter);
        // No letter at all is a different error: there is no character there to
        // call invalid. Measured — GNU says `Invalid command suffix`.
        assert_eq!(drive_err(&all, &[b"1k"]), EdError::InvalidCommandSuffix);
        // The same rule reading a mark back, and the same two sentences.
        assert_eq!(drive_err(&all, &[b"'Ap"]), EdError::InvalidMarkCharacter);
        assert_eq!(drive_err(&all, &[b"'1p"]), EdError::InvalidMarkCharacter);
        assert_eq!(drive_err(&all, &[b"'"]), EdError::InvalidMarkCharacter);
        // A mark that was never set is an *address* error, not a name one.
        assert_eq!(drive_err(&all, &[b"'zp"]), EdError::InvalidAddress);
        // And a mark that was set addresses its line, offsets and all.
        assert_eq!(
            drive(&all, &[b"1ka", b"'a,'a+1d"]),
            (lines(&["gamma"]), 1, true)
        );
    }

    #[test]
    fn undo_puts_back_one_change_and_a_second_undo_redoes_it() {
        let all = ["alpha", "beta", "gamma"];
        // One level deep, and a swap rather than a pop: this is GNU's undo, and
        // a stack would be a different editor rather than a better one.
        assert_eq!(drive(&all, &[b"1d", b"u"]), (lines(&all), 3, false));
        assert_eq!(
            drive(&all, &[b"1d", b"u", b"u"]),
            (lines(&["beta", "gamma"]), 1, true)
        );
        // Only the last change: two deletes then one undo brings back one line.
        assert_eq!(
            drive(&all, &[b"1d", b"1d", b"u"]),
            (lines(&["beta", "gamma"]), 1, true)
        );
        // Every buffer-modifying command records, including the new ones.
        for cmd in [
            &b"1d"[..],
            &b"1m$"[..],
            &b"1t$"[..],
            &b"1,2j"[..],
            &b"1s/a/X/"[..],
        ] {
            assert_eq!(
                drive(&all, &[cmd, b"u"]),
                (lines(&all), 3, false),
                "after {cmd:?}"
            );
        }
    }

    #[test]
    fn undo_restores_the_marks_the_change_disturbed() {
        // Measured: `1ka`, `1d`, `u`, `'ap` prints `alpha` in GNU. The mark the
        // delete destroyed comes back with the line, which is only true if the
        // marks are part of what is snapshotted.
        let mut e = editor_with(&["alpha", "beta", "gamma"]);
        assert!(e.execute(b"1ka").is_ok());
        assert!(e.execute(b"1d").is_ok());
        assert_eq!(e.mark_line(b'a'), Err(EdError::InvalidAddress));
        assert!(e.execute(b"u").is_ok());
        assert_eq!(e.mark_line(b'a'), Ok(1));
    }

    #[test]
    fn a_command_that_changes_nothing_leaves_nothing_to_undo() {
        let all = ["alpha", "beta", "gamma"];
        // Nothing has happened yet.
        assert_eq!(drive_err(&all, &[b"u"]), EdError::NothingToUndo);
        // A command that does not touch the buffer does not disturb the record.
        assert_eq!(drive(&all, &[b"1d", b"1p", b"u"]), (lines(&all), 3, false));
        assert_eq!(drive(&all, &[b"1d", b"1ka", b"u"]), (lines(&all), 3, false));
        // …but one that *starts* to and then changes nothing clears it. This is
        // the difference between clearing and replacing, and it is measured: a
        // `s` that matches nothing leaves `u` with nothing to put back.
        assert_eq!(
            drive_last_err(&all, &[b"1d", b"1s/zzz/X/", b"u"]),
            EdError::NothingToUndo
        );
        // An address that fails never gets that far, so the record survives.
        assert_eq!(drive(&all, &[b"1d", b"1p", b"u"]), (lines(&all), 3, false));
        assert_eq!(drive_err(&all, &[b"1d", b"9m0"]), EdError::InvalidAddress);
        // `u` takes no address of its own.
        assert_eq!(drive_err(&all, &[b"1d", b"1u"]), EdError::UnexpectedAddress);
    }

    #[test]
    fn a_whole_global_is_one_undo() {
        let all = ["alpha", "beta", "gamma"];
        // Three deletes inside one `g`, and one `u` brings all three back —
        // the snapshot is taken once, at the top of the global.
        assert_eq!(drive(&all, &[b"g/a/d", b"u"]), (lines(&all), 3, false));
        // …and it is taken *at the top*, not at the first modifying command
        // inside: `u` restores the current line as well as the buffer, and by
        // the time an inner command runs `.` has already been moved onto the
        // selected line. Measured against GNU: `.` at 2, `g/a/d`, `u` → 2.
        assert_eq!(
            drive(&all, &[b"2", b"g/a/d", b"u"]),
            (lines(&all), 2, false)
        );
        assert_eq!(
            drive(&all, &[b"1", b"g/a/d", b"u"]),
            (lines(&all), 1, false)
        );
        // A global clears the record the moment it starts, and a global that
        // modifies nothing therefore leaves `u` nothing at all to do — not a
        // no-op to do. Measured: GNU answers `Nothing to undo` here, and does
        // so even for a `v` that selects no line whatsoever. This is why the
        // snapshot is set aside rather than installed when the `g` starts.
        assert_eq!(
            drive_err(&all, &[b"1d", b"g/beta/p", b"u"]),
            EdError::NothingToUndo
        );
        assert_eq!(
            drive_err(&all, &[b"1d", b"v/zzz/p", b"u"]),
            EdError::NothingToUndo
        );
    }

    #[test]
    fn a_comment_is_the_rest_of_the_line_and_does_nothing() {
        let all = ["alpha", "beta", "gamma"];
        // Not even a print suffix: `#p` prints nothing, because `p` is comment.
        assert_eq!(drive(&all, &[b"#p"]), (lines(&all), 3, false));
        assert_eq!(drive(&all, &[b"#"]), (lines(&all), 3, false));
        // An address is accepted and ignored — but it is still *resolved*, so a
        // search address that finds nothing is still an error.
        assert_eq!(
            drive(&all, &[b"1#anything at all"]),
            (lines(&all), 3, false)
        );
        assert_eq!(drive_err(&all, &[b"/zzz/#"]), EdError::NoMatch);
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

    // ---------------- the cut buffer ----------------

    /// The cut buffer a list of commands leaves behind, with the buffer and the
    /// current line, since the interesting cases are about what `x` and `y`
    /// leave *un*changed.
    fn cut_after(lines_in: &[&str], cmds: &[&[u8]]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, usize, bool) {
        let mut e = editor_with(lines_in);
        for cmd in cmds {
            assert!(e.execute(cmd).is_ok(), "command failed: {:?}", *cmd);
        }
        (
            std::mem::take(&mut e.cut_buffer),
            std::mem::take(&mut e.buffer),
            e.current,
            e.modified,
        )
    }

    #[test]
    fn yank_fills_the_cut_buffer_and_changes_nothing_else() {
        let all = ["a", "b", "c"];
        // `.` starts at 3. `y` neither moves it nor marks the buffer modified,
        // which is what separates it from every other command that reads a
        // range — measured against GNU.
        let (cut, buf, cur, modified) = cut_after(&all, &[b"1,2y"]);
        assert_eq!(cut, lines(&["a", "b"]));
        assert_eq!(buf, lines(&all));
        assert_eq!(cur, 3);
        assert!(!modified);
        // The default range is `.,.`, not the whole buffer.
        assert_eq!(cut_after(&all, &[b"y"]).0, lines(&["c"]));
        assert_eq!(drive_err(&all, &[b"0y"]), EdError::InvalidAddress);
        assert_eq!(drive_err(&all, &[b"9y"]), EdError::InvalidAddress);
    }

    #[test]
    fn put_inserts_the_cut_buffer_after_the_addressed_line() {
        let all = ["a", "b", "c"];
        let (_, buf, cur, modified) = cut_after(&all, &[b"1y", b"$x"]);
        assert_eq!(buf, lines(&["a", "b", "c", "a"]));
        assert_eq!(cur, 4, "`.` is the last line put");
        assert!(modified);
        // `0x` is legal, which is why the range allows zero.
        assert_eq!(
            cut_after(&all, &[b"1y", b"0x"]).1,
            lines(&["a", "a", "b", "c"])
        );
        // The buffer is not consumed, so the same lines can be put twice.
        assert_eq!(
            cut_after(&all, &[b"1y", b"$x", b"$x"]).1,
            lines(&["a", "b", "c", "a", "a"])
        );
        assert_eq!(drive_err(&all, &[b"x"]), EdError::NothingToPut);
        // The address is checked before the buffer is: an `x` that is both off
        // the end and has nothing to put says `Invalid address`. Measured, and
        // it is the order GNU checks them in.
        assert_eq!(drive_err(&all, &[b"9x"]), EdError::InvalidAddress);
    }

    #[test]
    fn every_command_that_removes_a_line_fills_the_cut_buffer() {
        let all = ["a", "b", "c"];
        // The four that do. `j` is the surprising one: it looks like a rewrite
        // rather than a removal, but GNU builds the joined line and then
        // *deletes* the range, and deleting is what yanks.
        assert_eq!(cut_after(&all, &[b"1d"]).0, lines(&["a"]));
        assert_eq!(cut_after(&all, &[b"1,2d"]).0, lines(&["a", "b"]));
        assert_eq!(cut_after(&all, &[b"1,2j"]).0, lines(&["a", "b"]));
        assert_eq!(cut_after(&all, &[b"1s/a/X/"]).0, lines(&["a"]));
        // And a multi-line `s` leaves only the *last* line it changed, because
        // GNU yanks once per rewritten line and each yank clears the last.
        assert_eq!(
            cut_after(&["a1", "a2", "b"], &[b",s/a/X/"]).0,
            lines(&["a2"])
        );
        // The four that do not, however much they look like they should.
        assert!(cut_after(&all, &[b"1m$"]).0.is_empty());
        assert!(cut_after(&all, &[b"1t$"]).0.is_empty());
        // `a` and `i` are not here because their text comes from stdin, which a
        // unit test has no way to feed; `scripts/ed-diff.sh` covers them.
        // It survives an undo, being no part of the snapshot `u` swaps.
        assert_eq!(cut_after(&all, &[b"1d", b"u"]).0, lines(&["a"]));
    }

    // ---------------- explaining the last error ----------------

    #[test]
    fn the_last_error_is_remembered_for_h_and_is_not_cleared_by_reading_it() {
        let mut e = editor_with(&["a", "b", "c"]);
        assert!(e.last_error.is_none(), "nothing has failed yet");
        assert!(
            e.execute(b"h").is_ok(),
            "and `h` on nothing is not an error"
        );
        if let Err(err) = e.execute(b"9p") {
            e.fail(err);
        }
        assert_eq!(e.last_error, Some(EdError::InvalidAddress));
        // Reading it leaves it in place, so it can be asked for twice — and so
        // that an `H` turned on later still has something to print.
        assert!(e.execute(b"h").is_ok());
        assert_eq!(e.last_error, Some(EdError::InvalidAddress));
        assert!(e.execute(b"1p").is_ok());
        assert_eq!(e.last_error, Some(EdError::InvalidAddress));
    }

    #[test]
    fn h_and_h_take_no_address_and_only_a_print_suffix() {
        let all = ["a", "b", "c"];
        for cmd in [&b"1h"[..], b"1H", b"1P"] {
            assert_eq!(
                drive_err(&all, &[cmd]),
                EdError::UnexpectedAddress,
                "{cmd:?}"
            );
        }
        for cmd in [&b"h9"[..], b"H9", b"P9"] {
            assert_eq!(
                drive_err(&all, &[cmd]),
                EdError::InvalidCommandSuffix,
                "{cmd:?}"
            );
        }
        for cmd in [&b"hp"[..], b"Hp", b"Pp", b"hn", b"hl"] {
            let mut e = editor_with(&all);
            assert!(e.execute(cmd).is_ok(), "{cmd:?}");
        }
    }

    #[test]
    fn capital_h_toggles_the_same_flag_that_dash_v_sets() {
        // `-v` is exactly "start with `H` already on", which is why the flag
        // lives on the editor and not on the immutable options — and why `-v`
        // followed by an `H` turns the sentences *off* rather than on.
        let mut e = editor_with(&["a"]);
        assert!(!e.verbose);
        assert!(e.execute(b"H").is_ok());
        assert!(e.verbose);
        assert!(e.execute(b"H").is_ok());
        assert!(!e.verbose);
        drop(e);
        let mut v = Editor::new(Options {
            verbose: true,
            ..Options::default()
        });
        assert!(v.verbose);
        assert!(v.execute(b"H").is_ok());
        assert!(!v.verbose, "-v then H is off, not on");
    }

    #[test]
    fn capital_p_toggles_the_prompt_that_dash_p_starts_on() {
        let mut e = editor_with(&["a"]);
        assert!(!e.prompt_on);
        assert_eq!(e.prompt, b"*", "the default prompt string");
        assert!(e.execute(b"P").is_ok());
        assert!(e.prompt_on);
        assert!(e.execute(b"P").is_ok());
        assert!(!e.prompt_on);
        drop(e);
        let mut p = Editor::new(Options {
            prompt: Some(b"> ".to_vec()),
            ..Options::default()
        });
        assert!(p.prompt_on, "-p starts it on");
        assert_eq!(p.prompt, b"> ");
        assert!(p.execute(b"P").is_ok());
        assert!(!p.prompt_on, "-p then P is off");
    }

    // ---------------- paging ----------------

    #[test]
    fn z_prints_a_window_starting_at_its_address() {
        let ten = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
        // The address is the first line printed, not the last, so `.` ends up
        // at `address + window - 1`. That is the whole observable here, since
        // what `z` writes goes to the real stdout — `scripts/ed-diff.sh` checks
        // the bytes.
        let (_, cur, _) = drive(&ten, &[b"1z3"]);
        assert_eq!(cur, 3);
        // The count persists as the window for every later `z`.
        let (_, cur, _) = drive(&ten, &[b"1z3", b"z"]);
        assert_eq!(cur, 6, "the second z printed 4..6");
        // Two addresses: the *second* is the start.
        let (_, cur, _) = drive(&ten, &[b"1,3z2"]);
        assert_eq!(cur, 4, "started at 3, not at 1");
        // Clamped at the end rather than refused, which is what makes `z`
        // usable for walking off the bottom of a buffer.
        let (_, cur, _) = drive(&ten, &[b"8z9"]);
        assert_eq!(cur, 10);
    }

    #[test]
    fn z_defaults_to_the_line_after_the_current_one_and_to_a_window_of_22() {
        let mut e = editor_with(&["a", "b", "c"]);
        assert_eq!(e.window, 22, "GNU's default window");
        // `.` is 3, so `.+1` is off the end. This is the state the old
        // `known-issues.md` entry measured, and why it concluded — wrongly —
        // that GNU had no `z` at all: `?` from a command that does not exist
        // and `?` from one that does are the same `?`.
        assert_eq!(e.execute(b"z").err(), Some(EdError::InvalidAddress));
        assert!(e.execute(b"1").is_ok());
        assert!(e.execute(b"z").is_ok(), "from line 1 it pages to the end");
        assert_eq!(e.current, 3);
    }

    #[test]
    fn a_z_count_must_be_a_positive_number_it_can_hold() {
        let all = ["a", "b", "c"];
        for cmd in [&b"1z0"[..], b"1z-1", b"1zx", b"1z99999999999999999999"] {
            assert_eq!(
                drive_err(&all, &[cmd]),
                EdError::InvalidCommandSuffix,
                "{cmd:?}"
            );
        }
        // The address is checked first, so an off-the-end address wins over a
        // bad count.
        assert_eq!(drive_err(&all, &[b"9z0"]), EdError::InvalidAddress);
        assert_eq!(drive_err(&all, &[b"0z2"]), EdError::InvalidAddress);
        // A refused count does not become the window.
        let mut e = editor_with(&all);
        assert!(e.execute(b"1z2").is_ok());
        assert_eq!(e.window, 2);
        assert!(e.execute(b"1z0").is_err());
        assert_eq!(e.window, 2, "the refused 0 did not take");
    }
}
