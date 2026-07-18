//! Power Wake — Wake-on-LAN and scheduled wake management.
//!
//! Manages remote and scheduled system wake events including
//! Wake-on-LAN (WoL), scheduled wake timers, and wake history.
//!
//! ## Architecture
//!
//! ```text
//! Wake management
//!   → powerwake::schedule(time, reason) → schedule wake timer
//!   → powerwake::send_wol(mac) → send WoL magic packet
//!   → powerwake::history() → view wake event log
//!
//! Integration:
//!   → power (power management)
//!   → tasksched (scheduled tasks)
//!   → wakesensor (wake sensors)
//!   → netsettings (network config)
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

/// Wake source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeSource {
    Manual,
    Scheduled,
    WakeOnLan,
    UsbDevice,
    Keyboard,
    PowerButton,
    RtcAlarm,
}

impl WakeSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Scheduled => "Scheduled",
            Self::WakeOnLan => "Wake-on-LAN",
            Self::UsbDevice => "USB Device",
            Self::Keyboard => "Keyboard",
            Self::PowerButton => "Power Button",
            Self::RtcAlarm => "RTC Alarm",
        }
    }
}

/// A scheduled wake timer.
#[derive(Debug, Clone)]
pub struct WakeTimer {
    pub id: u32,
    pub wake_at_ns: u64,
    pub reason: String,
    pub enabled: bool,
    pub recurring: bool,
    pub interval_ns: u64,
    pub created_ns: u64,
}

/// A wake event in history.
#[derive(Debug, Clone)]
pub struct WakeEvent {
    pub timestamp_ns: u64,
    pub source: WakeSource,
    pub detail: String,
}

/// WoL target entry.
#[derive(Debug, Clone)]
pub struct WolTarget {
    pub id: u32,
    pub name: String,
    pub mac_address: String,
    pub last_sent_ns: u64,
    pub send_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TIMERS: usize = 100;
const MAX_HISTORY: usize = 500;
const MAX_WOL_TARGETS: usize = 50;

struct State {
    timers: Vec<WakeTimer>,
    history: Vec<WakeEvent>,
    wol_targets: Vec<WolTarget>,
    next_timer_id: u32,
    next_wol_id: u32,
    total_wakes: u64,
    total_wol_sent: u64,
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
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        timers: Vec::new(),
        history: alloc::vec![
            WakeEvent {
                timestamp_ns: now,
                source: WakeSource::PowerButton,
                detail: String::from("System boot"),
            },
        ],
        wol_targets: Vec::new(),
        next_timer_id: 1,
        next_wol_id: 1,
        total_wakes: 1,
        total_wol_sent: 0,
        ops: 0,
    });
}

/// Schedule a wake timer.
pub fn schedule_wake(wake_at_ns: u64, reason: &str, recurring: bool, interval_ns: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.timers.len() >= MAX_TIMERS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_timer_id;
        state.next_timer_id += 1;
        state.timers.push(WakeTimer {
            id, wake_at_ns, reason: String::from(reason),
            enabled: true, recurring, interval_ns, created_ns: now,
        });
        Ok(id)
    })
}

/// Cancel a wake timer.
pub fn cancel_timer(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.timers.len();
        state.timers.retain(|t| t.id != id);
        if state.timers.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a timer.
pub fn set_timer_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let timer = state.timers.iter_mut().find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        timer.enabled = enabled;
        Ok(())
    })
}

/// Add a WoL target.
pub fn add_wol_target(name: &str, mac: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.wol_targets.len() >= MAX_WOL_TARGETS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_wol_id;
        state.next_wol_id += 1;
        state.wol_targets.push(WolTarget {
            id, name: String::from(name), mac_address: String::from(mac),
            last_sent_ns: 0, send_count: 0,
        });
        Ok(id)
    })
}

/// Remove a WoL target.
pub fn remove_wol_target(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.wol_targets.len();
        state.wol_targets.retain(|w| w.id != id);
        if state.wol_targets.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Send WoL to a target (simulated).
pub fn send_wol(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let target = state.wol_targets.iter_mut().find(|w| w.id == id)
            .ok_or(KernelError::NotFound)?;
        target.last_sent_ns = now;
        target.send_count += 1;
        state.total_wol_sent += 1;
        Ok(())
    })
}

/// Record a wake event.
pub fn record_wake(source: WakeSource, detail: &str) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(WakeEvent {
            timestamp_ns: now, source, detail: String::from(detail),
        });
        state.total_wakes += 1;
        Ok(())
    })
}

/// List active timers.
pub fn list_timers() -> Vec<WakeTimer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.timers.clone())
}

/// List WoL targets.
pub fn list_wol_targets() -> Vec<WolTarget> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.wol_targets.clone())
}

/// Get wake history.
pub fn wake_history() -> Vec<WakeEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Statistics: (timer_count, wol_target_count, total_wakes, total_wol_sent, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.timers.len(), s.wol_targets.len(), s.total_wakes, s.total_wol_sent, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("powerwake::self_test() — running tests...");
    init_defaults();

    // 1: Default history.
    assert_eq!(wake_history().len(), 1);
    assert_eq!(wake_history()[0].source, WakeSource::PowerButton);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Schedule timer.
    let tid = schedule_wake(999_999_999, "backup", false, 0).expect("schedule");
    assert_eq!(list_timers().len(), 1);
    crate::serial_println!("  [2/8] schedule: OK");

    // 3: Enable/disable timer.
    set_timer_enabled(tid, false).expect("disable");
    let t = list_timers().iter().find(|t| t.id == tid).cloned().expect("find");
    assert!(!t.enabled);
    set_timer_enabled(tid, true).expect("enable");
    crate::serial_println!("  [3/8] timer toggle: OK");

    // 4: Cancel timer.
    cancel_timer(tid).expect("cancel");
    assert!(list_timers().is_empty());
    assert!(cancel_timer(tid).is_err());
    crate::serial_println!("  [4/8] cancel: OK");

    // 5: Add WoL target.
    let wid = add_wol_target("server", "AA:BB:CC:DD:EE:FF").expect("add_wol");
    assert_eq!(list_wol_targets().len(), 1);
    crate::serial_println!("  [5/8] add wol: OK");

    // 6: Send WoL.
    send_wol(wid).expect("send_wol");
    let w = list_wol_targets().iter().find(|w| w.id == wid).cloned().expect("find");
    assert_eq!(w.send_count, 1);
    crate::serial_println!("  [6/8] send wol: OK");

    // 7: Record wake + remove wol target.
    record_wake(WakeSource::WakeOnLan, "from server").expect("record");
    assert!(wake_history().len() >= 2);
    remove_wol_target(wid).expect("remove_wol");
    assert!(list_wol_targets().is_empty());
    crate::serial_println!("  [7/8] record/remove: OK");

    // 8: Stats.
    let (timers, wol_targets, total_wakes, total_wol_sent, ops) = stats();
    assert_eq!(timers, 0);
    assert_eq!(wol_targets, 0);
    assert!(total_wakes >= 2);
    assert!(total_wol_sent >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("powerwake::self_test() — all 8 tests passed");
}
