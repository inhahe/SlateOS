//! `fork()` with copy-on-write address spaces.
//!
//! ## Overview
//!
//! Unlike `spawn` (which loads a fresh ELF into a brand-new address
//! space), `fork` duplicates an existing process: the child gets a
//! copy-on-write clone of the parent's address space, an independent
//! clone of the parent's capability table, refcount-shared copies of
//! the parent's IPC/file handles, inherited signal-mask state, and a
//! single thread that resumes execution at exactly the point where the
//! parent issued the `fork` syscall — but with `RAX = 0` so userspace
//! can tell parent (returns child PID) from child (returns 0).
//!
//! ## Resume mechanism
//!
//! The parent enters the kernel via `SYSCALL`, so its complete user
//! register state is saved on the kernel stack in a [`SyscallFrame`].
//! `fork_process` snapshots that frame into a heap-boxed `[u64; 17]`
//! register image and spawns the child's thread with
//! [`fork_child_trampoline`] as its entry.  When the scheduler first
//! dispatches that thread, the trampoline reconstructs an `IRETQ` frame
//! from the snapshot and transitions to ring 3 at the parent's
//! post-syscall RIP/RSP/RFLAGS with all general-purpose registers
//! restored and `RAX` cleared to 0.
//!
//! The parent, meanwhile, returns from the syscall normally with the
//! child's PID as the return value — `sys_process_fork_with_frame` only
//! *reads* the parent's frame, it never rewrites it.
//!
//! ## Handle inheritance
//!
//! The child's userspace libc fd table lives in copy-on-write memory,
//! so it references the *same* kernel handle ids as the parent.  The
//! kernel must therefore bump the refcount on each existing id rather
//! than mint new ids.  Files, pipes, eventfds, and stream sockets all
//! support same-id refcounted duplication and are inherited.  Channels,
//! shared-memory regions, completion ports, and timers do **not** yet
//! have refcounted same-id dup and are *not* inherited by the child
//! (documented limitation — see `todo.txt`).
//!
//! ## References
//!
//! - Linux `kernel/fork.c` (`copy_process`, `dup_mm`) for the overall
//!   structure: clone mm → copy fds → copy signal state → set up the
//!   child task to return 0.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::syscall::entry::SyscallFrame;

use super::pcb::{self, ProcessId};
use super::thread;

// ---------------------------------------------------------------------------
// Register-image layout for the child resume trampoline
// ---------------------------------------------------------------------------

/// Number of `u64` slots in the child's saved register image.
const REG_IMAGE_LEN: usize = 17;

// Byte offsets into the register image, used by the inline-asm
// trampoline.  Keep these in sync with `build_reg_image`.
//
// [0]  RIP      [1]  CS       [2]  RFLAGS   [3]  RSP      [4]  SS
// [5]  RDI      [6]  RSI      [7]  RDX      [8]  R10      [9]  R8
// [10] R9       [11] RBX      [12] RBP      [13] R12      [14] R13
// [15] R14      [16] R15

/// Build the child's register image from the parent's syscall frame.
///
/// The child resumes at the same user RIP/RSP/RFLAGS as the parent,
/// with all general-purpose registers identical *except* RAX, which the
/// trampoline forces to 0.  RCX and R11 are intentionally omitted: the
/// `SYSCALL`/`SYSRET` ABI clobbers them, so userspace never relies on
/// their values after a syscall returns.
fn build_reg_image(frame: &SyscallFrame) -> [u64; REG_IMAGE_LEN] {
    [
        frame.user_rip,                  // 0: RIP
        u64::from(crate::gdt::USER_CS),  // 1: CS
        frame.user_rflags,               // 2: RFLAGS
        frame.user_rsp,                  // 3: RSP
        u64::from(crate::gdt::USER_DS),  // 4: SS
        frame.arg0,                      // 5: RDI
        frame.arg1,                      // 6: RSI
        frame.arg2,                      // 7: RDX
        frame.arg3,                      // 8: R10
        frame.arg4,                      // 9: R8
        frame.arg5,                      // 10: R9
        frame.rbx,                       // 11: RBX
        frame.rbp,                       // 12: RBP
        frame.r12,                       // 13: R12
        frame.r13,                       // 14: R13
        frame.r14,                       // 15: R14
        frame.r15,                       // 16: R15
    ]
}

/// Ring-0 trampoline that resumes a forked child in ring 3.
///
/// `image_raw` is a `Box<[u64; REG_IMAGE_LEN]>` (built by
/// [`fork_process`]) leaked via [`Box::into_raw`].  The trampoline
/// reclaims and frees the box, copies the register image onto its own
/// kernel stack, then builds an `IRETQ` frame and transitions to ring 3
/// with `RAX = 0`.
///
/// This runs on the child thread's freshly allocated kernel stack the
/// first time the scheduler dispatches it.
extern "C" fn fork_child_trampoline(image_raw: u64) {
    // Reclaim the heap-allocated register image and copy it onto our
    // own stack so we can free the heap allocation before the (never
    // returning) IRETQ.
    //
    // SAFETY: `image_raw` was produced by `Box::into_raw` in
    // `fork_process` for this thread alone.  No other code observes it.
    let boxed = unsafe { Box::from_raw(image_raw as *mut [u64; REG_IMAGE_LEN]) };
    let regs: [u64; REG_IMAGE_LEN] = *boxed;
    drop(boxed); // Free the heap allocation now — IRETQ never returns.

    let ptr = regs.as_ptr();

    // Build the IRETQ frame and transition to ring 3.
    //
    // SAFETY: The child's copy-on-write address space is active (the
    // scheduler switched CR3 to the child's PML4 before dispatching
    // this thread).  Every byte read here comes from `regs`, a live
    // stack array; `ptr` is kept in RCX, which is *not* among the
    // restored registers, so the memory reads complete before any
    // restore clobbers state.  The IRETQ frame is pushed in the
    // canonical order (SS, RSP, RFLAGS, CS, RIP) and matches the
    // selectors loaded into CS/SS.  RAX is zeroed so the child's
    // userspace observes a `fork()` return value of 0.
    unsafe {
        core::arch::asm!(
            // Push the IRETQ frame (stack grows down → reverse order).
            "mov rax, [rcx + 32]", "push rax", // SS
            "mov rax, [rcx + 24]", "push rax", // RSP
            "mov rax, [rcx + 16]", "push rax", // RFLAGS
            "mov rax, [rcx + 8]",  "push rax", // CS
            "mov rax, [rcx + 0]",  "push rax", // RIP
            // Restore general-purpose registers from the image.
            "mov rdi, [rcx + 40]",
            "mov rsi, [rcx + 48]",
            "mov rdx, [rcx + 56]",
            "mov r10, [rcx + 64]",
            "mov r8,  [rcx + 72]",
            "mov r9,  [rcx + 80]",
            "mov rbx, [rcx + 88]",
            "mov rbp, [rcx + 96]",
            "mov r12, [rcx + 104]",
            "mov r13, [rcx + 112]",
            "mov r14, [rcx + 120]",
            "mov r15, [rcx + 128]",
            // Child's fork() return value is 0.
            "xor rax, rax",
            "iretq",
            in("rcx") ptr,
            options(noreturn),
        );
    }
}

// ---------------------------------------------------------------------------
// Handle duplication for the child
// ---------------------------------------------------------------------------

/// Refcount-duplicate a single parent handle for the child.
///
/// Returns `Ok(Some(entry))` if the child should record `entry` in its
/// own handle list (same `(resource_type, id)` as the parent, with the
/// underlying resource's refcount bumped), `Ok(None)` if the handle is
/// not inheritable and should be skipped, or `Err` if duplication
/// failed (the caller then rolls back).
fn dup_one(rtype: ResourceType, id: u64) -> KernelResult<Option<(ResourceType, u64)>> {
    match rtype {
        ResourceType::File => {
            // Same id, shared file description, refcount bumped.
            crate::fs::handle::dup_shared(id)?;
            Ok(Some((rtype, id)))
        }
        ResourceType::Pipe => {
            crate::ipc::pipe::dup(crate::ipc::pipe::PipeHandle::from_raw(id))?;
            Ok(Some((rtype, id)))
        }
        ResourceType::EventFd => {
            crate::ipc::eventfd::dup(crate::ipc::eventfd::EventFdHandle::from_raw(id))?;
            Ok(Some((rtype, id)))
        }
        ResourceType::StreamSocket => {
            crate::ipc::stream_socket::dup(
                crate::ipc::stream_socket::StreamSocketHandle::from_raw(id),
            )?;
            Ok(Some((rtype, id)))
        }
        // No refcounted same-id dup yet — not inherited.  Documented
        // limitation in todo.txt; revisit when these gain dup support.
        ResourceType::Channel
        | ResourceType::SharedMemory
        | ResourceType::CompletionPort
        | ResourceType::Timer => {
            serial_println!(
                "[fork] Skipping non-inheritable handle: {:?} id={}",
                rtype, id
            );
            Ok(None)
        }
        // Permission tokens / externally managed objects: the child's
        // cloned capability table already carries the authority, and
        // these have no per-process cleanup of their own.  Skip them in
        // the child's IPC-handle list to avoid double-bookkeeping.
        ResourceType::Process
        | ResourceType::Thread
        | ResourceType::PortIo
        | ResourceType::DeviceIrq
        | ResourceType::Socket
        | ResourceType::IoScheduler
        | ResourceType::Service
        | ResourceType::Namespace => Ok(None),
    }
}

/// Close one of the child's duplicated handles (used on rollback and by
/// the normal teardown path via `cleanup_handles`).
fn close_one(rtype: ResourceType, id: u64) {
    match rtype {
        ResourceType::File => {
            let _ = crate::fs::handle::close(id);
        }
        ResourceType::Pipe => {
            crate::ipc::pipe::close(crate::ipc::pipe::PipeHandle::from_raw(id));
        }
        ResourceType::EventFd => {
            crate::ipc::eventfd::close(crate::ipc::eventfd::EventFdHandle::from_raw(id));
        }
        ResourceType::StreamSocket => {
            crate::ipc::stream_socket::close(
                crate::ipc::stream_socket::StreamSocketHandle::from_raw(id),
            );
        }
        // Nothing was duped for these in `dup_one`.
        _ => {}
    }
}

/// Close a list of already-duplicated child handles (rollback helper).
fn close_child_handles(handles: &[(ResourceType, u64)]) {
    for &(rtype, id) in handles {
        close_one(rtype, id);
    }
}

// ---------------------------------------------------------------------------
// Child construction
// ---------------------------------------------------------------------------

/// Build the forked child process (address space + PCB + handles +
/// signal state + namespace + parent capability), but do **not** spawn
/// its thread yet.
///
/// Returns the new child's `ProcessId` on success.  On any failure all
/// partially constructed state (cloned address space, duplicated
/// handles, child PCB) is torn down before returning the error, so the
/// caller never leaks.
///
/// # Errors
///
/// - [`KernelError::InternalError`] if the parent has no address space.
/// - [`KernelError::NoSuchProcess`] if the parent disappears.
/// - Propagates allocation / duplication failures from the clone and
///   handle-dup steps.
fn build_fork_child(parent_pid: ProcessId) -> KernelResult<ProcessId> {
    // 1. Parent address space.
    let parent_pml4 = pcb::get_pml4(parent_pid).ok_or(KernelError::InternalError)?;

    // 2. Snapshot parent's tracked handles (drops the table lock before
    //    we do any blocking dup work).
    let parent_handles =
        pcb::ipc_handles_snapshot(parent_pid).ok_or(KernelError::NoSuchProcess)?;

    // 3. Copy-on-write clone of the address space.
    //
    // SAFETY: `parent_pml4` is a live PML4 owned by `parent_pid`,
    // obtained from the PCB above.  `clone_address_space_cow` only
    // reads the parent tables and allocates new child tables; it does
    // not mutate parent mappings except to mark shared user pages
    // read-only for CoW, which is the intended behavior.
    let child_pml4 = unsafe { crate::mm::cow::clone_address_space_cow(parent_pml4) }?;

    // 4. Refcount-duplicate inheritable handles for the child.
    let mut child_handles: Vec<(ResourceType, u64)> = Vec::new();
    for &(rtype, id) in &parent_handles {
        match dup_one(rtype, id) {
            Ok(Some(entry)) => child_handles.push(entry),
            Ok(None) => {}
            Err(e) => {
                // Roll back: close what we duped, free the address space.
                close_child_handles(&child_handles);
                // SAFETY: `child_pml4` was just produced by
                // `clone_address_space_cow` and is owned by no live
                // process yet (no thread runs in it).
                unsafe {
                    crate::mm::page_table::destroy_user_address_space(child_pml4);
                }
                return Err(e);
            }
        }
    }

    // 5. Create the child PCB (clones name/caps/credentials/VMAs from
    //    the parent and takes ownership of the duplicated handles).
    let child_pid = match pcb::fork_create(
        parent_pid,
        child_pml4,
        child_handles.clone(),
        Vec::new(),
    ) {
        Ok(pid) => pid,
        Err(e) => {
            close_child_handles(&child_handles);
            // SAFETY: see above — child_pml4 is unowned.
            unsafe {
                crate::mm::page_table::destroy_user_address_space(child_pml4);
            }
            return Err(e);
        }
    };

    // 6. Inherit the parent's signal mask / trampoline (pending signals
    //    are *not* inherited, matching POSIX fork semantics).
    crate::proc::signal::inherit_for_fork(parent_pid, child_pid);
    // 6b. Inherit the Linux per-signal sigaction table (separate from
    //     the native signal-shim because the native model has only a
    //     single trampoline pointer per process — the Linux model is
    //     per-signal).
    crate::syscall::linux::linux_sigaction_on_fork(parent_pid, child_pid);

    // 7. Inherit the parent's filesystem namespace (best-effort, like
    //    spawn).  A non-root parent namespace propagates to the child.
    let parent_ns = crate::ipc::namespace::query(parent_pid);
    if parent_ns != crate::ipc::namespace::ROOT_NAMESPACE {
        if let Err(e) = crate::ipc::namespace::attach(child_pid, parent_ns) {
            serial_println!(
                "[fork] Warning: failed to attach child {} to namespace {}: {:?}",
                child_pid, parent_ns, e
            );
        }
    }

    // 8. Grant the parent a Process capability for the child so it can
    //    wait on / signal / inspect it (matches spawn's Step 5b).  PID 0
    //    (kernel) has implicit authority and needs no capability.
    if parent_pid != 0 {
        let rights = Rights::READ
            | Rights::WRITE
            | Rights::DELETE
            | Rights::WAIT
            | Rights::SIGNAL
            | Rights::DUPLICATE;
        if let Err(e) =
            pcb::grant_capability(parent_pid, ResourceType::Process, child_pid, rights)
        {
            serial_println!(
                "[fork] Warning: failed to grant Process cap to parent {}: {:?}",
                parent_pid, e
            );
            // Non-fatal — parent retains implicit parent authority.
        }
    }

    Ok(child_pid)
}

// ---------------------------------------------------------------------------
// Public fork entry
// ---------------------------------------------------------------------------

/// Fork `parent_pid`, returning the new child's PID to the parent.
///
/// `frame` is the parent's saved syscall register frame; it is read
/// (never modified) to build the child's resume image.  The child's
/// single thread is spawned to resume at the parent's post-`fork`
/// instruction with `RAX = 0`.
///
/// # Errors
///
/// Propagates failures from [`build_fork_child`] and from thread
/// spawning.  On thread-spawn failure the child process and its address
/// space are destroyed before returning.
pub fn fork_process(parent_pid: ProcessId, frame: &SyscallFrame) -> KernelResult<ProcessId> {
    let child_pid = build_fork_child(parent_pid)?;

    // Build the child's register image and hand it to the trampoline.
    let regs = build_reg_image(frame);
    let image_raw = Box::into_raw(Box::new(regs)) as u64;

    // Inherit the parent thread's effective scheduling priority so the
    // child runs at a comparable urgency; fall back to the default.
    let priority = crate::sched::get_effective_priority(crate::sched::current_task_id())
        .unwrap_or(crate::sched::task::DEFAULT_PRIORITY);

    match thread::spawn(
        child_pid,
        b"forked",
        priority,
        fork_child_trampoline,
        image_raw,
    ) {
        Ok(_task_id) => Ok(child_pid),
        Err(e) => {
            // The trampoline never ran, so it never freed the image —
            // reclaim and drop it here to avoid a leak.
            //
            // SAFETY: `image_raw` came from `Box::into_raw` just above
            // and was not consumed (spawn failed before the task ran).
            drop(unsafe { Box::from_raw(image_raw as *mut [u64; REG_IMAGE_LEN]) });
            // Tear down the child process (frees its address space and
            // closes its inherited handles via the reaper path).
            pcb::destroy(child_pid);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run fork self-tests.
///
/// These exercise the kernel-side construction path (address-space
/// clone, refcount bump, PCB clone).  End-to-end ring-3 resume of the
/// asm trampoline is validated by the POSIX `fork()` integration test
/// and the QEMU boot test — exercising the IRETQ path from a kernel
/// self-test would require a faulting userspace context and risks
/// triple-faulting the boot if anything is wrong, so it is deliberately
/// kept out of the boot-time self-test.
pub fn self_test() -> KernelResult<()> {
    test_fork_clones_pcb()?;
    Ok(())
}

/// Verify that forking a process clones its PCB, bumps the refcount on a
/// shared user frame, and gives the child a distinct address space that
/// maps the same physical frame copy-on-write.
fn test_fork_clones_pcb() -> KernelResult<()> {
    use crate::mm::frame::{self, PhysFrame};
    use crate::mm::page_table::{self, VirtAddr};

    // Build a parent process that owns a real user address space but has
    // NO scheduler thread.  We deliberately avoid `spawn_process` here:
    // spawn creates a *runnable* ring-3 thread (with a kernel stack and a
    // stack canary) that this test never runs.  Tearing such a thread
    // down via `pcb::destroy` (which frees the address space but does NOT
    // dequeue threads — its SAFETY contract assumes the caller already
    // killed them) left a dangling Ready run-queue entry that triple-
    // faulted on the first preemptive context switch (deterministic boot
    // hang at the APIC-timer self-test).  Worse, boot self-tests run
    // *before* the per-boot stack canary is randomized (main.rs
    // `init_canary`), so a never-run thread's stack carried the fixed
    // fallback canary and tripped a false-positive "stack overflow" halt
    // when the post-preemption reaper checked it against the now-random
    // canary.  The CoW-clone construction path only needs an address
    // space to duplicate, so we create one directly: `pcb::create`
    // allocates a PML4 (kernel half cloned from boot) and
    // `elf::load_segments` maps the test binary's code segment — no task,
    // no kernel stack, no canary, no scheduler involvement, no teardown
    // hazard.
    let parent_pid = pcb::create("fork-test-parent", 0);

    let parent_pml4 = match pcb::get_pml4(parent_pid).filter(|&p| p != 0) {
        Some(p) => p,
        None => {
            pcb::destroy(parent_pid);
            return Err(KernelError::OutOfMemory);
        }
    };

    // Map the canonical test ELF's loadable segment (code at entry
    // 0x0000_0040_0000_0000) into the parent address space.
    let elf_data = super::elf::build_test_elf_public();
    let elf_file = match super::elf::ElfFile::parse(&elf_data) {
        Ok(f) => f,
        Err(e) => {
            pcb::destroy(parent_pid);
            return Err(e);
        }
    };
    // SAFETY: `parent_pml4` is the freshly-created, non-zero PML4 for
    // `parent_pid`; the process has no threads, so no other CPU is using
    // this address space — satisfying `load_segments`' safety contract.
    if let Err(e) = unsafe { super::elf::load_segments(&elf_file, parent_pml4) } {
        pcb::destroy(parent_pid);
        return Err(e);
    }

    // The code frame at the entry virtual address is shared CoW after
    // fork, so its refcount must increase by exactly 1.
    let code_va = VirtAddr::new(0x0000_0040_0000_0000);
    let parent_phys = match page_table::translate(parent_pml4, code_va) {
        Some(p) => p,
        None => {
            pcb::destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    };
    let code_frame = match PhysFrame::from_addr(parent_phys) {
        Some(f) => f,
        None => {
            pcb::destroy(parent_pid);
            return Err(KernelError::InternalError);
        }
    };
    let rc_before = frame::refcount(code_frame);

    // Build the child (no thread spawned — pure construction path).
    let child_pid = match build_fork_child(parent_pid) {
        Ok(pid) => pid,
        Err(e) => {
            pcb::destroy(parent_pid);
            return Err(e);
        }
    };

    let mut failed: Option<&'static str> = None;

    // Refcount must have gone up by exactly one (one extra address space
    // now references the shared frame).
    let rc_after = frame::refcount(code_frame);
    if rc_after != rc_before.saturating_add(1) {
        serial_println!(
            "[fork]   FAIL: refcount {} -> {} (expected +1)",
            rc_before, rc_after
        );
        failed = Some("refcount");
    }

    // Child must have a distinct, non-zero PML4.
    let child_pml4 = pcb::get_pml4(child_pid).unwrap_or(0);
    if child_pml4 == 0 || child_pml4 == parent_pml4 {
        serial_println!(
            "[fork]   FAIL: child pml4 {:#x} invalid (parent {:#x})",
            child_pml4, parent_pml4
        );
        failed = Some("pml4");
    }

    // Child must map the same physical frame (CoW share) at the same VA.
    if failed.is_none() {
        match page_table::translate(child_pml4, code_va) {
            Some(child_phys) if child_phys == parent_phys => {}
            other => {
                serial_println!(
                    "[fork]   FAIL: child maps {:?}, parent {:#x}",
                    other, parent_phys
                );
                failed = Some("mapping");
            }
        }
    }

    // Teardown both processes (frees address spaces, drops refcounts).
    // Neither process has threads — the parent was built without one and
    // build_fork_child never spawns one — so plain `destroy` is exactly
    // what `pcb::destroy`'s no-live-threads contract expects.
    pcb::destroy(child_pid);
    pcb::destroy(parent_pid);

    match failed {
        Some(_) => Err(KernelError::InternalError),
        None => {
            serial_println!("[fork]   Fork clones PCB + CoW address space: OK");
            Ok(())
        }
    }
}
