//! SlateOS group switching utility.
//!
//! Multi-personality binary providing:
//! - **newgrp** — log in to a new group
//! - **sg** — execute command as different group
//!
//! Changes the current group ID during a login session, optionally
//! running a command under the new group context.

#![deny(clippy::all)]

use std::env;
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

fn get_user_shell() -> String {
    env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

fn exec_with_group(gid: u32, command: Option<&[String]>) -> i32 {
    // In a real OS, this would call setgid(gid) then exec.
    // Here we simulate by printing what would happen.
    match command {
        Some(cmd) if !cmd.is_empty() => {
            eprintln!(
                "newgrp: would setgid({}) and exec: {}",
                gid,
                cmd.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        _ => {
            let shell = get_user_shell();
            eprintln!("newgrp: would setgid({gid}) and exec: {shell}");
        }
    }
    0
}

// ============================================================================
// newgrp personality
// ============================================================================

fn newgrp_main(args: &[String]) -> i32 {
    let mut group_name: Option<String> = None;
    let mut login_shell = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
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
                println!("newgrp (SlateOS coreutils) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                group_name = Some(s.to_string());
            }
            other => {
                eprintln!("newgrp: invalid option '{other}'");
                return 1;
            }
        }
        i += 1;
    }

    let user = get_current_user();
    let group_db = read_group_db();

    let target_group = match &group_name {
        Some(name) => match find_group_by_name(&group_db, name) {
            Some(g) => g,
            None => {
                eprintln!("newgrp: group '{name}' does not exist");
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
        eprintln!("newgrp: starting login shell with group '{}'", target_group.name);
    }

    exec_with_group(target_group.gid, None)
}

// ============================================================================
// sg personality
// ============================================================================

fn sg_main(args: &[String]) -> i32 {
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

    match args[i].as_str() {
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
            println!("sg (SlateOS coreutils) {VERSION}");
            return 0;
        }
        _ => {}
    }

    let group_name = args[i].clone();
    i += 1;

    let mut command_args: Vec<String> = Vec::new();

    if i < args.len() && args[i] == "-c" {
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

    let target_group = match find_group_by_name(&group_db, &group_name) {
        Some(g) => g,
        None => {
            eprintln!("sg: group '{group_name}' does not exist");
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
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("newgrp");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

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
