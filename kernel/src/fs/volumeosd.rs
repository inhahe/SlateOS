//! Volume OSD — on-screen volume/brightness indicator display.
//!
//! Manages the transient overlay that appears when the user adjusts
//! volume, brightness, or other media keys via hardware buttons.
//!
//! ## Architecture
//!
//! ```text
//! User presses volume key
//!   → volumeosd::show(OsdType::Volume, level, icon)
//!     → displays overlay for timeout duration
//!     → auto-hides after delay
//!
//! Integration:
//!   → soundmixer (volume change events)
//!   → brightness (brightness change events)
//!   → mediakeys (track change info)
//!   → hotkeys (media key events)
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

/// OSD indicator type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdType {
    Volume,
    Mute,
    Brightness,
    MediaPlay,
    MediaPause,
    MediaNext,
    MediaPrev,
    KeyboardBacklight,
    AirplaneMode,
    Custom,
}

impl OsdType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Volume => "Volume",
            Self::Mute => "Mute",
            Self::Brightness => "Brightness",
            Self::MediaPlay => "Play",
            Self::MediaPause => "Pause",
            Self::MediaNext => "Next Track",
            Self::MediaPrev => "Previous Track",
            Self::KeyboardBacklight => "Keyboard Backlight",
            Self::AirplaneMode => "Airplane Mode",
            Self::Custom => "Custom",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Volume => "🔊",
            Self::Mute => "🔇",
            Self::Brightness => "🔆",
            Self::MediaPlay => "▶",
            Self::MediaPause => "⏸",
            Self::MediaNext => "⏭",
            Self::MediaPrev => "⏮",
            Self::KeyboardBacklight => "⌨",
            Self::AirplaneMode => "✈",
            Self::Custom => "⚙",
        }
    }
}

/// OSD position on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdPosition {
    TopCenter,
    TopRight,
    Center,
    BottomCenter,
    BottomLeft,
}

impl OsdPosition {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopCenter => "Top Center",
            Self::TopRight => "Top Right",
            Self::Center => "Center",
            Self::BottomCenter => "Bottom Center",
            Self::BottomLeft => "Bottom Left",
        }
    }
}

/// An active OSD display.
#[derive(Debug, Clone)]
pub struct OsdDisplay {
    pub id: u32,
    pub osd_type: OsdType,
    /// Value (0-100 for volume/brightness, 0/1 for toggles).
    pub value: u32,
    /// Optional label text.
    pub label: String,
    /// Optional subtitle.
    pub subtitle: String,
    pub shown_ns: u64,
    /// Duration to show in milliseconds.
    pub duration_ms: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 100;

struct State {
    active: Option<OsdDisplay>,
    history: Vec<OsdDisplay>,
    next_id: u32,
    position: OsdPosition,
    default_duration_ms: u32,
    enabled: bool,
    total_shown: u64,
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
        active: None,
        history: Vec::new(),
        next_id: 1,
        position: OsdPosition::TopCenter,
        default_duration_ms: 2000,
        enabled: true,
        total_shown: 0,
        ops: 0,
    });
}

/// Show an OSD indicator.
pub fn show(osd_type: OsdType, value: u32, label: &str, subtitle: &str) -> KernelResult<u32> {
    with_state(|state| {
        if !state.enabled {
            return Err(KernelError::NotSupported);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_shown += 1;

        let osd = OsdDisplay {
            id,
            osd_type,
            value: value.min(100),
            label: String::from(label),
            subtitle: String::from(subtitle),
            shown_ns: crate::hpet::elapsed_ns(),
            duration_ms: state.default_duration_ms,
        };

        // Move previous active to history.
        if let Some(prev) = state.active.take() {
            if state.history.len() >= MAX_HISTORY {
                state.history.remove(0);
            }
            state.history.push(prev);
        }

        state.active = Some(osd);
        Ok(id)
    })
}

/// Show volume OSD (convenience).
pub fn show_volume(level: u32, muted: bool) -> KernelResult<u32> {
    let osd_type = if muted { OsdType::Mute } else { OsdType::Volume };
    let label = if muted { "Muted" } else { "" };
    show(osd_type, level, label, "")
}

/// Show brightness OSD (convenience).
pub fn show_brightness(level: u32) -> KernelResult<u32> {
    show(OsdType::Brightness, level, "", "")
}

/// Dismiss the active OSD.
pub fn dismiss() -> KernelResult<()> {
    with_state(|state| {
        if let Some(osd) = state.active.take() {
            if state.history.len() >= MAX_HISTORY {
                state.history.remove(0);
            }
            state.history.push(osd);
        }
        Ok(())
    })
}

/// Get active OSD (if any).
pub fn get_active() -> Option<OsdDisplay> {
    STATE.lock().as_ref().and_then(|s| s.active.clone())
}

/// Set OSD position.
pub fn set_position(position: OsdPosition) -> KernelResult<()> {
    with_state(|state| {
        state.position = position;
        Ok(())
    })
}

/// Get OSD position.
pub fn get_position() -> OsdPosition {
    STATE.lock().as_ref().map_or(OsdPosition::TopCenter, |s| s.position)
}

/// Set default display duration.
pub fn set_duration(ms: u32) -> KernelResult<()> {
    with_state(|state| {
        state.default_duration_ms = ms.clamp(500, 10000);
        Ok(())
    })
}

/// Enable/disable OSD.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        if !enabled {
            state.active = None;
        }
        Ok(())
    })
}

/// Recent OSD history.
pub fn list_history(count: usize) -> Vec<OsdDisplay> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.history.len().saturating_sub(count);
        s.history[start..].to_vec()
    })
}

/// Statistics: (total_shown, enabled, position_label, ops).
pub fn stats() -> (u64, bool, &'static str, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_shown, s.enabled, s.position.label(), s.ops),
        None => (0, false, "Unknown", 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("volumeosd::self_test() — running tests...");
    init_defaults();

    // 1: No active OSD.
    assert!(get_active().is_none());
    crate::serial_println!("  [1/10] initial empty: OK");

    // 2: Show volume.
    let id1 = show_volume(75, false).expect("vol");
    assert!(id1 > 0);
    let active = get_active().expect("active");
    assert_eq!(active.osd_type, OsdType::Volume);
    assert_eq!(active.value, 75);
    crate::serial_println!("  [2/10] show volume: OK");

    // 3: Show replaces previous.
    let id2 = show_volume(80, false).expect("vol2");
    assert!(id2 > id1);
    let active = get_active().expect("active2");
    assert_eq!(active.value, 80);
    crate::serial_println!("  [3/10] replace: OK");

    // 4: Mute.
    show_volume(0, true).expect("mute");
    let active = get_active().expect("active3");
    assert_eq!(active.osd_type, OsdType::Mute);
    crate::serial_println!("  [4/10] mute: OK");

    // 5: Brightness.
    show_brightness(50).expect("bright");
    let active = get_active().expect("active4");
    assert_eq!(active.osd_type, OsdType::Brightness);
    assert_eq!(active.value, 50);
    crate::serial_println!("  [5/10] brightness: OK");

    // 6: Custom OSD.
    show(OsdType::Custom, 0, "Screen Rotation", "Landscape").expect("custom");
    let active = get_active().expect("active5");
    assert_eq!(active.label, "Screen Rotation");
    crate::serial_println!("  [6/10] custom OSD: OK");

    // 7: Dismiss.
    dismiss().expect("dismiss");
    assert!(get_active().is_none());
    crate::serial_println!("  [7/10] dismiss: OK");

    // 8: Position.
    set_position(OsdPosition::BottomCenter).expect("pos");
    assert_eq!(get_position(), OsdPosition::BottomCenter);
    crate::serial_println!("  [8/10] position: OK");

    // 9: Disable.
    set_enabled(false).expect("disable");
    let result = show_volume(50, false);
    assert!(result.is_err());
    set_enabled(true).expect("enable");
    crate::serial_println!("  [9/10] enable/disable: OK");

    // 10: History and stats.
    let history = list_history(10);
    assert!(!history.is_empty());
    let (total, enabled, _, ops) = stats();
    assert!(total >= 5);
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] history & stats: OK");

    crate::serial_println!("volumeosd::self_test() — all 10 tests passed");
}
