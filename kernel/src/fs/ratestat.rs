//! Rate Limiting Statistics — kernel rate limiter monitoring.
//!
//! Tracks rate limiting decisions across subsystems (printk,
//! network, syscall filtering). Records allow/deny counts,
//! current bucket levels, and burst events.
//!
//! ## Architecture
//!
//! ```text
//! Rate limiting monitoring
//!   → ratestat::register(name, rate, burst) → register limiter
//!   → ratestat::record_allow(name) → allowed through
//!   → ratestat::record_deny(name) → rate limited
//!   → ratestat::per_limiter() → per-limiter stats
//!
//! Integration:
//!   → kernlog (kernel logging)
//!   → netfilter (network filtering)
//!   → secmod (security module)
//!   → syslog (system logging)
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

/// Per-limiter stats.
#[derive(Debug, Clone)]
pub struct LimiterStats {
    pub name: String,
    pub rate_per_sec: u32,     // Tokens per second
    pub burst_size: u32,       // Max burst
    pub current_tokens: u32,   // Current token count
    pub allows: u64,
    pub denies: u64,
    pub burst_events: u64,     // Times burst was fully consumed
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_LIMITERS: usize = 128;

struct State {
    limiters: Vec<LimiterStats>,
    total_allows: u64,
    total_denies: u64,
    total_bursts: u64,
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

/// Initialise an **empty** rate-limiter statistics table.
///
/// Seeds NO limiters and zero counters.  Real rate-limiter accounting is wired
/// through [`register`] (one row per limiter a subsystem creates, with its real
/// rate/burst configuration) and the `record_allow`/`record_deny`/`refill`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/ratestat` and the `ratestat` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded three fictional limiters ("printk": rate 10 /
/// burst 50 / allows 1M / denies 500k / 10k burst events; "net_icmp": rate 100 /
/// burst 200 / allows 5M / denies 100k; "auth_fail": rate 5 / burst 10 / allows
/// 50k / denies 200k / 20k burst events) plus invented aggregate totals
/// (total_allows 6.05M, total_denies 800k, total_bursts 30.5k), which
/// `/proc/ratestat` then displayed as if they were real measured rate-limiting
/// decisions.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  Each subsystem
/// (printk, netfilter, auth, ...) is expected to call [`register`] for its
/// limiter and the record functions on every allow/deny decision.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        limiters: Vec::new(),
        total_allows: 0,
        total_denies: 0,
        total_bursts: 0,
        ops: 0,
    });
}

/// Register a rate limiter.
pub fn register(name: &str, rate_per_sec: u32, burst_size: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.limiters.len() >= MAX_LIMITERS { return Err(KernelError::ResourceExhausted); }
        if state.limiters.iter().any(|l| l.name == name) { return Err(KernelError::AlreadyExists); }
        state.limiters.push(LimiterStats {
            name: String::from(name), rate_per_sec, burst_size,
            current_tokens: burst_size, allows: 0, denies: 0, burst_events: 0,
        });
        Ok(())
    })
}

/// Record an allow.
pub fn record_allow(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let l = state.limiters.iter_mut().find(|l| l.name == name)
            .ok_or(KernelError::NotFound)?;
        l.allows += 1;
        if l.current_tokens > 0 {
            l.current_tokens -= 1;
        }
        if l.current_tokens == 0 {
            l.burst_events += 1;
            state.total_bursts += 1;
        }
        state.total_allows += 1;
        Ok(())
    })
}

/// Record a deny.
pub fn record_deny(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let l = state.limiters.iter_mut().find(|l| l.name == name)
            .ok_or(KernelError::NotFound)?;
        l.denies += 1;
        state.total_denies += 1;
        Ok(())
    })
}

/// Refill tokens (simulates time passing).
pub fn refill(name: &str, tokens: u32) -> KernelResult<()> {
    with_state(|state| {
        let l = state.limiters.iter_mut().find(|l| l.name == name)
            .ok_or(KernelError::NotFound)?;
        l.current_tokens = (l.current_tokens + tokens).min(l.burst_size);
        Ok(())
    })
}

/// Per-limiter stats.
pub fn per_limiter() -> Vec<LimiterStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.limiters.clone())
}

/// Statistics: (limiter_count, total_allows, total_denies, total_bursts, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.limiters.len(), s.total_allows, s.total_denies, s.total_bursts, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ratestat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/ratestat must never surface).  Resetting
    // first clears any residue from a prior `ratestat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated limiters or counters.
    assert_eq!(per_limiter().len(), 0);
    let (c0, a0, d0, b0, _o0) = stats();
    assert_eq!((c0, a0, d0, b0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — current_tokens seeded to burst_size; dup name fails.
    register("test_rl", 10, 5).expect("register");
    assert_eq!(per_limiter().len(), 1);
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.rate_per_sec, 10);
    assert_eq!(l.burst_size, 5);
    assert_eq!(l.current_tokens, 5);
    assert_eq!((l.allows, l.denies, l.burst_events), (0, 0, 0));
    assert!(register("test_rl", 10, 5).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Allow — count up, one token consumed.
    record_allow("test_rl").expect("allow");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.allows, 1);
    assert_eq!(l.current_tokens, 4);
    assert_eq!(l.burst_events, 0);
    crate::serial_println!("  [3/8] allow: OK");

    // 4: Deny — count up, tokens unchanged.
    record_deny("test_rl").expect("deny");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.denies, 1);
    assert_eq!(l.current_tokens, 4);
    crate::serial_println!("  [4/8] deny: OK");

    // 5: Burst detection — draining the last 4 tokens reaches 0 exactly once,
    // bumping burst_events once (only the allow that lands on 0 counts).
    for _ in 0..4 { record_allow("test_rl").expect("allow_drain"); }
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.current_tokens, 0);
    assert_eq!(l.allows, 5);
    assert_eq!(l.burst_events, 1);
    crate::serial_println!("  [5/8] burst: OK");

    // 6: Refill adds tokens without exceeding burst_size.
    refill("test_rl", 3).expect("refill");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.current_tokens, 3);
    crate::serial_println!("  [6/8] refill: OK");

    // 7: Refill caps at burst_size; unknown limiter → NotFound.
    refill("test_rl", 100).expect("refill_cap");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().expect("find");
    assert_eq!(l.current_tokens, 5); // capped at burst_size
    assert!(record_allow("missing").is_err());
    assert!(record_deny("missing").is_err());
    assert!(refill("missing", 1).is_err());
    crate::serial_println!("  [7/8] refill cap + not found: OK");

    // 8: Aggregate stats are exact: 5 allows, 1 deny, 1 burst event.
    let (limiters, allows, denies, bursts, ops) = stats();
    assert_eq!(limiters, 1);
    assert_eq!(allows, 5);
    assert_eq!(denies, 1);
    assert_eq!(bursts, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/ratestat table.
    *STATE.lock() = None;

    crate::serial_println!("ratestat::self_test() — all 8 tests passed");
}
