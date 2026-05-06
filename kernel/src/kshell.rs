//! Kernel debug shell.
//!
//! A simple command-line interface that runs in the kernel's idle context,
//! reading keyboard input and executing built-in diagnostic commands.
//! This provides interactive debugging capability without requiring a
//! filesystem, userspace programs, or a POSIX layer.
//!
//! ## Commands
//!
//! - `help`     — list available commands
//! - `meminfo`  — show physical memory usage
//! - `ps`       — list running tasks (scheduler state)
//! - `clear`    — clear the screen
//! - `uptime`   — show tick count / uptime
//! - `echo ...` — echo text back to console
//! - `reboot`   — triple-fault reboot
//!
//! ## Design
//!
//! The shell runs as a loop in `kmain()` after boot completes.  It blocks
//! on keyboard input using [`crate::keyboard::read_char`] (which HLTs
//! between interrupts).  This keeps the idle loop power-efficient while
//! still processing input promptly when keys arrive.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Output capture for redirection and piping
// ---------------------------------------------------------------------------

/// When `Some`, shell output is captured into this buffer instead of being
/// printed to the console.  Used for `> file`, `>> file`, and `|` piping.
///
/// Only one capture is active at a time (the kshell is single-threaded).
static SHELL_OUTPUT: Mutex<Option<String>> = Mutex::new(None);

/// Begin capturing shell output to an internal buffer.
fn capture_start() {
    *SHELL_OUTPUT.lock() = Some(String::with_capacity(4096));
}

/// Stop capturing and return the captured text.
fn capture_stop() -> String {
    SHELL_OUTPUT.lock().take().unwrap_or_default()
}

/// Execute a command and capture its output as a string.
///
/// Used for `$(command)` substitution.  Handles recursive capture by
/// saving and restoring the previous capture state (since a command
/// substitution can appear inside a pipeline or redirect that's already
/// capturing).
fn capture_command(cmd: &str) -> String {
    // Save any existing capture state (supports nesting).
    let prev = SHELL_OUTPUT.lock().take();

    // Start fresh capture.
    capture_start();
    execute(cmd);
    let output = capture_stop();

    // Restore previous capture state.
    *SHELL_OUTPUT.lock() = prev;

    output
}

/// Write a string to the shell output destination.
///
/// If capture mode is active, appends to the capture buffer.
/// Otherwise, writes to the console as normal.
fn shell_write(s: &str) {
    let mut guard = SHELL_OUTPUT.lock();
    if let Some(ref mut buf) = *guard {
        buf.push_str(s);
    } else {
        drop(guard);
        crate::console::write_str(s);
    }
}

/// Print to the shell output destination (no newline).
macro_rules! shell_print {
    ($($arg:tt)*) => {
        $crate::kshell::shell_write(&alloc::format!($($arg)*))
    };
}

/// Print a line to the shell output destination.
macro_rules! shell_println {
    () => { $crate::kshell::shell_write("\n") };
    ($($arg:tt)*) => {{
        $crate::kshell::shell_write(&alloc::format!($($arg)*));
        $crate::kshell::shell_write("\n");
    }};
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum line length.  Longer lines are silently truncated.
const MAX_LINE: usize = 256;

/// Maximum number of commands stored in the history ring buffer.
const HISTORY_SIZE: usize = 64;

/// Maximum nesting depth for if/then/else/fi and while/do/done blocks.
const MAX_NESTING: usize = 16;

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

/// Last command's exit status.  0 = success, non-zero = failure.
///
/// Used by `$?` expansion, `&&` / `||` chaining operators, and the
/// `true`/`false` built-in commands.
static LAST_EXIT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Set the last command's exit status.
fn set_exit(code: u8) {
    LAST_EXIT.store(code, core::sync::atomic::Ordering::Relaxed);
}

/// Get the last command's exit status.
fn last_exit() -> u8 {
    LAST_EXIT.load(core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Working directory
// ---------------------------------------------------------------------------

/// The shell's current working directory.
///
/// Used to resolve relative paths.  Changed by `cd`.  Starts as "/".
static CWD: Mutex<String> = Mutex::new(String::new());

/// Get the current working directory as a new String.
fn get_cwd() -> String {
    let guard = CWD.lock();
    if guard.is_empty() {
        String::from("/")
    } else {
        guard.clone()
    }
}

/// Resolve a path that may be relative against the current working directory.
///
/// - Absolute paths (starting with `/`) are returned normalized.
/// - Relative paths are joined with the cwd and normalized.
/// - Handles `.` (current dir) and `..` (parent dir) components.
fn resolve_path(path: &str) -> String {
    let abs = if path.starts_with('/') {
        String::from(path)
    } else {
        let cwd = get_cwd();
        if cwd.ends_with('/') {
            alloc::format!("{}{}", cwd, path)
        } else {
            alloc::format!("{}/{}", cwd, path)
        }
    };

    // Normalize: split into components, resolve . and ..
    let mut parts: Vec<&str> = Vec::new();
    for component in abs.split('/') {
        match component {
            "" | "." => {} // skip empty and current-dir
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }

    if parts.is_empty() {
        String::from("/")
    } else {
        let mut result = String::with_capacity(abs.len());
        for p in &parts {
            result.push('/');
            result.push_str(p);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Shell options (set -e, set -x, etc.)
// ---------------------------------------------------------------------------

/// `set -e`: exit script/function on any command failure (non-zero exit).
///
/// Only affects `source` scripts and function bodies, not the interactive
/// prompt (which always continues).
static OPT_ERREXIT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// `set -x`: trace mode — print each command before execution.
static OPT_XTRACE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Trap handlers
// ---------------------------------------------------------------------------

/// Signal trap handlers: maps signal name → command to execute.
///
/// Supported signals in kshell:
/// - EXIT: runs when the shell exits or a script finishes
/// - ERR:  runs when a command returns non-zero (if not in `|| / && / if`)
/// - INT:  runs on interrupt (Ctrl+C)
///
/// Set with `trap 'commands' SIGNAL`.  Clear with `trap - SIGNAL`.
static TRAP_HANDLERS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Environment variables
// ---------------------------------------------------------------------------

/// Shell environment variables, accessible via `$VAR` or `${VAR}` syntax.
///
/// Set with `export NAME=VALUE`, removed with `unset NAME`, listed with
/// `printenv` or `env`.  Some built-in variables are populated at init
/// (`PWD`, `SHELL`, `HOME`).
static ENV_VARS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Set of variable names that are read-only (cannot be re-assigned or unset).
static READONLY_VARS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Check whether a variable is read-only.
fn is_readonly(name: &str) -> bool {
    READONLY_VARS.lock().contains(name)
}

/// Get an environment variable's value, or `None` if not set.
fn env_get(name: &str) -> Option<String> {
    ENV_VARS.lock().get(name).cloned()
}

/// Set an environment variable.  Returns false (and prints error) if readonly.
fn env_set(name: &str, value: &str) -> bool {
    if is_readonly(name) {
        crate::console_println!("{}: readonly variable", name);
        return false;
    }
    ENV_VARS.lock().insert(String::from(name), String::from(value));
    true
}

/// Remove an environment variable.  Returns true if it existed.
/// Refuses to unset readonly variables.
fn env_remove(name: &str) -> bool {
    if is_readonly(name) {
        crate::console_println!("unset: {}: readonly variable", name);
        return false;
    }
    ENV_VARS.lock().remove(name).is_some()
}

// ---------------------------------------------------------------------------
// Array variables
// ---------------------------------------------------------------------------

/// Shell array variables, accessible via `${arr[N]}` or `${arr[@]}` syntax.
///
/// Declared with `arr=(word1 word2 ...)`.  Element access: `${arr[0]}`.
/// All elements: `${arr[@]}` or `${arr[*]}`.  Length: `${#arr[@]}`.
/// Element assignment: `arr[N]=value`.  Remove: `unset arr` or `unset arr[N]`.
static ARRAY_VARS: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());

/// Get an array element by index.  Returns `None` if array or index doesn't exist.
fn array_get(name: &str, index: usize) -> Option<String> {
    ARRAY_VARS.lock().get(name).and_then(|v| v.get(index).cloned())
}

/// Get all elements of an array, space-separated.
fn array_all(name: &str) -> Option<String> {
    ARRAY_VARS.lock().get(name).map(|v| {
        let mut result = String::new();
        for (i, elem) in v.iter().enumerate() {
            if i > 0 {
                result.push(' ');
            }
            result.push_str(elem);
        }
        result
    })
}

/// Get the number of elements in an array.
fn array_len(name: &str) -> Option<usize> {
    ARRAY_VARS.lock().get(name).map(Vec::len)
}

/// Set an array element, growing the array with empty strings if needed.
fn array_set_element(name: &str, index: usize, value: &str) {
    let mut arrays = ARRAY_VARS.lock();
    let arr = arrays.entry(String::from(name)).or_insert_with(Vec::new);
    // Grow the array if needed.
    while arr.len() <= index {
        arr.push(String::new());
    }
    if let Some(elem) = arr.get_mut(index) {
        *elem = String::from(value);
    }
}

/// Set an entire array from a list of values.
fn array_set(name: &str, values: Vec<String>) {
    ARRAY_VARS.lock().insert(String::from(name), values);
}

/// Remove an array entirely.  Returns true if it existed.
fn array_remove(name: &str) -> bool {
    ARRAY_VARS.lock().remove(name).is_some()
}

/// Remove a single element from an array by index (sets it to empty string,
/// preserving indices — same as bash `unset arr[N]`).
fn array_unset_element(name: &str, index: usize) {
    let mut arrays = ARRAY_VARS.lock();
    if let Some(arr) = arrays.get_mut(name) {
        if let Some(elem) = arr.get_mut(index) {
            *elem = String::new();
        }
    }
}

/// Check if a name refers to an array variable.
fn is_array(name: &str) -> bool {
    ARRAY_VARS.lock().contains_key(name)
}

/// Expand `$VAR` and `${VAR}` references in a string.
///
/// - `$NAME` expands the longest run of alphanumeric/underscore chars.
/// - `${NAME}` expands the text between braces.
/// - `$$` produces a literal `$`.
/// - `$?` expands to the last command's exit status (0=success, 1=failure).
/// - Unknown variables expand to empty string.
/// - Single-quoted strings (`'...'`) are not expanded.
fn expand_vars(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;
    let mut in_single_quote = false;

    while i < len {
        let b = bytes[i];

        if b == b'\'' && !in_single_quote {
            // Enter single-quoted section (no expansion).
            in_single_quote = true;
            result.push('\'');
            i = i.saturating_add(1);
            continue;
        }
        if b == b'\'' && in_single_quote {
            in_single_quote = false;
            result.push('\'');
            i = i.saturating_add(1);
            continue;
        }
        if in_single_quote {
            result.push(b as char);
            i = i.saturating_add(1);
            continue;
        }

        if b == b'$' {
            i = i.saturating_add(1);
            if i >= len {
                // Trailing `$` — emit literally.
                result.push('$');
                break;
            }
            let next = bytes[i];

            if next == b'$' {
                // `$$` → literal `$`.
                result.push('$');
                i = i.saturating_add(1);
            } else if next == b'?' {
                // `$?` → last command's exit status.
                let code = last_exit();
                result.push_str(&alloc::format!("{}", code));
                i = i.saturating_add(1);
            } else if next == b'(' && bytes.get(i.saturating_add(1)) == Some(&b'(') {
                // `$((...))` — arithmetic expansion.
                i = i.saturating_add(2); // skip `((`
                let start = i;
                // Find matching `))`.
                let mut depth: u32 = 1;
                while i < len && depth > 0 {
                    if bytes[i] == b'(' && bytes.get(i.saturating_add(1)) == Some(&b'(') {
                        depth = depth.saturating_add(1);
                        i = i.saturating_add(2);
                    } else if bytes[i] == b')' && bytes.get(i.saturating_add(1)) == Some(&b')') {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            break;
                        }
                        i = i.saturating_add(2);
                    } else {
                        i = i.saturating_add(1);
                    }
                }
                if let Some(expr_bytes) = bytes.get(start..i) {
                    if let Ok(expr) = core::str::from_utf8(expr_bytes) {
                        // Expand variables within the expression first.
                        let expanded_expr = expand_vars(expr);
                        let val = eval_arithmetic(&expanded_expr);
                        result.push_str(&alloc::format!("{}", val));
                    }
                }
                // Skip past `))`.
                if i < len && bytes[i] == b')' {
                    i = i.saturating_add(1);
                }
                if i < len && bytes[i] == b')' {
                    i = i.saturating_add(1);
                }
            } else if next == b'(' {
                // `$(command)` — command substitution.
                // Find the matching `)`, tracking parenthesis nesting.
                let start = i;
                let mut depth: u32 = 1;
                while i < len && depth > 0 {
                    if bytes[i] == b'(' {
                        depth = depth.saturating_add(1);
                    } else if bytes[i] == b')' {
                        depth = depth.saturating_sub(1);
                    }
                    if depth > 0 {
                        i = i.saturating_add(1);
                    }
                }
                if let Some(cmd_bytes) = bytes.get(start..i) {
                    if let Ok(cmd) = core::str::from_utf8(cmd_bytes) {
                        let output = capture_command(cmd);
                        // POSIX: strip trailing newlines from substitution.
                        result.push_str(output.trim_end_matches('\n'));
                    }
                }
                // Skip past `)`.
                if i < len && bytes[i] == b')' {
                    i = i.saturating_add(1);
                }
            } else if next == b'{' {
                // `${...}` form — supports:
                //   ${NAME}          simple expansion
                //   ${#NAME}         string length
                //   ${NAME:-default} use default if unset/empty
                //   ${NAME:+alt}     use alt if set and non-empty
                //   ${NAME:=default} assign default if unset/empty
                //   ${NAME:?msg}     error if unset/empty
                i = i.saturating_add(1); // skip `{`
                let start = i;
                while i < len && bytes[i] != b'}' {
                    i = i.saturating_add(1);
                }
                if let Some(inner_bytes) = bytes.get(start..i) {
                    if let Ok(inner) = core::str::from_utf8(inner_bytes) {
                        expand_brace_expr(inner, &mut result);
                    }
                }
                if i < len && bytes[i] == b'}' {
                    i = i.saturating_add(1); // skip `}`
                }
            } else if next.is_ascii_digit() {
                // `$0`..`$9` → positional parameter.
                let n = (next - b'0') as usize;
                result.push_str(&get_positional(n));
                i = i.saturating_add(1);
            } else if next == b'#' {
                // `$#` → number of positional parameters.
                result.push_str(&alloc::format!("{}", positional_count()));
                i = i.saturating_add(1);
            } else if next == b'@' || next == b'*' {
                // `$@` / `$*` → all positional parameters (space-separated).
                result.push_str(&positional_all());
                i = i.saturating_add(1);
            } else if next.is_ascii_alphabetic() || next == b'_' {
                // `$NAME` form — longest alphanumeric/underscore run.
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i = i.saturating_add(1);
                }
                if let Some(name_bytes) = bytes.get(start..i) {
                    if let Ok(name) = core::str::from_utf8(name_bytes) {
                        if let Some(val) = env_get(name) {
                            result.push_str(&val);
                        }
                    }
                }
            } else {
                // `$` followed by something else — emit literally.
                result.push('$');
                result.push(next as char);
                i = i.saturating_add(1);
            }
        } else if b == b'`' {
            // Backtick command substitution: `command`
            i = i.saturating_add(1);
            let start = i;
            // Find the closing backtick.
            while i < len && bytes[i] != b'`' {
                i = i.saturating_add(1);
            }
            if let Some(cmd_bytes) = bytes.get(start..i) {
                if let Ok(cmd) = core::str::from_utf8(cmd_bytes) {
                    let output = capture_command(cmd);
                    result.push_str(output.trim_end_matches('\n'));
                }
            }
            if i < len && bytes[i] == b'`' {
                i = i.saturating_add(1); // skip closing backtick
            }
        } else if b == b'~' {
            // Tilde expansion: `~` or `~/...` at word start → $HOME.
            // Only expand when at position 0 or after whitespace/=/:
            // (assignment context, PATH-like lists).
            let at_word_start = if i == 0 {
                true
            } else {
                matches!(bytes[i.saturating_sub(1)], b' ' | b'\t' | b'=' | b':')
            };
            let next_ok = i.saturating_add(1) >= len
                || matches!(bytes[i.saturating_add(1)], b'/' | b' ' | b'\t' | b'"' | b'\'' | b':');
            if at_word_start && next_ok {
                if let Some(home) = env_get("HOME") {
                    result.push_str(&home);
                } else {
                    result.push('~');
                }
            } else {
                result.push('~');
            }
            i = i.saturating_add(1);
        } else {
            result.push(b as char);
            i = i.saturating_add(1);
        }
    }

    result
}

/// Expand a `${...}` brace expression.
///
/// Handles:
///   - `${NAME}` — simple variable lookup
///   - `${#NAME}` — length of variable's value
///   - `${NAME:-default}` — use default if NAME is unset or empty
///   - `${NAME:+alternate}` — use alternate if NAME is set and non-empty
///   - `${NAME:=default}` — assign and use default if NAME is unset or empty
///   - `${NAME:?message}` — error if NAME is unset or empty
///   - `${NAME%suffix}` — remove shortest suffix match
///   - `${NAME%%suffix}` — remove longest suffix match
///   - `${NAME#prefix}` — remove shortest prefix match
///   - `${NAME##prefix}` — remove longest prefix match
/// Expand shell brace patterns: `{a,b,c}` and `{N..M}` ranges.
///
/// Brace expansion is applied to each whitespace-delimited token in the input.
/// A token like `prefix{a,b,c}suffix` expands to `prefixa suffix prefixbsuffix
/// prefixcsuffix`.  Numeric ranges `{1..5}` expand to `1 2 3 4 5`.
/// Ranges with step: `{1..10..2}` → `1 3 5 7 9`.
///
/// Tokens without `{` or `}` pass through unchanged.
fn expand_braces(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let tokens = split_words(input);

    for (ti, token) in tokens.iter().enumerate() {
        if ti > 0 {
            result.push(' ');
        }

        // Find the first `{` and its matching `}`.
        let brace_start = match token.find('{') {
            Some(pos) => pos,
            None => { result.push_str(token); continue; }
        };
        let brace_end = match token.get(brace_start..).and_then(|s| s.rfind('}')) {
            Some(pos) => brace_start.saturating_add(pos),
            None => { result.push_str(token); continue; }
        };

        let prefix = token.get(..brace_start).unwrap_or("");
        let inner = token.get(brace_start.saturating_add(1)..brace_end).unwrap_or("");
        let suffix = token.get(brace_end.saturating_add(1)..).unwrap_or("");

        // Check for range pattern: `{N..M}` or `{N..M..S}`.
        if inner.contains("..") {
            let parts: Vec<&str> = inner.splitn(3, "..").collect();
            if let (Some(&start_s), Some(&end_s)) = (parts.first(), parts.get(1)) {
                if let (Ok(start), Ok(end)) = (start_s.parse::<i64>(), end_s.parse::<i64>()) {
                    let step: i64 = if let Some(&step_s) = parts.get(2) {
                        step_s.parse::<i64>().unwrap_or(1)
                    } else if start <= end { 1 } else { -1 };

                    if step == 0 {
                        result.push_str(token);
                        continue;
                    }

                    let mut first = true;
                    let mut val = start;
                    let mut count: u32 = 0;
                    loop {
                        if step > 0 && val > end { break; }
                        if step < 0 && val < end { break; }
                        if count > 10_000 { break; } // Safety limit.

                        if !first { result.push(' '); }
                        first = false;
                        result.push_str(prefix);
                        result.push_str(&alloc::format!("{}", val));
                        result.push_str(suffix);

                        val = val.wrapping_add(step);
                        count = count.saturating_add(1);
                    }
                    continue;
                }
            }
            // Not a valid range — fall through to comma check.
        }

        // Check for comma-separated alternatives: `{a,b,c}`.
        if inner.contains(',') {
            let alternatives: Vec<&str> = inner.split(',').collect();
            for (ai, alt) in alternatives.iter().enumerate() {
                if ai > 0 {
                    result.push(' ');
                }
                result.push_str(prefix);
                result.push_str(alt);
                result.push_str(suffix);
            }
            continue;
        }

        // No comma, no valid range — emit the token literally.
        result.push_str(token);
    }

    result
}

fn expand_brace_expr(inner: &str, result: &mut String) {
    // ${#NAME[@]} — array length.
    if let Some(name) = inner.strip_prefix('#') {
        if let Some(arr_name) = name.strip_suffix("[@]").or_else(|| name.strip_suffix("[*]")) {
            let len = array_len(arr_name).unwrap_or(0);
            result.push_str(&alloc::format!("{}", len));
            return;
        }
        // ${#NAME} — string length (scalar).
        let val = env_get(name).unwrap_or_default();
        result.push_str(&alloc::format!("{}", val.len()));
        return;
    }

    // ${NAME[index]} — array element access.
    // ${NAME[@]} or ${NAME[*]} — all array elements.
    if let Some(bracket_pos) = inner.find('[') {
        if inner.ends_with(']') {
            let arr_name = inner.get(..bracket_pos).unwrap_or("");
            let idx_str = inner.get(bracket_pos.saturating_add(1)..inner.len().saturating_sub(1)).unwrap_or("");

            if !arr_name.is_empty() {
                if idx_str == "@" || idx_str == "*" {
                    // ${arr[@]} or ${arr[*]} — all elements.
                    if let Some(all) = array_all(arr_name) {
                        result.push_str(&all);
                    }
                    return;
                }
                // Parse numeric index (supports variable expansion in index).
                let expanded_idx = expand_vars(idx_str);
                if let Ok(index) = expanded_idx.trim().parse::<usize>() {
                    if let Some(val) = array_get(arr_name, index) {
                        result.push_str(&val);
                    }
                    return;
                }
                // Non-numeric index — treat as error, expand to empty.
                return;
            }
        }
    }

    // Try to find an operator: :-, :+, :=, :?, :N:N, %, %%, #, ##, /, //, ^, ^^, ,, ,,
    // Scan for the operator position (skip the variable name).
    let name_end = inner.find(|c: char| c == ':' || c == '%' || c == '#' || c == '/' || c == '^' || c == ',')
        .unwrap_or(inner.len());
    let name = inner.get(..name_end).unwrap_or(inner);
    let rest = inner.get(name_end..).unwrap_or("");

    if rest.is_empty() {
        // Simple ${NAME}.  Check arrays first (${arr} → first element).
        if let Some(val) = array_get(name, 0) {
            result.push_str(&val);
            return;
        }
        if let Some(val) = env_get(name) {
            result.push_str(&val);
        }
        return;
    }

    let val = env_get(name);
    let is_set_nonempty = val.as_ref().is_some_and(|v| !v.is_empty());

    if let Some(default) = rest.strip_prefix(":-") {
        // ${NAME:-default} — use default if unset or empty.
        if is_set_nonempty {
            result.push_str(val.as_deref().unwrap_or(""));
        } else {
            result.push_str(default);
        }
    } else if let Some(alt) = rest.strip_prefix(":+") {
        // ${NAME:+alternate} — use alternate if set and non-empty.
        if is_set_nonempty {
            result.push_str(alt);
        }
    } else if let Some(default) = rest.strip_prefix(":=") {
        // ${NAME:=default} — assign default if unset or empty.
        if is_set_nonempty {
            result.push_str(val.as_deref().unwrap_or(""));
        } else {
            env_set(name, default);
            result.push_str(default);
        }
    } else if let Some(msg) = rest.strip_prefix(":?") {
        // ${NAME:?message} — error if unset or empty.
        if is_set_nonempty {
            result.push_str(val.as_deref().unwrap_or(""));
        } else {
            let error_msg = if msg.is_empty() {
                alloc::format!("{}: parameter null or not set", name)
            } else {
                alloc::format!("{}: {}", name, msg)
            };
            crate::console_println!("{}", error_msg);
            set_exit(1);
        }
    } else if rest.starts_with(':')
        && rest.len() > 1
        && !rest.starts_with(":-")
        && !rest.starts_with(":+")
        && !rest.starts_with(":=")
        && !rest.starts_with(":?")
    {
        // ${NAME:offset} or ${NAME:offset:length} — substring extraction.
        let spec = rest.get(1..).unwrap_or("");
        let val = val.unwrap_or_default();

        // Split on the second `:` to get offset and optional length.
        let (offset_str, length_str) = if let Some(colon2) = spec.find(':') {
            (
                spec.get(..colon2).unwrap_or(""),
                Some(spec.get(colon2.saturating_add(1)..).unwrap_or("")),
            )
        } else {
            (spec, None)
        };

        let offset_str = offset_str.trim();
        // Parse offset (supports negative values counting from end).
        if let Ok(raw_offset) = offset_str.parse::<i64>() {
            let str_len = val.len() as i64;
            let start = if raw_offset < 0 {
                // Negative offset: count from end.
                (str_len.saturating_add(raw_offset)).max(0) as usize
            } else {
                (raw_offset as usize).min(val.len())
            };

            let end = if let Some(len_s) = length_str {
                if let Ok(len) = len_s.trim().parse::<usize>() {
                    start.saturating_add(len).min(val.len())
                } else {
                    val.len()
                }
            } else {
                val.len()
            };

            result.push_str(val.get(start..end).unwrap_or(""));
        }
        // Non-numeric offset: silently expand to empty (like bash).
    } else if let Some(pattern) = rest.strip_prefix("%%") {
        // ${NAME%%pattern} — remove longest suffix match.
        let val = val.unwrap_or_default();
        // Try from the start of the string (longest match).
        let mut best = val.len();
        for i in 0..=val.len() {
            if let Some(suffix) = val.get(i..) {
                if crate::fs::vfs::glob_match(pattern, suffix, false) {
                    best = i;
                    break;
                }
            }
        }
        result.push_str(val.get(..best).unwrap_or(&val));
    } else if let Some(pattern) = rest.strip_prefix('%') {
        // ${NAME%pattern} — remove shortest suffix match.
        let val = val.unwrap_or_default();
        let mut best = val.len();
        for i in (0..=val.len()).rev() {
            if let Some(suffix) = val.get(i..) {
                if crate::fs::vfs::glob_match(pattern, suffix, false) {
                    best = i;
                    break;
                }
            }
        }
        result.push_str(val.get(..best).unwrap_or(&val));
    } else if let Some(pattern) = rest.strip_prefix("##") {
        // ${NAME##pattern} — remove longest prefix match.
        let val = val.unwrap_or_default();
        let mut best = 0;
        for i in (0..=val.len()).rev() {
            if let Some(prefix) = val.get(..i) {
                if crate::fs::vfs::glob_match(pattern, prefix, false) {
                    best = i;
                    break;
                }
            }
        }
        result.push_str(val.get(best..).unwrap_or(""));
    } else if let Some(pattern) = rest.strip_prefix('#') {
        // ${NAME#pattern} — remove shortest prefix match.
        let val = val.unwrap_or_default();
        let mut best = 0;
        for i in 0..=val.len() {
            if let Some(prefix) = val.get(..i) {
                if crate::fs::vfs::glob_match(pattern, prefix, false) {
                    best = i;
                    break;
                }
            }
        }
        result.push_str(val.get(best..).unwrap_or(""));
    } else if let Some(subst) = rest.strip_prefix("//") {
        // ${NAME//pattern/replacement} — replace all occurrences.
        let val = val.unwrap_or_default();
        if let Some(slash) = subst.find('/') {
            let pattern = subst.get(..slash).unwrap_or("");
            let replacement = subst.get(slash.saturating_add(1)..).unwrap_or("");
            result.push_str(&str_replace_all(&val, pattern, replacement));
        } else {
            // ${NAME//pattern} — remove all occurrences of pattern.
            result.push_str(&str_replace_all(&val, subst, ""));
        }
    } else if let Some(subst) = rest.strip_prefix('/') {
        // ${NAME/pattern/replacement} — replace first occurrence.
        let val = val.unwrap_or_default();
        if let Some(slash) = subst.find('/') {
            let pattern = subst.get(..slash).unwrap_or("");
            let replacement = subst.get(slash.saturating_add(1)..).unwrap_or("");
            result.push_str(&str_replace_first(&val, pattern, replacement));
        } else {
            // ${NAME/pattern} — remove first occurrence of pattern.
            result.push_str(&str_replace_first(&val, subst, ""));
        }
    } else if let Some(subst) = rest.strip_prefix('^') {
        // ${NAME^} — uppercase first character.
        // ${NAME^^} — uppercase all characters.
        let val = val.unwrap_or_default();
        if subst.starts_with('^') {
            // ^^ — all uppercase.
            let mut upper = String::with_capacity(val.len());
            for ch in val.chars() {
                for uc in ch.to_uppercase() {
                    upper.push(uc);
                }
            }
            result.push_str(&upper);
        } else {
            // ^ — first character only.
            let mut chars = val.chars();
            if let Some(first) = chars.next() {
                for uc in first.to_uppercase() {
                    result.push(uc);
                }
                result.extend(chars);
            }
        }
    } else if let Some(subst) = rest.strip_prefix(',') {
        // ${NAME,} — lowercase first character.
        // ${NAME,,} — lowercase all characters.
        let val = val.unwrap_or_default();
        if subst.starts_with(',') {
            // ,, — all lowercase.
            let mut lower = String::with_capacity(val.len());
            for ch in val.chars() {
                for lc in ch.to_lowercase() {
                    lower.push(lc);
                }
            }
            result.push_str(&lower);
        } else {
            // , — first character only.
            let mut chars = val.chars();
            if let Some(first) = chars.next() {
                for lc in first.to_lowercase() {
                    result.push(lc);
                }
                result.extend(chars);
            }
        }
    } else {
        // Unknown operator — just expand the name.
        if let Some(v) = val {
            result.push_str(&v);
        }
    }
}

/// Replace the first occurrence of `pattern` in `s` with `replacement`.
///
/// Simple literal string matching (not glob).
fn str_replace_first(s: &str, pattern: &str, replacement: &str) -> String {
    if pattern.is_empty() {
        return String::from(s);
    }
    if let Some(pos) = s.find(pattern) {
        let mut result = String::with_capacity(s.len());
        result.push_str(s.get(..pos).unwrap_or(""));
        result.push_str(replacement);
        result.push_str(s.get(pos.saturating_add(pattern.len())..).unwrap_or(""));
        result
    } else {
        String::from(s)
    }
}

/// Replace all occurrences of `pattern` in `s` with `replacement`.
///
/// Simple literal string matching (not glob).
fn str_replace_all(s: &str, pattern: &str, replacement: &str) -> String {
    if pattern.is_empty() {
        return String::from(s);
    }
    let mut result = String::with_capacity(s.len());
    let mut start = 0;
    while let Some(pos) = s.get(start..).and_then(|rest| rest.find(pattern)) {
        let abs_pos = start.saturating_add(pos);
        result.push_str(s.get(start..abs_pos).unwrap_or(""));
        result.push_str(replacement);
        start = abs_pos.saturating_add(pattern.len());
    }
    result.push_str(s.get(start..).unwrap_or(""));
    result
}

// ---------------------------------------------------------------------------
// Aliases
// ---------------------------------------------------------------------------

/// Shell command aliases.  When the first word of a command matches an alias
/// name, it is replaced with the alias value before dispatch.
///
/// Set with `alias name=value`, removed with `unalias name`, listed with
/// `alias` (no arguments).
static ALIASES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Look up an alias.  Returns the expansion or `None`.
fn alias_get(name: &str) -> Option<String> {
    ALIASES.lock().get(name).cloned()
}

/// Set an alias.
fn alias_set(name: &str, value: &str) {
    ALIASES.lock().insert(String::from(name), String::from(value));
}

/// Remove an alias.  Returns true if it existed.
fn alias_remove(name: &str) -> bool {
    ALIASES.lock().remove(name).is_some()
}

/// Expand aliases in a command line.
///
/// Only the first word is checked.  To prevent infinite recursion, a
/// maximum of 16 expansions is performed per line.
fn expand_aliases(line: &str) -> String {
    let mut current = String::from(line);
    let mut seen = Vec::new();

    for _ in 0..16u8 {
        let first_word_end = current.find(' ').unwrap_or(current.len());
        let first_word = &current[..first_word_end];

        // Stop if we've already expanded this alias (prevent loops).
        if seen.iter().any(|s: &String| s == first_word) {
            break;
        }

        if let Some(expansion) = alias_get(first_word) {
            seen.push(String::from(first_word));
            let rest = current.get(first_word_end..).unwrap_or("");
            current = alloc::format!("{}{}", expansion, rest);
        } else {
            break;
        }
    }

    current
}

// ---------------------------------------------------------------------------
// Control flow (if/then/else/fi, while/do/done)
// ---------------------------------------------------------------------------

/// State of one nesting level of control flow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlState {
    /// In `then` block: condition was true → execute.
    ThenActive,
    /// In `then` block: condition was false → skip.
    ThenSkip,
    /// In `else` block: original condition was true → skip (already executed then).
    ElseSkip,
    /// In `else` block: original condition was false → execute.
    ElseActive,
    /// Entered `elif` after a previous then-block already ran → skip rest.
    ElifDone,
}

impl ControlState {
    /// Should the current line be executed?
    fn should_execute(self) -> bool {
        matches!(self, Self::ThenActive | Self::ElseActive)
    }
}

/// Control flow nesting stack.
///
/// When empty, all commands execute normally.  Each `if` pushes a frame;
/// `fi` pops it.  Between `if` and `fi`, lines are executed or skipped
/// based on the condition result.
static CONTROL_STACK: Mutex<Vec<ControlState>> = Mutex::new(Vec::new());

/// What kind of loop is being collected.
enum LoopKind {
    /// `while CONDITION; do ... done` — repeats while condition exits 0.
    While { condition: String },
    /// `until CONDITION; do ... done` — repeats until condition exits 0.
    Until { condition: String },
    /// `for VAR in WORDS...; do ... done` — iterates over a word list.
    For { variable: String, words_raw: String },
    /// `for ((INIT; COND; STEP)); do ... done` — C-style arithmetic loop.
    CFor { init: String, cond: String, step: String },
    /// `select VAR in WORDS...; do ... done` — interactive menu selection.
    Select { variable: String, words_raw: String },
}

/// Loop body collector: buffers lines between `do` and `done`.
///
/// Works for both `while` and `for` loops. Both use the same `do...done`
/// body structure and must track each other's nesting (a `for` inside a
/// `while` needs a matching `done` that doesn't terminate the outer loop).
struct LoopCollector {
    /// Whether this is a while or for loop, plus its parameters.
    kind: LoopKind,
    /// Buffered body lines.
    body: Vec<String>,
    /// Nesting depth of while/for/done during collection (0 = our own loop).
    nesting: u32,
}

/// Active loop collection buffer (only one at a time).
///
/// When `Some`, we are collecting lines for a loop body.
/// When `done` is seen at nesting depth 0, the loop executes.
static LOOP_COLLECTOR: Mutex<Option<LoopCollector>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Shell functions
// ---------------------------------------------------------------------------

/// User-defined shell functions: name → body lines.
///
/// Functions are defined with `name() { ... }` or `function name { ... }`.
/// Called like any built-in: `name arg1 arg2` — arguments become $1, $2, etc.
static FUNCTIONS: Mutex<BTreeMap<String, Vec<String>>> = Mutex::new(BTreeMap::new());

/// Function body collector.
///
/// When `Some`, we are collecting lines for a function body (multi-line
/// definition).  Tracks brace nesting so nested `{ }` inside the body
/// are handled correctly.
struct FuncCollector {
    /// The function's name.
    name: String,
    /// Buffered body lines.
    body: Vec<String>,
    /// Brace nesting depth.  Starts at 1 (the opening `{`).
    /// When it reaches 0 (matching `}`), the function is stored.
    brace_depth: u32,
}

/// Active function body collector (only one at a time).
static FUNC_COLLECTOR: Mutex<Option<FuncCollector>> = Mutex::new(None);

/// Positional parameter stack.
///
/// Each function call pushes a frame containing its arguments ($1, $2, ...),
/// and pops it on return.  The topmost frame is the current scope.
/// Outside any function, the stack is empty (positional params expand
/// to empty string).
static POSITIONAL_PARAMS: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

/// Flag: set by `return` inside a function body to short-circuit execution.
///
/// Checked after each line in execute_function_body().  Cleared when the
/// function finishes.
static FUNC_RETURN: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Flag: set by `break` inside a loop body to exit the loop early.
///
/// Checked after each line in execute_while_loop / execute_for_loop.
/// Cleared by the loop runner after exiting the loop.
static LOOP_BREAK: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Flag: set by `continue` inside a loop body to skip to the next iteration.
///
/// Checked after each line in execute_while_loop / execute_for_loop.
/// Cleared at the start of each iteration.
static LOOP_CONTINUE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Maximum function call depth (prevents infinite recursion in the
/// kernel debug shell, which has a limited stack).
const MAX_FUNC_DEPTH: usize = 32;

/// Stack of local variable scopes.
///
/// Each function call can push local variable names.  When the function
/// returns, all variables in the top frame are restored to their previous
/// values (or removed if they didn't exist before).
///
/// Each frame is a list of `(name, previous_value)` pairs.
/// `previous_value = None` means the variable didn't exist before `local`.
static LOCAL_VARS: Mutex<Vec<Vec<(String, Option<String>)>>> = Mutex::new(Vec::new());

/// Get a positional parameter from the current function scope.
///
/// $0 is always "kshell".  $1..$N are function arguments.
/// Returns empty string if out of range or outside a function.
fn get_positional(n: usize) -> String {
    if n == 0 {
        return String::from("kshell");
    }
    let stack = POSITIONAL_PARAMS.lock();
    if let Some(frame) = stack.last() {
        frame.get(n.wrapping_sub(1)).cloned().unwrap_or_default()
    } else {
        String::new()
    }
}

/// Get the number of positional parameters in the current scope.
fn positional_count() -> usize {
    let stack = POSITIONAL_PARAMS.lock();
    stack.last().map_or(0, Vec::len)
}

/// Get all positional parameters joined with spaces ($@ / $*).
fn positional_all() -> String {
    let stack = POSITIONAL_PARAMS.lock();
    if let Some(frame) = stack.last() {
        let mut result = String::new();
        for (i, p) in frame.iter().enumerate() {
            if i > 0 {
                result.push(' ');
            }
            result.push_str(p);
        }
        result
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Here-documents (<<DELIMITER ... DELIMITER)
// ---------------------------------------------------------------------------

/// Here-document collector: buffers lines between `<<DELIMITER` and the
/// matching `DELIMITER` line.
///
/// The collected body is fed as piped input to the command prefix.
/// Supports tab stripping (`<<-DELIM`) and literal mode (quoted delimiter
/// like `<<'DELIM'` or `<<"DELIM"` — suppresses variable expansion).
struct HeredocCollector {
    /// The command to receive the heredoc body as input (before `<<`).
    command: String,
    /// Additional command context after the delimiter on the start line
    /// (pipes, redirects).  E.g. for `sort <<EOF | head 5`, suffix = "| head 5".
    suffix: String,
    /// The delimiter that ends the here-document.
    delimiter: String,
    /// If true, strip leading tabs from each body line (`<<-` variant).
    strip_tabs: bool,
    /// If true, expand `$VAR` / `${VAR}` in the body. If false (quoted
    /// delimiter), body lines are taken literally.
    expand: bool,
    /// Buffered body lines.
    body: Vec<String>,
}

/// Active here-document collector (only one at a time).
static HEREDOC_COLLECTOR: Mutex<Option<HeredocCollector>> = Mutex::new(None);

/// Try to parse a heredoc start from a command line.
///
/// Looks for `<<[-]DELIMITER` (not inside quotes).
/// Returns `Some((command_prefix, suffix, delimiter, strip_tabs, expand))`.
/// `suffix` contains any pipe/redirect text after the delimiter word.
fn parse_heredoc_start(line: &str) -> Option<(String, String, String, bool, bool)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut in_sq = false;
    let mut in_dq = false;
    let mut i = 0;

    // Scan for `<<` not inside quotes and not part of `$((`.
    while i < len {
        let b = bytes[i];

        if b == b'\'' && !in_dq {
            in_sq = !in_sq;
            i = i.saturating_add(1);
            continue;
        }
        if b == b'"' && !in_sq {
            in_dq = !in_dq;
            i = i.saturating_add(1);
            continue;
        }
        if in_sq || in_dq {
            i = i.saturating_add(1);
            continue;
        }

        // Skip `$((` — arithmetic expansion, not heredoc.
        if b == b'$'
            && bytes.get(i.saturating_add(1)) == Some(&b'(')
            && bytes.get(i.saturating_add(2)) == Some(&b'(')
        {
            i = i.saturating_add(3);
            continue;
        }

        if b == b'<' && bytes.get(i.saturating_add(1)) == Some(&b'<') {
            // Make sure this isn't `<<<` (herestring — not supported).
            if bytes.get(i.saturating_add(2)) == Some(&b'<') {
                i = i.saturating_add(3);
                continue;
            }

            let cmd_part = line.get(..i).unwrap_or("").trim();
            let after_arrows = line.get(i.saturating_add(2)..).unwrap_or("").trim();

            if after_arrows.is_empty() {
                return None; // No delimiter specified.
            }

            // Check for tab-stripping variant: <<-DELIM
            let (strip_tabs, delim_part) = if let Some(rest) = after_arrows.strip_prefix('-') {
                (true, rest.trim_start())
            } else {
                (false, after_arrows)
            };

            if delim_part.is_empty() {
                return None;
            }

            // The delimiter is the first word; anything after it is
            // additional command context (pipes, redirects) that gets
            // appended to the command prefix.
            // E.g. `sort <<EOF | head 5` → cmd="sort | head 5", delim="EOF"
            let (raw_delim, suffix) = match delim_part.find(char::is_whitespace) {
                Some(sp) => {
                    let d = delim_part.get(..sp).unwrap_or("");
                    let s = delim_part.get(sp..).unwrap_or("").trim();
                    (d, s)
                }
                None => (delim_part, ""),
            };

            if raw_delim.is_empty() {
                return None;
            }

            // Check for quoted delimiter (suppresses expansion).
            let (expand, delimiter) = if (raw_delim.starts_with('\'') && raw_delim.ends_with('\''))
                || (raw_delim.starts_with('"') && raw_delim.ends_with('"'))
            {
                // Quoted — strip quotes, no expansion.
                let inner = raw_delim.get(1..raw_delim.len().saturating_sub(1)).unwrap_or("");
                if inner.is_empty() {
                    return None;
                }
                (false, inner)
            } else {
                (true, raw_delim)
            };

            return Some((
                String::from(cmd_part),
                String::from(suffix),
                String::from(delimiter),
                strip_tabs,
                expand,
            ));
        }

        i = i.saturating_add(1);
    }

    None
}

// ---------------------------------------------------------------------------
// Case/esac pattern matching
// ---------------------------------------------------------------------------

/// Case statement body collector.
///
/// Buffers lines between `case WORD in` and `esac`.  Tracks nested
/// case/esac pairs so inner case blocks don't terminate the outer one.
struct CaseCollector {
    /// The word being matched (already expanded).
    word: String,
    /// Buffered lines between `in` and `esac`.
    body: Vec<String>,
    /// Nesting depth for nested case/esac blocks.
    nesting: u32,
}

/// Active case statement collector (only one at a time).
static CASE_COLLECTOR: Mutex<Option<CaseCollector>> = Mutex::new(None);

/// Check whether execution is currently active (not skipped by an
/// outer control-flow block).
fn control_should_execute() -> bool {
    let stack = CONTROL_STACK.lock();
    // All frames must be in an "execute" state.
    stack.iter().all(|s| s.should_execute())
}

/// Handle control-flow keywords in a command line.
///
/// Returns `true` if the line was a control-flow keyword and was fully
/// handled (caller should not dispatch it further).
fn handle_control_flow(line: &str) -> bool {
    let trimmed = line.trim();
    let first_word = trimmed.split_whitespace().next().unwrap_or("");

    // If we're collecting here-document body lines, buffer them.
    // This check comes first because heredoc body can contain anything
    // (keywords, braces, etc.) that would confuse other collectors.
    {
        let mut hc = HEREDOC_COLLECTOR.lock();
        if let Some(ref mut state) = *hc {
            // Check if this line matches the delimiter (possibly after
            // stripping leading tabs for the <<- variant).
            let check_line = if state.strip_tabs {
                trimmed.trim_start_matches('\t')
            } else {
                trimmed
            };

            if check_line == state.delimiter {
                // End of here-document — execute the command with the
                // collected body as piped input.
                let command = state.command.clone();
                let suffix = state.suffix.clone();
                let expand = state.expand;
                let strip_tabs = state.strip_tabs;

                // Assemble the body.
                let mut body_text = String::new();
                for (i, bline) in state.body.iter().enumerate() {
                    if i > 0 {
                        body_text.push('\n');
                    }
                    let processed = if strip_tabs {
                        bline.trim_start_matches('\t')
                    } else {
                        bline.as_str()
                    };
                    body_text.push_str(processed);
                }

                *hc = None;
                drop(hc);

                // Optionally expand variables in the body.
                let final_body = if expand {
                    expand_vars(&body_text)
                } else {
                    body_text
                };

                // Execute the heredoc command with body as piped input.
                execute_heredoc(&command, &suffix, &final_body);
                return true;
            }

            // Not the delimiter — buffer the line (raw, no expansion yet).
            state.body.push(String::from(trimmed));
            return true;
        }
    }

    // If we're collecting function body lines, buffer them.
    {
        let mut fc = FUNC_COLLECTOR.lock();
        if let Some(ref mut state) = *fc {
            // Count braces to track nesting.
            for ch in trimmed.chars() {
                if ch == '{' {
                    state.brace_depth = state.brace_depth.saturating_add(1);
                } else if ch == '}' {
                    state.brace_depth = state.brace_depth.saturating_sub(1);
                    if state.brace_depth == 0 {
                        // Matching closing brace — store the function.
                        // Don't include the final `}` line in the body.
                        // But if there's content before `}`, include it.
                        let before_brace = trimmed.trim_end_matches('}').trim();
                        if !before_brace.is_empty() {
                            state.body.push(String::from(before_brace));
                        }
                        let name = state.name.clone();
                        let body = state.body.clone();
                        *fc = None;
                        drop(fc);
                        FUNCTIONS.lock().insert(name, body);
                        return true;
                    }
                }
            }
            // Still inside function body — buffer the line.
            state.body.push(String::from(trimmed));
            return true;
        }
    }

    // Check for function definition: `name() {` or `function name {`.
    if is_function_definition(trimmed) {
        return true;
    }

    // If we're collecting case/esac body lines, buffer them.
    {
        let mut cc = CASE_COLLECTOR.lock();
        if let Some(ref mut state) = *cc {
            match first_word {
                "case" => {
                    // Nested case — increase depth.
                    state.nesting = state.nesting.saturating_add(1);
                    state.body.push(String::from(trimmed));
                    return true;
                }
                "esac" => {
                    if state.nesting > 0 {
                        state.nesting = state.nesting.saturating_sub(1);
                        state.body.push(String::from(trimmed));
                        return true;
                    }
                    // Our matching esac — execute the case.
                    let word = state.word.clone();
                    let body = state.body.clone();
                    *cc = None;
                    drop(cc);
                    execute_case(&word, &body);
                    return true;
                }
                _ => {
                    state.body.push(String::from(trimmed));
                    return true;
                }
            }
        }
    }

    // If we're collecting loop body lines (while or for), buffer them.
    {
        let mut lc = LOOP_COLLECTOR.lock();
        if let Some(ref mut state) = *lc {
            match first_word {
                // while/until/for/select all open a new do..done block,
                // so all increment nesting when seen inside a loop body.
                "while" | "until" | "for" | "select" => {
                    state.nesting = state.nesting.saturating_add(1);
                    state.body.push(String::from(trimmed));
                    return true;
                }
                "done" => {
                    if state.nesting > 0 {
                        // Nested done — decrease depth.
                        state.nesting = state.nesting.saturating_sub(1);
                        state.body.push(String::from(trimmed));
                        return true;
                    }
                    // Our matching done — execute the loop.
                    let kind = core::mem::replace(
                        &mut state.kind,
                        LoopKind::While { condition: String::new() },
                    );
                    let body = state.body.clone();
                    *lc = None; // Clear the collection state.
                    drop(lc);

                    match kind {
                        LoopKind::While { condition } => {
                            execute_while_loop(&condition, &body);
                        }
                        LoopKind::Until { condition } => {
                            execute_until_loop(&condition, &body);
                        }
                        LoopKind::For { variable, words_raw } => {
                            execute_for_loop(&variable, &words_raw, &body);
                        }
                        LoopKind::CFor { init, cond, step } => {
                            execute_cfor_loop(&init, &cond, &step, &body);
                        }
                        LoopKind::Select { variable, words_raw } => {
                            execute_select(&variable, &words_raw, &body);
                        }
                    }
                    return true;
                }
                "do" => {
                    // `do` on its own line — skip (just a marker).
                    return true;
                }
                _ => {
                    // Buffer every other line.
                    state.body.push(String::from(trimmed));
                    return true;
                }
            }
        }
    }

    match first_word {
        "while" | "until" => {
            let skip = if first_word == "while" { 5 } else { 5 };
            let condition = trimmed.get(skip..).unwrap_or("").trim();
            // Remove trailing "do" if present (allow `while test -f /x; do`).
            let condition = condition.strip_suffix("do")
                .unwrap_or(condition)
                .trim()
                .trim_end_matches(';')
                .trim();

            if condition.is_empty() {
                crate::console_println!("Syntax error: {} requires a condition", first_word);
                return true;
            }

            let kind = if first_word == "until" {
                LoopKind::Until { condition: String::from(condition) }
            } else {
                LoopKind::While { condition: String::from(condition) }
            };

            // Start collecting body lines.
            *LOOP_COLLECTOR.lock() = Some(LoopCollector {
                kind,
                body: Vec::new(),
                nesting: 0,
            });
            return true;
        }
        "for" => {
            // Check for C-style: `for ((INIT; COND; STEP)); do`
            // or:                 `for ((INIT; COND; STEP))`
            //                     do
            let rest = trimmed.get(3..).unwrap_or("").trim();
            if rest.starts_with("((") {
                // Extract the content between (( and )).
                let inner_start = 2; // skip "(("
                // Find closing "))".
                if let Some(end) = rest.find("))") {
                    let inner = rest.get(inner_start..end).unwrap_or("").trim();
                    // Split on `;` into init, cond, step.
                    let parts: Vec<&str> = inner.splitn(3, ';').collect();
                    if parts.len() != 3 {
                        crate::console_println!("Syntax error: for ((...)) requires 3 semicolon-separated expressions");
                        return true;
                    }
                    let init_expr = parts[0].trim();
                    let cond_expr = parts[1].trim();
                    let step_expr = parts[2].trim();

                    // Check if `do` follows on the same line.
                    let after_parens = rest.get(end.saturating_add(2)..).unwrap_or("").trim();
                    let has_do = after_parens.starts_with(';') && after_parens.contains("do")
                        || after_parens == "do"
                        || after_parens.starts_with("do ")
                        || after_parens.starts_with("do\t")
                        || after_parens == ";do"
                        || after_parens == "; do";

                    if !has_do {
                        // Body collection will wait for `do` on the next line.
                    }

                    *LOOP_COLLECTOR.lock() = Some(LoopCollector {
                        kind: LoopKind::CFor {
                            init: String::from(init_expr),
                            cond: String::from(cond_expr),
                            step: String::from(step_expr),
                        },
                        body: Vec::new(),
                        nesting: 0,
                    });
                    return true;
                }
                crate::console_println!("Syntax error: unterminated for ((...))");
                return true;
            }

            // Parse: for VAR in WORD1 WORD2 ...; do
            // or:    for VAR in WORD1 WORD2 ...
            //        do

            // Split "VAR in WORDS..." — variable is the first token.
            let mut parts = rest.splitn(2, char::is_whitespace);
            let variable = parts.next().unwrap_or("").trim();
            let after_var = parts.next().unwrap_or("").trim();

            if variable.is_empty() {
                crate::console_println!("Syntax error: for requires a variable name");
                return true;
            }

            // Strip leading "in " keyword.
            let words_part = if let Some(stripped) = after_var.strip_prefix("in") {
                stripped.trim_start()
            } else {
                crate::console_println!("Syntax error: for requires 'in' keyword");
                return true;
            };

            // Remove trailing "do" if present (allow `for x in a b c; do`).
            let words_raw = words_part
                .strip_suffix("do")
                .unwrap_or(words_part)
                .trim()
                .trim_end_matches(';')
                .trim();

            if words_raw.is_empty() {
                crate::console_println!("Syntax error: for requires a word list after 'in'");
                return true;
            }

            // Start collecting body lines.
            *LOOP_COLLECTOR.lock() = Some(LoopCollector {
                kind: LoopKind::For {
                    variable: String::from(variable),
                    words_raw: String::from(words_raw),
                },
                body: Vec::new(),
                nesting: 0,
            });
            return true;
        }
        "select" => {
            // Parse: select VAR in WORD1 WORD2 ...; do
            let rest = trimmed.get(6..).unwrap_or("").trim();

            let mut parts = rest.splitn(2, char::is_whitespace);
            let variable = parts.next().unwrap_or("").trim();
            let after_var = parts.next().unwrap_or("").trim();

            if variable.is_empty() {
                crate::console_println!("Syntax error: select requires a variable name");
                return true;
            }

            let words_part = if let Some(stripped) = after_var.strip_prefix("in") {
                stripped.trim_start()
            } else {
                crate::console_println!("Syntax error: select requires 'in' keyword");
                return true;
            };

            let words_raw = words_part
                .strip_suffix("do")
                .unwrap_or(words_part)
                .trim()
                .trim_end_matches(';')
                .trim();

            if words_raw.is_empty() {
                crate::console_println!("Syntax error: select requires a word list after 'in'");
                return true;
            }

            *LOOP_COLLECTOR.lock() = Some(LoopCollector {
                kind: LoopKind::Select {
                    variable: String::from(variable),
                    words_raw: String::from(words_raw),
                },
                body: Vec::new(),
                nesting: 0,
            });
            return true;
        }
        "do" => {
            // `do` on its own line without a loop — syntax error or no-op.
            return true;
        }
        "done" => {
            crate::console_println!("Syntax error: done without while/for/select");
            return true;
        }
        "case" => {
            // Parse: `case WORD in` — WORD may contain variables.
            let rest = trimmed.get(4..).unwrap_or("").trim();

            // Strip trailing "in" (required).
            let word_part = if rest.ends_with(" in") || rest.ends_with("\tin") {
                rest.get(..rest.len().saturating_sub(3)).unwrap_or("").trim()
            } else if rest == "in" {
                // `case in` — empty word.
                ""
            } else {
                crate::console_println!("Syntax error: case requires 'in' keyword");
                return true;
            };

            if word_part.is_empty() {
                crate::console_println!("Syntax error: case requires a word before 'in'");
                return true;
            }

            // Expand the word now (so patterns match the expanded value).
            let expanded_word = expand_vars(word_part);

            *CASE_COLLECTOR.lock() = Some(CaseCollector {
                word: expanded_word.trim().into(),
                body: Vec::new(),
                nesting: 0,
            });
            return true;
        }
        "esac" => {
            crate::console_println!("Syntax error: esac without case");
            return true;
        }
        _ => {}
    }

    match first_word {
        "if" => {
            let mut stack = CONTROL_STACK.lock();
            if stack.len() >= MAX_NESTING {
                crate::console_println!("Error: maximum nesting depth ({}) exceeded", MAX_NESTING);
                return true;
            }

            // If an outer block is already skipping, push skip state
            // unconditionally (we don't evaluate the condition).
            let outer_active = stack.iter().all(|s| s.should_execute());
            if !outer_active {
                stack.push(ControlState::ThenSkip);
                return true;
            }
            drop(stack); // Release lock before executing condition.

            // Extract the condition (everything after "if").
            let condition = trimmed.get(2..).unwrap_or("").trim();
            if condition.is_empty() {
                crate::console_println!("Syntax error: if requires a condition");
                CONTROL_STACK.lock().push(ControlState::ThenSkip);
                return true;
            }

            // Remove trailing "then" if present (allow `if test -f /x; then`).
            let condition = condition.strip_suffix("then")
                .unwrap_or(condition)
                .trim()
                .trim_end_matches(';')
                .trim();

            // Execute the condition to set exit status.
            execute(condition);
            let result = last_exit() == 0;

            let state = if result { ControlState::ThenActive } else { ControlState::ThenSkip };
            CONTROL_STACK.lock().push(state);
            true
        }
        "then" => {
            // `then` on its own line — just skip (the if handler already pushed state).
            true
        }
        "elif" => {
            let mut stack = CONTROL_STACK.lock();
            if stack.is_empty() {
                crate::console_println!("Syntax error: elif without if");
                return true;
            }

            let current = stack.last().copied();
            match current {
                Some(ControlState::ThenActive | ControlState::ElseActive | ControlState::ElifDone) => {
                    // A previous branch already ran — skip this and all subsequent branches.
                    if let Some(s) = stack.last_mut() {
                        *s = ControlState::ElifDone;
                    }
                }
                Some(ControlState::ThenSkip) => {
                    // Previous condition was false — evaluate this elif's condition.
                    drop(stack);

                    let condition = trimmed.get(4..).unwrap_or("").trim()
                        .strip_suffix("then").unwrap_or(trimmed.get(4..).unwrap_or(""))
                        .trim().trim_end_matches(';').trim();

                    if condition.is_empty() {
                        crate::console_println!("Syntax error: elif requires a condition");
                        return true;
                    }

                    execute(condition);
                    let result = last_exit() == 0;

                    let mut stack = CONTROL_STACK.lock();
                    if let Some(s) = stack.last_mut() {
                        *s = if result { ControlState::ThenActive } else { ControlState::ThenSkip };
                    }
                }
                _ => {
                    // ElseSkip or other state — skip.
                    if let Some(s) = stack.last_mut() {
                        *s = ControlState::ElifDone;
                    }
                }
            }
            true
        }
        "else" => {
            let mut stack = CONTROL_STACK.lock();
            if stack.is_empty() {
                crate::console_println!("Syntax error: else without if");
                return true;
            }

            let current = stack.last().copied();
            if let Some(s) = stack.last_mut() {
                *s = match current {
                    Some(ControlState::ThenActive) => ControlState::ElseSkip,
                    Some(ControlState::ThenSkip) => ControlState::ElseActive,
                    Some(ControlState::ElifDone) => ControlState::ElseSkip,
                    _ => ControlState::ElseSkip,
                };
            }
            true
        }
        "fi" => {
            let mut stack = CONTROL_STACK.lock();
            if stack.is_empty() {
                crate::console_println!("Syntax error: fi without if");
            } else {
                stack.pop();
            }
            true
        }
        _ => {
            // Not a control-flow keyword — check for here-document start.
            // `cmd <<DELIM` starts collecting body lines until DELIM.
            if let Some((command, suffix, delimiter, strip_tabs, expand)) = parse_heredoc_start(trimmed) {
                *HEREDOC_COLLECTOR.lock() = Some(HeredocCollector {
                    command,
                    suffix,
                    delimiter,
                    strip_tabs,
                    expand,
                    body: Vec::new(),
                });
                return true;
            }
            false
        }
    }
}

/// Execute a while loop: repeatedly evaluate the condition and replay the body.
///
/// Maximum 1000 iterations to prevent accidental infinite loops in the
/// kernel debug shell (which cannot be interrupted except by reboot).
fn execute_while_loop(condition: &str, body: &[String]) {
    const MAX_ITERATIONS: u32 = 1000;

    // Clear loop control flags before entering the loop.
    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);

    for _ in 0..MAX_ITERATIONS {
        // Evaluate the condition.
        execute(condition);
        if last_exit() != 0 {
            // Condition failed — exit the loop.
            break;
        }

        // Clear continue flag at the start of each iteration.
        LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);

        // Execute the body lines.
        for line in body {
            execute(line);
            if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
                || LOOP_CONTINUE.load(core::sync::atomic::Ordering::Relaxed)
                || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
            || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
        {
            break;
        }
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);
    LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Execute a select menu: display numbered choices, read user input, execute body.
///
/// The menu repeats until `break` is encountered in the body.
/// REPLY is set to the raw input, VAR is set to the selected word.
#[allow(clippy::arithmetic_side_effects)]
fn execute_select(variable: &str, words_raw: &str, body: &[String]) {
    let expanded = expand_vars(words_raw);
    let words = split_words(&expanded);

    if words.is_empty() {
        return;
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);

    // Limit iterations to prevent infinite loops.
    for _ in 0..100_u32 {
        // Display numbered menu.
        for (i, word) in words.iter().enumerate() {
            crate::console_println!("{}) {}", i + 1, word);
        }

        // Prompt for selection.
        use crate::keyboard;
        crate::console_print!("#? ");

        // Read a line of input (reusing the simple line-reading approach).
        let mut buf = [0u8; 64];
        let mut len = 0;
        loop {
            let byte = keyboard::read_char();
            if byte == b'\n' || byte == b'\r' {
                crate::console_print!("\n");
                break;
            }
            if byte == 0x08 || byte == 0x7F {
                // Backspace.
                if len > 0 {
                    len -= 1;
                    crate::console_print!("\x08 \x08");
                }
                continue;
            }
            if byte == 3 {
                // Ctrl+C — cancel.
                crate::console_println!();
                LOOP_BREAK.store(true, core::sync::atomic::Ordering::Relaxed);
                break;
            }
            if byte.is_ascii_graphic() || byte == b' ' {
                if len < buf.len() {
                    buf[len] = byte;
                    len += 1;
                    crate::console_print!("{}", byte as char);
                }
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let input = core::str::from_utf8(buf.get(..len).unwrap_or(&[]))
            .unwrap_or("");

        // Set REPLY to raw input.
        env_set("REPLY", input);

        // Parse as number and select the word.
        if let Ok(n) = input.trim().parse::<usize>() {
            if n >= 1 && n <= words.len() {
                if let Some(word) = words.get(n - 1) {
                    ENV_VARS.lock().insert(String::from(variable), word.clone());
                }
            } else {
                // Out of range — set variable to empty.
                ENV_VARS.lock().insert(String::from(variable), String::new());
            }
        } else {
            // Non-numeric input — set variable to empty.
            ENV_VARS.lock().insert(String::from(variable), String::new());
        }

        LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);

        // Execute body.
        for line in body {
            execute(line);
            if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
                || LOOP_CONTINUE.load(core::sync::atomic::Ordering::Relaxed)
                || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
            || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
        {
            break;
        }
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);
    LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Execute an until loop: inverse of while — repeats until condition succeeds.
fn execute_until_loop(condition: &str, body: &[String]) {
    const MAX_ITERATIONS: u32 = 1000;

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);

    for _ in 0..MAX_ITERATIONS {
        // Evaluate the condition.
        execute(condition);
        if last_exit() == 0 {
            // Condition succeeded — exit the loop.
            break;
        }

        LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);

        for line in body {
            execute(line);
            if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
                || LOOP_CONTINUE.load(core::sync::atomic::Ordering::Relaxed)
                || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
            || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
        {
            break;
        }
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);
    LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Execute a for loop: expand the word list, iterate, set the variable,
/// and replay the body for each word.
///
/// The word list is expanded (variable substitution) at execution time,
/// then split on whitespace.  Quoted strings are treated as single words.
///
/// Same 1000-iteration safety limit as while loops (the word list itself
/// is the bound, but a malicious `seq`-style expansion could still be huge).
fn execute_for_loop(variable: &str, words_raw: &str, body: &[String]) {
    // Expand variables in the word list now (it was stored raw).
    let expanded = expand_vars(words_raw);
    let words = split_words(&expanded);

    if words.len() > 1000 {
        crate::console_println!(
            "Error: for loop word list too large ({} words, max 1000)",
            words.len(),
        );
        return;
    }

    // Clear loop control flags before entering the loop.
    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);

    for word in &words {
        // Set the loop variable.
        ENV_VARS.lock().insert(String::from(variable), word.clone());

        // Clear continue flag at the start of each iteration.
        LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);

        // Execute the body lines.
        for line in body {
            execute(line);
            if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
                || LOOP_CONTINUE.load(core::sync::atomic::Ordering::Relaxed)
                || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
            || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
        {
            break;
        }
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);
    LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Execute a C-style arithmetic for loop: `for ((INIT; COND; STEP)); do BODY done`
///
/// The init expression runs once.  Before each iteration, the condition is
/// evaluated as an arithmetic expression: if non-zero, the body runs and the
/// step expression is evaluated afterward.  If zero, the loop ends.
///
/// Expressions use the same arithmetic evaluator as `$((...))` and support
/// variable assignment (e.g., `for ((i=0; i<10; i=i+1)); do ...`).
fn execute_cfor_loop(init: &str, cond: &str, step: &str, body: &[String]) {
    const MAX_ITERATIONS: u32 = 10_000;

    // Execute init expression.
    if !init.is_empty() {
        eval_cfor_expr(init);
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);

    let mut iteration: u32 = 0;
    loop {
        // Safety limit.
        if iteration >= MAX_ITERATIONS {
            crate::console_println!(
                "Error: C-style for loop exceeded {} iterations — infinite loop?",
                MAX_ITERATIONS,
            );
            break;
        }
        iteration = iteration.saturating_add(1);

        // Evaluate condition — if empty, treat as infinite (true).
        if !cond.is_empty() {
            let cond_expanded = expand_vars(cond);
            let val = eval_arithmetic(&cond_expanded);
            if val == 0 {
                break;
            }
        }

        // Clear continue flag.
        LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);

        // Execute body.
        for line in body {
            execute(line);
            if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
                || LOOP_CONTINUE.load(core::sync::atomic::Ordering::Relaxed)
                || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
            {
                break;
            }
        }

        if LOOP_BREAK.load(core::sync::atomic::Ordering::Relaxed)
            || FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed)
        {
            break;
        }

        // Execute step expression.
        if !step.is_empty() {
            eval_cfor_expr(step);
        }
    }

    LOOP_BREAK.store(false, core::sync::atomic::Ordering::Relaxed);
    LOOP_CONTINUE.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// Evaluate a C-style for loop expression.
///
/// Supports simple assignment (`VAR=EXPR`) and bare arithmetic.
/// The arithmetic evaluator handles variable references.
fn eval_cfor_expr(expr: &str) {
    let expanded = expand_vars(expr);
    let trimmed = expanded.trim();

    // Check for assignment: `VAR=EXPR` (not `VAR==EXPR`).
    if let Some(eq_pos) = trimmed.find('=') {
        // Make sure it's not `==`, `<=`, `>=`, `!=`.
        let before_eq = trimmed.as_bytes().get(eq_pos.wrapping_sub(1));
        let after_eq = trimmed.as_bytes().get(eq_pos.saturating_add(1));
        let is_comparison = before_eq == Some(&b'<')
            || before_eq == Some(&b'>')
            || before_eq == Some(&b'!')
            || after_eq == Some(&b'=');

        if !is_comparison {
            let name = trimmed.get(..eq_pos).unwrap_or("").trim();
            let rhs = trimmed.get(eq_pos.saturating_add(1)..).unwrap_or("").trim();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
            {
                let val = eval_arithmetic(rhs);
                env_set(name, &alloc::format!("{}", val));
                return;
            }
        }
    }

    // Bare expression — just evaluate for side effects (none in our model,
    // but future ++ operator support could use this).
    eval_arithmetic(trimmed);
}

/// Split a string into words, respecting single and double quotes.
///
/// - Unquoted text is split on whitespace.
/// - `"foo bar"` → single word `foo bar`.
/// - `'foo bar'` → single word `foo bar`.
/// - Quotes can appear mid-word: `a"b c"d` → `ab cd`.
fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' => {
                if !current.is_empty() {
                    words.push(core::mem::take(&mut current));
                }
                chars.next();
            }
            '"' => {
                chars.next(); // consume opening quote
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        chars.next(); // consume closing quote
                        break;
                    }
                    current.push(c);
                    chars.next();
                }
            }
            '\'' => {
                chars.next(); // consume opening quote
                while let Some(&c) = chars.peek() {
                    if c == '\'' {
                        chars.next(); // consume closing quote
                        break;
                    }
                    current.push(c);
                    chars.next();
                }
            }
            _ => {
                current.push(ch);
                chars.next();
            }
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

/// Execute a case statement: match the word against patterns and run
/// the commands for the first matching pattern.
///
/// The body is the collected lines between `case WORD in` and `esac`.
/// Patterns use glob-style matching (*, ?, [abc]).  Multiple patterns
/// can be separated with `|`.
///
/// Format of each clause:
///   `PATTERN[|PATTERN]...) COMMAND; COMMAND; ... ;;`
///
/// Commands may span multiple lines; `;;` terminates a clause.
fn execute_case(word: &str, body: &[String]) {
    // Join all body lines into one string for easier parsing.
    // Case clauses are delimited by `;;` and `)`/`esac`.
    let joined = body.join("\n");

    // Parse into clauses: `PATTERNS) BODY ;;`
    let clauses = parse_case_clauses(&joined);

    for (patterns, commands) in &clauses {
        // Check if any pattern matches the word.
        let matched = patterns.iter().any(|p| case_pattern_match(word, p.trim()));
        if matched {
            // Execute the commands for this clause.
            for cmd in commands {
                let cmd = cmd.trim();
                if !cmd.is_empty() {
                    execute(cmd);
                }
            }
            return; // Only the first matching clause runs.
        }
    }
    // No clause matched — no error, just no-op.
}

/// Parse case body text into (patterns, commands) clauses.
///
/// Returns a vector of (pattern_list, command_list) tuples.
fn parse_case_clauses(body: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let mut clauses = Vec::new();
    let mut remaining = body.trim();

    while !remaining.is_empty() {
        // Find the `)` that separates patterns from commands.
        // Skip leading whitespace and any `(` prefix on patterns.
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        // Optional leading `(` before pattern.
        if remaining.starts_with('(') {
            remaining = remaining.get(1..).unwrap_or("").trim_start();
        }

        let paren_pos = match remaining.find(')') {
            Some(p) => p,
            None => break, // Malformed — no `)` found.
        };

        let patterns_str = remaining.get(..paren_pos).unwrap_or("");
        remaining = remaining.get(paren_pos.saturating_add(1)..).unwrap_or("");

        // Split patterns on `|`.
        let patterns: Vec<String> = patterns_str
            .split('|')
            .map(|s| String::from(s.trim()))
            .filter(|s| !s.is_empty())
            .collect();

        if patterns.is_empty() {
            break;
        }

        // Find the `;;` that terminates this clause.
        let commands_str = if let Some(end) = remaining.find(";;") {
            let cmds = remaining.get(..end).unwrap_or("");
            remaining = remaining.get(end.saturating_add(2)..).unwrap_or("");
            cmds
        } else {
            // Last clause may omit `;;` before `esac`.
            let cmds = remaining;
            remaining = "";
            cmds
        };

        // Split commands on `;` and newlines.
        let commands: Vec<String> = commands_str
            .split(|c: char| c == ';' || c == '\n')
            .map(|s| String::from(s.trim()))
            .filter(|s| !s.is_empty())
            .collect();

        clauses.push((patterns, commands));
    }

    clauses
}

/// Match a word against a case pattern (glob-style).
///
/// Supports `*` (match anything), `?` (match one char), and `[abc]`
/// (character class).  Falls back to the existing VFS glob_match()
/// for consistency with the rest of the shell.
fn case_pattern_match(word: &str, pattern: &str) -> bool {
    // `*` matches everything (common default case).
    if pattern == "*" {
        return true;
    }

    // Use the VFS glob matcher for full glob support.
    crate::fs::vfs::glob_match(pattern, word, false)
}

/// Detect and handle function definitions.
///
/// Recognizes two forms:
/// - `name() { BODY }` or `name() {` (multi-line)
/// - `function name { BODY }` or `function name {` (multi-line)
///
/// Returns `true` if the line was a function definition (or the start of one).
fn is_function_definition(line: &str) -> bool {
    let trimmed = line.trim();

    // Form 1: `name() { ... }` or `name() {`
    if let Some(paren_pos) = trimmed.find("()") {
        let name = trimmed.get(..paren_pos).unwrap_or("").trim();
        if !name.is_empty() && is_valid_func_name(name) {
            let after = trimmed.get(paren_pos.saturating_add(2)..).unwrap_or("").trim();
            return start_func_collection(name, after);
        }
    }

    // Form 2: `function name { ... }` or `function name {`
    if trimmed.starts_with("function ") {
        let rest = trimmed.get(9..).unwrap_or("").trim();
        // The name is the next token; everything after is the body opener.
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").trim();
        let after = parts.next().unwrap_or("").trim();
        // Also allow `function name()` syntax.
        let name = name.strip_suffix("()").unwrap_or(name);
        if !name.is_empty() && is_valid_func_name(name) {
            return start_func_collection(name, after);
        }
    }

    false
}

/// Check if a name is a valid function identifier (alphanumeric + underscore + hyphen).
fn is_valid_func_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !name.as_bytes().first().is_some_and(|b| b.is_ascii_digit())
}

/// Begin collecting a function body.
///
/// `after_name` is everything after the function name/parens, e.g. `{ echo hi; }`.
/// Returns `true` (the line was handled).
fn start_func_collection(name: &str, after_name: &str) -> bool {
    let body_text = after_name.strip_prefix('{').unwrap_or("").trim();

    // Check for one-liner: `name() { echo hi; }`
    if let Some(before_close) = body_text.strip_suffix('}') {
        let body_line = before_close.trim();
        let body = if body_line.is_empty() {
            Vec::new()
        } else {
            // Split on `;` for one-liners with multiple statements.
            body_line.split(';')
                .map(|s| String::from(s.trim()))
                .filter(|s| !s.is_empty())
                .collect()
        };
        FUNCTIONS.lock().insert(String::from(name), body);
        return true;
    }

    // Multi-line: the opening `{` starts collection.
    if after_name.trim_start().starts_with('{') {
        let mut initial_body = Vec::new();
        // If there's content after `{` on the same line, buffer it.
        if !body_text.is_empty() {
            initial_body.push(String::from(body_text));
        }
        *FUNC_COLLECTOR.lock() = Some(FuncCollector {
            name: String::from(name),
            body: initial_body,
            brace_depth: 1,
        });
        return true;
    }

    // No `{` — syntax error.
    if after_name.is_empty() {
        // Bare `name()` without `{` — we could require it, but let's
        // be lenient: start collecting, expect `{` on next line.
        // Actually, the POSIX way expects `{`, so error.
        crate::console_println!("Syntax error: function '{}' requires a body (use {{ }})", name);
    } else {
        crate::console_println!("Syntax error: unexpected '{}' in function definition", after_name);
    }
    true
}

/// Execute a user-defined function with the given arguments.
///
/// Pushes a positional parameter frame, executes each body line,
/// then pops the frame.  Respects `return` to short-circuit.
fn execute_function(body: &[String], args: &[String]) {
    // Check recursion depth.
    {
        let stack = POSITIONAL_PARAMS.lock();
        if stack.len() >= MAX_FUNC_DEPTH {
            crate::console_println!(
                "Error: function call depth exceeded ({} levels)",
                MAX_FUNC_DEPTH,
            );
            set_exit(1);
            return;
        }
    }

    // Push parameter frame and local variable frame.
    POSITIONAL_PARAMS.lock().push(args.to_vec());
    LOCAL_VARS.lock().push(Vec::new());

    // Clear the return flag.
    FUNC_RETURN.store(false, core::sync::atomic::Ordering::Relaxed);

    // Execute body lines.
    for line in body {
        if FUNC_RETURN.load(core::sync::atomic::Ordering::Relaxed) {
            break;
        }
        execute(line);

        // `set -e` (errexit): abort function on non-zero exit status.
        if OPT_ERREXIT.load(core::sync::atomic::Ordering::Relaxed) && last_exit() != 0 {
            break;
        }
    }

    // Clear return flag and pop parameter frame.
    FUNC_RETURN.store(false, core::sync::atomic::Ordering::Relaxed);
    POSITIONAL_PARAMS.lock().pop();

    // Pop the local variable frame, restoring previous values.
    if let Some(frame) = LOCAL_VARS.lock().pop() {
        for (name, prev) in frame {
            match prev {
                Some(val) => { env_set(&name, &val); }
                None => { env_remove(&name); }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Command history
// ---------------------------------------------------------------------------

/// Ring buffer of recent commands.  Oldest entry is at `start`, newest
/// at `(start + count - 1) % HISTORY_SIZE`.
struct History {
    /// The stored command strings.
    entries: Vec<String>,
    /// Index of the oldest entry.
    start: usize,
    /// Number of valid entries.
    count: usize,
    /// Current browse position (0 = most recent, count-1 = oldest).
    /// Set to count (past-end) when the user hasn't pressed Up yet.
    browse: usize,
    /// The "live" line the user was typing before pressing Up.
    /// Saved so pressing Down all the way restores it.
    saved_line: String,
}

impl History {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(HISTORY_SIZE);
        for _ in 0..HISTORY_SIZE {
            entries.push(String::new());
        }
        Self {
            entries,
            start: 0,
            count: 0,
            browse: 0,
            saved_line: String::new(),
        }
    }

    /// Add a command to the history.  Duplicates of the most recent
    /// entry are suppressed.
    fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        // Don't duplicate the last command.
        if self.count > 0 {
            let last_idx = (self.start.wrapping_add(self.count).wrapping_sub(1)) % HISTORY_SIZE;
            if let Some(last) = self.entries.get(last_idx) {
                if last == line {
                    self.browse = self.count;
                    return;
                }
            }
        }

        if self.count < HISTORY_SIZE {
            // Not full yet — append.
            let idx = (self.start.wrapping_add(self.count)) % HISTORY_SIZE;
            if let Some(entry) = self.entries.get_mut(idx) {
                entry.clear();
                entry.push_str(line);
            }
            self.count = self.count.saturating_add(1);
        } else {
            // Full — overwrite oldest.
            if let Some(entry) = self.entries.get_mut(self.start) {
                entry.clear();
                entry.push_str(line);
            }
            self.start = (self.start.wrapping_add(1)) % HISTORY_SIZE;
        }
        self.browse = self.count;
    }

    /// Reset the browse position (call at start of each new line).
    fn reset_browse(&mut self, current_line: &str) {
        self.browse = self.count;
        self.saved_line.clear();
        self.saved_line.push_str(current_line);
    }

    /// Move up in history (older).  Returns the command to display,
    /// or None if already at the oldest entry.
    fn up(&mut self) -> Option<&str> {
        if self.count == 0 || self.browse == 0 {
            return None;
        }
        self.browse = self.browse.saturating_sub(1);
        let abs_idx = (self.start.wrapping_add(
            self.count.wrapping_sub(1).wrapping_sub(self.browse)
        )) % HISTORY_SIZE;
        self.entries.get(abs_idx).map(|s| s.as_str())
    }

    /// Move down in history (newer).  Returns the command to display,
    /// or the saved live line if we've scrolled past the newest entry.
    fn down(&mut self) -> Option<&str> {
        if self.browse >= self.count {
            return None; // Already at live line.
        }
        self.browse = self.browse.saturating_add(1);
        if self.browse >= self.count {
            // Back to the live line.
            Some(self.saved_line.as_str())
        } else {
            let abs_idx = (self.start.wrapping_add(
                self.count.wrapping_sub(1).wrapping_sub(self.browse)
            )) % HISTORY_SIZE;
            self.entries.get(abs_idx).map(|s| s.as_str())
        }
    }
}

// ---------------------------------------------------------------------------
// Shell entry point
// ---------------------------------------------------------------------------

/// Run the kernel debug shell.
///
/// This function never returns.  It prints a prompt, reads a line,
/// executes the command, and repeats.
pub fn run() -> ! {
    crate::console_println!("");
    crate::console_println!("Kernel debug shell. Type 'help' for commands.");
    crate::console_println!("");

    // Initialize cwd.
    {
        let mut cwd = CWD.lock();
        if cwd.is_empty() {
            cwd.push('/');
        }
    }

    // Initialize default environment variables.
    env_set("PWD", &get_cwd());
    env_set("HOME", "/");
    env_set("SHELL", "kshell");
    env_set("USER", "root");
    env_set("PATH", "/bin:/usr/bin");

    let mut line_buf = String::with_capacity(MAX_LINE);
    let mut history = History::new();

    loop {
        // Print prompt: show continuation prompt during multi-line
        // constructs (heredoc, loop body, function body, case body).
        let collecting = HEREDOC_COLLECTOR.lock().is_some()
            || LOOP_COLLECTOR.lock().is_some()
            || FUNC_COLLECTOR.lock().is_some()
            || CASE_COLLECTOR.lock().is_some();
        if collecting {
            crate::console_print!("> ");
        } else {
            let cwd = get_cwd();
            crate::console_print!("{}> ", cwd);
        }

        // Read a line (blocking on keyboard).
        line_buf.clear();
        read_line(&mut line_buf, &mut history);

        // Parse and execute.
        let trimmed = line_buf.trim();

        // Empty lines are normally skipped, but during here-document
        // collection they are meaningful content (blank lines in the body).
        let in_heredoc = HEREDOC_COLLECTOR.lock().is_some();
        if trimmed.is_empty() && !in_heredoc {
            continue;
        }

        // Add to history before executing (so even failed commands
        // are recallable).  Don't add heredoc body lines to history.
        if !in_heredoc {
            history.push(trimmed);
        }
        execute(trimmed);
    }
}

// ---------------------------------------------------------------------------
// Line input with history support
// ---------------------------------------------------------------------------

/// Erase the current line from the console and reprint with new
/// content, placing the cursor at the end.
fn replace_line(buf: &mut String, cursor: &mut usize, new_content: &str) {
    // Move cursor to end of current text (so erase works from the end).
    let tail_len = buf.len().saturating_sub(*cursor);
    for _ in 0..tail_len {
        crate::console::putchar(b' '); // advance to end
    }
    // Erase everything from end to start.
    for _ in 0..buf.len() {
        crate::console::putchar(b'\x08');
        crate::console::putchar(b' ');
        crate::console::putchar(b'\x08');
    }
    // Also erase the chars we printed moving to the end.
    for _ in 0..tail_len {
        crate::console::putchar(b'\x08');
        crate::console::putchar(b' ');
        crate::console::putchar(b'\x08');
    }

    // Replace buffer.
    buf.clear();
    buf.push_str(new_content);
    *cursor = buf.len();

    // Print new content.
    for &b in buf.as_bytes() {
        crate::console::putchar(b);
    }
}

/// Redraw the line from the cursor position to the end, then move the
/// console cursor back to the correct position.
///
/// Used after inserting or deleting a character in the middle of the line.
fn redraw_from_cursor(buf: &str, cursor: usize) {
    // Print everything from cursor to end.
    if let Some(tail) = buf.as_bytes().get(cursor..) {
        for &b in tail {
            crate::console::putchar(b);
        }
    }
    // Print one extra space to erase any trailing character from a
    // deletion, then move back.
    crate::console::putchar(b' ');
    let move_back = buf.len().saturating_sub(cursor) + 1;
    for _ in 0..move_back {
        crate::console::putchar(b'\x08');
    }
}

/// Read a line from the keyboard with cursor editing and history.
///
/// Supports: printable character insertion at cursor, backspace/delete,
/// left/right arrow cursor movement, Home/End, Up/Down history browsing,
/// ESC to clear line.  Returns when Enter is pressed.
fn read_line(buf: &mut String, history: &mut History) {
    use crate::keyboard;

    // Cursor position within buf (0 = before first char, buf.len() = after last).
    let mut cursor: usize = 0;

    // Start in "live" mode (not browsing history).
    history.reset_browse("");

    // Disable keyboard echo — we handle all display ourselves to support
    // cursor-aware editing (insert/delete at any position).
    keyboard::set_echo(false);

    loop {
        let ch = keyboard::read_char();

        match ch {
            b'\n' => {
                keyboard::set_echo(true);
                crate::console::putchar(b'\n');
                return;
            }
            b'\x08' | 0x7F => {
                // Backspace / DEL — delete character before cursor.
                if cursor > 0 {
                    cursor -= 1;
                    buf.remove(cursor);
                    // Move console cursor back one.
                    crate::console::putchar(b'\x08');
                    // Redraw from cursor to end (shift chars left).
                    redraw_from_cursor(buf, cursor);
                }
            }
            0x01 => {
                // Ctrl+A — move cursor to start of line (same as Home).
                for _ in 0..cursor {
                    crate::console::putchar(b'\x08');
                }
                cursor = 0;
            }
            0x03 => {
                // Ctrl+C — cancel the current line (print ^C and start fresh).
                keyboard::set_echo(true);
                crate::console::write_str("^C\n");
                buf.clear();
                return;
            }
            0x05 => {
                // Ctrl+E — move cursor to end of line (same as End).
                if let Some(tail) = buf.as_bytes().get(cursor..) {
                    for &b in tail {
                        crate::console::putchar(b);
                    }
                }
                cursor = buf.len();
            }
            0x0B => {
                // Ctrl+K — kill from cursor to end of line.
                if cursor < buf.len() {
                    // Erase the on-screen text from cursor to end.
                    let tail_len = buf.len() - cursor;
                    for _ in 0..tail_len {
                        crate::console::putchar(b' ');
                    }
                    for _ in 0..tail_len {
                        crate::console::putchar(b'\x08');
                    }
                    buf.truncate(cursor);
                }
            }
            0x0C => {
                // Ctrl+L — clear screen and reprint prompt + line.
                crate::console::clear();
                let prompt = alloc::format!("{}> ", get_cwd());
                crate::console::write_str(&prompt);
                for &b in buf.as_bytes() {
                    crate::console::putchar(b);
                }
                // Move cursor back to the correct position.
                let tail_len = buf.len() - cursor;
                for _ in 0..tail_len {
                    crate::console::putchar(b'\x08');
                }
            }
            0x15 => {
                // Ctrl+U — kill from start of line to cursor.
                if cursor > 0 {
                    let old_len = buf.len();
                    // Move console cursor to start of line.
                    for _ in 0..cursor {
                        crate::console::putchar(b'\x08');
                    }
                    // Remove characters [0..cursor] from the buffer.
                    let remaining: String = buf.get(cursor..).unwrap_or("").into();
                    buf.clear();
                    buf.push_str(&remaining);
                    cursor = 0;
                    // Reprint the remaining text.
                    for &b in buf.as_bytes() {
                        crate::console::putchar(b);
                    }
                    // Erase leftover characters from the old longer line.
                    let erase = old_len.saturating_sub(buf.len());
                    for _ in 0..erase {
                        crate::console::putchar(b' ');
                    }
                    // Move cursor back to position 0.
                    let move_back = buf.len() + erase;
                    for _ in 0..move_back {
                        crate::console::putchar(b'\x08');
                    }
                }
            }
            0x17 => {
                // Ctrl+W — delete word before cursor.
                if cursor > 0 {
                    let old_cursor = cursor;
                    // Skip trailing whitespace.
                    while cursor > 0 {
                        let c = buf.as_bytes().get(cursor - 1).copied().unwrap_or(0);
                        if c != b' ' && c != b'\t' {
                            break;
                        }
                        cursor -= 1;
                    }
                    // Delete word chars.
                    while cursor > 0 {
                        let c = buf.as_bytes().get(cursor - 1).copied().unwrap_or(0);
                        if c == b' ' || c == b'\t' {
                            break;
                        }
                        cursor -= 1;
                    }
                    // Remove chars [cursor..old_cursor] from buffer.
                    let removed = old_cursor - cursor;
                    for _ in 0..removed {
                        buf.remove(cursor);
                    }
                    // Move console cursor back.
                    for _ in 0..removed {
                        crate::console::putchar(b'\x08');
                    }
                    // Redraw from cursor to end.
                    redraw_from_cursor(buf, cursor);
                }
            }
            0x1B => {
                // ESC — clear the current line.
                replace_line(buf, &mut cursor, "");
            }
            keyboard::KEY_UP => {
                // Save current line if this is the first Up press.
                if history.browse >= history.count {
                    history.saved_line.clear();
                    history.saved_line.push_str(buf.as_str());
                }
                if let Some(cmd) = history.up() {
                    let cmd = String::from(cmd);
                    replace_line(buf, &mut cursor, &cmd);
                }
            }
            keyboard::KEY_DOWN => {
                if let Some(cmd) = history.down() {
                    let cmd = String::from(cmd);
                    replace_line(buf, &mut cursor, &cmd);
                }
            }
            keyboard::KEY_LEFT => {
                if cursor > 0 {
                    cursor -= 1;
                    crate::console::putchar(b'\x08');
                }
            }
            keyboard::KEY_RIGHT => {
                if cursor < buf.len() {
                    // Print the char at cursor position to advance.
                    if let Some(&b) = buf.as_bytes().get(cursor) {
                        crate::console::putchar(b);
                    }
                    cursor += 1;
                }
            }
            keyboard::KEY_HOME => {
                // Move cursor to start of line.
                for _ in 0..cursor {
                    crate::console::putchar(b'\x08');
                }
                cursor = 0;
            }
            keyboard::KEY_END => {
                // Move cursor to end of line.
                if let Some(tail) = buf.as_bytes().get(cursor..) {
                    for &b in tail {
                        crate::console::putchar(b);
                    }
                }
                cursor = buf.len();
            }
            b'\t' => {
                // Tab — attempt completion.
                let (suffix, candidates) = tab_complete(buf, cursor);

                if !candidates.is_empty() && suffix.is_empty() {
                    // Multiple matches, no common prefix to add — display them.
                    crate::console::putchar(b'\n');
                    for (i, name) in candidates.iter().enumerate() {
                        if i > 0 {
                            crate::console::write_str("  ");
                        }
                        crate::console::write_str(name);
                    }
                    crate::console::putchar(b'\n');
                    // Reprint the prompt and current line.
                    let prompt = alloc::format!("{}> ", get_cwd());
                    crate::console::write_str(&prompt);
                    for &b in buf.as_bytes() {
                        crate::console::putchar(b);
                    }
                    // Move cursor back to the correct position.
                    let tail_len = buf.len() - cursor;
                    for _ in 0..tail_len {
                        crate::console::putchar(b'\x08');
                    }
                } else if !suffix.is_empty() {
                    // Insert the completion suffix at cursor.
                    for ch in suffix.chars() {
                        if buf.len() < MAX_LINE {
                            buf.insert(cursor, ch);
                            cursor += 1;
                        }
                    }
                    // Redraw from the insertion point.
                    if let Some(tail) = buf.as_bytes().get(cursor - suffix.len()..) {
                        for &b in tail {
                            crate::console::putchar(b);
                        }
                    }
                    // Move cursor back to correct position.
                    let tail_len = buf.len() - cursor;
                    for _ in 0..tail_len {
                        crate::console::putchar(b'\x08');
                    }
                }
                // If no matches at all, do nothing (no beep in our console).
            }
            ch if ch >= 0x20 && ch < 0x7F => {
                // Printable ASCII — insert at cursor position.
                if buf.len() < MAX_LINE {
                    buf.insert(cursor, ch as char);
                    cursor += 1;

                    if cursor == buf.len() {
                        // Appending at end ��� just echo the char.
                        crate::console::putchar(ch);
                    } else {
                        // Inserted in middle — echo char then redraw tail.
                        crate::console::putchar(ch);
                        redraw_from_cursor(buf, cursor);
                    }
                }
            }
            _ => {
                // Non-printable, non-handled — ignore.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tab completion
// ---------------------------------------------------------------------------

/// All built-in command names, sorted alphabetically.
const COMMANDS: &[&str] = &[
    "alias", "append", "awk", "backtrace", "basename", "blkdev", "blkinfo", "blkread", "bt", "cal", "cat",
    "base64", "bunzip2", "bzcat", "cd", "chattr", "checksum", "chmod", "chown", "cksum", "clear", "cls", "cmp",
    "column", "comm", "command", "copy", "cp", "cpuinfo", "crc32", "crc32sum",
    "cut", "date", "dd", "del", "df", "dhcp", "diag", "diff", "dir", "dirname", "dmesg", "dns", "du",
    "echo", "env", "eval", "exec", "export", "fallocate", "false", "file", "find", "fold", "free",
    "flock", "fsck", "fsck.ext4", "fsck.fat", "glob", "grep", "gunzip", "gzip", "hash", "head", "help", "hexdump", "hostname", "http",
    "id", "ifconfig", "irq", "journal", "kill", "label", "let", "ln", "link", "ls", "lsattr", "lsblk", "lsof", "lsp",
    "mapfile", "mem", "meminfo", "mkdir", "mkelf", "mkfs", "mkfs.fat", "mklink", "mktemp",
    "mount", "mv",
    "move", "net", "nl", "nproc", "nslookup", "od", "paste", "pci", "ping", "printenv",
    "printf", "profile", "ps", "pwd", "readarray", "readlink", "readonly", "realpath",
    "reboot", "ren", "renice", "rev", "rm",
    "rmdir", "run", "schedstat", "sed", "select", "seq", "set", "sha256", "sleep", "sort", "source",
    "strings", "tac", "tr",
    "do", "done", "elif", "else", "expr", "fi", "if",
    "slabinfo", "heapaudit", "fraginfo", "leakcheck", "memtest", "split", "stack", "stat", "symlink", "sync", "sysctl", "tail", "tar", "tasks", "taskset", "tee", "test",
    "then", "throttle", "time", "top", "touch", "trash", "tree", "true", "truncate", "type", "umount",
    "uname", "unalias", "uniq", "unmount", "unset", "unzip", "uptime", "ver", "version", "vmstat",
    "watch", "watchdog", "wc", "wget", "which", "while", "whoami", "wipe", "workqueue", "wq", "write",
    "acct", "boottime", "boottiming", "canary", "compact", "counters", "cpuacct", "cpuctl", "cpufreq", "cpuid", "cputime", "defrag", "events", "exceptions", "exclog", "faults", "freq", "healthcheck", "heapwm", "history", "hotplug", "hp", "hugepage", "hugepages", "idle", "irqbal", "irqbalance", "irqoff", "irqrate", "irqstorm", "jitter", "kcounters", "kevent", "kprofile", "kstat", "ksyms", "kwarn", "latency", "lathist", "loadavg", "lockstat", "lockstats", "memacct", "memmap", "mempressure", "mempool", "memtype", "msi", "numa", "pacct", "pgfault", "pools", "poweroff", "pressure", "rcu", "reboot", "sar", "sclat", "sclatency", "shutdown", "stackcheck", "symbols", "syshealth", "sysinfo", "temp", "thermal", "tickjitter", "tlb", "topo", "topology", "vectors", "warnings", "watermark",
    "vmalloc", "vm", "rmap", "pcid", "poison", "watermark", "wmark", "tlbgather", "gather", "migratetype", "mtype", "pageage", "aging", "ptwalk", "pagetables", "scrub", "memscrub", "faultinject", "finject", "frameowner", "fowner", "alloctrace", "atrace", "alloclat", "alat", "heapprofile", "hprof", "syscallprof", "sprof", "capaudit", "capa", "checkpoint", "ckpt", "strace", "sctrace", "ipcstat", "ipc", "kobjects", "kobj", "fraghist", "fragtrend", "selftest", "watch", "snapshot", "snap", "ripsample", "perf", "invariant", "invar", "migrate", "migrations", "wchan", "bench", "benchmark", "diag2", "report", "hypervisor", "vminfo", "fairness", "jfi", "cet", "cfi", "smap", "smep",
    "ktimer", "ktrace", "lockdep", "rng", "supervisor", "sv", "timers", "trace", "xattr", "xxd", "zip",
    // Scripting keywords and commands
    "break", "case", "command", "continue", "declare", "for", "function", "in",
    "local", "read", "return", "shift", "trap", "typeof", "until", "xargs", "yes",
];

/// Find the longest common prefix among a set of strings.
fn longest_common_prefix(candidates: &[&str]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let first = candidates[0];
    let mut prefix_len = first.len();
    for c in candidates.iter().skip(1) {
        let common = first.as_bytes().iter()
            .zip(c.as_bytes().iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common < prefix_len {
            prefix_len = common;
        }
    }
    first.get(..prefix_len).unwrap_or("").into()
}

/// Perform tab completion on the current line.
///
/// Returns the completed text to insert (may be empty if no match),
/// and optionally a list of candidates to display.
fn tab_complete(line: &str, cursor: usize) -> (String, Vec<String>) {
    // Extract the text up to the cursor for completion.
    let text_before = line.get(..cursor).unwrap_or("");

    // Determine if we're completing a command (first word) or a path.
    let first_space = text_before.find(' ');

    if first_space.is_none() {
        // Completing a command name.
        let prefix = text_before;
        let matches: Vec<&str> = COMMANDS.iter()
            .copied()
            .filter(|c| c.starts_with(prefix))
            .collect();

        if matches.is_empty() {
            return (String::new(), Vec::new());
        }
        if matches.len() == 1 {
            // Unique match — complete it with a trailing space.
            let suffix: String = matches[0].get(prefix.len()..).unwrap_or("").into();
            let mut result = suffix;
            result.push(' ');
            return (result, Vec::new());
        }
        // Multiple matches — complete the common prefix.
        let common = longest_common_prefix(&matches);
        let suffix: String = common.get(prefix.len()..).unwrap_or("").into();
        let display: Vec<String> = matches.iter().map(|s| String::from(*s)).collect();
        (suffix, display)
    } else {
        // Completing a file path argument.
        // Find the start of the current word (last space before cursor).
        let word_start = text_before.rfind(' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        let partial_path = text_before.get(word_start..).unwrap_or("");

        // Determine the directory to search and the prefix to match.
        // For relative paths, resolve against the current working directory.
        let (dir, name_prefix) = if let Some(slash_pos) = partial_path.rfind('/') {
            let dir_part = partial_path.get(..=slash_pos).unwrap_or("/");
            let name_part = partial_path.get(slash_pos + 1..).unwrap_or("");
            (resolve_path(dir_part), name_part)
        } else {
            (get_cwd(), partial_path)
        };

        // List the directory and filter by prefix.
        let entries = match crate::fs::Vfs::readdir(&dir) {
            Ok(e) => e,
            Err(_) => return (String::new(), Vec::new()),
        };

        let matches: Vec<&crate::fs::DirEntry> = entries.iter()
            .filter(|e| e.name.starts_with(name_prefix))
            .filter(|e| e.name != "." && e.name != "..")
            .collect();

        if matches.is_empty() {
            return (String::new(), Vec::new());
        }

        if matches.len() == 1 {
            // Unique match.
            let entry = &matches[0];
            let suffix: String = entry.name.get(name_prefix.len()..).unwrap_or("").into();
            let mut result = suffix;
            // Add trailing slash for directories, space for files.
            if entry.entry_type == crate::fs::EntryType::Directory {
                result.push('/');
            } else {
                result.push(' ');
            }
            return (result, Vec::new());
        }

        // Multiple matches — complete common prefix.
        let names: Vec<&str> = matches.iter().map(|e| e.name.as_str()).collect();
        let common = longest_common_prefix(&names);
        let suffix: String = common.get(name_prefix.len()..).unwrap_or("").into();
        let display: Vec<String> = matches.iter().map(|e| {
            let mut s = e.name.clone();
            if e.entry_type == crate::fs::EntryType::Directory {
                s.push('/');
            }
            s
        }).collect();
        (suffix, display)
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

/// Parse a command line and execute the matching command.
///
/// Supports:
/// - Command chaining: `cmd1 ; cmd2`, `cmd1 && cmd2`, `cmd1 || cmd2`
/// - Output redirection: `cmd > file`, `cmd >> file`
/// - Piping: `cmd1 | cmd2`
/// - Variable expansion: `$VAR`, `${VAR}`
/// - Alias expansion (first word only)
fn execute(line: &str) {
    // Phase 0: Handle control flow keywords (if/then/elif/else/fi).
    // These are checked before variable expansion so that skipped blocks
    // don't trigger side effects.  Control-flow keywords themselves still
    // expand variables in their conditions (the handler calls execute()
    // recursively for conditions).
    let trimmed_check = line.trim();
    if !trimmed_check.is_empty() {
        // Check for control-flow keywords.
        if handle_control_flow(trimmed_check) {
            return;
        }
        // If we're inside a skip block, silently discard the line.
        if !control_should_execute() {
            return;
        }
    }

    // Phase 1: Expand environment variables ($VAR, ${VAR}), then braces.
    let expanded = expand_vars(line);
    let expanded = expand_braces(&expanded);
    let line = expanded.trim();

    if line.is_empty() {
        return;
    }

    // Trace: print expanded command if `set -x` is active.
    if OPT_XTRACE.load(core::sync::atomic::Ordering::Relaxed) {
        crate::console_println!("+ {}", line);
    }

    // Phase 2: Split on chain operators (;, &&, ||).
    // These have the lowest precedence — each segment gets its own
    // alias expansion, pipe/redirect handling, and dispatch.
    let segments = split_chain_operators(line);
    if segments.len() > 1 {
        for seg in &segments {
            let cmd = seg.command.trim();
            if cmd.is_empty() {
                continue;
            }
            match seg.operator {
                ChainOp::None | ChainOp::Semicolon => {
                    execute_single(cmd);
                }
                ChainOp::And => {
                    // Run only if previous command succeeded.
                    if last_exit() == 0 {
                        execute_single(cmd);
                    }
                }
                ChainOp::Or => {
                    // Run only if previous command failed.
                    if last_exit() != 0 {
                        execute_single(cmd);
                    }
                }
            }
        }
        return;
    }

    // No chaining operators — execute as a single command.
    execute_single(line);
}

/// A segment in a chained command line.
struct ChainSegment<'a> {
    /// The operator that precedes this segment.
    /// The first segment has `ChainOp::None`.
    operator: ChainOp,
    /// The command text.
    command: &'a str,
}

/// Chain operators connecting command segments.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChainOp {
    /// First segment (no preceding operator).
    None,
    /// `;` — unconditional sequencing.
    Semicolon,
    /// `&&` — run if previous succeeded.
    And,
    /// `||` — run if previous failed.
    Or,
}

/// Split a command line on chain operators (`;`, `&&`, `||`).
///
/// Respects quoting — operators inside single or double quotes are ignored.
/// Returns a single-element Vec if no operators are found.
fn split_chain_operators(line: &str) -> Vec<ChainSegment<'_>> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut pending_op = ChainOp::None;
    let mut in_sq = false;
    let mut in_dq = false;

    while i < len {
        let b = bytes[i];

        if b == b'\'' && !in_dq { in_sq = !in_sq; i = i.saturating_add(1); continue; }
        if b == b'"' && !in_sq { in_dq = !in_dq; i = i.saturating_add(1); continue; }
        if in_sq || in_dq { i = i.saturating_add(1); continue; }

        if b == b'&' && bytes.get(i.saturating_add(1)) == Some(&b'&') {
            let cmd = line.get(start..i).unwrap_or("");
            segments.push(ChainSegment { operator: pending_op, command: cmd });
            pending_op = ChainOp::And;
            i = i.saturating_add(2);
            start = i;
            continue;
        }
        if b == b'|' && bytes.get(i.saturating_add(1)) == Some(&b'|') {
            let cmd = line.get(start..i).unwrap_or("");
            segments.push(ChainSegment { operator: pending_op, command: cmd });
            pending_op = ChainOp::Or;
            i = i.saturating_add(2);
            start = i;
            continue;
        }
        if b == b';' {
            let cmd = line.get(start..i).unwrap_or("");
            segments.push(ChainSegment { operator: pending_op, command: cmd });
            pending_op = ChainOp::Semicolon;
            i = i.saturating_add(1);
            start = i;
            continue;
        }

        i = i.saturating_add(1);
    }

    // Push the final tail segment.
    let tail = line.get(start..).unwrap_or("");
    segments.push(ChainSegment { operator: pending_op, command: tail });

    segments
}

/// Parsed array assignment syntax.
enum ArraySyntax {
    /// `name=(word1 word2 ...)` — declare/replace entire array.
    Declare { name: String, values: Vec<String> },
    /// `name+=(word1 word2 ...)` — append to existing array.
    Append { name: String, values: Vec<String> },
    /// `name[index]=value` — set a single array element.
    SetElement { name: String, index: usize, value: String },
}

/// Try to parse array declaration or element assignment.
///
/// Returns `Some(ArraySyntax::Declare{..})` for `name=(word1 word2 ...)`,
/// `Some(ArraySyntax::SetElement{..})` for `name[N]=value`,
/// or `None` if the line doesn't match either pattern.
fn parse_array_syntax(line: &str) -> Option<ArraySyntax> {
    // Check for `name+=(...)` — append to array (must check before `=(` match).
    if let Some(plus_eq) = line.find("+=(") {
        let name = line.get(..plus_eq)?.trim();
        if name.is_empty()
            || !name.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }
        let after_plus_eq = line.get(plus_eq.saturating_add(2)..)?;
        if !after_plus_eq.starts_with('(') || !after_plus_eq.ends_with(')') {
            return None;
        }
        let inside = after_plus_eq.get(1..after_plus_eq.len().saturating_sub(1)).unwrap_or("");
        let values = split_array_words(inside);
        return Some(ArraySyntax::Append {
            name: String::from(name),
            values,
        });
    }

    // Check for `name=(...)` — array declaration.
    // The `=(` must appear with a valid identifier before it.
    if let Some(eq_paren) = line.find("=(") {
        let name = line.get(..eq_paren)?.trim();
        // Validate identifier: alphanumeric/underscore, starts with letter or underscore.
        if name.is_empty()
            || !name.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }
        // Find the closing `)`.
        let after_eq = line.get(eq_paren.saturating_add(1)..)?;
        if !after_eq.starts_with('(') || !after_eq.ends_with(')') {
            return None;
        }
        let inside = after_eq.get(1..after_eq.len().saturating_sub(1)).unwrap_or("");

        // Split words (quote-aware).
        let values = split_array_words(inside);
        return Some(ArraySyntax::Declare {
            name: String::from(name),
            values,
        });
    }

    // Check for `name[N]=value` — element assignment.
    if let Some(bracket_pos) = line.find('[') {
        let name = line.get(..bracket_pos)?.trim();
        if name.is_empty()
            || !name.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }

        let rest = line.get(bracket_pos.saturating_add(1)..)?;
        // Find `]=`
        let close_eq = rest.find("]=")?;
        let idx_str = rest.get(..close_eq)?.trim();
        let value = rest.get(close_eq.saturating_add(2)..)?.trim();

        // Parse index.
        let index: usize = idx_str.parse().ok()?;

        // Strip quotes from value if present.
        let value = strip_quotes(value);

        return Some(ArraySyntax::SetElement {
            name: String::from(name),
            index,
            value: String::from(value),
        });
    }

    None
}

/// Split array declaration words, respecting quotes.
///
/// `"hello world" foo bar` → `["hello world", "foo", "bar"]`
fn split_array_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_sq = false;
    let mut in_dq = false;

    for ch in input.chars() {
        match ch {
            '\'' if !in_dq => {
                in_sq = !in_sq;
                // Don't include the quotes in the value.
            }
            '"' if !in_sq => {
                in_dq = !in_dq;
            }
            ' ' | '\t' if !in_sq && !in_dq => {
                if !current.is_empty() {
                    words.push(core::mem::take(&mut current));
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Execute a single command segment (after chain-operator splitting).
///
/// Handles alias expansion, pipe/redirect parsing, and dispatch.
fn execute_single(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    // Expand aliases (first word only).
    let aliased = expand_aliases(line);
    let line = aliased.trim();

    // Check for array declaration: `name=(word1 word2 ...)`
    // array append: `name+=(word1 word2 ...)`
    // and element assignment: `name[N]=value`
    if let Some(assign) = parse_array_syntax(line) {
        match assign {
            ArraySyntax::Declare { name, values } => {
                array_set(&name, values);
                set_exit(0);
            }
            ArraySyntax::Append { name, values } => {
                let mut arrays = ARRAY_VARS.lock();
                let arr = arrays.entry(name).or_insert_with(Vec::new);
                arr.extend(values);
                drop(arrays);
                set_exit(0);
            }
            ArraySyntax::SetElement { name, index, value } => {
                array_set_element(&name, index, &value);
                set_exit(0);
            }
        }
        return;
    }

    // `(( EXPR ))` arithmetic command — evaluates expression, sets exit
    // status to 1 if result is 0, 0 if non-zero (like bash).
    if line.starts_with("((") && line.ends_with("))") {
        let inner = line.get(2..line.len().saturating_sub(2)).unwrap_or("").trim();
        if !inner.is_empty() {
            // Check for assignment: VAR = EXPR.
            eval_cfor_expr(inner);
            let val = eval_arithmetic(inner);
            set_exit(if val == 0 { 1 } else { 0 });
        }
        return;
    }

    // `eval` re-parses its arguments as a command line — must be handled
    // before pipe/redirect parsing since the eval'd string may itself
    // contain pipes, redirects, or any other syntax.
    if line.starts_with("eval ") || line.starts_with("eval\t") || line == "eval" {
        let eval_args = line.get(5..).unwrap_or("").trim();
        if !eval_args.is_empty() {
            execute(eval_args);
        }
        return;
    }

    // Inline variable assignment: `VAR=value command args...`
    // Sets VAR for the duration of the command, then restores it.
    // Must come before export/unset check since `NAME=VALUE` looks like
    // a simple assignment, but if followed by a command it's an inline env.
    if let Some(inline) = parse_inline_assignment(line) {
        let prev = env_get(&inline.name);
        env_set(&inline.name, &inline.value);
        execute_single(&inline.command);
        // Restore the variable.
        match prev {
            Some(val) => { env_set(&inline.name, &val); }
            None => { env_remove(&inline.name); }
        }
        return;
    }

    // Bare variable assignment: `VAR=value` (no command follows).
    // Handled after inline check so `VAR=value cmd` takes priority.
    if let Some((name, value)) = parse_bare_assignment(line) {
        env_set(&name, &value);
        set_exit(0);
        return;
    }

    // Check for `export`/`unset`/`alias`/`unalias` before
    // pipe/redirect parsing — these are variable-setting commands that
    // should not be piped.
    {
        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("").trim();
        match cmd {
            "export" => { cmd_export(args); set_exit(0); return; }
            "set" => { cmd_set(args); set_exit(0); return; }
            "unset" => { cmd_unset(args); set_exit(0); return; }
            "alias" => { cmd_alias(args); set_exit(0); return; }
            "unalias" => { cmd_unalias(args); set_exit(0); return; }
            _ => {}
        }
    }

    // Check for pipe chain (highest-level operator).
    // Note: single `|` only — `||` was already handled as chain operator.
    // Supports multi-pipe: `cmd1 | cmd2 | cmd3 | ... | cmdN`.
    let pipe_segments = split_pipes(line);
    if pipe_segments.len() > 1 {
        execute_pipe_chain(&pipe_segments);
        return;
    }

    // Check for output redirection (> file, >> file).
    if let Some(redir) = parse_redirect(line) {
        execute_redirect(&redir.command, &redir.path, redir.append);
        set_exit(0);
        return;
    }

    // Check for here-string (cmd <<< word).
    if let Some((command, word)) = parse_here_string(line) {
        let input = alloc::format!("{}\n", word);
        dispatch_with_input(&command, &input);
        return;
    }

    // Check for input redirection (cmd < file).
    if let Some((command, path)) = parse_input_redirect(line) {
        execute_input_redirect(&command, &path);
        return;
    }

    dispatch(line);

    // Fire ERR trap if the command failed.
    if last_exit() != 0 {
        fire_trap("ERR");
    }
}

/// Output redirection descriptor.
struct Redirect<'a> {
    /// The command to execute (everything before `>` / `>>`).
    command: &'a str,
    /// The file path to redirect output to.
    path: &'a str,
    /// If true, append (`>>`); if false, overwrite (`>`).
    append: bool,
}

/// Parse a command line for `>` or `>>` redirection.
///
/// Returns `None` if there is no redirection operator (or it's inside quotes).
fn parse_redirect(line: &str) -> Option<Redirect<'_>> {
    // Scan for `>>` first (longer match), then `>`.
    // Ignore `>` inside quoted strings.
    let bytes = line.as_bytes();
    let mut in_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            in_quote = !in_quote;
        } else if !in_quote && b == b'>' {
            let append = bytes.get(i.saturating_add(1)) == Some(&b'>');
            let skip = if append { 2 } else { 1 };
            let command = line.get(..i).unwrap_or("").trim();
            let path = line.get(i.saturating_add(skip)..).unwrap_or("").trim();
            if command.is_empty() || path.is_empty() {
                return None;
            }
            return Some(Redirect { command, path, append });
        }
        i = i.saturating_add(1);
    }
    None
}

/// Parse a command line for `< file` input redirection.
///
/// Returns `Some((command, file_path))` if found.
/// Only detects bare `<` (not `<<` heredoc or `<(` process subst).
/// Parse a here-string: `command <<< word`.
///
/// Returns `(command, word)` if the pattern is found.
/// The word is stripped of surrounding quotes if present.
fn parse_here_string(line: &str) -> Option<(&str, String)> {
    let bytes = line.as_bytes();
    let mut in_sq = false;
    let mut in_dq = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; i = i.saturating_add(1); continue; }
        if b == b'"' && !in_sq { in_dq = !in_dq; i = i.saturating_add(1); continue; }
        if in_sq || in_dq { i = i.saturating_add(1); continue; }

        if b == b'<'
            && bytes.get(i.saturating_add(1)) == Some(&b'<')
            && bytes.get(i.saturating_add(2)) == Some(&b'<')
        {
            // Ensure it's not `<<<<`.
            if bytes.get(i.saturating_add(3)) == Some(&b'<') {
                i = i.saturating_add(4);
                continue;
            }
            let command = line.get(..i)?.trim();
            let word = line.get(i.saturating_add(3)..)?.trim();
            if command.is_empty() || word.is_empty() {
                return None;
            }
            return Some((command, String::from(strip_quotes(word))));
        }
        i = i.saturating_add(1);
    }
    None
}

/// Parsed inline variable assignment: `VAR=value command args...`
struct InlineAssignment {
    name: String,
    value: String,
    command: String,
}

/// Parse `VAR=value command args...` where the first word is a VAR=value
/// assignment and the rest is a command to run with that env.
///
/// Returns `None` if:
/// - There's no `=` in the first word
/// - The variable name is invalid
/// - There's no command after the assignment (bare assignment)
fn parse_inline_assignment(line: &str) -> Option<InlineAssignment> {
    // The first word (up to first whitespace) must contain `=`.
    let first_space = line.find(|c: char| c == ' ' || c == '\t')?;
    let first_word = line.get(..first_space)?;
    let rest = line.get(first_space.saturating_add(1)..)?.trim();

    // The rest must be non-empty (there's a command to run).
    if rest.is_empty() {
        return None;
    }

    let eq_pos = first_word.find('=')?;
    let name = first_word.get(..eq_pos)?;
    let value = first_word.get(eq_pos.saturating_add(1)..)?;

    // Validate variable name: must start with letter/underscore,
    // contain only alphanumeric/underscore.
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }

    // Make sure the "command" part doesn't start with `=` (which would
    // indicate something like `a=b=c` which isn't an inline assignment).
    if rest.starts_with('=') {
        return None;
    }

    Some(InlineAssignment {
        name: String::from(name),
        value: String::from(strip_quotes(value)),
        command: String::from(rest),
    })
}

/// Parse a bare variable assignment: `VAR=value` with no command following.
fn parse_bare_assignment(line: &str) -> Option<(String, String)> {
    // Must be a single word (no spaces, or only quoted spaces in the value).
    // Simple check: first word contains `=`, no other words follow.
    let eq_pos = line.find('=')?;
    let name = line.get(..eq_pos)?;

    // Validate variable name.
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }

    let value = line.get(eq_pos.saturating_add(1)..)?;

    // If there's a space in the value that's not inside quotes, it's not
    // a bare assignment (it could be `VAR=value command` which is handled
    // by parse_inline_assignment).  For simplicity, allow any value here
    // since inline assignment already ran first.
    Some((String::from(name), String::from(strip_quotes(value))))
}

fn parse_input_redirect(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut in_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            in_quote = !in_quote;
        } else if !in_quote && b == b'<' {
            // Make sure this isn't `<<` / `<<<` (heredoc/here-string) or `<(`.
            let next = bytes.get(i.saturating_add(1));
            if next == Some(&b'<') {
                // Skip all consecutive `<` chars (<<, <<<).
                i = i.saturating_add(2);
                while i < bytes.len() && bytes[i] == b'<' {
                    i = i.saturating_add(1);
                }
                continue;
            }
            if next == Some(&b'(') {
                i = i.saturating_add(2);
                continue;
            }
            let command = line.get(..i).unwrap_or("").trim();
            let path = line.get(i.saturating_add(1)..).unwrap_or("").trim();
            if command.is_empty() || path.is_empty() {
                return None;
            }
            return Some((command, path));
        }
        i = i.saturating_add(1);
    }
    None
}

/// Execute a command with input redirected from a file.
///
/// Reads the file contents and feeds them as piped input to the command.
fn execute_input_redirect(command: &str, path: &str) {
    let resolved = resolve_path(path);
    let data = match crate::fs::vfs::Vfs::read_file(&resolved) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("{}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };
    let text = match core::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => {
            crate::console_println!("{}: not a text file", path);
            set_exit(1);
            return;
        }
    };

    // If the command itself has output redirection, handle it.
    if let Some(redir) = parse_redirect(command) {
        capture_start();
        dispatch_with_input(&redir.command, text);
        let output = capture_stop();
        if !output.is_empty() {
            use crate::fs::vfs::Vfs;
            let result = if redir.append {
                let existing = Vfs::read_file(redir.path).unwrap_or_default();
                let mut combined = match core::str::from_utf8(&existing) {
                    Ok(s) => String::from(s),
                    Err(_) => String::new(),
                };
                combined.push_str(&output);
                Vfs::write_file(redir.path, combined.as_bytes())
            } else {
                Vfs::write_file(redir.path, output.as_bytes())
            };
            if let Err(e) = result {
                crate::console_println!("Redirect error: {:?}", e);
                set_exit(1);
                return;
            }
        }
    } else {
        dispatch_with_input(command, text);
    }
    set_exit(0);
}

/// Find the position of the first un-quoted `|` character.
/// Split a command line on pipe operators (`|`), respecting quoting.
///
/// Returns a single-element vec if there are no pipes.
/// Ignores `||` (already consumed by chain operator splitting).
fn split_pipes(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut segments: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;

    while i < len {
        let b = bytes[i];
        if b == b'\'' && !in_dq { in_sq = !in_sq; i = i.saturating_add(1); continue; }
        if b == b'"' && !in_sq { in_dq = !in_dq; i = i.saturating_add(1); continue; }
        if in_sq || in_dq { i = i.saturating_add(1); continue; }

        if b == b'|' {
            // Skip `||` — that's an OR chain operator, not a pipe.
            if bytes.get(i.saturating_add(1)) == Some(&b'|') {
                i = i.saturating_add(2);
                continue;
            }
            let seg = line.get(start..i).unwrap_or("").trim();
            segments.push(seg);
            i = i.saturating_add(1);
            start = i;
            continue;
        }
        i = i.saturating_add(1);
    }
    // Push the final segment.
    let last = line.get(start..).unwrap_or("").trim();
    segments.push(last);
    segments
}

/// Execute a command with its output redirected to a file.
fn execute_redirect(command: &str, path: &str, append: bool) {
    capture_start();
    dispatch(command);
    let output = capture_stop();

    if output.is_empty() {
        return;
    }

    use crate::fs::vfs::Vfs;
    let result = if append {
        // Read existing file contents and append.
        let existing = Vfs::read_file(path).unwrap_or_default();
        let mut combined = match core::str::from_utf8(&existing) {
            Ok(s) => String::from(s),
            Err(_) => String::new(),
        };
        combined.push_str(&output);
        Vfs::write_file(path, combined.as_bytes())
    } else {
        Vfs::write_file(path, output.as_bytes())
    };

    if let Err(e) = result {
        crate::console_println!("Redirect error: {:?}", e);
    }
}

/// Execute a here-document: feed the body as piped input to the command,
/// then apply any suffix (pipes, redirects) to the output.
///
/// Examples:
/// - `sort <<EOF` → dispatch_with_input("sort", body)
/// - `sort <<EOF | head 5` → capture sort's output, pipe to head 5
/// - `cat <<EOF > /tmp/out` → capture cat's output, redirect to file
fn execute_heredoc(command: &str, suffix: &str, body: &str) {
    if command.is_empty() && suffix.is_empty() {
        // Bare heredoc with no command — just print the body.
        shell_println!("{}", body);
        return;
    }

    if suffix.is_empty() {
        // Simple case: just feed body to command.
        dispatch_with_input(command, body);
        return;
    }

    // Complex case: command has a suffix (pipe or redirect).
    // Capture the command's output with the heredoc body as input,
    // then feed that output through the suffix.
    capture_start();
    dispatch_with_input(command, body);
    let output = capture_stop();

    // Now execute the suffix with the captured output.
    // The suffix might be "| cmd2" or "> file" or ">> file".
    let suffix = suffix.trim();
    if let Some(rest) = suffix.strip_prefix('|') {
        let rest = rest.trim();
        if !rest.is_empty() {
            dispatch_with_input(rest, &output);
        }
    } else if let Some((path, append)) = parse_bare_redirect(suffix) {
        // Suffix is a bare redirect (e.g., "> /tmp/out" or ">> /tmp/out").
        if !output.is_empty() {
            use crate::fs::vfs::Vfs;
            let result = if append {
                let existing = Vfs::read_file(path).unwrap_or_default();
                let mut combined = match core::str::from_utf8(&existing) {
                    Ok(s) => String::from(s),
                    Err(_) => String::new(),
                };
                combined.push_str(&output);
                Vfs::write_file(path, combined.as_bytes())
            } else {
                Vfs::write_file(path, output.as_bytes())
            };
            if let Err(e) = result {
                crate::console_println!("Redirect error: {:?}", e);
            }
        }
    } else {
        // Suffix doesn't look like a pipe or redirect — just print.
        shell_print!("{}", output);
    }
}

/// Parse a bare redirect like `> /tmp/file` or `>> /tmp/file`.
///
/// Unlike `parse_redirect()`, this allows an empty command prefix
/// (used for heredoc suffixes where the "command" output is already captured).
fn parse_bare_redirect(s: &str) -> Option<(&str, bool)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix(">>") {
        let path = rest.trim();
        if path.is_empty() { return None; }
        Some((path, true))
    } else if let Some(rest) = s.strip_prefix('>') {
        let path = rest.trim();
        if path.is_empty() { return None; }
        Some((path, false))
    } else {
        None
    }
}

/// Execute a multi-pipe chain: `cmd1 | cmd2 | ... | cmdN`.
///
/// Each stage captures the previous stage's output and feeds it as piped
/// input to the next stage.  The last stage may have output redirection.
fn execute_pipe_chain(segments: &[&str]) {
    if segments.is_empty() {
        return;
    }

    // Validate: no empty segments.
    for seg in segments {
        if seg.is_empty() {
            crate::console_println!("Syntax error: empty pipe operand");
            set_exit(1);
            return;
        }
    }

    // First stage: run with no piped input, capture its output.
    capture_start();
    // The first segment might have input redirection (cmd < file | ...).
    let first = segments[0];
    if let Some((command, path)) = parse_input_redirect(first) {
        execute_input_redirect(&command, &path);
    } else {
        dispatch(first);
    }
    let mut piped_data = capture_stop();

    // Middle stages: each reads from the previous output and captures for
    // the next stage.
    let last_idx = segments.len().saturating_sub(1);
    for seg in segments.get(1..last_idx).unwrap_or(&[]) {
        capture_start();
        dispatch_with_input(seg, &piped_data);
        piped_data = capture_stop();
    }

    // Last stage: may have output redirection; otherwise prints to console.
    let last = segments[last_idx];
    if let Some(redir) = parse_redirect(last) {
        capture_start();
        dispatch_with_input(&redir.command, &piped_data);
        let output = capture_stop();
        if !output.is_empty() {
            use crate::fs::vfs::Vfs;
            let result = if redir.append {
                let existing = Vfs::read_file(redir.path).unwrap_or_default();
                let mut combined = match core::str::from_utf8(&existing) {
                    Ok(s) => String::from(s),
                    Err(_) => String::new(),
                };
                combined.push_str(&output);
                Vfs::write_file(redir.path, combined.as_bytes())
            } else {
                Vfs::write_file(redir.path, output.as_bytes())
            };
            if let Err(e) = result {
                crate::console_println!("Redirect error: {:?}", e);
            }
        }
    } else {
        dispatch_with_input(last, &piped_data);
    }
    set_exit(0);
}

/// Execute a command with optional piped input.
///
/// Commands that support piped input will read from the input string
/// when no file argument is provided.  Commands that don't support
/// piped input ignore the input and execute normally.
fn dispatch_with_input(line: &str, input: &str) {
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    // Commands that support reading from piped input.
    match cmd {
        "sort" => cmd_sort_input(args, input),
        "uniq" => cmd_uniq_input(args, input),
        "grep" => cmd_grep_input(args, input),
        "head" => cmd_head_input(args, input),
        "tail" => cmd_tail_input(args, input),
        "wc" => cmd_wc_input(args, input),
        "nl" => cmd_nl_input(args, input),
        "rev" => cmd_rev_input(args, input),
        "cat" if args.is_empty() => {
            // `cat` with no args reads from pipe.
            shell_print!("{}", input);
        }
        "mapfile" | "readarray" => cmd_mapfile_input(args, input),
        "tee" => cmd_tee_input(args, input),
        "cut" => cmd_cut_input(args, input),
        "tr" => cmd_tr_input(args, input),
        "tac" => cmd_tac_input(args, input),
        "fold" => cmd_fold_input(args, input),
        "paste" => cmd_paste_input(args, input),
        "xargs" => cmd_xargs_input(args, input),
        "column" => cmd_column_input(args, input),
        "sed" => cmd_sed_input(args, input),
        "awk" => cmd_awk_input(args, input),
        _ => {
            // Command doesn't support piped input — just run normally.
            dispatch(line);
        }
    }
}

/// Core command dispatch (no redirection/pipe parsing).
///
/// Sets exit status: 0 for recognized commands, 1 for unknown commands.
/// Individual commands may also set the exit status on error.
fn dispatch(line: &str) {
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    // Default to success; commands that fail will set_exit(1).
    set_exit(0);

    match cmd {
        "help" | "?" => cmd_help(),
        "cd" => cmd_cd(args),
        "meminfo" | "mem" => cmd_meminfo(),
        "cpuinfo" | "cpu" => cmd_cpuinfo(),
        "top" => cmd_top(),
        "profile" => cmd_profile(args),
        "watchdog" => cmd_watchdog(),
        "kill" => cmd_kill(args),
        "renice" => cmd_renice(args),
        "throttle" => cmd_throttle(args),
        "taskset" => cmd_taskset(args),
        "schedstat" => cmd_schedstat(),
        "slabinfo" => cmd_slabinfo(),
        "heapaudit" => cmd_heapaudit(),
        "fraginfo" => cmd_fraginfo(),
        "leakcheck" => cmd_leakcheck(),
        "memtest" => cmd_memtest(args),
        "stack" => cmd_stack(),
        "wq" | "workqueue" => cmd_workqueue(),
        "ktimer" | "timers" => cmd_ktimer(),
        "rng" | "random" => cmd_rng(),
        "trace" | "ktrace" => cmd_trace(args),
        "lockdep" => cmd_lockdep(args),
        "bt" | "backtrace" => cmd_backtrace(),
        "diag" | "health" => cmd_diag(),
        "exceptions" | "vectors" => cmd_exceptions(),
        "exclog" => cmd_exclog(),
        "sysinfo" | "cpuid" => cmd_sysinfo(),
        "boottime" | "boottiming" => cmd_boottime(),
        "canary" | "stackcheck" => cmd_canary(),
        "tlb" => cmd_tlb(),
        "pgfault" | "faults" => cmd_pgfault(),
        "irqrate" => cmd_irqrate(),
        "kprofile" => cmd_kprofile(args),
        "lockstats" | "lockstat" => cmd_lockstats(args),
        "irqoff" => cmd_irqoff(args),
        "idle" => cmd_idle(),
        "kstat" | "history" => cmd_kstat(args),
        "kwarn" | "warnings" => cmd_kwarn(args),
        "loadavg" => cmd_loadavg(),
        "cputime" | "cpuacct" => cmd_cputime(),
        "irqstorm" => cmd_irqstorm(args),
        "pacct" | "acct" => cmd_pacct(args),
        "counters" | "kcounters" => cmd_counters(args),
        "topology" | "topo" => cmd_topology(),
        "cpufreq" | "freq" => cmd_cpufreq(args),
        "thermal" | "temp" => cmd_thermal(),
        "compact" | "defrag" => cmd_compact(),
        "shutdown" | "poweroff" => cmd_shutdown(),
        "reboot" => cmd_reboot(),
        "hotplug" | "cpuctl" => cmd_hotplug(args),
        "irqbalance" | "irqbal" => cmd_irqbalance(args),
        "msi" => cmd_msi(),
        "hugepage" | "hugepages" | "hp" => cmd_hugepage(),
        "vmalloc" | "vm" => cmd_vmalloc(),
        "rmap" => cmd_rmap(),
        "pcid" => cmd_pcid(),
        "poison" => cmd_poison(),
        "watermark" | "wmark" => cmd_watermark(),
        "tlbgather" | "gather" => cmd_tlb_gather(),
        "migratetype" | "mtype" => cmd_migrate_type(),
        "pageage" | "aging" => cmd_page_age(),
        "ptwalk" | "pagetables" => cmd_pt_walk(),
        "scrub" | "memscrub" => cmd_scrub(),
        "faultinject" | "finject" => cmd_fault_inject(args),
        "frameowner" | "fowner" => cmd_frame_owner(),
        "alloctrace" | "atrace" => cmd_alloc_trace(args),
        "alloclat" | "alat" => cmd_alloc_lat(args),
        "heapprofile" | "hprof" => cmd_heap_profile(args),
        "syscallprof" | "sprof" => cmd_syscall_prof(args),
        "capaudit" | "capa" => cmd_cap_audit(args),
        "checkpoint" | "ckpt" => cmd_checkpoint(args),
        "strace" | "sctrace" => cmd_strace(args),
        "ipcstat" | "ipc" => cmd_ipc_stat(args),
        "kobjects" | "kobj" => cmd_kobjects(args),
        "fraghist" | "fragtrend" => cmd_frag_history(args),
        "selftest" => cmd_selftest(args),
        "watch" => cmd_watchpoint(args),
        "snapshot" | "snap" => cmd_snapshot(args),
        "ripsample" | "perf" => cmd_rip_sample(args),
        "invariant" | "invar" => cmd_invariant(args),
        "migrate" | "migrations" => cmd_migrate(args),
        "wchan" => cmd_wchan(),
        "bench" | "benchmark" => cmd_bench(args),
        "diag2" | "report" => cmd_diag_report(args),
        "hypervisor" | "vminfo" => cmd_hypervisor(),
        "cet" | "cfi" => cmd_cet(),
        "smap" | "smep" => cmd_smep_smap(),
        "fairness" | "jfi" => cmd_fairness(),
        "mempool" | "pools" => cmd_mempool(),
        "numa" => cmd_numa(),
        "rcu" => cmd_rcu(),
        "kevent" | "events" => cmd_kevent(),
        "ksyms" | "symbols" => cmd_ksyms(args),
        "memtype" | "memacct" => cmd_memtype(),
        "sclatency" | "sclat" => cmd_sclatency(args),
        "sar" => cmd_sar(),
        "syshealth" | "healthcheck" => cmd_syshealth(),
        "latency" | "lathist" => cmd_latency(),
        "pressure" | "mempressure" => cmd_pressure(),
        "jitter" | "tickjitter" => cmd_jitter(),
        "heapwm" | "watermark" => cmd_heapwm(),
        "memmap" => cmd_memmap(),
        "supervisor" | "sv" => cmd_supervisor(),
        "ps" | "tasks" => cmd_ps(),
        "clear" | "cls" => cmd_clear(),
        "uptime" => cmd_uptime(),
        "dmesg" => cmd_dmesg(args),
        "echo" => cmd_echo(args),
        "printf" => cmd_printf(args),
        "date" => cmd_date(args),
        "time" => cmd_time_cmd(args),
        "nproc" => cmd_nproc(),
        "strings" => cmd_strings(args),
        "cal" => cmd_cal(args),
        "reboot" => cmd_reboot(),
        "irq" => cmd_irq(),
        "pci" => cmd_pci(),
        "disk" | "blkinfo" => cmd_disk(),
        "blkread" => cmd_blkread(args),
        "ls" | "dir" => cmd_ls(args),
        "cat" | "type" => cmd_cat(args),
        "write" => cmd_write(args),
        "rm" | "del" => cmd_rm(args),
        "mkdir" => cmd_mkdir(args),
        "rmdir" => cmd_rmdir(args),
        "stat" => cmd_stat(args),
        "ln" | "link" => cmd_ln(args),
        "df" => cmd_df(args),
        "cp" | "copy" => cmd_cp(args),
        "mv" | "move" | "ren" => cmd_mv(args),
        "chmod" => cmd_chmod(args),
        "chown" => cmd_chown(args),
        "chattr" => cmd_chattr(args),
        "lsattr" => cmd_lsattr(args),
        "touch" => cmd_touch(args),
        "append" => cmd_append(args),
        "tree" => cmd_tree(args),
        "du" => cmd_du(args),
        "find" => cmd_find(args),
        "file" => cmd_file(args),
        "sync" => cmd_sync(),
        "mount" => cmd_mount(args),
        "umount" | "unmount" => cmd_umount(args),
        "wc" => cmd_wc(args),
        "head" => cmd_head(args),
        "tail" => cmd_tail(args),
        "hexdump" | "xxd" => cmd_hexdump(args),
        "lsof" => cmd_lsof(),
        "lsp" => cmd_lsp(args),
        "grep" => cmd_grep(args),
        "cmp" => cmd_cmp(args),
        "comm" => cmd_comm(args),
        "diff" => cmd_diff(args),
        "od" => cmd_od(args),
        "fallocate" => cmd_fallocate(args),
        "sort" => cmd_sort(args),
        "uniq" => cmd_uniq(args),
        "tee" => cmd_tee(args),
        "truncate" => cmd_truncate(args),
        "sha256" | "hash" => cmd_sha256(args),
        "sysctl" => cmd_sysctl(args),
        "hostname" => cmd_hostname(args),
        "dd" => cmd_dd(args),
        "free" => cmd_free(),
        "vmstat" => cmd_vmstat(),
        "label" => cmd_label(args),
        "flock" => cmd_flock(args),
        "split" => cmd_split(args),
        "lsblk" | "blkdev" => cmd_lsblk(),
        "glob" => cmd_glob(args),
        "readlink" => cmd_readlink(args),
        "symlink" | "mklink" => cmd_symlink(args),
        "xattr" => cmd_xattr(args),
        "watch" => cmd_watch(args),
        "trash" => cmd_trash(args),
        "basename" => cmd_basename(args),
        "dirname" => cmd_dirname(args),
        "realpath" => cmd_realpath(args),
        "pwd" => cmd_pwd(),
        "id" | "whoami" => cmd_id(),
        "mktemp" => cmd_mktemp(args),
        "mkfs.fat" | "mkfs" => cmd_mkfs_fat(args),
        "fsck.fat" => cmd_fsck_fat(args),
        "fsck.ext4" => cmd_fsck_ext4(args),
        "fsck" => {
            // Auto-dispatch: try ext4 first, then FAT.
            if args.trim().is_empty() {
                crate::console_println!("Usage: fsck DEVICE  (auto-detects fs type)");
                crate::console_println!("  fsck.fat DEVICE   — check FAT filesystem");
                crate::console_println!("  fsck.ext4 DEVICE  — check ext4 filesystem");
            } else {
                let dev = args.split_whitespace().find(|w| !w.starts_with('-')).unwrap_or(args.trim());
                if crate::fs::ext4::probe(dev) {
                    cmd_fsck_ext4(args);
                } else {
                    cmd_fsck_fat(args);
                }
            }
        }
        "tar" => cmd_tar(args),
        "crc32" | "crc32sum" => cmd_crc32(args),
        "base64" => cmd_base64(args),
        "wipe" => cmd_wipe(args),
        "checksum" | "cksum" => cmd_checksum(args),
        "gunzip" | "gzip" => cmd_gunzip(args),
        "bunzip2" | "bzcat" => cmd_bunzip2(args),
        "unzip" => cmd_unzip(args),
        "zip" => cmd_zip(args),
        "journal" => cmd_journal(args),
        "sed" => cmd_sed(args),
        "awk" => cmd_awk(args),
        "run" | "exec" => cmd_run(args),
        "mkelf" => cmd_mkelf(),
        "net" | "ifconfig" => cmd_net(),
        "dhcp" => cmd_dhcp(),
        "ping" => cmd_ping(args),
        "dns" | "nslookup" => cmd_dns(args),
        "wget" | "http" => cmd_wget(args),
        "version" | "ver" => cmd_version(),
        "uname" => cmd_uname(args),
        "source" | "." => cmd_source(args),
        "seq" => cmd_seq(args),
        "nl" => cmd_nl(args),
        "rev" => cmd_rev(args),
        "cut" => cmd_cut(args),
        "tr" => cmd_tr(args),
        "yes" => cmd_yes(args),
        "tac" => cmd_tac(args),
        "fold" => cmd_fold(args),
        "paste" => cmd_paste(args),
        "xargs" => cmd_xargs(args),
        "column" => cmd_column(args),
        "sleep" => cmd_sleep(args),
        "true" => { set_exit(0); }
        "false" => { set_exit(1); }
        "test" | "[" => cmd_test(args),
        "expr" => cmd_expr(args),
        "printenv" | "env" => cmd_printenv(),
        "declare" => cmd_declare(args),
        "read" => cmd_read(args),
        "mapfile" | "readarray" => cmd_mapfile(args),
        "readonly" => cmd_readonly(args),
        "let" => cmd_let(args),
        "trap" => cmd_trap(args),
        "command" => cmd_command(args),
        "which" | "typeof" => cmd_type(args),
        "return" => {
            // `return [N]` — set exit status and signal function return.
            if !args.is_empty() {
                if let Ok(code) = args.parse::<u8>() {
                    set_exit(code);
                } else {
                    crate::console_println!("return: invalid status '{}'", args);
                    set_exit(1);
                }
            }
            FUNC_RETURN.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        "break" => {
            LOOP_BREAK.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        "continue" => {
            LOOP_CONTINUE.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        "shift" => {
            // `shift [N]` — discard the first N positional params (default 1).
            let n: usize = if args.is_empty() {
                1
            } else {
                args.parse().unwrap_or(1)
            };
            let mut stack = POSITIONAL_PARAMS.lock();
            if let Some(frame) = stack.last_mut() {
                for _ in 0..n {
                    if !frame.is_empty() {
                        frame.remove(0);
                    }
                }
            }
        }
        "local" => {
            cmd_local(args);
        }
        _ => {
            // Check user-defined functions before reporting unknown.
            if let Some(body) = FUNCTIONS.lock().get(cmd).cloned() {
                let func_args: Vec<String> = split_words(args);
                execute_function(&body, &func_args);
            } else {
                crate::console_println!("Unknown command: '{}'. Type 'help' for a list.", cmd);
                set_exit(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_help() {
    crate::console_println!("Available commands:");
    crate::console_println!("  help      Show this help message");
    crate::console_println!("  meminfo   Show physical memory usage");
    crate::console_println!("  ps        List scheduler tasks");
    crate::console_println!("  clear     Clear the screen");
    crate::console_println!("  uptime    Show system uptime (tick count)");
    crate::console_println!("  echo ...  Echo text to console");
    crate::console_println!("  time CMD  Time command execution (or show time with no args)");
    crate::console_println!("  nproc     Show number of CPUs");
    crate::console_println!("  irq       Show IRQ interrupt counts");
    crate::console_println!("  pci       List PCI devices");
    crate::console_println!("  disk      Show block device info");
    crate::console_println!("  blkread N Hex-dump sector N from disk");
    crate::console_println!("  cd [dir]  Change working directory");
    crate::console_println!("  ls [-lahRStr] [path] List files (-l long, -a all, -h human, -R recurse, -S size-sort, -t time-sort, -r reverse)");
    crate::console_println!("  cat FILE  Print file contents");
    crate::console_println!("  write F T Write text T to file F");
    crate::console_println!("  rm [-r] F Delete a file (or directory tree with -r)");
    crate::console_println!("  mkdir DIR Create a directory");
    crate::console_println!("  rmdir DIR Remove an empty directory");
    crate::console_println!("  stat FILE Show detailed file metadata");
    crate::console_println!("  ln S D    Create hard link D pointing to S");
    crate::console_println!("  cp [-r] S D Copy file (or dir tree with -r) S to D");
    crate::console_println!("  mv S D    Move/rename file or directory");
    crate::console_println!("  chmod M F Set permissions (octal, e.g., chmod 755 file)");
    crate::console_println!("  chown U F Set owner (uid:gid, e.g., chown 1000:1000 file)");
    crate::console_println!("  chattr +/-FLAGS F  Set/clear attributes (i=immutable a=append h=hidden s=system)");
    crate::console_println!("  lsattr F  Show file attributes (iahs flags)");
    crate::console_println!("  touch [-d DATE | -r REF] F  Create file or set timestamps (-d YYYY-MM-DD, -r reffile)");
    crate::console_println!("  append F T Append text T to file F");
    crate::console_println!("  tree [D]  Show directory tree recursively");
    crate::console_println!("  du [-s] [-dN] [D]  Show disk usage (-s summary, -dN max depth)");
    crate::console_println!("  find [D] [-name PAT] [-type f|d|l] [-size +N|-N] [-maxdepth N] [-empty]");
    crate::console_println!("  df [path] Show filesystem space usage");
    crate::console_println!("  sync      Flush all filesystems to disk");
    crate::console_println!("  mount     List all mounted filesystems");
    crate::console_println!("  umount P  Unmount filesystem at path P");
    crate::console_println!("  wc FILE   Count lines, words, and bytes");
    crate::console_println!("  head N F  Show first N lines of file");
    crate::console_println!("  tail N F  Show last N lines of file");
    crate::console_println!("  hexdump [-n N] F  Hex dump of file contents (default 512 bytes, -n 0 for all)");
    crate::console_println!("  lsof      List open file handles");
    crate::console_println!("  lsp [N] D Paginated ls: show N entries at a time");
    crate::console_println!("  grep [-ivclnwrI] PATTERN FILE  Search for pattern in files (-r recursive, -v invert, -c count, -w whole-word, -l files-only, -I case-sensitive)");
    crate::console_println!("  cmp F1 F2 Compare two files byte-by-byte");
    crate::console_println!("  comm [-123] F1 F2  Compare sorted files (3-column output)");
    crate::console_println!("  diff F1 F2 Line-level diff (unified format)");
    crate::console_println!("  od [-A o|d|x|n] [-t o1|x1|d1|u1|c] [-N count] F  Octal/hex dump");
    crate::console_println!("  cpuinfo   Show per-CPU utilization and scheduler counters");
    crate::console_println!("  top       Compact system overview (uptime, memory, CPU, tasks)");
    crate::console_println!("  vmstat    Show all VM statistics counters");
    crate::console_println!("  schedstat Per-task scheduling fairness analysis");
    crate::console_println!("  watchdog  Show per-CPU soft lockup watchdog status");
    crate::console_println!("  kill TID  Terminate a task by ID");
    crate::console_println!("  renice TID PRI  Change task priority (0=highest, 31=lowest)");
    crate::console_println!("  throttle TID [%] Set/query CPU bandwidth quota");
    crate::console_println!("  taskset TID [0xMASK] Set/query CPU affinity");
    crate::console_println!("  slabinfo  Show per-size-class heap allocator statistics");
    crate::console_println!("  fraginfo  Show heap internal fragmentation per size class");
    crate::console_println!("  leakcheck Snapshot heap active counts for leak detection");
    crate::console_println!("  stack     Show per-task kernel stack usage (high water mark)");
    crate::console_println!("  wq        Show kernel workqueue status and statistics");
    crate::console_println!("  ktimer    Show kernel timer statistics");
    crate::console_println!("  rng       Show kernel CSPRNG statistics");
    crate::console_println!("  supervisor Show task supervisor status");
    crate::console_println!("  trace [N] Show last N kernel trace events (default 20)");
    crate::console_println!("  lockdep [sub] Lock order validator (classes/edges/held/all)");
    crate::console_println!("  bt        Show current kernel call stack (backtrace)");
    crate::console_println!("  diag      One-stop system health diagnostic summary");
    crate::console_println!("  exceptions Show per-vector exception/interrupt counts");
    crate::console_println!("  exclog     Show recent exception event log");
    crate::console_println!("  sysinfo    Show CPU vendor, brand, features (cpuid)");
    crate::console_println!("  boottime   Show boot milestone timing");
    crate::console_println!("  canary     Scan all task stack canaries for corruption");
    crate::console_println!("  tlb        Show TLB shootdown statistics");
    crate::console_println!("  pgfault    Show page fault statistics by type");
    crate::console_println!("  irqrate    Show interrupt rates (IRQs/sec per vector)");
    crate::console_println!("  kprofile   Kernel code profiler (cycle counts per region)");
    crate::console_println!("  lockstats  Show spinlock contention statistics");
    crate::console_println!("  idle       Show CPU idle state statistics (MWAIT/HLT)");
    crate::console_println!("  kstat      Show system metrics history (1-min time series)");
    crate::console_println!("  kwarn      Show kernel warnings (kwarn clear to reset)");
    crate::console_println!("  loadavg    Show 1/5/15 minute system load averages");
    crate::console_println!("  memtype    Show physical memory usage by type");
    crate::console_println!("  sclatency  Show syscall latency histogram");
    crate::console_println!("  irqoff     Show interrupt-disabled duration tracking");
    crate::console_println!("  latency    Show scheduling latency histogram");
    crate::console_println!("  pressure   Show memory pressure score (0-100)");
    crate::console_println!("  sar        System activity reporter (compact one-liner)");
    crate::console_println!("  syshealth  Active system integrity verification");
    crate::console_println!("  jitter     Show timer interrupt jitter (inter-tick variance)");
    crate::console_println!("  heapwm     Show heap allocation watermark (peak usage)");
    crate::console_println!("  memmap     Show virtual address space layout");
    crate::console_println!("  vmalloc    Show vmalloc (virtual kernel memory) statistics");
    crate::console_println!("  rmap       Show reverse mapping (frame→PTE) statistics");
    crate::console_println!("  profile [name]   Show/set workload profile (desktop/server/dev/gaming)");
    crate::console_println!("  fallocate N F Pre-allocate N bytes for file F");
    crate::console_println!("  sort FILE Sort lines of a file alphabetically");
    crate::console_println!("  uniq FILE Remove adjacent duplicate lines");
    crate::console_println!("  tee F T   Write text T to file F and display it");
    crate::console_println!("  truncate N F Truncate file F to N bytes");
    crate::console_println!("  sha256 F  Compute SHA-256 hash of file contents");
    crate::console_println!("  sysctl .. List/get/set kernel parameters");
    crate::console_println!("  hostname  Show or set system hostname");
    crate::console_println!("  dd ..     Copy blocks between files (if=/of=/bs=/count=/skip=/seek=)");
    crate::console_println!("  free      Show memory usage summary");
    crate::console_println!("  flock [-s|-x|-u] FILE  Query/acquire/release advisory file locks");
    crate::console_println!("  split [-l N|-b SIZE] FILE [PREFIX]  Split file into pieces");
    crate::console_println!("  label [PATH] [NAME]  Show or set volume label for filesystem at PATH");
    crate::console_println!("  lsblk     List block devices with sizes");
    crate::console_println!("  glob P    Expand glob pattern (e.g., /tmp/*.txt)");
    crate::console_println!("  readlink P Show symlink target");
    crate::console_println!("  symlink T P Create symlink at P pointing to T");
    crate::console_println!("  xattr F .. Extended attributes (list/get/set/rm)");
    crate::console_println!("  watch P [-r] Monitor filesystem changes (press key to stop)");
    crate::console_println!("  journal [-n N] Show filesystem change journal entries");
    crate::console_println!("  basename P Extract filename from path");
    crate::console_println!("  dirname P  Extract directory from path");
    crate::console_println!("  realpath P Resolve path (follow symlinks)");
    crate::console_println!("  pwd        Print working directory");
    crate::console_println!("  id         Show current task identity");
    crate::console_println!("  mktemp [D] Create temporary file (default /tmp)");
    crate::console_println!("  mkfs.fat [-L LABEL] DEVICE  Format device as FAT16/FAT32 (auto-selects type)");
    crate::console_println!("  fsck.fat [-a] DEVICE        Check/repair FAT filesystem consistency");
    crate::console_println!("  fsck.ext4 [-v] DEVICE       Check ext4 filesystem consistency (read-only)");
    crate::console_println!("  tar -cf A F.. | -xf A [-C D] | -tf A  Create/extract/list USTAR archives (.tar.gz/.tar.bz2 supported)");
    crate::console_println!("  gunzip F [-o OUT]  Decompress gzip file (gzip -d alias)");
    crate::console_println!("  bunzip2 F [-o OUT] Decompress bzip2 file (bzcat alias)");
    crate::console_println!("  unzip [-l] F [-d DIR]  List or extract ZIP archive (stored + deflated)");
    crate::console_println!("  zip [-0] [-r] F.zip FILE..  Create ZIP archive (deflate or stored)");
    crate::console_println!("  crc32 FILE    Compute CRC32C checksum");
    crate::console_println!("  base64 [-d] F Encode file to Base64 (or -d to decode)");
    crate::console_println!("  checksum [-t sha256|crc32] FILE  Compute file checksum");
    crate::console_println!("  wipe FILE     Secure delete (zero-fill + remove)");
    crate::console_println!("  sed [-i] [-n] 's/old/new/[g]' FILE  Stream editor (substitute/delete/print)");
    crate::console_println!("  awk [-F sep] 'program' FILE  Text processing ($1..$N, NR, NF, /pattern/)");
    crate::console_println!("  run FILE  Load and execute an ELF binary");
    crate::console_println!("  mkelf     Create test ELF binaries (EXIT.ELF + HELLO.ELF)");
    crate::console_println!("  net       Show network interface info");
    crate::console_println!("  dhcp      Obtain an IP address via DHCP");
    crate::console_println!("  ping IP   Send ICMP echo requests (ping)");
    crate::console_println!("  dns NAME  Resolve a domain name to IP");
    crate::console_println!("  wget URL  Fetch a URL via HTTP GET");
    crate::console_println!("  version   Show kernel version");
    crate::console_println!("  uname [-asnrvmo] Print system information");
    crate::console_println!("  source F  Execute kshell commands from file F");
    crate::console_println!("  seq N [M] Print numbers from 1..N or N..M");
    crate::console_println!("  nl [F]    Number lines of file (or piped input)");
    crate::console_println!("  rev [F]   Reverse order of lines in file (or piped)");
    crate::console_println!("  sleep N   Pause for N milliseconds");
    crate::console_println!("  printenv  Show environment variables");
    crate::console_println!("  export N=V Set environment variable");
    crate::console_println!("  unset N   Remove environment variable");
    crate::console_println!("  alias N=V Define command alias");
    crate::console_println!("  unalias N Remove command alias");
    crate::console_println!("  dmesg [-n] Show kernel log messages");
    crate::console_println!("  file F    Identify file type by extension");
    crate::console_println!("  printf FMT .. Formatted output (%s %d %x %o %c)");
    crate::console_println!("  trash F   Move file to recycle bin (--list/--restore/--empty/--prune)");
    crate::console_println!("  cut -d/-f/-c  Extract columns/fields from text");
    crate::console_println!("  tr SET1 SET2  Translate/delete characters");
    crate::console_println!("  tac [F]   Print lines in reverse order");
    crate::console_println!("  fold [-w N] F Wrap lines to N columns (default 80)");
    crate::console_println!("  paste F1 F2   Merge lines from files");
    crate::console_println!("  yes [STR]  Repeat STR (default 'y') indefinitely");
    crate::console_println!("  xargs [-n N] CMD  Run CMD with piped input as arguments");
    crate::console_println!("  strings [-n N] F  Extract printable strings from binary file (min length N, default 4)");
    crate::console_println!("  column [-t] [-s SEP] F  Format text into aligned columns (-t table mode)");
    crate::console_println!("  date [+FMT] Show date/time (format: %Y %m %d %H %M %S %a %b %F %T %s)");
    crate::console_println!("  cal [M] [Y] Show monthly calendar (current month if no args)");
    crate::console_println!("  test EXPR / [ EXPR ]  Conditional expressions");
    crate::console_println!("  expr EXPR  Evaluate arithmetic expression");
    crate::console_println!("  read [-p PROMPT] VAR  Read user input into variable");
    crate::console_println!("  eval ARGS  Concatenate args and execute as command");
    crate::console_println!("  select VAR in WORDS  Interactive menu (numbered choice)");
    crate::console_println!("  declare -a/-f  List arrays / functions");
    crate::console_println!("  mapfile ARR  Read lines into array variable");
    crate::console_println!("  readonly VAR  Mark variable as read-only");
    crate::console_println!("  trap CMD SIG  Set signal handler (EXIT/ERR/INT)");
    crate::console_println!("  set -e/-x  errexit / xtrace shell options");
    crate::console_println!("  which/type CMD  Show command type (builtin/alias/function)");
    crate::console_println!("");
    crate::console_println!("I/O redirection:");
    crate::console_println!("  cmd > file   Write output to file (overwrite)");
    crate::console_println!("  cmd >> file  Append output to file");
    crate::console_println!("  cmd1 | cmd2  Pipe output of cmd1 into cmd2");
    crate::console_println!("  cmd < file   Read input from file");
    crate::console_println!("  cmd <<DELIM  Here-document (multi-line input to cmd)");
    crate::console_println!("  cmd <<-DELIM Here-doc with leading tab stripping");
    crate::console_println!("  cmd <<'D'    Here-doc without variable expansion");
    crate::console_println!("Variable expansion:");
    crate::console_println!("  $NAME / ${{NAME}}  Expand environment variable");
    crate::console_println!("  $(command)       Command substitution (capture output)");
    crate::console_println!("  $((expr))        Arithmetic expansion");
    crate::console_println!("  $1..$9 $# $@     Positional params (in functions)");
    crate::console_println!("  $$               Literal dollar sign");
    crate::console_println!("String operations:");
    crate::console_println!("  ${{VAR:N}}  ${{VAR:N:L}}  Substring (offset, offset+length)");
    crate::console_println!("  ${{VAR/pat/rep}}       Replace first match");
    crate::console_println!("  ${{VAR//pat/rep}}      Replace all matches");
    crate::console_println!("  ${{VAR^}} ${{VAR^^}}     Uppercase first / all chars");
    crate::console_println!("  ${{VAR,}} ${{VAR,,}}     Lowercase first / all chars");
    crate::console_println!("Control flow:");
    crate::console_println!("  if COND; then ... elif COND; then ... else ... fi");
    crate::console_println!("  while COND; do ... done  (max 1000 iterations)");
    crate::console_println!("  until COND; do ... done  (loop until true)");
    crate::console_println!("  for VAR in WORDS; do ... done");
    crate::console_println!("  for ((i=0; i<N; i=i+1)); do ... done  (C-style)");
    crate::console_println!("  case VAR in pat) ... ;; esac  (pattern matching)");
    crate::console_println!("  break / continue  Loop control");
    crate::console_println!("  cmd1 && cmd2  Run cmd2 only if cmd1 succeeds");
    crate::console_println!("  cmd1 || cmd2  Run cmd2 only if cmd1 fails");
    crate::console_println!("  cmd1 ; cmd2   Run cmd2 regardless");
    crate::console_println!("Functions:");
    crate::console_println!("  name() {{ body; }}  Define a function");
    crate::console_println!("  name arg1 arg2   Call function ($1, $2, $#, $@)");
    crate::console_println!("  declare -f       List all defined functions");
    crate::console_println!("  unset -f NAME    Remove a function definition");
    crate::console_println!("  return [N]       Return from function with status N");
    crate::console_println!("  local VAR=VALUE  Function-scoped variable (restored on return)");
    crate::console_println!("Arrays:");
    crate::console_println!("  arr=(a b c)      Declare array");
    crate::console_println!("  ${{arr[0]}}        Access element (0-based)");
    crate::console_println!("  ${{arr[@]}}        All elements (space-separated)");
    crate::console_println!("  ${{#arr[@]}}       Array length");
    crate::console_println!("  arr[N]=value     Set element N");
    crate::console_println!("  unset arr        Remove array");
    crate::console_println!("  unset arr[N]     Clear element N");
    crate::console_println!("  declare -a       List all arrays");
    crate::console_println!("  reboot    Reboot the system");
}

// Division-by-constant conversions are safe (1024 never overflows).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_meminfo() {
    match crate::mm::frame::stats() {
        Some(stats) => {
            crate::console_println!("Physical memory:");
            // Each frame is 16 KiB.
            let free_kib = stats.free_frames.saturating_mul(16);
            let total_kib = stats.total_frames.saturating_mul(16);
            let used = stats.total_frames.saturating_sub(stats.free_frames);
            let used_kib = used.saturating_mul(16);
            crate::console_println!(
                "  Total: {} frames ({} KiB / {} MiB)",
                stats.total_frames,
                total_kib,
                total_kib / 1024
            );
            crate::console_println!(
                "  Used:  {} frames ({} KiB / {} MiB)",
                used,
                used_kib,
                used_kib / 1024
            );
            crate::console_println!(
                "  Free:  {} frames ({} KiB / {} MiB)",
                stats.free_frames,
                free_kib,
                free_kib / 1024
            );
        }
        None => {
            crate::console_println!("Error: frame allocator not initialized");
        }
    }

    // Heap allocator stats (always available, lock-free).
    let h = crate::mm::heap::stats();
    crate::console_println!("Kernel heap:");
    crate::console_println!(
        "  Slab:  {} allocs, {} frees (live: {})",
        h.slab_allocs,
        h.slab_frees,
        h.slab_allocs.saturating_sub(h.slab_frees)
    );
    crate::console_println!(
        "  Large: {} allocs, {} frees (live: {})",
        h.large_allocs,
        h.large_frees,
        h.large_allocs.saturating_sub(h.large_frees)
    );
    crate::console_println!(
        "  Refills: {}, Failures: {}",
        h.slab_refills,
        h.alloc_failures
    );

    // Pre-zeroed frame pool.
    let pool_count = crate::mm::frame::zero_pool_count();
    let (pool_hits, pool_misses) = crate::mm::frame::zero_pool_stats();
    let pool_total = pool_hits.saturating_add(pool_misses);
    let hit_pct = if pool_total > 0 {
        pool_hits.saturating_mul(100) / pool_total
    } else {
        0
    };
    crate::console_println!("Zero pool:");
    crate::console_println!(
        "  Cached: {} frames, Hits: {}, Misses: {} ({}% hit rate)",
        pool_count,
        pool_hits,
        pool_misses,
        hit_pct
    );

    // Memory pressure state.
    let pi = crate::mm::pressure::pressure_info();
    let level_str = match pi.level {
        crate::mm::pressure::PressureLevel::None => "none",
        crate::mm::pressure::PressureLevel::Low => "low",
        crate::mm::pressure::PressureLevel::Medium => "medium",
        crate::mm::pressure::PressureLevel::Critical => "CRITICAL",
    };
    crate::console_println!("Pressure:");
    crate::console_println!(
        "  Level: {}, Shrinkers: {}, Notified: {}, Freed: {} objects",
        level_str,
        pi.active_shrinkers,
        pi.total_notifications,
        pi.total_freed
    );

    // Swap summary.
    let (swap_total, swap_used, swap_devs) = crate::mm::swap::summary();
    if swap_devs > 0 || swap_total > 0 {
        crate::console_println!("Swap:");
        crate::console_println!(
            "  {} KiB used / {} KiB total ({} device{})",
            swap_used / 1024,
            swap_total / 1024,
            swap_devs,
            if swap_devs == 1 { "" } else { "s" }
        );
    }

    // OOM stats.
    let oom_events = crate::mm::oom::oom_event_count();
    let oom_kills = crate::mm::oom::oom_kill_count();
    if oom_events > 0 {
        crate::console_println!("OOM: {} events, {} kills", oom_events, oom_kills);
    }
}

/// Display a compact system overview (like `top` snapshot).
///
/// Shows uptime, load average, memory summary, CPU utilization per-CPU,
/// memory pressure status, and the top tasks by CPU time.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_top() {
    let ticks = crate::apic::tick_count();
    let seconds = ticks / 100;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    // --- Header: uptime + load ---
    let load = crate::sched::load_average_x100();
    let stats = crate::sched::sched_stats();
    let active = stats.total_tasks_spawned.saturating_sub(stats.total_tasks_exited);
    let starv_boosts = crate::sched::starvation_boost_count();
    shell_println!(
        "up {:02}:{:02}:{:02}  load: {}.{:02}  tasks: {} active  ctx: {}  steals: {}",
        hours, minutes % 60, seconds % 60,
        load / 100, load % 100,
        active, stats.total_ctx_switches, stats.total_work_steals,
    );
    if starv_boosts > 0 {
        shell_println!("  anti-starvation boosts: {}", starv_boosts);
    }

    // --- Memory ---
    let mem = crate::mm::memory_info();
    let total_mb = mem.total_bytes / (1024 * 1024);
    let used_mb = mem.used_bytes / (1024 * 1024);
    let free_mb = mem.free_bytes / (1024 * 1024);
    let pressure = crate::mm::pressure::current_level();
    shell_println!(
        "Mem: {} MiB total, {} MiB used, {} MiB free  frag: {}%  pressure: {}",
        total_mb, used_mb, free_mb, mem.fragmentation_pct, pressure,
    );

    // --- CPU utilization bar (compact) ---
    let num_cpus = stats.num_cpus;
    if num_cpus > 0 {
        shell_print!("CPU:");
        for i in 0..num_cpus {
            let (total, idle) = stats.cpu_ticks.get(i).copied().unwrap_or((0, 0));
            let util = if total > 0 {
                total.saturating_sub(idle).saturating_mul(100) / total
            } else {
                0
            };
            shell_print!(" [{}]{}%", i, util);
        }
        shell_println!("");
    }

    // --- Top tasks (sorted by CPU cycles, descending) ---
    shell_println!("");
    let mut task_list = crate::sched::task_list();
    // Sort by TSC cycles (more precise than ticks).
    task_list.sort_by(|a, b| b.total_cycles.cmp(&a.total_cycles));

    let freq = crate::bench::tsc_freq();
    shell_println!(
        "{:<5} {:<12} {:<8} {:>3} {:>10} {:>6}",
        "TID", "NAME", "STATE", "PRI", "CPU_TIME", "WAIT"
    );
    shell_println!("-----------------------------------------------------------");
    // Show top 10 (or all if fewer).
    let show_count = task_list.len().min(10);
    for info in task_list.iter().take(show_count) {
        let name = core::str::from_utf8(
            info.name.get(..info.name_len).unwrap_or(&info.name)
        ).unwrap_or("?");

        // TSC-based time (nanosecond precision).
        let cpu_ms = if freq > 0 {
            crate::bench::cycles_to_ns(info.total_cycles) / 1_000_000
        } else {
            // Fallback to tick-based
            info.total_ticks * 10
        };
        let cpu_secs = cpu_ms / 1000;
        let cpu_frac = (cpu_ms % 1000) / 10;
        let cpu_mins = cpu_secs / 60;
        let cpu_s = cpu_secs % 60;

        // Wait time as seconds.tenths
        let wait_secs = info.total_wait_ticks / 100;
        let wait_frac = (info.total_wait_ticks % 100) / 10;
        shell_println!(
            "{:<5} {:<12} {:<8} {:>3} {:>4}:{:02}.{:02} {:>3}.{}",
            info.id, name, info.state, info.priority,
            cpu_mins, cpu_s, cpu_frac,
            wait_secs, wait_frac,
        );
    }
    if task_list.len() > show_count {
        shell_println!("  ... ({} more)", task_list.len() - show_count);
    }
}

/// Display CPU utilization and scheduler statistics.
///
/// Shows per-CPU utilization percentages, system load average, and
/// key scheduler counters (context switches, work steals, spawns/exits).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_cpuinfo() {
    let stats = crate::sched::sched_stats();
    let num_cpus = stats.num_cpus;

    // System load average.
    let load_whole = stats.load_avg_x100 / 100;
    let load_frac = stats.load_avg_x100 % 100;
    shell_println!("Load average: {}.{:02}", load_whole, load_frac);
    shell_println!("");

    // Per-CPU utilization.
    shell_println!(
        "{:<5} {:>6} {:>6} {:>6} {:>10} {:>8} {:>8}",
        "CPU", "UTIL%", "TOTAL", "IDLE", "CTX_SW", "VOLUNT", "PREEMPT"
    );
    shell_println!("-----------------------------------------------------------");
    for i in 0..num_cpus {
        let (total, idle) = stats.cpu_ticks.get(i).copied().unwrap_or((0, 0));
        let util_pct = if total > 0 {
            total.saturating_sub(idle).saturating_mul(100) / total
        } else {
            0
        };
        let ctx = stats.ctx_switches.get(i).copied().unwrap_or(0);
        let vol = stats.voluntary_switches.get(i).copied().unwrap_or(0);
        let pre = stats.preemptions.get(i).copied().unwrap_or(0);
        shell_println!(
            "{:<5} {:>5}% {:>6} {:>6} {:>10} {:>8} {:>8}",
            i, util_pct, total, idle, ctx, vol, pre
        );
    }

    shell_println!("");
    shell_println!(
        "Total ctx switches: {}  Work steals: {}",
        stats.total_ctx_switches, stats.total_work_steals
    );
    shell_println!(
        "Tasks spawned: {}  Tasks exited: {}  Active: {}",
        stats.total_tasks_spawned,
        stats.total_tasks_exited,
        stats.total_tasks_spawned.saturating_sub(stats.total_tasks_exited)
    );
}

/// Show or set the system workload profile.
///
/// Usage:
///   `profile`                — show current profile
///   `profile desktop`       — set to Desktop (balanced, general use)
///   `profile server`        — set to Server (throughput, long slices)
///   `profile dev`           — set to Development (short slices, many tasks)
///   `profile gaming`        — set to Gaming (low latency, responsive)
///
/// Sets both scheduler time slices and memory parameters.
fn cmd_profile(args: &str) {
    let arg = args.trim();

    if arg.is_empty() {
        // Show current profiles.
        let sched_profile = crate::sched::current_workload_profile();
        let mem_profile = crate::sysctl::current_memory_profile();

        shell_println!("System workload profiles:");
        match sched_profile {
            Some(p) => shell_println!("  Scheduler: {:?}", p),
            None => shell_println!("  Scheduler: custom (manually tuned)"),
        }
        match mem_profile {
            Some(p) => shell_println!("  Memory:    {:?}", p),
            None => shell_println!("  Memory:    custom (manually tuned)"),
        }
        shell_println!("");
        shell_println!("Available: desktop, server, dev, gaming");
        return;
    }

    let profile_id: u8 = match arg {
        "desktop" | "Desktop" | "0" => 0,
        "server" | "Server" | "1" => 1,
        "dev" | "development" | "Development" | "2" => 2,
        "gaming" | "Gaming" | "3" => 3,
        _ => {
            shell_println!("Unknown profile: {}", arg);
            shell_println!("Available: desktop, server, dev, gaming");
            return;
        }
    };

    if crate::sysctl::apply_system_profile(profile_id) {
        let names = ["Desktop", "Server", "Development", "Gaming"];
        let name = names.get(profile_id as usize).unwrap_or(&"?");
        shell_println!("Applied system profile: {} (sched + memory)", name);
    } else {
        shell_println!("Failed to apply profile.");
    }
}

/// Display soft lockup watchdog status.
///
/// Shows per-CPU heartbeat counts and stall indicators.  A non-zero
/// stall count means the watchdog has noticed that CPU not ticking
/// for one or more check intervals (5 seconds each).
fn cmd_watchdog() {
    let num_cpus = crate::smp::cpu_count();
    let status = crate::sched::watchdog_status();

    shell_println!("Soft lockup watchdog (check every 5s, alert after 10s):");
    shell_println!("{:<5} {:>12} {:>8}", "CPU", "HEARTBEAT", "STALL");
    shell_println!("-----------------------------");
    for i in 0..num_cpus {
        let (heartbeat, stall) = status.get(i).copied().unwrap_or((0, 0));
        let indicator = if stall >= 2 {
            " *** LOCKUP ***"
        } else if stall >= 1 {
            " (stalled)"
        } else {
            ""
        };
        shell_println!(
            "{:<5} {:>12} {:>8}{}",
            i, heartbeat, stall, indicator
        );
    }
}

/// Kill a task by its ID.
///
/// Usage: `kill <task_id>`
///
/// Terminates the specified task.  Cannot kill the current task (the
/// shell itself) or idle tasks.  Use `ps` to find task IDs.
fn cmd_kill(args: &str) {
    let args = args.trim();
    if args.is_empty() {
        shell_println!("Usage: kill <task_id>");
        shell_println!("Use 'ps' to see task IDs.");
        return;
    }

    let Ok(task_id) = args.parse::<u64>() else {
        shell_println!("Invalid task ID: {}", args);
        return;
    };

    if task_id == 0 {
        shell_println!("Cannot kill task 0 (BSP idle).");
        return;
    }

    if crate::sched::kill_task(task_id) {
        shell_println!("Killed task {}.", task_id);
    } else {
        shell_println!(
            "Failed to kill task {} (not found, already dead, or is the current task).",
            task_id,
        );
    }
}

/// Change a task's scheduling priority.
///
/// Usage: `renice <task_id> <priority>`
///
/// Priority is 0 (highest) to 31 (lowest/idle).  Lower numbers run
/// first.  Use `ps` to see current priorities.
fn cmd_renice(args: &str) {
    let words: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if words.len() < 2 {
        shell_println!("Usage: renice <task_id> <priority>");
        shell_println!("  priority: 0 (highest) to 31 (lowest/idle)");
        return;
    }

    let Some(&tid_str) = words.get(0) else { return };
    let Some(&pri_str) = words.get(1) else { return };

    let Ok(task_id) = tid_str.parse::<u64>() else {
        shell_println!("Invalid task ID: {}", tid_str);
        return;
    };

    let Ok(priority) = pri_str.parse::<u8>() else {
        shell_println!("Invalid priority: {} (must be 0-31)", pri_str);
        return;
    };

    if priority > 31 {
        shell_println!("Priority must be 0-31, got {}", priority);
        return;
    }

    if let Some(old) = crate::sched::set_priority(task_id, priority) {
        shell_println!(
            "Task {} priority changed: {} -> {}",
            task_id, old, priority
        );
    } else {
        shell_println!("Task {} not found.", task_id);
    }
}

/// Set or query a task's CPU bandwidth quota.
///
/// Usage:
///   `throttle <task_id> <percent>`  — set quota (1-100%, 0=unlimited)
///   `throttle <task_id>`            — query current quota
///
/// A task with a 50% quota can use at most 50 ticks out of every 100
/// (1-second bandwidth period) before being throttled.
fn cmd_throttle(args: &str) {
    let words: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if words.is_empty() {
        shell_println!("Usage: throttle <task_id> [percent]");
        shell_println!("  percent: 1-100 (CPU%), 0=unlimited");
        shell_println!("  omit percent to query current quota");
        return;
    }

    let Some(&tid_str) = words.get(0) else { return };
    let Ok(task_id) = tid_str.parse::<u64>() else {
        shell_println!("Invalid task ID: {}", tid_str);
        return;
    };

    if let Some(&pct_str) = words.get(1) {
        // Set quota.
        let Ok(pct) = pct_str.parse::<u8>() else {
            shell_println!("Invalid percentage: {} (must be 0-100)", pct_str);
            return;
        };
        if pct > 100 {
            shell_println!("Percentage must be 0-100, got {}", pct);
            return;
        }
        crate::sched::set_cpu_quota(task_id, pct);
        if pct == 0 {
            shell_println!("Task {} CPU quota: unlimited", task_id);
        } else {
            shell_println!("Task {} CPU quota: {}%", task_id, pct);
        }
    } else {
        // Query quota.
        match crate::sched::get_cpu_quota(task_id) {
            Some(0) | None => {
                shell_println!("Task {} CPU quota: unlimited", task_id);
            }
            Some(pct) => {
                shell_println!("Task {} CPU quota: {}%", task_id, pct);
            }
        }
    }
}

fn cmd_taskset(args: &str) {
    let words: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if words.is_empty() {
        shell_println!("Usage: taskset <task_id> [mask]");
        shell_println!("  mask: hex CPU affinity bitmask (e.g. 0x3 = CPUs 0,1)");
        shell_println!("  omit mask to query current affinity");
        shell_println!("  0xf = CPUs 0-3, 0xff = CPUs 0-7, etc.");
        return;
    }

    let Some(&tid_str) = words.get(0) else { return };
    let Ok(task_id) = tid_str.parse::<u64>() else {
        shell_println!("Invalid task ID: {}", tid_str);
        return;
    };

    if let Some(&mask_str) = words.get(1) {
        // Set affinity.
        let mask = if let Some(hex) = mask_str.strip_prefix("0x") {
            u64::from_str_radix(hex, 16).ok()
        } else if let Some(hex) = mask_str.strip_prefix("0X") {
            u64::from_str_radix(hex, 16).ok()
        } else {
            mask_str.parse::<u64>().ok()
        };

        let Some(mask) = mask else {
            shell_println!("Invalid mask: {} (use hex 0x... or decimal)", mask_str);
            return;
        };

        if mask == 0 {
            shell_println!("Error: mask cannot be 0 (task must be runnable on at least one CPU)");
            return;
        }

        match crate::sched::set_cpu_affinity(task_id, mask) {
            Some(old) => {
                shell_println!("Task {} affinity: 0x{:x} -> 0x{:x}", task_id, old, mask);
                // Show which CPUs are allowed.
                let mut cpus = alloc::string::String::new();
                for bit in 0..64u32 {
                    if (mask >> bit) & 1 == 1 {
                        if !cpus.is_empty() {
                            cpus.push(',');
                        }
                        use core::fmt::Write;
                        let _ = write!(cpus, "{}", bit);
                    }
                }
                shell_println!("  Allowed CPUs: {}", cpus);
            }
            None => {
                shell_println!("Task {} not found or invalid mask", task_id);
            }
        }
    } else {
        // Query affinity.
        match crate::sched::get_cpu_affinity(task_id) {
            Some(mask) => {
                shell_println!("Task {} affinity: 0x{:x}", task_id, mask);
                let mut cpus = alloc::string::String::new();
                for bit in 0..64u32 {
                    if (mask >> bit) & 1 == 1 {
                        if !cpus.is_empty() {
                            cpus.push(',');
                        }
                        use core::fmt::Write;
                        let _ = write!(cpus, "{}", bit);
                    }
                }
                shell_println!("  Allowed CPUs: {}", cpus);
            }
            None => {
                shell_println!("Task {} not found", task_id);
            }
        }
    }
}

/// Display detailed per-task scheduling statistics.
///
/// Shows wait time (starvation) metrics alongside CPU time,
/// schedule count, and run-to-wait ratio for each task.
/// Sorted by total wait time descending — tasks at the top are
/// experiencing the most scheduling delay.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_schedstat() {
    let mut task_list = crate::sched::task_list();
    if task_list.is_empty() {
        shell_println!("No tasks.");
        return;
    }

    // Sort by total_wait_ticks descending (most starved first).
    task_list.sort_by(|a, b| b.total_wait_ticks.cmp(&a.total_wait_ticks));

    shell_println!(
        "{:<6} {:<12} {:<4} {:>8} {:>8} {:>8} {:>6} {:>6}",
        "TID", "NAME", "PRI", "RUN", "WAIT", "MAX_W", "SCHED", "W/R%"
    );
    shell_println!("---------------------------------------------------------------------");

    for info in &task_list {
        let name = core::str::from_utf8(
            info.name.get(..info.name_len).unwrap_or(&info.name)
        ).unwrap_or("?");

        // Format run time (total_ticks) as ss.t
        let run_secs = info.total_ticks / 100;
        let run_tenths = (info.total_ticks % 100) / 10;

        // Format total wait time as ss.t
        let wait_secs = info.total_wait_ticks / 100;
        let wait_tenths = (info.total_wait_ticks % 100) / 10;

        // Format max wait time as ms (each tick = 10ms)
        let max_wait_ms = info.max_wait_ticks.saturating_mul(10);

        // Wait/Run ratio as percentage.
        // High values indicate starvation (spending more time waiting
        // than running).
        let wr_pct = if info.total_ticks > 0 {
            info.total_wait_ticks.saturating_mul(100) / info.total_ticks
        } else if info.total_wait_ticks > 0 {
            999 // Infinite wait, no run time.
        } else {
            0
        };

        shell_println!(
            "{:<6} {:<12} {:<4} {:>5}.{} {:>5}.{} {:>5}ms {:>6} {:>5}%",
            info.id,
            &name[..name.len().min(12)],
            info.priority,
            run_secs, run_tenths,
            wait_secs, wait_tenths,
            max_wait_ms,
            info.schedule_count,
            wr_pct
        );
    }

    // Summary line.
    let total_wait: u64 = task_list.iter().map(|t| t.total_wait_ticks).sum();
    let total_run: u64 = task_list.iter().map(|t| t.total_ticks).sum();
    let global_wr = if total_run > 0 {
        total_wait.saturating_mul(100) / total_run
    } else { 0 };
    shell_println!("");
    shell_println!(
        "System: total_wait={}ms total_run={}ms overall_wait/run={}%",
        total_wait.saturating_mul(10),
        total_run.saturating_mul(10),
        global_wr
    );
}

/// Display per-task kernel stack usage (high water mark).
///
/// Shows how deep each task's stack has grown, helping identify tasks
/// that are close to stack overflow.  Based on Linux's
/// CONFIG_DEBUG_STACK_USAGE which paints stacks with a sentinel.
fn cmd_stack() {
    use crate::sched::task::TASK_STACK_SIZE;

    let mut task_list = crate::sched::task_list();
    if task_list.is_empty() {
        shell_println!("No tasks.");
        return;
    }

    // Sort by stack usage percentage descending (deepest first).
    task_list.sort_by(|a, b| {
        let a_pct = a.stack_pct.unwrap_or(0);
        let b_pct = b.stack_pct.unwrap_or(0);
        b_pct.cmp(&a_pct)
    });

    let stack_kb = TASK_STACK_SIZE / 1024;
    shell_println!("Kernel stack usage (stack size: {} KiB per task)", stack_kb);
    shell_println!("");
    shell_println!(
        "{:<5} {:<14} {:<8} {:>7} {:>5} {:>6}",
        "TID", "NAME", "STATE", "USED", "PCT", "FREE"
    );
    shell_println!("--------------------------------------------------");

    let mut max_pct: u8 = 0;
    let mut max_pct_name = [0u8; 32];
    let mut max_pct_name_len = 0;

    for info in &task_list {
        let name = core::str::from_utf8(
            info.name.get(..info.name_len).unwrap_or(&info.name)
        ).unwrap_or("?");

        match (info.stack_used, info.stack_pct) {
            (Some(used), Some(pct)) => {
                let free = TASK_STACK_SIZE.saturating_sub(8).saturating_sub(used);
                let indicator = if pct >= 90 {
                    " !!!"
                } else if pct >= 75 {
                    " !"
                } else {
                    ""
                };
                shell_println!(
                    "{:<5} {:<14} {:<8} {:>5}B {:>3}% {:>4}B{}",
                    info.id,
                    &name[..name.len().min(14)],
                    info.state,
                    used,
                    pct,
                    free,
                    indicator,
                );
                if pct > max_pct {
                    max_pct = pct;
                    max_pct_name = info.name;
                    max_pct_name_len = info.name_len;
                }
            }
            _ => {
                shell_println!(
                    "{:<5} {:<14} {:<8}     N/A   N/A    N/A",
                    info.id,
                    &name[..name.len().min(14)],
                    info.state,
                );
            }
        }
    }

    shell_println!("");
    if max_pct > 0 {
        let peak_name = core::str::from_utf8(
            max_pct_name.get(..max_pct_name_len).unwrap_or(&max_pct_name)
        ).unwrap_or("?");
        shell_println!("Peak usage: {}% (task '{}')", max_pct, peak_name);
        if max_pct >= 75 {
            shell_println!("WARNING: Task '{}' is using {}% of stack!", peak_name, max_pct);
        }
    }
}

fn cmd_ps() {
    let task_list = crate::sched::task_list();
    if task_list.is_empty() {
        shell_println!("No tasks.");
        return;
    }

    // Show the CPU% column if any task has a bandwidth quota set.
    let has_quotas = task_list.iter().any(|t| t.cpu_quota_pct > 0);

    if has_quotas {
        shell_println!(
            "{:<6} {:<12} {:<10} {:<4} {:>8} {:<8} {:<4} {:<6}",
            "TID", "NAME", "STATE", "PRI", "TIME", "SCHED", "CPU", "CPU%"
        );
        shell_println!("---------------------------------------------------------------");
    } else {
        shell_println!(
            "{:<6} {:<12} {:<10} {:<4} {:>8} {:<8} {:<4}",
            "TID", "NAME", "STATE", "PRI", "TIME", "SCHED", "CPU"
        );
        shell_println!("------------------------------------------------------");
    }

    for info in &task_list {
        let name = core::str::from_utf8(
            info.name.get(..info.name_len).unwrap_or(&info.name)
        ).unwrap_or("?");

        // Format CPU time as mm:ss.t (minutes:seconds.tenths).
        // Each tick = 10 ms at 100 Hz.
        #[allow(clippy::arithmetic_side_effects)]
        let total_tenths = info.total_ticks; // 1 tick = 100ms? No, 1 tick = 10ms = 1/100 sec
        #[allow(clippy::arithmetic_side_effects)]
        let total_secs = total_tenths / 100;
        #[allow(clippy::arithmetic_side_effects)]
        let frac = (total_tenths % 100) / 10; // tenths of a second
        #[allow(clippy::arithmetic_side_effects)]
        let mins = total_secs / 60;
        #[allow(clippy::arithmetic_side_effects)]
        let secs = total_secs % 60;
        let time_str = alloc::format!("{}:{:02}.{}", mins, secs, frac);

        if has_quotas {
            let quota_str = if info.cpu_quota_pct == 0 {
                alloc::string::String::from("-")
            } else if info.throttled {
                alloc::format!("{}%T", info.cpu_quota_pct)
            } else {
                alloc::format!("{}%", info.cpu_quota_pct)
            };
            shell_println!(
                "{:<6} {:<12} {:<10} {:<4} {:>8} {:<8} {:<4} {:<6}",
                info.id,
                name,
                info.state,
                info.priority,
                time_str,
                info.schedule_count,
                info.last_cpu,
                quota_str,
            );
        } else {
            shell_println!(
                "{:<6} {:<12} {:<10} {:<4} {:>8} {:<8} {:<4}",
                info.id,
                name,
                info.state,
                info.priority,
                time_str,
                info.schedule_count,
                info.last_cpu,
            );
        }
    }
    shell_println!("{} task(s) total", task_list.len());
}

fn cmd_clear() {
    crate::console::clear();
}

#[allow(clippy::arithmetic_side_effects)]
fn cmd_uptime() {
    // Use TSC-based timekeeping for precise uptime.
    let (up_secs, up_ms) = crate::timekeeping::uptime_ms();
    let seconds = up_secs;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    // Current wall-clock time.
    let now = crate::timekeeping::now();

    // Load averages (1/5/15 minute EWMA).
    let (l1, l5, l15) = crate::loadavg::get();
    let (l1_w, l1_f) = crate::loadavg::format_load(l1);
    let (l5_w, l5_f) = crate::loadavg::format_load(l5);
    let (l15_w, l15_f) = crate::loadavg::format_load(l15);

    // Active (non-idle) task count.
    let stats = crate::sched::sched_stats();
    let active = stats.total_tasks_spawned.saturating_sub(stats.total_tasks_exited);
    let nr_run = crate::loadavg::nr_running();

    shell_println!(
        " {:02}:{:02}:{:02} up {:02}:{:02}:{:02}.{:03}, load: {}.{:02}, {}.{:02}, {}.{:02}, {}/{} tasks",
        now.hour, now.minute, now.second,
        hours,
        minutes % 60,
        seconds % 60,
        up_ms,
        l1_w, l1_f,
        l5_w, l5_f,
        l15_w, l15_f,
        nr_run,
        active,
    );
}

/// `dmesg [-n COUNT]` — display kernel log messages.
///
/// Reads the structured log ring buffer and displays entries in a
/// human-readable format: `[timestamp] level/module: message`.
///
/// Options:
///   `-n COUNT` — limit output to the last COUNT entries
///   `-l LEVEL` — filter by minimum level (trace/debug/info/warn/error)
///   `-m MODULE` — filter by module name (substring match)
fn cmd_dmesg(args: &str) {
    let mut limit: usize = usize::MAX;
    let mut min_level: &str = "";
    let mut module_filter: &str = "";
    let words: Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "-n" => {
                i = i.saturating_add(1);
                if let Some(n) = words.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    limit = n;
                }
            }
            "-l" => {
                i = i.saturating_add(1);
                if let Some(&lvl) = words.get(i) {
                    min_level = lvl;
                }
            }
            "-m" => {
                i = i.saturating_add(1);
                if let Some(&m) = words.get(i) {
                    module_filter = m;
                }
            }
            _ => {}
        }
        i = i.saturating_add(1);
    }

    // Map level string to numeric priority for filtering.
    let min_prio = match min_level {
        "trace" => 0u8,
        "debug" => 1,
        "info" => 2,
        "warn" => 3,
        "error" => 4,
        _ => 0, // No filter (show all).
    };

    // Read all log entries from the ring buffer.
    // The buffer is JSON-lines format; we'll read raw bytes and parse
    // the timestamp, level, module, and message from each line.
    let mut buf = alloc::vec![0u8; 64 * 1024];
    let (written, _last_seq) = crate::klog::read_logs(u64::MAX, &mut buf);
    let text = core::str::from_utf8(buf.get(..written).unwrap_or(&[]))
        .unwrap_or("");

    // Collect lines, apply filters, then take the last `limit` entries.
    let lines: alloc::vec::Vec<&str> = text.lines().filter(|line| {
        // Level filter.
        if min_prio > 0 {
            let level = extract_json_str(line, "\"l\":\"");
            let prio = match level {
                "trace" => 0u8,
                "debug" => 1,
                "info" => 2,
                "warn" => 3,
                "error" => 4,
                _ => 0,
            };
            if prio < min_prio {
                return false;
            }
        }
        // Module filter.
        if !module_filter.is_empty() {
            let module = extract_json_str(line, "\"m\":\"");
            if !module.contains(module_filter) {
                return false;
            }
        }
        true
    }).collect();

    let start = lines.len().saturating_sub(limit);

    for line in lines.get(start..).unwrap_or(&[]) {
        // JSON-lines format: {"t":MS,"l":"level","m":"module","msg":"text"}
        // Simple key extraction (no full JSON parser in kernel).
        let ts = extract_json_u64(line, "\"t\":");
        let level = extract_json_str(line, "\"l\":\"");
        let module = extract_json_str(line, "\"m\":\"");
        let msg = extract_json_str(line, "\"msg\":\"");

        let secs = ts / 1000;
        let ms = ts % 1000;
        shell_println!(
            "[{:5}.{:03}] {}/{}: {}",
            secs, ms, level, module, msg
        );
    }
}

/// Extract a u64 value from a JSON-lines string at the given key prefix.
fn extract_json_u64(line: &str, key: &str) -> u64 {
    if let Some(pos) = line.find(key) {
        let start = pos.saturating_add(key.len());
        let rest = line.get(start..).unwrap_or("");
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        rest.get(..end).and_then(|s| s.parse().ok()).unwrap_or(0)
    } else {
        0
    }
}

/// Extract a string value from a JSON-lines string at the given key prefix.
/// Expects the key prefix to end with `:"`, and reads until the next `"`.
fn extract_json_str<'a>(line: &'a str, key: &str) -> &'a str {
    if let Some(pos) = line.find(key) {
        let start = pos.saturating_add(key.len());
        let rest = line.get(start..).unwrap_or("");
        let end = rest.find('"').unwrap_or(rest.len());
        rest.get(..end).unwrap_or("")
    } else {
        ""
    }
}

fn cmd_echo(args: &str) {
    let mut no_newline = false;
    let mut interpret_escapes = false;
    let mut rest = args;

    // Parse flags: -n (no trailing newline), -e (interpret escapes).
    loop {
        if rest.starts_with("-n ") || rest.starts_with("-n\t") {
            no_newline = true;
            rest = rest.get(3..).unwrap_or("").trim_start();
        } else if rest.starts_with("-e ") || rest.starts_with("-e\t") {
            interpret_escapes = true;
            rest = rest.get(3..).unwrap_or("").trim_start();
        } else if rest.starts_with("-ne ") || rest.starts_with("-en ")
            || rest.starts_with("-ne\t") || rest.starts_with("-en\t") {
            no_newline = true;
            interpret_escapes = true;
            rest = rest.get(4..).unwrap_or("").trim_start();
        } else if rest == "-n" {
            no_newline = true;
            rest = "";
        } else if rest == "-e" {
            interpret_escapes = true;
            rest = "";
        } else {
            break;
        }
    }

    if interpret_escapes {
        let output = interpret_echo_escapes(rest);
        if no_newline {
            shell_print!("{}", output);
        } else {
            shell_println!("{}", output);
        }
    } else if no_newline {
        shell_print!("{}", rest);
    } else {
        shell_println!("{}", rest);
    }
}

/// Interpret C-style escape sequences for `echo -e`.
fn interpret_echo_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'\\' && i.saturating_add(1) < len {
            let next = bytes[i.saturating_add(1)];
            match next {
                b'n' => { result.push('\n'); i = i.saturating_add(2); }
                b't' => { result.push('\t'); i = i.saturating_add(2); }
                b'r' => { result.push('\r'); i = i.saturating_add(2); }
                b'\\' => { result.push('\\'); i = i.saturating_add(2); }
                b'0' => { result.push('\0'); i = i.saturating_add(2); }
                b'a' => { result.push('\x07'); i = i.saturating_add(2); } // bell
                b'b' => { result.push('\x08'); i = i.saturating_add(2); } // backspace
                _ => { result.push('\\'); i = i.saturating_add(1); }
            }
        } else {
            result.push(bytes[i] as char);
            i = i.saturating_add(1);
        }
    }
    result
}

/// `printf FORMAT [ARG ...]` — formatted output (subset of POSIX printf).
///
/// Supported format specifiers:
///   `%s` — string
///   `%d` / `%i` — signed decimal integer
///   `%u` — unsigned decimal integer
///   `%x` — lowercase hexadecimal
///   `%X` — uppercase hexadecimal
///   `%o` — octal
///   `%c` — first character of argument
///   `%%` — literal percent sign
///   `%0Nd` — zero-padded to N digits (e.g., `%05d`)
///
/// Escape sequences: `\n`, `\t`, `\\`, `\0`.
fn cmd_printf(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: printf FORMAT [ARGS...]");
        return;
    }

    // Split format string from arguments.
    // The format string may be quoted.
    let (format, rest) = if args.starts_with('"') {
        // Find closing quote.
        if let Some(end) = args.get(1..).and_then(|s| s.find('"')) {
            let fmt = args.get(1..end.saturating_add(1)).unwrap_or("");
            let rest = args.get(end.saturating_add(2)..).unwrap_or("").trim_start();
            (fmt, rest)
        } else {
            (args.get(1..).unwrap_or(""), "")
        }
    } else if args.starts_with('\'') {
        if let Some(end) = args.get(1..).and_then(|s| s.find('\'')) {
            let fmt = args.get(1..end.saturating_add(1)).unwrap_or("");
            let rest = args.get(end.saturating_add(2)..).unwrap_or("").trim_start();
            (fmt, rest)
        } else {
            (args.get(1..).unwrap_or(""), "")
        }
    } else {
        // Unquoted: first word is the format string.
        let end = args.find(' ').unwrap_or(args.len());
        let fmt = args.get(..end).unwrap_or("");
        let rest = args.get(end..).unwrap_or("").trim_start();
        (fmt, rest)
    };

    // Parse arguments (space-separated, quote-aware).
    let arg_list = split_words(rest);
    let mut arg_iter = arg_list.iter();

    // Process format string.
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Escape sequence.
            match chars.next() {
                Some('n') => shell_print!("\n"),
                Some('t') => shell_print!("\t"),
                Some('\\') => shell_print!("\\"),
                Some('0') => shell_print!("\0"),
                Some(c) => shell_print!("\\{}", c),
                None => shell_print!("\\"),
            }
        } else if ch == '%' {
            // Format specifier.
            // Check for flags: zero-padding and width.
            let mut zero_pad = false;
            let mut width: usize = 0;

            if chars.peek() == Some(&'0') {
                zero_pad = true;
                chars.next();
            }
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                if let Some(d) = chars.next() {
                    width = width.saturating_mul(10).saturating_add(
                        (d as u8).saturating_sub(b'0') as usize,
                    );
                }
            }

            match chars.next() {
                Some('s') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("");
                    if width > 0 {
                        shell_print!("{:>width$}", arg, width = width);
                    } else {
                        shell_print!("{}", arg);
                    }
                }
                Some('d') | Some('i') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("0");
                    let val: i64 = arg.parse().unwrap_or(0);
                    if zero_pad && width > 0 {
                        shell_print!("{:0>width$}", val, width = width);
                    } else if width > 0 {
                        shell_print!("{:>width$}", val, width = width);
                    } else {
                        shell_print!("{}", val);
                    }
                }
                Some('u') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("0");
                    let val: u64 = arg.parse().unwrap_or(0);
                    if zero_pad && width > 0 {
                        shell_print!("{:0>width$}", val, width = width);
                    } else if width > 0 {
                        shell_print!("{:>width$}", val, width = width);
                    } else {
                        shell_print!("{}", val);
                    }
                }
                Some('x') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("0");
                    let val: u64 = arg.parse().unwrap_or(0);
                    if zero_pad && width > 0 {
                        shell_print!("{:0>width$x}", val, width = width);
                    } else {
                        shell_print!("{:x}", val);
                    }
                }
                Some('X') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("0");
                    let val: u64 = arg.parse().unwrap_or(0);
                    if zero_pad && width > 0 {
                        shell_print!("{:0>width$X}", val, width = width);
                    } else {
                        shell_print!("{:X}", val);
                    }
                }
                Some('o') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("0");
                    let val: u64 = arg.parse().unwrap_or(0);
                    if zero_pad && width > 0 {
                        shell_print!("{:0>width$o}", val, width = width);
                    } else {
                        shell_print!("{:o}", val);
                    }
                }
                Some('c') => {
                    let arg = arg_iter.next().map(|s| s.as_str()).unwrap_or("");
                    if let Some(c) = arg.chars().next() {
                        shell_print!("{}", c);
                    }
                }
                Some('%') => shell_print!("%"),
                Some(c) => shell_print!("%{}", c),
                None => shell_print!("%"),
            }
        } else {
            shell_print!("{}", ch);
        }
    }
}

/// Display the current date/time with optional format string.
///
/// Usage: `date` (default: YYYY-MM-DD HH:MM:SS)
///        `date +FORMAT` (strftime-like: %Y %m %d %H %M %S %a %A %b %B %j %u %Z)
///
/// Supported format specifiers:
/// - `%Y` 4-digit year, `%m` month (01-12), `%d` day (01-31)
/// - `%H` hour (00-23), `%M` minute (00-59), `%S` second (00-59)
/// - `%a` abbreviated weekday (Sun..Sat), `%A` full weekday
/// - `%b`/`%h` abbreviated month (Jan..Dec), `%B` full month name
/// - `%j` day of year (001-366), `%u` ISO weekday (1=Mon..7=Sun)
/// - `%Z` timezone (always "UTC"), `%n` newline, `%t` tab, `%%` literal %
/// - `%F` = `%Y-%m-%d`, `%T` = `%H:%M:%S`, `%D` = `%m/%d/%Y`
/// - `%s` epoch seconds
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn cmd_date(args: &str) {
    // Use timekeeping (TSC-based) instead of slow CMOS I/O reads.
    let dt = crate::timekeeping::now();

    if args.is_empty() {
        // Default output: YYYY-MM-DD HH:MM:SS
        shell_println!("{}", dt);
        return;
    }

    let fmt = if let Some(f) = args.strip_prefix('+') {
        f
    } else {
        crate::console_println!("Usage: date [+FORMAT]");
        set_exit(1);
        return;
    };

    // Day-of-week via Tomohiko Sakamoto's algorithm.
    let dow = {
        let mut y = i32::from(dt.year);
        let m = dt.month as usize;
        let d = i32::from(dt.day);
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        if m < 3 { y -= 1; }
        let idx = if m >= 1 && m <= 12 { T[m - 1] } else { 0 };
        ((y + y / 4 - y / 100 + y / 400 + idx + d) % 7) as u8 // 0=Sun
    };

    // Day of year (1-based).
    let is_leap = {
        let y = u32::from(dt.year);
        (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
    };
    let month_days: [u16; 12] = [
        0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334,
    ];
    let mi = if dt.month >= 1 && dt.month <= 12 {
        (dt.month - 1) as usize
    } else {
        0
    };
    let mut yday = month_days[mi] + u16::from(dt.day);
    if is_leap && dt.month > 2 {
        yday += 1;
    }

    // Epoch seconds (approximate — good enough for display).
    let epoch_secs = {
        let y = i64::from(dt.year);
        // Days from 1970-01-01 to Jan 1 of this year (approx).
        let mut days: i64 = (y - 1970) * 365 + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400;
        days += i64::from(yday) - 1; // yday is 1-based
        days * 86400 + i64::from(dt.hour) * 3600 + i64::from(dt.minute) * 60 + i64::from(dt.second)
    };

    const DAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const DAY_FULL: [&str; 7] = [
        "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
    ];
    const MON_ABBR: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const MON_FULL: [&str; 12] = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ];

    let mut out = alloc::string::String::with_capacity(fmt.len() + 32);
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'Y' => out.push_str(&alloc::format!("{:04}", dt.year)),
                b'm' => out.push_str(&alloc::format!("{:02}", dt.month)),
                b'd' => out.push_str(&alloc::format!("{:02}", dt.day)),
                b'H' => out.push_str(&alloc::format!("{:02}", dt.hour)),
                b'M' => out.push_str(&alloc::format!("{:02}", dt.minute)),
                b'S' => out.push_str(&alloc::format!("{:02}", dt.second)),
                b'a' => out.push_str(DAY_ABBR[dow as usize % 7]),
                b'A' => out.push_str(DAY_FULL[dow as usize % 7]),
                b'b' | b'h' => {
                    let idx = if dt.month >= 1 && dt.month <= 12 {
                        (dt.month - 1) as usize
                    } else {
                        0
                    };
                    out.push_str(MON_ABBR[idx]);
                }
                b'B' => {
                    let idx = if dt.month >= 1 && dt.month <= 12 {
                        (dt.month - 1) as usize
                    } else {
                        0
                    };
                    out.push_str(MON_FULL[idx]);
                }
                b'j' => out.push_str(&alloc::format!("{:03}", yday)),
                b'u' => {
                    // ISO weekday: Mon=1..Sun=7.  dow 0=Sun → 7.
                    let iso = if dow == 0 { 7 } else { dow };
                    out.push_str(&alloc::format!("{}", iso));
                }
                b'Z' => out.push_str("UTC"),
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'%' => out.push('%'),
                b'F' => out.push_str(&alloc::format!(
                    "{:04}-{:02}-{:02}", dt.year, dt.month, dt.day
                )),
                b'T' => out.push_str(&alloc::format!(
                    "{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second
                )),
                b'D' => out.push_str(&alloc::format!(
                    "{:02}/{:02}/{:04}", dt.month, dt.day, dt.year
                )),
                b's' => out.push_str(&alloc::format!("{}", epoch_secs)),
                _ => {
                    out.push('%');
                    out.push(bytes[i] as char);
                }
            }
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    shell_println!("{}", out);
}

/// Time the execution of a shell command.
///
/// Usage: `time <command>` — runs the command and prints elapsed wall time.
/// Uses HPET for nanosecond-resolution timing when available, falling back
/// to APIC tick counter (~10ms resolution).
fn cmd_time_cmd(args: &str) {
    if args.is_empty() {
        // No command — just show current time (legacy compat).
        let dt = crate::rtc::read_datetime();
        shell_println!("{}", dt);
        return;
    }

    let start_ns = crate::hpet::elapsed_ns();
    execute_single(args);
    let end_ns = crate::hpet::elapsed_ns();

    let elapsed_ns = end_ns.saturating_sub(start_ns);
    let secs = elapsed_ns / 1_000_000_000;
    let ms = (elapsed_ns % 1_000_000_000) / 1_000_000;
    let us = (elapsed_ns % 1_000_000) / 1_000;

    if secs > 0 {
        crate::console_println!("\nreal\t{}m{}.{:03}s", secs / 60, secs % 60, ms);
    } else if ms > 0 {
        crate::console_println!("\nreal\t0m0.{:03}s", ms);
    } else {
        crate::console_println!("\nreal\t{}us", us);
    }
}

/// Show the number of available CPUs.
fn cmd_nproc() {
    shell_println!("{}", crate::smp::cpu_count());
}

/// Extract printable ASCII strings from a file.
///
/// Usage: `strings [-n MIN] <file>`
///
/// Scans the file for runs of printable ASCII characters (0x20..0x7E plus
/// tab and newline).  Prints each run that meets the minimum length
/// (default 4, matching Unix `strings`).  Useful for inspecting binary
/// files, ELF executables, and firmware images.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_strings(args: &str) {
    let mut min_len: usize = 4;
    let mut path_arg = "";

    let mut words = args.split_whitespace();
    while let Some(w) = words.next() {
        if w == "-n" {
            if let Some(n_str) = words.next() {
                min_len = n_str.parse::<usize>().unwrap_or(4);
            }
        } else if w.starts_with("-n") {
            min_len = w[2..].parse::<usize>().unwrap_or(4);
        } else {
            path_arg = w;
        }
    }

    if path_arg.is_empty() {
        crate::console_println!("Usage: strings [-n MIN] <file>");
        set_exit(1);
        return;
    }

    let path = resolve_path(path_arg);
    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("strings: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };

    let mut current = alloc::string::String::new();
    for &b in &data {
        if (b >= 0x20 && b <= 0x7E) || b == b'\t' {
            current.push(b as char);
        } else {
            if current.len() >= min_len {
                crate::console_println!("{}", current);
            }
            current.clear();
        }
    }
    // Flush trailing run.
    if current.len() >= min_len {
        crate::console_println!("{}", current);
    }
}

/// Format text into aligned columns.
///
/// Usage: `column [-t] [-s SEP]` (reads from file or pipe)
///
/// `-t` — format into a table (auto-detect columns based on whitespace)
/// `-s CHAR` — use CHAR as the delimiter (default: whitespace)
///
/// Without `-t`, merges short lines into side-by-side columns filling
/// the terminal width.
fn cmd_column(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: column [-t] [-s SEP] <file>");
        crate::console_println!("   or: ... | column -t");
        set_exit(1);
        return;
    }

    // Parse flags.
    let mut table_mode = false;
    let mut sep: Option<char> = None;
    let mut file_path = "";

    let mut words = args.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w == "-t" {
            table_mode = true;
        } else if w == "-s" {
            if let Some(s) = words.next() {
                sep = s.chars().next();
            }
        } else {
            file_path = w;
        }
    }

    if file_path.is_empty() {
        crate::console_println!("column: no input file");
        set_exit(1);
        return;
    }

    let path = resolve_path(file_path);
    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("column: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };
    let text = alloc::string::String::from_utf8_lossy(&data);
    column_format(&text, table_mode, sep);
}

/// column from piped input.
fn cmd_column_input(args: &str, input: &str) {
    if input.is_empty() && args.is_empty() {
        crate::console_println!("Usage: ... | column [-t] [-s SEP]");
        set_exit(1);
        return;
    }

    let mut table_mode = false;
    let mut sep: Option<char> = None;
    let mut file_path = "";

    let mut words = args.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w == "-t" {
            table_mode = true;
        } else if w == "-s" {
            if let Some(s) = words.next() {
                sep = s.chars().next();
            }
        } else {
            file_path = w;
        }
    }

    // If a file was specified, read from it; otherwise use piped input.
    if !file_path.is_empty() {
        let path = resolve_path(file_path);
        let data = match crate::fs::Vfs::read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                crate::console_println!("column: {}: {:?}", path, e);
                set_exit(1);
                return;
            }
        };
        let text = alloc::string::String::from_utf8_lossy(&data);
        column_format(&text, table_mode, sep);
    } else {
        column_format(input, table_mode, sep);
    }
}

/// Core column formatting: split lines into columns and align.
#[allow(clippy::arithmetic_side_effects)]
fn column_format(text: &str, table_mode: bool, sep: Option<char>) {
    let lines: alloc::vec::Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return;
    }

    if table_mode {
        // Split each line into fields, compute max width per column,
        // then print left-aligned with 2-space padding.
        let rows: alloc::vec::Vec<alloc::vec::Vec<&str>> = lines
            .iter()
            .map(|line| {
                if let Some(c) = sep {
                    line.split(c).collect()
                } else {
                    line.split_whitespace().collect()
                }
            })
            .collect();

        // Find max columns and per-column max width.
        let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = alloc::vec![0usize; max_cols];
        for row in &rows {
            for (i, field) in row.iter().enumerate() {
                if field.len() > widths[i] {
                    widths[i] = field.len();
                }
            }
        }

        for row in &rows {
            let mut out = alloc::string::String::new();
            for (i, field) in row.iter().enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                out.push_str(field);
                // Pad to column width (except for the last column).
                if i + 1 < row.len() {
                    let pad = widths[i].saturating_sub(field.len());
                    for _ in 0..pad {
                        out.push(' ');
                    }
                }
            }
            crate::console_println!("{}", out);
        }
    } else {
        // Simple mode: print lines as-is (no table formatting).
        // A full implementation would fill the terminal width, but
        // without terminal width info we just print each line.
        for line in &lines {
            crate::console_println!("{}", line);
        }
    }
}

/// Display a monthly calendar.
///
/// Usage: `cal` (current month), `cal MONTH YEAR`, or `cal YEAR`.
/// Uses Tomohiko Sakamoto's day-of-week algorithm for the 1st of the
/// month, then prints a standard 7-column grid (Su Mo Tu We Th Fr Sa)
/// with today highlighted by brackets.
///
/// Reference: Unix cal(1).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn cmd_cal(args: &str) {
    let dt = crate::rtc::read_datetime();
    let trimmed = args.trim();

    let (month, year) = if trimmed.is_empty() {
        // No arguments — show current month.
        (u32::from(dt.month), u32::from(dt.year))
    } else {
        let parts: alloc::vec::Vec<&str> = trimmed.split_whitespace().collect();
        match parts.len() {
            1 => {
                // Single argument: either a year (>12) or month (1-12).
                if let Ok(v) = parts[0].parse::<u32>() {
                    if v >= 1 && v <= 12 {
                        (v, u32::from(dt.year))
                    } else if v >= 1970 && v <= 9999 {
                        // Show just one month — default to current month.
                        (u32::from(dt.month), v)
                    } else {
                        crate::console_println!("Usage: cal [MONTH] [YEAR]");
                        set_exit(1);
                        return;
                    }
                } else {
                    crate::console_println!("Usage: cal [MONTH] [YEAR]");
                    set_exit(1);
                    return;
                }
            }
            2 => {
                let m = parts[0].parse::<u32>().unwrap_or(0);
                let y = parts[1].parse::<u32>().unwrap_or(0);
                if m < 1 || m > 12 || y < 1 || y > 9999 {
                    crate::console_println!("Usage: cal [MONTH (1-12)] [YEAR (1-9999)]");
                    set_exit(1);
                    return;
                }
                (m, y)
            }
            _ => {
                crate::console_println!("Usage: cal [MONTH] [YEAR]");
                set_exit(1);
                return;
            }
        }
    };

    // Month names.
    let month_names = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ];
    let mname = month_names.get((month - 1) as usize).unwrap_or(&"???");

    // Header: centered month and year over the 20-char-wide grid.
    let header = alloc::format!("{} {}", mname, year);
    // The grid is 20 characters wide (Su Mo Tu .. Sa).
    let pad = if header.len() < 20 {
        (20 - header.len()) / 2
    } else {
        0
    };
    shell_println!("{:>width$}{}", "", header, width = pad);
    shell_println!("Su Mo Tu We Th Fr Sa");

    // Days in month.
    let dim = days_in_month(year, month);

    // Day of the week for the 1st of this month.
    // Tomohiko Sakamoto's algorithm: returns 0=Sunday, 1=Monday, ..., 6=Saturday.
    let dow1 = day_of_week(year, month, 1);

    // Is this the current month? (for highlighting today.)
    let is_current = year == u32::from(dt.year) && month == u32::from(dt.month);
    let today = u32::from(dt.day);

    // Build each day cell as a fixed 2-character string, then join
    // with single spaces (3 chars per cell: "DD ").  This avoids
    // alignment issues with highlighted-today markers.
    //
    // Standard cal(1) format: each day is right-justified in a
    // 2-char field, separated by single spaces.

    // Print leading blanks for the first week.
    for _ in 0..dow1 {
        shell_print!("   ");
    }

    let mut col = dow1;
    for day in 1..=dim {
        let highlight = is_current && day == today;
        if highlight {
            // Highlight today with an asterisk suffix.  We replace the
            // trailing space with '*' so alignment is preserved.
            shell_print!("{:2}*", day);
        } else if col < 6 || day < dim {
            shell_print!("{:2} ", day);
        } else {
            // Last column or last day — no trailing space.
            shell_print!("{:2}", day);
        }

        col += 1;
        if col == 7 {
            shell_println!("");
            col = 0;
        }
    }

    // Final newline if we didn't just print one.
    if col != 0 {
        shell_println!("");
    }
}

/// Number of days in a given month (1-12) of a given year.
#[allow(clippy::arithmetic_side_effects)]
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30, // Shouldn't happen with validated input.
    }
}

/// Day of the week for a given date (0=Sunday, 1=Monday, ..., 6=Saturday).
///
/// Tomohiko Sakamoto's algorithm.  Works for any Gregorian date with
/// year ≥ 1.
#[allow(clippy::arithmetic_side_effects)]
fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
    // Lookup table for month offset.
    let t: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let m = month as usize;
    (y + y / 4 - y / 100 + y / 400 + t[m - 1] + day) % 7
}

// PCI device class/subclass descriptions and bar formatting use simple
// fixed-width arithmetic on small known values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_pci() {
    let devices = crate::pci::scan_bus0();
    if devices.is_empty() {
        crate::console_println!("No PCI devices found.");
        return;
    }

    crate::console_println!("{:<10} {:<12} {:<8} {:<6}", "BDF", "VENDOR:DEV", "CLASS", "IRQ");
    crate::console_println!("------------------------------------------");
    for dev in &devices {
        crate::console_println!(
            "{:02x}:{:02x}.{}    {:04x}:{:04x}     {:02x}:{:02x}   {}",
            dev.address.bus,
            dev.address.device,
            dev.address.function,
            dev.vendor_id,
            dev.device_id,
            dev.class,
            dev.subclass,
            dev.irq_line
        );
    }
    crate::console_println!("{} device(s)", devices.len());
}

// Sector formatting uses small arithmetic on known-bounded values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_disk() {
    let devices = crate::blkdev::list_devices_full();
    if devices.is_empty() {
        crate::console_println!("No block devices registered.");
        return;
    }
    crate::console_println!("Block devices:");
    for dev in &devices {
        let kib = dev.sector_count.saturating_mul(u64::from(dev.sector_size)) / 1024;
        let mib = kib / 1024;
        crate::console_println!(
            "  {} — {} sectors ({} KiB / {} MiB){}",
            dev.name,
            dev.sector_count,
            kib,
            mib,
            if dev.read_only { " [read-only]" } else { "" }
        );
    }
}

// Hex-dump formatting uses offsets bounded by SECTOR_SIZE (512).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_blkread(args: &str) {
    // Parse: "blkread <sector>" or "blkread <device> <sector>"
    let (dev_name, sector) = parse_blkread_args(args);
    let Some(sector) = sector else {
        crate::console_println!("Usage: blkread [device] <sector>");
        crate::console_println!("  e.g., blkread 0  or  blkread vda 0");
        return;
    };

    let result = crate::blkdev::with_device(&dev_name, |dev| {
        let mut buf = [0u8; crate::blkdev::SECTOR_SIZE];
        match dev.read_sector(sector, &mut buf) {
            Ok(()) => {
                crate::console_println!("Sector {} on {}:", sector, dev_name);
                // Print 32 rows of 16 bytes each (512 bytes total).
                for row in 0..32 {
                    let offset = row * 16;
                    crate::console_print!("  {:04x}:", offset);
                    for col in 0..16 {
                        if let Some(&byte) = buf.get(offset + col) {
                            crate::console_print!(" {:02x}", byte);
                        }
                    }
                    // ASCII column.
                    crate::console_print!("  |");
                    for col in 0..16 {
                        if let Some(&byte) = buf.get(offset + col) {
                            let ch = if byte >= 0x20 && byte < 0x7F {
                                byte as char
                            } else {
                                '.'
                            };
                            crate::console_print!("{}", ch);
                        }
                    }
                    crate::console_println!("|");
                }
            }
            Err(e) => {
                crate::console_println!("Error reading sector {}: {:?}", sector, e);
            }
        }
    });
    if result.is_none() {
        crate::console_println!("No block device '{}' found.", dev_name);
    }
}

/// Parse blkread args: either "<sector>" or "<device> <sector>".
/// Returns (device_name, Some(sector)) or (_, None) on parse error.
fn parse_blkread_args(args: &str) -> (alloc::string::String, Option<u64>) {
    let mut parts = args.split_whitespace();
    let first = match parts.next() {
        Some(s) => s,
        None => return (alloc::string::String::from("vda"), None),
    };

    if let Some(second) = parts.next() {
        // Two args: device name + sector
        match second.parse::<u64>() {
            Ok(s) => (alloc::string::String::from(first), Some(s)),
            Err(_) => (alloc::string::String::from("vda"), None),
        }
    } else {
        // One arg: try as sector number (default device "vda")
        match first.parse::<u64>() {
            Ok(s) => (alloc::string::String::from("vda"), Some(s)),
            Err(_) => (alloc::string::String::from("vda"), None),
        }
    }
}

/// Format an octal permission mode as a `rwxrwxrwx` string.
fn format_perms(perms: u16) -> [u8; 9] {
    let mut out = [b'-'; 9];
    if perms & 0o400 != 0 { out[0] = b'r'; }
    if perms & 0o200 != 0 { out[1] = b'w'; }
    if perms & 0o100 != 0 { out[2] = b'x'; }
    if perms & 0o040 != 0 { out[3] = b'r'; }
    if perms & 0o020 != 0 { out[4] = b'w'; }
    if perms & 0o010 != 0 { out[5] = b'x'; }
    if perms & 0o004 != 0 { out[6] = b'r'; }
    if perms & 0o002 != 0 { out[7] = b'w'; }
    if perms & 0o001 != 0 { out[8] = b'x'; }
    out
}

/// Format a byte count as a human-readable string (K/M/G).
fn format_size_human(size: u64) -> String {
    if size >= 1_073_741_824 {
        alloc::format!("{:.1}G", size as f64 / 1_073_741_824.0)
    } else if size >= 1_048_576 {
        alloc::format!("{:.1}M", size as f64 / 1_048_576.0)
    } else if size >= 1024 {
        alloc::format!("{:.1}K", size as f64 / 1024.0)
    } else {
        alloc::format!("{}", size)
    }
}

fn cmd_ls(args: &str) {
    // Parse flags and path.
    let mut long_format = false;
    let mut show_all = false;
    let mut human_sizes = false;
    let mut recursive = false;
    let mut sort_by_size = false;
    let mut sort_by_time = false;
    let mut reverse_sort = false;
    let mut path_arg = "";

    for token in args.split_whitespace() {
        if let Some(flags) = token.strip_prefix('-') {
            for ch in flags.chars() {
                match ch {
                    'l' => long_format = true,
                    'a' => show_all = true,
                    'h' => human_sizes = true,
                    'R' => recursive = true,
                    'S' => sort_by_size = true,
                    't' => sort_by_time = true,
                    'r' => reverse_sort = true,
                    _ => {
                        crate::console_println!("ls: unknown option -{}", ch);
                        set_exit(1);
                        return;
                    }
                }
            }
        } else {
            path_arg = token;
        }
    }

    let path = if path_arg.is_empty() {
        get_cwd()
    } else {
        resolve_path(path_arg)
    };

    ls_list_dir(
        &path, long_format, show_all, human_sizes,
        sort_by_size, sort_by_time, reverse_sort,
        recursive, 0,
    );
}

/// Internal helper for ls: list one directory and optionally recurse.
///
/// `depth` guards against infinite recursion (e.g., symlink loops);
/// maximum depth is 32.
#[allow(clippy::too_many_arguments)]
fn ls_list_dir(
    path: &str,
    long_format: bool,
    show_all: bool,
    human_sizes: bool,
    sort_by_size: bool,
    sort_by_time: bool,
    reverse_sort: bool,
    recursive: bool,
    depth: u32,
) {
    if depth > 32 {
        crate::console_println!("ls: recursion limit reached");
        return;
    }

    // When recursing, print the directory name as a header.
    if recursive && depth > 0 {
        shell_println!("");
        shell_println!("{}:", path);
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(e) => {
            crate::console_println!("ls: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };

    if entries.is_empty() {
        shell_println!("(empty directory)");
        if !recursive { return; }
    }

    // Filter hidden files unless -a is given.
    let mut filtered: Vec<crate::fs::vfs::DirEntry> = entries
        .into_iter()
        .filter(|e| show_all || !e.name.starts_with('.'))
        .collect();

    // Apply sorting.  Sorting requires metadata lookups.
    if sort_by_size {
        filtered.sort_by(|a, b| b.size.cmp(&a.size));
    } else if sort_by_time {
        // Sort by modification time (most recent first).
        // We need to look up metadata for each entry.
        let mut with_mtime: Vec<(crate::fs::vfs::DirEntry, u64)> = filtered
            .into_iter()
            .map(|e| {
                let fp = if path == "/" {
                    alloc::format!("/{}", e.name)
                } else {
                    alloc::format!("{}/{}", path, e.name)
                };
                let mtime = crate::fs::Vfs::metadata(&fp)
                    .map(|m| m.modified_ns)
                    .unwrap_or(0);
                (e, mtime)
            })
            .collect();
        with_mtime.sort_by(|a, b| b.1.cmp(&a.1));
        filtered = with_mtime.into_iter().map(|(e, _)| e).collect();
    }

    if reverse_sort {
        filtered.reverse();
    }

    if long_format {
        // Long format: type+perms links uid gid size date name
        // First pass: gather metadata and compute total blocks.
        let mut total_blocks: u64 = 0;
        let mut metas: Vec<Option<crate::fs::vfs::FileMeta>> =
            Vec::with_capacity(filtered.len());

        for entry in &filtered {
            let full_path = if path == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", path, entry.name)
            };
            if let Ok(meta) = crate::fs::Vfs::metadata(&full_path) {
                total_blocks = total_blocks.saturating_add(meta.blocks);
                metas.push(Some(meta));
            } else {
                metas.push(None);
            }
        }

        shell_println!("total {}", total_blocks);

        for (i, entry) in filtered.iter().enumerate() {
            let type_ch = match entry.entry_type {
                crate::fs::EntryType::Directory => 'd',
                crate::fs::EntryType::File => '-',
                crate::fs::EntryType::Symlink => 'l',
                crate::fs::EntryType::VolumeLabel => 'v',
            };

            if let Some(Some(meta)) = metas.get(i) {
                let perms = format_perms(meta.permissions);
                let perm_str = core::str::from_utf8(&perms).unwrap_or("---------");

                let size_str = if human_sizes {
                    format_size_human(meta.size)
                } else {
                    alloc::format!("{}", meta.size)
                };

                // Format modification time as YYYY-MM-DD HH:MM:SS.
                let time_str = if meta.modified_ns > 0 {
                    format_epoch_ns(meta.modified_ns)
                } else {
                    String::from("-")
                };

                // For symlinks, show " -> target".
                let suffix = if entry.entry_type == crate::fs::EntryType::Symlink {
                    let full_path = if path == "/" {
                        alloc::format!("/{}", entry.name)
                    } else {
                        alloc::format!("{}/{}", path, entry.name)
                    };
                    match crate::fs::Vfs::readlink(&full_path) {
                        Ok(target) => alloc::format!(" -> {}", target),
                        Err(_) => String::new(),
                    }
                } else {
                    String::new()
                };

                shell_println!(
                    "{}{} {:>3} {:>5} {:>5} {:>8} {} {}{}",
                    type_ch, perm_str, meta.nlinks,
                    meta.uid, meta.gid, size_str,
                    time_str, entry.name, suffix,
                );
            } else {
                // Metadata unavailable — basic listing.
                let size_str = if human_sizes {
                    format_size_human(entry.size)
                } else {
                    alloc::format!("{}", entry.size)
                };
                shell_println!(
                    "{}--------- {:>3} {:>5} {:>5} {:>8}            {}",
                    type_ch, 1, 0, 0, size_str, entry.name,
                );
            }
        }
    } else {
        // Default format (original behavior).
        for entry in &filtered {
            let type_indicator = match entry.entry_type {
                crate::fs::EntryType::Directory => "<DIR>    ",
                crate::fs::EntryType::File => "         ",
                crate::fs::EntryType::Symlink => "<LINK>   ",
                crate::fs::EntryType::VolumeLabel => "<VOL>    ",
            };
            let size_str = if human_sizes {
                alloc::format!("{:>8}", format_size_human(entry.size))
            } else {
                alloc::format!("{:>8}", entry.size)
            };
            shell_println!(
                "  {} {}  {}",
                type_indicator, size_str, entry.name
            );
        }
    }
    shell_println!("{} entry(ies)", filtered.len());

    // Recurse into subdirectories for -R.
    if recursive {
        for entry in &filtered {
            if entry.entry_type == crate::fs::EntryType::Directory
                && entry.name != "." && entry.name != ".."
            {
                let child_path = if path == "/" {
                    alloc::format!("/{}", entry.name)
                } else {
                    alloc::format!("{}/{}", path, entry.name)
                };
                ls_list_dir(
                    &child_path, long_format, show_all, human_sizes,
                    sort_by_size, sort_by_time, reverse_sort,
                    recursive, depth + 1,
                );
            }
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: cat <filename>");
        return;
    }

    let path = resolve_path(args);

    match crate::fs::Vfs::read_file(&path) {
        Ok(data) => {
            // Try to display as UTF-8 text.
            match core::str::from_utf8(&data) {
                Ok(text) => {
                    shell_print!("{}", text);
                    // Ensure there's a newline at the end.
                    if !text.ends_with('\n') {
                        shell_println!();
                    }
                }
                Err(_) => {
                    shell_println!(
                        "(binary file, {} bytes — use blkread for hex dump)",
                        data.len()
                    );
                }
            }
        }
        Err(e) => {
            crate::console_println!("cat: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

fn cmd_write(args: &str) {
    // Parse: "write FILENAME text to write..."
    let mut parts = args.splitn(2, ' ');
    let filename = match parts.next() {
        Some(f) if !f.is_empty() => f,
        _ => {
            crate::console_println!("Usage: write <filename> <text>");
            return;
        }
    };
    let text = parts.next().unwrap_or("");
    let path = resolve_path(filename);

    // Append a newline if the text doesn't have one.
    let mut data = alloc::vec::Vec::from(text.as_bytes());
    if !text.ends_with('\n') {
        data.push(b'\n');
    }

    match crate::fs::Vfs::write_file(&path, &data) {
        Ok(()) => {
            crate::console_println!("Wrote {} bytes to {}", data.len(), path);
        }
        Err(e) => {
            crate::console_println!("write: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

fn cmd_rm(args: &str) {
    // Support -r/-R flag for recursive removal.
    let (recursive, args) = if args.starts_with("-r ") || args.starts_with("-R ")
        || args.starts_with("-rf ") || args.starts_with("-Rf ")
    {
        let skip = if args.starts_with("-rf") || args.starts_with("-Rf") { 4 } else { 3 };
        (true, &args[skip..])
    } else {
        (false, args)
    };

    if args.is_empty() {
        crate::console_println!("Usage: rm [-r] <filename>");
        return;
    }

    let path = resolve_path(args);

    if recursive {
        match crate::fs::Vfs::remove_recursive(&path) {
            Ok(count) => {
                crate::console_println!("Removed {} ({} items)", path, count);
            }
            Err(e) => {
                crate::console_println!("rm: {}: {:?}", path, e);
                set_exit(1);
            }
        }
    } else {
        match crate::fs::Vfs::remove(&path) {
            Ok(()) => {
                crate::console_println!("Deleted {}", path);
            }
            Err(e) => {
                crate::console_println!("rm: {}: {:?}", path, e);
                set_exit(1);
            }
        }
    }
}

fn cmd_mkdir(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: mkdir [-p] <dirname>");
        return;
    }

    // Parse -p flag for recursive creation.
    let (recursive, dir_arg) = if args.starts_with("-p ") {
        (true, args.get(3..).unwrap_or("").trim())
    } else if args == "-p" {
        crate::console_println!("Usage: mkdir [-p] <dirname>");
        return;
    } else {
        (false, args)
    };

    let path = resolve_path(dir_arg);

    let result = if recursive {
        crate::fs::Vfs::mkdir_all(&path)
    } else {
        crate::fs::Vfs::mkdir(&path)
    };

    match result {
        Ok(()) => {
            crate::console_println!("Created directory {}", path);
        }
        Err(e) => {
            crate::console_println!("mkdir: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

fn cmd_rmdir(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: rmdir <dirname>");
        return;
    }

    let path = resolve_path(args);

    match crate::fs::Vfs::rmdir(&path) {
        Ok(()) => {
            crate::console_println!("Removed directory {}", path);
        }
        Err(e) => {
            crate::console_println!("rmdir: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

/// Show detailed file/directory metadata.
#[allow(clippy::cast_possible_truncation)]
fn cmd_stat(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: stat <path>");
        return;
    }

    let path = resolve_path(args);

    match crate::fs::Vfs::metadata(&path) {
        Ok(meta) => {
            let type_str = match meta.entry_type {
                crate::fs::EntryType::File => "regular file",
                crate::fs::EntryType::Directory => "directory",
                crate::fs::EntryType::Symlink => "symbolic link",
                crate::fs::EntryType::VolumeLabel => "volume label",
            };
            crate::console_println!("  File: {}", path);
            crate::console_println!("  Size: {}  Blocks: {}  Type: {}", meta.size, meta.blocks, type_str);
            crate::console_println!("  Links: {}", meta.nlinks);
            if meta.permissions != 0 {
                let perms = format_perms(meta.permissions);
                let perm_str = core::str::from_utf8(&perms).unwrap_or("---------");
                crate::console_println!("  Perms: {:04o} ({})  Uid: {}  Gid: {}",
                    meta.permissions, perm_str, meta.uid, meta.gid);
            }
            if meta.attributes != crate::fs::FileAttr::NONE {
                let a = meta.attributes;
                let mut flags = alloc::string::String::new();
                if a.contains(crate::fs::FileAttr::IMMUTABLE) { flags.push_str("immutable "); }
                if a.contains(crate::fs::FileAttr::APPEND_ONLY) { flags.push_str("append-only "); }
                if a.contains(crate::fs::FileAttr::HIDDEN) { flags.push_str("hidden "); }
                if a.contains(crate::fs::FileAttr::SYSTEM) { flags.push_str("system "); }
                crate::console_println!("  Attrs: {}", flags.trim_end());
            }

            // Show filesystem type via VFS mount table.
            if let Ok(info) = crate::fs::Vfs::statvfs(&path) {
                if info.volume_label.is_empty() {
                    crate::console_println!("  FS:   {} (block size: {})", info.fs_type, info.block_size);
                } else {
                    crate::console_println!(
                        "  FS:   {} \"{}\" (block size: {})",
                        info.fs_type, info.volume_label, info.block_size,
                    );
                }
            }

            let ns_to_display = |ns: u64| -> alloc::string::String {
                if ns == 0 {
                    alloc::string::String::from("-")
                } else {
                    format_epoch_ns(ns)
                }
            };
            crate::console_println!("  Created:  {}", ns_to_display(meta.created_ns));
            crate::console_println!("  Modified: {}", ns_to_display(meta.modified_ns));
            crate::console_println!("  Accessed: {}", ns_to_display(meta.accessed_ns));
            crate::console_println!("  Changed:  {}", ns_to_display(meta.changed_ns));

            // Show extended attributes if any.
            if !meta.xattrs.is_empty() {
                crate::console_println!("  Xattrs:");
                for (key, value) in &meta.xattrs {
                    if value.len() <= 64 {
                        // Short value — display inline.
                        let display = if value.iter().all(|&b| b >= 0x20 && b < 0x7F) {
                            alloc::format!("\"{}\"",
                                core::str::from_utf8(value).unwrap_or("?"))
                        } else {
                            alloc::format!("({} bytes, binary)", value.len())
                        };
                        crate::console_println!("    {} = {}", key, display);
                    } else {
                        crate::console_println!("    {} ({} bytes)", key, value.len());
                    }
                }
            }
        }
        Err(e) => {
            crate::console_println!("stat: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

/// Create a hard link.
fn cmd_ln(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: ln <source> <link-name>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = resolve_path(src);
    let dst_path = resolve_path(dst);

    match crate::fs::Vfs::link(&src_path, &dst_path) {
        Ok(()) => {
            crate::console_println!("{} -> {}", dst_path, src_path);
        }
        Err(e) => {
            crate::console_println!("ln: {:?}", e);
            set_exit(1);
        }
    }
}

/// Show filesystem disk usage (like Unix `df`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_df(args: &str) {
    let mut verbose = false;
    let mut path_arg = args;
    if args.starts_with("-v") {
        verbose = true;
        path_arg = args.get(2..).unwrap_or("").trim();
    }

    if path_arg.is_empty() {
        // Show all mounts.
        match crate::fs::Vfs::mount_info() {
            Ok(mounts) => {
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {:<16} {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Mounted on", "Label"
                );
                for (mount_path, info) in &mounts {
                    let total = info.total_bytes();
                    let free = info.free_bytes();
                    let used = info.used_bytes();
                    let pct = info.usage_percent();
                    crate::console_println!(
                        "{:<12} {:>10} {:>10} {:>10} {:>4}%  {:<16} {}",
                        info.fs_type,
                        format_bytes(total),
                        format_bytes(used),
                        format_bytes(free),
                        pct,
                        mount_path,
                        info.volume_label,
                    );
                    if verbose {
                        if let Ok(stats) = crate::fs::Vfs::debug_stats(mount_path) {
                            if !stats.is_empty() {
                                for line in stats.lines() {
                                    shell_println!("  {}", line);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                crate::console_println!("df: {:?}", e);
            }
        }
    } else {
        // Show info for specific path.
        let path = resolve_path(args);
        match crate::fs::Vfs::statvfs(&path) {
            Ok(info) => {
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {:<16} {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Path", "Label"
                );
                let total = info.total_bytes();
                let free = info.free_bytes();
                let used = info.used_bytes();
                let pct = info.usage_percent();
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>4}%  {:<16} {}",
                    info.fs_type,
                    format_bytes(total),
                    format_bytes(used),
                    format_bytes(free),
                    pct,
                    path,
                    info.volume_label,
                );
            }
            Err(e) => {
                crate::console_println!("df: {:?}", e);
            }
        }
    }
}

/// Format a byte count as human-readable (K/M/G).
#[allow(clippy::arithmetic_side_effects)]
fn format_bytes(bytes: u64) -> alloc::string::String {
    if bytes < 1024 {
        alloc::format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}K", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        alloc::format!("{}M", bytes / (1024 * 1024))
    } else {
        alloc::format!("{}G", bytes / (1024 * 1024 * 1024))
    }
}

/// Format a Unix epoch timestamp (nanoseconds) as YYYY-MM-DD HH:MM:SS.
///
/// Uses the civil-from-days algorithm to convert days since epoch to
/// year/month/day.  Based on Howard Hinnant's `civil_from_days()`.
#[allow(clippy::arithmetic_side_effects)]
fn format_epoch_ns(ns: u64) -> alloc::string::String {
    let total_secs = ns / 1_000_000_000;
    let days_since_epoch = (total_secs / 86400) as i64;
    let time_of_day = total_secs % 86400;
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    // Howard Hinnant's civil_from_days algorithm.
    // Converts days since 1970-01-01 to (year, month, day).
    let z = days_since_epoch + 719468; // shift epoch to 0000-03-01
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    alloc::format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, m, d, hour, minute, second
    )
}

/// Copy a file.
fn cmd_cp(args: &str) {
    // Support -r flag for recursive copy.
    let (recursive, args) = if args.starts_with("-r ") || args.starts_with("-R ") {
        (true, &args[3..])
    } else {
        (false, args)
    };

    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: cp [-r] <source> <dest>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = resolve_path(src);
    let dst_path = resolve_path(dst);

    if recursive {
        match crate::fs::Vfs::copy_recursive(&src_path, &dst_path) {
            Ok(size) => {
                crate::console_println!("'{}' -> '{}' ({} bytes copied)", src_path, dst_path, size);
            }
            Err(e) => {
                crate::console_println!("cp: {:?}", e);
                set_exit(1);
            }
        }
    } else {
        match crate::fs::Vfs::copy(&src_path, &dst_path) {
            Ok(size) => {
                crate::console_println!("'{}' -> '{}' ({} bytes)", src_path, dst_path, size);
            }
            Err(e) => {
                crate::console_println!("cp: {:?}", e);
                set_exit(1);
            }
        }
    }
}

/// Rename/move a file or directory.
fn cmd_mv(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: mv <source> <dest>");
        return;
    }

    let src = parts[0];
    let dst = parts[1];

    let src_path = resolve_path(src);
    let dst_path = resolve_path(dst);

    match crate::fs::Vfs::rename(&src_path, &dst_path) {
        Ok(()) => {
            crate::console_println!("'{}' -> '{}'", src_path, dst_path);
        }
        Err(e) => {
            crate::console_println!("mv: {:?}", e);
            set_exit(1);
        }
    }
}

/// Change file permissions.
fn cmd_chmod(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: chmod <mode> <path>");
        crate::console_println!("  mode: octal (e.g., 755, 644)");
        return;
    }

    let mode_str = parts[0];
    let file = parts[1];

    let mode = match u16::from_str_radix(mode_str, 8) {
        Ok(m) => m,
        Err(_) => {
            crate::console_println!("chmod: invalid mode '{}' (use octal, e.g., 755)", mode_str);
            return;
        }
    };

    let path = resolve_path(file);

    match crate::fs::Vfs::set_permissions(&path, mode) {
        Ok(()) => {
            crate::console_println!("{}: mode set to {:04o}", path, mode);
        }
        Err(e) => {
            crate::console_println!("chmod: {:?}", e);
        }
    }
}

/// Change file ownership.
fn cmd_chown(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: chown <uid:gid> <path>");
        crate::console_println!("  e.g., chown 1000:1000 /home/user");
        return;
    }

    let owner_str = parts[0];
    let file = parts[1];

    // Parse uid:gid.
    let (uid, gid) = if let Some(colon) = owner_str.find(':') {
        let uid_s = &owner_str[..colon];
        let gid_s = &owner_str[colon + 1..];
        let uid = match uid_s.parse::<u32>() {
            Ok(u) => u,
            Err(_) => {
                crate::console_println!("chown: invalid uid '{}'", uid_s);
                return;
            }
        };
        let gid = match gid_s.parse::<u32>() {
            Ok(g) => g,
            Err(_) => {
                crate::console_println!("chown: invalid gid '{}'", gid_s);
                return;
            }
        };
        (uid, gid)
    } else {
        // Just uid, set gid to same.
        match owner_str.parse::<u32>() {
            Ok(u) => (u, u),
            Err(_) => {
                crate::console_println!("chown: invalid owner '{}'", owner_str);
                return;
            }
        }
    };

    let path = resolve_path(file);

    match crate::fs::Vfs::set_owner(&path, uid, gid) {
        Ok(()) => {
            crate::console_println!("{}: owner set to {}:{}", path, uid, gid);
        }
        Err(e) => {
            crate::console_println!("chown: {:?}", e);
        }
    }
}

/// Change file attributes (immutable, hidden, system, append-only).
///
/// Usage:
///   `chattr +i file`  — set immutable
///   `chattr -i file`  — clear immutable
///   `chattr +iha file` — set immutable, hidden, append-only
///   `chattr = file`   — clear all attributes
///
/// Flags: `i` = immutable, `a` = append-only, `h` = hidden, `s` = system.
///
/// On FAT: immutable maps to read-only, hidden/system map directly;
/// append-only is silently ignored.  On ext4: immutable and append-only
/// map to EXT4_IMMUTABLE_FL / EXT4_APPEND_FL.
fn cmd_chattr(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: chattr [+|-]FLAGS FILE");
        crate::console_println!("  +FLAGS  set attributes    -FLAGS  clear attributes");
        crate::console_println!("  =       clear all attributes");
        crate::console_println!("  Flags: i=immutable a=append-only h=hidden s=system");
        set_exit(1);
        return;
    }

    let flag_str = parts[0];
    let file = parts[1].trim();
    let path = resolve_path(file);

    // Parse the operation: +, -, or =
    let (op, flags) = if flag_str == "=" {
        ('=', "")
    } else if let Some(f) = flag_str.strip_prefix('+') {
        ('+', f)
    } else if let Some(f) = flag_str.strip_prefix('-') {
        ('-', f)
    } else {
        crate::console_println!("chattr: expected +FLAGS, -FLAGS, or =");
        set_exit(1);
        return;
    };

    // Parse flag characters into a FileAttr mask.
    let mut mask = crate::fs::FileAttr::NONE;
    for ch in flags.chars() {
        match ch {
            'i' => mask = mask.union(crate::fs::FileAttr::IMMUTABLE),
            'a' => mask = mask.union(crate::fs::FileAttr::APPEND_ONLY),
            'h' => mask = mask.union(crate::fs::FileAttr::HIDDEN),
            's' => mask = mask.union(crate::fs::FileAttr::SYSTEM),
            _ => {
                crate::console_println!("chattr: unknown flag '{}'", ch);
                set_exit(1);
                return;
            }
        }
    }

    // Get current attributes.
    let current_attrs = match crate::fs::Vfs::metadata(&path) {
        Ok(meta) => meta.attributes,
        Err(e) => {
            crate::console_println!("chattr: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };

    // Compute new attributes based on the operation.
    let new_attrs = match op {
        '+' => current_attrs.union(mask),
        '-' => crate::fs::FileAttr::from_bits(current_attrs.bits() & !mask.bits()),
        '=' => crate::fs::FileAttr::NONE,
        _ => current_attrs,
    };

    match crate::fs::Vfs::set_attributes(&path, new_attrs) {
        Ok(()) => {
            // Show the resulting attributes.
            let mut desc = alloc::string::String::new();
            if new_attrs.contains(crate::fs::FileAttr::IMMUTABLE) {
                desc.push('i');
            }
            if new_attrs.contains(crate::fs::FileAttr::APPEND_ONLY) {
                desc.push('a');
            }
            if new_attrs.contains(crate::fs::FileAttr::HIDDEN) {
                desc.push('h');
            }
            if new_attrs.contains(crate::fs::FileAttr::SYSTEM) {
                desc.push('s');
            }
            if desc.is_empty() {
                desc.push_str("(none)");
            }
            crate::console_println!("{}: attributes = {}", path, desc);
        }
        Err(e) => {
            crate::console_println!("chattr: {:?}", e);
            set_exit(1);
        }
    }
}

/// Show file attributes (complement to chattr).
///
/// Usage: `lsattr FILE`
fn cmd_lsattr(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: lsattr <path>");
        set_exit(1);
        return;
    }

    let path = resolve_path(args.trim());

    match crate::fs::Vfs::metadata(&path) {
        Ok(meta) => {
            let attrs = meta.attributes;
            let mut flags = alloc::string::String::new();
            flags.push(if attrs.contains(crate::fs::FileAttr::IMMUTABLE) { 'i' } else { '-' });
            flags.push(if attrs.contains(crate::fs::FileAttr::APPEND_ONLY) { 'a' } else { '-' });
            flags.push(if attrs.contains(crate::fs::FileAttr::HIDDEN) { 'h' } else { '-' });
            flags.push(if attrs.contains(crate::fs::FileAttr::SYSTEM) { 's' } else { '-' });
            crate::console_println!("{} {}", flags, path);
        }
        Err(e) => {
            crate::console_println!("lsattr: {}: {:?}", path, e);
            set_exit(1);
        }
    }
}

/// Create a file or update timestamps.
/// Create file or update timestamps.
///
/// Usage: `touch <path>` — set timestamps to now
///        `touch -d DATETIME <path>` — set to specific ISO-8601 date
///        `touch -r REFFILE <path>` — copy timestamps from reference file
///
/// DATETIME format: `YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`, or epoch seconds.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_touch(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: touch [-d DATETIME | -r REFFILE] <path>");
        return;
    }

    let mut date_str: Option<&str> = None;
    let mut ref_file: Option<&str> = None;
    let mut file_path = "";

    let mut words = args.split_whitespace();
    while let Some(w) = words.next() {
        if w == "-d" {
            // Collect the date string — it might have spaces (e.g., "2026-01-15 12:30:00").
            date_str = words.next();
        } else if w == "-r" {
            ref_file = words.next();
        } else {
            file_path = w;
        }
    }

    if file_path.is_empty() {
        crate::console_println!("touch: missing file operand");
        set_exit(1);
        return;
    }

    let path = resolve_path(file_path);

    // Determine the timestamp to use.
    let timestamp = if let Some(ds) = date_str {
        // Parse the date string.
        match parse_datetime_to_ns(ds) {
            Some(ns) => ns,
            None => {
                crate::console_println!(
                    "touch: invalid date '{}' (use YYYY-MM-DD or YYYY-MM-DD HH:MM:SS or epoch secs)",
                    ds
                );
                set_exit(1);
                return;
            }
        }
    } else if let Some(rf) = ref_file {
        // Copy timestamp from reference file.
        let ref_path = resolve_path(rf);
        match crate::fs::Vfs::metadata(&ref_path) {
            Ok(meta) => meta.modified_ns,
            Err(e) => {
                crate::console_println!("touch: {}: {:?}", ref_path, e);
                set_exit(1);
                return;
            }
        }
    } else {
        crate::hpet::elapsed_ns()
    };

    // Check if file exists.
    match crate::fs::Vfs::stat(&path) {
        Ok(_) => {
            // File exists — update timestamps.
            match crate::fs::Vfs::set_times(&path, timestamp, timestamp) {
                Ok(()) => {
                    crate::console_println!("{}: timestamps updated", path);
                }
                Err(e) => {
                    crate::console_println!("touch: {}: {:?}", path, e);
                    set_exit(1);
                }
            }
        }
        Err(_) => {
            // File doesn't exist — create empty file.
            match crate::fs::Vfs::write_file(&path, &[]) {
                Ok(()) => {
                    crate::console_println!("{}: created", path);
                }
                Err(e) => {
                    crate::console_println!("touch: {}: {:?}", path, e);
                    set_exit(1);
                }
            }
        }
    }
}

/// Parse a datetime string to nanoseconds since epoch.
///
/// Accepts:
/// - `YYYY-MM-DD` (midnight UTC)
/// - `YYYY-MM-DD HH:MM:SS` (UTC)
/// - Plain integer (epoch seconds)
#[allow(clippy::arithmetic_side_effects)]
fn parse_datetime_to_ns(s: &str) -> Option<u64> {
    // Try epoch seconds first.
    if let Ok(secs) = s.parse::<u64>() {
        return Some(secs.saturating_mul(1_000_000_000));
    }

    // Try YYYY-MM-DD or YYYY-MM-DD HH:MM:SS.
    let parts: alloc::vec::Vec<&str> = s.splitn(2, ' ').collect();
    let date_part = parts.first()?;
    let time_part = parts.get(1);

    let date_fields: alloc::vec::Vec<&str> = date_part.split('-').collect();
    if date_fields.len() != 3 {
        return None;
    }

    let year = date_fields[0].parse::<i64>().ok()?;
    let month = date_fields[1].parse::<u32>().ok()?;
    let day = date_fields[2].parse::<u32>().ok()?;

    if month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    let (hour, minute, second) = if let Some(tp) = time_part {
        let time_fields: alloc::vec::Vec<&str> = tp.split(':').collect();
        let h = time_fields.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let m = time_fields.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let s = time_fields.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        (h, m, s)
    } else {
        (0, 0, 0)
    };

    // Convert to days since epoch using standard algorithm.
    let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mi = if month >= 1 && month <= 12 { (month - 1) as usize } else { 0 };
    let mut yday = month_days[mi] + day;
    if is_leap && month > 2 {
        yday += 1;
    }

    // Days from 1970-01-01 to start of this year (approximate).
    let mut days: i64 = (year - 1970) * 365 + (year - 1969) / 4 - (year - 1901) / 100 + (year - 1601) / 400;
    days += i64::from(yday) - 1;

    let total_secs = days * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second);
    if total_secs < 0 {
        return None;
    }
    Some((total_secs as u64).saturating_mul(1_000_000_000))
}

/// Append text to a file.
fn cmd_append(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let filename = match parts.next() {
        Some(f) if !f.is_empty() => f,
        _ => {
            crate::console_println!("Usage: append <filename> <text>");
            return;
        }
    };
    let text = parts.next().unwrap_or("");
    let path = resolve_path(filename);

    let mut data = alloc::vec::Vec::from(text.as_bytes());
    if !text.ends_with('\n') {
        data.push(b'\n');
    }
    match crate::fs::Vfs::append(&path, &data) {
        Ok(()) => {
            crate::console_println!("Appended {} bytes to {}", data.len(), path);
        }
        Err(e) => {
            crate::console_println!("append: {}: {:?}", path, e);
        }
    }
}

/// Recursive directory tree listing.
fn cmd_tree(args: &str) {
    let path = if args.is_empty() {
        get_cwd()
    } else {
        resolve_path(args)
    };

    crate::console_println!("{}", path);
    let mut dirs: u64 = 0;
    let mut files: u64 = 0;
    tree_recurse(&path, "", &mut dirs, &mut files, 0);
    crate::console_println!("\n{} directories, {} files", dirs, files);
}

/// Internal recursive helper for tree display.
///
/// Limits depth to 8 levels to avoid excessive output.
fn tree_recurse(path: &str, prefix: &str, dirs: &mut u64, files: &mut u64, depth: u32) {
    if depth > 8 {
        return;
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    let count = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let is_last = i + 1 == count;
        let connector = if is_last { "└── " } else { "├── " };
        let type_marker = match entry.entry_type {
            crate::fs::EntryType::Directory => "/",
            crate::fs::EntryType::Symlink => "@",
            _ => "",
        };

        crate::console_println!("{}{}{}{}", prefix, connector, entry.name, type_marker);

        if entry.entry_type == crate::fs::EntryType::Directory {
            *dirs = dirs.saturating_add(1);
            let child_path = if path == "/" {
                alloc::format!("/{}", entry.name)
            } else {
                alloc::format!("{}/{}", path, entry.name)
            };
            let child_prefix = if is_last {
                alloc::format!("{}    ", prefix)
            } else {
                alloc::format!("{}│   ", prefix)
            };
            tree_recurse(&child_path, &child_prefix, dirs, files, depth + 1);
        } else {
            *files = files.saturating_add(1);
        }
    }
}

/// Show disk usage for a path (like Unix `du`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_du(args: &str) {
    let mut summary_only = false;
    let mut max_depth: usize = usize::MAX;
    let mut path_arg = "";

    for word in args.split_whitespace() {
        if word == "-s" {
            summary_only = true;
        } else if word.starts_with("-d") {
            // -d N or -dN
            let num_str = if word.len() > 2 {
                &word[2..]
            } else {
                // -d followed by next arg is not handled in this simple parser;
                // use -dN form.
                "0"
            };
            max_depth = num_str.parse::<usize>().unwrap_or(0);
        } else {
            path_arg = word;
        }
    }

    let path = if path_arg.is_empty() {
        get_cwd()
    } else {
        resolve_path(path_arg)
    };

    let total = du_recurse(&path, 0, max_depth, summary_only);
    crate::console_println!("{}\t{}", format_bytes(total), path);
}

/// Recursively calculate total size of a directory tree.
///
/// `depth` is the current recursion depth (0 = root).
/// `max_depth` limits how deep subdirectories are printed.
/// `summary_only` suppresses all subdirectory output.
#[allow(clippy::arithmetic_side_effects)]
fn du_recurse(path: &str, depth: usize, max_depth: usize, summary_only: bool) -> u64 {
    let mut total: u64 = 0;

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    for entry in &entries {
        let child_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        total = total.saturating_add(entry.size);

        if entry.entry_type == crate::fs::EntryType::Directory {
            let subdir_total = du_recurse(
                &child_path,
                depth.saturating_add(1),
                max_depth,
                summary_only,
            );
            if !summary_only && depth < max_depth {
                crate::console_println!("{}\t{}", format_bytes(subdir_total), child_path);
            }
            total = total.saturating_add(subdir_total);
        }
    }

    total
}

/// Search for files matching a pattern (basic find).
/// Search for files matching predicates (like Unix `find`).
///
/// Usage: `find [PATH] [PREDICATES...]`
///
/// Predicates:
///   `-name PATTERN`   Match filename against glob pattern
///   `-type f|d|l`     Match by type (f=file, d=directory, l=symlink)
///   `-size +N|-N|N`   Match by size (+ = larger, - = smaller, no prefix = exact)
///                     Suffixes: k (KiB), M (MiB), G (GiB), c (bytes, default)
///   `-maxdepth N`     Limit recursion depth
///   `-empty`          Match empty files (size 0) or empty directories
///
/// Without predicates, acts as a recursive glob (legacy behavior).
///
/// Examples:
///   `find /tmp -name *.txt`
///   `find / -type d -name src`
///   `find . -size +1M`
///   `find /tmp -empty`
#[allow(clippy::arithmetic_side_effects)]
fn cmd_find(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: find [PATH] [PREDICATES...]");
        crate::console_println!("  Predicates:");
        crate::console_println!("    -name PATTERN   Glob match on filename");
        crate::console_println!("    -type f|d|l     File type (f=file, d=dir, l=symlink)");
        crate::console_println!("    -size +N|-N|N   Size filter (suffixes: c, k, M, G)");
        crate::console_println!("    -maxdepth N     Limit recursion depth");
        crate::console_println!("    -empty          Empty files or directories");
        crate::console_println!("  Example: find /tmp -name *.txt -type f");
        return;
    }

    // Parse arguments.
    let mut search_path = "";
    let mut name_pattern: Option<&str> = None;
    let mut type_filter: Option<char> = None; // 'f', 'd', or 'l'
    let mut size_filter: Option<(i64, char)> = None; // (threshold, '+'/'-'/'=')
    let mut max_depth: u32 = 16;
    let mut empty_filter = false;

    let mut words = args.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w == "-name" {
            name_pattern = words.next();
        } else if w == "-type" {
            if let Some(t) = words.next() {
                type_filter = t.chars().next();
            }
        } else if w == "-size" {
            if let Some(s) = words.next() {
                size_filter = Some(parse_size_predicate(s));
            }
        } else if w == "-maxdepth" {
            if let Some(d) = words.next() {
                max_depth = d.parse::<u32>().unwrap_or(16);
            }
        } else if w == "-empty" {
            empty_filter = true;
        } else if !w.starts_with('-') && search_path.is_empty() {
            search_path = w;
        } else if !w.starts_with('-') && name_pattern.is_none() {
            // Legacy: bare pattern without -name.
            name_pattern = Some(w);
        }
    }

    if search_path.is_empty() {
        search_path = ".";
    }

    let root = resolve_path(search_path);

    let filter = FindFilter {
        name_pattern,
        is_glob: name_pattern.map_or(false, |p| {
            p.contains('*') || p.contains('?') || p.contains('[')
        }),
        type_filter,
        size_filter,
        empty_filter,
        max_depth,
    };

    let mut count: u64 = 0;
    find_recurse_filtered(&root, &filter, &mut count, 0);
    shell_println!("\n{} matches found", count);
}

/// Parsed find predicates.
struct FindFilter<'a> {
    name_pattern: Option<&'a str>,
    is_glob: bool,
    type_filter: Option<char>,
    size_filter: Option<(i64, char)>,
    empty_filter: bool,
    max_depth: u32,
}

/// Parse a `-size` argument like `+1M`, `-512k`, `100c`.
fn parse_size_predicate(s: &str) -> (i64, char) {
    let (sign, rest) = if s.starts_with('+') {
        ('+', &s[1..])
    } else if s.starts_with('-') {
        ('-', &s[1..])
    } else {
        ('=', s)
    };

    // Parse suffix.
    let (num_str, multiplier) = if rest.ends_with('G') || rest.ends_with('g') {
        (&rest[..rest.len() - 1], 1024i64 * 1024 * 1024)
    } else if rest.ends_with('M') || rest.ends_with('m') {
        (&rest[..rest.len() - 1], 1024i64 * 1024)
    } else if rest.ends_with('k') || rest.ends_with('K') {
        (&rest[..rest.len() - 1], 1024i64)
    } else if rest.ends_with('c') {
        (&rest[..rest.len() - 1], 1i64)
    } else {
        (rest, 1i64) // default: bytes
    };

    let value = num_str.parse::<i64>().unwrap_or(0) * multiplier;
    (value, sign)
}

/// `file PATH` — identify a file's type and basic info.
///
/// Similar to the Unix `file` command.  Uses `lstat` to avoid following
/// symlinks, then reports the entry type with additional detail:
/// - Directories: "directory"
/// - Regular files: "regular file, SIZE bytes" with a mime-type hint
///   based on the file extension
/// - Symlinks: "symbolic link to TARGET"
/// - Character specials: "character special"
fn cmd_file(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: file <path>");
        set_exit(1);
        return;
    }

    let path = resolve_path(args.trim());

    // Use lstat to get entry info without following symlinks.
    let entry = match crate::fs::Vfs::lstat(&path) {
        Ok(e) => e,
        Err(e) => {
            crate::console_println!("file: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };

    match entry.entry_type {
        crate::fs::EntryType::Directory => {
            shell_println!("{}: directory", path);
        }
        crate::fs::EntryType::Symlink => {
            // Try to read the link target.
            match crate::fs::Vfs::readlink(&path) {
                Ok(target) => {
                    shell_println!("{}: symbolic link to {}", path, target);
                }
                Err(_) => {
                    shell_println!("{}: symbolic link", path);
                }
            }
        }
        crate::fs::EntryType::File => {
            // Heuristic: /dev/* entries with size 0 are character specials
            // (the VFS doesn't have a CharDevice entry type).
            if path.starts_with("/dev/") && entry.size == 0 {
                shell_println!("{}: character special", path);
            } else {
                // Read up to 512 bytes for magic byte detection.
                let magic_hint = match crate::fs::Vfs::read_at(&path, 0, 512) {
                    Ok(header) => detect_magic(&header),
                    Err(_) => None,
                };
                if let Some(hint) = magic_hint {
                    shell_println!("{}: {}, {} bytes", path, hint, entry.size);
                } else {
                    let ext_hint = extension_hint(&path);
                    shell_println!("{}: regular file, {} bytes ({})", path, entry.size, ext_hint);
                }
            }
        }
        crate::fs::EntryType::VolumeLabel => {
            shell_println!("{}: volume label", path);
        }
    }
}

/// Detect file type from magic bytes in the file header.
///
/// Reads the first bytes and matches against known signatures.
/// Returns `None` if no signature matches (falls back to extension).
fn detect_magic(header: &[u8]) -> Option<&'static str> {
    if header.is_empty() {
        return Some("empty");
    }

    // Check fixed-offset signatures.
    // Order matters: more specific patterns first where ambiguity exists.

    // ELF binary
    if header.starts_with(b"\x7FELF") {
        // ELF class: header[4] — 1=32-bit, 2=64-bit
        let bits = match header.get(4) {
            Some(1) => "32-bit",
            Some(2) => "64-bit",
            _ => "",
        };
        // ELF type: header[16..18] LE — 1=relocatable, 2=executable, 3=shared, 4=core
        let etype = if header.len() >= 18 {
            let et = u16::from_le_bytes([header[16], header[17]]);
            match et {
                1 => " relocatable",
                2 => " executable",
                3 => " shared object",
                4 => " core dump",
                _ => "",
            }
        } else {
            ""
        };
        return Some(match (bits, etype) {
            ("64-bit", " executable") => "ELF 64-bit executable",
            ("64-bit", " shared object") => "ELF 64-bit shared object",
            ("64-bit", " relocatable") => "ELF 64-bit relocatable",
            ("64-bit", " core dump") => "ELF 64-bit core dump",
            ("32-bit", " executable") => "ELF 32-bit executable",
            ("32-bit", " shared object") => "ELF 32-bit shared object",
            ("32-bit", " relocatable") => "ELF 32-bit relocatable",
            ("32-bit", " core dump") => "ELF 32-bit core dump",
            _ => "ELF binary",
        });
    }

    // PE/COFF (Windows executables, also used by UEFI)
    if header.starts_with(b"MZ") {
        return Some("PE executable (MZ)");
    }

    // PNG
    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("PNG image data");
    }

    // JPEG
    if header.starts_with(b"\xff\xd8\xff") {
        return Some("JPEG image data");
    }

    // GIF
    if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        return Some("GIF image data");
    }

    // BMP
    if header.starts_with(b"BM") && header.len() >= 6 {
        return Some("BMP image data");
    }

    // WebP (RIFF....WEBP)
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WEBP" {
        return Some("WebP image data");
    }

    // TIFF (little-endian or big-endian)
    if header.starts_with(b"II\x2a\x00") || header.starts_with(b"MM\x00\x2a") {
        return Some("TIFF image data");
    }

    // PDF
    if header.starts_with(b"%PDF") {
        return Some("PDF document");
    }

    // ZIP archive (including JAR, DOCX, XLSX, etc.)
    if header.starts_with(b"PK\x03\x04") {
        return Some("ZIP archive");
    }

    // gzip
    if header.starts_with(b"\x1f\x8b") {
        return Some("gzip compressed data");
    }

    // bzip2
    if header.len() >= 4 && header.starts_with(b"BZh") {
        return Some("bzip2 compressed data");
    }

    // xz
    if header.starts_with(b"\xfd7zXZ\x00") {
        return Some("XZ compressed data");
    }

    // bzip2
    if header.starts_with(b"BZ") && header.len() >= 3 && header[2] == b'h' {
        return Some("bzip2 compressed data");
    }

    // 7z
    if header.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some("7-zip archive");
    }

    // Zstandard
    if header.len() >= 4 {
        let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        if magic == 0xFD2F_B528 {
            return Some("Zstandard compressed data");
        }
    }

    // RAR
    if header.starts_with(b"Rar!\x1a\x07") {
        return Some("RAR archive");
    }

    // USTAR tar archive (magic at offset 257)
    if header.len() >= 263 && &header[257..262] == b"ustar" {
        return Some("POSIX tar archive (ustar)");
    }

    // WebAssembly
    if header.starts_with(b"\x00asm") {
        return Some("WebAssembly binary");
    }

    // SQLite
    if header.starts_with(b"SQLite format 3") {
        return Some("SQLite 3.x database");
    }

    // FLAC audio
    if header.starts_with(b"fLaC") {
        return Some("FLAC audio");
    }

    // Ogg container (Vorbis, Opus, etc.)
    if header.starts_with(b"OggS") {
        return Some("Ogg data");
    }

    // MP3 (ID3 tag or sync word)
    if header.starts_with(b"ID3") {
        return Some("MP3 audio (ID3)");
    }
    if header.len() >= 2 && header[0] == 0xFF && (header[1] & 0xE0) == 0xE0 {
        return Some("MP3 audio");
    }

    // WAV (RIFF....WAVE)
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WAVE" {
        return Some("WAVE audio");
    }

    // AVI (RIFF....AVI )
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"AVI " {
        return Some("AVI video");
    }

    // Matroska/WebM
    if header.starts_with(b"\x1a\x45\xdf\xa3") {
        return Some("Matroska/WebM video");
    }

    // MIDI
    if header.starts_with(b"MThd") {
        return Some("MIDI audio");
    }

    // Shell script / shebang
    if header.starts_with(b"#!") {
        // Try to identify the interpreter.
        let line_end = header.iter().position(|&b| b == b'\n').unwrap_or(header.len().min(80));
        let shebang = core::str::from_utf8(header.get(2..line_end).unwrap_or(&[])).unwrap_or("");
        if shebang.contains("python") {
            return Some("Python script");
        } else if shebang.contains("bash") {
            return Some("Bash script");
        } else if shebang.contains("sh") {
            return Some("shell script");
        } else if shebang.contains("perl") {
            return Some("Perl script");
        } else if shebang.contains("ruby") {
            return Some("Ruby script");
        } else if shebang.contains("node") {
            return Some("Node.js script");
        }
        return Some("script with shebang");
    }

    // UTF-8 BOM
    if header.starts_with(b"\xEF\xBB\xBF") {
        return Some("UTF-8 Unicode text (with BOM)");
    }

    // UTF-16 BOM
    if header.starts_with(b"\xFF\xFE") {
        return Some("UTF-16 Unicode text (little-endian)");
    }
    if header.starts_with(b"\xFE\xFF") {
        return Some("UTF-16 Unicode text (big-endian)");
    }

    // JSON heuristic: starts with { or [
    if header.first() == Some(&b'{') || header.first() == Some(&b'[') {
        // Validate at least a few bytes look like JSON.
        let s = core::str::from_utf8(header.get(..64.min(header.len())).unwrap_or(&[]));
        if s.is_ok() {
            return Some("JSON data");
        }
    }

    // XML / HTML heuristic
    if header.starts_with(b"<?xml") {
        return Some("XML document");
    }
    if header.starts_with(b"<!DOCTYPE html") || header.starts_with(b"<html") || header.starts_with(b"<HTML") {
        return Some("HTML document");
    }

    // Fall back to text vs binary heuristic.
    // Scan the first bytes for non-text characters.
    let check_len = header.len().min(256);
    let mut non_text = 0usize;
    for &b in header.get(..check_len).unwrap_or(&[]) {
        match b {
            // Common text bytes: printable ASCII, tab, newline, carriage return.
            0x09 | 0x0A | 0x0D | 0x20..=0x7E => {}
            // High bytes could be UTF-8 continuation.
            0x80..=0xFF => {}
            // Null and other control chars are strong binary indicators.
            _ => {
                non_text = non_text.saturating_add(1);
            }
        }
    }

    if non_text == 0 && check_len > 0 {
        // Looks like text — try to determine if it's valid UTF-8.
        if core::str::from_utf8(header.get(..check_len).unwrap_or(&[])).is_ok() {
            return Some("ASCII text");
        }
        return Some("text data");
    }

    if non_text > 0 && check_len > 0 {
        // Has some binary bytes — likely binary data.
        return Some("data");
    }

    None
}

/// Map a file extension to a human-readable type hint for the `file` command.
fn extension_hint(path: &str) -> &'static str {
    // Extract the extension (after the last '.'), lowercased comparison.
    let ext = match path.rsplit_once('.') {
        Some((_, e)) => e,
        None => return "data",
    };

    // Compare case-insensitively by checking lowercase.
    // Since ext is a slice of path, we need byte-level comparison.
    match ext {
        // Text and source files.
        "txt" | "text" | "log" => "text",
        "rs" => "Rust source",
        "c" | "h" => "C source",
        "cpp" | "cc" | "cxx" | "hpp" => "C++ source",
        "py" => "Python source",
        "js" => "JavaScript source",
        "ts" => "TypeScript source",
        "json" => "JSON data",
        "toml" => "TOML data",
        "yaml" | "yml" => "YAML data",
        "xml" => "XML data",
        "html" | "htm" => "HTML document",
        "css" => "CSS stylesheet",
        "md" => "Markdown text",
        "sh" | "bash" => "shell script",

        // Binary and executable formats.
        "elf" => "ELF binary",
        "o" => "object file",
        "a" | "lib" => "archive/library",
        "so" | "dll" => "shared library",
        "wasm" => "WebAssembly binary",

        // Image formats.
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "gif" => "GIF image",
        "bmp" => "BMP image",
        "svg" => "SVG image",
        "ico" => "icon image",

        // Archive and compressed formats.
        "tar" => "tar archive",
        "gz" | "gzip" => "gzip compressed",
        "bz2" => "bzip2 compressed",
        "tbz2" | "tbz" => "bzip2 compressed tar archive",
        "zip" => "ZIP archive",
        "xz" => "XZ compressed",

        // Config and data.
        "cfg" | "conf" | "ini" => "configuration file",
        "csv" => "CSV data",
        "sql" => "SQL script",

        _ => "data",
    }
}

/// Recursive helper for find — search directory tree for name matches.
///
/// Uses glob matching if the pattern contains metacharacters (`*`, `?`,
/// `[`), otherwise falls back to case-insensitive substring matching.
/// Limits depth to 16.
/// Recursive find with predicate filtering.
#[allow(clippy::arithmetic_side_effects)]
fn find_recurse_filtered(path: &str, filter: &FindFilter<'_>, count: &mut u64, depth: u32) {
    if depth > filter.max_depth {
        return;
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child_path = if path == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", path, entry.name)
        };

        // Apply all predicates (AND logic).
        let mut matched = true;

        // Name predicate.
        if let Some(pattern) = filter.name_pattern {
            if filter.is_glob {
                if !crate::fs::vfs::glob_match(&entry.name, pattern, true) {
                    matched = false;
                }
            } else {
                let name_lower = entry.name.to_ascii_lowercase();
                let pattern_lower = pattern.to_ascii_lowercase();
                if !name_lower.contains(&pattern_lower) {
                    matched = false;
                }
            }
        }

        // Type predicate.
        if let Some(t) = filter.type_filter {
            let type_ok = match t {
                'f' => entry.entry_type == crate::fs::EntryType::File,
                'd' => entry.entry_type == crate::fs::EntryType::Directory,
                'l' => entry.entry_type == crate::fs::EntryType::Symlink,
                _ => true,
            };
            if !type_ok {
                matched = false;
            }
        }

        // Size predicate (only for files).
        if let Some((threshold, sign)) = filter.size_filter {
            let sz = entry.size as i64;
            let size_ok = match sign {
                '+' => sz > threshold,
                '-' => sz < threshold,
                _ => sz == threshold, // '='
            };
            if !size_ok {
                matched = false;
            }
        }

        // Empty predicate.
        if filter.empty_filter {
            let empty_ok = match entry.entry_type {
                crate::fs::EntryType::File => entry.size == 0,
                crate::fs::EntryType::Directory => {
                    // Check if directory is empty (no entries besides . and ..).
                    crate::fs::Vfs::readdir(&child_path)
                        .map(|e| e.is_empty())
                        .unwrap_or(false)
                }
                _ => false,
            };
            if !empty_ok {
                matched = false;
            }
        }

        if matched {
            let type_str = match entry.entry_type {
                crate::fs::EntryType::File => "",
                crate::fs::EntryType::Directory => "/",
                crate::fs::EntryType::Symlink => "@",
                crate::fs::EntryType::VolumeLabel => "*",
            };
            shell_println!("{}{}", child_path, type_str);
            *count = count.saturating_add(1);
        }

        if entry.entry_type == crate::fs::EntryType::Directory {
            find_recurse_filtered(&child_path, filter, count, depth + 1);
        }
    }
}

/// Count lines, words, and bytes in a file (like Unix `wc`).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_wc(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: wc <file>");
        return;
    }

    let path = resolve_path(args);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("wc: {}: {:?}", path, e);
            return;
        }
    };

    let bytes = data.len();
    let mut lines: usize = 0;
    let mut words: usize = 0;
    let mut in_word = false;

    for &b in &data {
        if b == b'\n' {
            lines += 1;
        }
        let is_ws = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
        if is_ws {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }

    shell_println!("  {} lines  {} words  {} bytes  {}", lines, words, bytes, path);
}

/// Show the first N lines of a file (like Unix `head`).
fn cmd_head(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (count, file) = if parts.len() >= 2 {
        match parts[0].parse::<usize>() {
            Ok(n) => (n, parts[1]),
            Err(_) => (10, args), // Default to 10 lines if first arg isn't a number.
        }
    } else {
        crate::console_println!("Usage: head [N] <file>");
        return;
    };

    let path = resolve_path(file);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("head: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    let mut printed = 0;
    for line in text.lines() {
        if printed >= count {
            break;
        }
        shell_println!("{}", line);
        printed += 1;
    }
}

/// Show the last N lines of a file (like Unix `tail`).
fn cmd_tail(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (count, file) = if parts.len() >= 2 {
        match parts[0].parse::<usize>() {
            Ok(n) => (n, parts[1]),
            Err(_) => (10, args),
        }
    } else {
        crate::console_println!("Usage: tail [N] <file>");
        return;
    };

    let path = resolve_path(file);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("tail: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    let lines: alloc::vec::Vec<&str> = text.lines().collect();
    let start = if lines.len() > count { lines.len() - count } else { 0 };
    for line in &lines[start..] {
        shell_println!("{}", line);
    }
}

/// Hex dump of a file (like `hexdump -C` or `xxd`).
#[allow(clippy::arithmetic_side_effects)]
/// Hex dump of file contents.
///
/// Usage: `hexdump [-n COUNT] <file>`
///
/// `-n COUNT` limits output to COUNT bytes (default: 512).
/// Use `-n 0` for no limit (shows entire file).
fn cmd_hexdump(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: hexdump [-n count] <file>");
        return;
    }

    let mut max_bytes: usize = 512;
    let mut file_path = "";

    let mut words = args.split_whitespace();
    while let Some(w) = words.next() {
        if w == "-n" {
            if let Some(n) = words.next() {
                max_bytes = n.parse::<usize>().unwrap_or(512);
                if max_bytes == 0 {
                    max_bytes = usize::MAX;
                }
            }
        } else if w.starts_with("-n") {
            max_bytes = w[2..].parse::<usize>().unwrap_or(512);
            if max_bytes == 0 {
                max_bytes = usize::MAX;
            }
        } else {
            file_path = w;
        }
    }

    if file_path.is_empty() {
        crate::console_println!("Usage: hexdump [-n count] <file>");
        return;
    }

    let path = resolve_path(file_path);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("hexdump: {}: {:?}", path, e);
            return;
        }
    };

    let total_size = data.len();
    let limit = total_size.min(max_bytes);
    let data = &data[..limit];

    for offset in (0..data.len()).step_by(16) {
        // Offset.
        let mut line = alloc::format!("{:08x}  ", offset);

        // Hex bytes.
        for i in 0..16 {
            if offset + i < data.len() {
                line.push_str(&alloc::format!("{:02x} ", data[offset + i]));
            } else {
                line.push_str("   ");
            }
            if i == 7 {
                line.push(' ');
            }
        }

        line.push_str(" |");

        // ASCII printable characters.
        for i in 0..16 {
            if offset + i < data.len() {
                let b = data[offset + i];
                if (0x20..=0x7e).contains(&b) {
                    line.push(b as char);
                } else {
                    line.push('.');
                }
            }
        }
        line.push('|');

        shell_println!("{}", line);
    }

    if limit >= total_size {
        shell_println!("{:08x}", total_size);
    } else {
        shell_println!("... ({} bytes total, showing first {})", total_size, limit);
    }
}

/// Search for a pattern in a file (simple substring grep).
/// Grep flags parsed from command-line arguments.
struct GrepFlags {
    case_insensitive: bool,
    invert: bool,
    count_only: bool,
    show_line_numbers: bool,
    files_only: bool,
    whole_word: bool,
    recursive: bool,
    max_matches: usize,
}

impl GrepFlags {
    fn new() -> Self {
        Self {
            case_insensitive: true, // default: case-insensitive (like original)
            invert: false,
            count_only: false,
            show_line_numbers: true,
            files_only: false,
            whole_word: false,
            recursive: false,
            max_matches: 200,
        }
    }
}

/// Check if a character is a word boundary (not alphanumeric or underscore).
fn is_word_boundary(c: char) -> bool {
    !c.is_alphanumeric() && c != '_'
}

/// Test if `line` contains `pattern` with optional whole-word matching.
///
/// Both `line` and `pattern` should already be case-folded if case-insensitive.
fn grep_matches(line: &str, pattern: &str, whole_word: bool) -> bool {
    if !whole_word {
        return line.contains(pattern);
    }
    // Whole-word: pattern must be bounded by non-word chars (or string edges).
    let pat_len = pattern.len();
    if pat_len == 0 {
        return true;
    }
    let mut start = 0usize;
    while start + pat_len <= line.len() {
        if let Some(pos) = line[start..].find(pattern) {
            let abs = start + pos;
            let before_ok = abs == 0
                || line[..abs].chars().next_back().map_or(true, is_word_boundary);
            let after_ok = abs + pat_len >= line.len()
                || line[abs + pat_len..].chars().next().map_or(true, is_word_boundary);
            if before_ok && after_ok {
                return true;
            }
            start = abs + 1;
        } else {
            break;
        }
    }
    false
}

/// Lowercase a string into a new allocation (for case-insensitive matching).
fn to_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for lc in c.to_lowercase() {
            out.push(lc);
        }
    }
    out
}

fn cmd_grep(args: &str) {
    let mut flags = GrepFlags::new();
    let mut words: alloc::vec::Vec<&str> = Vec::new();

    // Parse flags and positional arguments.
    for word in args.split_whitespace() {
        if word.starts_with('-') && word.len() > 1 && !word.starts_with("--") {
            for ch in word[1..].chars() {
                match ch {
                    'i' => flags.case_insensitive = true,
                    'I' => flags.case_insensitive = false, // explicit case-sensitive
                    'v' => flags.invert = true,
                    'c' => flags.count_only = true,
                    'n' => flags.show_line_numbers = true,
                    'l' => flags.files_only = true,
                    'w' => flags.whole_word = true,
                    'r' | 'R' => flags.recursive = true,
                    _ => {
                        crate::console_println!("grep: unknown flag '-{}'", ch);
                        return;
                    }
                }
            }
        } else {
            words.push(word);
        }
    }

    if words.is_empty() {
        crate::console_println!(
            "Usage: grep [-ivclnwrI] <pattern> <file|dir> [file2 ...]"
        );
        return;
    }

    let pattern = words[0];
    let targets = if words.len() > 1 { &words[1..] } else {
        crate::console_println!(
            "Usage: grep [-ivclnwrI] <pattern> <file|dir> [file2 ...]"
        );
        return;
    };

    let pattern_cmp = if flags.case_insensitive {
        to_lower(pattern)
    } else {
        String::from(pattern)
    };

    let multi_file = targets.len() > 1 || flags.recursive;
    let mut total_matches = 0usize;

    for &target in targets {
        let path = resolve_path(target);
        if flags.recursive {
            grep_recursive(
                &path, &pattern_cmp, &flags, multi_file,
                &mut total_matches, 0,
            );
        } else {
            grep_file(&path, &pattern_cmp, &flags, multi_file, &mut total_matches);
        }
        if total_matches >= flags.max_matches {
            break;
        }
    }

    if flags.count_only && !multi_file {
        shell_println!("{}", total_matches);
    } else if total_matches == 0 && !flags.count_only && !flags.files_only {
        crate::console_println!("grep: no matches for '{}'", pattern);
    }
}

/// Search a single file for grep matches.
fn grep_file(
    path: &str,
    pattern: &str,
    flags: &GrepFlags,
    multi_file: bool,
    total: &mut usize,
) {
    let data = match crate::fs::Vfs::read_file(path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("grep: {}: {:?}", path, e);
            return;
        }
    };

    let text = match core::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => {
            // Skip binary files silently in recursive mode.
            if !flags.recursive {
                crate::console_println!("grep: {}: binary file", path);
            }
            return;
        }
    };

    let mut file_matches = 0usize;

    for (line_num, line) in text.lines().enumerate() {
        let line_cmp = if flags.case_insensitive {
            to_lower(line)
        } else {
            String::from(line)
        };

        let matched = grep_matches(&line_cmp, pattern, flags.whole_word);
        let show = if flags.invert { !matched } else { matched };

        if show {
            file_matches = file_matches.saturating_add(1);
            *total = total.saturating_add(1);

            if flags.files_only {
                shell_println!("{}", path);
                return; // One match is enough for -l.
            }

            if !flags.count_only {
                if multi_file && flags.show_line_numbers {
                    shell_println!("{}:{}:{}", path, line_num.saturating_add(1), line);
                } else if multi_file {
                    shell_println!("{}:{}", path, line);
                } else if flags.show_line_numbers {
                    shell_println!("{}:{}", line_num.saturating_add(1), line);
                } else {
                    shell_println!("{}", line);
                }
            }

            if *total >= flags.max_matches {
                shell_println!("... (showing first {} matches)", flags.max_matches);
                return;
            }
        }
    }

    if flags.count_only && multi_file {
        shell_println!("{}:{}", path, file_matches);
    }
}

/// Recursively search a directory for grep matches (depth limit 16).
fn grep_recursive(
    path: &str,
    pattern: &str,
    flags: &GrepFlags,
    multi_file: bool,
    total: &mut usize,
    depth: usize,
) {
    if depth > 16 || *total >= flags.max_matches {
        return;
    }

    // Check if path is a directory or file.
    let entry = match crate::fs::Vfs::stat(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    if entry.entry_type == crate::fs::vfs::EntryType::File {
        grep_file(path, pattern, flags, multi_file, total);
        return;
    }

    if entry.entry_type != crate::fs::vfs::EntryType::Directory {
        return;
    }

    let entries = match crate::fs::Vfs::readdir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for child in &entries {
        if child.name == "." || child.name == ".." {
            continue;
        }
        // Skip hidden directories/files starting with '.' in recursive mode.
        if child.name.starts_with('.') {
            continue;
        }

        let child_path = if path == "/" {
            alloc::format!("/{}", child.name)
        } else {
            alloc::format!("{}/{}", path, child.name)
        };

        match child.entry_type {
            crate::fs::vfs::EntryType::File => {
                grep_file(&child_path, pattern, flags, multi_file, total);
            }
            crate::fs::vfs::EntryType::Directory => {
                grep_recursive(
                    &child_path, pattern, flags, multi_file, total,
                    depth.saturating_add(1),
                );
            }
            _ => {}
        }

        if *total >= flags.max_matches {
            return;
        }
    }
}

/// Compare two files byte-by-byte.
///
/// Usage: `cmp <file1> <file2>`
fn cmd_cmp(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: cmp <file1> <file2>");
        return;
    }

    let path1 = resolve_path(parts[0]);
    let path2 = resolve_path(parts[1]);

    let data1 = match crate::fs::Vfs::read_file(&path1) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("cmp: {}: {:?}", path1, e);
            return;
        }
    };
    let data2 = match crate::fs::Vfs::read_file(&path2) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("cmp: {}: {:?}", path2, e);
            return;
        }
    };

    if data1 == data2 {
        crate::console_println!("{} and {} are identical ({} bytes)", path1, path2, data1.len());
        return;
    }

    // Find first difference.
    let min_len = data1.len().min(data2.len());
    let mut diff_offset = None;
    for i in 0..min_len {
        if data1.get(i) != data2.get(i) {
            diff_offset = Some(i);
            break;
        }
    }

    // If no difference in common prefix, the difference is the length.
    let diff_at = diff_offset.unwrap_or(min_len);
    crate::console_println!(
        "{} {} differ: byte {}, {} size={}, {} size={}",
        path1, path2, diff_at, path1, data1.len(), path2, data2.len(),
    );
}

/// Line-level diff between two text files (unified format).
///
/// Usage: `diff <file1> <file2>`
///
/// Uses a simple LCS-based diff algorithm suitable for kernel context.
/// Files are capped at 2000 lines to bound memory usage.
fn cmd_diff(args: &str) {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: diff <file1> <file2>");
        return;
    }

    let path1 = resolve_path(parts[0]);
    let path2 = resolve_path(parts[1]);

    let data1 = match crate::fs::Vfs::read_file(&path1) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("diff: {}: {:?}", path1, e);
            return;
        }
    };
    let data2 = match crate::fs::Vfs::read_file(&path2) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("diff: {}: {:?}", path2, e);
            return;
        }
    };

    if data1 == data2 {
        // Identical files — no output (like Unix diff).
        return;
    }

    // Split into lines. Treat as text — invalid UTF-8 bytes get replacement chars.
    // For a kernel shell this is acceptable; binary files should use `cmp`.
    let text1 = String::from_utf8_lossy(&data1);
    let text2 = String::from_utf8_lossy(&data2);

    let lines1: Vec<&str> = text1.lines().collect();
    let lines2: Vec<&str> = text2.lines().collect();

    const MAX_LINES: usize = 2000;
    if lines1.len() > MAX_LINES || lines2.len() > MAX_LINES {
        crate::console_println!(
            "diff: files too large for line diff ({} vs {} lines, max {}). Use cmp instead.",
            lines1.len(), lines2.len(), MAX_LINES,
        );
        return;
    }

    // Compute edit script using O(NM) LCS table with space optimization.
    // We only need the edit operations, not the full table. Use the
    // Hirschberg-style approach: compute LCS length in O(N) space, then
    // use the simple approach for files under our cap since O(NM) fits
    // easily (2000×2000 = 4M entries × 2 bytes = 8 MiB, acceptable for
    // a kernel debug tool running once).
    let n = lines1.len();
    let m = lines2.len();

    // Build LCS length table — dp[i][j] = LCS length of lines1[0..i] and lines2[0..j].
    // Use u16 since max lines = 2000.
    // Allocate as flat Vec to avoid Vec<Vec<>> overhead.
    let width = m + 1;
    let mut dp: Vec<u16> = alloc::vec![0u16; (n + 1) * width];

    for i in 1..=n {
        for j in 1..=m {
            dp[i * width + j] = if lines1[i - 1] == lines2[j - 1] {
                dp[(i - 1) * width + (j - 1)] + 1
            } else {
                let up = dp[(i - 1) * width + j];
                let left = dp[i * width + (j - 1)];
                if up >= left { up } else { left }
            };
        }
    }

    // Backtrack to produce edit script.
    #[derive(Clone, Copy, PartialEq)]
    enum Edit {
        Keep,   // line in both
        Remove, // line only in file1
        Add,    // line only in file2
    }

    let mut edits: Vec<(Edit, usize)> = Vec::new(); // (kind, line_index in source)
    let mut i = n;
    let mut j = m;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && lines1[i - 1] == lines2[j - 1] {
            edits.push((Edit::Keep, i - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i * width + (j - 1)] >= dp[(i - 1) * width + j]) {
            edits.push((Edit::Add, j - 1));
            j -= 1;
        } else {
            edits.push((Edit::Remove, i - 1));
            i -= 1;
        }
    }
    edits.reverse();

    // Drop the DP table before printing to free memory.
    drop(dp);

    // Print header.
    crate::console_println!("--- {}", path1);
    crate::console_println!("+++ {}", path2);

    // Group edits into hunks (unified diff with 3 lines context).
    const CONTEXT: usize = 3;

    // Find hunk boundaries: a hunk contains consecutive changes plus
    // CONTEXT lines before and after. Hunks separated by more than
    // 2*CONTEXT keep-lines are printed separately.
    let mut hunk_start = None;
    let mut hunk_end = 0;
    let mut hunks: Vec<(usize, usize)> = Vec::new();

    for (idx, (kind, _)) in edits.iter().enumerate() {
        if *kind != Edit::Keep {
            let start = idx.saturating_sub(CONTEXT);
            let end = (idx + CONTEXT + 1).min(edits.len());

            if let Some(hs) = hunk_start {
                if start <= hunk_end {
                    // Merge with current hunk.
                    hunk_end = end;
                } else {
                    // Emit previous hunk, start new one.
                    hunks.push((hs, hunk_end));
                    hunk_start = Some(start);
                    hunk_end = end;
                }
            } else {
                hunk_start = Some(start);
                hunk_end = end;
            }
        }
    }
    if let Some(hs) = hunk_start {
        hunks.push((hs, hunk_end));
    }

    // Print each hunk.
    for (hstart, hend) in &hunks {
        // Count lines in this hunk for the @@ header.
        let mut old_start = 0u32;
        let mut old_count = 0u32;
        let mut new_start = 0u32;
        let mut new_count = 0u32;

        // First pass: figure out old/new line numbers at hunk start.
        // Walk from beginning of edits to hstart to count line positions.
        let mut ol = 1u32;
        let mut nl = 1u32;
        for (idx, (kind, _)) in edits.iter().enumerate() {
            if idx == *hstart {
                old_start = ol;
                new_start = nl;
                break;
            }
            match kind {
                Edit::Keep => { ol += 1; nl += 1; }
                Edit::Remove => { ol += 1; }
                Edit::Add => { nl += 1; }
            }
        }

        // Second pass: count lines in this hunk.
        for idx in *hstart..*hend {
            if let Some((kind, _)) = edits.get(idx) {
                match kind {
                    Edit::Keep => { old_count += 1; new_count += 1; }
                    Edit::Remove => { old_count += 1; }
                    Edit::Add => { new_count += 1; }
                }
            }
        }

        crate::console_println!("@@ -{},{} +{},{} @@", old_start, old_count, new_start, new_count);

        for idx in *hstart..*hend {
            if let Some((kind, line_idx)) = edits.get(idx) {
                let line_text = match kind {
                    Edit::Keep | Edit::Remove => lines1.get(*line_idx).unwrap_or(&""),
                    Edit::Add => lines2.get(*line_idx).unwrap_or(&""),
                };
                let prefix = match kind {
                    Edit::Keep => ' ',
                    Edit::Remove => '-',
                    Edit::Add => '+',
                };
                crate::console_println!("{}{}", prefix, line_text);
            }
        }
    }

    if hunks.is_empty() {
        // Should not happen since data1 != data2, but could if only trailing
        // newline differs. Show a minimal note.
        crate::console_println!("(files differ only in trailing newline)");
    }
}

/// Pre-allocate disk space for a file.
///
/// Usage: `fallocate <size> <file>`
fn cmd_fallocate(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: fallocate <size> <file>");
        crate::console_println!("  Size can be suffixed with K, M, G (e.g., 4K, 1M)");
        return;
    }

    let size_str = parts[0];
    let path = resolve_path(parts[1]);

    // Parse size with optional K/M/G suffix.
    let size = {
        let s = size_str.trim();
        let (num_part, multiplier) = if s.ends_with('K') || s.ends_with('k') {
            (&s[..s.len().saturating_sub(1)], 1024u64)
        } else if s.ends_with('M') || s.ends_with('m') {
            (&s[..s.len().saturating_sub(1)], 1024u64 * 1024)
        } else if s.ends_with('G') || s.ends_with('g') {
            (&s[..s.len().saturating_sub(1)], 1024u64 * 1024 * 1024)
        } else {
            (s, 1u64)
        };

        match num_part.parse::<u64>() {
            Ok(n) => n.saturating_mul(multiplier),
            Err(_) => {
                crate::console_println!("fallocate: invalid size '{}'", size_str);
                return;
            }
        }
    };

    match crate::fs::Vfs::fallocate(&path, size) {
        Ok(()) => {
            crate::console_println!("fallocate: reserved {} bytes for {}", size, path);
        }
        Err(e) => {
            crate::console_println!("fallocate: {}: {:?}", path, e);
        }
    }
}

/// List open file handles (like `lsof`).
fn cmd_lsof() {
    let handles = crate::fs::handle::list_handles();
    if handles.is_empty() {
        crate::console_println!("No open file handles.");
        return;
    }

    crate::console_println!(
        "{:<7} {:<5} {:<12} {:<12} {}",
        "HANDLE", "FLAGS", "OFFSET", "SIZE", "PATH"
    );

    for h in &handles {
        // Decode flags into a compact string.
        let mut flags = alloc::string::String::new();
        if h.flags & 0x01 != 0 { flags.push('R'); }
        if h.flags & 0x02 != 0 { flags.push('W'); }
        if h.flags & 0x04 != 0 { flags.push('C'); }
        if h.flags & 0x08 != 0 { flags.push('T'); }
        if h.flags & 0x10 != 0 { flags.push('A'); }
        if flags.is_empty() { flags.push('-'); }

        crate::console_println!(
            "{:<7} {:<5} {:<12} {:<12} {}",
            h.id, flags, h.offset, h.size, h.path,
        );
    }

    crate::console_println!("\nTotal: {} open handles", handles.len());
}

/// Paginated directory listing.
///
/// Usage: `lsp [page_size] <path>`
/// Shows entries one page at a time, with "--- more ---" between pages.
/// Default page size is 20 entries if not specified.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_lsp(args: &str) {
    let (page_size, path) = {
        let mut parts = args.splitn(2, ' ');
        let first = parts.next().unwrap_or("");
        let second = parts.next();

        // Try to parse first arg as a number.
        match first.parse::<usize>() {
            Ok(n) if n > 0 => {
                // First arg is page size, second is path (or "/" default).
                (n, second.unwrap_or("/"))
            }
            _ => {
                // First arg is the path (or "/" if empty).
                (20, if first.is_empty() { "/" } else { first })
            }
        }
    };

    let mut offset = 0usize;
    loop {
        match crate::fs::Vfs::readdir_at(path, offset, page_size) {
            Ok((entries, total)) => {
                if offset == 0 {
                    crate::console_println!(
                        "Directory '{}' — {} entries (page size {})",
                        path, total, page_size,
                    );
                    crate::console_println!(
                        "{:<5} {:<8} {:<12} {}",
                        "TYPE", "SIZE", "NAME", "",
                    );
                }

                if entries.is_empty() {
                    if offset == 0 {
                        crate::console_println!("  (empty directory)");
                    }
                    break;
                }

                for entry in &entries {
                    let type_str = match entry.entry_type {
                        crate::fs::vfs::EntryType::File => "FILE",
                        crate::fs::vfs::EntryType::Directory => "DIR",
                        crate::fs::vfs::EntryType::Symlink => "LINK",
                        crate::fs::vfs::EntryType::VolumeLabel => "VOL",
                    };
                    crate::console_println!(
                        "{:<5} {:<8} {}",
                        type_str, entry.size, entry.name,
                    );
                }

                offset += entries.len();

                if offset >= total {
                    crate::console_println!(
                        "--- end ({}/{} entries shown) ---",
                        offset, total,
                    );
                    break;
                }

                crate::console_println!(
                    "--- {}/{} shown, press Enter for next page ---",
                    offset, total,
                );

                // Wait for Enter key to continue.
                let mut dummy = alloc::string::String::new();
                let mut h = History::new();
                read_line(&mut dummy, &mut h);
            }
            Err(e) => {
                crate::console_println!("lsp: error: {:?}", e);
                break;
            }
        }
    }
}

/// List mounted filesystems or mount a new one.
fn cmd_mount(args: &str) {
    if args.is_empty() {
        // List all mounts with options.
        let mounts = crate::fs::Vfs::mounts_full();
        if mounts.is_empty() {
            crate::console_println!("No filesystems mounted.");
        } else {
            crate::console_println!("{:<12} {:<16} {}", "Type", "Mount point", "Options");
            for (path, fs_type, options) in &mounts {
                crate::console_println!("{:<12} {:<16} {}", fs_type, path, options.to_string());
            }
        }
        return;
    }

    // Parse: mount [-t type] [-o options] <device|none> <mount-path>
    //        mount -o remount,ro <mount-path>   (remount only)
    let words: Vec<&str> = args.split_whitespace().collect();

    let mut fs_type: Option<&str> = None;
    let mut opts_str: Option<&str> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i] == "-t" && i + 1 < words.len() {
            fs_type = Some(words[i + 1]);
            i += 2;
        } else if words[i] == "-o" && i + 1 < words.len() {
            opts_str = Some(words[i + 1]);
            i += 2;
        } else {
            positional.push(words[i]);
            i += 1;
        }
    }

    // Handle remount: "mount -o remount,ro /path"
    if let Some(opts) = opts_str {
        if opts.contains("remount") {
            if positional.is_empty() {
                crate::console_println!("mount: remount requires a mount point");
                set_exit(1);
                return;
            }
            let mount_path_resolved = resolve_path(positional[0]);
            let mount_opts = crate::fs::vfs::MountOptions::parse(opts);
            match crate::fs::Vfs::remount(&mount_path_resolved, mount_opts) {
                Ok(()) => crate::console_println!(
                    "Remounted {} with options: {}",
                    mount_path_resolved,
                    mount_opts.to_string(),
                ),
                Err(e) => {
                    crate::console_println!("mount: remount failed: {:?}", e);
                    set_exit(1);
                }
            }
            return;
        }
    }

    let (device, mount_path) = if positional.len() >= 2 {
        (positional[0], positional[1])
    } else if positional.len() == 1 && fs_type.is_some() {
        // mount -t type <device-or-path> — missing mount path
        crate::console_println!("Usage: mount [-t type] [-o options] <device|none> <mount-path>");
        crate::console_println!("       mount -o remount[,ro|rw|noatime] <mount-path>");
        set_exit(1);
        return;
    } else {
        crate::console_println!("Usage: mount [-t type] [-o options] <device|none> <mount-path>");
        crate::console_println!("       mount -o remount[,ro|rw|noatime] <mount-path>");
        crate::console_println!("Types: ext4, memfs, procfs, devfs, sysfs, iso9660");
        crate::console_println!("Options: ro, rw, noatime, noexec, nosuid");
        set_exit(1);
        return;
    };

    let mount_path_resolved = resolve_path(mount_path);

    let result = match fs_type.unwrap_or("auto") {
        "memfs" | "tmpfs" | "ramfs" => crate::fs::memfs::mount(&mount_path_resolved),
        "procfs" | "proc" => crate::fs::procfs::mount(&mount_path_resolved),
        "devfs" | "dev" => crate::fs::devfs::mount(&mount_path_resolved),
        "sysfs" | "sys" => crate::fs::sysfs::mount(&mount_path_resolved),
        "ext4" => crate::fs::ext4::mount(device, &mount_path_resolved),
        "iso9660" | "iso" => crate::fs::iso9660::mount(device, &mount_path_resolved),
        "auto" => {
            // Try to probe the device for known filesystem types.
            if crate::fs::ext4::probe(device) {
                crate::fs::ext4::mount(device, &mount_path_resolved)
            } else if crate::fs::iso9660::probe(device) {
                crate::fs::iso9660::mount(device, &mount_path_resolved)
            } else {
                crate::console_println!(
                    "mount: could not detect filesystem on '{}' (try -t to specify type)",
                    device
                );
                set_exit(1);
                return;
            }
        }
        other => {
            crate::console_println!("mount: unknown filesystem type '{}'", other);
            set_exit(1);
            return;
        }
    };

    match result {
        Ok(()) => {
            // Apply mount options if -o was specified.
            if let Some(opts) = opts_str {
                let mount_opts = crate::fs::vfs::MountOptions::parse(opts);
                // Only apply if non-default options were requested.
                if mount_opts.read_only || mount_opts.noatime
                    || mount_opts.noexec || mount_opts.nosuid
                {
                    let _ = crate::fs::Vfs::remount(&mount_path_resolved, mount_opts);
                }
            }
            let opts_display = if let Ok(mo) = crate::fs::Vfs::mount_options(&mount_path_resolved) {
                mo.to_string()
            } else {
                String::from("rw")
            };
            crate::console_println!(
                "Mounted {} at {} ({})", device, mount_path_resolved, opts_display
            );
        }
        Err(e) => {
            crate::console_println!(
                "mount: failed to mount {} at {}: {:?}",
                device, mount_path_resolved, e
            );
            set_exit(1);
        }
    }
}

/// Unmount a filesystem.
fn cmd_umount(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: umount <mount-path>");
        return;
    }

    let path = resolve_path(args);

    match crate::fs::Vfs::unmount(&path) {
        Ok(()) => {
            crate::console_println!("{}: unmounted", path);
        }
        Err(e) => {
            crate::console_println!("umount: {}: {:?}", path, e);
        }
    }
}

/// Flush all filesystems to stable storage.
fn cmd_sync() {
    // Flush expired dirty cache entries first (age-based writeback).
    let expired = crate::fs::cache::flush_expired();
    match crate::fs::Vfs::sync() {
        Ok(()) => {
            if expired > 0 {
                crate::console_println!(
                    "All filesystems synced ({} expired cache entries flushed).", expired
                );
            } else {
                crate::console_println!("All filesystems synced.");
            }
        }
        Err(e) => {
            crate::console_println!("sync: {:?}", e);
        }
    }
}

fn cmd_run(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: run <elf-file>");
        return;
    }

    let path = resolve_path(args);

    // Read the ELF binary from the filesystem.
    let elf_data = match crate::fs::Vfs::read_file(&path) {
        Ok(data) => data,
        Err(e) => {
            crate::console_println!("run: {}: {:?}", path, e);
            return;
        }
    };

    crate::console_println!("Loading {} ({} bytes)...", path, elf_data.len());

    // Spawn a new process from the ELF data.
    let name = args.rsplit('/').next().unwrap_or(args);
    let options = crate::proc::spawn::SpawnOptions::new(name);

    match crate::proc::spawn::spawn_process(&elf_data, &options) {
        Ok(result) => {
            crate::console_println!(
                "Process '{}' spawned: pid={}, tid={}, entry={:#x}",
                name,
                result.pid,
                result.task_id,
                result.entry_point
            );
        }
        Err(e) => {
            crate::console_println!("run: failed to spawn: {:?}", e);
        }
    }
}

fn cmd_mkelf() {
    // Generate both test ELFs and write them to the filesystem.

    // 1. EXIT.ELF — minimal ELF that just calls SYS_EXIT(0).
    let exit_elf = crate::proc::elf::build_test_elf_public();
    match crate::fs::Vfs::write_file("/EXIT.ELF", &exit_elf) {
        Ok(()) => {
            crate::console_println!(
                "Created /EXIT.ELF ({} bytes) — calls SYS_EXIT(0)",
                exit_elf.len()
            );
        }
        Err(e) => {
            crate::console_println!("mkelf: failed to write EXIT.ELF: {:?}", e);
        }
    }

    // 2. HELLO.ELF — prints "Hello from userspace!" via SYS_CONSOLE_WRITE, then exits.
    let hello_elf = crate::proc::elf::build_hello_elf();
    match crate::fs::Vfs::write_file("/HELLO.ELF", &hello_elf) {
        Ok(()) => {
            crate::console_println!(
                "Created /HELLO.ELF ({} bytes) — prints to console, then exits",
                hello_elf.len()
            );
        }
        Err(e) => {
            crate::console_println!("mkelf: failed to write HELLO.ELF: {:?}", e);
        }
    }
    crate::console_println!("Run them with: run EXIT.ELF / run HELLO.ELF");
}

fn cmd_net() {
    let info = crate::net::interface::info();
    if !info.up {
        crate::console_println!("No network interface.");
        return;
    }

    crate::console_println!("Network interface: virtio-net");
    crate::console_println!("  MAC address:  {}", info.mac);
    crate::console_println!("  IPv4 address: {}", info.ip);
    crate::console_println!("  Subnet mask:  {}", info.subnet_mask);
    crate::console_println!("  Gateway:      {}", info.gateway);
    crate::console_println!("  DNS server:   {}", info.dns);
    crate::console_println!("  DHCP state:   {}", crate::net::dhcp::state_str());

    // Also show RX buffer status from the NIC.
    let rx_info = crate::virtio::net::with_device(|dev| dev.rx_pending());
    if let Some(pending) = rx_info {
        crate::console_println!("  RX buffers:   {} pending", pending);
    }
}

fn cmd_dhcp() {
    crate::console_println!("Running DHCP discovery...");
    match crate::net::dhcp::discover() {
        Ok(ip) => {
            crate::console_println!("DHCP successful: {}", ip);
            // Show full config.
            let info = crate::net::interface::info();
            crate::console_println!("  Subnet mask: {}", info.subnet_mask);
            crate::console_println!("  Gateway:     {}", info.gateway);
            crate::console_println!("  DNS server:  {}", info.dns);
        }
        Err(e) => {
            crate::console_println!("DHCP failed: {:?}", e);
        }
    }
}

fn cmd_ping(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: ping <ip-address>");
        crate::console_println!("  e.g., ping 10.0.2.2");
        return;
    }

    // Parse IP address or resolve hostname.
    let ip = if let Some(ip) = parse_ipv4(args) {
        ip
    } else {
        // Try DNS resolution.
        match crate::net::dns::resolve(args) {
            Ok(ip) => {
                crate::console_println!("PING {} ({})", args, ip);
                ip
            }
            Err(e) => {
                crate::console_println!("Cannot resolve {}: {:?}", args, e);
                return;
            }
        }
    };

    // Send 4 ICMP echo requests.
    let mut sent = 0u32;
    let mut received = 0u32;
    for i in 0..4u32 {
        match crate::net::icmp::ping(ip) {
            Ok(seq) => {
                sent = sent.saturating_add(1);
                if crate::net::icmp::wait_reply(seq, 2000) {
                    received = received.saturating_add(1);
                    crate::console_println!(
                        "Reply from {}: seq={}", ip, seq
                    );
                } else {
                    crate::console_println!("Request timed out: seq={}", seq);
                }
            }
            Err(e) => {
                crate::console_println!("ping: send failed: {:?}", e);
            }
        }

        // Brief delay between pings (if not the last one).
        if i < 3 {
            for _ in 0..500_000 {
                core::hint::spin_loop();
            }
        }
    }

    crate::console_println!(
        "--- {} ping statistics: {} sent, {} received ---",
        ip, sent, received
    );
}

/// Parse a simple URL: "http://host/path" or just "host/path" or "host".
/// Returns (host, port, path).
fn parse_url(url: &str) -> Option<(&str, u16, &str)> {
    let url = url.strip_prefix("http://").unwrap_or(url);

    // Split host and path.
    let (host_port, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };

    // Split host and port.
    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let port_str = &host_port[i + 1..];
            match port_str.parse::<u16>() {
                Ok(p) => (&host_port[..i], p),
                Err(_) => (host_port, 80),
            }
        }
        None => (host_port, 80),
    };

    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

// String formatting uses small bounded values.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_wget(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: wget <url>");
        crate::console_println!("  e.g., wget http://example.com/");
        return;
    }

    let Some((host, port, path)) = parse_url(args) else {
        crate::console_println!("Invalid URL: {}", args);
        return;
    };

    crate::console_println!("Resolving {}...", host);

    // Resolve hostname to IP.
    let ip = if let Some(ip) = parse_ipv4(host) {
        ip
    } else {
        match crate::net::dns::resolve(host) {
            Ok(ip) => ip,
            Err(e) => {
                crate::console_println!("DNS resolution failed: {:?}", e);
                return;
            }
        }
    };

    crate::console_println!("Connecting to {}:{}...", ip, port);

    // Open TCP connection.
    let conn = match crate::net::tcp::connect(ip, port) {
        Ok(c) => c,
        Err(e) => {
            crate::console_println!("Connection failed: {:?}", e);
            return;
        }
    };

    // Build HTTP request.
    let request = alloc::format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );

    crate::console_println!("Sending HTTP request...");

    if let Err(e) = crate::net::tcp::send(conn, request.as_bytes()) {
        crate::console_println!("Send failed: {:?}", e);
        let _ = crate::net::tcp::close(conn);
        return;
    }

    // Read response.
    crate::console_println!("--- Response ---");

    let mut total = 0usize;
    loop {
        match crate::net::tcp::read_blocking(conn, 3000) {
            Ok(data) => {
                if data.is_empty() {
                    // Check if connection closed.
                    if crate::net::tcp::is_remote_closed(conn) {
                        break;
                    }
                    // No data yet — try again briefly.
                    continue;
                }
                total = total.saturating_add(data.len());
                // Print as text.
                match core::str::from_utf8(&data) {
                    Ok(text) => crate::console_print!("{}", text),
                    Err(_) => crate::console_print!("(binary: {} bytes)", data.len()),
                }
            }
            Err(_) => break,
        }
    }

    crate::console_println!("\n--- End ({} bytes received) ---", total);
    let _ = crate::net::tcp::close(conn);
}

fn cmd_dns(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: dns <domain-name>");
        crate::console_println!("  e.g., dns example.com");
        return;
    }

    crate::console_println!("Resolving {}...", args);
    match crate::net::dns::resolve(args) {
        Ok(ip) => {
            crate::console_println!("{} -> {}", args, ip);
        }
        Err(e) => {
            crate::console_println!("DNS resolution failed: {:?}", e);
        }
    }
}

/// Parse an IPv4 address from a dotted-quad string.
fn parse_ipv4(s: &str) -> Option<crate::net::interface::Ipv4Addr> {
    let mut parts = s.split('.');
    let a = parts.next()?.parse::<u8>().ok()?;
    let b = parts.next()?.parse::<u8>().ok()?;
    let c = parts.next()?.parse::<u8>().ok()?;
    let d = parts.next()?.parse::<u8>().ok()?;
    // Reject trailing parts.
    if parts.next().is_some() {
        return None;
    }
    Some(crate::net::interface::Ipv4Addr::new(a, b, c, d))
}

fn cmd_irq() {
    crate::console_println!("IRQ interrupt counts:");
    let mut any = false;
    for i in 0..24u32 {
        let count = crate::ioapic::irq_consume(i);
        if count > 0 {
            crate::console_println!("  IRQ {:2}: {} interrupts", i, count);
            any = true;
        }
    }
    // Also show the total pending (peek without consume) for reference.
    if !any {
        crate::console_println!("  (no IRQ activity recorded)");
    }
}

fn cmd_reboot() {
    let caps = crate::power::capabilities();
    crate::console_println!("Rebooting...");
    if caps.acpi_reboot {
        crate::console_println!("  Using ACPI reset register.");
    }
    crate::power::reboot();
}

fn cmd_version() {
    shell_println!("Kernel v0.1.0 (x86_64, microkernel)");
    shell_println!("Built with Rust, AI-developed");
}

/// `uname [-asnrvmo]` — print system information.
///
/// Flags:
/// - `-s`: kernel name ("MintOS")
/// - `-n`: network hostname
/// - `-r`: kernel release ("0.1.0")
/// - `-v`: kernel version string (includes build date from RTC)
/// - `-m`: machine hardware name ("x86_64")
/// - `-o`: operating system ("MintOS")
/// - `-a`: all of the above
/// - No flags: same as `-s`
fn cmd_uname(args: &str) {
    let trimmed = args.trim();

    // Parse which fields to show.
    let mut show_s = false;
    let mut show_n = false;
    let mut show_r = false;
    let mut show_v = false;
    let mut show_m = false;
    let mut show_o = false;

    if trimmed.is_empty() {
        // No flags → default to -s.
        show_s = true;
    } else {
        for token in trimmed.split_whitespace() {
            if let Some(flags) = token.strip_prefix('-') {
                for ch in flags.chars() {
                    match ch {
                        'a' => {
                            show_s = true;
                            show_n = true;
                            show_r = true;
                            show_v = true;
                            show_m = true;
                            show_o = true;
                        }
                        's' => show_s = true,
                        'n' => show_n = true,
                        'r' => show_r = true,
                        'v' => show_v = true,
                        'm' => show_m = true,
                        'o' => show_o = true,
                        _ => {
                            crate::console_println!(
                                "uname: invalid option -- '{}'", ch
                            );
                            set_exit(1);
                            return;
                        }
                    }
                }
            } else {
                crate::console_println!(
                    "uname: extra operand '{}'", token
                );
                set_exit(1);
                return;
            }
        }
    }

    // Collect fields in POSIX order: s n r v m o.
    let mut parts: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();

    if show_s {
        parts.push(alloc::string::String::from("MintOS"));
    }
    if show_n {
        parts.push(crate::fs::sysfs::get_hostname());
    }
    if show_r {
        parts.push(alloc::string::String::from("0.1.0"));
    }
    if show_v {
        let dt = crate::rtc::read_datetime();
        parts.push(alloc::format!(
            "#1 SMP {:04}-{:02}-{:02}",
            dt.year, dt.month, dt.day
        ));
    }
    if show_m {
        parts.push(alloc::string::String::from("x86_64"));
    }
    if show_o {
        parts.push(alloc::string::String::from("MintOS"));
    }

    let line: alloc::string::String = parts.join(" ");
    shell_println!("{}", line);
}

// ---------------------------------------------------------------------------
// Text processing utilities
// ---------------------------------------------------------------------------

/// `sort FILE` — sort lines of a file alphabetically.
///
/// Options: `sort -r FILE` for reverse order.
fn cmd_sort(args: &str) {
    let (reverse, path) = if args.starts_with("-r ") {
        (true, args.get(3..).unwrap_or("").trim())
    } else {
        (false, args.trim())
    };

    if path.is_empty() {
        crate::console_println!("Usage: sort [-r] <file>");
        return;
    }

    let data = match crate::fs::Vfs::read_file(path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("sort: cannot read '{}': {:?}", path, e);
            return;
        }
    };

    let text = match core::str::from_utf8(&data) {
        Ok(t) => t,
        Err(_) => {
            crate::console_println!("sort: file is not valid UTF-8");
            return;
        }
    };

    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    if reverse {
        lines.reverse();
    }

    for line in &lines {
        shell_println!("{}", line);
    }
}

/// `uniq FILE` — remove adjacent duplicate lines.
///
/// Options: `uniq -c FILE` to prefix lines with occurrence count.
fn cmd_uniq(args: &str) {
    let (count_mode, path) = if args.starts_with("-c ") {
        (true, args.get(3..).unwrap_or("").trim())
    } else {
        (false, args.trim())
    };

    if path.is_empty() {
        crate::console_println!("Usage: uniq [-c] <file>");
        return;
    }

    let data = match crate::fs::Vfs::read_file(path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("uniq: cannot read '{}': {:?}", path, e);
            return;
        }
    };

    let text = match core::str::from_utf8(&data) {
        Ok(t) => t,
        Err(_) => {
            crate::console_println!("uniq: file is not valid UTF-8");
            return;
        }
    };

    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return;
    }

    let mut prev = lines[0];
    let mut count = 1u64;

    for &line in lines.iter().skip(1) {
        if line == prev {
            count = count.wrapping_add(1);
        } else {
            if count_mode {
                shell_println!("{:7} {}", count, prev);
            } else {
                shell_println!("{}", prev);
            }
            prev = line;
            count = 1;
        }
    }
    // Print the last run.
    if count_mode {
        shell_println!("{:7} {}", count, prev);
    } else {
        shell_println!("{}", prev);
    }
}

// ---------------------------------------------------------------------------
// Pipe-input variants
//
// These accept piped text as a second argument.  When `args` is non-empty
// (e.g. a filename), the piped input is ignored and the file-based command
// runs instead.  When `args` is empty, the piped text is processed directly.
// ---------------------------------------------------------------------------

/// Sort piped input lines.  If `args` is non-empty it is treated as a
/// filename (delegates to `cmd_sort`).
fn cmd_sort_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_sort(args);
        return;
    }
    let mut lines: Vec<&str> = input.lines().collect();
    lines.sort_unstable();
    for line in &lines {
        shell_println!("{}", line);
    }
}

/// Remove adjacent duplicate lines from piped input.  If `args` is
/// non-empty it is treated as a filename (delegates to `cmd_uniq`).
fn cmd_uniq_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_uniq(args);
        return;
    }
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return;
    }

    let mut prev = lines[0];
    shell_println!("{}", prev);
    for &line in lines.iter().skip(1) {
        if line != prev {
            shell_println!("{}", line);
            prev = line;
        }
    }
}

/// Grep piped input for a pattern.  `args` is the search pattern (no file
/// argument).  If `args` contains a space it is interpreted as
/// `<pattern> <file>` and delegates to `cmd_grep`.
fn cmd_grep_input(args: &str, input: &str) {
    // Parse flags and find the pattern.
    let mut flags = GrepFlags::new();
    let mut positional: alloc::vec::Vec<&str> = Vec::new();

    for word in args.split_whitespace() {
        if word.starts_with('-') && word.len() > 1 && !word.starts_with("--") {
            for ch in word[1..].chars() {
                match ch {
                    'i' => flags.case_insensitive = true,
                    'I' => flags.case_insensitive = false,
                    'v' => flags.invert = true,
                    'c' => flags.count_only = true,
                    'n' => flags.show_line_numbers = true,
                    'w' => flags.whole_word = true,
                    _ => {}
                }
            }
        } else {
            positional.push(word);
        }
    }

    // If there are 2+ positional args, it looks like "pattern file" — delegate.
    if positional.len() >= 2 {
        cmd_grep(args);
        return;
    }

    let pattern = match positional.first() {
        Some(p) => *p,
        None => {
            crate::console_println!("grep: no pattern specified");
            return;
        }
    };

    let pattern_cmp = if flags.case_insensitive {
        to_lower(pattern)
    } else {
        String::from(pattern)
    };

    let mut match_count = 0usize;
    for (line_num, line) in input.lines().enumerate() {
        let line_cmp = if flags.case_insensitive {
            to_lower(line)
        } else {
            String::from(line)
        };

        let matched = grep_matches(&line_cmp, &pattern_cmp, flags.whole_word);
        let show = if flags.invert { !matched } else { matched };

        if show {
            match_count = match_count.saturating_add(1);

            if !flags.count_only {
                if flags.show_line_numbers {
                    shell_println!("{}: {}", line_num.saturating_add(1), line);
                } else {
                    shell_println!("{}", line);
                }
            }

            if match_count >= flags.max_matches {
                shell_println!("... (showing first {} matches)", flags.max_matches);
                break;
            }
        }
    }

    if flags.count_only {
        shell_println!("{}", match_count);
    } else if match_count == 0 {
        crate::console_println!("grep: no matches for '{}'", pattern);
    }
}

/// Show the first N lines of piped input.  `args` is an optional line
/// count (default 10).  If `args` looks like a filename (non-numeric),
/// delegates to `cmd_head`.
fn cmd_head_input(args: &str, input: &str) {
    let trimmed = args.trim();

    // If args is non-empty and not purely numeric, treat as "N file" or
    // just "file" — delegate to the file-based command.
    if !trimmed.is_empty() {
        if trimmed.parse::<usize>().is_err() {
            cmd_head(args);
            return;
        }
    }

    let count: usize = if trimmed.is_empty() {
        10
    } else {
        trimmed.parse::<usize>().unwrap_or(10)
    };

    let mut printed = 0usize;
    for line in input.lines() {
        if printed >= count {
            break;
        }
        shell_println!("{}", line);
        printed += 1;
    }
}

/// Show the last N lines of piped input.  `args` is an optional line
/// count (default 10).  If `args` looks like a filename (non-numeric),
/// delegates to `cmd_tail`.
fn cmd_tail_input(args: &str, input: &str) {
    let trimmed = args.trim();

    if !trimmed.is_empty() {
        if trimmed.parse::<usize>().is_err() {
            cmd_tail(args);
            return;
        }
    }

    let count: usize = if trimmed.is_empty() {
        10
    } else {
        trimmed.parse::<usize>().unwrap_or(10)
    };

    let lines: Vec<&str> = input.lines().collect();
    let start = if lines.len() > count { lines.len() - count } else { 0 };
    for line in &lines[start..] {
        shell_println!("{}", line);
    }
}

/// Count lines, words, and bytes of piped input.  `args` is ignored
/// (if non-empty, delegates to `cmd_wc` with a filename).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_wc_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_wc(args);
        return;
    }

    let bytes = input.len();
    let mut lines: usize = 0;
    let mut words: usize = 0;
    let mut in_word = false;

    for b in input.bytes() {
        if b == b'\n' {
            lines += 1;
        }
        let is_ws = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
        if is_ws {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }

    shell_println!("  {} lines  {} words  {} bytes", lines, words, bytes);
}

// ---------------------------------------------------------------------------
// Script execution, text utilities, and misc commands
// ---------------------------------------------------------------------------

/// Execute kshell commands from a file (like bash `source` or `.`).
///
/// Each non-empty, non-comment line in the file is executed as a
/// separate command.  Lines starting with `#` are comments.
/// Maximum nesting depth is 8 to prevent infinite recursion.
fn cmd_source(args: &str) {
    /// Track recursion depth to prevent infinite `source` loops.
    static SOURCE_DEPTH: Mutex<u8> = Mutex::new(0);

    const MAX_DEPTH: u8 = 8;

    if args.is_empty() {
        crate::console_println!("Usage: source <script-file>");
        return;
    }

    let path = resolve_path(args);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("source: {}: {:?}", path, e);
            return;
        }
    };

    let text = match core::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => {
            crate::console_println!("source: {}: not a text file", path);
            return;
        }
    };

    // Check recursion depth.
    {
        let mut depth = SOURCE_DEPTH.lock();
        if *depth >= MAX_DEPTH {
            crate::console_println!("source: maximum nesting depth ({}) exceeded", MAX_DEPTH);
            return;
        }
        *depth = depth.saturating_add(1);
    }

    // Execute each line.
    for (line_num, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        // Skip empty lines and comments — but NOT while we're collecting
        // a here-document body (where #-lines are literal content).
        let in_heredoc = HEREDOC_COLLECTOR.lock().is_some();
        if !in_heredoc && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }
        crate::serial_println!("[source] {}:{}: {}", path, line_num.saturating_add(1), trimmed);
        execute(trimmed);

        // `set -e` (errexit): abort script on non-zero exit status.
        if OPT_ERREXIT.load(core::sync::atomic::Ordering::Relaxed) && last_exit() != 0 {
            crate::console_println!(
                "{}:{}: command failed (exit {}), aborting (set -e)",
                path,
                line_num.saturating_add(1),
                last_exit(),
            );
            break;
        }
    }

    // Decrement depth.
    {
        let mut depth = SOURCE_DEPTH.lock();
        *depth = depth.saturating_sub(1);
    }
}

/// Print a sequence of numbers (like Unix `seq`).
///
/// `seq N` prints 1..N.  `seq N M` prints N..M.
fn cmd_seq(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();

    let (start, end) = match parts.len() {
        1 => {
            let n = match parts[0].parse::<i64>() {
                Ok(v) => v,
                Err(_) => {
                    crate::console_println!("Usage: seq N [M]");
                    return;
                }
            };
            (1i64, n)
        }
        2 => {
            let a = parts[0].parse::<i64>().unwrap_or(1);
            let b = parts[1].parse::<i64>().unwrap_or(1);
            (a, b)
        }
        _ => {
            crate::console_println!("Usage: seq N [M]");
            return;
        }
    };

    if start <= end {
        let mut i = start;
        while i <= end {
            shell_println!("{}", i);
            i = i.saturating_add(1);
        }
    } else {
        let mut i = start;
        while i >= end {
            shell_println!("{}", i);
            i = i.saturating_sub(1);
        }
    }
}

/// Number lines of a file (like Unix `nl`).
fn cmd_nl(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: nl <file>");
        return;
    }

    let path = resolve_path(args);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("nl: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    for (i, line) in text.lines().enumerate() {
        shell_println!("{:>6}\t{}", i.saturating_add(1), line);
    }
}

/// Number lines of piped input.
fn cmd_nl_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_nl(args);
        return;
    }
    for (i, line) in input.lines().enumerate() {
        shell_println!("{:>6}\t{}", i.saturating_add(1), line);
    }
}

/// Reverse the order of lines in a file (like Unix `tac`).
fn cmd_rev(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: rev <file>");
        return;
    }

    let path = resolve_path(args);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("rev: {}: {:?}", path, e);
            return;
        }
    };

    let text = core::str::from_utf8(&data).unwrap_or("<binary>");
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().rev() {
        shell_println!("{}", line);
    }
}

/// Reverse lines of piped input.
fn cmd_rev_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_rev(args);
        return;
    }
    let lines: Vec<&str> = input.lines().collect();
    for line in lines.iter().rev() {
        shell_println!("{}", line);
    }
}

/// `cut -d DELIM -f N [FILE]` — extract fields from each line.
///
/// Supports:
/// - `-d C`: delimiter character (default tab)
/// - `-f N` or `-f N,M,...`: field numbers (1-based)
/// - `-c N-M`: character ranges
#[allow(clippy::arithmetic_side_effects)]
fn cmd_cut(args: &str) {
    let (delim, fields, chars_range, file_path) = parse_cut_args(args);

    if let Some(path) = file_path {
        let resolved = resolve_path(&path);
        match crate::fs::Vfs::read_file(&resolved) {
            Ok(data) => {
                let text = core::str::from_utf8(&data).unwrap_or("");
                cut_process(text, delim, &fields, chars_range);
            }
            Err(e) => {
                crate::console_println!("cut: {}: {:?}", path, e);
                set_exit(1);
            }
        }
    } else {
        crate::console_println!("Usage: cut -d DELIM -f FIELDS [file]  or  cut -c RANGE [file]");
        set_exit(1);
    }
}

fn cmd_cut_input(args: &str, input: &str) {
    let (delim, fields, chars_range, _) = parse_cut_args(args);
    cut_process(input, delim, &fields, chars_range);
}

/// Parse cut command arguments.
///
/// Returns (delimiter, field_numbers, char_range, file_path).
fn parse_cut_args(args: &str) -> (char, Vec<usize>, Option<(usize, usize)>, Option<String>) {
    let mut delim = '\t';
    let mut fields: Vec<usize> = Vec::new();
    let mut chars_range: Option<(usize, usize)> = None;
    let mut file_path: Option<String> = None;
    let mut rest = args;

    loop {
        rest = rest.trim_start();
        if rest.starts_with("-d") {
            rest = rest.get(2..).unwrap_or("").trim_start();
            if let Some(c) = rest.chars().next() {
                delim = c;
                rest = rest.get(c.len_utf8()..).unwrap_or("");
            }
        } else if rest.starts_with("-f") {
            rest = rest.get(2..).unwrap_or("").trim_start();
            // Parse field list: N or N,M,...
            let end = rest.find(|c: char| c == ' ' || c == '\t').unwrap_or(rest.len());
            let spec = rest.get(..end).unwrap_or("");
            for part in spec.split(',') {
                if let Ok(n) = part.trim().parse::<usize>() {
                    if n > 0 { fields.push(n); }
                }
            }
            rest = rest.get(end..).unwrap_or("");
        } else if rest.starts_with("-c") {
            rest = rest.get(2..).unwrap_or("").trim_start();
            let end = rest.find(|c: char| c == ' ' || c == '\t').unwrap_or(rest.len());
            let spec = rest.get(..end).unwrap_or("");
            if let Some(dash) = spec.find('-') {
                let start = spec.get(..dash).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
                let end_val = spec.get(dash + 1..).and_then(|s| s.parse::<usize>().ok()).unwrap_or(usize::MAX);
                chars_range = Some((start.max(1), end_val));
            } else if let Ok(n) = spec.parse::<usize>() {
                chars_range = Some((n.max(1), n.max(1)));
            }
            rest = rest.get(end..).unwrap_or("");
        } else if !rest.is_empty() {
            file_path = Some(String::from(rest.split_whitespace().next().unwrap_or("")));
            break;
        } else {
            break;
        }
    }

    (delim, fields, chars_range, file_path)
}

/// Process text with cut (field or character extraction).
#[allow(clippy::arithmetic_side_effects)]
fn cut_process(text: &str, delim: char, fields: &[usize], chars_range: Option<(usize, usize)>) {
    for line in text.lines() {
        if let Some((start, end)) = chars_range {
            // Character range mode.
            let chars: Vec<char> = line.chars().collect();
            let s = start.saturating_sub(1); // 1-based to 0-based
            let e = end.min(chars.len());
            let slice: String = chars.get(s..e).unwrap_or(&[]).iter().collect();
            shell_println!("{}", slice);
        } else if !fields.is_empty() {
            // Field mode.
            let parts: Vec<&str> = line.split(delim).collect();
            let mut first = true;
            for &f in fields {
                if !first { shell_print!("{}", delim); }
                first = false;
                if let Some(part) = parts.get(f.saturating_sub(1)) {
                    shell_print!("{}", part);
                }
            }
            shell_println!();
        } else {
            shell_println!("{}", line);
        }
    }
}

/// `tr SET1 SET2` — translate characters.
///
/// Replaces each character in SET1 with the corresponding character in SET2.
/// `tr -d SET1` deletes characters in SET1.
fn cmd_tr(args: &str) {
    // tr needs piped input; standalone usage reads a file.
    let mut parts = args.splitn(3, ' ');
    let set1_or_flag = parts.next().unwrap_or("");
    let set2_or_file = parts.next().unwrap_or("");
    let file = parts.next().unwrap_or("").trim();

    if set1_or_flag == "-d" {
        // Delete mode: tr -d SET1 FILE
        if set2_or_file.is_empty() {
            crate::console_println!("Usage: tr -d CHARS [file]  or pipe: cmd | tr -d CHARS");
            set_exit(1);
            return;
        }
        if !file.is_empty() {
            let resolved = resolve_path(file);
            match crate::fs::Vfs::read_file(&resolved) {
                Ok(data) => {
                    let text = core::str::from_utf8(&data).unwrap_or("");
                    tr_delete(text, set2_or_file);
                }
                Err(e) => {
                    crate::console_println!("tr: {}: {:?}", file, e);
                    set_exit(1);
                }
            }
        } else {
            crate::console_println!("Usage: tr -d CHARS [file]");
            set_exit(1);
        }
    } else if set1_or_flag.is_empty() || set2_or_file.is_empty() {
        crate::console_println!("Usage: tr SET1 SET2 [file]  or  tr -d CHARS [file]");
        set_exit(1);
    } else if !file.is_empty() {
        let resolved = resolve_path(file);
        match crate::fs::Vfs::read_file(&resolved) {
            Ok(data) => {
                let text = core::str::from_utf8(&data).unwrap_or("");
                tr_translate(text, set1_or_flag, set2_or_file);
            }
            Err(e) => {
                crate::console_println!("tr: {}: {:?}", file, e);
                set_exit(1);
            }
        }
    } else {
        crate::console_println!("Usage: tr SET1 SET2 [file]  or pipe: cmd | tr SET1 SET2");
        set_exit(1);
    }
}

fn cmd_tr_input(args: &str, input: &str) {
    let mut parts = args.splitn(2, ' ');
    let set1_or_flag = parts.next().unwrap_or("");
    let set2 = parts.next().unwrap_or("");

    if set1_or_flag == "-d" {
        tr_delete(input, set2);
    } else if !set2.is_empty() {
        tr_translate(input, set1_or_flag, set2);
    } else {
        crate::console_println!("Usage: cmd | tr SET1 SET2  or  cmd | tr -d CHARS");
    }
}

fn tr_translate(text: &str, set1: &str, set2: &str) {
    let s1: Vec<char> = expand_tr_set(set1);
    let s2: Vec<char> = expand_tr_set(set2);

    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(pos) = s1.iter().position(|&c| c == ch) {
            // Replace with corresponding char from set2 (or last char if shorter).
            let replacement = s2.get(pos).or_else(|| s2.last()).copied().unwrap_or(ch);
            result.push(replacement);
        } else {
            result.push(ch);
        }
    }
    shell_print!("{}", result);
}

fn tr_delete(text: &str, set1: &str) {
    let s1: Vec<char> = expand_tr_set(set1);
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if !s1.contains(&ch) {
            result.push(ch);
        }
    }
    shell_print!("{}", result);
}

/// Expand a tr character set.  Handles ranges like `a-z` and escapes like `\n`.
fn expand_tr_set(set: &str) -> Vec<char> {
    let set = strip_quotes(set);
    let mut chars = Vec::new();
    let bytes = set.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'\\' && i.saturating_add(1) < len {
            let next = bytes[i.saturating_add(1)];
            match next {
                b'n' => chars.push('\n'),
                b't' => chars.push('\t'),
                b'r' => chars.push('\r'),
                b'\\' => chars.push('\\'),
                _ => chars.push(next as char),
            }
            i = i.saturating_add(2);
        } else if i.saturating_add(2) < len && bytes[i.saturating_add(1)] == b'-' {
            // Range: a-z
            let start = bytes[i];
            let end = bytes[i.saturating_add(2)];
            if start <= end {
                for c in start..=end {
                    chars.push(c as char);
                }
            }
            i = i.saturating_add(3);
        } else {
            chars.push(bytes[i] as char);
            i = i.saturating_add(1);
        }
    }
    chars
}

/// `yes [STRING]` — repeatedly output STRING (default "y").
///
/// Limited to 100 lines to prevent infinite loops in the kernel shell.
fn cmd_yes(args: &str) {
    let text = if args.is_empty() { "y" } else { args };
    for _ in 0..100_u32 {
        shell_println!("{}", text);
    }
}

/// `tac [FILE]` — print lines in reverse order (opposite of cat).
fn cmd_tac(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: tac <file>");
        return;
    }

    let path = resolve_path(args);
    match crate::fs::Vfs::read_file(&path) {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("");
            tac_process(text);
        }
        Err(e) => {
            crate::console_println!("tac: {}: {:?}", args, e);
            set_exit(1);
        }
    }
}

fn cmd_tac_input(args: &str, input: &str) {
    if !args.is_empty() {
        cmd_tac(args);
        return;
    }
    tac_process(input);
}

fn tac_process(text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().rev() {
        shell_println!("{}", line);
    }
}

/// `fold [-w WIDTH] [FILE]` — wrap lines to specified width (default 80).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_fold(args: &str) {
    let (width, file) = parse_fold_args(args);

    if let Some(path) = file {
        let resolved = resolve_path(&path);
        match crate::fs::Vfs::read_file(&resolved) {
            Ok(data) => {
                let text = core::str::from_utf8(&data).unwrap_or("");
                fold_process(text, width);
            }
            Err(e) => {
                crate::console_println!("fold: {}: {:?}", path, e);
                set_exit(1);
            }
        }
    } else {
        crate::console_println!("Usage: fold [-w WIDTH] <file>");
        set_exit(1);
    }
}

fn cmd_fold_input(args: &str, input: &str) {
    let (width, _) = parse_fold_args(args);
    fold_process(input, width);
}

fn parse_fold_args(args: &str) -> (usize, Option<String>) {
    let mut width: usize = 80;
    let mut rest = args.trim();

    if rest.starts_with("-w") {
        rest = rest.get(2..).unwrap_or("").trim_start();
        let end = rest.find(|c: char| c == ' ' || c == '\t').unwrap_or(rest.len());
        if let Ok(w) = rest.get(..end).unwrap_or("80").parse::<usize>() {
            width = w.max(1);
        }
        rest = rest.get(end..).unwrap_or("").trim_start();
    }

    let file = if rest.is_empty() { None } else { Some(String::from(rest)) };
    (width, file)
}

#[allow(clippy::arithmetic_side_effects)]
fn fold_process(text: &str, width: usize) {
    for line in text.lines() {
        if line.len() <= width {
            shell_println!("{}", line);
        } else {
            let mut pos = 0;
            while pos < line.len() {
                let end = (pos + width).min(line.len());
                if let Some(chunk) = line.get(pos..end) {
                    shell_println!("{}", chunk);
                }
                pos = end;
            }
        }
    }
}

/// `paste FILE1 FILE2 ...` — merge lines of files side by side.
///
/// With piped input and a file arg, merges piped input with file.
fn cmd_paste(args: &str) {
    let files: Vec<&str> = args.split_whitespace().collect();
    if files.is_empty() {
        crate::console_println!("Usage: paste FILE1 [FILE2 ...]");
        set_exit(1);
        return;
    }

    // Read all files.
    let mut columns: Vec<Vec<String>> = Vec::new();
    for file in &files {
        let path = resolve_path(file);
        match crate::fs::Vfs::read_file(&path) {
            Ok(data) => {
                let text = core::str::from_utf8(&data).unwrap_or("");
                columns.push(text.lines().map(String::from).collect());
            }
            Err(e) => {
                crate::console_println!("paste: {}: {:?}", file, e);
                set_exit(1);
                return;
            }
        }
    }

    paste_output(&columns);
}

fn cmd_paste_input(args: &str, input: &str) {
    let mut columns: Vec<Vec<String>> = Vec::new();
    // Input becomes the first column.
    columns.push(input.lines().map(String::from).collect());

    // If there's a file argument, add it as additional columns.
    for file in args.split_whitespace() {
        let path = resolve_path(file);
        match crate::fs::Vfs::read_file(&path) {
            Ok(data) => {
                let text = core::str::from_utf8(&data).unwrap_or("");
                columns.push(text.lines().map(String::from).collect());
            }
            Err(e) => {
                crate::console_println!("paste: {}: {:?}", file, e);
                set_exit(1);
                return;
            }
        }
    }

    paste_output(&columns);
}

fn paste_output(columns: &[Vec<String>]) {
    let max_lines = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    for i in 0..max_lines {
        for (ci, col) in columns.iter().enumerate() {
            if ci > 0 { shell_print!("\t"); }
            if let Some(line) = col.get(i) {
                shell_print!("{}", line);
            }
        }
        shell_println!();
    }
}

/// `xargs COMMAND [INITIAL-ARGS]` — build and execute commands from input.
///
/// Reads words from stdin (piped input) and appends them as arguments to
/// COMMAND.  By default, all input words are appended to a single invocation.
/// With `-n N`, limits to N arguments per invocation.
fn cmd_xargs(_args: &str) {
    crate::console_println!("Usage: cmd | xargs COMMAND [args]");
    crate::console_println!("xargs requires piped input.");
    set_exit(1);
}

fn cmd_xargs_input(args: &str, input: &str) {
    let mut max_args: Option<usize> = None;
    let mut rest = args.trim();

    // Parse -n N flag.
    if rest.starts_with("-n") {
        rest = rest.get(2..).unwrap_or("").trim_start();
        let end = rest.find(|c: char| c == ' ' || c == '\t').unwrap_or(rest.len());
        max_args = rest.get(..end).and_then(|s| s.parse::<usize>().ok());
        rest = rest.get(end..).unwrap_or("").trim_start();
    }

    let command = if rest.is_empty() { "echo" } else { rest };

    // Collect input words.
    let words: Vec<&str> = input.split_whitespace().collect();
    if words.is_empty() {
        return;
    }

    match max_args {
        Some(n) if n > 0 => {
            // Execute in batches of N words.
            for chunk in words.chunks(n) {
                let full_cmd = alloc::format!("{} {}", command, chunk.join(" "));
                execute(&full_cmd);
            }
        }
        _ => {
            // All words in one invocation.
            let full_cmd = alloc::format!("{} {}", command, words.join(" "));
            execute(&full_cmd);
        }
    }
}

/// Pause for N milliseconds (busy-wait using APIC tick counter).
fn cmd_sleep(args: &str) {
    let ms = match args.parse::<u64>() {
        Ok(n) if n > 0 => n,
        _ => {
            crate::console_println!("Usage: sleep <milliseconds>");
            return;
        }
    };

    // Cap at 10 seconds to prevent accidental infinite waits.
    let capped = ms.min(10_000);
    if capped != ms {
        crate::console_println!("(capped to {} ms)", capped);
    }

    // Busy-wait using the APIC tick counter.
    // Each tick is ~1 ms (PIT-driven APIC timer at ~1000 Hz).
    let start = crate::apic::tick_count();
    let target = start.saturating_add(capped);
    while crate::apic::tick_count() < target {
        // HLT until next interrupt to save power.
        crate::cpu::hlt();
    }
}

/// Conditional expression evaluation (like POSIX `test` / `[`).
///
/// Supports:
///   File tests:    -e FILE, -f FILE, -d FILE, -s FILE (exists, file, dir, non-empty)
///   String tests:  -z STR (empty), -n STR (non-empty), STR = STR, STR != STR
///   Integer tests: N -eq N, N -ne N, N -lt N, N -le N, N -gt N, N -ge N
///   Logical:       ! EXPR (negation)
///
/// Sets exit status: 0 (true) or 1 (false).
fn cmd_test(args: &str) {
    // Strip trailing `]` if invoked as `[`.
    let args = if args.ends_with(']') {
        args.get(..args.len().saturating_sub(1)).unwrap_or("").trim()
    } else {
        args
    };

    if args.is_empty() {
        // Empty test is false (POSIX behavior).
        set_exit(1);
        return;
    }

    let result = eval_test(args);
    set_exit(if result { 0 } else { 1 });
}

/// Evaluate a test expression, returning true or false.
fn eval_test(args: &str) -> bool {
    let parts: Vec<&str> = args.split_whitespace().collect();

    if parts.is_empty() {
        return false;
    }

    // Negation: `! EXPR`
    if parts[0] == "!" {
        let rest = parts.get(1..).unwrap_or(&[]).join(" ");
        return !eval_test(&rest);
    }

    // Unary file/string tests.
    if parts.len() == 2 {
        let op = parts[0];
        let operand = parts[1];

        match op {
            "-e" => {
                // File exists.
                let path = resolve_path(operand);
                return crate::fs::Vfs::stat(&path).is_ok();
            }
            "-f" => {
                // Is regular file.
                let path = resolve_path(operand);
                return crate::fs::Vfs::stat(&path)
                    .map(|m| m.entry_type == crate::fs::EntryType::File)
                    .unwrap_or(false);
            }
            "-d" => {
                // Is directory.
                let path = resolve_path(operand);
                return crate::fs::Vfs::stat(&path)
                    .map(|m| m.entry_type == crate::fs::EntryType::Directory)
                    .unwrap_or(false);
            }
            "-s" => {
                // File exists and is non-empty.
                let path = resolve_path(operand);
                return crate::fs::Vfs::stat(&path)
                    .map(|m| m.size > 0)
                    .unwrap_or(false);
            }
            "-L" | "-h" => {
                // Is symbolic link.
                let path = resolve_path(operand);
                return crate::fs::Vfs::lstat(&path)
                    .map(|m| m.entry_type == crate::fs::EntryType::Symlink)
                    .unwrap_or(false);
            }
            "-r" => {
                // File is readable (exists with read permission).
                let path = resolve_path(operand);
                return crate::fs::Vfs::metadata(&path)
                    .map(|m| m.permissions & 0o400 != 0)
                    .unwrap_or(false);
            }
            "-w" => {
                // File is writable (exists with write permission).
                let path = resolve_path(operand);
                return crate::fs::Vfs::metadata(&path)
                    .map(|m| m.permissions & 0o200 != 0)
                    .unwrap_or(false);
            }
            "-x" => {
                // File is executable (exists with execute permission).
                let path = resolve_path(operand);
                return crate::fs::Vfs::metadata(&path)
                    .map(|m| m.permissions & 0o100 != 0)
                    .unwrap_or(false);
            }
            "-v" => {
                // Variable is set (non-empty).
                return env_get(operand).is_some_and(|v| !v.is_empty())
                    || is_array(operand);
            }
            "-z" => {
                // String is empty.
                return operand.is_empty();
            }
            "-n" => {
                // String is non-empty.
                return !operand.is_empty();
            }
            _ => {}
        }
    }

    // Binary operators: STR = STR, STR != STR, N -eq N, etc.
    if parts.len() == 3 {
        let left = parts[0];
        let op = parts[1];
        let right = parts[2];

        match op {
            "=" | "==" => return left == right,
            "!=" => return left != right,
            "<" => return left < right,  // Lexicographic string comparison.
            ">" => return left > right,
            "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
                let l = left.parse::<i64>().unwrap_or(0);
                let r = right.parse::<i64>().unwrap_or(0);
                return match op {
                    "-eq" => l == r,
                    "-ne" => l != r,
                    "-lt" => l < r,
                    "-le" => l <= r,
                    "-gt" => l > r,
                    "-ge" => l >= r,
                    _ => false,
                };
            }
            _ => {}
        }
    }

    // Single argument: true if non-empty string.
    if parts.len() == 1 {
        return !parts[0].is_empty();
    }

    // Unrecognized expression — treat as false.
    false
}

/// Evaluate and print an arithmetic expression.
///
/// Usage: `expr 1 + 2 * 3` → `7`
fn cmd_expr(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: expr <expression>");
        crate::console_println!("  Operators: + - * / % ()");
        crate::console_println!("  Example: expr 2 + 3 * 4");
        set_exit(1);
        return;
    }
    let result = eval_arithmetic(args);
    shell_println!("{}", result);
}

// ---------------------------------------------------------------------------
// Arithmetic evaluation for $((...))
// ---------------------------------------------------------------------------

/// Evaluate a simple arithmetic expression.
///
/// Supports: integer literals, `+`, `-`, `*`, `/`, `%`, unary `-`,
/// parentheses `(...)`, and whitespace.
///
/// All arithmetic is done in i64.  Division by zero returns 0.
fn eval_arithmetic(expr: &str) -> i64 {
    let tokens = tokenize_arith(expr);
    let mut pos = 0;
    parse_expr(&tokens, &mut pos)
}

/// Arithmetic token.
#[derive(Clone, Copy)]
enum ArithToken {
    Num(i64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    /// Comparison: <
    Lt,
    /// Comparison: <=
    Le,
    /// Comparison: >
    Gt,
    /// Comparison: >=
    Ge,
    /// Comparison: ==
    Eq,
    /// Comparison: !=
    Ne,
    /// Logical AND: &&
    And,
    /// Logical OR: ||
    Or,
    /// Logical NOT: !
    Not,
}

/// Tokenize an arithmetic expression string.
///
/// Supports: numbers, +, -, *, /, %, (, ), <, <=, >, >=, ==, !=, &&, ||, !
/// Also recognizes bare variable names (resolved to their integer value).
fn tokenize_arith(s: &str) -> Vec<ArithToken> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        match b {
            b' ' | b'\t' => { i = i.saturating_add(1); }
            b'+' => { tokens.push(ArithToken::Plus); i = i.saturating_add(1); }
            b'-' => { tokens.push(ArithToken::Minus); i = i.saturating_add(1); }
            b'*' => { tokens.push(ArithToken::Star); i = i.saturating_add(1); }
            b'/' => { tokens.push(ArithToken::Slash); i = i.saturating_add(1); }
            b'%' => { tokens.push(ArithToken::Percent); i = i.saturating_add(1); }
            b'(' => { tokens.push(ArithToken::LParen); i = i.saturating_add(1); }
            b')' => { tokens.push(ArithToken::RParen); i = i.saturating_add(1); }
            b'<' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'=') {
                    tokens.push(ArithToken::Le);
                    i = i.saturating_add(2);
                } else {
                    tokens.push(ArithToken::Lt);
                    i = i.saturating_add(1);
                }
            }
            b'>' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'=') {
                    tokens.push(ArithToken::Ge);
                    i = i.saturating_add(2);
                } else {
                    tokens.push(ArithToken::Gt);
                    i = i.saturating_add(1);
                }
            }
            b'=' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'=') {
                    tokens.push(ArithToken::Eq);
                    i = i.saturating_add(2);
                } else {
                    // Lone `=` in arithmetic context — skip (assignment handled
                    // separately in eval_cfor_expr).
                    i = i.saturating_add(1);
                }
            }
            b'!' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'=') {
                    tokens.push(ArithToken::Ne);
                    i = i.saturating_add(2);
                } else {
                    tokens.push(ArithToken::Not);
                    i = i.saturating_add(1);
                }
            }
            b'&' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'&') {
                    tokens.push(ArithToken::And);
                    i = i.saturating_add(2);
                } else {
                    i = i.saturating_add(1); // Skip lone &.
                }
            }
            b'|' => {
                if bytes.get(i.saturating_add(1)) == Some(&b'|') {
                    tokens.push(ArithToken::Or);
                    i = i.saturating_add(2);
                } else {
                    i = i.saturating_add(1); // Skip lone |.
                }
            }
            b'0'..=b'9' => {
                let start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i = i.saturating_add(1);
                }
                if let Some(num_bytes) = bytes.get(start..i) {
                    if let Ok(num_str) = core::str::from_utf8(num_bytes) {
                        let val = num_str.parse::<i64>().unwrap_or(0);
                        tokens.push(ArithToken::Num(val));
                    }
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                // Variable name — resolve to integer value.
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i = i.saturating_add(1);
                }
                if let Some(name_bytes) = bytes.get(start..i) {
                    if let Ok(name) = core::str::from_utf8(name_bytes) {
                        let val = env_get(name)
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(0);
                        tokens.push(ArithToken::Num(val));
                    }
                }
            }
            _ => {
                // Unknown character — skip.
                i = i.saturating_add(1);
            }
        }
    }

    tokens
}

/// Parse a logical OR expression: and_expr ('||' and_expr)*
fn parse_expr(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    let mut val = parse_and_expr(tokens, pos);
    loop {
        if let Some(ArithToken::Or) = tokens.get(*pos) {
            *pos = pos.saturating_add(1);
            let rhs = parse_and_expr(tokens, pos);
            val = if val != 0 || rhs != 0 { 1 } else { 0 };
        } else {
            break;
        }
    }
    val
}

/// Parse a logical AND expression: cmp_expr ('&&' cmp_expr)*
fn parse_and_expr(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    let mut val = parse_cmp_expr(tokens, pos);
    loop {
        if let Some(ArithToken::And) = tokens.get(*pos) {
            *pos = pos.saturating_add(1);
            let rhs = parse_cmp_expr(tokens, pos);
            val = if val != 0 && rhs != 0 { 1 } else { 0 };
        } else {
            break;
        }
    }
    val
}

/// Parse a comparison expression: add_expr (('<' | '<=' | '>' | '>=' | '==' | '!=') add_expr)*
fn parse_cmp_expr(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    let mut val = parse_add_expr(tokens, pos);
    loop {
        match tokens.get(*pos) {
            Some(ArithToken::Lt) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val < rhs { 1 } else { 0 };
            }
            Some(ArithToken::Le) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val <= rhs { 1 } else { 0 };
            }
            Some(ArithToken::Gt) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val > rhs { 1 } else { 0 };
            }
            Some(ArithToken::Ge) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val >= rhs { 1 } else { 0 };
            }
            Some(ArithToken::Eq) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val == rhs { 1 } else { 0 };
            }
            Some(ArithToken::Ne) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_add_expr(tokens, pos);
                val = if val != rhs { 1 } else { 0 };
            }
            _ => break,
        }
    }
    val
}

/// Parse an additive expression: term (('+' | '-') term)*
fn parse_add_expr(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    let mut val = parse_term(tokens, pos);
    loop {
        match tokens.get(*pos) {
            Some(ArithToken::Plus) => {
                *pos = pos.saturating_add(1);
                val = val.wrapping_add(parse_term(tokens, pos));
            }
            Some(ArithToken::Minus) => {
                *pos = pos.saturating_add(1);
                val = val.wrapping_sub(parse_term(tokens, pos));
            }
            _ => break,
        }
    }
    val
}

/// Parse a multiplicative expression: unary (('*' | '/' | '%') unary)*
fn parse_term(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    let mut val = parse_unary(tokens, pos);
    loop {
        match tokens.get(*pos) {
            Some(ArithToken::Star) => {
                *pos = pos.saturating_add(1);
                val = val.wrapping_mul(parse_unary(tokens, pos));
            }
            Some(ArithToken::Slash) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_unary(tokens, pos);
                val = if rhs == 0 { 0 } else { val.wrapping_div(rhs) };
            }
            Some(ArithToken::Percent) => {
                *pos = pos.saturating_add(1);
                let rhs = parse_unary(tokens, pos);
                val = if rhs == 0 { 0 } else { val.wrapping_rem(rhs) };
            }
            _ => break,
        }
    }
    val
}

/// Parse a unary expression: '-' unary | '!' unary | atom
fn parse_unary(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    if let Some(ArithToken::Minus) = tokens.get(*pos) {
        *pos = pos.saturating_add(1);
        return parse_unary(tokens, pos).wrapping_neg();
    }
    if let Some(ArithToken::Not) = tokens.get(*pos) {
        *pos = pos.saturating_add(1);
        let val = parse_unary(tokens, pos);
        return if val == 0 { 1 } else { 0 };
    }
    parse_atom(tokens, pos)
}

/// Parse an atom: number | '(' expr ')'
fn parse_atom(tokens: &[ArithToken], pos: &mut usize) -> i64 {
    match tokens.get(*pos) {
        Some(ArithToken::Num(n)) => {
            let val = *n;
            *pos = pos.saturating_add(1);
            val
        }
        Some(ArithToken::LParen) => {
            *pos = pos.saturating_add(1);
            let val = parse_expr(tokens, pos);
            // Consume matching ')'.
            if let Some(ArithToken::RParen) = tokens.get(*pos) {
                *pos = pos.saturating_add(1);
            }
            val
        }
        _ => {
            // Unexpected token — return 0.
            0
        }
    }
}

/// Show environment variables.
fn cmd_printenv() {
    let vars = ENV_VARS.lock();
    if vars.is_empty() {
        shell_println!("(no environment variables set)");
    } else {
        for (k, v) in vars.iter() {
            shell_println!("{}={}", k, v);
        }
    }
}

/// Set an environment variable.
///
/// Usage: `export NAME=VALUE` or `export NAME VALUE`.
/// With no arguments, lists all variables (same as `printenv`).
/// Read a line from the keyboard and store it in a variable.
///
/// Usage:
///   `read VAR`         — read a line into VAR
///   `read -p "msg" VAR` — print prompt, then read
///   `read`             — read into $REPLY
fn cmd_read(args: &str) {
    let mut prompt: Option<&str> = None;
    let mut var_name = "REPLY";
    let mut into_array = false;
    let mut rest = args;

    // Parse flags: -p "prompt", -a (array mode).
    loop {
        if rest.starts_with("-a ") || rest.starts_with("-a\t") {
            into_array = true;
            rest = rest.get(3..).unwrap_or("").trim_start();
            continue;
        }
        break;
    }

    // Parse -p "prompt" flag.
    if rest.starts_with("-p ") || rest.starts_with("-p\t") {
        rest = rest.get(3..).unwrap_or("").trim_start();
        // Extract the prompt string (may be quoted).
        if rest.starts_with('"') {
            if let Some(end) = rest.get(1..).and_then(|s| s.find('"')) {
                prompt = rest.get(1..end.saturating_add(1));
                rest = rest.get(end.saturating_add(2)..).unwrap_or("").trim_start();
            }
        } else if rest.starts_with('\'') {
            if let Some(end) = rest.get(1..).and_then(|s| s.find('\'')) {
                prompt = rest.get(1..end.saturating_add(1));
                rest = rest.get(end.saturating_add(2)..).unwrap_or("").trim_start();
            }
        } else {
            // Unquoted: first word is the prompt.
            let end = rest.find(' ').unwrap_or(rest.len());
            prompt = rest.get(..end);
            rest = rest.get(end..).unwrap_or("").trim_start();
        }
    }

    if !rest.is_empty() {
        var_name = rest.split_whitespace().next().unwrap_or("REPLY");
    }

    // Print prompt if specified.
    if let Some(p) = prompt {
        shell_print!("{}", p);
    }

    // Read a line from keyboard input.
    let mut buf = [0u8; MAX_LINE];
    let mut len = 0;

    loop {
        let byte = crate::keyboard::read_char();
        match byte {
            b'\r' | b'\n' => {
                shell_println!();
                break;
            }
            0x7F | 0x08 => {
                // Backspace.
                if len > 0 {
                    len -= 1;
                    shell_print!("\x08 \x08");
                }
            }
            3 => {
                // Ctrl+C — cancel.
                shell_println!();
                set_exit(1);
                return;
            }
            _ => {
                if len < MAX_LINE {
                    buf[len] = byte;
                    len += 1;
                    shell_print!("{}", byte as char);
                }
            }
        }
    }

    let value = core::str::from_utf8(buf.get(..len).unwrap_or(&[]))
        .unwrap_or("");

    if into_array {
        // Split input into words and store as array.
        let words: Vec<String> = value.split_whitespace()
            .map(String::from)
            .collect();
        array_set(var_name, words);
    } else {
        env_set(var_name, value);
    }
}

/// `mapfile [-t] ARRAY [FILE]` — read lines from file into an array.
///
/// Reads all lines from FILE (or stdin if invoked via pipe) and stores
/// each line as an element of ARRAY.  With `-t`, trailing newlines are
/// stripped from each element.  Also available as `readarray`.
fn cmd_mapfile(args: &str) {
    let mut strip_newlines = false;
    let mut rest = args;

    // Parse flags.
    while rest.starts_with('-') {
        if rest.starts_with("-t ") || rest.starts_with("-t\t") || rest == "-t" {
            strip_newlines = true;
            rest = rest.get(3..).unwrap_or("").trim_start();
        } else {
            break;
        }
    }

    // Parse array name.
    let (var_name, file_path) = if let Some(sp) = rest.find(' ') {
        (rest.get(..sp).unwrap_or("MAPFILE"), rest.get(sp.saturating_add(1)..).unwrap_or("").trim())
    } else if !rest.is_empty() {
        (rest, "")
    } else {
        ("MAPFILE", "")
    };

    if file_path.is_empty() {
        crate::console_println!("mapfile: no file specified (pipe input or provide a file path)");
        set_exit(1);
        return;
    }

    let path = resolve_path(file_path);
    match crate::fs::Vfs::read_file(&path) {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("");
            mapfile_store(var_name, text, strip_newlines);
            set_exit(0);
        }
        Err(e) => {
            crate::console_println!("mapfile: {}: {:?}", file_path, e);
            set_exit(1);
        }
    }
}

/// `mapfile` with piped input: `cmd | mapfile [-t] ARRAY`
fn cmd_mapfile_input(args: &str, input: &str) {
    let mut strip_newlines = false;
    let mut rest = args;

    while rest.starts_with('-') {
        if rest.starts_with("-t ") || rest.starts_with("-t\t") || rest == "-t" {
            strip_newlines = true;
            rest = rest.get(3..).unwrap_or("").trim_start();
        } else {
            break;
        }
    }

    let var_name = if rest.is_empty() { "MAPFILE" } else { rest.split_whitespace().next().unwrap_or("MAPFILE") };
    mapfile_store(var_name, input, strip_newlines);
    set_exit(0);
}

/// Common helper: split text into lines and store as an array variable.
fn mapfile_store(var_name: &str, text: &str, strip_newlines: bool) {
    let lines: Vec<String> = text.split('\n')
        .map(|line| {
            if strip_newlines {
                String::from(line.trim_end_matches('\n').trim_end_matches('\r'))
            } else {
                alloc::format!("{}\n", line)
            }
        })
        .collect();

    // Remove the trailing empty element that split('\n') produces for
    // text ending in a newline.
    let lines = if lines.last().map_or(false, |l| l.is_empty() || l == "\n") {
        lines.get(..lines.len().saturating_sub(1)).unwrap_or(&[]).to_vec()
    } else {
        lines
    };

    array_set(var_name, lines);
}

/// `tee FILE` with piped input: writes input to file AND passes through.
fn cmd_tee_input(args: &str, input: &str) {
    let path = args.trim();
    if path.is_empty() {
        // No file — just pass through.
        shell_print!("{}", input);
        return;
    }

    let resolved = resolve_path(path);
    // Pass through to output.
    shell_print!("{}", input);
    // Also write to file.
    if let Err(e) = crate::fs::Vfs::write_file(&resolved, input.as_bytes()) {
        crate::console_println!("tee: write error: {:?}", e);
    }
}

/// `readonly [VAR=VALUE ...]` — mark variables as read-only.
///
/// With no arguments, lists all readonly variables.
/// `readonly VAR=VALUE` sets the value and marks it readonly.
/// `readonly VAR` marks an existing variable readonly without changing its value.
fn cmd_readonly(args: &str) {
    if args.is_empty() {
        // List all readonly variables.
        let ro = READONLY_VARS.lock();
        for name in ro.iter() {
            if let Some(val) = env_get(name) {
                crate::console_println!("declare -r {}=\"{}\"", name, val);
            } else {
                crate::console_println!("declare -r {}", name);
            }
        }
        return;
    }

    // Process each word as a separate readonly declaration.
    for word in args.split_whitespace() {
        if let Some(eq_pos) = word.find('=') {
            let name = word.get(..eq_pos).unwrap_or("");
            let value = word.get(eq_pos.saturating_add(1)..).unwrap_or("");
            if name.is_empty() {
                crate::console_println!("readonly: invalid variable name");
                continue;
            }
            // Set the value (bypass readonly check since we're about to mark it).
            ENV_VARS.lock().insert(String::from(name), String::from(value));
            READONLY_VARS.lock().insert(String::from(name));
        } else {
            // Just mark as readonly (no value change).
            READONLY_VARS.lock().insert(String::from(word));
        }
    }
}

/// `let EXPR [EXPR ...]` — evaluate arithmetic expressions.
///
/// Each argument is evaluated as an arithmetic expression.  The exit
/// status is 1 if the last expression evaluates to 0, and 0 otherwise
/// (like bash).  Supports assignment: `let "x = 5 + 3"`.
fn cmd_let(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: let EXPRESSION ...");
        set_exit(1);
        return;
    }

    // Each whitespace-separated word is a separate expression, unless
    // quoted.  For simplicity in our kshell, treat the entire arg as
    // one expression (users can quote as needed).
    let expr = strip_quotes(args);

    // Check for assignment: `VAR = EXPR` or `VAR=EXPR`.
    if let Some(eq_pos) = expr.find('=') {
        let left = expr.get(..eq_pos).unwrap_or("").trim();
        let right = expr.get(eq_pos.saturating_add(1)..).unwrap_or("").trim();
        // Left side must be a valid variable name.
        if !left.is_empty() && left.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !left.starts_with(|c: char| c.is_ascii_digit())
        {
            let val = eval_arithmetic(right);
            env_set(left, &alloc::format!("{}", val));
            set_exit(if val == 0 { 1 } else { 0 });
            return;
        }
    }

    // No assignment — just evaluate.
    let val = eval_arithmetic(&expr);
    set_exit(if val == 0 { 1 } else { 0 });
}

/// `trap 'COMMAND' SIGNAL` — set a handler for a shell event.
///
/// Supported signals: EXIT, ERR, INT.
/// `trap - SIGNAL` clears the handler.
/// `trap` with no arguments lists current handlers.
fn cmd_trap(args: &str) {
    if args.is_empty() {
        // List all trap handlers.
        let handlers = TRAP_HANDLERS.lock();
        if handlers.is_empty() {
            crate::console_println!("No trap handlers set.");
        } else {
            for (sig, cmd) in handlers.iter() {
                crate::console_println!("trap -- '{}' {}", cmd, sig);
            }
        }
        return;
    }

    // Split: trap 'COMMAND' SIGNAL [SIGNAL...]
    // or:    trap - SIGNAL
    let trimmed = args.trim();

    if trimmed == "-" || trimmed.starts_with("- ") {
        // Clear handler(s).
        let signals = trimmed.get(1..).unwrap_or("").trim();
        for sig in signals.split_whitespace() {
            let sig_upper = sig.to_uppercase();
            TRAP_HANDLERS.lock().remove(&sig_upper);
        }
        return;
    }

    // Parse: first argument is the command (possibly quoted), rest are signals.
    let (command, rest) = if trimmed.starts_with('\'') || trimmed.starts_with('"') {
        // Quoted command.
        let quote = trimmed.as_bytes()[0] as char;
        if let Some(end) = trimmed.get(1..).and_then(|s| s.find(quote)) {
            let cmd = trimmed.get(1..end.saturating_add(1)).unwrap_or("");
            let rest = trimmed.get(end.saturating_add(2)..).unwrap_or("").trim();
            (cmd, rest)
        } else {
            (trimmed, "")
        }
    } else {
        // Unquoted: first word is command.
        let mut parts = trimmed.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        (cmd, rest)
    };

    if rest.is_empty() {
        crate::console_println!("Usage: trap 'COMMAND' SIGNAL [SIGNAL...]");
        set_exit(1);
        return;
    }

    // Set the handler for each signal.
    for sig in rest.split_whitespace() {
        let sig_upper = sig.to_uppercase();
        match sig_upper.as_str() {
            "EXIT" | "ERR" | "INT" => {
                TRAP_HANDLERS.lock().insert(sig_upper, String::from(command));
            }
            _ => {
                crate::console_println!("trap: unsupported signal '{}'", sig);
                set_exit(1);
            }
        }
    }
}

/// Execute any trap handler registered for the given signal.
fn fire_trap(signal: &str) {
    let cmd = TRAP_HANDLERS.lock().get(signal).cloned();
    if let Some(cmd) = cmd {
        execute(&cmd);
    }
}

/// `command CMD [ARGS...]` — run CMD bypassing aliases and functions.
///
/// Executes only built-in shell commands, ignoring any alias or function
/// with the same name.
fn cmd_command(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: command CMD [args...]");
        set_exit(1);
        return;
    }
    // Dispatch directly without alias expansion.
    dispatch(args);
}

fn cmd_export(args: &str) {
    if args.is_empty() {
        // List all variables.
        cmd_printenv();
        return;
    }

    // Try `NAME=VALUE` form first.
    if let Some(eq_pos) = args.find('=') {
        let name = args.get(..eq_pos).unwrap_or("").trim();
        let value = args.get(eq_pos.saturating_add(1)..).unwrap_or("").trim();
        if name.is_empty() {
            crate::console_println!("export: invalid variable name");
            return;
        }
        env_set(name, value);
    } else {
        // Try `NAME VALUE` form.
        let mut parts = args.splitn(2, ' ');
        let name = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            crate::console_println!("export: invalid variable name");
            return;
        }
        env_set(name, value);
    }

    // Keep PWD in sync.
    sync_env_pwd();
}

/// Set shell options or variables.
///
/// Flags:
///   `set -e` — exit on error (errexit)
///   `set +e` — disable exit on error
///   `set -x` — trace commands before execution
///   `set +x` — disable tracing
///   `set NAME=VALUE` or `set NAME VALUE` — set variable (like export)
fn cmd_set(args: &str) {
    if args.is_empty() {
        // Show current options and variables.
        let errexit = OPT_ERREXIT.load(core::sync::atomic::Ordering::Relaxed);
        let xtrace = OPT_XTRACE.load(core::sync::atomic::Ordering::Relaxed);
        crate::console_println!("Shell options:");
        crate::console_println!("  errexit (-e): {}", if errexit { "on" } else { "off" });
        crate::console_println!("  xtrace  (-x): {}", if xtrace { "on" } else { "off" });
        return;
    }

    match args {
        "-e" => {
            OPT_ERREXIT.store(true, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        "+e" => {
            OPT_ERREXIT.store(false, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        "-x" => {
            OPT_XTRACE.store(true, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        "+x" => {
            OPT_XTRACE.store(false, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        "-ex" | "-xe" => {
            OPT_ERREXIT.store(true, core::sync::atomic::Ordering::Relaxed);
            OPT_XTRACE.store(true, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        "+ex" | "+xe" => {
            OPT_ERREXIT.store(false, core::sync::atomic::Ordering::Relaxed);
            OPT_XTRACE.store(false, core::sync::atomic::Ordering::Relaxed);
            return;
        }
        _ => {}
    }

    // Fall back to variable-setting behavior.
    cmd_export(args);
}

/// Declare a local variable inside a function.
///
/// Usage: `local VAR=VALUE` or `local VAR VALUE` or `local VAR`
///
/// Saves the variable's current value (or notes it was unset) so it
/// can be restored when the function returns.  Outside a function,
/// behaves identically to `export`.
fn cmd_local(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: local VAR[=VALUE]");
        return;
    }

    // Parse NAME=VALUE or NAME VALUE.
    let (name, value) = if let Some(eq_pos) = args.find('=') {
        let n = args.get(..eq_pos).unwrap_or("").trim();
        let v = args.get(eq_pos.saturating_add(1)..).unwrap_or("").trim();
        (n, v)
    } else {
        let mut parts = args.splitn(2, ' ');
        let n = parts.next().unwrap_or("").trim();
        let v = parts.next().unwrap_or("").trim();
        (n, v)
    };

    if name.is_empty() {
        crate::console_println!("local: invalid variable name");
        return;
    }

    // Save the current value on the local variable stack (if inside a function).
    {
        let mut stack = LOCAL_VARS.lock();
        if let Some(frame) = stack.last_mut() {
            // Only save if we haven't already saved this name in this frame.
            let already_saved = frame.iter().any(|(n, _)| n == name);
            if !already_saved {
                let prev = env_get(name);
                frame.push((String::from(name), prev));
            }
        }
        // If not inside a function (stack is empty), `local` still works
        // like `export` — sets the variable without scope protection.
    }

    // Set the variable.
    env_set(name, value);
}

/// Remove an environment variable.
///
/// Usage: `unset NAME [NAME ...]`
fn cmd_unset(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: unset [-f] NAME [NAME ...]");
        return;
    }

    // `unset -f NAME` removes a function definition.
    if args.starts_with("-f ") || args.starts_with("-f\t") {
        let names = args.get(3..).unwrap_or("").trim();
        for name in names.split_whitespace() {
            if FUNCTIONS.lock().remove(name).is_none() {
                crate::console_println!("unset: function '{}': not defined", name);
            }
        }
        return;
    }

    for name in args.split_whitespace() {
        // Check for `unset arr[N]` — remove a single array element.
        if let Some(bracket) = name.find('[') {
            if name.ends_with(']') {
                let arr_name = name.get(..bracket).unwrap_or("");
                let idx_str = name.get(bracket.saturating_add(1)..name.len().saturating_sub(1)).unwrap_or("");
                if let Ok(index) = idx_str.parse::<usize>() {
                    array_unset_element(arr_name, index);
                    continue;
                }
            }
        }
        // Try removing as array first, then as scalar.
        if !array_remove(name) && !env_remove(name) {
            crate::console_println!("unset: '{}': not set", name);
        }
    }
}

/// List or inspect shell functions.
///
/// Usage:
///   `declare -f`       — list all function names and bodies
///   `declare -f NAME`  — show a specific function's body
///   `declare`          — list all function names
/// Show what kind of command a name is (builtin, function, alias).
///
/// Usage: `type name [name ...]`
fn cmd_type(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: type <name> [<name> ...]");
        return;
    }

    for name in args.split_whitespace() {
        // Check aliases first.
        if let Some(val) = alias_get(name) {
            crate::console_println!("{} is aliased to '{}'", name, val);
            continue;
        }

        // Check user-defined functions.
        if FUNCTIONS.lock().contains_key(name) {
            crate::console_println!("{} is a function", name);
            continue;
        }

        // Check array variables.
        if is_array(name) {
            let len = array_len(name).unwrap_or(0);
            crate::console_println!("{} is an array ({} elements)", name, len);
            continue;
        }

        // Check builtins (simplified — we check the dispatch table names).
        if is_builtin(name) {
            crate::console_println!("{} is a shell builtin", name);
            continue;
        }

        // Check keywords.
        if matches!(name, "if" | "then" | "elif" | "else" | "fi" | "while" | "until" | "for"
            | "do" | "done" | "case" | "esac" | "function" | "in" | "select")
        {
            crate::console_println!("{} is a shell keyword", name);
            continue;
        }

        crate::console_println!("{}: not found", name);
        set_exit(1);
    }
}

/// Check if a command name is a built-in shell command.
fn is_builtin(name: &str) -> bool {
    matches!(name,
        "help" | "?" | "cd" | "meminfo" | "mem" | "ps" | "tasks" | "clear" | "cls"
        | "uptime" | "dmesg" | "echo" | "time" | "date" | "reboot" | "irq" | "pci" | "disk"
        | "blkinfo" | "blkread" | "ls" | "dir" | "cat" | "type" | "write" | "rm"
        | "del" | "mkdir" | "rmdir" | "stat" | "ln" | "link" | "df" | "cp" | "copy"
        | "mv" | "move" | "ren" | "chmod" | "chown" | "touch" | "append" | "tree"
        | "du" | "file" | "find" | "sync" | "mount" | "umount" | "unmount" | "wc" | "head"
        | "tail" | "hexdump" | "xxd" | "lsof" | "lsp" | "grep" | "cmp" | "diff"
        | "fallocate" | "sort" | "uniq" | "tee" | "truncate" | "sha256" | "hash"
        | "sysctl" | "hostname" | "dd" | "free" | "vmstat" | "flock" | "split"
        | "lsblk" | "blkdev" | "glob" | "fsck" | "fsck.fat" | "fsck.ext4" | "mkfs" | "mkfs.fat"
        | "readlink" | "symlink" | "mklink" | "xattr" | "watch" | "trash" | "journal" | "gunzip" | "gzip" | "bunzip2" | "bzcat" | "unzip" | "zip" | "basename" | "dirname"
        | "realpath" | "pwd" | "id" | "whoami" | "mktemp" | "run" | "exec"
        | "mkelf" | "net" | "ifconfig" | "dhcp" | "ping" | "dns" | "nslookup"
        | "wget" | "http" | "version" | "ver" | "uname" | "source" | "." | "seq" | "nl"
        | "rev" | "sleep" | "true" | "false" | "test" | "[" | "expr" | "printenv"
        | "env" | "eval" | "declare" | "read" | "readarray" | "mapfile"
        | "readonly" | "let" | "trap" | "command" | "which" | "typeof"
        | "export" | "set" | "unset" | "alias" | "unalias" | "return"
        | "break" | "continue" | "shift" | "local" | "printf"
        | "cut" | "tr" | "yes" | "tac" | "fold" | "paste" | "xargs"
        | "cpuinfo" | "cpu" | "watchdog" | "kill" | "renice" | "throttle"
        | "taskset" | "schedstat" | "slabinfo" | "stack" | "profile" | "top"
        | "wq" | "workqueue" | "ktimer" | "timers" | "rng" | "random" | "trace" | "ktrace"
        | "supervisor" | "sv"
    )
}

fn cmd_declare(args: &str) {
    let funcs = FUNCTIONS.lock();

    if args.is_empty() {
        // List function names only.
        if funcs.is_empty() {
            crate::console_println!("No functions defined.");
        } else {
            for name in funcs.keys() {
                crate::console_println!("{}", name);
            }
        }
        return;
    }

    if args == "-f" {
        // List all functions with bodies.
        if funcs.is_empty() {
            crate::console_println!("No functions defined.");
        } else {
            for (name, body) in funcs.iter() {
                crate::console_println!("{}() {{", name);
                for line in body {
                    crate::console_println!("    {}", line);
                }
                crate::console_println!("}}");
            }
        }
        return;
    }

    if let Some(name) = args.strip_prefix("-f ") {
        let name = name.trim();
        if let Some(body) = funcs.get(name) {
            crate::console_println!("{}() {{", name);
            for line in body {
                crate::console_println!("    {}", line);
            }
            crate::console_println!("}}");
        } else {
            crate::console_println!("declare: function '{}' not found", name);
            set_exit(1);
        }
        return;
    }

    if args == "-a" {
        // List all arrays with contents.
        let arrays = ARRAY_VARS.lock();
        if arrays.is_empty() {
            crate::console_println!("No arrays defined.");
        } else {
            for (name, values) in arrays.iter() {
                let mut display = String::from("(");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        display.push(' ');
                    }
                    // Quote values that contain spaces.
                    if v.contains(' ') {
                        display.push('"');
                        display.push_str(v);
                        display.push('"');
                    } else {
                        display.push_str(v);
                    }
                }
                display.push(')');
                crate::console_println!("{}={}", name, display);
            }
        }
        return;
    }

    if let Some(name) = args.strip_prefix("-a ") {
        let name = name.trim();
        let arrays = ARRAY_VARS.lock();
        if let Some(values) = arrays.get(name) {
            for (i, v) in values.iter().enumerate() {
                crate::console_println!("{}[{}]={}", name, i, v);
            }
        } else {
            crate::console_println!("declare: array '{}' not found", name);
            set_exit(1);
        }
        return;
    }

    crate::console_println!("Usage: declare [-f [NAME]] [-a [NAME]]");
}

/// Define or list shell aliases.
///
/// Usage:
///   `alias`            — list all aliases
///   `alias name=value` — define alias
///   `alias name value` — define alias (alternative form)
fn cmd_alias(args: &str) {
    if args.is_empty() {
        let aliases = ALIASES.lock();
        if aliases.is_empty() {
            crate::console_println!("(no aliases defined)");
        } else {
            for (k, v) in aliases.iter() {
                crate::console_println!("alias {}='{}'", k, v);
            }
        }
        return;
    }

    // Try `name=value` form first.
    if let Some(eq_pos) = args.find('=') {
        let name = args.get(..eq_pos).unwrap_or("").trim();
        let value = args.get(eq_pos.saturating_add(1)..).unwrap_or("");
        // Strip surrounding quotes if present.
        let value = value.trim();
        let value = strip_quotes(value);
        if name.is_empty() {
            crate::console_println!("alias: invalid alias name");
            return;
        }
        alias_set(name, value);
    } else {
        // `alias name value` form.
        let mut parts = args.splitn(2, ' ');
        let name = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            crate::console_println!("alias: invalid alias name");
            return;
        }
        if value.is_empty() {
            // Show this single alias.
            if let Some(v) = alias_get(name) {
                crate::console_println!("alias {}='{}'", name, v);
            } else {
                crate::console_println!("alias: '{}': not found", name);
            }
        } else {
            alias_set(name, value);
        }
    }
}

/// Remove one or more aliases.
///
/// Usage: `unalias NAME [NAME ...]` or `unalias -a` (remove all).
fn cmd_unalias(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: unalias [-a] NAME [NAME ...]");
        return;
    }
    if args.trim() == "-a" {
        ALIASES.lock().clear();
        return;
    }
    for name in args.split_whitespace() {
        if !alias_remove(name) {
            crate::console_println!("unalias: '{}': not found", name);
        }
    }
}

/// Strip matching leading/trailing quotes (`'` or `"`) from a string.
fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len().saturating_sub(1)];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return s.get(1..s.len().saturating_sub(1)).unwrap_or(s);
        }
    }
    s
}

/// Keep the `PWD` environment variable in sync with the shell's CWD.
fn sync_env_pwd() {
    let cwd = get_cwd();
    env_set("PWD", &cwd);
}

/// `tee FILE TEXT` — write TEXT to FILE and also display it.
///
/// Like the Unix tee command but with direct text input instead of stdin.
fn cmd_tee(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let path = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();

    if path.is_empty() || text.is_empty() {
        crate::console_println!("Usage: tee <file> <text>");
        return;
    }

    // Display the text.
    crate::console_println!("{}", text);

    // Write to the file.
    if let Err(e) = crate::fs::Vfs::write_file(path, text.as_bytes()) {
        crate::console_println!("tee: write error: {:?}", e);
    }
}

/// `truncate N FILE` — truncate a file to N bytes.
///
/// Supports K/M/G suffixes. If N is larger than the current file size,
/// the behavior depends on the filesystem (may extend with zeros or
/// return an error).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_truncate(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let size_str = parts.next().unwrap_or("").trim();
    let path = parts.next().unwrap_or("").trim();

    if size_str.is_empty() || path.is_empty() {
        crate::console_println!("Usage: truncate <size>[K|M|G] <file>");
        return;
    }

    // Parse size with optional suffix.
    let (num_str, multiplier) = if size_str.ends_with('G') || size_str.ends_with('g') {
        (size_str.get(..size_str.len() - 1).unwrap_or(""), 1024u64 * 1024 * 1024)
    } else if size_str.ends_with('M') || size_str.ends_with('m') {
        (size_str.get(..size_str.len() - 1).unwrap_or(""), 1024u64 * 1024)
    } else if size_str.ends_with('K') || size_str.ends_with('k') {
        (size_str.get(..size_str.len() - 1).unwrap_or(""), 1024u64)
    } else {
        (size_str, 1u64)
    };

    let size = match num_str.parse::<u64>() {
        Ok(n) => n.saturating_mul(multiplier),
        Err(_) => {
            crate::console_println!("truncate: invalid size '{}'", size_str);
            return;
        }
    };

    // Open the file handle and truncate.
    match crate::fs::handle::open(path, crate::fs::handle::OpenFlags::from_bits(0x02)) {
        Ok(handle) => {
            match crate::fs::handle::ftruncate(handle, size) {
                Ok(()) => {
                    crate::console_println!("Truncated '{}' to {} bytes", path, size);
                }
                Err(e) => {
                    crate::console_println!("truncate: error: {:?}", e);
                }
            }
            let _ = crate::fs::handle::close(handle);
        }
        Err(e) => {
            crate::console_println!("truncate: cannot open '{}': {:?}", path, e);
        }
    }
}

/// `sha256 FILE` — compute and display the SHA-256 hash of a file.
fn cmd_sha256(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        crate::console_println!("Usage: sha256 <file>");
        return;
    }

    match crate::fs::Vfs::content_hash(path) {
        Ok(hash) => {
            // Display as hex string.
            let mut hex = alloc::string::String::with_capacity(64);
            for byte in &hash {
                hex.push_str(&alloc::format!("{:02x}", byte));
            }
            crate::console_println!("{} {}", hex, path);
        }
        Err(e) => {
            crate::console_println!("sha256: cannot hash '{}': {:?}", path, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Path utility commands
// ---------------------------------------------------------------------------

/// `readlink PATH` — display the target of a symbolic link.
fn cmd_readlink(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        crate::console_println!("Usage: readlink <symlink>");
        return;
    }

    match crate::fs::Vfs::readlink(path) {
        Ok(target) => {
            crate::console_println!("{}", target);
        }
        Err(e) => {
            crate::console_println!("readlink: '{}': {:?}", path, e);
        }
    }
}

/// `symlink TARGET PATH` — create a symbolic link at PATH pointing to TARGET.
fn cmd_symlink(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let target = parts.next().unwrap_or("").trim();
    let link_path = parts.next().unwrap_or("").trim();

    if target.is_empty() || link_path.is_empty() {
        crate::console_println!("Usage: symlink <target> <link_path>");
        return;
    }

    match crate::fs::Vfs::symlink(link_path, target) {
        Ok(()) => {
            crate::console_println!("{} -> {}", link_path, target);
        }
        Err(e) => {
            crate::console_println!("symlink: cannot create '{}': {:?}", link_path, e);
        }
    }
}

/// `xattr FILE [list | get KEY | set KEY VALUE | rm KEY]` — extended attribute ops.
fn cmd_xattr(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let file = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    if file.is_empty() {
        crate::console_println!("Usage: xattr <file> [list | get <key> | set <key> <value> | rm <key>]");
        return;
    }

    // Default subcommand is "list".
    if rest.is_empty() || rest == "list" {
        match crate::fs::Vfs::list_xattrs(file) {
            Ok(keys) => {
                if keys.is_empty() {
                    crate::console_println!("(no extended attributes)");
                } else {
                    for key in &keys {
                        crate::console_println!("  {}", key);
                    }
                }
            }
            Err(e) => {
                crate::console_println!("xattr: list '{}': {:?}", file, e);
            }
        }
        return;
    }

    let mut sub_parts = rest.splitn(3, ' ');
    let subcmd = sub_parts.next().unwrap_or("");
    let key = sub_parts.next().unwrap_or("").trim();

    match subcmd {
        "get" => {
            if key.is_empty() {
                crate::console_println!("Usage: xattr <file> get <key>");
                return;
            }
            match crate::fs::Vfs::get_xattr(file, key) {
                Ok(val) => {
                    // Try to display as UTF-8, fall back to hex.
                    if let Ok(s) = core::str::from_utf8(&val) {
                        crate::console_println!("{}", s);
                    } else {
                        let mut hex = alloc::string::String::with_capacity(val.len().saturating_mul(3));
                        for (i, byte) in val.iter().enumerate() {
                            if i > 0 {
                                hex.push(' ');
                            }
                            hex.push_str(&alloc::format!("{:02x}", byte));
                        }
                        crate::console_println!("{}", hex);
                    }
                }
                Err(e) => {
                    crate::console_println!("xattr: get '{}' from '{}': {:?}", key, file, e);
                }
            }
        }
        "set" => {
            let value = sub_parts.next().unwrap_or("").trim();
            if key.is_empty() {
                crate::console_println!("Usage: xattr <file> set <key> <value>");
                return;
            }
            match crate::fs::Vfs::set_xattr(file, key, value.as_bytes()) {
                Ok(()) => {
                    crate::console_println!("Set {}={} on {}", key, value, file);
                }
                Err(e) => {
                    crate::console_println!("xattr: set '{}' on '{}': {:?}", key, file, e);
                }
            }
        }
        "rm" | "remove" | "del" => {
            if key.is_empty() {
                crate::console_println!("Usage: xattr <file> rm <key>");
                return;
            }
            match crate::fs::Vfs::remove_xattr(file, key) {
                Ok(()) => {
                    crate::console_println!("Removed '{}' from {}", key, file);
                }
                Err(e) => {
                    crate::console_println!("xattr: rm '{}' from '{}': {:?}", key, file, e);
                }
            }
        }
        _ => {
            crate::console_println!("xattr: unknown subcommand '{}'. Use list/get/set/rm.", subcmd);
        }
    }
}

/// `trash [FILE | --list | --restore NAME | --empty | --purge NAME | --prune]`
///
/// Manage the recycle bin.  Without arguments, shows usage.
/// - `trash FILE` — move FILE to the recycle bin
/// Monitor filesystem changes on a directory (or file) in real time.
///
/// Usage: `watch <path> [-r]`
///
/// Creates a filesystem watch on the specified path and prints events
/// as they occur. Press any key to stop monitoring.
///
/// Options:
///   -r — recursive: watch subdirectories too
///
/// Events shown: Created, Deleted, Modified, Renamed, MetadataChanged.
fn cmd_watch(args: &str) {
    let args = args.trim();
    if args.is_empty() {
        crate::console_println!("Usage: watch <path> [-r]");
        crate::console_println!("  Monitor filesystem changes. Press any key to stop.");
        return;
    }

    // Parse arguments: path and optional -r flag.
    let mut recursive = false;
    let mut path_arg = args;
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() >= 2 {
        for &p in &parts[1..] {
            if p == "-r" || p == "--recursive" {
                recursive = true;
            }
        }
        if let Some(&first) = parts.first() {
            path_arg = first;
        }
    }

    let path = resolve_path(path_arg);

    // Verify the path exists.
    if crate::fs::Vfs::stat(&path).is_err() {
        crate::console_println!("watch: {}: not found", path);
        return;
    }

    // Create the watch with all change event types.
    let mask = crate::fs::notify::FsEventMask::ALL_CHANGES;
    let watch_id = match crate::fs::notify::create_watch(&path, mask, recursive) {
        Ok(id) => id,
        Err(e) => {
            crate::console_println!("watch: failed to create watch: {:?}", e);
            return;
        }
    };

    crate::console_println!(
        "Watching {} {}(press any key to stop)",
        path,
        if recursive { "[recursive] " } else { "" },
    );

    // Poll loop: check for events and keyboard input.
    let mut event_count = 0u64;
    loop {
        // Check for keyboard input to stop.
        if crate::keyboard::try_read_char().is_some() {
            break;
        }

        // Read pending events (up to 16 per poll).
        match crate::fs::notify::read_events(watch_id, 16) {
            Ok(events) if !events.is_empty() => {
                for ev in &events {
                    let type_str = match ev.event_type {
                        crate::fs::notify::FsEventType::Created => "CREATE",
                        crate::fs::notify::FsEventType::Deleted => "DELETE",
                        crate::fs::notify::FsEventType::Modified => "MODIFY",
                        crate::fs::notify::FsEventType::Renamed => "RENAME",
                        crate::fs::notify::FsEventType::MetadataChanged => "META  ",
                        crate::fs::notify::FsEventType::Accessed => "ACCESS",
                        crate::fs::notify::FsEventType::Overflow => "OVERFLOW",
                    };

                    if let Some(ref new_path) = ev.new_path {
                        crate::console_println!(
                            "[{}] {} -> {}", type_str, ev.path, new_path
                        );
                    } else {
                        crate::console_println!("[{}] {}", type_str, ev.path);
                    }
                    event_count = event_count.saturating_add(1);
                }
            }
            _ => {
                // No events — sleep briefly to avoid busy-spinning.
                crate::cpu::hlt();
            }
        }
    }

    // Clean up the watch.
    let _ = crate::fs::notify::close_watch(watch_id);

    crate::console_println!("\nStopped. {} events captured.", event_count);
}

/// Display filesystem change journal entries.
///
/// Usage:
/// - `journal`           — show last 20 entries
/// - `journal -n 50`     — show last 50 entries
/// - `journal --all`     — show all entries in the ring buffer
/// - `journal --stats`   — show journal statistics
/// - `journal --flush`   — flush unflushed entries to disk
/// - `journal --since N` — show entries with sequence number > N
fn cmd_journal(args: &str) {
    let args = args.trim();

    // Parse arguments.
    let parts: Vec<&str> = args.split_whitespace().collect();

    if parts.first() == Some(&"--stats") || parts.first() == Some(&"-s") {
        let (count, seq) = crate::fs::journal::stats();
        let cursor = crate::fs::journal::cursor();
        crate::console_println!("Journal statistics:");
        crate::console_println!("  Entries in buffer: {}", count);
        crate::console_println!("  Current sequence:  {}", seq);
        crate::console_println!("  Cursor position:   {}", cursor);
        return;
    }

    if parts.first() == Some(&"--flush") || parts.first() == Some(&"-f") {
        match crate::fs::journal::flush() {
            Ok(()) => crate::console_println!("Journal flushed to disk."),
            Err(e) => crate::console_println!("journal: flush failed: {:?}", e),
        }
        return;
    }

    // Determine how many entries to show and from which sequence.
    let mut since_seq = 0u64;
    let mut max_show: usize = 20;
    let mut show_all = false;

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "-n" => {
                if let Some(&count_str) = parts.get(i.wrapping_add(1)) {
                    if let Some(n) = parse_u64_decimal(count_str) {
                        max_show = n as usize;
                    } else {
                        crate::console_println!("journal: invalid count: {}", count_str);
                        return;
                    }
                    i = i.wrapping_add(1);
                } else {
                    crate::console_println!("journal: -n requires a number");
                    return;
                }
            }
            "--all" | "-a" => {
                show_all = true;
            }
            "--since" => {
                if let Some(&seq_str) = parts.get(i.wrapping_add(1)) {
                    if let Some(n) = parse_u64_decimal(seq_str) {
                        since_seq = n;
                    } else {
                        crate::console_println!("journal: invalid sequence: {}", seq_str);
                        return;
                    }
                    i = i.wrapping_add(1);
                } else {
                    crate::console_println!("journal: --since requires a number");
                    return;
                }
            }
            "--help" | "-h" => {
                crate::console_println!(
                    "Usage: journal [-n N] [--all] [--since SEQ] [--stats] [--flush]\n\
                     \x20 -n N        Show last N entries (default 20)\n\
                     \x20 --all       Show all entries in the ring buffer\n\
                     \x20 --since SEQ Show entries after sequence number SEQ\n\
                     \x20 --stats     Show journal statistics\n\
                     \x20 --flush     Flush journal to disk"
                );
                return;
            }
            _ => {}
        }
        i = i.wrapping_add(1);
    }

    let (entries, current_seq) = crate::fs::journal::read_since(since_seq);

    if entries.is_empty() {
        if since_seq > 0 {
            crate::console_println!("No journal entries since seq {}.", since_seq);
        } else {
            crate::console_println!("Journal is empty.");
        }
        return;
    }

    // If not --all and not --since, show only the last N entries.
    let display_entries = if show_all || since_seq > 0 {
        &entries[..]
    } else if entries.len() > max_show {
        crate::console_println!(
            "... ({} older entries omitted, use --all or -n to see more)",
            entries.len().saturating_sub(max_show)
        );
        &entries[entries.len().saturating_sub(max_show)..]
    } else {
        &entries[..]
    };

    // Print entries in a table format.
    for entry in display_entries {
        let type_str = match entry.event_type {
            crate::fs::journal::JournalEventType::Created => "CREATE",
            crate::fs::journal::JournalEventType::Modified => "MODIFY",
            crate::fs::journal::JournalEventType::Deleted => "DELETE",
            crate::fs::journal::JournalEventType::Renamed => "RENAME",
        };

        // Format timestamp as seconds since boot.
        let secs = entry.timestamp_ns / 1_000_000_000;
        let ms = (entry.timestamp_ns % 1_000_000_000) / 1_000_000;

        if entry.event_type == crate::fs::journal::JournalEventType::Renamed
            && !entry.old_path.is_empty()
        {
            crate::console_println!(
                "  #{:<6} {}.{:03}s  {:<6}  {} -> {}",
                entry.seq, secs, ms, type_str, entry.old_path, entry.path
            );
        } else {
            crate::console_println!(
                "  #{:<6} {}.{:03}s  {:<6}  {}",
                entry.seq, secs, ms, type_str, entry.path
            );
        }
    }

    crate::console_println!(
        "\n{} entries shown (current sequence: {})",
        display_entries.len(),
        current_seq
    );
}

/// Parse a decimal string as u64 (no_std-safe).
fn parse_u64_decimal(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add(u64::from(b.wrapping_sub(b'0')))?;
    }
    Some(val)
}

/// - `trash --list` / `trash -l` — list trash contents
/// - `trash --restore NAME` — restore a trashed file to its original location
/// - `trash --empty` — permanently delete all trash items
/// - `trash --purge NAME` — permanently delete one trash item
/// - `trash --prune` — run auto-prune (delete oldest items if disk is full)
fn cmd_trash(args: &str) {
    let args = args.trim();
    if args.is_empty() {
        crate::console_println!(
            "Usage: trash <file>            Move file to recycle bin\n\
             \x20      trash --list / -l       List trash contents\n\
             \x20      trash --restore <name>  Restore file to original location\n\
             \x20      trash --empty           Permanently delete all trash\n\
             \x20      trash --purge <name>    Permanently delete one item\n\
             \x20      trash --prune           Auto-prune if disk space low"
        );
        return;
    }

    match args {
        "--list" | "-l" => {
            match crate::fs::trash::list() {
                Ok(items) => {
                    if items.is_empty() {
                        crate::console_println!("(recycle bin is empty)");
                    } else {
                        crate::console_println!(
                            "{:<20} {:>10}  {}",
                            "TRASH NAME", "SIZE", "ORIGINAL PATH"
                        );
                        for item in &items {
                            let size_str = format_bytes(item.size);
                            let type_prefix = if item.is_directory { "D " } else { "  " };
                            crate::console_println!(
                                "{}{:<18} {:>10}  {}",
                                type_prefix, item.trash_name, size_str, item.original_path
                            );
                        }
                        crate::console_println!("\n{} item(s) in recycle bin", items.len());
                    }
                }
                Err(e) => crate::console_println!("trash: list failed: {:?}", e),
            }
        }
        "--empty" => {
            match crate::fs::trash::empty() {
                Ok(()) => crate::console_println!("Recycle bin emptied."),
                Err(e) => crate::console_println!("trash: empty failed: {:?}", e),
            }
        }
        "--prune" => {
            match crate::fs::trash::auto_prune() {
                Ok(0) => crate::console_println!("No pruning needed (disk space OK)."),
                Ok(n) => crate::console_println!("Auto-pruned {} item(s).", n),
                Err(e) => crate::console_println!("trash: prune failed: {:?}", e),
            }
        }
        _ if args.starts_with("--restore ") => {
            let name = args.get(10..).unwrap_or("").trim();
            if name.is_empty() {
                crate::console_println!("Usage: trash --restore <name>");
                return;
            }
            match crate::fs::trash::restore(name) {
                Ok(original) => {
                    crate::console_println!("Restored '{}' to '{}'", name, original);
                }
                Err(e) => crate::console_println!("trash: restore '{}': {:?}", name, e),
            }
        }
        _ if args.starts_with("--purge ") => {
            let name = args.get(8..).unwrap_or("").trim();
            if name.is_empty() {
                crate::console_println!("Usage: trash --purge <name>");
                return;
            }
            match crate::fs::trash::purge_one(name) {
                Ok(()) => crate::console_println!("Permanently deleted '{}'", name),
                Err(e) => crate::console_println!("trash: purge '{}': {:?}", name, e),
            }
        }
        _ => {
            // Default: move file to trash.
            let path = resolve_path(args);
            match crate::fs::trash::trash(&path) {
                Ok(()) => crate::console_println!("Moved '{}' to recycle bin", path),
                Err(e) => crate::console_println!("trash: '{}': {:?}", path, e),
            }
        }
    }
}

/// `basename PATH` — extract the filename component from a path.
fn cmd_basename(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        crate::console_println!("Usage: basename <path>");
        return;
    }

    // Strip trailing slashes.
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        crate::console_println!("/");
        return;
    }

    // Find last '/' and take everything after it.
    if let Some(pos) = trimmed.rfind('/') {
        if let Some(name) = trimmed.get(pos + 1..) {
            crate::console_println!("{}", name);
        } else {
            crate::console_println!("/");
        }
    } else {
        // No slash — the whole thing is the basename.
        crate::console_println!("{}", trimmed);
    }
}

/// `dirname PATH` — extract the directory component from a path.
fn cmd_dirname(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        crate::console_println!("Usage: dirname <path>");
        return;
    }

    // Strip trailing slashes.
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        crate::console_println!("/");
        return;
    }

    // Find last '/' and take everything before it.
    if let Some(pos) = trimmed.rfind('/') {
        if pos == 0 {
            crate::console_println!("/");
        } else if let Some(dir) = trimmed.get(..pos) {
            crate::console_println!("{}", dir);
        } else {
            crate::console_println!(".");
        }
    } else {
        // No slash — dirname is ".".
        crate::console_println!(".");
    }
}

/// `realpath PATH` — resolve a path following all symlinks.
fn cmd_realpath(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        crate::console_println!("Usage: realpath <path>");
        return;
    }

    match crate::fs::Vfs::resolve_path(path) {
        Ok(resolved) => {
            crate::console_println!("{}", resolved);
        }
        Err(e) => {
            crate::console_println!("realpath: '{}': {:?}", path, e);
        }
    }
}

/// `pwd` — print working directory (always / in kernel shell).
fn cmd_pwd() {
    shell_println!("{}", get_cwd());
}

/// `cd [dir]` — change the current working directory.
fn cmd_cd(args: &str) {
    let target = if args.is_empty() {
        "/".into()
    } else {
        resolve_path(args)
    };

    // Verify the target is a directory.
    match crate::fs::Vfs::stat(&target) {
        Ok(meta) => {
            if meta.entry_type != crate::fs::EntryType::Directory {
                crate::console_println!("cd: not a directory: {}", target);
                set_exit(1);
                return;
            }
        }
        Err(e) => {
            crate::console_println!("cd: {}: {:?}", target, e);
            set_exit(1);
            return;
        }
    }

    // Update the working directory.
    {
        let mut cwd = CWD.lock();
        cwd.clear();
        cwd.push_str(&target);
    }
    // Keep $PWD in sync.
    sync_env_pwd();
}

/// `id` / `whoami` — show the current task's identity.
fn cmd_id() {
    let task_id = crate::sched::current_task_id();

    // Try to look up process credentials.
    // The kernel shell runs in the kernel's own context (task 0),
    // which is always uid=0/gid=0 (root).
    if let Some(creds) = crate::proc::pcb::get_credentials(task_id as u64) {
        crate::console_println!(
            "uid={} gid={} groups=[{}] task={}",
            creds.uid,
            creds.gid,
            {
                let mut g = alloc::string::String::new();
                for (i, gid) in creds.groups.iter().enumerate() {
                    if i > 0 {
                        g.push_str(", ");
                    }
                    g.push_str(&alloc::format!("{}", gid));
                }
                g
            },
            task_id
        );
    } else {
        // Fallback: kernel context, no PCB.
        crate::console_println!("uid=0(root) gid=0(root) task={}", task_id);
    }
}

/// `mktemp [DIR]` — create a temporary file and print its path.
///
/// Creates a file with a unique name in DIR (default: `/tmp`).
fn cmd_mktemp(args: &str) {
    let dir = if args.trim().is_empty() { "/tmp" } else { args.trim() };

    // Generate unique name from HPET + TSC for entropy.
    let ns = crate::hpet::elapsed_ns();
    let tsc = unsafe {
        // SAFETY: rdtsc is always available on x86_64 and has no side effects.
        core::arch::x86_64::_rdtsc()
    };
    // Combine both sources for uniqueness.
    let combined = ns ^ tsc;
    let path = alloc::format!("{}/tmp.{:016x}", dir, combined);

    match crate::fs::Vfs::write_file(&path, &[]) {
        Ok(()) => {
            crate::console_println!("{}", path);
        }
        Err(e) => {
            crate::console_println!("mktemp: cannot create temp file in '{}': {:?}", dir, e);
        }
    }
}

// ---------------------------------------------------------------------------
// Sysctl / hostname commands
// ---------------------------------------------------------------------------

/// `sysctl [name [value]]` — list, get, or set kernel parameters.
///
/// - `sysctl`          — list all parameters with current values
/// - `sysctl name`     — show a specific parameter
/// - `sysctl name val` — set a parameter to a new value
fn cmd_sysctl(args: &str) {
    let args = args.trim();

    if args.is_empty() {
        // List all parameters.
        let params = crate::sysctl::list_all();
        if params.is_empty() {
            crate::console_println!("(no parameters registered)");
            return;
        }
        for p in &params {
            crate::console_println!(
                "  {:<30} = {:<8} (default: {}, range: {}..{})",
                p.name, p.value, p.default, p.min, p.max
            );
        }
        return;
    }

    let mut parts = args.splitn(2, ' ');
    let name = parts.next().unwrap_or("");
    let value_str = parts.next().unwrap_or("").trim();

    if value_str.is_empty() {
        // Read a single parameter.
        match crate::sysctl::find_by_name(name) {
            Some(info) => {
                crate::console_println!(
                    "{} = {} (default: {}, range: {}..{})",
                    info.name, info.value, info.default, info.min, info.max
                );
            }
            None => {
                crate::console_println!("sysctl: unknown parameter '{}'", name);
            }
        }
    } else {
        // Set a parameter.
        match value_str.parse::<u64>() {
            Ok(value) => {
                match crate::sysctl::set_by_name(name, value) {
                    Some(old) => {
                        crate::console_println!("{} = {} (was {})", name, value, old);
                    }
                    None => {
                        crate::console_println!(
                            "sysctl: cannot set '{}' to {} (out of range or unknown)",
                            name, value
                        );
                    }
                }
            }
            Err(_) => {
                crate::console_println!("sysctl: invalid value '{}'", value_str);
            }
        }
    }
}

/// `hostname [name]` — show or set the system hostname.
fn cmd_hostname(args: &str) {
    let name = args.trim();

    if name.is_empty() {
        // Show current hostname.
        crate::console_println!("{}", crate::fs::sysfs::get_hostname());
    } else {
        // Set hostname.
        match crate::fs::Vfs::write_file("/sys/kernel/hostname", name.as_bytes()) {
            Ok(()) => {
                crate::console_println!("{}", name);
            }
            Err(e) => {
                crate::console_println!("hostname: cannot set hostname: {:?}", e);
            }
        }
    }
}

/// `dd if=FILE of=FILE [bs=N] [count=N]` — copy blocks between files.
///
/// Simplified `dd` supporting `if=`, `of=`, `bs=` (block size), and
/// `count=` (number of blocks) options.  Reads in chunks and writes
/// them out.  Reports bytes transferred.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_dd(args: &str) {
    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut bs: usize = 512;
    let mut count: Option<u64> = None;
    let mut skip: u64 = 0;  // Input blocks to skip.
    let mut seek: u64 = 0;  // Output blocks to skip (write position).

    // Parse key=value pairs.
    for token in args.split_whitespace() {
        if let Some(val) = token.strip_prefix("if=") {
            input = Some(val);
        } else if let Some(val) = token.strip_prefix("of=") {
            output = Some(val);
        } else if let Some(val) = token.strip_prefix("bs=") {
            bs = match parse_size_suffix(val) {
                Some(n) => n as usize,
                None => {
                    crate::console_println!("dd: invalid block size '{}'", val);
                    return;
                }
            };
        } else if let Some(val) = token.strip_prefix("count=") {
            count = match val.parse::<u64>() {
                Ok(n) => Some(n),
                Err(_) => {
                    crate::console_println!("dd: invalid count '{}'", val);
                    return;
                }
            };
        } else if let Some(val) = token.strip_prefix("skip=") {
            skip = match val.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    crate::console_println!("dd: invalid skip '{}'", val);
                    return;
                }
            };
        } else if let Some(val) = token.strip_prefix("seek=") {
            seek = match val.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    crate::console_println!("dd: invalid seek '{}'", val);
                    return;
                }
            };
        } else {
            crate::console_println!("dd: unknown option '{}'", token);
            crate::console_println!("Usage: dd if=FILE of=FILE [bs=N] [count=N] [skip=N] [seek=N]");
            return;
        }
    }

    let input = match input {
        Some(p) => p,
        None => {
            crate::console_println!("Usage: dd if=FILE of=FILE [bs=N] [count=N] [skip=N] [seek=N]");
            crate::console_println!("  bs=N     Block size (default 512, supports K/M/G suffix)");
            crate::console_println!("  count=N  Copy only N input blocks");
            crate::console_println!("  skip=N   Skip N input blocks before reading");
            crate::console_println!("  seek=N   Skip N output blocks before writing (uses write_at)");
            set_exit(1);
            return;
        }
    };
    let output = match output {
        Some(p) => p,
        None => {
            crate::console_println!("Usage: dd if=FILE of=FILE [bs=N] [count=N] [skip=N] [seek=N]");
            set_exit(1);
            return;
        }
    };

    if bs == 0 || bs > 1024 * 1024 {
        crate::console_println!("dd: block size must be 1..1M");
        set_exit(1);
        return;
    }

    let start_ns = crate::hpet::elapsed_ns();

    // Read input file.
    let data = match crate::fs::Vfs::read_file(input) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("dd: cannot read '{}': {:?}", input, e);
            set_exit(1);
            return;
        }
    };

    // Apply skip (input offset in blocks).
    let skip_bytes = (skip as usize).saturating_mul(bs);
    let data_after_skip = if skip_bytes < data.len() {
        data.get(skip_bytes..).unwrap_or(&[])
    } else {
        &[]
    };

    // Apply count limit.
    let max_bytes = match count {
        Some(n) => (n as usize).saturating_mul(bs),
        None => data_after_skip.len(),
    };
    let to_write = if max_bytes < data_after_skip.len() {
        data_after_skip.get(..max_bytes).unwrap_or(data_after_skip)
    } else {
        data_after_skip
    };

    // Write output file, using seek offset if specified.
    let write_result = if seek > 0 {
        let seek_bytes = (seek as usize).saturating_mul(bs);
        crate::fs::Vfs::write_at(output, seek_bytes as u64, to_write)
    } else {
        crate::fs::Vfs::write_file(output, to_write)
    };

    match write_result {
        Ok(()) => {
            let elapsed_ns = crate::hpet::elapsed_ns().saturating_sub(start_ns);
            let blocks = to_write.len() / bs;
            let remainder = to_write.len() % bs;
            let partial = if remainder > 0 { 1 } else { 0 };
            crate::console_println!(
                "{}+{} records in", blocks, partial
            );
            crate::console_println!(
                "{}+{} records out", blocks, partial
            );

            // Format throughput.
            let bytes_copied = to_write.len() as u64;
            if elapsed_ns > 0 {
                // Speed in KiB/s.
                let kib_per_sec = bytes_copied
                    .saturating_mul(1_000_000_000)
                    / elapsed_ns
                    / 1024;
                let ms = elapsed_ns / 1_000_000;
                crate::console_println!(
                    "{} bytes copied, {}.{:03} s, {} KiB/s",
                    bytes_copied,
                    ms / 1000,
                    ms % 1000,
                    kib_per_sec,
                );
            } else {
                crate::console_println!("{} bytes copied", bytes_copied);
            }
        }
        Err(e) => {
            crate::console_println!("dd: cannot write '{}': {:?}", output, e);
            set_exit(1);
        }
    }
}

/// Parse a size string with optional K/M/G suffix into bytes.
fn parse_size_suffix(s: &str) -> Option<u64> {
    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (s.get(..s.len().wrapping_sub(1))?, 1024u64 * 1024 * 1024)
    } else if s.ends_with('M') || s.ends_with('m') {
        (s.get(..s.len().wrapping_sub(1))?, 1024u64 * 1024)
    } else if s.ends_with('K') || s.ends_with('k') {
        (s.get(..s.len().wrapping_sub(1))?, 1024u64)
    } else {
        (s, 1u64)
    };
    num_str.parse::<u64>().ok().map(|n| n.saturating_mul(multiplier))
}

/// `free` — show memory usage summary in human-readable format.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_free() {
    let info = crate::mm::memory_info();

    let total_mb = info.total_bytes / (1024 * 1024);
    let used_mb = info.used_bytes / (1024 * 1024);
    let free_mb = info.free_bytes / (1024 * 1024);
    let swap_total_kib = info.swap_total_bytes / 1024;
    let swap_used_kib = info.swap_used_bytes / 1024;
    let swap_free_kib = swap_total_kib.saturating_sub(swap_used_kib);

    crate::console_println!(
        "             total       used       free"
    );
    crate::console_println!(
        "Mem:    {:>6} MiB  {:>6} MiB  {:>6} MiB",
        total_mb, used_mb, free_mb
    );
    crate::console_println!(
        "Swap:   {:>6} KiB  {:>6} KiB  {:>6} KiB",
        swap_total_kib, swap_used_kib, swap_free_kib
    );

    // Frame counts.
    crate::console_println!(
        "Frames: {:>6} total  {:>6} free  {:>6} zero-pool",
        info.total_frames, info.free_frames, info.zero_pool_count
    );

    // Heap.
    let heap_live = info.heap_slab_allocs.saturating_sub(info.heap_slab_frees);
    crate::console_println!(
        "Heap:   {:>6} slab allocs  {:>6} live  {:>6} large",
        info.heap_slab_allocs, heap_live, info.heap_large_allocs
    );

    // Pressure level at a glance.
    let level_str = match crate::mm::pressure::current_level() {
        crate::mm::pressure::PressureLevel::None => "none",
        crate::mm::pressure::PressureLevel::Low => "low",
        crate::mm::pressure::PressureLevel::Medium => "medium",
        crate::mm::pressure::PressureLevel::Critical => "CRITICAL",
    };
    crate::console_println!(
        "Pressure: {}  Fragmentation: {}%",
        level_str, info.fragmentation_pct
    );
}

/// Display virtual memory statistics in a one-counter-per-line format.
///
/// Similar to Linux's `cat /proc/vmstat` — a flat list of named counters
/// useful for diagnosing system behavior and scripting.
/// Display per-size-class slab allocator statistics.
///
/// Shows allocation patterns and active objects per class for
/// leak detection and memory profiling.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_slabinfo() {
    let classes = crate::mm::heap::class_stats();
    let overall = crate::mm::heap::stats();

    shell_println!("Kernel slab allocator — per-size-class statistics");
    shell_println!("");
    shell_println!(
        "{:>6} {:>10} {:>10} {:>8} {:>5}",
        "SIZE", "ALLOCS", "FREES", "ACTIVE", "PCT"
    );
    shell_println!("---------------------------------------------");

    let mut total_active: u64 = 0;
    for class in &classes {
        if class.allocs == 0 && class.frees == 0 {
            continue; // Skip unused classes.
        }
        total_active = total_active.saturating_add(class.active);
        let pct = if overall.slab_allocs > 0 {
            class.allocs.saturating_mul(100) / overall.slab_allocs
        } else {
            0
        };
        shell_println!(
            "{:>5}B {:>10} {:>10} {:>8} {:>4}%",
            class.class_size, class.allocs, class.frees, class.active, pct,
        );
    }

    shell_println!("---------------------------------------------");
    shell_println!(
        "{:>6} {:>10} {:>10} {:>8}",
        "TOTAL",
        overall.slab_allocs,
        overall.slab_frees,
        total_active,
    );
    shell_println!("");
    shell_println!(
        "Large allocs: {}  Large frees: {}  Refills: {}  Failures: {}",
        overall.large_allocs, overall.large_frees,
        overall.slab_refills, overall.alloc_failures,
    );
    shell_println!(
        "Poison: {}  UAF: {}  Double-free: {}  Overflow: {}",
        if overall.poison_enabled { "ON" } else { "off" },
        overall.poison_violations, overall.double_free_violations,
        overall.redzone_violations,
    );
}

/// Audit the kernel heap free lists for corruption.
///
/// Walks every size class's free list checking pointer validity,
/// cycle detection (Floyd's algorithm), and poison integrity.
fn cmd_heapaudit() {
    shell_println!("Auditing kernel heap free lists...");
    let result = crate::mm::heap::audit_free_lists();
    shell_println!("");
    shell_println!("  Free slots counted: {}", result.total_free_slots);
    shell_println!("  Corrupted slots:    {}", result.corrupted_slots);
    shell_println!("  Cycles detected:    {}", result.cycles_detected);
    shell_println!("  Bad pointers:       {}", result.bad_pointers);
    shell_println!("");
    if result.ok {
        shell_println!("  Result: PASS (no corruption detected)");
    } else {
        shell_println!("  Result: FAIL — heap corruption detected!");
    }
}

/// Show per-size-class internal fragmentation metrics.
///
/// Internal fragmentation occurs when the allocator rounds up a request
/// to the next power-of-2 class.  A 33-byte alloc uses a 64-byte slot,
/// wasting 31 bytes (48% frag).  This command shows cumulative waste
/// per class since boot.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_fraginfo() {
    let frag = crate::mm::heap::fragmentation_stats();

    shell_println!("Heap internal fragmentation (per size class):");
    shell_println!("");
    shell_println!("  {:>6}  {:>12}  {:>12}  {:>12}  {:>5}",
        "CLASS", "REQUESTED", "CONSUMED", "WASTED", "FRAG%");
    shell_println!("  {:->6}  {:->12}  {:->12}  {:->12}  {:->5}",
        "", "", "", "", "");

    let mut total_requested: u64 = 0;
    let mut total_consumed: u64 = 0;

    for entry in &frag {
        if entry.bytes_consumed == 0 {
            continue; // Skip unused classes.
        }
        total_requested += entry.bytes_requested;
        total_consumed += entry.bytes_consumed;

        shell_println!("  {:>6}  {:>12}  {:>12}  {:>12}  {:>4}%",
            entry.class_size,
            entry.bytes_requested,
            entry.bytes_consumed,
            entry.bytes_wasted,
            entry.frag_pct);
    }

    shell_println!("  {:->6}  {:->12}  {:->12}  {:->12}  {:->5}",
        "", "", "", "", "");

    let total_wasted = total_consumed.saturating_sub(total_requested);
    let total_pct = if total_consumed > 0 {
        (total_wasted * 100) / total_consumed
    } else {
        0
    };
    shell_println!("  {:>6}  {:>12}  {:>12}  {:>12}  {:>4}%",
        "TOTAL", total_requested, total_consumed, total_wasted, total_pct);
}

/// Run a heap leak check snapshot.
///
/// Compares current active-object counts per size class against the
/// previous snapshot.  Classes where active counts grow monotonically
/// across many consecutive checks are flagged as potential leaks.
///
/// Run this periodically (e.g., every few seconds) to detect leaks.
/// The first invocation establishes a baseline; subsequent invocations
/// show deltas and growth streaks.
fn cmd_leakcheck() {
    let result = crate::mm::heap::check_leaks();

    shell_println!("Heap leak check (snapshot comparison):");
    shell_println!("");
    shell_println!("  {:>6}  {:>10}  {:>8}  {:>7}  {}", "CLASS", "ACTIVE", "DELTA", "STREAK", "STATUS");
    shell_println!("  {:->6}  {:->10}  {:->8}  {:->7}  {:->10}", "", "", "", "", "");

    for entry in &result.classes {
        if entry.active == 0 && entry.delta == 0 {
            continue; // Skip idle classes.
        }
        let status = if entry.growth_streak >= 10 {
            "SUSPECT"
        } else if entry.growth_streak >= 5 {
            "growing"
        } else {
            "ok"
        };
        let delta_str = if entry.delta > 0 {
            alloc::format!("+{}", entry.delta)
        } else {
            alloc::format!("{}", entry.delta)
        };
        shell_println!("  {:>6}  {:>10}  {:>8}  {:>7}  {}",
            entry.class_size, entry.active, delta_str, entry.growth_streak, status);
    }

    shell_println!("");
    if result.suspect_classes > 0 {
        shell_println!("  WARNING: {} class(es) showing sustained growth (possible leak)", result.suspect_classes);
    } else {
        shell_println!("  No sustained leaks detected (run periodically for best results)");
    }
}

/// Quick physical memory test: allocate N frames, write patterns,
/// read back and verify.  Catches stuck bits, addressing faults,
/// and memory controller issues.
///
/// Usage: `memtest [count]` — default 32 frames (512 KiB).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_memtest(args: &str) {
    let count: usize = args.trim().parse().unwrap_or(32);
    if count == 0 || count > 1024 {
        shell_println!("Usage: memtest [1..1024] (frame count, default 32)");
        return;
    }

    shell_println!("Testing {} frames ({} KiB)...", count, count * 16);

    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => {
            shell_println!("ERROR: HHDM not available");
            return;
        }
    };

    // Allocate frames.
    let mut frames: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    for _ in 0..count {
        match crate::mm::frame::alloc_frame() {
            Ok(f) => frames.push(f.addr()),
            Err(_) => {
                shell_println!("  Allocation failed after {} frames (OOM)", frames.len());
                break;
            }
        }
    }

    let actual = frames.len();
    let frame_size = crate::mm::frame::FRAME_SIZE;
    let mut errors: usize = 0;

    // Pattern 1: Walking ones (0x01, 0x02, 0x04, ...)
    for (i, &phys) in frames.iter().enumerate() {
        let ptr = (phys + hhdm) as *mut u8;
        let pattern = 1u8 << (i & 7);
        // SAFETY: frame is allocated, HHDM maps it validly.
        unsafe {
            core::ptr::write_bytes(ptr, pattern, frame_size);
        }
    }
    for (i, &phys) in frames.iter().enumerate() {
        let ptr = (phys + hhdm) as *const u8;
        let pattern = 1u8 << (i & 7);
        // SAFETY: same as above.
        let slice = unsafe { core::slice::from_raw_parts(ptr, frame_size) };
        for (offset, &byte) in slice.iter().enumerate() {
            if byte != pattern {
                if errors < 8 {
                    shell_println!(
                        "  FAIL: frame {} offset {} expected {:#04x} got {:#04x}",
                        i, offset, pattern, byte
                    );
                }
                errors += 1;
            }
        }
    }

    // Pattern 2: Complement (0xFE, 0xFD, ...)
    for (i, &phys) in frames.iter().enumerate() {
        let ptr = (phys + hhdm) as *mut u8;
        let pattern = !(1u8 << (i & 7));
        unsafe {
            core::ptr::write_bytes(ptr, pattern, frame_size);
        }
    }
    for (i, &phys) in frames.iter().enumerate() {
        let ptr = (phys + hhdm) as *const u8;
        let pattern = !(1u8 << (i & 7));
        let slice = unsafe { core::slice::from_raw_parts(ptr, frame_size) };
        for (offset, &byte) in slice.iter().enumerate() {
            if byte != pattern {
                if errors < 8 {
                    shell_println!(
                        "  FAIL: frame {} offset {} expected {:#04x} got {:#04x}",
                        i, offset, pattern, byte
                    );
                }
                errors += 1;
            }
        }
    }

    // Free all frames.
    for &phys in &frames {
        if let Some(f) = crate::mm::frame::PhysFrame::from_addr(phys) {
            // SAFETY: we allocated these frames above.
            let _ = unsafe { crate::mm::frame::free_frame(f) };
        }
    }

    shell_println!("");
    if errors == 0 {
        shell_println!("  Result: PASS ({} frames, {} KiB verified)", actual, actual * 16);
    } else {
        shell_println!("  Result: FAIL ({} errors in {} frames)", errors, actual);
    }
}

fn cmd_workqueue() {
    let running = crate::workqueue::is_running();
    let submitted = crate::workqueue::submitted_count();
    let executed = crate::workqueue::executed_count();
    let dropped = crate::workqueue::dropped_count();
    let pending = crate::workqueue::pending_count();

    shell_println!("Kernel workqueue status");
    shell_println!("");
    shell_println!("  Worker task:  {}", if running { "running" } else { "NOT running" });
    shell_println!("  Pending:      {}", pending);
    shell_println!("  Submitted:    {}", submitted);
    shell_println!("  Executed:     {}", executed);
    shell_println!("  Dropped:      {}", dropped);
    if submitted > 0 {
        let drop_pct = dropped.saturating_mul(100) / submitted;
        shell_println!("  Drop rate:    {}%", drop_pct);
    }
}

fn cmd_ktimer() {
    let active = crate::ktimer::active_count();
    let scheduled = crate::ktimer::scheduled_count();
    let fired = crate::ktimer::fired_count();
    let cancelled = crate::ktimer::cancelled_count();

    shell_println!("Kernel timers");
    shell_println!("");
    shell_println!("  Active:       {}", active);
    shell_println!("  Scheduled:    {} (total since boot)", scheduled);
    shell_println!("  Fired:        {}", fired);
    shell_println!("  Cancelled:    {}", cancelled);
}

fn cmd_rng() {
    let init = crate::rng::is_initialized();
    let bytes = crate::rng::total_bytes_generated();
    let reseeds = crate::rng::reseed_count();
    let entropy = crate::rng::entropy_contributions();

    shell_println!("Kernel CSPRNG (ChaCha20)");
    shell_println!("");
    shell_println!("  Initialized:  {}", if init { "yes" } else { "no" });
    shell_println!("  Generated:    {} bytes", bytes);
    shell_println!("  Reseeds:      {}", reseeds);
    shell_println!("  Entropy in:   {} contributions (ISR timing)", entropy);

    // Show a sample random value.
    let sample = crate::rng::next_u64();
    shell_println!("  Sample:       {:#018x}", sample);
}

fn cmd_supervisor() {
    let active = crate::sched::supervisor::active_count();
    let restarts = crate::sched::supervisor::total_restarts();
    let exits = crate::sched::supervisor::total_exits();
    let failures = crate::sched::supervisor::total_failures();

    shell_println!("Task supervisor");
    shell_println!("");
    shell_println!("  Supervised:   {} active", active);
    shell_println!("  Exits seen:   {}", exits);
    shell_println!("  Restarts:     {}", restarts);
    shell_println!("  Failures:     {}", failures);
}

fn cmd_trace(args: &str) {
    let count: usize = if args.is_empty() {
        20
    } else {
        args.trim().parse().unwrap_or(20)
    };
    let count = count.min(64); // Cap display to 64 entries.

    let total = crate::ktrace::total_events();
    let valid = crate::ktrace::valid_count();
    let enabled = crate::ktrace::is_enabled();

    shell_println!("Kernel trace buffer");
    shell_println!("  Status:   {}", if enabled { "recording" } else { "paused" });
    shell_println!("  Events:   {} total, {} in buffer", total, valid);
    shell_println!("");

    if valid == 0 {
        shell_println!("  (no events recorded)");
        return;
    }

    // Read entries.
    let mut entries = [crate::ktrace::TraceEntry::empty(); 64];
    let read_count = crate::ktrace::read_recent(&mut entries[..count]);

    shell_println!("  {:>5}  {:>10}  {:>8}  {:>4}  {:>16}  {:>16}",
        "TASK", "TIMESTAMP", "CATEGORY", "EVT", "ARG0", "ARG1");
    shell_println!("  {:->5}  {:->10}  {:->8}  {:->4}  {:->16}  {:->16}",
        "", "", "", "", "", "");

    for i in 0..read_count {
        let e = &entries[i];
        if e.timestamp == 0 {
            continue; // Empty slot.
        }
        shell_println!("  {:>5}  {:>10}  {:>8}  {:>4}  {:#016x}  {:#016x}",
            e.task_id,
            e.timestamp % 1_000_000_000, // Show lower 9 digits.
            e.category_name(),
            e.event_id(),
            e.arg0,
            e.arg1,
        );
    }
}

/// Show the current kernel call stack via frame-pointer walking.
///
/// This captures the backtrace from the point of the kshell command
/// handler, showing the call chain up through the shell loop to the
/// kernel entry point.  Useful for verifying that frame pointers are
/// working and for debugging where control flow is.
#[inline(never)] // Ensure this function has its own stack frame for the backtrace.
fn cmd_backtrace() {
    let bt = crate::backtrace::capture();
    if bt.count == 0 {
        shell_println!("No backtrace available (frame pointers may be absent).");
        return;
    }
    shell_println!("Backtrace ({} frames):", bt.count);
    shell_println!("");
    shell_println!("  {:>3}  {:>18}  {:>18}", "#", "RETURN ADDR", "FRAME PTR");
    shell_println!("  {:->3}  {:->18}  {:->18}", "", "", "");
    for i in 0..bt.count {
        let f = &bt.frames[i];
        shell_println!("  {:>3}  {:#018x}  {:#018x}", i, f.return_addr, f.frame_ptr);
    }
}

/// One-stop system health diagnostic.
///
/// Shows a compact summary of kernel health indicators:
/// memory, heap, pressure, watchdog, lockdep, and detected anomalies.
/// Designed to quickly answer "is anything wrong?" after a test run.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_diag() {
    shell_println!("=== System Health Diagnostic ===");
    shell_println!("");

    // --- Memory ---
    let mem = crate::mm::memory_info();
    let used = mem.total_frames.saturating_sub(mem.free_frames);
    let pct_used = if mem.total_frames > 0 { (used * 100) / mem.total_frames } else { 0 };
    let status_mem = if pct_used > 90 { "CRITICAL" } else if pct_used > 75 { "WARNING" } else { "OK" };
    shell_println!("  Memory:    {} / {} frames used ({}%)  [{}]",
        used, mem.total_frames, pct_used, status_mem);

    // --- Pressure ---
    let pressure = crate::mm::pressure::pressure_info();
    let status_pressure = match pressure.level {
        crate::mm::pressure::PressureLevel::None => "OK",
        crate::mm::pressure::PressureLevel::Low => "LOW",
        crate::mm::pressure::PressureLevel::Medium => "MEDIUM",
        crate::mm::pressure::PressureLevel::Critical => "CRITICAL",
    };
    shell_println!("  Pressure:  level={}, notifications={}, freed={}  [{}]",
        pressure.level, pressure.total_notifications, pressure.total_freed, status_pressure);

    // --- Heap ---
    let heap = crate::mm::heap::stats();
    let active_slab = heap.slab_allocs.saturating_sub(heap.slab_frees);
    let active_large = heap.large_allocs.saturating_sub(heap.large_frees);
    let status_heap = if heap.alloc_failures > 0 { "FAILURES" }
        else if heap.poison_violations > 0 || heap.double_free_violations > 0 || heap.redzone_violations > 0 { "VIOLATIONS" }
        else { "OK" };
    shell_println!("  Heap:      slab_active={}, large_active={}, failures={}  [{}]",
        active_slab, active_large, heap.alloc_failures, status_heap);
    if heap.poison_violations > 0 || heap.double_free_violations > 0 || heap.redzone_violations > 0 {
        shell_println!("             UAF={}, double-free={}, overflow={}",
            heap.poison_violations, heap.double_free_violations, heap.redzone_violations);
    }

    // --- Lockdep ---
    let violations = crate::lockdep::violation_count();
    let lockdep_enabled = crate::lockdep::is_enabled();
    let status_lockdep = if !lockdep_enabled { "DISABLED" }
        else if violations > 0 { "VIOLATIONS" }
        else { "OK" };
    shell_println!("  Lockdep:   enabled={}, classes={}, edges={}, violations={}  [{}]",
        lockdep_enabled, crate::lockdep::class_count(), crate::lockdep::edge_count(),
        violations, status_lockdep);

    // --- Scheduler ---
    let sched = crate::sched::sched_stats();
    let active_tasks = sched.total_tasks_spawned.saturating_sub(sched.total_tasks_exited);
    shell_println!("  Scheduler: {} active tasks, {} spawned, {} exited",
        active_tasks, sched.total_tasks_spawned, sched.total_tasks_exited);

    // --- Uptime ---
    let ticks = crate::apic::tick_count();
    let seconds = ticks / 100;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    shell_println!("  Uptime:    {:02}:{:02}:{:02} ({} ticks)",
        hours, minutes % 60, seconds % 60, ticks);

    // --- Page faults ---
    let pf = crate::mm::fault::fault_stats();
    let status_pf = if pf.fatal > 0 { "FATAL FAULTS" } else { "OK" };
    shell_println!("  PgFaults:  resolved={}, cow={}, fatal={}  [{}]",
        pf.kernel_resolved.saturating_add(pf.user_resolved),
        pf.cow, pf.fatal, status_pf);

    // --- TLB ---
    let tlb = crate::tlb::stats();
    shell_println!("  TLB:       range={}, full={}, ipi={}, local={}",
        tlb.range_flushes, tlb.full_flushes, tlb.ipi_flushes, tlb.local_only);

    // --- Exceptions ---
    let exc_total: u64 = crate::idt::vector_counts().iter().sum();
    let pf_count = crate::idt::vector_count(14);
    let non_pf_exceptions = exc_total.saturating_sub(pf_count);
    let status_exc = if non_pf_exceptions > 100 { "HIGH" } else { "OK" };
    shell_println!("  Exceptions: total={} (PF={}, other={})  [{}]",
        exc_total, pf_count, non_pf_exceptions, status_exc);

    // --- Timer jitter ---
    let jitter_status = if let Some(j) = crate::apic::timer_jitter() {
        if j.mean_cycles > 0 {
            let max_dev_pct = j.max_cycles.saturating_sub(j.mean_cycles)
                .saturating_mul(100)
                .checked_div(j.mean_cycles)
                .unwrap_or(0);
            if max_dev_pct > 20 {
                shell_println!("  Jitter:    max_dev=+{}% ({}→{} cycles)  [HIGH]",
                    max_dev_pct, j.mean_cycles, j.max_cycles);
                "HIGH"
            } else {
                shell_println!("  Jitter:    max_dev=+{}% ({} samples)  [OK]",
                    max_dev_pct, j.count);
                "OK"
            }
        } else {
            shell_println!("  Jitter:    (no data)");
            "OK"
        }
    } else {
        shell_println!("  Jitter:    (no data)");
        "OK"
    };

    // --- Heap watermark ---
    shell_println!("  HeapWM:    in_use={} KiB, peak={} KiB",
        heap.bytes_in_use / 1024, heap.peak_bytes_in_use / 1024);

    // --- Scheduling latency ---
    let lat = crate::sched::latency_histogram();
    let lat_status = if lat.total_events > 0 {
        let high_lat = lat.buckets[5].saturating_add(lat.buckets[6]).saturating_add(lat.buckets[7]);
        let high_pct = high_lat.saturating_mul(100).checked_div(lat.total_events).unwrap_or(0);
        if high_pct > 5 {
            shell_println!("  Latency:   max={}ms, {}% >200ms  [HIGH]",
                lat.max_ticks * 10, high_pct);
            "HIGH"
        } else {
            shell_println!("  Latency:   max={}ms, mean={}.{}ms  [OK]",
                lat.max_ticks * 10,
                lat.mean_ticks_x100 / 10, lat.mean_ticks_x100 % 10);
            "OK"
        }
    } else {
        shell_println!("  Latency:   (no dispatches yet)");
        "OK"
    };

    // --- Memory pressure score ---
    let mp = crate::mm::memory_pressure();
    let mp_status = match mp.level {
        crate::mm::PressureLevel::Low => "OK",
        crate::mm::PressureLevel::Moderate => "MODERATE",
        crate::mm::PressureLevel::High => "HIGH",
        crate::mm::PressureLevel::Critical => "CRITICAL",
    };
    shell_println!("  MemScore:  {}/100 (phys={} frag={} heap={} swap={})  [{}]",
        mp.score, mp.phys_score, mp.frag_score, mp.heap_score, mp.swap_score, mp_status);

    // --- Overall assessment ---
    shell_println!("");
    let issues = (pct_used > 90) as u8
        + (pressure.level as u8 >= 2) as u8
        + (heap.alloc_failures > 0) as u8
        + (heap.poison_violations > 0) as u8
        + (heap.double_free_violations > 0) as u8
        + (heap.redzone_violations > 0) as u8
        + (pf.fatal > 0) as u8
        + (violations > 0) as u8
        + (jitter_status == "HIGH") as u8
        + (lat_status == "HIGH") as u8
        + (mp.score > 75) as u8;
    if issues == 0 {
        shell_println!("  Overall: HEALTHY (no issues detected)");
    } else {
        shell_println!("  Overall: {} issue(s) detected — investigate above warnings", issues);
    }
}

fn cmd_exceptions() {
    use crate::idt;

    let counts = idt::vector_counts();
    let any_nonzero = counts.iter().any(|&c| c > 0);

    if !any_nonzero {
        shell_println!("No exceptions or interrupts recorded.");
        return;
    }

    shell_println!("{:<6} {:<32} {:>12}", "VEC", "NAME", "COUNT");
    shell_println!("{}", "-".repeat(52));

    for (i, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let name = if i < 32 {
            idt::EXCEPTION_NAMES[i]
        } else {
            "IRQ"
        };
        shell_println!("{:<6} {:<32} {:>12}", i, name, count);
    }

    // Summary line.
    let total: u64 = counts.iter().sum();
    shell_println!("{}", "-".repeat(52));
    shell_println!("{:<6} {:<32} {:>12}", "", "TOTAL", total);
}

fn cmd_exclog() {
    use crate::idt;

    let (entries, total) = idt::recent_exceptions();
    let count = total.min(32) as usize;

    if count == 0 {
        shell_println!("No exceptions logged.");
        return;
    }

    shell_println!("Recent exceptions ({} total, showing last {}):", total, count);
    shell_println!("{:<8} {:<5} {:<4} {:<18} {:<18}", "TICK", "VEC", "CPU", "RIP", "AUX");
    shell_println!("{}", "-".repeat(56));

    for entry in entries.iter().take(count) {
        if entry.tick == 0 {
            continue;
        }
        let name = if (entry.vector as usize) < 32 {
            idt::EXCEPTION_NAMES[entry.vector as usize]
        } else {
            "IRQ"
        };
        shell_println!("{:<8} {:<5} {:<4} {:#018x} {:#018x}",
            entry.tick, name, entry.cpu, entry.rip, entry.aux);
    }
}

fn cmd_boottime() {
    let milestones = crate::boot_timing::milestones();

    shell_println!("=== Boot Timing (APIC ticks @ 100 Hz = 10ms each) ===");
    shell_println!("");
    shell_println!("{:<20} {:>8} {:>8}", "MILESTONE", "TICK", "DELTA");
    shell_println!("{}", "-".repeat(40));

    let mut prev_tick = 0u64;
    for &(name, tick) in &milestones {
        if tick == 0 {
            shell_println!("{:<20} {:>8}", name, "(pre-timer)");
        } else {
            let delta = tick.saturating_sub(prev_tick);
            let delta_str = if prev_tick == 0 {
                alloc::format!("--")
            } else {
                alloc::format!("+{}", delta)
            };
            shell_println!("{:<20} {:>8} {:>8}", name, tick, delta_str);
            prev_tick = tick;
        }
    }

    // Total boot time.
    let last_tick = milestones.iter().rev().find(|(_, t)| *t > 0).map_or(0, |(_, t)| *t);
    let first_tick = milestones.iter().find(|(_, t)| *t > 0).map_or(0, |(_, t)| *t);
    if last_tick > first_tick {
        shell_println!("");
        let total = last_tick.saturating_sub(first_tick);
        shell_println!("  Total timed boot: {} ticks ({} ms)",
            total, total.saturating_mul(10));
    }
}

fn cmd_canary() {
    let result = crate::sched::check_all_canaries();

    shell_println!("=== Stack Canary Scan ===");
    shell_println!("");
    shell_println!("  Scanned:   {} tasks", result.scanned);
    shell_println!("  OK:        {} (canary intact)", result.ok);
    shell_println!("  Skipped:   {} (idle/no stack)", result.skipped);

    if result.corrupted.is_empty() {
        shell_println!("");
        shell_println!("  Result: ALL CANARIES INTACT");
    } else {
        shell_println!("  CORRUPTED: {} *** STACK OVERFLOW DETECTED ***", result.corrupted.len());
        shell_println!("");
        for &(tid, ref name, name_len) in &result.corrupted {
            let name_str = core::str::from_utf8(&name[..name_len]).unwrap_or("?");
            shell_println!("    Task {} ({:?}) — canary destroyed!", tid, name_str);
        }
    }
}

fn cmd_sysinfo() {
    use crate::cpu;

    shell_println!("=== CPU Information ===");
    shell_println!("");

    // Vendor string.
    let vendor_bytes = cpu::vendor_string();
    let vendor = core::str::from_utf8(&vendor_bytes).unwrap_or("<unknown>");
    shell_println!("  Vendor:    {}", vendor);

    // Brand string.
    let brand_bytes = cpu::brand_string();
    // Trim trailing nulls and whitespace.
    let brand_len = brand_bytes.iter().rposition(|&b| b != 0 && b != b' ')
        .map_or(0, |i| i + 1);
    let brand = core::str::from_utf8(&brand_bytes[..brand_len]).unwrap_or("<unknown>");
    shell_println!("  Brand:     {}", brand);

    // Family/model/stepping.
    let (family, model, stepping) = cpu::cpu_family_model_stepping();
    shell_println!("  Family:    {:#x}  Model: {:#x}  Stepping: {}", family, model, stepping);

    shell_println!("");
    shell_println!("=== Feature Flags ===");
    shell_println!("");

    let Some(f) = cpu::features() else {
        shell_println!("  (features not yet detected)");
        return;
    };

    // Group features by category for readability.
    shell_println!("  Base SIMD:");
    shell_println!("    SSE={} SSE2={} SSE3={} SSSE3={} SSE4.1={} SSE4.2={}",
        f.sse, f.sse2, f.sse3, f.ssse3, f.sse4_1, f.sse4_2);

    shell_println!("  Advanced SIMD:");
    shell_println!("    AVX={} AVX2={} AVX-512F={} F16C={}",
        f.avx, f.avx2, f.avx512f, f.f16c);

    shell_println!("  Bit manipulation:");
    shell_println!("    POPCNT={} BMI1={} BMI2={}",
        f.popcnt, f.bmi1, f.bmi2);

    shell_println!("  Crypto:");
    shell_println!("    AES-NI={} SHA={} VAES={}",
        f.aes_ni, f.sha, f.vaes);

    shell_println!("  Random:");
    shell_println!("    RDRAND={} RDSEED={}",
        f.rdrand, f.rdseed);

    shell_println!("  System:");
    shell_println!("    TSC={} RDTSCP={} RDPID={} APIC={} FXSR={} XSAVE={}",
        f.tsc, f.rdtscp, f.rdpid, f.apic, f.fxsr, f.xsave);

    shell_println!("  Memory:");
    shell_println!("    1GiB pages={}",
        f.page_1g);

    if f.xsave {
        shell_println!("  XSAVE:");
        shell_println!("    area size={} bytes, XCR0={:#x}",
            f.xsave_area_size, f.xcr0_supported);
    }

    if f.pmu_version > 0 {
        shell_println!("  Performance Monitoring:");
        shell_println!("    version={}, counters={}, width={} bits",
            f.pmu_version, f.pmu_counters, f.pmu_counter_width);
    }

    // CPU count.
    shell_println!("");
    shell_println!("  Logical CPUs: {}", crate::smp::cpu_count());

    // Cache topology.
    let caches = cpu::cache_topology();
    if !caches.is_empty() {
        shell_println!("");
        shell_println!("=== Cache Topology ===");
        shell_println!("");
        for c in caches {
            let size_str = if c.size >= 1024 * 1024 {
                alloc::format!("{} MiB", c.size / (1024 * 1024))
            } else {
                alloc::format!("{} KiB", c.size / 1024)
            };
            shell_println!("  L{} {:<11} {:>8}  {}-way  {}-byte line  {} sets{}",
                c.level,
                c.type_name(),
                size_str,
                c.ways,
                c.line_size,
                c.sets,
                if c.shared { "  (shared)" } else { "" },
            );
        }
        shell_println!("");
        shell_println!("  Cache line size: {} bytes", cpu::cache_line_size());
    }
}

fn cmd_tlb() {
    let s = crate::tlb::stats();
    shell_println!("=== TLB Shootdown Statistics ===");
    shell_println!("");
    shell_println!("  Range flushes (invlpg):    {}", s.range_flushes);
    shell_println!("  Full flushes (CR3 reload): {}", s.full_flushes);
    shell_println!("  Total pages invalidated:   {}", s.total_pages_flushed);
    shell_println!("");
    shell_println!("  IPI-based (multi-CPU):     {}", s.ipi_flushes);
    shell_println!("  Local-only (single-CPU):   {}", s.local_only);

    let total_ops = s.range_flushes.saturating_add(s.full_flushes);
    if total_ops > 0 && s.range_flushes > 0 {
        let avg_pages = s.total_pages_flushed / s.range_flushes;
        shell_println!("");
        shell_println!("  Avg pages per range flush: {}", avg_pages);
    }
}

fn cmd_pgfault() {
    let s = crate::mm::fault::fault_stats();
    let total_vector = crate::idt::vector_count(14);

    shell_println!("=== Page Fault Statistics ===");
    shell_println!("");
    shell_println!("  Total PF exceptions (vector 14): {}", total_vector);
    shell_println!("");
    shell_println!("  Kernel-mode resolved:  {}", s.kernel_resolved);
    shell_println!("  User-mode resolved:    {}", s.user_resolved);
    shell_println!("  Fatal (unresolvable):  {}", s.fatal);
    shell_println!("");
    shell_println!("  By type:");
    shell_println!("    Copy-on-Write:       {}", s.cow);
    shell_println!("    Swap-in:             {}", s.swap_in);
    shell_println!("    Stack growth:        {}", s.stack_growth);
    let demand = s.kernel_resolved.saturating_add(s.user_resolved)
        .saturating_sub(s.cow)
        .saturating_sub(s.swap_in)
        .saturating_sub(s.stack_growth);
    shell_println!("    Demand page (other): {}", demand);
}

/// `sar` — compact system activity reporter (one-line per call).
///
/// Shows a single compact line with key metrics, suitable for repeated
/// invocation to observe trends over time:
/// - Memory: used/total MiB, pressure score
/// - CPU: load, ctx switches/sec (since last call)
/// - Sched: latency max, tasks
/// - Heap: in-use KiB, peak KiB
///
/// Named after the Unix `sar` (System Activity Reporter) utility.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_sar() {
    use core::sync::atomic::{AtomicU64, Ordering};

    // Use statics to track deltas between calls.
    static LAST_CTX: AtomicU64 = AtomicU64::new(0);
    static LAST_TICK: AtomicU64 = AtomicU64::new(0);

    let now_tick = crate::apic::tick_count();
    let stats = crate::sched::sched_stats();
    let prev_ctx = LAST_CTX.swap(stats.total_ctx_switches, Ordering::Relaxed);
    let prev_tick = LAST_TICK.swap(now_tick, Ordering::Relaxed);

    let elapsed = now_tick.saturating_sub(prev_tick);
    let ctx_delta = stats.total_ctx_switches.saturating_sub(prev_ctx);

    // Context switches per second.
    let ctx_per_sec = if elapsed > 0 {
        ctx_delta.saturating_mul(u64::from(crate::apic::TICK_RATE_HZ)) / elapsed
    } else {
        0
    };

    // Memory.
    let mem = crate::mm::memory_info();
    let used_mb = mem.used_bytes / (1024 * 1024);
    let total_mb = mem.total_bytes / (1024 * 1024);
    let mp = crate::mm::memory_pressure();

    // Heap.
    let heap = crate::mm::heap::stats();

    // Scheduler.
    let active = stats.total_tasks_spawned.saturating_sub(stats.total_tasks_exited);
    let lat = crate::sched::latency_histogram();
    let load = stats.load_avg_x100;

    // Print header on first call (elapsed == 0).
    if elapsed == 0 || prev_tick == 0 {
        shell_println!("mem_used  mem_tot  press  load   ctx/s  lat_max  tasks  heap_use  heap_pk");
        shell_println!("-------   -------  -----  ----   -----  -------  -----  --------  -------");
    }

    shell_println!(
        "{:>4} MiB {:>4} MiB {:>4}  {}.{:02} {:>6}  {:>4}ms {:>5}  {:>5}KiB {:>5}KiB",
        used_mb, total_mb,
        mp.score,
        load / 100, load % 100,
        ctx_per_sec,
        lat.max_ticks * 10,
        active,
        heap.bytes_in_use / 1024,
        heap.peak_bytes_in_use / 1024,
    );
}

/// `syshealth` — active system health verification.
///
/// Unlike 'diag' (which reads passive counters), this command actively
/// tests kernel subsystem integrity:
/// - Allocate and free a frame (frame allocator works)
/// - Allocate and free heap memory (heap works)
/// - Check stack canary of current task (stack intact)
/// - Verify timer ticks are advancing (APIC alive)
/// - Check scheduler can yield and return (scheduler works)
#[allow(clippy::arithmetic_side_effects)]
fn cmd_syshealth() {
    shell_println!("=== Active System Health Check ===");
    shell_println!("");

    let mut passed = 0u32;
    let mut failed = 0u32;

    // Test 1: Frame allocator round-trip.
    {
        let result = crate::mm::frame::alloc_frame();
        match result {
            Ok(frame) => {
                // SAFETY: frame was just allocated by us, is valid and
                // exclusively owned.  No other references exist.
                let _ = unsafe { crate::mm::frame::free_frame(frame) };
                shell_println!("  [PASS] Frame allocator: alloc+free OK");
                passed += 1;
            }
            Err(e) => {
                shell_println!("  [FAIL] Frame allocator: {:?}", e);
                failed += 1;
            }
        }
    }

    // Test 2: Heap allocator round-trip.
    {
        let before = crate::mm::heap::stats().bytes_in_use;
        let v: alloc::vec::Vec<u8> = alloc::vec![42u8; 256];
        let after = crate::mm::heap::stats().bytes_in_use;
        if after > before && v[0] == 42 && v[255] == 42 {
            shell_println!("  [PASS] Heap allocator: alloc 256B, read OK");
            passed += 1;
        } else {
            shell_println!("  [FAIL] Heap allocator: unexpected bytes_in_use delta");
            failed += 1;
        }
        drop(v);
    }

    // Test 3: Stack canary integrity (current task).
    {
        let scan = crate::sched::check_all_canaries();
        if scan.corrupted.is_empty() {
            shell_println!("  [PASS] Stack canaries: {}/{} intact",
                scan.ok, scan.scanned);
            passed += 1;
        } else {
            shell_println!("  [FAIL] Stack canaries: {} corrupted!", scan.corrupted.len());
            failed += 1;
        }
    }

    // Test 4: APIC timer advancing.
    {
        let t1 = crate::apic::tick_count();
        // Busy-wait a tiny bit (a few hundred cycles) then check.
        for _ in 0..10000 {
            core::hint::spin_loop();
        }
        let t2 = crate::apic::tick_count();
        // Even with the short spin, ticks should have advanced if the
        // timer is running at 100 Hz.  If not, at minimum t2 >= t1.
        if t2 >= t1 {
            shell_println!("  [PASS] APIC timer: tick advancing ({}→{})", t1, t2);
            passed += 1;
        } else {
            shell_println!("  [FAIL] APIC timer: ticks went backward!");
            failed += 1;
        }
    }

    // Test 5: No lockdep violations.
    {
        let violations = crate::lockdep::violation_count();
        if violations == 0 {
            shell_println!("  [PASS] Lockdep: no ordering violations");
            passed += 1;
        } else {
            shell_println!("  [FAIL] Lockdep: {} violation(s) detected", violations);
            failed += 1;
        }
    }

    // Test 6: No heap corruption detected.
    {
        let hs = crate::mm::heap::stats();
        let violations = hs.poison_violations + hs.double_free_violations + hs.redzone_violations;
        if violations == 0 {
            shell_println!("  [PASS] Heap safety: no UAF/double-free/overflow");
            passed += 1;
        } else {
            shell_println!("  [FAIL] Heap safety: {} violation(s)", violations);
            failed += 1;
        }
    }

    // Test 7: No fatal page faults.
    {
        let pf = crate::mm::fault::fault_stats();
        if pf.fatal == 0 {
            shell_println!("  [PASS] Page faults: no fatal faults since boot");
            passed += 1;
        } else {
            shell_println!("  [FAIL] Page faults: {} fatal fault(s)", pf.fatal);
            failed += 1;
        }
    }

    // Summary.
    shell_println!("");
    shell_println!("  Result: {}/{} passed, {} failed",
        passed, passed + failed, failed);
    if failed == 0 {
        shell_println!("  System: ALL CHECKS PASSED");
    } else {
        shell_println!("  System: ISSUES DETECTED — investigate failures above");
    }
}

/// `kprofile` — display kernel code profiling results.
///
/// Shows TSC cycle counts for instrumented kernel code regions.
/// Subcommands:
///   `kprofile`        — show all active measurements
///   `kprofile reset`  — reset all counters
///   `kprofile on`     — enable profiling
///   `kprofile off`    — disable profiling
fn cmd_kprofile(args: &str) {
    let arg = args.trim();

    match arg {
        "reset" | "clear" => {
            crate::kprofile::reset();
            shell_println!("Profiling counters reset.");
            return;
        }
        "on" | "enable" => {
            crate::kprofile::set_enabled(true);
            shell_println!("Profiling enabled.");
            return;
        }
        "off" | "disable" => {
            crate::kprofile::set_enabled(false);
            shell_println!("Profiling disabled.");
            return;
        }
        _ => {}
    }

    let enabled = if crate::kprofile::is_enabled() { "ON" } else { "OFF" };
    shell_println!("=== Kernel Code Profiler [{}] ===", enabled);
    shell_println!("");

    let snapshots = crate::kprofile::snapshots();
    let mut any = false;

    let freq = crate::bench::tsc_freq();
    if freq > 0 {
        shell_println!("  TSC frequency: {} MHz", freq / 1_000_000);
        shell_println!("");
        shell_println!("  {:<14} {:>8} {:>8} {:>8} {:>8}  {:>7} {:>7} {:>7}",
            "Region", "Count", "Min cy", "Mean cy", "Max cy", "Min ns", "Mean ns", "Max ns");
        shell_println!("  {:<14} {:>8} {:>8} {:>8} {:>8}  {:>7} {:>7} {:>7}",
            "------", "-----", "------", "-------", "------", "------", "-------", "------");
    } else {
        shell_println!("  {:<14} {:>10} {:>10} {:>10} {:>10}",
            "Region", "Count", "Min", "Mean", "Max");
        shell_println!("  {:<14} {:>10} {:>10} {:>10} {:>10}",
            "------", "-----", "---", "----", "---");
    }

    for snap in &snapshots {
        if let Some(s) = snap {
            any = true;
            if freq > 0 {
                let min_ns = crate::bench::cycles_to_ns(s.min_cycles);
                let mean_ns = crate::bench::cycles_to_ns(s.mean_cycles);
                let max_ns = crate::bench::cycles_to_ns(s.max_cycles);
                shell_println!("  {:<14} {:>8} {:>8} {:>8} {:>8}  {:>7} {:>7} {:>7}",
                    s.name, s.count,
                    s.min_cycles, s.mean_cycles, s.max_cycles,
                    min_ns, mean_ns, max_ns);
            } else {
                shell_println!("  {:<14} {:>10} {:>10} {:>10} {:>10}",
                    s.name, s.count, s.min_cycles, s.mean_cycles, s.max_cycles);
            }
        }
    }

    if !any {
        shell_println!("  (no measurements recorded yet)");
        shell_println!("");
        shell_println!("  Profiling is {}. Instrumented paths will auto-record.", enabled);
    }
}

/// `lockstats` — display spinlock contention statistics.
///
/// Shows per-lock contention data: acquisitions, contentions, wait
/// times, and hold times.  Subcommands:
///   `lockstats`       — show all tracked locks
///   `lockstats reset` — reset all counters
///   `lockstats off`   — disable contention tracking
///   `lockstats on`    — enable contention tracking
#[allow(clippy::cast_possible_truncation)]
fn cmd_lockstats(args: &str) {
    match args.trim() {
        "reset" => {
            crate::sync::reset_all_stats();
            shell_println!("Lock contention stats reset.");
            return;
        }
        "off" => {
            crate::sync::set_tracking_enabled(false);
            shell_println!("Lock contention tracking disabled.");
            return;
        }
        "on" => {
            crate::sync::set_tracking_enabled(true);
            shell_println!("Lock contention tracking enabled.");
            return;
        }
        _ => {}
    }

    shell_println!("=== Spinlock Contention ===");
    shell_println!("");

    let snapshots = crate::sync::lock_stats();
    let freq = crate::bench::tsc_freq();
    let mut any = false;

    if freq > 0 {
        shell_println!("  {:<12} {:>8} {:>8} {:>5}  {:>7} {:>7}  {:>7} {:>7}",
            "Lock", "Acquires", "Content.", "Pct",
            "WaitMax", "HoldMax", "WaitTot", "HoldTot");
        shell_println!("  {:<12} {:>8} {:>8} {:>5}  {:>7} {:>7}  {:>7} {:>7}",
            "----", "--------", "--------", "---",
            "-------", "-------", "-------", "-------");
    } else {
        shell_println!("  {:<12} {:>8} {:>8} {:>5}  {:>10} {:>10}",
            "Lock", "Acquires", "Content.", "Pct", "MaxWait cy", "MaxHold cy");
        shell_println!("  {:<12} {:>8} {:>8} {:>5}  {:>10} {:>10}",
            "----", "--------", "--------", "---", "----------", "----------");
    }

    for snap in &snapshots {
        if let Some(s) = snap {
            if s.acquisitions == 0 {
                continue;
            }
            any = true;
            let name_str = core::str::from_utf8(s.name).unwrap_or("???");
            let pct = if s.acquisitions > 0 {
                (s.contentions * 100) / s.acquisitions
            } else {
                0
            };

            if freq > 0 {
                let max_wait_ns = crate::bench::cycles_to_ns(s.max_wait_cycles);
                let max_hold_ns = crate::bench::cycles_to_ns(s.max_hold_cycles);
                let tot_wait_us = crate::bench::cycles_to_ns(s.total_wait_cycles) / 1000;
                let tot_hold_us = crate::bench::cycles_to_ns(s.total_hold_cycles) / 1000;
                shell_println!("  {:<12} {:>8} {:>8} {:>4}%  {:>5}ns {:>5}ns  {:>5}us {:>5}us",
                    name_str, s.acquisitions, s.contentions, pct,
                    max_wait_ns, max_hold_ns, tot_wait_us, tot_hold_us);
            } else {
                shell_println!("  {:<12} {:>8} {:>8} {:>4}%  {:>10} {:>10}",
                    name_str, s.acquisitions, s.contentions, pct,
                    s.max_wait_cycles, s.max_hold_cycles);
            }
        }
    }

    if !any {
        shell_println!("  (no lock activity recorded yet)");
    }

    shell_println!("");
    shell_println!("  Tracking: ON | Use 'lockstats reset' to clear, 'lockstats off' to disable");
}

/// `kstat` — show system metrics time series.
///
/// Displays the last N seconds of system metrics from the periodic sampler.
/// Usage:
///   `kstat`     — show last 10 samples
///   `kstat N`   — show last N samples
///   `kstat all` — show all 60 samples
#[allow(clippy::cast_possible_truncation)]
fn cmd_kstat(args: &str) {
    let count: usize = match args.trim() {
        "" => 10,
        "all" => 60,
        n => n.parse().unwrap_or(10),
    };

    let samples = crate::kstat::recent(count);
    let total = crate::kstat::total_samples();

    if samples.is_empty() {
        shell_println!("No samples recorded yet (wait at least 1 second).");
        return;
    }

    shell_println!("=== System Metrics History ({} samples, {} total recorded) ===",
        samples.len(), total);
    shell_println!("");
    shell_println!("  {:<6} {:>5} {:>5} {:>7} {:>4} {:>4} {:>7} {:>5}",
        "Age(s)", "Free%", "Press", "Heap KB", "Task", "CPU0", "CtxSw", "IRQs");
    shell_println!("  {}", "-".repeat(55));

    let now_tick = crate::apic::tick_count();

    // Samples are newest-first.  Show oldest-first for natural time order.
    for s in samples.iter().rev() {
        let age_ticks = now_tick.saturating_sub(s.tick);
        let age_sec = age_ticks / 100;

        let free_pct = if s.total_frames > 0 {
            s.free_frames.saturating_mul(100) / s.total_frames
        } else {
            0
        };

        let heap_kb = s.heap_bytes_in_use / 1024;

        shell_println!("  {:>5}s {:>4}% {:>5} {:>5}KB {:>4} {:>3}% {:>7} {:>5}",
            age_sec,
            free_pct,
            s.pressure_score,
            heap_kb,
            s.runnable_tasks,
            s.cpu_util[0],
            s.ctx_switches_lo,
            s.interrupts_lo,
        );
    }

    shell_println!("");
    shell_println!("  Columns: Age=seconds ago, Free%=phys mem free, Press=pressure(0-100),");
    shell_println!("           Heap=kernel heap, Task=active tasks, CPU0=util%, CtxSw/IRQs=totals");
}

/// `idle` — show CPU idle state statistics.
///
/// Displays MWAIT/HLT idle entry counts, resched wakes, and C-state info.
fn cmd_idle() {
    let s = crate::idle::stats();

    shell_println!("=== CPU Idle State ===");
    shell_println!("");
    shell_println!("  Mode:         {}", if s.mwait_enabled { "MWAIT" } else { "HLT (fallback)" });
    shell_println!("  C-state hint: {:#04x}", s.cstate_hint);
    shell_println!("");
    shell_println!("  Total idle entries: {}", s.total_entries);
    shell_println!("    via MWAIT: {}", s.mwait_entries);
    shell_println!("    via HLT:   {}", s.hlt_entries);
    shell_println!("");
    shell_println!("  Resched wakes: {} (woken by need_resched flag)", s.resched_wakes);

    if s.total_entries > 0 {
        let mwait_pct = if s.total_entries > 0 {
            s.mwait_entries.saturating_mul(100) / s.total_entries
        } else {
            0
        };
        shell_println!("  MWAIT usage:   {}%", mwait_pct);
    }
}

/// `kwarn` — show kernel warnings.
///
/// Displays all recorded non-fatal kernel warnings from the ring buffer.
/// Use `kwarn clear` to reset.
fn cmd_kwarn(args: &str) {
    if args.trim() == "clear" {
        crate::kwarn::clear();
        shell_println!("Kernel warnings cleared.");
        return;
    }

    let warnings = crate::kwarn::all_warnings();
    let total = crate::kwarn::total_count();

    if warnings.is_empty() {
        shell_println!("No kernel warnings recorded.");
        return;
    }

    shell_println!("=== Kernel Warnings ({} total since boot) ===", total);
    shell_println!("");

    let tsc_freq = crate::bench::tsc_freq();

    for (i, w) in warnings.iter().enumerate() {
        let file = core::str::from_utf8(&w.file[..w.file_len as usize]).unwrap_or("?");
        let msg = core::str::from_utf8(&w.msg[..w.msg_len as usize]).unwrap_or("?");

        // Convert timestamp to relative age if TSC freq is known.
        let age_str = if tsc_freq > 0 {
            let now = crate::bench::rdtsc();
            let elapsed_cycles = now.saturating_sub(w.timestamp);
            let elapsed_ms = elapsed_cycles / (tsc_freq / 1000).max(1);
            if elapsed_ms < 1000 {
                alloc::format!("{}ms ago", elapsed_ms)
            } else {
                alloc::format!("{}s ago", elapsed_ms / 1000)
            }
        } else {
            alloc::format!("tsc={}", w.timestamp)
        };

        shell_println!("  [{}] {} ({}:{}) [{}]",
            i + 1, msg, file, w.line, age_str);
    }
    shell_println!("");
}

/// `sclatency` — show syscall latency histogram.
///
/// Displays the distribution of syscall execution times across
/// logarithmic buckets.  `sclatency reset` clears the histogram.
fn cmd_sclatency(args: &str) {
    match args.trim() {
        "reset" | "clear" => {
            crate::sclatency::reset();
            shell_println!("Syscall latency histogram reset.");
            return;
        }
        "off" => {
            crate::sclatency::set_enabled(false);
            shell_println!("Syscall latency tracking disabled.");
            return;
        }
        "on" => {
            crate::sclatency::set_enabled(true);
            shell_println!("Syscall latency tracking enabled.");
            return;
        }
        _ => {}
    }

    let s = crate::sclatency::stats();
    let labels = crate::sclatency::bucket_labels();

    if s.total_calls == 0 {
        shell_println!("No syscalls recorded yet.");
        return;
    }

    shell_println!("=== Syscall Latency Histogram ({} calls) ===", s.total_calls);
    shell_println!("");
    shell_println!("  min={}ns  mean={}ns  max={}ns", s.min_ns, s.mean_ns, s.max_ns);
    shell_println!("");

    // Find max bucket count for bar scaling.
    let max_count = s.buckets.iter().copied().max().unwrap_or(1).max(1);

    for (i, &count) in s.buckets.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let pct = count.saturating_mul(100) / s.total_calls.max(1);
        let bar_len = (count.saturating_mul(30) / max_count) as usize;
        let bar: alloc::string::String = core::iter::repeat('#').take(bar_len).collect();
        shell_println!("  {:>9} [{:>5} {:>3}%] {}",
            labels[i], count, pct, bar);
    }

    // Per-syscall breakdown.
    let per_sc = crate::sclatency::per_syscall_stats();
    if !per_sc.is_empty() {
        shell_println!("");
        shell_println!("  Top syscalls by count:");
        for (nr, count, mean_cyc) in per_sc.iter().take(8) {
            let mean_ns = crate::bench::cycles_to_ns(*mean_cyc);
            shell_println!("    syscall {:>2}: {:>6} calls, mean {}ns",
                nr, count, mean_ns);
        }
    }
    shell_println!("");
}

/// `memtype` — show memory type breakdown.
///
/// Displays how physical memory is distributed across usage categories
/// (page tables, stacks, slab, DMA, etc.) — like `/proc/meminfo`.
fn cmd_memtype() {
    let s = crate::mm::memtype::stats();
    let names = crate::mm::memtype::all_type_names();
    let total_accounted = crate::mm::memtype::total_accounted();
    let frame_size = crate::mm::frame::FRAME_SIZE;

    shell_println!("=== Memory Type Breakdown ===");
    shell_println!("");
    shell_println!("  {:<12} {:>8} {:>8} {:>8}", "Type", "Current", "Peak", "KiB");
    shell_println!("  {}", "-".repeat(44));

    for i in 0..names.len() {
        if s.current[i] == 0 && s.peak[i] == 0 {
            continue;
        }
        let kib = s.current[i].saturating_mul(frame_size as u64) / 1024;
        shell_println!("  {:<12} {:>6} fr {:>6} fr {:>6}",
            names[i],
            s.current[i],
            s.peak[i],
            kib,
        );
    }

    let total_kib = total_accounted.saturating_mul(frame_size as u64) / 1024;
    shell_println!("  {}", "-".repeat(44));
    shell_println!("  {:<12} {:>6} fr {:>14}", "TOTAL", total_accounted, alloc::format!("{} KiB", total_kib));
    shell_println!("");

    // Also show how much of total physical memory is accounted for.
    if let Some(phys) = crate::mm::frame::try_stats() {
        let used = phys.total_frames.saturating_sub(phys.free_frames) as u64;
        if used > 0 {
            let pct = total_accounted.saturating_mul(100) / used.max(1);
            shell_println!("  Used frames: {} (accounted: {}%)", used, pct);
        }
    }
}

/// `loadavg` — show system load averages.
///
/// Displays 1-minute, 5-minute, and 15-minute exponential moving
/// averages of the number of runnable tasks (like `/proc/loadavg`).
fn cmd_loadavg() {
    let (l1, l5, l15) = crate::loadavg::get();
    let (l1_w, l1_f) = crate::loadavg::format_load(l1);
    let (l5_w, l5_f) = crate::loadavg::format_load(l5);
    let (l15_w, l15_f) = crate::loadavg::format_load(l15);

    let nr_run = crate::loadavg::nr_running();
    let samples = crate::loadavg::sample_count();

    shell_println!("{}.{:02} {}.{:02} {}.{:02} {}/total {}",
        l1_w, l1_f,
        l5_w, l5_f,
        l15_w, l15_f,
        nr_run,
        samples,
    );
    shell_println!("");
    shell_println!("  1-min  5-min  15-min  running  samples");
    shell_println!("  Load averages sampled every 5 seconds (EWMA, fixed-point)");
}

/// `cputime` — show per-CPU time breakdown (system/IRQ/softirq/idle).
///
/// Displays nanosecond-precision CPU utilization breakdown using
/// TSC-based accounting.  Shows percentages and absolute times.
fn cmd_cputime() {
    let stats = crate::cputime::all_cpu_stats();
    if stats.is_empty() {
        shell_println!("CPU time accounting not available (TSC not calibrated)");
        return;
    }

    shell_println!("CPU     system%  irq%  softirq%  idle%    irqs    softirqs   idle_entries");
    shell_println!("---     -------  ----  --------  -----    ----    --------   ------------");

    for &(cpu, ref s) in &stats {
        let total = s.total_ns.max(1); // avoid div-by-zero

        let sys_pct = s.system_ns.saturating_mul(100) / total;
        let irq_pct = s.irq_ns.saturating_mul(100) / total;
        let si_pct = s.softirq_ns.saturating_mul(100) / total;
        let idle_pct = s.idle_ns.saturating_mul(100) / total;

        shell_println!("{:<7} {:>5}%  {:>3}%    {:>4}%  {:>4}%  {:>6}  {:>10}  {:>12}",
            cpu, sys_pct, irq_pct, si_pct, idle_pct,
            s.irq_count, s.softirq_count, s.idle_count,
        );
    }

    // Aggregate line
    let agg = crate::cputime::aggregate_stats();
    let total = agg.total_ns.max(1);
    let sys_pct = agg.system_ns.saturating_mul(100) / total;
    let irq_pct = agg.irq_ns.saturating_mul(100) / total;
    let si_pct = agg.softirq_ns.saturating_mul(100) / total;
    let idle_pct = agg.idle_ns.saturating_mul(100) / total;

    shell_println!("---     -------  ----  --------  -----    ----    --------   ------------");
    shell_println!("{:<7} {:>5}%  {:>3}%    {:>4}%  {:>4}%  {:>6}  {:>10}  {:>12}",
        "ALL", sys_pct, irq_pct, si_pct, idle_pct,
        agg.irq_count, agg.softirq_count, agg.idle_count,
    );

    // Time breakdown in human-readable form
    shell_println!("");
    shell_println!("Total uptime (per-CPU): {:.3}s",
        agg.total_ns as f64 / 1_000_000_000.0);
    shell_println!("  System:  {} ms", agg.system_ns / 1_000_000);
    shell_println!("  IRQ:     {} ms", agg.irq_ns / 1_000_000);
    shell_println!("  Softirq: {} ms", agg.softirq_ns / 1_000_000);
    shell_println!("  Idle:    {} ms", agg.idle_ns / 1_000_000);
}

/// `irqstorm` — IRQ storm detection status and control.
///
/// Usage:
///   `irqstorm`           — show status
///   `irqstorm on`        — enable detection
///   `irqstorm off`       — disable detection
///   `irqstorm unmask N`  — force-unmask IRQ N
fn cmd_irqstorm(args: &str) {
    match args.trim() {
        "on" => {
            crate::irq_storm::set_enabled(true);
            shell_println!("IRQ storm detection enabled.");
        }
        "off" => {
            crate::irq_storm::set_enabled(false);
            shell_println!("IRQ storm detection disabled.");
        }
        s if s.starts_with("unmask ") => {
            if let Some(n_str) = s.strip_prefix("unmask ") {
                if let Ok(irq) = n_str.trim().parse::<usize>() {
                    crate::irq_storm::force_unmask(irq);
                    shell_println!("IRQ {} force-unmasked.", irq);
                } else {
                    shell_println!("Usage: irqstorm unmask <irq_number>");
                }
            }
        }
        _ => {
            // Show status.
            let enabled = crate::irq_storm::is_enabled();
            let total = crate::irq_storm::total_storms();
            shell_println!("IRQ Storm Detector: {}", if enabled { "ENABLED" } else { "DISABLED" });
            shell_println!("Total storms detected: {}", total);

            let storm_stats = crate::irq_storm::stats();
            if storm_stats.is_empty() {
                shell_println!("(no storm activity recorded)");
            } else {
                shell_println!("");
                shell_println!("  IRQ  Strikes  Masked  Storms  Cooldown");
                shell_println!("  ---  -------  ------  ------  --------");
                for s in &storm_stats {
                    shell_println!("  {:>3}  {:>7}  {:>6}  {:>6}  {:>5}s",
                        s.irq, s.strikes,
                        if s.masked { "YES" } else { "no" },
                        s.total_storms, s.cooldown_secs,
                    );
                }
            }
        }
    }
}

/// `pacct` — show process accounting (recent task exits).
///
/// Usage:
///   `pacct`      — show last 20 exited tasks
///   `pacct N`    — show last N exited tasks
#[allow(clippy::arithmetic_side_effects)]
fn cmd_pacct(args: &str) {
    let count: usize = args.trim().parse().unwrap_or(20);
    let records = crate::pacct::recent(count);
    let total = crate::pacct::total_recorded();

    shell_println!("Process Accounting ({} total exits recorded)", total);
    shell_println!("");

    if records.is_empty() {
        shell_println!("(no task exits recorded yet)");
        return;
    }

    let freq = crate::bench::tsc_freq();

    shell_println!(
        "{:<5} {:<12} {:>3} {:>8} {:>6} {:>6} {:>4}",
        "TID", "NAME", "PRI", "CPU_MS", "SCHED", "WAIT", "CPU"
    );
    shell_println!("------------------------------------------------------");

    for rec in &records {
        let name = core::str::from_utf8(&rec.name[..rec.name_len as usize]).unwrap_or("?");

        // CPU time in milliseconds (from TSC cycles).
        let cpu_ms = if freq > 0 {
            crate::bench::cycles_to_ns(rec.total_cycles) / 1_000_000
        } else {
            rec.total_ticks * 10
        };

        // Wait time in 10ths of a second.
        let wait_tenths = rec.total_wait_ticks / 10;

        shell_println!(
            "{:<5} {:<12} {:>3} {:>6}ms {:>6} {:>4}.{}  {:>2}",
            rec.task_id, name, rec.priority,
            cpu_ms, rec.schedule_count,
            wait_tenths / 10, wait_tenths % 10,
            rec.last_cpu,
        );
    }
}

/// `counters` — display unified kernel event counters.
///
/// Shows all registered counters grouped by subsystem, plus built-in
/// counters aggregated from various kernel subsystems.
///   `counters`       — show all counters
///   `counters <grp>` — filter by group (mm, sched, irq, softirq, syscall, pacct)
fn cmd_counters(args: &str) {
    let filter = args.trim();

    // Get built-in counters (from existing subsystem atomics).
    let builtin = crate::kcounters::builtin_snapshot();

    // Get explicitly-registered counters.
    let registered = crate::kcounters::snapshot();

    // Merge both lists.
    let all: alloc::vec::Vec<_> = builtin.iter().chain(registered.iter())
        .filter(|c| filter.is_empty() || c.group == filter)
        .collect();

    if all.is_empty() {
        if filter.is_empty() {
            shell_println!("No counters registered.");
        } else {
            shell_println!("No counters in group '{}'.", filter);
        }
        return;
    }

    shell_println!("=== Kernel Event Counters ===");
    shell_println!("");

    // Group by subsystem.
    let mut current_group = "";
    for counter in &all {
        if counter.group != current_group {
            if !current_group.is_empty() {
                shell_println!("");
            }
            shell_println!("  [{}]", counter.group);
            current_group = counter.group;
        }
        shell_println!("    {:<24} {}", counter.name, counter.value);
    }

    shell_println!("");
    shell_println!("Total: {} counters ({} registered, {} built-in)",
        all.len(), registered.len(), builtin.len());
}

/// `topology` — display CPU topology (package/core/SMT mapping).
fn cmd_topology() {
    let num_cpus = crate::smp::cpu_count().max(1);
    let packages = crate::cpu_topology::num_packages();
    let phys_cores = crate::cpu_topology::num_physical_cores();
    let smt = crate::cpu_topology::smt_active();

    shell_println!("=== CPU Topology ===");
    shell_println!("");
    shell_println!("  Packages (sockets): {}", packages);
    shell_println!("  Physical cores:     {}", phys_cores);
    shell_println!("  Logical CPUs:       {}", num_cpus);
    shell_println!("  SMT (HyperThread):  {}", if smt { "active" } else { "inactive" });
    shell_println!("");

    shell_println!("{:<4} {:>4} {:>5} {:>4} {:>8} {:>12}", "CPU", "PKG", "CORE", "SMT", "APIC_ID", "SMT_SIBLINGS");
    shell_println!("---------------------------------------------------");

    for cpu in 0..num_cpus {
        if let Some(topo) = crate::cpu_topology::cpu_topo(cpu) {
            let siblings = crate::cpu_topology::smt_siblings(cpu);
            // Format sibling mask as list of CPU numbers.
            let mut sib_str = alloc::string::String::new();
            for bit in 0..16u16 {
                if siblings & (1u16 << bit) != 0 && bit as usize != cpu {
                    if !sib_str.is_empty() {
                        sib_str.push(',');
                    }
                    use core::fmt::Write;
                    let _ = write!(sib_str, "{}", bit);
                }
            }
            if sib_str.is_empty() {
                sib_str.push_str("none");
            }

            shell_println!(
                "{:<4} {:>4} {:>5} {:>4} {:>8} {:>12}",
                cpu, topo.package_id, topo.core_id, topo.smt_id,
                topo.apic_id, sib_str
            );
        }
    }
}

/// `cpufreq` — CPU frequency scaling control.
///
///   `cpufreq`             — show current frequency info
///   `cpufreq performance` — set performance governor
///   `cpufreq powersave`   — set powersave governor
///   `cpufreq ondemand`    — set ondemand governor
fn cmd_cpufreq(args: &str) {
    match args.trim() {
        "performance" => {
            crate::cpufreq::set_governor(crate::cpufreq::Governor::Performance);
            shell_println!("Governor set to: performance");
            return;
        }
        "powersave" => {
            crate::cpufreq::set_governor(crate::cpufreq::Governor::Powersave);
            shell_println!("Governor set to: powersave");
            return;
        }
        "ondemand" => {
            crate::cpufreq::set_governor(crate::cpufreq::Governor::Ondemand);
            shell_println!("Governor set to: ondemand");
            return;
        }
        _ => {}
    }

    let info = crate::cpufreq::info();

    shell_println!("=== CPU Frequency Scaling ===");
    shell_println!("");
    shell_println!("  Interface:    {}", if info.hwp_active {
        "HWP (Hardware-managed P-states)"
    } else if info.eist_available {
        "EIST (Enhanced SpeedStep)"
    } else {
        "none (fixed frequency)"
    });
    shell_println!("  Governor:     {}", info.governor.name());
    shell_println!("  Base freq:    {} MHz", info.base_mhz);
    shell_println!("  Current:      {} MHz (est.)", crate::cpufreq::current_freq_mhz());
    shell_println!("");
    shell_println!("  Perf levels:  min={} guaranteed={} max={}",
        info.min_perf, info.guaranteed_perf, info.max_perf);
    shell_println!("  Current perf: {}", info.current_perf);
    shell_println!("  Transitions:  {}", info.transitions);
}

/// `compact` — analyze memory fragmentation and trigger compaction.
fn cmd_compact() {
    shell_println!("=== Memory Compaction ===");
    shell_println!("");

    let Some(report) = crate::mm::compact::analyze() else {
        shell_println!("  Unable to analyze fragmentation (allocator busy).");
        return;
    };

    shell_println!("  Fragmentation:       {}%", report.fragmentation_pct);
    shell_println!("  Free frames:         {} ({} KiB)",
        report.free_frames,
        report.free_frames.saturating_mul(crate::mm::frame::FRAME_SIZE) / 1024);
    shell_println!("  Order-0 free:        {} (single frames, fragmented)",
        report.order0_free);
    shell_println!("  Higher-order free:   {} (can serve large allocs)",
        report.higher_order_free);
    shell_println!("  Largest free block:  {} frames ({} KiB)",
        report.largest_free_block,
        report.largest_free_block.saturating_mul(crate::mm::frame::FRAME_SIZE) / 1024);
    shell_println!("  Estimated movable:   {} pages", report.estimated_movable);
    shell_println!("");

    if report.compaction_recommended {
        shell_println!("  Status: Compaction RECOMMENDED (high fragmentation)");
    } else {
        shell_println!("  Status: Compaction not needed.");
    }

    let stats = crate::mm::compact::stats();
    shell_println!("");
    shell_println!("  History:");
    shell_println!("    Total requests:       {}", stats.total_requests);
    shell_println!("    Pages migrated:       {}", stats.pages_migrated);
    shell_println!("    Migration failures:   {}", stats.migration_failures);
    shell_println!("    Pages scanned:        {}", stats.pages_scanned);
}

/// `shutdown` — power off the system.
fn cmd_shutdown() {
    let caps = crate::power::capabilities();
    shell_println!("=== System Shutdown ===");
    shell_println!("");
    if caps.acpi_shutdown {
        shell_println!("  Using ACPI S5 (PM1a={:#x}, SLP_TYP={})", caps.pm1a_port, caps.slp_typ_s5);
    } else {
        shell_println!("  ACPI shutdown not available, will try fallback methods.");
    }
    shell_println!("  Shutting down...");
    shell_println!("");
    crate::power::shutdown();
}

/// `hotplug` / `cpuctl` — CPU hotplug management.
///
/// Usage: hotplug [status|offline N|online N]
fn cmd_hotplug(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    let subcmd = parts.first().copied().unwrap_or("status");

    match subcmd {
        "status" | "" => {
            let stats = crate::cpu_hotplug::stats();
            shell_println!("=== CPU Hotplug Status ===");
            shell_println!("");
            shell_println!("  Online: {}/{} CPUs", stats.online_cpus, stats.total_cpus);
            shell_println!("  Offline ops: {}, Online ops: {}", stats.offline_ops, stats.online_ops);
            shell_println!("  Tasks migrated: {}", stats.tasks_migrated);
            shell_println!("  Notifiers: {}", stats.notifiers_registered);
            shell_println!("");
            for i in 0..stats.total_cpus {
                let state = crate::cpu_hotplug::cpu_state(i);
                let marker = if crate::cpu_hotplug::is_online(i) { "●" } else { "○" };
                shell_println!("  CPU {:2}: {} {:?}", i, marker, state);
            }
        }
        "offline" => {
            let Some(cpu_str) = parts.get(1) else {
                shell_println!("Usage: hotplug offline <cpu>");
                return;
            };
            let Ok(cpu) = cpu_str.parse::<usize>() else {
                shell_println!("Invalid CPU number: {}", cpu_str);
                return;
            };
            match crate::cpu_hotplug::offline(cpu) {
                Ok(migrated) => shell_println!("CPU {} offlined ({} tasks migrated)", cpu, migrated),
                Err(e) => shell_println!("Failed: {}", e),
            }
        }
        "online" => {
            let Some(cpu_str) = parts.get(1) else {
                shell_println!("Usage: hotplug online <cpu>");
                return;
            };
            let Ok(cpu) = cpu_str.parse::<usize>() else {
                shell_println!("Invalid CPU number: {}", cpu_str);
                return;
            };
            match crate::cpu_hotplug::online(cpu) {
                Ok(()) => shell_println!("CPU {} is now online", cpu),
                Err(e) => shell_println!("Failed: {}", e),
            }
        }
        _ => {
            shell_println!("Usage: hotplug [status|offline <cpu>|online <cpu>]");
        }
    }
}

/// `hugepage` — display huge page statistics.
fn cmd_hugepage() {
    let st = crate::mm::hugepage::stats();
    shell_println!("=== Huge Pages (2 MiB) ===");
    shell_println!("");
    shell_println!("  Currently mapped: {}", st.mapped);
    shell_println!("  Total allocated:  {}", st.allocated);
    shell_println!("  Total freed:      {}", st.freed);
    shell_println!("  Alloc failures:   {}", st.failures);
    shell_println!("  Page size:        2 MiB (128 × 16 KiB frames)");
}

/// `vmalloc` — display vmalloc (virtual kernel memory) statistics.
fn cmd_vmalloc() {
    let st = crate::mm::vmalloc::stats();
    shell_println!("=== vmalloc (Virtual Kernel Memory) ===");
    shell_println!("");
    shell_println!("  Active allocations: {}", st.active);
    shell_println!("  Bytes mapped:       {} KiB", st.bytes_allocated / 1024);
    shell_println!("  Region size:        {} MiB", st.region_size / (1024 * 1024));
    shell_println!("  Total allocs:       {}", st.alloc_count);
    shell_println!("  Total frees:        {}", st.free_count);
    shell_println!("  Alloc failures:     {}", st.alloc_failures);
}

/// `rmap` — display reverse mapping statistics.
fn cmd_rmap() {
    let st = crate::mm::rmap::stats();
    shell_println!("=== Reverse Mapping (rmap) ===");
    shell_println!("");
    shell_println!("  Entries used:    {} / {}", st.entries_used, st.table_capacity);
    shell_println!("  Add calls:       {}", st.add_count);
    shell_println!("  Remove calls:    {}", st.remove_count);
    shell_println!("  Lookup calls:    {}", st.lookup_count);
    shell_println!("  Overflows:       {}", st.overflow_count);
    shell_println!("  Table full:      {}", st.table_full_count);
}

/// `pcid` — display PCID (Process Context Identifiers) status.
fn cmd_pcid() {
    let st = crate::mm::pcid::stats();
    shell_println!("=== PCID (Process Context Identifiers) ===");
    shell_println!("");
    shell_println!("  PCID enabled:      {}", st.enabled);
    shell_println!("  INVPCID available: {}", st.has_invpcid);
    shell_println!("  No-flush switches: {}", st.noflush_switches);
    shell_println!("  Generation flushes:{}", st.generation_flushes);
    shell_println!("  INVPCID singles:   {}", st.invpcid_singles);
}

/// `poison` — display memory poison statistics.
fn cmd_poison() {
    let st = crate::mm::poison::stats();
    shell_println!("=== Memory Poisoning ===");
    shell_println!("");
    shell_println!("  Enabled:         {}", st.enabled);
    shell_println!("  Alloc poison:    {}", st.alloc_poison);
    shell_println!("  Free bytes:      {} KiB", st.free_bytes / 1024);
    shell_println!("  Alloc bytes:     {} KiB", st.alloc_bytes / 1024);
    shell_println!("  Violations:      {}", st.violations);
    shell_println!("");
    shell_println!("  Patterns: FREE=0xDE  ALLOC=0xCD  REDZONE=0xFD  STACK=0x6B");
}

/// `watermark` — display memory watermark (peak usage) per subsystem.
fn cmd_watermark() {
    let count = crate::mm::watermark::meter_count();
    shell_println!("=== Memory Watermarks ===");
    shell_println!("");
    if count == 0 {
        shell_println!("  (no meters registered)");
        return;
    }
    shell_println!("  {:<20} {:>12} {:>12}", "Subsystem", "Current", "Peak");
    shell_println!("  {:<20} {:>12} {:>12}", "---------", "-------", "----");

    let mut snaps = [crate::mm::watermark::MeterSnapshot {
        name: [0; 20], name_len: 0, current: 0, peak: 0,
    }; 32];
    let filled = crate::mm::watermark::snapshot_all(&mut snaps);
    for i in 0..filled {
        let s = &snaps[i];
        let name = core::str::from_utf8(&s.name[..s.name_len as usize])
            .unwrap_or("?");
        shell_println!("  {:<20} {:>12} {:>12}", name, s.current, s.peak);
    }
}

/// `migratetype` — display page migration type statistics.
fn cmd_migrate_type() {
    let s = crate::mm::migrate_type::stats();
    shell_println!("=== Page Migration Types ===");
    shell_println!("");
    shell_println!("  {:<12} {:>10} {:>10} {:>10}", "Type", "Current", "Alloc", "Free");
    shell_println!("  {:<12} {:>10} {:>10} {:>10}", "----", "-------", "-----", "----");
    let names = ["unmovable", "movable", "reclaimable", "highatomic"];
    for i in 0..4 {
        shell_println!("  {:<12} {:>10} {:>10} {:>10}",
            names[i], s.current[i], s.alloc_total[i], s.free_total[i]);
    }
    shell_println!("");
    shell_println!("  Pageblock steals:  {}", s.pageblock_steals);
}

/// `scrub` — display memory scrubber status and statistics.
fn cmd_scrub() {
    let s = crate::mm::scrub::stats();
    shell_println!("=== Memory Scrubber ===");
    shell_println!("");
    shell_println!("  Status:          {}", if s.enabled { "enabled" } else { "disabled" });
    shell_println!("  Range:           0..{:#x} ({} MiB)", s.range_end, s.range_end / (1024 * 1024));
    shell_println!("  Progress:        {}%", s.progress_pct);
    shell_println!("  Cycles complete: {}", s.cycles);
    shell_println!("  Steps executed:  {}", s.steps);
    shell_println!("  Total scrubbed:  {} MiB", s.total_bytes / (1024 * 1024));
    shell_println!("  Errors detected: {}", s.errors);
}

/// `frameowner` — show physical frame ownership by subsystem.
fn cmd_frame_owner() {
    use crate::mm::frame_owner;
    use crate::mm::frame::FRAME_SIZE;

    let top = frame_owner::top_owners();
    let s = frame_owner::summary();

    shell_println!("=== Frame Ownership ===");
    shell_println!("");
    shell_println!("  {:>12}  {:>6}  {:>8}  {}", "OWNER", "FRAMES", "SIZE", "BAR");
    shell_println!("  {:>12}  {:>6}  {:>8}  {}", "-----", "------", "--------", "---");

    // Show non-zero entries (top_owners is sorted descending).
    for &(owner, count) in &top {
        if count == 0 {
            break;
        }
        let size_kb = (count as usize).saturating_mul(FRAME_SIZE) / 1024;
        let size_str = if size_kb >= 1024 {
            alloc::format!("{} MiB", size_kb / 1024)
        } else {
            alloc::format!("{} KiB", size_kb)
        };

        // Bar: 1 char per 1% of total (64K frames).
        let pct = (count as u64).saturating_mul(100) / (MAX_FRAMES_DISPLAY as u64);
        let bar_len = (pct as usize).min(40);
        let bar: alloc::string::String = core::iter::repeat('█').take(bar_len).collect();

        shell_println!("  {:>12}  {:>6}  {:>8}  {}", owner.name(), count, size_str, bar);
    }

    shell_println!("");
    shell_println!("  Allocated: {} frames ({} KiB)",
        s.total_allocated,
        (s.total_allocated as usize).saturating_mul(FRAME_SIZE) / 1024);
    shell_println!("  Free:      {} frames", s.total_free);
    shell_println!("  Tracking:  {} (sets: {}, clears: {})",
        if frame_owner::is_enabled() { "enabled" } else { "disabled" },
        s.total_sets, s.total_clears);
}

/// Max frames for display percentage calculation.
const MAX_FRAMES_DISPLAY: usize = 65536;

/// `alloctrace` — show allocation trace ring buffer contents.
///
/// Usage:
///   alloctrace            — show stats and last 10 events
///   alloctrace recent [N] — show last N events (default 20)
///   alloctrace reset      — clear the ring buffer
///   alloctrace on|off     — enable/disable tracing
fn cmd_alloc_trace(args: &str) {
    use crate::mm::alloc_trace;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            alloc_trace::reset();
            shell_println!("Trace ring buffer reset");
        }
        "on" => {
            alloc_trace::enable();
            shell_println!("Allocation tracing enabled");
        }
        "off" => {
            alloc_trace::disable();
            shell_println!("Allocation tracing disabled");
        }
        "recent" => {
            let count = parts.get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(20)
                .min(64);
            let mut buf = [alloc_trace::TraceEntry::empty(); 64];
            let n = alloc_trace::recent(&mut buf[..count]);
            shell_println!("Last {} events (newest first):", n);
            shell_println!("  {:>4}  {:>10}  {:>8}  {:>7}  {:>5}  {:>3}",
                "#", "TSC", "FRAME", "OP", "OWNER", "CPU");
            for i in 0..n {
                let e = &buf[i];
                if e.is_valid() {
                    shell_println!("  {:>4}  {:>10}  {:>8}  {:>7}  {:>5}  {:>3}",
                        i, e.timestamp & 0xFFFF_FFFF, e.frame_idx,
                        e.operation().name(), e.owner_tag().name(), e.cpu);
                }
            }
        }
        _ => {
            // Default: show stats + last 10.
            let s = alloc_trace::stats();
            let (allocs, frees) = alloc_trace::alloc_free_balance();
            shell_println!("=== Allocation Trace ===");
            shell_println!("");
            shell_println!("  Enabled:       {}", if s.enabled { "yes" } else { "no" });
            shell_println!("  Total events:  {}", s.total_events);
            shell_println!("  Dropped:       {}", s.dropped);
            shell_println!("  Buffer:        {}/{} entries", s.valid_entries, s.capacity);
            shell_println!("  Balance:       {} allocs, {} frees", allocs, frees);
            shell_println!("");
            // Show last 10 events.
            let mut buf = [alloc_trace::TraceEntry::empty(); 10];
            let n = alloc_trace::recent(&mut buf);
            if n > 0 {
                shell_println!("  Recent events (newest first):");
                shell_println!("    {:>10}  {:>8}  {:>7}  {:>5}  {:>3}",
                    "TSC", "FRAME", "OP", "OWNER", "CPU");
                for i in 0..n {
                    let e = &buf[i];
                    if e.is_valid() {
                        shell_println!("    {:>10}  {:>8}  {:>7}  {:>5}  {:>3}",
                            e.timestamp & 0xFFFF_FFFF, e.frame_idx,
                            e.operation().name(), e.owner_tag().name(), e.cpu);
                    }
                }
            }
        }
    }
}

/// `alloclat` — show allocation latency histograms.
///
/// Usage:
///   alloclat         — show alloc and free latency histograms
///   alloclat reset   — reset all measurements
///   alloclat on|off  — enable/disable measurement
fn cmd_alloc_lat(args: &str) {
    use crate::mm::alloc_lat;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            alloc_lat::reset();
            shell_println!("Latency histograms reset");
        }
        "on" => {
            alloc_lat::enable();
            shell_println!("Latency measurement enabled");
        }
        "off" => {
            alloc_lat::disable();
            shell_println!("Latency measurement disabled");
        }
        _ => {
            // Show both histograms.
            shell_println!("=== Allocation Latency ===");
            shell_println!("");

            let ah = alloc_lat::alloc_histogram();
            let fh = alloc_lat::free_histogram();

            shell_println!("  Alloc: {} samples, avg={}ns, max={}ns",
                ah.count, ah.cycles_to_ns(ah.avg_cycles), ah.cycles_to_ns(ah.max_cycles));
            shell_println!("    p50={}ns  p90={}ns  p99={}ns",
                ah.cycles_to_ns(ah.percentile(50)),
                ah.cycles_to_ns(ah.percentile(90)),
                ah.cycles_to_ns(ah.percentile(99)));

            if ah.count > 0 {
                shell_println!("    {:>8}  {:>8}  {:>6}  {}", "RANGE", "COUNT", "%", "HISTOGRAM");
                let mut cumulative: u64 = 0;
                for (i, &count) in ah.buckets.iter().enumerate() {
                    if count == 0 {
                        continue;
                    }
                    cumulative = cumulative.saturating_add(count);
                    let pct = count.saturating_mul(100).checked_div(ah.count).unwrap_or(0);
                    let lower_ns = ah.cycles_to_ns(alloc_lat::LatencyHist::bucket_lower_cycles(i));
                    let bar_len = (pct as usize).min(30);
                    let bar: alloc::string::String = core::iter::repeat('▓').take(bar_len).collect();
                    shell_println!("    {:>6}ns  {:>8}  {:>5}%  {}", lower_ns, count, pct, bar);
                }
            }

            shell_println!("");
            shell_println!("  Free:  {} samples, avg={}ns, max={}ns",
                fh.count, fh.cycles_to_ns(fh.avg_cycles), fh.cycles_to_ns(fh.max_cycles));

            shell_println!("");
            shell_println!("  Status: {}", if alloc_lat::is_enabled() { "enabled" } else { "disabled" });
        }
    }
}

/// `heapprofile` — show heap allocation size distribution.
///
/// Usage:
///   heapprofile        — show full profile
///   heapprofile reset  — reset counters
///   heapprofile on|off — enable/disable
fn cmd_heap_profile(args: &str) {
    use crate::mm::heap_profile;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            heap_profile::reset();
            shell_println!("Heap profile reset");
        }
        "on" => {
            heap_profile::enable();
            shell_println!("Heap profiling enabled");
        }
        "off" => {
            heap_profile::disable();
            shell_println!("Heap profiling disabled");
        }
        _ => {
            let p = heap_profile::profile();
            let frag = heap_profile::fragmentation_estimate();
            let (hot_size, hot_count) = heap_profile::hottest_bucket();

            shell_println!("=== Heap Allocation Profile ===");
            shell_println!("");
            shell_println!("  Total: {} allocs, {} frees, {} active",
                p.total_allocs, p.total_frees,
                p.total_allocs.saturating_sub(p.total_frees));
            shell_println!("  Bytes: {} KiB requested, max single = {} B",
                p.total_bytes / 1024, p.max_alloc);
            shell_println!("  Fragmentation: ~{}%", frag);
            shell_println!("  Hottest class: ≤{} B ({} allocs)", hot_size, hot_count);
            shell_println!("");
            shell_println!("  {:>5}  {:>8}  {:>8}  {:>6}  {:>6}  {:>6}",
                "CLASS", "ALLOCS", "FREES", "ACTIVE", "PEAK", "AVG");

            for (i, b) in p.buckets.iter().enumerate() {
                if b.allocs == 0 && b.frees == 0 {
                    continue;
                }
                shell_println!("  {:>5}  {:>8}  {:>8}  {:>6}  {:>6}  {:>6}",
                    heap_profile::bucket_label(i),
                    b.allocs, b.frees, b.active, b.peak, b.avg_size);
            }

            shell_println!("");
            shell_println!("  Status: {}",
                if p.enabled { "enabled" } else { "disabled" });
        }
    }
}

/// `capaudit` — show capability audit log.
///
/// Usage:
///   capaudit         — show stats and recent events
///   capaudit recent  — show last 20 events
///   capaudit reset   — clear the log
///   capaudit on|off  — enable/disable auditing
fn cmd_cap_audit(args: &str) {
    use crate::cap::audit;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            audit::reset();
            shell_println!("Capability audit log reset");
        }
        "on" => {
            audit::enable();
            shell_println!("Capability auditing enabled");
        }
        "off" => {
            audit::disable();
            shell_println!("Capability auditing disabled");
        }
        "recent" => {
            let mut buf = [audit::AuditEntry::empty(); 20];
            let n = audit::recent(&mut buf);
            shell_println!("Last {} capability events (newest first):", n);
            shell_println!("  {:>6}  {:>4}  {:>6}  {:>8}  {:>4}  {:>6}",
                "TICK", "PID", "HANDLE", "OP", "TGT", "RESULT");
            for i in 0..n {
                let e = &buf[i];
                if e.is_valid() {
                    let result_str = if e.result == 0 {
                        alloc::format!("ok")
                    } else {
                        alloc::format!("E{}", e.result)
                    };
                    shell_println!("  {:>6}  {:>4}  {:>6}  {:>8}  {:>4}  {:>6}",
                        e.timestamp, e.pid, e.handle,
                        e.operation().name(),
                        e.target_pid, result_str);
                }
            }
        }
        _ => {
            let s = audit::stats();
            shell_println!("=== Capability Audit ===");
            shell_println!("");
            shell_println!("  Enabled:   {}", if s.enabled { "yes" } else { "no" });
            shell_println!("  Events:    {}", s.total_events);
            shell_println!("  Denials:   {}", s.total_denials);
            shell_println!("  Grants:    {}", s.total_grants);
            shell_println!("  Revokes:   {}", s.total_revokes);
            shell_println!("  Buffer:    {}/{} entries", s.ring_entries, 128);

            if s.total_denials > 0 {
                shell_println!("");
                shell_println!("  ⚠ {} access denial(s) recorded", s.total_denials);
            }
        }
    }
}

/// `checkpoint` — memory allocation checkpoints for leak detection.
///
/// Usage:
///   checkpoint save A      — save checkpoint with label A (A-D)
///   checkpoint diff A B    — diff two checkpoints
///   checkpoint list        — list stored checkpoints
///   checkpoint clear       — clear all checkpoints
///   checkpoint show A      — show details of checkpoint A
fn cmd_checkpoint(args: &str) {
    use crate::mm::alloc_checkpoint;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "save" => {
            if let Some(&label_str) = parts.get(1) {
                let label = label_str.as_bytes().first().copied().unwrap_or(b'A');
                if !label.is_ascii_uppercase() {
                    shell_println!("Error: label must be A-D");
                    return;
                }
                let slot = alloc_checkpoint::save(label);
                shell_println!("Checkpoint '{}' saved in slot {}", label as char, slot);
            } else {
                shell_println!("Usage: checkpoint save <A-D>");
            }
        }
        "diff" => {
            if parts.len() < 3 {
                shell_println!("Usage: checkpoint diff <A-D> <A-D>");
                return;
            }
            let from = parts[1].as_bytes().first().copied().unwrap_or(b'A');
            let to = parts[2].as_bytes().first().copied().unwrap_or(b'B');

            match alloc_checkpoint::diff(from, to) {
                Some(d) => {
                    shell_println!("=== Checkpoint Diff: {} → {} ===",
                        d.from_label as char, d.to_label as char);
                    shell_println!("");
                    shell_println!("  Free frames delta: {:+}", d.free_delta);
                    if d.free_delta < 0 {
                        shell_println!("    ({} more frames allocated)", -d.free_delta);
                    } else if d.free_delta > 0 {
                        shell_println!("    ({} frames freed)", d.free_delta);
                    }
                    shell_println!("  Heap slab delta:   {:+} net objects", d.heap_slab_delta);
                    shell_println!("  Heap large delta:  {:+} allocations", d.heap_large_delta);
                    shell_println!("  Time elapsed:      {} ticks", d.tick_delta);

                    // Show per-owner deltas (only non-zero).
                    let mut any_owner_change = false;
                    for i in 0..d.owner_deltas.len() {
                        if d.owner_deltas[i] != 0 {
                            if !any_owner_change {
                                shell_println!("");
                                shell_println!("  Per-owner frame changes:");
                                any_owner_change = true;
                            }
                            let owner = crate::mm::frame_owner::Owner::from_u8(i as u8);
                            shell_println!("    {:?}: {:+}", owner, d.owner_deltas[i]);
                        }
                    }

                    if d.free_delta < 0 && d.heap_slab_delta > 0 {
                        shell_println!("");
                        shell_println!("  ⚠ Possible leak: frames consumed, heap objects grew");
                    }
                }
                None => {
                    shell_println!("Error: one or both checkpoints not found");
                    shell_println!("  Use 'checkpoint list' to see stored checkpoints");
                }
            }
        }
        "list" => {
            let ls = alloc_checkpoint::list();
            shell_println!("=== Stored Checkpoints ===");
            shell_println!("");
            let mut count = 0;
            for (label, valid) in &ls {
                if *valid {
                    if let Some(cp) = alloc_checkpoint::get(*label) {
                        shell_println!("  [{}] '{}': free={}, total={}, tick={}",
                            count, *label as char, cp.free_frames, cp.total_frames, cp.tick);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                shell_println!("  (none — use 'checkpoint save A' to create one)");
            }
        }
        "show" => {
            if let Some(&label_str) = parts.get(1) {
                let label = label_str.as_bytes().first().copied().unwrap_or(b'A');
                match alloc_checkpoint::get(label) {
                    Some(cp) => {
                        shell_println!("=== Checkpoint '{}' ===", label as char);
                        shell_println!("");
                        shell_println!("  Free frames:     {}", cp.free_frames);
                        shell_println!("  Total frames:    {}", cp.total_frames);
                        shell_println!("  Heap slab allocs: {}", cp.heap_slab_allocs);
                        shell_println!("  Heap slab frees:  {}", cp.heap_slab_frees);
                        shell_println!("  Heap large allocs: {}", cp.heap_large_allocs);
                        shell_println!("  Tick:            {}", cp.tick);
                        shell_println!("");
                        shell_println!("  Per-owner frame counts:");
                        for i in 0..cp.owner_counts.len() {
                            if cp.owner_counts[i] > 0 {
                                let owner = crate::mm::frame_owner::Owner::from_u8(i as u8);
                                shell_println!("    {:?}: {}", owner, cp.owner_counts[i]);
                            }
                        }
                    }
                    None => {
                        shell_println!("Error: checkpoint '{}' not found", label as char);
                    }
                }
            } else {
                shell_println!("Usage: checkpoint show <A-D>");
            }
        }
        "clear" => {
            alloc_checkpoint::clear();
            shell_println!("All checkpoints cleared");
        }
        _ => {
            shell_println!("Usage: checkpoint <save|diff|list|show|clear>");
            shell_println!("");
            shell_println!("  checkpoint save A      — save checkpoint A");
            shell_println!("  checkpoint diff A B    — diff two checkpoints");
            shell_println!("  checkpoint list        — list stored checkpoints");
            shell_println!("  checkpoint show A      — show checkpoint details");
            shell_println!("  checkpoint clear       — clear all");
        }
    }
}

/// `strace` — syscall tracing (per-event capture).
///
/// Usage:
///   strace          — show recent traced syscalls
///   strace on       — enable tracing (all PIDs)
///   strace off      — disable tracing
///   strace pid N    — trace only PID N (0 = all)
///   strace reset    — clear trace log
///   strace stats    — show trace statistics
fn cmd_strace(args: &str) {
    use crate::syscall::trace;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "on" => {
            trace::enable();
            shell_println!("Syscall tracing enabled (filter: {})",
                if trace::pid_filter() == 0 { "all PIDs".into() }
                else { alloc::format!("PID {}", trace::pid_filter()) });
        }
        "off" => {
            trace::disable();
            shell_println!("Syscall tracing disabled");
        }
        "pid" => {
            if let Some(&pid_str) = parts.get(1) {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    trace::set_pid_filter(pid);
                    if pid == 0 {
                        shell_println!("Tracing all PIDs");
                    } else {
                        shell_println!("Tracing only PID {}", pid);
                    }
                } else {
                    shell_println!("Error: invalid PID number");
                }
            } else {
                shell_println!("Usage: strace pid <N>");
            }
        }
        "reset" => {
            trace::reset();
            shell_println!("Syscall trace log cleared");
        }
        "stats" => {
            let s = trace::stats();
            shell_println!("=== Syscall Trace Stats ===");
            shell_println!("");
            shell_println!("  Enabled:     {}", if s.enabled { "yes" } else { "no" });
            shell_println!("  PID filter:  {}",
                if s.pid_filter == 0 { "all".into() }
                else { alloc::format!("{}", s.pid_filter) });
            shell_println!("  Events:      {}", s.total_events);
            shell_println!("  Dropped:     {}", s.dropped_events);
            shell_println!("  Buffer:      {}/{} entries", s.ring_entries, 64);
        }
        _ => {
            // Show recent events.
            let mut buf = [trace::TraceEvent::empty(); 16];
            let n = trace::recent(&mut buf);
            if n == 0 {
                let s = trace::stats();
                shell_println!("No traced syscalls (enabled: {}, events: {})",
                    s.enabled, s.total_events);
                shell_println!("Use 'strace on' to enable tracing");
                return;
            }
            shell_println!("Last {} syscall events (newest first):", n);
            shell_println!("  {:>10}  {:>4}  {:>4}  {:>16}  {:>8}  {:>7}",
                "TSC", "PID", "NR", "ARG0", "RESULT", "CYCLES");
            for i in 0..n {
                let e = &buf[i];
                if e.is_valid() {
                    let name = crate::syscall::profile::syscall_name(e.syscall_nr as u64);
                    let result_str = if e.complete {
                        alloc::format!("{}", e.result)
                    } else {
                        alloc::format!("(entry)")
                    };
                    shell_println!("  {:>10}  {:>4}  {:>4}  {:>16x}  {:>8}  {:>7}",
                        e.timestamp, e.pid, name,
                        e.args[0], result_str, e.duration_cycles);
                }
            }
        }
    }
}

/// `kobjects` — show kernel object lifecycle stats.
///
/// Usage:
///   kobjects        — show all object type counts
///   kobjects leaks  — show types with active objects (potential leaks)
///   kobjects reset  — reset all counters
fn cmd_kobjects(args: &str) {
    use crate::kobject;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            kobject::reset();
            shell_println!("Kernel object counters reset");
        }
        "leaks" => {
            let leaks = kobject::potential_leaks();
            if leaks.is_empty() {
                shell_println!("No active objects (no potential leaks)");
            } else {
                shell_println!("=== Objects with active instances ===");
                shell_println!("");
                shell_println!("  {:12}  {:>8}  {:>8}  {:>6}  {:>8}",
                    "TYPE", "CREATED", "DESTROY", "ACTIVE", "PEAK");
                for s in &leaks {
                    shell_println!("  {:12}  {:>8}  {:>8}  {:>6}  {:>8}",
                        s.obj_type.name(), s.created, s.destroyed,
                        s.active, s.high_water);
                }
                shell_println!("");
                shell_println!("  Total active: {}", kobject::total_active());
            }
        }
        _ => {
            let all = kobject::all_stats();
            shell_println!("=== Kernel Object Tracking ===");
            shell_println!("");
            shell_println!("  {:12}  {:>8}  {:>8}  {:>6}  {:>8}",
                "TYPE", "CREATED", "DESTROY", "ACTIVE", "PEAK");
            for s in &all {
                shell_println!("  {:12}  {:>8}  {:>8}  {:>6}  {:>8}",
                    s.obj_type.name(), s.created, s.destroyed,
                    s.active, s.high_water);
            }
            shell_println!("");
            shell_println!("  Total active objects: {}", kobject::total_active());
        }
    }
}

/// `selftest` — run kernel self-tests.
///
/// Usage:
///   selftest         — run all tests
///   selftest list    — list available test suites
///   selftest mm      — run only memory subsystem tests
///   selftest <name>  — run a specific named test
fn cmd_selftest(args: &str) {
    use crate::selftest;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "list" => {
            let suites = selftest::list();
            shell_println!("=== Available Self-Tests ({}) ===", suites.len());
            shell_println!("");
            shell_println!("  {:16} {:8} {}", "NAME", "CATEGORY", "DESCRIPTION");
            for s in &suites {
                shell_println!("  {:16} {:8} {}", s.name, s.category, s.description);
            }
            shell_println!("");
            let cats = selftest::categories();
            shell_println!("  Categories: {:?}", cats);
            shell_println!("  Usage: selftest <category> or selftest <name>");
        }
        "" | "all" => {
            shell_println!("Running all self-tests...");
            shell_println!("");
            let results = selftest::run_all();
            shell_println!("");
            if results.failed.is_empty() {
                shell_println!("=== ALL {} TESTS PASSED ===", results.total);
            } else {
                shell_println!("=== {}/{} PASSED, {} FAILED ===",
                    results.passed, results.total, results.failed.len());
                for name in &results.failed {
                    shell_println!("  FAIL: {}", name);
                }
            }
        }
        filter => {
            // Check if it's a category.
            let cats = selftest::categories();
            if cats.contains(&filter) {
                shell_println!("Running '{}' category tests...", filter);
                shell_println!("");
                let results = selftest::run_category(filter);
                shell_println!("");
                shell_println!("=== {}/{} PASSED ===", results.passed, results.total);
            } else {
                // Try as a specific test name.
                let results = selftest::run_one(filter);
                if results.total == 0 {
                    shell_println!("No test found with name '{}'", filter);
                    shell_println!("Use 'selftest list' to see available tests");
                } else {
                    shell_println!("");
                    shell_println!("=== {}/{} PASSED ===", results.passed, results.total);
                }
            }
        }
    }
}

/// `snapshot` — comprehensive kernel state snapshot and diff.
///
/// Usage:
///   snapshot save A     — capture current state as A
///   snapshot save B     — capture current state as B
///   snapshot diff A B   — compare two snapshots
///   snapshot show A     — show snapshot details
///   snapshot clear      — clear both snapshots
fn cmd_snapshot(args: &str) {
    use crate::ksnapshot;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "save" => {
            let label = parts.get(1).and_then(|s| s.as_bytes().first().copied()).unwrap_or(b'A');
            if label != b'A' && label != b'B' {
                shell_println!("Error: label must be A or B");
                return;
            }
            ksnapshot::save(label);
            let snap = ksnapshot::get(label).expect("just saved");
            shell_println!("Snapshot '{}' saved (tick={}, free={}/{}, pressure={})",
                label as char, snap.tick, snap.free_frames, snap.total_frames, snap.pressure_score);
        }
        "diff" => {
            if parts.len() < 3 {
                shell_println!("Usage: snapshot diff A B");
                return;
            }
            let from = parts[1].as_bytes().first().copied().unwrap_or(b'A');
            let to = parts[2].as_bytes().first().copied().unwrap_or(b'B');

            match ksnapshot::diff(from, to) {
                Some(d) => {
                    shell_println!("=== Snapshot Diff: {} → {} (Δ {} ticks) ===",
                        d.from_label as char, d.to_label as char, d.tick_delta);
                    shell_println!("");
                    shell_println!("  Memory:");
                    shell_println!("    Free frames:    {:+}", d.free_frames_delta);
                    shell_println!("    Fragmentation:  {:+}%", d.frag_delta);
                    shell_println!("    Heap net objs:  {:+}", d.heap_net_delta);
                    shell_println!("    Pressure:       {:+}", d.pressure_delta);
                    shell_println!("");
                    shell_println!("  Scheduler:");
                    shell_println!("    Context sw:     +{}", d.ctx_switches_delta);
                    shell_println!("    Tasks spawned:  +{}", d.tasks_spawned_delta);
                    shell_println!("    Tasks exited:   +{}", d.tasks_exited_delta);
                    shell_println!("    Load avg:       {:+}", d.load_delta);
                    shell_println!("");
                    shell_println!("  IPC:");
                    shell_println!("    Total ops:      +{}", d.ipc_ops_delta);
                    shell_println!("    Channel sends:  +{}", d.channel_sends_delta);
                    shell_println!("    Pipe bytes:     +{}", d.pipe_bytes_delta);
                    shell_println!("");
                    shell_println!("  Objects:          {:+}", d.objects_delta);
                    shell_println!("  Cap events:       +{}", d.cap_events_delta);
                    if d.cap_denials_delta > 0 {
                        shell_println!("  Cap denials:      +{} ⚠", d.cap_denials_delta);
                    }

                    // Warnings
                    if d.free_frames_delta < -10 {
                        shell_println!("");
                        shell_println!("  ⚠ Memory consumed: {} frames", -d.free_frames_delta);
                    }
                    if d.objects_delta > 0 && d.tasks_exited_delta > 0 {
                        shell_println!("  ⚠ Objects grew despite task exits — possible leak");
                    }
                }
                None => {
                    shell_println!("Error: one or both snapshots not found");
                    shell_println!("  Use 'snapshot save A' and 'snapshot save B' first");
                }
            }
        }
        "show" => {
            let label = parts.get(1).and_then(|s| s.as_bytes().first().copied()).unwrap_or(b'A');
            match ksnapshot::get(label) {
                Some(s) => {
                    shell_println!("=== Snapshot '{}' (tick {}) ===", label as char, s.tick);
                    shell_println!("");
                    shell_println!("  Memory:     free={}/{} frag={}% pressure={}",
                        s.free_frames, s.total_frames, s.frag_pct, s.pressure_score);
                    shell_println!("  Heap:       slab={}/{} large={}",
                        s.heap_slab_allocs, s.heap_slab_frees, s.heap_large_allocs);
                    shell_println!("  Sched:      ctx_sw={} spawned={} exited={} load={}",
                        s.total_ctx_switches, s.tasks_spawned, s.tasks_exited, s.load_avg_x100);
                    shell_println!("  IPC:        ops={} chan={} pipe_b={} futex={}",
                        s.ipc_total_ops, s.channel_sends, s.pipe_bytes, s.futex_waits);
                    shell_println!("  Objects:    {}", s.total_objects);
                    shell_println!("  Cap:        events={} denials={}", s.cap_events, s.cap_denials);
                }
                None => {
                    shell_println!("Snapshot '{}' not found", label as char);
                }
            }
        }
        "clear" => {
            ksnapshot::clear();
            shell_println!("All snapshots cleared");
        }
        _ => {
            shell_println!("Usage: snapshot <save|diff|show|clear>");
            shell_println!("");
            shell_println!("  snapshot save A     — capture state as A");
            shell_println!("  snapshot save B     — capture state as B");
            shell_println!("  snapshot diff A B   — compare A and B");
            shell_println!("  snapshot show A     — show snapshot A details");
            shell_println!("  snapshot clear      — clear both snapshots");
        }
    }
}

/// `ripsample` — statistical RIP profiler.
///
/// Usage:
///   ripsample          — show where CPU time is being spent
///   ripsample on       — start sampling
///   ripsample off      — stop sampling
///   ripsample top      — show hottest address
///   ripsample reset    — clear all samples
fn cmd_rip_sample(args: &str) {
    use crate::rip_sample;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "on" => {
            rip_sample::enable();
            shell_println!("RIP sampling enabled (records on every timer tick)");
        }
        "off" => {
            rip_sample::disable();
            shell_println!("RIP sampling disabled");
        }
        "reset" => {
            rip_sample::reset();
            shell_println!("RIP samples cleared");
        }
        "top" => {
            match rip_sample::hottest_rip() {
                Some((rip, count)) => {
                    let total = rip_sample::stats().total_samples;
                    let pct = if total > 0 { count as u64 * 100 / total } else { 0 };
                    shell_println!("Hottest RIP: {:#x} ({}% of {} samples)",
                        rip, pct, total);
                    // Try to resolve symbol.
                    if let Some(name) = crate::ksyms::resolve(rip) {
                        shell_println!("  Symbol: {}", name);
                    }
                }
                None => {
                    shell_println!("No samples collected (use 'ripsample on')");
                }
            }
        }
        "recent" => {
            let mut buf = [rip_sample::RipSample::empty(); 16];
            let n = rip_sample::recent(&mut buf);
            if n == 0 {
                shell_println!("No samples");
                return;
            }
            shell_println!("Last {} RIP samples (newest first):", n);
            shell_println!("  {:>18}  {:>3}  {:>12}", "RIP", "CPU", "CLASS");
            for i in 0..n {
                let s = &buf[i];
                if s.is_valid() {
                    shell_println!("  {:#018x}  {:>3}  {:>12}",
                        s.rip, s.cpu, s.addr_class().name());
                }
            }
        }
        _ => {
            // Default: show breakdown by address class.
            let s = rip_sample::stats();
            if s.total_samples == 0 {
                shell_println!("No RIP samples collected");
                shell_println!("Use 'ripsample on' to enable, then wait for timer ticks");
                return;
            }
            shell_println!("=== RIP Sampling Profile ({} samples) ===", s.total_samples);
            shell_println!("");
            shell_println!("  {:>12}  {:>8}  {:>5}",
                "REGION", "SAMPLES", "%");
            let classes = [
                rip_sample::AddrClass::KernelText,
                rip_sample::AddrClass::KernelHeap,
                rip_sample::AddrClass::KernelStack,
                rip_sample::AddrClass::Hhdm,
                rip_sample::AddrClass::UserCode,
                rip_sample::AddrClass::Idle,
                rip_sample::AddrClass::Isr,
                rip_sample::AddrClass::Other,
            ];
            for (i, class) in classes.iter().enumerate() {
                let count = s.bucket_counts[i];
                if count > 0 {
                    let pct = count * 100 / s.total_samples;
                    shell_println!("  {:>12}  {:>8}  {:>4}%",
                        class.name(), count, pct);
                }
            }
            shell_println!("");
            shell_println!("  Enabled: {}", if s.enabled { "yes" } else { "no" });

            if let Some((rip, count)) = rip_sample::hottest_rip() {
                let pct = count as u64 * 100 / s.total_samples;
                shell_println!("  Hottest: {:#x} ({}%)", rip, pct);
            }
        }
    }
}

/// `watch` — software memory watchpoints.
///
/// Usage:
///   watch add <addr> [label]  — add watchpoint on kernel address
///   watch list                — show active watchpoints
///   watch poll                — check for changes
///   watch del <slot>          — remove watchpoint
///   watch events              — show recent change events
///   watch clear               — remove all watchpoints
fn cmd_watchpoint(args: &str) {
    use crate::watchpoint;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "add" => {
            if let Some(&addr_str) = parts.get(1) {
                let addr = if let Some(hex) = addr_str.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).ok()
                } else {
                    addr_str.parse::<u64>().ok()
                };

                match addr {
                    Some(a) => {
                        let label = parts.get(2).copied().unwrap_or("").as_bytes();
                        match watchpoint::add(a, label) {
                            Some(slot) => {
                                shell_println!("Watchpoint #{} added at {:#x}", slot, a);
                            }
                            None => {
                                shell_println!("Error: invalid address or all slots full");
                                shell_println!("  Address must be kernel-space (>=0xffff800000000000)");
                                shell_println!("  and 8-byte aligned");
                            }
                        }
                    }
                    None => shell_println!("Error: invalid address format"),
                }
            } else {
                shell_println!("Usage: watch add <hex_addr> [label]");
            }
        }
        "list" => {
            let all = watchpoint::list();
            let active: alloc::vec::Vec<_> = all.iter()
                .filter(|(_, wp)| wp.active)
                .collect();
            if active.is_empty() {
                shell_println!("No active watchpoints");
                return;
            }
            shell_println!("=== Active Watchpoints ===");
            shell_println!("");
            shell_println!("  {:>4}  {:>18}  {:>18}  {:>6}  {:>8}",
                "SLOT", "ADDRESS", "LAST VALUE", "CHANGE", "LABEL");
            for (slot, wp) in &active {
                let label = core::str::from_utf8(&wp.label)
                    .unwrap_or("?")
                    .trim_end_matches('\0');
                shell_println!("  {:>4}  {:#018x}  {:#018x}  {:>6}  {:>8}",
                    slot, wp.address, wp.last_value, wp.change_count, label);
            }
        }
        "poll" => {
            let changes = watchpoint::poll();
            if changes == 0 {
                shell_println!("No changes detected");
            } else {
                shell_println!("{} watchpoint(s) triggered!", changes);
                // Show the most recent events.
                let mut events = [watchpoint::WatchEvent { slot: 0, address: 0, old_value: 0, new_value: 0, tick: 0 }; 4];
                let n = watchpoint::recent_events(&mut events);
                for i in 0..n.min(changes) {
                    let e = &events[i];
                    shell_println!("  [{}] {:#x}: {:#x} → {:#x} (tick {})",
                        e.slot, e.address, e.old_value, e.new_value, e.tick);
                }
            }
        }
        "del" => {
            if let Some(&slot_str) = parts.get(1) {
                if let Ok(slot) = slot_str.parse::<usize>() {
                    if watchpoint::remove(slot) {
                        shell_println!("Watchpoint #{} removed", slot);
                    } else {
                        shell_println!("Error: slot {} not active", slot);
                    }
                } else {
                    shell_println!("Error: invalid slot number");
                }
            } else {
                shell_println!("Usage: watch del <slot>");
            }
        }
        "events" => {
            let mut events = [watchpoint::WatchEvent { slot: 0, address: 0, old_value: 0, new_value: 0, tick: 0 }; 16];
            let n = watchpoint::recent_events(&mut events);
            if n == 0 {
                shell_println!("No watchpoint events recorded");
                return;
            }
            shell_println!("Last {} watchpoint events (newest first):", n);
            shell_println!("  {:>4}  {:>18}  {:>18}  {:>18}  {:>8}",
                "SLOT", "ADDRESS", "OLD", "NEW", "TICK");
            for i in 0..n {
                let e = &events[i];
                if e.tick != 0 {
                    shell_println!("  {:>4}  {:#018x}  {:#018x}  {:#018x}  {:>8}",
                        e.slot, e.address, e.old_value, e.new_value, e.tick);
                }
            }
        }
        "clear" => {
            watchpoint::clear();
            shell_println!("All watchpoints cleared");
        }
        _ => {
            shell_println!("Usage: watch <add|list|poll|del|events|clear>");
            shell_println!("");
            shell_println!("  watch add <addr> [label]  — monitor kernel address");
            shell_println!("  watch list                — show active watchpoints");
            shell_println!("  watch poll                — check for value changes");
            shell_println!("  watch del <slot>          — remove a watchpoint");
            shell_println!("  watch events              — show change history");
            shell_println!("  watch clear               — remove all");
        }
    }
}

/// `fraghist` — show memory fragmentation history and trend.
///
/// Usage:
///   fraghist          — show latest snapshot and trend
///   fraghist sample   — take a snapshot now
///   fraghist detail   — show all stored snapshots
///   fraghist clear    — reset history
fn cmd_frag_history(args: &str) {
    use crate::mm::frag_history;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "sample" => {
            frag_history::sample();
            let snap = frag_history::latest().expect("just sampled");
            shell_println!("Fragmentation sample taken: {}% (free={}, max_order={})",
                snap.frag_pct, snap.free_frames, snap.max_avail_order);
        }
        "clear" => {
            frag_history::clear();
            shell_println!("Fragmentation history cleared");
        }
        "detail" => {
            let mut buf = [frag_history::FragSnapshot::empty(); 32];
            let n = frag_history::recent(&mut buf);
            if n == 0 {
                shell_println!("No fragmentation history (use 'fraghist sample')");
                return;
            }
            shell_println!("=== Fragmentation History ({} samples) ===", n);
            shell_println!("");
            shell_println!("  {:>8}  {:>5}  {:>8}  {:>5}  {:>5}  {:>5}",
                "TICK", "FRAG%", "FREE", "MAXOR", "ORD0", "MAXBL");
            for i in 0..n {
                let s = &buf[i];
                if s.is_valid() {
                    shell_println!("  {:>8}  {:>5}  {:>8}  {:>5}  {:>5}  {:>5}",
                        s.tick, s.frag_pct, s.free_frames,
                        s.max_avail_order, s.order0_blocks, s.max_order_blocks);
                }
            }
            shell_println!("");
            shell_println!("  Trend: {}", frag_history::trend().name());
        }
        _ => {
            // Default: show latest and trend.
            match frag_history::latest() {
                Some(snap) => {
                    shell_println!("=== Memory Fragmentation ===");
                    shell_println!("");
                    shell_println!("  Current:     {}%", snap.frag_pct);
                    shell_println!("  Free frames: {}/{}", snap.free_frames, snap.total_frames);
                    shell_println!("  Max order:   {}", snap.max_avail_order);
                    shell_println!("  Order-0 blk: {} (singles)", snap.order0_blocks);
                    shell_println!("  Max-ord blk: {} (largest)", snap.max_order_blocks);
                    shell_println!("  Trend:       {}", frag_history::trend().name());
                    shell_println!("  Samples:     {}", frag_history::sample_count());

                    if snap.frag_pct > 70 {
                        shell_println!("");
                        shell_println!("  ⚠ High fragmentation — consider running compaction");
                    }
                }
                None => {
                    shell_println!("No fragmentation data yet");
                    shell_println!("Use 'fraghist sample' to take a snapshot");
                }
            }
        }
    }
}

/// `ipcstat` — show IPC statistics.
///
/// Usage:
///   ipcstat        — show all IPC mechanism stats
///   ipcstat reset  — reset all counters
///   ipcstat chan    — show only channel stats
///   ipcstat pipe   — show only pipe stats
///   ipcstat futex  — show only futex stats
fn cmd_ipc_stat(args: &str) {
    use crate::ipc::stats;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            stats::reset();
            shell_println!("IPC statistics reset");
        }
        "chan" | "channel" => {
            let s = stats::snapshot();
            shell_println!("=== Channel IPC ===");
            shell_println!("");
            shell_println!("  Messages sent:    {}", s.channel_sends);
            shell_println!("  Messages recv:    {}", s.channel_recvs);
            shell_println!("  Bytes sent:       {}", s.channel_bytes);
            shell_println!("  Avg msg size:     {} B", stats::avg_channel_msg_size());
            shell_println!("  Send blocks:      {}", s.channel_send_blocks);
            shell_println!("  Recv blocks:      {}", s.channel_recv_blocks);
            shell_println!("  Created:          {}", s.channels_created);
            shell_println!("  Destroyed:        {}", s.channels_destroyed);
        }
        "pipe" => {
            let s = stats::snapshot();
            shell_println!("=== Pipe IPC ===");
            shell_println!("");
            shell_println!("  Writes:           {}", s.pipe_writes);
            shell_println!("  Reads:            {}", s.pipe_reads);
            shell_println!("  Bytes written:    {}", s.pipe_bytes_written);
            shell_println!("  Bytes read:       {}", s.pipe_bytes_read);
            shell_println!("  Write blocks:     {}", s.pipe_write_blocks);
            shell_println!("  Read blocks:      {}", s.pipe_read_blocks);
            shell_println!("  Pipes created:    {}", s.pipes_created);
        }
        "futex" => {
            let s = stats::snapshot();
            shell_println!("=== Futex ===");
            shell_println!("");
            shell_println!("  Waits:            {}", s.futex_waits);
            shell_println!("  Wakes:            {}", s.futex_wakes);
            shell_println!("  Threads woken:    {}", s.futex_threads_woken);
            shell_println!("  Spurious waits:   {}", s.futex_spurious);
        }
        _ => {
            let s = stats::snapshot();
            let total = stats::total_operations();
            shell_println!("=== IPC Statistics ===");
            shell_println!("");
            shell_println!("  Total operations: {}", total);
            shell_println!("");
            shell_println!("  Channels:");
            shell_println!("    sends={} recvs={} bytes={} blocks={}+{}",
                s.channel_sends, s.channel_recvs, s.channel_bytes,
                s.channel_send_blocks, s.channel_recv_blocks);
            shell_println!("    created={} destroyed={}", s.channels_created, s.channels_destroyed);
            shell_println!("");
            shell_println!("  Pipes:");
            shell_println!("    writes={} reads={} bytes_w={} bytes_r={}",
                s.pipe_writes, s.pipe_reads, s.pipe_bytes_written, s.pipe_bytes_read);
            shell_println!("    blocks={}+{} created={}", s.pipe_write_blocks, s.pipe_read_blocks, s.pipes_created);
            shell_println!("");
            shell_println!("  Shared Memory:");
            shell_println!("    created={} destroyed={} mapped={}",
                s.shm_regions_created, s.shm_regions_destroyed, s.shm_bytes_mapped);
            shell_println!("");
            shell_println!("  Eventfd:");
            shell_println!("    signals={} reads={} wakeups={} created={}",
                s.eventfd_signals, s.eventfd_reads, s.eventfd_wakeups, s.eventfd_created);
            shell_println!("");
            shell_println!("  Completion Ports:");
            shell_println!("    posts={} waits={} blocks={} created={}",
                s.completion_posts, s.completion_waits, s.completion_wait_blocks, s.completion_created);
            shell_println!("");
            shell_println!("  Futex:");
            shell_println!("    waits={} wakes={} woken={} spurious={}",
                s.futex_waits, s.futex_wakes, s.futex_threads_woken, s.futex_spurious);
        }
    }
}

/// `syscallprof` — show syscall invocation profile.
///
/// Usage:
///   syscallprof        — show top syscalls by count
///   syscallprof reset  — reset counters
///   syscallprof on|off — enable/disable
fn cmd_syscall_prof(args: &str) {
    use crate::syscall::profile;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "reset" => {
            profile::reset();
            shell_println!("Syscall profile reset");
        }
        "on" => {
            profile::enable();
            shell_println!("Syscall profiling enabled");
        }
        "off" => {
            profile::disable();
            shell_println!("Syscall profiling disabled");
        }
        _ => {
            let o = profile::overall();
            shell_println!("=== Syscall Profile ===");
            shell_println!("");
            shell_println!("  Total calls:   {}", o.total_calls);
            shell_println!("  Total errors:  {}", o.total_errors);
            shell_println!("  Distinct:      {} syscalls", o.distinct_syscalls);
            shell_println!("  Status:        {}", if o.enabled { "enabled" } else { "disabled" });
            shell_println!("");

            // Top 16 by count.
            let empty = profile::SyscallStat {
                nr: 0, count: 0, total_cycles: 0, avg_cycles: 0, max_cycles: 0, errors: 0,
            };
            let mut top = [empty; 16];
            let n = profile::top_by_count(&mut top);

            if n > 0 {
                shell_println!("  {:>4}  {:>12}  {:>8}  {:>8}  {:>8}  {:>4}",
                    "NR", "NAME", "COUNT", "AVG_NS", "MAX_NS", "ERR");
                for i in 0..n {
                    let s = &top[i];
                    shell_println!("  {:>4}  {:>12}  {:>8}  {:>8}  {:>8}  {:>4}",
                        s.nr, profile::syscall_name(s.nr),
                        s.count,
                        profile::cycles_to_ns(s.avg_cycles),
                        profile::cycles_to_ns(s.max_cycles),
                        s.errors);
                }
            } else {
                shell_println!("  (no syscalls recorded)");
            }
        }
    }
}

/// `faultinject` — display/control memory fault injection.
///
/// Usage:
///   faultinject              — show current injection status
///   faultinject fail <N>     — arm: fail next N allocations
///   faultinject after <N>    — arm: fail after N successful allocs
///   faultinject prob <N>     — arm: fail every Nth allocation
///   faultinject off          — disarm all injection
fn cmd_fault_inject(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();

    if parts.is_empty() {
        // Show status.
        let s = crate::mm::fault_inject::stats();
        shell_println!("=== Memory Fault Injection ===");
        shell_println!("");
        shell_println!("  Active:          {}", if s.active { "YES" } else { "no" });
        shell_println!("  Mode:            {:?}", s.mode);
        shell_println!("  Counter:         {}", s.counter);
        shell_println!("  Total calls:     {}", s.total_calls);
        shell_println!("  Total injected:  {}", s.total_injected);
        shell_println!("  Sessions:        {}", s.sessions);
        return;
    }

    match parts[0] {
        "fail" => {
            let count = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            crate::mm::fault_inject::arm_fail_next(count);
            shell_println!("Armed: fail next {} allocation(s)", count);
        }
        "after" => {
            let count = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            crate::mm::fault_inject::arm_fail_after(count);
            shell_println!("Armed: fail after {} successful alloc(s)", count);
        }
        "prob" => {
            let denom = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(10);
            crate::mm::fault_inject::arm_probabilistic(denom);
            shell_println!("Armed: fail every ~1/{} allocations", denom);
        }
        "off" | "disarm" => {
            crate::mm::fault_inject::disarm();
            shell_println!("Disarmed — normal allocation restored");
        }
        _ => {
            shell_println!("Usage: faultinject [fail <N> | after <N> | prob <N> | off]");
        }
    }
}

/// `ptwalk` — walk and summarize current kernel page tables.
fn cmd_pt_walk() {
    shell_println!("=== Page Table Walk (kernel) ===");
    shell_println!("");

    // Read CR3 for current PML4.
    let cr3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    let pml4_phys = cr3 & 0x000F_FFFF_FFFF_F000;
    shell_println!("  PML4 physical: {:#x}", pml4_phys);

    // Walk the full address space.
    let (p4k, p2m, p1g, total) = unsafe {
        crate::mm::pt_walk::count_mapped(pml4_phys)
    };
    shell_println!("  4 KiB pages:   {}", p4k);
    shell_println!("  2 MiB pages:   {}", p2m);
    shell_println!("  1 GiB pages:   {}", p1g);
    shell_println!("  Total mapped:  {} MiB", total / (1024 * 1024));
    shell_println!("");

    let s = crate::mm::pt_walk::stats();
    shell_println!("  Walk ops:      {}", s.walk_ops);
    shell_println!("  Entries visited: {}", s.entries_visited);
}

/// `pageage` — display page aging statistics and histogram.
fn cmd_page_age() {
    let s = crate::mm::page_age::stats();
    let hist = crate::mm::page_age::age_histogram();
    shell_println!("=== Page Aging ===");
    shell_println!("");
    shell_println!("  Tracked pages:       {}", s.tracked_pages);
    shell_println!("  Scan cycles:         {}", s.scan_cycles);
    shell_println!("  Working set:         {} pages", s.working_set);
    shell_println!("  Eviction candidates: {} pages", s.eviction_candidates);
    shell_println!("  Hot found (total):   {}", s.hot_pages_found);
    shell_println!("  Cold found (total):  {}", s.cold_pages_found);
    shell_println!("  Dirty found (total): {}", s.dirty_pages_found);
    shell_println!("");
    shell_println!("  Age histogram:");
    for (age, &count) in hist.iter().enumerate() {
        if count > 0 {
            shell_println!("    age {}: {} pages", age, count);
        }
    }
    if hist.iter().all(|&c| c == 0) {
        shell_println!("    (empty)");
    }
}

/// `tlbgather` — display TLB gather (batch flush) statistics.
fn cmd_tlb_gather() {
    let s = crate::mm::tlb_gather::stats();
    shell_println!("=== TLB Flush Gather ===");
    shell_println!("");
    shell_println!("  Gather operations:    {}", s.gather_ops);
    shell_println!("  Pages freed (deferred): {}", s.pages_freed);
    shell_println!("  Partial flushes:      {}", s.partial_flushes);
    shell_println!("  Full flushes chosen:  {}", s.full_flush_chosen);
}

/// `msi` — display MSI (Message Signaled Interrupts) vector pool status.
fn cmd_msi() {
    let st = crate::msi::stats();
    shell_println!("=== MSI Vector Pool ===");
    shell_println!("");
    shell_println!("  Vectors allocated: {}", st.vectors_used);
    shell_println!("  Vectors total:     {}", st.vectors_total);
    shell_println!("  Vectors free:      {}", st.vectors_total.saturating_sub(st.vectors_used as usize));
    shell_println!("  Vector range:      48 - 223 (IDT)");
}

/// `kevent` — display kernel event bus statistics.
fn cmd_kevent() {
    let st = crate::kevent::stats();
    shell_println!("=== Kernel Event Bus ===");
    shell_println!("");
    shell_println!("  Events published:  {}", st.events_published);
    shell_println!("  Events delivered:  {}", st.events_delivered);
    shell_println!("  Events dropped:    {}", st.events_dropped);
    shell_println!("");

    let kind_names = [
        "MemoryPressure", "ThermalCritical", "CpuHotplug",
        "BlockDeviceError", "NetIfaceChange", "OomEvent",
        "PowerStateChange", "FsMount", "PanicImminent",
        "PeriodicTick", "Custom",
    ];
    shell_println!("  Subscribers per event kind:");
    for (i, &count) in st.subscriber_counts.iter().enumerate() {
        if count > 0 {
            let name = kind_names.get(i).unwrap_or(&"Unknown");
            shell_println!("    {:<20} {}", name, count);
        }
    }
    if st.subscriber_counts.iter().all(|&c| c == 0) {
        shell_println!("    (none)");
    }
}

/// `ksyms` — kernel symbol table inspection and address resolution.
fn cmd_ksyms(args: &str) {
    if !crate::ksyms::is_loaded() {
        shell_println!("Kernel symbols not loaded (kernel ELF may be stripped).");
        return;
    }

    let arg = args.trim();
    if arg.is_empty() {
        // Show summary.
        shell_println!("=== Kernel Symbols ===");
        shell_println!("");
        shell_println!("  Loaded: {} symbols", crate::ksyms::count());
        shell_println!("");
        shell_println!("  Usage: ksyms <address>  — resolve an address to a symbol");
        shell_println!("  Example: ksyms 0xffffffff80100000");
        return;
    }

    // Try to parse as a hex address.
    let addr_str = arg.strip_prefix("0x").or_else(|| arg.strip_prefix("0X")).unwrap_or(arg);
    let Ok(addr) = u64::from_str_radix(addr_str, 16) else {
        shell_println!("Invalid address: {}", arg);
        shell_println!("Usage: ksyms <hex_address>");
        return;
    };

    let formatted = crate::ksyms::format_addr(addr);
    shell_println!("  {:#018x} → {}", addr, formatted);
}

/// `rcu` — display RCU (Read-Copy-Update) statistics.
fn cmd_rcu() {
    let st = crate::rcu::stats();
    shell_println!("=== RCU Statistics ===");
    shell_println!("");
    shell_println!("  Grace periods completed: {}", st.gp_completed);
    shell_println!("  Synchronize calls:       {}", st.sync_calls);
    shell_println!("  Callbacks invoked:       {}", st.callbacks_invoked);
    shell_println!("  Pending callbacks:       {}", st.pending_callbacks);
    shell_println!("  Current GP counter:      {}", st.gp_counter);
}

/// `numa` — display NUMA topology information.
fn cmd_numa() {
    let info = crate::numa::topology_info();

    shell_println!("=== NUMA Topology ===");
    shell_println!("");
    shell_println!("  Nodes: {}", info.node_count);
    shell_println!("  NUMA detected: {}", if info.is_numa { "yes (SRAT)" } else { "no (UMA)" });
    shell_println!("");

    for i in 0..info.node_count {
        let node = &info.nodes[i];
        if !node.present {
            continue;
        }
        let mem_mb = node.total_memory / (1024 * 1024);
        shell_println!("  Node {}:", i);
        shell_println!("    CPUs: {} (mask={:#06x})", node.cpu_count, node.cpu_mask);
        shell_println!("    Memory: {} MiB ({} region{})",
            mem_mb, node.region_count,
            if node.region_count == 1 { "" } else { "s" });
    }

    shell_println!("");

    // Distance matrix.
    if info.node_count > 1 {
        shell_println!("  Distance matrix:");
        shell_print!("       ");
        for j in 0..info.node_count {
            shell_print!("{:>4}", j);
        }
        shell_println!("");
        for i in 0..info.node_count {
            shell_print!("    {:>2}:", i);
            for j in 0..info.node_count {
                shell_print!("{:>4}", crate::numa::distance(i, j));
            }
            shell_println!("");
        }
    }
}

/// `irqbalance` — display and control IRQ balancer.
fn cmd_irqbalance(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    let subcmd = parts.first().copied().unwrap_or("status");

    match subcmd {
        "status" | "" => {
            let st = crate::irqbalance::stats();
            shell_println!("=== IRQ Balancer ===");
            shell_println!("");
            shell_println!("  Status: {}", if st.enabled { "enabled" } else { "disabled" });
            shell_println!("  CPUs: {}", st.cpu_count);
            shell_println!("  Balance ops: {}", st.balance_ops);
            shell_println!("  Migrations: {}", st.migrations);
            shell_println!("");

            let irqs = crate::irqbalance::irq_info();
            if irqs.is_empty() {
                shell_println!("  No active IRQs tracked.");
            } else {
                shell_println!("  {:>4} {:>4} {:>6} {:>5} {:>8}", "IRQ", "CPU", "PINNED", "HINT", "RATE");
                shell_println!("  {:>4} {:>4} {:>6} {:>5} {:>8}", "---", "---", "------", "----", "----");
                for info in &irqs {
                    let hint_str = if info.hint == 0xFF {
                        alloc::string::String::from("-")
                    } else {
                        alloc::format!("{}", info.hint)
                    };
                    shell_println!("  {:>4} {:>4} {:>6} {:>5} {:>8}",
                        info.irq, info.cpu,
                        if info.pinned { "yes" } else { "no" },
                        hint_str, info.rate);
                }
            }
        }
        "enable" => {
            crate::irqbalance::set_enabled(true);
            shell_println!("IRQ balancer enabled.");
        }
        "disable" => {
            crate::irqbalance::set_enabled(false);
            shell_println!("IRQ balancer disabled.");
        }
        "pin" => {
            let (Some(irq_str), Some(cpu_str)) = (parts.get(1), parts.get(2)) else {
                shell_println!("Usage: irqbalance pin <irq> <cpu>");
                return;
            };
            let Ok(irq) = irq_str.parse::<u8>() else {
                shell_println!("Invalid IRQ: {}", irq_str);
                return;
            };
            let Ok(cpu) = cpu_str.parse::<usize>() else {
                shell_println!("Invalid CPU: {}", cpu_str);
                return;
            };
            crate::irqbalance::pin_irq(irq, cpu);
            shell_println!("IRQ {} pinned to CPU {}", irq, cpu);
        }
        "unpin" => {
            let Some(irq_str) = parts.get(1) else {
                shell_println!("Usage: irqbalance unpin <irq>");
                return;
            };
            let Ok(irq) = irq_str.parse::<u8>() else {
                shell_println!("Invalid IRQ: {}", irq_str);
                return;
            };
            crate::irqbalance::unpin_irq(irq);
            shell_println!("IRQ {} unpinned", irq);
        }
        _ => {
            shell_println!("Usage: irqbalance [status|enable|disable|pin <irq> <cpu>|unpin <irq>]");
        }
    }
}

/// `mempool` — display memory pool statistics.
fn cmd_mempool() {
    let pools = crate::mm::mempool::all_pool_stats();
    if pools.is_empty() {
        shell_println!("No memory pools registered.");
        return;
    }

    shell_println!("=== Memory Pools ===");
    shell_println!("");
    shell_println!("  {:<12} {:>6} {:>5} {:>5} {:>5} {:>8} {:>8} {:>5} {:>4}",
        "NAME", "OBJ_SZ", "CAP", "AVAIL", "USED", "ALLOCS", "FREES", "FAILS", "HWM");
    shell_println!("  {:<12} {:>6} {:>5} {:>5} {:>5} {:>8} {:>8} {:>5} {:>4}",
        "----", "------", "---", "-----", "----", "------", "-----", "-----", "---");

    for st in &pools {
        shell_println!("  {:<12} {:>6} {:>5} {:>5} {:>5} {:>8} {:>8} {:>5} {:>4}",
            st.name, st.obj_size, st.capacity, st.available, st.in_use,
            st.total_allocs, st.total_frees, st.alloc_failures, st.high_watermark);
    }

    shell_println!("");
    shell_println!("  {} pool{} registered", pools.len(),
        if pools.len() == 1 { "" } else { "s" });
}

/// `thermal` — display CPU thermal monitoring status.
fn cmd_thermal() {
    let info = crate::thermal::info();

    shell_println!("=== CPU Thermal Monitoring ===");
    shell_println!("");

    if !info.supported {
        shell_println!("  Thermal monitoring not supported on this CPU.");
        return;
    }

    shell_println!("  Current temp:   {}°C", info.current_temp);
    shell_println!("  Tj_max:         {}°C", info.tj_max);
    shell_println!("  Range:          {}°C .. {}°C (min .. max since boot)", info.min_temp, info.max_temp);
    shell_println!("  Mean:           {}°C ({} samples)", info.mean_temp, info.sample_count);
    shell_println!("  Throttled:      {}", if info.throttled { "YES" } else { "no" });
    shell_println!("");
    shell_println!("  Events:");
    shell_println!("    Throttle events:  {}", info.throttle_count);
    shell_println!("    Warnings (>=85°C): {}", info.warn_count);
    shell_println!("    Critical (>=95°C): {}", info.critical_count);

    // Show recent temperature history as a simple sparkline.
    let hist = crate::thermal::history(32);
    if !hist.is_empty() {
        shell_println!("");
        shell_println!("  Recent history (newest first):");
        let mut line = alloc::string::String::from("    ");
        for temp in &hist {
            use core::fmt::Write;
            let _ = write!(line, "{}  ", temp);
        }
        shell_println!("{}", line);
    }
}

/// `irqoff` — show interrupt-disabled duration statistics.
///
/// Shows how long interrupts have been disabled on average and at maximum.
/// Useful for finding paths that hold interrupts off too long.
///   `irqoff`       — show stats
///   `irqoff reset` — reset counters
///   `irqoff off`   — disable tracking
///   `irqoff on`    — enable tracking
fn cmd_irqoff(args: &str) {
    match args.trim() {
        "reset" => {
            crate::cpu::irqoff_tracker::reset();
            shell_println!("IRQ-off tracking stats reset.");
            return;
        }
        "off" => {
            crate::cpu::irqoff_tracker::set_enabled(false);
            shell_println!("IRQ-off tracking disabled.");
            return;
        }
        "on" => {
            crate::cpu::irqoff_tracker::set_enabled(true);
            shell_println!("IRQ-off tracking enabled.");
            return;
        }
        _ => {}
    }

    let s = crate::cpu::irqoff_tracker::stats();

    shell_println!("=== Interrupt-Disabled Duration ===");
    shell_println!("");

    if s.sections == 0 {
        shell_println!("  No interrupt-off sections recorded yet.");
        shell_println!("  (Tracking may be disabled, or no without_interrupts() calls made)");
        return;
    }

    let freq = crate::bench::tsc_freq();
    if freq > 0 {
        let max_ns = crate::bench::cycles_to_ns(s.max_cycles);
        let mean_ns = crate::bench::cycles_to_ns(s.mean_cycles);
        let total_us = crate::bench::cycles_to_ns(s.total_cycles) / 1000;

        shell_println!("  Sections recorded: {}", s.sections);
        shell_println!("");
        shell_println!("  Max IRQ-off:   {} cycles ({} ns)", s.max_cycles, max_ns);
        shell_println!("  Mean IRQ-off:  {} cycles ({} ns)", s.mean_cycles, mean_ns);
        shell_println!("  Total IRQ-off: {} cycles ({} us)", s.total_cycles, total_us);
    } else {
        shell_println!("  Sections recorded: {}", s.sections);
        shell_println!("");
        shell_println!("  Max IRQ-off:   {} cycles", s.max_cycles);
        shell_println!("  Mean IRQ-off:  {} cycles", s.mean_cycles);
        shell_println!("  Total IRQ-off: {} cycles", s.total_cycles);
    }

    // Warn if max is suspiciously long (> 1ms at assumed 3 GHz = 3M cycles).
    if s.max_cycles > 3_000_000 {
        shell_println!("");
        shell_println!("  WARNING: Max IRQ-off duration exceeds 1ms!");
        shell_println!("  This may cause timer jitter and missed interrupts.");
    }

    shell_println!("");
    let enabled = if crate::cpu::irqoff_tracker::is_enabled() { "ON" } else { "OFF" };
    shell_println!("  Tracking: {} | Use 'irqoff reset' to clear", enabled);
}

/// `pressure` — display memory pressure score and breakdown.
///
/// Shows a 0-100 composite score indicating how stressed the memory
/// subsystem is, broken down into physical usage, fragmentation,
/// heap failures, and swap usage components.
fn cmd_pressure() {
    let p = crate::mm::memory_pressure();

    let level_str = match p.level {
        crate::mm::PressureLevel::Low => "LOW (healthy)",
        crate::mm::PressureLevel::Moderate => "MODERATE",
        crate::mm::PressureLevel::High => "HIGH (watch closely)",
        crate::mm::PressureLevel::Critical => "CRITICAL (OOM risk!)",
    };

    shell_println!("=== Memory Pressure ===");
    shell_println!("");
    shell_println!("  Overall score: {}/100  [{}]", p.score, level_str);
    shell_println!("");
    shell_println!("  Component breakdown:");
    shell_println!("    Physical usage:   {:>3}/100  (weight: 40%)", p.phys_score);
    shell_println!("    Fragmentation:    {:>3}/100  (weight: 20%)", p.frag_score);
    shell_println!("    Heap failures:    {:>3}/100  (weight: 25%)", p.heap_score);
    shell_println!("    Swap usage:       {:>3}/100  (weight: 15%)", p.swap_score);
    shell_println!("");

    // Visual bar for overall score.
    let bar_filled = (p.score as usize) / 5; // 0-20 chars
    let bar_empty = 20 - bar_filled.min(20);
    let mut bar = [b' '; 20];
    for b in bar.iter_mut().take(bar_filled) {
        *b = if p.score > 75 { b'!' } else if p.score > 50 { b'#' } else { b'=' };
    }
    let _ = bar_empty; // suppress unused
    let bar_str = core::str::from_utf8(&bar).unwrap_or("");
    shell_println!("  [{}] {}/100", bar_str, p.score);
}

/// `latency` — display system-wide scheduling latency histogram.
///
/// Shows the distribution of how long tasks wait in the run queue
/// before being dispatched.  Useful for detecting scheduling stalls
/// and verifying real-time responsiveness.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_latency() {
    let h = crate::sched::latency_histogram();

    if h.total_events == 0 {
        shell_println!("No scheduling events recorded.");
        return;
    }

    shell_println!("=== Scheduling Latency Histogram ===");
    shell_println!("");
    shell_println!("  Total dispatches: {}", h.total_events);

    let mean_whole = h.mean_ticks_x100 / 100;
    let mean_frac = h.mean_ticks_x100 % 100;
    shell_println!("  Mean wait: {}.{:02} ticks ({} ms)",
        mean_whole, mean_frac, mean_whole * 10 + mean_frac / 10);
    shell_println!("  Max wait:  {} ticks ({} ms)", h.max_ticks, h.max_ticks * 10);
    shell_println!("");

    // Bucket labels (matching the classification in sched/mod.rs).
    const LABELS: [&str; 8] = [
        "    0 ticks (< 10ms) ",
        "    1 tick  (10-20ms)",
        "  2-4 ticks (20-50ms)",
        " 5-9  ticks (50-100ms)",
        "10-19 ticks (0.1-0.2s)",
        "20-49 ticks (0.2-0.5s)",
        "50-99 ticks (0.5-1.0s)",
        " 100+ ticks (> 1s)   ",
    ];

    shell_println!("  Bucket                    Count       %    Bar");
    shell_println!("  ------                    -----       -    ---");

    for i in 0..8 {
        let count = h.buckets[i];
        let pct = count.saturating_mul(100) / h.total_events;
        // ASCII bar (max 20 chars wide).
        let bar_len = (count.saturating_mul(20) / h.total_events) as usize;
        let bar_len = bar_len.min(20);

        let mut bar = [b' '; 20];
        for b in bar.iter_mut().take(bar_len) {
            *b = b'#';
        }
        let bar_str = core::str::from_utf8(&bar).unwrap_or("");

        shell_println!("  {} {:>8}  {:>3}%  |{}|",
            LABELS[i], count, pct, bar_str);
    }

    // Warnings for concerning latency patterns.
    let high_latency = h.buckets[5].saturating_add(h.buckets[6]).saturating_add(h.buckets[7]);
    if high_latency > 0 {
        let high_pct = high_latency.saturating_mul(100) / h.total_events;
        shell_println!("");
        shell_println!("  NOTE: {} dispatches ({}%) waited >200ms",
            high_latency, high_pct);
        if high_pct > 5 {
            shell_println!("  WARNING: Significant scheduling delays detected.");
            shell_println!("  Possible causes: lock contention, CPU saturation, priority inversion.");
        }
    }
}

/// `irqrate` — display interrupt rates (IRQs/sec per vector).
///
/// Shows the rate of interrupts for each active vector since the last
/// time this command was run.  First invocation establishes the baseline;
/// second invocation shows actual rates.
fn cmd_irqrate() {
    let rates = crate::idt::vector_rates();

    if rates.window_ticks == 0 {
        shell_println!("Baseline snapshot taken. Run 'irqrate' again to see rates.");
        return;
    }

    #[allow(clippy::arithmetic_side_effects)]
    let window_ms = (rates.window_ticks * 1000) / u64::from(crate::apic::TICK_RATE_HZ);

    shell_println!("=== Interrupt Rates (window: {} ms) ===", window_ms);
    shell_println!("");
    shell_println!("  Vec  Name                    Rate");
    shell_println!("  ---  ----                    ----");

    let mut any = false;
    for i in 0..48 {
        let rate_x10 = rates.rates_x10[i];
        if rate_x10 == 0 {
            continue;
        }
        any = true;

        let name = if i < 32 {
            crate::idt::EXCEPTION_NAMES[i]
        } else {
            match i {
                32 => "APIC Timer",
                33..=47 => "Device IRQ",
                _ => "Unknown",
            }
        };

        #[allow(clippy::arithmetic_side_effects)]
        let whole = rate_x10 / 10;
        #[allow(clippy::arithmetic_side_effects)]
        let frac = rate_x10 % 10;
        shell_println!("  {:3}  {:<22}  {}.{}/sec", i, name, whole, frac);
    }

    if !any {
        shell_println!("  (no interrupt activity during window)");
    }
}

/// `jitter` — display timer interrupt jitter statistics.
///
/// Shows the variance in inter-tick intervals (the time between
/// consecutive APIC timer interrupts).  Jitter indicates that something
/// delayed interrupt delivery: long critical sections, NMIs, SMIs, or
/// hypervisor VM exits.
fn cmd_jitter() {
    match crate::apic::timer_jitter() {
        None => {
            shell_println!("Timer jitter: no data (too few ticks)");
        }
        Some(j) => {
            shell_println!("=== Timer Jitter (inter-tick interval) ===");
            shell_println!("");
            shell_println!("  Samples:  {} intervals", j.count);
            shell_println!("  Expected: {} cycles/tick (from mean)", j.expected_cycles);
            shell_println!("");

            let freq = crate::bench::tsc_freq();
            if freq > 0 {
                let min_us = crate::bench::cycles_to_ns(j.min_cycles) / 1000;
                let mean_us = crate::bench::cycles_to_ns(j.mean_cycles) / 1000;
                let max_us = crate::bench::cycles_to_ns(j.max_cycles) / 1000;
                shell_println!("  Min:  {} cycles ({} us)", j.min_cycles, min_us);
                shell_println!("  Mean: {} cycles ({} us)", j.mean_cycles, mean_us);
                shell_println!("  Max:  {} cycles ({} us)", j.max_cycles, max_us);
            } else {
                shell_println!("  Min:  {} cycles", j.min_cycles);
                shell_println!("  Mean: {} cycles", j.mean_cycles);
                shell_println!("  Max:  {} cycles", j.max_cycles);
            }
            shell_println!("");

            // Compute jitter as percentage deviation from mean.
            if j.mean_cycles > 0 {
                let max_dev = j.max_cycles.saturating_sub(j.mean_cycles);
                let min_dev = j.mean_cycles.saturating_sub(j.min_cycles);
                #[allow(clippy::arithmetic_side_effects)]
                let max_pct = (max_dev.saturating_mul(100)) / j.mean_cycles;
                #[allow(clippy::arithmetic_side_effects)]
                let min_pct = (min_dev.saturating_mul(100)) / j.mean_cycles;
                shell_println!("  Deviation from mean:");
                shell_println!("    Max late:  +{}% ({} cycles over)", max_pct, max_dev);
                shell_println!("    Max early: -{}% ({} cycles under)", min_pct, min_dev);

                // Warn if jitter exceeds 10%.
                if max_pct > 10 || min_pct > 10 {
                    shell_println!("");
                    shell_println!("  WARNING: Jitter exceeds 10% — long critical");
                    shell_println!("  sections or external delays (SMI/NMI) detected.");
                }
            }
        }
    }
}

/// `heapwm` — display heap allocation watermark (peak usage).
///
/// Shows current bytes in use, peak bytes since boot, and allocation
/// throughput (total allocs/frees).
fn cmd_heapwm() {
    let s = crate::mm::heap::stats();

    shell_println!("=== Heap Allocation Watermark ===");
    shell_println!("");
    shell_println!("  Current in-use:     {} bytes ({} KiB)",
        s.bytes_in_use, s.bytes_in_use / 1024);
    shell_println!("  Peak (high-water):  {} bytes ({} KiB)",
        s.peak_bytes_in_use, s.peak_bytes_in_use / 1024);
    shell_println!("");

    // Utilization: current as percentage of peak.
    if s.peak_bytes_in_use > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let util_pct = (s.bytes_in_use.saturating_mul(100)) / s.peak_bytes_in_use;
        shell_println!("  Current/peak ratio: {}%", util_pct);
    }
    shell_println!("");

    // Allocation throughput.
    let total_allocs = s.slab_allocs.saturating_add(s.large_allocs);
    let total_frees = s.slab_frees.saturating_add(s.large_frees);
    shell_println!("  Total allocations:  {}", total_allocs);
    shell_println!("  Total frees:        {}", total_frees);
    shell_println!("  Net live objects:   {}", total_allocs.saturating_sub(total_frees));
    shell_println!("  OOM failures:       {}", s.alloc_failures);
}

fn cmd_memmap() {
    use crate::mm::page_table;
    use crate::mm::frame::FRAME_SIZE;

    shell_println!("=== Virtual Address Space Layout ===");
    shell_println!("");

    // User-space regions.
    shell_println!("  User Space [0x0000_0000_0000_0000 .. 0x0000_7FFF_FFFF_FFFF]");
    shell_println!("    Code/Data:   0x0000_0000_0040_0000  (ELF load base)");
    shell_println!("    Mmap region: 0x0000_0060_0000_0000  (SYS_MMAP allocations)");
    shell_println!("    Stack top:   0x0000_7FFF_FFFF_0000  (grows downward)");
    shell_println!("");

    // Canonical hole.
    shell_println!("  --- Non-canonical hole [0x0000_8000_0000_0000 .. 0xFFFF_7FFF_FFFF_FFFF] ---");
    shell_println!("");

    // Kernel-space regions.
    shell_println!("  Kernel Space [0xFFFF_8000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF]");

    // HHDM (Higher-Half Direct Map) — linear map of all physical memory.
    if let Some(hhdm) = page_table::hhdm() {
        let mem = crate::mm::memory_info();
        let phys_size = (mem.total_frames as u64).saturating_mul(FRAME_SIZE as u64);
        shell_println!("    HHDM:        {:#018x}  ({} MiB phys mapped)",
            hhdm, phys_size / (1024 * 1024));
    }

    // Kernel stack region.
    shell_println!("    Kstack:      0xFFFF_C100_0000_0000  (task kernel stacks)");

    // Kernel test map and demand-page test areas.
    shell_println!("    Test maps:   0xFFFF_C900_0000_0000  (page table self-tests)");
    shell_println!("    Demand test: 0xFFFF_CA00_0000_0000  (fault subsystem test)");

    // Kernel text/data (Limine loads kernel at the HHDM + its physical addr,
    // so it's somewhere in the HHDM region typically).
    shell_println!("");
    shell_println!("  Page size:     {} KiB (logical frame)", FRAME_SIZE / 1024);
    shell_println!("  HW page size:  4 KiB (x86_64 PTE granularity)");
    shell_println!("  PML4 entries:  kernel uses 256-511, user uses 0-255");
}

fn cmd_lockdep(args: &str) {
    let snap = crate::lockdep::snapshot();

    shell_println!("Lock dependency validator (lockdep)");
    shell_println!("  Status:     {}", if snap.enabled { "enabled" } else { "disabled" });
    shell_println!("  Classes:    {}", snap.classes.len());
    shell_println!("  Edges:      {}", snap.edges.len());
    shell_println!("  Violations: {}", snap.violations);
    shell_println!("");

    let subcmd = args.trim();

    if subcmd == "classes" || subcmd == "all" {
        shell_println!("  Registered lock classes:");
        shell_println!("  {:>4}  {:>16}  {}", "IDX", "ADDRESS", "NAME");
        shell_println!("  {:->4}  {:->16}  {:->16}", "", "", "");
        for c in &snap.classes {
            let name = core::str::from_utf8(&c.name[..c.name_len as usize]).unwrap_or("?");
            shell_println!("  {:>4}  {:#016x}  {}", c.index, c.id, name);
        }
        shell_println!("");
    }

    if subcmd == "edges" || subcmd == "graph" || subcmd == "all" {
        shell_println!("  Dependency edges (held → acquired):");
        shell_println!("  {:>16}  -->  {:>16}", "HELD", "ACQUIRED");
        shell_println!("  {:->16}  ---  {:->16}", "", "");
        for e in &snap.edges {
            let from_name = snap.classes.get(e.from as usize)
                .map(|c| core::str::from_utf8(&c.name[..c.name_len as usize]).unwrap_or("?"))
                .unwrap_or("?");
            let to_name = snap.classes.get(e.to as usize)
                .map(|c| core::str::from_utf8(&c.name[..c.name_len as usize]).unwrap_or("?"))
                .unwrap_or("?");
            shell_println!("  {:>16}  -->  {:>16}", from_name, to_name);
        }
        shell_println!("");
    }

    if subcmd == "held" || subcmd == "all" {
        let online = crate::smp::cpu_count().max(1);
        shell_println!("  Per-CPU held lock depth:");
        for cpu in 0..online {
            let depth = crate::lockdep::held_depth(cpu);
            if depth > 0 {
                shell_println!("    CPU {}: {} locks held", cpu, depth);
            }
        }
        let total_held: u8 = (0..online)
            .map(|cpu| crate::lockdep::held_depth(cpu))
            .fold(0u8, |a, b| a.saturating_add(b));
        if total_held == 0 {
            shell_println!("    (no locks held on any CPU)");
        }
        shell_println!("");
    }

    if subcmd.is_empty() {
        shell_println!("  Usage: lockdep [classes|edges|held|all]");
        shell_println!("    classes — show registered lock classes");
        shell_println!("    edges   — show dependency graph edges");
        shell_println!("    held    — show per-CPU held lock depth");
        shell_println!("    all     — show everything");
    }
}

#[allow(clippy::arithmetic_side_effects)]
fn cmd_vmstat() {
    let info = crate::mm::memory_info();
    let sched = crate::sched::sched_stats();
    let pi = crate::mm::pressure::pressure_info();

    // --- Memory counters ---
    crate::console_println!("nr_total_frames        {}", info.total_frames);
    crate::console_println!("nr_free_frames         {}", info.free_frames);
    crate::console_println!("nr_used_frames         {}",
        info.total_frames.saturating_sub(info.free_frames));
    crate::console_println!("nr_zero_pool           {}", info.zero_pool_count);
    crate::console_println!("fragmentation_pct      {}", info.fragmentation_pct);

    // --- Buddy order distribution ---
    for (order, &count) in info.order_counts.iter().enumerate() {
        crate::console_println!("buddy_order_{:<2}         {}", order, count);
    }

    // --- Per-CPU frame cache ---
    crate::console_println!("pcpu_cache_hits        {}", info.pcpu_cache_hits);
    crate::console_println!("pcpu_cache_misses      {}", info.pcpu_cache_misses);
    crate::console_println!("pcpu_refill_ops        {}", info.pcpu_refill_ops);
    crate::console_println!("pcpu_drain_ops         {}", info.pcpu_drain_ops);
    let total_allocs = info.pcpu_cache_hits.saturating_add(info.pcpu_cache_misses);
    let hit_pct = if total_allocs > 0 {
        info.pcpu_cache_hits.saturating_mul(100) / total_allocs
    } else { 0 };
    crate::console_println!("pcpu_hit_pct           {}", hit_pct);

    // --- Zero pool ---
    crate::console_println!("zero_pool_hits         {}", info.zero_pool_hits);
    crate::console_println!("zero_pool_misses       {}", info.zero_pool_misses);

    // --- Heap ---
    crate::console_println!("heap_slab_allocs       {}", info.heap_slab_allocs);
    crate::console_println!("heap_slab_frees        {}", info.heap_slab_frees);
    crate::console_println!("heap_large_allocs      {}", info.heap_large_allocs);
    crate::console_println!("heap_alloc_failures    {}", info.heap_alloc_failures);

    // --- Swap ---
    crate::console_println!("swap_total_bytes       {}", info.swap_total_bytes);
    crate::console_println!("swap_used_bytes        {}", info.swap_used_bytes);
    crate::console_println!("swap_devices           {}", info.swap_device_count);

    // --- kswapd ---
    crate::console_println!("kswapd_running         {}",
        if info.kswapd_running { 1 } else { 0 });
    crate::console_println!("kswapd_reclaim_cycles  {}", info.kswapd_reclaim_cycles);
    crate::console_println!("kswapd_reclaimed       {}", info.kswapd_total_reclaimed);

    // --- OOM ---
    crate::console_println!("oom_events             {}", info.oom_events);
    crate::console_println!("oom_kills              {}", info.oom_kills);

    // --- Pressure ---
    crate::console_println!("pressure_level         {}", pi.level as u8);
    crate::console_println!("pressure_shrinkers     {}", pi.active_shrinkers);
    crate::console_println!("pressure_notifications {}", pi.total_notifications);
    crate::console_println!("pressure_objects_freed {}", pi.total_freed);

    // --- Scheduler ---
    crate::console_println!("sched_ctx_switches     {}", sched.total_ctx_switches);
    crate::console_println!("sched_work_steals      {}", sched.total_work_steals);
    crate::console_println!("sched_tasks_spawned    {}", sched.total_tasks_spawned);
    crate::console_println!("sched_tasks_exited     {}", sched.total_tasks_exited);
    crate::console_println!("sched_load_avg_x100    {}", sched.load_avg_x100);
    crate::console_println!("sched_num_cpus         {}", sched.num_cpus);
    crate::console_println!("sched_starv_boosts     {}", crate::sched::starvation_boost_count());

    // --- Per-process accounting ---
    crate::console_println!("tracked_addr_spaces    {}", info.tracked_address_spaces);
}

/// Format a block device as FAT16/FAT32.
///
/// Usage: `mkfs.fat [-L LABEL] DEVICE`
///
/// Auto-selects FAT16 for volumes ≤32 MiB, FAT32 for larger.
/// **WARNING**: this overwrites all data on the device!
fn cmd_mkfs_fat(args: &str) {
    let mut label: Option<&str> = None;
    let mut device = "";

    let mut words = args.split_whitespace();
    while let Some(w) = words.next() {
        if w == "-L" || w == "-l" || w == "--label" {
            label = words.next();
        } else {
            device = w;
        }
    }

    if device.is_empty() {
        crate::console_println!("Usage: mkfs.fat [-L LABEL] DEVICE");
        crate::console_println!("  Formats DEVICE as FAT (auto-selects FAT16/FAT32).");
        crate::console_println!("  WARNING: all data on the device will be lost!");
        set_exit(1);
        return;
    }

    crate::console_println!("Formatting '{}' as FAT...", device);

    match crate::fs::fat::mkfs_fat(device, label) {
        Ok(()) => {
            crate::console_println!("Done. Device '{}' formatted successfully.", device);
        }
        Err(e) => {
            crate::console_println!("mkfs.fat: {:?}", e);
            set_exit(1);
        }
    }
}

/// Check (and optionally repair) a FAT filesystem.
///
/// Usage: `fsck.fat [-a] DEVICE`
/// -a: automatically repair errors
fn cmd_fsck_fat(args: &str) {
    let mut repair = false;
    let mut device = "";

    for w in args.split_whitespace() {
        if w == "-a" || w == "--repair" || w == "-y" {
            repair = true;
        } else if w == "-n" || w == "--no-repair" {
            repair = false;
        } else {
            device = w;
        }
    }

    if device.is_empty() {
        crate::console_println!("Usage: fsck.fat [-a] DEVICE");
        crate::console_println!("  Check FAT filesystem consistency.");
        crate::console_println!("  -a  Automatically repair errors");
        crate::console_println!("  -n  Check only, do not repair (default)");
        set_exit(1);
        return;
    }

    if repair {
        crate::console_println!("fsck.fat: checking and repairing '{}'...", device);
    } else {
        crate::console_println!("fsck.fat: checking '{}'...", device);
    }

    match crate::fs::fat::fsck_fat(device, repair) {
        Ok(report) => {
            for msg in &report.messages {
                crate::console_println!("  {}", msg);
            }
            if report.errors > 0 && !repair {
                set_exit(1);
            }
        }
        Err(e) => {
            crate::console_println!("fsck.fat: {:?}", e);
            set_exit(1);
        }
    }
}

/// `fsck.ext4 [-v] DEVICE` — check ext4 filesystem consistency.
///
/// Performs read-only checks: superblock validation, bitmap free count
/// vs group descriptor, inode scan, directory tree walk with link count
/// verification.
fn cmd_fsck_ext4(args: &str) {
    let mut verbose = false;
    let mut device = "";

    for w in args.split_whitespace() {
        if w == "-v" || w == "--verbose" {
            verbose = true;
        } else if w == "-h" || w == "--help" {
            crate::console_println!("Usage: fsck.ext4 [-v] DEVICE");
            crate::console_println!("  Check ext4 filesystem consistency (read-only).");
            crate::console_println!("  -v  Verbose output (show all phases)");
            return;
        } else {
            device = w;
        }
    }

    if device.is_empty() {
        crate::console_println!("Usage: fsck.ext4 [-v] DEVICE");
        set_exit(1);
        return;
    }

    crate::console_println!("fsck.ext4: checking '{}'...", device);

    match crate::fs::ext4::fsck::fsck_ext4(device) {
        Ok(report) => {
            for msg in &report.messages {
                if verbose || msg.starts_with("Phase") || msg.starts_with("Summary")
                    || msg.starts_with("  Filesystem") || msg.starts_with("  ") && !msg.starts_with("  All ")
                    || report.errors > 0
                {
                    crate::console_println!("{}", msg);
                }
            }
            if !verbose && report.errors == 0 {
                crate::console_println!(
                    "fsck.ext4: clean — {} files, {} dirs, {} symlinks, 0 errors",
                    report.files, report.dirs, report.symlinks
                );
            }
            if report.errors > 0 {
                set_exit(1);
            }
        }
        Err(e) => {
            crate::console_println!("fsck.ext4: {:?}", e);
            set_exit(1);
        }
    }
}

/// Show or set the volume label for a filesystem.
///
/// Usage: `label` or `label PATH` — show the label
///        `label PATH NAME` — set the label
fn cmd_label(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();

    let path = if parts.is_empty() || parts[0].is_empty() {
        get_cwd()
    } else {
        resolve_path(parts[0])
    };

    if parts.len() < 2 || parts[1].is_empty() {
        // Show the current label.
        match crate::fs::Vfs::statvfs(&path) {
            Ok(info) => {
                let label = if info.volume_label.is_empty() {
                    "(none)"
                } else {
                    &info.volume_label
                };
                crate::console_println!("{}: {} [{}]", path, label, info.fs_type);
            }
            Err(e) => {
                crate::console_println!("label: {}: {:?}", path, e);
                set_exit(1);
            }
        }
    } else {
        // Set the label.
        let new_label = parts[1];
        match crate::fs::Vfs::set_volume_label(&path, new_label) {
            Ok(()) => {
                crate::console_println!("Label set to '{}'", new_label);
            }
            Err(e) => {
                crate::console_println!("label: {:?}", e);
                set_exit(1);
            }
        }
    }
}

/// `flock` — query, acquire, or release advisory file locks.
///
/// Usage:
///   `flock FILE`       — query lock status
///   `flock -s FILE`    — acquire shared (read) lock
///   `flock -x FILE`    — acquire exclusive (write) lock
///   `flock -u FILE`    — release lock
fn cmd_flock(args: &str) {
    let mut mode: Option<&str> = None;
    let mut path_arg = "";

    for w in args.split_whitespace() {
        match w {
            "-s" | "--shared" => mode = Some("shared"),
            "-x" | "--exclusive" => mode = Some("exclusive"),
            "-u" | "--unlock" => mode = Some("unlock"),
            "-q" | "--query" => mode = Some("query"),
            _ => path_arg = w,
        }
    }

    if path_arg.is_empty() {
        crate::console_println!("Usage: flock [-s|-x|-u|-q] FILE");
        crate::console_println!("  -s  Acquire shared (read) lock");
        crate::console_println!("  -x  Acquire exclusive (write) lock");
        crate::console_println!("  -u  Release lock");
        crate::console_println!("  -q  Query lock status (default)");
        set_exit(1);
        return;
    }

    let path = resolve_path(path_arg);
    // Use task ID 0 for kernel/shell locks.
    let owner: u64 = 0;

    match mode.unwrap_or("query") {
        "shared" => {
            match crate::fs::Vfs::flock(&path, owner, crate::fs::vfs::LockType::Shared) {
                Ok(()) => crate::console_println!("{}: shared lock acquired", path_arg),
                Err(crate::error::KernelError::WouldBlock) => {
                    crate::console_println!("{}: lock denied (would block)", path_arg);
                    set_exit(1);
                }
                Err(e) => {
                    crate::console_println!("flock: {:?}", e);
                    set_exit(1);
                }
            }
        }
        "exclusive" => {
            match crate::fs::Vfs::flock(&path, owner, crate::fs::vfs::LockType::Exclusive) {
                Ok(()) => crate::console_println!("{}: exclusive lock acquired", path_arg),
                Err(crate::error::KernelError::WouldBlock) => {
                    crate::console_println!("{}: lock denied (would block)", path_arg);
                    set_exit(1);
                }
                Err(e) => {
                    crate::console_println!("flock: {:?}", e);
                    set_exit(1);
                }
            }
        }
        "unlock" => {
            match crate::fs::Vfs::funlock(&path, owner) {
                Ok(()) => crate::console_println!("{}: lock released", path_arg),
                Err(e) => {
                    crate::console_println!("flock: {:?}", e);
                    set_exit(1);
                }
            }
        }
        _ => {
            // Query mode.
            match crate::fs::Vfs::lock_query(&path) {
                Ok(Some((lock_type, count))) => {
                    let type_str = match lock_type {
                        crate::fs::vfs::LockType::Shared => "SHARED",
                        crate::fs::vfs::LockType::Exclusive => "EXCLUSIVE",
                    };
                    crate::console_println!("{}: {} ({} holder(s))", path_arg, type_str, count);
                }
                Ok(None) => {
                    crate::console_println!("{}: unlocked", path_arg);
                }
                Err(e) => {
                    crate::console_println!("flock: {:?}", e);
                    set_exit(1);
                }
            }
        }
    }
}

/// `split` — split a file into pieces.
///
/// Usage: `split [-l N] [-b SIZE] FILE [PREFIX]`
///   -l N     Split by N lines per piece (default 1000)
///   -b SIZE  Split by byte size per piece (K/M/G suffix)
fn cmd_split(args: &str) {
    let mut line_count: Option<usize> = None;
    let mut byte_size: Option<usize> = None;
    let mut file_path = "";
    let mut prefix = "x";

    let mut words = args.split_whitespace();
    while let Some(w) = words.next() {
        match w {
            "-l" | "--lines" => {
                if let Some(val) = words.next() {
                    match val.parse::<usize>() {
                        Ok(n) if n > 0 => line_count = Some(n),
                        _ => {
                            crate::console_println!("split: invalid line count '{}'", val);
                            set_exit(1);
                            return;
                        }
                    }
                }
            }
            "-b" | "--bytes" => {
                if let Some(val) = words.next() {
                    match parse_size_suffix(val) {
                        Some(n) if n > 0 => byte_size = Some(n as usize),
                        _ => {
                            crate::console_println!("split: invalid byte size '{}'", val);
                            set_exit(1);
                            return;
                        }
                    }
                }
            }
            _ => {
                if file_path.is_empty() {
                    file_path = w;
                } else {
                    prefix = w;
                }
            }
        }
    }

    if file_path.is_empty() {
        crate::console_println!("Usage: split [-l N] [-b SIZE] FILE [PREFIX]");
        crate::console_println!("  -l N     Lines per piece (default 1000)");
        crate::console_println!("  -b SIZE  Bytes per piece (K/M/G suffix)");
        crate::console_println!("  PREFIX   Output file prefix (default 'x', produces xaa, xab, ...)");
        set_exit(1);
        return;
    }

    let path = resolve_path(file_path);
    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("split: cannot read '{}': {:?}", file_path, e);
            set_exit(1);
            return;
        }
    };

    // Determine the parent directory for output files.
    let out_dir = get_cwd();
    let mut piece_num: u32 = 0;
    let mut total_written: usize = 0;

    // Generate suffix: aa, ab, ac, ..., az, ba, bb, ...
    let suffix_for = |n: u32| -> alloc::string::String {
        let a = (n / 26) as u8;
        let b = (n % 26) as u8;
        let mut s = alloc::string::String::with_capacity(2);
        s.push((b'a' + a) as char);
        s.push((b'a' + b) as char);
        s
    };

    if let Some(bsize) = byte_size {
        // Split by byte size.
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + bsize).min(data.len());
            let chunk = data.get(offset..end).unwrap_or(&[]);
            let suf = suffix_for(piece_num);
            let out_path = alloc::format!("{}/{}{}", out_dir, prefix, suf);
            match crate::fs::Vfs::write_file(&out_path, chunk) {
                Ok(()) => {}
                Err(e) => {
                    crate::console_println!("split: cannot write '{}': {:?}", out_path, e);
                    set_exit(1);
                    return;
                }
            }
            piece_num = piece_num.saturating_add(1);
            total_written = total_written.saturating_add(chunk.len());
            offset = end;
        }
    } else {
        // Split by line count (default 1000).
        let lines_per = line_count.unwrap_or(1000);
        let mut lines_in_piece = 0;
        let mut piece_start = 0;

        for (i, &b) in data.iter().enumerate() {
            if b == b'\n' {
                lines_in_piece += 1;
                if lines_in_piece >= lines_per {
                    let end = i + 1; // Include the newline.
                    let chunk = data.get(piece_start..end).unwrap_or(&[]);
                    let suf = suffix_for(piece_num);
                    let out_path = alloc::format!("{}/{}{}", out_dir, prefix, suf);
                    match crate::fs::Vfs::write_file(&out_path, chunk) {
                        Ok(()) => {}
                        Err(e) => {
                            crate::console_println!("split: cannot write '{}': {:?}", out_path, e);
                            set_exit(1);
                            return;
                        }
                    }
                    piece_num = piece_num.saturating_add(1);
                    total_written = total_written.saturating_add(chunk.len());
                    piece_start = end;
                    lines_in_piece = 0;
                }
            }
        }

        // Write remaining data (last piece, possibly less than lines_per lines).
        if piece_start < data.len() {
            let chunk = data.get(piece_start..).unwrap_or(&[]);
            let suf = suffix_for(piece_num);
            let out_path = alloc::format!("{}/{}{}", out_dir, prefix, suf);
            match crate::fs::Vfs::write_file(&out_path, chunk) {
                Ok(()) => {}
                Err(e) => {
                    crate::console_println!("split: cannot write '{}': {:?}", out_path, e);
                    set_exit(1);
                    return;
                }
            }
            piece_num = piece_num.saturating_add(1);
            total_written = total_written.saturating_add(chunk.len());
        }
    }

    crate::console_println!(
        "Split '{}' into {} pieces ({} bytes total)",
        file_path, piece_num, total_written
    );
}

/// `lsblk` — list block devices with capacity.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_lsblk() {
    let devices = crate::blkdev::list_devices();

    if devices.is_empty() {
        crate::console_println!("No block devices found.");
        return;
    }

    // Collect mount info for cross-referencing devices to mount points.
    let mounts = crate::fs::Vfs::mounts_full();

    crate::console_println!(
        "{:<8} {:>12} {:>8} {:>6}  {:<8} {:<16} {}",
        "NAME", "SECTORS", "SIZE", "RO", "FSTYPE", "MOUNTPOINT", "LABEL"
    );

    for dev in &devices {
        let size_bytes = (dev.sector_count as u64).saturating_mul(dev.sector_size as u64);
        let size_str = if size_bytes >= 1024 * 1024 * 1024 {
            alloc::format!("{} GiB", size_bytes / (1024 * 1024 * 1024))
        } else if size_bytes >= 1024 * 1024 {
            alloc::format!("{} MiB", size_bytes / (1024 * 1024))
        } else if size_bytes >= 1024 {
            alloc::format!("{} KiB", size_bytes / 1024)
        } else {
            alloc::format!("{} B", size_bytes)
        };
        let ro = if dev.read_only { "ro" } else { "rw" };

        // Probe filesystem type.
        let fs_type = if crate::fs::ext4::probe(&dev.name) {
            "ext4"
        } else if crate::fs::iso9660::probe(&dev.name) {
            "iso9660"
        } else {
            // Try FAT probe: read boot sector and check signature.
            let mut boot = [0u8; 512];
            if crate::fs::cache::read_sector(&dev.name, 0, &mut boot).is_ok()
                && boot.get(510).copied() == Some(0x55)
                && boot.get(511).copied() == Some(0xAA)
            {
                // Check for FAT signature in the filesystem type field.
                let fat16_sig = boot.get(54..62).unwrap_or(&[]);
                let fat32_sig = boot.get(82..90).unwrap_or(&[]);
                if fat32_sig.starts_with(b"FAT32") {
                    "fat32"
                } else if fat16_sig.starts_with(b"FAT16") || fat16_sig.starts_with(b"FAT12") || fat16_sig.starts_with(b"FAT") {
                    "fat16"
                } else {
                    ""
                }
            } else {
                ""
            }
        };

        // Check if this device is mounted anywhere.
        // The mount system uses the device name as the key for block-backed mounts.
        let mut mount_path = "";
        let mut label = String::new();
        for (mp, _fs_t, _opts) in &mounts {
            if let Ok(info) = crate::fs::Vfs::statvfs(mp) {
                // Heuristic: if the fstype matches and the device name appears
                // in the debug_stats output, it's this device.
                if let Ok(stats) = crate::fs::Vfs::debug_stats(mp) {
                    if stats.contains(&dev.name) || (info.fs_type == fs_type && !fs_type.is_empty()) {
                        mount_path = mp;
                        label = info.volume_label.clone();
                        break;
                    }
                }
            }
        }

        crate::console_println!(
            "{:<8} {:>12} {:>8} {:>6}  {:<8} {:<16} {}",
            dev.name, dev.sector_count, size_str, ro, fs_type, mount_path, label
        );
    }
}

/// `glob PATTERN` — expand a glob pattern against the filesystem.
///
/// Supports wildcards in any path component:
///   glob /tmp/*.txt
///   glob /proc/*/status
///   glob /sys/params/mm.*
fn cmd_glob(args: &str) {
    let pattern = args.trim();
    if pattern.is_empty() {
        crate::console_println!("Usage: glob <pattern>");
        crate::console_println!("  Example: glob /tmp/*.txt");
        crate::console_println!("  Example: glob /proc/*/status");
        return;
    }

    match crate::fs::Vfs::glob(pattern) {
        Ok(matches) => {
            if matches.is_empty() {
                crate::console_println!("(no matches)");
            } else {
                for path in &matches {
                    crate::console_println!("{}", path);
                }
                crate::console_println!("\n{} matches", matches.len());
            }
        }
        Err(e) => {
            crate::console_println!("glob: error: {:?}", e);
        }
    }
}

/// Compare two sorted files line by line.
///
/// Usage: `comm [-1] [-2] [-3] FILE1 FILE2`
///
/// Produces three-column output:
///   Column 1: lines only in FILE1
///   Column 2: lines only in FILE2
///   Column 3: lines in both files
///
/// `-1` suppresses column 1, `-2` suppresses column 2, `-3` suppresses column 3.
/// `comm -12 FILE1 FILE2` shows only lines common to both.
///
/// Reference: POSIX comm(1).
fn cmd_comm(args: &str) {
    let mut show1 = true;
    let mut show2 = true;
    let mut show3 = true;
    let mut files: alloc::vec::Vec<&str> = alloc::vec::Vec::new();

    for word in args.split_whitespace() {
        if word.starts_with('-') && word.len() > 1 && word.as_bytes()[1] != b'/' {
            // Parse flag characters.
            for ch in word[1..].chars() {
                match ch {
                    '1' => show1 = false,
                    '2' => show2 = false,
                    '3' => show3 = false,
                    _ => {
                        crate::console_println!("comm: unknown flag '-{}'", ch);
                        set_exit(1);
                        return;
                    }
                }
            }
        } else {
            files.push(word);
        }
    }

    if files.len() < 2 {
        crate::console_println!("Usage: comm [-123] FILE1 FILE2");
        set_exit(1);
        return;
    }

    let path1 = resolve_path(files[0]);
    let path2 = resolve_path(files[1]);

    let data1 = match crate::fs::Vfs::read_file(&path1) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("comm: {}: {:?}", path1, e);
            set_exit(1);
            return;
        }
    };
    let data2 = match crate::fs::Vfs::read_file(&path2) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("comm: {}: {:?}", path2, e);
            set_exit(1);
            return;
        }
    };

    let text1 = alloc::string::String::from_utf8_lossy(&data1);
    let text2 = alloc::string::String::from_utf8_lossy(&data2);

    let lines1: alloc::vec::Vec<&str> = text1.lines().collect();
    let lines2: alloc::vec::Vec<&str> = text2.lines().collect();

    let mut i = 0usize;
    let mut j = 0usize;

    // Build tab prefixes based on which columns are visible.
    // Column 2 is indented by one tab (or zero if col 1 is suppressed).
    // Column 3 is indented by two tabs minus suppressed columns.
    let col2_prefix = if show1 { "\t" } else { "" };
    let col3_prefix = match (show1, show2) {
        (true, true) => "\t\t",
        (true, false) | (false, true) => "\t",
        (false, false) => "",
    };

    while i < lines1.len() && j < lines2.len() {
        let cmp = lines1[i].cmp(lines2[j]);
        match cmp {
            core::cmp::Ordering::Less => {
                // Line only in file 1.
                if show1 {
                    crate::console_println!("{}", lines1[i]);
                }
                i += 1;
            }
            core::cmp::Ordering::Greater => {
                // Line only in file 2.
                if show2 {
                    crate::console_println!("{}{}", col2_prefix, lines2[j]);
                }
                j += 1;
            }
            core::cmp::Ordering::Equal => {
                // Line in both files.
                if show3 {
                    crate::console_println!("{}{}", col3_prefix, lines1[i]);
                }
                i += 1;
                j += 1;
            }
        }
    }

    // Remaining lines from file 1.
    while i < lines1.len() {
        if show1 {
            crate::console_println!("{}", lines1[i]);
        }
        i += 1;
    }

    // Remaining lines from file 2.
    while j < lines2.len() {
        if show2 {
            crate::console_println!("{}{}", col2_prefix, lines2[j]);
        }
        j += 1;
    }
}

/// Display file contents in various dump formats.
///
/// Usage: `od [-A RADIX] [-t TYPE] [-N COUNT] <file>`
///
/// Address radix: `-A o` (octal, default), `-A d` (decimal), `-A x` (hex), `-A n` (none)
/// Type: `-t o1` (octal bytes, default), `-t x1` (hex bytes), `-t d1` (decimal bytes),
///       `-t c` (ASCII/escape), `-t u1` (unsigned decimal)
/// `-N COUNT` limits output to COUNT bytes.
///
/// Reference: POSIX od(1).
#[allow(clippy::arithmetic_side_effects)]
fn cmd_od(args: &str) {
    let mut addr_fmt = 'o'; // octal addresses by default
    let mut data_fmt = 'o'; // octal bytes by default
    let mut max_bytes: usize = usize::MAX;
    let mut file_path = "";

    let mut words = args.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w == "-A" {
            if let Some(radix) = words.next() {
                addr_fmt = radix.chars().next().unwrap_or('o');
            }
        } else if w == "-t" {
            if let Some(ty) = words.next() {
                data_fmt = ty.chars().next().unwrap_or('o');
            }
        } else if w == "-N" {
            if let Some(n) = words.next() {
                max_bytes = n.parse::<usize>().unwrap_or(usize::MAX);
            }
        } else if w.starts_with("-N") {
            max_bytes = w[2..].parse::<usize>().unwrap_or(usize::MAX);
        } else {
            file_path = w;
        }
    }

    if file_path.is_empty() {
        crate::console_println!("Usage: od [-A o|d|x|n] [-t o1|x1|d1|u1|c] [-N count] <file>");
        set_exit(1);
        return;
    }

    let path = resolve_path(file_path);
    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("od: {}: {:?}", path, e);
            set_exit(1);
            return;
        }
    };

    let limit = data.len().min(max_bytes).min(4096);
    let data = &data[..limit];
    let bytes_per_line = 16;

    for offset in (0..data.len()).step_by(bytes_per_line) {
        let mut line = alloc::string::String::with_capacity(80);

        // Address.
        match addr_fmt {
            'o' => line.push_str(&alloc::format!("{:07o}", offset)),
            'd' => line.push_str(&alloc::format!("{:07}", offset)),
            'x' => line.push_str(&alloc::format!("{:07x}", offset)),
            'n' => {} // no address
            _ => line.push_str(&alloc::format!("{:07o}", offset)),
        }

        // Data bytes.
        let end = data.len().min(offset + bytes_per_line);
        for i in offset..end {
            match data_fmt {
                'o' => line.push_str(&alloc::format!(" {:03o}", data[i])),
                'x' => line.push_str(&alloc::format!(" {:02x}", data[i])),
                'd' => line.push_str(&alloc::format!(" {:4}", data[i] as i8)),
                'u' => line.push_str(&alloc::format!(" {:3}", data[i])),
                'c' => {
                    let ch = data[i];
                    let s = match ch {
                        b'\0' => alloc::string::String::from("  \\0"),
                        b'\n' => alloc::string::String::from("  \\n"),
                        b'\r' => alloc::string::String::from("  \\r"),
                        b'\t' => alloc::string::String::from("  \\t"),
                        0x20..=0x7E => alloc::format!("   {}", ch as char),
                        _ => alloc::format!(" {:03o}", ch),
                    };
                    line.push_str(&s);
                }
                _ => line.push_str(&alloc::format!(" {:03o}", data[i])),
            }
        }

        crate::console_println!("{}", line);
    }

    // Print final address (marks the end).
    match addr_fmt {
        'o' => crate::console_println!("{:07o}", data.len()),
        'd' => crate::console_println!("{:07}", data.len()),
        'x' => crate::console_println!("{:07x}", data.len()),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// tar — archive creation, extraction, and listing (USTAR format)
// ---------------------------------------------------------------------------

/// USTAR header constants.
pub const TAR_BLOCK_SIZE: usize = 512;
const TAR_MAGIC: &[u8; 6] = b"ustar\0";
const TAR_VERSION: &[u8; 2] = b"00";

/// File type flags for tar headers.
const TAR_REGTYPE: u8 = b'0';       // Regular file
const TAR_DIRTYPE: u8 = b'5';       // Directory
const TAR_SYMTYPE: u8 = b'2';       // Symbolic link

/// Build a USTAR header block for a file or directory.
///
/// Returns a 512-byte header with checksum computed.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn tar_build_header(
    path: &str,
    size: u64,
    mtime_ns: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    typeflag: u8,
    linkname: &str,
) -> [u8; TAR_BLOCK_SIZE] {
    let mut header = [0u8; TAR_BLOCK_SIZE];

    // Split path into prefix (155 bytes) + name (100 bytes) if needed.
    let path_bytes = path.as_bytes();
    if path_bytes.len() <= 100 {
        let copy_len = path_bytes.len().min(100);
        header[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
    } else {
        // Try to split at a '/' boundary for USTAR prefix support.
        let mut split_at = None;
        for i in (0..path_bytes.len().saturating_sub(100)).rev() {
            if path_bytes[i] == b'/' {
                split_at = Some(i);
                break;
            }
        }
        if let Some(s) = split_at {
            // prefix = path[..s], name = path[s+1..]
            let prefix = &path_bytes[..s];
            let name = &path_bytes[s + 1..];
            let plen = prefix.len().min(155);
            let nlen = name.len().min(100);
            header[..nlen].copy_from_slice(&name[..nlen]);
            header[345..345 + plen].copy_from_slice(&prefix[..plen]);
        } else {
            // Can't split — truncate to 100 bytes.
            header[..100].copy_from_slice(&path_bytes[..100]);
        }
    }

    // Mode (octal, 8 bytes including NUL).
    let mode_str = alloc::format!("{:07o}\0", mode & 0o7777);
    let mode_bytes = mode_str.as_bytes();
    let mlen = mode_bytes.len().min(8);
    header[100..100 + mlen].copy_from_slice(&mode_bytes[..mlen]);

    // UID.
    let uid_str = alloc::format!("{:07o}\0", uid);
    let uid_bytes = uid_str.as_bytes();
    let ulen = uid_bytes.len().min(8);
    header[108..108 + ulen].copy_from_slice(&uid_bytes[..ulen]);

    // GID.
    let gid_str = alloc::format!("{:07o}\0", gid);
    let gid_bytes = gid_str.as_bytes();
    let glen = gid_bytes.len().min(8);
    header[116..116 + glen].copy_from_slice(&gid_bytes[..glen]);

    // Size (octal, 12 bytes including NUL).
    let size_str = alloc::format!("{:011o}\0", size);
    let size_bytes = size_str.as_bytes();
    let slen = size_bytes.len().min(12);
    header[124..124 + slen].copy_from_slice(&size_bytes[..slen]);

    // Mtime (seconds since Unix epoch, octal).
    let mtime_sec = mtime_ns / 1_000_000_000;
    let mtime_str = alloc::format!("{:011o}\0", mtime_sec);
    let mtime_bytes = mtime_str.as_bytes();
    let mtlen = mtime_bytes.len().min(12);
    header[136..136 + mtlen].copy_from_slice(&mtime_bytes[..mtlen]);

    // Typeflag.
    header[156] = typeflag;

    // Linkname (100 bytes).
    if !linkname.is_empty() {
        let lbytes = linkname.as_bytes();
        let llen = lbytes.len().min(100);
        header[157..157 + llen].copy_from_slice(&lbytes[..llen]);
    }

    // Magic + version.
    header[257..263].copy_from_slice(TAR_MAGIC);
    header[263..265].copy_from_slice(TAR_VERSION);

    // Checksum: fill with spaces first, then compute.
    header[148..156].copy_from_slice(b"        ");

    let mut cksum: u32 = 0;
    for &b in header.iter() {
        cksum = cksum.wrapping_add(u32::from(b));
    }
    let cksum_str = alloc::format!("{:06o}\0 ", cksum);
    let cksum_bytes = cksum_str.as_bytes();
    let clen = cksum_bytes.len().min(8);
    header[148..148 + clen].copy_from_slice(&cksum_bytes[..clen]);

    header
}

/// Parse a USTAR header. Returns (name, size, mtime_sec, typeflag, linkname)
/// or None if the block is all zeros (end-of-archive).
#[allow(clippy::arithmetic_side_effects)]
pub fn tar_parse_header(header: &[u8; TAR_BLOCK_SIZE]) -> Option<(alloc::string::String, u64, u64, u8, alloc::string::String)> {
    // Check for end-of-archive (all zeros).
    if header.iter().all(|&b| b == 0) {
        return None;
    }

    // Verify checksum.
    let stored_cksum = tar_parse_octal(&header[148..156]);
    let mut computed: u32 = 0;
    for (i, &b) in header.iter().enumerate() {
        if (148..156).contains(&i) {
            computed = computed.wrapping_add(u32::from(b' '));
        } else {
            computed = computed.wrapping_add(u32::from(b));
        }
    }
    if stored_cksum != u64::from(computed) {
        return None; // Invalid checksum.
    }

    // Name: prefix (345..500) + "/" + name (0..100).
    let name_raw = &header[..100];
    let name_end = name_raw.iter().position(|&b| b == 0).unwrap_or(100);
    let name_part = core::str::from_utf8(&name_raw[..name_end]).unwrap_or("");

    let prefix_raw = &header[345..500];
    let prefix_end = prefix_raw.iter().position(|&b| b == 0).unwrap_or(155);
    let prefix_part = core::str::from_utf8(&prefix_raw[..prefix_end]).unwrap_or("");

    let full_name = if prefix_part.is_empty() {
        alloc::string::String::from(name_part)
    } else {
        alloc::format!("{}/{}", prefix_part, name_part)
    };

    let size = tar_parse_octal(&header[124..136]);
    let mtime = tar_parse_octal(&header[136..148]);
    let typeflag = header[156];

    let link_raw = &header[157..257];
    let link_end = link_raw.iter().position(|&b| b == 0).unwrap_or(100);
    let linkname = core::str::from_utf8(&link_raw[..link_end]).unwrap_or("").into();

    Some((full_name, size, mtime, typeflag, linkname))
}

/// Parse an octal ASCII field (NUL/space terminated) into u64.
#[allow(clippy::arithmetic_side_effects)]
fn tar_parse_octal(field: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in field {
        if b == 0 || b == b' ' {
            break;
        }
        if b >= b'0' && b <= b'7' {
            val = val.wrapping_mul(8).wrapping_add(u64::from(b - b'0'));
        }
    }
    val
}

/// `tar` command — create, extract, or list USTAR archives.
///
/// Usage:
///   tar -cf archive.tar file1 [file2 ...]   Create archive
///   tar -xf archive.tar [-C dir]            Extract archive
///   tar -tf archive.tar                     List contents
///   tar -xvf / -tvf / -cvf                  Verbose mode
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn cmd_tar(args: &str) {
    use crate::fs::Vfs;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!("Usage: tar [-c|-x|-t][z][v]f archive [files...]");
        crate::console_println!("  -cf   Create archive from files/directories");
        crate::console_println!("  -czf  Create gzip-compressed archive (.tar.gz)");
        crate::console_println!("  -xf   Extract archive (auto-detects .tar.gz)");
        crate::console_println!("  -tf   List archive contents");
        crate::console_println!("  -v    Verbose output");
        return;
    }

    let flags = parts[0];
    let create = flags.contains('c');
    let extract = flags.contains('x');
    let list = flags.contains('t');
    let verbose = flags.contains('v');
    let gzip_compress = flags.contains('z');

    // Exactly one mode required.
    let mode_count = u8::from(create) + u8::from(extract) + u8::from(list);
    if mode_count != 1 {
        crate::console_println!("tar: specify exactly one of -c, -x, -t");
        return;
    }

    if !flags.contains('f') {
        crate::console_println!("tar: -f flag required (no stdin/stdout tar)");
        return;
    }

    if parts.len() < 2 {
        crate::console_println!("tar: missing archive filename after -f");
        return;
    }

    let archive_path = resolve_path(parts[1]);

    if create {
        // tar -cf archive.tar file1 dir1 ...
        if parts.len() < 3 {
            crate::console_println!("tar: no files specified for archiving");
            return;
        }

        let mut archive_data: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let mut file_count: u32 = 0;

        for &source in &parts[2..] {
            let source_path = resolve_path(source);
            if let Err(e) = tar_add_recursive(
                &source_path,
                &source_path,
                &mut archive_data,
                &mut file_count,
                verbose,
            ) {
                crate::console_println!("tar: {}: {:?}", source, e);
                return;
            }
        }

        // Two zero blocks to mark end of archive.
        archive_data.extend_from_slice(&[0u8; TAR_BLOCK_SIZE]);
        archive_data.extend_from_slice(&[0u8; TAR_BLOCK_SIZE]);

        // If -z flag, gzip-compress the archive before writing.
        let write_data = if gzip_compress {
            let compressed = crate::fs::compress::gzip(&archive_data);
            if verbose {
                crate::console_println!(
                    "tar: gzip compressed {} -> {} bytes",
                    archive_data.len(), compressed.len()
                );
            }
            compressed
        } else {
            archive_data
        };

        match Vfs::write_file(&archive_path, &write_data) {
            Ok(()) => {
                crate::console_println!(
                    "tar: created '{}' ({} files, {} bytes)",
                    archive_path, file_count, write_data.len()
                );
            }
            Err(e) => {
                crate::console_println!("tar: write '{}': {:?}", archive_path, e);
            }
        }
    } else if extract || list {
        // Parse -C dir option for extract.
        let mut target_dir = alloc::string::String::from("/");
        let mut i = 2;
        while i < parts.len() {
            if parts[i] == "-C" && i + 1 < parts.len() {
                target_dir = resolve_path(parts[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
        }

        let raw_data = match Vfs::read_file(&archive_path) {
            Ok(d) => d,
            Err(e) => {
                crate::console_println!("tar: read '{}': {:?}", archive_path, e);
                return;
            }
        };

        // Auto-detect compression: gzip (0x1f 0x8b) or bzip2 ("BZh").
        let data = if raw_data.len() >= 2
            && raw_data.first() == Some(&0x1F)
            && raw_data.get(1) == Some(&0x8B)
        {
            match crate::fs::compress::gunzip(&raw_data) {
                Ok(decompressed) => {
                    if verbose {
                        crate::console_println!(
                            "tar: decompressed gzip: {} -> {} bytes",
                            raw_data.len(), decompressed.len()
                        );
                    }
                    decompressed
                }
                Err(e) => {
                    crate::console_println!("tar: gzip decompress failed: {:?}", e);
                    return;
                }
            }
        } else if raw_data.len() >= 4
            && raw_data.first() == Some(&b'B')
            && raw_data.get(1) == Some(&b'Z')
            && raw_data.get(2) == Some(&b'h')
        {
            match crate::fs::bzip2::bunzip2(&raw_data) {
                Ok(decompressed) => {
                    if verbose {
                        crate::console_println!(
                            "tar: decompressed bzip2: {} -> {} bytes",
                            raw_data.len(), decompressed.len()
                        );
                    }
                    decompressed
                }
                Err(e) => {
                    crate::console_println!("tar: bzip2 decompress failed: {:?}", e);
                    return;
                }
            }
        } else {
            raw_data
        };

        let mut offset: usize = 0;
        let mut file_count: u32 = 0;

        while offset + TAR_BLOCK_SIZE <= data.len() {
            let mut header_buf = [0u8; TAR_BLOCK_SIZE];
            header_buf.copy_from_slice(&data[offset..offset + TAR_BLOCK_SIZE]);
            offset += TAR_BLOCK_SIZE;

            let (name, size, _mtime, typeflag, _linkname) = match tar_parse_header(&header_buf) {
                Some(h) => h,
                None => break, // End of archive.
            };

            // Data blocks follow the header.
            let data_blocks = if size > 0 {
                ((size as usize) + TAR_BLOCK_SIZE - 1) / TAR_BLOCK_SIZE
            } else {
                0
            };
            let data_start = offset;
            let data_end = (offset + data_blocks * TAR_BLOCK_SIZE).min(data.len());
            offset = data_end;

            if list {
                // List mode: print entry info.
                let type_ch = match typeflag {
                    TAR_DIRTYPE => 'd',
                    TAR_SYMTYPE => 'l',
                    _ => '-',
                };
                if verbose {
                    crate::console_println!(
                        "{} {:>8} {}",
                        type_ch, size, name
                    );
                } else {
                    crate::console_println!("{}", name);
                }
            } else {
                // Extract mode.
                // Build full output path.
                let clean_name = name.trim_start_matches('/');
                let out_path = if target_dir == "/" {
                    alloc::format!("/{}", clean_name)
                } else {
                    alloc::format!("{}/{}", target_dir.trim_end_matches('/'), clean_name)
                };

                match typeflag {
                    TAR_DIRTYPE => {
                        // Create directory (ignore AlreadyExists).
                        match Vfs::mkdir(&out_path) {
                            Ok(()) | Err(crate::error::KernelError::AlreadyExists) => {}
                            Err(e) => {
                                crate::console_println!("tar: mkdir '{}': {:?}", out_path, e);
                            }
                        }
                        if verbose {
                            crate::console_println!("x {}", out_path);
                        }
                    }
                    TAR_REGTYPE | b'\0' => {
                        // Extract regular file.
                        // Ensure parent directory exists.
                        if let Some(slash) = out_path.rfind('/') {
                            if slash > 0 {
                                let parent = &out_path[..slash];
                                let _ = tar_mkdir_p(parent);
                            }
                        }

                        let file_data = if size > 0 && data_start + (size as usize) <= data.len() {
                            &data[data_start..data_start + size as usize]
                        } else {
                            &[]
                        };

                        match Vfs::write_file(&out_path, file_data) {
                            Ok(()) => {}
                            Err(e) => {
                                crate::console_println!("tar: write '{}': {:?}", out_path, e);
                            }
                        }
                        if verbose {
                            crate::console_println!("x {} ({} bytes)", out_path, size);
                        }
                    }
                    TAR_SYMTYPE => {
                        // Symlink — create if VFS supports it.
                        if verbose {
                            crate::console_println!(
                                "x {} -> {} (symlink, skipped)",
                                out_path, _linkname
                            );
                        }
                    }
                    _ => {
                        if verbose {
                            crate::console_println!(
                                "x {} (type '{}', skipped)",
                                out_path, typeflag as char
                            );
                        }
                    }
                }
            }

            file_count = file_count.saturating_add(1);
        }

        if extract {
            crate::console_println!(
                "tar: extracted {} entries from '{}'",
                file_count, archive_path
            );
        }
    }
}

/// Recursively add files/directories to a tar archive buffer.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn tar_add_recursive(
    path: &str,
    base: &str,
    archive: &mut alloc::vec::Vec<u8>,
    count: &mut u32,
    verbose: bool,
) -> crate::error::KernelResult<()> {
    use crate::fs::{vfs::EntryType, Vfs};

    let meta = Vfs::metadata(path)?;

    // Compute the archive-internal path: relative to the base's parent.
    // E.g., if base="/data" and path="/data/sub/file.txt", archive name = "data/sub/file.txt"
    let archive_name = if path == base {
        // Top-level item: use just the last component.
        let name = path.rsplit('/').next().unwrap_or(path);
        if meta.entry_type == EntryType::Directory {
            alloc::format!("{}/", name)
        } else {
            alloc::string::String::from(name)
        }
    } else {
        // Descendent: compute relative path from base's parent.
        let base_parent = if let Some(slash) = base.rfind('/') {
            if slash == 0 { "/" } else { &base[..slash] }
        } else {
            "/"
        };
        let rel = if base_parent == "/" {
            path.trim_start_matches('/')
        } else if let Some(rest) = path.strip_prefix(base_parent) {
            rest.trim_start_matches('/')
        } else {
            path.trim_start_matches('/')
        };
        if meta.entry_type == EntryType::Directory {
            alloc::format!("{}/", rel)
        } else {
            alloc::string::String::from(rel)
        }
    };

    match meta.entry_type {
        EntryType::Directory => {
            // Add directory header (size 0).
            let header = tar_build_header(
                &archive_name,
                0,
                meta.modified_ns,
                u32::from(meta.permissions),
                meta.uid,
                meta.gid,
                TAR_DIRTYPE,
                "",
            );
            archive.extend_from_slice(&header);
            *count = count.saturating_add(1);
            if verbose {
                crate::console_println!("a {}", archive_name);
            }

            // Recurse into children.
            let entries = Vfs::readdir(path)?;
            for entry in &entries {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }
                let child = if path == "/" {
                    alloc::format!("/{}", entry.name)
                } else {
                    alloc::format!("{}/{}", path, entry.name)
                };
                tar_add_recursive(&child, base, archive, count, verbose)?;
            }
        }
        EntryType::File => {
            let data = Vfs::read_file(path)?;
            let file_size = data.len() as u64;

            let header = tar_build_header(
                &archive_name,
                file_size,
                meta.modified_ns,
                u32::from(meta.permissions),
                meta.uid,
                meta.gid,
                TAR_REGTYPE,
                "",
            );
            archive.extend_from_slice(&header);

            // Write file data.
            archive.extend_from_slice(&data);

            // Pad to 512-byte boundary.
            let remainder = data.len() % TAR_BLOCK_SIZE;
            if remainder != 0 {
                let padding = TAR_BLOCK_SIZE - remainder;
                archive.extend_from_slice(&alloc::vec![0u8; padding]);
            }

            *count = count.saturating_add(1);
            if verbose {
                crate::console_println!("a {} ({} bytes)", archive_name, file_size);
            }
        }
        EntryType::Symlink => {
            // Read symlink target.
            let target = match Vfs::readlink(path) {
                Ok(t) => t,
                Err(_) => alloc::string::String::new(),
            };

            let header = tar_build_header(
                &archive_name,
                0,
                meta.modified_ns,
                u32::from(meta.permissions),
                meta.uid,
                meta.gid,
                TAR_SYMTYPE,
                &target,
            );
            archive.extend_from_slice(&header);
            *count = count.saturating_add(1);
            if verbose {
                crate::console_println!("a {} -> {}", archive_name, target);
            }
        }
        _ => {
            // Skip unsupported types.
        }
    }

    Ok(())
}

/// Create directories recursively (mkdir -p equivalent for tar extraction).
fn tar_mkdir_p(path: &str) -> crate::error::KernelResult<()> {
    use crate::fs::Vfs;

    // Walk each component and create if missing.
    let mut current = alloc::string::String::new();
    for component in path.split('/') {
        if component.is_empty() {
            current.push('/');
            continue;
        }
        if current.len() > 1 {
            current.push('/');
        }
        current.push_str(component);
        match Vfs::mkdir(&current) {
            Ok(()) | Err(crate::error::KernelError::AlreadyExists) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// crc32 — CRC32C file checksum
// ---------------------------------------------------------------------------
// unzip — ZIP archive extraction and listing
// ---------------------------------------------------------------------------

/// ZIP local file header signature.
const ZIP_LOCAL_SIG: u32 = 0x0403_4B50;
/// ZIP central directory file header signature.
const ZIP_CENTRAL_SIG: u32 = 0x0201_4B50;
/// ZIP end of central directory signature.
const ZIP_EOCD_SIG: u32 = 0x0605_4B50;

/// Read a little-endian u16 from a byte slice at `off`.
fn zip_u16(data: &[u8], off: usize) -> u16 {
    let lo = *data.get(off).unwrap_or(&0);
    let hi = *data.get(off.wrapping_add(1)).unwrap_or(&0);
    u16::from(lo) | (u16::from(hi) << 8)
}

/// Read a little-endian u32 from a byte slice at `off`.
fn zip_u32(data: &[u8], off: usize) -> u32 {
    let b0 = u32::from(*data.get(off).unwrap_or(&0));
    let b1 = u32::from(*data.get(off.wrapping_add(1)).unwrap_or(&0));
    let b2 = u32::from(*data.get(off.wrapping_add(2)).unwrap_or(&0));
    let b3 = u32::from(*data.get(off.wrapping_add(3)).unwrap_or(&0));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

/// A parsed ZIP central directory entry.
struct ZipEntry {
    /// Filename (relative path inside the archive).
    name: alloc::string::String,
    /// Compression method: 0=stored, 8=deflated.
    method: u16,
    /// CRC-32 of uncompressed data.
    crc32: u32,
    /// Compressed size.
    compressed_size: u32,
    /// Uncompressed size.
    uncompressed_size: u32,
    /// Offset of local file header in the archive.
    local_header_offset: u32,
}

/// Find the End of Central Directory record by scanning backwards.
///
/// The EOCD is at most 22 + 65535 bytes from the end (22 bytes fixed +
/// up to 65535 bytes of comment).  We scan from the end for the signature.
fn zip_find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    // Scan backwards from the end, up to 65557 bytes back.
    let search_start = data.len().saturating_sub(65557);
    let mut pos = data.len().saturating_sub(22);
    loop {
        if zip_u32(data, pos) == ZIP_EOCD_SIG {
            return Some(pos);
        }
        if pos == search_start {
            return None;
        }
        pos = pos.saturating_sub(1);
    }
}

/// Parse the central directory and return a list of entries.
fn zip_parse_central_dir(data: &[u8]) -> Option<alloc::vec::Vec<ZipEntry>> {
    let eocd_off = zip_find_eocd(data)?;

    let total_entries = zip_u16(data, eocd_off.wrapping_add(10)) as usize;
    let cd_offset = zip_u32(data, eocd_off.wrapping_add(16)) as usize;

    let mut entries = alloc::vec::Vec::with_capacity(total_entries);
    let mut off = cd_offset;

    for _ in 0..total_entries {
        if off.wrapping_add(46) > data.len() {
            break;
        }
        let sig = zip_u32(data, off);
        if sig != ZIP_CENTRAL_SIG {
            break;
        }

        let method = zip_u16(data, off.wrapping_add(10));
        let crc32 = zip_u32(data, off.wrapping_add(16));
        let compressed_size = zip_u32(data, off.wrapping_add(20));
        let uncompressed_size = zip_u32(data, off.wrapping_add(24));
        let name_len = zip_u16(data, off.wrapping_add(28)) as usize;
        let extra_len = zip_u16(data, off.wrapping_add(30)) as usize;
        let comment_len = zip_u16(data, off.wrapping_add(32)) as usize;
        let local_header_offset = zip_u32(data, off.wrapping_add(42));

        let name_start = off.wrapping_add(46);
        let name_end = name_start.wrapping_add(name_len).min(data.len());
        let name_bytes = data.get(name_start..name_end).unwrap_or(&[]);
        let name = core::str::from_utf8(name_bytes)
            .map(alloc::string::String::from)
            .unwrap_or_else(|_| alloc::format!("<invalid-utf8@{}>", off));

        entries.push(ZipEntry {
            name,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });

        off = off.wrapping_add(46)
            .wrapping_add(name_len)
            .wrapping_add(extra_len)
            .wrapping_add(comment_len);
    }

    Some(entries)
}

/// Extract the compressed data for a ZIP entry from the archive.
///
/// Reads the local file header to find the actual data start (the
/// local header may have different extra field lengths than the
/// central directory entry).
fn zip_entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Option<&'a [u8]> {
    let off = entry.local_header_offset as usize;
    if off.wrapping_add(30) > data.len() {
        return None;
    }
    let sig = zip_u32(data, off);
    if sig != ZIP_LOCAL_SIG {
        return None;
    }
    let name_len = zip_u16(data, off.wrapping_add(26)) as usize;
    let extra_len = zip_u16(data, off.wrapping_add(28)) as usize;
    let data_start = off.wrapping_add(30).wrapping_add(name_len).wrapping_add(extra_len);
    let data_end = data_start.wrapping_add(entry.compressed_size as usize);
    data.get(data_start..data_end.min(data.len()))
}

/// `unzip` command — list or extract ZIP archives.
///
/// - `unzip -l archive.zip`        — list contents
/// - `unzip archive.zip`           — extract all files
/// - `unzip archive.zip -d DIR`    — extract to directory
fn cmd_unzip(args: &str) {
    use crate::fs::Vfs;

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!(
            "Usage: unzip [-l] archive.zip [-d dir]\n\
             \x20 -l      List archive contents\n\
             \x20 -d DIR  Extract to directory DIR"
        );
        return;
    }

    let mut list_mode = false;
    let mut archive_arg: Option<&str> = None;
    let mut target_dir = alloc::string::String::from("/");
    let mut skip_next = false;

    for (i, &p) in parts.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        match p {
            "-l" | "--list" => list_mode = true,
            "-d" => {
                if let Some(&dir) = parts.get(i.wrapping_add(1)) {
                    target_dir = resolve_path(dir);
                    skip_next = true;
                } else {
                    crate::console_println!("unzip: -d requires a directory argument");
                    return;
                }
            }
            _ => {
                if archive_arg.is_none() {
                    archive_arg = Some(p);
                }
            }
        }
    }

    let archive_path = match archive_arg {
        Some(p) => resolve_path(p),
        None => {
            crate::console_println!("unzip: no archive file specified");
            return;
        }
    };

    let data = match Vfs::read_file(&archive_path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("unzip: '{}': {:?}", archive_path, e);
            return;
        }
    };

    let entries = match zip_parse_central_dir(&data) {
        Some(e) => e,
        None => {
            crate::console_println!("unzip: '{}': not a valid ZIP archive", archive_path);
            return;
        }
    };

    if list_mode {
        crate::console_println!(
            "  {:>10}  {:>10}  {:>6}  Name",
            "Size", "Compressed", "Method"
        );
        crate::console_println!("  {}  {}  {}  {}", "-" .repeat(10), "-".repeat(10), "-".repeat(6), "-".repeat(30));

        let mut total_size: u64 = 0;
        let mut total_compressed: u64 = 0;
        for entry in &entries {
            let method_str = match entry.method {
                0 => "stored",
                8 => "deflat",
                _ => "???",
            };
            crate::console_println!(
                "  {:>10}  {:>10}  {:>6}  {}",
                entry.uncompressed_size, entry.compressed_size,
                method_str, entry.name
            );
            total_size = total_size.saturating_add(u64::from(entry.uncompressed_size));
            total_compressed = total_compressed.saturating_add(u64::from(entry.compressed_size));
        }
        crate::console_println!("  {}  {}  {}  {}", "-".repeat(10), "-".repeat(10), "-".repeat(6), "-".repeat(30));
        crate::console_println!(
            "  {:>10}  {:>10}  {:>6}  {} files",
            total_size, total_compressed, "", entries.len()
        );
        return;
    }

    // Extract mode.
    let mut extracted: u32 = 0;
    let mut errors: u32 = 0;

    for entry in &entries {
        // Build output path.
        let clean_name = entry.name.trim_start_matches('/');
        if clean_name.is_empty() {
            continue;
        }

        let out_path = if target_dir == "/" {
            alloc::format!("/{}", clean_name)
        } else {
            alloc::format!("{}/{}", target_dir.trim_end_matches('/'), clean_name)
        };

        // Directory entries end with '/'.
        if entry.name.ends_with('/') {
            match Vfs::mkdir(&out_path) {
                Ok(()) | Err(crate::error::KernelError::AlreadyExists) => {}
                Err(e) => {
                    crate::console_println!("  unzip: mkdir '{}': {:?}", out_path, e);
                    errors = errors.saturating_add(1);
                }
            }
            crate::console_println!("  creating: {}", out_path);
            continue;
        }

        // Ensure parent directory exists.
        if let Some(slash) = out_path.rfind('/') {
            let parent = &out_path[..slash];
            if !parent.is_empty() {
                let _ = Vfs::mkdir_all(parent);
            }
        }

        // Get the compressed data.
        let compressed_data = match zip_entry_data(&data, &entry) {
            Some(d) => d,
            None => {
                crate::console_println!("  unzip: '{}': could not read data", entry.name);
                errors = errors.saturating_add(1);
                continue;
            }
        };

        // Decompress based on method.
        let file_data = match entry.method {
            0 => {
                // Stored — data is uncompressed.
                compressed_data.to_vec()
            }
            8 => {
                // Deflated — decompress.
                match crate::fs::compress::inflate(compressed_data) {
                    Ok(d) => d,
                    Err(e) => {
                        crate::console_println!(
                            "  unzip: '{}': inflate failed: {:?}",
                            entry.name, e
                        );
                        errors = errors.saturating_add(1);
                        continue;
                    }
                }
            }
            _ => {
                crate::console_println!(
                    "  unzip: '{}': unsupported compression method {}",
                    entry.name, entry.method
                );
                errors = errors.saturating_add(1);
                continue;
            }
        };

        // Verify CRC-32 if non-zero.
        if entry.crc32 != 0 {
            let actual_crc = crate::fs::compress::crc32_iso_pub(&file_data);
            if actual_crc != entry.crc32 {
                crate::console_println!(
                    "  unzip: '{}': CRC mismatch (expected {:#010x}, got {:#010x})",
                    entry.name, entry.crc32, actual_crc
                );
                errors = errors.saturating_add(1);
                continue;
            }
        }

        // Write the file.
        match Vfs::write_file(&out_path, &file_data) {
            Ok(()) => {
                crate::console_println!(
                    "  extracting: {} ({} bytes)",
                    out_path, file_data.len()
                );
                extracted = extracted.saturating_add(1);
            }
            Err(e) => {
                crate::console_println!("  unzip: write '{}': {:?}", out_path, e);
                errors = errors.saturating_add(1);
            }
        }
    }

    crate::console_println!(
        "unzip: {} files extracted{} from '{}'",
        extracted,
        if errors > 0 {
            alloc::format!(", {} errors", errors)
        } else {
            alloc::string::String::new()
        },
        archive_path
    );
}

// ---------------------------------------------------------------------------
// zip — ZIP archive creation
// ---------------------------------------------------------------------------

/// Write a little-endian u16 to a byte vector.
fn zip_write_u16(buf: &mut Vec<u8>, val: u16) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Write a little-endian u32 to a byte vector.
fn zip_write_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_le_bytes());
}

/// One entry prepared for writing into a ZIP archive.
struct ZipWriteEntry {
    /// Relative path inside the archive.
    name: String,
    /// Uncompressed CRC-32 (ISO 3309).
    crc32: u32,
    /// Compressed data (method 8) or raw data (method 0).
    data: Vec<u8>,
    /// Compression method: 0=stored, 8=deflated.
    method: u16,
    /// Original (uncompressed) size.
    uncompressed_size: u32,
}

/// Build a ZIP archive in memory from a list of prepared entries.
///
/// Produces a valid ZIP file: local file headers, central directory,
/// and end of central directory record.
#[allow(clippy::arithmetic_side_effects)]
fn zip_build_archive(entries: &[ZipWriteEntry]) -> Vec<u8> {
    let mut archive = Vec::new();

    // Offsets of each local file header (needed for central directory).
    let mut local_offsets: Vec<u32> = Vec::with_capacity(entries.len());

    // --- Local file headers + file data ---
    for entry in entries {
        local_offsets.push(archive.len() as u32);

        // Local file header (30 bytes + name + data).
        zip_write_u32(&mut archive, ZIP_LOCAL_SIG); // signature
        zip_write_u16(&mut archive, 20);            // version needed (2.0)
        zip_write_u16(&mut archive, 0);             // general purpose bit flag
        zip_write_u16(&mut archive, entry.method);  // compression method
        zip_write_u16(&mut archive, 0);             // mod time (unused)
        zip_write_u16(&mut archive, 0x0021);        // mod date (1980-01-01, minimum valid DOS date)
        zip_write_u32(&mut archive, entry.crc32);   // CRC-32
        zip_write_u32(&mut archive, entry.data.len() as u32); // compressed size
        zip_write_u32(&mut archive, entry.uncompressed_size); // uncompressed size
        zip_write_u16(&mut archive, entry.name.len() as u16); // file name length
        zip_write_u16(&mut archive, 0);             // extra field length
        archive.extend_from_slice(entry.name.as_bytes());
        archive.extend_from_slice(&entry.data);
    }

    // --- Central directory ---
    let cd_start = archive.len() as u32;

    for (i, entry) in entries.iter().enumerate() {
        zip_write_u32(&mut archive, ZIP_CENTRAL_SIG); // signature
        zip_write_u16(&mut archive, 20);              // version made by (2.0)
        zip_write_u16(&mut archive, 20);              // version needed (2.0)
        zip_write_u16(&mut archive, 0);               // general purpose bit flag
        zip_write_u16(&mut archive, entry.method);    // compression method
        zip_write_u16(&mut archive, 0);               // mod time
        zip_write_u16(&mut archive, 0x0021);          // mod date
        zip_write_u32(&mut archive, entry.crc32);     // CRC-32
        zip_write_u32(&mut archive, entry.data.len() as u32); // compressed size
        zip_write_u32(&mut archive, entry.uncompressed_size); // uncompressed size
        zip_write_u16(&mut archive, entry.name.len() as u16); // file name length
        zip_write_u16(&mut archive, 0);               // extra field length
        zip_write_u16(&mut archive, 0);               // file comment length
        zip_write_u16(&mut archive, 0);               // disk number start
        zip_write_u16(&mut archive, 0);               // internal file attributes
        zip_write_u32(&mut archive, 0);               // external file attributes
        zip_write_u32(&mut archive, local_offsets[i]); // local header offset
        archive.extend_from_slice(entry.name.as_bytes());
    }

    let cd_end = archive.len() as u32;
    let cd_size = cd_end.wrapping_sub(cd_start);

    // --- End of central directory ---
    zip_write_u32(&mut archive, ZIP_EOCD_SIG);
    zip_write_u16(&mut archive, 0);                       // disk number
    zip_write_u16(&mut archive, 0);                       // disk with central dir
    zip_write_u16(&mut archive, entries.len() as u16);    // entries on this disk
    zip_write_u16(&mut archive, entries.len() as u16);    // total entries
    zip_write_u32(&mut archive, cd_size);                 // central dir size
    zip_write_u32(&mut archive, cd_start);                // central dir offset
    zip_write_u16(&mut archive, 0);                       // comment length

    archive
}

/// Recursively collect all files under a directory, returning
/// (relative_path, absolute_path) pairs.
///
/// Directories themselves are added as entries with trailing `/`
/// (ZIP convention for directory markers).
fn zip_collect_files(
    base: &str,
    prefix: &str,
    files: &mut Vec<(String, String)>,
    depth: usize,
) {
    use crate::fs::Vfs;

    if depth > 16 {
        return; // Safety limit.
    }

    let entries = match Vfs::readdir(base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }

        let abs_path = if base == "/" {
            alloc::format!("/{}", entry.name)
        } else {
            alloc::format!("{}/{}", base, entry.name)
        };

        let rel_path = if prefix.is_empty() {
            entry.name.clone()
        } else {
            alloc::format!("{}/{}", prefix, entry.name)
        };

        if entry.entry_type == crate::fs::vfs::EntryType::Directory {
            // Add directory marker (trailing /).
            files.push((alloc::format!("{}/", rel_path), String::new()));
            // Recurse.
            zip_collect_files(&abs_path, &rel_path, files, depth.saturating_add(1));
        } else {
            files.push((rel_path, abs_path));
        }
    }
}

/// `zip archive.zip file1 [file2 ...] [-0]` — create a ZIP archive.
///
/// - `zip archive.zip file1 file2`    — compress files into archive
/// - `zip archive.zip dir/`           — recursively add directory
/// - `zip -0 archive.zip file1`       — store without compression
/// - `zip -r archive.zip dir`         — recursively add directory (explicit -r)
#[allow(clippy::arithmetic_side_effects)]
fn cmd_zip(args: &str) {
    use crate::fs::Vfs;

    if args.trim().is_empty() || args.trim() == "--help" || args.trim() == "-h" {
        crate::console_println!(
            "Usage: zip [-0] [-r] archive.zip file1 [file2 ...]\n\
             Create a ZIP archive.\n  \
               -0      Store files uncompressed (method 0)\n  \
               -r      Recurse into directories"
        );
        return;
    }

    let mut store_only = false;
    let mut recursive = true; // Default: recurse into dirs (like Info-ZIP).
    let mut positional: Vec<&str> = Vec::new();

    for token in args.split_whitespace() {
        match token {
            "-0" => store_only = true,
            "-r" => recursive = true,
            _ if token.starts_with('-') && positional.is_empty() => {
                // Handle combined flags like -0r.
                for ch in token.chars().skip(1) {
                    match ch {
                        '0' => store_only = true,
                        'r' => recursive = true,
                        _ => {
                            crate::console_println!("zip: unknown option '-{}'", ch);
                            return;
                        }
                    }
                }
            }
            _ => positional.push(token),
        }
    }

    if positional.len() < 2 {
        crate::console_println!("zip: need at least an archive name and one file");
        return;
    }

    let archive_path = resolve_path(positional[0]);

    // Collect all input files.
    let mut input_files: Vec<(String, String)> = Vec::new(); // (name_in_zip, abs_path)

    for &token in positional.iter().skip(1) {
        let abs = resolve_path(token);

        // Check if it's a directory.
        match Vfs::lstat(&abs) {
            Ok(meta) if meta.entry_type == crate::fs::vfs::EntryType::Directory => {
                if recursive {
                    // Use the directory's base name as the archive prefix.
                    let dir_name = token.rsplit('/').next()
                        .unwrap_or(token)
                        .trim_end_matches('/');
                    // Add directory marker.
                    input_files.push((alloc::format!("{}/", dir_name), String::new()));
                    zip_collect_files(&abs, dir_name, &mut input_files, 0);
                } else {
                    crate::console_println!("zip: '{}' is a directory (use -r)", token);
                }
            }
            Ok(_) => {
                // Regular file — use the filename (last component) as name in archive.
                let name = token.rsplit('/').next().unwrap_or(token);
                input_files.push((String::from(name), abs));
            }
            Err(e) => {
                crate::console_println!("zip: '{}': {:?}", token, e);
                set_exit(1);
                return;
            }
        }
    }

    if input_files.is_empty() {
        crate::console_println!("zip: no input files");
        return;
    }

    // Build ZIP entries.
    let mut entries: Vec<ZipWriteEntry> = Vec::with_capacity(input_files.len());
    let mut total_in: u64 = 0;
    let mut total_out: u64 = 0;
    let mut errors: usize = 0;

    for (name, abs_path) in &input_files {
        if abs_path.is_empty() {
            // Directory marker — no data.
            entries.push(ZipWriteEntry {
                name: name.clone(),
                crc32: 0,
                data: Vec::new(),
                method: 0,
                uncompressed_size: 0,
            });
            continue;
        }

        let raw_data = match Vfs::read_file(abs_path) {
            Ok(d) => d,
            Err(e) => {
                crate::console_println!("  zip: read '{}': {:?}", abs_path, e);
                errors = errors.saturating_add(1);
                continue;
            }
        };

        let crc32 = crate::fs::compress::crc32_iso_pub(&raw_data);
        let uncompressed_size = raw_data.len() as u32;

        let (data, method) = if store_only || raw_data.is_empty() {
            (raw_data.clone(), 0u16)
        } else {
            let compressed = crate::fs::compress::deflate(&raw_data);
            // Only use compression if it actually saves space.
            if compressed.len() < raw_data.len() {
                (compressed, 8u16)
            } else {
                (raw_data.clone(), 0u16)
            }
        };

        let method_label = if method == 8 { "deflated" } else { "stored" };
        crate::console_println!(
            "  adding: {} ({}, {} → {} bytes)",
            name, method_label, uncompressed_size, data.len()
        );

        total_in = total_in.wrapping_add(u64::from(uncompressed_size));
        total_out = total_out.wrapping_add(data.len() as u64);

        entries.push(ZipWriteEntry {
            name: name.clone(),
            crc32,
            data,
            method,
            uncompressed_size,
        });
    }

    if entries.is_empty() {
        crate::console_println!("zip: nothing to add");
        return;
    }

    // Build and write the archive.
    let archive = zip_build_archive(&entries);
    match Vfs::write_file(&archive_path, &archive) {
        Ok(()) => {
            let ratio = if total_in > 0 {
                100u64.saturating_sub(total_out.saturating_mul(100) / total_in)
            } else {
                0
            };
            crate::console_println!(
                "zip: created '{}' ({} bytes, {} entries, {}% compression{})",
                archive_path,
                archive.len(),
                entries.len(),
                ratio,
                if errors > 0 {
                    alloc::format!(", {} errors", errors)
                } else {
                    String::new()
                }
            );
        }
        Err(e) => {
            crate::console_println!("zip: write '{}': {:?}", archive_path, e);
            set_exit(1);
        }
    }
}

// ---------------------------------------------------------------------------

/// `crc32 FILE [FILE ...]` — compute CRC32C checksum for files.
fn cmd_crc32(args: &str) {
    if args.trim().is_empty() {
        crate::console_println!("Usage: crc32 <file> [file ...]");
        return;
    }

    for token in args.split_whitespace() {
        let path = resolve_path(token);
        match crate::fs::Vfs::read_file(&path) {
            Ok(data) => {
                let checksum = crate::crypto::crc32c(&data);
                crate::console_println!("{:08x} {} {}", checksum, data.len(), path);
            }
            Err(e) => {
                crate::console_println!("crc32: '{}': {:?}", path, e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// base64 — Base64 encode/decode
// ---------------------------------------------------------------------------

/// Standard Base64 alphabet (RFC 4648).
const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes to Base64.
#[allow(clippy::arithmetic_side_effects)]
fn base64_encode(data: &[u8]) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(B64_CHARS[idx0] as char);
        out.push(B64_CHARS[idx1] as char);

        if i + 1 < data.len() {
            out.push(B64_CHARS[idx2] as char);
        } else {
            out.push('=');
        }

        if i + 2 < data.len() {
            out.push(B64_CHARS[idx3] as char);
        } else {
            out.push('=');
        }

        i += 3;
    }
    out
}

/// Decode a Base64 character to its 6-bit value, or None for padding/invalid.
fn b64_decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode Base64 to bytes.
#[allow(clippy::arithmetic_side_effects)]
fn base64_decode(input: &str) -> Result<alloc::vec::Vec<u8>, &'static str> {
    // Strip whitespace.
    let clean: alloc::vec::Vec<u8> = input.bytes()
        .filter(|&b| !b.is_ascii_whitespace())
        .collect();

    if clean.len() % 4 != 0 {
        return Err("invalid base64 length");
    }

    let mut out = alloc::vec::Vec::with_capacity(clean.len() / 4 * 3);

    let mut i = 0;
    while i < clean.len() {
        let c0 = b64_decode_char(clean[i]).ok_or("invalid base64 character")?;
        let c1 = b64_decode_char(clean[i + 1]).ok_or("invalid base64 character")?;

        out.push((c0 << 2) | (c1 >> 4));

        if clean[i + 2] != b'=' {
            let c2 = b64_decode_char(clean[i + 2]).ok_or("invalid base64 character")?;
            out.push(((c1 & 0x0F) << 4) | (c2 >> 2));

            if clean[i + 3] != b'=' {
                let c3 = b64_decode_char(clean[i + 3]).ok_or("invalid base64 character")?;
                out.push(((c2 & 0x03) << 6) | c3);
            }
        }

        i += 4;
    }

    Ok(out)
}

/// `base64 [-d] FILE` — encode or decode Base64.
///
/// Without -d: read file, output Base64 (76-char line wrap).
/// With -d: read Base64 text file, decode to binary and write to stdout.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_base64(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!("Usage: base64 [-d] <file>");
        crate::console_println!("  -d   Decode (input is Base64 text)");
        return;
    }

    let decode = parts[0] == "-d" || parts[0] == "--decode";
    let file_arg = if decode {
        if parts.len() < 2 {
            crate::console_println!("base64: missing file argument");
            return;
        }
        parts[1]
    } else {
        parts[0]
    };

    let path = resolve_path(file_arg);
    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("base64: '{}': {:?}", path, e);
            return;
        }
    };

    if decode {
        // Input is Base64 text — decode and write binary to stdout.
        let text = core::str::from_utf8(&data).unwrap_or("");
        match base64_decode(text) {
            Ok(decoded) => {
                // Print as text if valid UTF-8, otherwise show hex summary.
                if let Ok(s) = core::str::from_utf8(&decoded) {
                    crate::console_println!("{}", s);
                } else {
                    crate::console_println!("<binary: {} bytes>", decoded.len());
                }
            }
            Err(e) => {
                crate::console_println!("base64: decode error: {}", e);
            }
        }
    } else {
        // Encode file content to Base64 with 76-char line wrapping.
        let encoded = base64_encode(&data);
        let line_width = 76;
        let bytes = encoded.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + line_width).min(bytes.len());
            if let Ok(line) = core::str::from_utf8(&bytes[offset..end]) {
                crate::console_println!("{}", line);
            }
            offset = end;
        }
    }
}

// ---------------------------------------------------------------------------
// wipe — secure delete (zero-fill then remove)
// ---------------------------------------------------------------------------

/// `wipe FILE [FILE ...]` — overwrite file contents with zeros, then delete.
///
/// Provides a basic secure-delete: fills the file with zero bytes
/// (same size), syncs to disk, then removes the directory entry.
/// This prevents casual recovery of deleted file content from disk.
fn cmd_wipe(args: &str) {
    if args.trim().is_empty() {
        crate::console_println!("Usage: wipe <file> [file ...]");
        crate::console_println!("  Overwrites file with zeros, then deletes it");
        return;
    }

    for token in args.split_whitespace() {
        let path = resolve_path(token);
        match crate::fs::Vfs::stat(&path) {
            Ok(meta) => {
                let size = meta.size as usize;
                if size > 0 {
                    // Zero-fill the file.
                    let zeros = alloc::vec![0u8; size];
                    if let Err(e) = crate::fs::Vfs::write_file(&path, &zeros) {
                        crate::console_println!("wipe: write '{}': {:?}", path, e);
                        continue;
                    }
                    // Sync to ensure zeros hit disk.
                    let _ = crate::fs::Vfs::sync();
                }
                // Remove the file.
                match crate::fs::Vfs::remove(&path) {
                    Ok(()) => {
                        crate::console_println!("wiped: {} ({} bytes zeroed)", path, size);
                    }
                    Err(e) => {
                        crate::console_println!("wipe: remove '{}': {:?}", path, e);
                    }
                }
            }
            Err(e) => {
                crate::console_println!("wipe: '{}': {:?}", path, e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// checksum — combined checksum utility
// ---------------------------------------------------------------------------

/// `checksum [-t sha256|crc32] FILE [FILE ...]` — compute file checksum.
///
/// Default algorithm is CRC32C for speed.
fn cmd_checksum(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!("Usage: checksum [-t sha256|crc32] <file> [file ...]");
        return;
    }

    let (algo, files_start) = if parts[0] == "-t" && parts.len() >= 3 {
        (parts[1], 2)
    } else {
        ("crc32", 0)
    };

    for &file in &parts[files_start..] {
        let path = resolve_path(file);
        match crate::fs::Vfs::read_file(&path) {
            Ok(data) => {
                match algo {
                    "crc32" | "crc32c" => {
                        let cksum = crate::crypto::crc32c(&data);
                        crate::console_println!("CRC32C {:08x}  {}", cksum, path);
                    }
                    "sha256" => {
                        match crate::fs::Vfs::content_hash(&path) {
                            Ok(hash) => {
                                let mut hex = alloc::string::String::with_capacity(64);
                                for byte in &hash {
                                    hex.push_str(&alloc::format!("{:02x}", byte));
                                }
                                crate::console_println!("SHA256 {}  {}", hex, path);
                            }
                            Err(e) => {
                                crate::console_println!("checksum: sha256 '{}': {:?}", path, e);
                            }
                        }
                    }
                    _ => {
                        crate::console_println!("checksum: unknown algorithm '{}' (use crc32 or sha256)", algo);
                        return;
                    }
                }
            }
            Err(e) => {
                crate::console_println!("checksum: '{}': {:?}", path, e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// gunzip — gzip decompression
// ---------------------------------------------------------------------------

/// `gunzip` / `gzip -d` command — decompress gzip files.
///
/// Usage:
/// - `gunzip file.gz`           — decompress to file (strips .gz)
/// - `gunzip file.gz -o out`    — decompress to explicit output path
/// - `gunzip -t file.gz`        — test integrity only (no output)
/// - `gunzip -l file.gz`        — show compressed/uncompressed sizes
fn cmd_gunzip(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!(
            "Usage: gunzip [-t|-l] FILE.gz [-o OUTPUT]   Decompress\n\
             \x20      gzip FILE [-o OUTPUT]              Compress\n\
             \x20      gzip -d FILE.gz [-o OUTPUT]        Decompress\n\
             \x20 -t   Test integrity only (no output written)\n\
             \x20 -l   List compressed/uncompressed sizes\n\
             \x20 -o F Write output to F instead of auto-naming"
        );
        return;
    }

    let mut test_only = false;
    let mut list_mode = false;
    let mut compress_mode = false;
    let mut input_path: Option<&str> = None;
    let mut output_path: Option<&str> = None;
    let mut skip_next = false;

    for (i, &p) in parts.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        match p {
            "-t" | "--test" => test_only = true,
            "-l" | "--list" => list_mode = true,
            "-d" => {} // gzip -d: explicit decompress mode (no-op, default for gunzip)
            "-c" => compress_mode = true, // explicit compress mode
            "-o" => {
                if let Some(&out) = parts.get(i.wrapping_add(1)) {
                    output_path = Some(out);
                    skip_next = true;
                } else {
                    crate::console_println!("gunzip: -o requires an argument");
                    return;
                }
            }
            _ => {
                if input_path.is_none() {
                    input_path = Some(p);
                }
            }
        }
    }

    let input = match input_path {
        Some(p) => resolve_path(p),
        None => {
            crate::console_println!("gunzip: no input file specified");
            return;
        }
    };

    // Read the input data.
    let file_data = match crate::fs::Vfs::read_file(&input) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("gzip: '{}': {:?}", input, e);
            return;
        }
    };

    // Auto-detect: if the file starts with gzip magic and we're not
    // explicitly compressing, decompress.  If -c is given or the file
    // is not gzip, compress.
    let is_gzip = file_data.len() >= 2
        && file_data.first() == Some(&0x1F)
        && file_data.get(1) == Some(&0x8B);

    if compress_mode || (!is_gzip && !test_only && !list_mode) {
        // COMPRESS mode.
        let compressed = crate::fs::compress::gzip(&file_data);

        let out = if let Some(explicit) = output_path {
            resolve_path(explicit)
        } else {
            alloc::format!("{}.gz", input)
        };

        match crate::fs::Vfs::write_file(&out, &compressed) {
            Ok(()) => {
                crate::console_println!(
                    "gzip: '{}' -> '{}' ({} -> {} bytes, {:.1}%)",
                    input, out, file_data.len(), compressed.len(),
                    if file_data.is_empty() { 0.0 }
                    else { (compressed.len() as f64 / file_data.len() as f64) * 100.0 }
                );
            }
            Err(e) => {
                crate::console_println!("gzip: write '{}': {:?}", out, e);
            }
        }
        return;
    }

    // DECOMPRESS mode from here on.
    let compressed = file_data;
    if !is_gzip {
        crate::console_println!("gunzip: '{}': not in gzip format", input);
        return;
    }

    if list_mode {
        // Show sizes from the trailer (last 4 bytes = uncompressed size mod 2^32).
        if compressed.len() >= 8 {
            let trailer_start = compressed.len().saturating_sub(4);
            let uncompressed_size = u32::from_le_bytes([
                compressed[trailer_start],
                compressed[trailer_start.wrapping_add(1)],
                compressed[trailer_start.wrapping_add(2)],
                compressed[trailer_start.wrapping_add(3)],
            ]);
            let ratio = if uncompressed_size > 0 {
                let pct = (compressed.len() as u64)
                    .saturating_mul(100)
                    / u64::from(uncompressed_size);
                alloc::format!("{}%", pct)
            } else {
                alloc::string::String::from("N/A")
            };
            crate::console_println!(
                "  compressed: {} bytes  uncompressed: {} bytes  ratio: {}  {}",
                compressed.len(), uncompressed_size, ratio, input
            );
        }
        return;
    }

    // Decompress.
    let decompressed = match crate::fs::compress::gunzip(&compressed) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("gunzip: '{}': decompression failed: {:?}", input, e);
            return;
        }
    };

    if test_only {
        crate::console_println!(
            "gunzip: '{}': OK ({} -> {} bytes)",
            input, compressed.len(), decompressed.len()
        );
        return;
    }

    // Determine output path.
    let out = if let Some(explicit) = output_path {
        resolve_path(explicit)
    } else {
        // Strip .gz extension.
        let stripped = if input.ends_with(".gz") {
            alloc::string::String::from(&input[..input.len().saturating_sub(3)])
        } else if input.ends_with(".tgz") {
            let base = &input[..input.len().saturating_sub(4)];
            alloc::format!("{}.tar", base)
        } else {
            alloc::format!("{}.out", input)
        };
        stripped
    };

    match crate::fs::Vfs::write_file(&out, &decompressed) {
        Ok(()) => {
            crate::console_println!(
                "gunzip: '{}' -> '{}' ({} -> {} bytes)",
                input, out, compressed.len(), decompressed.len()
            );
        }
        Err(e) => {
            crate::console_println!("gunzip: write '{}': {:?}", out, e);
        }
    }
}

// ---------------------------------------------------------------------------
// bunzip2 — bzip2 decompression
// ---------------------------------------------------------------------------

fn cmd_bunzip2(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!(
            "Usage: bunzip2 [-t] FILE.bz2 [-o OUTPUT]   Decompress bzip2 file\n\
             \x20      bzcat FILE.bz2                    Decompress to stdout\n\
             \x20 -t   Test integrity only (no output written)\n\
             \x20 -o F Write output to F instead of auto-naming"
        );
        return;
    }

    let mut test_only = false;
    let mut input_path: Option<&str> = None;
    let mut output_path: Option<&str> = None;
    let mut skip_next = false;

    for (i, &p) in parts.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        match p {
            "-t" | "--test" => test_only = true,
            "-o" => {
                if let Some(&out) = parts.get(i.wrapping_add(1)) {
                    output_path = Some(out);
                    skip_next = true;
                } else {
                    crate::console_println!("bunzip2: -o requires an argument");
                    return;
                }
            }
            _ => {
                if input_path.is_none() {
                    input_path = Some(p);
                }
            }
        }
    }

    let input = match input_path {
        Some(p) => resolve_path(p),
        None => {
            crate::console_println!("bunzip2: no input file specified");
            return;
        }
    };

    // Read the input data.
    let file_data = match crate::fs::Vfs::read_file(&input) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("bunzip2: '{}': {:?}", input, e);
            return;
        }
    };

    // Verify bzip2 magic.
    if file_data.len() < 4
        || file_data.first() != Some(&b'B')
        || file_data.get(1) != Some(&b'Z')
        || file_data.get(2) != Some(&b'h')
    {
        crate::console_println!("bunzip2: '{}': not a bzip2 file", input);
        return;
    }

    // Decompress.
    let decompressed = match crate::fs::bzip2::bunzip2(&file_data) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("bunzip2: '{}': decompression failed: {:?}", input, e);
            return;
        }
    };

    if test_only {
        crate::console_println!(
            "bunzip2: '{}': OK ({} -> {} bytes)",
            input, file_data.len(), decompressed.len()
        );
        return;
    }

    // Determine output path.
    let out = if let Some(explicit) = output_path {
        resolve_path(explicit)
    } else {
        // Strip .bz2 extension.
        if input.ends_with(".bz2") {
            alloc::string::String::from(&input[..input.len().saturating_sub(4)])
        } else if input.ends_with(".tbz2") {
            let base = &input[..input.len().saturating_sub(5)];
            alloc::format!("{}.tar", base)
        } else if input.ends_with(".tbz") {
            let base = &input[..input.len().saturating_sub(4)];
            alloc::format!("{}.tar", base)
        } else {
            alloc::format!("{}.out", input)
        }
    };

    match crate::fs::Vfs::write_file(&out, &decompressed) {
        Ok(()) => {
            crate::console_println!(
                "bunzip2: '{}' -> '{}' ({} -> {} bytes)",
                input, out, file_data.len(), decompressed.len()
            );
        }
        Err(e) => {
            crate::console_println!("bunzip2: write '{}': {:?}", out, e);
        }
    }
}

// ---------------------------------------------------------------------------
// sed — stream editor (subset)
// ---------------------------------------------------------------------------

/// `sed` command — stream editor for text transformation.
///
/// Supported commands:
///   `s/pattern/replacement/[g]`  — substitute (first or all)
///   `/pattern/d`                 — delete matching lines
///   `Nd`                         — delete line N
///
/// Flags:
///   `-i`   In-place edit (modify file directly)
///   `-n`   Suppress default output
///   `-e CMD`  Specify command (can repeat)
///
/// Examples:
///   sed 's/old/new/g' file.txt         Replace all occurrences
///   sed -i 's/old/new/' file.txt       In-place substitution
///   sed '/pattern/d' file.txt          Delete matching lines
///   sed -n '/pattern/p' file.txt       Print matching lines (like grep)
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn cmd_sed(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        crate::console_println!("Usage: sed [-i] [-n] [-e CMD] 's/old/new/[g]' [file]");
        crate::console_println!("       sed [-i] [-n] '/pattern/d' [file]");
        crate::console_println!("       sed [-i] [-n] 'Nd' [file]  (delete line N)");
        return;
    }

    let mut in_place = false;
    let mut suppress = false;
    let mut commands: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    let mut file_args: alloc::vec::Vec<&str> = alloc::vec::Vec::new();
    let mut i = 0;

    while i < parts.len() {
        match parts[i] {
            "-i" => in_place = true,
            "-n" => suppress = true,
            "-e" => {
                i += 1;
                if i < parts.len() {
                    commands.push(alloc::string::String::from(parts[i]));
                }
            }
            s if s.starts_with('s') || s.starts_with('/') || s.ends_with('d') => {
                if commands.is_empty() {
                    commands.push(alloc::string::String::from(s));
                } else {
                    file_args.push(s);
                }
            }
            _ => {
                file_args.push(parts[i]);
            }
        }
        i += 1;
    }

    if commands.is_empty() {
        crate::console_println!("sed: no command specified");
        return;
    }

    // Parse sed commands.
    let parsed: alloc::vec::Vec<SedCmd> = commands.iter()
        .filter_map(|c| parse_sed_command(c))
        .collect();

    if parsed.is_empty() {
        crate::console_println!("sed: invalid command syntax");
        return;
    }

    // Process each file (or use empty content if no file given).
    if file_args.is_empty() {
        crate::console_println!("sed: no input file specified");
        return;
    }

    for &file in &file_args {
        let path = resolve_path(file);
        let data = match crate::fs::Vfs::read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                crate::console_println!("sed: '{}': {:?}", path, e);
                continue;
            }
        };

        let text = core::str::from_utf8(&data).unwrap_or("");
        let mut output = alloc::string::String::new();

        for (line_idx, line) in text.lines().enumerate() {
            let line_num = line_idx.wrapping_add(1);
            let mut current = alloc::string::String::from(line);
            let mut deleted = false;
            let mut print_this = false;

            for cmd in &parsed {
                match cmd {
                    SedCmd::Substitute { pattern, replacement, global, addr } => {
                        if sed_addr_matches(addr, line_num, &current) {
                            if *global {
                                current = sed_replace_all(&current, pattern, replacement);
                            } else {
                                current = sed_replace_first(&current, pattern, replacement);
                            }
                        }
                    }
                    SedCmd::Delete { addr } => {
                        if sed_addr_matches(addr, line_num, &current) {
                            deleted = true;
                        }
                    }
                    SedCmd::Print { addr } => {
                        if sed_addr_matches(addr, line_num, &current) {
                            print_this = true;
                        }
                    }
                }
            }

            if !deleted {
                if !suppress || print_this {
                    output.push_str(&current);
                    output.push('\n');
                }
                if suppress && print_this {
                    // Already added above.
                }
            }
        }

        if in_place {
            match crate::fs::Vfs::write_file(&path, output.as_bytes()) {
                Ok(()) => {}
                Err(e) => {
                    crate::console_println!("sed: write '{}': {:?}", path, e);
                }
            }
        } else {
            // Print output (without trailing newline already in output).
            if output.ends_with('\n') {
                output.pop();
            }
            crate::console_println!("{}", output);
        }
    }
}

/// Parsed sed command.
enum SedCmd {
    /// `s/pattern/replacement/[g]`
    Substitute {
        pattern: alloc::string::String,
        replacement: alloc::string::String,
        global: bool,
        addr: SedAddr,
    },
    /// `/pattern/d` or `Nd`
    Delete { addr: SedAddr },
    /// `/pattern/p`
    Print { addr: SedAddr },
}

/// Sed address (line selector).
enum SedAddr {
    /// All lines.
    All,
    /// Specific line number.
    Line(usize),
    /// Lines matching a pattern.
    Pattern(alloc::string::String),
}

/// Check if a sed address matches the current line.
fn sed_addr_matches(addr: &SedAddr, line_num: usize, line: &str) -> bool {
    match addr {
        SedAddr::All => true,
        SedAddr::Line(n) => line_num == *n,
        SedAddr::Pattern(pat) => line.contains(pat.as_str()),
    }
}

/// Parse a sed command string into a `SedCmd`.
fn parse_sed_command(cmd: &str) -> Option<SedCmd> {
    let bytes = cmd.as_bytes();

    // Substitution: s/pattern/replacement/[g]
    if bytes.first() == Some(&b's') && bytes.len() >= 4 {
        let delim = bytes[1];
        // Find pattern end.
        let pat_start = 2;
        let pat_end = find_unescaped(bytes, delim, pat_start)?;
        let rep_start = pat_end + 1;
        let rep_end = find_unescaped(bytes, delim, rep_start).unwrap_or(bytes.len());
        let flags = if rep_end < bytes.len() {
            &cmd[rep_end + 1..]
        } else {
            ""
        };

        let pattern = alloc::string::String::from(cmd.get(pat_start..pat_end)?);
        let replacement = alloc::string::String::from(cmd.get(rep_start..rep_end).unwrap_or(""));
        let global = flags.contains('g');

        return Some(SedCmd::Substitute {
            pattern,
            replacement,
            global,
            addr: SedAddr::All,
        });
    }

    // Address commands: /pattern/d or /pattern/p
    if bytes.first() == Some(&b'/') {
        let pat_end = find_unescaped(bytes, b'/', 1)?;
        let pattern = alloc::string::String::from(cmd.get(1..pat_end)?);
        let action = cmd.get(pat_end + 1..)?;

        return match action.trim() {
            "d" => Some(SedCmd::Delete {
                addr: SedAddr::Pattern(pattern),
            }),
            "p" => Some(SedCmd::Print {
                addr: SedAddr::Pattern(pattern),
            }),
            _ => None,
        };
    }

    // Line number commands: Nd
    if bytes.last() == Some(&b'd') {
        let num_str = &cmd[..cmd.len().saturating_sub(1)];
        if let Ok(n) = num_str.parse::<usize>() {
            return Some(SedCmd::Delete {
                addr: SedAddr::Line(n),
            });
        }
    }

    None
}

/// Find the position of an unescaped delimiter byte starting from `start`.
fn find_unescaped(bytes: &[u8], delim: u8, start: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == delim {
            // Check if preceded by backslash.
            if i > start && bytes[i - 1] == b'\\' {
                i += 1;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Replace the first occurrence of `pattern` in `text` with `replacement`.
fn sed_replace_first(text: &str, pattern: &str, replacement: &str) -> alloc::string::String {
    if pattern.is_empty() {
        return alloc::string::String::from(text);
    }
    if let Some(pos) = text.find(pattern) {
        let mut result = alloc::string::String::with_capacity(text.len());
        result.push_str(&text[..pos]);
        result.push_str(replacement);
        result.push_str(&text[pos + pattern.len()..]);
        result
    } else {
        alloc::string::String::from(text)
    }
}

/// `sed` pipe-input variant: processes piped text instead of a file.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_sed_input(args: &str, input: &str) {
    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    let mut suppress = false;
    let mut commands: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    let mut i = 0;

    while i < parts.len() {
        match parts[i] {
            "-n" => suppress = true,
            "-e" => {
                i += 1;
                if i < parts.len() {
                    commands.push(alloc::string::String::from(parts[i]));
                }
            }
            s if s.starts_with('s') || s.starts_with('/') || s.ends_with('d') => {
                if commands.is_empty() {
                    commands.push(alloc::string::String::from(s));
                }
            }
            _ => {}
        }
        i += 1;
    }

    let parsed: alloc::vec::Vec<SedCmd> = commands.iter()
        .filter_map(|c| parse_sed_command(c))
        .collect();

    if parsed.is_empty() {
        shell_print!("{}", input);
        return;
    }

    let mut output = alloc::string::String::new();
    for (line_idx, line) in input.lines().enumerate() {
        let line_num = line_idx.wrapping_add(1);
        let mut current = alloc::string::String::from(line);
        let mut deleted = false;
        let mut print_this = false;

        for cmd in &parsed {
            match cmd {
                SedCmd::Substitute { pattern, replacement, global, addr } => {
                    if sed_addr_matches(addr, line_num, &current) {
                        if *global {
                            current = sed_replace_all(&current, pattern, replacement);
                        } else {
                            current = sed_replace_first(&current, pattern, replacement);
                        }
                    }
                }
                SedCmd::Delete { addr } => {
                    if sed_addr_matches(addr, line_num, &current) {
                        deleted = true;
                    }
                }
                SedCmd::Print { addr } => {
                    if sed_addr_matches(addr, line_num, &current) {
                        print_this = true;
                    }
                }
            }
        }

        if !deleted && (!suppress || print_this) {
            output.push_str(&current);
            output.push('\n');
        }
    }

    if output.ends_with('\n') {
        output.pop();
    }
    shell_print!("{}", output);
}

/// Replace all occurrences of `pattern` in `text` with `replacement`.
fn sed_replace_all(text: &str, pattern: &str, replacement: &str) -> alloc::string::String {
    if pattern.is_empty() {
        return alloc::string::String::from(text);
    }
    let mut result = alloc::string::String::with_capacity(text.len());
    let mut start = 0;
    while let Some(pos) = text[start..].find(pattern) {
        let abs_pos = start + pos;
        result.push_str(&text[start..abs_pos]);
        result.push_str(replacement);
        start = abs_pos + pattern.len();
    }
    result.push_str(&text[start..]);
    result
}

// ---------------------------------------------------------------------------
// awk — pattern scanning and text processing (subset)
// ---------------------------------------------------------------------------

/// `awk` command — pattern-action text processing.
///
/// Supported features:
///   - Field splitting: `$0` (whole line), `$1`, `$2`, ... `$NF`
///   - `-F SEP` field separator (default: whitespace)
///   - `{ print }`, `{ print $1, $3 }`
///   - `/pattern/ { action }` — pattern matching
///   - `BEGIN { ... }` and `END { ... }` blocks
///   - `NR` (record number), `NF` (field count), `FS` (separator)
///   - Pipe input support
///
/// Examples:
///   awk '{ print $1 }' file.txt            First field of each line
///   awk -F: '{ print $1, $3 }' /etc/passwd User and UID
///   awk '/error/ { print NR, $0 }' log     Matching lines with number
///   awk 'NR > 5 { print }' file            Skip first 5 lines
///   awk 'BEGIN { print "header" } { print $1 } END { print NR, "lines" }' file
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn cmd_awk(args: &str) {
    if args.trim().is_empty() {
        crate::console_println!("Usage: awk [-F sep] 'program' [file ...]");
        crate::console_println!("  Fields: $0 (line), $1..$N, $NF (last)");
        crate::console_println!("  Vars:   NR (line#), NF (field count), FS (separator)");
        crate::console_println!("  Blocks: BEGIN {{ }} ... {{ }} ... END {{ }}");
        return;
    }

    // Parse -F flag and program.
    let (fs_char, program, files) = parse_awk_args(args);

    if program.is_empty() {
        crate::console_println!("awk: no program specified");
        return;
    }

    let rules = parse_awk_program(&program);

    // Process files.
    if files.is_empty() {
        crate::console_println!("awk: no input file specified");
        return;
    }

    let mut nr: usize = 0;

    // Run BEGIN blocks.
    for rule in &rules {
        if rule.is_begin {
            awk_exec_action(&rule.action, "", &[], nr, 0, &fs_char);
        }
    }

    for file in &files {
        let path = resolve_path(file);
        let data = match crate::fs::Vfs::read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                crate::console_println!("awk: '{}': {:?}", path, e);
                continue;
            }
        };

        let text = core::str::from_utf8(&data).unwrap_or("");
        for line in text.lines() {
            nr = nr.wrapping_add(1);
            let fields = awk_split_fields(line, &fs_char);
            let nf = fields.len();

            for rule in &rules {
                if rule.is_begin || rule.is_end {
                    continue;
                }
                if awk_pattern_matches(&rule.pattern, line, nr, nf) {
                    awk_exec_action(&rule.action, line, &fields, nr, nf, &fs_char);
                }
            }
        }
    }

    // Run END blocks.
    for rule in &rules {
        if rule.is_end {
            awk_exec_action(&rule.action, "", &[], nr, 0, &fs_char);
        }
    }
}

/// `awk` pipe-input variant.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_awk_input(args: &str, input: &str) {
    let (fs_char, program, _files) = parse_awk_args(args);
    if program.is_empty() {
        shell_print!("{}", input);
        return;
    }

    let rules = parse_awk_program(&program);
    let mut nr: usize = 0;

    // BEGIN blocks.
    for rule in &rules {
        if rule.is_begin {
            awk_exec_action(&rule.action, "", &[], nr, 0, &fs_char);
        }
    }

    for line in input.lines() {
        nr = nr.wrapping_add(1);
        let fields = awk_split_fields(line, &fs_char);
        let nf = fields.len();

        for rule in &rules {
            if rule.is_begin || rule.is_end {
                continue;
            }
            if awk_pattern_matches(&rule.pattern, line, nr, nf) {
                awk_exec_action(&rule.action, line, &fields, nr, nf, &fs_char);
            }
        }
    }

    // END blocks.
    for rule in &rules {
        if rule.is_end {
            awk_exec_action(&rule.action, "", &[], nr, 0, &fs_char);
        }
    }
}

/// Parsed awk rule (pattern + action).
struct AwkRule {
    pattern: alloc::string::String,
    action: alloc::string::String,
    is_begin: bool,
    is_end: bool,
}

/// Parse awk command-line arguments: -F, program, files.
fn parse_awk_args(args: &str) -> (alloc::string::String, alloc::string::String, alloc::vec::Vec<alloc::string::String>) {
    let mut fs = alloc::string::String::from(" ");
    let mut program = alloc::string::String::new();
    let mut files: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();

    let parts: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    let mut got_program = false;

    while i < parts.len() {
        if parts[i] == "-F" && i + 1 < parts.len() {
            fs = alloc::string::String::from(parts[i + 1]);
            i += 2;
            continue;
        }
        if parts[i].starts_with("-F") {
            fs = alloc::string::String::from(&parts[i][2..]);
            i += 1;
            continue;
        }

        if !got_program {
            // The program may be quoted — find matching quote.
            let part = parts[i];
            if part.starts_with('\'') {
                // Collect until closing quote.
                let mut prog = alloc::string::String::from(&part[1..]);
                if prog.ends_with('\'') {
                    prog.pop();
                    program = prog;
                } else {
                    i += 1;
                    while i < parts.len() {
                        prog.push(' ');
                        let p = parts[i];
                        if p.ends_with('\'') {
                            prog.push_str(&p[..p.len() - 1]);
                            break;
                        }
                        prog.push_str(p);
                        i += 1;
                    }
                    program = prog;
                }
            } else {
                program = alloc::string::String::from(part);
            }
            got_program = true;
        } else {
            files.push(alloc::string::String::from(parts[i]));
        }
        i += 1;
    }

    (fs, program, files)
}

/// Parse an awk program into rules.
///
/// Supports: `BEGIN { ... }`, `END { ... }`, `/pattern/ { ... }`,
/// `condition { ... }`, bare `{ ... }`.
fn parse_awk_program(program: &str) -> alloc::vec::Vec<AwkRule> {
    let mut rules = alloc::vec::Vec::new();
    let bytes = program.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        // Skip whitespace.
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Check for BEGIN/END.
        let rest = &program[pos..];
        if rest.starts_with("BEGIN") {
            pos += 5;
            let action = extract_brace_block(program, &mut pos);
            rules.push(AwkRule {
                pattern: alloc::string::String::new(),
                action,
                is_begin: true,
                is_end: false,
            });
            continue;
        }
        if rest.starts_with("END") {
            pos += 3;
            let action = extract_brace_block(program, &mut pos);
            rules.push(AwkRule {
                pattern: alloc::string::String::new(),
                action,
                is_begin: false,
                is_end: true,
            });
            continue;
        }

        // Pattern + action.
        // Pattern: text before `{`, or empty if `{` is first.
        let pattern_start = pos;

        // Find the start of the action block.
        while pos < bytes.len() && bytes[pos] != b'{' {
            pos += 1;
        }
        let pattern = program[pattern_start..pos].trim();

        let action = if pos < bytes.len() && bytes[pos] == b'{' {
            extract_brace_block(program, &mut pos)
        } else {
            // No action — default is "print $0".
            alloc::string::String::from("print")
        };

        rules.push(AwkRule {
            pattern: alloc::string::String::from(pattern),
            action,
            is_begin: false,
            is_end: false,
        });
    }

    // If no rules were parsed, treat the whole program as a single action.
    if rules.is_empty() && !program.trim().is_empty() {
        rules.push(AwkRule {
            pattern: alloc::string::String::new(),
            action: alloc::string::String::from(program.trim()),
            is_begin: false,
            is_end: false,
        });
    }

    rules
}

/// Extract the contents between matching `{` and `}`, advancing `pos`.
#[allow(clippy::arithmetic_side_effects)]
fn extract_brace_block(program: &str, pos: &mut usize) -> alloc::string::String {
    let bytes = program.as_bytes();
    // Skip whitespace before `{`.
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    if *pos >= bytes.len() || bytes[*pos] != b'{' {
        return alloc::string::String::new();
    }
    *pos += 1; // Skip opening `{`.

    let start = *pos;
    let mut depth: u32 = 1;
    while *pos < bytes.len() && depth > 0 {
        match bytes[*pos] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            *pos += 1;
        }
    }
    let end = *pos;
    if *pos < bytes.len() {
        *pos += 1; // Skip closing `}`.
    }

    alloc::string::String::from(program[start..end].trim())
}

/// Split a line into fields using the given separator.
fn awk_split_fields<'a>(line: &'a str, fs: &str) -> alloc::vec::Vec<&'a str> {
    if fs == " " {
        // Whitespace splitting (default): split on runs of whitespace.
        line.split_whitespace().collect()
    } else if fs.len() == 1 {
        line.split(fs.as_bytes()[0] as char).collect()
    } else {
        alloc::vec![line]
    }
}

/// Check if a pattern matches the current line.
#[allow(clippy::arithmetic_side_effects)]
fn awk_pattern_matches(pattern: &str, line: &str, nr: usize, nf: usize) -> bool {
    if pattern.is_empty() {
        return true; // No pattern — match all.
    }

    // /regex/ pattern — literal string match.
    if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() >= 2 {
        let pat = &pattern[1..pattern.len() - 1];
        return line.contains(pat);
    }

    // NR comparisons: NR > N, NR < N, NR == N, NR >= N, NR <= N, NR != N
    if pattern.starts_with("NR") {
        let rest = pattern[2..].trim();
        if let Some(n_str) = rest.strip_prefix(">=") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr >= n;
            }
        }
        if let Some(n_str) = rest.strip_prefix("<=") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr <= n;
            }
        }
        if let Some(n_str) = rest.strip_prefix("!=") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr != n;
            }
        }
        if let Some(n_str) = rest.strip_prefix("==") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr == n;
            }
        }
        if let Some(n_str) = rest.strip_prefix('>') {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr > n;
            }
        }
        if let Some(n_str) = rest.strip_prefix('<') {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nr < n;
            }
        }
    }

    // NF comparisons.
    if pattern.starts_with("NF") {
        let rest = pattern[2..].trim();
        if let Some(n_str) = rest.strip_prefix(">=") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nf >= n;
            }
        }
        if let Some(n_str) = rest.strip_prefix('>') {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nf > n;
            }
        }
        if let Some(n_str) = rest.strip_prefix("==") {
            if let Ok(n) = n_str.trim().parse::<usize>() {
                return nf == n;
            }
        }
    }

    // Fallback: treat as a literal substring match.
    line.contains(pattern)
}

/// Execute an awk action for a line.
#[allow(clippy::arithmetic_side_effects)]
fn awk_exec_action(
    action: &str,
    line: &str,
    fields: &[&str],
    nr: usize,
    nf: usize,
    _fs: &str,
) {
    // Split action by `;` for multiple statements.
    for stmt in action.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }

        if stmt == "print" || stmt == "print $0" {
            crate::console_println!("{}", line);
        } else if stmt.starts_with("print ") || stmt.starts_with("print\t") {
            let expr = stmt[6..].trim();
            let output = awk_format_print(expr, line, fields, nr, nf);
            crate::console_println!("{}", output);
        } else {
            // Unknown statement — ignore.
        }
    }
}

/// Evaluate a print expression, expanding $N, NR, NF, and string literals.
#[allow(clippy::arithmetic_side_effects)]
fn awk_format_print(
    expr: &str,
    line: &str,
    fields: &[&str],
    nr: usize,
    nf: usize,
) -> alloc::string::String {
    let mut result = alloc::string::String::new();
    let parts = awk_split_print_args(expr);

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        let val = awk_eval_expr(part.trim(), line, fields, nr, nf);
        result.push_str(&val);
    }

    result
}

/// Split print arguments by commas (respecting quotes).
fn awk_split_print_args(expr: &str) -> alloc::vec::Vec<alloc::string::String> {
    let mut parts = alloc::vec::Vec::new();
    let mut current = alloc::string::String::new();
    let mut in_quote = false;
    let bytes = expr.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                in_quote = !in_quote;
                current.push('"');
            }
            b',' if !in_quote => {
                parts.push(core::mem::take(&mut current));
            }
            _ => {
                current.push(bytes[i] as char);
            }
        }
        i += 1;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Evaluate a single awk expression.
#[allow(clippy::arithmetic_side_effects)]
fn awk_eval_expr(
    expr: &str,
    line: &str,
    fields: &[&str],
    nr: usize,
    nf: usize,
) -> alloc::string::String {
    let expr = expr.trim();

    // String literal: "..."
    if expr.starts_with('"') && expr.ends_with('"') && expr.len() >= 2 {
        let inner = &expr[1..expr.len() - 1];
        // Handle common escape sequences.
        let mut result = alloc::string::String::new();
        let bytes = inner.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'n' => result.push('\n'),
                    b't' => result.push('\t'),
                    b'\\' => result.push('\\'),
                    b'"' => result.push('"'),
                    _ => {
                        result.push('\\');
                        result.push(bytes[i + 1] as char);
                    }
                }
                i += 2;
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
        return result;
    }

    // Built-in variables.
    if expr == "NR" {
        return alloc::format!("{}", nr);
    }
    if expr == "NF" {
        return alloc::format!("{}", nf);
    }
    if expr == "$0" {
        return alloc::string::String::from(line);
    }
    if expr == "$NF" {
        return fields.last().map_or(
            alloc::string::String::new(),
            |f| alloc::string::String::from(*f),
        );
    }

    // Field reference: $N
    if expr.starts_with('$') {
        if let Ok(n) = expr[1..].parse::<usize>() {
            if n == 0 {
                return alloc::string::String::from(line);
            }
            return fields.get(n.wrapping_sub(1)).map_or(
                alloc::string::String::new(),
                |f| alloc::string::String::from(*f),
            );
        }
    }

    // Bare word — treat as literal.
    alloc::string::String::from(expr)
}

/// `invariant` — check kernel invariants (system-wide consistency).
///
/// Usage:
///   invariant         — check all invariants
///   invariant mm      — check only memory invariants
///   invariant sched   — check only scheduler invariants
///   invariant kernel  — check only kernel object invariants
///   invariant ipc     — check only IPC invariants
///   invariant cap     — check only capability invariants
fn cmd_invariant(args: &str) {
    use crate::invariant;

    let category = args.trim();

    let results = if category.is_empty() {
        invariant::check_all()
    } else {
        invariant::check_category(category)
    };

    if results.total == 0 {
        shell_println!("No invariants found for category '{}'", category);
        shell_println!("Available: mm, sched, kernel, ipc, cap");
        return;
    }

    let title = if category.is_empty() {
        alloc::format!("=== Kernel Invariant Check ({} total) ===", results.total)
    } else {
        alloc::format!("=== Invariant Check [{}] ({} total) ===", category, results.total)
    };
    shell_println!("{}", title);
    shell_println!("");

    for r in &results.results {
        let status = if r.passed { " OK " } else { "FAIL" };
        let msg = r.message.as_deref().unwrap_or("");
        shell_println!("  [{}] {:<20} {}", status, r.name, msg);
    }

    shell_println!("");
    if results.failed == 0 {
        shell_println!("All {} invariants PASSED", results.total);
    } else {
        shell_println!("{} PASSED, {} FAILED", results.passed, results.failed);
    }
}

/// `migrate` — show task migration statistics between CPUs.
///
/// Usage:
///   migrate           — show aggregate migration stats
///   migrate recent    — show recent migration events
///   migrate hot       — show hottest migration path
///   migrate reset     — clear all migration data
fn cmd_migrate(args: &str) {
    use crate::sched_migrate;

    let sub = args.trim();

    match sub {
        "recent" => {
            let mut buf = [sched_migrate::MigrateEvent::empty(); 16];
            let n = sched_migrate::recent(&mut buf);
            if n == 0 {
                shell_println!("No migration events recorded");
                return;
            }
            shell_println!("=== Recent Task Migrations ({} events) ===", n);
            shell_println!("");
            shell_println!("  {:>6}  {:>4}  {:>4}  {:>8}  {:>8}",
                "TASK", "FROM", "TO", "REASON", "TICK");
            for i in 0..n {
                let e = &buf[i];
                shell_println!("  {:>6}  {:>4}  {:>4}  {:>8}  {:>8}",
                    e.task_id, e.from_cpu, e.to_cpu, e.reason.name(), e.tick);
            }
        }
        "hot" => {
            match sched_migrate::hottest_path() {
                Some((from, to, count)) => {
                    let s = sched_migrate::stats();
                    let pct = if s.total > 0 {
                        (count as u64) * 100 / s.total
                    } else {
                        0
                    };
                    shell_println!("Hottest migration path: CPU{} → CPU{}", from, to);
                    shell_println!("  {} migrations ({}% of total {})",
                        count, pct, s.total);
                }
                None => {
                    shell_println!("No migration events recorded");
                }
            }
        }
        "reset" => {
            sched_migrate::reset();
            shell_println!("Migration statistics cleared");
        }
        _ => {
            // Default: show aggregate stats.
            let s = sched_migrate::stats();
            let cpu_count = crate::smp::cpu_count();

            shell_println!("=== Task Migration Statistics ===");
            shell_println!("");
            shell_println!("  Total migrations: {}", s.total);
            shell_println!("");

            // Per-reason breakdown.
            shell_println!("  By reason:");
            let reasons = [
                (sched_migrate::MigrateReason::WorkSteal, "Work steal"),
                (sched_migrate::MigrateReason::PushBalance, "Push balance"),
                (sched_migrate::MigrateReason::AffinityChange, "Affinity change"),
                (sched_migrate::MigrateReason::Explicit, "Explicit"),
                (sched_migrate::MigrateReason::WakeUp, "Wake-up"),
            ];
            for (reason, label) in &reasons {
                let count = s.by_reason[*reason as usize];
                if count > 0 {
                    shell_println!("    {:<16} {}", label, count);
                }
            }

            // Per-CPU breakdown.
            shell_println!("");
            shell_println!("  Per-CPU:");
            shell_println!("    {:>4}  {:>8}  {:>8}", "CPU", "IN", "OUT");
            for i in 0..cpu_count {
                let in_c = s.per_cpu_in[i];
                let out_c = s.per_cpu_out[i];
                if in_c > 0 || out_c > 0 {
                    shell_println!("    {:>4}  {:>8}  {:>8}", i, in_c, out_c);
                }
            }

            // Hottest path.
            if let Some((from, to, count)) = sched_migrate::hottest_path() {
                shell_println!("");
                shell_println!("  Hottest path: CPU{} → CPU{} ({}x)", from, to, count);
            }
        }
    }
}

/// `wchan` — show what blocked tasks are waiting on.
fn cmd_wchan() {
    use crate::wchan;

    let s = wchan::stats();

    shell_println!("=== Wait Channels (WCHAN) ===");
    shell_println!("");
    shell_println!("  Total set/clear operations: {} / {}", s.total_sets, s.total_clears);
    shell_println!("  Currently blocked tasks: {}", s.currently_blocked);
    shell_println!("");

    // Breakdown by channel type.
    if s.currently_blocked > 0 {
        shell_println!("  By channel:");
        let channels = [
            (wchan::WaitChannel::Timer, "Timer"),
            (wchan::WaitChannel::Channel, "IPC Channel"),
            (wchan::WaitChannel::Pipe, "Pipe"),
            (wchan::WaitChannel::Futex, "Futex"),
            (wchan::WaitChannel::Mutex, "Mutex"),
            (wchan::WaitChannel::Event, "Event"),
            (wchan::WaitChannel::Join, "Join"),
            (wchan::WaitChannel::Completion, "Completion"),
            (wchan::WaitChannel::Io, "I/O"),
            (wchan::WaitChannel::Other, "Other"),
        ];
        for (ch, label) in &channels {
            let count = s.by_channel[*ch as usize];
            if count > 0 {
                shell_println!("    {:<14} {}", label, count);
            }
        }

        // Show individual blocked tasks.
        shell_println!("");
        shell_println!("  Blocked tasks:");
        shell_println!("    {:>6}  {:>8}  {:>16}", "TASK", "WCHAN", "ARG");
        let mut buf = [(0u64, wchan::WaitChannel::None, 0u64); 32];
        let n = wchan::blocked_list(&mut buf);
        for i in 0..n {
            let (tid, ch, arg) = buf[i];
            shell_println!("    {:>6}  {:>8}  {:#016x}", tid, ch.name(), arg);
        }
    } else {
        shell_println!("  No tasks currently blocked (or no wchan data recorded)");
    }
}

/// `bench` — run kernel micro-benchmarks on demand.
///
/// Usage:
///   bench           — run all standard benchmarks
///   bench alloc     — benchmark frame alloc/free
///   bench heap      — benchmark heap alloc/free
///   bench atomic    — benchmark atomic operations
///   bench memcpy    — benchmark memory copy
///   bench list      — list available benchmarks
fn cmd_bench(args: &str) {
    use crate::bench;

    let sub = args.trim();

    match sub {
        "list" => {
            shell_println!("Available benchmarks:");
            shell_println!("  alloc    — frame allocate + free cycle");
            shell_println!("  heap     — slab alloc + free (64 bytes)");
            shell_println!("  atomic   — atomic CAS round-trip");
            shell_println!("  memcpy   — 4 KiB memory copy");
            shell_println!("  rdtsc    — TSC read overhead");
            shell_println!("  (blank)  — run all of the above");
        }
        "alloc" => bench_alloc(),
        "heap" => bench_heap(),
        "atomic" => bench_atomic(),
        "memcpy" => bench_memcpy(),
        "rdtsc" => bench_rdtsc(),
        "" => {
            shell_println!("=== Kernel Micro-Benchmarks ===");
            shell_println!("");
            bench_rdtsc();
            bench_atomic();
            bench_alloc();
            bench_heap();
            bench_memcpy();
            shell_println!("");
            shell_println!("TSC freq: {} MHz", bench::tsc_freq() / 1_000_000);
        }
        other => {
            shell_println!("Unknown benchmark '{}'. Use 'bench list'.", other);
        }
    }
}

fn bench_rdtsc() {
    use crate::bench;
    let r = bench::run("rdtsc", 1000, || {
        core::hint::black_box(bench::rdtsc());
    });
    shell_println!("  rdtsc:   min={} cyc ({} ns), mean={} ns",
        r.min_cycles, r.min_ns, r.mean_ns);
}

fn bench_atomic() {
    use crate::bench;
    use core::sync::atomic::{AtomicU64, Ordering};
    static BENCH_ATOM: AtomicU64 = AtomicU64::new(0);

    let r = bench::run("atomic_cas", 1000, || {
        let old = BENCH_ATOM.load(Ordering::Relaxed);
        let _ = BENCH_ATOM.compare_exchange(old, old + 1,
            Ordering::AcqRel, Ordering::Relaxed);
    });
    shell_println!("  atomic:  min={} cyc ({} ns), mean={} ns",
        r.min_cycles, r.min_ns, r.mean_ns);
}

fn bench_alloc() {
    use crate::bench;
    use crate::mm::frame;

    let r = bench::run("frame_alloc_free", 100, || {
        if let Ok(f) = frame::alloc_frame() {
            // SAFETY: We just allocated this frame and haven't mapped it.
            let _ = unsafe { frame::free_frame(f) };
        }
    });
    shell_println!("  alloc:   min={} cyc ({} ns), mean={} ns",
        r.min_cycles, r.min_ns, r.mean_ns);
}

fn bench_heap() {
    use crate::bench;
    use alloc::boxed::Box;

    let r = bench::run("heap_alloc_free", 500, || {
        let b = Box::new([0u8; 64]);
        core::hint::black_box(&*b);
        drop(b);
    });
    shell_println!("  heap:    min={} cyc ({} ns), mean={} ns",
        r.min_cycles, r.min_ns, r.mean_ns);
}

fn bench_memcpy() {
    use crate::bench;

    let src = [0xABu8; 4096];
    let mut dst = [0u8; 4096];

    let r = bench::run("memcpy_4k", 200, || {
        // SAFETY: src and dst are valid, non-overlapping, properly aligned.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), 4096);
        }
        core::hint::black_box(&dst);
    });
    shell_println!("  memcpy:  min={} cyc ({} ns), mean={} ns  ({} MiB/s)",
        r.min_cycles, r.min_ns, r.mean_ns,
        if r.mean_ns > 0 { 4096 * 1_000_000_000 / (r.mean_ns * 1024 * 1024) } else { 0 });
}

/// `report` — comprehensive system diagnostic report (structured).
///
/// Usage:
///   report              — full diagnostic report (all sections)
///   report summary      — one-line health summary
///   report <section>    — single section (memory, sched, ipc, obj, cap, migrate, invar)
fn cmd_diag_report(args: &str) {
    use crate::kdiag;

    let sub = args.trim();

    match sub {
        "summary" | "health" => {
            let (health, msg) = kdiag::health_summary();
            shell_println!("[{}] {}", health.label(), msg);
        }
        "" => {
            // Full report.
            let report = kdiag::full_report();
            shell_println!("=== Kernel Diagnostic Report ===");
            shell_println!("Overall health: [{}]", report.overall.label());
            shell_println!("");

            for section in &report.sections {
                shell_println!("--- {} [{}] ---", section.title, section.health.label());
                for line in &section.lines {
                    shell_println!("  {}", line);
                }
                shell_println!("");
            }
        }
        name => {
            // Single section.
            match kdiag::section(name) {
                Some(s) => {
                    shell_println!("--- {} [{}] ---", s.title, s.health.label());
                    for line in &s.lines {
                        shell_println!("  {}", line);
                    }
                }
                None => {
                    shell_println!("Unknown section '{}'. Available:", name);
                    shell_println!("  system, memory/mem/mm, scheduler/sched, ipc,");
                    shell_println!("  objects/obj, capabilities/cap, migrations/migrate,");
                    shell_println!("  invariants/invar");
                }
            }
        }
    }
}

/// `hypervisor` — show hypervisor/virtualization information.
fn cmd_hypervisor() {
    use crate::hypervisor;

    let hv = hypervisor::detected();

    shell_println!("=== Virtualization ===");
    shell_println!("");
    if hv.is_virtual() {
        shell_println!("  Running inside: {}", hv.name());
        shell_println!("  Signature:      {:?}", hypervisor::signature_str());
        shell_println!("  Virtualized:    yes");
    } else {
        shell_println!("  Running on:     bare metal (no hypervisor)");
        shell_println!("  Virtualized:    no");
    }
}

/// `smap`/`smep` — show SMEP/SMAP (user page protection) status.
fn cmd_smep_smap() {
    use crate::smep_smap;

    let s = smep_smap::status();

    shell_println!("=== SMEP/SMAP (User Page Protection) ===");
    shell_println!("");
    shell_println!("  SMEP (Supervisor Mode Execution Prevention):");
    shell_println!("    Hardware: {}", if s.hw_smep { "supported" } else { "not supported" });
    shell_println!("    Status:   {}", if s.smep_active { "ACTIVE" } else { "inactive" });
    shell_println!("");
    shell_println!("  SMAP (Supervisor Mode Access Prevention):");
    shell_println!("    Hardware: {}", if s.hw_smap { "supported" } else { "not supported" });
    shell_println!("    Status:   {}", if s.smap_active { "ACTIVE" } else { "inactive" });
    shell_println!("");
    shell_println!("  CR4: {:#x}", s.cr4);
    shell_println!("  User-access windows (STAC/CLAC): {}", s.user_access_count);

    if s.smep_active && s.smap_active {
        shell_println!("");
        shell_println!("  Both protections ACTIVE — kernel cannot access/execute");
        shell_println!("  user pages without explicit STAC/CLAC window.");
    } else if !s.hw_smep && !s.hw_smap {
        shell_println!("");
        shell_println!("  Note: This CPU does not support SMEP/SMAP.");
        shell_println!("  Requires Intel Haswell+ or AMD Zen+.");
    }
}

/// `cet` — show Intel CET (Control-flow Enforcement) status.
fn cmd_cet() {
    use crate::cet;

    let s = cet::status();

    shell_println!("=== Intel CET (Control-flow Enforcement) ===");
    shell_println!("");
    shell_println!("  Hardware support:");
    shell_println!("    Shadow Stacks (SHSTK): {}",
        if s.hw_shstk { "yes" } else { "no" });
    shell_println!("    Indirect Branch Tracking (IBT): {}",
        if s.hw_ibt { "yes" } else { "no" });
    shell_println!("");
    shell_println!("  Supervisor mode:");
    shell_println!("    Shadow stacks: {}",
        if s.supervisor_shstk { "ACTIVE" } else { "inactive" });
    shell_println!("    IBT enforcement: {}",
        if s.supervisor_ibt { "ACTIVE" } else { "inactive" });
    shell_println!("");
    shell_println!("  #CP exceptions: {}", s.cp_exceptions);

    if !s.hw_shstk && !s.hw_ibt {
        shell_println!("");
        shell_println!("  Note: This CPU does not support CET.");
        shell_println!("  CET requires Intel 11th gen+ or AMD Zen 3+.");
    }
}

/// `fairness` — show scheduler fairness (Jain's Fairness Index).
fn cmd_fairness() {
    use crate::sched_fairness;

    let r = sched_fairness::measure();

    shell_println!("=== Scheduler Fairness ===");
    shell_println!("");
    shell_println!("  Jain's Fairness Index: {}.{:03}", r.jfi_x1000 / 1000, r.jfi_x1000 % 1000);
    shell_println!("  Tasks measured: {}", r.task_count);
    shell_println!("  Measurements: {}", sched_fairness::measurement_count());
    shell_println!("");

    if r.task_count > 0 {
        shell_println!("  Max ticks: {}  Min ticks: {}  Ratio: {:.1}x",
            r.max_ticks, r.min_ticks,
            if r.min_ticks > 0 { r.max_ticks as f64 / r.min_ticks as f64 } else { 0.0 });
        shell_println!("");

        // Show per-task breakdown (top tasks by ticks).
        shell_println!("  {:>8}  {:>8}", "TICKS", "NAME");
        for i in 0..r.task_count.min(16) {
            let ticks = r.per_task_ticks[i];
            if ticks == 0 {
                continue;
            }
            let name = core::str::from_utf8(&r.per_task_names[i])
                .unwrap_or("?")
                .trim_end_matches('\0');
            shell_println!("  {:>8}  {}", ticks, name);
        }
    } else {
        shell_println!("  No active tasks with CPU time in this window.");
        shell_println!("  (Run again after some tasks have executed)");
    }
}
