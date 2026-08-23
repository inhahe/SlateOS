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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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
    ///
    /// Bytes, not `String`: the first word of a command line is a filename,
    /// and a filename may contain any byte but `/` and NUL.  Requiring UTF-8
    /// here would make an executable with such a name un-runnable and
    /// un-recordable even though the filesystem holding it accepts the name.
    pub command: Vec<u8>,
    /// The resolved executable path, or `None` if resolution failed.
    ///
    /// This was a `String` that was empty when resolution failed; the empty
    /// path is not a path a file can have, but `Option` says so in the type
    /// instead of relying on the reader to know the convention.
    pub resolved_path: Option<PathBuf>,
    /// Timestamp (nanoseconds, monotonic).
    pub timestamp_ns: u64,
    /// Number of times this command was run.
    pub run_count: u64,
}

/// A completion suggestion.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The suggested text (bytes - see [`RecentCommand::command`]).
    pub text: Vec<u8>,
    /// Source of this completion.
    pub source: CompletionSource,
    /// Resolved path, or `None` when the suggestion has no path (a bookmark
    /// or an unresolved recent command).
    pub path: Option<PathBuf>,
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
    pub path: PathBuf,
    /// Arguments (if command included arguments), as raw bytes: argv is not
    /// required to be text any more than a path is.
    pub args: Vec<Vec<u8>>,
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
    aliases: BTreeMap<Vec<u8>, PathBuf>,
    /// PATH directories to search.
    path_dirs: Vec<PathBuf>,
    /// Known executables found via PATH (name → full path).
    ///
    /// Keyed by the filename exactly as `readdir` reported it, so an
    /// executable whose name is not UTF-8 is still cached and still
    /// completable.
    path_cache: BTreeMap<PathBuf, PathBuf>,
    /// Bookmarked favorite commands.
    bookmarks: Vec<Vec<u8>>,
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

/// ASCII-lowercase, over bytes.
///
/// Only ASCII is folded.  Command matching must be total over the byte
/// strings a filename can be, and there is no encoding to consult that would
/// tell us how to case-fold a byte >= 0x80 - so those bytes are left alone,
/// which is exactly what the old `char`-based version did for non-ASCII too.
fn to_lower(s: &[u8]) -> Vec<u8> {
    s.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Record a command execution in history.
pub fn record(command: impl AsRef<[u8]>, resolved_path: Option<&Path>) {
    let command = command.as_ref();
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
            if let Some(p) = resolved_path {
                entry.resolved_path = Some(p.to_path_buf());
            }
            // Move to front by sorting (newest first).
            state
                .recent
                .sort_by_key(|e| core::cmp::Reverse(e.timestamp_ns));
            return;
        }
    }

    // New entry.
    if state.recent.len() >= MAX_RECENT {
        state.recent.pop(); // Remove oldest.
    }
    state.recent.insert(
        0,
        RecentCommand {
            command: command.to_vec(),
            resolved_path: resolved_path.map(Path::to_path_buf),
            timestamp_ns: now,
            run_count: 1,
        },
    );
}

/// Get recent commands (newest first).
pub fn recent(limit: usize) -> Vec<RecentCommand> {
    let state = STATE.lock();
    state
        .recent
        .iter()
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
pub fn remove_recent(command: impl AsRef<[u8]>) -> KernelResult<()> {
    let cmd_lower = to_lower(command.as_ref());
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
pub fn register_alias(name: impl AsRef<[u8]>, path: impl AsRef<Path>) -> KernelResult<()> {
    let (name, path) = (name.as_ref(), path.as_ref());
    if name.is_empty() || path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    if !state.aliases.contains_key(name) && state.aliases.len() >= MAX_ALIASES {
        return Err(KernelError::ResourceExhausted);
    }
    state.aliases.insert(name.to_vec(), path.to_path_buf());
    Ok(())
}

/// Remove an alias.
pub fn remove_alias(name: impl AsRef<[u8]>) -> KernelResult<()> {
    let mut state = STATE.lock();
    state
        .aliases
        .remove(name.as_ref())
        .ok_or(KernelError::NotFound)?;
    Ok(())
}

/// List all aliases.
pub fn list_aliases() -> Vec<(Vec<u8>, PathBuf)> {
    let state = STATE.lock();
    state
        .aliases
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// PATH management
// ---------------------------------------------------------------------------

/// Set the PATH directories to search for executables.
pub fn set_path<P: AsRef<Path>>(dirs: &[P]) -> KernelResult<()> {
    if dirs.len() > MAX_PATH_ENTRIES {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state.path_dirs = dirs.iter().map(|d| d.as_ref().to_path_buf()).collect();
    Ok(())
}

/// Get current PATH directories.
pub fn get_path() -> Vec<PathBuf> {
    let state = STATE.lock();
    state.path_dirs.clone()
}

/// Register an executable found in PATH (pre-cache for fast completion).
pub fn register_executable(
    name: impl AsRef<Path>,
    full_path: impl AsRef<Path>,
) -> KernelResult<()> {
    let (name, full_path) = (name.as_ref(), full_path.as_ref());
    if name.is_empty() || full_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    state
        .path_cache
        .insert(name.to_path_buf(), full_path.to_path_buf());
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
                    // `Path::join` collapses a trailing separator on `dir`
                    // itself, so the old `dir.ends_with('/')` branch is gone.
                    let full_path = dir.join(&entry.name);
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
pub fn add_bookmark(command: impl AsRef<[u8]>) -> KernelResult<()> {
    let command = command.as_ref();
    if command.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let cmd = command.to_vec();
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
pub fn remove_bookmark(command: impl AsRef<[u8]>) -> KernelResult<()> {
    let command = command.as_ref();
    let mut state = STATE.lock();
    let len_before = state.bookmarks.len();
    state.bookmarks.retain(|c| c.as_slice() != command);
    if state.bookmarks.len() == len_before {
        Err(KernelError::NotFound)
    } else {
        Ok(())
    }
}

/// List bookmarked commands.
pub fn list_bookmarks() -> Vec<Vec<u8>> {
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
pub fn completions(prefix: impl AsRef<[u8]>) -> Vec<Completion> {
    let prefix = prefix.as_ref();
    COMPLETION_COUNT.fetch_add(1, Ordering::Relaxed);

    if prefix.is_empty() {
        // Return recent commands as suggestions.
        let state = STATE.lock();
        return state
            .recent
            .iter()
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
                // A bookmark is a command line, not a resolved location: it
                // has no path until `resolve` is run on it.
                path: None,
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
        let name_lower = to_lower(name.as_bytes());
        if name_lower.starts_with(&prefix_lower) && seen.insert(name_lower) {
            results.push(Completion {
                text: name.as_bytes().to_vec(),
                source: CompletionSource::Path,
                path: Some(full_path.clone()),
                // The description is human-facing only, so a lossy render is
                // acceptable here where it would not be in `path`.
                description: alloc::format!("{}", full_path.display()),
            });
        }
    }

    // 4. Aliases.
    for (name, target) in &state.aliases {
        if to_lower(name).starts_with(&prefix_lower) && seen.insert(to_lower(name)) {
            results.push(Completion {
                text: name.clone(),
                source: CompletionSource::Alias,
                path: Some(target.clone()),
                description: alloc::format!("alias → {}", target.display()),
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
pub fn resolve(command: impl AsRef<[u8]>) -> KernelResult<ResolveResult> {
    let command = command.as_ref();
    if command.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Split command and arguments.  Splitting on ASCII space and the usual
    // whitespace bytes is safe over raw bytes because none of them can be a
    // continuation byte of anything: this is a byte-oriented word split, the
    // same one a POSIX shell performs.
    let mut parts = command.splitn(2, |&b| b == b' ');
    let cmd = parts.next().unwrap_or(&[]);
    let args_str = parts.next().unwrap_or(&[]);
    let args: Vec<Vec<u8>> = args_str
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    let cmd = Path::new(cmd);

    // 1. Absolute path.
    if cmd.is_absolute() {
        return Ok(ResolveResult {
            path: cmd.to_path_buf(),
            args,
            source: CompletionSource::Path,
        });
    }

    let state = STATE.lock();

    // 2. Alias.
    if let Some(alias_path) = state.aliases.get(cmd.as_bytes()) {
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
        let full = dir.join(cmd);
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
///
/// The suite asserts exact table contents, so it needs a table of its own.
/// It used to get one by calling `clear_all()`, which — since this suite is
/// reachable from the shell — deleted whatever the user had stored here and
/// then reported success.  The live state is moved aside for the duration and
/// put back afterwards; `crate::fs::selftest` records why this shape rather
/// than the alternatives.
pub fn self_test() -> KernelResult<()> {
    // These counters live outside the table, so `with_pristine` cannot
    // see them; save and restore them here so a run leaves no trace.
    let saved_run_count = RUN_COUNT.load(Ordering::Relaxed);
    let saved_completion_count = COMPLETION_COUNT.load(Ordering::Relaxed);
    let result = crate::fs::selftest::with_pristine(&STATE, RunDialogState::new(), self_test_inner);
    RUN_COUNT.store(saved_run_count, Ordering::Relaxed);
    COMPLETION_COUNT.store(saved_completion_count, Ordering::Relaxed);
    result
}

fn self_test_inner() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: record and retrieve recent commands.
    {
        record("file-manager", Some(Path::new("/usr/bin/file-manager")));
        record("terminal", Some(Path::new("/usr/bin/terminal")));
        record("calculator", Some(Path::new("/usr/bin/calculator")));
        let r = recent(10);
        assert_eq!(r.len(), 3);
        // Most recent first.
        assert_eq!(r[0].command, b"calculator");
        serial_println!("[rundialog] test 1 passed: record/recent");
    }

    // Test 2: duplicate command updates count.
    {
        record("terminal", Some(Path::new("/usr/bin/terminal")));
        let r = recent(10);
        assert_eq!(r.len(), 3); // Still 3, not 4.
        // Terminal should be first (most recent) with count 2.
        assert_eq!(r[0].command, b"terminal");
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
        assert_eq!(resolved.path.as_path(), Path::new("/usr/bin/calculator"));
        assert_eq!(resolved.source, CompletionSource::Alias);
        serial_println!("[rundialog] test 3 passed: aliases");
    }

    // Test 4: PATH cache and resolution.
    {
        register_executable("ls", "/bin/ls")?;
        register_executable("cat", "/bin/cat")?;
        let resolved = resolve("ls")?;
        assert_eq!(resolved.path.as_path(), Path::new("/bin/ls"));
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
        assert_eq!(resolved.path.as_path(), Path::new("/usr/bin/custom-app"));
        assert_eq!(resolved.args.len(), 1);
        assert_eq!(resolved.args[0], b"--flag");
        serial_println!("[rundialog] test 7 passed: remove/absolute path");
    }

    // Test 8: a command whose name is not UTF-8 still resolves and completes.
    //
    // This is the whole point of the byte typing: `readdir` can hand us a
    // name like this from any filesystem that stores names as bytes, and the
    // run dialog must be able to cache, complete and resolve it.
    {
        let odd = Path::new(b"we\xffird-app".as_slice());
        register_executable(odd, Path::new(b"/bin/we\xffird-app".as_slice()))?;

        let resolved = resolve(b"we\xffird-app --go".as_slice())?;
        assert_eq!(
            resolved.path.as_path(),
            Path::new(b"/bin/we\xffird-app".as_slice())
        );
        assert_eq!(resolved.args, alloc::vec![b"--go".to_vec()]);

        let comps = completions(b"we\xff".as_slice());
        assert!(comps.iter().any(|c| c.text == b"we\xffird-app"));
        serial_println!("[rundialog] test 8 passed: non-UTF-8 executable name");
    }

    clear_all();
    reset_stats();

    serial_println!("[rundialog] all 7 self-tests passed");
    Ok(())
}
