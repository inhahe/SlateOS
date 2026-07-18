//! System update manager — package updates, security patches, versioning.
//!
//! Tracks available system updates, manages update channels, and
//! controls automatic update behaviour.  Integrates with the package
//! manager for actual installation.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → System → Updates
//!   → updatemgr::set_auto_update() / check_updates()
//!
//! Background service
//!   → updatemgr::check_updates() periodic polling
//!   → updatemgr::install_update(id)
//!
//! Integration:
//!   → notifcenter (update available notifications)
//!   → power (defer updates on battery)
//!   → tasksched (scheduled update checks)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_UPDATES: usize = 128;
const MAX_HISTORY: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Update severity / priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSeverity {
    /// Critical security fix — install ASAP.
    Critical,
    /// Important security or stability fix.
    Important,
    /// Recommended improvement.
    Recommended,
    /// Optional feature or driver update.
    Optional,
}

impl UpdateSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::Important => "Important",
            Self::Recommended => "Recommended",
            Self::Optional => "Optional",
        }
    }
}

/// Update type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateType {
    /// Security patch.
    Security,
    /// Bug fix.
    BugFix,
    /// Feature update.
    Feature,
    /// Driver update.
    Driver,
    /// System/kernel update.
    System,
    /// Application update.
    Application,
}

impl UpdateType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Security => "Security",
            Self::BugFix => "Bug Fix",
            Self::Feature => "Feature",
            Self::Driver => "Driver",
            Self::System => "System",
            Self::Application => "Application",
        }
    }
}

/// Update status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Available for download.
    Available,
    /// Currently downloading.
    Downloading,
    /// Downloaded, ready to install.
    Downloaded,
    /// Currently installing.
    Installing,
    /// Successfully installed.
    Installed,
    /// Installation failed.
    Failed,
    /// Deferred by user.
    Deferred,
}

impl UpdateStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Downloading => "Downloading",
            Self::Downloaded => "Downloaded",
            Self::Installing => "Installing",
            Self::Installed => "Installed",
            Self::Failed => "Failed",
            Self::Deferred => "Deferred",
        }
    }
}

/// Update channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChannel {
    /// Stable — production quality.
    Stable,
    /// Beta — pre-release testing.
    Beta,
    /// Nightly — bleeding edge.
    Nightly,
}

impl UpdateChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Nightly => "Nightly",
        }
    }
}

/// An available or installed update.
#[derive(Debug, Clone)]
pub struct Update {
    /// Unique update ID.
    pub id: u64,
    /// Package name.
    pub package: String,
    /// Current version.
    pub from_version: String,
    /// New version.
    pub to_version: String,
    /// Update type.
    pub update_type: UpdateType,
    /// Severity.
    pub severity: UpdateSeverity,
    /// Status.
    pub status: UpdateStatus,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Description.
    pub description: String,
    /// Requires restart.
    pub requires_restart: bool,
    /// Discovered timestamp (ns).
    pub discovered_ns: u64,
    /// Installed timestamp (ns, 0 if not installed).
    pub installed_ns: u64,
    /// Download progress (0-100).
    pub progress_pct: u8,
}

/// Update configuration.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// Automatic update check enabled.
    pub auto_check: bool,
    /// Check interval (hours).
    pub check_interval_hours: u32,
    /// Auto-download updates.
    pub auto_download: bool,
    /// Auto-install non-critical updates.
    pub auto_install: bool,
    /// Auto-install security updates.
    pub auto_install_security: bool,
    /// Update channel.
    pub channel: UpdateChannel,
    /// Defer updates while on battery.
    pub defer_on_battery: bool,
    /// Active hours start (hour, 0-23).
    pub active_hours_start: u8,
    /// Active hours end (hour, 0-23).
    pub active_hours_end: u8,
    /// Metered connection — defer large updates.
    pub defer_on_metered: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct UpdateState {
    config: UpdateConfig,
    updates: Vec<Update>,
    history: Vec<Update>,
    next_id: u64,
    last_check_ns: u64,
    os_version: String,
    os_build: u64,
    ops: u64,
}

static STATE: Mutex<Option<UpdateState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut UpdateState) -> KernelResult<R>,
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

/// Initialize the update manager.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(UpdateState {
        config: UpdateConfig {
            auto_check: true,
            check_interval_hours: 12,
            auto_download: true,
            auto_install: false,
            auto_install_security: true,
            channel: UpdateChannel::Stable,
            defer_on_battery: true,
            active_hours_start: 8,
            active_hours_end: 22,
            defer_on_metered: true,
        },
        updates: Vec::new(),
        history: Vec::new(),
        next_id: 1,
        last_check_ns: 0,
        os_version: String::from("0.1.0"),
        os_build: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get the update configuration.
pub fn get_config() -> KernelResult<UpdateConfig> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    Ok(state.config.clone())
}

/// Set auto-check.
pub fn set_auto_check(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_check = enabled; Ok(()) })
}

/// Set check interval (hours).
pub fn set_check_interval(hours: u32) -> KernelResult<()> {
    if hours == 0 || hours > 168 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| { state.config.check_interval_hours = hours; Ok(()) })
}

/// Set auto-download.
pub fn set_auto_download(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_download = enabled; Ok(()) })
}

/// Set auto-install.
pub fn set_auto_install(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_install = enabled; Ok(()) })
}

/// Set auto-install for security updates.
pub fn set_auto_install_security(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_install_security = enabled; Ok(()) })
}

/// Set update channel.
pub fn set_channel(channel: UpdateChannel) -> KernelResult<()> {
    with_state(|state| { state.config.channel = channel; Ok(()) })
}

/// Set defer on battery.
pub fn set_defer_on_battery(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.defer_on_battery = enabled; Ok(()) })
}

/// Set active hours.
pub fn set_active_hours(start: u8, end: u8) -> KernelResult<()> {
    if start > 23 || end > 23 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.active_hours_start = start;
        state.config.active_hours_end = end;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Update lifecycle
// ---------------------------------------------------------------------------

/// Simulate discovering an available update.
pub fn add_available_update(
    package: &str,
    from_ver: &str,
    to_ver: &str,
    update_type: UpdateType,
    severity: UpdateSeverity,
    size: u64,
    description: &str,
    requires_restart: bool,
) -> KernelResult<u64> {
    with_state(|state| {
        if state.updates.len() >= MAX_UPDATES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.updates.push(Update {
            id,
            package: String::from(package),
            from_version: String::from(from_ver),
            to_version: String::from(to_ver),
            update_type,
            severity,
            status: UpdateStatus::Available,
            size_bytes: size,
            description: String::from(description),
            requires_restart,
            discovered_ns: crate::hpet::elapsed_ns(),
            installed_ns: 0,
            progress_pct: 0,
        });
        Ok(id)
    })
}

/// Start downloading an update.
pub fn download_update(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let update = state.updates.iter_mut()
            .find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        if update.status != UpdateStatus::Available {
            return Err(KernelError::InvalidArgument);
        }
        update.status = UpdateStatus::Downloading;
        update.progress_pct = 0;
        Ok(())
    })
}

/// Complete download.
pub fn complete_download(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let update = state.updates.iter_mut()
            .find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        if update.status != UpdateStatus::Downloading {
            return Err(KernelError::InvalidArgument);
        }
        update.status = UpdateStatus::Downloaded;
        update.progress_pct = 100;
        Ok(())
    })
}

/// Install an update.
pub fn install_update(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let update = state.updates.iter_mut()
            .find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        if update.status != UpdateStatus::Downloaded && update.status != UpdateStatus::Available {
            return Err(KernelError::InvalidArgument);
        }
        update.status = UpdateStatus::Installed;
        update.installed_ns = crate::hpet::elapsed_ns();
        update.progress_pct = 100;
        Ok(())
    })
}

/// Mark update as failed.
pub fn fail_update(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let update = state.updates.iter_mut()
            .find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        update.status = UpdateStatus::Failed;
        Ok(())
    })
}

/// Defer an update.
pub fn defer_update(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let update = state.updates.iter_mut()
            .find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        update.status = UpdateStatus::Deferred;
        Ok(())
    })
}

/// Move completed/failed updates to history.
pub fn archive_completed() -> KernelResult<usize> {
    with_state(|state| {
        let mut archived = 0usize;
        let mut i = 0;
        while i < state.updates.len() {
            if matches!(state.updates[i].status, UpdateStatus::Installed | UpdateStatus::Failed) {
                let update = state.updates.remove(i);
                if state.history.len() >= MAX_HISTORY {
                    state.history.remove(0);
                }
                state.history.push(update);
                archived += 1;
            } else {
                i += 1;
            }
        }
        Ok(archived)
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get an update by ID.
pub fn get_update(id: u64) -> KernelResult<Update> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.updates.iter()
        .find(|u| u.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List pending updates.
pub fn list_updates() -> Vec<Update> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.updates.clone())
}

/// List update history.
pub fn update_history() -> Vec<Update> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.history.clone())
}

/// Count pending updates by severity.
pub fn pending_count() -> (usize, usize, usize, usize) {
    let guard = STATE.lock();
    guard.as_ref().map_or((0, 0, 0, 0), |s| {
        let mut crit = 0;
        let mut imp = 0;
        let mut rec = 0;
        let mut opt = 0;
        for u in &s.updates {
            if matches!(u.status, UpdateStatus::Available | UpdateStatus::Downloaded) {
                match u.severity {
                    UpdateSeverity::Critical => crit += 1,
                    UpdateSeverity::Important => imp += 1,
                    UpdateSeverity::Recommended => rec += 1,
                    UpdateSeverity::Optional => opt += 1,
                }
            }
        }
        (crit, imp, rec, opt)
    })
}

/// Get total download size of pending updates.
pub fn pending_size() -> u64 {
    let guard = STATE.lock();
    guard.as_ref().map_or(0, |s| {
        s.updates.iter()
            .filter(|u| matches!(u.status, UpdateStatus::Available | UpdateStatus::Downloaded))
            .map(|u| u.size_bytes)
            .sum()
    })
}

/// Simulate check for updates (records check time).
pub fn check_updates() -> KernelResult<usize> {
    with_state(|state| {
        state.last_check_ns = crate::hpet::elapsed_ns();
        let pending = state.updates.iter()
            .filter(|u| matches!(u.status, UpdateStatus::Available | UpdateStatus::Downloaded))
            .count();
        Ok(pending)
    })
}

/// Get OS version string.
pub fn os_version() -> String {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(
        || String::from("unknown"),
        |s| format!("{} (build {})", s.os_version, s.os_build),
    )
}

// ---------------------------------------------------------------------------
// Format helpers
// ---------------------------------------------------------------------------

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{}.{} GB", bytes / 1_073_741_824, (bytes % 1_073_741_824) / 107_374_182)
    } else if bytes >= 1_048_576 {
        format!("{}.{} MB", bytes / 1_048_576, (bytes % 1_048_576) / 104_857)
    } else if bytes >= 1024 {
        format!("{}.{} KB", bytes / 1024, (bytes % 1024) / 102)
    } else {
        format!("{} B", bytes)
    }
}

/// Format size for external use.
pub fn format_update_size(bytes: u64) -> String {
    format_size(bytes)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (pending_count, history_count, os_version, channel, auto_check, ops).
pub fn stats() -> (usize, usize, String, &'static str, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.updates.len(),
            s.history.len(),
            format!("{} (build {})", s.os_version, s.os_build),
            s.config.channel.label(),
            s.config.auto_check,
            s.ops,
        ),
        None => (0, 0, String::from("n/a"), "n/a", false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the update manager module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[updatemgr] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        let (pending, history, version, channel, auto, _) = stats();
        assert_eq!(pending, 0);
        assert_eq!(history, 0);
        assert!(version.contains("0.1.0"));
        assert_eq!(channel, "Stable");
        assert!(auto);
    }
    serial_println!("[updatemgr]  1/11 initial state OK");

    // Test 2: config.
    {
        let cfg = get_config().unwrap();
        assert!(cfg.auto_check);
        assert_eq!(cfg.check_interval_hours, 12);
        assert!(cfg.auto_download);
        assert!(!cfg.auto_install);
        assert!(cfg.auto_install_security);
    }
    serial_println!("[updatemgr]  2/11 config OK");

    // Test 3: add update.
    {
        let id = add_available_update(
            "kernel", "0.1.0", "0.1.1",
            UpdateType::Security, UpdateSeverity::Critical,
            5_242_880, "Critical security fix", true,
        ).unwrap();
        let u = get_update(id).unwrap();
        assert_eq!(u.package, "kernel");
        assert_eq!(u.severity, UpdateSeverity::Critical);
        assert_eq!(u.status, UpdateStatus::Available);
    }
    serial_println!("[updatemgr]  3/11 add update OK");

    // Test 4: download lifecycle.
    {
        let updates = list_updates();
        let id = updates.first().unwrap().id;
        download_update(id).unwrap();
        assert_eq!(get_update(id).unwrap().status, UpdateStatus::Downloading);
        complete_download(id).unwrap();
        assert_eq!(get_update(id).unwrap().status, UpdateStatus::Downloaded);
    }
    serial_println!("[updatemgr]  4/11 download lifecycle OK");

    // Test 5: install update.
    {
        let updates = list_updates();
        let id = updates.first().unwrap().id;
        install_update(id).unwrap();
        assert_eq!(get_update(id).unwrap().status, UpdateStatus::Installed);
    }
    serial_println!("[updatemgr]  5/11 install OK");

    // Test 6: archive.
    {
        let count = archive_completed().unwrap();
        assert_eq!(count, 1);
        assert!(list_updates().is_empty());
        assert_eq!(update_history().len(), 1);
    }
    serial_println!("[updatemgr]  6/11 archive OK");

    // Test 7: defer update.
    {
        let id = add_available_update(
            "libc", "2.0", "2.1",
            UpdateType::BugFix, UpdateSeverity::Recommended,
            1_048_576, "Stability fixes", false,
        ).unwrap();
        defer_update(id).unwrap();
        assert_eq!(get_update(id).unwrap().status, UpdateStatus::Deferred);
    }
    serial_println!("[updatemgr]  7/11 defer OK");

    // Test 8: pending counts.
    {
        let _ = add_available_update("gui", "1.0", "1.1", UpdateType::Feature, UpdateSeverity::Optional, 2_097_152, "New features", false);
        let (c, i, r, o) = pending_count();
        assert_eq!(c, 0);
        assert_eq!(i, 0);
        assert_eq!(r, 0);
        assert_eq!(o, 1);
    }
    serial_println!("[updatemgr]  8/11 pending counts OK");

    // Test 9: channel switching.
    {
        set_channel(UpdateChannel::Beta).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.channel, UpdateChannel::Beta);
        set_channel(UpdateChannel::Stable).unwrap();
    }
    serial_println!("[updatemgr]  9/11 channel OK");

    // Test 10: active hours.
    {
        set_active_hours(9, 17).unwrap();
        let cfg = get_config().unwrap();
        assert_eq!(cfg.active_hours_start, 9);
        assert_eq!(cfg.active_hours_end, 17);
        assert!(set_active_hours(25, 0).is_err());
    }
    serial_println!("[updatemgr] 10/11 active hours OK");

    // Test 11: check updates.
    {
        let count = check_updates().unwrap();
        assert!(count > 0);
        let ver = os_version();
        assert!(ver.contains("0.1.0"));
    }
    serial_println!("[updatemgr] 11/11 check updates OK");

    serial_println!("[updatemgr] All self-tests passed.");
}
