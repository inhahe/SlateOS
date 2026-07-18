//! Surround Sound — spatial audio and multi-channel configuration.
//!
//! Manages surround sound speaker layouts, channel mapping,
//! spatial audio processing, and speaker calibration.
//!
//! ## Architecture
//!
//! ```text
//! Audio device connected
//!   → surroundsound::detect_layout(device_id) → channel count
//!   → surroundsound::configure(layout) → speaker assignments
//!
//! Playback
//!   → surroundsound::upmix(stereo_data) → multi-channel
//!   → surroundsound::set_virtual_surround(enabled)
//!
//! Integration:
//!   → audiodevice (audio output devices)
//!   → soundmixer (volume per channel)
//!   → audiomux (routing)
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

/// Speaker layout configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerLayout {
    Mono,
    Stereo,
    Surround21,
    Surround51,
    Surround71,
    Atmos714,
}

impl SpeakerLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono (1.0)",
            Self::Stereo => "Stereo (2.0)",
            Self::Surround21 => "2.1",
            Self::Surround51 => "5.1 Surround",
            Self::Surround71 => "7.1 Surround",
            Self::Atmos714 => "7.1.4 Atmos",
        }
    }

    pub fn channel_count(self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Surround21 => 3,
            Self::Surround51 => 6,
            Self::Surround71 => 8,
            Self::Atmos714 => 12,
        }
    }
}

/// Individual speaker channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerChannel {
    FrontLeft,
    FrontRight,
    Center,
    Subwoofer,
    RearLeft,
    RearRight,
    SideLeft,
    SideRight,
    TopFrontLeft,
    TopFrontRight,
    TopRearLeft,
    TopRearRight,
}

impl SpeakerChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::FrontLeft => "Front Left",
            Self::FrontRight => "Front Right",
            Self::Center => "Center",
            Self::Subwoofer => "Subwoofer",
            Self::RearLeft => "Rear Left",
            Self::RearRight => "Rear Right",
            Self::SideLeft => "Side Left",
            Self::SideRight => "Side Right",
            Self::TopFrontLeft => "Top Front Left",
            Self::TopFrontRight => "Top Front Right",
            Self::TopRearLeft => "Top Rear Left",
            Self::TopRearRight => "Top Rear Right",
        }
    }
}

/// Speaker calibration entry.
#[derive(Debug, Clone)]
pub struct SpeakerCalibration {
    pub channel: SpeakerChannel,
    /// Volume trim in centibels (-600 to +600, 0 = no adjustment).
    pub trim_cb: i32,
    /// Distance from listener in centimeters.
    pub distance_cm: u32,
    /// Whether this channel is active.
    pub active: bool,
}

/// Surround configuration for a device.
#[derive(Debug, Clone)]
pub struct SurroundConfig {
    pub id: u32,
    pub device_name: String,
    pub layout: SpeakerLayout,
    pub speakers: Vec<SpeakerCalibration>,
    pub virtual_surround: bool,
    pub crossover_hz: u32,
    pub lfe_enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONFIGS: usize = 10;

struct State {
    configs: Vec<SurroundConfig>,
    next_id: u32,
    default_layout: SpeakerLayout,
    total_configs: u64,
    total_calibrations: u64,
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

fn speakers_for_layout(layout: SpeakerLayout) -> Vec<SpeakerCalibration> {
    let channels = match layout {
        SpeakerLayout::Mono => alloc::vec![SpeakerChannel::Center],
        SpeakerLayout::Stereo => alloc::vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight],
        SpeakerLayout::Surround21 => alloc::vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight, SpeakerChannel::Subwoofer],
        SpeakerLayout::Surround51 => alloc::vec![
            SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight, SpeakerChannel::Center,
            SpeakerChannel::Subwoofer, SpeakerChannel::RearLeft, SpeakerChannel::RearRight],
        SpeakerLayout::Surround71 => alloc::vec![
            SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight, SpeakerChannel::Center,
            SpeakerChannel::Subwoofer, SpeakerChannel::SideLeft, SpeakerChannel::SideRight,
            SpeakerChannel::RearLeft, SpeakerChannel::RearRight],
        SpeakerLayout::Atmos714 => alloc::vec![
            SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight, SpeakerChannel::Center,
            SpeakerChannel::Subwoofer, SpeakerChannel::SideLeft, SpeakerChannel::SideRight,
            SpeakerChannel::RearLeft, SpeakerChannel::RearRight,
            SpeakerChannel::TopFrontLeft, SpeakerChannel::TopFrontRight,
            SpeakerChannel::TopRearLeft, SpeakerChannel::TopRearRight],
    };
    channels.into_iter().map(|ch| SpeakerCalibration {
        channel: ch, trim_cb: 0, distance_cm: 200, active: true,
    }).collect()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let default_config = SurroundConfig {
        id: 1, device_name: String::from("Built-in Audio"),
        layout: SpeakerLayout::Stereo,
        speakers: speakers_for_layout(SpeakerLayout::Stereo),
        virtual_surround: false, crossover_hz: 80, lfe_enabled: false,
    };

    *guard = Some(State {
        configs: alloc::vec![default_config],
        next_id: 2,
        default_layout: SpeakerLayout::Stereo,
        total_configs: 1,
        total_calibrations: 0,
        ops: 0,
    });
}

/// Create surround config for a device.
pub fn create_config(device_name: &str, layout: SpeakerLayout) -> KernelResult<u32> {
    with_state(|state| {
        if state.configs.len() >= MAX_CONFIGS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_configs += 1;
        state.configs.push(SurroundConfig {
            id, device_name: String::from(device_name),
            layout, speakers: speakers_for_layout(layout),
            virtual_surround: false, crossover_hz: 80,
            lfe_enabled: layout.channel_count() > 2,
        });
        Ok(id)
    })
}

/// Change layout for a config (recalculates speakers).
pub fn set_layout(config_id: u32, layout: SpeakerLayout) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.layout = layout;
        cfg.speakers = speakers_for_layout(layout);
        cfg.lfe_enabled = layout.channel_count() > 2;
        Ok(())
    })
}

/// Calibrate a speaker channel.
pub fn calibrate_speaker(config_id: u32, channel: SpeakerChannel, trim_cb: i32, distance_cm: u32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        let speaker = cfg.speakers.iter_mut().find(|s| s.channel == channel)
            .ok_or(KernelError::NotFound)?;
        speaker.trim_cb = trim_cb.clamp(-600, 600);
        speaker.distance_cm = distance_cm;
        state.total_calibrations += 1;
        Ok(())
    })
}

/// Enable/disable virtual surround (headphone surround).
pub fn set_virtual_surround(config_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.virtual_surround = enabled;
        Ok(())
    })
}

/// Set crossover frequency for subwoofer.
pub fn set_crossover(config_id: u32, hz: u32) -> KernelResult<()> {
    with_state(|state| {
        let cfg = state.configs.iter_mut().find(|c| c.id == config_id)
            .ok_or(KernelError::NotFound)?;
        cfg.crossover_hz = hz.clamp(40, 200);
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
pub fn list_configs() -> Vec<SurroundConfig> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.configs.clone())
}

/// Get a config.
pub fn get_config(id: u32) -> KernelResult<SurroundConfig> {
    with_state(|state| {
        state.configs.iter().find(|c| c.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (config_count, total_configs, total_calibrations, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.configs.len(), s.total_configs, s.total_calibrations, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("surroundsound::self_test() — running tests...");
    init_defaults();

    // 1: Default config (stereo).
    let configs = list_configs();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].layout, SpeakerLayout::Stereo);
    assert_eq!(configs[0].speakers.len(), 2);
    crate::serial_println!("  [1/8] default stereo: OK");

    // 2: Create 5.1 config.
    let id = create_config("Surround System", SpeakerLayout::Surround51).expect("create");
    let cfg = get_config(id).expect("get");
    assert_eq!(cfg.speakers.len(), 6);
    assert!(cfg.lfe_enabled);
    crate::serial_println!("  [2/8] create 5.1: OK");

    // 3: Change to 7.1.
    set_layout(id, SpeakerLayout::Surround71).expect("layout");
    let cfg = get_config(id).expect("get2");
    assert_eq!(cfg.speakers.len(), 8);
    crate::serial_println!("  [3/8] change to 7.1: OK");

    // 4: Calibrate speaker.
    calibrate_speaker(id, SpeakerChannel::Center, -100, 250).expect("cal");
    let cfg = get_config(id).expect("get3");
    let center = cfg.speakers.iter().find(|s| s.channel == SpeakerChannel::Center).expect("center");
    assert_eq!(center.trim_cb, -100);
    assert_eq!(center.distance_cm, 250);
    crate::serial_println!("  [4/8] calibrate: OK");

    // 5: Virtual surround.
    set_virtual_surround(id, true).expect("vs");
    let cfg = get_config(id).expect("get4");
    assert!(cfg.virtual_surround);
    crate::serial_println!("  [5/8] virtual surround: OK");

    // 6: Crossover.
    set_crossover(id, 120).expect("xo");
    let cfg = get_config(id).expect("get5");
    assert_eq!(cfg.crossover_hz, 120);
    crate::serial_println!("  [6/8] crossover: OK");

    // 7: Remove config.
    remove_config(id).expect("remove");
    assert_eq!(list_configs().len(), 1);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (count, total, cals, ops) = stats();
    assert_eq!(count, 1);
    assert!(total >= 2);
    assert_eq!(cals, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("surroundsound::self_test() — all 8 tests passed");
}
