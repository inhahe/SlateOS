//! Virtual desktops (workspaces) — multiple desktop surfaces.
//!
//! Provides multiple virtual desktops so the user can organize windows
//! into separate workspaces.  The compositor switches between desktops;
//! windows belong to exactly one desktop (or can be pinned to all).
//!
//! ## Design Reference
//!
//! design.txt line 1321: "change desktops (if we allow more than one desktop)"
//! design.txt line 1321: hotkey to switch desktops
//!
//! ## Architecture
//!
//! ```text
//! Compositor / WM
//!   → vdesktop::switch(id)      // change visible desktop
//!   → vdesktop::move_window()   // move window between desktops
//!
//! Hotkey manager
//!   → vdesktop::next() / previous()
//!
//! Taskbar
//!   → vdesktop::list()          // desktop switcher widget
//!   → vdesktop::current()       // highlight active
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of virtual desktops.
const MAX_DESKTOPS: usize = 32;

/// Maximum windows tracked per desktop.
const MAX_WINDOWS_PER_DESKTOP: usize = 512;

/// Maximum pinned (show-on-all-desktops) windows.
const MAX_PINNED: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single virtual desktop workspace.
#[derive(Debug, Clone)]
pub struct Desktop {
    /// Unique desktop id (1-based, 0 = invalid).
    pub id: u32,
    /// User-visible name (e.g., "Work", "Personal").
    pub name: String,
    /// Window IDs on this desktop.
    pub windows: Vec<u64>,
    /// Optional wallpaper override path (empty = use global).
    pub wallpaper: String,
    /// Whether this desktop is the active/visible one.
    pub active: bool,
}

impl Desktop {
    fn new(id: u32, name: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            windows: Vec::new(),
            wallpaper: String::new(),
            active: false,
        }
    }
}

/// Which desktops are shown during overview/expose mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchAnimation {
    /// Instant switch with no animation.
    None,
    /// Slide horizontally.
    Slide,
    /// Fade through black.
    Fade,
    /// Zoom out showing all desktops.
    Overview,
}

impl SwitchAnimation {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "slide" => Some(Self::Slide),
            "fade" => Some(Self::Fade),
            "overview" => Some(Self::Overview),
            _ => None,
        }
    }

    /// Human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Slide => "slide",
            Self::Fade => "fade",
            Self::Overview => "overview",
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// All desktops in order.
    desktops: Vec<Desktop>,
    /// ID of the currently active desktop.
    current: u32,
    /// Next desktop id to assign.
    next_id: u32,
    /// Window IDs pinned to all desktops.
    pinned: Vec<u64>,
    /// Switch animation style.
    animation: SwitchAnimation,
    /// Whether to wrap around at edges (last→first, first→last).
    wrap_around: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            desktops: Vec::new(),
            current: 0,
            next_id: 1,
            pinned: Vec::new(),
            animation: SwitchAnimation::Slide,
            wrap_around: true,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static SWITCH_COUNT: AtomicU64 = AtomicU64::new(0);
static MOVE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Desktop management
// ---------------------------------------------------------------------------

/// Create a new virtual desktop with the given name.
///
/// Returns the new desktop's ID.
pub fn create(name: &str) -> KernelResult<u32> {
    let mut state = STATE.lock();
    if state.desktops.len() >= MAX_DESKTOPS {
        return Err(KernelError::ResourceExhausted);
    }
    let id = state.next_id;
    state.next_id = state.next_id.wrapping_add(1);
    let mut desktop = Desktop::new(id, name);
    // First desktop created becomes active.
    if state.desktops.is_empty() {
        desktop.active = true;
        state.current = id;
    }
    state.desktops.push(desktop);
    Ok(id)
}

/// Remove a virtual desktop.
///
/// All windows on it are moved to the adjacent desktop.
/// The last desktop cannot be removed.
pub fn remove(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.desktops.len() <= 1 {
        return Err(KernelError::InvalidArgument);
    }
    let idx = state.desktops.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    // Collect orphaned windows.
    let orphans: Vec<u64> = state.desktops[idx].windows.clone();

    // Pick the neighbor desktop to receive orphans.
    let neighbor_idx = if idx > 0 { idx - 1 } else { 1 };
    let neighbor_id = state.desktops.get(neighbor_idx)
        .map(|d| d.id)
        .ok_or(KernelError::InternalError)?;

    // Move orphans.
    if let Some(dest) = state.desktops.iter_mut().find(|d| d.id == neighbor_id) {
        for w in &orphans {
            if dest.windows.len() < MAX_WINDOWS_PER_DESKTOP && !dest.windows.contains(w) {
                dest.windows.push(*w);
            }
        }
    }

    let was_active = state.desktops[idx].active;
    state.desktops.remove(idx);

    // If we removed the active desktop, activate the neighbor.
    if was_active {
        let new_active = state.desktops.get(idx.min(state.desktops.len().saturating_sub(1)));
        if let Some(d) = new_active {
            let new_id = d.id;
            if let Some(d) = state.desktops.iter_mut().find(|d| d.id == new_id) {
                d.active = true;
                state.current = new_id;
            }
        }
    }

    Ok(())
}

/// Rename a desktop.
pub fn rename(id: u32, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let d = state.desktops.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    d.name = String::from(name);
    Ok(())
}

/// Get a desktop by id.
pub fn get(id: u32) -> Option<Desktop> {
    let state = STATE.lock();
    state.desktops.iter().find(|d| d.id == id).cloned()
}

/// List all desktops in order.
pub fn list() -> Vec<Desktop> {
    STATE.lock().desktops.clone()
}

/// Get the currently active desktop ID.
pub fn current() -> u32 {
    STATE.lock().current
}

/// Get the currently active desktop.
pub fn current_desktop() -> Option<Desktop> {
    let state = STATE.lock();
    let cur = state.current;
    state.desktops.iter().find(|d| d.id == cur).cloned()
}

// ---------------------------------------------------------------------------
// Switching
// ---------------------------------------------------------------------------

/// Switch to a specific desktop by id.
pub fn switch(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let old = state.current;
    if old == id {
        return Ok(()); // Already there.
    }
    // Deactivate old.
    if let Some(d) = state.desktops.iter_mut().find(|d| d.id == old) {
        d.active = false;
    }
    // Activate new.
    let d = state.desktops.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    d.active = true;
    state.current = id;
    SWITCH_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Switch to the next desktop (wraps if enabled).
pub fn next() -> KernelResult<()> {
    let state = STATE.lock();
    if state.desktops.is_empty() {
        return Err(KernelError::NotFound);
    }
    let cur = state.current;
    let idx = state.desktops.iter().position(|d| d.id == cur).unwrap_or(0);
    let next_idx = if idx + 1 < state.desktops.len() {
        idx + 1
    } else if state.wrap_around {
        0
    } else {
        return Ok(()); // At the end, no wrap.
    };
    let next_id = state.desktops[next_idx].id;
    drop(state);
    switch(next_id)
}

/// Switch to the previous desktop (wraps if enabled).
pub fn previous() -> KernelResult<()> {
    let state = STATE.lock();
    if state.desktops.is_empty() {
        return Err(KernelError::NotFound);
    }
    let cur = state.current;
    let idx = state.desktops.iter().position(|d| d.id == cur).unwrap_or(0);
    let prev_idx = if idx > 0 {
        idx - 1
    } else if state.wrap_around {
        state.desktops.len().saturating_sub(1)
    } else {
        return Ok(()); // At the start, no wrap.
    };
    let prev_id = state.desktops[prev_idx].id;
    drop(state);
    switch(prev_id)
}

/// Reorder a desktop to a new position (0-based).
pub fn reorder(id: u32, new_pos: usize) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.desktops.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    let clamped = new_pos.min(state.desktops.len().saturating_sub(1));
    let d = state.desktops.remove(idx);
    state.desktops.insert(clamped, d);
    Ok(())
}

// ---------------------------------------------------------------------------
// Window management
// ---------------------------------------------------------------------------

/// Add a window to a desktop.
pub fn add_window(desktop_id: u32, window_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let d = state.desktops.iter_mut().find(|d| d.id == desktop_id)
        .ok_or(KernelError::NotFound)?;
    if d.windows.len() >= MAX_WINDOWS_PER_DESKTOP {
        return Err(KernelError::ResourceExhausted);
    }
    if !d.windows.contains(&window_id) {
        d.windows.push(window_id);
    }
    Ok(())
}

/// Remove a window from a desktop.
pub fn remove_window(desktop_id: u32, window_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let d = state.desktops.iter_mut().find(|d| d.id == desktop_id)
        .ok_or(KernelError::NotFound)?;
    d.windows.retain(|&w| w != window_id);
    // Also unpin if pinned.
    state.pinned.retain(|&w| w != window_id);
    Ok(())
}

/// Remove a window from all desktops (when the window is destroyed).
pub fn remove_window_everywhere(window_id: u64) {
    let mut state = STATE.lock();
    for d in &mut state.desktops {
        d.windows.retain(|&w| w != window_id);
    }
    state.pinned.retain(|&w| w != window_id);
}

/// Move a window from one desktop to another.
pub fn move_window(window_id: u64, from: u32, to: u32) -> KernelResult<()> {
    if from == to {
        return Ok(());
    }
    let mut state = STATE.lock();

    // Remove from source.
    let src = state.desktops.iter_mut().find(|d| d.id == from)
        .ok_or(KernelError::NotFound)?;
    if !src.windows.contains(&window_id) {
        return Err(KernelError::NotFound);
    }
    src.windows.retain(|&w| w != window_id);

    // Add to destination.
    let dst = state.desktops.iter_mut().find(|d| d.id == to)
        .ok_or(KernelError::NotFound)?;
    if dst.windows.len() >= MAX_WINDOWS_PER_DESKTOP {
        return Err(KernelError::ResourceExhausted);
    }
    if !dst.windows.contains(&window_id) {
        dst.windows.push(window_id);
    }

    MOVE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get which desktop a window is on (returns first match).
pub fn desktop_of(window_id: u64) -> Option<u32> {
    let state = STATE.lock();
    // Check pinned first.
    if state.pinned.contains(&window_id) {
        return Some(state.current); // Pinned = always on current.
    }
    for d in &state.desktops {
        if d.windows.contains(&window_id) {
            return Some(d.id);
        }
    }
    None
}

/// Get all windows visible on a desktop (its own + pinned).
pub fn visible_windows(desktop_id: u32) -> Vec<u64> {
    let state = STATE.lock();
    let d = state.desktops.iter().find(|d| d.id == desktop_id);
    let mut result = match d {
        Some(d) => d.windows.clone(),
        None => Vec::new(),
    };
    // Add pinned windows not already present.
    for &w in &state.pinned {
        if !result.contains(&w) {
            result.push(w);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Pinning (show on all desktops)
// ---------------------------------------------------------------------------

/// Pin a window to appear on all desktops.
pub fn pin(window_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.pinned.len() >= MAX_PINNED {
        return Err(KernelError::ResourceExhausted);
    }
    if !state.pinned.contains(&window_id) {
        state.pinned.push(window_id);
    }
    Ok(())
}

/// Unpin a window (show only on its assigned desktop).
pub fn unpin(window_id: u64) {
    let mut state = STATE.lock();
    state.pinned.retain(|&w| w != window_id);
}

/// Check if a window is pinned.
pub fn is_pinned(window_id: u64) -> bool {
    STATE.lock().pinned.contains(&window_id)
}

/// List all pinned windows.
pub fn pinned_windows() -> Vec<u64> {
    STATE.lock().pinned.clone()
}

// ---------------------------------------------------------------------------
// Per-desktop wallpaper
// ---------------------------------------------------------------------------

/// Set a per-desktop wallpaper override.
pub fn set_wallpaper(id: u32, path: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let d = state.desktops.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    d.wallpaper = String::from(path);
    Ok(())
}

/// Clear per-desktop wallpaper (use global).
pub fn clear_wallpaper(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let d = state.desktops.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    d.wallpaper.clear();
    Ok(())
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the switch animation style.
pub fn set_animation(anim: SwitchAnimation) {
    STATE.lock().animation = anim;
}

/// Get the current animation style.
pub fn animation() -> SwitchAnimation {
    STATE.lock().animation
}

/// Set whether switching wraps around.
pub fn set_wrap(wrap: bool) {
    STATE.lock().wrap_around = wrap;
}

/// Get wrap-around setting.
pub fn wrap_around() -> bool {
    STATE.lock().wrap_around
}

// ---------------------------------------------------------------------------
// Init / defaults
// ---------------------------------------------------------------------------

/// Create default desktops (Desktop 1, Desktop 2).
pub fn init_defaults() -> KernelResult<()> {
    let state = STATE.lock();
    if !state.desktops.is_empty() {
        return Ok(()); // Already initialized.
    }
    drop(state);
    let _ = create("Desktop 1")?;
    let _ = create("Desktop 2")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (desktop_count, window_count, pinned_count, switch_count, move_count).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let state = STATE.lock();
    let dc = state.desktops.len();
    let wc: usize = state.desktops.iter().map(|d| d.windows.len()).sum();
    let pc = state.pinned.len();
    (dc, wc, pc, SWITCH_COUNT.load(Ordering::Relaxed), MOVE_COUNT.load(Ordering::Relaxed))
}

/// Reset statistics counters.
pub fn reset_stats() {
    SWITCH_COUNT.store(0, Ordering::Relaxed);
    MOVE_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all state.
pub fn clear_all() {
    let mut state = STATE.lock();
    state.desktops.clear();
    state.current = 0;
    state.next_id = 1;
    state.pinned.clear();
    state.animation = SwitchAnimation::Slide;
    state.wrap_around = true;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the virtual desktop subsystem.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Create desktops.
    serial_println!("  vdesktop::self_test 1: create desktops");
    let d1 = create("Work")?;
    let d2 = create("Personal")?;
    let d3 = create("Gaming")?;
    assert_eq!(list().len(), 3);
    assert_eq!(current(), d1); // First created is active.

    // Test 2: Switch desktops.
    serial_println!("  vdesktop::self_test 2: switch");
    switch(d2)?;
    assert_eq!(current(), d2);
    next()?;
    assert_eq!(current(), d3);
    next()?; // Wraps to first.
    assert_eq!(current(), d1);
    previous()?; // Wraps to last.
    assert_eq!(current(), d3);

    // Test 3: Window management.
    serial_println!("  vdesktop::self_test 3: windows");
    add_window(d1, 100)?;
    add_window(d1, 101)?;
    add_window(d2, 200)?;
    assert_eq!(desktop_of(100), Some(d1));
    assert_eq!(desktop_of(200), Some(d2));
    assert_eq!(visible_windows(d1).len(), 2);

    // Test 4: Move window.
    serial_println!("  vdesktop::self_test 4: move window");
    move_window(100, d1, d2)?;
    assert_eq!(desktop_of(100), Some(d2));
    let d2_wins = visible_windows(d2);
    assert!(d2_wins.contains(&100));
    assert!(d2_wins.contains(&200));

    // Test 5: Pin window.
    serial_println!("  vdesktop::self_test 5: pinning");
    pin(101)?;
    assert!(is_pinned(101));
    // Pinned window visible on every desktop.
    assert!(visible_windows(d2).contains(&101));
    assert!(visible_windows(d3).contains(&101));
    unpin(101);
    assert!(!is_pinned(101));

    // Test 6: Remove desktop (orphans move to neighbor).
    serial_println!("  vdesktop::self_test 6: remove desktop");
    add_window(d3, 300)?;
    switch(d3)?;
    remove(d3)?;
    // Window 300 should have moved to neighbor.
    assert!(desktop_of(300).is_some());
    assert_eq!(list().len(), 2);

    // Test 7: Rename, wallpaper, config.
    serial_println!("  vdesktop::self_test 7: config");
    rename(d1, "Renamed")?;
    assert_eq!(get(d1).map(|d| d.name), Some(String::from("Renamed")));
    set_wallpaper(d1, "/wallpapers/nature.jpg")?;
    assert_eq!(get(d1).map(|d| d.wallpaper), Some(String::from("/wallpapers/nature.jpg")));
    clear_wallpaper(d1)?;
    assert_eq!(get(d1).map(|d| d.wallpaper), Some(String::new()));
    set_animation(SwitchAnimation::Fade);
    assert_eq!(animation(), SwitchAnimation::Fade);
    set_wrap(false);
    assert!(!wrap_around());

    let (dc, _wc, _pc, switches, moves) = stats();
    assert_eq!(dc, 2);
    assert!(switches > 0);
    assert!(moves > 0);

    clear_all();
    reset_stats();
    serial_println!("  vdesktop: all tests passed");
    Ok(())
}
