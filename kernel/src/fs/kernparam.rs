//! Kernel Parameters — boot parameter management.
//!
//! Manages kernel command-line parameters passed at boot time,
//! provides a key=value store for boot options, and tracks which
//! parameters were consumed by which subsystem.
//!
//! ## Architecture
//!
//! ```text
//! Kernel parameters
//!   → kernparam::get(key) → get parameter value
//!   → kernparam::set(key, value) → set/override parameter
//!   → kernparam::cmdline() → full command line
//!   → kernparam::list() → all parameters
//!
//! Integration:
//!   → bootcfg (boot configuration)
//!   → sysctlfs (sysctl parameters)
//!   → kernlog (kernel logging)
//!   → sysinfo (system information)
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

/// Parameter origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamOrigin {
    Bootloader,
    Default,
    Runtime,
}

impl ParamOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bootloader => "boot",
            Self::Default => "default",
            Self::Runtime => "runtime",
        }
    }
}

/// A kernel parameter entry.
#[derive(Debug, Clone)]
pub struct KernelParam {
    pub key: String,
    pub value: String,
    pub origin: ParamOrigin,
    pub consumed_by: Option<String>,
    pub description: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PARAMS: usize = 256;

struct State {
    params: Vec<KernelParam>,
    cmdline: String,
    total_lookups: u64,
    total_sets: u64,
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
        params: alloc::vec![
            KernelParam {
                key: String::from("root"), value: String::from("/dev/sda1"),
                origin: ParamOrigin::Bootloader, consumed_by: Some(String::from("vfs")),
                description: String::from("Root filesystem device"),
            },
            KernelParam {
                key: String::from("console"), value: String::from("ttyS0,115200"),
                origin: ParamOrigin::Bootloader, consumed_by: Some(String::from("serial")),
                description: String::from("Console device"),
            },
            KernelParam {
                key: String::from("loglevel"), value: String::from("6"),
                origin: ParamOrigin::Default, consumed_by: Some(String::from("kernlog")),
                description: String::from("Kernel log level"),
            },
            KernelParam {
                key: String::from("quiet"), value: String::from(""),
                origin: ParamOrigin::Bootloader, consumed_by: None,
                description: String::from("Suppress boot messages"),
            },
            KernelParam {
                key: String::from("mem"), value: String::from("4G"),
                origin: ParamOrigin::Default, consumed_by: Some(String::from("mm")),
                description: String::from("Memory limit"),
            },
        ],
        cmdline: String::from("root=/dev/sda1 console=ttyS0,115200 loglevel=6 quiet mem=4G"),
        total_lookups: 0,
        total_sets: 0,
        ops: 0,
    });
}

/// Get a parameter value.
pub fn get(key: &str) -> Option<String> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.total_lookups += 1;
        state.ops += 1;
        state.params.iter().find(|p| p.key == key).map(|p| p.value.clone())
    } else {
        None
    }
}

/// Set or override a parameter.
pub fn set(key: &str, value: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(p) = state.params.iter_mut().find(|p| p.key == key) {
            p.value = String::from(value);
            p.origin = ParamOrigin::Runtime;
        } else {
            if state.params.len() >= MAX_PARAMS { return Err(KernelError::ResourceExhausted); }
            state.params.push(KernelParam {
                key: String::from(key), value: String::from(value),
                origin: ParamOrigin::Runtime, consumed_by: None,
                description: String::from("User-set parameter"),
            });
        }
        state.total_sets += 1;
        Ok(())
    })
}

/// Mark a parameter as consumed.
pub fn consume(key: &str, subsystem: &str) -> KernelResult<String> {
    with_state(|state| {
        let p = state.params.iter_mut().find(|p| p.key == key).ok_or(KernelError::NotFound)?;
        p.consumed_by = Some(String::from(subsystem));
        Ok(p.value.clone())
    })
}

/// Check if a boolean parameter is set (present with empty value or "1"/"yes"/"true").
pub fn is_set(key: &str) -> bool {
    get(key).is_some_and(|v| v.is_empty() || v == "1" || v == "yes" || v == "true")
}

/// List all parameters.
pub fn list_params() -> Vec<KernelParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.params.clone())
}

/// Get full command line.
pub fn cmdline() -> String {
    STATE.lock().as_ref().map_or(String::new(), |s| s.cmdline.clone())
}

/// Remove a parameter.
pub fn remove(key: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.params.len();
        state.params.retain(|p| p.key != key);
        if state.params.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Get unconsumed parameters.
pub fn unconsumed() -> Vec<KernelParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.params.iter().filter(|p| p.consumed_by.is_none()).cloned().collect()
    })
}

/// Statistics: (param_count, total_lookups, total_sets, unconsumed_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let unconsumed = s.params.iter().filter(|p| p.consumed_by.is_none()).count();
            (s.params.len(), s.total_lookups, s.total_sets, unconsumed, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kernparam::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_params().len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get parameter.
    let val = get("root").expect("get");
    assert_eq!(val, "/dev/sda1");
    crate::serial_println!("  [2/8] get: OK");

    // 3: Boolean check.
    assert!(is_set("quiet"));
    assert!(!is_set("nonexistent"));
    crate::serial_println!("  [3/8] is_set: OK");

    // 4: Set new parameter.
    set("debug", "1").expect("set");
    assert_eq!(get("debug").expect("get2"), "1");
    assert!(is_set("debug"));
    crate::serial_println!("  [4/8] set: OK");

    // 5: Override.
    set("loglevel", "7").expect("override");
    let p = list_params().iter().find(|p| p.key == "loglevel").expect("find").clone();
    assert_eq!(p.value, "7");
    assert_eq!(p.origin, ParamOrigin::Runtime);
    crate::serial_println!("  [5/8] override: OK");

    // 6: Consume.
    let val = consume("debug", "test_subsys").expect("consume");
    assert_eq!(val, "1");
    crate::serial_println!("  [6/8] consume: OK");

    // 7: Unconsumed.
    let uncon = unconsumed();
    assert!(uncon.iter().all(|p| p.consumed_by.is_none()));
    crate::serial_println!("  [7/8] unconsumed: OK");

    // 8: Stats.
    let (count, lookups, sets, unconsumed_count, ops) = stats();
    assert_eq!(count, 6);
    assert!(lookups >= 3);
    assert!(sets >= 2);
    let _ = unconsumed_count;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kernparam::self_test() — all 8 tests passed");
}
