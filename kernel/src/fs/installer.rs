//! Installation wizard — interactive OS installation and first-boot setup.
//!
//! Manages the full installation flow from partitioning through first-boot
//! configuration.  Both interactive and unattended modes are supported.
//!
//! ## Design Reference
//!
//! design.txt lines 1337-1362:
//! - Keyboard/layout selection
//! - DPI detection and scaling
//! - Easy vs manual installation (partition management)
//! - Workload type selection → memory/scheduling/filesystem defaults
//! - Post-reboot: audio, timezone, user account, browser, theme, wifi
//! - Unattended installation (YAML config, fallback to defaults)
//!
//! ## Architecture
//!
//! ```text
//! Install media boots
//!   → installer::create_session(mode)
//!   → installer::set_keyboard(layout)
//!   → installer::set_scaling(factor)
//!   → installer::set_install_target(disk, partition_plan)
//!   → installer::set_workload(type)
//!   → installer::execute_install()
//!
//! First boot
//!   → installer::first_boot_session()
//!   → installer::set_timezone(tz)
//!   → installer::create_user(name, pass)
//!   → installer::set_browser(name)
//!   → installer::set_theme(mode)
//!   → installer::set_wifi(ssid, pass)
//!   → installer::complete_first_boot()
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

/// Installation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Guided: automatic partitioning, default settings.
    Easy,
    /// Manual: user controls partitioning and all settings.
    Manual,
    /// Unattended: all options from a YAML config file.
    Unattended,
}

/// Installation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPhase {
    /// Not started.
    NotStarted,
    /// Pre-install: keyboard, scaling, disk selection.
    PreInstall,
    /// Partitioning in progress.
    Partitioning,
    /// Copying files.
    Copying,
    /// Configuring bootloader.
    Bootloader,
    /// Waiting for first reboot.
    PendingReboot,
    /// First-boot setup (audio, timezone, user, etc.).
    FirstBoot,
    /// Complete.
    Complete,
    /// Failed (error stored).
    Failed,
}

/// Workload type for system tuning defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadType {
    Desktop,
    Server,
    Development,
    Gaming,
}

/// Partition plan for manual install.
#[derive(Debug, Clone)]
pub struct PartitionPlan {
    /// Target disk name/path.
    pub disk: String,
    /// Boot partition size in MiB (minimum 512).
    pub boot_mib: u32,
    /// Swap: None = swap file, Some(mib) = swap partition.
    pub swap_mib: Option<u32>,
    /// Root partition gets remaining space.
    pub root_label: String,
    /// Whether to erase entire disk.
    pub erase_disk: bool,
}

/// Scaling preset based on DPI detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalePreset {
    /// 1x scaling (96 DPI, standard).
    Normal,
    /// 1.25x (120 DPI).
    Medium,
    /// 1.5x (144 DPI).
    Large,
    /// 2x (192 DPI, HiDPI/Retina).
    HiDpi,
    /// Custom factor (100 = 1x, 200 = 2x).
    Custom(u32),
}

/// Browser choice for default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserChoice {
    Firefox,
    Chromium,
    Epiphany,
    Custom,
}

/// Network configuration type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetConfig {
    /// Ethernet (auto DHCP).
    Ethernet,
    /// Wifi with SSID + password.
    Wifi,
    /// Skip network setup.
    Skip,
}

/// Complete installation session state.
#[derive(Debug, Clone)]
pub struct InstallSession {
    pub id: u64,
    pub mode: InstallMode,
    pub phase: InstallPhase,

    // Pre-install settings.
    pub keyboard_layout: String,
    pub keyboard_variant: String,
    pub scale_preset: ScalePreset,
    pub detected_dpi: u32,

    // Disk/partition.
    pub partition_plan: Option<PartitionPlan>,

    // Workload tuning.
    pub workload: WorkloadType,

    // Post-reboot / first-boot settings.
    pub timezone: String,
    pub username: String,
    pub password_set: bool,
    pub auto_login: bool,
    pub browser: BrowserChoice,
    pub theme_mode: String,
    pub wifi_ssid: String,
    pub wifi_password_set: bool,
    pub audio_device: String,

    // Unattended config path.
    pub config_path: String,

    // Progress tracking.
    pub progress_pct: u8,
    pub status_message: String,
    pub error_message: String,
    pub created_ns: u64,
    pub completed_ns: u64,
}

/// Sanity check result for partition/settings validation.
#[derive(Debug, Clone)]
pub struct SanityResult {
    pub passed: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    sessions: Vec<InstallSession>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    sessions: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Session lifecycle
// ---------------------------------------------------------------------------

/// Create a new installation session.
pub fn create_session(mode: InstallMode) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.sessions.len() >= 16 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.sessions.push(InstallSession {
        id,
        mode,
        phase: InstallPhase::PreInstall,
        keyboard_layout: String::from("us"),
        keyboard_variant: String::new(),
        scale_preset: ScalePreset::Normal,
        detected_dpi: 96,
        partition_plan: None,
        workload: WorkloadType::Desktop,
        timezone: String::new(),
        username: String::new(),
        password_set: false,
        auto_login: false,
        browser: BrowserChoice::Firefox,
        theme_mode: String::from("dark"),
        wifi_ssid: String::new(),
        wifi_password_set: false,
        audio_device: String::new(),
        config_path: String::new(),
        progress_pct: 0,
        status_message: String::from("Ready"),
        error_message: String::new(),
        created_ns: crate::hpet::elapsed_ns(),
        completed_ns: 0,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Get a session by ID.
pub fn get_session(session_id: u64) -> KernelResult<InstallSession> {
    STATE.lock().sessions.iter().find(|s| s.id == session_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all sessions.
pub fn list_sessions() -> Vec<InstallSession> {
    STATE.lock().sessions.clone()
}

/// Remove a completed/failed session.
pub fn remove_session(session_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = state.sessions.iter().find(|s| s.id == session_id)
        .ok_or(KernelError::NotFound)?;
    if s.phase != InstallPhase::Complete && s.phase != InstallPhase::Failed
        && s.phase != InstallPhase::NotStarted
    {
        return Err(KernelError::WouldBlock);
    }
    state.sessions.retain(|s| s.id != session_id);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pre-install configuration
// ---------------------------------------------------------------------------

/// Set keyboard layout and variant.
pub fn set_keyboard(session_id: u64, layout: &str, variant: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.keyboard_layout = String::from(layout);
    s.keyboard_variant = String::from(variant);
    state.changes += 1;
    Ok(())
}

/// Detect DPI and set scaling. Returns suggested ScalePreset.
pub fn detect_and_set_scaling(session_id: u64, monitor_dpi: u32) -> KernelResult<ScalePreset> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.detected_dpi = monitor_dpi;
    let preset = if monitor_dpi <= 110 {
        ScalePreset::Normal
    } else if monitor_dpi <= 132 {
        ScalePreset::Medium
    } else if monitor_dpi <= 168 {
        ScalePreset::Large
    } else {
        ScalePreset::HiDpi
    };
    s.scale_preset = preset;
    state.changes += 1;
    Ok(preset)
}

/// Override scaling to a custom factor.
pub fn set_scaling(session_id: u64, preset: ScalePreset) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.scale_preset = preset;
    state.changes += 1;
    Ok(())
}

/// Set the partition plan (manual install).
pub fn set_partition_plan(session_id: u64, plan: PartitionPlan) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.partition_plan = Some(plan);
    state.changes += 1;
    Ok(())
}

/// Set workload type for tuning defaults.
pub fn set_workload(session_id: u64, workload: WorkloadType) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.workload = workload;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Installation execution
// ---------------------------------------------------------------------------

/// Run sanity checks before installation.
pub fn sanity_check(session_id: u64) -> KernelResult<SanityResult> {
    use alloc::format;
    let state = STATE.lock();
    let s = state.sessions.iter().find(|s| s.id == session_id)
        .ok_or(KernelError::NotFound)?;

    let mut result = SanityResult {
        passed: true,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    // Check partition plan exists for manual mode.
    if s.mode == InstallMode::Manual && s.partition_plan.is_none() {
        result.errors.push(String::from("No partition plan set for manual install"));
        result.passed = false;
    }

    // Check partition sizes.
    if let Some(ref plan) = s.partition_plan {
        if plan.boot_mib < 512 {
            result.errors.push(String::from("Boot partition too small (min 512 MiB)"));
            result.passed = false;
        }
        if plan.boot_mib > 4096 {
            result.warnings.push(String::from("Boot partition unusually large (> 4 GiB)"));
        }
        if let Some(swap) = plan.swap_mib {
            if swap < 512 {
                result.warnings.push(String::from("Swap partition small (< 512 MiB)"));
            }
            if swap > 65536 {
                result.warnings.push(format!("Swap partition very large ({} MiB)", swap));
            }
        }
        if plan.disk.is_empty() {
            result.errors.push(String::from("No target disk specified"));
            result.passed = false;
        }
    }

    // Unattended mode needs a config path.
    if s.mode == InstallMode::Unattended && s.config_path.is_empty() {
        result.errors.push(String::from("No config file path for unattended install"));
        result.passed = false;
    }

    Ok(result)
}

/// Execute the installation (simulated — advances through phases).
pub fn execute_install(session_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    if s.phase != InstallPhase::PreInstall {
        return Err(KernelError::InvalidArgument);
    }

    // Simulate: advance through phases.
    s.phase = InstallPhase::Partitioning;
    s.progress_pct = 10;
    s.status_message = String::from("Partitioning disk...");

    // In a real system, each phase would be async. Here we simulate completion.
    s.phase = InstallPhase::Copying;
    s.progress_pct = 40;
    s.status_message = String::from("Copying files...");

    s.phase = InstallPhase::Bootloader;
    s.progress_pct = 80;
    s.status_message = String::from("Configuring bootloader...");

    s.phase = InstallPhase::PendingReboot;
    s.progress_pct = 90;
    s.status_message = String::from("Installation complete — reboot required");

    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// First-boot configuration
// ---------------------------------------------------------------------------

/// Start first-boot setup phase.
pub fn start_first_boot(session_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    if s.phase != InstallPhase::PendingReboot {
        return Err(KernelError::InvalidArgument);
    }
    s.phase = InstallPhase::FirstBoot;
    s.progress_pct = 92;
    s.status_message = String::from("First-boot setup...");
    state.changes += 1;
    Ok(())
}

/// Set audio device.
pub fn set_audio_device(session_id: u64, device: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.audio_device = String::from(device);
    state.changes += 1;
    Ok(())
}

/// Set timezone.
pub fn set_timezone(session_id: u64, tz: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.timezone = String::from(tz);
    state.changes += 1;
    Ok(())
}

/// Set user account.
pub fn set_user(session_id: u64, username: &str, has_password: bool, auto_login: bool) -> KernelResult<()> {
    if username.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.username = String::from(username);
    s.password_set = has_password;
    s.auto_login = auto_login;
    state.changes += 1;
    Ok(())
}

/// Set default browser.
pub fn set_browser(session_id: u64, browser: BrowserChoice) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.browser = browser;
    state.changes += 1;
    Ok(())
}

/// Set theme mode.
pub fn set_theme(session_id: u64, mode: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.theme_mode = String::from(mode);
    state.changes += 1;
    Ok(())
}

/// Set wifi configuration.
pub fn set_wifi(session_id: u64, ssid: &str, has_password: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.wifi_ssid = String::from(ssid);
    s.wifi_password_set = has_password;
    state.changes += 1;
    Ok(())
}

/// Set unattended config file path.
pub fn set_config_path(session_id: u64, path: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.config_path = String::from(path);
    state.changes += 1;
    Ok(())
}

/// Complete first-boot setup.
pub fn complete_first_boot(session_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    if s.phase != InstallPhase::FirstBoot {
        return Err(KernelError::InvalidArgument);
    }
    // Validate required fields.
    if s.username.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    s.phase = InstallPhase::Complete;
    s.progress_pct = 100;
    s.status_message = String::from("Installation complete");
    s.completed_ns = crate::hpet::elapsed_ns();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Mark session as failed.
pub fn mark_failed(session_id: u64, error: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let s = find_session_mut(&mut state, session_id)?;
    s.phase = InstallPhase::Failed;
    s.error_message = String::from(error);
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn find_session_mut(state: &mut State, id: u64) -> KernelResult<&mut InstallSession> {
    state.sessions.iter_mut().find(|s| s.id == id)
        .ok_or(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with a default demo session.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.sessions.is_empty() {
        return;
    }

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    state.sessions.push(InstallSession {
        id,
        mode: InstallMode::Easy,
        phase: InstallPhase::Complete,
        keyboard_layout: String::from("us"),
        keyboard_variant: String::new(),
        scale_preset: ScalePreset::Normal,
        detected_dpi: 96,
        partition_plan: Some(PartitionPlan {
            disk: String::from("/dev/sda"),
            boot_mib: 1024,
            swap_mib: None, // using swap file
            root_label: String::from("MintOS"),
            erase_disk: true,
        }),
        workload: WorkloadType::Desktop,
        timezone: String::from("America/New_York"),
        username: String::from("user"),
        password_set: true,
        auto_login: true,
        browser: BrowserChoice::Firefox,
        theme_mode: String::from("dark"),
        wifi_ssid: String::new(),
        wifi_password_set: false,
        audio_device: String::from("default"),
        config_path: String::new(),
        progress_pct: 100,
        status_message: String::from("Installation complete"),
        error_message: String::new(),
        created_ns: now,
        completed_ns: now,
    });
    state.changes += 1;
}

/// Return (session_count, complete_count, failed_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.sessions.len();
    let complete = state.sessions.iter().filter(|s| s.phase == InstallPhase::Complete).count();
    let failed = state.sessions.iter().filter(|s| s.phase == InstallPhase::Failed).count();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, complete, failed, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.sessions.clear();
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: create session.
    serial_println!("installer::self_test 1: create session");
    let s1 = create_session(InstallMode::Easy)?;
    let s2 = create_session(InstallMode::Manual)?;
    assert_eq!(list_sessions().len(), 2);

    // Test 2: pre-install config.
    serial_println!("installer::self_test 2: pre-install config");
    set_keyboard(s1, "de", "nodeadkeys")?;
    let preset = detect_and_set_scaling(s1, 192)?;
    assert_eq!(preset, ScalePreset::HiDpi);
    set_workload(s1, WorkloadType::Desktop)?;
    let sess = get_session(s1)?;
    assert_eq!(sess.keyboard_layout, "de");
    assert_eq!(sess.detected_dpi, 192);

    // Test 3: partition plan and sanity check.
    serial_println!("installer::self_test 3: partition plan and sanity");
    // Manual mode without plan should fail sanity check.
    let check = sanity_check(s2)?;
    assert!(!check.passed);
    assert!(!check.errors.is_empty());
    // Set plan and check again.
    set_partition_plan(s2, PartitionPlan {
        disk: String::from("/dev/sda"),
        boot_mib: 1024,
        swap_mib: Some(4096),
        root_label: String::from("MintOS"),
        erase_disk: true,
    })?;
    let check = sanity_check(s2)?;
    assert!(check.passed);

    // Test 4: execute install.
    serial_println!("installer::self_test 4: execute install");
    execute_install(s1)?;
    let sess = get_session(s1)?;
    assert_eq!(sess.phase, InstallPhase::PendingReboot);
    assert_eq!(sess.progress_pct, 90);

    // Test 5: first-boot setup.
    serial_println!("installer::self_test 5: first-boot setup");
    start_first_boot(s1)?;
    set_timezone(s1, "Europe/Berlin")?;
    set_user(s1, "testuser", true, false)?;
    set_browser(s1, BrowserChoice::Chromium)?;
    set_theme(s1, "light")?;
    set_audio_device(s1, "HDA Intel")?;
    let sess = get_session(s1)?;
    assert_eq!(sess.timezone, "Europe/Berlin");
    assert_eq!(sess.username, "testuser");

    // Test 6: complete first boot.
    serial_println!("installer::self_test 6: complete first boot");
    complete_first_boot(s1)?;
    let sess = get_session(s1)?;
    assert_eq!(sess.phase, InstallPhase::Complete);
    assert_eq!(sess.progress_pct, 100);

    // Test 7: mark failed + remove.
    serial_println!("installer::self_test 7: failure and removal");
    execute_install(s2)?;
    mark_failed(s2, "Disk write error")?;
    let sess = get_session(s2)?;
    assert_eq!(sess.phase, InstallPhase::Failed);
    remove_session(s2)?;
    remove_session(s1)?;
    assert!(list_sessions().is_empty());

    clear_all();
    serial_println!("installer::self_test: all 7 tests passed");
    Ok(())
}
