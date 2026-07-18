//! Process Isolation — namespace and container primitives.
//!
//! Manages per-process namespaces for filesystem, network, PID,
//! and user isolation. Enables lightweight containers and
//! sandboxing by giving processes isolated views of system resources.
//!
//! ## Architecture
//!
//! ```text
//! Process isolation
//!   → prociso::create_namespace(type) → create namespace
//!   → prociso::attach(pid, ns_id) → attach process to namespace
//!   → prociso::detach(pid, ns_id) → detach process
//!   → prociso::list_namespaces() → all namespaces
//!
//! Integration:
//!   → appsandbox (app sandboxing)
//!   → cgroupfs (resource limits)
//!   → secpolicy (security policy)
//!   → namespace (namespace module)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Namespace type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsType {
    Mount,
    Pid,
    Net,
    User,
    Ipc,
    Uts,     // Hostname/domain.
    Cgroup,
}

impl NsType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mount => "mnt",
            Self::Pid => "pid",
            Self::Net => "net",
            Self::User => "user",
            Self::Ipc => "ipc",
            Self::Uts => "uts",
            Self::Cgroup => "cgroup",
        }
    }
}

/// Isolation level for a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    None,        // Shared with parent.
    Partial,     // Some resources isolated.
    Full,        // Completely isolated.
}

impl IsolationLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Partial => "partial",
            Self::Full => "full",
        }
    }
}

/// A namespace instance.
#[derive(Debug, Clone)]
pub struct Namespace {
    pub id: u32,
    pub ns_type: NsType,
    pub name: String,
    pub isolation: IsolationLevel,
    pub parent_id: Option<u32>,
    pub processes: Vec<u32>,
    pub created_ns: u64,
}

/// A container (bundle of namespaces).
#[derive(Debug, Clone)]
pub struct Container {
    pub id: u32,
    pub name: String,
    pub namespaces: Vec<u32>,   // Namespace IDs.
    pub root_pid: Option<u32>,
    pub created_ns: u64,
    pub running: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NAMESPACES: usize = 256;
const MAX_CONTAINERS: usize = 64;

struct State {
    namespaces: Vec<Namespace>,
    containers: Vec<Container>,
    next_ns_id: u32,
    next_container_id: u32,
    total_attaches: u64,
    total_detaches: u64,
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
        namespaces: alloc::vec![
            Namespace {
                id: 1, ns_type: NsType::Mount, name: String::from("root_mnt"),
                isolation: IsolationLevel::None, parent_id: None,
                processes: alloc::vec![1], created_ns: now,
            },
            Namespace {
                id: 2, ns_type: NsType::Pid, name: String::from("root_pid"),
                isolation: IsolationLevel::None, parent_id: None,
                processes: alloc::vec![1], created_ns: now,
            },
            Namespace {
                id: 3, ns_type: NsType::Net, name: String::from("root_net"),
                isolation: IsolationLevel::None, parent_id: None,
                processes: alloc::vec![1], created_ns: now,
            },
        ],
        containers: Vec::new(),
        next_ns_id: 4,
        next_container_id: 1,
        total_attaches: 0,
        total_detaches: 0,
        ops: 0,
    });
}

/// Create a new namespace.
pub fn create_namespace(ns_type: NsType, name: &str, isolation: IsolationLevel, parent: Option<u32>) -> KernelResult<u32> {
    with_state(|state| {
        if state.namespaces.len() >= MAX_NAMESPACES { return Err(KernelError::ResourceExhausted); }
        if state.namespaces.iter().any(|n| n.name == name) { return Err(KernelError::AlreadyExists); }
        if let Some(pid) = parent {
            if !state.namespaces.iter().any(|n| n.id == pid) { return Err(KernelError::NotFound); }
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_ns_id;
        state.next_ns_id += 1;
        state.namespaces.push(Namespace {
            id, ns_type, name: String::from(name), isolation,
            parent_id: parent, processes: Vec::new(), created_ns: now,
        });
        Ok(id)
    })
}

/// Delete a namespace.
pub fn delete_namespace(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        if !ns.processes.is_empty() { return Err(KernelError::NotEmpty); }
        // Don't delete if children exist.
        if state.namespaces.iter().any(|n| n.parent_id == Some(id)) {
            return Err(KernelError::NotEmpty);
        }
        state.namespaces.retain(|n| n.id != id);
        Ok(())
    })
}

/// Attach a process to a namespace.
pub fn attach(pid: u32, ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter_mut().find(|n| n.id == ns_id)
            .ok_or(KernelError::NotFound)?;
        if ns.processes.contains(&pid) { return Err(KernelError::AlreadyExists); }
        ns.processes.push(pid);
        state.total_attaches += 1;
        Ok(())
    })
}

/// Detach a process from a namespace.
pub fn detach(pid: u32, ns_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let ns = state.namespaces.iter_mut().find(|n| n.id == ns_id)
            .ok_or(KernelError::NotFound)?;
        let before = ns.processes.len();
        ns.processes.retain(|&p| p != pid);
        if ns.processes.len() == before { return Err(KernelError::NotFound); }
        state.total_detaches += 1;
        Ok(())
    })
}

/// Create a container with a set of namespaces.
pub fn create_container(name: &str, ns_types: &[NsType]) -> KernelResult<u32> {
    with_state(|state| {
        if state.containers.len() >= MAX_CONTAINERS { return Err(KernelError::ResourceExhausted); }
        if state.containers.iter().any(|c| c.name == name) { return Err(KernelError::AlreadyExists); }
        let now = crate::hpet::elapsed_ns();
        let cid = state.next_container_id;
        state.next_container_id += 1;
        let mut ns_ids = Vec::new();
        for &nst in ns_types {
            if state.namespaces.len() >= MAX_NAMESPACES { return Err(KernelError::ResourceExhausted); }
            let ns_id = state.next_ns_id;
            state.next_ns_id += 1;
            let ns_name = format!("{}_{}_{}", name, nst.label(), ns_id);
            state.namespaces.push(Namespace {
                id: ns_id, ns_type: nst, name: ns_name,
                isolation: IsolationLevel::Full, parent_id: None,
                processes: Vec::new(), created_ns: now,
            });
            ns_ids.push(ns_id);
        }
        state.containers.push(Container {
            id: cid, name: String::from(name), namespaces: ns_ids,
            root_pid: None, created_ns: now, running: false,
        });
        Ok(cid)
    })
}

/// Delete a container.
pub fn delete_container(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.containers.iter().position(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        let container = state.containers.remove(idx);
        // Remove associated namespaces (only if empty).
        for &ns_id in &container.namespaces {
            state.namespaces.retain(|n| n.id != ns_id || n.processes.is_empty());
        }
        Ok(())
    })
}

/// List all namespaces.
pub fn list_namespaces() -> Vec<Namespace> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.namespaces.clone())
}

/// List all containers.
pub fn list_containers() -> Vec<Container> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.containers.clone())
}

/// Get namespace by ID.
pub fn get_namespace(id: u32) -> Option<Namespace> {
    STATE.lock().as_ref().and_then(|s| s.namespaces.iter().find(|n| n.id == id).cloned())
}

/// Statistics: (ns_count, container_count, total_attaches, total_detaches, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.namespaces.len(), s.containers.len(), s.total_attaches, s.total_detaches, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("prociso::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_namespaces().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create namespace.
    let id = create_namespace(NsType::User, "test_user", IsolationLevel::Full, None).expect("create");
    assert!(id >= 4);
    assert!(create_namespace(NsType::User, "test_user", IsolationLevel::Full, None).is_err());
    crate::serial_println!("  [2/8] create: OK");

    // 3: Attach/detach.
    attach(100, id).expect("attach");
    let ns = get_namespace(id).expect("get");
    assert_eq!(ns.processes.len(), 1);
    assert!(attach(100, id).is_err()); // Duplicate.
    detach(100, id).expect("detach");
    let ns = get_namespace(id).expect("get2");
    assert_eq!(ns.processes.len(), 0);
    crate::serial_println!("  [3/8] attach/detach: OK");

    // 4: Create container.
    let cid = create_container("test_container", &[NsType::Mount, NsType::Pid, NsType::Net]).expect("cont");
    let containers = list_containers();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].namespaces.len(), 3);
    crate::serial_println!("  [4/8] container: OK");

    // 5: Delete container.
    delete_container(cid).expect("del_cont");
    assert_eq!(list_containers().len(), 0);
    crate::serial_println!("  [5/8] delete container: OK");

    // 6: Nested namespace.
    let parent = create_namespace(NsType::Mount, "parent_mnt", IsolationLevel::Partial, None).expect("parent");
    let child = create_namespace(NsType::Mount, "child_mnt", IsolationLevel::Full, Some(parent)).expect("child");
    let ns = get_namespace(child).expect("get3");
    assert_eq!(ns.parent_id, Some(parent));
    crate::serial_println!("  [6/8] nested: OK");

    // 7: Cannot delete with children.
    assert!(delete_namespace(parent).is_err());
    delete_namespace(child).expect("del_child");
    delete_namespace(parent).expect("del_parent");
    crate::serial_println!("  [7/8] hierarchy: OK");

    // 8: Stats.
    let (ns_count, cont_count, attaches, detaches, ops) = stats();
    assert!(ns_count >= 3);
    let _ = cont_count;
    assert!(attaches >= 1);
    assert!(detaches >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("prociso::self_test() — all 8 tests passed");
}
