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

use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    /// Path to the pin's icon file, or the empty path if it has none.
    ///
    /// A `PathBuf`, not a `String`: a path may contain any byte except `/`
    /// and NUL. See design-decisions.md §261.
    pub icon_path: PathBuf,
    /// Path to the executable this pin launches.
    ///
    /// A `PathBuf` for the same reason, and here it matters most: a lossily
    /// decoded path does not merely display wrong, it launches a different
    /// binary or nothing at all.
    pub exec_path: PathBuf,
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
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        pins: alloc::vec![
            PinnedApp {
                app_name: String::from("files"),
                display_name: String::from("Files"),
                icon_path: PathBuf::from("/sys/icons/files.png"),
                exec_path: PathBuf::from("/usr/bin/files"),
                location: PinLocation::Taskbar,
                position: 0,
                group: String::new(),
                launch_count: 0
            },
            PinnedApp {
                app_name: String::from("browser"),
                display_name: String::from("Web Browser"),
                icon_path: PathBuf::from("/sys/icons/browser.png"),
                exec_path: PathBuf::from("/usr/bin/browser"),
                location: PinLocation::Taskbar,
                position: 1,
                group: String::new(),
                launch_count: 0
            },
            PinnedApp {
                app_name: String::from("terminal"),
                display_name: String::from("Terminal"),
                icon_path: PathBuf::from("/sys/icons/terminal.png"),
                exec_path: PathBuf::from("/usr/bin/terminal"),
                location: PinLocation::Taskbar,
                position: 2,
                group: String::new(),
                launch_count: 0
            },
            PinnedApp {
                app_name: String::from("settings"),
                display_name: String::from("Settings"),
                icon_path: PathBuf::from("/sys/icons/settings.png"),
                exec_path: PathBuf::from("/usr/bin/settings"),
                location: PinLocation::StartMenu,
                position: 0,
                group: String::from("System"),
                launch_count: 0
            },
        ],
        total_pins: 4,
        total_unpins: 0,
        total_launches: 0,
        ops: 0,
    });
}

/// Pin an app.
pub fn pin(
    location: PinLocation,
    app_name: &str,
    display_name: &str,
    exec_path: impl AsRef<Path>,
) -> KernelResult<()> {
    let exec_path = exec_path.as_ref();
    with_state(|state| {
        if state.pins.len() >= MAX_PINS {
            return Err(KernelError::ResourceExhausted);
        }
        // Check for duplicate in same location.
        if state
            .pins
            .iter()
            .any(|p| p.app_name == app_name && p.location == location)
        {
            return Err(KernelError::AlreadyExists);
        }
        // Find max position in location.
        let max_pos = state
            .pins
            .iter()
            .filter(|p| p.location == location)
            .map(|p| p.position)
            .max()
            .unwrap_or(0);
        state.pins.push(PinnedApp {
            app_name: String::from(app_name),
            display_name: String::from(display_name),
            icon_path: PathBuf::new(),
            exec_path: exec_path.to_path_buf(),
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
        state
            .pins
            .retain(|p| !(p.app_name == app_name && p.location == location));
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
        let pin = state
            .pins
            .iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.position = new_position;
        Ok(())
    })
}

/// Set group for a pinned app.
pub fn set_group(location: PinLocation, app_name: &str, group: &str) -> KernelResult<()> {
    with_state(|state| {
        let pin = state
            .pins
            .iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.group = String::from(group);
        Ok(())
    })
}

/// Set the icon file for a pinned app.
///
/// Only `init_defaults` used to fill `icon_path` in, so an app the user
/// pinned themselves was stuck with no icon and no way to give it one. Pass
/// the empty path to clear it.
///
/// # Errors
///
/// Returns [`KernelError::NotFound`] if no app of that name is pinned at
/// `location`.
pub fn set_icon(
    location: PinLocation,
    app_name: &str,
    icon_path: impl AsRef<Path>,
) -> KernelResult<()> {
    let icon_path = icon_path.as_ref();
    with_state(|state| {
        let pin = state
            .pins
            .iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.icon_path = icon_path.to_path_buf();
        Ok(())
    })
}

/// Set the executable a pinned app launches.
///
/// # Errors
///
/// Returns [`KernelError::NotFound`] if no app of that name is pinned at
/// `location`, and [`KernelError::InvalidArgument`] for the empty path, which
/// names no executable and would leave the pin unlaunchable.
pub fn set_exec(
    location: PinLocation,
    app_name: &str,
    exec_path: impl AsRef<Path>,
) -> KernelResult<()> {
    let exec_path = exec_path.as_ref();
    if exec_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let pin = state
            .pins
            .iter_mut()
            .find(|p| p.app_name == app_name && p.location == location)
            .ok_or(KernelError::NotFound)?;
        pin.exec_path = exec_path.to_path_buf();
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
        let mut pins: Vec<PinnedApp> = s
            .pins
            .iter()
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
        s.pins
            .iter()
            .any(|p| p.app_name == app_name && p.location == location)
    })
}

/// Statistics: (total_count, taskbar_count, start_count, total_launches, ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let taskbar = s
                .pins
                .iter()
                .filter(|p| p.location == PinLocation::Taskbar)
                .count();
            let start = s
                .pins
                .iter()
                .filter(|p| p.location == PinLocation::StartMenu)
                .count();
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

    // Reset at both ends. `init_defaults()` early-returns once the state
    // exists and `with_state` does not lazily initialise, so without this the
    // suite ran exactly once: on a second `pinnedapps test` the reorder in
    // test 6 and the launch count in test 7 are already in place, and test 7's
    // `assert_eq!(count, 1)` panics -- which in the kernel is a dead machine,
    // reachable by typing a shell command. `None` is what a fresh boot has:
    // nothing calls `init_defaults()` outside the `pinnedapps` commands.
    *STATE.lock() = None;
    init_defaults();

    // 1: Default pins.
    let all = list_all();
    assert_eq!(all.len(), 4);
    crate::serial_println!("  [1/9] default pins: OK");

    // 2: Taskbar pins.
    let taskbar = list_pins(PinLocation::Taskbar);
    assert_eq!(taskbar.len(), 3);
    assert_eq!(taskbar[0].app_name, "files");
    crate::serial_println!("  [2/9] taskbar pins: OK");

    // 3: Pin new app.
    pin(
        PinLocation::Taskbar,
        "editor",
        "Text Editor",
        "/usr/bin/editor",
    )
    .expect("pin");
    assert!(is_pinned(PinLocation::Taskbar, "editor"));
    assert_eq!(list_pins(PinLocation::Taskbar).len(), 4);
    crate::serial_println!("  [3/9] pin: OK");

    // 4: Duplicate rejection.
    assert!(pin(PinLocation::Taskbar, "editor", "Editor", "/usr/bin/editor").is_err());
    crate::serial_println!("  [4/9] duplicate rejection: OK");

    // 5: Unpin.
    unpin(PinLocation::Taskbar, "editor").expect("unpin");
    assert!(!is_pinned(PinLocation::Taskbar, "editor"));
    crate::serial_println!("  [5/9] unpin: OK");

    // 6: Reorder.
    reorder(PinLocation::Taskbar, "terminal", 0).expect("reorder");
    let taskbar = list_pins(PinLocation::Taskbar);
    assert_eq!(taskbar[0].app_name, "terminal");
    crate::serial_println!("  [6/9] reorder: OK");

    // 7: Launch tracking.
    let count = record_launch("files").expect("launch");
    assert_eq!(count, 1);
    crate::serial_println!("  [7/9] launch: OK");

    // 8: Stats.
    let (total, tb, sm, launches, ops) = stats();
    assert_eq!(total, 4);
    assert_eq!(tb, 3);
    assert_eq!(sm, 1);
    assert_eq!(launches, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: Non-UTF-8 executable and icon paths survive byte-exact (§261).
    //
    // A pin is a launcher: its `exec_path` is what gets run. Under the old
    // `String` typing an app installed at a path holding a byte with no UTF-8
    // spelling was pinned as a U+FFFD-bearing name, so clicking the pin ran a
    // different binary or nothing, and two such apps became indistinguishable.
    let raw_exec = Path::new(b"/opt/pl\xFFyer/bin/play");
    let raw_icon = Path::new(b"/sys/icons/pl\xFEyer.png");
    pin(PinLocation::Desktop, "rawapp", "Raw Path App", raw_exec).expect("pin raw");
    set_icon(PinLocation::Desktop, "rawapp", raw_icon).expect("set raw icon");
    let p = list_pins(PinLocation::Desktop)
        .into_iter()
        .find(|p| p.app_name == "rawapp")
        .expect("rawapp pinned");
    assert_eq!(
        p.exec_path.as_path().as_bytes(),
        &b"/opt/pl\xFFyer/bin/play"[..],
        "the executable path must round-trip byte-for-byte"
    );
    assert_eq!(
        p.icon_path.as_path().as_bytes(),
        &b"/sys/icons/pl\xFEyer.png"[..],
        "the icon path must round-trip byte-for-byte"
    );

    // `set_exec` must replace the path, and must refuse the empty one: a pin
    // with no executable is a button that does nothing.
    set_exec(
        PinLocation::Desktop,
        "rawapp",
        Path::new(b"/opt/pl\xFEyer/bin/play"),
    )
    .expect("set raw exec");
    let p = list_pins(PinLocation::Desktop)
        .into_iter()
        .find(|p| p.app_name == "rawapp")
        .expect("rawapp still pinned");
    assert_eq!(
        p.exec_path.as_path().as_bytes(),
        &b"/opt/pl\xFEyer/bin/play"[..],
        "paths differing only in a byte with no UTF-8 spelling must stay distinct"
    );
    assert!(set_exec(PinLocation::Desktop, "rawapp", "").is_err());
    assert!(set_icon(PinLocation::Desktop, "nosuchapp", raw_icon).is_err());

    unpin(PinLocation::Desktop, "rawapp").expect("unpin raw");
    crate::serial_println!("  [9/9] non-UTF-8 exec and icon paths: OK");

    // Back to the uninitialised state a fresh boot has, so the suite does not
    // leave its reorder and launch counts in a table the user then reads.
    *STATE.lock() = None;

    crate::serial_println!("pinnedapps::self_test() — all 9 tests passed");
}
