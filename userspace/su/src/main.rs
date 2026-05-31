//! OurOS User Switching Utility (`su` / `sudo`)
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
//! Reads `/etc/users.yaml` for user records. Passwords are stored as
//! SHA-256(salt + password). Root (uid 0) can switch to any user without
//! a password. For sudo, members of the `wheel` or `admin` group may
//! run commands as root.
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
// User data model (shared format with useradm)
// ============================================================================

#[derive(Clone)]
struct User {
    uid: u32,
    username: String,
    #[allow(dead_code)]
    display_name: String,
    password_hash: String,
    salt: String,
    shell: String,
    home: String,
    groups: Vec<String>,
    admin: bool,
    locked: bool,
}

const USER_DB_PATH: &str = "/etc/users.yaml";

/// Read all users from /etc/users.yaml.
///
/// Format matches what `useradm` writes:
/// ```yaml
/// users:
///   - uid: 0
///     username: root
///     password_hash: "..."
///     salt: "..."
///     shell: /bin/sh
///     home: /root
///     groups: [root, admin, wheel]
///     admin: true
///     locked: false
/// ```
fn read_users() -> Vec<User> {
    let content = match fs::read_to_string(USER_DB_PATH) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("su: cannot read {USER_DB_PATH}: {e}");
            return Vec::new();
        }
    };

    let mut users = Vec::new();
    let mut current: Option<User> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
            // New user entry -- flush previous.
            if let Some(user) = current.take() {
                users.push(user);
            }
            let uid: u32 = trimmed
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            current = Some(User {
                uid,
                username: String::new(),
                display_name: String::new(),
                password_hash: String::new(),
                salt: String::new(),
                shell: "/bin/sh".to_string(),
                home: String::new(),
                groups: Vec::new(),
                admin: false,
                locked: false,
            });
        } else if let Some(ref mut user) = current {
            if let Some(val) = trimmed.strip_prefix("username:") {
                user.username = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("display_name:") {
                user.display_name = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("password_hash:") {
                user.password_hash = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("salt:") {
                user.salt = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("shell:") {
                user.shell = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("home:") {
                user.home = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("groups:") {
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                user.groups = val
                    .split(',')
                    .map(|g| g.trim().trim_matches('"').to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            } else if let Some(val) = trimmed.strip_prefix("admin:") {
                user.admin = val.trim() == "true";
            } else if let Some(val) = trimmed.strip_prefix("locked:") {
                user.locked = val.trim() == "true";
            }
        }
    }

    if let Some(user) = current {
        users.push(user);
    }

    users
}

/// Look up a user by name.
fn find_user<'a>(users: &'a [User], name: &str) -> Option<&'a User> {
    users.iter().find(|u| u.username == name)
}

/// Look up a user by uid.
fn find_user_by_uid<'a>(users: &'a [User], uid: u32) -> Option<&'a User> {
    users.iter().find(|u| u.uid == uid)
}

// ============================================================================
// Password hashing -- must match useradm's SHA-256(salt + password) scheme
// ============================================================================

/// SHA-256 hash, returning a lowercase hex string.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Padding.
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process 64-byte blocks.
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{v:08x}")).collect()
}

/// Hash a password with the given salt, matching useradm's scheme.
fn hash_password(password: &str, salt: &str) -> String {
    let input = format!("{salt}{password}");
    sha256_hex(input.as_bytes())
}

/// Verify a password against a stored hash and salt.
fn verify_password(password: &str, stored_hash: &str, salt: &str) -> bool {
    let computed = hash_password(password, salt);
    // Constant-time comparison to prevent timing attacks.
    if computed.len() != stored_hash.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in computed.bytes().zip(stored_hash.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

// ============================================================================
// Environment and identity helpers
// ============================================================================

/// Get the current (calling) user's UID.
///
/// Tries /proc/self/status first, then falls back to the USER env var
/// matched against the user database, then defaults to u32::MAX (nobody).
fn get_caller_uid(users: &[User]) -> u32 {
    // Try /proc/self/status for the real UID.
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(uid_str) = rest.trim().split_whitespace().next() {
                    if let Ok(uid) = uid_str.parse::<u32>() {
                        return uid;
                    }
                }
            }
        }
    }

    // Fallback: resolve USER env var against the database.
    if let Ok(name) = env::var("USER") {
        if let Some(user) = find_user(users, &name) {
            return user.uid;
        }
    }

    // Unknown caller.
    u32::MAX
}

/// Read a password from the terminal without echoing.
///
/// On OurOS the terminal may not yet support disabling echo, so we
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
/// Writes to both `/run/sessions/<pid>` (OurOS native) and
/// `/tmp/.users/<username>` (fallback). Errors are non-fatal since
/// the switch itself should still proceed.
fn record_session(username: &str, tty: &str) {
    let now = current_epoch_secs();
    let pid = process::id();

    // OurOS native session file.
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
    target: &User,
    shell_override: Option<&str>,
    command: Option<&str>,
    login_mode: bool,
    preserve_env: bool,
) -> i32 {
    let shell = shell_override.unwrap_or(&target.shell);

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
        cmd.env("HOME", &target.home);
        cmd.env("SHELL", shell);
        cmd.env("USER", &target.username);
        cmd.env("LOGNAME", &target.username);
        cmd.env("PATH", default_path_for_uid(target.uid));

        // Propagate TERM if set -- shells need it for line editing.
        if let Ok(term) = env::var("TERM") {
            cmd.env("TERM", term);
        }

        // Set supplementary groups as a comma-separated list in an env var.
        // The kernel would normally set these at exec time via setgroups();
        // we expose them here for user-space awareness.
        if !target.groups.is_empty() {
            cmd.env("GROUPS", target.groups.join(","));
        }
    } else if preserve_env {
        // Keep the caller's entire environment, only override USER/LOGNAME.
        cmd.env("USER", &target.username);
        cmd.env("LOGNAME", &target.username);
    } else {
        // Non-login, non-preserve: update key variables.
        cmd.env("HOME", &target.home);
        cmd.env("SHELL", shell);
        cmd.env("USER", &target.username);
        cmd.env("LOGNAME", &target.username);
    }

    // Set working directory for login shells.
    if login_mode {
        cmd.current_dir(&target.home);
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
    let mut i = 1; // skip argv[0]

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-" | "-l" | "--login" => {
                opts.login = true;
            }
            "-c" | "--command" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("su: option '{arg}' requires an argument");
                    return Err(1);
                }
                opts.command = Some(args[i].clone());
            }
            "-m" | "-p" | "--preserve-environment" => {
                opts.preserve_env = true;
            }
            "-s" | "--shell" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("su: option '{arg}' requires an argument");
                    return Err(1);
                }
                opts.shell = Some(args[i].clone());
            }
            "-h" | "--help" => {
                print_su_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("su (OurOS) 0.1.0");
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
        i += 1;
    }

    // The last positional argument (if any) is the target username.
    if let Some(name) = positional.pop() {
        opts.target_user = name;
    }

    Ok(opts)
}

fn print_su_help() {
    println!("OurOS User Switch (su) v0.1.0");
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

    let users = read_users();
    if users.is_empty() {
        eprintln!("su: no user database found at {USER_DB_PATH}");
        return 1;
    }

    let target = match find_user(&users, &opts.target_user) {
        Some(u) => u,
        None => {
            eprintln!("su: unknown user: {}", opts.target_user);
            return 1;
        }
    };

    if target.locked {
        eprintln!("su: account '{}' is locked", target.username);
        return 1;
    }

    // Authenticate unless the caller is root.
    let caller_uid = get_caller_uid(&users);
    if caller_uid != 0 {
        if target.password_hash.is_empty() {
            eprintln!("su: account '{}' has no password set", target.username);
            return 1;
        }

        let password = match read_password("Password: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("su: {e}");
                return 1;
            }
        };

        if !verify_password(&password, &target.password_hash, &target.salt) {
            eprintln!("su: authentication failure");
            return 1;
        }
    }

    // Session tracking for login shells.
    let is_login = opts.login && opts.command.is_none();
    if is_login {
        let tty = detect_tty();
        record_session(&target.username, &tty);
    }

    let exit_code = exec_as_user(
        target,
        opts.shell.as_deref(),
        opts.command.as_deref(),
        opts.login,
        opts.preserve_env,
    );

    if is_login {
        remove_session(&target.username);
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

    let mut i = 1; // skip argv[0]

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-u" | "--user" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("sudo: option '{arg}' requires an argument");
                    return Err(1);
                }
                opts.target_user = args[i].clone();
            }
            "-l" | "--list" => {
                opts.list_mode = true;
            }
            "-h" | "--help" => {
                print_sudo_help();
                return Err(0);
            }
            "-V" | "--version" => {
                println!("sudo (OurOS) 0.1.0");
                return Err(0);
            }
            _ => {
                // Everything from here on is the command and its args.
                opts.command = args[i..].to_vec();
                break;
            }
        }
        i += 1;
    }

    if !opts.list_mode && opts.command.is_empty() {
        eprintln!("sudo: no command specified");
        eprintln!("Try 'sudo --help' for usage.");
        return Err(1);
    }

    Ok(opts)
}

fn print_sudo_help() {
    println!("OurOS Sudo v0.1.0");
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
fn sudo_authorised(caller: &User, target_uid: u32) -> bool {
    // Root can always sudo.
    if caller.uid == 0 {
        return true;
    }

    // For non-root targets, the caller must be root.
    if target_uid != 0 {
        // Allow wheel/admin members to sudo to any user.
        return caller.groups.iter().any(|g| g == "wheel" || g == "admin") || caller.admin;
    }

    // Wheel/admin members can sudo to root.
    caller.groups.iter().any(|g| g == "wheel" || g == "admin") || caller.admin
}

/// Print the caller's sudo permissions.
fn sudo_list_permissions(caller: &User) {
    println!("User {} may run the following commands:", caller.username);
    if caller.uid == 0 {
        println!("    (ALL) ALL");
    } else if caller.admin || caller.groups.iter().any(|g| g == "wheel" || g == "admin") {
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

    let users = read_users();
    if users.is_empty() {
        eprintln!("sudo: no user database found at {USER_DB_PATH}");
        return 1;
    }

    let caller_uid = get_caller_uid(&users);
    let caller = match find_user_by_uid(&users, caller_uid) {
        Some(u) => u,
        None => {
            eprintln!(
                "sudo: unknown calling user (uid {caller_uid}); \
                 cannot determine permissions"
            );
            return 1;
        }
    };

    if opts.list_mode {
        sudo_list_permissions(caller);
        return 0;
    }

    let target = match find_user(&users, &opts.target_user) {
        Some(u) => u,
        None => {
            eprintln!("sudo: unknown user: {}", opts.target_user);
            return 1;
        }
    };

    if target.locked {
        eprintln!("sudo: account '{}' is locked", target.username);
        return 1;
    }

    // Authorisation check.
    if !sudo_authorised(caller, target.uid) {
        eprintln!(
            "sudo: user '{}' is not in the sudoers file. \
             This incident will be reported.",
            caller.username
        );
        // Log the failed attempt.
        log_sudo_failure(&caller.username, &opts.command);
        return 1;
    }

    // Authenticate: require the caller's own password (sudo convention),
    // unless the caller is root.
    if caller.uid != 0 {
        if caller.password_hash.is_empty() {
            eprintln!("sudo: your account has no password set");
            return 1;
        }

        let password = match read_password(&format!("[sudo] password for {}: ", caller.username)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("sudo: {e}");
                return 1;
            }
        };

        if !verify_password(&password, &caller.password_hash, &caller.salt) {
            eprintln!("sudo: authentication failure");
            return 1;
        }
    }

    // Execute the command as the target user.
    let command_str = opts.command.join(" ");
    let exit_code = exec_as_user(
        target,
        None,
        Some(&command_str),
        false, // not a login shell
        false, // don't preserve env
    );

    exit_code
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- SHA-256 ---

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_long_input() {
        // SHA-256 of 64 'a' bytes (spans exactly one block after padding).
        let input = vec![b'a'; 64];
        let result = sha256_hex(&input);
        // Known hash for "aaaa...a" (64 a's):
        // ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb
        assert_eq!(
            result,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }

    // --- Password hashing ---

    #[test]
    fn test_hash_password_matches_verify() {
        let salt = "abcdef1234567890";
        let password = "secret123";
        let hash = hash_password(password, salt);
        assert!(verify_password(password, &hash, salt));
    }

    #[test]
    fn test_hash_password_wrong_password_fails() {
        let salt = "abcdef1234567890";
        let hash = hash_password("correct_password", salt);
        assert!(!verify_password("wrong_password", &hash, salt));
    }

    #[test]
    fn test_hash_password_wrong_salt_fails() {
        let salt = "salt_a";
        let hash = hash_password("password", salt);
        assert!(!verify_password("password", &hash, "salt_b"));
    }

    #[test]
    fn test_verify_password_length_mismatch() {
        assert!(!verify_password("x", "short", "s"));
    }

    // --- User database parsing ---

    fn sample_users_yaml() -> &'static str {
        r#"# OurOS user database
users:
  - uid: 0
    username: "root"
    display_name: "System Administrator"
    password_hash: "abc123"
    salt: "salt0"
    shell: "/bin/sh"
    home: "/root"
    groups: [root, admin, wheel]
    admin: true
    locked: false
  - uid: 1000
    username: "alice"
    display_name: "Alice"
    password_hash: "def456"
    salt: "salt1"
    shell: "/bin/bash"
    home: "/home/alice"
    groups: [users, wheel]
    admin: false
    locked: false
  - uid: 1001
    username: "bob"
    display_name: "Bob"
    password_hash: ""
    salt: ""
    shell: "/bin/sh"
    home: "/home/bob"
    groups: [users]
    admin: false
    locked: true
"#
    }

    /// Helper: parse sample YAML directly (bypasses file I/O).
    fn parse_sample_users() -> Vec<User> {
        let content = sample_users_yaml();
        let mut users = Vec::new();
        let mut current: Option<User> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
                if let Some(user) = current.take() {
                    users.push(user);
                }
                let uid: u32 = trimmed
                    .split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                current = Some(User {
                    uid,
                    username: String::new(),
                    display_name: String::new(),
                    password_hash: String::new(),
                    salt: String::new(),
                    shell: "/bin/sh".to_string(),
                    home: String::new(),
                    groups: Vec::new(),
                    admin: false,
                    locked: false,
                });
            } else if let Some(ref mut user) = current {
                if let Some(val) = trimmed.strip_prefix("username:") {
                    user.username = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("display_name:") {
                    user.display_name = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("password_hash:") {
                    user.password_hash = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("salt:") {
                    user.salt = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("shell:") {
                    user.shell = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("home:") {
                    user.home = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = trimmed.strip_prefix("groups:") {
                    let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                    user.groups = val
                        .split(',')
                        .map(|g| g.trim().trim_matches('"').to_string())
                        .filter(|g| !g.is_empty())
                        .collect();
                } else if let Some(val) = trimmed.strip_prefix("admin:") {
                    user.admin = val.trim() == "true";
                } else if let Some(val) = trimmed.strip_prefix("locked:") {
                    user.locked = val.trim() == "true";
                }
            }
        }
        if let Some(user) = current {
            users.push(user);
        }
        users
    }

    #[test]
    fn test_parse_user_count() {
        let users = parse_sample_users();
        assert_eq!(users.len(), 3);
    }

    #[test]
    fn test_parse_root_user() {
        let users = parse_sample_users();
        let root = find_user(&users, "root").expect("root should exist");
        assert_eq!(root.uid, 0);
        assert_eq!(root.home, "/root");
        assert_eq!(root.shell, "/bin/sh");
        assert!(root.admin);
        assert!(!root.locked);
        assert!(root.groups.contains(&"wheel".to_string()));
    }

    #[test]
    fn test_parse_normal_user() {
        let users = parse_sample_users();
        let alice = find_user(&users, "alice").expect("alice should exist");
        assert_eq!(alice.uid, 1000);
        assert_eq!(alice.home, "/home/alice");
        assert_eq!(alice.shell, "/bin/bash");
        assert!(!alice.admin);
        assert!(!alice.locked);
        assert!(alice.groups.contains(&"wheel".to_string()));
    }

    #[test]
    fn test_parse_locked_user() {
        let users = parse_sample_users();
        let bob = find_user(&users, "bob").expect("bob should exist");
        assert_eq!(bob.uid, 1001);
        assert!(bob.locked);
        assert!(bob.password_hash.is_empty());
    }

    #[test]
    fn test_find_user_nonexistent() {
        let users = parse_sample_users();
        assert!(find_user(&users, "nonexistent").is_none());
    }

    #[test]
    fn test_find_user_by_uid() {
        let users = parse_sample_users();
        let root = find_user_by_uid(&users, 0).expect("uid 0 should exist");
        assert_eq!(root.username, "root");
        let alice = find_user_by_uid(&users, 1000).expect("uid 1000 should exist");
        assert_eq!(alice.username, "alice");
        assert!(find_user_by_uid(&users, 9999).is_none());
    }

    // --- sudo authorisation ---

    #[test]
    fn test_sudo_root_always_authorised() {
        let users = parse_sample_users();
        let root = find_user(&users, "root").unwrap();
        assert!(sudo_authorised(root, 0));
        assert!(sudo_authorised(root, 1000));
        assert!(sudo_authorised(root, 1001));
    }

    #[test]
    fn test_sudo_wheel_member_authorised_for_root() {
        let users = parse_sample_users();
        let alice = find_user(&users, "alice").unwrap();
        // Alice is in wheel group -> can sudo to root.
        assert!(sudo_authorised(alice, 0));
    }

    #[test]
    fn test_sudo_wheel_member_authorised_for_other() {
        let users = parse_sample_users();
        let alice = find_user(&users, "alice").unwrap();
        // Wheel members can sudo to any user.
        assert!(sudo_authorised(alice, 1001));
    }

    #[test]
    fn test_sudo_non_wheel_denied() {
        let users = parse_sample_users();
        let bob = find_user(&users, "bob").unwrap();
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

    #[test]
    fn test_full_auth_flow() {
        // Simulate: useradm hashes "secret" with salt "testsalt".
        let salt = "testsalt";
        let password = "secret";
        let stored_hash = hash_password(password, salt);

        // Correct password verifies.
        assert!(verify_password(password, &stored_hash, salt));

        // Wrong password fails.
        assert!(!verify_password("wrong", &stored_hash, salt));

        // Different salt fails.
        assert!(!verify_password(password, &stored_hash, "othersalt"));
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
