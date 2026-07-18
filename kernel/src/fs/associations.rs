//! File type association registry.
//!
//! Maps MIME types (and file extensions) to application paths, enabling
//! "Open With" functionality and default application selection.
//!
//! ## Design
//!
//! - **Two-level lookup**: MIME type → app list, with extension → MIME as
//!   the first step.  This means `image/png` and `.png` both resolve the
//!   same way.
//! - **Priority ordering**: each association has a priority.  The highest-
//!   priority app is the default.  Users can reorder, add, or remove
//!   entries.
//! - **System + user layers**: system defaults are registered at boot
//!   (or by package install).  User overrides take precedence (higher
//!   priority values).
//! - **Persistent intent**: the registry is in-memory for now but designed
//!   to be serializable to a YAML config file in the future.
//!
//! ## Reference
//!
//! design.txt: "Easily discoverable UI to change associations"
//! roadmap.md line 750: "File extensions: .nx (executable), .dso (shared
//! library), .slib (static library)"

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single file type association entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Association {
    /// The application path (executable to launch).
    pub app_path: String,
    /// Human-readable application name.
    pub app_name: String,
    /// Priority (higher = preferred).  System defaults use 0-99,
    /// user overrides use 100+.
    pub priority: u32,
    /// Whether this was set by the user (vs. system default).
    pub user_set: bool,
}

/// Statistics about the association registry.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct AssociationStats {
    /// Number of MIME types with at least one association.
    pub mime_types: usize,
    /// Total number of association entries.
    pub total_entries: usize,
    /// Number of user-set associations.
    pub user_entries: usize,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct AssocInner {
    /// MIME type → list of associations, sorted by priority (highest first).
    by_mime: BTreeMap<String, Vec<Association>>,
    /// Extension → MIME type override (user can force an extension to a
    /// different MIME type than what `fs::mime` would detect).
    ext_override: BTreeMap<String, String>,
}

static ASSOC: Mutex<AssocInner> = Mutex::new(AssocInner {
    by_mime: BTreeMap::new(),
    ext_override: BTreeMap::new(),
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register an application for a MIME type.
///
/// If an association with the same `app_path` already exists for this
/// MIME type, it is updated (priority/name replaced).  Otherwise a new
/// entry is appended.  The list is re-sorted by priority.
pub fn register(mime: &str, app_path: &str, app_name: &str, priority: u32, user_set: bool) {
    let mut inner = ASSOC.lock();
    let list = inner.by_mime.entry(String::from(mime)).or_default();

    // Check for existing entry with same app_path.
    if let Some(existing) = list.iter_mut().find(|a| a.app_path == app_path) {
        existing.app_name = String::from(app_name);
        existing.priority = priority;
        existing.user_set = user_set;
    } else {
        list.push(Association {
            app_path: String::from(app_path),
            app_name: String::from(app_name),
            priority,
            user_set,
        });
    }

    // Sort by priority, highest first.
    list.sort_by_key(|e| core::cmp::Reverse(e.priority));
}

/// Unregister an application from a MIME type.
///
/// Returns true if the entry was found and removed.
pub fn unregister(mime: &str, app_path: &str) -> bool {
    let mut inner = ASSOC.lock();
    if let Some(list) = inner.by_mime.get_mut(mime) {
        let before = list.len();
        list.retain(|a| a.app_path != app_path);
        let removed = before != list.len();
        // Clean up empty lists.
        if list.is_empty() {
            inner.by_mime.remove(mime);
        }
        removed
    } else {
        false
    }
}

/// Get the default application for a MIME type.
///
/// Returns the highest-priority association, or None if no apps are
/// registered for this MIME type.
pub fn default_app(mime: &str) -> Option<Association> {
    let inner = ASSOC.lock();
    inner
        .by_mime
        .get(mime)
        .and_then(|list| list.first().cloned())
}

/// Get all applications registered for a MIME type.
///
/// Returns the list sorted by priority (highest first).
pub fn apps_for(mime: &str) -> Vec<Association> {
    let inner = ASSOC.lock();
    inner.by_mime.get(mime).cloned().unwrap_or_default()
}

/// Get the default application for a file path.
///
/// Detects the MIME type first (via magic bytes / extension), then
/// looks up the association.
pub fn default_app_for_file(path: &str) -> Option<Association> {
    // Check extension override first.
    if let Some(ext) = path_extension(path) {
        let inner = ASSOC.lock();
        if let Some(mime) = inner.ext_override.get(ext) {
            let result = inner.by_mime.get(mime.as_str()).and_then(|l| l.first().cloned());
            if result.is_some() {
                return result;
            }
        }
        drop(inner);
    }

    // Fall back to MIME detection.
    let mime = super::mime::detect(path).ok()?;
    default_app(mime)
}

/// Set an extension → MIME type override.
///
/// Allows users to force `.xyz` files to be treated as a specific MIME
/// type regardless of what the detection would return.
#[allow(dead_code)]
pub fn set_extension_override(ext: &str, mime: &str) {
    ASSOC
        .lock()
        .ext_override
        .insert(String::from(ext), String::from(mime));
}

/// Remove an extension override.
#[allow(dead_code)]
pub fn remove_extension_override(ext: &str) {
    ASSOC.lock().ext_override.remove(ext);
}

/// List all registered MIME types and their association counts.
pub fn list_types() -> Vec<(String, usize)> {
    let inner = ASSOC.lock();
    inner
        .by_mime
        .iter()
        .map(|(mime, list)| (mime.clone(), list.len()))
        .collect()
}

/// Get registry statistics.
pub fn stats() -> AssociationStats {
    let inner = ASSOC.lock();
    let total: usize = inner.by_mime.values().map(|l| l.len()).sum();
    let user: usize = inner
        .by_mime
        .values()
        .flat_map(|l| l.iter())
        .filter(|a| a.user_set)
        .count();
    AssociationStats {
        mime_types: inner.by_mime.len(),
        total_entries: total,
        user_entries: user,
    }
}

/// Clear all associations.
#[allow(dead_code)]
pub fn clear() {
    let mut inner = ASSOC.lock();
    inner.by_mime.clear();
    inner.ext_override.clear();
}

/// Register common system default associations.
///
/// Called during boot to populate sensible defaults.  These all use
/// priority 10 (low) so user settings (priority 100+) take precedence.
pub fn register_defaults() {
    // Text editor for text types.
    let text_types = &[
        "text/plain",
        "text/x-rust",
        "text/x-c",
        "text/x-c++",
        "text/x-python",
        "text/javascript",
        "text/typescript",
        "application/json",
        "application/toml",
        "application/x-yaml",
        "application/xml",
        "text/html",
        "text/css",
        "text/markdown",
        "text/x-shellscript",
        "text/csv",
        "application/sql",
    ];
    for &mime in text_types {
        register(mime, "/usr/bin/edit", "Text Editor", 10, false);
    }

    // Image viewer.
    let image_types = &[
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/bmp",
        "image/webp",
        "image/tiff",
        "image/svg+xml",
    ];
    for &mime in image_types {
        register(mime, "/usr/bin/viewer", "Image Viewer", 10, false);
    }

    // Media player.
    let media_types = &[
        "audio/mpeg",
        "audio/wav",
        "audio/ogg",
        "audio/flac",
        "audio/mp4",
        "audio/opus",
        "video/mp4",
        "video/x-msvideo",
        "video/x-matroska",
        "video/webm",
        "video/quicktime",
    ];
    for &mime in media_types {
        register(mime, "/usr/bin/player", "Media Player", 10, false);
    }

    // Archive manager.
    let archive_types = &[
        "application/zip",
        "application/gzip",
        "application/x-bzip2",
        "application/x-xz",
        "application/zstd",
        "application/x-7z-compressed",
        "application/x-rar-compressed",
        "application/x-tar",
    ];
    for &mime in archive_types {
        register(mime, "/usr/bin/archiver", "Archive Manager", 10, false);
    }

    // PDF viewer.
    register(
        "application/pdf",
        "/usr/bin/pdfview",
        "PDF Viewer",
        10,
        false,
    );

    // Web browser for HTML.
    register("text/html", "/usr/bin/browser", "Web Browser", 5, false);

    // OS-native executable formats.
    register(
        "application/x-nx-executable",
        "/usr/bin/run",
        "Program Launcher",
        10,
        false,
    );
    register(
        "application/x-nx-sharedlib",
        "/usr/bin/ldd",
        "Library Inspector",
        10,
        false,
    );
    register(
        "application/x-nx-staticlib",
        "/usr/bin/ar",
        "Archive Tool",
        10,
        false,
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the file extension from a path (lowercase, without dot).
fn path_extension(path: &str) -> Option<&str> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let dot_pos = filename.rfind('.')?;
    if dot_pos == 0 {
        return None;
    }
    Some(&filename[dot_pos.saturating_add(1)..])
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the file association registry.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[assoc] Running self-test...");

    // Save existing state.
    let saved_stats = stats();

    // --- Test 1: register and lookup ---
    {
        register(
            "test/selftest",
            "/usr/bin/test_app",
            "Test App",
            50,
            false,
        );

        let app = default_app("test/selftest");
        if app.is_none() {
            serial_println!("[assoc]   ERROR: registered app not found");
            return Err(KernelError::InternalError);
        }
        let app = app.unwrap();
        if app.app_path != "/usr/bin/test_app" {
            serial_println!("[assoc]   ERROR: wrong app_path");
            return Err(KernelError::InternalError);
        }

        serial_println!("[assoc]   register + lookup OK");
    }

    // --- Test 2: priority ordering ---
    {
        register(
            "test/selftest",
            "/usr/bin/test_low",
            "Low Priority",
            10,
            false,
        );
        register(
            "test/selftest",
            "/usr/bin/test_high",
            "High Priority",
            100,
            true,
        );

        let app = default_app("test/selftest").unwrap();
        if app.app_path != "/usr/bin/test_high" {
            serial_println!("[assoc]   ERROR: highest priority not returned as default");
            return Err(KernelError::InternalError);
        }

        let all = apps_for("test/selftest");
        if all.len() != 3 {
            serial_println!(
                "[assoc]   ERROR: expected 3 apps, got {}",
                all.len()
            );
            return Err(KernelError::InternalError);
        }

        serial_println!("[assoc]   priority ordering OK");
    }

    // --- Test 3: unregister ---
    {
        let removed = unregister("test/selftest", "/usr/bin/test_high");
        if !removed {
            serial_println!("[assoc]   ERROR: unregister returned false");
            return Err(KernelError::InternalError);
        }

        let app = default_app("test/selftest").unwrap();
        if app.app_path != "/usr/bin/test_app" {
            serial_println!("[assoc]   ERROR: wrong default after unregister");
            return Err(KernelError::InternalError);
        }

        // Non-existent unregister.
        let removed2 = unregister("test/selftest", "/nonexistent");
        if removed2 {
            serial_println!("[assoc]   ERROR: unregister of nonexistent returned true");
            return Err(KernelError::InternalError);
        }

        serial_println!("[assoc]   unregister OK");
    }

    // --- Test 4: stats ---
    {
        let st = stats();
        if st.total_entries < 2 {
            serial_println!("[assoc]   ERROR: expected >= 2 entries, got {}", st.total_entries);
            return Err(KernelError::InternalError);
        }

        serial_println!("[assoc]   stats OK (types: {}, entries: {})", st.mime_types, st.total_entries);
    }

    // --- Test 5: list_types ---
    {
        let types = list_types();
        let has_test = types.iter().any(|(m, _)| m == "test/selftest");
        if !has_test {
            serial_println!("[assoc]   ERROR: test/selftest not in list_types");
            return Err(KernelError::InternalError);
        }

        serial_println!("[assoc]   list_types OK");
    }

    // Cleanup test data.
    unregister("test/selftest", "/usr/bin/test_app");
    unregister("test/selftest", "/usr/bin/test_low");

    // Verify cleanup.
    if default_app("test/selftest").is_some() {
        serial_println!("[assoc]   ERROR: test entries not fully cleaned up");
        return Err(KernelError::InternalError);
    }

    serial_println!("[assoc] Self-test passed (5 tests).");
    let _ = saved_stats;
    Ok(())
}
