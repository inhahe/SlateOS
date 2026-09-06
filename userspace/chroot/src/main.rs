//! Slate OS Change Root Directory Utility
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

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::process;

use pwdb::Db;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_SHELL: &str = "/bin/sh";

// ============================================================================
// DESIGN GAP -- chroot/chdir/setuid/setgid/setgroups have no kernel ABI yet
// ============================================================================
//
// The Slate OS kernel does **not** currently expose syscalls for changing the
// process root directory, working directory, or supplementary group set.
// There is no SYS_CHROOT, SYS_CHDIR or SYS_SETGROUPS in the syscall table.
//
// It *does* expose SYS_PROCESS_SET_CREDENTIALS (530), which sets uid and gid,
// and `posix::unistd::setuid`/`setgid` are live on it -- so two of the five
// operations this block used to list as missing are not. This tool still
// refuses all five, and that is the point rather than an oversight: dropping
// privileges without changing the root would leave the caller believing they
// were sandboxed when they were not, which is a worse failure than refusing.
// The privilege drop goes in after the root change, not before, and not
// alone. Asked for in
// `requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.
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
//     The first two have a real home now (530, above); the third does not.
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

/// Read `/etc/passwd` and `/etc/group`.
///
/// An unreadable or absent file is an empty table, which is `pwdb`'s rule and
/// glibc's: names then cannot be resolved, so `--userspec alice:users` fails
/// with "invalid user" rather than silently running as somebody else.
///
/// This used to read `/etc/users.yaml`, and to *invent* the group table:
/// `root`, `admin` and `users` were given fixed ids and every other group name
/// mentioned by any account was numbered from 101 in order of appearance.
/// Those numbers were not the system's, so `--groups audio` dropped into the
/// sandbox with a gid no other program agreed with -- and one that changed
/// when an account was added. `/etc/group` has the real numbers, and since
/// §353 it is generated from the same database, so there is no longer a second
/// answer for it to disagree with.
fn read_db() -> Db {
    Db::load()
}

/// Resolve a username to a UID.
fn resolve_uid(name: &str, db: &Db) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    db.user_by_name(name.as_bytes()).map(|u| u.uid)
}

/// Resolve a group name to a GID.
fn resolve_gid(name: &str, db: &Db) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    db.group_by_name(name.as_bytes()).map(|g| g.gid)
}

// ============================================================================
// Caller UID detection
// ============================================================================

/// Get the current (calling) user's UID.
///
/// Tries /proc/self/status first, then falls back to the USER env var
/// matched against the user database, then defaults to u32::MAX (nobody).
fn get_caller_uid(db: &Db) -> u32 {
    // Try /proc/self/status for the real UID.
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:")
                && let Some(uid_str) = rest.split_whitespace().next()
                && let Ok(uid) = uid_str.parse::<u32>()
            {
                return uid;
            }
        }
    }

    // Fallback: resolve USER env var against the database.
    if let Ok(name) = env::var("USER")
        && let Some(user) = db.user_by_name(name.as_bytes())
    {
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
fn parse_userspec(spec: &str, db: &Db) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some(colon_pos) = spec.find(':') {
        let user_part = &spec[..colon_pos];
        let group_part = &spec[colon_pos + 1..];

        let uid = if user_part.is_empty() {
            None
        } else {
            Some(resolve_uid(user_part, db).ok_or_else(|| format!("invalid user: '{user_part}'"))?)
        };

        let gid = if group_part.is_empty() {
            None
        } else {
            Some(
                resolve_gid(group_part, db)
                    .ok_or_else(|| format!("invalid group: '{group_part}'"))?,
            )
        };

        Ok((uid, gid))
    } else {
        // No colon -- just a user.
        let uid = resolve_uid(spec, db).ok_or_else(|| format!("invalid user: '{spec}'"))?;
        Ok((Some(uid), None))
    }
}

/// Parse a comma-separated list of group names or numeric GIDs.
fn parse_group_list(list: &str, db: &Db) -> Result<Vec<u32>, String> {
    let mut gids = Vec::new();
    for item in list.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let gid = resolve_gid(item, db).ok_or_else(|| format!("invalid group: '{item}'"))?;
        gids.push(gid);
    }
    Ok(gids)
}

/// Parse command-line arguments into an `Options` struct.
fn parse_args(args: &[String], db: &Db) -> Result<Options, String> {
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
            let (uid, gid) = parse_userspec(val, db)?;
            opts.userspec_uid = uid;
            opts.userspec_gid = gid;
            i += 1;
            continue;
        }

        if let Some(val) = arg.strip_prefix("--groups=") {
            opts.supplementary_gids = parse_group_list(val, db)?;
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
                std::io::ErrorKind::NotFound => Err(format!(
                    "cannot change root directory to '{path}': \
                         no such file or directory"
                )),
                std::io::ErrorKind::PermissionDenied => Err(format!(
                    "cannot change root directory to '{path}': \
                         permission denied"
                )),
                _ => Err(format!("cannot change root directory to '{path}': {e}")),
            }
        }
    }
}

// ============================================================================
// Help and version output
// ============================================================================

fn print_help() {
    println!("Slate OS chroot v{VERSION} -- Change root directory and run command");
    println!();
    println!("USAGE:");
    println!("  chroot [OPTIONS] NEWROOT [COMMAND [ARGS...]]");
    println!();
    println!("DESCRIPTION:");
    println!("  Change the root directory to NEWROOT and execute COMMAND.");
    println!(
        "  If no COMMAND is given, run {}.",
        quoteaf_os(DEFAULT_SHELL)
    );
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
    println!("chroot (Slate OS) {VERSION}");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Load the user/group database for name resolution.
    let db = read_db();

    let opts = match parse_args(&args, &db) {
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
    let caller_uid = get_caller_uid(&db);
    if caller_uid != 0 {
        eprintln!("chroot: only root can use chroot (current uid: {caller_uid})");
        process::exit(125);
    }

    // Validate that the new root directory exists and is a directory.
    if let Err(e) = validate_newroot(&opts.newroot) {
        eprintln!("chroot: {e}");
        process::exit(125);
    }

    // Step 1: Change the root directory.
    if let Err(e) = do_chroot(&opts.newroot) {
        eprintln!(
            "chroot: cannot chroot to {}: {e}",
            quoteaf_os(&opts.newroot)
        );
        process::exit(125);
    }

    // Step 2: Change working directory to / (unless --skip-chdir).
    if !opts.skip_chdir
        && let Err(e) = do_chdir("/")
    {
        eprintln!("chroot: cannot change directory to '/': {e}");
        process::exit(125);
    }

    // Step 3: Set supplementary groups (before dropping to non-root).
    if !opts.supplementary_gids.is_empty()
        && let Err(e) = do_setgroups(&opts.supplementary_gids)
    {
        eprintln!("chroot: failed to set supplementary groups: {e}");
        process::exit(125);
    }

    // Step 4: Set group ID (before user ID -- setgid may fail after setuid
    // drops root privileges).
    if let Some(gid) = opts.userspec_gid
        && let Err(e) = do_setgid(gid)
    {
        eprintln!("chroot: failed to set group ID to {gid}: {e}");
        process::exit(125);
    }

    // Step 5: Set user ID (last, since this drops root).
    if let Some(uid) = opts.userspec_uid
        && let Err(e) = do_setuid(uid)
    {
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
                        "chroot: failed to run command {}: no such file or directory",
                        quoteaf_os(&opts.command)
                    );
                    process::exit(127);
                }
                std::io::ErrorKind::PermissionDenied => {
                    eprintln!(
                        "chroot: failed to run command {}: permission denied",
                        quoteaf_os(&opts.command)
                    );
                    process::exit(126);
                }
                _ => {
                    eprintln!(
                        "chroot: failed to run command {}: {e}",
                        quoteaf_os(&opts.command)
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

    /// A database in the form `useradm` writes it.
    ///
    /// The two files a POSIX system turns a name into a number with.
    ///
    /// Written as text because that is what they are, and because since
    /// `design-decisions.md` §353 they are *generated* from `/etc/users.yaml`
    /// -- so a fixture written by hand has the same shape as one the generator
    /// produces, and one test below checks exactly that.
    ///
    /// The gids are the system's. This crate used to number groups itself,
    /// from 101, in the order it happened to meet them across the accounts.
    fn test_db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:100:Alice:/home/alice:/bin/sh\n\
              bob:x:1001:100:Bob:/home/bob:/bin/sh\n\
              nobody:x:65534:65534:Nobody:/:/sbin/nologin\n",
            b"root:x:0:\n\
              admin:x:1:\n\
              wheel:x:10:root\n\
              audio:x:29:alice\n\
              users:x:100:alice,bob\n\
              nogroup:x:65534:nobody\n",
        )
    }

    // ---- Argument parsing: basic cases ----

    #[test]
    fn test_parse_args_newroot_only() {
        let db = test_db();
        let args = vec!["chroot".to_string(), "/mnt".to_string()];
        let opts = parse_args(&args, &db).unwrap();
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
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "/jail".to_string(),
            "/bin/bash".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
        assert_eq!(opts.newroot, "/jail");
        assert_eq!(opts.command, "/bin/bash");
        assert!(opts.command_args.is_empty());
    }

    #[test]
    fn test_parse_args_command_with_arguments() {
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "/root".to_string(),
            "ls".to_string(),
            "-la".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
        assert_eq!(opts.newroot, "/root");
        assert_eq!(opts.command, "ls");
        assert_eq!(opts.command_args, vec!["-la", "/tmp"]);
    }

    #[test]
    fn test_parse_args_missing_newroot() {
        let db = test_db();
        let args = vec!["chroot".to_string()];
        let err = parse_args(&args, &db).unwrap_err();
        assert!(err.contains("missing operand"), "got: {err}");
    }

    // ---- Argument parsing: options ----

    #[test]
    fn test_parse_args_skip_chdir() {
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "--skip-chdir".to_string(),
            "/mnt".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
        assert!(opts.skip_chdir);
        assert_eq!(opts.newroot, "/mnt");
    }

    #[test]
    fn test_parse_args_help_returns_empty_error() {
        let db = test_db();
        let args = vec!["chroot".to_string(), "--help".to_string()];
        let err = parse_args(&args, &db).unwrap_err();
        assert!(err.is_empty());
    }

    #[test]
    fn test_parse_args_version_returns_marker() {
        let db = test_db();
        let args = vec!["chroot".to_string(), "--version".to_string()];
        let err = parse_args(&args, &db).unwrap_err();
        assert_eq!(err, "\x00VERSION");
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "--bogus".to_string(),
            "/mnt".to_string(),
        ];
        let err = parse_args(&args, &db).unwrap_err();
        assert!(err.contains("unrecognized option"), "got: {err}");
    }

    // ---- --userspec parsing ----

    #[test]
    fn test_parse_userspec_user_and_group_by_name() {
        let db = test_db();
        let (uid, gid) = parse_userspec("alice:users", &db).unwrap();
        assert_eq!(uid, Some(1000));
        assert_eq!(gid, Some(100)); // "users" is well-known gid=100
    }

    #[test]
    fn test_parse_userspec_numeric() {
        let db = test_db();
        let (uid, gid) = parse_userspec("500:600", &db).unwrap();
        assert_eq!(uid, Some(500));
        assert_eq!(gid, Some(600));
    }

    #[test]
    fn test_parse_userspec_user_only() {
        let db = test_db();
        let (uid, gid) = parse_userspec("root", &db).unwrap();
        assert_eq!(uid, Some(0));
        assert_eq!(gid, None);
    }

    #[test]
    fn test_parse_userspec_group_only() {
        let db = test_db();
        let (uid, gid) = parse_userspec(":admin", &db).unwrap();
        assert_eq!(uid, None);
        assert_eq!(gid, Some(1)); // "admin" is well-known gid=1
    }

    #[test]
    fn test_parse_userspec_user_colon_empty() {
        let db = test_db();
        let (uid, gid) = parse_userspec("bob:", &db).unwrap();
        assert_eq!(uid, Some(1001));
        assert_eq!(gid, None);
    }

    #[test]
    fn test_parse_userspec_invalid_user() {
        let db = test_db();
        let err = parse_userspec("nonexistent:users", &db).unwrap_err();
        assert!(err.contains("invalid user"), "got: {err}");
    }

    #[test]
    fn test_parse_userspec_invalid_group() {
        let db = test_db();
        let err = parse_userspec("root:nonexistent", &db).unwrap_err();
        assert!(err.contains("invalid group"), "got: {err}");
    }

    #[test]
    fn test_parse_args_userspec_integration() {
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "--userspec=nobody:nogroup".to_string(),
            "/jail".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
        assert_eq!(opts.userspec_uid, Some(65534));
        // "nogroup" comes from nobody's groups, so it gets assigned
        // dynamically. Verify it resolved to something.
        assert!(opts.userspec_gid.is_some());
        assert_eq!(opts.newroot, "/jail");
    }

    // ---- --groups parsing ----

    #[test]
    fn test_parse_group_list_by_name() {
        let db = test_db();
        let gids = parse_group_list("root,admin", &db).unwrap();
        assert_eq!(gids, vec![0, 1]);
    }

    #[test]
    fn test_parse_group_list_numeric() {
        let db = test_db();
        let gids = parse_group_list("10,20,30", &db).unwrap();
        assert_eq!(gids, vec![10, 20, 30]);
    }

    #[test]
    fn test_parse_group_list_mixed() {
        let db = test_db();
        let gids = parse_group_list("root,42,admin", &db).unwrap();
        assert_eq!(gids, vec![0, 42, 1]);
    }

    #[test]
    fn test_parse_group_list_single() {
        let db = test_db();
        let gids = parse_group_list("users", &db).unwrap();
        assert_eq!(gids, vec![100]);
    }

    #[test]
    fn test_parse_group_list_invalid() {
        let db = test_db();
        let err = parse_group_list("root,bogus", &db).unwrap_err();
        assert!(err.contains("invalid group"), "got: {err}");
    }

    #[test]
    fn test_parse_group_list_empty_items_skipped() {
        let db = test_db();
        let gids = parse_group_list("root,,admin,", &db).unwrap();
        assert_eq!(gids, vec![0, 1]);
    }

    #[test]
    fn test_parse_args_groups_integration() {
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "--groups=root,admin,users".to_string(),
            "/mnt".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
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
        let err = validate_newroot("/this/path/does/not/exist/chroot_test_9817236").unwrap_err();
        assert!(
            err.contains("no such file")
                || err.contains("not found")
                || err.contains("cannot change root"),
            "got: {err}"
        );
    }

    // ---- User/group resolution ----

    #[test]
    fn test_resolve_uid_by_name() {
        let db = test_db();
        assert_eq!(resolve_uid("root", &db), Some(0));
        assert_eq!(resolve_uid("alice", &db), Some(1000));
        assert_eq!(resolve_uid("nobody", &db), Some(65534));
    }

    #[test]
    fn test_resolve_uid_numeric() {
        let db = test_db();
        assert_eq!(resolve_uid("0", &db), Some(0));
        assert_eq!(resolve_uid("9999", &db), Some(9999));
    }

    #[test]
    fn test_resolve_uid_nonexistent() {
        let db = test_db();
        assert_eq!(resolve_uid("ghost", &db), None);
    }

    /// `wheel` is a group because `/etc/group` says so.
    ///
    /// This crate used to add `wheel` to its invented table for any account
    /// with `is_admin: true`, because administrator-ness is a flag in the
    /// database rather than a group. That was a reasonable patch over an
    /// invented table and it is the wrong answer now: a gid a process can
    /// actually be given has to be one `/etc/group` names, and if `wheel` is
    /// not in that file then nothing on the system is in it.
    #[test]
    fn a_group_that_is_not_in_the_group_file_does_not_resolve() {
        let db = test_db();
        assert_eq!(resolve_gid("wheel", &db), Some(10));

        let without = Db::from_bytes(b"alice:x:1000:1000::/home/alice:/bin/sh\n", b"");
        assert_eq!(resolve_gid("wheel", &without), None);
    }

    /// A userspec resolves through the file the generator produced.
    ///
    /// The point the round-trip test used to make, one step further along:
    /// `userdb::UserDb::save` *generates* `/etc/passwd`, and this crate reads
    /// the generated bytes. A reader and a writer that disagree can only be
    /// seen to disagree at the step where one consumes the other's output.
    #[test]
    fn a_userspec_resolves_through_a_passwd_file_the_generator_produced() {
        let scratch = scratchdir::ScratchDir::new("chroot-generated");
        let path = scratch.path("users.yaml");
        let mut db = userdb::UserDb::new();
        let mut alice = userdb::Record::new();
        alice.set_uid(1000);
        alice.set_gid(100);
        alice.set(userdb::field::USERNAME, "alice");
        db.push(alice);
        db.save(&path).expect("save");

        let passwd = std::fs::read(scratch.path(userdb::PASSWD_NAME)).expect("generated passwd");
        let read = Db::from_bytes(&passwd, b"users:x:100:alice\n");
        assert_eq!(
            parse_userspec("alice:users", &read),
            Ok((Some(1000), Some(100)))
        );
    }

    #[test]
    fn test_resolve_gid_by_name() {
        let db = test_db();
        assert_eq!(resolve_gid("root", &db), Some(0));
        assert_eq!(resolve_gid("admin", &db), Some(1));
        assert_eq!(resolve_gid("users", &db), Some(100));
    }

    #[test]
    fn test_resolve_gid_numeric() {
        let db = test_db();
        assert_eq!(resolve_gid("42", &db), Some(42));
    }

    #[test]
    fn test_resolve_gid_nonexistent() {
        let db = test_db();
        assert_eq!(resolve_gid("phantom", &db), None);
    }

    // ---- Group table construction ----

    /// Every group in the file resolves to the id the file gives it. There is
    /// no table to build any more, and so no "well-known" ids and no
    /// dynamically-assigned ones -- the three tests that stood here were about
    /// a numbering this crate invented.
    #[test]
    fn every_group_in_the_file_resolves_to_its_own_id() {
        let db = test_db();
        for (name, gid) in [
            ("root", 0),
            ("admin", 1),
            ("wheel", 10),
            ("audio", 29),
            ("users", 100),
            ("nogroup", 65534),
        ] {
            assert_eq!(resolve_gid(name, &db), Some(gid), "{name}");
        }
    }

    /// A name that is in neither file resolves to nothing, rather than to a
    /// number that happened to be free.
    #[test]
    fn a_name_that_is_in_neither_file_resolves_to_nothing() {
        let db = test_db();
        assert_eq!(resolve_gid("phantom", &db), None);
        assert_eq!(resolve_uid("phantom", &db), None);
    }

    // ---- Combined option parsing ----

    #[test]
    fn test_parse_args_all_options() {
        let db = test_db();
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
        let opts = parse_args(&args, &db).unwrap();
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
        let db = test_db();
        let args = vec![
            "chroot".to_string(),
            "--skip-chdir".to_string(),
            "--userspec=0:0".to_string(),
            "/chroot-dir".to_string(),
        ];
        let opts = parse_args(&args, &db).unwrap();
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
        let db = test_db();
        let args = vec!["chroot".to_string(), "/newroot".to_string()];
        let opts = parse_args(&args, &db).unwrap();
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
