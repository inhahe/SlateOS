//! CPU Thermal Throttle — per-CPU throttling event monitoring.
//!
//! Tracks thermal throttle events, duration, frequency capping,
//! and per-package thermal pressure. Essential for understanding
//! sustained workload performance degradation.
//!
//! ## Architecture
//!
//! ```text
//! Thermal throttle monitoring
//!   → cputhr::record_throttle(cpu, duration_ms) → throttle event
//!   → cputhr::record_cap(cpu, max_mhz) → freq cap applied
//!   → cputhr::set_temp(cpu, millicelsius) → update temperature
//!   → cputhr::per_cpu() → per-CPU throttle stats
//!
//! Integration:
//!   → thermal (thermal zones)
//!   → cpustat (CPU utilization)
//!   → cpufreq (frequency scaling)
//!   → powerstat (power domains)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-CPU throttle stats.
#[derive(Debug, Clone)]
pub struct CpuThrottleStats {
    pub cpu_id: u32,
    pub package_id: u32,
    pub temp_mc: u32,          // millicelsius
    pub throttle_count: u64,
    pub total_throttle_ms: u64,
    pub max_throttle_ms: u64,
    pub freq_cap_mhz: u32,    // 0 = no cap
    pub cap_count: u64,
    pub is_throttled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;

struct State {
    cpus: Vec<CpuThrottleStats>,
    total_throttle_events: u64,
    total_throttle_ms: u64,
    total_cap_events: u64,
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
        cpus: alloc::vec![
            CpuThrottleStats { cpu_id: 0, package_id: 0, temp_mc: 65_000, throttle_count: 100, total_throttle_ms: 50_000, max_throttle_ms: 2000, freq_cap_mhz: 0, cap_count: 50, is_throttled: false },
            CpuThrottleStats { cpu_id: 1, package_id: 0, temp_mc: 67_000, throttle_count: 120, total_throttle_ms: 60_000, max_throttle_ms: 2500, freq_cap_mhz: 0, cap_count: 60, is_throttled: false },
            CpuThrottleStats { cpu_id: 2, package_id: 0, temp_mc: 63_000, throttle_count: 80, total_throttle_ms: 40_000, max_throttle_ms: 1500, freq_cap_mhz: 0, cap_count: 40, is_throttled: false },
            CpuThrottleStats { cpu_id: 3, package_id: 0, temp_mc: 64_000, throttle_count: 90, total_throttle_ms: 45_000, max_throttle_ms: 1800, freq_cap_mhz: 0, cap_count: 45, is_throttled: false },
        ],
        total_throttle_events: 390,
        total_throttle_ms: 195_000,
        total_cap_events: 195,
        ops: 0,
    });
}

/// Record a throttle event.
pub fn record_throttle(cpu_id: u32, duration_ms: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.throttle_count += 1;
        cpu.total_throttle_ms += duration_ms;
        if duration_ms > cpu.max_throttle_ms { cpu.max_throttle_ms = duration_ms; }
        cpu.is_throttled = true;
        state.total_throttle_events += 1;
        state.total_throttle_ms += duration_ms;
        Ok(())
    })
}

/// Clear throttle state.
pub fn clear_throttle(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.is_throttled = false;
        cpu.freq_cap_mhz = 0;
        Ok(())
    })
}

/// Record a frequency cap.
pub fn record_cap(cpu_id: u32, max_mhz: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.freq_cap_mhz = max_mhz;
        cpu.cap_count += 1;
        state.total_cap_events += 1;
        Ok(())
    })
}

/// Update temperature.
pub fn set_temp(cpu_id: u32, millicelsius: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.temp_mc = millicelsius;
        Ok(())
    })
}

/// Per-CPU stats.
pub fn per_cpu() -> Vec<CpuThrottleStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Count of currently throttled CPUs.
pub fn throttled_count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| {
        s.cpus.iter().filter(|c| c.is_throttled).count()
    })
}

/// Statistics: (cpu_count, total_events, total_ms, total_caps, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_throttle_events, s.total_throttle_ms, s.total_cap_events, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cputhr::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Throttle.
    let before = per_cpu()[0].throttle_count;
    record_throttle(0, 500).expect("throttle");
    let after = per_cpu()[0].throttle_count;
    assert_eq!(after, before + 1);
    assert!(per_cpu()[0].is_throttled);
    crate::serial_println!("  [2/8] throttle: OK");

    // 3: Clear.
    clear_throttle(0).expect("clear");
    assert!(!per_cpu()[0].is_throttled);
    crate::serial_println!("  [3/8] clear: OK");

    // 4: Cap.
    record_cap(0, 2000).expect("cap");
    assert_eq!(per_cpu()[0].freq_cap_mhz, 2000);
    crate::serial_println!("  [4/8] cap: OK");

    // 5: Temperature.
    set_temp(0, 85_000).expect("temp");
    assert_eq!(per_cpu()[0].temp_mc, 85_000);
    crate::serial_println!("  [5/8] temp: OK");

    // 6: Max throttle.
    record_throttle(1, 10_000).expect("big_throttle");
    assert!(per_cpu()[1].max_throttle_ms >= 10_000);
    crate::serial_println!("  [6/8] max throttle: OK");

    // 7: Not found.
    assert!(record_throttle(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, events, ms, caps, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(events > 390);
    assert!(ms > 195_000);
    assert!(caps > 195);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cputhr::self_test() — all 8 tests passed");
}
