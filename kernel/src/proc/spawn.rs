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
        }
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
    let is_linux_abi = elf_file.detect_linux_abi();
    if is_linux_abi {
        serial_println!("[spawn] Detected Linux x86_64 ABI binary");
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
