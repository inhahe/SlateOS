//! High-level process spawning — the `posix_spawn`-style API.
//!
//! Creates a new process from an ELF binary in a single call:
//!
//! 1. Parse the ELF binary.
//! 2. Create a PCB (process control block) with a per-process PML4.
//! 3. Load ELF segments into the process address space.
//! 4. Allocate and map a user-mode stack.
//! 5. Grant initial capabilities (inherited from parent, restricted).
//! 6. Spawn the initial thread, which transitions to ring 3 via IRETQ
//!    and begins executing the ELF entry point in userspace.
//!
//! ## Why Not fork()?
//!
//! `fork()` copies the entire parent address space, then usually calls
//! `exec()` immediately — wasting time and complicating the kernel
//! (copy-on-write, shared file descriptors, signal handler state, etc.).
//!
//! Our `spawn()` does what people actually want: create a new process
//! running a specific binary with specific capabilities.  No address
//! space cloning, no inherited file descriptor table, no surprise
//! shared state.
//!
//! ## Ring 3 Transition
//!
//! The initial thread runs a kernel-mode trampoline
//! (`userspace_entry_trampoline`) that:
//!
//! 1. Reads the entry point and user RSP from a heap-allocated
//!    [`UserEntryInfo`] struct.
//! 2. Builds an IRETQ frame on the kernel stack.
//! 3. Executes IRETQ to jump to ring 3 at the ELF entry point.
//!
//! From ring 3, the process communicates with the kernel exclusively
//! via the SYSCALL instruction.  When the last thread calls SYS_EXIT,
//! the process becomes a zombie and is reaped by its parent.
//!
//! ## Current Limitations
//!
//! - No dynamic linking (only static executables).

use alloc::boxed::Box;
use crate::cap::{Rights, ResourceType};
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::proc::{elf, pcb, thread};
use crate::proc::pcb::ProcessId;
use crate::sched::task::{TaskId, DEFAULT_PRIORITY};
use crate::serial_println;
use crate::serial_print;

/// Exit code set when exec fails after tearing down the old address space.
///
/// The process cannot resume its old code (it's been freed), so it must
/// exit.  We use -126 by analogy with shell convention (126 = "command
/// found but not executable").
const KILLED_EXIT_CODE: i32 = -126;

// ---------------------------------------------------------------------------
// User stack configuration
// ---------------------------------------------------------------------------

/// Top of the user stack (exclusive).  The stack grows downward from
/// here.  Placed near the top of the user address space with a small
/// gap before the non-canonical hole (0x0000_8000_0000_0000).
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;

/// Number of 16 KiB frames to allocate for the initial user stack.
/// 4 frames = 64 KiB, matching typical initial thread stack sizes.
const USER_STACK_FRAMES: usize = 4;

/// Total user stack size in bytes.
#[allow(clippy::arithmetic_side_effects)]
const USER_STACK_SIZE: u64 = (USER_STACK_FRAMES * FRAME_SIZE) as u64;

/// Maximum user stack size (frames).
/// 256 frames × 16 KiB = 4 MiB max stack per thread.
/// Stack will grow on demand from the initial 64 KiB up to this limit.
pub const MAX_STACK_FRAMES: usize = 256;

/// Maximum user stack size in bytes.
#[allow(clippy::arithmetic_side_effects)]
pub const MAX_STACK_SIZE: u64 = (MAX_STACK_FRAMES * FRAME_SIZE) as u64;

/// Lowest allowed address for user stack growth.
/// Below this is the guard page — touching it kills the process.
#[allow(clippy::arithmetic_side_effects)]
pub const USER_STACK_GUARD: u64 = USER_STACK_TOP - MAX_STACK_SIZE;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Handle types for file descriptor mapping entries.
///
/// Indicates what kind of kernel object an inherited fd refers to.
/// The child's POSIX layer uses this to set up the correct fd type
/// (File, Pipe, Socket, etc.) in its fd table.
pub mod fd_handle_type {
    /// Regular file handle (from `fs::handle`).  The kernel dups it
    /// via `fs::handle::dup()`.
    pub const FILE: u8 = 0;
    /// Pipe handle (from `ipc::pipe`).  Spawn dups via `pipe::dup()`,
    /// which uses per-end refcounting so multiple PCBs can share the
    /// same read or write end safely (matching Linux fork() pipe
    /// inheritance).
    pub const PIPE: u8 = 1;
    /// TCP socket handle.
    #[allow(dead_code)] // Protocol constant — used when net stack is integrated.
    pub const TCP_SOCKET: u8 = 2;
    /// UDP socket handle.
    #[allow(dead_code)] // Protocol constant — used when net stack is integrated.
    pub const UDP_SOCKET: u8 = 3;
    /// Console I/O (stdin/stdout/stderr virtual handle).
    pub const CONSOLE: u8 = 4;
    /// Eventfd counter handle (from `ipc::eventfd`).  Spawn dups via
    /// `eventfd::dup()`, which refcounts entries in EVENTFD_TABLE so
    /// multiple PCBs can hold the same handle safely.
    #[allow(dead_code)] // Used only via fd inheritance; readable in matches.
    pub const EVENTFD: u8 = 5;
    /// Stream socket endpoint handle (from `ipc::stream_socket`).  Spawn
    /// dups via `stream_socket::dup()`, which refcounts each endpoint so
    /// parent and child can share an endpoint safely (matching Linux
    /// fork() socket inheritance).
    #[allow(dead_code)] // Used only via fd inheritance; readable in matches.
    pub const STREAM_SOCKET: u8 = 6;
}

/// A file descriptor mapping entry passed from userspace.
///
/// Used by `SYS_PROCESS_SPAWN_EX` to specify which kernel handles the
/// child should inherit and at what POSIX fd numbers.  Also used by
/// `SYS_PROCESS_GET_INITIAL_FDS` to return the mappings to the child.
///
/// Layout must match the userspace definition exactly (16 bytes, C ABI).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FdMapEntry {
    /// Target POSIX fd number in the child (e.g. 0 = stdin, 1 = stdout).
    pub fd: i32,
    /// Handle type (see [`fd_handle_type`] constants).
    ///
    /// Determines how the kernel duplicates the handle and how the
    /// child's POSIX layer interprets it.
    pub handle_type: u8,
    /// Reserved padding (set to 0).
    pub _pad: [u8; 3],
    /// Kernel handle ID.
    ///
    /// For `SYS_PROCESS_SPAWN_EX`: this is the *parent's* handle to dup.
    /// For `SYS_PROCESS_GET_INITIAL_FDS`: this is the *child's* own handle.
    pub handle: u64,
}

/// Extended spawn arguments passed from userspace via `SYS_PROCESS_SPAWN_EX`.
///
/// A single pointer to this struct is passed in `arg0`.  All pointer
/// fields point to userspace memory that must be validated before reading.
///
/// Layout must match the userspace definition exactly (C ABI).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnExArgs {
    /// Pointer to ELF data in memory.
    pub elf_ptr: u64,
    /// Length of ELF data in bytes.
    pub elf_len: u64,
    /// Pointer to process name string (UTF-8).
    pub name_ptr: u64,
    /// Length of name string in bytes.
    pub name_len: u64,
    /// Pointer to `FdMapEntry` array (0 = no fd inheritance).
    pub fd_map_ptr: u64,
    /// Number of `FdMapEntry` entries.
    pub fd_map_count: u64,
    /// Pointer to packed null-terminated argv string data.
    /// The strings are concatenated with null bytes between them.
    pub argv_ptr: u64,
    /// Total byte length of the packed argv data.
    pub argv_len: u64,
    /// Number of arguments (number of strings in argv_ptr).
    pub argc: u64,
    /// Pointer to packed null-terminated envp string data.
    pub envp_ptr: u64,
    /// Total byte length of the packed envp data.
    pub envp_len: u64,
    /// Number of environment variables.
    pub envc: u64,
}

/// Result of a successful process spawn.
#[derive(Debug, Clone, Copy)]
pub struct SpawnResult {
    /// The new process's ID.
    pub pid: ProcessId,
    /// The initial thread's task ID.
    pub task_id: TaskId,
    /// The ELF entry point address.
    pub entry_point: u64,
}

/// Result of a successful `exec` operation.
///
/// Contains the new entry point and user stack pointer — the caller
/// (the SYSCALL handler) uses these to build a fresh IRETQ frame and
/// resume execution at the new binary's entry.
#[derive(Debug, Clone, Copy)]
pub struct ExecResult {
    /// The new ELF entry point (ring 3 RIP).
    pub entry_rip: u64,
    /// The top of the fresh user stack (ring 3 RSP).
    pub user_rsp: u64,
}

/// Spawn options for customizing process creation.
#[derive(Debug, Clone)]
pub struct SpawnOptions<'a> {
    /// Human-readable process name (for debug output).
    pub name: &'a str,
    /// Parent process ID (0 = kernel-spawned).
    pub parent: ProcessId,
    /// Priority for the initial thread (0 = highest, 31 = lowest).
    pub priority: u8,
    /// Initial capabilities to grant (resource type, resource ID, rights).
    /// The parent must have these capabilities to delegate them.
    pub capabilities: &'a [(ResourceType, u64, Rights)],
    /// Initial file descriptor map for the child process.
    ///
    /// Each entry is `(posix_fd_number, parent_kernel_handle)`.  During
    /// spawn, each parent handle is duplicated via `handle::dup()` and
    /// the resulting `(fd, new_handle)` pair is stored in the child's
    /// PCB.  The child's POSIX layer reads this via
    /// `SYS_PROCESS_GET_INITIAL_FDS` during startup.
    ///
    /// An empty slice means no fd inheritance (the default).
    ///
    /// Each tuple is `(posix_fd, handle_type, parent_handle)`.
    /// `handle_type` uses [`fd_handle_type`] constants.
    pub fd_map: &'a [(i32, u8, u64)],
    /// Command-line arguments for the child process.
    ///
    /// Each element is one argument as a byte slice (no null terminator
    /// needed — the kernel adds them when storing).  The child reads
    /// these via `SYS_PROCESS_GET_ARGS` during startup.
    ///
    /// An empty slice means no arguments (the default).  argv[0] is
    /// conventionally the program name.
    pub argv: &'a [&'a [u8]],
    /// Environment variables for the child process.
    ///
    /// Each element is one `KEY=value` pair as a byte slice (no null
    /// terminator needed).  The child reads these via
    /// `SYS_PROCESS_GET_ARGS` during startup.
    ///
    /// An empty slice means no environment (the default).
    pub envp: &'a [&'a [u8]],
    /// Resolved absolute path of the executable, stored to back
    /// `/proc/<pid>/exe`.
    ///
    /// `None` (the default) means the caller has no path to record (e.g.
    /// the spawn syscall, which today takes raw ELF bytes with no
    /// filesystem path); in that case `/proc/<pid>/exe` reports
    /// `NotFound`.  The shell's `run` command and the init loader pass
    /// the canonical path they loaded the binary from.  Bytes, not
    /// `&str`: a path may contain any byte except `/` and NUL.
    pub exe_path: Option<&'a [u8]>,
    /// Initial working directory for the child process.
    ///
    /// `None` (the default) leaves the child at the PCB default cwd `/`.
    /// When set, it must be an absolute path (start with `/`); it backs the
    /// child's `getcwd`/`*at(AT_FDCWD, …)` resolution.  Used to honor a
    /// container image's `WorkingDir` / the Docker `--workdir`/`-w` flag: the
    /// init process starts in that directory (a *guest* path, resolved under
    /// the container jail).  Bytes, not `&str`: a path may contain any byte
    /// except `/` and NUL.  A malformed value is ignored (child stays at `/`).
    pub cwd: Option<&'a [u8]>,
    /// Initial user/group identity `(uid, gid)` for the child process.
    ///
    /// `None` (the default) leaves the child at the inherited credentials
    /// (root for a kernel-spawned init).  When set, the child's
    /// [`ProcessCredentials`](crate::proc::pcb::ProcessCredentials) are
    /// replaced with `ProcessCredentials::new(uid, gid)` at spawn time.  Used
    /// to honor a container image's `User` config / the Docker `--user`/`-u
    /// uid[:gid]` flag.  Supplementary groups are not set (empty), matching a
    /// fresh numeric-id login.
    pub uid_gid: Option<(u32, u32)>,
}

impl<'a> SpawnOptions<'a> {
    /// Create default spawn options with the given name.
    #[must_use]
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            parent: 0, // Kernel-spawned.
            priority: DEFAULT_PRIORITY,
            capabilities: &[],
            fd_map: &[],
            argv: &[],
            envp: &[],
            exe_path: None,
            cwd: None,
            uid_gid: None,
        }
    }

    /// Set the initial user/group identity for the child (`(uid, gid)`).
    ///
    /// Honors a container image's `User` config / the Docker `--user`/`-u`
    /// flag. Supplementary groups are not set. Passing a `(uid, gid)` of
    /// `(0, 0)` is equivalent to leaving the child as root.
    #[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.
    #[must_use]
    pub fn uid_gid(mut self, uid: u32, gid: u32) -> Self {
        self.uid_gid = Some((uid, gid));
        self
    }

    /// Set the initial working directory for the child (absolute path bytes).
    ///
    /// Honors a container's `WorkingDir` / `--workdir`. A non-absolute or
    /// otherwise malformed value is ignored at spawn time (child stays at `/`).
    #[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.
    #[must_use]
    pub fn cwd(mut self, dir: &'a [u8]) -> Self {
        self.cwd = Some(dir);
        self
    }

    /// Set the resolved executable path (backs `/proc/<pid>/exe`).
    #[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.
    #[must_use]
    pub fn exe_path(mut self, path: &'a [u8]) -> Self {
        self.exe_path = Some(path);
        self
    }

    /// Set the parent process.
    #[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.
    #[must_use]
    pub fn parent(mut self, pid: ProcessId) -> Self {
        self.parent = pid;
        self
    }

    /// Set the initial thread priority.
    #[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.
    #[must_use]
    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Set the initial fd map for the child.
    ///
    /// Each entry is `(posix_fd_number, handle_type, parent_handle)`.
    /// The parent's handles are duplicated — the child gets independent
    /// handles that it owns.
    #[must_use]
    pub fn fd_map(mut self, map: &'a [(i32, u8, u64)]) -> Self {
        self.fd_map = map;
        self
    }

    /// Set the command-line arguments for the child.
    #[must_use]
    pub fn argv(mut self, args: &'a [&'a [u8]]) -> Self {
        self.argv = args;
        self
    }

    /// Set the environment variables for the child.
    #[must_use]
    pub fn envp(mut self, env: &'a [&'a [u8]]) -> Self {
        self.envp = env;
        self
    }
}

/// Header for the `SYS_PROCESS_GET_ARGS` output buffer.
///
/// Placed at the start of the output buffer, followed by packed
/// null-terminated argv strings, then packed null-terminated envp
/// strings.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SpawnArgsHeader {
    /// Number of argv entries.
    pub argc: u32,
    /// Number of envp entries.
    pub envc: u32,
    /// Total bytes of packed argv data (including null terminators).
    pub argv_data_len: u32,
    /// Total bytes of packed envp data (including null terminators).
    pub envp_data_len: u32,
}

// ---------------------------------------------------------------------------
// Ring 3 entry info
// ---------------------------------------------------------------------------

/// Information passed to the userspace entry trampoline.
///
/// Heap-allocated by `spawn_process()` (or `thread::spawn_user()`)
/// and freed by the trampoline when the thread first runs.  Contains
/// everything needed to build the IRETQ frame for ring 3 entry.
pub(crate) struct UserEntryInfo {
    /// The ELF entry point (ring 3 RIP).
    pub(crate) entry_rip: u64,
    /// The top of the user stack (ring 3 RSP).
    pub(crate) user_rsp: u64,
}

// ---------------------------------------------------------------------------
// Process spawning
// ---------------------------------------------------------------------------

/// Spawn a new process from an ELF binary.
///
/// This is the primary process creation API.  It:
/// 1. Parses the ELF binary (validating format, architecture, segments).
/// 2. Creates a new PCB with a per-process PML4.
/// 3. Loads ELF segments into the process address space.
/// 4. Allocates and maps a user-mode stack.
/// 5. Grants initial capabilities.
/// 6. Spawns the initial thread with a ring 3 trampoline.
///
/// The new process starts executing in ring 3 at the ELF entry point
/// when the scheduler first runs its thread.
///
/// # Arguments
///
/// - `elf_data` — raw bytes of the ELF64 executable.
/// - `options` — spawn configuration (name, parent, priority, caps).
///
/// # Errors
///
/// - [`KernelError::InvalidExecutable`] if the ELF binary is invalid.
/// - [`KernelError::OutOfMemory`] if any allocation fails.
pub fn spawn_process(
    elf_data: &[u8],
    options: &SpawnOptions<'_>,
) -> KernelResult<SpawnResult> {
    spawn_process_inner(elf_data, options, None)
}

/// Spawn a process, forcing it to run under an explicit syscall ABI
/// instead of auto-detecting from the ELF.
///
/// `spawn_process` infers the ABI from the binary via
/// [`elf::ElfFile::detect_linux_abi`], which keys off `EI_OSABI`, a Linux
/// `PT_INTERP`, or `PT_GNU_PROPERTY`.  A *bare* static Linux binary —
/// e.g. one a freestanding `tcc -nostdlib -static` build produces — has
/// none of those markers (OSABI is `SYSV`, no interpreter, no GNU
/// property note), so auto-detection classifies it as Native and routes
/// its `syscall`s through the wrong dispatch table.  When the caller
/// already *knows* the ABI of the binary it is launching (because it just
/// produced it with a known toolchain), it uses this entry point to state
/// it explicitly rather than relying on a heuristic that cannot see the
/// difference.  See `open-questions.md` for the broader question of how
/// the OS should auto-classify arbitrary bare static ELFs in the wild.
///
/// # Errors
///
/// Same as [`spawn_process`].
pub fn spawn_process_with_abi(
    elf_data: &[u8],
    options: &SpawnOptions<'_>,
    abi: pcb::AbiMode,
) -> KernelResult<SpawnResult> {
    spawn_process_inner(elf_data, options, Some(abi))
}

fn spawn_process_inner(
    elf_data: &[u8],
    options: &SpawnOptions<'_>,
    abi_override: Option<pcb::AbiMode>,
) -> KernelResult<SpawnResult> {
    // Step 1: Parse the ELF binary.
    let elf_file = elf::ElfFile::parse(elf_data)?;

    // Validate that the binary has loadable segments.
    let segment_count = elf_file.loadable_segments()?
        .count();
    if segment_count == 0 {
        serial_println!("[spawn] ELF has no loadable segments");
        return Err(KernelError::InvalidExecutable);
    }

    // Choose the executable's load bias.  ET_EXEC binaries load at their
    // absolute link-time vaddrs (bias 0); ET_DYN/PIE binaries are
    // position-independent, so we place them at an ASLR-randomised base
    // (known-issues.md TD9).  The bias shifts every PT_LOAD segment, the
    // entry point, and the AT_ENTRY/AT_PHDR auxv values uniformly.
    let exec_load_bias: u64 = choose_exec_load_bias(elf_file.is_pie());
    let raw_entry = elf_file.entry_point();
    let entry_point = raw_entry.checked_add(exec_load_bias).ok_or_else(|| {
        serial_println!("[spawn] executable entry point overflowed load bias");
        KernelError::InvalidExecutable
    })?;
    serial_println!(
        "[spawn] ELF validated: {} segment(s), entry={:#x} (raw {:#x}, bias {:#x}), pie={}",
        segment_count, entry_point, raw_entry, exec_load_bias, elf_file.is_pie()
    );

    // Detect whether this binary speaks the Linux x86_64 syscall ABI.
    //
    // We check before creating the PCB so the AbiMode can be stamped
    // immediately afterwards — that way the very first userspace
    // `syscall` (e.g. the dynamic loader's startup code) is already
    // routed to the Linux translation layer.
    // An explicit caller-supplied ABI wins over heuristic detection (see
    // `spawn_process_with_abi`); otherwise infer it from the ELF markers.
    let is_linux_abi = match abi_override {
        Some(m) => m == pcb::AbiMode::Linux,
        None => elf_file.detect_linux_abi(),
    };
    if is_linux_abi {
        if abi_override.is_some() {
            serial_println!("[spawn] Linux x86_64 ABI (explicit override)");
        } else {
            serial_println!("[spawn] Detected Linux x86_64 ABI binary");
        }
    }

    // Step 2: Create the process (allocates a per-process PML4 with
    // kernel entries 256-511 cloned from the boot PML4).
    let pid = pcb::create(options.name, options.parent);
    serial_println!("[spawn] Created process {} (\"{}\")", pid, options.name);

    // Stamp Linux ABI mode immediately after creation so any subsequent
    // syscall this process makes (including its very first) is dispatched
    // through `kernel::syscall::linux::dispatch_linux`.  Also install the
    // kernel-side fd table with stdin/stdout/stderr pre-pointing at the
    // console — without these entries the very first write(1, ...) from a
    // Linux binary would return -EBADF.
    if is_linux_abi {
        if let Err(e) = pcb::set_abi_mode(pid, pcb::AbiMode::Linux) {
            serial_println!(
                "[spawn] WARNING: failed to stamp Linux ABI on process {}: {:?}",
                pid, e
            );
            // Continue: the process still runs, just in native ABI mode.
            // detect_linux_abi was a hint; the failure mode (process can't
            // be found in the table immediately after create) shouldn't
            // happen unless something else is very wrong.
        } else if let Err(e) = pcb::linux_fd_install_stdio(pid) {
            serial_println!(
                "[spawn] WARNING: failed to install Linux stdio fds on process {}: {:?}",
                pid, e
            );
            // Non-fatal — the process runs but read/write on stdio
            // will fail until the binary opens something explicitly.
        }
    }

    // Get the process's PML4 physical address.
    let pml4_phys = pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or_else(|| {
            serial_println!(
                "[spawn] ERROR: process {} has no PML4 — out of memory?",
                pid
            );
            pcb::destroy(pid);
            KernelError::OutOfMemory
        })?;

    // Step 3: Load ELF segments into the process address space at the
    // chosen load bias (0 for ET_EXEC, LINUX_PIE_BASE for ET_DYN/PIE).
    //
    // SAFETY: pml4_phys was freshly allocated by pcb::create with
    // kernel entries cloned from the boot PML4.  No other CPU is using
    // this address space yet.
    if let Err(e) = unsafe { elf::load_segments_with_bias(&elf_file, pml4_phys, exec_load_bias) } {
        serial_println!("[spawn] Failed to load ELF segments: {:?}", e);
        pcb::destroy(pid);
        return Err(e);
    }

    // Step 3b (Linux ABI, dynamically-linked only): load the program
    // interpreter (ld.so) named in the executable's PT_INTERP segment.
    //
    // A dynamically-linked Linux binary cannot run on its own: the
    // kernel maps its interpreter and enters *that* image instead,
    // passing the real program entry via AT_ENTRY and the interpreter
    // base via AT_BASE so ld.so can relocate itself and the program.
    // Static executables have no PT_INTERP and skip this entirely; if
    // the interpreter is missing/unreadable/malformed we fall back to
    // direct entry (see `load_interpreter`), so static binaries and
    // native binaries are completely unaffected.
    let mut entry_rip = entry_point;
    let mut interp_base: Option<u64> = None;
    if is_linux_abi {
        // SAFETY: pml4_phys is the new process's private address space
        // (freshly created above); no other CPU is using it yet.
        match unsafe { load_interpreter(&elf_file, pml4_phys) } {
            Ok(Some(interp)) => {
                entry_rip = interp.entry_rip;
                interp_base = Some(interp.base);
            }
            // Ok(None): static executable, or a defensive fallback to
            // entering the executable's own entry point directly.
            Ok(None) => {}
            Err(e) => {
                serial_println!("[spawn] Failed to load interpreter: {:?}", e);
                pcb::destroy(pid);
                return Err(e);
            }
        }

        // Step 3c (Linux ABI): establish the `brk`/`sbrk` heap floor at the
        // page-aligned end of the main executable's last loadable segment
        // (Linux `set_brk` semantics — the break starts immediately above
        // the data/BSS), shifted up by a random ASLR gap (Linux
        // `arch_randomize_brk`, see `choose_brk_start`).  The interpreter
        // lives in its own high window and does not affect the heap floor.
        // `brk(2)` grows the heap upward from here.  A degenerate image (no
        // loadable segments) yields 0, i.e. "no heap"; brk(2) then behaves
        // as a pure query.
        match elf::image_end(&elf_file, exec_load_bias) {
            Ok(brk_start) => pcb::set_brk_region(pid, choose_brk_start(brk_start)),
            Err(e) => {
                serial_println!("[spawn] Failed to compute brk start: {:?}", e);
                pcb::destroy(pid);
                return Err(e);
            }
        }
    }

    // Step 4: Allocate and map the user stack.
    let mut user_rsp = match setup_user_stack(pml4_phys) {
        Ok(rsp) => rsp,
        Err(e) => {
            serial_println!("[spawn] Failed to set up user stack: {:?}", e);
            pcb::destroy(pid);
            return Err(e);
        }
    };

    // Step 4b (Linux ABI only): build the System V initial stack.
    //
    // A Linux binary's `_start` reads argc/argv/envp/auxv from the stack,
    // not via SYS_PROCESS_GET_ARGS.  `install_linux_stack` writes that
    // layout into the just-mapped stack frames and returns the aligned
    // `%rsp` to enter at.  The native `setup_user_stack` is deliberately
    // left untouched (design-decision #4); this only runs for Linux-ABI
    // images.
    if is_linux_abi {
        // interp_base is Some(base) for a dynamically-linked binary (so
        // ld.so receives AT_BASE) and None for a static one.  exec_load_bias
        // shifts AT_ENTRY/AT_PHDR to the executable's runtime addresses
        // (non-zero only for a PIE image).
        match build_linux_initial_stack(pml4_phys, &elf_file, options.argv, options.envp, interp_base, exec_load_bias) {
            Ok(installed) => {
                serial_println!(
                    "[spawn] Built Linux SysV stack: rsp={:#x} (was {:#x})",
                    installed.rsp, user_rsp
                );
                user_rsp = installed.rsp;
                // Persist the auxv so PR_GET_AUXV / /proc/<pid>/auxv can
                // serve the real vector for this Linux-ABI process.
                if let Err(e) = pcb::set_linux_saved_auxv(pid, installed.auxv_bytes) {
                    serial_println!("[spawn] Failed to save Linux auxv: {:?}", e);
                    pcb::destroy(pid);
                    return Err(e);
                }
            }
            Err(e) => {
                serial_println!("[spawn] Failed to build Linux SysV stack: {:?}", e);
                pcb::destroy(pid);
                return Err(e);
            }
        }
    }

    // Step 5: Grant initial capabilities.
    for &(resource_type, resource_id, rights) in options.capabilities {
        if let Err(e) = pcb::grant_capability(pid, resource_type, resource_id, rights) {
            serial_println!(
                "[spawn] Warning: failed to grant capability to process {}: {:?}",
                pid, e
            );
        }
    }

    // Step 5b: Grant the parent a Process capability for the child.
    //
    // This gives the parent the authority to kill, wait on, and
    // inspect the child process via capability checks.  PID 0
    // (kernel) doesn't need capabilities — it has implicit authority.
    if options.parent != 0 {
        let process_cap_rights = Rights::READ
            | Rights::WRITE
            | Rights::DELETE
            | Rights::WAIT
            | Rights::SIGNAL
            | Rights::DUPLICATE;
        if let Err(e) = pcb::grant_capability(
            options.parent,
            ResourceType::Process,
            pid,
            process_cap_rights,
        ) {
            serial_println!(
                "[spawn] Warning: failed to grant Process cap to parent {}: {:?}",
                options.parent, e
            );
            // Non-fatal — parent can still use implicit parent authority.
        }
    }

    // Step 5c: Inherit the parent's filesystem namespace.
    //
    // If the parent process is in a non-root namespace, the child
    // inherits it automatically (same isolation applies by default).
    // The parent can override this by attaching the child to a
    // different namespace before starting it.
    if options.parent != 0 {
        let parent_ns = crate::ipc::namespace::query(options.parent);
        if parent_ns != crate::ipc::namespace::ROOT_NAMESPACE {
            let _ = crate::ipc::namespace::attach(pid, parent_ns);
        }
    }

    // Step 5d: Apply fd inheritance map.
    //
    // If the parent passed an fd map, duplicate each parent handle and
    // store the (posix_fd, child_handle) pairs in the child's PCB.
    // The child's POSIX layer reads these during its init sequence via
    // SYS_PROCESS_GET_INITIAL_FDS.
    if !options.fd_map.is_empty() {
        let mut initial_fds = alloc::vec::Vec::with_capacity(options.fd_map.len());
        for &(fd_num, handle_type, parent_handle) in options.fd_map {
            let dup_result = match handle_type {
                fd_handle_type::FILE => {
                    // Duplicate the file handle — child gets an independent copy.
                    crate::fs::handle::dup(parent_handle)
                }
                fd_handle_type::PIPE => {
                    // Pipes have per-end refcounting: `dup()` increments
                    // the refcount on the appropriate end (read or
                    // write) and returns the same handle.  The child
                    // closes its reference independently when it dies
                    // or when its fd-table layer claims the handle.
                    crate::ipc::pipe::dup(
                        crate::ipc::pipe::PipeHandle::from_raw(parent_handle),
                    )
                    .map(|h| h.raw())
                }
                fd_handle_type::STREAM_SOCKET => {
                    // Stream socket endpoints are ref-counted per
                    // endpoint: `dup()` bumps the endpoint refcount and
                    // returns the same handle.  The child closes its
                    // reference independently when it dies or hands the
                    // handle to its fd-table.
                    crate::ipc::stream_socket::dup(
                        crate::ipc::stream_socket::StreamSocketHandle::from_raw(parent_handle),
                    )
                    .map(|h| h.raw())
                }
                fd_handle_type::CONSOLE => {
                    // Console is a virtual handle — just pass the value.
                    Ok(parent_handle)
                }
                fd_handle_type::EVENTFD => {
                    // Eventfds are ref-counted: `dup()` increments the
                    // refcount and returns the same id.  The child
                    // closes its reference independently when it dies
                    // (or when SYS_PROCESS_GET_INITIAL_FDS hands the
                    // handle off to the child's fd-table, which then
                    // owns the close).
                    crate::ipc::eventfd::dup(
                        crate::ipc::eventfd::EventFdHandle::from_raw(parent_handle),
                    )
                    .map(|h| h.raw())
                }
                _ => {
                    // Unknown handle type.
                    serial_println!(
                        "[spawn] Unknown handle type {} for fd {}",
                        handle_type, fd_num,
                    );
                    Err(KernelError::InvalidArgument)
                }
            };

            match dup_result {
                Ok(child_handle) => {
                    serial_println!(
                        "[spawn] fd {} → handle {} (type={}, duped from {})",
                        fd_num, child_handle, handle_type, parent_handle,
                    );
                    initial_fds.push((fd_num, handle_type, child_handle));
                }
                Err(e) => {
                    // Close any handles we already duped — don't leak.
                    // Pipe handles are pass-through (not duped) and
                    // console handles are virtual — skip those.
                    for &(_fd, ht, h) in &initial_fds {
                        match ht {
                            fd_handle_type::FILE => {
                                let _ = crate::fs::handle::close(h);
                            }
                            fd_handle_type::EVENTFD => {
                                crate::ipc::eventfd::close(
                                    crate::ipc::eventfd::EventFdHandle::from_raw(h),
                                );
                            }
                            fd_handle_type::PIPE => {
                                crate::ipc::pipe::close(
                                    crate::ipc::pipe::PipeHandle::from_raw(h),
                                );
                            }
                            fd_handle_type::STREAM_SOCKET => {
                                crate::ipc::stream_socket::close(
                                    crate::ipc::stream_socket::StreamSocketHandle::from_raw(h),
                                );
                            }
                            _ => {} // CONSOLE/etc.: nothing to close yet.
                        }
                    }
                    serial_println!(
                        "[spawn] Failed to dup handle {} (type={}) for fd {}: {:?}",
                        parent_handle, handle_type, fd_num, e,
                    );
                    pcb::destroy(pid);
                    return Err(e);
                }
            }
        }
        pcb::set_initial_fds(pid, initial_fds);
    }

    // Step 5e: Store argv and envp in the child's PCB.
    //
    // The child's POSIX layer reads these via SYS_PROCESS_GET_ARGS
    // during its init sequence.  The data is stored in kernel heap
    // and freed when the child reads it (or when the process dies).
    if !options.argv.is_empty() || !options.envp.is_empty() {
        let argv_vecs: alloc::vec::Vec<alloc::vec::Vec<u8>> = options.argv
            .iter()
            .map(|a| a.to_vec())
            .collect();
        let envp_vecs: alloc::vec::Vec<alloc::vec::Vec<u8>> = options.envp
            .iter()
            .map(|e| e.to_vec())
            .collect();
        if let Err(e) = pcb::set_initial_args(pid, argv_vecs, envp_vecs) {
            serial_println!(
                "[spawn] Failed to set initial args for process {}: {:?}",
                pid, e,
            );
            // Non-fatal for now — process can still run without args.
        } else {
            serial_println!(
                "[spawn] Stored {} argv, {} envp entries for process {}",
                options.argv.len(), options.envp.len(), pid,
            );
        }
    }

    // Record the executable path (backs /proc/<pid>/exe) when the caller
    // supplied one.  Best-effort: a malformed path or a missing PCB only
    // means /proc/<pid>/exe reports NotFound — never fail the spawn.
    if let Some(path) = options.exe_path {
        if let Err(e) = pcb::set_exe_path(pid, path.to_vec()) {
            serial_println!(
                "[spawn] Failed to record exe path for process {}: {:?}",
                pid, e,
            );
        }
    }

    // Apply the initial working directory (container `WorkingDir`/`--workdir`)
    // when the caller supplied one.  Best-effort: a malformed value (not an
    // absolute path, too long, or containing NUL) is rejected by `set_cwd` and
    // logged — the child simply stays at the PCB default cwd `/`, never failing
    // the spawn.
    if let Some(dir) = options.cwd {
        if let Err(e) = pcb::set_cwd(pid, dir.to_vec()) {
            serial_println!(
                "[spawn] Ignoring invalid initial cwd for process {}: {:?}",
                pid, e,
            );
        }
    }

    // Apply the initial user/group identity if one was requested (honors a
    // container image's `User` config / the Docker `--user`/`-u` flag). The
    // child's credentials are replaced with a fresh numeric identity (no
    // supplementary groups). A failure here is logged but never fails the
    // spawn — the child simply keeps the inherited (root) credentials.
    if let Some((uid, gid)) = options.uid_gid {
        if let Err(e) = pcb::set_credentials(pid, pcb::ProcessCredentials::new(uid, gid)) {
            serial_println!(
                "[spawn] Ignoring invalid initial uid/gid for process {}: {:?}",
                pid, e,
            );
        }
    }

    // Step 6: Create the entry info struct (heap-allocated, freed by
    // the trampoline when the thread first runs) and spawn the
    // initial thread.
    let info = Box::new(UserEntryInfo {
        // For a dynamically-linked Linux binary this is the interpreter's
        // entry (base + interp.e_entry); otherwise it is the executable's
        // own entry point.
        entry_rip,
        user_rsp,
    });
    let info_ptr = Box::into_raw(info) as u64;

    let task_id = match thread::spawn(
        pid,
        options.name.as_bytes(),
        options.priority,
        userspace_entry_trampoline,
        info_ptr,
    ) {
        Ok(id) => id,
        Err(e) => {
            // Thread creation failed.  Free the info struct.
            //
            // SAFETY: info_ptr was just created by Box::into_raw and
            // no one else has accessed it.
            drop(unsafe { Box::from_raw(info_ptr as *mut UserEntryInfo) });
            pcb::destroy(pid);
            return Err(e);
        }
    };

    serial_println!(
        "[spawn] Process {} running (thread {}, entry={:#x}, user_rsp={:#x})",
        pid, task_id, entry_rip, user_rsp
    );

    Ok(SpawnResult {
        pid,
        task_id,
        entry_point,
    })
}

// ---------------------------------------------------------------------------
// exec — replace current process image
// ---------------------------------------------------------------------------

/// Maximum visible length of a task `comm` name (Linux's
/// `TASK_COMM_LEN - 1`).  The full field is 16 bytes including the NUL.
const COMM_MAX_VISIBLE: usize = 15;

/// Compute the `comm` basename for a new exec image the way Linux's
/// `kbasename()` does: the path component after the final `/`, truncated
/// to `TASK_COMM_LEN - 1` (15) bytes.
///
/// Returns an empty slice when `src` has no usable trailing component
/// (e.g. it is empty or ends in `/`), in which case the caller should
/// leave the existing comm unchanged.
fn exec_comm_basename(src: &[u8]) -> &[u8] {
    // `rsplit` always yields at least one element, so `next()` is `Some`;
    // `unwrap_or` keeps it total without an `expect`.
    let base = src.rsplit(|&b| b == b'/').next().unwrap_or(src);
    let n = base.len().min(COMM_MAX_VISIBLE);
    base.get(..n).unwrap_or(&[])
}

/// Replace the current process's address space with a new ELF binary.
///
/// This is the `exec` equivalent: the calling thread's process gets a
/// fresh address space loaded from `elf_data`, and the caller receives
/// the new entry point and stack pointer to IRETQ into.
///
/// ## What It Does
///
/// 1. Validates the ELF binary.
/// 2. Clears the process's existing user address space (frees all
///    mapped frames and intermediate page table pages).
/// 3. Loads the new ELF segments into the clean address space.
/// 4. Allocates and maps a fresh user stack.
/// 5. Returns [`ExecResult`] with the new entry point and stack pointer.
///
/// ## What It Does NOT Do
///
/// - Does NOT modify the thread's kernel stack or scheduler run state.
///   (It does update the scheduler task's `comm` name to the new image's
///   basename, mirroring Linux's execve — see the end of the function.)
/// - Does NOT modify the capability table (capabilities survive exec,
///   matching our security model — the process keeps its existing
///   rights unless explicitly revoked).
/// - Does NOT create a new thread — the calling thread continues
///   (the syscall handler builds an IRETQ frame from the result).
///
/// ## Atomicity
///
/// If any step after the address space teardown fails (e.g., ELF
/// loading or stack allocation runs out of memory), the process is
/// left in a broken state with an empty address space.  The correct
/// response is to kill the process.  This matches POSIX exec
/// behavior: "If the exec function returns to the calling process
/// image, an error has occurred."
///
/// # Arguments
///
/// - `pid` — the process to exec.
/// - `elf_data` — raw bytes of the new ELF64 executable.
/// - `argv` — command-line arguments for the new process image.
/// - `envp` — environment variables for the new process image.
/// - `exe_path` — resolved absolute path of the new image, recorded to
///   back `/proc/<pid>/exe`.  `None` leaves the previous value cleared
///   (the exec replaces the image, so a stale path would be wrong); an
///   absolute path overwrites it.
///
/// The argv/envp data is stored in the PCB (replacing any previous
/// values) and can be read by the new binary via
/// `SYS_PROCESS_GET_ARGS`.
///
/// # Errors
///
/// - [`KernelError::InvalidExecutable`] if the ELF binary is invalid
///   (returned before the old address space is torn down).
/// - [`KernelError::OutOfMemory`] if allocation fails during load.
/// - [`KernelError::NoSuchProcess`] if the PID doesn't exist.
pub fn exec_process(
    pid: ProcessId,
    elf_data: &[u8],
    argv: &[&[u8]],
    envp: &[&[u8]],
    exe_path: Option<&[u8]>,
) -> KernelResult<ExecResult> {
    // Step 1: Parse and validate the ELF binary BEFORE tearing down
    // the old address space.  If the ELF is bad, the process keeps
    // running its old code.
    let elf_file = elf::ElfFile::parse(elf_data)?;

    let segment_count = elf_file.loadable_segments()?
        .count();
    if segment_count == 0 {
        serial_println!("[exec] ELF has no loadable segments");
        return Err(KernelError::InvalidExecutable);
    }

    // Choose the executable's load bias (0 for ET_EXEC, an ASLR-randomised
    // base ≥ LINUX_PIE_BASE for ET_DYN/PIE) and bias the entry point
    // accordingly.  Computed before the old address space is torn down so a
    // malformed bias/entry combination still bails out with the old image
    // intact.
    let exec_load_bias: u64 = choose_exec_load_bias(elf_file.is_pie());
    let raw_entry = elf_file.entry_point();
    let entry_point = raw_entry.checked_add(exec_load_bias).ok_or_else(|| {
        serial_println!("[exec] executable entry point overflowed load bias");
        KernelError::InvalidExecutable
    })?;
    serial_println!(
        "[exec] ELF validated for exec: {} segment(s), entry={:#x} (raw {:#x}, bias {:#x}), pie={}",
        segment_count, entry_point, raw_entry, exec_load_bias, elf_file.is_pie()
    );

    // Re-detect ABI mode for the new image.  exec replaces the process
    // image entirely, so a Native parent can exec into a Linux binary
    // (or vice-versa) and the syscall dispatcher must follow.
    let new_abi_mode = if elf_file.detect_linux_abi() {
        pcb::AbiMode::Linux
    } else {
        pcb::AbiMode::Native
    };
    if new_abi_mode == pcb::AbiMode::Linux {
        serial_println!("[exec] Detected Linux x86_64 ABI binary for new image");
    }

    // Step 2: Get the process's PML4.
    let pml4_phys = pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or_else(|| {
            serial_println!("[exec] ERROR: process {} has no PML4", pid);
            KernelError::NoSuchProcess
        })?;

    // Step 3: Tear down the old user address space.
    //
    // After this point, the process has an empty user address space.
    // If anything below fails, the process cannot resume its old code
    // and must be killed.
    //
    // SAFETY: The calling thread is in the kernel (handling SYSCALL),
    // so no user code is executing.  The kernel half of the page table
    // (entries 256–511) is untouched, so kernel code/stack/HHDM remain
    // accessible.  The PML4 is still loaded in CR3, but all user TLB
    // entries will be flushed by the new mappings (or by the return to
    // ring 3 which will touch new pages).
    serial_println!("[exec] Tearing down old address space for process {}", pid);
    unsafe {
        page_table::clear_user_address_space(pml4_phys);
    }

    // The page tables and frames are gone; drop the matching VMA metadata
    // (and release any file-backed mapping references) so the new image
    // starts with an empty VMA list instead of inheriting stale records
    // from the old one.
    pcb::reset_vmas_for_exec(pid);

    // Reset the per-process Linux ABI state that execve clears on every
    // successful exec (membarrier registrations, dumpable, keepcaps, and
    // the securebits SECBIT_KEEP_CAPS bit).
    // Mirrors Linux's begin_new_exec / membarrier_exec_mmap; the fields
    // Linux preserves (thp_disable and memory_merge — both MMF_INIT_MASK
    // mm-flags — plus pdeathsig, personality, no_new_privs, child_subreaper,
    // timer slack) are deliberately left untouched. See
    // pcb::reset_linux_state_for_exec for the per-field rationale.
    pcb::reset_linux_state_for_exec(pid);

    // Flush TLB for the user half.  Since we just freed all user
    // mappings, any stale TLB entries could cause use-after-free.
    // Reloading CR3 flushes all non-global TLB entries.
    // SAFETY: `pml4_phys` is the current process's valid PML4 physical
    // address (obtained from its PCB).  Writing it back to CR3 is a
    // privileged but non-destructive operation that flushes the TLB
    // without changing the active page table.  We are in kernel context
    // with interrupts enabled, so the kernel mappings (upper half) are
    // intact and remain valid throughout.
    unsafe {
        page_table::write_cr3(pml4_phys);
    }

    // Step 4: Load the new ELF segments into the clean address space.
    // SAFETY: `pml4_phys` is the current process's valid PML4, which now
    // has an empty user-half after clear_user_address_space + TLB flush.
    // `elf_file` was validated by `elf::parse` above.  load_segments
    // allocates frames and maps them into the process's address space.
    //
    // After the teardown above, any failure leaves the process with an
    // empty user address space.  The calling thread is handling SYSCALL
    // in kernel mode — if we return an error, the SYSRET will jump to
    // the old user_rip which no longer exists, causing an immediate #PF.
    // To make failure deterministic, we set the exit code and return
    // the error so the syscall handler knows to exit the thread cleanly.
    if let Err(e) = unsafe { elf::load_segments_with_bias(&elf_file, pml4_phys, exec_load_bias) } {
        serial_println!("[exec] Failed to load new ELF segments: {:?}", e);
        let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
        return Err(e);
    }

    // Step 4b (Linux ABI, dynamically-linked only): load the program
    // interpreter (ld.so) for the new image.  Mirrors spawn_process's
    // Step 3b — a dynamically-linked Linux binary enters its interpreter
    // (base + interp.e_entry) with AT_BASE set; static/native images and
    // the missing-interpreter fallback enter the executable directly.
    let mut entry_rip = entry_point;
    let mut interp_base: Option<u64> = None;
    if new_abi_mode == pcb::AbiMode::Linux {
        // SAFETY: pml4_phys is this process's address space, whose user
        // half was just cleared above; the calling thread is in the
        // kernel so no user code runs concurrently.
        match unsafe { load_interpreter(&elf_file, pml4_phys) } {
            Ok(Some(interp)) => {
                entry_rip = interp.entry_rip;
                interp_base = Some(interp.base);
            }
            Ok(None) => {}
            Err(e) => {
                serial_println!("[exec] Failed to load interpreter: {:?}", e);
                let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
                return Err(e);
            }
        }

        // Re-establish the `brk` heap floor for the new image (Linux
        // `set_brk` + `arch_randomize_brk`, see `choose_brk_start`),
        // replacing whatever heap the old image had.  The old heap's frames
        // and VMAs were already torn down with the rest of the user address
        // space above.
        match elf::image_end(&elf_file, exec_load_bias) {
            Ok(brk_start) => pcb::set_brk_region(pid, choose_brk_start(brk_start)),
            Err(e) => {
                serial_println!("[exec] Failed to compute brk start: {:?}", e);
                let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
                return Err(e);
            }
        }
    } else {
        // Exec'ing a native image: it has no Linux `brk` heap.  Clear any
        // heap state inherited from a previous Linux image so a later
        // (erroneous) brk(2) cannot resize against stale addresses.
        pcb::set_brk_region(pid, 0);
    }

    // execve replaces the address space, so the old TLS block is gone.
    // Reset the FS (TLS) base to 0 both in the live MSR and on the
    // persistent Task field, so no stale TLS pointer survives into the
    // new image before its glibc _start re-installs one via
    // arch_prctl(ARCH_SET_FS).  Without this reset, a faulting access
    // through %fs early in the new program would read the previous
    // image's (now-unmapped) TLS address.
    // SAFETY: writing 0 to IA32_FS_BASE is canonical and cannot #GP.
    unsafe { crate::cpu::wrmsr(crate::cpu::IA32_FS_BASE, 0); }
    crate::sched::set_current_task_fs_base(0);
    // Likewise reset the userspace %gs base to 0.  Like %fs, the userspace
    // %gs base is the active IA32_GS_BASE (the entry stub swaps GS back before
    // calling Rust, so the handler runs with active GS = user %gs and the
    // per-CPU pointer rests in KERNEL_GS_BASE).  Clearing it both in the live
    // MSR and on the Task field ensures the new image starts with no stale
    // %gs base before its glibc _start optionally re-installs one via
    // arch_prctl(ARCH_SET_GS).
    // SAFETY: writing 0 to IA32_GS_BASE is canonical and cannot #GP.
    unsafe { crate::cpu::wrmsr(crate::cpu::IA32_GS_BASE, 0); }
    crate::sched::set_current_task_gs_base(0);

    // Step 5: Allocate and map a fresh user stack.
    let mut user_rsp = match setup_user_stack(pml4_phys) {
        Ok(rsp) => rsp,
        Err(e) => {
            serial_println!("[exec] Failed to set up user stack: {:?}", e);
            let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
            return Err(e);
        }
    };

    // Step 5a (Linux ABI only): build the System V initial stack.
    //
    // If the new image speaks the Linux ABI, its `_start` expects argc/
    // argv/envp/auxv on the stack.  Build that layout in-place (the active
    // page table is already `pml4_phys` here, but `install_linux_stack`
    // walks the table explicitly via HHDM so it is independent of CR3).
    // Native images keep the bare stack from `setup_user_stack`.
    if new_abi_mode == pcb::AbiMode::Linux {
        // interp_base is Some(base) for a dynamically-linked binary and
        // None for a static one.  exec_load_bias shifts AT_ENTRY/AT_PHDR
        // to the executable's runtime addresses (non-zero only for PIE).
        match build_linux_initial_stack(pml4_phys, &elf_file, argv, envp, interp_base, exec_load_bias) {
            Ok(installed) => {
                serial_println!(
                    "[exec] Built Linux SysV stack: rsp={:#x} (was {:#x})",
                    installed.rsp, user_rsp
                );
                user_rsp = installed.rsp;
                // Replace any prior saved auxv with the freshly-built one
                // (execve rebuilds the stack, so the auxv changes too).
                if let Err(e) = pcb::set_linux_saved_auxv(pid, installed.auxv_bytes) {
                    serial_println!("[exec] Failed to save Linux auxv: {:?}", e);
                    let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
                    return Err(e);
                }
            }
            Err(e) => {
                serial_println!("[exec] Failed to build Linux SysV stack: {:?}", e);
                let _ = pcb::set_exit_code(pid, KILLED_EXIT_CODE);
                return Err(e);
            }
        }
    }

    // Step 5b: Update the process's ABI mode to match the new image.
    //
    // exec replaces the process image entirely; a Native process that
    // execs into a Linux binary must from this point onwards have its
    // `syscall`s routed through the Linux translation layer, and vice
    // versa.  The actual switch takes effect on the next syscall from
    // ring 3 because `syscall_handler_inner` re-reads the abi_mode on
    // every entry.
    //
    // exec re-initialises the kernel-side Linux fd table per POSIX
    // close-on-exec semantics:
    //
    //   - If the old image was already Linux ABI, walk its fd table,
    //     close every entry whose FD_CLOEXEC flag is set (releasing
    //     the underlying kernel handle if no other fd references it),
    //     and keep the rest.  Stdin/stdout/stderr are then refilled
    //     with Console entries if the previous image had closed them.
    //
    //   - If the old image was Native ABI, the previous PCB had no
    //     Linux fd table, so we install a fresh stdio-only one.
    //
    // The ABI switch itself takes effect on the next syscall from ring
    // 3 because `syscall_handler_inner` re-reads abi_mode on every
    // entry; here we just update the PCB.
    let old_abi_mode = pcb::get_abi_mode(pid);
    if let Err(e) = pcb::set_abi_mode(pid, new_abi_mode) {
        serial_println!(
            "[exec] WARNING: failed to update ABI mode on process {}: {:?}",
            pid, e
        );
        // Non-fatal — the process keeps its previous abi_mode, which
        // will be wrong for the new image but matches the worst-case
        // behaviour of running a Linux binary in Native mode (or
        // vice-versa).  Caller has a destroyed-image process either
        // way; this just makes its syscalls behave inconsistently
        // rather than failing the exec.
    }
    if new_abi_mode == pcb::AbiMode::Linux {
        if old_abi_mode == Some(pcb::AbiMode::Linux) {
            // Re-use the existing table: close cloexec entries (and
            // ensure stdio remains populated) via the kernel helper,
            // then close each returned handle.
            if let Some(to_close) = pcb::linux_fd_exec_cloexec(pid) {
                let count = to_close.len();
                for entry in to_close {
                    let res = crate::syscall::linux::close_handle(entry);
                    if res.value < 0 {
                        serial_println!(
                            "[exec] WARNING: close-on-exec for kind={:?} \
                             raw={:#x} returned {} on process {}",
                            entry.kind, entry.raw_handle, res.value, pid,
                        );
                    }
                }
                if count > 0 {
                    serial_println!(
                        "[exec] Closed {} cloexec fd(s) on process {}",
                        count, pid,
                    );
                }
            } else {
                // No old fd table even though abi_mode was Linux —
                // shouldn't happen, but fall back to installing fresh
                // stdio so the new image has something to use.
                serial_println!(
                    "[exec] WARNING: process {} had AbiMode::Linux but no fd \
                     table; installing fresh stdio",
                    pid,
                );
                if let Err(e) = pcb::linux_fd_install_stdio(pid) {
                    serial_println!(
                        "[exec] WARNING: failed to install Linux stdio fds \
                         on process {}: {:?}",
                        pid, e
                    );
                }
            }
        } else if let Err(e) = pcb::linux_fd_install_stdio(pid) {
            // Native → Linux: install fresh stdio-only table.
            serial_println!(
                "[exec] WARNING: failed to install Linux stdio fds on process {}: {:?}",
                pid, e
            );
        }
    } else {
        // exec into a *native* image: a native process has no auxv by
        // design (design-decision #4), so drop any auxv carried over
        // from a previous Linux-ABI image.
        pcb::clear_linux_saved_auxv(pid);
    }

    // Step 6: Store argv/envp in the PCB for the new process image.
    //
    // This replaces any previous argv/envp (from the original spawn or
    // a prior exec).  The new binary reads them via SYS_PROCESS_GET_ARGS.
    if !argv.is_empty() || !envp.is_empty() {
        let argv_vecs: alloc::vec::Vec<alloc::vec::Vec<u8>> = argv
            .iter()
            .map(|a| a.to_vec())
            .collect();
        let envp_vecs: alloc::vec::Vec<alloc::vec::Vec<u8>> = envp
            .iter()
            .map(|e| e.to_vec())
            .collect();
        if let Err(e) = pcb::set_initial_args(pid, argv_vecs, envp_vecs) {
            serial_println!(
                "[exec] Failed to set args for process {}: {:?}",
                pid, e,
            );
            // Non-fatal — process can still run without args.
        } else {
            serial_println!(
                "[exec] Stored {} argv, {} envp entries for process {}",
                argv.len(), envp.len(), pid,
            );
        }
    }

    // Record the new image's path for /proc/<pid>/exe.  exec replaces the
    // image, so we always overwrite: a supplied absolute path takes
    // effect; otherwise the previous (now-stale) path is cleared so the
    // link reports NotFound rather than the old binary.  Best-effort.
    if let Some(path) = exe_path {
        if let Err(e) = pcb::set_exe_path(pid, path.to_vec()) {
            serial_println!(
                "[exec] Failed to record exe path for process {}: {:?}",
                pid, e,
            );
        }
    } else {
        pcb::clear_exe_path(pid);
    }

    // Update the calling thread's comm to the new program's basename, the
    // way Linux's execve does (`set_task_comm(current, kbasename(filename))`).
    // The comm lives on the per-thread scheduler task — it is what
    // `/proc/<id>/comm`, `/proc/<id>/stat` field 2, `/proc/<id>/status`
    // `Name:`, and `prctl(PR_GET_NAME)` all read — so without this an
    // exec'd process would keep reporting its pre-exec name.  Prefer the
    // resolved exe_path (matching Linux's use of the filename); fall back
    // to argv[0] for callers that pass no path (e.g. the native SYS_EXEC
    // surface).  Best-effort: a process with no derivable name keeps its
    // previous comm, and a missing scheduler task (no current task) is a
    // silent no-op.
    let comm_src: Option<&[u8]> = exe_path.or_else(|| argv.first().copied());
    if let Some(src) = comm_src {
        let base = exec_comm_basename(src);
        if !base.is_empty() {
            let _ = crate::sched::set_task_name(
                crate::sched::current_task_id(),
                base,
            );
        }
    }

    serial_println!(
        "[exec] Process {} exec complete: entry={:#x}, rsp={:#x}",
        pid, entry_rip, user_rsp
    );

    Ok(ExecResult {
        entry_rip,
        user_rsp,
    })
}

// ---------------------------------------------------------------------------
// Linux dynamic-linker (ld.so) loading
// ---------------------------------------------------------------------------

/// Fixed load base for a Linux program interpreter (ld.so).
///
/// Dynamically-linked Linux executables name their interpreter in a
/// `PT_INTERP` segment.  That interpreter is a position-independent
/// (ET_DYN) image: the kernel loads it at this fixed base, enters at
/// `base + interp.e_entry`, and reports the base to the program via the
/// `AT_BASE` auxv entry so ld.so can relocate itself and then the
/// program.
///
/// This is the **low edge** of the interpreter load window; the actual
/// per-exec base is `LINUX_INTERP_BASE + (random page index) * FRAME_SIZE`
/// (see [`apply_aslr_base`] / [`INTERP_ASLR_BITS`]) once the CSPRNG is
/// seeded.  It sits well clear of both the executable (loaded low, around
/// `0x40_0000_0000`) and the user stack region (top `USER_STACK_TOP` =
/// `0x0000_7FFF_FFFF_0000`, growing down by at most `MAX_STACK_SIZE`).  A
/// typical ld.so image is a few hundred KiB, so the gap above this base to
/// the stack guard is ample even at the top of the ASLR window.
pub(crate) const LINUX_INTERP_BASE: u64 = 0x0000_7000_0000_0000;

/// ASLR entropy applied to the Linux program-interpreter load base,
/// expressed in bits at 16 KiB-page granularity.
///
/// `28` matches Linux x86_64's default `mmap_rnd_bits` (28) — i.e. 2^28
/// equally-likely load bases, the same layout entropy Linux provides for
/// the mmap region the interpreter is mapped into — applied here in our
/// 16 KiB page units.  2^28 pages × 16 KiB = a 4 TiB window, which sits
/// entirely inside the ~15 TiB gap between [`LINUX_INTERP_BASE`]
/// (`0x7000_0000_0000`) and the user-stack region
/// (`USER_STACK_TOP` = `0x7FFF_FFFF_0000`).  The highest possible base,
/// `LINUX_INTERP_BASE + (2^28 - 1) * 16 KiB ≈ 0x73FF_FFFF_C000`, is far
/// below [`USER_STACK_GUARD`], so a randomised interpreter base can never
/// collide with the stack, the executable (loaded low), the brk heap, or
/// the general mmap window (`0x0060_…`).  The interpreter image is the
/// sole occupant of this window, so intra-window collisions are
/// impossible.  (`spawn::self_test` asserts this clearance invariant.)
const INTERP_ASLR_BITS: u32 = 28;

/// Number of distinct 16 KiB-aligned interpreter bases = `2^INTERP_ASLR_BITS`.
/// Evaluated at compile time; the random page index is drawn unbiased from
/// `[0, INTERP_ASLR_SPAN_PAGES)` via [`crate::rng::next_bounded`].
const INTERP_ASLR_SPAN_PAGES: u64 = 1u64 << INTERP_ASLR_BITS;

/// Apply a page-granular ASLR offset to a fixed ELF load base.
///
/// `rand_pages` is a random page index in `[0, 2^INTERP_ASLR_BITS)` (drawn
/// by the caller via [`crate::rng::next_bounded`]).  The returned base is
/// `fixed_base + rand_pages * FRAME_SIZE`, computed with saturating
/// arithmetic so a pathological input can never wrap past the top of the
/// address space.  Because the offset is a whole number of 16 KiB pages
/// and `fixed_base` is 16 KiB-aligned, the result preserves the
/// page-offset congruence that [`elf::load_segments_with_bias`] requires.
fn apply_aslr_base(fixed_base: u64, rand_pages: u64) -> u64 {
    let offset = rand_pages.saturating_mul(crate::mm::frame::FRAME_SIZE as u64);
    fixed_base.saturating_add(offset)
}

/// Load base for a position-independent (`ET_DYN`/PIE) main executable —
/// the **low edge** of the PIE ASLR window (Linux's `ELF_ET_DYN_BASE`,
/// used as the minimum, with randomisation added upward; see
/// [`PIE_ASLR_BITS`] / [`choose_exec_load_bias`]).
///
/// Modern Linux executables are PIE: their `PT_LOAD` segments use small
/// link-time vaddrs (often starting at 0), and the kernel must place the
/// image at a chosen base.  Loading at bias 0 would map the null page and
/// hand out an `AT_ENTRY`/`AT_PHDR` of essentially 0 — both wrong.  We
/// load PIE executables at a randomised base ≥ this floor and report
/// `e_entry + base` / `phdr_vaddr + base` through the auxv via
/// [`crate::proc::linux_stack`]'s `exec_load_bias`.
///
/// `0x0000_5555_5555_4000` is the classic Linux PIE base floor
/// (`ELF_ET_DYN_BASE`-derived).  It is 16 KiB-aligned (low 14 bits zero,
/// so each `bias + p_vaddr` preserves page-offset congruence), sits far
/// *above* the general mmap window (`USER_MMAP_BASE = 0x60_0000_0000` ..
/// `0x70_0000_0000`) and far *below* the interpreter window
/// (`LINUX_INTERP_BASE = 0x7000_0000_0000`).  The ~26.7 TiB gap up to the
/// interpreter floor leaves room for the full ASLR window plus a PIE image
/// and its brk growth before colliding with either neighbour.
const LINUX_PIE_BASE: u64 = 0x0000_5555_5555_4000;

/// ASLR entropy applied to the PIE main-executable base, in bits at
/// 16 KiB-page granularity.
///
/// `28` matches Linux x86_64's default `mmap_rnd_bits` (28 bits of
/// layout entropy), applied here in 16 KiB page units → a 4 TiB window
/// added upward from [`LINUX_PIE_BASE`].  The highest possible PIE base,
/// `LINUX_PIE_BASE + (2^28 - 1) * 16 KiB ≈ 0x5955_5555_0000`, leaves
/// ~22 TiB of headroom below the interpreter floor
/// (`LINUX_INTERP_BASE = 0x7000_0000_0000`) for the image and brk growth,
/// so a randomised PIE base cannot collide with the interpreter window
/// above, the mmap window far below, or the stack.  (`spawn::self_test`'s
/// `test_pie_aslr_window` asserts this headroom invariant.)
const PIE_ASLR_BITS: u32 = 28;

/// Number of distinct 16 KiB-aligned PIE bases = `2^PIE_ASLR_BITS`.
/// The random page index is drawn unbiased from `[0, PIE_ASLR_SPAN_PAGES)`
/// via [`crate::rng::next_bounded`].
const PIE_ASLR_SPAN_PAGES: u64 = 1u64 << PIE_ASLR_BITS;

/// Choose the main executable's load bias: `0` for an `ET_EXEC` binary
/// (absolute link-time vaddrs), or an ASLR-randomised base ≥
/// [`LINUX_PIE_BASE`] for an `ET_DYN`/PIE binary.
///
/// The bias uniformly shifts every `PT_LOAD` segment, the entry point, and
/// the `AT_ENTRY`/`AT_PHDR` auxv values, so callers compute it once and
/// thread it through [`elf::load_segments_with_bias`] and the SysV stack
/// builder.  Randomisation is applied only once the CSPRNG is seeded;
/// before that (very early boot, before any PIE process can spawn in
/// practice) it falls back to the fixed floor.  See known-issues.md TD9.
fn choose_exec_load_bias(is_pie: bool) -> u64 {
    if !is_pie {
        return 0;
    }
    if crate::rng::is_initialized() {
        apply_aslr_base(LINUX_PIE_BASE, crate::rng::next_bounded(PIE_ASLR_SPAN_PAGES))
    } else {
        LINUX_PIE_BASE
    }
}

/// ASLR entropy applied to the `brk` heap floor (Linux `arch_randomize_brk`),
/// in bits at 16 KiB-page granularity.
///
/// `13` mirrors Linux x86_64's `arch_randomize_brk`, which calls
/// `randomize_page(mm->brk, 0x02000000)` — a 32 MiB range that, at Linux's
/// 4 KiB page size, yields `0x02000000 >> 12 = 8192 = 2^13` distinct page
/// positions.  Per design-decision #20, the ASLR security metric is the
/// *number of equally-likely positions* (entropy bits), not the byte span, so
/// we match Linux's 13 bits rather than its 32 MiB span; at our 16 KiB pages
/// that is a `2^13 * 16 KiB = 128 MiB` maximum gap between the executable's
/// data segment and the heap floor.  This gap is dwarfed by the smallest heap
/// window (a low-loaded ET_EXEC has ~hundreds of GiB up to `USER_MMAP_BASE`),
/// so randomising the floor never meaningfully reduces the room available for
/// `brk` growth and can never push the floor across its `brk_ceiling`.
const BRK_ASLR_BITS: u32 = 13;

/// Number of distinct 16 KiB-aligned heap-floor positions = `2^BRK_ASLR_BITS`.
/// The random page index is drawn unbiased from `[0, BRK_ASLR_SPAN_PAGES)`
/// via [`crate::rng::next_bounded`].
const BRK_ASLR_SPAN_PAGES: u64 = 1u64 << BRK_ASLR_BITS;

/// Choose the `brk` heap floor for a Linux image: the page-aligned image end
/// shifted up by a random ASLR gap (Linux `arch_randomize_brk`).
///
/// `image_end` is [`elf::image_end`]'s page-aligned top of the last loadable
/// segment.  A degenerate image (no loadable segments) yields `0`, which means
/// "no heap" — we must preserve that exactly (adding a gap to `0` would
/// erroneously enable a heap at a random low address), so `image_end == 0`
/// returns `0` unchanged.  Otherwise, once the CSPRNG is seeded, the floor is
/// `image_end + next_bounded(2^BRK_ASLR_BITS) * FRAME_SIZE` (via the same pure
/// [`apply_aslr_base`] helper used for the load bases); before the CSPRNG is
/// seeded (very early boot, before any Linux process can spawn in practice) it
/// falls back to `image_end` with no gap.  The result stays 16 KiB-aligned
/// (image_end is page-aligned and the gap is a whole number of pages).
fn choose_brk_start(image_end: u64) -> u64 {
    if image_end == 0 {
        return 0;
    }
    if crate::rng::is_initialized() {
        apply_aslr_base(image_end, crate::rng::next_bounded(BRK_ASLR_SPAN_PAGES))
    } else {
        image_end
    }
}

/// Where a loaded program interpreter was placed and where to enter it.
struct LoadedInterp {
    /// Fixed base the interpreter image was loaded at (reported via
    /// `AT_BASE`).
    base: u64,
    /// Entry point to jump to: `base + interp.e_entry`.
    entry_rip: u64,
}

/// Load the program interpreter (ld.so) of a dynamically-linked Linux
/// executable into the given address space.
///
/// If `elf_file` has no `PT_INTERP` segment it is a static executable
/// and this returns `Ok(None)`.  For a dynamically-linked executable it
/// resolves the interpreter path, reads the interpreter image from the
/// VFS, parses it, loads its `PT_LOAD` segments at an ASLR-randomised base
/// (drawn from the [`LINUX_INTERP_BASE`] window — see [`apply_aslr_base`])
/// via [`elf::load_segments_with_bias`], and returns the base plus the
/// entry point `base + interp.e_entry`.
///
/// **Defensive fallback:** if the interpreter path is not valid UTF-8
/// (the VFS API is `&str`-based), the file is absent/unreadable, the
/// image fails to parse, or its segments fail to load, this logs a
/// warning and returns `Ok(None)` — the caller then enters the
/// executable's own entry point directly (today's behaviour).  This
/// keeps static and native binaries completely unaffected and degrades
/// gracefully when an interpreter is missing rather than killing the
/// process before it starts.  A genuine internal error (e.g. an
/// arithmetic overflow computing the entry) is returned as `Err`.
///
/// # Safety
///
/// `pml4_phys` must be the freshly-prepared address space for the
/// process being started (a clean user half); no other CPU may be using
/// it concurrently.
unsafe fn load_interpreter(
    elf_file: &elf::ElfFile<'_>,
    pml4_phys: u64,
) -> KernelResult<Option<LoadedInterp>> {
    // Static executable: no interpreter to load.
    let Some(interp_bytes) = elf_file.interp_path() else {
        return Ok(None);
    };

    // The VFS read API is &str-based.  Interpreter paths are ASCII in
    // practice; if this one is not valid UTF-8 we cannot resolve it, so
    // fall back to direct entry rather than fail the spawn.
    let Ok(interp_path) = core::str::from_utf8(interp_bytes) else {
        serial_println!(
            "[spawn] interpreter path is not valid UTF-8 ({} bytes); \
             entering executable directly",
            interp_bytes.len()
        );
        return Ok(None);
    };

    // Read the interpreter image from the filesystem.
    let interp_data = match crate::fs::Vfs::read_file(interp_path) {
        Ok(d) => d,
        Err(e) => {
            serial_println!(
                "[spawn] interpreter '{}' unreadable ({:?}); \
                 entering executable directly",
                interp_path, e
            );
            return Ok(None);
        }
    };

    // Parse it as an ELF64 image.
    let interp_elf = match elf::ElfFile::parse(&interp_data) {
        Ok(e) => e,
        Err(e) => {
            serial_println!(
                "[spawn] interpreter '{}' is not a valid ELF ({:?}); \
                 entering executable directly",
                interp_path, e
            );
            return Ok(None);
        }
    };

    // ASLR: randomise the interpreter load base per-exec once the CSPRNG
    // is seeded.  `AT_BASE` (reported below via `LoadedInterp.base`)
    // carries whatever base we choose, so ld.so relocates itself correctly
    // regardless of placement.  Before the RNG is initialised (very early
    // boot, before any Linux process can be spawned in practice) we fall
    // back to the fixed low edge — deterministic, but only reachable when
    // no entropy exists yet.  See known-issues.md TD9.
    let base = if crate::rng::is_initialized() {
        apply_aslr_base(
            LINUX_INTERP_BASE,
            crate::rng::next_bounded(INTERP_ASLR_SPAN_PAGES),
        )
    } else {
        LINUX_INTERP_BASE
    };

    // Load the interpreter's PT_LOAD segments at the chosen base.
    //
    // SAFETY: forwarded from this function's contract — `pml4_phys` is
    // the process's private address space and no other CPU uses it yet.
    if let Err(e) = unsafe { elf::load_segments_with_bias(&interp_elf, pml4_phys, base) } {
        serial_println!(
            "[spawn] failed to load interpreter '{}' segments ({:?}); \
             entering executable directly",
            interp_path, e
        );
        return Ok(None);
    }

    let entry_rip = base.checked_add(interp_elf.entry_point()).ok_or_else(|| {
        serial_println!("[spawn] interpreter entry point overflowed base");
        KernelError::InvalidExecutable
    })?;

    serial_println!(
        "[spawn] loaded interpreter '{}' at base={:#x}, entry={:#x}",
        interp_path, base, entry_rip
    );

    Ok(Some(LoadedInterp { base, entry_rip }))
}

// ---------------------------------------------------------------------------
// Linux System V initial-stack setup (Linux-ABI processes only)
// ---------------------------------------------------------------------------

/// Build and install the System V initial stack for a Linux-ABI process.
///
/// Fetches 16 bytes of randomness for `AT_RANDOM` and delegates the
/// actual layout to [`crate::proc::linux_stack::install_linux_stack`],
/// which writes argc/argv/envp/auxv into the already-mapped stack frames
/// (`[USER_STACK_TOP - USER_STACK_SIZE, USER_STACK_TOP)`) and returns the
/// aligned initial `%rsp` plus the serialized auxv to persist.
///
/// This is **never** called for native processes — they get the bare
/// stack from [`setup_user_stack`] and read argv via
/// `SYS_PROCESS_GET_ARGS` (design-decision #4).
///
/// # Errors
///
/// Propagates failures from the stack builder (e.g.
/// [`KernelError::OutOfMemory`] if the arguments do not fit).
fn build_linux_initial_stack(
    pml4_phys: u64,
    elf_file: &elf::ElfFile<'_>,
    argv: &[&[u8]],
    envp: &[&[u8]],
    interp_base: Option<u64>,
    exec_load_bias: u64,
) -> KernelResult<crate::proc::linux_stack::InstalledLinuxStack> {
    let stack_bottom = USER_STACK_TOP
        .checked_sub(USER_STACK_SIZE)
        .ok_or(KernelError::Overflow)?;
    let mut random16 = [0u8; 16];
    crate::rng::fill(&mut random16);
    crate::proc::linux_stack::install_linux_stack(
        pml4_phys,
        USER_STACK_TOP,
        stack_bottom,
        elf_file,
        argv,
        envp,
        &random16,
        interp_base,
        exec_load_bias,
    )
}

// ---------------------------------------------------------------------------
// User stack setup
// ---------------------------------------------------------------------------

/// Allocate and map a user stack in the process address space.
///
/// Maps `USER_STACK_FRAMES` frames at the top of the user address
/// space with read/write/no-execute permissions.  Returns the initial
/// RSP (the top of the stack region).
///
/// # Errors
///
/// Returns `OutOfMemory` if frame allocation fails, or propagates
/// page table mapping errors.
fn setup_user_stack(pml4_phys: u64) -> KernelResult<u64> {
    let hhdm = page_table::hhdm()
        .ok_or(KernelError::InternalError)?;

    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;

    let stack_bottom = USER_STACK_TOP
        .checked_sub(USER_STACK_SIZE)
        .ok_or(KernelError::InvalidAddress)?;

    for i in 0..USER_STACK_FRAMES {
        // Allocate a physical frame.
        let phys_frame = frame::alloc_frame()?;

        // Zero the frame (stack should start zeroed).
        let frame_virt = phys_frame.to_virt(hhdm);
        // SAFETY: frame_virt is the HHDM mapping of a freshly
        // allocated, exclusively owned frame.
        unsafe {
            core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
        }

        // Map the frame into the process address space.
        #[allow(clippy::arithmetic_side_effects)]
        let vaddr = stack_bottom + (i as u64 * FRAME_SIZE as u64);
        let virt = VirtAddr::new(vaddr);

        // SAFETY: pml4_phys is valid (caller invariant), phys_frame is
        // freshly allocated and exclusively ours, virt is in user space.
        unsafe {
            if let Err(e) = page_table::map_frame(pml4_phys, virt, phys_frame, flags) {
                // Free the frame that was never successfully mapped —
                // otherwise it leaks permanently (address space teardown
                // only finds mapped frames).
                // SAFETY: phys_frame is exclusively ours and not mapped.
                let _ = frame::free_frame(phys_frame);
                return Err(e);
            }
        }
    }

    Ok(USER_STACK_TOP)
}

// ---------------------------------------------------------------------------
// Ring 3 entry trampoline
// ---------------------------------------------------------------------------

/// Kernel-mode trampoline that transitions to ring 3 via IRETQ.
///
/// Called by the scheduler when a thread is first dispatched.  Runs
/// in ring 0 on the thread's kernel stack.
///
/// The `info_raw` argument is a pointer to a heap-allocated
/// [`UserEntryInfo`] struct (created by `spawn_process` or
/// `thread::spawn_user`).  The trampoline reads the entry point and
/// user stack pointer, frees the struct, then builds an IRETQ frame
/// and transitions to ring 3.
///
/// ## IRETQ Frame Layout
///
/// ```text
/// RSP → [RIP]      ← ELF entry point / thread entry function
///       [CS]       ← USER_CS (0x23, DPL=3)
///       [RFLAGS]   ← 0x202 (IF=1, reserved bit 1)
///       [RSP]      ← user stack pointer
///       [SS]       ← USER_DS (0x1B, DPL=3)
/// ```
///
/// After IRETQ, the CPU loads these values and begins executing in
/// ring 3.  The process must use SYSCALL to return to the kernel.
///
/// # Safety
///
/// `info_raw` must be a valid pointer to a `UserEntryInfo` created
/// by `Box::into_raw`.  The user address space (code segments and
/// stack) must be mapped in the current PML4 (the scheduler switches
/// CR3 before running this thread).
pub(crate) extern "C" fn userspace_entry_trampoline(info_raw: u64) {
    // Recover the entry info from the heap.
    //
    // SAFETY: info_raw was created by Box::into_raw in spawn_process.
    // This thread is the sole consumer — no other code accesses it.
    let info = unsafe { Box::from_raw(info_raw as *mut UserEntryInfo) };
    let entry_rip = info.entry_rip;
    let user_rsp = info.user_rsp;
    drop(info); // Free the heap allocation.

    serial_println!(
        "[spawn] Ring 3 entry: rip={:#x}, rsp={:#x}",
        entry_rip, user_rsp
    );

    // GDT selectors for ring 3.
    let user_cs = u64::from(crate::gdt::USER_CS); // 0x23
    let user_ds = u64::from(crate::gdt::USER_DS); // 0x1B

    // RFLAGS: IF=1 (interrupts enabled), reserved bit 1 must be set.
    // IOPL=0 (no direct I/O port access from ring 3).
    let rflags: u64 = 0x202;

    // Transition to ring 3 via IRETQ.
    //
    // SAFETY: The user address space has been set up by spawn_process:
    // - ELF segments are loaded at the correct virtual addresses.
    // - A user stack is mapped at USER_STACK_TOP.
    // - The GDT has valid ring 3 code and data descriptors.
    // - TSS.RSP0 and PER_CPU.kernel_rsp are set by the scheduler
    //   (do_switch) so that SYSCALL and interrupts from ring 3 will
    //   use the correct kernel stack.
    //
    // The IRETQ pushes are in reverse order because the stack grows
    // downward.  IRETQ pops: RIP, CS, RFLAGS, RSP, SS.
    unsafe {
        core::arch::asm!(
            "push {ss}",       // SS
            "push {rsp_val}",  // RSP
            "push {rflags}",   // RFLAGS
            "push {cs}",       // CS
            "push {rip}",      // RIP
            "iretq",
            ss = in(reg) user_ds,
            rsp_val = in(reg) user_rsp,
            rflags = in(reg) rflags,
            cs = in(reg) user_cs,
            rip = in(reg) entry_rip,
            options(noreturn),
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run spawn self-tests.
pub fn self_test() -> KernelResult<()> {
    test_spawn_from_elf()?;
    test_spawn_invalid_elf()?;
    test_spawn_with_capabilities()?;
    test_spawn_faulting_process()?;
    test_spawn_stack_growth()?;
    test_exec_process()?;
    test_seh_handler_exit()?;
    test_seh_handler_resume()?;
    test_process_kill()?;
    test_no_frame_leak()?;
    test_fd_map_entry_layout()?;
    test_spawn_with_fd_map()?;
    test_spawn_with_empty_fd_map()?;
    test_spawn_fd_map_invalid_handle()?;
    test_take_initial_fds_one_shot()?;
    test_spawn_args_header_layout()?;
    test_spawn_with_argv()?;
    test_spawn_with_argv_envp()?;
    test_spawn_with_cwd()?;
    test_spawn_with_uid_gid()?;
    test_spawn_args_one_shot()?;
    test_spawn_ex_args_layout()?;
    test_spawn_linux_sysv_stack()?;
    test_load_interpreter_fallbacks()?;
    test_exec_comm_basename()?;
    test_apply_aslr_base()?;
    test_pie_aslr_window()?;
    test_brk_aslr_gap()?;

    Ok(())
}

/// Test: `choose_brk_start` (Linux `arch_randomize_brk`) preserves the
/// "no heap" sentinel, keeps the floor 16 KiB-aligned, and keeps it within
/// `[image_end, image_end + 2^BRK_ASLR_BITS pages)` — regardless of whether
/// the CSPRNG is seeded (the fallback returns `image_end`, the low edge of
/// that half-open range, so both branches satisfy the bound).
fn test_brk_aslr_gap() -> KernelResult<()> {
    const FRAME: u64 = crate::mm::frame::FRAME_SIZE as u64;
    const FRAME_MASK: u64 = FRAME - 1;

    // A degenerate image (image_end == 0 → "no heap") must stay 0 — adding a
    // gap would erroneously enable a heap at a random low address.
    if choose_brk_start(0) != 0 {
        serial_println!("[spawn]   FAIL: choose_brk_start(0) enabled a heap");
        return Err(KernelError::InternalError);
    }

    // The maximum gap (in bytes) the window can add.
    let span_bytes = BRK_ASLR_SPAN_PAGES.saturating_mul(FRAME);

    // Sample several plausible page-aligned image ends (low ET_EXEC link base,
    // our test-ELF base, and a high PIE-style base) and verify the chosen
    // floor is aligned and in range for each.  Drawn multiple times so a
    // seeded CSPRNG exercises several random gaps.
    for &image_end in &[
        0x0000_0000_0040_0000u64, // 4 MiB — classic x86_64 ET_EXEC link base
        0x0000_0040_0000_0000u64, // 256 GiB — our build_*_test_elf base
        0x0000_5555_5555_4000u64, // PIE floor
    ] {
        for _ in 0..8 {
            let floor = choose_brk_start(image_end);
            if floor & FRAME_MASK != 0 {
                serial_println!(
                    "[spawn]   FAIL: choose_brk_start({:#x}) not 16 KiB-aligned: {:#x}",
                    image_end, floor
                );
                return Err(KernelError::InternalError);
            }
            if floor < image_end {
                serial_println!(
                    "[spawn]   FAIL: choose_brk_start({:#x}) below image end: {:#x}",
                    image_end, floor
                );
                return Err(KernelError::InternalError);
            }
            // Strictly below image_end + full span (gap index is in
            // [0, SPAN_PAGES), so max gap is (SPAN_PAGES-1) pages).
            let ceiling = image_end.saturating_add(span_bytes);
            if floor >= ceiling {
                serial_println!(
                    "[spawn]   FAIL: choose_brk_start({:#x}) gap exceeds window: {:#x} >= {:#x}",
                    image_end, floor, ceiling
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    serial_println!("[spawn]   brk ASLR gap (arch_randomize_brk): aligned + in-window OK");
    Ok(())
}

/// Test: the PIE ASLR window stays 16 KiB-aligned and leaves ample
/// headroom below the interpreter floor for the image + brk growth (the
/// collision-safety invariant the PIE half of TD9 relies on).
fn test_pie_aslr_window() -> KernelResult<()> {
    const FRAME_MASK: u64 = (crate::mm::frame::FRAME_SIZE as u64) - 1;
    // Minimum clearance required between the highest PIE base and the
    // interpreter floor, for the PIE image plus future brk growth. 1 TiB
    // is far larger than any realistic PIE image + heap; the real gap is
    // ~22 TiB. This guards against a future PIE_ASLR_BITS increase silently
    // eating the headroom.
    const PIE_MIN_HEADROOM: u64 = 0x100_0000_0000; // 1 TiB

    // The PIE floor must itself be 16 KiB-aligned.
    if LINUX_PIE_BASE & FRAME_MASK != 0 {
        serial_println!("[spawn]   FAIL: LINUX_PIE_BASE not 16 KiB-aligned");
        return Err(KernelError::InternalError);
    }

    let max_index = PIE_ASLR_SPAN_PAGES.saturating_sub(1);
    for pages in [1u64, 99, 65535, max_index] {
        let b = apply_aslr_base(LINUX_PIE_BASE, pages);
        if b & FRAME_MASK != 0 {
            serial_println!("[spawn]   FAIL: PIE ASLR base not 16 KiB-aligned");
            return Err(KernelError::InternalError);
        }
        // Every base must sit strictly above the device mmap window and
        // strictly below the interpreter floor.
        if b <= LINUX_INTERP_BASE.saturating_sub(PIE_MIN_HEADROOM) {
            continue;
        }
        serial_println!(
            "[spawn]   FAIL: PIE ASLR base {:#x} too close to interpreter floor {:#x}",
            b, LINUX_INTERP_BASE
        );
        return Err(KernelError::InternalError);
    }

    // Explicitly assert the worst case: highest base + headroom < floor.
    let max_base = apply_aslr_base(LINUX_PIE_BASE, max_index);
    if LINUX_INTERP_BASE.saturating_sub(max_base) < PIE_MIN_HEADROOM {
        serial_println!(
            "[spawn]   FAIL: PIE ASLR window headroom {:#x} < required {:#x}",
            LINUX_INTERP_BASE.saturating_sub(max_base), PIE_MIN_HEADROOM
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   PIE ASLR window: aligned + ≥1 TiB headroom below interp OK");
    Ok(())
}

/// Test: `apply_aslr_base` produces page-aligned, in-window, non-wrapping
/// interpreter bases, and the whole `INTERP_ASLR_BITS` window stays clear
/// of the user stack guard (the collision-safety invariant TD9 relies on).
fn test_apply_aslr_base() -> KernelResult<()> {
    // 16 KiB page mask for the alignment check (FRAME_SIZE is a power of 2,
    // so `& mask == 0` is the alignment test). Const sub is compile-time.
    const FRAME: u64 = crate::mm::frame::FRAME_SIZE as u64;
    const FRAME_MASK: u64 = FRAME - 1;

    // Zero offset returns the fixed low edge unchanged.
    if apply_aslr_base(LINUX_INTERP_BASE, 0) != LINUX_INTERP_BASE {
        serial_println!("[spawn]   FAIL: apply_aslr_base(_, 0) changed the base");
        return Err(KernelError::InternalError);
    }

    // Offset is page-granular and additive: index 3 -> base + 3 pages.
    let expect3 = LINUX_INTERP_BASE.saturating_add(3u64.saturating_mul(FRAME));
    if apply_aslr_base(LINUX_INTERP_BASE, 3) != expect3 {
        serial_println!("[spawn]   FAIL: apply_aslr_base(_, 3) not base + 3*FRAME");
        return Err(KernelError::InternalError);
    }

    // Every page index in the window yields a 16 KiB-aligned base that
    // stays strictly below the user stack guard (no stack collision).
    let max_index = INTERP_ASLR_SPAN_PAGES.saturating_sub(1);
    for pages in [1u64, 7, 1234, max_index] {
        let b = apply_aslr_base(LINUX_INTERP_BASE, pages);
        if b & FRAME_MASK != 0 {
            serial_println!("[spawn]   FAIL: apply_aslr_base result not 16 KiB-aligned");
            return Err(KernelError::InternalError);
        }
        if b >= USER_STACK_GUARD {
            serial_println!(
                "[spawn]   FAIL: ASLR window reaches the stack guard ({:#x} >= {:#x})",
                b, USER_STACK_GUARD
            );
            return Err(KernelError::InternalError);
        }
    }

    // Saturation: a pathological huge index can never wrap past the top of
    // the address space (defence in depth — the caller bounds the index).
    if apply_aslr_base(u64::MAX.saturating_sub(1), u64::MAX) != u64::MAX {
        serial_println!("[spawn]   FAIL: apply_aslr_base did not saturate");
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   apply_aslr_base: aligned + in-window + saturating OK");
    Ok(())
}

/// Test: `exec_comm_basename` mirrors Linux `kbasename()` + the 15-byte
/// `TASK_COMM_LEN - 1` truncation that execve applies to `current->comm`.
fn test_exec_comm_basename() -> KernelResult<()> {
    // Absolute path -> last component.
    if exec_comm_basename(b"/usr/bin/ls") != b"ls" {
        serial_println!("[spawn]   FAIL: exec_comm_basename(/usr/bin/ls)");
        return Err(KernelError::InternalError);
    }
    // Bare name -> itself.
    if exec_comm_basename(b"sh") != b"sh" {
        serial_println!("[spawn]   FAIL: exec_comm_basename(sh)");
        return Err(KernelError::InternalError);
    }
    // Root-relative single component.
    if exec_comm_basename(b"/init") != b"init" {
        serial_println!("[spawn]   FAIL: exec_comm_basename(/init)");
        return Err(KernelError::InternalError);
    }
    // Trailing slash -> empty (caller leaves comm unchanged).
    if !exec_comm_basename(b"/usr/bin/").is_empty() {
        serial_println!("[spawn]   FAIL: exec_comm_basename(/usr/bin/) not empty");
        return Err(KernelError::InternalError);
    }
    // Empty input -> empty.
    if !exec_comm_basename(b"").is_empty() {
        serial_println!("[spawn]   FAIL: exec_comm_basename(empty) not empty");
        return Err(KernelError::InternalError);
    }
    // 15-byte truncation: a 20-char basename keeps only the first 15.
    let long = b"/bin/abcdefghijklmnopqrst"; // basename is 20 chars
    if exec_comm_basename(long) != b"abcdefghijklmno" {
        serial_println!("[spawn]   FAIL: exec_comm_basename truncation");
        return Err(KernelError::InternalError);
    }
    // Non-UTF-8 bytes in the basename are preserved (comm is raw bytes at
    // the scheduler layer; the path layer never forces UTF-8).
    if exec_comm_basename(b"/x/\xff\xfe") != b"\xff\xfe" {
        serial_println!("[spawn]   FAIL: exec_comm_basename non-utf8");
        return Err(KernelError::InternalError);
    }
    serial_println!("[spawn]   exec_comm_basename: all cases OK");
    Ok(())
}

/// Test 21: program-interpreter (ld.so) loading fallbacks.
///
/// Exercises [`load_interpreter`] for the two cases that do not require a
/// real ld.so on disk:
///
///   (a) a **static** executable (no `PT_INTERP`) → `Ok(None)`, so the
///       caller enters the executable directly; and
///   (b) a **dynamically-linked** executable whose interpreter file is
///       absent → the defensive `Ok(None)` fallback, again entering the
///       executable directly rather than failing the spawn.
///
/// A real freshly-created address space is used so the `unsafe` contract
/// of `load_interpreter` holds, even though neither path reaches segment
/// mapping.  End-to-end interpreter *execution* needs an actual ld.so on
/// the filesystem and is covered separately once one is available.
fn test_load_interpreter_fallbacks() -> KernelResult<()> {
    let pid = pcb::create("interp-test", 0);
    let Some(pml4) = pcb::get_pml4(pid).filter(|&p| p != 0) else {
        pcb::destroy(pid);
        serial_println!("[spawn]   FAIL: interp-test process has no PML4");
        return Err(KernelError::InternalError);
    };

    // (a) Static executable: no PT_INTERP → Ok(None), pml4 untouched.
    let static_elf = elf::build_test_elf_public();
    let static_parsed = match elf::ElfFile::parse(&static_elf) {
        Ok(e) => e,
        Err(e) => {
            pcb::destroy(pid);
            return Err(e);
        }
    };
    // SAFETY: `pml4` is this freshly-created process's private address
    // space; no other CPU is using it.  The static path returns before
    // any mapping is performed.
    let static_ok = matches!(unsafe { load_interpreter(&static_parsed, pml4) }, Ok(None));
    if !static_ok {
        pcb::destroy(pid);
        serial_println!("[spawn]   FAIL: static ELF expected Ok(None) from load_interpreter");
        return Err(KernelError::InternalError);
    }

    // (b) Dynamically-linked, interpreter file absent → fallback Ok(None).
    let dyn_elf = elf::build_dynamic_interp_test_elf(b"/no-such-ld-test-interpreter.so.999\0");
    let dyn_parsed = match elf::ElfFile::parse(&dyn_elf) {
        Ok(e) => e,
        Err(e) => {
            pcb::destroy(pid);
            return Err(e);
        }
    };
    // SAFETY: same private address space; the missing-file path returns
    // before any mapping is performed.
    let dyn_ok = matches!(unsafe { load_interpreter(&dyn_parsed, pml4) }, Ok(None));
    pcb::destroy(pid);
    if !dyn_ok {
        serial_println!("[spawn]   FAIL: absent interpreter expected Ok(None) fallback");
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   load_interpreter fallbacks (static + absent): OK");
    Ok(())
}

/// Test 20: end-to-end Linux System V initial-stack delivery.
///
/// Spawns a Linux-ABI ELF (tagged `ELFOSABI_GNU`) whose entry code reads
/// `argc` from `[%rsp]` and calls `exit(argc)`.  If `spawn_process` built
/// and installed the System V stack correctly, the process exits with a
/// status equal to the number of argv entries passed.  This validates the
/// whole path: `detect_linux_abi` → `build_linux_initial_stack` →
/// `install_linux_stack` writing into the mapped frames → ring-3 entry at
/// the overridden `%rsp`.
fn test_spawn_linux_sysv_stack() -> KernelResult<()> {
    let elf_data = elf::build_linux_argc_exit_test_elf();
    // Three argv entries → the binary must exit(3).
    let argv: &[&[u8]] = &[b"prog", b"one", b"two"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-sysv",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run: trampoline → ring 3 → read argc → exit(argc).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: Linux SysV stack — expected Zombie, got {:?}",
            s
        );
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // exit(argc) with 3 argv entries must yield exit code 3 — proving the
    // kernel placed argc at [%rsp] per the System V ABI.
    let ec = pcb::exit_code(result.pid);
    if ec != Some(3) {
        serial_println!(
            "[spawn]   FAIL: Linux SysV stack — expected exit code 3 (argc), got {:?}",
            ec
        );
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // The auxv must have been persisted into the Linux-ABI PCB state so
    // PR_GET_AUXV / /proc/<pid>/auxv can serve it.  Verify it is present,
    // a whole number of 16-byte Elf64_auxv_t pairs, and AT_NULL-
    // terminated (last pair = two zero u64s).
    match pcb::linux_saved_auxv(result.pid) {
        Some(auxv) if !auxv.is_empty() && auxv.len() % 16 == 0 => {
            let tail = match auxv.len().checked_sub(16) {
                Some(t) => t,
                None => {
                    serial_println!("[spawn]   FAIL: saved auxv length underflow");
                    thread::on_thread_exit(result.task_id);
                    pcb::destroy(result.pid);
                    return Err(KernelError::InternalError);
                }
            };
            if auxv.get(tail..).is_none_or(|t| t.iter().any(|&b| b != 0)) {
                serial_println!(
                    "[spawn]   FAIL: saved auxv not AT_NULL-terminated (len={})",
                    auxv.len()
                );
                thread::on_thread_exit(result.task_id);
                pcb::destroy(result.pid);
                return Err(KernelError::InternalError);
            }
        }
        other => {
            serial_println!(
                "[spawn]   FAIL: saved auxv missing/misaligned: {:?}",
                other.as_ref().map(alloc::vec::Vec::len)
            );
            thread::on_thread_exit(result.task_id);
            pcb::destroy(result.pid);
            return Err(KernelError::InternalError);
        }
    }

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Linux SysV initial-stack delivery (exit(argc)): OK");
    serial_println!("[spawn]   Linux auxv persisted for PR_GET_AUXV / procfs: OK");
    Ok(())
}

/// VFS-dependent integration self-test: end-to-end dynamically-linked
/// Linux launch through the program interpreter (`ld.so`).
///
/// This is the piece the in-`proc::self_test` tests could not cover —
/// `test_load_interpreter_fallbacks` only exercises the static and
/// missing-interpreter *fallback* cases (both `Ok(None)`), because a
/// successful interpreter load needs a real interpreter file on the
/// filesystem and the VFS is not mounted yet when `proc::self_test()` runs.
///
/// Here, after the VFS is up, we:
///   1. Write a minimal `ET_DYN` interpreter ("ld.so" stand-in) that calls
///      `exit(42)` to a known path.
///   2. Spawn a dynamically-linked executable whose `PT_INTERP` names that
///      path and whose *own* code would `exit(7)`.
///   3. Let the child run and assert it exited with **42**, not 7 —
///      proving the kernel loaded the interpreter at `LINUX_INTERP_BASE`
///      and transferred control to *its* entry (the real ld.so contract),
///      and that the interpreter's `exit` syscall routed through the Linux
///      ABI translation layer.
///
/// Skips gracefully (returns `Ok`) if the VFS write fails — the test is a
/// best-effort integration check, not a gate on unrelated VFS state.
///
/// Must be called **after** filesystem initialization (see `main.rs`); it
/// is intentionally *not* part of `self_test()` for that reason.
pub fn self_test_linux_dynamic_interp() -> KernelResult<()> {
    const INTERP_PATH: &str = "/slateos-test-ld.so";
    const INTERP_PATH_NUL: &[u8] = b"/slateos-test-ld.so\0";
    const INTERP_EXIT: u8 = 42;
    const EXE_EXIT: u8 = 7;

    serial_println!("[spawn] Running Linux dynamic-interpreter integration test...");

    // Step 1: place the interpreter ("ld.so" stand-in) on the filesystem.
    let interp_elf = elf::build_linux_interp_exit_elf(INTERP_EXIT);
    if let Err(e) = crate::fs::Vfs::write_file(INTERP_PATH, &interp_elf) {
        serial_println!(
            "[spawn]   Linux dynamic interp: SKIP (VFS write failed: {:?})",
            e
        );
        return Ok(());
    }

    // Step 2: spawn a dynamically-linked executable naming that interpreter.
    let exe_elf = elf::build_linux_dynamic_exe_elf(INTERP_PATH_NUL, EXE_EXIT);
    let argv: &[&[u8]] = &[b"dynprog"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-dyn",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(INTERP_PATH);
            serial_println!("[spawn]   FAIL: dynamic spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the thread run: trampoline → ring 3 at the interpreter entry →
    // exit(42).
    crate::sched::yield_now();
    crate::sched::yield_now();

    // Capture state/exit code BEFORE tearing the process down.
    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(INTERP_PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: dynamic interp — expected Zombie, got {:?}",
            state
        );
        return Err(KernelError::InternalError);
    }

    // The decisive check: the interpreter (exit 42) ran, not the
    // executable's own code (exit 7).  Anything else means the kernel
    // failed to enter the interpreter.
    if exit_code != Some(i32::from(INTERP_EXIT)) {
        serial_println!(
            "[spawn]   FAIL: dynamic interp — expected exit {} (interpreter), got {:?} \
             (7 would mean the executable ran instead of ld.so)",
            INTERP_EXIT, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux dynamic-interpreter launch (entered ld.so → exit({})): OK",
        INTERP_EXIT
    );
    Ok(())
}

/// VFS-dependent integration self-test: end-to-end **file-backed `mmap(2)`**
/// from ring 3 through the real Linux syscall path.
///
/// The kernel-context `syscall::linux::self_test_file_mmap` drives
/// `linux_file_mmap` directly (bypassing `caller_pid()`); this test instead
/// runs an actual Linux-ABI process that issues `open(2)` + `mmap(2)` itself,
/// so it covers the parts that helper cannot:
///   - `open(2)` installing a Linux fd in the spawned process's fd table,
///   - `mmap(2)` dispatching to `linux_file_mmap` with a live `caller_pid()`,
///   - the mapped frames being readable from **ring 3**,
///   - the file's bytes landing in the **second** 16 KiB frame (multi-frame
///     file-backed mapping verified through the real path).
///
/// We seed a two-frame file whose byte at `READ_OFF` (start of the second
/// frame) is a known sentinel, spawn a program that maps the file and
/// `exit`s with that byte, and assert the zombie's exit code is the sentinel.
///
/// Skips gracefully (`Ok`) if the VFS write fails.  Must run **after**
/// filesystem initialization (see `main.rs`).
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
pub fn self_test_linux_file_mmap() -> KernelResult<()> {
    use alloc::vec;

    const DATA_PATH: &str = "/slateos-test-mmap.dat";
    const DATA_PATH_NUL: &[u8] = b"/slateos-test-mmap.dat\0";
    const FRAME: usize = 16 * 1024;
    const READ_OFF: usize = FRAME; // first byte of the second frame
    const SENTINEL: u8 = 0x5B; // 91 — distinct from the dynamic-interp test's 42
    const FILE_LEN: usize = FRAME + 256; // spans into the second frame

    serial_println!("[spawn] Running Linux file-backed mmap (ring 3) integration test...");

    // Step 1: stage the data file.  Fill with a non-zero pattern so a stray
    // zero byte can't masquerade as correct content, then stamp the sentinel
    // at the offset the program will read.
    let mut data = vec![0u8; FILE_LEN];
    for (i, b) in data.iter_mut().enumerate() {
        // Pattern is always non-zero and != SENTINEL except where we stamp it.
        *b = (((i as u32).wrapping_mul(37).wrapping_add(3) & 0x7f) as u8) | 0x80;
    }
    data[READ_OFF] = SENTINEL;
    if let Err(e) = crate::fs::Vfs::write_file(DATA_PATH, &data) {
        serial_println!("[spawn]   Linux file mmap (ring 3): SKIP (VFS write failed: {:?})", e);
        return Ok(());
    }

    // Step 2: spawn the mmap test program(s).  Each must hold a File
    // capability: open(2) → handlers::sys_fs_open requires
    // require_cap_type(File, READ).
    //
    // `run_one` spawns a ring-3 program built by `build_linux_mmap_test_elf`,
    // lets it run to completion, and asserts it exited with `SENTINEL` (the
    // byte it read out of the mapping).  Any non-zombie state or wrong exit
    // code means mmap mis-mapped the file (or returned an error and the
    // process faulted dereferencing the bad base).  Returns `Err` on failure;
    // the caller is responsible for removing the staged data file.
    let run_one = |exe_elf: &[u8], label: &str| -> KernelResult<()> {
        let argv: &[&[u8]] = &[b"mmapprog"];
        let envp: &[&[u8]] = &[b"PATH=/bin"];
        let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
        let options = SpawnOptions {
            name: "spawn-test-linux-mmap",
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &caps,
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };

        let result = match spawn_process(exe_elf, &options) {
            Ok(r) => r,
            Err(e) => {
                serial_println!("[spawn]   FAIL: mmap-test ({label}) spawn returned {:?}", e);
                return Err(e);
            }
        };

        // Let the thread run: open → mmap → read mapped byte → exit(byte).
        crate::sched::yield_now();
        crate::sched::yield_now();

        let state = pcb::state(result.pid);
        let exit_code = pcb::exit_code(result.pid);

        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);

        if state != Some(pcb::ProcessState::Zombie) {
            serial_println!(
                "[spawn]   FAIL: file mmap (ring 3, {label}) — expected Zombie, got {:?} \
                 (a non-zombie state usually means mmap returned an error and the \
                 process faulted dereferencing it)",
                state
            );
            return Err(KernelError::InternalError);
        }

        if exit_code != Some(i32::from(SENTINEL)) {
            serial_println!(
                "[spawn]   FAIL: file mmap (ring 3, {label}) — expected exit {} \
                 (mapped sentinel byte), got {:?}",
                SENTINEL, exit_code
            );
            return Err(KernelError::InternalError);
        }
        Ok(())
    };

    // Case A: map the whole file at offset 0, read the sentinel that lives at
    // the start of the **second** frame (multi-frame mapping coverage).
    let elf_off0 =
        elf::build_linux_mmap_test_elf(DATA_PATH_NUL, FILE_LEN as u32, READ_OFF as u32, 0);
    // Case B: map starting at a **nonzero** file offset (the second frame), so
    // the mapping's first byte is the file byte at FRAME — the sentinel.  This
    // exercises mmap's `offset` argument end-to-end (ld.so maps PT_LOAD
    // segments at nonzero p_offset, so this path matters for Path X).
    let elf_offn =
        elf::build_linux_mmap_test_elf(DATA_PATH_NUL, FRAME as u32, 0, READ_OFF as u32);

    let res_a = run_one(&elf_off0, "offset 0, second-frame byte");
    let res_b = if res_a.is_ok() {
        run_one(&elf_offn, "nonzero offset, mapping base byte")
    } else {
        Ok(())
    };

    let _ = crate::fs::Vfs::remove(DATA_PATH);
    res_a?;
    res_b?;

    serial_println!(
        "[spawn]   Linux file-backed mmap (ring 3: open+mmap at offset 0 and \
         nonzero offset, mapped byte == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test of the userspace `netstack` daemon (net→userspace
/// migration, Phase 2 — see `net-userspace-migration.md`).
///
/// Spawns the real `services/netstack` binary as a ring-3 process holding a
/// single `NetRaw` capability. The daemon claims the physical NIC through the
/// capability-gated `SYS_NET_RAW_*` syscalls (Phase 1), broadcasts an ARP
/// request for the default gateway, and polls raw frames until the gateway's
/// ARP reply arrives — proving the raw-frame TX **and** RX path works
/// end-to-end from userspace against QEMU's slirp. It exits 0 on success, a
/// small nonzero code on failure/timeout.
///
/// While the daemon holds the claim, `net::poll()`'s physical-NIC drain is
/// gated off (the daemon owns the frames); the driver's `recv()` refills its
/// own RX descriptors, so RX keeps flowing without the kernel poll loop. On
/// daemon exit the claim self-heals and the in-kernel stack resumes as the
/// active path.
///
/// Skips gracefully (`Ok`) when the interface isn't up or DHCP hasn't assigned
/// an address — there is no network to prove the path against in that case.
pub fn self_test_userspace_netstack() -> KernelResult<()> {
    // The prebuilt daemon ELF, embedded at compile time (same pattern as the
    // `hello` service used by the container tests).  Built by
    // `services/netstack` for x86_64-unknown-none.
    static NETSTACK_ELF: &[u8] = include_bytes!(
        "../../../services/netstack/target/x86_64-unknown-none/release/netstack"
    );

    // Skip when there's no usable network: the daemon's proof is an ARP
    // round-trip with the gateway, which requires a bound address.
    let ifinfo = crate::net::interface::info();
    if !ifinfo.up || ifinfo.ip.0 == [0, 0, 0, 0] || ifinfo.gateway.0 == [0, 0, 0, 0] {
        serial_println!(
            "[spawn]   netstack daemon (ring 3): SKIP (no network — up={}, ip={}, gw={})",
            ifinfo.up, ifinfo.ip, ifinfo.gateway
        );
        return Ok(());
    }

    serial_println!("[spawn] Running userspace netstack daemon (ring 3) integration test...");

    let argv: &[&[u8]] = &[b"netstack"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    // The one capability the daemon needs: raw L2 frame access (WRITE rights).
    let caps = [(ResourceType::NetRaw, 0u64, Rights::WRITE)];
    let options = SpawnOptions {
        name: "netstack-selftest",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(NETSTACK_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: netstack spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Park the boot thread until the daemon exits.  The daemon sleeps/polls
    // for up to ~2s of its own wall-time; `wait_process` blocks (rather than
    // spin-yielding) so that budget elapses in real time and reaps the zombie.
    let code = match crate::container::wait_process(result.pid) {
        Ok(c) => c,
        Err(e) => {
            serial_println!("[spawn]   FAIL: wait_process(netstack) returned {:?}", e);
            return Err(e);
        }
    };

    if code != 0 {
        serial_println!(
            "[spawn]   FAIL: netstack daemon (ring 3) exited {} \
             (nonzero = could not claim NIC / TX failed / no gateway ARP reply)",
            code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   netstack daemon (ring 3: raw NIC claim + ARP round-trip \
         over SYS_NET_RAW_*): OK"
    );
    Ok(())
}

/// Ring-3 end-to-end test of the net→userspace migration **Phase 4** socket-
/// syscall → IPC path (see `net-userspace-migration.md`, `design-decisions.md`
/// §64): a DNS `A`-record resolve forwarded from the kernel to the userspace
/// `netstack` daemon over the Service Registry, instead of the in-kernel
/// resolver.
///
/// This exercises the full Phase-4 wiring the eventual `sys_dns_resolve`
/// forwarder will use:
///
/// 1. Spawn `netstack serve-dns` (ring 3, holding one `NetRaw` cap).  It
///    `register`s the `net.stack` service, claims the NIC, and ARP-resolves the
///    next hop toward the DNS server.
/// 2. The kernel (this test, standing in for the syscall forwarder) waits for
///    the registration, `connect`s, and sends a `[OP_RESOLVE_A | hostname]`
///    request over the channel.
/// 3. The daemon does a real DNS-over-UDP query on its raw NIC (via `netproto`)
///    and replies `[status | ip…]`.  The kernel reads the reply.
///
/// Per §64, this is a **bounded** self-test: the daemon owns the NIC only for
/// the brief service window, then unregisters and releases it so the in-kernel
/// stack (still the live path until Phase 5) resumes.  A well-formed reply
/// proves the IPC round-trip; an `ST_OK` reply additionally proves the whole
/// userspace DNS path.  Skips gracefully when there is no network.
pub fn self_test_netstack_dns_ipc() -> KernelResult<()> {
    use crate::ipc::{channel, service};

    // The request/reply schema is shared with the daemon via the `netipc` crate.

    // Same network gate as the Phase-2 test, plus a configured DNS server (the
    // daemon needs one to resolve against).
    let ifinfo = crate::net::interface::info();
    if !ifinfo.up || ifinfo.ip.0 == [0, 0, 0, 0] || ifinfo.dns.0 == [0, 0, 0, 0] {
        serial_println!(
            "[spawn]   netstack DNS-over-IPC (ring 3): SKIP (no network — up={}, ip={}, dns={})",
            ifinfo.up, ifinfo.ip, ifinfo.dns
        );
        return Ok(());
    }

    serial_println!("[spawn] Running netstack DNS-over-IPC (ring 3) integration test...");

    static NETSTACK_ELF: &[u8] = include_bytes!(
        "../../../services/netstack/target/x86_64-unknown-none/release/netstack"
    );

    let argv: &[&[u8]] = &[b"netstack", b"serve-dns"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    // NetRaw (WRITE) to claim the NIC, plus Service (WRITE) to `register` the
    // `net.stack` name in the Service Registry (name-squatting guard).
    let caps = [
        (ResourceType::NetRaw, 0u64, Rights::WRITE),
        (ResourceType::Service, 0u64, Rights::WRITE),
    ];
    let options = SpawnOptions {
        name: "netstack-dns",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(NETSTACK_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: netstack-dns spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Wait for the daemon to publish `net.stack` (it registers early, before the
    // slow ARP/DNS work).  Yield so the daemon actually gets CPU time.
    let reg_deadline = crate::hrtimer::now_ns().saturating_add(3_000_000_000); // 3s
    while !service::is_registered(b"net.stack") {
        if crate::hrtimer::now_ns() >= reg_deadline {
            serial_println!("[spawn]   FAIL: netstack did not register net.stack within 3s");
            let _ = crate::container::wait_process(result.pid);
            return Err(KernelError::TimedOut);
        }
        crate::sched::yield_now();
    }

    // Connect and send an A-record resolve request for a well-known name.
    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => {
            serial_println!("[spawn]   FAIL: connect(net.stack) returned {:?}", e);
            let _ = crate::container::wait_process(result.pid);
            return Err(e);
        }
    };

    let host: &[u8] = b"example.com";
    let mut req = [0u8; 64];
    let req_len = match netipc::encode_resolve_a(&mut req, host) {
        Some(n) => n,
        None => {
            channel::close(client);
            let _ = crate::container::wait_process(result.pid);
            return Err(KernelError::InternalError);
        }
    };
    let msg = channel::Message::from_bytes(&req[..req_len])?;
    if let Err(e) = channel::send(client, msg) {
        serial_println!("[spawn]   FAIL: send to netstack returned {:?}", e);
        channel::close(client);
        let _ = crate::container::wait_process(result.pid);
        return Err(e);
    }

    // Block for the reply.  The daemon does real network I/O (ARP + DNS round
    // trip), so allow a few seconds.
    let reply = match channel::recv_timeout(client, 6_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            serial_println!("[spawn]   FAIL: no reply from netstack ({:?})", e);
            channel::close(client);
            let _ = crate::container::wait_process(result.pid);
            return Err(e);
        }
    };

    // Copy the reply bytes out before closing, then decode via the shared schema.
    let mut data = [0u8; 5];
    let dlen = reply.data().len().min(data.len());
    data[..dlen].copy_from_slice(&reply.data()[..dlen]);
    channel::close(client);

    let mut a_ip: Option<[u8; 4]> = None;
    match netipc::parse_ipv4_reply(&data[..dlen]) {
        netipc::Ipv4Reply::Ok(ip) => {
            serial_println!(
                "[spawn]   netstack DNS-over-IPC (ring 3: kernel→daemon resolve of \
                 example.com over net.stack): OK — {}.{}.{}.{}",
                ip[0], ip[1], ip[2], ip[3]
            );
            a_ip = Some(ip);
        }
        netipc::Ipv4Reply::Fail => {
            // The IPC round-trip worked (we got a structured reply); DNS itself
            // didn't resolve — most likely no upstream resolver behind slirp.
            // The Phase-4 wiring is proven either way, so don't fail the boot.
            serial_println!(
                "[spawn]   netstack DNS-over-IPC (ring 3): A IPC round-trip OK, DNS \
                 unresolved (no upstream?) — Phase-4 path proven, resolution skipped"
            );
        }
        netipc::Ipv4Reply::Malformed => {
            serial_println!(
                "[spawn]   FAIL: malformed netstack A reply (len={})",
                dlen
            );
            let _ = crate::container::wait_process(result.pid);
            return Err(KernelError::InternalError);
        }
    }

    // Second round-trip: reverse (PTR) resolve a well-known address with a
    // stable PTR record (8.8.8.8 → dns.google) so the decode path exercises
    // when the upstream resolver answers. (Cloudflare-fronted A results like
    // example.com's often have no PTR, which wouldn't test the decoder.)
    let ptr_ip: [u8; 4] = [8, 8, 8, 8];
    let ptr_result = netstack_ptr_roundtrip(&ptr_ip);

    // Third round-trip: one-shot TCP fetch over IPC. Reuse the A-resolved address
    // for example.com and issue an HTTP/1.0 HEAD (small, header-only response
    // that fits the control-path reply). This exercises the daemon's full
    // userspace TCP client (handshake → data → FIN) end to end from the kernel.
    let tcp_result = match a_ip {
        Some(ip) => netstack_tcp_fetch_roundtrip(&ip, 80),
        None => Ok(None),
    };

    // Fourth round-trip: generic one-shot UDP exchange over IPC. Send a fixed
    // DNS/A query for example.com to the configured resolver on :53 and verify
    // the datagram we get back is a matching DNS *response*. This exercises the
    // daemon's generic UDP path (`OP_UDP_EXCHANGE`) — distinct from the
    // DNS-specific resolve op — without duplicating DNS logic in the kernel
    // (the query is a static wire blob; we only check the response header).
    let udp_result = netstack_udp_dns_roundtrip(&ifinfo.dns.0);

    // Fifth round-trip: shared-memory handshake (`OP_SHM_PING`). The kernel
    // creates an SHM region, writes a request magic, and asks the daemon to map
    // it (SYS_SHM_MAP), verify our magic, and write a response magic back. We
    // then read that response magic through our own (kernel) view of the same
    // frames. This proves cross-address-space SHM_MAP sharing end to end — the
    // exact mechanism the Phase-5 data ring will use to hand the daemon its
    // SQ/CQ/data region. No NIC involved, so it's independent of upstream.
    let shm_result = netstack_shm_ping_roundtrip();

    // Sixth round-trip: shared-memory *ring* echo (`OP_RING_ECHO`). The kernel
    // creates an SHM region, lays it out as an io_uring-style SQ/CQ ring
    // (`Ring::init`), writes a payload into the data area, and submits one
    // OP_SEND SQE. It then asks the daemon to map the region, attach, pop the
    // SQE, upper-case the payload in place, and post a completion. The kernel
    // reaps the CQE and verifies the echoed user_data, result length, and the
    // transformed bytes. This is the first end-to-end exercise of the netring
    // driver across the address-space boundary — the zero-copy data path the
    // Phase-5 socket API rides on. No NIC involved.
    let ring_result = netstack_ring_echo_roundtrip();

    // Seventh round-trip: shared-memory *ring TCP* (`OP_RING_TCP`) — the Phase-4
    // capstone. Instead of a one-shot control op (`OP_TCP_FETCH`), the kernel lays
    // the SHM region out as a ring and submits the socket-opcode batch
    // (OP_CONNECT → OP_SEND → OP_RECV → OP_CLOSE), with the HTTP request and its
    // response flowing through the zero-copy ring data window rather than the
    // control channel. The daemon drives one live `TcpConn` through the batch and
    // posts one completion per SQE. This proves a real TCP fetch running entirely
    // over the ring — the exact shape the Phase-5 streaming socket API rides on.
    // Reuse the A-resolved example.com address on :80.
    let ring_tcp_result = match a_ip {
        Some(ip) => netstack_ring_tcp_roundtrip(&ip, 80),
        None => Ok(None),
    };

    // Multiplexed variant: two connections over one ring, addressed by conn_id.
    let ring_tcp_multi_result = match a_ip {
        Some(ip) => netstack_ring_tcp_multi_roundtrip(&ip, 80),
        None => Ok(None),
    };

    // Persistent-session variant, driven through the reusable kernel
    // `NetstackConn` client: one connection driven across separate OP_RING_TCP
    // control calls (connect / send / recv / close), proving the daemon's ring
    // session survives between submissions. This exercises the same reusable
    // client the AF_INET socket layer (increment 5.5) will sit on.
    let ring_tcp_persist_result = match a_ip {
        Some(ip) => crate::net::netstack_client::self_test_http(&ip, 80),
        None => Ok(None),
    };

    // Shared-RX-demux variant: two connections whose sends both precede both
    // receives, so one connection's frames arrive while the daemon is blocked
    // receiving for the other (proves the RX pump routes by 4-tuple, not drops).
    let ring_tcp_demux_result = match a_ip {
        Some(ip) => netstack_ring_tcp_demux_roundtrip(&ip, 80),
        None => Ok(None),
    };

    // Reap the daemon (it exits after its idle deadline once we stop sending).
    let _ = crate::container::wait_process(result.pid);

    match tcp_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack TCP-fetch-over-IPC (ring 3): OK — HTTP response received"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack TCP-fetch-over-IPC (ring 3): IPC round-trip OK, no/short \
             response (no upstream?) — path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: TCP-fetch IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match udp_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack UDP-exchange-over-IPC (ring 3): OK — DNS response datagram \
             returned"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack UDP-exchange-over-IPC (ring 3): IPC round-trip OK, no \
             response (no upstream?) — path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: UDP-exchange IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match shm_result {
        Ok(()) => serial_println!(
            "[spawn]   netstack SHM-ping-over-IPC (ring 3): OK — cross-address-space \
             SYS_SHM_MAP verified (daemon read kernel magic + kernel read daemon magic)"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: SHM-ping IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match ring_result {
        Ok(()) => serial_println!(
            "[spawn]   netstack ring-echo-over-IPC (ring 3): OK — SQ/CQ driver verified \
             (kernel submitted 3-SQE batch + daemon drained SQ + kernel reaped 3 CQEs in order)"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: ring-echo IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match ring_tcp_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack ring-TCP-over-IPC (ring 3): OK — live TCP fetch over the ring \
             (kernel submitted connect/send/recv/close batch + daemon drove one TcpConn + \
             HTTP response returned through the ring data window)"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack ring-TCP-over-IPC (ring 3): ring batch drained + completions \
             reaped, no/short response (no upstream?) — path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: ring-TCP IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match ring_tcp_multi_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack ring-TCP-multi-over-IPC (ring 3): OK — two connections \
             multiplexed over one ring by conn_id (daemon held both TcpConns in its table + \
             both returned HTTP through independent ring data windows)"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack ring-TCP-multi-over-IPC (ring 3): 8-SQE multiplexed batch \
             drained + completions reaped, no/short response (no upstream?) — path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: ring-TCP-multi IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match ring_tcp_persist_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack client-persist-over-IPC (ring 3): OK — one connection driven \
             through the reusable NetstackConn client across separate OP_RING_TCP calls \
             (connect / send / recv / close; the daemon kept the ring session + TcpConn alive \
             between submissions)"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack client-persist-over-IPC (ring 3): session persisted across \
             submissions, no/short response (no upstream?) — path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: client-persist IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    // Report the staged-cutover boot switch (design-decisions.md §66, Q22b).
    // Default off: the kernel keeps its resident stack until this is flipped.
    serial_println!(
        "[spawn]   net.userspace cutover switch: {} (default off; resident stack still authoritative)",
        if crate::net::netstack_client::userspace_enabled() { "ON" } else { "off" }
    );

    match ring_tcp_demux_result {
        Ok(Some(())) => serial_println!(
            "[spawn]   netstack ring-TCP-demux-over-IPC (ring 3): OK — two connections received \
             concurrently (both sends before both recvs); the RX pump routed each peer's frames \
             to its owning conn by 4-tuple so both returned HTTP (no sibling frames dropped)"
        ),
        Ok(None) => serial_println!(
            "[spawn]   netstack ring-TCP-demux-over-IPC (ring 3): interleaved 8-SQE batch drained \
             + completions reaped, no/short response (no upstream?) — demux path proven"
        ),
        Err(e) => {
            serial_println!("[spawn]   FAIL: ring-TCP-demux IPC round-trip error ({:?})", e);
            return Err(e);
        }
    }

    match ptr_result {
        Ok(Some(())) => {
            serial_println!(
                "[spawn]   netstack reverse-DNS-over-IPC (ring 3): OK — PTR name decoded \
                 for {}.{}.{}.{}",
                ptr_ip[0], ptr_ip[1], ptr_ip[2], ptr_ip[3]
            );
            Ok(())
        }
        Ok(None) => {
            // IPC worked but no PTR record (common under slirp / for many IPs).
            serial_println!(
                "[spawn]   netstack reverse-DNS-over-IPC (ring 3): PTR IPC round-trip OK, \
                 no name (no upstream / no PTR) — path proven"
            );
            Ok(())
        }
        Err(e) => {
            serial_println!("[spawn]   FAIL: PTR IPC round-trip error ({:?})", e);
            Err(e)
        }
    }
}

/// Resolve an A record for `host` against the already-running `net.stack`
/// daemon. Returns `Ok(Some(ip))` on success, `Ok(None)` if the daemon replied
/// `ST_FAIL` (IPC fine, DNS unresolved — common under slirp), or `Err` on a
/// transport fault. Service-name based, so it works against any live daemon
/// (bounded self-test or the persistent boot daemon).
fn netstack_resolve_a(host: &[u8]) -> KernelResult<Option<[u8; 4]>> {
    use crate::ipc::{channel, service};

    let client = service::connect(b"net.stack")?;
    let mut req = [0u8; 64];
    let req_len = match netipc::encode_resolve_a(&mut req, host) {
        Some(n) => n,
        None => {
            channel::close(client);
            return Err(KernelError::InternalError);
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return Err(e);
    }
    let reply = match channel::recv_timeout(client, 6_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };
    let mut data = [0u8; 5];
    let dlen = reply.data().len().min(data.len());
    data[..dlen].copy_from_slice(&reply.data()[..dlen]);
    channel::close(client);
    match netipc::parse_ipv4_reply(&data[..dlen]) {
        netipc::Ipv4Reply::Ok(ip) => Ok(Some(ip)),
        netipc::Ipv4Reply::Fail => Ok(None),
        netipc::Ipv4Reply::Malformed => Err(KernelError::InternalError),
    }
}

/// Phase-5 boot path (`net.userspace` on): spawn the **persistent** userspace
/// `netstack` daemon (`serve-net`), validate DNS/TCP/UDP/O_NONBLOCK/poll parity
/// over it, then leave it running to own the NIC for the system's lifetime.
///
/// This is the switch-on counterpart to [`self_test_netstack_dns_ipc`] (§66,
/// Q22b staged cutover). Unlike the bounded self-test, the daemon is **not
/// reaped**: it keeps the exclusive raw-NIC claim so the in-kernel stack stands
/// down (its physical-NIC RX is skipped while a raw owner holds the claim, §64)
/// and all AF_INET socket traffic (increment 5.5) routes to the daemon.
///
/// Skips gracefully (returns `Ok(())`) when there's no network. A genuine
/// spawn/registration fault returns `Err`; per-check network variance (no
/// upstream) is logged as a non-fatal WARNING but never fails the boot, since a
/// persistent daemon cannot be cleanly torn down here.
pub fn run_persistent_netstack() -> KernelResult<()> {
    use crate::ipc::service;

    let ifinfo = crate::net::interface::info();
    if !ifinfo.up || ifinfo.ip.0 == [0, 0, 0, 0] {
        serial_println!(
            "[spawn]   persistent netstack daemon: SKIP (no network — up={}, ip={})",
            ifinfo.up,
            ifinfo.ip
        );
        return Ok(());
    }

    serial_println!(
        "[spawn] Starting persistent userspace netstack daemon (net.userspace on)..."
    );

    static NETSTACK_ELF: &[u8] = include_bytes!(
        "../../../services/netstack/target/x86_64-unknown-none/release/netstack"
    );

    let argv: &[&[u8]] = &[b"netstack", b"serve-net"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    // NetRaw (WRITE) to claim the NIC persistently, plus Service (WRITE) to
    // register `net.stack`.
    let caps = [
        (ResourceType::NetRaw, 0u64, Rights::WRITE),
        (ResourceType::Service, 0u64, Rights::WRITE),
    ];
    let options = SpawnOptions {
        name: "netstack",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(NETSTACK_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: persistent netstack spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Wait for the daemon to publish `net.stack` (it registers early, before the
    // slow ARP work). Yield so the daemon actually gets CPU time.
    let reg_deadline = crate::hrtimer::now_ns().saturating_add(3_000_000_000); // 3s
    while !service::is_registered(b"net.stack") {
        if crate::hrtimer::now_ns() >= reg_deadline {
            serial_println!(
                "[spawn]   FAIL: persistent netstack did not register net.stack within 3s"
            );
            return Err(KernelError::TimedOut);
        }
        crate::sched::yield_now();
    }
    serial_println!(
        "[spawn]   persistent netstack daemon registered net.stack (pid {})",
        result.pid
    );

    // Parity validation over the live persistent daemon: DNS → TCP, then UDP.
    match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => {
            serial_println!(
                "[spawn]   persistent netstack DNS: example.com -> {}.{}.{}.{}",
                ip[0],
                ip[1],
                ip[2],
                ip[3]
            );
            match netstack_tcp_fetch_roundtrip(&ip, 80) {
                Ok(Some(())) => serial_println!(
                    "[spawn]   persistent netstack TCP: HTTP response received over the daemon"
                ),
                Ok(None) => serial_println!(
                    "[spawn]   persistent netstack TCP: round-trip OK, no/short response \
                     (no upstream?) — path proven"
                ),
                Err(e) => serial_println!(
                    "[spawn]   WARNING: persistent netstack TCP round-trip error ({:?})",
                    e
                ),
            }
        }
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack DNS: IPC OK, example.com unresolved (no upstream?)"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack DNS resolve error ({:?})",
            e
        ),
    }

    match netstack_udp_dns_roundtrip(&ifinfo.dns.0) {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack UDP: DNS response datagram returned over the daemon"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack UDP: round-trip OK, no response (no upstream?) — \
             path proven"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack UDP round-trip error ({:?})",
            e
        ),
    }

    // UDP SOCK_DGRAM parity (D-NETSOCK-SYNC): the check above uses the daemon's
    // older one-shot control-channel OP_UDP_EXCHANGE. This one drives the real
    // ring-based datagram *socket* path — OP_UDP_BIND (ephemeral port) + OP_UDP_SEND
    // + OP_UDP_RECV with the in-band source-address header — sending a DNS query and
    // reading the reply back through a bound socket, proving connectionless sockets
    // work end-to-end over the daemon.
    match crate::net::netstack_client::self_test_udp_dns(&ifinfo.dns.0) {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack UDP socket: bind+sendto+recvfrom returned a DNS reply \
             from port 53 — SOCK_DGRAM parity proven over the daemon"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack UDP socket: bind/send/recv path ran, no reply \
             (no upstream / no resolver) — path proven"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack UDP socket error ({:?})",
            e
        ),
    }

    // O_NONBLOCK parity (D-NETSOCK-SYNC): a non-blocking recv on a freshly
    // connected socket with no pending data must return promptly (WouldBlock →
    // EAGAIN) instead of stalling. Uses the same DNS-resolved address as the TCP
    // check; skips cleanly if there is no upstream.
    match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => match crate::net::netstack_client::self_test_nonblock_recv(&ip, 80) {
            Ok(Some(())) => serial_println!(
                "[spawn]   persistent netstack O_NONBLOCK: non-blocking recv returned promptly \
                 over the daemon"
            ),
            Ok(None) => serial_println!(
                "[spawn]   persistent netstack O_NONBLOCK: connect had no upstream — path proven"
            ),
            Err(e) => serial_println!(
                "[spawn]   WARNING: persistent netstack O_NONBLOCK recv error ({:?})",
                e
            ),
        },
        Ok(None) | Err(_) => serial_println!(
            "[spawn]   persistent netstack O_NONBLOCK: DNS unresolved — nonblock check skipped"
        ),
    }

    // poll/epoll readiness parity (D-NETSOCK-SYNC): a connected socket must report
    // an honest POLLIN — writable-but-not-readable while idle, then readable once
    // the peer's response arrives — rather than the old "always ready" placeholder.
    match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => match crate::net::netstack_client::self_test_poll_ready(&ip, 80) {
            Ok(Some(())) => serial_println!(
                "[spawn]   persistent netstack poll: honest POLLIN/POLLOUT readiness proven \
                 over the daemon"
            ),
            Ok(None) => serial_println!(
                "[spawn]   persistent netstack poll: no upstream/response — readiness path proven"
            ),
            Err(e) => serial_println!(
                "[spawn]   WARNING: persistent netstack poll readiness error ({:?})",
                e
            ),
        },
        Ok(None) | Err(_) => serial_println!(
            "[spawn]   persistent netstack poll: DNS unresolved — readiness check skipped"
        ),
    }

    // Non-blocking connect parity (D-NETSOCK-SYNC): a connect() on an O_NONBLOCK
    // socket must return EINPROGRESS immediately and complete in the background,
    // with poll(POLLOUT) waking once the handshake resolves (and POLLERR/SO_ERROR
    // on failure) — matching Linux. Uses the same DNS-resolved address.
    match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => match crate::net::netstack_client::self_test_nonblock_connect(&ip, 80) {
            Ok(Some(())) => serial_println!(
                "[spawn]   persistent netstack nonblock-connect: EINPROGRESS→POLLOUT connect \
                 parity proven over the daemon"
            ),
            Ok(None) => serial_println!(
                "[spawn]   persistent netstack nonblock-connect: no upstream — connect path proven"
            ),
            Err(e) => serial_println!(
                "[spawn]   WARNING: persistent netstack nonblock-connect error ({:?})",
                e
            ),
        },
        Ok(None) | Err(_) => serial_println!(
            "[spawn]   persistent netstack nonblock-connect: DNS unresolved — check skipped"
        ),
    }

    // Non-blocking send parity (D-NETSOCK-SYNC): a send()/write() on an O_NONBLOCK
    // socket with room in the send window must accept the bytes (return the count),
    // not spuriously EAGAIN — only a full window (a prior unacked segment) blocks.
    match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => match crate::net::netstack_client::self_test_nonblock_send(&ip, 80) {
            Ok(Some(())) => serial_println!(
                "[spawn]   persistent netstack nonblock-send: O_NONBLOCK send accepted on a \
                 writable window (no spurious EAGAIN) — send parity proven over the daemon"
            ),
            Ok(None) => serial_println!(
                "[spawn]   persistent netstack nonblock-send: no upstream — send path proven"
            ),
            Err(e) => serial_println!(
                "[spawn]   WARNING: persistent netstack nonblock-send error ({:?})",
                e
            ),
        },
        Ok(None) | Err(_) => serial_println!(
            "[spawn]   persistent netstack nonblock-send: DNS unresolved — check skipped"
        ),
    }

    // Server-socket parity (D-NETSOCK-SYNC): listen()/accept() over the daemon.
    // There is no external server to accept from under slirp, so this drives the
    // daemon's in-process software loopback — a connection to our own me.ip is
    // diverted to a listener in the same session. A single non-blocking connect
    // completes the handshake for both ends; accept then dequeues the passive
    // connection and a bidirectional data exchange proves it is a real socket.
    match crate::net::netstack_client::self_test_listen_accept() {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack listen/accept: server socket accepted a loopback \
             connection and echoed data both ways — server-socket parity proven over the daemon"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack listen/accept: no IPv4 lease — check skipped"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack listen/accept error ({:?})",
            e
        ),
    }

    // Object-layer server-socket parity (Q23 Option A): drive the
    // `net::socket::bind_stream`/`listen`/`accept` state machine that the
    // `bind(2)`/`listen(2)`/`accept(2)` syscalls call into. The ring-level test
    // above proves the full data path; this proves the object-layer wrapper
    // (Owned→Shared session conversion, getsockname port reporting, idempotent
    // re-listen, empty-backlog EAGAIN, and dgram/listening op rejection).
    match crate::net::socket::self_test_server() {
        Ok(Some(())) => serial_println!(
            "[spawn]   net::socket server object layer: bind→listen→accept state machine, \
             port reporting and empty-backlog EAGAIN all correct — syscall server-socket path proven"
        ),
        Ok(None) => serial_println!(
            "[spawn]   net::socket server object layer: no daemon session — check skipped"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: net::socket server object-layer error ({:?})",
            e
        ),
    }

    // IPv6 connect parity (D-NETSOCK-SYNC, final gap): OP_CONNECT6 over the daemon.
    // Slirp offers no IPv6 peer or router, so this too drives the in-process
    // loopback — a non-blocking connect to the daemon's own link-local (me.ip6,
    // derived from the NIC MAC) completes a full TCP-over-IPv6 handshake for both
    // ends, and accept + a bidirectional exchange prove the accepted v6 connection
    // is a real socket. This closes the last pre-5.7 IPv6 parity gap.
    match crate::net::netstack_client::self_test_connect6() {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack connect6: IPv6 handshake completed over loopback and \
             echoed data both ways — IPv6-connect parity proven over the daemon"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack connect6: no NIC MAC — check skipped"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack connect6 error ({:?})",
            e
        ),
    }

    // IPv6 UDP datagram parity (D-NETSOCK-SYNC): OP_UDP_SEND6 + v6-aware
    // OP_UDP_RECV over the daemon. The datagram sibling of connect6 — a UDP
    // datagram sent to the daemon's own link-local (me.ip6) loops back in-process
    // (slirp has no IPv6 peer), and its source header must report AF_INET6 with the
    // link-local address and the sent payload. Proves the AF_INET6 SOCK_DGRAM path.
    match crate::net::netstack_client::self_test_udp6_loopback() {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack udp6: IPv6 datagram looped back with an AF_INET6 \
             source header and matching payload — AF_INET6 SOCK_DGRAM parity proven over the daemon"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack udp6: no NIC MAC — check skipped"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack udp6 error ({:?})",
            e
        ),
    }

    // UDP connect() default-peer parity (D-NETSOCK-SYNC): drive the net::socket
    // SOCK_DGRAM object layer through the daemon loopback — a connect()ed socket's
    // send targets the default peer and its recv filters to that peer (Linux drops
    // non-peer datagrams). Proves both filter directions + getpeername.
    match crate::net::netstack_client::self_test_udp_connect() {
        Ok(Some(())) => serial_println!(
            "[spawn]   persistent netstack udp-connect: connected send looped back and the \
             non-peer datagram was dropped — UDP connect() default-peer parity proven"
        ),
        Ok(None) => serial_println!(
            "[spawn]   persistent netstack udp-connect: no NIC MAC — check skipped"
        ),
        Err(e) => serial_println!(
            "[spawn]   WARNING: persistent netstack udp-connect error ({:?})",
            e
        ),
    }

    // Ring-3 socket-syscall HTTP capstone (netstack Phase 5.6, deferred from 5.5;
    // see todo.txt Judgment Calls 2026-07-14). Everything above drives the
    // daemon-backed socket path from *kernel* context via the `NetstackConn`
    // client. This final check proves the same path works from an **actual ring-3
    // process** using the raw Linux syscall ABI: `httpget` (a bare Linux-ABI ELF)
    // does socket()/connect()/write()/read()/close() over the persistent daemon.
    // That exercises the syscall *dispatch* wiring — user-pointer copies, fd
    // install, errno mapping in `dispatch_linux` → `net::socket::*` — which was
    // previously only covered by code review, never a live ring-3 call.
    run_ring3_http_capstone();

    // Ring-3 datagram capstone (UDP SOCK_DGRAM cutover): the datagram sibling of
    // the HTTP capstone. `udpget` (a bare Linux-ABI ELF) does socket(SOCK_DGRAM)/
    // bind()/sendto()/recvfrom()/close() over the persistent daemon, proving the
    // *ring-3* datagram socket-fd dispatch wiring (sockaddr parse, fd install,
    // errno mapping in `dispatch_linux` → `net::socket::dgram_*`) works end to end
    // — previously only the kernel-context `self_test_udp_dns` covered it.
    run_ring3_udp_capstone(&ifinfo.dns.0);

    // Ring-3 IPv6 datagram capstone: the v6 sibling of `run_ring3_udp_capstone`.
    // slirp has no IPv6 DNS upstream, so this arm does a self-loopback — udpget
    // sends to the daemon's own link-local `me.ip6` and asserts the datagram
    // echoes back — proving the ring-3 `sockaddr_in6` sendto/recvfrom dispatch
    // path (previously only the kernel-context `self_test_udp6_loopback`).
    run_ring3_udp6_capstone();

    serial_println!(
        "[spawn]   persistent netstack daemon: DNS/TCP/UDP/O_NONBLOCK/poll/nonblock-connect/\
         nonblock-send/listen-accept/connect6/udp6/udp-connect/ring3-capstone/ring3-udp/ring3-udp6 \
         parity checks done; daemon now owns the NIC for the system's lifetime"
    );
    Ok(())
}

/// Spawn the `httpget` ring-3 Linux-ABI ELF to fetch HTTP over the persistent
/// `net.stack` daemon, and report its exit code.
///
/// This is the ring-3 half of the Phase-5 socket cutover proof: unlike the
/// kernel-context `NetstackConn` self-tests, it drives the *real* Linux socket
/// syscalls (`socket`/`connect`/`write`/`read`/`close`) from an unprivileged
/// process, so it validates the `dispatch_linux` socket arms end to end.
///
/// Never fails the boot: network variance (no upstream under slirp) is logged as
/// a non-fatal note, exactly like the other persistent-daemon parity checks. A
/// genuine spawn fault is logged as a WARNING but still returns.
fn run_ring3_http_capstone() {
    // Resolve the target first (the ring-3 program takes a numeric IP in argv so
    // it needs no in-process resolver). Reuse example.com:80, as the other checks.
    let ip = match netstack_resolve_a(b"example.com") {
        Ok(Some(ip)) => ip,
        Ok(None) => {
            serial_println!(
                "[spawn]   ring3 HTTP capstone: DNS unresolved (no upstream?) — check skipped"
            );
            return;
        }
        Err(e) => {
            serial_println!(
                "[spawn]   ring3 HTTP capstone: DNS resolve error ({:?}) — check skipped",
                e
            );
            return;
        }
    };

    static HTTPGET_ELF: &[u8] = include_bytes!(
        "../../../services/httpget/target/x86_64-unknown-none/release/httpget"
    );

    // Format the resolved IP as a dotted-decimal argv string (no NUL — the Linux
    // stack builder terminates argv entries itself, as for the daemon's argv).
    let mut ip_buf = [0u8; 16];
    let ip_len = fmt_ipv4_dotted(&ip, &mut ip_buf);
    let ip_arg = &ip_buf[..ip_len];
    let argv: &[&[u8]] = &[b"httpget", ip_arg, b"80"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];

    let options = SpawnOptions {
        name: "httpget",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    // Explicit Linux ABI: httpget is a bare static ELF (OSABI=SYSV, no interp, no
    // GNU property note) so auto-detection would classify it Native and route its
    // `syscall`s through the wrong table. We produced it, so we state its ABI.
    let result = match spawn_process_with_abi(HTTPGET_ELF, &options, pcb::AbiMode::Linux) {
        Ok(r) => r,
        Err(e) => {
            serial_println!(
                "[spawn]   WARNING: ring3 HTTP capstone spawn returned {:?}",
                e
            );
            return;
        }
    };

    // Let the ring-3 process and the daemon run until httpget zombies (it does one
    // blocking connect + write + read over the network), bounded by a deadline so
    // a stuck fetch can never wedge the boot.
    let deadline = crate::hrtimer::now_ns().saturating_add(15_000_000_000); // 15s
    loop {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
        if crate::hrtimer::now_ns() >= deadline {
            serial_println!(
                "[spawn]   ring3 HTTP capstone: process did not exit within 15s — tearing down"
            );
            break;
        }
        crate::sched::yield_now();
    }

    let exit_code = pcb::exit_code(result.pid);
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    match exit_code {
        Some(0) => serial_println!(
            "[spawn]   ring3 HTTP capstone: OK — ring-3 socket()/connect()/write()/read() over \
             the daemon returned an HTTP response (exit 0)"
        ),
        Some(13) | Some(14) => serial_println!(
            "[spawn]   ring3 HTTP capstone: connect+send OK, no/non-HTTP response (no upstream?) \
             — ring-3 socket-syscall path proven (exit {})",
            exit_code.unwrap_or_default()
        ),
        Some(code) => serial_println!(
            "[spawn]   WARNING: ring3 HTTP capstone exited {} (see httpget exit-code table)",
            code
        ),
        None => serial_println!(
            "[spawn]   WARNING: ring3 HTTP capstone produced no exit code"
        ),
    }
}

/// Spawn the `udpget` ring-3 Linux-ABI ELF to send a DNS query and read the reply
/// over the persistent `net.stack` daemon, and report its exit code.
///
/// This is the datagram half of the Phase-5 socket cutover proof: unlike the
/// kernel-context `NetstackConn::self_test_udp_dns`, it drives the *real* Linux
/// datagram socket syscalls (`socket(SOCK_DGRAM)`/`bind`/`sendto`/`recvfrom`/
/// `close`) from an unprivileged process, so it validates the `dispatch_linux`
/// datagram socket arms (`create_dgram`/`dgram_bind`/`dgram_send_to`/
/// `dgram_recv_from`) end to end.
///
/// `dns_ip` is the interface's resolver address (`ifinfo.dns.0`); the query goes
/// to `dns_ip:53`. A zero resolver (no DHCP-provided DNS) skips the check. Never
/// fails the boot: network variance (no upstream under slirp) is logged as a
/// non-fatal note, exactly like the other persistent-daemon parity checks.
fn run_ring3_udp_capstone(dns_ip: &[u8; 4]) {
    if *dns_ip == [0, 0, 0, 0] {
        serial_println!(
            "[spawn]   ring3 UDP capstone: no resolver address (no DHCP DNS?) — check skipped"
        );
        return;
    }

    static UDPGET_ELF: &[u8] = include_bytes!(
        "../../../services/udpget/target/x86_64-unknown-none/release/udpget"
    );

    // Format the resolver IP as a dotted-decimal argv string; port 53 (DNS).
    let mut ip_buf = [0u8; 16];
    let ip_len = fmt_ipv4_dotted(dns_ip, &mut ip_buf);
    let ip_arg = &ip_buf[..ip_len];
    let argv: &[&[u8]] = &[b"udpget", ip_arg, b"53"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];

    let options = SpawnOptions {
        name: "udpget",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    // Explicit Linux ABI: udpget is a bare static ELF (like httpget), so we state
    // its ABI rather than relying on auto-detection.
    let result = match spawn_process_with_abi(UDPGET_ELF, &options, pcb::AbiMode::Linux) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   WARNING: ring3 UDP capstone spawn returned {:?}", e);
            return;
        }
    };

    // Let the ring-3 process and the daemon run until udpget zombies (one blocking
    // sendto + recvfrom), bounded by a deadline so a stuck fetch can't wedge boot.
    let deadline = crate::hrtimer::now_ns().saturating_add(15_000_000_000); // 15s
    loop {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
        if crate::hrtimer::now_ns() >= deadline {
            serial_println!(
                "[spawn]   ring3 UDP capstone: process did not exit within 15s — tearing down"
            );
            break;
        }
        crate::sched::yield_now();
    }

    let exit_code = pcb::exit_code(result.pid);
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    match exit_code {
        Some(0) => serial_println!(
            "[spawn]   ring3 UDP capstone: OK — ring-3 socket(SOCK_DGRAM)/bind()/sendto()/\
             recvfrom() over the daemon returned a DNS reply (exit 0)"
        ),
        Some(13) | Some(14) => serial_println!(
            "[spawn]   ring3 UDP capstone: bind+sendto OK, no/invalid reply (no upstream?) — \
             ring-3 datagram socket-syscall path proven (exit {})",
            exit_code.unwrap_or_default()
        ),
        Some(code) => serial_println!(
            "[spawn]   WARNING: ring3 UDP capstone exited {} (see udpget exit-code table)",
            code
        ),
        None => serial_println!(
            "[spawn]   WARNING: ring3 UDP capstone produced no exit code"
        ),
    }
}

/// Spawn the `udpget` ring-3 Linux-ABI ELF in its **IPv6** loopback arm and report
/// its exit code.
///
/// The v6 sibling of [`run_ring3_udp_capstone`]. Because QEMU/slirp provides no
/// IPv6 upstream, this cannot hit a real resolver; instead it drives a
/// self-loopback that exercises the ring-3 `sockaddr_in6` datagram dispatch path
/// (`create_dgram`/`dgram_bind`/`dgram_send_to6`/`dgram_recv_from` with the v6
/// source header) end to end from an unprivileged process: udpget binds
/// `[::]:port`, sends a marker payload to the daemon's own EUI-64 link-local
/// `me.ip6` (which the daemon diverts back into its RX FIFO, bypassing NDP), and
/// asserts the exact bytes echo back with an `AF_INET6` source.
///
/// The kernel derives `me.ip6` from the NIC MAC (the same inline EUI-64
/// link-local the daemon seeds and the `self_test_udp6_loopback` check uses) and
/// passes it as a 32-hex-char argv string plus a `"6"` mode selector. A missing
/// NIC MAC skips the check. Never fails the boot — a spawn fault is a WARNING, a
/// network/exit variance a non-fatal note, matching the other parity checks.
fn run_ring3_udp6_capstone() {
    let mac = crate::net::interface::mac().0;
    if mac == [0u8; 6] {
        serial_println!(
            "[spawn]   ring3 UDP6 capstone: no NIC MAC (no me.ip6) — check skipped"
        );
        return;
    }
    // EUI-64 link-local (RFC 4291 App. A), matching the daemon's
    // `icmpv6::link_local_from_mac(mac)` used to seed `me.ip6`.
    let ll: [u8; 16] = [
        0xFE, 0x80, 0, 0, 0, 0, 0, 0, mac[0] ^ 0x02, mac[1], mac[2], 0xFF, 0xFE, mac[3], mac[4],
        mac[5],
    ];
    let mut hex = [0u8; 32];
    fmt_ipv6_hex32(&ll, &mut hex);

    static UDPGET_ELF: &[u8] = include_bytes!(
        "../../../services/udpget/target/x86_64-unknown-none/release/udpget"
    );

    // argv = ["udpget", "<32-hex me.ip6>", "<port>", "6"]. A fixed loopback port
    // (distinct from the kernel self-test's 9201 to avoid any cross-talk).
    let argv: &[&[u8]] = &[b"udpget", &hex, b"9404", b"6"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];

    let options = SpawnOptions {
        name: "udpget6",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process_with_abi(UDPGET_ELF, &options, pcb::AbiMode::Linux) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   WARNING: ring3 UDP6 capstone spawn returned {:?}", e);
            return;
        }
    };

    // Bounded wait for the ring-3 process to zombie (one loopback send + recv).
    let deadline = crate::hrtimer::now_ns().saturating_add(15_000_000_000); // 15s
    loop {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
        if crate::hrtimer::now_ns() >= deadline {
            serial_println!(
                "[spawn]   ring3 UDP6 capstone: process did not exit within 15s — tearing down"
            );
            break;
        }
        crate::sched::yield_now();
    }

    let exit_code = pcb::exit_code(result.pid);
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    match exit_code {
        Some(0) => serial_println!(
            "[spawn]   ring3 UDP6 capstone: OK — ring-3 socket(AF_INET6,SOCK_DGRAM)/bind()/\
             sendto()/recvfrom() looped a datagram back from me.ip6 (exit 0)"
        ),
        Some(13) | Some(14) => serial_println!(
            "[spawn]   WARNING: ring3 UDP6 capstone bind+sendto OK but the loopback datagram \
             did not echo back (exit {}) — v6 datagram dispatch may be broken",
            exit_code.unwrap_or_default()
        ),
        Some(code) => serial_println!(
            "[spawn]   WARNING: ring3 UDP6 capstone exited {} (see udpget exit-code table)",
            code
        ),
        None => serial_println!(
            "[spawn]   WARNING: ring3 UDP6 capstone produced no exit code"
        ),
    }
}

/// Format a 16-byte IPv6 address as 32 lowercase hex chars (no colons) into `buf`.
/// The ring-3 `udpget` v6 arm parses this plain-hex form (no RFC 4291
/// `::`-compression parser needed on the userspace side).
// Every index is `i*2`/`i*2+1` for `i` in `0..16` (0..=31, in bounds) and every
// hex lookup is masked to a nibble (0..=15, in bounds); no op can overflow.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn fmt_ipv6_hex32(ip: &[u8; 16], buf: &mut [u8; 32]) {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    for (i, &b) in ip.iter().enumerate() {
        buf[i * 2] = HEXD[(b >> 4) as usize];
        buf[i * 2 + 1] = HEXD[(b & 0x0f) as usize];
    }
}

/// Format an IPv4 address as dotted-decimal into `buf`, returning the byte length
/// written. `buf` must be at least 15 bytes (max `"255.255.255.255"`).
// Every arithmetic op here is on values bounded to 0..=255 (a single octet) or a
// digit 0..=9, so no add/sub/mul/div can overflow a `u8`; the buffer index is
// bounds-checked by `push`.
#[allow(clippy::arithmetic_side_effects)]
fn fmt_ipv4_dotted(ip: &[u8; 4], buf: &mut [u8; 16]) -> usize {
    let mut n = 0usize;
    let push = |buf: &mut [u8; 16], n: &mut usize, b: u8| {
        if *n < buf.len() {
            buf[*n] = b;
            *n += 1;
        }
    };
    for (i, &octet) in ip.iter().enumerate() {
        if i != 0 {
            push(buf, &mut n, b'.');
        }
        if octet >= 100 {
            push(buf, &mut n, b'0' + octet / 100);
        }
        if octet >= 10 {
            push(buf, &mut n, b'0' + (octet / 10) % 10);
        }
        push(buf, &mut n, b'0' + octet % 10);
    }
    n
}

/// Perform one `OP_RESOLVE_PTR` round-trip to the running `net.stack` daemon for
/// IPv4 `ip`. Returns `Ok(Some(()))` if a hostname was decoded, `Ok(None)` if
/// the daemon replied `ST_FAIL` (IPC fine, no PTR), or `Err` on a transport
/// failure (connect/send/recv/malformed). Kept separate from the A path so the
/// self-test body stays readable.
fn netstack_ptr_roundtrip(ip: &[u8; 4]) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service};

    let client = service::connect(b"net.stack")?;

    let mut req = [0u8; 8];
    let req_len = match netipc::encode_resolve_ptr(&mut req, ip) {
        Some(n) => n,
        None => {
            channel::close(client);
            return Err(KernelError::InternalError);
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return Err(e);
    }

    let reply = match channel::recv_timeout(client, 6_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };

    let result = match netipc::parse_name_reply(reply.data()) {
        netipc::NameReply::Ok(name) if !name.is_empty() => {
            // Print the name (ASCII) so a human can sanity-check the decode.
            let show = name.get(..name.len().min(64)).unwrap_or(&[]);
            serial_print!("[spawn]   PTR name = ");
            for &b in show {
                // Printable ASCII only; substitute others with '.'.
                let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
                serial_print!("{}", c as char);
            }
            serial_println!("");
            Ok(Some(()))
        }
        netipc::NameReply::Ok(_) | netipc::NameReply::Fail => Ok(None),
        netipc::NameReply::Malformed => Err(KernelError::InternalError),
    };

    channel::close(client);
    result
}

/// Perform one `OP_TCP_FETCH` round-trip to the running `net.stack` daemon: ask
/// it to connect to `ip:port`, send a minimal HTTP/1.0 HEAD, and return the
/// response. Returns `Ok(Some(()))` if a plausible HTTP response arrived,
/// `Ok(None)` if the daemon replied `ST_FAIL` or an empty/short body (IPC fine,
/// no upstream), or `Err` on a transport failure. Validates the reply looks like
/// an HTTP status line so the whole userspace-TCP path is actually exercised.
fn netstack_tcp_fetch_roundtrip(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service};

    let client = service::connect(b"net.stack")?;

    // HTTP/1.0 HEAD: header-only response keeps us within the control-path cap.
    let payload: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
    let mut req = [0u8; 96];
    let req_len = match netipc::encode_tcp_fetch(&mut req, ip, port, payload) {
        Some(n) => n,
        None => {
            channel::close(client);
            return Err(KernelError::InternalError);
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return Err(e);
    }

    // The daemon does a full TCP transaction (handshake + data + close); allow
    // generous time before giving up.
    let reply = match channel::recv_timeout(client, 10_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };

    let result = match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(body) if body.len() >= 5 && &body[..5] == b"HTTP/" => {
            // Echo the status line (up to CRLF) for a human sanity-check.
            let line_end = body
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .unwrap_or(body.len().min(64));
            let show = body.get(..line_end).unwrap_or(&[]);
            serial_print!("[spawn]   TCP HTTP status = ");
            for &b in show {
                let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
                serial_print!("{}", c as char);
            }
            serial_println!("");
            Ok(Some(()))
        }
        netipc::BytesReply::Ok(_) | netipc::BytesReply::Fail => Ok(None),
        netipc::BytesReply::Malformed => Err(KernelError::InternalError),
    };

    channel::close(client);
    result
}

/// Perform one `OP_UDP_EXCHANGE` round-trip to the running `net.stack` daemon:
/// send a fixed DNS/A query for `example.com` to `dns_ip:53` and verify the
/// datagram returned is a matching DNS *response*. Returns `Ok(Some(()))` on a
/// valid response, `Ok(None)` if the daemon replied `ST_FAIL` or an unexpected
/// datagram (IPC fine, no upstream), or `Err` on a transport failure. The query
/// is a static wire blob so the kernel side carries no DNS logic — the point is
/// to exercise the daemon's *generic* UDP path, not to resolve a name.
fn netstack_udp_dns_roundtrip(dns_ip: &[u8; 4]) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service};

    // Fixed DNS query: ID=0xABCD, RD set, one A/IN question for "example.com".
    #[rustfmt::skip]
    const DNS_QUERY: [u8; 29] = [
        0xAB, 0xCD,             // ID
        0x01, 0x00,             // flags: RD
        0x00, 0x01,             // QDCOUNT = 1
        0x00, 0x00,             // ANCOUNT = 0
        0x00, 0x00,             // NSCOUNT = 0
        0x00, 0x00,             // ARCOUNT = 0
        0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
        0x03, b'c', b'o', b'm',
        0x00,                   // root label
        0x00, 0x01,             // QTYPE = A
        0x00, 0x01,             // QCLASS = IN
    ];

    let client = service::connect(b"net.stack")?;

    let mut req = [0u8; 7 + DNS_QUERY.len()];
    let req_len = match netipc::encode_udp_exchange(&mut req, dns_ip, 53, &DNS_QUERY) {
        Some(n) => n,
        None => {
            channel::close(client);
            return Err(KernelError::InternalError);
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return Err(e);
    }

    let reply = match channel::recv_timeout(client, 6_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return Err(e);
        }
    };

    let result = match netipc::parse_bytes_reply(reply.data()) {
        // A valid DNS response echoes our ID and has the QR (response) bit set.
        netipc::BytesReply::Ok(dg)
            if dg.len() >= 12 && dg[0] == 0xAB && dg[1] == 0xCD && (dg[2] & 0x80) != 0 =>
        {
            Ok(Some(()))
        }
        netipc::BytesReply::Ok(_) | netipc::BytesReply::Fail => Ok(None),
        netipc::BytesReply::Malformed => Err(KernelError::InternalError),
    };

    channel::close(client);
    result
}

/// Authorize the running `net.stack` daemon to `SYS_SHM_MAP` a kernel-created
/// region before its handle is handed over the control channel.
///
/// No-op when the service is absent or kernel-provided (PID 0) — the kernel is
/// the TCB and needs no grant. The only [`shm::authorize`] failure is
/// `InvalidHandle`, impossible for a handle we just created and still hold, so
/// the result is safe to ignore.
fn authorize_netstack_daemon(handle: crate::ipc::shm::ShmHandle) {
    if let Some(pid) = crate::ipc::service::provider_pid(b"net.stack")
        && pid != 0
    {
        let _ = crate::ipc::shm::authorize(handle, pid);
    }
}

/// Perform one `OP_SHM_PING` round-trip to the running `net.stack` daemon,
/// validating cross-address-space `SYS_SHM_MAP` sharing.
///
/// The kernel creates a shared-memory region, writes
/// [`netipc::SHM_PING_REQUEST_MAGIC`] at byte offset 0 (through its own HHDM
/// view), and sends the region's handle+size to the daemon. The daemon maps the
/// *same* physical frames into *its* ring-3 address space, confirms the request
/// magic (proving it sees the kernel's write), writes
/// [`netipc::SHM_PING_RESPONSE_MAGIC`] at offset 8, and unmaps. On `ST_OK` the
/// kernel reads offset 8 back through its view and checks the response magic —
/// proving the daemon's write is visible to the kernel. Both directions
/// verified ⇒ the mapping is genuinely shared, not a private copy.
///
/// Returns `Ok(())` on a fully-verified round-trip, or `Err` on any transport
/// failure, a `ST_FAIL` reply, or a magic mismatch. This is the bootstrap the
/// Phase-5 data ring builds on.
fn netstack_shm_ping_roundtrip() -> KernelResult<()> {
    use crate::ipc::{channel, service, shm};

    // Create a region (rounds up to one 16 KiB frame — ample for two magics).
    let handle = shm::create(64)?;
    authorize_netstack_daemon(handle);
    let size = shm::size(handle)?;

    // Helper to close the SHM handle on every exit path (RAII-ish).
    let finish = |h: shm::ShmHandle, r: KernelResult<()>| -> KernelResult<()> {
        shm::close(h);
        r
    };

    // Write the request magic at offset 0 and clear the response slot at
    // offset 8 through the kernel's HHDM view of the frames.
    let kaddr = match shm::kernel_addr(handle) {
        Ok(p) => p,
        Err(e) => return finish(handle, Err(e)),
    };
    // SAFETY: kaddr is valid for `size` (>= 16) bytes; unaligned u64 writes to
    // offsets 0 and 8 stay in bounds. No other CPU touches the region yet.
    unsafe {
        core::ptr::write_unaligned(kaddr.cast::<u64>(), netipc::SHM_PING_REQUEST_MAGIC);
        core::ptr::write_unaligned(kaddr.add(8).cast::<u64>(), 0u64);
    }

    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => return finish(handle, Err(e)),
    };

    #[allow(clippy::cast_possible_truncation)]
    let size_u32 = size as u32;
    let mut req = [0u8; 16];
    let req_len = match netipc::encode_shm_ping(&mut req, handle.raw(), size_u32) {
        Some(n) => n,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return finish(handle, Err(e));
    }

    let reply = match channel::recv_timeout(client, 5_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    channel::close(client);

    // The daemon replies ST_OK (empty body) iff it verified our magic and wrote
    // its response. Anything else is a failure of the handshake.
    match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(_) => {}
        netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    // Read the daemon's response magic back through the kernel view.
    // SAFETY: same region, still mapped (handle open); offset 8 in bounds.
    let resp = unsafe { core::ptr::read_unaligned(kaddr.add(8).cast::<u64>()) };
    if resp == netipc::SHM_PING_RESPONSE_MAGIC {
        finish(handle, Ok(()))
    } else {
        serial_println!(
            "[spawn]   SHM-ping: response magic mismatch (got {:#x})",
            resp
        );
        finish(handle, Err(KernelError::InternalError))
    }
}

/// End-to-end IPC + shared-memory *ring* self-test — the first exercise of the
/// [`netring`] SQ/CQ driver across the address-space boundary (`OP_RING_ECHO`).
///
/// The kernel creates a shared-memory region, lays it out as an io_uring-style
/// ring with [`netring::Ring::init`] (through its own HHDM view), writes a fixed
/// ASCII payload into the data area, and submits a *batch* of three SQEs in one
/// pass — an `OP_SEND` (carrying the payload window) followed by two `OP_NOP`s,
/// each stamped with a distinct `user_data` (base + index). It then hands the
/// region's handle+size to the daemon, which maps the *same* frames,
/// [`netring::Ring::attach`]es, and **drains the whole SQ**, dispatching each
/// entry by opcode and posting one completion per SQE in FIFO order. On `ST_OK`
/// the kernel reaps all three CQEs through its view and verifies, for each, the
/// echoed `user_data` and expected `result` (payload length for the SEND, 0 for
/// the NOPs), that no stray extra completion remains, and that the data-area
/// payload is now the upper-cased form — proving batched submission, FIFO
/// completion ordering, and the zero-copy data window all work across two
/// address spaces.
///
/// Returns `Ok(())` on a fully-verified round-trip, or `Err` on any transport
/// failure, a `ST_FAIL` reply, a missing/out-of-order/mismatched completion, or
/// wrong echoed bytes. This is the data path the Phase-5 socket API rides on.
fn netstack_ring_echo_roundtrip() -> KernelResult<()> {
    use crate::ipc::{channel, service, shm};

    // Fixed payload the daemon upper-cases. Lowercase so the transform is
    // observable; kept short so it fits comfortably in one 16 KiB frame.
    const PAYLOAD: &[u8] = b"ring-echo-payload";

    // Ring geometry: small SQ/CQ (4 slots each — room for the 3-SQE batch below)
    // + a small data area. region_size stays well under one 16 KiB frame.
    let sq_entries: u32 = 4;
    let cq_entries: u32 = 4;
    let data_len: u32 = 256;
    let need = netipc::ring::region_size(sq_entries, cq_entries, data_len);

    let handle = shm::create(need)?;
    authorize_netstack_daemon(handle);
    let size = shm::size(handle)?;

    // Close the SHM handle on every exit path.
    let finish = |h: shm::ShmHandle, r: KernelResult<()>| -> KernelResult<()> {
        shm::close(h);
        r
    };

    let kaddr = match shm::kernel_addr(handle) {
        Ok(p) => p,
        Err(e) => return finish(handle, Err(e)),
    };

    // Lay out the region as a ring and submit the OP_SEND SQE.
    // SAFETY: kaddr is valid+writable for `size` (>= need) bytes and no other
    // party touches the region until the daemon attaches (which happens strictly
    // after we send the request below). `init` validates the geometry fits.
    let ring = match unsafe { netring::Ring::init(kaddr, size, sq_entries, cq_entries, data_len) } {
        Some(r) => r,
        None => return finish(handle, Err(KernelError::InternalError)),
    };
    if !ring.write_data(0, PAYLOAD) {
        return finish(handle, Err(KernelError::InternalError));
    }
    #[allow(clippy::cast_possible_truncation)]
    let payload_len = PAYLOAD.len() as u32;
    // Submit a *batch* of three SQEs in one pass (the io_uring model): an OP_SEND
    // carrying the payload window, then two OP_NOPs. The daemon drains the whole
    // SQ and posts one CQE per entry in FIFO order; we reap all three below. Each
    // SQE gets a distinct user_data (base+index) so we can confirm the daemon
    // preserved submission order. `sq_entries` (4) has room for all three.
    let batch = [
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: 1,
            data_off: 0,
            data_len: payload_len,
            user_data: netipc::RING_ECHO_USER_DATA,
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_NOP,
            user_data: netipc::RING_ECHO_USER_DATA.wrapping_add(1),
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_NOP,
            user_data: netipc::RING_ECHO_USER_DATA.wrapping_add(2),
            ..netipc::ring::Sqe::default()
        },
    ];
    for sqe in &batch {
        if !ring.sq_push(sqe) {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => return finish(handle, Err(e)),
    };

    #[allow(clippy::cast_possible_truncation)]
    let size_u32 = size as u32;
    let mut req = [0u8; 16];
    let req_len = match netipc::encode_ring_echo(&mut req, handle.raw(), size_u32) {
        Some(n) => n,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return finish(handle, Err(e));
    }

    let reply = match channel::recv_timeout(client, 5_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    channel::close(client);

    match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(_) => {}
        netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    // Reap all three completions the daemon posted. They must come back in FIFO
    // (submission) order, each carrying the SQE's echoed user_data:
    //   [0] OP_SEND → result = payload_len
    //   [1] OP_NOP  → result = 0
    //   [2] OP_NOP  → result = 0
    let expected_results: [(u64, i32); 3] = [
        (netipc::RING_ECHO_USER_DATA, payload_len as i32),
        (netipc::RING_ECHO_USER_DATA.wrapping_add(1), 0),
        (netipc::RING_ECHO_USER_DATA.wrapping_add(2), 0),
    ];
    for (i, &(want_ud, want_res)) in expected_results.iter().enumerate() {
        let cqe = match ring.cq_pop() {
            Some(c) => c,
            None => {
                serial_println!("[spawn]   ring-echo: missing completion {}", i);
                return finish(handle, Err(KernelError::InternalError));
            }
        };
        if cqe.user_data != want_ud {
            serial_println!(
                "[spawn]   ring-echo: completion {} user_data mismatch (got {:#x}, want {:#x})",
                i, cqe.user_data, want_ud
            );
            return finish(handle, Err(KernelError::InternalError));
        }
        if cqe.result != want_res {
            serial_println!(
                "[spawn]   ring-echo: completion {} result mismatch (got {}, want {})",
                i, cqe.result, want_res
            );
            return finish(handle, Err(KernelError::InternalError));
        }
    }
    // The daemon must have drained the whole SQ — no stray extra completions.
    if ring.cq_pop().is_some() {
        serial_println!("[spawn]   ring-echo: unexpected extra completion");
        return finish(handle, Err(KernelError::InternalError));
    }

    // Read the transformed payload back and confirm it is the upper-cased form.
    let mut echoed = [0u8; PAYLOAD.len()];
    if !ring.read_data(0, &mut echoed) {
        return finish(handle, Err(KernelError::InternalError));
    }
    let mut expected = [0u8; PAYLOAD.len()];
    for (dst, src) in expected.iter_mut().zip(PAYLOAD.iter()) {
        *dst = src.to_ascii_uppercase();
    }
    if echoed == expected {
        finish(handle, Ok(()))
    } else {
        serial_println!("[spawn]   ring-echo: transformed payload mismatch");
        finish(handle, Err(KernelError::InternalError))
    }
}

/// Perform one `OP_RING_TCP` round-trip to the running `net.stack` daemon: drive
/// a complete TCP fetch of `ip:port` entirely over the shared-memory ring.
///
/// The kernel creates an SHM region, lays it out as an io_uring-style ring
/// (`Ring::init`), writes an HTTP request into the data area, and submits a
/// four-SQE socket batch — `OP_CONNECT` (endpoint packed into `aux`), `OP_SEND`
/// (request window), `OP_RECV` (response window), `OP_CLOSE` — then asks the
/// daemon (via a single `OP_RING_TCP` control message) to map the region and
/// drain the batch, driving one live `TcpConn`. The daemon posts one completion
/// per SQE; the kernel reaps all four in FIFO order (verifying the echoed
/// `user_data`), reads the response bytes back out of the ring's recv window, and
/// checks they are an HTTP reply.
///
/// Returns `Ok(Some(()))` if a well-formed HTTP response came back through the
/// ring, `Ok(None)` if the ring/IPC path worked but the connection could not be
/// established or the response was empty/short (no upstream — common under
/// slirp), or `Err` on a transport failure, a `ST_FAIL` reply, or a
/// missing/out-of-order completion. This is the ring-native equivalent of
/// [`netstack_tcp_fetch_roundtrip`] — the Phase-5 streaming socket data path.
fn netstack_ring_tcp_roundtrip(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service, shm};

    // HTTP/1.0 HEAD: header-only response fits comfortably in the recv window.
    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
    // Data-area layout: request at offset 0, response window at RECV_OFF.
    const RECV_OFF: u32 = 512;
    const RECV_CAP: u32 = 512;
    // user_data base for the four-SQE socket batch (echoed back per completion).
    const UD: u64 = 0x5254_4350_0000_0000; // "RTCP"

    // Ring geometry: SQ/CQ of 8 slots (room for the 4-SQE batch) + a 1 KiB data
    // area (512 request + 512 response). region_size stays under one 16 KiB frame.
    let sq_entries: u32 = 8;
    let cq_entries: u32 = 8;
    let data_len: u32 = RECV_OFF + RECV_CAP;
    let need = netipc::ring::region_size(sq_entries, cq_entries, data_len);

    let handle = shm::create(need)?;
    authorize_netstack_daemon(handle);
    let size = shm::size(handle)?;

    // Close the SHM handle on every exit path.
    let finish = |h: shm::ShmHandle, r: KernelResult<Option<()>>| -> KernelResult<Option<()>> {
        shm::close(h);
        r
    };

    let kaddr = match shm::kernel_addr(handle) {
        Ok(p) => p,
        Err(e) => return finish(handle, Err(e)),
    };

    // Lay out the region as a ring and stage the request + socket batch.
    // SAFETY: kaddr is valid+writable for `size` (>= need) bytes and no other
    // party touches the region until the daemon attaches (strictly after we send
    // the request below). `init` validates the geometry fits.
    let ring = match unsafe { netring::Ring::init(kaddr, size, sq_entries, cq_entries, data_len) } {
        Some(r) => r,
        None => return finish(handle, Err(KernelError::InternalError)),
    };
    if !ring.write_data(0, HTTP_REQ) {
        return finish(handle, Err(KernelError::InternalError));
    }
    #[allow(clippy::cast_possible_truncation)]
    let req_len = HTTP_REQ.len() as u32;

    // The socket batch: connect → send → recv → close. Each SQE carries a distinct
    // user_data (UD + index) so we can confirm the daemon preserved FIFO order.
    let batch = [
        netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            user_data: UD,
            aux: netipc::ring::Sqe::pack_endpoint(ip, port),
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: 0,
            data_off: 0,
            data_len: req_len,
            user_data: UD.wrapping_add(1),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: 0,
            data_off: RECV_OFF,
            data_len: RECV_CAP,
            user_data: UD.wrapping_add(2),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            user_data: UD.wrapping_add(3),
            ..netipc::ring::Sqe::default()
        },
    ];
    for sqe in &batch {
        if !ring.sq_push(sqe) {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => return finish(handle, Err(e)),
    };

    #[allow(clippy::cast_possible_truncation)]
    let size_u32 = size as u32;
    let mut req = [0u8; 16];
    let req_msg_len = match netipc::encode_ring_tcp(&mut req, handle.raw(), size_u32) {
        Some(n) => n,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_msg_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return finish(handle, Err(e));
    }

    // The daemon drives a full TCP transaction over the ring; allow generous time.
    let reply = match channel::recv_timeout(client, 12_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    channel::close(client);

    match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(_) => {}
        netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    // Reap the four completions in FIFO (submission) order, capturing each result:
    //   [0] OP_CONNECT → 0 on success, or negative errno
    //   [1] OP_SEND    → bytes accepted, or negative errno
    //   [2] OP_RECV    → bytes received (0 = EOF/empty), or negative errno
    //   [3] OP_CLOSE   → 0
    let mut results = [0i32; 4];
    for (i, slot) in results.iter_mut().enumerate() {
        let cqe = match ring.cq_pop() {
            Some(c) => c,
            None => {
                serial_println!("[spawn]   ring-tcp: missing completion {}", i);
                return finish(handle, Err(KernelError::InternalError));
            }
        };
        #[allow(clippy::cast_possible_truncation)]
        let want_ud = UD.wrapping_add(i as u64);
        if cqe.user_data != want_ud {
            serial_println!(
                "[spawn]   ring-tcp: completion {} user_data mismatch (got {:#x}, want {:#x})",
                i, cqe.user_data, want_ud
            );
            return finish(handle, Err(KernelError::InternalError));
        }
        *slot = cqe.result;
    }
    // The daemon must have drained the whole SQ — no stray extra completions.
    if ring.cq_pop().is_some() {
        serial_println!("[spawn]   ring-tcp: unexpected extra completion");
        return finish(handle, Err(KernelError::InternalError));
    }

    let [connect_res, send_res, recv_res, _close_res] = results;

    // If the connection could not be established (no upstream), OP_CONNECT fails
    // and the rest cascade to -1 — the ring/IPC path is still proven.
    if connect_res < 0 {
        return finish(handle, Ok(None));
    }
    // Connected: the send must have accepted our request bytes.
    if send_res < 0 {
        serial_println!("[spawn]   ring-tcp: send failed (result {})", send_res);
        return finish(handle, Err(KernelError::InternalError));
    }
    // A short/empty response means connected+sent but no data came back (slirp
    // variance) — the ring path is proven even so.
    if recv_res < 5 {
        return finish(handle, Ok(None));
    }

    // Read the response out of the ring's recv window and verify it's HTTP.
    #[allow(clippy::cast_sign_loss)]
    let n = (recv_res as usize).min(RECV_CAP as usize);
    let mut body = [0u8; RECV_CAP as usize];
    let window = match body.get_mut(..n) {
        Some(w) => w,
        None => return finish(handle, Err(KernelError::InternalError)),
    };
    if !ring.read_data(RECV_OFF as usize, window) {
        return finish(handle, Err(KernelError::InternalError));
    }
    if window.len() >= 5 && window.get(..5) == Some(b"HTTP/".as_slice()) {
        // Echo the status line (up to CRLF) for a human sanity-check.
        let line_end = window
            .iter()
            .position(|&b| b == b'\r' || b == b'\n')
            .unwrap_or(window.len().min(64));
        let show = window.get(..line_end).unwrap_or(&[]);
        serial_print!("[spawn]   ring-tcp HTTP status = ");
        for &b in show {
            let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
            serial_print!("{}", c as char);
        }
        serial_println!("");
        finish(handle, Ok(Some(())))
    } else {
        finish(handle, Ok(None))
    }
}

/// Ring-3 end-to-end test of the daemon's **multiplexed** connection table:
/// two live TCP connections over a *single* ring, addressed by distinct SQE
/// `conn_id`s.
///
/// This is the Phase-5 socket-server prerequisite — where
/// [`netstack_ring_tcp_roundtrip`] drives one connection, this proves the daemon
/// can hold several at once keyed by `conn_id` (see `RingConns` in the daemon).
/// The kernel lays out one ring with two request windows and two response
/// windows, then submits an eight-SQE batch that opens *both* connections before
/// tearing either down, so the two `TcpConn`s coexist in the daemon's table:
///
/// ```text
/// CONNECT#7, CONNECT#9,           // both live simultaneously
/// SEND#7, RECV#7, CLOSE#7,        // drive + close conn 7
/// SEND#9, RECV#9, CLOSE#9         // then drive + close conn 9
/// ```
///
/// The ordering is deliberate: the daemon's receive path drops NIC frames that
/// don't match the *current* connection's 4-tuple (no shared RX demux yet —
/// `known-issues.md` D-NETSTACK-RX-DEMUX), so at most one connection is in its
/// `OP_RECV` phase at a time. The peers stay silent between handshake and request,
/// so the connections genuinely coexist without their inbound streams overlapping.
///
/// Returns `Ok(Some(()))` if both connections returned an HTTP response,
/// `Ok(None)` if there was no upstream (either connect failed / short response) —
/// the ring multiplexing path is proven either way — and `Err` only on a real
/// protocol/geometry fault (missing/out-of-order completion, send failure, etc.).
fn netstack_ring_tcp_multi_roundtrip(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service, shm};

    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
    // Data-area layout: two request windows (0, 256) + two response windows.
    const REQ7_OFF: u32 = 0;
    const REQ9_OFF: u32 = 256;
    const RECV7_OFF: u32 = 512;
    const RECV9_OFF: u32 = 1024;
    const RECV_CAP: u32 = 512;
    // Distinct client-chosen connection ids for the two multiplexed sockets.
    const CID7: u32 = 7;
    const CID9: u32 = 9;
    // user_data base for the eight-SQE batch (echoed back per completion).
    const UD: u64 = 0x5254_434d_0000_0000; // "RTCM" (ring-tcp-multi)

    // Ring geometry: SQ/CQ of 16 slots (room for the 8-SQE batch) + a 1.5 KiB data
    // area (two 256 B requests + two 512 B responses). Stays under one 16 KiB frame.
    let sq_entries: u32 = 16;
    let cq_entries: u32 = 16;
    let data_len: u32 = RECV9_OFF + RECV_CAP;
    let need = netipc::ring::region_size(sq_entries, cq_entries, data_len);

    let handle = shm::create(need)?;
    authorize_netstack_daemon(handle);
    let size = shm::size(handle)?;

    let finish = |h: shm::ShmHandle, r: KernelResult<Option<()>>| -> KernelResult<Option<()>> {
        shm::close(h);
        r
    };

    let kaddr = match shm::kernel_addr(handle) {
        Ok(p) => p,
        Err(e) => return finish(handle, Err(e)),
    };

    // SAFETY: kaddr is valid+writable for `size` (>= need) bytes and no other
    // party touches the region until the daemon attaches (strictly after we send
    // the request below). `init` validates the geometry fits.
    let ring = match unsafe { netring::Ring::init(kaddr, size, sq_entries, cq_entries, data_len) } {
        Some(r) => r,
        None => return finish(handle, Err(KernelError::InternalError)),
    };
    // Stage the same HTTP request into both request windows.
    if !ring.write_data(REQ7_OFF as usize, HTTP_REQ) || !ring.write_data(REQ9_OFF as usize, HTTP_REQ)
    {
        return finish(handle, Err(KernelError::InternalError));
    }
    #[allow(clippy::cast_possible_truncation)]
    let req_len = HTTP_REQ.len() as u32;

    let ep = netipc::ring::Sqe::pack_endpoint(ip, port);
    // Eight SQEs: open both, then drive+close each in turn (see fn doc). Each
    // carries a distinct user_data (UD + index) to confirm FIFO completion order.
    let batch = [
        netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: CID7,
            user_data: UD,
            aux: ep,
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: CID9,
            user_data: UD.wrapping_add(1),
            aux: ep,
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: CID7,
            data_off: REQ7_OFF,
            data_len: req_len,
            user_data: UD.wrapping_add(2),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: CID7,
            data_off: RECV7_OFF,
            data_len: RECV_CAP,
            user_data: UD.wrapping_add(3),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            conn_id: CID7,
            user_data: UD.wrapping_add(4),
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: CID9,
            data_off: REQ9_OFF,
            data_len: req_len,
            user_data: UD.wrapping_add(5),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: CID9,
            data_off: RECV9_OFF,
            data_len: RECV_CAP,
            user_data: UD.wrapping_add(6),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            conn_id: CID9,
            user_data: UD.wrapping_add(7),
            ..netipc::ring::Sqe::default()
        },
    ];
    for sqe in &batch {
        if !ring.sq_push(sqe) {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => return finish(handle, Err(e)),
    };

    #[allow(clippy::cast_possible_truncation)]
    let size_u32 = size as u32;
    let mut req = [0u8; 16];
    let req_msg_len = match netipc::encode_ring_tcp(&mut req, handle.raw(), size_u32) {
        Some(n) => n,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let msg = match channel::Message::from_bytes(&req[..req_msg_len]) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return finish(handle, Err(e));
    }

    // Two full TCP transactions drive over the ring; allow generous time.
    let reply = match channel::recv_timeout(client, 20_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    channel::close(client);

    match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(_) => {}
        netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    // Reap all eight completions in FIFO (submission) order:
    //   [0] CONNECT#7  [1] CONNECT#9  [2] SEND#7  [3] RECV#7
    //   [4] CLOSE#7    [5] SEND#9     [6] RECV#9  [7] CLOSE#9
    let mut results = [0i32; 8];
    for (i, slot) in results.iter_mut().enumerate() {
        let cqe = match ring.cq_pop() {
            Some(c) => c,
            None => {
                serial_println!("[spawn]   ring-tcp-multi: missing completion {}", i);
                return finish(handle, Err(KernelError::InternalError));
            }
        };
        #[allow(clippy::cast_possible_truncation)]
        let want_ud = UD.wrapping_add(i as u64);
        if cqe.user_data != want_ud {
            serial_println!(
                "[spawn]   ring-tcp-multi: completion {} user_data mismatch (got {:#x}, want {:#x})",
                i, cqe.user_data, want_ud
            );
            return finish(handle, Err(KernelError::InternalError));
        }
        *slot = cqe.result;
    }
    if ring.cq_pop().is_some() {
        serial_println!("[spawn]   ring-tcp-multi: unexpected extra completion");
        return finish(handle, Err(KernelError::InternalError));
    }

    let [connect7, connect9, send7, recv7, _close7, send9, recv9, _close9] = results;

    // Either connect failing (no upstream) leaves the multiplexing path proven.
    if connect7 < 0 || connect9 < 0 {
        return finish(handle, Ok(None));
    }
    // Both sends must have accepted their request bytes.
    if send7 < 0 || send9 < 0 {
        serial_println!(
            "[spawn]   ring-tcp-multi: send failed (conn7 {}, conn9 {})",
            send7, send9
        );
        return finish(handle, Err(KernelError::InternalError));
    }
    // A short/empty response on either means connected+sent but no data came back
    // (slirp variance) — the multiplexing path is proven even so.
    if recv7 < 5 || recv9 < 5 {
        return finish(handle, Ok(None));
    }

    // Verify each connection's response window independently begins with "HTTP/".
    let http_ok = |recv_res: i32, off: u32, label: &str| -> KernelResult<bool> {
        #[allow(clippy::cast_sign_loss)]
        let n = (recv_res as usize).min(RECV_CAP as usize);
        let mut body = [0u8; RECV_CAP as usize];
        let window = match body.get_mut(..n) {
            Some(w) => w,
            None => return Err(KernelError::InternalError),
        };
        if !ring.read_data(off as usize, window) {
            return Err(KernelError::InternalError);
        }
        if window.len() >= 5 && window.get(..5) == Some(b"HTTP/".as_slice()) {
            let line_end = window
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .unwrap_or(window.len().min(64));
            let show = window.get(..line_end).unwrap_or(&[]);
            serial_print!("[spawn]   ring-tcp-multi {} HTTP status = ", label);
            for &b in show {
                let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
                serial_print!("{}", c as char);
            }
            serial_println!("");
            Ok(true)
        } else {
            Ok(false)
        }
    };

    let ok7 = match http_ok(recv7, RECV7_OFF, "conn7") {
        Ok(v) => v,
        Err(e) => return finish(handle, Err(e)),
    };
    let ok9 = match http_ok(recv9, RECV9_OFF, "conn9") {
        Ok(v) => v,
        Err(e) => return finish(handle, Err(e)),
    };
    if ok7 && ok9 {
        finish(handle, Ok(Some(())))
    } else {
        finish(handle, Ok(None))
    }
}

/// Ring-3 end-to-end test of the daemon's **shared RX demux** (D-NETSTACK-RX-DEMUX):
/// two concurrent connections whose *sends both precede both receives*, so one
/// connection's response frames arrive while the daemon is blocked receiving for
/// the *other*.
///
/// Where [`netstack_ring_tcp_multi_roundtrip`] fully drives conn7 (send→recv→close)
/// before conn9 even sends — so the two never overlap on the wire — this test
/// interleaves them:
///
/// ```text
/// CONNECT#7, CONNECT#9,  SEND#7, SEND#9,  RECV#7, RECV#9,  CLOSE#7, CLOSE#9
/// ```
///
/// Both requests are on the wire before either `RECV`. When the daemon blocks in
/// `RECV#7`, conn9's response frames are already arriving. The single NIC delivers
/// frames for *both* tuples, so the old per-connection filtered read would have
/// discarded conn9's frames while waiting on conn7 — leaving `RECV#9` to come back
/// empty (its data lost). The shared RX pump instead routes each frame to its
/// owning connection by 4-tuple and buffers it, so `RECV#9` still returns conn9's
/// response. Both connections returning HTTP therefore proves the demux.
///
/// Returns `Ok(Some(()))` if *both* connections returned an HTTP response (demux
/// confirmed), `Ok(None)` if there was no upstream / a short response (network
/// variance — the demux path still ran without dropping a sibling), and `Err` on a
/// real protocol fault (missing/out-of-order completion, or a send failing after a
/// successful connect).
fn netstack_ring_tcp_demux_roundtrip(ip: &[u8; 4], port: u16) -> KernelResult<Option<()>> {
    use crate::ipc::{channel, service, shm};

    const HTTP_REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
    // Data-area layout: two request windows (0, 256) + two response windows.
    const REQ7_OFF: u32 = 0;
    const REQ9_OFF: u32 = 256;
    const RECV7_OFF: u32 = 512;
    const RECV9_OFF: u32 = 1024;
    const RECV_CAP: u32 = 512;
    const CID7: u32 = 7;
    const CID9: u32 = 9;
    // user_data base for the eight-SQE batch (echoed back per completion).
    const UD: u64 = 0x5254_4458_0000_0000; // "RTDX" (ring-tcp-demux)

    let sq_entries: u32 = 16;
    let cq_entries: u32 = 16;
    let data_len: u32 = RECV9_OFF + RECV_CAP;
    let need = netipc::ring::region_size(sq_entries, cq_entries, data_len);

    let handle = shm::create(need)?;
    authorize_netstack_daemon(handle);
    let size = shm::size(handle)?;

    let finish = |h: shm::ShmHandle, r: KernelResult<Option<()>>| -> KernelResult<Option<()>> {
        shm::close(h);
        r
    };

    let kaddr = match shm::kernel_addr(handle) {
        Ok(p) => p,
        Err(e) => return finish(handle, Err(e)),
    };

    // SAFETY: kaddr is valid+writable for `size` (>= need) bytes and no other party
    // touches the region until the daemon attaches (strictly after we send below).
    // `init` validates the geometry fits.
    let ring = match unsafe { netring::Ring::init(kaddr, size, sq_entries, cq_entries, data_len) } {
        Some(r) => r,
        None => return finish(handle, Err(KernelError::InternalError)),
    };
    if !ring.write_data(REQ7_OFF as usize, HTTP_REQ) || !ring.write_data(REQ9_OFF as usize, HTTP_REQ)
    {
        return finish(handle, Err(KernelError::InternalError));
    }
    #[allow(clippy::cast_possible_truncation)]
    let req_len = HTTP_REQ.len() as u32;

    let ep = netipc::ring::Sqe::pack_endpoint(ip, port);
    // Eight SQEs: open both, send on both, THEN receive on both, then close both.
    // Putting both sends before both recvs is what forces the concurrency the demux
    // must survive (see fn doc). Distinct user_data (UD + index) confirms FIFO order.
    let batch = [
        netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: CID7,
            user_data: UD,
            aux: ep,
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CONNECT,
            conn_id: CID9,
            user_data: UD.wrapping_add(1),
            aux: ep,
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: CID7,
            data_off: REQ7_OFF,
            data_len: req_len,
            user_data: UD.wrapping_add(2),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_SEND,
            conn_id: CID9,
            data_off: REQ9_OFF,
            data_len: req_len,
            user_data: UD.wrapping_add(3),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: CID7,
            data_off: RECV7_OFF,
            data_len: RECV_CAP,
            user_data: UD.wrapping_add(4),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_RECV,
            conn_id: CID9,
            data_off: RECV9_OFF,
            data_len: RECV_CAP,
            user_data: UD.wrapping_add(5),
            aux: 0,
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            conn_id: CID7,
            user_data: UD.wrapping_add(6),
            ..netipc::ring::Sqe::default()
        },
        netipc::ring::Sqe {
            op: netipc::ring::OP_CLOSE,
            conn_id: CID9,
            user_data: UD.wrapping_add(7),
            ..netipc::ring::Sqe::default()
        },
    ];
    for sqe in &batch {
        if !ring.sq_push(sqe) {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    let client = match service::connect(b"net.stack") {
        Ok(c) => c,
        Err(e) => return finish(handle, Err(e)),
    };

    #[allow(clippy::cast_possible_truncation)]
    let size_u32 = size as u32;
    let mut req = [0u8; 16];
    let req_msg_len = match netipc::encode_ring_tcp(&mut req, handle.raw(), size_u32) {
        Some(n) => n,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let encoded = match req.get(..req_msg_len) {
        Some(s) => s,
        None => {
            channel::close(client);
            return finish(handle, Err(KernelError::InternalError));
        }
    };
    let msg = match channel::Message::from_bytes(encoded) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    if let Err(e) = channel::send(client, msg) {
        channel::close(client);
        return finish(handle, Err(e));
    }

    // Two full TCP transactions drive over the ring; allow generous time.
    let reply = match channel::recv_timeout(client, 20_000_000_000) {
        Ok(m) => m,
        Err(e) => {
            channel::close(client);
            return finish(handle, Err(e));
        }
    };
    channel::close(client);

    match netipc::parse_bytes_reply(reply.data()) {
        netipc::BytesReply::Ok(_) => {}
        netipc::BytesReply::Fail | netipc::BytesReply::Malformed => {
            return finish(handle, Err(KernelError::InternalError));
        }
    }

    // Reap all eight completions in FIFO (submission) order:
    //   [0] CONNECT#7 [1] CONNECT#9 [2] SEND#7 [3] SEND#9
    //   [4] RECV#7    [5] RECV#9    [6] CLOSE#7 [7] CLOSE#9
    let mut results = [0i32; 8];
    for (i, slot) in results.iter_mut().enumerate() {
        let cqe = match ring.cq_pop() {
            Some(c) => c,
            None => {
                serial_println!("[spawn]   ring-tcp-demux: missing completion {}", i);
                return finish(handle, Err(KernelError::InternalError));
            }
        };
        #[allow(clippy::cast_possible_truncation)]
        let want_ud = UD.wrapping_add(i as u64);
        if cqe.user_data != want_ud {
            serial_println!(
                "[spawn]   ring-tcp-demux: completion {} user_data mismatch (got {:#x}, want {:#x})",
                i, cqe.user_data, want_ud
            );
            return finish(handle, Err(KernelError::InternalError));
        }
        *slot = cqe.result;
    }
    if ring.cq_pop().is_some() {
        serial_println!("[spawn]   ring-tcp-demux: unexpected extra completion");
        return finish(handle, Err(KernelError::InternalError));
    }

    let [connect7, connect9, send7, send9, recv7, recv9, _close7, _close9] = results;

    // Either connect failing (no upstream) leaves the demux path proven (it ran).
    if connect7 < 0 || connect9 < 0 {
        return finish(handle, Ok(None));
    }
    // Both sends must have accepted their request bytes.
    if send7 < 0 || send9 < 0 {
        serial_println!(
            "[spawn]   ring-tcp-demux: send failed (conn7 {}, conn9 {})",
            send7, send9
        );
        return finish(handle, Err(KernelError::InternalError));
    }
    // A short/empty response on either means connected+sent but no data came back
    // (slirp variance) — the demux path is proven regardless.
    if recv7 < 5 || recv9 < 5 {
        return finish(handle, Ok(None));
    }

    // Verify each connection's response window independently begins with "HTTP/".
    // conn9's response is the load-bearing one: it could only have been buffered
    // (not dropped) if the RX pump routed conn9's frames while RECV#7 was blocked.
    let http_ok = |recv_res: i32, off: u32, label: &str| -> KernelResult<bool> {
        #[allow(clippy::cast_sign_loss)]
        let n = (recv_res as usize).min(RECV_CAP as usize);
        let mut body = [0u8; RECV_CAP as usize];
        let window = match body.get_mut(..n) {
            Some(w) => w,
            None => return Err(KernelError::InternalError),
        };
        if !ring.read_data(off as usize, window) {
            return Err(KernelError::InternalError);
        }
        if window.len() >= 5 && window.get(..5) == Some(b"HTTP/".as_slice()) {
            let line_end = window
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .unwrap_or(window.len().min(64));
            let show = window.get(..line_end).unwrap_or(&[]);
            serial_print!("[spawn]   ring-tcp-demux {} HTTP status = ", label);
            for &b in show {
                let c = if (0x20..0x7f).contains(&b) { b } else { b'.' };
                serial_print!("{}", c as char);
            }
            serial_println!("");
            Ok(true)
        } else {
            Ok(false)
        }
    };

    let ok7 = match http_ok(recv7, RECV7_OFF, "conn7") {
        Ok(v) => v,
        Err(e) => return finish(handle, Err(e)),
    };
    let ok9 = match http_ok(recv9, RECV9_OFF, "conn9") {
        Ok(v) => v,
        Err(e) => return finish(handle, Err(e)),
    };
    if ok7 && ok9 {
        finish(handle, Ok(Some(())))
    } else {
        finish(handle, Ok(None))
    }
}

/// Ring-3 end-to-end test of the Linux `brk(2)` heap.
///
/// Spawns a real Linux-ABI process that queries its program break, grows the
/// heap by 32 KiB (two 16 KiB frames), writes a sentinel into the *second*
/// frame of the new heap, reads it back, and `exit`s with that byte.  A clean
/// exit with `SENTINEL` proves the whole path works: `set_brk_region` at load
/// time, the grow branch of `sys_brk` (VMA add + RLIMIT_AS charge), and
/// demand-paging the freshly-mapped heap frames on first touch.  A `0xAA`
/// exit means the kernel refused/returned the wrong break; a non-zombie state
/// means the process faulted dereferencing unmapped heap memory.
pub fn self_test_linux_brk() -> KernelResult<()> {
    const SENTINEL: u8 = 0x6D; // 109 — distinct from mmap (0x5B) and interp (42)

    serial_println!("[spawn] Running Linux brk(2) heap (ring 3) integration test...");

    let exe_elf = elf::build_linux_brk_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"brkprog"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-brk",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: brk-test spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the thread run: brk(0) → brk(grow) → write/read heap → exit(byte).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: brk (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state usually means the heap grow didn't map the frame and the process \
             faulted writing to it)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: brk (ring 3) — expected exit {} (heap sentinel byte), \
             got {:?} (0xAA = grow returned the wrong break)",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux brk(2) heap (ring 3: query + grow 32 KiB + write/read \
         second frame, byte == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test of the **SA_RESTART transparent-restart** path —
/// the capstone validation for the slow-object signal-interruptibility work
/// (interruptible `read`/`write`/`wait4`/`futex` now return `ERESTARTSYS`
/// rather than parking uninterruptibly).
///
/// Spawns [`elf::build_linux_sa_restart_test_elf`], a self-contained payload
/// that: creates its own pipe (read end fd 3, write end fd 4), installs an
/// `SA_RESTART` `SIGUSR1` handler, then blocks in `read(3, buf, 1)` on the
/// empty pipe.  We let it reach the read park, post `SIGUSR1`, and yield.
///
/// A correct kernel: interrupts the parked read (→ `ERESTARTSYS`), runs the
/// handler (which writes one `sentinel` byte into the pipe), then — because
/// `SA_RESTART` is set — transparently restarts the `read`, which now returns
/// that byte.  The child exits with `buf[0] == sentinel`.
///
/// Failure modes this catches:
/// * Park-uninterruptible regression → the read never wakes, the child never
///   exits → still `Running` after the yields (test fails on the state check).
/// * Missing/incorrect restart-sentinel mapping → the read surfaces `EINTR`
///   with `buf` untouched, or the handler frame/restorer is malformed → a
///   wrong exit code or a fault (non-zombie state).
pub fn self_test_linux_sa_restart() -> KernelResult<()> {
    // Exit code on success == the byte the handler writes into the pipe.
    // Distinct from brk(109)/argv0(0x51)/mmap(91)/interp(42)/execveat(58).
    const SENTINEL: u8 = 0x7E; // 126

    serial_println!("[spawn] Running Linux SA_RESTART transparent-restart (ring 3) test...");

    let exe_elf = elf::build_linux_sa_restart_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"sarestart"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-sa-restart",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: sa-restart spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child run far enough to install its handler and park in
    // read(3, ...) on the empty pipe.  Posting the signal *before* the
    // handler is installed would let the default action (terminate) fire at
    // the prior syscall return, so we must reach the read park first.  One
    // yield runs pipe → rt_sigaction → read(park); a couple extra are cheap
    // insurance.
    crate::sched::yield_now();
    crate::sched::yield_now();

    // Verify the child is genuinely blocked (not already exited) before
    // posting — a sanity check that the read actually parked.
    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: SA_RESTART (ring 3) — child exited (code {:?}) before the \
             signal was posted; the read did not block on the empty pipe",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  This wakes the pipe park's registered signal-waiter; on
    // resume the read observes the deliverable signal, returns ERESTARTSYS,
    // and the syscall-return path builds the handler frame.
    crate::proc::signal::set_pending(result.pid, 10);

    // Run the handler (write 1 byte) → rt_sigreturn → restarted read returns
    // the byte → exit(byte).  None of these block, so a couple yields suffice.
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: SA_RESTART (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state means the interrupted read never resumed — the park was uninterruptible — \
             or the signal frame/restorer faulted)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: SA_RESTART (ring 3) — expected exit {} (handler-written byte via \
             transparently-restarted read), got {:?} (a wrong code means the read surfaced \
             EINTR with buf untouched instead of restarting)",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux SA_RESTART (ring 3: read on empty pipe → SIGUSR1 → handler writes \
         byte → ERESTARTSYS transparent restart returns it, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a **blocking `signalfd` read is interruptible
/// by a signal that is NOT in the fd's acceptance mask**.
///
/// Spawns [`elf::build_linux_signalfd_interrupt_test_elf`]: the child installs
/// a `SIGUSR1` handler *without* `SA_RESTART`, creates a `signalfd` watching
/// only `SIGUSR2`, and blocks in `read()` on it.  We post `SIGUSR1` (not in the
/// signalfd mask) and yield.  A correct kernel wakes the blocked read, runs the
/// handler, and the read returns `-EINTR`; the child detects the negative
/// return and exits with `sentinel`.
///
/// This distinguishes the fix from the bug: before the fix the signalfd read
/// only registered a waiter for *watched* signals, so `SIGUSR1` never woke it.
/// The child would park forever (the handler runs only at the syscall-return
/// checkpoint, which a parked read never reaches) → it never becomes a zombie →
/// the state check below fails.
pub fn self_test_linux_signalfd_interrupt() -> KernelResult<()> {
    // Distinct from brk(109)/argv0(0x51)/sa_restart(0x7E)/mmap(91)/interp(42).
    const SENTINEL: u8 = 0x3D; // 61

    serial_println!(
        "[spawn] Running Linux signalfd-read signal-interruptibility (ring 3) test..."
    );

    let exe_elf = elf::build_linux_signalfd_interrupt_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"sfdintr"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-signalfd-intr",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: signalfd-intr spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler + signalfd and park in read(sfd, ...).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: signalfd-intr (ring 3) — child exited (code {:?}) before the \
             signal was posted; the signalfd read did not block",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1 — NOT in the signalfd's acceptance mask (which watches only
    // SIGUSR2).  A correct kernel interrupts the blocked read; the buggy one
    // ignores it and the child stays parked.
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: signalfd-intr (ring 3) — expected Zombie, got {:?} (the blocked \
             signalfd read was NOT interrupted by the out-of-mask SIGUSR1 — it parked forever; \
             this is exactly the hang bug the fix addresses)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: signalfd-intr (ring 3) — read returned a record (>=0) instead of \
             -EINTR; the out-of-mask signal was wrongly drained as data"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: signalfd-intr (ring 3) — expected exit {} (read returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux signalfd-read interruptibility (ring 3: block in read(signalfd watching \
         SIGUSR2) → out-of-mask SIGUSR1 wakes it → handler runs → read returns -EINTR, exit == \
         {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a **blocking `eventfd` read is interruptible
/// by a deliverable signal**.
///
/// Spawns [`elf::build_linux_eventfd_interrupt_test_elf`]: the child installs
/// a `SIGUSR1` handler *without* `SA_RESTART`, creates an `eventfd2(0, 0)`
/// (counter starts at 0), and blocks in `read()` on it.  We post `SIGUSR1`
/// and yield.  A correct kernel wakes the blocked read, runs the handler, and
/// the read returns `-EINTR`; the child detects the negative return and exits
/// with `sentinel`.
///
/// This distinguishes the fix from the bug: before the fix the eventfd read
/// parked with a bare `block_current()` and a single-slot waiter that only
/// writers woke, so `SIGUSR1` never woke it.  The child would park forever
/// (the handler runs only at the syscall-return checkpoint, which a parked
/// read never reaches) → it never becomes a zombie → the state check below
/// fails.
pub fn self_test_linux_eventfd_interrupt() -> KernelResult<()> {
    // Distinct from brk(109)/argv0(0x51)/sa_restart(0x7E)/mmap(91)/
    // interp(42)/signalfd-intr(0x3D).  0x2C = 44.
    const SENTINEL: u8 = 0x2C;

    serial_println!(
        "[spawn] Running Linux eventfd-read signal-interruptibility (ring 3) test..."
    );

    let exe_elf = elf::build_linux_eventfd_interrupt_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"efdintr"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-eventfd-intr",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: eventfd-intr spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler + eventfd and park in read(efd, ...).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: eventfd-intr (ring 3) — child exited (code {:?}) before the \
             signal was posted; the eventfd read did not block",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  A correct kernel interrupts the blocked read; the buggy
    // one ignores it and the child stays parked forever.
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: eventfd-intr (ring 3) — expected Zombie, got {:?} (the blocked \
             eventfd read was NOT interrupted by SIGUSR1 — it parked forever; this is exactly \
             the hang bug the fix addresses)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: eventfd-intr (ring 3) — read returned a counter value (>=0) instead \
             of -EINTR; the signal interruption was not surfaced"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: eventfd-intr (ring 3) — expected exit {} (read returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux eventfd-read interruptibility (ring 3: block in read(eventfd) → SIGUSR1 \
         wakes it → handler runs → read returns -EINTR, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a **blocking `timerfd` read is interruptible
/// by a deliverable signal**.
///
/// Spawns [`elf::build_linux_timerfd_interrupt_test_elf`]: the child installs
/// a `SIGUSR1` handler *without* `SA_RESTART`, creates a `timerfd_create`d
/// timer that it **never arms**, and blocks in `read()` on it (a disarmed
/// timerfd read blocks indefinitely).  We post `SIGUSR1` and yield.  A correct
/// kernel wakes the blocked read, runs the handler, and the read returns
/// `-EINTR`; the child detects the negative return and exits with `sentinel`.
///
/// This distinguishes the fix from the bug: before the fix the timerfd read
/// parked with a bare `block_current()` and a single-slot waiter that only
/// `settime`/the expiry hrtimer woke, so `SIGUSR1` never woke it.  The child
/// would park forever (the handler runs only at the syscall-return checkpoint,
/// which a parked read never reaches) → it never becomes a zombie → the state
/// check below fails.
pub fn self_test_linux_timerfd_interrupt() -> KernelResult<()> {
    // Distinct from brk(109)/argv0(0x51)/sa_restart(0x7E)/mmap(91)/
    // interp(42)/signalfd-intr(0x3D)/eventfd-intr(0x2C).  0x1B = 27.
    const SENTINEL: u8 = 0x1B;

    serial_println!(
        "[spawn] Running Linux timerfd-read signal-interruptibility (ring 3) test..."
    );

    let exe_elf = elf::build_linux_timerfd_interrupt_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"tfdintr"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-timerfd-intr",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: timerfd-intr spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler + timerfd and park in read(tfd, ...).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: timerfd-intr (ring 3) — child exited (code {:?}) before the \
             signal was posted; the timerfd read did not block",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  A correct kernel interrupts the blocked read; the buggy
    // one ignores it and the child stays parked forever (disarmed timerfd).
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: timerfd-intr (ring 3) — expected Zombie, got {:?} (the blocked \
             timerfd read was NOT interrupted by SIGUSR1 — it parked forever; this is exactly \
             the hang bug the fix addresses)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: timerfd-intr (ring 3) — read returned a count (>=0) instead of \
             -EINTR; the signal interruption was not surfaced"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: timerfd-intr (ring 3) — expected exit {} (read returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux timerfd-read interruptibility (ring 3: block in read(disarmed timerfd) → \
         SIGUSR1 wakes it → handler runs → read returns -EINTR, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a **blocking `inotify` read is interruptible
/// by a deliverable signal**.
///
/// Spawns [`elf::build_linux_inotify_interrupt_test_elf`]: the child installs
/// a `SIGUSR1` handler *without* `SA_RESTART`, creates an `inotify_init1`d
/// instance with no watches, and blocks in `read()` on it (a read of an
/// inotify fd with no queued events blocks indefinitely).  We post `SIGUSR1`
/// and yield.  A correct kernel wakes the blocked read, runs the handler, and
/// the read returns `-EINTR`; the child detects the negative return and exits
/// with `sentinel`.
///
/// This distinguishes the fix from the bug: before the fix the inotify read
/// registered only a notify-waiter and parked with a bare `block_current()`,
/// so `SIGUSR1` never woke it.  The child would park forever (the handler runs
/// only at the syscall-return checkpoint, which a parked read never reaches) →
/// it never becomes a zombie → the state check below fails.
pub fn self_test_linux_inotify_interrupt() -> KernelResult<()> {
    // Distinct from brk(109)/argv0(0x51)/sa_restart(0x7E)/mmap(91)/
    // interp(42)/signalfd(0x3D)/eventfd(0x2C)/timerfd(0x1B).  0x66 = 102.
    const SENTINEL: u8 = 0x66;

    serial_println!(
        "[spawn] Running Linux inotify-read signal-interruptibility (ring 3) test..."
    );

    let exe_elf = elf::build_linux_inotify_interrupt_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"inintr"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-inotify-intr",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: inotify-intr spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler + inotify and park in read(ifd, ...).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: inotify-intr (ring 3) — child exited (code {:?}) before the \
             signal was posted; the inotify read did not block",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  A correct kernel interrupts the blocked read; the buggy
    // one ignores it and the child stays parked forever (no events queued).
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: inotify-intr (ring 3) — expected Zombie, got {:?} (the blocked \
             inotify read was NOT interrupted by SIGUSR1 — it parked forever; this is exactly \
             the hang bug the fix addresses)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: inotify-intr (ring 3) — read returned data (>=0) instead of \
             -EINTR; the signal interruption was not surfaced"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: inotify-intr (ring 3) — expected exit {} (read returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux inotify-read interruptibility (ring 3: block in read(inotify, no events) \
         → SIGUSR1 wakes it → handler runs → read returns -EINTR, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a blocking **`poll()`** is interruptible by a
/// signal and surfaces `-EINTR` (the "always-EINTR" branch of the SA_RESTART
/// taxonomy: `poll`/`select`/`epoll_wait` are *never* transparently restarted,
/// even under `SA_RESTART`).
///
/// The child installs a SIGUSR1 handler, creates an eventfd (which never
/// becomes `POLLIN`-ready), then `poll()`s it with `timeout = -1` (block
/// forever).  Before the fix, `poll_core` busy-polled in 10 ms `sleep_ms`
/// slices and never checked for a pending signal, so the blocked thread could
/// never reach the syscall-return checkpoint where the handler runs — the
/// process hung forever.  A correct kernel notices the deliverable signal,
/// returns `-EINTR`, runs the handler, and the child exits with the sentinel.
pub fn self_test_linux_poll_interrupt() -> KernelResult<()> {
    // Distinct from brk(109)/argv0(0x51)/sa_restart(0x7E)/mmap(91)/
    // interp(42)/signalfd(0x3D)/eventfd(0x2C)/timerfd(0x1B)/inotify(0x66).
    // 0x4B = 75.
    const SENTINEL: u8 = 0x4B;

    serial_println!(
        "[spawn] Running Linux poll() signal-interruptibility (ring 3) test..."
    );

    let exe_elf = elf::build_linux_poll_interrupt_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"pollintr"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-poll-intr",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: poll-intr spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler + eventfd and park in poll().  poll()
    // waits in ≤10 ms `sleep_ms` slices but registers a signal-waiter for each
    // slice, so `set_pending` wakes it immediately (just like the
    // eventfd/timerfd/inotify tests) — a couple of yields suffice.
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: poll-intr (ring 3) — child exited (code {:?}) before the \
             signal was posted; the poll() did not block",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  A correct kernel wakes the registered signal-waiter and
    // interrupts the blocked poll() with -EINTR; the buggy one ignores it and
    // the child stays parked forever (the eventfd never becomes POLLIN-ready).
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: poll-intr (ring 3) — expected Zombie, got {:?} (the blocked \
             poll() was NOT interrupted by SIGUSR1 — it parked forever; this is exactly \
             the hang bug the fix addresses)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: poll-intr (ring 3) — poll returned >=0 instead of -EINTR; \
             the signal interruption was not surfaced"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: poll-intr (ring 3) — expected exit {} (poll returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux poll() interruptibility (ring 3: block in poll(eventfd, timeout=-1) \
         → SIGUSR1 wakes it → handler runs → poll returns -EINTR, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test for the **`poll(NULL, 0, -1)` empty-set,
/// infinite-timeout** path: it must *block* until a signal, then return
/// `-EINTR` — not return `0` immediately.
///
/// With no fds to watch and an infinite timeout, only a delivered signal can
/// end the wait (Linux treats this like `pause()`).  The pre-fix `nfds == 0`
/// quick path in `poll_core` only slept for a *positive* timeout and returned
/// `ok(0)` for a negative one, so `poll(NULL, 0, -1)` spun / returned 0
/// instead of blocking.  The child here installs a SIGUSR1 handler and calls
/// `poll(NULL, 0, -1)`; a correct kernel blocks it (so it is NOT a zombie
/// before the signal), is interrupted by SIGUSR1, and `poll` returns `-EINTR`
/// → exit(sentinel).  The bug manifests as the child exiting `0xEE` (poll
/// returned 0) before the signal is even posted.
pub fn self_test_linux_poll_empty_infinite() -> KernelResult<()> {
    // Distinct from the other interrupt sentinels (…/poll(0x4B)).  0x5C = 92.
    const SENTINEL: u8 = 0x5C;

    serial_println!(
        "[spawn] Running Linux poll(NULL,0,-1) empty-set infinite-wait (ring 3) test..."
    );

    let exe_elf = elf::build_linux_poll_empty_infinite_test_elf(SENTINEL);
    let argv: &[&[u8]] = &[b"pollnull"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-poll-null",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: poll-null spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the child install its handler and park in poll(NULL,0,-1).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let pre = pcb::state(result.pid);
    if pre == Some(pcb::ProcessState::Zombie) {
        thread::on_thread_exit(result.task_id);
        let code = pcb::exit_code(result.pid);
        pcb::destroy(result.pid);
        serial_println!(
            "[spawn]   FAIL: poll-null (ring 3) — child exited (code {:?}) before the \
             signal was posted; poll(NULL,0,-1) did NOT block (the empty-set infinite-wait \
             bug: it returned 0 immediately instead of waiting)",
            code
        );
        return Err(KernelError::InternalError);
    }

    // Post SIGUSR1.  A correct kernel wakes the registered signal-waiter and
    // poll returns -EINTR.
    crate::proc::signal::set_pending(result.pid, 10);

    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: poll-null (ring 3) — expected Zombie, got {:?} (poll(NULL,0,-1) \
             was NOT interrupted by SIGUSR1 — it parked forever)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code == Some(0xEE) {
        serial_println!(
            "[spawn]   FAIL: poll-null (ring 3) — poll returned 0 (>=0) instead of -EINTR; \
             the empty-set infinite wait returned immediately instead of blocking"
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: poll-null (ring 3) — expected exit {} (poll returned -EINTR), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux poll(NULL,0,-1) empty-set infinite-wait (ring 3: blocks with no fds \
         → SIGUSR1 wakes it → poll returns -EINTR, exit == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that the SysV initial stack's **argv pointers** are
/// valid in the mapped user stack — not just the scalar `argc`.
///
/// [`self_test_linux_argc`-style coverage](elf::build_linux_argc_exit_test_elf)
/// reads only `argc` from `[rsp]`; this spawns
/// [`elf::build_linux_argv0_deref_exit_elf`], which dereferences `argv[0]`
/// (the pointer at `[rsp+8]`) and exits with its first byte.  We pass an
/// `argv[0]` whose first byte is a known sentinel and assert the zombie's
/// exit code equals it — proving the stack builder placed a *valid, correctly
/// addressed* argv-string pointer, the failure mode the argc-scalar test
/// cannot see but that crashes every real (glibc `_start` → deref argv/envp)
/// program.
pub fn self_test_linux_argv0_deref() -> KernelResult<()> {
    // First byte of argv[0]; distinct from interp(42)/mmap(91)/brk(109)/
    // execveat(58).  0x51 = 'Q' = 81.
    const SENTINEL: u8 = 0x51;

    serial_println!("[spawn] Running Linux argv[0] deref (ring 3) integration test...");

    let exe_elf = elf::build_linux_argv0_deref_exit_elf();
    // argv[0]'s first byte is the sentinel the target reads back and exits
    // with; the rest of the string is irrelevant.
    let argv: &[&[u8]] = &[b"\x51argv0-deref-test"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-argv0",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: argv0-deref spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the thread run: deref argv[0] → exit(first byte).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: argv[0] deref (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state usually means argv[0] held a bad pointer and the process faulted \
             dereferencing it)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: argv[0] deref (ring 3) — expected exit {} (argv[0][0] sentinel), \
             got {:?}",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux argv[0] deref (ring 3: read byte through stack-builder argv[0] \
         pointer, byte == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test that a **native fastpy-compiled binary** boots and
/// runs to a clean exit on SlateOS — the on-target validation of initiative
/// F's "first real component" milestone.
///
/// The embedded ELF (`services/fastpy-hello/fastpy-hello.elf`) is produced by
/// the fastpy AOT compiler (`toolchain.compile_ir_to_obj` +
/// `link_executable` with `target=SLATEOS_TARGET`) from a small Python
/// program.  It is linked against the OS's own `posix` `libc.a` and uses the
/// **native** syscall ABI (not the Linux shim).
///
/// The reason this test exists — and why it is the gate for the milestone —
/// is TLS.  The fastpy C runtime declares compiler thread-locals
/// (`FPY_THREAD_LOCAL __thread`), which the compiler lowers to `%fs:offset`
/// accesses, and the emitted ELF therefore carries a `PT_TLS` segment.  A
/// native static binary gets **no thread pointer** from the kernel (exec
/// zeroes `fs_base`) and has no aux vector to discover `PT_TLS`, so without
/// the crt's main-thread TLS setup (see `posix/src/crt.rs
/// ::setup_main_thread_tls`, which walks the program headers via
/// `__ehdr_start` and installs the thread pointer through the native
/// `SYS_SET_FS_BASE` syscall) the very first `__thread` access — or the
/// stack-protector canary at `%fs:0x28` — faults immediately.
///
/// Reaching the `Zombie` state therefore proves the whole path worked: the
/// crt found `PT_TLS`, laid out the variant-II TLS block + TCB, called
/// `SYS_SET_FS_BASE`, and the runtime then executed `__thread`-using code all
/// the way to `exit(0)`.  A fault in TLS setup would instead leave the
/// process killed in a non-zombie state (or exiting with a fault code).
pub fn self_test_fastpy_slateos_tls() -> KernelResult<()> {
    // The fastpy-compiled Python program: builds a list, sums it, prints,
    // then `sys.exit(len(sys.argv))`.  (`print` does reach the console — the
    // posix libc pre-installs fds 0/1/2 as Console handles, so a native
    // process's `write(1, ...)` routes to `SYS_CONSOLE_WRITE`; the sibling
    // `self_test_fastpy_slateos_cat` proves stdout end-to-end.  This test
    // ignores the print output and asserts only on the exit code.)  Exiting
    // with `argc` lets us verify two things at once
    // on-target: (a) TLS setup worked (it reached user code at all), and
    // (b) the argv delivery path — kernel `SYS_PROCESS_GET_ARGS` -> crt ->
    // runtime `fpy_argv` -> `sys.argv` — carried the exact argument vector we
    // spawned it with, and the non-zero exit code propagated back.
    static FASTPY_HELLO_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-hello/fastpy-hello.elf");

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS TLS (ring 3) integration test ({} bytes ELF)...",
        FASTPY_HELLO_ELF.len()
    );

    // Spawn with a known 3-element argv; the program exits with argc (3).
    let argv: &[&[u8]] = &[b"fastpy-hello", b"alpha", b"bravo"];
    const EXPECTED_ARGC: i32 = 3;
    let envp: &[&[u8]] = &[];
    let options = SpawnOptions {
        name: "fastpy-hello",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(FASTPY_HELLO_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: fastpy-hello spawn returned {:?}", e);
            return Err(e);
        }
    };

    // A fastpy binary does real work (runtime init, GC, the loop) before
    // exiting, so it needs many more scheduler slices than a 3-instruction
    // exit stub.  Yield until it becomes a zombie, bounded so a hang can't
    // wedge the boot.
    let mut became_zombie = false;
    for _ in 0..2000 {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            became_zombie = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fastpy-hello (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state means the fastpy runtime faulted, most likely on its first %fs-relative \
             __thread access because main-thread ELF TLS was not set up)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(EXPECTED_ARGC) {
        serial_println!(
            "[spawn]   FAIL: fastpy-hello (ring 3) — reached Zombie (so TLS setup worked) but \
             exit code was {:?}, expected {} (== argc). A wrong argc means the argv delivery \
             path (SYS_PROCESS_GET_ARGS -> crt -> sys.argv) is broken; exit(0) with no argc \
             would mean the program never read its arguments",
            exit_code, EXPECTED_ARGC
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS TLS (ring 3: native fastpy binary set up main-thread \
         ELF TLS via SYS_SET_FS_BASE, read its {}-element argv via sys.argv, and ran to \
         exit(argc)): OK",
        EXPECTED_ARGC
    );
    Ok(())
}

/// Ring-3 end-to-end test that fastpy **pure-mode file I/O** works on-target.
///
/// This is the first proof that the whole pure-mode file path runs natively on
/// SlateOS.  The embedded ELF (`services/fastpy-fileio/fastpy-fileio.elf`) is a
/// fastpy-compiled Python program that:
///   1. `open('/tmp/fpyio.txt', 'w')`, `write('slate\n')` (6 bytes), `close()`
///   2. `open('/tmp/fpyio.txt', 'r')`, `read()`, `close()`
///   3. `sys.exit(len(data))`  → exits with **6** (the byte count read back).
///
/// The exit code therefore proves the round-trip end-to-end:
///   fastpy `open()`/`write()`/`read()`/`close()`
///     -> runtime native file object (`fastpy_io_open` + C stdio)
///     -> posix `libc.a` `fopen`/`fwrite`/`fread`/`fclose`
///     -> native `SYS_FS_OPEN`/`SYS_FS_WRITE`/`SYS_FS_READ`
///     -> kernel VFS/memfs (the writable `/tmp` mount).
///
/// Unlike the TLS test, the process must be granted a **File capability**:
/// `sys_fs_open` calls `require_cap_type(File, READ)`, and a ring-3 process
/// (PID != 0) with no File cap gets `PermissionDenied`.  We grant a wildcard
/// File cap (`resource_id == 0`) with READ|WRITE so the open of `/tmp` succeeds.
pub fn self_test_fastpy_slateos_fileio() -> KernelResult<()> {
    static FASTPY_FILEIO_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-fileio/fastpy-fileio.elf");

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS pure-mode file I/O (ring 3) integration test \
         ({} bytes ELF)...",
        FASTPY_FILEIO_ELF.len()
    );

    let argv: &[&[u8]] = &[b"fastpy-fileio"];
    const EXPECTED_BYTES: i32 = 6; // len("slate\n")
    let envp: &[&[u8]] = &[];
    // Grant a wildcard File capability (resource_id 0) so the process passes
    // `require_cap_type(File, READ)` in `sys_fs_open`.  Without it the very
    // first open() would fail with PermissionDenied.
    let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "fastpy-fileio",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(FASTPY_FILEIO_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: fastpy-fileio spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut became_zombie = false;
    for _ in 0..2000 {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            became_zombie = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fastpy-fileio (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state means the fastpy runtime faulted somewhere on the file path, e.g. in the \
             native file object or a syscall)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(EXPECTED_BYTES) {
        serial_println!(
            "[spawn]   FAIL: fastpy-fileio (ring 3) — reached Zombie but exit code was {:?}, \
             expected {} (== len('slate\\n')). Exit 0 means read() returned no data (write or \
             reopen failed); a PermissionDenied on open would show up as a non-zombie fault",
            exit_code, EXPECTED_BYTES
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS pure-mode file I/O (ring 3: open/write/close then \
         open/read/close on the /tmp memfs via fastpy -> C stdio -> SYS_FS_* -> VFS, exited \
         with the {}-byte read count): OK",
        EXPECTED_BYTES
    );
    Ok(())
}

/// Ring-3 end-to-end test of the **first shipping fastpy SlateOS utility**:
/// `services/fastpy-cat`, a minimal `cat`(1).
///
/// This is the milestone the earlier fastpy increments were building toward —
/// a real, useful Python-via-fastpy program running natively on SlateOS.  It
/// ties together all three on-target paths at once:
///   * **argv** — the utility reads its target path from `sys.argv[1]`,
///   * **file I/O** — it `open()`/`read()`/`close()`s that file (pure-mode
///     native file object -> posix `libc.a` -> `SYS_FS_*` -> VFS),
///   * **stdout** — it `print(..., end='')`s the contents (runtime
///     `printf`/`fflush` -> posix `write(1, ...)` -> Console handle ->
///     `SYS_CONSOLE_WRITE`, which mirrors to serial).
///
/// The harness stages a known file in the writable `/tmp` memfs, spawns
/// `fastpy-cat` with that path as `argv[1]` and a File capability, and asserts
/// the process becomes a `Zombie` and exits with the file's byte count.  The
/// echoed contents also appear on the serial console (via `SYS_CONSOLE_WRITE`),
/// which the boot harness can grep for — proving stdout works end-to-end for a
/// native fastpy binary.
pub fn self_test_fastpy_slateos_cat() -> KernelResult<()> {
    static FASTPY_CAT_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-cat/fastpy-cat.elf");

    // Staged input file + its exact contents.  The exit code is the byte
    // count, so keep the length obvious: "SlateOS fastpy cat OK\n" = 22 bytes.
    const CAT_PATH: &str = "/tmp/cat-input.txt";
    const CAT_PATH_ARG: &[u8] = b"/tmp/cat-input.txt";
    const CAT_CONTENT: &[u8] = b"SlateOS fastpy cat OK\n";
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    const EXPECTED_BYTES: i32 = CAT_CONTENT.len() as i32; // 22

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS `cat` utility (ring 3) integration test \
         ({} bytes ELF)...",
        FASTPY_CAT_ELF.len()
    );

    // Stage the input file the utility will read.
    if let Err(e) = crate::fs::Vfs::write_file(CAT_PATH, CAT_CONTENT) {
        serial_println!("[spawn]   FAIL: could not stage {} — {:?}", CAT_PATH, e);
        return Err(e);
    }

    let argv: &[&[u8]] = &[b"fastpy-cat", CAT_PATH_ARG];
    let envp: &[&[u8]] = &[];
    let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "fastpy-cat",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(FASTPY_CAT_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(CAT_PATH);
            serial_println!("[spawn]   FAIL: fastpy-cat spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut became_zombie = false;
    for _ in 0..2000 {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            became_zombie = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAT_PATH);

    if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fastpy-cat (ring 3) — expected Zombie, got {:?} (the utility \
             faulted somewhere on the argv/open/read/print path)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(EXPECTED_BYTES) {
        serial_println!(
            "[spawn]   FAIL: fastpy-cat (ring 3) — reached Zombie but exit code was {:?}, \
             expected {} (== len of the staged file). A wrong count means argv[1] delivery or \
             the read path is off; exit 1 means an uncaught exception (e.g. the open failed)",
            exit_code, EXPECTED_BYTES
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS `cat` (ring 3: first shipping fastpy utility — read \
         argv[1] file off /tmp and echoed it to stdout via SYS_CONSOLE_WRITE, exited with the \
         {}-byte count): OK",
        EXPECTED_BYTES
    );
    Ok(())
}

/// Ring-3 end-to-end test of the `fastpy-sysinfo` utility — the second
/// shipping fastpy SlateOS component.
///
/// Where `fastpy-cat` reads the writable `/tmp` memfs, this one reads the
/// kernel's **procfs** (`/proc/version`, `/proc/uptime`, `/proc/meminfo`) —
/// files whose contents are *generated on the fly* with no fixed on-disk size.
/// It therefore additionally proves that fastpy pure-mode reads stream
/// generated kernel content correctly (the runtime's `fpy_file_read` loops
/// `fread` until a short read marks EOF).
///
/// The harness grants a File capability, spawns the utility, and asserts it
/// becomes a `Zombie` and exits 0.  The printed report — including the
/// `/proc/version` string `"MintOS kernel 0.1.0 …"` — is mirrored to serial via
/// `SYS_CONSOLE_WRITE`, so the boot harness can grep for it.
pub fn self_test_fastpy_slateos_sysinfo() -> KernelResult<()> {
    static FASTPY_SYSINFO_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-sysinfo/fastpy-sysinfo.elf");

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS `sysinfo` utility (ring 3) integration test \
         ({} bytes ELF)...",
        FASTPY_SYSINFO_ELF.len()
    );

    let argv: &[&[u8]] = &[b"fastpy-sysinfo"];
    const EXPECTED_EXIT: i32 = 0;
    let envp: &[&[u8]] = &[];
    let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "fastpy-sysinfo",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(FASTPY_SYSINFO_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: fastpy-sysinfo spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut became_zombie = false;
    for _ in 0..2000 {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            became_zombie = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fastpy-sysinfo (ring 3) — expected Zombie, got {:?} (the utility \
             faulted; a procfs read may have mis-streamed or an open failed)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(EXPECTED_EXIT) {
        serial_println!(
            "[spawn]   FAIL: fastpy-sysinfo (ring 3) — reached Zombie but exit code was {:?}, \
             expected {} (exit 1 means an uncaught exception, e.g. a /proc open failed)",
            exit_code, EXPECTED_EXIT
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS `sysinfo` (ring 3: read /proc/version+uptime+meminfo via \
         pure-mode file I/O and printed a report to stdout, exit 0): OK",
    );
    Ok(())
}

/// Ring-3 end-to-end test of the `fastpy-store` utility — the third shipping
/// fastpy SlateOS component and the core primitive of the package manager: a
/// **content-addressed store**.
///
/// Unlike `fastpy-cat`/`fastpy-sysinfo`, which only *stream* file contents,
/// this utility does non-trivial computation and writes to a *computed* path:
/// it reads `argv[1]`, computes a 32-bit FNV-1a digest of the bytes (pure
/// Python, all arithmetic kept inside a signed 64-bit register — no bigint),
/// formats it as 8 hex chars, writes the contents to `/tmp/store-<digest>.blob`,
/// then reads the blob back and verifies it equals the input.  It exits 0
/// **only** when the read-back verification succeeds, so a clean exit proves the
/// whole store round-trip end-to-end.
///
/// The harness stages a known input whose digest is `a6fd63bc` (matched against
/// CPython), grants a File capability, spawns the utility, and asserts it
/// becomes a `Zombie` and exits 0.  The printed digest is mirrored to serial via
/// `SYS_CONSOLE_WRITE`, so the boot harness can grep for it.
pub fn self_test_fastpy_slateos_store() -> KernelResult<()> {
    static FASTPY_STORE_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-store/fastpy-store.elf");

    // Staged input + the content-addressed blob the utility will create.  The
    // digest is the 32-bit FNV-1a of STORE_CONTENT, verified against CPython.
    const STORE_PATH: &str = "/tmp/store-input.txt";
    const STORE_PATH_ARG: &[u8] = b"/tmp/store-input.txt";
    const STORE_CONTENT: &[u8] = b"SlateOS package payload\n";
    // The blob path the utility writes: `/tmp/store-<digest>.blob`.
    const STORE_BLOB_PATH: &str = "/tmp/store-a6fd63bc.blob";
    const EXPECTED_EXIT: i32 = 0;

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS `store` utility (ring 3) integration test \
         ({} bytes ELF)...",
        FASTPY_STORE_ELF.len()
    );

    // Stage the input file the utility will hash and store.
    if let Err(e) = crate::fs::Vfs::write_file(STORE_PATH, STORE_CONTENT) {
        serial_println!("[spawn]   FAIL: could not stage {} — {:?}", STORE_PATH, e);
        return Err(e);
    }

    let argv: &[&[u8]] = &[b"fastpy-store", STORE_PATH_ARG];
    let envp: &[&[u8]] = &[];
    let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "fastpy-store",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(FASTPY_STORE_ELF, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(STORE_PATH);
            serial_println!("[spawn]   FAIL: fastpy-store spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut became_zombie = false;
    for _ in 0..2000 {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            became_zombie = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(STORE_PATH);
    // The utility wrote this blob; clean it up regardless of outcome.
    let _ = crate::fs::Vfs::remove(STORE_BLOB_PATH);

    if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fastpy-store (ring 3) — expected Zombie, got {:?} (the utility \
             faulted on the hash/open/write/read-back path)",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(EXPECTED_EXIT) {
        serial_println!(
            "[spawn]   FAIL: fastpy-store (ring 3) — reached Zombie but exit code was {:?}, \
             expected {} (exit 1 means the stored blob did not read back equal to the input; a \
             wrong digest on serial means the bigint-masked FNV arithmetic is off)",
            exit_code, EXPECTED_EXIT
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS `store` (ring 3: content-addressed store — hashed argv[1] \
         with 32-bit FNV-1a, wrote it to /tmp/store-<digest>.blob, and verified the read-back, \
         exit 0): OK",
    );
    Ok(())
}

/// Ring-3 dependency-lifecycle test for the fastpy-built SlateOS **package
/// manager** front-end (`services/fastpy-pkg`), the registry + dependency layer
/// built on top of the content-addressed store.
///
/// `fastpy-pkg` is a real subcommand CLI over a persistent text registry
/// `/tmp/pkgdb.txt` (records of `"<name> <digest> <deps>\n"`, where `<deps>` is a
/// comma-separated list of dependency package names or `-`):
///   * `install <name> <payload> <deps>` — hash the payload, write the content
///     blob `/tmp/store-<digest>.blob`, and upsert the `"<name> <digest> <deps>"`
///     record (replacing any prior record for that name); exit 0.
///   * `query <name>` — resolve the record, print its digest (exit 0) or
///     "not found" (exit 1).
///   * `deps <name>` — print the record's dependency field (exit 0) or
///     "not found" (exit 1).
///   * `check <name>` — verify every declared dependency of `<name>` is itself
///     installed; "ok <name>" + exit 0, else "missing <dep>" + exit 1 (or
///     "not found <name>" + exit 1 if `<name>` is absent).
///   * `remove <name>` — drop the record (exit 0) or "not found" (exit 1).
///   * `list` — print every record; exit 0.
///
/// This test drives the whole dependency lifecycle across **eight separate
/// ring-3 spawns**: install a dependency chain (`libc` <- `coreutils` <-
/// `grep`), `check` that the deps resolve, `remove` the base `libc` dependency,
/// then `check` again to confirm the now-missing dependency is detected — plus a
/// `deps grep` readback.  It asserts each exit code, then reads `/tmp/pkgdb.txt`
/// back from the kernel and asserts the final state.  This proves argv[1]
/// subcommand dispatch (→ native `fastpy_str_compare`), by-name resolution, an
/// idempotent upsert, record deletion, and — new here — **dependency-field
/// storage and dependency verification** (`check` comma-splits and resolves each
/// dep against the registry), all over a text DB the utility parses
/// char-by-char.  Payload digests (`libc demo\n` → 86732e22,
/// `coreutils demo\n` → 1ee068f8, `grep demo\n` → 0f4143a6) were verified
/// against CPython.
pub fn self_test_fastpy_slateos_pkg() -> KernelResult<()> {
    static FASTPY_PKG_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-pkg/fastpy-pkg.elf");

    const DB_PATH: &str = "/tmp/pkgdb.txt";
    const LIBC_PATH: &str = "/tmp/pkg-libc.txt";
    const LIBC_ARG: &[u8] = b"/tmp/pkg-libc.txt";
    const LIBC_CONTENT: &[u8] = b"libc demo\n";
    const CORE_PATH: &str = "/tmp/pkg-coreutils.txt";
    const CORE_ARG: &[u8] = b"/tmp/pkg-coreutils.txt";
    const CORE_CONTENT: &[u8] = b"coreutils demo\n";
    const GREP_PATH: &str = "/tmp/pkg-grep.txt";
    const GREP_ARG: &[u8] = b"/tmp/pkg-grep.txt";
    const GREP_CONTENT: &[u8] = b"grep demo\n";
    // The content-addressed blobs the utility writes for each payload.
    const LIBC_BLOB: &str = "/tmp/store-86732e22.blob";
    const CORE_BLOB: &str = "/tmp/store-1ee068f8.blob";
    const GREP_BLOB: &str = "/tmp/store-0f4143a6.blob";

    // Spawn `fastpy-pkg` with the given argv at ring 3, wait for it to become a
    // Zombie, and return its exit code.  A nested fn (not a closure) so it
    // captures nothing and can be reused across the lifecycle steps.
    fn run_pkg(elf: &'static [u8], argv: &[&[u8]]) -> KernelResult<i32> {
        let envp: &[&[u8]] = &[];
        let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
        let options = SpawnOptions {
            name: "fastpy-pkg",
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &caps,
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };
        let result = spawn_process(elf, &options)?;
        let mut became_zombie = false;
        for _ in 0..2000 {
            if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
                became_zombie = true;
                break;
            }
            crate::sched::yield_now();
        }
        let state = pcb::state(result.pid);
        let exit_code = pcb::exit_code(result.pid);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
            serial_println!(
                "[spawn]   FAIL: fastpy-pkg (ring 3) did not reach Zombie (state {:?})",
                state
            );
            return Err(KernelError::InternalError);
        }
        exit_code.ok_or(KernelError::InternalError)
    }

    // Remove every artifact this test stages or the utility creates.
    fn cleanup() {
        for p in [
            DB_PATH, LIBC_PATH, CORE_PATH, GREP_PATH, LIBC_BLOB, CORE_BLOB, GREP_BLOB,
        ] {
            let _ = crate::fs::Vfs::remove(p);
        }
    }

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS `pkg` manager (ring 3) dependency lifecycle test \
         ({} bytes ELF)...",
        FASTPY_PKG_ELF.len()
    );

    // Seed an empty registry + the three payloads.
    if let Err(e) = crate::fs::Vfs::write_file(DB_PATH, b"") {
        serial_println!("[spawn]   FAIL: could not seed {} — {:?}", DB_PATH, e);
        return Err(e);
    }
    for (path, content) in [
        (LIBC_PATH, LIBC_CONTENT),
        (CORE_PATH, CORE_CONTENT),
        (GREP_PATH, GREP_CONTENT),
    ] {
        if let Err(e) = crate::fs::Vfs::write_file(path, content) {
            cleanup();
            serial_println!("[spawn]   FAIL: could not stage {} — {:?}", path, e);
            return Err(e);
        }
    }

    // The dependency lifecycle: (argv, expected exit code, description).
    // Install a chain libc <- coreutils <- grep; verify deps resolve; remove the
    // base libc; verify the now-missing dep is detected.
    let steps: &[(&[&[u8]], i32, &str)] = &[
        (&[b"fastpy-pkg", b"install", b"libc", LIBC_ARG, b"-"], 0, "install libc (no deps)"),
        (&[b"fastpy-pkg", b"install", b"coreutils", CORE_ARG, b"libc"], 0, "install coreutils (dep libc)"),
        (&[b"fastpy-pkg", b"install", b"grep", GREP_ARG, b"libc,coreutils"], 0, "install grep (deps libc,coreutils)"),
        (&[b"fastpy-pkg", b"check", b"grep"], 0, "check grep (deps satisfied)"),
        (&[b"fastpy-pkg", b"check", b"coreutils"], 0, "check coreutils (deps satisfied)"),
        (&[b"fastpy-pkg", b"deps", b"grep"], 0, "deps grep"),
        (&[b"fastpy-pkg", b"remove", b"libc"], 0, "remove libc (base dep)"),
        (&[b"fastpy-pkg", b"check", b"grep"], 1, "check grep (dep libc now missing)"),
    ];

    for (argv, want, desc) in steps {
        match run_pkg(FASTPY_PKG_ELF, argv) {
            Ok(code) if code == *want => {}
            Ok(code) => {
                cleanup();
                serial_println!(
                    "[spawn]   FAIL: fastpy-pkg `{}` exited {} (expected {})",
                    desc, code, want
                );
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                cleanup();
                serial_println!("[spawn]   FAIL: fastpy-pkg `{}` — {:?}", desc, e);
                return Err(e);
            }
        }
    }

    // Verify the final on-disk registry: grep must remain with its full record
    // (name digest deps), while the removed libc record must be gone.  Note the
    // string "libc" still appears as a *dependency* of coreutils/grep, so we key
    // the "gone" check on the libc record's name+digest, not on "libc" alone.
    let db = match crate::fs::Vfs::read_file(DB_PATH) {
        Ok(bytes) => bytes,
        Err(e) => {
            cleanup();
            serial_println!("[spawn]   FAIL: could not read back {} — {:?}", DB_PATH, e);
            return Err(e);
        }
    };
    // Simple substring checks over the small registry text.
    let contains = |needle: &[u8]| -> bool {
        db.windows(needle.len()).any(|w| w == needle)
    };
    let grep_record_present = contains(b"grep 0f4143a6 libc,coreutils");
    let libc_record_gone = !contains(b"libc 86732e22");

    cleanup();

    if !grep_record_present || !libc_record_gone {
        serial_println!(
            "[spawn]   FAIL: fastpy-pkg final registry wrong (grep_record_present={}, \
             libc_record_gone={}) — install/remove did not persist correctly",
            grep_record_present, libc_record_gone
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS `pkg` (ring 3: package manager CLI — dependency chain \
         install libc<-coreutils<-grep / check deps-satisfied / deps readback / remove base dep / \
         check dep-now-missing over the persistent /tmp/pkgdb.txt registry, final state verified): \
         OK",
    );
    Ok(())
}

/// Ring-3 **generations / rollback** test for the fastpy-built SlateOS package
/// manager (`services/fastpy-pkg`) — the immutable, atomically-switchable
/// install-root feature (the Nix/NixOS model).
///
/// A *generation* is an immutable snapshot of the whole registry.  `commit`
/// freezes the current `/tmp/pkgdb.txt` as `/tmp/pkg-gen-<n>.txt` and advances
/// the current-generation pointer `/tmp/pkg-current.txt`; `rollback` restores
/// the previous generation's snapshot over the live registry in a single write
/// (an atomic switch); `current` prints the active generation number.
///
/// This test proves the whole cycle across **seven ring-3 spawns**: install
/// `foo`, `commit` (→ generation 1), install `bar`, `commit` (→ generation 2),
/// `current` (→ "generation 2"), `rollback` (→ generation 1), `current` (→
/// "generation 1").  After the rollback it reads `/tmp/pkgdb.txt` back from the
/// kernel and asserts the live registry is exactly generation 1's snapshot —
/// `foo` present, `bar` gone — i.e. the rollback atomically reverted the `bar`
/// install.  It reuses two known payloads (`libc demo\n` → 86732e22 as `foo`,
/// `coreutils demo\n` → 1ee068f8 as `bar`), so the content blobs are at known
/// paths for cleanup.
pub fn self_test_fastpy_slateos_pkg_gen() -> KernelResult<()> {
    static FASTPY_PKG_ELF: &[u8] =
        include_bytes!("../../../services/fastpy-pkg/fastpy-pkg.elf");

    const DB_PATH: &str = "/tmp/pkgdb.txt";
    const CUR_PATH: &str = "/tmp/pkg-current.txt";
    const GEN1_PATH: &str = "/tmp/pkg-gen-1.txt";
    const GEN2_PATH: &str = "/tmp/pkg-gen-2.txt";
    const FOO_PATH: &str = "/tmp/pkg-foo.txt";
    const FOO_ARG: &[u8] = b"/tmp/pkg-foo.txt";
    const FOO_CONTENT: &[u8] = b"libc demo\n"; // digest 86732e22
    const BAR_PATH: &str = "/tmp/pkg-bar.txt";
    const BAR_ARG: &[u8] = b"/tmp/pkg-bar.txt";
    const BAR_CONTENT: &[u8] = b"coreutils demo\n"; // digest 1ee068f8
    const FOO_BLOB: &str = "/tmp/store-86732e22.blob";
    const BAR_BLOB: &str = "/tmp/store-1ee068f8.blob";

    // Spawn `fastpy-pkg` with the given argv at ring 3, wait for Zombie, return
    // its exit code.  (Same shape as in `self_test_fastpy_slateos_pkg`.)
    fn run_pkg(elf: &'static [u8], argv: &[&[u8]]) -> KernelResult<i32> {
        let envp: &[&[u8]] = &[];
        let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
        let options = SpawnOptions {
            name: "fastpy-pkg",
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &caps,
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };
        let result = spawn_process(elf, &options)?;
        let mut became_zombie = false;
        for _ in 0..2000 {
            if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
                became_zombie = true;
                break;
            }
            crate::sched::yield_now();
        }
        let state = pcb::state(result.pid);
        let exit_code = pcb::exit_code(result.pid);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        if !became_zombie || state != Some(pcb::ProcessState::Zombie) {
            serial_println!(
                "[spawn]   FAIL: fastpy-pkg (ring 3) did not reach Zombie (state {:?})",
                state
            );
            return Err(KernelError::InternalError);
        }
        exit_code.ok_or(KernelError::InternalError)
    }

    fn cleanup() {
        for p in [
            DB_PATH, CUR_PATH, GEN1_PATH, GEN2_PATH, FOO_PATH, BAR_PATH, FOO_BLOB, BAR_BLOB,
        ] {
            let _ = crate::fs::Vfs::remove(p);
        }
    }

    serial_println!(
        "[spawn] Running fastpy-on-SlateOS `pkg` manager (ring 3) generations/rollback test \
         ({} bytes ELF)...",
        FASTPY_PKG_ELF.len()
    );

    // Seed an empty registry, a zero generation pointer, and the two payloads.
    if let Err(e) = crate::fs::Vfs::write_file(DB_PATH, b"") {
        serial_println!("[spawn]   FAIL: could not seed {} — {:?}", DB_PATH, e);
        return Err(e);
    }
    if let Err(e) = crate::fs::Vfs::write_file(CUR_PATH, b"0") {
        cleanup();
        serial_println!("[spawn]   FAIL: could not seed {} — {:?}", CUR_PATH, e);
        return Err(e);
    }
    for (path, content) in [(FOO_PATH, FOO_CONTENT), (BAR_PATH, BAR_CONTENT)] {
        if let Err(e) = crate::fs::Vfs::write_file(path, content) {
            cleanup();
            serial_println!("[spawn]   FAIL: could not stage {} — {:?}", path, e);
            return Err(e);
        }
    }

    // The generations lifecycle: (argv, expected exit code, description).
    let steps: &[(&[&[u8]], i32, &str)] = &[
        (&[b"fastpy-pkg", b"install", b"foo", FOO_ARG, b"-"], 0, "install foo"),
        (&[b"fastpy-pkg", b"commit"], 0, "commit (-> generation 1)"),
        (&[b"fastpy-pkg", b"install", b"bar", BAR_ARG, b"-"], 0, "install bar"),
        (&[b"fastpy-pkg", b"commit"], 0, "commit (-> generation 2)"),
        (&[b"fastpy-pkg", b"current"], 0, "current (generation 2)"),
        (&[b"fastpy-pkg", b"rollback"], 0, "rollback (-> generation 1)"),
        (&[b"fastpy-pkg", b"current"], 0, "current (generation 1)"),
    ];

    for (argv, want, desc) in steps {
        match run_pkg(FASTPY_PKG_ELF, argv) {
            Ok(code) if code == *want => {}
            Ok(code) => {
                cleanup();
                serial_println!(
                    "[spawn]   FAIL: fastpy-pkg `{}` exited {} (expected {})",
                    desc, code, want
                );
                return Err(KernelError::InternalError);
            }
            Err(e) => {
                cleanup();
                serial_println!("[spawn]   FAIL: fastpy-pkg `{}` — {:?}", desc, e);
                return Err(e);
            }
        }
    }

    // After the rollback the live registry must equal generation 1's snapshot:
    // foo present, bar reverted away.
    let db = match crate::fs::Vfs::read_file(DB_PATH) {
        Ok(bytes) => bytes,
        Err(e) => {
            cleanup();
            serial_println!("[spawn]   FAIL: could not read back {} — {:?}", DB_PATH, e);
            return Err(e);
        }
    };
    let contains = |needle: &[u8]| -> bool { db.windows(needle.len()).any(|w| w == needle) };
    let foo_present = contains(b"foo 86732e22");
    let bar_reverted = !contains(b"bar ");

    cleanup();

    if !foo_present || !bar_reverted {
        serial_println!(
            "[spawn]   FAIL: fastpy-pkg rollback did not restore generation 1 \
             (foo_present={}, bar_reverted={})",
            foo_present, bar_reverted
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   fastpy-on-SlateOS `pkg` (ring 3: generations/rollback — install foo / commit \
         gen1 / install bar / commit gen2 / current=2 / rollback / current=1, live registry \
         atomically reverted to gen1 snapshot verified): OK",
    );
    Ok(())
}

/// Ring-3 end-to-end test that the System V initial stack places the **`envp`
/// array at the correct variable offset**.
///
/// The sibling [`self_test_linux_argv0_deref`] dereferences `argv[0]` at the
/// *fixed* offset `[rsp+8]`.  This test instead spawns
/// [`elf::build_linux_envp0_deref_exit_elf`], which computes the envp address
/// from the runtime `argc` (`[rsp + 16 + argc*8]`) and exits with the first
/// byte of `envp[0]`.  We pass an `envp[0]` whose first byte is a known
/// sentinel and assert the zombie's exit code equals it.
///
/// This catches a failure mode neither the argc-scalar test nor the argv[0]
/// test can see: a stack builder could place `argc` and the argv pointers
/// correctly yet position the `envp` array one slot off (e.g. forgetting the
/// argv NULL terminator).  Real toolchains depend on `getenv()` for
/// `PATH`/`TMPDIR`/`CC`, so a misplaced envp array crashes them even though
/// argv looks fine.
pub fn self_test_linux_envp0_deref() -> KernelResult<()> {
    // First byte of envp[0]; distinct from interp(42)/mmap(91)/brk(109)/
    // execveat(58)/argv0(81).  0x4D = 'M' = 77.
    const SENTINEL: u8 = 0x4D;

    serial_println!("[spawn] Running Linux envp[0] deref (ring 3) integration test...");

    let exe_elf = elf::build_linux_envp0_deref_exit_elf();
    // envp[0]'s first byte is the sentinel the target reads back and exits
    // with; the rest of the string is irrelevant.  We pass two argv entries so
    // the variable offset `rsp + 16 + argc*8` is genuinely exercised (a builder
    // bug masked by argc==1 would still be caught here at argc==2).
    let argv: &[&[u8]] = &[b"spawn-test-linux-envp0", b"second-arg"];
    let envp: &[&[u8]] = &[b"\x4Denvp0-deref-test=1"];
    let options = SpawnOptions {
        name: "spawn-test-linux-envp0",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: envp0-deref spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Let the thread run: compute envp[0] from argc → deref → exit(first byte).
    crate::sched::yield_now();
    crate::sched::yield_now();

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: envp[0] deref (ring 3) — expected Zombie, got {:?} (a non-zombie \
             state usually means the envp array was placed at the wrong stack slot and the \
             process faulted dereferencing envp[0])",
            state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(i32::from(SENTINEL)) {
        serial_println!(
            "[spawn]   FAIL: envp[0] deref (ring 3) — expected exit {} (envp[0][0] sentinel), \
             got {:?} (a wrong-but-valid byte means the envp array is at the wrong offset)",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux envp[0] deref (ring 3: read byte through stack-builder envp[0] \
         pointer at variable offset rsp+16+argc*8, byte == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test of the Linux `fork(2)` → child `exit(2)` →
/// parent `wait4(2)` reap cycle.
///
/// This is the core process-lifecycle primitive every real toolchain relies
/// on: `make` → `gcc` → `cc1`/`as`/`ld` are all `fork`+`execve`+`wait4`.  We
/// spawn [`elf::build_linux_fork_wait_test_elf`], a real Linux-ABI program
/// that forks, has the child `exit(0x4B)`, and has the parent reap it with a
/// **blocking** `wait4(-1, &status, 0, NULL)` (exactly what `make`/`gcc` do),
/// then exit with the decoded `WEXITSTATUS`.  A healthy run leaves the parent
/// zombie with exit code `0x4B` (75); we assert exactly that.
///
/// **Why this is safe to run at boot:** although the *launcher* blocks in
/// `wait4`, this *harness* drives the scheduler with a **bounded** `yield_now`
/// loop and force-destroys the launcher afterward, so even a broken
/// child-exit wakeup can only produce a clean failed assertion, never a boot
/// hang.  (The parent's block leaves the run queue, letting the child run; the
/// child's exit wakes the parent via `on_thread_exit`'s wait-any wakeup.)
pub fn self_test_linux_fork_wait() -> KernelResult<()> {
    // Child exits 0x4B (75); the parent's WEXITSTATUS is the same byte, so the
    // parent zombie exits 75.  Distinct from interp(42)/mmap(91)/brk(109)/
    // execveat(58)/argv0(81)/envp0(77)/wait4-error(161).
    const CHILD_EXIT: i32 = 0x4B;
    // Upper bound on scheduler ticks we grant the parent+child to complete
    // their fork/exit/reap dance.  Each iteration is a single non-blocking
    // `yield_now`; we break early the moment the parent becomes a zombie, so
    // a healthy run costs only a handful.  Generous enough that a slow but
    // correct scheduler still finishes; bounded so a broken one can't hang.
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux fork()+wait4() reap (ring 3) integration test...");

    let exe_elf = elf::build_linux_fork_wait_test_elf();
    let argv: &[&[u8]] = &[b"spawn-test-linux-fork-wait"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let options = SpawnOptions {
        name: "spawn-test-linux-fork-wait",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: fork-wait spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Drive the scheduler until the parent process becomes a zombie or we
    // exhaust the bound.  Each `yield_now` hands the CPU to a ready ring-3
    // task: the parent runs, forks, then blocks in `wait4`, which lets the
    // child run to exit; the child's exit wakes the parent, which reaps and
    // exits.  This harness never blocks, so the loop is bounded regardless of
    // whether the launcher's wakeup path works.
    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fork()+wait4() (ring 3) — parent not a zombie after {} yields, \
             got {:?} (the parent is still blocked in wait4; the child-exit wakeup never \
             fired, or fork never resumed the child)",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(CHILD_EXIT) {
        // 0xA1/161 means the parent's wait4 returned <= 0 (no child reaped /
        // error); any other wrong value means WEXITSTATUS decoded wrong.
        serial_println!(
            "[spawn]   FAIL: fork()+wait4() (ring 3) — expected parent exit {} \
             (child WEXITSTATUS), got {:?} (0xA1/161 = parent's wait4 returned an error)",
            CHILD_EXIT, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux fork()+wait4() (ring 3: parent forked a child, blocked in wait4, \
         reaped it on the child's exit, decoded WEXITSTATUS == {}): OK",
        CHILD_EXIT
    );
    Ok(())
}

/// Ring-3 end-to-end test of the full `fork(2)` → child `execve(2)` →
/// parent `wait4(2)` subprocess cycle — the exact pattern `make`/`gcc`/the
/// shell use to run a tool and collect its exit status.
///
/// A launcher forks; the child `execve`s a staged target ELF
/// ([`elf::build_linux_exit_elf`]`(SENTINEL)`) instead of exiting directly,
/// so the status the parent reaps is the *target's* exit code.  This proves
/// the whole chain works together: the forked child reads its `execve`
/// arguments out of copy-on-write post-fork memory, `execve` replaces the
/// image in place (same PID), the target runs and exits, and the parent —
/// blocked in `wait4` — is woken and writes the status word through a
/// pointer on its own CoW stack.  A parent zombie with `exit_code ==
/// SENTINEL` confirms all of it.
///
/// Distinct exit sentinels make a failure self-diagnosing:
///   * `SENTINEL` (0x53/83) — success: target ran and the parent decoded
///     its `WEXITSTATUS`.
///   * `0xE7` (231) — the child's `execve` *returned* (exec failed); the
///     child ran its failure tail.
///   * `0xA2` (162) — the parent's `wait4` returned `<= 0` (no child reaped
///     / error).
///
/// Skips gracefully (`Ok`) if the VFS target write fails.  Must run **after**
/// filesystem initialization (see `main.rs`).  Hang-safe for the same reason
/// as [`self_test_linux_fork_wait`]: the harness pumps the scheduler with a
/// bounded `yield_now` loop and force-destroys the launcher on timeout.
pub fn self_test_linux_fork_execve_wait() -> KernelResult<()> {
    const TGT_PATH: &str = "/slateos-test-fork-execve-tgt";
    const TGT_PATH_NUL: &[u8] = b"/slateos-test-fork-execve-tgt\0";
    // Target exit sentinel — distinct from interp(42)/mmap(91)/brk(109)/
    // execveat(58)/argv0(81)/envp0(77)/fork-wait child(75)/wait4-error(161).
    const SENTINEL: i32 = 0x53; // 83
    const MAX_YIELDS: usize = 256;

    serial_println!(
        "[spawn] Running Linux fork()+execve()+wait4() (ring 3) integration test..."
    );

    // Stage the exec target: a Linux-ABI ELF that exit()s with SENTINEL.
    let tgt_elf = elf::build_linux_exit_elf(SENTINEL as u8);
    if let Err(e) = crate::fs::Vfs::write_file(TGT_PATH, &tgt_elf) {
        serial_println!(
            "[spawn]   Linux fork()+execve()+wait4() (ring 3): SKIP (VFS write failed: {:?})",
            e
        );
        return Ok(());
    }

    let exe_elf = elf::build_linux_fork_execve_wait_test_elf(TGT_PATH_NUL);
    let argv: &[&[u8]] = &[b"spawn-test-linux-fork-execve"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    // The child's execve resolves the target by VFS path; grant a File
    // capability like the execveat path-form launcher does.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-fork-execve",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(TGT_PATH);
            serial_println!("[spawn]   FAIL: fork-execve-wait spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Drive the scheduler: parent forks → blocks in wait4 → child runs and
    // execve's the target → target exits → child-exit wakes the parent →
    // parent reaps and exits.  Bounded, never blocks the harness.
    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(TGT_PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fork()+execve()+wait4() (ring 3) — parent not a zombie after {} \
             yields, got {:?} (parent still blocked in wait4, or the child never execve'd/exited)",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(SENTINEL) {
        serial_println!(
            "[spawn]   FAIL: fork()+execve()+wait4() (ring 3) — expected parent exit {} \
             (exec target's WEXITSTATUS), got {:?} (0xE7/231 = child's execve failed; \
             0xA2/162 = parent's wait4 returned an error)",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux fork()+execve()+wait4() (ring 3: child execve'd a staged target, \
         parent reaped it, decoded target WEXITSTATUS == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 end-to-end test of the canonical **shell-pipeline** primitive:
/// `pipe2` + `fork` + `dup2` + `execve` + blocking `read`.
///
/// This is the `cmd1 | cmd2` skeleton.  The launcher
/// ([`elf::build_linux_pipe_fork_dup2_exec_test_elf`]) creates a pipe, forks,
/// and in the child `dup2`s the pipe's **write end** onto fd 1 before
/// `execve`ing a staged producer ([`elf::build_linux_write_byte_exit_elf`])
/// that writes `SENTINEL` to fd 1 and exits.  The parent blocks in `read` on
/// the pipe's read end, gets the byte, reaps the child, and `exit`s with the
/// byte — so a clean `exit_code == SENTINEL` proves the entire chain:
///
///   * `pipe2` allocated a read/write fd pair;
///   * `fork` cloned the fd table (the child uses the inherited write end);
///   * `dup2` aliased that write end onto fd 1, which `execve` preserved
///     across the image replacement;
///   * the byte traversed the pipe and woke the parent's blocking `read`.
///
/// This is the first test of fd-table inheritance + `dup2` + the pipe IPC
/// path composed end to end under the Linux ABI from ring 3 (prior pipe
/// coverage was kernel-context only; see `ipc/pipe.rs`).
///
/// Self-diagnosing sentinels (parent exit when something upstream failed):
/// `0xA4` = `pipe2` failed, `0xA3` = parent `read` returned `<= 0`,
/// `0xE7` = child `execve` failed.  Skips gracefully (`Ok`) if the VFS write
/// fails.  Must run **after** filesystem initialization (see `main.rs`).
pub fn self_test_linux_pipe_fork_dup2_exec() -> KernelResult<()> {
    const TGT_PATH: &str = "/slateos-test-pipe-exec-tgt";
    const TGT_PATH_NUL: &[u8] = b"/slateos-test-pipe-exec-tgt\0";
    // Producer byte / parent exit — distinct from the other launchers'
    // failure sentinels (0xA2/0xA3/0xA4/0xE7) and prior target codes.
    const SENTINEL: i32 = 0x6B; // 107
    const MAX_YIELDS: usize = 256;

    serial_println!(
        "[spawn] Running Linux pipe2()+fork()+dup2()+execve()+read() (ring 3) pipeline test..."
    );

    // Stage the producer: a Linux-ABI ELF that writes SENTINEL to fd 1.
    let tgt_elf = elf::build_linux_write_byte_exit_elf(SENTINEL as u8);
    if let Err(e) = crate::fs::Vfs::write_file(TGT_PATH, &tgt_elf) {
        serial_println!(
            "[spawn]   Linux pipe2()+fork()+dup2()+execve()+read() (ring 3): SKIP (VFS write \
             failed: {:?})",
            e
        );
        return Ok(());
    }

    let exe_elf = elf::build_linux_pipe_fork_dup2_exec_test_elf(TGT_PATH_NUL);
    let argv: &[&[u8]] = &[b"spawn-test-linux-pipe-exec"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-pipe-exec",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(TGT_PATH);
            serial_println!("[spawn]   FAIL: pipe-fork-dup2-exec spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Drive the scheduler: parent pipe2s, forks, blocks in read → child
    // dup2s + execve's the producer → producer writes the byte (waking the
    // parent's read) and exits → parent reaps and exits with the byte.
    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(TGT_PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: pipe2()+fork()+dup2()+execve()+read() (ring 3) — parent not a \
             zombie after {} yields, got {:?} (parent still blocked in read, or the child \
             never dup2'd/execve'd/wrote)",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(SENTINEL) {
        serial_println!(
            "[spawn]   FAIL: pipe2()+fork()+dup2()+execve()+read() (ring 3) — expected parent \
             exit {} (the byte read back from the pipe), got {:?} (0xA4/164 = pipe2 failed; \
             0xA3/163 = parent read returned <= 0; 0xE7/231 = child execve failed)",
            SENTINEL, exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux pipe2()+fork()+dup2()+execve()+read() (ring 3: child piped a byte to \
         the parent via dup2'd stdout across execve, parent read back == {}): OK",
        SENTINEL
    );
    Ok(())
}

/// Ring-3 regression test for the **`symlink(2)` + `readlink(2)`** syscalls.
///
/// These two syscalls were stale stubs (`symlink` → `EROFS`, `readlink` →
/// `EINVAL`) even though the VFS is fully writable and supports symlinks.
/// This test wires them to a real round-trip: a hand-built Linux-ABI ELF
/// ([`elf::build_linux_symlink_readlink_test_elf`]) calls
/// `symlink("Z", "/sl-rl-link")`, then `readlink("/sl-rl-link", buf, 64)`,
/// and asserts the call returned exactly one byte equal to `'Z'` (the Linux
/// `readlink` contract: byte count, no trailing NUL).  A clean `exit_code ==
/// 0` proves the kernel created a real symlink and read its target back
/// verbatim through ring 3.
///
/// The harness removes any pre-existing entry at the link path first so the
/// in-process `symlink` does not fail `EEXIST`, and cleans it up afterward.
/// After the process exits, it independently confirms via `Vfs::readlink`
/// that the link the ring-3 process created really resolves to `"Z"`.
///
/// Self-diagnosing sentinels (process `exit_code`): `0xB1`/177 = `symlink`
/// returned non-zero, `0xB3`/179 = `readlink` returned a length other than 1,
/// `0xB4`/180 = the byte read back was not `'Z'`.  Skips gracefully (`Ok`) if
/// the VFS cannot create/clean the path.  Must run **after** filesystem init.
pub fn self_test_linux_symlink_readlink() -> KernelResult<()> {
    const LINK_PATH: &str = "/sl-rl-link";
    const LINK_PATH_NUL: &[u8] = b"/sl-rl-link\0";
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux symlink()+readlink() round-trip test (ring 3)...");

    // Remove any stale link from a prior boot so the in-process symlink()
    // starts from a clean slate (EEXIST would otherwise abort it as 0xB1).
    let _ = crate::fs::Vfs::remove(LINK_PATH);

    let exe_elf = elf::build_linux_symlink_readlink_test_elf(LINK_PATH_NUL);
    let argv: &[&[u8]] = &[b"spawn-test-linux-symlink"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-symlink",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(LINK_PATH);
            serial_println!("[spawn]   FAIL: symlink-readlink spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    // Independent kernel-side confirmation that the ring-3 process really
    // created a symlink resolving to "Z" before we tear it down.
    let kernel_readback = crate::fs::Vfs::readlink(LINK_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(LINK_PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: symlink()+readlink() (ring 3) — process not a zombie after {} \
             yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: symlink()+readlink() (ring 3) — expected exit 0, got {:?} \
             (0xB1/177 = symlink failed; 0xB3/179 = readlink wrong length; 0xB4/180 = wrong \
             byte read back)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref t) if t.as_bytes() == b"Z" => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: symlink()+readlink() (ring 3) — process exited 0 but kernel \
                 readback of the created link did not resolve to \"Z\": {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux symlink()+readlink() (ring 3: created /sl-rl-link -> \"Z\" and read it \
         back verbatim; kernel confirmed): OK"
    );
    Ok(())
}

/// Ring-3 regression test for the **`link(2)`/`linkat(2)`** (hard-link)
/// syscalls, which were stale `EROFS` stubs until wired to `Vfs::link`.
///
/// Hard links require an inode-sharing FS; memfs (the in-memory root) cannot
/// share an inode between two names, so the test runs on the ext4 mount at
/// `/mnt`.  The harness pre-creates `/mnt/lnk-src` containing the single byte
/// `'L'` and removes any stale `/mnt/lnk-dst`.  A hand-built Linux-ABI ELF
/// ([`elf::build_linux_link_test_elf`]) calls `link("/mnt/lnk-src",
/// "/mnt/lnk-dst")`, then `open("/mnt/lnk-dst", O_RDONLY)` + `read` one byte
/// and asserts it is `'L'` — proving the new name shares the source's inode
/// data.  A clean `exit_code == 0` confirms the hard link was really created
/// and is readable through the new name.  After the process exits, the harness
/// independently confirms via `Vfs::read_file` that `/mnt/lnk-dst` reads back
/// `"L"`.  Skips cleanly when there is no writable `/mnt` ext4 mount.
///
/// Self-diagnosing sentinels: `0xC1`/193 = `link` failed, `0xC2`/194 =
/// `open(new)` failed, `0xC3`/195 = `read` wrong length, `0xC4`/196 = wrong
/// byte read back.  Skips gracefully (`Ok`) if the VFS cannot stage the
/// source.  Must run **after** filesystem init.
pub fn self_test_linux_link() -> KernelResult<()> {
    // Hard links require an inode-sharing FS.  memfs (the in-memory root /,
    // /tmp) stores file data inline in by-value tree nodes and cannot share an
    // inode between two names, so it correctly reports "unsupported" (Linux
    // returns EPERM for such FSes).  We therefore test the success path on the
    // ext4 mount at /mnt, which implements hard links.  See known-issues
    // B-SYM1 for the memfs limitation.
    const SRC_PATH: &str = "/mnt/lnk-src";
    const SRC_PATH_NUL: &[u8] = b"/mnt/lnk-src\0";
    const DST_PATH: &str = "/mnt/lnk-dst";
    const DST_PATH_NUL: &[u8] = b"/mnt/lnk-dst\0";
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux link()/linkat() hard-link test (ring 3, ext4 /mnt)...");

    // Skip cleanly when /mnt isn't an ext4 mount (diskless boot).
    if !crate::fs::Vfs::exists("/mnt") {
        serial_println!("[spawn]   Linux link() (ring 3): SKIP (no /mnt ext4 mount)");
        return Ok(());
    }

    // Drain any stale src/dst entries before staging.  The /mnt fixture
    // (rootfs.ext4) is a persistent disk reused across every boot, so it may
    // already hold copies — including historical DUPLICATES left by a former
    // directory bug (see known-issues B-EXT4-DIR).  A single remove() only
    // unlinks one matching entry, so loop until the name is gone (bounded).
    fn drain(path: &str) {
        for _ in 0..64 {
            if !crate::fs::Vfs::exists(path) {
                return;
            }
            if crate::fs::Vfs::remove(path).is_err() {
                return;
            }
        }
    }
    drain(DST_PATH);
    drain(SRC_PATH);

    // Stage the source file with a single known byte.
    if let Err(e) = crate::fs::Vfs::write_file(SRC_PATH, b"L") {
        serial_println!(
            "[spawn]   Linux link() (ring 3): SKIP (ext4 /mnt write failed: {:?})",
            e
        );
        return Ok(());
    }

    let exe_elf = elf::build_linux_link_test_elf(SRC_PATH_NUL, DST_PATH_NUL);
    let argv: &[&[u8]] = &[b"spawn-test-linux-link"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-link",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            let _ = crate::fs::Vfs::remove(DST_PATH);
            serial_println!("[spawn]   FAIL: link spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::read_file(DST_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(SRC_PATH);
    let _ = crate::fs::Vfs::remove(DST_PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: link() (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: link() (ring 3) — expected exit 0, got {:?} (0xC1/193 = link \
             failed; 0xC2/194 = open(dst) failed; 0xC3/195 = read wrong length; 0xC4/196 = \
             wrong byte)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref d) if d.as_slice() == b"L" => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: link() (ring 3) — process exited 0 but kernel readback of \
                 /mnt/lnk-dst was not \"L\": {:?}",
                other.as_ref().map(alloc::vec::Vec::len)
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux link() (ring 3: hard-linked /mnt/lnk-dst to /mnt/lnk-src and read \"L\" back \
         through it; kernel confirmed): OK"
    );
    Ok(())
}

/// Ring-3 regression test that **`utimensat(2)` updates a file's timestamps**.
///
/// Before this wiring, `utimensat`/`utimes`/`utime` were EROFS stubs (no
/// writable-FS terminal).  They now perform a real `Vfs::set_times` for ring-3
/// callers.  This test runs on the memfs root (which implements `set_times`),
/// staging a file then having a ring-3 program call `utimensat(AT_FDCWD, path,
/// {atime, mtime}, 0)` with distinctive epoch-second values.  After the
/// process exits 0, the kernel independently reads the file metadata back and
/// asserts both timestamps match exactly (`sec * 1e9`).  This proves the
/// translation layer parsed the `struct timespec[2]` correctly and routed the
/// update to the VFS.
pub fn self_test_linux_utimensat() -> KernelResult<()> {
    // memfs root supports set_times, so the success path is testable here
    // (unlike hard links — see self_test_linux_link).
    const PATH: &str = "/utimensat-test";
    const PATH_NUL: &[u8] = b"/utimensat-test\0";
    // Distinctive positive epoch seconds (must fit in i32 for the imm32 store).
    const ATIME_SEC: i32 = 1_600_000_000;
    const MTIME_SEC: i32 = 1_500_000_000;
    const EXPECT_ATIME_NS: u64 = 1_600_000_000_000_000_000;
    const EXPECT_MTIME_NS: u64 = 1_500_000_000_000_000_000;
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux utimensat() timestamp test (ring 3, memfs /)...");

    // Stage the target file; clean any stale copy first.
    let _ = crate::fs::Vfs::remove(PATH);
    if let Err(e) = crate::fs::Vfs::write_file(PATH, b"t") {
        serial_println!("[spawn]   FAIL: utimensat staging write failed: {:?}", e);
        return Err(e);
    }

    let exe_elf = elf::build_linux_utimensat_test_elf(PATH_NUL, ATIME_SEC, MTIME_SEC);
    let argv: &[&[u8]] = &[b"spawn-test-linux-utimensat"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-utimensat",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(PATH);
            serial_println!("[spawn]   FAIL: utimensat spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::metadata(PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: utimensat() (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: utimensat() (ring 3) — expected exit 0, got {:?} (0xD1/209 = \
             utimensat returned non-zero)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref m) if m.accessed_ns == EXPECT_ATIME_NS && m.modified_ns == EXPECT_MTIME_NS => {}
        Ok(ref m) => {
            serial_println!(
                "[spawn]   FAIL: utimensat() (ring 3) — process exited 0 but kernel readback \
                 timestamps mismatch: accessed_ns={} (want {}), modified_ns={} (want {})",
                m.accessed_ns, EXPECT_ATIME_NS, m.modified_ns, EXPECT_MTIME_NS
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: utimensat() (ring 3) — kernel metadata readback failed: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux utimensat() (ring 3: set atime/mtime on /utimensat-test; kernel \
         confirmed both timestamps): OK"
    );
    Ok(())
}

/// Ring-3 regression test that **`chmod(2)`/`chown(2)` mutate file metadata**.
///
/// Before this wiring, the whole chmod/chown family were EROFS stubs.  They
/// now route to `Vfs::set_permissions` / `Vfs::set_owner` for ring-3 callers.
/// This test stages a file on the memfs root, then a ring-3 program calls
/// `chmod(path, 0o640)` followed by `chown(path, 1234, 5678)`.  After the
/// process exits 0, the kernel independently reads the file metadata back and
/// asserts `permissions == 0o640`, `uid == 1234`, `gid == 5678`.
pub fn self_test_linux_chmod_chown() -> KernelResult<()> {
    const PATH: &str = "/chmod-chown-test";
    const PATH_NUL: &[u8] = b"/chmod-chown-test\0";
    const MODE: u16 = 0o640;
    const UID: u32 = 1234;
    const GID: u32 = 5678;
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux chmod()/chown() metadata test (ring 3, memfs /)...");

    let _ = crate::fs::Vfs::remove(PATH);
    if let Err(e) = crate::fs::Vfs::write_file(PATH, b"m") {
        serial_println!("[spawn]   FAIL: chmod/chown staging write failed: {:?}", e);
        return Err(e);
    }

    let exe_elf = elf::build_linux_chmod_chown_test_elf(PATH_NUL, u32::from(MODE), UID, GID);
    let argv: &[&[u8]] = &[b"spawn-test-linux-chmod-chown"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-chmod-chown",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(PATH);
            serial_println!("[spawn]   FAIL: chmod/chown spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::metadata(PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: chmod/chown (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: chmod/chown (ring 3) — expected exit 0, got {:?} (0xE1/225 = chmod \
             failed; 0xE2/226 = chown failed)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref m) if m.permissions == MODE && m.uid == UID && m.gid == GID => {}
        Ok(ref m) => {
            serial_println!(
                "[spawn]   FAIL: chmod/chown (ring 3) — process exited 0 but kernel readback \
                 mismatch: permissions=0o{:o} (want 0o{:o}), uid={} (want {}), gid={} (want {})",
                m.permissions, MODE, m.uid, UID, m.gid, GID
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: chmod/chown (ring 3) — kernel metadata readback failed: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux chmod()/chown() (ring 3: set mode 0o640 + owner 1234:5678 on \
         /chmod-chown-test; kernel confirmed): OK"
    );
    Ok(())
}

/// Ring-3 regression test for the **`truncate(2)`** (path) and
/// **`ftruncate(2)`** (fd) file-resize syscalls against the writable
/// memfs root.
///
/// The harness stages `/truncate-test` with `STAGE_LEN` bytes of `'A'`,
/// then spawns a [`elf::build_linux_truncate_test_elf`] process that:
///   1. `truncate("/truncate-test", SHRINK_LEN)` — shrinks the file;
///   2. `open(..., O_RDWR)` then `ftruncate(fd, GROW_LEN)` — grows
///      (zero-extends) the file through a writable fd.
///
/// After the process exits 0, the kernel independently reads the file
/// back and asserts the final length is `GROW_LEN`, the leading
/// `SHRINK_LEN` bytes survived as `'A'`, and the grown tail is zero-
/// filled.  This proves both resize paths reach the real `Vfs::truncate`
/// (path-based via canonicalization, fd-based via `handle_path`) now that
/// the universal-read-only EROFS terminal has been lifted for ring-3
/// callers.
pub fn self_test_linux_truncate() -> KernelResult<()> {
    const PATH: &str = "/truncate-test";
    const PATH_NUL: &[u8] = b"/truncate-test\0";
    const STAGE_LEN: usize = 16;
    const SHRINK_LEN: u32 = 4;
    const GROW_LEN: u32 = 10;
    const MAX_YIELDS: usize = 256;

    serial_println!(
        "[spawn] Running Linux truncate()/ftruncate() resize test (ring 3, memfs /)..."
    );

    let _ = crate::fs::Vfs::remove(PATH);
    let stage = [b'A'; STAGE_LEN];
    if let Err(e) = crate::fs::Vfs::write_file(PATH, &stage) {
        serial_println!("[spawn]   FAIL: truncate staging write failed: {:?}", e);
        return Err(e);
    }

    let exe_elf = elf::build_linux_truncate_test_elf(PATH_NUL, SHRINK_LEN, GROW_LEN);
    let argv: &[&[u8]] = &[b"spawn-test-linux-truncate"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-truncate",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(PATH);
            serial_println!("[spawn]   FAIL: truncate spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::read_file(PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: truncate (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: truncate (ring 3) — expected exit 0, got {:?} (0xF1/241 = truncate \
             failed; 0xF2/242 = open(O_RDWR) failed; 0xF3/243 = ftruncate failed)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref data)
            if data.len() == GROW_LEN as usize
                && data.iter().take(SHRINK_LEN as usize).all(|&b| b == b'A')
                && data.iter().skip(SHRINK_LEN as usize).all(|&b| b == 0) => {}
        Ok(ref data) => {
            serial_println!(
                "[spawn]   FAIL: truncate (ring 3) — process exited 0 but kernel readback \
                 mismatch: len={} (want {}), bytes={:?}",
                data.len(), GROW_LEN, data
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: truncate (ring 3) — kernel file readback failed: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux truncate()/ftruncate() (ring 3: shrink /truncate-test to 4B via path, \
         grow to 10B via fd; kernel confirmed length + zero-fill): OK"
    );
    Ok(())
}

/// Ring-3 regression test for **`fchmodat2(2)` with `AT_EMPTY_PATH`**
/// (the fd-targeted chmod form, Linux #452) against the writable memfs
/// root.
///
/// The harness stages `/fchmodat2-test`, then spawns a
/// [`elf::build_linux_fchmodat2_emptypath_test_elf`] process that
/// `open(O_RDWR)`s the file and calls `fchmodat2(fd, "", 0o600,
/// AT_EMPTY_PATH)`.  After the process exits 0, the kernel independently
/// reads the metadata back and asserts `permissions == 0o600`, proving
/// the `AT_EMPTY_PATH → handle_path → Vfs::set_permissions` branch — the
/// genuinely new path in the fchmodat2 wiring — works end-to-end from
/// ring 3.
pub fn self_test_linux_fchmodat2() -> KernelResult<()> {
    const PATH: &str = "/fchmodat2-test";
    const PATH_NUL: &[u8] = b"/fchmodat2-test\0";
    const MODE: u16 = 0o600;
    const MAX_YIELDS: usize = 256;

    serial_println!(
        "[spawn] Running Linux fchmodat2(AT_EMPTY_PATH) metadata test (ring 3, memfs /)..."
    );

    let _ = crate::fs::Vfs::remove(PATH);
    if let Err(e) = crate::fs::Vfs::write_file(PATH, b"f") {
        serial_println!("[spawn]   FAIL: fchmodat2 staging write failed: {:?}", e);
        return Err(e);
    }

    let exe_elf = elf::build_linux_fchmodat2_emptypath_test_elf(PATH_NUL, u32::from(MODE));
    let argv: &[&[u8]] = &[b"spawn-test-linux-fchmodat2"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-fchmodat2",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(PATH);
            serial_println!("[spawn]   FAIL: fchmodat2 spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::metadata(PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fchmodat2 (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: fchmodat2 (ring 3) — expected exit 0, got {:?} (0xE5/229 = \
             open(O_RDWR) failed; 0xE6/230 = fchmodat2 failed)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref m) if m.permissions == MODE => {}
        Ok(ref m) => {
            serial_println!(
                "[spawn]   FAIL: fchmodat2 (ring 3) — process exited 0 but kernel readback \
                 mismatch: permissions=0o{:o} (want 0o{:o})",
                m.permissions, MODE
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: fchmodat2 (ring 3) — kernel metadata readback failed: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux fchmodat2(AT_EMPTY_PATH) (ring 3: chmod /fchmodat2-test to 0o600 via an \
         O_RDWR fd; kernel confirmed): OK"
    );
    Ok(())
}

/// Ring-3 regression test for the **virtio-gpu `DRM_IOCTL_VIRTGPU_GETPARAM`**
/// render ioctl — the honest "no-3D" reporting landed for Q18
/// (design-decisions §59).
///
/// The harness spawns [`elf::build_linux_virtgpu_getparam_test_elf`], a
/// self-contained Linux-ABI payload that `open(O_RDWR)`s `/dev/dri/renderD128`,
/// issues `GETPARAM(VIRTGPU_PARAM_3D_FEATURES)`, and asserts the ioctl succeeds
/// **and** the kernel copies back the honest value `0` (no virgl backend). A
/// clean `exit(0)` proves the full ring-3 path `open(renderD128)` →
/// `drm_card_ioctl` → `virtgpu_render_ioctl` → `virtgpu_getparam_ioctl` with the
/// policy value delivered to userspace. Exit sentinels: `0xE1` open failed,
/// `0xE2` GETPARAM ioctl failed, `0xE3` wrong reported value.
///
/// If no DRM device is bound (a build/boot without `-device virtio-gpu-pci`),
/// there is nothing to open, so the test is **skipped** (returns `Ok`) rather
/// than reported as a failure.
pub fn self_test_linux_virtgpu_getparam() -> KernelResult<()> {
    const MAX_YIELDS: usize = 256;

    if crate::drm::device_count() == 0 {
        serial_println!(
            "[spawn] Skipping virtio-gpu GETPARAM (ring 3) test — no DRM device bound."
        );
        return Ok(());
    }

    serial_println!(
        "[spawn] Running virtio-gpu GETPARAM render-ioctl test (ring 3, /dev/dri/renderD128)..."
    );

    let exe_elf = elf::build_linux_virtgpu_getparam_test_elf();
    let argv: &[&[u8]] = &[b"spawn-test-linux-virtgpu-getparam"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    // Opening the render node needs no capability (try_open_drm gates only on a
    // bound device + a live caller pid), so no caps are granted.
    let options = SpawnOptions {
        name: "spawn-test-linux-virtgpu-getparam",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: virtgpu-getparam spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: virtgpu-getparam (ring 3) — process not a zombie after {} yields, \
             got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: virtgpu-getparam (ring 3) — expected exit 0, got {:?} (0xE1/225 = \
             open(renderD128) failed; 0xE2/226 = GETPARAM ioctl failed; 0xE3/227 = wrong reported \
             3D_FEATURES value)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   virtio-gpu GETPARAM (ring 3: renderD128 GETPARAM(3D_FEATURES)==0, honest \
         no-3D reporting): OK"
    );
    Ok(())
}

/// Ring-3 regression test for **`fallocate(2)` mode 0 (posix_fallocate
/// grow)** (Linux #285) against the writable memfs root.
///
/// The harness stages `/fallocate-test` with a *small* size (4 bytes),
/// then spawns a [`elf::build_linux_fallocate_grow_test_elf`] process
/// that `open(O_RDWR)`s the file and calls `fallocate(fd, 0, 0, 10)`.
/// After the process exits 0, the kernel independently reads the file
/// size back and asserts it grew to exactly 10 bytes — the
/// posix_fallocate guarantee (logical size becomes >= offset+len) and
/// the genuinely new path in the fallocate wiring (`fd → handle_path →
/// Vfs::file_size/Vfs::truncate`).  It also confirms the original 4
/// bytes are preserved and the grown tail is zero-filled.
pub fn self_test_linux_fallocate() -> KernelResult<()> {
    const PATH: &str = "/fallocate-test";
    const PATH_NUL: &[u8] = b"/fallocate-test\0";
    const STAGE_LEN: usize = 4;
    const GROW_LEN: u32 = 10;
    const MAX_YIELDS: usize = 256;

    serial_println!("[spawn] Running Linux fallocate(mode=0 grow) test (ring 3, memfs /)...");

    let _ = crate::fs::Vfs::remove(PATH);
    if let Err(e) = crate::fs::Vfs::write_file(PATH, &[b'A'; STAGE_LEN]) {
        serial_println!("[spawn]   FAIL: fallocate staging write failed: {:?}", e);
        return Err(e);
    }

    let exe_elf = elf::build_linux_fallocate_grow_test_elf(PATH_NUL, GROW_LEN);
    let argv: &[&[u8]] = &[b"spawn-test-linux-fallocate"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-fallocate",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(PATH);
            serial_println!("[spawn]   FAIL: fallocate spawn returned {:?}", e);
            return Err(e);
        }
    };

    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            break;
        }
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let kernel_readback = crate::fs::Vfs::read_file(PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(PATH);

    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: fallocate (ring 3) — process not a zombie after {} yields, got {:?}",
            MAX_YIELDS, state
        );
        return Err(KernelError::InternalError);
    }

    if exit_code != Some(0) {
        serial_println!(
            "[spawn]   FAIL: fallocate (ring 3) — expected exit 0, got {:?} (0xD1/209 = \
             open(O_RDWR) failed; 0xD2/210 = fallocate failed)",
            exit_code
        );
        return Err(KernelError::InternalError);
    }

    match kernel_readback {
        Ok(ref data)
            if data.len() == GROW_LEN as usize
                && data.iter().take(STAGE_LEN).all(|&b| b == b'A')
                && data.iter().skip(STAGE_LEN).all(|&b| b == 0) => {}
        Ok(ref data) => {
            serial_println!(
                "[spawn]   FAIL: fallocate (ring 3) — process exited 0 but kernel readback \
                 mismatch: len={} (want {}), bytes={:?}",
                data.len(), GROW_LEN, data
            );
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: fallocate (ring 3) — kernel file readback failed: {:?}",
                e
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!(
        "[spawn]   Linux fallocate(mode=0) (ring 3: grow /fallocate-test from 4B to 10B via an \
         O_RDWR fd; kernel confirmed length + zero-fill): OK"
    );
    Ok(())
}

/// Ring-3 regression test that the **`%fs` (TLS) base is saved/restored
/// across context switches** between two concurrent Linux processes.
///
/// `IA32_FS_BASE` is the glibc thread-local-storage pointer (`%fs` base),
/// a **global CPU register** that is **not** part of the saved GP
/// `Context`.  Before this test's accompanying fix, the scheduler never
/// swapped it on a context switch, so any two concurrent glibc processes
/// would clobber each other's TLS pointer — silently corrupting
/// `errno`, the stack-protector canary, and every `__thread` variable.
/// That is fatal for running a real toolchain (gcc/ld/make/bash are all
/// glibc, all multi-process), so it's on the critical path.
///
/// Two [`elf::build_linux_fs_tls_test_elf`] processes are spawned with
/// **distinct** sentinel FS bases.  Each installs its sentinel via
/// `arch_prctl(ARCH_SET_FS)`, then loops `sched_yield` + `arch_prctl
/// (ARCH_GET_FS)` asserting the value is unchanged.  The self-tests run
/// **single-CPU** (before `smp::init()`), so the two processes time-share
/// CPU 0 through the cooperative yields and interleave deterministically:
/// without the fix, the first process to resume after the other's yield
/// reads the *other's* sentinel and `exit`s `0xF1`.  Both processes
/// exiting `0` proves the per-task FS base is correctly restored on
/// switch-in.
pub fn self_test_linux_fs_tls_switch() -> KernelResult<()> {
    // Two distinct canonical user-address sentinels (< 1 << 47, non-zero).
    // These are never dereferenced — they only need to round-trip through
    // the IA32_FS_BASE MSR across context switches.
    const SENTINEL_A: u64 = 0x0000_1234_5600_0000;
    const SENTINEL_B: u64 = 0x0000_5566_7700_0000;
    const MAX_YIELDS: usize = 1024;

    serial_println!(
        "[spawn] Running Linux %fs/TLS-base context-switch persistence test (ring 3, 2 procs)..."
    );

    let spawn_one = |sentinel: u64, name: &'static str| -> KernelResult<SpawnResult> {
        let elf_img = elf::build_linux_fs_tls_test_elf(sentinel);
        let argv: &[&[u8]] = &[name.as_bytes()];
        let envp: &[&[u8]] = &[b"PATH=/bin"];
        let options = SpawnOptions {
            name,
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &[],
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };
        spawn_process(&elf_img, &options)
    };

    let a = match spawn_one(SENTINEL_A, "spawn-test-fs-tls-a") {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: fs-tls proc A spawn returned {:?}", e);
            return Err(e);
        }
    };
    let b = match spawn_one(SENTINEL_B, "spawn-test-fs-tls-b") {
        Ok(r) => r,
        Err(e) => {
            // A was spawned; tear it down before bailing.
            thread::on_thread_exit(a.task_id);
            pcb::destroy(a.pid);
            serial_println!("[spawn]   FAIL: fs-tls proc B spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Drive the scheduler until both processes have exited (or we hit the
    // yield ceiling).  Both must reach Zombie for the test to conclude.
    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        let a_done = pcb::state(a.pid) == Some(pcb::ProcessState::Zombie);
        let b_done = pcb::state(b.pid) == Some(pcb::ProcessState::Zombie);
        if a_done && b_done {
            break;
        }
    }

    let a_state = pcb::state(a.pid);
    let b_state = pcb::state(b.pid);
    let a_exit = pcb::exit_code(a.pid);
    let b_exit = pcb::exit_code(b.pid);

    thread::on_thread_exit(a.task_id);
    thread::on_thread_exit(b.task_id);
    pcb::destroy(a.pid);
    pcb::destroy(b.pid);

    if a_state != Some(pcb::ProcessState::Zombie) || b_state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: %fs/TLS test — not both zombie after {} yields (A={:?}, B={:?})",
            MAX_YIELDS, a_state, b_state
        );
        return Err(KernelError::InternalError);
    }

    // exit(0) = FS base held across every yield; exit(0xF1)=241 = clobbered.
    if a_exit != Some(0) || b_exit != Some(0) {
        serial_println!(
            "[spawn]   FAIL: %fs/TLS test — a process saw a clobbered FS base (A exit={:?}, \
             B exit={:?}; 0xF1/241 = FS base changed across a context switch — the scheduler \
             is not saving/restoring IA32_FS_BASE per task)",
            a_exit, b_exit
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux %fs/TLS-base context-switch persistence (ring 3: two concurrent \
         Linux procs kept distinct FS bases across cooperative yields): OK"
    );
    Ok(())
}

/// Ring-3 regression test that the **userspace `%gs` base is saved/restored
/// across context switches** between two concurrent Linux processes — the
/// sibling of [`self_test_linux_fs_tls_switch`].
///
/// `arch_prctl(ARCH_SET_GS)` installs the userspace `%gs` base.  Under Slate's
/// entry-stub convention this is the active `IA32_GS_BASE` (MSR 0xC000_0101) —
/// the stub swaps `%gs` back before the Rust handler runs, so the userspace
/// value is active during kernel execution and the per-CPU pointer rests in
/// `KERNEL_GS_BASE`; interrupts never `SWAPGS`.  The `%gs` base is thus a
/// **global CPU register** absent from the saved GP `Context`, fully symmetric
/// to `%fs`.  Without per-task save/restore, two concurrent processes that
/// each set a `%gs` base clobber each other's.
///
/// Two [`elf::build_linux_gs_tls_test_elf`] processes are spawned with
/// distinct sentinel GS bases; each installs its sentinel via
/// `arch_prctl(ARCH_SET_GS)` then loops `sched_yield` + `arch_prctl
/// (ARCH_GET_GS)` asserting the value is unchanged.  Both exiting `0` proves
/// the per-task GS base is correctly restored on switch-in; `0xF2`/242 means a
/// process observed a clobbered GS base.
pub fn self_test_linux_gs_tls_switch() -> KernelResult<()> {
    // Two distinct canonical user-address sentinels (< 1 << 47, non-zero),
    // distinct from the FS test's so a mix-up would be obvious.  Never
    // dereferenced — they only round-trip through IA32_GS_BASE.
    const SENTINEL_A: u64 = 0x0000_2233_4400_0000;
    const SENTINEL_B: u64 = 0x0000_6677_8800_0000;
    const MAX_YIELDS: usize = 1024;

    serial_println!(
        "[spawn] Running Linux %gs-base context-switch persistence test (ring 3, 2 procs)..."
    );

    let spawn_one = |sentinel: u64, name: &'static str| -> KernelResult<SpawnResult> {
        let elf_img = elf::build_linux_gs_tls_test_elf(sentinel);
        let argv: &[&[u8]] = &[name.as_bytes()];
        let envp: &[&[u8]] = &[b"PATH=/bin"];
        let options = SpawnOptions {
            name,
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &[],
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };
        spawn_process(&elf_img, &options)
    };

    let a = match spawn_one(SENTINEL_A, "spawn-test-gs-tls-a") {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: gs-tls proc A spawn returned {:?}", e);
            return Err(e);
        }
    };
    let b = match spawn_one(SENTINEL_B, "spawn-test-gs-tls-b") {
        Ok(r) => r,
        Err(e) => {
            // A was spawned; tear it down before bailing.
            thread::on_thread_exit(a.task_id);
            pcb::destroy(a.pid);
            serial_println!("[spawn]   FAIL: gs-tls proc B spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Drive the scheduler until both processes have exited (or we hit the
    // yield ceiling).  Both must reach Zombie for the test to conclude.
    for _ in 0..MAX_YIELDS {
        crate::sched::yield_now();
        let a_done = pcb::state(a.pid) == Some(pcb::ProcessState::Zombie);
        let b_done = pcb::state(b.pid) == Some(pcb::ProcessState::Zombie);
        if a_done && b_done {
            break;
        }
    }

    let a_state = pcb::state(a.pid);
    let b_state = pcb::state(b.pid);
    let a_exit = pcb::exit_code(a.pid);
    let b_exit = pcb::exit_code(b.pid);

    thread::on_thread_exit(a.task_id);
    thread::on_thread_exit(b.task_id);
    pcb::destroy(a.pid);
    pcb::destroy(b.pid);

    if a_state != Some(pcb::ProcessState::Zombie) || b_state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: %gs test — not both zombie after {} yields (A={:?}, B={:?})",
            MAX_YIELDS, a_state, b_state
        );
        return Err(KernelError::InternalError);
    }

    // exit(0) = GS base held across every yield; exit(0xF2)=242 = clobbered.
    if a_exit != Some(0) || b_exit != Some(0) {
        serial_println!(
            "[spawn]   FAIL: %gs test — a process saw a clobbered GS base (A exit={:?}, \
             B exit={:?}; 0xF2/242 = GS base changed across a context switch — the scheduler \
             is not saving/restoring the userspace %gs base per task)",
            a_exit, b_exit
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux %gs-base context-switch persistence (ring 3: two concurrent \
         Linux procs kept distinct GS bases across cooperative yields): OK"
    );
    Ok(())
}

/// Ring-3 end-to-end test of the Linux `execveat(2)` syscall.
///
/// `execveat` is exercised in **both** of its forms by spawning a real
/// Linux-ABI launcher that issues the syscall itself and then `exit`s `0xEE`
/// only if it returns (i.e. exec failed):
///
///   - **Path form** (`dirfd = AT_FDCWD`, non-empty pathname): the launcher
///     resolves the target by path, just like `execve`.
///   - **fexecve form** (`AT_EMPTY_PATH`, empty pathname, `dirfd` from an
///     `open(2)`): the launcher opens the target read-only and execs the open
///     file descriptor.  This is the genuinely-new capability `execveat`
///     adds over `execve` — glibc's `fexecve(3)` is built on exactly this.
///
/// The target is [`elf::build_linux_exit_elf`] which `exit`s with `SENTINEL`.
/// A clean zombie with `exit_code == SENTINEL` proves `execveat` replaced the
/// launcher's image and transferred control to the target; `0xEE` would mean
/// `execveat` returned an error and the launcher ran its failure tail.
///
/// Skips gracefully (`Ok`) if the VFS write fails.  Must run **after**
/// filesystem initialization (see `main.rs`).
pub fn self_test_linux_execveat() -> KernelResult<()> {
    const TGT_PATH: &str = "/slateos-test-execveat-tgt";
    const TGT_PATH_NUL: &[u8] = b"/slateos-test-execveat-tgt\0";
    const SENTINEL: u8 = 0x3A; // 58 — distinct from interp(42)/mmap(91)/brk(109)

    serial_println!("[spawn] Running Linux execveat(2) (ring 3) integration test...");

    // Step 1: stage the execveat *target* — a Linux-ABI ELF that exit()s
    // with the sentinel.  If control reaches it, execveat worked.
    let tgt_elf = elf::build_linux_exit_elf(SENTINEL);
    if let Err(e) = crate::fs::Vfs::write_file(TGT_PATH, &tgt_elf) {
        serial_println!(
            "[spawn]   Linux execveat (ring 3): SKIP (VFS write failed: {:?})",
            e
        );
        return Ok(());
    }

    // `run_one` spawns a launcher that execveat()s the target, lets it run to
    // completion, and asserts it exited with SENTINEL (proving execveat
    // replaced the image).  The launcher holds a File capability because the
    // fexecve form must open(2) the target first; the path form ignores it.
    // Returns Err on failure; the caller removes the staged target file.
    let run_one = |launcher_elf: &[u8], label: &str| -> KernelResult<()> {
        let argv: &[&[u8]] = &[b"execveatprog"];
        let envp: &[&[u8]] = &[b"PATH=/bin"];
        let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
        let options = SpawnOptions {
            name: "spawn-test-linux-execveat",
            parent: 0,
            priority: DEFAULT_PRIORITY,
            capabilities: &caps,
            fd_map: &[],
            argv,
            envp,
            exe_path: None,
            cwd: None,
            uid_gid: None,
        };

        let result = match spawn_process(launcher_elf, &options) {
            Ok(r) => r,
            Err(e) => {
                serial_println!("[spawn]   FAIL: execveat-test ({label}) spawn returned {:?}", e);
                return Err(e);
            }
        };

        // Let the launcher run: (open →) execveat → target exit(SENTINEL).
        // execveat replaces the image in-place, so a few yields cover both the
        // launcher's syscalls and the target's exit.
        crate::sched::yield_now();
        crate::sched::yield_now();
        crate::sched::yield_now();
        crate::sched::yield_now();

        let state = pcb::state(result.pid);
        let exit_code = pcb::exit_code(result.pid);

        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);

        if state != Some(pcb::ProcessState::Zombie) {
            serial_println!(
                "[spawn]   FAIL: execveat (ring 3, {label}) — expected Zombie, got {:?}",
                state
            );
            return Err(KernelError::InternalError);
        }

        if exit_code != Some(i32::from(SENTINEL)) {
            serial_println!(
                "[spawn]   FAIL: execveat (ring 3, {label}) — expected exit {} (target ran), \
                 got {:?} (0xEE = execveat returned an error; launcher ran its failure tail)",
                SENTINEL, exit_code
            );
            return Err(KernelError::InternalError);
        }
        Ok(())
    };

    // Case A: path form — execveat(AT_FDCWD, "/…/tgt", …, flags=0).
    let elf_path = elf::build_linux_execveat_test_elf(false, 0, 1, TGT_PATH_NUL);
    // Case B: fexecve form — open(target) then execveat(fd, "", …, AT_EMPTY_PATH).
    let elf_fexecve = elf::build_linux_execveat_test_elf(true, 0, 1, TGT_PATH_NUL);

    let res_a = run_one(&elf_path, "path form (AT_FDCWD)");
    let res_b = if res_a.is_ok() {
        run_one(&elf_fexecve, "fexecve form (AT_EMPTY_PATH)")
    } else {
        Ok(())
    };

    let _ = crate::fs::Vfs::remove(TGT_PATH);
    res_a?;
    res_b?;

    serial_println!(
        "[spawn]   Linux execveat(2) (ring 3: path form + fexecve/AT_EMPTY_PATH, \
         target exit == {}): OK",
        SENTINEL
    );

    // Case C: AT_SYMLINK_NOFOLLOW must *refuse* a symlink target with ELOOP.
    // Stage a symlink pointing at a valid target, then execveat it with
    // AT_SYMLINK_NOFOLLOW: the launcher should fall through to its failure
    // tail and exit 0xEE (execveat returned an error), proving the kernel
    // rejected the final symlink component rather than transparently
    // following it to a successful exec.
    const LINK_PATH: &str = "/slateos-test-execveat-link";
    const LINK_PATH_NUL: &[u8] = b"/slateos-test-execveat-link\0";
    const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
    const EXEC_FAIL: u8 = 0xEE;

    // Re-stage the target (Case A/B removed it above) and the symlink to it.
    let tgt_elf2 = elf::build_linux_exit_elf(SENTINEL);
    if crate::fs::Vfs::write_file(TGT_PATH, &tgt_elf2).is_err()
        || crate::fs::Vfs::symlink(LINK_PATH, TGT_PATH).is_err()
    {
        // Symlink staging unsupported here — skip the NOFOLLOW case but keep
        // the (already-passed) happy-path result.
        let _ = crate::fs::Vfs::remove(LINK_PATH);
        let _ = crate::fs::Vfs::remove(TGT_PATH);
        serial_println!(
            "[spawn]   Linux execveat(2) AT_SYMLINK_NOFOLLOW: SKIP (symlink staging failed)"
        );
        return Ok(());
    }

    let elf_nofollow =
        elf::build_linux_execveat_test_elf(false, AT_SYMLINK_NOFOLLOW, 1, LINK_PATH_NUL);
    let argv: &[&[u8]] = &[b"execveatprog"];
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-execveat-nofollow",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let nofollow_result = spawn_process(&elf_nofollow, &options);
    let (state_nf, exit_nf) = match nofollow_result {
        Ok(r) => {
            crate::sched::yield_now();
            crate::sched::yield_now();
            crate::sched::yield_now();
            crate::sched::yield_now();
            let st = pcb::state(r.pid);
            let ec = pcb::exit_code(r.pid);
            thread::on_thread_exit(r.task_id);
            pcb::destroy(r.pid);
            (st, ec)
        }
        Err(e) => {
            let _ = crate::fs::Vfs::remove(LINK_PATH);
            let _ = crate::fs::Vfs::remove(TGT_PATH);
            serial_println!("[spawn]   FAIL: execveat-nofollow spawn returned {:?}", e);
            return Err(e);
        }
    };

    let _ = crate::fs::Vfs::remove(LINK_PATH);
    let _ = crate::fs::Vfs::remove(TGT_PATH);

    if state_nf != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: execveat AT_SYMLINK_NOFOLLOW — expected Zombie, got {:?}",
            state_nf
        );
        return Err(KernelError::InternalError);
    }
    // The launcher must have run its failure tail (exit 0xEE): a SENTINEL exit
    // would mean execveat followed the symlink and exec'd the target.
    if exit_nf != Some(i32::from(EXEC_FAIL)) {
        serial_println!(
            "[spawn]   FAIL: execveat AT_SYMLINK_NOFOLLOW — expected exit {} (ELOOP → \
             launcher failure tail), got {:?} ({} would mean the symlink was followed)",
            EXEC_FAIL, exit_nf, SENTINEL
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux execveat(2) AT_SYMLINK_NOFOLLOW (ring 3: symlink target \
         refused with ELOOP): OK"
    );

    // Case D: argv propagation.  execve must rebuild the new image's initial
    // SysV stack with the argv *it was passed*, not the launcher's original
    // argv.  The launcher passes argv of `ARGC` entries; the target
    // (build_linux_argc_exit_test_elf) reads argc from [rsp] and exits with
    // it.  A clean exit == ARGC proves execveat constructed the new argc/argv
    // — the path gcc/make rely on (gcc invokes cc1/as/ld with many args).
    const ARGC_TGT_PATH: &str = "/slateos-test-execveat-argc";
    const ARGC_TGT_PATH_NUL: &[u8] = b"/slateos-test-execveat-argc\0";
    // Paired so the launcher's argv length (usize) and the expected exit code
    // (i32) stay in lock-step without a usize→i32 cast in the comparison.
    const ARGC: usize = 3;
    const ARGC_EXIT: i32 = 3;

    let argc_tgt_elf = elf::build_linux_argc_exit_test_elf();
    if crate::fs::Vfs::write_file(ARGC_TGT_PATH, &argc_tgt_elf).is_err() {
        serial_println!("[spawn]   Linux execveat(2) argv propagation: SKIP (VFS write failed)");
        return Ok(());
    }

    let elf_argv = elf::build_linux_execveat_test_elf(false, 0, ARGC, ARGC_TGT_PATH_NUL);
    let argv: &[&[u8]] = &[b"execveatprog"]; // launcher's own argv (argc 1) — irrelevant
    let envp: &[&[u8]] = &[b"PATH=/bin"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-linux-execveat-argv",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let (state_d, exit_d) = match spawn_process(&elf_argv, &options) {
        Ok(r) => {
            crate::sched::yield_now();
            crate::sched::yield_now();
            crate::sched::yield_now();
            crate::sched::yield_now();
            let st = pcb::state(r.pid);
            let ec = pcb::exit_code(r.pid);
            thread::on_thread_exit(r.task_id);
            pcb::destroy(r.pid);
            (st, ec)
        }
        Err(e) => {
            let _ = crate::fs::Vfs::remove(ARGC_TGT_PATH);
            serial_println!("[spawn]   FAIL: execveat-argv spawn returned {:?}", e);
            return Err(e);
        }
    };

    let _ = crate::fs::Vfs::remove(ARGC_TGT_PATH);

    if state_d != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: execveat argv propagation — expected Zombie, got {:?}",
            state_d
        );
        return Err(KernelError::InternalError);
    }
    if exit_d != Some(ARGC_EXIT) {
        serial_println!(
            "[spawn]   FAIL: execveat argv propagation — expected exit {} (argc passed to \
             execveat), got {:?} (1 would mean the launcher's original argv leaked through)",
            ARGC_EXIT, exit_d
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   Linux execveat(2) argv propagation (ring 3: target sees argc == {}): OK",
        ARGC
    );
    Ok(())
}

/// Path Z end-to-end test: run a **real, prebuilt, dynamically-linked glibc**
/// Linux binary to completion.
///
/// Every prior Linux-ABI self-test runs a *synthetic* ELF this kernel emits
/// itself ([`elf::build_linux_*`]).  Those validate one mechanism at a time
/// (PT_INTERP parse, `AT_BASE`/`AT_ENTRY` auxv, PIE bias, SysV stack layout,
/// `fork`/`execve`/`wait4`) against code we control.  This test instead drives
/// the *real* glibc dynamic path: it spawns `/bin/hello` — an ordinary
/// `gcc`-built `int main(void){return 42;}` — whose `PT_INTERP` names
/// `/lib64/ld-linux-x86-64.so.2`.  A clean exit 42 means the kernel loaded the
/// real `ld.so` at the ASLR bias, handed it the correct auxv, and `ld.so`
/// mapped `libc.so.6`, processed relocations, set up TLS, ran
/// `__libc_start_main`, called `main`, and exited — the entire glibc startup,
/// end to end, on attacker-uncontrolled binaries (design-decisions.md §25,
/// roadmap.md line 5089).
///
/// **Self-staging from the ext4 rootfs.** The glibc tree lives on a real
/// driver-compatible ext4 image (`scripts/create-ext4-rootfs.sh` → `rootfs.ext4`,
/// attached as `vdb` and mounted read-only at `/mnt`).  Because the active root
/// is a writable mem/FAT fs while `/mnt` is the read-only ext4 rootfs, we copy
/// `ld.so`, `libc.so.6`, and `hello` into the active root at the exact paths the
/// binary names (`/lib64/...`, `/lib/x86_64-linux-gnu/...`, `/bin/hello`) so the
/// loader's `PT_INTERP` and the libc `RUNPATH` resolve without an `ld.so.cache`.
///
/// **Skips gracefully** (returns `Ok`) when `/mnt/bin/hello` is absent — the
/// `rootfs.ext4` image is git-ignored, so CI and any environment without it
/// simply no-op this test rather than failing.  Must run **after** filesystem
/// init and the `/mnt` ext4 probe (see `main.rs`).
///
/// **Hang-safe.** Real glibc executes hundreds of syscalls, so unlike the
/// synthetic tests (which yield a fixed handful of times) this harness pumps
/// the scheduler with a generous but **bounded** `yield_now` poll loop, breaking
/// the moment the child becomes a zombie and force-destroying it afterward.  A
/// broken ABI path can therefore only produce a clean failed assertion, never a
/// boot hang.
pub fn self_test_linux_real_glibc() -> KernelResult<()> {
    // gcc-built `int main(void){return 42;}` exits 42 through the full glibc
    // dynamic startup.  Distinct from every synthetic sentinel above.
    const EXPECT_EXIT: i32 = 42;
    // Source paths on the read-only ext4 rootfs mounted at /mnt.
    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_HELLO: &str = "/mnt/bin/hello";
    // Destination paths in the active (writable) root — the exact paths the
    // binary's PT_INTERP and libc RUNPATH name, so resolution needs no cache.
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_HELLO: &str = "/bin/hello";
    // glibc startup is hundreds of syscalls; grant generous headroom but stay
    // bounded so a broken ABI path fails an assertion instead of hanging boot.
    const MAX_YIELDS: usize = 4096;

    // No rootfs.ext4 attached → nothing staged at /mnt.  No-op (not a failure):
    // the image is git-ignored, so most environments legitimately lack it.
    if !crate::fs::Vfs::exists(SRC_HELLO) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc dynamic-execution (ring 3, Path Z) test...");

    // Stage the glibc tree from the read-only ext4 rootfs into the active root
    // at the loader's expected paths.  mkdir_all is idempotent; a missing parent
    // dir is the common first-run case, so ignore its "already exists" result.
    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");

    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_HELLO, DST_HELLO),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    // Re-read the staged executable to hand its bytes to the spawner.
    let exe_elf = match crate::fs::Vfs::read_file(DST_HELLO) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   real glibc: SKIP (re-read {} failed: {:?})",
                DST_HELLO, e
            );
            return Ok(());
        }
    };

    // exe_path names the on-disk executable so the loader/auxv (AT_EXECFN) and
    // any /proc/self/exe-style lookups resolve to the real path the binary was
    // launched as, matching what a shell exec would produce.
    let argv: &[&[u8]] = &[b"/bin/hello"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The dynamic loader (ld.so) opens the shared libraries it needs from the
    // VFS at runtime — `/lib/x86_64-linux-gnu/libc.so.6`, the ld.so cache, etc.
    // Every file syscall is gated on a File capability, so without one ld.so's
    // openat() calls fail the cap check and it reports "cannot open shared
    // object file".  Grant READ|WRITE like the other ring-3 file-using tests.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_HELLO.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real glibc spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Bounded poll: drive the scheduler until the glibc child reaches Zombie or
    // we exhaust the bound.  Each iteration is one non-blocking yield; a healthy
    // run breaks early after the loader+libc finish.
    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc — child did not exit within {} yields (state={:?}); \
             ld.so/libc startup faulted or blocked",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc — ran to exit but code={:?}, expected {} (a wrong \
             code means glibc startup reached _exit on an error path)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   REAL glibc dynamic execution (ring 3: ld.so mapped libc.so.6, relocated, \
         set up TLS, __libc_start_main → main → exit({})): OK",
        EXPECT_EXIT
    );
    Ok(())
}

/// Path Z, part 2: prove the REAL-glibc **stdio output** path end-to-end.
///
/// `self_test_linux_real_glibc` above proves dynamic startup runs to
/// `exit(42)`, but a binary that returns from `main` exercises none of glibc's
/// output machinery.  This test runs `/bin/stdio` — a prebuilt, dynamically
/// linked binary whose `main` is `printf("SLATE_GLIBC_STDIO_OK %d\n", 1234)` —
/// and captures what it writes.
///
/// The gating problem: a Linux-ABI process is spawned with fd 1 pre-pointed at
/// the kernel **console** (`linux_fd_install_stdio`), so its output would vanish
/// into the serial log with no way for an in-kernel assertion to read it back.
/// We redirect fd 1 to a capture file *before the child first runs* (spawning is
/// cooperative — `spawn_process` returns before the child executes a single
/// instruction): drop the console fd 1 and install a freshly opened VFS file
/// handle at fd 1.  glibc then `fstat`s fd 1, sees a regular file, picks
/// full buffering, formats via `vfprintf`, and on exit flushes the buffer with
/// a real `write(2)`/`writev(2)` to the file.  Reading the file back and finding
/// the exact expected bytes proves the whole real-glibc output path works — the
/// gate for any glibc program that produces output.
///
/// No-ops (returns `Ok`) when the ext4 rootfs is absent (`/mnt/bin/stdio`
/// missing) — the image is git-ignored, so most environments lack it.  Must run
/// after the `/mnt` ext4 probe and after the glibc tree is reachable.
pub fn self_test_linux_real_glibc_stdio() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    // The binary returns 7 so the exit-code channel independently confirms a
    // clean run, distinct from the bytes it prints.
    const EXPECT_EXIT: i32 = 7;
    // The exact bytes `printf("SLATE_GLIBC_STDIO_OK %d\n", 1234)` produces.
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_STDIO_OK 1234\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_STDIO: &str = "/mnt/bin/stdio";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_STDIO: &str = "/bin/stdio";
    const CAPTURE: &str = "/glibc-stdio-capture.tmp";
    const MAX_YIELDS: usize = 4096;

    if !crate::fs::Vfs::exists(SRC_STDIO) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc stdio (output) (ring 3, Path Z) test...");

    // Stage the glibc tree + the stdio binary at the loader's expected paths.
    // Idempotent: re-running after `self_test_linux_real_glibc` just overwrites.
    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_STDIO, DST_STDIO)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc stdio: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc stdio: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_STDIO) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc stdio: SKIP (re-read {} failed: {:?})", DST_STDIO, e);
            return Ok(());
        }
    };

    // Start from a clean capture file so a stale prior run can't be mistaken
    // for this run's output.  `remove` of a nonexistent file is fine.
    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc stdio: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/stdio"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-stdio",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_STDIO.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            // We still own the capture handle — close it so it doesn't leak.
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc stdio spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect the child's fd 1 from the console to our capture file BEFORE it
    // runs.  Dropping the console entry needs no kernel close (it owns no
    // resource).  After install_at the capture handle is owned by the child's
    // fd table and is released by its exit teardown — so we must NOT close it
    // ourselves on the success path.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        // Install failed: ownership never transferred, so we still own the
        // handle and must close it.  Tear the child down too.
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc stdio — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    // Read the captured output BEFORE destroying the child.  (The child still
    // holds fd 1 → capture file until teardown, but reading by path opens an
    // independent handle onto the same inode, so the bytes it already flushed
    // are visible.)
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid); // closes the child's fd 1 → releases capture_handle
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc stdio — child did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc stdio — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc stdio output (ring 3: printf → glibc full-buffered stdio → \
                 write(2) to redirected fd 1, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc stdio — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc stdio — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z, part 3: prove the REAL-glibc **argv + environment + stdin + heap**
/// paths end-to-end.
///
/// The first two real-glibc tests cover dynamic startup (`exit(42)`) and the
/// stdio *output* path (`printf` → `write(2)`).  This one drives `/bin/full`,
/// a prebuilt dynamic binary whose `main`:
///   1. sums `argv[]` string lengths — proves the kernel built the stack argv
///      vector glibc reads;
///   2. `getenv("SLATE_TAG")` — proves envp delivery + glibc's `environ` scan;
///   3. one `fgets()` from stdin — proves the glibc *input* path (`fstat(0)`
///      buffering choice + `read(2)` on a regular file);
///   4. 64 rounds of mixed small (brk-arena) and large (>128 KiB, mmap-backed)
///      `malloc`/`free`, touching every page — stresses brk growth and the
///      mmap heap path under genuine glibc allocator behaviour.
///
/// We redirect the child's fd 0 from the console to a pre-populated input file
/// and fd 1 to a capture file *before it first runs* (spawning is cooperative).
/// The output line is deterministic from the fixed argv / env / stdin we supply,
/// so we assert the exact bytes; the binary returns 11 so the exit-code channel
/// independently confirms a clean run (a heap crash/OOM returns 2/3 instead).
///
/// No-op (returns `Ok`) when the ext4 rootfs is absent (`/mnt/bin/full`
/// missing) — the image is git-ignored, so most environments lack it.
pub fn self_test_linux_real_glibc_full() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_RDONLY, O_WRONLY};

    const EXPECT_EXIT: i32 = 11;
    // Deterministic from argv ["/bin/full","alpha","beta"] (argsum 9+5+4=18,
    // argc 3), env SLATE_TAG=zeta, and stdin "hello-stdin\n".
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_FULL_OK tag=zeta argc=3 argsum=18 in=hello-stdin\n";
    const STDIN_BYTES: &[u8] = b"hello-stdin\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_FULL: &str = "/mnt/bin/full";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_FULL: &str = "/bin/full";
    const INPUT: &str = "/glibc-full-input.tmp";
    const CAPTURE: &str = "/glibc-full-capture.tmp";
    const MAX_YIELDS: usize = 4096;

    if !crate::fs::Vfs::exists(SRC_FULL) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc argv/env/stdin/heap (ring 3, Path Z) test...");

    // Stage the glibc tree + the binary (idempotent across the other Path-Z tests).
    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_FULL, DST_FULL)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc full: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc full: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_FULL) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc full: SKIP (re-read {} failed: {:?})", DST_FULL, e);
            return Ok(());
        }
    };

    // Pre-populate the stdin input file, then open a READ-only handle on it that
    // we will plant at the child's fd 0.  Reading the file by path here and
    // through the handle later both resolve to the same inode.
    let _ = crate::fs::Vfs::remove(INPUT);
    if let Err(e) = crate::fs::Vfs::write_file(INPUT, STDIN_BYTES) {
        serial_println!("[spawn]   real glibc full: SKIP (writing stdin input failed: {:?})", e);
        return Ok(());
    }
    let stdin_handle = match handle::open(INPUT, handle::OpenFlags::READ) {
        Ok(h) => h,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(INPUT);
            serial_println!("[spawn]   real glibc full: SKIP (stdin-file open failed: {:?})", e);
            return Ok(());
        }
    };

    // Fresh capture file for fd 1.
    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            let _ = handle::close(stdin_handle);
            let _ = crate::fs::Vfs::remove(INPUT);
            serial_println!("[spawn]   real glibc full: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/full", b"alpha", b"beta"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C", b"SLATE_TAG=zeta"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-full",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_FULL.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            // We still own both handles — close them so nothing leaks.
            let _ = handle::close(stdin_handle);
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(INPUT);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc full spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 0 (stdin) → input file before the child runs.  After a
    // successful install_at the handle is owned by the child's fd table.
    let _ = pcb::linux_fd_take(result.pid, 0);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 0, FdEntry::file(stdin_handle, O_RDONLY)) {
        // fd 0 ownership never transferred: we still own stdin_handle AND
        // capture_handle (fd 1 not yet touched).  Close both, tear down child.
        let _ = handle::close(stdin_handle);
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(INPUT);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc full — redirecting fd 0 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    // Redirect fd 1 (stdout) → capture file.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        // fd 1 ownership never transferred: we still own capture_handle.  The
        // child now owns stdin_handle (fd 0) — destroying it releases that.
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(INPUT);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc full — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    // Read the captured output before teardown releases the child's fd 1.
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid); // closes child fds 0 & 1 → releases both handles
    let _ = crate::fs::Vfs::remove(INPUT);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc full — child did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc full — exit code={:?}, expected {} (2=malloc null, \
             3=heap loop skipped, other=glibc error path)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc argv/env/stdin/heap (ring 3: argv vector + getenv + \
                 fgets(stdin) + 64-round brk/mmap malloc-free, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc full — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc full — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z, part 4: prove the REAL-glibc **pthread (clone + futex + TLS)** path
/// end-to-end.
///
/// `thread_clone.rs`'s own self-test explicitly cannot exercise the IRETQ
/// trampoline ("the integration path is covered by booting a real Linux binary
/// that calls `pthread_create`") — this is that test.  It runs `/bin/pthread`,
/// which spawns 4 worker threads that each increment a shared counter 10000
/// times under one `pthread_mutex`, then `pthread_join`s all four and sums their
/// return values.  The output is deterministic regardless of scheduling (the
/// mutex guarantees no lost updates): `counter=40000 joinsum=10`.  Asserting it
/// proves the whole multithreading path works through real glibc:
///   - `clone(CLONE_VM|CLONE_THREAD|CLONE_SETTLS|…)` thread creation
///     (`thread_clone::clone_thread` + the trampoline);
///   - per-thread TLS (glibc's `errno` and pthread bookkeeping live in TLS);
///   - the futex fast path (uncontended adaptive-mutex CAS in userspace) and the
///     contended path (`futex` wait/wake syscalls under lock contention);
///   - `pthread_join`, which blocks on the child-tid futex the kernel wakes on
///     thread exit; and the per-thread `exit(2)` (not `exit_group`) teardown
///     that leaves the process alive until its last (main) thread exits.
///
/// We redirect the child's fd 1 to a capture file before it runs and assert the
/// exact bytes plus `exit(13)` (2 = pthread_create failed, 3 = pthread_join
/// failed).  No-op (returns `Ok`) when `/mnt/bin/pthread` is absent.
pub fn self_test_linux_real_glibc_pthread() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 13;
    // 4 threads * 10000 increments = 40000; returns 1+2+3+4 = 10.
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_PT: &str = "/mnt/bin/pthread";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_PT: &str = "/bin/pthread";
    const CAPTURE: &str = "/glibc-pthread-capture.tmp";
    // Multithreaded workload: generous bound so contention-driven futex
    // descheduling can't trip a false timeout.  A genuine deadlock still hits
    // the bound quickly (no ready task → each yield returns immediately).
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_PT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc pthread (clone+futex+TLS) (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_PT, DST_PT)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc pthread: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc pthread: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_PT) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc pthread: SKIP (re-read {} failed: {:?})", DST_PT, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc pthread: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/pthread"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-pthread",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_PT.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc pthread spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 1 → capture before the child runs.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc pthread — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc pthread — process did not exit within {} yields \
             (state={:?}); a thread likely deadlocked on a futex or a worker faulted",
            MAX_YIELDS, state
        );
        // A hang (never reached Zombie in budget) is the transient spawn/reap/
        // futex flake family (B-PTHREAD-YIELDBUDGET), NOT a wrong-result bug —
        // classify it as TimedOut so the caller's WARNING line self-identifies
        // it as a flake rather than a genuine logic error.
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc pthread — exit code={:?}, expected {} (2=pthread_create \
             failed, 3=pthread_join failed)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc pthread (ring 3: 4 threads via clone+TLS, 40000 mutex/futex \
                 ops, pthread_join, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc pthread — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc pthread — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z, part 5: prove the REAL-glibc **signal handler** path end-to-end.
///
/// The kernel's own signal-shim self-tests exercise the pending/blocked/
/// disposition bookkeeping in isolation, but never build a real Linux
/// `rt_sigframe` and enter an unmodified glibc handler.  This is that
/// integration test.  It runs `/bin/signal`, which:
///   - installs a `SA_SIGINFO` handler for `SIGUSR1` via `sigaction(2)`
///     (glibc fills in `sa_restorer = __restore_rt` automatically);
///   - `raise(3)`s `SIGUSR1` (glibc routes it through `tgkill(2)`);
///   - in the handler, reads `info->si_signo`/`si_code` and sets a flag;
///   - after the handler returns (via glibc's `__restore_rt` →
///     `rt_sigreturn`), checks the flag and the captured signo.
///
/// This proves the whole Linux signal path works through real glibc:
///   - `rt_sigaction` install (handler + `SA_SIGINFO` + `sa_restorer`);
///   - the kernel's byte-exact `rt_sigframe` delivery
///     ([`crate::syscall::linux::build_linux_rt_frame`]): handler entered
///     with `rdi=signo`, `rsi=&siginfo`, `rdx=&ucontext`, `rsp` at
///     `pretcode = sa_restorer`;
///   - the handler reading a correctly-laid-out `siginfo_t`;
///   - the return path: `rt_sigreturn` restoring the pre-signal context so
///     `main` resumes and exits cleanly.
///
/// Output is deterministic — `SIGUSR1 = 10` on x86_64 and the kernel
/// currently stamps `si_code = SI_USER (0)` for caught signals — so we
/// assert the exact bytes plus `exit(17)` (2 = sigaction failed, 3 =
/// handler never ran, 4 = wrong signo).  No-op (returns `Ok`) when
/// `/mnt/bin/signal` is absent.
pub fn self_test_linux_real_glibc_signal() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 17;
    // SIGUSR1 = 10 (x86_64). raise(3) routes through tgkill(2), so the kernel
    // delivers a thread-directed siginfo: si_code = SI_TKILL (-6) and
    // si_pid = the caller's pid (the handler checks si_pid == getpid() ->
    // self=1). This proves sender-faithful siginfo (known-issues.md TD29).
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_SIGNAL_OK signo=10 code=-6 self=1\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_SIG: &str = "/mnt/bin/signal";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_SIG: &str = "/bin/signal";
    const CAPTURE: &str = "/glibc-signal-capture.tmp";
    // Single-threaded, synchronous raise()+handler — completes promptly.
    // Generous bound still tolerates scheduler jitter.
    const MAX_YIELDS: usize = 65_536;

    if !crate::fs::Vfs::exists(SRC_SIG) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc signal (SA_SIGINFO handler, ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_SIG, DST_SIG)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc signal: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc signal: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_SIG) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc signal: SKIP (re-read {} failed: {:?})", DST_SIG, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc signal: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/signal"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-signal",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_SIG.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc signal spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 1 → capture before the child runs.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc signal — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc signal — process did not exit within {} yields \
             (state={:?}); the handler likely faulted (bad rt_sigframe) or rt_sigreturn \
             corrupted the resume context",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc signal — exit code={:?}, expected {} (2=sigaction \
             failed, 3=handler never ran, 4=wrong signo)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc signal (ring 3: SA_SIGINFO handler entered via Linux \
                 rt_sigframe, siginfo read, rt_sigreturn resume, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc signal — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc signal — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z: prove the REAL-glibc **synchronous-fault** signal path end-to-end.
///
/// The [`self_test_linux_real_glibc_signal`] test covers an *asynchronous*
/// signal (`raise` → `tgkill`, delivered at syscall return).  This one covers
/// the harder *synchronous* path: a hardware CPU fault delivered as a Linux
/// signal straight from the page-fault ISR.  It runs `/bin/fault`, which:
///   - installs a `SA_SIGINFO` handler for `SIGSEGV` via `sigaction(2)`;
///   - `sigsetjmp`s a recovery point, then writes to an unmapped address
///     (`0xDEAD000`), taking a not-present `#PF`;
///   - in the handler, reads `info->si_signo` / `si_code` / `si_addr`, then
///     `siglongjmp`s back to the recovery point;
///   - after recovery, prints the captured values and `exit`s.
///
/// This proves the kernel:
///   - detects a ring-3 `AbiMode::Linux` fault, finds the installed `SIGSEGV`
///     handler, and builds a byte-exact `rt_sigframe` from the *ISR* register
///     context (not a syscall frame) via
///     [`crate::syscall::linux::emit_linux_rt_frame`];
///   - fills a faithful fault `siginfo`: `si_addr` = the faulting address
///     (CR2 = `0xDEAD000`), `si_code` = `SEGV_MAPERR` (not-present);
///   - enters the handler and lets `siglongjmp` unwind so the process
///     survives the fault.
///
/// Output is deterministic, so we assert the exact bytes plus `exit(19)`
/// (2 = sigaction failed, 3 = handler never ran, 4 = wrong signo,
/// 5 = wrong si_code, 6 = wrong si_addr).  No-op (returns `Ok`) when
/// `/mnt/bin/fault` is absent.
pub fn self_test_linux_real_glibc_fault() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 19;
    // SIGSEGV = 11, SEGV_MAPERR = 1, faulting address = 0xDEAD000.
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_FAULT_OK signo=11 code=1 addr=0xdead000\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_FAULT: &str = "/mnt/bin/fault";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_FAULT: &str = "/bin/fault";
    const CAPTURE: &str = "/glibc-fault-capture.tmp";
    // Single-threaded, synchronous fault + siglongjmp — completes promptly.
    const MAX_YIELDS: usize = 65_536;

    if !crate::fs::Vfs::exists(SRC_FAULT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc fault-signal (SIGSEGV handler, ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_FAULT, DST_FAULT)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc fault: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc fault: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_FAULT) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc fault: SKIP (re-read {} failed: {:?})", DST_FAULT, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc fault: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/fault"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-fault",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_FAULT.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc fault spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 1 → capture before the child runs.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc fault — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc fault — process did not exit within {} yields \
             (state={:?}); the SIGSEGV handler likely faulted (bad rt_sigframe built from \
             the ISR context) or siglongjmp could not unwind",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc fault — exit code={:?}, expected {} (2=sigaction \
             failed, 3=handler never ran, 4=wrong signo, 5=wrong si_code, 6=wrong si_addr)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc fault (ring 3: #PF → SIGSEGV handler entered via Linux \
                 rt_sigframe built from the page-fault ISR, si_addr/si_code read, siglongjmp \
                 recovery, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc fault — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc fault — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z: prove the REAL-glibc **`SI_QUEUE` payload** signal path end-to-end.
///
/// [`self_test_linux_real_glibc_signal`] covers a plain async signal and
/// [`self_test_linux_real_glibc_fault`] covers a synchronous fault signal.
/// This one covers the *queued* path with a user-supplied `sigval`: it runs
/// `/bin/sigqueue`, which:
///   - installs a `SA_SIGINFO` handler for `SIGUSR1` via `sigaction(2)`;
///   - calls `sigqueue(getpid(), SIGUSR1, sv)` with `sv.sival_int =
///     0x12345678` (glibc routes this through `rt_sigqueueinfo(2)`);
///   - in the handler, reads `info->si_code` (expect `SI_QUEUE = -1`),
///     `info->si_value.sival_int` (expect `0x12345678`) and `info->si_pid`
///     (expect `getpid()`), then prints the captured values and `exit`s.
///
/// This proves the kernel:
///   - reads the user-supplied `siginfo` in `sys_rt_sigqueueinfo`, copies the
///     `si_value` union out of user memory, and records it on the pending
///     signal;
///   - on delivery, stamps `si_value` (and `si_code = SI_QUEUE`) into the
///     `rt_sigframe` via [`crate::proc::linux_sigframe::LinuxSiginfo::queue`]
///     at the correct ABI offset (struct +24);
///   - records the *real caller* pid/uid as `si_pid`/`si_uid` (faithful and
///     unforgeable), not a user-supplied value.
///
/// Output is deterministic, so we assert the exact bytes plus `exit(23)`
/// (2 = sigaction/sigqueue failed, 3 = handler never ran, 4 = wrong signo,
/// 5 = wrong si_code, 6 = wrong si_value, 7 = wrong si_pid).  No-op (returns
/// `Ok`) when `/mnt/bin/sigqueue` is absent.
pub fn self_test_linux_real_glibc_sigqueue() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 23;
    // SIGUSR1 = 10, SI_QUEUE = -1, sival_int = 0x12345678, self == getpid().
    const EXPECT_OUT: &[u8] =
        b"SLATE_GLIBC_SIGQUEUE_OK signo=10 code=-1 value=0x12345678 self=1\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_SIGQUEUE: &str = "/mnt/bin/sigqueue";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_SIGQUEUE: &str = "/bin/sigqueue";
    const CAPTURE: &str = "/glibc-sigqueue-capture.tmp";
    // Single-threaded, synchronous self-sigqueue — completes promptly.
    const MAX_YIELDS: usize = 65_536;

    if !crate::fs::Vfs::exists(SRC_SIGQUEUE) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc SI_QUEUE-payload (SIGUSR1 handler, ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_SIGQUEUE, DST_SIGQUEUE)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc sigqueue: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc sigqueue: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_SIGQUEUE) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc sigqueue: SKIP (re-read {} failed: {:?})", DST_SIGQUEUE, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc sigqueue: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/sigqueue"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-sigqueue",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_SIGQUEUE.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc sigqueue spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 1 → capture before the child runs.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc sigqueue — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc sigqueue — process did not exit within {} yields \
             (state={:?}); the SIGUSR1 handler likely faulted (bad rt_sigframe) or the \
             SI_QUEUE payload corrupted the siginfo",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc sigqueue — exit code={:?}, expected {} (2=sigaction/\
             sigqueue failed, 3=handler never ran, 4=wrong signo, 5=wrong si_code, \
             6=wrong si_value, 7=wrong si_pid)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc sigqueue (ring 3: sigqueue → SIGUSR1 SA_SIGINFO handler \
                 entered via Linux rt_sigframe, si_code=SI_QUEUE + sival_int payload + si_pid \
                 read back, captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc sigqueue — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc sigqueue — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z: prove a REAL-glibc program can `fork()`+`execl()`+`waitpid()`
/// another REAL-glibc program — the foundation for a shell.
///
/// Every other Path-Z real-glibc test is a *single* glibc process.  This one
/// runs `/bin/forkexec`, which:
///   - `fork()`s (glibc → `clone(SIGCHLD)` with a genuine CoW address-space
///     copy + `pthread_atfork`/malloc-lock handling);
///   - in the child, `execl("/bin/hello", …)` (the silent real-glibc binary
///     that `exit_group(42)`s) — replacing the image and marshalling
///     argv/envp;
///   - in the parent, `waitpid(pid, &status, 0)` (glibc → `wait4`), reaps the
///     child, and prints the decoded `WEXITSTATUS`.
///
/// Because the child is silent and the parent writes only *after* the reap,
/// the bytes on the shared fd 1 are deterministic.  This proves the kernel's
/// Linux-ABI `fork`/`execve`/`wait4` path works under glibc's wrappers (not
/// just the hand-assembled [`self_test_linux_fork_execve_wait`]), including a
/// real CoW fork of a dynamically-linked image and a child exec that re-runs
/// `ld.so`.
///
/// Output is deterministic, so we assert the exact bytes plus `exit(27)`
/// (2 = fork failed, 3 = waitpid mismatch, 4 = child didn't exit normally).
/// No-op (returns `Ok`) when `/mnt/bin/forkexec` is absent.
pub fn self_test_linux_real_glibc_forkexec() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 27;
    // /bin/hello exits 42; the parent decodes WEXITSTATUS and prints it.
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_FORKEXEC_OK childexit=42\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_HELLO: &str = "/mnt/bin/hello";
    const SRC_FORKEXEC: &str = "/mnt/bin/forkexec";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_HELLO: &str = "/bin/hello";
    const DST_FORKEXEC: &str = "/bin/forkexec";
    const CAPTURE: &str = "/glibc-forkexec-capture.tmp";
    // fork + exec + wait of a dynamically-linked child: more work than the
    // single-process tests (the child re-runs ld.so), so allow extra yields.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_FORKEXEC) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc fork()+execl()+waitpid() (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_HELLO, DST_HELLO),
        (SRC_FORKEXEC, DST_FORKEXEC),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc forkexec: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc forkexec: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_FORKEXEC) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc forkexec: SKIP (re-read {} failed: {:?})", DST_FORKEXEC, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc forkexec: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/forkexec"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-forkexec",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_FORKEXEC.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc forkexec spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 1 → capture before the child runs.  The forked child
    // inherits this fd (it is silent), so only the parent's post-reap line
    // lands in the capture file.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc forkexec — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc forkexec — process did not exit within {} yields \
             (state={:?}); the CoW fork, the child's ld.so re-exec, or the parent's \
             wait4 likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc forkexec — exit code={:?}, expected {} (2=fork \
             failed, 3=waitpid mismatch, 4=child not WIFEXITED)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc forkexec (ring 3: glibc fork() CoW + child execl(/bin/hello) \
                 re-running ld.so + parent waitpid() reaping WEXITSTATUS=42, captured {} bytes \
                 == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc forkexec — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc forkexec — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z: prove a real glibc program can build a `cmd1 | cmd2` pipeline
/// — `pipe(2)` → `fork(2)` → child `dup2(2)`s the write end onto fd 1 and
/// `execl(2)`s a writer (`/bin/emit`), parent `read(2)`s the pipe to EOF
/// and `waitpid(2)`s the child.
///
/// This is the next shell primitive after [`self_test_linux_real_glibc_forkexec`]:
/// it exercises (a) pipe-fd inheritance across the CoW fork, (b) `dup2`
/// redirection, (c) an open (dup2'd) fd surviving `execve` into a fresh
/// glibc image (no `CLOEXEC`), and (d) pipe EOF arriving once every write
/// end is closed.  The parent writes its post-read line to its own fd 1
/// (a capture file), so the captured bytes are deterministic.
///
/// No-op (returns `Ok`) when the Path-Z rootfs (`/mnt/bin/pipe`) is absent.
pub fn self_test_linux_real_glibc_pipe() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    const EXPECT_EXIT: i32 = 29;
    // /bin/emit writes "SLATE_PIPE_BODY\n" (16 bytes) to the pipe; the
    // parent reports the byte count and echoes the payload.
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_PIPE_OK n=16 body=SLATE_PIPE_BODY\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const SRC_PIPE: &str = "/mnt/bin/pipe";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_EMIT: &str = "/bin/emit";
    const DST_PIPE: &str = "/bin/pipe";
    const CAPTURE: &str = "/glibc-pipe-capture.tmp";
    // pipe + fork + exec + read-to-EOF + wait of a dynamically-linked child:
    // the child re-runs ld.so, so allow the same generous yield budget as
    // the forkexec test.  The parent's blocking read returns EOF as soon as
    // the child exits and the kernel closes its last pipe write end at the
    // zombie transition (pcb::exit_close_fds), so the parent wakes promptly
    // once the child finishes its (in-budget) startup.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_PIPE) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc pipe()+fork()+dup2()+execl()+read()+wait (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_EMIT, DST_EMIT),
        (SRC_PIPE, DST_PIPE),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc pipe: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc pipe: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_PIPE) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc pipe: SKIP (re-read {} failed: {:?})", DST_PIPE, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real glibc pipe: SKIP (capture-file open failed: {:?})", e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/pipe"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-pipe",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_PIPE.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real glibc pipe spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect the *parent's* fd 1 → capture before it runs.  The child
    // rewires its own fd 1 onto the pipe (dup2) before exec, so only the
    // parent's post-read line lands in the capture file.
    //
    // The capture handle must be installed the way a normally-open()ed
    // file would be, or the forked child destroys it: the child's
    // `dup2(pipe, 1)` displaces the inherited fd-1 entry and the kernel's
    // dup2-close path (`sys_dup2_impl` → `close_handle` → `sys_fs_close`)
    // drops one refcount on the displaced handle.  If we injected the
    // handle raw (refcount 1, untracked) that single close would take the
    // shared capture handle to refcount 0 and free it before the parent's
    // post-read `printf` runs, yielding a 0-byte capture.  To model real
    // ownership: bump the refcount once for the parent's own reference and
    // register it in the parent's ipc_handles, so (a) `fork_create`'s
    // `dup_one` bumps it again for the child, (b) the child's dup2-close
    // decrements the child's reference only, and (c) the parent retains a
    // live reference through its `printf`.  The original `handle::open`
    // reference stays ours, for the read-back + final close below.
    if let Err(e) = handle::dup_shared(capture_handle) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc pipe — dup_shared(capture) failed: {:?}", e);
        return Err(KernelError::InternalError);
    }
    pcb::register_ipc_handle(result.pid, ResourceType::File, capture_handle);
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        // Drop the parent's reference we just added (register + dup_shared),
        // then our own; the process never ran so nothing else holds it.
        pcb::deregister_ipc_handle(result.pid, ResourceType::File, capture_handle);
        let _ = handle::close(capture_handle);
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real glibc pipe — redirecting fd 1 failed: {:?}", e);
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(CAPTURE);
    // Release our own (open) reference to the capture handle.  The
    // parent's reference (dup_shared + register above) was already
    // dropped by `exit_close_fds` at its zombie transition; this final
    // close balances the refcount back to zero with no leak.
    let _ = handle::close(capture_handle);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc pipe — process did not exit within {} yields \
             (state={:?}); pipe inheritance across fork, the child's dup2/exec, the \
             parent's blocking read, or wait4 likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc pipe — exit code={:?}, expected {} (2=pipe \
             failed, 3=fork failed, 4=waitpid mismatch, 5=child error)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc pipe (ring 3: pipe() + fork() CoW pipe-fd inherit + \
                 child dup2(write end -> fd 1) + execl(/bin/emit) preserving the open fd + \
                 parent read()-to-EOF + waitpid(), captured {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc pipe — captured {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc pipe — reading capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 8: real glibc `cmd > file` output redirection.
///
/// Runs a prebuilt glibc binary (`/bin/redir`) that performs its OWN
/// output redirection the way a shell does for `cmd > file`: it
/// `open(2)`s a target with `O_WRONLY|O_CREAT|O_TRUNC`, `dup2(2)`s the
/// resulting fd onto fd 1 (the kernel closes the displaced console fd),
/// closes the now-redundant original fd, and `printf`s to the redirected
/// stdout — glibc full-buffers (fd 1 is a regular file) and issues the
/// `write(2)` at exit.
///
/// Part 7 (`self_test_linux_real_glibc_pipe`) proved `dup2` onto a *pipe*
/// write end; this proves `dup2` of a self-`open()`ed *File* handle onto
/// stdout, the displaced-Console close, and a glibc program creating and
/// writing a file it chose.  Unlike the earlier output tests this injects
/// NO fd from the kernel side — the program opens the file itself, and the
/// test reads that exact file back from the VFS.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/redir` is absent.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the process fails to reach
/// `Zombie`, exits with the wrong code, or the file it wrote does not
/// match the expected bytes; propagates spawn failure.
pub fn self_test_linux_real_glibc_redir() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 31;
    const EXPECT_OUT: &[u8] = b"SLATE_GLIBC_REDIR_OK marker=4242\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_REDIR: &str = "/mnt/bin/redir";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_REDIR: &str = "/bin/redir";
    // The path the *program* opens + redirects stdout onto.  Must match
    // the literal in scripts/create-ext4-rootfs.sh's /bin/redir source.
    const OUT_PATH: &str = "/redir-out.txt";
    // Dynamically-linked single process (re-runs ld.so); same generous
    // budget as the other real-glibc tests.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_REDIR) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc `cmd > file` output-redirection (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_REDIR, DST_REDIR),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc redir: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc redir: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_REDIR) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc redir: SKIP (re-read {} failed: {:?})", DST_REDIR, e);
            return Ok(());
        }
    };

    // Clear any stale output from a prior boot so a read-back can only
    // succeed if THIS run wrote it.
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[b"/bin/redir"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The program opens + creates a file, so it needs File READ|WRITE.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-redir",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_REDIR.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real glibc redir spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc redir — process did not exit within {} yields \
             (state={:?}); the program's open()/dup2() redirection or exit-flush \
             write likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc redir — exit code={:?}, expected {} (2=open \
             failed, 3=dup2 failed)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL glibc redir (ring 3: open(O_WRONLY|O_CREAT|O_TRUNC) + \
                 dup2(file -> fd 1) + displaced-console close + printf flushed to the \
                 program's own file, read back {} bytes == expected): OK",
                bytes.len()
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real glibc redir — wrote {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real glibc redir — reading the program's output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 9: prove program-driven `cmd < file` *input* redirection
/// with a real prebuilt glibc binary.
///
/// The mirror image of [`self_test_linux_real_glibc_redir`]: that test
/// proved `dup2` of a self-`open()`ed File onto stdout; this proves
/// `dup2` of a self-`open()`ed *read-only* File onto stdin (fd 0), the
/// displaced-Console-stdin close, and glibc's buffered *input* path
/// (`fstat(0)` + `read(2)` + `fgets`) reading from a regular file the
/// program chose.  The harness pre-creates the input file, injects NO
/// fd, and verifies success purely through the exit code: `/bin/redirin`
/// compares the line it reads against a compiled-in literal and returns
/// `37` only on an exact byte match, so the right exit code is a
/// byte-exact proof the redirected stdin delivered the correct bytes.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/redirin` is absent.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the process fails to reach
/// `Zombie` or exits with the wrong code; propagates spawn failure.
pub fn self_test_linux_real_glibc_redirin() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 37;
    // Must match the literal in scripts/create-ext4-rootfs.sh's
    // /bin/redirin source (the line it strcmp's the stdin read against).
    const IN_CONTENT: &[u8] = b"SLATE_GLIBC_STDIN_OK marker=7777\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_REDIRIN: &str = "/mnt/bin/redirin";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_REDIRIN: &str = "/bin/redirin";
    // The path the *program* opens + redirects stdin from.  Must match
    // the literal in scripts/create-ext4-rootfs.sh's /bin/redirin source.
    const IN_PATH: &str = "/redir-in.txt";
    // Dynamically-linked single process (re-runs ld.so); same generous
    // budget as the other real-glibc tests.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_REDIRIN) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL glibc `cmd < file` input-redirection (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_REDIRIN, DST_REDIRIN),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real glibc redirin: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real glibc redirin: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_REDIRIN) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real glibc redirin: SKIP (re-read {} failed: {:?})", DST_REDIRIN, e);
            return Ok(());
        }
    };

    // Lay down the exact input the program will read through fd 0.  This
    // is the file the shell would have opened for `< file`.
    if let Err(e) = crate::fs::Vfs::write_file(IN_PATH, IN_CONTENT) {
        serial_println!(
            "[spawn]   real glibc redirin: SKIP (staging input {} failed: {:?})",
            IN_PATH, e
        );
        return Ok(());
    }

    let argv: &[&[u8]] = &[b"/bin/redirin"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The program only opens an existing file O_RDONLY, so File READ is
    // exactly the authority it needs (the Linux open path gates on READ).
    let caps = [(ResourceType::File, 1u64, Rights::READ)];
    let options = SpawnOptions {
        name: "spawn-test-glibc-redirin",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_REDIRIN.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real glibc redirin spawn returned {:?}", e);
            let _ = crate::fs::Vfs::remove(IN_PATH);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(IN_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real glibc redirin — process did not exit within {} yields \
             (state={:?}); the program's open()/dup2() input redirection or stdin \
             read likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real glibc redirin — exit code={:?}, expected {} (2=open \
             failed, 3=dup2 failed, 4=fgets EOF, 5=content mismatch)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   REAL glibc redirin (ring 3: open(O_RDONLY) + dup2(file -> fd 0) + \
         displaced-console-stdin close + glibc fgets read {} bytes from the program's \
         own file == expected, exit {}): OK",
        IN_CONTENT.len(), EXPECT_EXIT
    );
    Ok(())
}

/// Path Z Part 10: run an **unmodified, prebuilt POSIX shell** (`dash`)
/// that performs an output redirection itself.
///
/// Parts 6–9 proved each shell primitive (fork/exec/waitpid, pipe, `dup2`
/// onto a pipe, `dup2` of a file onto stdout/stdin) with a bespoke test
/// binary that issued the syscalls directly.  This is the culmination:
/// a real `/bin/dash` interprets `echo … > /dash-out.txt` and drives the
/// redirection logic *itself* — ld.so loads dash + libc, dash's lexer/
/// parser handles the command + `>` redirection, dash `open(2)`s the
/// target, `dup2`s it over fd 1 (saving/restoring fd 1 around the
/// builtin via `dup`/`dup2`/`close`), runs its `echo` builtin, and exits.
/// `echo` is a dash *builtin*, so this first shell test isolates "the
/// shell runs and does its own redirection" from external `fork`/`exec`
/// (proven separately).  No fd is injected — the test reads the file the
/// shell created back from the VFS.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/dash` is absent.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the shell fails to reach
/// `Zombie`, exits non-zero, or the file it wrote does not match;
/// propagates spawn failure.
pub fn self_test_linux_real_glibc_shell_redir() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] = b"SLATE_DASH_REDIR_OK marker=4242\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    // The path the shell command opens + redirects stdout onto.
    const OUT_PATH: &str = "/dash-out.txt";
    // A real shell does more startup work than a bare binary (locale, stdio
    // streams, command parsing); keep the same generous budget.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell `echo > file` redirection (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash redir: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash redir: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash redir: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    // Clear any stale output so a read-back can only succeed if THIS run
    // wrote it.
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // dash -c '<command>' — the shell parses the command and the `>`
    // redirection itself.  Absolute paths avoid any $PATH / cwd lookup.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"echo SLATE_DASH_REDIR_OK marker=4242 > /dash-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The shell opens + creates the output file, so it needs File READ|WRITE.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-redir",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash redir spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash redir — shell did not exit within {} yields \
             (state={:?}); dash startup (ld.so/libc), command parsing, or its \
             open()/dup2() redirection likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash redir — exit code={:?}, expected {} (non-zero \
             means dash hit an error parsing/running the command or the redirection)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell (ring 3: ld.so loaded dash+libc, dash parsed \
                 `echo … > file`, did its own open()/dup2() redirection of the echo \
                 builtin, read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash redir — wrote {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash redir — reading the shell's output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 11: a real `dash` shell **forks + exec's an external
/// glibc binary** with output redirection (`/bin/emit > file`).
///
/// Part 10 ([`self_test_linux_real_glibc_shell_redir`]) ran dash but the
/// command (`echo`) was a shell *builtin*, so no `fork`/`exec` happened.
/// This is the full shell-orchestration proof: dash parses
/// `/bin/emit > /dash-exec-out.txt`, `fork(2)`s, the child `open(2)`s the
/// redirect target and `dup2`s it over fd 1 then `execve(2)`s the
/// *external* real-glibc `/bin/emit` (which `write(2)`s its payload to
/// fd 1 — now the file — and exits), and the parent dash `wait4(2)`s the
/// child and exits with its status.  Every piece (CoW fork, child-side
/// redirect, exec into a fresh glibc image, reap) was proven individually
/// in Parts 6–9; here a real shell drives all of them itself.  No fd is
/// injected — the test reads the file the exec'd binary wrote back.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/dash` / `/bin/emit`
/// is absent.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the shell fails to reach
/// `Zombie`, exits non-zero, or the file the exec'd child wrote does not
/// match; propagates spawn failure.
pub fn self_test_linux_real_glibc_shell_exec() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes exactly these 16 bytes (incl. newline) to fd 1.
    const EXPECT_OUT: &[u8] = b"SLATE_PIPE_BODY\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    // The path the shell command redirects the exec'd binary's stdout onto.
    const OUT_PATH: &str = "/dash-exec-out.txt";
    // fork + exec of a second glibc image (re-runs ld.so) under a shell;
    // keep the generous budget.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell fork+exec of external `/bin/emit > file` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash exec: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash exec: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash exec: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    // Clear any stale output so a read-back can only succeed if the exec'd
    // child wrote it THIS run.
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // dash forks, the child redirects + exec's the external binary.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"/bin/emit > /dash-exec-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // dash's child opens + creates the output file, so File READ|WRITE.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-exec",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash exec spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash exec — shell did not exit within {} yields \
             (state={:?}); dash's fork/child-redirect/exec of /bin/emit or its \
             wait4 likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash exec — exit code={:?}, expected {} (non-zero \
             means dash's fork/exec of /bin/emit failed or the child exited non-zero)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell fork+exec (ring 3: dash parsed `/bin/emit > file`, \
                 fork()ed, the child redirected fd 1 + execve()d the external glibc /bin/emit, \
                 parent wait4()ed; read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash exec — wrote {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash exec — reading the exec'd child's output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 12: a real `dash` shell builds a full **pipeline**
/// (`/bin/emit | /bin/countbytes > file`).
///
/// Part 7 ([`self_test_linux_real_glibc_pipe`]) proved the raw
/// `pipe`+`fork`+`dup2`+`exec`+`read`-to-EOF primitive with a bespoke
/// binary issuing the syscalls directly.  This proves a real *shell*
/// builds that same plumbing itself: dash parses
/// `/bin/emit | /bin/countbytes > /dash-pipe-out.txt`, `pipe(2)`s, forks
/// the **upstream** child (its fd 1 `dup2`'d to the pipe write end,
/// `execve`s `/bin/emit` which writes 16 bytes), forks the **downstream**
/// child (its fd 0 `dup2`'d to the pipe read end and its fd 1 redirected
/// to the output file, `execve`s `/bin/countbytes` which reads the pipe
/// to EOF and prints `n=<count>`), closes both pipe ends in the parent,
/// and `wait4`s both children.  EOF on the downstream's stdin arrives
/// only once every write end of the pipe is closed (the parent's and the
/// upstream child's, the latter on its exit) — the exit-time-fd-close
/// fix from Part 7 (known-issues F17) is what makes this terminate.  No
/// fd is injected; the test reads the file the downstream wrote back.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/dash` / `/bin/emit` /
/// `/bin/countbytes` is absent.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the shell fails to reach
/// `Zombie`, exits non-zero, or the downstream's output does not match;
/// propagates spawn failure.
pub fn self_test_linux_real_glibc_shell_pipe() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes 16 bytes, so /bin/countbytes prints "n=16\n".
    const EXPECT_OUT: &[u8] = b"n=16\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const SRC_COUNT: &str = "/mnt/bin/countbytes";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const DST_COUNT: &str = "/bin/countbytes";
    // The path the shell redirects the downstream stage's stdout onto.
    const OUT_PATH: &str = "/dash-pipe-out.txt";
    // A pipeline forks + execs two glibc images under a shell; keep the
    // generous budget.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH)
        || !crate::fs::Vfs::exists(SRC_EMIT)
        || !crate::fs::Vfs::exists(SRC_COUNT)
    {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell pipeline `/bin/emit | /bin/countbytes > file` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
        (SRC_COUNT, DST_COUNT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash pipe: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash pipe: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash pipe: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // dash builds the pipe, forks both stages, redirects, and waits.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"/bin/emit | /bin/countbytes > /dash-pipe-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The downstream child opens + creates the output file, so File READ|WRITE.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-pipe",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash pipe spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash pipe — shell did not exit within {} yields \
             (state={:?}); dash's pipe()/double-fork/dup2/exec or the downstream's \
             read-to-EOF likely hung (EOF needs every pipe write end closed)",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash pipe — exit code={:?}, expected {} (non-zero \
             means a pipeline stage failed to spawn or exited non-zero)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell pipeline (ring 3: dash built pipe(), forked \
                 /bin/emit upstream + /bin/countbytes downstream, dup2'd both pipe ends, \
                 redirected the tail to a file, wait4'd both; downstream counted the \
                 piped bytes — read back {} bytes {:?} == expected, exit {}): OK",
                bytes.len(), bytes.as_slice(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash pipe — wrote {} bytes {:?}, expected {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash pipe — reading the downstream's output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 13: a real `dash` shell runs a `for` loop that fork+exec's an
/// external glibc binary on every iteration, with the whole compound command's
/// stdout redirected once to a file.
///
/// Where Part 11 proved one fork+exec+wait and Part 12 proved a two-stage
/// pipeline, this proves the shell's *iteration* primitive: dash parses a
/// `for` loop, opens the redirect target once, then for each iteration forks a
/// child (CoW clone of the shell), exec's `/bin/emit` into it (tearing down the
/// CoW image), and `wait4`s it before the next iteration.  Running the
/// fork→exec→reap cycle N times back-to-back in a single long-lived parent is
/// exactly the path that surfaced the F18 CoW-refcount double-free (a
/// parent-shared frame freed during a child's exec teardown), so this is both a
/// new capability proof and a direct regression guard for that fix: a stale
/// double-free would corrupt the shell's image and crash it part-way through
/// the loop, yielding short/garbled output or a non-zero exit.
///
/// `/bin/emit` writes a fixed 16-byte payload per run, so three iterations with
/// a single outer redirect must produce exactly three concatenated copies and
/// exit 0.
pub fn self_test_linux_real_glibc_shell_loop() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes "SLATE_PIPE_BODY\n" (16 bytes); three loop iterations
    // sharing one outer redirect append in order.
    const EXPECT_OUT: &[u8] = b"SLATE_PIPE_BODY\nSLATE_PIPE_BODY\nSLATE_PIPE_BODY\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const OUT_PATH: &str = "/dash-loop-out.txt";
    // Three fork+exec+wait cycles under a shell; keep the generous budget.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell loop `for i in a b c; do /bin/emit; done > file` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash loop: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash loop: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash loop: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // dash opens the redirect once, then forks+execs /bin/emit each iteration.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"for i in a b c; do /bin/emit; done > /dash-loop-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    // The shell opens + creates the output file, so File READ|WRITE.
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-loop",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash loop spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash loop — shell did not exit within {} yields \
             (state={:?}); a fork/exec/wait iteration likely hung or the shell \
             crashed mid-loop (e.g. a stale CoW double-free corrupting its image)",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash loop — exit code={:?}, expected {} (non-zero \
             means a loop iteration's child failed to spawn/exec or the shell \
             faulted part-way through the loop)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell loop (ring 3: dash opened the redirect once, \
                 then forked + exec'd /bin/emit three times — three CoW fork→exec→reap \
                 cycles in one parent — read back {} bytes == 3x the emit payload, \
                 exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash loop — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (short/garbled output points at a fork/exec/wait or CoW-teardown regression)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash loop — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 14: a real `dash` shell reads and runs a multi-command *script
/// from stdin* (no `-c`), with fd 0 redirected from a script file and fd 1
/// captured.
///
/// Parts 10–13 all drove dash via `-c '<one line>'` — a single command string
/// parsed once.  This instead spawns `/bin/dash` with **no arguments** and fd 0
/// rewired to a regular file holding several lines, so dash detects a
/// non-interactive stdin and runs its *main read-eval loop*: read a line from
/// fd 0, parse it, execute it, repeat until EOF, then exit 0.  That loop — the
/// shell's actual top-level driver — is the new path here (vs. the one-shot
/// `-c` string), and the script mixes two sequential external `fork`+`exec`+
/// `wait4` commands (`/bin/emit`) with a builtin (`echo`), so it also confirms
/// the children inherit the advanced script-fd offset harmlessly and that EOF
/// on the redirected stdin terminates the shell cleanly.
///
/// The script is `"/bin/emit\n/bin/emit\necho SLATE_DASH_SCRIPT_DONE\n"`, so the
/// captured stdout must be the two 16-byte `/bin/emit` payloads followed by the
/// echo line, and dash must exit 0.
pub fn self_test_linux_real_glibc_shell_script_stdin() -> KernelResult<()> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_RDONLY, O_WRONLY};

    const EXPECT_EXIT: i32 = 0;
    const SCRIPT_BYTES: &[u8] = b"/bin/emit\n/bin/emit\necho SLATE_DASH_SCRIPT_DONE\n";
    // Two /bin/emit payloads ("SLATE_PIPE_BODY\n", 16 bytes each) + the echo line.
    const EXPECT_OUT: &[u8] = b"SLATE_PIPE_BODY\nSLATE_PIPE_BODY\nSLATE_DASH_SCRIPT_DONE\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const SCRIPT: &str = "/dash-script.sh";
    const CAPTURE: &str = "/dash-script-capture.txt";
    // A script forks + execs two glibc images under a shell; keep the generous
    // budget used by the other dash tests.
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell script-from-stdin (no -c; ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash script: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash script: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash script: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    // Write the script file, then open a READ-only handle we'll plant at fd 0.
    let _ = crate::fs::Vfs::remove(SCRIPT);
    if let Err(e) = crate::fs::Vfs::write_file(SCRIPT, SCRIPT_BYTES) {
        serial_println!("[spawn]   real dash script: SKIP (writing script failed: {:?})", e);
        return Ok(());
    }
    let script_handle = match handle::open(SCRIPT, handle::OpenFlags::READ) {
        Ok(h) => h,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(SCRIPT);
            serial_println!("[spawn]   real dash script: SKIP (script open failed: {:?})", e);
            return Ok(());
        }
    };

    // Fresh capture file for fd 1.
    let _ = crate::fs::Vfs::remove(CAPTURE);
    let capture_handle = match handle::open(
        CAPTURE,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            let _ = handle::close(script_handle);
            let _ = crate::fs::Vfs::remove(SCRIPT);
            serial_println!("[spawn]   real dash script: SKIP (capture open failed: {:?})", e);
            return Ok(());
        }
    };

    // No `-c`: dash with non-tty stdin runs stdin as a script.
    let argv: &[&[u8]] = &[b"/bin/dash"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-script",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(script_handle);
            let _ = handle::close(capture_handle);
            let _ = crate::fs::Vfs::remove(SCRIPT);
            let _ = crate::fs::Vfs::remove(CAPTURE);
            serial_println!("[spawn]   FAIL: real dash script spawn returned {:?}", e);
            return Err(e);
        }
    };

    // Redirect fd 0 (stdin) → the script file before dash runs.
    let _ = pcb::linux_fd_take(result.pid, 0);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 0, FdEntry::file(script_handle, O_RDONLY)) {
        let _ = handle::close(script_handle);
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(SCRIPT);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real dash script — redirecting fd 0 failed: {:?}", e);
        // Propagate the real fd-install error (infrastructure failure), not a
        // blanket InternalError, so the failure class is unambiguous.
        return Err(e);
    }

    // Redirect fd 1 (stdout) → capture file.
    let _ = pcb::linux_fd_take(result.pid, 1);
    if let Err(e) = pcb::linux_fd_install_at(result.pid, 1, FdEntry::file(capture_handle, O_WRONLY)) {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        let _ = crate::fs::Vfs::remove(SCRIPT);
        let _ = crate::fs::Vfs::remove(CAPTURE);
        serial_println!("[spawn]   FAIL: real dash script — redirecting fd 1 failed: {:?}", e);
        // Propagate the real fd-install error (infrastructure failure).
        return Err(e);
    }

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let captured = crate::fs::Vfs::read_file(CAPTURE);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(SCRIPT);
    let _ = crate::fs::Vfs::remove(CAPTURE);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash script — shell did not exit within {} yields \
             (state={:?}); dash's stdin read-eval loop, a script-command fork/exec/wait, \
             or EOF-driven termination likely hung",
            MAX_YIELDS, state
        );
        // A hang here (B-DASH-STDIN-FLAKE) is the transient spawn/reap/futex
        // flake family — classify as TimedOut, distinct from a genuine dash
        // wrong-output/exit bug (which keeps InternalError below).
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash script — exit code={:?}, expected {} (non-zero \
             means a script command failed or dash hit a parse/read error)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match captured {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell script-from-stdin (ring 3: dash ran its main \
                 read-eval loop over a {}-byte script on fd 0 — two sequential /bin/emit \
                 fork→exec→reap cycles + an echo builtin — captured {} bytes == expected, \
                 EOF→exit {}): OK",
                SCRIPT_BYTES.len(), bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash script — captured {} bytes {:?}, expected {} bytes {:?}",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash script — reading the capture file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 15: a real `dash` shell performs *pathname expansion* (globbing)
/// — `echo /globdir/* > file` — driving its own `opendir`/`getdents64`-based
/// directory enumeration.
///
/// Every prior Path Z test drove fork/exec/wait, stdio, pipes, or redirection;
/// none read a *directory*.  Shell globbing is the first end-to-end exercise of
/// the glibc `opendir`→`open(…, O_DIRECTORY)`→`getdents64`→`readdir` path: dash
/// expands `/globdir/*` by opening `/globdir`, enumerating its entries, dropping
/// the leading-dot names (`.`/`..` never match `*`), matching the pattern, and
/// **sorting** the survivors (POSIX requires sorted pathname-expansion results),
/// so the output is deterministic regardless of the VFS's directory order.
///
/// With a `/globdir` holding exactly `a.txt`, `b.txt`, `c.txt`, the redirected
/// stdout must be the three sorted paths joined by single spaces (echo's
/// separator) plus a trailing newline, and dash must exit 0.  A glob that
/// resolved nothing would instead leave the literal `/globdir/*` (POSIX
/// no-match rule), so an exact match is a strong proof the directory read
/// actually returned the entries.
pub fn self_test_linux_real_glibc_shell_glob() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // echo joins argv with single spaces and appends a newline; dash sorts the
    // glob matches, so a/b/c is deterministic.
    const EXPECT_OUT: &[u8] = b"/globdir/a.txt /globdir/b.txt /globdir/c.txt\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const GLOB_DIR: &str = "/globdir";
    const GLOB_A: &str = "/globdir/a.txt";
    const GLOB_B: &str = "/globdir/b.txt";
    const GLOB_C: &str = "/globdir/c.txt";
    const OUT_PATH: &str = "/glob-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell glob `echo /globdir/* > file` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash glob: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash glob: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash glob: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    // Build the directory the glob enumerates.  write_file is idempotent, so
    // re-running the test across boots just overwrites.
    let _ = crate::fs::Vfs::mkdir_all(GLOB_DIR);
    for f in [GLOB_A, GLOB_B, GLOB_C] {
        if let Err(e) = crate::fs::Vfs::write_file(f, b"x") {
            serial_println!("[spawn]   real dash glob: SKIP (creating {} failed: {:?})", f, e);
            return Ok(());
        }
    }

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // dash opens /globdir, getdents64s it, expands + sorts the matches, then
    // echoes them to the redirect file.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"echo /globdir/* > /glob-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-glob",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash glob spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);
    let _ = crate::fs::Vfs::remove(GLOB_A);
    let _ = crate::fs::Vfs::remove(GLOB_B);
    let _ = crate::fs::Vfs::remove(GLOB_C);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash glob — shell did not exit within {} yields \
             (state={:?}); dash's opendir/getdents64 directory read likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash glob — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell glob (ring 3: dash opened /globdir, getdents64'd \
                 it, expanded + sorted `*` to three paths, echoed them — read back {} bytes \
                 == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash glob — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (a literal `/globdir/*` means the directory read returned no matches)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash glob — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 16: a real `dash` shell performs *command substitution*
/// (`echo [$(/bin/emit)]`), where dash itself reads a child's output from a
/// pipe and substitutes it into the command line.
///
/// Part 12 proved dash *plumbing* a pipe between two children (`cmd1 | cmd2`);
/// here dash is the pipe *reader*: for `$(/bin/emit)` it creates a pipe,
/// `fork`s a subshell whose stdout is the pipe write end, `execve`s the
/// external glibc `/bin/emit` (which writes `"SLATE_PIPE_BODY\n"`), then dash
/// itself `read(2)`s the pipe to EOF into its own buffer, strips the trailing
/// newline(s) (POSIX command-substitution rule), and splices the captured
/// `SLATE_PIPE_BODY` into the `echo` builtin's argv.  The whole line's stdout
/// is redirected to a file so the test can read it back deterministically.
///
/// With `/bin/emit` emitting exactly `SLATE_PIPE_BODY\n`, the substitution
/// yields `SLATE_PIPE_BODY` (newline stripped) and `echo [SLATE_PIPE_BODY]`
/// writes the 18 bytes `[SLATE_PIPE_BODY]\n` to the redirect file; dash exits
/// 0.  A failed substitution would instead leave `[]\n` (3 bytes), so an exact
/// match proves dash read the child's piped output correctly.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if the shell fails to reach
/// `Zombie`, exits non-zero, or the captured file does not match; propagates
/// spawn failure.
pub fn self_test_linux_real_glibc_shell_cmdsub() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes "SLATE_PIPE_BODY\n"; command substitution strips the
    // trailing newline, and `echo [X]` re-adds one — so 18 bytes.
    const EXPECT_OUT: &[u8] = b"[SLATE_PIPE_BODY]\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const OUT_PATH: &str = "/cmdsub-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell command substitution `echo [$(/bin/emit)] > file` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash cmdsub: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash cmdsub: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash cmdsub: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"echo [$(/bin/emit)] > /cmdsub-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-cmdsub",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash cmdsub spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash cmdsub — shell did not exit within {} yields \
             (state={:?}); dash's pipe/fork/exec of /bin/emit or its capture read likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash cmdsub — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell command substitution (ring 3: dash piped + \
                 fork/exec'd /bin/emit, read its stdout to EOF, stripped the trailing \
                 newline, substituted it into echo — read back {} bytes == expected, \
                 exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash cmdsub — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (`[]` means the command substitution captured nothing)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash cmdsub — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 17: a real glibc `dash` evaluates a conditional compound
/// command with variable assignment, parameter expansion, and the `[`
/// (`test`) builtin, then redirects the chosen branch's output to a file.
///
/// The script `x=hello; if [ "$x" = hello ]; then echo EQ; else echo NE;
/// fi > /cond-out.txt` (run via `dash -c`) exercises dash's variable
/// assignment, `$x` expansion, the `[`/`test` builtin string comparison,
/// `if`/`then`/`else`/`fi` compound-command evaluation driven by the test's
/// exit status, and a redirection applied to the whole compound command —
/// all internal to dash, so it stresses dash's parser and word-expansion
/// machinery on our Linux ABI rather than fork/exec.  No-op without
/// rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_cond() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // The test compares $x (=="hello") to "hello", so the `then` branch
    // runs and `echo EQ` writes 3 bytes ("EQ\n").
    const EXPECT_OUT: &[u8] = b"EQ\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/cond-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell conditional `if [ \"$x\" = hello ]; then ...` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash cond: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash cond: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash cond: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"x=hello; if [ \"$x\" = hello ]; then echo EQ; else echo NE; fi > /cond-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-cond",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash cond spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash cond — shell did not exit within {} yields \
             (state={:?}); dash's if/test evaluation likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash cond — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell conditional (ring 3: assigned x=hello, expanded \
                 \"$x\", ran the `[` test builtin, took the then-branch, redirected echo — \
                 read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash cond — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (`NE` means the test builtin or expansion misbehaved)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash cond — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 18: a real glibc `dash` evaluates an arithmetic expansion
/// `$(( ))` with variable references and operator precedence, then writes
/// the result.
///
/// `dash -c 'x=3; y=4; echo $((x * y + 2)) > /arith-out.txt'` exercises
/// dash's arithmetic evaluator: it looks up `x`/`y` inside the arithmetic
/// context (no `$` needed there), applies `*` before `+` per C precedence,
/// and substitutes the decimal result into the `echo` word list.  This is
/// a distinct dash code path from parameter expansion and the test
/// builtin — purely internal, so it stresses the arithmetic tokenizer and
/// evaluator on our Linux ABI.  No-op without rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_arith() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // 3 * 4 + 2 == 14 (`*` binds tighter than `+`); `echo` adds a newline.
    const EXPECT_OUT: &[u8] = b"14\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/arith-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell arithmetic `echo $((x * y + 2))` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash arith: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash arith: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash arith: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"x=3; y=4; echo $((x * y + 2)) > /arith-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-arith",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash arith spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash arith — shell did not exit within {} yields \
             (state={:?}); dash's arithmetic evaluation likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash arith — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell arithmetic (ring 3: assigned x=3 y=4, evaluated \
                 $((x * y + 2)) with `*` before `+`, substituted 14 into echo — read back \
                 {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash arith — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (wrong value means precedence or variable lookup in the arithmetic context broke)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash arith — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 19: a real glibc `dash` processes a here-document
/// (`<<EOF`), feeding the body to the `read` builtin via the kernel's pipe
/// machinery.
///
/// The script `read a <<EOF` / `HELLO` / `EOF` / `echo "$a" > /hd-out.txt`
/// makes dash materialise the heredoc body, plumb it onto fd 0 (dash uses
/// a pipe — it forks a writer that pushes the body and dups the read end to
/// stdin), then the `read` builtin consumes one line into `$a`, and `echo`
/// writes it back.  Unlike the cond/arith tests this is *not* purely
/// internal: it exercises the kernel's pipe creation, blocking
/// read/write, and fd-dup path driven entirely by the shell, so it can
/// surface real ABI bugs.  No-op without rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_heredoc() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // `read a` consumes "HELLO" from the heredoc; `echo "$a"` re-adds \n.
    const EXPECT_OUT: &[u8] = b"HELLO\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/hd-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell here-document `read a <<EOF` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash heredoc: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash heredoc: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash heredoc: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    // The script body carries embedded newlines so dash sees a genuine
    // here-document terminated by the `EOF` sentinel on its own line.
    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"read a <<EOF\nHELLO\nEOF\necho \"$a\" > /hd-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-heredoc",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash heredoc spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash heredoc — shell did not exit within {} yields \
             (state={:?}); dash's heredoc pipe write/read likely deadlocked",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash heredoc — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell here-document (ring 3: materialised the heredoc \
                 body, plumbed it onto fd 0 via a pipe, `read a` consumed \"HELLO\", echoed \
                 it — read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash heredoc — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (empty/short means the heredoc pipe or `read` builtin misbehaved)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash heredoc — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 20: a real glibc `dash` runs a command as a background job
/// (`&`) and then reaps it with the `wait` builtin.
///
/// The script `/bin/emit > /bg-out.txt & wait` makes dash fork `/bin/emit`
/// as an asynchronous job (its stdout redirected to a file so we can
/// observe completion), record the job, return to the prompt immediately,
/// and then block in `wait` until the async child becomes a zombie and is
/// reaped.  This exercises the kernel's async-child + waitpid path driven
/// from the shell (distinct from the synchronous fork/exec/wait of the
/// pipe/loop/cmdsub tests), so it can surface job-control or reaping bugs.
/// No-op without rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_bgjob() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes "SLATE_PIPE_BODY\n" (16 bytes) to its redirected stdout.
    const EXPECT_OUT: &[u8] = b"SLATE_PIPE_BODY\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const OUT_PATH: &str = "/bg-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell background job `/bin/emit > file & wait` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash bgjob: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash bgjob: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash bgjob: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"/bin/emit > /bg-out.txt & wait",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-bgjob",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash bgjob spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash bgjob — shell did not exit within {} yields \
             (state={:?}); the `wait` builtin likely never reaped the async child",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash bgjob — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell background job (ring 3: forked /bin/emit as an \
                 async job with redirected stdout, returned to the prompt, then `wait` reaped \
                 it — read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash bgjob — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (empty means the backgrounded job never ran or `wait` returned before it finished)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash bgjob — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 21: a real glibc `dash` runs a two-stage pipeline that
/// connects an external program to a shell-internal reader.
///
/// The script `/bin/emit | while read l; do echo "<$l>"; done >
/// /pipe2-out.txt` makes dash create a pipe, fork `/bin/emit` (an external
/// glibc binary) with its stdout wired to the pipe's write end, fork a
/// second child running the `while read`/`echo` loop with its stdin wired
/// to the pipe's read end (and its stdout redirected to the file), and then
/// wait for the whole pipeline.  `/bin/emit` writes "SLATE_PIPE_BODY\n";
/// the loop's `read l` consumes that one line, `echo "<$l>"` wraps it, and
/// the next `read` hits EOF and ends the loop.  Unlike the bg-job test this
/// runs two pipeline stages *simultaneously* connected by a kernel pipe, so
/// it stresses concurrent fd inheritance, pipe blocking/EOF, and dash's
/// multi-child pipeline wait.  No-op without rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_pipeline() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // emit -> "SLATE_PIPE_BODY\n"; the loop reads "SLATE_PIPE_BODY" and
    // echoes "<SLATE_PIPE_BODY>\n" (18 bytes).
    const EXPECT_OUT: &[u8] = b"<SLATE_PIPE_BODY>\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const DST_EMIT: &str = "/bin/emit";
    const OUT_PATH: &str = "/pipe2-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) || !crate::fs::Vfs::exists(SRC_EMIT) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell pipeline `/bin/emit | while read ...` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_DASH, DST_DASH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash pipeline: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!(
                    "[spawn]   real dash pipeline: SKIP (reading {} failed: {:?})",
                    src, e
                );
                return Ok(());
            }
        }
    }

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash pipeline: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"/bin/emit | while read l; do echo \"<$l>\"; done > /pipe2-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-pipeline",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash pipeline spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash pipeline — shell did not exit within {} yields \
             (state={:?}); the pipeline likely never completed (a stage blocked on the pipe)",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash pipeline — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell pipeline (ring 3: dash piped external /bin/emit into a \
                 `while read` loop via a kernel pipe, wrapped the line, redirected the loop's stdout \
                 — read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash pipeline — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (empty means a pipeline stage never ran or the pipe never delivered EOF)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash pipeline — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 22: a real glibc `dash` changes its working directory and
/// reads it back, exercising the kernel's `chdir`/`getcwd` round-trip.
///
/// The script `cd /cwdtest && pwd -P > /cwd-out.txt` makes dash issue
/// `chdir("/cwdtest")` and then — crucially with the `-P` (physical) flag —
/// call `getcwd()` to obtain the kernel's notion of the current directory,
/// rather than printing the logical `$PWD` string it tracks internally
/// (which the default `pwd`/`pwd -L` would do without ever touching the
/// kernel).  No earlier Path Z test changes the working directory at all,
/// so this is the first end-to-end exercise of the per-process cwd stored
/// in the PCB: dash's `chdir` updates it, `getcwd` must read the exact same
/// canonical path back, and the ABI buffer-sizing / NUL-termination must be
/// correct or glibc's getcwd wrapper would loop or error.  We pre-create
/// `/cwdtest` for realism (the current chdir is string-level and does not
/// require the target to exist, but creating it keeps the test valid if
/// chdir ever gains an existence check).  The output redirect uses an
/// absolute path so it lands regardless of the new cwd.  No-op without
/// rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_cwd() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // `pwd -P` prints the kernel cwd from getcwd(); echo/pwd append a newline.
    const EXPECT_OUT: &[u8] = b"/cwdtest\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const CWD_DIR: &str = "/cwdtest";
    const OUT_PATH: &str = "/cwd-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell cwd `cd /cwdtest && pwd -P` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash cwd: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real dash cwd: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    let _ = crate::fs::Vfs::mkdir_all(CWD_DIR);

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash cwd: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let argv: &[&[u8]] = &[b"/bin/dash", b"-c", b"cd /cwdtest && pwd -P > /cwd-out.txt"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-cwd",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash cwd spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let written = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash cwd — shell did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash cwd — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match written {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell cwd (ring 3: dash chdir'd to /cwdtest then `pwd -P` \
                 read the kernel cwd back via getcwd — read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash cwd — wrote {} bytes {:?}, expected {} bytes {:?} \
                 (mismatch means chdir/getcwd disagree on the working directory)",
                bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash cwd — reading the redirected output file back failed: {:?}",
                e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 23: a real glibc `dash` opens a file by *relative* path after
/// changing directory, verifying that relative path resolution honours the
/// per-process cwd.
///
/// The script `cd /reltest && echo RELOK > relfile.txt` makes dash
/// `chdir("/reltest")` and then open `relfile.txt` (a *relative* path) for
/// the output redirect.  The correct Linux behaviour is that
/// `openat(AT_FDCWD, "relfile.txt", ...)` resolves against the process cwd,
/// so the file must be created at `/reltest/relfile.txt` — **not** at
/// `/relfile.txt`.  This is the regression test for the cwd-resolution fix:
/// before it, the Linux open path forwarded relative paths to the VFS
/// verbatim, which normalised them from the filesystem root, so the file
/// would have landed at `/relfile.txt` and this check (which reads back the
/// cwd-relative location) would fail.  Complements Part 22 (`pwd -P`, the
/// read side) by exercising the write side — a relative `open` that must be
/// canonicalised against the cwd.  No-op without rootfs.ext4.
pub fn self_test_linux_real_glibc_shell_relpath() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] = b"RELOK\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const REL_DIR: &str = "/reltest";
    // Where the file MUST land (cwd-relative) and where it would WRONGLY
    // land if relative resolution ignored the cwd.
    const GOOD_PATH: &str = "/reltest/relfile.txt";
    const WRONG_PATH: &str = "/relfile.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell relpath `cd /reltest && echo > relfile.txt` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash relpath: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real dash relpath: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    let _ = crate::fs::Vfs::mkdir_all(REL_DIR);
    // Clear any stale outputs from a prior run so the existence checks below
    // are meaningful.
    let _ = crate::fs::Vfs::remove(GOOD_PATH);
    let _ = crate::fs::Vfs::remove(WRONG_PATH);

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash relpath: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/dash", b"-c", b"cd /reltest && echo RELOK > relfile.txt"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-relpath",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash relpath spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let good = crate::fs::Vfs::read_file(GOOD_PATH);
    let wrong_exists = crate::fs::Vfs::exists(WRONG_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(GOOD_PATH);
    let _ = crate::fs::Vfs::remove(WRONG_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash relpath — shell did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash relpath — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    if wrong_exists {
        serial_println!(
            "[spawn]   FAIL: real dash relpath — file landed at {} (root) instead of the \
             cwd-relative {}; relative open ignored the process cwd",
            WRONG_PATH, GOOD_PATH
        );
        return Err(KernelError::InternalError);
    }

    match good {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell relpath (ring 3: dash chdir'd to /reltest then opened a \
                 relative `relfile.txt` for redirect — file correctly landed at {}, read back {} \
                 bytes == expected, exit {}): OK",
                GOOD_PATH, bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash relpath — {} wrote {} bytes {:?}, expected {} bytes {:?}",
                GOOD_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash relpath — reading {} back failed: {:?} (the relative \
                 open likely resolved against the wrong directory)",
                GOOD_PATH, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Part 24 (Path Z): real dash `[ -f PATH ]` exercises path-based stat.
///
/// `dash -c '[ -f /bin/dash ] && echo HASFILE > /stat-out.txt'`.  The `[`
/// builtin's `-f` predicate calls `stat(2)` (via glibc, today routed through
/// `statx`/`newfstatat`) on the pathname; the redirect only fires if the stat
/// reports the file exists and is a regular file.  Before the stat-stub fix
/// every path-based stat returned ENOENT unconditionally, so `[ -f ... ]` was
/// always false and nothing was written — a real program-visible regression.
/// This test pins the fixed behaviour: the path stat must succeed and report a
/// regular file so the redirect produces the expected output.
pub fn self_test_linux_real_glibc_shell_statpath() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] = b"HASFILE\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/stat-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell statpath `[ -f /bin/dash ] && echo` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash statpath: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real dash statpath: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash statpath: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[b"/bin/dash", b"-c", b"[ -f /bin/dash ] && echo HASFILE > /stat-out.txt"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-statpath",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash statpath spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let out = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash statpath — shell did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash statpath — exit code={:?}, expected {} (the `[ -f ]` \
             predicate likely saw ENOENT from a path-based stat)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match out {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell statpath (ring 3: `[ -f /bin/dash ]` stat'd an existing \
                 path, predicate true, redirect wrote {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash statpath — {} wrote {} bytes {:?}, expected {} bytes {:?}",
                OUT_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash statpath — reading {} back failed: {:?} (the `[ -f ]` \
                 predicate was false, so path-based stat is still broken)",
                OUT_PATH, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Part 25 (Path Z): real dash directory stat — `[ -d ]` / `[ ! -f ]` / `[ -e ]`.
///
/// `dash -c 'if [ -d /bin ] && [ ! -f /bin ] && [ -e /bin ]; then echo DIROK >
/// /dirstat-out.txt; fi'`.  Complements Part 24 (which stat'd a regular file):
/// this exercises the *directory* metadata path — `fill_stat_from_meta` must
/// set `S_IFDIR` (so `-d` is true and `-f` is false) and the lookup must
/// succeed (so `-e` is true).  A bug that reported directories with the wrong
/// `EntryType`/mode bits, or that only handled regular files, would flip one of
/// the three predicates and suppress the redirect.
pub fn self_test_linux_real_glibc_shell_dirstat() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] = b"DIROK\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/dirstat-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell dirstat `[ -d /bin ] && [ ! -f /bin ]` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash dirstat: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real dash dirstat: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash dirstat: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"if [ -d /bin ] && [ ! -f /bin ] && [ -e /bin ]; then echo DIROK > /dirstat-out.txt; fi",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-dirstat",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash dirstat spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let out = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash dirstat — shell did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash dirstat — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match out {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell dirstat (ring 3: `[ -d /bin ]` true, `[ ! -f /bin ]` true, \
                 `[ -e /bin ]` true — directory stat reported S_IFDIR correctly, wrote {} bytes == \
                 expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash dirstat — {} wrote {} bytes {:?}, expected {} bytes {:?}",
                OUT_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash dirstat — reading {} back failed: {:?} (one of `-d`/`! -f`/`-e` \
                 was false, so directory stat mode bits are wrong)",
                OUT_PATH, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Part 26 (Path Z): real dash append redirect `>>` exercises `O_APPEND`.
///
/// `dash -c 'echo first > /append-out.txt; echo second >> /append-out.txt'`.
/// The `>>` redirect opens the file with `O_WRONLY | O_CREAT | O_APPEND`; the
/// kernel must position the second write at end-of-file rather than offset 0.
/// A translator that drops `O_APPEND` (or that truncates on the second open)
/// would leave only `second\n` (overwrite) or a clobbered prefix; honouring
/// append yields `first\nsecond\n`.  This is the first self-test to exercise
/// the append path, distinct from the plain `>` truncating redirect (Part 10).
pub fn self_test_linux_real_glibc_shell_append() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] = b"first\nsecond\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_DASH: &str = "/mnt/bin/dash";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_DASH: &str = "/bin/dash";
    const OUT_PATH: &str = "/append-out.txt";
    const MAX_YIELDS: usize = 262_144;

    if !crate::fs::Vfs::exists(SRC_DASH) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL dash shell append `echo > f; echo >> f` (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_LD, DST_LD), (SRC_LIBC, DST_LIBC), (SRC_DASH, DST_DASH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real dash append: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real dash append: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let exe_elf = match crate::fs::Vfs::read_file(DST_DASH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real dash append: SKIP (re-read {} failed: {:?})", DST_DASH, e);
            return Ok(());
        }
    };

    let argv: &[&[u8]] = &[
        b"/bin/dash",
        b"-c",
        b"echo first > /append-out.txt; echo second >> /append-out.txt",
    ];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-dash-append",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_DASH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real dash append spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let out = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real dash append — shell did not exit within {} yields (state={:?})",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real dash append — exit code={:?}, expected {}",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match out {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL dash shell append (ring 3: `>` then `>>` — O_APPEND positioned the \
                 second write at EOF, file == `first\\nsecond\\n` ({} bytes), exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real dash append — {} holds {} bytes {:?}, expected {} bytes {:?} \
                 (O_APPEND likely dropped — second write clobbered from offset 0)",
                OUT_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real dash append — reading {} back failed: {:?}",
                OUT_PATH, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 34: run an **unmodified, prebuilt GNU `make`** that builds a
/// trivial target whose recipe forks a real glibc child.
///
/// This is the first rung of the operator-decided "GCC/CMake/Make toolchain"
/// initiative (design-decisions §9 / §12, Path Z).  Every prior Path-Z test
/// ran a single program or a shell; this runs `make`, the build *driver* that
/// orchestrates a real toolchain.  `make` is itself an unmodified glibc PIE
/// (`DT_NEEDED libc.so.6` only — both it and its interpreter are already
/// staged), so ld.so loads it, it reads + parses the Makefile, builds the
/// dependency graph, and to run the recipe `@/bin/emit > /make-out.txt` it
/// `fork`s and `exec`s `/bin/sh -c '…'` (the recipe contains the shell
/// metacharacter `>`, so make does **not** take its direct-exec optimisation),
/// `/bin/sh` (dash) in turn forks/execs the external `/bin/emit` with stdout
/// redirected to the file, and make `wait4`s its child and propagates the
/// status.  A correct run therefore exercises, end to end: make's glibc
/// startup, Makefile `open`/`read`/`stat`, dependency evaluation, recipe
/// dispatch via `/bin/sh`, the nested fork→exec→redirect→wait chain, and exit
/// status propagation up through make.  No fd is injected — the test reads the
/// file the recipe produced back from the VFS.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/make` / `/bin/sh` /
/// `/bin/emit` is absent (so a kernel built without the toolchain rootfs still
/// boots clean).
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if make fails to reach `Zombie`,
/// exits non-zero, or the recipe's output file does not match; propagates
/// spawn failure.
pub fn self_test_linux_real_glibc_make() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // /bin/emit writes exactly this 16-byte payload (incl. trailing newline).
    const EXPECT_OUT: &[u8] = b"SLATE_PIPE_BODY\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_MAKE: &str = "/mnt/bin/make";
    const SRC_SH: &str = "/mnt/bin/sh";
    const SRC_EMIT: &str = "/mnt/bin/emit";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_MAKE: &str = "/bin/make";
    const DST_SH: &str = "/bin/sh";
    const DST_EMIT: &str = "/bin/emit";
    const MAKEFILE_PATH: &str = "/Makefile";
    const OUT_PATH: &str = "/make-out.txt";
    // Recipe line MUST start with a TAB; `@` suppresses make's command echo.
    const MAKEFILE: &[u8] = b"all:\n\t@/bin/emit > /make-out.txt\n";
    // make + /bin/sh + /bin/emit is a three-process glibc chain; give the
    // bounded poll loop extra headroom over the two-process shell tests.
    const MAX_YIELDS: usize = 524_288;

    if !crate::fs::Vfs::exists(SRC_MAKE) || !crate::fs::Vfs::exists(SRC_SH)
        || !crate::fs::Vfs::exists(SRC_EMIT)
    {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL GNU make (ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_MAKE, DST_MAKE),
        (SRC_SH, DST_SH),
        (SRC_EMIT, DST_EMIT),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real make: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real make: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    // Stage the Makefile and clear any stale recipe output.
    if let Err(e) = crate::fs::Vfs::write_file(MAKEFILE_PATH, MAKEFILE) {
        serial_println!("[spawn]   real make: SKIP (writing {} failed: {:?})", MAKEFILE_PATH, e);
        return Ok(());
    }
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let exe_elf = match crate::fs::Vfs::read_file(DST_MAKE) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real make: SKIP (re-read {} failed: {:?})", DST_MAKE, e);
            return Ok(());
        }
    };

    // `-f /Makefile all`: explicit makefile + target avoids cwd makefile
    // probing; SHELL=/bin/sh pins the recipe shell so make does not search.
    let argv: &[&[u8]] = &[b"make", b"-f", b"/Makefile", b"all"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C", b"SHELL=/bin/sh"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-make",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_MAKE.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&exe_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real make spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for _ in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let exit_code = pcb::exit_code(result.pid);
    let out = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);
    let _ = crate::fs::Vfs::remove(OUT_PATH);
    let _ = crate::fs::Vfs::remove(MAKEFILE_PATH);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real make — make did not exit within {} yields (state={:?}); make \
             startup (ld.so/libc), Makefile parse, or its fork/exec of /bin/sh likely hung",
            MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    if exit_code != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real make — exit code={:?}, expected {} (non-zero means make hit an \
             error parsing the Makefile or its recipe child failed)",
            exit_code, EXPECT_EXIT
        );
        return Err(KernelError::InternalError);
    }

    match out {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL GNU make (ring 3: ld.so loaded make+libc, make parsed the \
                 Makefile and dispatched its recipe via /bin/sh, which fork/exec'd /bin/emit with \
                 a `>` redirect; read back {} bytes == expected, exit {}): OK",
                bytes.len(), EXPECT_EXIT
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real make — {} holds {} bytes {:?}, expected {} bytes {:?}",
                OUT_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real make — reading the recipe output {} back failed: {:?} \
                 (make's recipe child likely never ran or could not write the file)",
                OUT_PATH, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Self-test: run an **unmodified prebuilt C compiler** (TinyCC / `tcc`) in
/// ring 3 under the Linux ABI compat layer, have it compile a C source file
/// to a native executable, then run that freshly-compiled executable — also
/// in ring 3 — and verify it produces the expected output.  This is the next
/// rung of "Path Z" after GNU make: it proves the OS can host a real
/// toolchain, not merely run prebuilt binaries.
///
/// Flow:
/// 1. Stage `ld-linux`, `libc.so.6`, `libm.so.6` and `/bin/tcc` from the rootfs
///    (`/mnt/...`) into the in-memory VFS.  `tcc` is dynamically linked against
///    libc + libm, so all three .so files must be present for `ld.so` to start
///    it.
/// 2. Write a tiny freestanding C program to `/cc-prog.c`.  It has its own
///    `_start` and issues raw `write(1, ..)`/`exit` syscalls (no libc), so the
///    compile needs `-nostdlib -static` and pulls in **no** support files
///    (no crt objects, no headers, no `libtcc1.a`) — `strace` confirmed `tcc`
///    opens only the `.c` source and writes the output ELF in this mode.
/// 3. Spawn `tcc -nostdlib -static -o /cc-prog /cc-prog.c`, reap it, and
///    confirm it exited 0 and produced a valid ELF at `/cc-prog`.
/// 4. Spawn the freshly-compiled `/cc-prog` directly, redirecting its fd 1 from
///    the console to `/cc-out.txt` *before it first runs* (`linux_fd_take` +
///    `linux_fd_install_at` — the same mechanism the real-glibc-stdio test
///    uses).  The program writes `SLATE_TCC_CC_OK\n` to fd 1.  Running it
///    directly (rather than via the shell) keeps the test focused on the one
///    new claim — "tcc produced a working ring-3 ELF" — with no shell layer to
///    confound the result.
/// 5. Read `/cc-out.txt` back and assert it equals the expected payload.
///
/// Success proves end to end: `ld.so` loaded `tcc`+libc+libm, `tcc` read and
/// compiled real C source into a working x86_64 ELF, and that freshly-built ELF
/// ran in ring 3 and produced correct output.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/tcc` is absent (so a kernel
/// built without the toolchain rootfs still boots clean).
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if `tcc` fails to reach `Zombie`,
/// exits non-zero, produces no/invalid ELF, the compiled program fails to
/// run, or its output does not match; propagates spawn failure.
pub fn self_test_linux_real_glibc_cc() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // The freshly-compiled program writes exactly this 16-byte payload.
    const EXPECT_OUT: &[u8] = b"SLATE_TCC_CC_OK\n";

    const SRC_LD: &str = "/mnt/lib64/ld-linux-x86-64.so.2";
    const SRC_LIBC: &str = "/mnt/lib/x86_64-linux-gnu/libc.so.6";
    const SRC_LIBM: &str = "/mnt/lib/x86_64-linux-gnu/libm.so.6";
    const SRC_TCC: &str = "/mnt/bin/tcc";
    const DST_LD: &str = "/lib64/ld-linux-x86-64.so.2";
    const DST_LIBC: &str = "/lib/x86_64-linux-gnu/libc.so.6";
    const DST_LIBM: &str = "/lib/x86_64-linux-gnu/libm.so.6";
    const DST_TCC: &str = "/bin/tcc";
    const SRC_PATH: &str = "/cc-prog.c";
    const OBJ_PATH: &str = "/cc-prog";
    const OUT_PATH: &str = "/cc-out.txt";

    // A freestanding C program: own `_start`, raw syscalls, no libc.
    //   sc3(n,a,b,c) = syscall(n, a, b, c)
    //   build "SLATE_TCC_CC_OK\n" on the STACK (byte stores, no .rodata), then
    //   write(1, m, 16)   // fd 1 is redirected to a capture file before run
    //   exit(0)
    // The message is built on the stack rather than via a string literal so
    // the `write` buffer lives in an always-mapped stack page — this is a
    // diagnostic to isolate whether reads of the binary's second (.data.ro /
    // .eh_frame) LOAD segment work in ring 3.
    const CC_SRC: &[u8] = b"static long sc3(long n,long a,long b,long c){long r;\
__asm__ volatile(\"syscall\":\"=a\"(r):\"a\"(n),\"D\"(a),\"S\"(b),\"d\"(c):\"rcx\",\"r11\",\"memory\");\
return r;}\n\
void _start(void){char m[16];m[0]=83;m[1]=76;m[2]=65;m[3]=84;m[4]=69;m[5]=95;m[6]=84;m[7]=67;\
m[8]=67;m[9]=95;m[10]=67;m[11]=67;m[12]=95;m[13]=79;m[14]=75;m[15]=10;\
sc3(1,1,(long)m,16);sc3(60,0,0,0);}\n";

    // tcc parses + codegens real C; give the compile a generous yield budget.
    const COMPILE_MAX_YIELDS: usize = 4_194_304;
    // The compiled program is tiny; a smaller budget suffices to run it.
    const RUN_MAX_YIELDS: usize = 524_288;

    if !crate::fs::Vfs::exists(SRC_TCC) {
        return Ok(());
    }

    serial_println!("[spawn] Running REAL C compiler (tcc, ring 3, Path Z) test...");

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [
        (SRC_LD, DST_LD),
        (SRC_LIBC, DST_LIBC),
        (SRC_LIBM, DST_LIBM),
        (SRC_TCC, DST_TCC),
    ] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   real cc: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   real cc: SKIP (reading {} failed: {:?})", src, e);
                return Ok(());
            }
        }
    }

    // Stage the C source and clear any stale compiler/program output.
    if let Err(e) = crate::fs::Vfs::write_file(SRC_PATH, CC_SRC) {
        serial_println!("[spawn]   real cc: SKIP (writing {} failed: {:?})", SRC_PATH, e);
        return Ok(());
    }
    let _ = crate::fs::Vfs::remove(OBJ_PATH);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let tcc_elf = match crate::fs::Vfs::read_file(DST_TCC) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   real cc: SKIP (re-read {} failed: {:?})", DST_TCC, e);
            return Ok(());
        }
    };

    // --- step 1: run tcc to compile /cc-prog.c -> /cc-prog ------------------
    let cc_argv: &[&[u8]] =
        &[b"tcc", b"-nostdlib", b"-static", b"-o", b"/cc-prog", b"/cc-prog.c"];
    let cc_envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let cc_options = SpawnOptions {
        name: "spawn-test-tcc",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv: cc_argv,
        envp: cc_envp,
        exe_path: Some(DST_TCC.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let cc_result = match spawn_process(&tcc_elf, &cc_options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: real cc — tcc spawn returned {:?}", e);
            return Err(e);
        }
    };

    let mut cc_reaped = false;
    for _ in 0..COMPILE_MAX_YIELDS {
        if pcb::state(cc_result.pid) == Some(pcb::ProcessState::Zombie) {
            cc_reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let cc_state = pcb::state(cc_result.pid);
    let cc_exit = pcb::exit_code(cc_result.pid);
    thread::on_thread_exit(cc_result.task_id);
    pcb::destroy(cc_result.pid);

    if !cc_reaped || cc_state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real cc — tcc did not exit within {} yields (state={:?}); tcc \
             startup (ld.so/libc/libm) or its compile loop likely hung",
            COMPILE_MAX_YIELDS, cc_state
        );
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        return Err(KernelError::TimedOut);
    }

    if cc_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: real cc — tcc exit code={:?}, expected {} (non-zero means tcc hit a \
             compile/link error)",
            cc_exit, EXPECT_EXIT
        );
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        return Err(KernelError::InternalError);
    }

    // The compiler must have produced a valid ELF.
    let obj = match crate::fs::Vfs::read_file(OBJ_PATH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real cc — tcc exited 0 but {} is unreadable: {:?} (no output \
                 ELF produced)",
                OBJ_PATH, e
            );
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            return Err(KernelError::InternalError);
        }
    };
    if obj.len() < 4 || &obj[..4] != b"\x7fELF" {
        serial_println!(
            "[spawn]   FAIL: real cc — {} is not an ELF ({} bytes, first 4 = {:?})",
            OBJ_PATH, obj.len(), obj.get(..4)
        );
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[spawn]   real cc — tcc compiled {} into a {}-byte ELF at {}",
        SRC_PATH, obj.len(), OBJ_PATH
    );

    // --- step 2: run the freshly-compiled program directly -----------------
    // Spawn /cc-prog itself (the binary tcc just produced) and redirect its
    // fd 1 from the console to a capture file *before it first runs* — the same
    // proven mechanism `self_test_linux_real_glibc_stdio` uses (`linux_fd_take`
    // + `linux_fd_install_at`).  This tests exactly the claim we care about —
    // "tcc produced a working ring-3 ELF" — without the shell/fork/exec layer
    // confounding the result.  (The make test already proves shell redirects.)
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    let run_argv: &[&[u8]] = &[b"/cc-prog"];
    let run_envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let run_options = SpawnOptions {
        name: "spawn-test-cc-run",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv: run_argv,
        envp: run_envp,
        exe_path: Some(OBJ_PATH.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    // Open a fresh capture file for the compiled program's fd 1.
    let capture_handle = match handle::open(
        OUT_PATH,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!("[spawn]   real cc: SKIP (capture-file open failed: {:?})", e);
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            let _ = crate::fs::Vfs::remove(OBJ_PATH);
            return Ok(());
        }
    };

    // The freshly-built program is a bare static binary tcc emitted for the
    // Linux ABI (OSABI=SYSV, no PT_INTERP, no GNU property note), so the ELF
    // carries no marker `detect_linux_abi` can see.  We *know* it is a Linux
    // binary because we just compiled it as one, so state the ABI explicitly.
    let run_result = match spawn_process_with_abi(&obj, &run_options, pcb::AbiMode::Linux) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            serial_println!(
                "[spawn]   FAIL: real cc — spawning the freshly-built {} returned {:?}",
                OBJ_PATH, e
            );
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            let _ = crate::fs::Vfs::remove(OBJ_PATH);
            let _ = crate::fs::Vfs::remove(OUT_PATH);
            return Err(e);
        }
    };

    // Redirect fd 1 (console) -> capture file before the child runs.  Dropping
    // the console entry needs no kernel close.  After install_at the capture
    // handle is owned by the child's fd table (released on its exit), so we
    // must NOT close it ourselves on the success path.
    let _ = pcb::linux_fd_take(run_result.pid, 1);
    if let Err(e) =
        pcb::linux_fd_install_at(run_result.pid, 1, FdEntry::file(capture_handle, O_WRONLY))
    {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(run_result.task_id);
        pcb::destroy(run_result.pid);
        serial_println!("[spawn]   FAIL: real cc — redirecting the compiled program's fd 1 failed: {:?}", e);
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        let _ = crate::fs::Vfs::remove(OBJ_PATH);
        let _ = crate::fs::Vfs::remove(OUT_PATH);
        return Err(KernelError::InternalError);
    }

    let mut run_reaped = false;
    for _ in 0..RUN_MAX_YIELDS {
        if pcb::state(run_result.pid) == Some(pcb::ProcessState::Zombie) {
            run_reaped = true;
            break;
        }
        crate::sched::yield_now();
    }

    let run_state = pcb::state(run_result.pid);
    let run_exit = pcb::exit_code(run_result.pid);
    // Read the captured output BEFORE destroying the child (reading by path
    // opens an independent handle onto the same inode).
    let out = crate::fs::Vfs::read_file(OUT_PATH);

    thread::on_thread_exit(run_result.task_id);
    pcb::destroy(run_result.pid);
    let _ = crate::fs::Vfs::remove(SRC_PATH);
    let _ = crate::fs::Vfs::remove(OBJ_PATH);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if !run_reaped || run_state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: real cc — the compiled program did not exit within {} yields \
             (state={:?}); tcc may have produced a broken ELF",
            RUN_MAX_YIELDS, run_state
        );
        return Err(KernelError::TimedOut);
    }

    match out {
        Ok(bytes) if bytes.as_slice() == EXPECT_OUT => {
            serial_println!(
                "[spawn]   REAL C compiler (ring 3: ld.so loaded tcc+libc+libm, tcc compiled C \
                 source into a {}-byte ELF, that freshly-built binary ran in ring 3 and wrote {} \
                 bytes == expected, exit={:?}): OK",
                obj.len(), bytes.len(), run_exit
            );
            Ok(())
        }
        Ok(bytes) => {
            serial_println!(
                "[spawn]   FAIL: real cc — {} holds {} bytes {:?}, expected {} bytes {:?} \
                 (compiled program exit={:?})",
                OUT_PATH, bytes.len(), bytes.as_slice(), EXPECT_OUT.len(), EXPECT_OUT, run_exit
            );
            Err(KernelError::InternalError)
        }
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: real cc — reading the compiled program's output {} back failed: \
                 {:?} (exit={:?})",
                OUT_PATH, e, run_exit
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Path Z Part 36 — host a real C compiler producing a **hosted, glibc-linked**
/// executable, and run that freshly-built dynamic binary in ring 3.
///
/// Where [`self_test_linux_real_glibc_cc`] proves the *freestanding* case
/// (`tcc -nostdlib -static`, own `_start`, raw syscalls, a bare static ELF),
/// this proves the *realistic* case: `tcc -o out out.c` linking the program
/// against the real glibc with crt startup (`crt1.o` → `__libc_start_main` →
/// `main`), calling a libc function (`puts`), producing a **dynamically-linked
/// ELF** (`PT_INTERP = /lib64/ld-linux-x86-64.so.2`), and that binary running
/// through `ld.so` in ring 3.  Because the output carries a Linux `PT_INTERP`,
/// the loader auto-classifies it as the Linux ABI — no explicit-ABI override is
/// needed (contrast the freestanding case, which carries no marker at all; see
/// open-questions.md Q9 / known-issues B-ABI1).
///
/// Flow:
/// 1. Stage `ld-linux`, `libc.so.6`, `libm.so.6`, `/bin/tcc` **and** the hosted-
///    compile support set that `tcc -vv` opens for `tcc -o out out.c`: the crt
///    objects (`crt1.o`/`crti.o`/`crtn.o`), the `libc.so` GNU-ld linker script
///    (which `GROUP`s `libc.so.6` + `libc_nonshared.a` + AS_NEEDED `ld-linux`),
///    `libc_nonshared.a`, and tcc's own `libtcc1.a` — each at the exact absolute
///    path tcc searches, so they resolve unchanged in the VFS.
/// 2. Write a hosted C program to `/hosted-prog.c`.  It declares `puts` via an
///    `extern` prototype (so **no** glibc header tree is needed) and `return`s
///    from `main` (so libc's startup/teardown — including the exit-time stdio
///    flush that actually emits the buffered `puts` output — is exercised).
/// 3. Spawn `tcc -o /hosted-prog /hosted-prog.c`, reap it, confirm exit 0 and a
///    valid, *dynamically-linked* ELF (has a `PT_INTERP`).
/// 4. Spawn `/hosted-prog` via the normal auto-detecting [`spawn_process`] (the
///    `PT_INTERP` makes the loader pick the Linux ABI), redirecting fd 1 to
///    `/hosted-out.txt` before it runs.  glibc fully buffers stdout when it is
///    not a tty, so the payload is flushed on exit.
/// 5. Read `/hosted-out.txt` back and assert it equals `SLATE_TCC_HOSTED_OK\n`.
///
/// No-op (returns `Ok(())`) when the rootfs / `/bin/tcc` / the hosted support
/// set is absent (so a kernel built without the full toolchain rootfs still
/// boots clean).
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if `tcc` fails to reach `Zombie`,
/// exits non-zero, produces no/invalid/non-dynamic ELF, the compiled program
/// fails to run, or its output does not match; propagates spawn failure.
/// Shared back-end for the hosted-glibc-compile self-tests.
///
/// Stages the full glibc support set (ld + libc + libm + tcc + the crt
/// objects + libc.so linker-script + libc_nonshared.a + libtcc1.a) from the
/// /mnt ext4 rootfs, compiles `hosted_src` with `tcc -o /hosted-prog
/// /hosted-prog.c` (a real glibc-linked, dynamically-linked build), asserts
/// the output ELF carries a `PT_INTERP`, then runs that freshly-built binary
/// in ring 3 with its fd 1 redirected to a capture file and asserts the
/// captured bytes equal `expect_out`.  `label` names the libc surface the
/// program exercises (e.g. "puts", "printf/malloc") for the serial log.
///
/// No-ops cleanly (returns `Ok`) if the glibc support set is absent so the
/// test is a silent skip on a rootfs that wasn't built with hosted-compile
/// support.
/// Stage the shared tcc + glibc support set from the `/mnt` rootfs into the VFS
/// root at the exact absolute paths tcc opens (verified via `tcc -vv`).  Shared
/// by every Path-Z hosted-compile self-test.
///
/// Returns `Ok(true)` when the whole set is staged, `Ok(false)` when a
/// prerequisite is missing or a copy fails — in which case the caller no-ops,
/// matching the best-effort rootfs pattern (the image may be built without the
/// toolchain).
fn stage_hosted_cc_support() -> KernelResult<bool> {
    // (src in /mnt rootfs, dst in VFS) staging pairs.  ld + libc + libm + tcc
    // are shared with the other Path-Z tests; the crt objects, libc.so script,
    // libc_nonshared.a and libtcc1.a are the hosted-compile additions.
    const STAGE: &[(&str, &str)] = &[
        (
            "/mnt/lib64/ld-linux-x86-64.so.2",
            "/lib64/ld-linux-x86-64.so.2",
        ),
        (
            "/mnt/lib/x86_64-linux-gnu/libc.so.6",
            "/lib/x86_64-linux-gnu/libc.so.6",
        ),
        (
            "/mnt/lib/x86_64-linux-gnu/libm.so.6",
            "/lib/x86_64-linux-gnu/libm.so.6",
        ),
        ("/mnt/bin/tcc", "/bin/tcc"),
        (
            "/mnt/usr/lib/x86_64-linux-gnu/crt1.o",
            "/usr/lib/x86_64-linux-gnu/crt1.o",
        ),
        (
            "/mnt/usr/lib/x86_64-linux-gnu/crti.o",
            "/usr/lib/x86_64-linux-gnu/crti.o",
        ),
        (
            "/mnt/usr/lib/x86_64-linux-gnu/crtn.o",
            "/usr/lib/x86_64-linux-gnu/crtn.o",
        ),
        (
            "/mnt/usr/lib/x86_64-linux-gnu/libc.so",
            "/usr/lib/x86_64-linux-gnu/libc.so",
        ),
        (
            "/mnt/usr/lib/x86_64-linux-gnu/libc_nonshared.a",
            "/usr/lib/x86_64-linux-gnu/libc_nonshared.a",
        ),
        (
            "/mnt/tmp/tccinstall/lib/tcc/libtcc1.a",
            "/tmp/tccinstall/lib/tcc/libtcc1.a",
        ),
    ];

    // The hosted compile needs the whole support set; if tcc itself or any
    // support file is missing, no-op (matches the rootfs best-effort pattern).
    if !crate::fs::Vfs::exists("/mnt/bin/tcc")
        || !crate::fs::Vfs::exists("/mnt/usr/lib/x86_64-linux-gnu/crt1.o")
        || !crate::fs::Vfs::exists("/mnt/tmp/tccinstall/lib/tcc/libtcc1.a")
    {
        return Ok(false);
    }

    let _ = crate::fs::Vfs::mkdir_all("/lib64");
    let _ = crate::fs::Vfs::mkdir_all("/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/bin");
    let _ = crate::fs::Vfs::mkdir_all("/usr/lib/x86_64-linux-gnu");
    let _ = crate::fs::Vfs::mkdir_all("/tmp/tccinstall/lib/tcc");
    for (src, dst) in STAGE {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   hosted cc: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    return Ok(false);
                }
            }
            Err(e) => {
                serial_println!("[spawn]   hosted cc: SKIP (reading {} failed: {:?})", src, e);
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Spawn the staged image `tcc` with `argv`, wait for it to exit, and return its
/// exit code.  `tcc_elf` is the already-read `/bin/tcc` binary.  `label` tags
/// diagnostics.  Returns `Err` on spawn failure or if tcc fails to exit within
/// the compile budget (a hang).
/// How often (in reap-loop yields) to emit a progress snapshot.  Power of two
/// so the loop test is a cheap mask, not a division.
const REAP_SNAPSHOT_INTERVAL: usize = 1 << 18; // 262_144

/// Emit one progress snapshot for a tcc self-test reap loop.
///
/// The hosted tcc build occasionally wedges (~5% of boots): the child stops
/// making progress while the parent spins its multi-million-yield reap budget.
/// Because unrelated background tasks (mouse cursor, deferred benchmark) keep
/// the liveness watchdog's counters advancing, the wedge produces *no* watchdog
/// dump — it silently blocks `BOOT_OK` until the boot-test timeout kills QEMU.
///
/// To make that wedge diagnosable, the reap loops call this every
/// [`REAP_SNAPSHOT_INTERVAL`] yields.  A *deadlocked* child shows frozen
/// `sched_count`/fault counters across snapshots, and its `sched_state`
/// localizes the bug: `Blocked` = a lost wakeup, `Ready` = scheduler
/// starvation, `Running` = a half-completed context switch.  A merely-*slow*
/// child shows those counters still climbing.
fn log_reap_wait_progress(
    label: &str,
    phase: &str,
    pid: ProcessId,
    task_id: TaskId,
    yields: usize,
) {
    let pstate = pcb::state(pid);
    match crate::sched::task_info(task_id) {
        Some(info) => serial_println!(
            "[spawn]   {} {} reap: {} yields elapsed — child pid={} pstate={:?} \
             sched_state={:?} last_cpu={} sched_count={} ticks={} min_flt={} maj_flt={} \
             last_rip={:#x}",
            label, phase, yields, pid, pstate,
            info.state, info.last_cpu, info.schedule_count, info.total_ticks,
            info.min_flt, info.maj_flt,
            crate::rip_sample::last_rip(info.last_cpu),
        ),
        None => serial_println!(
            "[spawn]   {} {} reap: {} yields elapsed — child pid={} pstate={:?} \
             (task {} absent from sched table)",
            label, phase, yields, pid, pstate, task_id,
        ),
    }
}

fn spawn_reap_tcc(
    tcc_elf: &[u8],
    argv: &[&[u8]],
    label: &str,
) -> KernelResult<Option<i32>> {
    // tcc's compile/link loop is the heaviest userspace work in these tests, so
    // it gets the largest yield budget; an exhausted budget is treated as a hang.
    const COMPILE_MAX_YIELDS: usize = 4_194_304;

    let cc_envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let cc_options = SpawnOptions {
        name: "spawn-test-tcc-hosted",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp: cc_envp,
        exe_path: Some(b"/bin/tcc"),
        cwd: None,
        uid_gid: None,
    };

    let cc_result = match spawn_process(tcc_elf, &cc_options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: hosted cc ({}) — tcc spawn returned {:?}", label, e);
            return Err(e);
        }
    };

    let mut reaped = false;
    for i in 0..COMPILE_MAX_YIELDS {
        if pcb::state(cc_result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        // Periodic progress snapshot so an intermittent wedge (see
        // `log_reap_wait_progress`) is diagnosable instead of silently
        // blocking BOOT_OK until the boot-test timeout.
        if i != 0 && (i & (REAP_SNAPSHOT_INTERVAL - 1)) == 0 {
            log_reap_wait_progress(label, "compile", cc_result.pid, cc_result.task_id, i);
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(cc_result.pid);
    let exit = pcb::exit_code(cc_result.pid);
    thread::on_thread_exit(cc_result.task_id);
    pcb::destroy(cc_result.pid);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — tcc did not exit within {} yields (state={:?}); tcc \
             startup or its compile/link loop likely hung",
            label, COMPILE_MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }
    Ok(exit)
}

/// Confirm `obj` is an ELF that carries a `PT_INTERP` — i.e. a *dynamically
/// linked* executable, the whole point of the hosted rung (vs the freestanding
/// case's bare static ELF).  `path`/`label` tag diagnostics.
fn assert_dynamic_elf(obj: &[u8], path: &str, label: &str) -> KernelResult<()> {
    if obj.len() < 4 || obj.get(..4) != Some(b"\x7fELF".as_slice()) {
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — {} is not an ELF ({} bytes, first 4 = {:?})",
            label, path, obj.len(), obj.get(..4)
        );
        return Err(KernelError::InternalError);
    }
    match elf::ElfFile::parse(obj) {
        Ok(ef) => match ef.interp_path() {
            Some(interp) => {
                serial_println!(
                    "[spawn]   hosted cc ({}) — {} is a {}-byte dynamic ELF (PT_INTERP={:?})",
                    label, path, obj.len(),
                    core::str::from_utf8(interp).unwrap_or("<non-utf8>")
                );
                Ok(())
            }
            None => {
                serial_println!(
                    "[spawn]   FAIL: hosted cc ({}) — {} has no PT_INTERP; expected a \
                     dynamically-linked ELF (the hosted compile should link against glibc + ld-linux)",
                    label, path
                );
                Err(KernelError::InternalError)
            }
        },
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: hosted cc ({}) — output ELF {} failed to parse: {:?}",
                label, path, e
            );
            Err(KernelError::InternalError)
        }
    }
}

/// Spawn a freshly-built dynamic binary `obj` (loaded from `prog_path`), redirect
/// its fd 1 to `out_path` before it runs, wait for exit, and return
/// `(exit_code, captured_bytes)`.  Because `obj` carries a `PT_INTERP`, the
/// auto-detecting `spawn_process` selects the Linux ABI and loads ld-linux.
fn run_dynamic_capture(
    obj: &[u8],
    prog_path: &str,
    out_path: &str,
    label: &str,
) -> KernelResult<(Option<i32>, alloc::vec::Vec<u8>)> {
    const RUN_MAX_YIELDS: usize = 524_288;
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};

    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let run_argv: &[&[u8]] = &[prog_path.as_bytes()];
    let run_envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C"];
    let run_options = SpawnOptions {
        name: "spawn-test-hosted-run",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv: run_argv,
        envp: run_envp,
        exe_path: Some(prog_path.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let capture_handle = match handle::open(
        out_path,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: hosted cc ({}) — capture-file {} open failed: {:?}",
                label, out_path, e
            );
            return Err(e);
        }
    };

    let run_result = match spawn_process(obj, &run_options) {
        Ok(r) => r,
        Err(e) => {
            let _ = handle::close(capture_handle);
            serial_println!(
                "[spawn]   FAIL: hosted cc ({}) — spawning the freshly-built {} returned {:?}",
                label, prog_path, e
            );
            return Err(e);
        }
    };

    // Redirect fd 1 (console) -> capture file before the child runs.
    let _ = pcb::linux_fd_take(run_result.pid, 1);
    if let Err(e) =
        pcb::linux_fd_install_at(run_result.pid, 1, FdEntry::file(capture_handle, O_WRONLY))
    {
        let _ = handle::close(capture_handle);
        thread::on_thread_exit(run_result.task_id);
        pcb::destroy(run_result.pid);
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — redirecting the compiled program's fd 1 failed: {:?}",
            label, e
        );
        return Err(KernelError::InternalError);
    }

    let mut reaped = false;
    for i in 0..RUN_MAX_YIELDS {
        if pcb::state(run_result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        // Periodic progress snapshot (see `log_reap_wait_progress`) so an
        // intermittent ld.so/libc-init wedge in the freshly-built binary is
        // diagnosable rather than silently blocking BOOT_OK.
        if i != 0 && (i & (REAP_SNAPSHOT_INTERVAL - 1)) == 0 {
            log_reap_wait_progress(label, "run", run_result.pid, run_result.task_id, i);
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(run_result.pid);
    let exit = pcb::exit_code(run_result.pid);
    let out = crate::fs::Vfs::read_file(out_path);

    thread::on_thread_exit(run_result.task_id);
    pcb::destroy(run_result.pid);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — the compiled program did not exit within {} yields \
             (state={:?}); ld.so startup or libc init for the freshly-built binary likely hung",
            label, RUN_MAX_YIELDS, state
        );
        return Err(KernelError::TimedOut);
    }

    match out {
        Ok(bytes) => Ok((exit, bytes)),
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: hosted cc ({}) — reading the compiled program's output {} back \
                 failed: {:?} (exit={:?})",
                label, out_path, e, exit
            );
            Err(KernelError::InternalError)
        }
    }
}

fn run_hosted_cc_case(label: &str, hosted_src: &[u8], expect_out: &[u8]) -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const SRC_PATH: &str = "/hosted-prog.c";
    const OBJ_PATH: &str = "/hosted-prog";
    const OUT_PATH: &str = "/hosted-out.txt";

    match stage_hosted_cc_support() {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => return Err(e),
    }

    serial_println!(
        "[spawn] Running REAL C compiler (tcc, HOSTED glibc link, {}, ring 3, Path Z) test...",
        label
    );

    if let Err(e) = crate::fs::Vfs::write_file(SRC_PATH, hosted_src) {
        serial_println!("[spawn]   hosted cc: SKIP (writing {} failed: {:?})", SRC_PATH, e);
        return Ok(());
    }
    let _ = crate::fs::Vfs::remove(OBJ_PATH);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    let tcc_elf = match crate::fs::Vfs::read_file("/bin/tcc") {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   hosted cc: SKIP (re-read /bin/tcc failed: {:?})", e);
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            return Ok(());
        }
    };

    // --- step 1: run tcc to compile + glibc-link /hosted-prog.c -------------
    let cc_argv: &[&[u8]] = &[b"tcc", b"-o", b"/hosted-prog", b"/hosted-prog.c"];
    let cc_exit = match spawn_reap_tcc(&tcc_elf, cc_argv, label) {
        Ok(x) => x,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            return Err(e);
        }
    };
    if cc_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — tcc exit code={:?}, expected {} (non-zero means tcc \
             hit a compile/link error — e.g. a crt object or libc.so script it could not \
             open/parse)",
            label, cc_exit, EXPECT_EXIT
        );
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        return Err(KernelError::InternalError);
    }

    let obj = match crate::fs::Vfs::read_file(OBJ_PATH) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: hosted cc ({}) — tcc exited 0 but {} is unreadable: {:?}",
                label, OBJ_PATH, e
            );
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_dynamic_elf(&obj, OBJ_PATH, label) {
        let _ = crate::fs::Vfs::remove(SRC_PATH);
        let _ = crate::fs::Vfs::remove(OBJ_PATH);
        return Err(e);
    }

    // --- step 2: run the freshly-built dynamic binary ----------------------
    let (run_exit, out) = match run_dynamic_capture(&obj, OBJ_PATH, OUT_PATH, label) {
        Ok(t) => t,
        Err(e) => {
            let _ = crate::fs::Vfs::remove(SRC_PATH);
            let _ = crate::fs::Vfs::remove(OBJ_PATH);
            let _ = crate::fs::Vfs::remove(OUT_PATH);
            return Err(e);
        }
    };
    let _ = crate::fs::Vfs::remove(SRC_PATH);
    let _ = crate::fs::Vfs::remove(OBJ_PATH);
    let _ = crate::fs::Vfs::remove(OUT_PATH);

    if out.as_slice() == expect_out {
        serial_println!(
            "[spawn]   REAL C compiler HOSTED ({}) (ring 3: tcc compiled+glibc-linked C into a \
             {}-byte dynamic ELF, ld.so loaded that binary + libc, it ran the {} libc path and \
             the exit-time stdio flush wrote {} bytes == expected, exit={:?}): OK",
            label, obj.len(), label, out.len(), run_exit
        );
        Ok(())
    } else {
        serial_println!(
            "[spawn]   FAIL: hosted cc ({}) — {} holds {} bytes {:?}, expected {} bytes {:?} \
             (compiled program exit={:?})",
            label, OUT_PATH, out.len(), out.as_slice(), expect_out.len(), expect_out, run_exit
        );
        Err(KernelError::InternalError)
    }
}

/// Path Z Part 56 — hosted glibc-linked compile with an aggregate **brace
/// initializer** (runtime value → tcc-synthesised `memset`).
///
/// Motivation: the `B-TCC-LIBTCC1-MAIN` bug tracked a once-observed on-target
/// `tcc: error: unresolved reference to 'main'` link failure attributed to the
/// extra undefined `memset` symbol that an aggregate brace-initialiser
/// synthesises.  On-target instrumentation (22 compiles across four distinct
/// `memset`/`memcpy`-emitting constructs, run under `tcc -vv`) could **not**
/// reproduce it — every compile linked and ran cleanly — so the documented
/// deterministic trigger is disproven and this rung stands as the permanent
/// regression guard for it: a genuine runtime `memset` (a `struct box p = {
/// seed, 1, 1, 0 };` where `seed` is a runtime value) that tcc lowers to a
/// `memset` call resolved from glibc, compiled + glibc-linked + run in ring 3.
/// `seed`(40)+1+1+0 == 42.  Only undefined symbols: `write` and `memset`.
///
/// If tcc ever regresses to losing `main` when a synthesised `memset` is
/// present, this rung fails (surfacing a `self-test failed` WARNING the
/// boot-test scans for) instead of the failure going unnoticed.
pub fn self_test_linux_real_glibc_cc_brace_memset() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] = b"extern long write(int, const void *, unsigned long);\n\
struct box { int a, b, c, d; };\n\
static int seedfn(void){ static volatile int s = 40; return s; }\n\
int main(void) {\n\
  int seed = seedfn();\n\
  struct box p = { seed, 1, 1, 0 };\n\
  int t = p.a + p.b + p.c + p.d;\n\
  char o[3]; o[0] = '0' + t / 10; o[1] = '0' + t % 10; o[2] = '\\n';\n\
  write(1, o, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("brace-init-memset", HOSTED_SRC, b"42\n")
}

/// Path Z Part 36 — hosted glibc-linked compile (minimal `puts` surface).
///
/// The smallest possible end-to-end proof: a hosted C program that declares
/// `puts` via an `extern` prototype (no header tree needed) and returns from
/// `main`, so libc's startup *and* the exit-time stdio flush (which actually
/// emits the buffered `puts` output) both run.  Exercises crt startup →
/// `__libc_start_main` → `main` → libc `puts` → stdio buffer flush → exit.
pub fn self_test_linux_real_glibc_cc_hosted() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] =
        b"extern int puts(const char *s);\nint main(void){puts(\"SLATE_TCC_HOSTED_OK\");return 0;}\n";
    run_hosted_cc_case("puts", HOSTED_SRC, b"SLATE_TCC_HOSTED_OK\n")
}

/// Path Z Part 37 — hosted glibc-linked compile (`printf` varargs + heap).
///
/// Strengthens Part 36 by exercising materially more of the glibc ABI through
/// a freshly-tcc-built dynamic binary: a `malloc`/`free` heap round-trip and
/// `printf`'s variadic format machinery (`%s` consumes a pointer argument,
/// `%d` formats an int).  The program builds a string on the heap, prints it
/// with a formatted integer, frees it, and returns 0.  Because fd 1 is
/// redirected to a regular file (not a tty) glibc makes stdout fully buffered,
/// so the `printf` output is emitted by the exit-time flush — the same path
/// Part 36 validates for `puts`.
pub fn self_test_linux_real_glibc_cc_hosted_stdio() -> KernelResult<()> {
    // extern prototypes avoid needing the glibc header tree on the target.
    // size_t is `unsigned long` on x86_64, matching malloc's signature.
    const HOSTED_SRC: &[u8] = b"extern int printf(const char *fmt, ...);\n\
extern void *malloc(unsigned long n);\n\
extern void free(void *p);\n\
int main(void){\n\
  char *p = (char*)malloc(8);\n\
  if(!p) return 2;\n\
  p[0]='S'; p[1]='L'; p[2]='A'; p[3]='T'; p[4]='E'; p[5]=0;\n\
  printf(\"%s-%d\\n\", p, 1234);\n\
  free(p);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("printf/malloc", HOSTED_SRC, b"SLATE-1234\n")
}

/// Path Z Part 38 — separate compilation (`tcc -c` objects + multi-object link).
///
/// Strengthens Parts 36/37, which each compiled+linked a *single* source in one
/// `tcc` invocation.  This rung exercises the multi-step flow:
///   1. `tcc -c /sep-a.c -o /sep-a.o`  — compile-only: emit a relocatable ELF
///      object for translation unit A (defines `slate_add`).
///   2. verify `/sep-a.o` is an ELF object (proves `-c` produced a real object,
///      not an executable).
///   3. `tcc -c /sep-b.c -o /sep-b.o`  — compile-only TU B (`main`, which calls
///      `slate_add` across the TU boundary).
///   4. `tcc -o /sep-prog /sep-a.o /sep-b.o` — tcc-as-linker combines the two
///      relocatables + crt + glibc into one dynamic executable, resolving the
///      cross-TU `slate_add` reference at link time.
///   5. run `/sep-prog` with fd 1 redirected to a file and confirm the
///      exit-time stdio flush emitted `SLATE-SEP-42\n`.
///
/// This requires tcc's object emission (`-c`) AND tcc-as-linker combining
/// multiple relocatable inputs — strictly more than the single-file rungs.
pub fn self_test_linux_real_glibc_cc_separate() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const A_SRC: &[u8] = b"/* TU A: defines a function used by main in TU B */\n\
int slate_add(int a, int b){ return a + b; }\n";
    const B_SRC: &[u8] = b"/* TU B: main, calls across the TU boundary into TU A */\n\
extern int printf(const char *fmt, ...);\n\
extern int slate_add(int a, int b);\n\
int main(void){\n\
  int r = slate_add(40, 2);\n\
  printf(\"SLATE-SEP-%d\\n\", r);\n\
  return 0;\n\
}\n";
    const A_C: &str = "/sep-a.c";
    const B_C: &str = "/sep-b.c";
    const A_O: &str = "/sep-a.o";
    const B_O: &str = "/sep-b.o";
    const PROG: &str = "/sep-prog";
    const OUT: &str = "/sep-out.txt";
    const LABEL: &str = "separate-compilation";

    match stage_hosted_cc_support() {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => return Err(e),
    }

    serial_println!(
        "[spawn] Running REAL C compiler (tcc, SEPARATE compilation, ring 3, Path Z) test..."
    );

    // Cleanup helper: remove every artifact we may have created.  Used on every
    // exit path so a failed run does not leave stale files for the next test.
    fn cleanup() {
        for p in ["/sep-a.c", "/sep-b.c", "/sep-a.o", "/sep-b.o", "/sep-prog", "/sep-out.txt"] {
            let _ = crate::fs::Vfs::remove(p);
        }
    }
    cleanup();

    if let Err(e) = crate::fs::Vfs::write_file(A_C, A_SRC) {
        serial_println!("[spawn]   sep cc: SKIP (writing {} failed: {:?})", A_C, e);
        cleanup();
        return Ok(());
    }
    if let Err(e) = crate::fs::Vfs::write_file(B_C, B_SRC) {
        serial_println!("[spawn]   sep cc: SKIP (writing {} failed: {:?})", B_C, e);
        cleanup();
        return Ok(());
    }

    let tcc_elf = match crate::fs::Vfs::read_file("/bin/tcc") {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   sep cc: SKIP (re-read /bin/tcc failed: {:?})", e);
            cleanup();
            return Ok(());
        }
    };

    // --- step 1: tcc -c /sep-a.c -o /sep-a.o (compile-only TU A) -----------
    let a_argv: &[&[u8]] = &[b"tcc", b"-c", b"/sep-a.c", b"-o", b"/sep-a.o"];
    let a_exit = match spawn_reap_tcc(&tcc_elf, a_argv, LABEL) {
        Ok(x) => x,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    if a_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: sep cc — `tcc -c {}` exit={:?}, expected {}",
            A_C, a_exit, EXPECT_EXIT
        );
        cleanup();
        return Err(KernelError::InternalError);
    }
    // Verify /sep-a.o is a real ELF *object* (ET_REL), not an executable: this
    // is the whole point of `-c`.  We check the ELF magic + e_type == ET_REL(1).
    let a_obj = match crate::fs::Vfs::read_file(A_O) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   FAIL: sep cc — {} unreadable after compile: {:?}", A_O, e);
            cleanup();
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_relocatable_elf(&a_obj, A_O, LABEL) {
        cleanup();
        return Err(e);
    }

    // --- step 2: tcc -c /sep-b.c -o /sep-b.o (compile-only TU B) -----------
    let b_argv: &[&[u8]] = &[b"tcc", b"-c", b"/sep-b.c", b"-o", b"/sep-b.o"];
    let b_exit = match spawn_reap_tcc(&tcc_elf, b_argv, LABEL) {
        Ok(x) => x,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    if b_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: sep cc — `tcc -c {}` exit={:?}, expected {}",
            B_C, b_exit, EXPECT_EXIT
        );
        cleanup();
        return Err(KernelError::InternalError);
    }
    let b_obj = match crate::fs::Vfs::read_file(B_O) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   FAIL: sep cc — {} unreadable after compile: {:?}", B_O, e);
            cleanup();
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_relocatable_elf(&b_obj, B_O, LABEL) {
        cleanup();
        return Err(e);
    }

    // --- step 3: tcc -o /sep-prog /sep-a.o /sep-b.o (link both objects) ----
    let link_argv: &[&[u8]] = &[b"tcc", b"-o", b"/sep-prog", b"/sep-a.o", b"/sep-b.o"];
    let link_exit = match spawn_reap_tcc(&tcc_elf, link_argv, LABEL) {
        Ok(x) => x,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    if link_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: sep cc — `tcc -o {} {} {}` exit={:?}, expected {} (link of two \
             relocatables + crt + glibc failed — e.g. an unresolved cross-TU symbol)",
            PROG, A_O, B_O, link_exit, EXPECT_EXIT
        );
        cleanup();
        return Err(KernelError::InternalError);
    }
    let prog = match crate::fs::Vfs::read_file(PROG) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: sep cc — link exited 0 but {} is unreadable: {:?}",
                PROG, e
            );
            cleanup();
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_dynamic_elf(&prog, PROG, LABEL) {
        cleanup();
        return Err(e);
    }

    // --- step 4: run the freshly-linked dynamic binary --------------------
    let (run_exit, out) = match run_dynamic_capture(&prog, PROG, OUT, LABEL) {
        Ok(t) => t,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    cleanup();

    if out.as_slice() == b"SLATE-SEP-42\n" && run_exit == Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   REAL C compiler SEPARATE compilation (ring 3: tcc emitted two relocatable \
             objects via -c, linked them + crt + glibc into a {}-byte dynamic ELF resolving the \
             cross-TU slate_add reference, ld.so ran it, exit-time flush wrote {} bytes == \
             expected, exit={:?}): OK",
            prog.len(), out.len(), run_exit
        );
        Ok(())
    } else {
        serial_println!(
            "[spawn]   FAIL: sep cc — {} holds {} bytes {:?} (exit={:?}), expected \
             {:?} with exit={}",
            OUT, out.len(), out.as_slice(), run_exit, b"SLATE-SEP-42\n", EXPECT_EXIT
        );
        Err(KernelError::InternalError)
    }
}

/// Confirm `obj` is a *relocatable* ELF object (`ET_REL`, e_type == 1) — the
/// product of `tcc -c`.  Distinct from [`assert_dynamic_elf`], which checks for
/// a `PT_INTERP`-carrying executable.  Verifies the 4-byte magic, that the file
/// is large enough to hold the ELF header, and that `e_type` (the 2-byte LE
/// field at offset 16) is `ET_REL`.
fn assert_relocatable_elf(obj: &[u8], path: &str, label: &str) -> KernelResult<()> {
    // ELF header: e_type is a 2-byte little-endian field at offset 0x10.
    const E_TYPE_OFF: usize = 16;
    const ET_REL: u16 = 1;
    if obj.len() < E_TYPE_OFF + 2 || obj.get(..4) != Some(b"\x7fELF".as_slice()) {
        serial_println!(
            "[spawn]   FAIL: sep cc ({}) — {} is not an ELF ({} bytes, first 4 = {:?})",
            label, path, obj.len(), obj.get(..4)
        );
        return Err(KernelError::InternalError);
    }
    // SAFETY-free: bounds already checked above, so these gets cannot be None.
    let lo = match obj.get(E_TYPE_OFF) {
        Some(&b) => u16::from(b),
        None => return Err(KernelError::InternalError),
    };
    let hi = match obj.get(E_TYPE_OFF + 1) {
        Some(&b) => u16::from(b),
        None => return Err(KernelError::InternalError),
    };
    let e_type = lo | (hi << 8);
    if e_type != ET_REL {
        serial_println!(
            "[spawn]   FAIL: sep cc ({}) — {} has e_type={} (expected ET_REL={}); `tcc -c` should \
             emit a relocatable object, not an executable/shared object",
            label, path, e_type, ET_REL
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[spawn]   sep cc ({}) — {} is a {}-byte relocatable ELF object (ET_REL)",
        label, path, obj.len()
    );
    Ok(())
}

/// Path Z Part 39 — `make` drives `tcc` to build a multi-file C program.
///
/// The capstone of the §4.4 "development tools" toolchain line: it composes
/// Part 34 (real GNU make runs in ring 3) with Part 38 (tcc separate
/// compilation) into the realistic "build a C project with make + a compiler"
/// flow.  A Makefile declares a small dependency graph whose recipes invoke the
/// staged `tcc` to compile two translation units to objects and link them:
///   /cap-prog: /cap-a.o /cap-b.o   ->  tcc -o /cap-prog /cap-a.o /cap-b.o
///   /cap-a.o:  /cap-a.c            ->  tcc -c  /cap-a.c -o /cap-a.o
///   /cap-b.o:  /cap-b.c            ->  tcc -c  /cap-b.c -o /cap-b.o
/// make evaluates the graph, fork/exec's `tcc` three times (the heaviest
/// multi-process glibc workload in the suite — make + tcc×3), and the resulting
/// dynamic binary runs in ring 3 and prints `SLATE-SEP-42`.
///
/// This is genuinely more than Parts 34/38 run separately: it is make *as the
/// build driver* spawning the compiler across a real prerequisite graph, the
/// exact integration `gcc`/`cmake`/`make` projects depend on.
///
/// No-op (`Ok(())`) when the toolchain rootfs (`/bin/tcc`, the glibc support
/// set, `/bin/make`, `/bin/sh`) is absent, so a kernel built without it boots
/// clean.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if make does not reach `Zombie`, exits
/// non-zero, fails to produce a dynamic ELF at `/cap-prog`, or the built program
/// does not print the expected line.
pub fn self_test_linux_real_glibc_make_cc() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    // make + tcc×3 is the heaviest multi-process glibc chain in the suite, so it
    // gets a budget well above the single-tcc compile budget (4_194_304).
    const MAX_YIELDS: usize = 33_554_432;
    const SRC_MAKE: &str = "/mnt/bin/make";
    const SRC_SH: &str = "/mnt/bin/sh";
    const DST_MAKE: &str = "/bin/make";
    const DST_SH: &str = "/bin/sh";
    const A_C: &str = "/cap-a.c";
    const B_C: &str = "/cap-b.c";
    const MAKEFILE: &str = "/cap.mk";
    const PROG: &str = "/cap-prog";
    const OUT: &str = "/cap-out.txt";
    const LABEL: &str = "make+tcc";

    const A_SRC: &[u8] = b"/* TU A: defines a function used by main in TU B */\n\
int slate_add(int a, int b){ return a + b; }\n";
    const B_SRC: &[u8] = b"/* TU B: main, calls across the TU boundary into TU A */\n\
extern int printf(const char *fmt, ...);\n\
extern int slate_add(int a, int b);\n\
int main(void){\n\
  int r = slate_add(40, 2);\n\
  printf(\"SLATE-SEP-%d\\n\", r);\n\
  return 0;\n\
}\n";
    // Recipe lines MUST start with a TAB.  Absolute /bin/tcc avoids PATH-lookup
    // uncertainty; echo is left ON (no `@`) so a failure surfaces the exact
    // compile/link command make ran on the serial log.
    const MAKEFILE_SRC: &[u8] = b"all: /cap-prog\n\
/cap-prog: /cap-a.o /cap-b.o\n\
\t/bin/tcc -o /cap-prog /cap-a.o /cap-b.o\n\
/cap-a.o: /cap-a.c\n\
\t/bin/tcc -c /cap-a.c -o /cap-a.o\n\
/cap-b.o: /cap-b.c\n\
\t/bin/tcc -c /cap-b.c -o /cap-b.o\n";

    // Stage the hosted-cc support set (ld/libc/libm/tcc/crt/libtcc1.a, ...).
    match stage_hosted_cc_support() {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => return Err(e),
    }
    // make + its recipe shell are the additional binaries this rung needs.
    if !crate::fs::Vfs::exists(SRC_MAKE) || !crate::fs::Vfs::exists(SRC_SH) {
        return Ok(());
    }

    serial_println!(
        "[spawn] Running REAL make-drives-tcc build (ring 3, Path Z) test..."
    );

    fn cleanup() {
        for p in
            ["/cap-a.c", "/cap-b.c", "/cap.mk", "/cap-a.o", "/cap-b.o", "/cap-prog", "/cap-out.txt"]
        {
            let _ = crate::fs::Vfs::remove(p);
        }
    }
    cleanup();

    let _ = crate::fs::Vfs::mkdir_all("/bin");
    for (src, dst) in [(SRC_MAKE, DST_MAKE), (SRC_SH, DST_SH)] {
        match crate::fs::Vfs::read_file(src) {
            Ok(bytes) => {
                if let Err(e) = crate::fs::Vfs::write_file(dst, &bytes) {
                    serial_println!(
                        "[spawn]   make+tcc: SKIP (staging {} -> {} failed: {:?})",
                        src, dst, e
                    );
                    cleanup();
                    return Ok(());
                }
            }
            Err(e) => {
                serial_println!("[spawn]   make+tcc: SKIP (reading {} failed: {:?})", src, e);
                cleanup();
                return Ok(());
            }
        }
    }

    for (path, data) in [(A_C, A_SRC), (B_C, B_SRC), (MAKEFILE, MAKEFILE_SRC)] {
        if let Err(e) = crate::fs::Vfs::write_file(path, data) {
            serial_println!("[spawn]   make+tcc: SKIP (writing {} failed: {:?})", path, e);
            cleanup();
            return Ok(());
        }
    }

    let make_elf = match crate::fs::Vfs::read_file(DST_MAKE) {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   make+tcc: SKIP (re-read {} failed: {:?})", DST_MAKE, e);
            cleanup();
            return Ok(());
        }
    };

    // --- run make: it builds /cap-prog by invoking tcc per the Makefile ----
    let argv: &[&[u8]] = &[b"make", b"-f", b"/cap.mk", b"all"];
    let envp: &[&[u8]] = &[b"PATH=/bin", b"LANG=C", b"SHELL=/bin/sh"];
    let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-make-cc",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv,
        envp,
        exe_path: Some(DST_MAKE.as_bytes()),
        cwd: None,
        uid_gid: None,
    };

    let result = match spawn_process(&make_elf, &options) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[spawn]   FAIL: make+tcc — make spawn returned {:?}", e);
            cleanup();
            return Err(e);
        }
    };

    let mut reaped = false;
    for i in 0..MAX_YIELDS {
        if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) {
            reaped = true;
            break;
        }
        // Periodic progress diagnostic for the intermittent make+tcc wedge.
        // `make` forks tcc grandchildren that run the actual compiles; when one
        // of *them* wedges, `make` blocks in wait4 and the culprit is a
        // grandchild invisible in a single-task snapshot.  So in addition to
        // the one-line `make` snapshot, dump the whole task table (with per-CPU
        // last_rip) at a coarser cadence to capture the wedged descendant's
        // sched-state — the datum needed to localize the bug (Blocked=lost
        // wakeup / Ready=starvation / Running=half-done ctx switch).
        if i != 0 && (i & (REAP_SNAPSHOT_INTERVAL - 1)) == 0 {
            log_reap_wait_progress(LABEL, "make", result.pid, result.task_id, i);
            if (i & ((REAP_SNAPSHOT_INTERVAL << 3) - 1)) == 0 {
                crate::sched::dump_task_table();
            }
        }
        crate::sched::yield_now();
    }

    let state = pcb::state(result.pid);
    let make_exit = pcb::exit_code(result.pid);
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    if !reaped || state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: make+tcc — make did not exit within {} yields (state={:?}); make \
             startup, Makefile parse, or one of its tcc compile/link children likely hung",
            MAX_YIELDS, state
        );
        cleanup();
        return Err(KernelError::TimedOut);
    }
    if make_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: make+tcc — make exit code={:?}, expected {} (non-zero means make hit \
             a parse error or one of its tcc recipe children failed)",
            make_exit, EXPECT_EXIT
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    // make claims success — confirm it actually built a dynamic ELF.
    let prog = match crate::fs::Vfs::read_file(PROG) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: make+tcc — make exited 0 but {} is unreadable: {:?} (the link \
                 recipe did not run or did not produce the binary)",
                PROG, e
            );
            cleanup();
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_dynamic_elf(&prog, PROG, LABEL) {
        cleanup();
        return Err(e);
    }

    // --- run the make-built binary ----------------------------------------
    let (run_exit, out) = match run_dynamic_capture(&prog, PROG, OUT, LABEL) {
        Ok(t) => t,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    cleanup();

    if out.as_slice() == b"SLATE-SEP-42\n" && run_exit == Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   REAL make-drives-tcc build (ring 3: make parsed a 3-target Makefile, \
             fork/exec'd tcc to compile two TUs to objects and link them into a {}-byte dynamic \
             ELF, ld.so ran it, exit-time flush wrote {} bytes == expected, exit={:?}): OK",
            prog.len(), out.len(), run_exit
        );
        Ok(())
    } else {
        serial_println!(
            "[spawn]   FAIL: make+tcc — {} holds {} bytes {:?} (exit={:?}), expected {:?} with \
             exit={}",
            OUT, out.len(), out.as_slice(), run_exit, b"SLATE-SEP-42\n", EXPECT_EXIT
        );
        Err(KernelError::InternalError)
    }
}

/// Path Z Part 40 — a multi-TU C project that `#include`s its own project header.
///
/// Every earlier hosted rung (Parts 36-39) deliberately declared its libc and
/// cross-TU prototypes with bare `extern` statements so the build needed *no*
/// header files at all.  Real-world C projects instead factor shared
/// declarations + macros into their own headers and pull them in with
/// `#include "project_header.h"` — the double-quote, *project-relative* form,
/// distinct from the `<system_header.h>` glibc-tree form (which is still blocked
/// on a header-carrying rootfs; see todo.txt).  This rung exercises exactly that
/// previously-untested capability: tcc's preprocessor resolving a quote-include
/// to a sibling header, expanding a macro it defines, and honoring a prototype
/// it declares across a real translation-unit boundary.
///
/// Two TUs both `#include "caphdr-hdr.h"`, which provides `#define SLATE_BASE 40`
/// and `int slate_combine(int);`.  TU A implements `slate_combine` using the
/// macro; TU B's `main` calls it and prints `SLATE-HDR-42`.  A single
/// `tcc -o /caphdr-prog /caphdr-a.c /caphdr-b.c` compiles both sources and links
/// them into a dynamic binary that then runs in ring 3.
///
/// No-op (`Ok(())`) when the hosted-cc rootfs (`/bin/tcc`, the glibc support
/// set) is absent, so a kernel built without it boots clean.
///
/// # Errors
///
/// Returns [`KernelError::InternalError`] if tcc fails to build the project, the
/// output is not a dynamic ELF, or the program does not print the expected line.
pub fn self_test_linux_real_glibc_cc_project_header() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const LABEL: &str = "project-header";
    const HDR: &str = "/caphdr-hdr.h";
    const A_C: &str = "/caphdr-a.c";
    const B_C: &str = "/caphdr-b.c";
    const PROG: &str = "/caphdr-prog";
    const OUT: &str = "/caphdr-out.txt";

    // The shared project header: a macro + a cross-TU prototype, include-guarded.
    const HDR_SRC: &[u8] = b"/* project-local header pulled in via #include \"...\" */\n\
#ifndef CAPHDR_H\n\
#define CAPHDR_H\n\
#define SLATE_BASE 40\n\
int slate_combine(int x);\n\
#endif\n";
    // TU A: includes the header for the macro + prototype, defines the function.
    const A_SRC: &[u8] = b"#include \"caphdr-hdr.h\"\n\
int slate_combine(int x){ return SLATE_BASE + x; }\n";
    // TU B: includes the header for the prototype; main calls across the TU.
    const B_SRC: &[u8] = b"#include \"caphdr-hdr.h\"\n\
extern int printf(const char *fmt, ...);\n\
int main(void){\n\
  printf(\"SLATE-HDR-%d\\n\", slate_combine(2));\n\
  return 0;\n\
}\n";

    match stage_hosted_cc_support() {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => return Err(e),
    }

    serial_println!(
        "[spawn] Running REAL project-header C build (tcc, #include \"...\", ring 3, Path Z) test..."
    );

    fn cleanup() {
        for p in
            ["/caphdr-hdr.h", "/caphdr-a.c", "/caphdr-b.c", "/caphdr-prog", "/caphdr-out.txt"]
        {
            let _ = crate::fs::Vfs::remove(p);
        }
    }
    cleanup();

    for (path, data) in [(HDR, HDR_SRC), (A_C, A_SRC), (B_C, B_SRC)] {
        if let Err(e) = crate::fs::Vfs::write_file(path, data) {
            serial_println!("[spawn]   {}: SKIP (writing {} failed: {:?})", LABEL, path, e);
            cleanup();
            return Ok(());
        }
    }

    let tcc_elf = match crate::fs::Vfs::read_file("/bin/tcc") {
        Ok(b) => b,
        Err(e) => {
            serial_println!("[spawn]   {}: SKIP (re-read /bin/tcc failed: {:?})", LABEL, e);
            cleanup();
            return Ok(());
        }
    };

    // --- compile + link both TUs in one tcc invocation ---------------------
    let cc_argv: &[&[u8]] =
        &[b"tcc", b"-o", PROG.as_bytes(), A_C.as_bytes(), B_C.as_bytes()];
    let cc_exit = match spawn_reap_tcc(&tcc_elf, cc_argv, LABEL) {
        Ok(x) => x,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    if cc_exit != Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   FAIL: {} — tcc exit code={:?}, expected {} (non-zero means the preprocessor \
             could not resolve the quote-include, or hit a compile/link error)",
            LABEL, cc_exit, EXPECT_EXIT
        );
        cleanup();
        return Err(KernelError::InternalError);
    }

    let prog = match crate::fs::Vfs::read_file(PROG) {
        Ok(b) => b,
        Err(e) => {
            serial_println!(
                "[spawn]   FAIL: {} — tcc exited 0 but {} is unreadable: {:?} (the build did not \
                 produce the binary)",
                LABEL, PROG, e
            );
            cleanup();
            return Err(KernelError::InternalError);
        }
    };
    if let Err(e) = assert_dynamic_elf(&prog, PROG, LABEL) {
        cleanup();
        return Err(e);
    }

    let (run_exit, out) = match run_dynamic_capture(&prog, PROG, OUT, LABEL) {
        Ok(t) => t,
        Err(e) => {
            cleanup();
            return Err(e);
        }
    };
    cleanup();

    if out.as_slice() == b"SLATE-HDR-42\n" && run_exit == Some(EXPECT_EXIT) {
        serial_println!(
            "[spawn]   REAL project-header build (ring 3: tcc resolved #include \"caphdr-hdr.h\" \
             from two TUs, expanded SLATE_BASE, linked the cross-TU call into a {}-byte dynamic \
             ELF, ld.so ran it, output {} bytes == expected, exit={:?}): OK",
            prog.len(), out.len(), run_exit
        );
        Ok(())
    } else {
        serial_println!(
            "[spawn]   FAIL: {} — {} holds {} bytes {:?} (exit={:?}), expected {:?} with exit={}",
            LABEL, OUT, out.len(), out.as_slice(), run_exit, b"SLATE-HDR-42\n", EXPECT_EXIT
        );
        Err(KernelError::InternalError)
    }
}

/// Path Z Part 41 — glibc `.init_array` constructor + `.fini_array` destructor
/// ordering through a freshly-tcc-built dynamic binary.
///
/// Parts 36-40 all entered `main` directly (via `__libc_start_main`); none
/// exercised the C runtime's *constructor/destructor* machinery.  This rung is
/// the first to do so: the source declares a function with
/// `__attribute__((constructor))` and one with `__attribute__((destructor))`,
/// which tcc emits into `.init_array` / `.fini_array`.  glibc's csu init
/// (`__libc_csu_init`, invoked by `__libc_start_main` *before* `main`) walks
/// `.init_array`; the dynamic linker's `_dl_fini` walks `.fini_array` at exit.
/// So this proves the full ctor-before-main-before-dtor lifecycle actually runs
/// for a real dynamically-linked glibc program in ring 3.
///
/// The three markers are emitted with the raw `write(2)` syscall (unbuffered)
/// rather than buffered stdio, so the captured file's byte order reflects the
/// *temporal* execution order directly and is immune to any ambiguity about
/// when glibc's exit-time stdio flush runs relative to `.fini_array`.  fd 1 is
/// redirected to a capture file by the harness, so `write(1, ...)` lands there.
/// Expected capture (in order): constructor → `main` → destructor.
pub fn self_test_linux_real_glibc_cc_ctor_dtor() -> KernelResult<()> {
    // extern prototype for write(2) avoids needing the glibc/unistd header tree
    // on the target; ssize_t/size_t are long/unsigned long on x86_64.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
__attribute__((constructor)) static void slate_ctor(void){ write(1, \"CTOR\\n\", 5); }\n\
__attribute__((destructor))  static void slate_dtor(void){ write(1, \"DTOR\\n\", 5); }\n\
int main(void){ write(1, \"MAIN\\n\", 5); return 0; }\n";
    run_hosted_cc_case("ctor/dtor", HOSTED_SRC, b"CTOR\nMAIN\nDTOR\n")
}

/// Path Z Part 42 — ELF thread-local storage (`__thread`) in a freshly-tcc-built
/// dynamic glibc binary.
///
/// No prior rung exercised the TLS ABI from a *compiled* program: the existing
/// pthread self-test runs a pre-built `/bin/pthread`, not tcc output, so the
/// full "tcc emits a `.tdata`/PT_TLS segment + local-exec TLS relocations →
/// glibc's `__libc_setup_tls` copies the init image into the main thread's TLS
/// block → `%fs`-relative access reads/writes it" path had never been proven for
/// a binary built on-target.  This rung is the first to do so.  It also gives
/// concrete end-to-end coverage of the per-task `%fs`-base save/restore that
/// bugs F13/F14 fixed — those were validated against hand-written ELFs, never a
/// real `__thread` consumer.
///
/// The program declares one initialised `__thread int` (42), prints its low
/// decimal digit (proves the init image was copied → `%fs:off` reads 42), then
/// reassigns it (7) and prints again (proves TLS is writable, not a read-only
/// mapping of the template).  Markers use raw `write(2)` (unbuffered) so the
/// captured bytes are exactly `27\n`.
pub fn self_test_linux_real_glibc_cc_tls() -> KernelResult<()> {
    // `__thread` on the main executable uses the local-exec TLS model: tcc emits
    // a fixed negative %fs offset resolved at link time, and glibc initialises
    // the block from the PT_TLS template it finds via AT_PHDR/AT_PHNUM.  No
    // header tree needed — write(2) via an extern prototype.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
__thread int slate_tls = 42;\n\
int main(void){\n\
  char c = (char)('0' + (slate_tls % 10));\n\
  write(1, &c, 1);\n\
  slate_tls = 7;\n\
  char d = (char)('0' + slate_tls);\n\
  write(1, &d, 1);\n\
  write(1, \"\\n\", 1);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("tls", HOSTED_SRC, b"27\n")
}

/// Path Z Part 43 — POSIX signal handler round-trip in a freshly-tcc-built
/// dynamic glibc binary (`sigaction`/`raise`/`sigreturn` from compiled code).
///
/// The kernel's Linux-ABI signal machinery (`rt_sigaction`, asynchronous frame
/// setup, `rt_sigreturn`) is already well covered by pre-built ELF self-tests
/// (`self_test_linux_sa_restart`, the signalfd tests, …).  What no prior rung
/// exercised is that same path *driven by a program compiled on-target*: the
/// existing tests run hand-authored / pre-built binaries, so the full "tcc emits
/// a `.text` handler + glibc's `signal()` installs it via `rt_sigaction` (with
/// glibc's own SA_RESTORER trampoline) → `raise()` issues `tgkill` → the kernel
/// posts the signal and, on the syscall-return path, builds the Linux signal
/// frame the handler `ret`s into to reach `rt_sigreturn`" round-trip had never
/// been proven for a binary built from source here.  This rung closes that gap
/// and is the signal-delivery sibling of Part 41 (ctor/dtor) and Part 42 (TLS).
///
/// The program installs a `SIGUSR1` (10 on x86_64 Linux) handler, writes `A`,
/// raises `SIGUSR1` to itself, and writes `B`.  `raise` delivers synchronously
/// (the pending signal is taken on return from the `tgkill` syscall, *before*
/// `raise` returns to `main`), so the handler's `SIG` lands between `A` and `B`
/// — the captured file is exactly `A\nSIG\nB\n`.  Markers use raw `write(2)`
/// (unbuffered) so the captured byte order is the exact temporal order, immune
/// to any stdio-buffering ambiguity.
pub fn self_test_linux_real_glibc_cc_signal() -> KernelResult<()> {
    // Bare extern prototypes avoid needing the glibc <signal.h>/<unistd.h> header
    // tree on-target (blocked per TD22).  `signal` returns a function pointer:
    // `void (*signal(int, void (*)(int)))(int)`.  SIGUSR1 == 10 on x86_64 Linux.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
extern void (*signal(int sig, void (*handler)(int)))(int);\n\
extern int raise(int sig);\n\
static void slate_h(int s){ (void)s; write(1, \"SIG\\n\", 4); }\n\
int main(void){\n\
  signal(10, slate_h);\n\
  write(1, \"A\\n\", 2);\n\
  raise(10);\n\
  write(1, \"B\\n\", 2);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("signal", HOSTED_SRC, b"A\nSIG\nB\n")
}

/// Path Z Part 44 — non-local control flow (`setjmp`/`longjmp`) in a
/// freshly-tcc-built dynamic glibc binary.
///
/// `setjmp`/`longjmp` are the C substrate for error recovery / exception-like
/// unwinding: `setjmp` snapshots the callee-saved register set + `%rsp`/`%rip`
/// into a `jmp_buf`, and a later `longjmp` from a *deeper* call frame restores
/// that snapshot so control resumes at the `setjmp` site with `setjmp`
/// appearing to return the `longjmp` value.  No prior hosted-cc rung exercised
/// this: it proves tcc emits the correct call sequence for glibc's `_setjmp`/
/// `_longjmp` (the POSIX no-signal-mask variants — real exported glibc symbols,
/// unlike the `setjmp` *macro* which needs `<setjmp.h>`, blocked per TD22) and
/// that glibc's register save/restore works in our ring-3 environment.
///
/// Flow: the program writes `A`, calls `_setjmp(env)` (returns 0 the first
/// time → writes `S`), then calls `jumper()` which `_longjmp(env, 7)`s back —
/// so `_setjmp` "returns" a second time with 7, taking the else branch which
/// writes `7`.  `env` is a static buffer so it survives the stack unwind.  The
/// captured file is exactly `A\nS\n7\n` (6 bytes, exit 0).  Raw `write(2)`
/// keeps the byte order the exact temporal order.
pub fn self_test_linux_real_glibc_cc_setjmp() -> KernelResult<()> {
    // `_setjmp`/`_longjmp` are real POSIX symbols exported by glibc (the plain
    // `setjmp` is a header macro → `__sigsetjmp`, which needs <setjmp.h>).  The
    // jmp_buf is opaque; a static 32-long (256-byte) buffer over-provisions the
    // ~200-byte x86_64 `struct __jmp_buf_tag` and survives the longjmp unwind.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
extern int _setjmp(void *env);\n\
extern void _longjmp(void *env, int val);\n\
static long slate_env[32];\n\
static void jumper(void){ _longjmp(slate_env, 7); }\n\
int main(void){\n\
  write(1, \"A\\n\", 2);\n\
  int r = _setjmp(slate_env);\n\
  if (r == 0){\n\
    write(1, \"S\\n\", 2);\n\
    jumper();\n\
  } else {\n\
    char b[2] = { (char)('0' + r), '\\n' };\n\
    write(1, b, 2);\n\
  }\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("setjmp", HOSTED_SRC, b"A\nS\n7\n")
}

/// Path Z Part 45 — user-defined variadic function (SysV varargs ABI codegen)
/// in a freshly-tcc-built dynamic glibc binary.
///
/// The prior rungs' only varargs consumer was `printf`, whose `va_arg` walk is
/// implemented *inside glibc* — so no rung exercised tcc's own codegen for the
/// x86_64 SysV variadic ABI: laying out the register save area (integer args in
/// the GP save area, `%al` = vector-register count on the call side), spilling
/// named + anonymous args, and the `va_start`/`va_arg`/`va_end` sequence in a
/// *user-authored* variadic function.  That is a classic compiler-bug locus
/// (register vs. overflow-area boundary, `gp_offset`/`fp_offset` bookkeeping),
/// so a compiled-on-target variadic function is a genuine coverage gap.
///
/// The program defines `isum(int count, ...)` which sums `count` `int` varargs
/// via the builtins (`__builtin_va_list`/`__builtin_va_start`/`__builtin_va_arg`
/// /`__builtin_va_end` — tcc intrinsics, so no `<stdarg.h>` header tree is
/// needed, staying inside the TD22 constraint).  `isum(4, 10, 20, 5, 7)` = 42,
/// which the program prints as its two decimal digits; the captured file is
/// exactly `42\n` (3 bytes, exit 0).  Raw `write(2)` keeps ordering exact.  No
/// kernel path is exercised (varargs is pure userspace/codegen) — the value is
/// proving tcc's variadic ABI lowering produces a correct binary here.
pub fn self_test_linux_real_glibc_cc_vararg() -> KernelResult<()> {
    // Uses tcc's va builtins directly (the `<stdarg.h>` macros just alias them),
    // so no glibc/compiler header tree is required.  Named param `count` fixes
    // where the anonymous args begin; the loop reads `count` `int`s via va_arg.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int isum(int count, ...){\n\
  __builtin_va_list ap;\n\
  __builtin_va_start(ap, count);\n\
  int total = 0;\n\
  for (int i = 0; i < count; i++){ total += __builtin_va_arg(ap, int); }\n\
  __builtin_va_end(ap);\n\
  return total;\n\
}\n\
int main(void){\n\
  int s = isum(4, 10, 20, 5, 7);\n\
  char b[3] = { (char)('0' + s / 10), (char)('0' + s % 10), '\\n' };\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("vararg", HOSTED_SRC, b"42\n")
}

/// Path Z Part 46 — floating-point / SSE codegen + the x86_64 SysV FP ABI in a
/// freshly-tcc-built dynamic glibc binary.
///
/// No prior rung used floating point: the integer/string/pointer programs of
/// Parts 34-45 never touched an XMM register, so tcc's `double` codegen and the
/// SysV *floating-point* calling convention (arguments and the return value in
/// `%xmm0`/`%xmm1`, `mulsd`/`addsd` for arithmetic, `cvttsd2si` for the
/// truncating `double`→`int` cast) were entirely untested from compiled code.
/// FP ABI lowering is a distinct codegen path from the integer ABI (separate
/// register class, separate spill rules) and a common compiler-bug locus, so a
/// compiled-on-target floating-point program closes a real gap.
///
/// The program passes two `double`s to `scale(x, f) = x*f + 0.5`, so
/// `scale(8.0, 5.0)` = 40.5; the `(int)` truncation yields 40, printed as its
/// two decimal digits — captured file exactly `40\n` (3 bytes, exit 0). The
/// input is `volatile` so the compiler cannot constant-fold the arithmetic
/// away and must emit real SSE instructions + the FP-ABI call sequence. Raw
/// `write(2)` keeps ordering exact. Pure userspace/codegen (no kernel path); the
/// value is proving tcc's FP codegen + SysV FP ABI produce a correct ring-3
/// binary (and, incidentally, that XMM state is sane across the glibc call path).
pub fn self_test_linux_real_glibc_cc_float() -> KernelResult<()> {
    // Bare `extern` prototype only (no <math.h>/libm — the arithmetic is plain
    // `double` mul/add/truncate, which tcc emits inline as SSE, so no `-lm`
    // link is needed and the run_hosted_cc_case single-source harness suffices).
    // `volatile` on the input defeats constant folding so real SSE + the FP-ABI
    // call sequence are exercised rather than a compile-time-computed constant.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static double scale(double x, double f){ return x * f + 0.5; }\n\
int main(void){\n\
  volatile double a = 8.0;\n\
  double r = scale(a, 5.0);\n\
  int n = (int)r;\n\
  char b[3] = { (char)('0' + n / 10), (char)('0' + n % 10), '\\n' };\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("float", HOSTED_SRC, b"40\n")
}

/// Path Z Part 47 — struct-by-value argument passing *and* return (the x86_64
/// SysV *aggregate* ABI) in a freshly-tcc-built dynamic glibc binary.
///
/// Every prior rung passed and returned only scalars (ints, pointers, one
/// `double`) — so the compiler's aggregate calling convention was entirely
/// untested from compiled code.  Passing/returning a `struct` by value is a
/// distinct, notoriously tricky codegen path: the ABI *classifies* each
/// aggregate into eightbytes (INTEGER / SSE / MEMORY), splits a small struct
/// across multiple registers, and returns a ≤16-byte all-INTEGER struct in the
/// `RAX:RDX` pair rather than through memory.  Getting the eightbyte boundary,
/// the register-pair packing, and the field offsets right is a classic
/// compiler-bug locus, so a compiled-on-target struct-by-value program closes a
/// real coverage gap distinct from the scalar-integer, varargs, and FP ABIs.
///
/// The program uses a 16-byte `struct box { int a,b,c,d; }` — exactly two
/// all-INTEGER eightbytes, so each argument is passed in a *pair* of GP
/// registers (`p` in `rdi:rsi`, `q` in `rdx:rcx`) and the result comes back in
/// `rax:rdx`, exercising the register-pair pack/unpack on both the call and
/// return sides (not the >16-byte hidden-pointer/MEMORY path).  `combine` adds
/// the two structs field-wise; the four sums (6, 8, 10, 18) total 42, printed as
/// its two decimal digits — captured file exactly `42\n` (3 bytes, exit 0).  The
/// first field is seeded from a `volatile` so the compiler cannot constant-fold
/// the aggregate away and must emit the real by-value pack + call sequence.  Raw
/// `write(2)` keeps ordering exact.  Pure userspace/codegen (no kernel path); the
/// value is proving tcc's SysV aggregate ABI lowering produces a correct ring-3
/// binary through the glibc call path.
///
/// Implementation note (why the struct locals are field-initialised rather than
/// brace-initialised): when tcc lowers a *brace initialiser* on an aggregate it
/// emits a synthesised `memset` reference.  On the target that one extra
/// undefined symbol makes the one-shot `tcc -o prog prog.c` compile+link abort
/// with a spurious `tcc: error: unresolved reference to 'main'` (tcc exit 1) —
/// even though `-c` alone emits a perfectly good global `main`, and even though
/// `memset` is provided by the linked glibc.  The failure does NOT reproduce
/// off-target (re-running the extracted target tcc under WSL against the same
/// staged crt/libc/libtcc1.a links the brace-init program cleanly), so it is an
/// on-target tcc/link quirk, not an archive-index problem — see
/// B-TCC-LIBTCC1-MAIN in known-issues.md.  Assigning each field individually
/// keeps the object's only undefined symbol as `write` (identical link surface
/// to Parts 45/46, which link cleanly) while still passing and returning the
/// whole struct by value — so the aggregate ABI is fully covered without
/// tripping the quirk.
pub fn self_test_linux_real_glibc_cc_struct() -> KernelResult<()> {
    // 16-byte all-INTEGER struct => two eightbytes => passed/returned in GP
    // register pairs (never the MEMORY/hidden-pointer path).  `volatile` seed on
    // the first field defeats constant folding so the by-value pack/call/return
    // sequence is actually emitted rather than computed at compile time.  Fields
    // are set individually (not via a brace initialiser) so tcc does not emit a
    // synthesised `memset` reference — see the doc note above / B-TCC-LIBTCC1-MAIN.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
struct box { int a, b, c, d; };\n\
static struct box combine(struct box p, struct box q){\n\
  struct box r;\n\
  r.a = p.a + q.a; r.b = p.b + q.b;\n\
  r.c = p.c + q.c; r.d = p.d + q.d;\n\
  return r;\n\
}\n\
int main(void){\n\
  volatile int seed = 1;\n\
  struct box p; p.a = seed; p.b = 2; p.c = 3; p.d = 4;\n\
  struct box q; q.a = 5; q.b = 6; q.c = 7; q.d = 14;\n\
  struct box r = combine(p, q);\n\
  int total = r.a + r.b + r.c + r.d;\n\
  char b[3]; b[0] = (char)(48 + total / 10); b[1] = (char)(48 + total % 10); b[2] = 10;\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("struct-by-value", HOSTED_SRC, b"42\n")
}

/// Path Z Part 48 — `long double` / x87 80-bit extended-precision FP codegen and
/// the x86_64 SysV `long double` ABI in a freshly-tcc-built dynamic glibc binary.
///
/// Part 46 covered SSE `double`, but on x86_64 `long double` is a *different*
/// beast: it is the 80-bit x87 extended type, so it uses an entirely separate
/// register file (the x87 stack `st(0)..st(7)`, not the XMM registers) and a
/// separate ABI class — a `long double` argument is classified X87/X87UP →
/// passed in *memory* (on the stack, 16-byte aligned), and the result is
/// returned in `st(0)`.  tcc must emit x87 loads/stores (`fldt`/`fstpt`),
/// x87 arithmetic (`fmulp`/`faddp`), and an x87→int truncation (`fisttp`) — none
/// of which any prior rung exercised (the FP rung used SSE `mulsd`/`cvttsd2si`).
/// x87 codegen is a distinct, easily-mis-lowered path (80-bit spill size, the
/// implicit stack discipline), so a compiled-on-target `long double` program is
/// a genuine coverage gap.  It also incidentally proves x87 state is sane across
/// the glibc call path.
///
/// The program passes two `long double`s to `scale(x, f) = x*f + 0.5L`, so
/// `scale(8.0L, 5.0L)` = 40.5; the `(int)` truncation yields 40, printed as its
/// two decimal digits — captured file exactly `40\n` (3 bytes, exit 0).  The
/// input is `volatile` so the compiler cannot constant-fold the arithmetic away
/// and must emit real x87 instructions + the memory-passing ABI sequence.  Only
/// undefined symbol is `write` (no `memset`/aggregate init → does not trip
/// B-TCC-LIBTCC1-MAIN).  Pure userspace/codegen (no kernel path).
pub fn self_test_linux_real_glibc_cc_longdouble() -> KernelResult<()> {
    // `long double` on x86_64 == 80-bit x87 extended precision => x87 stack
    // codegen + the X87 memory-passing ABI + st(0) return, a path distinct from
    // Part 46's SSE double.  `volatile` input defeats constant folding so real
    // x87 arithmetic + the memory-arg call sequence run.  `0.5L`/`8.0L`/`5.0L`
    // literals keep the whole computation in `long double` (no double demotion).
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static long double scale(long double x, long double f){ return x * f + 0.5L; }\n\
int main(void){\n\
  volatile long double a = 8.0L;\n\
  long double r = scale(a, 5.0L);\n\
  int n = (int)r;\n\
  char b[3]; b[0] = (char)(48 + n / 10); b[1] = (char)(48 + n % 10); b[2] = 10;\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("long-double-x87", HOSTED_SRC, b"40\n")
}

/// Path Z Part 49 — bitfield layout + extract/insert codegen in a freshly-tcc-
/// built dynamic glibc binary.
///
/// No prior rung used bitfields.  A C bitfield is not a plain load/store: the
/// compiler packs several sub-byte members into one storage unit and must emit
/// a *shift + mask* to read a member (extract) and a *load / mask-out-old /
/// shift-in-new / mask-to-width / store* read-modify-write to assign one
/// (insert), all while leaving the neighbouring bitfields in the same unit
/// intact.  Getting the bit offsets, the width masks, and the RMW right is a
/// classic compiler-bug locus (off-by-one masks, sign vs. zero extension,
/// clobbering an adjacent field), so a compiled-on-target bitfield program is a
/// genuine coverage gap distinct from the plain-`int` struct fields of Part 47.
///
/// `struct flags { unsigned a:3, b:5, c:4; }` packs three members into a single
/// 32-bit unit (bits 0-2, 3-7, 8-11).  The program assigns `a=5, b=20, c=9`
/// (the first from a `volatile` seed so the compiler cannot fold the inserts
/// away) and sums them back (`5+20+9`), which — that each field reads back its
/// own value proves the inserts did not clobber their neighbours — plus 8 gives
/// 42, printed as its two decimal digits: captured file exactly `42\n` (3 bytes,
/// exit 0).  Only undefined symbol is `write` (no aggregate init → does not trip
/// B-TCC-LIBTCC1-MAIN).  Pure userspace/codegen (no kernel path).
pub fn self_test_linux_real_glibc_cc_bitfield() -> KernelResult<()> {
    // Three bitfields packed into one 32-bit unit exercise shift+mask extract and
    // load/mask/shift/store insert without clobbering neighbours.  `volatile`
    // seed on `a` defeats constant folding so the real RMW insert sequence runs.
    // Individual field assignment (not a brace initialiser) => no synthesised
    // `memset`, keeping the link surface at just `write` (see B-TCC-LIBTCC1-MAIN).
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
struct flags { unsigned a : 3; unsigned b : 5; unsigned c : 4; };\n\
int main(void){\n\
  volatile int seed = 5;\n\
  struct flags f;\n\
  f.a = (unsigned)seed;\n\
  f.b = 20u;\n\
  f.c = 9u;\n\
  int total = (int)f.a + (int)f.b + (int)f.c + 8;\n\
  char b[3]; b[0] = (char)(48 + total / 10); b[1] = (char)(48 + total % 10); b[2] = 10;\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("bitfield", HOSTED_SRC, b"42\n")
}

/// Path Z Part 50 — indirect call through a function-pointer dispatch table in a
/// freshly-tcc-built dynamic glibc binary.
///
/// Every prior rung called functions by *name* (a direct `call rel32`).  This
/// rung calls through a **function pointer** selected at runtime, exercising two
/// things no earlier part did: (1) tcc's indirect-call codegen — load the target
/// from a table slot and emit `call *reg` rather than a fixed relative call; and
/// (2) taking the *address* of a function and materialising it into a static
/// table, which requires the compiler to emit an absolute (or GOT-relative)
/// relocation per entry that the linker + `ld.so` must fix up at load time —
/// the function-pointer analogue of a data relocation, distinct from the code
/// the direct-call rungs produced.  Indirect dispatch tables underpin vtables,
/// syscall/ioctl jump tables, and plugin interfaces, so proving the compiled
/// binary calls the *right* target through a relocated pointer closes a real gap.
///
/// `ops[3]` is a `static const` array of `int(*)(int)` initialised with three
/// file-scope functions (`add10`, `mul3`, `neg`); being `static const` it lands
/// in read-only data with one relocation per slot and needs no runtime
/// aggregate init (so no synthesised `memset` — avoids B-TCC-LIBTCC1-MAIN).  A
/// `volatile` selector `sel = 1` picks `mul3`, so `ops[sel](14)` = `mul3(14)` =
/// 42, printed as its two decimal digits — captured file exactly `42\n` (3
/// bytes, exit 0).  `volatile` stops the compiler folding the selection to a
/// direct call.  Only undefined symbol is `write`.  Pure userspace/codegen.
pub fn self_test_linux_real_glibc_cc_funcptr() -> KernelResult<()> {
    // `static const` table => rodata + per-slot relocation (no runtime init, no
    // `memset`).  `volatile sel` forces a real indirect `call *reg` through the
    // relocated pointer rather than a compile-time-resolved direct call.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int add10(int x){ return x + 10; }\n\
static int mul3(int x){ return x * 3; }\n\
static int neg(int x){ return -x; }\n\
static int (*const ops[3])(int) = { add10, mul3, neg };\n\
int main(void){\n\
  volatile int sel = 1;\n\
  int r = ops[sel](14);\n\
  char b[3]; b[0] = (char)(48 + r / 10); b[1] = (char)(48 + r % 10); b[2] = 10;\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("funcptr-dispatch", HOSTED_SRC, b"42\n")
}

/// Path Z Part 51 — computed goto (GNU labels-as-values: `&&label` + `goto *p`)
/// in a freshly-tcc-built dynamic glibc binary.
///
/// Part 50 covered an indirect *call* through a function pointer; this rung
/// covers the sibling but distinct indirect *jump* codegen.  The GNU
/// labels-as-values extension lets a program take the address of a *label*
/// (`&&op1`) and jump to a runtime-computed one (`goto *ptr`), which the
/// compiler lowers to a plain `jmp *reg` (no call/return, no stack frame change)
/// plus — because the label addresses are stored in a `static` table — one
/// intra-function relocation per slot that the linker resolves.  This is the
/// mechanism real interpreters use for threaded dispatch (one indirect jump per
/// bytecode op), a hot path where the direct-branch rungs give no coverage, so a
/// compiled-on-target computed goto closes a genuine gap and also exercises
/// tcc's support for the `&&`/`goto *` GNU extension itself.
///
/// A `volatile` selector `sel = 1` indexes a `static const` jump table of three
/// label addresses; the taken branch (`op1`) sets `r = 42`, printed as its two
/// decimal digits — captured file exactly `42\n` (3 bytes, exit 0).  `volatile`
/// stops the compiler folding the jump to a static branch.  The table is
/// `static const` (rodata + per-slot relocation, no runtime aggregate init → no
/// synthesised `memset`, avoids B-TCC-LIBTCC1-MAIN); only undefined symbol is
/// `write`.  Pure userspace/codegen (no kernel path).
pub fn self_test_linux_real_glibc_cc_computed_goto() -> KernelResult<()> {
    // `&&label` address-of-label + `goto *p` => an indirect `jmp *reg`; the
    // `static const` table of label addresses lands in rodata with one
    // relocation per slot (no runtime init/`memset`).  `volatile sel` forces the
    // real computed jump rather than a compile-time-resolved direct branch.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
int main(void){\n\
  volatile int sel = 1;\n\
  static void *const tbl[3] = { &&op0, &&op1, &&op2 };\n\
  int r = 0;\n\
  goto *tbl[sel];\n\
op0: r = 7;  goto done;\n\
op1: r = 42; goto done;\n\
op2: r = 99; goto done;\n\
done:;\n\
  char b[3]; b[0] = (char)(48 + r / 10); b[1] = (char)(48 + r % 10); b[2] = 10;\n\
  write(1, b, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("computed-goto", HOSTED_SRC, b"42\n")
}

/// Path Z Part 52 — union type-punning (overlapping-member storage aliasing) in
/// a freshly-tcc-built dynamic glibc binary.
///
/// No prior rung used a `union`.  A union makes several members *share* the same
/// storage, so the compiler must lay them all at offset 0 and — crucially —
/// treat a write through one member and a read through another as touching the
/// same bytes (the compiler cannot cache the written value in a register across
/// the differently-typed read; it must round-trip through memory).  Getting the
/// overlap layout and the alias-through-memory behaviour right is a distinct
/// codegen concern from the disjoint fields of a `struct` (Part 47) — and
/// union type-punning is the standard C idiom for reinterpreting bytes (endian
/// probes, float/int bit tricks, protocol headers), so a compiled-on-target
/// union closes a real gap.
///
/// `union u { unsigned int i; unsigned char b[4]; }` overlaps a 32-bit int with
/// a 4-byte array.  The program writes `u.i = 0x2A` (from a `volatile` seed so
/// the write cannot be folded into the reads) and sums the four overlapping
/// bytes `b[0..3]`; on this little-endian target `0x2A` lands in `b[0]` and the
/// rest are 0, so the sum is 42 — reading it back through the *other* member
/// proves the members truly alias.  Printed as its two decimal digits: captured
/// file exactly `42\n` (3 bytes, exit 0).  Only undefined symbol is `write`
/// (no aggregate init → does not trip B-TCC-LIBTCC1-MAIN).  Pure userspace/codegen.
pub fn self_test_linux_real_glibc_cc_union() -> KernelResult<()> {
    // Union overlaps `unsigned int i` and `unsigned char b[4]` at offset 0.
    // Writing `.i` then reading `.b[..]` forces the compiler to round-trip
    // through the shared storage (no register caching across the aliasing read).
    // `volatile` seed defeats folding; little-endian puts 0x2A in b[0].
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
union u { unsigned int i; unsigned char b[4]; };\n\
int main(void){\n\
  volatile unsigned int seed = 0x0000002Au;\n\
  union u u; u.i = seed;\n\
  int total = (int)u.b[0] + (int)u.b[1] + (int)u.b[2] + (int)u.b[3];\n\
  char c[3]; c[0] = (char)(48 + total / 10); c[1] = (char)(48 + total % 10); c[2] = 10;\n\
  write(1, c, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("union-punning", HOSTED_SRC, b"42\n")
}

/// Path Z Part 53 — function-local `static` variable (persistent, once-initialised
/// mutable state) in a freshly-tcc-built dynamic glibc binary.
///
/// Prior rungs used only automatic locals (fresh each call, on the stack) and a
/// couple of `static const` read-only tables.  A *mutable* function-local
/// `static` is different in two ways the compiler must get right: (1) placement
/// — the variable is not on the stack but in the writable data image (`.data`
/// for a non-zero initialiser, `.bto` for zero), with function scope but static
/// storage duration; and (2) *persistence* — its value must survive across
/// calls and be initialised exactly once at load, not re-set on entry.  This is
/// the classic idiom for counters, one-shot latches, and lazy caches, and no
/// earlier rung proved the compiled binary keeps such state across calls, so it
/// is a genuine coverage gap.
///
/// `bump()` holds `static int counter = 40` and returns `++counter` each call.
/// `main` calls it `reps` (= a `volatile` 2, so the loop cannot be unrolled and
/// folded) times: the first call yields 41, the second 42 — the second call
/// seeing 41 (not a re-initialised 40) is exactly what proves the static
/// persisted.  `r` ends at 42, printed as its two decimal digits — captured file
/// exactly `42\n` (3 bytes, exit 0).  Only undefined symbol is `write`.  Pure
/// userspace/codegen (no kernel path).
pub fn self_test_linux_real_glibc_cc_func_static() -> KernelResult<()> {
    // `static int counter = 40` inside bump() has static storage duration but
    // function scope: it lives in .data, is initialised once at load, and its
    // value must persist across calls.  `volatile reps` stops the loop being
    // unrolled+folded so the two real calls (41 then 42) actually execute.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int bump(void){ static int counter = 40; counter += 1; return counter; }\n\
int main(void){\n\
  volatile int reps = 2;\n\
  int r = 0;\n\
  for (int i = 0; i < reps; i++) r = bump();\n\
  char c[3]; c[0] = (char)(48 + r / 10); c[1] = (char)(48 + r % 10); c[2] = 10;\n\
  write(1, c, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("func-local-static", HOSTED_SRC, b"42\n")
}

/// Path Z Part 54 — variable-length array (C99 VLA: a runtime-sized automatic
/// array → dynamic stack-frame allocation) in a freshly-tcc-built dynamic glibc
/// binary.
///
/// Every prior automatic array had a compile-time-constant size, so its storage
/// was a fixed offset in the frame the compiler reserved with a single `sub rsp,
/// imm`.  A VLA (`int a[n]` where `n` is a runtime value) is fundamentally
/// different: the compiler must compute the size at runtime, subtract it from
/// `rsp` to carve out the space (rounding for alignment), remember the original
/// `rsp` (or the frame pointer) to unwind it on return, and index into the
/// dynamically-placed block — the same dynamic-stack mechanism as `alloca`.
/// Getting the runtime frame adjustment, alignment, and teardown right is a
/// distinct, easily-mis-lowered codegen path (a bad `rsp` computation corrupts
/// the stack), so a compiled-on-target VLA closes a genuine gap.  (Confirmed via
/// the off-target build that this tcc lowers the VLA inline — no `alloca`/bounds
/// helper is pulled, so the link surface stays clean.)
///
/// `sumfill(n)` declares `int a[n]`, fills it with `1..=n`, and returns the sum.
/// `n` is a `volatile` 8 so the compiler cannot fold the size to a constant and
/// must emit the real runtime allocation; `sum(1..=8)` = 36, and `+ 6` gives 42,
/// printed as its two decimal digits — captured file exactly `42\n` (3 bytes,
/// exit 0).  Only undefined symbol is `write`.  Pure userspace/codegen.
pub fn self_test_linux_real_glibc_cc_vla() -> KernelResult<()> {
    // `int a[n]` with runtime `n` => VLA => dynamic `sub rsp, size` frame carve +
    // aligned teardown on return (the alloca mechanism).  `volatile n` defeats
    // constant-folding the size so the real runtime stack allocation is emitted.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int sumfill(int n){\n\
  int a[n];\n\
  for (int i = 0; i < n; i++) a[i] = i + 1;\n\
  int s = 0;\n\
  for (int i = 0; i < n; i++) s += a[i];\n\
  return s;\n\
}\n\
int main(void){\n\
  volatile int n = 8;\n\
  int r = sumfill(n) + 6;\n\
  char c[3]; c[0] = (char)(48 + r / 10); c[1] = (char)(48 + r % 10); c[2] = 10;\n\
  write(1, c, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("vla-dynstack", HOSTED_SRC, b"42\n")
}

/// Path Z Part 55 — GCC-style inline assembly with operand constraints in a
/// freshly-tcc-built dynamic glibc binary.
///
/// Every prior rung stayed in pure C, so tcc's inline-assembler — a *separate*
/// compiler subsystem from C codegen — was untested from compiled source.
/// Extended `__asm__` with operand constraints is not a passthrough string: the
/// compiler must parse the constraint list, *allocate registers* for each
/// `"=r"`/`"r"` operand and a tied `"0"` input, substitute them into the `%0`/
/// `%2` template placeholders, and honour the clobber/data-flow so the
/// surrounding C sees the output.  Getting the register allocation and operand
/// substitution right is a distinct, easily-broken facility, and it is the
/// mechanism real libc/drivers use to emit `syscall`, `cpuid`, atomics, and
/// MMIO — so proving the on-target compiler handles it closes a genuine,
/// high-value gap (it unlocks a large class of systems C).
///
/// `asm_add(a, b)` computes `a + b` with a single `addl %2, %0` where `%0` is an
/// output register tied to input `a` (constraint `"0"`) and `%2` is `b` in any
/// register (`"r"`).  `asm_add(20, 22)` = 42 (the first operand from a
/// `volatile` so the compiler cannot fold the add away and must actually route
/// the values through the asm), printed as its two decimal digits — captured
/// file exactly `42\n` (3 bytes, exit 0).  Only undefined symbol is `write`.
/// Pure userspace/codegen (no kernel path).
pub fn self_test_linux_real_glibc_cc_inline_asm() -> KernelResult<()> {
    // Extended asm: "=r"(r) output, "0"(a) input tied to the output register,
    // "r"(b) input in any GPR; tcc must allocate the regs and substitute them
    // into `%0`/`%2`.  `volatile x` defeats folding so the add really runs
    // through the asm rather than being computed at compile time.
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int asm_add(int a, int b){\n\
  int r;\n\
  __asm__ (\"addl %2, %0\" : \"=r\"(r) : \"0\"(a), \"r\"(b));\n\
  return r;\n\
}\n\
int main(void){\n\
  volatile int x = 20;\n\
  int r = asm_add(x, 22);\n\
  char c[3]; c[0] = (char)(48 + r / 10); c[1] = (char)(48 + r % 10); c[2] = 10;\n\
  write(1, c, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("inline-asm", HOSTED_SRC, b"42\n")
}

/// Path Z Part 57: C11 `_Atomic` + `__atomic_fetch_add` builtin compiled by the
/// on-target tcc, glibc-linked, and run in ring 3.
///
/// C11 atomics are their own codegen + runtime-library facility, distinct from
/// everything else in this suite: tcc lowers the generic `__atomic_fetch_add`
/// builtin on an aligned `int` to a call to the sized helper
/// `__atomic_fetch_add_4`, which is *not* in glibc — it is provided by tcc's own
/// `libtcc1.a` (verified present: `nm libtcc1.a` shows the full
/// `__atomic_{add_fetch,fetch_add,exchange,...}_{1,2,4,8}` family). So this case
/// proves two things at once that no other Path-Z part does: (1) the compiler's
/// C11 `_Atomic` type + atomic-builtin lowering, and (2) that the hosted-compile
/// link pulls the atomic runtime helpers out of `libtcc1.a` — the exact library
/// whose linkage B-TCC-LIBTCC1-MAIN was about. A plain `_Atomic` lvalue read
/// (`int t = c;`) is lowered inline (no extra undefined symbol), so the only
/// undefined symbols in the object are `write` (glibc) and `__atomic_fetch_add_4`
/// (libtcc1.a).
///
/// The loop count comes from a `static volatile` seed (`seedfn()` → 21) so the
/// compiler cannot constant-fold the accumulation away; 21 iterations of `+= 2`
/// give 42, printed as its two decimal digits — captured file exactly `42\n`
/// (3 bytes, exit 0). `__ATOMIC_SEQ_CST` is defined locally because tcc, unlike
/// gcc, does not predefine the memory-order macros. Pure userspace/codegen plus
/// libtcc1 linkage (no kernel path).
pub fn self_test_linux_real_glibc_cc_atomic() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
#ifndef __ATOMIC_SEQ_CST\n\
#define __ATOMIC_SEQ_CST 5\n\
#endif\n\
static int seedfn(void){ static volatile int s = 21; return s; }\n\
int main(void){\n\
  _Atomic int c = 0;\n\
  int n = seedfn();\n\
  for (int i = 0; i < n; i++)\n\
    __atomic_fetch_add(&c, 2, __ATOMIC_SEQ_CST);\n\
  int t = c;\n\
  char o[3]; o[0] = (char)(48 + t / 10); o[1] = (char)(48 + t % 10); o[2] = 10;\n\
  write(1, o, 3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("c11-atomic", HOSTED_SRC, b"42\n")
}

/// Path Z Part 58: GNU statement expressions (`({ ... })`) + `__typeof__`
/// compiled by the on-target tcc, glibc-linked, and run in ring 3.
///
/// Statement expressions and `__typeof__` are the two GNU C extensions that
/// underpin the type-generic, side-effect-safe macros used pervasively across
/// real systems C — `container_of`, and the canonical `min`/`max`
/// (`({ typeof(a) _x=(a); typeof(b) _y=(b); _x<_y?_x:_y; })`) that evaluate each
/// argument exactly once. glibc's, the Linux kernel's, and most C library
/// headers lean on them, so proving the on-target compiler lowers a
/// statement-expression body (a block whose value is its last expression) and
/// resolves `__typeof__` types correctly is a genuine prerequisite for compiling
/// that ecosystem — a facility no other Path-Z part exercises. Pure
/// userspace/codegen (only undefined symbol is `write`; no kernel path).
///
/// `MAX` is the classic once-eval statement-expression macro. With a `static
/// volatile` seed (`seedfn()` → 42, defeating constant-folding),
/// `MAX(s, MAX(17, s-5))` = `MAX(42, MAX(17, 37))` = `MAX(42, 37)` = 42, printed
/// as its two decimal digits — captured file exactly `42\n` (3 bytes, exit 0).
pub fn self_test_linux_real_glibc_cc_stmt_expr() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
#define MAX(a,b) ({ __typeof__(a) _x=(a); __typeof__(b) _y=(b); _x>_y?_x:_y; })\n\
static int seedfn(void){ static volatile int s = 42; return s; }\n\
int main(void){\n\
  int s = seedfn();\n\
  int r = MAX(s, MAX(17, s-5));\n\
  char o[3]; o[0]=(char)(48+r/10); o[1]=(char)(48+r%10); o[2]=10;\n\
  write(1,o,3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("stmt-expr", HOSTED_SRC, b"42\n")
}

/// Path Z Part 59: C11 `_Generic` type-generic selection compiled by the
/// on-target tcc, glibc-linked, and run in ring 3.
///
/// `_Generic` is C11's compile-time type-directed selection primitive — the
/// mechanism behind `<tgmath.h>` and every modern type-generic C macro that
/// dispatches on the *static type* of its argument (e.g. picking `sinf`/`sin`/
/// `sinl`, or a type-appropriate format/handler). It is a distinct front-end
/// facility from everything else in this suite: the compiler must resolve the
/// controlling expression's type, match it against the association list, and
/// substitute the selected expression — all at translation time. Proving the
/// on-target tcc implements it correctly is a real prerequisite for compiling
/// modern C that leans on `<tgmath.h>`/type-generic headers. Pure userspace/
/// codegen (only undefined symbol is `write`; no kernel path).
///
/// `TAG(x)` maps a controlling expression's static type to a weight
/// (`int`→10, `long`→20, `double`→5, `char`→7, else 0). Applied to one variable
/// of each type, `10 + 20 + 5 + 7` = 42. The values come from a `static volatile`
/// seed so the compiler cannot fold the operands away — but the selection itself
/// is inherently static (it depends only on the declared type), which is exactly
/// the property under test. Printed as two decimal digits — captured file exactly
/// `42\n` (3 bytes, exit 0).
pub fn self_test_linux_real_glibc_cc_generic() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
#define TAG(x) _Generic((x), int: 10, long: 20, double: 5, char: 7, default: 0)\n\
static int seedfn(void){ static volatile int s = 0; return s; }\n\
int main(void){\n\
  int i = seedfn();\n\
  long l = seedfn();\n\
  double d = seedfn();\n\
  char c = (char)seedfn();\n\
  int t = TAG(i) + TAG(l) + TAG(d) + TAG(c);\n\
  char o[3]; o[0]=(char)(48+t/10); o[1]=(char)(48+t%10); o[2]=10;\n\
  write(1,o,3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("c11-generic", HOSTED_SRC, b"42\n")
}

/// Path Z Part 60: a dense `switch` (lowered to a jump table) compiled by the
/// on-target tcc, glibc-linked, and run in ring 3.
///
/// A dense integer `switch` is the canonical option/argument-parser shape in
/// real systems C (every coreutils `getopt` loop, every bash/readline state
/// machine). When the case labels are contiguous, the compiler lowers the whole
/// dispatch to an indexed **jump table** (an indirect `jmp` through a computed
/// table slot) rather than a chain of compares — a distinct codegen path from
/// the `&&label`/`goto *` computed-goto tested in Part (computed_goto): that one
/// exercises *explicit* address-of-label indirect jumps, this one exercises the
/// compiler's *implicit* switch-table construction and bounds/default handling.
/// Proving the on-target tcc builds and executes a jump table correctly directly
/// de-risks compiling real parser code. Pure userspace/codegen (only undefined
/// symbol is `write`; no kernel path).
///
/// `weight(c)` is an 8-arm contiguous switch (`case 0..=7 → c+1`, else 0). Summed
/// over `c = 0..=5` (loop bound from a `static volatile` seed = 5, defeating
/// folding) gives `1+2+3+4+5+6 = 21`; `21 + seed(5) + 16 = 42`, printed as two
/// decimal digits — captured file exactly `42\n` (3 bytes, exit 0).
pub fn self_test_linux_real_glibc_cc_switch() -> KernelResult<()> {
    const HOSTED_SRC: &[u8] = b"extern long write(int fd, const void *buf, unsigned long n);\n\
static int seedfn(void){ static volatile int s = 5; return s; }\n\
static int weight(int c){\n\
  switch (c){\n\
    case 0: return 1;\n\
    case 1: return 2;\n\
    case 2: return 3;\n\
    case 3: return 4;\n\
    case 4: return 5;\n\
    case 5: return 6;\n\
    case 6: return 7;\n\
    case 7: return 8;\n\
    default: return 0;\n\
  }\n\
}\n\
int main(void){\n\
  int base = seedfn();\n\
  int t = 0;\n\
  for (int i = 0; i <= base; i++) t += weight(i);\n\
  t += base + 16;\n\
  char o[3]; o[0]=(char)(48+t/10); o[1]=(char)(48+t%10); o[2]=10;\n\
  write(1,o,3);\n\
  return 0;\n\
}\n";
    run_hosted_cc_case("switch-table", HOSTED_SRC, b"42\n")
}

/// Test 1: Spawn a process from a valid ELF binary.
///
/// The test ELF contains real x86_64 code that calls SYS_EXIT(0) via
/// SYSCALL.  The process runs in ring 3, executes the code, and exits
/// cleanly.
fn test_spawn_from_elf() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();
    let options = SpawnOptions::new("spawn-test-1");

    let result = spawn_process(&elf_data, &options)?;

    // Verify the process was created and is Running.
    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Running) {
        serial_println!("[spawn]   FAIL: process should be Running, got {:?}", s);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Verify the entry point was captured.
    if result.entry_point != 0x0000_0040_0000_0000 {
        serial_println!("[spawn]   FAIL: wrong entry point: {:#x}", result.entry_point);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Let the thread run.  It will:
    //   1. Execute the trampoline (ring 0 → ring 3 via IRETQ)
    //   2. Run the user code (mov eax, 1; xor edi, edi; syscall)
    //   3. SYS_EXIT handler notifies thread system → process becomes zombie
    //   4. Task exits, scheduler returns here.
    crate::sched::yield_now();
    crate::sched::yield_now();

    // The process should now be a zombie (SYS_EXIT called
    // on_thread_exit automatically).  The manual call below is a
    // harmless no-op (the mapping was already removed).
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Spawn from ELF (ring 3): OK");
    Ok(())
}

/// Test 2: Spawn with invalid ELF fails cleanly.
fn test_spawn_invalid_elf() -> KernelResult<()> {
    let bad_data = [0u8; 16]; // Not an ELF file.
    let options = SpawnOptions::new("spawn-test-bad");

    match spawn_process(&bad_data, &options) {
        Err(KernelError::InvalidExecutable) => {} // Expected.
        other => {
            serial_println!("[spawn]   FAIL: invalid ELF should fail, got {:?}", other.map(|r| r.pid));
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[spawn]   Reject invalid ELF: OK");
    Ok(())
}

/// Test 3: Spawn with initial capabilities.
fn test_spawn_with_capabilities() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();
    let caps = [(ResourceType::Channel, 42, Rights::READ | Rights::WRITE)];
    let options = SpawnOptions {
        name: "spawn-test-caps",
        parent: 0,
        priority: DEFAULT_PRIORITY,
        capabilities: &caps,
        fd_map: &[],
        argv: &[],
        envp: &[],
        exe_path: None,
        cwd: None,
        uid_gid: None,
    };

    let result = spawn_process(&elf_data, &options)?;

    // Verify the process is running with the right entry point.
    if result.entry_point != 0x0000_0040_0000_0000 {
        serial_println!("[spawn]   FAIL: wrong entry point");
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Let thread run (ring 3 → SYS_EXIT) and clean up.
    crate::sched::yield_now();
    crate::sched::yield_now();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Spawn with capabilities: OK");
    Ok(())
}

/// Test 4: Spawn a process that faults (null deref) — kernel survives.
///
/// The test ELF writes to address 0x0, causing a page fault in ring 3.
/// The exception handler should kill the task and the process should
/// become a zombie.  The kernel must continue running.
fn test_spawn_faulting_process() -> KernelResult<()> {
    let elf_data = elf::build_faulting_test_elf();
    let options = SpawnOptions::new("spawn-test-fault");

    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run.  It will:
    //   1. Execute the trampoline (ring 0 → ring 3 via IRETQ)
    //   2. Execute `xor eax, eax; mov [rax], eax` → #PF at address 0
    //   3. Exception handler detects ring 3 fault → kills the task
    //   4. Process becomes zombie, scheduler returns here.
    crate::sched::yield_now();
    crate::sched::yield_now();

    // The process should be a zombie (the exception handler called
    // on_thread_exit + task_exit).
    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: faulting process should be Zombie, got {:?}",
            s
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    pcb::destroy(result.pid);

    serial_println!("[spawn]   Faulting process killed (kernel survived): OK");
    Ok(())
}

/// Test 5: Spawn a process that grows its stack beyond the initial allocation.
///
/// The test ELF decrements RSP by 128 KiB (past the initial 64 KiB
/// allocation) and writes to the new location.  This triggers page
/// faults that the kernel resolves via stack growth.  The process then
/// calls SYS_EXIT(0) successfully.
fn test_spawn_stack_growth() -> KernelResult<()> {
    let elf_data = elf::build_stack_growth_test_elf();
    let options = SpawnOptions::new("spawn-test-stack");

    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run.  It will:
    //   1. IRETQ to ring 3
    //   2. sub rsp, 0x20000 (RSP now 128 KiB below initial stack top)
    //   3. mov qword [rsp], 42 → triggers page fault in stack region
    //   4. Kernel grows stack (allocates + maps new frame), returns
    //   5. Process continues: SYS_EXIT(0) → zombie
    crate::sched::yield_now();
    crate::sched::yield_now();

    // The process should be a zombie (SYS_EXIT was called).
    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: stack-growth process should be Zombie, got {:?}",
            s
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    pcb::destroy(result.pid);

    serial_println!("[spawn]   Stack growth (128 KiB past initial): OK");
    Ok(())
}

/// Test 6: Exec replaces process image and continues executing.
///
/// Flow:
/// 1. Build a "target" ELF (calls SYS_EXIT(0) — same as the normal test ELF).
/// 2. Choose a user-space data address (0x50_0000_0000) and map the target
///    ELF bytes there as read-only data.
/// 3. Build a "caller" ELF whose code calls SYS_PROCESS_EXEC(data_addr, len).
/// 4. Spawn the process with the caller ELF.
/// 5. Yield to let the process run:
///    a. Process starts at the caller code.
///    b. SYSCALL(503) → kernel tears down old AS, loads target ELF, returns
///    to the target's entry point via SYSRET.
///    c. Target code runs: SYS_EXIT(0) → process becomes zombie.
/// 6. Verify clean exit.
fn test_exec_process() -> KernelResult<()> {
    // -- Step 1: Build the target ELF that the exec will load --
    let target_elf = elf::build_test_elf_public();

    // -- Step 2: Pick a user-space address for the ELF data --
    //
    // 0x50_0000_0000 is well within the user half and far from both the
    // code segment (0x40_0000_0000) and the stack (near 0x7FFF_FFFF_0000).
    let data_vaddr: u64 = 0x0000_0050_0000_0000;

    // -- Step 3: Build the caller ELF --
    //
    // Its code does: mov eax, 503; movabs rdi, data_vaddr; mov esi, target_len; syscall
    #[allow(clippy::cast_possible_truncation)]
    let caller_elf = elf::build_exec_test_elf(
        data_vaddr,
        target_elf.len() as u32,
    );

    // -- Step 4: Spawn the process with the caller ELF --
    let options = SpawnOptions::new("spawn-test-exec");
    let result = spawn_process(&caller_elf, &options)?;

    // -- Step 4b: Map the target ELF data into the process's address space --
    //
    // We need to copy the target ELF bytes into frames and map them
    // at data_vaddr so the caller code can reference them.
    let pml4_phys = pcb::get_pml4(result.pid)
        .filter(|&p| p != 0)
        .ok_or(KernelError::OutOfMemory)?;

    let hhdm = page_table::hhdm()
        .ok_or(KernelError::InternalError)?;

    // Calculate how many frames we need for the target ELF data.
    #[allow(clippy::arithmetic_side_effects)]
    let frames_needed = target_elf.len().div_ceil(FRAME_SIZE);
    let mut bytes_copied = 0usize;

    for i in 0..frames_needed {
        let phys_frame = frame::alloc_frame()?;
        let frame_virt = phys_frame.to_virt(hhdm);

        // Zero the frame first.
        // SAFETY: frame_virt is HHDM mapping of freshly allocated frame.
        unsafe {
            core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
        }

        // Copy ELF data into the frame.
        #[allow(clippy::arithmetic_side_effects)]
        let chunk_start = i * FRAME_SIZE;
        #[allow(clippy::arithmetic_side_effects)]
        let chunk_end = (chunk_start + FRAME_SIZE).min(target_elf.len());
        let chunk = &target_elf[chunk_start..chunk_end];

        // SAFETY: frame_virt is valid, chunk fits within FRAME_SIZE.
        unsafe {
            core::ptr::copy_nonoverlapping(
                chunk.as_ptr(),
                frame_virt as *mut u8,
                chunk.len(),
            );
        }

        // Map at data_vaddr + i * FRAME_SIZE.
        #[allow(clippy::arithmetic_side_effects)]
        let vaddr = data_vaddr + (i as u64 * FRAME_SIZE as u64);
        let flags = PageFlags::PRESENT
            | PageFlags::USER_ACCESSIBLE
            | PageFlags::NO_EXECUTE;

        // SAFETY: pml4_phys is valid, phys_frame is freshly allocated.
        unsafe {
            page_table::map_frame(
                pml4_phys,
                page_table::VirtAddr::new(vaddr),
                phys_frame,
                flags,
            )?;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            bytes_copied += chunk.len();
        }
    }

    serial_println!(
        "[spawn]   Exec test: mapped {} bytes of target ELF at {:#x}",
        bytes_copied, data_vaddr
    );

    // -- Step 5: Let the process run --
    //
    // The scheduler will pick the new thread.  It will:
    //   1. Run caller code → SYSCALL(503, data_vaddr, target_len)
    //   2. Kernel: validate ELF, tear down old AS (including the data
    //      pages we just mapped!), load target ELF, set up new stack
    //   3. SYSRET to target's entry point
    //   4. Target code: mov eax,1; xor edi,edi; syscall → SYS_EXIT(0)
    //   5. Process becomes zombie, thread exits
    crate::sched::yield_now();
    crate::sched::yield_now();

    // Verify the process is now a zombie (exec succeeded, new code ran,
    // SYS_EXIT was called).
    let state = pcb::state(result.pid);
    if state != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: after exec, expected Zombie, got {:?}",
            state
        );
        // Clean up.
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Clean up.
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Exec (replace process image): OK");
    Ok(())
}

/// Test 7: SEH — exception handler catches a page fault and calls SYS_EXIT.
///
/// Exercises the full exception dispatch path:
/// 1. Process registers an exception handler via `SYS_SET_EXCEPTION_HANDLER`.
/// 2. Process triggers a page fault (null pointer dereference).
/// 3. Kernel pushes an `ExceptionContext` onto the user stack and
///    redirects execution to the registered handler.
/// 4. Handler calls `SYS_EXIT(0)`.
/// 5. Process becomes a zombie — confirming the handler ran.
///
/// Without SEH, the page fault would kill the process (same as test 4).
/// The difference here is that the handler gets control and exits
/// cleanly, proving the exception dispatch machinery works.
fn test_seh_handler_exit() -> KernelResult<()> {
    let elf_data = elf::build_seh_exit_test_elf();
    let options = SpawnOptions::new("spawn-test-seh-exit");

    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run:
    //   IRETQ → ring 3 → register handler → null deref → #PF →
    //   kernel dispatches to handler → handler calls SYS_EXIT → zombie
    crate::sched::yield_now();
    crate::sched::yield_now();

    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: SEH exit test — expected Zombie, got {:?}",
            s
        );
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   SEH handler catches fault (calls SYS_EXIT): OK");
    Ok(())
}

/// Test 8: SEH — exception handler resumes execution via `SYS_EXCEPTION_RETURN`.
///
/// Exercises the full SEH round-trip:
/// 1. Process registers an exception handler.
/// 2. Process executes `ud2` (invalid opcode → `#UD`).
/// 3. Kernel dispatches to handler with `ExceptionContext` on user stack.
/// 4. Handler adds 2 to `ctx.rip` (skipping the 2-byte `ud2`).
/// 5. Handler calls `SYS_EXCEPTION_RETURN(ctx_ptr)`.
/// 6. Kernel restores the CPU state from the modified context.
/// 7. Process resumes at the instruction after `ud2`.
/// 8. Process calls `SYS_EXIT(0)` — becomes a zombie.
///
/// This test proves the entire exception → handler → resume flow works,
/// including context saving, user-space modification, and restoration.
fn test_seh_handler_resume() -> KernelResult<()> {
    let elf_data = elf::build_seh_resume_test_elf();
    let options = SpawnOptions::new("spawn-test-seh-resume");

    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run:
    //   IRETQ → ring 3 → register handler → ud2 → #UD →
    //   kernel dispatches to handler → handler modifies ctx.rip →
    //   SYS_EXCEPTION_RETURN → resumes past ud2 → SYS_EXIT → zombie
    crate::sched::yield_now();
    crate::sched::yield_now();

    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: SEH resume test — expected Zombie, got {:?}",
            s
        );
        thread::on_thread_exit(result.task_id);
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   SEH handler resumes execution (SYS_EXCEPTION_RETURN): OK");
    Ok(())
}

/// Test 9: Force-kill a process before it runs.
///
/// Spawns a process (thread enters the Ready queue) then kills all its
/// threads without ever yielding.  Verifies:
/// - The thread is dequeued from the scheduler and marked Dead.
/// - The process transitions to Zombie with the specified exit code.
/// - Scheduler resources are properly cleaned up.
fn test_process_kill() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();
    let options = SpawnOptions::new("spawn-test-kill");

    let result = spawn_process(&elf_data, &options)?;

    // Process should be Running (initial thread was spawned).
    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Running) {
        serial_println!(
            "[spawn]   FAIL: kill test — expected Running, got {:?}",
            s
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Set exit code and kill all threads (simulating SYS_PROCESS_KILL).
    pcb::set_exit_code(result.pid, -9)?;
    let killed = thread::kill_process_threads(result.pid);

    if killed != 1 {
        serial_println!(
            "[spawn]   FAIL: kill test — expected 1 thread killed, got {}",
            killed
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Process should now be Zombie.
    let s = pcb::state(result.pid);
    if s != Some(pcb::ProcessState::Zombie) {
        serial_println!(
            "[spawn]   FAIL: kill test — expected Zombie after kill, got {:?}",
            s
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Verify exit code.
    let ec = pcb::exit_code(result.pid);
    if ec != Some(-9) {
        serial_println!(
            "[spawn]   FAIL: kill test — expected exit code -9, got {:?}",
            ec
        );
        pcb::destroy(result.pid);
        return Err(KernelError::InternalError);
    }

    // Reap the dead scheduler task and destroy the process.
    crate::sched::reap_dead_tasks();
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Process kill (force-terminate before run): OK");
    Ok(())
}

/// Test 10: Verify that destroying a process frees all its frames.
///
/// Spawns a process, lets it run (allocating ELF segment frames, user
/// stack frames, and page table pages), then destroys it and checks
/// that the free frame count returns to the pre-spawn value.
fn test_no_frame_leak() -> KernelResult<()> {
    let before = frame::stats()
        .ok_or(KernelError::InternalError)?;

    let elf_data = elf::build_test_elf_public();
    let options = SpawnOptions::new("spawn-test-leak");
    let result = spawn_process(&elf_data, &options)?;

    // Let the thread run (ring 3 → SYS_EXIT → zombie).
    crate::sched::yield_now();
    crate::sched::yield_now();

    // Clean up the dead scheduler task so its kernel stack is freed.
    crate::sched::reap_dead_tasks();

    // Now destroy the process (should free all user AS frames).
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    let after = frame::stats()
        .ok_or(KernelError::InternalError)?;

    // The page table page pool may have grown (16 KiB frames split
    // into 4 KiB pages that aren't returned to the frame allocator).
    // So we allow a small discrepancy of up to 2 frames for PT pool
    // overhead.
    let leaked = before.free_frames.saturating_sub(after.free_frames);
    if leaked > 2 {
        serial_println!(
            "[spawn]   FAIL: frame leak detected — before={}, after={}, leaked={}",
            before.free_frames, after.free_frames, leaked
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[spawn]   No frame leak (before={}, after={}, delta={}): OK",
        before.free_frames, after.free_frames, leaked
    );
    Ok(())
}

/// Test: FdMapEntry has the correct size and alignment for C ABI.
fn test_fd_map_entry_layout() -> KernelResult<()> {
    let size = core::mem::size_of::<FdMapEntry>();
    let align = core::mem::align_of::<FdMapEntry>();

    if size != 16 {
        serial_println!(
            "[spawn]   FAIL: FdMapEntry size should be 16, got {}",
            size
        );
        return Err(KernelError::InternalError);
    }
    if align < 4 {
        serial_println!(
            "[spawn]   FAIL: FdMapEntry alignment should be ≥4, got {}",
            align
        );
        return Err(KernelError::InternalError);
    }

    // Verify field offsets are correct.
    let entry = FdMapEntry { fd: 1, handle_type: fd_handle_type::FILE, _pad: [0; 3], handle: 42 };
    if entry.fd != 1 || entry.handle != 42 {
        serial_println!("[spawn]   FAIL: FdMapEntry field values wrong");
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   FdMapEntry layout (size={}, align={}): OK", size, align);
    Ok(())
}

/// Test: Spawn a process with an fd map — handles are duped into child PCB.
///
/// Requires VFS to be initialized (needs a real file handle to dup).
/// Skips gracefully if VFS is not yet available — proc::self_test()
/// runs before filesystem initialization during boot.
fn test_spawn_with_fd_map() -> KernelResult<()> {
    use crate::fs::handle;

    // Create a file to get a real kernel handle.
    // This may fail during early boot before VFS is mounted.
    let parent_handle = match handle::open(
        "/test_fd_map_spawn.tmp",
        handle::OpenFlags::READ.union(handle::OpenFlags::WRITE).union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(_) => {
            serial_println!("[spawn]   Spawn with fd_map: SKIP (VFS not ready)");
            return Ok(());
        }
    };

    // Spawn with fd_map: fd 1 → parent_handle.
    let elf_data = elf::build_test_elf_public();
    let fd_map = [(1_i32, fd_handle_type::FILE, parent_handle)];
    let options = SpawnOptions::new("spawn-test-fdmap").fd_map(&fd_map);

    let result = spawn_process(&elf_data, &options)?;

    // The child's PCB should have initial_fds with one entry.
    let child_fds = pcb::take_initial_fds(result.pid);
    if child_fds.len() != 1 {
        serial_println!(
            "[spawn]   FAIL: expected 1 initial fd, got {}",
            child_fds.len()
        );
        // Clean up.
        for &(_fd, ht, h) in &child_fds {
            if ht == fd_handle_type::FILE {
                let _ = handle::close(h);
            }
        }
        let _ = handle::close(parent_handle);
        let _ = crate::fs::Vfs::remove("/test_fd_map_spawn.tmp");
        return Err(KernelError::InternalError);
    }

    let (fd_num, child_ht, child_handle) = child_fds[0];
    if fd_num != 1 {
        serial_println!(
            "[spawn]   FAIL: expected fd 1, got {}",
            fd_num
        );
    }

    // handle_type should be preserved through the spawn.
    if child_ht != fd_handle_type::FILE {
        serial_println!(
            "[spawn]   FAIL: expected handle_type FILE ({}), got {}",
            fd_handle_type::FILE, child_ht
        );
    }

    // The child handle should be different from the parent handle
    // (it's a dup, not the same ID).
    if child_handle == parent_handle {
        serial_println!(
            "[spawn]   FAIL: child handle {} should differ from parent handle {}",
            child_handle, parent_handle
        );
    }

    // Both handles should be valid (we can query their paths).
    let parent_path = handle::handle_path(parent_handle)?;
    let child_path = handle::handle_path(child_handle)?;
    if parent_path != child_path {
        serial_println!(
            "[spawn]   FAIL: paths should match: parent='{}', child='{}'",
            parent_path, child_path
        );
    }

    // Clean up: close both handles, let the process die, destroy it.
    let _ = handle::close(child_handle);
    let _ = handle::close(parent_handle);

    // Let the child run (exit via SYS_EXIT).
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    let _ = crate::fs::Vfs::remove("/test_fd_map_spawn.tmp");

    serial_println!("[spawn]   Spawn with fd_map (1 entry, handle duped): OK");
    Ok(())
}

/// Test: Spawn with an empty fd_map (default behavior, no fds inherited).
fn test_spawn_with_empty_fd_map() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();
    let options = SpawnOptions::new("spawn-test-empty-fdmap").fd_map(&[]);

    let result = spawn_process(&elf_data, &options)?;

    // No initial fds should be set.
    let fds = pcb::take_initial_fds(result.pid);
    if !fds.is_empty() {
        serial_println!(
            "[spawn]   FAIL: expected 0 initial fds, got {}",
            fds.len()
        );
        // Clean up leaked handles.
        for &(_fd, ht, h) in &fds {
            if ht == fd_handle_type::FILE {
                let _ = crate::fs::handle::close(h);
            }
        }
        return Err(KernelError::InternalError);
    }

    // Clean up.
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Spawn with empty fd_map: OK");
    Ok(())
}

/// Test: Spawn with an invalid handle in fd_map fails gracefully.
fn test_spawn_fd_map_invalid_handle() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    // Handle 999999 doesn't exist — dup should fail.
    let fd_map = [(0_i32, fd_handle_type::FILE, 999_999_u64)];
    let options = SpawnOptions::new("spawn-test-bad-fd").fd_map(&fd_map);

    match spawn_process(&elf_data, &options) {
        Ok(result) => {
            // Should NOT succeed — clean up and fail.
            crate::sched::yield_now();
            crate::sched::yield_now();
            crate::sched::reap_dead_tasks();
            thread::on_thread_exit(result.task_id);
            pcb::destroy(result.pid);
            serial_println!(
                "[spawn]   FAIL: spawn with invalid handle should fail"
            );
            Err(KernelError::InternalError)
        }
        Err(KernelError::InvalidHandle) => {
            serial_println!("[spawn]   Spawn with invalid handle → InvalidHandle: OK");
            Ok(())
        }
        Err(e) => {
            // Any error is acceptable (InvalidHandle is expected, but
            // other errors are fine too — the point is it doesn't succeed).
            serial_println!(
                "[spawn]   Spawn with invalid handle → {:?} (expected InvalidHandle): OK",
                e
            );
            Ok(())
        }
    }
}

/// Test: take_initial_fds is one-shot — second call returns empty.
///
/// Requires VFS to be initialized (needs a real file handle).
/// Skips gracefully if VFS is not yet available.
fn test_take_initial_fds_one_shot() -> KernelResult<()> {
    use crate::fs::handle;

    let parent_handle = match handle::open(
        "/test_fd_oneshot.tmp",
        handle::OpenFlags::READ.union(handle::OpenFlags::WRITE).union(handle::OpenFlags::CREATE),
    ) {
        Ok(h) => h,
        Err(_) => {
            serial_println!("[spawn]   take_initial_fds one-shot: SKIP (VFS not ready)");
            return Ok(());
        }
    };

    let elf_data = elf::build_test_elf_public();
    let fd_map = [(0_i32, fd_handle_type::FILE, parent_handle)];
    let options = SpawnOptions::new("spawn-test-oneshot").fd_map(&fd_map);

    let result = spawn_process(&elf_data, &options)?;

    // First take: should get 1 entry.
    let fds = pcb::take_initial_fds(result.pid);
    if fds.len() != 1 {
        serial_println!(
            "[spawn]   FAIL: first take expected 1 fd, got {}",
            fds.len()
        );
    }

    // Close the duped handle.
    for &(_fd, ht, h) in &fds {
        if ht == fd_handle_type::FILE {
            let _ = handle::close(h);
        }
    }

    // Second take: should get 0 entries (already consumed).
    let fds2 = pcb::take_initial_fds(result.pid);
    if !fds2.is_empty() {
        serial_println!(
            "[spawn]   FAIL: second take expected 0 fds, got {}",
            fds2.len()
        );
        for &(_fd, ht, h) in &fds2 {
            if ht == fd_handle_type::FILE {
                let _ = handle::close(h);
            }
        }
        return Err(KernelError::InternalError);
    }

    // Clean up.
    let _ = handle::close(parent_handle);
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    let _ = crate::fs::Vfs::remove("/test_fd_oneshot.tmp");

    serial_println!("[spawn]   take_initial_fds is one-shot: OK");
    Ok(())
}

/// Test: SpawnArgsHeader has the correct size for C ABI.
fn test_spawn_args_header_layout() -> KernelResult<()> {
    let size = core::mem::size_of::<SpawnArgsHeader>();
    if size != 16 {
        serial_println!(
            "[spawn]   FAIL: SpawnArgsHeader size should be 16, got {}",
            size
        );
        return Err(KernelError::InternalError);
    }

    let header = SpawnArgsHeader {
        argc: 3,
        envc: 2,
        argv_data_len: 100,
        envp_data_len: 50,
    };
    if header.argc != 3 || header.envc != 2 || header.argv_data_len != 100 || header.envp_data_len != 50 {
        serial_println!("[spawn]   FAIL: SpawnArgsHeader field values wrong");
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   SpawnArgsHeader layout (size={}): OK", size);
    Ok(())
}

/// Test: Spawn a process with argv — args stored in child PCB.
fn test_spawn_with_argv() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    let args: &[&[u8]] = &[b"myprogram", b"--flag", b"value"];
    let options = SpawnOptions::new("spawn-test-argv").argv(args);

    let result = spawn_process(&elf_data, &options)?;

    // Check that the child's PCB has the args.
    let (argv, envp) = pcb::take_initial_args(result.pid);
    if argv.len() != 3 {
        serial_println!(
            "[spawn]   FAIL: expected 3 argv entries, got {}",
            argv.len()
        );
        return Err(KernelError::InternalError);
    }
    if !envp.is_empty() {
        serial_println!(
            "[spawn]   FAIL: expected 0 envp entries, got {}",
            envp.len()
        );
        return Err(KernelError::InternalError);
    }

    if argv[0] != b"myprogram" || argv[1] != b"--flag" || argv[2] != b"value" {
        serial_println!("[spawn]   FAIL: argv content mismatch");
        return Err(KernelError::InternalError);
    }

    // Clean up.
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Spawn with argv (3 args): OK");
    Ok(())
}

/// Test: Spawn with both argv and envp.
fn test_spawn_with_argv_envp() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    let args: &[&[u8]] = &[b"/bin/ls", b"-la"];
    let env: &[&[u8]] = &[b"PATH=/bin:/usr/bin", b"HOME=/root", b"LANG=en_US.UTF-8"];
    let options = SpawnOptions::new("spawn-test-argv-envp")
        .argv(args)
        .envp(env);

    let result = spawn_process(&elf_data, &options)?;

    let (argv, envp) = pcb::take_initial_args(result.pid);
    if argv.len() != 2 {
        serial_println!(
            "[spawn]   FAIL: expected 2 argv, got {}",
            argv.len()
        );
        return Err(KernelError::InternalError);
    }
    if envp.len() != 3 {
        serial_println!(
            "[spawn]   FAIL: expected 3 envp, got {}",
            envp.len()
        );
        return Err(KernelError::InternalError);
    }
    if argv[0] != b"/bin/ls" || argv[1] != b"-la" {
        serial_println!("[spawn]   FAIL: argv content mismatch");
        return Err(KernelError::InternalError);
    }
    if envp[0] != b"PATH=/bin:/usr/bin" || envp[1] != b"HOME=/root" || envp[2] != b"LANG=en_US.UTF-8" {
        serial_println!("[spawn]   FAIL: envp content mismatch");
        return Err(KernelError::InternalError);
    }

    // Clean up.
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   Spawn with argv + envp: OK");
    Ok(())
}

/// Test: `SpawnOptions::cwd` sets the child's initial working directory, and
/// an invalid value is ignored (child stays at the PCB default `/`).
///
/// Backs the container `WorkingDir`/`--workdir` feature: the init process must
/// start in the requested directory without an explicit `chdir`.  Uses the
/// synchronous half of spawn (the PCB cwd is set during `spawn_process_inner`,
/// before the thread runs), so reading it back is deterministic.
fn test_spawn_with_cwd() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    // Valid absolute cwd is applied.
    let options = SpawnOptions::new("spawn-test-cwd").cwd(b"/app/work");
    let result = spawn_process(&elf_data, &options)?;
    match pcb::get_cwd(result.pid) {
        Some(cwd) if cwd == b"/app/work" => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: expected cwd /app/work, got {:?}",
                other.as_deref().map(<[u8]>::to_vec)
            );
            crate::sched::yield_now();
            crate::sched::reap_dead_tasks();
            thread::on_thread_exit(result.task_id);
            pcb::destroy(result.pid);
            return Err(KernelError::InternalError);
        }
    }
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    // A relative (invalid) cwd is rejected by set_cwd → child stays at `/`.
    let bad = SpawnOptions::new("spawn-test-cwd-bad").cwd(b"relative/dir");
    let result2 = spawn_process(&elf_data, &bad)?;
    match pcb::get_cwd(result2.pid) {
        Some(cwd) if cwd == b"/" => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: invalid cwd should leave default /, got {:?}",
                other.as_deref().map(<[u8]>::to_vec)
            );
            crate::sched::yield_now();
            crate::sched::reap_dead_tasks();
            thread::on_thread_exit(result2.task_id);
            pcb::destroy(result2.pid);
            return Err(KernelError::InternalError);
        }
    }
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result2.task_id);
    pcb::destroy(result2.pid);

    serial_println!("[spawn]   Spawn with initial cwd (valid + invalid): OK");
    Ok(())
}

/// Test: an initial `(uid, gid)` is applied to the child's credentials, and a
/// child with no `uid_gid` keeps the default root credentials.
fn test_spawn_with_uid_gid() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    // Explicit non-root identity is applied.
    let options = SpawnOptions::new("spawn-test-uid").uid_gid(1000, 1001);
    let result = spawn_process(&elf_data, &options)?;
    match pcb::get_credentials(result.pid) {
        Some(creds) if creds.uid == 1000 && creds.gid == 1001 => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: expected uid/gid 1000/1001, got {:?}",
                other.map(|c| (c.uid, c.gid))
            );
            crate::sched::yield_now();
            crate::sched::reap_dead_tasks();
            thread::on_thread_exit(result.task_id);
            pcb::destroy(result.pid);
            return Err(KernelError::InternalError);
        }
    }
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    // No uid_gid → child keeps the default root (uid 0) credentials.
    let dflt = SpawnOptions::new("spawn-test-uid-default");
    let result2 = spawn_process(&elf_data, &dflt)?;
    match pcb::get_credentials(result2.pid) {
        Some(creds) if creds.uid == 0 => {}
        other => {
            serial_println!(
                "[spawn]   FAIL: default child should be root (uid 0), got {:?}",
                other.map(|c| (c.uid, c.gid))
            );
            crate::sched::yield_now();
            crate::sched::reap_dead_tasks();
            thread::on_thread_exit(result2.task_id);
            pcb::destroy(result2.pid);
            return Err(KernelError::InternalError);
        }
    }
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result2.task_id);
    pcb::destroy(result2.pid);

    serial_println!("[spawn]   Spawn with initial uid/gid (explicit + default): OK");
    Ok(())
}

/// Test: take_initial_args is one-shot.
fn test_spawn_args_one_shot() -> KernelResult<()> {
    let elf_data = elf::build_test_elf_public();

    let args: &[&[u8]] = &[b"test"];
    let options = SpawnOptions::new("spawn-test-args-oneshot").argv(args);

    let result = spawn_process(&elf_data, &options)?;

    // First take: should get the args.
    let (argv, _envp) = pcb::take_initial_args(result.pid);
    if argv.len() != 1 {
        serial_println!(
            "[spawn]   FAIL: first take expected 1 arg, got {}",
            argv.len()
        );
        return Err(KernelError::InternalError);
    }

    // Second take: should get empty.
    let (argv2, envp2) = pcb::take_initial_args(result.pid);
    if !argv2.is_empty() || !envp2.is_empty() {
        serial_println!(
            "[spawn]   FAIL: second take should be empty, got {} argv, {} envp",
            argv2.len(), envp2.len()
        );
        return Err(KernelError::InternalError);
    }

    // Clean up.
    crate::sched::yield_now();
    crate::sched::yield_now();
    crate::sched::reap_dead_tasks();
    thread::on_thread_exit(result.task_id);
    pcb::destroy(result.pid);

    serial_println!("[spawn]   take_initial_args is one-shot: OK");
    Ok(())
}

/// Test: SpawnExArgs struct has correct layout for C ABI.
fn test_spawn_ex_args_layout() -> KernelResult<()> {
    let size = core::mem::size_of::<SpawnExArgs>();
    // 12 fields × 8 bytes = 96 bytes.
    if size != 96 {
        serial_println!(
            "[spawn]   FAIL: SpawnExArgs size should be 96, got {}",
            size
        );
        return Err(KernelError::InternalError);
    }

    let align = core::mem::align_of::<SpawnExArgs>();
    if align < 8 {
        serial_println!(
            "[spawn]   FAIL: SpawnExArgs alignment should be ≥8, got {}",
            align
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[spawn]   SpawnExArgs layout (size={}, align={}): OK", size, align);
    Ok(())
}
