//! Task monitor — process monitoring and task management subsystem.
//!
//! Provides a centralized registry for tracking running tasks (processes),
//! their states, resource usage, and priority levels.  This is the kernel-
//! side backing store for process explorers, taskbars, and system monitors.
//!
//! ## Architecture
//!
//! ```text
//! kernel::proc::spawn()
//!   -> taskmon::register_task(name, priority, parent, user)
//!   -> returns pid
//!
//! scheduler tick / accounting
//!   -> taskmon::update_task_usage(pid, cpu_percent, memory_kb)
//!   -> taskmon::update_resources(total_cpu, used_mem, total_mem)
//!
//! process exit / signal
//!   -> taskmon::kill_task(pid)    -- marks Zombie
//!   -> taskmon::suspend_task(pid) -- marks Stopped
//!   -> taskmon::resume_task(pid)  -- marks Running
//!
//! userspace queries (procfs, process explorer, taskbar)
//!   -> taskmon::list_tasks()
//!   -> taskmon::get_task(pid)
//!   -> taskmon::get_resources()
//! ```
//!
//! ## Integration Points
//!
//! - **proc**: calls `register_task` / `kill_task` on process lifecycle
//! - **sched**: calls `update_task_usage` on accounting ticks
//! - **procfs**: reads `list_tasks` / `get_resources` for `/proc` entries
//! - **taskbar**: reads task list for running-app display
//! - **sysdiag**: reads resources for system health panels

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of tracked tasks.
const MAX_TASKS: usize = 500;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Execution state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Actively running or ready to run.
    Running,
    /// Sleeping (waiting for I/O, timer, or event).
    Sleeping,
    /// Stopped (suspended by user or debugger).
    Stopped,
    /// Zombie (exited but not yet reaped by parent).
    Zombie,
    /// Idle (kernel idle task, only runs when nothing else can).
    Idle,
}

impl TaskState {
    /// Human-readable label for this state.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Running  => "Running",
            Self::Sleeping => "Sleeping",
            Self::Stopped  => "Stopped",
            Self::Zombie   => "Zombie",
            Self::Idle     => "Idle",
        }
    }
}

/// Priority class for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    /// Hard real-time — highest priority, preempts everything.
    RealTime,
    /// High priority — system services and latency-sensitive apps.
    High,
    /// Normal priority — default for user applications.
    Normal,
    /// Below normal — background work that should yield to Normal.
    BelowNormal,
    /// Low priority — best-effort background processing.
    Low,
    /// Idle priority — only runs when CPU is otherwise idle.
    Idle,
}

impl TaskPriority {
    /// Human-readable label for this priority class.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RealTime    => "RealTime",
            Self::High        => "High",
            Self::Normal      => "Normal",
            Self::BelowNormal => "BelowNormal",
            Self::Low         => "Low",
            Self::Idle        => "Idle",
        }
    }
}

/// Information about a single task (process).
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Process ID.
    pub pid: u32,
    /// Task name (executable or service name).
    pub name: String,
    /// Current execution state.
    pub state: TaskState,
    /// Priority class.
    pub priority: TaskPriority,
    /// CPU usage as integer 0-10000 representing 0.00%-100.00%.
    pub cpu_percent: u32,
    /// Resident memory in KiB.
    pub memory_kb: u64,
    /// Number of threads in this task.
    pub threads: u32,
    /// Parent process ID (0 for the root kernel task).
    pub parent_pid: u32,
    /// User/owner name.
    pub user: String,
    /// Timestamp (nanoseconds since boot) when the task was created.
    pub started_ns: u64,
}

/// Aggregate system resource snapshot.
#[derive(Debug, Clone)]
pub struct SystemResources {
    /// Total CPU usage across all cores, 0-10000 (0.00%-100.00%).
    pub total_cpu_percent: u32,
    /// Used physical memory in KiB.
    pub used_memory_kb: u64,
    /// Total physical memory in KiB.
    pub total_memory_kb: u64,
    /// Number of live processes.
    pub process_count: u32,
    /// Total thread count across all processes.
    pub thread_count: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    /// All tracked tasks.
    tasks: Vec<TaskInfo>,
    /// Next PID to assign.
    next_pid: u32,
    /// Lifetime counters.
    total_created: u64,
    total_killed: u64,
    total_suspended: u64,
    /// Operation counter.
    ops: u64,
    /// System-wide resource snapshot.
    system_resources: SystemResources,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the task monitor with an **empty** registry.
///
/// Seeds NO tasks and zeroed system resources.  Real tasks are tracked through
/// [`register_task`] / [`kill_task`] / [`update_task_usage`] as `proc::spawn`
/// and the scheduler accounting path report process lifecycle and usage; until
/// that wiring exists, `/proc/taskmon` and the `taskmon` kshell command report
/// an empty table and zeroed resources rather than fabricated processes — the
/// kernel's hard "never invent data in procfs" rule.
///
/// (Previously this seeded three FABRICATED bootstrap tasks — `kernel` pid 0
/// idle/RealTime 1024 KiB, `init` pid 1 Running/High 0.50% CPU 2048 KiB 2
/// threads, `kshell` pid 2 Running/Normal 1.00% CPU 4096 KiB 2 threads — plus an
/// invented [`SystemResources`] snapshot (100% CPU, 64 MiB used of 1 GiB,
/// 3 processes, 5 threads), which the `taskmon` kshell command then displayed as
/// if they were real running processes.  The authoritative live process list is
/// [`crate::sched::task_list`]; see the DEFERRED PROPER FIX note in todo.txt for
/// wiring taskmon to read it.  The self-test now builds its own fixtures via the
/// real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(State {
        tasks: Vec::new(),
        next_pid: 1,
        total_created: 0,
        total_killed: 0,
        total_suspended: 0,
        ops: 0,
        system_resources: SystemResources {
            total_cpu_percent: 0,
            used_memory_kb: 0,
            total_memory_kb: 0,
            process_count: 0,
            thread_count: 0,
        },
    });
}

// ---------------------------------------------------------------------------
// Task lifecycle
// ---------------------------------------------------------------------------

/// Register a new task and return its assigned PID.
pub fn register_task(
    name: &str,
    priority: TaskPriority,
    parent_pid: u32,
    user: &str,
) -> KernelResult<u32> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    with_state(|s| {
        if s.tasks.len() >= MAX_TASKS {
            return Err(KernelError::ResourceExhausted);
        }

        let pid = s.next_pid;
        s.next_pid = s.next_pid.saturating_add(1);

        s.tasks.push(TaskInfo {
            pid,
            name: String::from(name),
            state: TaskState::Running,
            priority,
            cpu_percent: 0,
            memory_kb: 0,
            threads: 1,
            parent_pid,
            user: String::from(user),
            started_ns: crate::hpet::elapsed_ns(),
        });

        s.total_created += 1;
        s.system_resources.process_count = s.tasks.len() as u32;
        s.system_resources.thread_count += 1;

        Ok(pid)
    })
}

/// Mark a task as Zombie (killed).
///
/// The kernel task (pid 0) cannot be killed.
pub fn kill_task(pid: u32) -> KernelResult<()> {
    if pid == 0 {
        return Err(KernelError::PermissionDenied);
    }

    with_state(|s| {
        let task = s.tasks.iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;

        if task.state == TaskState::Zombie {
            return Err(KernelError::InvalidArgument);
        }

        task.state = TaskState::Zombie;
        task.cpu_percent = 0;
        s.total_killed += 1;
        Ok(())
    })
}

/// Suspend a task (set state to Stopped).
pub fn suspend_task(pid: u32) -> KernelResult<()> {
    if pid == 0 {
        return Err(KernelError::PermissionDenied);
    }

    with_state(|s| {
        let task = s.tasks.iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;

        if task.state == TaskState::Zombie {
            return Err(KernelError::InvalidArgument);
        }
        if task.state == TaskState::Stopped {
            return Err(KernelError::InvalidArgument);
        }

        task.state = TaskState::Stopped;
        task.cpu_percent = 0;
        s.total_suspended += 1;
        Ok(())
    })
}

/// Resume a stopped task (set state to Running).
pub fn resume_task(pid: u32) -> KernelResult<()> {
    with_state(|s| {
        let task = s.tasks.iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;

        if task.state != TaskState::Stopped {
            return Err(KernelError::InvalidArgument);
        }

        task.state = TaskState::Running;
        Ok(())
    })
}

/// Change the priority class of a task.
pub fn set_priority(pid: u32, priority: TaskPriority) -> KernelResult<()> {
    with_state(|s| {
        let task = s.tasks.iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;

        if task.state == TaskState::Zombie {
            return Err(KernelError::InvalidArgument);
        }

        task.priority = priority;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Resource accounting
// ---------------------------------------------------------------------------

/// Update CPU and memory usage for a specific task.
pub fn update_task_usage(pid: u32, cpu_percent: u32, memory_kb: u64) -> KernelResult<()> {
    with_state(|s| {
        let task = s.tasks.iter_mut()
            .find(|t| t.pid == pid)
            .ok_or(KernelError::NotFound)?;

        if task.state == TaskState::Zombie {
            return Err(KernelError::InvalidArgument);
        }

        task.cpu_percent = cpu_percent;
        task.memory_kb = memory_kb;
        Ok(())
    })
}

/// Update system-wide resource counters.
pub fn update_resources(total_cpu_percent: u32, used_memory_kb: u64, total_memory_kb: u64) {
    let mut guard = STATE.lock();
    if let Some(s) = guard.as_mut() {
        s.system_resources.total_cpu_percent = total_cpu_percent;
        s.system_resources.used_memory_kb = used_memory_kb;
        s.system_resources.total_memory_kb = total_memory_kb;
        s.ops += 1;
        OPS.store(s.ops, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get information about a single task by PID.
pub fn get_task(pid: u32) -> KernelResult<TaskInfo> {
    with_state(|s| {
        s.tasks.iter()
            .find(|t| t.pid == pid)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all tracked tasks.
pub fn list_tasks() -> Vec<TaskInfo> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.tasks.clone(),
        None => Vec::new(),
    }
}

/// Get current system resource snapshot.
pub fn get_resources() -> SystemResources {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.system_resources.clone(),
        None => SystemResources {
            total_cpu_percent: 0,
            used_memory_kb: 0,
            total_memory_kb: 0,
            process_count: 0,
            thread_count: 0,
        },
    }
}

/// Return summary statistics.
///
/// Returns `(task_count, total_created, total_killed, total_suspended, ops)`.
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.tasks.len(),
            s.total_created,
            s.total_killed,
            s.total_suspended,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the task monitor module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[taskmon] Running self-tests...");

    // Residue-free: begin from a clean EMPTY registry and build every fixture
    // via the real API so the assertions are exact and no test tasks leak into
    // the live /proc/taskmon table (the kshell `taskmon test` subcommand calls
    // this directly).
    *STATE.lock() = None;
    init_defaults();

    // Test 1: empty after init — no fabricated tasks, zeroed resources.
    {
        assert_eq!(list_tasks().len(), 0);
        let res = get_resources();
        assert_eq!(res.process_count, 0);
        assert_eq!(res.total_memory_kb, 0);
        assert_eq!(res.used_memory_kb, 0);
        let (count, created, killed, suspended, _ops) = stats();
        assert_eq!((count, created, killed, suspended), (0, 0, 0, 0));
        serial_println!("[taskmon]   1. Empty init — OK");
    }

    // Test 2: register a new task (PIDs start at 1).
    let testapp_pid = {
        let pid = register_task("testapp", TaskPriority::Normal, 1, "alice")
            .expect("register testapp");
        assert_eq!(pid, 1);
        let task = get_task(pid).expect("get testapp");
        assert_eq!(task.name, "testapp");
        assert_eq!(task.state, TaskState::Running);
        assert_eq!(task.parent_pid, 1);
        assert_eq!(task.user, "alice");
        serial_println!("[taskmon]   2. Register new task — OK");
        pid
    };

    // Test 3: kill a task sets Zombie
    {
        kill_task(testapp_pid).expect("kill testapp");
        let task = get_task(testapp_pid).expect("get killed task");
        assert_eq!(task.state, TaskState::Zombie);
        assert_eq!(task.cpu_percent, 0);
        serial_println!("[taskmon]   3. Kill task sets Zombie — OK");
    }

    // Test 4: cannot kill pid 0 (the kernel task, even when not registered)
    {
        let result = kill_task(0);
        assert!(result.is_err());
        serial_println!("[taskmon]   4. Cannot kill kernel (pid 0) — OK");
    }

    // Test 5: suspend and resume
    let daemon_pid = {
        let pid = register_task("daemon", TaskPriority::High, 1, "root")
            .expect("register daemon");
        suspend_task(pid).expect("suspend daemon");
        let task = get_task(pid).expect("get suspended");
        assert_eq!(task.state, TaskState::Stopped);

        resume_task(pid).expect("resume daemon");
        let task = get_task(pid).expect("get resumed");
        assert_eq!(task.state, TaskState::Running);
        serial_println!("[taskmon]   5. Suspend and resume — OK");
        pid
    };

    // Test 6: cannot suspend pid 0
    {
        let result = suspend_task(0);
        assert!(result.is_err());
        serial_println!("[taskmon]   6. Cannot suspend kernel (pid 0) — OK");
    }

    // Test 7: set priority
    {
        set_priority(daemon_pid, TaskPriority::Low).expect("set priority");
        let task = get_task(daemon_pid).expect("get reprioritized");
        assert_eq!(task.priority, TaskPriority::Low);
        serial_println!("[taskmon]   7. Set priority — OK");
    }

    // Test 8: update task usage
    {
        update_task_usage(daemon_pid, 2500, 8192).expect("update usage");
        let task = get_task(daemon_pid).expect("get updated usage");
        assert_eq!(task.cpu_percent, 2500);
        assert_eq!(task.memory_kb, 8192);
        serial_println!("[taskmon]   8. Update task usage — OK");
    }

    // Test 9: system resources — start zeroed, then update to exact values.
    {
        let res = get_resources();
        assert_eq!(res.total_memory_kb, 0);
        assert_eq!(res.used_memory_kb, 0);

        update_resources(5000, 131_072, 2_097_152);
        let res = get_resources();
        assert_eq!(res.total_cpu_percent, 5000);
        assert_eq!(res.used_memory_kb, 131_072);
        assert_eq!(res.total_memory_kb, 2_097_152);
        serial_println!("[taskmon]   9. System resources update — OK");
    }

    // Test 10: stats counters reflect exactly the operations above.
    {
        let (count, created, killed, suspended, ops) = stats();
        assert_eq!(count, 2);      // testapp (zombie, still tracked) + daemon
        assert_eq!(created, 2);    // testapp + daemon
        assert_eq!(killed, 1);     // testapp
        assert_eq!(suspended, 1);  // daemon was suspended once
        assert!(ops > 0);
        serial_println!("[taskmon]  10. Stats counters — OK");
    }

    // Test 11: label methods
    {
        assert_eq!(TaskState::Running.label(), "Running");
        assert_eq!(TaskState::Sleeping.label(), "Sleeping");
        assert_eq!(TaskState::Stopped.label(), "Stopped");
        assert_eq!(TaskState::Zombie.label(), "Zombie");
        assert_eq!(TaskState::Idle.label(), "Idle");

        assert_eq!(TaskPriority::RealTime.label(), "RealTime");
        assert_eq!(TaskPriority::High.label(), "High");
        assert_eq!(TaskPriority::Normal.label(), "Normal");
        assert_eq!(TaskPriority::BelowNormal.label(), "BelowNormal");
        assert_eq!(TaskPriority::Low.label(), "Low");
        assert_eq!(TaskPriority::Idle.label(), "Idle");
        serial_println!("[taskmon]  11. Enum label methods — OK");
    }

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/taskmon table with its fixtures (the original test left `testapp`
    // and `daemon` in STATE).  Reset to the uninitialised state so production
    // reads report an empty table until proc::spawn / scheduler accounting wire
    // real task tracking.
    *STATE.lock() = None;
    serial_println!("[taskmon] All 11 self-tests passed.");
}
