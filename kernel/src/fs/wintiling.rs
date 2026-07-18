//! Window Tiling — automatic window layout and tiling management.
//!
//! Provides tiling layouts (horizontal/vertical split, grid, master+stack),
//! workspace management, and keyboard-driven window arrangement.
//!
//! ## Architecture
//!
//! ```text
//! User triggers tiling
//!   → wintiling::tile(workspace, layout)
//!     → calculates positions for all windows
//!     → applies geometry to each window
//!
//! Window added/removed
//!   → wintiling::retile(workspace) → recalculates
//!
//! Integration:
//!   → winsnap (snapping to tile zones)
//!   → vdesktop (workspace assignment)
//!   → hotkeys (tiling keyboard shortcuts)
//!   → display (monitor geometry)
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

/// Tiling layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingLayout {
    /// No tiling (floating).
    Floating,
    /// Horizontal split (side by side).
    HorizontalSplit,
    /// Vertical split (stacked top/bottom).
    VerticalSplit,
    /// Master on left, stack on right.
    MasterStack,
    /// Equal grid.
    Grid,
    /// Maximized (one window fills workspace).
    Monocle,
    /// Three-column layout.
    ThreeColumn,
}

impl TilingLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Floating => "Floating",
            Self::HorizontalSplit => "Horizontal Split",
            Self::VerticalSplit => "Vertical Split",
            Self::MasterStack => "Master + Stack",
            Self::Grid => "Grid",
            Self::Monocle => "Monocle",
            Self::ThreeColumn => "Three Column",
        }
    }
}

/// Window geometry (position and size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A tiled window entry.
#[derive(Debug, Clone)]
pub struct TiledWindow {
    pub window_id: u32,
    pub title: String,
    pub workspace_id: u32,
    pub geometry: WindowGeometry,
    pub is_floating: bool,
    pub is_master: bool,
}

/// A workspace with tiling settings.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: u32,
    pub name: String,
    pub layout: TilingLayout,
    pub gap_px: u32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    /// Master area ratio (0-100, percent of width for master).
    pub master_ratio: u32,
    pub window_count: usize,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_WORKSPACES: usize = 20;
const MAX_WINDOWS: usize = 200;

struct State {
    workspaces: Vec<Workspace>,
    windows: Vec<TiledWindow>,
    next_workspace_id: u32,
    active_workspace: u32,
    default_gap: u32,
    default_layout: TilingLayout,
    total_tiles: u64,
    total_retiles: u64,
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

/// Calculate tile geometries for a workspace.
fn calculate_tiles(ws: &Workspace, windows: &mut [TiledWindow]) {
    let tiled: Vec<usize> = windows.iter().enumerate()
        .filter(|(_, w)| w.workspace_id == ws.id && !w.is_floating)
        .map(|(i, _)| i)
        .collect();

    let count = tiled.len();
    if count == 0 { return; }

    let gap = ws.gap_px;
    let mw = ws.monitor_width;
    let mh = ws.monitor_height;

    match ws.layout {
        TilingLayout::Floating | TilingLayout::Monocle => {
            for idx in &tiled {
                windows[*idx].geometry = WindowGeometry {
                    x: gap as i32,
                    y: gap as i32,
                    width: mw.saturating_sub(gap * 2),
                    height: mh.saturating_sub(gap * 2),
                };
            }
        }
        TilingLayout::HorizontalSplit => {
            let w = (mw.saturating_sub(gap * (count as u32 + 1))) / count as u32;
            for (i, idx) in tiled.iter().enumerate() {
                windows[*idx].geometry = WindowGeometry {
                    x: (gap + i as u32 * (w + gap)) as i32,
                    y: gap as i32,
                    width: w,
                    height: mh.saturating_sub(gap * 2),
                };
            }
        }
        TilingLayout::VerticalSplit => {
            let h = (mh.saturating_sub(gap * (count as u32 + 1))) / count as u32;
            for (i, idx) in tiled.iter().enumerate() {
                windows[*idx].geometry = WindowGeometry {
                    x: gap as i32,
                    y: (gap + i as u32 * (h + gap)) as i32,
                    width: mw.saturating_sub(gap * 2),
                    height: h,
                };
            }
        }
        TilingLayout::MasterStack => {
            if count == 1 {
                windows[tiled[0]].geometry = WindowGeometry {
                    x: gap as i32, y: gap as i32,
                    width: mw.saturating_sub(gap * 2),
                    height: mh.saturating_sub(gap * 2),
                };
                windows[tiled[0]].is_master = true;
            } else {
                let master_w = (mw * ws.master_ratio / 100).saturating_sub(gap);
                let stack_w = mw.saturating_sub(master_w + gap * 3);
                let stack_count = (count - 1) as u32;
                let stack_h = (mh.saturating_sub(gap * (stack_count + 1))) / stack_count;

                windows[tiled[0]].geometry = WindowGeometry {
                    x: gap as i32, y: gap as i32,
                    width: master_w,
                    height: mh.saturating_sub(gap * 2),
                };
                windows[tiled[0]].is_master = true;

                for (i, idx) in tiled[1..].iter().enumerate() {
                    windows[*idx].geometry = WindowGeometry {
                        x: (master_w + gap * 2) as i32,
                        y: (gap + i as u32 * (stack_h + gap)) as i32,
                        width: stack_w,
                        height: stack_h,
                    };
                    windows[*idx].is_master = false;
                }
            }
        }
        TilingLayout::Grid => {
            // Integer square root ceiling: find smallest cols where cols*cols >= count.
            let mut cols = 1u32;
            while (cols * cols) < count as u32 { cols += 1; }
            let rows = (count as u32).div_ceil(cols);
            let cw = (mw.saturating_sub(gap * (cols + 1))) / cols;
            let ch = (mh.saturating_sub(gap * (rows + 1))) / rows;
            for (i, idx) in tiled.iter().enumerate() {
                let col = i as u32 % cols;
                let row = i as u32 / cols;
                windows[*idx].geometry = WindowGeometry {
                    x: (gap + col * (cw + gap)) as i32,
                    y: (gap + row * (ch + gap)) as i32,
                    width: cw,
                    height: ch,
                };
            }
        }
        TilingLayout::ThreeColumn => {
            if count <= 3 {
                let w = (mw.saturating_sub(gap * (count as u32 + 1))) / count as u32;
                for (i, idx) in tiled.iter().enumerate() {
                    windows[*idx].geometry = WindowGeometry {
                        x: (gap + i as u32 * (w + gap)) as i32,
                        y: gap as i32,
                        width: w,
                        height: mh.saturating_sub(gap * 2),
                    };
                }
            } else {
                // Center column gets master, sides split remaining.
                let col_w = (mw.saturating_sub(gap * 4)) / 3;
                windows[tiled[0]].geometry = WindowGeometry {
                    x: (col_w + gap * 2) as i32, y: gap as i32,
                    width: col_w,
                    height: mh.saturating_sub(gap * 2),
                };
                let left: Vec<usize> = tiled[1..].iter().enumerate()
                    .filter(|(i, _)| *i % 2 == 0).map(|(_, idx)| *idx).collect();
                let right: Vec<usize> = tiled[1..].iter().enumerate()
                    .filter(|(i, _)| *i % 2 == 1).map(|(_, idx)| *idx).collect();

                let left_count = left.len().max(1) as u32;
                let lh = (mh.saturating_sub(gap * (left_count + 1))) / left_count;
                for (i, idx) in left.iter().enumerate() {
                    windows[*idx].geometry = WindowGeometry {
                        x: gap as i32,
                        y: (gap + i as u32 * (lh + gap)) as i32,
                        width: col_w,
                        height: lh,
                    };
                }

                let right_count = right.len().max(1) as u32;
                let rh = (mh.saturating_sub(gap * (right_count + 1))) / right_count;
                for (i, idx) in right.iter().enumerate() {
                    windows[*idx].geometry = WindowGeometry {
                        x: (col_w * 2 + gap * 3) as i32,
                        y: (gap + i as u32 * (rh + gap)) as i32,
                        width: col_w,
                        height: rh,
                    };
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let default_ws = Workspace {
        id: 1,
        name: String::from("Main"),
        layout: TilingLayout::MasterStack,
        gap_px: 8,
        monitor_width: 1920,
        monitor_height: 1080,
        master_ratio: 55,
        window_count: 0,
    };

    *guard = Some(State {
        workspaces: alloc::vec![default_ws],
        windows: Vec::new(),
        next_workspace_id: 2,
        active_workspace: 1,
        default_gap: 8,
        default_layout: TilingLayout::MasterStack,
        total_tiles: 0,
        total_retiles: 0,
        ops: 0,
    });
}

/// Create a workspace.
pub fn create_workspace(name: &str, layout: TilingLayout) -> KernelResult<u32> {
    with_state(|state| {
        if state.workspaces.len() >= MAX_WORKSPACES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_workspace_id;
        state.next_workspace_id += 1;
        state.workspaces.push(Workspace {
            id, name: String::from(name), layout,
            gap_px: state.default_gap,
            monitor_width: 1920, monitor_height: 1080,
            master_ratio: 55, window_count: 0,
        });
        Ok(id)
    })
}

/// Remove a workspace (moves windows to workspace 1).
pub fn remove_workspace(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if id == 1 { return Err(KernelError::PermissionDenied); } // Can't remove default.
        let pos = state.workspaces.iter().position(|w| w.id == id)
            .ok_or(KernelError::NotFound)?;
        state.workspaces.remove(pos);
        // Move windows to workspace 1.
        for w in state.windows.iter_mut() {
            if w.workspace_id == id {
                w.workspace_id = 1;
            }
        }
        if state.active_workspace == id { state.active_workspace = 1; }
        Ok(())
    })
}

/// Add a window to a workspace.
pub fn add_window(window_id: u32, title: &str, workspace_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.windows.len() >= MAX_WINDOWS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.windows.iter().any(|w| w.window_id == window_id) {
            return Err(KernelError::AlreadyExists);
        }
        if !state.workspaces.iter().any(|ws| ws.id == workspace_id) {
            return Err(KernelError::NotFound);
        }
        state.windows.push(TiledWindow {
            window_id, title: String::from(title), workspace_id,
            geometry: WindowGeometry { x: 0, y: 0, width: 640, height: 480 },
            is_floating: false, is_master: false,
        });
        // Update count.
        if let Some(ws) = state.workspaces.iter_mut().find(|ws| ws.id == workspace_id) {
            ws.window_count = state.windows.iter().filter(|w| w.workspace_id == workspace_id).count();
        }
        Ok(())
    })
}

/// Remove a window.
pub fn remove_window(window_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.windows.iter().position(|w| w.window_id == window_id)
            .ok_or(KernelError::NotFound)?;
        let ws_id = state.windows[pos].workspace_id;
        state.windows.remove(pos);
        if let Some(ws) = state.workspaces.iter_mut().find(|ws| ws.id == ws_id) {
            ws.window_count = state.windows.iter().filter(|w| w.workspace_id == ws_id).count();
        }
        Ok(())
    })
}

/// Retile a workspace (recalculate all window positions).
pub fn retile(workspace_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ws = state.workspaces.iter().find(|ws| ws.id == workspace_id)
            .ok_or(KernelError::NotFound)?.clone();
        calculate_tiles(&ws, &mut state.windows);
        state.total_retiles += 1;
        Ok(())
    })
}

/// Set layout for a workspace.
pub fn set_layout(workspace_id: u32, layout: TilingLayout) -> KernelResult<()> {
    with_state(|state| {
        let ws = state.workspaces.iter_mut().find(|ws| ws.id == workspace_id)
            .ok_or(KernelError::NotFound)?;
        ws.layout = layout;
        let ws_clone = ws.clone();
        calculate_tiles(&ws_clone, &mut state.windows);
        state.total_tiles += 1;
        Ok(())
    })
}

/// Set gap size.
pub fn set_gap(workspace_id: u32, gap_px: u32) -> KernelResult<()> {
    with_state(|state| {
        let ws = state.workspaces.iter_mut().find(|ws| ws.id == workspace_id)
            .ok_or(KernelError::NotFound)?;
        ws.gap_px = gap_px;
        Ok(())
    })
}

/// Set master ratio (0-100).
pub fn set_master_ratio(workspace_id: u32, ratio: u32) -> KernelResult<()> {
    with_state(|state| {
        let ws = state.workspaces.iter_mut().find(|ws| ws.id == workspace_id)
            .ok_or(KernelError::NotFound)?;
        ws.master_ratio = ratio.clamp(10, 90);
        Ok(())
    })
}

/// Toggle a window's floating state.
pub fn toggle_floating(window_id: u32) -> KernelResult<bool> {
    with_state(|state| {
        let win = state.windows.iter_mut().find(|w| w.window_id == window_id)
            .ok_or(KernelError::NotFound)?;
        win.is_floating = !win.is_floating;
        Ok(win.is_floating)
    })
}

/// Move a window to another workspace.
pub fn move_to_workspace(window_id: u32, workspace_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.workspaces.iter().any(|ws| ws.id == workspace_id) {
            return Err(KernelError::NotFound);
        }
        let win = state.windows.iter_mut().find(|w| w.window_id == window_id)
            .ok_or(KernelError::NotFound)?;
        let old_ws = win.workspace_id;
        win.workspace_id = workspace_id;
        // Update counts.
        for ws in state.workspaces.iter_mut() {
            if ws.id == old_ws || ws.id == workspace_id {
                ws.window_count = state.windows.iter().filter(|w| w.workspace_id == ws.id).count();
            }
        }
        Ok(())
    })
}

/// Set active workspace.
pub fn set_active_workspace(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.workspaces.iter().any(|ws| ws.id == id) {
            return Err(KernelError::NotFound);
        }
        state.active_workspace = id;
        Ok(())
    })
}

/// List workspaces.
pub fn list_workspaces() -> Vec<Workspace> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.workspaces.clone())
}

/// List windows in a workspace.
pub fn list_windows(workspace_id: u32) -> Vec<TiledWindow> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        if workspace_id == 0 {
            s.windows.clone()
        } else {
            s.windows.iter().filter(|w| w.workspace_id == workspace_id).cloned().collect()
        }
    })
}

/// Statistics: (workspace_count, window_count, total_tiles, total_retiles, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.workspaces.len(), s.windows.len(), s.total_tiles, s.total_retiles, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("wintiling::self_test() — running tests...");
    init_defaults();

    // 1: Default workspace.
    let wss = list_workspaces();
    assert_eq!(wss.len(), 1);
    assert_eq!(wss[0].layout, TilingLayout::MasterStack);
    crate::serial_println!("  [1/10] default workspace: OK");

    // 2: Create workspace.
    let ws2 = create_workspace("Secondary", TilingLayout::Grid).expect("create");
    assert_eq!(list_workspaces().len(), 2);
    crate::serial_println!("  [2/10] create workspace: OK");

    // 3: Add windows.
    add_window(100, "Terminal", 1).expect("add1");
    add_window(101, "Browser", 1).expect("add2");
    add_window(102, "Editor", 1).expect("add3");
    assert_eq!(list_windows(1).len(), 3);
    crate::serial_println!("  [3/10] add windows: OK");

    // 4: Retile (master+stack).
    retile(1).expect("retile");
    let wins = list_windows(1);
    assert!(wins[0].is_master);
    assert!(wins[0].geometry.width > 0);
    crate::serial_println!("  [4/10] retile master+stack: OK");

    // 5: Change layout.
    set_layout(1, TilingLayout::Grid).expect("layout");
    crate::serial_println!("  [5/10] change layout: OK");

    // 6: Toggle floating.
    let floating = toggle_floating(101).expect("float");
    assert!(floating);
    crate::serial_println!("  [6/10] toggle floating: OK");

    // 7: Move to workspace.
    move_to_workspace(102, ws2).expect("move");
    assert_eq!(list_windows(ws2).len(), 1);
    crate::serial_println!("  [7/10] move to workspace: OK");

    // 8: Set gap and master ratio.
    set_gap(1, 12).expect("gap");
    set_master_ratio(1, 60).expect("ratio");
    let wss = list_workspaces();
    let ws1 = wss.iter().find(|w| w.id == 1).expect("find");
    assert_eq!(ws1.gap_px, 12);
    assert_eq!(ws1.master_ratio, 60);
    crate::serial_println!("  [8/10] gap and ratio: OK");

    // 9: Remove window.
    remove_window(100).expect("remove");
    assert_eq!(list_windows(1).len(), 1); // 101 still there (floating)
    crate::serial_println!("  [9/10] remove window: OK");

    // 10: Stats.
    let (ws_count, win_count, tiles, retiles, ops) = stats();
    assert_eq!(ws_count, 2);
    assert!(win_count >= 1);
    assert!(tiles >= 1);
    assert!(retiles >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("wintiling::self_test() — all 10 tests passed");
}
