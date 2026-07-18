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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** ACPI event statistics table.
///
/// Seeds NO event counts, NO GPE rows, and zero suspend/resume/total
/// counters.  The system starts in `S0` (the genuine power-on working state —
/// that is a real initial condition, not fabricated observed data).  Real ACPI
/// accounting is wired through [`register_gpe`] (one row per enumerated GPE,
/// zeroed, with its real enabled/disabled state), [`record_event`],
/// [`record_gpe`], and [`set_s_state`]; until those are called the table is
/// genuinely empty, so the `/proc/acpistat` file and the `acpistat` kshell
/// command report zeros rather than fabricated numbers — the kernel's hard
/// "never invent data in procfs" rule.
///
/// NOTE: this previously seeded fictional event counts ([10, 5, 50, 50, 100,
/// 95, 5000, 200] across power/sleep/lid/AC/battery/thermal), four fictional
/// GPEs (0x11 count 500000, 0x16 count 100000, 0x1b count 50000, 0x6e count
/// 10000 disabled), and invented suspend/resume counts (55 each) and aggregate
/// totals (total_events 5510, total_gpes 660000), which `/proc/acpistat` then
/// displayed as if they were real ACPI event measurements.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The ACPI subsystem is expected to call
/// [`register_gpe`] per enumerated GPE and the record_* functions as events
/// fire.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        event_counts: [0; NUM_EVENT_TYPES],
        gpes: Vec::new(),
        current_state: SState::S0,
        suspend_count: 0,
        resume_count: 0,
        total_events: 0,
        total_gpes: 0,
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

/// Register a GPE (General Purpose Event) with its enabled/disabled state.
///
/// The ACPI subsystem calls this once per GPE it enumerates at bring-up so the
/// table reflects the real GPE block — including GPEs that are *disabled* and
/// would therefore never fire (and so would never be created by the
/// auto-registering [`record_gpe`] path).  Counts start at zero.
pub fn register_gpe(gpe_num: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        if state.gpes.iter().any(|g| g.gpe_num == gpe_num) { return Err(KernelError::AlreadyExists); }
        if state.gpes.len() >= MAX_GPES { return Err(KernelError::ResourceExhausted); }
        state.gpes.push(GpeStats { gpe_num, count: 0, enabled });
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
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/acpistat must never surface).
    // Resetting first clears any residue from a prior `acpistat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated GPEs, event counts, or totals.
    //    The initial sleep state is S0 (a real power-on condition, not data).
    assert_eq!(gpe_list().len(), 0);
    for (_, count) in event_counts() { assert_eq!(count, 0); }
    let (e0, g0, s0, r0, _o0) = stats();
    assert_eq!((e0, g0, s0, r0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: register_gpe seeds zeroed rows (incl. a disabled one); dup fails.
    register_gpe(0x11, true).expect("reg gpe");
    register_gpe(0x6e, false).expect("reg gpe disabled");
    assert!(register_gpe(0x11, true).is_err());
    assert_eq!(gpe_list().len(), 2);
    let g = gpe_list().iter().find(|g| g.gpe_num == 0x6e).cloned().expect("gpe");
    assert_eq!(g.count, 0);
    assert!(!g.enabled);
    crate::serial_println!("  [2/8] register_gpe: OK");

    // 3: Event increments its slot and the total exactly from zero.
    record_event(AcpiEvent::PowerButton).expect("event");
    assert_eq!(event_counts()[AcpiEvent::PowerButton.index()].1, 1);
    crate::serial_println!("  [3/8] event: OK");

    // 4: GPE firing on a registered GPE increments its count from zero.
    record_gpe(0x11).expect("gpe_exist");
    let g = gpe_list().iter().find(|g| g.gpe_num == 0x11).cloned().expect("gpe");
    assert_eq!(g.count, 1);
    crate::serial_println!("  [4/8] gpe existing: OK");

    // 5: GPE firing on an unseen number auto-registers it (count 1, enabled).
    record_gpe(0xFF).expect("gpe_new");
    assert_eq!(gpe_list().len(), 3);
    let g = gpe_list().iter().find(|g| g.gpe_num == 0xFF).cloned().expect("gpe");
    assert_eq!(g.count, 1);
    assert!(g.enabled);
    crate::serial_println!("  [5/8] gpe new: OK");

    // 6: S-state suspend then resume increment exactly from zero.
    set_s_state(SState::S3).expect("suspend");
    let (_, _, sus, _, _) = stats();
    assert_eq!(sus, 1);
    set_s_state(SState::S0).expect("resume");
    let (_, _, _, res, _) = stats();
    assert_eq!(res, 1);
    crate::serial_println!("  [6/8] suspend/resume: OK");

    // 7: Event-count array has all 8 slots; only PowerButton is non-zero.
    let counts = event_counts();
    assert_eq!(counts.len(), NUM_EVENT_TYPES);
    assert_eq!(counts[AcpiEvent::BatteryUpdate.index()].1, 0);
    crate::serial_println!("  [7/8] event counts: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (events, gpes, suspends, resumes, ops) = stats();
    assert_eq!(events, 1);   // one record_event
    assert_eq!(gpes, 2);     // two record_gpe firings (0x11, 0xFF)
    assert_eq!(suspends, 1);
    assert_eq!(resumes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/acpistat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the ACPI subsystem wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("acpistat::self_test() — all 8 tests passed");
}
