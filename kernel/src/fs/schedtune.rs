//! Scheduler tuning parameters — settings panel for scheduling model configuration.
//!
//! Provides a settings interface for choosing and tuning the scheduler model.
//! Users can select workload-type presets (Desktop, Server, etc.) that populate
//! tuning parameters with recommended values.  Shows advantages and disadvantages
//! of each configuration.
//!
//! ## Design Reference
//!
//! design.txt line 1277: scheduling model/tuning parameters
//! design.txt line 1279: show advantages and disadvantages of each profile
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Scheduler Tuning
//!   → schedtune::list_profiles() → available tuning profiles
//!   → schedtune::apply_profile(id) → set active configuration
//!   → schedtune::tradeoffs(id) → advantages & disadvantages
//!
//! Kernel scheduler reads active config
//!   → schedtune::active_config() → current tuning parameters
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

/// Scheduler model / algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedModel {
    /// Priority round-robin — our default.
    PriorityRoundRobin,
    /// Completely Fair Scheduler style (virtual-runtime fairness).
    Cfs,
    /// Earliest Eligible Virtual Deadline First.
    Eevdf,
    /// Brain Fuck Scheduler — single-queue desktop-optimised.
    Bfs,
    /// Real-time FIFO (for RT tasks).
    RtFifo,
}

/// Workload type for preset selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadType {
    /// General desktop use — balanced latency and throughput.
    Desktop,
    /// Server workload — maximise throughput, tolerate latency.
    Server,
    /// Gaming — minimise jitter, prioritise foreground.
    Gaming,
    /// Development / compilation — parallel throughput.
    Development,
    /// Audio/video production — ultra-low latency.
    Realtime,
    /// Embedded / low-power — minimise context switches.
    LowPower,
}

/// Preemption model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreemptModel {
    /// No kernel preemption (only at syscall boundaries).
    None,
    /// Voluntary preemption (explicit yield points in kernel).
    Voluntary,
    /// Full preemption (anywhere in kernel except critical sections).
    Full,
    /// Real-time preemption (even critical sections can be preempted).
    RealTime,
}

/// Load balancing strategy for SMP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceStrategy {
    /// Work-stealing from busy CPUs.
    WorkStealing,
    /// Push migration — migrate tasks when imbalanced.
    PushMigration,
    /// Hybrid push + steal.
    Hybrid,
    /// None — tasks stay on their initial CPU.
    Pinned,
}

/// A complete set of scheduler tuning parameters.
#[derive(Debug, Clone)]
pub struct SchedConfig {
    /// Unique ID.
    pub id: u64,
    /// Profile name.
    pub name: String,
    /// Workload type this was designed for.
    pub workload: WorkloadType,
    /// Scheduler model.
    pub model: SchedModel,
    /// Preemption model.
    pub preempt: PreemptModel,
    /// Time slice in microseconds (100-100000).
    pub timeslice_us: u32,
    /// Minimum granularity in microseconds (for CFS/EEVDF).
    pub min_granularity_us: u32,
    /// Target scheduling latency in microseconds.
    pub target_latency_us: u32,
    /// Number of priority levels (2-256).
    pub priority_levels: u16,
    /// Whether to boost interactive tasks.
    pub interactive_boost: bool,
    /// Interactive detection threshold in microseconds (sleep time).
    pub interactive_threshold_us: u32,
    /// CPU affinity strictness (0=loose, 100=strict).
    pub affinity_strictness: u8,
    /// Load balance interval in milliseconds.
    pub balance_interval_ms: u32,
    /// Load balance strategy.
    pub balance_strategy: BalanceStrategy,
    /// Whether to use per-CPU run queues.
    pub per_cpu_queues: bool,
    /// Whether to enable NUMA-aware scheduling.
    pub numa_aware: bool,
    /// Maximum migration cost in microseconds.
    pub migration_cost_us: u32,
    /// Whether to enable priority inheritance.
    pub priority_inheritance: bool,
    /// Idle governor: power-save vs performance.
    pub idle_powersave: bool,
    /// Whether this is a built-in profile.
    pub builtin: bool,
    /// Whether this profile is currently active.
    pub active: bool,
    /// Requires recompile to apply.
    pub requires_recompile: bool,
    /// Requires reboot to apply.
    pub requires_reboot: bool,
}

/// Tradeoff description for user display.
#[derive(Debug, Clone)]
pub struct TradeoffInfo {
    /// Short label (e.g., "High throughput").
    pub label: String,
    /// Advantages of this configuration.
    pub advantages: Vec<String>,
    /// Disadvantages / tradeoffs.
    pub disadvantages: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    profiles: Vec<SchedConfig>,
    tradeoffs: Vec<(u64, TradeoffInfo)>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    profiles: Vec::new(),
    tradeoffs: Vec::new(),
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Create a tuning profile.
pub fn create_profile(
    name: &str,
    workload: WorkloadType,
    model: SchedModel,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.profiles.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let cfg = defaults_for(id, name, workload, model);
    state.profiles.push(cfg);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Generate default tuning parameters for a workload/model combination.
fn defaults_for(id: u64, name: &str, workload: WorkloadType, model: SchedModel) -> SchedConfig {
    let (timeslice, min_gran, target_lat, interactive, affinity, balance_ms, preempt, idle_ps) =
        match workload {
            WorkloadType::Desktop => (4000, 750, 6000, true, 30, 4, PreemptModel::Full, true),
            WorkloadType::Server => (10000, 2000, 24000, false, 60, 8, PreemptModel::Voluntary, false),
            WorkloadType::Gaming => (2000, 500, 3000, true, 80, 2, PreemptModel::Full, false),
            WorkloadType::Development => (6000, 1000, 12000, true, 40, 6, PreemptModel::Voluntary, true),
            WorkloadType::Realtime => (1000, 250, 1000, true, 90, 1, PreemptModel::RealTime, false),
            WorkloadType::LowPower => (15000, 3000, 30000, false, 50, 16, PreemptModel::None, true),
        };

    let balance_strategy = match workload {
        WorkloadType::Gaming | WorkloadType::Realtime => BalanceStrategy::Pinned,
        WorkloadType::Server => BalanceStrategy::PushMigration,
        _ => BalanceStrategy::Hybrid,
    };

    SchedConfig {
        id,
        name: String::from(name),
        workload,
        model,
        preempt,
        timeslice_us: timeslice,
        min_granularity_us: min_gran,
        target_latency_us: target_lat,
        priority_levels: 140,
        interactive_boost: interactive,
        interactive_threshold_us: 1000,
        affinity_strictness: affinity,
        balance_interval_ms: balance_ms,
        balance_strategy,
        per_cpu_queues: true,
        numa_aware: true,
        migration_cost_us: 500,
        priority_inheritance: true,
        idle_powersave: idle_ps,
        builtin: false,
        active: false,
        requires_recompile: model != SchedModel::PriorityRoundRobin,
        requires_reboot: true,
    }
}

/// Remove a profile (built-in profiles cannot be removed).
pub fn remove_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    if p.builtin {
        return Err(KernelError::PermissionDenied);
    }
    if p.active {
        return Err(KernelError::PermissionDenied);
    }
    state.profiles.retain(|p| p.id != profile_id);
    state.tradeoffs.retain(|(pid, _)| *pid != profile_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get a profile by ID.
pub fn get_profile(profile_id: u64) -> KernelResult<SchedConfig> {
    STATE.lock().profiles.iter().find(|p| p.id == profile_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all profiles.
pub fn list_profiles() -> Vec<SchedConfig> {
    STATE.lock().profiles.clone()
}

/// Get the active profile.
pub fn active_profile() -> KernelResult<SchedConfig> {
    STATE.lock().profiles.iter().find(|p| p.active).cloned()
        .ok_or(KernelError::NotFound)
}

/// Activate a profile (deactivate all others).
pub fn apply_profile(profile_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.profiles.iter().any(|p| p.id == profile_id) {
        return Err(KernelError::NotFound);
    }
    for p in &mut state.profiles {
        p.active = p.id == profile_id;
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Parameter tuning
// ---------------------------------------------------------------------------

/// Set time slice.
pub fn set_timeslice(profile_id: u64, us: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.timeslice_us = us.clamp(100, 100_000);
    state.changes += 1;
    Ok(())
}

/// Set preemption model.
pub fn set_preempt(profile_id: u64, preempt: PreemptModel) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.preempt = preempt;
    p.requires_recompile = true;
    state.changes += 1;
    Ok(())
}

/// Set target latency.
pub fn set_target_latency(profile_id: u64, us: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.target_latency_us = us.clamp(100, 100_000);
    state.changes += 1;
    Ok(())
}

/// Set interactive boost.
pub fn set_interactive_boost(profile_id: u64, enable: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.interactive_boost = enable;
    state.changes += 1;
    Ok(())
}

/// Set affinity strictness (0-100).
pub fn set_affinity(profile_id: u64, strictness: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.affinity_strictness = strictness.min(100);
    state.changes += 1;
    Ok(())
}

/// Set load balance strategy.
pub fn set_balance_strategy(profile_id: u64, strategy: BalanceStrategy) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.balance_strategy = strategy;
    state.changes += 1;
    Ok(())
}

/// Set balance interval.
pub fn set_balance_interval(profile_id: u64, ms: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.balance_interval_ms = ms.clamp(1, 1000);
    state.changes += 1;
    Ok(())
}

/// Set priority inheritance.
pub fn set_priority_inheritance(profile_id: u64, enable: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.priority_inheritance = enable;
    state.changes += 1;
    Ok(())
}

/// Set NUMA-aware scheduling.
pub fn set_numa_aware(profile_id: u64, enable: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.numa_aware = enable;
    state.changes += 1;
    Ok(())
}

/// Set idle power-save.
pub fn set_idle_powersave(profile_id: u64, enable: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
        .ok_or(KernelError::NotFound)?;
    p.idle_powersave = enable;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tradeoff display
// ---------------------------------------------------------------------------

/// Get tradeoff information for a profile.
pub fn tradeoffs(profile_id: u64) -> KernelResult<TradeoffInfo> {
    let state = STATE.lock();
    if !state.profiles.iter().any(|p| p.id == profile_id) {
        return Err(KernelError::NotFound);
    }
    state.tradeoffs.iter()
        .find(|(pid, _)| *pid == profile_id)
        .map(|(_, info)| info.clone())
        .ok_or(KernelError::NotFound)
}

/// List all tradeoffs.
pub fn list_tradeoffs() -> Vec<(u64, TradeoffInfo)> {
    STATE.lock().tradeoffs.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

fn add_builtin(state: &mut State, name: &str, workload: WorkloadType, model: SchedModel,
    active: bool, advantages: &[&str], disadvantages: &[&str])
{
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut cfg = defaults_for(id, name, workload, model);
    cfg.builtin = true;
    cfg.active = active;
    state.profiles.push(cfg);

    let info = TradeoffInfo {
        label: String::from(name),
        advantages: advantages.iter().map(|s| String::from(*s)).collect(),
        disadvantages: disadvantages.iter().map(|s| String::from(*s)).collect(),
    };
    state.tradeoffs.push((id, info));
}

/// Initialise default scheduler tuning profiles.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.profiles.is_empty() {
        return;
    }

    add_builtin(&mut state, "Desktop (Default)", WorkloadType::Desktop,
        SchedModel::PriorityRoundRobin, true,
        &["Good interactivity for GUI applications",
          "Balanced CPU distribution across tasks",
          "Low latency for user input response",
          "Energy-efficient with idle power-save"],
        &["Slightly lower throughput than Server profile",
          "May not saturate all CPUs under batch workloads"]);

    add_builtin(&mut state, "Server", WorkloadType::Server,
        SchedModel::Cfs, false,
        &["Maximum throughput for parallel workloads",
          "Fair CPU time distribution",
          "Good for web servers, databases, VMs",
          "Efficient load balancing across cores"],
        &["Higher scheduling latency",
          "Less responsive to interactive tasks",
          "Not ideal for desktop use"]);

    add_builtin(&mut state, "Gaming", WorkloadType::Gaming,
        SchedModel::PriorityRoundRobin, false,
        &["Minimal jitter for frame timing",
          "Foreground process gets priority",
          "Strict CPU affinity reduces cache thrashing",
          "Short time slices for fast preemption"],
        &["Background tasks may be starved",
          "Higher CPU usage (no power-save)",
          "Pinned balancing limits load distribution"]);

    add_builtin(&mut state, "Development", WorkloadType::Development,
        SchedModel::PriorityRoundRobin, false,
        &["Good parallel compilation throughput",
          "Interactive IDE stays responsive",
          "Moderate energy efficiency",
          "Hybrid balancing works well with mixed workloads"],
        &["Compile times not as fast as Server profile",
          "Not optimised for single-threaded benchmarks"]);

    add_builtin(&mut state, "Realtime / Audio", WorkloadType::Realtime,
        SchedModel::RtFifo, false,
        &["Ultra-low latency (< 1ms scheduling)",
          "Deterministic timing for audio/video",
          "RT preemption model prevents priority inversion",
          "Strict affinity prevents migration jitter"],
        &["Requires RT preemption (recompile needed)",
          "Badly-behaved RT tasks can lock out the system",
          "No power saving",
          "Not suitable for general desktop use"]);

    add_builtin(&mut state, "Low Power", WorkloadType::LowPower,
        SchedModel::PriorityRoundRobin, false,
        &["Minimal context switches save energy",
          "Longer time slices reduce overhead",
          "Aggressive idle power-save",
          "Good for laptops on battery"],
        &["Higher latency for interactive tasks",
          "Sluggish UI response under load",
          "Poor for latency-sensitive workloads"]);

    state.changes += 1;
}

/// Return (profile_count, active_name_len, tradeoff_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.profiles.len();
    let active = state.profiles.iter().filter(|p| p.active).count();
    let tradeoffs = state.tradeoffs.len();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, active, tradeoffs, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.profiles.clear();
    state.tradeoffs.clear();
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: init defaults.
    serial_println!("schedtune::self_test 1: init defaults");
    init_defaults();
    let profiles = list_profiles();
    assert!(profiles.len() >= 6);
    // Desktop should be active by default.
    let active = active_profile()?;
    assert_eq!(active.workload, WorkloadType::Desktop);
    assert!(active.active);

    // Test 2: create custom profile.
    serial_println!("schedtune::self_test 2: create custom");
    clear_all();
    let p1 = create_profile("Custom1", WorkloadType::Gaming, SchedModel::PriorityRoundRobin)?;
    let p2 = create_profile("Custom2", WorkloadType::Server, SchedModel::Cfs)?;
    assert_eq!(list_profiles().len(), 2);

    // Test 3: apply profile.
    serial_println!("schedtune::self_test 3: apply");
    apply_profile(p1)?;
    let a = active_profile()?;
    assert_eq!(a.id, p1);
    apply_profile(p2)?;
    let a = active_profile()?;
    assert_eq!(a.id, p2);
    // p1 should no longer be active.
    let p1_cfg = get_profile(p1)?;
    assert!(!p1_cfg.active);

    // Test 4: parameter tuning.
    serial_println!("schedtune::self_test 4: tuning");
    set_timeslice(p1, 8000)?;
    set_preempt(p1, PreemptModel::RealTime)?;
    set_target_latency(p1, 2000)?;
    set_interactive_boost(p1, false)?;
    set_affinity(p1, 95)?;
    set_balance_strategy(p1, BalanceStrategy::WorkStealing)?;
    set_balance_interval(p1, 10)?;
    set_priority_inheritance(p1, false)?;
    set_numa_aware(p1, false)?;
    set_idle_powersave(p1, true)?;
    let cfg = get_profile(p1)?;
    assert_eq!(cfg.timeslice_us, 8000);
    assert_eq!(cfg.preempt, PreemptModel::RealTime);
    assert_eq!(cfg.target_latency_us, 2000);
    assert!(!cfg.interactive_boost);
    assert_eq!(cfg.affinity_strictness, 95);
    assert_eq!(cfg.balance_strategy, BalanceStrategy::WorkStealing);

    // Test 5: remove (cannot remove active).
    serial_println!("schedtune::self_test 5: remove");
    assert!(remove_profile(p2).is_err()); // p2 is active
    apply_profile(p1)?; // switch active to p1
    remove_profile(p2)?; // now p2 can be removed
    assert_eq!(list_profiles().len(), 1);

    // Test 6: built-in protection.
    serial_println!("schedtune::self_test 6: built-in protection");
    clear_all();
    init_defaults();
    let builtins = list_profiles();
    assert!(remove_profile(builtins[0].id).is_err());

    // Test 7: tradeoffs.
    serial_println!("schedtune::self_test 7: tradeoffs");
    let info = tradeoffs(builtins[0].id)?;
    assert!(!info.advantages.is_empty());
    assert!(!info.disadvantages.is_empty());

    clear_all();
    serial_println!("schedtune::self_test: all 7 tests passed");
    Ok(())
}
