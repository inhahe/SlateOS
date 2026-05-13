//! Network Filter — packet filtering and firewall statistics.
//!
//! Tracks firewall rules, packet filtering decisions, NAT
//! translations, and connection tracking. Provides monitoring
//! for the kernel's network security subsystem.
//!
//! ## Architecture
//!
//! ```text
//! Network filter statistics
//!   → netfilter::add_rule(chain, rule) → add filter rule
//!   → netfilter::remove_rule(chain, id) → remove rule
//!   → netfilter::record_match(chain, action) → track match
//!   → netfilter::conntrack_stats() → connection tracking
//!
//! Integration:
//!   → netmon (network monitor)
//!   → audit (audit logging)
//!   → secpolicy (security policy)
//!   → telemetry (system telemetry)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Filter chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Input,
    Output,
    Forward,
    PreRouting,
    PostRouting,
}

impl Chain {
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "INPUT",
            Self::Output => "OUTPUT",
            Self::Forward => "FORWARD",
            Self::PreRouting => "PREROUTING",
            Self::PostRouting => "POSTROUTING",
        }
    }
}

/// Filter action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Drop,
    Reject,
    Log,
    Nat,
    Redirect,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Drop => "DROP",
            Self::Reject => "REJECT",
            Self::Log => "LOG",
            Self::Nat => "NAT",
            Self::Redirect => "REDIRECT",
        }
    }
}

/// A filter rule.
#[derive(Debug, Clone)]
pub struct FilterRule {
    pub id: u32,
    pub chain: Chain,
    pub action: Action,
    pub description: String,
    pub matches: u64,
    pub bytes_matched: u64,
    pub enabled: bool,
}

/// Connection tracking entry.
#[derive(Debug, Clone)]
pub struct ConnTrackEntry {
    pub src_ip: u32,
    pub dst_ip: u32,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub packets: u64,
    pub bytes: u64,
    pub state: ConnState,
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    New,
    Established,
    Related,
    Invalid,
    TimeWait,
}

impl ConnState {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::Established => "ESTABLISHED",
            Self::Related => "RELATED",
            Self::Invalid => "INVALID",
            Self::TimeWait => "TIME_WAIT",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 256;
const MAX_CONNTRACK: usize = 1024;

struct State {
    rules: Vec<FilterRule>,
    conntrack: Vec<ConnTrackEntry>,
    next_rule_id: u32,
    total_packets: u64,
    total_accepted: u64,
    total_dropped: u64,
    total_rejected: u64,
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
        rules: alloc::vec![
            FilterRule { id: 1, chain: Chain::Input, action: Action::Accept, description: String::from("allow established"), matches: 5_000_000, bytes_matched: 10_000_000_000, enabled: true },
            FilterRule { id: 2, chain: Chain::Input, action: Action::Accept, description: String::from("allow ssh"), matches: 50000, bytes_matched: 100_000_000, enabled: true },
            FilterRule { id: 3, chain: Chain::Input, action: Action::Drop, description: String::from("default deny"), matches: 100000, bytes_matched: 50_000_000, enabled: true },
            FilterRule { id: 4, chain: Chain::Output, action: Action::Accept, description: String::from("allow all out"), matches: 4_000_000, bytes_matched: 8_000_000_000, enabled: true },
        ],
        conntrack: alloc::vec![
            ConnTrackEntry { src_ip: 0xC0A80001, dst_ip: 0xC0A80064, src_port: 12345, dst_port: 22, protocol: 6, packets: 500, bytes: 50000, state: ConnState::Established },
            ConnTrackEntry { src_ip: 0xC0A80001, dst_ip: 0x08080808, src_port: 54321, dst_port: 443, protocol: 6, packets: 2000, bytes: 500000, state: ConnState::Established },
        ],
        next_rule_id: 5,
        total_packets: 9_150_000,
        total_accepted: 9_050_000,
        total_dropped: 100000,
        total_rejected: 0,
        ops: 0,
    });
}

/// Add a filter rule.
pub fn add_rule(chain: Chain, action: Action, description: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_rule_id;
        state.next_rule_id += 1;
        state.rules.push(FilterRule {
            id, chain, action, description: String::from(description),
            matches: 0, bytes_matched: 0, enabled: true,
        });
        Ok(id)
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.rules.iter().position(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        state.rules.remove(idx);
        Ok(())
    })
}

/// Record a filter match.
pub fn record_match(chain: Chain, action: Action, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.total_packets += 1;
        match action {
            Action::Accept | Action::Nat | Action::Redirect => state.total_accepted += 1,
            Action::Drop => state.total_dropped += 1,
            Action::Reject => state.total_rejected += 1,
            Action::Log => {}
        }
        // Update first matching enabled rule in that chain with that action.
        if let Some(rule) = state.rules.iter_mut()
            .find(|r| r.chain == chain && r.action == action && r.enabled) {
            rule.matches += 1;
            rule.bytes_matched += bytes;
        }
        Ok(())
    })
}

/// Toggle a rule's enabled state.
pub fn toggle_rule(id: u32) -> KernelResult<bool> {
    with_state(|state| {
        let rule = state.rules.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        rule.enabled = !rule.enabled;
        Ok(rule.enabled)
    })
}

/// List all rules.
pub fn list_rules() -> Vec<FilterRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// List rules for a specific chain.
pub fn rules_for_chain(chain: Chain) -> Vec<FilterRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.rules.iter().filter(|r| r.chain == chain).cloned().collect()
    })
}

/// Connection tracking entries.
pub fn conntrack() -> Vec<ConnTrackEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.conntrack.clone())
}

/// Statistics: (rule_count, conntrack_count, total_packets, total_accepted, total_dropped, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.conntrack.len(), s.total_packets, s.total_accepted, s.total_dropped, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netfilter::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_rules().len(), 4);
    assert_eq!(conntrack().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add rule.
    let id = add_rule(Chain::Forward, Action::Accept, "allow forward").expect("add");
    assert!(id >= 5);
    assert_eq!(list_rules().len(), 5);
    crate::serial_println!("  [2/8] add rule: OK");

    // 3: Remove rule.
    remove_rule(id).expect("remove");
    assert_eq!(list_rules().len(), 4);
    assert!(remove_rule(id).is_err());
    crate::serial_println!("  [3/8] remove rule: OK");

    // 4: Record match.
    let before = list_rules()[0].matches;
    record_match(Chain::Input, Action::Accept, 1500).expect("match");
    let after = list_rules()[0].matches;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] record match: OK");

    // 5: Toggle.
    let enabled = toggle_rule(1).expect("toggle");
    assert!(!enabled);
    let enabled2 = toggle_rule(1).expect("toggle2");
    assert!(enabled2);
    crate::serial_println!("  [5/8] toggle: OK");

    // 6: Chain filter.
    let input_rules = rules_for_chain(Chain::Input);
    assert!(input_rules.len() >= 3);
    crate::serial_println!("  [6/8] chain filter: OK");

    // 7: Drop stats.
    record_match(Chain::Input, Action::Drop, 64).expect("drop");
    let (_, _, _, _, dropped, _) = stats();
    assert!(dropped > 100000);
    crate::serial_println!("  [7/8] drop stats: OK");

    // 8: Stats.
    let (rules, ct, packets, accepted, _dropped, ops) = stats();
    assert_eq!(rules, 4);
    assert_eq!(ct, 2);
    assert!(packets > 9_150_000);
    assert!(accepted > 9_050_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netfilter::self_test() — all 8 tests passed");
}
