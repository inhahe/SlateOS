//! chown — change file owner and group.
//!
//! ```text
//! Usage: chown [OPTION]... [OWNER][:[GROUP]] FILE...
//!   or:  chown [OPTION]... --reference=RFILE FILE...
//! ```
//!
//! # Why the symlink rules are the important part of this file
//!
//! `chown` hands a file to another user, so an implementation that can be
//! talked into handing over the *wrong* file is a way to take over an account.
//! Two rules keep that from happening by default, and both were once broken
//! (`known-issues.md` → `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`):
//!
//! 1. **`-R` does not walk through symlinks unless asked.** It used to test
//!    `path.is_dir()`, which follows them, so `chown -R alice srv/` on a tree
//!    containing `srv/x -> /etc` walked into `/etc` and gave alice the lot.
//!    POSIX makes `-P` the default for exactly this reason.
//! 2. **Under `-R` alone, a symlink met is changed, not its target.** The
//!    `chown(2)` call follows links, so `srv/x -> /etc/shadow` used to hand
//!    `/etc/shadow` to alice. The walk uses `lchown(2)` -- and
//!    `-R --dereference`, which asks for the opposite with nothing walked
//!    through, is refused, as upstream refuses it.
//!
//! `-H` and `-L` are the caller asking for symlinks to be followed, and then
//! GNU's rules apply exactly, including the one that surprises: a symlink met
//! *inside* the tree has its target changed unless `-h` is given as well. The
//! walk and those rules are [`coreutils::chowncore`], shared with `chgrp` as
//! upstream shares `chown-core.c`; design-decisions.md §1029 records why they
//! follow GNU rather than the stricter mixture this file used to have.
//!
//! `-r` is **not** accepted as a spelling of `-R`. It is not an option at all
//! in POSIX chown, and quietly treating a typo as "recurse" is how a change
//! meant for one file reaches a whole tree.
//!
//! # The owner spec is a grammar, and it was three lines
//!
//! This file used to read `OWNER[:GROUP]` with `split_once(':')` and
//! `str::parse::<u32>()`, and documented the result as "name lookup not yet
//! supported". That last part had stopped being true: `userspace/pwdb` parses
//! `/etc/passwd` and `/etc/group` and `ls -l` has been resolving both columns
//! through it. So `chown alice file` — the spelling in every piece of
//! documentation and every script in the world — answered `invalid user:
//! 'alice'` on a system that knew perfectly well who alice was.
//!
//! Restoring it means porting gnulib's `parse_user_spec`, because the grammar
//! is much larger than "a name or a number split on a colon", and every corner
//! of it is observable. All of the following were **measured** against GNU
//! coreutils 9.4 rather than recalled:
//!
//! | Spec | Means |
//! |---|---|
//! | `alice` | uid only; the group is left alone |
//! | `alice:` | uid, **and alice's login group** — which only an account has |
//! | `1000:` | `invalid spec` — a number has no login group to look up |
//! | `alice:staff` | both |
//! | `:staff` | group only |
//! | `:` and the empty string | neither; a no-op that still reports |
//! | `alice.staff` | both, after `warning: '.' should be ':'` |
//! | `+1000` | uid 1000, *skipping* the name lookup |
//! | ` 1000` | uid 1000 — the fallback is `strtoul`, which skips whitespace |
//! | `-0`, `1000 `, `0x10` | `invalid user` — `strtoul` with the whole string |
//! | `4294967295` | `invalid user`; it is `(uid_t)-1`, "leave this alone" |
//!
//! The `.` rule is a POSIX-compatible extension and is tried **only** after the
//! colon-less reading has already failed, which is why an account genuinely
//! called `a.b` still works.
//!
//! # `-v` and `-c` report names, and the rule for which is not obvious
//!
//! Ported from `chown-core.c`'s `describe_change` and `user_group_str`, which
//! together decide between four sentences and two spec shapes. The rule is
//! that a field contributes a *name* only when a lookup found one, so the same
//! gid reached two ways prints two different lines:
//!
//! ```text
//! $ chown -v :root f          ownership of 'f' retained as root:root
//! $ chown -v :0 f             group of 'f' retained as root
//! ```
//!
//! That is not a bug being reproduced for its own sake — it falls out of
//! `chown.c` setting the user name to the empty string when a group was named
//! but a user was not, so that the message reads `:GROUP` rather than `GROUP`.
//! One genuine upstream oddity does come along with it, and is reproduced
//! rather than tidied because a diagnostic that differs from GNU's is a
//! diagnostic no existing script can parse: `chown -v 1234:daemon f` sets uid
//! 1234 and prints `to :daemon`, dropping the uid it just set. Measured.
//!
//! # Non-unix hosts
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches). On non-unix hosts — Windows, where
//! `cargo test --workspace` runs — a stub `main` keeps the workspace
//! compile-clean and every pure helper is still compiled and unit-tested,
//! because an untestable security rule is one that regresses quietly.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::chowncore::{Dereference, Traverse, Verbosity, walk_policy};
#[cfg(not(unix))]
use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::userspec::{Spec, parse_user_spec};
use pwdb::Db;
use std::ffi::OsString;

/// `chown`'s usage status is 1 — measured: `chown; echo $?` prints 1.
const CHOWN: Program = Program::new("chown", 1);

/// GNU `chown`'s `getopt_long` short string, exactly.
const SHORT_OPTIONS: &str = "HLPRcfhv";

/// GNU `chown`'s `long_options[]`, in its declaration order.
///
/// The order is user-visible, not bookkeeping: an ambiguous prefix lists its
/// candidates in table order. Measured with the empty prefix, which matches
/// every entry:
///
/// ```text
/// $ chown --=x
/// chown: option '--=x' is ambiguous; possibilities: '--recursive' '--changes'
/// '--dereference' '--from' '--no-dereference' '--no-preserve-root'
/// '--preserve-root' '--quiet' '--silent' '--reference' '--verbose' '--help'
/// '--version'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("recursive", Takes::Nothing),
    ("changes", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("from", Takes::Required),
    ("no-dereference", Takes::Nothing),
    ("no-preserve-root", Takes::Nothing),
    ("preserve-root", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("reference", Takes::Required),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--quiet` and `--silent` are one option. Without this the parser would call
/// `--s` ambiguous, which it is not: it resolves to `silent`, an alias of
/// `quiet`, and GNU accepts it. Measured: `chown --s` reaches `missing operand`.
const LONG_ALIASES: &[(&str, &str)] = &[("silent", "quiet")];

/// Where the new ownership comes from.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Source {
    /// An `OWNER[:GROUP]` operand, still unparsed: resolving it needs the
    /// account database, and this parse touches no file.
    Spec(OsString),
    /// `--reference=RFILE`: whatever owner and group that file turns out to
    /// have. Always dereferenced, as the help text promises.
    Reference(OsString),
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Settings {
    recursive: bool,
    /// The traversal in effect, after [`walk_policy`].
    traverse: Traverse,
    /// Change a symlink's target rather than the link, after [`walk_policy`].
    affect_referent: bool,
    verbosity: Verbosity,
    /// `-f`: keep going quietly. The exit status still reflects the failures;
    /// only the messages are suppressed.
    force_silent: bool,
    /// `--preserve-root`: refuse to recurse from `/`.
    preserve_root: bool,
    /// `--from=CURRENT_OWNER:CURRENT_GROUP`, still unparsed for the same reason
    /// as [`Source::Spec`].
    from: Option<OsString>,
    source: Source,
    files: Vec<OsString>,
}

/// What the command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

fn help_text() -> String {
    "\
Usage: chown [OPTION]... [OWNER][:[GROUP]] FILE...
  or:  chown [OPTION]... --reference=RFILE FILE...
Change the owner and/or group of each FILE to OWNER and/or GROUP.
With --reference, change the owner and group of each FILE to those of RFILE.

  -c, --changes          like verbose but report only when a change is made
  -f, --silent, --quiet  suppress most error messages
  -v, --verbose          output a diagnostic for every file processed
      --dereference      affect the referent of each symbolic link (this is
                         the default), rather than the symbolic link itself
  -h, --no-dereference   affect symbolic links instead of any referenced file
      --from=CURRENT_OWNER:CURRENT_GROUP
                         change the owner and/or group of each file only if
                         its current owner and/or group match those specified
                         here.  Either may be omitted, in which case a match
                         is not required for the omitted attribute
      --no-preserve-root  do not treat '/' specially (the default)
      --preserve-root    fail to operate recursively on '/'
      --reference=RFILE  use RFILE's owner and group rather than specifying
                         OWNER:GROUP values.  RFILE is always dereferenced.
  -R, --recursive        operate on files and directories recursively

The following options modify how a hierarchy is traversed when the -R
option is also specified.  If more than one is specified, only the final
one takes effect.

  -H                     if a command line argument is a symbolic link
                         to a directory, traverse it
  -L                     traverse every symbolic link to a directory
                         encountered
  -P                     do not traverse any symbolic links (default)

      --help        display this help and exit
      --version     output version information and exit

Owner is unchanged if missing.  Group is unchanged if missing, but changed
to login group if implied by a ':' following a symbolic OWNER.
OWNER and GROUP may be numeric as well as symbolic.

Examples:
  chown root /u        Change the owner of /u to \"root\".
  chown root:staff /u  Likewise, but also change its group to \"staff\".
  chown -hR root /u    Change the owner of /u and subfiles to \"root\".
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse chown's argv.
///
/// Unknown options are an error. They used to be silently accepted as
/// positionals, so `chown 1000 -v file` tried to change the owner of a file
/// literally named `-v` and reported "No such file or directory" about it.
///
/// # Errors
///
/// An unknown or ambiguous option, or too few operands.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut recursive = false;
    let mut dereference = Dereference::default();
    let mut traverse = Traverse::default();
    let mut verbosity = Verbosity::Off;
    let mut force_silent = false;
    let mut preserve_root = false;
    let mut from: Option<OsString> = None;
    let mut reference: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();

    for item in CHOWN.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Short(b'R', _) | Opt::Long("recursive", _) => recursive = true,
            Opt::Short(b'c', _) | Opt::Long("changes", _) => verbosity = Verbosity::ChangesOnly,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => verbosity = Verbosity::High,
            // Both spellings, because an exact long option resolves to the name
            // that was typed rather than to the alias's target — the alias map
            // settles ambiguity and nothing else. See `resolve_long_aliased`.
            Opt::Short(b'f', _) | Opt::Long("quiet" | "silent", _) => force_silent = true,
            Opt::Short(b'h', _) | Opt::Long("no-dereference", _) => dereference = Dereference::Link,
            Opt::Long("dereference", _) => dereference = Dereference::Referent,
            Opt::Short(b'H', _) => traverse = Traverse::CommandLine,
            Opt::Short(b'L', _) => traverse = Traverse::Logical,
            Opt::Short(b'P', _) => traverse = Traverse::Physical,
            Opt::Long("preserve-root", _) => preserve_root = true,
            Opt::Long("no-preserve-root", _) => preserve_root = false,
            Opt::Long("from", value) => from = value,
            Opt::Long("reference", value) => reference = value,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the tables list nothing else, and every entry is
            // handled above. Refusing rather than ignoring, so an option added
            // to a table without a handler fails loudly instead of silently
            // doing nothing.
            Opt::Long(other, _) => {
                return Err(CHOWN.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(CHOWN.invalid_option(other)),
        }
    }

    // Checked before the operands, as upstream checks it: `chown -R
    // --dereference` alone is this message, not `missing operand`. Upstream's
    // `error (EXIT_FAILURE, …)`, so status 1 and no referral.
    let (traverse, affect_referent) = walk_policy(recursive, traverse, dereference)
        .map_err(|message| CHOWN.usage(message.to_string()))?;

    // `--reference` supplies the ownership, so it needs one fewer operand.
    let wanted = if reference.is_some() { 1 } else { 2 };
    if operands.len() < wanted {
        // Two wordings, and which one appears depends only on whether there was
        // anything to name. Measured: `chown` is `missing operand`, `chown 1000`
        // is `missing operand after ‘1000’`.
        let message = match operands.last() {
            None => "missing operand".to_string(),
            Some(last) => format!("missing operand after {}", quote(&os_bytes(last))),
        };
        return Err(CHOWN.usage_referring(message));
    }

    let mut files = operands;
    let source = match reference {
        Some(rfile) => Source::Reference(rfile),
        // Not `remove(0)` on a `Vec` we own outright: the owner operand is the
        // first word and the files are the rest.
        None => Source::Spec(files.remove(0)),
    };

    Ok(Request::Run(Box::new(Settings {
        recursive,
        traverse,
        affect_referent,
        verbosity,
        force_silent,
        preserve_root,
        from,
        source,
        files,
    })))
}

// ------------------------------------------------------------- owner specs ---

/// The operand spec as `chown` itself resolves it: gnulib's
/// [`parse_user_spec`], then `chown.c`'s group-only fixup.
///
/// The two are separable in upstream's source but not in its behaviour, so they
/// are one function here. The fixup, in `chown.c`'s own words: "If a group is
/// specified but no user, set the user name to the empty string so that
/// diagnostics say 'ownership :GROUP' rather than 'group GROUP'."
///
/// That empty name is not a lookup result and never reaches `chown(2)` — `uid`
/// stays `None`, so the user field is left alone. It exists only to steer
/// [`describe_change`] onto the `ownership` sentence, which is why `chown
/// :staff` and `chown :5678` word their `-v` lines differently despite doing
/// the same thing to the file: the first resolved a group *name* and so gets a
/// user half to pair it with, the second did not.
///
/// It is deliberately not applied to `--from`, which discards names entirely
/// and compares only ids — upstream passes null name pointers there.
///
/// # Errors
///
/// As [`parse_user_spec`].
fn resolve_spec(text: &[u8], db: &Db) -> Result<(Spec, bool), &'static str> {
    let (mut spec, dotted) = parse_user_spec(text, db)?;
    if spec.user_name.is_none() && spec.group_name.is_some() {
        spec.user_name = Some(Vec::new());
    }
    Ok((spec, dotted))
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    diag!("chown: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{CHOWN, Request, Settings, Source, help_text, parse_args, parse_user_spec, resolve_spec};
    use coreutils::chowncore::{Ids, Options, chown_files};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, quote, quoteaf_os};
    use coreutils::stdfd::{self, Stream};
    use coreutils::userspec::{Spec, gid_to_name, uid_to_name};
    use pwdb::Db;
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::process::ExitCode;

    /// Resolve the spec the whole run will apply, before touching any operand.
    ///
    /// Returns `Err` with the process's exit status; every failure here is
    /// fatal, because there is nothing left to do without an answer.
    fn resolve_source(settings: &Settings, db: &Db) -> Result<Spec, u8> {
        match &settings.source {
            Source::Reference(rfile) => {
                // Dereferenced: `metadata`, not `symlink_metadata`. GNU's help
                // text promises this in as many words.
                let meta = fs::metadata(Path::new(rfile)).map_err(|e| {
                    diag!(
                        "chown: failed to get attributes of {}: {}",
                        quoteaf_os(rfile),
                        strerror(&e)
                    );
                    1u8
                })?;
                // A reference file always contributes *both* fields, and both
                // as names, so `-v` reports `user:group` even for ids the
                // database does not know (`uid_to_name` falls back to digits).
                Ok(Spec {
                    uid: Some(meta.uid()),
                    gid: Some(meta.gid()),
                    user_name: Some(uid_to_name(db, meta.uid())),
                    group_name: Some(gid_to_name(db, meta.gid())),
                })
            }
            Source::Spec(text) => {
                let bytes = os_bytes(text);
                let (spec, dotted) = resolve_spec(&bytes, db).map_err(|message| {
                    diag!("chown: {message}: {}", quote(&bytes));
                    1u8
                })?;
                if dotted {
                    // Upstream calls `error (0, …)` here — a warning, not a
                    // failure — and carries on with the dot reading.
                    diag!("chown: warning: '.' should be ':': {}", quote(&bytes));
                }
                Ok(spec)
            }
        }
    }

    /// Resolve `--from`, discarding the names: only the ids are compared.
    fn resolve_from(settings: &Settings, db: &Db) -> Result<Ids, u8> {
        let Some(text) = &settings.from else {
            return Ok(Ids::default());
        };
        let bytes = os_bytes(text);
        let (spec, dotted) = parse_user_spec(&bytes, db).map_err(|message| {
            diag!("chown: {message}: {}", quote(&bytes));
            1u8
        })?;
        if dotted {
            diag!("chown: warning: '.' should be ':': {}", quote(&bytes));
        }
        Ok(Ids {
            uid: spec.uid,
            gid: spec.gid,
        })
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Run(settings)) => *settings,
            Ok(Request::Help) => {
                let mut out = Stream::stdout();
                // Deliberately unread: `close_stdout` reports a failed write.
                let _ = out.write_all(help_text().as_bytes());
                return stdfd::close_stdout("chown", out, ExitCode::SUCCESS);
            }
            Ok(Request::Version) => {
                let mut out = Stream::stdout();
                let _ = out.write_all(b"chown (SlateOS coreutils) 0.1.0\n");
                return stdfd::close_stdout("chown", out, ExitCode::SUCCESS);
            }
            Err(e) => {
                CHOWN.report(&e);
                return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
            }
        };

        // One read of `/etc/passwd` and `/etc/group` for the whole run. A
        // database that cannot be read is an empty one, not an error — see
        // `pwdb::Db::from_files` — so a system without the files still does
        // numeric chowns rather than refusing to start.
        let db = Db::load();
        let spec = match resolve_source(&settings, &db) {
            Ok(spec) => spec,
            Err(status) => return ExitCode::from(status),
        };
        let required = match resolve_from(&settings, &db) {
            Ok(required) => required,
            Err(status) => return ExitCode::from(status),
        };

        let root_dev_ino = if settings.recursive && settings.preserve_root {
            match fs::metadata(Path::new("/")) {
                Ok(meta) => Some((meta.dev(), meta.ino())),
                Err(e) => {
                    diag!(
                        "chown: failed to get attributes of {}: {}",
                        quoteaf_os("/"),
                        strerror(&e)
                    );
                    return ExitCode::from(1);
                }
            }
        } else {
            None
        };

        let options = Options {
            program: "chown",
            recursive: settings.recursive,
            traverse: settings.traverse,
            affect_referent: settings.affect_referent,
            verbosity: settings.verbosity,
            force_silent: settings.force_silent,
            root_dev_ino,
            user_name: spec.user_name.clone(),
            group_name: spec.group_name.clone(),
        };
        // `-v` and `-c` lines go through a `Stream`, so a full or closed stdout
        // is reported once, at the end, as `write error` -- `println!` would
        // have panicked on the first line instead.
        let mut out = Stream::stdout();
        let ok = chown_files(
            &settings.files,
            Ids {
                uid: spec.uid,
                gid: spec.gid,
            },
            required,
            &options,
            &db,
            &mut out,
        );
        let earned = if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
        stdfd::close_stdout("chown", out, earned)
    }
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`coreutils::stdfd::close_stderr`].
#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use coreutils::chowncore::{ChangeStatus, describe_change, user_group_str};
    use std::ffi::OsStr;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn run(items: &[&str]) -> Settings {
        match parse_args(&argv(items)).unwrap() {
            Request::Run(settings) => *settings,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(items: &[&str]) -> String {
        parse_args(&argv(items)).unwrap_err().message()
    }

    fn owner_of(settings: &Settings) -> String {
        match &settings.source {
            Source::Spec(text) => text.to_string_lossy().into_owned(),
            Source::Reference(r) => panic!("expected a spec, got --reference={r:?}"),
        }
    }

    /// A small, self-contained account database. `Db::from_bytes` exists for
    /// exactly this: the development host has no `/etc/passwd` at all, so a
    /// test that read the real one would be a test that only runs on the target.
    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:1000:Alice:/home/alice:/bin/sh\n\
              bob:x:1001:2000:Bob:/home/bob:/bin/sh\n\
              1000:x:4000:4000:confusing:/:/bin/sh\n\
              a.b:x:5000:5000:dotted:/:/bin/sh\n",
            b"root:x:0:\n\
              alice:x:1000:\n\
              staff:x:2000:alice\n\
              nogroupfile:x:9999:\n",
        )
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_no_args_errors() {
        assert!(err(&[]).contains("missing operand"));
        assert!(err(&[]).contains("Try 'chown --help'"));
    }

    /// Two wordings, and the second names the last word typed. Measured:
    /// `chown 1000` is `missing operand after ‘1000’`, not `missing operand`.
    #[test]
    fn parse_owner_only_names_the_operand() {
        let message = err(&["1000"]);
        assert!(message.contains("missing operand after"), "{message}");
        assert!(message.contains("1000"), "{message}");
    }

    #[test]
    fn parse_owner_and_file() {
        let a = run(&["1000", "f"]);
        assert!(!a.recursive);
        assert_eq!(owner_of(&a), "1000");
        assert_eq!(a.files, argv(&["f"]));
    }

    #[test]
    fn parse_recursive_dash_r_uppercase() {
        let a = run(&["-R", "1000:100", "dir"]);
        assert!(a.recursive);
        assert_eq!(owner_of(&a), "1000:100");
        assert_eq!(a.files, argv(&["dir"]));
    }

    /// POSIX chown has no `-r`. This used to set `recursive`, so a typo turned
    /// a single-file change into a whole-tree one -- and a test asserted that
    /// it did. See known-issues.md -> B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING.
    #[test]
    fn parse_lowercase_r_is_rejected_not_treated_as_recursive() {
        let message = err(&["-r", "1000", "f"]);
        assert!(message.contains("invalid option"), "got: {message}");
        assert!(message.contains('r'));
    }

    #[test]
    fn parse_multiple_files() {
        assert_eq!(run(&["0:0", "a", "b", "c"]).files, argv(&["a", "b", "c"]));
    }

    /// Options after the owner are still options, and the owner is still the
    /// first *operand* rather than the first word.
    #[test]
    fn parse_recursive_flag_position_independent() {
        let a = run(&["1000", "-R", "dir"]);
        assert!(a.recursive);
        assert_eq!(owner_of(&a), "1000");
        assert_eq!(a.files, argv(&["dir"]));
    }

    #[test]
    fn parse_long_recursive() {
        assert!(run(&["--recursive", "1000", "dir"]).recursive);
    }

    /// The whole point of routing through `getopt`: GNU abbreviates every long
    /// option, and the hand-written parser this replaced accepted only the full
    /// spelling. `--recur` is unambiguous; `--r` is not, because `--reference`
    /// shares the prefix. Both measured.
    #[test]
    fn parse_long_options_abbreviate() {
        assert!(run(&["--recur", "1000", "d"]).recursive);
        let message = err(&["--r", "1000", "d"]);
        assert!(message.contains("ambiguous"), "{message}");
        assert!(message.contains("'--recursive' '--reference'"), "{message}");
    }

    /// `--silent` is an alias of `--quiet`, so `--s` resolves rather than being
    /// ambiguous. Without the alias table it would be refused.
    #[test]
    fn parse_silent_abbreviation_is_not_ambiguous() {
        assert!(run(&["--s", "1000", "f"]).force_silent);
        assert!(run(&["--q", "1000", "f"]).force_silent);
    }

    /// Bundling, which the hand-written parser also could not do.
    #[test]
    fn parse_short_options_bundle() {
        let a = run(&["-Rvf", "1000", "d"]);
        assert!(a.recursive);
        assert_eq!(a.verbosity, Verbosity::High);
        assert!(a.force_silent);
    }

    #[test]
    fn parse_no_dereference() {
        assert!(!run(&["-h", "1000", "link"]).affect_referent);
        assert!(!run(&["--no-dereference", "1000", "link"]).affect_referent);
        // Without -h an operand's target is what changes.
        assert!(run(&["1000", "link"]).affect_referent);
    }

    #[test]
    fn parse_dereference_overrides_h() {
        // Last one wins, as with GNU.
        assert!(run(&["-h", "--dereference", "1000", "link"]).affect_referent);
        assert!(!run(&["--dereference", "-h", "1000", "link"]).affect_referent);
    }

    /// -P is the default, and under -R it also means every symlink met is
    /// changed as a link: without both, `-R` on a tree containing a symlink to
    /// /etc would walk into /etc or hand its target away.
    #[test]
    fn parse_traverse_defaults_to_physical_and_links() {
        let a = run(&["-R", "1000", "d"]);
        assert_eq!((a.traverse, a.affect_referent), (Traverse::Physical, false));
    }

    #[test]
    fn parse_traverse_flags() {
        let a = run(&["-R", "-H", "1000", "d"]);
        assert_eq!((a.traverse, a.affect_referent), (Traverse::CommandLine, true));
        let a = run(&["-R", "-L", "1000", "d"]);
        assert_eq!((a.traverse, a.affect_referent), (Traverse::Logical, true));
        let a = run(&["-R", "-L", "-h", "1000", "d"]);
        assert_eq!((a.traverse, a.affect_referent), (Traverse::Logical, false));
        // The last of -H/-L/-P wins.
        assert_eq!(run(&["-R", "-L", "-P", "1000", "d"]).traverse, Traverse::Physical);
        // Without -R nothing is walked, whatever was asked.
        assert_eq!(run(&["-L", "1000", "d"]).traverse, Traverse::Physical);
    }

    /// Measured: refused before the operands are counted, with no referral.
    #[test]
    fn parse_r_dereference_needs_h_or_l() {
        assert_eq!(
            err(&["-R", "--dereference", "1000", "d"]),
            "-R --dereference requires either -H or -L"
        );
        assert_eq!(
            err(&["-R", "--dereference"]),
            "-R --dereference requires either -H or -L"
        );
        assert!(run(&["-R", "-H", "--dereference", "1000", "d"]).affect_referent);
        // Without -R there is nothing to require.
        assert!(run(&["--dereference", "1000", "d"]).affect_referent);
    }

    #[test]
    fn parse_verbose_and_quiet() {
        let a = run(&["-v", "-f", "1000", "f"]);
        assert_eq!(a.verbosity, Verbosity::High);
        assert!(a.force_silent);
        assert_eq!(run(&["-c", "1000", "f"]).verbosity, Verbosity::ChangesOnly);
    }

    #[test]
    fn parse_preserve_root_flags() {
        assert!(!run(&["1000", "f"]).preserve_root);
        assert!(run(&["--preserve-root", "1000", "f"]).preserve_root);
        assert!(!run(&["--preserve-root", "--no-preserve-root", "1000", "f"]).preserve_root);
    }

    /// `--reference` supplies the ownership, so one operand is enough — and the
    /// word that would have been the owner is a file instead.
    #[test]
    fn parse_reference_consumes_no_operand() {
        let a = run(&["--reference=r", "f"]);
        assert_eq!(a.source, Source::Reference(OsString::from("r")));
        assert_eq!(a.files, argv(&["f"]));
        assert!(err(&["--reference=r"]).contains("missing operand"));
    }

    #[test]
    fn parse_from_is_kept_unparsed() {
        assert_eq!(
            run(&["--from=alice", "1000", "f"]).from,
            Some(OsString::from("alice"))
        );
        assert_eq!(
            run(&["--from", "alice", "1000", "f"]).from,
            Some(OsString::from("alice"))
        );
    }

    #[test]
    fn parse_unknown_short_option_rejected() {
        assert!(err(&["-z", "1000", "f"]).contains("invalid option"));
    }

    #[test]
    fn parse_unknown_long_option_rejected() {
        assert!(err(&["--frobnicate", "1000", "f"]).contains("unrecognized option"));
    }

    #[test]
    fn parse_help_and_version_win_over_later_errors() {
        // `getopt` yields one item at a time precisely so this ordering works:
        // measured, `chown --help --bogus` prints the help.
        assert_eq!(
            parse_args(&argv(&["--help", "--bogus"])).unwrap(),
            Request::Help
        );
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        assert!(parse_args(&argv(&["--bogus", "--help"])).is_err());
    }

    #[test]
    fn parse_double_dash_ends_options() {
        // The only way to address a file called `-R`.
        let a = run(&["--", "1000", "-R", "-h"]);
        assert!(!a.recursive);
        assert!(a.affect_referent);
        assert_eq!(owner_of(&a), "1000");
        assert_eq!(a.files, argv(&["-R", "-h"]));
    }

    #[test]
    fn parse_bare_dash_is_positional() {
        assert_eq!(run(&["1000", "-"]).files, argv(&["-"]));
    }

    /// argv is bytes. A file name that is not UTF-8 is a legal name here, and
    /// the parser must carry it through untouched rather than refuse it.
    #[cfg(unix)]
    #[test]
    fn parse_keeps_non_utf8_operands() {
        use std::os::unix::ffi::OsStringExt;
        let name = OsString::from_vec(vec![0xff, 0xfe, b'x']);
        let args = vec![OsString::from("1000"), name.clone()];
        let Request::Run(settings) = parse_args(&args).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(settings.files, vec![name]);
    }

    // ---------------- resolve_spec ----------------

    /// The gnulib half alone, without chown's fixup. Its own grammar is tested
    /// in `coreutils::userspec`; what is left here is what chown adds.
    fn spec(text: &str) -> Spec {
        parse_user_spec(text.as_bytes(), &db()).unwrap().0
    }

    /// The spec as the *utility* resolves it, fixup included — which is what
    /// the reporting tests must use, since the fixup exists only to change what
    /// is reported.
    fn resolved(text: &str) -> Spec {
        resolve_spec(text.as_bytes(), &db()).unwrap().0
    }

    /// The group-only fixup: a resolved group name conjures an empty user name,
    /// an unresolved one does not. Nothing about the file changes either way —
    /// `uid` stays `None` in both — so this is purely about wording.
    #[test]
    fn resolve_spec_adds_an_empty_user_only_for_a_named_group() {
        let named = resolved(":staff");
        assert_eq!(named.user_name.as_deref(), Some(&b""[..]));
        assert_eq!(named.uid, None);
        assert_eq!(named.gid, Some(2000));

        let numeric = resolved(":5678");
        assert_eq!(numeric.user_name, None);
        assert_eq!(numeric.uid, None);
        assert_eq!(numeric.gid, Some(5678));

        // A user was given, so there is nothing to fix up.
        assert_eq!(
            resolved("alice:staff").user_name.as_deref(),
            Some(&b"alice"[..])
        );
        // Neither half resolved a name, and an empty spec must stay empty.
        assert_eq!(resolved("").user_name, None);
    }

    // ---------------- describe_change ----------------

    fn described(status: ChangeStatus, spec: &Spec, old: (&str, &str)) -> String {
        // Mirrors `report`'s `chopt->user_name ? … : uid_to_str (uid)`.
        let user = spec
            .user_name
            .clone()
            .or_else(|| spec.uid.map(|uid| uid.to_string().into_bytes()));
        let group = spec
            .group_name
            .clone()
            .or_else(|| spec.gid.map(|gid| gid.to_string().into_bytes()));
        describe_change(
            OsStr::new("f"),
            status,
            Some(old.0.as_bytes()),
            Some(old.1.as_bytes()),
            user.as_deref(),
            group.as_deref(),
        )
    }

    /// The four sentences, against measured GNU output. The pair that matters
    /// is `:root` against `:0`: the same gid, reached two ways, prints two
    /// different lines because only one of them resolved a name.
    #[test]
    fn describe_matches_measured_gnu_wording() {
        let old = ("root", "root");
        assert_eq!(
            described(ChangeStatus::Succeeded, &resolved(":staff"), old),
            "changed ownership of 'f' from root:root to :staff"
        );
        assert_eq!(
            described(ChangeStatus::Succeeded, &resolved(":5678"), old),
            "changed group of 'f' from root to 5678"
        );
        assert_eq!(
            described(ChangeStatus::NoChangeRequested, &resolved(":root"), old),
            "ownership of 'f' retained as root:root"
        );
        assert_eq!(
            described(ChangeStatus::NoChangeRequested, &resolved(":0"), old),
            "group of 'f' retained as root"
        );
        assert_eq!(
            described(ChangeStatus::Succeeded, &resolved("1234"), old),
            "changed ownership of 'f' from root to 1234"
        );
        assert_eq!(
            described(ChangeStatus::NoChangeRequested, &resolved(""), old),
            "ownership of 'f' retained"
        );
    }

    /// A file that could not be stat'd has no "from", and upstream shifts the
    /// new spec into that slot rather than printing an empty one. Measured:
    /// `chown -v 1234 nosuch`.
    #[test]
    fn describe_failure_without_a_stat_drops_the_from() {
        assert_eq!(
            describe_change(
                OsStr::new("nosuch"),
                ChangeStatus::Failed,
                None,
                None,
                Some(b"1234"),
                None,
            ),
            "failed to change ownership of 'nosuch' to 1234"
        );
        assert_eq!(
            describe_change(
                OsStr::new("f"),
                ChangeStatus::Failed,
                Some(b"alice"),
                Some(b"alice"),
                Some(b"1234"),
                None,
            ),
            "failed to change ownership of 'f' from alice to 1234"
        );
    }

    /// The fourth status names neither the old ownership nor the new one,
    /// because nothing moved — and it is not a failure, because POSIX says a
    /// system may decline to own a symlink and that declining is not an error.
    #[test]
    fn describe_not_applied_names_no_ownership_at_all() {
        assert_eq!(
            describe_change(
                OsStr::new("link"),
                ChangeStatus::NotApplied,
                Some(b"root"),
                Some(b"root"),
                Some(b"alice"),
                Some(b"staff"),
            ),
            "neither symbolic link 'link' nor referent has been changed"
        );
    }

    /// A file name is quoted, because a path may contain a newline and a
    /// `-v` line that printed one raw would let whoever chose the name write
    /// extra lines of our output.
    #[test]
    fn describe_quotes_the_file_name() {
        let line = describe_change(
            OsStr::new("two\nlines"),
            ChangeStatus::NoChangeRequested,
            Some(b"root"),
            Some(b"root"),
            Some(b"root"),
            None,
        );
        assert!(!line.contains('\n'), "{line}");
        // GNU 9.4, measured under `fakeroot`: the shell-escape style breaks the
        // name into three quoted runs rather than escaping inside one, so the
        // substring to look for is not `two\nlines`. Checked against the real
        // thing rather than recalled — this is exactly the kind of detail
        // recall gets subtly wrong.
        assert_eq!(line, "ownership of 'two'$'\\n''lines' retained as root");
    }

    /// The upstream oddity, reproduced deliberately: a numeric user with a
    /// named group prints `to :daemon` and drops the uid it is about to set.
    /// Measured against GNU 9.4; a diagnostic that differs from GNU's is a
    /// diagnostic no existing script can parse.
    #[test]
    fn describe_reproduces_the_dropped_numeric_uid() {
        let mut s = spec("1234:staff");
        assert_eq!(s.uid, Some(1234));
        // `chown.c`'s fixup, applied by `resolve_source`.
        if s.user_name.is_none() && s.group_name.is_some() {
            s.user_name = Some(Vec::new());
        }
        assert_eq!(
            described(ChangeStatus::Succeeded, &s, ("root", "root")),
            "changed ownership of 'f' from root:root to :staff"
        );
    }

    // ---------------- user_group_str ----------------

    #[test]
    fn user_group_str_joins_only_what_exists() {
        assert_eq!(
            user_group_str(Some(b"a"), Some(b"b")),
            Some(b"a:b".to_vec())
        );
        assert_eq!(user_group_str(Some(b"a"), None), Some(b"a".to_vec()));
        assert_eq!(user_group_str(None, Some(b"b")), Some(b"b".to_vec()));
        assert_eq!(user_group_str(None, None), None);
        // The empty user name is *present*, which is the whole point of the
        // `chown.c` fixup: it makes the spec read `:GROUP`.
        assert_eq!(user_group_str(Some(b""), Some(b"b")), Some(b":b".to_vec()));
    }
}
