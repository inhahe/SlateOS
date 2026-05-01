//! Process Control Block (PCB) — the kernel's representation of a process.
//!
//! Each process has a PCB that stores its address space root, capability
//! table, thread list, parent relationship, and exit status.
//!
//! ## Process IDs
//!
//! Process IDs are monotonically increasing `u64` values.  PID 0 is
//! reserved for the kernel "process" (the boot context).  PID 1 is
//! the init process.
//!
//! ## Lifecycle
//!
//! 1. `create()` — allocate PCB, address space, capability table.
//! 2. Load binary (ELF loader — future).
//! 3. Spawn initial thread.
//! 4. Process runs until all threads exit or it's killed.
//! 5. `destroy()` — reclaim address space, capability table, notify parent.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::cap::{self, CapTable, Rights, ResourceType};
use crate::error::{KernelError, KernelResult};
use crate::sched::task::TaskId;
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Process ID
// ---------------------------------------------------------------------------

/// A unique process identifier.
///
/// PIDs are monotonically increasing and never reused within a boot
/// session.  PID 0 is the kernel, PID 1 is init.
pub type ProcessId = u64;

/// Counter for generating unique process IDs.
/// Starts at 1 (PID 0 = kernel).
static NEXT_PID: AtomicU64 = AtomicU64::new(1);

fn alloc_pid() -> ProcessId {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Process credentials
// ---------------------------------------------------------------------------

/// Process credentials — identity and privilege information.
///
/// Every process has credentials that identify it in the user/group
/// model.  Credentials are inherited from the parent at spawn time
/// and can be changed by privileged processes.
///
/// UID 0 = root/system (full authority).
/// GID 0 = system group.
///
/// During early development, all processes run as uid=0 (root).
/// The user/group model is enforced once a login service exists.
#[derive(Debug, Clone)]
pub struct ProcessCredentials {
    /// User ID (0 = root/system).
    pub uid: u32,
    /// Primary group ID.
    pub gid: u32,
    /// Supplementary group IDs.
    pub groups: Vec<u32>,
}

impl ProcessCredentials {
    /// Create default (root) credentials.
    #[must_use]
    pub fn root() -> Self {
        Self {
            uid: 0,
            gid: 0,
            groups: Vec::new(),
        }
    }

    /// Create credentials for a specific user/group.
    #[must_use]
    pub fn new(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            gid,
            groups: Vec::new(),
        }
    }

    /// Check if this process runs as root.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.uid == 0
    }
}

// ---------------------------------------------------------------------------
// Process state
// ---------------------------------------------------------------------------

/// The current state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is being created (loading binary, setting up address space).
    Creating,
    /// Process is running (has at least one ready/running thread).
    Running,
    /// Process has exited (all threads done, waiting for parent to reap).
    Zombie,
}

// ---------------------------------------------------------------------------
// Process Control Block
// ---------------------------------------------------------------------------

/// The Process Control Block — one per process.
///
/// Stores everything the kernel needs to manage a process.
pub struct Process {
    /// Unique process ID.
    pub pid: ProcessId,
    /// Human-readable name (for debug output).
    pub name: String,
    /// Current state.
    pub state: ProcessState,
    /// Parent process ID (0 = kernel-spawned).
    pub parent: ProcessId,
    /// Thread IDs belonging to this process.
    pub threads: Vec<TaskId>,
    /// Per-process capability table.
    pub cap_table: CapTable,
    /// Exit code (set when all threads have exited).
    pub exit_code: Option<i32>,
    /// Process credentials (uid, gid, supplementary groups).
    pub credentials: ProcessCredentials,
    /// PML4 physical address for this process's address space.
    ///
    /// 0 means "uses the kernel address space" (for kernel-mode
    /// processes during early development).
    pub pml4_phys: u64,
    /// Task waiting to reap this process (if any).
    pub wait_task: Option<TaskId>,
}

impl Process {
    /// Create a new process (internal — use `create()` below).
    fn new(name: String, parent: ProcessId) -> Self {
        Self {
            pid: alloc_pid(),
            name,
            state: ProcessState::Creating,
            parent,
            threads: Vec::new(),
            cap_table: CapTable::new(),
            exit_code: None,
            credentials: ProcessCredentials::root(),
            pml4_phys: 0, // Kernel address space for now.
            wait_task: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Global process table
// ---------------------------------------------------------------------------

/// Global table of all processes.
///
/// Lock ordering: `PROCESS_TABLE` → `SCHED`.
static PROCESS_TABLE: Mutex<BTreeMap<ProcessId, Process>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new process.
///
/// The process starts in `Creating` state with an empty capability
/// table and no threads.  The caller should:
/// 1. Load a binary into the process's address space.
/// 2. Grant initial capabilities.
/// 3. Spawn the initial thread via `add_thread()`.
/// 4. Transition to `Running` state.
///
/// Returns the new process's PID.
pub fn create(name: &str, parent: ProcessId) -> ProcessId {
    let mut proc = Process::new(String::from(name), parent);

    // Allocate a per-process PML4 with kernel entries cloned.
    // If allocation fails, the process falls back to the kernel
    // address space (pml4_phys remains 0).
    match crate::mm::page_table::alloc_pml4() {
        Ok(pml4) => {
            proc.pml4_phys = pml4;
        }
        Err(e) => {
            crate::serial_println!(
                "[proc] WARNING: PML4 alloc failed for '{}': {:?} — using kernel AS",
                name, e
            );
        }
    }

    let pid = proc.pid;

    let mut table = PROCESS_TABLE.lock();
    table.insert(pid, proc);

    pid
}

/// Mark a process as running.
///
/// Called after the binary is loaded and the initial thread is spawned.
pub fn set_running(pid: ProcessId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.state = ProcessState::Running;
    Ok(())
}

/// Add a thread to a process.
pub fn add_thread(pid: ProcessId, task_id: TaskId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.threads.push(task_id);
    Ok(())
}

/// Remove a thread from a process.
///
/// If this was the last thread, the process enters Zombie state.
/// Returns `(is_zombie, wait_task)` — if zombie, the optional task ID
/// that should be woken (the parent waiting to reap this process).
pub fn remove_thread(
    pid: ProcessId,
    task_id: TaskId,
) -> KernelResult<(bool, Option<TaskId>)> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.threads.retain(|&t| t != task_id);

    if proc.threads.is_empty() && proc.state == ProcessState::Running {
        proc.state = ProcessState::Zombie;
        if proc.exit_code.is_none() {
            proc.exit_code = Some(0); // Default exit code.
        }
        let wake = proc.wait_task.take();
        return Ok((true, wake));
    }

    Ok((false, None))
}

/// Grant a capability to a process.
pub fn grant_capability(
    pid: ProcessId,
    resource_type: ResourceType,
    resource_id: u64,
    rights: Rights,
) -> KernelResult<cap::table::CapHandle> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.cap_table.insert(resource_type, resource_id, rights)
}

/// Check a capability for a process.
pub fn check_capability(
    pid: ProcessId,
    handle: cap::table::CapHandle,
    required: Rights,
) -> KernelResult<()> {
    let table = PROCESS_TABLE.lock();
    let proc = table
        .get(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.cap_table.check_rights(handle, required)?;
    Ok(())
}

/// Set the exit code for a process.
///
/// Typically called before the process transitions to Zombie state
/// (e.g., by the last exiting thread or by a kill operation).
pub fn set_exit_code(pid: ProcessId, code: i32) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.exit_code = Some(code);
    Ok(())
}

/// Try to reap (wait for) a zombie child process.
///
/// If the child process `child_pid` is a zombie:
/// - Returns `Ok(Some(exit_code))` and destroys the process.
///
/// If the child process exists but is still running:
/// - Returns `Ok(None)` (non-blocking — caller should block and retry).
///
/// If the child process doesn't exist:
/// - Returns `Err(NoSuchProcess)`.
///
/// The caller must be the parent of the child process (or PID 0 for
/// kernel-spawned processes).
pub fn try_reap(
    parent_pid: ProcessId,
    child_pid: ProcessId,
) -> KernelResult<Option<i32>> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get(&child_pid)
        .ok_or(KernelError::NoSuchProcess)?;

    // Verify parent relationship.
    if proc.parent != parent_pid {
        return Err(KernelError::PermissionDenied);
    }

    if proc.state == ProcessState::Zombie {
        let exit_code = proc.exit_code.unwrap_or(0);
        table.remove(&child_pid);
        Ok(Some(exit_code))
    } else {
        Ok(None) // Still running.
    }
}

/// Register a task to be woken when a process exits.
///
/// When the process transitions to Zombie, the scheduler should wake
/// this task.  Only one waiter per process.
pub fn set_wait_task(pid: ProcessId, task_id: TaskId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.wait_task = Some(task_id);
    Ok(())
}

/// Get and clear the wait task for a process.
///
/// Called when a process becomes a zombie to retrieve the task that
/// should be woken.
pub fn take_wait_task(pid: ProcessId) -> Option<TaskId> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    proc.wait_task.take()
}

/// Get the exit code of a zombie process.
#[allow(dead_code)]
pub fn exit_code(pid: ProcessId) -> Option<i32> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).and_then(|p| p.exit_code)
}

/// Get the parent PID of a process.
#[allow(dead_code)]
pub fn parent(pid: ProcessId) -> Option<ProcessId> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.parent)
}

/// Destroy a process, removing it from the table.
///
/// Called when the parent has reaped the zombie, or when the process
/// is forcefully killed.  Reclaims all physical memory used by the
/// process's address space (mapped frames, intermediate page tables,
/// and the PML4 itself).
pub fn destroy(pid: ProcessId) {
    // Remove exception handler registration (if any).
    crate::proc::exception::remove_handler(pid);

    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.remove(&pid) {
        // Free the entire user address space (mapped frames,
        // intermediate page tables, and the PML4 page).
        if proc.pml4_phys != 0 {
            // SAFETY: The process is being destroyed — no threads
            // are running in this address space, and no CPU has
            // this PML4 loaded in CR3.  All user-half pages were
            // allocated specifically for this process.
            unsafe {
                crate::mm::page_table::destroy_user_address_space(proc.pml4_phys);
            }
        }
    }
}

/// Look up a process name (for debug output).
#[allow(dead_code)]
pub fn name(pid: ProcessId) -> Option<String> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.name.clone())
}

/// Get the PML4 physical address for a process's address space.
///
/// Returns 0 if the process uses the kernel address space (no PML4
/// was allocated or the process doesn't exist).
pub fn get_pml4(pid: ProcessId) -> Option<u64> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.pml4_phys)
}

/// Check if a process holds a capability for a specific resource
/// with sufficient rights.
///
/// Searches the process's capability table for a valid entry matching
/// the resource type and ID with the required rights.
pub fn has_capability_for(
    pid: ProcessId,
    resource_type: ResourceType,
    resource_id: u64,
    required_rights: Rights,
) -> bool {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        proc.cap_table.has_resource(resource_type, resource_id, required_rights)
    } else {
        false
    }
}

/// Check if a process holds any capability of a given type with
/// sufficient rights, regardless of resource ID.
///
/// Used for "does this process have general filesystem access?" or
/// "can this process use the network?" style queries.
pub fn has_capability_type(
    pid: ProcessId,
    resource_type: ResourceType,
    required_rights: Rights,
) -> bool {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        proc.cap_table.has_capability_type(resource_type, required_rights)
    } else {
        false
    }
}

/// Get the number of valid capabilities a process holds.
pub fn cap_count(pid: ProcessId) -> Option<usize> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.cap_table.count())
}

/// Get the credentials for a process.
pub fn get_credentials(pid: ProcessId) -> Option<ProcessCredentials> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.credentials.clone())
}

/// Set the credentials for a process.
///
/// Only processes running as root (uid=0) or the kernel (PID 0
/// caller) should call this.  The authorization check is the
/// caller's responsibility.
pub fn set_credentials(
    pid: ProcessId,
    credentials: ProcessCredentials,
) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.credentials = credentials;
    Ok(())
}

/// Get the list of thread task IDs for a process.
///
/// Returns `None` if the process doesn't exist.  Returns an empty
/// `Vec` if the process exists but has no threads (zombie or creating).
pub fn get_threads(pid: ProcessId) -> Option<Vec<TaskId>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.threads.clone())
}

/// Get the state of a process.
#[allow(dead_code)]
pub fn state(pid: ProcessId) -> Option<ProcessState> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.state)
}

/// Get the number of live processes.
#[allow(dead_code)]
pub fn count() -> usize {
    let table = PROCESS_TABLE.lock();
    table.len()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run PCB self-tests.
pub fn self_test() -> KernelResult<()> {
    test_create_and_lookup()?;
    test_thread_lifecycle()?;
    test_capability_integration()?;
    test_destroy()?;
    test_reap_zombie()?;

    Ok(())
}

/// Test 1: create a process and look it up.
fn test_create_and_lookup() -> KernelResult<()> {
    let pid = create("test-proc", 0);

    let s = state(pid).ok_or(KernelError::InternalError)?;
    if s != ProcessState::Creating {
        serial_println!("[proc]   FAIL: initial state should be Creating");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    set_running(pid)?;
    let s = state(pid).ok_or(KernelError::InternalError)?;
    if s != ProcessState::Running {
        serial_println!("[proc]   FAIL: state should be Running");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    destroy(pid);
    serial_println!("[proc]   Create + lookup: OK");
    Ok(())
}

/// Test 2: add and remove threads, verify zombie transition.
fn test_thread_lifecycle() -> KernelResult<()> {
    let pid = create("thread-test", 0);
    set_running(pid)?;

    // Add two threads.
    add_thread(pid, 100)?;
    add_thread(pid, 200)?;

    // Remove first — process should still be running.
    let (zombie, _wake) = remove_thread(pid, 100)?;
    if zombie {
        serial_println!("[proc]   FAIL: should not be zombie with 1 thread left");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Remove last — process becomes zombie.
    let (zombie, _wake) = remove_thread(pid, 200)?;
    if !zombie {
        serial_println!("[proc]   FAIL: should be zombie with 0 threads");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    let s = state(pid).ok_or(KernelError::InternalError)?;
    if s != ProcessState::Zombie {
        serial_println!("[proc]   FAIL: state should be Zombie");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    destroy(pid);
    serial_println!("[proc]   Thread lifecycle: OK");
    Ok(())
}

/// Test 3: capability integration — grant and check.
fn test_capability_integration() -> KernelResult<()> {
    let pid = create("cap-test", 0);

    let handle = grant_capability(
        pid,
        ResourceType::Channel,
        42,
        Rights::READ | Rights::WRITE,
    )?;

    // Check should pass for READ.
    check_capability(pid, handle, Rights::READ)?;

    // Check should fail for EXECUTE.
    match check_capability(pid, handle, Rights::EXECUTE) {
        Err(KernelError::PermissionDenied) => {} // Expected.
        other => {
            serial_println!(
                "[proc]   FAIL: execute check should fail: {:?}",
                other
            );
            destroy(pid);
            return Err(KernelError::InternalError);
        }
    }

    // Type-level check: should find Channel+READ.
    if !has_capability_type(pid, ResourceType::Channel, Rights::READ) {
        serial_println!("[proc]   FAIL: has_capability_type should find Channel+READ");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Type-level check: should NOT find File (not granted).
    if has_capability_type(pid, ResourceType::File, Rights::READ) {
        serial_println!("[proc]   FAIL: has_capability_type should NOT find File");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Grant a File cap and re-check.
    grant_capability(pid, ResourceType::File, 0, Rights::READ | Rights::WRITE)?;
    if !has_capability_type(pid, ResourceType::File, Rights::READ) {
        serial_println!("[proc]   FAIL: has_capability_type should find File+READ after grant");
        destroy(pid);
        return Err(KernelError::InternalError);
    }
    if has_capability_type(pid, ResourceType::File, Rights::DELETE) {
        serial_println!("[proc]   FAIL: has_capability_type should NOT find File+DELETE");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    destroy(pid);
    serial_println!("[proc]   Capability integration: OK");
    Ok(())
}

/// Test 4: destroy removes the process.
fn test_destroy() -> KernelResult<()> {
    let pid = create("destroy-test", 0);
    destroy(pid);

    if state(pid).is_some() {
        serial_println!("[proc]   FAIL: process still exists after destroy");
        return Err(KernelError::InternalError);
    }

    serial_println!("[proc]   Destroy: OK");
    Ok(())
}

/// Test 5: reap a zombie child process.
fn test_reap_zombie() -> KernelResult<()> {
    // Parent creates a child.
    let parent_pid = create("reap-parent", 0);
    let child_pid = create("reap-child", parent_pid);

    set_running(child_pid)?;
    add_thread(child_pid, 900)?;

    // Try to reap before zombie — should return None.
    match try_reap(parent_pid, child_pid)? {
        None => {} // Expected: child still running.
        Some(code) => {
            serial_println!("[proc]   FAIL: reap should return None (still running), got {}", code);
            destroy(child_pid);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // Set exit code and make zombie.
    set_exit_code(child_pid, 42)?;
    let (zombie, _wake) = remove_thread(child_pid, 900)?;
    if !zombie {
        serial_println!("[proc]   FAIL: should be zombie after last thread exits");
        destroy(child_pid);
        destroy(parent_pid);
        return Err(KernelError::InternalError);
    }

    // Reap the zombie.
    match try_reap(parent_pid, child_pid)? {
        Some(42) => {} // Expected.
        other => {
            serial_println!("[proc]   FAIL: reap should return 42, got {:?}", other);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // Child should no longer exist (reaped).
    if state(child_pid).is_some() {
        serial_println!("[proc]   FAIL: child should be gone after reap");
        destroy(parent_pid);
        return Err(KernelError::InternalError);
    }

    // Wrong parent should fail.
    let child2 = create("reap-child-2", parent_pid);
    set_running(child2)?;
    add_thread(child2, 901)?;
    set_exit_code(child2, 0)?;
    let _ = remove_thread(child2, 901)?;

    match try_reap(99999, child2) {
        Err(KernelError::PermissionDenied) => {} // Expected.
        other => {
            serial_println!("[proc]   FAIL: wrong parent should fail: {:?}", other);
            destroy(child2);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    destroy(child2);
    destroy(parent_pid);
    serial_println!("[proc]   Reap zombie: OK");
    Ok(())
}
