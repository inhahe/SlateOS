//! Package manager — software installation, removal, and dependency resolution.
//!
//! Manages installed packages with version tracking, dependency resolution,
//! repository management, and upgrade operations. Provides the UI-facing
//! API for the software center and command-line package management.
//!
//! ## Architecture
//!
//! ```text
//! Software Center / CLI
//!   → pkgmgr::install(name) / remove(name) / upgrade()
//!
//! System updates
//!   → pkgmgr::check_updates() → list of available upgrades
//!   → updatemgr triggers pkgmgr::upgrade_all()
//!
//! Integration:
//!   → updatemgr (update lifecycle)
//!   → installer (initial package seeding)
//!   → appregistry (installed app metadata)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Package status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgStatus {
    /// Installed and up to date.
    Installed,
    /// Installed but upgrade available.
    Upgradeable,
    /// Available in repo but not installed.
    Available,
    /// Being installed / upgraded.
    Installing,
    /// Marked for removal.
    Removing,
    /// Installation failed.
    Failed,
}

impl PkgStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Upgradeable => "Upgrade Available",
            Self::Available => "Available",
            Self::Installing => "Installing...",
            Self::Removing => "Removing...",
            Self::Failed => "Failed",
        }
    }
}

/// Package priority/section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgSection {
    System,
    Libraries,
    Development,
    Editors,
    Graphics,
    Multimedia,
    Network,
    Games,
    Utilities,
    Fonts,
    Other,
}

impl PkgSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Libraries => "Libraries",
            Self::Development => "Development",
            Self::Editors => "Editors",
            Self::Graphics => "Graphics",
            Self::Multimedia => "Multimedia",
            Self::Network => "Network",
            Self::Games => "Games",
            Self::Utilities => "Utilities",
            Self::Fonts => "Fonts",
            Self::Other => "Other",
        }
    }
}

/// A package entry.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name.
    pub name: String,
    /// Installed version (empty if not installed).
    pub version: String,
    /// Available version (from repo).
    pub available_version: String,
    /// One-line description.
    pub description: String,
    /// Section.
    pub section: PkgSection,
    /// Status.
    pub status: PkgStatus,
    /// Installed size in bytes.
    pub installed_size: u64,
    /// Download size in bytes.
    pub download_size: u64,
    /// Dependencies (package names).
    pub depends: Vec<String>,
    /// Reverse dependencies (packages that depend on this).
    pub rdepends: Vec<String>,
    /// Repository source name.
    pub repo: String,
    /// Auto-installed (pulled as dependency).
    pub auto_installed: bool,
}

/// A package repository.
#[derive(Debug, Clone)]
pub struct Repository {
    /// Repository name.
    pub name: String,
    /// URL.
    pub url: String,
    /// Whether enabled.
    pub enabled: bool,
    /// Number of packages in this repo.
    pub package_count: usize,
}

const MAX_PACKAGES: usize = 10_000;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    packages: Vec<Package>,
    repos: Vec<Repository>,
    total_installed: u64,
    total_removed: u64,
    total_upgraded: u64,
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

/// Initialise the package manager with an EMPTY database.
///
/// We never fabricate an installed-package list or repository set. The previous
/// implementation seeded phantom "Installed" packages (kernel/coreutils/libc)
/// with invented sizes plus two repositories pointing at placeholder URLs
/// (`packages.example.os`), which `/proc` and the `pkgmgr` shell command
/// surfaced as a real package database. Neither corresponds to anything the
/// system actually tracks.
///
/// The real installed set is owned by the content-addressed package store
/// (`pkg/`), and repositories are configuration supplied by the installer or
/// the user. So the database starts empty: packages are added via `install()`
/// and repositories via `add_repo()`.
///
/// DEFERRED PROPER FIX: read the installed-package set through the real
/// content-addressed store / generations database once `pkg/` exposes a query
/// API, and load repository configuration from its on-disk config.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(State {
        packages: Vec::new(),
        repos: Vec::new(),
        total_installed: 0,
        total_removed: 0,
        total_upgraded: 0,
        ops: 0,
    });
}

/// Install a package.
pub fn install(name: &str, version: &str, description: &str, section: PkgSection, size: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.packages.iter().any(|p| p.name == name && p.status == PkgStatus::Installed) {
            return Err(KernelError::AlreadyExists);
        }
        if state.packages.len() >= MAX_PACKAGES {
            return Err(KernelError::ResourceExhausted);
        }

        // Update existing entry or create new.
        if let Some(pkg) = state.packages.iter_mut().find(|p| p.name == name) {
            pkg.version = String::from(version);
            pkg.status = PkgStatus::Installed;
            pkg.installed_size = size;
        } else {
            state.packages.push(Package {
                name: String::from(name),
                version: String::from(version),
                available_version: String::from(version),
                description: String::from(description),
                section,
                status: PkgStatus::Installed,
                installed_size: size,
                download_size: size / 2,
                depends: Vec::new(),
                rdepends: Vec::new(),
                repo: String::from("main"),
                auto_installed: false,
            });
        }

        state.total_installed += 1;
        Ok(())
    })
}

/// Remove a package.
pub fn remove(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let pkg = state.packages.iter_mut().find(|p| p.name == name)
            .ok_or(KernelError::NotFound)?;
        if pkg.status != PkgStatus::Installed && pkg.status != PkgStatus::Upgradeable {
            return Err(KernelError::InvalidArgument);
        }

        // Check reverse dependencies.
        if !pkg.rdepends.is_empty() {
            // In a real implementation, we'd check if rdepends are still installed.
            // For now, just warn but allow removal.
        }

        pkg.status = PkgStatus::Available;
        pkg.version = String::new();
        pkg.installed_size = 0;
        state.total_removed += 1;
        Ok(())
    })
}

/// Mark a package as having an available upgrade.
pub fn mark_upgradeable(name: &str, new_version: &str) -> KernelResult<()> {
    with_state(|state| {
        let pkg = state.packages.iter_mut().find(|p| p.name == name)
            .ok_or(KernelError::NotFound)?;
        if pkg.status != PkgStatus::Installed {
            return Err(KernelError::InvalidArgument);
        }
        pkg.available_version = String::from(new_version);
        pkg.status = PkgStatus::Upgradeable;
        Ok(())
    })
}

/// Upgrade a package to its available version.
pub fn upgrade(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let pkg = state.packages.iter_mut().find(|p| p.name == name)
            .ok_or(KernelError::NotFound)?;
        if pkg.status != PkgStatus::Upgradeable {
            return Err(KernelError::InvalidArgument);
        }
        pkg.version = pkg.available_version.clone();
        pkg.status = PkgStatus::Installed;
        state.total_upgraded += 1;
        Ok(())
    })
}

/// Upgrade all upgradeable packages.
pub fn upgrade_all() -> KernelResult<usize> {
    with_state(|state| {
        let mut count = 0usize;
        for pkg in &mut state.packages {
            if pkg.status == PkgStatus::Upgradeable {
                pkg.version = pkg.available_version.clone();
                pkg.status = PkgStatus::Installed;
                state.total_upgraded += 1;
                count += 1;
            }
        }
        Ok(count)
    })
}

/// Search packages by name (substring match).
pub fn search(query: &str) -> Vec<Package> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.packages.iter()
            .filter(|p| p.name.contains(query) || p.description.contains(query))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// Get package info.
pub fn get_package(name: &str) -> KernelResult<Package> {
    with_state(|state| {
        state.packages.iter().find(|p| p.name == name)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List installed packages.
pub fn list_installed() -> Vec<Package> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.packages.iter()
            .filter(|p| p.status == PkgStatus::Installed || p.status == PkgStatus::Upgradeable)
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// List upgradeable packages.
pub fn list_upgradeable() -> Vec<Package> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.packages.iter()
            .filter(|p| p.status == PkgStatus::Upgradeable)
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// Add a repository.
pub fn add_repo(name: &str, url: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.repos.iter().any(|r| r.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        state.repos.push(Repository {
            name: String::from(name),
            url: String::from(url),
            enabled: true,
            package_count: 0,
        });
        Ok(())
    })
}

/// Remove a repository.
pub fn remove_repo(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.repos.iter().position(|r| r.name == name)
            .ok_or(KernelError::NotFound)?;
        state.repos.remove(pos);
        Ok(())
    })
}

/// List repositories.
pub fn list_repos() -> Vec<Repository> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.repos.clone(),
        None => Vec::new(),
    }
}

/// Total installed size in bytes.
pub fn total_installed_size() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.packages.iter()
            .filter(|p| p.status == PkgStatus::Installed || p.status == PkgStatus::Upgradeable)
            .map(|p| p.installed_size)
            .sum(),
        None => 0,
    }
}

/// Statistics: (installed_count, available_count, upgradeable_count, repo_count, ops).
pub fn stats() -> (usize, usize, usize, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let installed = s.packages.iter().filter(|p| p.status == PkgStatus::Installed || p.status == PkgStatus::Upgradeable).count();
            let available = s.packages.iter().filter(|p| p.status == PkgStatus::Available).count();
            let upgradeable = s.packages.iter().filter(|p| p.status == PkgStatus::Upgradeable).count();
            (installed, available, upgradeable, s.repos.len(), s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pkgmgr::self_test() — running tests...");

    // Residue-free: start from a known-empty database.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: the database starts EMPTY — no fabricated packages or repos.
    assert_eq!(list_installed().len(), 0);
    assert_eq!(list_repos().len(), 0);
    let (i0, _a0, _u0, r0, _o0) = stats();
    assert_eq!(i0, 0);
    assert_eq!(r0, 0);

    // Build deterministic fixtures via the real entry points: two repos and the
    // base packages the old fabricated default invented (now installed
    // explicitly inside the test rather than at boot).
    add_repo("main", "https://packages.example.os/main").expect("add main repo");
    add_repo("community", "https://packages.example.os/community").expect("add community repo");
    install("kernel", "0.1.0", "OS kernel", PkgSection::System, 4 * 1024 * 1024).expect("install kernel");
    install("coreutils", "1.0.0", "Core system utilities", PkgSection::System, 2 * 1024 * 1024).expect("install coreutils");
    install("libc", "1.0.0", "C standard library", PkgSection::Libraries, 8 * 1024 * 1024).expect("install libc");
    assert_eq!(list_installed().len(), 3);
    crate::serial_println!("  [1/11] empty db + base fixtures: OK");

    // Test 2: Install new package.
    install("editor", "2.0.0", "Text editor", PkgSection::Editors, 5 * 1024 * 1024).expect("install");
    let pkg = get_package("editor").expect("get editor");
    assert_eq!(pkg.status, PkgStatus::Installed);
    crate::serial_println!("  [2/11] install package: OK");

    // Test 3: Duplicate install fails.
    let result = install("editor", "2.0.0", "Text editor", PkgSection::Editors, 5 * 1024 * 1024);
    assert!(result.is_err());
    crate::serial_println!("  [3/11] duplicate rejected: OK");

    // Test 4: Search.
    let results = search("editor");
    assert!(!results.is_empty());
    crate::serial_println!("  [4/11] search: OK");

    // Test 5: Mark upgradeable.
    mark_upgradeable("editor", "2.1.0").expect("mark upgrade");
    let pkg = get_package("editor").expect("get after mark");
    assert_eq!(pkg.status, PkgStatus::Upgradeable);
    assert_eq!(pkg.available_version, "2.1.0");
    crate::serial_println!("  [5/11] mark upgradeable: OK");

    // Test 6: Upgrade.
    upgrade("editor").expect("upgrade");
    let pkg = get_package("editor").expect("get after upgrade");
    assert_eq!(pkg.status, PkgStatus::Installed);
    assert_eq!(pkg.version, "2.1.0");
    crate::serial_println!("  [6/11] upgrade: OK");

    // Test 7: Remove package.
    remove("editor").expect("remove");
    let pkg = get_package("editor").expect("get after remove");
    assert_eq!(pkg.status, PkgStatus::Available);
    crate::serial_println!("  [7/11] remove package: OK");

    // Test 8: Add repository.
    add_repo("testing", "https://packages.example.os/testing").expect("add repo");
    let repos = list_repos();
    assert_eq!(repos.len(), 3);
    crate::serial_println!("  [8/11] add repo: OK");

    // Test 9: Remove repository.
    remove_repo("testing").expect("remove repo");
    let repos = list_repos();
    assert_eq!(repos.len(), 2);
    crate::serial_println!("  [9/11] remove repo: OK");

    // Test 10: Total installed size.
    let size = total_installed_size();
    assert!(size > 0);
    crate::serial_println!("  [10/11] installed size: OK");

    // Test 11: Stats.
    let (installed, available, upgradeable, repos, ops) = stats();
    assert!(installed >= 3);
    assert!(available >= 1);
    assert_eq!(upgradeable, 0);
    assert_eq!(repos, 2);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Residue-free: leave no fixtures behind.
    *STATE.lock() = None;

    crate::serial_println!("pkgmgr::self_test() — all 11 tests passed");
}
