//! tar — tape archive utility.
//!
//! Usage: tar -c [-f ARCHIVE] [-v] [FILE...]   create archive
//!        tar -x [-f ARCHIVE] [-v] [-C DIR]    extract archive
//!        tar -t [-f ARCHIVE]                   list archive
//!
//! Each of those has GNU's long spelling too — `--create`, `--extract` (or
//! `--get`), `--list`, `--verbose`, `--file`, `--directory`,
//! `--preserve-permissions` (or `--same-permissions`) — abbreviable to any
//! unambiguous prefix, and `--` ends the options. `-?`/`--help`, `--usage` and
//! `--version` answer and exit 0. The other 160 long options GNU has are
//! **recognised and refused** rather than ignored; see
//! [`LONG_OPTIONS`] for why a table of names this tar does not implement is
//! load-bearing rather than decorative, and [`unsupported`] for why refusing
//! beats the two cheaper answers.
//!
//! Supports basic POSIX/ustar tar format (uncompressed).
//! Files > 8GB and paths > 255 chars are not supported.
//!
//! Create mode is unix-only (requires `mode`/`uid`/`gid`/`mtime` from
//! `MetadataExt`).  Listing and extraction are platform-independent at
//! the parsing level; the cross-platform helpers
//! (`parse_args`, `parse_octal`, `extract_string`, `TarHeader`,
//! `list_archive`, `strip_leading`) are exercised by unit tests on
//! every host.
//!
//! # An archive is untrusted input
//!
//! The member names in a tar file are chosen by whoever made it, and an
//! extractor that believes them will write wherever it is told. This one used
//! to: `fs::write(&name, ...)` with `name` straight out of the header, so a
//! member called `../../etc/passwd` or `/etc/shadow` was written there, and
//! `-C` was no protection at all — an absolute name ignores the current
//! directory entirely. That is the "tar slip" class of vulnerability, and
//! `tar -xf` on a downloaded archive is exactly the situation it exists for.
//! Two things now stand between the header and the filesystem, and they are
//! GNU's two rather than invented ones. `strip_leading` (GNU's
//! `safer_name_suffix`) cuts a name back past its last `..` component and
//! announces the cut, which turns `/etc/shadow` into `etc/shadow` and
//! `../../etc/passwd` into `etc/passwd`; `contains_dot_dot` then refuses
//! outright the member names that survive with a `..` still in them, since
//! `a/../b` is equivalent to `b` only if `a` is a real directory and not a
//! symlink. The third leg is [`is_delayed_target`]: a symlink pointing out of
//! the tree is withheld until every member has been written, so a `d -> /tmp`
//! followed by `d/pwned` cannot land in `/tmp`. See `known-issues.md` →
//! `B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY`.
//!
//! The related rule is that a failed write is never silent. Creating an
//! archive that could not be written, or extracting a member that could not
//! be created, exits 2 (GNU's fatal-error status), not 0.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
// `escape`, not `quotef`, and that is a deliberate departure from the house
// style of the other 85 bins. GNU tar calls `set_quoting_style (NULL,
// escape_quoting_style)` at startup, so *every* name it prints -- in a
// diagnostic, in `-t`, and in `-cv`/`-xv` -- comes out the same way: C escapes,
// octal for anything that is not a valid character, and no quotes at all.
// Measured: `tar: caf\351: Not found in archive`, where a `quotef`-shaped tar
// would have said `tar: 'caf'$'\351': Not found in archive`.
// `quote` is the one exception to that, and it is GNU's exception too: the
// link-target diagnostics (`Cannot hard link to ‘x’`) come out of gnulib's
// `quote()`, which is pinned to the *locale* style and so is unaffected by the
// `set_quoting_style` above. Measured side by side in one run of one command:
// `tar: esc: Cannot hard link to ‘etc/passwd’: ...` next to
// `tar: caf\351: Not found in archive`.
// `quoteaf` left with the hand-written option loop: every command-line
// diagnostic now comes from `coreutils::getopt`, which does its own rendering
// (glibc's straight-marked style, not gnulib's locale-aware one).
use coreutils::quote::{escape, escape_os, os_bytes, quote};
// Only the non-unix twin of `Dir` builds a path out of a member's bytes; on
// unix every component is handed to `openat` as it stands.
#[cfg(any(not(unix), test))]
use coreutils::quote::os_from_bytes;
use coreutils::stdfd;
#[cfg(unix)]
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

/// GNU tar's exit status for "a fatal error occurred". Used for every failure
/// that leaves the archive or the extracted tree incomplete, because a caller
/// that only sees 0 has no way to discover that half its files are missing.
const EXIT_FATAL: i32 = 2;

/// GNU tar's exit status for a *command line* that could not be parsed.
///
/// Not 2, and not 1: tar's argument parser is argp, and argp exits with
/// `EX_USAGE` (64) when an option is unknown or its argument is missing.
/// Measured: `tar -Q; echo $?` and `tar -cf; echo $?` both print 64, while
/// every runtime failure prints 2. The distinction is worth keeping because it
/// is the one a wrapper script can act on — 64 means "I typed it wrong", 2
/// means "the archive or the filesystem was the problem".
const EXIT_USAGE: i32 = 64;

/// The second line argp prints after any usage error, verbatim.
///
/// **Two commands, not one**, and that is why every call site here prints this
/// rather than [`getopt::Error::message`]. The shared getopt module ends its
/// diagnostics with gnulib's one-command `Try 'tar --help' for more
/// information.`, which is right for the 85 utilities that use `getopt_long`
/// directly and wrong for tar: argp supplies its own referral naming both
/// `--help` and `--usage`. The *sentences* are glibc's either way — argp calls
/// `getopt_long` to do the parsing — so only this last line differs. Measured
/// side by side: all six of tar's command-line errors end in this line.
const TRY_HELP: &str = "Try 'tar --help' or 'tar --usage' for more information.";

/// tar's name and its usage status, bound once so no diagnostic can drift.
///
/// 64, not 1 and not 2 — see [`EXIT_USAGE`]. It is passed here rather than
/// written into each message so that the two cannot disagree.
const TAR: Program = Program::new("tar", EXIT_USAGE);

/// The short options this tar implements, in `getopt` notation.
///
/// No leading `+`: tar **permutes**, so an option may follow an operand.
/// Measured — `tar -tf t.tar a --verbose` applies the `--verbose` and prints a
/// long listing, rather than treating it as a second member name.
///
/// `?` is a real option letter here, not the error return: argp gives `--help`
/// the short form `-?`, and `tar -?` prints the help and exits 0. The shared
/// parser looks the letter up by byte and has no special case for `?`, so
/// listing it is all that is needed.
const SHORT_OPTIONS: &str = "cxtvpkUf:C:?";

/// Every long option GNU tar 1.35 has — all 172 — in argp's own table order.
///
/// # Why the whole set, when this tar implements twelve of them
///
/// Because the table is what decides whether an abbreviation is *ambiguous*,
/// and an abbreviation is resolved against every name tar knows, not every name
/// tar acts on. Drop the 160 unimplemented entries and `tar --ex` stops being
/// an error and silently becomes `--extract` — GNU refuses it, listing
/// fourteen candidates. A table that lists only what it implements does not
/// merely give a worse message; it gives a *different command* than GNU would.
///
/// The unimplemented names are still refused, by [`parse_args`] — recognising a
/// name and performing it are separate things. See [`unsupported`].
///
/// # Why the order is load-bearing, and must not be sorted
///
/// `getopt_long` lists an ambiguous prefix's candidates in the order the caller
/// declared them, so this array's order is observable output:
///
/// ```text
/// $ tar --ex
/// tar: option '--ex' is ambiguous; possibilities: '--extract' '--exclude' ...
/// ```
///
/// `--extract` precedes `--exclude`, which alphabetical order would reverse.
/// The order is neither alphabetical nor arbitrary: it is argp's own grouping
/// by function — the operation modes, then the incremental options, then the
/// overwrite-control ones, and so on — which is why sorting it by any key at
/// all is wrong.
///
/// # How this was obtained
///
/// One measurement, from the binary:
///
/// ```text
/// $ tar --=x
/// tar: option '--' is ambiguous; possibilities: '--list' '--extract' ...
/// ```
///
/// The empty name is a prefix of every entry, so the ambiguity list *is* the
/// table — all 172 names, in declaration order, in one line.
/// `scripts/getopt-ambiguity-check.py` runs exactly this at push time and
/// refuses a push whose table disagrees, so the array below is verified against
/// the real utility on every push rather than only when it was written.
///
/// The argument classes are a second sweep: `tar --opt=zz` answering `doesn't
/// allow an argument` is `Nothing`; `tar --opt` *as the last word* answering
/// `requires an argument` is `Required`; neither is `Optional`. The option must
/// be last — a `Required` one followed by anything simply eats it, so `tar
/// --file --version` reports nothing at all. That comes out 113 / 52 / 7.
///
/// **Two names are invisible to `--help`,** which is why the ambiguity list and
/// not the help text is the authority for the name *set*:
///
/// - `--program-name`, which argp hides.
/// - `--HANG`, a hidden debug option that sleeps forever, and the only name
///   here beginning with a capital letter. It was missed by the first version
///   of this table, which was reconstructed a letter at a time from `tar --a`,
///   `tar --b`, … over the lower-case alphabet — a sweep that can never reach
///   it. The push-time check caught it. The lesson is the general one:
///   enumerate from the utility's own output, never from an alphabet you chose.
///
/// The table was also checked exhaustively against GNU: every distinct prefix
/// of every name was put to the binary and its verdict compared with this
/// table's — resolved, unrecognised, or ambiguous, and for ambiguous ones the
/// candidate list in order — for **zero** mismatches. That is also what
/// establishes tar needs [`Program::resolve_long`] rather than
/// `resolve_long_aliased`: tar does have aliases (`--extract`/`--get`), but no
/// two of them share a prefix, so name-only resolution is exact here. Were that
/// untrue, some prefix would have been accepted by GNU and called ambiguous by
/// us.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("list", Takes::Nothing),
    ("extract", Takes::Nothing),
    ("get", Takes::Nothing),
    ("create", Takes::Nothing),
    ("diff", Takes::Nothing),
    ("compare", Takes::Nothing),
    ("append", Takes::Nothing),
    ("update", Takes::Nothing),
    ("catenate", Takes::Nothing),
    ("concatenate", Takes::Nothing),
    ("delete", Takes::Nothing),
    ("test-label", Takes::Nothing),
    ("sparse", Takes::Nothing),
    ("hole-detection", Takes::Required),
    ("sparse-version", Takes::Required),
    ("incremental", Takes::Nothing),
    ("listed-incremental", Takes::Required),
    ("level", Takes::Required),
    ("ignore-failed-read", Takes::Nothing),
    ("occurrence", Takes::Optional),
    ("seek", Takes::Nothing),
    ("no-seek", Takes::Nothing),
    ("no-check-device", Takes::Nothing),
    ("check-device", Takes::Nothing),
    ("verify", Takes::Nothing),
    ("remove-files", Takes::Nothing),
    ("keep-old-files", Takes::Nothing),
    ("skip-old-files", Takes::Nothing),
    ("keep-newer-files", Takes::Nothing),
    ("overwrite", Takes::Nothing),
    ("unlink-first", Takes::Nothing),
    ("recursive-unlink", Takes::Nothing),
    ("no-overwrite-dir", Takes::Nothing),
    ("overwrite-dir", Takes::Nothing),
    ("keep-directory-symlink", Takes::Nothing),
    ("one-top-level", Takes::Optional),
    ("to-stdout", Takes::Nothing),
    ("to-command", Takes::Required),
    ("ignore-command-error", Takes::Nothing),
    ("no-ignore-command-error", Takes::Nothing),
    ("owner", Takes::Required),
    ("group", Takes::Required),
    ("owner-map", Takes::Required),
    ("group-map", Takes::Required),
    ("mtime", Takes::Required),
    ("clamp-mtime", Takes::Nothing),
    ("mode", Takes::Required),
    ("atime-preserve", Takes::Optional),
    ("touch", Takes::Nothing),
    ("same-owner", Takes::Nothing),
    ("no-same-owner", Takes::Nothing),
    ("numeric-owner", Takes::Nothing),
    ("preserve-permissions", Takes::Nothing),
    ("same-permissions", Takes::Nothing),
    ("no-same-permissions", Takes::Nothing),
    ("preserve-order", Takes::Nothing),
    ("same-order", Takes::Nothing),
    ("delay-directory-restore", Takes::Nothing),
    ("no-delay-directory-restore", Takes::Nothing),
    ("sort", Takes::Required),
    ("xattrs", Takes::Nothing),
    ("no-xattrs", Takes::Nothing),
    ("xattrs-include", Takes::Required),
    ("xattrs-exclude", Takes::Required),
    ("selinux", Takes::Nothing),
    ("no-selinux", Takes::Nothing),
    ("acls", Takes::Nothing),
    ("no-acls", Takes::Nothing),
    ("file", Takes::Required),
    ("force-local", Takes::Nothing),
    ("rmt-command", Takes::Required),
    ("rsh-command", Takes::Required),
    ("multi-volume", Takes::Nothing),
    ("tape-length", Takes::Required),
    ("info-script", Takes::Required),
    ("new-volume-script", Takes::Required),
    ("volno-file", Takes::Required),
    ("blocking-factor", Takes::Required),
    ("record-size", Takes::Required),
    ("ignore-zeros", Takes::Nothing),
    ("read-full-records", Takes::Nothing),
    ("format", Takes::Required),
    ("old-archive", Takes::Nothing),
    ("portability", Takes::Nothing),
    ("posix", Takes::Nothing),
    ("pax-option", Takes::Required),
    ("label", Takes::Required),
    ("auto-compress", Takes::Nothing),
    ("no-auto-compress", Takes::Nothing),
    ("use-compress-program", Takes::Required),
    ("bzip2", Takes::Nothing),
    ("gzip", Takes::Nothing),
    ("gunzip", Takes::Nothing),
    ("ungzip", Takes::Nothing),
    ("compress", Takes::Nothing),
    ("uncompress", Takes::Nothing),
    ("lzip", Takes::Nothing),
    ("lzma", Takes::Nothing),
    ("lzop", Takes::Nothing),
    ("xz", Takes::Nothing),
    ("zstd", Takes::Nothing),
    ("one-file-system", Takes::Nothing),
    ("absolute-names", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("hard-dereference", Takes::Nothing),
    ("starting-file", Takes::Required),
    ("newer", Takes::Required),
    ("after-date", Takes::Required),
    ("newer-mtime", Takes::Required),
    ("backup", Takes::Optional),
    ("suffix", Takes::Required),
    ("strip-components", Takes::Required),
    ("transform", Takes::Required),
    ("xform", Takes::Required),
    ("checkpoint", Takes::Optional),
    ("checkpoint-action", Takes::Required),
    ("check-links", Takes::Nothing),
    ("totals", Takes::Optional),
    ("utc", Takes::Nothing),
    ("full-time", Takes::Nothing),
    ("index-file", Takes::Required),
    ("block-number", Takes::Nothing),
    ("show-defaults", Takes::Nothing),
    ("show-snapshot-field-ranges", Takes::Nothing),
    ("show-omitted-dirs", Takes::Nothing),
    ("show-transformed-names", Takes::Nothing),
    ("show-stored-names", Takes::Nothing),
    ("quoting-style", Takes::Required),
    ("quote-chars", Takes::Required),
    ("no-quote-chars", Takes::Required),
    ("interactive", Takes::Nothing),
    ("confirmation", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("warning", Takes::Required),
    ("restrict", Takes::Nothing),
    ("add-file", Takes::Required),
    ("directory", Takes::Required),
    ("files-from", Takes::Required),
    ("null", Takes::Nothing),
    ("no-null", Takes::Nothing),
    ("unquote", Takes::Nothing),
    ("no-unquote", Takes::Nothing),
    ("verbatim-files-from", Takes::Nothing),
    ("no-verbatim-files-from", Takes::Nothing),
    ("exclude", Takes::Required),
    ("exclude-from", Takes::Required),
    ("exclude-caches", Takes::Nothing),
    ("exclude-caches-under", Takes::Nothing),
    ("exclude-caches-all", Takes::Nothing),
    ("exclude-tag", Takes::Required),
    ("exclude-ignore", Takes::Required),
    ("exclude-ignore-recursive", Takes::Required),
    ("exclude-tag-under", Takes::Required),
    ("exclude-tag-all", Takes::Required),
    ("exclude-vcs", Takes::Nothing),
    ("exclude-vcs-ignores", Takes::Nothing),
    ("exclude-backups", Takes::Nothing),
    ("recursion", Takes::Nothing),
    ("no-recursion", Takes::Nothing),
    ("anchored", Takes::Nothing),
    ("no-anchored", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("no-ignore-case", Takes::Nothing),
    ("wildcards", Takes::Nothing),
    ("no-wildcards", Takes::Nothing),
    ("wildcards-match-slash", Takes::Nothing),
    ("no-wildcards-match-slash", Takes::Nothing),
    ("help", Takes::Nothing),
    ("usage", Takes::Nothing),
    ("program-name", Takes::Required),
    ("HANG", Takes::Optional),
    ("version", Takes::Nothing),
];

/// A long option GNU tar has and this one does not implement.
///
/// **This is a deliberate divergence, and the only one in the parser.** GNU
/// would accept every one of these; we refuse, with a sentence that says which
/// of the two reasons applies so the reader is not sent looking for a typo.
///
/// The alternative is worse in a way that is not obvious until it bites. Before
/// long options existed here, `--exclude=*.o` was not an option at all — it fell
/// through to the operand branch and became a *file name to archive*, so
/// `tar -cf a.tar --exclude=*.o src` quietly produced an archive containing
/// everything the user asked to leave out, plus a spurious member, and exited 0.
/// A refusal at status 64 is a script that stops; silence is a backup that is
/// wrong and says it succeeded.
///
/// Reporting it as `unrecognized option` — the other cheap answer — would be a
/// second lie: the name *is* tar's, and telling a user that `--exclude` is not
/// a tar option sends them to the manual to check a spelling that was right.
fn unsupported(name: &str) -> getopt::Error {
    TAR.usage(format!(
        "option '--{name}' is recognised but not implemented by this tar"
    ))
}

/// Close out a run that had at least one non-fatal failure.
///
/// GNU prints this once, at exit, in addition to whatever was said about the
/// individual member — so a log that scrolled past the specific complaint still
/// ends with the fact that the run did not do what was asked. Returns the
/// status so call sites can `return failed_with_previous_errors()`.
fn failed_with_previous_errors() -> i32 {
    diag!("tar: Exiting with failure status due to previous errors");
    EXIT_FATAL
}

/// Close out a run that could not continue at all.
///
/// The distinction from [`failed_with_previous_errors`] is GNU's and is not
/// cosmetic: "previous errors" means the rest of the archive was processed and
/// some members were not, while this one means processing stopped where it was.
/// A reader of the log can tell from the last line alone whether the output is
/// partial or merely incomplete.
fn fatal() -> i32 {
    diag!("tar: Error is not recoverable: exiting now");
    EXIT_FATAL
}

// ============================================================================
// argv parsing — pure, cross-platform
// ============================================================================

/// What the command line turned out to be asking for.
///
/// Three of the four answers are not archive work at all. They are separated
/// from [`TarArgs`] rather than folded into it as flags because the difference
/// is total: `--help` does not extract anything, does not care that no
/// operation was given, and exits 0. Returning them from the parser also puts
/// the **precedence** rule in one place — the parser stops at the first of
/// them it reaches — and precedence is the whole of the observable behaviour
/// here. Measured against GNU:
///
/// | Command line | GNU | Why |
/// |---|---|---|
/// | `tar --help --frobnicate` | help, exit 0 | `--help` came first |
/// | `tar --frobnicate --help` | `unrecognized option`, exit 64 | the bad one came first |
/// | `tar -c --help` | help, exit 0 | `-c` is fine; `--help` wins over doing it |
/// | `tar --file --help` | `You must specify one of…`, exit 2 | `--help` was eaten as `--file`'s value |
///
/// An iterator gives all four rows for free, because it hands back one item at
/// a time in argv order and this function acts on the first that decides the
/// run. A parser that validated the whole of argv before returning anything
/// would fail the first row, and one that scanned for `--help` up front would
/// fail the second and the fourth.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// `-?`, `--help`: the full option list, on stdout, exit 0.
    Help,
    /// `--usage`: the one-paragraph synopsis, on stdout, exit 0. It exists
    /// because [`TRY_HELP`] names it — a referral that points at an option the
    /// program rejects is worse than no referral.
    Usage,
    /// `--version`, on stdout, exit 0.
    Version,
    /// Do the archive work described by these arguments.
    Run(TarArgs),
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct TarArgs {
    create: bool,
    extract: bool,
    list: bool,
    verbose: bool,
    /// `-p`, `--same-permissions`: restore the stored mode exactly, umask and
    /// setuid bits included. Without it a non-root extraction applies
    /// `mode & 0o777 & !umask`, which is what GNU does and what this tar did
    /// not do at all — it left every extracted file at whatever `File::create`
    /// produced.
    same_permissions: bool,
    /// What to do about something already standing where a member is to go.
    /// See [`OldFiles`].
    old_files: OldFiles,
    archive_file: Option<OsString>,
    directory: Option<OsString>,
    files: Vec<OsString>,
}

/// What extraction does about an entry that is *already there*.
///
/// GNU keeps these in one variable, not five flags, and that is observable
/// rather than an implementation detail: naming two of them is a usage error
/// (`tar: '--overwrite' cannot be used with '--keep-old-files'`, exit 2) while
/// naming the *same* one twice is fine. A set of independent booleans cannot
/// produce that pair of behaviours; a single-valued setting produces both for
/// free. See [`OldFiles::choose`].
///
/// # What each one does, measured
///
/// The archive holds `a`; the destination already holds something called `a`.
///
/// | | plain file there | hard-linked file | symlink there | directory there | nothing there |
/// |---|---|---|---|---|---|
/// | [`Replace`](Self::Replace) (default) | replaced, new inode | link broken, other name keeps its contents | **link replaced**, its target untouched | removed if empty, else `Cannot open: File exists` | created |
/// | [`Overwrite`](Self::Overwrite) | **written through**, same inode | **other name changes too** | link replaced | `Cannot open: Is a directory` | created |
/// | [`Keep`](Self::Keep) | `Cannot open: File exists`, exit 2 | as left | as left | `Cannot open: File exists` | created |
/// | [`Skip`](Self::Skip) | untouched, exit 0 | untouched | untouched | untouched | created |
/// | [`UnlinkFirst`](Self::UnlinkFirst) | removed, then created | link broken | link removed | removed if empty, else `Cannot unlink: Directory not empty` | created |
/// | [`KeepNewer`](Self::KeepNewer) | kept if its mtime ≥ the member's | as the mtime says | the *link's* own mtime decides | always replaced — directories are exempt | created |
///
/// The two rows that are easy to get wrong are the first two. `Replace`
/// **unlinks and recreates**, so a file with other hard links to it keeps those
/// names pointing at the old contents; `Overwrite` **truncates in place**, so
/// every name for that inode changes at once. That is the entire difference
/// between them, it is invisible in the output of both, and reversing it
/// silently rewrites files the archive never mentioned.
///
/// The third row is a security property rather than a convenience: none of
/// these follows a pre-existing symlink out of the destination. Measured
/// against GNU across the whole family (`tar-ovw2.sh`, `tar-ovw4.sh`), the
/// only way to make GNU write outside the destination is `-P --keep-old-files`,
/// where `-P` switches the containment check off by request. `--overwrite` in
/// particular does *not* write at the far end of a link — see
/// [`Located::create_file_overwriting`], which is where that is arranged.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum OldFiles {
    /// The default: remove what is there and create the member afresh.
    #[default]
    Replace,
    /// `--overwrite`: truncate what is there and write into it.
    Overwrite,
    /// `-k`, `--keep-old-files`: refuse, and report it as a failure.
    Keep,
    /// `--skip-old-files`: step over the member, exit status untouched.
    Skip,
    /// `-U`, `--unlink-first`: remove first, before even trying to create.
    UnlinkFirst,
    /// `--keep-newer-files`: refuse when what is there is at least as new as
    /// the member, and say so — but as a warning, not a failure.
    KeepNewer,
}

impl OldFiles {
    /// The spelling used in the conflict diagnostic.
    ///
    /// Always the *long* name, even for a setting given as `-k` or `-U`: GNU
    /// answers `tar -U -k` with `'--keep-old-files' cannot be used with
    /// '--unlink-first'`, naming neither letter.
    fn long_name(self) -> &'static str {
        match self {
            // Never printed — the default is what "no conflict" means — but a
            // name is owed here rather than a panic, since the compiler cannot
            // see that and a future caller might not either.
            Self::Replace => "--no-overwrite-dir",
            Self::Overwrite => "--overwrite",
            Self::Keep => "--keep-old-files",
            Self::Skip => "--skip-old-files",
            Self::UnlinkFirst => "--unlink-first",
            Self::KeepNewer => "--keep-newer-files",
        }
    }

    /// Adopt `wanted`, or report the conflict with what was already chosen.
    ///
    /// Repetition is allowed — `tar -k -k` and `tar --overwrite --overwrite`
    /// both extract, measured — because the test is whether the *value* would
    /// change, not whether an option was seen twice.
    ///
    /// The exit status is [`EXIT_FATAL`] (2) and not [`EXIT_USAGE`] (64), which
    /// looks like a mistake and is not: 64 is argp's `argp_err_exit_status`,
    /// used for the option table's own complaints (`unrecognized option`,
    /// `is ambiguous`), while this sentence comes from tar's `USAGE_ERROR`
    /// macro, which exits with tar's ordinary failure status. Both were
    /// measured; `tar --frobnicate` is 64 and `tar -k --overwrite` is 2.
    fn choose(&mut self, wanted: Self) -> Result<(), getopt::Error> {
        if *self != Self::Replace && *self != wanted {
            return Err(getopt::Error {
                sentence: format!(
                    "'{}' cannot be used with '{}'",
                    wanted.long_name(),
                    self.long_name()
                ),
                referral: None,
                status: EXIT_FATAL,
            });
        }
        *self = wanted;
        Ok(())
    }
}

/// Parse tar's argv: short options, long options, and operands.
///
/// The walk itself is [`coreutils::getopt`], which is `getopt_long`'s rules
/// rather than an approximation of them — and argp, which is what real tar
/// uses, *calls* `getopt_long`, so matching the shared module matches tar. That
/// is worth more than it sounds: the four spellings of a value
/// (`-f A`, `-fA`, `--file A`, `--file=A`) are all accepted now, where the
/// hand-written loop this replaced understood only the first and third, and an
/// unimplemented option's value is consumed rather than being left behind as a
/// stray operand.
///
/// Two behaviours arrive with it that were previously bugs, not choices:
///
/// - **`--` ends the options.** It used to fall through to the operand branch
///   and be looked for inside the archive, so `tar -xf t.tar -- a` answered
///   `tar: --: Not found in archive` and exited 2 where GNU extracts `a` and
///   exits 0.
/// - **`-fA` works.** A value attached to its letter was previously read as
///   more option letters, so `-fout.tar` failed on `o` — `invalid option --
///   'o'` — instead of naming the archive.
///
/// # Errors
///
/// The five glibc sentences, verbatim, plus [`unsupported`]'s. They are
/// returned rather than printed so that `main` prints tar's own referral; see
/// [`TRY_HELP`], which is argp's two-command line and not the getopt module's.
///
/// # Bytes, not text
///
/// Values and operands come out as `OsString` unchanged. Every one of them is a
/// path — the archive, the `-C` destination, each file to add — and on this OS
/// a path may hold any byte but `/` and NUL. Reading argv as `String` made
/// `tar -cf a.tar <name>` abort before doing anything at all when the name was
/// not valid UTF-8, which is a legal name here. See `known-issues.md` →
/// `B-tar-READ-EVERY-PATH-AS-UTF-8`. The shared parser preserves that: it
/// splits clusters by **byte**, so `-é` is refused by its first byte instead of
/// panicking, and a long name that is not UTF-8 can match no option and so
/// takes the `unrecognized option` path rather than failing some third way.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut out = TarArgs::default();

    for item in TAR.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // Returned the moment they are reached, which is what makes
            // `tar --help --frobnicate` print help and `tar --frobnicate
            // --help` an error. See [`Request`].
            Opt::Short(b'?', _) => return Ok(Request::Help),
            Opt::Short(b'c', _) => out.create = true,
            Opt::Short(b'x', _) => out.extract = true,
            Opt::Short(b't', _) => out.list = true,
            Opt::Short(b'v', _) => out.verbose = true,
            Opt::Short(b'p', _) => out.same_permissions = true,
            Opt::Short(b'k', _) => out.old_files.choose(OldFiles::Keep)?,
            Opt::Short(b'U', _) => out.old_files.choose(OldFiles::UnlinkFirst)?,
            Opt::Short(b'f', value) => out.archive_file = value,
            Opt::Short(b'C', value) => out.directory = value,
            // Unreachable while this arm and `SHORT_OPTIONS` agree, since the
            // parser rejects any letter the string does not list. It is a
            // refusal rather than a panic so that adding a letter to
            // `SHORT_OPTIONS` and forgetting the arm is a usage error, which is
            // true, instead of an abort.
            Opt::Short(other, _) => return Err(TAR.invalid_option(other)),

            // Long names, as the *table* spells them: an abbreviation has
            // already been resolved, so `--extr` arrives here as `extract`.
            Opt::Long(name, value) => match name {
                "create" => out.create = true,
                // GNU's own alias pair for `-x`, and likewise `-p` below. Both
                // spellings are separate table entries because `getopt_long`
                // matches on names; they converge here.
                "extract" | "get" => out.extract = true,
                "list" => out.list = true,
                "verbose" => out.verbose = true,
                "preserve-permissions" | "same-permissions" => out.same_permissions = true,
                "file" => out.archive_file = value,
                "directory" => out.directory = value,
                // The overwrite-control family. Every one of them assigns to
                // the same setting, through the check that makes naming two of
                // them an error. See [`OldFiles`].
                "overwrite" => out.old_files.choose(OldFiles::Overwrite)?,
                "keep-old-files" => out.old_files.choose(OldFiles::Keep)?,
                "skip-old-files" => out.old_files.choose(OldFiles::Skip)?,
                "unlink-first" => out.old_files.choose(OldFiles::UnlinkFirst)?,
                "keep-newer-files" => out.old_files.choose(OldFiles::KeepNewer)?,
                "help" => return Ok(Request::Help),
                "usage" => return Ok(Request::Usage),
                "version" => return Ok(Request::Version),
                other => return Err(unsupported(other)),
            },

            Opt::Operand(arg) => out.files.push(arg.clone()),
        }
    }

    Ok(Request::Run(out))
}

/// The full option list, on stdout, for `-?` and `--help`.
///
/// **It describes this tar, not GNU's.** Reproducing GNU's help verbatim would
/// have been the easy answer and is the wrong one: it advertises 171 long
/// options of which twelve work here, so a reader following it would be told to
/// use `--exclude` by the very program that refuses it. That is the same
/// silent-lie failure that made unimplemented options a refusal rather than a
/// no-op — see `design-decisions.md` 703 — and it would be worse in help text,
/// because help is what someone reads *specifically* to find out what is
/// available.
///
/// The shape is argp's, since that is what a tar user recognises: the `Usage:`
/// line, an examples block, the operation modes separated from the modifiers,
/// and the informational options last. The closing paragraph states the two
/// facts that distinguish this tar from the one the reader has used before —
/// ustar only, and the other names refuse rather than being ignored — because
/// a user who does not know the second will read a refusal as a bug.
fn help_text() -> String {
    "\
Usage: tar [OPTION...] [FILE]...
Save many files together into a single archive, and restore individual files
from an archive.

Examples:
  tar -cf archive.tar foo bar  # Create archive.tar from files foo and bar.
  tar -tvf archive.tar         # List all files in archive.tar verbosely.
  tar -xf archive.tar          # Extract all files from archive.tar.

 Main operation mode:

  -c, --create               create a new archive
  -t, --list                 list the contents of an archive
  -x, --extract, --get       extract files from an archive

 Operation modifiers:

  -C, --directory=DIR        change to directory DIR
  -f, --file=ARCHIVE         use archive file ARCHIVE; with no -f the archive
                               is standard input or standard output
  -p, --preserve-permissions, --same-permissions
                             extract the stored permissions exactly, rather
                               than applying the umask
  -v, --verbose              list each file as it is processed

 Overwrite control:

      --keep-newer-files     don't replace existing files that are newer than
                             their archive copies
  -k, --keep-old-files       don't replace existing files when extracting,
                             treat them as errors
      --overwrite            overwrite existing files when extracting
      --skip-old-files       don't replace existing files when extracting,
                             silently skip over them
  -U, --unlink-first         remove each file prior to extracting over it

 Informational options:

  -?, --help                 give this help list
      --usage                give a short usage message
      --version              print program version

Mandatory arguments to long options are mandatory for the corresponding short
options too.  A long option may be abbreviated to any unambiguous prefix.

This tar reads and writes POSIX ustar archives.  GNU tar's other long options
are recognised but not implemented: naming one is an error rather than being
quietly ignored, so a command asking for something this tar cannot do stops
instead of doing something else and reporting success.
"
    .to_string()
}

/// The short synopsis, on stdout, for `--usage`.
///
/// argp generates this by wrapping the option table into a bracketed list, and
/// wraps it to the terminal width with continuation lines indented to clear the
/// `Usage: ` prefix. With seventeen names the whole thing still fits in a few
/// lines, so it is written out rather than generated; the indent matches argp's
/// so the two look alike side by side.
///
/// The order is **GNU's own**, reduced to the names this tar implements, and it
/// is neither alphabetical nor the order of [`help_text`]. argp emits the option
/// table's declaration order — the same grouping-by-function that makes
/// [`LONG_OPTIONS`] unsortable — with the argument-taking short letters pulled
/// out after the cluster. Measured from `tar --usage`, whose first line is
/// `[-AcdrtuxGnSkUWOmpsMBiajJzZhPlRvwo?] [-g FILE] [-C DIR] …`: strike the
/// letters this tar does not have and `ctxkUpv?` is what is left, `p` before
/// `v` and `k` before `U`. The long names come from the same line and put the
/// overwrite family between `--directory` and `--preserve-permissions`, which is
/// where argp's table has it and *not* where the help text does.
fn usage_text() -> String {
    "\
Usage: tar [-ctxkUpv?] [-C DIR] [-f ARCHIVE] [--create] [--list] [--extract]
            [--get] [--directory=DIR] [--keep-newer-files] [--keep-old-files]
            [--overwrite] [--skip-old-files] [--unlink-first]
            [--preserve-permissions] [--same-permissions] [--file=ARCHIVE]
            [--verbose] [--help] [--usage] [--version] [FILE]...
"
    .to_string()
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            // `e.sentence`, deliberately, and never `e.message()` or
            // `TAR.report(&e)`: both append the getopt module's one-command
            // referral, and tar's is argp's two-command one. See `TRY_HELP`.
            diag!("tar: {}", e.sentence);
            diag!("{TRY_HELP}");
            process::exit(e.status);
        }
    };

    // The three informational requests answer and leave, before any mode
    // check: `tar --help` prints help rather than complaining that no
    // operation was given, and exits 0.
    //
    // The write is unchecked, and that is GNU's behaviour rather than an
    // oversight: measured, `tar --help >&-` prints nothing and exits **0**,
    // where `wc --help >&-` reports `write error: Bad file descriptor` and
    // exits 1. argp writes help to a `FILE*` and never asks whether it landed.
    // Matching it also keeps `tar --help | head` from failing, which is how
    // help is most often read.
    let parsed = match parsed {
        Request::Help => {
            drop(stdfd::write_all(1, help_text().as_bytes()));
            process::exit(0);
        }
        Request::Usage => {
            drop(stdfd::write_all(1, usage_text().as_bytes()));
            process::exit(0);
        }
        Request::Version => {
            drop(stdfd::write_all(1, b"tar (SlateOS coreutils) 0.1.0\n"));
            process::exit(0);
        }
        Request::Run(parsed) => parsed,
    };

    // Every mode returns its own status rather than exiting inline, so that
    // "some members failed" survives to the caller. A tool that reports 0
    // after writing half an archive is worse than one that fails outright:
    // the script that invoked it deletes the source and moves on.
    let status = if parsed.create {
        // The one case where the member list is a diagnostic rather than
        // output: with no `-f`, the archive itself is on stdout, and a name
        // printed there would be a block of the archive.
        let verbose = match (parsed.verbose, parsed.archive_file.is_some()) {
            (false, _) => Verbose::Off,
            (true, true) => Verbose::Stdout,
            (true, false) => Verbose::Stderr,
        };
        #[cfg(unix)]
        {
            do_create(parsed.archive_file.as_deref(), &parsed.files, verbose)
        }
        #[cfg(not(unix))]
        {
            let _ = verbose;
            diag!("tar: create mode is unix-only on this build");
            EXIT_FATAL
        }
    } else if parsed.extract {
        do_extract(
            parsed.archive_file.as_deref(),
            parsed.directory.as_deref(),
            if parsed.verbose {
                Verbose::Stdout
            } else {
                Verbose::Off
            },
            &parsed.files,
            parsed.same_permissions,
            parsed.old_files,
        )
    } else if parsed.list {
        do_list_main(
            parsed.archive_file.as_deref(),
            parsed.verbose,
            &parsed.files,
        )
    } else {
        // GNU's own sentence, listing options this tar does not have. That is
        // deliberate: the message tells the reader what the *format* accepts,
        // and a user who reaches for `-r` after reading it gets a specific
        // `invalid option` rather than being told twice that they typed
        // nothing. The status is 2, not argp's 64 — this is not a malformed
        // command line, it is a well-formed one that asked for no operation.
        diag!("tar: You must specify one of the '-Acdtrux', '--delete' or '--test-label' options");
        diag!("{TRY_HELP}");
        EXIT_FATAL
    };

    process::exit(status);
}

// ============================================================================
// TAR header format (512 bytes, POSIX ustar) — cross-platform
// ============================================================================

const BLOCK_SIZE: usize = 512;

#[repr(C)]
#[cfg_attr(not(unix), allow(dead_code))]
struct TarHeader {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typeflag: u8,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    _pad: [u8; 12],
}

/// The `name` field's width. A name of exactly this length fills it with no
/// room for a terminator, which is legal: ustar NUL-terminates only when the
/// name is short enough to leave a byte spare.
const NAME_FIELD: usize = 100;

/// The `prefix` field's width.
const PREFIX_FIELD: usize = 155;

/// Why a member name could not be stored in a ustar header.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum NameTooLong {
    /// Longer than the two fields and the `/` between them can hold at all.
    Max,
    /// Short enough in total, but with no `/` in a position that would put no
    /// more than [`NAME_FIELD`] bytes after it.
    CannotSplit,
}

impl NameTooLong {
    /// GNU's wording, which names the limit in the first case and not in the
    /// second. Both end `; not dumped`, and both leave the archive otherwise
    /// intact — the member is skipped, the rest are written, and the exit
    /// status is 2.
    fn message(self, name: &[u8]) -> String {
        match self {
            Self::Max => format!(
                "tar: {}: file name is too long (max {}); not dumped",
                escape(name),
                NAME_FIELD.saturating_add(PREFIX_FIELD).saturating_add(1)
            ),
            Self::CannotSplit => format!(
                "tar: {}: file name is too long (cannot be split); not dumped",
                escape(name)
            ),
        }
    }
}

/// Split `full` into ustar's `prefix` and `name`, as GNU's `split_long_name`
/// does.
///
/// The whole name is `prefix` + `/` + `name` when a prefix is used, so the
/// split has to fall on a `/` and that `/` is not stored. Names that fit in
/// [`NAME_FIELD`] outright use no prefix at all.
///
/// The search is *backwards from a capped position*, not "the last slash", and
/// the cap is what makes some names unsplittable that look splittable:
///
/// 1. Consider only the first `PREFIX_FIELD + 1` bytes, since a prefix cannot
///    reach past that anyway.
/// 2. Unless the cap already applied, ignore a trailing `/` — a directory
///    member is stored with one and it must not be chosen as the split point.
/// 3. Take the last `/` at or before that position; offset 0 does not count,
///    because an empty prefix is not a prefix.
/// 4. Refuse if what follows it is empty or longer than [`NAME_FIELD`].
///
/// Measured against GNU tar 1.35 across the boundary (`tar-longname.sh`):
/// `t/` + 96×`d` + `/fff` splits at offset 98; a 100-byte remainder is
/// accepted and a 101-byte one is refused; and `t/` + 150×`d` + `/` — 153
/// bytes, which would fit a 152-byte prefix and a 0-byte name — is refused,
/// because rule 3 finds the slash at offset 1 and leaves 151 bytes after it.
fn split_ustar_name(full: &[u8]) -> Result<(&[u8], &[u8]), NameTooLong> {
    if full.len() <= NAME_FIELD {
        return Ok((&[], full));
    }
    if full.len() > NAME_FIELD.saturating_add(PREFIX_FIELD).saturating_add(1) {
        return Err(NameTooLong::Max);
    }
    let capped = PREFIX_FIELD.saturating_add(1);
    let mut end = full.len();
    if end > capped {
        end = capped;
    } else if full.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    // Backwards over `full[1..end]`: offset 0 is excluded because a prefix of
    // no bytes is the no-prefix case, which the length test above already took.
    let split = full
        .get(1..end)
        .unwrap_or(&[])
        .iter()
        .rposition(|&b| b == b'/')
        .map(|i| i.saturating_add(1));
    let Some(i) = split else {
        return Err(NameTooLong::CannotSplit);
    };
    let (Some(prefix), Some(name)) = (full.get(..i), full.get(i.saturating_add(1)..)) else {
        return Err(NameTooLong::CannotSplit);
    };
    if name.is_empty() || name.len() > NAME_FIELD {
        return Err(NameTooLong::CannotSplit);
    }
    Ok((prefix, name))
}

#[cfg_attr(not(unix), allow(dead_code))]
impl TarHeader {
    fn new() -> Self {
        Self {
            name: [0; 100],
            mode: [0; 8],
            uid: [0; 8],
            gid: [0; 8],
            size: [0; 12],
            mtime: [0; 12],
            checksum: [0; 8],
            typeflag: 0,
            linkname: [0; 100],
            magic: [0; 6],
            version: [0; 2],
            uname: [0; 32],
            gname: [0; 32],
            devmajor: [0; 8],
            devminor: [0; 8],
            prefix: [0; 155],
            _pad: [0; 12],
        }
    }

    /// Store a member name across the header's `name` and `prefix` fields.
    ///
    /// Bytes, not `&str`: the fields hold whatever the filesystem gave us, and
    /// ustar has never required it to be text.
    ///
    /// This used to copy the first 99 bytes into `name` and stop. Two separate
    /// defects in one line: a 100-byte name lost its last byte, because the
    /// field is not NUL-terminated when it is full; and a name longer than that
    /// was silently truncated, producing a well-formed archive holding the
    /// wrong name and exiting 0. See [`split_ustar_name`] for the split and for
    /// what happens when there is none.
    fn set_name(&mut self, full: &[u8]) -> Result<(), NameTooLong> {
        let (prefix, name) = split_ustar_name(full)?;
        if let (Some(dst), Some(src)) = (self.name.get_mut(..name.len()), name.get(..)) {
            dst.copy_from_slice(src);
        }
        if let (Some(dst), Some(src)) = (self.prefix.get_mut(..prefix.len()), prefix.get(..)) {
            dst.copy_from_slice(src);
        }
        Ok(())
    }

    /// Store a link target in the 100-byte `linkname` field, cutting it to fit.
    ///
    /// Returns whether the whole target fit; the caller warns when it did not.
    ///
    /// There is no `prefix` for this one — ustar gives the link target a single
    /// field and no escape hatch — and unlike a member name, which GNU refuses
    /// outright, a target that does not fit is stored truncated. Measured: a
    /// 101-byte symlink target produces the warning, exit status 2, *and* a
    /// member in the archive whose link is the first 100 bytes.
    fn set_linkname(&mut self, target: &[u8]) -> bool {
        let kept = target.len().min(NAME_FIELD);
        if let (Some(dst), Some(src)) = (self.linkname.get_mut(..kept), target.get(..kept)) {
            dst.copy_from_slice(src);
        }
        target.len() <= NAME_FIELD
    }

    /// Write `value` as a zero-padded octal string into `field`.  The
    /// field always ends with a trailing null byte, matching ustar.
    fn set_octal(field: &mut [u8], value: u64) {
        if field.is_empty() {
            return;
        }
        let width = field.len().saturating_sub(1);
        let s = format!("{value:0>width$o}");
        let bytes = s.as_bytes();
        // If `s` is longer than the field allows, take only the rightmost
        // `width` chars so the low-order digits survive.
        let start = bytes.len().saturating_sub(width);
        let src = bytes.get(start..).unwrap_or(&[]);
        let copy_len = src.len().min(width);
        if let (Some(dst), Some(src)) = (field.get_mut(..copy_len), src.get(..copy_len)) {
            dst.copy_from_slice(src);
        }
        // Trailing byte stays NUL.
    }

    fn compute_checksum(&mut self) {
        // Fill checksum field with spaces for computation.
        self.checksum = [b' '; 8];

        // SAFETY: `TarHeader` is `#[repr(C)]` with explicit byte-array
        // fields whose sizes add to exactly `BLOCK_SIZE` (512).  There
        // are no padding bytes or non-trivial drop glue, so it is sound
        // to view `self` as `[u8; BLOCK_SIZE]`.  The borrow lasts only
        // for the duration of this function.
        let header_bytes =
            unsafe { std::slice::from_raw_parts((self as *const Self).cast::<u8>(), BLOCK_SIZE) };
        let sum: u32 = header_bytes.iter().map(|&b| u32::from(b)).sum();

        let s = format!("{sum:06o}\0 ");
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(8);
        if let (Some(dst), Some(src)) = (self.checksum.get_mut(..copy_len), bytes.get(..copy_len)) {
            dst.copy_from_slice(src);
        }
    }

    fn as_bytes(&self) -> &[u8; BLOCK_SIZE] {
        // SAFETY: see `compute_checksum` — `#[repr(C)]` byte fields
        // tiling to exactly `BLOCK_SIZE` make this cast sound.
        unsafe { &*(self as *const Self).cast::<[u8; BLOCK_SIZE]>() }
    }
}

/// Where `-v` writes its running list of member names.
///
/// This used to be "stderr, always", which is wrong in the ordinary case and
/// right in exactly one unusual one. GNU writes the list to **stdout**, because
/// it is output, not a diagnostic: `tar -cvf a.tar d > manifest` is how you get
/// a manifest, and ours produced an empty `manifest` and printed the names past
/// the redirection onto the terminal. The single exception is an archive being
/// written *to* stdout — `tar -cvf - d` — where the names would be interleaved
/// with the archive bytes and ruin both; there, and only there, they go to
/// stderr.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verbose {
    /// No `-v`: say nothing.
    Off,
    /// The usual case.
    Stdout,
    /// `-cv` with the archive itself on stdout.
    Stderr,
}

impl Verbose {
    /// Announce one member name, rendered exactly as a diagnostic would render
    /// it.
    ///
    /// This used to write the name's bytes raw, on the reasoning that a listing
    /// is output rather than a message and should carry the name intact. That
    /// is not what GNU does and it is not safe: `-cv`, `-xv` and `-t` all go
    /// through the same `escape` style as tar's diagnostics, so a member called
    /// `a\nb` prints as `a\nb` on one line rather than as two lines that a
    /// script reading the manifest would take for two files. Measured against
    /// GNU tar 1.35 for all of `-t`, `-tv`, `-cv` and `-xv`.
    ///
    /// The cost is that the rendering is no longer reversible — a name holding
    /// a literal backslash comes back doubled — which is exactly the cost GNU
    /// pays, and the reason `tar -t` has never been a safe way to feed names to
    /// another program.
    fn line(self, name: &[u8]) {
        let shown = escape(name);
        let mut line = Vec::with_capacity(shown.len().saturating_add(1));
        line.extend_from_slice(shown.as_bytes());
        line.push(b'\n');
        match self {
            Self::Off => {}
            // Unbuffered, by fd. Nothing else in `-c`/`-x` writes to stdout, so
            // there is no ordering to keep with a `BufWriter`, and a failure to
            // write the listing must not abort the archive.
            Self::Stdout => drop(stdfd::write_all(1, &line)),
            Self::Stderr => stdfd::diag_bytes(&line),
        }
    }
}

/// Does any whole component of this name climb a level?
///
/// The test a *member name* has to pass before it is used as a path, and the
/// one thing prefix stripping cannot make safe: `a/../b` is equivalent to `b`
/// only if `a` is a real directory and not a symlink, and the archive is
/// precisely the thing we are not willing to trust about that. GNU refuses
/// such a member outright — `tar: a/../b: Member name contains '..'`, status 2
/// — and does so *after* printing the notice about the prefix it stripped, so
/// a run against `a/../b` produces both lines. Measured, `tar-rules2.sh`.
///
/// A hard link's *target* is deliberately not put through this: GNU rewrites
/// `a/../base` to `base` and links to it, and a `..` there cannot name a file
/// the archive did not already reach.
///
/// Operates on **bytes**, because the name it is given is 100 bytes out of an
/// archive header and nothing guarantees they are text. The previous version
/// of this check took `&str`, which meant the name had already been through
/// `String::from_utf8_lossy` — and that is not just a display problem, it is a
/// correctness one: the replacement happened *before* the `..` test, so the
/// guarantee was being made about a string that was no longer the name being
/// written.
///
/// `\` is not a separator here. It was once tested as one, as defence in depth
/// for the host builds this file is unit-tested on; that made a member
/// legitimately named `a\..` — a name `design.txt` allows, since paths admit
/// every byte but `/` and NUL — unextractable on the OS this tar is *for*. The
/// path is built by joining on `/` alone, so a backslash never traverses.
fn contains_dot_dot(name: &[u8]) -> bool {
    name.split(|&b| b == b'/').any(|c| c == b"..")
}

/// Split a device number the way ustar stores it, into `devmajor`/`devminor`.
///
/// Not a plain shift: Linux packs `dev_t` in two pieces so that old 16-bit
/// numbers keep their old encoding — 12 bits of major and 8 of minor in the
/// low half, the rest of each in the high half.
#[cfg(unix)]
fn split_dev(rdev: u64) -> (u64, u64) {
    let major = ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff);
    let minor = (rdev & 0xff) | ((rdev >> 12) & !0xff);
    (major, minor)
}

/// Split a name into the prefix tar refuses to honour and the rest, returning
/// `(removed, rest)` — GNU's `safer_name_suffix`, ported.
///
/// An archive holding `/etc/passwd` is a loaded gun: extracting it anywhere
/// writes to `/etc/passwd`. GNU's answer is at both ends — it strips the prefix
/// when writing the archive as well as when reading one — so this one function
/// serves the writer, the lister and the extractor, which is why they agree.
///
/// The rule is **not** "a leading run of `/` and `../`", which is what this
/// used to implement. GNU scans the *whole* name for components equal to `..`
/// and puts the cut just past the last one, then swallows the slashes after it.
/// Measured against GNU tar 1.35 (`tar-rules.sh`, `tar-rules2.sh`):
///
/// | given | removed | rest |
/// |---|---|---|
/// | `/a/b` | `/` | `a/b` |
/// | `//a/b` | `//` | `a/b` — the exact run, not one `/` |
/// | `../a` | `../` | `a` |
/// | `..` | `..` | *(empty — the caller substitutes `.`)* |
/// | `/d/../e` | `/d/../` | `e` |
/// | `a/../b` | `a/../` | `b` |
/// | `d/..` | `d/..` | *(empty)* |
/// | `./a` | *(none)* | `./a` — a leading `.` is not a prefix |
///
/// The interior case is not academic: it decides where a hard link whose target
/// is `a/../base` points (at `base`), and it is the prefix that gets announced
/// when a *member* named `a/../b` is refused. A leading `.` is deliberately not
/// in the set; it names the directory being archived and takes the extractor
/// nowhere it was not already, which is why `tar -cf - .` round-trips through
/// `./f` unaltered.
fn strip_leading(name: &[u8]) -> (&[u8], &[u8]) {
    let mut cut = 0usize;
    let mut i = 0usize;
    while i < name.len() {
        if name.get(i..).is_some_and(|s| s.starts_with(b".."))
            && matches!(name.get(i.saturating_add(2)), None | Some(&b'/'))
        {
            cut = i.saturating_add(2);
        }
        // Step over this component and the one slash that ends it, if any.
        while let Some(&c) = name.get(i) {
            i = i.saturating_add(1);
            if c == b'/' {
                break;
            }
        }
    }
    while name.get(cut) == Some(&b'/') {
        cut = cut.saturating_add(1);
    }
    (
        name.get(..cut).unwrap_or(&[]),
        name.get(cut..).unwrap_or(&[]),
    )
}

/// Which of tar's two independent prefix notices a strip belongs to.
#[derive(Clone, Copy)]
enum PrefixKind {
    MemberNames,
    LinkTargets,
}

impl PrefixKind {
    /// Plural, for the notice about a prefix — it describes a class of names.
    fn label(self) -> &'static str {
        match self {
            Self::MemberNames => "member names",
            Self::LinkTargets => "hard link targets",
        }
    }

    /// Singular, for the notice about one empty name. GNU's two sentences
    /// really do differ in number; both were measured.
    fn one(self) -> &'static str {
        match self {
            Self::MemberNames => "member name",
            Self::LinkTargets => "hard link target",
        }
    }
}

/// The name substituted when stripping consumes the whole name.
const DOT: &[u8] = b".";

/// [`strip_leading`] plus the once-per-*distinct*-prefix notice around it.
///
/// GNU announces a removal the first time it cuts that exact prefix, and keeps
/// two independent sets — one for member names, one for hard link targets.
/// Measured (`tar-rules4.sh`): the hard link targets `/x ../x /x //x ../x /x
/// a/../x` produce exactly four lines, for `/`, `../`, `//` and `a/../`. The
/// repeats say nothing.
///
/// Two earlier readings of this were wrong and are worth recording, because
/// both fit the shorter probe that produced them. It is **not** "announce when
/// the prefix changes" (that re-announces `/` after `../`), and it is **not** a
/// high-water mark on prefix length (that would swallow the `a/../` above,
/// since `../` is no shorter, and it swallowed every notice for a run of
/// distinct same-length prefixes).
///
/// One struct for all three drivers because GNU's state is shared the same way,
/// and because `tar -tf` prints these notices too: a listing and an extraction
/// of the same archive produce the same stderr, and only a shared
/// implementation keeps that true.
struct PrefixNotice {
    names: BTreeSet<Vec<u8>>,
    targets: BTreeSet<Vec<u8>>,
}

impl PrefixNotice {
    fn new() -> Self {
        Self {
            names: BTreeSet::new(),
            targets: BTreeSet::new(),
        }
    }

    /// [`Self::strip_flushing`] for the drivers that write no buffered stdout.
    fn strip<'a>(&mut self, name: &'a [u8], kind: PrefixKind) -> &'a [u8] {
        self.strip_flushing(name, kind, &mut || {})
    }

    /// The suffix `name` is stored or extracted under, announcing the cut the
    /// first time this exact prefix is seen.
    ///
    /// Two distinct emptinesses, and GNU treats them differently. A name that
    /// *arrives* empty is announced — `Substituting `.' for empty member name`,
    /// on **every** such member, not once — because an empty field in a header
    /// is a defect worth reporting. A name that merely *strips* to nothing
    /// (`/`, `..`, `d/..`) becomes `.` silently, since the notice about the
    /// prefix has already said everything there is to say. Both measured,
    /// `tar-rules4.sh`; an earlier reading passed the empty target through
    /// unchanged, which reported a link to `‘’` where GNU links to `.`.
    ///
    /// `flush` is called immediately before a diagnostic and not otherwise. It
    /// exists for the lister, whose member lines go through a `BufWriter`:
    /// gnulib's `error()` flushes stdout before writing to stderr, which is why
    /// GNU's `-tv` interleaves a notice with its listing at the right point.
    /// Flushing unconditionally would cost a syscall per member for a case that
    /// arises a handful of times per archive at most.
    fn strip_flushing<'a>(
        &mut self,
        name: &'a [u8],
        kind: PrefixKind,
        flush: &mut dyn FnMut(),
    ) -> &'a [u8] {
        if name.is_empty() {
            flush();
            diag!("tar: Substituting `.' for empty {}", kind.one());
            return DOT;
        }
        let (removed, rest) = strip_leading(name);
        if !removed.is_empty() {
            let seen = match kind {
                PrefixKind::MemberNames => &mut self.names,
                PrefixKind::LinkTargets => &mut self.targets,
            };
            if seen.insert(removed.to_vec()) {
                flush();
                diag!(
                    "tar: Removing leading `{}' from {}",
                    escape(removed),
                    kind.label()
                );
            }
        }
        if rest.is_empty() { DOT } else { rest }
    }
}

/// The state one `-c` pass carries from member to member.
///
/// This was four free functions threading a `&mut i32`. The hard-link table is
/// why it is a struct: recognising a second name for an inode means
/// remembering every inode already written for the whole run and across
/// operands — measured, GNU stores `t/h` as a link to `t/a.txt` even when the
/// two are separate command-line arguments, and stores the *first* name it
/// happened to archive, so `tar -c t/h t/a.txt` links `a.txt` to `h`.
#[cfg(unix)]
struct Creator<'a> {
    out: &'a mut dyn Write,
    verbose: Verbose,
    /// 0, or [`EXIT_FATAL`] once anything has gone wrong. A member that cannot
    /// be archived sets this and is skipped; it does not abandon the archive.
    status: i32,
    /// Every inode already archived that could have another name, and the name
    /// it went in under. Keyed by `(dev, ino)`, because an inode number is only
    /// unique within one filesystem — a bare `ino` key would link together two
    /// unrelated files that happen to share a number across mount points.
    links: BTreeMap<(u64, u64), Vec<u8>>,
    /// How much has already been stripped from a member name and from a hard
    /// link's target, so that the notice is issued once per *longer* prefix.
    /// This is why `tar -c ..` produces two lines, ``Removing leading `..'``
    /// for the directory itself and ``Removing leading `../'`` for everything
    /// in it — and only two, however many members follow.
    prefixes: PrefixNotice,
    /// `(dev, ino)` of the archive being written, when it is a file we can
    /// identify. `tar -cf backup.tar .` names the archive among the things to
    /// archive, and a tar that obliges copies the archive into itself as it
    /// grows — the result is a much larger file holding a truncated snapshot of
    /// itself, and no warning that it happened.
    archive_id: Option<(u64, u64)>,
    /// Cleared by the first failed write. There is no point continuing after
    /// one: every later member would land at the wrong offset, producing a file
    /// that looks like an archive and is not one.
    writable: bool,
}

#[cfg(unix)]
impl Creator<'_> {
    fn fail(&mut self) {
        self.status = EXIT_FATAL;
    }

    fn write(&mut self, buf: &[u8]) -> bool {
        if !self.writable {
            return false;
        }
        match self.out.write_all(buf) {
            Ok(()) => true,
            Err(e) => {
                diag!("tar: Cannot write: {}", strerror(&e));
                self.writable = false;
                self.fail();
                false
            }
        }
    }

    /// The name this member goes into the archive under: `name` with any
    /// leading `/` or `../` taken off, and with the trailing slash a directory
    /// member carries put on.
    ///
    /// The two happen in that order, which matters for `tar -c ..`: strip
    /// first and `..` becomes nothing, which is stored as `.` and listed as
    /// `./`. Appending first would have stripped the slash back off again.
    fn stored_name(&mut self, name: &[u8], dir: bool) -> Vec<u8> {
        let mut stored = self.prefixes.strip(name, PrefixKind::MemberNames).to_vec();
        if dir {
            stored.push(b'/');
        }
        stored
    }

    /// The same for a hard link's target, which is a member name too and gets
    /// the same treatment under a message of its own.
    ///
    /// Not for a *symlink* target: that one is data, not a member name, and an
    /// absolute symlink is a legitimate thing to archive. Measured — GNU stores
    /// `/etc/passwd` for `ln -s /etc/passwd x` and says nothing.
    fn stored_link_target(&mut self, target: &[u8]) -> Vec<u8> {
        self.prefixes
            .strip(target, PrefixKind::LinkTargets)
            .to_vec()
    }

    /// Fill in the fields every member type shares. `None` means the name
    /// cannot be stored — reported here, and the member is skipped.
    ///
    /// `size` is left at zero: only a regular file overrides it, and getting
    /// that wrong on a link or a device would make the extractor read the next
    /// member's header as file contents.
    fn header(&mut self, name: &[u8], meta: &fs::Metadata, dir: bool) -> Option<TarHeader> {
        use std::os::unix::fs::MetadataExt;
        let name = &self.stored_name(name, dir);
        let mut header = TarHeader::new();
        if let Err(e) = header.set_name(name) {
            // Skipped, not fatal to the archive: GNU writes every other member
            // and exits 2, so one unstorable name does not cost you the backup.
            // Measured — an archive of a tree holding such a file still lists
            // the rest.
            diag!("{}", e.message(name));
            self.fail();
            return None;
        }
        TarHeader::set_octal(&mut header.mode, u64::from(meta.mode()) & 0o7777);
        TarHeader::set_octal(&mut header.uid, u64::from(meta.uid()));
        TarHeader::set_octal(&mut header.gid, u64::from(meta.gid()));
        TarHeader::set_octal(&mut header.size, 0);
        TarHeader::set_octal(&mut header.mtime, meta.mtime().unsigned_abs());
        header.magic = *b"ustar\0";
        header.version = *b"00";
        Some(header)
    }

    /// Archive whatever `path` turns out to be, under the member name `name`.
    ///
    /// The type test is `symlink_metadata`, not `metadata`. The previous code
    /// asked `path.is_dir()` and then `fs::metadata`, both of which follow
    /// symlinks, so a symlink was archived as a *copy of whatever it pointed
    /// at* — a symlink to a directory pulled that whole directory into the
    /// archive under the link's name, and a symlink to a file duplicated the
    /// file. Restoring such an archive does not restore the tree.
    fn add(&mut self, path: &Path, name: &[u8]) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                diag!("tar: {}: Cannot stat: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };
        if self.archive_id == Some((meta.dev(), meta.ino())) {
            // A warning, not an error: GNU exits 0 for this, because the
            // archive it produced is exactly the one that was asked for minus
            // the one member that could not possibly have gone in it.
            diag!(
                "tar: {}: archive cannot contain itself; not dumped",
                escape(name)
            );
            return;
        }
        let ft = meta.file_type();
        if ft.is_dir() {
            self.add_dir(path, name, &meta);
            return;
        }
        // A second name for an inode already archived is stored as a link to
        // the first, whatever the two names are. Checked before the type
        // dispatch because it applies to fifos and devices too, not just
        // regular files, and checked only when the inode admits another name:
        // a link count of one cannot have a second name to find.
        if meta.nlink() > 1 {
            let key = (meta.dev(), meta.ino());
            if let Some(first) = self.links.get(&key) {
                let first = first.clone();
                self.add_link(name, &meta, b'1', &first);
                return;
            }
            self.links.insert(key, name.to_vec());
        }
        if ft.is_symlink() {
            let target = match fs::read_link(path) {
                Ok(t) => os_bytes(t.as_os_str()).into_owned(),
                Err(e) => {
                    diag!("tar: {}: Cannot read link: {}", escape(name), strerror(&e));
                    self.fail();
                    return;
                }
            };
            self.add_link(name, &meta, b'2', &target);
        } else if ft.is_file() {
            self.add_regular(path, name, &meta);
        } else if ft.is_fifo() {
            self.add_special(name, &meta, b'6');
        } else if ft.is_char_device() {
            self.add_special(name, &meta, b'3');
        } else if ft.is_block_device() {
            self.add_special(name, &meta, b'4');
        } else if ft.is_socket() {
            // Not an error, and measured as such: a socket is a kernel object
            // with no contents an archive could hold, so GNU says so and still
            // exits 0. Skipping it silently would be the wrong half of that —
            // the file is missing from the archive and the user should know.
            diag!("tar: {}: socket ignored", escape(name));
        } else {
            diag!("tar: {}: Unknown file type; file ignored", escape(name));
            self.fail();
        }
    }

    /// A member that is a name pointing at another name and nothing else: a
    /// symlink (`2`) or a hard link (`1`). Both store zero bytes of data.
    fn add_link(&mut self, name: &[u8], meta: &fs::Metadata, typeflag: u8, target: &[u8]) {
        // The order is GNU's: the member name's prefix is reported before the
        // link target's, because `header` runs first.
        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        let target = &if typeflag == b'1' {
            self.stored_link_target(target)
        } else {
            target.to_vec()
        };
        header.typeflag = typeflag;
        if !header.set_linkname(target) {
            // GNU says "not dumped" and then dumps it anyway, with the target
            // cut to 100 bytes — measured, the member is in the archive with a
            // truncated link. We match that rather than skipping, because the
            // alternative loses the member entirely: a truncated target almost
            // certainly does not exist, so extraction fails loudly, whereas a
            // skipped member is simply absent. Note the message names the
            // *target*, not the member — that is GNU's wording, not a slip.
            diag!("tar: {}: link name is too long; not dumped", escape(target));
            self.fail();
        }
        header.compute_checksum();
        if self.write(header.as_bytes()) {
            self.verbose.line(name);
        }
    }

    /// A fifo (`6`) or a device (`3`/`4`): a header, no data.
    fn add_special(&mut self, name: &[u8], meta: &fs::Metadata, typeflag: u8) {
        use std::os::unix::fs::MetadataExt;
        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        header.typeflag = typeflag;
        if typeflag == b'3' || typeflag == b'4' {
            let (major, minor) = split_dev(meta.rdev());
            TarHeader::set_octal(&mut header.devmajor, major);
            TarHeader::set_octal(&mut header.devminor, minor);
        }
        header.compute_checksum();
        if self.write(header.as_bytes()) {
            self.verbose.line(name);
        }
    }

    /// A regular file: a header, then its contents padded out to a block.
    fn add_regular(&mut self, path: &Path, name: &[u8], meta: &fs::Metadata) {
        // The header commits to a length, so the body must be exactly that
        // many bytes however the read goes. Writing fewer would not merely
        // truncate this member: the extractor reads a fixed number of blocks
        // per header, so every subsequent member would be read from the wrong
        // offset and the whole archive after this point would be garbage.
        let declared = meta.len();

        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };

        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        TarHeader::set_octal(&mut header.size, declared);
        header.typeflag = b'0';
        header.compute_checksum();
        if !self.write(header.as_bytes()) {
            return;
        }

        self.verbose.line(name);

        let mut remaining = declared;
        let mut buf = [0u8; BLOCK_SIZE];
        let mut short = false;
        while remaining > 0 {
            let want = usize::try_from(remaining)
                .unwrap_or(BLOCK_SIZE)
                .min(BLOCK_SIZE);
            let mut filled = 0usize;
            while filled < want && !short {
                match f.read(buf.get_mut(filled..want).unwrap_or(&mut [])) {
                    // Only 0 means end of file. A short read is ordinary — the
                    // previous code took any single `read` as the whole block
                    // and NUL-padded the rest, so a file delivered in pieces
                    // was archived with holes punched through it.
                    Ok(0) => short = true,
                    Ok(n) => filled = filled.saturating_add(n),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        diag!("tar: {}: Cannot read: {}", escape(name), strerror(&e));
                        self.fail();
                        short = true;
                    }
                }
            }
            if let Some(pad) = buf.get_mut(filled..) {
                pad.fill(0);
            }
            if !self.write(&buf) {
                return;
            }
            remaining = remaining.saturating_sub(want as u64);
        }
        if short {
            // The file shrank between the stat and the read, or never had the
            // length it claimed. The archive stays well-formed because the
            // remaining blocks were padded, but it no longer holds the file.
            diag!(
                "tar: {}: file shorter than expected; padded with zeros",
                escape(name)
            );
            self.fail();
        }
    }

    /// A directory (`5`) and, after it, everything under it in name order.
    ///
    /// `name` is the directory's member name *without* the trailing slash the
    /// header carries; children are named by appending to it.
    fn add_dir(&mut self, dir: &Path, name: &[u8], meta: &fs::Metadata) {
        // A directory has an owner, permissions and an mtime like anything
        // else, and all four used to be hard-coded here: every directory in
        // every archive we wrote came out `drwxr-xr-x 0/0` stamped 1970. Not a
        // cosmetic difference — restoring such an archive as root would hand
        // every directory in it to root and open a 0700 directory to the world.
        let Some(mut header) = self.header(name, meta, true) else {
            // The directory is skipped and so, necessarily, is everything under
            // it: a member name that cannot be stored has no children whose
            // names could be.
            return;
        };
        header.typeflag = b'5';
        header.compute_checksum();
        if !self.write(header.as_bytes()) {
            return;
        }

        // The *unstripped* name, with the trailing slash. GNU's `-cv` names the
        // file it is reading, not the member it is writing: `tar -cvf a.tar
        // /etc` lists `/etc/...` while the archive holds `etc/...`. Measured.
        let mut shown = name.to_vec();
        shown.push(b'/');
        self.verbose.line(&shown);

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                // Previously `if let Ok(entries)`, so an unreadable directory
                // produced an archive silently missing its whole subtree.
                diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };
        // Sorted so that archiving the same tree twice produces the same
        // bytes; `read_dir` order is whatever the filesystem feels like.
        //
        // The names are collected as bytes. `to_string_lossy().into_owned()`
        // was here, and it did not merely misprint a name under `-v`: the
        // lossy copy was what got stored in the header *and* what the recursion
        // descended with, so a directory entry whose name is not UTF-8 — legal
        // on this OS — was archived under a different name than it has on disk,
        // and restoring the archive would not restore the tree. Sorting by
        // bytes rather than by `String` also keeps the ordering stable for
        // names that no longer survive a lossy round trip.
        let mut children: Vec<(Vec<u8>, std::path::PathBuf)> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => children.push((os_bytes(&e.file_name()).into_owned(), e.path())),
                Err(e) => {
                    diag!("tar: {}: Cannot read: {}", escape(name), strerror(&e));
                    self.fail();
                }
            }
        }
        children.sort();
        for (file_name, entry_path) in children {
            let mut entry_name = name.to_vec();
            entry_name.push(b'/');
            entry_name.extend_from_slice(&file_name);
            self.add(&entry_path, &entry_name);
        }
    }
}

#[cfg(unix)]
fn do_create(archive_file: Option<&OsStr>, files: &[OsString], verbose: Verbose) -> i32 {
    // Identified by inode, not by name: `tar -cf ./b.tar .` and `tar -cf b.tar
    // .` name the archive differently and it is the same file both times, and
    // comparing the strings would catch neither.
    let mut archive_id = None;
    let mut out: Box<dyn Write> = match archive_file {
        Some(path) => match File::create(path) {
            Ok(f) => {
                use std::os::unix::fs::MetadataExt;
                // A stat that fails is not fatal; it only costs the self-check,
                // and the archive is otherwise fine.
                if let Ok(m) = f.metadata() {
                    archive_id = Some((m.dev(), m.ino()));
                }
                Box::new(f)
            }
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdout()),
    };

    let mut creator = Creator {
        out: &mut out,
        verbose,
        status: 0,
        links: BTreeMap::new(),
        prefixes: PrefixNotice::new(),
        archive_id,
        writable: true,
    };
    for operand in files {
        // The member name is the operand exactly as typed, byte for byte —
        // which is what GNU stores too.
        let name = os_bytes(operand);
        creator.add(Path::new(operand), &name);
    }

    let zero_block = [0u8; BLOCK_SIZE];
    let _ = creator.write(&zero_block) && creator.write(&zero_block);
    let mut status = creator.status;
    // The end-of-archive marker is the last thing written, so a flush that
    // fails here loses precisely the bytes that make the file a valid archive.
    if let Err(e) = out.flush() {
        diag!("tar: Cannot write: {}", strerror(&e));
        status = EXIT_FATAL;
    }
    if status == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

// ============================================================================
// reading an archive — one decoder, shared by `-t` and `-x`
// ============================================================================

/// One member's header, decoded.
///
/// The two modes used to decode headers separately, and had drifted: `-t` and
/// `-x` each read their own 100 bytes of name, each ignored `prefix`, and each
/// stopped silently at the first block they did not understand. A single
/// decoder is not tidiness — it is the only way the listing and the extraction
/// of the same archive are guaranteed to be talking about the same members.
struct Member {
    /// The full stored name: `prefix` + `/` + `name` when `prefix` is used.
    name: Vec<u8>,
    /// The stored permission bits, all twelve of them (setuid/setgid/sticky
    /// included). What is *applied* on extraction is decided elsewhere.
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
    typeflag: u8,
    /// The target of a symlink, or the other name of a hard link.
    linkname: Vec<u8>,
    /// The two halves of a device node's number. Meaningful only for the `3`
    /// and `4` type flags; zero everywhere else, since ustar leaves the fields
    /// blank for every other type.
    devmajor: u64,
    devminor: u64,
    /// Owner and group *names*, which ustar stores beside the numbers. Empty
    /// in an archive written with `--numeric-owner`, and then `-tv` falls back
    /// to the numbers — which is what GNU does.
    uname: Vec<u8>,
    gname: Vec<u8>,
}

impl Member {
    /// Is this member a directory?
    ///
    /// The typeflag is authoritative, but the trailing slash is not merely a
    /// fallback for old archives: a v7 header has no typeflag at all, and a
    /// directory in one is recognisable only by the `/`.
    fn is_dir(&self) -> bool {
        self.typeflag == b'5'
            || (matches!(self.typeflag, b'0' | b'\0') && self.name.last() == Some(&b'/'))
    }

    /// Does this member carry data blocks after its header?
    ///
    /// The list is of the types that *cannot*, rather than of the types that
    /// can, and that inversion is the fix for a stream-desynchronising bug: a
    /// type flag this tar does not know — a GNU `L` long-name block, a `Z`
    /// nobody has defined — was assumed to carry nothing, so its data blocks
    /// were read as the next header, failed the checksum, and aborted the whole
    /// archive. Measured, GNU reads them: a five-byte member flagged `Z` is
    /// extracted with its five bytes and the member after it comes out intact.
    ///
    /// The size is already zero for the types listed here — [`decode_member`]
    /// clears it — so this is belt and braces; both are kept because the two
    /// answer different questions (how many blocks to skip, and what `-tv`
    /// prints in the size column).
    fn has_data(&self) -> bool {
        !self.is_dir() && !matches!(self.typeflag, b'1' | b'2' | b'3' | b'4' | b'5' | b'6')
    }

    /// The type flag to *render*, which is the stored one except that a v7
    /// directory — flagged as a regular file and recognisable only by its
    /// trailing slash — is reported as the directory it is.
    fn effective_typeflag(&self) -> u8 {
        if self.is_dir() { b'5' } else { self.typeflag }
    }
}

/// Is this a type flag with a defined meaning?
///
/// Anything else is rendered `?` by [`mode_string`], announced by `-tv` as an
/// `unknown file type`, and extracted as a plain file — which is what GNU does
/// with one, and is the only thing that can be done without knowing what it
/// meant.
fn known_typeflag(typeflag: u8) -> bool {
    matches!(
        typeflag,
        b'0' | b'\0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7'
    )
}

/// Whether a 512-byte block's stored checksum matches its contents.
///
/// This is the check that was missing, and its absence was not a nicety: with
/// no checksum test, `tar -tf not-an-archive` read 512 bytes of text, found a
/// NUL-free "name", printed it, and exited **0**. A caller cannot tell that
/// from an empty archive. GNU refuses the file outright.
///
/// Both the unsigned and the signed sum are accepted. Historic tars on
/// platforms with a signed `char` computed the sum with sign extension, so an
/// archive holding a member name with a byte above 0x7F — legal here, where a
/// name is bytes — can carry either. Refusing the signed form would reject
/// real archives written by real tars.
fn checksum_ok(block: &[u8; BLOCK_SIZE]) -> bool {
    let stored = parse_octal(block.get(148..156).unwrap_or(&[]));
    let mut unsigned: u64 = 0;
    let mut signed: i64 = 0;
    for (i, &raw) in block.iter().enumerate() {
        // The checksum field itself counts as eight spaces, which is how the
        // sum can cover a field that does not exist yet when it is computed.
        let b = if (148..156).contains(&i) { b' ' } else { raw };
        unsigned = unsigned.saturating_add(u64::from(b));
        let as_signed = i64::from(b).saturating_sub(if b >= 0x80 { 256 } else { 0 });
        signed = signed.saturating_add(as_signed);
    }
    stored == unsigned || i64::try_from(stored).is_ok_and(|s| s == signed)
}

/// Decode a header block that has already passed [`checksum_ok`].
fn decode_member(block: &[u8; BLOCK_SIZE]) -> Member {
    let name_field = field_bytes(block.get(..100).unwrap_or(&[]));
    // The `prefix` field is the whole reason ustar can hold a name longer than
    // 100 bytes, and it was never read. An archive of a deep tree therefore
    // listed and extracted every such member under its *last* 100 bytes —
    // `long/dd…dd/ff…ff` came out as `ff…ff` in the top-level directory, which
    // is silent misplacement of data, not a display bug.
    //
    // Honoured only when the magic says ustar: in the older v7 format those
    // bytes are padding, and reading them would invent a directory prefix out
    // of whatever happened to be there.
    let prefix = if block.get(257..262) == Some(b"ustar") {
        field_bytes(block.get(345..500).unwrap_or(&[]))
    } else {
        &[]
    };
    let mut name = Vec::with_capacity(
        prefix
            .len()
            .saturating_add(name_field.len())
            .saturating_add(1),
    );
    if !prefix.is_empty() {
        name.extend_from_slice(prefix);
        name.push(b'/');
    }
    name.extend_from_slice(name_field);

    let octal32 = |range: std::ops::Range<usize>| -> u32 {
        u32::try_from(parse_octal(block.get(range).unwrap_or(&[]))).unwrap_or(0)
    };
    let typeflag = block.get(156).copied().unwrap_or(0);
    // A link, a device or a fifo has no data, whatever its size field says, and
    // GNU does not believe the field either: it zeroes the size for these types
    // before anything reads it, so `-tv` prints `0` for a symlink whose header
    // claims five bytes. Believing the field instead would let a crafted header
    // make this program skip five blocks of a *following* member.
    let size = if matches!(typeflag, b'1' | b'2' | b'3' | b'4' | b'5' | b'6') {
        0
    } else {
        parse_octal(block.get(124..136).unwrap_or(&[]))
    };
    Member {
        name,
        mode: octal32(100..108),
        uid: octal32(108..116),
        gid: octal32(116..124),
        size,
        // A time before the epoch cannot be stored in an octal field, so the
        // only way this saturates is a hostile header; `i64::MAX` is then
        // refused by the clock rather than silently becoming a small number.
        mtime: i64::try_from(parse_octal(block.get(136..148).unwrap_or(&[]))).unwrap_or(i64::MAX),
        typeflag,
        linkname: field_bytes(block.get(157..257).unwrap_or(&[])).to_vec(),
        devmajor: parse_octal(block.get(329..337).unwrap_or(&[])),
        devminor: parse_octal(block.get(337..345).unwrap_or(&[])),
        uname: field_bytes(block.get(265..297).unwrap_or(&[])).to_vec(),
        gname: field_bytes(block.get(297..329).unwrap_or(&[])).to_vec(),
    }
}

/// Why a walk over an archive stopped.
///
/// Every variant but [`Stop::End`] used to be the same code path — `break` —
/// and the same exit status: zero. That is the defect this enum exists to
/// remove. A tool that cannot distinguish "the archive ended" from "the file
/// was never an archive" reports success for both.
#[cfg_attr(test, derive(Debug))]
enum Stop {
    /// Ran out of blocks at a header boundary. An archive may legally end
    /// without its two zero blocks and GNU accepts that in silence, so this is
    /// the *only* clean ending.
    End,
    /// Ended after a single zero block where the marker is a pair. Clean — GNU
    /// exits 0 — but it warns, and the warning carries the block's ordinal.
    LoneZeroBlock(u64),
    /// The first block was not a header: an empty file, a short read at offset
    /// zero, or a checksum that does not match.
    NotAnArchive,
    /// A later block was not a header.
    BadHeader,
    /// The stream ended inside a member's *data*. Note that ending inside a
    /// later *header* is not this — see [`walk`].
    Truncated,
    /// The archive could not be read at all — the classic case being a
    /// directory passed to `-f`, which opens and then fails at the first read.
    /// The flag is "this was the very first block", which GNU words differently
    /// ("At beginning of tape, quitting now").
    Unreadable(io::Error, bool),
    /// The handler asked to stop and has already reported why.
    Handler(i32),
}

/// Why [`read_block`] could not deliver a whole block.
enum ReadStop {
    /// Some bytes arrived, then the stream ended.
    Short,
    /// The read itself failed.
    Io(io::Error),
}

/// What a member handler did with the member's data blocks.
enum Handled {
    /// The handler read all of them.
    Consumed,
    /// The driver should skip them.
    Skip,
    /// The data ran out before the member did.
    Truncated,
    /// Stop the walk with this status; the reason is already reported.
    Stop(i32),
}

/// Read exactly one block. `Ok(None)` is a clean end at a block boundary.
fn read_block(input: &mut dyn Read, buf: &mut [u8; BLOCK_SIZE]) -> Result<Option<()>, ReadStop> {
    let mut filled = 0usize;
    while filled < BLOCK_SIZE {
        match input.read(buf.get_mut(filled..).unwrap_or(&mut [])) {
            Ok(0) => break,
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // A read *error* is not an end of file, and conflating the two is
            // how `tar -tf some-directory` came to exit 0 having printed
            // nothing: `read_exact` reports both as `Err(())`.
            Err(e) => return Err(ReadStop::Io(e)),
        }
    }
    if filled == 0 {
        Ok(None)
    } else if filled == BLOCK_SIZE {
        Ok(Some(()))
    } else {
        Err(ReadStop::Short)
    }
}

/// Walk an archive, handing each member header to `handle`.
///
/// The handler is responsible for the member's data blocks only when it returns
/// [`Handled::Consumed`]; otherwise the driver skips them, which is what keeps
/// the stream aligned when a member is refused.
fn walk<F>(input: &mut dyn Read, mut handle: F) -> Stop
where
    F: FnMut(&Member, &mut dyn Read) -> Handled,
{
    let mut first = true;
    // Counts every block consumed, so the lone-zero-block warning can name one.
    let mut ordinal = 0u64;
    loop {
        let mut block = [0u8; BLOCK_SIZE];
        match read_block(input, &mut block) {
            Ok(Some(())) => ordinal = ordinal.saturating_add(1),
            Ok(None) => {
                // An empty file is not an archive; an archive that simply ran
                // out after its last member is one that ended.
                return if first { Stop::NotAnArchive } else { Stop::End };
            }
            // A short read where a *header* should start is an ending, not a
            // truncation — GNU only calls it truncation when a member's data
            // runs out. Measured on a 3584-byte archive whose members are 6 and
            // 8 bytes: `head -c 512`, `-c 513` and `-c 700` all exit 0 in
            // silence, while `-c 1024` and `-c 1100` — which cut into the first
            // member's data — give "Unexpected EOF in archive". Only a short
            // read of the *first* block is rejected outright, as GNU's "This
            // does not look like a tar archive" (`head -c 300`).
            Err(ReadStop::Short) if first => return Stop::NotAnArchive,
            Err(ReadStop::Short) => return Stop::End,
            Err(ReadStop::Io(e)) => return Stop::Unreadable(e, first),
        }

        // The end-of-archive marker is *two* zero blocks. GNU accepts one and
        // exits 0, but warns, so look ahead once to tell the two apart.
        if block.iter().all(|&b| b == 0) {
            let mut next = [0u8; BLOCK_SIZE];
            return match read_block(input, &mut next) {
                Ok(Some(())) if next.iter().all(|&b| b == 0) => Stop::End,
                _ => Stop::LoneZeroBlock(ordinal),
            };
        }
        if !checksum_ok(&block) {
            return if first {
                Stop::NotAnArchive
            } else {
                Stop::BadHeader
            };
        }
        first = false;

        let member = decode_member(&block);
        // A header that passes the checksum and names nothing is *not* refused.
        // This used to `return Stop::BadHeader`, on the reasoning that an empty
        // name is not something to guess about — but GNU does not guess either,
        // it substitutes: `Substituting `.' for empty member name`, then lists
        // and extracts the member as `.`. Refusing it here made this tar abandon
        // the rest of a readable archive over one blank name field, which is a
        // strictly worse answer than GNU's; see [`PrefixNotice::strip`].
        let size = if member.has_data() { member.size } else { 0 };
        match handle(&member, input) {
            Handled::Consumed => {}
            Handled::Skip => {
                if !skip_data(input, size) {
                    return Stop::Truncated;
                }
            }
            Handled::Truncated => return Stop::Truncated,
            Handled::Stop(s) => return Stop::Handler(s),
        }
    }
}

/// Turn the reason a walk stopped into GNU's closing diagnostics and a status.
///
/// `label` is the archive's name in bytes, for the one message that mentions
/// it — `-` when the archive is standard input, as GNU spells it.
fn report_stop(stop: Stop, label: &[u8]) -> i32 {
    match stop {
        Stop::End => 0,
        Stop::LoneZeroBlock(n) => {
            // A warning, not an error: GNU prints this and still exits 0.
            // Measured — a 3584-byte archive cut to 3072 leaves one zero block
            // as its sixth, and GNU says "A lone zero block at 6", rc 0.
            diag!("tar: A lone zero block at {n}");
            0
        }
        Stop::NotAnArchive => {
            diag!("tar: This does not look like a tar archive");
            failed_with_previous_errors()
        }
        Stop::BadHeader => {
            // GNU scans forward for the next plausible header and says so. We
            // stop instead — the remaining bytes are of unknown provenance and
            // resynchronising on them is guessing — but the line it prints is
            // the same, because what a caller needs to know is that a header
            // was not where one was expected.
            diag!("tar: Skipping to next header");
            failed_with_previous_errors()
        }
        Stop::Truncated => {
            diag!("tar: Unexpected EOF in archive");
            fatal()
        }
        Stop::Unreadable(e, at_start) => {
            diag!("tar: {}: Cannot read: {}", escape(label), strerror(&e));
            if at_start {
                // GNU's phrasing for "nothing at all was read", inherited from
                // when the archive really was on tape. Kept because it is the
                // line that distinguishes an unreadable archive from one that
                // failed part-way through.
                diag!("tar: At beginning of tape, quitting now");
            }
            fatal()
        }
        Stop::Handler(s) => s,
    }
}

/// Number of 512-byte blocks a member of `size` bytes occupies.
fn data_blocks(size: u64) -> u64 {
    size.saturating_add(BLOCK_SIZE as u64 - 1)
        .saturating_div(BLOCK_SIZE as u64)
}

/// Consume and discard a member's data blocks so the next header is read from
/// the right offset. Returns false if the archive ended early.
fn skip_data(input: &mut dyn Read, size: u64) -> bool {
    let mut block = [0u8; BLOCK_SIZE];
    for _ in 0..data_blocks(size) {
        if input.read_exact(&mut block).is_err() {
            return false;
        }
    }
    true
}

// ============================================================================
// member selection, and the metadata an extraction restores
// ============================================================================

/// The operands after the archive: which members the caller asked for.
///
/// With none, everything is wanted. With some, only the named members and —
/// this is the part that is easy to get wrong — everything *under* a named
/// directory, because `tar -xf a.tar dir` is expected to unpack the subtree,
/// not the bare directory entry.
///
/// This did not exist. `tar -xf a.tar one-file` unpacked the entire archive,
/// which is not a cosmetic difference: it writes files the caller did not ask
/// for, over whatever was already there.
struct Selector {
    /// Each operand, trailing slashes trimmed, paired with "did anything match
    /// it". The flag is what makes `NAME: Not found in archive` possible.
    wanted: Vec<(Vec<u8>, bool)>,
}

impl Selector {
    fn new(members: &[OsString]) -> Self {
        Self {
            wanted: members
                .iter()
                .map(|m| (trim_slashes(&os_bytes(m)).to_vec(), false))
                .collect(),
        }
    }

    /// Does the caller want the member named `name`? Records the match.
    fn wants(&mut self, name: &[u8]) -> bool {
        if self.wanted.is_empty() {
            return true;
        }
        // The stored name of a directory ends in `/` and the operand normally
        // does not, so both sides are trimmed before they are compared.
        let n = trim_slashes(name);
        let mut hit = false;
        for (w, matched) in &mut self.wanted {
            let under = n.len() > w.len()
                && n.get(..w.len()) == Some(w.as_slice())
                && n.get(w.len()) == Some(&b'/');
            if n == w.as_slice() || under {
                *matched = true;
                hit = true;
            }
        }
        hit
    }

    /// Complain about every operand that named nothing, GNU's way, and return a
    /// non-zero status if there was one. Silence here is what let
    /// `tar -xf a.tar typo` succeed while extracting nothing.
    fn report_missing(&self) -> i32 {
        let mut status = 0;
        for (w, matched) in &self.wanted {
            if !matched {
                diag!("tar: {}: Not found in archive", escape(w));
                status = EXIT_FATAL;
            }
        }
        status
    }
}

/// Drop trailing `/` from a member name or an operand, but never reduce a name
/// to nothing — `/` alone stays `/` rather than becoming the empty string,
/// which would then match every member.
fn trim_slashes(name: &[u8]) -> &[u8] {
    let mut end = name.len();
    while end > 1 && name.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    name.get(..end).unwrap_or(name)
}

// The umask has to be read to be known — POSIX gives no read-only spelling, so
// reading it means setting it — and `std` exposes no wrapper.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// The process umask, read once and left as it was found.
///
/// Two halves, each needed for a measured reason:
///
/// *Cached*, because reading is destructive — the call sets the mask and returns
/// the old value, so a naive second call would answer whatever the first one
/// stored.
///
/// *Restored*, because the umask still has a job to do here. This tar sets the
/// mode of every member it extracts explicitly, so for those the kernel's mask
/// is irrelevant; but the parent directories it creates implicitly on the way to
/// a member (`dir/sub/f` extracted on its own) are left to `mkdir`, and GNU
/// lets the umask gate those. Leaving the mask at `0` made them 0777 where GNU
/// produced 0755 — visible in `scripts/tar-diff.sh` as a mode mismatch on an
/// implicitly created parent.
#[cfg(unix)]
fn read_umask() -> u32 {
    use std::sync::OnceLock;
    static UMASK: OnceLock<u32> = OnceLock::new();
    *UMASK.get_or_init(|| {
        // SAFETY: `umask` is a POSIX call that cannot fail and touches only this
        // process's file-mode creation mask. The pair leaves the mask exactly as
        // it was found; it is racy only against another thread creating a file
        // in between, and this runs before any extraction work starts.
        unsafe {
            let old = umask(0);
            umask(old);
            old
        }
    })
}

/// [`read_umask`] on the target; `0` on a host that has no such thing, so that
/// the pure arithmetic in [`extraction_mode`] is still testable there.
#[cfg(not(unix))]
fn read_umask() -> u32 {
    0
}

/// The mode an extracted member actually gets.
///
/// Measured against GNU as a non-root user: by default the stored mode is
/// masked by the umask *and* stripped of setuid, setgid and sticky — a 0777
/// file lands 0755 under umask 022 and 0700 under umask 077, and an 04755 file
/// lands 0755. With `-p` the stored mode is applied whole, setuid included.
///
/// The reasoning behind the default is worth stating because it looks
/// over-cautious: an archive is an untrusted input, and honouring a setuid bit
/// out of one would let anyone who can hand you a tarball hand you a setuid
/// binary. `-p` is the caller saying they know where the archive came from.
///
/// Pure, so it can be unit-tested on every host rather than only where a real
/// umask exists.
fn extraction_mode(stored: u32, same_permissions: bool, umask: u32) -> u32 {
    if same_permissions {
        stored & 0o7777
    } else {
        stored & 0o777 & !umask
    }
}

/// Apply a member's stored mode and mtime to a path that has been created.
///
/// Both were dropped entirely: an extracted file kept whatever mode
/// `File::create` gave it and whatever time it was written at, so unpacking a
/// tree of scripts produced a tree of non-executable files, and every `make`
/// run after an unpack rebuilt everything.
///
/// The two failures are reported separately, and neither aborts the other: a
/// filesystem that cannot store a timestamp can usually still store a mode, and
/// getting one of the two right is better than getting neither.
///
/// The wording of the mode failure is GNU's, symbolic bits and all
/// (`Cannot change mode to rwxr-xr-x`), which is why [`mode_string`] is shared
/// with the `-tv` listing rather than each having its own.
fn restore_metadata(at: &Located, name: &[u8], mode: u32, mtime: i64, status: &mut i32) {
    if let Err(e) = at.set_mtime(mtime) {
        diag!("tar: {}: Cannot utime: {}", escape(name), strerror(&e));
        *status = EXIT_FATAL;
    }
    if let Err(e) = at.set_mode(mode) {
        let bits = mode_string(mode, b'0');
        diag!(
            "tar: {}: Cannot change mode to {}: {}",
            escape(name),
            String::from_utf8_lossy(bits.get(1..).unwrap_or(&[])),
            strerror(&e)
        );
        *status = EXIT_FATAL;
    }
}

/// Create something for the member called `name`, beneath `root`, replacing
/// whatever is already standing there.
///
/// Two steps, and the split between them is the security boundary. First the
/// member's **parent** is resolved beneath `root` — see [`Dir::locate`], which
/// is what refuses to walk through a symlink out of the destination. Then the
/// leaf is created relative to the descriptor that came back, so the kernel
/// never re-resolves the path that was just vetted.
///
/// The recovery is GNU's `maybe_recoverable`, and the order it does things in
/// is observable. The creation is attempted *first*; only an `EEXIST` provokes
/// a removal and a second attempt. That is why extracting a symlink over a
/// **non-empty** directory reports `File exists` and not `Directory not empty`
/// — the removal's failure is discarded and the original `EEXIST` is what the
/// caller sees. Measured both ways: over an *empty* directory the symlink is
/// created (so the removal is an `rmdir` as well as an `unlink`), and over a
/// non-empty one GNU says `Cannot create symlink to ‘f’: File exists`.
///
/// `unlink` is tried before `rmdir` because the overwhelmingly common case is a
/// file, and because `unlink` on a directory is the cheap failure.
///
/// Generic in the created thing so the one recovery serves every member type.
/// A regular member yields the open [`File`]; the others yield `()`. Before,
/// regular files had a second, subtly different copy of this in
/// `open_for_member`, and the two drifted: only this one replaced an existing
/// entry, which is how `create_file` came to write through a hard link.
///
/// The resolved location is handed back with the result because every caller
/// wants it afterwards — to stamp a mode and an mtime on what it just made, or
/// to write the member's data into it.
fn create_at<T, F>(
    root: &Dir,
    name: &[u8],
    ovw: Overwriting,
    status: &mut i32,
    create: F,
) -> Result<(Located, T), NotCreated>
where
    F: Fn(&Located) -> io::Result<T>,
{
    // Resolving the parent is what used to be an `ENOENT` from the creation
    // itself: an archive may store `a/b/link` without storing `a/`, and the
    // resolution is the thing that discovers the ancestor is absent. Ancestors
    // are made *only* on that error — never speculatively — because a `mkdir -p`
    // run before the attempt is what turns a withheld symlink into a traversal.
    let loc = match root.locate(name) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            make_ancestors(root, name, status);
            // Retried even when nothing was made: GNU reports the *second*
            // attempt's error, and this is the line that puts its `Cannot mkdir`
            // message before the caller's `Cannot open` one.
            root.locate(name)?
        }
        other => other?,
    };
    // `-U` is the one member of the family that acts *before* the attempt, and
    // that ordering is the whole of it: the removal's own failure is reported
    // and the member abandoned, where the default's identical removal happens
    // only after an `EEXIST` and has its failure discarded. Measured, that is
    // the difference between `tar: dir: Cannot unlink: Directory not empty`
    // (`-U`) and `tar: dir: Cannot open: File exists` (default).
    if ovw.old_files == OldFiles::UnlinkFirst
        && let Err(e) = loc.clear_for_unlink_first()
    {
        diag!("tar: {}: Cannot unlink: {}", escape(name), strerror(&e));
        *status = EXIT_FATAL;
        return Err(NotCreated::Silent);
    }
    match create(&loc) {
        Err(first) if in_the_way(&first) => match ovw.old_files {
            // A directory member is the family's standing exception: `-k` steps
            // over an existing directory without a word, where for every other
            // type it is an error. Measured — `tar -xkf` over a tree it has
            // already unpacked prints nothing about the directories and exits 0
            // if nothing else is in the way.
            OldFiles::Keep if ovw.directory_member => Err(NotCreated::Silent),
            // The original `EEXIST` is handed back rather than a sentence of our
            // own, so the caller phrases it in the member type's own operation:
            // `Cannot open: File exists`, `Cannot mkfifo: File exists`,
            // `Cannot create symlink to ‘t’: File exists`. That is GNU's shape
            // because GNU likewise just stops suppressing the error.
            OldFiles::Keep => Err(NotCreated::Failed(first)),
            OldFiles::Skip => {
                // Every member type, directories included — unlike `-k`, whose
                // silence about a directory is total. And only under `-v`:
                // without it `--skip-old-files` says nothing at all.
                if ovw.verbose {
                    diag!("tar: {}: skipping existing file", escape(name));
                }
                Err(NotCreated::Silent)
            }
            _ => {
                if loc.unlink().is_err() && loc.rmdir().is_err() {
                    return Err(NotCreated::Immovable(first));
                }
                create(&loc).map(|v| (loc, v)).map_err(NotCreated::Failed)
            }
        },
        other => other.map(|v| (loc, v)).map_err(NotCreated::Failed),
    }
}

/// Is this failure "something is already standing there"?
///
/// `EEXIST` for every creating call that passes `O_EXCL` or has no choice
/// (`mkdir`, `symlinkat`, `mkfifoat`, `linkat`), and `ELOOP` for the one that
/// does not: `--overwrite`'s open passes `O_TRUNC|O_NOFOLLOW`, so a symlink in
/// the way is refused as a link that must not be followed rather than as an
/// entry that already exists. Both mean the same thing to the recovery below,
/// and treating only the first as recoverable would leave `--overwrite` unable
/// to replace a symlink — which GNU does (`tar-ovw2.sh`).
fn in_the_way(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::AlreadyExists || e.raw_os_error() == Some(ELOOP)
}

/// Why [`create_at`] created nothing.
///
/// Three outcomes rather than one error, because the overwrite-control family
/// added a way to decline that is not a failure. Collapsing them into an
/// `io::Error` would force every caller to guess which of its `Cannot …`
/// sentences to print, and `--skip-old-files` prints none of them.
enum NotCreated {
    /// The creating call failed, and the caller should say so in its own
    /// wording. This is the only arm that sets the exit status, and it is the
    /// caller that sets it.
    Failed(io::Error),
    /// Something was already standing there, the policy called for removing it,
    /// and the removal failed — a non-empty directory, in practice.
    ///
    /// Carries the *creation's* first error rather than the removal's, because
    /// that is what all but one caller prints: GNU discards the failed removal
    /// and reports the original `EEXIST`, so a symlink member over a non-empty
    /// directory says `Cannot open: File exists` and not `Directory not empty`.
    /// The one exception is a directory member, which has its own sentence for
    /// it (`Unexpected inconsistency when making directory`) and is the only
    /// member type that can reach this at all under a default extraction —
    /// [`Located::mkdir_member`] accepts an existing directory as success, so
    /// only `--keep-newer-files`, which must use a plain `mkdir`, gets here.
    Immovable(io::Error),
    /// Nothing was created and nothing more is to be said. Either
    /// `--skip-old-files` stepped over an existing entry (exit status
    /// untouched), or `-k` met a directory that was already there (likewise),
    /// or `-U` could not clear the way and has already reported it.
    Silent,
}

impl From<io::Error> for NotCreated {
    fn from(e: io::Error) -> Self {
        Self::Failed(e)
    }
}

/// What [`create_at`] needs to know about the extraction it is part of.
///
/// Passed as one `Copy` value rather than three parameters because it is
/// threaded through six member-type arms and two helpers, and a bare
/// `(OldFiles, bool, bool)` at each of those call sites is two booleans nobody
/// can tell apart.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Overwriting {
    /// What to do about an entry that is already there. See [`OldFiles`].
    old_files: OldFiles,
    /// Whether `-v` was given, which is the only thing that decides whether
    /// `--skip-old-files` announces what it stepped over.
    verbose: bool,
    /// Whether the member being created is a **directory**, which `-k` treats
    /// differently from every other type: silently, and without restoring the
    /// member's mode or mtime onto the directory that was already there.
    directory_member: bool,
}

/// Create a member's missing ancestor directories, GNU's way.
///
/// Not `create_dir_all`. The two agree whenever they succeed, and agree on the
/// mode (`0777 & ~umask`) and the mtime (now) of what they invent — but they
/// report different failures, and the message is observable. `create_dir_all`
/// stops at the first component it cannot make and names *that* one; GNU keeps
/// going, because a failure part-way up does not prove the rest are hopeless,
/// and names the last one it tried.
///
/// The rule, measured across `tar-rules7.sh` and `tar-rules9.sh`:
///
/// * walk the ancestors left to right, skipping empty components (a leading or
///   doubled `/`), and `mkdir` each;
/// * `EEXIST` is not a failure — that ancestor is simply already there;
/// * any other failure is *remembered* and the walk continues;
/// * `ENOENT` is remembered and stops the walk, since a missing grandparent
///   really does doom everything deeper;
/// * at the end, the remembered failure — if any — is the one reported.
///
/// The case that forces this shape is an unwritable destination with two levels
/// missing: `mkdir a` fails `EACCES`, and GNU nonetheless goes on to `mkdir a/b`
/// and reports *that* — `a/b: Cannot mkdir: No such file or directory`, not
/// `a: … Permission denied`. And with `a` present but unwritable, member
/// `a/b/c/d` reports `a/b/c`, three components in, which no stop-at-the-first
/// rule can produce.
///
/// The caller retries the creation afterwards and reports its own failure too,
/// so a failed ancestor produces the two lines GNU prints, in GNU's order.
fn make_ancestors(root: &Dir, name: &[u8], status: &mut i32) {
    // (end offset of the ancestor, why it could not be made)
    let mut failure: Option<(usize, io::Error)> = None;
    for (i, byte) in name.iter().enumerate() {
        if *byte != b'/' || i == 0 || name.get(i.wrapping_sub(1)) == Some(&b'/') {
            continue;
        }
        // The *prefix* of the name, separators and all, rather than the
        // components rejoined: it is what the diagnostic prints, and a name
        // with a doubled slash must be named back the way it was written.
        let ancestor = name.get(..i).unwrap_or(name);
        match root.locate(ancestor).and_then(|at| at.mkdir()) {
            Ok(()) => failure = None,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => failure = None,
            Err(e) => {
                let doomed = e.kind() == io::ErrorKind::NotFound;
                failure = Some((i, e));
                if doomed {
                    break;
                }
            }
        }
    }
    if let Some((end, e)) = failure {
        let ancestor = name.get(..end).unwrap_or(name);
        diag!("tar: {}: Cannot mkdir: {}", escape(ancestor), strerror(&e));
        *status = EXIT_FATAL;
    }
}

/// A path as a NUL-terminated byte string, or `None` if it contains a NUL.
///
/// Bytes throughout: the name came out of an archive header and nothing
/// promises it is text, so converting through `str` would be the UTF-8
/// assumption rule 7 forbids. A NUL inside is refused rather than truncated,
/// because a C call handed one would act on the *prefix* — a member named
/// `safe\0/../../etc/passwd` must not become `safe`.
#[cfg(unix)]
fn c_path(path: &Path) -> Option<Vec<u8>> {
    let bytes = os_bytes(path.as_os_str());
    if bytes.contains(&0) {
        return None;
    }
    let mut buf = Vec::with_capacity(bytes.len().saturating_add(1));
    buf.extend_from_slice(&bytes);
    buf.push(0);
    Some(buf)
}

/// The error a C call gets when the path it was handed cannot be expressed.
#[cfg(unix)]
fn embedded_nul() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte")
}

/// A single path component as a NUL-terminated byte string.
#[cfg(unix)]
fn c_name(component: &[u8]) -> io::Result<Vec<u8>> {
    if component.contains(&0) {
        return Err(embedded_nul());
    }
    let mut buf = Vec::with_capacity(component.len().saturating_add(1));
    buf.extend_from_slice(component);
    buf.push(0);
    Ok(buf)
}

// The syscalls `std` does not wrap. Declared here, beside their callers, rather
// than pulled from a libc binding — this crate depends on none, and the
// signatures are short enough to check against `posix/src/file.rs` by eye.
//
// They are the `*at` forms, taking a directory descriptor and a single
// component, and that is not a stylistic preference: it is half of the defence
// described on [`Dir::locate`]. Creating by *path* would make the kernel
// re-resolve every component, following any symlink it met, so no amount of
// checking beforehand could stop somebody who swapped a component in between.
#[cfg(unix)]
unsafe extern "C" {
    fn openat(dirfd: i32, path: *const u8, flags: i32, mode: u32) -> i32;
    fn close(fd: i32) -> i32;
    fn readlinkat(dirfd: i32, path: *const u8, buf: *mut u8, bufsiz: usize) -> isize;
    fn mkdirat(dirfd: i32, path: *const u8, mode: u32) -> i32;
    fn symlinkat(target: *const u8, newdirfd: i32, linkpath: *const u8) -> i32;
    fn linkat(
        olddirfd: i32,
        oldpath: *const u8,
        newdirfd: i32,
        newpath: *const u8,
        flags: i32,
    ) -> i32;
    fn mkfifoat(dirfd: i32, path: *const u8, mode: u32) -> i32;
    fn mknodat(dirfd: i32, path: *const u8, mode: u32, dev: u64) -> i32;
    fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32;
    fn fchmodat(dirfd: i32, path: *const u8, mode: u32, flags: i32) -> i32;
    fn utimensat(dirfd: i32, path: *const u8, times: *const CTimespec, flags: i32) -> i32;
    fn fstatat(dirfd: i32, path: *const u8, buf: *mut CStat, flags: i32) -> i32;
}

/// The `open` flags used here, as Linux numbers them and as
/// `posix/src/fcntl.rs` declares them.
#[cfg(unix)]
mod oflag {
    pub const RDONLY: i32 = 0;
    pub const WRONLY: i32 = 1;
    pub const CREAT: i32 = 0o100;
    pub const EXCL: i32 = 0o200;
    pub const TRUNC: i32 = 0o1000;
    pub const NONBLOCK: i32 = 0o4000;
    pub const DIRECTORY: i32 = 0o200_000;
    pub const NOFOLLOW: i32 = 0o400_000;
    pub const CLOEXEC: i32 = 0o2_000_000;
}

/// `AT_REMOVEDIR` — `unlinkat` should `rmdir` rather than `unlink`.
#[cfg(unix)]
const AT_REMOVEDIR: i32 = 0x200;

/// `AT_SYMLINK_NOFOLLOW` — act on the link, not on what it names.
#[cfg(unix)]
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;

/// `AT_FDCWD` — resolve a relative path against the working directory.
#[cfg(unix)]
const AT_FDCWD: i32 = -100;

/// `EXDEV`, which is what a resolution that would leave the destination reports.
///
/// An odd errno for a symlink, and deliberately GNU's: `openat2` returns it for
/// a `RESOLVE_BENEATH` violation, so a tar built on that call reports "Invalid
/// cross-device link" for a link that never crossed a device. Ours says the same
/// thing because the *messages* are the interface, and a log that has to be read
/// beside GNU's should not have two spellings of one refusal.
#[cfg(unix)]
const EXDEV: i32 = 18;

/// `ELOOP`, for a chain of symlinks with no end — and for the `O_NOFOLLOW` open
/// that refuses to start one, which is how `--overwrite` meets a symlink.
///
/// Not `#[cfg(unix)]` like its neighbours because [`in_the_way`] compares
/// against it on every host; off unix nothing produces the number, so the test
/// is simply never true there.
const ELOOP: i32 = 40;

/// How many symlinks one member's parent may be resolved through.
///
/// Linux's own limit is 40, and matching it means a tree this tar can unpack is
/// a tree the rest of the system can then open.
#[cfg(unix)]
const MAX_SYMLINK_HOPS: u32 = 40;

/// The refusal that keeps an extraction inside the directory it was pointed at.
#[cfg(unix)]
fn escapes_destination() -> io::Error {
    io::Error::from_raw_os_error(EXDEV)
}

/// Split a `/`-separated name into components, dropping the empty ones a
/// leading or doubled slash produces, and the `.`s that name no step.
fn components(name: &[u8]) -> Vec<Vec<u8>> {
    name.split(|b| *b == b'/')
        .filter(|c| !c.is_empty() && *c != b".")
        .map(<[u8]>::to_vec)
        .collect()
}

/// An open handle on a directory.
///
/// It owns the descriptor and closes it on drop, which is the only reason this
/// is a type rather than a bare `i32`: [`Dir::locate`] keeps a stack of these
/// and unwinds it on every `..` and on every failure.
#[cfg(unix)]
struct Dir(i32);

#[cfg(unix)]
impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `openat` and is owned solely by this value
        // — `Dir` is not `Copy` and never hands the number out — so this is the
        // one and only close of it.
        unsafe { close(self.0) };
    }
}

#[cfg(unix)]
impl Dir {
    /// Open the destination root. The one place a directory is opened by path.
    fn open_root(path: &Path) -> io::Result<Self> {
        let Some(cpath) = c_path(path) else {
            return Err(embedded_nul());
        };
        Self::open_child(AT_FDCWD, &cpath)
    }

    /// `openat` for a directory, refusing to follow a symlink.
    ///
    /// `O_NOFOLLOW` is the load-bearing flag. Without it this walk would follow
    /// exactly the links it exists to catch; with it, a symlink component fails
    /// (`ELOOP` on Linux) and the caller gets to decide whether the target is
    /// somewhere the extraction may go.
    fn open_child(dirfd: i32, cname: &[u8]) -> io::Result<Self> {
        // SAFETY: `cname` is NUL-terminated and outlives the call, which does
        // not retain the pointer. The mode argument is unused without `O_CREAT`.
        let fd = unsafe {
            openat(
                dirfd,
                cname.as_ptr(),
                oflag::RDONLY | oflag::DIRECTORY | oflag::NOFOLLOW | oflag::CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(fd))
        }
    }

    /// A second, independent handle on the same directory.
    fn reopen(&self) -> io::Result<Self> {
        Self::open_child(self.0, b".\0")
    }

    /// Read a component as a symlink, or `None` if it is not one.
    fn read_link(dirfd: i32, cname: &[u8]) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; 4096];
        // SAFETY: `cname` is NUL-terminated; `buf` is a live allocation of
        // exactly the length passed, and `readlinkat` writes no more than that.
        let n = unsafe { readlinkat(dirfd, cname.as_ptr(), buf.as_mut_ptr(), buf.len()) };
        let n = usize::try_from(n).ok()?;
        // A target that filled the buffer may have been cut short, and acting on
        // the *prefix* of a symlink target is how a check gets fooled.
        if n >= buf.len() {
            return None;
        }
        buf.truncate(n);
        Some(buf)
    }

    /// Resolve `name`'s parent directory, refusing to leave this one.
    ///
    /// This is the rule GNU gets from `openat2(RESOLVE_BENEATH)`, emulated on
    /// primitives every target here has, because ours does not enforce the
    /// resolve flags (`posix/src/file.rs`: "accepted but not enforced") and
    /// glibc exports no wrapper for the call at all.
    ///
    /// # Why it exists
    ///
    /// Every other defence in this program is about names the *archive*
    /// chooses: `strip_leading` cuts a name back past its last `..`,
    /// `contains_dot_dot` refuses what survives, and [`is_delayed_target`]
    /// withholds a symlink member that points out of the tree until the last
    /// member has been written. None of them says anything about a symlink that
    /// was **already on disk** when tar started. Unpacking into a directory
    /// where `x -> ../../elsewhere` already exists used to follow it and write
    /// `x/f` outside the destination, in silence, exit 0 — and it took only two
    /// ordinary archives to arrange, the first holding `x -> ../../elsewhere`
    /// as a perfectly normal symlink member and the second holding `x/f`.
    /// See `known-issues.md` →
    /// `B-tar-WALKS-THROUGH-A-PRE-EXISTING-SYMLINK-AND-WRITES-OUTSIDE-THE-DESTINATION`.
    ///
    /// # The rule, measured against GNU tar 1.35
    ///
    /// A symlink ancestor is *followed* when it stays beneath the destination
    /// and *refused* otherwise, judged step by step rather than by resolving
    /// the whole path and comparing:
    ///
    /// | target of an ancestor link | |
    /// |---|---|
    /// | `sub`, `deep/../sub`, `deep/er/../..` | followed |
    /// | `/anything`, **including the destination root itself** | refused |
    /// | `../out`, and `../dest/sub` that comes straight back in | refused |
    /// | a chain, if every hop is beneath | followed |
    ///
    /// The two refusals nothing else would produce are the ones that pin the
    /// rule down: an *absolute* target is refused however harmless it is, and a
    /// `..` is refused the moment it would step above the root even if a later
    /// component returns. Canonicalising and checking the prefix would allow
    /// both. Measured in `tar-rules16.sh`; `tar-rules14.sh`'s earlier table
    /// looked like evidence for the same conclusion but was not — its targets
    /// passed back through the very link under test, so they were resolution
    /// loops refused for an unrelated reason.
    ///
    /// # Shape
    ///
    /// A stack of open descriptors, one per level below the root, so `..` is a
    /// pop and popping the root is the refusal. Symlink targets are spliced
    /// into the front of the pending components rather than resolved
    /// separately, which is what makes a chain cost nothing extra and keeps one
    /// hop counter honest across all of them.
    ///
    /// Holding a descriptor per level is also what makes this race-free: the
    /// caller creates relative to the descriptor that came back, so there is no
    /// second resolution for anyone to interfere with. A check-then-create by
    /// path would be correct on a quiet disk and defeatable on a busy one.
    fn locate(&self, name: &[u8]) -> io::Result<Located> {
        let mut pending: std::collections::VecDeque<Vec<u8>> = components(name).into();
        // No components at all: `.`, `/`, `./`, or the empty name — all of them
        // the destination itself. That is not an error and must not be turned
        // into one: `tar -cf x.tar .` stores a `./` directory member, and
        // extracting it applies the archive's mode and timestamp to the
        // destination directory. Naming the root `.` relative to the root's own
        // descriptor lets every operation below take its natural course —
        // `mkdirat` says `EEXIST` and the member is accepted, `openat(O_EXCL)`
        // says `EEXIST` and is reported, `linkat` says `EPERM` because a
        // directory cannot be hard-linked. Each is what GNU prints. Returning
        // `EINVAL` here instead made all three read `Invalid argument` and lost
        // the `./` member's metadata entirely.
        let leaf = pending.pop_back().unwrap_or_else(|| b".".to_vec());
        // Never as the leaf, whatever the caller thought it was asking for.
        // `contains_dot_dot` already refuses these upstream, but this function
        // is the boundary and must not depend on having been called correctly.
        if leaf == b".." {
            return Err(escapes_destination());
        }
        let mut stack = vec![self.reopen()?];
        let mut hops: u32 = 0;
        while let Some(comp) = pending.pop_front() {
            if comp == b".." {
                // Length 1 is the root itself; popping it is the escape.
                if stack.len() <= 1 {
                    return Err(escapes_destination());
                }
                stack.pop();
                continue;
            }
            let cname = c_name(&comp)?;
            let Some(top) = stack.last().map(|d| d.0) else {
                return Err(escapes_destination());
            };
            match Dir::open_child(top, &cname) {
                Ok(dir) => stack.push(dir),
                Err(e) => {
                    // The open refused a symlink, or the component is not a
                    // directory, or it is not there at all. `readlinkat` is what
                    // tells the first case from the rest, and it is tried
                    // unconditionally rather than on a particular errno: the
                    // code for "would have followed a symlink" is `ELOOP` on
                    // Linux and is not promised to be anywhere else.
                    let Some(target) = Dir::read_link(top, &cname) else {
                        return Err(e);
                    };
                    hops = hops.saturating_add(1);
                    if hops > MAX_SYMLINK_HOPS {
                        return Err(io::Error::from_raw_os_error(ELOOP));
                    }
                    if target.first() == Some(&b'/') {
                        return Err(escapes_destination());
                    }
                    for c in components(&target).into_iter().rev() {
                        pending.push_front(c);
                    }
                }
            }
        }
        let Some(dir) = stack.pop() else {
            return Err(escapes_destination());
        };
        Ok(Located { dir, leaf })
    }
}

/// Somewhere a member may be created: the directory it goes in, held open, and
/// the one component naming it there.
///
/// Produced only by [`Dir::locate`], which is what makes holding one a proof
/// that the place is beneath the destination.
#[cfg(unix)]
struct Located {
    dir: Dir,
    leaf: Vec<u8>,
}

#[cfg(unix)]
impl Located {
    /// The result of an `*at` call that returns 0 or -1.
    fn checked(rc: i32) -> io::Result<()> {
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Open the leaf, `O_CLOEXEC` added to whatever else was asked for.
    fn open(&self, flags: i32, mode: u32) -> io::Result<File> {
        use std::os::unix::io::FromRawFd;
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call.
        let fd = unsafe { openat(self.dir.0, cname.as_ptr(), flags | oflag::CLOEXEC, mode) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just returned by `openat`, is not -1, and is owned
        // here — nothing else holds it, so handing it to `File` transfers the
        // sole responsibility for closing it.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// `mkdir`, plain: `EEXIST` comes straight back out.
    fn mkdir(&self) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call. 0o777 is
        // masked by the umask, which is what `fs::create_dir` passes too.
        Self::checked(unsafe { mkdirat(self.dir.0, cname.as_ptr(), 0o777) })
    }

    /// `mkdir` for a directory *member*, where an existing directory is success.
    ///
    /// The `EEXIST` is passed through for anything that is *not* a directory, so
    /// [`create_at`] removes the obstacle and tries again. That is GNU's
    /// behaviour and it matters twice over: a directory member extracted over a
    /// plain file replaces the file (measured, `tar-rules11.sh` case 4), and one
    /// extracted over a **symlink pointing at a directory** replaces the
    /// *symlink* (`tar-rules12.sh`) instead of quietly extracting through it
    /// into wherever it pointed. `create_dir_all`, which this replaced, did the
    /// opposite: it asks `is_dir()`, which follows the link, sees a directory
    /// and reports success — leaving the symlink in place for every member that
    /// followed to be written through. Not following the link is the whole of
    /// the difference, and [`Dir::open_child`]'s `O_NOFOLLOW` is where it lives.
    fn mkdir_member(&self) -> io::Result<()> {
        match self.mkdir() {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && self.is_real_dir() => Ok(()),
            other => other,
        }
    }

    /// Is the leaf a directory in its own right, rather than a link to one?
    fn is_real_dir(&self) -> bool {
        c_name(&self.leaf).is_ok_and(|cname| Dir::open_child(self.dir.0, &cname).is_ok())
    }

    fn symlink(&self, target: &[u8]) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        let ctarget = c_name(target)?;
        // SAFETY: both strings are NUL-terminated and outlive the call.
        Self::checked(unsafe { symlinkat(ctarget.as_ptr(), self.dir.0, cname.as_ptr()) })
    }

    /// Hard-link this leaf to `target`, which must have been resolved beneath
    /// the same root — GNU confines the target as well as the link
    /// (`tar-rules17.sh`: a target reached through an escaping symlink reports
    /// `Cannot hard link to ‘x/secret’: Invalid cross-device link`).
    fn hard_link_to(&self, target: &Self) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        let ctarget = c_name(&target.leaf)?;
        // SAFETY: both strings are NUL-terminated and outlive the call; the two
        // descriptors are live for the duration because `self` and `target` are
        // borrowed. `flags` is 0: a hard link to a symlink stores the link.
        Self::checked(unsafe {
            linkat(
                target.dir.0,
                ctarget.as_ptr(),
                self.dir.0,
                cname.as_ptr(),
                0,
            )
        })
    }

    fn mkfifo(&self, mode: u32) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call. `mkfifoat`
        // supplies the `S_IFIFO` bit itself.
        Self::checked(unsafe { mkfifoat(self.dir.0, cname.as_ptr(), mode & 0o7777) })
    }

    fn mknod(&self, mode: u32, dev: u64) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call. `mode`
        // carries the `S_IFCHR`/`S_IFBLK` bit the caller put there.
        Self::checked(unsafe { mknodat(self.dir.0, cname.as_ptr(), mode, dev) })
    }

    fn unlink(&self) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call.
        Self::checked(unsafe { unlinkat(self.dir.0, cname.as_ptr(), 0) })
    }

    fn rmdir(&self) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: as above; `AT_REMOVEDIR` makes this an `rmdir`.
        Self::checked(unsafe { unlinkat(self.dir.0, cname.as_ptr(), AT_REMOVEDIR) })
    }

    fn chmod(&self, mode: u32) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        // SAFETY: `cname` is NUL-terminated and outlives the call.
        Self::checked(unsafe { fchmodat(self.dir.0, cname.as_ptr(), mode, 0) })
    }

    /// Stamp the leaf's access and modification times to the same second.
    ///
    /// `flags` is `0` to follow a final symlink or [`AT_SYMLINK_NOFOLLOW`] to
    /// stamp the link itself.
    ///
    /// Named after the syscall rather than after `File::set_times` for a reason
    /// that only appeared once this tar could create a **fifo**: stamping by
    /// opening and calling `futimens` means opening a fifo, and opening a fifo
    /// blocks until somebody opens the other end. An archive holding one named
    /// pipe was therefore a hang — `tar -xf` sat there for ever with no output
    /// and no way to tell it from slow I/O. `utimensat` acts on the name and
    /// never opens anything, which is also why GNU uses it.
    fn stamp(&self, mtime: i64, flags: i32) -> io::Result<()> {
        let cname = c_name(&self.leaf)?;
        let t = CTimespec {
            tv_sec: mtime,
            tv_nsec: 0,
        };
        let times = [t, t];
        // SAFETY: `cname` is NUL-terminated and `times` is exactly the
        // two-element array `utimensat` reads; both outlive the call, which
        // retains neither.
        Self::checked(unsafe { utimensat(self.dir.0, cname.as_ptr(), times.as_ptr(), flags) })
    }

    /// The `(device, inode)` pair identifying whatever is at the leaf now.
    ///
    /// `fstatat`, not an open. This was `openat(O_RDONLY|O_NOFOLLOW)` plus
    /// `File::metadata`, to avoid declaring a `struct stat` by hand — and that
    /// broke the delayed-symlink placeholder outright, because the placeholder
    /// is created **mode 0** and an unprivileged process cannot open a mode-0
    /// file it owns. Every archive holding a symlink to an absolute or climbing
    /// target failed with `tar: x: Cannot open: Permission denied`. Asking about
    /// a name is not the same as asking to read it, and only the second needs
    /// permission.
    ///
    /// `AT_SYMLINK_NOFOLLOW` so a link that appeared since is *not* the file we
    /// made, and so a dangling one still answers rather than erroring.
    fn identity(&self) -> io::Result<(u64, u64)> {
        let cname = c_name(&self.leaf)?;
        let mut st = CStat::default();
        // SAFETY: `cname` is NUL-terminated and `st` is a `CStat`, which is the
        // layout both C libraries this links against declare for `struct stat`
        // (see the type's own comment); the call fills it and retains neither.
        Self::checked(unsafe {
            fstatat(self.dir.0, cname.as_ptr(), &raw mut st, AT_SYMLINK_NOFOLLOW)
        })?;
        Ok((st.st_dev, st.st_ino))
    }
}

/// `struct stat`, in the layout `posix/src/stat.rs` declares — which is itself
/// documented as matching Linux x86-64's, so the one declaration serves both
/// the host build and the SlateOS one.
///
/// Only `st_dev` and `st_ino` are read. The rest is present so the struct is
/// the right *size*: `fstatat` writes all of it, and a short buffer would be a
/// stack overwrite rather than a wrong answer.
#[repr(C)]
#[derive(Default)]
#[cfg_attr(not(unix), allow(dead_code))]
struct CStat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    _pad0: i32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atim: CTimespec,
    st_mtim: CTimespec,
    st_ctim: CTimespec,
    _reserved: [i64; 3],
}

/// `struct timespec`, in the layout `posix/src/stat.rs` declares.
#[repr(C)]
#[derive(Clone, Copy, Default)]
#[cfg_attr(not(unix), allow(dead_code))]
struct CTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// The size [`CStat`] must have, checked here rather than discovered as a
/// corrupted stack: `fstatat` writes 144 bytes on x86-64 whatever this file
/// thinks, so a declaration that drifted from the C one has to fail the build.
#[cfg(unix)]
const _: () = assert!(core::mem::size_of::<CStat>() == 144);

/// Rebuild a `dev_t` from the two halves ustar stores it in.
///
/// The exact inverse of [`split_dev`], and it has to be exact: a device node
/// archived on one machine and extracted on another must come back with the
/// same number, and Linux's packing puts the low 8 bits of the minor and the
/// low 12 of the major where a naive `(major << 8) | minor` would put something
/// else entirely.
fn make_dev(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | ((major & !0xfff) << 32) | (minor & 0xff) | ((minor & !0xff) << 12)
}

/// The `S_IFMT` bits `mknod` wants for each of the two device flavours.
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;

/// The file-type field of `st_mode`, and the value in it that means "directory".
///
/// Needed because `--keep-newer-files` exempts directories: what is standing in
/// the way has to be classified before its mtime is worth comparing. See
/// [`Located::kind_and_mtime`].
#[cfg(unix)]
const S_IFMT: u32 = 0o170000;
#[cfg(unix)]
const S_IFDIR: u32 = 0o040000;

/// The parts of a member's creation whose *wording* is worth keeping beside
/// the flags that produce it.
#[cfg(unix)]
impl Located {
    /// Open the leaf for a regular member: `O_WRONLY|O_CREAT|O_EXCL|O_NONBLOCK`.
    ///
    /// **Exclusive, not truncating**, which is GNU's `open_output_file` when
    /// `--overwrite` was not given, and the difference is not a nicety. This used
    /// to pass `.create(true).truncate(true)`, i.e. `O_TRUNC`, and so wrote
    /// *through* whatever already stood at the path:
    ///
    /// * over a file with **other hard links**, `O_TRUNC` rewrites the shared
    ///   inode, so extracting `x` silently rewrote every other name for it.
    ///   GNU's `O_EXCL` fails, [`create_at`] unlinks the name, and the second
    ///   open makes a *new* inode -- the link is broken, the other names keep
    ///   their contents. Measured (`tar-rules5.sh`): after extracting over a
    ///   twice-linked file GNU leaves `links=1` and the other name unchanged.
    /// * over a **symlink**, `O_TRUNC` follows it and writes at the far end. An
    ///   archive of `x` unpacked into a directory where `x -> ../outside`
    ///   already exists therefore wrote outside the destination -- a traversal
    ///   that survived every other defence here, because the symlink came from
    ///   the filesystem rather than from the archive. GNU replaces the symlink
    ///   itself.
    ///
    /// `O_NONBLOCK` remains because `open` on a **fifo** blocks until a reader
    /// appears. With `O_EXCL` an existing fifo is now unlinked and replaced
    /// rather than opened, so the flag no longer has a case to answer on the
    /// paths this program takes -- but it costs nothing and the guarantee is
    /// worth stating outright rather than deducing from the flag two lines above.
    ///
    /// 0o666 is the mode `File::create` would have asked for, and the umask
    /// takes it down from there; the archive's own mode is applied afterwards.
    fn create_file(&self) -> io::Result<File> {
        self.open(
            oflag::WRONLY | oflag::CREAT | oflag::EXCL | oflag::NONBLOCK,
            0o666,
        )
    }

    /// [`create_file`](Self::create_file) as `--overwrite` wants it: keep the
    /// inode and truncate it, rather than unlinking the name and making a new
    /// one.
    ///
    /// `O_TRUNC` in place of `O_EXCL` is the whole of the option — it is what
    /// makes an extraction over a hard-linked file change every name for that
    /// inode, which is the behaviour `create_file`'s doc explains the default
    /// avoids. Both are right; they are different requests.
    ///
    /// **`O_NOFOLLOW` is not optional here.** Without `O_EXCL` the open would
    /// otherwise follow a symlink already on disk and truncate whatever it
    /// pointed at, so an archive of `x` unpacked with `--overwrite` into a
    /// directory holding `x -> ../../outside` would write outside the
    /// destination — past every other defence in this program, because the link
    /// came from the filesystem and not from the archive. With the flag the
    /// open fails `ELOOP`, [`create_at`] removes the link and retries, and a
    /// *regular file* appears in its place. That is GNU's answer too: measured
    /// (`tar-ovw2.sh`), `--overwrite` over such a link leaves `x` a plain file
    /// and the outside target byte-for-byte unchanged.
    ///
    /// A **directory** in the way is deliberately not recovered from: `openat`
    /// says `EISDIR`, and GNU reports `Cannot open: Is a directory` and exits 2
    /// rather than removing it. `--overwrite` truncates; it does not delete.
    fn create_file_overwriting(&self) -> io::Result<File> {
        self.open(
            oflag::WRONLY | oflag::CREAT | oflag::TRUNC | oflag::NOFOLLOW | oflag::NONBLOCK,
            0o666,
        )
    }

    /// Clear the leaf out of the way for `-U`, before anything is created.
    ///
    /// `unlink` then `rmdir`, in that order and for the same reason
    /// [`create_at`] uses it: a file is the common case and `unlink` on a
    /// directory is the cheap failure. An empty directory is therefore removed,
    /// a non-empty one is not, and `ENOENT` — nothing there at all — is the
    /// success this is usually asking for.
    ///
    /// The error kept is the **second** call's, which is what puts
    /// `Directory not empty` in GNU's message rather than the `Is a directory`
    /// the `unlink` produced: measured, a `-U` extraction over a non-empty
    /// directory says `tar: dir: Cannot unlink: Directory not empty`.
    fn clear_for_unlink_first(&self) -> io::Result<()> {
        match self.unlink() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => match self.rmdir() {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        }
    }

    /// The leaf's own mtime in whole seconds, and whether it is a directory —
    /// or `None` when nothing is there.
    ///
    /// `AT_SYMLINK_NOFOLLOW`, so a symlink answers for *itself*. That is
    /// measured GNU behaviour and not an obvious choice: under
    /// `--keep-newer-files` a link whose own mtime is old is replaced even when
    /// it points at a brand-new file, and one whose own mtime is new is kept
    /// even when its target is ancient (`tar-knf.sh`). It also means a dangling
    /// link answers rather than erroring, which is why such a link is *kept*
    /// instead of provoking a `Cannot stat` warning.
    ///
    /// Whole seconds is exact rather than a simplification: a member's mtime
    /// comes from a ustar header and so has no fractional part, and the test
    /// this feeds is `on-disk ≥ member`. Since a nanosecond field is never
    /// negative, `sec ≥ member` and `(sec, nsec) ≥ (member, 0)` agree on every
    /// input. Measured at the boundary: on-disk one nanosecond after the
    /// member's second is kept, one second before it is replaced.
    fn kind_and_mtime(&self) -> io::Result<Option<(bool, i64)>> {
        let cname = c_name(&self.leaf)?;
        let mut st = CStat::default();
        // SAFETY: as `identity` — `cname` is NUL-terminated and `st` is the
        // full-size `struct stat` the call fills and does not retain.
        let rc = unsafe { fstatat(self.dir.0, cname.as_ptr(), &raw mut st, AT_SYMLINK_NOFOLLOW) };
        if rc == 0 {
            return Ok(Some((st.st_mode & S_IFMT == S_IFDIR, st.st_mtim.tv_sec)));
        }
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(e)
        }
    }

    /// Stand up the empty file a delayed symlink's place is held with.
    ///
    /// Mode 0, as GNU's `create_placeholder_file` opens it: nothing should be
    /// able to use this file for anything during the seconds it exists.
    fn create_placeholder(&self) -> io::Result<()> {
        self.open(
            oflag::WRONLY | oflag::CREAT | oflag::EXCL | oflag::NONBLOCK,
            0,
        )
        .map(drop)
    }

    /// Create a symlink at the leaf, pointing at `target`.
    ///
    /// The target is stored, and used, **verbatim** — absolute targets and
    /// targets full of `..` alike. That is measured GNU behaviour
    /// (`abs -> /etc/passwd` and `up -> ../../outside` are both created, in
    /// silence, exit 0) and it is safe for the same reason it is safe in GNU: a
    /// symlink is only a name until something follows it, and whether to follow
    /// it is the *caller's* choice later, not this program's now. What must not
    /// happen is that tar itself walks through one while unpacking the rest of
    /// the archive — see [`Dir::locate`], which is what stops it.
    fn make_symlink(&self, target: &[u8]) -> io::Result<()> {
        self.symlink(target)
    }

    /// Create a named pipe at the leaf.
    ///
    /// The mode is applied again by the caller through [`restore_metadata`], and
    /// has to be: `mkfifo` masks what it is given by the umask, so a 0666 fifo
    /// under umask 022 arrives 0644 whatever the archive said, and `-p` would
    /// have no effect at all.
    fn make_fifo(&self, mode: u32) -> io::Result<()> {
        self.mkfifo(mode)
    }

    /// Create a character or block device node at the leaf.
    ///
    /// Fails with `EPERM` for anyone but root, which is not a defect to work
    /// around — it is the kernel refusing to let an archive hand an
    /// unprivileged user a readable `/dev/sda`. GNU reports it and carries on
    /// with the next member, and so does the caller here.
    fn make_device(&self, mode: u32, block: bool, major: u64, minor: u64) -> io::Result<()> {
        let kind = if block { S_IFBLK } else { S_IFCHR };
        self.mknod(kind | (mode & 0o7777), make_dev(major, minor))
    }

    /// Set the leaf's modification time, following a final symlink.
    fn set_mtime(&self, mtime: i64) -> io::Result<()> {
        self.stamp(mtime, 0)
    }

    /// Set a **symlink's own** modification time, without following it.
    ///
    /// [`Located::set_mtime`] would stamp the target instead — or fail outright
    /// on a dangling link, which is a perfectly ordinary thing for an archive to
    /// hold. Measured: GNU restores it, and a symlink archived at 2019-05-06
    /// comes back dated 2019-05-06 even when what it points at does not exist.
    fn set_symlink_mtime(&self, mtime: i64) -> io::Result<()> {
        self.stamp(mtime, AT_SYMLINK_NOFOLLOW)
    }

    /// Set the leaf's permission bits.
    fn set_mode(&self, mode: u32) -> io::Result<()> {
        self.chmod(mode)
    }
}

// ---------------------------------------------------------------------------
// The same thing off unix, where the primitives it is built from do not exist.
// ---------------------------------------------------------------------------

/// A directory, named rather than held open.
///
/// The unix [`Dir`] holds a descriptor per level and creates through it, which
/// is what makes the confinement race-free. There is no `openat` here to build
/// that from, so this twin resolves lexically and creates by path: the member
/// still cannot be *named* outside the destination, but a symlink planted in
/// the destination between the check and the creation would not be caught.
///
/// That is a real gap and it is recorded as one. It is also not the platform
/// this program is for: extraction off unix already cannot make a symlink, a
/// fifo or a device node, and says so.
#[cfg(not(unix))]
struct Dir(std::path::PathBuf);

#[cfg(not(unix))]
impl Dir {
    fn open_root(path: &Path) -> io::Result<Self> {
        // Canonicalised so the stored root is something later joins cannot walk
        // out of by accident, and so a destination that does not exist fails
        // here rather than once per member.
        Ok(Self(fs::canonicalize(path)?))
    }

    fn locate(&self, name: &[u8]) -> io::Result<Located> {
        let mut stack: Vec<Vec<u8>> = Vec::new();
        let mut comps = components(name);
        // The destination itself when the name has no components — see the unix
        // twin for why that is a location and not an error.
        let leaf = comps.pop().unwrap_or_else(|| b".".to_vec());
        if leaf == b".." {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "name escapes the destination",
            ));
        }
        for comp in comps {
            if comp == b".." {
                if stack.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "name escapes the destination",
                    ));
                }
            } else {
                stack.push(comp);
            }
        }
        let mut path = self.0.clone();
        for comp in &stack {
            path.push(os_from_bytes(comp));
        }
        Ok(Located { dir: path, leaf })
    }
}

#[cfg(not(unix))]
struct Located {
    dir: std::path::PathBuf,
    leaf: Vec<u8>,
}

#[cfg(not(unix))]
impl Located {
    fn path(&self) -> std::path::PathBuf {
        self.dir.join(os_from_bytes(&self.leaf))
    }

    fn mkdir(&self) -> io::Result<()> {
        fs::create_dir(self.path())
    }

    fn mkdir_member(&self) -> io::Result<()> {
        match self.mkdir() {
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(self.path()).is_ok_and(|m| m.is_dir()) =>
            {
                Ok(())
            }
            other => other,
        }
    }

    fn unlink(&self) -> io::Result<()> {
        fs::remove_file(self.path())
    }

    fn rmdir(&self) -> io::Result<()> {
        fs::remove_dir(self.path())
    }

    fn hard_link_to(&self, target: &Self) -> io::Result<()> {
        fs::hard_link(target.path(), self.path())
    }

    fn create_file(&self) -> io::Result<File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path())
    }

    fn create_file_overwriting(&self) -> io::Result<File> {
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.path())
    }

    fn clear_for_unlink_first(&self) -> io::Result<()> {
        match self.unlink() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => match self.rmdir() {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        }
    }

    /// The unix twin's doc explains what this is for. Here the metadata comes
    /// from `symlink_metadata`, which is `lstat` under another name, so a
    /// symlink still answers for itself.
    fn kind_and_mtime(&self) -> io::Result<Option<(bool, i64)>> {
        use std::time::UNIX_EPOCH;
        let md = match fs::symlink_metadata(self.path()) {
            Ok(md) => md,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mtime = md.modified()?;
        // `duration_since` reports a time *before* the epoch as an error whose
        // payload is the distance back to it, so the two arms are the two signs
        // of one number rather than a success and a failure.
        let secs = match mtime.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
            Err(e) => i64::try_from(e.duration().as_secs())
                .unwrap_or(i64::MAX)
                .saturating_neg(),
        };
        Ok(Some((md.is_dir(), secs)))
    }

    fn create_placeholder(&self) -> io::Result<()> {
        self.create_file().map(drop)
    }

    /// `(0, 0)` for everything: there is no inode number to compare here, so the
    /// call answers only "does it still exist". The placeholder check it feeds
    /// is therefore weaker off unix, in the same way and for the same reason
    /// resolution is.
    fn identity(&self) -> io::Result<(u64, u64)> {
        fs::symlink_metadata(self.path())?;
        Ok((0, 0))
    }

    fn make_symlink(&self, _target: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks are not supported on this host",
        ))
    }

    fn make_fifo(&self, _mode: u32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "named pipes are not supported on this host",
        ))
    }

    fn make_device(&self, _mode: u32, _block: bool, _major: u64, _minor: u64) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "device nodes are not supported on this host",
        ))
    }

    fn set_mtime(&self, mtime: i64) -> io::Result<()> {
        use std::fs::FileTimes;
        use std::time::{Duration, SystemTime};
        let t = if mtime >= 0 {
            SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(mtime.unsigned_abs()))
        } else {
            SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(mtime.unsigned_abs()))
        };
        // A header can hold a time no clock can represent; refusing it is right,
        // and refusing it *loudly* is what tells the caller the tree is not the
        // archive.
        let Some(t) = t else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "timestamp out of range",
            ));
        };
        let f = File::open(self.path())?;
        f.set_times(FileTimes::new().set_accessed(t).set_modified(t))
    }

    /// A no-op: there are no symlinks here to stamp.
    fn set_symlink_mtime(&self, _mtime: i64) -> io::Result<()> {
        Ok(())
    }

    /// A no-op off unix, where there are no permission bits to set.
    fn set_mode(&self, _mode: u32) -> io::Result<()> {
        Ok(())
    }
}

/// Stand up the empty file that holds a delayed symlink's place, and identify
/// it so [`apply_delayed_links`] can tell later whether it is still ours.
///
/// Created exclusively, so an existing entry is replaced through the same
/// remove-and-retry [`create_at`] does everywhere else -- which is why a symlink
/// over a *non-empty directory* reports `Cannot open: File exists` rather than
/// the `Cannot create symlink to ‘…’` a relative one would.
fn make_placeholder(
    root: &Dir,
    name: &[u8],
    ovw: Overwriting,
    status: &mut i32,
) -> Result<(u64, u64), NotCreated> {
    let (at, ()) = create_at(root, name, ovw, status, Located::create_placeholder)?;
    Ok(at.identity()?)
}

/// `--keep-newer-files`: is what already stands at `name` reason to leave the
/// member unextracted? Says so if it is.
///
/// Three answers rather than two, and each was measured (`tar-knf.sh`,
/// `tar-knf2.sh`, `tar-knf3.sh`):
///
/// * **Nothing there** — extract. An absent ancestor counts as nothing there
///   too, which is why `NotFound` from the resolution is not an error here.
/// * **A directory there** — extract, whatever its timestamp. Directories are
///   exempt from this option entirely; a regular member lands on top of a
///   directory dated a year from now without a word (and then usually fails at
///   the creation, if the directory is not empty). The exemption is of what is
///   *on disk*, not of the member: a directory **member** is declined like any
///   other when a newer plain file holds its name. The exemption is also the
///   reason a directory member over an existing directory is decided entirely
///   by whether that directory can be removed — see the `--keep-newer-files`
///   arm of the directory member's `create` choice in [`do_extract`].
/// * **Anything else** — compare `lstat`'s mtime, and decline if it is at least
///   as new as the member's.
///
/// A resolution or `stat` failure is a *warning*, not an error: GNU prints
/// `Warning: Cannot stat` and then declines the member anyway, leaving the exit
/// status alone. The case that produces it is an archive holding `a/i` where `a`
/// was itself kept and is a plain file, so the warning is the expected
/// consequence of a decision this option already made.
///
/// The comparison is in whole seconds, which is exact rather than approximate:
/// see [`Located::kind_and_mtime`].
fn keeps_newer(root: &Dir, name: &[u8], member_mtime: i64) -> bool {
    let newer = match root.locate(name).and_then(|at| at.kind_and_mtime()) {
        Ok(None) => return false,
        Ok(Some((true, _))) => return false,
        Ok(Some((false, mtime))) => mtime >= member_mtime,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(e) => {
            diag!(
                "tar: {}: Warning: Cannot stat: {}",
                escape(name),
                strerror(&e)
            );
            true
        }
    };
    if newer {
        // gnulib's `quote()` — curly quotes, not tar's escaping style — and the
        // name is *not* prefixed with `tar: name:` the way a failure is. GNU's
        // sentence is `tar: Current ‘a’ is newer or same age`, which reads as a
        // remark about the tree rather than a complaint about the member.
        diag!("tar: Current {} is newer or same age", quote(name));
    }
    newer
}

/// Replace every placeholder with the symlink it stood for.
///
/// A placeholder that is no longer the file this run created is left alone and
/// nothing is said, which is GNU's handling: the archive has already been read,
/// something else now owns that path, and unlinking it on the strength of a
/// name would be the very substitution the placeholder exists to prevent.
///
/// The location is resolved afresh rather than held from the first pass. Holding
/// one would mean an open descriptor per delayed link for the length of the
/// extraction, and an archive is free to contain as many of them as it likes.
fn apply_delayed_links(root: &Dir, links: Vec<DelayedLink>, status: &mut i32) {
    for link in links {
        let Ok(at) = root.locate(&link.name) else {
            // The name resolved when the placeholder was made. That it does not
            // now means something moved underneath us, which is exactly the
            // case the identity check below exists to decline.
            continue;
        };
        if at.identity().ok() != Some(link.id) {
            continue;
        }
        if let Err(e) = at.unlink() {
            diag!(
                "tar: {}: Cannot unlink: {}",
                escape(&link.name),
                strerror(&e)
            );
            *status = EXIT_FATAL;
            continue;
        }
        if let Err(e) = at.make_symlink(&link.target) {
            diag!(
                "tar: {}: Cannot create symlink to {}: {}",
                escape(&link.name),
                quote(&link.target),
                strerror(&e)
            );
            *status = EXIT_FATAL;
            continue;
        }
        if let Err(e) = at.set_symlink_mtime(link.mtime) {
            diag!(
                "tar: {}: Cannot utime: {}",
                escape(&link.name),
                strerror(&e)
            );
            *status = EXIT_FATAL;
        }
    }
}

fn do_extract(
    archive_file: Option<&OsStr>,
    directory: Option<&OsStr>,
    verbose: Verbose,
    members: &[OsString],
    same_permissions: bool,
    old_files: OldFiles,
) -> i32 {
    // The archive is opened before the `-C` chdir, so its own path is resolved
    // against the directory the user was standing in, as GNU does.
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdin()),
    };

    if let Some(dir) = directory
        && let Err(e) = env::set_current_dir(dir)
    {
        diag!("tar: {}: Cannot chdir: {}", escape_os(dir), strerror(&e));
        return fatal();
    }

    // After the chdir, not before: `-C` chooses the destination, and the
    // destination is the boundary every member is resolved beneath. Held open
    // for the whole extraction, so that a rename of the destination underneath
    // us moves the extraction with it rather than spilling into whatever took
    // the old name.
    let root = match Dir::open_root(Path::new(".")) {
        Ok(root) => root,
        Err(e) => {
            diag!("tar: {}: Cannot open: {}", escape(b"."), strerror(&e));
            return fatal();
        }
    };

    let mut status = 0;
    // A hard link's target gets its own notice, separate from the member-name
    // one: an archive can hold `/etc/passwd` as a target without holding it as
    // a name, and GNU prints the two lines independently.
    let mut prefixes = PrefixNotice::new();
    let mut selector = Selector::new(members);
    let umask = read_umask();
    // Directory metadata is applied last, in reverse order. It has to be: a
    // directory's mtime is bumped by every child written into it, and a
    // directory whose stored mode has no write bit cannot receive children at
    // all. GNU defers both for the same reason, which is why `tar -xf` restores
    // a 0500 directory's mode *and* its timestamp, and ours restored neither.
    let mut pending_dirs: Vec<(Vec<u8>, u32, i64)> = Vec::new();
    let mut delayed: Vec<DelayedLink> = Vec::new();
    // The same for every member except the directories, which flip one field.
    let ovw = Overwriting {
        old_files,
        verbose: verbose != Verbose::Off,
        directory_member: false,
    };

    let stop = walk(input.as_mut(), |member, input| {
        let raw_name = member.name.as_slice();

        // Stripping happens as the header is decoded, before the member is
        // selected and before it is announced: measured, `tar -tvf` prints the
        // notice above the line for the member that provoked it, and prints it
        // for a member `-t` is only listing. Both names go through it here so
        // that a listing and an extraction of one archive say the same things.
        let name = prefixes.strip(raw_name, PrefixKind::MemberNames).to_vec();
        let link_target = strip_link_target(member, &mut prefixes, &mut || {});

        // Operands select members. With none, everything is wanted — but a
        // member the caller did not ask for must be skipped *before* anything
        // is written, which is why this test comes first. `tar -xf a.tar one`
        // used to unpack the whole archive. The operand is matched against the
        // name as *stored*, which is what the caller read out of `tar -t`.
        if !selector.wants(raw_name) {
            return Handled::Skip;
        }

        // Nothing below may use `raw_name` as a path. It is attacker-chosen,
        // and stripping a prefix does not make a `..` in the middle safe.
        if contains_dot_dot(raw_name) {
            diag!("tar: {}: Member name contains '..'", escape(raw_name));
            status = EXIT_FATAL;
            return Handled::Skip;
        }

        // The announced name is the member's as *stored*, not the path it lands
        // at: GNU prints `/a` while extracting to `a`, and prints `./` for the
        // `.` member of an archive made with `tar -cf x.tar .`. No slash is
        // appended for a directory either — a type-`5` member stored without
        // one is announced without one (`tar-rules3.sh`).
        if verbose != Verbose::Off {
            verbose.line(raw_name);
        }

        // `--keep-newer-files` is decided here, above the type switch, because
        // it applies to *every* member type and not only to the regular files
        // whose data would be rewritten: measured, a symlink, a fifo, a hard
        // link and a directory member are all declined by a newer file standing
        // in the way (`tar-knf2.sh`). It is also below the `-v` name line,
        // which is where GNU prints its notice — `a` then `tar: Current ‘a’ is
        // newer or same age`, in that order.
        if old_files == OldFiles::KeepNewer && keeps_newer(&root, trim_slashes(&name), member.mtime)
        {
            return Handled::Skip;
        }

        match member.typeflag {
            _ if member.is_dir() => {
                // A directory member is stored as `d/`, but every diagnostic
                // about it names `d` — and the trailing slash would also make
                // the ancestor walk treat the member itself as its own ancestor.
                let name = trim_slashes(&name).to_vec();
                let ovw = Overwriting {
                    directory_member: true,
                    ..ovw
                };
                // Three of the five need a plain `mkdir` rather than
                // [`Located::mkdir_member`], which reports an existing
                // *directory* as success — right for the default and for
                // `--overwrite`, wrong for these, and for two different reasons.
                //
                // Under `-k` and `--skip-old-files` it is wrong because success
                // is what puts the member's mode and mtime in `pending_dirs`:
                // measured, `tar -xkf` over a directory it has already unpacked
                // leaves that directory's 0700 mode and old timestamp exactly as
                // found, where the default restores the member's (`tar-ovw6.sh`).
                //
                // Under `--keep-newer-files` it is wrong because GNU does not
                // take the shortcut at all — it *removes* the existing directory
                // and makes the member's, so an empty one is replaced (mode and
                // mtime become the member's) and a non-empty one is a hard error.
                // The mtime plays no part: an on-disk directory is exempt from
                // the age test (see [`keeps_newer`]), so what decides is only
                // whether the removal succeeds (`tar-knf3.sh`).
                let create: fn(&Located) -> io::Result<()> = match old_files {
                    OldFiles::Keep | OldFiles::Skip | OldFiles::KeepNewer => Located::mkdir,
                    _ => Located::mkdir_member,
                };
                match create_at(&root, &name, ovw, &mut status, create) {
                    Err(NotCreated::Failed(e)) => {
                        diag!("tar: {}: Cannot mkdir: {}", escape(&name), strerror(&e));
                        status = EXIT_FATAL;
                    }
                    // The one member type with a sentence of its own for an
                    // obstacle it could not clear, and it names neither the call
                    // nor the errno. Reached only under `--keep-newer-files`,
                    // over a directory whose removal failed; the members *inside*
                    // it are still extracted, into the directory that stayed.
                    Err(NotCreated::Immovable(_)) => {
                        diag!(
                            "tar: {}: Unexpected inconsistency when making directory",
                            escape(&name)
                        );
                        status = EXIT_FATAL;
                    }
                    Err(NotCreated::Silent) => {}
                    Ok(_) => {
                        pending_dirs.push((
                            name,
                            extraction_mode(member.mode, same_permissions, umask),
                            member.mtime,
                        ));
                    }
                }
                Handled::Skip
            }
            b'0' | b'\0' | b'7' => {
                let mode = extraction_mode(member.mode, same_permissions, umask);
                extract_plain(&root, input, &name, member, mode, ovw, &mut status)
            }
            b'2' if is_delayed_target(&member.linkname) => {
                // A symlink out of the destination — absolute, or climbing —
                // is not created now. It is stood up as an empty placeholder
                // *file* and turned into the symlink after the last member,
                // which is GNU's design and is one leg of tar's defence against
                // being walked through its own output: an archive holding
                // `d -> /tmp` followed by `d/pwned` writes nothing to `/tmp`,
                // because at the moment `d/pwned` is opened, `d` is a regular
                // file and the open fails with `Not a directory`. Measured,
                // `tar-rules2.sh`.
                //
                // It is only *one* leg, and only covers the run that creates
                // the link: the placeholder is replaced before this function
                // returns, so a second archive would meet a real symlink. What
                // stops that one is [`Dir::locate`], which refuses to walk
                // through it however it got there.
                match make_placeholder(&root, &name, ovw, &mut status) {
                    Ok(id) => delayed.push(DelayedLink {
                        name: name.clone(),
                        target: member.linkname.clone(),
                        mtime: member.mtime,
                        id,
                    }),
                    Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) => {
                        // `Cannot open`, not `Cannot create symlink`: at this
                        // point GNU really is opening a file, and the wording
                        // is how the two paths are told apart in a log.
                        diag!("tar: {}: Cannot open: {}", escape(&name), strerror(&e));
                        status = EXIT_FATAL;
                    }
                    // Nothing stood up, so nothing is queued: the link is not
                    // made at the end of the run either, which is the point of
                    // having declined to disturb what is there.
                    Err(NotCreated::Silent) => {}
                }
                Handled::Skip
            }
            b'2' => {
                let create = |at: &Located| at.make_symlink(&member.linkname);
                match create_at(&root, &name, ovw, &mut status, create) {
                    Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) => {
                        diag!(
                            "tar: {}: Cannot create symlink to {}: {}",
                            escape(&name),
                            quote(&member.linkname),
                            strerror(&e)
                        );
                        status = EXIT_FATAL;
                    }
                    Err(NotCreated::Silent) => {}
                    // No mode is applied. A symlink has none of its own on any
                    // system this runs on — `lrwxrwxrwx` is a constant — and
                    // `chmod` through one would silently repermission whatever
                    // it points at, which for an archived `-> /etc/passwd` is
                    // the whole attack.
                    Ok((at, ())) => {
                        if let Err(e) = at.set_symlink_mtime(member.mtime) {
                            diag!("tar: {}: Cannot utime: {}", escape(&name), strerror(&e));
                            status = EXIT_FATAL;
                        }
                    }
                }
                Handled::Skip
            }
            b'1' => {
                // The *target* is resolved beneath the destination too, not
                // just the link's own name. Measured (`tar-rules17.sh`): with
                // `x -> ../out` already in the destination, GNU refuses a
                // member linking to `x/secret` with `Cannot hard link to
                // ‘x/secret’: Invalid cross-device link` — a hard link to a
                // file outside the tree would otherwise hand its contents, and
                // write access to them, to whoever unpacked the archive.
                let create = |at: &Located| {
                    let target = root.locate(&link_target)?;
                    at.hard_link_to(&target)
                };
                if let Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) =
                    create_at(&root, &name, ovw, &mut status, create)
                {
                    // The one place in this program that quotes with `‘…’`
                    // instead of tar's escape style, because it is the one
                    // place GNU does: this sentence is built with gnulib's
                    // `quote()`, which is pinned to the locale style and so
                    // ignores the `escape_quoting_style` set at startup.
                    diag!(
                        "tar: {}: Cannot hard link to {}: {}",
                        escape(&name),
                        quote(&link_target),
                        strerror(&e)
                    );
                    status = EXIT_FATAL;
                }
                // No mode and no mtime: the link *is* the target's inode, and
                // stamping it would restamp the file it was linked to.
                Handled::Skip
            }
            b'6' => {
                let mode = extraction_mode(member.mode, same_permissions, umask);
                match create_at(&root, &name, ovw, &mut status, |at| at.make_fifo(mode)) {
                    Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) => {
                        diag!("tar: {}: Cannot mkfifo: {}", escape(&name), strerror(&e));
                        status = EXIT_FATAL;
                    }
                    Err(NotCreated::Silent) => {}
                    Ok((at, ())) => {
                        restore_metadata(&at, &name, mode, member.mtime, &mut status);
                    }
                }
                Handled::Skip
            }
            b'3' | b'4' => {
                let mode = extraction_mode(member.mode, same_permissions, umask);
                let block = member.typeflag == b'4';
                let create =
                    |at: &Located| at.make_device(mode, block, member.devmajor, member.devminor);
                match create_at(&root, &name, ovw, &mut status, create) {
                    Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) => {
                        // `Operation not permitted` for everyone but root, and
                        // that is the point rather than a shortcoming: an
                        // archive that could conjure a block device would be
                        // handing whoever unpacked it raw access to a disk.
                        diag!("tar: {}: Cannot mknod: {}", escape(&name), strerror(&e));
                        status = EXIT_FATAL;
                    }
                    Err(NotCreated::Silent) => {}
                    Ok((at, ())) => {
                        restore_metadata(&at, &name, mode, member.mtime, &mut status);
                    }
                }
                Handled::Skip
            }
            other => {
                // A type flag with no defined meaning. GNU keeps the data and
                // exits **0** — `Unknown file type 'Z', extracted as normal
                // file` — which is the only answer that cannot lose anything:
                // the bytes are there, and refusing them would discard the sole
                // copy over a byte nobody has defined.
                //
                // The type flag is one byte of the archive's own header, as
                // attacker-chosen as the name beside it. GNU writes it into the
                // message raw, so a member flagged with a newline forges a line
                // of tar's stderr; `escape` renders that byte as `\n` inside
                // the same `'…'` GNU uses, so the common case is identical and
                // the hostile one is inert.
                diag!(
                    "tar: {}: Unknown file type '{}', extracted as normal file",
                    escape(&name),
                    escape(&[other])
                );
                let mode = extraction_mode(member.mode, same_permissions, umask);
                extract_plain(&root, input, &name, member, mode, ovw, &mut status)
            }
        }
    });

    // Links before directories, because creating one bumps the mtime of the
    // directory it lands in. GNU reaches the same place by a longer route — it
    // fixes every directory that is *not* an ancestor of a delayed link, then
    // applies the links, then fixes the ancestors — and the observable result
    // is identical: measured, an archive whose only member under `sub/` is a
    // symlink to an absolute path leaves `sub`'s stored mtime intact.
    apply_delayed_links(&root, delayed, &mut status);

    // Deepest first: `pending_dirs` is in archive order, which is parents
    // before children, so the reverse leaves a parent's timestamp untouched by
    // work still to be done inside it.
    for (name, mode, mtime) in pending_dirs.into_iter().rev() {
        match root.locate(&name) {
            Ok(at) => restore_metadata(&at, &name, mode, mtime, &mut status),
            Err(e) => {
                // The directory was made a moment ago and resolved then. That
                // it does not resolve now means the tree changed underneath the
                // extraction, and the stamp is declined rather than applied to
                // whatever took the name.
                diag!("tar: {}: Cannot utime: {}", escape(&name), strerror(&e));
                status = EXIT_FATAL;
            }
        }
    }

    let missing = selector.report_missing();
    let walk_status = report_stop(stop, &archive_label(archive_file));
    if walk_status != 0 {
        return walk_status;
    }
    if status == 0 && missing == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

/// Extract a member as an ordinary file, returning the driver's verdict on its
/// data blocks.
///
/// Shared by the regular-file arm and the unknown-type arm, which are the same
/// operation: GNU's answer to a type flag it does not recognise is to keep the
/// bytes and say so.
fn extract_plain(
    root: &Dir,
    input: &mut dyn Read,
    name: &[u8],
    member: &Member,
    mode: u32,
    ovw: Overwriting,
    status: &mut i32,
) -> Handled {
    if extract_regular_file(root, input, name, member, mode, ovw, status) {
        Handled::Consumed
    } else {
        Handled::Truncated
    }
}

/// The target a hard link is made to, after the same prefix stripping a member
/// name gets — and the linkname untouched for every other type.
///
/// A hard link's target is a *name in the archive*, so it is exactly as
/// attacker-chosen as the member's own name; GNU strips it and announces the
/// removal under a heading of its own (``Removing leading `/' from hard link
/// targets``). It is *not* refused for containing `..`, which member names are:
/// GNU cuts up to the last `..` and links what is left, and since the cut can
/// only move the target further inside the destination there is nothing to
/// refuse. Measured, `tar-rules2.sh`: a target of `a/../base` links to `base`.
///
/// A **symlink**'s target is not a member name at all — it is the link's data,
/// resolved later by whoever follows it — so it is stored and restored
/// verbatim, absolute or not. That is GNU's behaviour and the reason
/// [`is_delayed_target`] exists to make it safe.
fn strip_link_target(
    member: &Member,
    prefixes: &mut PrefixNotice,
    flush: &mut dyn FnMut(),
) -> Vec<u8> {
    if member.typeflag == b'1' {
        prefixes
            .strip_flushing(&member.linkname, PrefixKind::LinkTargets, flush)
            .to_vec()
    } else {
        member.linkname.clone()
    }
}

/// Is this symlink target one that must not be created until the end?
///
/// Absolute, or containing a `..` component. Either could be followed by a
/// later member of the same archive to somewhere outside the destination, so
/// GNU holds these back; anything else points within the tree being unpacked
/// and is created as it is met.
fn is_delayed_target(target: &[u8]) -> bool {
    target.first() == Some(&b'/') || contains_dot_dot(target)
}

/// A symlink whose creation was held back, and the placeholder standing in for
/// it until the archive has been read.
struct DelayedLink {
    name: Vec<u8>,
    target: Vec<u8>,
    mtime: i64,
    /// `(dev, ino)` of the placeholder as created. Checked again before the
    /// placeholder is removed, so that a link is never made to a path that has
    /// become something else in the meantime.
    id: (u64, u64),
}

/// The archive's name for a diagnostic: its path, or `-` for standard input.
fn archive_label(archive_file: Option<&OsStr>) -> Vec<u8> {
    archive_file.map_or_else(|| b"-".to_vec(), |p| os_bytes(p).into_owned())
}

/// Open the file a regular member is to be written to, reporting why not.
///
/// The ordering here is a security property, not a style choice. This used to
/// `create_dir_all(parent)` up front and *then* open -- which quietly undid
/// tar's traversal defence. An archive holding `d -> /tmp` followed by
/// `d/pwned` gets the symlink withheld (see [`is_delayed_target`]), so `d` is a
/// zero-length placeholder file when `d/pwned` arrives; `create_dir_all` saw
/// `d` was not a directory and reported `d: Cannot mkdir: File exists`, and on
/// a run where the placeholder had *not* been left behind it would have made
/// the directory and written the member. GNU resolves first and only reaches
/// for `mkdir` on the `ENOENT` that says an ancestor is genuinely absent, which
/// makes the same archive fail the way it should: `tar: d/pwned: Cannot open:
/// Not a directory`. Measured, `tar-xmine.sh` cases `traverse`/`traverse2`.
///
/// The recovery still has to exist, because an archive need not store a
/// directory before the members inside it -- `tar -cf - a/b/c` with no `a/` or
/// `a/b/` member is legal and GNU extracts it. It lives in [`create_at`],
/// shared with every other member type; this function is just the wording of
/// the failure.
///
/// The location comes back with the handle so the mode and timestamp can be
/// applied through the *same* resolved parent the file was created in, rather
/// than by walking the name a second time and hoping it still leads to the file
/// just written.
fn open_for_member(
    root: &Dir,
    name: &[u8],
    ovw: Overwriting,
    status: &mut i32,
) -> Option<(Located, File)> {
    // `--overwrite` is the one setting that changes the *open* rather than what
    // is done about its failure: `O_TRUNC` where the others use `O_EXCL`, so the
    // inode survives and every other name for it changes with the contents. See
    // [`Located::create_file_overwriting`].
    let create: fn(&Located) -> io::Result<File> = if ovw.old_files == OldFiles::Overwrite {
        Located::create_file_overwriting
    } else {
        Located::create_file
    };
    match create_at(root, name, ovw, status, create) {
        Ok(pair) => Some(pair),
        Err(NotCreated::Failed(e) | NotCreated::Immovable(e)) => {
            diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
            *status = EXIT_FATAL;
            None
        }
        Err(NotCreated::Silent) => None,
    }
}

/// Stream one regular member out of `input` into `name`. Returns false when
/// the archive ended mid-member, which means the outer loop must stop.
///
/// This streams rather than buffering. The previous version did
/// `Vec::with_capacity(size)` from the header's own size field, so an archive
/// whose header claimed 2^40 bytes made this program try to reserve a
/// terabyte before reading a single block — a one-line denial of service
/// costing the attacker 512 bytes of file.
///
/// Takes the whole `member` rather than its size and mtime separately — the
/// `mode` stays a parameter because it is not the member's stored one but the
/// result of [`extraction_mode`], which the caller has already worked out.
fn extract_regular_file(
    root: &Dir,
    input: &mut dyn Read,
    name: &[u8],
    member: &Member,
    mode: u32,
    ovw: Overwriting,
    status: &mut i32,
) -> bool {
    let size = member.size;
    // Still consume the data whatever happens below: the archive may hold
    // members after this one, and abandoning the stream would lose them too.
    // That is as true of a member `--skip-old-files` stepped over as of one that
    // could not be opened — the bytes are in the stream either way, and the
    // headers after them only line up if they are read.
    let mut opened = open_for_member(root, name, ovw, status);

    let mut remaining = size;
    let mut block = [0u8; BLOCK_SIZE];
    for _ in 0..data_blocks(size) {
        if input.read_exact(&mut block).is_err() {
            diag!("tar: Unexpected EOF in archive");
            *status = EXIT_FATAL;
            return false;
        }
        let take = usize::try_from(remaining)
            .unwrap_or(BLOCK_SIZE)
            .min(BLOCK_SIZE);
        remaining = remaining.saturating_sub(take as u64);
        if let Some((_, f)) = opened.as_mut()
            && let Err(e) = f.write_all(block.get(..take).unwrap_or(&[]))
        {
            diag!("tar: {}: Cannot write: {}", escape(name), strerror(&e));
            *status = EXIT_FATAL;
            // Drop the handle so the rest of the member is only skipped, but
            // keep reading so the following headers stay aligned. Dropping the
            // location with it is what stops the metadata below being stamped
            // onto a file whose contents are wrong.
            opened = None;
        }
    }
    // Buffered data is not the issue here (`File` is unbuffered), but a
    // filesystem that reports a write error at close would otherwise be
    // ignored, which is the same defect as the discarded `write_all` above.
    //
    // Only a file this call actually created is stamped. Applying the mode and
    // time of a member that could not be opened would be writing the archive's
    // metadata onto whatever was already at that path.
    if let Some((at, mut f)) = opened {
        if let Err(e) = f.flush() {
            diag!("tar: {}: Cannot write: {}", escape(name), strerror(&e));
            *status = EXIT_FATAL;
        } else {
            restore_metadata(&at, name, mode, member.mtime, status);
        }
    }
    true
}

fn do_list_main(archive_file: Option<&OsStr>, verbose: bool, members: &[OsString]) -> i32 {
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdin()),
    };

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut selector = Selector::new(members);
    // Read once, before any member is printed. `-tv` renders every timestamp in
    // the machine's zone, and re-resolving `TZ` per member would let a listing
    // straddle a zone change mid-file.
    let zone = localtime::Zone::from_env();

    let (stop, write_err) = list_archive(input.as_mut(), &mut out, verbose, &mut selector, &zone);

    let flush_err = out.flush().err();
    if let Some(e) = write_err.or(flush_err) {
        // `tar -tf big.tar | head -5` closes the pipe on purpose; that is how
        // a pipeline ends, not a failure of this program.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return 0;
        }
        diag!("tar: 'standard output': Cannot write: {}", strerror(&e));
        return fatal();
    }

    // Order matters: a member the caller named and the archive does not hold is
    // worth saying even when the archive also ended badly, because the two are
    // different complaints about different things.
    let missing = selector.report_missing();
    let walk_status = report_stop(stop, &archive_label(archive_file));
    if walk_status != 0 {
        return walk_status;
    }
    if missing == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

/// The width GNU's `-tv` reserves for `user/group` plus the size, before any
/// member has been seen.
///
/// It is a running maximum that starts at 18 and only ever grows, which is why
/// a listing's columns line up within one archive and need not line up between
/// two. Reproduced rather than replaced by a two-pass measurement because the
/// output is an interface — `tar -tv | awk '{print $3}'` is a real idiom, and a
/// listing whose columns differ from GNU's breaks it for no gain.
const UGSWIDTH_MIN: usize = 18;

/// List an archive's members to `out`.
///
/// Returns the reason the walk stopped and the first write error, if any; the
/// caller turns those into a status. Splitting it that way is what lets the
/// unit tests drive a synthetic archive through the real code path and inspect
/// both the bytes written and *why* the read ended — the old version returned
/// `io::Result<()>` and answered `Ok(())` for a truncated archive, a corrupt
/// one, and a file that was never an archive alike.
fn list_archive(
    input: &mut dyn Read,
    out: &mut dyn Write,
    verbose: bool,
    selector: &mut Selector,
    zone: &localtime::Zone,
) -> (Stop, Option<io::Error>) {
    let mut ugswidth = UGSWIDTH_MIN;
    let mut write_err: Option<io::Error> = None;
    // Listing announces the prefixes it *would* strip, exactly as extraction
    // does: measured, `tar -tf` on an archive holding `/a` prints ``Removing
    // leading `/' from member names`` above the `/a` line, and exits 0. The
    // notice belongs to reading a header, not to writing a file.
    let mut prefixes = PrefixNotice::new();

    let stop = walk(input, |member, _data| {
        // The notices go to stderr while the listing goes through a buffer, so
        // without a flush every notice would surface after the whole listing.
        // GNU has the same split and solves it the same way: gnulib's `error()`
        // does `fflush(stdout)` before it writes. The hook is only invoked when
        // a diagnostic is actually about to print, so the common member costs
        // no extra syscall.
        let _stored_under =
            prefixes.strip_flushing(&member.name, PrefixKind::MemberNames, &mut || {
                drop(out.flush());
            });
        let link_target = strip_link_target(member, &mut prefixes, &mut || {
            drop(out.flush());
        });
        if !selector.wants(&member.name) {
            return Handled::Skip;
        }
        // Listing shows the name as stored, not the sanitized one: the point
        // of `tar -t` is to tell you what is in the archive, and a member
        // called `../../etc/passwd` is exactly what you want to be shown.
        //
        // Shown through `escape`, which is what GNU does and is why a name that
        // is not UTF-8 comes out as `caf\351.txt` rather than as the bytes
        // themselves. The earlier reasoning here — that `tar -t` output must be
        // feedable back to `tar -x`, so the bytes must survive — was wrong on
        // its own terms: GNU's output is not feedable back either, and a name
        // containing a newline would put two lines in the manifest.
        let line = if verbose {
            long_line(member, &link_target, &mut ugswidth, zone)
        } else {
            let mut l = escape(&member.name).into_bytes();
            l.push(b'\n');
            l
        };
        if let Err(e) = out.write_all(&line) {
            write_err = Some(e);
            // Zero, not a failure status: the reason is carried in `write_err`
            // and a closed pipe is not an error at all.
            return Handled::Stop(0);
        }
        Handled::Skip
    });

    (stop, write_err)
}

/// One line of `tar -tv`, byte for byte as GNU lays it out.
///
/// The column arithmetic is GNU's and was measured rather than guessed:
/// `pad` counts the user, the group, the size and the one `/` between the
/// first two, `ugswidth` is the running maximum of every `pad` seen so far (and
/// never less than [`UGSWIDTH_MIN`]), and the gap before the size is
/// `ugswidth - pad + 1` spaces. Confirmed against `tar -tvf` for numeric
/// owners (`1000/1000`, 9 spaces), for names (`inhahe/inhahe`, 5), for a 20 MiB
/// member (2), and for a 46-column `user/group` where the gap collapses to the
/// single space the formula's `+ 1` guarantees.
fn long_line(
    member: &Member,
    link_target: &[u8],
    ugswidth: &mut usize,
    zone: &localtime::Zone,
) -> Vec<u8> {
    // ustar stores the owner's *name* beside the number; `--numeric-owner`
    // leaves it empty and GNU then prints the number. Falling back the other
    // way — looking the uid up in this machine's passwd file — would be wrong:
    // the archive may come from a machine where uid 1000 is someone else.
    let user = if member.uname.is_empty() {
        member.uid.to_string().into_bytes()
    } else {
        member.uname.clone()
    };
    let group = if member.gname.is_empty() {
        member.gid.to_string().into_bytes()
    } else {
        member.gname.clone()
    };
    // A device has no size to print, so GNU reuses the column for the pair the
    // header does carry — `1,5` for `/dev/zero`, `7,0` for `loop0`. Measured;
    // ours printed `0` there. Note there is no space after the comma: the
    // column is one field, and the `%d,%d` keeps `-tv | awk` counting the same
    // number of words for a device as for a file.
    let size = match member.typeflag {
        b'3' | b'4' => format!("{},{}", member.devmajor, member.devminor).into_bytes(),
        // A directory and a link occupy no data blocks, and GNU prints 0 for
        // them whatever the header's size field happens to say.
        _ => {
            let size = if member.has_data() { member.size } else { 0 };
            size.to_string().into_bytes()
        }
    };

    let pad = user
        .len()
        .saturating_add(group.len())
        .saturating_add(size.len())
        .saturating_add(1);
    *ugswidth = (*ugswidth).max(pad);
    let gap = ugswidth.saturating_sub(pad).saturating_add(1);

    let mut line = mode_string(member.mode, member.effective_typeflag());
    line.push(b' ');
    line.extend_from_slice(&user);
    line.push(b'/');
    line.extend_from_slice(&group);
    line.resize(line.len().saturating_add(gap), b' ');
    line.extend_from_slice(&size);
    line.push(b' ');

    let tm = zone.local(member.mtime, 0);
    line.extend_from_slice(&localtime::strftime(b"%Y-%m-%d %H:%M", &tm));
    line.push(b' ');
    // The name and the link target are escaped; the user and group names above
    // are not. That asymmetry is GNU's — `print_header` passes the name and the
    // linkname through `quotearg` and prints the owner fields with a plain
    // `%s` — and it is the reason the column arithmetic can use the owner
    // lengths as they stand: nothing before the name column can change width.
    line.extend_from_slice(escape(&member.name).as_bytes());

    // GNU's two suffixes, and part of the reason `-tv` is worth having over
    // `-t`: a symlink's target and a hard link's other name are stored in the
    // header and are invisible in a plain listing.
    match member.typeflag {
        b'2' => {
            line.extend_from_slice(b" -> ");
            line.extend_from_slice(escape(&member.linkname).as_bytes());
        }
        b'1' => {
            // The *stripped* target, which is the name the link would be made
            // to. Measured: an archive whose target is `../t` lists as
            // `link to t`, under the notice about the `../` it removed. The
            // symlink arm above is the stored target instead, because that one
            // is data rather than a member name.
            line.extend_from_slice(b" link to ");
            line.extend_from_slice(escape(link_target).as_bytes());
        }
        // A third suffix, for the same reason as the other two: the `?` in the
        // mode column says the type is not one of the nine, and this says which
        // one it was. GNU's wording and GNU's `quote()` — curly marks, not the
        // escape style the name beside it uses. Measured:
        // `?rw-r--r-- 0/0  5 2020-01-01 22:04 weird unknown file type ‘Z’`.
        other if !known_typeflag(member.effective_typeflag()) => {
            line.extend_from_slice(b" unknown file type ");
            line.extend_from_slice(quote(&[other]).as_bytes());
        }
        _ => {}
    }
    line.push(b'\n');
    line
}

/// The ten-character `drwxr-xr-x` rendering of a type and a mode.
///
/// Shared between the `-tv` listing and the `Cannot change mode to ...`
/// diagnostic, which is GNU's arrangement too — the two must agree, since a
/// user comparing them is comparing the mode that was asked for against the
/// mode that is there.
fn mode_string(mode: u32, typeflag: u8) -> Vec<u8> {
    let kind = match typeflag {
        b'1' => b'h',
        b'2' => b'l',
        b'3' => b'c',
        b'4' => b'b',
        b'5' => b'd',
        b'6' => b'p',
        b'7' => b'C',
        b'0' | b'\0' => b'-',
        // A type this tar does not know is not a regular file, and saying so is
        // more use than pretending.
        _ => b'?',
    };
    let mut out = vec![kind];
    // (read, write, execute, the bit that overrides the execute character,
    //  its letter when execute is set, its letter when execute is clear)
    let triads: [(u32, u32, u32, u32, u8, u8); 3] = [
        (0o400, 0o200, 0o100, 0o4000, b's', b'S'),
        (0o040, 0o020, 0o010, 0o2000, b's', b'S'),
        (0o004, 0o002, 0o001, 0o1000, b't', b'T'),
    ];
    for (r, w, x, extra, set, unset) in triads {
        out.push(if mode & r != 0 { b'r' } else { b'-' });
        out.push(if mode & w != 0 { b'w' } else { b'-' });
        out.push(match (mode & x != 0, mode & extra != 0) {
            (true, false) => b'x',
            (true, true) => set,
            (false, true) => unset,
            (false, false) => b'-',
        });
    }
    out
}

/// Take the used part of a fixed-size, NUL-padded header field.
///
/// Borrows rather than decoding. This was `extract_string`, which ran the
/// bytes through `String::from_utf8_lossy` — and since it is what read the
/// 100-byte `name` field, every member name in the archive passed through it
/// before anything else saw it. A member legitimately named with a byte that
/// is not UTF-8 (legal on this OS: any byte but `/` and NUL) was therefore
/// *listed* under a different name by `-t` and *extracted* under a different
/// name by `-x`, with each offending byte replaced by U+FFFD — silent
/// renaming, not a display quirk, and irreversible. See `known-issues.md` →
/// `B-tar-READ-EVERY-PATH-AS-UTF-8`.
fn field_bytes(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.get(..end).unwrap_or(&[])
}

/// Parse a NUL/space-padded octal field into a `u64`.  Non-octal input
/// silently parses as 0 (matching common tar implementations on
/// malformed archives).
///
/// A field that is not ASCII is not octal either, so the `from_utf8` failure
/// path lands on the same 0 as `"garbage"` does; the lossy decode this used to
/// go through could only ever have turned one non-number into another.
fn parse_octal(buf: &[u8]) -> u64 {
    let trimmed = field_bytes(buf).trim_ascii();
    str::from_utf8(trimmed)
        .ok()
        .and_then(|s| u64::from_str_radix(s, 8).ok())
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use localtime::Zone;

    fn s(items: &[&str]) -> Vec<OsString> {
        items.iter().map(|x| OsString::from(*x)).collect()
    }

    /// Build an argv out of raw byte strings, so a test can pass an argument
    /// that no `&str` can hold.
    fn b(items: &[&[u8]]) -> Vec<OsString> {
        items.iter().map(|x| os_from_bytes(x)).collect()
    }

    /// [`parse_args`] for the tests that are about *parsing*, not about which
    /// [`Request`] came back.
    ///
    /// It panics rather than returning on `Help`/`Usage`/`Version`, so a change
    /// that made, say, `-p` collide with `--help` would fail loudly here
    /// instead of quietly comparing unequal against a default `TarArgs`.
    fn run_args(args: &[OsString]) -> Result<TarArgs, getopt::Error> {
        match parse_args(args) {
            Ok(Request::Run(parsed)) => Ok(parsed),
            Ok(other) => panic!("expected an archive run, got {other:?}"),
            Err(e) => Err(e),
        }
    }

    /// Build a single tar header block with the given name and size.
    fn make_header(name: &[u8], size: u64, typeflag: u8) -> [u8; BLOCK_SIZE] {
        let mut h = TarHeader::new();
        h.set_name(name).unwrap();
        TarHeader::set_octal(&mut h.mode, 0o644);
        TarHeader::set_octal(&mut h.uid, 0);
        TarHeader::set_octal(&mut h.gid, 0);
        TarHeader::set_octal(&mut h.size, size);
        TarHeader::set_octal(&mut h.mtime, 0);
        h.typeflag = typeflag;
        h.magic = *b"ustar\0";
        h.version = *b"00";
        h.compute_checksum();
        *h.as_bytes()
    }

    /// Drive the real listing path over a synthetic archive, in the plain
    /// (`-t`) form with no member operands and a fixed zone, and hand back the
    /// reason it stopped. The zone is UTC rather than the machine's so that a
    /// timestamp assertion means the same thing on every machine that runs the
    /// suite.
    fn list_names(input: &[u8], out: &mut Vec<u8>) -> Stop {
        let mut sel = Selector::new(&[]);
        let (stop, err) = list_archive(&mut &input[..], out, false, &mut sel, &Zone::utc());
        assert!(err.is_none(), "unexpected write error listing to a Vec");
        stop
    }

    /// As [`list_names`], in the long (`-tv`) form.
    fn list_long(input: &[u8], out: &mut Vec<u8>) -> Stop {
        let mut sel = Selector::new(&[]);
        let (stop, err) = list_archive(&mut &input[..], out, true, &mut sel, &Zone::utc());
        assert!(err.is_none(), "unexpected write error listing to a Vec");
        stop
    }

    /// A header with every field a `-tv` line reads set explicitly.
    #[allow(clippy::too_many_arguments)]
    fn make_full_header(
        name: &[u8],
        mode: u32,
        uid: u32,
        gid: u32,
        size: u64,
        mtime: u64,
        typeflag: u8,
        linkname: &[u8],
        uname: &[u8],
        gname: &[u8],
    ) -> [u8; BLOCK_SIZE] {
        let mut h = TarHeader::new();
        h.set_name(name).unwrap();
        TarHeader::set_octal(&mut h.mode, u64::from(mode));
        TarHeader::set_octal(&mut h.uid, u64::from(uid));
        TarHeader::set_octal(&mut h.gid, u64::from(gid));
        TarHeader::set_octal(&mut h.size, size);
        TarHeader::set_octal(&mut h.mtime, mtime);
        h.typeflag = typeflag;
        h.linkname[..linkname.len()].copy_from_slice(linkname);
        h.uname[..uname.len()].copy_from_slice(uname);
        h.gname[..gname.len()].copy_from_slice(gname);
        h.magic = *b"ustar\0";
        h.version = *b"00";
        h.compute_checksum();
        *h.as_bytes()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty() {
        let a = run_args(&s(&[])).unwrap();
        assert_eq!(a, TarArgs::default());
    }

    #[test]
    fn parse_create_with_file() {
        let a = run_args(&s(&["-c", "-f", "out.tar", "a", "b"])).unwrap();
        assert!(a.create);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.files, s(&["a", "b"]));
    }

    #[test]
    fn parse_clustered_create_verbose_file() {
        // -cvf out.tar a -- the f consumes the next argv element.
        let a = run_args(&s(&["-cvf", "out.tar", "a"])).unwrap();
        assert!(a.create);
        assert!(a.verbose);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.files, s(&["a"]));
    }

    #[test]
    fn parse_extract_with_directory() {
        let a = run_args(&s(&["-x", "-C", "/tmp", "-f", "in.tar"])).unwrap();
        assert!(a.extract);
        assert_eq!(a.directory.as_deref(), Some(OsStr::new("/tmp")));
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("in.tar")));
    }

    #[test]
    fn parse_list() {
        let a = run_args(&s(&["-tf", "x.tar"])).unwrap();
        assert!(a.list);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("x.tar")));
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = run_args(&s(&["-Z"])).unwrap_err();
        // Byte for byte what GNU tar 1.35 says for `tar -Q`, less the
        // `tar: ` prefix the caller adds.
        assert_eq!(err.sentence, "invalid option -- 'Z'");
        // 64, not 2: a command line that could not be parsed. Measured on every
        // one of tar's six getopt errors.
        assert_eq!(err.status, EXIT_USAGE);
    }

    #[test]
    fn parse_missing_f_value_errors() {
        // argp's wording, byte for byte: `tar -cf` says exactly this and exits
        // 64. See `scripts/tar-diff.sh`, case "-f with no argument".
        let err = run_args(&s(&["-f"])).unwrap_err();
        assert_eq!(err.sentence, "option requires an argument -- 'f'");
    }

    #[test]
    fn parse_missing_c_value_errors() {
        let err = run_args(&s(&["-C"])).unwrap_err();
        assert_eq!(err.sentence, "option requires an argument -- 'C'");
    }

    #[test]
    fn parse_files_with_dashes_handled() {
        // Bare positional arg starting with non-dash is a file.
        let a = run_args(&s(&["-c", "f1", "f2"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, s(&["f1", "f2"]));
    }

    // The point of the byte conversion: every one of these arguments is a
    // legal filename on this OS (`design.txt`: any byte but `/` and NUL), and
    // every one of them made `tar` abort at startup, before touching an
    // archive, when argv was read as `String`. See `known-issues.md` ->
    // `B-tar-READ-EVERY-PATH-AS-UTF-8`.

    #[test]
    fn parse_keeps_an_operand_that_is_not_utf8() {
        let a = run_args(&b(&[b"-c", b"caf\xe9", b"ok"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, b(&[b"caf\xe9", b"ok"]));
        // Not merely "did not crash": the bytes are the ones passed in, so the
        // file that gets archived is the file that was named.
        assert_eq!(os_bytes(&a.files[0]).as_ref(), b"caf\xe9");
    }

    #[test]
    fn parse_keeps_a_dash_f_value_that_is_not_utf8() {
        let a = run_args(&b(&[b"-cf", b"\xff\xfe.tar", b"x"])).unwrap();
        let f = a.archive_file.unwrap();
        assert_eq!(os_bytes(&f).as_ref(), b"\xff\xfe.tar");
    }

    #[test]
    fn parse_keeps_a_dash_c_value_that_is_not_utf8() {
        let a = run_args(&b(&[b"-x", b"-C", b"/tmp/d\x80r"])).unwrap();
        assert!(a.extract);
        let d = a.directory.unwrap();
        assert_eq!(os_bytes(&d).as_ref(), b"/tmp/d\x80r");
    }

    #[test]
    fn parse_refuses_a_cluster_byte_that_is_not_an_option_without_panicking() {
        // A cluster is walked byte by byte, so a `-` followed by something
        // that is not UTF-8 at all is refused like any other unknown flag
        // rather than being a case the parser cannot represent.
        let err = run_args(&b(&[b"-\xe9"])).unwrap_err();
        assert!(err.sentence.contains("invalid option"), "{err}");
        // The byte is escaped rather than emitted raw, so the message cannot
        // forge a line of tar's stderr.
        assert!(!err.sentence.as_bytes().contains(&0xe9), "{err}");
    }

    // ---------------- long options ----------------
    //
    // Every expectation below was measured against GNU tar 1.35, and the table
    // that drives them was checked against it exhaustively: all 1381 distinct
    // prefixes of every name, verdict and candidate list, zero mismatches.
    // See `LONG_OPTIONS`.

    #[test]
    fn parse_long_forms_of_every_short_option() {
        let a = run_args(&s(&[
            "--create",
            "--verbose",
            "--preserve-permissions",
            "--file=out.tar",
            "--directory=/tmp",
            "x",
        ]))
        .unwrap();
        assert!(a.create && a.verbose && a.same_permissions);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.directory.as_deref(), Some(OsStr::new("/tmp")));
        assert_eq!(a.files, s(&["x"]));
    }

    #[test]
    fn parse_long_value_may_be_the_next_word_or_attached() {
        // `--file=A` and `--file A` are the same thing; the hand-written parser
        // this replaced understood neither.
        for argv in [
            s(&["--extract", "--file=in.tar"]),
            s(&["--extract", "--file", "in.tar"]),
        ] {
            let a = run_args(&argv).unwrap();
            assert!(a.extract);
            assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("in.tar")));
        }
    }

    #[test]
    fn parse_accepts_gnus_alias_spellings() {
        // Two names, one option, in both of GNU's pairs.
        assert!(run_args(&s(&["--get"])).unwrap().extract);
        assert!(run_args(&s(&["--extract"])).unwrap().extract);
        assert!(
            run_args(&s(&["--same-permissions"]))
                .unwrap()
                .same_permissions
        );
        assert!(
            run_args(&s(&["--preserve-permissions"]))
                .unwrap()
                .same_permissions
        );
    }

    #[test]
    fn parse_takes_each_member_of_the_overwrite_family() {
        for (argv, want) in [
            (s(&["-x"]), OldFiles::Replace),
            (s(&["-x", "--overwrite"]), OldFiles::Overwrite),
            (s(&["-x", "-k"]), OldFiles::Keep),
            (s(&["-x", "--keep-old-files"]), OldFiles::Keep),
            (s(&["-x", "--skip-old-files"]), OldFiles::Skip),
            (s(&["-x", "-U"]), OldFiles::UnlinkFirst),
            (s(&["-x", "--unlink-first"]), OldFiles::UnlinkFirst),
            (s(&["-x", "--keep-newer-files"]), OldFiles::KeepNewer),
            // Clustered, to prove the two new letters are in `SHORT_OPTIONS`
            // rather than only in the long table.
            (s(&["-xk"]), OldFiles::Keep),
            (s(&["-xU"]), OldFiles::UnlinkFirst),
        ] {
            assert_eq!(run_args(&argv).unwrap().old_files, want, "{argv:?}");
        }
    }

    #[test]
    fn naming_one_of_the_overwrite_family_twice_is_allowed() {
        // GNU tests whether the *value* would change, not whether an option was
        // seen twice, so a repetition is not a conflict. Measured: `tar -k -k`
        // and `tar --overwrite --overwrite` both extract (`tar-ovw-diff.sh`).
        assert_eq!(
            run_args(&s(&["-x", "-k", "-k"])).unwrap().old_files,
            OldFiles::Keep
        );
        assert_eq!(
            run_args(&s(&["-x", "--overwrite", "--overwrite"]))
                .unwrap()
                .old_files,
            OldFiles::Overwrite
        );
        // Two spellings of one option are the same value, so likewise fine.
        assert_eq!(
            run_args(&s(&["-x", "-k", "--keep-old-files"]))
                .unwrap()
                .old_files,
            OldFiles::Keep
        );
    }

    #[test]
    fn naming_two_of_the_overwrite_family_is_a_usage_error() {
        // The *second* option is named first, and both are named by their long
        // spelling however they were written -- `tar -U -k` complains about
        // `'--keep-old-files' … '--unlink-first'`, naming neither letter.
        let e = run_args(&s(&["-x", "-U", "-k"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "'--keep-old-files' cannot be used with '--unlink-first'"
        );
        // Exit 2, not the 64 an unrecognised option gets: this sentence comes
        // from tar's own `USAGE_ERROR`, not from argp's option table. Measured
        // both ways -- `tar --frobnicate` is 64 and `tar -k --overwrite` is 2.
        assert_eq!(e.status, EXIT_FATAL);
        assert_eq!(
            run_args(&s(&["-x", "--keep-newer-files", "--overwrite"]))
                .unwrap_err()
                .sentence,
            "'--overwrite' cannot be used with '--keep-newer-files'"
        );
    }

    #[test]
    fn parse_accepts_an_unambiguous_abbreviation() {
        // `--extr` is a prefix of `--extract` alone. The resolved name is what
        // reaches the match arm, so no arm needs to know about abbreviations.
        let a = run_args(&s(&["--extr", "--verbo"])).unwrap();
        assert!(a.extract && a.verbose);
    }

    #[test]
    fn parse_refuses_an_abbreviation_ambiguous_only_because_of_an_option_we_lack() {
        // `--verb` is one letter short of unambiguous, and the option that
        // makes it so — `--verbatim-files-from` — is one this tar does not
        // implement. So this case is decided *entirely* by an entry that exists
        // for no other reason, which is the clearest statement of why the table
        // carries all 172 names. Measured: GNU refuses `--verb` identically,
        // and `--verbo` lists an archive.
        let err = run_args(&s(&["--verb"])).unwrap_err();
        assert_eq!(
            err.sentence,
            "option '--verb' is ambiguous; possibilities: '--verbose' '--verbatim-files-from'"
        );
        assert!(run_args(&s(&["--verbo"])).unwrap().verbose);
    }

    #[test]
    fn parse_refuses_an_ambiguous_abbreviation_listing_gnus_candidates() {
        // The reason the table carries all 172 names. With only the twelve this
        // tar implements, `--ex` would be a *unique* prefix of `--extract` and
        // would silently extract.
        let err = run_args(&s(&["--ex"])).unwrap_err();
        assert_eq!(
            err.sentence,
            "option '--ex' is ambiguous; possibilities: '--extract' '--exclude' \
             '--exclude-from' '--exclude-caches' '--exclude-caches-under' \
             '--exclude-caches-all' '--exclude-tag' '--exclude-ignore' \
             '--exclude-ignore-recursive' '--exclude-tag-under' '--exclude-tag-all' \
             '--exclude-vcs' '--exclude-vcs-ignores' '--exclude-backups'"
        );
    }

    #[test]
    fn parse_lists_ambiguity_candidates_in_gnus_order_not_alphabetically() {
        // The single most easily-broken property of the table: `--extract`
        // precedes `--exclude`, which sorting would reverse. Asserted on its
        // own so that an alphabetised table fails with an obvious message
        // rather than inside the long string above.
        let err = run_args(&s(&["--ex"])).unwrap_err();
        let extract = err.sentence.find("'--extract'").unwrap();
        let exclude = err.sentence.find("'--exclude'").unwrap();
        assert!(extract < exclude, "{}", err.sentence);
    }

    #[test]
    fn parse_refuses_an_unknown_long_option() {
        let err = run_args(&s(&["--frobnicate"])).unwrap_err();
        // The whole word as typed, `--` included — there is no resolved name to
        // report instead.
        assert_eq!(err.sentence, "unrecognized option '--frobnicate'");
    }

    #[test]
    fn parse_refuses_a_value_given_to_a_long_option_that_takes_none() {
        let err = run_args(&s(&["--extract=yes"])).unwrap_err();
        // Named as the table spells it, not as typed. Measured: GNU answers an
        // abbreviated `--extr=yes` with `'--extract'` too.
        assert_eq!(err.sentence, "option '--extract' doesn't allow an argument");
        assert_eq!(
            run_args(&s(&["--extr=yes"])).unwrap_err().sentence,
            "option '--extract' doesn't allow an argument"
        );
    }

    #[test]
    fn parse_refuses_a_long_option_whose_required_value_is_missing() {
        let err = run_args(&s(&["--file"])).unwrap_err();
        // Note the word order differs from the short form's `option requires an
        // argument -- 'f'`. That is glibc's, not a slip.
        assert_eq!(err.sentence, "option '--file' requires an argument");
    }

    #[test]
    fn parse_refuses_a_recognised_but_unimplemented_option() {
        let err = run_args(&s(&["--exclude=*.o"])).unwrap_err();
        assert_eq!(
            err.sentence,
            "option '--exclude' is recognised but not implemented by this tar"
        );
        assert_eq!(err.status, EXIT_USAGE);
    }

    #[test]
    fn parse_does_not_leave_an_unimplemented_options_value_as_an_operand() {
        // The trap this avoids: `--exclude` takes a value, so if the parser did
        // not know that, `*.o` would be left behind and archived as a file. The
        // refusal is what the user sees, but the operand list is what would
        // have been silently wrong had the table's argument class been missing.
        let err = run_args(&s(&["-c", "--exclude", "*.o", "src"])).unwrap_err();
        assert!(err.sentence.contains("--exclude"), "{err}");
    }

    #[test]
    fn parse_treats_double_dash_as_end_of_options() {
        // Previously a bug, not a choice: `--` fell through to the operand
        // branch and was looked for inside the archive, so this exited 2 with
        // `tar: --: Not found in archive` where GNU exits 0.
        let a = run_args(&s(&["-xf", "t.tar", "--", "a"])).unwrap();
        assert!(a.extract);
        assert_eq!(a.files, s(&["a"]));
    }

    #[test]
    fn parse_after_double_dash_an_option_shaped_name_is_a_file() {
        // The point of the terminator: a member really called `--exclude` is
        // archivable, and a member called `-c` does not turn on create.
        let a = run_args(&s(&["-c", "--", "--exclude", "-c"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, s(&["--exclude", "-c"]));
    }

    #[test]
    fn parse_accepts_a_short_value_attached_to_its_letter() {
        // `-fout.tar` used to be read as more option letters and failed on the
        // `o`: `invalid option -- 'o'`.
        let a = run_args(&s(&["-cvfout.tar", "x"])).unwrap();
        assert!(a.create && a.verbose);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.files, s(&["x"]));
    }

    #[test]
    fn parse_permutes_options_after_operands() {
        // Measured: `tar -tf t.tar a --verbose` gives a long listing, so an
        // option after an operand is still an option.
        let a = run_args(&s(&["-tf", "t.tar", "a", "--verbose"])).unwrap();
        assert!(a.list && a.verbose);
        assert_eq!(a.files, s(&["a"]));
    }

    #[test]
    fn parse_keeps_a_long_option_value_that_is_not_utf8() {
        // The `=VALUE` half of a long option is a path like any other, and must
        // survive bytes that are not text.
        let a = run_args(&b(&[b"-x", b"--directory=/tmp/d\x80r"])).unwrap();
        let d = a.directory.unwrap();
        assert_eq!(os_bytes(&d).as_ref(), b"/tmp/d\x80r");
    }

    #[test]
    fn parse_refuses_a_long_name_that_is_not_utf8_without_panicking() {
        // No option name is non-ASCII, so such a name matches nothing; it must
        // reach the unrecognised path rather than failing some third way.
        let err = run_args(&b(&[b"--caf\xe9"])).unwrap_err();
        assert!(err.sentence.contains("unrecognized option"), "{err}");
        assert!(!err.sentence.as_bytes().contains(&0xe9), "{err}");
    }

    #[test]
    fn long_option_table_is_in_gnus_measured_order_and_has_no_duplicates() {
        // Guards the table itself rather than the parser. A duplicate name
        // would make one entry unreachable, and the count pins the set: 170
        // from `--help` plus the two it hides, `--program-name` and `--HANG`.
        // `scripts/getopt-ambiguity-check.py` re-derives the whole thing from
        // `tar --=x` at push time; this is the cheap local guard.
        assert_eq!(LONG_OPTIONS.len(), 172);
        // Fully qualified: the file's `BTreeSet` import is `#[cfg(unix)]`, and
        // this test is not.
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in LONG_OPTIONS {
            assert!(seen.insert(*name), "duplicate long option: --{name}");
        }
        // The two `--help` does not mention. `--HANG` is also the only name
        // with a capital in it, and so the one no lower-case prefix sweep can
        // ever reach — which is how the first version of this table lost it.
        assert!(seen.contains("program-name"));
        assert!(seen.contains("HANG"));
        // Not sorted — and that is the invariant, not an accident. See the
        // ambiguity-order test above.
        assert!(
            LONG_OPTIONS.windows(2).any(|w| w[0].0 > w[1].0),
            "table looks alphabetised; GNU's order is observable output"
        );
    }

    #[test]
    fn the_empty_long_name_lists_the_whole_table_in_order() {
        // `tar --=x`: the empty name is a prefix of every entry, so the
        // ambiguity list *is* the table. This is the only case in which the
        // full cross-letter order is observable — every other ambiguous prefix
        // has candidates that all share its first letter — which is why the
        // array is GNU's declaration order and not a per-letter reconstruction
        // of it. Measured byte for byte against GNU: 2806 identical bytes,
        // both exiting 64. It is also the measurement
        // `scripts/getopt-ambiguity-check.py` reads GNU's table with.
        let err = run_args(&s(&["--=x"])).unwrap_err();
        let expected: String = LONG_OPTIONS
            .iter()
            .map(|(name, _)| format!(" '--{name}'"))
            .collect();
        // The word as typed, `=x` and all — glibc names the argv word in an
        // ambiguity, and only resolves to a table name once one entry has won.
        assert_eq!(
            err.sentence,
            format!("option '--=x' is ambiguous; possibilities:{expected}")
        );
        assert_eq!(err.status, EXIT_USAGE);
    }

    // ---------------- --help, --usage, --version ----------------

    #[test]
    fn parse_recognises_the_three_informational_options() {
        assert_eq!(parse_args(&s(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&s(&["-?"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&s(&["--usage"])).unwrap(), Request::Usage);
        assert_eq!(parse_args(&s(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn parse_abbreviates_help_and_version_but_not_usage() {
        // `--us` is ambiguous, and not for a reason anyone would guess:
        // `--use-compress-program` shares the prefix. Measured against GNU,
        // which gives these two candidates in this order. `--hel` and `--vers`
        // are each unique and resolve.
        assert_eq!(parse_args(&s(&["--hel"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&s(&["--vers"])).unwrap(), Request::Version);
        assert_eq!(
            parse_args(&s(&["--us"])).unwrap_err().sentence,
            "option '--us' is ambiguous; possibilities: '--use-compress-program' '--usage'"
        );
        assert_eq!(parse_args(&s(&["--usa"])).unwrap(), Request::Usage);
    }

    #[test]
    fn help_wins_over_a_bad_option_after_it_and_loses_to_one_before_it() {
        // The whole of the precedence rule, and the reason `parse_args` returns
        // a `Request` from inside the loop rather than validating argv first.
        // Both rows measured against GNU: exit 0 with help, then exit 64 with
        // `unrecognized option`.
        assert_eq!(
            parse_args(&s(&["--help", "--frobnicate"])).unwrap(),
            Request::Help
        );
        assert_eq!(
            parse_args(&s(&["--frobnicate", "--help"]))
                .unwrap_err()
                .sentence,
            "unrecognized option '--frobnicate'"
        );
    }

    #[test]
    fn help_wins_over_an_operation_that_precedes_it() {
        // `tar -c --help` prints help; it does not create an archive. The `-c`
        // is parsed and then discarded with the rest of `TarArgs`, which is why
        // `Request` is an enum rather than a flag on `TarArgs`.
        assert_eq!(parse_args(&s(&["-c", "--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&s(&["-c?"])).unwrap(), Request::Help);
    }

    #[test]
    fn help_given_as_a_value_is_a_value_and_not_a_request() {
        // `--file --help` names an archive called `--help`. GNU agrees, and
        // then fails at exit 2 with `You must specify one of…` because no
        // operation was given -- which is `main`'s branch, not the parser's.
        let a = run_args(&s(&["--file", "--help"])).unwrap();
        assert_eq!(a.archive_file, Some(OsString::from("--help")));
    }

    #[test]
    fn parse_refuses_a_value_given_to_help() {
        assert_eq!(
            parse_args(&s(&["--help=x"])).unwrap_err().sentence,
            "option '--help' doesn't allow an argument"
        );
    }

    #[test]
    fn help_text_documents_every_option_this_tar_implements_and_no_others() {
        let help = help_text();
        for name in [
            "--create",
            "--list",
            "--extract",
            "--get",
            "--directory",
            "--file",
            "--preserve-permissions",
            "--same-permissions",
            "--verbose",
            "--keep-newer-files",
            "--keep-old-files",
            "--overwrite",
            "--skip-old-files",
            "--unlink-first",
            "--help",
            "--usage",
            "--version",
        ] {
            assert!(help.contains(name), "help omits {name}");
        }
        // The point of writing our own help rather than reproducing GNU's: an
        // option the parser refuses must not be advertised. `--exclude` stands
        // in for the other 155. See `design-decisions.md` 703.
        //
        // `--overwrite-dir` is here rather than among the advertised names on
        // purpose, and it is the reason this list is checked as a *substring*
        // search rather than a set comparison: it is a real GNU option this tar
        // does not have, and `--overwrite` — which it does — is a prefix of it,
        // so a help text that named the pair the other way round would pass a
        // naive `contains` and lie. The assertion below therefore only holds
        // because `--overwrite` is written with two trailing spaces before its
        // description and never as `--overwrite-dir`.
        for name in [
            "--exclude",
            "--overwrite-dir",
            "--gzip",
            "--strip-components",
        ] {
            assert!(
                !help.contains(name),
                "help advertises {name}, which we refuse"
            );
        }
    }

    /// The synopsis and the help must describe the *same* tar.
    ///
    /// They are two hand-written strings that have to be edited together, and
    /// the failure mode when they are not is silent: an option that works, is
    /// documented in one place and missing from the other. Every long name in
    /// the synopsis must appear in the help, and every long name the help
    /// advertises must appear in the synopsis.
    #[test]
    fn usage_and_help_advertise_the_same_options() {
        let (usage, help) = (usage_text(), help_text());
        // `[--directory=DIR]` in the synopsis is `--directory=DIR` in the help
        // too, so the argument spelling needs no special handling; only the
        // brackets have to come off.
        for word in usage.split_whitespace() {
            let Some(name) = word.strip_prefix("[--").map(|w| w.trim_end_matches(']')) else {
                continue;
            };
            assert!(
                help.contains(&format!("--{name}")),
                "the synopsis offers --{name} and the help does not mention it"
            );
        }
        for line in help.lines() {
            for word in line.split([' ', ',']).filter(|w| w.starts_with("--")) {
                assert!(
                    usage.contains(&format!("[{word}]")),
                    "the help documents {word} and the synopsis omits it"
                );
            }
        }
    }

    #[test]
    fn help_and_usage_are_wrapped_and_end_in_a_newline() {
        for text in [help_text(), usage_text()] {
            assert!(text.ends_with('\n'), "{text:?}");
            assert!(!text.ends_with("\n\n"), "{text:?}");
            for line in text.lines() {
                assert!(line.len() <= 79, "line too long ({}): {line}", line.len());
            }
        }
        assert!(help_text().starts_with("Usage: tar [OPTION...] [FILE]...\n"));
        assert!(usage_text().starts_with("Usage: tar [-ctxkUpv?]"));
    }

    // ---------------- contains_dot_dot ----------------
    //
    // Every case here was a file written outside the destination directory
    // before this check existed. See known-issues.md ->
    // B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY. What has changed since
    // is which half does the work: `strip_leading` makes the ordinary hostile
    // name safe and announces it, and this refuses what stripping cannot make
    // safe -- a `..` with anything after it.

    #[test]
    fn dot_dot_is_recognised_only_as_a_whole_component() {
        assert!(contains_dot_dot(b".."));
        assert!(contains_dot_dot(b"../../etc/passwd"));
        assert!(contains_dot_dot(b"a/../b"));
        assert!(contains_dot_dot(b"a/../../etc/passwd"));
        assert!(contains_dot_dot(b"/../../root/.ssh/authorized_keys"));
        assert!(contains_dot_dot(b"a/.."));
        // The test is on the bytes, so a neighbouring component that is not
        // text does not weaken it. When this took `&str` the caller had already
        // replaced `\xe9` with U+FFFD, and the guarantee was being made about a
        // string that was no longer the name being written.
        assert!(contains_dot_dot(b"\xe9/../../etc/passwd"));
    }

    #[test]
    fn dotfiles_and_names_starting_with_dots_are_ordinary() {
        assert!(!contains_dot_dot(b".bashrc"));
        assert!(!contains_dot_dot(b"a/..foo"));
        assert!(!contains_dot_dot(b"a/..."));
        assert!(!contains_dot_dot(b"./a//b/./c"));
        assert!(!contains_dot_dot(b"a/b/c.txt"));
        assert!(!contains_dot_dot(b"caf\xe9/x"));
        assert!(!contains_dot_dot(b""));
    }

    #[test]
    fn backslash_is_not_a_separator() {
        // It is one on the host these tests run on, and this check did once
        // treat it as one, as defence in depth. That made a member legitimately
        // named `a\..` -- a name design.txt allows, since paths admit every
        // byte but `/` and NUL -- unextractable on the OS this tar is *for*.
        // The path is built by joining on `/` alone, so a backslash never
        // traverses.
        assert!(!contains_dot_dot(b"..\\..\\windows\\system32\\x"));
        assert!(!contains_dot_dot(b"a/..\\b"));
        assert!(!contains_dot_dot(b"a/b\\c"));
    }

    // ---------------- PrefixNotice ----------------

    #[test]
    fn prefix_notice_strips_and_substitutes_a_dot() {
        let mut p = PrefixNotice::new();
        // Critical: `Path::join` with an absolute path throws the base away, so
        // an unstripped name would ignore `-C` entirely.
        assert_eq!(
            p.strip(b"/etc/passwd", PrefixKind::MemberNames),
            b"etc/passwd"
        );
        assert_eq!(
            p.strip(b"///etc/passwd", PrefixKind::MemberNames),
            b"etc/passwd"
        );
        // Non-UTF-8 bytes survive: the name is what gets created on disk, so
        // any alteration here is a silent rename.
        assert_eq!(p.strip(b"/\xff\xfe", PrefixKind::MemberNames), b"\xff\xfe");
        // A name entirely consumed becomes `.` -- how `tar -c ..` stores the
        // directory itself.
        assert_eq!(p.strip(b"..", PrefixKind::MemberNames), b".");
        assert_eq!(p.strip(b"d/..", PrefixKind::MemberNames), b".");
        // As does a name that arrives empty, which is announced separately.
        assert_eq!(p.strip(b"", PrefixKind::MemberNames), b".");
        assert_eq!(p.strip(b"", PrefixKind::LinkTargets), b".");
    }

    #[test]
    fn prefix_notice_keeps_member_names_and_link_targets_apart() {
        // Two independent sets, and each announces a given prefix once. There
        // is no observable return-value difference -- the point is the stderr
        // -- so what this pins is that the bookkeeping does not make the
        // *second* kind's strip a no-op.
        let mut p = PrefixNotice::new();
        assert_eq!(p.strip(b"/x", PrefixKind::MemberNames), b"x");
        assert_eq!(p.strip(b"/x", PrefixKind::MemberNames), b"x");
        assert_eq!(p.strip(b"/x", PrefixKind::LinkTargets), b"x");
        assert_eq!(p.names.len(), 1);
        assert_eq!(p.targets.len(), 1);
        // Distinct prefixes accumulate; a repeat of a *shorter* one does not
        // re-announce. Measured, `tar-rules4.sh`: the targets
        // `/ ../ / // ../ / a/../` announce exactly `/`, `../`, `//`, `a/../`.
        for t in [b"../x".as_slice(), b"//x", b"/x", b"a/../x", b"../x"] {
            p.strip(t, PrefixKind::LinkTargets);
        }
        assert_eq!(p.targets.len(), 4);
    }

    #[test]
    fn prefix_notice_leaves_a_leading_dot_alone() {
        // `.` names the directory being archived and takes the extractor
        // nowhere it was not already, which is why `tar -cf - .` round-trips
        // through `./f` unaltered.
        let mut p = PrefixNotice::new();
        assert_eq!(
            p.strip(b"./a//b/./c", PrefixKind::MemberNames),
            b"./a//b/./c"
        );
        assert_eq!(p.strip(b"a/b/c.txt", PrefixKind::MemberNames), b"a/b/c.txt");
        assert!(p.names.is_empty());
    }

    // ---------------- data_blocks ----------------

    #[test]
    fn data_blocks_rounds_up() {
        assert_eq!(data_blocks(0), 0);
        assert_eq!(data_blocks(1), 1);
        assert_eq!(data_blocks(512), 1);
        assert_eq!(data_blocks(513), 2);
        assert_eq!(data_blocks(1024), 2);
    }

    #[test]
    fn data_blocks_does_not_overflow() {
        // A hostile header can name any u64; this must not wrap to 0 blocks
        // and desynchronise the reader.
        assert!(data_blocks(u64::MAX) > 0);
    }

    // ---------------- field_bytes / parse_octal ----------------

    #[test]
    fn field_bytes_stops_at_nul() {
        let buf = b"hello\0\0\0world";
        assert_eq!(field_bytes(buf), b"hello");
    }

    #[test]
    fn field_bytes_no_nul_uses_all() {
        let buf = b"hello";
        assert_eq!(field_bytes(buf), b"hello");
    }

    #[test]
    fn field_bytes_empty() {
        assert_eq!(field_bytes(&[]), b"");
        assert_eq!(field_bytes(&[0u8; 8]), b"");
    }

    #[test]
    fn field_bytes_does_not_decode() {
        // The whole reason this is not `extract_string`: the 100-byte name
        // field holds a filename, and a filename here is bytes. Decoding it
        // lossily renamed the member.
        let mut buf = [0u8; 100];
        buf[..5].copy_from_slice(b"a\xff\xfeb\xc3");
        assert_eq!(field_bytes(&buf), b"a\xff\xfeb\xc3");
    }

    #[test]
    fn parse_octal_basic() {
        let mut buf = [0u8; 12];
        buf[..4].copy_from_slice(b"0755");
        assert_eq!(parse_octal(&buf), 0o755);
    }

    #[test]
    fn parse_octal_space_padded() {
        let mut buf = [0u8; 12];
        buf[..6].copy_from_slice(b"  0755");
        assert_eq!(parse_octal(&buf), 0o755);
    }

    #[test]
    fn parse_octal_garbage_is_zero() {
        let buf = *b"garbage\0\0\0\0\0";
        assert_eq!(parse_octal(&buf), 0);
    }

    #[test]
    fn parse_octal_empty_is_zero() {
        assert_eq!(parse_octal(&[]), 0);
    }

    #[test]
    fn parse_octal_non_ascii_is_zero() {
        // A hostile header can put any byte in the size field. Non-UTF-8 is
        // not a number, so it takes the same path as `garbage` -- and must
        // take it without panicking, since the result feeds `data_blocks`.
        let mut buf = [0u8; 12];
        buf[..3].copy_from_slice(b"\xff\xfe7");
        assert_eq!(parse_octal(&buf), 0);
    }

    #[test]
    fn parse_octal_rejects_digits_outside_the_base() {
        // `8` and `9` are not octal; the whole field is refused rather than
        // truncated at the bad digit, which would read a wrong size.
        let mut buf = [0u8; 12];
        buf[..3].copy_from_slice(b"789");
        assert_eq!(parse_octal(&buf), 0);
    }

    // ---------------- TarHeader::set_octal ----------------

    #[test]
    fn set_octal_basic() {
        let mut f = [0u8; 8];
        TarHeader::set_octal(&mut f, 0o755);
        assert_eq!(parse_octal(&f), 0o755);
        // Trailing byte should remain NUL.
        assert_eq!(f.get(7), Some(&0));
    }

    #[test]
    fn set_octal_zero() {
        let mut f = [0u8; 8];
        TarHeader::set_octal(&mut f, 0);
        assert_eq!(parse_octal(&f), 0);
    }

    #[test]
    fn set_octal_large_value_round_trips() {
        let mut f = [0u8; 12];
        TarHeader::set_octal(&mut f, 1_234_567);
        assert_eq!(parse_octal(&f), 1_234_567);
    }

    #[test]
    fn set_octal_empty_field_noop() {
        let mut f: [u8; 0] = [];
        TarHeader::set_octal(&mut f, 0o755); // must not panic
    }

    // ---------------- TarHeader::compute_checksum ----------------

    #[test]
    fn checksum_is_stable() {
        let mut h1 = TarHeader::new();
        h1.set_name(b"foo").unwrap();
        TarHeader::set_octal(&mut h1.mode, 0o644);
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name(b"foo").unwrap();
        TarHeader::set_octal(&mut h2.mode, 0o644);
        h2.compute_checksum();

        assert_eq!(h1.checksum, h2.checksum);
    }

    #[test]
    fn checksum_changes_with_name() {
        let mut h1 = TarHeader::new();
        h1.set_name(b"foo").unwrap();
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name(b"bar").unwrap();
        h2.compute_checksum();

        assert_ne!(h1.checksum, h2.checksum);
    }

    // ---------------- list_archive ----------------

    #[test]
    fn list_empty_archive_writes_nothing() {
        let mut input: Vec<u8> = Vec::new();
        // Two zero blocks = empty archive.
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty());
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_single_zero_byte_file() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"hello.txt", 0, b'0'));
        // No data blocks (size = 0).
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "hello.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_single_file_with_data() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"data.bin", 100, b'0'));
        // 100-byte file occupies 1 data block.
        input.extend_from_slice(&[b'x'; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "data.bin\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_multiple_files() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&make_header(b"b.txt", 600, b'0'));
        // 600-byte file = ceil(600/512) = 2 data blocks.
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&make_header(b"c.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        let listing = String::from_utf8(out).unwrap();
        assert_eq!(listing, "a.txt\nb.txt\nc.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_escapes_a_byte_that_is_not_a_character() {
        // Through `String::from_utf8_lossy` and `writeln!` this printed
        // `caf<U+FFFD>.txt` -- three bytes where one belongs, and the same
        // three for every distinct bad byte, so two different members could
        // list under one name. GNU's answer is the octal escape, which is
        // unambiguous and stays on one line. Measured.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"caf\xe9.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "caf\\351.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_leaves_a_valid_multibyte_name_alone() {
        // The other half of the rule: `escape` escapes what is not a character,
        // not what is not ASCII. GNU under a UTF-8 locale prints `café.txt`
        // whole and only falls back to octal for the bytes that decode to
        // nothing.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("café.txt".as_bytes(), 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "café.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    /// `t/` + `n`×`d` + `/` + `m`×`f`, the shape every split test uses.
    fn deep(dirlen: usize, filelen: usize) -> Vec<u8> {
        let mut v = b"t/".to_vec();
        v.extend(std::iter::repeat_n(b'd', dirlen));
        v.push(b'/');
        v.extend(std::iter::repeat_n(b'f', filelen));
        v
    }

    #[test]
    fn a_name_that_exactly_fills_the_field_keeps_its_last_byte() {
        // The bug this replaced: `name[..99]`, so a 100-byte name lost a byte.
        // The field has no terminator when it is full, which is legal ustar and
        // is what GNU writes.
        let full = deep(96, 1);
        assert_eq!(full.len(), 100);
        assert_eq!(split_ustar_name(&full), Ok((&b""[..], &full[..])));
        let mut h = TarHeader::new();
        h.set_name(&full).unwrap();
        assert_eq!(&h.name[..], &full[..]);
        assert_eq!(h.prefix[0], 0);
    }

    #[test]
    fn one_byte_past_the_field_moves_the_directory_into_the_prefix() {
        // Measured against GNU: `t/` + 96×`d` + `/fff` -> prefix 98, name 3.
        let full = deep(96, 3);
        assert_eq!(full.len(), 102);
        let (prefix, name) = split_ustar_name(&full).unwrap();
        assert_eq!(prefix.len(), 98);
        assert_eq!(name, b"fff");
        // The `/` at the seam is not stored in either field.
        assert_eq!(prefix.last(), Some(&b'd'));
    }

    #[test]
    fn the_remainder_may_be_a_hundred_bytes_but_not_a_hundred_and_one() {
        // GNU accepts the first and refuses the second. The boundary matters
        // because it is the one place a name that *fits in 256 bytes* is still
        // rejected, and getting it wrong either drops a file GNU keeps or
        // writes a header GNU would not.
        let ok = deep(3, 100);
        assert_eq!(split_ustar_name(&ok).unwrap().1.len(), 100);
        let refused = deep(3, 101);
        assert_eq!(split_ustar_name(&refused), Err(NameTooLong::CannotSplit));
    }

    #[test]
    fn a_single_component_too_long_cannot_be_split() {
        let mut full = b"t/".to_vec();
        full.extend(std::iter::repeat_n(b'f', 200));
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::CannotSplit));
    }

    #[test]
    fn past_two_hundred_and_fifty_six_bytes_is_the_other_refusal() {
        // Two different GNU messages, so two different errors: "max 256" when
        // no split could ever work, "cannot be split" when the pieces do not
        // land where ustar needs them.
        let full = deep(200, 100);
        assert_eq!(full.len(), 303);
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::Max));
    }

    #[test]
    fn a_directory_name_that_only_a_zero_length_name_would_fit_is_refused() {
        // 153 bytes: `t/` + 150×`d` + `/`. A 152-byte prefix and an empty name
        // would hold it, and GNU still refuses -- the backward search starts at
        // the trailing slash, skips it, and then finds only the `/` at offset 1,
        // leaving 151 bytes for a 100-byte field. Measured; this is the case
        // that proves the search is capped-and-backward rather than "last `/`".
        let mut full = b"t/".to_vec();
        full.extend(std::iter::repeat_n(b'd', 150));
        full.push(b'/');
        assert_eq!(full.len(), 153);
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::CannotSplit));
    }

    #[test]
    #[cfg(unix)]
    fn a_leading_slash_run_is_removed_whole_not_one_slash_at_a_time() {
        // The message quotes the prefix it removed, so `//a` must report `//`.
        assert_eq!(strip_leading(b"/a/b"), (&b"/"[..], &b"a/b"[..]));
        assert_eq!(strip_leading(b"//a/b"), (&b"//"[..], &b"a/b"[..]));
        assert_eq!(strip_leading(b"///a"), (&b"///"[..], &b"a"[..]));
    }

    #[test]
    #[cfg(unix)]
    fn a_dotdot_component_is_removed_but_a_leading_dot_is_not() {
        // Measured: GNU stores `./t` unchanged and says nothing, but turns
        // `../t` into `t` with a message. A `.` takes an extractor nowhere it
        // was not already; a `..` takes it out of the destination.
        assert_eq!(strip_leading(b"../t"), (&b"../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"../../t"), (&b"../../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"/../t"), (&b"/../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"./t"), (&b""[..], &b"./t"[..]));
        assert_eq!(strip_leading(b".t"), (&b""[..], &b".t"[..]));
        // `...` is a legal file name and is not two dots followed by anything.
        assert_eq!(strip_leading(b".../t"), (&b""[..], &b".../t"[..]));
    }

    #[test]
    #[cfg(unix)]
    fn an_interior_dotdot_moves_the_cut_past_itself() {
        // This is `safer_name_suffix`'s whole point and it is not a *leading*
        // prefix rule: GNU scans the entire name and cuts past the last `..`
        // component. It decides where a hard link whose target is `a/../base`
        // points -- at `base` -- and which prefix gets announced when a member
        // named `a/../b` is refused. This tar read the rule as "a leading run
        // of `/` and `../`" until `tar-rules2.sh` measured it.
        assert_eq!(strip_leading(b"a/../b"), (&b"a/../"[..], &b"b"[..]));
        assert_eq!(strip_leading(b"/d/../e"), (&b"/d/../"[..], &b"e"[..]));
        assert_eq!(strip_leading(b"a/../base"), (&b"a/../"[..], &b"base"[..]));
        // The cut lands past the *last* one, not the first.
        assert_eq!(
            strip_leading(b"a/../b/../c"),
            (&b"a/../b/../"[..], &b"c"[..])
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_name_that_is_nothing_but_prefix_is_left_with_nothing() {
        // `tar -c ..` -- the caller asked for the parent directory, and every
        // byte of the name is a prefix tar removes. GNU stores `.` (listed as
        // `./`), which is the only name left that means anything; the
        // substitution is `PrefixNotice`'s, not this function's.
        assert_eq!(strip_leading(b".."), (&b".."[..], &b""[..]));
        assert_eq!(strip_leading(b"d/.."), (&b"d/.."[..], &b""[..]));
        assert_eq!(strip_leading(b"/"), (&b"/"[..], &b""[..]));
        assert_eq!(strip_leading(b""), (&b""[..], &b""[..]));
    }

    #[test]
    fn a_link_target_that_does_not_fit_is_cut_rather_than_refused() {
        // GNU says "not dumped" and dumps it anyway with the target cut to 100
        // bytes. Measured -- a 101-byte symlink target warns, exits 2, and is
        // in the archive. The boolean is what drives the warning.
        let mut h = TarHeader::new();
        assert!(h.set_linkname(&[b'y'; 100]));
        assert_eq!(&h.linkname[..], &[b'y'; 100][..]);
        let mut h = TarHeader::new();
        assert!(!h.set_linkname(&[b'y'; 101]));
        assert_eq!(&h.linkname[..], &[b'y'; 100][..]);
    }

    #[test]
    #[cfg(unix)]
    fn device_numbers_split_the_way_the_kernel_packs_them() {
        // /dev/null is 1,3 -- measured, GNU lists it as `crw-rw-rw- 0/0 1,3`.
        assert_eq!(split_dev(0x0103), (1, 3));
        // A minor past 255 spills into the high half rather than into major.
        assert_eq!(split_dev((8 << 8) | 0x11), (8, 0x11));
    }

    #[test]
    fn list_keeps_a_name_holding_a_newline_on_one_line() {
        // The reason escaping is not merely cosmetic: a member called `a\nb`
        // printed raw makes `tar -t` report two files, one of them named `b`.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a\nb.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a\\nb.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn a_non_utf8_name_survives_the_header_round_trip() {
        // Store and read back, since `set_name` and `field_bytes` are the two
        // ends of the conversion and either alone could be lossless while the
        // pair is not.
        let name = b"\xff/dir\x80/\xe9\xe9";
        let block = make_header(name, 0, b'0');
        assert_eq!(field_bytes(block.get(..100).unwrap()), name);
        // And the stripping that stands between the two does not touch it.
        let mut prefixes = PrefixNotice::new();
        assert_eq!(
            prefixes.strip(
                field_bytes(block.get(..100).unwrap()),
                PrefixKind::MemberNames
            ),
            name
        );
    }

    #[test]
    fn list_reports_a_truncated_archive_rather_than_succeeding() {
        // Header announces a 1024-byte file but no data follows. The name was
        // already printed, so the listing is not empty -- but the walk must end
        // in `Truncated`, which is what turns into GNU's `Unexpected EOF in
        // archive` and a status of 2. This returned `Ok(())` and exited 0.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"liar.bin", 1024, b'0'));
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "liar.bin\n");
        assert!(matches!(stop, Stop::Truncated), "{stop:?}");
    }

    #[test]
    fn list_treats_a_short_later_header_as_a_clean_end() {
        // The counterpart to the test above, and the distinction GNU actually
        // draws: a partial block where a *header* would start is an ending, not
        // a truncation. Measured -- `head -c 700` of a 3584-byte archive (one
        // full member, then 188 bytes of the next header) exits 0 in silence.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"whole.txt", 0, b'0'));
        input.extend_from_slice(&[0xab; 188]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "whole.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_warns_about_a_lone_zero_block_but_still_succeeds() {
        // The end-of-archive marker is two zero blocks. One is accepted -- GNU
        // exits 0 -- but it warns and names the block, counting from 1: here the
        // header is block 1 and the zero block is block 2.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a.txt\n");
        assert!(matches!(stop, Stop::LoneZeroBlock(2)), "{stop:?}");
    }

    #[test]
    fn list_is_silent_about_a_proper_two_block_marker() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_refuses_a_short_first_block() {
        // Less than one full header block: nothing is written, and the file is
        // reported as not being an archive at all.
        let input = vec![0u8; 100];
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty());
        assert!(matches!(stop, Stop::NotAnArchive), "{stop:?}");
    }

    #[test]
    fn list_refuses_a_file_that_is_not_an_archive() {
        // The defect this whole reader exists to fix: 512 bytes of text have a
        // NUL-free "name" in the first 100, so the old listing printed a line of
        // the file's own contents and exited 0.
        let input = vec![b'A'; BLOCK_SIZE * 2];
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
        assert!(matches!(stop, Stop::NotAnArchive), "{stop:?}");
    }

    #[test]
    fn list_reports_a_bad_checksum_on_a_later_header() {
        // A good first member proves the file is an archive, so a corrupt
        // second header is a *different* complaint from "not an archive" -- GNU
        // says `Skipping to next header` for one and `This does not look like a
        // tar archive` for the other.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"good.txt", 0, b'0'));
        let mut bad = make_header(b"bad.txt", 0, b'0');
        bad[148..156].copy_from_slice(b"000000\0 ");
        input.extend_from_slice(&bad);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "good.txt\n");
        assert!(matches!(stop, Stop::BadHeader), "{stop:?}");
    }

    #[test]
    fn list_joins_the_ustar_prefix_to_the_name() {
        // The `prefix` field is how ustar stores a name longer than 100 bytes,
        // and it was never read -- so `long/dd.../ff...` listed (and extracted)
        // as just `ff...`, in the top-level directory.
        let mut block = make_header(b"leaf.txt", 0, b'0');
        block[345..345 + 8].copy_from_slice(b"deep/dir");
        // The checksum covers the prefix, so it has to be recomputed.
        let sum: u32 = block
            .iter()
            .enumerate()
            .map(|(i, &b)| u32::from(if (148..156).contains(&i) { b' ' } else { b }))
            .sum();
        let cs = format!("{sum:06o}\0 ");
        block[148..156].copy_from_slice(cs.as_bytes());

        let mut input = block.to_vec();
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "deep/dir/leaf.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_ignores_the_prefix_field_in_a_v7_header() {
        // In v7 those 155 bytes are padding, so reading them would invent a
        // directory out of whatever happened to be there.
        let mut block = make_header(b"leaf.txt", 0, b'0');
        block[257..263].copy_from_slice(&[0u8; 6]); // no `ustar` magic
        block[345..345 + 8].copy_from_slice(b"deep/dir");
        let sum: u32 = block
            .iter()
            .enumerate()
            .map(|(i, &b)| u32::from(if (148..156).contains(&i) { b' ' } else { b }))
            .sum();
        let cs = format!("{sum:06o}\0 ");
        block[148..156].copy_from_slice(cs.as_bytes());

        let mut input = block.to_vec();
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "leaf.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    // ---------------- the -tv long format ----------------

    /// GNU, measured, for an archive of one 6-byte 0755 file owned by 1000:1000
    /// with mtime 2020-01-02 03:04:05 UTC:
    ///
    /// ```text
    /// -rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt
    /// ```
    #[test]
    fn long_format_matches_gnu_for_numeric_owners() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// And with names stored, where `pad` is 14 rather than 10 and the gap
    /// narrows from nine spaces to five:
    ///
    /// ```text
    /// -rwxr-xr-x inhahe/inhahe     6 2020-01-02 03:04 t/a.txt
    /// ```
    #[test]
    fn long_format_prefers_the_stored_owner_names() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"inhahe",
            b"inhahe",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rwxr-xr-x inhahe/inhahe     6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// A 20 MiB member takes eight digits, so `pad` is 17 and the gap is two.
    /// The *next*, narrower line still uses the same width-18 column, which is
    /// what makes a listing's columns line up.
    #[test]
    fn long_format_column_is_a_running_maximum() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"big",
            0o644,
            1000,
            1000,
            20_971_520,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        // 20 MiB of data blocks, heap-allocated: a 20 MiB array literal is a
        // stack overflow, not a test.
        input.extend_from_slice(&vec![0u8; BLOCK_SIZE * 40_960]);
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rw-r--r-- 1000/1000  20971520 2020-01-02 03:04 big\n\
             -rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// Past 18 the column grows to fit, and the gap collapses to the single
    /// space the `+ 1` in the formula guarantees. Measured against GNU with
    /// `--owner=averyverylongusername --group=averyverylonggroupname`.
    #[test]
    fn long_format_column_grows_past_the_minimum() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o644,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"averyverylongusername",
            b"averyverylonggroupname",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rw-r--r-- averyverylongusername/averyverylonggroupname 6 \
             2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// The two suffixes, the directory's trailing slash, and the type letters.
    #[test]
    fn long_format_renders_every_member_type() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/",
            0o755,
            1000,
            1000,
            0,
            1_577_934_245,
            b'5',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/fifo",
            0o644,
            1000,
            1000,
            0,
            1_577_934_245,
            b'6',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/hard",
            0o755,
            1000,
            1000,
            0,
            1_577_934_245,
            b'1',
            b"t/a.txt",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/link",
            0o777,
            1000,
            1000,
            0,
            1_577_934_245,
            b'2',
            b"a.txt",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/su",
            0o4755,
            1000,
            1000,
            3,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "drwxr-xr-x 1000/1000         0 2020-01-02 03:04 t/\n\
             prw-r--r-- 1000/1000         0 2020-01-02 03:04 t/fifo\n\
             hrwxr-xr-x 1000/1000         0 2020-01-02 03:04 t/hard link to t/a.txt\n\
             lrwxrwxrwx 1000/1000         0 2020-01-02 03:04 t/link -> a.txt\n\
             -rwsr-xr-x 1000/1000         3 2020-01-02 03:04 t/su\n"
        );
    }

    // ---------------- mode_string ----------------

    #[test]
    fn mode_string_renders_the_override_bits() {
        assert_eq!(mode_string(0o755, b'0'), b"-rwxr-xr-x");
        assert_eq!(mode_string(0o644, b'0'), b"-rw-r--r--");
        assert_eq!(mode_string(0o000, b'0'), b"----------");
        assert_eq!(mode_string(0o777, b'5'), b"drwxrwxrwx");
        // setuid/setgid/sticky replace the execute letter, and their capital
        // form is how you tell "set, and not executable" from "not set".
        assert_eq!(mode_string(0o4755, b'0'), b"-rwsr-xr-x");
        assert_eq!(mode_string(0o4644, b'0'), b"-rwSr--r--");
        assert_eq!(mode_string(0o2755, b'0'), b"-rwxr-sr-x");
        assert_eq!(mode_string(0o2745, b'0'), b"-rwxr-Sr-x");
        assert_eq!(mode_string(0o1777, b'5'), b"drwxrwxrwt");
        assert_eq!(mode_string(0o1776, b'5'), b"drwxrwxrwT");
    }

    #[test]
    fn mode_string_names_every_type() {
        for (flag, letter) in [
            (b'0', b'-'),
            (b'\0', b'-'),
            (b'1', b'h'),
            (b'2', b'l'),
            (b'3', b'c'),
            (b'4', b'b'),
            (b'5', b'd'),
            (b'6', b'p'),
            (b'7', b'C'),
            (b'x', b'?'),
        ] {
            assert_eq!(mode_string(0, flag).first(), Some(&letter), "flag {flag:?}");
        }
    }

    // ---------------- extraction_mode ----------------

    /// Measured against GNU as a non-root user: the stored mode is masked by
    /// the umask and stripped of setuid/setgid/sticky, unless `-p` is given.
    #[test]
    fn extraction_mode_applies_the_umask_by_default() {
        assert_eq!(extraction_mode(0o777, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o777, false, 0o077), 0o700);
        assert_eq!(extraction_mode(0o644, false, 0o022), 0o644);
        assert_eq!(extraction_mode(0o755, false, 0o000), 0o755);
    }

    #[test]
    fn extraction_mode_drops_setuid_unless_asked() {
        // An archive is an untrusted input; honouring its setuid bit would let
        // anyone who can hand you a tarball hand you a setuid binary.
        assert_eq!(extraction_mode(0o4755, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o2755, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o1777, false, 0o022), 0o755);
        // `-p` is the caller saying they know where the archive came from.
        assert_eq!(extraction_mode(0o4755, true, 0o022), 0o4755);
        assert_eq!(extraction_mode(0o777, true, 0o077), 0o777);
    }

    // ---------------- Selector ----------------

    #[test]
    fn selector_with_no_operands_wants_everything() {
        let mut sel = Selector::new(&[]);
        assert!(sel.wants(b"anything"));
        assert!(sel.wants(b"a/b/c"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn selector_matches_a_named_member_and_its_subtree() {
        // `tar -xf a.tar dir` unpacks the subtree, not the bare entry -- and
        // with no selector at all this used to unpack the whole archive.
        let mut sel = Selector::new(&s(&["t/sub"]));
        assert!(sel.wants(b"t/sub/"));
        assert!(sel.wants(b"t/sub/b.bin"));
        assert!(!sel.wants(b"t/a.txt"));
        // Not a prefix match on bytes: `t/subterranean` is a different name.
        assert!(!sel.wants(b"t/subterranean"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn selector_ignores_trailing_slashes_on_either_side() {
        let mut sel = Selector::new(&s(&["t/sub/"]));
        assert!(sel.wants(b"t/sub"));
        assert!(sel.wants(b"t/sub/b.bin"));
    }

    #[test]
    fn selector_reports_an_operand_that_matched_nothing() {
        let mut sel = Selector::new(&s(&["present", "absent"]));
        assert!(sel.wants(b"present"));
        assert_eq!(sel.report_missing(), EXIT_FATAL);
    }

    #[test]
    fn selector_takes_an_operand_that_is_not_utf8() {
        let mut sel = Selector::new(&b(&[b"caf\xe9"]));
        assert!(sel.wants(b"caf\xe9"));
        assert!(sel.wants(b"caf\xe9/inside"));
        assert!(!sel.wants(b"cafe"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn trim_slashes_never_empties_a_name() {
        // Two callers: matching an operand against a member name, and turning a
        // directory member's stored `d/` into the `d` that its diagnostics name
        // and that `make_ancestors` must not mistake for its own ancestor.
        assert_eq!(trim_slashes(b"a/"), b"a");
        assert_eq!(trim_slashes(b"a///"), b"a");
        assert_eq!(trim_slashes(b"a/b//"), b"a/b");
        assert_eq!(trim_slashes(b"a"), b"a");
        // `/` alone would otherwise become the empty string, which prefixes
        // every member name there is.
        assert_eq!(trim_slashes(b"/"), b"/");
        assert_eq!(trim_slashes(b""), b"");
    }
}
