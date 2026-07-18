//! Display Arrangement — multi-monitor layout configuration.
//!
//! Manages physical positioning of multiple displays, primary display
//! selection, and display topology (extend/mirror/single).
//!
//! ## Architecture
//!
//! ```text
//! Displays connected
//!   → displayarrange::detect() → auto-arrange
//!   → displayarrange::set_position(id, x, y)
//!   → displayarrange::set_topology(mode)
//!
//! Integration:
//!   → monitors (multi-display info)
//!   → display (display management)
//!   → dpiscaling (per-display scaling)
//!   → hdrdisplay (HDR per display)
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

/// Display topology mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    /// Extend desktop across all displays.
    Extend,
    /// Mirror primary on all displays.
    Mirror,
    /// Only use one display.
    SingleDisplay,
}

impl Topology {
    pub fn label(self) -> &'static str {
        match self {
            Self::Extend => "Extend",
            Self::Mirror => "Mirror",
            Self::SingleDisplay => "Single Display",
        }
    }
}

/// Display orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

impl Orientation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Landscape => "Landscape",
            Self::Portrait => "Portrait",
            Self::LandscapeFlipped => "Landscape (Flipped)",
            Self::PortraitFlipped => "Portrait (Flipped)",
        }
    }
}

/// A display in the arrangement.
#[derive(Debug, Clone)]
pub struct ArrangedDisplay {
    pub id: u32,
    pub name: String,
    /// Position X in logical pixels.
    pub x: i32,
    /// Position Y in logical pixels.
    pub y: i32,
    /// Width in logical pixels.
    pub width: u32,
    /// Height in logical pixels.
    pub height: u32,
    pub orientation: Orientation,
    pub is_primary: bool,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DISPLAYS: usize = 16;

struct State {
    displays: Vec<ArrangedDisplay>,
    next_id: u32,
    topology: Topology,
    total_rearrangements: u64,
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

    let primary = ArrangedDisplay {
        id: 1, name: String::from("Primary"),
        x: 0, y: 0, width: 1920, height: 1080,
        orientation: Orientation::Landscape,
        is_primary: true, enabled: true,
    };

    *guard = Some(State {
        displays: alloc::vec![primary],
        next_id: 2,
        topology: Topology::Extend,
        total_rearrangements: 0,
        ops: 0,
    });
}

/// Add a display to the arrangement.
pub fn add_display(name: &str, width: u32, height: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.displays.len() >= MAX_DISPLAYS {
            return Err(KernelError::ResourceExhausted);
        }
        // Auto-place to the right of the rightmost display.
        let right_edge = state.displays.iter()
            .filter(|d| d.enabled)
            .map(|d| d.x + d.width as i32)
            .max()
            .unwrap_or(0);
        let id = state.next_id;
        state.next_id += 1;
        state.displays.push(ArrangedDisplay {
            id, name: String::from(name),
            x: right_edge, y: 0, width, height,
            orientation: Orientation::Landscape,
            is_primary: false, enabled: true,
        });
        Ok(id)
    })
}

/// Set display position.
pub fn set_position(display_id: u32, x: i32, y: i32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.x = x;
        d.y = y;
        state.total_rearrangements += 1;
        Ok(())
    })
}

/// Set display orientation.
pub fn set_orientation(display_id: u32, orientation: Orientation) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        if orientation == Orientation::Portrait || orientation == Orientation::PortraitFlipped {
            let (w, h) = (d.width, d.height);
            d.width = h;
            d.height = w;
        }
        d.orientation = orientation;
        state.total_rearrangements += 1;
        Ok(())
    })
}

/// Set primary display.
pub fn set_primary(display_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.displays.iter().any(|d| d.id == display_id) {
            return Err(KernelError::NotFound);
        }
        for d in &mut state.displays {
            d.is_primary = d.id == display_id;
        }
        Ok(())
    })
}

/// Enable/disable a display.
pub fn set_enabled(display_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.enabled = enabled;
        Ok(())
    })
}

/// Set topology mode.
pub fn set_topology(topology: Topology) -> KernelResult<()> {
    with_state(|state| {
        state.topology = topology;
        state.total_rearrangements += 1;
        Ok(())
    })
}

/// Remove a display.
pub fn remove_display(display_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.displays.iter().position(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        state.displays.remove(pos);
        Ok(())
    })
}

/// List displays.
pub fn list_displays() -> Vec<ArrangedDisplay> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.displays.clone())
}

/// Get display.
pub fn get_display(id: u32) -> KernelResult<ArrangedDisplay> {
    with_state(|state| {
        state.displays.iter().find(|d| d.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Current topology.
pub fn topology() -> Topology {
    STATE.lock().as_ref().map_or(Topology::Extend, |s| s.topology)
}

/// Statistics: (display_count, topology, total_rearrangements, ops).
pub fn stats() -> (usize, &'static str, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.displays.len(), s.topology.label(), s.total_rearrangements, s.ops),
        None => (0, "Unknown", 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("displayarrange::self_test() — running tests...");
    init_defaults();

    // 1: Default display.
    let displays = list_displays();
    assert_eq!(displays.len(), 1);
    assert!(displays[0].is_primary);
    assert_eq!(displays[0].x, 0);
    crate::serial_println!("  [1/8] default display: OK");

    // 2: Add second display (auto-placed right).
    let id2 = add_display("Secondary", 2560, 1440).expect("add");
    let d2 = get_display(id2).expect("get");
    assert_eq!(d2.x, 1920); // right edge of primary
    crate::serial_println!("  [2/8] add display: OK");

    // 3: Set position.
    set_position(id2, 0, -1440).expect("pos");
    let d2 = get_display(id2).expect("get2");
    assert_eq!(d2.y, -1440);
    crate::serial_println!("  [3/8] set position: OK");

    // 4: Set primary.
    set_primary(id2).expect("primary");
    let d1 = get_display(1).expect("get3");
    let d2 = get_display(id2).expect("get4");
    assert!(!d1.is_primary);
    assert!(d2.is_primary);
    crate::serial_println!("  [4/8] set primary: OK");

    // 5: Orientation.
    set_orientation(1, Orientation::Portrait).expect("orient");
    let d1 = get_display(1).expect("get5");
    assert_eq!(d1.orientation, Orientation::Portrait);
    assert_eq!(d1.width, 1080);
    assert_eq!(d1.height, 1920);
    crate::serial_println!("  [5/8] orientation: OK");

    // 6: Topology.
    set_topology(Topology::Mirror).expect("topo");
    assert_eq!(topology(), Topology::Mirror);
    crate::serial_println!("  [6/8] topology: OK");

    // 7: Disable display.
    set_enabled(id2, false).expect("disable");
    let d2 = get_display(id2).expect("get6");
    assert!(!d2.enabled);
    crate::serial_println!("  [7/8] disable: OK");

    // 8: Stats.
    let (count, _topo, rearrangements, ops) = stats();
    assert_eq!(count, 2);
    assert!(rearrangements >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("displayarrange::self_test() — all 8 tests passed");
}
