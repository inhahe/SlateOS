//! Device Frequency — device frequency scaling monitoring.
//!
//! Tracks device frequency governors, transitions, and
//! power/performance states for GPU, memory bus, and other
//! frequency-scalable devices. Essential for power management.
//!
//! ## Architecture
//!
//! ```text
//! Device frequency monitoring
//!   → devfreq::register(name, min, max) → register device
//!   → devfreq::record_transition(id, freq) → frequency change
//!   → devfreq::set_governor(id, gov) → change governor
//!   → devfreq::list() → list devices
//!
//! Integration:
//!   → cpufreq (CPU frequency)
//!   → powerstat (power domains)
//!   → thermal (thermal zones)
//!   → cputhr (CPU throttle)
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

/// Frequency governor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Governor {
    Performance,
    PowerSave,
    Userspace,
    OnDemand,
    Simple,
}

impl Governor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::PowerSave => "powersave",
            Self::Userspace => "userspace",
            Self::OnDemand => "ondemand",
            Self::Simple => "simple",
        }
    }
}

/// Device frequency info.
#[derive(Debug, Clone)]
pub struct DevFreqInfo {
    pub id: u32,
    pub name: String,
    pub min_freq_khz: u64,
    pub max_freq_khz: u64,
    pub cur_freq_khz: u64,
    pub governor: Governor,
    pub transitions: u64,
    pub time_in_state_ms: [u64; 5], // 5 frequency buckets
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

struct State {
    devices: Vec<DevFreqInfo>,
    next_id: u32,
    total_transitions: u64,
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

/// Initialise the device-frequency-scaling statistics state.
///
/// Starts with no registered devices and zero transitions. The
/// `/proc/devfreq` generator and the `devfreq` kshell command surface this
/// table as if it reflects real frequency-scalable devices, so seeding it
/// with invented devices and transition counts would be fabricated procfs
/// data. Devices are registered through [`register`] by the power-
/// management subsystem as it discovers real frequency-scalable hardware,
/// and the counters advance only through real [`record_transition`] calls.
///
/// (Previously this seeded two fictional devices — "gpu0" 200MHz-2GHz
/// OnDemand with 500k transitions, and "membus" 400MHz-3.2GHz Performance
/// with 10k transitions — plus invented per-bucket time-in-state values
/// and a 510k total-transitions count.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        next_id: 1,
        total_transitions: 0,
        ops: 0,
    });
}

/// Register a device.
pub fn register(name: &str, min_khz: u64, max_khz: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(DevFreqInfo {
            id, name: String::from(name), min_freq_khz: min_khz, max_freq_khz: max_khz,
            cur_freq_khz: min_khz, governor: Governor::OnDemand, transitions: 0,
            time_in_state_ms: [0; 5],
        });
        Ok(id)
    })
}

/// Record a frequency transition.
pub fn record_transition(id: u32, new_freq_khz: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        d.cur_freq_khz = new_freq_khz.clamp(d.min_freq_khz, d.max_freq_khz);
        d.transitions += 1;
        state.total_transitions += 1;
        Ok(())
    })
}

/// Set governor.
pub fn set_governor(id: u32, gov: Governor) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        d.governor = gov;
        Ok(())
    })
}

/// Unregister a device.
pub fn unregister(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.devices.iter().position(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        state.devices.remove(idx);
        Ok(())
    })
}

/// List devices.
pub fn list() -> Vec<DevFreqInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Statistics: (device_count, total_transitions, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_transitions, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("devfreq::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live device table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated devices, no transitions.
    assert_eq!(list().len(), 0);
    let (devs0, trans0, _) = stats();
    assert_eq!((devs0, trans0), (0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — ids are monotonic starting at 1; cur defaults to min.
    let id = register("test_dev", 100_000, 1_000_000).expect("register");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    let d = list().into_iter().find(|d| d.id == id).expect("dev");
    assert_eq!(d.cur_freq_khz, 100_000);
    assert_eq!(d.transitions, 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Transition updates current frequency and counts.
    record_transition(id, 500_000).expect("transition");
    let d = list().into_iter().find(|d| d.id == id).expect("dev");
    assert_eq!(d.cur_freq_khz, 500_000);
    assert_eq!(d.transitions, 1);
    crate::serial_println!("  [3/8] transition: OK");

    // 4: Out-of-range targets clamp to [min, max].
    record_transition(id, 9_999_999).expect("clamp_high");
    assert_eq!(list().into_iter().find(|d| d.id == id).expect("dev").cur_freq_khz, 1_000_000);
    record_transition(id, 1).expect("clamp_low");
    assert_eq!(list().into_iter().find(|d| d.id == id).expect("dev").cur_freq_khz, 100_000);
    crate::serial_println!("  [4/8] clamp: OK");

    // 5: Governor change is recorded.
    set_governor(id, Governor::Performance).expect("governor");
    assert_eq!(list().into_iter().find(|d| d.id == id).expect("dev").governor, Governor::Performance);
    crate::serial_println!("  [5/8] governor: OK");

    // 6: Total transitions accumulated exactly (3 record_transition calls).
    let (_, trans, _) = stats();
    assert_eq!(trans, 3);
    crate::serial_println!("  [6/8] total transitions: OK");

    // 7: Unregister removes the device; double-unregister and unknown-id
    //    operations error.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 0);
    assert!(unregister(id).is_err());
    assert!(record_transition(99, 0).is_err());
    crate::serial_println!("  [7/8] unregister + not found: OK");

    // 8: Final stats reflect only the real activity above.
    let (devs, trans, ops) = stats();
    assert_eq!(devs, 0);
    assert_eq!(trans, 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("devfreq::self_test() — all 8 tests passed");
}
