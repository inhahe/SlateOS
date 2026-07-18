//! Power Statistics — energy accounting and power domain monitoring.
//!
//! Tracks power domain states, energy consumption estimates,
//! frequency transitions, and wake events. Essential for
//! power management and battery life optimization.
//!
//! ## Architecture
//!
//! ```text
//! Power monitoring
//!   → powerstat::record_transition(domain, from, to) → state change
//!   → powerstat::record_wake(source) → wake event
//!   → powerstat::update_energy(domain, uj) → energy consumed
//!   → powerstat::domain_stats() → per-domain stats
//!
//! Integration:
//!   → cpuidle (CPU C-states)
//!   → cpufreq (frequency scaling)
//!   → thermal (thermal management)
//!   → sysdiag (diagnostics)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Power domain type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerDomain {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Network,
    Display,
    Audio,
    Usb,
}

impl PowerDomain {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Memory => "memory",
            Self::Storage => "storage",
            Self::Network => "network",
            Self::Display => "display",
            Self::Audio => "audio",
            Self::Usb => "usb",
        }
    }
}

/// Power state for a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Active,
    Idle,
    Standby,
    Suspended,
    Off,
}

impl PowerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Standby => "standby",
            Self::Suspended => "suspended",
            Self::Off => "off",
        }
    }
}

/// Wake event source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeSource {
    Timer,
    Interrupt,
    UserInput,
    Network,
    Usb,
    Rtc,
}

impl WakeSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Interrupt => "irq",
            Self::UserInput => "input",
            Self::Network => "network",
            Self::Usb => "usb",
            Self::Rtc => "rtc",
        }
    }
}

/// Per-domain power stats.
#[derive(Debug, Clone)]
pub struct DomainStats {
    pub domain: PowerDomain,
    pub current_state: PowerState,
    pub energy_uj: u64,
    pub transitions: u64,
    pub active_time_ns: u64,
    pub idle_time_ns: u64,
    pub last_transition_ns: u64,
}

/// A recorded wake event.
#[derive(Debug, Clone)]
pub struct WakeEvent {
    pub source: WakeSource,
    pub timestamp_ns: u64,
    pub domain: PowerDomain,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_WAKE_LOG: usize = 128;

struct State {
    domains: Vec<DomainStats>,
    wake_log: Vec<WakeEvent>,
    total_energy_uj: u64,
    total_transitions: u64,
    total_wakes: u64,
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

/// Initialise an **empty** power statistics table.
///
/// Seeds NO power domains, an empty wake log, and zero counters.  Real power
/// accounting is wired through [`register_domain`] (one row per power domain the
/// power-management layer brings online, with its real initial [`PowerState`])
/// and the `record_transition`/`update_energy`/`record_wake` functions; until
/// those are called the table is genuinely empty, so `/proc/powerstat` and the
/// `powerstat` kshell command report zeros rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional domains (cpu: active / energy
/// 50J / transitions 100k / 3600s active; gpu: idle / 20J / 5k transitions;
/// memory: active / 10J; storage: active / 5J / 50k transitions) plus invented
/// aggregate totals (total_energy_uj 85J, total_transitions 156k, total_wakes
/// 50k), which `/proc/powerstat` then displayed as if they were real measured
/// energy consumption and power-state activity.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The power-management layer is expected to call
/// [`register_domain`] for each domain and the record functions on every state
/// transition, energy update, and wake event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        domains: Vec::new(),
        wake_log: Vec::new(),
        total_energy_uj: 0,
        total_transitions: 0,
        total_wakes: 0,
        ops: 0,
    });
}

/// Register a power domain the power-management layer has brought online.
///
/// The domain starts in `initial_state` with zeroed energy/transition/time
/// counters; `last_transition_ns` is stamped with the current HPET time so the
/// first real transition accounts elapsed time correctly.  Returns
/// [`KernelError::AlreadyExists`] if the domain is already registered.
pub fn register_domain(domain: PowerDomain, initial_state: PowerState) -> KernelResult<()> {
    with_state(|state| {
        if state.domains.iter().any(|d| d.domain == domain) {
            return Err(KernelError::AlreadyExists);
        }
        let now = crate::hpet::elapsed_ns();
        state.domains.push(DomainStats {
            domain,
            current_state: initial_state,
            energy_uj: 0,
            transitions: 0,
            active_time_ns: 0,
            idle_time_ns: 0,
            last_transition_ns: now,
        });
        Ok(())
    })
}

/// Record a power state transition.
pub fn record_transition(domain: PowerDomain, new_state: PowerState) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let d = state.domains.iter_mut().find(|d| d.domain == domain)
            .ok_or(KernelError::NotFound)?;
        let elapsed = now.saturating_sub(d.last_transition_ns);
        match d.current_state {
            PowerState::Active => d.active_time_ns += elapsed,
            _ => d.idle_time_ns += elapsed,
        }
        d.current_state = new_state;
        d.transitions += 1;
        d.last_transition_ns = now;
        state.total_transitions += 1;
        Ok(())
    })
}

/// Record energy consumed (in microjoules).
pub fn update_energy(domain: PowerDomain, uj: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.domains.iter_mut().find(|d| d.domain == domain)
            .ok_or(KernelError::NotFound)?;
        d.energy_uj += uj;
        state.total_energy_uj += uj;
        Ok(())
    })
}

/// Record a wake event.
pub fn record_wake(source: WakeSource, domain: PowerDomain) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.total_wakes += 1;
        if state.wake_log.len() >= MAX_WAKE_LOG { state.wake_log.remove(0); }
        state.wake_log.push(WakeEvent { source, timestamp_ns: now, domain });
        Ok(())
    })
}

/// Get per-domain stats.
pub fn domain_stats() -> Vec<DomainStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.domains.clone())
}

/// Recent wake events.
pub fn wake_log(n: usize) -> Vec<WakeEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.wake_log.len() { 0 } else { s.wake_log.len() - n };
        s.wake_log[start..].to_vec()
    })
}

/// Statistics: (domain_count, total_energy_uj, total_transitions, total_wakes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.domains.len(), s.total_energy_uj, s.total_transitions, s.total_wakes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("powerstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/powerstat must never surface).
    // Resetting first clears any residue from a prior `powerstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated domains, wakes, or totals.
    assert_eq!(domain_stats().len(), 0);
    assert_eq!(wake_log(10).len(), 0);
    let (d0, e0, t0, w0, _o0) = stats();
    assert_eq!((d0, e0, t0, w0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register domains — zeroed counters, given initial state; dup fails.
    register_domain(PowerDomain::Cpu, PowerState::Active).expect("reg cpu");
    register_domain(PowerDomain::Gpu, PowerState::Idle).expect("reg gpu");
    assert!(register_domain(PowerDomain::Cpu, PowerState::Off).is_err()); // AlreadyExists
    assert_eq!(domain_stats().len(), 2);
    let cpu = domain_stats().iter().find(|d| d.domain == PowerDomain::Cpu).cloned().expect("cpu");
    assert_eq!(cpu.current_state, PowerState::Active);
    assert_eq!((cpu.energy_uj, cpu.transitions, cpu.active_time_ns), (0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Transition updates state + transition count exactly from zero.
    record_transition(PowerDomain::Cpu, PowerState::Idle).expect("transition");
    let cpu = domain_stats().iter().find(|d| d.domain == PowerDomain::Cpu).cloned().expect("cpu");
    assert_eq!(cpu.current_state, PowerState::Idle);
    assert_eq!(cpu.transitions, 1);
    crate::serial_println!("  [3/8] transition: OK");

    // 4: Second transition counts again; state follows.
    record_transition(PowerDomain::Cpu, PowerState::Active).expect("transition2");
    let cpu = domain_stats().iter().find(|d| d.domain == PowerDomain::Cpu).cloned().expect("cpu");
    assert_eq!(cpu.current_state, PowerState::Active);
    assert_eq!(cpu.transitions, 2);
    crate::serial_println!("  [4/8] back to active: OK");

    // 5: Energy accumulates exactly from zero.
    update_energy(PowerDomain::Gpu, 1000).expect("energy");
    update_energy(PowerDomain::Gpu, 500).expect("energy2");
    let gpu = domain_stats().iter().find(|d| d.domain == PowerDomain::Gpu).cloned().expect("gpu");
    assert_eq!(gpu.energy_uj, 1500);
    crate::serial_println!("  [5/8] energy: OK");

    // 6: Wake events are logged in order, count tracked.
    record_wake(WakeSource::Timer, PowerDomain::Cpu).expect("wake");
    record_wake(WakeSource::UserInput, PowerDomain::Gpu).expect("wake2");
    let log = wake_log(10);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].source, WakeSource::Timer);
    assert_eq!(log[1].source, WakeSource::UserInput);
    crate::serial_println!("  [6/8] wakes: OK");

    // 7: Unregistered domain → NotFound.
    assert!(record_transition(PowerDomain::Audio, PowerState::Off).is_err());
    assert!(update_energy(PowerDomain::Audio, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (domains, energy, transitions, wakes, ops) = stats();
    assert_eq!(domains, 2);
    assert_eq!(energy, 1500);     // 1000 + 500 on gpu
    assert_eq!(transitions, 2);   // 2 cpu transitions
    assert_eq!(wakes, 2);         // 2 wake events
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/powerstat table.
    *STATE.lock() = None;

    crate::serial_println!("powerstat::self_test() — all 8 tests passed");
}
