//! rm — remove files or directories.
//!
//! # Why this file exists in this shape
//!
//! The first rewrite fixed four bugs of the `String`-based original: argv is
//! `OsString` and stays that way to the syscall, so a name holding a byte that
//! is not valid UTF-8 — legal on this OS, by design — no longer panics
//! part-way through a recursive delete; `--` ends the options; `-f` ignores a
//! file that is *absent* rather than every failure; and nothing follows a
//! symlink when deciding what a name is.
//!
//! What it did **not** do was implement the rest of `rm`. Six options were
//! recognised and refused, and one of the refusals mattered far more than the
//! others: without `--preserve-root` there was no root failsafe at all, so
//! `rm -rf /` proceeded. Nor was there a refusal of `.` and `..`, and the
//! recursive path was `fs::remove_dir_all`, which happily emptied the current
//! directory when told `rm -rf .`. Those are the two ways `rm` destroys data
//! that its user did not ask it to destroy, and both are now closed.
//!
//! # What was measured, and why so much of it
//!
//! Everything below was read off GNU coreutils 9.4 rather than inferred from
//! the manual, because `rm`'s interactive behaviour is a four-state machine
//! that no prose description of `-f`/`-i`/`-I` gets right. Nine transcript
//! runs settled it; the surprising parts, each of which a plausible
//! implementation gets wrong:
//!
//! - **`-f` is not "the opposite of `-i`".** `-f` sets *two* things — never
//!   prompt, and ignore a missing operand. `-i`, `-I`, `--interactive=once`
//!   and `--interactive=always` clear the second as well as setting the first,
//!   but `--interactive=never` clears **neither**. So `rm -f --interactive=never`
//!   with no operands is silent and exits 0, while `rm -f -i` with no operands
//!   is `missing operand`. Last one wins, and which fields it touches depends
//!   on which spelling won.
//! - **The default is not "never prompt".** With no interactive option at all,
//!   `rm` still prompts before a **write-protected** file — but only when
//!   standard input is a terminal. That is what `---presume-input-tty` exists
//!   to fake, and it is the only thing it does.
//! - **Declining is not an error.** Answering `n` exits 0. But declining a
//!   *descend* and declining a *removal* differ in what happens to the
//!   ancestors: a declined descend silently abandons every enclosing
//!   directory, while a declined file leaves its parent to try and fail with
//!   `Directory not empty` — which *is* an error. See [`Verdict`].
//! - **An empty directory gets one prompt, not two.** `descend into
//!   directory` is asked only when there is something to descend into;
//!   otherwise the single question is `remove directory`.
//! - **A directory that cannot be read is never prompted about.** The read
//!   failure is discovered first, and reported as `cannot remove`.
//! - **`--no-preserve-root` may not be abbreviated**, alone among every long
//!   option in coreutils. `rm --no-p` is not the switch; it is the error
//!   `you may not abbreviate the --no-preserve-root option`. Nothing else in
//!   the table begins with `n`, so getopt resolves the prefix without
//!   complaint and the refusal is `rm`'s own, written against
//!   `argv[optind - 1]`. See [`no_abbreviating`].
//! - **The operand's spelling is echoed, not a normalised path.** `rm -rv
//!   tree/` ends `removed directory 'tree/'`, and `rm -rv ./tree` says
//!   `./tree/...` throughout. Only a *run* of trailing slashes is collapsed,
//!   and `//` is left alone because it is not `/`. That is gnulib `fts`'s rule
//!   and it is reproduced in [`normalize_operand`].
//!
//! # What is still not certified against GNU
//!
//! `--one-file-system` and `--preserve-root=all` both need a real mount point
//! to exercise, which needs root, which the differential harness does not
//! have. They are implemented from GNU's source rather than from a transcript,
//! and `scripts/rm-diff.sh` cannot cover them. Logged in `known-issues.md`.
//!
//! Off unix — the development host — there is no `euidaccess` and no
//! `st_dev`, so the write-protected prompt never fires and `--one-file-system`
//! never skips. Both degrade towards *more* prompting being skipped rather
//! than towards deleting something unasked.
//!
//! # The walk resolves descriptors, not paths
//!
//! Every syscall below a command-line operand goes through
//! [`coreutils::dirfd`]: an open descriptor on the directory being walked, plus
//! one component. Nothing here hands the kernel `dir/sub/file` and asks it to
//! re-walk that from the top, which is what let a second process swap `sub` for
//! a symlink mid-removal and send the deletions somewhere else entirely. The
//! path strings survive untouched — they are what gets *printed*, and
//! `scripts/rm-diff.sh` certifies their spelling case by case — but they are no
//! longer what gets *resolved*. See [`Loc`], and `known-issues.md` →
//! `TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`.
//!
//! The operand itself is still reached by path, because at the top there is no
//! descriptor above it yet to reach it through. That is GNU's position too: an
//! `fts` root is opened by the name it was given. What changes is everything
//! after the first step.

use coreutils::diag;
use coreutils::dirfd::{Dir, Kind, Stat};
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf};
use coreutils::stdfd::{self, Stream};
use coreutils::yesno::{Answers, StdinAnswers, yesno};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's descriptors.
coreutils::guard_std_fds!();

/// `rm`'s usage status is 1 — measured, and the majority case. The utilities
/// that use 2 (`ls`, `sort`, `grep`) are the ones that already spent 1 on a
/// real answer, which `rm` does not.
const RM: Program = Program::new("rm", 1);

/// GNU `rm`'s own `getopt_long` string, copied verbatim.
const SHORT_OPTIONS: &str = "dfirvIR";

/// GNU `rm`'s `long_opts[]`, **in its declaration order**, which is observable:
/// `getopt_long` lists an ambiguous prefix's candidates in table order.
///
/// Measured with `rm --=x`, which an empty prefix makes print the whole table:
///
/// ```text
/// rm: option '--=x' is ambiguous; possibilities: '--force' '--interactive'
/// '--one-file-system' '--no-preserve-root' '--preserve-root'
/// '---presume-input-tty' '--recursive' '--dir' '--verbose' '--help'
/// '--version'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("force", Takes::Nothing),
    ("interactive", Takes::Optional),
    ("one-file-system", Takes::Nothing),
    ("no-preserve-root", Takes::Nothing),
    ("preserve-root", Takes::Optional),
    // The leading hyphen is part of the *name*, not a typo: GNU's table really
    // holds `-presume-input-tty`, so it is typed with three dashes. It is
    // deliberately unspellable-by-accident because it is `rm`'s own internal
    // handle for "pretend stdin is a terminal", not a user-facing option.
    //
    // Because the name begins with `-`, it is reachable only from a `---`
    // prefix, so it can never collide with a normal `--name`: `rm ---p`
    // resolves to it, and `rm --p` still means `--preserve-root` alone.
    ("-presume-input-tty", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("dir", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--interactive`'s words, in GNU's `interactive_args[]` order.
///
/// Three spellings share `Never` and two share `Always`, which is why
/// [`getopt::Program::argmatch`] judges ambiguity by value: `--interactive=n`
/// matches `never`, `no` and `none`, all of which mean the same thing, so it
/// resolves rather than being refused. `--interactive=` matches everything and
/// is ambiguous. Both measured.
const INTERACTIVE_ARGS: &[(&str, Interactive)] = &[
    ("never", Interactive::Never),
    ("no", Interactive::Never),
    ("none", Interactive::Never),
    ("once", Interactive::Once),
    ("always", Interactive::Always),
    ("yes", Interactive::Always),
];

/// When to ask.
///
/// Four states, not a `bool`, because [`Interactive::Default`] and
/// [`Interactive::Never`] are genuinely different: the default still prompts
/// before a write-protected file on a terminal, and `Never` does not prompt at
/// all. Collapsing them is the mistake that makes `rm --interactive=never` on
/// a terminal ask about a read-only file.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Interactive {
    /// No interactive option given: prompt only for a write-protected file,
    /// and only when standard input is a terminal.
    #[default]
    Default,
    /// `-f`, `--interactive=never|no|none`. Never prompt.
    Never,
    /// `-I`, `--interactive=once`. One prompt up front, for more than three
    /// operands or for any recursive removal.
    Once,
    /// `-i`, `--interactive=always|yes`, bare `--interactive`. Prompt for
    /// everything.
    Always,
}

/// The command line, as `rm`'s `struct rm_options` plus the two flags GNU
/// keeps in `main`.
///
/// The bools are separate fields rather than a bitflag set because each is a
/// distinct upstream field and the interactions between them are the whole
/// difficulty of this utility; packing them would hide exactly the thing that
/// has to stay readable.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    /// `-r`, `-R`, `--recursive`.
    recursive: bool,
    /// `-d`, `--dir`: remove an empty directory without `-r`.
    dir: bool,
    /// `-v`, `--verbose`.
    verbose: bool,
    /// When to prompt.
    interactive: Interactive,
    /// `-f`'s *other* half: a missing operand, and a missing file, are not
    /// errors. Cleared again by `-i`, `-I`, `--interactive=once` and
    /// `--interactive=always`, but **not** by `--interactive=never`.
    ignore_missing_files: bool,
    /// `--one-file-system`.
    one_file_system: bool,
    /// `--preserve-root` (the default) versus `--no-preserve-root`.
    preserve_root: bool,
    /// `--preserve-root=all`: also refuse an operand that is a mount point.
    preserve_all_root: bool,
    /// `---presume-input-tty`, which does nothing but claim stdin is a
    /// terminal.
    presume_tty: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            recursive: false,
            dir: false,
            verbose: false,
            interactive: Interactive::Default,
            ignore_missing_files: false,
            one_file_system: false,
            // The one field whose default is not `false`. GNU's failsafe is on
            // unless `--no-preserve-root` turns it off, and a derived `Default`
            // would silently arm nothing.
            preserve_root: true,
            preserve_all_root: false,
            presume_tty: false,
        }
    }
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Options, Vec<OsString>),
}

/// The funnel. A diagnostic that could not be written turns the earned status
/// into `exit_failure`, which is what upstream's `atexit (close_stdout)` does
/// on every exit path at once. See [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            diag!("rm: {}", e.message());
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, and fail like any
    // other: `rm --help >&-` is a write error, not a success.
    let mut out = Stream::stdout();
    let (options, operands) = match request {
        Request::Help => return say(out, help_text().as_bytes()),
        Request::Version => return say(out, b"rm (SlateOS coreutils) 0.1.0\n"),
        Request::Run(options, operands) => (options, operands),
    };

    let stdin_tty = options.presume_tty || stdfd::is_tty(0);
    let mut err = Stream::stderr();
    let mut answers = StdinAnswers::new();
    let earned = {
        let mut rm = Rm {
            options: &options,
            out: &mut out,
            err: &mut err,
            answers: &mut answers,
            stdin_tty,
            root: file_id(Path::new("/")),
            failed: false,
        };
        rm.run(&operands);
        if rm.failed {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        }
    };
    stdfd::close_stdout("rm", out, earned)
}

/// Say one thing and stop — `--help` and `--version`.
fn say(mut out: Stream, bytes: &[u8]) -> ExitCode {
    let _ = out.write_all(bytes);
    stdfd::close_stdout("rm", out, ExitCode::SUCCESS)
}

/// GNU's `--help`, minus the project's `Report bugs to:` block, as every
/// converted utility here omits it.
fn help_text() -> String {
    "\
Usage: rm [OPTION]... [FILE]...
Remove (unlink) the FILE(s).

  -f, --force           ignore nonexistent files and arguments, never prompt
  -i                    prompt before every removal
  -I                    prompt once before removing more than three files, or
                          when removing recursively; less intrusive than -i,
                          while still giving protection against most mistakes
      --interactive[=WHEN]  prompt according to WHEN: never, once (-I), or
                          always (-i); without WHEN, prompt always
      --one-file-system  when removing a hierarchy recursively, skip any
                          directory that is on a file system different from
                          that of the corresponding command line argument
      --no-preserve-root  do not treat '/' specially
      --preserve-root[=all]  do not remove '/' (default);
                              with 'all', reject any command line argument
                              on a separate device from its parent
  -r, -R, --recursive   remove directories and their contents recursively
  -d, --dir             remove empty directories
  -v, --verbose         explain what is being done
      --help        display this help and exit
      --version     output version information and exit

By default, rm does not remove directories.  Use the --recursive (-r or -R)
option to remove each listed directory, too, along with all of its contents.

To remove a file whose name starts with a '-', for example '-foo',
use one of these commands:
  rm -- -foo

  rm ./-foo

Note that if you use rm to remove a file, it might be possible to recover
some of its contents, given sufficient expertise and/or time.  For greater
assurance that the contents are truly unrecoverable, consider using shred(1).
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `rm`'s argv.
///
/// Options and operands may be interleaved — `rm a -r b` sets `-r` and removes
/// both — which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// Any getopt diagnostic, `argmatch`'s for an `--interactive` value naming no
/// mode or several, and GNU's own hand-written one for a `--preserve-root`
/// value other than `all`.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut operands: Vec<OsString> = Vec::new();

    // Bound rather than consumed by a `for` loop, because one arm below needs
    // to ask the parser which argv word the item it just yielded came out of.
    let mut parser = RM.parse(args, SHORT_OPTIONS, LONG_OPTIONS);
    while let Some(item) = parser.next() {
        match item? {
            // Two fields, and the order they are written in matters: a later
            // `-i` puts `ignore_missing_files` back to false, which is why
            // `rm -f -i nope` reports and `rm -i -f nope` does not.
            Opt::Short(b'f', _) | Opt::Long("force", _) => {
                options.interactive = Interactive::Never;
                options.ignore_missing_files = true;
            }
            Opt::Short(b'i', _) => {
                options.interactive = Interactive::Always;
                options.ignore_missing_files = false;
            }
            Opt::Short(b'I', _) => {
                options.interactive = Interactive::Once;
                options.ignore_missing_files = false;
            }
            Opt::Long("interactive", value) => {
                let when = match value {
                    Some(word) => {
                        RM.argmatch(&os_bytes(&word), "--interactive", INTERACTIVE_ARGS)?
                    }
                    // Bare `--interactive` is `always`.
                    None => Interactive::Always,
                };
                options.interactive = when;
                // `never` leaves `ignore_missing_files` exactly as it found it
                // — the one asymmetry in this whole table, and the reason
                // `rm -f --interactive=never` with no operands is silent.
                if when != Interactive::Never {
                    options.ignore_missing_files = false;
                }
            }
            Opt::Short(b'r' | b'R', _) | Opt::Long("recursive", _) => options.recursive = true,
            Opt::Short(b'd', _) | Opt::Long("dir", _) => options.dir = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => options.verbose = true,
            Opt::Long("one-file-system", _) => options.one_file_system = true,
            Opt::Long("no-preserve-root", _) => {
                // `current_word` is still describing the item just yielded;
                // the next `next()` would move it on. It is the whole argv
                // word, which for a long option is the `--name` as typed.
                let typed = parser.current_word().map(|w| os_bytes(w));
                if typed.as_deref() != Some(b"--no-preserve-root".as_slice()) {
                    return Err(no_abbreviating());
                }
                options.preserve_root = false;
            }
            Opt::Long("preserve-root", value) => {
                if let Some(word) = value {
                    let bytes = os_bytes(&word);
                    if *bytes != *b"all" {
                        return Err(bad_preserve_root(&bytes));
                    }
                    options.preserve_all_root = true;
                }
                options.preserve_root = true;
            }
            Opt::Long("-presume-input-tty", _) => options.presume_tty = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => operands.push(word.clone()),
            // Every entry of the two tables is handled above; an unknown one
            // arrives as an `Err` from `parse`.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(options, operands))
}

/// `--preserve-root=<anything but all>`.
///
/// Not an `argmatch` diagnostic and not a usage one: upstream writes this by
/// hand with `die`, so it carries **no** `Try 'rm --help'` referral where
/// every other bad-value message here does. Measured:
///
/// ```text
/// $ rm -rf --preserve-root=bad tree; echo $?
/// rm: unrecognized --preserve-root argument: 'bad'
/// 1
/// ```
/// `--no-p`, `--no-pre`, or any other abbreviation of `--no-preserve-root`.
///
/// Every other long option here may be shortened to its shortest unambiguous
/// prefix, and this one may not — deliberately, and it is the only option in
/// coreutils treated this way. The reason is the blast radius: `--no-preserve-root`
/// is the switch that lets `rm -rf /` proceed, so upstream will not let a
/// typo-adjacent abbreviation reach it. `rm.c` implements it by comparing
/// `argv[optind - 1]` against the literal spelling after `getopt_long` has
/// already resolved the prefix, which is exactly what
/// [`current_word`](getopt::Parser::current_word) reproduces here.
///
/// Like [`bad_preserve_root`] it is a `die` rather than a usage error, so
/// there is no `Try 'rm --help'` referral. Measured:
///
/// ```text
/// $ rm --no-p tree/a.txt; echo $?
/// rm: you may not abbreviate the --no-preserve-root option
/// 1
/// ```
///
/// The word is not echoed back, so nothing needs quoting.
fn no_abbreviating() -> getopt::Error {
    getopt::Error {
        sentence: "you may not abbreviate the --no-preserve-root option".to_owned(),
        referral: None,
        status: 1,
    }
}

fn bad_preserve_root(given: &[u8]) -> getopt::Error {
    getopt::Error {
        sentence: format!("unrecognized --preserve-root argument: {}", quoteaf(given)),
        referral: None,
        status: 1,
    }
}

// ------------------------------------------------------------- the answer ---
//
// What counts as an answer is [`coreutils::yesno`], shared with `cp -i` and
// `find -ok`: `rm` and `find` had each written their own and already disagreed
// about a non-UTF-8 line. What is *asked* stays here — the wording is per
// utility, and `rm`'s is the most elaborate of them (see [`Rm::prompt`]).

// --------------------------------------------------------------- removal ----

/// What happened to one entry, from the point of view of the directory that
/// contains it.
///
/// The distinction between the last two is not a nicety: it is the whole of
/// GNU's `mark_ancestor_dirs`, and getting it wrong changes both the output
/// and the exit status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    /// Gone. The parent may go too.
    Removed,
    /// The user declined to remove *this* entry. The parent is still asked
    /// about, still attempts its own removal, and still fails with
    /// `Directory not empty` — which is an error, and does set the status.
    /// Measured; it is not what a "declined means skip the parent" reading
    /// predicts.
    Declined,
    /// A failure was reported, or a *descend* was declined. Every enclosing
    /// directory is skipped in silence: no prompt, no message, no second
    /// error. The status was already set by whatever caused this, if anything
    /// was — a declined descend exits 0.
    Abandoned,
}

/// Which question is being asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Question {
    /// `descend into [write-protected ]directory 'x'? `
    Descend,
    /// `remove [write-protected ]<type> 'x'? `
    Remove,
}

/// A file's identity, for "is this the same file as `/`?".
///
/// Two shapes because the honest answer is `st_dev` plus `st_ino` and only
/// unix exposes those on stable Rust. The canonical path is a stand-in good
/// enough for the development host's tests; it would be wrong for a hard link,
/// and `/` cannot be hard-linked.
#[derive(Clone, PartialEq, Eq, Debug)]
enum FileId {
    #[cfg(unix)]
    DevIno(u64, u64),
    #[cfg(not(unix))]
    Canonical(std::path::PathBuf),
}

/// The identity of a file whose lookup is already in hand.
///
/// `None` means "no identity could be established", which every caller must
/// read as "not known to be the same file" — never as a match. Off unix the
/// lookup is unusable and the path is canonicalised instead, which is why this
/// takes both.
fn id_of(path: &Path, st: &Stat) -> Option<FileId> {
    #[cfg(unix)]
    {
        let _ = path;
        st.identity().map(|(dev, ino)| FileId::DevIno(dev, ino))
    }
    #[cfg(not(unix))]
    {
        let _ = st;
        fs::canonicalize(path).ok().map(FileId::Canonical)
    }
}

/// The identity of a file named by path, looked up now. Used once, for `/`.
fn file_id(path: &Path) -> Option<FileId> {
    let st = Stat::of_path(path).ok()?;
    id_of(path, &st)
}

/// Where an entry is — both for the kernel and for the reader of a message.
///
/// The two are deliberately different things, and keeping them apart is the
/// whole of the fix recorded in this file's "The walk resolves descriptors, not
/// paths". `path` is the string GNU prints and `scripts/rm-diff.sh` certifies;
/// `dir` and `name` are what actually reach a syscall.
///
/// `dir` is `None` for exactly one entry per operand — the operand itself,
/// which has no descriptor above it because nothing has been opened yet. Every
/// other entry in the walk has one, and reaching an entry through its parent's
/// descriptor is what a swapped component cannot redirect.
struct Loc<'a> {
    /// The open parent directory, or `None` at a command-line operand.
    dir: Option<&'a Dir>,
    /// The single component naming this entry inside `dir`. Not read when
    /// `dir` is `None`, where the operand may be any number of components.
    name: &'a [u8],
    /// The spelling to print, and — at an operand only — the path to resolve.
    path: &'a [u8],
}

impl<'a> Loc<'a> {
    /// A command-line operand: reached by the name the user typed.
    fn top(path: &'a [u8]) -> Self {
        Self {
            dir: None,
            name: path,
            path,
        }
    }

    /// An entry inside an open directory.
    fn in_dir(dir: &'a Dir, name: &'a [u8], path: &'a [u8]) -> Self {
        Self {
            dir: Some(dir),
            name,
            path,
        }
    }

    /// What this entry is, without following it if it is a link.
    fn stat(&self) -> io::Result<Stat> {
        match self.dir {
            Some(dir) => dir.stat(self.name),
            None => Stat::of_path(&as_path(self.path)),
        }
    }

    /// Remove a non-directory.
    fn unlink(&self) -> io::Result<()> {
        match self.dir {
            Some(dir) => dir.unlink(self.name),
            None => fs::remove_file(as_path(self.path)),
        }
    }

    /// Remove an empty directory.
    fn rmdir(&self) -> io::Result<()> {
        match self.dir {
            Some(dir) => dir.rmdir(self.name),
            None => fs::remove_dir(as_path(self.path)),
        }
    }

    /// Open this entry as a directory, so the walk can go on below it.
    ///
    /// `st` is the lookup that decided it *was* a directory, and the descriptor
    /// is checked against it — a name that resolved somewhere else since is
    /// refused rather than descended into. See [`coreutils::dirfd`].
    fn open_dir(&self, st: &Stat) -> io::Result<Dir> {
        match self.dir {
            Some(dir) => dir.open_child(self.name, st),
            None => Dir::open_root(&as_path(self.path), st),
        }
    }

    /// Whether the prompt should say `write-protected`.
    ///
    /// GNU distinguishes `EACCES` (write-protected) from any other failure (an
    /// error in its own right, reported and the entry skipped). This treats
    /// everything that is not a plain success as "not write-protected"
    /// instead: the removal itself is about to run and will report the real
    /// problem with the real errno, and inventing a second diagnostic here
    /// could only turn a removable file into a refused one.
    fn write_protected(&self) -> bool {
        let writable = match self.dir {
            Some(dir) => dir.writable(self.name),
            None => path_writable(self.path),
        };
        writable == Some(false)
    }
}

/// One `rm` run: the options, the two output streams, the answer source, and
/// the exit status being earned.
struct Rm<'a> {
    options: &'a Options,
    /// Verbose output. Standard output, as upstream.
    out: &'a mut dyn Write,
    /// Diagnostics **and prompts**. Both go to standard error, which is why
    /// `rm -i ... 2>/dev/null` swallows the question and not the answer.
    err: &'a mut dyn Write,
    answers: &'a mut dyn Answers,
    stdin_tty: bool,
    /// The identity of `/`, or `None` if it could not be looked up. Compared
    /// against each recursive operand when [`Options::preserve_root`] is on.
    root: Option<FileId>,
    /// Set by every reported failure. This, not [`Verdict`], is the exit
    /// status: declining a prompt is not a failure.
    failed: bool,
}

impl Rm<'_> {
    /// Every operand, in order. One failure does not stop the others.
    fn run(&mut self, operands: &[OsString]) {
        if operands.is_empty() {
            // `-f` makes a missing operand not an error, which is what lets
            // `rm -f $maybe_empty` work in a shell script. Note the test is
            // `ignore_missing_files`, not "is interactive Never": measured,
            // `rm --interactive=never` alone *is* `missing operand`.
            if !self.options.ignore_missing_files {
                let sentence = RM.usage_referring("missing operand".into());
                self.diagnose(&sentence.message());
            }
            return;
        }

        if !self.prompt_once(operands.len()) {
            return;
        }

        for operand in operands {
            self.operand(operand);
        }
    }

    /// `-I`'s single up-front question. Returns whether to go on.
    ///
    /// The gate is measured: three operands is silent, four asks, and any
    /// number asks when `-r` is on. A nonexistent operand still counts.
    fn prompt_once(&mut self, count: usize) -> bool {
        if self.options.interactive != Interactive::Once {
            return true;
        }
        let recursive = self.options.recursive;
        if !(count > 3 || (recursive && count > 0)) {
            return true;
        }
        let noun = if count == 1 { "argument" } else { "arguments" };
        let how = if recursive { " recursively" } else { "" };
        self.ask(&format!("rm: remove {count} {noun}{how}? "))
    }

    /// One command-line operand, with the checks that apply only at the top.
    fn operand(&mut self, operand: &OsString) {
        // gnulib `fts`'s trailing-slash rule, applied once. From here on the
        // normalised spelling is both what gets printed and what gets passed
        // to the syscalls; collapsing a run of trailing slashes cannot change
        // which file a path names.
        let path = normalize_operand(&os_bytes(operand));
        let loc = Loc::top(&path);

        let st = match loc.stat() {
            Ok(st) => st,
            Err(e) if e.kind() == io::ErrorKind::NotFound && self.options.ignore_missing_files => {
                return;
            }
            Err(e) => {
                self.cannot_remove(&path, &e);
                return;
            }
        };

        // Both top-level refusals are GNU's, and both apply only to a
        // directory being removed recursively — which is why `rm -d .` is
        // `Directory not empty` and `rm .` is `Is a directory`, neither of
        // them a refusal.
        if st.is_dir() && self.options.recursive {
            if is_dot_or_dotdot(&path) {
                // POSIX: diagnose it and do nothing more with that argument.
                self.diagnose(&format!(
                    "refusing to remove '.' or '..' directory: skipping {}",
                    quoteaf(&path)
                ));
                return;
            }
            // `self.root.is_some()` first: two unknown identities are not a
            // match, and without the guard `None == None` would refuse every
            // recursive operand on a host where `/` cannot be looked up.
            if self.options.preserve_root
                && self.root.is_some()
                && self.root == id_of(&as_path(&path), &st)
            {
                let name = quoteaf(&path);
                let same = if *path == *b"/" {
                    String::new()
                } else {
                    format!(" (same as {})", quoteaf(b"/"))
                };
                self.diagnose(&format!(
                    "it is dangerous to operate recursively on {name}{same}"
                ));
                self.diagnose("use --no-preserve-root to override this failsafe");
                return;
            }
            if self.options.preserve_all_root && self.crosses_into_its_parent(&path, &st) {
                return;
            }
        }

        let top = st.dev();
        self.entry(&loc, &st, 0, top);
    }

    /// `--preserve-root=all`: an operand that is a mount point — on a
    /// different device from its own parent — is refused.
    ///
    /// Returns whether it was refused. Not certified against GNU: making a
    /// mount point needs root, which the differential harness has not got.
    fn crosses_into_its_parent(&mut self, path: &[u8], st: &Stat) -> bool {
        let mut parent = strip_trailing_slashes(path).to_vec();
        parent.extend_from_slice(b"/..");
        let parent_st = match Stat::of_path(&as_path(&parent)) {
            Ok(m) => m,
            Err(_) => {
                self.diagnose(&format!(
                    "failed to stat {}: skipping {}",
                    quoteaf(&parent),
                    quoteaf(path)
                ));
                return true;
            }
        };
        if st.dev() != parent_st.dev() {
            self.diagnose(&format!(
                "skipping {}, since it's on a different device",
                quoteaf(path)
            ));
            self.diagnose("and --preserve-root=all is in effect");
            return true;
        }
        false
    }

    /// One entry, at `level` below its command-line operand.
    ///
    /// `top` is the device the operand itself is on, for `--one-file-system`.
    fn entry(&mut self, loc: &Loc<'_>, st: &Stat, level: u32, top: Option<u64>) -> Verdict {
        if !st.is_dir() {
            return self.remove_nondirectory(loc, st);
        }

        // `--one-file-system` skips a directory below the operand that is on
        // another filesystem. The operand itself is never skipped: it is what
        // defines the filesystem to stay on.
        if level > 0 && self.options.one_file_system && top.is_some() && st.dev() != top {
            self.diagnose(&format!(
                "skipping {}, since it's on a different device",
                quoteaf(loc.path)
            ));
            return Verdict::Abandoned;
        }

        if self.options.recursive {
            self.remove_tree(loc, st, level, top)
        } else if self.options.dir {
            self.remove_empty_directory(loc, st)
        } else {
            // Not `-r` and not `-d`: the directory is not read at all, which
            // is why an unreadable one still answers `Is a directory`.
            self.cannot_remove(loc.path, &io::Error::from(io::ErrorKind::IsADirectory));
            Verdict::Abandoned
        }
    }

    /// A file, symlink, fifo, socket or device node: prompt, then unlink.
    fn remove_nondirectory(&mut self, loc: &Loc<'_>, st: &Stat) -> Verdict {
        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        match loc.unlink() {
            Ok(()) => {
                self.verbose("removed", loc.path);
                Verdict::Removed
            }
            // Vanished between the stat and the unlink. Under `-f` that is the
            // outcome asked for.
            Err(e) if self.options.ignore_missing_files && e.kind() == io::ErrorKind::NotFound => {
                Verdict::Removed
            }
            Err(e) => {
                self.cannot_remove(loc.path, &e);
                Verdict::Abandoned
            }
        }
    }

    /// `-d` without `-r`: only an empty directory may go.
    fn remove_empty_directory(&mut self, loc: &Loc<'_>, st: &Stat) -> Verdict {
        let children = match list(loc, st) {
            Ok((_, names)) => names,
            Err(e) => {
                self.cannot_remove(loc.path, &e);
                return Verdict::Abandoned;
            }
        };
        if !children.is_empty() {
            // No prompt: measured, `rm -i -d nonempty` asks nothing and goes
            // straight to the error, because the `rmdir` cannot succeed.
            self.cannot_remove(loc.path, &io::Error::from(io::ErrorKind::DirectoryNotEmpty));
            return Verdict::Abandoned;
        }
        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        self.rmdir(loc)
    }

    /// `-r`: the directory, its contents, and the two prompts around them.
    fn remove_tree(&mut self, loc: &Loc<'_>, st: &Stat, level: u32, top: Option<u64>) -> Verdict {
        // Open and read first. A directory that cannot be read is reported
        // without ever being prompted about — measured, and the natural order
        // anyway, since whether it is empty decides which question gets asked.
        let (dir, children) = match list(loc, st) {
            Ok(pair) => pair,
            Err(e) => {
                self.cannot_remove(loc.path, &e);
                return Verdict::Abandoned;
            }
        };

        if children.is_empty() {
            // One question, not two: there is nothing to descend into.
            if !self.prompt(loc, st, Question::Remove) {
                return Verdict::Declined;
            }
            drop(dir);
            return self.rmdir(loc);
        }

        if !self.prompt(loc, st, Question::Descend) {
            // A declined *descend* abandons the enclosing directories in
            // silence, and is not an error.
            return Verdict::Abandoned;
        }

        let mut worst = Verdict::Removed;
        for name in children {
            // `join` builds the string that gets *printed*; `Loc::in_dir` is
            // what the syscalls see, and it carries the open parent rather than
            // the string. That split is the fix — see this file's header.
            let child_path = join(loc.path, &name);
            let child = Loc::in_dir(&dir, &name, &child_path);
            let verdict = match child.stat() {
                Ok(child_st) => self.entry(&child, &child_st, level.saturating_add(1), top),
                Err(e)
                    if e.kind() == io::ErrorKind::NotFound && self.options.ignore_missing_files =>
                {
                    Verdict::Removed
                }
                Err(e) => {
                    self.cannot_remove(&child_path, &e);
                    Verdict::Abandoned
                }
            };
            worst = worse(worst, verdict);
        }

        if worst == Verdict::Abandoned {
            // Silence, deliberately: the child already said what went wrong,
            // and a second message about the parent would be noise.
            return Verdict::Abandoned;
        }

        if !self.prompt(loc, st, Question::Remove) {
            return Verdict::Declined;
        }
        // Closed before the `rmdir`, not left to fall out of scope after it.
        // Unix does not mind removing a directory somebody still has open, but
        // the host build does, and a descriptor whose only remaining purpose is
        // to be dropped is worth dropping where the reason is visible.
        drop(dir);
        // With a declined child still in it this fails with `Directory not
        // empty`, which is exactly what GNU prints. The failure is not
        // special-cased into silence.
        self.rmdir(loc)
    }

    fn rmdir(&mut self, loc: &Loc<'_>) -> Verdict {
        match loc.rmdir() {
            Ok(()) => {
                self.verbose("removed directory", loc.path);
                Verdict::Removed
            }
            Err(e) if self.options.ignore_missing_files && e.kind() == io::ErrorKind::NotFound => {
                Verdict::Removed
            }
            Err(e) => {
                self.cannot_remove(loc.path, &e);
                Verdict::Abandoned
            }
        }
    }

    // ------------------------------------------------------------ prompts --

    /// GNU's `prompt()`. Returns whether to go ahead.
    fn prompt(&mut self, loc: &Loc<'_>, st: &Stat, question: Question) -> bool {
        if self.options.interactive == Interactive::Never {
            return true;
        }

        // The write-protection probe is itself conditional: it costs a syscall
        // per entry, and upstream only pays it when the answer could change
        // anything. A symlink is never probed — the bit that matters would be
        // the target's.
        let write_protected = !self.options.ignore_missing_files
            && (self.options.interactive == Interactive::Always || self.stdin_tty)
            && !st.is_symlink()
            && loc.write_protected();

        if !(write_protected || self.options.interactive == Interactive::Always) {
            return true;
        }

        let name = quoteaf(loc.path);
        let sentence = match (question, write_protected) {
            (Question::Descend, true) => {
                format!("rm: descend into write-protected directory {name}? ")
            }
            (Question::Descend, false) => format!("rm: descend into directory {name}? "),
            (Question::Remove, true) => {
                format!("rm: remove write-protected {} {name}? ", file_type(st))
            }
            (Question::Remove, false) => format!("rm: remove {} {name}? ", file_type(st)),
        };
        self.ask(&sentence)
    }

    /// Put a question and read the answer. No trailing newline: the cursor
    /// stays on the question's line, as upstream.
    fn ask(&mut self, sentence: &str) -> bool {
        let _ = self.err.write_all(sentence.as_bytes());
        let _ = self.err.flush();
        yesno(self.answers)
    }

    // ------------------------------------------------------------- output --

    fn verbose(&mut self, what: &str, path: &[u8]) {
        if self.options.verbose {
            let _ = writeln!(self.out, "{what} {}", quoteaf(path));
        }
    }

    /// `rm: cannot remove 'x': <why>`, and the exit status with it.
    ///
    /// `strerror`, not `{e}`: why it failed has to read the same wherever it
    /// is printed. On a Windows *host* `{e}` says `The system cannot find the
    /// file specified. (os error 2)`, which is neither POSIX's wording nor
    /// what this utility prints on the target it ships on.
    fn cannot_remove(&mut self, path: &[u8], e: &io::Error) {
        self.diagnose(&format!("cannot remove {}: {}", quoteaf(path), strerror(e)));
    }

    fn diagnose(&mut self, sentence: &str) {
        self.failed = true;
        let _ = writeln!(self.err, "rm: {sentence}");
    }
}

/// Open a directory the walk is about to act on, and read its names.
///
/// The two are returned together because they must not be separated: the names
/// are only meaningful as names *inside that descriptor*, and a caller that
/// kept the list but dropped the handle would be back to resolving them by
/// path — which is the bug this file's header is about.
///
/// The whole listing is read before anything is removed, as `fts` does, so the
/// order is `readdir`'s — which is observable through `-v`.
fn list(loc: &Loc<'_>, st: &Stat) -> io::Result<(Dir, Vec<Vec<u8>>)> {
    let dir = loc.open_dir(st)?;
    let names = dir.names()?;
    Ok((dir, names))
}

/// The more serious of two verdicts, for a directory summarising its children.
fn worse(a: Verdict, b: Verdict) -> Verdict {
    match (a, b) {
        (Verdict::Abandoned, _) | (_, Verdict::Abandoned) => Verdict::Abandoned,
        (Verdict::Declined, _) | (_, Verdict::Declined) => Verdict::Declined,
        _ => Verdict::Removed,
    }
}

/// gnulib's `file_type()`, whose words appear in the prompts verbatim.
///
/// It reads the [`Stat`] the walk already took rather than taking one of its
/// own — the prompt has to describe the same file the removal is about to act
/// on, and a second lookup by path could describe a different one.
fn file_type(st: &Stat) -> &'static str {
    match st.kind() {
        Kind::SymbolicLink => "symbolic link",
        Kind::Directory => "directory",
        Kind::BlockDevice => "block special file",
        Kind::CharDevice => "character special file",
        Kind::Fifo => "fifo",
        Kind::Socket => "socket",
        // The empty/non-empty split is upstream's, and visible: `rm -i` on a
        // zero-length file says `regular empty file`.
        Kind::Regular if st.size() == 0 => "regular empty file",
        Kind::Regular => "regular file",
        Kind::Other => "weird file",
    }
}

/// gnulib `fts`'s rule for a root operand's spelling, reproduced exactly:
///
/// > If there are two or more trailing slashes, trim all but one, but don't
/// > change `//` to `/`, and do not modify a lone `/`, and do not trim a
/// > trailing slash from a name like `x/`.
///
/// This is observable and not merely internal: `rm -rv tree//` ends
/// `removed directory 'tree/'`, while `rm -rf //` refuses `'//'` and
/// `rm -rf ///` refuses `'/'`.
fn normalize_operand(arg: &[u8]) -> Vec<u8> {
    let mut len = arg.len();
    if len > 2 && arg.get(len.saturating_sub(1)) == Some(&b'/') {
        while len > 1 && arg.get(len.saturating_sub(2)) == Some(&b'/') {
            len = len.saturating_sub(1);
        }
    }
    arg.get(..len).unwrap_or(arg).to_vec()
}

/// gnulib `fts`'s `NAPPEND`: a parent path ending in `/` loses that one slash
/// before the separator goes on, so `tree/` yields `tree/a.txt` and not
/// `tree//a.txt`. An interior double slash is left alone — `tree//sub` yields
/// `tree//sub/b.txt` — because only the *last* character is examined.
fn join(parent: &[u8], name: &[u8]) -> Vec<u8> {
    let trimmed = if parent.last() == Some(&b'/') {
        parent
            .get(..parent.len().saturating_sub(1))
            .unwrap_or(parent)
    } else {
        parent
    };
    let mut out = trimmed.to_vec();
    out.push(b'/');
    out.extend_from_slice(name);
    out
}

fn strip_trailing_slashes(path: &[u8]) -> &[u8] {
    let mut len = path.len();
    while len > 0 && path.get(len.saturating_sub(1)) == Some(&b'/') {
        len = len.saturating_sub(1);
    }
    path.get(..len).unwrap_or(path)
}

/// gnulib's `dot_or_dotdot (last_component (name))`.
///
/// Trailing slashes do not hide it: `rm -r ./` is refused just as `rm -r .` is.
fn is_dot_or_dotdot(path: &[u8]) -> bool {
    let body = strip_trailing_slashes(path);
    if body.is_empty() {
        // The whole name was slashes, so it names the root, not a dot.
        return false;
    }
    let last = match body.iter().rposition(|&c| c == b'/') {
        Some(at) => body.get(at.saturating_add(1)..).unwrap_or_default(),
        None => body,
    };
    last == b"." || last == b".."
}

/// A byte path as something a syscall will take. See `quote::os_from_bytes`
/// for why the round trip is the only correct one on this OS.
fn as_path(path: &[u8]) -> std::path::PathBuf {
    std::path::PathBuf::from(os_from_bytes(path))
}

#[cfg(unix)]
unsafe extern "C" {
    /// `euidaccess(path, mode)`, where mode 2 is `W_OK`. The *effective* uid
    /// is the one that matters: `access(2)` asks about the real one, which for
    /// a setuid `rm` would answer a question nobody asked.
    fn euidaccess(path: *const u8, mode: i32) -> i32;
}

/// Whether a command-line operand may be written by the effective user, or
/// `None` if the question could not be answered.
///
/// The by-path twin of [`coreutils::dirfd::Dir::writable`], and used only where
/// that one cannot be: at an operand, which has no descriptor above it. Below
/// one, the probe goes through the parent's handle like everything else.
#[cfg(unix)]
fn path_writable(path: &[u8]) -> Option<bool> {
    let mut c_path = path.to_vec();
    if c_path.contains(&0) {
        return None;
    }
    c_path.push(0);
    // SAFETY: `c_path` is NUL-terminated, has no interior NUL, and outlives
    // the call. `euidaccess` reads it and does not retain it.
    let rc = unsafe { euidaccess(c_path.as_ptr(), 2) };
    Some(rc == 0)
}

/// Off unix there is no `euidaccess`, so the question is unanswerable, nothing
/// is ever reported as write-protected, and the default interactivity never
/// prompts. That is the conservative direction only in the sense that it
/// matches `--interactive=never`; the host build is a test vehicle, not a
/// shipping one.
#[cfg(not(unix))]
fn path_writable(_path: &[u8]) -> Option<bool> {
    None
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::expect_used
)]
mod tests {
    use super::*;
    /// The canned answer queue is shared with `cp`'s prompt tests; see
    /// [`coreutils::yesno`].
    use coreutils::yesno::Canned;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(options, operands)` from a successful parse, or a panic naming the
    /// error.
    fn run_parse(items: &[&str]) -> (Options, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(o, p) => (
                o,
                p.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn no_args() {
        let (o, p) = run_parse(&[]);
        assert_eq!(o, Options::default());
        assert!(p.is_empty());
    }

    #[test]
    fn just_paths() {
        let (o, p) = run_parse(&["a", "b"]);
        assert_eq!(o, Options::default());
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn preserve_root_is_on_by_default() {
        let (o, _) = run_parse(&["a"]);
        assert!(o.preserve_root, "the failsafe must be armed without asking");
        assert!(!o.preserve_all_root);
    }

    #[test]
    fn recursive_spellings() {
        for spelling in ["-r", "-R", "--recursive", "--rec"] {
            let (o, _) = run_parse(&[spelling, "a"]);
            assert!(o.recursive, "{spelling}");
        }
    }

    #[test]
    fn dir_and_verbose() {
        let (o, _) = run_parse(&["-dv", "a"]);
        assert!(o.dir && o.verbose);
        let (o, _) = run_parse(&["--dir", "--verbose", "a"]);
        assert!(o.dir && o.verbose);
    }

    #[test]
    fn combined_rf() {
        let (o, p) = run_parse(&["-rf", "a"]);
        assert!(o.recursive && o.ignore_missing_files);
        assert_eq!(o.interactive, Interactive::Never);
        assert_eq!(p, vec!["a"]);
    }

    #[test]
    fn dash_alone_is_path() {
        let (_, p) = run_parse(&["-"]);
        assert_eq!(p, vec!["-"]);
    }

    #[test]
    fn flag_at_end() {
        let (o, p) = run_parse(&["a", "-r"]);
        assert!(o.recursive);
        assert_eq!(p, vec!["a"]);
    }

    /// The documented way to remove a file whose name begins with a dash.
    #[test]
    fn double_dash_ends_options() {
        let (o, p) = run_parse(&["-r", "--", "-foo", "-r"]);
        assert!(o.recursive);
        assert_eq!(p, vec!["-foo", "-r"], "after --, everything is an operand");
    }

    // ------------------------------------------- parsing: the four states --

    /// The measured table. Each row is a command line and the state it must
    /// leave behind; the last two rows are the pair that no "-f is the
    /// opposite of -i" model predicts.
    #[test]
    fn interactivity_is_four_states_and_order_matters() {
        let cases: &[(&[&str], Interactive, bool)] = &[
            (&[], Interactive::Default, false),
            (&["-f"], Interactive::Never, true),
            (&["-i"], Interactive::Always, false),
            (&["-I"], Interactive::Once, false),
            (&["--interactive"], Interactive::Always, false),
            (&["--interactive=always"], Interactive::Always, false),
            (&["--interactive=yes"], Interactive::Always, false),
            (&["--interactive=once"], Interactive::Once, false),
            (&["--interactive=never"], Interactive::Never, false),
            (&["--interactive=no"], Interactive::Never, false),
            (&["--interactive=none"], Interactive::Never, false),
            // Prefixes: all three "no" words share a value, so `n` resolves.
            (&["--interactive=n"], Interactive::Never, false),
            (&["--interactive=o"], Interactive::Once, false),
            // Order, and which fields each spelling touches.
            (&["-f", "-i"], Interactive::Always, false),
            (&["-i", "-f"], Interactive::Never, true),
            (&["-f", "-I"], Interactive::Once, false),
            (&["-I", "-f"], Interactive::Never, true),
            (&["-f", "--interactive=always"], Interactive::Always, false),
            // The asymmetry: `never` leaves `-f`'s second half standing.
            (&["-f", "--interactive=never"], Interactive::Never, true),
            (&["--interactive=never", "-f"], Interactive::Never, true),
        ];
        for (argv, when, ignore) in cases {
            let (o, _) = run_parse(argv);
            assert_eq!(o.interactive, *when, "{argv:?}");
            assert_eq!(
                o.ignore_missing_files, *ignore,
                "{argv:?} ignore_missing_files"
            );
        }
    }

    #[test]
    fn an_empty_interactive_value_is_ambiguous() {
        let e = fail(&["--interactive=", "a"]);
        assert!(
            e.sentence.contains("ambiguous argument"),
            "{:?}",
            e.sentence
        );
        assert_eq!(e.status, 1);
    }

    #[test]
    fn a_bad_interactive_value_lists_the_valid_ones() {
        let e = fail(&["--interactive=bad", "a"]);
        assert!(e.sentence.contains("invalid argument"), "{:?}", e.sentence);
        assert!(
            e.sentence.contains("Valid arguments are"),
            "{:?}",
            e.sentence
        );
        assert_eq!(e.referral, Some("rm"), "argmatch's message does refer");
    }

    // ---------------------------------------------- parsing: the failsafe --

    #[test]
    fn preserve_root_is_last_wins() {
        let (o, _) = run_parse(&["--no-preserve-root", "--preserve-root", "/"]);
        assert!(o.preserve_root);
        let (o, _) = run_parse(&["--preserve-root", "--no-preserve-root", "/"]);
        assert!(!o.preserve_root);
    }

    #[test]
    fn preserve_root_all_is_accepted() {
        let (o, _) = run_parse(&["--preserve-root=all", "/"]);
        assert!(o.preserve_root && o.preserve_all_root);
    }

    /// Upstream writes this one by hand with `die`, so unlike every other
    /// bad-value message here it carries **no** `Try 'rm --help'` line.
    #[test]
    fn a_bad_preserve_root_value_has_no_referral() {
        let e = fail(&["--preserve-root=bad", "a"]);
        assert_eq!(e.sentence, "unrecognized --preserve-root argument: 'bad'");
        assert_eq!(e.referral, None);
        assert_eq!(e.status, 1);
        assert_eq!(e.message(), e.sentence, "no second line");
    }

    /// The one long option in coreutils that may not be abbreviated, because
    /// it is the switch that disarms the `/` failsafe. Every prefix of it is
    /// unambiguous — nothing else in the table starts with `n` — so getopt
    /// resolves them all happily and the refusal has to be `rm`'s own.
    #[test]
    fn no_preserve_root_may_not_be_abbreviated() {
        for typed in [
            "--n",
            "--no",
            "--no-p",
            "--no-preserve",
            "--no-preserve-roo",
        ] {
            let e = fail(&[typed, "a"]);
            assert_eq!(
                e.sentence, "you may not abbreviate the --no-preserve-root option",
                "{typed}"
            );
            assert_eq!(e.referral, None, "{typed}");
            assert_eq!(e.status, 1, "{typed}");
        }
        // Spelled in full it works, and it is still the *only* option the rule
        // touches: `--p` for `--preserve-root` abbreviates freely.
        let (o, _) = run_parse(&["--no-preserve-root", "a"]);
        assert!(!o.preserve_root);
        let (o, _) = run_parse(&["--p", "a"]);
        assert!(o.preserve_root);
    }

    #[test]
    fn presume_input_tty_needs_three_dashes() {
        let (o, _) = run_parse(&["---presume-input-tty", "a"]);
        assert!(o.presume_tty);
        // And it stays out of the way of the ordinary names.
        let (o, _) = run_parse(&["--p", "a"]);
        assert!(o.preserve_root && !o.presume_tty);
        let (o, _) = run_parse(&["---p", "a"]);
        assert!(o.presume_tty);
    }

    #[test]
    fn one_file_system_is_recognised() {
        let (o, _) = run_parse(&["--one-file-system", "-r", "a"]);
        assert!(o.one_file_system);
    }

    // ------------------------------------------------- parsing: the rest ---

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-x", "a"]);
        assert_eq!(e.sentence, "invalid option -- 'x'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz", "a"]);
        assert_eq!(e.sentence, "unrecognized option '--zzz'");
    }

    /// `--v` must not resolve to `--version`. `--verbose` is in the table
    /// precisely so that this stays ambiguous rather than silently printing a
    /// version banner and deleting nothing.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v", "a"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("'--verbose'"), "{:?}", e.sentence);
        assert!(e.sentence.contains("'--version'"), "{:?}", e.sentence);
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--force=1", "a"]);
        assert_eq!(e.sentence, "option '--force' doesn't allow an argument");
    }

    /// The reason this file was first rewritten. A `String`-based parser
    /// panics here rather than returning; reaching the assert at all is the
    /// test.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        match parse_args(&[OsString::from("-r"), bad.clone()]).unwrap() {
            Request::Run(o, p) => {
                assert!(o.recursive);
                assert_eq!(p, vec![bad], "the operand must survive byte-for-byte");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'-', b'-', 0x80]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    /// The two tests above are `#[cfg(unix)]`, so on the development host —
    /// Windows — the regression tests for the bug this file was rewritten to
    /// fix would not run at all without these. Windows has its own argument
    /// that no `String` can hold: an unpaired surrogate.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone()]).unwrap() {
            Request::Run(o, p) => {
                assert!(o.recursive);
                assert_eq!(p, vec![bad], "the operand must survive unchanged");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x002D, 0x002D, 0xD800]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    // ------------------------------------------------------ path arithmetic

    #[test]
    fn trailing_slashes_collapse_to_one_but_not_to_none() {
        for (given, want) in [
            ("tree", "tree"),
            ("tree/", "tree/"),
            ("tree//", "tree/"),
            ("tree///", "tree/"),
            ("/", "/"),
            // `//` is not `/` and fts refuses to make it one.
            ("//", "//"),
            ("///", "/"),
            ("", ""),
            ("./tree", "./tree"),
            ("tree//sub", "tree//sub"),
        ] {
            assert_eq!(
                normalize_operand(given.as_bytes()),
                want.as_bytes(),
                "{given}"
            );
        }
    }

    #[test]
    fn joining_drops_one_trailing_slash_only() {
        assert_eq!(join(b"tree", b"a"), b"tree/a");
        assert_eq!(join(b"tree/", b"a"), b"tree/a");
        assert_eq!(join(b"tree//sub", b"a"), b"tree//sub/a");
        assert_eq!(join(b"/", b"a"), b"/a");
        assert_eq!(join(b"//", b"a"), b"//a".to_vec(), "only the last slash");
    }

    #[test]
    fn dot_and_dotdot_are_seen_through_trailing_slashes() {
        for yes in [
            ".",
            "..",
            "./",
            "../",
            "tree/.",
            "tree/sub/..",
            "/.",
            "/..",
            ".///",
        ] {
            assert!(is_dot_or_dotdot(yes.as_bytes()), "{yes}");
        }
        for no in [
            "tree", "tree/", ".hidden", "..a", "a..", "/", "//", "", "....",
        ] {
            assert!(!is_dot_or_dotdot(no.as_bytes()), "{no}");
        }
    }

    // What counts as a yes is tested in `coreutils::yesno`, where the rule
    // now lives; the tests below are about *which* question `rm` asks and
    // when, which is the part that is `rm`'s own.

    // ------------------------------------------------------------ removal --

    fn scratch(stem: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("rm_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Append `tail` after a **forward** slash.
    ///
    /// Not `PathBuf::push`, which uses the host separator. `rm` reads a path
    /// as bytes divided by `/` — that is the SlateOS separator and the only
    /// one gnulib knows — so on the Windows development host `dir.join(".")`
    /// yields `dir\.`, whose last `/`-delimited component is the whole thing
    /// and not `.`. A test written that way exercises nothing: the `.`/`..`
    /// refusal correctly declines to fire and the tree is really removed.
    /// Both separators reach the same file through a Windows syscall, so the
    /// mixed spelling is harmless for setup.
    fn slash(dir: &Path, tail: &str) -> std::path::PathBuf {
        let mut bytes = os_bytes(dir.as_os_str()).into_owned();
        bytes.push(b'/');
        bytes.extend_from_slice(tail.as_bytes());
        std::path::PathBuf::from(os_from_bytes(&bytes))
    }

    /// One run, with the whole transcript back.
    struct Ran {
        ok: bool,
        out: String,
        err: String,
    }

    fn run_with(options: &Options, operands: &[&Path], answers: &[&str], tty: bool) -> Ran {
        let owned: Vec<OsString> = operands.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut canned = Canned::new(answers);
        let mut rm = Rm {
            options,
            out: &mut out,
            err: &mut err,
            answers: &mut canned,
            stdin_tty: tty,
            root: file_id(Path::new("/")),
            failed: false,
        };
        rm.run(&owned);
        let failed = rm.failed;
        Ran {
            ok: !failed,
            out: String::from_utf8_lossy(&out).into_owned(),
            err: String::from_utf8_lossy(&err).into_owned(),
        }
    }

    fn run(options: &Options, operands: &[&Path]) -> Ran {
        run_with(options, operands, &[], false)
    }

    fn plain() -> Options {
        Options::default()
    }

    fn force() -> Options {
        Options {
            interactive: Interactive::Never,
            ignore_missing_files: true,
            ..Options::default()
        }
    }

    fn recursive() -> Options {
        Options {
            recursive: true,
            ..Options::default()
        }
    }

    #[test]
    fn removes_a_file() {
        let dir = scratch("file");
        let f = dir.join("a");
        fs::write(&f, b"x").unwrap();
        let r = run(&plain(), &[&f]);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "");
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_operand_without_force_is_an_error() {
        let r = run(&plain(), &[]);
        assert!(!r.ok);
        assert!(r.err.contains("missing operand"), "{}", r.err);
        assert!(r.err.contains("Try 'rm --help'"), "{}", r.err);
    }

    #[test]
    fn missing_operand_with_force_is_not() {
        let r = run(&force(), &[]);
        assert!(r.ok);
        assert_eq!(r.err, "");
    }

    /// Measured: it is `ignore_missing_files` and not "never prompt" that
    /// makes a missing operand acceptable, so `--interactive=never` alone
    /// still reports one.
    #[test]
    fn interactive_never_alone_still_wants_an_operand() {
        let options = Options {
            interactive: Interactive::Never,
            ..Options::default()
        };
        let r = run(&options, &[]);
        assert!(!r.ok);
        assert!(r.err.contains("missing operand"), "{}", r.err);
    }

    #[test]
    fn absent_file_reports_unless_forced() {
        let dir = scratch("absent");
        let f = dir.join("nope");
        let r = run(&plain(), &[&f]);
        assert!(!r.ok);
        assert!(r.err.contains("No such file or directory"), "{}", r.err);

        let r = run(&force(), &[&f]);
        assert!(r.ok, "{}", r.err);
        assert_eq!(
            r.err, "",
            "-f must be silent about a file that is not there"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_needs_recursive() {
        let dir = scratch("isdir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        let r = run(&plain(), &[&sub]);
        assert!(!r.ok);
        assert!(r.err.contains("Is a directory"), "{}", r.err);
        assert!(sub.is_dir(), "the directory must still be there");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `-f` must not report success for a file that is still present.
    #[test]
    fn force_does_not_hide_a_directory_it_cannot_remove() {
        let dir = scratch("forcedir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        let r = run(&force(), &[&sub]);
        assert!(!r.ok, "-f must not claim success for a file still present");
        assert!(r.err.contains("Is a directory"), "{}", r.err);
        assert!(sub.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dash_d_removes_an_empty_directory_only() {
        let dir = scratch("dashd");
        let empty = dir.join("empty");
        let full = dir.join("full");
        fs::create_dir(&empty).unwrap();
        fs::create_dir(&full).unwrap();
        fs::write(full.join("f"), b"x").unwrap();
        let options = Options {
            dir: true,
            ..Options::default()
        };

        let r = run(&options, &[&empty]);
        assert!(r.ok, "{}", r.err);
        assert!(!empty.exists());

        let r = run(&options, &[&full]);
        assert!(!r.ok);
        assert!(r.err.contains("Directory not empty"), "{}", r.err);
        assert!(full.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_removes_a_tree() {
        let dir = scratch("tree");
        let sub = dir.join("d");
        fs::create_dir_all(sub.join("inner")).unwrap();
        fs::write(sub.join("inner").join("f"), b"x").unwrap();
        let r = run(&recursive(), &[&sub]);
        assert!(r.ok, "{}", r.err);
        assert!(!sub.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Depth first, children before the directory, and the operand's own
    /// spelling echoed all the way down.
    #[test]
    fn verbose_names_everything_it_removed_deepest_first() {
        let dir = scratch("verbose");
        let sub = dir.join("d");
        fs::create_dir_all(sub.join("inner")).unwrap();
        fs::write(sub.join("inner").join("f"), b"x").unwrap();
        let options = Options {
            recursive: true,
            verbose: true,
            ..Options::default()
        };
        let r = run(&options, &[&sub]);
        assert!(r.ok, "{}", r.err);
        let lines: Vec<&str> = r.out.lines().collect();
        assert_eq!(lines.len(), 3, "{:?}", lines);
        assert!(lines[0].starts_with("removed '"), "{:?}", lines);
        assert!(lines[1].starts_with("removed directory '"), "{:?}", lines);
        assert!(lines[2].starts_with("removed directory '"), "{:?}", lines);
        assert!(lines[2].ends_with("d'"), "{:?}", lines);
        let _ = fs::remove_dir_all(&dir);
    }

    /// One bad operand must not stop the others, and must still be reported.
    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("continue");
        let a = dir.join("a");
        let c = dir.join("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&c, b"x").unwrap();
        let missing = dir.join("b");
        let r = run(&plain(), &[&a, &missing, &c]);
        assert!(!r.ok);
        assert!(r.err.contains("No such file or directory"), "{}", r.err);
        assert!(!a.exists(), "operands before the failure are removed");
        assert!(!c.exists(), "operands after the failure are removed");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A dangling symlink exists and is removable.
    #[test]
    #[cfg(unix)]
    fn removes_a_dangling_symlink() {
        let dir = scratch("dangling");
        let link = dir.join("l");
        std::os::unix::fs::symlink(dir.join("does-not-exist"), &link).unwrap();
        let r = run(&plain(), &[&link]);
        assert!(r.ok, "{}", r.err);
        assert!(fs::symlink_metadata(&link).is_err(), "link must be gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `rm` unlinks a symlink; it never follows one into a directory, with or
    /// without `-r`.
    #[test]
    #[cfg(unix)]
    fn a_symlink_to_a_directory_is_unlinked_not_followed() {
        let dir = scratch("symdir");
        let target = dir.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"x").unwrap();
        let link = dir.join("l");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let r = run(&recursive(), &[&link]);
        assert!(r.ok, "-r must not follow a symlink: {}", r.err);
        assert!(fs::symlink_metadata(&link).is_err(), "link must be gone");
        assert!(
            target.join("keep").exists(),
            "the target's contents must be untouched"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The end-to-end version of the reason for the first rewrite.
    #[test]
    #[cfg(unix)]
    fn removes_a_file_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let dir = scratch("nonutf8");
        let mut name = dir.as_os_str().to_owned().into_vec();
        name.extend_from_slice(b"/a\x80b");
        let path = std::path::PathBuf::from(OsString::from_vec(name));
        fs::write(&path, b"x").unwrap();
        assert!(fs::symlink_metadata(&path).is_ok(), "setup failed");

        let r = run(&plain(), &[&path]);
        assert!(r.ok, "{}", r.err);
        assert!(fs::symlink_metadata(&path).is_err(), "file must be gone");
        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------- the two refusals

    /// The defect this rewrite exists for. `rm -rf .` used to reach
    /// `fs::remove_dir_all(".")` and empty the current directory.
    #[test]
    fn dot_and_dotdot_are_refused_and_nothing_is_touched() {
        let dir = scratch("dotrefusal");
        fs::write(dir.join("keep"), b"x").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        // `sub/..` and not `..`: both end in a component of `..`, but if this
        // test ever fails it must destroy the scratch directory and not the
        // whole of the temporary directory that contains it.
        for spelling in [".", "./", "sub/..", "sub/../"] {
            let path = slash(&dir, spelling);
            let r = run(&recursive(), &[&path]);
            assert!(!r.ok, "{spelling} must be an error");
            assert!(
                r.err.contains("refusing to remove '.' or '..' directory"),
                "{spelling}: {}",
                r.err
            );
            assert!(dir.join("keep").exists(), "{spelling} deleted something");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The refusal is recursive-only: without `-r` the same operand gets the
    /// ordinary `Is a directory`, and with `-d` it gets `Directory not empty`.
    /// Both measured, and both are what stops the refusal reading as a
    /// blanket ban on the spelling.
    #[test]
    fn dot_is_not_refused_without_recursive() {
        let dir = scratch("dotplain");
        fs::write(dir.join("keep"), b"x").unwrap();
        let dot = slash(&dir, ".");

        let r = run(&plain(), &[&dot]);
        assert!(!r.ok);
        assert!(r.err.contains("Is a directory"), "{}", r.err);

        let options = Options {
            dir: true,
            ..Options::default()
        };
        let r = run(&options, &[&dot]);
        assert!(!r.ok);
        assert!(r.err.contains("Directory not empty"), "{}", r.err);
        assert!(dir.join("keep").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The root failsafe, exercised against a stand-in root so that the test
    /// can run without being catastrophic. `Rm::root` is GNU's
    /// `x.root_dev_ino`, and pointing it at a scratch directory is exactly
    /// what upstream's own test suite does with a bind mount.
    #[test]
    fn a_recursive_operand_that_is_the_root_is_refused() {
        let dir = scratch("rootfailsafe");
        fs::write(dir.join("keep"), b"x").unwrap();
        let owned = vec![dir.as_os_str().to_owned()];
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut canned = Canned::new(&[]);
        let options = recursive();
        let mut rm = Rm {
            options: &options,
            out: &mut out,
            err: &mut err,
            answers: &mut canned,
            stdin_tty: false,
            // Say that this scratch directory *is* the root.
            root: file_id(&dir),
            failed: false,
        };
        rm.run(&owned);
        assert!(rm.failed);
        let text = String::from_utf8_lossy(&err);
        assert!(
            text.contains("it is dangerous to operate recursively on"),
            "{text}"
        );
        assert!(
            text.contains("(same as '/')"),
            "a name that is not / says so: {text}"
        );
        assert!(
            text.contains("use --no-preserve-root to override this failsafe"),
            "{text}"
        );
        assert!(dir.join("keep").exists(), "nothing may be removed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_preserve_root_disarms_the_failsafe() {
        let dir = scratch("nopreserve");
        fs::write(dir.join("keep"), b"x").unwrap();
        let owned = vec![dir.as_os_str().to_owned()];
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut canned = Canned::new(&[]);
        let options = Options {
            recursive: true,
            preserve_root: false,
            ..Options::default()
        };
        let mut rm = Rm {
            options: &options,
            out: &mut out,
            err: &mut err,
            answers: &mut canned,
            stdin_tty: false,
            root: file_id(&dir),
            failed: false,
        };
        rm.run(&owned);
        assert!(!rm.failed, "{}", String::from_utf8_lossy(&err));
        assert!(!dir.exists(), "with the failsafe off it really does go");
    }

    /// The failsafe is recursive-only, as measured: `rm /` is `Is a
    /// directory`, not the dangerous-operation refusal.
    #[test]
    fn the_failsafe_does_not_fire_without_recursive() {
        let dir = scratch("rootnonrec");
        fs::write(dir.join("keep"), b"x").unwrap();
        let owned = vec![dir.as_os_str().to_owned()];
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut canned = Canned::new(&[]);
        let options = plain();
        let mut rm = Rm {
            options: &options,
            out: &mut out,
            err: &mut err,
            answers: &mut canned,
            stdin_tty: false,
            root: file_id(&dir),
            failed: false,
        };
        rm.run(&owned);
        let text = String::from_utf8_lossy(&err);
        assert!(text.contains("Is a directory"), "{text}");
        assert!(!text.contains("dangerous"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------------- prompting

    fn interactive() -> Options {
        Options {
            interactive: Interactive::Always,
            ..Options::default()
        }
    }

    #[test]
    fn a_declined_file_is_not_an_error_and_is_not_removed() {
        let dir = scratch("declinefile");
        let f = dir.join("a");
        fs::write(&f, b"x").unwrap();
        let r = run_with(&interactive(), &[&f], &["n\n"], false);
        assert!(r.ok, "declining is not a failure: {}", r.err);
        assert!(f.exists());
        assert!(r.err.contains("remove regular file"), "{}", r.err);
        assert!(
            !r.err.ends_with('\n'),
            "no newline after a prompt: {:?}",
            r.err
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_file_is_named_as_one() {
        let dir = scratch("emptyfile");
        let f = dir.join("a");
        fs::write(&f, b"").unwrap();
        let r = run_with(&interactive(), &[&f], &["n\n"], false);
        assert!(r.err.contains("remove regular empty file"), "{}", r.err);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_is_named_as_one_and_is_never_probed_for_write_protection() {
        let dir = scratch("symprompt");
        let link = dir.join("l");
        std::os::unix::fs::symlink("nowhere", &link).unwrap();
        let r = run_with(&interactive(), &[&link], &["n\n"], true);
        assert!(r.err.contains("remove symbolic link"), "{}", r.err);
        assert!(!r.err.contains("write-protected"), "{}", r.err);
        let _ = fs::remove_dir_all(&dir);
    }

    /// An empty directory gets one question, and it is `remove`, not
    /// `descend into`.
    #[test]
    fn an_empty_directory_is_asked_about_once() {
        let dir = scratch("emptydir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        let options = Options {
            recursive: true,
            interactive: Interactive::Always,
            ..Options::default()
        };
        let r = run_with(&options, &[&sub], &["y\n"], false);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err.matches("? ").count(), 1, "{}", r.err);
        assert!(r.err.contains("remove directory"), "{}", r.err);
        assert!(!r.err.contains("descend into"), "{}", r.err);
        assert!(!sub.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A non-empty one gets `descend into` first and `remove directory` after
    /// its children.
    #[test]
    fn a_full_directory_is_descended_into_then_removed() {
        let dir = scratch("fulldir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("f"), b"x").unwrap();
        let options = Options {
            recursive: true,
            interactive: Interactive::Always,
            ..Options::default()
        };
        let r = run_with(&options, &[&sub], &["y\n", "y\n", "y\n"], false);
        assert!(r.ok, "{}", r.err);
        let descend = r.err.find("descend into directory").expect(&r.err);
        let file = r.err.find("remove regular file").expect(&r.err);
        let remove = r.err.find("remove directory").expect(&r.err);
        assert!(descend < file && file < remove, "{}", r.err);
        assert!(!sub.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Declining the descend abandons every enclosing directory **in
    /// silence**, and exits 0.
    #[test]
    fn declining_a_descend_silently_abandons_the_ancestors() {
        let dir = scratch("declinedescend");
        let outer = dir.join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("f"), b"x").unwrap();
        let options = Options {
            recursive: true,
            interactive: Interactive::Always,
            ..Options::default()
        };
        // Descend into outer? yes. Descend into inner? no.
        let r = run_with(&options, &[&outer], &["y\n", "n\n"], false);
        assert!(r.ok, "a declined descend is not an error: {}", r.err);
        assert!(
            !r.err.contains("cannot remove"),
            "and says nothing more: {}",
            r.err
        );
        assert!(outer.is_dir(), "the ancestor must be left alone");
        assert!(inner.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Declining a *file*, by contrast, leaves the parent to try and fail —
    /// which is an error, with a message, and status 1. This is the pair of
    /// behaviours that a single "declined" state cannot express.
    #[test]
    fn declining_a_file_leaves_its_parent_to_fail() {
        let dir = scratch("declinechild");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("f"), b"x").unwrap();
        let options = Options {
            recursive: true,
            interactive: Interactive::Always,
            ..Options::default()
        };
        // Descend into d? yes. Remove f? no. Remove directory d? yes.
        let r = run_with(&options, &[&sub], &["y\n", "n\n", "y\n"], false);
        assert!(!r.ok, "the parent's rmdir fails, and that is an error");
        assert!(r.err.contains("Directory not empty"), "{}", r.err);
        assert!(sub.is_dir());
        assert!(sub.join("f").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// `-I`'s gate: three operands is silent, four asks.
    #[test]
    fn the_once_prompt_counts_operands() {
        let dir = scratch("onceprompt");
        let mut made: Vec<std::path::PathBuf> = Vec::new();
        for i in 0..4 {
            let f = dir.join(format!("f{i}"));
            fs::write(&f, b"x").unwrap();
            made.push(f);
        }
        let options = Options {
            interactive: Interactive::Once,
            ..Options::default()
        };

        let three: Vec<&Path> = made[..3].iter().map(std::path::PathBuf::as_path).collect();
        let r = run_with(&options, &three, &[], false);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "", "three operands ask nothing");

        for f in &made {
            fs::write(f, b"x").unwrap();
        }
        let four: Vec<&Path> = made.iter().map(std::path::PathBuf::as_path).collect();
        let r = run_with(&options, &four, &["y\n"], false);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "rm: remove 4 arguments? ", "{}", r.err);
        assert!(!made[0].exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// With `-r`, any number of operands asks — and the wording is singular
    /// for one.
    #[test]
    fn the_once_prompt_always_asks_when_recursive() {
        let dir = scratch("oncerecursive");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("f"), b"x").unwrap();
        let options = Options {
            recursive: true,
            interactive: Interactive::Once,
            ..Options::default()
        };

        let r = run_with(&options, &[&sub], &["n\n"], false);
        assert!(r.ok, "declining is not an error");
        assert_eq!(r.err, "rm: remove 1 argument recursively? ");
        assert!(sub.is_dir(), "nothing may be removed after a no");

        let r = run_with(&options, &[&sub], &["y\n"], false);
        assert!(r.ok, "{}", r.err);
        assert!(!sub.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// `--interactive=never` suppresses even the write-protected prompt, and
    /// the default suppresses it when standard input is not a terminal.
    #[test]
    fn never_asks_nothing_at_all() {
        let dir = scratch("neverask");
        let f = dir.join("a");
        fs::write(&f, b"x").unwrap();
        let options = Options {
            interactive: Interactive::Never,
            ..Options::default()
        };
        let r = run_with(&options, &[&f], &["n\n"], true);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "", "not one question, even on a terminal");
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The default state is not `Never`: on a terminal it still asks about a
    /// write-protected file. Unix only, since the probe is `euidaccess`.
    #[test]
    #[cfg(unix)]
    fn the_default_asks_about_a_write_protected_file_on_a_terminal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("writeprotected");
        let f = dir.join("a");
        fs::write(&f, b"x").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o400)).unwrap();

        // Not a terminal: no question, and it goes.
        let r = run_with(&plain(), &[&f], &["n\n"], false);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "");
        assert!(!f.exists());

        fs::write(&f, b"x").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o400)).unwrap();
        // A terminal: asked, declined, still there.
        let r = run_with(&plain(), &[&f], &["n\n"], true);
        assert!(r.ok);
        assert!(
            r.err.contains("remove write-protected regular file"),
            "{}",
            r.err
        );
        assert!(f.exists());

        // And `-f` puts it back to silence.
        let r = run_with(&force(), &[&f], &["n\n"], true);
        assert!(r.ok, "{}", r.err);
        assert_eq!(r.err, "");
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A failure below stops the ancestors being removed, and says so exactly
    /// once.
    #[test]
    #[cfg(unix)]
    fn an_unreadable_directory_is_reported_once_and_its_parent_left() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("unreadable");
        let outer = dir.join("outer");
        let closed = outer.join("closed");
        fs::create_dir_all(&closed).unwrap();
        fs::write(outer.join("f"), b"x").unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();

        let r = run(&recursive(), &[&outer]);
        assert!(!r.ok);
        assert_eq!(r.err.matches("cannot remove").count(), 1, "{}", r.err);
        assert!(r.err.contains("Permission denied"), "{}", r.err);
        assert!(outer.is_dir(), "the ancestor is left, in silence");
        assert!(!outer.join("f").exists(), "its siblings still go");

        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
