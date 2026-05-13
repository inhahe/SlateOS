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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: alloc::vec![
            DevFreqInfo { id: 1, name: String::from("gpu0"), min_freq_khz: 200_000, max_freq_khz: 2_000_000, cur_freq_khz: 1_500_000, governor: Governor::OnDemand, transitions: 500_000, time_in_state_ms: [100_000, 200_000, 300_000, 500_000, 200_000] },
            DevFreqInfo { id: 2, name: String::from("membus"), min_freq_khz: 400_000, max_freq_khz: 3_200_000, cur_freq_khz: 3_200_000, governor: Governor::Performance, transitions: 10_000, time_in_state_ms: [10_000, 20_000, 30_000, 50_000, 1_000_000] },
        ],
        next_id: 3,
        total_transitions: 510_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    let id = register("test_dev", 100_000, 1_000_000).expect("register");
    assert!(id >= 3);
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Transition.
    record_transition(id, 500_000).expect("transition");
    let d = list().iter().find(|d| d.id == id).cloned().unwrap();
    assert_eq!(d.cur_freq_khz, 500_000);
    assert_eq!(d.transitions, 1);
    crate::serial_println!("  [3/8] transition: OK");

    // 4: Clamping.
    record_transition(id, 9_999_999).expect("clamp_high");
    let d = list().iter().find(|d| d.id == id).cloned().unwrap();
    assert_eq!(d.cur_freq_khz, 1_000_000); // clamped to max
    crate::serial_println!("  [4/8] clamp: OK");

    // 5: Governor.
    set_governor(id, Governor::Performance).expect("governor");
    let d = list().iter().find(|d| d.id == id).cloned().unwrap();
    assert_eq!(d.governor, Governor::Performance);
    crate::serial_println!("  [5/8] governor: OK");

    // 6: Unregister.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 2);
    assert!(unregister(id).is_err());
    crate::serial_println!("  [6/8] unregister: OK");

    // 7: Not found.
    assert!(record_transition(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (devs, trans, ops) = stats();
    assert_eq!(devs, 2);
    assert!(trans > 510_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("devfreq::self_test() — all 8 tests passed");
}
