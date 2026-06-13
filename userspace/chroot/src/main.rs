//! SlateOS Change Root Directory Utility
//!
//! Changes the apparent root directory for a command invocation, providing
//! filesystem isolation. Only the superuser (uid 0) may invoke `chroot`.
//!
//! # Usage
//!
//! ```text
//! chroot NEWROOT [COMMAND [ARGS...]]
//! chroot --userspec=USER:GROUP NEWROOT [COMMAND [ARGS...]]
//! chroot --groups=G1,G2,... NEWROOT [COMMAND [ARGS...]]
//! chroot --skip-chdir NEWROOT [COMMAND [ARGS...]]
//! ```
//!
//! If no command is given, `/bin/sh` is executed by default.
//! After changing the root, the working directory is changed to `/`
//! unless `--skip-chdir` is specified.

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_SHELL: &str = "/bin/sh";

// ============================================================================
// DESIGN GAP -- chroot/chdir/setuid/setgid/setgroups have no kernel ABI yet
// ============================================================================
//
// The SlateOS kernel does **not** currently expose syscalls for changing the
// process root directory, working directory, real/effective UID/GID, or
// supplementary group set. There is no SYS_CHROOT, SYS_CHDIR, SYS_SETUID,
// SYS_SETGID, or SYS_SETGROUPS in the kernel's syscall table.
//
// An earlier version of this file hardcoded fake syscall numbers
// (SYS_CHROOT=61, SYS_CHDIR=49, SYS_SETUID=105, SYS_SETGID=106,
// SYS_SETGROUPS=116) that collided with **destructive** unrelated kernel
// syscalls. In particular:
//
//   * SYS_CHROOT=61 collided with SYS_SYSCTL_SET, so `chroot /tmp` would
//     fire `sysctl::set(low_16_bits_of_path_ptr, path_length)` -- silently
//     mutating an arbitrary sysctl to an arbitrary value.
//   * SYS_CHDIR=49 collided with SYS_DMA_DETACH, so a chdir would release
//     a random DMA mapping ID.
//   * SYS_SETUID=105 / SYS_SETGID=106 / SYS_SETGROUPS=116 were unassigned
//     (only 100..103 in that range are wired up), so those calls hit the
//     kernel's unknown-syscall path -- benign but undetectable from here.
//
// The safe and correct interim behavior is for `chroot` to fail with a
// clear "not implemented" error rather than execute any syscall. The
// userland tool stays in the tree so it's ready when the kernel ABI lands;
// see `todo.txt` for the tracking entry that will trigger reinstating the
// real syscalls once they exist.

/// Stub return path for every privilege-changing operation in this tool.
///
/// Returns a `Result::Err` carrying the standard ENOSYS message so callers
/// can surface a clear "not implemented" diagnostic without ever touching
/// the `syscall` instruction.
#[inline]
fn enosys(op: &str) -> Result<(), String> {
    Err(format!(
        "{op}: not implemented in this kernel \
         (no SYS_CHROOT / SYS_CHDIR / SYS_SET*ID ABI yet)"
    ))
}

// ============================================================================
// Privileged-operation stubs (all currently fail safely)
// ============================================================================

/// Change the apparent root directory.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block
/// above for why the previous implementation was removed.
fn do_chroot(_path: &str) -> Result<(), String> {
    enosys("chroot")
}

/// Change the working directory.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block.
fn do_chdir(_path: &str) -> Result<(), String> {
    enosys("chdir")
}

/// Set the real and effective user ID of the calling process.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block.
fn do_setuid(_uid: u32) -> Result<(), String> {
    enosys("setuid")
}

/// Set the real and effective group ID of the calling process.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block.
fn do_setgid(_gid: u32) -> Result<(), String> {
    enosys("setgid")
}

/// Set the supplementary group IDs of the calling process.
///
/// **Currently fails with ENOSYS-equivalent.** See the DESIGN GAP block.
fn do_setgroups(_gids: &[u32]) -> Result<(), String> {
    enosys("setgroups")
}

// ============================================================================
// User/group database reading
// ============================================================================

const USER_DB_PATH: &str = "/etc/users.yaml";

/// A resolved user entry from the SlateOS user database.
struct UserEntry {
    uid: u32,
    username: String,
    groups: Vec<String>,
}

/// A resolved group with a numeric GID.
struct GroupEntry {
    gid: u32,
    name: String,
}

/// Read all users from /etc/users.yaml (same format as useradm/chown).
fn read_users() -> Vec<UserEntry> {
    let content = match fs::read_to_string(USER_DB_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut users = Vec::new();
    let mut uid: u32 = 0;
    let mut username = String::new();
    let mut groups: Vec<String> = Vec::new();
    let mut in_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
            // Flush previous entry.
            if in_entry && !username.is_empty() {
                users.push(UserEntry {
                    uid,
                    username: username.clone(),
                    groups: groups.clone(),
                });
            }
            uid = trimmed
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            username.clear();
            groups.clear();
            in_entry = true;
        } else if in_entry {
            if let Some(val) = trimmed.strip_prefix("username:") {
                username = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("groups:") {
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                groups = val
                    .split(',')
                    .map(|g| g.trim().trim_matches('"').to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            }
        }
    }

    // Flush the last entry.
    if in_entry && !username.is_empty() {
        users.push(UserEntry {
            uid,
            username,
            groups,
        });
    }

    users
}

/// Build the group table by collecting every unique group name from all users
/// and assigning GIDs in order. Well-known groups get fixed IDs.
fn build_group_table(users: &[UserEntry]) -> Vec<GroupEntry> {
    let mut groups = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Well-known groups first.
    for (name, gid) in [("root", 0u32), ("admin", 1), ("users", 100)] {
        groups.push(GroupEntry {
            gid,
            name: name.to_string(),
        });
        seen.insert(name.to_string());
    }

    let mut next_gid: u32 = 101;
    for user in users {
        for g in &user.groups {
            if !seen.contains(g) {
                groups.push(GroupEntry {
                    gid: next_gid,
                    name: g.clone(),
                });
                seen.insert(g.clone());
                next_gid = next_gid.saturating_add(1);
            }
        }
    }

    groups
}

/// Resolve a username or numeric UID string to a UID.
fn resolve_uid(name: &str, users: &[UserEntry]) -> Option<u32> {
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    users.iter().find(|u| u.username == name).map(|u| u.uid)
}

/// Resolve a group name or numeric GID string to a GID.
fn resolve_gid(name: &str, groups: &[GroupEntry]) -> Option<u32> {
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    groups.iter().find(|g| g.name == name).map(|g| g.gid)
}

// ============================================================================
// Caller UID detection
// ============================================================================

/// Get the current (calling) user's UID.
///
/// Tries /proc/self/status first, then falls back to the USER env var
/// matched against the user database, then defaults to u32::MAX (nobody).
fn get_caller_uid(users: &[UserEntry]) -> u32 {
    // Try /proc/self/status for the real UID.
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:")
                && let Some(uid_str) = rest.split_whitespace().next()
                    && let Ok(uid) = uid_str.parse::<u32>() {
                        return uid;
                    }
        }
    }

    // Fallback: resolve USER env var against the database.
    if let Ok(name) = env::var("USER")
        && let Some(user) = users.iter().find(|u| u.username == name) {
            return user.uid;
        }

    // Unknown caller.
    u32::MAX
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line options for chroot.
#[derive(Debug)]
struct Options {
    /// The new root directory path.
    newroot: String,
    /// Command to execute (default: /bin/sh).
    command: String,
    /// Arguments to the command.
    command_args: Vec<String>,
    /// --userspec=USER:GROUP -- user and group to run as after chroot.
    userspec_uid: Option<u32>,
    userspec_gid: Option<u32>,
    /// --groups=G1,G2,... -- supplementary groups.
    supplementary_gids: Vec<u32>,
    /// --skip-chdir -- do not change working directory to / after chroot.
    skip_chdir: bool,
}

/// Parse a `USER:GROUP` specification string.
///
/// Returns `(uid, gid)`. Either side may be absent:
/// - `USER` -> (Some(uid), None)
/// - `USER:GROUP` -> (Some(uid), Some(gid))
/// - `:GROUP` -> (None, Some(gid))
/// - `USER:` -> (Some(uid), None)
fn parse_userspec(
    spec: &str,
    users: &[UserEntry],
    groups: &[GroupEntry],
) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some(colon_pos) = spec.find(':') {
        let user_part = &spec[..colon_pos];
        let group_part = &spec[colon_pos + 1..];

        let uid = if user_part.is_empty() {
            None
        } else {
            Some(
                resolve_uid(user_part, users)
                    .ok_or_else(|| format!("invalid user: '{user_part}'"))?,
            )
        };

        let gid = if group_part.is_empty() {
            None
        } else {
            Some(
                resolve_gid(group_part, groups)
                    .ok_or_else(|| format!("invalid group: '{group_part}'"))?,
            )
        };

        Ok((uid, gid))
    } else {
        // No colon -- just a user.
        let uid = resolve_uid(spec, users)
            .ok_or_else(|| format!("invalid user: '{spec}'"))?;
        Ok((Some(uid), None))
    }
}

/// Parse a comma-separated list of group names or numeric GIDs.
fn parse_group_list(
    list: &str,
    groups: &[GroupEntry],
) -> Result<Vec<u32>, String> {
    let mut gids = Vec::new();
    for item in list.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let gid = resolve_gid(item, groups)
            .ok_or_else(|| format!("invalid group: '{item}'"))?;
        gids.push(gid);
    }
    Ok(gids)
}

/// Parse command-line arguments into an `Options` struct.
fn parse_args(
    args: &[String],
    users: &[UserEntry],
    groups: &[GroupEntry],
) -> Result<Options, String> {
    let mut opts = Options {
        newroot: String::new(),
        command: DEFAULT_SHELL.to_string(),
        command_args: Vec::new(),
        userspec_uid: None,
        userspec_gid: None,
        supplementary_gids: Vec::new(),
        skip_chdir: false,
    };

    let mut i = 1; // skip argv[0]
    let mut found_newroot = false;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" || arg == "-h" {
            return Err(String::new()); // empty error triggers help
        }

        if arg == "--version" || arg == "-V" {
            // Signal version display via a special marker.
            return Err("\x00VERSION".to_string());
        }

        if arg == "--skip-chdir" {
            opts.skip_chdir = true;
            i += 1;
            continue;
        }

        if let Some(val) = arg.strip_prefix("--userspec=") {
            let (uid, gid) = parse_userspec(val, users, groups)?;
            opts.userspec_uid = uid;
            opts.userspec_gid = gid;
            i += 1;
            continue;
        }

        if let Some(val) = arg.strip_prefix("--groups=") {
            opts.supplementary_gids = parse_group_list(val, groups)?;
            i += 1;
            continue;
        }

        // End-of-options marker.
        if arg == "--" {
            i += 1;
            break;
        }

        // Unknown long option.
        if arg.starts_with("--") {
            return Err(format!("unrecognized option: '{arg}'"));
        }

        // First non-option argument is the newroot.
        if !found_newroot {
            opts.newroot = arg.clone();
            found_newroot = true;
            i += 1;
            continue;
        }

        // Second non-option argument is the command.
        opts.command = arg.clone();
        i += 1;

        // Everything after the command is arguments to it.
        while i < args.len() {
            opts.command_args.push(args[i].clone());
            i += 1;
        }
        break;
    }

    // Handle remaining args after `--`.
    while i < args.len() {
        if !found_newroot {
            opts.newroot = args[i].clone();
            found_newroot = true;
        } else if opts.command == DEFAULT_SHELL && opts.command_args.is_empty() {
            // Check if command was already explicitly set; if not, first
            // post-newroot arg after -- is the command.
            opts.command = args[i].clone();
        } else {
            opts.command_args.push(args[i].clone());
        }
        i += 1;
    }

    if !found_newroot {
        return Err("missing operand: NEWROOT".to_string());
    }

    Ok(opts)
}

// ============================================================================
// Path validation
// ============================================================================

/// Check that a path looks like a valid directory for chroot.
///
/// Returns Ok(()) if the path exists and is a directory.
/// Returns Err with a descriptive message otherwise.
fn validate_newroot(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("cannot change root directory to empty path".to_string());
    }

    match fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_dir() {
                Err(format!(
                    "cannot change root directory to '{path}': not a directory"
                ))
            } else {
                Ok(())
            }
        }
        Err(e) => {
            let kind = e.kind();
            match kind {
                std::io::ErrorKind::NotFound => {
                    Err(format!(
                        "cannot change root directory to '{path}': \
                         no such file or directory"
                    ))
                }
                std::io::ErrorKind::PermissionDenied => {
                    Err(format!(
                        "cannot change root directory to '{path}': \
                         permission denied"
                    ))
                }
                _ => {
                    Err(format!(
                        "cannot change root directory to '{path}': {e}"
                    ))
                }
            }
        }
    }
}

// ============================================================================
// Help and version output
// ============================================================================

fn print_help() {
    println!("SlateOS chroot v{VERSION} -- Change root directory and run command");
    println!();
    println!("USAGE:");
    println!("  chroot [OPTIONS] NEWROOT [COMMAND [ARGS...]]");
    println!();
    println!("DESCRIPTION:");
    println!("  Change the root directory to NEWROOT and execute COMMAND.");
    println!("  If no COMMAND is given, run '{DEFAULT_SHELL}'.");
    println!();
    println!("OPTIONS:");
    println!("  --userspec=USER:GROUP   Run command as USER with primary group GROUP");
    println!("  --groups=G1,G2,...      Set supplementary groups");
    println!("  --skip-chdir           Do not change working directory to /");
    println!("  --help, -h             Show this help message");
    println!("  --version, -V          Show version information");
    println!();
    println!("NOTES:");
    println!("  Only root (uid 0) can use chroot.");
    println!("  USER and GROUP may be names (from /etc/users.yaml) or numeric IDs.");
    println!("  The order of privilege operations is: chroot, chdir, setgroups,");
    println!("  setgid, setuid. Credentials are dropped after entering the new root");
    println!("  so that COMMAND runs with reduced privileges.");
    println!();
    println!("EXAMPLES:");
    println!("  chroot /mnt/sysimage");
    println!("  chroot /mnt/sysimage /bin/bash");
    println!("  chroot --userspec=nobody:nogroup /jail /bin/sh");
    println!("  chroot --groups=audio,video --userspec=user:user /sandbox app");
    println!("  chroot --skip-chdir /newroot /bin/pwd");
}

fn print_version() {
    println!("chroot (SlateOS) {VERSION}");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Load the user/group database for name resolution.
    let users = read_users();
    let groups_table = build_group_table(&users);

    let opts = match parse_args(&args, &users, &groups_table) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                print_help();
                process::exit(0);
            }
            if msg == "\x00VERSION" {
                print_version();
                process::exit(0);
            }
            eprintln!("chroot: {msg}");
            eprintln!("Try 'chroot --help' for usage information.");
            process::exit(125);
        }
    };

    // Root privilege check: only uid 0 may use chroot.
    let caller_uid = get_caller_uid(&users);
    if caller_uid != 0 {
        eprintln!(
            "chroot: only root can use chroot (current uid: {caller_uid})"
        );
        process::exit(125);
    }

    // Validate that the new root directory exists and is a directory.
    if let Err(e) = validate_newroot(&opts.newroot) {
        eprintln!("chroot: {e}");
        process::exit(125);
    }

    // Step 1: Change the root directory.
    if let Err(e) = do_chroot(&opts.newroot) {
        eprintln!("chroot: cannot chroot to '{}': {e}", opts.newroot);
        process::exit(125);
    }

    // Step 2: Change working directory to / (unless --skip-chdir).
    if !opts.skip_chdir
        && let Err(e) = do_chdir("/") {
            eprintln!("chroot: cannot change directory to '/': {e}");
            process::exit(125);
        }

    // Step 3: Set supplementary groups (before dropping to non-root).
    if !opts.supplementary_gids.is_empty()
        && let Err(e) = do_setgroups(&opts.supplementary_gids) {
            eprintln!("chroot: failed to set supplementary groups: {e}");
            process::exit(125);
        }

    // Step 4: Set group ID (before user ID -- setgid may fail after setuid
    // drops root privileges).
    if let Some(gid) = opts.userspec_gid
        && let Err(e) = do_setgid(gid) {
            eprintln!("chroot: failed to set group ID to {gid}: {e}");
            process::exit(125);
        }

    // Step 5: Set user ID (last, since this drops root).
    if let Some(uid) = opts.userspec_uid
        && let Err(e) = do_setuid(uid) {
            eprintln!("chroot: failed to set user ID to {uid}: {e}");
            process::exit(125);
        }

    // Step 6: Execute the command.
    let mut cmd = process::Command::new(&opts.command);
    for arg in &opts.command_args {
        cmd.arg(arg);
    }

    let err = cmd.status();
    match err {
        Ok(status) => {
            let code = status.code().unwrap_or(126);
            process::exit(code);
        }
        Err(e) => {
            let kind = e.kind();
            match kind {
                std::io::ErrorKind::NotFound => {
                    eprintln!(
                        "chroot: failed to run command '{}': \
                         no such file or directory",
                        opts.command
                    );
                    process::exit(127);
                }
                std::io::ErrorKind::PermissionDenied => {
                    eprintln!(
                        "chroot: failed to run command '{}': \
                         permission denied",
                        opts.command
                    );
                    process::exit(126);
                }
                _ => {
                    eprintln!(
                        "chroot: failed to run command '{}': {e}",
                        opts.command
                    );
                    process::exit(126);
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Helper: build a test user/group database ----

    fn test_users() -> Vec<UserEntry> {
        vec![
            UserEntry {
                uid: 0,
                username: "root".to_string(),
                groups: vec![
                    "root".to_string(),
                    "admin".to_string(),
                    "wheel".to_string(),
                ],
            },
            UserEntry {
                uid: 1000,
                username: "alice".to_string(),
                groups: vec!["users".to_string(), "audio".to_string()],
            },
            UserEntry {
                uid: 1001,
                username: "bob".to_string(),
                groups: vec!["users".to_string()],
            },
            UserEntry {
                uid: 65534,
                username: "nobody".to_string(),
                groups: vec!["nogroup".to_string()],
            },
        ]
    }

    fn test_groups() -> Vec<GroupEntry> {
        build_group_table(&test_users())
    }

    // ---- Argument parsing: basic cases ----

    #[test]
    fn test_parse_args_newroot_only() {
        let users = test_users();
        let groups = test_groups();
        let args = vec!["chroot".to_string(), "/mnt".to_string()];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.newroot, "/mnt");
        assert_eq!(opts.command, DEFAULT_SHELL);
        assert!(opts.command_args.is_empty());
        assert!(!opts.skip_chdir);
        assert!(opts.userspec_uid.is_none());
        assert!(opts.userspec_gid.is_none());
        assert!(opts.supplementary_gids.is_empty());
    }

    #[test]
    fn test_parse_args_newroot_and_command() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "/jail".to_string(),
            "/bin/bash".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.newroot, "/jail");
        assert_eq!(opts.command, "/bin/bash");
        assert!(opts.command_args.is_empty());
    }

    #[test]
    fn test_parse_args_command_with_arguments() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "/root".to_string(),
            "ls".to_string(),
            "-la".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.newroot, "/root");
        assert_eq!(opts.command, "ls");
        assert_eq!(opts.command_args, vec!["-la", "/tmp"]);
    }

    #[test]
    fn test_parse_args_missing_newroot() {
        let users = test_users();
        let groups = test_groups();
        let args = vec!["chroot".to_string()];
        let err = parse_args(&args, &users, &groups).unwrap_err();
        assert!(err.contains("missing operand"), "got: {err}");
    }

    // ---- Argument parsing: options ----

    #[test]
    fn test_parse_args_skip_chdir() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--skip-chdir".to_string(),
            "/mnt".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert!(opts.skip_chdir);
        assert_eq!(opts.newroot, "/mnt");
    }

    #[test]
    fn test_parse_args_help_returns_empty_error() {
        let users = test_users();
        let groups = test_groups();
        let args = vec!["chroot".to_string(), "--help".to_string()];
        let err = parse_args(&args, &users, &groups).unwrap_err();
        assert!(err.is_empty());
    }

    #[test]
    fn test_parse_args_version_returns_marker() {
        let users = test_users();
        let groups = test_groups();
        let args = vec!["chroot".to_string(), "--version".to_string()];
        let err = parse_args(&args, &users, &groups).unwrap_err();
        assert_eq!(err, "\x00VERSION");
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--bogus".to_string(),
            "/mnt".to_string(),
        ];
        let err = parse_args(&args, &users, &groups).unwrap_err();
        assert!(err.contains("unrecognized option"), "got: {err}");
    }

    // ---- --userspec parsing ----

    #[test]
    fn test_parse_userspec_user_and_group_by_name() {
        let users = test_users();
        let groups = test_groups();
        let (uid, gid) =
            parse_userspec("alice:users", &users, &groups).unwrap();
        assert_eq!(uid, Some(1000));
        assert_eq!(gid, Some(100)); // "users" is well-known gid=100
    }

    #[test]
    fn test_parse_userspec_numeric() {
        let users = test_users();
        let groups = test_groups();
        let (uid, gid) =
            parse_userspec("500:600", &users, &groups).unwrap();
        assert_eq!(uid, Some(500));
        assert_eq!(gid, Some(600));
    }

    #[test]
    fn test_parse_userspec_user_only() {
        let users = test_users();
        let groups = test_groups();
        let (uid, gid) =
            parse_userspec("root", &users, &groups).unwrap();
        assert_eq!(uid, Some(0));
        assert_eq!(gid, None);
    }

    #[test]
    fn test_parse_userspec_group_only() {
        let users = test_users();
        let groups = test_groups();
        let (uid, gid) =
            parse_userspec(":admin", &users, &groups).unwrap();
        assert_eq!(uid, None);
        assert_eq!(gid, Some(1)); // "admin" is well-known gid=1
    }

    #[test]
    fn test_parse_userspec_user_colon_empty() {
        let users = test_users();
        let groups = test_groups();
        let (uid, gid) =
            parse_userspec("bob:", &users, &groups).unwrap();
        assert_eq!(uid, Some(1001));
        assert_eq!(gid, None);
    }

    #[test]
    fn test_parse_userspec_invalid_user() {
        let users = test_users();
        let groups = test_groups();
        let err =
            parse_userspec("nonexistent:users", &users, &groups).unwrap_err();
        assert!(err.contains("invalid user"), "got: {err}");
    }

    #[test]
    fn test_parse_userspec_invalid_group() {
        let users = test_users();
        let groups = test_groups();
        let err =
            parse_userspec("root:nonexistent", &users, &groups).unwrap_err();
        assert!(err.contains("invalid group"), "got: {err}");
    }

    #[test]
    fn test_parse_args_userspec_integration() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--userspec=nobody:nogroup".to_string(),
            "/jail".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.userspec_uid, Some(65534));
        // "nogroup" comes from nobody's groups, so it gets assigned
        // dynamically. Verify it resolved to something.
        assert!(opts.userspec_gid.is_some());
        assert_eq!(opts.newroot, "/jail");
    }

    // ---- --groups parsing ----

    #[test]
    fn test_parse_group_list_by_name() {
        let groups = test_groups();
        let gids = parse_group_list("root,admin", &groups).unwrap();
        assert_eq!(gids, vec![0, 1]);
    }

    #[test]
    fn test_parse_group_list_numeric() {
        let groups = test_groups();
        let gids = parse_group_list("10,20,30", &groups).unwrap();
        assert_eq!(gids, vec![10, 20, 30]);
    }

    #[test]
    fn test_parse_group_list_mixed() {
        let groups = test_groups();
        let gids = parse_group_list("root,42,admin", &groups).unwrap();
        assert_eq!(gids, vec![0, 42, 1]);
    }

    #[test]
    fn test_parse_group_list_single() {
        let groups = test_groups();
        let gids = parse_group_list("users", &groups).unwrap();
        assert_eq!(gids, vec![100]);
    }

    #[test]
    fn test_parse_group_list_invalid() {
        let groups = test_groups();
        let err = parse_group_list("root,bogus", &groups).unwrap_err();
        assert!(err.contains("invalid group"), "got: {err}");
    }

    #[test]
    fn test_parse_group_list_empty_items_skipped() {
        let groups = test_groups();
        let gids = parse_group_list("root,,admin,", &groups).unwrap();
        assert_eq!(gids, vec![0, 1]);
    }

    #[test]
    fn test_parse_args_groups_integration() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--groups=root,admin,users".to_string(),
            "/mnt".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.supplementary_gids, vec![0, 1, 100]);
    }

    // ---- Path validation ----

    #[test]
    fn test_validate_newroot_empty() {
        let err = validate_newroot("").unwrap_err();
        assert!(err.contains("empty path"), "got: {err}");
    }

    #[test]
    fn test_validate_newroot_nonexistent() {
        let err = validate_newroot(
            "/this/path/does/not/exist/chroot_test_9817236"
        )
        .unwrap_err();
        assert!(
            err.contains("no such file") || err.contains("not found")
                || err.contains("cannot change root"),
            "got: {err}"
        );
    }

    // ---- User/group resolution ----

    #[test]
    fn test_resolve_uid_by_name() {
        let users = test_users();
        assert_eq!(resolve_uid("root", &users), Some(0));
        assert_eq!(resolve_uid("alice", &users), Some(1000));
        assert_eq!(resolve_uid("nobody", &users), Some(65534));
    }

    #[test]
    fn test_resolve_uid_numeric() {
        let users = test_users();
        assert_eq!(resolve_uid("0", &users), Some(0));
        assert_eq!(resolve_uid("9999", &users), Some(9999));
    }

    #[test]
    fn test_resolve_uid_nonexistent() {
        let users = test_users();
        assert_eq!(resolve_uid("ghost", &users), None);
    }

    #[test]
    fn test_resolve_gid_by_name() {
        let groups = test_groups();
        assert_eq!(resolve_gid("root", &groups), Some(0));
        assert_eq!(resolve_gid("admin", &groups), Some(1));
        assert_eq!(resolve_gid("users", &groups), Some(100));
    }

    #[test]
    fn test_resolve_gid_numeric() {
        let groups = test_groups();
        assert_eq!(resolve_gid("42", &groups), Some(42));
    }

    #[test]
    fn test_resolve_gid_nonexistent() {
        let groups = test_groups();
        assert_eq!(resolve_gid("phantom", &groups), None);
    }

    // ---- Group table construction ----

    #[test]
    fn test_build_group_table_well_known() {
        let groups = test_groups();
        // Well-known groups should be present with fixed GIDs.
        let root = groups.iter().find(|g| g.name == "root").unwrap();
        assert_eq!(root.gid, 0);
        let admin = groups.iter().find(|g| g.name == "admin").unwrap();
        assert_eq!(admin.gid, 1);
        let users_grp = groups.iter().find(|g| g.name == "users").unwrap();
        assert_eq!(users_grp.gid, 100);
    }

    #[test]
    fn test_build_group_table_dynamic_groups() {
        let groups = test_groups();
        // "wheel", "audio", "nogroup" should be dynamically assigned.
        let wheel = groups.iter().find(|g| g.name == "wheel");
        assert!(wheel.is_some());
        let audio = groups.iter().find(|g| g.name == "audio");
        assert!(audio.is_some());
        let nogroup = groups.iter().find(|g| g.name == "nogroup");
        assert!(nogroup.is_some());
    }

    #[test]
    fn test_build_group_table_no_duplicates() {
        let groups = test_groups();
        let mut names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate group names found");
    }

    // ---- Combined option parsing ----

    #[test]
    fn test_parse_args_all_options() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--userspec=alice:users".to_string(),
            "--groups=root,admin".to_string(),
            "--skip-chdir".to_string(),
            "/sandbox".to_string(),
            "/usr/bin/app".to_string(),
            "--flag".to_string(),
            "value".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.newroot, "/sandbox");
        assert_eq!(opts.command, "/usr/bin/app");
        assert_eq!(opts.command_args, vec!["--flag", "value"]);
        assert!(opts.skip_chdir);
        assert_eq!(opts.userspec_uid, Some(1000));
        assert_eq!(opts.userspec_gid, Some(100));
        assert_eq!(opts.supplementary_gids, vec![0, 1]);
    }

    #[test]
    fn test_parse_args_options_before_newroot() {
        let users = test_users();
        let groups = test_groups();
        let args = vec![
            "chroot".to_string(),
            "--skip-chdir".to_string(),
            "--userspec=0:0".to_string(),
            "/chroot-dir".to_string(),
        ];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert!(opts.skip_chdir);
        assert_eq!(opts.userspec_uid, Some(0));
        assert_eq!(opts.userspec_gid, Some(0));
        assert_eq!(opts.newroot, "/chroot-dir");
    }

    // ---- ENOSYS stubs for chroot/chdir/setuid/setgid/setgroups ----
    //
    // These confirm the privilege-changing wrappers fail safely instead of
    // firing destructive syscalls (see the DESIGN GAP block near the top
    // of this file).

    #[test]
    fn test_do_chroot_returns_enosys() {
        let err = do_chroot("/nowhere").unwrap_err();
        assert!(err.contains("chroot"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_do_chdir_returns_enosys() {
        let err = do_chdir("/").unwrap_err();
        assert!(err.contains("chdir"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_do_setuid_returns_enosys() {
        let err = do_setuid(1000).unwrap_err();
        assert!(err.contains("setuid"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_do_setgid_returns_enosys() {
        let err = do_setgid(1000).unwrap_err();
        assert!(err.contains("setgid"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    #[test]
    fn test_do_setgroups_returns_enosys() {
        let err = do_setgroups(&[100, 101]).unwrap_err();
        assert!(err.contains("setgroups"), "got: {err}");
        assert!(err.contains("not implemented"), "got: {err}");
    }

    // ---- Default command ----

    #[test]
    fn test_default_command_is_bin_sh() {
        assert_eq!(DEFAULT_SHELL, "/bin/sh");
    }

    #[test]
    fn test_parse_args_default_command() {
        let users = test_users();
        let groups = test_groups();
        let args = vec!["chroot".to_string(), "/newroot".to_string()];
        let opts = parse_args(&args, &users, &groups).unwrap();
        assert_eq!(opts.command, "/bin/sh");
    }

    // ---- Version constant ----

    #[test]
    fn test_version_not_empty() {
        assert!(!VERSION.is_empty());
        // Should look like a semver string.
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
    }
}
