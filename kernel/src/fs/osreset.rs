//! OS reset and repair — restore the OS to initial or known-good state.
//!
//! Provides the data model for the reset/repair functionality described in
//! design.txt lines 1283-1288: reset the OS while optionally preserving
//! user files, re-importing applications, and restoring OS settings by
//! category. Supports rollback checkpoints and staged operations.
//!
//! ## Design Reference
//!
//! design.txt lines 1283-1288:
//! "reset the OS, like on Windows?"
//! "separate settings and files needed for apps ... reinstallation or
//! repair and restore the OS to its initial state but import as many
//! applications as possible ... rollback options ... without having to
//! install a newer or equal version of the OS."
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → OS Reset
//!   → osreset::create_checkpoint() — saves current state
//!   → osreset::plan_reset(options) — computes what will change
//!   → osreset::execute_reset(plan_id) — performs the reset
//!   → osreset::rollback(checkpoint_id) — reverts to checkpoint
//!
//! OS Repair
//!   → osreset::scan_integrity() — checks system file hashes
//!   → osreset::repair_files() — restores corrupted files
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

/// What to reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetScope {
    /// Full factory reset — remove everything.
    Full,
    /// Reset OS but keep user files.
    KeepFiles,
    /// Reset OS but keep user files and applications.
    KeepFilesAndApps,
    /// Repair only — fix corrupted system files without resetting.
    RepairOnly,
}

/// Category of OS settings that can be selectively re-imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    /// Display settings (resolution, scaling, theme).
    Display,
    /// Network settings (WiFi passwords, DNS).
    Network,
    /// Sound/audio settings.
    Audio,
    /// Keyboard and mouse settings.
    Input,
    /// Privacy and security settings.
    Privacy,
    /// Power management settings.
    Power,
    /// User accounts and groups.
    Accounts,
    /// Language and locale.
    Locale,
    /// System services configuration.
    Services,
    /// Desktop appearance (wallpaper, icons, taskbar).
    Appearance,
    /// File associations and default apps.
    FileAssociations,
    /// Accessibility settings.
    Accessibility,
}

/// Risk level for re-importing an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRiskLevel {
    /// Safe — uses only standard capabilities.
    Safe,
    /// Moderate — uses elevated capabilities.
    Moderate,
    /// Dangerous — uses system-level or invasive capabilities.
    Dangerous,
    /// Unknown — risk cannot be determined.
    Unknown,
}

/// Information about an app that can be re-imported after reset.
#[derive(Debug, Clone)]
pub struct AppImportInfo {
    /// Application ID.
    pub app_id: String,
    /// Application name.
    pub name: String,
    /// Risk level.
    pub risk: AppRiskLevel,
    /// Whether to include in re-import (user's choice).
    pub include: bool,
    /// Whether to import app settings.
    pub import_settings: bool,
    /// Whether to import app data.
    pub import_data: bool,
    /// Size of files to preserve (bytes).
    pub size_bytes: u64,
    /// Warning message (e.g., "uses dangerous capabilities").
    pub warning: String,
}

/// Information about a settings category for selective import.
#[derive(Debug, Clone)]
pub struct SettingsImportInfo {
    /// Category.
    pub category: SettingsCategory,
    /// Human-readable description.
    pub description: String,
    /// Whether to include in re-import.
    pub include: bool,
    /// Number of settings entries.
    pub entry_count: u32,
}

/// A pre-reset checkpoint for rollback support.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Unique checkpoint ID.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Creation timestamp (ns).
    pub created_ns: u64,
    /// Scope of state captured.
    pub scope: ResetScope,
    /// Size of checkpoint data (bytes).
    pub size_bytes: u64,
    /// Whether this checkpoint is still valid for rollback.
    pub valid: bool,
}

/// A reset plan computed from options — shows what will happen.
#[derive(Debug, Clone)]
pub struct ResetPlan {
    /// Unique plan ID.
    pub id: u64,
    /// Reset scope.
    pub scope: ResetScope,
    /// Apps that will be re-imported.
    pub apps: Vec<AppImportInfo>,
    /// Settings categories that will be re-imported.
    pub settings: Vec<SettingsImportInfo>,
    /// Total data to preserve (bytes).
    pub preserve_bytes: u64,
    /// Total data to delete (bytes).
    pub delete_bytes: u64,
    /// Checkpoint created before this plan executes.
    pub checkpoint_id: u64,
    /// Whether the plan has been executed.
    pub executed: bool,
}

/// System file integrity check result.
#[derive(Debug, Clone)]
pub struct IntegrityResult {
    /// Total files checked.
    pub total_files: u64,
    /// Files that passed integrity check.
    pub good_files: u64,
    /// Files with corrupted content.
    pub corrupted_files: u64,
    /// Files that are missing.
    pub missing_files: u64,
    /// Files with wrong permissions.
    pub permission_errors: u64,
    /// Paths of corrupted/missing files.
    pub problem_paths: Vec<String>,
}

/// Status of the reset subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetStatus {
    /// Ready — no operation in progress.
    Idle,
    /// Scanning system integrity.
    Scanning,
    /// Creating checkpoint.
    Checkpointing,
    /// Planning reset.
    Planning,
    /// Executing reset (dangerous phase).
    Executing,
    /// Repair in progress.
    Repairing,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    checkpoints: Vec<Checkpoint>,
    plans: Vec<ResetPlan>,
    status: ResetStatus,
    last_integrity: Option<IntegrityResult>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    checkpoints: Vec::new(),
    plans: Vec::new(),
    status: ResetStatus::Idle,
    last_integrity: None,
    changes: 0,
});

static NEXT_CHECKPOINT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PLAN_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Checkpoints
// ---------------------------------------------------------------------------

/// Create a checkpoint (snapshot of current state for rollback).
pub fn create_checkpoint(name: &str, scope: ResetScope) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.checkpoints.len() >= 32 {
        return Err(KernelError::ResourceExhausted);
    }
    if state.status != ResetStatus::Idle {
        return Err(KernelError::WouldBlock);
    }
    let id = NEXT_CHECKPOINT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    state.checkpoints.push(Checkpoint {
        id,
        name: String::from(name),
        created_ns: now,
        scope,
        size_bytes: 0, // Would be computed from actual backup.
        valid: true,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Delete a checkpoint.
pub fn delete_checkpoint(checkpoint_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Cannot delete if a plan references it and hasn't been executed.
    if state.plans.iter().any(|p| p.checkpoint_id == checkpoint_id && !p.executed) {
        return Err(KernelError::NotEmpty);
    }
    let before = state.checkpoints.len();
    state.checkpoints.retain(|c| c.id != checkpoint_id);
    if state.checkpoints.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

/// List all checkpoints.
pub fn list_checkpoints() -> Vec<Checkpoint> {
    STATE.lock().checkpoints.clone()
}

/// Get a specific checkpoint.
pub fn get_checkpoint(checkpoint_id: u64) -> KernelResult<Checkpoint> {
    let state = STATE.lock();
    state
        .checkpoints
        .iter()
        .find(|c| c.id == checkpoint_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// Rollback to a checkpoint (marks intent — actual restore is out of scope).
pub fn rollback(checkpoint_id: u64) -> KernelResult<Checkpoint> {
    let state = STATE.lock();
    let cp = state
        .checkpoints
        .iter()
        .find(|c| c.id == checkpoint_id && c.valid)
        .cloned()
        .ok_or(KernelError::NotFound)?;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(cp)
}

// ---------------------------------------------------------------------------
// Reset planning
// ---------------------------------------------------------------------------

/// Create a reset plan: computes what will happen for the given scope.
pub fn plan_reset(scope: ResetScope) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.status != ResetStatus::Idle {
        return Err(KernelError::WouldBlock);
    }
    if state.plans.len() >= 16 {
        return Err(KernelError::ResourceExhausted);
    }

    // Create a pre-reset checkpoint.
    let cp_id = NEXT_CHECKPOINT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    state.checkpoints.push(Checkpoint {
        id: cp_id,
        name: String::from("pre-reset-auto"),
        created_ns: now,
        scope,
        size_bytes: 0,
        valid: true,
    });

    // Build default app import list (simulated).
    let apps = match scope {
        ResetScope::Full => Vec::new(),
        ResetScope::RepairOnly => Vec::new(),
        _ => {
            alloc::vec![
                AppImportInfo {
                    app_id: String::from("system.fileexplorer"),
                    name: String::from("File Explorer"),
                    risk: AppRiskLevel::Safe,
                    include: true,
                    import_settings: true,
                    import_data: true,
                    size_bytes: 2_048_000,
                    warning: String::new(),
                },
                AppImportInfo {
                    app_id: String::from("system.terminal"),
                    name: String::from("Terminal"),
                    risk: AppRiskLevel::Safe,
                    include: true,
                    import_settings: true,
                    import_data: false,
                    size_bytes: 512_000,
                    warning: String::new(),
                },
            ]
        }
    };

    // Build default settings import list.
    let settings = if scope == ResetScope::Full {
        Vec::new()
    } else {
        use SettingsCategory::*;
        alloc::vec![
            SettingsImportInfo { category: Display, description: String::from("Display and resolution"), include: true, entry_count: 12 },
            SettingsImportInfo { category: Network, description: String::from("WiFi and network"), include: true, entry_count: 8 },
            SettingsImportInfo { category: Audio, description: String::from("Sound settings"), include: true, entry_count: 6 },
            SettingsImportInfo { category: Input, description: String::from("Keyboard and mouse"), include: true, entry_count: 15 },
            SettingsImportInfo { category: Privacy, description: String::from("Privacy and security"), include: false, entry_count: 10 },
            SettingsImportInfo { category: Power, description: String::from("Power management"), include: true, entry_count: 5 },
            SettingsImportInfo { category: Accounts, description: String::from("User accounts"), include: true, entry_count: 4 },
            SettingsImportInfo { category: Locale, description: String::from("Language and region"), include: true, entry_count: 7 },
            SettingsImportInfo { category: Appearance, description: String::from("Desktop appearance"), include: true, entry_count: 9 },
            SettingsImportInfo { category: FileAssociations, description: String::from("File type associations"), include: true, entry_count: 20 },
            SettingsImportInfo { category: Accessibility, description: String::from("Accessibility"), include: true, entry_count: 11 },
        ]
    };

    let preserve: u64 = apps.iter().filter(|a| a.include).map(|a| a.size_bytes).sum();

    let plan_id = NEXT_PLAN_ID.fetch_add(1, Ordering::Relaxed);
    state.plans.push(ResetPlan {
        id: plan_id,
        scope,
        apps,
        settings,
        preserve_bytes: preserve,
        delete_bytes: 500_000_000, // simulated
        checkpoint_id: cp_id,
        executed: false,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(plan_id)
}

/// Get a reset plan by ID.
pub fn get_plan(plan_id: u64) -> KernelResult<ResetPlan> {
    let state = STATE.lock();
    state
        .plans
        .iter()
        .find(|p| p.id == plan_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all plans.
pub fn list_plans() -> Vec<ResetPlan> {
    STATE.lock().plans.clone()
}

/// Modify app inclusion in a plan.
pub fn set_app_include(plan_id: u64, app_id: &str, include: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let plan = state
        .plans
        .iter_mut()
        .find(|p| p.id == plan_id && !p.executed)
        .ok_or(KernelError::NotFound)?;
    let app = plan
        .apps
        .iter_mut()
        .find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.include = include;
    // Recompute preserve total.
    plan.preserve_bytes = plan.apps.iter().filter(|a| a.include).map(|a| a.size_bytes).sum();
    state.changes += 1;
    Ok(())
}

/// Modify settings category inclusion in a plan.
pub fn set_settings_include(
    plan_id: u64,
    category: SettingsCategory,
    include: bool,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let plan = state
        .plans
        .iter_mut()
        .find(|p| p.id == plan_id && !p.executed)
        .ok_or(KernelError::NotFound)?;
    let sett = plan
        .settings
        .iter_mut()
        .find(|s| s.category == category)
        .ok_or(KernelError::NotFound)?;
    sett.include = include;
    state.changes += 1;
    Ok(())
}

/// Execute a reset plan (marks as executed — real OS would do the work).
pub fn execute_reset(plan_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.status != ResetStatus::Idle {
        return Err(KernelError::WouldBlock);
    }
    let plan = state
        .plans
        .iter_mut()
        .find(|p| p.id == plan_id && !p.executed)
        .ok_or(KernelError::NotFound)?;
    plan.executed = true;
    state.status = ResetStatus::Executing;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    // In a real OS, this would trigger the actual reset process.
    // For now, just mark as done.
    state.status = ResetStatus::Idle;
    Ok(())
}

/// Cancel an unexecuted plan.
pub fn cancel_plan(plan_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let before = state.plans.len();
    state.plans.retain(|p| p.id != plan_id || p.executed);
    if state.plans.len() == before {
        return Err(KernelError::NotFound);
    }
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Integrity scanning / repair
// ---------------------------------------------------------------------------

/// Scan system file integrity (simulated).
pub fn scan_integrity() -> KernelResult<IntegrityResult> {
    let mut state = STATE.lock();
    if state.status != ResetStatus::Idle {
        return Err(KernelError::WouldBlock);
    }
    state.status = ResetStatus::Scanning;
    // Simulated scan.
    let result = IntegrityResult {
        total_files: 5000,
        good_files: 4998,
        corrupted_files: 1,
        missing_files: 1,
        permission_errors: 0,
        problem_paths: alloc::vec![
            String::from("/usr/lib/libcrypto.so (corrupted)"),
            String::from("/etc/default/locale (missing)"),
        ],
    };
    state.last_integrity = Some(result.clone());
    state.status = ResetStatus::Idle;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(result)
}

/// Repair corrupted/missing system files (simulated).
pub fn repair_files() -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.status != ResetStatus::Idle {
        return Err(KernelError::WouldBlock);
    }
    let problems = state
        .last_integrity
        .as_ref()
        .map_or(0, |r| r.corrupted_files + r.missing_files);
    if problems == 0 {
        return Ok(0);
    }
    state.status = ResetStatus::Repairing;
    // Simulated repair.
    if let Some(ref mut result) = state.last_integrity {
        result.good_files += result.corrupted_files + result.missing_files;
        result.corrupted_files = 0;
        result.missing_files = 0;
        result.problem_paths.clear();
    }
    state.status = ResetStatus::Idle;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(problems)
}

/// Get the last integrity scan result.
pub fn last_integrity() -> Option<IntegrityResult> {
    STATE.lock().last_integrity.clone()
}

/// Get current reset subsystem status.
pub fn status() -> ResetStatus {
    STATE.lock().status
}

// ---------------------------------------------------------------------------
// Stats / housekeeping
// ---------------------------------------------------------------------------

/// Return (checkpoint_count, plan_count, integrity_problems, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    let cps = state.checkpoints.len();
    let plans = state.plans.len();
    let problems = state
        .last_integrity
        .as_ref()
        .map_or(0, |r| r.corrupted_files + r.missing_files);
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (cps, plans, problems, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.checkpoints.clear();
    state.plans.clear();
    state.status = ResetStatus::Idle;
    state.last_integrity = None;
    state.changes = 0;
    NEXT_CHECKPOINT_ID.store(1, Ordering::Relaxed);
    NEXT_PLAN_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: create checkpoints.
    serial_println!("osreset::self_test 1: checkpoints");
    let cp1 = create_checkpoint("before-update", ResetScope::KeepFiles)?;
    let cp2 = create_checkpoint("manual-backup", ResetScope::Full)?;
    assert_eq!(list_checkpoints().len(), 2);
    let cp = get_checkpoint(cp1)?;
    assert_eq!(cp.name, "before-update");
    assert!(cp.valid);

    // Test 2: plan reset (KeepFilesAndApps).
    serial_println!("osreset::self_test 2: plan reset");
    let plan_id = plan_reset(ResetScope::KeepFilesAndApps)?;
    let plan = get_plan(plan_id)?;
    assert_eq!(plan.scope, ResetScope::KeepFilesAndApps);
    assert!(!plan.apps.is_empty());
    assert!(!plan.settings.is_empty());
    assert!(!plan.executed);

    // Test 3: modify plan.
    serial_println!("osreset::self_test 3: modify plan");
    set_app_include(plan_id, "system.terminal", false)?;
    let plan = get_plan(plan_id)?;
    let term = plan.apps.iter().find(|a| a.app_id == "system.terminal").expect("terminal");
    assert!(!term.include);
    set_settings_include(plan_id, SettingsCategory::Privacy, true)?;
    let plan = get_plan(plan_id)?;
    let priv_cat = plan.settings.iter().find(|s| s.category == SettingsCategory::Privacy).expect("privacy");
    assert!(priv_cat.include);

    // Test 4: execute plan.
    serial_println!("osreset::self_test 4: execute plan");
    execute_reset(plan_id)?;
    let plan = get_plan(plan_id)?;
    assert!(plan.executed);
    // Cannot execute again.
    assert!(execute_reset(plan_id).is_err());

    // Test 5: integrity scan.
    serial_println!("osreset::self_test 5: integrity scan");
    let result = scan_integrity()?;
    assert_eq!(result.total_files, 5000);
    assert!(result.corrupted_files > 0 || result.missing_files > 0);
    assert!(!result.problem_paths.is_empty());

    // Test 6: repair.
    serial_println!("osreset::self_test 6: repair");
    let fixed = repair_files()?;
    assert!(fixed > 0);
    let result = last_integrity().expect("integrity result");
    assert_eq!(result.corrupted_files, 0);
    assert_eq!(result.missing_files, 0);

    // Test 7: rollback and cleanup.
    serial_println!("osreset::self_test 7: rollback and cleanup");
    let cp = rollback(cp1)?;
    assert_eq!(cp.name, "before-update");
    delete_checkpoint(cp2)?;
    assert_eq!(list_checkpoints().len(), 2); // cp1 + auto-checkpoint from plan
    cancel_plan(999).ok(); // no-op, just verifying it doesn't panic
    assert_eq!(status(), ResetStatus::Idle);

    clear_all();
    serial_println!("osreset::self_test: all 7 tests passed");
    Ok(())
}
