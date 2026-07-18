//! SysRq — magic SysRq key handler registry.
//!
//! Manages SysRq key bindings for emergency system operations
//! (sync, unmount, reboot, kill, memory info, etc.). Each key
//! maps to a handler that performs a specific action.
//!
//! ## Architecture
//!
//! ```text
//! SysRq system
//!   → sysrq::trigger(key) → execute SysRq action
//!   → sysrq::register(key, handler) → add custom handler
//!   → sysrq::list() → show registered keys
//!   → sysrq::set_enabled(mask) → control which keys work
//!
//! Integration:
//!   → kernlog (kernel logging)
//!   → power (system power)
//!   → sysinfo (system info)
//!   → coredump (crash dumps)
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

/// SysRq action category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysRqCategory {
    Reboot,
    Process,
    Memory,
    Filesystem,
    Debug,
    Info,
    Custom,
}

impl SysRqCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Reboot => "Reboot",
            Self::Process => "Process",
            Self::Memory => "Memory",
            Self::Filesystem => "Filesystem",
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Custom => "Custom",
        }
    }
}

/// A registered SysRq handler.
#[derive(Debug, Clone)]
pub struct SysRqHandler {
    pub key: char,
    pub category: SysRqCategory,
    pub description: String,
    pub enabled: bool,
    pub trigger_count: u64,
    pub last_triggered_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HANDLERS: usize = 64;

struct State {
    handlers: Vec<SysRqHandler>,
    enabled_mask: u32,  // Bitmask for categories.
    total_triggers: u64,
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

fn category_bit(cat: SysRqCategory) -> u32 {
    match cat {
        SysRqCategory::Reboot => 1,
        SysRqCategory::Process => 2,
        SysRqCategory::Memory => 4,
        SysRqCategory::Filesystem => 8,
        SysRqCategory::Debug => 16,
        SysRqCategory::Info => 32,
        SysRqCategory::Custom => 64,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        handlers: alloc::vec![
            SysRqHandler { key: 'b', category: SysRqCategory::Reboot,
                description: String::from("Immediately reboot"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'e', category: SysRqCategory::Process,
                description: String::from("Send SIGTERM to all processes"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'i', category: SysRqCategory::Process,
                description: String::from("Send SIGKILL to all processes"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 's', category: SysRqCategory::Filesystem,
                description: String::from("Sync all filesystems"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'u', category: SysRqCategory::Filesystem,
                description: String::from("Remount all filesystems read-only"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'm', category: SysRqCategory::Memory,
                description: String::from("Show memory info"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 't', category: SysRqCategory::Debug,
                description: String::from("Show task list"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'h', category: SysRqCategory::Info,
                description: String::from("Show SysRq help"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'l', category: SysRqCategory::Debug,
                description: String::from("Show backtrace for all CPUs"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
            SysRqHandler { key: 'f', category: SysRqCategory::Memory,
                description: String::from("Call OOM killer"), enabled: true,
                trigger_count: 0, last_triggered_ns: 0 },
        ],
        enabled_mask: 0x7F, // All categories enabled.
        total_triggers: 0,
        total_blocked: 0,
        ops: 0,
    });
}

/// List all registered handlers.
pub fn list_handlers() -> Vec<SysRqHandler> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.handlers.clone())
}

/// Get handler for a key.
pub fn get_handler(key: char) -> Option<SysRqHandler> {
    STATE.lock().as_ref().and_then(|s| s.handlers.iter().find(|h| h.key == key).cloned())
}

/// Trigger a SysRq action.
pub fn trigger(key: char) -> KernelResult<()> {
    with_state(|state| {
        let handler = state.handlers.iter_mut().find(|h| h.key == key)
            .ok_or(KernelError::NotFound)?;
        if !handler.enabled {
            state.total_blocked += 1;
            return Err(KernelError::PermissionDenied);
        }
        let cat_bit = category_bit(handler.category);
        if state.enabled_mask & cat_bit == 0 {
            state.total_blocked += 1;
            return Err(KernelError::PermissionDenied);
        }
        let now = crate::hpet::elapsed_ns();
        handler.trigger_count += 1;
        handler.last_triggered_ns = now;
        state.total_triggers += 1;
        Ok(())
    })
}

/// Register a custom SysRq handler.
pub fn register(key: char, category: SysRqCategory, description: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.handlers.len() >= MAX_HANDLERS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.handlers.iter().any(|h| h.key == key) {
            return Err(KernelError::AlreadyExists);
        }
        state.handlers.push(SysRqHandler {
            key, category, description: String::from(description),
            enabled: true, trigger_count: 0, last_triggered_ns: 0,
        });
        Ok(())
    })
}

/// Unregister a handler.
pub fn unregister(key: char) -> KernelResult<()> {
    with_state(|state| {
        let before = state.handlers.len();
        state.handlers.retain(|h| h.key != key);
        if state.handlers.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set enabled category mask.
pub fn set_enabled_mask(mask: u32) -> KernelResult<()> {
    with_state(|state| {
        state.enabled_mask = mask;
        Ok(())
    })
}

/// Get enabled mask.
pub fn get_enabled_mask() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| s.enabled_mask)
}

/// Enable/disable a specific handler.
pub fn set_handler_enabled(key: char, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let handler = state.handlers.iter_mut().find(|h| h.key == key)
            .ok_or(KernelError::NotFound)?;
        handler.enabled = enabled;
        Ok(())
    })
}

/// Statistics: (handler_count, total_triggers, total_blocked, enabled_mask, ops).
pub fn stats() -> (usize, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.handlers.len(), s.total_triggers, s.total_blocked, s.enabled_mask, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysrq::self_test() — running tests...");
    init_defaults();

    // 1: Default handlers.
    assert_eq!(list_handlers().len(), 10);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get handler.
    let h = get_handler('b').expect("get");
    assert_eq!(h.category, SysRqCategory::Reboot);
    assert_eq!(h.trigger_count, 0);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Trigger.
    trigger('s').expect("trigger");
    let h = get_handler('s').expect("get2");
    assert_eq!(h.trigger_count, 1);
    assert!(h.last_triggered_ns > 0);
    crate::serial_println!("  [3/8] trigger: OK");

    // 4: Register custom.
    register('x', SysRqCategory::Custom, "Test handler").expect("register");
    assert_eq!(list_handlers().len(), 11);
    assert!(register('x', SysRqCategory::Custom, "dup").is_err());
    crate::serial_println!("  [4/8] register: OK");

    // 5: Unregister.
    unregister('x').expect("unregister");
    assert_eq!(list_handlers().len(), 10);
    crate::serial_println!("  [5/8] unregister: OK");

    // 6: Disable handler.
    set_handler_enabled('b', false).expect("disable");
    assert!(trigger('b').is_err());
    set_handler_enabled('b', true).expect("enable");
    crate::serial_println!("  [6/8] enable/disable: OK");

    // 7: Category mask.
    set_enabled_mask(0).expect("mask");
    assert!(trigger('m').is_err()); // All categories disabled.
    set_enabled_mask(0x7F).expect("mask2");
    trigger('m').expect("trigger2");
    crate::serial_println!("  [7/8] mask: OK");

    // 8: Stats.
    let (count, triggers, blocked, mask, ops) = stats();
    assert_eq!(count, 10);
    assert!(triggers >= 2);
    assert!(blocked >= 2);
    assert_eq!(mask, 0x7F);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("sysrq::self_test() — all 8 tests passed");
}
