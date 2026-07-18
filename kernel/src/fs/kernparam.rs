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
use crate::sync::PreemptSpinMutex as Mutex;

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

    // Read the REAL kernel command line from the bootloader (Limine). We never
    // fabricate boot parameters: if the bootloader passed no cmdline (as with
    // the default limine.conf), the parameter set is genuinely empty. When a
    // cmdline IS present, each whitespace-separated `key[=value]` token is
    // parsed into a parameter with Bootloader origin, so /proc and the
    // `kernparam` shell command always reflect what the machine actually
    // booted with rather than an invented command line.
    let cmdline = crate::boot::kernel_cmdline().unwrap_or("");
    let params = parse_cmdline(cmdline);

    *guard = Some(State {
        params,
        cmdline: String::from(cmdline),
        total_lookups: 0,
        total_sets: 0,
        ops: 0,
    });
}

/// Parse a boot command line into parameter entries.
///
/// Tokens are split on ASCII whitespace; each token is either `key=value` (split
/// at the first `=`) or a bare `key` (treated as a present flag with an empty
/// value). All parsed parameters carry [`ParamOrigin::Bootloader`].
fn parse_cmdline(cmdline: &str) -> Vec<KernelParam> {
    let mut params = Vec::new();
    for token in cmdline.split_ascii_whitespace() {
        if token.is_empty() {
            continue;
        }
        let (key, value) = token.split_once('=').unwrap_or((token, ""));
        params.push(KernelParam {
            key: String::from(key),
            value: String::from(value),
            origin: ParamOrigin::Bootloader,
            consumed_by: None,
            description: String::new(),
        });
    }
    params
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

    // 1: parse_cmdline — the pure parser over the real boot cmdline format.
    let parsed = parse_cmdline("root=/dev/vda1 console=ttyS0,115200 quiet loglevel=6");
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].key, "root");
    assert_eq!(parsed[0].value, "/dev/vda1");
    let quiet = parsed.iter().find(|p| p.key == "quiet").expect("quiet");
    assert_eq!(quiet.value, ""); // bare flag → empty value.
    assert!(parsed.iter().all(|p| p.origin == ParamOrigin::Bootloader));
    crate::serial_println!("  [1/8] parse_cmdline: OK");

    // Residue-free: install a known, controlled State for the stateful tests so
    // assertions hold regardless of the cmdline this machine actually booted
    // with (and regardless of prior kshell/procfs activity).
    let fixture = "root=/dev/vda1 console=ttyS0,115200 quiet loglevel=6 mem=512M";
    *STATE.lock() = Some(State {
        params: parse_cmdline(fixture),
        cmdline: String::from(fixture),
        total_lookups: 0,
        total_sets: 0,
        ops: 0,
    });

    // 2: Get parameter.
    assert_eq!(get("root").expect("get"), "/dev/vda1");
    crate::serial_println!("  [2/8] get: OK");

    // 3: Boolean check.
    assert!(is_set("quiet"));
    assert!(!is_set("nonexistent"));
    crate::serial_println!("  [3/8] is_set: OK");

    // 4: Set new parameter (runtime origin).
    set("debug", "1").expect("set");
    assert_eq!(get("debug").expect("get2"), "1");
    assert!(is_set("debug"));
    crate::serial_println!("  [4/8] set: OK");

    // 5: Override (origin flips to Runtime).
    set("loglevel", "7").expect("override");
    let p = list_params().iter().find(|p| p.key == "loglevel").expect("find").clone();
    assert_eq!(p.value, "7");
    assert_eq!(p.origin, ParamOrigin::Runtime);
    crate::serial_println!("  [5/8] override: OK");

    // 6: Consume.
    assert_eq!(consume("debug", "test_subsys").expect("consume"), "1");
    crate::serial_println!("  [6/8] consume: OK");

    // 7: Unconsumed — the 5 Bootloader params remain unconsumed (debug now is).
    let uncon = unconsumed();
    assert!(uncon.iter().all(|p| p.consumed_by.is_none()));
    assert!(uncon.iter().any(|p| p.key == "root"));
    crate::serial_println!("  [7/8] unconsumed: OK");

    // 8: Stats — 5 seeded + 1 new (debug) = 6 params; exactly 2 sets.
    let (count, lookups, sets, unconsumed_count, ops) = stats();
    assert_eq!(count, 6);
    assert!(lookups >= 3);
    assert_eq!(sets, 2);
    assert_eq!(unconsumed_count, 5);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    crate::serial_println!("kernparam::self_test() — all 8 tests passed");
}
