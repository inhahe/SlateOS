//! Split View — multi-pane window management.
//!
//! Manages split-view layouts for dividing windows into
//! multiple resizable panes with flexible orientations.
//!
//! ## Architecture
//!
//! ```text
//! Split management
//!   → splitview::create_split(orientation) → new split container
//!   → splitview::add_pane(split, window_id) → add window to pane
//!   → splitview::resize(split, pane, ratio) → adjust sizes
//!
//! Integration:
//!   → wintiling (tiling window manager)
//!   → snaplayout (snap layouts)
//!   → winsnap (window snapping)
//!   → display (screen dimensions)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Split orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal",
            Self::Vertical => "Vertical",
        }
    }
}

/// A pane within a split.
#[derive(Debug, Clone)]
pub struct Pane {
    pub id: u32,
    pub window_id: Option<u32>,
    pub ratio: u32,     // 1-100, relative size within split.
    pub focused: bool,
}

/// A split container.
#[derive(Debug, Clone)]
pub struct SplitContainer {
    pub id: u32,
    pub orientation: Orientation,
    pub panes: Vec<Pane>,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SPLITS: usize = 50;
const MAX_PANES_PER_SPLIT: usize = 8;

struct State {
    splits: Vec<SplitContainer>,
    next_split_id: u32,
    next_pane_id: u32,
    active_split: Option<u32>,
    total_splits_created: u64,
    total_panes_added: u64,
    total_resizes: u64,
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
        splits: Vec::new(),
        next_split_id: 1,
        next_pane_id: 1,
        active_split: None,
        total_splits_created: 0,
        total_panes_added: 0,
        total_resizes: 0,
        ops: 0,
    });
}

/// Create a new split container.
pub fn create_split(orientation: Orientation) -> KernelResult<u32> {
    with_state(|state| {
        if state.splits.len() >= MAX_SPLITS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_split_id;
        state.next_split_id += 1;
        state.total_splits_created += 1;
        state.splits.push(SplitContainer {
            id, orientation, panes: Vec::new(), created_ns: now,
        });
        if state.active_split.is_none() {
            state.active_split = Some(id);
        }
        Ok(id)
    })
}

/// Remove a split container.
pub fn remove_split(split_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.splits.len();
        state.splits.retain(|s| s.id != split_id);
        if state.splits.len() == before { return Err(KernelError::NotFound); }
        if state.active_split == Some(split_id) {
            state.active_split = state.splits.first().map(|s| s.id);
        }
        Ok(())
    })
}

/// Add a pane to a split container.
pub fn add_pane(split_id: u32, window_id: Option<u32>) -> KernelResult<u32> {
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        if split.panes.len() >= MAX_PANES_PER_SPLIT {
            return Err(KernelError::ResourceExhausted);
        }
        let pane_id = state.next_pane_id;
        state.next_pane_id += 1;
        state.total_panes_added += 1;
        // Distribute ratio equally among all panes.
        let new_count = split.panes.len() as u32 + 1;
        let each = 100 / new_count;
        for p in &mut split.panes {
            p.ratio = each;
            p.focused = false;
        }
        split.panes.push(Pane {
            id: pane_id, window_id, ratio: each, focused: true,
        });
        Ok(pane_id)
    })
}

/// Remove a pane from a split.
pub fn remove_pane(split_id: u32, pane_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        let before = split.panes.len();
        split.panes.retain(|p| p.id != pane_id);
        if split.panes.len() == before { return Err(KernelError::NotFound); }
        // Redistribute ratios.
        if !split.panes.is_empty() {
            let each = 100 / split.panes.len() as u32;
            for p in &mut split.panes {
                p.ratio = each;
            }
        }
        Ok(())
    })
}

/// Resize a pane (set ratio 1-100).
pub fn resize_pane(split_id: u32, pane_id: u32, ratio: u32) -> KernelResult<()> {
    if ratio == 0 || ratio > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        let pane = split.panes.iter_mut().find(|p| p.id == pane_id)
            .ok_or(KernelError::NotFound)?;
        pane.ratio = ratio;
        state.total_resizes += 1;
        Ok(())
    })
}

/// Focus a pane.
pub fn focus_pane(split_id: u32, pane_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        for p in &mut split.panes {
            p.focused = p.id == pane_id;
        }
        state.active_split = Some(split_id);
        Ok(())
    })
}

/// Assign a window to a pane.
pub fn set_window(split_id: u32, pane_id: u32, window_id: Option<u32>) -> KernelResult<()> {
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        let pane = split.panes.iter_mut().find(|p| p.id == pane_id)
            .ok_or(KernelError::NotFound)?;
        pane.window_id = window_id;
        Ok(())
    })
}

/// Change split orientation.
pub fn set_orientation(split_id: u32, orientation: Orientation) -> KernelResult<()> {
    with_state(|state| {
        let split = state.splits.iter_mut().find(|s| s.id == split_id)
            .ok_or(KernelError::NotFound)?;
        split.orientation = orientation;
        Ok(())
    })
}

/// List all splits.
pub fn list_splits() -> Vec<SplitContainer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.splits.clone())
}

/// Get active split.
pub fn active_split() -> Option<u32> {
    STATE.lock().as_ref().and_then(|s| s.active_split)
}

/// Statistics: (split_count, total_created, total_panes, total_resizes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.splits.len(), s.total_splits_created, s.total_panes_added, s.total_resizes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("splitview::self_test() — running tests...");
    init_defaults();

    // 1: Empty initially.
    assert!(list_splits().is_empty());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Create split.
    let s1 = create_split(Orientation::Horizontal).expect("create");
    assert_eq!(list_splits().len(), 1);
    assert_eq!(active_split(), Some(s1));
    crate::serial_println!("  [2/8] create: OK");

    // 3: Add panes.
    let p1 = add_pane(s1, Some(100)).expect("pane1");
    let p2 = add_pane(s1, Some(200)).expect("pane2");
    let splits = list_splits();
    assert_eq!(splits[0].panes.len(), 2);
    crate::serial_println!("  [3/8] add panes: OK");

    // 4: Equal ratio distribution.
    let splits = list_splits();
    assert_eq!(splits[0].panes[0].ratio, 50);
    assert_eq!(splits[0].panes[1].ratio, 50);
    crate::serial_println!("  [4/8] ratios: OK");

    // 5: Resize pane.
    resize_pane(s1, p1, 70).expect("resize");
    let splits = list_splits();
    assert_eq!(splits[0].panes[0].ratio, 70);
    crate::serial_println!("  [5/8] resize: OK");

    // 6: Focus pane.
    focus_pane(s1, p1).expect("focus");
    let splits = list_splits();
    assert!(splits[0].panes[0].focused);
    assert!(!splits[0].panes[1].focused);
    crate::serial_println!("  [6/8] focus: OK");

    // 7: Remove pane.
    remove_pane(s1, p2).expect("remove_pane");
    let splits = list_splits();
    assert_eq!(splits[0].panes.len(), 1);
    assert_eq!(splits[0].panes[0].ratio, 100); // Redistributed.
    crate::serial_println!("  [7/8] remove pane: OK");

    // 8: Stats.
    let (splits_count, created, panes, resizes, ops) = stats();
    assert_eq!(splits_count, 1);
    assert_eq!(created, 1);
    assert_eq!(panes, 2);
    assert_eq!(resizes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("splitview::self_test() — all 8 tests passed");
}
