//! Environment variable management.
//!
//! Provides system-wide and per-user environment variable storage,
//! similar to Windows Environment Variables dialog, or `/etc/environment`
//! + `~/.profile` on Linux.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → System → Environment Variables
//!   → envvars::set_system() / set_user()
//!
//! Process spawn
//!   → envvars::resolve_env(uid) → merged env for child process
//!
//! Integration:
//!   → rundialog (PATH resolution)
//!   → autostart (service environment)
//!   → installer (first-boot PATH setup)
//! ```
//!
//! ## Variable Scope
//!
//! - **System**: visible to all users and services (e.g., PATH, TERM)
//! - **User**: per-user overrides (e.g., EDITOR, LANG)
//! - User variables override system variables of the same name
//! - PATH is special: user PATH is *appended* to system PATH

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_SYSTEM_VARS: usize = 256;
const MAX_USER_VARS: usize = 128;
const MAX_USERS: usize = 64;
const MAX_NAME_LEN: usize = 256;
const MAX_VALUE_LEN: usize = 8192;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Scope of an environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarScope {
    /// System-wide, visible to all users.
    System,
    /// Per-user override.
    User,
}

impl VarScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::User => "User",
        }
    }
}

/// An environment variable entry.
#[derive(Debug, Clone)]
pub struct EnvVar {
    /// Variable name (case-sensitive).
    pub name: String,
    /// Variable value.
    pub value: String,
    /// Whether this is a system or user variable.
    pub scope: VarScope,
    /// User ID (0 for system variables).
    pub uid: u32,
    /// Whether the variable is read-only (system-protected).
    pub read_only: bool,
    /// Description/purpose.
    pub description: String,
}

/// A resolved environment for process spawning.
#[derive(Debug, Clone)]
pub struct ResolvedEnv {
    /// Merged variables (system + user overrides).
    pub vars: Vec<(String, String)>,
}

impl ResolvedEnv {
    /// Get a variable value by name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct UserVars {
    uid: u32,
    vars: Vec<EnvVar>,
}

struct EnvState {
    system_vars: Vec<EnvVar>,
    user_vars: Vec<UserVars>,
    ops: u64,
}

static STATE: Mutex<Option<EnvState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut EnvState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

fn validate_name(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }
    // Variable names: letters, digits, underscores. Must start with letter/underscore.
    let first = name.as_bytes().first().copied().unwrap_or(0);
    if !first.is_ascii_alphabetic() && first != b'_' {
        return Err(KernelError::InvalidArgument);
    }
    for &b in name.as_bytes() {
        if !b.is_ascii_alphanumeric() && b != b'_' {
            return Err(KernelError::InvalidArgument);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the environment variable subsystem with standard defaults.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut system_vars = Vec::new();

    // Standard system environment variables.
    let defaults: &[(&str, &str, bool, &str)] = &[
        ("PATH", "/bin:/usr/bin:/usr/local/bin", false,
            "Executable search path"),
        ("HOME", "/home", true,
            "Default home directory base"),
        ("TERM", "xterm-256color", false,
            "Terminal type"),
        ("SHELL", "/bin/sh", false,
            "Default shell"),
        ("LANG", "en_US.UTF-8", false,
            "Default locale"),
        ("LC_ALL", "", false,
            "Override all locale categories"),
        ("EDITOR", "nano", false,
            "Default text editor"),
        ("VISUAL", "nano", false,
            "Default visual editor"),
        ("PAGER", "less", false,
            "Default pager"),
        ("TMPDIR", "/tmp", true,
            "Temporary file directory"),
        ("XDG_CONFIG_HOME", "", false,
            "User config directory (default: ~/.config)"),
        ("XDG_DATA_HOME", "", false,
            "User data directory (default: ~/.local/share)"),
        ("XDG_CACHE_HOME", "", false,
            "User cache directory (default: ~/.cache)"),
        ("XDG_RUNTIME_DIR", "/run/user", true,
            "User runtime directory"),
        ("DISPLAY", ":0", false,
            "Display server connection"),
        ("HOSTNAME", "", false,
            "System hostname"),
    ];

    for (name, value, read_only, desc) in defaults {
        system_vars.push(EnvVar {
            name: String::from(*name),
            value: String::from(*value),
            scope: VarScope::System,
            uid: 0,
            read_only: *read_only,
            description: String::from(*desc),
        });
    }

    *guard = Some(EnvState {
        system_vars,
        user_vars: Vec::new(),
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// System variables
// ---------------------------------------------------------------------------

/// Set a system-wide environment variable.
pub fn set_system(name: &str, value: &str) -> KernelResult<()> {
    validate_name(name)?;
    if value.len() > MAX_VALUE_LEN {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if let Some(var) = state.system_vars.iter_mut().find(|v| v.name == name) {
            if var.read_only {
                return Err(KernelError::PermissionDenied);
            }
            var.value = String::from(value);
        } else {
            if state.system_vars.len() >= MAX_SYSTEM_VARS {
                return Err(KernelError::ResourceExhausted);
            }
            state.system_vars.push(EnvVar {
                name: String::from(name),
                value: String::from(value),
                scope: VarScope::System,
                uid: 0,
                read_only: false,
                description: String::new(),
            });
        }
        Ok(())
    })
}

/// Get a system-wide environment variable.
pub fn get_system(name: &str) -> KernelResult<String> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.system_vars.iter()
        .find(|v| v.name == name)
        .map(|v| v.value.clone())
        .ok_or(KernelError::NotFound)
}

/// Remove a system variable.
pub fn remove_system(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.system_vars.iter().position(|v| v.name == name) {
            if state.system_vars[pos].read_only {
                return Err(KernelError::PermissionDenied);
            }
            state.system_vars.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all system variables.
pub fn list_system() -> Vec<EnvVar> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.system_vars.clone())
}

/// Set a system variable's description.
pub fn set_description(name: &str, scope: VarScope, uid: u32, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        match scope {
            VarScope::System => {
                let var = state.system_vars.iter_mut()
                    .find(|v| v.name == name)
                    .ok_or(KernelError::NotFound)?;
                var.description = String::from(desc);
                Ok(())
            }
            VarScope::User => {
                let uv = state.user_vars.iter_mut()
                    .find(|u| u.uid == uid)
                    .ok_or(KernelError::NotFound)?;
                let var = uv.vars.iter_mut()
                    .find(|v| v.name == name)
                    .ok_or(KernelError::NotFound)?;
                var.description = String::from(desc);
                Ok(())
            }
        }
    })
}

// ---------------------------------------------------------------------------
// User variables
// ---------------------------------------------------------------------------

/// Set a per-user environment variable.
pub fn set_user(uid: u32, name: &str, value: &str) -> KernelResult<()> {
    validate_name(name)?;
    if value.len() > MAX_VALUE_LEN {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        // Find or create user entry.
        let user_entry = if let Some(pos) = state.user_vars.iter().position(|u| u.uid == uid) {
            &mut state.user_vars[pos]
        } else {
            if state.user_vars.len() >= MAX_USERS {
                return Err(KernelError::ResourceExhausted);
            }
            state.user_vars.push(UserVars {
                uid,
                vars: Vec::new(),
            });
            state.user_vars.last_mut().ok_or(KernelError::InternalError)?
        };

        if let Some(var) = user_entry.vars.iter_mut().find(|v| v.name == name) {
            var.value = String::from(value);
        } else {
            if user_entry.vars.len() >= MAX_USER_VARS {
                return Err(KernelError::ResourceExhausted);
            }
            user_entry.vars.push(EnvVar {
                name: String::from(name),
                value: String::from(value),
                scope: VarScope::User,
                uid,
                read_only: false,
                description: String::new(),
            });
        }
        Ok(())
    })
}

/// Get a per-user environment variable.
pub fn get_user(uid: u32, name: &str) -> KernelResult<String> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.user_vars.iter()
        .find(|u| u.uid == uid)
        .and_then(|u| u.vars.iter().find(|v| v.name == name))
        .map(|v| v.value.clone())
        .ok_or(KernelError::NotFound)
}

/// Remove a per-user variable.
pub fn remove_user(uid: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        let user_entry = state.user_vars.iter_mut()
            .find(|u| u.uid == uid)
            .ok_or(KernelError::NotFound)?;
        if let Some(pos) = user_entry.vars.iter().position(|v| v.name == name) {
            user_entry.vars.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all user variables for a given user.
pub fn list_user(uid: u32) -> Vec<EnvVar> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.user_vars.iter()
            .find(|u| u.uid == uid)
            .map_or_else(Vec::new, |u| u.vars.clone())
    })
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Resolve the full environment for a user.
///
/// Merges system variables with user overrides.
/// Special handling for PATH: user PATH is appended to system PATH.
pub fn resolve_env(uid: u32) -> ResolvedEnv {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return ResolvedEnv { vars: Vec::new() },
    };

    let mut vars: Vec<(String, String)> = Vec::new();

    // Start with system variables.
    for sv in &state.system_vars {
        if !sv.value.is_empty() {
            vars.push((sv.name.clone(), sv.value.clone()));
        }
    }

    // Apply user overrides.
    if let Some(uv) = state.user_vars.iter().find(|u| u.uid == uid) {
        for uvar in &uv.vars {
            if uvar.name == "PATH" {
                // PATH is special: append user PATH to system PATH.
                if let Some(entry) = vars.iter_mut().find(|(n, _)| n == "PATH") {
                    if !uvar.value.is_empty() {
                        entry.1 = format!("{}:{}", entry.1, uvar.value);
                    }
                } else {
                    vars.push((uvar.name.clone(), uvar.value.clone()));
                }
            } else if let Some(entry) = vars.iter_mut().find(|(n, _)| n == &uvar.name) {
                // Override existing.
                entry.1 = uvar.value.clone();
            } else {
                // New variable.
                vars.push((uvar.name.clone(), uvar.value.clone()));
            }
        }
    }

    // Set HOME for the user.
    if let Some(entry) = vars.iter_mut().find(|(n, _)| n == "HOME") {
        entry.1 = format!("/home/{}", uid);
    }

    // Set USER.
    if let Some(entry) = vars.iter_mut().find(|(n, _)| n == "USER") {
        entry.1 = format!("{}", uid);
    } else {
        vars.push((String::from("USER"), format!("{}", uid)));
    }

    ResolvedEnv { vars }
}

/// Expand variable references in a string.
///
/// Supports `$VAR` and `${VAR}` syntax. Uses the resolved environment
/// for the given user.
pub fn expand(uid: u32, input: &str) -> String {
    let env = resolve_env(uid);
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' && i + 1 < len {
            if bytes[i + 1] == b'{' {
                // ${VAR} syntax.
                if let Some(end) = bytes[i + 2..].iter().position(|&b| b == b'}') {
                    let name = &input[i + 2..i + 2 + end];
                    if let Some(val) = env.get(name) {
                        result.push_str(val);
                    }
                    i = i + 3 + end;
                } else {
                    result.push('$');
                    i += 1;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_' {
                // $VAR syntax.
                let start = i + 1;
                let mut end = start;
                while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                    end += 1;
                }
                let name = &input[start..end];
                if let Some(val) = env.get(name) {
                    result.push_str(val);
                }
                i = end;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            // Safe: we're iterating byte-by-byte on ASCII-dominated content.
            // Non-ASCII bytes are copied as-is.
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Search for variables matching a pattern (case-insensitive substring).
pub fn search(query: &str) -> Vec<EnvVar> {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let query_lower = query.to_ascii_lowercase();
    let mut results = Vec::new();

    for v in &state.system_vars {
        if v.name.to_ascii_lowercase().contains(&query_lower)
            || v.value.to_ascii_lowercase().contains(&query_lower)
        {
            results.push(v.clone());
        }
    }

    for uv in &state.user_vars {
        for v in &uv.vars {
            if v.name.to_ascii_lowercase().contains(&query_lower)
                || v.value.to_ascii_lowercase().contains(&query_lower)
            {
                results.push(v.clone());
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// PATH helpers
// ---------------------------------------------------------------------------

/// Append a directory to the system PATH.
pub fn path_append(dir: &str) -> KernelResult<()> {
    with_state(|state| {
        let path_var = state.system_vars.iter_mut()
            .find(|v| v.name == "PATH")
            .ok_or(KernelError::NotFound)?;
        if path_var.value.is_empty() {
            path_var.value = String::from(dir);
        } else {
            path_var.value = format!("{}:{}", path_var.value, dir);
        }
        Ok(())
    })
}

/// Prepend a directory to the system PATH.
pub fn path_prepend(dir: &str) -> KernelResult<()> {
    with_state(|state| {
        let path_var = state.system_vars.iter_mut()
            .find(|v| v.name == "PATH")
            .ok_or(KernelError::NotFound)?;
        if path_var.value.is_empty() {
            path_var.value = String::from(dir);
        } else {
            path_var.value = format!("{}:{}", dir, path_var.value);
        }
        Ok(())
    })
}

/// Remove a directory from the system PATH.
pub fn path_remove(dir: &str) -> KernelResult<()> {
    with_state(|state| {
        let path_var = state.system_vars.iter_mut()
            .find(|v| v.name == "PATH")
            .ok_or(KernelError::NotFound)?;
        let parts: Vec<&str> = path_var.value.split(':')
            .filter(|p| *p != dir)
            .collect();
        path_var.value = parts.join(":");
        Ok(())
    })
}

/// List all directories in the system PATH.
pub fn path_list() -> Vec<String> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.system_vars.iter()
            .find(|v| v.name == "PATH")
            .map_or_else(Vec::new, |v| {
                v.value.split(':')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            })
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (system_var_count, user_count, total_user_vars, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let total_uv: usize = s.user_vars.iter().map(|u| u.vars.len()).sum();
            (s.system_vars.len(), s.user_vars.len(), total_uv, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the environment variables module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[envvars] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state has system variables.
    {
        let (sys_count, _, _, _) = stats();
        assert!(sys_count >= 10);
        let path = get_system("PATH").unwrap();
        assert!(path.contains("/bin"));
    }
    serial_println!("[envvars]  1/11 initial state OK");

    // Test 2: get/set system variable.
    {
        set_system("MY_VAR", "hello").unwrap();
        let val = get_system("MY_VAR").unwrap();
        assert_eq!(val, "hello");
        set_system("MY_VAR", "world").unwrap();
        let val = get_system("MY_VAR").unwrap();
        assert_eq!(val, "world");
    }
    serial_println!("[envvars]  2/11 system get/set OK");

    // Test 3: remove system variable.
    {
        set_system("TEMP_VAR", "temp").unwrap();
        remove_system("TEMP_VAR").unwrap();
        assert!(get_system("TEMP_VAR").is_err());
    }
    serial_println!("[envvars]  3/11 remove system OK");

    // Test 4: read-only protection.
    {
        assert!(set_system("HOME", "hacked").is_err());
        assert!(remove_system("HOME").is_err());
    }
    serial_println!("[envvars]  4/11 read-only protection OK");

    // Test 5: user variables.
    {
        set_user(1000, "EDITOR", "vim").unwrap();
        let val = get_user(1000, "EDITOR").unwrap();
        assert_eq!(val, "vim");
        assert!(get_user(1001, "EDITOR").is_err());
    }
    serial_println!("[envvars]  5/11 user variables OK");

    // Test 6: resolve_env merges system + user.
    {
        set_user(1000, "CUSTOM", "myval").unwrap();
        let env = resolve_env(1000);
        // System PATH should be present.
        assert!(env.get("PATH").is_some());
        // User EDITOR should override system.
        assert_eq!(env.get("EDITOR"), Some("vim"));
        // Custom user var.
        assert_eq!(env.get("CUSTOM"), Some("myval"));
    }
    serial_println!("[envvars]  6/11 resolve_env OK");

    // Test 7: PATH append for user.
    {
        set_user(1000, "PATH", "/home/1000/bin").unwrap();
        let env = resolve_env(1000);
        let path = env.get("PATH").unwrap();
        assert!(path.contains("/home/1000/bin"));
        assert!(path.contains("/bin")); // System PATH still present.
    }
    serial_println!("[envvars]  7/11 user PATH append OK");

    // Test 8: expand variables.
    {
        let result = expand(1000, "editor=$EDITOR path=${PATH}");
        assert!(result.contains("editor=vim"));
        assert!(result.contains("path=/bin"));
    }
    serial_println!("[envvars]  8/11 expand OK");

    // Test 9: PATH helpers.
    {
        path_append("/opt/custom/bin").unwrap();
        let dirs = path_list();
        assert!(dirs.iter().any(|d| d == "/opt/custom/bin"));

        path_prepend("/sbin").unwrap();
        let dirs = path_list();
        assert_eq!(dirs.first().map(|s| s.as_str()), Some("/sbin"));

        path_remove("/opt/custom/bin").unwrap();
        let dirs = path_list();
        assert!(!dirs.iter().any(|d| d == "/opt/custom/bin"));
    }
    serial_println!("[envvars]  9/11 PATH helpers OK");

    // Test 10: search.
    {
        let results = search("PATH");
        assert!(!results.is_empty());
        assert!(results.iter().any(|v| v.name == "PATH"));
    }
    serial_println!("[envvars] 10/11 search OK");

    // Test 11: name validation.
    {
        assert!(set_system("", "val").is_err());
        assert!(set_system("1BAD", "val").is_err());
        assert!(set_system("HAS SPACE", "val").is_err());
        assert!(set_system("_OK_NAME", "val").is_ok());
    }
    serial_println!("[envvars] 11/11 validation OK");

    serial_println!("[envvars] All self-tests passed.");
}
