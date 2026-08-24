//! id — print user and group information for each specified USER, or (when
//! USER is omitted) for the current process.
//!
//! ```text
//! Usage: id [OPTION]... [USER]...
//! ```
//!
//! # What this file used to be
//!
//! Three flags (`-u`, `-g`, `-n`), a `main` that read `env::args()` into
//! `Vec<String>`, and four defects that only opening the file revealed. All
//! four were user-visible, and the last two mean that anything which parsed
//! `id`'s output on this system was parsing something that no `id` anywhere
//! else produces:
//!
//! 1. **`-n` printed a number.** It was accepted and then ignored, with the
//!    module docs recording that as "not yet supported". `id -un` is the
//!    idiomatic spelling of "who am I", so this answered the question with the
//!    wrong kind of thing rather than with an error.
//! 2. **A USER operand was silently discarded.** `parse_args` looked only at
//!    arguments starting with `-`; `id alice` therefore printed *your own* ids
//!    and exited 0. A wrong answer with a success status is worse than a
//!    failure, because a script cannot tell.
//! 3. **The default line had the fields in the wrong order** — `uid= euid=
//!    gid= egid=` where GNU emits `uid= gid= euid= egid=`.
//! 4. **There was no `groups=` field at all**, which is most of what the
//!    default line is *for*.
//!
//! The rewrite is a port of GNU's `src/id.c`, `src/group-list.c` and gnulib's
//! `lib/mgetgroups.c`, on top of [`coreutils::userspec`] (the account-operand
//! grammar, shared with `chown`) and [`pwdb`] (the account database).
//!
//! # `id alice:staff` is `no such user`, and that is correct
//!
//! `id` resolves its operand with the *same* gnulib function `chown` uses,
//! `parse_user_spec` — but with the group out-parameter passed as `NULL`, and
//! that nullness is what switches the group half of the grammar off:
//!
//! ```c
//! char const *colon = gid ? strchr (spec, ':') : NULL;
//! ```
//!
//! With no separator there is no group, and no `.`-instead-of-`:` fallback
//! either, so the whole operand is looked up as one account name. Measured
//! against GNU 9.4 on a machine where `id root` works:
//!
//! ```text
//! $ id root:root      id: ‘root:root’: no such user
//! $ id root.root      id: ‘root.root’: no such user
//! $ id root:          id: ‘root:’: no such user
//! ```
//!
//! A leading `+` still skips the name lookup, so `id +0` is uid 0 by number
//! even on a system with an account literally called `0`. See
//! [`coreutils::userspec::parse_user_only`].
//!
//! # The group list is two different lists
//!
//! `groups=` and `-G` both come from gnulib's `mgetgroups`, which has two
//! completely separate paths depending on whether a *username* is in hand:
//!
//! * **With a username** (an operand) it is `getgrouplist(name, pw_gid)`: the
//!   login group first, then every `/etc/group` line naming the user. This is
//!   a pure database query, so it lives in [`pwdb::Db::group_list`] where the
//!   POSIX layer can reach it too.
//! * **Without one** (the current process) it is `getgroups(2)` with the
//!   effective gid pushed on the front — the *kernel's* answer, not the
//!   database's.
//!
//! That distinction is worth keeping rather than smoothing over. Our
//! `getgroups(2)` currently returns 0 supplementary groups
//! (`posix/src/unistd.rs`), so `id` with no operand prints just the effective
//! gid, while `id $USER` prints everything `/etc/group` says. That is not an
//! inconsistency to paper over by reading the file in both cases: the file
//! says what you *may* be granted, `getgroups` says what you *were* granted,
//! and reporting the former as the latter would have `id` vouch for privileges
//! the kernel has not handed out. gnulib takes the same view — it only
//! synthesises a list when `getgroups` fails with `ENOSYS`, never when it
//! succeeds with none.
//!
//! # `-G` filters and `groups=` does not
//!
//! Two lists, two rules, both measured:
//!
//! ```text
//! $ id 1000
//! uid=1000(inhahe) gid=1000(inhahe) groups=1000(inhahe),4(adm),24(cdrom),…
//! $ id -G 1000
//! 1000 4 24 …
//! ```
//!
//! They look alike because the login group happens to lead both, but they are
//! produced by different code: `print_full_info` prints the list verbatim,
//! while `print_group_list` prints `rgid`, then `egid` if it differs, and then
//! only those group ids *equal to neither*. Feed it a process whose effective
//! and real gids differ and the two diverge.
//!
//! # `-Z` is an error here, so `just_context` does not exist
//!
//! Upstream carries a `just_context` flag through four branches. On any kernel
//! without SELinux — ours — `-Z` fails during option parsing:
//!
//! ```text
//! $ id -Z      id: --context (-Z) works only on an SELinux-enabled kernel
//! ```
//!
//! so the flag can never become true and those branches are unreachable.
//! Modelling it as an error at the point of parsing removes them, along with
//! upstream's `cannot print security context when user specified` check, which
//! is likewise unreachable. If SELinux ever arrives, the flag comes back.
//!
//! # One upstream artefact deliberately not reproduced
//!
//! `id ''` prints, on GNU 9.4:
//!
//! ```text
//! id: ‘’: no such user: No such file or directory
//! ```
//!
//! The trailing clause is stale `errno`: id.c skips `parse_user_spec` entirely
//! for an empty spec (`if (*spec)`) and then reports the failure with
//! `error (0, errno, …)`, so whatever last set `errno` — locale setup, here —
//! is appended. It is not a fact about the operand, it is not reproducible
//! across builds, and a script matching on it would be matching noise. We
//! print `id: ‘’: no such user`.
//!
//! # Non-unix hosts
//!
//! The id-fetching syscalls are unix-only, so `main` is; every formatting and
//! parsing function above it is pure and compiled — and unit-tested —
//! everywhere, including the Windows host `cargo test --workspace` runs on.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::userspec::parse_user_only;
use pwdb::Db;
use std::ffi::OsString;

/// Measured: `id --zzz; echo $?` prints 1.
const ID: Program = Program::new("id", 1);

/// GNU `id`'s `getopt_long` short string, exactly.
const SHORT_OPTIONS: &str = "agnruzGZ";

/// GNU `id`'s `longopts[]`, in its declaration order — which is user-visible,
/// because an ambiguous prefix lists its candidates in table order:
///
/// ```text
/// $ id --g
/// id: option '--g' is ambiguous; possibilities: '--group' '--groups'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("context", Takes::Nothing),
    ("group", Takes::Nothing),
    ("groups", Takes::Nothing),
    ("name", Takes::Nothing),
    ("real", Takes::Nothing),
    ("user", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

const NO_SELINUX: &str = "--context (-Z) works only on an SELinux-enabled kernel";

/// Which of the four mutually-exclusive reports the command line asked for.
///
/// Upstream spells this as four independent booleans and then rejects the
/// combinations, which is why the "more than one choice" error counts them.
/// Here the count happens once, in [`parse_args`], and the rest of the program
/// sees a single answer.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Format {
    /// The default line: `uid=… gid=… [euid=…] [egid=…] groups=…`.
    #[default]
    Full,
    /// `-u`.
    User,
    /// `-g`.
    Group,
    /// `-G`.
    GroupList,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Settings {
    format: Format,
    /// `-n`: print names rather than numbers, for `-u`, `-g` and `-G`.
    use_name: bool,
    /// `-r`: print the real id rather than the effective one, same three.
    use_real: bool,
    /// `-z`: NUL between entries instead of a space, and NUL instead of the
    /// trailing newline.
    zero: bool,
    /// The USER operands, unresolved: resolving needs the account database.
    users: Vec<OsString>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

/// The four ids a single report is about.
///
/// For a USER operand all four come from that account's passwd line, so
/// `ruid == euid` and `rgid == egid`; for the current process they are the
/// four getters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Ids {
    ruid: u32,
    euid: u32,
    rgid: u32,
    egid: u32,
}

/// What a report produced: bytes for stdout, sentences for stderr, and whether
/// anything went wrong.
///
/// Names come out of the account database as bytes and may be any byte but `/`
/// and NUL, so stdout is assembled as `Vec<u8>` rather than `String`. Errors
/// are collected rather than printed so that the report functions stay pure
/// and testable — upstream's are not, which is why upstream's `ok` is a file
/// static.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Output {
    out: Vec<u8>,
    errors: Vec<String>,
    ok: bool,
}

impl Output {
    fn new() -> Self {
        Output {
            out: Vec::new(),
            errors: Vec::new(),
            ok: true,
        }
    }

    /// Record a diagnostic and remember that the exit status is now 1.
    ///
    /// Upstream's `ok &= false` — the run continues to the next operand, which
    /// is why `id -u root nosuchuser` prints `0` *and* exits 1.
    fn fail(&mut self, message: String) {
        self.errors.push(message);
        self.ok = false;
    }

    fn push_number(&mut self, value: u32) {
        self.out.extend_from_slice(value.to_string().as_bytes());
    }

    /// `(name)`, the parenthesised half of `uid=0(root)`.
    fn push_parenthesised(&mut self, name: &[u8]) {
        self.out.push(b'(');
        self.out.extend_from_slice(name);
        self.out.push(b')');
    }
}

fn help_text() -> String {
    "\
Usage: id [OPTION]... [USER]...
Print user and group information for each specified USER,
or (when USER omitted) for the current process.

  -a             ignore, for compatibility with other versions
  -Z, --context  print only the security context of the process
  -g, --group    print only the effective group ID
  -G, --groups   print all group IDs
  -n, --name     print a name instead of a number, for -ugG
  -r, --real     print the real ID instead of the effective ID, with -ugG
  -u, --user     print only the effective user ID
  -z, --zero     delimit entries with NUL characters, not whitespace;
                   not permitted in default format
      --help        display this help and exit
      --version     output version information and exit

Without any OPTION, print some useful set of identified information.
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse id's argv.
///
/// The four validation errors are checked in a fixed order, and the order is
/// observable because more than one can apply at once. All measured:
///
/// | Command line | Message |
/// |---|---|
/// | `id -zZ` | `--context (-Z) works only on an SELinux-enabled kernel` |
/// | `id -ugG` | `cannot print "only" of more than one choice` |
/// | `id -nz` | `cannot print only names or real IDs in default format` |
/// | `id -z` | `option --zero not permitted in default format` |
///
/// `-Z` comes first because upstream rejects it inside the option loop rather
/// than after it, which is also why `id -Z root` reports the kernel rather
/// than complaining about the operand.
///
/// None of these carry a `Try 'id --help'` referral — they are `error
/// (EXIT_FAILURE, …)` upstream, not `usage (EXIT_FAILURE)`. Unknown and
/// ambiguous options, which getopt itself reports, do carry it.
///
/// # Errors
///
/// An unknown, ambiguous or unsupported option, or a combination of flags that
/// asks for two reports at once.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut just_user = false;
    let mut just_group = false;
    let mut just_group_list = false;
    let mut use_name = false;
    let mut use_real = false;
    let mut zero = false;
    let mut users: Vec<OsString> = Vec::new();

    for item in ID.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => users.push(name.clone()),
            // `-a` is accepted and ignored, for compatibility with SVR4 id.
            Opt::Short(b'a', _) => {}
            Opt::Short(b'Z', _) | Opt::Long("context", _) => {
                return Err(ID.usage(NO_SELINUX.to_string()));
            }
            Opt::Short(b'u', _) | Opt::Long("user", _) => just_user = true,
            Opt::Short(b'g', _) | Opt::Long("group", _) => just_group = true,
            Opt::Short(b'G', _) | Opt::Long("groups", _) => just_group_list = true,
            Opt::Short(b'n', _) | Opt::Long("name", _) => use_name = true,
            Opt::Short(b'r', _) | Opt::Long("real", _) => use_real = true,
            Opt::Short(b'z', _) | Opt::Long("zero", _) => zero = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: every table entry is handled above. Refusing rather
            // than ignoring, so an option added to a table without a handler
            // fails loudly instead of silently doing nothing.
            Opt::Long(other, _) => {
                return Err(ID.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(ID.invalid_option(other)),
        }
    }

    // Counting the *distinct* choices, not the options seen: `id -u -u` is one
    // choice and is legal. Measured.
    let choices = usize::from(just_user) + usize::from(just_group) + usize::from(just_group_list);
    if choices > 1 {
        return Err(ID.usage("cannot print \"only\" of more than one choice".to_string()));
    }

    let format = if just_user {
        Format::User
    } else if just_group {
        Format::Group
    } else if just_group_list {
        Format::GroupList
    } else {
        Format::Full
    };

    if format == Format::Full {
        // `-n` and `-r` modify one of the three "only" reports; with none of
        // them chosen there is nothing for them to modify, and printing the
        // default line as if they had not been given would answer a question
        // that was not asked.
        if use_real || use_name {
            return Err(
                ID.usage("cannot print only names or real IDs in default format".to_string())
            );
        }
        if zero {
            return Err(ID.usage("option --zero not permitted in default format".to_string()));
        }
    }

    Ok(Request::Run(Settings {
        format,
        use_name,
        use_real,
        zero,
        users,
    }))
}

/// id.c's operand resolution: `parse_user_spec (spec, &euid, NULL, &pw_name,
/// NULL)`, then `pw_name ? getpwnam (pw_name) : getpwuid (euid)`.
///
/// gnulib sets `pw_name` only when the *name* lookup succeeded, so a numeric
/// operand — or one escaped with `+` — comes back here with no name and is
/// resolved by uid instead. That two-step is why `id 1000` finds an account
/// *called* `1000` in preference to uid 1000, and why `id +1000` does not.
///
/// The empty operand is rejected without parsing, as upstream's `if (*spec)`
/// does: gnulib treats it as a valid no-op spec, which is meaningful to
/// `chown` and meaningless here.
///
/// Returns the account's name, uid and gid — the three fields id.c takes from
/// the `struct passwd` it ends up with.
fn resolve_operand(spec: &[u8], db: &Db) -> Option<(Vec<u8>, u32, u32)> {
    if spec.is_empty() {
        return None;
    }
    let parsed = parse_user_only(spec, db).ok()?;
    let found = match &parsed.user_name {
        Some(name) => db.user_by_name(name),
        None => db.user_by_uid(parsed.uid?),
    }?;
    Some((found.name.clone(), found.uid, found.gid))
}

// ---------------------------------------------------------------- reports ---

/// gnulib's `mgetgroups`, which is two unrelated functions sharing a name.
///
/// With a `username` it is `getgrouplist` — a query against `/etc/group`,
/// answered by [`pwdb::Db::group_list`]. Without one it is `getgroups(2)`,
/// the kernel's list for *this* process, with `gid` (the effective gid)
/// pushed on the front because some systems' `getgroups` omits it and some
/// return it twice.
///
/// That prepending is what makes the duplicate reduction below necessary, and
/// it is deliberately the same weak one gnulib uses: a single pass that drops
/// any entry equal to the first element or to its immediate predecessor.
/// gnulib documents the result as free of *pair-wise* duplicates rather than
/// minimal, having judged a sort or a hash table not worth it — and since the
/// output order is user-visible, a stronger rule would print a different list.
fn groups_for(username: Option<&[u8]>, gid: u32, db: &Db, process_groups: &[u32]) -> Vec<u32> {
    match username {
        Some(name) => db.group_list(name, gid),
        None => {
            let mut all = Vec::with_capacity(process_groups.len().saturating_add(1));
            all.push(gid);
            all.extend_from_slice(process_groups);
            reduce_duplicates(&all)
        }
    }
}

/// gnulib's O(n) duplicate pass. See [`groups_for`] for why it is this and not
/// a real dedup.
fn reduce_duplicates(groups: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(groups.len());
    for &gid in groups {
        let redundant = match (out.first(), out.last()) {
            (Some(&first), Some(&last)) => gid == first || gid == last,
            // The first element is always kept.
            _ => false,
        };
        if !redundant {
            out.push(gid);
        }
    }
    out
}

/// id.c's `print_user`. The number is printed either way; `-n` only adds the
/// chance of a diagnostic.
fn print_user(out: &mut Output, uid: u32, use_name: bool, db: &Db) {
    let name = if use_name {
        let found = db.user_by_uid(uid).map(|user| user.name.clone());
        if found.is_none() {
            out.fail(format!("cannot find name for user ID {uid}"));
        }
        found
    } else {
        None
    };
    match name {
        Some(name) => out.out.extend_from_slice(&name),
        None => out.push_number(uid),
    }
}

/// group-list.c's `print_group`, the mirror of [`print_user`].
fn print_group(out: &mut Output, gid: u32, use_name: bool, db: &Db) {
    let name = if use_name {
        let found = db.group_by_gid(gid).map(|group| group.name.clone());
        if found.is_none() {
            out.fail(format!("cannot find name for group ID {gid}"));
        }
        found
    } else {
        None
    };
    match name {
        Some(name) => out.out.extend_from_slice(&name),
        None => out.push_number(gid),
    }
}

/// group-list.c's `print_group_list`: the `-G` report.
///
/// The real gid leads, the effective one follows when it differs, and the
/// supplementary list contributes only what is neither — see the module docs
/// for why this is not the same list `groups=` prints.
///
/// The login group fed to the lookup is `getpwuid(ruid)`'s, *not* `rgid`. The
/// two are normally equal, but they are reached differently — `rgid` came from
/// the account named on the command line, this one from whichever passwd line
/// holds that uid first — so a database with two lines sharing a uid can make
/// them differ. Upstream sets `ok = false` without a message when that lookup
/// fails, and so does this.
fn print_group_list(
    out: &mut Output,
    username: Option<&[u8]>,
    ids: Ids,
    use_name: bool,
    delim: u8,
    db: &Db,
    process_groups: &[u32],
) {
    let login_group = match username {
        Some(_) => match db.user_by_uid(ids.ruid) {
            Some(user) => user.gid,
            None => {
                out.ok = false;
                ids.egid
            }
        },
        None => ids.egid,
    };

    print_group(out, ids.rgid, use_name, db);
    if ids.egid != ids.rgid {
        out.out.push(delim);
        print_group(out, ids.egid, use_name, db);
    }
    for gid in groups_for(username, login_group, db, process_groups) {
        if gid != ids.rgid && gid != ids.egid {
            out.out.push(delim);
            print_group(out, gid, use_name, db);
        }
    }
}

/// id.c's `print_full_info`: the default line.
///
/// `euid=` and `egid=` appear only when they differ from the real ids, which
/// is why the plain `id` of an ordinary process shows two fields and not four.
///
/// Upstream reads the login group out of a `pwd` variable that `print_full_info`
/// has been reusing, and which the `euid != ruid` branch may have reassigned to
/// `getpwuid (euid)`. That aliasing cannot bite: the branch only runs when the
/// two ids differ, and they differ only in the no-username case, where the
/// login group is `egid` and `pwd` is not consulted at all. So the lookup is
/// written here as what it always resolves to.
fn print_full_info(
    out: &mut Output,
    username: Option<&[u8]>,
    ids: Ids,
    db: &Db,
    process_groups: &[u32],
) {
    out.out.extend_from_slice(b"uid=");
    out.push_number(ids.ruid);
    if let Some(user) = db.user_by_uid(ids.ruid) {
        out.push_parenthesised(&user.name);
    }

    out.out.extend_from_slice(b" gid=");
    out.push_number(ids.rgid);
    if let Some(group) = db.group_by_gid(ids.rgid) {
        out.push_parenthesised(&group.name);
    }

    if ids.euid != ids.ruid {
        out.out.extend_from_slice(b" euid=");
        out.push_number(ids.euid);
        if let Some(user) = db.user_by_uid(ids.euid) {
            out.push_parenthesised(&user.name);
        }
    }

    if ids.egid != ids.rgid {
        out.out.extend_from_slice(b" egid=");
        out.push_number(ids.egid);
        if let Some(group) = db.group_by_gid(ids.egid) {
            out.push_parenthesised(&group.name);
        }
    }

    // `(gid_t) -1` when the uid has no passwd line: upstream's `pwd ?
    // pwd->pw_gid : -1`, which then reaches getgrouplist as the login group
    // and so appears in the output. Reproduced rather than tidied, because a
    // number that looks wrong is a better signal than a silently-dropped
    // entry.
    let login_group = match username {
        Some(_) => db.user_by_uid(ids.ruid).map_or(u32::MAX, |user| user.gid),
        None => ids.egid,
    };
    let groups = groups_for(username, login_group, db, process_groups);
    if !groups.is_empty() {
        out.out.extend_from_slice(b" groups=");
    }
    for (index, gid) in groups.iter().enumerate() {
        if index > 0 {
            out.out.push(b',');
        }
        out.push_number(*gid);
        if let Some(group) = db.group_by_gid(*gid) {
            out.push_parenthesised(&group.name);
        }
    }
}

/// id.c's `print_stuff`: one record, terminator included.
///
/// The terminator is the odd corner. `-z` normally ends a record with one NUL,
/// but `-G -z` with *more than one* USER ends it with two, because a
/// NUL-delimited group list would otherwise be indistinguishable from the next
/// user's. Measured:
///
/// ```text
/// $ id -Gz root           | od -c     0  \0
/// $ id -Gz root inhahe    | od -c     0  \0  \0   1 0 0 0  \0  4  \0 …  \0  \0
/// $ id -uz root inhahe    | od -c     0  \0   1   0 0 0 \0
/// ```
///
/// Note the second: the doubling applies to the last record too, so the stream
/// ends with two NULs rather than one.
fn print_stuff(
    out: &mut Output,
    settings: &Settings,
    ids: Ids,
    username: Option<&[u8]>,
    db: &Db,
    process_groups: &[u32],
    multiple_users: bool,
) {
    let delim = if settings.zero { b'\0' } else { b' ' };
    match settings.format {
        Format::User => {
            let uid = if settings.use_real {
                ids.ruid
            } else {
                ids.euid
            };
            print_user(out, uid, settings.use_name, db);
        }
        Format::Group => {
            let gid = if settings.use_real {
                ids.rgid
            } else {
                ids.egid
            };
            print_group(out, gid, settings.use_name, db);
        }
        Format::GroupList => print_group_list(
            out,
            username,
            ids,
            settings.use_name,
            delim,
            db,
            process_groups,
        ),
        Format::Full => print_full_info(out, username, ids, db, process_groups),
    }

    if settings.zero && settings.format == Format::GroupList && multiple_users {
        out.out.push(b'\0');
        out.out.push(b'\0');
    } else {
        out.out.push(if settings.zero { b'\0' } else { b'\n' });
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("id: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{
        Ids, Output, Request, Settings, help_text, parse_args, print_stuff, resolve_operand,
    };
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, quote};
    use pwdb::Db;
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process::ExitCode;

    unsafe extern "C" {
        fn getuid() -> u32;
        fn geteuid() -> u32;
        fn getgid() -> u32;
        fn getegid() -> u32;
        /// `getgroups(0, NULL)` counts; a second call fills. Returns -1 on
        /// failure, which for us means "no supplementary groups" rather than
        /// an error worth reporting — see the module docs.
        fn getgroups(size: i32, list: *mut u32) -> i32;
    }

    /// All four ids, unconditionally.
    ///
    /// Upstream fetches only the ones the chosen report will read, so that a
    /// Hurd-style failure of an *unused* getter cannot abort the run. The
    /// unfetched ones keep their static-storage zero, and none of the four
    /// reports ever reads one: `-u` reads only `ruid`/`euid`, `-g` only the
    /// gids, `-G` reads `ruid` solely to look up a username it does not have
    /// in this branch, and the default line reads all four. So fetching all
    /// four is observationally identical and does not need the conditional
    /// ladder.
    fn current_ids() -> Ids {
        // SAFETY: four POSIX getters, no arguments, no pointers, and POSIX
        // requires that they cannot fail.
        unsafe {
            Ids {
                ruid: getuid(),
                euid: geteuid(),
                rgid: getgid(),
                egid: getegid(),
            }
        }
    }

    /// The current process's supplementary groups, or none if the kernel has
    /// no answer. Never an error: gnulib treats a failed count as an empty
    /// list unless it can grow one from `gid`, which the caller supplies.
    fn process_groups() -> Vec<u32> {
        // SAFETY: the counting form; POSIX requires a null list pointer be
        // ignored when the size is 0.
        let counted = unsafe { getgroups(0, std::ptr::null_mut()) };
        let Ok(count) = usize::try_from(counted) else {
            return Vec::new();
        };
        if count == 0 {
            return Vec::new();
        }
        let Ok(size) = i32::try_from(count) else {
            return Vec::new();
        };
        let mut buffer = vec![0_u32; count];
        // SAFETY: `buffer` has room for `size` gids, which is what is claimed.
        let filled = unsafe { getgroups(size, buffer.as_mut_ptr()) };
        match usize::try_from(filled) {
            // A shrinking race is possible — a group could be dropped between
            // the two calls — so trust the second answer, not the first.
            Ok(filled) if filled <= count => {
                buffer.truncate(filled);
                buffer
            }
            _ => Vec::new(),
        }
    }

    /// Flush one record: diagnostics first, because stderr is unbuffered and a
    /// reader watching both streams should see the complaint next to the line
    /// it is about.
    fn drain(out: &mut Output, sink: &mut impl Write) -> io::Result<()> {
        for message in out.errors.drain(..) {
            eprintln!("id: {message}");
        }
        let written = sink.write_all(&out.out);
        out.out.clear();
        written
    }

    fn run(
        settings: &Settings,
        db: &Db,
        out: &mut Output,
        sink: &mut impl Write,
    ) -> io::Result<()> {
        if settings.users.is_empty() {
            print_stuff(
                out,
                settings,
                current_ids(),
                None,
                db,
                &process_groups(),
                false,
            );
            return drain(out, sink);
        }

        let multiple_users = settings.users.len() > 1;
        for spec in &settings.users {
            let bytes = os_bytes(spec);
            match resolve_operand(&bytes, db) {
                None => out.fail(format!("{}: no such user", quote(&bytes))),
                Some((name, uid, gid)) => print_stuff(
                    out,
                    settings,
                    Ids {
                        ruid: uid,
                        euid: uid,
                        rgid: gid,
                        egid: gid,
                    },
                    Some(&name),
                    db,
                    // Unread: a username sends `groups_for` down the database
                    // path, which never looks at the current process.
                    &[],
                    multiple_users,
                ),
            }
            drain(out, sink)?;
        }
        Ok(())
    }

    pub fn main() -> ExitCode {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Help) => {
                print!("{}", help_text());
                return ExitCode::SUCCESS;
            }
            Ok(Request::Version) => {
                println!("id (SlateOS coreutils) 0.1.0");
                return ExitCode::SUCCESS;
            }
            Ok(Request::Run(settings)) => settings,
            Err(e) => {
                eprintln!("id: {e}");
                return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
            }
        };

        // One read of `/etc/passwd` and `/etc/group` for the whole run. An
        // unreadable database is an empty one, not an error, so `id -u` still
        // answers on a system without the files.
        let db = Db::load();
        let mut out = Output::new();
        let stdout = io::stdout();
        let mut sink = stdout.lock();

        if let Err(e) = run(&settings, &db, &mut out, &mut sink) {
            eprintln!("id: write error: {}", strerror(&e));
            return ExitCode::from(1);
        }
        if let Err(e) = sink.flush() {
            eprintln!("id: write error: {}", strerror(&e));
            return ExitCode::from(1);
        }
        if out.ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
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
    use coreutils::quote::{os_bytes, quote};

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// A database with the corners the tests need: an account whose *name* is
    /// a number, two accounts sharing a uid, a user in several groups, a gid
    /// with no `/etc/group` line, and a group whose member list is empty.
    fn db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:1000:Alice:/home/alice:/bin/sh\n\
              bob:x:1001:5000:Bob:/home/bob:/bin/sh\n\
              1000:x:4000:4000:confusing:/:/bin/sh\n\
              twin:x:1001:2000:Shares bob's uid:/:/bin/sh\n",
            b"root:x:0:\n\
              alice:x:1000:\n\
              staff:x:2000:alice,bob\n\
              wheel:x:3000:alice\n",
        )
    }

    fn settings(args: &[&str]) -> Settings {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(settings) => settings,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn err(args: &[&str]) -> String {
        parse_args(&argv(args)).unwrap_err().message()
    }

    /// Render one report the way `main` would, and return stdout plus stderr.
    fn report(
        args: &[&str],
        ids: Ids,
        username: Option<&[u8]>,
        groups: &[u32],
    ) -> (Vec<u8>, Vec<String>) {
        let settings = settings(args);
        let mut out = Output::new();
        print_stuff(
            &mut out,
            &settings,
            ids,
            username,
            &db(),
            groups,
            settings.users.len() > 1,
        );
        (out.out, out.errors)
    }

    /// The whole pipeline for USER operands: resolve each, print each.
    fn run_users(args: &[&str]) -> (String, Vec<String>, bool) {
        let settings = settings(args);
        let db = db();
        let mut out = Output::new();
        let multiple = settings.users.len() > 1;
        for spec in &settings.users {
            let bytes = os_bytes(spec);
            match resolve_operand(&bytes, &db) {
                None => out.fail(format!("{}: no such user", quote(&bytes))),
                Some((name, uid, gid)) => print_stuff(
                    &mut out,
                    &settings,
                    Ids {
                        ruid: uid,
                        euid: uid,
                        rgid: gid,
                        egid: gid,
                    },
                    Some(&name),
                    &db,
                    &[],
                    multiple,
                ),
            }
        }
        (
            String::from_utf8(out.out).unwrap(),
            out.errors.clone(),
            out.ok,
        )
    }

    // ---------------- parse_args: flags ----------------

    #[test]
    fn no_arguments_is_the_full_report() {
        let s = settings(&[]);
        assert_eq!(s.format, Format::Full);
        assert!(!s.use_name && !s.use_real && !s.zero);
        assert!(s.users.is_empty());
    }

    #[test]
    fn each_only_flag_selects_its_format() {
        assert_eq!(settings(&["-u"]).format, Format::User);
        assert_eq!(settings(&["-g"]).format, Format::Group);
        assert_eq!(settings(&["-G"]).format, Format::GroupList);
        assert_eq!(settings(&["--user"]).format, Format::User);
        assert_eq!(settings(&["--group"]).format, Format::Group);
        assert_eq!(settings(&["--groups"]).format, Format::GroupList);
    }

    #[test]
    fn operands_are_collected_not_discarded() {
        // The defect this file was rewritten for: `id alice` used to print the
        // caller's own ids and exit 0.
        assert_eq!(settings(&["alice"]).users, argv(&["alice"]));
        assert_eq!(
            settings(&["-u", "alice", "bob"]).users,
            argv(&["alice", "bob"])
        );
    }

    #[test]
    fn an_operand_may_look_like_an_option_after_a_double_dash() {
        assert_eq!(settings(&["-u", "--", "-n"]).users, argv(&["-n"]));
    }

    #[test]
    fn a_is_accepted_and_ignored() {
        let s = settings(&["-a", "root"]);
        assert_eq!(s.format, Format::Full);
        assert_eq!(s.users, argv(&["root"]));
    }

    #[test]
    fn repeating_one_choice_is_legal() {
        // `just_user + just_group + …` counts distinct booleans, so `-u -u` is
        // one choice. Measured: `id -u -u root` prints 0.
        assert_eq!(settings(&["-u", "-u"]).format, Format::User);
        assert_eq!(settings(&["-uu"]).format, Format::User);
    }

    #[test]
    fn name_and_real_are_independent_modifiers() {
        let s = settings(&["-Gnr"]);
        assert_eq!(s.format, Format::GroupList);
        assert!(s.use_name && s.use_real);
    }

    // ---------------- parse_args: errors ----------------

    #[test]
    fn two_choices_is_an_error_without_a_referral() {
        assert_eq!(
            err(&["-ugG"]),
            "cannot print \"only\" of more than one choice"
        );
        assert_eq!(
            err(&["-u", "--group"]),
            "cannot print \"only\" of more than one choice"
        );
    }

    #[test]
    fn names_or_real_ids_need_a_choice() {
        let expected = "cannot print only names or real IDs in default format";
        assert_eq!(err(&["-n"]), expected);
        assert_eq!(err(&["-r"]), expected);
        assert_eq!(err(&["-nr"]), expected);
    }

    #[test]
    fn zero_is_rejected_in_the_default_format() {
        let expected = "option --zero not permitted in default format";
        assert_eq!(err(&["-z"]), expected);
        assert_eq!(err(&["--zero"]), expected);
        // An unambiguous prefix: `--z` and `--zer` resolve to `--zero`.
        assert_eq!(err(&["--zer"]), expected);
        assert_eq!(err(&["--z"]), expected);
    }

    #[test]
    fn name_is_checked_before_zero() {
        // Both apply to `id -nz`; upstream's order makes it the -n message.
        assert_eq!(
            err(&["-nz"]),
            "cannot print only names or real IDs in default format"
        );
        assert_eq!(
            err(&["-zn"]),
            "cannot print only names or real IDs in default format"
        );
    }

    #[test]
    fn context_is_rejected_before_everything_else() {
        let expected = NO_SELINUX.to_string();
        assert_eq!(err(&["-Z"]), expected);
        assert_eq!(err(&["--context"]), expected);
        // Even alongside errors that would otherwise be reported instead,
        // because upstream rejects -Z inside the option loop.
        assert_eq!(err(&["-zZ"]), expected);
        assert_eq!(err(&["-Z", "root"]), expected);
        assert_eq!(err(&["-uZ"]), expected);
    }

    #[test]
    fn unknown_and_ambiguous_options_do_carry_a_referral() {
        assert_eq!(
            err(&["-X"]),
            "invalid option -- 'X'\nTry 'id --help' for more information."
        );
        assert_eq!(
            err(&["--g"]),
            "option '--g' is ambiguous; possibilities: '--group' '--groups'\n\
             Try 'id --help' for more information."
        );
        assert_eq!(
            err(&["--gro"]),
            "option '--gro' is ambiguous; possibilities: '--group' '--groups'\n\
             Try 'id --help' for more information."
        );
    }

    #[test]
    fn help_and_version_win_over_a_bad_command_line() {
        assert_eq!(parse_args(&argv(&["-z", "--help"])).unwrap(), Request::Help);
        assert_eq!(
            parse_args(&argv(&["-z", "--version"])).unwrap(),
            Request::Version
        );
    }

    #[test]
    fn the_usage_status_is_one() {
        assert_eq!(parse_args(&argv(&["-X"])).unwrap_err().status, 1);
        assert_eq!(parse_args(&argv(&["-z"])).unwrap_err().status, 1);
    }

    // ---------------- resolve_operand ----------------

    #[test]
    fn an_operand_resolves_by_name() {
        assert_eq!(
            resolve_operand(b"alice", &db()),
            Some((b"alice".to_vec(), 1000, 1000))
        );
    }

    #[test]
    fn a_name_that_looks_like_a_number_beats_the_number() {
        // The account called `1000` has uid 4000; `+1000` skips the lookup and
        // means uid 1000, which is alice.
        assert_eq!(
            resolve_operand(b"1000", &db()),
            Some((b"1000".to_vec(), 4000, 4000))
        );
        assert_eq!(
            resolve_operand(b"+1000", &db()),
            Some((b"alice".to_vec(), 1000, 1000))
        );
    }

    #[test]
    fn a_number_with_no_account_of_that_name_is_a_uid() {
        assert_eq!(resolve_operand(b"0", &db()), Some((b"root".to_vec(), 0, 0)));
    }

    #[test]
    fn a_shared_uid_resolves_to_the_line_that_was_named() {
        // Both `bob` and `twin` are uid 1001, with different gids. Naming one
        // gets that one's gid; naming the *number* gets the first line.
        assert_eq!(
            resolve_operand(b"twin", &db()),
            Some((b"twin".to_vec(), 1001, 2000))
        );
        assert_eq!(
            resolve_operand(b"1001", &db()),
            Some((b"bob".to_vec(), 1001, 5000))
        );
    }

    #[test]
    fn a_separator_makes_the_operand_no_such_user() {
        // The group half of the grammar is switched off for id; see the module
        // docs. All three measured against GNU 9.4.
        let db = db();
        assert_eq!(resolve_operand(b"alice:staff", &db), None);
        assert_eq!(resolve_operand(b"alice.staff", &db), None);
        assert_eq!(resolve_operand(b"alice:", &db), None);
        assert_eq!(resolve_operand(b":alice", &db), None);
    }

    #[test]
    fn the_empty_operand_and_the_leave_alone_sentinel_are_rejected() {
        let db = db();
        assert_eq!(resolve_operand(b"", &db), None);
        // (uid_t)-1 is "leave this alone", never an account.
        assert_eq!(resolve_operand(b"4294967295", &db), None);
        assert_eq!(resolve_operand(b"nosuchuser", &db), None);
    }

    // ---------------- the group list ----------------

    #[test]
    fn a_username_uses_the_database_and_leads_with_the_login_group() {
        let db = db();
        assert_eq!(
            groups_for(Some(b"alice"), 1000, &db, &[]),
            vec![1000, 2000, 3000]
        );
        // The login group is suppressed from the member-list half even when it
        // is reached under a different name, so no duplicate appears.
        assert_eq!(groups_for(Some(b"alice"), 2000, &db, &[]), vec![2000, 3000]);
    }

    #[test]
    fn no_username_uses_the_process_list_with_the_gid_in_front() {
        let db = db();
        assert_eq!(groups_for(None, 1000, &db, &[]), vec![1000]);
        assert_eq!(
            groups_for(None, 1000, &db, &[2000, 3000]),
            vec![1000, 2000, 3000]
        );
        // gnulib's weak dedup: equal to the first element, or to the previous
        // kept one.
        assert_eq!(
            groups_for(None, 1000, &db, &[1000, 2000, 2000, 1000]),
            vec![1000, 2000]
        );
    }

    #[test]
    fn the_duplicate_pass_is_gnulibs_weak_one_not_a_real_dedup() {
        // 5 reappears after 7 and survives, because it is neither the first
        // element nor adjacent to its earlier self. Upstream documents this.
        assert_eq!(reduce_duplicates(&[9, 5, 7, 5]), vec![9, 5, 7, 5]);
        assert_eq!(reduce_duplicates(&[]), Vec::<u32>::new());
        assert_eq!(reduce_duplicates(&[7]), vec![7]);
    }

    // ---------------- reports ----------------

    fn same(uid: u32, gid: u32) -> Ids {
        Ids {
            ruid: uid,
            euid: uid,
            rgid: gid,
            egid: gid,
        }
    }

    #[test]
    fn the_default_line_puts_gid_before_euid() {
        // The old file emitted `uid= euid= gid= egid=`. GNU does not.
        let (out, errors) = report(
            &[],
            Ids {
                ruid: 1000,
                euid: 0,
                rgid: 1000,
                egid: 0,
            },
            None,
            &[],
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "uid=1000(alice) gid=1000(alice) euid=0(root) egid=0(root) groups=0(root)\n"
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn the_default_line_omits_effective_ids_that_match() {
        let (out, _) = report(&[], same(1000, 1000), None, &[]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "uid=1000(alice) gid=1000(alice) groups=1000(alice)\n"
        );
    }

    #[test]
    fn the_default_line_prints_the_whole_group_list_unfiltered() {
        let (out, _) = report(&[], same(1000, 1000), Some(b"alice"), &[]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "uid=1000(alice) gid=1000(alice) groups=1000(alice),2000(staff),3000(wheel)\n"
        );
    }

    #[test]
    fn an_unnamed_id_prints_as_a_bare_number() {
        let (out, errors) = report(&[], same(7777, 8888), None, &[]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "uid=7777 gid=8888 groups=8888\n"
        );
        // No `-n`, so no diagnostic: the number *is* the answer.
        assert!(errors.is_empty());
    }

    #[test]
    fn group_list_filters_out_the_real_and_effective_gids() {
        // alice's list is 1000,2000,3000; with egid 2000 both 1000 and 2000
        // are already printed and must not repeat.
        let (out, _) = report(
            &["-G"],
            Ids {
                ruid: 1000,
                euid: 1000,
                rgid: 1000,
                egid: 2000,
            },
            Some(b"alice"),
            &[],
        );
        assert_eq!(String::from_utf8(out).unwrap(), "1000 2000 3000\n");
    }

    #[test]
    fn group_list_names_with_n() {
        let (out, _) = report(&["-Gn"], same(1000, 1000), Some(b"alice"), &[]);
        assert_eq!(String::from_utf8(out).unwrap(), "alice staff wheel\n");
    }

    #[test]
    fn a_missing_group_name_is_reported_and_the_number_printed() {
        let (out, errors) = report(&["-gn"], same(1000, 6000), None, &[]);
        assert_eq!(String::from_utf8(out).unwrap(), "6000\n");
        assert_eq!(errors, vec!["cannot find name for group ID 6000"]);
    }

    #[test]
    fn a_missing_user_name_is_reported_and_the_number_printed() {
        let (out, errors) = report(&["-un"], same(7777, 1000), None, &[]);
        assert_eq!(String::from_utf8(out).unwrap(), "7777\n");
        assert_eq!(errors, vec!["cannot find name for user ID 7777"]);
    }

    #[test]
    fn real_swaps_which_id_is_printed() {
        let ids = Ids {
            ruid: 1000,
            euid: 0,
            rgid: 1000,
            egid: 0,
        };
        assert_eq!(report(&["-u"], ids, None, &[]).0, b"0\n");
        assert_eq!(report(&["-ur"], ids, None, &[]).0, b"1000\n");
        assert_eq!(report(&["-g"], ids, None, &[]).0, b"0\n");
        assert_eq!(report(&["-gr"], ids, None, &[]).0, b"1000\n");
    }

    // ---------------- terminators ----------------

    #[test]
    fn zero_ends_a_single_record_with_one_nul() {
        assert_eq!(report(&["-uz"], same(0, 0), None, &[]).0, b"0\0");
        assert_eq!(report(&["-Gz"], same(0, 0), Some(b"root"), &[]).0, b"0\0");
    }

    #[test]
    fn zero_and_group_list_double_the_nul_only_for_several_users() {
        // Measured: `id -Gz root inhahe | od -c` shows `0 \0 \0 … \0 \0`.
        let (out, _, _) = run_users(&["-Gz", "root", "alice"]);
        assert_eq!(
            out,
            ["0", "\0\0", "1000", "\0", "2000", "\0", "3000", "\0\0"].concat()
        );
    }

    #[test]
    fn zero_with_u_stays_single_nul_for_several_users() {
        let (out, _, _) = run_users(&["-uz", "root", "alice"]);
        assert_eq!(out, ["0", "\0", "1000", "\0"].concat());
    }

    // ---------------- operands end to end ----------------

    #[test]
    fn several_users_produce_several_lines() {
        let (out, errors, ok) = run_users(&["root", "alice"]);
        assert_eq!(
            out,
            "uid=0(root) gid=0(root) groups=0(root)\n\
             uid=1000(alice) gid=1000(alice) groups=1000(alice),2000(staff),3000(wheel)\n"
        );
        assert!(errors.is_empty());
        assert!(ok);
    }

    #[test]
    fn a_bad_operand_reports_and_the_run_continues() {
        // Measured: `id -u root nosuchuser` prints 0, complains, exits 1.
        let (out, errors, ok) = run_users(&["-u", "root", "nosuchuser"]);
        assert_eq!(out, "0\n");
        assert_eq!(errors, vec!["\u{2018}nosuchuser\u{2019}: no such user"]);
        assert!(!ok);
    }

    #[test]
    fn a_non_utf8_operand_is_reported_without_mangling() {
        // The whole point of the conversion: argv is bytes. The old `main`
        // panicked outright on this before reaching any of the above.
        let db = db();
        assert_eq!(resolve_operand(b"caf\xe9", &db), None);
        let mut out = Output::new();
        out.fail(format!("{}: no such user", quote(b"caf\xe9")));
        assert_eq!(out.errors, vec!["\u{2018}caf\\351\u{2019}: no such user"]);
    }

    #[test]
    fn a_user_whose_name_is_not_utf8_still_prints() {
        let db = Db::from_bytes(b"caf\xe9:x:1234:1234::/:/bin/sh\n", b"caf\xe9:x:1234:\n");
        let mut out = Output::new();
        print_stuff(
            &mut out,
            &settings(&[]),
            same(1234, 1234),
            Some(b"caf\xe9"),
            &db,
            &[],
            false,
        );
        assert_eq!(
            out.out,
            b"uid=1234(caf\xe9) gid=1234(caf\xe9) groups=1234(caf\xe9)\n"
        );
    }

    // ---------------- help ----------------

    #[test]
    fn help_lists_every_option_in_the_tables() {
        let help = help_text();
        for (name, _) in LONG_OPTIONS {
            assert!(help.contains(&format!("--{name}")), "help omits --{name}");
        }
        for flag in SHORT_OPTIONS.chars() {
            assert!(help.contains(&format!("-{flag}")), "help omits -{flag}");
        }
    }
}
