//! Keyboard layout manager — custom key remapping and layout selection.
//!
//! Allows users to rearrange keyboard keys, disable keys (e.g., CapsLock),
//! create custom named layouts, and switch between them.
//!
//! ## Design Reference
//!
//! design.txt line 1003: "include uncommon but highly optimized keyboard layouts"
//! design.txt line 1330: "arbitrarily rearrange keyboard layout... disabling or
//! moving capslock, can save it as a named layout"
//! design.txt line 1338: "keyboard/layout selection" in install wizard
//!
//! ## Architecture
//!
//! ```text
//! Keyboard driver (scancode → keycode)
//!   → keylayout::translate(keycode) → mapped keycode
//!
//! Settings panel
//!   → keylayout::list_layouts()
//!   → keylayout::set_active(name)
//!   → keylayout::remap(from, to)
//!
//! Installation wizard
//!   → keylayout::set_active("us") / "dvorak" / etc.
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of saved layouts.
const MAX_LAYOUTS: usize = 64;

/// Maximum remappings per layout.
const MAX_REMAPS: usize = 256;

/// Maximum disabled keys per layout.
const MAX_DISABLED: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Standard key identifiers.
///
/// Uses a u16 keycode space.  Values 0x0001..0x00FF correspond to PC/AT
/// scancodes; higher values for modifier keys and special keys.
pub type KeyCode = u16;

/// Well-known key constants.
pub mod keys {
    use super::KeyCode;

    pub const ESCAPE: KeyCode = 0x01;
    pub const KEY_1: KeyCode = 0x02;
    pub const KEY_2: KeyCode = 0x03;
    pub const KEY_3: KeyCode = 0x04;
    pub const KEY_4: KeyCode = 0x05;
    pub const KEY_5: KeyCode = 0x06;
    pub const KEY_6: KeyCode = 0x07;
    pub const KEY_7: KeyCode = 0x08;
    pub const KEY_8: KeyCode = 0x09;
    pub const KEY_9: KeyCode = 0x0A;
    pub const KEY_0: KeyCode = 0x0B;
    pub const MINUS: KeyCode = 0x0C;
    pub const EQUALS: KeyCode = 0x0D;
    pub const BACKSPACE: KeyCode = 0x0E;
    pub const TAB: KeyCode = 0x0F;
    pub const KEY_Q: KeyCode = 0x10;
    pub const KEY_W: KeyCode = 0x11;
    pub const KEY_E: KeyCode = 0x12;
    pub const KEY_R: KeyCode = 0x13;
    pub const KEY_T: KeyCode = 0x14;
    pub const KEY_Y: KeyCode = 0x15;
    pub const KEY_U: KeyCode = 0x16;
    pub const KEY_I: KeyCode = 0x17;
    pub const KEY_O: KeyCode = 0x18;
    pub const KEY_P: KeyCode = 0x19;
    pub const LEFT_BRACKET: KeyCode = 0x1A;
    pub const RIGHT_BRACKET: KeyCode = 0x1B;
    pub const ENTER: KeyCode = 0x1C;
    pub const LEFT_CTRL: KeyCode = 0x1D;
    pub const KEY_A: KeyCode = 0x1E;
    pub const KEY_S: KeyCode = 0x1F;
    pub const KEY_D: KeyCode = 0x20;
    pub const KEY_F: KeyCode = 0x21;
    pub const KEY_G: KeyCode = 0x22;
    pub const KEY_H: KeyCode = 0x23;
    pub const KEY_J: KeyCode = 0x24;
    pub const KEY_K: KeyCode = 0x25;
    pub const KEY_L: KeyCode = 0x26;
    pub const SEMICOLON: KeyCode = 0x27;
    pub const APOSTROPHE: KeyCode = 0x28;
    pub const GRAVE: KeyCode = 0x29;
    pub const LEFT_SHIFT: KeyCode = 0x2A;
    pub const BACKSLASH: KeyCode = 0x2B;
    pub const KEY_Z: KeyCode = 0x2C;
    pub const KEY_X: KeyCode = 0x2D;
    pub const KEY_C: KeyCode = 0x2E;
    pub const KEY_V: KeyCode = 0x2F;
    pub const KEY_B: KeyCode = 0x30;
    pub const KEY_N: KeyCode = 0x31;
    pub const KEY_M: KeyCode = 0x32;
    pub const COMMA: KeyCode = 0x33;
    pub const DOT: KeyCode = 0x34;
    pub const SLASH: KeyCode = 0x35;
    pub const RIGHT_SHIFT: KeyCode = 0x36;
    pub const KP_MULTIPLY: KeyCode = 0x37;
    pub const LEFT_ALT: KeyCode = 0x38;
    pub const SPACE: KeyCode = 0x39;
    pub const CAPS_LOCK: KeyCode = 0x3A;
    pub const F1: KeyCode = 0x3B;
    pub const F2: KeyCode = 0x3C;
    pub const F3: KeyCode = 0x3D;
    pub const F4: KeyCode = 0x3E;
    pub const F5: KeyCode = 0x3F;
    pub const F6: KeyCode = 0x40;
    pub const F7: KeyCode = 0x41;
    pub const F8: KeyCode = 0x42;
    pub const F9: KeyCode = 0x43;
    pub const F10: KeyCode = 0x44;
    pub const F11: KeyCode = 0x57;
    pub const F12: KeyCode = 0x58;
}

/// A keyboard layout definition.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Unique name (e.g., "us", "dvorak", "custom-1").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Key remappings: from_keycode → to_keycode.
    pub remaps: BTreeMap<KeyCode, KeyCode>,
    /// Disabled keys (produce no output).
    pub disabled: Vec<KeyCode>,
    /// Whether this is a built-in or user-created layout.
    pub builtin: bool,
}

impl Layout {
    fn new(name: &str, desc: &str, builtin: bool) -> Self {
        Self {
            name: String::from(name),
            description: String::from(desc),
            remaps: BTreeMap::new(),
            disabled: Vec::new(),
            builtin,
        }
    }

    /// Translate a keycode through this layout.
    ///
    /// Returns `None` if the key is disabled, `Some(mapped)` otherwise.
    pub fn translate(&self, key: KeyCode) -> Option<KeyCode> {
        if self.disabled.contains(&key) {
            return None;
        }
        Some(self.remaps.get(&key).copied().unwrap_or(key))
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// All saved layouts, keyed by name.
    layouts: BTreeMap<String, Layout>,
    /// Name of the active layout (empty = passthrough, no remapping).
    active: String,
}

impl State {
    const fn new() -> Self {
        Self {
            layouts: BTreeMap::new(),
            active: String::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static TRANSLATE_COUNT: AtomicU64 = AtomicU64::new(0);
static SWITCH_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Layout management
// ---------------------------------------------------------------------------

/// Create a new empty layout.
pub fn create_layout(name: &str, desc: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.layouts.len() >= MAX_LAYOUTS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.layouts.contains_key(name) {
        return Err(KernelError::AlreadyExists);
    }
    state.layouts.insert(String::from(name), Layout::new(name, desc, false));
    Ok(())
}

/// Remove a layout.  Cannot remove the active layout.
pub fn remove_layout(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.active == name {
        return Err(KernelError::InvalidArgument);
    }
    state.layouts.remove(name).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Get a layout by name.
pub fn get_layout(name: &str) -> Option<Layout> {
    STATE.lock().layouts.get(name).cloned()
}

/// List all layout names and descriptions.
pub fn list_layouts() -> Vec<(String, String, bool)> {
    let state = STATE.lock();
    state.layouts.values()
        .map(|l| (l.name.clone(), l.description.clone(), l.builtin))
        .collect()
}

/// Set the active layout.
pub fn set_active(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !name.is_empty() && !state.layouts.contains_key(name) {
        return Err(KernelError::NotFound);
    }
    state.active = String::from(name);
    SWITCH_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get the active layout name (empty = no remapping).
pub fn active() -> String {
    STATE.lock().active.clone()
}

/// Get the active layout (if any).
pub fn active_layout() -> Option<Layout> {
    let state = STATE.lock();
    if state.active.is_empty() {
        return None;
    }
    state.layouts.get(&state.active).cloned()
}

// ---------------------------------------------------------------------------
// Key remapping
// ---------------------------------------------------------------------------

/// Add a key remapping to a layout.
pub fn remap(layout_name: &str, from: KeyCode, to: KeyCode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let layout = state.layouts.get_mut(layout_name)
        .ok_or(KernelError::NotFound)?;
    if layout.remaps.len() >= MAX_REMAPS {
        return Err(KernelError::ResourceExhausted);
    }
    layout.remaps.insert(from, to);
    Ok(())
}

/// Remove a remapping from a layout.
pub fn unmap(layout_name: &str, from: KeyCode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let layout = state.layouts.get_mut(layout_name)
        .ok_or(KernelError::NotFound)?;
    layout.remaps.remove(&from);
    Ok(())
}

/// Disable a key in a layout (key produces no output).
pub fn disable_key(layout_name: &str, key: KeyCode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let layout = state.layouts.get_mut(layout_name)
        .ok_or(KernelError::NotFound)?;
    if layout.disabled.len() >= MAX_DISABLED {
        return Err(KernelError::ResourceExhausted);
    }
    if !layout.disabled.contains(&key) {
        layout.disabled.push(key);
    }
    Ok(())
}

/// Re-enable a disabled key.
pub fn enable_key(layout_name: &str, key: KeyCode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let layout = state.layouts.get_mut(layout_name)
        .ok_or(KernelError::NotFound)?;
    layout.disabled.retain(|&k| k != key);
    Ok(())
}

// ---------------------------------------------------------------------------
// Translation (called on every keypress)
// ---------------------------------------------------------------------------

/// Translate a keycode through the active layout.
///
/// Returns `None` if the key is disabled, `Some(mapped_key)` otherwise.
/// If no layout is active, returns the key unchanged.
pub fn translate(key: KeyCode) -> Option<KeyCode> {
    TRANSLATE_COUNT.fetch_add(1, Ordering::Relaxed);
    let state = STATE.lock();
    if state.active.is_empty() {
        return Some(key);
    }
    match state.layouts.get(&state.active) {
        Some(layout) => layout.translate(key),
        None => Some(key), // Layout not found, passthrough.
    }
}

// ---------------------------------------------------------------------------
// Built-in layouts
// ---------------------------------------------------------------------------

/// Register built-in keyboard layouts.
pub fn init_defaults() -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.layouts.contains_key("us") {
        return Ok(()); // Already initialized.
    }

    // US QWERTY — identity mapping (no remaps needed, it's the default).
    state.layouts.insert(
        String::from("us"),
        Layout::new("us", "US QWERTY (standard)", true),
    );

    // Dvorak layout.
    let mut dvorak = Layout::new("dvorak", "Dvorak Simplified Keyboard", true);
    // Top row: QWERTY → Dvorak
    dvorak.remaps.insert(keys::KEY_Q, keys::APOSTROPHE);
    dvorak.remaps.insert(keys::KEY_W, keys::COMMA);
    dvorak.remaps.insert(keys::KEY_E, keys::DOT);
    dvorak.remaps.insert(keys::KEY_R, keys::KEY_P);
    dvorak.remaps.insert(keys::KEY_T, keys::KEY_Y);
    dvorak.remaps.insert(keys::KEY_Y, keys::KEY_F);
    dvorak.remaps.insert(keys::KEY_U, keys::KEY_G);
    dvorak.remaps.insert(keys::KEY_I, keys::KEY_C);
    dvorak.remaps.insert(keys::KEY_O, keys::KEY_R);
    dvorak.remaps.insert(keys::KEY_P, keys::KEY_L);
    // Home row
    dvorak.remaps.insert(keys::KEY_S, keys::KEY_O);
    dvorak.remaps.insert(keys::KEY_D, keys::KEY_E);
    dvorak.remaps.insert(keys::KEY_F, keys::KEY_U);
    dvorak.remaps.insert(keys::KEY_G, keys::KEY_I);
    dvorak.remaps.insert(keys::KEY_H, keys::KEY_D);
    dvorak.remaps.insert(keys::KEY_J, keys::KEY_H);
    dvorak.remaps.insert(keys::KEY_K, keys::KEY_T);
    dvorak.remaps.insert(keys::KEY_L, keys::KEY_N);
    dvorak.remaps.insert(keys::SEMICOLON, keys::KEY_S);
    // Bottom row
    dvorak.remaps.insert(keys::KEY_Z, keys::SEMICOLON);
    dvorak.remaps.insert(keys::KEY_X, keys::KEY_Q);
    dvorak.remaps.insert(keys::KEY_C, keys::KEY_J);
    dvorak.remaps.insert(keys::KEY_V, keys::KEY_K);
    dvorak.remaps.insert(keys::KEY_B, keys::KEY_X);
    dvorak.remaps.insert(keys::KEY_N, keys::KEY_B);
    dvorak.remaps.insert(keys::KEY_M, keys::KEY_M); // Same.
    dvorak.remaps.insert(keys::COMMA, keys::KEY_W);
    dvorak.remaps.insert(keys::DOT, keys::KEY_V);
    dvorak.remaps.insert(keys::SLASH, keys::KEY_Z);
    state.layouts.insert(String::from("dvorak"), dvorak);

    // Colemak layout.
    let mut colemak = Layout::new("colemak", "Colemak", true);
    colemak.remaps.insert(keys::KEY_E, keys::KEY_F);
    colemak.remaps.insert(keys::KEY_R, keys::KEY_P);
    colemak.remaps.insert(keys::KEY_T, keys::KEY_G);
    colemak.remaps.insert(keys::KEY_Y, keys::KEY_J);
    colemak.remaps.insert(keys::KEY_U, keys::KEY_L);
    colemak.remaps.insert(keys::KEY_I, keys::KEY_U);
    colemak.remaps.insert(keys::KEY_O, keys::KEY_Y);
    colemak.remaps.insert(keys::KEY_P, keys::SEMICOLON);
    colemak.remaps.insert(keys::KEY_S, keys::KEY_R);
    colemak.remaps.insert(keys::KEY_D, keys::KEY_S);
    colemak.remaps.insert(keys::KEY_F, keys::KEY_T);
    colemak.remaps.insert(keys::KEY_G, keys::KEY_D);
    colemak.remaps.insert(keys::KEY_J, keys::KEY_N);
    colemak.remaps.insert(keys::KEY_K, keys::KEY_E);
    colemak.remaps.insert(keys::KEY_L, keys::KEY_I);
    colemak.remaps.insert(keys::SEMICOLON, keys::KEY_O);
    colemak.remaps.insert(keys::KEY_N, keys::KEY_K);
    state.layouts.insert(String::from("colemak"), colemak);

    // "No CapsLock" variant — CapsLock becomes extra Ctrl.
    let mut nocaps = Layout::new("us-nocaps", "US QWERTY (CapsLock → Ctrl)", true);
    nocaps.remaps.insert(keys::CAPS_LOCK, keys::LEFT_CTRL);
    state.layouts.insert(String::from("us-nocaps"), nocaps);

    // Set US as default active.
    state.active = String::from("us");
    Ok(())
}

/// Get a human-readable name for a keycode.
pub fn key_name(code: KeyCode) -> &'static str {
    match code {
        keys::ESCAPE => "Esc",
        keys::KEY_1 => "1", keys::KEY_2 => "2", keys::KEY_3 => "3",
        keys::KEY_4 => "4", keys::KEY_5 => "5", keys::KEY_6 => "6",
        keys::KEY_7 => "7", keys::KEY_8 => "8", keys::KEY_9 => "9",
        keys::KEY_0 => "0",
        keys::MINUS => "-", keys::EQUALS => "=",
        keys::BACKSPACE => "Backspace", keys::TAB => "Tab",
        keys::KEY_Q => "Q", keys::KEY_W => "W", keys::KEY_E => "E",
        keys::KEY_R => "R", keys::KEY_T => "T", keys::KEY_Y => "Y",
        keys::KEY_U => "U", keys::KEY_I => "I", keys::KEY_O => "O",
        keys::KEY_P => "P",
        keys::LEFT_BRACKET => "[", keys::RIGHT_BRACKET => "]",
        keys::ENTER => "Enter", keys::LEFT_CTRL => "LCtrl",
        keys::KEY_A => "A", keys::KEY_S => "S", keys::KEY_D => "D",
        keys::KEY_F => "F", keys::KEY_G => "G", keys::KEY_H => "H",
        keys::KEY_J => "J", keys::KEY_K => "K", keys::KEY_L => "L",
        keys::SEMICOLON => ";", keys::APOSTROPHE => "'",
        keys::GRAVE => "`", keys::LEFT_SHIFT => "LShift",
        keys::BACKSLASH => "\\",
        keys::KEY_Z => "Z", keys::KEY_X => "X", keys::KEY_C => "C",
        keys::KEY_V => "V", keys::KEY_B => "B", keys::KEY_N => "N",
        keys::KEY_M => "M",
        keys::COMMA => ",", keys::DOT => ".", keys::SLASH => "/",
        keys::RIGHT_SHIFT => "RShift",
        keys::LEFT_ALT => "LAlt", keys::SPACE => "Space",
        keys::CAPS_LOCK => "CapsLock",
        keys::F1 => "F1", keys::F2 => "F2", keys::F3 => "F3",
        keys::F4 => "F4", keys::F5 => "F5", keys::F6 => "F6",
        keys::F7 => "F7", keys::F8 => "F8", keys::F9 => "F9",
        keys::F10 => "F10", keys::F11 => "F11", keys::F12 => "F12",
        _ => "?",
    }
}

/// Parse a key name to keycode.
pub fn parse_key(name: &str) -> Option<KeyCode> {
    match name.to_ascii_lowercase().as_str() {
        "esc" | "escape" => Some(keys::ESCAPE),
        "1" => Some(keys::KEY_1), "2" => Some(keys::KEY_2),
        "3" => Some(keys::KEY_3), "4" => Some(keys::KEY_4),
        "5" => Some(keys::KEY_5), "6" => Some(keys::KEY_6),
        "7" => Some(keys::KEY_7), "8" => Some(keys::KEY_8),
        "9" => Some(keys::KEY_9), "0" => Some(keys::KEY_0),
        "backspace" | "bs" => Some(keys::BACKSPACE),
        "tab" => Some(keys::TAB),
        "q" => Some(keys::KEY_Q), "w" => Some(keys::KEY_W),
        "e" => Some(keys::KEY_E), "r" => Some(keys::KEY_R),
        "t" => Some(keys::KEY_T), "y" => Some(keys::KEY_Y),
        "u" => Some(keys::KEY_U), "i" => Some(keys::KEY_I),
        "o" => Some(keys::KEY_O), "p" => Some(keys::KEY_P),
        "enter" | "return" => Some(keys::ENTER),
        "lctrl" | "leftctrl" => Some(keys::LEFT_CTRL),
        "a" => Some(keys::KEY_A), "s" => Some(keys::KEY_S),
        "d" => Some(keys::KEY_D), "f" => Some(keys::KEY_F),
        "g" => Some(keys::KEY_G), "h" => Some(keys::KEY_H),
        "j" => Some(keys::KEY_J), "k" => Some(keys::KEY_K),
        "l" => Some(keys::KEY_L),
        "lshift" | "leftshift" => Some(keys::LEFT_SHIFT),
        "z" => Some(keys::KEY_Z), "x" => Some(keys::KEY_X),
        "c" => Some(keys::KEY_C), "v" => Some(keys::KEY_V),
        "b" => Some(keys::KEY_B), "n" => Some(keys::KEY_N),
        "m" => Some(keys::KEY_M),
        "rshift" | "rightshift" => Some(keys::RIGHT_SHIFT),
        "lalt" | "leftalt" => Some(keys::LEFT_ALT),
        "space" => Some(keys::SPACE),
        "capslock" | "caps" => Some(keys::CAPS_LOCK),
        "f1" => Some(keys::F1), "f2" => Some(keys::F2),
        "f3" => Some(keys::F3), "f4" => Some(keys::F4),
        "f5" => Some(keys::F5), "f6" => Some(keys::F6),
        "f7" => Some(keys::F7), "f8" => Some(keys::F8),
        "f9" => Some(keys::F9), "f10" => Some(keys::F10),
        "f11" => Some(keys::F11), "f12" => Some(keys::F12),
        _ => name.parse::<u16>().ok(),
    }
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (layout_count, remap_count, translate_count, switch_count).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    let lc = state.layouts.len();
    let rc: usize = state.layouts.values().map(|l| l.remaps.len()).sum();
    (lc, rc, TRANSLATE_COUNT.load(Ordering::Relaxed), SWITCH_COUNT.load(Ordering::Relaxed))
}

/// Reset statistics.
pub fn reset_stats() {
    TRANSLATE_COUNT.store(0, Ordering::Relaxed);
    SWITCH_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all state.
pub fn clear_all() {
    let mut state = STATE.lock();
    state.layouts.clear();
    state.active = String::new();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Create and list layouts.
    serial_println!("  keylayout::self_test 1: create layouts");
    create_layout("test1", "Test Layout 1")?;
    create_layout("test2", "Test Layout 2")?;
    assert_eq!(list_layouts().len(), 2);

    // Test 2: Key remapping.
    serial_println!("  keylayout::self_test 2: remapping");
    remap("test1", keys::CAPS_LOCK, keys::LEFT_CTRL)?;
    remap("test1", keys::KEY_A, keys::KEY_B)?;
    set_active("test1")?;
    assert_eq!(translate(keys::CAPS_LOCK), Some(keys::LEFT_CTRL));
    assert_eq!(translate(keys::KEY_A), Some(keys::KEY_B));
    assert_eq!(translate(keys::KEY_Z), Some(keys::KEY_Z)); // Unmapped → passthrough.

    // Test 3: Disable key.
    serial_println!("  keylayout::self_test 3: disable key");
    disable_key("test1", keys::KEY_Q)?;
    assert_eq!(translate(keys::KEY_Q), None); // Disabled.
    enable_key("test1", keys::KEY_Q)?;
    assert_eq!(translate(keys::KEY_Q), Some(keys::KEY_Q));

    // Test 4: Switch layout.
    serial_println!("  keylayout::self_test 4: switch layout");
    set_active("test2")?;
    // test2 has no remaps — everything is passthrough.
    assert_eq!(translate(keys::CAPS_LOCK), Some(keys::CAPS_LOCK));
    assert_eq!(translate(keys::KEY_A), Some(keys::KEY_A));

    // Test 5: No active layout.
    serial_println!("  keylayout::self_test 5: passthrough");
    set_active("")?;
    assert_eq!(translate(keys::KEY_A), Some(keys::KEY_A));

    // Test 6: Built-in layouts.
    serial_println!("  keylayout::self_test 6: init defaults");
    clear_all();
    init_defaults()?;
    let layouts = list_layouts();
    assert!(layouts.len() >= 4); // us, dvorak, colemak, us-nocaps
    set_active("dvorak")?;
    // In Dvorak, QWERTY 'S' → 'O'.
    assert_eq!(translate(keys::KEY_S), Some(keys::KEY_O));
    set_active("us-nocaps")?;
    assert_eq!(translate(keys::CAPS_LOCK), Some(keys::LEFT_CTRL));

    // Test 7: Remove and unmap.
    serial_println!("  keylayout::self_test 7: remove/unmap");
    set_active("us")?;
    create_layout("temp", "Temporary")?;
    remap("temp", keys::KEY_A, keys::KEY_Z)?;
    unmap("temp", keys::KEY_A)?;
    let temp = get_layout("temp");
    assert!(temp.is_some());
    assert!(temp.as_ref().map(|l| l.remaps.is_empty()).unwrap_or(false));
    remove_layout("temp")?;
    assert!(get_layout("temp").is_none());

    let (lc, _rc, tc, sc) = stats();
    assert!(lc >= 4);
    assert!(tc > 0);
    assert!(sc > 0);

    clear_all();
    reset_stats();
    serial_println!("  keylayout: all tests passed");
    Ok(())
}
