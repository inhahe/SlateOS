//! Default applications — preferred app selection per content type.
//!
//! Manages the mapping of content types (MIME types, URL schemes,
//! file extensions) to preferred applications.  Users can set
//! per-type or per-category defaults.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Default Applications
//!   → defaultapps::set_default() / set_category_default()
//!
//! File open flow
//!   1. Resolve MIME type via fs::mime
//!   2. Check user override via defaultapps::default_for_type()
//!   3. Fall back to appregistry / associations
//!
//! Integration:
//!   → mime (MIME type detection)
//!   → associations (fallback file associations)
//!   → appregistry (app metadata)
//!   → openwith (open-with dialog)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_DEFAULTS: usize = 256;
const MAX_CATEGORY_DEFAULTS: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Application category for high-level default selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    /// Web browser.
    WebBrowser,
    /// Email client.
    EmailClient,
    /// File manager.
    FileManager,
    /// Text editor.
    TextEditor,
    /// Terminal emulator.
    Terminal,
    /// Image viewer.
    ImageViewer,
    /// Video player.
    VideoPlayer,
    /// Music player.
    MusicPlayer,
    /// PDF viewer.
    PdfViewer,
    /// Archive manager.
    ArchiveManager,
    /// Calculator.
    Calculator,
    /// Calendar.
    Calendar,
    /// Map / navigation.
    Maps,
    /// System monitor.
    SystemMonitor,
}

impl AppCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::WebBrowser => "Web Browser",
            Self::EmailClient => "Email Client",
            Self::FileManager => "File Manager",
            Self::TextEditor => "Text Editor",
            Self::Terminal => "Terminal",
            Self::ImageViewer => "Image Viewer",
            Self::VideoPlayer => "Video Player",
            Self::MusicPlayer => "Music Player",
            Self::PdfViewer => "PDF Viewer",
            Self::ArchiveManager => "Archive Manager",
            Self::Calculator => "Calculator",
            Self::Calendar => "Calendar",
            Self::Maps => "Maps",
            Self::SystemMonitor => "System Monitor",
        }
    }

    /// MIME types handled by this category.
    pub fn mime_types(self) -> &'static [&'static str] {
        match self {
            Self::WebBrowser => &["text/html", "application/xhtml+xml", "x-scheme-handler/http", "x-scheme-handler/https"],
            Self::EmailClient => &["x-scheme-handler/mailto", "message/rfc822"],
            Self::FileManager => &["inode/directory"],
            Self::TextEditor => &["text/plain", "text/x-csrc", "text/x-python", "application/json"],
            Self::Terminal => &["x-scheme-handler/terminal"],
            Self::ImageViewer => &["image/png", "image/jpeg", "image/gif", "image/bmp", "image/svg+xml", "image/webp"],
            Self::VideoPlayer => &["video/mp4", "video/x-matroska", "video/webm", "video/avi"],
            Self::MusicPlayer => &["audio/mpeg", "audio/ogg", "audio/flac", "audio/wav", "audio/aac"],
            Self::PdfViewer => &["application/pdf"],
            Self::ArchiveManager => &["application/zip", "application/x-tar", "application/gzip", "application/x-7z-compressed"],
            Self::Calculator => &[],
            Self::Calendar => &["text/calendar"],
            Self::Maps => &["x-scheme-handler/geo"],
            Self::SystemMonitor => &[],
        }
    }
}

/// A default app mapping.
#[derive(Debug, Clone)]
pub struct DefaultMapping {
    /// MIME type or URL scheme.
    pub content_type: String,
    /// Application ID.
    pub app_id: String,
    /// Whether this is a user override (vs system default).
    pub user_override: bool,
    /// User ID (0 = system-wide).
    pub uid: u32,
}

/// A category default.
#[derive(Debug, Clone)]
pub struct CategoryDefault {
    pub category: AppCategory,
    pub app_id: String,
    pub uid: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct DefaultAppsState {
    /// Per-type defaults.
    defaults: Vec<DefaultMapping>,
    /// Per-category defaults.
    category_defaults: Vec<CategoryDefault>,
    ops: u64,
}

static STATE: Mutex<Option<DefaultAppsState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut DefaultAppsState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the default apps subsystem with built-in defaults.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut defaults = Vec::new();

    // System-wide defaults for common types.
    let system_defaults: &[(&str, &str)] = &[
        ("text/plain", "textedit"),
        ("text/html", "webbrowser"),
        ("application/pdf", "pdfviewer"),
        ("image/png", "imageviewer"),
        ("image/jpeg", "imageviewer"),
        ("image/gif", "imageviewer"),
        ("image/svg+xml", "imageviewer"),
        ("video/mp4", "videoplayer"),
        ("video/x-matroska", "videoplayer"),
        ("audio/mpeg", "musicplayer"),
        ("audio/ogg", "musicplayer"),
        ("audio/flac", "musicplayer"),
        ("application/zip", "archivemgr"),
        ("application/x-tar", "archivemgr"),
        ("application/gzip", "archivemgr"),
        ("inode/directory", "filemanager"),
        ("x-scheme-handler/http", "webbrowser"),
        ("x-scheme-handler/https", "webbrowser"),
        ("x-scheme-handler/mailto", "emailclient"),
    ];

    for (mime, app) in system_defaults {
        defaults.push(DefaultMapping {
            content_type: String::from(*mime),
            app_id: String::from(*app),
            user_override: false,
            uid: 0,
        });
    }

    let category_defaults = alloc::vec![
        CategoryDefault { category: AppCategory::WebBrowser, app_id: String::from("webbrowser"), uid: 0 },
        CategoryDefault { category: AppCategory::EmailClient, app_id: String::from("emailclient"), uid: 0 },
        CategoryDefault { category: AppCategory::FileManager, app_id: String::from("filemanager"), uid: 0 },
        CategoryDefault { category: AppCategory::TextEditor, app_id: String::from("textedit"), uid: 0 },
        CategoryDefault { category: AppCategory::Terminal, app_id: String::from("terminal"), uid: 0 },
        CategoryDefault { category: AppCategory::ImageViewer, app_id: String::from("imageviewer"), uid: 0 },
        CategoryDefault { category: AppCategory::VideoPlayer, app_id: String::from("videoplayer"), uid: 0 },
        CategoryDefault { category: AppCategory::MusicPlayer, app_id: String::from("musicplayer"), uid: 0 },
        CategoryDefault { category: AppCategory::PdfViewer, app_id: String::from("pdfviewer"), uid: 0 },
        CategoryDefault { category: AppCategory::ArchiveManager, app_id: String::from("archivemgr"), uid: 0 },
    ];

    *guard = Some(DefaultAppsState {
        defaults,
        category_defaults,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Per-type defaults
// ---------------------------------------------------------------------------

/// Get the default app for a content type.
///
/// Checks user overrides first (for the given UID), then system defaults.
pub fn default_for_type(content_type: &str, uid: u32) -> Option<String> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;

    // User override first.
    if uid != 0 {
        if let Some(m) = state.defaults.iter()
            .find(|d| d.content_type == content_type && d.uid == uid)
        {
            return Some(m.app_id.clone());
        }
    }

    // System default.
    state.defaults.iter()
        .find(|d| d.content_type == content_type && d.uid == 0)
        .map(|d| d.app_id.clone())
}

/// Set the default app for a content type.
pub fn set_default(content_type: &str, app_id: &str, uid: u32) -> KernelResult<()> {
    if content_type.is_empty() || app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        // Update existing mapping.
        if let Some(existing) = state.defaults.iter_mut()
            .find(|d| d.content_type == content_type && d.uid == uid)
        {
            existing.app_id = String::from(app_id);
            existing.user_override = uid != 0;
            return Ok(());
        }
        // Add new mapping.
        if state.defaults.len() >= MAX_DEFAULTS {
            return Err(KernelError::ResourceExhausted);
        }
        state.defaults.push(DefaultMapping {
            content_type: String::from(content_type),
            app_id: String::from(app_id),
            user_override: uid != 0,
            uid,
        });
        Ok(())
    })
}

/// Remove a default mapping.
pub fn remove_default(content_type: &str, uid: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.defaults.iter()
            .position(|d| d.content_type == content_type && d.uid == uid)
        {
            state.defaults.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all defaults for a user (includes system defaults).
pub fn list_defaults(uid: u32) -> Vec<DefaultMapping> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.defaults.iter()
            .filter(|d| d.uid == uid || d.uid == 0)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Category defaults
// ---------------------------------------------------------------------------

/// Get the default app for a category.
pub fn category_default(category: AppCategory, uid: u32) -> Option<String> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;

    // User override first.
    if uid != 0 {
        if let Some(cd) = state.category_defaults.iter()
            .find(|c| c.category == category && c.uid == uid)
        {
            return Some(cd.app_id.clone());
        }
    }

    state.category_defaults.iter()
        .find(|c| c.category == category && c.uid == 0)
        .map(|c| c.app_id.clone())
}

/// Set the default app for a category.
///
/// Also updates all MIME types associated with that category.
pub fn set_category_default(category: AppCategory, app_id: &str, uid: u32) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        // Update category default.
        if let Some(existing) = state.category_defaults.iter_mut()
            .find(|c| c.category == category && c.uid == uid)
        {
            existing.app_id = String::from(app_id);
        } else {
            if state.category_defaults.len() >= MAX_CATEGORY_DEFAULTS {
                return Err(KernelError::ResourceExhausted);
            }
            state.category_defaults.push(CategoryDefault {
                category,
                app_id: String::from(app_id),
                uid,
            });
        }

        // Also update associated MIME type mappings.
        for mime in category.mime_types() {
            let mime_str = *mime;
            if let Some(existing) = state.defaults.iter_mut()
                .find(|d| d.content_type == mime_str && d.uid == uid)
            {
                existing.app_id = String::from(app_id);
            } else if state.defaults.len() < MAX_DEFAULTS {
                state.defaults.push(DefaultMapping {
                    content_type: String::from(mime_str),
                    app_id: String::from(app_id),
                    user_override: uid != 0,
                    uid,
                });
            }
        }

        Ok(())
    })
}

/// List all category defaults.
pub fn list_category_defaults(uid: u32) -> Vec<CategoryDefault> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.category_defaults.iter()
            .filter(|c| c.uid == uid || c.uid == 0)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Find which category a MIME type belongs to.
pub fn category_for_type(content_type: &str) -> Option<AppCategory> {
    let categories = [
        AppCategory::WebBrowser, AppCategory::EmailClient, AppCategory::FileManager,
        AppCategory::TextEditor, AppCategory::Terminal, AppCategory::ImageViewer,
        AppCategory::VideoPlayer, AppCategory::MusicPlayer, AppCategory::PdfViewer,
        AppCategory::ArchiveManager, AppCategory::Calculator, AppCategory::Calendar,
        AppCategory::Maps, AppCategory::SystemMonitor,
    ];
    for cat in &categories {
        if cat.mime_types().contains(&content_type) {
            return Some(*cat);
        }
    }
    None
}

/// Search defaults by app_id or content_type.
pub fn search(query: &str) -> Vec<DefaultMapping> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        let q = query;
        s.defaults.iter()
            .filter(|d| d.content_type.contains(q) || d.app_id.contains(q))
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (type_count, category_count, user_overrides, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let overrides = s.defaults.iter().filter(|d| d.user_override).count();
            (s.defaults.len(), s.category_defaults.len(), overrides, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the default apps module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[defaultapps] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial defaults.
    {
        let defaults = list_defaults(0);
        assert!(!defaults.is_empty());
        let (tc, cc, _, _) = stats();
        assert!(tc > 0);
        assert!(cc > 0);
    }
    serial_println!("[defaultapps]  1/11 initial defaults OK");

    // Test 2: query type default.
    {
        let app = default_for_type("text/plain", 0);
        assert_eq!(app.as_deref(), Some("textedit"));
        let app = default_for_type("text/html", 0);
        assert_eq!(app.as_deref(), Some("webbrowser"));
    }
    serial_println!("[defaultapps]  2/11 query type OK");

    // Test 3: set user override.
    {
        set_default("text/plain", "vim", 1000).unwrap();
        let app = default_for_type("text/plain", 1000);
        assert_eq!(app.as_deref(), Some("vim"));
        // System default unchanged.
        let app = default_for_type("text/plain", 0);
        assert_eq!(app.as_deref(), Some("textedit"));
    }
    serial_println!("[defaultapps]  3/11 user override OK");

    // Test 4: remove override.
    {
        remove_default("text/plain", 1000).unwrap();
        // Falls back to system default.
        let app = default_for_type("text/plain", 1000);
        assert_eq!(app.as_deref(), Some("textedit"));
    }
    serial_println!("[defaultapps]  4/11 remove override OK");

    // Test 5: category default.
    {
        let app = category_default(AppCategory::WebBrowser, 0);
        assert_eq!(app.as_deref(), Some("webbrowser"));
    }
    serial_println!("[defaultapps]  5/11 category default OK");

    // Test 6: set category default.
    {
        set_category_default(AppCategory::WebBrowser, "firefox", 0).unwrap();
        let app = category_default(AppCategory::WebBrowser, 0);
        assert_eq!(app.as_deref(), Some("firefox"));
        // MIME types should also be updated.
        let app = default_for_type("text/html", 0);
        assert_eq!(app.as_deref(), Some("firefox"));
    }
    serial_println!("[defaultapps]  6/11 set category OK");

    // Test 7: category for type.
    {
        assert_eq!(category_for_type("text/html"), Some(AppCategory::WebBrowser));
        assert_eq!(category_for_type("image/png"), Some(AppCategory::ImageViewer));
        assert_eq!(category_for_type("application/pdf"), Some(AppCategory::PdfViewer));
        assert_eq!(category_for_type("unknown/type"), None);
    }
    serial_println!("[defaultapps]  7/11 category for type OK");

    // Test 8: list category defaults.
    {
        let cats = list_category_defaults(0);
        assert!(!cats.is_empty());
    }
    serial_println!("[defaultapps]  8/11 list categories OK");

    // Test 9: search.
    {
        let results = search("image");
        assert!(!results.is_empty());
        let results = search("firefox");
        assert!(!results.is_empty());
    }
    serial_println!("[defaultapps]  9/11 search OK");

    // Test 10: validation.
    {
        assert!(set_default("", "app", 0).is_err());
        assert!(set_default("text/plain", "", 0).is_err());
        assert!(set_category_default(AppCategory::WebBrowser, "", 0).is_err());
    }
    serial_println!("[defaultapps] 10/11 validation OK");

    // Test 11: stats.
    {
        let (tc, cc, _, ops) = stats();
        assert!(tc > 0);
        assert!(cc > 0);
        assert!(ops > 0);
    }
    serial_println!("[defaultapps] 11/11 stats OK");

    serial_println!("[defaultapps] All self-tests passed.");
}
