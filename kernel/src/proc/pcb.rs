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

/// Cumulative count of processes created since boot.
///
/// Incremented once per successful process creation — both fresh
/// [`create`] and [`fork_create`].  This is a monotonic forks-since-boot
/// counter (it never decrements when a process exits), which is exactly
/// the semantics Linux's `/proc/stat` `processes` field reports.  It is
/// distinct from the live process count (the size of `PROCESS_TABLE`):
/// `NEXT_PID` also advances but is an implementation detail of PID
/// allocation, so we keep a dedicated counter rather than deriving the
/// value from it.
static PROCESSES_CREATED: AtomicU64 = AtomicU64::new(0);

fn alloc_pid() -> ProcessId {
    NEXT_PID.fetch_add(1, Ordering::Relaxed)
}

/// Cumulative number of processes created since boot.
///
/// Backs `/proc/stat`'s `processes` field.  Counts every successful
/// process creation (initial spawn and fork) and never decreases.
#[must_use]
pub fn processes_created() -> u64 {
    PROCESSES_CREATED.load(Ordering::Relaxed)
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
// Syscall ABI mode — selects which syscall table interprets the process's
// `syscall` instructions.
// ---------------------------------------------------------------------------

/// The syscall ABI that a process targets.
///
/// All processes default to [`AbiMode::Native`] — they invoke the kernel
/// using our native syscall numbers (see `crate::syscall::number`).
/// Processes spawned with the Linux ABI flag use [`AbiMode::Linux`], which
/// routes their `syscall` instructions through
/// [`crate::syscall::linux::dispatch_linux`].  The translation layer
/// remaps Linux x86_64 syscall numbers and argument semantics onto native
/// kernel operations and returns Linux-style errno values (negated) in
/// `rax`.
///
/// This is the foundation for Linuxulator/WINE-style binary
/// compatibility: a prebuilt Linux ELF can run on this kernel by being
/// loaded into a process with `abi_mode = Linux`.  ELF auto-detection
/// (PT_GNU_PROPERTY, `e_osabi`, `INTERP = /lib64/ld-linux-x86-64.so.2`)
/// lives in the loader and stamps this field when it recognises a Linux
/// binary; tests and tooling can also set it explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbiMode {
    /// Our native kernel ABI (default).
    #[default]
    Native,
    /// Linux x86_64 ABI — dispatched through the translation layer.
    ///
    /// Currently only set by tests and the future ELF loader's Linux-binary
    /// detection path; suppress the dead-code lint until that loader lands.
    #[allow(dead_code)]
    Linux,
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
    /// Task waiting to reap this *specific* process (if any).
    ///
    /// Set by `set_wait_task` when a parent calls `waitpid(child_pid)`.
    /// Woken when this process becomes a zombie.
    pub wait_task: Option<TaskId>,
    /// Task (belonging to *this* process) blocked in `waitpid(-1)` —
    /// i.e. waiting to reap *any* child.
    ///
    /// Unlike [`wait_task`](Self::wait_task), which lives on the child
    /// being waited for, this lives on the *parent*: when any child of
    /// this process becomes a zombie, the scheduler wakes this task so
    /// it can re-scan for a reapable child.  Only one any-child waiter
    /// per process (a process has a single main thread doing waits in
    /// the common case; concurrent waiters would race, which POSIX
    /// permits — one wins the reap, the other sees ECHILD/retries).
    pub wait_any_task: Option<TaskId>,
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
    /// Persistent snapshot of the process's argv, kept for the whole
    /// process lifetime to back `/proc/<pid>/cmdline`.
    ///
    /// Distinct from [`Self::initial_argv`]: that field is the one-shot
    /// startup channel the child drains via `SYS_PROCESS_GET_ARGS`,
    /// whereas this snapshot is never cleared, mirroring Linux's
    /// `/proc/<pid>/cmdline` which stays readable as long as the process
    /// lives.  Set (by cloning) in [`set_initial_args`]; inherited from
    /// the parent across `fork_create` (a forked child shares the
    /// parent's cmdline until it `execve`s).  Empty for processes
    /// spawned without argv (e.g. the initial kernel-spawned task), in
    /// which case `/proc/<pid>/cmdline` falls back to the process name.
    pub proc_argv: Vec<Vec<u8>>,
    /// Persistent snapshot of the process's environment, kept for the
    /// whole process lifetime to back `/proc/<pid>/environ`.  Same
    /// lifecycle as [`Self::proc_argv`].
    pub proc_envp: Vec<Vec<u8>>,
    /// Syscall ABI the process speaks.
    ///
    /// [`AbiMode::Native`] (the default) routes `syscall` instructions
    /// through the native dispatch table.  [`AbiMode::Linux`] routes
    /// them through [`crate::syscall::linux::dispatch_linux`], which
    /// translates Linux x86_64 syscall numbers and semantics onto our
    /// native kernel operations.  Stamped by the ELF loader (when it
    /// detects a Linux binary) or set explicitly via
    /// [`set_abi_mode`].  Inherited across `fork`.
    pub abi_mode: AbiMode,
    /// Kernel-side fd table for Linux-ABI processes.
    ///
    /// `None` for Native processes (whose fd table lives in
    /// userspace under `posix/src/fdtable.rs`).  `Some` for Linux-ABI
    /// processes: holds the mapping from Linux integer fds to
    /// (`HandleKind`, raw kernel handle), pre-populated with
    /// console-backed stdin/stdout/stderr at process creation.
    ///
    /// Allocated when the ELF loader detects a Linux binary and stamps
    /// [`AbiMode::Linux`].  See [`crate::proc::linux_fd`] for the
    /// table implementation and [`crate::syscall::linux`] for the
    /// callers.
    pub linux_fd_table: Option<alloc::boxed::Box<super::linux_fd::KernelFdTable>>,
    /// Saved auxiliary vector for Linux-ABI processes, as the raw
    /// little-endian `Elf64_auxv_t` byte stream (pairs of
    /// `(a_type, a_val)` u64s) ending in an `AT_NULL` terminator.
    ///
    /// `None` for Native processes — they receive argv/envp from the
    /// kernel via `SYS_PROCESS_GET_ARGS` and have no auxv at all by
    /// design (the auxv is a Linux/SysV-ABI construct that must never
    /// enter the native launch path — design-decision #4).  `Some` for
    /// Linux-ABI processes: a verbatim copy of the auxv that
    /// [`crate::proc::linux_stack::install_linux_stack`] wrote onto the
    /// SysV initial stack, captured at spawn/exec time so it can be
    /// served back from `prctl(PR_GET_AUXV)` and `/proc/<pid>/auxv`
    /// without re-reading the user stack.  Replaced on `exec`; cleared
    /// to `None` for native processes; **not** inherited across `fork`
    /// in the sense of being rebuilt (a forked child shares the parent's
    /// already-constructed stack, so the copy is cloned verbatim).
    pub linux_saved_auxv: Option<alloc::vec::Vec<u8>>,
    /// Current working directory, stored as a canonical absolute path.
    ///
    /// Invariants maintained by [`set_cwd`]:
    /// - Starts with `b'/'`.
    /// - Never contains `..`, `.`, empty components, or duplicate `/`.
    /// - Has no trailing `/` except the root itself (which is exactly
    ///   `b"/"`).
    /// - No interior NULs.
    /// - Length ≤ `PATH_MAX` (4096) **including** the trailing NUL the
    ///   `getcwd` syscall writes (so the stored slice is ≤ 4095 bytes).
    ///
    /// Set by `chdir` / `fchdir`.  Inherited by `fork`.  Used by every
    /// `*at(AT_FDCWD, …)` resolution path (future) and by `getcwd`.
    ///
    /// We store the cwd on every process (not just Linux-ABI ones).
    /// Native processes don't currently expose it via a syscall, but
    /// the field is cheap (one heap allocation per process) and keeps
    /// fork's structural invariant simple: every child inherits.
    pub cwd: Vec<u8>,
    /// Resolved absolute path of the executable image, stored as bytes.
    ///
    /// Backs `/proc/<pid>/exe` (a magic symlink in Linux).  Captured at
    /// `exec` time by the ELF loader, which writes the canonical path of
    /// the binary it loaded.  Empty until the process has exec'd a binary
    /// (e.g. a freshly kernel-spawned task or a forked child that has not
    /// yet `execve`d), in which case `/proc/<pid>/exe` reports `NotFound`,
    /// matching Linux's behaviour for a process with no mm-backed exe.
    ///
    /// Lifecycle differs from [`Self::cwd`]: `exe_path` is **inherited on
    /// `fork`** (clone — the child runs the same image until it execs) but
    /// **overwritten on `exec`** (exec replaces the image, so the path is
    /// not carried across the exec boundary).  Stored as bytes because a
    /// path may contain any byte except `/` and NUL.
    pub exe_path: Vec<u8>,
    /// Per-process Linux resource limits.
    ///
    /// Indexed by `RLIMIT_*` resource number (0..=15).  Each entry is
    /// `(rlim_cur, rlim_max)` where `u64::MAX` represents `RLIM_INFINITY`.
    /// Initialised from [`DEFAULT_RLIMITS`] on process creation and
    /// inherited verbatim across `fork`.  Modified by `setrlimit` /
    /// `prlimit64`; read by `getrlimit` / `prlimit64`.
    ///
    /// The kernel doesn't currently *enforce* most of these limits — the
    /// scheduler, allocator, and fd table predate this field — but
    /// programs that lower then re-read their limits (a common idiom in
    /// shells and language runtimes during sandbox setup) now see
    /// consistent state.  Enforcement is tracked separately per resource.
    pub rlimits: [(u64, u64); 16],
    /// Total bytes currently charged to this process under the Linux
    /// address-space accounting used to enforce `RLIMIT_AS`.
    ///
    /// Incremented by [`linux_as_charge`] (called from the Linux `mmap`
    /// path with the *aligned* mapping size) and decremented by
    /// [`linux_as_release`] (called from `munmap`).  Inherited verbatim
    /// across `fork_create` — the child starts with the same charge as
    /// the parent since its address space mirrors the parent's at the
    /// moment of fork.  Native (non-Linux) mmap paths do not touch this
    /// field, matching Linux's "RLIMIT_AS only applies to processes
    /// going through the Linux ABI" model in our codebase.
    pub linux_as_bytes: u64,
    /// Per-process file-mode creation mask, as installed by Linux's
    /// `umask(2)`.
    ///
    /// Stored as a `u16` (the upper bits are always zero — Linux masks
    /// the user-supplied value with `& 0o777` before storing).  The
    /// default for a new process is `0o022` (group/other lose write
    /// bits), matching the de-facto distro default that programs
    /// expect from a fresh shell.  Inherited verbatim across `fork`,
    /// in line with POSIX.
    ///
    /// The VFS does not currently consult this field at file-creation
    /// time — it's read and written through the Linux `sys_umask`
    /// translation only.  That means programs that round-trip the
    /// umask (`old = umask(N); ... ; umask(old);`) see consistent
    /// state and their `old != N` invariant holds, even though the
    /// kernel's actual default-mode behaviour is unaffected.  Real
    /// VFS plumbing is tracked separately in todo.txt.
    pub linux_umask: u16,
    /// Per-process Linux `personality(2)` value.
    ///
    /// The default is `0` (`PER_LINUX`, no personality flags).
    /// Programs set it via the Linux `personality` syscall — most
    /// commonly to enable `ADDR_NO_RANDOMIZE` (gdb's reproducible-
    /// build sequence) or `READ_IMPLIES_EXEC` (legacy binaries).
    ///
    /// Inherited verbatim across `fork_create` (Linux propagates
    /// personality across fork).  The kernel does not yet *act* on
    /// any of the flags — we don't randomize address space, so
    /// ADDR_NO_RANDOMIZE is a no-op; we don't honour
    /// READ_IMPLIES_EXEC at mmap time either.  But persisting the
    /// value lets programs round-trip it correctly through
    /// `personality(persona)` followed by `personality(0xffffffff)`,
    /// which gdb in particular relies on for its own bookkeeping.
    pub linux_personality: u32,
    /// Linux `prctl(PR_SET_PDEATHSIG)` — signal to deliver to this
    /// process when its parent exits.  `0` means "disabled" (the
    /// default and what every freshly-forked process starts with).
    ///
    /// We currently only store and round-trip the value via prctl —
    /// the actual signal delivery on parent death is not wired
    /// because we don't yet have user-signal infrastructure with the
    /// required lifecycle hooks.  See todo.txt entry for batch 61.
    pub linux_pdeathsig: u32,
    /// Linux `sched_setscheduler(2)` policy ID for the process.
    ///
    /// Values match Linux's `SCHED_*` constants:
    ///   - 0 = `SCHED_OTHER` (the default for every freshly-created
    ///     task on Linux and what every shell-spawned process
    ///     inherits)
    ///   - 1 = `SCHED_FIFO` (real-time)
    ///   - 2 = `SCHED_RR` (real-time)
    ///   - 3 = `SCHED_BATCH`
    ///   - 5 = `SCHED_IDLE`
    ///   - 6 = `SCHED_DEADLINE`
    ///   - 7 = `SCHED_EXT`
    ///
    /// We store the value purely for ABI round-trip — our actual
    /// scheduler is a single priority-round-robin and does not honour
    /// real-time policies.  Programs that query the policy after
    /// setting it (and many do, as a sanity check) will at least see
    /// their request reflected back, instead of always observing
    /// `SCHED_OTHER`.  See todo.txt entry for batch 62.
    pub linux_sched_policy: u32,
    /// Static priority for the process, as set via
    /// `sched_setscheduler` / `sched_setparam` and read via
    /// `sched_getparam`.
    ///
    /// Range constraints are enforced at the syscall surface (the
    /// pure helper `sched_priority_check_for_policy`):
    ///   - `SCHED_FIFO` / `SCHED_RR`: 1..=99
    ///   - everything else: must be exactly 0
    ///
    /// Storing it per-PCB lets the get-side report the value the
    /// caller actually installed, instead of always 0.
    pub linux_sched_priority: i32,
    /// Linux nice value, as set via `setpriority(2)` and reported via
    /// `getpriority(2)`.  Range -20..=19; default 0.
    ///
    /// ABI quirk worth recording at the call site: `getpriority`
    /// returns `20 - nice` (so a result of 20 means "nice=0", 39
    /// means "nice=-19", etc.).  The PCB stores the *logical* nice
    /// value; the ABI translation happens in `sys_getpriority`.
    ///
    /// Inherited verbatim across fork and preserved across exec —
    /// matches Linux exactly.  We store this purely for ABI
    /// round-trip; our scheduler does not currently honour nice in
    /// its priority decisions (that lives under the scheduler
    /// roadmap).
    pub linux_nice: i32,
    /// Linux `prctl(PR_SET_DUMPABLE)` flag.  Controls whether the
    /// process is core-dumpable and, on Linux, whether its
    /// `/proc/<pid>/{maps,mem,…}` are owned by the real uid (1) or
    /// by root (2 = SUID_DUMP_ROOT, set after `execve` of a setuid
    /// binary).
    ///
    /// Valid stored values (rejected at the `PR_SET_DUMPABLE`
    /// surface, not here):
    ///   - 0 = `SUID_DUMP_DISABLE` (no core dump, /proc/self/* owned by
    ///     root)
    ///   - 1 = `SUID_DUMP_USER` (the default for every normal process —
    ///     dumpable, /proc/self/* owned by real uid)
    ///   - 2 = `SUID_DUMP_ROOT` (Linux sets this transiently after
    ///     execve of a setuid binary; user-callable only with privilege)
    ///
    /// Default 1 (`SUID_DUMP_USER`) so a freshly-forked process matches
    /// what Linux userspace expects to read back from
    /// `PR_GET_DUMPABLE`.  Inherited verbatim across fork (Linux
    /// semantics).  Linux *resets* dumpable to 1 on every successful
    /// `execve`, regardless of the prior value, unless the binary is
    /// setuid (then 2) or PR_SET_DUMPABLE(0) is "sticky" through
    /// /proc/sys/fs/suid_dumpable — we don't model setuid binaries
    /// and we don't have an exec hook for this yet, so the exec-time
    /// reset is a known limitation tracked in todo.txt.
    pub linux_dumpable: u32,
    /// Linux `prctl(PR_SET_KEEPCAPS)` flag.  Controls whether the
    /// process retains its permitted-capability set across a uid
    /// change to non-root.  Stored as 0 (`KEEPCAPS_CLEAR`, the
    /// default — capabilities cleared on uid change) or 1
    /// (`KEEPCAPS_KEEP` — capabilities preserved).
    ///
    /// Inherited verbatim across fork on Linux (the per-thread
    /// keepcaps flag is preserved by `copy_process`); Linux *resets*
    /// it to 0 on every successful `execve`.  We preserve across
    /// exec for now — same exec-time hook limitation as
    /// `linux_dumpable`.  Tracked in todo.txt.
    ///
    /// We do not model POSIX capability sets, so the flag has no
    /// effect on actual privilege transitions — it exists purely for
    /// ABI round-trip so that programs which set and then read it
    /// back observe the value they wrote.
    pub linux_keepcaps: u32,
    /// Linux `prctl(PR_SET_NO_NEW_PRIVS)` sticky flag.  Once set to
    /// 1, execve(2) cannot grant privileges that the caller didn't
    /// already have (setuid bits become no-ops, file capabilities
    /// become non-functional, AT_SECURE is forced).  Once 1, **can
    /// never be unset** — Linux explicitly refuses to ever clear it,
    /// and the documented sticky semantics let sandboxes rely on
    /// the bit being monotonically increasing.
    ///
    /// Default 0.  Inherited verbatim across fork (Linux semantics).
    /// Also preserved across execve (Linux semantics — unlike
    /// `linux_dumpable` and `linux_keepcaps`, NNP propagates through
    /// exec by design so a sandbox parent can `fork`+`execve` an
    /// untrusted child without the child being able to escape NNP).
    ///
    /// We do not model setuid binaries so NNP has no effect on
    /// actual privilege transitions; it exists purely for ABI
    /// round-trip.  systemd, dbus, and chromium's sandbox all probe
    /// this flag during startup.
    pub linux_no_new_privs: u32,
    /// Linux `prctl(PR_SET_CHILD_SUBREAPER)` flag.  When set, the
    /// process becomes the "subreaper" for any orphaned descendant —
    /// instead of being reparented to pid 1 (init), an orphaned
    /// process is reparented to the nearest ancestor that has this
    /// flag set.  systemd uses this for per-service supervision so a
    /// daemon's grandchildren can be reaped by the supervisor
    /// instead of escaping to init.
    ///
    /// Default 0.  **NOT inherited across fork** on Linux — a forked
    /// child starts as a non-subreaper regardless of the parent's
    /// flag (the parent's subreaper-ness still affects the child's
    /// re-parenting destination, but the child does not itself
    /// inherit the bit).  Preserved across exec.
    ///
    /// We store the flag per-PCB purely for ABI round-trip;
    /// re-parenting on orphan is not yet wired in our process
    /// lifecycle (no per-PCB "find subreaper ancestor" walk).
    /// Tracked in todo.txt.
    pub linux_child_subreaper: u32,
    /// Linux `prctl(PR_SET_THP_DISABLE)` flag.  When set, transparent
    /// huge pages are disabled for the process's address space.  On
    /// Linux this is stored as `MMF_DISABLE_THP` on the `mm_struct`;
    /// we store it per-PCB instead.
    ///
    /// Default 0 (THP enabled — the system-wide policy applies).
    /// Inherited verbatim across fork (Linux: mm flags are copied
    /// from parent's mm_struct when the child mm is set up).
    /// Linux *clears* this on execve (the new mm gets default
    /// flags); we preserve across exec for now — same exec-hook
    /// limitation as the other prctl-flag entries.
    ///
    /// We do not implement THP at all (every page is a single 16
    /// KiB base page in our design), so the flag has no effect on
    /// actual page allocation.  It exists purely for ABI round-trip.
    pub linux_thp_disable: u32,
    /// Linux `prctl(PR_SET_TIMERSLACK)` — per-process timer slack
    /// in nanoseconds.  Power-management daemons set this larger
    /// on background processes so the kernel can coalesce timer
    /// expirations and reduce wakeups; foreground processes get a
    /// smaller (or zero) value.  Stored on `task_struct` in Linux.
    ///
    /// Default: 50_000 ns (50us) — the Linux compile-time
    /// `DEFAULT_TIMER_SLACK_NS`.  PR_SET_TIMERSLACK with arg2 == 0
    /// restores the per-process *default* recorded at fork time
    /// (see `linux_timer_slack_default_ns` below) — NOT the
    /// system-wide 50us constant.  Inherited verbatim across fork.
    /// Preserved across exec (Linux does NOT reset this on execve).
    ///
    /// We do not actually use this for anything yet — our timer
    /// subsystem does not coalesce.  Stored purely for ABI
    /// round-trip; future timer-coalescing work will consult it.
    pub linux_timer_slack_ns: u64,
    /// Linux `task_struct::default_timer_slack_ns` — the value to
    /// restore when `prctl(PR_SET_TIMERSLACK, 0)` is called.  Set
    /// at process creation from the compile-time default
    /// (50_000 ns) and inherited verbatim across fork (so a
    /// child's "default" matches whatever the parent had when
    /// fork happened).  Preserved across exec.  Linux exposes
    /// no syscall to change this directly; it is purely the
    /// reset target for `PR_SET_TIMERSLACK(0)`.
    pub linux_timer_slack_default_ns: u64,
    /// Linux `prctl(PR_SET_TSC)` mode.  Controls whether
    /// userspace `RDTSC` / `RDTSCP` raises `SIGSEGV` (sandboxes
    /// use this to force determinism; some VMM hot-patchers
    /// also probe it).  Encoded with Linux's user-visible
    /// values:
    ///
    /// * `1` = `PR_TSC_ENABLE`  — TSC reads allowed (default).
    /// * `2` = `PR_TSC_SIGSEGV` — TSC reads raise `SIGSEGV`.
    ///
    /// On Linux this corresponds to `TIF_NOTSC` on
    /// `thread_info`; we store the user-visible value
    /// directly (no internal bit-flip) because it makes the
    /// PR_GET path a trivial copy.
    ///
    /// Default 1 (TSC enabled).  Inherited verbatim across
    /// fork (Linux: `TIF_NOTSC` is in the thread_info copy
    /// path).  Preserved across exec (Linux's `flush_thread`
    /// does not touch `TIF_NOTSC`).
    ///
    /// Known limitation: we do not actually wire the
    /// `CR4.TSD` bit on context switch yet, so the flag is
    /// round-tripped for ABI compatibility but `RDTSC` reads
    /// never trap.  Sandbox callers will still see the right
    /// PR_GET answer, just no enforcement.  Tracked in todo.txt.
    pub linux_tsc_mode: u32,
    /// Linux `prctl(PR_MCE_KILL)` policy.  Selects what happens to
    /// the process when a machine-check exception unmaps a page it
    /// holds: kill *early* (before recovery), kill *late* (after
    /// recovery fails), or use the system *default*.
    ///
    /// Encoded with Linux's user-visible values:
    /// * 0 = `PR_MCE_KILL_LATE`
    /// * 1 = `PR_MCE_KILL_EARLY`
    /// * 2 = `PR_MCE_KILL_DEFAULT`  — system policy applies
    ///   (the documented default).
    ///
    /// On Linux this is encoded as a pair of bits in
    /// `task_struct::flags` (`PF_MCE_PROCESS` + `PF_MCE_EARLY`);
    /// we collapse the encoding into a single `u32` storing the
    /// user-visible value, so the PR_MCE_KILL_GET path is a
    /// trivial read.
    ///
    /// Default 2 (`PR_MCE_KILL_DEFAULT`).  Inherited verbatim
    /// across fork (Linux: the two PF_MCE bits are in the
    /// `task_struct::flags` copy path).  Preserved across exec
    /// (`flush_thread` does not touch the bits).
    ///
    /// Known limitation: we do not implement machine-check
    /// exception handling at all.  The stored value is round-tripped
    /// for ABI compatibility only.  When MCE handling lands it
    /// should consult `get_mce_kill_policy(pid)` to choose between
    /// SIGBUS-immediately vs. let-recovery-try-first.
    pub linux_mce_kill_policy: u32,
    /// Linux `prctl(PR_SET_MDWE)` bits — Memory Deny Write+Execute.
    /// A security policy that forbids any subsequent `mmap`/
    /// `mprotect` from setting both `PROT_WRITE` and `PROT_EXEC`
    /// on the same page range; used by sandboxes (Chromium, Firefox,
    /// systemd hardened services) to prevent JIT-spray injection.
    ///
    /// Bitmask of:
    /// * `PR_MDWE_REFUSE_EXEC_GAIN` (1) — refuse any mmap/mprotect
    ///   that would make a writable region executable.
    /// * `PR_MDWE_NO_INHERIT` (2) — clear the flag on execve.
    ///   Only valid when `REFUSE_EXEC_GAIN` is also set.
    ///
    /// Default 0 (no policy).  STICKY MONOTONE: once a non-zero
    /// value has been installed, any attempt to set a different
    /// value (including 0) returns `EPERM`; only setting the same
    /// value again is allowed.
    ///
    /// Inheritance: across fork, the bits are copied verbatim
    /// (Linux: `mm->flags` MDWE bits are duplicated by
    /// `dup_mm_flags`).  Across exec, the bits are CLEARED iff
    /// `PR_MDWE_NO_INHERIT` was set; otherwise they're preserved.
    /// We do not have an exec-time hook yet, so we preserve
    /// unconditionally (same caveat as dumpable/keepcaps — tracked
    /// in todo.txt).
    ///
    /// Known limitation: we do not actually consult this flag in
    /// `mmap` / `mprotect` yet.  The flag is round-tripped per
    /// ABI compatibility but the security promise of refusing
    /// `PROT_WRITE|PROT_EXEC` is NOT honoured.  Sandbox callers
    /// will still see the right PR_GET answer.  Will need an
    /// `mmap`/`mprotect` hook to consult `get_mdwe_bits(pid)` and
    /// return `EACCES` on a forbidden combination.  Tracked in
    /// todo.txt.
    pub linux_mdwe_bits: u32,
    /// Linux `prctl(PR_SET_IO_FLUSHER)` bit — the calling task is
    /// part of the I/O flushing path (e.g. `drbd-worker`,
    /// `multipathd`, `nbd-client`, `dm_crypt_write` worker).  Linux
    /// uses this to mark tasks that must be allowed to make memory
    /// reclaim progress even while the writeback path is congested
    /// (avoids a self-deadlock where the flusher needs free pages to
    /// flush, but reclaim is waiting for the flusher to finish).
    ///
    /// Stored as 0/1.  Default 0.  Inherited verbatim across fork
    /// (Linux: `PR_IO_FLUSHER` is a `task->flags` bit copied by
    /// `copy_process`).  Preserved across exec (`flush_thread` does
    /// not touch it).
    ///
    /// Known limitation: we do not implement memory reclaim or
    /// writeback at all yet, so the flag is round-tripped for ABI
    /// compatibility only.  When reclaim lands it should check
    /// `get_io_flusher(pid)` and grant the same `__GFP_MEMALLOC`
    /// fast-path that Linux uses for these tasks.
    pub linux_io_flusher: u32,
    /// Linux `prctl(PR_SET_MEMORY_MERGE)` bit — Kernel Same-page
    /// Merging (KSM) opt-in.  When set (1), the kernel is allowed to
    /// merge identical pages in this task's anonymous VMAs to save
    /// memory.  Used by VM hosts (qemu/kvm), JVM-with-many-containers
    /// setups, and language runtimes with large deduplicable working
    /// sets (Python multi-process pools).
    ///
    /// Stored as 0/1.  Default 0.  Inherited verbatim across fork
    /// (Linux: `MMF_VM_MERGE_ANY` is in `mm->flags` and survives
    /// `dup_mmap`).  Preserved across exec (the flag survives
    /// `flush_old_exec`).
    ///
    /// Known limitation: we do not implement KSM at all.  The flag
    /// is round-tripped for ABI compatibility only; no actual page
    /// merging happens.  When KSM lands, the VMA-walk on each mmap
    /// must consult `get_memory_merge(pid)` and queue mergeable
    /// anonymous regions onto the KSM scanner's worklist.
    pub linux_memory_merge: u32,
    /// Linux `prctl(PR_CAP_AMBIENT, RAISE/LOWER/…)` per-task ambient
    /// capability set.  This is a bitmask of POSIX capability
    /// numbers (CAP_CHOWN=0, CAP_KILL=5, CAP_NET_ADMIN=12, …).  The
    /// ambient set is the only capability set that a non-root,
    /// non-file-capability execve preserves: systemd uses it to
    /// give services like `nm-online` CAP_NET_ADMIN without making
    /// the binary setuid.  Container runtimes use it to drop all
    /// caps and then re-add a hand-picked few.
    ///
    /// Stored as a u64 bitmask, indexed by capability number.
    /// Default 0 (empty set).  Inherited verbatim across fork
    /// (Linux: `cred->ambient` is copied by `prepare_cred`).
    /// Preserved across exec — this is the defining property of
    /// the ambient set (compare to `cred->cap_inheritable`, which
    /// is also preserved across exec but is gated by file
    /// capabilities).
    ///
    /// Last valid cap (CAP_LAST_CAP) is fixed at 40
    /// (CAP_CHECKPOINT_RESTORE in Linux 5.9+).  Any cap number
    /// above 40 is rejected with EINVAL by the syscall surface;
    /// the storage helper accepts arbitrary u64 masks so tests
    /// can probe boundaries.
    ///
    /// Known limitation: we do not actually enforce capabilities
    /// anywhere — all processes have effective root anyway.  The
    /// ambient set is round-tripped for ABI compatibility only.
    /// When capability enforcement lands, every syscall that
    /// currently grants implicit privilege (mount, kexec_load,
    /// reboot, …) must consult both the ambient and effective
    /// sets to decide whether to permit the call.
    pub linux_ambient_caps: u64,
    /// Linux `prctl(PR_SET_SECUREBITS)` per-task securebits
    /// bitfield.  Eight bits in four (flag, locked) pairs that
    /// modify how the kernel handles uid 0 and capability
    /// inheritance:
    ///
    /// | Bit | Constant                       | Effect                           |
    /// |-----|--------------------------------|----------------------------------|
    /// | 0   | SECBIT_NOROOT                  | uid 0 doesn't grant caps         |
    /// | 1   | SECBIT_NOROOT_LOCKED           | bit 0 frozen                     |
    /// | 2   | SECBIT_NO_SETUID_FIXUP         | no cap reset across setuid       |
    /// | 3   | SECBIT_NO_SETUID_FIXUP_LOCKED  | bit 2 frozen                     |
    /// | 4   | SECBIT_KEEP_CAPS               | retain caps over setuid (legacy) |
    /// | 5   | SECBIT_KEEP_CAPS_LOCKED        | bit 4 frozen                     |
    /// | 6   | SECBIT_NO_CAP_AMBIENT_RAISE    | block PR_CAP_AMBIENT_RAISE       |
    /// | 7   | SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED | bit 6 frozen                 |
    ///
    /// Once a `_LOCKED` bit is set, its companion flag bit and the
    /// lock bit itself become immutable for the lifetime of the
    /// process (and any forked children that inherit it).  This
    /// gives a hardened process a way to permanently relinquish
    /// privilege transitions.
    ///
    /// Default 0 (all bits clear).  Inherited verbatim across fork
    /// (Linux: `cred->securebits` is copied by `prepare_cred`).
    /// Preserved across exec (Linux clears KEEP_CAPS on exec but
    /// leaves the other bits — caveat: we don't have an exec hook
    /// yet, so we preserve everything, including KEEP_CAPS.  Same
    /// limitation pattern as MDWE/IO_FLUSHER; tracked in todo.txt).
    ///
    /// The storage helper bypasses lock validation so test fixtures
    /// can probe boundary cases.  The syscall surface enforces:
    /// (a) unknown bits -> EINVAL, (b) attempting to clear a lock
    /// bit that is currently set -> EPERM, (c) attempting to flip
    /// a bit whose lock is set -> EPERM.
    pub linux_securebits: u32,
    /// Linux `prctl(PR_CAPBSET_READ)` / `PR_CAPBSET_DROP`
    /// per-task capability bounding set.  Bit `n` corresponds to
    /// capability number `n` (`CAP_CHOWN = 0`, …,
    /// `CAP_CHECKPOINT_RESTORE = 40`).  A set bit means the cap
    /// *may* appear in the permitted set of any execve'd binary;
    /// dropping a bit permanently removes that capability from
    /// the bounding set for this process and all descendants
    /// (the bounding set is monotone-shrinking by design).
    ///
    /// Default: [`LINUX_CAP_FULL_SET`] — all caps 0..=40 set,
    /// matching Linux's `init_cred.cap_bset` and the value every
    /// uid-0 process starts with.
    ///
    /// Fork inheritance: verbatim copy (Linux `prepare_cred`
    /// copies `cred->cap_bset` along with the rest of the
    /// credential block).
    ///
    /// Exec semantics on Linux: the bounding set is preserved
    /// across exec — it's the whole point.  We have no exec hook
    /// yet but the storage helper does the right thing
    /// automatically: PCB-level state survives.
    ///
    /// The storage helpers bypass cap-validity checks so test
    /// fixtures can install arbitrary masks.  The syscall surface
    /// enforces `cap <= LINUX_CAP_LAST_CAP`.
    pub linux_cap_bset: u64,

    /// Packed Linux `ioprio_set(2)` / `ioprio_get(2)` value for
    /// this process.
    ///
    /// Layout (matches `linux/include/uapi/linux/ioprio.h`):
    /// `(class << 13) | data`, where `class` is one of
    /// `IOPRIO_CLASS_NONE=0`, `_RT=1`, `_BE=2`, `_IDLE=3` (top 3
    /// bits) and `data` is a 0..=7 sub-priority within the class
    /// (low 13 bits).
    ///
    /// We do not run a per-task I/O scheduler — the block layer
    /// is currently FIFO — so this is a **stored-only** ABI
    /// round-trip.  `ionice -p $$ -c 1 -n 0` followed by
    /// `ionice -p $$` will see the value it just installed; the
    /// underlying I/O traffic is unaffected.  Once a CFQ / BFQ
    /// equivalent lands, this field becomes the source of truth
    /// for scheduling-class decisions.
    ///
    /// Default: `LINUX_IOPRIO_DEFAULT = (IOPRIO_CLASS_BE << 13) | 4`
    /// — Linux's documented default for tasks that have not
    /// called `ioprio_set` (the middle of the best-effort band).
    ///
    /// Fork inheritance: verbatim copy.  Linux propagates the
    /// I/O context across `clone()` unless `CLONE_IO` is unset
    /// and a fresh context is allocated; either way the initial
    /// class/data are inherited from the parent, so a plain copy
    /// is correct.
    pub linux_ioprio: i32,

    // --- Per-process I/O byte accounting (backs /proc/<pid>/io) ---
    //
    // These mirror four of Linux's `task_io_accounting` counters, kept
    // per-process and updated at the read/write syscall boundary (see
    // `account_io_read` / `account_io_write`).  We track only the four
    // fields we can populate *honestly* from the syscall layer:
    //
    //   - `io_rchar` / `io_wchar`: bytes transferred by the read/write
    //     syscall family, counted by the syscall's return value.
    //   - `io_syscr` / `io_syscw`: number of read/write syscalls issued.
    //
    // Linux's three storage-layer counters — `read_bytes`,
    // `write_bytes`, `cancelled_write_bytes` — require per-process
    // attribution inside the block layer, which we do not have.  Rather
    // than fabricate them, `/proc/<pid>/io` reports those three as 0
    // (genuinely untracked), in line with the project's "never invent
    // data in procfs" rule.  Inherited as zero across fork (Linux resets
    // task I/O accounting for a freshly-forked child).
    /// Cumulative bytes returned by read-family syscalls (`rchar`).
    pub io_rchar: u64,
    /// Cumulative bytes consumed by write-family syscalls (`wchar`).
    pub io_wchar: u64,
    /// Number of read-family syscalls issued (`syscr`).
    pub io_syscr: u64,
    /// Number of write-family syscalls issued (`syscw`).
    pub io_syscw: u64,
}

/// Highest valid Linux capability number — fixed at
/// `CAP_CHECKPOINT_RESTORE` (40), added in Linux 5.9.  Any cap
/// number above this is `EINVAL` for the `PR_CAP_AMBIENT`
/// surface and for any future cap-bearing prctl options.
pub const LINUX_CAP_LAST_CAP: u32 = 40;

/// Linux `CAP_FULL_SET` — every defined capability bit set
/// (bits 0..=40).  Matches `init_cred.cap_bset`; this is the
/// value every fresh task observes from `PR_CAPBSET_READ`
/// before anyone drops anything.
///
/// Expressed as `(1 << 41) - 1` so the constant tracks
/// `LINUX_CAP_LAST_CAP + 1` automatically if Linux extends the
/// capability range.
pub const LINUX_CAP_FULL_SET: u64 = (1u64 << (LINUX_CAP_LAST_CAP + 1)) - 1;

/// Linux I/O priority class: "no specific class" — fall back to
/// the process scheduler's class hint.  Matches
/// `IOPRIO_CLASS_NONE` (0) in `linux/uapi/linux/ioprio.h`.
pub const LINUX_IOPRIO_CLASS_NONE: i32 = 0;
/// Linux I/O priority class: real-time.  Matches
/// `IOPRIO_CLASS_RT` (1).
pub const LINUX_IOPRIO_CLASS_RT: i32 = 1;
/// Linux I/O priority class: best-effort (the default).  Matches
/// `IOPRIO_CLASS_BE` (2).
pub const LINUX_IOPRIO_CLASS_BE: i32 = 2;
/// Linux I/O priority class: idle.  Matches
/// `IOPRIO_CLASS_IDLE` (3).
pub const LINUX_IOPRIO_CLASS_IDLE: i32 = 3;

/// Shift count for the class field in the packed ioprio word.
pub const LINUX_IOPRIO_CLASS_SHIFT: i32 = 13;
/// Mask for the data (priority-within-class) field of the
/// packed ioprio word.  Linux limits user-meaningful data to
/// 0..=7 but the field itself is 13 bits wide.
pub const LINUX_IOPRIO_DATA_MASK: i32 = (1 << LINUX_IOPRIO_CLASS_SHIFT) - 1;

/// Default packed ioprio for every fresh task — best-effort
/// class at priority 4 (the middle of the BE band).  Matches
/// what `ionice -p $$` prints on a stock Linux task that has
/// never called `ioprio_set`.
pub const LINUX_IOPRIO_DEFAULT: i32 =
    (LINUX_IOPRIO_CLASS_BE << LINUX_IOPRIO_CLASS_SHIFT) | 4;

/// Securebit: uid 0 does not grant capabilities (bit 0).
pub const LINUX_SECBIT_NOROOT: u32 = 1 << 0;
/// Securebit lock: freeze [`LINUX_SECBIT_NOROOT`] (bit 1).
pub const LINUX_SECBIT_NOROOT_LOCKED: u32 = 1 << 1;
/// Securebit: no capability reset across setuid (bit 2).
pub const LINUX_SECBIT_NO_SETUID_FIXUP: u32 = 1 << 2;
/// Securebit lock: freeze [`LINUX_SECBIT_NO_SETUID_FIXUP`] (bit 3).
pub const LINUX_SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = 1 << 3;
/// Securebit: retain caps over setuid — legacy "keep caps" (bit 4).
pub const LINUX_SECBIT_KEEP_CAPS: u32 = 1 << 4;
/// Securebit lock: freeze [`LINUX_SECBIT_KEEP_CAPS`] (bit 5).
pub const LINUX_SECBIT_KEEP_CAPS_LOCKED: u32 = 1 << 5;
/// Securebit: block `PR_CAP_AMBIENT_RAISE` (bit 6).
pub const LINUX_SECBIT_NO_CAP_AMBIENT_RAISE: u32 = 1 << 6;
/// Securebit lock: freeze [`LINUX_SECBIT_NO_CAP_AMBIENT_RAISE`]
/// (bit 7).
pub const LINUX_SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 1 << 7;

/// All defined "flag" securebits (the even-numbered bits) — bits
/// that toggle a behaviour.
pub const LINUX_SECURE_ALL_BITS: u32 = LINUX_SECBIT_NOROOT
    | LINUX_SECBIT_NO_SETUID_FIXUP
    | LINUX_SECBIT_KEEP_CAPS
    | LINUX_SECBIT_NO_CAP_AMBIENT_RAISE;

/// All defined "lock" securebits (the odd-numbered bits) — bits
/// that freeze their paired flag bit.
pub const LINUX_SECURE_ALL_LOCKS: u32 = LINUX_SECBIT_NOROOT_LOCKED
    | LINUX_SECBIT_NO_SETUID_FIXUP_LOCKED
    | LINUX_SECBIT_KEEP_CAPS_LOCKED
    | LINUX_SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

/// Union of all defined securebits — any bit outside this mask in
/// `PR_SET_SECUREBITS arg2` is `EINVAL`.
pub const LINUX_SECURE_ALL_FLAGS: u32 =
    LINUX_SECURE_ALL_BITS | LINUX_SECURE_ALL_LOCKS;

/// Linux's compile-time `DEFAULT_TIMER_SLACK_NS` — the timer-slack
/// value every fresh `task_struct` starts with on Linux (and which
/// `PR_SET_TIMERSLACK(0)` resets to, modulo the parent-inheritance
/// quirk above).  50 microseconds.
pub const LINUX_DEFAULT_TIMER_SLACK_NS: u64 = 50_000;

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
            wait_any_task: None,
            ready: false,
            vmas: Vec::new(),
            ipc_handles: Vec::new(),
            crash_info: None,
            initial_fds: Vec::new(),
            initial_argv: Vec::new(),
            initial_envp: Vec::new(),
            // Persistent /proc snapshots — populated by set_initial_args
            // once the parent supplies argv/envp.
            proc_argv: Vec::new(),
            proc_envp: Vec::new(),
            abi_mode: AbiMode::Native,
            linux_fd_table: None,
            linux_saved_auxv: None,
            // Every process starts at the filesystem root.  `chdir`
            // changes this; `fork_create` clones the parent's value.
            cwd: alloc::vec![b'/'],
            // Empty until the ELF loader records the exec'd binary's path.
            exe_path: Vec::new(),
            // Compiled-in Linux rlimit defaults; modified per-process
            // by setrlimit / prlimit64 and inherited across fork.
            rlimits: DEFAULT_RLIMITS,
            // Fresh process has no Linux-mapped pages yet.
            linux_as_bytes: 0,
            // De-facto Linux distro default — what programs expect
            // when they query a freshly-spawned process's umask.
            linux_umask: 0o022,
            // PER_LINUX (no personality flags) — what every modern
            // Linux process inherits from init.
            linux_personality: 0,
            // PR_SET_PDEATHSIG default is "disabled".  Inherited
            // across fork as zero per Linux: see the explicit reset
            // in `kernel/copy_process` for the same reason
            // (children of a forked task do not inherit the
            // parent's death signal).
            linux_pdeathsig: 0,
            // Default to SCHED_OTHER, priority 0 — what every freshly
            // exec'd binary inherits on stock Linux.
            linux_sched_policy: 0,
            linux_sched_priority: 0,
            // Default nice value is 0 on Linux for every freshly
            // exec'd binary that hasn't inherited a non-zero value.
            linux_nice: 0,
            // Linux default: SUID_DUMP_USER (1) — process is
            // core-dumpable and /proc/self entries are owned by the
            // real uid.  PR_SET_DUMPABLE may flip this to 0 or 2.
            linux_dumpable: 1,
            // Linux default: KEEPCAPS_CLEAR (0) — capability set is
            // cleared on uid-change-from-root.  PR_SET_KEEPCAPS(1)
            // opts out so caps survive setuid.
            linux_keepcaps: 0,
            // Linux default: NNP cleared (0).  PR_SET_NO_NEW_PRIVS(1)
            // sets it; once set, sticky forever.
            linux_no_new_privs: 0,
            // Linux default: not a child subreaper.  systemd, dumb-init,
            // etc., opt in via PR_SET_CHILD_SUBREAPER(1).  NOT inherited
            // across fork.
            linux_child_subreaper: 0,
            // Linux default: THP enabled (system-wide policy applies).
            // PR_SET_THP_DISABLE(1) opts the process out.
            linux_thp_disable: 0,
            // Linux default: 50us timer slack (`DEFAULT_TIMER_SLACK_NS`).
            // Both the active value and the per-process "default to
            // restore on PR_SET_TIMERSLACK(0)" start at the same point.
            linux_timer_slack_ns: LINUX_DEFAULT_TIMER_SLACK_NS,
            linux_timer_slack_default_ns: LINUX_DEFAULT_TIMER_SLACK_NS,
            // Linux default: PR_TSC_ENABLE (1) — userspace TSC
            // reads are allowed.  Sandboxes set PR_TSC_SIGSEGV (2).
            linux_tsc_mode: 1,
            // Linux default: PR_MCE_KILL_DEFAULT (2) — system policy
            // applies.  Container runtimes / OOM-handling daemons
            // override.
            linux_mce_kill_policy: LINUX_PR_MCE_KILL_DEFAULT,
            // Linux default: MDWE off (0).  Sandboxes opt in via
            // PR_SET_MDWE.  Once non-zero, sticky monotone.
            linux_mdwe_bits: 0,
            // Linux default: not an I/O flusher.  Storage daemons
            // (drbd-worker, multipathd, nbd-client, dm_crypt_write)
            // set PR_SET_IO_FLUSHER on themselves at init.
            linux_io_flusher: 0,
            // Linux default: KSM merging off.  VM hosts and large
            // language runtimes opt in via PR_SET_MEMORY_MERGE.
            linux_memory_merge: 0,
            // Linux default: empty ambient set.  systemd /
            // container runtimes populate via PR_CAP_AMBIENT_RAISE.
            linux_ambient_caps: 0,
            // Linux default: securebits cleared.  Hardened
            // containers (LXC, Docker --security-opt) flip
            // SECBIT_NOROOT and friends at startup.
            linux_securebits: 0,
            // Linux default: every capability present in the
            // bounding set (CAP_FULL_SET).  Userspace narrows
            // this by calling PR_CAPBSET_DROP at startup.
            linux_cap_bset: LINUX_CAP_FULL_SET,
            // Linux default for I/O priority: best-effort class
            // (2) at priority 4 — the middle of the BE band.
            // Matches `ionice -p $$` on a stock task.
            linux_ioprio: LINUX_IOPRIO_DEFAULT,
            // Fresh process has issued no I/O yet.
            io_rchar: 0,
            io_wchar: 0,
            io_syscr: 0,
            io_syscw: 0,
        }
    }
}

/// `prctl(PR_SET_MDWE)` bit: refuse any subsequent `mmap` /
/// `mprotect` that would make a writable region executable.
pub const LINUX_PR_MDWE_REFUSE_EXEC_GAIN: u32 = 1;

/// `prctl(PR_SET_MDWE)` bit: clear the MDWE flag on `execve`.
/// Only valid when [`LINUX_PR_MDWE_REFUSE_EXEC_GAIN`] is also set.
pub const LINUX_PR_MDWE_NO_INHERIT: u32 = 2;

/// Bitmask of all defined MDWE bits — anything else in
/// `PR_SET_MDWE arg2` is `EINVAL`.
pub const LINUX_PR_MDWE_VALID_MASK: u32 =
    LINUX_PR_MDWE_REFUSE_EXEC_GAIN | LINUX_PR_MDWE_NO_INHERIT;

/// `prctl(PR_MCE_KILL)` policy: kill the process **after** the
/// kernel's recovery attempt fails.
pub const LINUX_PR_MCE_KILL_LATE: u32 = 0;
/// `prctl(PR_MCE_KILL)` policy: kill the process **before** any
/// recovery attempt (faster but loses any chance of resuming).
pub const LINUX_PR_MCE_KILL_EARLY: u32 = 1;
/// `prctl(PR_MCE_KILL)` policy: use the **system default** (this
/// is the documented per-process default).
pub const LINUX_PR_MCE_KILL_DEFAULT: u32 = 2;

/// `prctl(PR_SET_TSC)` value meaning "RDTSC reads are allowed".
pub const LINUX_PR_TSC_ENABLE: u32 = 1;

/// `prctl(PR_SET_TSC)` value meaning "RDTSC reads raise SIGSEGV".
pub const LINUX_PR_TSC_SIGSEGV: u32 = 2;

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
    PROCESSES_CREATED.fetch_add(1, Ordering::Relaxed);

    pid
}

/// Create a child process for `fork()`.
///
/// Unlike [`create`], this does **not** allocate a fresh address space —
/// the caller passes a copy-on-write clone of the parent's PML4 (built by
/// [`crate::mm::cow::clone_address_space_cow`]).  The child inherits the
/// parent's capability table, credentials, and VMA list (all cloned),
/// plus the IPC handles and initial-fd records the caller has already
/// duplicated/refcount-shared for the child.
///
/// The child starts in `Creating` state with no threads; the caller
/// spawns the child's single (forked) thread next.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if the parent no longer exists.
pub fn fork_create(
    parent_pid: ProcessId,
    child_pml4: u64,
    ipc_handles: Vec<(crate::cap::ResourceType, u64)>,
    initial_fds: Vec<(i32, u8, u64)>,
) -> KernelResult<ProcessId> {
    let mut table = PROCESS_TABLE.lock();

    // Clone the parent-derived state while holding only an immutable
    // borrow, then release it before inserting the child.
    //
    // Linux fd table inheritance contract: we duplicate the parent's
    // table *structurally* so child fds remain numerically valid.  We
    // intentionally do NOT call any per-handle dup function here —
    // refcount bumping is the caller's responsibility and is driven by
    // the parent's `ipc_handles` list.  Specifically, `fork::build_
    // fork_child` snapshots `ipc_handles`, runs `dup_one` for each
    // entry (which bumps the per-resource refcount via `pipe::dup`,
    // `fs::handle::dup_shared`, etc.), and passes the duplicated list
    // in as `ipc_handles`.  The invariant that makes this work is that
    // every Linux-ABI install path (`open_common`, `pipe_common`,
    // future `socketpair` etc.) calls `register_ipc_handle` *exactly
    // once per kernel handle per process* before installing into the
    // fd table.  Per-process `dup`/`dup2`/`dup3` only touch the fd
    // table — they do not register an additional ipc_handle entry and
    // do not bump the underlying refcount.  Combined with `sys_close`
    // checking `is_handle_referenced` before invoking the native close
    // path, this keeps the refcount at exactly "one per process that
    // holds at least one fd referencing the handle", which is what
    // fork's `dup_one` per-process bump preserves.
    let (
        name,
        cap_table,
        credentials,
        vmas,
        abi_mode,
        linux_fd_table,
        linux_saved_auxv,
        cwd,
        rlimits,
        linux_as_bytes,
        linux_umask,
        linux_personality,
        linux_sched_policy,
        linux_sched_priority,
        linux_nice,
        linux_dumpable,
        linux_keepcaps,
        linux_no_new_privs,
        linux_thp_disable,
        linux_timer_slack_ns,
        linux_timer_slack_default_ns,
        linux_tsc_mode,
        linux_mce_kill_policy,
        linux_mdwe_bits,
        linux_io_flusher,
        linux_memory_merge,
        linux_ambient_caps,
        linux_securebits,
        linux_cap_bset,
        linux_ioprio,
        proc_argv,
        proc_envp,
        exe_path,
    ) = {
        let parent = table.get(&parent_pid).ok_or(KernelError::NoSuchProcess)?;
        let cloned_fd_table = parent.linux_fd_table.as_ref().map(|t| {
            let mut copy = super::linux_fd::KernelFdTable::new();
            for (fd, entry) in t.open_entries() {
                // install_at on a fresh table cannot fail for fds in
                // range [0, MAX_FDS); the iterator only yields those.
                let _ = copy.install_at(fd, entry);
            }
            alloc::boxed::Box::new(copy)
        });
        (
            parent.name.clone(),
            parent.cap_table.clone(),
            parent.credentials.clone(),
            parent.vmas.clone(),
            parent.abi_mode,
            cloned_fd_table,
            // A forked child shares the parent's already-built SysV
            // initial stack via CoW, so it carries the same auxv until
            // it execve's (which rebuilds it).
            parent.linux_saved_auxv.clone(),
            parent.cwd.clone(),
            parent.rlimits,
            parent.linux_as_bytes,
            parent.linux_umask,
            parent.linux_personality,
            parent.linux_sched_policy,
            parent.linux_sched_priority,
            parent.linux_nice,
            parent.linux_dumpable,
            parent.linux_keepcaps,
            parent.linux_no_new_privs,
            parent.linux_thp_disable,
            parent.linux_timer_slack_ns,
            parent.linux_timer_slack_default_ns,
            parent.linux_tsc_mode,
            parent.linux_mce_kill_policy,
            parent.linux_mdwe_bits,
            parent.linux_io_flusher,
            parent.linux_memory_merge,
            parent.linux_ambient_caps,
            parent.linux_securebits,
            parent.linux_cap_bset,
            parent.linux_ioprio,
            // A forked child shares the parent's cmdline/environ until
            // it execve's (Linux semantics).
            parent.proc_argv.clone(),
            parent.proc_envp.clone(),
            // A forked child runs the same executable image until it
            // execve's, so it inherits the parent's exe path.
            parent.exe_path.clone(),
        )
    };

    // Enforce RLIMIT_NPROC (resource index 6): per-uid count of live
    // processes owned by `credentials.uid` must remain below the
    // soft limit, else fork returns EAGAIN.  Linux exempts processes
    // with CAP_SYS_RESOURCE / CAP_SYS_ADMIN; we don't have those caps
    // wired up yet, so we exempt uid 0 (root) by convention — it's
    // the same effective behaviour for the systems we run.
    //
    // RLIM_INFINITY skips the check.  This runs against the just-
    // snapshotted parent rlimits and credentials; we already hold
    // the PROCESS_TABLE lock so the count is consistent with the
    // limit decision.
    let nproc_soft = rlimits[6].0;
    if credentials.uid != 0 && nproc_soft != RLIM_INFINITY {
        let target_uid = credentials.uid;
        let mut count: u64 = 0;
        for (_, p) in table.iter() {
            // Count only live processes (not Zombie/Exited): a zombie
            // still occupies a PID slot until reaped, but Linux
            // includes them in RLIMIT_NPROC since they still hold the
            // uid quota.  We follow Linux and count all non-finalised
            // processes regardless of state.
            if p.credentials.uid == target_uid {
                count = count.saturating_add(1);
            }
        }
        // Forking adds one more.  Reject before allocating a PID so
        // we don't leak a PID on the failure path.
        if count.saturating_add(1) > nproc_soft {
            return Err(KernelError::WouldBlock);
        }
    }

    let pid = alloc_pid();
    let child = Process {
        pid,
        name,
        state: ProcessState::Creating,
        parent: parent_pid,
        threads: Vec::new(),
        cap_table,
        exit_code: None,
        credentials,
        pml4_phys: child_pml4,
        wait_task: None,
        wait_any_task: None,
        ready: false,
        vmas,
        ipc_handles,
        crash_info: None,
        initial_fds,
        // argv/envp are not re-read by a forked child — its argument
        // vector already lives in its copy-on-write userspace memory.
        initial_argv: Vec::new(),
        initial_envp: Vec::new(),
        // The persistent /proc snapshots, however, are inherited from
        // the parent so `/proc/<child>/cmdline` and `/environ` reflect
        // the shared argv/environ until the child execve's.
        proc_argv,
        proc_envp,
        // Inherited from the parent: the child runs the same image until
        // it execve's, at which point the loader overwrites this.
        exe_path,
        // Linux/native ABI is a property of the loaded binary, so a
        // forked child speaks the same ABI as its parent.
        abi_mode,
        linux_fd_table,
        linux_saved_auxv,
        // POSIX: the child inherits the parent's cwd at the moment
        // of fork.  Subsequent chdirs in either process do not affect
        // the other (each owns its own Vec).
        cwd,
        // POSIX: rlimits inherit verbatim across fork.  setrlimit in
        // either process is independent thereafter.
        rlimits,
        // The child's address space mirrors the parent's (CoW clone),
        // so it inherits the same RLIMIT_AS charge.  Each future
        // mmap/munmap in either process is accounted independently.
        linux_as_bytes,
        // POSIX: the child inherits the parent's umask at the moment
        // of fork.  Subsequent umask calls in either process are
        // independent.
        linux_umask,
        // Linux: personality() flags propagate verbatim across fork.
        // execve resets persona to PER_LINUX (0), but fork preserves
        // whatever the parent had set.
        linux_personality,
        // Linux: PR_SET_PDEATHSIG is reset across fork.  A parent
        // who has PDEATHSIG armed does not pass that arming to its
        // children; each child starts with no death signal and must
        // re-arm via prctl(PR_SET_PDEATHSIG) itself.  Same rule
        // applies across exec.  Match Linux exactly.
        linux_pdeathsig: 0,
        // Linux: scheduling policy and priority are inherited
        // verbatim across fork.  (SCHED_RESET_ON_FORK is opt-in via
        // OR'ing 0x40000000 into the policy at set time; we do not
        // implement that flag yet — see todo entry.)
        linux_sched_policy,
        linux_sched_priority,
        // Linux: nice value is inherited verbatim across fork and
        // preserved across exec.  Forked children start with the
        // same nice as their parent.
        linux_nice,
        // Linux: PR_SET_DUMPABLE state propagates verbatim across
        // fork.  Linux RESETS it to 1 on execve (unless the binary
        // is setuid, in which case it becomes 2) — we don't model
        // setuid binaries and we don't have an exec-time hook for
        // this yet, so exec preserves rather than resets.  Known
        // limitation tracked in todo.txt.
        linux_dumpable,
        // Linux: PR_SET_KEEPCAPS propagates verbatim across fork
        // and is RESET to 0 on execve.  We preserve across exec
        // for the same reason as dumpable above — pending exec-time
        // PCB cleanup hook.  Tracked in todo.txt.
        linux_keepcaps,
        // Linux: PR_SET_NO_NEW_PRIVS propagates across fork AND
        // across exec by design (it is a sticky monotone flag —
        // sandboxes rely on it being preserved through exec).  Fork
        // verbatim covers both.
        linux_no_new_privs,
        // Linux: PR_SET_CHILD_SUBREAPER is NOT inherited across
        // fork.  The parent's subreaper-ness still influences the
        // child's eventual orphan re-parenting destination, but the
        // child does not itself start as a subreaper — it must opt
        // in via prctl if it wants the role.  A forked child
        // therefore always starts with the flag cleared.
        linux_child_subreaper: 0,
        // Linux: MMF_DISABLE_THP is copied from the parent's
        // mm_struct when the child mm is set up, so PR_SET_THP_DISABLE
        // propagates verbatim across fork.  Linux CLEARS it on
        // execve (the new mm gets default flags); we preserve
        // across exec for now — same exec-hook limitation as the
        // other prctl-flag entries.  Tracked in todo.txt.
        linux_thp_disable,
        // Linux: timer_slack_ns and default_timer_slack_ns are
        // both copied verbatim from the parent's task_struct on
        // copy_process.  The child's "default" therefore is
        // whatever the parent had at fork time (NOT the
        // compile-time 50us — PR_SET_TIMERSLACK(0) in the child
        // resets to the parent's slack-at-fork value).  Both
        // values are preserved across exec.
        linux_timer_slack_ns,
        linux_timer_slack_default_ns,
        // Linux: TIF_NOTSC is in the thread_info copy path, so
        // PR_SET_TSC propagates verbatim across fork.  Preserved
        // across exec (flush_thread does not touch the flag).
        linux_tsc_mode,
        // Linux: PF_MCE_PROCESS and PF_MCE_EARLY are in the
        // task_struct::flags copy path, so PR_MCE_KILL state
        // propagates verbatim across fork.  Preserved across exec.
        linux_mce_kill_policy,
        // Linux: MDWE bits live in `mm->flags` (`MMF_HAS_MDWE_*`)
        // and are duplicated by `dup_mm_flags` at fork.  Across
        // exec the bits are cleared iff `PR_MDWE_NO_INHERIT` was
        // set; we preserve unconditionally for now — exec-hook
        // limitation tracked in todo.txt.
        linux_mdwe_bits,
        // Linux: `PR_IO_FLUSHER` is a `task->flags` bit copied by
        // `copy_process`, so the flag propagates verbatim across
        // fork.  Preserved across exec (flush_thread does not
        // touch it).
        linux_io_flusher,
        // Linux: `MMF_VM_MERGE_ANY` lives in `mm->flags` and
        // survives `dup_mmap`, so the KSM-merge opt-in propagates
        // verbatim across fork.  Preserved across exec
        // (flush_old_exec keeps the mm flags subset that
        // includes MMF_VM_MERGE_ANY).
        linux_memory_merge,
        // Linux: `cred->ambient` is copied by `prepare_cred` so
        // the ambient cap set propagates verbatim across fork.
        // Preserved across exec — this is the defining property
        // of the ambient set vs the inheritable set.
        linux_ambient_caps,
        // Linux: `cred->securebits` is copied by `prepare_cred`
        // alongside the rest of the credential block, so
        // securebits (including any locks) propagate verbatim
        // across fork.  Lock bits in the parent stay locked in
        // the child — child has no way to clear what parent
        // froze.  Preserved across exec aside from KEEP_CAPS
        // (which Linux clears on exec); we don't yet have an exec
        // hook so KEEP_CAPS survives too (todo).
        linux_securebits,
        // Linux: `cred->cap_bset` is copied by `prepare_cred`.
        // Bounding set is monotone-shrinking — child inherits
        // the parent's current set and can only narrow it.
        // Preserved across exec (that is the bounding set's
        // defining property).
        linux_cap_bset,
        // Linux: I/O priority class/data are inherited by the
        // child either via the shared io_context (CLONE_IO) or
        // via the io_context_clone path on a fresh context.
        // Either way the *initial* class/data the child observes
        // equal the parent's, so a verbatim copy is correct.
        // Preserved across exec (the io_context survives exec
        // unless O_CLOEXEC-like behaviour is opted in, which we
        // don't model).
        linux_ioprio,
        // Linux resets task I/O accounting for a freshly-forked child
        // (copy_process zeroes task->ioac); the child starts its own
        // /proc/<pid>/io counters from zero.
        io_rchar: 0,
        io_wchar: 0,
        io_syscr: 0,
        io_syscw: 0,
    };

    table.insert(pid, child);
    PROCESSES_CREATED.fetch_add(1, Ordering::Relaxed);
    Ok(pid)
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
/// Returns `(is_zombie, wait_task, any_waiter)`:
/// - `wait_task` — a task blocked in `waitpid(pid)` for *this* process,
///   to be woken now that it's a zombie.
/// - `any_waiter` — a task in this process's *parent* blocked in
///   `waitpid(-1)` (wait for any child); it must be woken so it can
///   re-scan and reap this newly-zombied child.
///
/// Both are `None` when the process did not transition to zombie.
///
/// When a process becomes a zombie, all its living children are
/// reparented to PID 1 (init) and registered as orphans so init
/// can reap them when they eventually exit.
pub fn remove_thread(
    pid: ProcessId,
    task_id: TaskId,
) -> KernelResult<(bool, Option<TaskId>, Option<TaskId>)> {
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
        // Capture the parent before re-borrowing the table so we can
        // wake any `waitpid(-1)` waiter blocked in the parent.
        let parent_of_zombie = proc.parent;

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

        // Wake a parent blocked in `waitpid(-1)`.  Take (clear) it —
        // the parent re-registers on its next blocking wait if more
        // children remain.  Guard against a process being its own
        // parent (kernel pid 0 / pathological cases).
        let any_waiter = if parent_of_zombie != pid {
            table
                .get_mut(&parent_of_zombie)
                .and_then(|p| p.wait_any_task.take())
        } else {
            None
        };

        // Drop the lock before calling initproc to avoid potential
        // lock ordering issues (PROCESS_TABLE → initproc STATE).
        drop(table);

        for &orphan_pid in &orphan_pids {
            #[allow(clippy::cast_possible_truncation)]
            let _ = crate::initproc::register_orphan(orphan_pid as u32);
        }

        return Ok((true, wake, any_waiter));
    }

    Ok((false, None, None))
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

// ---------------------------------------------------------------------------
// Per-process current working directory
// ---------------------------------------------------------------------------

/// Maximum length (in bytes, **excluding** the trailing NUL) of a
/// stored canonical cwd path.  Matches Linux's `PATH_MAX = 4096`: the
/// `getcwd` syscall must return the path plus a NUL inside a single
/// `PATH_MAX` buffer, so the stored slice itself is at most
/// `PATH_MAX - 1 = 4095` bytes.
pub const CWD_MAX_LEN: usize = 4095;

/// Read the current working directory of a process.
///
/// Returns a cloned `Vec<u8>` because the cwd lives inside the
/// PROCESS_TABLE-locked PCB; callers that want to inspect/copy out the
/// path must own the bytes.  Returns `None` if `pid` is not in the
/// table.
///
/// The returned path satisfies the invariants documented on
/// [`Process::cwd`].
#[must_use]
pub fn get_cwd(pid: ProcessId) -> Option<Vec<u8>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.cwd.clone())
}

/// Replace the current working directory of a process.
///
/// The caller is responsible for ensuring `new_cwd` already satisfies
/// the canonical-path invariants on [`Process::cwd`] (starts with `/`,
/// no `.`/`..`/empty components, no trailing `/` except root, no
/// interior NULs, length ≤ [`CWD_MAX_LEN`]).  This accessor performs
/// a defensive sanity check (start-with-`/`, length, interior NUL)
/// and rejects malformed input with `KernelError::InvalidArgument`;
/// the heavier component-level normalization is the syscall layer's
/// job and happens before we get here.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` is not in the table.
/// - [`KernelError::InvalidArgument`] if `new_cwd` violates the
///   shallow invariants above.
pub fn set_cwd(pid: ProcessId, new_cwd: Vec<u8>) -> KernelResult<()> {
    if new_cwd.is_empty() || new_cwd[0] != b'/' {
        return Err(KernelError::InvalidArgument);
    }
    if new_cwd.len() > CWD_MAX_LEN {
        return Err(KernelError::InvalidArgument);
    }
    if new_cwd.contains(&0) {
        return Err(KernelError::InvalidArgument);
    }
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;
    proc.cwd = new_cwd;
    Ok(())
}

/// Return a clone of the process's executable path, or `None` if the
/// process does not exist.  An empty `Vec` means the process has not yet
/// exec'd a binary (no `/proc/<pid>/exe` target).
///
/// Returns a cloned `Vec<u8>` because [`Process::exe_path`] lives inside
/// the lock-protected table and references cannot escape the lock.
#[must_use]
pub fn get_exe_path(pid: ProcessId) -> Option<Vec<u8>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.exe_path.clone())
}

/// Record the resolved absolute path of the executable a process has
/// loaded, backing `/proc/<pid>/exe`.
///
/// Called by the ELF loader at `exec` time with the canonical path of
/// the binary being loaded.  Overwrites any prior value (exec replaces
/// the image; the path is not carried across the exec boundary).  Stored
/// as raw bytes — a path may contain any byte except `/` and NUL.
///
/// Performs a shallow sanity check: the path must be non-empty, start
/// with `b'/'` (absolute), and contain no interior NULs.  The caller is
/// responsible for full canonicalisation.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` is not in the table.
/// - [`KernelError::InvalidArgument`] if `path` is empty, not absolute,
///   or contains an interior NUL.
pub fn set_exe_path(pid: ProcessId, path: Vec<u8>) -> KernelResult<()> {
    if path.is_empty() || path[0] != b'/' {
        return Err(KernelError::InvalidArgument);
    }
    if path.contains(&0) {
        return Err(KernelError::InvalidArgument);
    }
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;
    proc.exe_path = path;
    Ok(())
}

/// Clear a process's recorded executable path.
///
/// Used on `exec` when the caller supplied no path: the old path refers
/// to the now-replaced image and must not survive, so we drop it and
/// `/proc/<pid>/exe` reports `NotFound` until a path is recorded.
/// No-op if the process does not exist.
pub fn clear_exe_path(pid: ProcessId) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.exe_path.clear();
    }
}

// ---------------------------------------------------------------------------
// Per-process Linux resource limits
// ---------------------------------------------------------------------------

/// `RLIM_INFINITY` on Linux x86_64.  Distinct from "no limit known" —
/// `u64::MAX` is the explicit sentinel programs check for.
pub const RLIM_INFINITY: u64 = u64::MAX;

/// Compiled-in default `(rlim_cur, rlim_max)` for each Linux RLIMIT_*
/// resource, indexed by resource number.  Used to initialise every
/// fresh `Process` and as the answer for kernel-context callers that
/// have no per-process state.
///
/// Values mirror typical Linux distro defaults where they matter for
/// program startup (RLIMIT_STACK == 8 MiB so glibc sizes the main
/// stack correctly; RLIMIT_NOFILE == 1024; RLIMIT_CORE == 0 so we
/// don't pretend to support core dumps).  Everything else is
/// `RLIM_INFINITY` because nothing in the kernel imposes a real
/// limit on those resources today.
pub const DEFAULT_RLIMITS: [(u64, u64); 16] = [
    // 0  RLIMIT_CPU:        CPU seconds.  No limiter today.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 1  RLIMIT_FSIZE:      max file size.  No limiter today.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 2  RLIMIT_DATA:       data-segment size.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 3  RLIMIT_STACK:      8 MiB matches glibc's main-thread sizing.
    (8 * 1024 * 1024, RLIM_INFINITY),
    // 4  RLIMIT_CORE:       0 — we never produce core dumps.
    (0, 0),
    // 5  RLIMIT_RSS:        resident set size.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 6  RLIMIT_NPROC:      per-uid process count.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 7  RLIMIT_NOFILE:     per-process open-fd limit.  1024 matches
    //                      most Linux distros; programs that select()
    //                      on bare fd numbers rely on this fitting in
    //                      FD_SETSIZE.
    (1024, 4096),
    // 8  RLIMIT_MEMLOCK:    mlock()'d memory.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 9  RLIMIT_AS:         address-space size.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 10 RLIMIT_LOCKS:      fcntl(F_SETLK) lock count.  No tracker.
    (RLIM_INFINITY, RLIM_INFINITY),
    // 11 RLIMIT_SIGPENDING: per-uid pending signal count.  Generous cap.
    (65_536, 65_536),
    // 12 RLIMIT_MSGQUEUE:   POSIX message-queue bytes.  Linux default.
    (819_200, 819_200),
    // 13 RLIMIT_NICE:       nice ceiling.  We don't support nice.
    (0, 0),
    // 14 RLIMIT_RTPRIO:     real-time priority ceiling.  No RT today.
    (0, 0),
    // 15 RLIMIT_RTTIME:     max contiguous RT CPU microseconds.
    (RLIM_INFINITY, RLIM_INFINITY),
];

/// Number of `RLIMIT_*` resources we track.  Linux's stable kernel ABI
/// reserves the range 0..=15; anything outside should be rejected at
/// the syscall layer with `EINVAL`.
pub const NUM_RLIMITS: u32 = 16;

// Compile-time invariant: `NUM_RLIMITS` is the bound checked by
// `get_rlimit`/`set_rlimit` before they index `DEFAULT_RLIMITS` and the
// per-process `rlimits` array (e.g. `p.rlimits[resource as usize]`).  The
// `rlimits` field length is already compile-linked to `DEFAULT_RLIMITS` via
// the `rlimits: DEFAULT_RLIMITS` initializer, so guarding `DEFAULT_RLIMITS`
// against `NUM_RLIMITS` here makes the whole chain consistent.  Without this,
// bumping `NUM_RLIMITS` to add a new RLIMIT_* without extending the tables
// would turn those indexed accesses into runtime out-of-bounds panics (a DoS
// in this kernel's threat model) instead of a build failure.
const _: () = assert!(DEFAULT_RLIMITS.len() == NUM_RLIMITS as usize);

/// Read the current `(rlim_cur, rlim_max)` for `pid`'s `resource`.
///
/// Returns `None` if `pid` is unknown or `resource >= NUM_RLIMITS`.
/// Callers in kernel context (no live PCB) should use
/// [`DEFAULT_RLIMITS`] directly rather than going through this lookup.
#[must_use]
pub fn get_rlimit(pid: ProcessId, resource: u32) -> Option<(u64, u64)> {
    if resource >= NUM_RLIMITS {
        return None;
    }
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.rlimits[resource as usize])
}

/// Install a new `(rlim_cur, rlim_max)` for `pid`'s `resource`.
///
/// Enforces the two invariants Linux enforces unconditionally:
///   - `new_cur <= new_max`  (else `InvalidArgument`).
///   - `new_max <= old_max`  (else `PermissionDenied`) — raising the
///     hard limit requires `CAP_SYS_RESOURCE` on Linux; we have no
///     equivalent, so unprivileged callers can only lower the hard
///     limit.
///
/// `RLIM_INFINITY` (`u64::MAX`) is treated as "no limit"; setting a
/// finite value when the old hard limit was infinity is permitted
/// (it's a lowering operation, not a raise).
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` is unknown.
/// - [`KernelError::InvalidArgument`] if `resource >= NUM_RLIMITS` or
///   `new_cur > new_max`.
/// - [`KernelError::PermissionDenied`] if `new_max` exceeds the
///   existing hard limit.
pub fn set_rlimit(
    pid: ProcessId,
    resource: u32,
    new_cur: u64,
    new_max: u64,
) -> KernelResult<()> {
    if resource >= NUM_RLIMITS {
        return Err(KernelError::InvalidArgument);
    }
    if new_cur > new_max {
        return Err(KernelError::InvalidArgument);
    }
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;
    let (_, old_max) = proc.rlimits[resource as usize];
    if new_max > old_max {
        return Err(KernelError::PermissionDenied);
    }
    proc.rlimits[resource as usize] = (new_cur, new_max);
    Ok(())
}

/// Read the current Linux file-mode creation mask for `pid`.
///
/// Returns `None` if `pid` is unknown.  The returned value is always
/// in the range `0..=0o777` (set_umask masks higher bits on the way
/// in).  Callers in kernel context (no live PCB) should fall back to
/// the `0o022` distro default rather than going through this lookup.
#[must_use]
pub fn get_umask(pid: ProcessId) -> Option<u16> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_umask)
}

/// Install a new Linux file-mode creation mask for `pid`, returning
/// the old one.
///
/// The new mask is masked with `& 0o777` before being stored, matching
/// Linux's `umask(2)` semantics — out-of-range bits are silently
/// dropped, never rejected.  Returns `None` if `pid` is unknown.
pub fn set_umask(pid: ProcessId, new: u16) -> Option<u16> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_umask;
    proc.linux_umask = new & 0o777;
    Some(old)
}

/// Read the current Linux personality flags for `pid`.
///
/// Returns `None` if `pid` is unknown.  Callers in kernel context (no
/// live PCB) should fall back to `0` (`PER_LINUX`).
#[must_use]
pub fn get_personality(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_personality)
}

/// Install a new Linux personality value for `pid`, returning the old
/// one.
///
/// Linux stores the full 32-bit value (low byte = persona, upper bytes
/// = flags) verbatim; range validation (e.g. rejecting non-PER_LINUX
/// personae) is the caller's responsibility.  Returns `None` if `pid`
/// is unknown.
pub fn set_personality(pid: ProcessId, new: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_personality;
    proc.linux_personality = new;
    Some(old)
}

/// Read the parent-death signal armed via `prctl(PR_SET_PDEATHSIG)`
/// for `pid`.  Returns `None` if `pid` is unknown, `Some(0)` if no
/// death signal is armed.
#[must_use]
pub fn get_pdeathsig(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_pdeathsig)
}

/// Install a new parent-death signal value for `pid`, returning the
/// old one.
///
/// `sig == 0` is the documented "disable" value.  Caller is
/// responsible for range validation (Linux accepts 0..=64).  Returns
/// `None` if `pid` is unknown.
pub fn set_pdeathsig(pid: ProcessId, sig: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_pdeathsig;
    proc.linux_pdeathsig = sig;
    Some(old)
}

/// Read the recorded `sched_setscheduler` policy for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (SCHED_OTHER) for every newly-created process that has
/// not yet called `sched_setscheduler`.
#[must_use]
pub fn get_sched_policy(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_sched_policy)
}

/// Install a new scheduling policy for `pid`, returning the prior
/// value.  Caller is responsible for policy validation (this helper
/// stores whatever value it is given).  Returns `None` if `pid` is
/// unknown.
pub fn set_sched_policy(pid: ProcessId, policy: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_sched_policy;
    proc.linux_sched_policy = policy;
    Some(old)
}

/// Read the recorded `sched_priority` for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (SCHED_OTHER demands priority 0).
#[must_use]
pub fn get_sched_priority(pid: ProcessId) -> Option<i32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_sched_priority)
}

/// Install a new sched priority for `pid`, returning the prior
/// value.  Caller is responsible for range validation against the
/// process's current policy.  Returns `None` if `pid` is unknown.
pub fn set_sched_priority(pid: ProcessId, prio: i32) -> Option<i32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_sched_priority;
    proc.linux_sched_priority = prio;
    Some(old)
}

/// Read the recorded `setpriority` nice value for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default for every newly-created process.  The value is the
/// *logical* nice in -20..=19, not the `getpriority`-encoded form.
#[must_use]
pub fn get_nice(pid: ProcessId) -> Option<i32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_nice)
}

/// Install a new nice value for `pid`, returning the prior value.
/// Caller is responsible for clamping to -20..=19; this helper
/// stores whatever it is given.  Returns `None` if `pid` is
/// unknown.
pub fn set_nice(pid: ProcessId, nice: i32) -> Option<i32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_nice;
    proc.linux_nice = nice;
    Some(old)
}

/// Read the recorded `prctl(PR_SET_DUMPABLE)` flag for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(1)` is the documented
/// default (`SUID_DUMP_USER` — process is dumpable and its
/// `/proc/self/*` entries are owned by the real uid).
#[must_use]
pub fn get_dumpable(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_dumpable)
}

/// Install a new dumpable flag for `pid`, returning the prior
/// value.  Caller is responsible for validating the value is one
/// of 0 (`SUID_DUMP_DISABLE`), 1 (`SUID_DUMP_USER`), or 2
/// (`SUID_DUMP_ROOT`); this helper stores whatever it is given.
/// Returns `None` if `pid` is unknown.
pub fn set_dumpable(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_dumpable;
    proc.linux_dumpable = val;
    Some(old)
}

/// Read the recorded `prctl(PR_SET_KEEPCAPS)` flag for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (`KEEPCAPS_CLEAR` — capabilities cleared on uid change).
#[must_use]
pub fn get_keepcaps(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_keepcaps)
}

/// Install a new keepcaps flag for `pid`, returning the prior
/// value.  Caller is responsible for validating the value is 0 or
/// 1; this helper stores whatever it is given.  Returns `None` if
/// `pid` is unknown.
pub fn set_keepcaps(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_keepcaps;
    proc.linux_keepcaps = val;
    Some(old)
}

/// Read the recorded `prctl(PR_SET_NO_NEW_PRIVS)` sticky flag for
/// `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (NNP cleared — execve may grant new privileges).
#[must_use]
pub fn get_no_new_privs(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_no_new_privs)
}

/// Install the no-new-privs flag for `pid`, returning the prior
/// value.
///
/// **Sticky semantics**: this helper itself does not enforce stickiness
/// — the syscall surface for `PR_SET_NO_NEW_PRIVS` always passes 1
/// (Linux rejects any other value with EINVAL) and the bit, once set,
/// is never cleared by any other ABI path.  The helper accepts an
/// arbitrary value so that future code (test fixtures, exec-time
/// hooks) can manipulate it; the surface must not.  Returns `None` if
/// `pid` is unknown.
pub fn set_no_new_privs(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_no_new_privs;
    proc.linux_no_new_privs = val;
    Some(old)
}

/// Read the recorded `prctl(PR_SET_CHILD_SUBREAPER)` flag for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (not a subreaper).
#[must_use]
pub fn get_child_subreaper(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_child_subreaper)
}

/// Install the child-subreaper flag for `pid`, returning the prior
/// value.  Linux's `PR_SET_CHILD_SUBREAPER` normalises the input to
/// `!!arg2` (any non-zero argument becomes 1); we leave that
/// normalisation to the syscall surface and store whatever value the
/// caller provides here.  Returns `None` if `pid` is unknown.
pub fn set_child_subreaper(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_child_subreaper;
    proc.linux_child_subreaper = val;
    Some(old)
}

/// Read the recorded `prctl(PR_SET_THP_DISABLE)` flag for `pid`.
///
/// Returns `None` if `pid` is unknown; `Some(0)` is the documented
/// default (THP enabled — system-wide policy applies).  The flag is a
/// per-process opt-out for transparent huge pages; we don't implement
/// THP at all, so the value is round-tripped for ABI compatibility
/// only and has no effect on actual page allocation.
#[must_use]
pub fn get_thp_disable(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_thp_disable)
}

/// Install the THP-disable flag for `pid`, returning the prior value.
/// Linux's `PR_SET_THP_DISABLE` normalises the input to `!!arg2` (any
/// non-zero argument becomes 1); we leave that normalisation to the
/// syscall surface and store whatever value the caller provides here.
/// Returns `None` if `pid` is unknown.
pub fn set_thp_disable(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_thp_disable;
    proc.linux_thp_disable = val;
    Some(old)
}

/// Read the recorded `prctl(PR_GET_TIMERSLACK)` value (nanoseconds)
/// for `pid`.  Returns `None` if `pid` is unknown; the documented
/// default for fresh processes is `LINUX_DEFAULT_TIMER_SLACK_NS`
/// (50_000 ns).
#[must_use]
pub fn get_timer_slack_ns(pid: ProcessId) -> Option<u64> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_timer_slack_ns)
}

/// Install the timer-slack value for `pid`, returning the prior
/// value.  The syscall surface for `PR_SET_TIMERSLACK` interprets
/// `arg2 == 0` as "restore the per-process default" — that
/// remapping happens at the surface (it consults
/// `get_timer_slack_default_ns`), so this helper accepts the value
/// to store directly and does not interpret 0 specially.  Returns
/// `None` if `pid` is unknown.
pub fn set_timer_slack_ns(pid: ProcessId, val: u64) -> Option<u64> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_timer_slack_ns;
    proc.linux_timer_slack_ns = val;
    Some(old)
}

/// Read the per-process *default* timer slack — the value that
/// `PR_SET_TIMERSLACK(0)` resets the active slack to.  Set at fork
/// time from the parent's default (so a child's "default" matches
/// the parent's at-fork value, not the compile-time constant).
/// Returns `None` if `pid` is unknown.
#[must_use]
pub fn get_timer_slack_default_ns(pid: ProcessId) -> Option<u64> {
    PROCESS_TABLE
        .lock()
        .get(&pid)
        .map(|p| p.linux_timer_slack_default_ns)
}

/// Read the recorded `prctl(PR_GET_TSC)` mode for `pid` (one of
/// [`LINUX_PR_TSC_ENABLE`] / [`LINUX_PR_TSC_SIGSEGV`]).  Returns
/// `None` if `pid` is unknown; `Some(LINUX_PR_TSC_ENABLE)` is the
/// documented default.
#[must_use]
pub fn get_tsc_mode(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_tsc_mode)
}

/// Install the TSC mode for `pid`, returning the prior value.  The
/// syscall surface validates `val` is in {1, 2}; this helper accepts
/// arbitrary values so test fixtures / future kernel paths can set
/// it freely.  Returns `None` if `pid` is unknown.
pub fn set_tsc_mode(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_tsc_mode;
    proc.linux_tsc_mode = val;
    Some(old)
}

/// Read the recorded `prctl(PR_MCE_KILL_GET)` policy for `pid`
/// (one of [`LINUX_PR_MCE_KILL_LATE`] / [`LINUX_PR_MCE_KILL_EARLY`]
/// / [`LINUX_PR_MCE_KILL_DEFAULT`]).  Returns `None` if `pid` is
/// unknown; `Some(LINUX_PR_MCE_KILL_DEFAULT)` is the documented
/// default.
#[must_use]
pub fn get_mce_kill_policy(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE
        .lock()
        .get(&pid)
        .map(|p| p.linux_mce_kill_policy)
}

/// Install the MCE-kill policy for `pid`, returning the prior
/// value.  The syscall surface validates the value is one of the
/// three documented constants; this helper accepts arbitrary values
/// so test fixtures and future kernel paths can set it freely.
/// Returns `None` if `pid` is unknown.
pub fn set_mce_kill_policy(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_mce_kill_policy;
    proc.linux_mce_kill_policy = val;
    Some(old)
}

/// Read the recorded `prctl(PR_GET_MDWE)` bits for `pid` (a
/// bitmask of [`LINUX_PR_MDWE_REFUSE_EXEC_GAIN`] /
/// [`LINUX_PR_MDWE_NO_INHERIT`]).  Returns `None` if `pid` is
/// unknown; `Some(0)` is the documented default (no policy).
#[must_use]
pub fn get_mdwe_bits(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_mdwe_bits)
}

/// Install the MDWE bits for `pid`, returning the prior value.
///
/// **Sticky monotone semantics**: this helper itself does not
/// enforce the "cannot change once set" rule — the syscall surface
/// for `PR_SET_MDWE` performs that check (re-setting to the same
/// non-zero value is allowed; any other change once non-zero is
/// `EPERM`).  The helper accepts arbitrary values so test fixtures
/// can manipulate it freely.  Returns `None` if `pid` is unknown.
pub fn set_mdwe_bits(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_mdwe_bits;
    proc.linux_mdwe_bits = val;
    Some(old)
}

/// Read the `PR_GET_IO_FLUSHER` bit for `pid` (0 or 1).  Returns
/// `None` if `pid` is unknown; `Some(0)` is the default.
#[must_use]
pub fn get_io_flusher(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_io_flusher)
}

/// Install the `PR_SET_IO_FLUSHER` bit (must be 0 or 1) for `pid`,
/// returning the prior value.  Returns `None` if `pid` is unknown.
/// The helper does not validate the value — the syscall surface
/// rejects anything outside {0, 1}.
pub fn set_io_flusher(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_io_flusher;
    proc.linux_io_flusher = val;
    Some(old)
}

/// Read the `PR_GET_MEMORY_MERGE` bit for `pid` (0 or 1).  Returns
/// `None` if `pid` is unknown; `Some(0)` is the default.
#[must_use]
pub fn get_memory_merge(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_memory_merge)
}

/// Install the `PR_SET_MEMORY_MERGE` bit (must be 0 or 1) for
/// `pid`, returning the prior value.  Returns `None` if `pid` is
/// unknown.  The helper does not validate the value — the syscall
/// surface rejects anything outside {0, 1}.
pub fn set_memory_merge(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_memory_merge;
    proc.linux_memory_merge = val;
    Some(old)
}

/// Read the full `PR_CAP_AMBIENT` mask for `pid` (bitmask indexed
/// by capability number).  Returns `None` if `pid` is unknown;
/// `Some(0)` is the default (empty set).
#[must_use]
pub fn get_ambient_caps(pid: ProcessId) -> Option<u64> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_ambient_caps)
}

/// Install the full `PR_CAP_AMBIENT` mask for `pid`, returning
/// the prior value.  Returns `None` if `pid` is unknown.  Bypasses
/// the syscall surface's cap-validity check so test fixtures can
/// install arbitrary masks (including bits beyond
/// [`LINUX_CAP_LAST_CAP`]).
pub fn set_ambient_caps(pid: ProcessId, val: u64) -> Option<u64> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_ambient_caps;
    proc.linux_ambient_caps = val;
    Some(old)
}

/// Raise (set to 1) the bit for capability `cap` in the ambient
/// set of `pid`.  Returns the prior value of the bit (0 or 1), or
/// `None` if `pid` is unknown.  Does not validate `cap` — caller
/// (syscall surface) must verify `cap <= LINUX_CAP_LAST_CAP`.
pub fn raise_ambient_cap(pid: ProcessId, cap: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let bit: u64 = 1 << cap;
    let was_set = u32::from((proc.linux_ambient_caps & bit) != 0);
    proc.linux_ambient_caps |= bit;
    Some(was_set)
}

/// Lower (set to 0) the bit for capability `cap` in the ambient
/// set of `pid`.  Returns the prior value of the bit (0 or 1), or
/// `None` if `pid` is unknown.
pub fn lower_ambient_cap(pid: ProcessId, cap: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let bit: u64 = 1 << cap;
    let was_set = u32::from((proc.linux_ambient_caps & bit) != 0);
    proc.linux_ambient_caps &= !bit;
    Some(was_set)
}

/// Query whether capability `cap` is in the ambient set of `pid`.
/// Returns `Some(0)` or `Some(1)` if `pid` exists; `None` if not.
#[must_use]
pub fn is_ambient_cap_set(pid: ProcessId, cap: u32) -> Option<u32> {
    let bit: u64 = 1 << cap;
    PROCESS_TABLE
        .lock()
        .get(&pid)
        .map(|p| u32::from((p.linux_ambient_caps & bit) != 0))
}

/// Read the full capability bounding set for `pid`.  Returns
/// `None` if `pid` is unknown; `Some(LINUX_CAP_FULL_SET)` is the
/// default for a fresh process.
#[must_use]
pub fn get_cap_bset(pid: ProcessId) -> Option<u64> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_cap_bset)
}

/// Install the full capability bounding set for `pid`, returning
/// the prior value.  Returns `None` if `pid` is unknown.  Bypasses
/// the syscall surface's cap-validity check so test fixtures can
/// install arbitrary masks (including bits beyond
/// [`LINUX_CAP_LAST_CAP`]).  Does NOT enforce monotonicity — the
/// caller (or a future setter wired to a sandbox) must keep the
/// bounding set monotone-shrinking; the unrestricted helper exists
/// for test setup.
pub fn set_cap_bset(pid: ProcessId, val: u64) -> Option<u64> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_cap_bset;
    proc.linux_cap_bset = val;
    Some(old)
}

/// Query whether capability `cap` is in the bounding set of
/// `pid`.  Returns `Some(0)` or `Some(1)` if `pid` exists; `None`
/// if not.  Does not validate `cap` — caller must verify
/// `cap <= LINUX_CAP_LAST_CAP`.
#[must_use]
pub fn is_cap_in_bset(pid: ProcessId, cap: u32) -> Option<u32> {
    let bit: u64 = 1 << cap;
    PROCESS_TABLE
        .lock()
        .get(&pid)
        .map(|p| u32::from((p.linux_cap_bset & bit) != 0))
}

/// Drop capability `cap` from the bounding set of `pid`.  Returns
/// the prior value of the bit (0 or 1), or `None` if `pid` is
/// unknown.  The bounding set is monotone-shrinking so this is
/// the only mutator besides [`set_cap_bset`] (which exists for
/// test setup).  Does not validate `cap` — caller must verify
/// `cap <= LINUX_CAP_LAST_CAP`.
pub fn drop_cap_from_bset(pid: ProcessId, cap: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let bit: u64 = 1 << cap;
    let was_set = u32::from((proc.linux_cap_bset & bit) != 0);
    proc.linux_cap_bset &= !bit;
    Some(was_set)
}

/// Read the `PR_GET_SECUREBITS` value for `pid`.  Returns `None` if
/// `pid` is unknown; `Some(0)` is the default (all bits clear).
#[must_use]
pub fn get_securebits(pid: ProcessId) -> Option<u32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_securebits)
}

/// Install the `PR_SET_SECUREBITS` value for `pid`, returning the
/// prior value.  Returns `None` if `pid` is unknown.  Bypasses lock
/// validation so test fixtures can install arbitrary masks — the
/// syscall surface enforces the Linux rules (no unknown bits, no
/// clearing a set lock, no flipping a locked flag).
pub fn set_securebits(pid: ProcessId, val: u32) -> Option<u32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_securebits;
    proc.linux_securebits = val;
    Some(old)
}

/// Read the packed `ioprio_set(2)` / `ioprio_get(2)` value for
/// `pid`.  Returns `None` if `pid` is unknown; `Some(LINUX_IOPRIO_DEFAULT)`
/// is the value every fresh task starts with.
#[must_use]
pub fn get_ioprio(pid: ProcessId) -> Option<i32> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_ioprio)
}

/// Install the packed ioprio value for `pid`, returning the
/// prior value.  Returns `None` if `pid` is unknown.  Bypasses
/// the class/data validity check so test fixtures can install
/// arbitrary words; the syscall surface enforces the Linux rules
/// (class in 0..=3, data in 0..=7).
pub fn set_ioprio(pid: ProcessId, val: i32) -> Option<i32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = proc.linux_ioprio;
    proc.linux_ioprio = val;
    Some(old)
}

/// Index of `RLIMIT_AS` (resident *address space* size) in [`Process::rlimits`].
///
/// Pulled into a named constant so the [`linux_as_charge`] /
/// [`linux_as_release`] helpers and any future enforcement sites do not
/// have to repeat the magic number.
pub const RLIMIT_AS_INDEX: usize = 9;

/// Index of `RLIMIT_FSIZE` (maximum file size a process may write) in
/// [`Process::rlimits`].  Used by the Linux `pwrite` / `pwritev` /
/// `pwritev2` / `ftruncate` / `truncate` translation layers to clip
/// writes that would push a file past the per-process limit.
pub const RLIMIT_FSIZE_INDEX: usize = 1;

/// Index of `RLIMIT_STACK` (maximum stack size) in [`Process::rlimits`].
///
/// Consulted from the page-fault handler ([`crate::idt::try_grow_user_stack`])
/// via [`try_get_rlimit`] to bound on-demand stack growth.  The page
/// fault handler runs in interrupt context where the regular process
/// table lock cannot be acquired safely; the `try_lock`-based accessor
/// is the only path that should be used from that site.
pub const RLIMIT_STACK_INDEX: usize = 3;

/// Index of `RLIMIT_NICE` (nice-value ceiling) in [`Process::rlimits`].
///
/// Linux encodes the ceiling as `rlim_cur = 20 - lowest_allowed_nice`,
/// so a `rlim_cur` of 0 means "nice may never be lowered below 20",
/// effectively forbidding any priority boost.  Higher values allow
/// lower (more negative) nice values: `rlim_cur = 21` allows nice as
/// low as -1, `rlim_cur = 40` allows the full -20..=19 range.
///
/// Consulted by the Linux `setpriority` translation layer.
pub const RLIMIT_NICE_INDEX: usize = 13;

/// Index of `RLIMIT_RTPRIO` (real-time-priority ceiling) in
/// [`Process::rlimits`].
///
/// Linux encodes the ceiling directly: `rlim_cur` is the maximum
/// `sched_priority` the process may request when switching to a
/// real-time scheduling policy (SCHED_FIFO or SCHED_RR).  `rlim_cur =
/// 0` (our default) means real-time policies are forbidden entirely
/// because every valid RT priority is in `[1, 99]`.
///
/// Consulted by the Linux `sched_setscheduler` / `sched_setparam`
/// translation layers.
pub const RLIMIT_RTPRIO_INDEX: usize = 14;

/// Read the current `(rlim_cur, rlim_max)` for `pid`'s `resource` using
/// `try_lock()`, returning `None` if the process table is currently held
/// by another CPU or if `pid` is unknown.
///
/// This is the **only** safe accessor for callers that run with
/// interrupts disabled or are themselves servicing an interrupt — most
/// notably the page fault handler's stack-growth path
/// ([`crate::idt::try_grow_user_stack`]).  A regular [`get_rlimit`] call
/// from those contexts would deadlock if the interrupted code happened
/// to hold the process table.
///
/// On lock contention the caller is expected to fall back to whatever
/// behavior it had before this accessor existed (typically: allow the
/// operation without enforcing the rlimit, matching pre-enforcement
/// semantics).  The bound is best-effort and may occasionally let a
/// stack page slip past the limit during a contended fork/exec, but
/// will never wrongly *reject* a growth that would actually fit.
#[must_use]
pub fn try_get_rlimit(pid: ProcessId, resource: u32) -> Option<(u64, u64)> {
    if resource >= NUM_RLIMITS {
        return None;
    }
    let table = PROCESS_TABLE.try_lock()?;
    table.get(&pid).map(|p| p.rlimits[resource as usize])
}

/// Charge `bytes` to the process's Linux address-space accounting and
/// enforce [`RLIMIT_AS`] (resource index 9).
///
/// Called from the Linux `mmap` translation layer with the *aligned*
/// mapping size before delegating to the native mmap path.  Returns
/// [`KernelError::OutOfMemory`] (mapped to `ENOMEM` at the syscall
/// boundary) when applying the charge would exceed the soft limit;
/// [`RLIM_INFINITY`] always passes.  Native (non-Linux) mmap paths do
/// not call this function — RLIMIT_AS is a Linux-ABI concept and
/// native programs are unaffected.
///
/// On success the charge is committed; if the subsequent native mmap
/// fails the caller **must** call [`linux_as_release`] with the same
/// `bytes` value to refund the accounting.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` is unknown.
/// - [`KernelError::OutOfMemory`] if `linux_as_bytes + bytes` would
///   exceed `rlimits[9].0` (the RLIMIT_AS soft limit).
pub fn linux_as_charge(pid: ProcessId, bytes: u64) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&pid)
        .ok_or(KernelError::NoSuchProcess)?;
    let soft = proc.rlimits[RLIMIT_AS_INDEX].0;
    // saturating_add gives a deterministic large value rather than
    // wrapping; in practice the soft limit will always be the deciding
    // factor since u64::MAX is RLIM_INFINITY.
    let new_total = proc.linux_as_bytes.saturating_add(bytes);
    if soft != RLIM_INFINITY && new_total > soft {
        return Err(KernelError::OutOfMemory);
    }
    proc.linux_as_bytes = new_total;
    Ok(())
}

/// Refund `bytes` from the process's Linux address-space accounting.
///
/// Called from the Linux `munmap` translation layer after a successful
/// unmap, and from the Linux `mmap` translation layer on the failure
/// path to roll back a prior [`linux_as_charge`].  Saturating
/// subtraction — if a caller releases more than was charged (for
/// example a `munmap` whose size exceeds any prior `mmap`), the
/// counter simply clamps to zero rather than wrapping.
///
/// Silently no-op if `pid` is unknown (the process is already gone).
pub fn linux_as_release(pid: ProcessId, bytes: u64) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.linux_as_bytes = proc.linux_as_bytes.saturating_sub(bytes);
    }
}

/// Read the current Linux address-space charge for `pid`.
///
/// Returns `None` if the process is unknown.  Used by self-tests and
/// by future diagnostic syscalls (e.g. `/proc/<pid>/statm`).
#[must_use]
pub fn linux_as_used(pid: ProcessId) -> Option<u64> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.linux_as_bytes)
}

/// Record one read-family syscall against `pid`'s I/O accounting.
///
/// Bumps `io_syscr` by one (every read syscall counts, matching Linux's
/// unconditional `inc_syscr`) and `io_rchar` by `bytes` (the number of
/// bytes the syscall actually returned — pass 0 for an error or EOF, so
/// only real transfers accumulate, like Linux's `add_rchar`).  Backs
/// `/proc/<pid>/io`.  Silently no-ops if `pid` is unknown (the caller
/// was kernel context, or the process has already been reaped).
pub fn account_io_read(pid: ProcessId, bytes: u64) {
    if let Some(proc) = PROCESS_TABLE.lock().get_mut(&pid) {
        proc.io_syscr = proc.io_syscr.saturating_add(1);
        proc.io_rchar = proc.io_rchar.saturating_add(bytes);
    }
}

/// Record one write-family syscall against `pid`'s I/O accounting.
///
/// Mirror of [`account_io_read`] for the write path: bumps `io_syscw`
/// unconditionally and `io_wchar` by the bytes the syscall consumed.
pub fn account_io_write(pid: ProcessId, bytes: u64) {
    if let Some(proc) = PROCESS_TABLE.lock().get_mut(&pid) {
        proc.io_syscw = proc.io_syscw.saturating_add(1);
        proc.io_wchar = proc.io_wchar.saturating_add(bytes);
    }
}

/// Snapshot a process's I/O byte counters as
/// `(rchar, wchar, syscr, syscw)`.
///
/// Returns `None` if `pid` is unknown.  Used by procfs to render
/// `/proc/<pid>/io`.  The three storage-layer counters Linux also
/// exposes (`read_bytes`, `write_bytes`, `cancelled_write_bytes`) are
/// not tracked here — procfs reports them as 0 rather than fabricate
/// per-process block-layer attribution we do not collect.
#[must_use]
pub fn io_counters(pid: ProcessId) -> Option<(u64, u64, u64, u64)> {
    PROCESS_TABLE
        .lock()
        .get(&pid)
        .map(|p| (p.io_rchar, p.io_wchar, p.io_syscr, p.io_syscw))
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

/// Try to reap *any* zombie child of `parent_pid` (POSIX `waitpid(-1)`).
///
/// Scans the process table for children of `parent_pid`:
/// - If a zombie child is found, it is reaped (returns
///   `Ok(Some((child_pid, ExitInfo)))` and destroys the child).  When
///   several zombies exist, the lowest PID is chosen for determinism.
/// - If `parent_pid` has living (non-zombie) children but none are
///   ready, returns `Ok(None)` — the caller should block and retry.
/// - If `parent_pid` has no children at all, returns
///   `Err(NoChildProcess)` (POSIX `ECHILD`).
///
/// Mirrors [`try_reap`] but without a known child PID.  Cleanup is done
/// outside the `PROCESS_TABLE` lock (same two-phase pattern) to avoid
/// lock-ordering hazards.
pub fn try_reap_any(
    parent_pid: ProcessId,
) -> KernelResult<Option<(ProcessId, ExitInfo)>> {
    #[allow(clippy::type_complexity)]
    let reaped: Option<(
        ProcessId,
        ExitInfo,
        u64,
        Vec<(crate::cap::ResourceType, u64)>,
        Vec<(i32, u8, u64)>,
    )>;

    {
        let mut table = PROCESS_TABLE.lock();

        // First pass: does this process have any children at all, and is
        // there a zombie among them?  BTreeMap iterates in ascending key
        // order, so the first zombie found has the lowest PID.
        let mut has_child = false;
        let mut zombie_child: Option<ProcessId> = None;
        for proc in table.values() {
            if proc.parent == parent_pid && proc.pid != parent_pid {
                has_child = true;
                if proc.state == ProcessState::Zombie {
                    zombie_child = Some(proc.pid);
                    break;
                }
            }
        }

        if !has_child {
            return Err(KernelError::NoChildProcess);
        }

        let Some(child_pid) = zombie_child else {
            // Children exist but none are zombies yet — caller blocks.
            return Ok(None);
        };

        // Extract the zombie's info and remove it from the table.
        let (exit_code, crash, pml4_phys) = {
            let proc = table
                .get(&child_pid)
                .ok_or(KernelError::NoSuchProcess)?;
            (proc.exit_code.unwrap_or(0), proc.crash_info, proc.pml4_phys)
        };

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
        reaped = Some((child_pid, info, pml4_phys, ipc_handles, initial_fds));
    }
    // PROCESS_TABLE lock dropped here.

    if let Some((child_pid, info, pml4_phys, ipc_handles, initial_fds)) = reaped {
        destroy_process_resources(child_pid, pml4_phys, &ipc_handles, &initial_fds);
        Ok(Some((child_pid, info)))
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
// Per-process VMA management (address-space records for all mmap regions)
// ---------------------------------------------------------------------------

/// Add a VMA to a process's per-process VMA list.
///
/// Used by `SYS_MMAP` to register a mapped region — both committed
/// (default) and lazy/demand-paged (`MAP_LAZY`) mappings register a VMA
/// so the list reflects the full user address space (and drives
/// `/proc/<pid>/maps`).  The VMA must not overlap any existing VMA in
/// the process.
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

/// Snapshot a process's VMA list, sorted by start address.
///
/// Returns a cloned `Vec` (the live list is behind the process-table
/// lock, which we must not hand out by reference) so callers such as
/// `/proc/<pid>/maps` can render it without holding the lock.  Returns
/// `None` if the PID has no live process record; `Some(empty)` for a
/// process that has registered no VMAs yet.
#[must_use]
pub fn list_vmas(pid: ProcessId) -> Option<Vec<Vma>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|proc| proc.vmas.clone())
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

/// Register a task as the "wait for any child" waiter on `parent_pid`.
///
/// When any child of `parent_pid` becomes a zombie, the scheduler wakes
/// this task (see [`remove_thread`]).  Used by the blocking
/// `waitpid(-1)` path.  Only one any-child waiter per process.
pub fn set_wait_any_task(parent_pid: ProcessId, task_id: TaskId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table
        .get_mut(&parent_pid)
        .ok_or(KernelError::NoSuchProcess)?;

    proc.wait_any_task = Some(task_id);
    Ok(())
}

/// Clear the "wait for any child" waiter on `parent_pid`, but only if it
/// is still `task_id`.
///
/// Called by a `waitpid(-1)` caller when it stops waiting (reaped a
/// child or hit ECHILD) so a later child exit doesn't deliver a stale
/// wake to an unrelated `block_current`.  The `task_id` guard avoids
/// clobbering a different thread's registration.  No-op if the process
/// is gone or the slot holds a different/no task.
pub fn clear_wait_any_task(parent_pid: ProcessId, task_id: TaskId) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&parent_pid) {
        if proc.wait_any_task == Some(task_id) {
            proc.wait_any_task = None;
        }
    }
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
    // Drop any Linux per-signal sigaction state for this process.
    crate::syscall::linux::linux_sigaction_on_exit(pid);

    // Release any advisory file locks (flock) held by this process.
    // Locks are owner-keyed by PID; without this a crashed lock holder
    // would block every other waiter on that path until reboot.
    crate::fs::Vfs::funlock_all(pid);

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
            crate::proc::spawn::fd_handle_type::STREAM_SOCKET => {
                // Spawn dup'd the parent's stream-socket endpoint ref
                // (per-endpoint refcount); closing here drops just that
                // ref.  Unreached if userspace already claimed the handle
                // into its fd-table (initial_fds is emptied at claim).
                crate::ipc::stream_socket::close(
                    crate::ipc::stream_socket::StreamSocketHandle::from_raw(handle),
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

/// Install a new process "comm" name for `pid`, returning the previous
/// name.
///
/// Linux's `PR_SET_NAME` semantics: the comm field is 16 bytes
/// (`TASK_COMM_LEN`) including a trailing NUL, so the visible name is
/// truncated to 15 bytes.  Trailing NULs (and anything after the first
/// NUL) are not stored.  Callers should perform the byte-level NUL
/// scan and UTF-8 validation before invoking this helper; this layer
/// just persists the resulting string.
///
/// Returns `None` if `pid` is unknown.
pub fn set_name(pid: ProcessId, new: String) -> Option<String> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let old = core::mem::replace(&mut proc.name, new);
    Some(old)
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

/// Snapshot a process's tracked IPC handles for `fork()`.
///
/// Returns a clone of the `(resource_type, handle_raw)` list so the
/// caller can refcount-duplicate each one for the child without holding
/// the process-table lock across the (potentially blocking) dup calls.
///
/// Returns `None` if the process no longer exists.
pub fn ipc_handles_snapshot(pid: ProcessId) -> Option<Vec<(ResourceType, u64)>> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|proc| proc.ipc_handles.clone())
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
        // Persistent /proc snapshots: keep a copy for the process
        // lifetime (cloned before the one-shot move below) so
        // `/proc/<pid>/cmdline` and `/proc/<pid>/environ` stay readable
        // after the child drains `initial_argv`/`initial_envp` at
        // startup.  This is the only extra cost — bounded by
        // `MAX_ARGS_BYTES` and freed when the process exits.
        proc.proc_argv = argv.clone();
        proc.proc_envp = envp.clone();
        proc.initial_argv = argv;
        proc.initial_envp = envp;
        Ok(())
    } else {
        Err(KernelError::NoSuchProcess)
    }
}

/// Read a clone of the persistent argv snapshot for `/proc/<pid>/cmdline`.
///
/// Returns `None` if `pid` is unknown.  Returns an empty vec for a
/// process that was spawned without argv (the caller should fall back to
/// the process name, matching Linux's behaviour for kernel threads whose
/// `cmdline` is empty).
#[must_use]
pub fn get_proc_argv(pid: ProcessId) -> Option<Vec<Vec<u8>>> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.proc_argv.clone())
}

/// Read a clone of the persistent environ snapshot for
/// `/proc/<pid>/environ`.
///
/// Returns `None` if `pid` is unknown.  Returns an empty vec for a
/// process spawned without an environment.
#[must_use]
pub fn get_proc_envp(pid: ProcessId) -> Option<Vec<Vec<u8>>> {
    PROCESS_TABLE.lock().get(&pid).map(|p| p.proc_envp.clone())
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

/// Get the syscall ABI mode for a process.
///
/// Returns `None` if the process does not exist (already reaped,
/// never created, or PID 0 for kernel tasks).  A returned
/// [`AbiMode::Linux`] means the syscall dispatcher must route the
/// process's `syscall` instructions through
/// [`crate::syscall::linux::dispatch_linux`].
pub fn get_abi_mode(pid: ProcessId) -> Option<AbiMode> {
    let table = PROCESS_TABLE.lock();
    table.get(&pid).map(|p| p.abi_mode)
}

/// Set the syscall ABI mode for a process.
///
/// Called by the ELF loader when it detects a Linux binary (so that
/// the first userspace `syscall` is already routed correctly), and by
/// tests/tooling that want to flip ABI mode explicitly.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
pub fn set_abi_mode(pid: ProcessId, mode: AbiMode) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    proc.abi_mode = mode;
    Ok(())
}

// ---------------------------------------------------------------------------
// Linux fd table accessors
// ---------------------------------------------------------------------------
//
// All operations take the global `PROCESS_TABLE` lock for the duration
// of the call.  Callers should NOT hold any other PCB-related lock
// when invoking these — they are designed to be called from the Linux
// syscall translators in `kernel::syscall::linux`, which run in the
// SYSCALL handler with no other locks held.

/// Install an empty Linux fd table (with stdio pre-installed) on
/// `pid`, replacing any prior table.
///
/// Idempotent in the sense that calling it twice on the same Linux-ABI
/// process simply re-initialises the table — but typically it is
/// called exactly once, immediately after [`set_abi_mode`] flips the
/// process to Linux ABI in `spawn_process` / `exec_process`.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
pub fn linux_fd_install_stdio(pid: ProcessId) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    proc.linux_fd_table = Some(alloc::boxed::Box::new(
        super::linux_fd::KernelFdTable::with_stdio(),
    ));
    Ok(())
}

/// Record the auxiliary-vector byte stream that the Linux SysV
/// initial-stack builder wrote onto `pid`'s stack, so it can later be
/// served from `prctl(PR_GET_AUXV)` and `/proc/<pid>/auxv` without
/// re-reading user memory.
///
/// `auxv` is the verbatim little-endian `Elf64_auxv_t` stream produced
/// by [`crate::proc::linux_stack::install_linux_stack`]: pairs of
/// `(a_type, a_val)` `u64`s terminated by an `AT_NULL` (0, 0) entry.
/// Replaces any prior saved auxv (e.g. across `execve`).
///
/// Only meaningful for Linux-ABI processes; native processes must never
/// call this (they have no auxv by design — design-decision #4).
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a live
///   process.
pub fn set_linux_saved_auxv(
    pid: ProcessId,
    auxv: alloc::vec::Vec<u8>,
) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    proc.linux_saved_auxv = Some(auxv);
    Ok(())
}

/// Return a copy of `pid`'s saved Linux auxiliary vector, or `None` if
/// the process has none (native processes, or a Linux-ABI process whose
/// stack has not yet been built).
///
/// The returned bytes are the raw `Elf64_auxv_t` stream — see
/// [`set_linux_saved_auxv`].  Callers (`PR_GET_AUXV`, `/proc/<pid>/auxv`)
/// copy it out under the process-table lock so the snapshot is
/// consistent.
#[must_use]
pub fn linux_saved_auxv(pid: ProcessId) -> Option<alloc::vec::Vec<u8>> {
    let table = PROCESS_TABLE.lock();
    let proc = table.get(&pid)?;
    proc.linux_saved_auxv.clone()
}

/// Drop `pid`'s saved Linux auxiliary vector, if any.
///
/// Called on `execve` into a *native* image: a native process has no
/// auxv by design, so any auxv left over from a previous Linux-ABI image
/// must not linger.  A no-op if `pid` has no saved auxv or does not
/// refer to a live process.
pub fn clear_linux_saved_auxv(pid: ProcessId) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get_mut(&pid) {
        proc.linux_saved_auxv = None;
    }
}

/// Walk `pid`'s Linux fd table, remove every entry whose `FD_CLOEXEC`
/// flag is set, ensure stdin/stdout/stderr remain populated, and
/// return the deduplicated list of underlying kernel handles that the
/// caller should release with the appropriate native `close`.
///
/// Implements the kernel half of POSIX `close-on-exec` for Linux-ABI
/// processes.  Called by `exec_process` when re-using an existing
/// Linux fd table across an `execve()`.
///
/// The returned list:
/// - Excludes `HandleKind::Console` entries (no kernel resource).
/// - Excludes any `(kind, raw_handle)` still referenced by a
///   non-cloexec fd left in the table (so the open file description
///   survives, matching POSIX).
/// - Is deduplicated by `(kind, raw_handle)` so that two cloexec fds
///   pointing at the same handle yield exactly one close.
///
/// Returns an empty vector if `pid` has no Linux fd table (e.g. it
/// was previously a Native-ABI process); the caller can then install
/// a fresh stdio-only table via [`linux_fd_install_stdio`].  Returns
/// `None` only if `pid` does not refer to a live process at all.
#[must_use]
pub fn linux_fd_exec_cloexec(
    pid: ProcessId,
) -> Option<alloc::vec::Vec<super::linux_fd::FdEntry>> {
    use super::linux_fd::FdEntry;

    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let fd_table = proc.linux_fd_table.as_mut()?;

    let taken = fd_table.take_cloexec_entries();
    fd_table.ensure_stdio();

    // Build the to-close list: kernel-resource-bearing entries, not
    // referenced by any remaining fd, deduplicated by (kind, raw).
    let mut to_close: alloc::vec::Vec<FdEntry> = alloc::vec::Vec::new();
    for entry in taken {
        if !entry.kind.needs_kernel_close() {
            continue;
        }
        let already_listed = to_close
            .iter()
            .any(|e| e.kind == entry.kind && e.raw_handle == entry.raw_handle);
        if already_listed {
            continue;
        }
        // `excluded_fd` is irrelevant here — the cloexec entries are
        // already gone from the table, so we just scan what remains.
        // Use -1 (never a valid fd) to mean "exclude nothing extra".
        let still_referenced = fd_table.is_handle_referenced(
            entry.kind,
            entry.raw_handle,
            -1,
        );
        if !still_referenced {
            to_close.push(entry);
        }
    }

    Some(to_close)
}

/// Look up `fd` in the Linux fd table.  Returns `None` if the process
/// does not have a Linux fd table or if `fd` is unused/out-of-range.
#[must_use]
pub fn linux_fd_lookup(
    pid: ProcessId,
    fd: i32,
) -> Option<super::linux_fd::FdEntry> {
    let table = PROCESS_TABLE.lock();
    let proc = table.get(&pid)?;
    let fd_table = proc.linux_fd_table.as_ref()?;
    fd_table.lookup(fd)
}

/// Install `entry` at the lowest free fd >= `min_fd`.
///
/// Enforces `RLIMIT_NOFILE`: if the lowest free fd would be `>=` the
/// process's current soft limit (`rlim_cur` for resource index 7,
/// `RLIMIT_NOFILE`), returns `TooManyOpenFiles` rather than installing.
/// This is the central choke point — every Linux-ABI open / pipe /
/// dup / accept install path goes through here, so enforcing here
/// catches them all uniformly.
///
/// `RLIM_INFINITY` (`u64::MAX`) disables the check, which matches
/// Linux's behaviour for processes that have explicitly opted out.
/// The `MAX_FDS` cap on the underlying table still applies after the
/// rlimit check.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if the process has no Linux fd
///   table (i.e. it is a Native-ABI process).
/// - [`KernelError::TooManyOpenFiles`] if the table is full or the
///   installation would exceed `RLIMIT_NOFILE`.
pub fn linux_fd_install(
    pid: ProcessId,
    entry: super::linux_fd::FdEntry,
    min_fd: i32,
) -> KernelResult<i32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    // Snapshot RLIMIT_NOFILE soft limit before borrowing the fd table
    // mutably (fd table lives inside `proc`).
    let nofile_soft = proc.rlimits[7].0;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    let fd = fd_table.install_lowest_from(min_fd, entry)?;
    // Enforce RLIMIT_NOFILE.  install_lowest_from returns the chosen
    // fd; if it lands at or above the soft limit, roll the install
    // back and surface EMFILE.  Skip the check entirely for
    // RLIM_INFINITY (the documented "no per-process limit" sentinel).
    if nofile_soft != RLIM_INFINITY && (fd as u64) >= nofile_soft {
        // Roll the install back so the caller doesn't see a leaked
        // entry.  We allocated it; we own the rollback.
        let _ = fd_table.take(fd);
        return Err(KernelError::TooManyOpenFiles);
    }
    Ok(fd)
}

/// Remove the entry at `fd` and return it, so the caller can decide
/// whether to call the appropriate kernel close on the underlying
/// handle.  Returns `None` if the process has no Linux fd table or
/// `fd` was already closed.
#[must_use]
pub fn linux_fd_take(
    pid: ProcessId,
    fd: i32,
) -> Option<super::linux_fd::FdEntry> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid)?;
    let fd_table = proc.linux_fd_table.as_mut()?;
    fd_table.take(fd)
}

/// Check whether any fd OTHER than `excluded_fd` references the same
/// `(kind, raw_handle)`.  Used by `close()` to decide whether to
/// release the underlying kernel resource.
#[must_use]
pub fn linux_fd_is_handle_referenced(
    pid: ProcessId,
    kind: super::linux_fd::HandleKind,
    raw_handle: u64,
    excluded_fd: i32,
) -> bool {
    let table = PROCESS_TABLE.lock();
    let Some(proc) = table.get(&pid) else { return false };
    let Some(fd_table) = proc.linux_fd_table.as_ref() else { return false };
    fd_table.is_handle_referenced(kind, raw_handle, excluded_fd)
}

/// Duplicate `oldfd` onto the lowest free slot >= `min_fd`.
///
/// Implements both `dup` (min_fd=0) and `fcntl(F_DUPFD, min_fd)`.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if the process has no Linux fd
///   table or `oldfd` is not open.
/// - [`KernelError::TooManyOpenFiles`] if the table is full.
pub fn linux_fd_dup(
    pid: ProcessId,
    oldfd: i32,
    min_fd: i32,
) -> KernelResult<i32> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.dup_lowest(oldfd, min_fd)
}

/// Duplicate `oldfd` onto exactly `newfd`, returning `(newfd,
/// previous_occupant)`.  The caller is responsible for closing the
/// previous occupant (if `Some`) after dropping the lock.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if `oldfd` is not open or the
///   process has no Linux fd table.
/// - [`KernelError::TooManyOpenFiles`] if `newfd` is out of range.
pub fn linux_fd_dup2(
    pid: ProcessId,
    oldfd: i32,
    newfd: i32,
) -> KernelResult<(i32, Option<super::linux_fd::FdEntry>)> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.dup2(oldfd, newfd)
}

/// Set `FD_CLOEXEC` (and any other future fd flags) for `fd`.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if `fd` is not open or the
///   process has no Linux fd table.
pub fn linux_fd_set_fd_flags(
    pid: ProcessId,
    fd: i32,
    fd_flags: u32,
) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.set_fd_flags(fd, fd_flags)
}

/// Set status flags (`O_APPEND` / `O_NONBLOCK` / ...) for `fd`,
/// preserving the access-mode bits.
///
/// # Errors
///
/// As [`linux_fd_set_fd_flags`].
pub fn linux_fd_set_status_flags(
    pid: ProcessId,
    fd: i32,
    new_flags: u32,
) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.set_status_flags(fd, new_flags)
}

/// Read the `fcntl(F_GETOWN)` value (SIGIO delivery target — pid if
/// positive, pgid if negative, 0 if cleared) for `fd` in `pid`'s
/// Linux fd table.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if `fd` is not open or the
///   process has no Linux fd table.
pub fn linux_fd_get_owner(pid: ProcessId, fd: i32) -> KernelResult<i32> {
    let table = PROCESS_TABLE.lock();
    let proc = table.get(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_ref()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.get_owner(fd)
}

/// Snapshot the open Linux fds of `pid` as ascending `(fd, entry)` pairs.
///
/// Returns `None` if the process does not exist or has no Linux fd table
/// (i.e. it is a native-ABI process whose fd table lives in userspace and
/// is therefore not kernel-visible).  Backs `/proc/<pid>/fd/`.
#[must_use]
pub fn linux_fd_list(
    pid: ProcessId,
) -> Option<alloc::vec::Vec<(i32, super::linux_fd::FdEntry)>> {
    let table = PROCESS_TABLE.lock();
    let proc = table.get(&pid)?;
    let fd_table = proc.linux_fd_table.as_ref()?;
    Some(fd_table.list_open())
}

/// Set the `fcntl(F_SETOWN)` value for `fd`.
///
/// # Errors
///
/// As [`linux_fd_get_owner`].
pub fn linux_fd_set_owner(pid: ProcessId, fd: i32, owner: i32) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.set_owner(fd, owner)
}

/// Read the `fcntl(F_GETSIG)` value for `fd` (0 means "use the
/// default SIGIO").
///
/// # Errors
///
/// As [`linux_fd_get_owner`].
pub fn linux_fd_get_sig(pid: ProcessId, fd: i32) -> KernelResult<i32> {
    let table = PROCESS_TABLE.lock();
    let proc = table.get(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_ref()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.get_owner_sig(fd)
}

/// Set the `fcntl(F_SETSIG)` value for `fd`.
///
/// Linux validates `sig == 0 || (1..=64).contains(&sig)`; the
/// helper returns [`KernelError::InvalidArgument`] otherwise.
///
/// # Errors
///
/// - [`KernelError::NoSuchProcess`] if `pid` does not refer to a
///   live process.
/// - [`KernelError::InvalidHandle`] if `fd` is not open or the
///   process has no Linux fd table.
/// - [`KernelError::InvalidArgument`] if `sig` is outside the
///   permitted range.
pub fn linux_fd_set_sig(pid: ProcessId, fd: i32, sig: i32) -> KernelResult<()> {
    let mut table = PROCESS_TABLE.lock();
    let proc = table.get_mut(&pid).ok_or(KernelError::NoSuchProcess)?;
    let fd_table = proc
        .linux_fd_table
        .as_mut()
        .ok_or(KernelError::InvalidHandle)?;
    fd_table.set_owner_sig(fd, sig)
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
    test_reap_any()?;
    test_io_accounting()?;

    Ok(())
}

/// Test: per-process I/O byte accounting (`/proc/<pid>/io` backing).
///
/// Exercises the real [`account_io_read`] / [`account_io_write`] /
/// [`io_counters`] logic against a throwaway process — the part that
/// matters (the increment semantics), independent of the userspace
/// syscall harness that the boot self-test cannot drive.  Verifies:
///   - a fresh process starts at all-zero counters,
///   - `syscr`/`syscw` bump once per call (even a zero-byte call),
///   - `rchar`/`wchar` accumulate only the supplied byte counts,
///   - reads and writes are tracked independently,
///   - an unknown pid is a silent no-op (kernel-context / reaped case).
fn test_io_accounting() -> KernelResult<()> {
    let pid = create("io-acct-test", 0);

    // Fresh process: every counter starts at zero.
    if io_counters(pid) != Some((0, 0, 0, 0)) {
        serial_println!("[proc]   FAIL: fresh io_counters not all-zero");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Two reads (4096 + 0 bytes) and one write (1024 bytes).  The
    // zero-byte read models an EOF/short read: it must still bump
    // syscr but contribute nothing to rchar.
    account_io_read(pid, 4096);
    account_io_read(pid, 0);
    account_io_write(pid, 1024);

    // Expect rchar=4096, wchar=1024, syscr=2, syscw=1.
    if io_counters(pid) != Some((4096, 1024, 2, 1)) {
        serial_println!(
            "[proc]   FAIL: io_counters {:?} != Some((4096, 1024, 2, 1))",
            io_counters(pid)
        );
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    destroy(pid);

    // After reap the pid is unknown: counters read None and accounting
    // is a silent no-op (no panic, no resurrection of the entry).
    account_io_read(pid, 999);
    account_io_write(pid, 999);
    if io_counters(pid).is_some() {
        serial_println!("[proc]   FAIL: io_counters live after destroy");
        return Err(KernelError::InternalError);
    }

    serial_println!("[proc]   I/O accounting: OK");
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
    let (zombie, _wake, _any) = remove_thread(pid, 100)?;
    if zombie {
        serial_println!("[proc]   FAIL: should not be zombie with 1 thread left");
        destroy(pid);
        return Err(KernelError::InternalError);
    }

    // Remove last — process becomes zombie.
    let (zombie, _wake, _any) = remove_thread(pid, 200)?;
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
    let (zombie, _wake, _any) = remove_thread(child_pid, 900)?;
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
    let (zombie, _, _) = remove_thread(crash_child, 950)?;
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

/// Test 6: `try_reap_any` — POSIX `waitpid(-1)` semantics.
///
/// Covers: no children → `NoChildProcess` (ECHILD); living children but
/// no zombie → `None`; a zombie child is reaped and reported by PID;
/// once all children are reaped → `NoChildProcess` again.
fn test_reap_any() -> KernelResult<()> {
    let parent_pid = create("reapany-parent", 0);

    // No children yet → ECHILD.
    match try_reap_any(parent_pid) {
        Err(KernelError::NoChildProcess) => {} // Expected.
        other => {
            serial_println!(
                "[proc]   FAIL: reap_any with no children should be NoChildProcess, got {:?}",
                other.map(|o| o.map(|(p, _)| p))
            );
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // Two running children.
    let child_a = create("reapany-a", parent_pid);
    let child_b = create("reapany-b", parent_pid);
    set_running(child_a)?;
    set_running(child_b)?;
    add_thread(child_a, 960)?;
    add_thread(child_b, 961)?;

    // Children exist but none are zombies → None (would block).
    match try_reap_any(parent_pid)? {
        None => {} // Expected.
        Some((p, _)) => {
            serial_println!("[proc]   FAIL: reap_any should block (None), reaped {}", p);
            destroy(child_a);
            destroy(child_b);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // Make child_b a zombie with a distinctive exit code.
    set_exit_code(child_b, 7)?;
    let (zombie, _wake, _any) = remove_thread(child_b, 961)?;
    if !zombie {
        serial_println!("[proc]   FAIL: child_b should be zombie");
        destroy(child_a);
        destroy(child_b);
        destroy(parent_pid);
        return Err(KernelError::InternalError);
    }

    // reap_any should reap child_b (the only zombie) and report its PID.
    match try_reap_any(parent_pid)? {
        Some((reaped, info)) if reaped == child_b && info.exit_code == 7 => {}
        other => {
            serial_println!(
                "[proc]   FAIL: reap_any should reap child_b(={}) code=7, got {:?}",
                child_b,
                other.map(|(p, i)| (p, i.exit_code))
            );
            destroy(child_a);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // child_a still running → None again.
    match try_reap_any(parent_pid)? {
        None => {} // Expected.
        Some((p, _)) => {
            serial_println!("[proc]   FAIL: reap_any should still block (child_a alive), reaped {}", p);
            destroy(child_a);
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // Reap child_a too.
    set_exit_code(child_a, 0)?;
    let (zombie, _wake, _any) = remove_thread(child_a, 960)?;
    if !zombie {
        serial_println!("[proc]   FAIL: child_a should be zombie");
        destroy(child_a);
        destroy(parent_pid);
        return Err(KernelError::InternalError);
    }
    match try_reap_any(parent_pid)? {
        Some((reaped, _)) if reaped == child_a => {}
        other => {
            serial_println!("[proc]   FAIL: reap_any should reap child_a, got {:?}",
                other.map(|(p, _)| p));
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    // All children reaped → ECHILD once more.
    match try_reap_any(parent_pid) {
        Err(KernelError::NoChildProcess) => {} // Expected.
        other => {
            serial_println!("[proc]   FAIL: reap_any after all reaped should be NoChildProcess, got {:?}",
                other.map(|o| o.map(|(p, _)| p)));
            destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    }

    destroy(parent_pid);
    serial_println!("[proc]   Reap any (waitpid -1): OK");
    Ok(())
}
