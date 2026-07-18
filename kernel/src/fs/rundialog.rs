//! Run dialog backend (Ctrl+R / Win+R equivalent).
//!
//! Provides the infrastructure for the "Run" dialog that lets users
//! type a program name or path and launch it.  Maintains a history
//! of recent commands and supports path completion.
//!
//! ## Design Reference
//!
//! design.txt line 720: "ctrl+r to run a program, like on Windows,
//! supports dropdown with completion like on windows, but also shows
//! most recent commands in dropdown."
//!
//! roadmap.md line 810: "Ctrl+R run dialog (completion, recent commands)"
//!
//! ## Architecture
//!
//! ```text
//! User presses Ctrl+R
//!   → GUI shows run dialog
//!   → rundialog::completions("fi") → ["file-manager", "find", "firefox"]
//!   → rundialog::recent() → last N commands
//!   → User types "file-manager"
//!   → rundialog::run("file-manager")
//!     → resolve: PATH lookup, alias lookup, recent history
//!     → launch process
//!     → record in history
//! ```
//!
//! ## Completion Sources
//!
//! 1. Recent commands (most-recently-used first)
//! 2. PATH directories (executables found in $PATH)
//! 3. Registered aliases (e.g., "calc" → "/usr/bin/calculator")
//! 4. Bookmarked commands (user-pinned favorites)

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recent commands.
const MAX_RECENT: usize = 256;

/// Maximum registered aliases.
const MAX_ALIASES: usize = 512;

/// Maximum path entries in the search path.
const MAX_PATH_ENTRIES: usize = 64;

/// Maximum completions to return.
const MAX_COMPLETIONS: usize = 50;

/// Maximum bookmarked commands.
const MAX_BOOKMARKS: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A recent command entry.
#[derive(Debug, Clone)]
pub struct RecentCommand {
    /// The command string as typed.
    pub command: String,
    /// The resolved executable path (if found).
    pub resolved_path: String,
    /// Timestamp (nanoseconds, monotonic).
    pub timestamp_ns: u64,
    /// Number of times this command was run.
    pub run_count: u64,
}

/// A completion suggestion.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The suggested text.
    pub text: String,
    /// Source of this completion.
    pub source: CompletionSource,
    /// Resolved path (if available).
    pub path: String,
    /// Description/tooltip.
    pub description: String,
}

/// Where a completion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionSource {
    /// From recent command history.
    Recent,
    /// From PATH search.
    Path,
    /// From registered alias.
    Alias,
    /// From bookmarks.
    Bookmark,
}

/// Result of resolving a command.
#[derive(Debug, Clone)]
pub struct ResolveResult {
    /// The resolved executable path.
    pub path: String,
    /// Arguments (if command included arguments).
    pub args: Vec<String>,
    /// How it was resolved.
    pub source: CompletionSource,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct RunDialogState {
    /// Recent commands (newest first).
    recent: Vec<RecentCommand>,
    /// Aliases: short name → executable path.
    aliases: BTreeMap<String, String>,
    /// PATH directories to search.
    path_dirs: Vec<String>,
    /// Known executables found via PATH (name → full path).
    path_cache: BTreeMap<String, String>,
    /// Bookmarked favorite commands.
    bookmarks: Vec<String>,
}

impl RunDialogState {
    const fn new() -> Self {
        Self {
            recent: Vec::new(),
            aliases: BTreeMap::new(),
            path_dirs: Vec::new(),
            path_cache: BTreeMap::new(),
            bookmarks: Vec::new(),
        }
    }
}

static STATE: Mutex<RunDialogState> = Mutex::new(RunDialogState::new());
static RUN_COUNT: AtomicU64 = AtomicU64::new(0);
static COMPLETION_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// ASCII-lowercase.
fn to_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push((c as u8 + 32) as char);
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Record a command execution in history.
pub fn record(command: &str, resolved_path: &str) {
    RUN_COUNT.fetch_add(1, Ordering::Relaxed);

    if command.is_empty() {
        return;
    }

    let now = crate::timekeeping::clock_monotonic();
    let mut state = STATE.lock();

    // Check if already in recent (update count + timestamp).
    let cmd_lower = to_lower(command);
    for entry in &mut state.recent {
        if to_lower(&entry.command) == cmd_lower {
            entry.timestamp_ns = now;
            entry.run_count = entry.run_count.saturating_add(1);
            if !resolved_path.is_empty() {
                entry.resolved_path = String::from(resolved_path);
            }
            // Move to front by sorting (newest first).
            state.recent.sort_by_key(|e| core::cmp::Reverse(e.timestamp_ns));
            return;
        }
    }

    // New entry.
    if state.recent.len() >= MAX_RECENT {
        state.recent.pop(); // Remove oldest.
    }
    state.recent.insert(0, RecentCommand {
        command: String::from(command),
        resolved_path: String::from(resolved_path),
        timestamp_ns: now,
        run_count: 1,
    });
}

/// Get recent commands (newest first).
pub fn recent(limit: usize) -> Vec<RecentCommand> {
    let state = STATE.lock();
    state.recent.iter()
        .take(if limit == 0 { MAX_RECENT } else { limit })
        .cloned()
        .collect()
}

/// Clear recent history.
pub fn clear_recent() {
    let mut state = STATE.lock();
    state.recent.clear();
}

/// Remove a specific command from recent history.
pub fn remove_recent(command: &str) -> KernelResult<()> {
    let cmd_lower = to_lower(command);
    let mut state = STATE.lock();
    let len_before = state.recent.len();
    state.recent.retain(|e| to_lower(&e.command) != cmd_lower);
    if state.recent.len() == len_before {
        Err(KernelError::NotFound)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Aliases
// ---------------------------------------------------------------------------

/// Register an alias (short name → executable path).
pub fn register_alias(name: &str, path: &str) -> KernelResult<()> {
    if name.is_empty() || path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    if !state.aliases.contains_key(name) && state.aliases.len() >= MAX_ALIASES {
        return Err(KernelError::ResourceExhausted);
    }
    state.aliases.insert(String::from(name), String::from(path));
    Ok(())
}

/// Remove an alias.
pub fn remove_alias(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    state.aliases.remove(name).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// List all aliases.
pub fn list_aliases() -> Vec<(String, String)> {
    let state = STATE.lock();
    state.aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

// ---------------------------------------------------------------------------
// PATH management
// ---------------------------------------------------------------------------

/// Set the PATH directories to search for executables.
pub fn set_path(dirs: &[&str]) -> KernelResult<()> {
    if dirs.len() > MAX_PATH_ENTRIES {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.path_dirs = dirs.iter().map(|d| String::from(*d)).collect();
    Ok(())
}

/// Get current PATH directories.
pub fn get_path() -> Vec<String> {
    let state = STATE.lock();
    state.path_dirs.clone()
}

/// Register an executable found in PATH (pre-cache for fast completion).
pub fn register_executable(name: &str, full_path: &str) -> KernelResult<()> {
    if name.is_empty() || full_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.path_cache.insert(String::from(name), String::from(full_path));
    Ok(())
}

/// Refresh PATH cache by scanning directories.
///
/// This is a simplified version that uses VFS readdir to find executables.
pub fn refresh_path_cache() -> KernelResult<usize> {
    use crate::fs::Vfs;

    let state = STATE.lock();
    let dirs = state.path_dirs.clone();
    drop(state);

    let mut found = 0;
    for dir in &dirs {
        if let Ok(entries) = Vfs::readdir(dir) {
            for entry in &entries {
                if entry.entry_type == crate::fs::EntryType::File {
                    let full_path = if dir.ends_with('/') {
                        alloc::format!("{}{}", dir, entry.name)
                    } else {
                        alloc::format!("{}/{}", dir, entry.name)
                    };
                    let mut state = STATE.lock();
                    state.path_cache.insert(entry.name.clone(), full_path);
                    found += 1;
                }
            }
        }
    }

    Ok(found)
}

/// Clear the PATH cache.
pub fn clear_path_cache() {
    let mut state = STATE.lock();
    state.path_cache.clear();
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

/// Add a command to bookmarks (favorites).
pub fn add_bookmark(command: &str) -> KernelResult<()> {
    if command.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let cmd = String::from(command);
    if state.bookmarks.contains(&cmd) {
        return Ok(()); // Already bookmarked.
    }
    if state.bookmarks.len() >= MAX_BOOKMARKS {
        return Err(KernelError::ResourceExhausted);
    }
    state.bookmarks.push(cmd);
    Ok(())
}

/// Remove a bookmark.
pub fn remove_bookmark(command: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len_before = state.bookmarks.len();
    state.bookmarks.retain(|c| c != command);
    if state.bookmarks.len() == len_before {
        Err(KernelError::NotFound)
    } else {
        Ok(())
    }
}

/// List bookmarked commands.
pub fn list_bookmarks() -> Vec<String> {
    let state = STATE.lock();
    state.bookmarks.clone()
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

/// Get completion suggestions for a prefix.
///
/// Returns suggestions from all sources, ordered by relevance:
/// 1. Bookmarks matching prefix
/// 2. Recent commands matching prefix (most recent first)
/// 3. PATH executables matching prefix
/// 4. Aliases matching prefix
pub fn completions(prefix: &str) -> Vec<Completion> {
    COMPLETION_COUNT.fetch_add(1, Ordering::Relaxed);

    if prefix.is_empty() {
        // Return recent commands as suggestions.
        let state = STATE.lock();
        return state.recent.iter()
            .take(MAX_COMPLETIONS)
            .map(|e| Completion {
                text: e.command.clone(),
                source: CompletionSource::Recent,
                path: e.resolved_path.clone(),
                description: alloc::format!("run {} times", e.run_count),
            })
            .collect();
    }

    let prefix_lower = to_lower(prefix);
    let state = STATE.lock();
    let mut results = Vec::new();
    let mut seen = BTreeSet::new();

    // 1. Bookmarks.
    for cmd in &state.bookmarks {
        if to_lower(cmd).starts_with(&prefix_lower) && seen.insert(to_lower(cmd)) {
            results.push(Completion {
                text: cmd.clone(),
                source: CompletionSource::Bookmark,
                path: String::new(),
                description: String::from("bookmarked"),
            });
        }
    }

    // 2. Recent commands.
    for entry in &state.recent {
        let entry_lower = to_lower(&entry.command);
        if entry_lower.starts_with(&prefix_lower) && seen.insert(entry_lower) {
            results.push(Completion {
                text: entry.command.clone(),
                source: CompletionSource::Recent,
                path: entry.resolved_path.clone(),
                description: alloc::format!("run {} times", entry.run_count),
            });
        }
    }

    // 3. PATH executables.
    for (name, full_path) in &state.path_cache {
        if to_lower(name).starts_with(&prefix_lower) && seen.insert(to_lower(name)) {
            results.push(Completion {
                text: name.clone(),
                source: CompletionSource::Path,
                path: full_path.clone(),
                description: full_path.clone(),
            });
        }
    }

    // 4. Aliases.
    for (name, target) in &state.aliases {
        if to_lower(name).starts_with(&prefix_lower) && seen.insert(to_lower(name)) {
            results.push(Completion {
                text: name.clone(),
                source: CompletionSource::Alias,
                path: target.clone(),
                description: alloc::format!("alias → {}", target),
            });
        }
    }

    results.truncate(MAX_COMPLETIONS);
    results
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Resolve a command string to an executable path.
///
/// Resolution order:
/// 1. If it starts with '/', treat as absolute path
/// 2. Check aliases
/// 3. Check PATH cache
/// 4. Search PATH directories directly
pub fn resolve(command: &str) -> KernelResult<ResolveResult> {
    if command.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Split command and arguments.
    let mut parts = command.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args_str = parts.next().unwrap_or("");
    let args: Vec<String> = if args_str.is_empty() {
        Vec::new()
    } else {
        args_str.split_whitespace().map(String::from).collect()
    };

    // 1. Absolute path.
    if cmd.starts_with('/') {
        return Ok(ResolveResult {
            path: String::from(cmd),
            args,
            source: CompletionSource::Path,
        });
    }

    let state = STATE.lock();

    // 2. Alias.
    if let Some(alias_path) = state.aliases.get(cmd) {
        return Ok(ResolveResult {
            path: alias_path.clone(),
            args,
            source: CompletionSource::Alias,
        });
    }

    // 3. PATH cache.
    if let Some(cached_path) = state.path_cache.get(cmd) {
        return Ok(ResolveResult {
            path: cached_path.clone(),
            args,
            source: CompletionSource::Path,
        });
    }

    // 4. Search PATH directories.
    for dir in &state.path_dirs {
        let full = if dir.ends_with('/') {
            alloc::format!("{}{}", dir, cmd)
        } else {
            alloc::format!("{}/{}", dir, cmd)
        };
        // Check if file exists via VFS.
        if crate::fs::Vfs::metadata(&full).is_ok() {
            return Ok(ResolveResult {
                path: full,
                args,
                source: CompletionSource::Path,
            });
        }
    }

    Err(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Built-in default PATH and aliases
// ---------------------------------------------------------------------------

/// Initialize with default PATH and common aliases.
pub fn init_defaults() -> KernelResult<()> {
    set_path(&["/bin", "/usr/bin", "/usr/local/bin", "/sbin", "/usr/sbin"])?;
    register_alias("calc", "/usr/bin/calculator")?;
    register_alias("editor", "/usr/bin/text-editor")?;
    register_alias("files", "/usr/bin/file-manager")?;
    register_alias("term", "/usr/bin/terminal")?;
    register_alias("settings", "/usr/bin/settings")?;
    register_alias("sysinfo", "/usr/bin/system-info")?;
    register_alias("explorer", "/usr/bin/file-manager")?;
    register_alias("notepad", "/usr/bin/text-editor")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (recent_count, alias_count, path_cache_count, bookmark_count,
///          run_ops, completion_ops).
pub fn stats() -> (usize, usize, usize, usize, u64, u64) {
    let state = STATE.lock();
    (
        state.recent.len(),
        state.aliases.len(),
        state.path_cache.len(),
        state.bookmarks.len(),
        RUN_COUNT.load(Ordering::Relaxed),
        COMPLETION_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    RUN_COUNT.store(0, Ordering::Relaxed);
    COMPLETION_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = STATE.lock();
    state.recent.clear();
    state.aliases.clear();
    state.path_dirs.clear();
    state.path_cache.clear();
    state.bookmarks.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the run dialog module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: record and retrieve recent commands.
    {
        record("file-manager", "/usr/bin/file-manager");
        record("terminal", "/usr/bin/terminal");
        record("calculator", "/usr/bin/calculator");
        let r = recent(10);
        assert_eq!(r.len(), 3);
        // Most recent first.
        assert_eq!(r[0].command, "calculator");
        serial_println!("[rundialog] test 1 passed: record/recent");
    }

    // Test 2: duplicate command updates count.
    {
        record("terminal", "/usr/bin/terminal");
        let r = recent(10);
        assert_eq!(r.len(), 3); // Still 3, not 4.
        // Terminal should be first (most recent) with count 2.
        assert_eq!(r[0].command, "terminal");
        assert_eq!(r[0].run_count, 2);
        serial_println!("[rundialog] test 2 passed: duplicate handling");
    }

    // Test 3: aliases.
    {
        register_alias("calc", "/usr/bin/calculator")?;
        register_alias("fm", "/usr/bin/file-manager")?;
        let aliases = list_aliases();
        assert_eq!(aliases.len(), 2);

        let resolved = resolve("calc")?;
        assert_eq!(resolved.path, "/usr/bin/calculator");
        assert_eq!(resolved.source, CompletionSource::Alias);
        serial_println!("[rundialog] test 3 passed: aliases");
    }

    // Test 4: PATH cache and resolution.
    {
        register_executable("ls", "/bin/ls")?;
        register_executable("cat", "/bin/cat")?;
        let resolved = resolve("ls")?;
        assert_eq!(resolved.path, "/bin/ls");
        assert_eq!(resolved.source, CompletionSource::Path);
        serial_println!("[rundialog] test 4 passed: PATH resolution");
    }

    // Test 5: completions.
    {
        let comps = completions("c");
        // Should match: calculator (recent), calc (alias), cat (PATH).
        assert!(comps.len() >= 2);
        // Check that different sources are represented.
        let sources: BTreeSet<_> = comps.iter().map(|c| c.source).collect();
        assert!(sources.len() >= 2);
        serial_println!("[rundialog] test 5 passed: completions");
    }

    // Test 6: bookmarks.
    {
        add_bookmark("ssh server1")?;
        add_bookmark("rsync --backup")?;
        let bm = list_bookmarks();
        assert_eq!(bm.len(), 2);

        let comps = completions("ssh");
        assert!(comps.iter().any(|c| c.source == CompletionSource::Bookmark));
        serial_println!("[rundialog] test 6 passed: bookmarks");
    }

    // Test 7: remove and absolute path resolution.
    {
        remove_recent("terminal")?;
        let r = recent(10);
        assert_eq!(r.len(), 2); // Down from 3.

        let resolved = resolve("/usr/bin/custom-app --flag")?;
        assert_eq!(resolved.path, "/usr/bin/custom-app");
        assert_eq!(resolved.args.len(), 1);
        assert_eq!(resolved.args[0], "--flag");
        serial_println!("[rundialog] test 7 passed: remove/absolute path");
    }

    clear_all();
    reset_stats();

    serial_println!("[rundialog] all 7 self-tests passed");
    Ok(())
}
