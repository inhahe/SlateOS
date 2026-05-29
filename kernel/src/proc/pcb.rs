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
use crate::mm::vma::Vma;
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
    #[allow(dead_code)] // Public API — used when login/user management is implemented.
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
// Crash information — details about how a process died
// ---------------------------------------------------------------------------

/// Information about a process crash (unhandled exception).
///
/// Captured by the kernel when a userspace process takes an unhandled
/// hardware exception (access violation, divide-by-zero, etc.).  Stored
/// in the PCB and available to the parent via `SYS_PROCESS_CRASH_INFO`.
///
/// The init service manager uses this to distinguish normal exits from
/// crashes, and to log crash diagnostics for driver restart decisions.
#[derive(Debug, Clone, Copy)]
pub struct CrashInfo {
    /// The exception code that killed the process.
    pub exception_code: u64,
    /// Faulting instruction pointer (RIP at the time of the exception).
    pub faulting_rip: u64,
    /// Auxiliary value (e.g., page fault address for access violations,
    /// error code for GP faults).
    pub aux: u64,
    /// The thread that caused the crash.
    pub thread_id: TaskId,
}

/// Conventional exit code for processes killed by an unhandled exception.
///
/// Mirrors Unix convention (128 + signal number) but uses exception codes
/// instead of signal numbers.  The parent can distinguish normal exit
/// (exit_code >= 0) from crash (exit_code < 0 or == CRASH_EXIT_BASE + code).
///
/// We use negative exit codes for crashes: -(exception_code).
/// DivideError (1) → exit_code = -1
/// AccessViolation (8) → exit_code = -8
/// This is a clean, simple convention.  The parent service manager
/// checks `exit_code < 0` to detect crashes.
pub const fn crash_exit_code(exception_code: u64) -> i32 {
    // Negate the exception code.  Exception codes are small positive
    // integers (1-11), so this always fits in i32.
    #[allow(clippy::cast_possible_wrap)]
    let neg = -(exception_code as i32);
    neg
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
    /// Whether the process has signaled it is fully initialized.
    ///
    /// Set via `SYS_NOTIFY_READY` (508).  The init service manager
    /// uses this to know when a service has finished startup and is
    /// ready to accept requests.
    pub ready: bool,
    /// Per-process VMAs for lazy/demand-paged memory regions.
    ///
    /// Sorted by start address, no overlaps.  Used by the page fault
    /// handler to resolve user-space faults on lazy-allocated memory
    /// (regions mapped with `MAP_LAZY`).
    ///
    /// VMAs are added by `SYS_MMAP` with `MAP_LAZY` and removed by
    /// `SYS_MUNMAP`.  Stack growth is handled separately by the IDT
    /// handler — it doesn't use this VMA list.
    pub vmas: Vec<Vma>,
    /// Owned IPC handles — cleaned up when the process is reaped.
    ///
    /// Each entry is `(ResourceType, handle_raw)`.  IPC create syscalls
    /// register handles here; IPC close syscalls deregister them.
    /// On process death, all remaining handles are released.
    pub ipc_handles: Vec<(crate::cap::ResourceType, u64)>,
    /// Crash information — set when the process dies from an unhandled
    /// exception.  `None` for normal exits.  The parent can read this
    /// via `SYS_PROCESS_CRASH_INFO` to get diagnostics.
    pub crash_info: Option<CrashInfo>,
    /// Initial file descriptor mappings inherited from parent.
    ///
    /// Each entry is `(posix_fd_number, handle_type, kernel_handle_id)`.
    /// Set by `SYS_PROCESS_SPAWN_EX` when the parent passes an fd map.
    /// The child's POSIX layer reads this via `SYS_PROCESS_GET_INITIAL_FDS`
    /// during startup and clears it (one-shot).
    ///
    /// `handle_type` is one of the `fd_handle_type` constants (FILE, PIPE,
    /// CONSOLE, etc.) and tells the child how to interpret the handle.
    ///
    /// The kernel handles stored here are *duplicates* of the parent's
    /// handles — the child owns them independently.  If the child never
    /// reads them (e.g., a non-POSIX process), they are cleaned up when
    /// the process is reaped.
    pub initial_fds: Vec<(i32, u8, u64)>,
    /// Initial command-line arguments for the child process.
    ///
    /// Each element is one argument as a byte string (NOT null-terminated
    /// in storage — the null terminators are added when copying out).
    /// Set by `SYS_PROCESS_SPAWN_EX` when the parent passes argv data.
    /// The child's POSIX layer reads this via `SYS_PROCESS_GET_ARGS`
    /// during startup and clears it (one-shot).
    pub initial_argv: Vec<Vec<u8>>,
    /// Initial environment variables for the child process.
    ///
    /// Same format as `initial_argv` — each element is one `KEY=value`
    /// byte string.
    pub initial_envp: Vec<Vec<u8>>,
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
            ready: false,
            vmas: Vec::new(),
            ipc_handles: Vec::new(),
            crash_info: None,
            initial_fds: Vec::new(),
            initial_argv: Vec::new(),
            initial_envp: Vec::new(),
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
///
/// When a process becomes a zombie, all its living children are
/// reparented to PID 1 (init) and registered as orphans so init
/// can reap them when they eventually exit.
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

        // Reparent living children to init (PID 1).
        //
        // Any child whose parent just died becomes an orphan.  We
        // change its parent field to INIT_PID so that init's
        // try_reap() calls will satisfy the parent check.
        let mut orphan_pids = Vec::new();
        for child in table.values_mut() {
            if child.parent == pid && child.pid != pid {
                child.parent = crate::initproc::INIT_PID as ProcessId;
                orphan_pids.push(child.pid);
            }
        }
        // Drop the lock before calling initproc to avoid potential
        // lock ordering issues (PROCESS_TABLE → initproc STATE).
        drop(table);

        for &orphan_pid in &orphan_pids {
            #[allow(clippy::cast_possible_truncation)]
            let _ = crate::initproc::register_orphan(orphan_pid as u32);
        }

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

    let result = proc.cap_table.insert(resource_type, resource_id, rights);

    // Audit the grant operation.
    match &result {
        Ok(handle) => {
            #[allow(clippy::cast_possible_truncation)]
            cap::audit::record_grant(pid as u32, handle.raw() as u32, rights.raw() as u8);
        }
        Err(_) => {
            // Grant failed (table full) — not a security event.
        }
    }

    result
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

    match proc.cap_table.check_rights(handle, required) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Audit the denial.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            cap::audit::record_deny(
                pid as u32,
                handle.raw() as u32,
                required.raw() as u8,
                (-e.code()) as u32,
            );
            Err(e)
        }
    }
}

/// Remove capability entries from a process table (for cap transfer).
///
/// Validates all handles first (all-or-nothing).  If any handle is
/// invalid, no changes are made.  On success, returns the detached
/// entries in the same order as `handles`.
///
/// # Errors
///
/// - `NoSuchProcess` — PID not found.
/// - `InvalidCapability` — one of the handles is invalid or revoked.
pub fn remove_caps(
    pid: ProcessId,
    handles: &[u64],
) -> KernelResult<Vec<cap::table::CapEntry>> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    // Validate all first.
    for &raw in handles {
        let h = cap::table::CapHandle::from_raw(raw);
        proc.cap_table.lookup(h)?;
    }

    // Remove all (guaranteed to succeed since we just validated).
    let mut entries = Vec::new();
    for &raw in handles {
        let h = cap::table::CapHandle::from_raw(raw);
        if let Some(entry) = proc.cap_table.remove(h) {
            // Audit: capability dropped/transferred from this process.
            #[allow(clippy::cast_possible_truncation)]
            cap::audit::record(
                cap::audit::AuditOp::Drop,
                pid as u32,
                raw as u32,
                entry.rights.raw() as u8,
                0,
                0,
            );
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Insert capability entries into a process table (for cap transfer).
///
/// Used by the IPC layer when delivering messages that carry
/// transferred capabilities.
///
/// # Returns
///
/// A vector of the new handle values assigned in the receiver's table.
/// If the table is full for some entries, those are dropped (lost) and
/// a shorter vector is returned.
pub fn insert_caps(
    pid: ProcessId,
    entries: &[(crate::cap::ResourceType, u64, Rights)],
) -> KernelResult<Vec<u64>> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    let mut new_handles = Vec::new();
    for &(resource_type, resource_id, rights) in entries {
        match proc.cap_table.insert(resource_type, resource_id, rights) {
            Ok(h) => new_handles.push(h.raw()),
            Err(_) => {
                // Table full — silently drop this entry.
                // The caller can detect this by comparing counts.
            }
        }
    }

    Ok(new_handles)
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

/// Record crash information for a process killed by an unhandled exception.
///
/// Sets the crash info and the exit code to a negative value derived
/// from the exception code.  Called by the exception handler just
/// before killing the process.
///
/// The exit code convention is: -(exception_code).  This means:
/// - exit_code >= 0: normal exit
/// - exit_code < 0: crash (negated exception code)
///
/// The parent can call `SYS_PROCESS_CRASH_INFO` to get full details
/// (exception code, faulting RIP, auxiliary value).
pub fn set_crash_info(
    pid: ProcessId,
    info: CrashInfo,
) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.exit_code = Some(crash_exit_code(info.exception_code));
    proc.crash_info = Some(info);
    Ok(())
}

/// Get crash information for a zombie process.
///
/// Returns `None` if the process exited normally (no crash) or if the
/// process doesn't exist.  Must be called before reaping — the crash
/// info is destroyed when the process is reaped.
pub fn get_crash_info(pid: ProcessId) -> Option<CrashInfo> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).and_then(|p| p.crash_info)
}

/// Exit information returned by `try_reap`.
///
/// Contains the exit code and optional crash details (if the process
/// died from an unhandled exception).
#[derive(Debug, Clone)]
pub struct ExitInfo {
    /// Process exit code.  Normal exit: >= 0.  Crash: < 0 (negated
    /// exception code).
    pub exit_code: i32,
    /// Crash details (exception code, faulting address, etc.).
    /// `None` for normal exits.
    pub crash: Option<CrashInfo>,
}

/// Try to reap (wait for) a zombie child process.
///
/// If the child process `child_pid` is a zombie:
/// - Returns `Ok(Some(ExitInfo))` and destroys the process (frees
///   address space, DMA buffers, IPC handles, capability table).
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
) -> KernelResult<Option<ExitInfo>> {
    // Phase 1: Under PROCESS_TABLE lock — verify state, extract
    // process info, and remove from table.  We must extract all
    // fields needed for cleanup before dropping the lock.
    #[allow(clippy::type_complexity)]
    let reaped: Option<(
        ExitInfo,
        u64,
        Vec<(crate::cap::ResourceType, u64)>,
        Vec<(i32, u8, u64)>,
    )>;

    {
        let mut table = PROCESS_TABLE.lock();
        let proc = table
            .get(&child_pid)
            .ok_or(KernelError::NoSuchProcess)?;

        // Verify parent relationship.
        if proc.parent != parent_pid {
            return Err(KernelError::PermissionDenied);
        }

        if proc.state != ProcessState::Zombie {
            return Ok(None); // Still running.
        }

        let exit_code = proc.exit_code.unwrap_or(0);
        let crash = proc.crash_info;
        let pml4_phys = proc.pml4_phys;

        // Extract the IPC handle list and initial fds before removing.
        let mut removed = table.remove(&child_pid);
        let ipc_handles = removed
            .as_mut()
            .map(|p| core::mem::take(&mut p.ipc_handles))
            .unwrap_or_default();
        let initial_fds = removed
            .as_mut()
            .map(|p| core::mem::take(&mut p.initial_fds))
            .unwrap_or_default();

        let info = ExitInfo { exit_code, crash };
        reaped = Some((info, pml4_phys, ipc_handles, initial_fds));
    }
    // PROCESS_TABLE lock dropped here.

    if let Some((info, pml4_phys, ipc_handles, initial_fds)) = reaped {
        // Phase 2: Cleanup without holding PROCESS_TABLE lock.
        // This avoids ABBA deadlocks with exception handler / DMA / IPC locks.
        destroy_process_resources(child_pid, pml4_phys, &ipc_handles, &initial_fds);
        Ok(Some(info))
    } else {
        Ok(None)
    }
}

/// Mark a process as "ready" (fully initialized and accepting requests).
///
/// Called by the process itself via `SYS_NOTIFY_READY`.  The parent
/// (typically init's service manager) can query this flag to know
/// when a service has completed startup.
pub fn set_ready(pid: ProcessId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.ready = true;
    Ok(())
}

/// Check whether a process has signaled readiness.
///
/// Returns `Ok(true)` if the process exists and has called
/// `SYS_NOTIFY_READY`, `Ok(false)` if it exists but hasn't, or
/// `Err(NoSuchProcess)` if the PID is not found.
pub fn is_ready(pid: ProcessId) -> KernelResult<bool> {
    let table = PROCESS_TABLE.lock();
    let proc = table
        .get(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    Ok(proc.ready)
}

/// Check whether a process exists and is in the Running state.
///
/// Returns `true` if the PID is found in the process table and its state
/// is `ProcessState::Running`.  Returns `false` if the process does not
/// exist, is a zombie (exited but not yet reaped), or is still being
/// created.  This is used by the driver monitor (`drvmon`) to detect
/// crashed driver processes.
pub fn is_process_running(pid: ProcessId) -> bool {
    let table = PROCESS_TABLE.lock();
    match table.get(&pid) {
        Some(proc) => proc.state == ProcessState::Running,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Per-process VMA management (for lazy/demand-paged allocations)
// ---------------------------------------------------------------------------

/// Add a VMA to a process's per-process VMA list.
///
/// Used by `SYS_MMAP` with `MAP_LAZY` to register a demand-paged
/// memory region.  The VMA must not overlap any existing VMA in the
/// process.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if the PID doesn't exist.
/// - [`KernelError::BadAlignment`] if start/end are not frame-aligned.
/// - [`KernelError::AlreadyExists`] if the range overlaps an existing VMA.
pub fn add_vma(pid: ProcessId, vma: Vma) -> KernelResult<()> {
    use crate::mm::page_table::VirtAddr;

    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;

    // Validate alignment.
    if !VirtAddr::new(vma.start).is_frame_aligned()
        || !VirtAddr::new(vma.end).is_frame_aligned()
    {
        return Err(KernelError::BadAlignment);
    }
    if vma.end <= vma.start {
        return Err(KernelError::InvalidArgument);
    }

    // Check for overlaps.
    for existing in &proc.vmas {
        if vma.start < existing.end && vma.end > existing.start {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Insert sorted by start address.
    let pos = proc.vmas
        .binary_search_by_key(&vma.start, |v| v.start)
        .unwrap_or_else(|p| p);
    proc.vmas.insert(pos, vma);

    Ok(())
}

/// Remove a VMA from a process's VMA list by start address.
///
/// Returns `true` if a VMA was found and removed, `false` otherwise.
pub fn remove_vma(pid: ProcessId, start: u64) -> bool {
    let mut table = PROCESS_TABLE.lock();
    let Some(proc) = table.get_mut(&pid) else {
        return false;
    };

    if let Ok(idx) = proc.vmas.binary_search_by_key(&start, |v| v.start) {
        proc.vmas.remove(idx);
        true
    } else {
        false
    }
}

/// Resolve a user-space page fault against a process's VMA list.
///
/// Called from the page fault handler (IDT vector 14) when a user-mode
/// fault occurs on a lazy-allocated region.  This function:
///
/// 1. Looks up the faulting address in the process's VMA list.
/// 2. Checks permissions against the error code.
/// 3. For Anonymous VMAs: allocates a frame, zeroes it, maps it.
///
/// Uses `try_lock()` to avoid deadlock if the process table is already
/// held (e.g., from a syscall that triggered a fault).
///
/// Returns `true` if the fault was resolved, `false` if not.
pub fn try_resolve_fault(pid: ProcessId, fault_addr: u64, error_code: u64) -> bool {
    use crate::mm::fault::PageFaultError;
    use crate::mm::frame::{self, FRAME_SIZE};
    use crate::mm::page_table::{self, PageFlags, VirtAddr};
    use crate::mm::vma::VmaKind;

    let error = PageFaultError::new(error_code);

    // Reserved-bit violations are never resolvable.
    if error.is_reserved() {
        return false;
    }

    // For present + write faults, try Copy-on-Write resolution.
    // A present page with the COW bit set means this page is shared
    // and needs to be copied on first write.
    if error.is_present() && error.is_write() {
        let Some(table) = PROCESS_TABLE.try_lock() else {
            return false;
        };
        let Some(proc) = table.get(&pid) else {
            return false;
        };
        let pml4_phys = proc.pml4_phys;
        drop(table); // Release lock before CoW resolution (it allocates).

        if pml4_phys != 0 {
            if crate::mm::cow::resolve_cow_fault(pml4_phys, fault_addr).is_ok() {
                return true; // CoW resolved — retry instruction.
            }
        }
        return false;
    }

    // Only handle not-present faults (demand paging).
    if error.is_present() {
        return false;
    }

    // Try to acquire the process table lock.  If it's already held,
    // we can't resolve (avoid deadlock).
    let Some(table) = PROCESS_TABLE.try_lock() else {
        return false;
    };
    let Some(proc) = table.get(&pid) else {
        return false;
    };

    // Look up the VMA containing the fault address.
    let idx = match proc.vmas.binary_search_by_key(&fault_addr, |v| v.start) {
        Ok(i) => i,
        Err(0) => return false,
        #[allow(clippy::arithmetic_side_effects)]
        Err(i) => i - 1,
    };
    let Some(vma) = proc.vmas.get(idx) else {
        return false;
    };
    if !vma.contains(fault_addr) {
        return false;
    }

    // Only demand-page Anonymous and Stack VMAs.
    match vma.kind {
        VmaKind::Anonymous | VmaKind::Stack => {}
        VmaKind::Guard | VmaKind::Fixed => return false,
    }

    let flags = vma.flags;
    let pml4_phys = proc.pml4_phys;

    // Permission checks.
    if error.is_write() && !flags.contains(PageFlags::WRITABLE) {
        return false;
    }
    if error.is_instruction_fetch() && flags.contains(PageFlags::NO_EXECUTE) {
        return false;
    }

    // Drop the process table lock before doing allocation + mapping
    // (those acquire the frame allocator and page table locks).
    drop(table);

    if pml4_phys == 0 {
        // No user address space — can't resolve.
        return false;
    }

    // Allocate, zero, and map a frame.
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return false,
    };

    #[allow(clippy::arithmetic_side_effects)]
    let frame_base = fault_addr & !(FRAME_SIZE as u64 - 1);
    let virt = VirtAddr::new(frame_base);

    // Enforce cgroup memory limits before allocating.
    //
    // If this process belongs to a cgroup with a memory limit, charge
    // one frame before allocation.  The charge is released when the
    // frame is freed (via the per-frame cgroup tracking in frame.rs).
    // If the group is over its limit, reject the fault — the process
    // will receive SIGSEGV (or our equivalent structured exception).
    if crate::cgroup::try_charge_current_mem(1).is_err() {
        return false;
    }

    let phys_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(_) => {
            // Alloc failed — uncharge the frame we pre-charged.
            crate::cgroup::uncharge_current_mem(1);
            return false;
        }
    };

    // Zero the frame via HHDM.
    // SAFETY: phys_frame.to_virt(hhdm) is valid HHDM mapping.
    // We have exclusive ownership of this freshly-allocated frame.
    unsafe {
        let hhdm_ptr = phys_frame.to_virt(hhdm) as *mut u8;
        core::ptr::write_bytes(hhdm_ptr, 0, FRAME_SIZE);
    }

    // Map the frame.
    // SAFETY: pml4_phys is the process's valid PML4, phys_frame is
    // freshly allocated, virt is within a VMA that permits this mapping.
    let map_result = unsafe {
        page_table::map_frame(pml4_phys, virt, phys_frame, flags)
    };

    if map_result.is_err() {
        // Map failed — free the frame.
        // SAFETY: phys_frame was just allocated and not exposed.
        let _ = unsafe { frame::free_frame(phys_frame) };
        return false;
    }

    // Flush TLB so the CPU sees the new mapping.
    // SAFETY: invlpg is always safe in ring 0.
    unsafe {
        page_table::flush_frame(virt);
    }

    // Register the new page as reclaimable so the swap subsystem's
    // Clock algorithm can evict it under memory pressure.
    crate::mm::swap::register_reclaimable(pml4_phys, frame_base, flags);

    // Register reverse mapping so the compaction subsystem can find
    // and migrate this frame.  Without rmap entries, compaction cannot
    // relocate demand-paged user pages.  The mm-zone rmap wiring
    // covers cow.rs, swap.rs, and compact.rs; this covers the initial
    // demand-page allocation in the process zone.
    crate::mm::rmap::add(phys_frame.addr(), pml4_phys, frame_base);

    serial_println!(
        "[fault] Demand-paged user frame for pid {} at {:#x}",
        pid, frame_base
    );
    true
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
#[allow(dead_code)] // Public API — called when wait/exit is fully wired.
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

/// Internal: release all resources associated with a process.
///
/// Called from `try_reap()` after the process has been removed from
/// `PROCESS_TABLE`.  Must NOT hold `PROCESS_TABLE` lock (this function
/// acquires locks in other subsystems).
///
/// Releases:
/// - Exception handler registration
/// - IPC handles (channels, pipes, eventfds, etc.)
/// - Namespace attachment (idempotent — already detached in `on_thread_exit`)
/// - DMA buffers
/// - User address space (page tables + physical frames)
fn destroy_process_resources(
    pid: ProcessId,
    pml4_phys: u64,
    ipc_handles: &[(crate::cap::ResourceType, u64)],
    initial_fds: &[(i32, u8, u64)],
) {
    // Remove exception handler registration (if any).
    crate::proc::exception::remove_handler(pid);

    // Drop any signal state (pending set, blocked mask, trampoline).
    crate::proc::signal::remove(pid);

    // Close all IPC handles owned by this process.
    crate::ipc::cleanup_handles(ipc_handles);

    // Close any unclaimed initial fd handles.
    //
    // If the child process never called SYS_PROCESS_GET_INITIAL_FDS
    // (e.g., it crashed before init, or is a non-POSIX process), the
    // duplicated handles are still in the global table.  Close them
    // now to avoid handle leaks.
    //
    // Console handles are virtual (no kernel resource to free).
    // Pipe and eventfd handles are ref-counted; spawn dup'd the
    // parent ref into the child, so the child's `close()` only drops
    // its own reference (not the parent's).
    // File handles were duped via `fs::handle::dup()` and must be
    // closed.
    for &(_fd, handle_type, handle) in initial_fds {
        match handle_type {
            crate::proc::spawn::fd_handle_type::CONSOLE => {
                // Virtual handle — nothing to close.
            }
            crate::proc::spawn::fd_handle_type::PIPE => {
                // Spawn dup'd the parent's pipe ref (per-end refcount);
                // closing here drops just that ref.  If userspace
                // already claimed the handle via the initial_fds
                // syscall, this branch isn't reached (the vec is
                // emptied at claim time).
                crate::ipc::pipe::close(
                    crate::ipc::pipe::PipeHandle::from_raw(handle),
                );
            }
            crate::proc::spawn::fd_handle_type::EVENTFD => {
                // Spawn dup'd the parent's eventfd ref into the child;
                // closing here drops that ref.  If userspace already
                // claimed the handle via SYS_PROCESS_GET_INITIAL_FDS
                // and put it into the fd-table, the fd-table layer
                // owns the close instead and this branch is unreached
                // because `initial_fds` is emptied at claim time.
                crate::ipc::eventfd::close(
                    crate::ipc::eventfd::EventFdHandle::from_raw(handle),
                );
            }
            _ => {
                // FILE, TCP_SOCKET, UDP_SOCKET, and any unknown types —
                // close via the file handle table.
                let _ = crate::fs::handle::close(handle);
            }
        }
    }

    // Detach from namespace (idempotent — may already be done
    // during zombie transition, but safe to call again).
    crate::ipc::namespace::detach(pid);

    // Free address space resources.
    if pml4_phys != 0 {
        // Free DMA buffers allocated for this process before
        // destroying the address space (DMA buffers are tracked
        // separately from normal page table entries).
        crate::mm::dma::free_all_for_process(pml4_phys);

        // Free the entire user address space (mapped frames,
        // intermediate page tables, and the PML4 page).
        // SAFETY: The process is being destroyed — no threads
        // are running in this address space, and no CPU has
        // this PML4 loaded in CR3.  All user-half pages were
        // allocated specifically for this process.
        unsafe {
            crate::mm::page_table::destroy_user_address_space(pml4_phys);
        }
    }
}

/// Destroy a process, removing it from the table and freeing resources.
///
/// Called when the parent has reaped the zombie, or when the process
/// is forcefully killed.  Reclaims all physical memory used by the
/// process's address space (mapped frames, intermediate page tables,
/// and the PML4 itself), plus IPC handles and exception registrations.
pub fn destroy(pid: ProcessId) {
    // Extract the process from the table.
    let removed;
    {
        let mut table = PROCESS_TABLE.lock();
        removed = table.remove(&pid);
    }
    // PROCESS_TABLE lock dropped — safe to acquire other locks.

    if let Some(proc) = removed {
        destroy_process_resources(pid, proc.pml4_phys, &proc.ipc_handles, &proc.initial_fds);
    }
}

/// Look up a process name (for debug output).
#[allow(dead_code)]
pub fn name(pid: ProcessId) -> Option<String> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.name.clone())
}

// ---------------------------------------------------------------------------
// IPC handle tracking
// ---------------------------------------------------------------------------

/// Register an IPC handle as owned by a process.
///
/// Called by IPC create syscalls so the kernel can release the handle
/// if the process dies without explicitly closing it.
pub fn register_ipc_handle(pid: ProcessId, resource_type: ResourceType, handle_raw: u64) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.ipc_handles.push((resource_type, handle_raw));
    }
}

/// Deregister an IPC handle from a process (handle was explicitly closed).
///
/// Removes the first matching `(resource_type, handle_raw)` entry.
/// No-op if the handle isn't found (e.g., kernel-owned handles).
pub fn deregister_ipc_handle(pid: ProcessId, resource_type: ResourceType, handle_raw: u64) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        if let Some(pos) = proc.ipc_handles.iter().position(|&(rt, h)| {
            rt == resource_type && h == handle_raw
        }) {
            proc.ipc_handles.swap_remove(pos);
        }
    }
}

// ---------------------------------------------------------------------------
// Initial fd mapping (for fd inheritance across spawn)
// ---------------------------------------------------------------------------

/// Store initial file descriptor mappings in a child process's PCB.
///
/// Called by `spawn_process()` when the parent passes an fd map.
/// Each entry is `(posix_fd_number, handle_type, kernel_handle_id)`
/// where the handle is a *duplicate* that the child owns and
/// `handle_type` is one of the `fd_handle_type` constants.
pub fn set_initial_fds(pid: ProcessId, fds: Vec<(i32, u8, u64)>) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.initial_fds = fds;
    }
}

/// Take (move out) the initial fd mappings from a process's PCB.
///
/// Returns the fd map and clears it in the PCB.  This is a one-shot
/// operation — the child calls `SYS_PROCESS_GET_INITIAL_FDS` once
/// during startup, and subsequent calls return an empty vec.
///
/// Each entry is `(posix_fd_number, handle_type, kernel_handle_id)`.
pub fn take_initial_fds(pid: ProcessId) -> Vec<(i32, u8, u64)> {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        core::mem::take(&mut proc.initial_fds)
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Initial argv/envp (for argument passing across spawn)
// ---------------------------------------------------------------------------

/// Maximum total bytes of argv + envp data per process (256 KiB).
///
/// Prevents a parent from allocating unbounded kernel heap for a child
/// that may never read the data.
const MAX_ARGS_BYTES: usize = 256 * 1024;

/// Store initial arguments and environment in a child process's PCB.
///
/// Called by `spawn_process()` when the parent passes argv/envp data.
/// Returns `Err(InvalidArgument)` if the total data exceeds `MAX_ARGS_BYTES`.
pub fn set_initial_args(
    pid: ProcessId,
    argv: Vec<Vec<u8>>,
    envp: Vec<Vec<u8>>,
) -> KernelResult<()> {
    // Check total size including null terminators.  When the child
    // retrieves these via SYS_PROCESS_GET_ARGS, each string gets a
    // null terminator appended (len + 1 per entry).  We must account
    // for that here so the size check is consistent.
    let total: usize = argv.iter().map(|a| a.len().saturating_add(1)).sum::<usize>()
        + envp.iter().map(|e| e.len().saturating_add(1)).sum::<usize>();
    if total > MAX_ARGS_BYTES {
        return Err(KernelError::InvalidArgument);
    }

    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.initial_argv = argv;
        proc.initial_envp = envp;
        Ok(())
    } else {
        Err(KernelError::NoSuchProcess)
    }
}

/// Take (move out) the initial argv/envp from a process's PCB.
///
/// Returns `(argv, envp)` and clears them in the PCB.  One-shot:
/// subsequent calls return empty vecs.
pub fn take_initial_args(pid: ProcessId) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        let argv = core::mem::take(&mut proc.initial_argv);
        let envp = core::mem::take(&mut proc.initial_envp);
        (argv, envp)
    } else {
        (Vec::new(), Vec::new())
    }
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
#[allow(dead_code)] // Public API — called when login/user management lands.
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
        Some(info) => {
            serial_println!("[proc]   FAIL: reap should return None (still running), got {}", info.exit_code);
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
        Some(info) if info.exit_code == 42 => {
            // Normal exit — no crash info expected.
            if info.crash.is_some() {
                serial_println!("[proc]   FAIL: normal exit should have no crash info");
                destroy(parent_pid);
                return Err(KernelError::InternalError);
            }
        }
        other => {
            serial_println!("[proc]   FAIL: reap should return exit_code=42, got {:?}",
                other.map(|i| i.exit_code));
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

    // --- Test 8: Crash info on process death ---
    serial_println!("[proc]   Testing crash info...");
    let crash_parent = create("crash-parent", 0);
    let crash_child = create("crash-child", crash_parent);
    set_running(crash_child)?;
    add_thread(crash_child, 950)?;

    // Simulate a crash: set crash info (AccessViolation at 0xDEAD).
    let info = CrashInfo {
        exception_code: 8, // AccessViolation
        faulting_rip: 0xDEAD_BEEF,
        aux: 0xBAD_FA6E,
        thread_id: 950,
    };
    set_crash_info(crash_child, info)?;

    // Verify crash info is queryable before reaping.
    match get_crash_info(crash_child) {
        Some(ci) => {
            if ci.exception_code != 8 || ci.faulting_rip != 0xDEAD_BEEF || ci.aux != 0xBAD_FA6E {
                serial_println!("[proc]   FAIL: crash info mismatch");
                destroy(crash_child);
                destroy(crash_parent);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            serial_println!("[proc]   FAIL: crash info should exist");
            destroy(crash_child);
            destroy(crash_parent);
            return Err(KernelError::InternalError);
        }
    }

    // Verify exit code is negative (crash convention).
    {
        let table = PROCESS_TABLE.lock();
        let proc = table.get(&crash_child).expect("crash child exists");
        let code = proc.exit_code.unwrap_or(0);
        if code >= 0 {
            serial_println!("[proc]   FAIL: crash exit code should be negative, got {}", code);
            drop(table);
            destroy(crash_child);
            destroy(crash_parent);
            return Err(KernelError::InternalError);
        }
        if code != -8 {
            serial_println!("[proc]   FAIL: crash exit code should be -8, got {}", code);
            drop(table);
            destroy(crash_child);
            destroy(crash_parent);
            return Err(KernelError::InternalError);
        }
    }

    // Make zombie and reap — crash info should be in ExitInfo.
    let (zombie, _) = remove_thread(crash_child, 950)?;
    if !zombie {
        serial_println!("[proc]   FAIL: crash child should be zombie");
        destroy(crash_child);
        destroy(crash_parent);
        return Err(KernelError::InternalError);
    }
    match try_reap(crash_parent, crash_child)? {
        Some(exit_info) => {
            if exit_info.exit_code != -8 {
                serial_println!("[proc]   FAIL: reap crash exit_code should be -8, got {}", exit_info.exit_code);
                destroy(crash_parent);
                return Err(KernelError::InternalError);
            }
            match exit_info.crash {
                Some(ci) => {
                    if ci.exception_code != 8 || ci.faulting_rip != 0xDEAD_BEEF {
                        serial_println!("[proc]   FAIL: reap crash info mismatch");
                        destroy(crash_parent);
                        return Err(KernelError::InternalError);
                    }
                }
                None => {
                    serial_println!("[proc]   FAIL: reap should include crash info");
                    destroy(crash_parent);
                    return Err(KernelError::InternalError);
                }
            }
        }
        None => {
            serial_println!("[proc]   FAIL: reap should succeed (zombie)");
            destroy(crash_parent);
            return Err(KernelError::InternalError);
        }
    }

    destroy(crash_parent);
    serial_println!("[proc]   Crash info: OK");

    Ok(())
}
