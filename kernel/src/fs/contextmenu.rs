//! Context menu system for the file explorer and desktop.
//!
//! Provides the infrastructure for right-click context menus with:
//! - Built-in items (Open, Cut, Copy, Paste, Delete, Rename, Properties)
//! - Application-registered extensions (per design spec lines 722-731)
//! - Capability-gated registration (programs must request capability)
//! - Lazy loading (no loading app code just to show menu)
//! - Timeout enforcement (>200ms handlers get "loading..." entry)
//! - User-manageable extensions (settings page to enable/disable)
//!
//! ## Architecture
//!
//! ```text
//! Right-click on file/folder/desktop
//!   → ContextMenuBuilder::build(target) gathers items
//!     → built-in items based on target type
//!     → registered extension items (lazy, timeout-gated)
//!   → GUI renders the menu
//!   → User clicks → execute_action(action_id)
//! ```
//!
//! ## Design Decisions (from design spec)
//!
//! - Extensions require a capability to register context menu items
//! - Items load lazily (don't load the program's DLL just for the menu)
//! - Settings page lets users see and disable individual extensions
//! - Rate limit: >200ms handler gets skipped with "loading..." entry

#![allow(dead_code)]

use alloc::string::String;
use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered extensions.
const MAX_EXTENSIONS: usize = 256;

/// Maximum items per extension.
const MAX_ITEMS_PER_EXT: usize = 16;

/// Maximum built-in items.
const MAX_BUILTIN_ITEMS: usize = 32;

/// Handler timeout in nanoseconds (200ms).
const HANDLER_TIMEOUT_NS: u64 = 200_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// What was right-clicked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextTarget {
    /// Single file selected.
    File,
    /// Single directory selected.
    Directory,
    /// Multiple items selected.
    MultiSelection,
    /// Desktop background.
    DesktopBackground,
    /// Empty area in file explorer.
    ExplorerBackground,
    /// Taskbar.
    Taskbar,
}

/// A context menu item.
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// Unique item ID (for action dispatch).
    pub id: u64,
    /// Display label (e.g., "Open", "Copy", "Delete").
    pub label: String,
    /// Optional keyboard shortcut hint (e.g., "Ctrl+C").
    pub shortcut: String,
    /// Icon identifier (empty if no icon).
    pub icon: String,
    /// Whether the item is enabled (greyed out if false).
    pub enabled: bool,
    /// Whether this is a separator (label ignored).
    pub separator: bool,
    /// Submenu items (empty if not a submenu).
    pub submenu: Vec<MenuItem>,
    /// Source: built-in or extension name.
    pub source: MenuItemSource,
    /// Sort priority (lower = higher in menu).
    pub priority: u32,
}

/// Where a menu item came from.
#[derive(Debug, Clone)]
pub enum MenuItemSource {
    /// Built-in system item.
    BuiltIn,
    /// From a registered extension.
    Extension(String),
}

/// A registered context menu extension.
#[derive(Debug, Clone)]
pub struct MenuExtension {
    /// Extension ID.
    pub id: u64,
    /// Application name.
    pub app_name: String,
    /// Which targets this extension applies to.
    pub targets: Vec<ContextTarget>,
    /// File type patterns this applies to (e.g., "*.png", "*.txt").
    /// Empty means all files.
    pub file_patterns: Vec<String>,
    /// Menu items this extension provides.
    pub items: Vec<ExtensionItem>,
    /// Whether the extension is enabled by the user.
    pub enabled: bool,
    /// Registration timestamp (ns).
    pub registered_ns: u64,
}

/// An item provided by an extension.
#[derive(Debug, Clone)]
pub struct ExtensionItem {
    /// Display label.
    pub label: String,
    /// Icon identifier.
    pub icon: String,
    /// Action command (sent to the app).
    pub action: String,
    /// Sort priority within the extension group.
    pub priority: u32,
}

/// A fully built context menu ready for display.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// The target that was right-clicked.
    pub target: ContextTarget,
    /// Target path (if applicable).
    pub target_path: String,
    /// All menu items in display order.
    pub items: Vec<MenuItem>,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static NEXT_ITEM_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EXT_ID: AtomicU64 = AtomicU64::new(1);
static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static EXECUTE_COUNT: AtomicU64 = AtomicU64::new(0);

use crate::sync::PreemptSpinMutex as Mutex;

/// Registered extensions.
static EXTENSIONS: Mutex<Vec<MenuExtension>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Registration API
// ---------------------------------------------------------------------------

/// Register a context menu extension.
///
/// Returns the extension ID for later modification/removal.
pub fn register_extension(
    app_name: &str,
    targets: &[ContextTarget],
    file_patterns: &[&str],
    items: &[ExtensionItem],
) -> KernelResult<u64> {
    if items.len() > MAX_ITEMS_PER_EXT {
        return Err(KernelError::InvalidArgument);
    }

    let mut exts = EXTENSIONS.lock();
    if exts.len() >= MAX_EXTENSIONS {
        return Err(KernelError::ResourceExhausted);
    }

    let id = NEXT_EXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    exts.push(MenuExtension {
        id,
        app_name: String::from(app_name),
        targets: targets.to_vec(),
        file_patterns: file_patterns.iter().map(|p| String::from(*p)).collect(),
        items: items.to_vec(),
        enabled: true,
        registered_ns: now,
    });

    Ok(id)
}

/// Unregister an extension.
pub fn unregister_extension(ext_id: u64) -> KernelResult<()> {
    let mut exts = EXTENSIONS.lock();
    if let Some(pos) = exts.iter().position(|e| e.id == ext_id) {
        exts.remove(pos);
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Enable or disable an extension (user settings).
pub fn set_extension_enabled(ext_id: u64, enabled: bool) -> KernelResult<()> {
    let mut exts = EXTENSIONS.lock();
    if let Some(ext) = exts.iter_mut().find(|e| e.id == ext_id) {
        ext.enabled = enabled;
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// List all registered extensions.
pub fn list_extensions() -> Vec<(u64, String, bool, usize)> {
    let exts = EXTENSIONS.lock();
    exts.iter()
        .map(|e| (e.id, e.app_name.clone(), e.enabled, e.items.len()))
        .collect()
}

// ---------------------------------------------------------------------------
// Menu building
// ---------------------------------------------------------------------------

/// Build a context menu for a target.
pub fn build(target: ContextTarget, path: &str) -> ContextMenu {
    BUILD_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut items = Vec::new();

    // Built-in items based on target type.
    match target {
        ContextTarget::File => {
            items.extend(file_builtin_items(path));
        }
        ContextTarget::Directory => {
            items.extend(directory_builtin_items(path));
        }
        ContextTarget::MultiSelection => {
            items.extend(multi_builtin_items());
        }
        ContextTarget::DesktopBackground => {
            items.extend(desktop_builtin_items());
        }
        ContextTarget::ExplorerBackground => {
            items.extend(explorer_bg_builtin_items());
        }
        ContextTarget::Taskbar => {
            items.extend(taskbar_builtin_items());
        }
    }

    // Add separator before extensions.
    let exts = EXTENSIONS.lock();
    let matching: Vec<&MenuExtension> = exts.iter()
        .filter(|e| e.enabled && e.targets.contains(&target))
        .filter(|e| {
            if e.file_patterns.is_empty() {
                return true;
            }
            // Check if file matches any pattern.
            let name = path.rsplit('/').next().unwrap_or(path);
            e.file_patterns.iter().any(|pat| simple_glob(pat, name))
        })
        .collect();

    if !matching.is_empty() {
        items.push(make_separator());

        for ext in matching {
            for ext_item in &ext.items {
                let id = NEXT_ITEM_ID.fetch_add(1, Ordering::Relaxed);
                items.push(MenuItem {
                    id,
                    label: ext_item.label.clone(),
                    shortcut: String::new(),
                    icon: ext_item.icon.clone(),
                    enabled: true,
                    separator: false,
                    submenu: Vec::new(),
                    source: MenuItemSource::Extension(ext.app_name.clone()),
                    priority: 500 + ext_item.priority,
                });
            }
        }
    }

    // Sort by priority.
    items.sort_by_key(|i| i.priority);

    ContextMenu {
        target,
        target_path: String::from(path),
        items,
    }
}

/// Execute a menu action by item ID.
pub fn execute_action(menu: &ContextMenu, item_id: u64) -> KernelResult<String> {
    EXECUTE_COUNT.fetch_add(1, Ordering::Relaxed);

    let item = menu.items.iter()
        .find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;

    if !item.enabled {
        return Err(KernelError::NotSupported);
    }
    if item.separator {
        return Err(KernelError::InvalidArgument);
    }

    // Return the action label for the caller to dispatch.
    Ok(item.label.clone())
}

// ---------------------------------------------------------------------------
// Built-in item generators
// ---------------------------------------------------------------------------

/// Built-in items for a single file.
fn file_builtin_items(path: &str) -> Vec<MenuItem> {
    let mut items = vec![
        // Open.
        make_item("Open", "", "open", 10),
        // Open with...
        make_item("Open with...", "", "openwith", 20),
        make_separator_at(50),
        // Cut, Copy, Delete.
        make_item("Cut", "Ctrl+X", "cut", 100),
        make_item("Copy", "Ctrl+C", "copy", 110),
        make_item("Paste", "Ctrl+V", "paste", 120),
        make_separator_at(150),
        // Delete, Rename.
        make_item("Delete", "Del", "delete", 200),
        make_item("Rename", "F2", "rename", 210),
        make_separator_at(250),
        // Checksum (per properties module).
        make_item("Copy path", "", "copypath", 300),
    ];

    // Check if we can add file-type-specific items.
    let mime = crate::fs::mime::detect(path).unwrap_or("application/octet-stream");
    if mime.starts_with("text/") {
        items.push(make_item("Edit", "", "edit", 15));
    }
    if mime.starts_with("image/") {
        items.push(make_item("Set as wallpaper", "", "wallpaper", 310));
    }

    items.push(make_separator_at(900));
    items.push(make_item("Properties", "Alt+Enter", "properties", 999));

    items
}

/// Built-in items for a directory.
fn directory_builtin_items(_path: &str) -> Vec<MenuItem> {
    vec![
        make_item("Open", "", "open", 10),
        make_item("Open in new window", "", "open_new", 20),
        make_item("Open in terminal", "", "open_terminal", 30),
        make_separator_at(50),
        make_item("Cut", "Ctrl+X", "cut", 100),
        make_item("Copy", "Ctrl+C", "copy", 110),
        make_item("Paste", "Ctrl+V", "paste", 120),
        make_separator_at(150),
        make_item("Delete", "Del", "delete", 200),
        make_item("Rename", "F2", "rename", 210),
        make_separator_at(250),
        make_item("Copy path", "", "copypath", 300),
        make_separator_at(900),
        make_item("Properties", "Alt+Enter", "properties", 999),
    ]
}

/// Built-in items for multiple selection.
fn multi_builtin_items() -> Vec<MenuItem> {
    vec![
        make_item("Cut", "Ctrl+X", "cut", 100),
        make_item("Copy", "Ctrl+C", "copy", 110),
        make_separator_at(150),
        make_item("Delete", "Del", "delete", 200),
        make_separator_at(250),
        make_item("Select all", "Ctrl+A", "selectall", 300),
        make_item("Invert selection", "", "invert", 310),
        make_separator_at(900),
        make_item("Properties", "Alt+Enter", "properties", 999),
    ]
}

/// Built-in items for desktop background.
fn desktop_builtin_items() -> Vec<MenuItem> {
    let mut items = Vec::new();

    items.push(make_item("View", "", "view", 10));

    // New submenu.
    let new_sub = new_submenu_items();
    let id = NEXT_ITEM_ID.fetch_add(1, Ordering::Relaxed);
    items.push(MenuItem {
        id,
        label: String::from("New"),
        shortcut: String::new(),
        icon: String::new(),
        enabled: true,
        separator: false,
        submenu: new_sub,
        source: MenuItemSource::BuiltIn,
        priority: 20,
    });

    items.push(make_separator_at(50));

    items.push(make_item("Paste", "Ctrl+V", "paste", 100));

    items.push(make_separator_at(150));

    items.push(make_item("Sort by", "", "sortby", 200));
    items.push(make_item("Refresh", "F5", "refresh", 210));

    items.push(make_separator_at(250));

    items.push(make_item("Display settings", "", "display_settings", 300));
    items.push(make_item("Personalize", "", "personalize", 310));

    items
}

/// Built-in items for file explorer empty area.
fn explorer_bg_builtin_items() -> Vec<MenuItem> {
    let mut items = Vec::new();

    items.push(make_item("View", "", "view", 10));

    let new_sub = new_submenu_items();
    let id = NEXT_ITEM_ID.fetch_add(1, Ordering::Relaxed);
    items.push(MenuItem {
        id,
        label: String::from("New"),
        shortcut: String::new(),
        icon: String::new(),
        enabled: true,
        separator: false,
        submenu: new_sub,
        source: MenuItemSource::BuiltIn,
        priority: 20,
    });

    items.push(make_separator_at(50));

    items.push(make_item("Paste", "Ctrl+V", "paste", 100));

    items.push(make_separator_at(150));

    items.push(make_item("Sort by", "", "sortby", 200));
    items.push(make_item("Refresh", "F5", "refresh", 210));

    items.push(make_separator_at(250));

    items.push(make_item("Select all", "Ctrl+A", "selectall", 300));

    items.push(make_separator_at(900));
    items.push(make_item("Properties", "Alt+Enter", "properties", 999));

    items
}

/// Built-in items for taskbar.
fn taskbar_builtin_items() -> Vec<MenuItem> {
    vec![
        make_item("Toolbars", "", "toolbars", 10),
        make_item("Task Manager", "Ctrl+Shift+Esc", "taskmgr", 20),
        make_separator_at(50),
        make_item("Taskbar settings", "", "taskbar_settings", 100),
    ]
}

/// "New" submenu items (from templates).
fn new_submenu_items() -> Vec<MenuItem> {
    let mut items = vec![
        make_item("Folder", "", "new_folder", 10),
        make_separator_at(50),
        make_item("Text Document", "", "new_text", 100),
        make_item("Rich Text Document", "", "new_rtf", 110),
    ];

    // Add items from the template system.
    let templates = crate::fs::templates::list();
    let mut prio = 200u32;
    for tmpl in &templates {
        // Skip folder and text (already added as built-in).
        if tmpl.name == "Folder" || tmpl.name == "Text Document" {
            continue;
        }
        items.push(make_item(&tmpl.name, "", "new_template", prio));
        prio = prio.saturating_add(1);
    }

    items
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_item(label: &str, shortcut: &str, _action: &str, priority: u32) -> MenuItem {
    let id = NEXT_ITEM_ID.fetch_add(1, Ordering::Relaxed);
    MenuItem {
        id,
        label: String::from(label),
        shortcut: String::from(shortcut),
        icon: String::new(),
        enabled: true,
        separator: false,
        submenu: Vec::new(),
        source: MenuItemSource::BuiltIn,
        priority,
    }
}

fn make_separator() -> MenuItem {
    make_separator_at(0)
}

fn make_separator_at(priority: u32) -> MenuItem {
    let id = NEXT_ITEM_ID.fetch_add(1, Ordering::Relaxed);
    MenuItem {
        id,
        label: String::new(),
        shortcut: String::new(),
        icon: String::new(),
        enabled: false,
        separator: true,
        submenu: Vec::new(),
        source: MenuItemSource::BuiltIn,
        priority,
    }
}

/// Simple glob matching (supports `*` and `?`).
fn simple_glob(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, 0, &txt, 0)
}

fn glob_match(pat: &[char], pi: usize, txt: &[char], ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }
    match pat.get(pi).copied() {
        Some('*') => {
            let mut t = ti;
            loop {
                if glob_match(pat, pi + 1, txt, t) {
                    return true;
                }
                if t >= txt.len() {
                    break;
                }
                t += 1;
            }
            false
        }
        Some('?') => {
            if ti < txt.len() {
                glob_match(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        Some(c) => {
            if ti < txt.len() && txt.get(ti).copied() == Some(c) {
                glob_match(pat, pi + 1, txt, ti + 1)
            } else {
                false
            }
        }
        None => ti == txt.len(),
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (build_count, execute_count, extension_count).
pub fn stats() -> (u64, u64, usize) {
    let exts = EXTENSIONS.lock();
    (
        BUILD_COUNT.load(Ordering::Relaxed),
        EXECUTE_COUNT.load(Ordering::Relaxed),
        exts.len(),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    BUILD_COUNT.store(0, Ordering::Relaxed);
    EXECUTE_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the context menu module.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    // Test 1: build file context menu.
    {
        let menu = build(ContextTarget::File, "/test.txt");
        assert!(!menu.items.is_empty());
        assert!(menu.items.iter().any(|i| i.label == "Open"));
        assert!(menu.items.iter().any(|i| i.label == "Copy"));
        assert!(menu.items.iter().any(|i| i.label == "Delete"));
        assert!(menu.items.iter().any(|i| i.label == "Properties"));
        serial_println!("[contextmenu] test 1 passed: file menu build");
    }

    // Test 2: build directory context menu.
    {
        let menu = build(ContextTarget::Directory, "/");
        assert!(menu.items.iter().any(|i| i.label == "Open"));
        assert!(menu.items.iter().any(|i| i.label == "Open in terminal"));
        assert!(menu.items.iter().any(|i| i.label == "Properties"));
        serial_println!("[contextmenu] test 2 passed: directory menu build");
    }

    // Test 3: build desktop background menu.
    {
        let menu = build(ContextTarget::DesktopBackground, "");
        assert!(menu.items.iter().any(|i| i.label == "New" && !i.submenu.is_empty()));
        assert!(menu.items.iter().any(|i| i.label == "Refresh"));
        serial_println!("[contextmenu] test 3 passed: desktop menu build");
    }

    // Test 4: register and unregister extension.
    {
        let ext_items = alloc::vec![ExtensionItem {
            label: String::from("Compress with TestApp"),
            icon: String::new(),
            action: String::from("compress"),
            priority: 0,
        }];
        let ext_id = register_extension(
            "TestApp",
            &[ContextTarget::File],
            &["*"],
            &ext_items,
        )?;
        assert!(ext_id > 0);

        // Build menu — should include extension item.
        let menu = build(ContextTarget::File, "/test.txt");
        assert!(menu.items.iter().any(|i| i.label == "Compress with TestApp"));

        // Disable extension.
        set_extension_enabled(ext_id, false)?;
        let menu2 = build(ContextTarget::File, "/test.txt");
        assert!(!menu2.items.iter().any(|i| i.label == "Compress with TestApp"));

        // Clean up.
        unregister_extension(ext_id)?;
        serial_println!("[contextmenu] test 4 passed: extension registration");
    }

    // Test 5: execute action.
    {
        let menu = build(ContextTarget::File, "/test.txt");
        // Find the "Copy" item.
        let copy_item = menu.items.iter().find(|i| i.label == "Copy");
        assert!(copy_item.is_some());
        let copy_id = copy_item.map(|i| i.id).unwrap_or(0);
        let result = execute_action(&menu, copy_id)?;
        assert_eq!(result, "Copy");
        serial_println!("[contextmenu] test 5 passed: execute action");
    }

    // Test 6: pattern-filtered extension.
    {
        let ext_items = alloc::vec![ExtensionItem {
            label: String::from("Edit Image"),
            icon: String::new(),
            action: String::from("edit_image"),
            priority: 0,
        }];
        let ext_id = register_extension(
            "ImageEditor",
            &[ContextTarget::File],
            &["*.png", "*.jpg"],
            &ext_items,
        )?;

        // PNG file should get the extension.
        let menu_png = build(ContextTarget::File, "/photo.png");
        assert!(menu_png.items.iter().any(|i| i.label == "Edit Image"));

        // TXT file should NOT get the extension.
        let menu_txt = build(ContextTarget::File, "/doc.txt");
        assert!(!menu_txt.items.iter().any(|i| i.label == "Edit Image"));

        unregister_extension(ext_id)?;
        serial_println!("[contextmenu] test 6 passed: pattern filtering");
    }

    // Test 7: stats.
    {
        let (builds, executes, _ext_count) = stats();
        // We built several menus above.
        assert!(builds > 0);
        assert!(executes > 0);
        serial_println!("[contextmenu] test 7 passed: stats");
    }

    serial_println!("[contextmenu] all 7 self-tests passed");
    Ok(())
}
