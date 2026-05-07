//! Filesystem path bookmarks and favorites.
//!
//! Provides named bookmarks for frequently accessed directories and files,
//! powering the file explorer's sidebar favorites panel, the shell's quick
//! navigation (`cd @work`), and application file dialogs.
//!
//! ## Architecture
//!
//! ```text
//! File explorer sidebar         Shell (cd @name)
//!         ↓                          ↓
//!   bookmarks::list()           bookmarks::resolve("name")
//!         ↓                          ↓
//!   sorted bookmark list        → full path string
//! ```
//!
//! ## Features
//!
//! - **Named bookmarks** — "work" → "/home/user/projects"
//! - **Ordered** — bookmarks have a display order for sidebar
//! - **Categories** — group bookmarks (places, recent, devices, network)
//! - **Icons** — optional icon name per bookmark
//! - **Validation** — check if bookmarked path still exists
//! - **System defaults** — Home, Desktop, Documents, Downloads, etc.
//!
//! ## Design Notes
//!
//! - Maximum 128 bookmarks.
//! - Names are case-insensitive for lookup but preserve original case.
//! - System bookmarks (Home, etc.) cannot be removed, only hidden.
//! - Thread-safe via spin::Mutex.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum bookmarks.
const MAX_BOOKMARKS: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Bookmark category for sidebar grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Standard places (Home, Desktop, Documents, etc.).
    Places,
    /// User-defined favorites.
    Favorites,
    /// Mounted devices (USB drives, network shares).
    Devices,
    /// Network locations.
    Network,
}

impl Category {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Places => "Places",
            Self::Favorites => "Favorites",
            Self::Devices => "Devices",
            Self::Network => "Network",
        }
    }

    /// Parse from string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "places" | "p" => Some(Self::Places),
            "favorites" | "fav" | "f" => Some(Self::Favorites),
            "devices" | "dev" | "d" => Some(Self::Devices),
            "network" | "net" | "n" => Some(Self::Network),
            _ => None,
        }
    }
}

/// A filesystem bookmark.
#[derive(Debug, Clone)]
pub struct Bookmark {
    /// Short name for reference ("work", "docs", "home").
    pub name: String,
    /// Full filesystem path.
    pub path: String,
    /// Display label (may differ from name: "My Documents").
    pub label: String,
    /// Category for sidebar grouping.
    pub category: Category,
    /// Optional icon identifier.
    pub icon: String,
    /// Display order within category (lower = higher).
    pub order: u32,
    /// Whether this is a system bookmark (cannot be removed).
    pub system: bool,
    /// Whether the bookmark is visible in the sidebar.
    pub visible: bool,
    /// Access count (how many times navigated to).
    pub access_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Bookmarks storage.
static BOOKMARKS: spin::Mutex<Vec<Bookmark>> = spin::Mutex::new(Vec::new());

/// Whether system defaults have been initialized.
static INITIALIZED: spin::Mutex<bool> = spin::Mutex::new(false);

/// Statistics.
static RESOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
static ADD_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — Initialization
// ---------------------------------------------------------------------------

/// Initialize system default bookmarks.
///
/// Called once at startup. Creates standard desktop directory bookmarks.
pub fn init() {
    let mut inited = INITIALIZED.lock();
    if *inited {
        return;
    }

    let mut bm = BOOKMARKS.lock();

    let defaults = [
        ("home", "/home/user", "Home", "folder-home", 0),
        ("desktop", "/home/user/Desktop", "Desktop", "user-desktop", 1),
        ("documents", "/home/user/Documents", "Documents", "folder-documents", 2),
        ("downloads", "/home/user/Downloads", "Downloads", "folder-download", 3),
        ("music", "/home/user/Music", "Music", "folder-music", 4),
        ("pictures", "/home/user/Pictures", "Pictures", "folder-pictures", 5),
        ("videos", "/home/user/Videos", "Videos", "folder-videos", 6),
        ("root", "/", "Filesystem", "drive-harddisk", 10),
    ];

    for (name, path, label, icon, order) in &defaults {
        bm.push(Bookmark {
            name: String::from(*name),
            path: String::from(*path),
            label: String::from(*label),
            category: Category::Places,
            icon: String::from(*icon),
            order: *order,
            system: true,
            visible: true,
            access_count: 0,
        });
    }

    *inited = true;
}

// ---------------------------------------------------------------------------
// Public API — Bookmark Management
// ---------------------------------------------------------------------------

/// Add a user bookmark.
pub fn add(name: &str, path: &str, label: &str, category: Category) -> KernelResult<()> {
    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
    init(); // Ensure system defaults exist.

    let mut bm = BOOKMARKS.lock();

    // Check name uniqueness (case-insensitive).
    let name_lower = name.to_lowercase();
    if bm.iter().any(|b| b.name.to_lowercase() == name_lower) {
        return Err(KernelError::AlreadyExists);
    }

    if bm.len() >= MAX_BOOKMARKS {
        return Err(KernelError::OutOfMemory);
    }

    // Find highest order in category for placement.
    let max_order = bm.iter()
        .filter(|b| b.category == category)
        .map(|b| b.order)
        .max()
        .unwrap_or(0);

    bm.push(Bookmark {
        name: String::from(name),
        path: String::from(path),
        label: if label.is_empty() { String::from(name) } else { String::from(label) },
        category,
        icon: String::new(),
        order: max_order + 1,
        system: false,
        visible: true,
        access_count: 0,
    });

    Ok(())
}

/// Remove a bookmark by name.
///
/// System bookmarks cannot be removed — only hidden.
pub fn remove(name: &str) -> KernelResult<()> {
    let mut bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();

    let pos = bm.iter().position(|b| b.name.to_lowercase() == name_lower);
    match pos {
        Some(i) => {
            if bm[i].system {
                return Err(KernelError::PermissionDenied);
            }
            bm.swap_remove(i);
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Resolve a bookmark name to its path.
///
/// Used by `cd @name` in the shell.
pub fn resolve(name: &str) -> Option<String> {
    RESOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    init();

    let bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();
    bm.iter()
        .find(|b| b.name.to_lowercase() == name_lower)
        .map(|b| b.path.clone())
}

/// Record an access to a bookmark (increments counter).
pub fn record_access(name: &str) {
    let mut bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();
    if let Some(b) = bm.iter_mut().find(|b| b.name.to_lowercase() == name_lower) {
        b.access_count = b.access_count.saturating_add(1);
    }
}

/// Rename a bookmark.
pub fn rename(old_name: &str, new_name: &str) -> KernelResult<()> {
    let mut bm = BOOKMARKS.lock();
    let old_lower = old_name.to_lowercase();
    let new_lower = new_name.to_lowercase();

    // Check new name uniqueness.
    if bm.iter().any(|b| b.name.to_lowercase() == new_lower) {
        return Err(KernelError::AlreadyExists);
    }

    let pos = bm.iter().position(|b| b.name.to_lowercase() == old_lower);
    match pos {
        Some(i) => {
            bm[i].name = String::from(new_name);
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Update a bookmark's path.
pub fn update_path(name: &str, new_path: &str) -> KernelResult<()> {
    let mut bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();

    let pos = bm.iter().position(|b| b.name.to_lowercase() == name_lower);
    match pos {
        Some(i) => {
            bm[i].path = String::from(new_path);
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Set a bookmark's icon.
pub fn set_icon(name: &str, icon: &str) -> KernelResult<()> {
    let mut bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();

    let pos = bm.iter().position(|b| b.name.to_lowercase() == name_lower);
    match pos {
        Some(i) => {
            bm[i].icon = String::from(icon);
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

/// Toggle bookmark visibility.
pub fn set_visible(name: &str, visible: bool) -> KernelResult<()> {
    let mut bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();

    let pos = bm.iter().position(|b| b.name.to_lowercase() == name_lower);
    match pos {
        Some(i) => {
            bm[i].visible = visible;
            Ok(())
        }
        None => Err(KernelError::NotFound),
    }
}

// ---------------------------------------------------------------------------
// Public API — Querying
// ---------------------------------------------------------------------------

/// List all bookmarks, sorted by category and order.
pub fn list() -> Vec<Bookmark> {
    init();
    let bm = BOOKMARKS.lock();
    let mut result: Vec<Bookmark> = bm.clone();
    result.sort_by(|a, b| {
        (a.category as u8).cmp(&(b.category as u8))
            .then(a.order.cmp(&b.order))
    });
    result
}

/// List visible bookmarks only.
pub fn list_visible() -> Vec<Bookmark> {
    list().into_iter().filter(|b| b.visible).collect()
}

/// List bookmarks in a specific category.
pub fn list_category(category: Category) -> Vec<Bookmark> {
    list().into_iter().filter(|b| b.category == category).collect()
}

/// Get a specific bookmark by name.
pub fn get(name: &str) -> Option<Bookmark> {
    init();
    let bm = BOOKMARKS.lock();
    let name_lower = name.to_lowercase();
    bm.iter()
        .find(|b| b.name.to_lowercase() == name_lower)
        .cloned()
}

/// Validate all bookmarks — check if paths still exist.
///
/// Returns list of (name, path, exists) tuples.
pub fn validate() -> Vec<(String, String, bool)> {
    init();
    let bm = BOOKMARKS.lock();
    bm.iter()
        .map(|b| {
            let exists = crate::fs::Vfs::metadata(&b.path).is_ok();
            (b.name.clone(), b.path.clone(), exists)
        })
        .collect()
}

/// Get bookmark count.
pub fn count() -> usize {
    init();
    BOOKMARKS.lock().len()
}

// ---------------------------------------------------------------------------
// Public API — Statistics
// ---------------------------------------------------------------------------

/// Get statistics.
pub fn stats() -> (u64, u64, usize) {
    let count = BOOKMARKS.lock().len();
    (
        RESOLVE_COUNT.load(Ordering::Relaxed),
        ADD_COUNT.load(Ordering::Relaxed),
        count,
    )
}

/// Reset statistics.
pub fn reset_stats() {
    RESOLVE_COUNT.store(0, Ordering::Relaxed);
    ADD_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[bookmarks] Running self-test...");

    test_init();
    test_add_and_resolve();
    test_remove();
    test_rename();
    test_categories();
    test_visibility();

    serial_println!("[bookmarks] Self-test passed (6 tests).");
    Ok(())
}

fn test_init() {
    // Reset for clean test.
    {
        BOOKMARKS.lock().clear();
        *INITIALIZED.lock() = false;
    }

    init();
    let bm = BOOKMARKS.lock();
    assert!(bm.len() >= 8); // At least 8 system defaults.
    assert!(bm.iter().any(|b| b.name == "home"));
    assert!(bm.iter().any(|b| b.name == "desktop"));
    assert!(bm.iter().all(|b| b.system)); // All are system bookmarks.

    serial_println!("[bookmarks]   init: ok");
}

fn test_add_and_resolve() {
    let result = add("work", "/home/user/work", "Work Projects", Category::Favorites);
    assert!(result.is_ok());

    let path = resolve("work");
    assert_eq!(path, Some(String::from("/home/user/work")));

    // Case-insensitive lookup.
    let path = resolve("WORK");
    assert_eq!(path, Some(String::from("/home/user/work")));

    // Duplicate name.
    let result = add("work", "/other/path", "", Category::Favorites);
    assert!(result.is_err());

    // Clean up.
    let _ = remove("work");
    serial_println!("[bookmarks]   add_and_resolve: ok");
}

fn test_remove() {
    let _ = add("temp", "/tmp/test", "Temp", Category::Favorites);

    // Remove user bookmark — should succeed.
    assert!(remove("temp").is_ok());

    // Remove system bookmark — should fail.
    assert!(remove("home").is_err());

    // Remove nonexistent — should fail.
    assert!(remove("nonexistent").is_err());

    serial_println!("[bookmarks]   remove: ok");
}

fn test_rename() {
    let _ = add("old_name", "/some/path", "Old", Category::Favorites);

    assert!(rename("old_name", "new_name").is_ok());
    assert!(resolve("old_name").is_none());
    assert!(resolve("new_name").is_some());

    // Rename to existing name.
    assert!(rename("new_name", "home").is_err());

    let _ = remove("new_name");
    serial_println!("[bookmarks]   rename: ok");
}

fn test_categories() {
    let _ = add("net_share", "/mnt/network", "Network Share", Category::Network);

    let net_bm = list_category(Category::Network);
    assert!(net_bm.iter().any(|b| b.name == "net_share"));

    let places = list_category(Category::Places);
    assert!(places.iter().any(|b| b.name == "home"));
    assert!(!places.iter().any(|b| b.name == "net_share"));

    let _ = remove("net_share");
    serial_println!("[bookmarks]   categories: ok");
}

fn test_visibility() {
    assert!(set_visible("home", false).is_ok());

    let visible = list_visible();
    assert!(!visible.iter().any(|b| b.name == "home"));

    let all = list();
    assert!(all.iter().any(|b| b.name == "home"));

    // Restore.
    assert!(set_visible("home", true).is_ok());

    serial_println!("[bookmarks]   visibility: ok");
}
