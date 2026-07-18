//! Hot Corners — screen corner trigger actions.
//!
//! Configures actions triggered when the mouse pointer reaches a screen corner
//! (top-left, top-right, bottom-left, bottom-right) with optional modifier keys.
//!
//! ## Architecture
//!
//! ```text
//! Mouse hits corner
//!   → hotcorners::trigger(corner) → execute assigned action
//!
//! Configuration
//!   → hotcorners::set_action(corner, action)
//!   → hotcorners::set_delay(corner, ms)
//!
//! Integration:
//!   → vdesktop (virtual desktop switching)
//!   → screensaver (activate)
//!   → winsnap (window management)
//!   → quicksettings (panel toggle)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Screen corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top-Left",
            Self::TopRight => "Top-Right",
            Self::BottomLeft => "Bottom-Left",
            Self::BottomRight => "Bottom-Right",
        }
    }

    pub fn all() -> [Corner; 4] {
        [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight]
    }
}

/// Action triggered by a hot corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerAction {
    /// No action.
    None,
    /// Show all windows / mission control.
    ShowAllWindows,
    /// Show desktop.
    ShowDesktop,
    /// Start screensaver.
    StartScreensaver,
    /// Lock screen.
    LockScreen,
    /// Open notifications panel.
    OpenNotifications,
    /// Open quick settings.
    OpenQuickSettings,
    /// Switch to next virtual desktop.
    NextDesktop,
    /// Switch to previous virtual desktop.
    PrevDesktop,
    /// Open app launcher / start menu.
    OpenLauncher,
    /// Disable screen corner (hot corner still active but ignored in fullscreen).
    DisableScreen,
}

impl CornerAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::ShowAllWindows => "Show All Windows",
            Self::ShowDesktop => "Show Desktop",
            Self::StartScreensaver => "Start Screensaver",
            Self::LockScreen => "Lock Screen",
            Self::OpenNotifications => "Notifications",
            Self::OpenQuickSettings => "Quick Settings",
            Self::NextDesktop => "Next Desktop",
            Self::PrevDesktop => "Previous Desktop",
            Self::OpenLauncher => "App Launcher",
            Self::DisableScreen => "Disable Screen",
        }
    }
}

/// Hot corner configuration.
#[derive(Debug, Clone)]
pub struct CornerConfig {
    pub corner: Corner,
    pub action: CornerAction,
    /// Delay in milliseconds before triggering (0 = instant).
    pub delay_ms: u32,
    /// Require modifier key (ctrl/alt/shift).
    pub require_modifier: bool,
    pub enabled: bool,
    pub trigger_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    corners: [CornerConfig; 4],
    global_enabled: bool,
    total_triggers: u64,
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

fn corner_index(c: Corner) -> usize {
    match c {
        Corner::TopLeft => 0,
        Corner::TopRight => 1,
        Corner::BottomLeft => 2,
        Corner::BottomRight => 3,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        corners: [
            CornerConfig { corner: Corner::TopLeft, action: CornerAction::ShowAllWindows, delay_ms: 300, require_modifier: false, enabled: true, trigger_count: 0 },
            CornerConfig { corner: Corner::TopRight, action: CornerAction::ShowDesktop, delay_ms: 300, require_modifier: false, enabled: true, trigger_count: 0 },
            CornerConfig { corner: Corner::BottomLeft, action: CornerAction::OpenLauncher, delay_ms: 0, require_modifier: false, enabled: true, trigger_count: 0 },
            CornerConfig { corner: Corner::BottomRight, action: CornerAction::None, delay_ms: 300, require_modifier: false, enabled: false, trigger_count: 0 },
        ],
        global_enabled: true,
        total_triggers: 0,
        ops: 0,
    });
}

/// Set action for a corner.
pub fn set_action(corner: Corner, action: CornerAction) -> KernelResult<()> {
    with_state(|state| {
        let idx = corner_index(corner);
        state.corners[idx].action = action;
        state.corners[idx].enabled = action != CornerAction::None;
        Ok(())
    })
}

/// Set delay for a corner.
pub fn set_delay(corner: Corner, delay_ms: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = corner_index(corner);
        state.corners[idx].delay_ms = delay_ms.min(5000);
        Ok(())
    })
}

/// Set modifier requirement.
pub fn set_require_modifier(corner: Corner, require: bool) -> KernelResult<()> {
    with_state(|state| {
        let idx = corner_index(corner);
        state.corners[idx].require_modifier = require;
        Ok(())
    })
}

/// Enable/disable a corner.
pub fn set_enabled(corner: Corner, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let idx = corner_index(corner);
        state.corners[idx].enabled = enabled;
        Ok(())
    })
}

/// Trigger a hot corner action. Returns the action if triggered.
pub fn trigger(corner: Corner) -> KernelResult<CornerAction> {
    with_state(|state| {
        if !state.global_enabled {
            return Ok(CornerAction::None);
        }
        let idx = corner_index(corner);
        let cfg = &mut state.corners[idx];
        if !cfg.enabled || cfg.action == CornerAction::None {
            return Ok(CornerAction::None);
        }
        cfg.trigger_count += 1;
        state.total_triggers += 1;
        Ok(cfg.action)
    })
}

/// Enable/disable hot corners globally.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// Get config for a specific corner.
pub fn get_corner(corner: Corner) -> Option<CornerConfig> {
    STATE.lock().as_ref().map(|s| s.corners[corner_index(corner)].clone())
}

/// Get all corner configs.
pub fn get_all() -> Vec<CornerConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.corners.to_vec())
}

/// Statistics: (enabled_count, total_triggers, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let enabled = s.corners.iter().filter(|c| c.enabled).count();
            (enabled, s.total_triggers, s.ops)
        }
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("hotcorners::self_test() — running tests...");
    init_defaults();

    // 1: Default config.
    let all = get_all();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].action, CornerAction::ShowAllWindows);
    assert_eq!(all[1].action, CornerAction::ShowDesktop);
    assert_eq!(all[2].action, CornerAction::OpenLauncher);
    assert_eq!(all[3].action, CornerAction::None);
    crate::serial_println!("  [1/8] default config: OK");

    // 2: Trigger active corner.
    let action = trigger(Corner::TopLeft).expect("trigger");
    assert_eq!(action, CornerAction::ShowAllWindows);
    crate::serial_println!("  [2/8] trigger: OK");

    // 3: Trigger disabled corner.
    let action = trigger(Corner::BottomRight).expect("trigger2");
    assert_eq!(action, CornerAction::None);
    crate::serial_println!("  [3/8] disabled corner: OK");

    // 4: Set action.
    set_action(Corner::BottomRight, CornerAction::LockScreen).expect("set");
    let cfg = get_corner(Corner::BottomRight).expect("corner");
    assert_eq!(cfg.action, CornerAction::LockScreen);
    assert!(cfg.enabled);
    crate::serial_println!("  [4/8] set action: OK");

    // 5: Set delay.
    set_delay(Corner::TopLeft, 500).expect("delay");
    let cfg = get_corner(Corner::TopLeft).expect("corner2");
    assert_eq!(cfg.delay_ms, 500);
    crate::serial_println!("  [5/8] delay: OK");

    // 6: Modifier requirement.
    set_require_modifier(Corner::TopRight, true).expect("mod");
    let cfg = get_corner(Corner::TopRight).expect("corner3");
    assert!(cfg.require_modifier);
    crate::serial_println!("  [6/8] modifier: OK");

    // 7: Global disable.
    set_global_enabled(false).expect("global");
    let action = trigger(Corner::TopLeft).expect("trigger3");
    assert_eq!(action, CornerAction::None);
    set_global_enabled(true).expect("global2");
    crate::serial_println!("  [7/8] global disable: OK");

    // 8: Stats.
    let (enabled, triggers, ops) = stats();
    assert_eq!(enabled, 4); // all 4 enabled now
    assert!(triggers >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("hotcorners::self_test() — all 8 tests passed");
}
