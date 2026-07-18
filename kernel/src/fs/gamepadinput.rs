//! Gamepad input — controller/gamepad input management.
//!
//! Handles game controller detection, button/axis mapping,
//! rumble/vibration feedback, dead zone configuration,
//! and per-game profile support.
//!
//! ## Architecture
//!
//! ```text
//! USB/Bluetooth gamepad detected
//!   → gamepadinput::register_gamepad() → new device entry
//!
//! Game/application
//!   → gamepadinput::poll_state(id) → button/axis state
//!   → gamepadinput::set_rumble(id, intensity) → vibration
//!
//! Settings panel → Input → Controllers
//!   → gamepadinput::list_gamepads() → connected controllers
//!   → gamepadinput::set_mapping() → button remapping
//!
//! Integration:
//!   → usbmgr (USB controller detection)
//!   → bluetooth (wireless controller pairing)
//!   → focusassist (disable input when not focused)
//!   → devicemgr (device enumeration)
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

/// Gamepad type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadType {
    Xbox,
    PlayStation,
    NintendoSwitch,
    Generic,
    FlightStick,
    RacingWheel,
}

impl GamepadType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Xbox => "Xbox",
            Self::PlayStation => "PlayStation",
            Self::NintendoSwitch => "Switch",
            Self::Generic => "Generic",
            Self::FlightStick => "Flight Stick",
            Self::RacingWheel => "Racing Wheel",
        }
    }
}

/// Connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    Usb,
    Bluetooth,
    Wireless24Ghz,
}

impl ConnectionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Bluetooth => "Bluetooth",
            Self::Wireless24Ghz => "2.4 GHz",
        }
    }
}

/// Standard gamepad button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadButton {
    A, B, X, Y,
    LeftBumper, RightBumper,
    LeftTrigger, RightTrigger,
    LeftStick, RightStick,
    DpadUp, DpadDown, DpadLeft, DpadRight,
    Start, Select, Home,
}

impl GamepadButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A", Self::B => "B", Self::X => "X", Self::Y => "Y",
            Self::LeftBumper => "LB", Self::RightBumper => "RB",
            Self::LeftTrigger => "LT", Self::RightTrigger => "RT",
            Self::LeftStick => "LS", Self::RightStick => "RS",
            Self::DpadUp => "D-Up", Self::DpadDown => "D-Down",
            Self::DpadLeft => "D-Left", Self::DpadRight => "D-Right",
            Self::Start => "Start", Self::Select => "Select", Self::Home => "Home",
        }
    }
}

/// Axis state (integer, -10000 to +10000).
#[derive(Debug, Clone, Copy)]
pub struct AxisState {
    pub left_x: i32,
    pub left_y: i32,
    pub right_x: i32,
    pub right_y: i32,
    pub left_trigger: i32,  // 0 to 10000
    pub right_trigger: i32, // 0 to 10000
}

/// A connected gamepad.
#[derive(Debug, Clone)]
pub struct Gamepad {
    /// Gamepad ID.
    pub id: u32,
    /// Name.
    pub name: String,
    /// Type.
    pub gamepad_type: GamepadType,
    /// Connection.
    pub connection: ConnectionType,
    /// Battery percent (0-100, 255 = wired/unknown).
    pub battery_percent: u8,
    /// Whether connected.
    pub connected: bool,
    /// Button states (pressed = true).
    pub buttons: u32, // Bitmask for up to 32 buttons.
    /// Axis state.
    pub axes: AxisState,
    /// Dead zone (0-5000).
    pub dead_zone: i32,
    /// Rumble intensity (0-10000).
    pub rumble_intensity: u32,
    /// Player number (1-4, 0 = unassigned).
    pub player_number: u8,
    /// Total button presses.
    pub total_presses: u64,
    /// Connected timestamp (ns).
    pub connected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GAMEPADS: usize = 8;

struct State {
    gamepads: Vec<Gamepad>,
    next_id: u32,
    total_connected: u64,
    total_disconnected: u64,
    total_inputs: u64,
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
        gamepads: Vec::new(),
        next_id: 1,
        total_connected: 0,
        total_disconnected: 0,
        total_inputs: 0,
        ops: 0,
    });
}

/// Register a new gamepad.
pub fn register_gamepad(
    name: &str, gamepad_type: GamepadType, connection: ConnectionType, battery: u8,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.gamepads.len() >= MAX_GAMEPADS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_connected += 1;

        // Assign player number.
        let mut assigned = [false; 4];
        for g in &state.gamepads {
            if g.player_number >= 1 && g.player_number <= 4 {
                assigned[(g.player_number - 1) as usize] = true;
            }
        }
        let player = assigned.iter().position(|a| !a).map(|p| (p + 1) as u8).unwrap_or(0);

        state.gamepads.push(Gamepad {
            id, name: String::from(name), gamepad_type, connection,
            battery_percent: battery, connected: true,
            buttons: 0,
            axes: AxisState { left_x: 0, left_y: 0, right_x: 0, right_y: 0, left_trigger: 0, right_trigger: 0 },
            dead_zone: 1500, rumble_intensity: 0, player_number: player,
            total_presses: 0, connected_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Disconnect a gamepad.
pub fn disconnect_gamepad(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let gp = state.gamepads.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        gp.connected = false;
        state.total_disconnected += 1;
        Ok(())
    })
}

/// Remove a gamepad entirely.
pub fn remove_gamepad(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.gamepads.iter().position(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        state.gamepads.remove(pos);
        Ok(())
    })
}

/// Update button state (bitmask).
pub fn update_buttons(id: u32, buttons: u32) -> KernelResult<()> {
    with_state(|state| {
        let gp = state.gamepads.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        // Count new presses (bits that changed from 0 to 1).
        let new_presses = buttons & !gp.buttons;
        gp.total_presses += new_presses.count_ones() as u64;
        gp.buttons = buttons;
        state.total_inputs += 1;
        Ok(())
    })
}

/// Update axis state.
pub fn update_axes(id: u32, axes: AxisState) -> KernelResult<()> {
    with_state(|state| {
        let gp = state.gamepads.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        gp.axes = axes;
        state.total_inputs += 1;
        Ok(())
    })
}

/// Set dead zone.
pub fn set_dead_zone(id: u32, dead_zone: i32) -> KernelResult<()> {
    with_state(|state| {
        let gp = state.gamepads.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        gp.dead_zone = dead_zone.clamp(0, 5000);
        Ok(())
    })
}

/// Set rumble intensity (0-10000).
pub fn set_rumble(id: u32, intensity: u32) -> KernelResult<()> {
    with_state(|state| {
        let gp = state.gamepads.iter_mut().find(|g| g.id == id)
            .ok_or(KernelError::NotFound)?;
        gp.rumble_intensity = intensity.min(10000);
        Ok(())
    })
}

/// Get gamepad info.
pub fn get_gamepad(id: u32) -> KernelResult<Gamepad> {
    with_state(|state| {
        state.gamepads.iter().find(|g| g.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all gamepads.
pub fn list_gamepads() -> Vec<Gamepad> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.gamepads.clone())
}

/// Statistics: (gamepad_count, connected_count, total_connected, total_inputs, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let connected = s.gamepads.iter().filter(|g| g.connected).count();
            (s.gamepads.len(), connected, s.total_connected, s.total_inputs, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("gamepadinput::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_gamepads().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Register Xbox controller.
    let id1 = register_gamepad("Xbox Wireless", GamepadType::Xbox, ConnectionType::Bluetooth, 85)
        .expect("register xbox");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] register xbox: OK");

    // 3: Player assignment.
    let gp = get_gamepad(id1).expect("get");
    assert_eq!(gp.player_number, 1);
    crate::serial_println!("  [3/11] player assignment: OK");

    // 4: Register second controller.
    let id2 = register_gamepad("DualSense", GamepadType::PlayStation, ConnectionType::Usb, 255)
        .expect("register ps");
    let gp2 = get_gamepad(id2).expect("get2");
    assert_eq!(gp2.player_number, 2);
    crate::serial_println!("  [4/11] second controller: OK");

    // 5: Button input.
    update_buttons(id1, 0b0101).expect("buttons"); // A + X pressed
    let gp = get_gamepad(id1).expect("get3");
    assert_eq!(gp.buttons, 0b0101);
    assert_eq!(gp.total_presses, 2);
    crate::serial_println!("  [5/11] button input: OK");

    // 6: Axis input.
    let axes = AxisState { left_x: 5000, left_y: -3000, right_x: 0, right_y: 0, left_trigger: 8000, right_trigger: 0 };
    update_axes(id1, axes).expect("axes");
    let gp = get_gamepad(id1).expect("get4");
    assert_eq!(gp.axes.left_x, 5000);
    crate::serial_println!("  [6/11] axis input: OK");

    // 7: Dead zone.
    set_dead_zone(id1, 2000).expect("deadzone");
    let gp = get_gamepad(id1).expect("get5");
    assert_eq!(gp.dead_zone, 2000);
    crate::serial_println!("  [7/11] dead zone: OK");

    // 8: Rumble.
    set_rumble(id1, 7500).expect("rumble");
    let gp = get_gamepad(id1).expect("get6");
    assert_eq!(gp.rumble_intensity, 7500);
    crate::serial_println!("  [8/11] rumble: OK");

    // 9: Disconnect.
    disconnect_gamepad(id2).expect("disconnect");
    let gp = get_gamepad(id2).expect("get7");
    assert!(!gp.connected);
    crate::serial_println!("  [9/11] disconnect: OK");

    // 10: Remove.
    remove_gamepad(id2).expect("remove");
    assert_eq!(list_gamepads().len(), 1);
    crate::serial_println!("  [10/11] remove: OK");

    // 11: Stats.
    let (count, connected, total_conn, total_inp, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(connected, 1);
    assert_eq!(total_conn, 2);
    assert!(total_inp > 0);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("gamepadinput::self_test() — all 11 tests passed");
}
