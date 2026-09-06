//! Slate OS User Switching Utility (`su`)
//!
//! Switch to another user's identity and optionally run a command.
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
//! # This program is not `sudo`
//!
//! It used to be, in part: invoked through an `argv[0]` of `sudo` it ran a
//! second, much smaller command-runner whose entire policy was "root, or a
//! member of `wheel`/`admin`, may run anything as anyone". That personality is
//! gone. `userspace/sudo` is the real implementation — it parses
//! `/etc/sudoers`, honours per-user and per-host rules, `Defaults`, `NOPASSWD`
//! and `env_keep`, and ships `visudo`.
//!
//! Deleting it was a security fix, not tidying. The copy here never read
//! `/etc/sudoers` at all, so on any system where both binaries existed an
//! administrator who revoked a user's rights in `/etc/sudoers` had not revoked
//! anything: the same user invoking the other binary still got `(ALL) ALL` on
//! the strength of a `wheel` membership. Two programs answering "may this user
//! run this command as root?" differently is a policy split, and the safe
//! number of answers to that question is one. See `known-issues.md`
//! (TD-B-TWO-PROGRAMS-BOTH-CLAIM-THE-NAME-`sudo`).
//!
//! # Authentication
//!
//! Reads `/etc/users.yaml` through the shared `userdb` crate. Passwords are
//! `crypt(3)` entries — SHA-512-crypt — and are checked by re-running `crypt`
//! on the stored entry, which is a valid setting for itself. Root (uid 0) can
//! switch to any user without a password.
//!
//! # Session tracking
//!
//! On login-shell switches, writes a session file to `/run/sessions/`
//! and a fallback marker to `/tmp/.users/` so that `who`/`w` can
//! report the logged-in user.

use quoting::{os_bytes, quoteaf_os};
use std::env;
use std::ffi::{OsStr, OsString};
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
/// `who` is the name to report the failure under. It is always `"su"` now that
/// this binary has one personality, but it stays a parameter because the
/// messages below are the ones an administrator reads when the database is
/// unavailable, and hard-coding the program name into them is how a message
/// ends up naming the wrong program after a rename.
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
///
/// # The shared failed-attempt tally
///
/// Every guess here counts against the same `authlib` tally the console
/// `login` prompt uses, and a delay earned at either one is honoured at both
/// (`design-decisions.md` §354). A limit only one of them obeys is not a
/// limit: an attacker slowed to one guess per five minutes at the login
/// prompt would simply run `su` and guess at full speed.
///
/// **Only a *typed* password counts.** The three checks above the prompt —
/// locked, no password set, unverifiable legacy hash — return without ever
/// asking for one, and counting them would hand any local user a free denial
/// of service: `su alice` against a locked account, run five times, would lock
/// alice out of her own console without the attacker knowing anything at all.
/// The tally exists to make *guessing* expensive, and a refusal that involved
/// no guess is not an attempt.
///
/// A user already inside a delay is refused *before* the account-state checks,
/// so the refusal cannot be used as an oracle for whether the account is
/// locked or has a password set.
fn authenticate(
    auth: &mut authlib::Authenticator,
    record: &Record,
    prompt: &str,
    who: &str,
) -> bool {
    let name = name_of(record);

    if auth.rate_limited(&name).is_some() {
        // `retry_after_secs` is not surfaced: telling the caller exactly how
        // long is left tells them their guesses are landing on a real account.
        eprintln!(
            "{who}: {}",
            authlib::Outcome::RateLimited {
                retry_after_secs: 0,
            }
            .user_message()
        );
        return false;
    }

    if record.is_locked() {
        eprintln!("{who}: account {} is locked", quoteaf_os(&name));
        return false;
    }

    // Probing with the empty password distinguishes "no password stored" from
    // "the stored password is the empty string": the first answers
    // `NoPassword`, the second `Accepted`, and only the first is refused here.
    if record.check_password("") == Auth::NoPassword {
        eprintln!("{who}: account {} has no password set", quoteaf_os(&name));
        return false;
    }

    if record.has_legacy_password() {
        eprintln!(
            "{who}: account {} has a password stored in a format this system can no longer verify; run `useradm passwd {name}` as root",
            quoteaf_os(&name)
        );
        return false;
    }

    let password = match read_password(prompt) {
        Ok(p) => p,
        Err(e) => {
            // No guess was made — the terminal failed us, not the user.
            eprintln!("{who}: {e}");
            return false;
        }
    };

    match record.check_password(&password) {
        Auth::Accepted => {
            auth.reset(&name);
            true
        }
        Auth::Locked | Auth::Unusable | Auth::NoPassword | Auth::Rejected => {
            // The three non-`Rejected` cases were ruled out above and can only
            // arise from a change under our feet; they get the same message
            // because at this point the password has already been typed and a
            // detailed answer would say something about the account to whoever
            // typed it.
            auth.note_failure(&name);
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
    //
    // `var_os` and an explicit `to_str`, not `var`. A value that is not text
    // cannot name a user in a YAML file, so the outcome is the same either
    // way — but `var` reports it as `Err(NotUnicode)`, which an `if let Ok`
    // silently treats as "unset". The two are different facts, and writing
    // code that cannot tell them apart is how `sudo` ended up ignoring a set
    // `EDITOR` (see known-issues.md, TD-B-SUDO-...).
    if let Some(name) = env::var_os("USER")
        && let Some(name) = name.to_str()
        && let Some(user) = users.find(name)
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

/// Whether `name` is safe to use as a single filename under a directory this
/// program creates.
///
/// The fallback marker is written to `/tmp/.users/<username>`, with the name
/// coming from the user database — so a database entry named `../../etc/shadow`
/// would have this program, running as root, truncate a file two directories up.
/// The database is administrator-controlled and this is therefore not a local
/// privilege escalation, but "only an administrator can trigger it" is not the
/// same as "it is fine", and the guard costs one comparison.
///
/// Empty, `.`, `..`, and anything containing `/` or a NUL are refused; those are
/// the only byte sequences that can escape the directory, since a path here is
/// bytes with `/` as the one separator (`design.txt`).
fn is_safe_filename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// Record a login session so that `who` and `w` can see it.
///
/// Writes to both `/run/sessions/<pid>` (Slate OS native) and
/// `/tmp/.users/<username>` (fallback). Errors are non-fatal since
/// the switch itself should still proceed.
fn record_session(username: &str, tty: &OsStr) {
    let now = current_epoch_secs();
    let pid = process::id();

    // Slate OS native session file. Assembled as **bytes**, because `tty` is a
    // device name and a device name is bytes on this OS. It used to be built
    // with `format!`, which meant `detect_tty` had to hand over a `String` and
    // therefore had to run the name through `to_string_lossy` first — so a
    // terminal whose name is not valid UTF-8 was recorded as one with U+FFFD
    // in it, and `who` reported a terminal nobody is on.
    let session_dir = "/run/sessions";
    let _ = fs::create_dir_all(session_dir);
    let mut session_content = Vec::new();
    session_content.extend_from_slice(b"user=");
    session_content.extend_from_slice(username.as_bytes());
    session_content.extend_from_slice(b"\ntty=");
    session_content.extend_from_slice(&os_bytes(tty));
    session_content.extend_from_slice(b"\nhost=\ntime=");
    session_content.extend_from_slice(now.to_string().as_bytes());
    session_content.extend_from_slice(b"\npid=");
    session_content.extend_from_slice(pid.to_string().as_bytes());
    session_content.push(b'\n');
    let _ = fs::write(format!("{session_dir}/{pid}"), &session_content);

    // Fallback marker for /tmp/.users/.
    if !is_safe_filename(username) {
        return;
    }
    let users_dir = "/tmp/.users";
    let _ = fs::create_dir_all(users_dir);
    let _ = fs::write(format!("{users_dir}/{username}"), format!("{now}"));
}

/// Remove session records when the shell exits.
fn remove_session(username: &str) {
    let pid = process::id();
    let _ = fs::remove_file(format!("/run/sessions/{pid}"));
    // Same guard as the write: a name that was never used to create a file
    // must not be used to delete one either.
    if is_safe_filename(username) {
        let _ = fs::remove_file(format!("/tmp/.users/{username}"));
    }
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
    shell_override: Option<&OsStr>,
    command: Option<&OsStr>,
    login_mode: bool,
    preserve_env: bool,
) -> i32 {
    let target_shell = shell_of(target);
    let target_home = home_of(target);
    let target_name = name_of(target);
    let target_uid = target.uid().unwrap_or(u32::MAX);
    // `-s` names a path, and a path on this OS is bytes; the database's own
    // `shell:` field comes from YAML and so is text. Both are borrowed as
    // `&OsStr`, which is the type that can hold either.
    let shell: &OsStr = shell_override.unwrap_or_else(|| target_shell.as_ref());

    let mut cmd = process::Command::new(shell);

    // `-c` mode passes ["-c", "command"] through untouched: the command is
    // the shell's to parse, and this program must not narrow what it can say.
    //
    // Interactive mode passes nothing. There used to be a computed argv[0]
    // here — `-bash` for a login shell, per the convention a shell uses to
    // decide whether to read its login profile — and it was dead code, as its
    // own comment conceded: `std::process::Command` sets argv[0] to the
    // program path and offers no way to override it, so the value was built
    // and then dropped on the floor for interactive shells and passed as a
    // *positional argument* for none. It is gone rather than left in place,
    // because a computation whose result is discarded reads to the next
    // person as a feature that works. The convention needs an exec that takes
    // argv[0] separately (`SYS_PROCESS_SPAWN_EX2`); see `todo.txt`.
    if let Some(c) = command {
        cmd.arg("-c");
        cmd.arg(c);
    }

    if login_mode && !preserve_env {
        // Clean environment: only set what a login shell expects.
        cmd.env_clear();
        cmd.env("HOME", &target_home);
        cmd.env("SHELL", shell);
        cmd.env("USER", &target_name);
        cmd.env("LOGNAME", &target_name);
        cmd.env("PATH", default_path_for_uid(target_uid));

        // Propagate TERM if set -- shells need it for line editing.
        //
        // `var_os`, not `var`: the value is handed straight back to a child
        // process, so this program never needs to read it as text, and `var`
        // would drop a non-UTF-8 terminal name on the floor as
        // `Err(NotUnicode)` — leaving the login shell with no TERM at all and
        // therefore no line editing, which is the failure a user reports as
        // "su - breaks my arrow keys".
        if let Some(term) = env::var_os("TERM") {
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
            eprintln!("su: failed to execute {}: {e}", quoteaf_os(shell));
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
///
/// Returns an `OsString` rather than a `String` because a device name is bytes
/// on this OS, and the caller writes it into a file `who` and `w` read back.
/// The previous version went through `to_string_lossy`, so a terminal whose
/// name is not valid UTF-8 was recorded with U+FFFD substituted for the bytes
/// that made it unique — two such terminals recorded identically, and neither
/// name could be matched against anything real.
///
/// The prefix is stripped with `Path::strip_prefix`, not `str::strip_prefix`:
/// the string form would need a lossy conversion first, and it is the
/// conversion that loses the bytes.
///
/// # Why the name is screened for control bytes
///
/// `/proc/self/fd/0` names whatever stdin happens to be, which the *caller*
/// chooses — `su - alice < /dev/shm/evil` is an ordinary redirection any
/// unprivileged user may perform. The name goes into a `key=value\n` record,
/// so a name containing a newline appends attacker-chosen fields to it, and a
/// name containing a NUL truncates the record where the reader splits. Neither
/// is a name any real terminal has, so screening costs nothing and closes the
/// injection: a suspicious name is reported as unknown rather than sanitised,
/// because a *silently repaired* name is one `who` reports as real.
fn detect_tty() -> OsString {
    // Try /proc/self/fd/0 symlink.
    if let Ok(link) = fs::read_link("/proc/self/fd/0")
        && let Ok(rest) = link.strip_prefix("/dev/")
        && tty_name_is_plausible(rest.as_os_str())
    {
        return rest.as_os_str().to_os_string();
    }
    OsString::from("?")
}

/// Whether `name` could be a terminal's name, as opposed to something chosen
/// to be written into a `key=value` record.
///
/// Split out from [`detect_tty`] so it can be tested: the input the real
/// function reads is `/proc/self/fd/0`, which a unit test cannot arrange.
///
/// Every byte below `0x20` and `0x7f` (DEL) is refused. `\n` is the one that
/// matters — it ends a field — but a NUL truncates at the syscall boundary and
/// a `\r` can hide the rest of a line on a terminal, and no terminal has ever
/// been named with any of them, so the whole control range goes.
fn tty_name_is_plausible(name: &OsStr) -> bool {
    let bytes = os_bytes(name);
    !bytes.is_empty() && !bytes.iter().any(|b| *b < 0x20 || *b == 0x7f)
}

// ============================================================================
// su mode
// ============================================================================

/// Parsed options for `su`.
///
/// Every field that comes from the command line is an `OsString`. Two of them
/// name things the kernel names in bytes — a shell is a path, and a `-c`
/// command is handed to that shell verbatim — so narrowing them to `String`
/// would mean this program could not express arguments the system it runs on
/// can. `target_user` is text in the end (the database is YAML), but it is
/// held as an `OsString` so that a name which *is not* text can be reported
/// back to the user instead of aborting the process: `env::args()` panics on a
/// non-UTF-8 argument, which killed `su` before it reached its first statement.
#[derive(Debug)]
struct SuOptions {
    /// Target username (default: "root").
    target_user: OsString,
    /// Login shell mode (-l, --login, leading `-`).
    login: bool,
    /// Command to run via shell -c.
    command: Option<OsString>,
    /// Preserve the caller's environment (-m, -p, --preserve-environment).
    preserve_env: bool,
    /// Override the target user's shell (-s, --shell).
    shell: Option<OsString>,
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
///
/// # Dispatching on `to_str()`
///
/// Every option this program understands is ASCII, so an argument that is not
/// valid text cannot be one of them and the `None` arm of `to_str()` falls
/// through to the positional/unknown-option branch. That branch must not use
/// the same `None` as its own test, though: whether an argument is an option
/// is decided by its **first byte**, not by whether it is text. `-` followed
/// by a non-UTF-8 byte is a misspelled option and has to be rejected as one;
/// treating it as a username instead would make `su -<garbage>` silently try
/// to become a user by that name, and — because the last positional wins —
/// silently discard a real username given after it.
fn parse_su_args(args: &[OsString]) -> Result<SuOptions, i32> {
    let mut opts = SuOptions {
        target_user: OsString::from("root"),
        login: false,
        command: None,
        preserve_env: false,
        shell: None,
    };

    let mut positional: Vec<OsString> = Vec::new();
    // Driven by the iterator rather than an index, so that "this option takes
    // a value" is expressed by consuming the next item and cannot run off the
    // end of the slice.
    let mut rest = args.iter().skip(1);

    while let Some(arg) = rest.next() {
        match arg.to_str() {
            Some("-" | "-l" | "--login") => {
                opts.login = true;
            }
            Some("-c" | "--command") => {
                let Some(value) = rest.next() else {
                    eprintln!("su: option {} requires an argument", quoteaf_os(arg));
                    return Err(1);
                };
                opts.command = Some(value.clone());
            }
            Some("-m" | "-p" | "--preserve-environment") => {
                opts.preserve_env = true;
            }
            Some("-s" | "--shell") => {
                let Some(value) = rest.next() else {
                    eprintln!("su: option {} requires an argument", quoteaf_os(arg));
                    return Err(1);
                };
                opts.shell = Some(value.clone());
            }
            Some("-h" | "--help") => {
                print_su_help();
                return Err(0);
            }
            Some("-V" | "--version") => {
                println!("su (Slate OS) 0.1.0");
                return Err(0);
            }
            _ => {
                if os_bytes(arg).first() == Some(&b'-') {
                    eprintln!("su: unknown option: {}", quoteaf_os(arg));
                    eprintln!("Try 'su --help' for usage.");
                    return Err(1);
                }
                positional.push(arg.clone());
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
fn run_su(args: &[OsString]) -> i32 {
    let opts = match parse_su_args(args) {
        Ok(o) => o,
        Err(code) => return code,
    };

    let Some(users) = load_users("su") else {
        return 1;
    };

    // A name that is not text cannot appear in a YAML database, so it is an
    // unknown user — reported through the same message rather than a second
    // one, because from the caller's side the two are the same fact. The
    // message quotes the name (`quoteaf_os`); it used to interpolate it raw,
    // which let `su $'root\nsu: switched to root'` write a convincing second
    // line onto the caller's terminal.
    let Some(target) = opts.target_user.to_str().and_then(|n| users.find(n)) else {
        eprintln!("su: unknown user: {}", quoteaf_os(&opts.target_user));
        return 1;
    };

    if target.is_locked() {
        eprintln!("su: account {} is locked", quoteaf_os(name_of(target)));
        return 1;
    }

    // Authenticate unless the caller is root. The tally is keyed by the
    // account whose password is being guessed — here the *target*, since `su`
    // asks for the password of the user you are becoming.
    let caller_uid = get_caller_uid(&users);
    let mut auth = authlib::Authenticator::new();
    if caller_uid != 0 && !authenticate(&mut auth, target, "Password: ", "su") {
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
// Entry point
// ============================================================================

// This used to dispatch on `basename(argv[0])`, running a built-in `sudo` when
// the binary was invoked under that name. It does not any more, and the
// `basename` helper went with it: there is exactly one program here now, so
// consulting `argv[0]` to decide what to be could only ever pick wrong. A
// symlink named `sudo` pointing at this binary now runs `su`, which is the
// honest outcome — the `sudo` behaviour lives in `userspace/sudo`.

fn main() {
    // `args_os`, not `args`. `env::args()` panics on an argument that is not
    // valid UTF-8, and on this OS a path may hold every byte but `/` and NUL —
    // so `su -s /bin/<non-utf8> alice` did not fail with a message, it aborted
    // the process before `run_su` ran a single statement, printing a panic that
    // named this program's internals rather than the user's mistake.
    let args: Vec<OsString> = env::args_os().collect();
    process::exit(run_su(&args));
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

    // The sudo-authorisation tests that used to sit here are gone with the
    // policy they tested. They asserted this binary's own "root, wheel or
    // admin may run anything" rule, which was never consulted by the real
    // `sudo` and is not the rule `userspace/sudo` applies; keeping them would
    // have pinned a second, contradictory answer to "who may run what as
    // root". Authorisation coverage belongs to `userspace/sudo` and its
    // `/etc/sudoers` parser.

    // --- su argument parsing ---

    /// A command line, as `main` would hand it over.
    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    /// An `OsString` that is *not* valid text, on either host.
    ///
    /// This exists to be a portable fixture, and the portability is the whole
    /// point. The obvious way to build one is a raw byte such as `0xff` via
    /// `OsStringExt::from_vec`, which is unix-only — so a test written that way
    /// has to be `#[cfg(unix)]`, which on this project's Windows development
    /// host means it never runs at all. That is not hypothetical: 198 tests
    /// across 40 files are gated that way and have never once executed here
    /// (`known-issues.md`, TD-B-CFG-UNIX-GATED-TESTS-RUN-NOWHERE).
    ///
    /// The Windows arm uses an unpaired surrogate (`U+D800`), which `OsString`
    /// stores — its internal encoding is WTF-8 — and which `to_str()` refuses,
    /// exactly as byte `0xff` is refused on unix. The two hosts disagree about
    /// *which* sequence is unrepresentable, but agree that this one is, and
    /// that agreement is all these tests rest on.
    ///
    /// The fixture asserts its own postcondition, so a future standard-library
    /// or platform change that made either sequence valid text fails loudly
    /// here rather than quietly turning every caller into a duplicate of the
    /// plain-ASCII test next to it.
    fn not_text(prefix: &str, suffix: &str) -> OsString {
        #[cfg(unix)]
        let out = {
            use std::os::unix::ffi::OsStringExt;
            let mut v = prefix.as_bytes().to_vec();
            v.push(0xff);
            v.extend_from_slice(suffix.as_bytes());
            OsString::from_vec(v)
        };
        #[cfg(not(unix))]
        let out = {
            use std::os::windows::ffi::OsStringExt;
            let mut w: Vec<u16> = prefix.encode_utf16().collect();
            w.push(0xD800);
            w.extend(suffix.encode_utf16());
            OsString::from_wide(&w)
        };
        assert!(out.to_str().is_none(), "the fixture must not be valid text");
        out
    }

    #[test]
    fn test_su_args_default() {
        let opts = parse_su_args(&argv(&["su"])).unwrap();
        assert_eq!(opts.target_user, "root");
        assert!(!opts.login);
        assert!(opts.command.is_none());
        assert!(!opts.preserve_env);
        assert!(opts.shell.is_none());
    }

    #[test]
    fn test_su_args_user() {
        let opts = parse_su_args(&argv(&["su", "alice"])).unwrap();
        assert_eq!(opts.target_user, "alice");
        assert!(!opts.login);
    }

    #[test]
    fn test_su_args_login_dash() {
        let opts = parse_su_args(&argv(&["su", "-"])).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn test_su_args_login_dash_user() {
        let opts = parse_su_args(&argv(&["su", "-", "alice"])).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_login_l() {
        let opts = parse_su_args(&argv(&["su", "-l", "bob"])).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, "bob");
    }

    #[test]
    fn test_su_args_command() {
        let opts = parse_su_args(&argv(&["su", "-c", "whoami", "alice"])).unwrap();
        assert_eq!(opts.command.as_deref(), Some(OsStr::new("whoami")));
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_preserve_env() {
        let opts = parse_su_args(&argv(&["su", "-m", "alice"])).unwrap();
        assert!(opts.preserve_env);
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_shell_override() {
        let opts = parse_su_args(&argv(&["su", "-s", "/bin/zsh", "root"])).unwrap();
        assert_eq!(opts.shell.as_deref(), Some(OsStr::new("/bin/zsh")));
        assert_eq!(opts.target_user, "root");
    }

    #[test]
    fn test_su_args_command_missing_arg() {
        assert_eq!(parse_su_args(&argv(&["su", "-c"])).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_shell_missing_arg() {
        assert_eq!(parse_su_args(&argv(&["su", "-s"])).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_unknown_option() {
        assert_eq!(parse_su_args(&argv(&["su", "--bogus"])).unwrap_err(), 1);
    }

    #[test]
    fn test_su_args_help() {
        assert_eq!(parse_su_args(&argv(&["su", "--help"])).unwrap_err(), 0);
    }

    #[test]
    fn test_su_args_version() {
        assert_eq!(parse_su_args(&argv(&["su", "--version"])).unwrap_err(), 0);
    }

    // --- Arguments that are not text ---
    //
    // `env::args()` panicked on every one of these, so before the conversion
    // this program did not misparse them — it died before `run_su` executed a
    // statement, and printed a panic naming this file rather than the mistake.
    // The parser must now carry them intact to the place that can report them.

    /// A shell path is a path, and a path on this OS is any bytes but `/` and
    /// NUL. `-s` therefore has to survive one that is not text — this is the
    /// case with a real user behind it, since the shell is *executed*, so a
    /// name this program cannot express is a shell it cannot start.
    #[test]
    fn a_shell_override_that_is_not_text_survives_parsing() {
        let shell = not_text("/bin/", "sh");
        let args = vec![
            OsString::from("su"),
            OsString::from("-s"),
            shell.clone(),
            OsString::from("root"),
        ];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.shell.as_deref(), Some(shell.as_os_str()));
        assert_eq!(opts.target_user, "root");
    }

    /// `-c` hands its argument to the shell verbatim, so this program must not
    /// narrow what may be said in it. A command that names a file whose name
    /// is not text is the ordinary case.
    #[test]
    fn a_command_that_is_not_text_survives_parsing() {
        let command = not_text("cat /tmp/", ".log");
        let args = vec![OsString::from("su"), OsString::from("-c"), command.clone()];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.command.as_deref(), Some(command.as_os_str()));
        // Still the default target: the command consumed the only positional.
        assert_eq!(opts.target_user, "root");
    }

    /// A username that is not text reaches the lookup as itself. It will not
    /// be found — the database is YAML and YAML is text — but *not being
    /// found* is the outcome, reported with a quoted name, rather than a
    /// process that never started.
    #[test]
    fn a_username_that_is_not_text_reaches_the_lookup_intact() {
        let name = not_text("ali", "ce");
        let args = vec![OsString::from("su"), name.clone()];
        let opts = parse_su_args(&args).unwrap();
        assert_eq!(opts.target_user, name);
        assert!(
            opts.target_user.to_str().is_none(),
            "run_su reports this as an unknown user rather than looking it up"
        );
    }

    /// The branch that decides "option or positional" tests the **first byte**,
    /// not whether the argument is text. Deciding it by `to_str()` instead
    /// would make a mistyped option that happens to contain a stray byte into
    /// a *username*, and — since the last positional wins — silently discard
    /// the real username typed after it, running a shell as the wrong user.
    #[test]
    fn a_leading_dash_is_an_unknown_option_even_when_the_rest_is_not_text() {
        let bogus = not_text("--bog", "us");
        let args = vec![OsString::from("su"), bogus, OsString::from("alice")];
        assert_eq!(
            parse_su_args(&args).unwrap_err(),
            1,
            "must be refused as an option, not accepted as a username"
        );
    }

    /// The mirror of the case above: no leading dash means positional, whatever
    /// the remaining bytes are.
    #[test]
    fn no_leading_dash_is_a_positional_even_when_the_rest_is_not_text() {
        let name = not_text("bo", "b");
        let args = vec![OsString::from("su"), name.clone(), OsString::from("-l")];
        let opts = parse_su_args(&args).unwrap();
        assert!(opts.login);
        assert_eq!(opts.target_user, name);
    }

    /// A value consumed by `-s`/`-c` is never re-examined as an option, so a
    /// shell path that legitimately begins with `-` is a value, not a flag.
    #[test]
    fn an_option_value_beginning_with_a_dash_is_still_a_value() {
        let opts = parse_su_args(&argv(&["su", "-c", "-l", "alice"])).unwrap();
        assert_eq!(opts.command.as_deref(), Some(OsStr::new("-l")));
        assert!(!opts.login, "the -l was the command, not a flag");
        assert_eq!(opts.target_user, "alice");
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
        let opts = parse_su_args(&argv(&["su", "first", "second"])).unwrap();
        assert_eq!(opts.target_user, "second");
    }

    #[test]
    fn test_su_args_login_and_command() {
        let opts = parse_su_args(&argv(&["su", "-l", "-c", "id", "alice"])).unwrap();
        assert!(opts.login);
        assert_eq!(opts.command.as_deref(), Some(OsStr::new("id")));
        assert_eq!(opts.target_user, "alice");
    }

    #[test]
    fn test_su_args_all_flags_combined() {
        let opts = parse_su_args(&argv(&[
            "su",
            "-l",
            "-p",
            "-s",
            "/bin/fish",
            "-c",
            "uname -a",
            "root",
        ]))
        .unwrap();
        assert!(opts.login);
        assert!(opts.preserve_env);
        assert_eq!(opts.shell.as_deref(), Some(OsStr::new("/bin/fish")));
        assert_eq!(opts.command.as_deref(), Some(OsStr::new("uname -a")));
        assert_eq!(opts.target_user, "root");
    }

    // --- The session record `who` and `w` read ---

    /// The record is `key=value` lines, so a value carrying a newline appends
    /// fields to it. `/proc/self/fd/0` names whatever stdin is, and stdin is
    /// the *caller's* to choose — `su - alice < '/dev/shm/x\nuser=root'` is an
    /// ordinary redirection — so this is reachable by an unprivileged user and
    /// would have `who` report a root session that does not exist.
    #[test]
    fn a_terminal_name_that_could_forge_a_session_field_is_refused() {
        for hostile in [
            &b"pts/0\nuser=root"[..],
            &b"pts/0\rwho cares"[..],
            &b"pts/0\0trailing"[..],
            b"",
        ] {
            let name = quoting::os_from_bytes(hostile);
            assert!(
                !tty_name_is_plausible(&name),
                "{:?} must not be recorded as a terminal name",
                String::from_utf8_lossy(hostile)
            );
        }
    }

    /// The names real terminals have must still get through — a screen that
    /// rejects everything is the same bug as one that accepts everything, just
    /// harder to notice, because the fallback `?` looks like a plausible
    /// answer.
    #[test]
    fn the_names_real_terminals_have_are_accepted() {
        for good in ["tty1", "pts/0", "ttyS0", "console"] {
            assert!(
                tty_name_is_plausible(OsStr::new(good)),
                "{good} is a real terminal name"
            );
        }
        // Including one that is not text: a device name is bytes on this OS,
        // and screening is for control characters, not for non-UTF-8.
        assert!(tty_name_is_plausible(&not_text("pts/", "0")));
    }

    /// The fallback marker is `/tmp/.users/<username>`, with the name coming
    /// from the database. A name of `..` or one containing `/` would have this
    /// program — running as root — write and later *delete* a file outside the
    /// directory it created.
    #[test]
    fn a_username_that_would_escape_the_marker_directory_is_refused() {
        for hostile in ["", ".", "..", "../etc/passwd", "a/b", "nul\0byte"] {
            assert!(
                !is_safe_filename(hostile),
                "{hostile:?} must not be used as a filename"
            );
        }
        for ordinary in ["root", "alice", "user.name", "a-b_c"] {
            assert!(
                is_safe_filename(ordinary),
                "{ordinary:?} is an ordinary name"
            );
        }
    }

    // --- The shared failed-attempt tally (§354) ---

    /// A verifier that counts in memory and nowhere else.
    ///
    /// `Authenticator::with_stores` deliberately attaches no faillock file, so
    /// running this suite cannot run up a delay against a real account on the
    /// developer's machine — which a test that used `Authenticator::new()`
    /// would do, silently, every time it ran.
    fn scratch_authenticator() -> authlib::Authenticator {
        let missing = std::path::Path::new("/nonexistent/su-tests");
        authlib::Authenticator::with_stores(missing, missing)
    }

    /// A record with a real, verifiable password.
    fn record_with_password(username: &str, password: &str) -> Record {
        let mut record = Record::new();
        record.set("username", username);
        record
            .set_password_with_salt(password, "0123456789abcdef")
            .expect("a 16-character salt is storable");
        record
    }

    /// A refusal that never asked for a password must not count against the
    /// account, or `su` becomes a denial-of-service tool: five runs of
    /// `su alice` against a locked account, costing the attacker nothing and
    /// telling them nothing, would lock alice out of her own console.
    #[test]
    fn a_refusal_that_asked_for_no_password_is_not_an_attempt() {
        let mut auth = scratch_authenticator();

        let mut locked = record_with_password("erin", "hunter2");
        locked.set_locked(true);
        for _ in 0..(FREE_ATTEMPTS_HEADROOM) {
            assert!(!authenticate(&mut auth, &locked, "Password: ", "su"));
        }
        assert_eq!(auth.failures("erin"), 0);
        assert!(auth.rate_limited("erin").is_none());

        // Same for an account with no password stored at all.
        let mut passwordless = Record::new();
        passwordless.set("username", "frank");
        for _ in 0..(FREE_ATTEMPTS_HEADROOM) {
            assert!(!authenticate(&mut auth, &passwordless, "Password: ", "su"));
        }
        assert_eq!(auth.failures("frank"), 0);

        // And for an entry whose stored hash predates `posix::crypt`.
        let mut legacy = Record::new();
        legacy.set("username", "dave");
        legacy.set(
            "password_hash",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        for _ in 0..(FREE_ATTEMPTS_HEADROOM) {
            assert!(!authenticate(&mut auth, &legacy, "Password: ", "su"));
        }
        assert_eq!(auth.failures("dave"), 0);
    }

    /// Enough attempts to be well past [`authlib::FREE_ATTEMPTS`].
    const FREE_ATTEMPTS_HEADROOM: u32 = authlib::FREE_ATTEMPTS + 3;

    /// A user already inside a delay is turned away without the guess being
    /// counted a second time. Counting it would let anyone hold a real user
    /// out indefinitely by making refused attempts that each refresh the
    /// clock — the delay would never expire.
    #[test]
    fn a_delayed_user_is_refused_and_the_refusal_is_not_counted() {
        let mut auth = scratch_authenticator();
        let record = record_with_password("grace", "hunter2");

        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            auth.note_failure("grace");
        }
        let before = auth.failures("grace");
        let delay_before = auth
            .rate_limited("grace")
            .expect("past the free attempts, grace is delayed");

        // The refusal happens without stdin being touched, so this returns
        // immediately whatever the harness has connected to it.
        assert!(!authenticate(&mut auth, &record, "Password: ", "su"));

        assert_eq!(
            auth.failures("grace"),
            before,
            "a refusal is not an attempt"
        );
        let delay_after = auth
            .rate_limited("grace")
            .expect("still delayed, but no further");
        assert!(
            delay_after <= delay_before,
            "the wait must run down, not restart: {delay_before}s then {delay_after}s"
        );
    }

    /// The delay is checked before the account-state messages, so that being
    /// refused for guessing cannot be told apart from being refused because
    /// the account is locked. Otherwise the rate limit itself becomes the
    /// oracle it exists to close.
    #[test]
    fn the_delay_is_checked_before_any_account_state_is_disclosed() {
        let mut auth = scratch_authenticator();
        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            auth.note_failure("heidi");
        }

        // A locked account and a healthy one are both refused, and neither
        // refusal moves the tally — the locked-account branch was never
        // reached, so it cannot have printed "account is locked".
        let mut locked = record_with_password("heidi", "hunter2");
        locked.set_locked(true);
        let before = auth.failures("heidi");
        assert!(!authenticate(&mut auth, &locked, "Password: ", "su"));
        assert_eq!(auth.failures("heidi"), before);
    }

    /// `su` and the console `login` prompt share one count, so a limit reached
    /// at either is honoured at both. This is the whole point of §354: an
    /// attacker slowed to one guess every five minutes at the login prompt
    /// must not be able to walk over to `su` and resume at full speed.
    #[test]
    fn su_and_login_share_one_tally_through_the_faillock_file() {
        let dir = std::env::temp_dir().join(format!(
            "su-faillock-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory under the temp dir");
        let faillock = dir.join("faillock");
        let missing = std::path::Path::new("/nonexistent/su-tests");

        // What `login` recorded.
        let mut as_login =
            authlib::Authenticator::with_stores(missing, missing).with_faillock(&faillock);
        for _ in 0..FREE_ATTEMPTS_HEADROOM {
            as_login.note_failure("ivan");
        }

        // What `su` sees: a separate process, a fresh in-memory tally, the
        // same file.
        let mut as_su =
            authlib::Authenticator::with_stores(missing, missing).with_faillock(&faillock);
        assert!(
            as_su.rate_limited("ivan").is_some(),
            "su must honour the delay login earned"
        );
        let record = record_with_password("ivan", "hunter2");
        assert!(!authenticate(&mut as_su, &record, "Password: ", "su"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
