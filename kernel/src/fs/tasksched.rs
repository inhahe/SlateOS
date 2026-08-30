//! Task scheduler — cron-like scheduled and recurring task execution.
//!
//! Provides timed execution of commands similar to Windows Task Scheduler,
//! cron/systemd timers on Linux, or launchd on macOS.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Task Scheduler
//!   → tasksched::create_task() → SchedTask
//!   → tasksched::check_due(h, m, weekday) → Vec<due tasks>
//!
//! Init system integration
//!   → tasksched::init_defaults() at boot
//!   → periodic check_due() calls from timer service
//!
//! Integration:
//!   → autostart (boot-time tasks)
//!   → power (wake-to-run, skip on battery)
//!   → storageclean (scheduled cleanup)
//!   → backup (scheduled backups)
//! ```
//!
//! ## Schedule Types
//!
//! - **Once**: run at a specific date/time, then mark done
//! - **Daily**: run every day at specified time
//! - **Weekly**: run on specified days of the week
//! - **Interval**: run every N minutes/hours
//! - **Boot**: run at system startup
//! - **Login**: run at user login

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_TASKS: usize = 256;
const MAX_HISTORY: usize = 512;
const MAX_COMMAND_LEN: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// When a scheduled task should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleType {
    /// Run once at a specific time.
    Once,
    /// Run every day at a specific time.
    Daily,
    /// Run on specified days of the week.
    Weekly,
    /// Run every N minutes.
    Interval,
    /// Run at system startup.
    Boot,
    /// Run at user login.
    Login,
}

impl ScheduleType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "Once",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Interval => "Interval",
            Self::Boot => "Boot",
            Self::Login => "Login",
        }
    }
}

/// Priority of a scheduled task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    Low,
    Normal,
    High,
}

impl TaskPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }
}

/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Ready to run at next scheduled time.
    Ready,
    /// Currently running.
    Running,
    /// Completed (for Once tasks).
    Completed,
    /// Disabled by user.
    Disabled,
    /// Failed on last run.
    Failed,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Disabled => "Disabled",
            Self::Failed => "Failed",
        }
    }
}

/// A scheduled task definition.
#[derive(Debug, Clone)]
pub struct SchedTask {
    /// Unique task ID.
    pub id: u64,
    /// Human-readable name.
    pub name: String,
    /// Command to execute.
    pub command: String,
    /// Optional arguments.
    pub arguments: String,
    /// Schedule type.
    pub schedule_type: ScheduleType,
    /// Hour of day to run (0–23). Used by Once/Daily/Weekly.
    pub hour: u8,
    /// Minute to run (0–59). Used by Once/Daily/Weekly.
    pub minute: u8,
    /// Days of the week (0=Sun..6=Sat). Used by Weekly.
    pub weekdays: [bool; 7],
    /// Interval in minutes. Used by Interval type.
    pub interval_minutes: u32,
    /// Priority level.
    pub priority: TaskPriority,
    /// Current status.
    pub status: TaskStatus,
    /// Whether the task is a system task (cannot be removed).
    pub system: bool,
    /// Skip if running on battery power.
    pub skip_on_battery: bool,
    /// Wake the system from sleep to run.
    pub wake_to_run: bool,
    /// Run with elevated privileges.
    pub elevated: bool,
    /// Maximum run time in seconds (0 = no limit).
    pub timeout_seconds: u32,
    /// Number of times this task has run.
    pub run_count: u64,
    /// When the most recent run *started* (ns since boot), or `None` if the
    /// task has never been started.
    ///
    /// This is an `Option` rather than a `0`-means-never `u64` because `0` is
    /// a perfectly legal uptime — the instant the machine came up — so the
    /// two readings are indistinguishable in band. `check_due` happened to
    /// special-case the zero, but `record_complete` did not, and computed the
    /// task's duration as `now - 0`, i.e. reported the entire uptime as the
    /// run time of a task that was never timed. See
    /// `known-issues.md -> FIXED-A-SYSMAINT-NEVER-RUN-TASKS-WERE-NEVER-DUE`
    /// for the same sentinel getting it wrong in the other direction.
    pub last_run_ns: Option<u64>,
    /// Last run duration (us).
    pub last_duration_us: u64,
    /// Last run success.
    pub last_success: bool,
    /// Retry count on failure.
    pub retry_count: u32,
    /// Max retries (0 = no retry).
    pub max_retries: u32,
    /// Created timestamp (ns).
    pub created_ns: u64,
    /// User who owns the task.
    pub uid: u32,
    /// Description.
    pub description: String,
}

/// Record of a task execution.
#[derive(Debug, Clone)]
pub struct TaskRun {
    pub task_id: u64,
    pub task_name: String,
    pub started_ns: u64,
    pub duration_us: u64,
    pub success: bool,
    pub exit_code: i32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct SchedState {
    tasks: Vec<SchedTask>,
    history: Vec<TaskRun>,
    next_id: u64,
    total_runs: u64,
    total_failures: u64,
    ops: u64,
}

static STATE: Mutex<Option<SchedState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut SchedState) -> KernelResult<R>,
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

/// Initialize the task scheduler with default system tasks.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let now = crate::hpet::elapsed_ns();

    let mut state = SchedState {
        tasks: Vec::new(),
        history: Vec::new(),
        next_id: 1,
        total_runs: 0,
        total_failures: 0,
        ops: 0,
    };

    // System tasks — standard maintenance jobs.

    // Daily storage cleanup check.
    let id = state.next_id;
    state.next_id += 1;
    state.tasks.push(SchedTask {
        id,
        name: String::from("Storage Cleanup"),
        command: String::from("storageclean"),
        arguments: String::from("auto"),
        schedule_type: ScheduleType::Daily,
        hour: 3,
        minute: 0,
        weekdays: [false; 7],
        interval_minutes: 0,
        priority: TaskPriority::Low,
        status: TaskStatus::Ready,
        system: true,
        skip_on_battery: true,
        wake_to_run: false,
        elevated: false,
        timeout_seconds: 300,
        run_count: 0,
        last_run_ns: None,
        last_duration_us: 0,
        last_success: true,
        retry_count: 0,
        max_retries: 1,
        created_ns: now,
        uid: 0,
        description: String::from("Automatic storage cleanup and temp file removal"),
    });

    // Weekly diagnostics.
    let id = state.next_id;
    state.next_id += 1;
    let mut weekdays = [false; 7];
    weekdays[0] = true; // Sunday
    state.tasks.push(SchedTask {
        id,
        name: String::from("System Diagnostics"),
        command: String::from("sysdiag"),
        arguments: String::from("run"),
        schedule_type: ScheduleType::Weekly,
        hour: 4,
        minute: 0,
        weekdays,
        interval_minutes: 0,
        priority: TaskPriority::Low,
        status: TaskStatus::Ready,
        system: true,
        skip_on_battery: true,
        wake_to_run: false,
        elevated: false,
        timeout_seconds: 120,
        run_count: 0,
        last_run_ns: None,
        last_duration_us: 0,
        last_success: true,
        retry_count: 0,
        max_retries: 0,
        created_ns: now,
        uid: 0,
        description: String::from("Weekly system health check"),
    });

    // Filesystem trim (weekly, Tuesday).
    let id = state.next_id;
    state.next_id += 1;
    let mut weekdays = [false; 7];
    weekdays[2] = true; // Tuesday
    state.tasks.push(SchedTask {
        id,
        name: String::from("Filesystem Trim"),
        command: String::from("fstrim"),
        arguments: String::from("/"),
        schedule_type: ScheduleType::Weekly,
        hour: 2,
        minute: 30,
        weekdays,
        interval_minutes: 0,
        priority: TaskPriority::Low,
        status: TaskStatus::Ready,
        system: true,
        skip_on_battery: true,
        wake_to_run: false,
        elevated: true,
        timeout_seconds: 600,
        run_count: 0,
        last_run_ns: None,
        last_duration_us: 0,
        last_success: true,
        retry_count: 0,
        max_retries: 0,
        created_ns: now,
        uid: 0,
        description: String::from("Weekly SSD TRIM operation"),
    });

    *guard = Some(state);
}

// ---------------------------------------------------------------------------
// Task management
// ---------------------------------------------------------------------------

/// Create a new scheduled task. Returns task ID.
pub fn create_task(name: &str, command: &str, schedule_type: ScheduleType) -> KernelResult<u64> {
    if name.is_empty() || command.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if command.len() > MAX_COMMAND_LEN {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.tasks.len() >= MAX_TASKS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.tasks.push(SchedTask {
            id,
            name: String::from(name),
            command: String::from(command),
            arguments: String::new(),
            schedule_type,
            hour: 0,
            minute: 0,
            weekdays: [false; 7],
            interval_minutes: 60,
            priority: TaskPriority::Normal,
            status: TaskStatus::Ready,
            system: false,
            skip_on_battery: false,
            wake_to_run: false,
            elevated: false,
            timeout_seconds: 0,
            run_count: 0,
            last_run_ns: None,
            last_duration_us: 0,
            last_success: true,
            retry_count: 0,
            max_retries: 0,
            created_ns: crate::hpet::elapsed_ns(),
            uid: 0,
            description: String::new(),
        });
        Ok(id)
    })
}

/// Remove a task by ID. System tasks cannot be removed.
pub fn remove_task(id: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.tasks.iter().position(|t| t.id == id) {
            if state.tasks[pos].system {
                return Err(KernelError::PermissionDenied);
            }
            state.tasks.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get a task by ID.
pub fn get_task(id: u64) -> KernelResult<SchedTask> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state
        .tasks
        .iter()
        .find(|t| t.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all tasks.
pub fn list_tasks() -> Vec<SchedTask> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.tasks.clone())
}

// ---------------------------------------------------------------------------
// Task configuration
// ---------------------------------------------------------------------------

/// Set the scheduled time (hour:minute).
pub fn set_time(id: u64, hour: u8, minute: u8) -> KernelResult<()> {
    if hour > 23 || minute > 59 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.hour = hour;
        task.minute = minute;
        Ok(())
    })
}

/// Set the weekday schedule (0=Sun..6=Sat).
pub fn set_weekday(id: u64, day: usize, enabled: bool) -> KernelResult<()> {
    if day > 6 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.weekdays[day] = enabled;
        Ok(())
    })
}

/// Set the interval in minutes (for Interval schedule type).
pub fn set_interval(id: u64, minutes: u32) -> KernelResult<()> {
    if minutes == 0 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.interval_minutes = minutes;
        Ok(())
    })
}

/// Set task arguments.
pub fn set_arguments(id: u64, args: &str) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.arguments = String::from(args);
        Ok(())
    })
}

/// Set task description.
pub fn set_description(id: u64, desc: &str) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.description = String::from(desc);
        Ok(())
    })
}

/// Set task priority.
pub fn set_priority(id: u64, prio: TaskPriority) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.priority = prio;
        Ok(())
    })
}

/// Enable a task.
pub fn enable_task(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.status = TaskStatus::Ready;
        Ok(())
    })
}

/// Disable a task.
pub fn disable_task(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.status = TaskStatus::Disabled;
        Ok(())
    })
}

/// Set whether to skip on battery.
pub fn set_skip_on_battery(id: u64, skip: bool) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.skip_on_battery = skip;
        Ok(())
    })
}

/// Set whether to wake system from sleep to run.
pub fn set_wake_to_run(id: u64, wake: bool) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.wake_to_run = wake;
        Ok(())
    })
}

/// Set the timeout in seconds.
pub fn set_timeout(id: u64, seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.timeout_seconds = seconds;
        Ok(())
    })
}

/// Set max retries on failure.
pub fn set_max_retries(id: u64, retries: u32) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.max_retries = retries;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Scheduling and execution
// ---------------------------------------------------------------------------

/// Check which tasks are due at the given time.
///
/// `weekday` is 0=Sunday, 1=Monday, ..., 6=Saturday.
/// Returns a list of task IDs that should be run now.
pub fn check_due(hour: u8, minute: u8, weekday: u8) -> Vec<u64> {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut due = Vec::new();
    for task in &state.tasks {
        if task.status == TaskStatus::Disabled || task.status == TaskStatus::Completed {
            continue;
        }
        match task.schedule_type {
            ScheduleType::Daily => {
                if task.hour == hour && task.minute == minute {
                    due.push(task.id);
                }
            }
            ScheduleType::Weekly => {
                if (weekday as usize) < 7
                    && task.weekdays[weekday as usize]
                    && task.hour == hour
                    && task.minute == minute
                {
                    due.push(task.id);
                }
            }
            ScheduleType::Once => {
                if task.hour == hour && task.minute == minute && task.run_count == 0 {
                    due.push(task.id);
                }
            }
            ScheduleType::Interval => {
                // For interval tasks, check if enough time has elapsed.
                //
                // The multiply saturates because `set_interval` accepts any
                // non-zero u32, and u32::MAX minutes overflows u64 nanoseconds
                // by an order of magnitude. A wrapped product is a *small*
                // interval, so "run this every eight thousand years" would have
                // become "run this on every tick" — a failure in the direction
                // that looks like the feature working.
                let now_ns = crate::hpet::elapsed_ns();
                let interval_ns = u64::from(task.interval_minutes)
                    .saturating_mul(60)
                    .saturating_mul(1_000_000_000);
                let due_now = match task.last_run_ns {
                    // Never started: due immediately.
                    None => true,
                    Some(last) => now_ns.saturating_sub(last) >= interval_ns,
                };
                if due_now {
                    due.push(task.id);
                }
            }
            ScheduleType::Boot | ScheduleType::Login => {
                // Boot/Login tasks are handled at startup, not by periodic check.
            }
        }
    }
    due
}

/// Get all boot-triggered tasks.
pub fn boot_tasks() -> Vec<u64> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.tasks
            .iter()
            .filter(|t| t.schedule_type == ScheduleType::Boot && t.status != TaskStatus::Disabled)
            .map(|t| t.id)
            .collect()
    })
}

/// Get all login-triggered tasks for a user.
pub fn login_tasks(uid: u32) -> Vec<u64> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.tasks
            .iter()
            .filter(|t| {
                t.schedule_type == ScheduleType::Login
                    && t.status != TaskStatus::Disabled
                    && (t.uid == uid || t.uid == 0)
            })
            .map(|t| t.id)
            .collect()
    })
}

/// Record that a task has started running.
pub fn record_start(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        task.status = TaskStatus::Running;
        task.last_run_ns = Some(crate::hpet::elapsed_ns());
        Ok(())
    })
}

/// Record that a task has completed.
pub fn record_complete(id: u64, success: bool, exit_code: i32) -> KernelResult<()> {
    with_state(|state| {
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;

        let now = crate::hpet::elapsed_ns();
        // A completion without a matching `record_start` has no start time to
        // subtract. Attributing `now - 0` to it — which is what the old
        // `u64` sentinel did — reported the machine's entire uptime as this
        // task's run duration. Treat the completion instant as the start
        // instead, which yields a duration of zero: unknown, not enormous.
        let started_ns = task.last_run_ns.unwrap_or(now);
        let duration_us = now.saturating_sub(started_ns) / 1000;

        task.run_count = task.run_count.saturating_add(1);
        task.last_duration_us = duration_us;
        task.last_success = success;
        state.total_runs = state.total_runs.saturating_add(1);

        if success {
            task.retry_count = 0;
            if task.schedule_type == ScheduleType::Once {
                task.status = TaskStatus::Completed;
            } else {
                task.status = TaskStatus::Ready;
            }
        } else {
            state.total_failures += 1;
            task.retry_count += 1;
            if task.retry_count > task.max_retries && task.max_retries > 0 {
                task.status = TaskStatus::Failed;
            } else {
                task.status = TaskStatus::Ready;
            }
        }

        // Record in history.
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(TaskRun {
            task_id: id,
            task_name: task.name.clone(),
            started_ns,
            duration_us,
            success,
            exit_code,
        });

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get task execution history.
pub fn task_history(id: u64) -> Vec<TaskRun> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.history
            .iter()
            .filter(|h| h.task_id == id)
            .cloned()
            .collect()
    })
}

/// Get all execution history.
pub fn all_history() -> Vec<TaskRun> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.history.clone())
}

/// Get tasks by status.
pub fn tasks_by_status(status: TaskStatus) -> Vec<SchedTask> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.tasks
            .iter()
            .filter(|t| t.status == status)
            .cloned()
            .collect()
    })
}

/// Get next due task (earliest scheduled time from now).
pub fn next_due() -> Option<(u64, String, u8, u8)> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    state
        .tasks
        .iter()
        .filter(|t| {
            t.status == TaskStatus::Ready
                && matches!(
                    t.schedule_type,
                    ScheduleType::Daily | ScheduleType::Weekly | ScheduleType::Once
                )
        })
        .min_by_key(|t| t.hour as u32 * 60 + t.minute as u32)
        .map(|t| (t.id, t.name.clone(), t.hour, t.minute))
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (task_count, total_runs, total_failures, history_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.tasks.len(),
            s.total_runs,
            s.total_failures,
            s.history.len(),
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    use crate::serial_println;

    serial_println!("[tasksched] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state with system tasks.
    {
        let (count, _, _, _, _) = stats();
        assert!(count >= 3); // 3 default system tasks
        let tasks = list_tasks();
        assert!(tasks.iter().any(|t| t.name == "Storage Cleanup"));
        assert!(tasks.iter().any(|t| t.name == "System Diagnostics"));
        assert!(tasks.iter().any(|t| t.name == "Filesystem Trim"));
    }
    serial_println!("[tasksched]  1/12 initial state OK");

    // Test 2: create task.
    {
        let id = create_task("Test Task", "echo hello", ScheduleType::Daily).unwrap();
        assert!(id > 0);
        let task = get_task(id).unwrap();
        assert_eq!(task.name, "Test Task");
        assert_eq!(task.command, "echo hello");
        assert_eq!(task.schedule_type, ScheduleType::Daily);
        assert_eq!(task.status, TaskStatus::Ready);
    }
    serial_println!("[tasksched]  2/12 create task OK");

    // Test 3: set time.
    {
        let id = create_task("Timed", "cmd", ScheduleType::Daily).unwrap();
        set_time(id, 14, 30).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.hour, 14);
        assert_eq!(task.minute, 30);
        assert!(set_time(id, 25, 0).is_err());
    }
    serial_println!("[tasksched]  3/12 set time OK");

    // Test 4: weekday schedule.
    {
        let id = create_task("Weekly", "backup", ScheduleType::Weekly).unwrap();
        set_weekday(id, 1, true).unwrap(); // Monday
        set_weekday(id, 5, true).unwrap(); // Friday
        set_time(id, 9, 0).unwrap();
        let task = get_task(id).unwrap();
        assert!(task.weekdays[1]);
        assert!(task.weekdays[5]);
        assert!(!task.weekdays[0]);
        assert!(set_weekday(id, 7, true).is_err());
    }
    serial_println!("[tasksched]  4/12 weekday schedule OK");

    // Test 5: check_due.
    {
        let id = create_task("Due Test", "test", ScheduleType::Daily).unwrap();
        set_time(id, 10, 0).unwrap();
        let due = check_due(10, 0, 0);
        assert!(due.contains(&id));
        let not_due = check_due(11, 0, 0);
        assert!(!not_due.contains(&id));
    }
    serial_println!("[tasksched]  5/12 check_due OK");

    // Test 6: record execution.
    {
        let id = create_task("Exec Test", "run", ScheduleType::Daily).unwrap();
        record_start(id).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.status, TaskStatus::Running);

        record_complete(id, true, 0).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.status, TaskStatus::Ready);
        assert_eq!(task.run_count, 1);
        assert!(task.last_success);
    }
    serial_println!("[tasksched]  6/12 record execution OK");

    // Test 7: history.
    {
        let (_, _, _, hist_count, _) = stats();
        assert!(hist_count > 0);
        let hist = all_history();
        assert!(!hist.is_empty());
    }
    serial_println!("[tasksched]  7/12 history OK");

    // Test 8: disable/enable.
    {
        let id = create_task("Toggle", "cmd", ScheduleType::Daily).unwrap();
        set_time(id, 12, 0).unwrap();
        disable_task(id).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.status, TaskStatus::Disabled);
        // Disabled tasks should not show up in check_due.
        let due = check_due(12, 0, 0);
        assert!(!due.contains(&id));
        enable_task(id).unwrap();
        let due = check_due(12, 0, 0);
        assert!(due.contains(&id));
    }
    serial_println!("[tasksched]  8/12 disable/enable OK");

    // Test 9: remove task.
    {
        let id = create_task("Removable", "cmd", ScheduleType::Once).unwrap();
        remove_task(id).unwrap();
        assert!(get_task(id).is_err());

        // System tasks cannot be removed.
        let tasks = list_tasks();
        let system_id = tasks.iter().find(|t| t.system).map(|t| t.id).unwrap();
        assert!(remove_task(system_id).is_err());
    }
    serial_println!("[tasksched]  9/12 remove task OK");

    // Test 10: once task completion.
    {
        let id = create_task("One Time", "cmd", ScheduleType::Once).unwrap();
        set_time(id, 15, 0).unwrap();
        record_start(id).unwrap();
        record_complete(id, true, 0).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        // Completed Once tasks should not show up in check_due.
        let due = check_due(15, 0, 0);
        assert!(!due.contains(&id));
    }
    serial_println!("[tasksched] 10/12 once completion OK");

    // Test 11: failure and retry.
    {
        let id = create_task("Retry", "flaky", ScheduleType::Daily).unwrap();
        set_max_retries(id, 2).unwrap();
        record_start(id).unwrap();
        record_complete(id, false, 1).unwrap();
        let task = get_task(id).unwrap();
        // Should still be Ready (retry_count=1 <= max_retries=2).
        assert_eq!(task.status, TaskStatus::Ready);
        assert_eq!(task.retry_count, 1);

        record_start(id).unwrap();
        record_complete(id, false, 1).unwrap();
        record_start(id).unwrap();
        record_complete(id, false, 1).unwrap();
        let task = get_task(id).unwrap();
        // Should be Failed after exceeding retries.
        assert_eq!(task.status, TaskStatus::Failed);
    }
    serial_println!("[tasksched] 11/12 failure/retry OK");

    // Test 12: interval scheduling — the two things the `u64` sentinel and
    // the unchecked multiply each got wrong.
    {
        // A task that has never run is due, whatever its interval. This is
        // the case `last_run_ns: Option<u64>` exists to express; the old
        // `0`-means-never encoding only worked because `check_due` carried a
        // hand-written special case for it.
        let id = create_task("Interval", "poll", ScheduleType::Interval).unwrap();
        let task = get_task(id).unwrap();
        assert_eq!(task.last_run_ns, None, "a fresh task has never run");
        assert_eq!(task.interval_minutes, 60, "and defaults to hourly");
        assert!(check_due(0, 0, 0).contains(&id), "never run means due now");

        // Having just run, it is not due again for an hour. The clock does
        // not advance inside this suite, so "an hour from now" is a bound the
        // test can rely on.
        record_start(id).unwrap();
        record_complete(id, true, 0).unwrap();
        assert!(get_task(id).unwrap().last_run_ns.is_some());
        assert!(
            !check_due(0, 0, 0).contains(&id),
            "just ran, so not due for another hour"
        );

        // An interval too large to express in nanoseconds must saturate, not
        // wrap. u32::MAX minutes is ~8,000 years, which overflows u64 ns by
        // more than tenfold; a wrapped product is a *short* interval, so the
        // bug would have made "almost never" mean "every single tick" — the
        // failure direction that looks like the feature working.
        set_interval(id, u32::MAX).unwrap();
        assert!(
            !check_due(0, 0, 0).contains(&id),
            "an eight-thousand-year interval is not due one tick later"
        );
        assert!(set_interval(id, 0).is_err(), "a zero interval is refused");

        // A completion with no matching start has no duration to report. The
        // old code subtracted the sentinel and so attributed the machine's
        // entire uptime to a task that was never timed.
        let orphan = create_task("Orphan", "never-started", ScheduleType::Daily).unwrap();
        record_complete(orphan, true, 0).unwrap();
        assert_eq!(
            get_task(orphan).unwrap().last_duration_us,
            0,
            "an untimed run lasted an unknown time, not the whole uptime"
        );

        remove_task(id).unwrap();
        remove_task(orphan).unwrap();
    }
    serial_println!("[tasksched] 12/12 interval scheduling OK");

    serial_println!("[tasksched] All self-tests passed.");
}
