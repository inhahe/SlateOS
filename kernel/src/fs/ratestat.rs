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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        limiters: alloc::vec![
            LimiterStats { name: String::from("printk"), rate_per_sec: 10, burst_size: 50, current_tokens: 50, allows: 1_000_000, denies: 500_000, burst_events: 10_000 },
            LimiterStats { name: String::from("net_icmp"), rate_per_sec: 100, burst_size: 200, current_tokens: 200, allows: 5_000_000, denies: 100_000, burst_events: 500 },
            LimiterStats { name: String::from("auth_fail"), rate_per_sec: 5, burst_size: 10, current_tokens: 10, allows: 50_000, denies: 200_000, burst_events: 20_000 },
        ],
        total_allows: 6_050_000,
        total_denies: 800_000,
        total_bursts: 30_500,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_limiter().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("test_rl", 10, 5).expect("register");
    assert_eq!(per_limiter().len(), 4);
    assert!(register("test_rl", 10, 5).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Allow.
    record_allow("test_rl").expect("allow");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().unwrap();
    assert_eq!(l.allows, 1);
    assert_eq!(l.current_tokens, 4);
    crate::serial_println!("  [3/8] allow: OK");

    // 4: Deny.
    record_deny("test_rl").expect("deny");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().unwrap();
    assert_eq!(l.denies, 1);
    crate::serial_println!("  [4/8] deny: OK");

    // 5: Burst detection.
    for _ in 0..4 { record_allow("test_rl").expect("allow_drain"); }
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().unwrap();
    assert_eq!(l.current_tokens, 0);
    assert!(l.burst_events >= 1);
    crate::serial_println!("  [5/8] burst: OK");

    // 6: Refill.
    refill("test_rl", 3).expect("refill");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().unwrap();
    assert_eq!(l.current_tokens, 3);
    crate::serial_println!("  [6/8] refill: OK");

    // 7: Refill cap.
    refill("test_rl", 100).expect("refill_cap");
    let l = per_limiter().iter().find(|l| l.name == "test_rl").cloned().unwrap();
    assert_eq!(l.current_tokens, 5); // capped at burst_size
    crate::serial_println!("  [7/8] refill cap: OK");

    // 8: Stats.
    let (limiters, allows, denies, bursts, ops) = stats();
    assert!(limiters >= 4);
    assert!(allows > 6_050_000);
    assert!(denies > 800_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ratestat::self_test() — all 8 tests passed");
}
