//! mv — move (rename) files.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `mv` is the
//! second of the 49 bins listed there, after `rm`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Four further bugs, in the lines this rewrite replaced
//!
//! 1. **`--` was not an end-of-options marker.** `mv -- -foo bar` answered
//!    `unknown option: --`. `--` is the only portable way to name a source file
//!    whose name begins with a dash, so such a file could not be moved at all.
//!
//! 2. **`-f` suppressed the diagnostic but not the failure.** The old `-f`
//!    branch skipped the `eprintln!` and still set the exit status to 1, so
//!    `mv -f a b` on a failure printed *nothing* and exited non-zero: the
//!    caller was told something went wrong and given no way to find out what.
//!    That is not what `-f` means anywhere. In GNU `mv`, `-f` suppresses the
//!    *prompt* that would otherwise be raised before overwriting; it has never
//!    suppressed errors.
//!
//!    For a while after the rewrite it was accepted and inert, on the reasoning
//!    that there were no prompts for it to suppress. That was true and is no
//!    longer: with [`Interactive`] implemented, `-f` is the third value of the
//!    field `-i` and `-n` also write, so it cancels an earlier `-i` —
//!    `mv -i -f a b` moves silently — and it suppresses the question
//!    [`abandon_move`] puts over an unwritable destination *with no option
//!    given at all*, which is the arm nobody expects.
//!
//! 3. **A source ending in `..` moved something the user never named.**
//!    `compute_target` did `dest.join(src.file_name().unwrap_or_default())`, and
//!    `Path::file_name` is `None` for a path whose last component is `..` — so
//!    `unwrap_or_default()` produced an *empty* name, `dest.join("")` collapsed
//!    back to `dest` itself, and `mv a/.. dst` asked the kernel to rename `a`'s
//!    **parent directory** to `dst`. If `dst` was an empty directory that
//!    succeeds: the user asks to move something into `dst` and instead the
//!    directory they were standing in is moved *onto* `dst`. Reachable from an
//!    ordinary glob (`mv */.. dst`).
//!
//!    The target name is now built by [`target_in_directory`], which appends the
//!    last component's *bytes* — `.` and `..` included — and so has no empty
//!    name to collapse. See [`coreutils::fileid`] for why the split is done on
//!    bytes rather than through `Path::file_name`.
//!
//! 4. **The cross-filesystem fallback silently turned a symlink into a copy of
//!    its target.** When `rename` fails with `EXDEV`, `mv` must copy and then
//!    unlink. The old fallback used `fs::copy`, which *follows* symlinks — so
//!    moving a symlink across a filesystem boundary read the file it pointed at,
//!    wrote those bytes at the destination as an ordinary file, and deleted the
//!    link. A symlink went in and a full copy came out, with no message. The
//!    link is now recreated with `symlink(2)` and only then unlinked. (A
//!    *dangling* symlink hit the same path and failed with `No such file or
//!    directory`, naming the link — which reads as "the link is missing" when
//!    the link was right there.)
//!
//!    The fallback is also no longer entered for *every* rename failure, only
//!    for a genuine cross-device one. Previously a `mv nonexistent dst` failed
//!    `rename`, fell through to `fs::copy`, and reported the *copy's* error,
//!    which happened to read the same but need not have.
//!
//! # And seventeen more, found by measurement rather than by reading
//!
//! The four above were found by reading the code. That method had reached its
//! limit — the remaining bugs were all in behaviour that *looked* right. So
//! `scripts/mv-diff.sh` runs this `mv` and GNU coreutils 9.4 over the same 178
//! fixtures and compares exit status, both streams, and the resulting tree
//! byte-for-byte. It found **seventeen** differences on its first run, none of
//! which had been suspected.
//!
//! Nearly all of them came from one structural mistake: the old code decided
//! *first* whether the destination was a directory, then computed a target, then
//! renamed. GNU inverts this. It renames **speculatively** first
//! (`mv.c:466`) — `RENAME_NOREPLACE`, so it cannot clobber — and only asks any
//! further question if that fails. The order is not an optimisation; it is what
//! makes the answers come out right, because a rename that succeeded proves the
//! destination was free and every "is the destination …" check is then moot. The
//! tri-state that carries this is [`Renamed`], GNU's `x.rename_errno`.
//!
//! The differences it exposed, grouped:
//!
//! - **Refusals that were not made at all.** Moving a file onto itself
//!   (`mv a a`), onto a hard link to itself, or through a symlink to itself
//!   destroyed the file and left nothing — `mv link file`, where `link` points
//!   at `file`, deleted `file`. [`same_file_ok`] is GNU's check, reduced to this
//!   `mv`'s option set and then measured case by case against GNU, including the
//!   pair upstream documents at `copy.c:1909`: with `l` a hard link to `f` and
//!   `s` a symlink to `f`, `mv s f` must fail and `mv s l` must succeed.
//! - **Two sources with the same basename silently ate each other.**
//!   `mv one/same two/same dir` moved both to `dir/same` and reported success:
//!   two files in, one file out, no message. GNU keeps a set of
//!   already-written destinations ([`DestInfo`]) and refuses the second with
//!   `will not overwrite just-created`.
//! - **Directory-vs-non-directory collisions.** Overwriting a directory with a
//!   file, or a file with a directory, produced the kernel's bare `Is a
//!   directory` rather than the sentence naming both operands.
//! - **The wrong operand was named.** A failure caused by the *destination* —
//!   it is a non-empty directory, it is a running binary, the disk is full —
//!   named the source too, which `copy.c:2851` says "is more likely to confuse
//!   the user than be helpful". See [`blames_the_destination`].
//! - **Diagnostics that were this file's own sentences** rather than the ones
//!   scripts and tests actually match on: `target 'c' is not a directory` for
//!   `target 'c': Not a directory`, and a bare `Invalid argument` where a
//!   directory had been asked to become a subdirectory of itself.
//!
//! An eighteenth arrived long afterwards, from a case written while measuring
//! `--strip-trailing-slashes` — the no-option half of one of §21's pairs, aimed
//! at something else entirely. `mv f d/`, with `d` a regular file, said `cannot
//! move 'f' to 'd/': Not a directory` where GNU says `cannot stat 'd/': Not a
//! directory`. The destination's own `lstat` had its error discarded with
//! `.ok()`, so what got printed was the errno left over from the *speculative*
//! rename — a sentence claiming a rename was attempted and refused, when none
//! had been attempted and the failure belonged to a lookup. It is the same
//! structural mistake as the rest of the list, arriving through the one path
//! nobody had aimed a case at, and it is the argument for the paragraph below:
//! the harness finds these, reading does not. Pinned by §9's four
//! destination-cannot-be-stat'd cases; see [`move_one`]'s destination stat.
//!
//! The harness is the artifact to keep, not the fix list: it is 300 cases, it
//! runs in about a minute, and it is how the next seventeen get found.
//! Twelve of its cases are marked as differing on purpose: four are `--help`
//! and `--version`, which name SlateOS rather than the GNU project and always
//! will; six are options this file does not implement yet — so
//! implementing one is expected to *promote* a case rather than to add one; and
//! two are the cross-device defects in the section below, which are the first
//! xfails here that name a `known-issues.md` entry rather than a missing option.
//! `-v` was the first to be promoted
//! that way; its five entries became §14 and gained four more, which is the
//! shape the rest should follow. `-i`/`-f`/`-n` was the second: its twelve
//! entries became §15 and gained twenty more, plus the `--no-cl` abbreviation
//! case in §2 that had been an xfail only because the option it resolves to did
//! not exist. `-t`/`-T` was the third and the widest so far: eleven entries
//! became §16 and gained twenty-two more, because those two options are about
//! *which operand is the destination*, so every operand-count and wrong-shape
//! diagnostic acquires a second spelling. `-u`/`--update` was the fourth: six
//! entries became §17 and gained thirty-one more, nearly all of them about the
//! *order* two options were given in, because `--update`'s three words write
//! the same two fields `-i`/`-f`/`-n` do. `-b`/`--backup`/`-S` was the fifth:
//! fifteen entries became §18 and gained forty-three more, because a backup is
//! a second file under a name this `mv` chooses — so every case has a name to
//! check as well as a tree — and because the option *lifts* three of §13's
//! refusals, each of which then has to be measured both ways round.
//! `--strip-trailing-slashes` was the sixth, and the narrowest: two entries
//! became §21 and gained twenty-two more. It has no policy of its own — it
//! edits the operands and then everything else happens as it would have — so
//! half of those cases are *pairs*, the same command line with the option and
//! without, and nearly every one is about *when* the edit lands relative to the
//! four checks that read the operands first. See [`strip_operands`].
//!
//! # Options this implementation does not have
//!
//! `-Z`/`--context`, `--debug` and `--no-copy` are
//! recognised and rejected with a message saying they are not implemented,
//! rather than ignored. Silently ignoring an option that changes what happens
//! to an existing destination would lose a file the user asked to be kept; for
//! this utility that mistake is unrecoverable, and an error costs only a
//! retype.
//!
//! They are all listed in [`LONG_OPTIONS`] anyway, because the table is what
//! decides whether an abbreviation is ambiguous — drop `--verbose` and `mv --v`
//! resolves to `--version` and prints a banner instead of failing.
//!
//! Moving a **directory across a filesystem boundary** is also not implemented:
//! it needs a recursive copy that preserves modes, symlinks and hard links, and
//! doing it wrong loses data quietly. It reports that it is not implemented
//! rather than attempting a partial job. Logged in `known-issues.md`.
//!
//! # The cross-device fallback, and what §22 found in it
//!
//! §22 of the harness arrived long after the rest and is the one section not
//! about an option: it moves files between two *filesystems*, which the other
//! 300 cases never do because they all run inside one temporary directory. The
//! section exists because of a claim that had been written down twice and never
//! checked — that a second filesystem needed a mount and therefore a password.
//! It did not. `$XDG_RUNTIME_DIR` and `/dev/shm` are already mounted, already
//! writable, and already a different `st_dev` from `/tmp`.
//!
//! Its first run found four defects in a fallback that had been read carefully
//! and looked right, and every one of them was invisible on a one-filesystem
//! machine:
//!
//! - **The destination was written *through*, not replaced.** `fs::copy` opens
//!   the destination with `O_TRUNC`, so `mv far/f g` rewrote the inode `g`
//!   named — and every *other* name for it. A file the user never mentioned was
//!   destroyed. GNU unlinks first, and its comment says why in one sentence:
//!   *"so that a cross-device `mv' acts as if it were really using the rename
//!   syscall"*. See [`clear_destination`].
//! - **The times, the owner and the set-ID bits were all thrown away.**
//!   `fs::copy` is documented to carry the permission bits and documented to
//!   carry nothing else, so a moved file arrived stamped with the moment of the
//!   move. A move is meant to be indistinguishable from a rename, and `mv` has
//!   no option to turn any of that preservation off. See
//!   [`copy_across_devices`] for the four ordered steps that replaced it.
//! - **Failures named the operation rather than the step.** One `cannot move X
//!   to Y` for a source that would not open and a destination that would not be
//!   created — the same errno for both, with the half of the information that
//!   says which file to go and look at discarded. See [`Failed`].
//! - **A hard-linked group moved together arrives as separate files.** Still
//!   true; it belongs with the recursive walk. Logged in `known-issues.md`.
//!
//! The first three are fixed and their cases are ordinary `run_case`s now. The
//! general lesson is the one the eighteenth bug above already taught, in a
//! sharper form: reading found none of these, and the reason the harness could
//! not find them either was a sentence in its own header asserting the test was
//! impossible.

use coreutils::backup::{self, BackupType};
use coreutils::copy::{self, Made, ModeDebt, chown_to_source, preserve_attributes};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::fileid::{
    self, Copied, FileId, file_id, nlink, same_entry, same_inode, split_entry,
};
use coreutils::fsattr::{self, Link, On};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::hardlink;
use coreutils::overwrite::{self, Interactive};
use coreutils::pathname::strip_trailing_slashes;
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
use coreutils::utimecmp;
use coreutils::yesno::{Answers, StdinAnswers};
use std::borrow::Cow;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `mv`'s usage status is 1, like almost every utility's; see
/// [`coreutils::getopt::Error`] for the two that differ and why.
const MV: Program = Program::new("mv", 1);

/// GNU `mv`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Every entry is here whether or not this implementation acts on it —
/// see the module docs for why leaving one out is a silent wrong answer rather
/// than a missing feature.
///
/// Measured with `mv --=x`, which an empty prefix makes print the whole table:
///
/// ```text
/// mv: option '--=x' is ambiguous; possibilities: '--backup' '--context'
/// '--debug' '--force' '--interactive' '--no-clobber' '--no-copy'
/// '--no-target-directory' '--strip-trailing-slashes' '--suffix'
/// '--target-directory' '--update' '--verbose' '--help' '--version'
/// ```
///
/// **This table was originally written from recall and was wrong in both
/// directions**, which is the reason `scripts/getopt-ambiguity-check.py` now
/// exists — it found this by asking GNU about every prefix. It carried an
/// `("exchange", …)` that the reference does not have (it is a later upstream
/// addition) and lacked `("no-copy", …)` that it does, so `mv --no-c` resolved
/// to `--no-clobber` here where GNU calls it ambiguous. Nothing user-visible
/// went wrong only because this `mv` refuses `--no-clobber` anyway; the day it
/// implements it, `mv --no-c` would have silently meant `--no-clobber`.
///
/// The rule the mistake teaches: **the table tracks the reference we can
/// measure, not the newest upstream we can remember.** A table half from one
/// release and half from another matches no getopt anywhere.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("backup", Takes::Optional),
    ("context", Takes::Optional),
    ("debug", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("no-clobber", Takes::Nothing),
    ("no-copy", Takes::Nothing),
    ("no-target-directory", Takes::Nothing),
    ("strip-trailing-slashes", Takes::Nothing),
    ("suffix", Takes::Required),
    ("target-directory", Takes::Required),
    ("update", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What `--update`'s argument can say — GNU's `update_type_string` paired with
/// `update_type` (`mv.c:55-63`), in that order, which is the order the
/// `Valid arguments are:` list is printed in.
///
/// Three words for what is really two fields, and the mapping is not the
/// obvious one: `all` and `none` both turn [`MvFlags::update`] *off*, differing
/// in what they set [`MvFlags::interactive`] to. See the parse arm.
const UPDATE_TYPES: &[(&str, UpdateType)] = &[
    ("all", UpdateType::All),
    ("none", UpdateType::None),
    ("older", UpdateType::Older),
];

/// GNU's `enum Update_type` (`copy.h:61`). Not stored — it is resolved into
/// [`MvFlags`]'s two fields at parse time, exactly as `mv.c:381-397` does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateType {
    /// `--update=all`: the default behaviour, spelled out. Cancels an earlier
    /// `-u` *and* an earlier `-i`.
    All,
    /// `--update=none`: leave every existing destination, and succeed.
    None,
    /// `--update=older`, and the meaning of a bare `-u`.
    Older,
}

/// The options that change what a move *does*.
///
/// `Clone` but not `Copy` since [`MvFlags::backup`] arrived: the suffix is an
/// owned `Vec<u8>`, because it can come from `$SIMPLE_BACKUP_SUFFIX` and so
/// cannot be a borrow of argv. That is why [`Job`] holds a *reference* to this
/// rather than a copy of it — `cp` does the same, for the same field.
#[derive(Default, Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MvFlags {
    /// `-v`/`--verbose`: name every move on **stdout** as it happens. See
    /// [`announce`] for the three ways this is not the obvious feature.
    verbose: bool,
    /// `-i`, `-f` and `-n`, which are **one** field and not three, so the last
    /// one on the command line wins. See [`Interactive`] and [`abandon_move`].
    ///
    /// `-f` used to have no field at all here, on the reasoning that it "only
    /// suppresses a prompt this `mv` never raises". That reasoning was sound
    /// while there were no prompts and is wrong now twice over: `-f` after `-i`
    /// cancels it, and `-f` on its own suppresses the *unwritable-destination*
    /// question that [`abandon_move`] asks with no option given at all.
    interactive: Interactive,
    /// `-u` / `--update=older`: leave a destination that is **not older** than
    /// the source, and call that success. GNU's `x.update` (`copy.h:196`), read
    /// only by [`destination_is_older`].
    ///
    /// A separate field from [`MvFlags::interactive`] and not a fifth value of
    /// it, because upstream keeps them separate and `--update`'s three words
    /// write *both*: `none` is `interactive = AlwaysSkip` with `update` off,
    /// `older` is `update` on with `interactive` untouched, and `all` clears
    /// both. Folding them together would have to invent an ordering between
    /// "skip whatever is there" and "skip only what is newer", and the command
    /// line never expresses both at once.
    update: bool,
    /// `-b` / `--backup[=CONTROL]` / `-S SUFFIX`: move the destination aside
    /// before overwriting it, rather than destroying it.
    ///
    /// The policy and not a `bool`, because "make backups" and "make *which*
    /// backups" arrive from four places that do not agree — `-b`, `--backup=W`,
    /// `-S`, and `$VERSION_CONTROL` — and only [`backup::Backup`] holds the
    /// resolved answer. `Backup::disabled()` is the no-option value, which is
    /// what [`Default`] gives.
    ///
    /// This field does more than add a rename: it also *relaxes three
    /// refusals*. See [`refuse_overwrite_checks`] steps 4, 6 and 7 — under
    /// `--backup` a directory may replace a non-directory and vice versa,
    /// because the thing that would be destroyed is being kept.
    backup: backup::Backup,
    /// Whether descriptor 0 is a terminal, sampled once at startup rather than
    /// per operand.
    ///
    /// GNU's `x.stdin_tty`, set from `isatty (STDIN_FILENO)` in `mv.c:152` and
    /// read only by [`abandon_move`]. Sampled once because upstream samples it
    /// once, and because a `mv` whose stdin is closed part-way through a long
    /// move should not start behaving differently half-way down its operand
    /// list.
    stdin_tty: bool,
}

/// How the command line named its destination — GNU's `target_directory` and
/// `no_target_directory` (`mv.c:320-321`).
///
/// Two independent fields rather than one three-state enum, because **both can
/// be given at once** and that combination is a diagnostic of its own rather
/// than one of them winning. Collapsing them would have to pick a winner, and
/// every choice of winner silently obeys an option the user is being told is
/// contradictory.
///
/// It is separate from [`MvFlags`] because it is not a policy applied to each
/// move: it decides the *shape of the operand list*, once, before any move
/// happens.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Destination {
    /// `-t DIR` / `--target-directory=DIR`: the destination is named ahead of
    /// the operands, so every operand is a source — which is also the only
    /// shape in which one source and a directory is unambiguous.
    directory: Option<OsString>,
    /// `-T` / `--no-target-directory`: the last operand is a name to move
    /// *onto*, never a directory to move *into*.
    no_directory: bool,
    /// `--strip-trailing-slashes`: remove trailing `/` from the operands before
    /// they are moved. GNU's `remove_trailing_slashes` (`mv.c:319`).
    ///
    /// Here and not in [`MvFlags`] because it is not a policy applied to each
    /// move — it rewrites the operand list once, and it does so at a point that
    /// only [`move_all`] can see. See [`strip_operands`] for *which* operands,
    /// which is the whole subtlety of the option.
    ///
    /// What it buys is `mv symlink-to-dir/ into-a-directory`. A trailing slash
    /// makes the kernel resolve the symlink, so that command asks to move the
    /// *directory* and fails `ENOTDIR`; strip it and the symlink itself moves.
    /// Shells append the slash for you on tab-completion, so the option exists
    /// for scripts that cannot control how their argument was spelled.
    ///
    /// It does *not* buy `mv --strip-trailing-slashes symlink-to-dir/ newname`,
    /// where the destination does not exist — measured against GNU 9.4, that
    /// still fails `Not a directory`. The speculative rename runs before the
    /// strip and caches the `ENOTDIR` it got from the *unstripped* name, and
    /// nothing afterwards retries. The option therefore helps in exactly the
    /// three shapes that reach no speculative rename: a destination that is a
    /// directory, `-t`, and `-T`. See [`strip_operands`].
    strip_slashes: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Every operand, in order, and where they are going. Without `-t` the last
    /// operand is the destination; with it, every one is a source.
    Run(MvFlags, Destination, Vec<OsString>),
}

/// The parts of a run that every step below needs: what was asked for, and the
/// two streams the answer goes to.
///
/// It exists because `-v` gives `mv` a *second* output stream, and threading two
/// sinks plus a flags struct through [`move_all`] → [`move_one`] as separate
/// parameters puts both over `clippy::too_many_arguments`. `cp.rs`'s `Job` is
/// the same struct for the same reason; keeping the shape identical is what lets
/// the two files' move/copy cores stay readable side by side.
struct Job<'a, O: Write, E: Write> {
    /// Borrowed rather than held by value: [`MvFlags`] stopped being `Copy`
    /// when `-b`'s owned suffix joined it, and a `Job` built per operand would
    /// otherwise clone a heap allocation for every file on the command line.
    flags: &'a MvFlags,
    /// Where `--verbose` goes. **Not** where diagnostics go — see [`announce`].
    out: &'a mut O,
    err: &'a mut E,
    /// Where `-i`'s answer comes from. A trait object rather than stdin so that
    /// a test can put a canned reply behind a prompt without a terminal; see
    /// [`coreutils::yesno::Canned`].
    answers: &'a mut dyn Answers,
    /// GNU's one `src_to_dest` table (`cp-hash.c:45`): which inodes this command
    /// has already put somewhere, and where. One per command and not one per
    /// operand — that is the whole point of it, and it is why it lives on the
    /// `Job` rather than inside [`move_one`]. See the `earlier_file` block there.
    copied: &'a mut Copied,
    /// The process's file-mode creation mask, read once at startup.
    ///
    /// Not a flag — nothing on the command line sets it — but on the `Job` for
    /// the same reason `cp.rs` puts it there: it is an input to the shared copy
    /// engine, and [`copy::Opts`] is where the engine's inputs live. Upstream
    /// reaches it through a function-static cache (`cached_umask()`), which is a
    /// global because `copy.c` has nowhere better; we have the struct that is
    /// already threaded to every step that could want it.
    ///
    /// No step a *move* reaches actually reads it, and that is worth stating
    /// rather than exploiting: `settle_mode` consults the mask only in its
    /// `--no-preserve=mode` branch and in the settle-up subtraction, and
    /// `preserve_mode` — which `cp_option_init` sets unconditionally
    /// (`mv.c:136`) — returns before either. Carrying the real value anyway is
    /// what keeps that a fact about the flags rather than a dependency on it:
    /// were the short-circuit ever to stop holding, the engine would find the
    /// right mask rather than a zero that quietly widens every copy.
    umask: u32,
}

impl<O: Write, E: Write> Job<'_, O, E> {
    /// This job as the shared copy engine sees it.
    ///
    /// Every field is a constant, because `mv` has no option that changes any of
    /// them: `cp_option_init` (`mv.c:119`) sets them and mv's getopt writes none
    /// of them back. That is the whole reason `mv` can drive the same engine
    /// `cp` does without a single branch inside it that names a program — the
    /// two differ only in what they put in this struct.
    ///
    /// Written out one field per line with its citation rather than as a
    /// `Default`, because the point of the list is that it is *checkable*
    /// against upstream. A default would hide which of these are upstream's
    /// choices and which are Rust's zero values.
    fn run(&mut self) -> copy::Run<'_, E> {
        copy::Run {
            opts: mv_opts(self.umask),
            err: self.err,
        }
    }
}

/// `cp_option_init` (`mv.c:119`), as much of it as the copy engine reads.
///
/// A free function rather than a body inside [`Job::run`] so that the tests
/// which call [`copy_across_devices`] directly go through *this* list and not a
/// second one written beside it. A duplicated options list is a test that
/// passes against itself: it would keep passing after a change to the real one,
/// which is precisely the change a test of the preserve tail exists to catch.
fn mv_opts(umask: u32) -> copy::Opts {
    copy::Opts {
        prog: "mv",
        preserve_mode: true,              // mv.c:136
        preserve_timestamps: true,        // mv.c:137
        preserve_ownership: true,         // mv.c:134
        preserve_xattr: true,             // mv.c:145
        require_preserve: false,          // mv.c:143
        require_preserve_xattr: false,    // mv.c:146
        reduce_diagnostics: false,        // mv.c:141
        explicit_no_preserve_mode: false, // mv.c:138
        umask,
    }
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
            println!("mv (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(mut flags, dest, paths)) => {
            // GNU's `mv.c:152`. Sampled here rather than inside the check that
            // reads it, so that the answer is the one the process started with.
            flags.stdin_tty = stdfd::is_tty(0);
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut out = Stream::stdout();
            let mut err = Stream::stderr();
            let mut answers = StdinAnswers::default();
            // Built here and nowhere else: GNU's `src_to_dest` is a file-scope
            // hash in `copy.c` created once per process, which is why two
            // separate `mv` commands correctly produce two separate files where
            // one command producing two names produces one file.
            let mut copied = Copied::default();
            let earned = {
                let mut job = Job {
                    flags: &flags,
                    out: &mut out,
                    err: &mut err,
                    answers: &mut answers,
                    copied: &mut copied,
                    umask: coreutils::umask::current(),
                };
                if move_all(&mut job, &dest, &paths) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            };
            // `--verbose` is the only thing `mv` ever writes to stdout, and a
            // line of it that never arrived has to change the status the same
            // way a lost diagnostic does — otherwise `mv -v … | head -1`
            // reports success for output nobody received.
            stdfd::close_stdout("mv", out, earned)
        }
        Err(e) => {
            diag!("mv: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: mv [OPTION]... [-T] SOURCE DEST
  or:  mv [OPTION]... SOURCE... DIRECTORY
  or:  mv [OPTION]... -t DIRECTORY SOURCE...
Rename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.

      --backup[=CONTROL]  make a backup of each existing destination file
  -b                 like --backup but does not accept an argument
  -f, --force        do not prompt before overwriting
  -i, --interactive  prompt before overwrite
  -n, --no-clobber   do not overwrite an existing file
If you specify more than one of -i, -f, -n, only the final one takes effect.
      --strip-trailing-slashes  remove any trailing slashes from each SOURCE
                       argument
  -S, --suffix=SUFFIX  override the usual backup suffix
  -t, --target-directory=DIRECTORY  move all SOURCE arguments into DIRECTORY
  -T, --no-target-directory  treat DEST as a normal file
      --update[=UPDATE]  control which existing files are updated;
                       UPDATE={all,none,older(default)}.  See below
  -u                 equivalent to --update[=older]
  -v, --verbose      explain what is being done
      --help         display this help and exit
      --version      output version information and exit

UPDATE controls which existing files in the destination are replaced.
'all' is the default operation when an --update option is not specified,
and results in all existing files in the destination being replaced.
'none' is similar to the --no-clobber option, in that no files in the
destination are replaced, but also skipped files do not induce a failure.
'older' is the default operation when --update is specified, and results
in files being replaced if they're older than the corresponding source file.

The backup suffix is '~', unless set with --suffix or SIMPLE_BACKUP_SUFFIX.
The version control method may be selected via the --backup option or through
the VERSION_CONTROL environment variable.  Here are the values:

  none, off       never make backups (even if --backup is given)
  numbered, t     make numbered backups
  existing, nil   numbered if numbered backups exist, simple otherwise
  simple, never   always make simple backups

To move a file whose name starts with a '-', for example '-foo',
use one of these commands:
  mv -- -foo bar
  mv ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// GNU `mv`'s short options, in the string it hands `getopt_long`
/// (`mv.c:339`), colons included.
///
/// The colons are the part that matters here. `t:` is what makes `-t dir`
/// consume the following word, and it has to be declared even while `-t` is
/// refused: a parser that does not know `-t` takes a value treats `dir` as an
/// operand, so `mv -t dir file` would refuse the option *and* — had the refusal
/// been a warning rather than an error — have moved `file` onto `dir` as a
/// plain two-operand rename. Declaring the shape and refusing the option are
/// separate questions, and only the second is about what is implemented.
const SHORT_OPTIONS: &str = "bfint:uvS:TZ";

/// Parse `mv`'s argv into its operands.
///
/// Options and operands may be interleaved — `mv a -f b` is `mv a b` — which is
/// `getopt_long`'s default permuting behaviour and what [`getopt::Parser`] does.
///
/// This walks the shared parser rather than the hand-written scanner that used
/// to be here. The scanner could not express an option that takes a value at
/// all: it split every non-`--` word into bytes and looked each up, so `-t` had
/// no way to reach for the word after it. That was invisible while `-t` was
/// refused and would have been a wrong answer the moment it was not.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, a
/// long option given a value it does not take, or one denied a value it needs.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = MvFlags::default();
    let mut dest = Destination::default();
    let mut paths: Vec<OsString> = Vec::new();
    // The three inputs to [`MvFlags::backup`], kept apart until the loop ends
    // because they are settled in a fixed order that is not the order they are
    // typed in: whether to back up at all, then which shape, then the suffix.
    // Upstream carries the same three (`make_backups`, `version_control_string`,
    // `backup_suffix`) as locals of `main` for the same reason.
    let mut make_backups = false;
    let mut version_control: Option<OsString> = None;
    let mut backup_suffix: Option<OsString> = None;

    for item in MV.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // A lone `-` arrives here, not as an option: `mv` has no
            // standard-input operand for it to mean anything else.
            Opt::Operand(name) => paths.push(name.clone()),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // One field, three spellings, last one wins — including inside a
            // single cluster, since the parser hands a bundle over one byte at a
            // time. `mv -if` does not ask; `mv -fi` does. Assignment and not
            // `|=` for the same reason: `mv --force --interactive` asks.
            Opt::Short(b'f', _) | Opt::Long("force", _) => {
                flags.interactive = Interactive::AlwaysYes;
            }
            Opt::Short(b'i', _) | Opt::Long("interactive", _) => {
                flags.interactive = Interactive::AskUser;
            }
            Opt::Short(b'n', _) | Opt::Long("no-clobber", _) => {
                flags.interactive = Interactive::AlwaysNo;
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            // `mv.c:344`. One arm for both spellings because upstream has one:
            // `b` carries no colon in [`SHORT_OPTIONS`] and `--backup` is
            // `Takes::Optional`, so `value` is `None` for a bare `-b` and for a
            // bare `--backup` alike.
            //
            // The `if let` rather than a plain assignment is upstream's `if
            // (optarg)`, and it is what makes `mv --backup=numbered -b` stay
            // numbered: a later bare `-b` turns backups on again without
            // erasing the word an earlier one chose.
            Opt::Short(b'b', value) | Opt::Long("backup", value) => {
                make_backups = true;
                if let Some(word) = value {
                    version_control = Some(word);
                }
            }
            // `-S` sets `make_backups` **as well as** the suffix (`mv.c:405`),
            // which is the surprising half: `mv -S .bak a b` makes a backup
            // although `-b` was never given. Omitting that line would make `-S`
            // alone silently do nothing, and "silently do nothing" here means
            // the destination is destroyed by a command that asked for it to be
            // kept under another name.
            Opt::Short(b'S', value) | Opt::Long("suffix", value) => {
                // Unreachable: `S:` in [`SHORT_OPTIONS`] and `Takes::Required`
                // in [`LONG_OPTIONS`] both make the parser supply a value or
                // fail before this arm is reached.
                let Some(given) = value else {
                    return Err(MV.short_missing_argument(b'S'));
                };
                make_backups = true;
                backup_suffix = Some(given);
            }
            // `mv.c:375`. One arm for both spellings because upstream has one:
            // `u` carries no colon in [`SHORT_OPTIONS`] and `--update` is
            // `Takes::Optional`, so a `None` here is either a bare `-u` or a
            // bare `--update`, and GNU treats those identically.
            Opt::Short(b'u', value) | Opt::Long("update", value) => match value {
                None => flags.update = true,
                // **The guard is on the long form only**, and that asymmetry is
                // upstream's `else if (x->interactive != I_ALWAYS_NO)` with the
                // comment `/* -n takes precedence.  */`. It is observable, and
                // only because a *later* option can move `interactive` off
                // `AlwaysNo` again:
                //
                // ```text
                // mv -n --update=older -i a b    # update off — the guard ate it
                // mv -n -u -i a b                # update on  — no guard on -u
                // ```
                //
                // The two command lines differ in a spelling the --help text
                // calls equivalent. Measured against 9.4, not deduced.
                //
                // Note also *where* the guard sits: upstream's `XARGMATCH` is
                // inside the guarded block, so under `-n` the word is never
                // looked at and `mv -n --update=nosuchword a b` is accepted in
                // silence. That is not an oversight worth improving on — a
                // stricter check here would reject a command line GNU runs.
                Some(word) => {
                    if flags.interactive != Interactive::AlwaysNo {
                        match MV.argmatch(&os_bytes(&word), "--update", UPDATE_TYPES)? {
                            UpdateType::All => {
                                flags.update = false;
                                flags.interactive = Interactive::Unspecified;
                            }
                            UpdateType::None => {
                                flags.update = false;
                                flags.interactive = Interactive::AlwaysSkip;
                            }
                            UpdateType::Older => {
                                flags.update = true;
                                flags.interactive = Interactive::Unspecified;
                            }
                        }
                    }
                }
            },
            Opt::Short(b't', value) | Opt::Long("target-directory", value) => {
                // Refused here rather than at use, and refused without
                // comparing the two directories — GNU asks only whether one was
                // given already, so `mv -t d -t d a` fails as surely as
                // `-t d -t e` does. Measured, not assumed.
                //
                // A plain diagnostic with no "Try 'mv --help'" after it, because
                // upstream raises it with `error (EXIT_FAILURE, …)` and not
                // through `usage`.
                if dest.directory.is_some() {
                    return Err(MV.usage("multiple target directories specified".into()));
                }
                // Unreachable: `t:` in [`SHORT_OPTIONS`] and `Takes::Required`
                // in [`LONG_OPTIONS`] both make the parser supply a value or
                // fail before this arm is reached.
                let Some(dir) = value else {
                    return Err(MV.short_missing_argument(b't'));
                };
                dest.directory = Some(dir);
            }
            Opt::Short(b'T', _) | Opt::Long("no-target-directory", _) => {
                dest.no_directory = true;
            }
            Opt::Long("strip-trailing-slashes", _) => {
                dest.strip_slashes = true;
            }
            // GNU `mv`'s remaining options, refused by name. Reaching them
            // means the parser recognised the option and, for `-S`, has already
            // taken its value out of argv — which is what keeps the *rest* of
            // the line reading the way GNU reads it.
            Opt::Short(flag, _) => return Err(unimplemented_short(flag)),
            Opt::Long(name, _) => return Err(unimplemented_long(name)),
        }
    }

    // `mv.c:509`. The guard in the `-u` arm above catches only a `-n` that came
    // *earlier*; this catches one that came later, and it is the reason `-n`
    // beats `-u` in both orders even though neither of them writes the other's
    // field. Both halves are needed: drop this and `mv -u -n a b` would ask
    // `-u`'s question of a destination `-n` has already declined to touch.
    if flags.interactive == Interactive::AlwaysNo {
        flags.update = false;
    }

    // `mv.c:512`, and it must come *after* the clamp above because upstream's
    // does — not that the order is observable here, since neither writes what
    // the other reads, but because the two sit two lines apart in one function
    // and reordering them invites the assumption that they are independent.
    //
    // It is a real contradiction rather than a tidiness rule: `-n` says "leave
    // the destination exactly as it is" and `-b` says "move the destination
    // aside", so a command line with both has asked for the file to stay and to
    // go. Refused rather than resolved, because either resolution silently
    // ignores half of what was typed.
    //
    // Reachable through `-S` as well as `-b`, since `-S` sets `make_backups`:
    // `mv -n -S .bak a b` fails with this.
    //
    // `AlwaysSkip` is **not** `AlwaysNo`, so `mv --backup --update=none a b` is
    // legal and backs nothing up — `--update=none` skips before the backup
    // block is reached. That asymmetry is upstream's: the check names
    // `x.interactive == I_ALWAYS_NO` and `--update=none` sets `I_ALWAYS_SKIP`.
    //
    // `usage_referring` and not `usage`: upstream reaches this through
    // `usage (EXIT_FAILURE)` rather than through `error (EXIT_FAILURE, …)`, so
    // the sentence is followed by `Try 'mv --help' for more information.` —
    // unlike `multiple target directories specified` a few arms above, which
    // deliberately carries no referral.
    if make_backups && flags.interactive == Interactive::AlwaysNo {
        return Err(
            MV.usage_referring("options --backup and --no-clobber are mutually exclusive".into())
        );
    }

    // The type is asked for only when an option asked for backups; the suffix
    // is settled unconditionally, exactly as upstream does it (`mv.c:519-523`).
    // That asymmetry is load-bearing in one direction only: `$VERSION_CONTROL`
    // alone must never enable backups, while `$SIMPLE_BACKUP_SUFFIX` alone is
    // harmless because nothing reads the suffix unless backups are on.
    flags.backup = if make_backups {
        backup::Backup::new(
            backup::control(MV, version_control.as_deref())?,
            backup::suffix(backup_suffix.as_deref()),
        )
    } else {
        backup::Backup::disabled()
    };

    Ok(Request::Run(flags, dest, paths))
}

/// The diagnostic for an option that GNU `mv` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-n` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    MV.usage_referring(format!(
        "option -{} is not implemented by this mv",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    MV.usage_referring(format!("option '--{name}' is not implemented by this mv"))
}

// ----------------------------------------------------------------- moving ---

/// What the speculative rename left behind — GNU's `x.rename_errno`
/// (`copy.h:277`), whose three states drive everything below.
///
/// The tri-state is not an implementation detail that could be flattened into a
/// `Result`: which of the three it is decides *what is even checked*. `Done`
/// means the move already happened and nothing may be looked at again;
/// `Failed(EEXIST)` means something is in the way, which is where every refusal
/// lives; any other `Failed` is reported without ever consulting the
/// destination.
enum Renamed {
    /// GNU's `-1`. No attempt yet, so [`move_one`] makes it.
    NotTried,
    /// GNU's `0`. The source is at the destination already; there was nothing
    /// there to overwrite, so no question of overwriting arose.
    Done,
    /// A failed attempt, carrying the reason.
    Failed(io::Error),
}

/// Try to rename, but only onto a name that does not exist: GNU's
/// `renameatu (…, RENAME_NOREPLACE)` (`mv.c:466`).
///
/// The point of doing this *first*, before `mv` has decided whether the last
/// operand is a directory, is that the overwhelmingly common case — a rename
/// onto a free name — then costs one syscall and skips every check, and the
/// checks are only reached when there is something to check.
///
/// [`coreutils::rename::noreplace`] is the call, shared with [`backup`] because
/// both are saying "I checked that this name was free" and a plain `rename(2)`
/// cannot say it. This used to be a private copy that only ever emulated the
/// flag with an `lstat`, which kept gnulib's race after the kernel had stopped
/// having one; see that module.
///
/// [`backup`]: coreutils::backup
fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    coreutils::rename::noreplace(src, dst)
}

/// Is this the errno that means "the destination is already there"?
///
/// Compared as a *kind* rather than as a number because [`rename_noreplace`]
/// may synthesise it rather than receive it from the kernel — the emulated path
/// does — and the two must answer alike.
fn is_exists(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::AlreadyExists
}

/// Can this operand be used as a directory to move things into? gnulib's
/// `target_directory_operand` (`lib/targetdir.c`).
///
/// Upstream opens it `O_PATH | O_DIRECTORY` and keeps the descriptor for the
/// `*at` calls that follow; we have no such calls, so the question reduces to
/// the one the open answers. It *follows* symlinks, which is why
/// `mv a link-to-dir` puts `a` inside the directory.
///
/// # Errors
///
/// The failure the caller reports as `target 'x': …` — `ENOENT` when the operand
/// is absent (including a dangling symlink, since this follows), `ENOTDIR` when
/// it exists and is not a directory.
///
/// The `ENOTDIR` case is synthesised rather than observed: upstream gets it from
/// the `O_DIRECTORY` open, whereas the `metadata` call here *succeeds* and the
/// `is_dir` test is what fails. It is built from the [`io::ErrorKind`] and not
/// from the number 20, because that number is `ENOTDIR` only on a host where it
/// is an errno at all — on the Windows development host `from_raw_os_error(20)`
/// is a Win32 code and prints `The system cannot find the device specified.`
/// The kind is what [`strerror`] reads, and it yields `Not a directory` on both.
fn target_directory_operand(path: &Path) -> io::Result<()> {
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// The destinations already written by *this* command, as GNU's `dest_info`
/// (`copy.h:289`) — a set of `(name, file)` pairs, not just names.
///
/// It has to be pairs. The question it answers is not "did two operands share a
/// basename" but "is the thing sitting at that name the thing I just put
/// there": if the name held something else all along, overwriting it is an
/// ordinary overwrite and GNU performs it silently.
type DestInfo = std::collections::HashSet<(OsString, FileId)>;

/// Where `src` lands inside directory `dir`, and under what name.
///
/// GNU's `mv.c:540`: `file_name_concat (target_directory, last_component
/// (source), &dst_relname)`, followed by `strip_trailing_slashes (dst_relname)`.
/// [`split_entry`] already does the stripping, so the two halves of its answer
/// are the two halves of this one.
///
/// **The last component is appended verbatim, `.` and `..` included.** This is
/// the one place `mv` and `cp` genuinely differ: `cp` has an
/// `arg_base += STREQ (arg_base, "..")` bump (`cp.c:739`) and `mv` has no such
/// line, so `cp a/.. d` targets `d/a` while `mv a/.. d` targets `d/..`. Reading
/// that as "`mv` forgot" and adding the bump here would be wrong twice over: it
/// would move the wrong file, and it would do so silently, where the verbatim
/// name reliably fails `EEXIST` or `EBUSY` and says so.
fn target_in_directory(dir: &Path, src: &Path) -> (PathBuf, OsString) {
    let (_, base) = split_entry(src);
    (dir.join(&base), base)
}

/// `--strip-trailing-slashes` applied to the operands that are still operands.
///
/// GNU `mv.c:505`:
///
/// ```text
/// if (remove_trailing_slashes)
///   for (int i = 0; i < n_files; i++)
///     strip_trailing_slashes (file[i]);
/// ```
///
/// Two things about that loop decide everything this function has to get right,
/// and neither is visible in the option's one-line help text.
///
/// **It runs late.** Every operand-shape question — the missing-operand
/// diagnostic, `-T`'s extra operand, `-t`'s directory, the speculative
/// `renameatu (…, RENAME_NOREPLACE)`, and the probe asking whether the last
/// operand is a directory — is asked *before* it, on the unstripped words. All
/// four diagnostics therefore quote the slash the user typed, and are measured
/// doing so: `mv --strip-trailing-slashes -T a b c/` says `extra operand 'c/'`,
/// and `mv --strip-trailing-slashes a b c/` says `target 'c/': Not a directory`.
/// Only what happens *after* the shape is settled sees stripped names.
///
/// The speculative rename is the sharp one, because its errno outlives it:
/// `mv --strip-trailing-slashes symlink-to-dir/ newname` fails
/// `cannot move 'sym' to 'newname': Not a directory`, an `ENOTDIR` that could
/// only have come from renaming `sym/` — the name in the message is stripped
/// and the error in it is not. That is upstream's, and it is why this is not
/// simply done to `paths` on the way in.
///
/// **It runs over `n_files`, which the directory probe may already have
/// decremented.** So a destination that *is* a directory keeps its slashes and
/// one that is not loses them, and `-t`'s directory — which never was one of
/// `file[]` — always keeps them. That asymmetry is unobservable for the
/// directory cases (a trailing slash on a name that must be a directory changes
/// nothing) and observable for the other: `mv --strip-trailing-slashes -T a
/// symlink-to-dir/` replaces the *symlink*, where without the option it reports
/// that it cannot overwrite a directory with a non-directory.
///
/// Borrows when the option is off, so the common case allocates nothing and the
/// operands stay the exact bytes the kernel was handed. An operand with nothing
/// to strip is *cloned* rather than rebuilt from bytes for the same reason:
/// [`os_bytes`] is lossy on a Windows development host, and a name that this
/// option does not change should not be able to change anyway.
fn strip_operands<'a>(dest: &Destination, paths: &'a [OsString]) -> Cow<'a, [OsString]> {
    if !dest.strip_slashes {
        return Cow::Borrowed(paths);
    }
    Cow::Owned(
        paths
            .iter()
            .map(|p| {
                let bytes = os_bytes(p);
                let kept = strip_trailing_slashes(&bytes);
                if kept.len() == bytes.len() {
                    p.clone()
                } else {
                    os_from_bytes(kept)
                }
            })
            .collect(),
    )
}

/// Move every source onto the destination, reporting failures to `job.err`.
///
/// Returns `true` if every source was moved. Takes both streams through [`Job`]
/// rather than writing to `stderr` and `stdout` directly so the diagnostics —
/// the part of `mv` a caller actually sees when something goes wrong — and the
/// `--verbose` lines can be asserted on in tests. The old file had no test of
/// this path at all, which is how bugs 2–4 in the module docs survived.
///
/// A failure on one source does not stop the others: `mv a b c dir/` with `b`
/// unmovable still moves `a` and `c`, and exits 1.
///
/// This is where [`Destination`] is resolved into one of three shapes — every
/// operand into `-t`'s directory, one operand onto `-T`'s name, or the trailing
/// operand deciding between the two. The order of the checks that pick between
/// them is GNU's and is observable at every step; see the comments inline.
///
/// The shape follows GNU's `main` (`mv.c:427-550`), and the order is
/// load-bearing rather than stylistic — see [`Renamed`].
fn move_all<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    dest: &Destination,
    paths: &[OsString],
) -> bool {
    // GNU's `n_files <= !target_directory` (`mv.c:427`). With `-t` the
    // destination came from the option, so *one* operand is a whole command;
    // without it the last operand is the destination and two are needed. Zero
    // and one are distinct diagnostics, as in GNU — "missing operand" alone left
    // the user to work out *which*.
    if paths.len() <= usize::from(dest.directory.is_none()) {
        let message = match paths.first() {
            None => "missing file operand".to_string(),
            Some(first) => format!(
                "missing destination file operand after {}",
                quoteaf_os(first)
            ),
        };
        let _ = writeln!(job.err, "mv: {}", MV.usage_referring(message));
        return false;
    }

    // Both `-T` refusals come before `-t`'s directory is so much as stat'd,
    // which is GNU's order and is observable: `mv -T -t nosuchdir a b` reports
    // the combination and not the missing directory.
    if dest.no_directory {
        if dest.directory.is_some() {
            let _ = writeln!(
                job.err,
                "mv: cannot combine --target-directory (-t) and --no-target-directory (-T)"
            );
            return false;
        }
        // `-T` says the destination is exactly one name, so a third operand is
        // not a third source — there is nowhere for it to go.
        if let Some(extra) = paths.get(2) {
            let _ = writeln!(
                job.err,
                "mv: {}",
                MV.usage_referring(format!("extra operand {}", quoteaf_os(extra)))
            );
            return false;
        }
    }

    // `-t`: every operand is a source, and the directory is checked once, here.
    // The failure names it as a *target directory*, which is a different
    // sentence from the trailing operand's bare `target` below — the user named
    // this one as a directory, so being told it is not one is the whole answer.
    if let Some(dir) = &dest.directory {
        if let Err(e) = target_directory_operand(Path::new(dir)) {
            let why = strerror(&e);
            let _ = writeln!(job.err, "mv: target directory {}: {why}", quoteaf_os(dir));
            return false;
        }
        // Every operand is a source under `-t`, so every operand is stripped —
        // and the directory is not, because it came from the option and was
        // never one of GNU's `file[]`. Both halves are unobservable here (the
        // check above has already established it is a directory, and a trailing
        // slash on a directory name changes nothing), so this matches upstream
        // by construction rather than by measurement.
        let sources = strip_operands(dest, paths);
        return move_into_directory(job, Path::new(dir), &sources);
    }

    // Unreachable: the operand count was checked above.
    let Some((dest_operand, sources)) = paths.split_last() else {
        return false;
    };
    let last = Path::new(dest_operand);

    // `-T`: the destination is a name to move *onto*, so it is never asked
    // whether it is a directory — that question is the whole of what `-T`
    // switches off, and `mv -T file dir` therefore reaches the sentence about
    // overwriting a directory rather than putting `file` inside it.
    //
    // `Renamed::NotTried` and not the speculative attempt below, matching
    // upstream, whose `renameatu (…, RENAME_NOREPLACE)` sits in the branch
    // neither option reaches (`mv.c:501`). The attempt is not skipped so much as
    // moved: [`move_one`] makes it on `NotTried`, with the same arguments and
    // the same answer. What it does not do here is come *before* the
    // last-operand-is-a-directory probe, because under `-T` there is no such
    // probe for it to come before.
    if dest.no_directory {
        // `-T` leaves `n_files` at two and `target_directory` unset, so *both*
        // operands are stripped — the destination included, which is the one
        // place stripping the destination is observable:
        // `mv --strip-trailing-slashes -T a symlink-to-dir/` replaces the
        // symlink, where without the option the slash resolves it and the move
        // is refused as overwriting a directory with a non-directory.
        let stripped = strip_operands(dest, paths);
        // Unreachable: `split_last` above proves there are at least two.
        let (Some(src), Some(target)) = (stripped.first(), stripped.get(1)) else {
            return false;
        };
        return move_one(
            job,
            Path::new(src),
            Path::new(target),
            target,
            Renamed::NotTried,
            true,
            &mut None,
        );
    }

    let mut state = if sources.len() == 1 {
        match rename_noreplace(Path::new(&sources[0]), last) {
            Ok(()) => Renamed::Done,
            Err(e) => Renamed::Failed(e),
        }
    } else {
        Renamed::NotTried
    };

    // Only now — and only if that did not already settle it — is the last
    // operand asked whether it is a directory.
    let mut into: Option<&Path> = None;
    if !matches!(state, Renamed::Done) {
        match target_directory_operand(last) {
            Ok(()) => {
                state = Renamed::NotTried;
                into = Some(last);
            }
            Err(e) => {
                // With two operands the last one is simply the new name, and
                // not being a directory is unremarkable. With three or more it
                // *had* to be a directory, and this is fatal for the whole
                // command rather than for one source: GNU's
                // `error (EXIT_FAILURE, …)` at `mv.c:495`.
                if sources.len() > 1 {
                    let why = strerror(&e);
                    let _ = writeln!(job.err, "mv: target {}: {why}", quoteaf_os(dest_operand));
                    return false;
                }
            }
        }
    }

    // Only now, with the shape settled, does `--strip-trailing-slashes` act —
    // and on the operand list as it stands *after* the probe above, which is
    // GNU's `n_files--`. See [`strip_operands`]: everything up to this line has
    // read, and quoted, the words the user typed.
    let stripped = strip_operands(dest, paths);

    let Some(dir) = into else {
        // Two operands, last operand not a directory: one move, to that name.
        // Both are stripped, because the destination is still an operand.
        //
        // `state` is carried across the strip deliberately. It holds the errno
        // of a speculative rename made on the *unstripped* pair, and upstream
        // reports that errno against the stripped names —
        // `mv --strip-trailing-slashes symlink-to-dir/ new` says
        // `cannot move 'sym' to 'new': Not a directory`, an `ENOTDIR` no
        // rename of `sym` could have produced.
        let (Some(src), Some(target)) = (stripped.first(), stripped.get(1)) else {
            return false;
        };
        return move_one(
            job,
            Path::new(src),
            Path::new(target),
            target,
            state,
            true,
            &mut None,
        );
    };

    // The destination *is* a directory, so `n_files--` took it out of the loop
    // and only the sources are stripped. `dir` is still the unstripped word,
    // which is upstream's `target_directory = lastfile` — a pointer taken
    // before the strip, and to a name the strip would not have reached anyway.
    let Some(stripped_sources) = stripped.split_last().map(|(_, rest)| rest) else {
        return false;
    };
    move_into_directory(job, dir, stripped_sources)
}

/// Move every source in `sources` into `dir`, which the caller has already
/// established is a directory.
///
/// Both spellings of "the destination is a directory" end here — the trailing
/// operand and `-t` — which is the point of the split. `-t`'s only difference
/// from a trailing directory is *which operands are sources*; once that is
/// settled, the collision bookkeeping, the per-source diagnostics and the
/// exit status are the same code and not a second copy of it.
///
/// Returns `true` if every source moved. One failure does not stop the rest:
/// `mv a b c dir/` with `b` unmovable still moves `a` and `c`, and exits 1.
fn move_into_directory<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    dir: &Path,
    sources: &[OsString],
) -> bool {
    // The set is built only when it can matter — GNU's comment at `mv.c:529`:
    // "the problem it is used to detect can arise only if there are two or more
    // files to move."
    let mut seen: Option<DestInfo> = (sources.len() >= 2).then(DestInfo::default);

    let mut ok = true;
    for (i, src) in sources.iter().enumerate() {
        let src_path = Path::new(src);
        let (target, base) = target_in_directory(dir, src_path);
        // The last operand is exempt from being recorded, because nothing that
        // follows could collide with it (`copy.c:2778`).
        let last_file = i.saturating_add(1) == sources.len();
        if !move_one(
            job,
            src_path,
            &target,
            &base,
            Renamed::NotTried,
            last_file,
            &mut seen,
        ) {
            ok = false;
        }
    }
    ok
}

/// Move one source to one already-computed target: GNU's `copy_internal`
/// reduced to the options this `mv` has.
///
/// `relname` is the target's name *within the destination directory*, which is
/// the key [`DestInfo`] is built on; with two operands it is the whole
/// destination operand, and is then never consulted because `seen` is `None`.
///
/// Returns `false` if this source should count against the exit status.
#[allow(clippy::too_many_lines)]
fn move_one<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    src: &Path,
    target: &Path,
    relname: &OsString,
    state: Renamed,
    last_file: bool,
    seen: &mut Option<DestInfo>,
) -> bool {
    let mut failure = match state {
        // Already moved, and with `last_file` the recording is skipped too, so
        // there is nothing left to do. This is the common case.
        Renamed::Done => {
            // No backup argument on these two: both are paths on which the
            // destination did not exist, so there was nothing to move aside.
            announce(job, "renamed", src, target, None);
            return record_move(target, relname, last_file, seen);
        }
        Renamed::NotTried => match rename_noreplace(src, target) {
            Ok(()) => {
                announce(job, "renamed", src, target, None);
                return record_move(target, relname, last_file, seen);
            }
            Err(e) => e,
        },
        Renamed::Failed(e) => e,
    };

    // The source is stat'd only now, which is why a missing source is reported
    // as `cannot stat` rather than as a rename failure. `symlink_metadata`, not
    // `exists`/`is_dir`: `mv` moves a symlink as itself, whatever it points at
    // — including nothing.
    let src_meta = match fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(e) => {
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(job.err, "mv: cannot stat {}: {why}", quoteaf_os(src));
            return false;
        }
    };

    // Whatever the rename said, a destination that exists makes this the
    // "something is in the way" case (`copy.c:2322`) — so `mv a/. d`, which
    // fails `EBUSY`, is examined as an overwrite and only *then* fails `EBUSY`
    // for real.
    //
    // A stat that fails for a reason *other* than "not there" ends the move
    // right here, naming the destination and nothing else (`copy.c:2330`):
    // `mv f d/`, where `d` is a regular file, says `cannot stat 'd/': Not a
    // directory` rather than `cannot move 'f' to 'd/': Not a directory`. The
    // difference is not cosmetic. `cannot move A to B` says a rename was tried
    // and refused; here none was — the errno in `failure` came from the
    // *speculative* rename in [`move_all`], possibly on a differently-spelled
    // pair (see [`strip_operands`]), and the thing that actually just went
    // wrong is this stat. Discarding its error with `.ok()` reported the older,
    // less relevant one and attributed it to an operation that never happened.
    //
    // Every errno but `ENOENT` lands here for this utility. Upstream lets
    // `ELOOP` through when `unlink_dest_after_failed_open` is set, so that the
    // destination can be unlinked later — and `mv` sets it false (`mv.c:128`),
    // which leaves `ENOENT` as the only way past.
    let dst_meta = match fs::symlink_metadata(target) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "mv: cannot stat {}: {why}", quoteaf_os(target));
            return false;
        }
    };
    if dst_meta.is_some() {
        failure = io::Error::from(io::ErrorKind::AlreadyExists);
    }

    // Where `-b` put the destination, if it put it anywhere — GNU's
    // `dst_backup`. `None` means three different things that all lead to the
    // same place: no `-b` was given, `-b` was given but there was nothing at
    // the destination to move aside, or the source's last component was `.` or
    // `..`. Only the *rename* paths below read it, to name it in `-v`'s line
    // and to put it back if the move then fails.
    let mut moved_aside: Option<PathBuf> = None;

    // The refusals are asked only of a destination that is actually there —
    // there is nothing to refuse to overwrite otherwise. So is the backup:
    // upstream's block is inside the same `if (dst_exists)`, which is why `-b`
    // onto a free name makes no `~` file.
    if let Some(dst_meta) = &dst_meta {
        match refuse_overwrite_checks(src, &src_meta, target, dst_meta, relname, seen, job) {
            Verdict::Proceed => {}
            Verdict::Refused => return false,
            // Deliberately *not* [`record_move`]: upstream's `skip:` label
            // returns before `record_file` (`copy.c:2445`), so a destination
            // that was left alone is not one this command line "just created"
            // and the later `will not overwrite just-created` cannot fire on
            // it. `mv --update=none a/f b/f dir`, with `dir/f` already present,
            // therefore skips twice and exits 0 rather than refusing the
            // second.
            Verdict::Skipped => return true,
        }
        match make_backup(src, &src_meta, target, job) {
            Ok(name) => moved_aside = name,
            Err(()) => return false,
        }
    }

    // GNU's `earlier_file` block (`copy.c:2662`): has this command already put
    // *this inode* somewhere? If it has, the second name for it becomes a hard
    // link to where the first one landed rather than a second file, because a
    // rename would have kept the two names together and `mv` promises to be
    // indistinguishable from a rename — `cp_option_init` (`mv.c:119`) sets
    // `preserve_links` (`mv.c:135`) unconditionally, with no option to turn it
    // off.
    //
    // # Why it is *here*, and not down in the cross-device fallback
    //
    // The obvious place would be beside the copy, since a copy is the only thing
    // that can produce a second file where a rename would not have. Upstream
    // puts it before the rename that is allowed to replace, and the difference
    // is measurable rather than stylistic. With `d/a` and `d/b` already present,
    // GNU 9.4's `mv -v a b d` on one filesystem — `a` and `b` two names for one
    // inode — prints:
    //
    // ```text
    // renamed 'a' -> 'd/a'
    // removed 'd/b'
    // removed 'b'
    // ```
    //
    // The second operand is *linked and then unlinked*, not renamed. The tree it
    // leaves is the same either way, which is exactly why the placement has to be
    // copied rather than reasoned about: only the `-v` transcript can tell, and
    // `scripts/mv-diff.sh` compares it byte-for-byte.
    //
    // # The two ways in
    //
    // GNU asks the table differently depending on the link count, and it is not
    // an optimisation:
    //
    // * **`st_nlink > 1`** — `remember_copied`, which both looks up and records.
    //   This is the source that *has* another name, so a later operand may be it.
    // * **`st_nlink == 1`** — a bare lookup (`copy.c:2672`). This arm is the one
    //   that is easy to leave out and fatal to: by the time the last of a set of
    //   links is reached, the earlier ones have been removed and its count is
    //   back down to 1. A rule spelled "only when the count is above one" would
    //   never fire on the operand that needs it most.
    //
    // Nothing is asked when the rename *succeeded* (`copy.c:2663`), and this
    // function has already returned in that case.
    //
    // Directories are left out. Upstream's first arm handles them under
    // `x->recursive` and produces `warning: source directory specified more than
    // once`; this `mv` refuses a cross-device directory move outright, so the
    // arm has nothing to protect yet. It belongs with the recursive fallback —
    // `known-issues.md` → `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`.
    let src_id = if src_meta.is_dir() {
        None
    } else {
        file_id(src, &src_meta)
    };
    if let Some(id) = &src_id {
        let earlier = if nlink(&src_meta) > 1 {
            job.copied.remember(id, target)
        } else {
            job.copied.lookup(id).map(Path::to_path_buf)
        };
        if let Some(earlier) = earlier {
            return link_to_earlier(job, &earlier, src, target, moved_aside.as_deref());
        }
    }

    // Now the real rename, the one allowed to replace what is there. Keyed on
    // the errno rather than on `dst_meta`, which is `copy.c:2757` exactly and
    // is not the same condition: between the speculative rename above and the
    // stat, something else can *remove* the destination. GNU retries and
    // succeeds; reporting `File exists` for a name that is now free would be
    // wrong. (When `dst_meta` is `Some` the assignment above has already made
    // this true, so the ordinary overwrite still passes through here.)
    if is_exists(&failure) {
        match fs::rename(src, target) {
            Ok(()) => {
                announce(job, "renamed", src, target, moved_aside.as_deref());
                return record_move(target, relname, last_file, seen);
            }
            Err(e) => failure = e,
        }
    }

    // A directory asked to become a subdirectory of itself. GNU keys on this
    // one errno and says so is fragile (`copy.c:2798`); there is no better
    // signal, and the alternative is the unhelpfully bare `Invalid argument`.
    if is_subdirectory_of_itself(&failure) {
        let _ = writeln!(
            job.err,
            "mv: cannot move {} to a subdirectory of itself, {}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        // No [`Copied::forget`] here, and upstream says why in as many words:
        // "there is no need to call forget_created here, (compare with the other
        // calls in this file) since the destination directory didn't exist
        // before" (`copy.c:2807`). This `mv` cannot even reach it with an entry
        // to forget — the table takes no directories — but the omission is
        // deliberate rather than an oversight, which is why it is written down.
        return false;
    }

    if !is_cross_device(&failure) {
        let why = strerror(&failure);
        // When the destination is what went wrong, naming the source as well
        // "is more likely to confuse the user than be helpful"
        // (`copy.c:2851`).
        if blames_the_destination(&failure) {
            let _ = writeln!(
                job.err,
                "mv: cannot overwrite {}: {why}",
                quoteaf_os(target)
            );
        } else {
            let _ = writeln!(
                job.err,
                "mv: cannot move {} to {}: {why}",
                quoteaf_os(src),
                quoteaf_os(target)
            );
        }
        // The destination was never written, so the entry the block above may
        // have just added would make a *later* link point at a name that does
        // not exist. GNU's `forget_created` at `copy.c:2865`, and it is
        // unconditional there for the same reason it can be here: on the arm
        // that only looked the inode up, there is nothing recorded to remove.
        forget(job, src_id.as_ref());
        return false;
    }

    // The one shape this fallback cannot do, asked **before** the destination is
    // cleared and not inside [`copy_across_devices`] with the rest of the kind
    // analysis. GNU's order is clear-then-copy, and it can afford that because
    // its copy handles a directory; ours does not, so clearing first would
    // delete a destination that this command is then going to refuse to
    // replace — losing a file to a move that did not happen, which is worse
    // than either the refusal or the move.
    //
    // No `copied` line precedes it, matching GNU's `!S_ISDIR (src_mode)` guard
    // and reading correctly besides: announcing a copy about to be declined
    // would be a lie rather than an oddity.
    if src_meta.is_dir() {
        return give_up_cross_device(
            job,
            &no_directories(src, target),
            target,
            moved_aside.as_deref(),
        );
    }

    // Clear the destination, so that the copy standing in for the rename ends
    // with a *new* file at that name rather than the old one rewritten. GNU
    // says why in as many words — "remove any existing destination file so that
    // a cross-device `mv` acts as if it were really using the rename syscall"
    // (`copy.c:2869`) — and the difference is not bookkeeping. Written through
    // instead, the destination keeps its inode, and with it its mode, its owner
    // and *its other hard links*: `mv /other/fs/f g`, where `g` is one of a
    // linked pair, silently rewrote the pair's other name too, and `g` came out
    // wearing a mode the source never had. A rename does none of that.
    //
    // Not conditional on the `dst_meta` taken further up: that stat and this
    // unlink are two syscalls with a rename attempt between them, and the name
    // may have been freed — or taken — since. `ENOENT` is therefore the
    // ordinary answer rather than a failure, and it is the *only* one excused;
    // GNU spells the test `errno != ENOENT` exactly.
    if let Err(e) = clear_destination(target) {
        let why = strerror(&e);
        let _ = writeln!(
            job.err,
            "mv: inter-device move failed: {} to {}; unable to remove target: {why}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        // No `un_backup` here, deliberately, and that is upstream's shape:
        // `copy.c:2884` returns straight out rather than jumping to its
        // `un_backup` label. So a `-b` whose backup was made and whose target
        // could not then be cleared leaves the backup standing with no
        // destination — the same odd-but-measured outcome the rename failures
        // above produce, for the same reason.
        //
        // The table *is* corrected, though (`copy.c:2883`): un-backing-up and
        // forgetting are two different repairs and upstream skips only the first
        // one here.
        forget(job, src_id.as_ref());
        return false;
    }

    // Announced *before* the copy is attempted, which is upstream's order and
    // not an accident of this file's shape: `copy.c:2887` prints `copied` in the
    // block that clears the destination, and only then falls through to the copy
    // itself. So a cross-device move whose copy fails still prints the line —
    // measured, `mv -v` of an unreadable file onto another filesystem prints
    // `copied 'u' -> '…'` on stdout, `mv: cannot open 'u' for reading:
    // Permission denied` on stderr, and exits 1. It looks like a bug and reads
    // like one; it is what the reference does, and `scripts/mv-diff.sh` compares
    // both streams byte-for-byte, so "fixing" it here would turn a passing case
    // red.
    announce(job, "copied", src, target, moved_aside.as_deref());

    if let Err(failure) = copy_across_devices(src, target, &src_meta, &mut job.run()) {
        // Upstream's `un_backup:` label forgets too, guarded by `earlier_file ==
        // nullptr` (`copy.c:3361`) — "unless we've just failed to create a hard
        // link", because *that* failure leaves the earlier entry legitimately
        // pointing at a destination that does exist. That guard is expressed here
        // by [`link_to_earlier`] having its own exit, so this call site is
        // unconditionally the "we recorded and then failed" one.
        forget(job, src_id.as_ref());
        return give_up_cross_device(job, &failure, target, moved_aside.as_deref());
    }
    // The second line of the pair, and it comes from somewhere else entirely in
    // GNU: `mv.c:238` hands the source to `rm()` with `rm_options.verbose` set,
    // and it is `remove.c:400` that prints it. That is why the wording is
    // `removed 'src'` with no arrow and no destination — it is `rm -v`'s
    // sentence, not `mv`'s. Reached only on success, because `do_move` only
    // calls `rm()` when `copy` returned true.
    announce_removed(job, src);
    record_move(target, relname, last_file, seen)
}

/// GNU's `emit_verbose` (`copy.c:2082`) with the verb its callers prefix —
/// `renamed` for a move that `rename(2)` performed, `copied` for the
/// cross-device fallback.
///
/// Three things about it are not what the obvious implementation would do:
///
/// * **It goes to stdout, not stderr.** `emit_verbose` is a `printf`. So
///   `mv -v a b > log` captures the line and `mv -v a b 2>/dev/null` does not
///   silence it — the reverse of what a diagnostic does. That is also why
///   [`run_main`] routes stdout through [`stdfd::close_stdout`]: with `-v` this
///   utility finally *has* stdout output whose loss must change the status.
/// * **Both names are quoted, in one style.** GNU writes `quoteaf_n (0, src)`
///   and `quoteaf_n (1, dst)` — two slots of the same style, not two styles — so
///   `mv -v 'a b' c` prints `'a b' -> c` and the reader can tell a space *in* a
///   name from the space *between* names.
/// * **There is no flush.** The line is buffered like any other stdout write and
///   leaves through [`Stream`]'s close, so `mv -v` into a pipe writes in blocks
///   rather than a syscall per file.
///
/// `backup` names the file `-b` moved out of the way, and appends
/// ` (backup: 'b~')` when there is one. It is a third *slot* of the same style
/// rather than a separate sentence, so `mv -vb a b` prints one line:
/// `renamed 'a' -> 'b' (backup: 'b~')`. The quoting differs by one letter from
/// the other two — upstream uses `quoteaf` here and `quoteaf_n` above — and that
/// distinction has no visible effect, since `quoteaf_n`'s slot number only
/// selects which of the reusable buffers the string is built in.
fn announce<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    verb: &str,
    src: &Path,
    dst: &Path,
    backup: Option<&Path>,
) {
    if !job.flags.verbose {
        return;
    }
    let _ = write!(job.out, "{verb} {} -> {}", quoteaf_os(src), quoteaf_os(dst));
    if let Some(name) = backup {
        let _ = write!(job.out, " (backup: {})", quoteaf_os(name));
    }
    let _ = writeln!(job.out);
}

/// `rm -v`'s line, printed by `mv` for the source it removes after a
/// cross-device copy (`remove.c:400`, reached through `mv.c:238`).
///
/// Separate from [`announce`] because it is a different sentence with a
/// different shape — one name, no arrow — and because upstream's is
/// `removed directory %s` for a directory. This `mv` cannot reach that case: it
/// refuses cross-device directory moves outright, so the only file it ever
/// removes here is a non-directory.
fn announce_removed<O: Write, E: Write>(job: &mut Job<'_, O, E>, src: &Path) {
    if !job.flags.verbose {
        return;
    }
    let _ = writeln!(job.out, "removed {}", quoteaf_os(src));
}

/// GNU's `forget_created`: drop this source's entry from the table because the
/// destination it would have named was not created after all.
///
/// A one-line wrapper so that the three call sites read as the three
/// `forget_created` calls they are, and so that "no id" — a directory, or a
/// stat the portable arm could not turn into an inode number — is handled once
/// rather than three times.
fn forget<O: Write, E: Write>(job: &mut Job<'_, O, E>, id: Option<&FileId>) {
    if let Some(id) = id {
        job.copied.forget(id);
    }
}

/// The source names an inode this command has already placed at `earlier`: link
/// the two destinations together instead of producing a second file, then remove
/// the source the way a successful move does.
///
/// GNU's non-directory arm of the `earlier_file` block (`copy.c:2744`), which is
/// three lines — `create_hard_link` or `goto un_backup`, then `return true` —
/// plus what `mv.c` does with that `true`.
///
/// # What is *not* printed
///
/// No `renamed` and no `copied` line, and that is structural rather than a
/// choice: upstream returns from `copy_internal` here, above the block that
/// prints either of them. `-v` still says two things, from two other places —
/// `removed 'dst'` out of [`hardlink::force_link`] when the destination it took
/// was occupied, and `removed 'src'` out of the `rm` below. Measured against GNU
/// 9.4, `mv -v a b d` with `a`/`b` linked and `d/a`/`d/b` present prints exactly
/// `renamed 'a' -> 'd/a'` / `removed 'd/b'` / `removed 'b'`.
///
/// # Why the failed removal does not put a backup back
///
/// [`give_up_cross_device`] exists because upstream's copying machinery jumps to
/// its `un_backup` label from eleven places. The removal is not one of them: it
/// happens in `do_move` (`mv.c:238`) *after* `copy()` has returned, so a source
/// that could not be removed sets the exit status and leaves the destination —
/// and any `-b` backup — exactly where they are. The link failure above it, by
/// contrast, *is* an `un_backup` jump.
///
/// [`Copied::forget`] is likewise not called on either failure. On the link
/// failure upstream's guard `if (earlier_file == nullptr)` skips it (`copy.c:3361`),
/// because the entry names a destination that was created and is still there —
/// it was this *second* name that could not be made. On the removal failure the
/// destination is complete, so there is nothing to forget either.
fn link_to_earlier<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    earlier: &Path,
    src: &Path,
    target: &Path,
    moved_aside: Option<&Path>,
) -> bool {
    if !hardlink::force_link(
        "mv",
        earlier,
        target,
        job.flags.verbose,
        &mut *job.out,
        &mut *job.err,
    ) {
        backup::un_backup(
            "mv",
            moved_aside,
            target,
            job.flags.verbose,
            &mut *job.out,
            &mut *job.err,
        );
        return false;
    }
    if let Err(failure) = remove_source(src) {
        let why = strerror(&failure.err);
        let what = &failure.what;
        let _ = writeln!(job.err, "mv: {what}: {why}");
        return false;
    }
    announce_removed(job, src);
    true
}

/// Note that the file just moved now sits at `relname`, so a later source that
/// lands on the same name can be told apart from an ordinary overwrite.
///
/// # Why this stats the destination and not the source
///
/// The source is *gone*: the rename that this is recording the success of has
/// just moved it away, so there is nothing left at that name to stat. GNU stats
/// the destination for exactly this reason — `copy.c:2246` picks the name to
/// stat with `rename_errno == 0 ? dst_name : src_name`, and the variable it
/// fills is called `src_sb` only because the *other* branch fills it from the
/// source. A rename does not change a file's device or inode, so the two are the
/// same identity, and only one of them is still readable.
///
/// Getting this backwards is silent rather than loud, which is what makes it
/// worth a comment: the stat simply fails, the set stays empty, and the
/// just-created check it exists to feed never fires. `mv one/same two/same dir`
/// then overwrites `dir/same` and reports success — two files in, one file out.
///
/// Always returns `true`: it is called only on success paths, and a set that
/// could not be updated costs the next source its refusal but never invents one.
fn record_move(
    target: &Path,
    relname: &OsString,
    last_file: bool,
    seen: &mut Option<DestInfo>,
) -> bool {
    // The last source is exempt: nothing follows it that could collide
    // (`copy.c:2778`), and GNU does not even take the stat.
    if last_file {
        return true;
    }
    if let Some(set) = seen {
        // `symlink_metadata`: `mv` is `DEREF_NEVER`, so a moved symlink is
        // recorded as itself rather than as whatever it points at.
        if let Ok(meta) = fs::symlink_metadata(target)
            && let Some(id) = file_id(target, &meta)
        {
            set.insert((relname.clone(), id));
        }
    }
    true
}

/// What the checks below decided about a destination that is already there.
///
/// Three outcomes and not a `bool`, because two of GNU's paths leave the
/// destination alone and they disagree about the exit status. Upstream carries
/// this as two locals — `skipped` and `return_val` (`copy.c:2341`) — and the
/// second is written `return_val = x->interactive == I_ALWAYS_SKIP`, which is
/// exactly the distinction this enum names.
///
/// It was a `bool` until `--update` arrived, and the reason it could be is that
/// every refusal `mv` had was a *failure*. `--update=none` and `--update=older`
/// are the first two that are not.
#[derive(PartialEq, Eq, Debug)]
enum Verdict {
    /// Nothing stands in the way; go on to the rename.
    Proceed,
    /// Reported, and this operand counts against the exit status.
    Refused,
    /// Left alone on purpose, silently, and the command still succeeds.
    Skipped,
}

/// The refusals that stand between "something is at the destination" and the
/// rename that would replace it.
///
/// The order is GNU's, and it is observable: a request that trips two of these
/// gets the first one's wording. Two of the orderings look wrong until measured
/// against 9.4, and both are pinned by tests:
///
/// * `-n` and `-i` come **before** the directory checks, not after. `mv -n dir
///   file` prints `not replacing 'file'` rather than `cannot overwrite
///   non-directory 'file' with directory 'dir'`, and `mv -i dir file` *asks*.
///   Upstream's `abandon_move` block is at `copy.c:2409` and the directory
///   sentences begin at 2455.
/// * `-n` comes **before** the same-file check too, by being the reason that
///   check is skipped — see step 1. So `mv -n s f`, where `s` is a symlink to
///   `f`, prints `not replacing 'f'` rather than `'s' and 'f' are the same
///   file`.
/// * `-u` comes **between** the two: after the same-file check and before
///   `abandon_move`. So `mv -u f l` on a hard-link pair still says `are the
///   same file` — the mtimes are equal, so `-u` would have skipped it, but it
///   never gets the chance.
///
/// Unlike `cp`'s, none of this exempts a **directory source**: `cp`'s block is
/// guarded by `! S_ISDIR (src_mode)` because `cp -r` descends and asks about the
/// files inside, while `mv` renames the tree in one operation and so has one
/// question to put about it. Step 2 is the exception, and it is upstream's:
/// `x->update && !S_ISDIR (src_mode)` exempts a directory source there, so
/// `mv -u dir existing-dir` is not skipped for being no newer.
fn refuse_overwrite_checks<O: Write, E: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    target: &Path,
    dst_meta: &fs::Metadata,
    relname: &OsString,
    seen: &Option<DestInfo>,
    job: &mut Job<'_, O, E>,
) -> Verdict {
    // 1. Is the destination the source? (`copy.c:2345`)
    //
    //    Skipped entirely under `-n` and `--update=none`, which is upstream's
    //    `x->interactive != I_ALWAYS_NO && x->interactive != I_ALWAYS_SKIP`
    //    guard on the call and not an optimisation: the two produce different
    //    sentences for the same command line, and `-n`'s is the one GNU prints.
    //    Measured — `mv -n f l` on a hard link pair says `not replacing 'l'`,
    //    and `mv --update=none f l` says nothing at all and exits 0.
    if !matches!(
        job.flags.interactive,
        Interactive::AlwaysNo | Interactive::AlwaysSkip
    ) && !same_file_ok(src, src_meta, target, dst_meta)
    {
        let _ = writeln!(
            job.err,
            "mv: {} and {} are the same file",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return Verdict::Refused;
    }

    // 2. Is the destination already at least as new as the source? (`-u`,
    //    `copy.c:2353`.) Silent, and a success.
    if job.flags.update
        && !src_meta.is_dir()
        && destination_is_up_to_date(src_meta, target, dst_meta)
    {
        // …but not *quite* a no-op, because a skipped pair still goes into the
        // table (`copy.c:2380`). Upstream explains it and then flags what it
        // costs, in two comments a dozen lines apart:
        //
        // > However, we still must record that we've processed this src/dest
        // > pair, in case this source file is hard-linked to another one. In
        // > that case, we'll use the mapping information to link the
        // > corresponding destination names.
        //
        // > Note we currently replace DST_NAME unconditionally, even if it was
        // > a newer separate file.
        //
        // The second sentence is not a caveat about an unlikely corner: it is
        // this branch destroying the very thing `--update` was asked to
        // protect. Measured against GNU 9.4 — `d/a` and `d/b` two unrelated
        // newer files, `a` and `b` two names for one older inode:
        //
        // ```text
        // $ mv -uv a b d
        // removed 'd/b'
        // ```
        //
        // `d/b`'s contents are gone and it is now a second name for `d/a`;
        // both sources survive, because the skip has already promised they
        // would. Ours left `d/b` alone, which is the kinder answer and the
        // wrong one — this utility is measured against the reference, and
        // `scripts/mv-diff.sh` §22 now pins it.
        //
        // Unconditional [`Copied::remember`], with none of the link-count
        // sorting the main `earlier_file` block does: at this point the source
        // has not moved and never will, so its count is whatever it always
        // was, and a first operand with a count of one that is *later* joined
        // by nothing still has to be on record for the second one to find.
        if let Some(id) = file_id(src, src_meta)
            && let Some(earlier) = job.copied.remember(&id, target)
            && !hardlink::force_link(
                "mv",
                &earlier,
                target,
                job.flags.verbose,
                &mut *job.out,
                &mut *job.err,
            )
        {
            // Upstream's `goto un_backup` (`copy.c:2391`). Nothing to undo:
            // this runs before [`make_backup`], exactly as 2380 runs before
            // `dst_backup` at 2558. Nothing to [`Copied::forget`] either — the
            // label's forget is guarded by `earlier_file == nullptr`
            // (`copy.c:3361`), and reaching here means it was not.
            return Verdict::Refused;
        }
        return Verdict::Skipped;
    }

    // 3. Is this destination to be left alone? (`copy.c:2407-2431`)
    if abandon_move(target, dst_meta, job) {
        // GNU sets `*rename_succeeded = true` here so that `mv` does not go on
        // to `rm` the source. This `mv` has no such flag to set: the caller
        // returns without reaching either the rename or the cross-device `rm`,
        // so the source survives by construction.
        //
        // Which of the two verdicts this is, is upstream's `return_val =
        // x->interactive == I_ALWAYS_SKIP`, and the sentence goes with it: only
        // `-n` says anything, because only `-n` is a failure. `-i` prints
        // nothing beyond the question it already asked, and `--update=none`
        // prints nothing at all.
        if job.flags.interactive == Interactive::AlwaysNo {
            let _ = writeln!(job.err, "mv: not replacing {}", quoteaf_os(target));
            return Verdict::Refused;
        }
        return if job.flags.interactive == Interactive::AlwaysSkip {
            Verdict::Skipped
        } else {
            Verdict::Refused
        };
    }

    let (src_dir, dst_dir) = (src_meta.is_dir(), dst_meta.is_dir());

    // 4. A directory onto a non-directory (`copy.c:2450`). The destination is
    //    named first, which reads oddly until you notice the sentence is about
    //    what is being destroyed.
    //
    //    `--backup` lifts it, and upstream's comment says why in one line:
    //    "Moving a directory onto an existing non-directory is ok only with
    //    --backup." The refusal is about destroying the non-directory, and with
    //    a backup it is not destroyed. Note this is a *move-mode* relaxation —
    //    `cp` keeps refusing, because `x->move_mode` is part of the condition.
    if !dst_dir && src_dir && !job.flags.backup.enabled() {
        let _ = writeln!(
            job.err,
            "mv: cannot overwrite non-directory {} with directory {}",
            quoteaf_os(target),
            quoteaf_os(src)
        );
        return Verdict::Refused;
    }

    // 5. A destination this same command line just created (`copy.c:2473`).
    //    GNU's comment: "Don't let the user destroy their data, even if they
    //    try hard: this mv command must fail: mv a/f b/f c".
    //
    //    Only **numbered** backups lift it, and that is not an arbitrary line:
    //    upstream's own comment ends "Note that it works fine if you use
    //    --backup=numbered." A simple backup of `c/f` is `c/f~` every time, so
    //    `mv a/f b/f c` under `-b` would back `a/f` up to `c/f~` and then
    //    overwrite `c/f~` with the backup of `b/f` — the data the refusal
    //    exists to save is lost anyway, one name to the left. Numbered backups
    //    have somewhere new to put each one, so they are genuinely safe.
    if !dst_dir
        && job.flags.backup.kind() != BackupType::Numbered
        && let Some(set) = seen
        && let Some(id) = file_id(target, dst_meta)
        && set.contains(&(relname.clone(), id))
    {
        let _ = writeln!(
            job.err,
            "mv: will not overwrite just-created {} with {}",
            quoteaf_os(target),
            quoteaf_os(src)
        );
        return Verdict::Refused;
    }

    // 6. A non-directory onto a directory (`copy.c:2484`), which unlike 4 does
    //    not name the source at all. Lifted by `--backup` for the same reason
    //    as 4, and with the mirror-image comment upstream.
    if !src_dir && dst_dir && !job.flags.backup.enabled() {
        let _ = writeln!(
            job.err,
            "mv: cannot overwrite directory {} with non-directory",
            quoteaf_os(target)
        );
        return Verdict::Refused;
    }

    // 7. `copy.c:2503`, and it is **not** redundant with 4 even though it asks
    //    the same question of the same two files. 4 now stands down under
    //    `--backup`; this one does too — `x->backup_type == no_backups` is part
    //    of its condition — so with `-b` a directory really may replace a
    //    non-directory, and without `-b` the pair are two spellings of one
    //    refusal and 4 always wins.
    //
    //    Which makes this dead code today, exactly as it was before `-b`: the
    //    two guards are now identical rather than merely coincident, so nothing
    //    reaches here. It is kept because it is upstream's, and because the two
    //    sentences are different — 4's names both files and calls them
    //    directory and non-directory, this one uses `quotef` rather than
    //    `quoteaf` and reads as an arrow. If a future divergence ever splits
    //    the two conditions, deleting this would silently drop a refusal.
    if src_dir && !dst_dir && !job.flags.backup.enabled() {
        let _ = writeln!(
            job.err,
            "mv: cannot move directory onto non-directory: {} -> {}",
            quotef_os(src),
            quotef_os(target)
        );
        return Verdict::Refused;
    }

    Verdict::Proceed
}

/// Step 8, the last thing between the refusals and the rename: `-b`'s move of
/// the destination out of the way (`copy.c:2517`).
///
/// Its own function rather than eight more lines inside [`move_one`] because it
/// has three outcomes and the middle one is the easy mistake: `Ok(None)` — no
/// `-b`, or `-b` with nothing at the destination to move — is a *success* that
/// must fall through to the rename, not a reason to stop. Folding it into the
/// caller would put a `return false` and a `break`-shaped path in the same
/// block, which is where the file's other backup bug would have been.
///
/// # The two conditions on making one at all
///
/// * Backups must be enabled. `-b`, `--backup`, `-S` or `$VERSION_CONTROL`
///   *with* one of those; the environment alone never turns them on.
/// * The source's last component must not be `.` or `..`. `mv a/. d` would
///   otherwise back `d` up and then move `a`'s contents into a name that is no
///   longer there. `mv`'s operand checks already refuse a `.`-suffixed source
///   before this is reached, so for `mv` it guards an unreachable case — kept
///   because it is upstream's condition and because those operand checks are
///   not the reason it is safe. See [`backup::src_base_is_dot_or_dotdot`].
///
/// GNU's third condition, `(x->move_mode || ! S_ISDIR (dst_sb.st_mode))`, is
/// `true` for every caller here: this *is* move mode. Upstream's comment
/// explains the half that does not apply — `cp` does not back up a destination
/// *directory*, `mv` does.
///
/// # Errors
///
/// `Err(())` means the operand has failed and has already been reported. Two
/// ways to get there: the backup would have destroyed the source, or the rename
/// itself failed for a reason other than "there was nothing there". The unit
/// error carries nothing because there is nothing left to say — both arms print
/// their own sentence, and they are different sentences.
fn make_backup<O: Write, E: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    target: &Path,
    job: &mut Job<'_, O, E>,
) -> Result<Option<PathBuf>, ()> {
    if !job.flags.backup.enabled() || backup::src_base_is_dot_or_dotdot(src) {
        return Ok(None);
    }

    // Upstream's comment: "Fail if creating the backup file would likely
    // destroy the source file." The recipe is `cd /tmp; rm -f a a~; : > a; echo
    // A > a~; mv -b a~ a`, where the backup of `a` is named `a~` — the source —
    // so the backup rename moves the source onto itself and the move that
    // follows has nothing left to move. Skipped for numbered backups, which
    // never choose a name the user typed.
    if job.flags.backup.kind() != BackupType::Numbered
        && backup::source_is_dst_backup(src, src_meta, target, job.flags.backup.simple_suffix())
    {
        // **Two spaces after the semicolon**, which is upstream's format string
        // and not a stray keystroke here — `"backing up %s might destroy
        // source;  %s not moved"`. `scripts/mv-diff.sh` compares stderr
        // byte-for-byte, so collapsing it to one would fail the case.
        //
        // `moved` and not `copied`: the two programs share the check and pick
        // the verb from `x->move_mode`.
        let _ = writeln!(
            job.err,
            "mv: backing up {} might destroy source;  {} not moved",
            quoteaf_os(target),
            quoteaf_os(src)
        );
        return Err(());
    }

    match job.flags.backup.rename(target) {
        Ok(name) => Ok(Some(name)),
        // Upstream's `else if (errno != ENOENT)`: nothing was there to back up,
        // which is not an error. Reachable despite the `dst_meta` the caller
        // holds — that stat and this rename are two syscalls, and something else
        // may remove the destination between them.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(job.err, "mv: cannot backup {}: {why}", quoteaf_os(target));
            Err(())
        }
    }
}

/// `-u`'s question: is the destination already at least as new as the source,
/// and so not worth replacing? GNU asks it as `0 <= utimecmpat (…)` and skips
/// when that holds (`copy.c:2365`).
///
/// Modification times only — `-u` has never consulted `st_ctime` or the size —
/// and equal counts as at least as new, so `mv -u a b` between two files
/// stamped the same second-and-nanosecond leaves `b` alone. That is the case
/// `touch -r` produces and the one a rerun of the same `mv` produces, so it is
/// the common one rather than a corner.
///
/// # The truncation, and why the flag is exactly "crosses a filesystem"
///
/// Upstream computes the flag as
///
/// ```c
/// x->preserve_timestamps && ! (x->move_mode && dst_sb.st_dev == src_sb.st_dev)
/// ```
///
/// `mv` sets both `preserve_timestamps` (`mv.c:137`) and `move_mode`
/// (`mv.c:131`) unconditionally, so for this program the whole expression
/// reduces to `dst_dev != src_dev` — the move is going to be a copy rather than
/// a rename. That is the only case in which the destination's timestamp is a
/// *rounded* version of the source's rather than the same bytes, and rounding is
/// the thing [`coreutils::utimecmp`] corrects for; see that module for what it
/// costs and when it touches the disk.
///
/// The correction became reachable on 2026-09-01. Until then the cross-device
/// fallback was `fs::copy` and carried no timestamp at all, so there was no
/// preserved stamp for a truncation to be about; [`copy_across_devices`] carries
/// both stamps now. See `known-issues.md` →
/// `B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER`, whose fix
/// is what made this half necessary.
fn destination_is_up_to_date(
    src_meta: &fs::Metadata,
    target: &Path,
    dst_meta: &fs::Metadata,
) -> bool {
    let crosses_filesystem = !fileid::same_device(src_meta, dst_meta);
    utimecmp::utimecmp(target, dst_meta, src_meta, crosses_filesystem).at_least_as_new()
}

/// GNU's `abandon_move` (`copy.c:2062`): should this move be given up rather
/// than performed? `true` means leave both files where they are.
///
/// Upstream's comment beside the call site is the reason this is `mv`'s own
/// function and not [`coreutils::overwrite`]'s: "cp and mv treat -i and -f
/// differently." Three of the four differences are here.
///
/// **The `-i`/`-n`/`-f` half is the ordinary one.** `-n` abandons without
/// asking, `-i` asks, `-f` never abandons. All three are one field, so the last
/// one on the command line decides -- `mv -in` is `-n`, `mv -ni` is `-i`,
/// `mv -if` is `-f`. All measured. `--update=none` is a fourth spelling in the
/// same field and abandons exactly as `-n` does; what it does *not* share is
/// the sentence and the exit status, and neither of those is decided here.
///
/// **The fourth arm is the one nobody expects, and it fires with no option at
/// all.** With [`Interactive::Unspecified`], if stdin is a terminal *and* the
/// destination is not writable, `mv` asks anyway. So the same command is silent
/// in a script and puts a question in a shell:
///
/// ```text
/// $ chmod 444 d
/// $ mv f d                       # in a script: moves, silently, exit 0
/// $ mv f d                       # at a terminal:
/// mv: replace 'd', overriding mode 0444 (r--r--r--)?
/// ```
///
/// Both measured against 9.4 -- the second through `script(1)`, since it needs a
/// real terminal on descriptor 0. That is also why `scripts/mv-diff.sh` cannot
/// reach this arm: its cases run with stdin redirected, which is the first
/// branch. It is pinned by unit test instead.
///
/// `cp` has no such arm. For `cp` an unwritable destination changes only the
/// *wording* of a question `-i` had already decided to ask; for `mv` it is the
/// reason to ask one. That asymmetry is deliberate upstream: `cp` writes
/// *through* the destination and will simply be refused by the kernel, while
/// `mv` unlinks it, which the mode does not prevent -- so for `mv` the mode is
/// the only warning there will be.
fn abandon_move<O: Write, E: Write>(
    target: &Path,
    dst_meta: &fs::Metadata,
    job: &mut Job<'_, O, E>,
) -> bool {
    let ask = match job.flags.interactive {
        // The two "skip" values are one arm because upstream's `||` makes them
        // one: `abandon_move` answers *whether* to give up, and the difference
        // between failing and succeeding afterwards is the caller's.
        Interactive::AlwaysNo | Interactive::AlwaysSkip => return true,
        Interactive::AlwaysYes => return false,
        Interactive::AskUser => true,
        Interactive::Unspecified => {
            job.flags.stdin_tty && !overwrite::writable_destination(target, dst_meta)
        }
    };
    // `clears_destination` is `true` unconditionally: it is upstream's
    // `x->move_mode || ...`, and this program is `move_mode`. So `mv` only ever
    // puts the `replace ..., overriding mode ...?` form of the question, never
    // `cp`'s `unwritable ...; try anyway?`.
    ask && !overwrite::overwrite_ok(job.err, "mv", target, Some(dst_meta), true, job.answers)
}

/// Would moving `src` onto `target` destroy the very thing being moved? GNU's
/// `same_file_ok` (`copy.c:1739`), reduced to `mv`'s option set — `move_mode`,
/// `DEREF_NEVER`, no backups, no hard/symbolic linking.
///
/// `true` means "go ahead". The reduction drops three whole branches
/// (`x->hard_link`, the `dereference != DEREF_NEVER` arm, and the backup block),
/// and what is left is genuinely subtle, so each surviving step says which
/// question it answers.
///
/// The case that makes this worth its length is the one GNU spells out in its
/// own comment (`copy.c:1909`):
///
/// ```text
/// touch f && ln f l && ln -s f s
/// mv s f   must fail — `f` is the only thing `s` names, and moving the link
///          onto it leaves a link pointing at itself
/// mv s l   must succeed — `f` survives as the other name for the data
/// ```
///
/// Measured against GNU 9.4 both ways; this `mv` previously performed the first
/// one and destroyed the file.
fn same_file_ok(src: &Path, src_meta: &fs::Metadata, dst: &Path, dst_meta: &fs::Metadata) -> bool {
    let same = same_inode((src, src_meta), (dst, dst_meta));
    let (src_link, dst_link) = (
        src_meta.file_type().is_symlink(),
        dst_meta.file_type().is_symlink(),
    );

    // Two symlinks: what matters is whether they are the same *link*, because
    // replacing one link with another touches nothing either points at.
    if src_link && dst_link {
        let same_name = same_entry(src, dst);
        // Unless they are two hard links to one symlink, where the rename would
        // do nothing at all and silently report success.
        if !same_name && same {
            return false;
        }
        return !same_name;
    }

    // Moving onto a symlink is fine: the rename replaces the link itself, so
    // whatever it pointed at is untouched.
    if dst_link {
        return true;
    }

    // Two hard links to one file, reached by different names. The rename would
    // remove one of them, and which one is a race.
    if same && nlink(dst_meta) > 1 && !same_entry(src, dst) {
        return false;
    }

    // Neither is a symlink, so the only way to be the same file is to be it.
    if !src_link && !same {
        return true;
    }

    // A symlink onto a file that has another name: the data survives under that
    // other name, so this is allowed. `canonicalize`, because the question is
    // where the link *ends up*, not what one hop of it says.
    if src_link
        && nlink(dst_meta) > 1
        && let Ok(resolved) = fs::canonicalize(src)
    {
        return !same_entry(&resolved, dst);
    }

    // Last: follow both sides all the way and compare. This is what catches
    // `mv link file` where `link` resolves to `file` — the two are different
    // *entries* and different *links*, and the same file.
    let followed = |path: &Path, meta: &fs::Metadata, is_link: bool| {
        if is_link {
            fs::metadata(path).ok()
        } else {
            Some(meta.clone())
        }
    };
    let (Some(s), Some(d)) = (
        followed(src, src_meta, src_link),
        followed(dst, dst_meta, dst_link),
    ) else {
        // A dangling link is not the same file as anything.
        return true;
    };
    !same_inode((src, &s), (dst, &d))
}

/// `EXDEV` — the kernel refusing to rename across a filesystem boundary, which
/// is the one `rename` failure `mv` is supposed to work around rather than
/// report.
#[cfg(unix)]
const CROSS_DEVICE_ERRNO: i32 = 18;
/// `ERROR_NOT_SAME_DEVICE`, the same condition on the development host.
#[cfg(windows)]
const CROSS_DEVICE_ERRNO: i32 = 17;

/// `EINVAL`, which `rename` reports for "the destination is inside the source".
///
/// `mv` gives this its own diagnostic (`copy.c:2798`) rather than the generic
/// one, because "Invalid argument" tells the user nothing about which of the two
/// paths was the problem.
#[cfg(unix)]
const SUBDIRECTORY_OF_ITSELF_ERRNO: i32 = 22;

/// Is this the `rename` failure that means "you asked me to put a directory
/// inside itself"?
///
/// Only asked of a number, and only on a host where that number is an errno.
/// `ErrorKind` has no variant for this, and the kind std *does* map `EINVAL` to
/// — `InvalidInput` — is far too broad to key a specific diagnostic on: on the
/// development host it would claim every rejected rename was this case. GNU
/// itself notes at `copy.c:2798` that keying on the errno is fragile; keying on
/// a coarser classification would be worse.
#[cfg(unix)]
fn is_subdirectory_of_itself(e: &io::Error) -> bool {
    e.raw_os_error() == Some(SUBDIRECTORY_OF_ITSELF_ERRNO)
}

/// On a host where that number is not an errno there is nothing to key on, so
/// the request falls through to the generic `cannot move` diagnostic.
#[cfg(not(unix))]
fn is_subdirectory_of_itself(_e: &io::Error) -> bool {
    false
}

/// Does this `rename` failure blame the destination rather than the move?
///
/// `copy.c:2848` — the switch that picks between `cannot overwrite %s`, naming
/// only the destination, and `cannot move %s to %s`, naming both. Every code
/// here is one the kernel can only be reporting *about* the existing
/// destination: it is a directory, it is not empty, it is a running binary, it
/// is out of space or quota, it already has the maximum link count. Naming the
/// source in those cases would point at the wrong file.
///
/// The values are Linux's. This runs on the development host too, where the
/// numbers differ and nothing here matches — the fallback diagnostic is the
/// less specific one, which is safe; the target is where it must be right.
fn blames_the_destination(e: &io::Error) -> bool {
    /// `EEXIST`, `EISDIR`, `ENOTEMPTY`, `ETXTBSY`, `EDQUOT`, `EMLINK`,
    /// `ENOSPC` — in the order `copy.c` lists them.
    const DESTINATION_CODES: &[i32] = &[
        122, // EDQUOT
        17,  // EEXIST
        21,  // EISDIR
        31,  // EMLINK
        28,  // ENOSPC
        26,  // ETXTBSY
        39,  // ENOTEMPTY
    ];
    if cfg!(unix)
        && e.raw_os_error()
            .is_some_and(|n| DESTINATION_CODES.contains(&n))
    {
        return true;
    }
    // The two the standard library classifies for us, so that the development
    // host reaches the same branch for the cases it can actually produce.
    matches!(
        e.kind(),
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty
    )
}

fn is_cross_device(e: &io::Error) -> bool {
    #[cfg(any(unix, windows))]
    if e.raw_os_error() == Some(CROSS_DEVICE_ERRNO) {
        return true;
    }
    // Checked second, not first: our own target's libstd may not yet map EXDEV
    // onto this variant, and a rename that *is* cross-device must not be
    // reported as a hard failure just because the classification is missing.
    e.kind() == io::ErrorKind::CrossesDevices
}

/// A step of the cross-device fallback that failed, and the sentence GNU prints
/// for that step.
///
/// The sentence is carried rather than derived from the error, because the two
/// steps that fail with the same errno say different things: an unreadable
/// source and an unwritable destination directory both give `EACCES`, and GNU
/// answers `cannot open 'f' for reading` for one and `cannot create regular file
/// 'd/g'` for the other. Ours used to answer `cannot move 'f' to 'd/g'` for both
/// — the same exit status and the same errno, with the half of the information
/// that says where to go and look thrown away. See `known-issues.md` →
/// `B-MVS-CROSS-DEVICE-FAILURES-DO-NOT-NAME-THE-STEP`.
///
/// It is the whole diagnostic bar the errno and the `mv: ` prefix, already
/// quoted, because the quoting style is not the same in all of them: GNU quotes
/// with `quoteaf` almost everywhere and with `quotef` in the permissions
/// sentence, and a helper that took the names and picked a style would have to
/// know which sentence it was building anyway.
#[cfg_attr(test, derive(Debug))]
struct Failed {
    /// e.g. `cannot open 'f' for reading`.
    what: String,
    /// The errno, appended after `: `.
    err: io::Error,
}

impl Failed {
    fn new(what: String, err: io::Error) -> Self {
        Failed { what, err }
    }
}

/// Say that a cross-device move failed, put back anything `-b` moved aside, and
/// report the move as not done.
///
/// One function because the two callers must agree: upstream reaches its
/// `un_backup` label from eleven places, all of them in the *copying* machinery
/// that this fallback is, and both of ours are in it. What is **not** here is
/// the destination-clearing failure above, which upstream returns from directly;
/// keeping that one out of this helper is the point of having the helper.
fn give_up_cross_device<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    failure: &Failed,
    target: &Path,
    moved_aside: Option<&Path>,
) -> bool {
    let why = strerror(&failure.err);
    let what = &failure.what;
    let _ = writeln!(job.err, "mv: {what}: {why}");
    // **The only place `mv` puts a backup back**, and that is upstream's shape
    // rather than an omission here: the move-mode rename failures above it all
    // say `return false` outright (`copy.c:2866`). So `mv -b a b` whose rename
    // fails leaves `b~` in place with no `b` — odd, measured, and deliberately
    // reproduced, because `scripts/mv-diff.sh` compares the resulting tree and
    // "fixing" it here would turn a passing case red.
    backup::un_backup(
        "mv",
        moved_aside,
        target,
        job.flags.verbose,
        &mut *job.out,
        &mut *job.err,
    );
    false
}

/// What a cross-device move of a directory is refused with. See
/// `known-issues.md` → `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`, and
/// `scripts/mv-diff.sh` §22 for the case that becomes an XPASS the moment a
/// recursive fallback lands.
///
/// The one sentence in this fallback that is *not* GNU's, because GNU has no
/// equivalent — it does the move. `cannot move X to Y` is what the whole
/// fallback used to say and is the right shape for a refusal of the operation
/// rather than of a step within it.
fn no_directories(src: &Path, target: &Path) -> Failed {
    Failed::new(
        format!("cannot move {} to {}", quoteaf_os(src), quoteaf_os(target)),
        io::Error::new(
            io::ErrorKind::Unsupported,
            "moving a directory across filesystems is not implemented by this mv",
        ),
    )
}

/// Unlink whatever is at `target`, treating "nothing was there" as done.
///
/// The kind comes from the caller having already established that the source is
/// not a directory, which is GNU's `S_ISDIR (src_mode) ? AT_REMOVEDIR : 0` with
/// the directory arm unreachable: a directory source against a non-directory
/// destination was refused much further up, so the two kinds agree, and the
/// source is the one whose `stat` is already in hand.
fn clear_destination(target: &Path) -> io::Result<()> {
    match fs::remove_file(target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The `EXDEV` fallback: reproduce the source at `target`, then remove it.
///
/// `target` is a free name: [`clear_destination`] has just unlinked anything
/// that was there, so every kind here creates rather than overwrites. That is
/// also why nothing here is conditional on a `new_dst`: GNU's is set `true` by
/// the block that does the unlinking (`copy.c:2892`), so every test of it in the
/// machinery below that point is a test of a constant for this caller.
///
/// **A move is meant to be indistinguishable from a rename**, and that is not a
/// figure of speech: `cp_option_init` (`mv.c:119`) turns on `preserve_mode`
/// (136), `preserve_timestamps` (137), `preserve_ownership` (134),
/// `preserve_links` (135) and `preserve_xattr` (145) unconditionally, with no
/// option to turn any of them off.
/// Everything a rename would have kept for free this function has to carry by
/// hand, in an order that is not free either:
///
/// 1. **the bytes**, into a destination created with the group and other bits
///    held back — see [`create_destination`];
/// 2. **the times**, from the source's `stat` at whatever resolution it has;
/// 3. **the owner**, which may be refused, and a refusal costs the set-ID bits;
/// 4. **the mode**, last, because a `chown` clears `S_ISUID`/`S_ISGID` for a
///    non-root process and a mode written before it would be silently undone.
///
/// GNU's comment at the top of that sequence is the whole of the argument:
/// *"chown turns off set[ug]id bits for non-root, so do the chmod last"*
/// (`copy.c:3245`).
///
/// None of steps 2–4 is fatal. `mv` leaves `require_preserve` false
/// (`mv.c:143`), so a preservation that fails is *reported* on `err` and the
/// move still counts as done — which is the right answer for the overwhelmingly
/// common one, an ordinary user who may not give the copy away.
///
/// **Extended attributes are not carried yet**, though `preserve_xattr` is on
/// upstream. See `known-issues.md` →
/// `B-MVS-CROSS-DEVICE-FALLBACK-DROPS-EXTENDED-ATTRIBUTES`.
///
/// # Errors
///
/// The first step that failed, carrying [`Failed`]'s sentence for it. A
/// directory never reaches here — [`move_one`] refuses it before the destination
/// is cleared, because clearing first and refusing second would destroy a file
/// for a move that then did not happen — but the arm is kept so that the
/// function is safe to call directly, which is how it is tested.
fn copy_across_devices<E: Write>(
    src: &Path,
    target: &Path,
    metadata: &fs::Metadata,
    run: &mut copy::Run<'_, E>,
) -> Result<(), Failed> {
    let kind = metadata.file_type();

    if kind.is_symlink() {
        // NOT `fs::copy`, which follows the link — see module docs, bug 4. The
        // link's *text* is reproduced verbatim, so a relative link keeps meaning
        // whatever it means relative to its new directory, exactly as `rename`
        // would have left it.
        let points_at = fs::read_link(src).map_err(|e| {
            Failed::new(format!("cannot read symbolic link {}", quoteaf_os(src)), e)
        })?;
        symlink(&points_at, target).map_err(|e| {
            Failed::new(
                format!("cannot create symbolic link {}", quoteaf_os(target)),
                e,
            )
        })?;
        // The link's owner is taken *here*, where the link was made, and not by
        // the tail below — whose ownership step skips a symlink destination
        // outright. That is GNU's arrangement rather than this file's: the
        // `lchownat` is inline in `copy_internal`'s symlink arm (`copy.c:3180`)
        // and the shared tail's is guarded by `!dest_is_symlink`, so dropping
        // this call in favour of the tail's would leave a moved link unable to
        // keep its owner at all. [`Made::Symlink`] is what tells the engine
        // which of the two it is being asked for; it also selects the bare
        // `lchownat` with no group-only retry, and the unquoted name upstream
        // prints for this one sentence alone.
        //
        // Unconditional, where the tail's is guarded by "the owner differs":
        // the link was made a line ago, so it is new by construction, and it is
        // `new_dst ||` that makes the tail's guard true for a new destination
        // in any case.
        let source = copy::Source::new(On::Path(src, Link::NoFollow), src, metadata);
        let on = On::Path(target, Link::NoFollow);
        // The result is discarded rather than propagated, and that is a fact
        // about `mv`'s options rather than a shortcut: [`Chowned::Failed`] is
        // produced only under `require_preserve`, which `cp_option_init` leaves
        // false (`mv.c:143`). A refused `lchown` on a link is therefore always
        // [`Chowned::Disowned`] — reported, and not fatal — and there is no
        // mode for the narrowing it would otherwise force, a symlink having
        // none. See [`Job::run`].
        let _ = chown_to_source(source, on, target, Made::Symlink, true, run);
        // Zero debt: nothing was withheld from a link, which has no mode to
        // withhold from. The engine returns before consulting it in any case —
        // see the note on [`Job::umask`] for why the value is still built
        // honestly rather than relied on to go unread.
        let mut debt = ModeDebt::default();
        // Always `true` for a move; see the discard above and [`Job::run`].
        let _ = preserve_attributes(source, on, target, Made::Symlink, true, &mut debt, run);
        return remove_source(src);
    }

    if kind.is_dir() {
        return Err(no_directories(src, target));
    }

    let mode = fsattr::permission_bits(metadata);
    let mut source = fs::File::open(src)
        .map_err(|e| Failed::new(format!("cannot open {} for reading", quoteaf_os(src)), e))?;
    let (mut dest, mut debt) = create_destination(target, mode).map_err(|e| {
        Failed::new(
            format!("cannot create regular file {}", quoteaf_os(target)),
            e,
        )
    })?;

    // `io::copy` and not a hand-written loop, because `std` specialises it to
    // `copy_file_range` when both sides are files — the same kernel-side copy
    // GNU reaches for, and the same reason: it moves the data without a trip
    // through userspace and it reproduces a sparse file's holes instead of
    // writing out the zeroes.
    //
    // **The price is that a read failure and a write failure arrive as one
    // error**, where GNU distinguishes `error reading %s` from
    // `error writing %s`. The destination's sentence is the one used, because
    // that is the side that fails in practice — `ENOSPC`, `EDQUOT`, a full
    // quota, a device going away mid-write — while a read error means the
    // *source* medium is failing, which is rarer and louder. Telling them apart
    // would mean giving up `copy_file_range`, which is a real loss for a real
    // gain in a case nothing measures; it is logged rather than traded for.
    // See `known-issues.md` →
    // `B-MVS-CROSS-DEVICE-COPY-CANNOT-TELL-A-READ-FAILURE-FROM-A-WRITE-FAILURE`
    // for the recoverable version, and `design-decisions.md` §741 for the whole
    // of the argument, including the two alternatives that were rejected.
    io::copy(&mut source, &mut dest)
        .map_err(|e| Failed::new(format!("error writing {}", quoteaf_os(target)), e))?;

    // The same tail `cp` runs, out of the same code: times, then ownership, then
    // the extended attributes, then the mode — an order that is correctness
    // rather than arrangement, and whose two reasons GNU leaves written above
    // the steps. See [`copy::preserve_attributes`].
    //
    // Both handles rather than both names, which is [`On`]'s reason and not a
    // saved syscall: the mode restored last carries the set-user-ID bit, and
    // writing it by *name* after the bytes are down leaves a window in which the
    // name can be made to mean a different file.
    //
    // Always `true` for a move — see [`Job::run`] — so the discard says only
    // that `mv` has no fatal preservation step, which is `require_preserve` and
    // `require_preserve_xattr` both being false.
    let source_view = copy::Source::new(On::File(&source), src, metadata);
    let _ = preserve_attributes(
        source_view,
        On::File(&dest),
        target,
        Made::Regular,
        true,
        &mut debt,
        run,
    );
    remove_source(src)
}

/// Create `target` for writing, with the source's mode *narrowed* to the owner.
///
/// The withholding is GNU's `omitted_permissions`, which for a move is
/// `dst_mode & (S_IRWXG | S_IRWXO)` — every group and other bit — because
/// `preserve_ownership` is on (`copy.c:2902`). The bits come back in the final
/// [`fsattr::copy_permissions`], and the window they are missing from is the one
/// between the file existing and it having the right owner. Without the
/// withholding, a file whose source is group- or world-readable is briefly
/// readable by *this* process's group and by everyone, holding the source's
/// contents, before the `chown` hands it to whoever should have had it.
///
/// `create_new` is GNU's `O_EXCL`, which it uses whenever `new_dst`
/// (`copy.c:1456`) — and after the destination has been unlinked, `new_dst` is
/// what this caller always is. It is not an optimisation: without it a name
/// created between the unlink and the open would be opened and truncated, which
/// is the very thing the unlink was there to prevent.
///
/// The `S_IWUSR` that goes the other way is GNU's too, and it is there for the
/// extended attributes rather than for the bytes: Linux's `xattr_permission`
/// (`fs/xattr.c`) demands write access to the *inode* before it will set an
/// attribute on it, so a read-only source — mode `0444` — would otherwise
/// produce a copy that no `setxattr` could write to. `copy.c:1452` widens the
/// open mode by exactly that bit, and only for a non-root caller, root's
/// `setxattr` not being subject to the check. That condition is
/// [`fsattr::chown_privileges`], which is upstream's `x->owner_privileges`
/// under its other name; the `preserve_xattr &&` half of upstream's test is
/// constant here, because `mv.c:145` sets it and mv's getopt never clears it.
///
/// It costs no exposure, which is why it can be ORed in beside a withholding
/// that exists to prevent some. The bit is the *owner's* write bit, and the
/// owner at this instant is the process doing the copying — which already holds
/// a writable descriptor to the file it just created. Nothing is granted to
/// anyone who did not have it.
///
/// GNU tracks it as `extra_permissions` so that its `omitted_permissions`
/// fallback can take it off again; nothing here has to, because a move never
/// reaches that branch. `if (x->preserve_mode || x->move_mode)` (`copy.c:1672`)
/// claims the chain first and calls `copy_acl` with `src_mode`, which writes the
/// mode absolutely. The `copy_permissions` that closes
/// [`copy::preserve_attributes`] is that call, and it starts with the same
/// absolute `set_mode`, so the extra bit leaves with the rest of the temporary
/// mode.
#[cfg_attr(not(unix), allow(unused_variables))]
fn create_destination(target: &Path, mode: u32) -> io::Result<(fs::File, ModeDebt)> {
    let extra = if fsattr::chown_privileges() {
        0
    } else {
        OWNER_WRITE
    };
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode((mode & !GROUP_AND_OTHER) | extra);
    }
    let file = opts.open(target)?;
    top_up_extra(&file, extra);
    // Returned rather than reconstructed by the caller, because this is the one
    // place that knows both halves. Nothing a *move* does reads it — the mode
    // step takes `preserve_mode`'s branch and returns before the settle-up — but
    // an honest value costs a struct and a wrong one would be a landmine for
    // whoever changes that; see the note on [`Job::umask`], which is the same
    // argument about the same short-circuit.
    let debt = ModeDebt {
        omitted: mode & GROUP_AND_OTHER,
        forced: None,
        extra,
    };
    Ok((file, debt))
}

/// Put the extra owner-write bit on if the `open` did not manage it.
///
/// The mode handed to `open` is narrowed by the umask, which can perfectly well
/// include `0o200` — `umask 0222` is unusual but legal, and under it the bit
/// asked for at creation simply does not arrive. Asking is therefore not the
/// same as having, and a move of a read-only file under such a umask carries no
/// extended attributes at all: every `setxattr` onto the copy is refused by
/// `xattr_permission`, and each refusal is reported, so what should be a silent
/// move becomes a screenful of `Permission denied`.
///
/// GNU makes the same repair in the same place, immediately after the open and
/// before any attribute is written (`copy.c:1539`), and states the fallback for
/// when even that fails: *"if that fails give up with extra permissions, letting
/// `copy_attr` fail later."* Which is why both failures here are discarded — the
/// step is an optimisation of a permission check, and the thing it exists to
/// make possible reports its own failure with a better sentence than this
/// function could.
///
/// Nothing has to take the bit off again. The `copy_permissions` that closes
/// [`copy::preserve_attributes`] writes the source's mode absolutely, so the
/// temporary widening leaves with the rest of the temporary mode — that is
/// `copy.c:1672`'s `if (x->preserve_mode || x->move_mode)` claiming the chain
/// before GNU's own `extra_permissions` branch can be reached.
fn top_up_extra(file: &fs::File, extra: u32) {
    if extra == 0 {
        return;
    }
    let Ok(meta) = file.metadata() else {
        // See above: a descriptor opened a moment ago has no reachable stat
        // failure, and the fallback for one is the same as for a refused chmod.
        return;
    };
    let now = fsattr::permission_bits(&meta);
    if now | extra != now {
        // Discarded deliberately; see above.
        let _ = fsattr::set_mode(On::File(file), now | extra);
    }
}

/// `S_IRWXG | S_IRWXO` — the bits [`create_destination`] holds back until the
/// owner is settled.
#[cfg_attr(not(unix), allow(dead_code))]
const GROUP_AND_OTHER: u32 = 0o077;

/// `S_IWUSR` — the bit [`create_destination`] adds so the extended attributes
/// can be written onto a read-only file, and [`top_up_extra`] re-adds if the
/// umask took it off again.
const OWNER_WRITE: u32 = 0o200;

/// Unlink the source once the copy is complete, with GNU's `rm` sentence.
///
/// `mv` does this through `rm()` (`mv.c:238`) rather than in `copy.c`, which is
/// why the wording is `rm`'s: `remove.c:352` reports a failed unlink as
/// `cannot remove %s`.
fn remove_source(src: &Path) -> Result<(), Failed> {
    fs::remove_file(src).map_err(|e| Failed::new(format!("cannot remove {}", quoteaf_os(src)), e))
}

#[cfg(unix)]
fn symlink(points_at: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(points_at, at)
}

/// Recreating a symlink needs a distinction between file and directory links on
/// Windows, and a privilege the test host does not necessarily have. Refusing is
/// the only answer that does not silently produce something other than a
/// symlink; the target OS is the `#[cfg(unix)]` branch above.
#[cfg(not(unix))]
fn symlink(_points_at: &Path, _at: &Path) -> io::Result<()> {
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
    use coreutils::fsattr::Times;
    use coreutils::yesno::Canned;
    use scratchdir::ScratchDir;
    use std::time::{Duration, SystemTime};

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// What [`copy_across_devices`] is handed by a test: `err`, plus the options
    /// `mv` actually runs with.
    ///
    /// Through [`mv_opts`] rather than an options list written here, which is
    /// the difference between a test of the preserve tail and a test of a copy
    /// of the preserve tail's arguments. The umask is read per call for the
    /// same reason [`Job`] reads it per command: a test that sets the mask
    /// around a move must get the mask it set.
    fn cross_device_run<E: Write>(err: &mut E) -> copy::Run<'_, E> {
        copy::Run {
            opts: mv_opts(coreutils::umask::current()),
            err,
        }
    }

    /// The operands of a successful parse, or a panic naming what came back.
    fn run_parse(items: &[&str]) -> Vec<String> {
        run_parse_full(items).1
    }

    /// The flags *and* operands of a successful parse.
    fn run_parse_full(items: &[&str]) -> (MvFlags, Vec<String>) {
        let (f, _, p) = run_parse_dest(items);
        (f, p)
    }

    /// The whole of a successful parse, [`Destination`] included.
    fn run_parse_dest(items: &[&str]) -> (MvFlags, Destination, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, d, p) => (
                f,
                d,
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
        assert!(run_parse(&[]).is_empty());
    }

    #[test]
    fn simple_rename() {
        assert_eq!(run_parse(&["a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn force_is_accepted() {
        assert_eq!(run_parse(&["-f", "a", "b"]), vec!["a", "b"]);
        assert_eq!(run_parse(&["--force", "a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn force_clustered_and_repeated() {
        assert_eq!(run_parse(&["-ff", "a", "b"]), vec!["a", "b"]);
    }

    /// The whole of `-i`/`-f`/`-n`'s parsing, which is one assignment each and
    /// would need no test but for the rule that makes them one field: **the
    /// last one wins**, and it wins across every spelling and across a cluster.
    ///
    /// Every row is measured against GNU 9.4 by running the command; the
    /// harness's §15 has the same table from the other end, as observed
    /// behaviour rather than as a parse.
    #[test]
    fn the_last_of_minus_i_f_n_wins() {
        let cases: &[(&[&str], Interactive)] = &[
            (&[], Interactive::Unspecified),
            (&["-i"], Interactive::AskUser),
            (&["-f"], Interactive::AlwaysYes),
            (&["-n"], Interactive::AlwaysNo),
            (&["--interactive"], Interactive::AskUser),
            (&["--force"], Interactive::AlwaysYes),
            (&["--no-clobber"], Interactive::AlwaysNo),
            // Two options, two orders, two answers.
            (&["-i", "-n"], Interactive::AlwaysNo),
            (&["-n", "-i"], Interactive::AskUser),
            (&["-i", "-f"], Interactive::AlwaysYes),
            (&["-f", "-i"], Interactive::AskUser),
            (&["-n", "-f"], Interactive::AlwaysYes),
            (&["-f", "-n"], Interactive::AlwaysNo),
            // A cluster is not one option: `getopt` hands the bytes over
            // singly, so last-wins applies *inside* it too.
            (&["-if"], Interactive::AlwaysYes),
            (&["-fi"], Interactive::AskUser),
            (&["-nfi"], Interactive::AskUser),
            (&["-ifn"], Interactive::AlwaysNo),
            // Long and short mix, and options may follow the operands.
            (&["--force", "-i"], Interactive::AskUser),
            (&["-i", "--no-clobber"], Interactive::AlwaysNo),
        ];
        for (opts, want) in cases {
            let mut items: Vec<&str> = opts.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let (flags, paths) = run_parse_full(&items);
            assert_eq!(flags.interactive, *want, "{opts:?}");
            assert_eq!(paths, vec!["a", "b"], "{opts:?}");
        }
        // Trailing, after the operands, since parsing permutes.
        assert_eq!(
            run_parse_full(&["a", "b", "-i", "-n"]).0.interactive,
            Interactive::AlwaysNo
        );
    }

    #[test]
    fn verbose_is_recorded_by_both_spellings() {
        for form in [
            vec!["-v", "a", "b"],
            vec!["--verbose", "a", "b"],
            // The abbreviation has to be unambiguous, so `--verb` and not
            // `--verb`'s shorter prefixes; see `ambiguous_abbreviation_is_refused`.
            vec!["--verb", "a", "b"],
            // Clustered with the option it is most often typed beside.
            vec!["-fv", "a", "b"],
        ] {
            let (flags, paths) = run_parse_full(&form);
            assert!(flags.verbose, "{form:?}");
            assert_eq!(paths, vec!["a", "b"], "{form:?}");
        }
    }

    #[test]
    fn verbose_is_off_unless_asked_for() {
        let (flags, _) = run_parse_full(&["a", "b"]);
        assert_eq!(flags, MvFlags::default());
        assert!(!flags.verbose);
    }

    /// `--verbose=1` is a value given to an option that takes none, which is a
    /// usage error rather than a truthy flag. Worth pinning because the natural
    /// way to add a flag — matching on the name and ignoring `inline` — accepts
    /// it silently.
    #[test]
    fn verbose_takes_no_value() {
        let e = fail(&["--verbose=1", "a", "b"]);
        assert!(
            e.sentence.contains("doesn't allow an argument"),
            "{:?}",
            e.sentence
        );
    }

    #[test]
    fn flag_may_follow_operands() {
        assert_eq!(run_parse(&["a", "b", "-f"]), vec!["a", "b"]);
    }

    #[test]
    fn multiple_sources() {
        assert_eq!(run_parse(&["a", "b", "c", "d"]), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]), vec!["-", "dest"]);
    }

    /// Bug 1 in the module docs: this used to answer `unknown option: --`, so a
    /// file named `-foo` could not be moved at all.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]), vec!["-foo", "bar"]);
        assert_eq!(run_parse(&["--", "-f"]), vec!["-f"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).is_empty());
    }

    #[test]
    fn long_options_abbreviate() {
        assert_eq!(run_parse(&["--for", "a", "b"]), vec!["a", "b"]);
    }

    /// `--v` must stay ambiguous between `--verbose` and `--version`. It only
    /// does so because `--verbose` is in the table despite being unimplemented;
    /// this is the test that fails if someone prunes the table to what is
    /// actually handled.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--verbose"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--version"), "{:?}", e.sentence);
    }

    /// Likewise `--n`, across all three `no-` options.
    #[test]
    fn ambiguous_no_prefix_is_refused() {
        let e = fail(&["--n"]);
        assert_eq!(
            e.sentence,
            "option '--n' is ambiguous; possibilities: '--no-clobber' \
             '--no-copy' '--no-target-directory'"
        );
    }

    /// The prefix that caught the table being wrong. `--no-c` reaches
    /// `--no-clobber` and `--no-copy`; before `("no-copy", …)` was added it
    /// resolved here and was ambiguous in GNU, which is the exact shape of
    /// silently acting on an option the user did not unambiguously name.
    #[test]
    fn ambiguous_no_c_prefix_is_refused() {
        let e = fail(&["--no-c"]);
        assert_eq!(
            e.sentence,
            "option '--no-c' is ambiguous; possibilities: '--no-clobber' \
             '--no-copy'"
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-z", "a", "b"]);
        assert!(e.sentence.contains("invalid option"), "{:?}", e.sentence);
        assert!(e.sentence.contains('z'), "{:?}", e.sentence);
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

    /// Unimplemented options are rejected *by name*, not as typos. `-Z` asks
    /// for a security context to be set; answering "invalid option" sends the
    /// user to check a spelling that was right, and ignoring it would produce a
    /// destination that looks correct and is labelled wrong.
    ///
    /// `-i`, `-n`, `--interactive` and `--no-clobber` were on this list until
    /// they were implemented, which is what a promotion out of it looks like:
    /// the letters move to [`the_last_of_minus_i_f_n_wins`] and the harness's
    /// `missing` markers move to its own section. `-t` and `-T` left the same
    /// way, to [`a_target_directory_is_taken_out_of_the_operands`] and §16;
    /// `-u`/`--update` to [`the_three_update_words_write_two_fields`] and §17;
    /// and `-b`/`-S`/`--backup`/`--suffix` to
    /// [`the_four_ways_to_ask_for_a_backup`] and §18. `-Z` is the only short
    /// letter left, which is why this asserts on one option rather than looping
    /// over a list as its long-option sibling still does.
    #[test]
    fn the_one_unimplemented_short_option_is_rejected_by_name() {
        let e = fail(&["-Z", "a", "b"]);
        assert!(e.sentence.contains("not implemented"), "{:?}", e.sentence);
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in ["--no-copy", "--debug", "--context"] {
            let e = fail(&[name, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
        }
    }

    /// `-S`'s suffix does not survive into the operand list as a file to move,
    /// and a `-S` with no value is the *parser's* error rather than anything
    /// this file says.
    ///
    /// This is the half of [`SHORT_OPTIONS`] that is not about which options
    /// exist — the colon after `S`. It was pinned here for a release while `-S`
    /// was still refused, on the grounds that nothing user-visible depended on
    /// it *yet*; both halves now matter, and neither assertion had to change
    /// when they started to. `-t` spent a release in this test the same way and
    /// is now [`a_target_directory_is_taken_out_of_the_operands`].
    #[test]
    fn a_suffix_is_taken_out_of_the_operands() {
        let (flags, paths) = run_parse_full(&["-S", ".bak", "a", "b"]);
        assert_eq!(paths, ["a", "b"]);
        assert_eq!(flags.backup.simple_suffix(), b".bak");
        // GNU reports this too, and from `getopt` rather than from the switch
        // arm — which is why the wording is the parser's and not `mv`'s.
        let e = fail(&["-S"]);
        assert!(e.sentence.contains("requires an argument"), "{e:?}");
    }

    /// All four spellings of `-t`, and the fact that its value never lands in
    /// the operand list. `-tdir` is the one that could only work through a
    /// table that says the letter takes a value; `mv a b -t d` is the one that
    /// could only work through a parser that permutes.
    #[test]
    fn a_target_directory_is_taken_out_of_the_operands() {
        for spelling in [
            &["-t", "d", "a", "b"][..],
            &["-td", "a", "b"][..],
            &["--target-directory=d", "a", "b"][..],
            &["--target-directory", "d", "a", "b"][..],
            &["a", "b", "-t", "d"][..],
        ] {
            let (_, dest, paths) = run_parse_dest(spelling);
            assert_eq!(dest.directory, Some(OsString::from("d")), "{spelling:?}");
            assert!(!dest.no_directory, "{spelling:?}");
            assert_eq!(paths, ["a", "b"], "{spelling:?}");
        }
    }

    /// `--strip-trailing-slashes` has no short form and no value, so the only
    /// thing the parse can get wrong is which field it writes — and it must
    /// write *only* that one, since the option is otherwise inert.
    ///
    /// The abbreviation is included because [`LONG_OPTIONS`] is what decides
    /// whether one is ambiguous, and `--str` has been unambiguous all along:
    /// before this option was implemented it resolved to a refusal, which is
    /// the one outcome that looks the same whether the table is right or wrong.
    #[test]
    fn strip_trailing_slashes_sets_only_its_own_field() {
        for spelling in [
            &["--strip-trailing-slashes", "a", "b"][..],
            &["--str", "a", "b"][..],
            &["a", "b", "--strip-trailing-slashes"][..],
        ] {
            let (flags, dest, paths) = run_parse_dest(spelling);
            assert!(dest.strip_slashes, "{spelling:?}");
            assert_eq!(dest.directory, None, "{spelling:?}");
            assert!(!dest.no_directory, "{spelling:?}");
            assert_eq!(paths, ["a", "b"], "{spelling:?}");
            assert_eq!(flags, MvFlags::default(), "{spelling:?}");
        }
        // And it is off without the option, which is what makes every other
        // test in this file a test of the unstripped path.
        let (_, dest, _) = run_parse_dest(&["a", "b"]);
        assert!(!dest.strip_slashes);
    }

    /// [`strip_operands`] itself: gnulib's `strip_trailing_slashes` semantics,
    /// which are not "trim `/`".
    ///
    /// A filesystem root keeps one slash — `///` is `/`, not the empty name —
    /// and an interior slash is never touched, so `a//b//` loses only the pair
    /// at the end. Both are gnulib's, and both matter here rather than being
    /// pathname trivia: the empty name would turn `mv --strip-trailing-slashes
    /// / x` into a move of the current directory's unnamed sibling, and
    /// trimming interior slashes would silently rename the operand.
    #[test]
    fn strip_operands_follows_gnulib_not_intuition() {
        let on = Destination {
            strip_slashes: true,
            ..Destination::default()
        };
        let cases: &[(&str, &str)] = &[
            ("a/", "a"),
            ("a///", "a"),
            ("a", "a"),
            ("a//b//", "a//b"),
            ("/", "/"),
            ("///", "/"),
            ("", ""),
            (".", "."),
            ("..//", ".."),
        ];
        let given: Vec<OsString> = cases.iter().map(|(g, _)| OsString::from(*g)).collect();
        let want: Vec<OsString> = cases.iter().map(|(_, w)| OsString::from(*w)).collect();
        assert_eq!(strip_operands(&on, &given).as_ref(), want.as_slice());

        // Off, the operands are handed on untouched *and unallocated* — the
        // bytes the kernel sees are the bytes argv held, which is what keeps a
        // name this host cannot spell from being rewritten on its way through.
        let off = Destination::default();
        assert!(matches!(strip_operands(&off, &given), Cow::Borrowed(_)));
        assert_eq!(strip_operands(&off, &given).as_ref(), given.as_slice());
    }

    /// GNU compares nothing here — it asks only whether one was given already —
    /// so naming the same directory twice fails just as two different ones do.
    #[test]
    fn a_second_target_directory_is_refused() {
        for spelling in [
            &["-t", "d", "-t", "d", "a"][..],
            &["-t", "d", "-t", "e", "a"][..],
            &["-t", "d", "--target-directory=e", "a"][..],
        ] {
            let e = fail(spelling);
            assert_eq!(e.sentence, "multiple target directories specified");
            // `error (EXIT_FAILURE, …)` upstream, not `usage`, so there is no
            // "Try 'mv --help'" after it.
            assert_eq!(e.referral, None, "{spelling:?}");
        }
    }

    /// `-T` is a flag, both spellings, and it does not disturb the operands.
    /// Repeating it is not an error — unlike `-t`, there is no value to
    /// disagree with.
    #[test]
    fn no_target_directory_is_a_flag_both_ways() {
        for spelling in [
            &["-T", "a", "b"][..],
            &["--no-target-directory", "a", "b"][..],
            &["-T", "-T", "a", "b"][..],
            &["a", "b", "-T"][..],
        ] {
            let (_, dest, paths) = run_parse_dest(spelling);
            assert!(dest.no_directory, "{spelling:?}");
            assert_eq!(dest.directory, None, "{spelling:?}");
            assert_eq!(paths, ["a", "b"], "{spelling:?}");
        }
    }

    /// The contradiction is *recorded*, not resolved, by the parser: both
    /// fields come back set, and the diagnostic is [`move_all`]'s. This is what
    /// [`Destination`]'s two fields buy — a single three-state field would have
    /// had to pick a winner here, and every winner obeys an option the user is
    /// about to be told is contradictory.
    #[test]
    fn both_target_options_together_survive_parsing() {
        let (_, dest, paths) = run_parse_dest(&["-T", "-t", "d", "a"]);
        assert_eq!(dest.directory, Some(OsString::from("d")));
        assert!(dest.no_directory);
        assert_eq!(paths, ["a"]);
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--force=yes", "a", "b"]);
        assert!(e.sentence.contains("doesn't allow"), "{:?}", e.sentence);
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
        match parse_args(&[OsString::from("-f"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(_, _, p) => assert_eq!(p, vec![bad, OsString::from("d")]),
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
    /// survive, so it is closed rather than noted.
    ///
    /// Windows has its own argument that no `String` can hold: an unpaired
    /// surrogate (a UTF-16 code unit in `0xD800..=0xDFFF` with no partner).
    /// `OsString` stores it as WTF-8, `String` cannot represent it, and
    /// `env::args()` unwraps on exactly it — the same `unwrap`, in the same std
    /// function, reached by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-f"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(_, _, p) => assert_eq!(p, vec![bad, OsString::from("d")]),
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

    // ------------------------------------------------ target_in_directory --

    #[test]
    fn target_file_into_dir() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("src/a.txt"));
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
        assert_eq!(rel, OsString::from("a.txt"));
    }

    #[test]
    fn target_nested_source_into_dir() {
        let (t, rel) = target_in_directory(Path::new("/tmp"), Path::new("a/b/c.txt"));
        assert_eq!(t, PathBuf::from("/tmp").join("c.txt"));
        assert_eq!(rel, OsString::from("c.txt"));
    }

    /// Trailing slashes are decoration on the source, and GNU strips them from
    /// the relname (`strip_trailing_slashes`, `mv.c:541`) so that the set of
    /// already-written destinations is keyed on the name and not on how the
    /// operand was typed. `mv d/ x` and `mv d x` must collide with each other.
    #[test]
    fn a_trailing_slash_on_the_source_is_not_part_of_the_name() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/b///"));
        assert_eq!(t, PathBuf::from("dst").join("b"));
        assert_eq!(rel, OsString::from("b"));
    }

    /// Bug 3 in the module docs, now fixed the way GNU fixes it — which is by
    /// *not* special-casing it at all.
    ///
    /// The old code called `Path::file_name`, which answers `None` for a name
    /// ending in `..`, and `unwrap_or_default()` turned that into
    /// `dst.join("")` == `dst`: a silent request to rename `a`'s **parent**
    /// onto `dst`. The fix that followed refused the operand outright, which
    /// was safe but still not GNU: GNU appends the component verbatim, so the
    /// target is the literal `dst/..`. That name then fails on its own merits —
    /// `EEXIST`, and with `-T` `EBUSY` — with a diagnostic naming a path the
    /// user can recognise.
    #[test]
    fn a_source_ending_in_dotdot_appends_dotdot_verbatim() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/.."));
        assert_eq!(t, PathBuf::from("dst").join(".."));
        assert_eq!(rel, OsString::from(".."));

        let (t, rel) = target_in_directory(Path::new("dst"), Path::new(".."));
        assert_eq!(t, PathBuf::from("dst").join(".."));
        assert_eq!(rel, OsString::from(".."));
    }

    /// And the same for `.`, which `Path::file_name` also answers `None` for.
    #[test]
    fn a_source_ending_in_dot_appends_dot_verbatim() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/."));
        assert_eq!(t, PathBuf::from("dst").join("."));
        assert_eq!(rel, OsString::from("."));
    }

    // ------------------------------------------------------------ moving --

    /// A private directory for one test, removed when the binding drops.
    ///
    /// Delegated to `scratchdir` rather than hand-rolled, for the reason spelled
    /// out at `cp.rs`'s copy of this helper: the hand-rolled version built child
    /// paths with `Path::join`, which uses the host's `\` on this development
    /// box, and this file's own [`split_entry`] — like every path function in
    /// the tree — treats `/` as the only separator and `\` as an ordinary byte
    /// in a filename.
    fn scratch(stem: &str) -> ScratchDir {
        ScratchDir::new(&format!("mv_test_{stem}"))
    }

    /// `move_all` plus whatever it wrote to its error sink.
    fn mv(paths: &[&Path]) -> (bool, String) {
        let (ok, _, err) = mv_flags(MvFlags::default(), paths);
        (ok, err)
    }

    /// `move_all` under given flags, plus both of its streams: `(ok, out, err)`.
    ///
    /// The answer source is empty, which [`coreutils::yesno`] reads as end of
    /// input and therefore as "no" — the same thing a `-i` in a script with no
    /// stdin gets. A test that means to answer uses [`mv_answering`].
    fn mv_flags(flags: MvFlags, paths: &[&Path]) -> (bool, String, String) {
        let (ok, out, err, _) = mv_answering(flags, &[], paths);
        (ok, out, err)
    }

    /// `mv_flags` with canned replies for `-i`'s prompts, and the count of
    /// prompts that actually consumed one: `(ok, out, err, asked)`.
    ///
    /// `asked` is what distinguishes "did not prompt" from "prompted and the
    /// wording changed", which asserting on the transcript alone cannot.
    fn mv_answering(
        flags: MvFlags,
        replies: &[&str],
        paths: &[&Path],
    ) -> (bool, String, String, usize) {
        mv_to(flags, Destination::default(), replies, paths)
    }

    /// [`mv_answering`] with a [`Destination`] other than the default, which is
    /// how `-t` and `-T` are exercised without going through argv.
    fn mv_to(
        flags: MvFlags,
        dest: Destination,
        replies: &[&str],
        paths: &[&Path],
    ) -> (bool, String, String, usize) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(replies);
        let mut copied = Copied::default();
        let ok = {
            let mut job = Job {
                flags: &flags,
                out: &mut out,
                err: &mut err,
                answers: &mut answers,
                copied: &mut copied,
                // Read here, once per `mv(…)` rather than once per process, so
                // that a test which sets the mask around a move gets the mask it
                // set. `cp.rs`'s test helper reads it in the same place for the
                // same reason.
                umask: coreutils::umask::current(),
            };
            move_all(&mut job, &dest, &owned)
        };
        (
            ok,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
            answers.consumed(),
        )
    }

    /// The name a scratch path prints as inside a `--verbose` line.
    ///
    /// The lines carry whole paths, and a scratch directory's is
    /// machine-specific, so the assertions below compare against this rather
    /// than against a literal. It goes through [`quoteaf_os`] for the same
    /// reason the real line does — a temp directory on the development host can
    /// hold a space, and then the quoted form is not the bare path.
    fn shown(p: &Path) -> String {
        quoteaf_os(p).to_string()
    }

    #[test]
    fn renames_a_file() {
        let dir = scratch("rename");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = mv(&[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"hello");
    }

    #[test]
    fn moves_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.path("a");
        let sub = dir.path("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
    }

    // ----------------------------------------------------------- verbose --

    #[test]
    fn verbose_names_a_rename_on_stdout() {
        let dir = scratch("v_rename");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, out, err) = mv_flags(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            &[&a, &b],
        );
        assert!(ok, "{err}");
        assert_eq!(err, "");
        // GNU: `renamed 'a' -> 'b'`, on stdout, one line, both names quoted.
        assert_eq!(out, format!("renamed {} -> {}\n", shown(&a), shown(&b)));
    }

    /// Silence is the default, and it has to be *complete* silence: a `mv` that
    /// wrote its line unconditionally would break every pipeline that reads
    /// `mv`'s stdout expecting nothing.
    #[test]
    fn without_verbose_stdout_stays_empty() {
        let dir = scratch("v_quiet");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, out, err) = mv_flags(MvFlags::default(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(out, "");
    }

    /// `--strip-trailing-slashes` reaches the move itself, not just the parse.
    ///
    /// Asserted through `-v` because that is the one effect this option has
    /// that does not need a symlink to see, and symlinks are what the
    /// interesting cases are made of — a trailing slash changes what a *name*
    /// resolves to only when the last component is a symlink, and the Windows
    /// development host cannot create one without a privilege the test runner
    /// does not have. The behaviour those cases pin lives in `mv-diff.sh` §21,
    /// measured against GNU on Linux; what is checked here is the half that is
    /// platform-independent: the stripped name is what gets moved and what gets
    /// announced.
    ///
    /// Note that with the option on, the filesystem never sees the slash at
    /// all — which is exactly why this test is safe on a host whose `rename`
    /// treats a trailing separator differently from Linux's.
    #[test]
    fn strip_trailing_slashes_moves_the_stripped_name() {
        let dir = scratch("v_strip");
        let src = dir.path("s");
        let into = dir.path("d");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&into).unwrap();
        let with_slash = PathBuf::from(format!("{}/", src.display()));
        let landed = into.join("s");
        let (ok, out, err, _) = mv_to(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            Destination {
                strip_slashes: true,
                ..Destination::default()
            },
            &[],
            &[&with_slash, &into],
        );
        assert!(ok, "{err}");
        assert_eq!(err, "");
        // `shown(&src)` and not `shown(&with_slash)`: the announced source is
        // the operand as the option left it.
        assert_eq!(
            out,
            format!("renamed {} -> {}\n", shown(&src), shown(&landed))
        );
        assert!(landed.is_dir());
    }

    /// One line per source, in operand order, and each names the *target it
    /// landed on* rather than the directory that was asked for.
    #[test]
    fn verbose_names_each_source_moved_into_a_directory() {
        let dir = scratch("v_into_dir");
        let a = dir.path("a");
        let b = dir.path("b");
        let sub = dir.path("sub");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, out, err) = mv_flags(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            &[&a, &b, &sub],
        );
        assert!(ok, "{err}");
        assert_eq!(
            out,
            format!(
                "renamed {} -> {}\nrenamed {} -> {}\n",
                shown(&a),
                shown(&sub.join("a")),
                shown(&b),
                shown(&sub.join("b"))
            )
        );
    }

    /// An overwrite goes through the *second* rename — the one keyed on
    /// `EEXIST` — and that path has its own `announce` call. Left out, a plain
    /// `mv -v a b` with `b` already there would move the file and say nothing.
    #[test]
    fn verbose_names_an_overwriting_rename() {
        let dir = scratch("v_overwrite");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, out, err) = mv_flags(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            &[&a, &b],
        );
        assert!(ok, "{err}");
        assert_eq!(out, format!("renamed {} -> {}\n", shown(&a), shown(&b)));
        assert_eq!(fs::read(&b).unwrap(), b"new");
    }

    /// A move that failed prints no line at all: GNU's `emit_verbose` sits
    /// *inside* the `rename_errno == 0` arm (`copy.c:2761`). The cross-device
    /// fallback is the one exception, and it is deliberate — see [`move_one`].
    #[test]
    fn a_failed_move_announces_nothing() {
        let dir = scratch("v_failure");
        let missing = dir.path("nosuch");
        let b = dir.path("b");
        let (ok, out, err) = mv_flags(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            &[&missing, &b],
        );
        assert!(!ok);
        assert_eq!(out, "");
        assert!(err.contains("cannot stat"), "{err}");
    }

    /// A refusal is a failure too, and it is reported on stderr while stdout
    /// stays empty — the two streams do not mix.
    #[test]
    fn a_refused_overwrite_announces_nothing() {
        let dir = scratch("v_refused");
        let a = dir.path("a");
        fs::write(&a, b"x").unwrap();
        let (ok, out, err) = mv_flags(
            MvFlags {
                verbose: true,
                ..MvFlags::default()
            },
            &[&a, &a],
        );
        assert!(!ok);
        assert_eq!(out, "");
        assert!(err.contains("are the same file"), "{err}");
    }

    /// The cross-device pair, pinned at the sentence level.
    ///
    /// It is asserted here and not through [`move_all`] because nothing in this
    /// test suite — or in `scripts/mv-diff.sh`, which says so at its head — can
    /// produce an `EXDEV`: that needs two filesystems, and both have one. So the
    /// branch that emits these is covered only by its wording, which is still
    /// the half that goes wrong silently. `copied` and `renamed` are different
    /// verbs for a reason, and `removed` is a different *sentence* — one name,
    /// no arrow — because it comes from `rm -v` rather than from `mv`.
    #[test]
    fn the_cross_device_pair_reads_as_rm_and_cp_write_it() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&[]);
        let flags = MvFlags {
            verbose: true,
            ..MvFlags::default()
        };
        let mut copied = Copied::default();
        let mut job = Job {
            flags: &flags,
            out: &mut out,
            err: &mut err,
            answers: &mut answers,
            copied: &mut copied,
            umask: coreutils::umask::current(),
        };
        announce(
            &mut job,
            "copied",
            Path::new("g"),
            Path::new("/other/g"),
            None,
        );
        announce_removed(&mut job, Path::new("g"));
        assert_eq!(
            String::from_utf8_lossy(&out),
            "copied 'g' -> '/other/g'\nremoved 'g'\n"
        );
        assert!(err.is_empty());
    }

    /// Both halves obey the flag. [`announce_removed`] having its own early
    /// return is easy to forget, and forgetting it makes a plain `mv` across a
    /// filesystem boundary print `removed 'g'` at a user who asked for nothing.
    #[test]
    fn neither_verbose_sentence_is_printed_unasked() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let mut answers = Canned::new(&[]);
        let flags = MvFlags::default();
        let mut copied = Copied::default();
        let mut job = Job {
            flags: &flags,
            out: &mut out,
            err: &mut err,
            answers: &mut answers,
            copied: &mut copied,
            umask: coreutils::umask::current(),
        };
        announce(&mut job, "renamed", Path::new("a"), Path::new("b"), None);
        announce(&mut job, "copied", Path::new("a"), Path::new("b"), None);
        announce_removed(&mut job, Path::new("a"));
        assert!(out.is_empty());
    }

    // ----------------------------------------------------- -i / -f / -n --

    /// The one flag these three options set, spelled out so the tests below read
    /// as the option they are about rather than as a struct literal.
    fn overwrite(interactive: Interactive) -> MvFlags {
        MvFlags {
            interactive,
            ..MvFlags::default()
        }
    }

    /// `-n` refuses, says so, and **fails**. The exit status is the surprising
    /// half — "did not overwrite" sounds like a success, and Ubuntu's patched
    /// `mv` agrees, which is exactly why this is pinned; see
    /// `design-decisions.md` §726. Upstream 9.4 exits 1 and this follows
    /// upstream.
    #[test]
    fn no_clobber_refuses_and_fails() {
        let dir = scratch("n_refuses");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, out, err, asked) =
            mv_answering(overwrite(Interactive::AlwaysNo), &["y\n"], &[&a, &b]);
        assert!(!ok);
        assert_eq!(out, "");
        assert_eq!(err, format!("mv: not replacing {}\n", shown(&b)));
        assert_eq!(asked, 0, "-n is a decision, not a question");
        // Neither end moved: the source is still there, which is the part a
        // `rename_succeeded` bug would break rather than the diagnostic.
        assert_eq!(fs::read(&a).unwrap(), b"new");
        assert_eq!(fs::read(&b).unwrap(), b"old");
    }

    /// `-n` is about *existing* destinations only. A `mv -n` onto a free name is
    /// an ordinary move, silent and successful.
    #[test]
    fn no_clobber_over_a_free_name_just_moves() {
        let dir = scratch("n_free");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"x").unwrap();
        let (ok, out, err) = mv_flags(overwrite(Interactive::AlwaysNo), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!((out.as_str(), err.as_str()), ("", ""));
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"x");
    }

    /// `-i` asks on stderr and does what it is told. The prompt has no trailing
    /// newline — it ends `? ` so the answer is typed on the same line — and the
    /// accepted answers are gnulib's `^[yY]`, which is why `yes` and `Y` are in
    /// the table beside `y` and `n` is not merely "the other one".
    #[test]
    fn interactive_asks_and_obeys_the_answer() {
        let dir = scratch("i_answers");
        for (i, (reply, expect_moved)) in [
            ("y\n", true),
            ("yes\n", true),
            ("Y", true),
            ("n\n", false),
            ("no\n", false),
            ("", false),
        ]
        .into_iter()
        .enumerate()
        {
            let a = dir.path(&format!("src{i}"));
            let b = dir.path(&format!("dst{i}"));
            fs::write(&a, b"new").unwrap();
            fs::write(&b, b"old").unwrap();
            let (ok, out, err, asked) =
                mv_answering(overwrite(Interactive::AskUser), &[reply], &[&a, &b]);
            assert_eq!(asked, 1, "reply {reply:?}");
            assert_eq!(
                err,
                format!("mv: overwrite {}? ", shown(&b)),
                "reply {reply:?}"
            );
            assert_eq!(out, "", "reply {reply:?}");
            assert_eq!(ok, expect_moved, "reply {reply:?}");
            assert_eq!(a.exists(), !expect_moved, "reply {reply:?}");
            let want: &[u8] = if expect_moved { b"new" } else { b"old" };
            assert_eq!(fs::read(&b).unwrap(), want, "reply {reply:?}");
        }
    }

    /// End of input is a "no". `mv -i a b < /dev/null` in a script must not
    /// silently overwrite because nobody was there to say no.
    #[test]
    fn interactive_takes_silence_for_no() {
        let dir = scratch("i_silence");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, _, err, asked) = mv_answering(overwrite(Interactive::AskUser), &[], &[&a, &b]);
        assert!(!ok);
        assert_eq!(asked, 1, "it asked; there was simply no one to answer");
        assert_eq!(err, format!("mv: overwrite {}? ", shown(&b)));
        assert_eq!(fs::read(&b).unwrap(), b"old");
    }

    /// `-f` is the whole of `abandon_move`'s `I_ALWAYS_YES` arm: no question,
    /// under any circumstance the other arms would have asked in. `stdin_tty` is
    /// on here precisely because that is the state in which an unflagged `mv`
    /// *would* prompt for an unwritable destination.
    #[test]
    fn force_never_asks() {
        let dir = scratch("f_silent");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let flags = MvFlags {
            interactive: Interactive::AlwaysYes,
            stdin_tty: true,
            ..MvFlags::default()
        };
        let (ok, out, err, asked) = mv_answering(flags, &["n\n"], &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!((out.as_str(), err.as_str(), asked), ("", "", 0));
        assert_eq!(fs::read(&b).unwrap(), b"new");
    }

    /// With **no option at all**, a destination the user cannot write is asked
    /// about — but only at a terminal, and in `mv`'s wording rather than `cp`'s.
    ///
    /// Measured through `script(1)` against 9.4:
    ///
    /// ```text
    /// mv: replace 'd', overriding mode 0444 (r--r--r--)?
    /// ```
    ///
    /// `cp` says `unwritable 'd' (mode 0444, r--r--r--); try anyway?` for the
    /// same file. The difference is upstream's `clears_destination`, which is
    /// `x->move_mode || …` and so is unconditionally true here: `cp` writes
    /// *through* the destination and will be refused by the kernel, while `mv`
    /// unlinks it, which the mode does not prevent. Getting this wrong would
    /// promise the user a refusal that is not going to come.
    #[cfg(unix)]
    #[test]
    fn the_unwritable_prompt_is_mvs_wording_not_cps() {
        use std::os::unix::fs::PermissionsExt;

        // Root writes any file, so the prompt never appears and there is
        // nothing to assert. Skipping beats asserting the wrong thing.
        if overwrite::can_write_any_file() {
            return;
        }
        let dir = scratch("unwritable");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        fs::set_permissions(&b, fs::Permissions::from_mode(0o444)).unwrap();

        let at_tty = MvFlags {
            stdin_tty: true,
            ..MvFlags::default()
        };
        let (ok, _, err, asked) = mv_answering(at_tty, &["n\n"], &[&a, &b]);
        assert!(!ok);
        assert_eq!(asked, 1);
        assert_eq!(
            err,
            format!(
                "mv: replace {}, overriding mode 0444 (r--r--r--)? ",
                shown(&b)
            )
        );

        // Same file, same mode, no terminal: GNU moves it without a word. The
        // prompt is a courtesy to a human, not a permission check.
        let (ok, out, err, asked) = mv_answering(MvFlags::default(), &["n\n"], &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!((out.as_str(), err.as_str(), asked), ("", "", 0));
        assert_eq!(fs::read(&b).unwrap(), b"new");
    }

    /// The other half of that arm: a destination that *is* writable is not asked
    /// about even at a terminal. Without the `!writable_destination` test, a
    /// plain interactive `mv` would prompt on every overwrite — which is `-i`,
    /// and is not the default.
    #[test]
    fn a_writable_destination_is_not_asked_about_at_a_terminal() {
        let dir = scratch("writable_tty");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let flags = MvFlags {
            stdin_tty: true,
            ..MvFlags::default()
        };
        let (ok, out, err, asked) = mv_answering(flags, &["n\n"], &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!((out.as_str(), err.as_str(), asked), ("", "", 0));
        assert_eq!(fs::read(&b).unwrap(), b"new");
    }

    /// `-n` and `-i` sit **above** the directory sentences, so a directory source
    /// onto a plain file is refused or asked about rather than being told it
    /// cannot overwrite a non-directory. Measured: `mv -n d g` says `not
    /// replacing 'g'` and `mv -i d g` asks `overwrite 'g'? `.
    ///
    /// This is where `mv` parts company with `cp`, whose equivalent block is
    /// guarded by `! S_ISDIR (src_mode)`.
    #[test]
    fn the_refusals_come_before_the_directory_sentences() {
        let dir = scratch("refusal_order");
        let d = dir.path("d");
        let g = dir.path("g");
        fs::create_dir(&d).unwrap();
        fs::write(&g, b"old").unwrap();

        let (ok, _, err, asked) = mv_answering(overwrite(Interactive::AlwaysNo), &[], &[&d, &g]);
        assert!(!ok);
        assert_eq!(err, format!("mv: not replacing {}\n", shown(&g)));
        assert_eq!(asked, 0);

        let (ok, _, err, asked) =
            mv_answering(overwrite(Interactive::AskUser), &["n\n"], &[&d, &g]);
        assert!(!ok);
        assert_eq!(err, format!("mv: overwrite {}? ", shown(&g)));
        assert_eq!(asked, 1);

        // And with neither, the directory sentence is what comes out.
        let (ok, _, err) = mv_flags(MvFlags::default(), &[&d, &g]);
        assert!(!ok);
        assert_eq!(
            err,
            format!(
                "mv: cannot overwrite non-directory {} with directory {}\n",
                shown(&g),
                shown(&d)
            )
        );
        assert!(
            d.is_dir() && g.is_file(),
            "nothing moved in any of the three"
        );
    }

    /// `-n` displaces the same-file refusal rather than losing to it, because
    /// upstream guards the whole `same_file_ok` call with `x->interactive !=
    /// I_ALWAYS_NO`. Measured: `mv f f` says `'f' and 'f' are the same file`,
    /// `mv -n f f` says `not replacing 'f'`. Both exit 1, so only the wording
    /// tells them apart — which is why the wording is the assertion.
    #[test]
    fn no_clobber_displaces_the_same_file_refusal() {
        let dir = scratch("n_same_file");
        let f = dir.path("f");
        fs::write(&f, b"x").unwrap();

        let (ok, _, err) = mv_flags(MvFlags::default(), &[&f, &f]);
        assert!(!ok);
        assert_eq!(
            err,
            format!("mv: {} and {} are the same file\n", shown(&f), shown(&f))
        );

        let (ok, _, err) = mv_flags(overwrite(Interactive::AlwaysNo), &[&f, &f]);
        assert!(!ok);
        assert_eq!(err, format!("mv: not replacing {}\n", shown(&f)));
        assert_eq!(fs::read(&f).unwrap(), b"x");
    }

    /// A refusal ends that *operand*, not the command. Measured: `mv -n a b sub`
    /// with `sub/a` present moves `b`, leaves `a`, and exits 1.
    #[test]
    fn one_refusal_does_not_abandon_the_other_operands() {
        let dir = scratch("n_partial");
        let a = dir.path("a");
        let b = dir.path("b");
        let sub = dir.path("sub");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"two").unwrap();
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("a"), b"old").unwrap();

        let (ok, _, err) = mv_flags(overwrite(Interactive::AlwaysNo), &[&a, &b, &sub]);
        assert!(!ok);
        assert_eq!(
            err,
            format!("mv: not replacing {}\n", shown(&sub.join("a")))
        );
        assert!(a.is_file(), "the refused source stays");
        assert_eq!(fs::read(sub.join("a")).unwrap(), b"old");
        assert!(!b.exists(), "the other operand still moved");
        assert_eq!(fs::read(sub.join("b")).unwrap(), b"two");
    }

    /// The prompt goes to stderr and the `--verbose` line to stdout, in the one
    /// command that produces both. A prompt on stdout would be invisible to a
    /// user running `mv -iv … > log` — they would sit at an apparently hung
    /// terminal — and a verbose line on stderr would corrupt every script that
    /// treats `mv`'s stderr as its error report.
    #[test]
    fn verbose_and_interactive_use_different_streams() {
        let dir = scratch("iv_streams");
        let p = dir.path("p");
        let q = dir.path("q");
        fs::write(&p, b"new").unwrap();
        fs::write(&q, b"old").unwrap();
        let flags = MvFlags {
            verbose: true,
            interactive: Interactive::AskUser,
            ..MvFlags::default()
        };
        let (ok, out, err, asked) = mv_answering(flags, &["y\n"], &[&p, &q]);
        assert!(ok, "{err}");
        assert_eq!(asked, 1);
        assert_eq!(out, format!("renamed {} -> {}\n", shown(&p), shown(&q)));
        assert_eq!(err, format!("mv: overwrite {}? ", shown(&q)));
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, err) = mv(&[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
        let _ = err;
    }

    /// GNU distinguishes "no operands" from "one operand" and names the one it
    /// got; the old code printed `missing operand` for both.
    #[test]
    fn one_operand_names_it() {
        let (ok, err) = mv(&[Path::new("solo")]);
        assert!(!ok);
        assert!(err.contains("missing destination file operand"), "{err}");
        assert!(err.contains("solo"), "{err}");
    }

    /// The wording is GNU's `error (EXIT_FAILURE, err, _("target %s"), …)`
    /// (`mv.c:495`) — `target 'c': Not a directory`, the operand named and the
    /// reason appended by the same `errno`-printing path every other diagnostic
    /// uses. This file used to compose its own sentence, `target 'c' is not a
    /// directory`, which reads better and is not what anything greps for.
    #[test]
    fn several_sources_need_a_directory() {
        let dir = scratch("not_a_dir");
        let a = dir.path("a");
        let b = dir.path("b");
        let c = dir.path("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();
        fs::write(&c, b"z").unwrap();
        let (ok, err) = mv(&[&a, &b, &c]);
        assert!(!ok);
        assert!(err.contains("Not a directory"), "{err}");
        // Nothing was touched.
        assert!(a.is_file() && b.is_file() && c.is_file());
    }

    // ------------------------------------------------------------ -t and -T --

    /// `-t DIR`, as a [`Destination`].
    fn into_dir(dir: &Path) -> Destination {
        Destination {
            directory: Some(dir.as_os_str().to_owned()),
            no_directory: false,
            strip_slashes: false,
        }
    }

    /// `-T`. Named for what it does rather than for the letter: the destination
    /// is a name, not a directory to fill.
    fn as_name() -> Destination {
        Destination {
            directory: None,
            no_directory: true,
            strip_slashes: false,
        }
    }

    fn mv_dest(dest: Destination, paths: &[&Path]) -> (bool, String) {
        let (ok, _, err, _) = mv_to(MvFlags::default(), dest, &[], paths);
        (ok, err)
    }

    /// The shape `-t` exists for and that nothing else can spell: a *single*
    /// operand that still goes inside the directory rather than being taken for
    /// the destination.
    #[test]
    fn a_target_directory_takes_every_operand_as_a_source() {
        let dir = scratch("t_one");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let a = dir.path("a");
        fs::write(&a, b"A").unwrap();

        let (ok, err) = mv_dest(into_dir(&dest), &[&a]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"A");
        assert!(!a.exists());
    }

    /// The collision check is not a property of the trailing-directory shape —
    /// `move_into_directory` is one function precisely so that `-t` cannot
    /// arrive at it by a route that skips the bookkeeping.
    #[test]
    fn a_target_directory_still_refuses_a_shared_basename() {
        let dir = scratch("t_collide");
        let dest = dir.path("dest");
        let one = dir.path("one");
        let two = dir.path("two");
        for d in [&dest, &one, &two] {
            fs::create_dir(d).unwrap();
        }
        fs::write(one.join("same"), b"1").unwrap();
        fs::write(two.join("same"), b"2").unwrap();

        let (ok, err) = mv_dest(into_dir(&dest), &[&one.join("same"), &two.join("same")]);
        assert!(!ok);
        assert!(err.contains("will not overwrite just-created"), "{err}");
        // The first arrival is the one that survives, and the second is still
        // where it was.
        assert_eq!(fs::read(dest.join("same")).unwrap(), b"1");
        assert!(two.join("same").is_file());
    }

    /// With `-t` the destination is not the last operand, so *one* operand is a
    /// whole command and zero is the "no operands at all" diagnostic — not the
    /// "missing destination" one, which would name an operand that was given.
    #[test]
    fn a_target_directory_makes_one_operand_enough() {
        let dir = scratch("t_count");
        let dest = dir.path("dest");
        fs::create_dir(&dest).unwrap();
        let (ok, err) = mv_dest(into_dir(&dest), &[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
        assert!(!err.contains("missing destination"), "{err}");
    }

    /// `target directory 'x': …`, not the trailing operand's bare `target 'x':
    /// …`. The user named this one as a directory, so being told it is not one
    /// is the whole answer; the other sentence has to also say *which* role the
    /// operand was being read in.
    #[test]
    fn a_target_directory_that_is_not_one_is_named_as_a_target_directory() {
        let dir = scratch("t_notdir");
        let a = dir.path("a");
        let plain = dir.path("plain");
        fs::write(&a, b"A").unwrap();
        fs::write(&plain, b"P").unwrap();

        let (ok, err) = mv_dest(into_dir(&plain), &[&a]);
        assert!(!ok);
        assert!(err.contains("target directory"), "{err}");
        assert!(err.contains("Not a directory"), "{err}");
        assert!(
            a.is_file(),
            "nothing may move once the directory is refused"
        );

        let (ok, err) = mv_dest(into_dir(&dir.path("nosuch")), &[&a]);
        assert!(!ok);
        assert!(err.contains("target directory"), "{err}");
        assert!(err.contains("No such file"), "{err}");
    }

    /// `-T` against a directory is a refusal, and the very same operands
    /// *without* `-T` are a move into it. The pair is the whole point of the
    /// option, so it is asserted as a pair.
    #[test]
    fn no_target_directory_refuses_the_directory_the_default_would_fill() {
        let dir = scratch("cap_t_dir");
        let a = dir.path("a");
        let d = dir.path("d");
        fs::write(&a, b"A").unwrap();
        fs::create_dir(&d).unwrap();

        let (ok, err) = mv_dest(as_name(), &[&a, &d]);
        assert!(!ok);
        assert!(
            err.contains("cannot overwrite directory"),
            "{err}: -T must not move it inside"
        );
        assert!(!d.join("a").exists());

        let (ok, err) = mv_dest(Destination::default(), &[&a, &d]);
        assert!(ok, "{err}");
        assert!(d.join("a").is_file());
    }

    /// The case `-T` is *for*: replacing a directory rather than nesting it
    /// inside itself. `mv newdir olddir` puts `newdir` at `olddir/newdir`;
    /// `mv -T newdir olddir` makes `newdir` *be* `olddir`.
    #[test]
    fn no_target_directory_replaces_an_empty_directory() {
        let dir = scratch("cap_t_replace");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(src.join("inside"), b"I").unwrap();

        let (ok, err) = mv_dest(as_name(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!src.exists());
        assert_eq!(fs::read(dst.join("inside")).unwrap(), b"I");
    }

    /// `-T` says the destination is exactly one name, so a third operand is not
    /// a third source — there is nowhere for it to go.
    #[test]
    fn no_target_directory_refuses_a_third_operand() {
        let dir = scratch("cap_t_extra");
        let (ok, err) = mv_dest(as_name(), &[&dir.path("a"), &dir.path("b"), &dir.path("c")]);
        assert!(!ok);
        assert!(err.contains("extra operand"), "{err}");
        assert!(err.contains('c'), "{err}");
    }

    /// Both options at once is a diagnostic of its own, and it is raised
    /// *before* `-t`'s directory is stat'd — so a directory that does not exist
    /// is not what gets reported. That order is GNU's and this is the only case
    /// that can see it.
    #[test]
    fn both_target_options_are_refused_before_the_directory_is_looked_at() {
        let dir = scratch("cap_t_combine");
        let dest = Destination {
            directory: Some(dir.path("nosuchdir").into_os_string()),
            no_directory: true,
            strip_slashes: false,
        };
        let (ok, err) = mv_dest(dest, &[&dir.path("a")]);
        assert!(!ok);
        assert!(
            err.contains("cannot combine --target-directory (-t) and --no-target-directory (-T)"),
            "{err}"
        );
        assert!(
            !err.contains("nosuchdir"),
            "the missing directory must not be reported: {err}"
        );
        // And it is a plain line, not a usage error, so there is no referral.
        assert!(!err.contains("Try 'mv"), "{err}");
    }

    /// Bug 2 in the module docs: with `-f` the old code printed nothing here and
    /// still exited 1. `-f` is not even a parameter any more, so the only way to
    /// get silence would be to lose the diagnostic for everyone.
    #[test]
    fn a_missing_source_is_reported() {
        let dir = scratch("missing_src");
        let (ok, err) = mv(&[&dir.path("nope"), &dir.path("dst")]);
        assert!(!ok);
        assert!(err.contains("cannot stat"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("partial");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        let a = dir.path("a");
        let c = dir.path("c");
        fs::write(&a, b"a").unwrap();
        fs::write(&c, b"c").unwrap();
        let (ok, err) = mv(&[&a, &dir.path("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still move");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
    }

    /// Bug 3, end to end. Before the fix this asked the kernel to rename the
    /// scratch directory itself onto `sub`.
    ///
    /// The wording is GNU's, and it is worth saying why it is *this* wording
    /// rather than a refusal of the operand. `inner/..` and `sub/..` are both
    /// the scratch directory, so the two operands name one file, and "are the
    /// same file" is both true and the most informative thing available. It is
    /// also not a special case anywhere in the code: the target is built by
    /// appending `..` verbatim, and the ordinary same-file check then catches
    /// it. Measured against GNU coreutils 9.4, which prints exactly this.
    #[test]
    fn a_dotdot_source_does_not_move_the_parent() {
        let dir = scratch("dotdot");
        let inner = dir.path("inner");
        let sub = dir.path("sub");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&inner.join(".."), &sub]);
        assert!(!ok);
        assert!(err.contains("are the same file"), "{err}");
        assert!(dir.dir().is_dir(), "the parent must still be where it was");
        assert!(inner.is_dir());
    }

    /// A dangling symlink is a thing that exists and can be renamed. The old
    /// code's `fs::copy` fallback read *through* it, so this reported "No such
    /// file or directory" about a link that was plainly there.
    #[test]
    #[cfg(unix)]
    fn moves_a_dangling_symlink() {
        let dir = scratch("dangling");
        let link = dir.path("link");
        std::os::unix::fs::symlink(dir.path("nowhere"), &link).unwrap();
        let moved = dir.path("moved");
        let (ok, err) = mv(&[&link, &moved]);
        assert!(ok, "{err}");
        assert!(
            fs::symlink_metadata(&moved)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::symlink_metadata(&link).is_err());
    }

    /// Bug 4's unit: the cross-device fallback must reproduce a symlink *as* a
    /// symlink. `fs::rename` would not have gone through here, so the fallback
    /// is called directly — there is no portable way to make two filesystems
    /// appear in a unit test.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_relinks_rather_than_copying_the_target() {
        let dir = scratch("xdev_symlink");
        let real = dir.path("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.path("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let moved = dir.path("moved");

        let meta = fs::symlink_metadata(&link).unwrap();
        copy_across_devices(&link, &moved, &meta, &mut cross_device_run(&mut Vec::new())).unwrap();

        let moved_meta = fs::symlink_metadata(&moved).unwrap();
        assert!(
            moved_meta.file_type().is_symlink(),
            "a symlink must arrive as a symlink, not as a copy of its target"
        );
        assert_eq!(fs::read_link(&moved).unwrap(), real);
        assert!(fs::symlink_metadata(&link).is_err(), "source must be gone");
        assert_eq!(fs::read(&real).unwrap(), b"contents", "target untouched");
    }

    #[test]
    fn the_cross_device_fallback_moves_a_plain_file() {
        let dir = scratch("xdev_file");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"bytes").unwrap();
        let meta = fs::symlink_metadata(&a).unwrap();
        copy_across_devices(&a, &b, &meta, &mut cross_device_run(&mut Vec::new())).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"bytes");
    }

    /// A move is meant to be indistinguishable from a rename, and a rename does
    /// not re-date the file. This is the unit `known-issues.md` →
    /// `B-MVS-CROSS-DEVICE-FALLBACK-THROWS-AWAY-THE-TIMES-AND-THE-OWNER` asked
    /// for: it pins the source's stamp, calls the function, and asserts the
    /// destination's is the same one — which the differential harness can only
    /// see through the whole program and only on a machine with two filesystems.
    ///
    /// The stamp is deliberately in the past. "Not now" is the only assertion
    /// that distinguishes a preserved time from a fresh one, and a time set to
    /// *now* would pass whether or not anything was preserved.
    #[test]
    fn the_cross_device_fallback_carries_the_times() {
        let dir = scratch("xdev_times");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"bytes").unwrap();

        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        fsattr::set_times(On::Path(&a, Link::Follow), Times::both(long_ago)).unwrap();

        let meta = fs::symlink_metadata(&a).unwrap();
        let want = meta.modified().unwrap();
        copy_across_devices(&a, &b, &meta, &mut cross_device_run(&mut Vec::new())).unwrap();

        let got = fs::symlink_metadata(&b).unwrap().modified().unwrap();
        assert_eq!(got, want, "the copy must keep the source's stamp");
    }

    /// The mode, including the bits `fs::copy` used to carry and the set-user-ID
    /// bit it did not. The `chmod` is written *after* the `chown`, so a run as
    /// root — where the `chown` actually happens and clears `S_ISUID` — is the
    /// case this pins; as an ordinary user the `chown` is skipped and the bit
    /// would survive either ordering.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_carries_the_set_user_id_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("xdev_mode");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"bytes").unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o4741)).unwrap();

        let meta = fs::symlink_metadata(&a).unwrap();
        copy_across_devices(&a, &b, &meta, &mut cross_device_run(&mut Vec::new())).unwrap();

        let got = fs::symlink_metadata(&b).unwrap().permissions().mode() & 0o7777;
        assert_eq!(got, 0o4741, "every bit, set-user-ID included");
    }

    /// Put a `user.` attribute on a file, or say the filesystem underneath the
    /// scratch directory has none. `/tmp` is usually ext4 and usually does; a
    /// tmpfs built without `CONFIG_TMPFS_XATTR` does not, and a test that failed
    /// there would be reporting the kernel's build options rather than this
    /// `mv`. Same helper, same reasoning, as `cp`'s.
    #[cfg(unix)]
    fn seed_xattr(path: &Path, name: &[u8], value: &[u8]) -> bool {
        fsattr::set_xattr(On::Path(path, Link::NoFollow), name, value).is_ok()
    }

    /// What `path` has under `name`, or `None` if it has nothing.
    #[cfg(unix)]
    fn xattr_of(path: &Path, name: &[u8]) -> Option<Vec<u8>> {
        fsattr::get_xattr(On::Path(path, Link::NoFollow), name).ok()
    }

    /// The unit `known-issues.md` →
    /// `B-MVS-CROSS-DEVICE-FALLBACK-DROPS-EXTENDED-ATTRIBUTES` asked for. A
    /// rename carries every attribute by not touching the inode at all, so the
    /// fallback that stands in for it has to put them back by hand.
    ///
    /// The value is deliberately not text. An attribute's value is arbitrary
    /// bytes, and a copy that round-tripped it through UTF-8 would corrupt
    /// exactly this while passing any test written with a printable string.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_carries_an_extended_attribute() {
        const VALUE: &[u8] = b"\x00\xff\x80not text";
        let dir = scratch("xdev_xattr");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"bytes").unwrap();
        if !seed_xattr(&a, b"user.tag", VALUE) {
            return;
        }

        let meta = fs::symlink_metadata(&a).unwrap();
        let mut err = Vec::new();
        copy_across_devices(&a, &b, &meta, &mut cross_device_run(&mut err)).unwrap();

        assert_eq!(String::from_utf8_lossy(&err), "");
        assert_eq!(
            xattr_of(&b, b"user.tag").as_deref(),
            Some(VALUE),
            "the attribute must arrive with the bytes"
        );
    }

    /// The unit for the `S_IWUSR` that [`create_destination`] ORs in. A `0444`
    /// source produces a `0444` destination, and Linux's `xattr_permission`
    /// wants *write* access to an inode before it will set an attribute on it —
    /// so without the widening this `setxattr` fails with `EACCES`, the
    /// attribute is silently absent and the failure is printed.
    ///
    /// Both halves are asserted, because the widening would be just as wrong if
    /// it leaked: the mode at the end has to be the source's `0444` again, the
    /// extra bit having left with the rest of the temporary mode when
    /// [`fsattr::copy_permissions`] wrote the real one.
    ///
    /// As root the widening is skipped and `setxattr` bypasses the check, so
    /// this passes either way there. It discriminates as an ordinary user, which
    /// is who runs the suite.
    #[test]
    #[cfg(unix)]
    fn a_read_only_source_still_gets_its_attributes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("xdev_xattr_ro");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::write(&a, b"bytes").unwrap();
        if !seed_xattr(&a, b"user.tag", b"v") {
            return;
        }
        fs::set_permissions(&a, fs::Permissions::from_mode(0o444)).unwrap();

        let meta = fs::symlink_metadata(&a).unwrap();
        let mut err = Vec::new();
        copy_across_devices(&a, &b, &meta, &mut cross_device_run(&mut err)).unwrap();

        assert_eq!(String::from_utf8_lossy(&err), "");
        assert_eq!(
            xattr_of(&b, b"user.tag").as_deref(),
            Some(&b"v"[..]),
            "a read-only file must still receive its attributes"
        );
        assert_eq!(
            fs::symlink_metadata(&b).unwrap().permissions().mode() & 0o7777,
            0o444,
            "and the write bit that allowed it must not survive"
        );
    }

    /// A symlink has its own modification time, and `mv` carries it: the
    /// `utimensat` in the shared tail is done with `AT_SYMLINK_NOFOLLOW`, so it
    /// stamps the link and not what the link names. The target is checked too,
    /// because stamping through the link would be the silent failure.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_carries_a_links_own_time() {
        let dir = scratch("xdev_link_times");
        let real = dir.path("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.path("link");
        std::os::unix::fs::symlink("real", &link).unwrap();

        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        fsattr::set_times(On::Path(&link, Link::NoFollow), Times::both(long_ago)).unwrap();
        let target_stamp = fs::symlink_metadata(&real).unwrap().modified().unwrap();

        let meta = fs::symlink_metadata(&link).unwrap();
        let want = meta.modified().unwrap();
        let moved = dir.path("moved");
        copy_across_devices(&link, &moved, &meta, &mut cross_device_run(&mut Vec::new())).unwrap();

        let got = fs::symlink_metadata(&moved).unwrap().modified().unwrap();
        assert_eq!(got, want, "the link's own stamp must come across");
        assert_eq!(
            fs::symlink_metadata(&real).unwrap().modified().unwrap(),
            target_stamp,
            "and must not have been written through the link"
        );
    }

    /// The destination is *replaced*, not written through, and this is the step
    /// that makes that true. Without it the copy opens the existing name and
    /// truncates it, so the file that comes out keeps the old one's inode — and
    /// with it its mode and every other name linked to it.
    ///
    /// Measured against GNU 9.4 across a real filesystem boundary: `mv far/f g`,
    /// with `g` one of a linked pair, left both `g` and `g2` rewritten and still
    /// linked, where GNU leaves `g2` untouched at its old size. The harness case
    /// is in `scripts/mv-diff.sh` §22; this is the unit underneath it.
    #[test]
    fn clearing_the_destination_breaks_its_other_links() {
        let dir = scratch("xdev_clear");
        let one = dir.path("one");
        let two = dir.path("two");
        fs::write(&one, b"original").unwrap();
        fs::hard_link(&one, &two).unwrap();

        clear_destination(&one).unwrap();

        assert!(fs::symlink_metadata(&one).is_err(), "the name is free");
        assert_eq!(
            fs::read(&two).unwrap(),
            b"original",
            "the other name still reads what it always did"
        );
    }

    /// `ENOENT` is the ordinary answer, not a failure: the usual cross-device
    /// move is onto a name that was never there, and `-b` has just renamed away
    /// any name that was. GNU spells the test `errno != ENOENT`.
    #[test]
    fn clearing_a_destination_that_is_not_there_succeeds() {
        let dir = scratch("xdev_clear_absent");
        clear_destination(&dir.path("never-existed")).unwrap();
    }

    /// A symlink at the destination is unlinked as itself. Following it would
    /// clear whatever it names — a file in another directory entirely — and
    /// leave the name being moved onto still occupied.
    #[test]
    #[cfg(unix)]
    fn clearing_a_symlink_destination_does_not_follow_it() {
        let dir = scratch("xdev_clear_link");
        let real = dir.path("real");
        let link = dir.path("link");
        fs::write(&real, b"kept").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        clear_destination(&link).unwrap();

        assert!(fs::symlink_metadata(&link).is_err(), "the link is gone");
        assert_eq!(fs::read(&real).unwrap(), b"kept", "its target is not");
    }

    /// Anything else is reported, so that the caller can print
    /// `inter-device move failed: … unable to remove target`. A directory is the
    /// reachable shape: `unlink` refuses one, and this `mv` is holding a
    /// non-directory source, so the two kinds disagreeing is exactly the case
    /// that must not be silently skipped.
    #[test]
    fn clearing_a_destination_that_will_not_go_is_reported() {
        let dir = scratch("xdev_clear_dir");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inside"), b"x").unwrap();

        clear_destination(&sub).unwrap_err();

        assert!(sub.join("inside").is_file(), "nothing may be removed");
    }

    /// Not implemented, and it says so rather than moving part of the tree.
    #[test]
    fn the_cross_device_fallback_refuses_a_directory() {
        let dir = scratch("xdev_dir");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inside"), b"x").unwrap();
        let meta = fs::symlink_metadata(&sub).unwrap();
        let e = copy_across_devices(
            &sub,
            &dir.path("elsewhere"),
            &meta,
            &mut cross_device_run(&mut Vec::new()),
        )
        .unwrap_err();
        assert_eq!(e.err.kind(), io::ErrorKind::Unsupported);
        assert!(sub.join("inside").is_file(), "nothing may be moved");
    }

    // ------------------------------------------------- the src_to_dest table --

    /// Two names for one inode, both moved onto names that already exist. The
    /// destinations must come out as one file, and the transcript must show how:
    /// the second operand is *linked* to the first result and then unlinked, not
    /// renamed. Measured against GNU 9.4, which prints exactly these three lines.
    ///
    /// A same-filesystem test on purpose — no copy happens here at all, which is
    /// the point. The table is consulted before the rename that is allowed to
    /// replace (`copy.c:2662`), so it changes what a move that never leaves the
    /// disk *says*. `scripts/mv-diff.sh` §22 has the cross-device twin.
    #[test]
    #[cfg(unix)]
    fn a_second_name_for_one_inode_is_linked_to_where_the_first_landed() {
        let dir = scratch("linked_pair");
        let (a, b, d) = (dir.path("a"), dir.path("b"), dir.path("d"));
        fs::create_dir(&d).unwrap();
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(d.join("a"), b"old").unwrap();
        fs::write(d.join("b"), b"old").unwrap();

        let flags = MvFlags {
            verbose: true,
            ..MvFlags::default()
        };
        let (ok, out, err) = mv_flags(flags, &[&a, &b, &d]);

        assert!(ok, "{err}");
        assert!(err.is_empty(), "{err}");
        assert_eq!(
            out,
            format!(
                "renamed {} -> {}\nremoved {}\nremoved {}\n",
                quoteaf_os(&a),
                quoteaf_os(d.join("a")),
                quoteaf_os(d.join("b")),
                quoteaf_os(&b)
            )
        );
        assert!(!a.exists() && !b.exists(), "both sources are gone");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"hello");
        assert_eq!(
            file_id(&d.join("a"), &fs::symlink_metadata(d.join("a")).unwrap()),
            file_id(&d.join("b"), &fs::symlink_metadata(d.join("b")).unwrap()),
            "one inode, two names — what a rename would have left"
        );
    }

    /// The `st_nlink == 1` arm, which is the one that is easy to leave out. By
    /// the time the third name is reached the first two have been removed and the
    /// count is back to one, so a rule spelled "only when the count is above one"
    /// would copy — or here, rename — it into a separate file.
    #[test]
    #[cfg(unix)]
    fn the_last_link_is_found_even_though_its_count_is_back_to_one() {
        let dir = scratch("linked_three");
        let (a, b, c, d) = (dir.path("a"), dir.path("b"), dir.path("c"), dir.path("d"));
        fs::create_dir(&d).unwrap();
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::hard_link(&a, &c).unwrap();
        for name in ["a", "b", "c"] {
            fs::write(d.join(name), b"old").unwrap();
        }

        let (ok, err) = mv(&[&a, &b, &c, &d]);

        assert!(ok, "{err}");
        assert_eq!(nlink(&fs::symlink_metadata(d.join("c")).unwrap()), 3);
    }

    /// Nothing is recorded when the rename *succeeds* (`copy.c:2663`), so a pair
    /// moved onto free names is two renames and says so. The tree is the same
    /// either way — a rename keeps links together by itself — which is precisely
    /// why a table consulted unconditionally would go unnoticed until the
    /// transcript was read.
    #[test]
    #[cfg(unix)]
    fn a_rename_that_works_records_nothing_and_stays_a_rename() {
        let dir = scratch("linked_free");
        let (a, b, d) = (dir.path("a"), dir.path("b"), dir.path("d"));
        fs::create_dir(&d).unwrap();
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let flags = MvFlags {
            verbose: true,
            ..MvFlags::default()
        };
        let (ok, out, err) = mv_flags(flags, &[&a, &b, &d]);

        assert!(ok, "{err}");
        assert_eq!(
            out,
            format!(
                "renamed {} -> {}\nrenamed {} -> {}\n",
                quoteaf_os(&a),
                quoteaf_os(d.join("a")),
                quoteaf_os(&b),
                quoteaf_os(d.join("b"))
            )
        );
        assert_eq!(nlink(&fs::symlink_metadata(d.join("b")).unwrap()), 2);
    }

    /// `--update` records into the table even though it is skipping, so the
    /// second skip links over a destination it was asked to leave alone
    /// (`copy.c:2380`). Both sources survive — the skip promised that — and
    /// `d/b`'s own bytes do not, which is upstream's own "even if it was a newer
    /// separate file" in a test.
    ///
    /// The destinations are written *after* the sources, so they are no older by
    /// construction and `-u` skips both without a stamp having to be forced.
    #[test]
    #[cfg(unix)]
    fn an_update_skip_still_links_the_second_name_over_what_it_spared() {
        let dir = scratch("update_linked");
        let (a, b, d) = (dir.path("a"), dir.path("b"), dir.path("d"));
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::create_dir(&d).unwrap();
        fs::write(d.join("a"), b"newer").unwrap();
        fs::write(d.join("b"), b"newer2").unwrap();

        let flags = MvFlags {
            update: true,
            verbose: true,
            ..MvFlags::default()
        };
        let (ok, out, err) = mv_flags(flags, &[&a, &b, &d]);

        assert!(ok, "{err}");
        assert!(err.is_empty(), "{err}");
        // No `renamed` and no `copied`: both operands were skipped. The one line
        // comes from inside `force_link`, replacing a name that was in use.
        assert_eq!(out, format!("removed {}\n", quoteaf_os(d.join("b"))));
        assert!(a.exists() && b.exists(), "a skip keeps both sources");
        assert_eq!(fs::read(d.join("a")).unwrap(), b"newer");
        assert_eq!(
            fs::read(d.join("b")).unwrap(),
            b"newer",
            "the spared file was replaced by a link to the other one"
        );
        assert_eq!(nlink(&fs::symlink_metadata(d.join("b")).unwrap()), 2);
    }

    /// The two routes into one table, side by side. `d/a` is newer so the first
    /// operand is skipped and merely recorded; `d/b` is older so the second is
    /// *not* skipped and reaches the ordinary `earlier_file` block, which links
    /// it to `d/a` — a destination this command never wrote — and then removes
    /// its source. One command, one inode, and two different answers to whether
    /// the source lives.
    #[test]
    #[cfg(unix)]
    fn a_recorded_skip_is_found_by_an_operand_that_is_not_skipped() {
        let dir = scratch("update_mixed");
        let (a, b, d) = (dir.path("a"), dir.path("b"), dir.path("d"));
        fs::create_dir(&d).unwrap();
        fs::write(d.join("b"), b"old").unwrap();
        // Forced back a decade so it is older than the sources on any
        // granularity; `d/a` is written afterwards and so is newer than both.
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(978_307_200);
        fsattr::set_times(On::Path(&d.join("b"), Link::Follow), Times::both(old)).unwrap();
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(d.join("a"), b"newer").unwrap();

        let flags = MvFlags {
            update: true,
            verbose: true,
            ..MvFlags::default()
        };
        let (ok, out, err) = mv_flags(flags, &[&a, &b, &d]);

        assert!(ok, "{err}");
        assert!(err.is_empty(), "{err}");
        assert_eq!(
            out,
            format!(
                "removed {}\nremoved {}\n",
                quoteaf_os(d.join("b")),
                quoteaf_os(&b)
            ),
            "the link's replacement, then the removal of the source that moved"
        );
        assert!(a.exists(), "the skipped operand kept its source");
        assert!(!b.exists(), "the linked one did not");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"newer");
        assert_eq!(
            file_id(&d.join("a"), &fs::symlink_metadata(d.join("a")).unwrap()),
            file_id(&d.join("b"), &fs::symlink_metadata(d.join("b")).unwrap()),
        );
    }

    /// Two names for one inode are still two *files* when only one of them is
    /// asked for. The table is keyed on the inode, and the source that stays
    /// behind must keep its bytes — a table that unlinked the inode rather than
    /// the named link would lose it.
    #[test]
    #[cfg(unix)]
    fn moving_one_name_of_a_pair_leaves_the_other_alone() {
        let dir = scratch("linked_one");
        let (a, b, g) = (dir.path("a"), dir.path("b"), dir.path("g"));
        fs::write(&a, b"hello").unwrap();
        fs::hard_link(&a, &b).unwrap();

        let (ok, err) = mv(&[&a, &g]);

        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"hello");
        assert_eq!(fs::read(&g).unwrap(), b"hello");
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must move like any other.
    #[test]
    #[cfg(unix)]
    fn moves_a_file_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let dir = scratch("nonutf8");
        let mut name = dir.dir().to_path_buf().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let src = PathBuf::from(OsString::from_vec(name));
        fs::write(&src, b"x").unwrap();
        let dst = dir.path("ok");
        let (ok, err) = mv(&[&src, &dst]);
        assert!(ok, "{err}");
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"x");
    }

    #[test]
    fn is_cross_device_does_not_fire_on_an_ordinary_error() {
        assert!(!is_cross_device(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_cross_device(&io::Error::from(io::ErrorKind::NotFound)));
    }

    #[test]
    fn is_cross_device_fires_on_the_platform_errno() {
        assert!(is_cross_device(&io::Error::from_raw_os_error(
            CROSS_DEVICE_ERRNO
        )));
    }

    // ------------------------------------------------------- -u / --update --

    /// Stamp `p`'s modification time at `secs` past the epoch.
    ///
    /// The nanoseconds are deliberately non-zero and *identical* for every
    /// stamp, so that "the same second" here means the same instant to the
    /// nanosecond. A comparison that silently truncated to whole seconds would
    /// pass either way and is what this rules out when a test stamps two files
    /// the same.
    fn stamp(p: &Path, secs: u64) {
        use std::fs::FileTimes;
        let t = std::time::UNIX_EPOCH + std::time::Duration::new(secs, 246_813_579);
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_times(FileTimes::new().set_accessed(t).set_modified(t))
            .unwrap();
    }

    /// `-u`, as flags.
    fn updating() -> MvFlags {
        MvFlags {
            update: true,
            ..MvFlags::default()
        }
    }

    /// Two files, `src` stamped at `src_secs` and `dst` at `dst_secs`, moved
    /// under `flags`: `(ok, err, dst contents)`.
    fn timed_move(
        stem: &str,
        flags: MvFlags,
        src_secs: u64,
        dst_secs: u64,
    ) -> (bool, String, Vec<u8>) {
        let dir = scratch(stem);
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        stamp(&a, src_secs);
        stamp(&b, dst_secs);
        let (ok, out, err) = mv_flags(flags, &[&a, &b]);
        assert_eq!(out, "", "-u is silent without -v");
        let dst = fs::read(&b).unwrap();
        // Every outcome here is all-or-nothing: either the move happened, and
        // then `b` holds the source's bytes and `a` is gone, or it did not, and
        // then `b` is untouched and `a` is still there. Upstream's skip sets
        // `*rename_succeeded = true` (`copy.c:2373`), which is precisely the
        // flag that tells `mv` not to unlink the source afterwards — so a
        // mistake there breaks this pairing rather than either half alone, and
        // it is checked on every case rather than in one dedicated test.
        assert_eq!(a.exists(), dst == b"old", "{stem}");
        (ok, err, dst)
    }

    /// The whole of `--update`'s parse arm, as a table: which of the two fields
    /// each spelling writes.
    ///
    /// The two rows worth reading twice are `all` and `none`, which both turn
    /// `update` **off** — `--update=all` is "replace everything", which is what
    /// no option at all already means, and it is here so that it can *cancel* an
    /// earlier `-u` or `-i`.
    #[test]
    fn the_three_update_words_write_two_fields() {
        for (argv, update, interactive) in [
            (&["-u"][..], true, Interactive::Unspecified),
            (&["--update"][..], true, Interactive::Unspecified),
            (&["--update=older"][..], true, Interactive::Unspecified),
            (&["--update=all"][..], false, Interactive::Unspecified),
            (&["--update=none"][..], false, Interactive::AlwaysSkip),
            // gnulib's `argmatch` is a prefix match, so a unique prefix of any
            // of the three works — the same rule that makes `--up` resolve to
            // `--update`, applied one level down to its argument.
            (&["--update=o"][..], true, Interactive::Unspecified),
            (&["--update=n"][..], false, Interactive::AlwaysSkip),
            (&["--update=a"][..], false, Interactive::Unspecified),
            // Last wins, as for the `-i`/`-f`/`-n` field, and across the two
            // spellings: `--update=all` cancels an earlier `-u`.
            (&["-u", "--update=all"][..], false, Interactive::Unspecified),
            (&["--update=all", "-u"][..], true, Interactive::Unspecified),
            (&["--update=none", "-u"][..], true, Interactive::AlwaysSkip),
            // `--update=all` writes `interactive` too, so it cancels a `-i`
            // that came before it and not one that comes after.
            (&["-i", "--update=all"][..], false, Interactive::Unspecified),
            (&["--update=all", "-i"][..], false, Interactive::AskUser),
            (
                &["-i", "--update=older"][..],
                true,
                Interactive::Unspecified,
            ),
        ] {
            let mut items: Vec<&str> = argv.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let (flags, _, paths) = run_parse_dest(&items);
            assert_eq!(flags.update, update, "{argv:?}");
            assert_eq!(flags.interactive, interactive, "{argv:?}");
            assert_eq!(paths, ["a", "b"], "{argv:?}");
        }
    }

    /// `-n` beats `-u` in **both** orders, and by two different mechanisms —
    /// which is the only reason it is worth a test of its own rather than a row
    /// in the table above.
    ///
    /// A `-n` that came *first* is a guard on the `--update` arm
    /// (`mv.c:378`, `/* -n takes precedence.  */`); a `-n` that came *last* is
    /// the clamp after the loop (`mv.c:509`). Either alone would leave one of
    /// these two orders wrong.
    #[test]
    fn no_clobber_beats_update_in_both_orders() {
        for argv in [
            &["-n", "-u"][..],
            &["-u", "-n"][..],
            &["-n", "--update=older"][..],
            &["--update=older", "-n"][..],
            &["-n", "--update=none"][..],
            &["--update=none", "-n"][..],
        ] {
            let mut items: Vec<&str> = argv.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let (flags, _, _) = run_parse_dest(&items);
            assert!(!flags.update, "{argv:?}");
            assert_eq!(flags.interactive, Interactive::AlwaysNo, "{argv:?}");
        }
    }

    /// The one place the two mechanisms come apart, and it makes `-u` and
    /// `--update=older` — which `--help` calls equivalent — behave differently.
    ///
    /// With a later `-i` to lift `interactive` back off `AlwaysNo`, the clamp no
    /// longer fires, so what survives is whatever the guard let through. The
    /// guard is on the long form only, so the long form loses its `update` and
    /// the short one keeps it. Measured against 9.4.
    #[test]
    fn the_precedence_guard_is_on_the_long_form_only() {
        let (long, _, _) = run_parse_dest(&["-n", "--update=older", "-i", "a", "b"]);
        assert!(!long.update, "the guard swallowed --update=older");
        assert_eq!(long.interactive, Interactive::AskUser);

        let (short, _, _) = run_parse_dest(&["-n", "-u", "-i", "a", "b"]);
        assert!(short.update, "-u is not guarded");
        assert_eq!(short.interactive, Interactive::AskUser);
    }

    /// An argument that names no word is `argmatch`'s error, listing the three.
    #[test]
    fn an_unknown_update_word_is_refused_with_the_list() {
        let e = fail(&["--update=sometimes", "a", "b"]);
        assert!(
            e.sentence.contains("invalid argument") && e.sentence.contains("--update"),
            "{e:?}"
        );
        for word in ["all", "none", "older"] {
            assert!(e.sentence.contains(word), "{word} missing from {e:?}");
        }
        // An empty argument is a prefix of all three, which disagree, so it is
        // the *ambiguous* message and not the invalid one.
        let e = fail(&["--update=", "a", "b"]);
        assert!(e.sentence.contains("ambiguous argument"), "{e:?}");
    }

    /// …except under `-n`, where upstream never looks at the word at all,
    /// because `XARGMATCH` sits inside the block the precedence guard skips.
    /// So a typo that would be fatal on its own is accepted in silence.
    #[test]
    fn no_clobber_makes_an_unknown_update_word_pass_unread() {
        let (flags, _, paths) = run_parse_dest(&["-n", "--update=sometimes", "a", "b"]);
        assert_eq!(flags.interactive, Interactive::AlwaysNo);
        assert!(!flags.update);
        assert_eq!(paths, ["a", "b"]);
    }

    /// The skip: a destination newer than the source is left, the source is
    /// left, nothing is said, and the command **succeeds**. The exit status is
    /// the whole difference from `-n`.
    #[test]
    fn update_leaves_a_newer_destination_and_succeeds() {
        let (ok, err, dst) = timed_move("u_newer", updating(), 1_000_000, 2_000_000);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(dst, b"old");
    }

    /// Equal counts as "not older", which is what makes running the same
    /// `mv -u` twice a no-op rather than a move back and forth.
    #[test]
    fn update_leaves_a_destination_of_the_same_age() {
        let (ok, err, dst) = timed_move("u_equal", updating(), 1_000_000, 1_000_000);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(dst, b"old");
    }

    /// And the other half: an older destination is replaced exactly as it would
    /// be without the option.
    #[test]
    fn update_replaces_an_older_destination() {
        let (ok, err, dst) = timed_move("u_older", updating(), 2_000_000, 1_000_000);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(dst, b"new");
    }

    /// `--update=none` against `-n`, on the same fixture, which is the pair the
    /// `--help` text is describing when it says "skipped files do not induce a
    /// failure".
    #[test]
    fn update_none_skips_where_no_clobber_fails() {
        let (ok, err, dst) = timed_move(
            "u_none",
            overwrite(Interactive::AlwaysSkip),
            2_000_000,
            1_000_000,
        );
        assert!(ok, "{err}");
        assert_eq!(err, "", "--update=none is silent");
        assert_eq!(dst, b"old", "and it does not replace, newer source or not");

        let (ok, err, dst) = timed_move(
            "n_none",
            overwrite(Interactive::AlwaysNo),
            2_000_000,
            1_000_000,
        );
        assert!(!ok);
        assert!(err.contains("not replacing"), "{err}");
        assert_eq!(dst, b"old");
    }

    /// `-u` sits *after* the same-file check, so a hard-link pair still gets the
    /// sentence about it even though the two stamps are necessarily equal and
    /// `-u` would have skipped the move silently.
    #[cfg(unix)]
    #[test]
    fn update_does_not_reach_a_hard_link_pair() {
        let dir = scratch("u_samefile");
        let f = dir.path("f");
        let l = dir.path("l");
        fs::write(&f, b"x").unwrap();
        fs::hard_link(&f, &l).unwrap();
        let (ok, _, err) = mv_flags(updating(), &[&f, &l]);
        assert!(!ok);
        assert!(err.contains("are the same file"), "{err}");
        assert!(f.exists() && l.exists());
    }

    /// `--update=none` *does* skip it, because it is one of the two values the
    /// same-file check is guarded against — and it says nothing, where `-n` on
    /// the same pair says `not replacing`.
    #[cfg(unix)]
    #[test]
    fn update_none_swallows_the_same_file_sentence() {
        let dir = scratch("none_samefile");
        let f = dir.path("f");
        let l = dir.path("l");
        fs::write(&f, b"x").unwrap();
        fs::hard_link(&f, &l).unwrap();
        let (ok, _, err) = mv_flags(overwrite(Interactive::AlwaysSkip), &[&f, &l]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(f.exists() && l.exists());
    }

    /// A **directory** source is exempt from `-u`'s skip — upstream's
    /// `!S_ISDIR (src_mode)` — so an old directory still replaces an empty new
    /// one rather than being silently left behind.
    #[test]
    fn update_does_not_skip_a_directory_source() {
        let dir = scratch("u_dir");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(src.join("inside"), b"x").unwrap();

        let (ok, _, err) = mv_flags(updating(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!src.exists());
        assert!(dst.join("src").join("inside").is_file());
    }

    /// A skipped destination is not recorded as one this command line created,
    /// so the `will not overwrite just-created` check cannot fire on it —
    /// upstream returns at `skip:` before `record_file` (`copy.c:2445`).
    ///
    /// Without that, `mv --update=none one/same two/same dir` would skip the
    /// first and *fail* on the second, which is the one thing `--update=none`
    /// promises not to do.
    #[test]
    fn a_skipped_destination_is_not_recorded_as_just_created() {
        let dir = scratch("skip_norecord");
        let dest = dir.path("dest");
        let one = dir.path("one");
        let two = dir.path("two");
        for d in [&dest, &one, &two] {
            fs::create_dir(d).unwrap();
        }
        fs::write(one.join("same"), b"1").unwrap();
        fs::write(two.join("same"), b"2").unwrap();
        fs::write(dest.join("same"), b"kept").unwrap();

        let (ok, _, err, _) = mv_to(
            overwrite(Interactive::AlwaysSkip),
            into_dir(&dest),
            &[],
            &[&one.join("same"), &two.join("same")],
        );
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(dest.join("same")).unwrap(), b"kept");
        // Both sources survive: a skip moves nothing.
        assert!(one.join("same").is_file() && two.join("same").is_file());
    }

    /// `-u` over a destination that is *not there* is not a skip at all — the
    /// speculative rename succeeds and the question never arises. Pinned
    /// because the obvious implementation of "is the destination older" has to
    /// invent an answer for a destination with no timestamp.
    #[test]
    fn update_over_nothing_is_an_ordinary_move() {
        let dir = scratch("u_fresh");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();
        let (ok, _, err) = mv_flags(updating(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"A");
        assert!(!a.exists());
    }

    // ------------------------------------------- -b / --backup / -S / -T --

    /// `-b` with the default control and suffix, as flags.
    fn backing_up() -> MvFlags {
        MvFlags {
            backup: backup::Backup::new(BackupType::Simple, b"~".to_vec()),
            ..MvFlags::default()
        }
    }

    /// `--backup=numbered`, as flags.
    fn numbering() -> MvFlags {
        MvFlags {
            backup: backup::Backup::new(BackupType::Numbered, b"~".to_vec()),
            ..MvFlags::default()
        }
    }

    /// The same, plus `-v`, for the tests that read the announcement.
    fn verbosely(mut flags: MvFlags) -> MvFlags {
        flags.verbose = true;
        flags
    }

    /// Four spellings ask for a backup, not two.
    ///
    /// `-S`/`--suffix` set `make_backups` as well as the suffix (`mv.c:405`),
    /// so `mv -S .bak a b` backs `b` up even though no `-b` was typed. That is
    /// the fact about this option that a from-memory implementation gets wrong,
    /// and it is measured: GNU 9.4 given `-S .bak a b` leaves `b.bak` behind.
    ///
    /// The two environment variables the parse also reads —
    /// `$VERSION_CONTROL` and `$SIMPLE_BACKUP_SUFFIX` — are deliberately *not*
    /// tested here. Setting a variable is process-global and these tests run in
    /// parallel threads, so a test that set one would change the answer of any
    /// other test that happened to parse at the same moment. The harness's §18
    /// covers them instead, one process per case.
    #[test]
    fn the_four_ways_to_ask_for_a_backup() {
        for argv in [
            &["-b"][..],
            &["--backup"][..],
            &["-S", ".bak"][..],
            &["--suffix=.bak"][..],
        ] {
            let mut items: Vec<&str> = argv.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let (flags, paths) = run_parse_full(&items);
            assert!(flags.backup.enabled(), "{argv:?}");
            assert_eq!(paths, vec!["a", "b"], "{argv:?}");
        }
        // And no option at all asks for nothing, which is the row that says
        // `enabled()` is not simply always true.
        assert!(!run_parse_full(&["a", "b"]).0.backup.enabled());
    }

    /// Which control word maps to which type, including the four aliases and
    /// the prefix matching gnulib's `argmatch` does on all of them.
    #[test]
    fn every_backup_control_word_and_its_alias() {
        for (word, want) in [
            ("none", BackupType::None),
            ("off", BackupType::None),
            ("numbered", BackupType::Numbered),
            ("t", BackupType::Numbered),
            ("existing", BackupType::NumberedExisting),
            ("nil", BackupType::NumberedExisting),
            ("simple", BackupType::Simple),
            ("never", BackupType::Simple),
            // Unambiguous prefixes. `n` is *not* one of them — it starts
            // `none`, `numbered` and `nil` alike — and is checked below.
            ("num", BackupType::Numbered),
            ("ex", BackupType::NumberedExisting),
            ("si", BackupType::Simple),
        ] {
            let (flags, _) = run_parse_full(&[&format!("--backup={word}"), "a", "b"]);
            assert_eq!(flags.backup.kind(), want, "--backup={word}");
        }
        // A word that is in no entry, and a word that is the prefix of three,
        // are two different diagnostics — and both list the whole table, since
        // a rejection that does not say what was allowed is unactionable.
        for (word, wording) in [
            ("nosuchword", "invalid argument"),
            ("n", "ambiguous argument"),
        ] {
            let e = fail(&[&format!("--backup={word}"), "a", "b"]);
            assert!(e.sentence.contains(wording), "{word}: {e:?}");
            assert!(e.sentence.contains("‘existing’, ‘nil’"), "{word}: {e:?}");
        }
    }

    /// `--backup=none` asks for a backup and then asks for no backup, which is
    /// not a contradiction: `enabled()` is false and the option is still
    /// "given" for the purposes of the `-n` check below.
    #[test]
    fn backup_none_is_asked_for_and_does_nothing() {
        let (flags, _) = run_parse_full(&["--backup=none", "a", "b"]);
        assert_eq!(flags.backup.kind(), BackupType::None);
        assert!(!flags.backup.enabled());
    }

    /// A later bare `-b` does not erase an earlier `--backup=WORD`.
    ///
    /// `-b` writes only the flag; the word lives in a separate variable that
    /// nothing clears (`mv.c:344`). So the two options are not "last wins" the
    /// way `-i`/`-f`/`-n` are, and the order that looks like it should matter
    /// does not. `--backup=none` after `-b` *does* change the answer, because
    /// that one carries a word.
    #[test]
    fn a_later_bare_b_does_not_erase_the_word() {
        for argv in [
            &["--backup=numbered", "-b"][..],
            &["-b", "--backup=numbered"][..],
        ] {
            let mut items: Vec<&str> = argv.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let (flags, _) = run_parse_full(&items);
            assert_eq!(flags.backup.kind(), BackupType::Numbered, "{argv:?}");
        }
        let (flags, _) = run_parse_full(&["-b", "--backup=none", "a", "b"]);
        assert_eq!(flags.backup.kind(), BackupType::None);
    }

    /// `--backup` and `--no-clobber` are refused together (`mv.c:512`), and the
    /// test is on whether a backup was *asked for* rather than on what it
    /// resolved to — `--backup=none -n` is refused even though it would have
    /// done nothing. `-S` reaches the same check, since it sets the same flag.
    ///
    /// `--update=none` skips like `-n` does but writes a different value of the
    /// same field, so it is *not* caught: `mv --backup --update=none a b` is a
    /// legal line. That asymmetry is upstream's and is measured.
    #[test]
    fn backup_and_no_clobber_are_mutually_exclusive() {
        for argv in [
            &["-b", "-n"][..],
            &["-n", "-b"][..],
            &["-S", ".bak", "-n"][..],
            &["--backup=none", "-n"][..],
            &["--backup=numbered", "--no-clobber"][..],
        ] {
            let mut items: Vec<&str> = argv.to_vec();
            items.extend_from_slice(&["a", "b"]);
            let e = fail(&items);
            assert!(e.sentence.contains("mutually exclusive"), "{argv:?}: {e:?}");
        }
        // The one that looks the same and is not.
        let (flags, _) = run_parse_full(&["--backup", "--update=none", "a", "b"]);
        assert!(flags.backup.enabled());
        assert_eq!(flags.interactive, Interactive::AlwaysSkip);
    }

    /// The whole option in one move: the destination survives under a new name,
    /// the source arrives, and `-v` names the backup in the same line.
    #[test]
    fn a_backup_is_the_destination_under_a_new_name() {
        let dir = scratch("b_simple");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();

        let (ok, out, err) = mv_flags(verbosely(backing_up()), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(fs::read(dir.path("b~")).unwrap(), b"old");
        assert!(!a.exists());
        assert_eq!(
            out,
            format!(
                "renamed {} -> {} (backup: {})\n",
                shown(&a),
                shown(&b),
                shown(&dir.path("b~"))
            )
        );
    }

    /// A suffix other than `~`, which is the whole of what `-S` changes.
    #[test]
    fn the_suffix_names_the_backup() {
        let dir = scratch("b_suffix");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();

        let flags = MvFlags {
            backup: backup::Backup::new(BackupType::Simple, b".bak".to_vec()),
            ..MvFlags::default()
        };
        let (ok, _, err) = mv_flags(flags, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dir.path("b.bak")).unwrap(), b"old");
        assert!(!dir.path("b~").exists());
    }

    /// Nothing at the destination is neither an error nor a backup, and the
    /// verbose line has no `(backup: …)` clause at all.
    ///
    /// Pinned because the natural implementation asks the backup machinery for
    /// a name unconditionally and then has to decide what a rename of a file
    /// that is not there means. It means "no backup", not "failure".
    #[test]
    fn nothing_at_the_destination_means_no_backup() {
        let dir = scratch("b_fresh");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"A").unwrap();

        let (ok, out, err) = mv_flags(verbosely(backing_up()), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(out, format!("renamed {} -> {}\n", shown(&a), shown(&b)));
        assert!(!dir.path("b~").exists());
        assert_eq!(fs::read(&b).unwrap(), b"A");
    }

    /// Numbered backups count up from whatever is already there, which is the
    /// property that makes them the only kind safe to use twice on one name.
    #[test]
    fn numbered_backups_count_up() {
        let dir = scratch("b_numbered");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&b, b"old").unwrap();
        fs::write(dir.path("b.~1~"), b"one").unwrap();
        fs::write(dir.path("b.~2~"), b"two").unwrap();
        fs::write(&a, b"new").unwrap();

        let (ok, out, err) = mv_flags(verbosely(numbering()), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(out.ends_with(&format!("(backup: {})\n", shown(&dir.path("b.~3~")))));
        assert_eq!(fs::read(dir.path("b.~3~")).unwrap(), b"old");
        // The earlier ones are left exactly as they were.
        assert_eq!(fs::read(dir.path("b.~1~")).unwrap(), b"one");
    }

    /// A simple backup silently overwrites one already under that name — which
    /// is the loss the numbered forms exist to prevent, and is measured rather
    /// than assumed.
    #[test]
    fn a_simple_backup_overwrites_an_older_one() {
        let dir = scratch("b_clobber");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        fs::write(dir.path("b~"), b"older").unwrap();

        let (ok, _, err) = mv_flags(backing_up(), &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(dir.path("b~")).unwrap(), b"old");
    }

    /// Moving a file onto the name whose backup *is that file* is refused,
    /// because the backup would rename the source out from under the move and
    /// leave nothing at all.
    ///
    /// The refusal is on the name, so the same basename in another directory is
    /// fine, and `--backup=numbered` is exempt because it picks a fresh name.
    #[test]
    fn backing_up_over_the_source_is_refused() {
        let dir = scratch("b_selfdestruct");
        let b = dir.path("b");
        let bak = dir.path("b~");
        fs::write(&b, b"old").unwrap();
        fs::write(&bak, b"the source").unwrap();

        let (ok, _, err) = mv_flags(backing_up(), &[&bak, &b]);
        assert!(!ok);
        assert_eq!(
            err,
            format!(
                "mv: backing up {} might destroy source;  {} not moved\n",
                shown(&b),
                shown(&bak)
            )
        );
        // Nothing moved, which is the point of refusing.
        assert_eq!(fs::read(&b).unwrap(), b"old");
        assert_eq!(fs::read(&bak).unwrap(), b"the source");
    }

    /// The same line under `--backup=numbered`, which is allowed: the backup
    /// goes to `b.~1~` and the source is untouched by it.
    #[test]
    fn numbered_backups_are_exempt_from_the_source_check() {
        let dir = scratch("b_selfok");
        let b = dir.path("b");
        let bak = dir.path("b~");
        fs::write(&b, b"old").unwrap();
        fs::write(&bak, b"the source").unwrap();

        let (ok, _, err) = mv_flags(numbering(), &[&bak, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"the source");
        assert_eq!(fs::read(dir.path("b.~1~")).unwrap(), b"old");
    }

    /// A same-named file in another directory is not the destination's backup,
    /// so the check does not fire on it.
    #[test]
    fn the_source_check_compares_the_whole_name() {
        let dir = scratch("b_elsewhere");
        let other = dir.path("other");
        fs::create_dir(&other).unwrap();
        let b = dir.path("b");
        let src = other.join("b~");
        fs::write(&b, b"old").unwrap();
        fs::write(&src, b"new").unwrap();

        let (ok, _, err) = mv_flags(backing_up(), &[&src, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        assert_eq!(fs::read(dir.path("b~")).unwrap(), b"old");
    }

    /// `--backup` lifts the refusal to move a directory onto a non-directory.
    ///
    /// The refusal exists because the rename destroys the file at the
    /// destination; with a backup it is not destroyed, so upstream's own
    /// comment says the move is "ok only with --backup" (`copy.c:2455`). Both
    /// halves are asserted, because a lifted refusal that lifts unconditionally
    /// is the bug this guards against.
    #[test]
    fn backup_lifts_the_directory_onto_file_refusal() {
        let dir = scratch("b_dir_onto_file");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("inside"), b"x").unwrap();
        fs::write(&dst, b"old").unwrap();

        let (ok, err) = mv(&[&src, &dst]);
        assert!(!ok);
        assert!(err.contains("cannot overwrite non-directory"), "{err}");

        let (ok, _, err) = mv_flags(backing_up(), &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(dst.join("inside").is_file());
        assert_eq!(fs::read(dir.path("dst~")).unwrap(), b"old");
    }

    /// And the refusal the other way round — a non-directory onto a directory —
    /// which needs `-T` to be reached at all, since without it the directory is
    /// a place to move *into* rather than a thing to overwrite.
    #[test]
    fn backup_lifts_the_file_onto_directory_refusal() {
        let dir = scratch("b_file_onto_dir");
        let src = dir.path("src");
        let dst = dir.path("dst");
        fs::write(&src, b"new").unwrap();
        fs::create_dir(&dst).unwrap();

        let (ok, err) = mv_dest(as_name(), &[&src, &dst]);
        assert!(!ok);
        assert!(err.contains("cannot overwrite directory"), "{err}");

        let (ok, _, err, _) = mv_to(backing_up(), as_name(), &[], &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert!(dir.path("dst~").is_dir());
    }

    /// Only **numbered** backups lift the `will not overwrite just-created`
    /// refusal, and the reason is arithmetic rather than policy: a simple
    /// backup of `into/f` is `into/f~` every time, so the second source would
    /// back the first source's arrival up over the first source's own backup
    /// and destroy it. Upstream says as much — "it works fine if you use
    /// --backup=numbered" (`copy.c:2472`).
    ///
    /// `NumberedExisting` is *not* numbered for this purpose even when it would
    /// end up numbering, because the check reads the type that was asked for
    /// rather than the name that comes out. Measured against GNU 9.4, which
    /// refuses `--backup=existing` here.
    #[test]
    fn only_numbered_backups_lift_the_just_created_refusal() {
        fn attempt(stem: &str, flags: MvFlags) -> (bool, String) {
            let dir = scratch(stem);
            let dest = dir.path("into");
            let one = dir.path("from1");
            let two = dir.path("from2");
            for d in [&dest, &one, &two] {
                fs::create_dir(d).unwrap();
            }
            fs::write(one.join("f"), b"1").unwrap();
            fs::write(two.join("f"), b"2").unwrap();
            let (ok, _, err, _) = mv_to(
                flags,
                into_dir(&dest),
                &[],
                &[&one.join("f"), &two.join("f")],
            );
            // Whatever happened, the second source must not have vanished
            // without arriving: either it moved, or it is still where it was.
            assert!(
                dest.join("f").is_file() && (ok || two.join("f").is_file()),
                "{stem}: {err}"
            );
            (ok, err)
        }

        for (stem, flags) in [
            ("jc_plain", MvFlags::default()),
            ("jc_simple", backing_up()),
            (
                "jc_existing",
                MvFlags {
                    backup: backup::Backup::new(BackupType::NumberedExisting, b"~".to_vec()),
                    ..MvFlags::default()
                },
            ),
        ] {
            let (ok, err) = attempt(stem, flags);
            assert!(!ok, "{stem} should have refused");
            assert!(err.contains("will not overwrite just-created"), "{err}");
        }

        let (ok, err) = attempt("jc_numbered", numbering());
        assert!(ok, "{err}");
        assert_eq!(err, "");
    }

    /// A backup that cannot be made stops the move, and the file that was about
    /// to be overwritten is still there afterwards.
    ///
    /// The failure is induced portably by putting a **directory** where the
    /// backup name has to go: renaming a file onto a non-empty directory fails
    /// on every platform this builds for, whereas the obvious alternative —
    /// an unwritable parent — does nothing on Windows and nothing at all when
    /// the tests run as root. The message's `errno` half is therefore not
    /// asserted, only its prefix.
    ///
    /// That it fails *before* anything is written is the property worth having:
    /// backing up is the first step, so this costs nothing.
    #[test]
    fn a_backup_that_cannot_be_made_stops_the_move() {
        let dir = scratch("b_fails");
        let a = dir.path("a");
        let b = dir.path("b");
        let bak = dir.path("b~");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        fs::create_dir(&bak).unwrap();
        fs::write(bak.join("occupied"), b"x").unwrap();

        let (ok, _, err) = mv_flags(backing_up(), &[&a, &b]);
        assert!(!ok);
        assert!(
            err.starts_with(&format!("mv: cannot backup {}: ", shown(&b))),
            "{err}"
        );
        assert_eq!(fs::read(&a).unwrap(), b"new");
        assert_eq!(fs::read(&b).unwrap(), b"old");
    }

    /// `-i`'s prompt comes *before* the backup, so a refusal leaves the tree
    /// exactly as it was — no half-made `b~` either.
    #[test]
    fn a_refused_prompt_makes_no_backup() {
        let dir = scratch("b_prompt_no");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();

        let (ok, _, err, asked) = mv_answering(
            verbosely(MvFlags {
                interactive: Interactive::AskUser,
                ..backing_up()
            }),
            &["n"],
            &[&a, &b],
        );
        assert!(!ok);
        assert_eq!(asked, 1, "{err}");
        assert!(!dir.path("b~").exists());
        assert_eq!(fs::read(&b).unwrap(), b"old");
        assert_eq!(fs::read(&a).unwrap(), b"new");
    }

    /// Several sources into a directory: each destination gets its own backup,
    /// named inside the destination directory rather than beside the source.
    #[test]
    fn each_destination_in_a_directory_gets_its_own_backup() {
        let dir = scratch("b_into_dir");
        let dest = dir.path("into");
        fs::create_dir(&dest).unwrap();
        let a = dir.path("a");
        let c = dir.path("c");
        fs::write(&a, b"A").unwrap();
        fs::write(&c, b"C").unwrap();
        fs::write(dest.join("a"), b"old A").unwrap();
        fs::write(dest.join("c"), b"old C").unwrap();

        let (ok, _, err, _) = mv_to(backing_up(), into_dir(&dest), &[], &[&a, &c]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(dest.join("a")).unwrap(), b"A");
        assert_eq!(fs::read(dest.join("a~")).unwrap(), b"old A");
        assert_eq!(fs::read(dest.join("c")).unwrap(), b"C");
        assert_eq!(fs::read(dest.join("c~")).unwrap(), b"old C");
        // And nothing beside the sources, which is what a backup named from
        // the wrong path would leave.
        assert!(!dir.path("a~").exists() && !dir.path("c~").exists());
    }
}
