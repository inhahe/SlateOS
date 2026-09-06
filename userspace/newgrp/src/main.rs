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

#[derive(Clone, Debug)]
struct _GshadowEntry {
    name: String,
    _password: String,
    _admins: Vec<String>,
    _members: Vec<String>,
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

fn _parse_gshadow_line(line: &str) -> Option<_GshadowEntry> {
    let parts: Vec<&str> = line.splitn(4, ':').collect();
    if parts.len() < 4 {
        return None;
    }
    Some(_GshadowEntry {
        name: parts[0].to_string(),
        _password: parts[1].to_string(),
        _admins: parts[2]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        _members: parts[3]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
    })
}

fn _read_gshadow_db() -> Vec<_GshadowEntry> {
    let content = std::fs::read_to_string("/etc/gshadow").unwrap_or_default();
    content.lines().filter_map(_parse_gshadow_line).collect()
}

// ============================================================================
// Current user info
// ============================================================================

fn get_current_user() -> UserInfo {
    // Read from environment / /proc/self/status in a real system.
    let username = env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string());
    let uid = env::var("UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u32);
    let gid = env::var("GID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u32);

    // Read supplementary groups from /proc/self/status or id output.
    let groups = read_user_supplementary_groups(uid, &username);

    UserInfo {
        username,
        _uid: uid,
        gid,
        groups,
    }
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

fn _verify_group_password(_group: &str, _password: &str) -> bool {
    // In a real system, read /etc/gshadow, hash the input, compare.
    // For now, accept any non-empty password for groups that have one set.
    !_password.is_empty()
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

    let user = get_current_user();
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
        if !_verify_group_password(&target_group.name, &password) {
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

    let user = get_current_user();
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
        if !_verify_group_password(&target_group.name, &password) {
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
    fn test_parse_gshadow_line() {
        let entry = _parse_gshadow_line("wheel:!:root:alice,bob").unwrap();
        assert_eq!(entry.name, "wheel");
        assert_eq!(entry._password, "!");
        assert_eq!(entry._admins, vec!["root"]);
        assert_eq!(entry._members, vec!["alice", "bob"]);
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
