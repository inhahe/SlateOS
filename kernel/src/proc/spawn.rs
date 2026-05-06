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
//! - No argument passing (argc/argv/envp) yet.
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
        }
    }

    /// Set the parent process.
    #[must_use]
    pub fn parent(mut self, pid: ProcessId) -> Self {
        self.parent = pid;
        self
    }

    /// Set the initial thread priority.
    #[must_use]
    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
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

    let entry_point = elf_file.entry_point();
    serial_println!(
        "[spawn] ELF validated: {} segment(s), entry={:#x}, pie={}",
        segment_count, entry_point, elf_file.is_pie()
    );

    // Step 2: Create the process (allocates a per-process PML4 with
    // kernel entries 256-511 cloned from the boot PML4).
    let pid = pcb::create(options.name, options.parent);
    serial_println!("[spawn] Created process {} (\"{}\")", pid, options.name);

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

    // Step 3: Load ELF segments into the process address space.
    //
    // SAFETY: pml4_phys was freshly allocated by pcb::create with
    // kernel entries cloned from the boot PML4.  No other CPU is using
    // this address space yet.
    if let Err(e) = unsafe { elf::load_segments(&elf_file, pml4_phys) } {
        serial_println!("[spawn] Failed to load ELF segments: {:?}", e);
        pcb::destroy(pid);
        return Err(e);
    }

    // Step 4: Allocate and map the user stack.
    let user_rsp = match setup_user_stack(pml4_phys) {
        Ok(rsp) => rsp,
        Err(e) => {
            serial_println!("[spawn] Failed to set up user stack: {:?}", e);
            pcb::destroy(pid);
            return Err(e);
        }
    };

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

    // Step 6: Create the entry info struct (heap-allocated, freed by
    // the trampoline when the thread first runs) and spawn the
    // initial thread.
    let info = Box::new(UserEntryInfo {
        entry_rip: entry_point,
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
        pid, task_id, entry_point, user_rsp
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
/// - Does NOT modify the thread's kernel stack or scheduler state.
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

    let entry_point = elf_file.entry_point();
    serial_println!(
        "[exec] ELF validated for exec: {} segment(s), entry={:#x}",
        segment_count, entry_point
    );

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

    // Flush TLB for the user half.  Since we just freed all user
    // mappings, any stale TLB entries could cause use-after-free.
    // Reloading CR3 flushes all non-global TLB entries.
    unsafe {
        page_table::write_cr3(pml4_phys);
    }

    // Step 4: Load the new ELF segments into the clean address space.
    if let Err(e) = unsafe { elf::load_segments(&elf_file, pml4_phys) } {
        serial_println!("[exec] Failed to load new ELF segments: {:?}", e);
        // Process is in a broken state — caller should kill it.
        return Err(e);
    }

    // Step 5: Allocate and map a fresh user stack.
    let user_rsp = setup_user_stack(pml4_phys)?;

    serial_println!(
        "[exec] Process {} exec complete: entry={:#x}, rsp={:#x}",
        pid, entry_point, user_rsp
    );

    Ok(ExecResult {
        entry_rip: entry_point,
        user_rsp,
    })
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
            page_table::map_frame(pml4_phys, virt, phys_frame, flags)?;
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
///       to the target's entry point via SYSRET.
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
    let frames_needed = (target_elf.len() + FRAME_SIZE - 1) / FRAME_SIZE;
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
