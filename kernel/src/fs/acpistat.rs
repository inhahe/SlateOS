//! ACPI Event Statistics — ACPI event monitoring.
//!
//! Tracks ACPI events: power button, lid switch, thermal
//! notifications, battery updates, and GPE (General Purpose
//! Event) handling. Essential for power management diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! ACPI event monitoring
//!   → acpistat::record_event(type) → ACPI event fired
//!   → acpistat::record_gpe(gpe_num) → GPE fired
//!   → acpistat::set_s_state(state) → system sleep state
//!   → acpistat::event_counts() → per-type event counts
//!
//! Integration:
//!   → powerstat (power domains)
//!   → thermal (thermal zones)
//!   → cputhr (CPU throttle)
//!   → clocksrc (clock sources)
//! ```

use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// ACPI event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiEvent {
    PowerButton,
    SleepButton,
    LidOpen,
    LidClose,
    AcConnect,
    AcDisconnect,
    BatteryUpdate,
    ThermalTrip,
}

impl AcpiEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerButton => "power_btn",
            Self::SleepButton => "sleep_btn",
            Self::LidOpen => "lid_open",
            Self::LidClose => "lid_close",
            Self::AcConnect => "ac_connect",
            Self::AcDisconnect => "ac_disconnect",
            Self::BatteryUpdate => "battery",
            Self::ThermalTrip => "thermal",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::PowerButton => 0,
            Self::SleepButton => 1,
            Self::LidOpen => 2,
            Self::LidClose => 3,
            Self::AcConnect => 4,
            Self::AcDisconnect => 5,
            Self::BatteryUpdate => 6,
            Self::ThermalTrip => 7,
        }
    }
}

const NUM_EVENT_TYPES: usize = 8;

/// System sleep state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SState {
    S0,  // Working
    S1,  // Power on suspend
    S3,  // Suspend to RAM
    S4,  // Suspend to disk (hibernate)
    S5,  // Soft off
}

impl SState {
    pub fn label(self) -> &'static str {
        match self {
            Self::S0 => "S0 (working)",
            Self::S1 => "S1 (standby)",
            Self::S3 => "S3 (suspend)",
            Self::S4 => "S4 (hibernate)",
            Self::S5 => "S5 (off)",
        }
    }
}

/// GPE (General Purpose Event) stats.
#[derive(Debug, Clone)]
pub struct GpeStats {
    pub gpe_num: u32,
    pub count: u64,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GPES: usize = 128;

struct State {
    event_counts: [u64; NUM_EVENT_TYPES],
    gpes: Vec<GpeStats>,
    current_state: SState,
    suspend_count: u64,
    resume_count: u64,
    total_events: u64,
    total_gpes: u64,
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
        event_counts: [10, 5, 50, 50, 100, 95, 5000, 200],
        gpes: alloc::vec![
            GpeStats { gpe_num: 0x11, count: 500_000, enabled: true },
            GpeStats { gpe_num: 0x16, count: 100_000, enabled: true },
            GpeStats { gpe_num: 0x1b, count: 50_000, enabled: true },
            GpeStats { gpe_num: 0x6e, count: 10_000, enabled: false },
        ],
        current_state: SState::S0,
        suspend_count: 55,
        resume_count: 55,
        total_events: 5510,
        total_gpes: 660_000,
        ops: 0,
    });
}

/// Record an ACPI event.
pub fn record_event(event: AcpiEvent) -> KernelResult<()> {
    with_state(|state| {
        state.event_counts[event.index()] += 1;
        state.total_events += 1;
        Ok(())
    })
}

/// Record a GPE firing.
pub fn record_gpe(gpe_num: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(g) = state.gpes.iter_mut().find(|g| g.gpe_num == gpe_num) {
            g.count += 1;
        } else {
            if state.gpes.len() >= MAX_GPES { return Err(KernelError::ResourceExhausted); }
            state.gpes.push(GpeStats { gpe_num, count: 1, enabled: true });
        }
        state.total_gpes += 1;
        Ok(())
    })
}

/// Set system sleep state.
pub fn set_s_state(new_state: SState) -> KernelResult<()> {
    with_state(|state| {
        if new_state != SState::S0 && state.current_state == SState::S0 {
            state.suspend_count += 1;
        } else if new_state == SState::S0 && state.current_state != SState::S0 {
            state.resume_count += 1;
        }
        state.current_state = new_state;
        Ok(())
    })
}

/// Event counts per type.
pub fn event_counts() -> [(AcpiEvent, u64); NUM_EVENT_TYPES] {
    let guard = STATE.lock();
    let counts = guard.as_ref().map_or([0u64; NUM_EVENT_TYPES], |s| s.event_counts);
    [
        (AcpiEvent::PowerButton, counts[0]),
        (AcpiEvent::SleepButton, counts[1]),
        (AcpiEvent::LidOpen, counts[2]),
        (AcpiEvent::LidClose, counts[3]),
        (AcpiEvent::AcConnect, counts[4]),
        (AcpiEvent::AcDisconnect, counts[5]),
        (AcpiEvent::BatteryUpdate, counts[6]),
        (AcpiEvent::ThermalTrip, counts[7]),
    ]
}

/// GPE list.
pub fn gpe_list() -> Vec<GpeStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.gpes.clone())
}

/// Statistics: (total_events, total_gpes, suspend_count, resume_count, ops).
pub fn stats() -> (u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_events, s.total_gpes, s.suspend_count, s.resume_count, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("acpistat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(gpe_list().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Event.
    let counts_before = event_counts()[0].1;
    record_event(AcpiEvent::PowerButton).expect("event");
    let counts_after = event_counts()[0].1;
    assert_eq!(counts_after, counts_before + 1);
    crate::serial_println!("  [2/8] event: OK");

    // 3: GPE (existing).
    let before = gpe_list().iter().find(|g| g.gpe_num == 0x11).cloned().unwrap().count;
    record_gpe(0x11).expect("gpe_exist");
    let after = gpe_list().iter().find(|g| g.gpe_num == 0x11).cloned().unwrap().count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] gpe existing: OK");

    // 4: GPE (new).
    record_gpe(0xFF).expect("gpe_new");
    assert_eq!(gpe_list().len(), 5);
    crate::serial_println!("  [4/8] gpe new: OK");

    // 5: S-state suspend.
    let (_, _, sus_before, _, _) = stats();
    set_s_state(SState::S3).expect("suspend");
    let (_, _, sus_after, _, _) = stats();
    assert_eq!(sus_after, sus_before + 1);
    crate::serial_println!("  [5/8] suspend: OK");

    // 6: S-state resume.
    let (_, _, _, res_before, _) = stats();
    set_s_state(SState::S0).expect("resume");
    let (_, _, _, res_after, _) = stats();
    assert_eq!(res_after, res_before + 1);
    crate::serial_println!("  [6/8] resume: OK");

    // 7: Event counts array.
    let counts = event_counts();
    assert_eq!(counts.len(), 8);
    assert!(counts[6].1 >= 5000); // Battery
    crate::serial_println!("  [7/8] event counts: OK");

    // 8: Stats.
    let (events, gpes, suspends, resumes, ops) = stats();
    assert!(events > 5510);
    assert!(gpes > 660_000);
    assert!(suspends > 55);
    assert!(resumes > 55);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("acpistat::self_test() — all 8 tests passed");
}
