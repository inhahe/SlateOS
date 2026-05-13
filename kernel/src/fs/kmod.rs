//! Kernel Module — loadable kernel module management (simulated).
//!
//! Tracks loaded kernel modules, their dependencies, parameters,
//! and provides load/unload lifecycle management.
//!
//! ## Architecture
//!
//! ```text
//! Kernel modules
//!   → kmod::load(name) → load module
//!   → kmod::unload(name) → unload module
//!   → kmod::list() → list loaded modules
//!   → kmod::info(name) → module details
//!
//! Integration:
//!   → devicemgr (device manager)
//!   → driverupdate (driver updates)
//!   → sysinfo (system information)
//!   → kernlog (kernel logging)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Module state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Loading,
    Live,
    Unloading,
    Gone,
}

impl ModuleState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Loading => "Loading",
            Self::Live => "Live",
            Self::Unloading => "Unloading",
            Self::Gone => "Gone",
        }
    }
}

/// Module type/category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Driver,
    Filesystem,
    Network,
    Security,
    Crypto,
    Other,
}

impl ModuleType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Driver => "Driver",
            Self::Filesystem => "Filesystem",
            Self::Network => "Network",
            Self::Security => "Security",
            Self::Crypto => "Crypto",
            Self::Other => "Other",
        }
    }
}

/// A loaded kernel module.
#[derive(Debug, Clone)]
pub struct KernelModule {
    pub name: String,
    pub mod_type: ModuleType,
    pub state: ModuleState,
    pub version: String,
    pub size_bytes: u64,
    pub ref_count: u32,
    pub depends_on: Vec<String>,
    pub loaded_at_ns: u64,
    pub description: String,
}

/// Module parameter.
#[derive(Debug, Clone)]
pub struct ModuleParam {
    pub module_name: String,
    pub param_name: String,
    pub value: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_MODULES: usize = 128;

struct State {
    modules: Vec<KernelModule>,
    params: Vec<ModuleParam>,
    total_loads: u64,
    total_unloads: u64,
    total_errors: u64,
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
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        modules: alloc::vec![
            KernelModule {
                name: String::from("virtio_blk"), mod_type: ModuleType::Driver,
                state: ModuleState::Live, version: String::from("1.0"),
                size_bytes: 32768, ref_count: 1, depends_on: Vec::new(),
                loaded_at_ns: now, description: String::from("VirtIO block driver"),
            },
            KernelModule {
                name: String::from("virtio_net"), mod_type: ModuleType::Network,
                state: ModuleState::Live, version: String::from("1.0"),
                size_bytes: 45056, ref_count: 1, depends_on: Vec::new(),
                loaded_at_ns: now, description: String::from("VirtIO network driver"),
            },
            KernelModule {
                name: String::from("fat"), mod_type: ModuleType::Filesystem,
                state: ModuleState::Live, version: String::from("1.0"),
                size_bytes: 65536, ref_count: 2, depends_on: Vec::new(),
                loaded_at_ns: now, description: String::from("FAT filesystem"),
            },
        ],
        params: Vec::new(),
        total_loads: 3,
        total_unloads: 0,
        total_errors: 0,
        ops: 0,
    });
}

/// List all loaded modules.
pub fn list_modules() -> Vec<KernelModule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.modules.clone())
}

/// Get module info by name.
pub fn get_module(name: &str) -> Option<KernelModule> {
    STATE.lock().as_ref().and_then(|s| s.modules.iter().find(|m| m.name == name).cloned())
}

/// Load a module.
pub fn load_module(name: &str, mod_type: ModuleType, size: u64, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.modules.len() >= MAX_MODULES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.modules.iter().any(|m| m.name == name && m.state == ModuleState::Live) {
            return Err(KernelError::AlreadyExists);
        }
        let now = crate::hpet::elapsed_ns();
        state.modules.push(KernelModule {
            name: String::from(name), mod_type, state: ModuleState::Live,
            version: String::from("1.0"), size_bytes: size, ref_count: 0,
            depends_on: Vec::new(), loaded_at_ns: now,
            description: String::from(desc),
        });
        state.total_loads += 1;
        Ok(())
    })
}

/// Unload a module.
pub fn unload_module(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.modules.iter().position(|m| m.name == name && m.state == ModuleState::Live)
            .ok_or(KernelError::NotFound)?;
        if state.modules[idx].ref_count > 0 {
            return Err(KernelError::WouldBlock);
        }
        // Check if other modules depend on this one.
        let depended = state.modules.iter()
            .any(|m| m.state == ModuleState::Live && m.depends_on.iter().any(|d| d == name));
        if depended {
            return Err(KernelError::WouldBlock);
        }
        state.modules[idx].state = ModuleState::Gone;
        state.total_unloads += 1;
        Ok(())
    })
}

/// Add a dependency relationship.
pub fn add_dependency(module_name: &str, depends_on: &str) -> KernelResult<()> {
    with_state(|state| {
        // Verify dependency target exists.
        if !state.modules.iter().any(|m| m.name == depends_on && m.state == ModuleState::Live) {
            return Err(KernelError::NotFound);
        }
        let module = state.modules.iter_mut()
            .find(|m| m.name == module_name && m.state == ModuleState::Live)
            .ok_or(KernelError::NotFound)?;
        if module.depends_on.iter().any(|d| d == depends_on) {
            return Ok(()); // Already recorded.
        }
        module.depends_on.push(String::from(depends_on));
        Ok(())
    })
}

/// Set a module parameter.
pub fn set_param(module_name: &str, param_name: &str, value: &str, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.modules.iter().any(|m| m.name == module_name) {
            return Err(KernelError::NotFound);
        }
        if let Some(p) = state.params.iter_mut()
            .find(|p| p.module_name == module_name && p.param_name == param_name) {
            p.value = String::from(value);
        } else {
            state.params.push(ModuleParam {
                module_name: String::from(module_name),
                param_name: String::from(param_name),
                value: String::from(value),
                description: String::from(desc),
            });
        }
        Ok(())
    })
}

/// Get module parameters.
pub fn get_params(module_name: &str) -> Vec<ModuleParam> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.params.iter().filter(|p| p.module_name == module_name).cloned().collect()
    })
}

/// Count of live modules.
pub fn live_count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| {
        s.modules.iter().filter(|m| m.state == ModuleState::Live).count()
    })
}

/// Statistics: (live_count, total_loads, total_unloads, total_errors, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let live = s.modules.iter().filter(|m| m.state == ModuleState::Live).count();
            (live, s.total_loads, s.total_unloads, s.total_errors, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kmod::self_test() — running tests...");
    init_defaults();

    // 1: Default modules.
    assert_eq!(list_modules().len(), 3);
    assert_eq!(live_count(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get module.
    let m = get_module("virtio_blk").expect("get");
    assert_eq!(m.mod_type, ModuleType::Driver);
    assert_eq!(m.state, ModuleState::Live);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Load module.
    load_module("test_mod", ModuleType::Other, 4096, "test module").expect("load");
    assert_eq!(live_count(), 4);
    crate::serial_println!("  [3/8] load: OK");

    // 4: Duplicate load rejected.
    assert!(load_module("test_mod", ModuleType::Other, 4096, "dup").is_err());
    crate::serial_println!("  [4/8] dup load: OK");

    // 5: Unload module.
    unload_module("test_mod").expect("unload");
    assert_eq!(live_count(), 3);
    crate::serial_println!("  [5/8] unload: OK");

    // 6: Can't unload with ref_count.
    assert!(unload_module("virtio_blk").is_err());
    crate::serial_println!("  [6/8] ref_count block: OK");

    // 7: Module params.
    set_param("fat", "codepage", "437", "Default codepage").expect("param");
    let params = get_params("fat");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].value, "437");
    crate::serial_println!("  [7/8] params: OK");

    // 8: Stats.
    let (live, loads, unloads, errors, ops) = stats();
    assert_eq!(live, 3);
    assert!(loads >= 4);
    assert!(unloads >= 1);
    let _ = errors;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kmod::self_test() — all 8 tests passed");
}
