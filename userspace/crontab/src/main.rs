//! OurOS `crontab` — per-user cron schedule management
//!
//! User-facing companion to the `crond` daemon. Manages crontab files stored
//! in `/var/spool/cron/<username>`. After any modification the tool signals
//! `crond` to reload by writing to `/run/crond/reload`.
//!
//! # Usage
//!
//! ```text
//! crontab -l              List current user's crontab
//! crontab -e              Edit crontab in $EDITOR (validates before saving)
//! crontab -r              Remove current user's crontab
//! crontab -u <user> -l    List another user's crontab (root only)
//! crontab <file>          Install crontab from file
//! crontab --validate <f>  Validate crontab syntax without installing
//! ```
//!
//! # Crontab Format
//!
//! Five-field time specification plus command:
//!
//! ```text
//! # min  hour  dom  month  dow  command
//!   */5   *     *    *      *   /bin/cleanup --temp
//!   0     3     *    *      *   /bin/backup start
//! ```
//!
//! Special strings: `@reboot`, `@hourly`, `@daily`, `@weekly`, `@monthly`,
//! `@yearly`/`@annually`.
//!
//! Comment lines (starting with `#`), blank lines, and environment variable
//! assignments (`KEY=VALUE`) are preserved verbatim.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Per-user crontab spool directory.
const SPOOL_DIR: &str = "/var/spool/cron";

/// Path where we write to signal `crond` to reload crontab files.
const RELOAD_SIGNAL_PATH: &str = "/run/crond/reload";

/// Fallback editor when `$EDITOR` and `$VISUAL` are both unset.
const DEFAULT_EDITOR: &str = "/bin/vi";

/// Temporary file prefix for edit sessions (placed next to the real crontab).
const TEMP_SUFFIX: &str = ".crontab.tmp";

// ============================================================================
// Error type
// ============================================================================

/// All errors the tool can produce, mapped to user-facing messages.
enum Error {
    /// A problem with crontab syntax (line number, description).
    Syntax(usize, String),
    /// An I/O or permission error.
    Io(String),
    /// A usage error (bad flags, missing arguments).
    Usage(String),
    /// Permission denied (non-root trying `-u`).
    Permission(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Syntax(line, msg) => write!(f, "syntax error on line {line}: {msg}"),
            Error::Io(msg) => write!(f, "{msg}"),
            Error::Usage(msg) => write!(f, "{msg}"),
            Error::Permission(msg) => write!(f, "permission denied: {msg}"),
        }
    }
}

// ============================================================================
// Crontab validation
// ============================================================================

/// Result of validating a single crontab line.
enum LineKind {
    /// Blank line — preserve as-is.
    Blank,
    /// Comment line (starts with `#`) — preserve as-is.
    Comment,
    /// Environment variable assignment (`KEY=VALUE`) — preserve as-is.
    EnvVar,
    /// A valid cron schedule entry.
    CronEntry,
}

/// Validate one line of a crontab file.
///
/// Returns `Ok(LineKind)` if the line is acceptable, or `Err(description)` if
/// the line contains a syntax error.
fn validate_line(line: &str) -> Result<LineKind, String> {
    let trimmed = line.trim();

    // Blank lines are fine.
    if trimmed.is_empty() {
        return Ok(LineKind::Blank);
    }

    // Comment lines.
    if trimmed.starts_with('#') {
        return Ok(LineKind::Comment);
    }

    // Environment variable assignment: KEY=VALUE (key must be [A-Za-z_][A-Za-z0-9_]*).
    if let Some(eq_pos) = trimmed.find('=') {
        let key = &trimmed[..eq_pos];
        if !key.is_empty() && is_valid_env_key(key) && !trimmed.starts_with('@') {
            return Ok(LineKind::EnvVar);
        }
    }

    // Special time strings.
    if trimmed.starts_with('@') {
        return validate_special(trimmed);
    }

    // Standard 5-field entry.
    validate_five_field(trimmed)
}

/// Check whether `key` is a valid environment variable name.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate a special time string line (`@keyword command`).
fn validate_special(line: &str) -> Result<LineKind, String> {
    let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
    let keyword = parts[0];

    match keyword {
        "@reboot" | "@hourly" | "@daily" | "@midnight" | "@weekly" | "@monthly"
        | "@yearly" | "@annually" => {}
        other => return Err(format!("unknown special keyword '{other}'")),
    }

    if parts.len() < 2 || parts[1].trim().is_empty() {
        return Err(format!("{keyword} requires a command"));
    }

    Ok(LineKind::CronEntry)
}

/// Validate a standard 5-field cron entry.
fn validate_five_field(line: &str) -> Result<LineKind, String> {
    let parts: Vec<&str> = line.splitn(6, char::is_whitespace)
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() < 6 {
        return Err("expected 5 time fields followed by a command".into());
    }

    validate_field(parts[0], 0, 59, "minute")?;
    validate_field(parts[1], 0, 23, "hour")?;
    validate_field(parts[2], 1, 31, "day-of-month")?;
    validate_field(parts[3], 1, 12, "month")?;
    validate_field(parts[4], 0, 6, "day-of-week")?;

    // parts[5] is the command — anything non-empty is fine.
    if parts[5].trim().is_empty() {
        return Err("command is empty".into());
    }

    Ok(LineKind::CronEntry)
}

/// Validate a single cron field (e.g. minute, hour).
///
/// Supports: `*`, `N`, `N-M`, `*/N`, `N-M/S`, `N,M,O`.
fn validate_field(field: &str, min: u32, max: u32, name: &str) -> Result<(), String> {
    // Comma-separated list: validate each element.
    if field.contains(',') {
        for part in field.split(',') {
            validate_field_atom(part.trim(), min, max, name)?;
        }
        return Ok(());
    }

    validate_field_atom(field, min, max, name)
}

/// Validate a single atom within a cron field (no commas).
fn validate_field_atom(atom: &str, min: u32, max: u32, name: &str) -> Result<(), String> {
    // Step form: base/N
    if let Some((base_part, step_str)) = atom.split_once('/') {
        let step: u32 = step_str
            .parse()
            .map_err(|_| format!("{name}: invalid step value '{step_str}'"))?;
        if step == 0 {
            return Err(format!("{name}: step value must not be 0"));
        }
        if base_part == "*" {
            return Ok(());
        }
        // Range/N form.
        if let Some((lo_str, hi_str)) = base_part.split_once('-') {
            let lo = parse_bound(lo_str, name)?;
            let hi = parse_bound(hi_str, name)?;
            check_bounds(lo, min, max, name)?;
            check_bounds(hi, min, max, name)?;
            if lo > hi {
                return Err(format!("{name}: range start {lo} > end {hi}"));
            }
            return Ok(());
        }
        let base = parse_bound(base_part, name)?;
        check_bounds(base, min, max, name)?;
        return Ok(());
    }

    // Range: N-M
    if let Some((lo_str, hi_str)) = atom.split_once('-') {
        let lo = parse_bound(lo_str, name)?;
        let hi = parse_bound(hi_str, name)?;
        check_bounds(lo, min, max, name)?;
        check_bounds(hi, min, max, name)?;
        if lo > hi {
            return Err(format!("{name}: range start {lo} > end {hi}"));
        }
        return Ok(());
    }

    // Wildcard.
    if atom == "*" {
        return Ok(());
    }

    // Single value.
    let val = parse_bound(atom, name)?;
    check_bounds(val, min, max, name)
}

/// Parse a numeric value from a cron field token.
fn parse_bound(s: &str, name: &str) -> Result<u32, String> {
    s.parse::<u32>()
        .map_err(|_| format!("{name}: expected a number, got '{s}'"))
}

/// Check that a value falls within the allowed range for this field.
fn check_bounds(val: u32, min: u32, max: u32, name: &str) -> Result<(), String> {
    if val < min || val > max {
        Err(format!("{name}: value {val} out of range {min}-{max}"))
    } else {
        Ok(())
    }
}

// ============================================================================
// Full-file validation
// ============================================================================

/// Validate every line in a crontab file. Returns a list of errors (if any)
/// and the count of actual cron entries found.
fn validate_crontab(content: &str) -> (Vec<Error>, usize) {
    let mut errors = Vec::new();
    let mut entry_count = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;
        match validate_line(line) {
            Ok(LineKind::CronEntry) => entry_count += 1,
            Ok(_) => {} // blank, comment, env var — all fine
            Err(msg) => errors.push(Error::Syntax(line_num, msg)),
        }
    }

    (errors, entry_count)
}

// ============================================================================
// User / UID helpers
// ============================================================================

/// Determine the current username. Checks `$USER`, then `$LOGNAME`, then
/// falls back to the UID.
fn current_username() -> String {
    if let Ok(user) = env::var("USER") {
        if !user.is_empty() {
            return user;
        }
    }
    if let Ok(user) = env::var("LOGNAME") {
        if !user.is_empty() {
            return user;
        }
    }
    // Fallback: use the numeric UID as the "username" so we at least have a
    // unique spool path. On a real OurOS system this would call getuid().
    format!("uid{}", std::process::id())
}

/// Determine the current effective UID. Returns 0 for root.
///
/// Checks `$EUID` first (set by our shell), then assumes non-root.
fn effective_uid() -> u32 {
    if let Ok(val) = env::var("EUID") {
        if let Ok(uid) = val.parse::<u32>() {
            return uid;
        }
    }
    // Conservative default: not root.
    1000
}

/// Build the path to a user's crontab file.
fn crontab_path(username: &str) -> PathBuf {
    PathBuf::from(SPOOL_DIR).join(username)
}

// ============================================================================
// Signal crond to reload
// ============================================================================

/// Write a reload trigger so `crond` picks up changes on the next cycle.
///
/// We write the username to `/run/crond/reload`. If the directory or file
/// doesn't exist yet (crond not running), we silently ignore the error --
/// crond will load the file on its next startup regardless.
fn signal_reload(username: &str) {
    if let Some(parent) = Path::new(RELOAD_SIGNAL_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(RELOAD_SIGNAL_PATH, username.as_bytes());
}

// ============================================================================
// Commands
// ============================================================================

/// `crontab -l` / `crontab -u <user> -l` — list a crontab.
fn cmd_list(username: &str) -> Result<(), Error> {
    let path = crontab_path(username);
    match fs::read_to_string(&path) {
        Ok(content) => {
            // Write directly to stdout, preserving the file content exactly.
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            let _ = handle.write_all(content.as_bytes());
            // Ensure trailing newline for clean terminal output.
            if !content.ends_with('\n') && !content.is_empty() {
                let _ = handle.write_all(b"\n");
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(Error::Io(format!("no crontab for {username}")))
        }
        Err(e) => Err(Error::Io(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

/// `crontab -e` — edit the crontab interactively.
///
/// Opens `$VISUAL` or `$EDITOR` (falling back to `/bin/vi`) on a temporary
/// copy. If the user saves, validates before installing. Loops on error so the
/// user can fix mistakes without losing their edits.
fn cmd_edit(username: &str) -> Result<(), Error> {
    let path = crontab_path(username);
    let tmp_path = PathBuf::from(format!("{}{TEMP_SUFFIX}", path.display()));

    // Ensure spool directory exists.
    ensure_spool_dir()?;

    // Copy current crontab (if any) into the temp file.
    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(Error::Io(format!(
                "cannot read {}: {e}",
                path.display()
            )));
        }
    };
    fs::write(&tmp_path, &existing).map_err(|e| {
        Error::Io(format!("cannot write temp file {}: {e}", tmp_path.display()))
    })?;

    let editor = pick_editor();

    loop {
        // Launch editor.
        let status = process::Command::new(&editor)
            .arg(&tmp_path)
            .status();

        match status {
            Ok(s) if !s.success() => {
                // Editor exited with error — user probably wants to abort.
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(format!(
                    "editor '{}' exited with {}",
                    editor,
                    s.code().map_or("signal".to_string(), |c| c.to_string())
                )));
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(format!("cannot run editor '{editor}': {e}")));
            }
            Ok(_) => {} // success — continue to validation
        }

        // Read back the edited content.
        let new_content = match fs::read_to_string(&tmp_path) {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                return Err(Error::Io(format!(
                    "cannot read temp file after editing: {e}"
                )));
            }
        };

        // If the user didn't change anything, skip the install.
        if new_content == existing {
            let _ = fs::remove_file(&tmp_path);
            eprintln!("crontab: no changes made");
            return Ok(());
        }

        // Validate.
        let (errors, entry_count) = validate_crontab(&new_content);
        if errors.is_empty() {
            // Install.
            fs::write(&path, &new_content).map_err(|e| {
                Error::Io(format!("cannot install crontab: {e}"))
            })?;
            let _ = fs::remove_file(&tmp_path);
            signal_reload(username);
            eprintln!(
                "crontab: installing new crontab ({} entr{})",
                entry_count,
                if entry_count == 1 { "y" } else { "ies" }
            );
            return Ok(());
        }

        // Report errors and let the user re-edit.
        eprintln!("crontab: errors in crontab file, cannot install:");
        for err in &errors {
            eprintln!("  {err}");
        }
        eprint!("Edit again? (y/n) ");
        let _ = io::stderr().flush();

        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("y")
        {
            let _ = fs::remove_file(&tmp_path);
            eprintln!("crontab: edits left in {}", tmp_path.display());
            return Err(Error::Io("crontab not installed due to errors".into()));
        }
        // Loop back to editor.
    }
}

/// `crontab -r` — remove the crontab.
fn cmd_remove(username: &str) -> Result<(), Error> {
    let path = crontab_path(username);
    match fs::remove_file(&path) {
        Ok(()) => {
            signal_reload(username);
            eprintln!("crontab: crontab for {username} removed");
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(Error::Io(format!("no crontab for {username}")))
        }
        Err(e) => Err(Error::Io(format!(
            "cannot remove {}: {e}",
            path.display()
        ))),
    }
}

/// `crontab <file>` — install a crontab from a file (or stdin if `-`).
fn cmd_install(username: &str, source: &str) -> Result<(), Error> {
    let content = if source == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).map_err(|e| {
            Error::Io(format!("cannot read stdin: {e}"))
        })?;
        buf
    } else {
        fs::read_to_string(source).map_err(|e| {
            Error::Io(format!("cannot read '{}': {e}", source))
        })?
    };

    // Validate before installing.
    let (errors, entry_count) = validate_crontab(&content);
    if !errors.is_empty() {
        eprintln!("crontab: errors in input file:");
        for err in &errors {
            eprintln!("  {err}");
        }
        return Err(Error::Io("crontab not installed due to errors".into()));
    }

    ensure_spool_dir()?;

    let path = crontab_path(username);
    fs::write(&path, &content).map_err(|e| {
        Error::Io(format!("cannot install crontab: {e}"))
    })?;

    signal_reload(username);
    eprintln!(
        "crontab: installing new crontab ({} entr{})",
        entry_count,
        if entry_count == 1 { "y" } else { "ies" }
    );
    Ok(())
}

/// `crontab --validate <file>` — check syntax without installing.
fn cmd_validate(source: &str) -> Result<(), Error> {
    let content = if source == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).map_err(|e| {
            Error::Io(format!("cannot read stdin: {e}"))
        })?;
        buf
    } else {
        fs::read_to_string(source).map_err(|e| {
            Error::Io(format!("cannot read '{}': {e}", source))
        })?
    };

    let (errors, entry_count) = validate_crontab(&content);

    if errors.is_empty() {
        let line_count = content.lines().count();
        println!(
            "OK: {line_count} line(s), {entry_count} cron entr{}",
            if entry_count == 1 { "y" } else { "ies" }
        );
        Ok(())
    } else {
        for err in &errors {
            eprintln!("  {err}");
        }
        Err(Error::Io(format!(
            "{} error(s) found",
            errors.len()
        )))
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Ensure the spool directory exists.
fn ensure_spool_dir() -> Result<(), Error> {
    fs::create_dir_all(SPOOL_DIR).map_err(|e| {
        Error::Io(format!("cannot create spool directory {SPOOL_DIR}: {e}"))
    })
}

/// Pick the best available editor, checking `$VISUAL`, `$EDITOR`, then the
/// default fallback.
fn pick_editor() -> String {
    if let Ok(ed) = env::var("VISUAL") {
        if !ed.is_empty() {
            return ed;
        }
    }
    if let Ok(ed) = env::var("EDITOR") {
        if !ed.is_empty() {
            return ed;
        }
    }
    DEFAULT_EDITOR.to_string()
}

fn print_usage() {
    eprintln!("OurOS crontab v0.1.0 — manage per-user cron schedules");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  crontab -l                 List current user's crontab");
    eprintln!("  crontab -e                 Edit crontab ($EDITOR)");
    eprintln!("  crontab -r                 Remove current user's crontab");
    eprintln!("  crontab -u <user> -l       List another user's crontab (root only)");
    eprintln!("  crontab -u <user> -e       Edit another user's crontab (root only)");
    eprintln!("  crontab -u <user> -r       Remove another user's crontab (root only)");
    eprintln!("  crontab <file>             Install crontab from file");
    eprintln!("  crontab -                  Install crontab from stdin");
    eprintln!("  crontab --validate <file>  Validate syntax without installing");
    eprintln!();
    eprintln!("CRONTAB FORMAT:");
    eprintln!("  # Comment lines and blank lines are preserved.");
    eprintln!("  # Environment variables:");
    eprintln!("  SHELL=/bin/sh");
    eprintln!("  PATH=/usr/bin:/bin");
    eprintln!();
    eprintln!("  # min  hour  dom  month  dow  command");
    eprintln!("  */5    *     *    *      *    /bin/cleanup --temp");
    eprintln!("  0      3     *    *      *    /bin/backup start");
    eprintln!("  @daily /bin/report --summary");
    eprintln!();
    eprintln!("SPECIAL STRINGS:");
    eprintln!("  @reboot    Run once at crond startup");
    eprintln!("  @hourly    Equivalent to: 0 * * * *");
    eprintln!("  @daily     Equivalent to: 0 0 * * *");
    eprintln!("  @weekly    Equivalent to: 0 0 * * 0");
    eprintln!("  @monthly   Equivalent to: 0 0 1 * *");
    eprintln!("  @yearly    Equivalent to: 0 0 1 1 *");
    eprintln!();
    eprintln!("FIELD SYNTAX:");
    eprintln!("  *       any value");
    eprintln!("  N       exact value");
    eprintln!("  N-M     range (inclusive)");
    eprintln!("  N,M,O   list of values");
    eprintln!("  */N     every N-th value");
    eprintln!("  N-M/S   range with step S");
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line arguments.
struct Args {
    /// Target username (from -u, or current user).
    username: String,
    /// Whether -u was explicitly provided (requires root).
    explicit_user: bool,
    /// The action to perform.
    action: Action,
}

enum Action {
    List,
    Edit,
    Remove,
    Install(String),
    Validate(String),
    Help,
}

fn parse_args() -> Result<Args, Error> {
    let argv: Vec<String> = env::args().collect();
    let argc = argv.len();

    if argc < 2 {
        return Ok(Args {
            username: current_username(),
            explicit_user: false,
            action: Action::Help,
        });
    }

    let mut username: Option<String> = None;
    let mut action: Option<Action> = None;
    let mut i = 1;

    while i < argc {
        match argv[i].as_str() {
            "-h" | "--help" | "help" => {
                action = Some(Action::Help);
                i += 1;
            }
            "-l" | "--list" => {
                action = Some(Action::List);
                i += 1;
            }
            "-e" | "--edit" => {
                action = Some(Action::Edit);
                i += 1;
            }
            "-r" | "--remove" => {
                action = Some(Action::Remove);
                i += 1;
            }
            "-u" | "--user" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("-u requires a username".into()));
                }
                username = Some(argv[i + 1].clone());
                i += 2;
            }
            "--validate" => {
                if i + 1 >= argc {
                    return Err(Error::Usage("--validate requires a file path".into()));
                }
                action = Some(Action::Validate(argv[i + 1].clone()));
                i += 2;
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                // Handle combined short flags like -lu or multi-char unknowns.
                return Err(Error::Usage(format!("unknown option: {arg}")));
            }
            _ => {
                // Positional argument: treat as a file to install from.
                action = Some(Action::Install(argv[i].clone()));
                i += 1;
            }
        }
    }

    let explicit_user = username.is_some();
    let username = username.unwrap_or_else(current_username);
    let action = action.unwrap_or(Action::Help);

    Ok(Args {
        username,
        explicit_user,
        action,
    })
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), Error> {
    let args = parse_args()?;

    // If -u was specified, verify we are root.
    if args.explicit_user && effective_uid() != 0 {
        return Err(Error::Permission(
            "only root can use -u to manage another user's crontab".into(),
        ));
    }

    match args.action {
        Action::Help => {
            print_usage();
            Ok(())
        }
        Action::List => cmd_list(&args.username),
        Action::Edit => cmd_edit(&args.username),
        Action::Remove => cmd_remove(&args.username),
        Action::Install(ref source) => cmd_install(&args.username, source),
        Action::Validate(ref source) => cmd_validate(source),
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("crontab: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- validate_line tests --

    #[test]
    fn blank_lines_accepted() {
        assert!(matches!(validate_line(""), Ok(LineKind::Blank)));
        assert!(matches!(validate_line("   "), Ok(LineKind::Blank)));
    }

    #[test]
    fn comment_lines_accepted() {
        assert!(matches!(validate_line("# this is a comment"), Ok(LineKind::Comment)));
        assert!(matches!(validate_line("  # indented comment"), Ok(LineKind::Comment)));
    }

    #[test]
    fn env_var_lines_accepted() {
        assert!(matches!(validate_line("SHELL=/bin/sh"), Ok(LineKind::EnvVar)));
        assert!(matches!(validate_line("PATH=/usr/bin:/bin"), Ok(LineKind::EnvVar)));
        assert!(matches!(validate_line("MAILTO=admin@example.com"), Ok(LineKind::EnvVar)));
        assert!(matches!(validate_line("_FOO=bar"), Ok(LineKind::EnvVar)));
    }

    #[test]
    fn env_var_key_must_be_valid() {
        // A line like "123=abc" has a digit-starting key, so it won't match as
        // an env var and will be parsed as a cron entry (which will fail).
        assert!(validate_line("123=abc").is_err());
    }

    #[test]
    fn special_strings_valid() {
        assert!(matches!(validate_line("@reboot /bin/foo"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@hourly /bin/bar"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@daily /bin/baz"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@weekly /bin/qux"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@monthly /bin/quux"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@yearly /bin/corge"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@annually /bin/grault"), Ok(LineKind::CronEntry)));
        assert!(matches!(validate_line("@midnight /bin/garply"), Ok(LineKind::CronEntry)));
    }

    #[test]
    fn special_string_requires_command() {
        assert!(validate_line("@reboot").is_err());
        assert!(validate_line("@daily   ").is_err());
    }

    #[test]
    fn unknown_special_rejected() {
        assert!(validate_line("@never /bin/foo").is_err());
    }

    #[test]
    fn five_field_valid() {
        assert!(validate_line("* * * * * /bin/true").is_ok());
        assert!(validate_line("0 0 1 1 0 /bin/happy-new-year").is_ok());
        assert!(validate_line("*/5 * * * * /bin/cleanup").is_ok());
        assert!(validate_line("0 3 * * 1-5 /bin/weekday-backup").is_ok());
        assert!(validate_line("0,30 * * * * /bin/halfhour").is_ok());
        assert!(validate_line("0-10/2 * * * * /bin/even-minutes").is_ok());
    }

    #[test]
    fn five_field_too_few_fields() {
        assert!(validate_line("* * * * /bin/missing-field").is_err());
        assert!(validate_line("* * *").is_err());
    }

    #[test]
    fn field_out_of_range() {
        // minute 60 is out of range (0-59).
        assert!(validate_line("60 * * * * /bin/bad").is_err());
        // hour 24 is out of range (0-23).
        assert!(validate_line("0 24 * * * /bin/bad").is_err());
        // dom 0 is out of range (1-31).
        assert!(validate_line("0 0 0 * * /bin/bad").is_err());
        // dom 32 is out of range.
        assert!(validate_line("0 0 32 * * /bin/bad").is_err());
        // month 0 is out of range (1-12).
        assert!(validate_line("0 0 1 0 * /bin/bad").is_err());
        // month 13 is out of range.
        assert!(validate_line("0 0 1 13 * /bin/bad").is_err());
        // dow 7 is out of range (0-6).
        assert!(validate_line("0 0 * * 7 /bin/bad").is_err());
    }

    #[test]
    fn step_zero_rejected() {
        assert!(validate_line("*/0 * * * * /bin/bad").is_err());
    }

    #[test]
    fn range_inverted_rejected() {
        assert!(validate_line("30-10 * * * * /bin/bad").is_err());
    }

    #[test]
    fn non_numeric_value_rejected() {
        assert!(validate_line("abc * * * * /bin/bad").is_err());
    }

    // -- validate_field tests --

    #[test]
    fn field_wildcard() {
        assert!(validate_field("*", 0, 59, "minute").is_ok());
    }

    #[test]
    fn field_single_value() {
        assert!(validate_field("0", 0, 59, "minute").is_ok());
        assert!(validate_field("59", 0, 59, "minute").is_ok());
    }

    #[test]
    fn field_range() {
        assert!(validate_field("1-5", 0, 59, "minute").is_ok());
        assert!(validate_field("10-20", 0, 23, "hour").is_ok());
    }

    #[test]
    fn field_step() {
        assert!(validate_field("*/15", 0, 59, "minute").is_ok());
        assert!(validate_field("1-30/5", 0, 59, "minute").is_ok());
        assert!(validate_field("5/10", 0, 59, "minute").is_ok());
    }

    #[test]
    fn field_list() {
        assert!(validate_field("1,15,30,45", 0, 59, "minute").is_ok());
        assert!(validate_field("0,6", 0, 6, "day-of-week").is_ok());
    }

    // -- validate_crontab (whole-file) tests --

    #[test]
    fn full_crontab_valid() {
        let content = "\
# Backup schedule
SHELL=/bin/sh
PATH=/usr/bin:/bin

*/5 * * * * /bin/cleanup --temp
0 3 * * * /bin/backup start /home
@daily /bin/report --summary
@reboot /bin/indexer daemon
";
        let (errors, count) = validate_crontab(content);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors.iter().map(|e| e.to_string()).collect::<Vec<_>>());
        assert_eq!(count, 4);
    }

    #[test]
    fn full_crontab_with_errors() {
        let content = "\
# Good comment
0 3 * * * /bin/backup
60 * * * * /bin/bad-minute
@bogus /bin/unknown-keyword
";
        let (errors, count) = validate_crontab(content);
        assert_eq!(count, 1); // only the valid line
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn empty_crontab_is_valid() {
        let (errors, count) = validate_crontab("");
        assert!(errors.is_empty());
        assert_eq!(count, 0);
    }

    // -- is_valid_env_key tests --

    #[test]
    fn env_key_validation() {
        assert!(is_valid_env_key("SHELL"));
        assert!(is_valid_env_key("_PRIVATE"));
        assert!(is_valid_env_key("PATH2"));
        assert!(!is_valid_env_key("2BAD"));
        assert!(!is_valid_env_key(""));
        assert!(!is_valid_env_key("foo-bar"));
    }

    // -- crontab_path tests --

    #[test]
    fn crontab_path_construction() {
        let path = crontab_path("alice");
        assert_eq!(path, PathBuf::from("/var/spool/cron/alice"));
    }
}
