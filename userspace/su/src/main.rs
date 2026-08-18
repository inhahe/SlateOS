//! Slate OS User Switching Utility (`su` / `sudo`)
//!
//! Switch to another user's identity and optionally run a command.
//! When invoked as `sudo` (detected via `argv[0]`), runs a single command
//! as root (or another user via `-u`).
//!
//! # su usage
//!
//! ```text
//! su [options] [username]          Switch to user (default: root)
//! su - [username]                  Login shell for user
//! su -l [username]                 Login shell for user
//! su -c 'command' [username]       Run command as user
//! su -m [username]                 Preserve caller's environment
//! su -p [username]                 Preserve caller's environment
//! su -s /bin/shell [username]      Override target user's shell
//! ```
//!
//! # sudo usage
//!
//! ```text
//! sudo command [args...]           Run command as root
//! sudo -u user command [args...]   Run command as user
//! sudo -l                          List permissions
//! ```
//!
//! # Authentication
//!
//! Reads `/etc/users.yaml` through the shared `userdb` crate. Passwords are
//! `crypt(3)` entries — SHA-512-crypt — and are checked by re-running `crypt`
//! on the stored entry, which is a valid setting for itself. Root (uid 0) can
//! switch to any user without a password. For sudo, members of the `wheel` or
//! `admin` group may run commands as root.
//!
//! # Session tracking
//!
//! On login-shell switches, writes a session file to `/run/sessions/`
//! and a fallback marker to `/tmp/.users/` so that `who`/`w` can
//! report the logged-in user.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;
use std::time::SystemTime;

// ============================================================================
// User database
// ============================================================================

// `/etc/users.yaml` is read through `userdb`, the one implementation of the
// format. This file used to carry its own parser and its own
// `sha256(salt + password)`, and both disagreed with what the two *writers* of
// the file produced — most visibly, it looked for `home:` where neither writer
// has ever written anything but `home_dir:`, so `su -` put every user in a
// home directory of "". See `design-decisions.md` §330.

use userdb::{Auth, Record, UserDb};

const USER_DB_PATH: &str = userdb::DEFAULT_PATH;

/// Load the user database, or print why it could not be loaded.
///
/// `who` is the name the binary was invoked as, `su` or `sudo`.
///
/// A missing file and an unreadable one are reported differently on purpose:
/// this program decides who may become root, so "there is no database" and "I
/// was not allowed to look" must not collapse into one message that an
/// administrator reads as the first.
fn load_users(who: &str) -> Option<UserDb> {
    match UserDb::load(USER_DB_PATH) {
        Ok(db) if db.records().is_empty() => {
            eprintln!("{who}: no user database at {USER_DB_PATH}");
            None
        }
        Ok(db) => Some(db),
        Err(e) => {
            eprintln!("{who}: cannot read {USER_DB_PATH}: {e}");
            None
        }
    }
}

/// The record's login name, or the empty string. Used only in messages.
fn name_of(record: &Record) -> String {
    record.username().unwrap_or_default()
}

/// The record's home directory, or `/`.
///
/// The fallback matters: a login shell is started *in* this directory, and
/// `Command::current_dir("")` fails rather than meaning "wherever we are", so
/// a record with no home used to make `su -` fail to exec at all.
fn home_of(record: &Record) -> String {
    match record.home() {
        Some(home) if !home.is_empty() => home,
        _ => "/".to_string(),
    }
}

/// The record's login shell, or the system default.
fn shell_of(record: &Record) -> String {
    record.shell().unwrap_or_else(|| "/bin/sh".to_string())
}

/// Whether `record` is a member of a group that confers administrator rights.
fn in_admin_group(record: &Record) -> bool {
    record.groups().iter().any(|g| g == "wheel" || g == "admin")
}

// ============================================================================
// Authentication
// ============================================================================

/// Prompt for `record`'s password and check it, printing the reason on
/// failure. Returns true only on a verified match.
///
/// The outcomes are kept distinct because three of the four are an
/// administrator's problem rather than a typing mistake, and reporting all of
/// them as "authentication failure" is how an account with an unverifiable
/// stored hash gets diagnosed as a forgotten password.
fn authenticate(record: &Record, prompt: &str, who: &str) -> bool {
    if record.is_locked() {
        eprintln!("{who}: account '{}' is locked", name_of(record));
        return false;
    }

    // Probing with the empty password distinguishes "no password stored" from
    // "the stored password is the empty string": the first answers
    // `NoPassword`, the second `Accepted`, and only the first is refused here.
    if record.check_password("") == Auth::NoPassword {
        eprintln!("{who}: account '{}' has no password set", name_of(record));
        return false;
    }

    if record.has_legacy_password() {
        let name = name_of(record);
        eprintln!(
            "{who}: account '{name}' has a password stored in a format this \
             system can no longer verify; run `useradm passwd {name}` as root"
        );
        return false;
    }

    let password = match read_password(prompt) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{who}: {e}");
            return false;
        }
    };

    match record.check_password(&password) {
        Auth::Accepted => true,
        Auth::Locked | Auth::Unusable | Auth::NoPassword | Auth::Rejected => {
            // The three non-`Rejected` cases were ruled out above and can only
            // arise from a change under our feet; they get the same message
            // because at this point the password has already been typed and a
            // detailed answer would say something about the account to whoever
            // typed it.
            eprintln!("{who}: authentication failure");
            false
        }
    }
}

// ============================================================================
// Environment and identity helpers
// ============================================================================

/// Get the current (calling) user's UID.
///
/// Tries /proc/self/status first, then falls back to the USER env var
/// matched against the user database, then defaults to u32::MAX (nobody).
fn get_caller_uid(users: &UserDb) -> u32 {
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
        && let Some(user) = users.find(&name)
        && let Some(uid) = user.uid()
    {
        return uid;
    }

    // Unknown caller.
    u32::MAX
}

/// Read a password from the terminal without echoing.
///
/// On Slate OS the terminal may not yet support disabling echo, so we
/// read a line from stdin. The prompt is written to stderr so that it
/// appears even when stdout is redirected.
fn read_password(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();

    let mut password = String::new();
    io::stdin()
        .read_line(&mut password)
        .map_err(|e| format!("failed to read password: {e}"))?;

    // Strip the trailing newline.
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }

    Ok(password)
}

/// Get the current epoch time in seconds.
fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Session tracking (who/w integration)
// ============================================================================

/// Record a login session so that `who` and `w` can see it.
///
/// Writes to both `/run/sessions/<pid>` (Slate OS native) and
/// `/tmp/.users/<username>` (fallback). Errors are non-fatal since
/// the switch itself should still proceed.
fn record_session(username: &str, tty: &str) {
    let now = current_epoch_secs();
    let pid = process::id();

    // Slate OS native session file.
    let session_dir = "/run/sessions";
    let _ = fs::create_dir_all(session_dir);
    let session_content = format!("user={username}\ntty={tty}\nhost=\ntime={now}\npid={pid}\n");
    let _ = fs::write(format!("{session_dir}/{pid}"), &session_content);

    // Fallback marker for /tmp/.users/.
    let users_dir = "/tmp/.users";
    let _ = fs::create_dir_all(users_dir);
    let _ = fs::write(format!("{users_dir}/{username}"), format!("{now}"));
}

/// Remove session records when the shell exits.
fn remove_session(username: &str) {
    let pid = process::id();
    let _ = fs::remove_file(format!("/run/sessions/{pid}"));
    let _ = fs::remove_file(format!("/tmp/.users/{username}"));
}

// ============================================================================
// Command execution
// ============================================================================

/// Build and execute a command as the target user.
///
/// In login mode, the environment is replaced with a clean set derived
/// from the target user's record. In preserve mode, the caller's
/// environment is kept. Otherwise a minimal set of variables is updated.
///
/// Returns the exit code of the child process.
fn exec_as_user(
    target: &Record,
    shell_override: Option<&str>,
    command: Option<&str>,
    login_mode: bool,
    preserve_env: bool,
) -> i32 {
    let target_shell = shell_of(target);
    let target_home = home_of(target);
    let target_name = name_of(target);
    let target_uid = target.uid().unwrap_or(u32::MAX);
    let shell = shell_override.unwrap_or(&target_shell);

    // Determine the program and arguments.
    let (program, args): (String, Vec<String>) = if let Some(cmd) = command {
        // Run a single command via the shell's -c flag.
        (shell.to_string(), vec!["-c".to_string(), cmd.to_string()])
    } else {
        // Interactive shell. For login shells, argv[0] is prefixed with '-'.
        let argv0 = if login_mode {
            let base = shell.rsplit('/').next().unwrap_or(shell);
            format!("-{base}")
        } else {
            shell.to_string()
        };
        (shell.to_string(), vec![argv0])
    };

    let mut cmd = process::Command::new(&program);

    // For a "shell -c 'command'" invocation, args are ["-c", "command"].
    // For an interactive shell, the single arg is the argv[0] name.
    // process::Command already sets argv[0] to the program path, but for
    // login shells we want the '-bash' convention. Since Command does not
    // let us override argv[0] directly, we pass it as the shell argument
    // and let the shell detect it. On our OS the shell may or may not
    // honour this convention, but we follow standard practice.
    if command.is_some() {
        // -c mode: pass ["-c", "command"].
        for arg in &args {
            cmd.arg(arg);
        }
    }
    // For interactive mode (no -c), we just launch the shell with no args.
    // The shell reads its own argv[0] from the OS, which we cannot override
    // via std::process::Command. The login-shell convention is best-effort.

    if login_mode && !preserve_env {
        // Clean environment: only set what a login shell expects.
        cmd.env_clear();
        cmd.env("HOME", &target_home);
        cmd.env("SHELL", shell);
        cmd.env("USER", &target_name);
        cmd.env("LOGNAME", &target_name);
        cmd.env("PATH", default_path_for_uid(target_uid));

        // Propagate TERM if set -- shells need it for line editing.
        if let Ok(term) = env::var("TERM") {
            cmd.env("TERM", term);
        }

        // Set supplementary groups as a comma-separated list in an env var.
        // The kernel would normally set these at exec time via setgroups();
        // we expose them here for user-space awareness.
        let groups = target.groups();
        if !groups.is_empty() {
            cmd.env("GROUPS", groups.join(","));
        }
    } else if preserve_env {
        // Keep the caller's entire environment, only override USER/LOGNAME.
        cmd.env("USER", &target_name);
        cmd.env("LOGNAME", &target_name);
    } else {
        // Non-login, non-preserve: update key variables.
        cmd.env("HOME", &target_home);
        cmd.env("SHELL", shell);
        cmd.env("USER", &target_name);
        cmd.env("LOGNAME", &target_name);
    }

    // Set working directory for login shells.
    if login_mode {
        cmd.current_dir(&target_home);
    }

    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("su: failed to execute {program}: {e}");
            126
        }
    }
}

/// Return the default PATH for a given uid.
///
/// Root gets sbin directories; normal users do not.
fn default_path_for_uid(uid: u32) -> &'static str {
    if uid == 0 {
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    } else {
        "/usr/local/bin:/usr/bin:/bin"
    }
}

// ============================================================================
// Terminal detection
// ============================================================================

/// Detect the controlling terminal name for session tracking.
fn detect_tty() -> String {
    // Try /proc/self/fd/0 symlink.
    if let Ok(link) = fs::read_link("/proc/self/fd/0") {
        let path_str = link.to_string_lossy();
        if let Some(tty) = path_str.strip_prefix("/dev/") {
            return tty.to_string();
        }
    }
    "?".to_string()
}

// ============================================================================
// su mode
// ============================================================================

/// Parsed options for `su`.
#[derive(Debug)]
struct SuOptions {
    /// Target username (default: "root").
    target_user: String,
    /// Login shell mode (-l, --login, leading `-`).
    login: bool,
    /// Command to run via shell -c.
    command: Option<String>,
    /// Preserve the caller's environment (-m, -p, --preserve-environment).
    preserve_env: bool,
    /// Override the target user's shell (-s, --shell).
    shell: Option<String>,
}

/// Parse `su` command-line arguments.
///
/// Accepted forms:
///   su [options] [username]
///   su - [username]
///   su -l [username]
///   su -c 'cmd' [username]
///   su -s /path/shell [username]
///   su -m [username]
fn parse_su_args(args: &[String]) -> Result<SuOptions, i32> {
    let mut opts = SuOptions {
        target_user: "root".to_string(),
        login: false,
        command: None,
        preserve_env: false,
        shell: None,
    };

    let mut positional: Vec<String> = Vec::new();
    // Driven by the iterator rather than an index, so that "this option takes
    // a value" is expressed by consuming the next item and cannot run off the
    // end of the slice.
    let mut rest = args.iter().skip(1);

    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-" | "-l" | "--login" => {
                opts.login = true;
            }
            "-c" | "--command" => {
                let Some(value) = rest.next() else {
                    eprintln!("su: option '{arg}' requires an argument");
                    return Err(1);
                };
                opts.command = Some(value.clone());
            }
            "-m" | "-p" | "--preserve-environment" => {
                opts.preserve_env = true;
            }
            "-s" | "--shell" => {
                let Some(value) = rest.next() else {
                    eprintln!("su: option '{arg}' requires an argument");
                    return Err(1);
                };
                opts.shell = Some(value.clone());
            }
            "-h" | "--help" => {
                print_su_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("su (Slate OS) 0.1.0");
                return Err(0);
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("su: unknown option: {other}");
                    eprintln!("Try 'su --help' for usage.");
                    return Err(1);
                }
                positional.push(other.to_string());
            }
        }
    }

    // The last positional argument (if any) is the target username.
    if let Some(name) = positional.pop() {
        opts.target_user = name;
    }

    Ok(opts)
}

fn print_su_help() {
    println!("Slate OS User Switch (su) v0.1.0");
    println!();
    println!("Switch to another user account.");
    println!();
    println!("USAGE:");
    println!("  su [options] [username]");
    println!("  su - [username]");
    println!();
    println!("OPTIONS:");
    println!("  -, -l, --login              Start a login shell");
    println!("  -c, --command <command>      Run a single command");
    println!("  -m, -p, --preserve-environment");
    println!("                               Keep the caller's environment");
    println!("  -s, --shell <shell>          Override the target user's shell");
    println!("  -h, --help                   Show this help");
    println!("  -V, --version                Show version");
    println!();
    println!("If no username is given, switches to root.");
    println!("Root can switch to any user without a password.");
}

/// Run the `su` command.
fn run_su(args: &[String]) -> i32 {
    let opts = match parse_su_args(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let Some(users) = load_users("su") else {
        return 1;
    };

    let Some(target) = users.find(&opts.target_user) else {
        eprintln!("su: unknown user: {}", opts.target_user);
        return 1;
    };

    if target.is_locked() {
        eprintln!("su: account '{}' is locked", name_of(target));
        return 1;
    }

    // Authenticate unless the caller is root.
    let caller_uid = get_caller_uid(&users);
    if caller_uid != 0 && !authenticate(target, "Password: ", "su") {
        return 1;
    }

    let target_name = name_of(target);

    // Session tracking for login shells.
    let is_login = opts.login && opts.command.is_none();
    if is_login {
        let tty = detect_tty();
        record_session(&target_name, &tty);
    }

    let exit_code = exec_as_user(
        target,
        opts.shell.as_deref(),
        opts.command.as_deref(),
        opts.login,
        opts.preserve_env,
    );

    if is_login {
        remove_session(&target_name);
    }

    exit_code
}

// ============================================================================
// sudo mode
// ============================================================================

/// Parsed options for `sudo`.
#[derive(Debug)]
struct SudoOptions {
    /// Target username (default: "root").
    target_user: String,
    /// The command and its arguments.
    command: Vec<String>,
    /// List permissions mode (-l).
    list_mode: bool,
}

/// Parse `sudo` command-line arguments.
///
/// Accepted forms:
///   sudo command [args...]
///   sudo -u user command [args...]
///   sudo -l
fn parse_sudo_args(args: &[String]) -> Result<SudoOptions, i32> {
    let mut opts = SudoOptions {
        target_user: "root".to_string(),
        command: Vec::new(),
        list_mode: false,
    };

    // See `parse_su_args`: driven by the iterator so that an option needing a
    // value cannot read past the end of the slice.
    let mut rest = args.iter().skip(1);

    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-u" | "--user" => {
                let Some(value) = rest.next() else {
                    eprintln!("sudo: option '{arg}' requires an argument");
                    return Err(1);
                };
                opts.target_user = value.clone();
            }
            "-l" | "--list" => {
                opts.list_mode = true;
            }
            "-h" | "--help" => {
                print_sudo_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("sudo (Slate OS) 0.1.0");
                return Err(0);
            }
            _ => {
                // Everything from here on is the command and its args. The
                // first word is the one already taken from the iterator.
                opts.command = std::iter::once(arg).chain(rest).cloned().collect();
                break;
            }
        }
    }

    if !opts.list_mode && opts.command.is_empty() {
        eprintln!("sudo: no command specified");
        eprintln!("Try 'sudo --help' for usage.");
        return Err(1);
    }

    Ok(opts)
}

fn print_sudo_help() {
    println!("Slate OS Sudo v0.1.0");
    println!();
    println!("Run a command as another user (default: root).");
    println!();
    println!("USAGE:");
    println!("  sudo [options] command [args...]");
    println!();
    println!("OPTIONS:");
    println!("  -u, --user <user>   Run as this user (default: root)");
    println!("  -l, --list          List caller's permissions");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
    println!();
    println!("POLICY:");
    println!("  root can run any command as any user.");
    println!("  Members of the 'wheel' or 'admin' group can sudo to root.");
}

/// Check whether the caller is authorised to use sudo.
///
/// Policy: root (uid 0) can do anything. Members of `wheel` or `admin`
/// groups can sudo to root. Other combinations are denied.
/// `_target_uid` is unused: the policy grants an administrator the right to
/// become *anyone*, so sudo-to-root and sudo-to-alice take the same test. The
/// parameter is kept because that is a policy choice rather than an oversight,
/// and a future policy that does distinguish them needs it back.
fn sudo_authorised(caller: &Record, _target_uid: u32) -> bool {
    // Root can always sudo.
    if caller.uid() == Some(0) {
        return true;
    }
    in_admin_group(caller) || caller.is_admin()
}

/// Print the caller's sudo permissions.
fn sudo_list_permissions(caller: &Record) {
    println!("User {} may run the following commands:", name_of(caller));
    if caller.uid() == Some(0) {
        println!("    (ALL) ALL");
    } else if caller.is_admin() || in_admin_group(caller) {
        println!("    (ALL) ALL  [via wheel/admin group membership]");
    } else {
        println!("    (NONE)");
    }
}

/// Run the `sudo` command.
fn run_sudo(args: &[String]) -> i32 {
    let opts = match parse_sudo_args(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let Some(users) = load_users("sudo") else {
        return 1;
    };

    let caller_uid = get_caller_uid(&users);
    let Some(caller) = users.find_uid(caller_uid) else {
        eprintln!(
            "sudo: unknown calling user (uid {caller_uid}); \
             cannot determine permissions"
        );
        return 1;
    };

    if opts.list_mode {
        sudo_list_permissions(caller);
        return 0;
    }

    let Some(target) = users.find(&opts.target_user) else {
        eprintln!("sudo: unknown user: {}", opts.target_user);
        return 1;
    };

    if target.is_locked() {
        eprintln!("sudo: account '{}' is locked", name_of(target));
        return 1;
    }

    // Authorisation check.
    if !sudo_authorised(caller, target.uid().unwrap_or(u32::MAX)) {
        let caller_name = name_of(caller);
        eprintln!(
            "sudo: user '{caller_name}' is not in the sudoers file. \
             This incident will be reported."
        );
        // Log the failed attempt.
        log_sudo_failure(&caller_name, &opts.command);
        return 1;
    }

    // Authenticate: require the caller's own password (sudo convention),
    // unless the caller is root.
    if caller_uid != 0 {
        let prompt = format!("[sudo] password for {}: ", name_of(caller));
        if !authenticate(caller, &prompt, "sudo") {
            return 1;
        }
    }

    // Execute the command as the target user.
    let command_str = opts.command.join(" ");

    exec_as_user(
        target,
        None,
        Some(&command_str),
        false, // not a login shell
        false, // don't preserve env
    )
}

/// Log a sudo authorisation failure to syslog or a fallback file.
fn log_sudo_failure(username: &str, command: &[String]) {
    let now = current_epoch_secs();
    let cmd_str = command.join(" ");
    let msg = format!("{now} sudo: DENIED user={username} command=\"{cmd_str}\"\n");
    // Best-effort: append to the auth log.
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/var/log/auth.log")
    {
        let _ = file.write_all(msg.as_bytes());
    }
}

// ============================================================================
// Entry point: detect su vs sudo via argv[0]
// ============================================================================

/// Extract the base name from a path (everything after the last `/` or `\`).
fn basename(path: &str) -> &str {
    let after_slash = path.rsplit('/').next().unwrap_or(path);
    after_slash.rsplit('\\').next().unwrap_or(after_slash)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog = args.first().map(|s| basename(s)).unwrap_or("su");

    let exit_code = if prog == "sudo" || prog == "sudo.exe" {
        run_sudo(&args)
    } else {
        run_su(&args)
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

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

    // --- User database parsing ---
    //
    // The SHA-256 and `hash_password`/`verify_password` tests that used to
    // stand here are deleted rather than ported. They asserted that hashing was
    // deterministic, that different inputs hashed differently, and that the
    // output was the right length — all of which are true of any function
    // written by accident, which is what the thing under test turned out to be.
    // The hash now comes from `posix::crypt`, which is checked against
    // Drepper's published vectors, and what is worth testing here is that this
    // program agrees with the programs that *write* the file.

    fn sample_users_yaml() -> &'static str {
        // Deliberately mixed spelling: `root` uses the login manager's
        // `home_dir`, `alice` uses `useradm`'s `home`. Both must read back,
        // because both spellings exist in files this tree has written.
        r#"# Slate OS user database
users:
  - uid: 0
    username: "root"
    display_name: "System Administrator"
    password_hash: "$6$rootsalt$dummy"
    shell: "/bin/sh"
    home_dir: "/root"
    groups: [root, admin, wheel]
    is_admin: true
    locked: false
  - uid: 1000
    username: "alice"
    display_name: "Alice"
    password_hash: "$6$alicesalt$dummy"
    shell: "/bin/bash"
    home: "/home/alice"
    groups: [users, wheel]
    admin: false
    locked: false
  - uid: 1001
    username: "bob"
    display_name: "Bob"
    password_hash: ""
    shell: "/bin/sh"
    home: "/home/bob"
    groups: [users]
    admin: false
    locked: true
"#
    }

    /// Parse the sample directly, bypassing file I/O.
    fn parse_sample_users() -> UserDb {
        UserDb::parse(sample_users_yaml())
    }

    #[test]
    fn test_parse_user_count() {
        let users = parse_sample_users();
        assert_eq!(users.records().len(), 3);
    }

    #[test]
    fn test_parse_root_user() {
        let users = parse_sample_users();
        let root = users.find("root").expect("root should exist");
        assert_eq!(root.uid(), Some(0));
        assert_eq!(home_of(root), "/root");
        assert_eq!(shell_of(root), "/bin/sh");
        assert!(root.is_admin());
        assert!(!root.is_locked());
        assert!(root.groups().contains(&"wheel".to_string()));
    }

    #[test]
    fn test_parse_normal_user() {
        let users = parse_sample_users();
        let alice = users.find("alice").expect("alice should exist");
        assert_eq!(alice.uid(), Some(1000));
        assert_eq!(home_of(alice), "/home/alice");
        assert_eq!(shell_of(alice), "/bin/bash");
        assert!(!alice.is_admin());
        assert!(!alice.is_locked());
        assert!(alice.groups().contains(&"wheel".to_string()));
    }

    /// Both writers' spellings of the home directory are read.
    ///
    /// This is the regression test for the bug that prompted the migration:
    /// this program read only `home:`, and *neither* writer of the file has
    /// ever written anything but `home_dir:`, so `su - root` started a login
    /// shell with `HOME` unset in every real database.
    #[test]
    fn test_both_spellings_of_home_are_read() {
        let users = parse_sample_users();
        let root = users.find("root").expect("root should exist");
        let alice = users.find("alice").expect("alice should exist");
        assert_eq!(home_of(root), "/root", "home_dir: must be read");
        assert_eq!(home_of(alice), "/home/alice", "home: must be read");
    }

    /// Likewise for the administrator flag, where `root` carries `is_admin`.
    #[test]
    fn test_both_spellings_of_the_admin_flag_are_read() {
        let users = parse_sample_users();
        assert!(users.find("root").expect("root").is_admin());
        assert!(!users.find("alice").expect("alice").is_admin());
    }

    #[test]
    fn test_parse_locked_user() {
        let users = parse_sample_users();
        let bob = users.find("bob").expect("bob should exist");
        assert_eq!(bob.uid(), Some(1001));
        assert!(bob.is_locked());
    }

    #[test]
    fn test_find_user_nonexistent() {
        let users = parse_sample_users();
        assert!(users.find("nonexistent").is_none());
    }

    #[test]
    fn test_find_user_by_uid() {
        let users = parse_sample_users();
        let root = users.find_uid(0).expect("uid 0 should exist");
        assert_eq!(root.username().as_deref(), Some("root"));
        let alice = users.find_uid(1000).expect("uid 1000 should exist");
        assert_eq!(alice.username().as_deref(), Some("alice"));
        assert!(users.find_uid(9999).is_none());
    }

    // --- Authentication ---

    /// A password set the way `useradm` and the login manager set it is
    /// accepted here. This is the property that was broken: three programs,
    /// three constructions, and no test that compared any two of them.
    #[test]
    fn test_a_password_set_through_userdb_is_accepted() {
        let mut record = Record::new();
        record.set("username", "carol");
        record
            .set_password_with_salt("correct horse", "0123456789abcdef")
            .expect("a 16-character salt is storable");

        assert_eq!(record.check_password("correct horse"), Auth::Accepted);
        assert_eq!(record.check_password("Correct horse"), Auth::Rejected);
        assert_eq!(record.check_password(""), Auth::Rejected);
    }

    /// An entry in either of the two formats this tree used to write reports
    /// itself unverifiable rather than wrong, so that an administrator is told
    /// to run `useradm passwd` instead of hunting a forgotten password.
    #[test]
    fn test_a_legacy_entry_is_unusable_not_wrong() {
        let mut record = Record::new();
        record.set("username", "dave");
        // 64 hex digits: what both of the replaced constructions produced.
        record.set(
            "password_hash",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        assert!(record.has_legacy_password());
        assert_eq!(record.check_password("anything"), Auth::Unusable);
    }

    /// A locked account reports itself locked whatever is typed, and does so
    /// before the stored hash is consulted at all.
    #[test]
    fn test_a_locked_account_refuses_its_own_password() {
        let mut record = Record::new();
        record.set("username", "erin");
        record
            .set_password_with_salt("hunter2", "0123456789abcdef")
            .expect("a 16-character salt is storable");
        record.set_locked(true);
        assert_eq!(record.check_password("hunter2"), Auth::Locked);
    }

    // --- sudo authorisation ---

    #[test]
    fn test_sudo_root_always_authorised() {
        let users = parse_sample_users();
        let root = users.find("root").unwrap();
        assert!(sudo_authorised(root, 0));
        assert!(sudo_authorised(root, 1000));
        assert!(sudo_authorised(root, 1001));
    }

    #[test]
    fn test_sudo_wheel_member_authorised_for_root() {
        let users = parse_sample_users();
        let alice = users.find("alice").unwrap();
        // Alice is in wheel group -> can sudo to root.
        assert!(sudo_authorised(alice, 0));
    }

    #[test]
    fn test_sudo_wheel_member_authorised_for_other() {
        let users = parse_sample_users();
        let alice = users.find("alice").unwrap();
        // Wheel members can sudo to any user.
        assert!(sudo_authorised(alice, 1001));
    }

    #[test]
    fn test_sudo_non_wheel_denied() {
        let users = parse_sample_users();
        let bob = users.find("bob").unwrap();
        // Bob is only in 'users' group -- no sudo.
        assert!(!sudo_authorised(bob, 0));
    }

    // --- su argument parsing ---

    #[test]
    fn test_su_args_default() {
        let args = vec!["su".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.target_user, "root");
        assert!(!opts.login);
        assert!(opts.command.is_none());
        assert!(!opts.preserve_env);
        assert!(opts.shell.is_none());
    }

    #[test]
    fn test_su_args_user() {
        let args = vec!["su".to_string(), "alice".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.target_user, "alice");
        assert!(!opts.login);
    }

    #[test]
    fn test_su_args_login_dash() {
        let args = vec!["su".to_string(), "-".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn test_su_args_login_dash_user() {
        let args = vec!["su".to_string(), "-".to_string(), "alice".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_login_l() {
        let args = vec!["su".to_string(), "-l".to_string(), "bob".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "bob");
    }

    #[test]
    fn test_su_args_command() {
        let args = vec![
            "su".to_string(),
            "-c".to_string(),
            "whoami".to_string(),
            "alice".to_string(),
        ];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.command.as_deref(), Some("whoami"));
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_preserve_env() {
        let args = vec!["su".to_string(), "-m".to_string(), "alice".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.preserve_env);
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_shell_override() {
        let args = vec![
            "su".to_string(),
            "-s".to_string(),
            "/bin/zsh".to_string(),
            "root".to_string(),
        ];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn test_su_args_command_missing_arg() {
        let args = vec!["su".to_string(), "-c".to_string()];
        assert_eq!(parse_su_args(&args).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_shell_missing_arg() {
        let args = vec!["su".to_string(), "-s".to_string()];
        assert_eq!(parse_su_args(&args).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_unknown_option() {
        let args = vec!["su".to_string(), "--bogus".to_string()];
        assert_eq!(parse_su_args(&args).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_help() {
        let args = vec!["su".to_string(), "--help".to_string()];
        assert_eq!(parse_su_args(&args).unwrap_err(), 0);
    }

    #[test]
    fn test_su_args_version() {
        let args = vec!["su".to_string(), "--version".to_string()];
        assert_eq!(parse_su_args(&args).unwrap_err(), 0);
    }

    // --- sudo argument parsing ---

    #[test]
    fn test_sudo_args_simple_command() {
        let args = vec!["sudo".to_string(), "ls".to_string(), "-la".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_user, "root");
        assert_eq!(opts.command, vec!["ls", "-la"]);
        assert!(!opts.list_mode);
    }

    #[test]
    fn test_sudo_args_user_flag() {
        let args = vec![
            "sudo".to_string(),
            "-u".to_string(),
            "alice".to_string(),
            "whoami".to_string(),
        ];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.target_user, "alice");
        assert_eq!(opts.command, vec!["whoami"]);
    }

    #[test]
    fn test_sudo_args_list_mode() {
        let args = vec!["sudo".to_string(), "-l".to_string()];
        let opts = parse_sudo_args(&args).unwrap();
        assert!(opts.list_mode);
    }

    #[test]
    fn test_sudo_args_no_command() {
        let args = vec!["sudo".to_string()];
        assert_eq!(parse_sudo_args(&args).unwrap_err(), 1);
    }

    #[test]
    fn test_sudo_args_user_missing_arg() {
        let args = vec!["sudo".to_string(), "-u".to_string()];
        assert_eq!(parse_sudo_args(&args).unwrap_err(), 1);
    }

    #[test]
    fn test_sudo_args_help() {
        let args = vec!["sudo".to_string(), "--help".to_string()];
        assert_eq!(parse_sudo_args(&args).unwrap_err(), 0);
    }

    #[test]
    fn test_sudo_args_version() {
        let args = vec!["sudo".to_string(), "--version".to_string()];
        assert_eq!(parse_sudo_args(&args).unwrap_err(), 0);
    }

    // --- Basename ---

    #[test]
    fn test_basename_simple() {
        assert_eq!(basename("su"), "su");
    }

    #[test]
    fn test_basename_with_slash() {
        assert_eq!(basename("/usr/bin/su"), "su");
        assert_eq!(basename("/usr/bin/sudo"), "sudo");
    }

    #[test]
    fn test_basename_with_backslash() {
        assert_eq!(basename("C:\\Windows\\sudo.exe"), "sudo.exe");
    }

    #[test]
    fn test_basename_mixed() {
        assert_eq!(basename("/usr/bin\\su"), "su");
    }

    // --- Default path ---

    #[test]
    fn test_default_path_root() {
        let path = default_path_for_uid(0);
        assert!(path.contains("/sbin"));
        assert!(path.contains("/usr/sbin"));
    }

    #[test]
    fn test_default_path_normal() {
        let path = default_path_for_uid(1000);
        assert!(!path.contains("/sbin"));
        assert!(path.contains("/usr/bin"));
    }

    // --- Combined su + password flow ---

    /// A password written to the file by one program is read back out of the
    /// file and accepted by this one.
    ///
    /// The test this replaces *simulated* `useradm` by re-implementing what it
    /// was believed to do, which is why it passed for as long as the belief was
    /// wrong. This one goes through the serialiser and the parser, so the only
    /// way it can pass is if the bytes on disk are the bytes both programs
    /// agree on.
    #[test]
    fn test_full_auth_flow_through_the_file() {
        let mut db = UserDb::parse(sample_users_yaml());
        db.find_mut("alice")
            .expect("alice should exist")
            .set_password_with_salt("secret", "0123456789abcdef")
            .expect("a 16-character salt is storable");

        let round_tripped = UserDb::parse(&db.to_text());
        let alice = round_tripped.find("alice").expect("alice survives a save");

        assert_eq!(alice.check_password("secret"), Auth::Accepted);
        assert_eq!(alice.check_password("wrong"), Auth::Rejected);
        // And the fields this program does not set are still there.
        assert_eq!(home_of(alice), "/home/alice");
        assert!(alice.groups().contains(&"wheel".to_string()));
    }

    // --- Edge cases ---

    #[test]
    fn test_su_args_multiple_positionals_last_wins() {
        // Only the last positional is the username; earlier ones are ignored.
        // (Matches traditional su behavior: extra args before the username
        //  are not meaningful.)
        let args = vec!["su".to_string(), "first".to_string(), "second".to_string()];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.target_user, "second");
    }

    #[test]
    fn test_su_args_login_and_command() {
        let args = vec![
            "su".to_string(),
            "-l".to_string(),
            "-c".to_string(),
            "id".to_string(),
            "alice".to_string(),
        ];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert_eq!(opts.command.as_deref(), Some("id"));
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_all_flags_combined() {
        let args = vec![
            "su".to_string(),
            "-l".to_string(),
            "-p".to_string(),
            "-s".to_string(),
            "/bin/fish".to_string(),
            "-c".to_string(),
            "uname -a".to_string(),
            "root".to_string(),
        ];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert!(opts.preserve_env);
        assert_eq!(opts.shell.as_deref(), Some("/bin/fish"));
        assert_eq!(opts.command.as_deref(), Some("uname -a"));
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn test_sudo_command_with_flags() {
        // Everything after the first non-option arg is the command.
        let args = vec![
            "sudo".to_string(),
            "ls".to_string(),
            "-la".to_string(),
            "/tmp".to_string(),
        ];
        let opts = parse_sudo_args(&args).unwrap();
        assert_eq!(opts.command, vec!["ls", "-la", "/tmp"]);
    }
}
