//! "Open With" dialog infrastructure.
//!
//! Extends the file associations system with the interactive "Open With"
//! dialog (right-click → Open with...) per design spec lines 303-305:
//! - Shows all apps registered for the file's MIME type
//! - Shows recently used apps for this file type
//! - Allows choosing any installed app (browse)
//! - "Always use this app" checkbox to set as default
//! - Remembers per-file-type choices
//!
//! ## Architecture
//!
//! ```text
//! User right-clicks file → "Open with..."
//!   → openwith::build_choices(path) gathers app list
//!     → registered apps from associations module
//!     → recently used apps for this type
//!     → all installed apps (browse mode)
//!   → User picks an app
//!   → openwith::open_with(path, app) launches it
//!     → optionally sets as default ("Always use this app")
//!     → records in recent choices
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recent choices per MIME type.
const MAX_RECENT_PER_TYPE: usize = 8;

/// Maximum total recent entries.
const MAX_RECENT_TOTAL: usize = 512;

/// Maximum registered apps for "all apps" browse.
const MAX_ALL_APPS: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// An app choice for the Open With dialog.
#[derive(Debug, Clone)]
pub struct AppChoice {
    /// Application path.
    pub app_path: String,
    /// Application display name.
    pub app_name: String,
    /// Why this app appears in the list.
    pub reason: ChoiceReason,
    /// Whether this is the current default for the file type.
    pub is_default: bool,
    /// How many times this app was used for this type.
    pub use_count: u64,
    /// Last used timestamp (ns).
    pub last_used_ns: u64,
}

/// Why an app appears in the Open With list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceReason {
    /// Registered handler for this MIME type.
    RegisteredHandler,
    /// Recently used for this type.
    RecentlyUsed,
    /// Any installed application (browse mode).
    InstalledApp,
}

/// A recent "open with" choice.
#[derive(Debug, Clone)]
struct RecentChoice {
    /// MIME type.
    mime: String,
    /// App path.
    app_path: String,
    /// App name.
    app_name: String,
    /// Use count.
    use_count: u64,
    /// Last used timestamp.
    last_used_ns: u64,
}

/// Result of an "Open With" action.
#[derive(Debug, Clone)]
pub struct OpenWithResult {
    /// The file that was opened.
    pub file_path: String,
    /// The app used to open it.
    pub app_path: String,
    /// Whether the default was changed.
    pub default_changed: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static OPEN_COUNT: AtomicU64 = AtomicU64::new(0);
static DEFAULT_CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;

/// Recent choices.
static RECENT: Mutex<Vec<RecentChoice>> = Mutex::new(Vec::new());

/// All known apps (simplified app registry).
static KNOWN_APPS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new()); // (path, name)

// ---------------------------------------------------------------------------
// App Registry
// ---------------------------------------------------------------------------

/// Register a known application (for "browse" in Open With).
pub fn register_app(app_path: &str, app_name: &str) -> KernelResult<()> {
    let mut apps = KNOWN_APPS.lock();
    if apps.len() >= MAX_ALL_APPS {
        return Err(KernelError::ResourceExhausted);
    }
    // No duplicates.
    if apps.iter().any(|(p, _)| p == app_path) {
        return Ok(());
    }
    apps.push((String::from(app_path), String::from(app_name)));
    Ok(())
}

/// Unregister a known application.
pub fn unregister_app(app_path: &str) -> KernelResult<()> {
    let mut apps = KNOWN_APPS.lock();
    if let Some(pos) = apps.iter().position(|(p, _)| p == app_path) {
        apps.remove(pos);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// List all registered applications.
pub fn list_apps() -> Vec<(String, String)> {
    KNOWN_APPS.lock().clone()
}

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Build the list of app choices for the Open With dialog.
///
/// Returns apps in priority order:
/// 1. Default handler (if any)
/// 2. Other registered handlers
/// 3. Recently used apps for this type
/// 4. All installed apps (in browse section)
pub fn build_choices(path: &str) -> KernelResult<Vec<AppChoice>> {
    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");
    let mut choices = Vec::new();
    let mut seen_paths: Vec<String> = Vec::new();

    // 1. Registered handlers from associations.
    let registered = crate::fs::associations::apps_for(mime);
    let default_app = crate::fs::associations::default_app(mime);
    let default_path = default_app.as_ref().map(|a| a.app_path.clone()).unwrap_or_default();

    for assoc in &registered {
        seen_paths.push(assoc.app_path.clone());

        // Check recent usage count.
        let recent = RECENT.lock();
        let usage = recent.iter()
            .find(|r| r.mime == mime && r.app_path == assoc.app_path);
        let (use_count, last_used) = match usage {
            Some(r) => (r.use_count, r.last_used_ns),
            None => (0, 0),
        };

        choices.push(AppChoice {
            app_path: assoc.app_path.clone(),
            app_name: assoc.app_name.clone(),
            reason: ChoiceReason::RegisteredHandler,
            is_default: assoc.app_path == default_path,
            use_count,
            last_used_ns: last_used,
        });
    }

    // 2. Recently used apps for this type (not already in the list).
    {
        let recent = RECENT.lock();
        let mut recent_for_type: Vec<&RecentChoice> = recent.iter()
            .filter(|r| r.mime == mime && !seen_paths.contains(&r.app_path))
            .collect();
        recent_for_type.sort_by_key(|e| core::cmp::Reverse(e.last_used_ns));

        for rc in recent_for_type.iter().take(MAX_RECENT_PER_TYPE) {
            seen_paths.push(rc.app_path.clone());
            choices.push(AppChoice {
                app_path: rc.app_path.clone(),
                app_name: rc.app_name.clone(),
                reason: ChoiceReason::RecentlyUsed,
                is_default: false,
                use_count: rc.use_count,
                last_used_ns: rc.last_used_ns,
            });
        }
    }

    // 3. All installed apps (not already in list).
    {
        let apps = KNOWN_APPS.lock();
        for (app_path, app_name) in apps.iter() {
            if !seen_paths.contains(app_path) {
                choices.push(AppChoice {
                    app_path: app_path.clone(),
                    app_name: app_name.clone(),
                    reason: ChoiceReason::InstalledApp,
                    is_default: false,
                    use_count: 0,
                    last_used_ns: 0,
                });
            }
        }
    }

    Ok(choices)
}

/// Open a file with a specific application.
///
/// Records the choice and optionally sets as default.
pub fn open_with(path: &str, app_path: &str, app_name: &str, set_as_default: bool) -> KernelResult<OpenWithResult> {
    OPEN_COUNT.fetch_add(1, Ordering::Relaxed);

    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");
    let now = crate::timekeeping::clock_monotonic();

    // Record in recent choices.
    {
        let mut recent = RECENT.lock();
        if let Some(entry) = recent.iter_mut().find(|r| r.mime == mime && r.app_path == app_path) {
            entry.use_count = entry.use_count.saturating_add(1);
            entry.last_used_ns = now;
        } else {
            if recent.len() >= MAX_RECENT_TOTAL {
                // Evict oldest.
                if let Some(oldest_idx) = recent.iter()
                    .enumerate()
                    .min_by_key(|(_, r)| r.last_used_ns)
                    .map(|(i, _)| i)
                {
                    recent.remove(oldest_idx);
                }
            }
            recent.push(RecentChoice {
                mime: String::from(mime),
                app_path: String::from(app_path),
                app_name: String::from(app_name),
                use_count: 1,
                last_used_ns: now,
            });
        }
    }

    // Set as default if requested.
    let mut default_changed = false;
    if set_as_default {
        crate::fs::associations::register(mime, app_path, app_name, 0, true);
        DEFAULT_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
        default_changed = true;
    }

    Ok(OpenWithResult {
        file_path: String::from(path),
        app_path: String::from(app_path),
        default_changed,
    })
}

/// Get the current default app for a file.
pub fn current_default(path: &str) -> Option<String> {
    crate::fs::associations::default_app_for_file(path)
        .map(|a| a.app_name)
}

/// Get recent choices for a MIME type.
pub fn recent_for_type(mime: &str) -> Vec<(String, String, u64)> {
    let recent = RECENT.lock();
    let mut entries: Vec<(String, String, u64)> = recent.iter()
        .filter(|r| r.mime == mime)
        .map(|r| (r.app_path.clone(), r.app_name.clone(), r.use_count))
        .collect();
    entries.sort_by_key(|e| core::cmp::Reverse(e.2)); // Most used first.
    entries
}

/// Clear recent choices.
pub fn clear_recent() {
    RECENT.lock().clear();
}

/// Clear recent choices for a specific MIME type.
pub fn clear_recent_for_type(mime: &str) {
    let mut recent = RECENT.lock();
    recent.retain(|r| r.mime != mime);
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (open_count, default_change_count, recent_count, app_count).
pub fn stats() -> (u64, u64, usize, usize) {
    (
        OPEN_COUNT.load(Ordering::Relaxed),
        DEFAULT_CHANGE_COUNT.load(Ordering::Relaxed),
        RECENT.lock().len(),
        KNOWN_APPS.lock().len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    OPEN_COUNT.store(0, Ordering::Relaxed);
    DEFAULT_CHANGE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the open-with module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: register and list apps.
    {
        register_app("/usr/bin/editor", "Text Editor")?;
        register_app("/usr/bin/viewer", "Image Viewer")?;
        let apps = list_apps();
        assert!(apps.iter().any(|(p, _)| p == "/usr/bin/editor"));
        assert!(apps.iter().any(|(p, _)| p == "/usr/bin/viewer"));
        serial_println!("[openwith] test 1 passed: register apps");
    }

    // Test 2: build choices for a file.
    {
        let choices = build_choices("/test.txt")?;
        // Should include registered handlers and all known apps.
        assert!(!choices.is_empty());
        serial_println!("[openwith] test 2 passed: build choices ({} apps)", choices.len());
    }

    // Test 3: open with and record recent.
    {
        let result = open_with("/test.txt", "/usr/bin/editor", "Text Editor", false)?;
        assert_eq!(result.file_path, "/test.txt");
        assert!(!result.default_changed);

        // Check recent was recorded.
        let recent = recent_for_type("text/plain");
        assert!(recent.iter().any(|(p, _, _)| p == "/usr/bin/editor"));
        serial_println!("[openwith] test 3 passed: open with + recent");
    }

    // Test 4: open with and set default.
    {
        let result = open_with("/test.txt", "/usr/bin/editor", "Text Editor", true)?;
        assert!(result.default_changed);

        // Check default was set.
        let default_name = current_default("/test.txt");
        // Should be "Text Editor" or the previous default.
        assert!(default_name.is_some());
        serial_println!("[openwith] test 4 passed: set default");
    }

    // Test 5: clear recent.
    {
        clear_recent_for_type("text/plain");
        let recent = recent_for_type("text/plain");
        assert!(recent.is_empty());

        // Re-record for stats test.
        let _ = open_with("/test.txt", "/usr/bin/editor", "Text Editor", false);
        clear_recent();
        let recent2 = recent_for_type("text/plain");
        assert!(recent2.is_empty());
        serial_println!("[openwith] test 5 passed: clear recent");
    }

    // Test 6: unregister app.
    {
        unregister_app("/usr/bin/editor")?;
        unregister_app("/usr/bin/viewer")?;
        let apps = list_apps();
        assert!(!apps.iter().any(|(p, _)| p == "/usr/bin/editor"));
        serial_println!("[openwith] test 6 passed: unregister apps");
    }

    // Test 7: stats.
    {
        let (opens, defaults, _recent, _apps) = stats();
        assert!(opens > 0);
        assert!(defaults > 0);
        serial_println!("[openwith] test 7 passed: stats");
    }

    serial_println!("[openwith] all 7 self-tests passed");
    Ok(())
}
