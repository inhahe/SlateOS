//! Application registry — central database of installed applications.
//!
//! Tracks all installed applications with their metadata: name, path,
//! icon, category, capabilities, MIME types handled, etc.  Used by:
//! - Start menu (application tree organized by category)
//! - Run dialog (PATH + alias lookup)
//! - File associations (which apps handle which types)
//! - Open With dialog (list of installed apps)
//! - System tray (which apps want tray icons)
//!
//! ## Design Reference
//!
//! design.txt line 721: "start menu, contains applications tree,
//! settings icon, terminal, power off, logout, reboot, ..."
//!
//! design.txt line 303: "user can ... select from any installed program
//! that's registered to be able to load that file extension, or select
//! from any installed program"
//!
//! ## Architecture
//!
//! ```text
//! Package manager installs app
//!   → appregistry::register(AppInfo { ... })
//!   → stored in REGISTRY
//!
//! Start menu builds tree
//!   → appregistry::by_category(AppCategory::Office)
//!   → list of apps in that category
//!
//! File association
//!   → appregistry::handlers_for_mime("image/png")
//!   → all apps that handle image/png
//!
//! System search
//!   → appregistry::search("calc")
//!   → matching apps by name, description, or keywords
//! ```
//!
//! ## Categories
//!
//! Applications are organized into categories for the start menu tree.
//! An app can belong to multiple categories.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered applications.
const MAX_APPS: usize = 4096;

/// Maximum MIME types per app.
const MAX_MIME_PER_APP: usize = 64;

/// Maximum keywords per app.
const MAX_KEYWORDS: usize = 32;

/// Maximum categories per app.
const MAX_CATEGORIES: usize = 8;

/// Maximum search results.
const MAX_SEARCH_RESULTS: usize = 100;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Application category for start menu organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AppCategory {
    /// System utilities and settings.
    System,
    /// Office and productivity.
    Office,
    /// Graphics, image editing, photo viewers.
    Graphics,
    /// Audio and video players/editors.
    Multimedia,
    /// Web browsers, email, chat.
    Internet,
    /// Games.
    Games,
    /// Software development tools.
    Development,
    /// Education and reference.
    Education,
    /// Science and math.
    Science,
    /// Accessories and small utilities.
    Accessories,
    /// Terminal emulators and shells.
    Terminal,
    /// File managers.
    FileManager,
    /// Settings and configuration.
    Settings,
    /// Other / uncategorized.
    Other,
}

impl AppCategory {
    /// Display label for the category.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Office => "Office",
            Self::Graphics => "Graphics",
            Self::Multimedia => "Multimedia",
            Self::Internet => "Internet",
            Self::Games => "Games",
            Self::Development => "Development",
            Self::Education => "Education",
            Self::Science => "Science",
            Self::Accessories => "Accessories",
            Self::Terminal => "Terminal",
            Self::FileManager => "File Manager",
            Self::Settings => "Settings",
            Self::Other => "Other",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "office" => Some(Self::Office),
            "graphics" => Some(Self::Graphics),
            "multimedia" | "media" => Some(Self::Multimedia),
            "internet" | "web" => Some(Self::Internet),
            "games" => Some(Self::Games),
            "development" | "dev" => Some(Self::Development),
            "education" | "edu" => Some(Self::Education),
            "science" => Some(Self::Science),
            "accessories" | "acc" => Some(Self::Accessories),
            "terminal" | "term" => Some(Self::Terminal),
            "filemanager" | "fm" => Some(Self::FileManager),
            "settings" => Some(Self::Settings),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    /// All categories in display order.
    pub fn all() -> &'static [AppCategory] {
        &[
            Self::Accessories, Self::Development, Self::Education,
            Self::FileManager, Self::Games, Self::Graphics,
            Self::Internet, Self::Multimedia, Self::Office,
            Self::Science, Self::Settings, Self::System,
            Self::Terminal, Self::Other,
        ]
    }
}

/// Information about an installed application.
#[derive(Debug, Clone)]
pub struct AppInfo {
    /// Unique application ID (e.g., "org.example.calculator").
    pub id: String,
    /// Display name (e.g., "Calculator").
    pub name: String,
    /// Short description.
    pub description: String,
    /// Path to the executable.
    pub exec_path: String,
    /// Icon identifier.
    pub icon: String,
    /// Categories this app belongs to.
    pub categories: Vec<AppCategory>,
    /// MIME types this app can handle.
    pub mime_types: Vec<String>,
    /// Search keywords.
    pub keywords: Vec<String>,
    /// Whether to show in start menu.
    pub show_in_menu: bool,
    /// Whether app wants a system tray icon.
    pub tray_icon: bool,
    /// Whether app starts hidden (in tray).
    pub start_hidden: bool,
    /// Version string.
    pub version: String,
    /// Timestamp of installation (nanoseconds).
    pub installed_ns: u64,
}

/// A start menu entry (simplified view of AppInfo for the GUI).
#[derive(Debug, Clone)]
pub struct MenuEntry {
    /// Application ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Icon.
    pub icon: String,
    /// Executable path.
    pub exec_path: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct Registry {
    /// App ID → AppInfo.
    apps: BTreeMap<String, AppInfo>,
    /// MIME type → set of app IDs that handle it.
    mime_index: BTreeMap<String, BTreeSet<String>>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            apps: BTreeMap::new(),
            mime_index: BTreeMap::new(),
        }
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
static REGISTER_COUNT: AtomicU64 = AtomicU64::new(0);
static LOOKUP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Register an application.
pub fn register(info: AppInfo) -> KernelResult<()> {
    if info.id.is_empty() || info.name.is_empty() || info.exec_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if info.categories.len() > MAX_CATEGORIES {
        return Err(KernelError::InvalidArgument);
    }
    if info.mime_types.len() > MAX_MIME_PER_APP {
        return Err(KernelError::InvalidArgument);
    }
    if info.keywords.len() > MAX_KEYWORDS {
        return Err(KernelError::InvalidArgument);
    }
    REGISTER_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut reg = REGISTRY.lock();
    if !reg.apps.contains_key(&info.id) && reg.apps.len() >= MAX_APPS {
        return Err(KernelError::ResourceExhausted);
    }

    // Remove old MIME index entries if updating.
    // Collect old MIME types into a local vec to avoid borrow conflict.
    let old_mimes: Vec<String> = reg.apps.get(&info.id)
        .map(|old| old.mime_types.clone())
        .unwrap_or_default();
    for mime in &old_mimes {
        if let Some(set) = reg.mime_index.get_mut(mime) {
            set.remove(&info.id);
        }
    }

    // Add MIME index entries.
    for mime in &info.mime_types {
        reg.mime_index.entry(mime.clone())
            .or_default()
            .insert(info.id.clone());
    }

    reg.apps.insert(info.id.clone(), info);
    Ok(())
}

/// Unregister an application.
pub fn unregister(id: &str) -> KernelResult<()> {
    let mut reg = REGISTRY.lock();
    let app = reg.apps.remove(id).ok_or(KernelError::NotFound)?;

    // Clean up MIME index.
    for mime in &app.mime_types {
        if let Some(set) = reg.mime_index.get_mut(mime) {
            set.remove(id);
            if set.is_empty() {
                reg.mime_index.remove(mime);
            }
        }
    }
    Ok(())
}

/// Get application info by ID.
pub fn get(id: &str) -> Option<AppInfo> {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);
    let reg = REGISTRY.lock();
    reg.apps.get(id).cloned()
}

/// List all registered applications.
pub fn list_all() -> Vec<AppInfo> {
    let reg = REGISTRY.lock();
    reg.apps.values().cloned().collect()
}

/// Get apps in a specific category.
pub fn by_category(category: AppCategory) -> Vec<AppInfo> {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);
    let reg = REGISTRY.lock();
    reg.apps.values()
        .filter(|a| a.categories.contains(&category))
        .cloned()
        .collect()
}

/// Get apps that handle a specific MIME type.
pub fn handlers_for_mime(mime: &str) -> Vec<AppInfo> {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);
    let reg = REGISTRY.lock();
    if let Some(ids) = reg.mime_index.get(mime) {
        ids.iter()
            .filter_map(|id| reg.apps.get(id).cloned())
            .collect()
    } else {
        Vec::new()
    }
}

/// Build the start menu tree (apps organized by category).
pub fn menu_tree() -> Vec<(AppCategory, Vec<MenuEntry>)> {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);
    let reg = REGISTRY.lock();

    let mut tree: BTreeMap<AppCategory, Vec<MenuEntry>> = BTreeMap::new();
    for app in reg.apps.values() {
        if !app.show_in_menu {
            continue;
        }
        let entry = MenuEntry {
            id: app.id.clone(),
            name: app.name.clone(),
            icon: app.icon.clone(),
            exec_path: app.exec_path.clone(),
        };
        for &cat in &app.categories {
            tree.entry(cat).or_default().push(entry.clone());
        }
        // If no categories, put in Other.
        if app.categories.is_empty() {
            tree.entry(AppCategory::Other).or_default().push(entry);
        }
    }

    // Sort entries within each category by name.
    for entries in tree.values_mut() {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
    }

    tree.into_iter().collect()
}

/// Search for apps by name, description, or keywords.
pub fn search(query: &str) -> Vec<AppInfo> {
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);
    if query.is_empty() {
        return Vec::new();
    }

    let q = to_lower(query);
    let reg = REGISTRY.lock();
    let mut results = Vec::new();

    for app in reg.apps.values() {
        let name_match = to_lower(&app.name).contains(&q);
        let desc_match = to_lower(&app.description).contains(&q);
        let id_match = to_lower(&app.id).contains(&q);
        let kw_match = app.keywords.iter().any(|k| to_lower(k).contains(&q));

        if name_match || desc_match || id_match || kw_match {
            results.push(app.clone());
            if results.len() >= MAX_SEARCH_RESULTS {
                break;
            }
        }
    }

    // Sort: name matches first, then description, then others.
    results.sort_by(|a, b| {
        let a_name = to_lower(&a.name).contains(&q);
        let b_name = to_lower(&b.name).contains(&q);
        b_name.cmp(&a_name)
    });

    results
}

/// Get apps that want tray icons.
pub fn tray_apps() -> Vec<AppInfo> {
    let reg = REGISTRY.lock();
    reg.apps.values()
        .filter(|a| a.tray_icon)
        .cloned()
        .collect()
}

/// Count registered applications.
pub fn app_count() -> usize {
    let reg = REGISTRY.lock();
    reg.apps.len()
}

// ---------------------------------------------------------------------------
// Built-in apps
// ---------------------------------------------------------------------------

/// Register built-in system applications.
pub fn register_builtins() -> KernelResult<()> {
    let now = crate::timekeeping::clock_monotonic();

    let builtins = [
        ("org.os.files", "File Manager", "Browse and manage files",
         "/usr/bin/file-manager", "icon-files",
         &[AppCategory::FileManager, AppCategory::System][..],
         &["application/x-directory"][..],
         &["explorer", "finder", "nautilus"][..]),
        ("org.os.terminal", "Terminal", "Command-line terminal emulator",
         "/usr/bin/terminal", "icon-terminal",
         &[AppCategory::Terminal, AppCategory::System][..],
         &[][..],
         &["shell", "console", "command"][..]),
        ("org.os.editor", "Text Editor", "Edit text files",
         "/usr/bin/text-editor", "icon-editor",
         &[AppCategory::Accessories][..],
         &["text/plain", "text/html", "text/css", "application/json"][..],
         &["notepad", "edit", "vim", "nano"][..]),
        ("org.os.settings", "Settings", "System configuration",
         "/usr/bin/settings", "icon-settings",
         &[AppCategory::Settings, AppCategory::System][..],
         &[][..],
         &["preferences", "config", "control"][..]),
        ("org.os.calculator", "Calculator", "Perform calculations",
         "/usr/bin/calculator", "icon-calculator",
         &[AppCategory::Accessories][..],
         &[][..],
         &["calc", "math"][..]),
        ("org.os.sysinfo", "System Information", "View hardware and OS details",
         "/usr/bin/system-info", "icon-sysinfo",
         &[AppCategory::System][..],
         &[][..],
         &["about", "hardware", "specs"][..]),
        ("org.os.procexp", "Process Explorer", "Monitor running processes",
         "/usr/bin/process-explorer", "icon-procexp",
         &[AppCategory::System][..],
         &[][..],
         &["task", "manager", "top", "htop"][..]),
        ("org.os.viewer", "Image Viewer", "View images and photos",
         "/usr/bin/image-viewer", "icon-viewer",
         &[AppCategory::Graphics][..],
         &["image/png", "image/jpeg", "image/gif", "image/bmp", "image/svg+xml"][..],
         &["photo", "picture", "gallery"][..]),
        ("org.os.player", "Media Player", "Play audio and video",
         "/usr/bin/media-player", "icon-player",
         &[AppCategory::Multimedia][..],
         &["audio/mpeg", "audio/wav", "video/mp4", "audio/ogg"][..],
         &["music", "video", "vlc", "mpv"][..]),
    ];

    for (id, name, desc, path, icon, cats, mimes, kws) in &builtins {
        register(AppInfo {
            id: String::from(*id),
            name: String::from(*name),
            description: String::from(*desc),
            exec_path: String::from(*path),
            icon: String::from(*icon),
            categories: cats.to_vec(),
            mime_types: mimes.iter().map(|m| String::from(*m)).collect(),
            keywords: kws.iter().map(|k| String::from(*k)).collect(),
            show_in_menu: true,
            tray_icon: false,
            start_hidden: false,
            version: String::from("1.0"),
            installed_ns: now,
        })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (app_count, mime_types, register_ops, lookup_ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let reg = REGISTRY.lock();
    let mime_count = reg.mime_index.len();
    (
        reg.apps.len(),
        mime_count,
        REGISTER_COUNT.load(Ordering::Relaxed),
        LOOKUP_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    REGISTER_COUNT.store(0, Ordering::Relaxed);
    LOOKUP_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut reg = REGISTRY.lock();
    reg.apps.clear();
    reg.mime_index.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the application registry.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: register and get.
    {
        register(AppInfo {
            id: String::from("test.app"),
            name: String::from("Test App"),
            description: String::from("A test application"),
            exec_path: String::from("/usr/bin/test-app"),
            icon: String::from("icon-test"),
            categories: alloc::vec![AppCategory::Accessories],
            mime_types: alloc::vec![String::from("text/plain")],
            keywords: alloc::vec![String::from("testing")],
            show_in_menu: true,
            tray_icon: false,
            start_hidden: false,
            version: String::from("1.0"),
            installed_ns: 0,
        })?;
        let app = get("test.app").unwrap();
        assert_eq!(app.name, "Test App");
        serial_println!("[appregistry] test 1 passed: register/get");
    }

    // Test 2: by_category.
    {
        let acc = by_category(AppCategory::Accessories);
        assert_eq!(acc.len(), 1);
        assert_eq!(acc[0].id, "test.app");
        serial_println!("[appregistry] test 2 passed: by_category");
    }

    // Test 3: handlers_for_mime.
    {
        let handlers = handlers_for_mime("text/plain");
        assert_eq!(handlers.len(), 1);
        let empty = handlers_for_mime("image/png");
        assert!(empty.is_empty());
        serial_println!("[appregistry] test 3 passed: handlers_for_mime");
    }

    // Test 4: search.
    {
        let results = search("test");
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "test.app");

        let results = search("testing");
        assert!(!results.is_empty());
        serial_println!("[appregistry] test 4 passed: search");
    }

    // Test 5: menu tree.
    {
        let tree = menu_tree();
        assert!(!tree.is_empty());
        let acc_entry = tree.iter().find(|(cat, _)| *cat == AppCategory::Accessories);
        assert!(acc_entry.is_some());
        serial_println!("[appregistry] test 5 passed: menu_tree");
    }

    // Test 6: register builtins.
    {
        register_builtins()?;
        assert!(app_count() >= 9); // At least 9 built-in apps.
        let terminal = by_category(AppCategory::Terminal);
        assert!(!terminal.is_empty());
        serial_println!("[appregistry] test 6 passed: register_builtins");
    }

    // Test 7: unregister.
    {
        let count_before = app_count();
        unregister("test.app")?;
        assert_eq!(app_count(), count_before - 1);
        assert!(get("test.app").is_none());
        // MIME index should be cleaned up.
        let handlers = handlers_for_mime("text/plain");
        assert!(!handlers.iter().any(|a| a.id == "test.app"));
        serial_println!("[appregistry] test 7 passed: unregister");
    }

    clear_all();
    reset_stats();

    serial_println!("[appregistry] all 7 self-tests passed");
    Ok(())
}
