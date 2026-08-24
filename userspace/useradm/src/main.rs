//! Slate OS User Account Management
//!
//! Unified tool for creating, modifying, and deleting user accounts.
//! Manages `/etc/users.yaml` (our OS's user database format) and user home
//! directories.
//!
//! The format itself — the parser, the serialiser and the password hash —
//! lives in the [`userdb`] crate, not here. It used to live here *as well as*
//! in the login manager, in two versions that disagreed about the name of the
//! salt field and about what was hashed, with the result that an account
//! created with `useradm add` could not log in. A file format with two
//! implementations is a file format with no definition; see
//! `design-decisions.md` §330.
//!
//! # Usage
//!
//! ```text
//! useradm add <username>           Create a new user
//! useradm del <username>           Delete a user
//! useradm mod <username> [opts]    Modify a user
//! useradm passwd <username>        Change a user's password
//! useradm list                     List all users
//! useradm info <username>          Show user details
//! useradm lock <username>          Lock account (disable login)
//! useradm unlock <username>        Unlock account
//! useradm groups <username>        Show group memberships
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

use userdb::{Record, UserDb, field};

// ============================================================================
// The database
// ============================================================================

/// Load the database, or exit.
///
/// An unreadable database is a hard error rather than an empty one. The
/// previous code returned an empty `Vec` for *any* read failure, and every
/// command that writes would then have saved that empty database over the real
/// one — so running `useradm lock alice` without permission to read
/// `/etc/users.yaml` would have deleted every account on the machine if it
/// could write.
fn load_db() -> UserDb {
    match UserDb::load(userdb::DEFAULT_PATH) {
        Ok(db) => db,
        Err(e) => {
            eprintln!(
                "error: cannot read {}: {e}\n\
                 refusing to continue, because writing now would replace the \
                 user database with an empty one",
                userdb::DEFAULT_PATH
            );
            process::exit(1);
        }
    }
}

/// Save the database, or exit.
fn save_db(db: &UserDb) {
    if let Err(e) = db.save(userdb::DEFAULT_PATH) {
        eprintln!("error writing {}: {e}", userdb::DEFAULT_PATH);
        process::exit(1);
    }
}

/// The record for `username`, or exit with the standard message.
fn require_user<'a>(db: &'a UserDb, username: &str) -> &'a Record {
    match db.find(username) {
        Some(r) => r,
        None => {
            eprintln!("error: user {} not found", quoteaf_os(username));
            process::exit(1);
        }
    }
}

/// The mutable record for `username`, or exit with the standard message.
fn require_user_mut<'a>(db: &'a mut UserDb, username: &str) -> &'a mut Record {
    if db.find(username).is_none() {
        eprintln!("error: user {} not found", quoteaf_os(username));
        process::exit(1);
    }
    match db.find_mut(username) {
        Some(r) => r,
        // Unreachable: the immutable lookup above just succeeded, and nothing
        // mutates the database in between.
        None => process::exit(1),
    }
}

/// A record's display name, falling back to the login name.
fn display_name(record: &Record) -> String {
    record
        .get(field::DISPLAY_NAME)
        .or_else(|| record.username())
        .unwrap_or_default()
}

// ============================================================================
// Passwords
// ============================================================================

/// Prompt for a line on stdin.
///
/// The password is echoed, which it should not be. Turning echo off needs the
/// termios ioctls, which the kernel does not yet serve; `userspace/passwd` has
/// the same limitation and the same note, and both will move to one helper
/// when the ioctl lands.
fn prompt(text: &str) -> String {
    print!("{text}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        eprintln!("error reading input");
        process::exit(1);
    }
    line.trim().to_string()
}

/// Prompt for a new password twice and store it in `record`.
///
/// Hashing happens through [`userdb`], which means through `crypt(3)` — the
/// same call the login manager verifies with. Nothing here composes a hash out
/// of a salt and a password by hand, which is what the three previous
/// implementations each did differently.
fn set_new_password(record: &mut Record, username: &str) {
    let password = prompt(&format!("New password for {username}: "));
    if password.is_empty() {
        eprintln!("error: password cannot be empty");
        process::exit(1);
    }
    let confirm = prompt("Confirm password: ");
    if password != confirm {
        eprintln!("error: passwords do not match");
        process::exit(1);
    }

    if let Err(e) = record.set_password(&password) {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_add(username: &str, args: &[String]) {
    let mut db = load_db();

    if db.find(username).is_some() {
        eprintln!("error: user {} already exists", quoteaf_os(username));
        process::exit(1);
    }

    // Validate username.
    if username.is_empty() || username.len() > 32 {
        eprintln!("error: username must be 1-32 characters");
        process::exit(1);
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        eprintln!("error: username must be alphanumeric (plus _ - .)");
        process::exit(1);
    }

    // Next UID: one past the highest in use, and never below 1000, which is
    // where the system accounts stop.
    let next_uid = db
        .records()
        .iter()
        .filter_map(Record::uid)
        .max()
        .unwrap_or(999)
        .saturating_add(1)
        .max(1000);

    let mut record = Record::new();
    record.set_uid(next_uid);
    record.set(field::USERNAME, username);
    record.set(field::DISPLAY_NAME, username);
    record.set(field::SHELL, "/bin/sh");
    record.set_home(&format!("/home/{username}"));
    record.set_groups(&["users".to_string()]);
    record.set_admin(false);
    record.set_locked(false);

    // Parse optional arguments.
    let mut i = 0;
    while i < args.len() {
        let Some(flag) = args.get(i).map(String::as_str) else {
            break;
        };
        let value = args.get(i.saturating_add(1));
        match (flag, value) {
            ("--shell" | "-s", Some(v)) => {
                record.set(field::SHELL, v);
                i = i.saturating_add(2);
            }
            ("--home" | "-d", Some(v)) => {
                record.set_home(v);
                i = i.saturating_add(2);
            }
            ("--name" | "-c", Some(v)) => {
                record.set(field::DISPLAY_NAME, v);
                i = i.saturating_add(2);
            }
            ("--groups" | "-G", Some(v)) => {
                record.set_groups(&split_groups(v));
                i = i.saturating_add(2);
            }
            ("--uid" | "-u", Some(v)) => {
                match v.parse::<u32>() {
                    Ok(uid) => record.set_uid(uid),
                    Err(_) => {
                        eprintln!("error: --uid expects a number, got {}", quoteaf_os(v));
                        process::exit(1);
                    }
                }
                i = i.saturating_add(2);
            }
            ("--admin", _) => {
                grant_admin(&mut record);
                i = i.saturating_add(1);
            }
            _ => i = i.saturating_add(1),
        }
    }

    if let Some(uid) = record.uid()
        && let Some(existing) = db.find_uid(uid)
    {
        eprintln!(
            "error: uid {uid} is already used by {}",
            quoteaf_os(existing.username().unwrap_or_default())
        );
        process::exit(1);
    }

    set_new_password(&mut record, username);

    let home = record.home().unwrap_or_default();
    let uid = record.uid().unwrap_or_default();
    db.push(record);
    save_db(&db);

    // Create home directory.
    if let Err(e) = fs::create_dir_all(&home) {
        eprintln!("warning: could not create home directory {home}: {e}");
    }

    println!(
        "Created user {} (uid={uid}, home={home})",
        quoteaf_os(username)
    );
}

/// Split a comma-separated `--groups` value.
fn split_groups(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect()
}

/// Mark a record as an administrator, and put it in the `admin` group.
///
/// Both, because they are two different questions: the flag is what the login
/// manager and the settings app read, and the group is what `sudo` reads.
fn grant_admin(record: &mut Record) {
    record.set_admin(true);
    let mut groups = record.groups();
    if !groups.iter().any(|g| g == "admin") {
        groups.push("admin".to_string());
        record.set_groups(&groups);
    }
}

fn cmd_del(username: &str) {
    // Prevent deleting root.
    if username == "root" {
        eprintln!("error: cannot delete root user");
        process::exit(1);
    }

    let mut db = load_db();
    let home = require_user(&db, username).home();

    if !db.remove(username) {
        eprintln!("error: user {} not found", quoteaf_os(username));
        process::exit(1);
    }
    save_db(&db);

    // Optionally remove home directory. The record's own home is used rather
    // than a guess at `/home/<name>`, so an account with a home somewhere else
    // does not leave its files behind while the prompt asks about a directory
    // that was never its.
    if let Some(home) = home
        && std::path::Path::new(&home).exists()
    {
        let answer = prompt(&format!("Remove home directory {home}? [y/N] "));
        if answer.eq_ignore_ascii_case("y") {
            match fs::remove_dir_all(&home) {
                Ok(()) => println!("Removed {home}"),
                Err(e) => eprintln!("warning: could not remove {home}: {e}"),
            }
        }
    }

    println!("Deleted user {}", quoteaf_os(username));
}

fn cmd_passwd(username: &str) {
    let mut db = load_db();
    let record = require_user_mut(&mut db, username);
    set_new_password(record, username);
    save_db(&db);
    println!("Password updated for {}", quoteaf_os(username));
}

fn cmd_list() {
    let db = load_db();

    if db.records().is_empty() {
        println!("No users found (is {} readable?)", userdb::DEFAULT_PATH);
        return;
    }

    println!(
        "{:<6} {:<16} {:<24} {:<16} {:<6} Groups",
        "UID", "Username", "Display Name", "Shell", "Admin"
    );
    println!(
        "{:<6} {:<16} {:<24} {:<16} {:<6} ------",
        "---", "--------", "------------", "-----", "-----"
    );

    for record in db.records() {
        let uid = record
            .uid()
            .map_or_else(|| "?".to_string(), |u| u.to_string());
        println!(
            "{:<6} {:<16} {:<24} {:<16} {:<6} {}{}",
            uid,
            record.username().unwrap_or_default(),
            // Truncated by characters, not by bytes: slicing a UTF-8 display
            // name at byte 22 panics if a character straddles the boundary.
            display_name(record).chars().take(22).collect::<String>(),
            record.get(field::SHELL).unwrap_or_default(),
            if record.is_admin() { "yes" } else { "no" },
            record.groups().join(", "),
            if record.is_locked() { " (locked)" } else { "" },
        );
    }
}

fn cmd_info(username: &str) {
    let db = load_db();
    let record = require_user(&db, username);

    println!("Username:     {}", record.username().unwrap_or_default());
    println!(
        "UID:          {}",
        record
            .uid()
            .map_or_else(|| "(none)".to_string(), |u| u.to_string())
    );
    println!("Display name: {}", display_name(record));
    println!("Home:         {}", record.home().unwrap_or_default());
    println!(
        "Shell:        {}",
        record.get(field::SHELL).unwrap_or_default()
    );
    println!("Groups:       {}", record.groups().join(", "));
    println!("Admin:        {}", yes_no(record.is_admin()));
    println!("Locked:       {}", yes_no(record.is_locked()));
    if let Some(avatar) = record.avatar() {
        println!("Avatar:       {avatar}");
    }
    // Worth saying plainly: an entry in one of the two formats that predate
    // the shared implementation is not a password that happens to be wrong,
    // it is a password that cannot be checked at all, and the account is
    // unreachable until root sets a new one.
    if record.has_legacy_password() {
        println!(
            "Password:     stored in a format that predates the shared hash \
             and cannot be verified — run `useradm passwd {username}' to reset it"
        );
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn cmd_lock(username: &str) {
    let mut db = load_db();
    require_user_mut(&mut db, username).set_locked(true);
    save_db(&db);
    println!("Locked account {}", quoteaf_os(username));
}

fn cmd_unlock(username: &str) {
    let mut db = load_db();
    require_user_mut(&mut db, username).set_locked(false);
    save_db(&db);
    println!("Unlocked account {}", quoteaf_os(username));
}

fn cmd_groups(username: &str) {
    let db = load_db();
    let record = require_user(&db, username);
    println!(
        "{}: {}",
        record.username().unwrap_or_default(),
        record.groups().join(" ")
    );
}

fn cmd_mod(username: &str, args: &[String]) {
    let mut db = load_db();
    let record = require_user_mut(&mut db, username);

    let mut i = 0;
    while i < args.len() {
        let Some(flag) = args.get(i).map(String::as_str) else {
            break;
        };
        let value = args.get(i.saturating_add(1));
        match (flag, value) {
            ("--shell" | "-s", Some(v)) => {
                record.set(field::SHELL, v);
                i = i.saturating_add(2);
            }
            ("--home" | "-d", Some(v)) => {
                record.set_home(v);
                i = i.saturating_add(2);
            }
            ("--name" | "-c", Some(v)) => {
                record.set(field::DISPLAY_NAME, v);
                i = i.saturating_add(2);
            }
            ("--groups" | "-G", Some(v)) => {
                record.set_groups(&split_groups(v));
                i = i.saturating_add(2);
            }
            ("--admin", _) => {
                grant_admin(record);
                i = i.saturating_add(1);
            }
            _ => i = i.saturating_add(1),
        }
    }

    save_db(&db);
    println!("Modified user {}", quoteaf_os(username));
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS User Account Manager v0.1.0");
    println!();
    println!("Manage user accounts in the system.");
    println!();
    println!("USAGE:");
    println!("  useradm <command> <username> [options]");
    println!();
    println!("COMMANDS:");
    println!("  add <user> [opts]   Create a new user");
    println!("  del <user>          Delete a user");
    println!("  mod <user> [opts]   Modify user properties");
    println!("  passwd <user>       Change password");
    println!("  list                List all users");
    println!("  info <user>         Show user details");
    println!("  lock <user>         Lock account (prevent login)");
    println!("  unlock <user>       Unlock account");
    println!("  groups <user>       Show group memberships");
    println!();
    println!("ADD/MOD OPTIONS:");
    println!("  --shell, -s <path>  Login shell (default: /bin/sh)");
    println!("  --home, -d <path>   Home directory (default: /home/<user>)");
    println!("  --name, -c <name>   Display name");
    println!("  --groups, -G <g,g>  Group memberships (comma-separated)");
    println!("  --admin             Grant admin privileges");
    println!("  --uid, -u <uid>     Set specific UID");
}

/// The username argument, or exit with the standard message.
fn require_username<'a>(args: &'a [String], command: &str) -> &'a str {
    match args.get(2) {
        Some(name) => name.as_str(),
        None => {
            eprintln!("error: {} requires a username", quoteaf_os(command));
            process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let Some(command) = args.get(1).map(String::as_str) else {
        print_usage();
        process::exit(0);
    };

    let rest = args.get(3..).unwrap_or(&[]);

    match command {
        "add" | "useradd" | "create" => cmd_add(require_username(&args, "add"), rest),
        "del" | "userdel" | "delete" | "rm" => cmd_del(require_username(&args, "del")),
        "mod" | "usermod" | "modify" => cmd_mod(require_username(&args, "mod"), rest),
        "passwd" | "password" => cmd_passwd(require_username(&args, "passwd")),
        "list" | "ls" => cmd_list(),
        "info" | "show" => cmd_info(require_username(&args, "info")),
        "lock" => cmd_lock(require_username(&args, "lock")),
        "unlock" => cmd_unlock(require_username(&args, "unlock")),
        "groups" => cmd_groups(require_username(&args, "groups")),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'useradm help' for usage.");
            process::exit(1);
        }
    }
}

// The workspace's defensive lints are for production code; a test that indexes
// a fixture it just built is asserting, and an assertion that fails by
// panicking is a test doing its job.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// The one thing this whole change is for: an account created the way
    /// `useradm add` creates one authenticates the way the login manager
    /// authenticates. Both sides go through `userdb`, so this is now a
    /// statement about one implementation rather than a hope about two.
    #[test]
    fn an_account_created_here_authenticates() {
        let mut record = Record::new();
        record.set_uid(1000);
        record.set(field::USERNAME, "alice");
        record
            .set_password_with_salt("correct horse", "abcdefgh")
            .expect("hash");

        let mut db = UserDb::new();
        db.push(record);

        let reparsed = UserDb::parse(&db.to_text());
        let stored = reparsed.find("alice").expect("alice");
        assert_eq!(
            stored.check_password("correct horse"),
            userdb::Auth::Accepted
        );
        assert_eq!(stored.check_password("wrong"), userdb::Auth::Rejected);
    }

    #[test]
    fn granting_admin_sets_both_the_flag_and_the_group() {
        let mut record = Record::new();
        record.set_groups(&["users".to_string()]);
        grant_admin(&mut record);
        assert!(record.is_admin());
        assert_eq!(
            record.groups(),
            vec!["users".to_string(), "admin".to_string()]
        );

        // Twice is the same as once.
        grant_admin(&mut record);
        assert_eq!(
            record.groups(),
            vec!["users".to_string(), "admin".to_string()]
        );
    }

    #[test]
    fn group_lists_ignore_stray_separators() {
        assert_eq!(
            split_groups("users, admin , ,video"),
            vec![
                "users".to_string(),
                "admin".to_string(),
                "video".to_string()
            ]
        );
        assert!(split_groups("").is_empty());
    }

    /// A modification must not delete the fields `useradm` has no field for.
    /// The old serialiser rebuilt the file from its own struct, so a record
    /// the login manager had written lost `auto_login`, `login_count` and
    /// `last_login_timestamp` the first time anyone ran `useradm mod`.
    #[test]
    fn a_modification_keeps_fields_useradm_does_not_model() {
        let text = "users:\n  \
            - uid: 1000\n    \
            username: \"alice\"\n    \
            home_dir: \"/home/alice\"\n    \
            auto_login: false\n    \
            login_count: 42\n    \
            last_login_timestamp: 1700000000\n";
        let mut db = UserDb::parse(text);
        db.find_mut("alice")
            .expect("alice")
            .set(field::SHELL, "/bin/osh");

        let out = db.to_text();
        assert!(out.contains("login_count: 42"), "{out}");
        assert!(out.contains("last_login_timestamp: 1700000000"), "{out}");
        assert!(out.contains("auto_login: false"), "{out}");
        assert!(out.contains("shell: \"/bin/osh\""), "{out}");
    }

    /// A display name long enough to need truncating is cut on a character
    /// boundary. The previous `&name[..22]` panicked on any name whose 22nd
    /// byte fell inside a multi-byte character.
    #[test]
    fn a_long_multibyte_display_name_does_not_panic() {
        let mut record = Record::new();
        record.set(field::USERNAME, "yuki");
        record.set(field::DISPLAY_NAME, &"é".repeat(40));
        let shown: String = display_name(&record).chars().take(22).collect();
        assert_eq!(shown.chars().count(), 22);
    }
}
