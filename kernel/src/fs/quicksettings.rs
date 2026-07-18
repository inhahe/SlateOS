//! Quick Settings — system quick settings panel management.
//!
//! Provides a toggleable panel of frequently-accessed system settings
//! (WiFi, Bluetooth, brightness, volume, airplane mode, night light, etc.)
//! with quick toggle and slider controls.
//!
//! ## Architecture
//!
//! ```text
//! User opens quick settings panel
//!   → quicksettings::get_tiles() → current toggle/slider states
//!
//! User toggles setting
//!   → quicksettings::toggle(tile_id) → on/off
//!   → quicksettings::set_value(tile_id, val) → slider
//!
//! Integration:
//!   → systray (panel trigger)
//!   → bluetooth, netsettings, soundmixer, brightness, nightlight
//!   → power (airplane mode, battery saver)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Quick settings tile type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileType {
    Toggle,
    Slider,
    Action,
}

impl TileType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Toggle",
            Self::Slider => "Slider",
            Self::Action => "Action",
        }
    }
}

/// A quick settings tile.
#[derive(Debug, Clone)]
pub struct SettingsTile {
    pub id: u32,
    pub name: String,
    pub icon: String,
    pub tile_type: TileType,
    /// Whether enabled (for toggles).
    pub enabled: bool,
    /// Value (0-100 for sliders, 0/1 for toggles).
    pub value: u32,
    /// Subtitle text (e.g., "Connected to Home WiFi").
    pub subtitle: String,
    /// Position in the panel (lower = higher).
    pub position: u32,
    /// Whether visible in the panel.
    pub visible: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TILES: usize = 30;

struct State {
    tiles: Vec<SettingsTile>,
    next_id: u32,
    panel_open: bool,
    total_toggles: u64,
    total_adjustments: u64,
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

    let tiles = alloc::vec![
        SettingsTile { id: 1, name: String::from("Wi-Fi"), icon: String::from("wifi"),
            tile_type: TileType::Toggle, enabled: true, value: 1,
            subtitle: String::from("Connected"), position: 1, visible: true },
        SettingsTile { id: 2, name: String::from("Bluetooth"), icon: String::from("bluetooth"),
            tile_type: TileType::Toggle, enabled: true, value: 1,
            subtitle: String::from("On"), position: 2, visible: true },
        SettingsTile { id: 3, name: String::from("Airplane Mode"), icon: String::from("airplane"),
            tile_type: TileType::Toggle, enabled: false, value: 0,
            subtitle: String::from("Off"), position: 3, visible: true },
        SettingsTile { id: 4, name: String::from("Night Light"), icon: String::from("nightlight"),
            tile_type: TileType::Toggle, enabled: false, value: 0,
            subtitle: String::from("Off"), position: 4, visible: true },
        SettingsTile { id: 5, name: String::from("Brightness"), icon: String::from("brightness"),
            tile_type: TileType::Slider, enabled: true, value: 70,
            subtitle: String::from("70%"), position: 5, visible: true },
        SettingsTile { id: 6, name: String::from("Volume"), icon: String::from("volume"),
            tile_type: TileType::Slider, enabled: true, value: 50,
            subtitle: String::from("50%"), position: 6, visible: true },
        SettingsTile { id: 7, name: String::from("Do Not Disturb"), icon: String::from("dnd"),
            tile_type: TileType::Toggle, enabled: false, value: 0,
            subtitle: String::from("Off"), position: 7, visible: true },
        SettingsTile { id: 8, name: String::from("Battery Saver"), icon: String::from("battery"),
            tile_type: TileType::Toggle, enabled: false, value: 0,
            subtitle: String::from("Off"), position: 8, visible: true },
    ];

    *guard = Some(State {
        tiles,
        next_id: 9,
        panel_open: false,
        total_toggles: 0,
        total_adjustments: 0,
        ops: 0,
    });
}

/// Toggle a setting on/off.
pub fn toggle(tile_id: u32) -> KernelResult<bool> {
    with_state(|state| {
        let tile = state.tiles.iter_mut().find(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        if tile.tile_type != TileType::Toggle {
            return Err(KernelError::InvalidArgument);
        }
        tile.enabled = !tile.enabled;
        tile.value = if tile.enabled { 1 } else { 0 };
        tile.subtitle = String::from(if tile.enabled { "On" } else { "Off" });
        state.total_toggles += 1;
        Ok(tile.enabled)
    })
}

/// Set slider value (0-100).
pub fn set_value(tile_id: u32, value: u32) -> KernelResult<()> {
    with_state(|state| {
        let tile = state.tiles.iter_mut().find(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        if tile.tile_type != TileType::Slider {
            return Err(KernelError::InvalidArgument);
        }
        tile.value = value.min(100);
        tile.subtitle = format!("{}%", tile.value);
        state.total_adjustments += 1;
        Ok(())
    })
}

/// Set subtitle text for a tile.
pub fn set_subtitle(tile_id: u32, subtitle: &str) -> KernelResult<()> {
    with_state(|state| {
        let tile = state.tiles.iter_mut().find(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        tile.subtitle = String::from(subtitle);
        Ok(())
    })
}

/// Add a custom tile.
pub fn add_tile(name: &str, icon: &str, tile_type: TileType) -> KernelResult<u32> {
    with_state(|state| {
        if state.tiles.len() >= MAX_TILES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        let position = state.tiles.len() as u32 + 1;
        state.tiles.push(SettingsTile {
            id, name: String::from(name), icon: String::from(icon),
            tile_type, enabled: false, value: 0,
            subtitle: String::from("Off"), position, visible: true,
        });
        Ok(id)
    })
}

/// Remove a tile.
pub fn remove_tile(tile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.tiles.iter().position(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        state.tiles.remove(pos);
        Ok(())
    })
}

/// Show/hide a tile.
pub fn set_visible(tile_id: u32, visible: bool) -> KernelResult<()> {
    with_state(|state| {
        let tile = state.tiles.iter_mut().find(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        tile.visible = visible;
        Ok(())
    })
}

/// Reorder a tile.
pub fn set_position(tile_id: u32, position: u32) -> KernelResult<()> {
    with_state(|state| {
        let tile = state.tiles.iter_mut().find(|t| t.id == tile_id)
            .ok_or(KernelError::NotFound)?;
        tile.position = position;
        Ok(())
    })
}

/// Open/close panel.
pub fn set_panel_open(open: bool) -> KernelResult<()> {
    with_state(|state| {
        state.panel_open = open;
        Ok(())
    })
}

/// Check if panel is open.
pub fn is_panel_open() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.panel_open)
}

/// Get all visible tiles sorted by position.
pub fn get_tiles() -> Vec<SettingsTile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut tiles: Vec<SettingsTile> = s.tiles.iter()
            .filter(|t| t.visible)
            .cloned()
            .collect();
        tiles.sort_by_key(|t| t.position);
        tiles
    })
}

/// Get all tiles (including hidden).
pub fn list_all() -> Vec<SettingsTile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tiles.clone())
}

/// Statistics: (tile_count, total_toggles, total_adjustments, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tiles.len(), s.total_toggles, s.total_adjustments, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("quicksettings::self_test() — running tests...");
    init_defaults();

    // 1: Default tiles.
    let tiles = get_tiles();
    assert_eq!(tiles.len(), 8);
    crate::serial_println!("  [1/10] default tiles: OK");

    // 2: Toggle WiFi off.
    let was_on = toggle(1).expect("toggle");
    assert!(!was_on); // Was on, now off.
    crate::serial_println!("  [2/10] toggle: OK");

    // 3: Toggle back on.
    let now_on = toggle(1).expect("toggle2");
    assert!(now_on);
    crate::serial_println!("  [3/10] toggle back: OK");

    // 4: Set slider.
    set_value(5, 85).expect("slider");
    let tiles = get_tiles();
    let brightness = tiles.iter().find(|t| t.id == 5).expect("find");
    assert_eq!(brightness.value, 85);
    assert_eq!(brightness.subtitle, "85%");
    crate::serial_println!("  [4/10] slider: OK");

    // 5: Add custom tile.
    let custom = add_tile("VPN", "vpn", TileType::Toggle).expect("add");
    assert_eq!(get_tiles().len(), 9);
    crate::serial_println!("  [5/10] add tile: OK");

    // 6: Hide tile.
    set_visible(custom, false).expect("hide");
    assert_eq!(get_tiles().len(), 8); // Hidden.
    assert_eq!(list_all().len(), 9);   // Still exists.
    crate::serial_println!("  [6/10] hide tile: OK");

    // 7: Remove tile.
    remove_tile(custom).expect("remove");
    assert_eq!(list_all().len(), 8);
    crate::serial_println!("  [7/10] remove tile: OK");

    // 8: Set subtitle.
    set_subtitle(1, "Connected to Office").expect("subtitle");
    let tiles = get_tiles();
    let wifi = tiles.iter().find(|t| t.id == 1).expect("wifi");
    assert_eq!(wifi.subtitle, "Connected to Office");
    crate::serial_println!("  [8/10] subtitle: OK");

    // 9: Panel open/close.
    set_panel_open(true).expect("open");
    assert!(is_panel_open());
    set_panel_open(false).expect("close");
    assert!(!is_panel_open());
    crate::serial_println!("  [9/10] panel state: OK");

    // 10: Stats.
    let (count, toggles, adjustments, ops) = stats();
    assert_eq!(count, 8);
    assert!(toggles >= 2);
    assert_eq!(adjustments, 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("quicksettings::self_test() — all 10 tests passed");
}
