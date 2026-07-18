//! Pen Input — stylus and pen tablet input support.
//!
//! Manages pen/stylus devices with pressure sensitivity, tilt,
//! button mapping, and gesture recognition for drawing applications.
//!
//! ## Architecture
//!
//! ```text
//! Pen device connected
//!   → peninput::register_pen(name, capabilities)
//!
//! Pen input event
//!   → peninput::report_input(pen_id, x, y, pressure, tilt)
//!     → dispatches to focused app
//!
//! Integration:
//!   → gamepadinput (generic input device framework)
//!   → gestures (pen gesture recognition)
//!   → a11y (handwriting recognition)
//!   → display (pen cursor rendering)
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

/// Pen device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenType {
    Stylus,
    AirBrush,
    ArtPen,
    Eraser,
    Touch,
}

impl PenType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stylus => "Stylus",
            Self::AirBrush => "Airbrush",
            Self::ArtPen => "Art Pen",
            Self::Eraser => "Eraser",
            Self::Touch => "Touch",
        }
    }
}

/// Pen capabilities bitflags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenCapabilities {
    pub pressure: bool,
    pub tilt: bool,
    pub rotation: bool,
    pub eraser_tip: bool,
    pub buttons: u8,
}

/// Pen device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenState {
    OutOfRange,
    Hovering,
    Contact,
}

impl PenState {
    pub fn label(self) -> &'static str {
        match self {
            Self::OutOfRange => "Out of Range",
            Self::Hovering => "Hovering",
            Self::Contact => "Contact",
        }
    }
}

/// A registered pen device.
#[derive(Debug, Clone)]
pub struct PenDevice {
    pub id: u32,
    pub name: String,
    pub pen_type: PenType,
    pub capabilities: PenCapabilities,
    pub state: PenState,
    /// Current X position (0-32767 normalized).
    pub x: u16,
    /// Current Y position (0-32767 normalized).
    pub y: u16,
    /// Pressure level (0-4096).
    pub pressure: u16,
    /// Tilt X in tenths of degrees (-900 to 900).
    pub tilt_x: i16,
    /// Tilt Y in tenths of degrees (-900 to 900).
    pub tilt_y: i16,
    /// Rotation in tenths of degrees (0-3599).
    pub rotation: u16,
    /// Button state bitmask.
    pub buttons: u8,
    pub registered_ns: u64,
}

/// Pen input event (logged).
#[derive(Debug, Clone)]
pub struct PenEvent {
    pub id: u32,
    pub pen_id: u32,
    pub event_type: PenEventType,
    pub x: u16,
    pub y: u16,
    pub pressure: u16,
    pub timestamp_ns: u64,
}

/// Pen event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenEventType {
    ProximityIn,
    ProximityOut,
    Contact,
    Release,
    Move,
    ButtonPress,
    ButtonRelease,
}

impl PenEventType {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProximityIn => "Proximity In",
            Self::ProximityOut => "Proximity Out",
            Self::Contact => "Contact",
            Self::Release => "Release",
            Self::Move => "Move",
            Self::ButtonPress => "Button Press",
            Self::ButtonRelease => "Button Release",
        }
    }
}

/// Pen button mapping.
#[derive(Debug, Clone)]
pub struct ButtonMapping {
    pub pen_id: u32,
    pub button_index: u8,
    pub action: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PENS: usize = 10;
const MAX_EVENTS: usize = 500;
const MAX_MAPPINGS: usize = 50;

struct State {
    pens: Vec<PenDevice>,
    events: Vec<PenEvent>,
    mappings: Vec<ButtonMapping>,
    next_pen_id: u32,
    next_event_id: u32,
    total_events: u64,
    total_strokes: u64,
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

fn log_event(state: &mut State, pen_id: u32, event_type: PenEventType, x: u16, y: u16, pressure: u16) {
    if state.events.len() >= MAX_EVENTS {
        state.events.remove(0);
    }
    let id = state.next_event_id;
    state.next_event_id += 1;
    state.total_events += 1;
    state.events.push(PenEvent {
        id, pen_id, event_type, x, y, pressure,
        timestamp_ns: crate::hpet::elapsed_ns(),
    });
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pens: Vec::new(),
        events: Vec::new(),
        mappings: Vec::new(),
        next_pen_id: 1,
        next_event_id: 1,
        total_events: 0,
        total_strokes: 0,
        ops: 0,
    });
}

/// Register a pen device.
pub fn register_pen(name: &str, pen_type: PenType, capabilities: PenCapabilities) -> KernelResult<u32> {
    with_state(|state| {
        if state.pens.len() >= MAX_PENS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_pen_id;
        state.next_pen_id += 1;
        state.pens.push(PenDevice {
            id, name: String::from(name), pen_type, capabilities,
            state: PenState::OutOfRange,
            x: 0, y: 0, pressure: 0,
            tilt_x: 0, tilt_y: 0, rotation: 0,
            buttons: 0,
            registered_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Unregister a pen device.
pub fn unregister_pen(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.pens.iter().position(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        state.pens.remove(pos);
        state.mappings.retain(|m| m.pen_id != id);
        Ok(())
    })
}

/// Report pen proximity in.
pub fn proximity_in(pen_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pen = state.pens.iter_mut().find(|p| p.id == pen_id)
            .ok_or(KernelError::NotFound)?;
        pen.state = PenState::Hovering;
        log_event(state, pen_id, PenEventType::ProximityIn, 0, 0, 0);
        Ok(())
    })
}

/// Report pen proximity out.
pub fn proximity_out(pen_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pen = state.pens.iter_mut().find(|p| p.id == pen_id)
            .ok_or(KernelError::NotFound)?;
        pen.state = PenState::OutOfRange;
        pen.pressure = 0;
        let (px, py) = (pen.x, pen.y);
        log_event(state, pen_id, PenEventType::ProximityOut, px, py, 0);
        Ok(())
    })
}

/// Report pen contact (tip down).
pub fn contact(pen_id: u32, x: u16, y: u16, pressure: u16) -> KernelResult<()> {
    with_state(|state| {
        let pen = state.pens.iter_mut().find(|p| p.id == pen_id)
            .ok_or(KernelError::NotFound)?;
        pen.state = PenState::Contact;
        pen.x = x;
        pen.y = y;
        pen.pressure = pressure.min(4096);
        state.total_strokes += 1;
        log_event(state, pen_id, PenEventType::Contact, x, y, pressure);
        Ok(())
    })
}

/// Report pen release (tip up).
pub fn release(pen_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pen = state.pens.iter_mut().find(|p| p.id == pen_id)
            .ok_or(KernelError::NotFound)?;
        pen.state = PenState::Hovering;
        pen.pressure = 0;
        let (px, py) = (pen.x, pen.y);
        log_event(state, pen_id, PenEventType::Release, px, py, 0);
        Ok(())
    })
}

/// Report pen movement.
pub fn report_move(pen_id: u32, x: u16, y: u16, pressure: u16, tilt_x: i16, tilt_y: i16) -> KernelResult<()> {
    with_state(|state| {
        let pen = state.pens.iter_mut().find(|p| p.id == pen_id)
            .ok_or(KernelError::NotFound)?;
        pen.x = x;
        pen.y = y;
        pen.pressure = pressure.min(4096);
        pen.tilt_x = tilt_x.clamp(-900, 900);
        pen.tilt_y = tilt_y.clamp(-900, 900);
        log_event(state, pen_id, PenEventType::Move, x, y, pressure);
        Ok(())
    })
}

/// Set button mapping for a pen.
pub fn set_button_mapping(pen_id: u32, button_index: u8, action: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.pens.iter().any(|p| p.id == pen_id) {
            return Err(KernelError::NotFound);
        }
        // Update existing or add new.
        if let Some(m) = state.mappings.iter_mut().find(|m| m.pen_id == pen_id && m.button_index == button_index) {
            m.action = String::from(action);
        } else {
            if state.mappings.len() >= MAX_MAPPINGS {
                return Err(KernelError::ResourceExhausted);
            }
            state.mappings.push(ButtonMapping {
                pen_id, button_index, action: String::from(action),
            });
        }
        Ok(())
    })
}

/// Get button mapping.
pub fn get_button_mapping(pen_id: u32, button_index: u8) -> Option<String> {
    STATE.lock().as_ref().and_then(|s| {
        s.mappings.iter()
            .find(|m| m.pen_id == pen_id && m.button_index == button_index)
            .map(|m| m.action.clone())
    })
}

/// List all pen devices.
pub fn list_pens() -> Vec<PenDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.pens.clone())
}

/// Recent events.
pub fn list_events(count: usize) -> Vec<PenEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.events.len().saturating_sub(count);
        s.events[start..].to_vec()
    })
}

/// Statistics: (pen_count, total_events, total_strokes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.pens.len(), s.total_events, s.total_strokes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("peninput::self_test() — running tests...");
    init_defaults();

    // 1: No pens initially.
    assert!(list_pens().is_empty());
    crate::serial_println!("  [1/10] empty initial: OK");

    // 2: Register pen.
    let caps = PenCapabilities { pressure: true, tilt: true, rotation: false, eraser_tip: true, buttons: 2 };
    let pen_id = register_pen("Wacom Intuos", PenType::Stylus, caps).expect("register");
    assert!(pen_id > 0);
    crate::serial_println!("  [2/10] register: OK");

    // 3: Proximity in.
    proximity_in(pen_id).expect("prox_in");
    let pens = list_pens();
    assert_eq!(pens[0].state, PenState::Hovering);
    crate::serial_println!("  [3/10] proximity in: OK");

    // 4: Contact.
    contact(pen_id, 16000, 8000, 2048).expect("contact");
    let pens = list_pens();
    assert_eq!(pens[0].state, PenState::Contact);
    assert_eq!(pens[0].pressure, 2048);
    crate::serial_println!("  [4/10] contact: OK");

    // 5: Move with tilt.
    report_move(pen_id, 16500, 8500, 3000, 150, -200).expect("move");
    let pens = list_pens();
    assert_eq!(pens[0].x, 16500);
    assert_eq!(pens[0].tilt_x, 150);
    crate::serial_println!("  [5/10] move: OK");

    // 6: Release.
    release(pen_id).expect("release");
    let pens = list_pens();
    assert_eq!(pens[0].state, PenState::Hovering);
    assert_eq!(pens[0].pressure, 0);
    crate::serial_println!("  [6/10] release: OK");

    // 7: Proximity out.
    proximity_out(pen_id).expect("prox_out");
    let pens = list_pens();
    assert_eq!(pens[0].state, PenState::OutOfRange);
    crate::serial_println!("  [7/10] proximity out: OK");

    // 8: Button mapping.
    set_button_mapping(pen_id, 0, "right_click").expect("map");
    let action = get_button_mapping(pen_id, 0).expect("get_map");
    assert_eq!(action, "right_click");
    crate::serial_println!("  [8/10] button mapping: OK");

    // 9: Events logged.
    let events = list_events(10);
    assert!(!events.is_empty());
    crate::serial_println!("  [9/10] events: OK");

    // 10: Stats.
    let (pen_count, total_events, total_strokes, ops) = stats();
    assert_eq!(pen_count, 1);
    assert!(total_events >= 5);
    assert_eq!(total_strokes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("peninput::self_test() — all 10 tests passed");
}
