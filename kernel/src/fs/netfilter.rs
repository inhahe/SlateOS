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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the netfilter statistics state.
///
/// Starts empty: no firewall rules, no connection-tracking entries, and
/// all packet/accept/drop/reject counters at zero. The `/proc/netfilter`
/// generator and the `netfilter` kshell command surface this table as if
/// it reflects real kernel firewall activity, so seeding it with invented
/// rows would be fabricated procfs data. Rules are registered through
/// [`add_rule`], connection-tracking entries through [`track_connection`],
/// and the counters are advanced only by real [`record_match`] calls.
///
/// (Previously this seeded 4 fictional rules — "allow established"
/// 5M matches/10GB, "allow ssh", "default deny", "allow all out"
/// 4M matches/8GB — plus 2 fabricated conntrack entries
/// (192.168.0.1→…:22 and 192.168.0.1→8.8.8.8:443) and invented totals
/// (9.15M packets, 9.05M accepted, 100k dropped).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        rules: Vec::new(),
        conntrack: Vec::new(),
        next_rule_id: 1,
        total_packets: 0,
        total_accepted: 0,
        total_dropped: 0,
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

/// Begin tracking a connection.
///
/// Registers a new connection-tracking entry for the given 5-tuple in the
/// `New` state with zeroed packet/byte counters. If an entry for the same
/// 5-tuple already exists it is returned unchanged (idempotent). Real
/// traffic is accounted via [`update_connection`], and the connection's
/// lifecycle state is advanced via [`set_conn_state`]. Returns
/// `ResourceExhausted` once the conntrack table is full.
pub fn track_connection(
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    protocol: u8,
) -> KernelResult<()> {
    with_state(|state| {
        if state.conntrack.iter().any(|c| {
            c.src_ip == src_ip && c.dst_ip == dst_ip
                && c.src_port == src_port && c.dst_port == dst_port
                && c.protocol == protocol
        }) {
            return Ok(());
        }
        if state.conntrack.len() >= MAX_CONNTRACK {
            return Err(KernelError::ResourceExhausted);
        }
        state.conntrack.push(ConnTrackEntry {
            src_ip, dst_ip, src_port, dst_port, protocol,
            packets: 0, bytes: 0, state: ConnState::New,
        });
        Ok(())
    })
}

/// Account real traffic against a tracked connection.
///
/// Adds `packets` and `bytes` to the matching 5-tuple's counters. Returns
/// `NotFound` if the connection is not being tracked.
pub fn update_connection(
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    protocol: u8,
    packets: u64,
    bytes: u64,
) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.conntrack.iter_mut().find(|c| {
            c.src_ip == src_ip && c.dst_ip == dst_ip
                && c.src_port == src_port && c.dst_port == dst_port
                && c.protocol == protocol
        }).ok_or(KernelError::NotFound)?;
        entry.packets = entry.packets.saturating_add(packets);
        entry.bytes = entry.bytes.saturating_add(bytes);
        Ok(())
    })
}

/// Advance the lifecycle state of a tracked connection.
///
/// Returns `NotFound` if the connection is not being tracked.
pub fn set_conn_state(
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    protocol: u8,
    new_state: ConnState,
) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.conntrack.iter_mut().find(|c| {
            c.src_ip == src_ip && c.dst_ip == dst_ip
                && c.src_port == src_port && c.dst_port == dst_port
                && c.protocol == protocol
        }).ok_or(KernelError::NotFound)?;
        entry.state = new_state;
        Ok(())
    })
}

/// Stop tracking a connection (e.g. on close/timeout).
///
/// Returns `NotFound` if the connection is not being tracked.
pub fn untrack_connection(
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    protocol: u8,
) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.conntrack.iter().position(|c| {
            c.src_ip == src_ip && c.dst_ip == dst_ip
                && c.src_port == src_port && c.dst_port == dst_port
                && c.protocol == protocol
        }).ok_or(KernelError::NotFound)?;
        state.conntrack.remove(idx);
        Ok(())
    })
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live /proc/netfilter table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated rules, conntrack, or counters.
    assert_eq!(list_rules().len(), 0);
    assert_eq!(conntrack().len(), 0);
    let (rules0, ct0, packets0, accepted0, dropped0, _) = stats();
    assert_eq!((rules0, ct0, packets0, accepted0, dropped0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Add rules — ids are monotonic starting at 1.
    let r_in = add_rule(Chain::Input, Action::Accept, "allow ssh").expect("add in");
    let r_drop = add_rule(Chain::Input, Action::Drop, "default deny").expect("add drop");
    let r_fwd = add_rule(Chain::Forward, Action::Accept, "allow forward").expect("add fwd");
    assert_eq!((r_in, r_drop, r_fwd), (1, 2, 3));
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [2/8] add rules: OK");

    // 3: Remove rule — gone, and double-remove errors.
    remove_rule(r_fwd).expect("remove");
    assert_eq!(list_rules().len(), 2);
    assert!(remove_rule(r_fwd).is_err());
    crate::serial_println!("  [3/8] remove rule: OK");

    // 4: Record match routes to the matching enabled rule and the totals.
    record_match(Chain::Input, Action::Accept, 1500).expect("match accept");
    record_match(Chain::Input, Action::Drop, 64).expect("match drop");
    let accept_rule = list_rules().into_iter().find(|r| r.id == r_in).expect("rule");
    assert_eq!(accept_rule.matches, 1);
    assert_eq!(accept_rule.bytes_matched, 1500);
    let (_, _, packets, accepted, dropped, _) = stats();
    assert_eq!((packets, accepted, dropped), (2, 1, 1));
    crate::serial_println!("  [4/8] record match: OK");

    // 5: Toggle disables the rule so it no longer matches.
    let enabled = toggle_rule(r_in).expect("toggle");
    assert!(!enabled);
    record_match(Chain::Input, Action::Accept, 999).expect("match disabled");
    let still = list_rules().into_iter().find(|r| r.id == r_in).expect("rule");
    assert_eq!(still.matches, 1); // unchanged — rule disabled
    assert!(toggle_rule(r_in).expect("toggle back"));
    crate::serial_println!("  [5/8] toggle: OK");

    // 6: Chain filter only returns matching-chain rules.
    let input_rules = rules_for_chain(Chain::Input);
    assert_eq!(input_rules.len(), 2);
    assert!(input_rules.iter().all(|r| r.chain == Chain::Input));
    assert_eq!(rules_for_chain(Chain::Output).len(), 0);
    crate::serial_println!("  [6/8] chain filter: OK");

    // 7: Connection tracking — register, dedup, account, advance, untrack.
    track_connection(0x0A000001, 0x0A000002, 40000, 80, 6).expect("track");
    track_connection(0x0A000001, 0x0A000002, 40000, 80, 6).expect("track dup");
    assert_eq!(conntrack().len(), 1); // idempotent on the same 5-tuple
    update_connection(0x0A000001, 0x0A000002, 40000, 80, 6, 3, 4096).expect("update");
    set_conn_state(0x0A000001, 0x0A000002, 40000, 80, 6, ConnState::Established).expect("state");
    let entry = conntrack().into_iter().next().expect("entry");
    assert_eq!(entry.packets, 3);
    assert_eq!(entry.bytes, 4096);
    assert_eq!(entry.state, ConnState::Established);
    assert!(update_connection(1, 2, 3, 4, 6, 1, 1).is_err()); // untracked → NotFound
    untrack_connection(0x0A000001, 0x0A000002, 40000, 80, 6).expect("untrack");
    assert_eq!(conntrack().len(), 0);
    crate::serial_println!("  [7/8] conntrack: OK");

    // 8: Final stats reflect only the real activity above. record_match
    //    advances the per-action totals unconditionally (independent of
    //    whether an enabled rule matched), so the 3 record_match calls
    //    (2 Accept + 1 Drop) give packets=3, accepted=2, dropped=1.
    let (rules, ct, packets, accepted, dropped, ops) = stats();
    assert_eq!(rules, 2);
    assert_eq!(ct, 0);
    assert_eq!((packets, accepted, dropped), (3, 2, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("netfilter::self_test() — all 8 tests passed");
}
