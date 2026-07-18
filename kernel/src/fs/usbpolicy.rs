//! USB Policy — USB device access policies and security.
//!
//! Controls which USB devices are allowed to connect, with
//! allowlists/blocklists, class-based filtering, and logging.
//!
//! ## Architecture
//!
//! ```text
//! USB device connects
//!   → usbpolicy::check_device(vid, pid, class) → allow/deny
//!   → usbpolicy::log_event(device, decision)
//!
//! Integration:
//!   → usbmgr (USB device management)
//!   → devicemgr (device manager)
//!   → parental (parental controls)
//!   → audit (security audit)
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

/// USB device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    Storage,
    HumanInterface,
    Audio,
    Video,
    Printer,
    Network,
    Wireless,
    SmartCard,
    Hub,
    Other,
}

impl UsbClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Storage => "Storage",
            Self::HumanInterface => "HID",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Printer => "Printer",
            Self::Network => "Network",
            Self::Wireless => "Wireless",
            Self::SmartCard => "SmartCard",
            Self::Hub => "Hub",
            Self::Other => "Other",
        }
    }
}

/// Policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    AskUser,
    ReadOnly,
}

impl Decision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::AskUser => "Ask User",
            Self::ReadOnly => "Read Only",
        }
    }
}

/// A USB policy rule.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub id: u32,
    pub name: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub class: Option<UsbClass>,
    pub decision: Decision,
    pub enabled: bool,
    pub hit_count: u64,
}

/// A USB event log entry.
#[derive(Debug, Clone)]
pub struct UsbEvent {
    pub vendor_id: u16,
    pub product_id: u16,
    pub class: UsbClass,
    pub device_name: String,
    pub decision: Decision,
    pub rule_id: Option<u32>,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_RULES: usize = 100;
const MAX_LOG: usize = 500;

struct State {
    rules: Vec<PolicyRule>,
    log: Vec<UsbEvent>,
    next_id: u32,
    default_decision: Decision,
    block_unknown: bool,
    total_allowed: u64,
    total_denied: u64,
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
            PolicyRule { id: 1, name: String::from("Allow HID"), vendor_id: None, product_id: None, class: Some(UsbClass::HumanInterface), decision: Decision::Allow, enabled: true, hit_count: 0 },
            PolicyRule { id: 2, name: String::from("Allow Audio"), vendor_id: None, product_id: None, class: Some(UsbClass::Audio), decision: Decision::Allow, enabled: true, hit_count: 0 },
            PolicyRule { id: 3, name: String::from("Storage ask"), vendor_id: None, product_id: None, class: Some(UsbClass::Storage), decision: Decision::AskUser, enabled: true, hit_count: 0 },
        ],
        log: Vec::new(),
        next_id: 4,
        default_decision: Decision::AskUser,
        block_unknown: false,
        total_allowed: 0,
        total_denied: 0,
        ops: 0,
    });
}

/// Check a device against policy rules.
pub fn check_device(vid: u16, pid: u16, class: UsbClass, name: &str) -> KernelResult<Decision> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Check rules in order (first match wins).
        for rule in &mut state.rules {
            if !rule.enabled { continue; }
            let vid_match = rule.vendor_id.is_none_or(|v| v == vid);
            let pid_match = rule.product_id.is_none_or(|p| p == pid);
            let class_match = rule.class.is_none_or(|c| c == class);
            if vid_match && pid_match && class_match {
                rule.hit_count += 1;
                let decision = rule.decision;
                let rule_id = rule.id;
                match decision {
                    Decision::Allow | Decision::ReadOnly => state.total_allowed += 1,
                    Decision::Deny => state.total_denied += 1,
                    Decision::AskUser => {}
                }
                if state.log.len() >= MAX_LOG { state.log.remove(0); }
                state.log.push(UsbEvent {
                    vendor_id: vid, product_id: pid, class,
                    device_name: String::from(name), decision,
                    rule_id: Some(rule_id), timestamp_ns: now,
                });
                return Ok(decision);
            }
        }
        // Default decision.
        let decision = if state.block_unknown { Decision::Deny } else { state.default_decision };
        match decision {
            Decision::Allow | Decision::ReadOnly => state.total_allowed += 1,
            Decision::Deny => state.total_denied += 1,
            Decision::AskUser => {}
        }
        if state.log.len() >= MAX_LOG { state.log.remove(0); }
        state.log.push(UsbEvent {
            vendor_id: vid, product_id: pid, class,
            device_name: String::from(name), decision,
            rule_id: None, timestamp_ns: now,
        });
        Ok(decision)
    })
}

/// Add a policy rule.
pub fn add_rule(name: &str, vid: Option<u16>, pid: Option<u16>, class: Option<UsbClass>, decision: Decision) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.rules.push(PolicyRule {
            id, name: String::from(name),
            vendor_id: vid, product_id: pid, class,
            decision, enabled: true, hit_count: 0,
        });
        Ok(id)
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.rules.len();
        state.rules.retain(|r| r.id != id);
        if state.rules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set default decision for unmatched devices.
pub fn set_default(decision: Decision) -> KernelResult<()> {
    with_state(|state| {
        state.default_decision = decision;
        Ok(())
    })
}

/// Block all unknown devices.
pub fn set_block_unknown(block: bool) -> KernelResult<()> {
    with_state(|state| {
        state.block_unknown = block;
        Ok(())
    })
}

/// List rules.
pub fn list_rules() -> Vec<PolicyRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// Get event log.
pub fn get_log(max: usize) -> Vec<UsbEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut log = s.log.clone();
        log.reverse();
        log.truncate(max);
        log
    })
}

/// Statistics: (rule_count, log_size, total_allowed, total_denied, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.rules.len(), s.log.len(), s.total_allowed, s.total_denied, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("usbpolicy::self_test() — running tests...");
    init_defaults();

    // 1: Default rules.
    assert_eq!(list_rules().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: HID device allowed.
    let d = check_device(0x046D, 0xC52B, UsbClass::HumanInterface, "Logitech Mouse").expect("check1");
    assert_eq!(d, Decision::Allow);
    crate::serial_println!("  [2/8] hid allowed: OK");

    // 3: Storage asks user.
    let d = check_device(0x0781, 0x5567, UsbClass::Storage, "USB Drive").expect("check2");
    assert_eq!(d, Decision::AskUser);
    crate::serial_println!("  [3/8] storage ask: OK");

    // 4: Add deny rule for specific vendor.
    let _rid = add_rule("Block BadUSB", Some(0xDEAD), None, None, Decision::Deny).expect("add");
    let d = check_device(0xDEAD, 0x0001, UsbClass::Other, "BadUSB").expect("check3");
    assert_eq!(d, Decision::Deny);
    crate::serial_println!("  [4/8] deny rule: OK");

    // 5: Unknown device gets default.
    let d = check_device(0x1234, 0x5678, UsbClass::Network, "Unknown Net").expect("check4");
    assert_eq!(d, Decision::AskUser); // Default is AskUser.
    crate::serial_println!("  [5/8] default: OK");

    // 6: Block unknown mode.
    set_block_unknown(true).expect("block");
    let d = check_device(0xAAAA, 0xBBBB, UsbClass::Other, "Random").expect("check5");
    assert_eq!(d, Decision::Deny);
    crate::serial_println!("  [6/8] block unknown: OK");

    // 7: Event log.
    let log = get_log(10);
    assert!(log.len() >= 5);
    crate::serial_println!("  [7/8] log: OK");

    // 8: Stats.
    let (rules, _log_size, allowed, denied, ops) = stats();
    assert_eq!(rules, 4);
    assert!(allowed >= 1);
    assert!(denied >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("usbpolicy::self_test() — all 8 tests passed");
}
