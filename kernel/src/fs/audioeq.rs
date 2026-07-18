//! Audio Equalizer — frequency band adjustment and presets.
//!
//! Manages per-device audio equalizer settings with configurable
//! frequency bands, preset profiles, and custom user curves.
//!
//! ## Architecture
//!
//! ```text
//! Audio playback
//!   → audioeq::apply(device_id) → adjusts frequency bands
//!   → audioeq::set_preset(preset) → loads predefined curve
//!
//! Integration:
//!   → soundmixer (volume control)
//!   → audiodevice (output devices)
//!   → surroundsound (spatial audio)
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

/// Equalizer preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqPreset {
    Flat,
    Rock,
    Pop,
    Jazz,
    Classical,
    HipHop,
    Electronic,
    Vocal,
    Bass,
    Treble,
    Custom,
}

impl EqPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Rock => "Rock",
            Self::Pop => "Pop",
            Self::Jazz => "Jazz",
            Self::Classical => "Classical",
            Self::HipHop => "Hip-Hop",
            Self::Electronic => "Electronic",
            Self::Vocal => "Vocal",
            Self::Bass => "Bass Boost",
            Self::Treble => "Treble Boost",
            Self::Custom => "Custom",
        }
    }
}

/// A single frequency band.
#[derive(Debug, Clone)]
pub struct EqBand {
    /// Center frequency in Hz.
    pub freq_hz: u32,
    /// Gain in centibels (-1200 to +1200, 0 = flat).
    pub gain_cb: i32,
    /// Q factor * 100 (e.g., 141 = Q of 1.41).
    pub q_factor: u32,
}

/// Equalizer configuration for a device.
#[derive(Debug, Clone)]
pub struct EqConfig {
    pub id: u32,
    pub device_name: String,
    pub preset: EqPreset,
    pub bands: Vec<EqBand>,
    pub enabled: bool,
    pub preamp_cb: i32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONFIGS: usize = 16;
const DEFAULT_BANDS: &[(u32, i32)] = &[
    (32, 0), (64, 0), (125, 0), (250, 0), (500, 0),
    (1000, 0), (2000, 0), (4000, 0), (8000, 0), (16000, 0),
];

struct State {
    configs: Vec<EqConfig>,
    next_id: u32,
    total_adjustments: u64,
    total_preset_changes: u64,
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

fn make_bands(gains: &[(u32, i32)]) -> Vec<EqBand> {
    gains.iter().map(|&(freq, gain)| EqBand {
        freq_hz: freq, gain_cb: gain, q_factor: 141,
    }).collect()
}

fn preset_gains(preset: EqPreset) -> Vec<(u32, i32)> {
    match preset {
        EqPreset::Flat => DEFAULT_BANDS.to_vec(),
        EqPreset::Rock => alloc::vec![(32,300),(64,200),(125,100),(250,0),(500,-50),(1000,-50),(2000,100),(4000,200),(8000,300),(16000,300)],
        EqPreset::Pop => alloc::vec![(32,-100),(64,0),(125,100),(250,200),(500,200),(1000,100),(2000,0),(4000,-100),(8000,-100),(16000,-100)],
        EqPreset::Jazz => alloc::vec![(32,200),(64,100),(125,0),(250,100),(500,-100),(1000,-100),(2000,0),(4000,100),(8000,200),(16000,300)],
        EqPreset::Classical => alloc::vec![(32,200),(64,100),(125,0),(250,0),(500,0),(1000,0),(2000,0),(4000,100),(8000,200),(16000,200)],
        EqPreset::HipHop => alloc::vec![(32,400),(64,300),(125,200),(250,100),(500,0),(1000,0),(2000,0),(4000,100),(8000,200),(16000,100)],
        EqPreset::Electronic => alloc::vec![(32,300),(64,200),(125,0),(250,-100),(500,0),(1000,100),(2000,0),(4000,-100),(8000,200),(16000,400)],
        EqPreset::Vocal => alloc::vec![(32,-200),(64,-100),(125,0),(250,100),(500,200),(1000,300),(2000,200),(4000,100),(8000,0),(16000,-100)],
        EqPreset::Bass => alloc::vec![(32,500),(64,400),(125,300),(250,200),(500,100),(1000,0),(2000,0),(4000,0),(8000,0),(16000,0)],
        EqPreset::Treble => alloc::vec![(32,0),(64,0),(125,0),(250,0),(500,0),(1000,100),(2000,200),(4000,300),(8000,400),(16000,500)],
        EqPreset::Custom => DEFAULT_BANDS.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let config = EqConfig {
        id: 1, device_name: String::from("Built-in Audio"),
        preset: EqPreset::Flat,
        bands: make_bands(DEFAULT_BANDS),
        enabled: true, preamp_cb: 0,
    };

    *guard = Some(State {
        configs: alloc::vec![config],
        next_id: 2,
        total_adjustments: 0,
        total_preset_changes: 0,
        ops: 0,
    });
}

/// Create an EQ config for a device.
pub fn create_config(device_name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.configs.len() >= MAX_CONFIGS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.configs.push(EqConfig {
            id, device_name: String::from(device_name),
            preset: EqPreset::Flat,
            bands: make_bands(DEFAULT_BANDS),
            enabled: true, preamp_cb: 0,
        });
        Ok(id)
    })
}

/// Apply a preset to a config.
pub fn set_preset(config_id: u32, preset: EqPreset) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        let gains = preset_gains(preset);
        cfg.bands = make_bands(&gains);
        cfg.preset = preset;
        state.total_preset_changes += 1;
        Ok(())
    })
}

/// Adjust a single band's gain.
pub fn set_band_gain(config_id: u32, band_index: usize, gain_cb: i32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        let band = cfg.bands.get_mut(band_index)
            .ok_or(KernelError::InvalidArgument)?;
        band.gain_cb = gain_cb.clamp(-1200, 1200);
        cfg.preset = EqPreset::Custom;
        state.total_adjustments += 1;
        Ok(())
    })
}

/// Set preamp gain.
pub fn set_preamp(config_id: u32, preamp_cb: i32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.preamp_cb = preamp_cb.clamp(-1200, 1200);
        state.total_adjustments += 1;
        Ok(())
    })
}

/// Enable/disable equalizer.
pub fn set_enabled(config_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.enabled = enabled;
        Ok(())
    })
}

/// Remove a config.
pub fn remove_config(config_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.configs.iter().position(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        state.configs.remove(pos);
        Ok(())
    })
}

/// List all configs.
pub fn list_configs() -> Vec<EqConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.configs.clone())
}

/// Get a config.
pub fn get_config(id: u32) -> KernelResult<EqConfig> {
    with_state(|state| {
        state.configs.iter().find(|c| c.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (config_count, total_adjustments, total_preset_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.configs.len(), s.total_adjustments, s.total_preset_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("audioeq::self_test() — running tests...");
    init_defaults();

    // 1: Default config (flat).
    let configs = list_configs();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].preset, EqPreset::Flat);
    assert_eq!(configs[0].bands.len(), 10);
    assert!(configs[0].enabled);
    crate::serial_println!("  [1/8] default flat: OK");

    // 2: Set preset.
    set_preset(1, EqPreset::Rock).expect("preset");
    let cfg = get_config(1).expect("get");
    assert_eq!(cfg.preset, EqPreset::Rock);
    assert_eq!(cfg.bands[0].gain_cb, 300); // 32 Hz boosted
    crate::serial_println!("  [2/8] rock preset: OK");

    // 3: Adjust band → becomes Custom.
    set_band_gain(1, 0, -200).expect("band");
    let cfg = get_config(1).expect("get2");
    assert_eq!(cfg.preset, EqPreset::Custom);
    assert_eq!(cfg.bands[0].gain_cb, -200);
    crate::serial_println!("  [3/8] custom band: OK");

    // 4: Preamp.
    set_preamp(1, -300).expect("preamp");
    let cfg = get_config(1).expect("get3");
    assert_eq!(cfg.preamp_cb, -300);
    crate::serial_println!("  [4/8] preamp: OK");

    // 5: Clamp out-of-range.
    set_band_gain(1, 5, 2000).expect("clamp");
    let cfg = get_config(1).expect("get4");
    assert_eq!(cfg.bands[5].gain_cb, 1200); // clamped
    crate::serial_println!("  [5/8] clamp gain: OK");

    // 6: Disable/enable.
    set_enabled(1, false).expect("disable");
    let cfg = get_config(1).expect("get5");
    assert!(!cfg.enabled);
    set_enabled(1, true).expect("enable");
    crate::serial_println!("  [6/8] enable/disable: OK");

    // 7: Create and remove.
    let id2 = create_config("Headphones").expect("create");
    assert_eq!(list_configs().len(), 2);
    remove_config(id2).expect("remove");
    assert_eq!(list_configs().len(), 1);
    crate::serial_println!("  [7/8] create/remove: OK");

    // 8: Stats.
    let (count, adj, presets, ops) = stats();
    assert_eq!(count, 1);
    assert!(adj >= 3);
    assert!(presets >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("audioeq::self_test() — all 8 tests passed");
}
