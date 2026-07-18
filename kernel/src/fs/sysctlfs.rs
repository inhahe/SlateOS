//! Sysctl — system parameter tuning.
//!
//! Provides a key-value store for kernel tunables, enabling
//! runtime configuration of system behavior without reboot.
//!
//! ## Architecture
//!
//! ```text
//! Parameter management
//!   → sysctlfs::get(key) → read parameter value
//!   → sysctlfs::set(key, value) → modify parameter
//!   → sysctlfs::list() → all parameters
//!
//! Integration:
//!   → schedtune (scheduler tuning)
//!   → mmtune (memory tuning)
//!   → fstune (filesystem tuning)
//!   → netsettings (network settings)
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

/// Parameter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Integer,
    Boolean,
    StringVal,
    Percentage,
}

impl ParamType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Integer => "int",
            Self::Boolean => "bool",
            Self::StringVal => "string",
            Self::Percentage => "percent",
        }
    }
}

/// A system parameter.
#[derive(Debug, Clone)]
pub struct SysParam {
    pub key: String,
    pub value: String,
    pub param_type: ParamType,
    pub description: String,
    pub read_only: bool,
    pub modified: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PARAMS: usize = 500;

struct State {
    params: Vec<SysParam>,
    total_reads: u64,
    total_writes: u64,
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
            SysParam { key: String::from("kernel.hostname"), value: String::from("localhost"),
                param_type: ParamType::StringVal, description: String::from("System hostname"),
                read_only: false, modified: false },
            SysParam { key: String::from("kernel.ostype"), value: String::from("MintOS"),
                param_type: ParamType::StringVal, description: String::from("OS type"),
                read_only: true, modified: false },
            SysParam { key: String::from("kernel.osrelease"), value: String::from("0.1.0"),
                param_type: ParamType::StringVal, description: String::from("OS release version"),
                read_only: true, modified: false },
            SysParam { key: String::from("vm.swappiness"), value: String::from("60"),
                param_type: ParamType::Percentage, description: String::from("Swap aggressiveness (0-100)"),
                read_only: false, modified: false },
            SysParam { key: String::from("vm.dirty_ratio"), value: String::from("20"),
                param_type: ParamType::Percentage, description: String::from("Dirty page ratio threshold"),
                read_only: false, modified: false },
            SysParam { key: String::from("vm.overcommit"), value: String::from("0"),
                param_type: ParamType::Boolean, description: String::from("Allow memory overcommit"),
                read_only: false, modified: false },
            SysParam { key: String::from("net.ipv4.ip_forward"), value: String::from("0"),
                param_type: ParamType::Boolean, description: String::from("Enable IPv4 forwarding"),
                read_only: false, modified: false },
            SysParam { key: String::from("net.core.somaxconn"), value: String::from("128"),
                param_type: ParamType::Integer, description: String::from("Max socket listen backlog"),
                read_only: false, modified: false },
            SysParam { key: String::from("fs.file-max"), value: String::from("65536"),
                param_type: ParamType::Integer, description: String::from("Max open files system-wide"),
                read_only: false, modified: false },
            SysParam { key: String::from("fs.inotify.max_user_watches"), value: String::from("8192"),
                param_type: ParamType::Integer, description: String::from("Max inotify watches per user"),
                read_only: false, modified: false },
        ],
        total_reads: 0,
        total_writes: 0,
        ops: 0,
    });
}

/// Get a parameter value.
pub fn get(key: &str) -> KernelResult<String> {
    with_state(|state| {
        let param = state.params.iter().find(|p| p.key == key)
            .ok_or(KernelError::NotFound)?;
        state.total_reads += 1;
        Ok(param.value.clone())
    })
}

/// Set a parameter value.
pub fn set(key: &str, value: &str) -> KernelResult<()> {
    with_state(|state| {
        let param = state.params.iter_mut().find(|p| p.key == key)
            .ok_or(KernelError::NotFound)?;
        if param.read_only {
            return Err(KernelError::PermissionDenied);
        }
        param.value = String::from(value);
        param.modified = true;
        state.total_writes += 1;
        Ok(())
    })
}

/// Add a new parameter.
pub fn add_param(key: &str, value: &str, param_type: ParamType, description: &str, read_only: bool) -> KernelResult<()> {
    with_state(|state| {
        if state.params.len() >= MAX_PARAMS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.params.iter().any(|p| p.key == key) {
            return Err(KernelError::AlreadyExists);
        }
        state.params.push(SysParam {
            key: String::from(key), value: String::from(value),
            param_type, description: String::from(description),
            read_only, modified: false,
        });
        Ok(())
    })
}

/// Remove a parameter.
pub fn remove_param(key: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.params.len();
        state.params.retain(|p| p.key != key);
        if state.params.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all parameters.
pub fn list_all() -> Vec<SysParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.params.clone())
}

/// List parameters matching a prefix.
pub fn list_prefix(prefix: &str) -> Vec<SysParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.params.iter().filter(|p| p.key.starts_with(prefix)).cloned().collect()
    })
}

/// List modified parameters.
pub fn list_modified() -> Vec<SysParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.params.iter().filter(|p| p.modified).cloned().collect()
    })
}

/// Statistics: (param_count, total_reads, total_writes, modified_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let modified = s.params.iter().filter(|p| p.modified).count();
            (s.params.len(), s.total_reads, s.total_writes, modified, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysctlfs::self_test() — running tests...");

    // Residue-free: start from a clean, controlled default config so the
    // assertions are deterministic regardless of prior kshell/procfs activity
    // (init_defaults early-returns when STATE is already populated).
    *STATE.lock() = None;
    init_defaults();

    // 1: Default tunables — sysctl ships a fixed set of configuration defaults.
    assert_eq!(list_all().len(), 10);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get.
    let val = get("kernel.hostname").expect("get");
    assert_eq!(val, "localhost");
    crate::serial_println!("  [2/8] get: OK");

    // 3: Set.
    set("kernel.hostname", "myhost").expect("set");
    let val = get("kernel.hostname").expect("get2");
    assert_eq!(val, "myhost");
    crate::serial_println!("  [3/8] set: OK");

    // 4: Read-only.
    assert!(set("kernel.ostype", "other").is_err());
    crate::serial_println!("  [4/8] read-only: OK");

    // 5: Add param.
    add_param("custom.test", "42", ParamType::Integer, "Test param", false).expect("add");
    assert!(get("custom.test").is_ok());
    crate::serial_println!("  [5/8] add: OK");

    // 6: Prefix search (kernel.hostname / kernel.ostype / kernel.osrelease).
    let kernel_params = list_prefix("kernel.");
    assert_eq!(kernel_params.len(), 3);
    crate::serial_println!("  [6/8] prefix: OK");

    // 7: Modified list.
    let modified = list_modified();
    assert!(modified.iter().any(|p| p.key == "kernel.hostname"));
    crate::serial_println!("  [7/8] modified: OK");

    // 8: Stats — exact: 11 params (10 defaults + custom.test), 3 reads, 1 write,
    //    1 modified (kernel.hostname).
    let (count, reads, writes, modified_count, ops) = stats();
    assert_eq!(count, 11);
    assert_eq!(reads, 3);
    assert_eq!(writes, 1);
    assert_eq!(modified_count, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    crate::serial_println!("sysctlfs::self_test() — all 8 tests passed");
}
