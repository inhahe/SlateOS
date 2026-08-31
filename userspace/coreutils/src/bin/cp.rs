//! cp — copy files and directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `cp` is the
//! third of the 49 bins listed there, after `rm` and `mv`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Six further bugs, in the lines this rewrite replaced
//!
//! 1. **A symlink inside a recursive copy was followed, so `cp -r` could not
//!    terminate.** `copy_dir_recursive` asked `src_path.is_dir()`, which follows
//!    symlinks, and then `fs::copy`, which also follows. A directory containing
//!    a link to any of its own ancestors — `ln -s .. loop`, which is ordinary —
//!    made `cp -r` descend for ever, writing an ever-deepening tree until the
//!    disk filled or the path length gave out. Even without a loop, every
//!    symlink in the tree silently became a full copy of whatever it pointed at,
//!    so copying a tree of a hundred links to one big file produced a hundred
//!    big files. GNU's `-r` does not dereference; neither does this one now.
//!    `DirEntry::file_type` is the non-following call and is what the walk uses.
//!
//! 2. **`cp -r` would copy a directory into itself, without limit.** `cp -r a a`
//!    and `cp -r a .` both resolve the destination to a path *inside* the
//!    source, so the walk copied what it had just written, for ever. GNU refuses
//!    with `cannot copy a directory into itself`; so does this, after resolving
//!    both paths as far as they exist.
//!
//! 3. **A copied directory came out world-readable.** `fs::create_dir_all` makes
//!    a directory with the process umask's default mode, so `cp -r private dst`
//!    — where `private` is mode 0700 — produced a 0755 `dst`, publishing every
//!    file inside it. POSIX says the new directory takes the source's
//!    permission bits. The mode is now applied *after* the contents are copied,
//!    because applying a mode like 0500 first would lock out the copy itself.
//!    (Regular files were never affected: `fs::copy` carries the mode over.)
//!
//! 4. **`--` was not an end-of-options marker.** `cp -- -foo bar` answered
//!    `unknown option: --`, so a file whose name begins with a dash could not be
//!    copied at all.
//!
//! 5. **A source ending in `..` or `/` copied into the wrong place.** The target
//!    was `dest.join(src.file_name().unwrap_or_default())`, and `Path::file_name`
//!    is `None` for such a path — so `unwrap_or_default()` gave an *empty* name
//!    and `dest.join("")` collapsed back to `dest` itself. `cp -r a/.. dst` then
//!    emptied `a`'s parent *into* `dst` rather than into `dst/<name>`, silently
//!    merging it with whatever was already there. The old test suite asserted
//!    this behaviour (`target_source_with_no_filename_into_dir`), which is how it
//!    lasted; that test now asserts the refusal.
//!
//! 6. **One unreadable file abandoned the rest of the copy.** `copy_dir_recursive`
//!    propagated the first error with `?`, so a single permission denial part-way
//!    through a large tree stopped the walk, reported one message, and left a
//!    partial copy that looked complete to anything that did not check the exit
//!    status. Each entry is now attempted, each failure reported, and the worst
//!    outcome returned — which is what `cp` is specified to do and what makes the
//!    diagnostics worth reading.
//!
//! # A seventh, found later, by measurement rather than by reading
//!
//! 7. **`cp a a` emptied `a`, silently, and exited 0.** The destination is
//!    opened with `O_TRUNC` before the source is read, so naming one file
//!    twice truncated it to nothing and then copied the nothing back over
//!    itself. It said nothing and reported success, so a shell loop that did
//!    it could destroy a directory's worth of files without a single
//!    diagnostic. Every one of these reached it — `cp a a`, `cp a ./a`,
//!    `cp a dir/../a`, `cp a hard-link-to-a`, `cp a symlink-to-a`, and
//!    `cp -r a .` — because there is no string comparison that catches the
//!    last four, and GNU does not attempt one: it compares the two `stat`
//!    results, device and inode, and so does [`is_same_file`] now.
//!
//!    This one is worth separating from bugs 1–6 for a reason that has nothing
//!    to do with `cp`. Those six were found by *reading* the code it replaced.
//!    This was found by `scripts/cp-diff.sh` on its first run, against a file
//!    that had already been rewritten once with the defect in it — the rewrite
//!    swapped a hand-rolled walk for `fs::copy` and never asked what `fs::copy`
//!    does when handed one file twice. Reading finds the bugs you thought to
//!    look for. See `known-issues.md` ->
//!    `B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT`.
//!
//! # An eighth, from the same harness: three wrong answers about permissions
//!
//! 8. **`fs::copy` gave the destination the source's mode, exactly, in every
//!    case.** That is wrong three separate ways, and two of them publish a file
//!    that was private:
//!
//!    * *A new file ignored the umask.* `fs::copy` creates the destination and
//!      then `chmod`s it to the source's mode, so a 0777 source produced a 0777
//!      copy. GNU passes the mode to `open` and lets the kernel subtract the
//!      umask, so under the ordinary 022 the copy is 0755. Measured, both ways,
//!      across three umasks — see [`mode_of_a_new_file_is_narrowed_by_umask`].
//!    * *An existing destination had its mode overwritten.* `cp public private`
//!      — a 0777 source over somebody's 0600 file — left that file 0777. GNU
//!      reopens an existing destination **without** a mode argument, so its
//!      permissions are not touched at all; only its contents are. This is the
//!      one that is a security bug rather than a cosmetic one, and no amount of
//!      reading the old code would have suggested looking for it, because the
//!      old code mentioned modes for files nowhere at all.
//!    * *A directory ignored the umask too, and had a window.* `cp -r` of a
//!      1777 directory produced 1777 where GNU produces 1755, and the copy was
//!      made group- and other-writable *before* its contents were written —
//!      a window in which anyone could add a file to a directory that is about
//!      to look like a faithful copy. [`copy_tree`] now does GNU's dance:
//!      withhold group/other write at `mkdir`, force owner-rwx on if the source
//!      lacked it, and put both back at the end, less the umask.
//!
//!    Bug 3 above was the same subject and did not go far enough: it noticed
//!    that a copied directory came out *wider* than its source and fixed that
//!    by copying the mode over verbatim, which is a different wrong answer. The
//!    lesson is bug 7's — a fix derived by reading is worth what the reading
//!    was worth, and only measurement says whether it was right.
//!
//! # A ninth: one operand quietly destroying another's work
//!
//! 9. **Nothing remembered what this command had already written.** `cp a
//!    other/a d` copied `a` to `d/a`, then copied `other/a` over it. Exit 0,
//!    nothing printed, and the copy the user asked for was gone. Neither
//!    operand is wrong on its own, which is why no amount of checking one
//!    operand at a time could have found it; GNU keeps three tables for exactly
//!    this question and this had none. [`Seen`] is that record, and it answers
//!    three refusals — the collision above, the same collision reached through
//!    a symlink this command created (where the damage lands on a file nobody
//!    named), and a source given twice, which is a warning rather than an
//!    error because the file the user asked for is in fact there.
//!
//!    The last of those is where the identity question gets sharp: `cp a ./a d`
//!    is one file named twice, but `cp a hard-link-to-a d` is two directory
//!    entries that share an inode and a perfectly reasonable request for two
//!    copies. Telling them apart needs the entry — the directory it is in plus
//!    the final component — and not the inode alone. See [`entry_id`].
//!
//! # Options this implementation does not have
//!
//! Everything except `-r`/`-R`/`--recursive`, `-t`/`--target-directory`,
//! `-T`/`--no-target-directory`, `-v`/`--verbose` and the three symlink
//! policies `-P`/`--no-dereference`, `-H` and `-L`/`--dereference`. The rest
//! are recognised and rejected with a message saying they are not implemented,
//! rather than ignored, and they are listed in [`LONG_OPTIONS`] anyway because
//! the table is what decides whether an abbreviation is ambiguous.
//!
//! Ignoring them would be worse than refusing in almost every case: `-n` asks
//! for an existing file to be left alone, `-p` asks for ownership and timestamps
//! to survive, and `-l` and `-s` ask for a link rather than a copy. Every one of
//! those, ignored, produces a destination that looks right and is not. `-d` is
//! refused for a subtler version of the same reason: it is `-P` *plus*
//! `--preserve=links`, and honouring only the half that exists would turn two
//! hard-linked sources into two independent copies without saying so.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::quoteaf_os;
use coreutils::stdfd::{self, Stream};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// The file-mode creation mask. There is no read-only spelling of it in POSIX —
// reading it means setting it — and `std` exposes no wrapper, so this is the
// libc call itself. `mkdir.rs` declares it the same way for the same reason.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// `cp`'s usage status is 1, like almost every utility's; see
/// [`coreutils::getopt::Error`] for the two that differ and why.
const CP: Program = Program::new("cp", 1);

/// GNU `cp`'s `long_opts[]`, **in its declaration order**, which is observable:
/// `getopt_long` lists an ambiguous prefix's candidates in table order. Every
/// entry is here whether or not this implementation acts on it — see the module
/// docs for why leaving one out is a silent wrong answer rather than a missing
/// feature.
///
/// Measured with `cp --=x`, which an empty prefix makes print the whole table.
/// It once also held `("keep-directory-symlink", …)`, which is a `tar` option
/// and has never been a `cp` one; `scripts/getopt-ambiguity-check.py` now
/// compares this list against that readout on every run, because the same
/// mistake had independently reached `mv` as `--exchange`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("archive", Takes::Nothing),
    ("attributes-only", Takes::Nothing),
    ("backup", Takes::Optional),
    ("copy-contents", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("link", Takes::Nothing),
    ("no-clobber", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-preserve", Takes::Required),
    ("no-target-directory", Takes::Nothing),
    ("one-file-system", Takes::Nothing),
    ("parents", Takes::Nothing),
    // Deprecated upstream but still in the table. It is the *same option* as
    // `--parents` — same `val` in GNU's `struct option` — which is why it is
    // named in [`ALIASES`] below and why `cp --pa` resolves rather than being
    // ambiguous. An earlier revision of this file asserted the opposite in a
    // comment here; measuring it (`cp --pa=1` answers `option '--parents'
    // doesn't allow an argument`) settled it the other way.
    ("path", Takes::Nothing),
    ("preserve", Takes::Optional),
    ("recursive", Takes::Nothing),
    ("remove-destination", Takes::Nothing),
    ("sparse", Takes::Required),
    ("reflink", Takes::Optional),
    ("strip-trailing-slashes", Takes::Nothing),
    ("suffix", Takes::Required),
    ("symbolic-link", Takes::Nothing),
    ("target-directory", Takes::Required),
    ("update", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("context", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The one pair of spellings in [`LONG_OPTIONS`] that name a single option.
///
/// See [`Program::resolve_long_aliased`]: without this, `--path` would count as
/// a second candidate for the prefix `--pa` and make `--parents` impossible to
/// abbreviate — which GNU allows. It does **not** make `--p` unambiguous, and a
/// test below pins that: `--p` still matches `--preserve`, which is a genuinely
/// different option.
const ALIASES: &[(&str, &str)] = &[("path", "parents")];

/// GNU `cp`'s `getopt_long` string, verbatim (`cp.c:992`).
///
/// The two colons are the part that cannot be left out. `-t` and `-S` take a
/// value, so `cp -t d a` is a target directory and one source, not three
/// operands, and `cp -S` is `option requires an argument -- 'S'` rather than a
/// copy of nothing. A table that merely listed the letters would parse both of
/// those into silently wrong operand lists.
const SHORT_OPTIONS: &str = "abdfHilLnprst:uvxPRS:TZ";

/// Whether `cp` copies a symbolic link, or copies whatever it points at.
///
/// GNU's `enum Dereference_symlink` (`copy.h`), spelled the same way and with
/// the same four members, including the one that is not a policy: `Undefined`
/// means none of `-P`, `-H`, `-L` was given, and is resolved by
/// [`CpFlags::resolved_deref`] rather than acted on.
///
/// Two policies and not one, because "follow a link" is answered differently
/// depending on *where the link was found*. That distinction is the whole of
/// `-H`, and it is invisible in any single boolean.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Deref {
    /// None of the three options was given. Never observed outside
    /// [`CpFlags::resolved_deref`].
    #[default]
    Undefined,
    /// `-P` / `--no-dereference`: copy the link itself, wherever it was found.
    Never,
    /// `-H`: follow a link named as an operand; copy links found by walking a
    /// directory.
    CommandLine,
    /// `-L` / `--dereference`: follow every link, wherever it was found.
    Always,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct CpFlags {
    recursive: bool,
    /// `-t DIR` / `--target-directory=DIR`: the destination, named by the
    /// option instead of by the last operand, so that every operand is a
    /// source. What `xargs cp -t dir` exists for.
    target_directory: Option<OsString>,
    /// `-T` / `--no-target-directory`: the destination is a name to create or
    /// replace, never a directory to copy *into*. `cp -T a d` overwrites `d`
    /// rather than writing `d/a` — or refuses to, when `d` is a directory.
    no_target_directory: bool,
    /// `-v` / `--verbose`: name every copy as it is made. See [`announce`] for
    /// where those lines come out and why they are not diagnostics.
    verbose: bool,
    /// `-P` / `-H` / `-L`: what to do with a symbolic link. Stored exactly as
    /// given, including "not given"; ask [`CpFlags::follow_operand`] and
    /// [`CpFlags::follow_walked`] rather than reading it.
    dereference: Deref,
}

impl CpFlags {
    /// [`Self::dereference`] with `Undefined` replaced by what it means.
    ///
    /// GNU does this once, after the option loop (`cp.c:1239`), and calls the
    /// default "compatible with FreeBSD": recursive copies keep links, flat
    /// copies follow them. That is why plain `cp link dst` writes a *file* and
    /// plain `cp -r link dst` writes a *link* — one option that was never
    /// given changing meaning because another one was.
    ///
    /// Resolved on demand here rather than written back into the struct, so
    /// that the parse tests can see `-r` and `-rP` as the different command
    /// lines they are, and so that there is no window in which an unresolved
    /// value could be read. GNU's `x.hard_link` also takes part in the rule
    /// (`x.recursive && ! x.hard_link`); `-l` is not implemented here, so its
    /// half of the condition is not yet expressible and is noted rather than
    /// guessed at.
    fn resolved_deref(&self) -> Deref {
        match self.dereference {
            Deref::Undefined if self.recursive => Deref::Never,
            Deref::Undefined => Deref::Always,
            given => given,
        }
    }

    /// Whether a source *named on the command line* is stat'd through.
    ///
    /// `copy.c:2250` picks `AT_SYMLINK_NOFOLLOW` exactly when the policy is
    /// `DEREF_NEVER`, so `-H` follows here and `-P` does not.
    fn follow_operand(&self) -> bool {
        self.resolved_deref() != Deref::Never
    }

    /// Whether a source *found by walking a directory* is stat'd through.
    ///
    /// GNU expresses this by handing the recursion a modified copy of the
    /// options: `copy.c:845` sets `non_command_line_options.dereference =
    /// DEREF_NEVER` when the policy is `DEREF_COMMAND_LINE_ARGUMENTS`. So only
    /// `-L` follows in here, which is what makes `cp -Hr` and `cp -Lr` differ
    /// at all — they agree about the operand and disagree about everything
    /// underneath it.
    fn follow_walked(&self) -> bool {
        self.resolved_deref() == Deref::Always
    }
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order. The last operand is the
    /// destination.
    Run(CpFlags, Vec<OsString>),
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
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
            println!("cp (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, paths)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut out = Stream::stdout();
            let mut err = Stream::stderr();
            let earned = {
                let mut job = Job {
                    flags: &flags,
                    out: &mut out,
                    err: &mut err,
                };
                if copy_all(&mut job, &paths) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            };
            // `--verbose` is the only thing `cp` ever writes to stdout, and a
            // line of it that never arrived has to change the status the same
            // way a lost diagnostic does — otherwise `cp -v … | head -1`
            // reports success for output nobody received.
            stdfd::close_stdout("cp", out, earned)
        }
        Err(e) => {
            diag!("cp: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: cp [OPTION]... SOURCE DEST
  or:  cp [OPTION]... SOURCE... DIRECTORY
Copy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.

  -H                    follow command-line symbolic links in SOURCE
  -L, --dereference     always follow symbolic links in SOURCE
  -P, --no-dereference  never follow symbolic links in SOURCE
  -r, -R, --recursive   copy directories recursively.  Symbolic links are
                          copied as symbolic links, not followed.
  -t, --target-directory=DIRECTORY
                        copy all SOURCE arguments into DIRECTORY
  -T, --no-target-directory
                        treat DEST as a normal file
  -v, --verbose         explain what is being done
      --help            display this help and exit
      --version         output version information and exit

To copy a file whose name starts with a '-', for example '-foo',
use one of these commands:
  cp -- -foo bar
  cp ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `cp`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `cp a -r b` is `cp -r a b` — which
/// is `getopt_long`'s default permuting behaviour and what [`getopt::Parser`]
/// does.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, a
/// long option given a value it does not take, or an option missing a value it
/// requires.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = CpFlags::default();
    let mut paths: Vec<OsString> = Vec::new();

    for item in CP.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item? {
            Opt::Operand(name) => paths.push(name.clone()),
            Opt::Short(b'r' | b'R', _) | Opt::Long("recursive", _) => flags.recursive = true,
            Opt::Short(b't', value) | Opt::Long("target-directory", value) => {
                // A second `-t` is refused even when it names the same
                // directory as the first — GNU compares nothing, it just asks
                // whether one was already given. Measured: `cp -t d -t d a`
                // fails. And it is a plain diagnostic, with no "Try 'cp
                // --help'" after it, because GNU raises it with `error
                // (EXIT_FAILURE, …)` rather than through `usage`.
                if flags.target_directory.is_some() {
                    return Err(CP.usage("multiple target directories specified".into()));
                }
                // Unreachable: `t:` in [`SHORT_OPTIONS`] and `Takes::Required`
                // in [`LONG_OPTIONS`] both make the parser supply a value or
                // fail before this point.
                let Some(dir) = value else {
                    return Err(CP.short_missing_argument(b't'));
                };
                flags.target_directory = Some(dir);
            }
            Opt::Short(b'T', _) | Opt::Long("no-target-directory", _) => {
                flags.no_target_directory = true;
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            // Plain assignment, so the last of several wins: `cp -LP` copies
            // the link and `cp -PL` follows it. GNU's three `case` arms do the
            // same, and there is no diagnostic for giving two of them — unlike
            // `-t`, where a repeat is an error.
            //
            // `-d` is deliberately *not* here. It is `--no-dereference` and
            // `--preserve=links` together (`cp.c:1044`), and the second half —
            // recreating a hard link between two sources that share an inode
            // rather than copying the file twice — is not implemented. Half of
            // `-d` would be the silent wrong answer the module docs are about,
            // so it stays refused until `--preserve=links` exists.
            Opt::Short(b'P', _) | Opt::Long("no-dereference", _) => {
                flags.dereference = Deref::Never;
            }
            Opt::Short(b'L', _) | Opt::Long("dereference", _) => {
                flags.dereference = Deref::Always;
            }
            // No long spelling: GNU gives `-H` none either.
            Opt::Short(b'H', _) => flags.dereference = Deref::CommandLine,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Everything else in the two tables is an option GNU has and this
            // one does not. Refused rather than ignored: see the module docs —
            // every one of them, ignored, produces a destination that looks
            // right and is not. The `Parser` has already turned a byte that is
            // in *no* table into `invalid option`, so nothing that reaches here
            // is a typo.
            Opt::Long(other, _) => return Err(unimplemented_long(other)),
            Opt::Short(other, _) => return Err(unimplemented_short(other)),
        }
    }

    Ok(Request::Run(flags, paths))
}

/// The diagnostic for an option that GNU `cp` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-p` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    CP.usage_referring(format!(
        "option -{} is not implemented by this cp",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    CP.usage_referring(format!("option '--{name}' is not implemented by this cp"))
}

// ---------------------------------------------------------------- copying ---

/// Everything a copy needs that is not the two paths: what was asked for, and
/// the two places it can say something.
///
/// One value rather than three parameters, because it is the *recursion* that
/// needs them. [`copy_tree`] and [`copy_entry`] could reach neither the flags
/// nor stdout, so no option that changes what happens inside a directory could
/// be written at all — and `--verbose`, `-p`, `-x`, `-L`/`-H` and
/// `--copy-contents` are all of them that. Two of those now exist: `--verbose`
/// reads `job.out`, and `-L` reads `job.flags` from inside [`copy_entry`].
///
/// Both sinks are parameters rather than `stdout()`/`stderr()` taken directly,
/// so that a test can assert on what a copy said. The old file had no test of
/// that path at all, which is how bugs 1–3 and 6 in the module docs survived.
struct Job<'a, O: Write, E: Write> {
    flags: &'a CpFlags,
    /// Where `--verbose` announces. Measured: GNU's `emit_verbose` uses
    /// `printf`, so the line is on stdout and is *not* a diagnostic.
    out: &'a mut O,
    err: &'a mut E,
}

/// `--verbose`'s one line about one copy: `'src' -> 'dst'`.
///
/// Three measured facts are packed into four lines of code, and each of them is
/// a way the obvious implementation would be wrong:
///
/// * **It goes to stdout, not stderr.** GNU's `emit_verbose` (`copy.c:2082`) is
///   a `printf`. So `cp -v a b > log` captures the line and `cp -v a b
///   2>/dev/null` does not silence it — the reverse of what a diagnostic does.
///   That is also why `run_main` has to route stdout through
///   [`stdfd::close_stdout`]: with `-v` this utility finally *has* stdout
///   output whose loss must change the exit status.
/// * **Both names are quoted, in the same style as a diagnostic's.** GNU writes
///   `quoteaf_n (0, src)` and `quoteaf_n (1, dst)` — two slots of one style, not
///   two styles — so `cp -v 'a b' c` prints `'a b' -> c` and the reader can tell
///   a space in a name from a space between names.
/// * **There is no flush here.** The line is buffered like any other stdout
///   write and lands in order with respect to nothing else, because `cp` writes
///   nothing else to stdout. Interleaving with stderr is not a property GNU has
///   either — piping the two together reorders them there too.
///
/// *When* it is called is the part that is not local to this function, and is
/// documented at each of the four call sites: after every refusal and before the
/// copy for a non-directory, and only on the `mkdir` actually happening for a
/// directory.
fn announce<O: Write, E: Write>(job: &mut Job<'_, O, E>, src: &Path, dst: &Path) {
    if !job.flags.verbose {
        return;
    }
    let _ = writeln!(job.out, "{} -> {}", quoteaf_os(src), quoteaf_os(dst));
}

/// Copy every source onto the destination.
///
/// Returns `true` if everything asked for was copied.
fn copy_all<O: Write, E: Write>(job: &mut Job<'_, O, E>, paths: &[OsString]) -> bool {
    let flags = job.flags;
    // GNU's `n_files <= !target_directory`. With `-t` the destination came from
    // the option, so one operand is enough; without it the last operand *is* the
    // destination and two are needed. Zero and one are distinct diagnostics —
    // "missing operand" alone left the user to work out which.
    let least = usize::from(flags.target_directory.is_none());
    if paths.len() <= least {
        let message = match paths.first() {
            None => "missing file operand".to_string(),
            Some(first) => format!(
                "missing destination file operand after {}",
                quoteaf_os(first)
            ),
        };
        let _ = writeln!(job.err, "cp: {}", CP.usage_referring(message));
        return false;
    }

    // Both `-T` checks come before `-t`'s directory is even looked at, which is
    // GNU's order and is observable: `cp -t nosuch -T a b` reports the
    // combination rather than the missing directory.
    if flags.no_target_directory {
        if flags.target_directory.is_some() {
            let _ = writeln!(
                job.err,
                "cp: cannot combine --target-directory (-t) and --no-target-directory (-T)"
            );
            return false;
        }
        // `-T` is "the destination is one name", so a third operand is not a
        // source that went to the wrong place — it is an operand with nowhere
        // to go at all.
        if let Some(extra) = paths.get(2) {
            let _ = writeln!(
                job.err,
                "cp: {}",
                CP.usage_referring(format!("extra operand {}", quoteaf_os(extra)))
            );
            return false;
        }
    }

    let (sources, dest, dest_is_dir) = match &flags.target_directory {
        // Every operand is a source. The directory is checked once, here, and
        // the failure names it as a *target directory* — a different sentence
        // from the one below, because the user named it as one.
        Some(dir) => {
            if let Some(e) = dest_directory_error(Path::new(dir)) {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: target directory {}: {why}", quoteaf_os(dir));
                return false;
            }
            (paths, dir, true)
        }
        None => {
            // Unreachable: the operand count was checked above.
            let Some((dest, sources)) = paths.split_last() else {
                return false;
            };
            if flags.no_target_directory {
                // `-T` asks for the destination to be treated as a name and
                // never as a directory to copy *into*, so it is not stat'd for
                // that question at all and `cp -T a d` goes on to report that a
                // directory cannot be overwritten with a non-directory.
                (sources, dest, false)
            } else {
                // The destination is followed, which is right here: `cp a
                // link-to-dir/` puts `a` inside the directory.
                let not_a_dir = dest_directory_error(Path::new(dest));

                // GNU reports *why* the last operand is not a directory, and
                // the two reasons read differently: `cp a b nosuch` says "No
                // such file or directory" while `cp a b afile` says "Not a
                // directory". One fixed sentence for both loses the distinction
                // that tells a user whether they mistyped the name or forgot to
                // make the directory.
                if sources.len() > 1
                    && let Some(e) = not_a_dir
                {
                    let why = strerror(&e);
                    let _ = writeln!(job.err, "cp: target {}: {why}", quoteaf_os(dest));
                    return false;
                }
                (sources, dest, not_a_dir.is_none())
            }
        }
    };
    let dest_path = Path::new(dest);

    // Both "named twice" problems need two sources to exist at all, so GNU
    // builds the tables only in that case and this follows it — not to save the
    // allocation, but because the tables also decide whether a *repeat* is
    // possible, and with one source it never is. Counted after `-t` has been
    // resolved, as GNU counts it: `cp -t d a a` is two sources and does warn.
    let mut seen = (sources.len() > 1).then(Seen::default);

    let mut ok = true;
    for src in sources {
        if !copy_one(src, dest_path, dest_is_dir, seen.as_mut(), job) {
            ok = false;
        }
    }
    ok
}

/// `None` if `dest` is a directory, otherwise the failure that says why not.
///
/// GNU asks this by *opening* the operand with `O_DIRECTORY` and keeping the
/// errno. Asking `stat` gives the same two answers — `ENOENT` for a name that
/// is not there, `ENOTDIR` for one that is something else — without needing
/// `O_PATH`, which is a Linux extension the target does not have. The case the
/// two could part company on is a directory that can be stat'd but not
/// searched; `O_PATH` opens that successfully and so does `stat`, so they
/// agree there too.
fn dest_directory_error(dest: &Path) -> Option<io::Error> {
    match fs::metadata(dest) {
        Ok(m) if m.is_dir() => None,
        Ok(_) => Some(io::Error::from(io::ErrorKind::NotADirectory)),
        Err(e) => Some(e),
    }
}

/// What this command has already copied, and where it put it.
///
/// Three of GNU's refusals need it, and all three exist to stop one operand
/// destroying the result of an earlier one in the same command — `cp a
/// other/a d` would otherwise leave `d/a` holding `other/a`, and the copy of
/// `a` the user asked for would be gone with nothing said. GNU keeps two hash
/// tables for this (`copy.c`'s `src_info` and `dest_info`, plus the
/// `remember_copied` table); the three fields below are the same information.
///
/// Only *command-line* sources go in. A file reached by recursing into a
/// directory cannot be named twice on one command line, so recording it would
/// be work spent on a question that cannot arise.
#[derive(Default)]
struct Seen {
    /// Non-directory sources already copied. Keyed on the file's identity
    /// *and* the entry that named it, which is GNU's `triple_compare`: `cp a
    /// ./a d` is one file named twice, while `cp a hard-link-to-a d` is two
    /// entries that happen to share an inode and is a legitimate request.
    sources: HashSet<(FileId, EntryId)>,
    /// Directory sources already copied, and which entry each was written to.
    /// The destination is part of the answer here and not for files, because
    /// GNU's directory rule asks a different question — see [`copy_one`].
    dirs: HashMap<FileId, EntryId>,
    /// Destinations this command created, by path *and* identity. Both halves
    /// are needed: the path is what a later operand would collide with, and
    /// the identity is what says the thing at that path is still the one we
    /// made.
    dests: HashSet<(PathBuf, FileId)>,
}

impl Seen {
    /// Records a non-directory source and reports whether it was already
    /// there. Recorded even when the copy goes on to fail, which is GNU's
    /// behaviour and the reason `cp f f d` with `d/f` a directory reports the
    /// refusal once and the repeat once rather than the refusal twice.
    fn saw_source(&mut self, id: FileId, entry: EntryId) -> bool {
        !self.sources.insert((id, entry))
    }

    /// Whether `target` is a destination this command created and which is
    /// still the same file.
    fn made(&self, target: &Path, id: FileId) -> bool {
        self.dests.contains(&(target.to_path_buf(), id))
    }

    /// Remember a destination just written. `lstat`, as GNU does: what is
    /// wanted is the identity of the *entry*, so that a symbolic link just
    /// created is recognised as itself rather than as whatever it points at.
    fn record_dest(&mut self, target: &Path) {
        if let Ok(m) = fs::symlink_metadata(target)
            && let Some(id) = file_id(target, &m)
        {
            self.dests.insert((target.to_path_buf(), id));
        }
    }
}

/// Copy one source. Returns `false` if it should count against the exit status.
fn copy_one<O: Write, E: Write>(
    src: &OsString,
    dest: &Path,
    dest_is_dir: bool,
    mut seen: Option<&mut Seen>,
    job: &mut Job<'_, O, E>,
) -> bool {
    let flags = job.flags;
    let src_path = Path::new(src);

    // Whether a symlink *operand* is followed is the whole of `-P`/`-H`/`-L`,
    // and with none of them given it depends on `-r`: plain `cp link dst`
    // copies what the link points at, while `cp -r link dst` copies the link
    // itself. [`CpFlags::follow_operand`] holds that rule and its citation.
    //
    // Not following is what keeps `cp -r` finite: a followed link to an
    // ancestor is an endless descent (module docs, bug 1). `cp -Lr` therefore
    // *is* endless on such a tree — measured, and GNU is too, which is why
    // there is no guard against it here.
    let metadata = if flags.follow_operand() {
        fs::metadata(src_path)
    } else {
        fs::symlink_metadata(src_path)
    };
    let metadata = match metadata {
        Ok(m) => m,
        Err(e) => {
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(src));
            return false;
        }
    };

    // Before anything is worked out about the destination, as in GNU: the
    // refusal is a fact about the source alone, and asking it here is what
    // makes `cp tree/.. dst` say which of its two problems came first.
    if metadata.is_dir() && !flags.recursive {
        let _ = writeln!(
            job.err,
            "cp: -r not specified; omitting directory {}",
            quoteaf_os(src)
        );
        return false;
    }

    // A non-directory named twice is asked about here, before the destination
    // is worked out at all, and GNU asks it in the same place: `cp f f d` where
    // `d/f` is a directory prints the refusal for the first `f` and this
    // warning for the second, which only happens if the source is recorded
    // even when its copy failed. The repeat is *not* an error — the user asked
    // for a file that is already there, and it is.
    //
    // Identity, not spelling: `cp a ./a d` is the same request twice. But two
    // hard links to one inode are two entries, and copying both is a
    // legitimate thing to ask for, so [`same_entry`] separates them.
    if !metadata.is_dir()
        && let Some(seen) = seen.as_deref_mut()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(entry) = entry_id(src_path)
        && seen.saw_source(id, entry)
    {
        let _ = writeln!(
            job.err,
            "cp: warning: source file {} specified more than once",
            quoteaf_os(src)
        );
        return true;
    }

    let target = match compute_target(src_path, dest, dest_is_dir) {
        Ok(t) => t,
        Err(reason) => {
            let _ = writeln!(
                job.err,
                "cp: cannot copy {} into {}: {reason}",
                quoteaf_os(src),
                quoteaf_os(dest)
            );
            return false;
        }
    };

    // GNU stats the destination here, and a failure that is *not* "it isn't
    // there" ends this operand rather than being rediscovered later while
    // opening it: `cp a b/c` where `b` is a file says `cannot stat 'b/c'`, not
    // `cannot create regular file 'b/c'`.
    //
    // Which stat is used follows GNU as well. A regular file can be written
    // *through* a symlink, so a regular source looks at what the destination
    // resolves to; a directory or a symlink cannot be, so those look at the
    // destination name itself. That distinction is what makes `cp a
    // dangling-link` reach the O_EXCL path in [`create_destination`] and be
    // refused there, rather than being taken for an existing file here.
    let use_lstat = metadata.is_dir() || metadata.file_type().is_symlink();
    let dest_stat = if use_lstat {
        fs::symlink_metadata(&target)
    } else {
        fs::metadata(&target)
    };
    let dest_meta = match dest_stat {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(&target));
            return false;
        }
    };

    if let Some(dest_meta) = &dest_meta {
        // Module docs, bug 7. `stat` results rather than strings, which is the
        // only comparison that catches every spelling; GNU's `same_file_ok`
        // makes the same one, at the same point in the same order. Asked only
        // when the destination exists, again as GNU asks it.
        if is_same_file(src_path, &target, !flags.follow_operand()) {
            let _ = writeln!(
                job.err,
                "cp: {} and {} are the same file",
                quoteaf_os(src),
                quoteaf_os(&target)
            );
            return false;
        }

        // Neither kind can be put where the other is. Without these two the
        // walk would go on and fail somewhere less informative — a directory
        // source would report `cannot create directory … File exists` about a
        // name that is not a directory at all.
        if metadata.is_dir() && !dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite non-directory {} with directory {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }
        // After the refusal above and before the one below, which is where GNU
        // asks it — inside the "destination is not a directory" arm.
        //
        // This is the one that stops `cp a other/a d` silently throwing away
        // the copy of `a` it made a moment ago. Nothing about the two operands
        // is wrong on its own; what is wrong is the pair, and only a record of
        // what this command already wrote can see it.
        if !dest_meta.is_dir()
            && let Some(seen) = seen.as_deref_mut()
            && let Some(id) = file_id(&target, dest_meta)
            && seen.made(&target, id)
        {
            let _ = writeln!(
                job.err,
                "cp: will not overwrite just-created {} with {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }

        if !metadata.is_dir() && dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "cp: cannot overwrite directory {} with non-directory",
                quoteaf_os(&target)
            );
            return false;
        }
    }

    // The same guard again, for the case the one above cannot see. A regular
    // source stats its destination *followed*, so when that destination is a
    // symlink this command just created, the identity compared above is the
    // link's target rather than the link — and writing through it would
    // clobber whatever the link points at. GNU asks this separately for the
    // same reason, with its own `lstat`.
    if let Some(seen) = seen.as_deref_mut()
        && let Ok(link_meta) = fs::symlink_metadata(&target)
        && link_meta.file_type().is_symlink()
        && let Some(id) = file_id(&target, &link_meta)
        && seen.made(&target, id)
    {
        let _ = writeln!(
            job.err,
            "cp: will not copy {} through just-created symlink {}",
            quoteaf_os(src),
            quoteaf_os(&target)
        );
        return false;
    }

    // A directory named twice is asked about here, not up with the file case:
    // GNU reaches it only after the two refusals above, so `cp -r t t d` with
    // `d/t` a plain file reports "cannot overwrite non-directory" for *both*
    // operands rather than warning about the second.
    //
    // And the question asked is a different one. Two operands naming one
    // directory are a repeat only if they were also going to the same place;
    // where they are not, GNU refuses with a message about hard links instead,
    // which this `cp` has no equivalent of yet.
    if metadata.is_dir()
        && let Some(seen) = seen.as_deref_mut()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(entry) = entry_id(&target)
    {
        match seen.dirs.get(&id) {
            Some(earlier) if *earlier == entry => {
                let _ = writeln!(
                    job.err,
                    "cp: warning: source directory {} specified more than once",
                    quoteaf_os(src)
                );
                return true;
            }
            Some(_) => {}
            None => {
                seen.dirs.insert(id, entry);
            }
        }
    }

    let ok = place_source(src, src_path, &metadata, &target, dest_meta.is_some(), job);

    // One recording site, reached however the copy was done, and only on
    // success — a destination that was never written is not one a later
    // operand can be accused of overwriting. GNU records in the same single
    // place and under the same condition.
    if ok && let Some(seen) = seen {
        seen.record_dest(&target);
    }
    ok
}

/// Make the copy, now that the destination path is settled and every refusal
/// has been made. Split out of [`copy_one`] so that it has one place to record
/// what it created rather than one per kind of source.
fn place_source<O: Write, E: Write>(
    src: &OsString,
    src_path: &Path,
    metadata: &fs::Metadata,
    target: &Path,
    dest_exists: bool,
    job: &mut Job<'_, O, E>,
) -> bool {
    if metadata.file_type().is_symlink() {
        // Reachable exactly when the stat in [`copy_one`] did not follow — `-P`,
        // or `-r` with none of `-P`/`-H`/`-L` given. Not under `-H`: that
        // follows an operand, so a link named on the command line never gets
        // here, only one found inside a tree does (in [`copy_entry`]).
        //
        // An existing destination is removed first. `symlinkat` has no
        // "replace", and refusing instead would leave `cp -r` unable to update
        // a tree it had already copied once — so GNU unlinks, under exactly
        // this condition (`copy.c`: `dereference == DEREF_NEVER` and the source
        // is not a regular file).
        if dest_exists {
            if let Err(e) = fs::remove_file(target)
                && e.kind() != io::ErrorKind::NotFound
            {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot remove {}: {why}", quoteaf_os(target));
                return false;
            }
            // `-v` names the removal too, on stdout, in its own sentence and
            // before the arrow line (`copy.c:2586`). Only here: this is the one
            // place anything is unlinked, because replacing a *regular* file is
            // done by truncating it rather than by removing it.
            //
            // Reached on "it was already gone" as well as on success, which is
            // GNU's control flow rather than an oversight — its condition is
            // `unlinkat (…) != 0 && errno != ENOENT`, so a destination that
            // vanished between the stat and the unlink is still announced as
            // removed. Only a race can produce that, and agreeing about it
            // costs nothing.
            if job.flags.verbose {
                let _ = writeln!(job.out, "removed {}", quoteaf_os(target));
            }
        }
        // *After* the removal above, which is GNU's order: its `unlink` of the
        // destination (`copy.c:2582`) comes before `emit_verbose`
        // (`copy.c:2630`), so a `cp -v` that cannot clear the way announces
        // nothing. And before the link is made, so a failure to create it is
        // still announced — `-v` reports what was attempted, not what worked.
        announce(job, src_path, target);
        return match clone_symlink(src_path, target) {
            Ok(()) => true,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(
                    job.err,
                    "cp: cannot create symbolic link {}: {why}",
                    quoteaf_os(target)
                );
                false
            }
        };
    }

    if !metadata.is_dir() {
        return copy_regular_file(src_path, metadata, target, job);
    }

    // Module docs, bug 2: without this, `cp -r a a` and `cp -r a .` copy what
    // they have just written, for ever.
    if is_inside(target, src_path) {
        let _ = writeln!(
            job.err,
            "cp: cannot copy a directory, {}, into itself, {}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return false;
    }

    copy_tree(src_path, permission_bits(metadata), target, job)
}

/// Would copying `src` to `dst` write over `src` itself?
///
/// Both are *followed*, so a destination that is a symlink to the source counts
/// — writing through it truncates the source exactly as surely as naming the
/// source directly. The one exception is GNU's, and it is the reason `nofollow`
/// is a parameter: when the source is *not* being followed, two names that are
/// both symlinks are the same file only when they are the same *link*, because
/// replacing one link with a copy of another does not touch what either points
/// at. `cp -P linkA linkB` where both point at one file is therefore allowed,
/// while `cp -P link file` — where `link` resolves to `file` — is not, and GNU
/// makes exactly that distinction in `same_file_ok` (`copy.c:1764`), keyed on
/// `x->dereference == DEREF_NEVER` and not on `-r`.
///
/// The caller passes `!flags.follow_operand()`, which under `-r` alone is the
/// `-P` case — which is why this used to take `recursive` and behaved the same.
/// It stops behaving the same the moment `-L` or `-H` is given with `-r`.
///
/// `false` when either side cannot be stat'd. A source that is a dangling
/// symlink is not the same file as anything, and a destination that cannot be
/// reached will produce its own diagnostic a moment later.
#[cfg(unix)]
fn is_same_file(src: &Path, dst: &Path, nofollow: bool) -> bool {
    use std::os::unix::fs::MetadataExt;
    fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    if nofollow {
        let (sl, dl) = (fs::symlink_metadata(src), fs::symlink_metadata(dst));
        if let (Ok(sl), Ok(dl)) = (&sl, &dl)
            && sl.file_type().is_symlink()
            && dl.file_type().is_symlink()
        {
            return same(sl, dl);
        }
    }
    match (fs::metadata(src), fs::metadata(dst)) {
        (Ok(a), Ok(b)) => same(&a, &b),
        _ => false,
    }
}

/// Windows exposes a file's identity only through `windows_by_handle`, which is
/// unstable, so the development host compares resolved paths instead. That
/// still catches a repeated operand and a `.` or `..` in the middle of one; it
/// misses a hard link, which is why the guarantee is stated on the
/// `#[cfg(unix)]` arm above — the arm the target OS and the certification
/// harness both use. The unit tests that pin the refusal run on both.
#[cfg(not(unix))]
fn is_same_file(src: &Path, dst: &Path, _nofollow: bool) -> bool {
    match (fs::canonicalize(src), fs::canonicalize(dst)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Where one source lands.
///
/// # Errors
///
/// The source having no file-name component while the destination is a
/// directory — `cp a/.. dst`. See module docs, bug 5: the previous code turned
/// this into a request to merge `a`'s parent *into* `dst`.
fn compute_target(src: &Path, dest: &Path, dest_is_dir: bool) -> Result<PathBuf, &'static str> {
    if !dest_is_dir {
        return Ok(dest.to_path_buf());
    }
    match src.file_name() {
        Some(name) => Ok(dest.join(name)),
        None => {
            Err("the source path ends in '.', '..' or '/', so it names nothing to create there")
        }
    }
}

/// Would writing at `target` write inside `root`?
///
/// Both are resolved as far as they exist — `target` usually does not exist yet,
/// so its nearest existing ancestor is canonicalised and the rest appended. That
/// makes `cp -r a .` (target `./a`) and `cp -r a a` (target `a/a`) both
/// recognisable as the same directory reached by a different spelling, which a
/// textual comparison would miss.
/// What tells one file from another, for [`Seen`]'s three questions.
///
/// The `(device, inode)` pair, which is the only answer that survives the file
/// being reached by a different name — and reaching it by a different name is
/// exactly what the three questions are about.
#[cfg(unix)]
type FileId = (u64, u64);

/// The portable stand-in: a host with no inode numbers has no cheaper answer
/// than the resolved path. It agrees with the pair above on every question
/// except hard links, which such a host does not have either.
#[cfg(not(unix))]
type FileId = PathBuf;

#[cfg(unix)]
fn file_id(_path: &Path, meta: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn file_id(path: &Path, _meta: &fs::Metadata) -> Option<FileId> {
    fs::canonicalize(path).ok()
}

/// What tells one *directory entry* from another: which directory it is in,
/// and the final component of the name.
///
/// Distinct from [`FileId`], and both are needed. Two hard links to one file
/// share a `FileId` and have different `EntryId`s, and `cp a hard-a d` is a
/// request for two copies rather than a repeat — GNU's `same_nameat` draws the
/// line in the same place. Conversely `a` and `./a` are two spellings of one
/// entry, and `cp a ./a d` is a repeat.
type EntryId = (FileId, OsString);

/// The entry `path` names, or `None` if the directory holding it cannot be
/// identified. `None` means "cannot answer", and every caller treats that as
/// "not the same entry" — the same conclusion GNU reaches when its `stat` of
/// the parent fails.
fn entry_id(path: &Path) -> Option<EntryId> {
    let (dir, name) = split_entry(path);
    let meta = fs::metadata(&dir).ok()?;
    Some((file_id(&dir, &meta)?, name))
}

/// A path's directory and final component, GNU's `dir_name`/`base_name` pair.
///
/// Trailing slashes belong to neither — `tree/` names the entry `tree` — and a
/// path with no slash at all names an entry in the current directory. Done on
/// the bytes rather than through `Path::file_name`, which answers `None` for a
/// name ending in `.` or `..` and so would make `cp -r a/. b/.. d` look like
/// one entry named twice.
#[cfg(unix)]
fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    // Everything after the last byte that is not a separator is decoration.
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(bytes.len(), |i| i + 1);
    let head = bytes.get(..end).unwrap_or(bytes);
    match head.iter().rposition(|&b| b == b'/') {
        Some(cut) => {
            // An empty directory half means the name was rooted: `/etc` is the
            // entry `etc` in `/`, not in the current directory.
            let dir = head.get(..cut).filter(|d| !d.is_empty()).unwrap_or(b"/");
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsStr::from_bytes(dir)),
                OsStr::from_bytes(name).to_os_string(),
            )
        }
        None => (PathBuf::from("."), OsStr::from_bytes(head).to_os_string()),
    }
}

/// The same split for the only non-POSIX host this builds on, Windows, where
/// it exists so that `cargo test` on the development machine exercises the
/// same code shape rather than a weaker stand-in. `OsStr` is not bytes there,
/// so the units are UTF-16 and both separators count.
#[cfg(not(unix))]
fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    // `b'/' as u16` in a pattern position is not const-evaluable, and the two
    // code units are fixed by ASCII, so they are written out.
    const SLASH: u16 = 0x2F;
    const BACKSLASH: u16 = 0x5C;
    let sep = |c: u16| c == SLASH || c == BACKSLASH;

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let end = wide
        .iter()
        .rposition(|&c| !sep(c))
        .map_or(wide.len(), |i| i + 1);
    let head = wide.get(..end).unwrap_or(&wide);
    match head.iter().rposition(|&c| sep(c)) {
        Some(cut) => {
            // An empty directory half means the name was rooted, and the
            // separator itself is then the directory.
            let dir = if cut == 0 {
                head.get(..1)
            } else {
                head.get(..cut)
            };
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsString::from_wide(dir.unwrap_or_default())),
                OsString::from_wide(name),
            )
        }
        None => (PathBuf::from("."), OsString::from_wide(head)),
    }
}

fn is_inside(target: &Path, root: &Path) -> bool {
    match (
        resolve_as_far_as_exists(root),
        resolve_as_far_as_exists(target),
    ) {
        (Some(root), Some(target)) => target.starts_with(&root),
        // If neither can be resolved at all there is nothing useful to say, and
        // refusing a copy on the strength of a failed lookup would be worse than
        // the loop this guards against is likely. The `read_dir` walk still
        // terminates on any real tree; only a self-copy loops.
        _ => false,
    }
}

/// `canonicalize`, but tolerating a path that does not exist yet: the longest
/// existing prefix is canonicalised and the remaining components appended.
fn resolve_as_far_as_exists(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = fs::canonicalize(path) {
        return Some(real);
    }
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut here = path;
    loop {
        let name = here.file_name()?;
        tail.push(name);
        here = here.parent()?;
        // An empty parent means a bare relative name: resolve against the
        // current directory, which is what the kernel would do with it.
        let base = if here.as_os_str().is_empty() {
            fs::canonicalize(Path::new(".")).ok()?
        } else if let Ok(real) = fs::canonicalize(here) {
            real
        } else {
            continue;
        };
        let mut out = base;
        for name in tail.iter().rev() {
            out.push(name);
        }
        return Some(out);
    }
}

/// Copy the tree at `src`, whose permission bits are `src_mode`, to `dest`,
/// reporting every failure to `err`.
///
/// Returns `false` if anything failed. A failure on one entry does not abandon
/// the others — module docs, bug 6.
///
/// The mode is taken as an argument rather than re-stat'd because the caller
/// has already stat'd `src` and a second look could see a different directory.
fn copy_tree<O: Write, E: Write>(
    src: &Path,
    src_mode: u32,
    dest: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let mut ok = true;

    // Group- and other-write are *withheld* at `mkdir` and put back at the end,
    // so that there is no window in which the directory exists, is writable by
    // someone else, and is not yet filled. That is GNU's `omitted_permissions`
    // (`copy.c`), and it is the reason a copy is not simply `mkdir(src_mode)`.
    let mut omitted = src_mode & 0o022;

    // The second adjustment, in the opposite direction: a source that is not
    // owner-rwx — 0500 is perfectly ordinary — would leave this process unable
    // to fill the directory it has just made. So owner-rwx goes on now and the
    // real mode goes back at the end. `dst_mode` is what to go back *to*.
    let mut dst_mode = 0;
    let mut restore = false;

    match make_dir(dest, src_mode & !omitted) {
        Ok(true) => match permission_bits_of(dest) {
            Ok(made) => {
                // A directory is announced *here* and nowhere else, and GNU
                // says why in a comment of its own (`copy.c`, above the
                // `emit_verbose` at 2991): "we don't always create the
                // destination directory, so --verbose should not announce
                // anything until we're sure we'll create a directory." So
                // `cp -rv a b` where `b/a` already exists announces the files
                // it refreshes and says nothing about the directory holding
                // them — the directory was not copied, it was reused.
                announce(job, src, dest);
                if made & 0o700 != 0o700 {
                    dst_mode = made;
                    restore = true;
                    if let Err(e) = set_mode(dest, made | 0o700) {
                        let why = strerror(&e);
                        let _ = writeln!(
                            job.err,
                            "cp: setting permissions for {}: {why}",
                            quoteaf_os(dest)
                        );
                        return false;
                    }
                }
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(dest));
                return false;
            }
        },
        Ok(false) => {
            // The destination directory was already there. GNU leaves its mode
            // alone — exactly as it leaves an existing *file*'s mode alone — so
            // there is nothing to withhold and nothing to put back.
            omitted = 0;
        }
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot create directory {}: {why}",
                quoteaf_os(dest)
            );
            return false;
        }
    }

    // An unreadable source directory is *not* a reason to leave the copy
    // early: the directory has already been created, and it must still end up
    // with the mode it is supposed to have. GNU carries the mode over in this
    // case too, and a `dst` left at the forced owner-rwx would be a copy of a
    // 0500 directory that anyone could write into.
    match read_dir_fastread(src) {
        Ok(entries) => {
            for entry in entries {
                if !copy_entry(&entry, dest, job) {
                    ok = false;
                }
            }
        }
        Err(e) => {
            // GNU's wording, and it is the only one it has for this: `copy_dir`
            // slurps the whole directory with `savedir` and reports every way
            // that can fail as `cannot access`. "cannot read directory" would
            // be the more precise sentence and is what `rm` prints, but a
            // utility that differs from GNU only in the words of a diagnostic
            // is still a utility whose output a script cannot match on.
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot access {}: {why}", quoteaf_os(src));
            ok = false;
        }
    }

    // What was withheld goes back on, less the umask — which is the subtraction
    // the kernel would have done had `mkdir` been handed the mode outright, and
    // is why a 1777 source produces a 1755 copy under the ordinary 022. Skipping
    // it would publish group-write on every copy of a 0775 directory made by a
    // process whose umask says otherwise.
    omitted &= !cached_umask();
    if omitted != 0 && !restore {
        // Deducing the mode the directory actually got is not worth attempting
        // — `mkdir` applies implementation-defined rules to the special bits —
        // so it is read back. GNU says the same in the same place.
        match permission_bits_of(dest) {
            Ok(now) => {
                dst_mode = now;
                if omitted & !now != 0 {
                    restore = true;
                }
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(dest));
                ok = false;
            }
        }
    }
    if restore && let Err(e) = set_mode(dest, dst_mode | omitted) {
        let why = strerror(&e);
        let _ = writeln!(
            job.err,
            "cp: preserving permissions for {}: {why}",
            quoteaf_os(dest)
        );
        ok = false;
    }
    ok
}

/// Every entry of `src`, read in one go and put in the order GNU walks them.
///
/// This is gnulib's `savedir (dir, SAVEDIR_SORT_FASTREAD)`, which is what
/// `copy.c`'s `copy_dir` calls, reproduced for two reasons that are the same
/// reason twice.
///
/// **The order is observable now.** Until `--verbose` there was no way to tell
/// what order a tree was walked in — the copy it leaves is the same either way
/// — and `fs::read_dir`'s raw `readdir` order was as good as any. `cp -rv` puts
/// that order on stdout, and on ext4 the two disagree: a directory holding
/// `a.txt`, `sub` and `link` created in the order `sub`, `a.txt`, `link` is
/// named by GNU in creation order and by an unsorted `readdir` in hash order.
/// Neither is more correct, but only one of them is GNU's, and this program's
/// job is to be indistinguishable from GNU.
///
/// **And the order GNU picked is the fast one**, which is why gnulib calls it
/// `FASTREAD` rather than `SORTED`. Inode number is roughly on-disk position on
/// every filesystem that allocates inodes in tables, so walking a directory in
/// inode order turns the scattered reads of a `stat` per entry into a forward
/// scan. That is a real win on a cold cache and costs one sort of a list that
/// had to be materialised anyway.
///
/// The eager read is gnulib's too, and it changes one thing besides order: a
/// `readdir` that fails part-way through now abandons the whole directory
/// rather than copying the entries it had already seen. `savedir` returns
/// `NULL` in exactly that case, and `copy_dir` reports it as the one
/// `cannot access` diagnostic — so this is not a new behaviour so much as the
/// one GNU always had.
///
/// # Errors
///
/// Opening the directory, or any `readdir` within it.
fn read_dir_fastread(src: &Path) -> io::Result<Vec<fs::DirEntry>> {
    // `mut` is written only by the `#[cfg(unix)]` arm below. Off Unix there is
    // no inode to sort by, so the binding is never assigned to and the compiler
    // would rightly say so.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(src)?.collect::<io::Result<Vec<_>>>()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirEntryExt as _;
        // `d_ino` straight out of the `dirent`, not a `stat` — the sort must
        // not cost what it is there to save. Unstable sort because gnulib's
        // `qsort_r` is unstable too, and the only way to get a tie is two hard
        // links to one inode in one directory, where the two orders differ in
        // which of two names is copied first and in nothing else.
        entries.sort_unstable_by_key(fs::DirEntry::ino);
    }
    // Off Unix there is nothing to sort by, which is also gnulib's answer:
    // `SAVEDIR_SORT_FASTREAD` degrades to `SAVEDIR_SORT_NONE` where
    // `D_INO_IN_DIRENT` is not defined. See the `#[cfg(unix)]` arm above.
    Ok(entries)
}

/// One entry of a directory being walked. Split out of [`copy_tree`] only to
/// keep the mode bookkeeping either side of the walk readable in one screen.
///
/// The containing directory is no longer a parameter: the `readdir` that could
/// fail now happens in [`read_dir_fastread`], so the only caller that ever had
/// to name the *source directory* in a diagnostic is the one that reads it.
fn copy_entry<O: Write, E: Write>(
    entry: &fs::DirEntry,
    dest: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    let from = entry.path();
    let to = dest.join(entry.file_name());

    // `DirEntry::metadata` does **not** follow symlinks, unlike `Path::is_dir`.
    // That is the whole of the fix for bug 1, and it also hands over the mode
    // the copy is to be created with, which a second `stat` might not.
    //
    // `-L` is the one policy that wants the other answer *here*, and asking for
    // it costs the extra `stat` that `entry.metadata()` was avoiding — there is
    // no following variant of it. That is the right way round: the option that
    // is not given pays nothing. See [`CpFlags::follow_walked`] for why `-H`
    // takes this branch and not the other one.
    let meta = if job.flags.follow_walked() {
        fs::metadata(&from)
    } else {
        entry.metadata()
    };
    let meta = match meta {
        Ok(m) => m,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: cannot stat {}: {why}", quoteaf_os(&from));
            return false;
        }
    };

    if meta.file_type().is_symlink() {
        // The recursive twin of the announcement in [`place_source`]: GNU
        // reaches this through the same `copy_internal`, so a link found inside
        // a tree is named exactly as a link named on the command line is.
        announce(job, &from, &to);
        return match clone_symlink(&from, &to) {
            Ok(()) => true,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(
                    job.err,
                    "cp: cannot create symbolic link {}: {why}",
                    quoteaf_os(&to)
                );
                false
            }
        };
    }
    if meta.is_dir() {
        return copy_tree(&from, permission_bits(&meta), &to, job);
    }
    copy_regular_file(&from, &meta, &to, job)
}

/// Create `dest` as a directory with mode `mode`, before the umask is applied.
///
/// `Ok(true)` if it was created, `Ok(false)` if a directory was already there —
/// a distinction the caller needs, because an existing directory's mode is left
/// alone. Plain `create_dir` and not `create_dir_all`: GNU's single `mkdirat`
/// does not invent missing parents either, and `cp -r a no/such/dir` must fail
/// rather than quietly build the path.
fn make_dir(dest: &Path, mode: u32) -> io::Result<bool> {
    match create_dir_with_mode(dest, mode) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // Only "already there" if it is a *directory*. A regular file under
            // that name is a failure, and reporting it as one is what stops the
            // walk from writing a directory's contents into whatever it found.
            if fs::metadata(dest).is_ok_and(|m| m.is_dir()) {
                Ok(false)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// Copy the regular file `src` to `dst`.
///
/// This does by hand what `fs::copy` does in one call, for two reasons that are
/// not about speed:
///
/// * **`fs::copy` reports four different failures as one error.** The source
///   not opening, the destination not being creatable, a read fault and a write
///   fault all arrive as a single `io::Error` with nothing to say which
///   happened. GNU has a different sentence for each, and which sentence is
///   printed is the difference between knowing that a disk is full and knowing
///   that a file is unreadable.
/// * **`fs::copy` ends by giving the destination the source's exact mode.**
///   That is wrong twice: it ignores the umask on a file it has just created,
///   so a 0777 source lands as 0777 where GNU lands it as 0755; and it
///   overwrites the mode of a destination that *already existed*, so copying a
///   0777 file over somebody's 0600 one published it. See module docs, bug 8.
fn copy_regular_file<O: Write, E: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    dst: &Path,
    job: &mut Job<'_, O, E>,
) -> bool {
    // Before the source is even opened, which is GNU's order: `emit_verbose`
    // (`copy.c:2630`) runs before `copy_reg`, so an unreadable source is
    // announced and *then* complained about. One site here rather than one in
    // each caller, because both of them — a file named on the command line and
    // a file found inside a tree — go through GNU's one `copy_internal` too.
    announce(job, src, dst);

    let mut input = match fs::File::open(src) {
        Ok(f) => f,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot open {} for reading: {why}",
                quoteaf_os(src)
            );
            return false;
        }
    };

    let mut output = match create_destination(src_meta, dst) {
        Ok(f) => f,
        Err(DestError::Dangling) => {
            let _ = writeln!(
                job.err,
                "cp: not writing through dangling symlink {}",
                quoteaf_os(dst)
            );
            return false;
        }
        Err(DestError::Io(e)) => {
            let why = strerror(&e);
            let _ = writeln!(
                job.err,
                "cp: cannot create regular file {}: {why}",
                quoteaf_os(dst)
            );
            return false;
        }
    };

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // A signal arriving mid-read is not a read failure, and reporting
            // it as one would make `cp` unreliable under any job control.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(job.err, "cp: error reading {}: {why}", quoteaf_os(src));
                return false;
            }
        };
        let Some(chunk) = buf.get(..n) else {
            // Unreachable: `read` returns at most the buffer's length. Handled
            // rather than indexed so the crate's `indexing_slicing` lint has
            // nothing to complain about and a broken `Read` cannot panic here.
            break;
        };
        if let Err(e) = output.write_all(chunk) {
            let why = strerror(&e);
            let _ = writeln!(job.err, "cp: error writing {}: {why}", quoteaf_os(dst));
            return false;
        }
    }
    true
}

/// Why a destination could not be opened for writing.
enum DestError {
    Io(io::Error),
    /// The name is a symlink that points at nothing. Resolving it to a
    /// (directory, name) pair to write through is racy by construction, so GNU
    /// refuses and says so rather than creating the link's target.
    Dangling,
}

/// Open `dst` for writing, creating it with the source's mode if it is new and
/// leaving its mode entirely alone if it is not.
///
/// The order is GNU's: try `O_CREAT|O_EXCL` with the mode, and treat `EEXIST`
/// as "it was already there, reopen it without a mode". That is what keeps the
/// umask in the kernel's hands for a new file — the only place it can be
/// applied without a window in which the file exists at the wider mode — and
/// what leaves an existing file's permissions untouched.
fn create_destination(src_meta: &fs::Metadata, dst: &Path) -> Result<fs::File, DestError> {
    match open_new(dst, permission_bits(src_meta)) {
        Ok(f) => return Ok(f),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(DestError::Io(e)),
    }

    // `symlink_metadata` sees the link itself; `metadata` follows it, so
    // failing there is exactly "points at nothing".
    if fs::symlink_metadata(dst).is_ok_and(|m| m.file_type().is_symlink())
        && fs::metadata(dst).is_err()
    {
        return Err(DestError::Dangling);
    }

    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(dst)
        .map_err(DestError::Io)
}

/// `O_WRONLY|O_CREAT|O_EXCL` with `mode`, which the kernel narrows by the umask.
#[cfg(unix)]
fn open_new(dst: &Path, mode: u32) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(dst)
}

/// The development host has no mode to give, so the file is created with
/// whatever Windows would have given it. The target OS is the `#[cfg(unix)]`
/// arm above; see [`permission_bits`].
#[cfg(not(unix))]
fn open_new(dst: &Path, _mode: u32) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
}

/// The permission and special bits — `07777` — of `meta`.
#[cfg(unix)]
fn permission_bits(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o7777
}

/// Windows has no mode bits. `0o777` rather than `0` so that the arithmetic in
/// [`copy_tree`] — withhold group/other write, put it back if the directory did
/// not get it — cancels out to no change at all, which is the right answer on a
/// host where every `chmod` is a no-op anyway.
#[cfg(not(unix))]
fn permission_bits(_meta: &fs::Metadata) -> u32 {
    0o777
}

/// `permission_bits` of the name `path`, without following a final symlink.
fn permission_bits_of(path: &Path) -> io::Result<u32> {
    fs::symlink_metadata(path).map(|m| permission_bits(&m))
}

/// `mkdir(path, mode)`; the kernel narrows `mode` by the umask.
#[cfg(unix)]
fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(mode).create(path)
}

/// See [`open_new`]'s non-unix arm.
#[cfg(not(unix))]
fn create_dir_with_mode(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}

/// `chmod(path, mode)`.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// `set_permissions` on Windows only toggles the read-only flag, which is not
/// what POSIX is asking for; doing nothing is the honest answer. The target OS
/// is the `#[cfg(unix)]` arm above.
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// The process's file-mode creation mask.
///
/// There is no read-only spelling of it in POSIX — reading it means setting it
/// — so this sets it to deny everything and immediately puts the old value
/// back. That is GNU's `cached_umask` (`copy.c`) verbatim, and it is safe for
/// the reason it is safe there: `cp` is single-threaded and creates nothing
/// between the two calls, so nothing can observe the wider-denying mask.
#[cfg(unix)]
fn read_umask() -> u32 {
    // SAFETY: `umask` cannot fail, takes and returns a plain integer, and
    // touches no memory. Setting it back to the value just read leaves the
    // process's mask exactly as it was found.
    unsafe {
        let old = umask(0o777);
        umask(old);
        old
    }
}

/// [`read_umask`], remembered, as GNU remembers it: a deep `cp -r` should open
/// that momentary all-denying window once, not once per directory.
///
/// A real `cp` is one process with one umask for its whole life, so caching
/// changes no answer. The **test build does not cache** — `cargo test` runs
/// dozens of copies inside one process, and the mode tests set the umask around
/// each one, so a value remembered from the first would make every later row
/// assert against the wrong mask. That is the cache being wrong about the test
/// harness, not the tests being wrong about `cp`.
#[cfg(all(unix, not(test)))]
fn cached_umask() -> u32 {
    use std::sync::OnceLock;
    static CACHE: OnceLock<u32> = OnceLock::new();
    *CACHE.get_or_init(read_umask)
}

/// See the caching note above.
#[cfg(all(unix, test))]
fn cached_umask() -> u32 {
    read_umask()
}

/// Windows has no umask. Zero makes [`copy_tree`]'s subtraction a no-op.
#[cfg(not(unix))]
fn cached_umask() -> u32 {
    0
}

/// Reproduce the symlink at `src` as a symlink at `at`.
///
/// The link's *text* is copied verbatim, so a relative link keeps meaning
/// whatever it means relative to its new directory — which is what makes copying
/// a self-consistent tree of relative links produce another self-consistent
/// tree.
#[cfg(unix)]
fn clone_symlink(src: &Path, at: &Path) -> io::Result<()> {
    let points_at = fs::read_link(src)?;
    std::os::unix::fs::symlink(points_at, at)
}

/// Recreating a symlink needs a distinction between file and directory links on
/// Windows, and a privilege the test host does not necessarily have. Refusing is
/// the only answer that does not silently produce something other than a
/// symlink — and silently producing something else is precisely bug 1.
#[cfg(not(unix))]
fn clone_symlink(_src: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "recreating a symlink is not supported on this host",
    ))
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

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (CpFlags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, p) => (
                f,
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
        let (f, p) = run_parse(&[]);
        assert!(!f.recursive);
        assert!(p.is_empty());
    }

    #[test]
    fn simple_copy() {
        let (f, p) = run_parse(&["a", "b"]);
        assert!(!f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn dash_r_sets_recursive() {
        let (f, p) = run_parse(&["-r", "src", "dst"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["src", "dst"]);
    }

    #[test]
    fn capital_r_also_recursive() {
        assert!(run_parse(&["-R", "src", "dst"]).0.recursive);
        assert!(run_parse(&["-rR", "a", "b"]).0.recursive);
        assert!(run_parse(&["--recursive", "a", "b"]).0.recursive);
    }

    #[test]
    fn flag_may_follow_operands() {
        let (f, p) = run_parse(&["a", "b", "-r"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn multiple_sources() {
        assert_eq!(run_parse(&["a", "b", "c", "d"]).1, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]).1, vec!["-", "dest"]);
    }

    /// Bug 4 in the module docs: this used to answer `unknown option: --`.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, p) = run_parse(&["--", "-r"]);
        assert!(!f.recursive, "-r after -- is a filename, not a flag");
        assert_eq!(p, vec!["-r"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).1.is_empty());
    }

    #[test]
    fn long_options_abbreviate() {
        assert!(run_parse(&["--recur", "a", "b"]).0.recursive);
    }

    /// `--r` must stay ambiguous — `--recursive`, `--reflink` and
    /// `--remove-destination` all begin with it. This is the test that fails if
    /// someone prunes the table to what is actually handled.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--r"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--recursive"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--reflink"), "{:?}", e.sentence);
    }

    /// `--pa` is **not** ambiguous, because `--path` and `--parents` are one
    /// option under two spellings. An earlier revision of this test asserted the
    /// opposite from recall; the measurement says otherwise:
    ///
    /// ```text
    /// $ cp --pa=1 a b
    /// cp: option '--parents' doesn't allow an argument
    /// ```
    ///
    /// which is `getopt_long` having resolved it, then complaining about the
    /// value. So the alias resolves, and the name it resolves to is the first
    /// *table* entry that matched — `--parents`, which precedes `--path` in
    /// `cp`'s table (it is the other way round in `rmdir`'s).
    ///
    /// Only `--pa` actually reaches both spellings; `--pat` is already past
    /// `--parents` and `--paren` already past `--path`. Each is listed with the
    /// name GNU answers with, measured the same way, because the interesting
    /// claim is not "it resolves" but *which* of the two it names:
    ///
    /// ```text
    /// --pa     cp: option '--parents' doesn't allow an argument
    /// --pat    cp: option '--path'    doesn't allow an argument
    /// --paren  cp: option '--parents' doesn't allow an argument
    /// ```
    #[test]
    fn the_deprecated_alias_does_not_make_its_own_option_ambiguous() {
        for (typed, named) in [
            ("--pa", "--parents"),
            ("--pat", "--path"),
            ("--paren", "--parents"),
        ] {
            let e = fail(&[typed, "a", "b"]);
            assert!(
                !e.sentence.contains("ambiguous"),
                "{typed}: {:?}",
                e.sentence
            );
            // It resolves, and is then refused for the separate reason that
            // this `cp` implements neither spelling.
            assert!(
                e.sentence
                    .contains(&format!("'{named}' is not implemented")),
                "{typed}: {:?}",
                e.sentence
            );
        }
    }

    /// The other half of the rule, and the half that a naive "hide the aliases"
    /// implementation gets wrong: `--p` matches `--parents`, `--path` **and**
    /// `--preserve`, and is ambiguous — but the message lists two, not three.
    /// Measured:
    ///
    /// ```text
    /// cp: option '--p' is ambiguous; possibilities: '--parents' '--preserve'
    /// ```
    #[test]
    fn a_prefix_that_reaches_past_the_alias_is_still_ambiguous() {
        assert_eq!(
            fail(&["--p", "a", "b"]).sentence,
            "option '--p' is ambiguous; possibilities: '--parents' '--preserve'"
        );
    }

    /// An exact alias resolves to itself, not to what it aliases — `getopt_long`
    /// returns the entry it matched.
    #[test]
    fn the_exact_alias_spelling_is_reported_as_typed() {
        assert!(
            fail(&["--path", "a", "b"])
                .sentence
                .contains("'--path' is not implemented"),
            "{:?}",
            fail(&["--path", "a", "b"]).sentence
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "a", "b"]);
        assert!(e.sentence.contains("invalid option"), "{:?}", e.sentence);
        assert!(e.sentence.contains('q'), "{:?}", e.sentence);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "a", "b"]);
        assert!(
            e.sentence.contains("unrecognized option"),
            "{:?}",
            e.sentence
        );
        assert!(e.sentence.contains("--zzz=1"), "{:?}", e.sentence);
    }

    /// Ignoring any of these would produce a destination that looks right and is
    /// not — `-p` silently drops permissions, `-n` silently overwrites, `-l` and
    /// `-s` silently copy instead of linking.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in [
            // `-S` is here with a value attached: it takes a required one, so
            // bare `-S` would swallow the operand after it and this would be an
            // arity test rather than a rejection test.
            // `-d` is here and `-P` is not, though the two set the same
            // dereference policy: `-d` is also `--preserve=links`, which does
            // not exist yet. See the parse arm.
            "-a", "-b", "-d", "-f", "-i", "-l", "-n", "-p", "-s", "-S.bak", "-u", "-x", "-Z",
        ] {
            let e = fail(&[flag, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{flag}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in [
            "--archive",
            "--attributes-only",
            "--backup",
            "--copy-contents",
            "--force",
            "--interactive",
            "--link",
            "--no-clobber",
            "--one-file-system",
            "--parents",
            "--preserve",
            "--remove-destination",
            "--strip-trailing-slashes",
            "--symbolic-link",
            "--update",
            // Given values inline so the option cannot swallow an operand and
            // turn a rejection test into an arity test.
            "--sparse=always",
            "--reflink=always",
        ] {
            let e = fail(&[name, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--recursive=yes", "a", "b"]);
        assert!(e.sentence.contains("doesn't allow"), "{:?}", e.sentence);
    }

    // ------------------------------------------- where the destination is --

    /// All three spellings of `-t`, and the fact that its value never lands in
    /// the operand list. `-td` is the one that could only work through a table
    /// that says the letter takes a value.
    #[test]
    fn a_target_directory_is_taken_out_of_the_operands() {
        for spelling in [
            &["-t", "d", "a", "b"][..],
            &["-td", "a", "b"][..],
            &["--target-directory=d", "a", "b"][..],
            &["a", "b", "-t", "d"][..],
        ] {
            let (f, p) = run_parse(spelling);
            assert_eq!(f.target_directory, Some(OsString::from("d")));
            assert_eq!(p, ["a", "b"], "{spelling:?}");
        }
    }

    /// GNU compares nothing here — it asks only whether one was already given —
    /// so naming the same directory twice fails just as two different ones do.
    #[test]
    fn a_second_target_directory_is_refused() {
        for spelling in [
            &["-t", "d", "-t", "d", "a"][..],
            &["-t", "d", "-t", "e", "a"][..],
        ] {
            let e = fail(spelling);
            assert_eq!(e.sentence, "multiple target directories specified");
            // `error (EXIT_FAILURE, …)` upstream, not `usage`, so there is no
            // "Try 'cp --help'" after it.
            assert_eq!(e.referral, None, "{spelling:?}");
        }
    }

    #[test]
    fn a_missing_target_directory_value_is_a_getopt_error() {
        let e = fail(&["-t"]);
        assert!(
            e.sentence.contains("option requires an argument"),
            "{:?}",
            e.sentence
        );
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for the reason this file was rewritten. On this OS a
    /// filename may hold any byte but `/` and NUL, and byte `0x80` alone is not
    /// valid UTF-8, so an operand containing it cannot be a `String` at all.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad, OsString::from("d")]);
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
    /// Windows — the regression tests for the bug this file was rewritten to fix
    /// **do not run at all**. That is the same blind spot that let the bug
    /// survive, so it is closed rather than noted. Windows has its own argument
    /// that no `String` can hold: an unpaired surrogate (a UTF-16 code unit in
    /// `0xD800..=0xDFFF` with no partner), which reaches the same `unwrap` in
    /// `env::args()` by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad, OsString::from("d")]);
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

    // ----------------------------------------------------- compute_target --

    #[test]
    fn target_file_to_file() {
        let t = compute_target(Path::new("a.txt"), Path::new("b.txt"), false).unwrap();
        assert_eq!(t, PathBuf::from("b.txt"));
    }

    #[test]
    fn target_file_into_dir() {
        let t = compute_target(Path::new("src/a.txt"), Path::new("dst"), true).unwrap();
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
    }

    #[test]
    fn target_dir_into_dir_appends_basename() {
        let t = compute_target(Path::new("src/sub"), Path::new("dst"), true).unwrap();
        assert_eq!(t, PathBuf::from("dst").join("sub"));
    }

    /// Bug 5 in the module docs. The old test here asserted the *broken*
    /// behaviour — that `dst.join("")`, i.e. `dst` itself, was the right answer —
    /// which is why the bug lasted. Merging `a`'s parent into `dst` is not what
    /// `cp -r a/.. dst` asks for.
    #[test]
    fn a_source_with_no_file_name_is_refused_not_collapsed() {
        for src in ["/", "..", "a/..", "."] {
            let e = compute_target(Path::new(src), Path::new("dst"), true).unwrap_err();
            assert!(e.contains("names nothing"), "{src}: {e}");
        }
    }

    #[test]
    fn a_source_with_no_file_name_is_fine_when_dest_is_not_a_dir() {
        let t = compute_target(Path::new("a/.."), Path::new("dst"), false).unwrap();
        assert_eq!(t, PathBuf::from("dst"));
    }

    // ------------------------------------------------------------ copying --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("cp_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PLAIN: CpFlags = CpFlags {
        recursive: false,
        target_directory: None,
        no_target_directory: false,
        verbose: false,
        dereference: Deref::Undefined,
    };
    const RECURSIVE: CpFlags = CpFlags {
        recursive: true,
        target_directory: None,
        no_target_directory: false,
        verbose: false,
        dereference: Deref::Undefined,
    };
    /// `-T`, which the two above never set. Named for what it does rather than
    /// for the letter: the destination is a name, not a directory to fill.
    const AS_NAME: CpFlags = CpFlags {
        recursive: false,
        target_directory: None,
        no_target_directory: true,
        verbose: false,
        dereference: Deref::Undefined,
    };
    const VERBOSE: CpFlags = CpFlags {
        recursive: false,
        target_directory: None,
        no_target_directory: false,
        verbose: true,
        dereference: Deref::Undefined,
    };
    const VERBOSE_RECURSIVE: CpFlags = CpFlags {
        recursive: true,
        target_directory: None,
        no_target_directory: false,
        verbose: true,
        dereference: Deref::Undefined,
    };
    // The three below are `#[cfg(unix)]` because every test that uses one has
    // to create a symlink to mean anything, and the development host cannot.
    // Without the gate they are dead code there and the build is not warning-
    // free. [`the_dereference_table`] needs no filesystem and so runs on both.
    /// `-P`: the link, not its target, with no `-r` to make that the default.
    #[cfg(unix)]
    const NO_DEREF: CpFlags = CpFlags {
        recursive: false,
        target_directory: None,
        no_target_directory: false,
        verbose: false,
        dereference: Deref::Never,
    };
    /// `-Lr`: follow every link, including ones found inside the tree.
    #[cfg(unix)]
    const DEREF_ALL_R: CpFlags = CpFlags {
        recursive: true,
        target_directory: None,
        no_target_directory: false,
        verbose: false,
        dereference: Deref::Always,
    };
    /// `-Hr`: follow the operand, keep the links found underneath it. The one
    /// combination in which the two questions have different answers.
    #[cfg(unix)]
    const DEREF_CMD_R: CpFlags = CpFlags {
        recursive: true,
        target_directory: None,
        no_target_directory: false,
        verbose: false,
        dereference: Deref::CommandLine,
    };

    /// `copy_all` plus whatever it wrote to its error sink.
    ///
    /// The stdout half is dropped here rather than returned, because all but a
    /// handful of these tests do not set `-v` and so could only ever assert
    /// that it was empty. [`cp_out`] is the same call for the ones that care.
    fn cp(flags: &CpFlags, paths: &[&Path]) -> (bool, String) {
        let (ok, _out, err) = cp_out(flags, paths);
        (ok, err)
    }

    /// `copy_all` plus *both* of the things it wrote: `(ok, stdout, stderr)`.
    ///
    /// The two sinks are separate `Vec`s and not one, which is the point: a
    /// test that asserted on their concatenation could not tell a `--verbose`
    /// line on stdout from the same text misdirected to stderr, and getting
    /// that wrong is exactly the bug worth catching — GNU's is a `printf`.
    fn cp_out(flags: &CpFlags, paths: &[&Path]) -> (bool, String, String) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let ok = {
            let mut job = Job {
                flags,
                out: &mut out,
                err: &mut err,
            };
            copy_all(&mut job, &owned)
        };
        (
            ok,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn copies_a_file() {
        let dir = scratch("file");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&a).unwrap(), b"hello", "the source stays");
        assert_eq!(fs::read(&b).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.join("a");
        let sub = dir.join("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    // --------------------------------------------- which failure it reports --

    /// GNU names the errno rather than restating the option's requirement, and
    /// the two errnos read differently enough to matter: one says the name is
    /// missing, the other that it is the wrong kind of thing.
    #[test]
    fn the_target_diagnostic_names_the_reason() {
        let dir = scratch("target_why");
        let a = dir.join("a");
        let b = dir.join("b");
        let file = dir.join("plain");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        fs::write(&file, b"3").unwrap();

        let missing = dir.join("nosuch");
        let (ok, e) = cp(&PLAIN, &[&a, &b, &missing]);
        assert!(!ok);
        assert!(
            e.ends_with(": No such file or directory\n"),
            "a name that is not there: {e}"
        );

        let (ok, e) = cp(&PLAIN, &[&a, &b, &file]);
        assert!(!ok);
        assert!(
            e.ends_with(": Not a directory\n"),
            "a name that is something else: {e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A destination whose *parent* is not a directory is a failed `stat`, not
    /// a failed create, and GNU says which. Reporting it at the create would
    /// name the wrong operation and, once `cp` grows `-i`, would do so after
    /// having already asked to overwrite something.
    #[cfg(unix)]
    #[test]
    fn a_destination_under_a_plain_file_fails_at_the_stat() {
        let dir = scratch("dst_stat");
        let a = dir.join("a");
        let blocking = dir.join("blocking");
        fs::write(&a, b"1").unwrap();
        fs::write(&blocking, b"2").unwrap();

        let under = blocking.join("under");
        let (ok, e) = cp(&PLAIN, &[&a, &under]);
        assert!(!ok);
        assert!(e.starts_with("cp: cannot stat "), "{e}");
        assert!(e.ends_with(": Not a directory\n"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Under `-r` a symlink operand is copied as a link, so its own inode is
    /// what a naive comparison sees — and that inode is never the destination's.
    /// GNU resolves both sides unless *both* are links, which is what makes
    /// `cp -r link file` a refusal where `link` points at `file`.
    #[cfg(unix)]
    #[test]
    fn a_symlink_operand_resolving_to_the_destination_is_refused() {
        let dir = scratch("link_same");
        let file = dir.join("file");
        let link = dir.join("link");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &link).unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&link, &file]);
        assert!(!ok);
        assert!(e.contains("are the same file"), "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The other half of that rule: two *distinct* links to one file are not
    /// the same file, because replacing one with a copy of the other leaves
    /// what they point at alone.
    #[cfg(unix)]
    #[test]
    fn two_symlinks_to_one_file_are_not_the_same_file() {
        let dir = scratch("two_links");
        let file = dir.join("file");
        let one = dir.join("one");
        let two = dir.join("two");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&one, &two]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept", "the target is untouched");
        let _ = fs::remove_dir_all(&dir);
    }

    // ------------------------------------------- one operand against another --
    //
    // Three refusals that no single operand can be judged by: what is wrong is
    // the *pair*, and only a record of what the command already wrote can see
    // it. The data-loss one is `cp a other/a d` — without the guard, `d/a`
    // ends up holding `other/a` and the copy of `a` the user asked for is gone
    // with nothing printed.

    #[test]
    fn a_second_source_will_not_overwrite_the_copy_the_first_just_made() {
        let dir = scratch("just_created");
        let other = dir.join("other");
        let dest = dir.join("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let first = dir.join("f");
        let second = other.join("f");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let (ok, e) = cp(&PLAIN, &[&first, &second, &dest]);
        assert!(!ok, "the pair must count against the exit status");
        // Not asserted against a whole quoted path: the scratch directory is
        // absolute and its spelling differs by host.
        assert!(e.contains("will not overwrite just-created"), "{e}");
        assert_eq!(
            fs::read(dest.join("f")).unwrap(),
            b"first",
            "the copy that was asked for first survives"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same guard, for the case the first cannot see. A regular source
    /// stats its destination *followed*, so a destination that is a symlink
    /// this command just made compares as whatever it points at; without a
    /// second look at the link itself the copy goes through it.
    #[cfg(unix)]
    #[test]
    fn a_second_source_will_not_be_written_through_a_just_created_symlink() {
        let dir = scratch("through_link");
        let other = dir.join("other");
        let dest = dir.join("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let pointee = dir.join("pointee");
        fs::write(&pointee, b"untouched").unwrap();
        let link = dir.join("l");
        std::os::unix::fs::symlink(&pointee, &link).unwrap();
        let plain = other.join("l");
        fs::write(&plain, b"second").unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&link, &plain, &dest]);
        assert!(!ok, "{e}");
        assert!(e.contains("through just-created symlink"), "{e}");
        assert_eq!(
            fs::read(&pointee).unwrap(),
            b"untouched",
            "what the link points at is not written to"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_source_named_twice_is_a_warning_and_not_a_failure() {
        let dir = scratch("named_twice");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"body").unwrap();
        let dotted = dir.join(".").join("f");

        let (ok, e) = cp(&PLAIN, &[&f, &dotted, &dest]);
        assert!(ok, "a repeat is not an error: {e}");
        assert!(e.contains("specified more than once"), "{e}");
        assert_eq!(fs::read(dest.join("f")).unwrap(), b"body");
        let _ = fs::remove_dir_all(&dir);
    }

    // ------------------------------------------- where the destination is --

    /// `-t` with the destination named by the option, and — the part that has
    /// no equivalent without it — a *single* operand still going inside the
    /// directory rather than being taken for the destination.
    #[test]
    fn a_target_directory_takes_every_operand_as_a_source() {
        let dir = scratch("t_dest");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let a = dir.join("a");
        fs::write(&a, b"A").unwrap();

        let flags = CpFlags {
            target_directory: Some(dest.clone().into_os_string()),
            ..PLAIN
        };
        let (ok, e) = cp(&flags, &[&a]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"A");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The wording is `target directory`, not the bare `target` the last
    /// operand gets: the user named this one as a directory, so the diagnostic
    /// says which claim failed.
    #[test]
    fn a_target_directory_that_is_not_one_says_so() {
        let dir = scratch("t_notdir");
        let plain = dir.join("plain");
        fs::write(&plain, b"x").unwrap();
        let a = dir.join("a");
        fs::write(&a, b"A").unwrap();

        let flags = CpFlags {
            target_directory: Some(plain.clone().into_os_string()),
            ..PLAIN
        };
        let (ok, e) = cp(&flags, &[&a]);
        assert!(!ok);
        assert!(e.starts_with("cp: target directory "), "{e}");
        assert!(e.contains("Not a directory"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Checked before `-t`'s directory is looked at, which is GNU's order and
    /// is visible: the directory here does not exist, and the combination is
    /// still what gets reported.
    #[test]
    fn the_two_destination_options_cannot_be_combined() {
        let flags = CpFlags {
            target_directory: Some(OsString::from("nosuch")),
            no_target_directory: true,
            recursive: false,
            verbose: false,
            dereference: Deref::Undefined,
        };
        let (ok, e) = cp(&flags, &[Path::new("a"), Path::new("b")]);
        assert!(!ok);
        assert_eq!(
            e,
            "cp: cannot combine --target-directory (-t) and --no-target-directory (-T)\n"
        );
    }

    /// Without `-T` this would be `cp a b dir` and put both inside `dir`. With
    /// it the destination is one name, so the third operand has nowhere to go.
    #[test]
    fn a_third_operand_has_nowhere_to_go_under_no_target_directory() {
        let (ok, e) = cp(&AS_NAME, &[Path::new("a"), Path::new("b"), Path::new("c")]);
        assert!(!ok);
        assert!(e.starts_with("cp: extra operand "), "{e}");
        assert!(e.contains("'c'"), "{e}");
    }

    /// The whole point of `-T`: a destination that *is* a directory is not
    /// somewhere to copy into, so the copy is refused rather than silently
    /// landing one level down.
    #[test]
    fn no_target_directory_will_not_copy_into_a_directory() {
        let dir = scratch("cap_T");
        let a = dir.join("a");
        fs::write(&a, b"A").unwrap();
        let d = dir.join("d");
        fs::create_dir(&d).unwrap();

        let (ok, e) = cp(&AS_NAME, &[&a, &d]);
        assert!(!ok);
        assert!(e.contains("cannot overwrite directory"), "{e}");
        assert!(!d.join("a").exists(), "nothing went inside it");
        let _ = fs::remove_dir_all(&dir);
    }

    /// And the same destination without `-T`, so the test above is pinning the
    /// flag rather than a refusal that was there anyway.
    #[test]
    fn without_it_the_same_destination_is_copied_into() {
        let dir = scratch("no_cap_T");
        let a = dir.join("a");
        fs::write(&a, b"A").unwrap();
        let d = dir.join("d");
        fs::create_dir(&d).unwrap();

        let (ok, e) = cp(&PLAIN, &[&a, &d]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(d.join("a")).unwrap(), b"A");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `cp -rT src dst` is how a tree is copied *onto* another rather than
    /// inside it — the one thing plain `cp -r` cannot express once `dst`
    /// exists.
    #[test]
    fn recursive_no_target_directory_copies_a_tree_onto_the_destination() {
        let dir = scratch("rT");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("x"), b"X").unwrap();
        let dst = dir.join("dst");
        fs::create_dir(&dst).unwrap();
        fs::write(dst.join("keep"), b"K").unwrap();

        let flags = CpFlags {
            recursive: true,
            ..AS_NAME
        };
        let (ok, e) = cp(&flags, &[&src, &dst]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(dst.join("x")).unwrap(), b"X");
        assert!(!dst.join("src").exists(), "not one level down");
        assert_eq!(
            fs::read(dst.join("keep")).unwrap(),
            b"K",
            "a merge, not a replacement"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The repeat tables count *sources*, and under `-t` every operand is one —
    /// so two operands is two sources even though there are only two words.
    #[test]
    fn a_target_directory_still_notices_a_source_named_twice() {
        let dir = scratch("t_twice");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"body").unwrap();
        let dotted = dir.join(".").join("f");

        let flags = CpFlags {
            target_directory: Some(dest.clone().into_os_string()),
            ..PLAIN
        };
        let (ok, e) = cp(&flags, &[&f, &dotted]);
        assert!(ok, "{e}");
        assert!(e.contains("specified more than once"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    // ----------------------------------------------------- --verbose says --

    /// The whole of `-v` on one file: the arrow line, on **stdout**, and
    /// nothing on stderr. Both halves are asserted, because the sink is the
    /// half that is easy to get wrong and impossible to see once the two are
    /// merged into a terminal.
    #[test]
    fn verbose_names_the_copy_on_stdout() {
        let dir = scratch("v_one");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"hello").unwrap();

        let (ok, out, err) = cp_out(&VERBOSE, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "", "a report of work done is not a diagnostic");
        assert_eq!(out, format!("{} -> {}\n", quoteaf_os(&a), quoteaf_os(&b)));
        let _ = fs::remove_dir_all(&dir);
    }

    /// And without it, silence — so the test above is pinning the option and
    /// not merely observing that `cp` talks.
    #[test]
    fn without_verbose_a_copy_says_nothing() {
        let dir = scratch("v_off");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"hello").unwrap();

        let (ok, out, err) = cp_out(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(out, "");
        assert_eq!(err, "");
        let _ = fs::remove_dir_all(&dir);
    }

    /// GNU announces *before* it opens the source, so a copy that fails is
    /// still announced. This is the case that decides whether `-v` reports
    /// attempts or successes, and upstream's answer is attempts.
    #[test]
    fn verbose_announces_a_copy_that_then_fails() {
        let dir = scratch("v_fail");
        let missing = dir.join("nosuch").join("a");
        let b = dir.join("b");

        // The failure is `cannot stat`, from before the announcement — so this
        // one is *not* announced, which is the other half of the rule.
        let (ok, out, err) = cp_out(&VERBOSE, &[&missing, &b]);
        assert!(!ok);
        assert!(err.contains("cannot stat"), "{err}");
        assert_eq!(out, "", "a source that could not be stat'd is not a copy");

        // Whereas a source that stats and then cannot be *written* is: the
        // destination here is a directory that `cp` without `-r` will not
        // overwrite, and the refusal comes from `copy_one`, still before the
        // announcement.
        let a = dir.join("a");
        fs::write(&a, b"x").unwrap();
        let d = dir.join("d");
        fs::create_dir(&d).unwrap();
        let onto = d.join("a");
        fs::create_dir(&onto).unwrap();
        let (ok, out, err) = cp_out(&VERBOSE, &[&a, &d]);
        assert!(!ok);
        assert!(err.contains("cannot overwrite directory"), "{err}");
        assert_eq!(out, "");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A tree names the directory and then everything in it, and the directory
    /// line comes first because the `mkdir` does.
    #[test]
    fn verbose_names_a_created_directory_and_then_its_contents() {
        let dir = scratch("v_tree");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"F").unwrap();
        let dst = dir.join("dst");

        let (ok, out, err) = cp_out(&VERBOSE_RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            vec![
                format!("{} -> {}", quoteaf_os(&src), quoteaf_os(&dst)),
                format!(
                    "{} -> {}",
                    quoteaf_os(src.join("f")),
                    quoteaf_os(dst.join("f"))
                ),
            ],
            "{out}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The rule GNU wrote a comment to explain: a directory is announced only
    /// when it is *created*. Copying the same tree a second time refreshes the
    /// files and reuses the directory, so the second run names the files alone.
    #[test]
    fn verbose_is_silent_about_a_directory_that_was_already_there() {
        let dir = scratch("v_again");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"F").unwrap();
        let dst = dir.join("dst");
        fs::create_dir(&dst).unwrap();
        fs::create_dir(dst.join("src")).unwrap();

        let (ok, out, err) = cp_out(&VERBOSE_RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(
            out,
            format!(
                "{} -> {}\n",
                quoteaf_os(src.join("f")),
                quoteaf_os(dst.join("src").join("f"))
            ),
            "the directory was reused, so only the file was copied"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A name with a space in it is quoted, which is the reason the line uses
    /// the diagnostic quoting style at all: without it, `a b -> c` could be one
    /// copy or the tail of a different one.
    ///
    /// Asserted by splitting the line on its arrow rather than by rebuilding it
    /// with [`quoteaf_os`], which is what the two tests above do: rebuilding it
    /// would agree with any style at all, including one that never quotes. The
    /// question here is whether the space in the source's name reached the
    /// reader as a quoted name or as two words, and only reading the halves
    /// apart can answer it.
    #[test]
    fn verbose_quotes_a_name_that_needs_it() {
        let dir = scratch("v_quote");
        let a = dir.join("a b");
        let c = dir.join("c");
        fs::write(&a, b"x").unwrap();

        let (ok, out, err) = cp_out(&VERBOSE, &[&a, &c]);
        assert!(ok, "{err}");
        let line = out.strip_suffix('\n').unwrap_or(&out);
        let (rendered_src, _) = line.rsplit_once(" -> ").unwrap_or((line, ""));
        assert!(rendered_src.starts_with('\''), "{rendered_src}");
        assert!(rendered_src.ends_with("a b'"), "{rendered_src}");
        let _ = fs::remove_dir_all(&dir);
    }

    // -------------------------------------- -P / -H / -L: links or targets --

    /// The whole of `cp.c:1239` and `copy.c:845` as a table, with no
    /// filesystem in the way. Every row is a command line; the two columns are
    /// the only two questions the rest of the program ever asks.
    ///
    /// The two rows worth staring at are the `-H` ones — they are the only
    /// place the columns disagree, and a single `follow: bool` could not
    /// express them at all.
    #[test]
    fn the_dereference_table() {
        let rows: &[(bool, Deref, bool, bool)] = &[
            // recursive, given,               operand, walked
            (false, Deref::Undefined, true, true),
            (true, Deref::Undefined, false, false),
            (false, Deref::Never, false, false),
            (true, Deref::Never, false, false),
            (false, Deref::Always, true, true),
            (true, Deref::Always, true, true),
            (false, Deref::CommandLine, true, false),
            (true, Deref::CommandLine, true, false),
        ];
        for &(recursive, dereference, operand, walked) in rows {
            let flags = CpFlags {
                recursive,
                dereference,
                ..PLAIN
            };
            assert_eq!(
                (flags.follow_operand(), flags.follow_walked()),
                (operand, walked),
                "{recursive} {dereference:?}"
            );
        }
    }

    /// `-P` alone. Without it, `cp link dst` writes a *file*; the point of the
    /// option is to get `-r`'s behaviour without `-r`.
    #[cfg(unix)]
    #[test]
    fn no_dereference_copies_the_link_without_r() {
        let dir = scratch("P_link");
        fs::write(dir.join("file"), b"BODY").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink("file", &link).unwrap();
        let dst = dir.join("dst");

        let (ok, e) = cp(&NO_DEREF, &[&link, &dst]);
        assert!(ok, "{e}");
        let meta = fs::symlink_metadata(&dst).unwrap();
        assert!(meta.file_type().is_symlink(), "a link, not its target");
        assert_eq!(fs::read_link(&dst).unwrap(), PathBuf::from("file"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same-file guard keys on the policy and not on `-r`, which is what
    /// changed when `-P` arrived: two distinct links to one file are two
    /// distinct things to copy, so this is allowed. Under [`PLAIN`] — where
    /// both are followed — the identical command is refused, and the test
    /// above this one in the file pins that half.
    #[cfg(unix)]
    #[test]
    fn no_dereference_lets_one_link_replace_another_without_r() {
        let dir = scratch("P_two_links");
        fs::write(dir.join("file"), b"BODY").unwrap();
        let (one, two) = (dir.join("one"), dir.join("two"));
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let (ok, e) = cp(&NO_DEREF, &[&one, &two]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        let (ok, e) = cp(&PLAIN, &[&one, &two]);
        assert!(!ok, "followed, they are one file");
        assert!(e.contains("are the same file"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `-L` reaches *inside* the tree, which is the half no option could
    /// express before [`Job`] carried the flags into the recursion: the link
    /// to a file becomes a file, and the link to a directory becomes a
    /// directory with the contents copied again.
    #[cfg(unix)]
    #[test]
    fn dereference_follows_links_found_by_the_walk() {
        let dir = scratch("L_walk");
        let src = dir.join("t");
        fs::create_dir(&src).unwrap();
        fs::create_dir(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        fs::write(src.join("sub/s.txt"), b"S").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("flink")).unwrap();
        std::os::unix::fs::symlink("sub", src.join("dlink")).unwrap();
        let dst = dir.join("d");

        let (ok, e) = cp(&DEREF_ALL_R, &[&src, &dst]);
        assert!(ok, "{e}");
        assert!(
            !fs::symlink_metadata(dst.join("flink"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link to a file became a file"
        );
        assert_eq!(fs::read(dst.join("flink")).unwrap(), b"A");
        assert!(
            dst.join("dlink").is_dir()
                && !fs::symlink_metadata(dst.join("dlink"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
            "the link to a directory became a directory"
        );
        assert_eq!(fs::read(dst.join("dlink/s.txt")).unwrap(), b"S");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A link that points at nothing has no target to copy, so `-L` fails on
    /// it where the default would have copied the link. GNU's wording, from
    /// the same `stat` this one comes from: `cannot stat 't/dangle'`.
    #[cfg(unix)]
    #[test]
    fn dereference_fails_on_a_dangling_link_in_the_tree() {
        let dir = scratch("L_dangle");
        let src = dir.join("t");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        std::os::unix::fs::symlink("nowhere", src.join("dangle")).unwrap();
        let dst = dir.join("d");

        let (ok, e) = cp(&DEREF_ALL_R, &[&src, &dst]);
        assert!(!ok, "one entry failed, so the copy failed");
        assert!(e.contains("cannot stat "), "{e}");
        assert!(e.contains("dangle"), "{e}");
        // The rest of the directory is still copied: one bad entry ends that
        // entry, not the walk.
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"A");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `-H` is the split one: the operand is followed, so a link to a
    /// directory is descended into, and every link *found* down there is
    /// copied as a link — including a dangling one, which `-L` could not have
    /// copied at all.
    #[cfg(unix)]
    #[test]
    fn command_line_dereference_follows_only_the_operand() {
        let dir = scratch("H_split");
        let src = dir.join("t");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"A").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("flink")).unwrap();
        std::os::unix::fs::symlink("nowhere", src.join("dangle")).unwrap();
        let dlink = dir.join("dlink");
        std::os::unix::fs::symlink("t", &dlink).unwrap();
        let dst = dir.join("d");

        let (ok, e) = cp(&DEREF_CMD_R, &[&dlink, &dst]);
        assert!(ok, "{e}");
        assert!(
            dst.is_dir() && !fs::symlink_metadata(&dst).unwrap().file_type().is_symlink(),
            "the operand was followed"
        );
        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"A");
        for name in ["flink", "dangle"] {
            assert!(
                fs::symlink_metadata(dst.join(name))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{name} was found by the walk, so it stays a link"
            );
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// Replacing a link says two things, in this order: the removal and then
    /// the copy. The removal line exists because there is no atomic "replace"
    /// for a symlink — and it is the only case where `cp` unlinks anything, so
    /// a regular file overwritten in place says nothing extra.
    #[cfg(unix)]
    #[test]
    fn verbose_names_the_link_it_removed_first() {
        let dir = scratch("P_removed");
        fs::write(dir.join("file"), b"BODY").unwrap();
        let (one, two) = (dir.join("one"), dir.join("two"));
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let flags = CpFlags {
            verbose: true,
            ..NO_DEREF
        };
        let (ok, out, err) = cp_out(&flags, &[&one, &two]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].starts_with("removed "), "{out}");
        assert!(lines[0].contains("two"), "{out}");
        assert!(lines[1].contains(" -> "), "{out}");

        // Nothing to remove, nothing said.
        let three = dir.join("three");
        let (ok, out, err) = cp_out(&flags, &[&one, &three]);
        assert!(ok, "{err}");
        assert_eq!(out.lines().count(), 1, "{out}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Three options that set one field, so the last one given wins and there
    /// is no diagnostic for giving two. Measured against GNU: `cp -LP link d`
    /// writes a link and `cp -PL link d` writes a file.
    #[test]
    fn the_last_dereference_option_wins() {
        for (argv, want) in [
            (["-P", "-L"], Deref::Always),
            (["-L", "-P"], Deref::Never),
            (["-H", "-P"], Deref::Never),
            (["-P", "-H"], Deref::CommandLine),
        ] {
            let (flags, _) = run_parse(&[argv[0], argv[1], "a", "b"]);
            assert_eq!(flags.dereference, want, "{argv:?}");
        }
    }

    /// The long spellings, and the fact that `-H` has none — GNU gives it no
    /// entry in `long_opts[]`, so `--H` is not an option at all.
    #[test]
    fn the_dereference_long_spellings() {
        let (flags, _) = run_parse(&["--dereference", "a", "b"]);
        assert_eq!(flags.dereference, Deref::Always);
        let (flags, _) = run_parse(&["--no-dereference", "a", "b"]);
        assert_eq!(flags.dereference, Deref::Never);
    }

    // ---------------------------------------- the order a directory is read --

    /// Whatever the order, every entry has to come out exactly once. Asserted
    /// on both platforms, because only one of them sorts and a sort that drops
    /// or duplicates an entry would be a silently incomplete copy.
    #[test]
    fn the_directory_read_returns_every_entry_once() {
        let dir = scratch("read_all");
        for name in ["a", "b", "c", "d"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        fs::create_dir(dir.join("sub")).unwrap();

        let mut got: Vec<String> = read_dir_fastread(&dir)
            .unwrap()
            .iter()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        got.sort();
        assert_eq!(got, ["a", "b", "c", "d", "sub"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// And on Unix the order is inode-ascending, which is gnulib's
    /// `SAVEDIR_SORT_FASTREAD` and therefore GNU `cp`'s. Asserted against the
    /// inodes themselves rather than against a fixed list of names: what the
    /// filesystem allocates is its business, and the claim being made is only
    /// that whatever it allocated comes back in order.
    ///
    /// Five entries rather than two, because a two-element list is sorted by
    /// half the possible implementations of `sort` including several wrong
    /// ones.
    #[cfg(unix)]
    #[test]
    fn the_directory_read_is_in_inode_order() {
        use std::os::unix::fs::DirEntryExt as _;

        let dir = scratch("read_ino");
        // Names deliberately anti-correlated with creation order, so that a
        // sort by *name* would produce a different answer and be caught.
        for name in ["e", "d", "c", "b", "a"] {
            fs::write(dir.join(name), b"x").unwrap();
        }

        let inodes: Vec<u64> = read_dir_fastread(&dir)
            .unwrap()
            .iter()
            .map(fs::DirEntry::ino)
            .collect();
        assert_eq!(inodes.len(), 5);
        assert!(
            inodes.windows(2).all(|w| w[0] <= w[1]),
            "not ascending: {inodes:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The line the repeat rule draws. Two hard links share an inode but are
    /// two entries, and asking for a copy of each is a legitimate request —
    /// which is why the rule needs [`entry_id`] and not just [`file_id`].
    #[cfg(unix)]
    #[test]
    fn two_hard_links_to_one_file_are_not_one_source_named_twice() {
        let dir = scratch("two_hard");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let one = dir.join("one");
        let two = dir.join("two");
        fs::write(&one, b"body").unwrap();
        fs::hard_link(&one, &two).unwrap();

        let (ok, e) = cp(&PLAIN, &[&one, &two, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "", "nothing to warn about");
        assert_eq!(fs::read(dest.join("one")).unwrap(), b"body");
        assert_eq!(fs::read(dest.join("two")).unwrap(), b"body");
        let _ = fs::remove_dir_all(&dir);
    }

    /// With one source there is no pair, so the tables are never built. This
    /// asserts the case that would otherwise be caught by them wrongly: a
    /// single source copied onto a destination the *previous* run made.
    #[test]
    fn a_lone_source_is_never_a_repeat_of_itself() {
        let dir = scratch("lone");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"body").unwrap();

        assert!(cp(&PLAIN, &[&f, &dest]).0);
        let (ok, e) = cp(&PLAIN, &[&f, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_entry_keeps_dot_and_dotdot_apart() {
        assert_eq!(split_entry(Path::new("a/.")).1, OsString::from("."));
        assert_eq!(split_entry(Path::new("a/..")).1, OsString::from(".."));
        assert_eq!(split_entry(Path::new("a/b/")).1, OsString::from("b"));
        assert_eq!(
            split_entry(Path::new("b")),
            (PathBuf::from("."), "b".into())
        );
    }

    // ------------------------------------------------- modes, module docs 8 --
    //
    // These four run only on a POSIX host, because there is nothing on Windows
    // for them to assert about. `scripts/cp-diff.sh` is what certifies the same
    // behaviour against GNU itself; these exist so that a regression is caught
    // by `cargo test` on the target rather than only by a harness that needs a
    // GNU userland to run.

    /// Set the umask, run `f`, put the umask back.
    ///
    /// The umask is process-wide and `cargo test` runs tests on threads of one
    /// process, so two tests doing this at once would each see the other's
    /// mask. The lock makes them take turns. It does *not* protect against an
    /// unrelated test creating a file while a mask is installed — nothing can,
    /// short of running single-threaded — which is why these are the only tests
    /// in this file that assert a mode at all.
    #[cfg(unix)]
    fn with_umask<T>(mask: u32, f: impl FnOnce() -> T) -> T {
        use std::sync::Mutex;
        static TURN: Mutex<()> = Mutex::new(());
        // A poisoned lock means another umask test panicked; its `old` was
        // restored on unwind only if it got that far, so the mask may be
        // whatever it left. Proceeding is still the right call: the panic will
        // already be reported, and refusing here would hide this test's result
        // behind that one.
        let _guard = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `umask` cannot fail, takes and returns a plain integer, and
        // touches no memory.
        let old = unsafe { umask(mask) };
        let out = f();
        // SAFETY: as above.
        unsafe { umask(old) };
        out
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set_test_mode(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    /// A new destination is created with the source's mode *narrowed by the
    /// umask* — which is what the kernel does when the mode is passed to
    /// `open`, and what `fs::copy`'s trailing `chmod` undid.
    ///
    /// The expectations are measured, not derived: each row was produced by
    /// GNU `cp` under WSL before it was written down here.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_new_file_is_narrowed_by_umask() {
        // (umask, source mode, what GNU produces)
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o600, 0o600),
            (0o000, 0o777, 0o777),
            (0o077, 0o777, 0o700),
            (0o077, 0o600, 0o600),
        ];
        let dir = scratch("file_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.join(format!("a{i}"));
            let b = dir.join(format!("b{i}"));
            fs::write(&a, b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&PLAIN, &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The security half of module docs, bug 8: copying a wide file over a
    /// narrow one must not widen the narrow one. GNU reopens an existing
    /// destination with no mode argument at all.
    #[cfg(unix)]
    #[test]
    fn an_existing_destination_keeps_its_own_mode() {
        let dir = scratch("keep_mode");
        let a = dir.join("public");
        let b = dir.join("private");
        fs::write(&a, b"wide").unwrap();
        set_test_mode(&a, 0o777);
        fs::write(&b, b"narrow").unwrap();
        set_test_mode(&b, 0o600);

        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"wide", "contents are copied");
        assert_eq!(mode_of(&b), 0o600, "permissions are not");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A copied directory ends at `src & 07777 & ~umask`, sticky bit included
    /// — 1777 under 022 is 1755, which is what GNU produces and what the
    /// verbatim mode carry-over got wrong.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_copied_directory_is_narrowed_by_umask() {
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o1777, 0o1755),
            (0o000, 0o1777, 0o1777),
            (0o077, 0o1777, 0o1700),
            (0o022, 0o700, 0o700),
        ];
        let dir = scratch("dir_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.join(format!("s{i}"));
            let b = dir.join(format!("d{i}"));
            fs::create_dir(&a).unwrap();
            fs::write(a.join("inner"), b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&RECURSIVE, &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
            assert!(b.join("inner").is_file(), "and it was actually filled");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A source directory the copy cannot write into must still be filled: the
    /// mode goes on last, not first. 0500 is the case that mattered — it has no
    /// owner-write, so a copy that set the mode at `mkdir` time could not put
    /// anything inside what it had just made.
    #[cfg(unix)]
    #[test]
    fn a_read_only_source_directory_is_copied_whole_and_ends_read_only() {
        let dir = scratch("ro_dir");
        let a = dir.join("src");
        let b = dir.join("dst");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("inner"), b"x").unwrap();
        set_test_mode(&a, 0o500);

        let (ok, err) = cp(&RECURSIVE, &[&a, &b]);
        assert!(ok, "{err}");
        assert!(b.join("inner").is_file(), "contents got in");
        assert_eq!(mode_of(&b), 0o500, "and the mode went on afterwards");

        // Leave both writable again so the scratch directory can be removed.
        set_test_mode(&a, 0o700);
        set_test_mode(&b, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Module docs, bug 7. The assertion that matters is not the message but
    /// `fs::read`: the defect this pins reported success and said nothing, so a
    /// test that only checked the diagnostic would have passed against it.
    #[test]
    fn copying_a_file_onto_itself_is_refused_and_leaves_it_whole() {
        let dir = scratch("same_file");
        let a = dir.join("a");
        fs::write(&a, b"contents").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &a]);
        assert!(!ok, "should have been refused");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same file reached by a second spelling. A string comparison of the
    /// two operands would let this one through, which is why there is not one.
    #[test]
    fn a_file_onto_itself_by_another_path_is_refused() {
        let dir = scratch("same_file_dotted");
        let a = dir.join("a");
        let sub = dir.join("sub");
        fs::write(&a, b"contents").unwrap();
        fs::create_dir(&sub).unwrap();
        let dotted = sub.join("..").join("a");
        let (ok, err) = cp(&PLAIN, &[&a, &dotted]);
        assert!(!ok, "should have been refused: {err}");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A destination that merely *exists* is not the same file, and must still
    /// be overwritten. Without this the refusal above would be a way of
    /// breaking `cp` for every ordinary overwrite.
    #[test]
    fn an_existing_different_destination_is_still_overwritten() {
        let dir = scratch("same_file_neg");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, err) = cp(&PLAIN, &[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
    }

    #[test]
    fn one_operand_names_it() {
        let (ok, err) = cp(&PLAIN, &[Path::new("solo")]);
        assert!(!ok);
        assert!(err.contains("missing destination file operand"), "{err}");
        assert!(err.contains("solo"), "{err}");
    }

    #[test]
    fn a_directory_needs_recursive() {
        let dir = scratch("needs_r");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&PLAIN, &[&sub, &dir.join("copy")]);
        assert!(!ok);
        assert!(err.contains("omitting directory"), "{err}");
        assert!(!dir.join("copy").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_a_tree() {
        let dir = scratch("tree");
        let src = dir.join("src");
        fs::create_dir_all(src.join("deep/deeper")).unwrap();
        fs::write(src.join("top"), b"1").unwrap();
        fs::write(src.join("deep/mid"), b"2").unwrap();
        fs::write(src.join("deep/deeper/bottom"), b"3").unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("top")).unwrap(), b"1");
        assert_eq!(fs::read(dst.join("deep/mid")).unwrap(), b"2");
        assert_eq!(fs::read(dst.join("deep/deeper/bottom")).unwrap(), b"3");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("partial");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let a = dir.join("a");
        let c = dir.join("c");
        fs::write(&a, b"a").unwrap();
        fs::write(&c, b"c").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &dir.join("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still copy");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 2 in the module docs. Before the fix this filled the disk.
    #[test]
    fn refuses_to_copy_a_directory_into_itself() {
        let dir = scratch("into_itself");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"x").unwrap();

        // `cp -r src src` — the target resolves to `src/src`.
        let (ok, err) = cp(&RECURSIVE, &[&src, &src]);
        assert!(!ok);
        assert!(err.contains("into itself"), "{err}");
        assert!(!src.join("src").exists());

        // `cp -r src src/nested` — the same thing spelled differently.
        let (ok, err) = cp(&RECURSIVE, &[&src, &src.join("nested")]);
        assert!(!ok, "{err}");
        assert!(err.contains("into itself"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 5, end to end: the source resolves to no name, so there is nothing to
    /// create inside the destination and the copy is refused rather than merged.
    #[test]
    fn a_dotdot_source_is_refused_rather_than_merged() {
        let dir = scratch("dotdot");
        let inner = dir.join("inner");
        let dst = dir.join("dst");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(dir.join("sibling"), b"x").unwrap();

        let (ok, err) = cp(&RECURSIVE, &[&inner.join(".."), &dst]);
        assert!(!ok);
        assert!(err.contains("names nothing"), "{err}");
        assert!(
            !dst.join("sibling").exists(),
            "the parent's contents must not be merged into the destination"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 1 in the module docs, the non-terminating half. `loop` points at its
    /// own parent, so following it descends for ever. With `file_type()` the walk
    /// copies the link and stops.
    #[test]
    #[cfg(unix)]
    fn a_symlink_loop_in_the_tree_does_not_recurse_for_ever() {
        let dir = scratch("loop");
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/f"), b"x").unwrap();
        std::os::unix::fs::symlink("..", src.join("sub/loop")).unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("sub/f")).unwrap(), b"x");
        let link = fs::symlink_metadata(dst.join("sub/loop")).unwrap();
        assert!(
            link.file_type().is_symlink(),
            "the loop must arrive as a link, not as a copied subtree"
        );
        assert_eq!(
            fs::read_link(dst.join("sub/loop")).unwrap(),
            Path::new("..")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 1's other half: a link in the tree used to become a full copy of its
    /// target, so a tree of N links to one file produced N copies of that file.
    #[test]
    #[cfg(unix)]
    fn a_symlink_in_the_tree_is_copied_as_a_symlink() {
        let dir = scratch("tree_link");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("real"), b"contents").unwrap();
        std::os::unix::fs::symlink("real", src.join("link")).unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(meta.file_type().is_symlink(), "a link must stay a link");
        assert_eq!(fs::read_link(dst.join("link")).unwrap(), Path::new("real"));
        // And the relative link still resolves, in its new directory.
        assert_eq!(fs::read(dst.join("link")).unwrap(), b"contents");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Without `-r`, a symlink operand is followed — that is GNU's behaviour and
    /// it is unchanged. Only the recursive case stopped following.
    #[test]
    #[cfg(unix)]
    fn without_recursive_a_symlink_operand_is_still_followed() {
        let dir = scratch("deref");
        let real = dir.join("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let out = dir.join("out");
        let (ok, err) = cp(&PLAIN, &[&link, &out]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(&out).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "plain cp copies what the link points at"
        );
        assert_eq!(fs::read(&out).unwrap(), b"contents");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 3 in the module docs: a 0700 source used to produce a 0755 copy,
    /// publishing everything inside it.
    #[test]
    #[cfg(unix)]
    fn a_copied_directory_keeps_the_source_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("modes");
        let src = dir.join("private");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("secret"), b"x").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();

        let dst = dir.join("copy");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        let mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "a private directory must stay private");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must copy like any other, including through a recursive walk.
    #[test]
    #[cfg(unix)]
    fn copies_a_tree_holding_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let dir = scratch("nonutf8");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();

        let mut name = src.clone().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let odd = PathBuf::from(OsString::from_vec(name));
        fs::write(&odd, b"x").unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");

        let mut want = dst.clone().into_os_string().into_vec();
        want.extend_from_slice(b"/\x80bad");
        let copied = PathBuf::from(OsString::from_vec(want));
        assert_eq!(fs::read(&copied).unwrap(), b"x");
        assert!(
            copied.as_os_str().as_bytes().ends_with(b"\x80bad"),
            "the name must survive byte for byte"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------- is_inside --

    #[test]
    fn is_inside_sees_through_a_different_spelling() {
        let dir = scratch("inside");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();

        assert!(is_inside(&src.join("child"), &src));
        assert!(is_inside(&src.join("a/b/c"), &src));
        // `src/./` and `src` are the same directory.
        assert!(is_inside(&src.join(".").join("child"), &src));
        assert!(!is_inside(&dir.join("sibling"), &src));
        // A sibling whose name merely starts with the source's is not inside it.
        assert!(!is_inside(&dir.join("srcery"), &src));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_as_far_as_exists_handles_a_path_that_is_not_there_yet() {
        let dir = scratch("resolve");
        let deep = dir.join("nope/not/here");
        let resolved = resolve_as_far_as_exists(&deep).unwrap();
        assert!(resolved.ends_with("nope/not/here"), "{resolved:?}");
        assert!(resolved.is_absolute(), "{resolved:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
