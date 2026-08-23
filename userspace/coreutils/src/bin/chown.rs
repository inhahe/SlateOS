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
//! Two rules keep that from happening, and both were once broken
//! (`known-issues.md` → `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`):
//!
//! 1. **`-R` does not follow symlinks unless asked.** It used to test
//!    `path.is_dir()`, which follows them, so `chown -R alice srv/` on a tree
//!    containing `srv/x -> /etc` walked into `/etc` and gave alice the lot.
//!    POSIX makes `-P` the default for exactly this reason; `-L` restores the
//!    old behaviour for anyone who genuinely wants it, and then only with
//!    symlink-loop protection.
//! 2. **A symlink met during traversal is changed, not its target.** The
//!    `chown(2)` call follows links, so `srv/x -> /etc/shadow` used to hand
//!    `/etc/shadow` to alice. Traversal now uses `lchown(2)`.
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

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf_os};
use pwdb::Db;
use std::ffi::{OsStr, OsString};

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

/// How much `chown` says about each file it visits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Verbosity {
    /// `-v`: a line for every file, changed or not.
    High,
    /// `-c`: a line only for a file whose ownership actually moved.
    ChangesOnly,
    /// The default: nothing.
    #[default]
    Off,
}

/// Which symlinks a recursive run may walk through. POSIX's `-H`/`-L`/`-P`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Traverse {
    /// `-P`, and the default: follow none. A symlink is a thing to change,
    /// never a door to walk through.
    #[default]
    Never,
    /// `-H`: follow a symlink named on the command line, but none found
    /// inside the tree.
    CommandLine,
    /// `-L`: follow every symlink. Needs loop protection, which is why the
    /// traversal carries a visited set.
    Always,
}

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
    /// `-h`: act on the link itself even for a command-line operand.
    no_dereference: bool,
    traverse: Traverse,
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
    let mut no_dereference = false;
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
            Opt::Short(b'h', _) | Opt::Long("no-dereference", _) => no_dereference = true,
            Opt::Long("dereference", _) => no_dereference = false,
            Opt::Short(b'H', _) => traverse = Traverse::CommandLine,
            Opt::Short(b'L', _) => traverse = Traverse::Always,
            Opt::Short(b'P', _) => traverse = Traverse::Never,
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
        no_dereference,
        traverse,
        verbosity,
        force_silent,
        preserve_root,
        from,
        source,
        files,
    })))
}

// ----------------------------------------------------------- symlink rules ---

/// Whether a **command-line operand** is resolved through a symlink
/// (`chown(2)`) or changed as the link itself (`lchown(2)`).
///
/// Pure, and compiled on every platform, because it is the decision the
/// symlink bug was made of and the walk around it only exists under
/// `cfg(unix)` — an untestable security rule is one that regresses quietly.
///
/// * Without `-R` the default is to follow: `chown alice link` is
///   conventionally about the file, and `-h` is how you say otherwise.
/// * With `-R` the default is `-P`, so a command-line symlink is *changed*
///   rather than walked into unless `-H` or `-L` overrides it.
fn follow_operand(recursive: bool, no_dereference: bool, traverse: Traverse) -> bool {
    if no_dereference {
        return false;
    }
    !recursive || traverse != Traverse::Never
}

/// Whether an entry found **inside** a traversal is followed.
///
/// Only `-L` says yes. `-H`'s exception is the command line, and it has
/// already been spent by the time this is asked; `-P` never follows. This is
/// the rule whose absence let `chown -R` walk out of the tree it was given.
fn follow_child(traverse: Traverse) -> bool {
    traverse == Traverse::Always
}

// ------------------------------------------------------------- owner specs ---

/// gnulib's `xstrtoul (s, nullptr, 10, &n, "")`, which is what decides whether
/// a spec that named no account is nonetheless a number.
///
/// It is `strtoul` with the whole string required, so it is emphatically **not**
/// `str::parse::<u32>()`, and every difference is observable. All measured:
///
/// | Input | GNU | Why |
/// |---|---|---|
/// | `" 1000"` | 1000 | `strtoul` skips leading whitespace |
/// | `"+1000"` | 1000 | and accepts a `+` sign |
/// | `"-0"` | `invalid user` | but not a `-` one, for an unsigned conversion |
/// | `"1000 "` | `invalid user` | the empty suffix list means "consume it all" |
/// | `"0x10"` | `invalid user` | base 10 is explicit, so no `0x` |
/// | `"007"` | 7 | and no octal either |
/// | `"4294967295"` | `invalid user` | see below |
///
/// The last is not an overflow: `(uid_t)-1` is POSIX's "leave this field
/// alone" sentinel, so accepting it would turn an explicit request into a
/// silent no-op. 4294967294 is fine.
fn numeric_id(text: &[u8]) -> Option<u32> {
    let mut rest = text;
    while let Some((first, tail)) = rest.split_first() {
        if first.is_ascii_whitespace() {
            rest = tail;
        } else {
            break;
        }
    }
    if let Some(tail) = rest.strip_prefix(b"+") {
        rest = tail;
    }
    if rest.is_empty() || !rest.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: u64 = 0;
    for digit in rest {
        value = value
            .checked_mul(10)?
            .checked_add(u64::from(*digit).saturating_sub(u64::from(b'0')))?;
        if value > u64::from(u32::MAX) {
            return None;
        }
    }
    let value = u32::try_from(value).ok()?;
    if value == u32::MAX { None } else { Some(value) }
}

/// What an `OWNER[:GROUP]` spec resolved to.
///
/// The two name fields are **not** the resolved account's name — they are the
/// text as typed, present only when a lookup succeeded on it. That asymmetry is
/// gnulib's and it is what the `-v` wording keys off; see the module docs.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Spec {
    /// The uid to set, or `None` for "leave it alone".
    uid: Option<u32>,
    gid: Option<u32>,
    /// The user field as typed, when it named an account that exists.
    user_name: Option<Vec<u8>>,
    /// The group field as typed, when it named a group that exists — or, for a
    /// trailing colon, the *login group's* name, which is the one case where
    /// this is a resolution rather than an echo.
    group_name: Option<Vec<u8>>,
}

/// gnulib's `parse_with_separator`: read a spec whose separator is already
/// located (or known absent).
///
/// # Errors
///
/// `"invalid spec"` for a trailing separator after something that is not an
/// account (there is no login group to look up), `"invalid user"` or
/// `"invalid group"` for a field that is neither a name nor a number.
fn parse_with_separator(spec: &[u8], sep: Option<usize>, db: &Db) -> Result<Spec, &'static str> {
    let (user, group): (Option<&[u8]>, Option<&[u8]>) = match sep {
        None => (Some(spec).filter(|s| !s.is_empty()), None),
        Some(at) => {
            let head = spec.get(..at).unwrap_or_default();
            let tail = spec.get(at.saturating_add(1)..).unwrap_or_default();
            (
                Some(head).filter(|s| !s.is_empty()),
                Some(tail).filter(|s| !s.is_empty()),
            )
        }
    };

    let mut out = Spec::default();
    if let Some(user) = user {
        // A leading `+` skips the lookup outright. That is how you name uid
        // 1000 on a system that also has an account literally called `1000`,
        // and it is why `+alice` is an error rather than a synonym for `alice`.
        let found = if user.first() == Some(&b'+') {
            None
        } else {
            db.user_by_name(user)
        };
        match found {
            None => {
                if sep.is_some() && group.is_none() {
                    // `1000:` — the trailing colon asks for "and the owner's
                    // login group", which is a property of an account. A number
                    // does not have one, so this is not a uid-only change; it
                    // is a spec that cannot be honoured.
                    return Err("invalid spec");
                }
                out.uid = Some(numeric_id(user).ok_or("invalid user")?);
                // `user_name` stays `None`: nothing was resolved, so there is
                // no name for a diagnostic to print instead of the number.
            }
            Some(account) => {
                out.uid = Some(account.uid);
                out.user_name = Some(user.to_vec());
                if group.is_none() && sep.is_some() {
                    out.gid = Some(account.gid);
                    out.group_name = Some(match db.group_by_gid(account.gid) {
                        Some(found) => found.name.clone(),
                        // A gid with no `/etc/group` line still has a name for
                        // reporting purposes: its number.
                        None => account.gid.to_string().into_bytes(),
                    });
                }
            }
        }
    }
    if let Some(group) = group {
        let found = if group.first() == Some(&b'+') {
            None
        } else {
            db.group_by_name(group)
        };
        match found {
            None => out.gid = Some(numeric_id(group).ok_or("invalid group")?),
            Some(entry) => {
                out.gid = Some(entry.gid);
                out.group_name = Some(group.to_vec());
            }
        }
    }
    Ok(out)
}

/// gnulib's `parse_user_spec_warn`. Returns the resolution and whether a `.`
/// had to be read as the separator.
///
/// The `.` fallback is a POSIX-compatible *extension*, and the order matters:
/// the colon-less reading is tried first and in full, so an account genuinely
/// called `a.b` is found rather than split. Only when that fails does the first
/// `.` become a separator, and then the caller warns.
///
/// # Errors
///
/// As [`parse_with_separator`]. When the dot fallback also fails, the error
/// reported is the *first* attempt's, which is why `chown a.b.c` says
/// `invalid user` about the whole spec rather than about `a`.
fn parse_user_spec(spec: &[u8], db: &Db) -> Result<(Spec, bool), &'static str> {
    let colon = spec.iter().position(|c| *c == b':');
    let first = parse_with_separator(spec, colon, db);
    let Err(error) = first else {
        return first.map(|s| (s, false));
    };
    if colon.is_some() {
        return Err(error);
    }
    let Some(dot) = spec.iter().position(|c| *c == b'.') else {
        return Err(error);
    };
    match parse_with_separator(spec, Some(dot), db) {
        Ok(out) => Ok((out, true)),
        Err(_) => Err(error),
    }
}

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

/// `chown-core.c`'s `uid_to_name`: the account's name, or the number when the
/// database does not know it. Used for the *old* ownership in a `-v` line,
/// which is why it always produces something.
fn uid_to_name(db: &Db, uid: u32) -> Vec<u8> {
    match db.user_by_uid(uid) {
        Some(found) => found.name.clone(),
        None => uid.to_string().into_bytes(),
    }
}

/// `gid_to_name`, as [`uid_to_name`].
fn gid_to_name(db: &Db, gid: u32) -> Vec<u8> {
    match db.group_by_gid(gid) {
        Some(found) => found.name.clone(),
        None => gid.to_string().into_bytes(),
    }
}

// -------------------------------------------------------------- reporting ---

/// `chown-core.c`'s `user_group_str`: `USER:GROUP`, or whichever half exists.
///
/// `None` when neither does, which is a distinct case rather than an empty
/// string — it selects a different sentence in [`describe_change`].
fn user_group_str(user: Option<&[u8]>, group: Option<&[u8]>) -> Option<Vec<u8>> {
    match (user, group) {
        (Some(user), Some(group)) => {
            let mut out = user.to_vec();
            out.push(b':');
            out.extend_from_slice(group);
            Some(out)
        }
        (Some(one), None) | (None, Some(one)) => Some(one.to_vec()),
        (None, None) => None,
    }
}

/// What happened to one file, as `chown-core.c`'s `enum Change_status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChangeStatus {
    Succeeded,
    Failed,
    /// The ownership was already what was asked for — or `--from` did not
    /// match, which upstream reports the same way and does not call an error.
    NoChangeRequested,
    /// `lchown` on a symlink was refused for lack of support. POSIX requires
    /// that this *not* be an error, so it is a fourth outcome rather than a
    /// failure: nothing changed, and nothing was wrong.
    NotApplied,
}

/// `chown-core.c`'s `describe_change`, verbatim in behaviour.
///
/// `user`/`group` are the *new* names — [`Spec`]'s, or the plain number when no
/// name was resolved — and `old_user`/`old_group` are the file's current ones,
/// or `None` when the file could not be stat'd at all.
///
/// Which of the four sentences is used turns on whether `user` is present, not
/// on what actually changed; that is the load-bearing subtlety and the module
/// docs explain what it is for.
fn describe_change(
    file: &OsStr,
    status: ChangeStatus,
    old_user: Option<&[u8]>,
    old_group: Option<&[u8]>,
    user: Option<&[u8]>,
    group: Option<&[u8]>,
) -> String {
    let mut spec = user_group_str(user, group);
    // The old spec names only the fields the new one does: `chown 1000 f`
    // reports `from root`, not `from root:root`, because the group is not part
    // of what was asked.
    let mut old_spec = user_group_str(user.and(old_user), group.and(old_group));

    let text =
        |value: &Option<Vec<u8>>| -> String { value.as_deref().map(name_text).unwrap_or_default() };

    let name = quoteaf_os(file);
    match status {
        // Names neither what was asked for nor what is there, because nothing
        // moved and nothing was wrong.
        ChangeStatus::NotApplied => {
            format!("neither symbolic link {name} nor referent has been changed")
        }
        ChangeStatus::Succeeded => {
            if user.is_some() {
                format!(
                    "changed ownership of {name} from {} to {}",
                    text(&old_spec),
                    text(&spec)
                )
            } else if group.is_some() {
                format!(
                    "changed group of {name} from {} to {}",
                    text(&old_spec),
                    text(&spec)
                )
            } else {
                format!("no change to ownership of {name}")
            }
        }
        ChangeStatus::Failed => {
            if old_spec.is_some() {
                if user.is_some() {
                    format!(
                        "failed to change ownership of {name} from {} to {}",
                        text(&old_spec),
                        text(&spec)
                    )
                } else if group.is_some() {
                    format!(
                        "failed to change group of {name} from {} to {}",
                        text(&old_spec),
                        text(&spec)
                    )
                } else {
                    format!("failed to change ownership of {name}")
                }
            } else {
                // No stat, so there is no "from". Upstream shifts the *new*
                // spec into the first slot rather than printing an empty one,
                // which is why `chown -v 1234 nosuch` says
                // `failed to change ownership of 'nosuch' to 1234`.
                old_spec = spec.take();
                if user.is_some() {
                    format!(
                        "failed to change ownership of {name} to {}",
                        text(&old_spec)
                    )
                } else if group.is_some() {
                    format!("failed to change group of {name} to {}", text(&old_spec))
                } else {
                    format!("failed to change ownership of {name}")
                }
            }
        }
        ChangeStatus::NoChangeRequested => {
            if user.is_some() {
                format!("ownership of {name} retained as {}", text(&old_spec))
            } else if group.is_some() {
                format!("group of {name} retained as {}", text(&old_spec))
            } else {
                format!("ownership of {name} retained")
            }
        }
    }
}

/// Render a name for a message. Names come from `/etc/passwd`, which is bytes,
/// and a byte that is not text must not become a raw control character in a
/// diagnostic — the same argument as for file names.
fn name_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.bytes().any(|b| b < 0x20 || b == 0x7f) => text.to_string(),
        _ => coreutils::quote::escape_unprintable(bytes),
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("chown: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{
        ChangeStatus, Request, Settings, Source, Spec, Verbosity, describe_change, follow_child,
        follow_operand, gid_to_name, help_text, parse_args, parse_user_spec, resolve_spec,
        uid_to_name,
    };
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, quote, quoteaf_os, quotef_os};
    use pwdb::Db;
    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::fs::{self, Metadata};
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    // libc-level chown/lchown — our POSIX layer provides both. `lchown` is what
    // makes rule 2 of the module docs enforceable: `chown` follows symlinks by
    // definition, so there is no flag that makes it safe during traversal.
    unsafe extern "C" {
        fn chown(path: *const u8, owner: u32, group: u32) -> i32;
        fn lchown(path: *const u8, owner: u32, group: u32) -> i32;
        /// The descriptor-based form, which is what makes [`restricted_chown`]
        /// possible: it names an inode we already hold rather than a path
        /// someone else could re-point.
        fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    }

    /// POSIX's "leave this field alone" sentinel for `chown(2)`: `(uid_t)-1`.
    ///
    /// Passing it is better than reading the current owner and passing that
    /// back: the read-then-write version has a window in which the file can be
    /// replaced, and it turns a no-op field into a real ownership write.
    const UNCHANGED: u32 = u32::MAX;

    /// Everything the walk needs that does not change from file to file.
    struct Job {
        settings: Settings,
        db: Db,
        /// The ownership to write, already resolved.
        spec: Spec,
        /// `--from`: change a file only if its current ids match. Each half is
        /// independently optional, and an absent half matches anything.
        required: (Option<u32>, Option<u32>),
        /// `(dev, ino)` of `/`, when `--preserve-root` and `-R` are both on.
        root_dev_ino: Option<(u64, u64)>,
        /// `(dev, ino)` of every directory already entered, so `-L` on a tree
        /// that links back to one of its own ancestors terminates instead of
        /// recursing until the stack runs out.
        seen: HashSet<(u64, u64)>,
        status: u8,
    }

    impl Job {
        /// Report a failure, unless `-f` said not to. The status moves either
        /// way: silence is about the message, not about the answer.
        fn fail(&mut self, message: &str) {
            if !self.settings.force_silent {
                eprintln!("chown: {message}");
            }
            self.status = 1;
        }
    }

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
                    eprintln!(
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
                    eprintln!("chown: {message}: {}", quote(&bytes));
                    1u8
                })?;
                if dotted {
                    // Upstream calls `error (0, …)` here — a warning, not a
                    // failure — and carries on with the dot reading.
                    eprintln!("chown: warning: '.' should be ':': {}", quote(&bytes));
                }
                Ok(spec)
            }
        }
    }

    /// Resolve `--from`, discarding the names: only the ids are compared.
    fn resolve_from(settings: &Settings, db: &Db) -> Result<(Option<u32>, Option<u32>), u8> {
        let Some(text) = &settings.from else {
            return Ok((None, None));
        };
        let bytes = os_bytes(text);
        let (spec, dotted) = parse_user_spec(&bytes, db).map_err(|message| {
            eprintln!("chown: {message}: {}", quote(&bytes));
            1u8
        })?;
        if dotted {
            eprintln!("chown: warning: '.' should be ':': {}", quote(&bytes));
        }
        Ok((spec.uid, spec.gid))
    }

    pub fn main() -> ExitCode {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Help) => {
                print!("{}", help_text());
                return ExitCode::SUCCESS;
            }
            Ok(Request::Version) => {
                println!("chown (SlateOS coreutils) 0.1.0");
                return ExitCode::SUCCESS;
            }
            Ok(Request::Run(settings)) => *settings,
            Err(e) => {
                eprintln!("chown: {e}");
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
                    eprintln!(
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

        let mut job = Job {
            settings,
            db,
            spec,
            required,
            root_dev_ino,
            seen: HashSet::new(),
            status: 0,
        };

        let follow = follow_operand(
            job.settings.recursive,
            job.settings.no_dereference,
            job.settings.traverse,
        );
        for file in job.settings.files.clone() {
            visit(&mut job, &PathBuf::from(&file), follow);
        }

        // A closed stdout must not pass for success when `-v` had things to say.
        if io::stdout().flush().is_err() {
            job.status = 1;
        }
        ExitCode::from(job.status)
    }

    /// Stat one path the way `follow` asks, distinguishing the two failures the
    /// way GNU does.
    ///
    /// The link is stat'd first even when following, so that a dangling symlink
    /// reports `cannot dereference` rather than `cannot access` — those are
    /// different problems and a script that retries on one should not retry on
    /// the other.
    fn stat(job: &mut Job, path: &Path, follow: bool) -> Option<Metadata> {
        let link = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => {
                job.fail(&format!(
                    "cannot access {}: {}",
                    quoteaf_os(path),
                    strerror(&e)
                ));
                return None;
            }
        };
        if !follow || !link.file_type().is_symlink() {
            return Some(link);
        }
        match fs::metadata(path) {
            Ok(meta) => Some(meta),
            Err(e) => {
                job.fail(&format!(
                    "cannot dereference {}: {}",
                    quoteaf_os(path),
                    strerror(&e)
                ));
                None
            }
        }
    }

    /// Apply the ownership to one path, and — under `-R`, and only for a real
    /// directory — to everything beneath it.
    ///
    /// Errors are reported and recorded rather than returned, because a walk
    /// that stops at the first failure leaves the caller with a tree in an
    /// unknown state.
    fn visit(job: &mut Job, path: &Path, follow: bool) {
        let Some(meta) = stat(job, path, follow) else {
            // The file could not be looked at, so there is no "from" to report;
            // `describe_change` has a shape for exactly that.
            report(job, path, ChangeStatus::Failed, None);
            return;
        };

        if let Some(root) = job.root_dev_ino
            && meta.is_dir()
            && (meta.dev(), meta.ino()) == root
        {
            refuse_root(job, path);
            return;
        }

        // A symlink we are not following is a leaf: change the link, do not
        // look at what is on the other side of it. `is_dir()` on the old code
        // answered about the *target*, which is how `-R` escaped the tree.
        if job.settings.recursive && meta.is_dir() {
            if !job.seen.insert((meta.dev(), meta.ino())) {
                // Only reachable under `-L`, where a symlink can point at an
                // ancestor. Without this the recursion is unbounded.
                job.fail(&format!(
                    "{}: directory loop detected; not descending again",
                    quotef_os(path)
                ));
                return;
            }

            let entries = match fs::read_dir(path) {
                Ok(entries) => entries,
                Err(e) => {
                    // Upstream's `FTS_DNR`: the directory is *not* changed
                    // either. It reaches the reporting code with `ok` already
                    // false, so `-v` calls it a failure and nothing is written.
                    job.fail(&format!(
                        "cannot read directory {}: {}",
                        quoteaf_os(path),
                        strerror(&e)
                    ));
                    report(job, path, ChangeStatus::Failed, Some(&meta));
                    return;
                }
            };
            let child_follow = follow_child(job.settings.traverse);
            for entry in entries {
                match entry {
                    Ok(entry) => visit(job, &entry.path(), child_follow),
                    Err(e) => job.fail(&format!("{}: {}", quotef_os(path), strerror(&e))),
                }
            }
        }

        // A directory is changed *after* its children, not before. That is
        // GNU's order — it acts on `FTS_DP`, the post-order visit, and returns
        // early from the pre-order `FTS_D` whenever `-R` is on — and the order
        // is load-bearing rather than cosmetic: handing a directory to another
        // owner first can cost us the search permission we still need to reach
        // what is inside it. It is also what `-v` output looks like, so a
        // script reading that output sees the children first.
        apply(job, path, follow, &meta);
    }

    /// The `--preserve-root` refusal, which is two sentences upstream and is
    /// kept as two here so `-f` silences them together — a lone
    /// "use --no-preserve-root" would be baffling.
    fn refuse_root(job: &mut Job, path: &Path) {
        job.fail(&format!(
            "it is dangerous to operate recursively on {}",
            quoteaf_os(path)
        ));
        job.fail("use --no-preserve-root to override this failsafe");
    }

    /// `chown(2)` or `lchown(2)` on a NUL-terminated path.
    ///
    /// `follow` decides between the two; there is no third option, and getting
    /// it wrong is the whole bug this file exists to not have.
    fn chown_path(c_path: &[u8], uid: u32, gid: u32, follow: bool) -> io::Result<()> {
        // SAFETY: `c_path` is NUL-terminated and contains no interior NUL (the
        // caller checked), and outlives the call. Both functions come from the
        // POSIX layer and take a borrowed C string they do not retain.
        let ret = unsafe {
            if follow {
                chown(c_path.as_ptr(), uid, gid)
            } else {
                lchown(c_path.as_ptr(), uid, gid)
            }
        };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// What [`restricted_chown`] managed to do.
    enum Restricted {
        /// The descriptor was chowned; nothing further to do.
        Done,
        /// Not worth protecting, or not openable in a way that would help.
        /// Fall back to changing the file by name.
        ByName,
        /// The open, the re-stat, or the `fchown` failed.
        Failed(io::Error),
        /// The file changed identity under us, or stopped matching `--from`
        /// between the two stats. Upstream gives no diagnostic here — its
        /// source carries a FIXME asking whether it should — but still fails
        /// the run.
        Excluded,
    }

    /// `chown-core.c`'s `restricted_chown`: change the file through an open
    /// descriptor rather than by name.
    ///
    /// Reachable only with `--from` *and* symlink-following, which is exactly
    /// the combination that is attackable. In that combination we stat a file,
    /// decide from the result that its owner matches `--from`, and would then
    /// change it *by name*. Anyone who can write the containing directory can
    /// replace the file with a symlink inside that window and have us change
    /// the ownership of its target instead — a file they could not otherwise
    /// touch, chowned with our privileges.
    ///
    /// Opening the file, checking that the descriptor still refers to the inode
    /// we stat'd, and changing the *descriptor* closes the window: a descriptor
    /// cannot be redirected once it is open.
    ///
    /// Without `--from` there is nothing to protect — the decision to change
    /// the file did not depend on reading it — so that case is left by name,
    /// as upstream leaves it.
    fn restricted_chown(
        path: &Path,
        orig: &Metadata,
        uid: u32,
        gid: u32,
        required: (Option<u32>, Option<u32>),
    ) -> Restricted {
        if required.0.is_none() && required.1.is_none() {
            return Restricted::ByName;
        }
        let is_regular = orig.file_type().is_file();
        if !is_regular && !orig.is_dir() {
            // Opening a FIFO would block for a writer, and opening a device can
            // have side effects on the device. Upstream declines both rather
            // than risk either, and takes the race.
            return Restricted::ByName;
        }

        let opened = fs::File::open(path).or_else(|e| {
            // A file we may not read may still be one we may write, and either
            // descriptor pins the inode equally well.
            if e.kind() == io::ErrorKind::PermissionDenied && is_regular {
                fs::OpenOptions::new().write(true).open(path)
            } else {
                Err(e)
            }
        });
        let file = match opened {
            Ok(file) => file,
            // Not openable at all: no protection is available, so do what we
            // would have done anyway rather than refuse the whole operation.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Restricted::ByName,
            Err(e) => return Restricted::Failed(e),
        };

        let now = match file.metadata() {
            Ok(now) => now,
            Err(e) => return Restricted::Failed(e),
        };
        if (now.dev(), now.ino()) != (orig.dev(), orig.ino()) {
            return Restricted::Excluded;
        }
        // Re-checked against the descriptor's own stat, not the earlier one:
        // the earlier one is the answer we are refusing to trust.
        if !(required.0.is_none_or(|uid| uid == now.uid())
            && required.1.is_none_or(|gid| gid == now.gid()))
        {
            return Restricted::Excluded;
        }

        // SAFETY: `file` owns the descriptor and is still alive here, so the
        // number is valid for the duration of the call.
        if unsafe { fchown(file.as_raw_fd(), uid, gid) } == 0 {
            Restricted::Done
        } else {
            Restricted::Failed(io::Error::last_os_error())
        }
    }

    /// Change one file's owner, honouring `--from`, and report as `-v`/`-c` ask.
    fn apply(job: &mut Job, path: &Path, follow: bool, meta: &Metadata) {
        let (want_uid, want_gid) = (job.spec.uid, job.spec.gid);
        let matches_from = job.required.0.is_none_or(|uid| uid == meta.uid())
            && job.required.1.is_none_or(|gid| gid == meta.gid());

        if !matches_from {
            // Upstream does not treat this as an error, and neither does the
            // exit status: the file simply was not one of the ones asked for.
            report(job, path, ChangeStatus::NoChangeRequested, Some(meta));
            return;
        }

        // Paths are bytes. An older version went through `to_str()` and refused
        // anything that was not UTF-8, which on this OS is a legal filename
        // (`design.txt`: every byte but `/` and NUL). A NUL cannot arrive from
        // argv or a directory read — both are NUL-terminated at the source — so
        // this is a guard against a future caller, not against today's.
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&0) {
            job.fail(&format!("{}: path contains a NUL byte", quotef_os(path)));
            report(job, path, ChangeStatus::Failed, Some(meta));
            return;
        }
        let mut c_path: Vec<u8> = Vec::with_capacity(bytes.len().saturating_add(1));
        c_path.extend_from_slice(bytes);
        c_path.push(0);

        let uid = want_uid.unwrap_or(UNCHANGED);
        let gid = want_gid.unwrap_or(UNCHANGED);

        // Upstream's `symlink_changed`: the one outcome that is neither success
        // nor failure. POSIX allows a system to refuse to change a symlink's
        // own ownership and requires that refusal not be an error.
        let mut symlink_changed = true;
        // `Err(None)` is a failure with no message of its own.
        let result: Result<(), Option<io::Error>> = if follow {
            match restricted_chown(path, meta, uid, gid, job.required) {
                Restricted::Done => Ok(()),
                Restricted::ByName => chown_path(&c_path, uid, gid, true).map_err(Some),
                Restricted::Failed(e) => Err(Some(e)),
                Restricted::Excluded => Err(None),
            }
        } else {
            match chown_path(&c_path, uid, gid, false) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                    symlink_changed = false;
                    Ok(())
                }
                Err(e) => Err(Some(e)),
            }
        };

        if let Err(error) = result {
            if let Some(error) = error {
                // The message names which half was asked for, as upstream does:
                // a `chgrp`-shaped invocation should not report an ownership
                // failure.
                let what = if want_uid.is_some() {
                    "changing ownership of"
                } else {
                    "changing group of"
                };
                job.fail(&format!(
                    "{what} {}: {}",
                    quoteaf_os(path),
                    strerror(&error)
                ));
            } else {
                job.status = 1;
            }
            report(job, path, ChangeStatus::Failed, Some(meta));
            return;
        }

        if !symlink_changed {
            report(job, path, ChangeStatus::NotApplied, Some(meta));
            return;
        }

        // "Changed" means the ids actually moved, not that a change was asked
        // for: `chown -c 1000 f` on a file already owned by 1000 prints nothing.
        let changed = !(want_uid.is_none_or(|uid| uid == meta.uid())
            && want_gid.is_none_or(|gid| gid == meta.gid()));
        let status = if changed {
            ChangeStatus::Succeeded
        } else {
            ChangeStatus::NoChangeRequested
        };
        report(job, path, status, Some(meta));
    }

    /// Print a `-v`/`-c` line, if this run wants one for this outcome.
    fn report(job: &mut Job, path: &Path, status: ChangeStatus, meta: Option<&Metadata>) {
        let changed = status == ChangeStatus::Succeeded;
        if job.settings.verbosity == Verbosity::Off
            || (!changed && job.settings.verbosity != Verbosity::High)
        {
            return;
        }
        let old_user = meta.map(|m| uid_to_name(&job.db, m.uid()));
        let old_group = meta.map(|m| gid_to_name(&job.db, m.gid()));
        // The new name, or the plain number when nothing was resolved — this is
        // `chown-core.c`'s `chopt->user_name ? … : uid_to_str (uid)`.
        let user = job
            .spec
            .user_name
            .clone()
            .or_else(|| job.spec.uid.map(|uid| uid.to_string().into_bytes()));
        let group = job
            .spec
            .group_name
            .clone()
            .or_else(|| job.spec.gid.map(|gid| gid.to_string().into_bytes()));
        println!(
            "{}",
            describe_change(
                path.as_os_str(),
                status,
                old_user.as_deref(),
                old_group.as_deref(),
                user.as_deref(),
                group.as_deref(),
            )
        );
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    imp::main()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

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
        assert!(run(&["-h", "1000", "link"]).no_dereference);
        assert!(run(&["--no-dereference", "1000", "link"]).no_dereference);
    }

    #[test]
    fn parse_dereference_overrides_h() {
        // Last one wins, as with GNU.
        assert!(!run(&["-h", "--dereference", "1000", "link"]).no_dereference);
    }

    #[test]
    fn parse_traverse_defaults_to_never() {
        // -P is the default: without it, `-R` on a tree containing a symlink
        // to /etc would walk into /etc.
        assert_eq!(run(&["-R", "1000", "d"]).traverse, Traverse::Never);
    }

    #[test]
    fn parse_traverse_flags() {
        assert_eq!(
            run(&["-R", "-H", "1000", "d"]).traverse,
            Traverse::CommandLine
        );
        assert_eq!(run(&["-R", "-L", "1000", "d"]).traverse, Traverse::Always);
        assert_eq!(
            run(&["-R", "-L", "-P", "1000", "d"]).traverse,
            Traverse::Never
        );
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
        assert!(!a.no_dereference);
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

    // ---------------- symlink policy ----------------
    //
    // This is the security rule, stated once. `chown -R` used to test
    // `path.is_dir()`, which resolves symlinks, and then call `chown(2)`,
    // which also resolves them -- so a symlink inside the tree was both a
    // door out of it and a way to hand its target away. known-issues.md ->
    // B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING.

    #[test]
    fn follow_operand_without_r_dereferences_by_default() {
        // `chown alice link` is about the file, not the link.
        assert!(follow_operand(false, false, Traverse::Never));
    }

    #[test]
    fn follow_operand_h_wins_everywhere() {
        for recursive in [false, true] {
            for t in [Traverse::Never, Traverse::CommandLine, Traverse::Always] {
                assert!(!follow_operand(recursive, true, t), "{recursive} {t:?}");
            }
        }
    }

    #[test]
    fn follow_operand_with_r_defaults_to_not_following() {
        // The POSIX default is -P. This single `false` is what stops
        // `chown -R alice srv/` from walking into `/etc` via `srv/x -> /etc`.
        assert!(!follow_operand(true, false, Traverse::Never));
    }

    #[test]
    fn follow_operand_h_and_l_opt_back_in() {
        assert!(follow_operand(true, false, Traverse::CommandLine));
        assert!(follow_operand(true, false, Traverse::Always));
    }

    #[test]
    fn follow_child_only_under_dash_l() {
        assert!(!follow_child(Traverse::Never));
        // -H's exception is the command line only, and it is already spent.
        assert!(!follow_child(Traverse::CommandLine));
        assert!(follow_child(Traverse::Always));
    }

    #[test]
    fn default_args_never_follow_a_symlink_during_recursion() {
        // End to end through the parser: the plain, everyday invocation.
        let a = run(&["-R", "1000", "dir"]);
        assert!(!follow_operand(a.recursive, a.no_dereference, a.traverse));
        assert!(!follow_child(a.traverse));
    }

    // ---------------- numeric_id ----------------

    #[test]
    fn numeric_id_is_strtoul_not_parse() {
        assert_eq!(numeric_id(b"1000"), Some(1000));
        assert_eq!(numeric_id(b"007"), Some(7));
        assert_eq!(numeric_id(b" 1000"), Some(1000));
        assert_eq!(numeric_id(b"\t\n 1000"), Some(1000));
        assert_eq!(numeric_id(b"+1000"), Some(1000));
        assert_eq!(numeric_id(b"1000 "), None);
        assert_eq!(numeric_id(b"-0"), None);
        assert_eq!(numeric_id(b"-1"), None);
        assert_eq!(numeric_id(b"++1"), None);
        assert_eq!(numeric_id(b"0x10"), None);
        assert_eq!(numeric_id(b"1e3"), None);
        assert_eq!(numeric_id(b""), None);
        assert_eq!(numeric_id(b"+"), None);
    }

    /// `(uid_t)-1` is the kernel's "leave this alone", so asking for it would be
    /// indistinguishable from asking for nothing. GNU refuses it; one less is
    /// fine. Both measured.
    #[test]
    fn numeric_id_refuses_the_unchanged_sentinel() {
        assert_eq!(numeric_id(b"4294967294"), Some(4_294_967_294));
        assert_eq!(numeric_id(b"4294967295"), None);
        assert_eq!(numeric_id(b"4294967296"), None);
        assert_eq!(numeric_id(b"99999999999999999999"), None);
    }

    // ---------------- parse_user_spec ----------------

    fn spec(text: &str) -> Spec {
        parse_user_spec(text.as_bytes(), &db()).unwrap().0
    }

    fn spec_err(text: &str) -> &'static str {
        parse_user_spec(text.as_bytes(), &db()).unwrap_err()
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

    #[test]
    fn spec_name_resolves_through_the_account_database() {
        // This is the capability the old parser declared missing while `ls`
        // was already using it.
        let a = spec("alice");
        assert_eq!(a.uid, Some(1000));
        assert_eq!(a.gid, None);
        assert_eq!(a.user_name.as_deref(), Some(&b"alice"[..]));
        assert_eq!(a.group_name, None);
    }

    #[test]
    fn spec_name_and_group() {
        let a = spec("alice:staff");
        assert_eq!((a.uid, a.gid), (Some(1000), Some(2000)));
        assert_eq!(a.user_name.as_deref(), Some(&b"alice"[..]));
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    /// A trailing colon means "and the owner's login group".
    #[test]
    fn spec_trailing_colon_takes_the_login_group() {
        let a = spec("bob:");
        assert_eq!((a.uid, a.gid), (Some(1001), Some(2000)));
        // The group *name*, resolved from the gid rather than echoed.
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    /// ...and a gid with no `/etc/group` line still reports as its number.
    #[test]
    fn spec_login_group_without_a_group_line_falls_back_to_digits() {
        let db = Db::from_bytes(b"carol:x:1:12345::/:/bin/sh\n", b"");
        let (a, _) = parse_user_spec(b"carol:", &db).unwrap();
        assert_eq!(a.gid, Some(12345));
        assert_eq!(a.group_name.as_deref(), Some(&b"12345"[..]));
    }

    /// A number has no login group, so a trailing colon after one is not a
    /// uid-only change — it is a spec that cannot be honoured. Measured:
    /// `chown 1000: f` is `invalid spec`, and this is the only place that
    /// message comes from.
    #[test]
    fn spec_trailing_colon_after_a_number_is_invalid_spec() {
        assert_eq!(spec_err("1234:"), "invalid spec");
        assert_eq!(spec_err("+1000:"), "invalid spec");
    }

    #[test]
    fn spec_group_only() {
        let a = spec(":staff");
        assert_eq!((a.uid, a.gid), (None, Some(2000)));
        assert_eq!(a.user_name, None);
        assert_eq!(a.group_name.as_deref(), Some(&b"staff"[..]));
    }

    #[test]
    fn spec_empty_and_bare_colon_change_nothing() {
        for text in ["", ":"] {
            let a = spec(text);
            assert_eq!((a.uid, a.gid), (None, None), "{text}");
            assert_eq!((a.user_name, a.group_name), (None, None), "{text}");
        }
    }

    /// A number resolves but contributes no *name*, which is what makes
    /// `chown :0` and `chown :root` print different lines.
    #[test]
    fn spec_numbers_resolve_without_names() {
        let a = spec("1234:5678");
        assert_eq!((a.uid, a.gid), (Some(1234), Some(5678)));
        assert_eq!((a.user_name, a.group_name), (None, None));
        let b = spec(":0");
        assert_eq!(b.gid, Some(0));
        assert_eq!(b.group_name, None);
        let c = spec(":root");
        assert_eq!(c.gid, Some(0));
        assert_eq!(c.group_name.as_deref(), Some(&b"root"[..]));
    }

    /// `+` skips the lookup, which is the only way to mean uid 1000 on a system
    /// that also has an account named `1000` — and this database has one.
    #[test]
    fn spec_plus_skips_the_name_lookup() {
        assert_eq!(spec("1000").uid, Some(4000));
        assert_eq!(spec("1000").user_name.as_deref(), Some(&b"1000"[..]));
        assert_eq!(spec("+1000").uid, Some(1000));
        assert_eq!(spec("+1000").user_name, None);
        assert_eq!(spec_err("+alice"), "invalid user");
    }

    #[test]
    fn spec_unknown_names_are_rejected() {
        assert_eq!(spec_err("nosuchuser"), "invalid user");
        assert_eq!(spec_err("alice:nosuchgroup"), "invalid group");
        assert_eq!(spec_err("nosuchuser:staff"), "invalid user");
    }

    /// The `.` separator is a compatible extension, tried only after the
    /// colon-less reading fails — so an account genuinely called `a.b` wins.
    #[test]
    fn spec_dot_separator_is_the_fallback_not_the_rule() {
        let (dotted, warned) = parse_user_spec(b"alice.staff", &db()).unwrap();
        assert!(warned);
        assert_eq!((dotted.uid, dotted.gid), (Some(1000), Some(2000)));

        let (literal, warned) = parse_user_spec(b"a.b", &db()).unwrap();
        assert!(!warned, "an account called a.b must not be split");
        assert_eq!(literal.uid, Some(5000));
    }

    /// With a colon present the dot is just a character, and a spec that fails
    /// both readings reports the *first* attempt's error.
    #[test]
    fn spec_dot_fallback_is_skipped_when_a_colon_exists() {
        assert_eq!(spec_err("nosuch.user:staff"), "invalid user");
        assert_eq!(spec_err("a.b.c"), "invalid user");
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
