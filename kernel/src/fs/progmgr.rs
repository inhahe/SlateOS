//! Program management — per-program settings, capabilities, and lifecycle.
//!
//! Provides the data model for the "Programs" settings panel described in
//! design.txt lines 1289-1300: per-program priority, capability grants,
//! notification preferences, data directory tracking, uninstall records,
//! and program snapshot/rollback support.
//!
//! ## Design Reference
//!
//! design.txt lines 1289-1300:
//! - set priority
//! - set capabilities (with per-program grants)
//! - uninstall (keep program files? keep settings?)
//! - recompile with specified parameters
//! - notification config (sound, display, per-notification-type)
//! - standard settings/data directories per program
//! - wipe program data
//! - take/rollback program snapshots
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Programs
//!   → progmgr::set_priority(app_id, level)
//!   → progmgr::grant_capability(app_id, cap)
//!   → progmgr::set_notification(app_id, notif, config)
//!   → progmgr::uninstall(app_id, opts)
//!
//! Program install
//!   → progmgr::register(ProgramEntry { ... })
//!   → tracks directories, capabilities needed
//!
//! Snapshot
//!   → progmgr::create_snapshot(app_id, what)
//!   → progmgr::rollback(app_id, snapshot_id)
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

/// Process scheduling priority level for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityLevel {
    /// Lowest — background tasks.
    Idle,
    /// Below normal.
    BelowNormal,
    /// Default.
    Normal,
    /// Above normal.
    AboveNormal,
    /// Highest user priority.
    High,
    /// Real-time — use with caution.
    Realtime,
}

/// A capability that can be granted to a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgCapability {
    /// Access the network.
    Network,
    /// Access the filesystem beyond its own directory.
    FilesystemFull,
    /// Access removable media.
    RemovableMedia,
    /// Access audio devices.
    Audio,
    /// Access video/camera devices.
    Camera,
    /// Access GPS/location.
    Location,
    /// Show notifications.
    Notifications,
    /// Run at startup.
    Autostart,
    /// Modify system settings.
    SystemSettings,
    /// Access other programs' data.
    CrossAppData,
    /// Run in the background.
    Background,
    /// Hardware access (USB, serial, etc.).
    Hardware,
    /// Install additional software.
    InstallSoftware,
    /// Manage user accounts.
    UserManagement,
    /// Access clipboard.
    Clipboard,
}

/// What to include in a program snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotScope {
    /// Only program settings.
    SettingsOnly,
    /// Only program data.
    DataOnly,
    /// Both settings and data.
    Full,
}

/// Per-notification-type configuration.
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// Notification type name (e.g., "message", "update", "error").
    pub name: String,
    /// Whether to show in the notification pane.
    pub show: bool,
    /// Sound file path (empty = default, "none" = silent).
    pub sound: String,
    /// Whether to show a popup banner.
    pub banner: bool,
    /// Whether to show on lock screen.
    pub lock_screen: bool,
}

/// A program snapshot for rollback support.
#[derive(Debug, Clone)]
pub struct ProgramSnapshot {
    /// Unique snapshot ID.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Parent snapshot ID (0 = root, forms a tree like VM snapshots).
    pub parent_id: u64,
    /// What was captured.
    pub scope: SnapshotScope,
    /// Creation timestamp (ns).
    pub created_ns: u64,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Compilation/build record for recompilable programs.
#[derive(Debug, Clone)]
pub struct BuildRecord {
    /// Source directory path.
    pub source_dir: String,
    /// Hash of source tree at last compile.
    pub source_hash: u64,
    /// Compile parameters (flags, options).
    pub parameters: String,
    /// Last compile timestamp (ns).
    pub last_compile_ns: u64,
    /// Whether source has changed since last compile.
    pub source_changed: bool,
}

/// Uninstall options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOption {
    /// Remove everything.
    Full,
    /// Keep program files (executable, resources).
    KeepFiles,
    /// Keep settings only.
    KeepSettings,
    /// Keep both files and settings.
    KeepAll,
}

/// A registered program with its management settings.
#[derive(Debug, Clone)]
pub struct ProgramEntry {
    /// Application ID (matches appregistry).
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// Install directory.
    pub install_dir: String,
    /// Settings directory (standard subdirectory).
    pub settings_dir: String,
    /// Data directory (standard subdirectory).
    pub data_dir: String,
    /// Scheduling priority.
    pub priority: PriorityLevel,
    /// Granted capabilities.
    pub capabilities: Vec<ProgCapability>,
    /// Per-notification-type configs.
    pub notifications: Vec<NotificationConfig>,
    /// Install timestamp (ns).
    pub installed_ns: u64,
    /// Installed size in bytes.
    pub size_bytes: u64,
    /// Whether the program can be recompiled.
    pub compilable: bool,
    /// Build record (if compilable).
    pub build: Option<BuildRecord>,
    /// Program snapshots (tree structure via parent_id).
    pub snapshots: Vec<ProgramSnapshot>,
    /// Whether currently installed (false = uninstalled but data kept).
    pub installed: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    programs: Vec<ProgramEntry>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    programs: Vec::new(),
    changes: 0,
});

static NEXT_SNAP_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Register a new program for management.
pub fn register(
    app_id: &str,
    name: &str,
    version: &str,
    install_dir: &str,
    size_bytes: u64,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.programs.len() >= 4096 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.programs.iter().any(|p| p.app_id == app_id) {
        return Err(KernelError::AlreadyExists);
    }
    let settings_dir = {
        use alloc::format;
        format!("{}/settings", install_dir)
    };
    let data_dir = {
        use alloc::format;
        format!("{}/data", install_dir)
    };
    let now = crate::hpet::elapsed_ns();
    state.programs.push(ProgramEntry {
        app_id: String::from(app_id),
        name: String::from(name),
        version: String::from(version),
        install_dir: String::from(install_dir),
        settings_dir,
        data_dir,
        priority: PriorityLevel::Normal,
        capabilities: Vec::new(),
        notifications: Vec::new(),
        installed_ns: now,
        size_bytes,
        compilable: false,
        build: None,
        snapshots: Vec::new(),
        installed: true,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Unregister / uninstall a program.
pub fn uninstall(app_id: &str, option: UninstallOption) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    match option {
        UninstallOption::Full => {
            // Mark for full removal — caller handles actual file deletion.
            let idx = state
                .programs
                .iter()
                .position(|p| p.app_id == app_id)
                .ok_or(KernelError::NotFound)?;
            state.programs.remove(idx);
        }
        UninstallOption::KeepFiles | UninstallOption::KeepSettings | UninstallOption::KeepAll => {
            prog.installed = false;
        }
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a program entry by app_id.
pub fn get_program(app_id: &str) -> KernelResult<ProgramEntry> {
    let state = STATE.lock();
    state
        .programs
        .iter()
        .find(|p| p.app_id == app_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all registered programs.
pub fn list_programs() -> Vec<ProgramEntry> {
    STATE.lock().programs.clone()
}

/// List only currently installed programs.
pub fn list_installed() -> Vec<ProgramEntry> {
    STATE
        .lock()
        .programs
        .iter()
        .filter(|p| p.installed)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Set scheduling priority for a program.
pub fn set_priority(app_id: &str, priority: PriorityLevel) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.priority = priority;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// Grant a capability to a program.
pub fn grant_capability(app_id: &str, cap: ProgCapability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    if !prog.capabilities.contains(&cap) {
        if prog.capabilities.len() >= 64 {
            return Err(KernelError::ResourceExhausted);
        }
        prog.capabilities.push(cap);
        state.changes += 1;
    }
    Ok(())
}

/// Revoke a capability from a program.
pub fn revoke_capability(app_id: &str, cap: ProgCapability) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.capabilities.retain(|c| *c != cap);
    state.changes += 1;
    Ok(())
}

/// Check if a program has a specific capability.
pub fn has_capability(app_id: &str, cap: ProgCapability) -> bool {
    let state = STATE.lock();
    state
        .programs
        .iter()
        .find(|p| p.app_id == app_id)
        .is_some_and(|p| p.capabilities.contains(&cap))
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/// Register a notification type for a program.
pub fn add_notification(app_id: &str, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    if prog.notifications.iter().any(|n| n.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    if prog.notifications.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    prog.notifications.push(NotificationConfig {
        name: String::from(name),
        show: true,
        sound: String::new(), // default sound
        banner: true,
        lock_screen: false,
    });
    state.changes += 1;
    Ok(())
}

/// Remove a notification type from a program.
pub fn remove_notification(app_id: &str, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let before = prog.notifications.len();
    prog.notifications.retain(|n| n.name != name);
    if prog.notifications.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// Configure a notification type for a program.
pub fn set_notification_config(
    app_id: &str,
    name: &str,
    show: Option<bool>,
    sound: Option<&str>,
    banner: Option<bool>,
    lock_screen: Option<bool>,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let notif = prog
        .notifications
        .iter_mut()
        .find(|n| n.name == name)
        .ok_or(KernelError::NotFound)?;
    if let Some(v) = show {
        notif.show = v;
    }
    if let Some(s) = sound {
        notif.sound = String::from(s);
    }
    if let Some(v) = banner {
        notif.banner = v;
    }
    if let Some(v) = lock_screen {
        notif.lock_screen = v;
    }
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data management
// ---------------------------------------------------------------------------

/// Set custom settings directory for a program.
pub fn set_settings_dir(app_id: &str, dir: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.settings_dir = String::from(dir);
    state.changes += 1;
    Ok(())
}

/// Set custom data directory for a program.
pub fn set_data_dir(app_id: &str, dir: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.data_dir = String::from(dir);
    state.changes += 1;
    Ok(())
}

/// Mark program data for wipe (caller handles actual deletion).
/// Returns the data directory that should be cleared.
pub fn wipe_data(app_id: &str) -> KernelResult<String> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let dir = prog.data_dir.clone();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(dir)
}

/// Mark program settings for wipe (caller handles actual deletion).
/// Returns the settings directory that should be cleared.
pub fn wipe_settings(app_id: &str) -> KernelResult<String> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let dir = prog.settings_dir.clone();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Build / recompile
// ---------------------------------------------------------------------------

/// Set a program as compilable with a source directory.
pub fn set_compilable(app_id: &str, source_dir: &str, parameters: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.compilable = true;
    prog.build = Some(BuildRecord {
        source_dir: String::from(source_dir),
        source_hash: 0,
        parameters: String::from(parameters),
        last_compile_ns: 0,
        source_changed: true,
    });
    state.changes += 1;
    Ok(())
}

/// Record a compilation event.
pub fn record_compile(app_id: &str, source_hash: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let build = prog.build.as_mut().ok_or(KernelError::NotSupported)?;
    build.source_hash = source_hash;
    build.last_compile_ns = crate::hpet::elapsed_ns();
    build.source_changed = false;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Mark source as changed (e.g., after filesystem watch detects change).
pub fn mark_source_changed(app_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let build = prog.build.as_mut().ok_or(KernelError::NotSupported)?;
    build.source_changed = true;
    state.changes += 1;
    Ok(())
}

/// Update build parameters.
pub fn set_build_params(app_id: &str, params: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let build = prog.build.as_mut().ok_or(KernelError::NotSupported)?;
    build.parameters = String::from(params);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// Create a snapshot of a program's data/settings.
pub fn create_snapshot(
    app_id: &str,
    name: &str,
    scope: SnapshotScope,
    parent_id: u64,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    if prog.snapshots.len() >= 256 {
        return Err(KernelError::ResourceExhausted);
    }
    // Verify parent exists (0 = root, no parent needed).
    if parent_id != 0 && !prog.snapshots.iter().any(|s| s.id == parent_id) {
        return Err(KernelError::NotFound);
    }
    let id = NEXT_SNAP_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    prog.snapshots.push(ProgramSnapshot {
        id,
        name: String::from(name),
        parent_id,
        scope,
        created_ns: now,
        size_bytes: prog.size_bytes, // simplified estimate
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Delete a snapshot.
pub fn delete_snapshot(app_id: &str, snapshot_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    // Check no children reference this snapshot.
    if prog.snapshots.iter().any(|s| s.parent_id == snapshot_id) {
        return Err(KernelError::NotEmpty);
    }
    let before = prog.snapshots.len();
    prog.snapshots.retain(|s| s.id != snapshot_id);
    if prog.snapshots.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Rollback to a snapshot (marks intent — caller handles actual restore).
/// Returns the snapshot details for the caller to act on.
pub fn rollback(app_id: &str, snapshot_id: u64) -> KernelResult<ProgramSnapshot> {
    let state = STATE.lock();
    let prog = state
        .programs
        .iter()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let snap = prog
        .snapshots
        .iter()
        .find(|s| s.id == snapshot_id)
        .cloned()
        .ok_or(KernelError::NotFound)?;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(snap)
}

/// List snapshots for a program.
pub fn list_snapshots(app_id: &str) -> KernelResult<Vec<ProgramSnapshot>> {
    let state = STATE.lock();
    let prog = state
        .programs
        .iter()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    Ok(prog.snapshots.clone())
}

// ---------------------------------------------------------------------------
// Version / update
// ---------------------------------------------------------------------------

/// Update program version.
pub fn set_version(app_id: &str, version: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.version = String::from(version);
    state.changes += 1;
    Ok(())
}

/// Update installed size.
pub fn set_size(app_id: &str, size_bytes: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let prog = state
        .programs
        .iter_mut()
        .find(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    prog.size_bytes = size_bytes;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Init / stats / housekeeping
// ---------------------------------------------------------------------------

/// Initialize with sample/default programs.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.programs.is_empty() {
        return;
    }
    let now = crate::hpet::elapsed_ns();

    // The four entries below are the bundled core system apps that genuinely ship
    // with the OS (File Explorer, Terminal, Text Editor, Settings). Their app_id,
    // name, version, directories, capabilities, and notification configs are a
    // legitimate compiled-in MANIFEST (config, not observation). Their `size_bytes`,
    // however, is an OBSERVATION — the size of the installed binary on disk — which
    // cannot be known without measuring the real file. The previous code fabricated
    // round numbers (2_048_000 / 512_000 / 1_024_000 / 768_000); those are seeded as
    // 0 (unknown) instead. The real value is recorded via `set_size()` once a binary
    // is actually measured. DEFERRED: have the app installer/loader measure and set
    // the true on-disk size when these apps are staged into the rootfs.

    // File explorer — always installed, core system app.
    state.programs.push(ProgramEntry {
        app_id: String::from("system.fileexplorer"),
        name: String::from("File Explorer"),
        version: String::from("1.0.0"),
        install_dir: String::from("/usr/lib/fileexplorer"),
        settings_dir: String::from("/usr/lib/fileexplorer/settings"),
        data_dir: String::from("/usr/lib/fileexplorer/data"),
        priority: PriorityLevel::Normal,
        capabilities: alloc::vec![
            ProgCapability::FilesystemFull,
            ProgCapability::RemovableMedia,
            ProgCapability::Clipboard,
            ProgCapability::Notifications,
        ],
        notifications: alloc::vec![
            NotificationConfig {
                name: String::from("transfer"),
                show: true,
                sound: String::new(),
                banner: true,
                lock_screen: false,
            },
            NotificationConfig {
                name: String::from("error"),
                show: true,
                sound: String::from("error"),
                banner: true,
                lock_screen: false,
            },
        ],
        installed_ns: now,
        size_bytes: 0, // unknown until the installed binary is measured
        compilable: false,
        build: None,
        snapshots: Vec::new(),
        installed: true,
    });

    // Terminal — system app.
    state.programs.push(ProgramEntry {
        app_id: String::from("system.terminal"),
        name: String::from("Terminal"),
        version: String::from("1.0.0"),
        install_dir: String::from("/usr/lib/terminal"),
        settings_dir: String::from("/usr/lib/terminal/settings"),
        data_dir: String::from("/usr/lib/terminal/data"),
        priority: PriorityLevel::Normal,
        capabilities: alloc::vec![
            ProgCapability::FilesystemFull,
            ProgCapability::Network,
            ProgCapability::Clipboard,
            ProgCapability::Hardware,
        ],
        notifications: Vec::new(),
        installed_ns: now,
        size_bytes: 0, // unknown until the installed binary is measured
        compilable: false,
        build: None,
        snapshots: Vec::new(),
        installed: true,
    });

    // Text editor — system app.
    state.programs.push(ProgramEntry {
        app_id: String::from("system.texteditor"),
        name: String::from("Text Editor"),
        version: String::from("1.0.0"),
        install_dir: String::from("/usr/lib/texteditor"),
        settings_dir: String::from("/usr/lib/texteditor/settings"),
        data_dir: String::from("/usr/lib/texteditor/data"),
        priority: PriorityLevel::Normal,
        capabilities: alloc::vec![
            ProgCapability::FilesystemFull,
            ProgCapability::Clipboard,
            ProgCapability::Notifications,
        ],
        notifications: alloc::vec![NotificationConfig {
            name: String::from("autosave"),
            show: true,
            sound: String::new(),
            banner: false,
            lock_screen: false,
        }],
        installed_ns: now,
        size_bytes: 0, // unknown until the installed binary is measured
        compilable: false,
        build: None,
        snapshots: Vec::new(),
        installed: true,
    });

    // Settings — system app.
    state.programs.push(ProgramEntry {
        app_id: String::from("system.settings"),
        name: String::from("Settings"),
        version: String::from("1.0.0"),
        install_dir: String::from("/usr/lib/settings"),
        settings_dir: String::from("/usr/lib/settings/settings"),
        data_dir: String::from("/usr/lib/settings/data"),
        priority: PriorityLevel::Normal,
        capabilities: alloc::vec![
            ProgCapability::SystemSettings,
            ProgCapability::Network,
            ProgCapability::Hardware,
            ProgCapability::UserManagement,
        ],
        notifications: Vec::new(),
        installed_ns: now,
        size_bytes: 0, // unknown until the installed binary is measured
        compilable: false,
        build: None,
        snapshots: Vec::new(),
        installed: true,
    });

    state.changes += 1;
}

/// Return (program_count, installed_count, total_snapshots, total_ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.programs.len();
    let installed = state.programs.iter().filter(|p| p.installed).count();
    let snaps: usize = state.programs.iter().map(|p| p.snapshots.len()).sum();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, installed, snaps, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.programs.clear();
    state.changes = 0;
    NEXT_SNAP_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: register program.
    serial_println!("progmgr::self_test 1: register program");
    register("test.app", "TestApp", "1.0", "/opt/testapp", 1024)?;
    let prog = get_program("test.app")?;
    assert_eq!(prog.name, "TestApp");
    assert_eq!(prog.priority, PriorityLevel::Normal);
    assert!(prog.installed);

    // Test 2: set priority.
    serial_println!("progmgr::self_test 2: set priority");
    set_priority("test.app", PriorityLevel::High)?;
    let prog = get_program("test.app")?;
    assert_eq!(prog.priority, PriorityLevel::High);

    // Test 3: grant/revoke capabilities.
    serial_println!("progmgr::self_test 3: capabilities");
    grant_capability("test.app", ProgCapability::Network)?;
    grant_capability("test.app", ProgCapability::Audio)?;
    assert!(has_capability("test.app", ProgCapability::Network));
    revoke_capability("test.app", ProgCapability::Network)?;
    assert!(!has_capability("test.app", ProgCapability::Network));
    assert!(has_capability("test.app", ProgCapability::Audio));

    // Test 4: notifications.
    serial_println!("progmgr::self_test 4: notifications");
    add_notification("test.app", "alert")?;
    add_notification("test.app", "update")?;
    set_notification_config("test.app", "alert", Some(false), Some("ding.wav"), None, None)?;
    let prog = get_program("test.app")?;
    assert_eq!(prog.notifications.len(), 2);
    let alert = prog.notifications.iter().find(|n| n.name == "alert").expect("alert notif");
    assert!(!alert.show);
    assert_eq!(alert.sound, "ding.wav");
    remove_notification("test.app", "update")?;
    let prog = get_program("test.app")?;
    assert_eq!(prog.notifications.len(), 1);

    // Test 5: snapshots (tree structure).
    serial_println!("progmgr::self_test 5: snapshots");
    let s1 = create_snapshot("test.app", "initial", SnapshotScope::Full, 0)?;
    let s2 = create_snapshot("test.app", "after-config", SnapshotScope::SettingsOnly, s1)?;
    let snaps = list_snapshots("test.app")?;
    assert_eq!(snaps.len(), 2);
    // Cannot delete parent with children.
    assert!(delete_snapshot("test.app", s1).is_err());
    // Can delete leaf.
    delete_snapshot("test.app", s2)?;
    delete_snapshot("test.app", s1)?;
    let snaps = list_snapshots("test.app")?;
    assert!(snaps.is_empty());

    // Test 6: build records.
    serial_println!("progmgr::self_test 6: build records");
    set_compilable("test.app", "/src/testapp", "--release")?;
    let prog = get_program("test.app")?;
    assert!(prog.compilable);
    let build = prog.build.as_ref().expect("build record");
    assert!(build.source_changed);
    record_compile("test.app", 0xDEAD_BEEF)?;
    let prog = get_program("test.app")?;
    let build = prog.build.as_ref().expect("build record");
    assert!(!build.source_changed);
    assert_eq!(build.source_hash, 0xDEAD_BEEF);
    mark_source_changed("test.app")?;
    let prog = get_program("test.app")?;
    assert!(prog.build.as_ref().expect("build").source_changed);

    // Test 7: uninstall.
    serial_println!("progmgr::self_test 7: uninstall");
    register("test.app2", "App2", "2.0", "/opt/app2", 512)?;
    uninstall("test.app2", UninstallOption::KeepSettings)?;
    let prog = get_program("test.app2")?;
    assert!(!prog.installed);
    uninstall("test.app", UninstallOption::Full)?;
    assert!(get_program("test.app").is_err());

    clear_all();
    serial_println!("progmgr::self_test: all 7 tests passed");
    Ok(())
}
