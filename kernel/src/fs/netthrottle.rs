//! Network throttle — per-application bandwidth management.
//!
//! Controls network bandwidth allocation per application and
//! per network interface. Supports QoS priorities, rate limiting,
//! and bandwidth reservation for critical services.
//!
//! ## Architecture
//!
//! ```text
//! App sends/receives data
//!   → netthrottle::check_limit(app_id, bytes) → allow/throttle
//!
//! Settings panel → Network → Bandwidth
//!   → netthrottle::set_limit(app, rate) → per-app limit
//!   → netthrottle::set_priority(app, level) → QoS priority
//!
//! Integration:
//!   → netsettings (interface configuration)
//!   → datausage (usage tracking)
//!   → appregistry (app identification)
//!   → notifcenter (throttle alerts)
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

/// QoS priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosPriority {
    Critical,
    High,
    Normal,
    Low,
    Background,
}

impl QosPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
            Self::Background => "Background",
        }
    }
}

/// Throttle state for an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleState {
    Normal,
    Throttled,
    Blocked,
}

impl ThrottleState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Throttled => "Throttled",
            Self::Blocked => "Blocked",
        }
    }
}

/// A per-app bandwidth rule.
#[derive(Debug, Clone)]
pub struct BandwidthRule {
    /// Rule ID.
    pub id: u32,
    /// Application name (or "*" for global).
    pub app_name: String,
    /// Max download rate in bytes/sec (0 = unlimited).
    pub max_down_bps: u64,
    /// Max upload rate in bytes/sec (0 = unlimited).
    pub max_up_bps: u64,
    /// QoS priority.
    pub priority: QosPriority,
    /// Current state.
    pub state: ThrottleState,
    /// Bytes downloaded since rule creation.
    pub bytes_down: u64,
    /// Bytes uploaded since rule creation.
    pub bytes_up: u64,
    /// Whether rule is enabled.
    pub enabled: bool,
}

/// Global bandwidth settings.
#[derive(Debug, Clone)]
pub struct GlobalSettings {
    /// Global max download (0 = unlimited).
    pub max_down_bps: u64,
    /// Global max upload (0 = unlimited).
    pub max_up_bps: u64,
    /// Whether throttling is enabled.
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 100;

struct State {
    rules: Vec<BandwidthRule>,
    next_id: u32,
    global: GlobalSettings,
    total_throttled: u64,
    total_blocked: u64,
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
        rules: Vec::new(),
        next_id: 1,
        global: GlobalSettings {
            max_down_bps: 0,
            max_up_bps: 0,
            enabled: true,
        },
        total_throttled: 0,
        total_blocked: 0,
        ops: 0,
    });
}

/// Add a bandwidth rule for an application.
pub fn add_rule(app_name: &str, max_down_bps: u64, max_up_bps: u64, priority: QosPriority) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(BandwidthRule {
            id, app_name: String::from(app_name),
            max_down_bps, max_up_bps, priority,
            state: ThrottleState::Normal,
            bytes_down: 0, bytes_up: 0, enabled: true,
        });
        Ok(id)
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.rules.iter().position(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        state.rules.remove(pos);
        Ok(())
    })
}

/// Set rule limits.
pub fn set_limits(id: u32, max_down: u64, max_up: u64) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.max_down_bps = max_down;
        rule.max_up_bps = max_up;
        Ok(())
    })
}

/// Set rule priority.
pub fn set_priority(id: u32, priority: QosPriority) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.priority = priority;
        Ok(())
    })
}

/// Enable/disable a rule.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = enabled;
        Ok(())
    })
}

/// Record traffic and check throttle state.
pub fn record_traffic(app_name: &str, bytes_down: u64, bytes_up: u64) -> KernelResult<ThrottleState> {
    with_state(|state| {
        if !state.global.enabled {
            return Ok(ThrottleState::Normal);
        }
        if let Some(rule) = state.rules.iter_mut().find(|r| r.app_name == app_name && r.enabled) {
            rule.bytes_down += bytes_down;
            rule.bytes_up += bytes_up;

            // Simple check: if limits are set and exceeded, throttle.
            let over_down = rule.max_down_bps > 0 && bytes_down > rule.max_down_bps;
            let over_up = rule.max_up_bps > 0 && bytes_up > rule.max_up_bps;

            if over_down || over_up {
                if rule.priority == QosPriority::Background {
                    rule.state = ThrottleState::Blocked;
                    state.total_blocked += 1;
                    return Ok(ThrottleState::Blocked);
                }
                rule.state = ThrottleState::Throttled;
                state.total_throttled += 1;
                return Ok(ThrottleState::Throttled);
            }
            rule.state = ThrottleState::Normal;
            Ok(ThrottleState::Normal)
        } else {
            Ok(ThrottleState::Normal)
        }
    })
}

/// Set global bandwidth limits.
pub fn set_global_limits(max_down: u64, max_up: u64) -> KernelResult<()> {
    with_state(|state| {
        state.global.max_down_bps = max_down;
        state.global.max_up_bps = max_up;
        Ok(())
    })
}

/// Enable/disable global throttling.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.global.enabled = enabled; Ok(()) })
}

/// Get rule by ID.
pub fn get_rule(id: u32) -> KernelResult<BandwidthRule> {
    with_state(|state| {
        state.rules.iter().find(|r| r.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all rules.
pub fn list_rules() -> Vec<BandwidthRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Statistics: (rule_count, total_throttled, total_blocked, enabled, ops).
pub fn stats() -> (usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.total_throttled, s.total_blocked, s.global.enabled, s.ops),
        None => (0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netthrottle::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_rules().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Add rule.
    let id1 = add_rule("browser", 1_000_000, 500_000, QosPriority::Normal).expect("add");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] add rule: OK");

    // 3: Get rule.
    let r = get_rule(id1).expect("get");
    assert_eq!(r.app_name, "browser");
    assert_eq!(r.max_down_bps, 1_000_000);
    crate::serial_println!("  [3/11] get rule: OK");

    // 4: Normal traffic.
    let state = record_traffic("browser", 100, 50).expect("traffic");
    assert_eq!(state, ThrottleState::Normal);
    crate::serial_println!("  [4/11] normal traffic: OK");

    // 5: Throttled traffic.
    let state = record_traffic("browser", 2_000_000, 0).expect("traffic2");
    assert_eq!(state, ThrottleState::Throttled);
    crate::serial_println!("  [5/11] throttled: OK");

    // 6: Background blocked.
    let id2 = add_rule("torrent", 100_000, 50_000, QosPriority::Background).expect("add2");
    let state = record_traffic("torrent", 200_000, 0).expect("traffic3");
    assert_eq!(state, ThrottleState::Blocked);
    crate::serial_println!("  [6/11] background blocked: OK");

    // 7: Set priority.
    set_priority(id1, QosPriority::High).expect("prio");
    let r = get_rule(id1).expect("get2");
    assert_eq!(r.priority, QosPriority::High);
    crate::serial_println!("  [7/11] set priority: OK");

    // 8: Disable rule.
    set_enabled(id2, false).expect("disable");
    let state = record_traffic("torrent", 200_000, 0).expect("traffic4");
    assert_eq!(state, ThrottleState::Normal); // Rule disabled.
    crate::serial_println!("  [8/11] disable rule: OK");

    // 9: Remove rule.
    remove_rule(id2).expect("remove");
    assert_eq!(list_rules().len(), 1);
    crate::serial_println!("  [9/11] remove rule: OK");

    // 10: Global disable.
    set_global_enabled(false).expect("global");
    let state = record_traffic("browser", 9_999_999, 0).expect("traffic5");
    assert_eq!(state, ThrottleState::Normal);
    set_global_enabled(true).expect("global2");
    crate::serial_println!("  [10/11] global disable: OK");

    // 11: Stats.
    let (count, throttled, blocked, enabled, ops) = stats();
    assert_eq!(count, 1);
    assert!(throttled >= 1);
    assert!(blocked >= 1);
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("netthrottle::self_test() — all 11 tests passed");
}
