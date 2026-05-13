//! App Store — application marketplace and distribution.
//!
//! Manages a catalog of available applications, handles installation
//! requests, tracks installed versions, and provides update checking.
//!
//! ## Architecture
//!
//! ```text
//! User browses store
//!   → appstore::search(query) → matching apps
//!   → appstore::get_details(app_id) → full app info
//!
//! Install/update
//!   → appstore::install(app_id) → download + verify + install
//!   → appstore::update(app_id) → check version, install if newer
//!
//! Integration:
//!   → pkgmgr (actual package installation)
//!   → appregistry (register installed apps)
//!   → notifcenter (update available notifications)
//!   → appsandbox (permission review)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Application category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Productivity,
    Development,
    Graphics,
    Multimedia,
    Games,
    Utilities,
    System,
    Communication,
    Education,
    Other,
}

impl AppCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Productivity => "Productivity",
            Self::Development => "Development",
            Self::Graphics => "Graphics",
            Self::Multimedia => "Multimedia",
            Self::Games => "Games",
            Self::Utilities => "Utilities",
            Self::System => "System",
            Self::Communication => "Communication",
            Self::Education => "Education",
            Self::Other => "Other",
        }
    }
}

/// Installation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Available,
    Downloading,
    Installing,
    Installed,
    UpdateAvailable,
    Uninstalling,
    Failed,
}

impl InstallState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Downloading => "Downloading",
            Self::Installing => "Installing",
            Self::Installed => "Installed",
            Self::UpdateAvailable => "Update Available",
            Self::Uninstalling => "Uninstalling",
            Self::Failed => "Failed",
        }
    }
}

/// A store application entry.
#[derive(Debug, Clone)]
pub struct StoreApp {
    pub id: u32,
    pub name: String,
    pub developer: String,
    pub description: String,
    pub category: AppCategory,
    pub version: String,
    pub installed_version: String,
    pub size_kb: u64,
    pub state: InstallState,
    /// Rating in hundredths (450 = 4.50 stars).
    pub rating: u32,
    pub download_count: u64,
    pub added_ns: u64,
}

/// A user review.
#[derive(Debug, Clone)]
pub struct AppReview {
    pub id: u32,
    pub app_id: u32,
    pub user: String,
    /// Rating 1-5.
    pub rating: u8,
    pub comment: String,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 500;
const MAX_REVIEWS: usize = 1000;

struct State {
    apps: Vec<StoreApp>,
    reviews: Vec<AppReview>,
    next_app_id: u32,
    next_review_id: u32,
    total_installs: u64,
    total_uninstalls: u64,
    total_updates: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let now = crate::hpet::elapsed_ns();
    let apps = alloc::vec![
        StoreApp {
            id: 1, name: String::from("Text Editor Pro"), developer: String::from("DevTools Inc"),
            description: String::from("Advanced text editor with syntax highlighting"),
            category: AppCategory::Development, version: String::from("2.1.0"),
            installed_version: String::new(), size_kb: 15360,
            state: InstallState::Available, rating: 450, download_count: 25000, added_ns: now,
        },
        StoreApp {
            id: 2, name: String::from("Image Viewer"), developer: String::from("PixelCraft"),
            description: String::from("Fast image viewer with format support"),
            category: AppCategory::Graphics, version: String::from("1.5.0"),
            installed_version: String::new(), size_kb: 8192,
            state: InstallState::Available, rating: 420, download_count: 50000, added_ns: now,
        },
        StoreApp {
            id: 3, name: String::from("File Manager Plus"), developer: String::from("SystemUtils"),
            description: String::from("Dual-pane file manager with tabs"),
            category: AppCategory::Utilities, version: String::from("3.0.0"),
            installed_version: String::new(), size_kb: 12288,
            state: InstallState::Available, rating: 470, download_count: 100000, added_ns: now,
        },
    ];

    *guard = Some(State {
        apps,
        reviews: Vec::new(),
        next_app_id: 4,
        next_review_id: 1,
        total_installs: 0,
        total_uninstalls: 0,
        total_updates: 0,
        ops: 0,
    });
}

/// Add an app to the store catalog.
pub fn add_app(name: &str, developer: &str, description: &str, category: AppCategory, version: &str, size_kb: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.apps.len() >= MAX_APPS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_app_id;
        state.next_app_id += 1;
        state.apps.push(StoreApp {
            id, name: String::from(name), developer: String::from(developer),
            description: String::from(description), category,
            version: String::from(version), installed_version: String::new(),
            size_kb, state: InstallState::Available,
            rating: 0, download_count: 0, added_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Install an app.
pub fn install(app_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let app = state.apps.iter_mut().find(|a| a.id == app_id)
            .ok_or(KernelError::NotFound)?;
        if app.state == InstallState::Installed {
            return Err(KernelError::AlreadyExists);
        }
        app.state = InstallState::Installed;
        app.installed_version = app.version.clone();
        app.download_count += 1;
        state.total_installs += 1;
        Ok(())
    })
}

/// Uninstall an app.
pub fn uninstall(app_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let app = state.apps.iter_mut().find(|a| a.id == app_id)
            .ok_or(KernelError::NotFound)?;
        if app.state != InstallState::Installed && app.state != InstallState::UpdateAvailable {
            return Err(KernelError::InvalidArgument);
        }
        app.state = InstallState::Available;
        app.installed_version = String::new();
        state.total_uninstalls += 1;
        Ok(())
    })
}

/// Check for updates (sets UpdateAvailable if version differs).
pub fn check_updates() -> KernelResult<Vec<u32>> {
    with_state(|state| {
        let mut updated = Vec::new();
        for app in state.apps.iter_mut() {
            if app.state == InstallState::Installed && app.installed_version != app.version {
                app.state = InstallState::UpdateAvailable;
                updated.push(app.id);
            }
        }
        Ok(updated)
    })
}

/// Update an app to latest version.
pub fn update_app(app_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let app = state.apps.iter_mut().find(|a| a.id == app_id)
            .ok_or(KernelError::NotFound)?;
        if app.state != InstallState::UpdateAvailable && app.state != InstallState::Installed {
            return Err(KernelError::InvalidArgument);
        }
        app.installed_version = app.version.clone();
        app.state = InstallState::Installed;
        state.total_updates += 1;
        Ok(())
    })
}

/// Set the store version for an app (simulates new version published).
pub fn publish_version(app_id: u32, new_version: &str) -> KernelResult<()> {
    with_state(|state| {
        let app = state.apps.iter_mut().find(|a| a.id == app_id)
            .ok_or(KernelError::NotFound)?;
        app.version = String::from(new_version);
        Ok(())
    })
}

/// Search apps by name.
pub fn search(query: &str) -> Vec<StoreApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let q = query.to_ascii_lowercase();
        s.apps.iter()
            .filter(|a| a.name.to_ascii_lowercase().contains(&q)
                || a.description.to_ascii_lowercase().contains(&q))
            .cloned()
            .collect()
    })
}

/// List apps by category.
pub fn list_by_category(category: AppCategory) -> Vec<StoreApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.apps.iter().filter(|a| a.category == category).cloned().collect()
    })
}

/// List installed apps.
pub fn list_installed() -> Vec<StoreApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.apps.iter().filter(|a| a.state == InstallState::Installed || a.state == InstallState::UpdateAvailable).cloned().collect()
    })
}

/// List all apps.
pub fn list_apps() -> Vec<StoreApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.apps.clone())
}

/// Get app by ID.
pub fn get_app(id: u32) -> KernelResult<StoreApp> {
    with_state(|state| {
        state.apps.iter().find(|a| a.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Add a review.
pub fn add_review(app_id: u32, user: &str, rating: u8, comment: &str) -> KernelResult<u32> {
    with_state(|state| {
        if !state.apps.iter().any(|a| a.id == app_id) {
            return Err(KernelError::NotFound);
        }
        if state.reviews.len() >= MAX_REVIEWS {
            state.reviews.remove(0);
        }
        let r = rating.clamp(1, 5);
        let id = state.next_review_id;
        state.next_review_id += 1;
        state.reviews.push(AppReview {
            id, app_id, user: String::from(user),
            rating: r, comment: String::from(comment),
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        // Update app rating (simple average).
        let app_reviews: Vec<&AppReview> = state.reviews.iter().filter(|rev| rev.app_id == app_id).collect();
        let total: u32 = app_reviews.iter().map(|rev| u32::from(rev.rating)).sum();
        let count = app_reviews.len() as u32;
        if count > 0 {
            if let Some(app) = state.apps.iter_mut().find(|a| a.id == app_id) {
                app.rating = (total * 100) / count;
            }
        }
        Ok(id)
    })
}

/// List reviews for an app.
pub fn list_reviews(app_id: u32) -> Vec<AppReview> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.reviews.iter().filter(|r| r.app_id == app_id).cloned().collect()
    })
}

/// Remove an app from catalog.
pub fn remove_app(app_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.apps.iter().position(|a| a.id == app_id)
            .ok_or(KernelError::NotFound)?;
        state.apps.remove(pos);
        state.reviews.retain(|r| r.app_id != app_id);
        Ok(())
    })
}

/// Statistics: (app_count, installed_count, total_installs, total_updates, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let installed = s.apps.iter().filter(|a| a.state == InstallState::Installed || a.state == InstallState::UpdateAvailable).count();
            (s.apps.len(), installed, s.total_installs, s.total_updates, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("appstore::self_test() — running tests...");
    init_defaults();

    // 1: Default catalog.
    let apps = list_apps();
    assert_eq!(apps.len(), 3);
    crate::serial_println!("  [1/10] default catalog: OK");

    // 2: Search.
    let results = search("editor");
    assert!(!results.is_empty());
    assert!(results[0].name.contains("Editor"));
    crate::serial_println!("  [2/10] search: OK");

    // 3: Install.
    install(1).expect("install");
    let app = get_app(1).expect("get");
    assert_eq!(app.state, InstallState::Installed);
    crate::serial_println!("  [3/10] install: OK");

    // 4: Already installed error.
    let result = install(1);
    assert!(result.is_err());
    crate::serial_println!("  [4/10] duplicate install: OK");

    // 5: List installed.
    let installed = list_installed();
    assert_eq!(installed.len(), 1);
    crate::serial_println!("  [5/10] list installed: OK");

    // 6: Publish new version and check updates.
    publish_version(1, "2.2.0").expect("publish");
    let updates = check_updates().expect("check");
    assert!(updates.contains(&1));
    let app = get_app(1).expect("get2");
    assert_eq!(app.state, InstallState::UpdateAvailable);
    crate::serial_println!("  [6/10] check updates: OK");

    // 7: Update.
    update_app(1).expect("update");
    let app = get_app(1).expect("get3");
    assert_eq!(app.state, InstallState::Installed);
    assert_eq!(app.installed_version, "2.2.0");
    crate::serial_println!("  [7/10] update: OK");

    // 8: Add review.
    let rid = add_review(1, "user1", 5, "Great editor!").expect("review");
    assert!(rid > 0);
    let reviews = list_reviews(1);
    assert_eq!(reviews.len(), 1);
    crate::serial_println!("  [8/10] review: OK");

    // 9: Uninstall.
    uninstall(1).expect("uninstall");
    let app = get_app(1).expect("get4");
    assert_eq!(app.state, InstallState::Available);
    crate::serial_println!("  [9/10] uninstall: OK");

    // 10: Stats.
    let (count, installed, installs, updates, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(installed, 0);
    assert_eq!(installs, 1);
    assert_eq!(updates, 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("appstore::self_test() — all 10 tests passed");
}
