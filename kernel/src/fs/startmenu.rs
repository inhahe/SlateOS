//! Start menu — application launcher and system actions.
//!
//! Builds the start menu tree from the application registry,
//! provides search/filter, and manages system action entries
//! (power off, logout, reboot, sleep, etc.).
//!
//! ## Design Reference
//!
//! design.txt line 721:
//! "start menu, contains applications tree, settings icon, terminal,
//!  power off, logout, reboot, hibernate?, sleep, reboot in safe mode,
//!  reboot the OS but without rebooting the PC?, input field for
//!  finding and running apps"
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────┐
//! │ 🔍 Search apps...           │
//! ├─────────────────────────────┤
//! │ ★ Pinned / Favorites        │
//! │   [Files] [Terminal] [Web]  │
//! ├─────────────────────────────┤
//! │ 📂 All Apps (by category)   │
//! │   Accessories ▸             │
//! │   Development ▸             │
//! │   Internet ▸                │
//! │   ...                       │
//! ├─────────────────────────────┤
//! │ ⚡ Quick Links               │
//! │   ⚙ Settings                │
//! │   >_ Terminal               │
//! ├─────────────────────────────┤
//! │ ⏻ Power                     │
//! │   Shut down / Restart /     │
//! │   Sleep / Hibernate /       │
//! │   Log out / Lock            │
//! └─────────────────────────────┘
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

/// Maximum pinned/favorite apps in start menu.
const MAX_FAVORITES: usize = 32;

/// Maximum recent apps tracked.
const MAX_RECENT: usize = 20;

/// Maximum quick-link entries.
const MAX_QUICK_LINKS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// System power/session action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAction {
    /// Shut down the computer.
    ShutDown,
    /// Restart the computer.
    Restart,
    /// Suspend to RAM.
    Sleep,
    /// Suspend to disk.
    Hibernate,
    /// Log out current user.
    LogOut,
    /// Lock the screen.
    Lock,
    /// Restart into safe mode.
    RestartSafeMode,
    /// Restart the OS without rebooting hardware.
    SoftRestart,
}

impl SystemAction {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ShutDown => "Shut down",
            Self::Restart => "Restart",
            Self::Sleep => "Sleep",
            Self::Hibernate => "Hibernate",
            Self::LogOut => "Log out",
            Self::Lock => "Lock",
            Self::RestartSafeMode => "Restart (Safe Mode)",
            Self::SoftRestart => "Soft Restart",
        }
    }

    /// Icon name.
    pub fn icon(self) -> &'static str {
        match self {
            Self::ShutDown => "icon-power-off",
            Self::Restart => "icon-restart",
            Self::Sleep => "icon-sleep",
            Self::Hibernate => "icon-hibernate",
            Self::LogOut => "icon-logout",
            Self::Lock => "icon-lock",
            Self::RestartSafeMode => "icon-safe-mode",
            Self::SoftRestart => "icon-soft-restart",
        }
    }

    /// All available system actions.
    pub fn all() -> &'static [SystemAction] {
        &[
            Self::ShutDown,
            Self::Restart,
            Self::Sleep,
            Self::Hibernate,
            Self::LogOut,
            Self::Lock,
            Self::RestartSafeMode,
            Self::SoftRestart,
        ]
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "shutdown" | "poweroff" | "off" => Some(Self::ShutDown),
            "restart" | "reboot" => Some(Self::Restart),
            "sleep" | "suspend" => Some(Self::Sleep),
            "hibernate" | "hib" => Some(Self::Hibernate),
            "logout" | "logoff" => Some(Self::LogOut),
            "lock" => Some(Self::Lock),
            "safemode" | "safe" => Some(Self::RestartSafeMode),
            "softrestart" | "soft" => Some(Self::SoftRestart),
            _ => None,
        }
    }
}

/// A quick-link entry (e.g., Settings, Terminal).
#[derive(Debug, Clone)]
pub struct QuickLink {
    /// Application ID.
    pub app_id: String,
    /// Display label.
    pub label: String,
    /// Icon name.
    pub icon: String,
    /// Position (lower = higher in list).
    pub position: u32,
}

/// A recent app entry.
#[derive(Debug, Clone)]
pub struct RecentApp {
    /// Application ID.
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Number of times launched.
    pub launch_count: u32,
    /// Last launched timestamp (nanoseconds).
    pub last_launched_ns: u64,
}

/// Section of the start menu for rendering.
#[derive(Debug, Clone)]
pub enum MenuSection {
    /// Search results (when search is active).
    SearchResults(Vec<SearchResult>),
    /// Pinned/favorite apps.
    Favorites(Vec<FavoriteEntry>),
    /// All apps organized by category.
    AllApps(Vec<CategoryGroup>),
    /// Quick links (Settings, Terminal, etc.).
    QuickLinks(Vec<QuickLink>),
    /// Recently used apps.
    RecentApps(Vec<RecentApp>),
    /// System actions (power, logout, etc.).
    SystemActions(Vec<SystemAction>),
}

/// A favorite/pinned app in the start menu.
#[derive(Debug, Clone)]
pub struct FavoriteEntry {
    /// Application ID.
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Icon name.
    pub icon: String,
    /// Position.
    pub position: u32,
}

/// A category group for the All Apps view.
#[derive(Debug, Clone)]
pub struct CategoryGroup {
    /// Category label.
    pub label: String,
    /// Apps in this category.
    pub apps: Vec<AppEntry>,
}

/// An app entry in the menu.
#[derive(Debug, Clone)]
pub struct AppEntry {
    /// Application ID.
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Icon name.
    pub icon: String,
    /// Executable path.
    pub exec_path: String,
}

/// A search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Application ID.
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Icon name.
    pub icon: String,
    /// Match description (e.g., "name match", "keyword match").
    pub match_desc: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct StartMenuState {
    /// Pinned/favorite app IDs, ordered.
    favorites: Vec<String>,
    /// Quick links.
    quick_links: Vec<QuickLink>,
    /// Recent app entries.
    recent: Vec<RecentApp>,
    /// Whether to show recent apps section.
    show_recent: bool,
    /// Next quick-link position counter.
    next_ql_pos: u32,
}

impl StartMenuState {
    const fn new() -> Self {
        Self {
            favorites: Vec::new(),
            quick_links: Vec::new(),
            recent: Vec::new(),
            show_recent: true,
            next_ql_pos: 0,
        }
    }
}

static MENU: Mutex<StartMenuState> = Mutex::new(StartMenuState::new());
static OPEN_COUNT: AtomicU64 = AtomicU64::new(0);
static SEARCH_COUNT: AtomicU64 = AtomicU64::new(0);
static LAUNCH_COUNT: AtomicU64 = AtomicU64::new(0);

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
// Favorites API
// ---------------------------------------------------------------------------

/// Add an app to start menu favorites.
pub fn add_favorite(app_id: &str) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut menu = MENU.lock();
    if menu.favorites.iter().any(|id| id == app_id) {
        return Err(KernelError::AlreadyExists);
    }
    if menu.favorites.len() >= MAX_FAVORITES {
        return Err(KernelError::ResourceExhausted);
    }
    menu.favorites.push(String::from(app_id));
    Ok(())
}

/// Remove an app from start menu favorites.
pub fn remove_favorite(app_id: &str) -> KernelResult<()> {
    let mut menu = MENU.lock();
    let idx = menu.favorites.iter().position(|id| id == app_id)
        .ok_or(KernelError::NotFound)?;
    menu.favorites.remove(idx);
    Ok(())
}

/// Reorder a favorite to a new position.
pub fn reorder_favorite(app_id: &str, new_pos: usize) -> KernelResult<()> {
    let mut menu = MENU.lock();
    let idx = menu.favorites.iter().position(|id| id == app_id)
        .ok_or(KernelError::NotFound)?;
    let item = menu.favorites.remove(idx);
    let target = new_pos.min(menu.favorites.len());
    menu.favorites.insert(target, item);
    Ok(())
}

/// Get favorites with appregistry info.
pub fn favorites() -> Vec<FavoriteEntry> {
    let menu = MENU.lock();
    menu.favorites.iter().enumerate().filter_map(|(i, app_id)| {
        super::appregistry::get(app_id).map(|app| FavoriteEntry {
            app_id: app.id,
            name: app.name,
            icon: app.icon,
            position: i as u32,
        })
    }).collect()
}

// ---------------------------------------------------------------------------
// Quick links API
// ---------------------------------------------------------------------------

/// Add a quick link (e.g., Settings, Terminal).
pub fn add_quick_link(app_id: &str, label: &str, icon: &str) -> KernelResult<()> {
    if app_id.is_empty() || label.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut menu = MENU.lock();
    if menu.quick_links.iter().any(|ql| ql.app_id == app_id) {
        return Err(KernelError::AlreadyExists);
    }
    if menu.quick_links.len() >= MAX_QUICK_LINKS {
        return Err(KernelError::ResourceExhausted);
    }
    let pos = menu.next_ql_pos;
    menu.next_ql_pos = menu.next_ql_pos.saturating_add(1);
    menu.quick_links.push(QuickLink {
        app_id: String::from(app_id),
        label: String::from(label),
        icon: String::from(icon),
        position: pos,
    });
    Ok(())
}

/// Remove a quick link.
pub fn remove_quick_link(app_id: &str) -> KernelResult<()> {
    let mut menu = MENU.lock();
    let idx = menu.quick_links.iter().position(|ql| ql.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    menu.quick_links.remove(idx);
    Ok(())
}

/// Get all quick links.
pub fn quick_links() -> Vec<QuickLink> {
    let menu = MENU.lock();
    let mut links = menu.quick_links.clone();
    links.sort_by_key(|ql| ql.position);
    links
}

// ---------------------------------------------------------------------------
// Recent apps API
// ---------------------------------------------------------------------------

/// Record an app launch (updates recent list).
pub fn record_launch(app_id: &str) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    LAUNCH_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();

    let mut menu = MENU.lock();
    if let Some(entry) = menu.recent.iter_mut().find(|r| r.app_id == app_id) {
        entry.launch_count = entry.launch_count.saturating_add(1);
        entry.last_launched_ns = now;
    } else {
        let name = super::appregistry::get(app_id)
            .map_or_else(|| String::from(app_id), |app| app.name);
        if menu.recent.len() >= MAX_RECENT {
            // Remove oldest.
            if let Some(oldest_idx) = menu.recent.iter().enumerate()
                .min_by_key(|(_, r)| r.last_launched_ns)
                .map(|(i, _)| i)
            {
                menu.recent.remove(oldest_idx);
            }
        }
        menu.recent.push(RecentApp {
            app_id: String::from(app_id),
            name,
            launch_count: 1,
            last_launched_ns: now,
        });
    }

    // Sort by last launched (most recent first).
    menu.recent.sort_by_key(|e| core::cmp::Reverse(e.last_launched_ns));
    Ok(())
}

/// Get recent apps.
pub fn recent_apps() -> Vec<RecentApp> {
    let menu = MENU.lock();
    menu.recent.clone()
}

/// Clear recent apps list.
pub fn clear_recent() {
    let mut menu = MENU.lock();
    menu.recent.clear();
}

/// Toggle showing recent apps section.
pub fn set_show_recent(show: bool) {
    let mut menu = MENU.lock();
    menu.show_recent = show;
}

// ---------------------------------------------------------------------------
// Search API
// ---------------------------------------------------------------------------

/// Search for apps in the start menu.
pub fn search(query: &str) -> Vec<SearchResult> {
    SEARCH_COUNT.fetch_add(1, Ordering::Relaxed);
    if query.is_empty() {
        return Vec::new();
    }

    // Delegate to appregistry search.
    let results = super::appregistry::search(query);
    let q = to_lower(query);

    results.iter().map(|app| {
        let match_desc = if to_lower(&app.name).contains(&q) {
            String::from("name")
        } else if app.keywords.iter().any(|k| to_lower(k).contains(&q)) {
            String::from("keyword")
        } else if to_lower(&app.description).contains(&q) {
            String::from("description")
        } else {
            String::from("id")
        };
        SearchResult {
            app_id: app.id.clone(),
            name: app.name.clone(),
            icon: app.icon.clone(),
            match_desc,
        }
    }).collect()
}

// ---------------------------------------------------------------------------
// Full menu build
// ---------------------------------------------------------------------------

/// Build the complete start menu for rendering.
pub fn build_menu() -> Vec<MenuSection> {
    OPEN_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut sections = Vec::new();

    // Favorites.
    let favs = favorites();
    if !favs.is_empty() {
        sections.push(MenuSection::Favorites(favs));
    }

    // All apps (from appregistry).
    let tree = super::appregistry::menu_tree();
    if !tree.is_empty() {
        let groups: Vec<CategoryGroup> = tree.into_iter().map(|(cat, entries)| {
            CategoryGroup {
                label: String::from(cat.label()),
                apps: entries.into_iter().map(|e| AppEntry {
                    app_id: e.id,
                    name: e.name,
                    icon: e.icon,
                    exec_path: e.exec_path,
                }).collect(),
            }
        }).collect();
        sections.push(MenuSection::AllApps(groups));
    }

    // Quick links.
    let links = quick_links();
    if !links.is_empty() {
        sections.push(MenuSection::QuickLinks(links));
    }

    // Recent apps.
    let menu = MENU.lock();
    if menu.show_recent && !menu.recent.is_empty() {
        sections.push(MenuSection::RecentApps(menu.recent.clone()));
    }
    drop(menu);

    // System actions (always present).
    sections.push(MenuSection::SystemActions(SystemAction::all().to_vec()));

    sections
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Set up default quick links and favorites.
pub fn init_defaults() -> KernelResult<()> {
    // Default quick links: Settings and Terminal.
    let _ = add_quick_link("org.os.settings", "Settings", "icon-settings");
    let _ = add_quick_link("org.os.terminal", "Terminal", "icon-terminal");

    // Default favorites from common apps.
    let default_favs = [
        "org.os.files",
        "org.os.terminal",
        "org.os.editor",
        "org.os.settings",
        "org.os.calculator",
    ];
    for id in &default_favs {
        let _ = add_favorite(id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (favorites, quick_links, recent, open_ops, search_ops, launch_ops).
pub fn stats() -> (usize, usize, usize, u64, u64, u64) {
    let menu = MENU.lock();
    (
        menu.favorites.len(),
        menu.quick_links.len(),
        menu.recent.len(),
        OPEN_COUNT.load(Ordering::Relaxed),
        SEARCH_COUNT.load(Ordering::Relaxed),
        LAUNCH_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    OPEN_COUNT.store(0, Ordering::Relaxed);
    SEARCH_COUNT.store(0, Ordering::Relaxed);
    LAUNCH_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut menu = MENU.lock();
    menu.favorites.clear();
    menu.quick_links.clear();
    menu.recent.clear();
    menu.show_recent = true;
    menu.next_ql_pos = 0;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the start menu.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: add and list favorites.
    {
        add_favorite("test.app1")?;
        add_favorite("test.app2")?;
        let favs = {
            let menu = MENU.lock();
            menu.favorites.clone()
        };
        assert_eq!(favs.len(), 2);
        assert_eq!(favs[0], "test.app1");

        // Duplicate should fail.
        assert!(add_favorite("test.app1").is_err());
        serial_println!("[startmenu] test 1 passed: favorites");
    }

    // Test 2: reorder favorites.
    {
        reorder_favorite("test.app2", 0)?;
        let favs = {
            let menu = MENU.lock();
            menu.favorites.clone()
        };
        assert_eq!(favs[0], "test.app2");
        assert_eq!(favs[1], "test.app1");
        serial_println!("[startmenu] test 2 passed: reorder favorites");
    }

    // Test 3: quick links.
    {
        add_quick_link("org.os.settings", "Settings", "icon-settings")?;
        add_quick_link("org.os.terminal", "Terminal", "icon-terminal")?;
        let links = quick_links();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].label, "Settings");

        // Duplicate should fail.
        assert!(add_quick_link("org.os.settings", "Settings", "icon-settings").is_err());
        serial_println!("[startmenu] test 3 passed: quick links");
    }

    // Test 4: recent apps.
    {
        record_launch("test.app1")?;
        record_launch("test.app2")?;
        record_launch("test.app1")?; // Second launch.
        let recent = recent_apps();
        assert_eq!(recent.len(), 2);
        // app1 was most recent.
        assert_eq!(recent[0].app_id, "test.app1");
        assert_eq!(recent[0].launch_count, 2);
        serial_println!("[startmenu] test 4 passed: recent apps");
    }

    // Test 5: search (delegates to appregistry).
    {
        // Register a test app in appregistry first.
        super::appregistry::register(super::appregistry::AppInfo {
            id: String::from("test.search"),
            name: String::from("Search Test App"),
            description: String::from("A test for searching"),
            exec_path: String::from("/usr/bin/test-search"),
            icon: String::from("icon-test"),
            categories: alloc::vec![super::appregistry::AppCategory::Accessories],
            mime_types: Vec::new(),
            keywords: alloc::vec![String::from("findme")],
            show_in_menu: true,
            tray_icon: false,
            start_hidden: false,
            version: String::from("1.0"),
            installed_ns: 0,
        })?;
        let results = search("findme");
        assert!(!results.is_empty());
        assert_eq!(results[0].match_desc, "keyword");

        let results = search("Search Test");
        assert!(!results.is_empty());
        assert_eq!(results[0].match_desc, "name");

        // Clean up.
        let _ = super::appregistry::unregister("test.search");
        serial_println!("[startmenu] test 5 passed: search");
    }

    // Test 6: build menu.
    {
        let sections = build_menu();
        // Should have at least favorites and system actions.
        assert!(sections.len() >= 2);
        // Last section should be system actions.
        let last = sections.last().unwrap();
        match last {
            MenuSection::SystemActions(actions) => {
                assert_eq!(actions.len(), 8);
            }
            _ => panic!("Last section should be SystemActions"),
        }
        serial_println!("[startmenu] test 6 passed: build menu");
    }

    // Test 7: remove and clear.
    {
        remove_favorite("test.app1")?;
        let favs = {
            let menu = MENU.lock();
            menu.favorites.clone()
        };
        assert_eq!(favs.len(), 1);

        remove_quick_link("org.os.settings")?;
        let links = quick_links();
        assert_eq!(links.len(), 1);

        clear_recent();
        let recent = recent_apps();
        assert!(recent.is_empty());
        serial_println!("[startmenu] test 7 passed: remove/clear");
    }

    clear_all();
    reset_stats();

    serial_println!("[startmenu] all 7 self-tests passed");
    Ok(())
}
