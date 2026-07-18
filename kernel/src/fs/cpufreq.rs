//! CPU Frequency — CPU frequency scaling management.
//!
//! Manages CPU frequency governors, per-core frequency settings,
//! and power/performance profiles for dynamic frequency scaling.
//!
//! ## Architecture
//!
//! ```text
//! Frequency management
//!   → cpufreq::set_governor(governor) → change scaling policy
//!   → cpufreq::get_frequency(cpu) → current frequency
//!   → cpufreq::set_range(cpu, min, max) → frequency limits
//!
//! Integration:
//!   → power (power management)
//!   → powerprofile (power profiles)
//!   → energysaver (energy saver)
//!   → schedtune (scheduler tuning)
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

/// CPU frequency governor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Governor {
    Performance,
    Powersave,
    OnDemand,
    Conservative,
    Schedutil,
    Userspace,
}

impl Governor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Powersave => "powersave",
            Self::OnDemand => "ondemand",
            Self::Conservative => "conservative",
            Self::Schedutil => "schedutil",
            Self::Userspace => "userspace",
        }
    }
}

/// Per-CPU frequency info.
#[derive(Debug, Clone)]
pub struct CpuFreqInfo {
    pub cpu_id: u32,
    pub current_khz: u64,
    pub min_khz: u64,
    pub max_khz: u64,
    pub base_khz: u64,
    pub governor: Governor,
    pub scaling_min_khz: u64,
    pub scaling_max_khz: u64,
    pub transitions: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 128;

struct State {
    cpus: Vec<CpuFreqInfo>,
    global_governor: Governor,
    total_transitions: u64,
    total_governor_changes: u64,
    boost_enabled: bool,
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
    // Simulate 4 CPUs at 3.6 GHz base, 4.8 GHz max.
    let mut cpus = Vec::new();
    for i in 0..4u32 {
        cpus.push(CpuFreqInfo {
            cpu_id: i,
            current_khz: 3_600_000,
            min_khz: 800_000,
            max_khz: 4_800_000,
            base_khz: 3_600_000,
            governor: Governor::Schedutil,
            scaling_min_khz: 800_000,
            scaling_max_khz: 4_800_000,
            transitions: 0,
        });
    }
    *guard = Some(State {
        cpus,
        global_governor: Governor::Schedutil,
        total_transitions: 0,
        total_governor_changes: 0,
        boost_enabled: true,
        ops: 0,
    });
}

/// Set the global governor.
pub fn set_governor(governor: Governor) -> KernelResult<()> {
    with_state(|state| {
        state.global_governor = governor;
        for cpu in &mut state.cpus {
            cpu.governor = governor;
        }
        state.total_governor_changes += 1;
        Ok(())
    })
}

/// Get the global governor.
pub fn get_governor() -> Option<Governor> {
    STATE.lock().as_ref().map(|s| s.global_governor)
}

/// Set per-CPU governor.
pub fn set_cpu_governor(cpu_id: u32, governor: Governor) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.governor = governor;
        state.total_governor_changes += 1;
        Ok(())
    })
}

/// Get frequency info for a CPU.
pub fn get_cpu_info(cpu_id: u32) -> Option<CpuFreqInfo> {
    STATE.lock().as_ref().and_then(|s| {
        s.cpus.iter().find(|c| c.cpu_id == cpu_id).cloned()
    })
}

/// Set frequency scaling range.
pub fn set_scaling_range(cpu_id: u32, min_khz: u64, max_khz: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        if min_khz > max_khz || min_khz < cpu.min_khz || max_khz > cpu.max_khz {
            return Err(KernelError::InvalidArgument);
        }
        cpu.scaling_min_khz = min_khz;
        cpu.scaling_max_khz = max_khz;
        Ok(())
    })
}

/// Simulate a frequency transition.
pub fn set_frequency(cpu_id: u32, freq_khz: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        if freq_khz < cpu.scaling_min_khz || freq_khz > cpu.scaling_max_khz {
            return Err(KernelError::InvalidArgument);
        }
        cpu.current_khz = freq_khz;
        cpu.transitions += 1;
        state.total_transitions += 1;
        Ok(())
    })
}

/// Enable/disable boost.
pub fn set_boost(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.boost_enabled = enabled;
        Ok(())
    })
}

/// Check if boost is enabled.
pub fn boost_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.boost_enabled)
}

/// List all CPUs.
pub fn list_cpus() -> Vec<CpuFreqInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Format frequency.
pub fn format_freq(khz: u64) -> String {
    if khz >= 1_000_000 {
        format!("{}.{} GHz", khz / 1_000_000, (khz % 1_000_000) / 100_000)
    } else {
        format!("{} MHz", khz / 1_000)
    }
}

/// Statistics: (cpu_count, total_transitions, total_governor_changes, boost_enabled, ops).
pub fn stats() -> (usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_transitions, s.total_governor_changes, s.boost_enabled, s.ops),
        None => (0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cpufreq::self_test() — running tests...");
    init_defaults();

    // 1: Default CPUs.
    assert_eq!(list_cpus().len(), 4);
    let cpu0 = get_cpu_info(0).expect("cpu0");
    assert_eq!(cpu0.governor, Governor::Schedutil);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Set global governor.
    set_governor(Governor::Performance).expect("set_gov");
    for cpu in list_cpus() {
        assert_eq!(cpu.governor, Governor::Performance);
    }
    crate::serial_println!("  [2/8] global governor: OK");

    // 3: Per-CPU governor.
    set_cpu_governor(1, Governor::Powersave).expect("per_cpu_gov");
    let cpu1 = get_cpu_info(1).expect("cpu1");
    assert_eq!(cpu1.governor, Governor::Powersave);
    crate::serial_println!("  [3/8] per-CPU governor: OK");

    // 4: Set frequency.
    set_frequency(0, 4_000_000).expect("set_freq");
    let cpu0 = get_cpu_info(0).expect("cpu0_2");
    assert_eq!(cpu0.current_khz, 4_000_000);
    crate::serial_println!("  [4/8] set frequency: OK");

    // 5: Scaling range.
    set_scaling_range(0, 1_000_000, 4_000_000).expect("range");
    assert!(set_frequency(0, 4_500_000).is_err()); // Above max.
    assert!(set_frequency(0, 500_000).is_err()); // Below min.
    crate::serial_println!("  [5/8] scaling range: OK");

    // 6: Boost.
    assert!(boost_enabled());
    set_boost(false).expect("disable_boost");
    assert!(!boost_enabled());
    set_boost(true).expect("enable_boost");
    crate::serial_println!("  [6/8] boost: OK");

    // 7: Format frequency.
    assert_eq!(format_freq(3_600_000), "3.6 GHz");
    assert_eq!(format_freq(800_000), "800 MHz");
    crate::serial_println!("  [7/8] format: OK");

    // 8: Stats.
    let (cpu_count, transitions, gov_changes, boost, ops) = stats();
    assert_eq!(cpu_count, 4);
    assert!(transitions >= 1);
    assert!(gov_changes >= 2);
    assert!(boost);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cpufreq::self_test() — all 8 tests passed");
}
