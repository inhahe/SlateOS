//! Pinned Apps — taskbar and start menu pinned application management.
//!
//! Manages which apps are pinned to the taskbar and start menu,
//! their order, and grouping preferences.
//!
//! ## Architecture
//!
//! ```text
//! User pins app
//!   → pinnedapps::pin(location, app) → add to pinned list
//!   → pinnedapps::unpin(location, app) → remove from list
//!   → pinnedapps::reorder(location, app, pos) → change position
//!
//! Integration:
//!   → taskbar (taskbar pin list)
//!   → startmenu (start menu pin list)
//!   → appregistry (app identity)
//!   → contextmenu (pin/unpin menu items)
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

/// Pin location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinLocation {
    Taskbar,
    StartMenu,
    Desktop,
}

impl PinLocation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Taskbar => "Taskbar",
            Self::StartMenu => "Start Menu",
            Self::Desktop => "Desktop",
        }
    }
}

/// A pinned app entry.
#[derive(Debug, Clone)]
pub struct PinnedApp {
    pub app_name: String,
    pub display_name: String,
    pub icon_path: String,
    pub exec_path: String,
    pub location: PinLocation,
    pub position: u32,
    pub group: String,
    pub launch_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PINS: usize = 100;

struct State {
    pins: Vec<PinnedApp>,
    total_pins: u64,
    total_unpins: u64,
    total_launches: u64,
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
    *guard = Some(State {
        pins: alloc::vec![
            PinnedApp { app_name: String::from("files"), display_name: String::from("Files"), icon_path: String::from("/sys/icons/files.png"), exec_path: String::from("/usr/bin/files"), location: PinLocation::Taskbar, position: 0, group: String::new(), launch_count: 0 },
            PinnedApp { app_name: String::from("browser"), display_name: String::from("Web Browser"), icon_path: String::from("/sys/icons/browser.png"), exec_path: String::from("/usr/bin/browser"), location: PinLocation::Taskbar, position: 1, group: String::new(), launch_count: 0 },
            PinnedApp { app_name: String::from("terminal"), display_name: String::from("Terminal"), icon_path: String::from("/sys/icons/terminal.png"), exec_path: String::from("/usr/bin/terminal"), location: PinLocation::Taskbar, position: 2, group: String::new(), launch_count: 0 },
            PinnedApp { app_name: String::from("settings"), display_name: String::from("Settings"), icon_path: String::from("/sys/icons/settings.png"), exec_path: String::from("/usr/bin/settings"), location: PinLocation::StartMenu, position: 0, group: String::from("System"), launch_count: 0 },
        ],
        total_pins: 4,
        total_unpins: 0,
        total_launches: 0,
        ops: 0,
    });
}

/// Pin an app.
pub fn pin(location: PinLocation, app_name: &str, display_name: &str, exec_path: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.pins.len() >= MAX_PINS {
            return Err(KernelError::ResourceExhausted);
        }
        // Check for duplicate in same location.
        if state.pins.iter().any(|p| p.app_name == app_name && p.location == location) {
            return Err(KernelError::AlreadyExists);
        }
        // Find max position in location.
        let max_pos = state.pins.iter()
            .filter(|p| p.location == location)
            .map(|p| p.position)
            .max()
            .unwrap_or(0);
        state.pins.push(PinnedApp {
            app_name: String::from(app_name),
            display_name: String::from(display_name),
            icon_path: String::new(),
            exec_path: String::from(exec_path),
            location,
            position: max_pos + 1,
            group: String::new(),
            launch_count: 0,
        });
        state.total_pins += 1;
        Ok(())
    })
}

/// Unpin an app.
pub fn unpin(location: PinLocation, app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.pins.len();
        state.pins.retain(|p| !(p.app_name == app_name && p.location == location));
        if state.pins.len() == before {
            return Err(KernelError::NotFound);
        }
        state.total_unpins += 1;
        Ok(())
    })
}

/// Move app to a new position.
pub fn reorder(location: PinLocation, app_name: &str, new_position: u32) -> KernelResult<()> {
    with_state(|state| {
        let pin = state.pins.iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.position = new_position;
        Ok(())
    })
}

/// Set group for a pinned app.
pub fn set_group(location: PinLocation, app_name: &str, group: &str) -> KernelResult<()> {
    with_state(|state| {
        let pin = state.pins.iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.group = String::from(group);
        Ok(())
    })
}

/// Record a launch.
pub fn record_launch(app_name: &str) -> KernelResult<u64> {
    with_state(|state| {
        let mut count = 0u64;
        for pin in state.pins.iter_mut().filter(|p| p.app_name == app_name) {
            pin.launch_count += 1;
            count = pin.launch_count;
        }
        if count > 0 {
            state.total_launches += 1;
        }
        Ok(count)
    })
}

/// List pinned apps for a location, sorted by position.
pub fn list_pins(location: PinLocation) -> Vec<PinnedApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut pins: Vec<PinnedApp> = s.pins.iter()
            .filter(|p| p.location == location)
            .cloned()
            .collect();
        pins.sort_by_key(|p| p.position);
        pins
    })
}

/// List all pinned apps.
pub fn list_all() -> Vec<PinnedApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.pins.clone())
}

/// Check if an app is pinned in a location.
pub fn is_pinned(location: PinLocation, app_name: &str) -> bool {
    STATE.lock().as_ref().is_some_and(|s| {
        s.pins.iter().any(|p| p.app_name == app_name && p.location == location)
    })
}

/// Statistics: (total_count, taskbar_count, start_count, total_launches, ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let taskbar = s.pins.iter().filter(|p| p.location == PinLocation::Taskbar).count();
            let start = s.pins.iter().filter(|p| p.location == PinLocation::StartMenu).count();
            (s.pins.len(), taskbar, start, s.total_launches, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pinnedapps::self_test() — running tests...");
    init_defaults();

    // 1: Default pins.
    let all = list_all();
    assert_eq!(all.len(), 4);
    crate::serial_println!("  [1/8] default pins: OK");

    // 2: Taskbar pins.
    let taskbar = list_pins(PinLocation::Taskbar);
    assert_eq!(taskbar.len(), 3);
    assert_eq!(taskbar[0].app_name, "files");
    crate::serial_println!("  [2/8] taskbar pins: OK");

    // 3: Pin new app.
    pin(PinLocation::Taskbar, "editor", "Text Editor", "/usr/bin/editor").expect("pin");
    assert!(is_pinned(PinLocation::Taskbar, "editor"));
    assert_eq!(list_pins(PinLocation::Taskbar).len(), 4);
    crate::serial_println!("  [3/8] pin: OK");

    // 4: Duplicate rejection.
    assert!(pin(PinLocation::Taskbar, "editor", "Editor", "/usr/bin/editor").is_err());
    crate::serial_println!("  [4/8] duplicate rejection: OK");

    // 5: Unpin.
    unpin(PinLocation::Taskbar, "editor").expect("unpin");
    assert!(!is_pinned(PinLocation::Taskbar, "editor"));
    crate::serial_println!("  [5/8] unpin: OK");

    // 6: Reorder.
    reorder(PinLocation::Taskbar, "terminal", 0).expect("reorder");
    let taskbar = list_pins(PinLocation::Taskbar);
    assert_eq!(taskbar[0].app_name, "terminal");
    crate::serial_println!("  [6/8] reorder: OK");

    // 7: Launch tracking.
    let count = record_launch("files").expect("launch");
    assert_eq!(count, 1);
    crate::serial_println!("  [7/8] launch: OK");

    // 8: Stats.
    let (total, tb, sm, launches, ops) = stats();
    assert_eq!(total, 4);
    assert_eq!(tb, 3);
    assert_eq!(sm, 1);
    assert_eq!(launches, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pinnedapps::self_test() — all 8 tests passed");
}
