//! HDR Display — High Dynamic Range display management.
//!
//! Manages HDR capability detection, HDR mode switching, tone mapping
//! configuration, and per-display HDR settings.
//!
//! ## Architecture
//!
//! ```text
//! Display connected
//!   → hdrdisplay::detect_hdr(display_id) → capabilities
//!
//! User enables HDR
//!   → hdrdisplay::enable(display_id) → switches to HDR mode
//!   → hdrdisplay::set_peak_brightness(nits)
//!
//! Integration:
//!   → display (display management)
//!   → displaycolor (color calibration)
//!   → brightness (backlight interaction)
//!   → monitors (multi-display)
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

/// HDR standard supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrStandard {
    Hdr10,
    Hdr10Plus,
    DolbyVision,
    Hlg,
    None,
}

impl HdrStandard {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdr10 => "HDR10",
            Self::Hdr10Plus => "HDR10+",
            Self::DolbyVision => "Dolby Vision",
            Self::Hlg => "HLG",
            Self::None => "None (SDR)",
        }
    }
}

/// Tone mapping algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneMapping {
    /// System default tone mapping.
    Auto,
    /// ACES filmic curve.
    AcesFilmic,
    /// Reinhard simple.
    Reinhard,
    /// Hable/Uncharted 2.
    Hable,
    /// No tone mapping (passthrough).
    None,
}

impl ToneMapping {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::AcesFilmic => "ACES Filmic",
            Self::Reinhard => "Reinhard",
            Self::Hable => "Hable",
            Self::None => "None (Passthrough)",
        }
    }
}

/// Color space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    Bt2020,
    DciP3,
    AdobeRgb,
}

impl ColorSpace {
    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::Bt2020 => "BT.2020",
            Self::DciP3 => "DCI-P3",
            Self::AdobeRgb => "Adobe RGB",
        }
    }
}

/// HDR display configuration.
#[derive(Debug, Clone)]
pub struct HdrDisplay {
    pub id: u32,
    pub name: String,
    pub supported_standards: Vec<HdrStandard>,
    pub active_standard: HdrStandard,
    pub hdr_enabled: bool,
    /// Peak brightness in nits.
    pub peak_nits: u32,
    /// Max content light level in nits.
    pub max_cll: u32,
    /// Max frame-average light level in nits.
    pub max_fall: u32,
    pub color_space: ColorSpace,
    pub tone_mapping: ToneMapping,
    /// Color depth in bits (8, 10, 12).
    pub bit_depth: u8,
    /// SDR content brightness boost (0-100).
    pub sdr_boost: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DISPLAYS: usize = 8;

struct State {
    displays: Vec<HdrDisplay>,
    next_id: u32,
    total_switches: u64,
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

    let display = HdrDisplay {
        id: 1, name: String::from("Primary Display"),
        supported_standards: alloc::vec![HdrStandard::Hdr10, HdrStandard::Hlg],
        active_standard: HdrStandard::None,
        hdr_enabled: false,
        peak_nits: 400, max_cll: 400, max_fall: 200,
        color_space: ColorSpace::Srgb,
        tone_mapping: ToneMapping::Auto,
        bit_depth: 8, sdr_boost: 50,
    };

    *guard = Some(State {
        displays: alloc::vec![display],
        next_id: 2,
        total_switches: 0,
        ops: 0,
    });
}

/// Register HDR display.
pub fn register_display(name: &str, peak_nits: u32, standards: Vec<HdrStandard>) -> KernelResult<u32> {
    with_state(|state| {
        if state.displays.len() >= MAX_DISPLAYS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.displays.push(HdrDisplay {
            id, name: String::from(name),
            supported_standards: standards,
            active_standard: HdrStandard::None,
            hdr_enabled: false,
            peak_nits, max_cll: peak_nits, max_fall: peak_nits / 2,
            color_space: ColorSpace::Srgb,
            tone_mapping: ToneMapping::Auto,
            bit_depth: 10, sdr_boost: 50,
        });
        Ok(id)
    })
}

/// Enable HDR on a display.
pub fn enable_hdr(display_id: u32, standard: HdrStandard) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        if !d.supported_standards.contains(&standard) {
            return Err(KernelError::InvalidArgument);
        }
        d.hdr_enabled = true;
        d.active_standard = standard;
        d.color_space = ColorSpace::Bt2020;
        d.bit_depth = 10;
        state.total_switches += 1;
        Ok(())
    })
}

/// Disable HDR on a display.
pub fn disable_hdr(display_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.hdr_enabled = false;
        d.active_standard = HdrStandard::None;
        d.color_space = ColorSpace::Srgb;
        d.bit_depth = 8;
        state.total_switches += 1;
        Ok(())
    })
}

/// Set tone mapping algorithm.
pub fn set_tone_mapping(display_id: u32, tm: ToneMapping) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.tone_mapping = tm;
        Ok(())
    })
}

/// Set SDR content brightness boost (0-100).
pub fn set_sdr_boost(display_id: u32, boost: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.sdr_boost = boost.min(100);
        Ok(())
    })
}

/// Set color space.
pub fn set_color_space(display_id: u32, cs: ColorSpace) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.color_space = cs;
        Ok(())
    })
}

/// List all displays.
pub fn list_displays() -> Vec<HdrDisplay> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.displays.clone())
}

/// Get HDR capabilities for a display.
pub fn get_display(id: u32) -> KernelResult<HdrDisplay> {
    with_state(|state| {
        state.displays.iter().find(|d| d.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (display_count, hdr_enabled_count, total_switches, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let enabled = s.displays.iter().filter(|d| d.hdr_enabled).count();
            (s.displays.len(), enabled, s.total_switches, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("hdrdisplay::self_test() — running tests...");
    init_defaults();

    // 1: Default display (SDR).
    let displays = list_displays();
    assert_eq!(displays.len(), 1);
    assert!(!displays[0].hdr_enabled);
    crate::serial_println!("  [1/8] default SDR: OK");

    // 2: Enable HDR.
    enable_hdr(1, HdrStandard::Hdr10).expect("enable");
    let d = get_display(1).expect("get");
    assert!(d.hdr_enabled);
    assert_eq!(d.active_standard, HdrStandard::Hdr10);
    assert_eq!(d.color_space, ColorSpace::Bt2020);
    crate::serial_println!("  [2/8] enable HDR: OK");

    // 3: Unsupported standard rejected.
    let result = enable_hdr(1, HdrStandard::DolbyVision);
    assert!(result.is_err());
    crate::serial_println!("  [3/8] unsupported rejected: OK");

    // 4: Tone mapping.
    set_tone_mapping(1, ToneMapping::AcesFilmic).expect("tm");
    let d = get_display(1).expect("get2");
    assert_eq!(d.tone_mapping, ToneMapping::AcesFilmic);
    crate::serial_println!("  [4/8] tone mapping: OK");

    // 5: SDR boost.
    set_sdr_boost(1, 75).expect("boost");
    let d = get_display(1).expect("get3");
    assert_eq!(d.sdr_boost, 75);
    crate::serial_println!("  [5/8] SDR boost: OK");

    // 6: Disable HDR.
    disable_hdr(1).expect("disable");
    let d = get_display(1).expect("get4");
    assert!(!d.hdr_enabled);
    assert_eq!(d.color_space, ColorSpace::Srgb);
    crate::serial_println!("  [6/8] disable HDR: OK");

    // 7: Register HDR monitor.
    let d2 = register_display("4K HDR Monitor", 1000,
        alloc::vec![HdrStandard::Hdr10, HdrStandard::Hdr10Plus, HdrStandard::DolbyVision]).expect("reg");
    enable_hdr(d2, HdrStandard::DolbyVision).expect("enable_dv");
    let d = get_display(d2).expect("get5");
    assert_eq!(d.active_standard, HdrStandard::DolbyVision);
    crate::serial_println!("  [7/8] HDR monitor: OK");

    // 8: Stats.
    let (count, enabled, switches, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(enabled, 1);
    assert!(switches >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("hdrdisplay::self_test() — all 8 tests passed");
}
