//! Snap Layouts — window snap zone templates and management.
//!
//! Provides predefined and custom snap zone layouts for window tiling,
//! including layout selection UI, zone definitions, and snap group tracking.
//!
//! ## Architecture
//!
//! ```text
//! User triggers snap
//!   → snaplayout::snap_to_zone(window, zone) → position window
//!   → snaplayout::suggest_layout(count) → recommend layout
//!
//! Configuration
//!   → snaplayout::add_layout(name, zones)
//!   → snaplayout::set_active(id)
//!
//! Integration:
//!   → winsnap (snap detection and anchoring)
//!   → wintiling (tiling engine)
//!   → displayarrange (multi-monitor zone spanning)
//!   → compositor (zone overlay rendering)
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

/// A zone within a snap layout (position as percentage of screen).
#[derive(Debug, Clone)]
pub struct SnapZone {
    /// Zone identifier within the layout.
    pub id: u32,
    /// Left edge as percentage (0-100).
    pub x_pct: u32,
    /// Top edge as percentage (0-100).
    pub y_pct: u32,
    /// Width as percentage (0-100).
    pub w_pct: u32,
    /// Height as percentage (0-100).
    pub h_pct: u32,
}

/// A complete snap layout with named zones.
#[derive(Debug, Clone)]
pub struct SnapLayout {
    pub id: u32,
    pub name: String,
    pub zones: Vec<SnapZone>,
    pub use_count: u64,
}

/// A snap group (windows snapped together in a layout).
#[derive(Debug, Clone)]
pub struct SnapGroup {
    pub group_id: u32,
    pub layout_id: u32,
    /// (window_id, zone_id) pairs.
    pub assignments: Vec<(u32, u32)>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_LAYOUTS: usize = 30;
const MAX_GROUPS: usize = 50;

struct State {
    layouts: Vec<SnapLayout>,
    groups: Vec<SnapGroup>,
    active_layout_id: u32,
    next_layout_id: u32,
    next_group_id: u32,
    total_snaps: u64,
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

fn default_layouts(start_id: u32) -> (Vec<SnapLayout>, u32) {
    let mut id = start_id;
    let mut layouts = Vec::new();

    // 1: Half-half (left/right).
    layouts.push(SnapLayout {
        id: { let i = id; id += 1; i },
        name: String::from("Half-Half"),
        zones: alloc::vec![
            SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 50, h_pct: 100 },
            SnapZone { id: 1, x_pct: 50, y_pct: 0, w_pct: 50, h_pct: 100 },
        ],
        use_count: 0,
    });

    // 2: Three columns (33/33/34).
    layouts.push(SnapLayout {
        id: { let i = id; id += 1; i },
        name: String::from("Three Columns"),
        zones: alloc::vec![
            SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 33, h_pct: 100 },
            SnapZone { id: 1, x_pct: 33, y_pct: 0, w_pct: 34, h_pct: 100 },
            SnapZone { id: 2, x_pct: 67, y_pct: 0, w_pct: 33, h_pct: 100 },
        ],
        use_count: 0,
    });

    // 3: Quadrants.
    layouts.push(SnapLayout {
        id: { let i = id; id += 1; i },
        name: String::from("Quadrants"),
        zones: alloc::vec![
            SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 50, h_pct: 50 },
            SnapZone { id: 1, x_pct: 50, y_pct: 0, w_pct: 50, h_pct: 50 },
            SnapZone { id: 2, x_pct: 0, y_pct: 50, w_pct: 50, h_pct: 50 },
            SnapZone { id: 3, x_pct: 50, y_pct: 50, w_pct: 50, h_pct: 50 },
        ],
        use_count: 0,
    });

    // 4: Main + sidebar (70/30).
    layouts.push(SnapLayout {
        id: { let i = id; id += 1; i },
        name: String::from("Main + Sidebar"),
        zones: alloc::vec![
            SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 70, h_pct: 100 },
            SnapZone { id: 1, x_pct: 70, y_pct: 0, w_pct: 30, h_pct: 100 },
        ],
        use_count: 0,
    });

    // 5: Main + 2 side (60/40 split).
    layouts.push(SnapLayout {
        id: { let i = id; id += 1; i },
        name: String::from("Main + 2 Side"),
        zones: alloc::vec![
            SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 60, h_pct: 100 },
            SnapZone { id: 1, x_pct: 60, y_pct: 0, w_pct: 40, h_pct: 50 },
            SnapZone { id: 2, x_pct: 60, y_pct: 50, w_pct: 40, h_pct: 50 },
        ],
        use_count: 0,
    });

    (layouts, id)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let (layouts, next_id) = default_layouts(1);
    *guard = Some(State {
        active_layout_id: 1,
        layouts,
        groups: Vec::new(),
        next_layout_id: next_id,
        next_group_id: 1,
        total_snaps: 0,
        ops: 0,
    });
}

/// Add a custom layout.
pub fn add_layout(name: &str, zones: Vec<SnapZone>) -> KernelResult<u32> {
    with_state(|state| {
        if state.layouts.len() >= MAX_LAYOUTS {
            return Err(KernelError::ResourceExhausted);
        }
        if zones.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let id = state.next_layout_id;
        state.next_layout_id += 1;
        state.layouts.push(SnapLayout {
            id, name: String::from(name), zones, use_count: 0,
        });
        Ok(id)
    })
}

/// Remove a custom layout.
pub fn remove_layout(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.layouts.len();
        state.layouts.retain(|l| l.id != id);
        if state.layouts.len() == before {
            return Err(KernelError::NotFound);
        }
        if state.active_layout_id == id {
            state.active_layout_id = state.layouts.first().map_or(0, |l| l.id);
        }
        Ok(())
    })
}

/// Set active layout.
pub fn set_active(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.layouts.iter().any(|l| l.id == id) {
            return Err(KernelError::NotFound);
        }
        state.active_layout_id = id;
        Ok(())
    })
}

/// Get the active layout.
pub fn get_active() -> Option<SnapLayout> {
    STATE.lock().as_ref().and_then(|s| {
        s.layouts.iter().find(|l| l.id == s.active_layout_id).cloned()
    })
}

/// Snap a window to a zone in the active layout.
pub fn snap_to_zone(window_id: u32, zone_id: u32) -> KernelResult<SnapZone> {
    with_state(|state| {
        let layout = state.layouts.iter_mut()
            .find(|l| l.id == state.active_layout_id)
            .ok_or(KernelError::NotFound)?;
        let zone = layout.zones.iter()
            .find(|z| z.id == zone_id)
            .ok_or(KernelError::NotFound)?
            .clone();
        layout.use_count += 1;
        state.total_snaps += 1;

        // Add to or update snap group.
        if let Some(group) = state.groups.iter_mut().find(|g| g.layout_id == state.active_layout_id) {
            group.assignments.retain(|&(wid, _)| wid != window_id);
            group.assignments.push((window_id, zone_id));
        } else {
            if state.groups.len() < MAX_GROUPS {
                let gid = state.next_group_id;
                state.next_group_id += 1;
                state.groups.push(SnapGroup {
                    group_id: gid,
                    layout_id: state.active_layout_id,
                    assignments: alloc::vec![(window_id, zone_id)],
                });
            }
        }
        Ok(zone)
    })
}

/// Suggest a layout based on window count.
pub fn suggest_layout(window_count: usize) -> Option<SnapLayout> {
    STATE.lock().as_ref().and_then(|s| {
        // Find layout with zone count closest to window count.
        s.layouts.iter()
            .min_by_key(|l| {
                
                if l.zones.len() >= window_count {
                    l.zones.len() - window_count
                } else {
                    window_count - l.zones.len()
                }
            })
            .cloned()
    })
}

/// List all layouts.
pub fn list_layouts() -> Vec<SnapLayout> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.layouts.clone())
}

/// List snap groups.
pub fn list_groups() -> Vec<SnapGroup> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.groups.clone())
}

/// Statistics: (layout_count, group_count, total_snaps, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.layouts.len(), s.groups.len(), s.total_snaps, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("snaplayout::self_test() — running tests...");
    init_defaults();

    // 1: Default layouts.
    let layouts = list_layouts();
    assert_eq!(layouts.len(), 5);
    assert_eq!(layouts[0].name, "Half-Half");
    crate::serial_println!("  [1/8] default layouts: OK");

    // 2: Active layout.
    let active = get_active().expect("active");
    assert_eq!(active.name, "Half-Half");
    assert_eq!(active.zones.len(), 2);
    crate::serial_println!("  [2/8] active layout: OK");

    // 3: Snap to zone.
    let zone = snap_to_zone(100, 0).expect("snap");
    assert_eq!(zone.x_pct, 0);
    assert_eq!(zone.w_pct, 50);
    crate::serial_println!("  [3/8] snap to zone: OK");

    // 4: Switch layout.
    set_active(3).expect("switch"); // Quadrants
    let active = get_active().expect("active2");
    assert_eq!(active.name, "Quadrants");
    assert_eq!(active.zones.len(), 4);
    crate::serial_println!("  [4/8] switch layout: OK");

    // 5: Add custom layout.
    let custom_zones = alloc::vec![
        SnapZone { id: 0, x_pct: 0, y_pct: 0, w_pct: 25, h_pct: 100 },
        SnapZone { id: 1, x_pct: 25, y_pct: 0, w_pct: 50, h_pct: 100 },
        SnapZone { id: 2, x_pct: 75, y_pct: 0, w_pct: 25, h_pct: 100 },
    ];
    let cid = add_layout("Custom 3-col", custom_zones).expect("add");
    assert!(cid >= 6);
    assert_eq!(list_layouts().len(), 6);
    crate::serial_println!("  [5/8] custom layout: OK");

    // 6: Suggest layout.
    let suggested = suggest_layout(4).expect("suggest");
    assert_eq!(suggested.name, "Quadrants"); // 4 zones = best match for 4 windows
    crate::serial_println!("  [6/8] suggest: OK");

    // 7: Remove layout.
    remove_layout(cid).expect("remove");
    assert_eq!(list_layouts().len(), 5);
    crate::serial_println!("  [7/8] remove layout: OK");

    // 8: Stats.
    let (layouts, groups, snaps, ops) = stats();
    assert_eq!(layouts, 5);
    assert!(groups >= 1);
    assert!(snaps >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("snaplayout::self_test() — all 8 tests passed");
}
