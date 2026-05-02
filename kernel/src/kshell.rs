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
    "alias", "append", "basename", "blkdev", "blkinfo", "blkread", "cat",
    "cd", "chmod", "chown", "clear", "cls", "cmp", "command", "copy", "cp",
    "cut", "date", "dd", "del", "df", "dhcp", "diff", "dir", "dirname", "dmesg", "dns", "du",
    "echo", "env", "eval", "exec", "export", "fallocate", "false", "file", "find", "fold", "free",
    "glob", "grep", "hash", "head", "help", "hexdump", "hostname", "http",
    "id", "ifconfig", "irq", "let", "ln", "link", "ls", "lsblk", "lsof", "lsp",
    "mapfile", "mem", "meminfo", "mkdir", "mkelf", "mklink", "mktemp", "mount", "mv",
    "move", "net", "nl", "nslookup", "paste", "pci", "ping", "printenv",
    "printf", "ps", "pwd", "readarray", "readlink", "readonly", "realpath",
    "reboot", "ren", "rev", "rm",
    "rmdir", "run", "select", "seq", "set", "sha256", "sleep", "sort", "source",
    "tac", "tr",
    "do", "done", "elif", "else", "expr", "fi", "if",
    "stat", "symlink", "sync", "sysctl", "tail", "tasks", "tee", "test",
    "then", "time", "touch", "trash", "tree", "true", "truncate", "type", "umount",
    "uname", "unalias", "uniq", "unmount", "unset", "uptime", "ver", "version",
    "wc", "wget", "which", "while", "whoami", "write", "xattr", "xxd",
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
        "ps" | "tasks" => cmd_ps(),
        "clear" | "cls" => cmd_clear(),
        "uptime" => cmd_uptime(),
        "dmesg" => cmd_dmesg(args),
        "echo" => cmd_echo(args),
        "printf" => cmd_printf(args),
        "time" | "date" => cmd_time(),
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
        "diff" => cmd_diff(args),
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
        "lsblk" | "blkdev" => cmd_lsblk(),
        "glob" => cmd_glob(args),
        "readlink" => cmd_readlink(args),
        "symlink" | "mklink" => cmd_symlink(args),
        "xattr" => cmd_xattr(args),
        "trash" => cmd_trash(args),
        "basename" => cmd_basename(args),
        "dirname" => cmd_dirname(args),
        "realpath" => cmd_realpath(args),
        "pwd" => cmd_pwd(),
        "id" | "whoami" => cmd_id(),
        "mktemp" => cmd_mktemp(args),
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
    crate::console_println!("  time      Show current date and time (RTC)");
    crate::console_println!("  irq       Show IRQ interrupt counts");
    crate::console_println!("  pci       List PCI devices");
    crate::console_println!("  disk      Show block device info");
    crate::console_println!("  blkread N Hex-dump sector N from disk");
    crate::console_println!("  cd [dir]  Change working directory");
    crate::console_println!("  ls [path] List files in directory");
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
    crate::console_println!("  touch F   Create file or update timestamps");
    crate::console_println!("  append F T Append text T to file F");
    crate::console_println!("  tree [D]  Show directory tree recursively");
    crate::console_println!("  du [D]    Show disk usage of directory");
    crate::console_println!("  find [D]P Search for files matching pattern");
    crate::console_println!("  df [path] Show filesystem space usage");
    crate::console_println!("  sync      Flush all filesystems to disk");
    crate::console_println!("  mount     List all mounted filesystems");
    crate::console_println!("  umount P  Unmount filesystem at path P");
    crate::console_println!("  wc FILE   Count lines, words, and bytes");
    crate::console_println!("  head N F  Show first N lines of file");
    crate::console_println!("  tail N F  Show last N lines of file");
    crate::console_println!("  hexdump F Hex dump of file contents");
    crate::console_println!("  lsof      List open file handles");
    crate::console_println!("  lsp [N] D Paginated ls: show N entries at a time");
    crate::console_println!("  grep P F  Search for pattern P in file F");
    crate::console_println!("  cmp F1 F2 Compare two files byte-by-byte");
    crate::console_println!("  diff F1 F2 Line-level diff (unified format)");
    crate::console_println!("  fallocate N F Pre-allocate N bytes for file F");
    crate::console_println!("  sort FILE Sort lines of a file alphabetically");
    crate::console_println!("  uniq FILE Remove adjacent duplicate lines");
    crate::console_println!("  tee F T   Write text T to file F and display it");
    crate::console_println!("  truncate N F Truncate file F to N bytes");
    crate::console_println!("  sha256 F  Compute SHA-256 hash of file contents");
    crate::console_println!("  sysctl .. List/get/set kernel parameters");
    crate::console_println!("  hostname  Show or set system hostname");
    crate::console_println!("  dd ..     Copy blocks between files (if=/of=/bs=/count=)");
    crate::console_println!("  free      Show memory usage summary");
    crate::console_println!("  lsblk     List block devices with sizes");
    crate::console_println!("  glob P    Expand glob pattern (e.g., /tmp/*.txt)");
    crate::console_println!("  readlink P Show symlink target");
    crate::console_println!("  symlink T P Create symlink at P pointing to T");
    crate::console_println!("  xattr F .. Extended attributes (list/get/set/rm)");
    crate::console_println!("  basename P Extract filename from path");
    crate::console_println!("  dirname P  Extract directory from path");
    crate::console_println!("  realpath P Resolve path (follow symlinks)");
    crate::console_println!("  pwd        Print working directory");
    crate::console_println!("  id         Show current task identity");
    crate::console_println!("  mktemp [D] Create temporary file (default /tmp)");
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
    crate::console_println!("  date       Show current date and time");
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
}

fn cmd_ps() {
    let task_list = crate::sched::task_list();
    if task_list.is_empty() {
        shell_println!("No tasks.");
        return;
    }

    shell_println!(
        "{:<6} {:<12} {:<10} {:<4} {:<8} {:<8} {:<4}",
        "TID", "NAME", "STATE", "PRI", "TICKS", "SCHED", "CPU"
    );
    shell_println!("------------------------------------------------------");
    for info in &task_list {
        let name = core::str::from_utf8(&info.name[..info.name_len])
            .unwrap_or("?");
        shell_println!(
            "{:<6} {:<12} {:<10} {:<4} {:<8} {:<8} {:<4}",
            info.id,
            name,
            info.state,
            info.priority,
            info.total_ticks,
            info.schedule_count,
            info.last_cpu,
        );
    }
    shell_println!("{} task(s) total", task_list.len());
}

fn cmd_clear() {
    crate::console::clear();
}

fn cmd_uptime() {
    let ticks = crate::apic::tick_count();
    // Timer runs at 100 Hz, so ticks / 100 = seconds.
    let seconds = ticks / 100;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    shell_println!(
        "Uptime: {} ticks ({:02}:{:02}:{:02})",
        ticks,
        hours,
        minutes % 60,
        seconds % 60
    );
}

/// `dmesg [-n COUNT]` — display kernel log messages.
///
/// Reads the structured log ring buffer and displays entries in a
/// human-readable format: `[timestamp] level/module: message`.
/// Use `-n COUNT` to limit output to the last COUNT entries.
fn cmd_dmesg(args: &str) {
    let mut limit: usize = usize::MAX;
    let words: Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        if words[i] == "-n" {
            i = i.saturating_add(1);
            if let Some(n) = words.get(i).and_then(|s| s.parse::<usize>().ok()) {
                limit = n;
            }
        }
        i = i.saturating_add(1);
    }

    // Read all log entries from the ring buffer.
    // The buffer is JSON-lines format; we'll read raw bytes and parse
    // the timestamp, level, module, and message from each line.
    let mut buf = alloc::vec![0u8; 64 * 1024];
    let (written, _last_seq) = crate::klog::read_logs(u64::MAX, &mut buf);
    let text = core::str::from_utf8(buf.get(..written).unwrap_or(&[]))
        .unwrap_or("");

    // Collect lines, then take the last `limit` entries.
    let lines: alloc::vec::Vec<&str> = text.lines().collect();
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

fn cmd_time() {
    let dt = crate::rtc::read_datetime();
    shell_println!("{}", dt);
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
    let mut path_arg = "";

    for token in args.split_whitespace() {
        if let Some(flags) = token.strip_prefix('-') {
            for ch in flags.chars() {
                match ch {
                    'l' => long_format = true,
                    'a' => show_all = true,
                    'h' => human_sizes = true,
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

    match crate::fs::Vfs::readdir(&path) {
        Ok(entries) => {
            if entries.is_empty() {
                shell_println!("(empty directory)");
                return;
            }

            // Filter hidden files unless -a is given.
            let filtered: Vec<&crate::fs::vfs::DirEntry> = entries.iter()
                .filter(|e| show_all || !e.name.starts_with('.'))
                .collect();

            if long_format {
                // Long format: type+perms links uid gid size date name
                for entry in &filtered {
                    // Build the entry's full path for metadata lookup.
                    let full_path = if path == "/" {
                        alloc::format!("/{}", entry.name)
                    } else {
                        alloc::format!("{}/{}", path, entry.name)
                    };

                    let type_ch = match entry.entry_type {
                        crate::fs::EntryType::Directory => 'd',
                        crate::fs::EntryType::File => '-',
                        crate::fs::EntryType::Symlink => 'l',
                        crate::fs::EntryType::VolumeLabel => 'v',
                    };

                    // Try to get rich metadata; fall back to basic info.
                    if let Ok(meta) = crate::fs::Vfs::metadata(&full_path) {
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

                        shell_println!(
                            "{}{} {:>3} {:>5} {:>5} {:>8} {} {}",
                            type_ch, perm_str, meta.nlinks,
                            meta.uid, meta.gid, size_str,
                            time_str, entry.name,
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
        }
        Err(e) => {
            crate::console_println!("ls: {}: {:?}", path, e);
            set_exit(1);
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
            crate::console_println!("  Size: {}  Type: {}", meta.size, type_str);
            crate::console_println!("  Links: {}", meta.nlinks);
            if meta.permissions != 0 {
                crate::console_println!("  Perms: {:04o}  Uid: {}  Gid: {}",
                    meta.permissions, meta.uid, meta.gid);
            }
            if meta.attributes != crate::fs::FileAttr::NONE {
                crate::console_println!("  Attrs: {:?}", meta.attributes);
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
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Mounted on"
                );
                for (mount_path, info) in &mounts {
                    let total = info.total_bytes();
                    let free = info.free_bytes();
                    let used = info.used_bytes();
                    let pct = info.usage_percent();
                    crate::console_println!(
                        "{:<12} {:>10} {:>10} {:>10} {:>4}%  {}",
                        info.fs_type,
                        format_bytes(total),
                        format_bytes(used),
                        format_bytes(free),
                        pct,
                        mount_path
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
                    "{:<12} {:>10} {:>10} {:>10} {:>5}  {}",
                    "Filesystem", "Size", "Used", "Avail", "Use%", "Path"
                );
                let total = info.total_bytes();
                let free = info.free_bytes();
                let used = info.used_bytes();
                let pct = info.usage_percent();
                crate::console_println!(
                    "{:<12} {:>10} {:>10} {:>10} {:>4}%  {}",
                    info.fs_type,
                    format_bytes(total),
                    format_bytes(used),
                    format_bytes(free),
                    pct,
                    path
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

/// Create a file or update timestamps.
fn cmd_touch(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: touch <path>");
        return;
    }

    let path = resolve_path(args);

    // Check if file exists.
    match crate::fs::Vfs::stat(&path) {
        Ok(_) => {
            // File exists — update timestamps to "now".
            let now = crate::hpet::elapsed_ns();
            match crate::fs::Vfs::set_times(&path, now, now) {
                Ok(()) => {
                    crate::console_println!("{}: timestamps updated", path);
                }
                Err(e) => {
                    crate::console_println!("touch: {}: {:?}", path, e);
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
                }
            }
        }
    }
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
    let path = if args.is_empty() {
        get_cwd()
    } else {
        resolve_path(args)
    };

    let total = du_recurse(&path);
    crate::console_println!("{}\t{}", format_bytes(total), path);
}

/// Recursively calculate total size of a directory tree.
#[allow(clippy::arithmetic_side_effects)]
fn du_recurse(path: &str) -> u64 {
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
            let subdir_total = du_recurse(&child_path);
            crate::console_println!("{}\t{}", format_bytes(subdir_total), child_path);
            total = total.saturating_add(subdir_total);
        }
    }

    total
}

/// Search for files matching a pattern (basic find).
fn cmd_find(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    let (search_path, pattern) = if parts.len() >= 2 {
        (parts[0], parts[1])
    } else if !args.is_empty() {
        ("/", args)
    } else {
        crate::console_println!("Usage: find [path] <pattern>");
        crate::console_println!("  Searches for files/dirs matching the glob pattern.");
        crate::console_println!("  Patterns: * (any), ? (one char), [abc] (set), [a-z] (range)");
        crate::console_println!("  Example: find /tmp *.txt");
        crate::console_println!("  Example: find / *.rs");
        return;
    };

    let root = resolve_path(search_path);

    // Detect whether the pattern contains glob metacharacters.
    let is_glob = pattern.contains('*') || pattern.contains('?') || pattern.contains('[');

    let mut count: u64 = 0;
    find_recurse(&root, pattern, is_glob, &mut count, 0);
    shell_println!("\n{} matches found", count);
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
                let hint = extension_hint(&path);
                shell_println!("{}: regular file, {} bytes ({})", path, entry.size, hint);
            }
        }
        crate::fs::EntryType::VolumeLabel => {
            shell_println!("{}: volume label", path);
        }
    }
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
fn find_recurse(path: &str, pattern: &str, is_glob: bool, count: &mut u64, depth: u32) {
    if depth > 16 {
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

        // Match the entry name against the pattern.
        let matched = if is_glob {
            // Glob pattern matching (case-insensitive).
            crate::fs::vfs::glob_match(&entry.name, pattern, true)
        } else {
            // Legacy: case-insensitive substring match.
            let name_lower = entry.name.to_ascii_lowercase();
            let pattern_lower = pattern.to_ascii_lowercase();
            name_lower.contains(&pattern_lower)
        };

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
            find_recurse(&child_path, pattern, is_glob, count, depth + 1);
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
fn cmd_hexdump(args: &str) {
    if args.is_empty() {
        crate::console_println!("Usage: hexdump <file>");
        return;
    }

    let path = resolve_path(args);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("hexdump: {}: {:?}", path, e);
            return;
        }
    };

    // Limit output to first 512 bytes to avoid flooding the console.
    let limit = data.len().min(512);
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

    if data.len() < limit {
        shell_println!("{:08x}", data.len());
    } else {
        shell_println!("... ({} bytes total, showing first {})", data.len(), limit);
    }
}

/// Search for a pattern in a file (simple substring grep).
fn cmd_grep(args: &str) {
    let parts: alloc::vec::Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        crate::console_println!("Usage: grep <pattern> <file>");
        return;
    }

    let pattern = parts[0];
    let file_arg = parts[1];

    let path = resolve_path(file_arg);

    let data = match crate::fs::Vfs::read_file(&path) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("grep: {}: {:?}", path, e);
            return;
        }
    };

    // Try to interpret as UTF-8 text.
    let text = match core::str::from_utf8(&data) {
        Ok(s) => s,
        Err(_) => {
            crate::console_println!("grep: {}: binary file (not UTF-8)", path);
            return;
        }
    };

    // Case-insensitive substring search across lines.
    let pattern_lower = {
        let mut p = alloc::string::String::with_capacity(pattern.len());
        for c in pattern.chars() {
            for lc in c.to_lowercase() {
                p.push(lc);
            }
        }
        p
    };

    let mut match_count = 0usize;
    for (line_num, line) in text.lines().enumerate() {
        // Build lowercase version of line for comparison.
        let line_lower = {
            let mut l = alloc::string::String::with_capacity(line.len());
            for c in line.chars() {
                for lc in c.to_lowercase() {
                    l.push(lc);
                }
            }
            l
        };

        if line_lower.contains(pattern_lower.as_str()) {
            shell_println!(
                "{}:{}: {}",
                line_num.saturating_add(1),
                path,
                line,
            );
            match_count = match_count.saturating_add(1);

            // Limit output to prevent flooding.
            if match_count >= 50 {
                shell_println!("... (showing first 50 matches)");
                break;
            }
        }
    }

    if match_count == 0 {
        crate::console_println!("grep: no matches for '{}' in {}", pattern, path);
    } else {
        shell_println!("{} matches", match_count);
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
        // List all mounts.
        let mounts = crate::fs::Vfs::mounts();
        if mounts.is_empty() {
            crate::console_println!("No filesystems mounted.");
        } else {
            crate::console_println!("{:<12} {}", "Type", "Mount point");
            for (path, fs_type) in &mounts {
                crate::console_println!("{:<12} {}", fs_type, path);
            }
        }
        return;
    }

    // Parse: mount [-t type] <device|none> <mount-path>
    let words: Vec<&str> = args.split_whitespace().collect();

    let (fs_type, device, mount_path) = if words.len() >= 4
        && words.first() == Some(&"-t")
    {
        // mount -t <type> <device> <path>
        (Some(words[1]), words[2], words[3])
    } else if words.len() >= 2 {
        // mount <device> <path> — auto-detect type
        (None, words[0], words[1])
    } else {
        crate::console_println!("Usage: mount [-t type] <device|none> <mount-path>");
        crate::console_println!("Types: ext4, memfs, procfs, devfs, sysfs, iso9660");
        crate::console_println!("Use 'none' as device for virtual filesystems.");
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
        Ok(()) => crate::console_println!("Mounted {} at {}", device, mount_path_resolved),
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
    match crate::fs::Vfs::sync() {
        Ok(()) => {
            crate::console_println!("All filesystems synced.");
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
    crate::console_println!("Rebooting...");

    // Triple-fault reboot: load a null IDT and trigger an interrupt.
    // The CPU will triple-fault, and the chipset will reset.
    //
    // SAFETY: We're intentionally crashing the system to reboot.
    unsafe {
        // Load a zero-length IDT.
        let null_idt: [u8; 10] = [0; 10];
        core::arch::asm!(
            "lidt [{}]",
            in(reg) null_idt.as_ptr(),
            options(noreturn)
        );
    }
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
    // If args contains a space, it looks like "pattern file" — delegate.
    if args.contains(' ') {
        cmd_grep(args);
        return;
    }

    let pattern = args.trim();
    if pattern.is_empty() {
        crate::console_println!("grep: no pattern specified");
        return;
    }

    // Build a lowercase version of the pattern for case-insensitive search.
    let pattern_lower = {
        let mut p = String::with_capacity(pattern.len());
        for c in pattern.chars() {
            for lc in c.to_lowercase() {
                p.push(lc);
            }
        }
        p
    };

    let mut match_count = 0usize;
    for (line_num, line) in input.lines().enumerate() {
        let line_lower = {
            let mut l = String::with_capacity(line.len());
            for c in line.chars() {
                for lc in c.to_lowercase() {
                    l.push(lc);
                }
            }
            l
        };

        if line_lower.contains(pattern_lower.as_str()) {
            shell_println!("{}: {}", line_num.saturating_add(1), line);
            match_count = match_count.saturating_add(1);

            if match_count >= 50 {
                shell_println!("... (showing first 50 matches)");
                break;
            }
        }
    }

    if match_count == 0 {
        crate::console_println!("grep: no matches for '{}'", pattern);
    } else {
        shell_println!("{} matches", match_count);
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
        | "sysctl" | "hostname" | "dd" | "free" | "lsblk" | "blkdev" | "glob"
        | "readlink" | "symlink" | "mklink" | "xattr" | "trash" | "basename" | "dirname"
        | "realpath" | "pwd" | "id" | "whoami" | "mktemp" | "run" | "exec"
        | "mkelf" | "net" | "ifconfig" | "dhcp" | "ping" | "dns" | "nslookup"
        | "wget" | "http" | "version" | "ver" | "uname" | "source" | "." | "seq" | "nl"
        | "rev" | "sleep" | "true" | "false" | "test" | "[" | "expr" | "printenv"
        | "env" | "eval" | "declare" | "read" | "readarray" | "mapfile"
        | "readonly" | "let" | "trap" | "command" | "which" | "typeof"
        | "export" | "set" | "unset" | "alias" | "unalias" | "return"
        | "break" | "continue" | "shift" | "local" | "printf"
        | "cut" | "tr" | "yes" | "tac" | "fold" | "paste" | "xargs"
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

    // Parse key=value pairs.
    for token in args.split_whitespace() {
        if let Some(val) = token.strip_prefix("if=") {
            input = Some(val);
        } else if let Some(val) = token.strip_prefix("of=") {
            output = Some(val);
        } else if let Some(val) = token.strip_prefix("bs=") {
            // Parse block size with optional K/M suffix.
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
        } else {
            crate::console_println!("dd: unknown option '{}'", token);
            crate::console_println!("Usage: dd if=<input> of=<output> [bs=N] [count=N]");
            return;
        }
    }

    let input = match input {
        Some(p) => p,
        None => {
            crate::console_println!("Usage: dd if=<input> of=<output> [bs=N] [count=N]");
            return;
        }
    };
    let output = match output {
        Some(p) => p,
        None => {
            crate::console_println!("Usage: dd if=<input> of=<output> [bs=N] [count=N]");
            return;
        }
    };

    // Clamp block size to something reasonable.
    if bs == 0 || bs > 1024 * 1024 {
        crate::console_println!("dd: block size must be 1..1M");
        return;
    }

    // Read input file.
    let data = match crate::fs::Vfs::read_file(input) {
        Ok(d) => d,
        Err(e) => {
            crate::console_println!("dd: cannot read '{}': {:?}", input, e);
            return;
        }
    };

    // Apply count limit.
    let max_bytes = match count {
        Some(n) => (n as usize).saturating_mul(bs),
        None => data.len(),
    };
    let to_write = if max_bytes < data.len() {
        data.get(..max_bytes).unwrap_or(&data)
    } else {
        &data
    };

    // Write output file.
    match crate::fs::Vfs::write_file(output, to_write) {
        Ok(()) => {
            let blocks = to_write.len() / bs;
            let remainder = to_write.len() % bs;
            let total_blocks = if remainder > 0 { blocks + 1 } else { blocks };
            crate::console_println!(
                "{}+{} records in",
                blocks,
                if remainder > 0 { 1 } else { 0 }
            );
            crate::console_println!(
                "{}+{} records out",
                blocks,
                if remainder > 0 { 1 } else { 0 }
            );
            crate::console_println!(
                "{} bytes ({} blocks of {} bytes) copied",
                to_write.len(),
                total_blocks,
                bs
            );
        }
        Err(e) => {
            crate::console_println!("dd: cannot write '{}': {:?}", output, e);
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
}

/// `lsblk` — list block devices with capacity.
#[allow(clippy::arithmetic_side_effects)]
fn cmd_lsblk() {
    let devices = crate::blkdev::list_devices();

    if devices.is_empty() {
        crate::console_println!("No block devices found.");
        return;
    }

    crate::console_println!(
        "{:<8} {:>12} {:>8} {:>6}  {}",
        "NAME", "SECTORS", "SIZE", "RO", "TYPE"
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

        crate::console_println!(
            "{:<8} {:>12} {:>8} {:>6}  disk",
            dev.name, dev.sector_count, size_str, ro
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
