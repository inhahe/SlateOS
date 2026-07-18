//! Autostart manager — login startup items and delayed services.
//!
//! Manages programs that launch automatically at user login or system
//! startup.  Provides ordering, delay, conditions, and per-user overrides.
//!
//! ## Design Reference
//!
//! design.txt line 1290: programs can have Autostart capability
//! design.txt line 1354: post-reboot configuration, services start
//!
//! ## Architecture
//!
//! ```text
//! Login / session manager
//!   → autostart::items_for_user(uid) → sorted startup list
//!   → autostart::launch_item(id) → mark as launched
//!
//! Settings panel → Startup Applications
//!   → autostart::list_items() → all configured items
//!   → autostart::add_item(...)
//!   → autostart::set_enabled(id, false) → disable without removing
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

/// When the item should start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPhase {
    /// Very early — system services, before desktop.
    SystemService,
    /// Desktop environment ready — compositor, window manager.
    DesktopReady,
    /// User login — tray apps, daemons.
    UserLogin,
    /// After desktop is idle for a few seconds — lower priority.
    Deferred,
}

/// Condition for whether an item should start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartCondition {
    /// Always start.
    Always,
    /// Only when on AC power (skip on battery).
    AcPower,
    /// Only when network is available.
    NetworkAvailable,
    /// Only on first login after boot.
    FirstLoginOnly,
}

/// Impact level for user display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    /// Minimal startup time impact.
    Low,
    /// Moderate impact (< 500ms).
    Medium,
    /// Significant impact (> 500ms).
    High,
    /// Unknown.
    Unknown,
}

/// An autostart item.
#[derive(Debug, Clone)]
pub struct AutostartItem {
    /// Unique ID.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Program/command to execute.
    pub command: String,
    /// Arguments.
    pub arguments: String,
    /// Start phase.
    pub phase: StartPhase,
    /// Start condition.
    pub condition: StartCondition,
    /// Delay in milliseconds after phase trigger.
    pub delay_ms: u32,
    /// Ordering within the phase (lower = earlier).
    pub order: u32,
    /// Whether enabled.
    pub enabled: bool,
    /// Whether this is a system item (cannot be removed by user).
    pub system: bool,
    /// Startup impact assessment.
    pub impact: Impact,
    /// User ID this item belongs to (0 = all users).
    pub uid: u64,
    /// Description of what this does.
    pub description: String,
    /// Number of times launched.
    pub launch_count: u64,
    /// Last launch timestamp (ns).
    pub last_launch_ns: u64,
    /// Average launch duration (ms).
    pub avg_duration_ms: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    items: Vec<AutostartItem>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    items: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Item management
// ---------------------------------------------------------------------------

/// Add an autostart item.
pub fn add_item(
    name: &str,
    command: &str,
    phase: StartPhase,
    uid: u64,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.items.len() >= 256 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.items.push(AutostartItem {
        id,
        name: String::from(name),
        command: String::from(command),
        arguments: String::new(),
        phase,
        condition: StartCondition::Always,
        delay_ms: 0,
        order: 100, // default middle priority
        enabled: true,
        system: false,
        impact: Impact::Unknown,
        uid,
        description: String::new(),
        launch_count: 0,
        last_launch_ns: 0,
        avg_duration_ms: 0,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove an item (system items cannot be removed).
pub fn remove_item(item_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    if item.system {
        return Err(KernelError::PermissionDenied);
    }
    state.items.retain(|i| i.id != item_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get item by ID.
pub fn get_item(item_id: u64) -> KernelResult<AutostartItem> {
    STATE.lock().items.iter().find(|i| i.id == item_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all items.
pub fn list_items() -> Vec<AutostartItem> {
    STATE.lock().items.clone()
}

/// Get items for a specific user (uid=0 items included for all users),
/// sorted by phase then order.
pub fn items_for_user(uid: u64) -> Vec<AutostartItem> {
    let state = STATE.lock();
    let mut items: Vec<AutostartItem> = state.items.iter()
        .filter(|i| i.enabled && (i.uid == 0 || i.uid == uid))
        .cloned()
        .collect();
    items.sort_by(|a, b| {
        let phase_ord = (a.phase as u32).cmp(&(b.phase as u32));
        if phase_ord != core::cmp::Ordering::Equal { phase_ord }
        else { a.order.cmp(&b.order) }
    });
    items
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Enable or disable an item.
pub fn set_enabled(item_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.enabled = enabled;
    state.changes += 1;
    Ok(())
}

/// Set delay.
pub fn set_delay(item_id: u64, delay_ms: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.delay_ms = delay_ms;
    state.changes += 1;
    Ok(())
}

/// Set order within phase (lower = earlier).
pub fn set_order(item_id: u64, order: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.order = order;
    state.changes += 1;
    Ok(())
}

/// Set phase.
pub fn set_phase(item_id: u64, phase: StartPhase) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.phase = phase;
    state.changes += 1;
    Ok(())
}

/// Set condition.
pub fn set_condition(item_id: u64, condition: StartCondition) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.condition = condition;
    state.changes += 1;
    Ok(())
}

/// Set arguments.
pub fn set_arguments(item_id: u64, args: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.arguments = String::from(args);
    state.changes += 1;
    Ok(())
}

/// Set description.
pub fn set_description(item_id: u64, desc: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.description = String::from(desc);
    state.changes += 1;
    Ok(())
}

/// Set impact level.
pub fn set_impact(item_id: u64, impact: Impact) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.impact = impact;
    state.changes += 1;
    Ok(())
}

/// Record a launch event (called by session manager).
pub fn record_launch(item_id: u64, duration_ms: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let item = state.items.iter_mut().find(|i| i.id == item_id)
        .ok_or(KernelError::NotFound)?;
    item.launch_count += 1;
    item.last_launch_ns = crate::hpet::elapsed_ns();
    // Running average.
    if item.avg_duration_ms == 0 {
        item.avg_duration_ms = duration_ms;
    } else {
        item.avg_duration_ms = (item.avg_duration_ms + duration_ms) / 2;
    }
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_system_item(state: &mut State, name: &str, cmd: &str, phase: StartPhase,
    order: u32, delay: u32, desc: &str)
{
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.items.push(AutostartItem {
        id,
        name: String::from(name),
        command: String::from(cmd),
        arguments: String::new(),
        phase,
        condition: StartCondition::Always,
        delay_ms: delay,
        order,
        enabled: true,
        system: true,
        impact: Impact::Low,
        uid: 0, // all users
        description: String::from(desc),
        launch_count: 0,
        last_launch_ns: 0,
        avg_duration_ms: 0,
    });
}

/// Initialise with default system startup items.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.items.is_empty() {
        return;
    }

    add_system_item(&mut state, "Compositor", "/usr/bin/compositor",
        StartPhase::SystemService, 10, 0, "Display compositor");
    add_system_item(&mut state, "Window Manager", "/usr/bin/wm",
        StartPhase::DesktopReady, 10, 0, "Window manager and taskbar");
    add_system_item(&mut state, "System Tray", "/usr/bin/systray",
        StartPhase::DesktopReady, 20, 100, "System tray notifications");
    add_system_item(&mut state, "Network Manager", "/usr/bin/netmgr",
        StartPhase::SystemService, 20, 0, "Network connection manager");
    add_system_item(&mut state, "Sound Server", "/usr/bin/soundsrv",
        StartPhase::SystemService, 30, 0, "Audio mixing daemon");
    add_system_item(&mut state, "File Indexer", "/usr/bin/findex",
        StartPhase::Deferred, 50, 5000, "Background file search indexer");

    state.changes += 1;
}

/// Return (item_count, enabled_count, system_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.items.len();
    let enabled = state.items.iter().filter(|i| i.enabled).count();
    let system = state.items.iter().filter(|i| i.system).count();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, enabled, system, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.items.clear();
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

    // Test 1: add items.
    serial_println!("autostart::self_test 1: add items");
    let a1 = add_item("App1", "/bin/app1", StartPhase::UserLogin, 1000)?;
    let a2 = add_item("App2", "/bin/app2", StartPhase::Deferred, 1000)?;
    let a3 = add_item("Daemon", "/bin/daemon", StartPhase::SystemService, 0)?;
    assert_eq!(list_items().len(), 3);

    // Test 2: user filtering and ordering.
    serial_println!("autostart::self_test 2: user filtering");
    let user_items = items_for_user(1000);
    assert_eq!(user_items.len(), 3); // a1, a2, and a3 (uid=0 for all users)
    // System service should come first.
    assert_eq!(user_items[0].id, a3);

    // Test 3: enable/disable.
    serial_println!("autostart::self_test 3: enable/disable");
    set_enabled(a2, false)?;
    let user_items = items_for_user(1000);
    assert_eq!(user_items.len(), 2); // a2 disabled

    // Test 4: configure.
    serial_println!("autostart::self_test 4: configure");
    set_delay(a1, 2000)?;
    set_order(a1, 5)?;
    set_condition(a1, StartCondition::AcPower)?;
    set_arguments(a1, "--minimized")?;
    set_description(a1, "My test app")?;
    let item = get_item(a1)?;
    assert_eq!(item.delay_ms, 2000);
    assert_eq!(item.order, 5);
    assert_eq!(item.condition, StartCondition::AcPower);
    assert_eq!(item.arguments, "--minimized");

    // Test 5: record launches.
    serial_println!("autostart::self_test 5: launch recording");
    record_launch(a1, 150)?;
    record_launch(a1, 250)?;
    let item = get_item(a1)?;
    assert_eq!(item.launch_count, 2);
    assert_eq!(item.avg_duration_ms, 200); // (150+250)/2

    // Test 6: remove (non-system only).
    serial_println!("autostart::self_test 6: remove");
    remove_item(a1)?;
    assert_eq!(list_items().len(), 2);
    // a3 is not system in this test, but let's verify with init_defaults.
    clear_all();
    init_defaults();
    let sys_items = list_items();
    assert!(sys_items.len() >= 6);
    assert!(remove_item(sys_items[0].id).is_err()); // system item

    // Test 7: impact assessment.
    serial_println!("autostart::self_test 7: impact");
    clear_all();
    let a = add_item("TestApp", "/bin/test", StartPhase::UserLogin, 1000)?;
    set_impact(a, Impact::High)?;
    let item = get_item(a)?;
    assert_eq!(item.impact, Impact::High);

    clear_all();
    serial_println!("autostart::self_test: all 7 tests passed");
    Ok(())
}
