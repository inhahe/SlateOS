//! Spatial Audio — 3D audio positioning and virtual speaker layouts.
//!
//! Provides spatial audio rendering with virtual speaker configurations,
//! head tracking support, and per-app spatialization settings.
//!
//! ## Architecture
//!
//! ```text
//! Audio output
//!   → spatialaudio::spatialize(stream, position) → processed audio
//!
//! Configuration
//!   → spatialaudio::set_layout(layout)
//!   → spatialaudio::set_headtracking(enabled)
//!   → spatialaudio::set_app_enabled(app, enabled)
//!
//! Integration:
//!   → soundmixer (audio routing)
//!   → audiomux (multi-output)
//!   → surroundsound (surround config)
//!   → audiodevice (output device)
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

/// Virtual speaker layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerLayout {
    /// Standard stereo.
    Stereo,
    /// 5.1 surround.
    Surround51,
    /// 7.1 surround.
    Surround71,
    /// Dolby Atmos-style height channels.
    Atmos,
    /// Binaural (headphone-optimized).
    Binaural,
    /// Custom layout.
    Custom,
}

impl SpeakerLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Surround51 => "5.1 Surround",
            Self::Surround71 => "7.1 Surround",
            Self::Atmos => "Atmos",
            Self::Binaural => "Binaural",
            Self::Custom => "Custom",
        }
    }

    pub fn channel_count(self) -> u32 {
        match self {
            Self::Stereo => 2,
            Self::Surround51 => 6,
            Self::Surround71 => 8,
            Self::Atmos => 12,
            Self::Binaural => 2,
            Self::Custom => 0,
        }
    }
}

/// Room size simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomSize {
    None,
    Small,
    Medium,
    Large,
    Hall,
    Arena,
}

impl RoomSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Small => "Small Room",
            Self::Medium => "Medium Room",
            Self::Large => "Large Room",
            Self::Hall => "Concert Hall",
            Self::Arena => "Arena",
        }
    }
}

/// Per-app spatial audio config.
#[derive(Debug, Clone)]
pub struct AppSpatialConfig {
    pub app_name: String,
    pub enabled: bool,
    pub layout_override: Option<SpeakerLayout>,
    pub room_override: Option<RoomSize>,
}

/// Spatial audio configuration.
#[derive(Debug, Clone)]
pub struct SpatialConfig {
    pub global_enabled: bool,
    pub layout: SpeakerLayout,
    pub room_size: RoomSize,
    pub head_tracking: bool,
    pub reverb_level: u32,
    pub distance_attenuation: bool,
    pub doppler_effect: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_APP_CONFIGS: usize = 100;

struct State {
    config: SpatialConfig,
    app_configs: Vec<AppSpatialConfig>,
    total_streams_processed: u64,
    total_config_changes: u64,
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
        config: SpatialConfig {
            global_enabled: false,
            layout: SpeakerLayout::Stereo,
            room_size: RoomSize::Medium,
            head_tracking: false,
            reverb_level: 30,
            distance_attenuation: true,
            doppler_effect: false,
        },
        app_configs: Vec::new(),
        total_streams_processed: 0,
        total_config_changes: 0,
        ops: 0,
    });
}

/// Enable/disable spatial audio globally.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.global_enabled = enabled;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Set speaker layout.
pub fn set_layout(layout: SpeakerLayout) -> KernelResult<()> {
    with_state(|state| {
        state.config.layout = layout;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Set room size.
pub fn set_room_size(size: RoomSize) -> KernelResult<()> {
    with_state(|state| {
        state.config.room_size = size;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Enable/disable head tracking.
pub fn set_head_tracking(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.head_tracking = enabled;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Set reverb level (0-100).
pub fn set_reverb(level: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.reverb_level = level.min(100);
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Enable/disable distance attenuation.
pub fn set_distance_attenuation(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.distance_attenuation = enabled;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Enable/disable Doppler effect.
pub fn set_doppler(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.doppler_effect = enabled;
        state.total_config_changes += 1;
        Ok(())
    })
}

/// Set per-app spatial audio config.
pub fn set_app_enabled(app_name: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        if let Some(cfg) = state.app_configs.iter_mut().find(|c| c.app_name == app_name) {
            cfg.enabled = enabled;
        } else {
            if state.app_configs.len() >= MAX_APP_CONFIGS {
                return Err(KernelError::ResourceExhausted);
            }
            state.app_configs.push(AppSpatialConfig {
                app_name: String::from(app_name),
                enabled,
                layout_override: None,
                room_override: None,
            });
        }
        Ok(())
    })
}

/// Set per-app layout override.
pub fn set_app_layout(app_name: &str, layout: SpeakerLayout) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.app_configs.iter_mut().find(|c| c.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        cfg.layout_override = Some(layout);
        Ok(())
    })
}

/// Record a stream being processed with spatial audio.
pub fn record_stream() -> KernelResult<()> {
    with_state(|state| {
        state.total_streams_processed += 1;
        Ok(())
    })
}

/// Get current spatial config.
pub fn get_config() -> Option<SpatialConfig> {
    STATE.lock().as_ref().map(|s| s.config.clone())
}

/// Get effective config for an app (considering overrides).
pub fn get_app_config(app_name: &str) -> Option<SpatialConfig> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    let mut cfg = state.config.clone();
    if let Some(app) = state.app_configs.iter().find(|c| c.app_name == app_name) {
        if !app.enabled {
            cfg.global_enabled = false;
        }
        if let Some(layout) = app.layout_override {
            cfg.layout = layout;
        }
        if let Some(room) = app.room_override {
            cfg.room_size = room;
        }
    }
    Some(cfg)
}

/// List per-app configs.
pub fn list_app_configs() -> Vec<AppSpatialConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.app_configs.clone())
}

/// Statistics: (app_config_count, streams_processed, config_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.app_configs.len(), s.total_streams_processed, s.total_config_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("spatialaudio::self_test() — running tests...");
    init_defaults();

    // 1: Default config.
    let cfg = get_config().expect("config");
    assert!(!cfg.global_enabled);
    assert_eq!(cfg.layout, SpeakerLayout::Stereo);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enable and set layout.
    set_enabled(true).expect("enable");
    set_layout(SpeakerLayout::Surround71).expect("layout");
    let cfg = get_config().expect("config2");
    assert!(cfg.global_enabled);
    assert_eq!(cfg.layout, SpeakerLayout::Surround71);
    assert_eq!(cfg.layout.channel_count(), 8);
    crate::serial_println!("  [2/8] enable/layout: OK");

    // 3: Room size.
    set_room_size(RoomSize::Hall).expect("room");
    let cfg = get_config().expect("config3");
    assert_eq!(cfg.room_size, RoomSize::Hall);
    crate::serial_println!("  [3/8] room size: OK");

    // 4: Head tracking and effects.
    set_head_tracking(true).expect("ht");
    set_reverb(60).expect("reverb");
    set_doppler(true).expect("doppler");
    let cfg = get_config().expect("config4");
    assert!(cfg.head_tracking);
    assert_eq!(cfg.reverb_level, 60);
    assert!(cfg.doppler_effect);
    crate::serial_println!("  [4/8] effects: OK");

    // 5: Per-app config.
    set_app_enabled("game", true).expect("app_enable");
    set_app_layout("game", SpeakerLayout::Atmos).expect("app_layout");
    let app_cfg = get_app_config("game").expect("app_config");
    assert_eq!(app_cfg.layout, SpeakerLayout::Atmos);
    crate::serial_println!("  [5/8] per-app: OK");

    // 6: App disabled overrides global.
    set_app_enabled("music", false).expect("app_disable");
    let app_cfg = get_app_config("music").expect("app_config2");
    assert!(!app_cfg.global_enabled);
    crate::serial_println!("  [6/8] app disable: OK");

    // 7: Record stream.
    record_stream().expect("stream");
    record_stream().expect("stream2");
    crate::serial_println!("  [7/8] streams: OK");

    // 8: Stats.
    let (apps, streams, changes, ops) = stats();
    assert_eq!(apps, 2);
    assert_eq!(streams, 2);
    assert!(changes >= 6);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("spatialaudio::self_test() — all 8 tests passed");
}
