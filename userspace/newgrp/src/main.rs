//! Slate OS group switching utility.
//!
//! Multi-personality binary providing:
//! - **newgrp** — log in to a new group
//! - **sg** — execute command as different group
//!
//! Changes the current group ID during a login session, optionally
//! running a command under the new group context.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct GroupEntry {
    name: String,
    gid: u32,
    members: Vec<String>,
}

#[derive(Clone, Debug)]
struct UserInfo {
    username: String,
    _uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

/// One line of `/etc/gshadow`: `name:password:admins:members`.
///
/// The password field is a `crypt(3)` entry, or `!`/`*` for a group nobody may
/// join by password, or empty for one that has none. It is not decoration: it
/// is the only thing standing between a non-member and the group.
#[derive(Clone, Debug)]
struct GshadowEntry {
    name: String,
    password: String,
    /// Who may change this group's password. Parsed because it is the format's
    /// third field and a parser that skipped it would silently accept a line
    /// whose fields had shifted; not consulted, because nothing here changes a
    /// group password yet. `gpasswd` is what would read it.
    #[allow(dead_code)]
    admins: Vec<String>,
    /// The group's members, as `/etc/gshadow` records them -- a second copy of
    /// what `/etc/group` already says. Deliberately *not* consulted for
    /// membership: `user_is_member` asks `/etc/group`, which is the file every
    /// other tool asks, and reading a second copy here would mean this program
    /// could admit someone the rest of the system does not consider a member.
    /// See `open-questions.md` on group membership being stored twice.
    #[allow(dead_code)]
    members: Vec<String>,
}

// ============================================================================
// Group database parsing (/etc/group format)
// ============================================================================

fn parse_group_line(line: &str) -> Option<GroupEntry> {
    let parts: Vec<&str> = line.splitn(4, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let name = parts[0].to_string();
    let gid = parts.get(2).and_then(|s| s.parse().ok())?;
    let members = parts
        .get(3)
        .map(|s| {
            s.split(',')
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string())
                .collect()
        })
        .unwrap_or_default();
    Some(GroupEntry { name, gid, members })
}

fn read_group_db() -> Vec<GroupEntry> {
    let content = std::fs::read_to_string("/etc/group").unwrap_or_default();
    content.lines().filter_map(parse_group_line).collect()
}

fn parse_gshadow_line(line: &str) -> Option<GshadowEntry> {
    let parts: Vec<&str> = line.splitn(4, ':').collect();
    if parts.len() < 4 {
        return None;
    }
    Some(GshadowEntry {
        name: parts[0].to_string(),
        password: parts[1].to_string(),
        admins: parts[2]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        members: parts[3]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Read `/etc/gshadow`.
///
/// An unreadable file is an empty list, and an empty list refuses everyone --
/// see [`verify_group_password`]. `newgrp` runs setuid root precisely so that
/// it *can* read this file; a caller that cannot is not a reason to admit
/// anyone.
fn read_gshadow_db() -> Vec<GshadowEntry> {
    let content = std::fs::read_to_string("/etc/gshadow").unwrap_or_default();
    content.lines().filter_map(parse_gshadow_line).collect()
}

// ============================================================================
// Current user info
// ============================================================================

/// Who is running this, from the kernel and the account database.
///
/// # What this replaces, and why it mattered here more than most
///
/// The username came from `$USER`, falling back to `$LOGNAME` and then to the
/// literal string `"root"`; the uid and gid came from `$UID` and `$GID`, both
/// falling back to `0`. All four are set by whoever starts the program.
///
/// `newgrp` is setuid root and the username is not decoration: it selects the
/// caller's group memberships out of `/etc/group`, and
/// [`user_is_member`] uses that to decide whether to ask for the group
/// password at all. So `USER=<anyone in wheel> newgrp wheel` skipped the
/// password outright -- and with `USER` unset, which is the normal case, the
/// caller was `"root"` and inherited root's memberships. The gshadow password
/// this program exists to check was reachable only by callers who had already
/// failed to be somebody useful.
///
/// # Resolution, and what happens when it fails
///
/// The uid is `getuid(2)` -- see `authlib::identity::caller_uid` -- and the
/// name and gid come from that uid's record in the account database.
///
/// A uid with no record is **not** an error here: the caller gets an empty
/// username, which matches no group's member list, and a gid equal to their
/// uid, which is the user-private-group convention `useradd` follows. The
/// effect is that an unknown caller has no memberships and must supply the
/// group password, which is the fail-closed direction and still leaves the
/// program usable. Refusing outright would deny them the one legitimate way
/// in.
fn get_current_user() -> Option<UserInfo> {
    let uid = authlib::identity::caller_uid()?;

    let record = userdb::UserDb::load(userdb::DEFAULT_PATH)
        .ok()
        .and_then(|db| db.find_uid(uid).cloned());

    let username = record
        .as_ref()
        .and_then(userdb::Record::username)
        .unwrap_or_default();
    // No `gid:` of its own means the user-private group, which `useradd`
    // numbers after the uid.
    let gid = record.as_ref().and_then(userdb::Record::gid).unwrap_or(uid);

    let groups = read_user_supplementary_groups(uid, &username);

    Some(UserInfo {
        username,
        _uid: uid,
        gid,
        groups,
    })
}

fn read_user_supplementary_groups(_uid: u32, username: &str) -> Vec<u32> {
    let group_db = read_group_db();
    group_db
        .iter()
        .filter(|g| g.members.iter().any(|m| m == username))
        .map(|g| g.gid)
        .collect()
}

// ============================================================================
// Group membership check
// ============================================================================

fn user_is_member(user: &UserInfo, group: &GroupEntry) -> bool {
    // User's primary group matches.
    if user.gid == group.gid {
        return true;
    }
    // User is in the group member list.
    if group.members.iter().any(|m| m == &user.username) {
        return true;
    }
    // User has this group as supplementary.
    if user.groups.contains(&group.gid) {
        return true;
    }
    false
}

fn find_group_by_name(groups: &[GroupEntry], name: &str) -> Option<GroupEntry> {
    groups.iter().find(|g| g.name == name).cloned()
}

fn find_group_by_gid(groups: &[GroupEntry], gid: u32) -> Option<GroupEntry> {
    groups.iter().find(|g| g.gid == gid).cloned()
}

// ============================================================================
// Password verification (stub — real impl would use crypt(3))
// ============================================================================

/// Is `password` the password of `group`?
///
/// # What this replaced
///
/// ```text
/// // For now, accept any non-empty password for groups that have one set.
/// !_password.is_empty()
/// ```
///
/// That accepted **any** non-empty string as any group's password. It was inert
/// only because [`exec_with_group`] cannot yet change the process's groups, so
/// passing the check gained nothing -- but the whole point of
/// `requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`
/// is to make that exec real, and the check would have been armed by the very
/// change that was asked for. A stub that says yes is not a smaller version of
/// a check; it is the absence of one, wearing the shape.
///
/// # The rules, which are `/etc/gshadow`'s and not this program's
///
/// * **No entry for the group** -- no password is set, so there is none to
///   match, and a non-member cannot join. Refused.
/// * **An empty password field** -- likewise: `newgrp(1)` treats a group with
///   no password as closed to non-members, *not* as open to everyone. This is
///   the case the old stub inverted.
/// * **`!` or `*`** -- the group is locked. [`authlib::check_stored`] reports
///   these as unrecomputable, so they match nothing, which is the answer.
/// * **A `crypt(3)` entry** -- checked by [`authlib::check_stored`], the same
///   function `login`, `su`, `doas` and `passwd` ask. This program states
///   nothing of its own about what a stored entry means; that was the defect
///   §341 exists to prevent.
fn verify_group_password(group: &str, password: &str) -> bool {
    let db = read_gshadow_db();
    password_opens(db.iter().find(|e| e.name == group), password)
}

/// The rule itself, separated from the file so it can be tested.
///
/// `entry` is `None` for a group with no `/etc/gshadow` line at all. Taking the
/// lookup's *result* rather than the group name is what makes every branch
/// reachable from a test: the caller reads a fixed path, and a rule that can
/// only be exercised through `/etc/gshadow` is a rule that is never exercised
/// on a development host that has none -- which is exactly how a stub that
/// accepted any password survived here.
fn password_opens(entry: Option<&GshadowEntry>, password: &str) -> bool {
    let Some(entry) = entry else {
        return false;
    };
    if entry.password.is_empty() {
        return false;
    }
    authlib::check_stored(password.as_bytes(), entry.password.as_bytes()).is_accepted()
}

fn prompt_password() -> String {
    eprint!("Password: ");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

// ============================================================================
// Shell execution
// ============================================================================

/// The shell to start, from `SHELL`.
///
/// `var_os`, not `var`: `SHELL` names a program, a program is named by a
/// path, and a path on this OS may hold any byte but `/` and NUL. `env::var`
/// would report such a value as an error and this would silently fall back to
/// `/bin/sh` -- starting a shell the user did not choose.
fn get_user_shell() -> OsString {
    env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"))
}

fn exec_with_group(gid: u32, command: Option<&[OsString]>) -> i32 {
    // In a real OS, this would call setgid(gid) then exec. It cannot: there is
    // no SYS_SETGROUPS, and a group change that set the primary gid without
    // the supplementary set would be a partial one. See
    // `requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.
    // Until then it says what it would do.
    match command {
        Some(cmd) if !cmd.is_empty() => {
            // Each word quoted separately, because they are separate: joining
            // them with spaces first would render `sg users -c 'rm a b'` and
            // `sg users -c 'rm' 'a b'` identically, and an argument may hold a
            // newline besides.
            let shown: Vec<String> = cmd.iter().map(quoteaf_os).collect();
            eprintln!(
                "newgrp: would setgid({}) and exec: {}",
                gid,
                shown.join(" ")
            );
        }
        _ => {
            eprintln!(
                "newgrp: would setgid({gid}) and exec: {}",
                quoteaf_os(get_user_shell())
            );
        }
    }
    0
}

// ============================================================================
// newgrp personality
// ============================================================================

fn newgrp_main(args: &[OsString]) -> i32 {
    let mut group_name: Option<OsString> = None;
    let mut login_shell = false;
    let mut i = 0;

    // An argument that is not valid UTF-8 matches no option, so it falls to
    // the `!starts_with('-')` arm and is taken as a group name -- which is
    // what it would have to be.
    while i < args.len() {
        let Some(raw) = args.get(i) else { break };
        match raw.to_str().unwrap_or_default() {
            "-" | "-l" => login_shell = true,
            "--help" => {
                println!("Usage: newgrp [-] [-l] [group]");
                println!();
                println!("Log in to a new group.");
                println!();
                println!("Options:");
                println!("  -        Start a login shell");
                println!("  -l       Start a login shell");
                println!("  --help   Display this help");
                println!("  --version Display version");
                return 0;
            }
            "--version" => {
                println!("newgrp (Slate OS coreutils) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                group_name = Some(raw.clone());
            }
            other => {
                eprintln!("newgrp: invalid option {}", quoteaf_os(other));
                return 1;
            }
        }
        i += 1;
    }

    let Some(user) = get_current_user() else {
        eprintln!("newgrp: cannot determine who is running this command");
        return 1;
    };
    let group_db = read_group_db();

    let target_group = match &group_name {
        // A group name is text, so a name that is not text names no group --
        // and is reported as one that does not exist, because that is what it
        // is.
        Some(name) => match name
            .to_str()
            .and_then(|text| find_group_by_name(&group_db, text))
        {
            Some(g) => g,
            None => {
                eprintln!("newgrp: group {} does not exist", quoteaf_os(name));
                return 1;
            }
        },
        None => {
            // Reset to user's primary group.
            match find_group_by_gid(&group_db, user.gid) {
                Some(g) => g,
                None => {
                    eprintln!("newgrp: cannot find primary group {}", user.gid);
                    return 1;
                }
            }
        }
    };

    // Check membership.
    if !user_is_member(&user, &target_group) {
        // Not a member — need password.
        let password = prompt_password();
        if !verify_group_password(&target_group.name, &password) {
            eprintln!("newgrp: permission denied");
            return 1;
        }
    }

    if login_shell {
        // Would set up a clean environment for login shell.
        eprintln!(
            "newgrp: starting login shell with group {}",
            quoteaf_os(&target_group.name)
        );
    }

    exec_with_group(target_group.gid, None)
}

// ============================================================================
// sg personality
// ============================================================================

fn sg_main(args: &[OsString]) -> i32 {
    if args.is_empty() {
        eprintln!("Usage: sg group [-c command]");
        eprintln!("       sg group [command]");
        return 1;
    }

    let mut i = 0;

    // First positional arg is the group name.
    if i >= args.len() {
        eprintln!("sg: missing group name");
        return 1;
    }

    match args.get(i).and_then(|a| a.to_str()).unwrap_or_default() {
        "--help" => {
            println!("Usage: sg group [-c command]");
            println!("       sg group [command]");
            println!();
            println!("Execute a command as a different group.");
            println!();
            println!("Options:");
            println!("  -c COMMAND  Execute COMMAND");
            println!("  --help      Display this help");
            println!("  --version   Display version");
            return 0;
        }
        "--version" => {
            println!("sg (Slate OS coreutils) {VERSION}");
            return 0;
        }
        _ => {}
    }

    let Some(group_name) = args.get(i).cloned() else {
        eprintln!("sg: missing group name");
        return 1;
    };
    i += 1;

    // The command and its arguments are what will be exec'd, so they are
    // carried as given: an argument is very often a filename, and a filename
    // on this OS may hold any byte but `/` and NUL.
    let mut command_args: Vec<OsString> = Vec::new();

    if args.get(i).is_some_and(|a| a == OsStr::new("-c")) {
        i += 1;
        while i < args.len() {
            command_args.push(args[i].clone());
            i += 1;
        }
    } else {
        while i < args.len() {
            command_args.push(args[i].clone());
            i += 1;
        }
    }

    let Some(user) = get_current_user() else {
        eprintln!("sg: cannot determine who is running this command");
        return 1;
    };
    let group_db = read_group_db();

    // A group name is text, so a name that is not text names no group -- and
    // is reported as one that does not exist, because that is what it is.
    let target_group = match group_name
        .to_str()
        .and_then(|name| find_group_by_name(&group_db, name))
    {
        Some(g) => g,
        None => {
            eprintln!("sg: group {} does not exist", quoteaf_os(&group_name));
            return 1;
        }
    };

    if !user_is_member(&user, &target_group) {
        let password = prompt_password();
        if !verify_group_password(&target_group.name, &password) {
            eprintln!("sg: permission denied");
            return 1;
        }
    }

    let cmd = if command_args.is_empty() {
        None
    } else {
        Some(command_args.as_slice())
    };

    exec_with_group(target_group.gid, cmd)
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    // `args_os`, not `args`: the latter's iterator is a literal `unwrap` and
    // panics on an argument that is not valid UTF-8.
    let args: Vec<OsString> = env::args_os().collect();

    // The personality is read from `argv[0]`, which is a path. A path that
    // cannot be decoded is not one of the two names this binary answers to,
    // so it takes the `newgrp` default -- the same answer it would give for
    // any other unrecognised name.
    let prog_name = {
        let argv0 = args
            .first()
            .cloned()
            .unwrap_or_else(|| OsString::from("newgrp"));
        let text = argv0.to_str().unwrap_or("newgrp");
        let mut last_sep = 0;
        for (i, &b) in text.as_bytes().iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = text.get(last_sep..).unwrap_or(text);
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<OsString> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "sg" => sg_main(&rest),
        _ => newgrp_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A `crypt(3)` entry for `password`, as `/etc/gshadow` would hold one.
    fn entry_for(password: &str) -> String {
        let mut sb = posix::crypt::buf();
        let setting =
            posix::crypt::setting_into(posix::crypt::Method::Sha512, b"grouptest", &mut sb)
                .expect("setting")
                .to_string();
        let mut hb = posix::crypt::buf();
        posix::crypt::hash_into(password.as_bytes(), setting.as_bytes(), &mut hb)
            .expect("hash")
            .to_string()
    }

    /// The check the old stub could not make.
    ///
    /// `_verify_group_password` returned `!password.is_empty()`, so every
    /// assertion below that expects `false` would have been `true` -- any
    /// single character admitted a non-member to any group. These go through
    /// `authlib::check_stored` directly, because `verify_group_password` reads
    /// `/etc/gshadow` and this host has none; what is asserted is the rule the
    /// function applies to the entry once it has it.
    #[test]
    fn a_group_password_is_checked_and_not_merely_non_empty() {
        let stored = entry_for("hunter2");
        assert!(authlib::check_stored(b"hunter2", stored.as_bytes()).is_accepted());
        assert!(!authlib::check_stored(b"x", stored.as_bytes()).is_accepted());
        assert!(!authlib::check_stored(b"wrong", stored.as_bytes()).is_accepted());
        assert!(!authlib::check_stored(b"", stored.as_bytes()).is_accepted());
    }

    /// A locked group admits nobody, with the right password or any other.
    /// The old stub admitted anyone who typed a character.
    #[test]
    fn a_locked_group_admits_nobody() {
        for locked in ["!", "*", "!!"] {
            assert!(
                !authlib::check_stored(b"anything", locked.as_bytes()).is_accepted(),
                "{locked}"
            );
        }
    }

    /// A `/etc/gshadow` entry with the given password field.
    fn group_with(password: &str) -> GshadowEntry {
        GshadowEntry {
            name: "audio".to_string(),
            password: password.to_string(),
            admins: Vec::new(),
            members: Vec::new(),
        }
    }

    /// The whole rule, driven through the function that decides it.
    ///
    /// Every `false` below was `true` under the old stub, which returned
    /// `!password.is_empty()`: any single character opened any group.
    #[test]
    fn only_the_right_password_opens_a_group() {
        let stored = entry_for("hunter2");
        let audio = group_with(&stored);

        assert!(password_opens(Some(&audio), "hunter2"));
        assert!(!password_opens(Some(&audio), "x"));
        assert!(!password_opens(Some(&audio), "wrong"));
        assert!(!password_opens(Some(&audio), ""));
    }

    /// A group whose password field is empty has no password, and `newgrp(1)`
    /// treats that as closed to non-members rather than open to everyone --
    /// the case the old stub inverted exactly, since `""` was the one input it
    /// refused and every other input it accepted.
    #[test]
    fn a_group_with_no_password_is_closed_not_open() {
        let audio = group_with("");
        assert!(!password_opens(Some(&audio), ""));
        assert!(!password_opens(Some(&audio), "anything"));
    }

    /// A group absent from `/etc/gshadow` has no password to match, so nothing
    /// opens it -- including an unreadable file, which reads as absent.
    #[test]
    fn a_group_with_no_gshadow_entry_admits_nobody() {
        assert!(!password_opens(None, "anything"));
        assert!(!password_opens(None, ""));
    }

    /// A locked group, through the same function rather than through
    /// `authlib` directly.
    #[test]
    fn a_locked_group_is_closed_through_the_rule_too() {
        for locked in ["!", "*", "!!"] {
            let audio = group_with(locked);
            assert!(!password_opens(Some(&audio), "anything"), "{locked}");
        }
    }

    /// An argument that a `String` cannot hold. The development host is
    /// Windows, where argv arrives as UTF-16 and the unrepresentable case is
    /// an unpaired surrogate rather than a stray byte -- so the fixture is
    /// written both ways.
    fn not_text() -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            OsString::from_vec(vec![b'a', 0x80, b'b'])
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::ffi::OsStringExt as _;
            OsString::from_wide(&[0x0061, 0xD800, 0x0062])
        }
    }

    /// A group name that is not text is reported as a group that does not
    /// exist, rather than panicking before `main` runs a line of this file --
    /// which is what `env::args()` did, because its iterator is a literal
    /// `unwrap`. See `known-issues.md` ->
    /// `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
    #[test]
    fn a_group_name_that_is_not_text_does_not_exist_rather_than_crashing() {
        let odd = not_text();
        assert!(
            odd.to_str().is_none(),
            "the fixture must be unrepresentable as a `String`, or this test              asserts nothing"
        );
        assert_eq!(newgrp_main(std::slice::from_ref(&odd)), 1);
    }

    /// `sg` reports the same way, and does not read the odd name as an option.
    #[test]
    fn sg_reports_a_group_name_that_is_not_text_as_missing() {
        assert_eq!(sg_main(std::slice::from_ref(&not_text())), 1);
    }

    /// A command argument is carried to the exec as given: it is very often a
    /// filename, and a filename here may hold any byte but `/` and NUL.
    #[test]
    fn a_command_argument_that_is_not_text_survives_to_the_exec() {
        // No group of this name exists, so this reaches the same refusal --
        // what is asserted is that getting there does not panic on the way.
        let args = vec![
            OsString::from("nosuchgroup"),
            OsString::from("-c"),
            OsString::from("cat"),
            not_text(),
        ];
        assert_eq!(sg_main(&args), 1);
    }

    #[test]
    fn test_parse_group_line_basic() {
        let entry = parse_group_line("wheel:x:10:alice,bob").unwrap();
        assert_eq!(entry.name, "wheel");
        assert_eq!(entry.gid, 10);
        assert_eq!(entry.members, vec!["alice", "bob"]);
    }

    #[test]
    fn test_parse_group_line_no_members() {
        let entry = parse_group_line("nogroup:x:65534:").unwrap();
        assert_eq!(entry.name, "nogroup");
        assert_eq!(entry.gid, 65534);
        assert!(entry.members.is_empty());
    }

    #[test]
    fn test_parse_group_line_single_member() {
        let entry = parse_group_line("docker:x:999:alice").unwrap();
        assert_eq!(entry.members, vec!["alice"]);
    }

    #[test]
    fn test_parse_group_line_invalid() {
        assert!(parse_group_line("bad").is_none());
        assert!(parse_group_line("").is_none());
    }

    #[test]
    fn testparse_gshadow_line() {
        let entry = parse_gshadow_line("wheel:!:root:alice,bob").unwrap();
        assert_eq!(entry.name, "wheel");
        assert_eq!(entry.password, "!");
        assert_eq!(entry.admins, vec!["root"]);
        assert_eq!(entry.members, vec!["alice", "bob"]);
    }

    #[test]
    fn test_user_is_member_primary_group() {
        let user = UserInfo {
            username: "alice".to_string(),
            _uid: 1000,
            gid: 1000,
            groups: vec![],
        };
        let group = GroupEntry {
            name: "alice".to_string(),
            gid: 1000,
            members: vec![],
        };
        assert!(user_is_member(&user, &group));
    }

    #[test]
    fn test_user_is_member_explicit() {
        let user = UserInfo {
            username: "alice".to_string(),
            _uid: 1000,
            gid: 1000,
            groups: vec![],
        };
        let group = GroupEntry {
            name: "wheel".to_string(),
            gid: 10,
            members: vec!["alice".to_string(), "bob".to_string()],
        };
        assert!(user_is_member(&user, &group));
    }

    #[test]
    fn test_user_is_member_supplementary() {
        let user = UserInfo {
            username: "alice".to_string(),
            _uid: 1000,
            gid: 1000,
            groups: vec![10, 20],
        };
        let group = GroupEntry {
            name: "wheel".to_string(),
            gid: 10,
            members: vec![],
        };
        assert!(user_is_member(&user, &group));
    }

    #[test]
    fn test_user_not_member() {
        let user = UserInfo {
            username: "alice".to_string(),
            _uid: 1000,
            gid: 1000,
            groups: vec![],
        };
        let group = GroupEntry {
            name: "wheel".to_string(),
            gid: 10,
            members: vec!["bob".to_string()],
        };
        assert!(!user_is_member(&user, &group));
    }

    #[test]
    fn test_find_group_by_name() {
        let groups = vec![
            GroupEntry {
                name: "root".to_string(),
                gid: 0,
                members: vec![],
            },
            GroupEntry {
                name: "wheel".to_string(),
                gid: 10,
                members: vec!["alice".to_string()],
            },
        ];
        let found = find_group_by_name(&groups, "wheel").unwrap();
        assert_eq!(found.gid, 10);
        assert!(find_group_by_name(&groups, "nonexistent").is_none());
    }

    #[test]
    fn test_find_group_by_gid() {
        let groups = vec![
            GroupEntry {
                name: "root".to_string(),
                gid: 0,
                members: vec![],
            },
            GroupEntry {
                name: "wheel".to_string(),
                gid: 10,
                members: vec![],
            },
        ];
        let found = find_group_by_gid(&groups, 10).unwrap();
        assert_eq!(found.name, "wheel");
        assert!(find_group_by_gid(&groups, 999).is_none());
    }
}
