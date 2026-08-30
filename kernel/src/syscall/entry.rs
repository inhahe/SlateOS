//! Low-level SYSCALL/SYSRET entry and exit paths.
//!
//! When userspace executes the `syscall` instruction:
//!
//! 1. The CPU loads CS from `IA32_STAR[47:32]` (kernel CS = 0x08).
//! 2. RIP is loaded from `IA32_LSTAR` (our `syscall_entry` stub).
//! 3. RFLAGS is masked with `IA32_FMASK`.  Everything not masked there is
//!    inherited verbatim from ring 3, so we mask the full Linux set — IF, DF,
//!    TF, AC, NT, IOPL, RF, ID and the arithmetic flags.  See `init()`.
//! 4. The old RIP is saved in RCX, old RFLAGS in R11.
//! 5. RSP is NOT changed — we must switch to the kernel stack manually.
//!
//! The assembly stub:
//! - Uses `swapgs` to access the kernel GS base (per-CPU data).
//! - Saves the user RSP and loads the kernel RSP from per-CPU data.
//! - Saves all user registers on the kernel stack.
//! - Calls the Rust dispatcher.
//! - Restores user registers.
//! - Returns to userspace via `sysretq`.
//!
//! ## Register Convention
//!
//! - `rax`: syscall number (input) / return value (output)
//! - `rdi`: arg0
//! - `rsi`: arg1
//! - `rdx`: arg2
//! - `r10`: arg3 (not `rcx` — `rcx` is clobbered by `syscall`)
//! - `r8`:  arg4
//! - `r9`:  arg5
//!
//! ## References
//!
//! - Intel SDM Vol. 2, SYSCALL/SYSRET instructions
//! - AMD APM Vol. 2, "SYSCALL and SYSRET" section

use crate::cpu;
use crate::serial_println;
use core::arch::global_asm;

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// IA32_EFER — Extended Feature Enable Register.
///
/// Bit 0 (SCE) must be set to enable the SYSCALL/SYSRET instructions.
/// Without it, SYSCALL causes #UD even in 64-bit mode.
const IA32_EFER: u32 = 0xC000_0080;

/// IA32_LSTAR — target RIP for SYSCALL.
const IA32_LSTAR: u32 = 0xC000_0082;

/// IA32_FMASK — RFLAGS mask on SYSCALL (bits set here are cleared).
const IA32_FMASK: u32 = 0xC000_0084;

/// IA32_KERNEL_GS_BASE — for SWAPGS to access per-CPU kernel data.
const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

// ---------------------------------------------------------------------------
// Per-CPU kernel data
// ---------------------------------------------------------------------------

/// Storage for kernel/user RSP during SWAPGS-based stack switching.
///
/// On SYSCALL entry, `swapgs` makes this accessible via the GS segment.
/// `[gs:0]` = kernel RSP, `[gs:8]` = scratch for saving user RSP.
///
/// For SMP, this would be one instance per CPU.  For now, single CPU.
#[repr(C, align(16))]
pub struct PerCpuData {
    /// Kernel stack pointer (set by scheduler on context switch).
    pub kernel_rsp: u64,
    /// Scratch storage for the user RSP during syscall processing.
    pub user_rsp: u64,
}

/// Single-CPU per-CPU data.
static mut PER_CPU: PerCpuData = PerCpuData {
    kernel_rsp: 0,
    user_rsp: 0,
};

// ---------------------------------------------------------------------------
// SYSCALL entry assembly
// ---------------------------------------------------------------------------

global_asm!(
    // syscall_entry — low-level SYSCALL handler.
    //
    // CPU state on entry:
    //   RCX = user RIP (return address)
    //   R11 = user RFLAGS
    //   RSP = user RSP (unchanged by SYSCALL!)
    //   CS  = kernel CS (from IA32_STAR[47:32])
    //   IF  = 0, and DF = AC = NT = 0 (all cleared by IA32_FMASK)
    //
    // Syscall arguments in registers:
    //   RAX = syscall number
    //   RDI, RSI, RDX, R10, R8, R9 = args 0..5
    //
    ".global syscall_entry",
    "syscall_entry:",
    // --- Phase 1: Switch to kernel stack ---

    // Swap GS base: user GS ↔ IA32_KERNEL_GS_BASE.
    // After this, GS points to our PerCpuData.
    "swapgs",
    // Save user RSP in per-CPU scratch, load kernel RSP.
    "mov gs:[8], rsp", // per_cpu.user_rsp = user RSP
    "mov rsp, gs:[0]", // rsp = per_cpu.kernel_rsp
    // --- Phase 2: Save user context on kernel stack ---
    //
    // Stack layout (grows down; first push = highest offset):
    //
    //   [rsp + 15*8] = user RIP    (rcx)
    //   [rsp + 14*8] = user RFLAGS (r11)
    //   [rsp + 13*8] = rbp
    //   [rsp + 12*8] = rbx
    //   [rsp + 11*8] = r12
    //   [rsp + 10*8] = r13
    //   [rsp +  9*8] = r14
    //   [rsp +  8*8] = r15
    //   [rsp +  7*8] = arg0  (rdi)
    //   [rsp +  6*8] = arg1  (rsi)
    //   [rsp +  5*8] = arg2  (rdx)
    //   [rsp +  4*8] = arg3  (r10)
    //   [rsp +  3*8] = arg4  (r8)
    //   [rsp +  2*8] = arg5  (r9)
    //   [rsp +  1*8] = syscall_nr (rax)
    //   [rsp +  0*8] = user RSP
    "push rcx", // User RIP
    "push r11", // User RFLAGS
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "push rdi",    // arg0
    "push rsi",    // arg1
    "push rdx",    // arg2
    "push r10",    // arg3
    "push r8",     // arg4
    "push r9",     // arg5
    "push rax",    // syscall number
    "push gs:[8]", // user RSP (from per-CPU scratch)
    // Swap GS back to user's GS base (so kernel code sees normal GS).
    "swapgs",
    // --- Phase 3: Call Rust handler ---

    // Re-enable interrupts now that we're safely on the kernel stack.
    "sti",
    // Call syscall_handler_inner(frame_ptr: *const SyscallFrame).
    "mov rdi, rsp",
    "call syscall_handler_inner",
    // RAX now holds the syscall return value.

    // --- Phase 4: Return to userspace ---

    // Disable interrupts for the SYSRET sequence (we'll manipulate
    // the stack and per-CPU data).
    "cli",
    // Swap to kernel GS for per-CPU data access.
    "swapgs",
    // Save user RSP from the frame into per-CPU scratch.
    // [rsp + 0] = user RSP.
    "mov rdi, [rsp]",
    "mov gs:[8], rdi",
    // Skip user_rsp and syscall_nr (rax already has the return value).
    "add rsp, 16",
    // Restore all registers in reverse order.
    "pop r9",
    "pop r8",
    "pop r10",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "pop r11", // User RFLAGS → R11
    "pop rcx", // User RIP → RCX
    // Restore user RSP from per-CPU scratch.
    "mov rsp, gs:[8]",
    // Swap GS back to user's GS base.
    "swapgs",
    // Return to userspace.
    // SYSRETQ: RIP = RCX, RFLAGS = R11 (with forced bits),
    //          CS = STAR[63:48]+16 (0x20 | 3 = user CS),
    //          SS = STAR[63:48]+8  (0x18 | 3 = user DS).
    "sysretq",
);

// Import the assembly symbol.
unsafe extern "C" {
    fn syscall_entry();
}

// ---------------------------------------------------------------------------
// Stack frame layout
// ---------------------------------------------------------------------------

/// The register frame pushed by `syscall_entry`.
///
/// Matches the push order in the assembly above.
#[repr(C)]
pub struct SyscallFrame {
    /// User stack pointer.
    pub user_rsp: u64,
    /// Syscall number (from rax).
    pub syscall_nr: u64,
    /// Arg5 (r9).
    pub arg5: u64,
    /// Arg4 (r8).
    pub arg4: u64,
    /// Arg3 (r10).
    pub arg3: u64,
    /// Arg2 (rdx).
    pub arg2: u64,
    /// Arg1 (rsi).
    pub arg1: u64,
    /// Arg0 (rdi).
    pub arg0: u64,
    /// Saved r15.
    pub r15: u64,
    /// Saved r14.
    pub r14: u64,
    /// Saved r13.
    pub r13: u64,
    /// Saved r12.
    pub r12: u64,
    /// Saved rbx.
    pub rbx: u64,
    /// Saved rbp.
    pub rbp: u64,
    /// User RFLAGS (from r11).
    pub user_rflags: u64,
    /// User RIP (from rcx).
    pub user_rip: u64,
}

// ---------------------------------------------------------------------------
// Sanitising an attacker-supplied return state
// ---------------------------------------------------------------------------
//
// Several syscalls rewrite the SYSRET frame from a register snapshot that
// lives on the *user* stack: `SYS_EXCEPTION_RETURN`, `SYS_SIGNAL_RETURN` and
// the Linux-ABI `rt_sigreturn`/`sigreturn`.  Userspace can put anything in
// those snapshots, so the RIP, RSP and RFLAGS they carry are attacker-
// controlled and must be filtered here rather than trusted.

/// RFLAGS bits userspace is permitted to restore through a frame-rewriting
/// return.
///
/// Kept: CF(0) PF(2) AF(4) ZF(6) SF(7) TF(8) DF(10) OF(11) AC(18) ID(21).
/// TF is permitted so a debugger-driven single-step survives a signal
/// round-trip, matching Linux.
///
/// Dropped: IOPL(12,13) — would grant ring 3 direct I/O-port access; IF(9) —
/// forced on below, since resuming ring 3 with interrupts disabled wedges the
/// CPU; NT(14), RF(16), VM(17), VIF(19), VIP(20); and every reserved bit.
pub const USER_RFLAGS_MASK: u64 = 0x0024_0DD5;

/// RFLAGS bits always forced on: IF (interrupts enabled) plus the mandatory
/// reserved bit 1.
pub const USER_RFLAGS_FORCED: u64 = 0x0000_0202;

/// Reduce an attacker-supplied RFLAGS value to one that is safe to load on
/// return to ring 3.
#[must_use]
pub const fn sanitize_user_rflags(raw: u64) -> u64 {
    (raw & USER_RFLAGS_MASK) | USER_RFLAGS_FORCED
}

/// Check that an attacker-supplied `(rip, rsp)` pair is safe to install in the
/// SYSRET frame.
///
/// Both must lie in the user half.  The RIP check is not merely hygiene: on
/// `sysretq` the CPU loads RIP from RCX *while still at CPL 0*, and a
/// non-canonical value raises `#GP` in ring 0 — after this stub has already
/// switched RSP to the user stack and run `swapgs`, so the fault handler would
/// run with kernel privilege on an attacker-controlled stack and an
/// attacker-controlled GS base.  That is the CVE-2012-0217 shape.  Requiring
/// `rip < USER_SPACE_END` is strictly stronger than "canonical" and is what a
/// legitimate user context always satisfies.
#[must_use]
pub fn user_return_state_ok(rip: u64, rsp: u64) -> bool {
    use crate::mm::page_table::USER_SPACE_END;
    rip < USER_SPACE_END && rsp < USER_SPACE_END
}

// ---------------------------------------------------------------------------
// Rust-level syscall handler
// ---------------------------------------------------------------------------

/// Rust-level syscall handler called from the assembly entry stub.
///
/// Receives a pointer to the saved register frame on the kernel stack.
/// Returns the syscall result value in RAX.
///
/// The frame is mutable because certain syscalls (notably `SYS_PROCESS_EXEC`)
/// need to modify the saved user RIP and RSP so that the SYSRET path
/// returns to a different location (the new binary's entry point).
#[unsafe(no_mangle)]
extern "C" fn syscall_handler_inner(frame: *mut SyscallFrame) -> i64 {
    // SAFETY: frame points to valid data on the current kernel stack,
    // pushed by our assembly stub moments ago.  No other code accesses
    // the frame concurrently (single CPU, interrupts re-enabled after
    // this read).
    let f = unsafe { &mut *frame };

    // Check for syscalls that need to modify the frame directly
    // (they change RIP/RSP rather than just returning a value).
    if f.syscall_nr == super::number::SYS_PROCESS_EXEC {
        return super::handlers::sys_process_exec_with_frame(f);
    }
    if f.syscall_nr == super::number::SYS_PROCESS_FORK {
        // fork only *reads* the parent frame to snapshot the child's
        // resume state; the parent returns the child PID normally.
        return super::handlers::sys_process_fork_with_frame(f);
    }
    if f.syscall_nr == super::number::SYS_EXCEPTION_RETURN {
        return super::handlers::sys_exception_return_with_frame(f);
    }
    if f.syscall_nr == super::number::SYS_SIGNAL_RETURN {
        // sigreturn restores the interrupted frame. After restoring, a
        // *different* pending signal may still need delivery, so fall
        // through to the delivery check below using the restored frame.
        let restored_rax = super::handlers::sys_signal_return_with_frame(f);
        if super::handlers::deliver_pending_signal(f, restored_rax) {
            // Frame now points at the trampoline; the return value is
            // unused (trampoline gets args via the frame's registers).
            return 0;
        }
        return restored_rax;
    }

    // Build SyscallArgs from the frame and dispatch.
    let args = super::dispatch::SyscallArgs {
        arg0: f.arg0,
        arg1: f.arg1,
        arg2: f.arg2,
        arg3: f.arg3,
        arg4: f.arg4,
        arg5: f.arg5,
    };

    // ABI-mode routing: a process loaded as a Linux binary runs against
    // the Linux x86_64 syscall numbering and -errno return convention.
    // Route those calls through the translation layer instead of the
    // native dispatch table.  See `kernel/src/syscall/linux.rs` for the
    // full set of translated syscalls and their semantics.
    //
    // Linux frame-modifying syscalls (fork / vfork / clone / execve)
    // are dispatched via `dispatch_linux_with_frame` BEFORE the regular
    // value-returning dispatcher gets a chance, because they need to
    // mutate the saved register frame (clone snapshots the parent
    // frame for the child trampoline; execve rewrites user_rip/rsp).
    // Returns `None` for any other syscall number and we fall through.
    let abi_mode = crate::proc::thread::owner_process(crate::sched::current_task_id())
        .and_then(crate::proc::pcb::get_abi_mode)
        .unwrap_or(crate::proc::pcb::AbiMode::Native);

    if abi_mode == crate::proc::pcb::AbiMode::Linux {
        if let Some(rax) = super::linux::dispatch_linux_with_frame(f) {
            // Same signal-delivery hook as the regular return path —
            // ensures pending signals can interrupt around an execve
            // boundary, exactly as in native sys_process_exec_with_frame.
            if super::handlers::deliver_pending_signal(f, rax) {
                return 0;
            }
            return rax;
        }
    }

    let result = match abi_mode {
        crate::proc::pcb::AbiMode::Native => super::dispatch::dispatch(f.syscall_nr, &args),
        crate::proc::pcb::AbiMode::Linux => super::linux::dispatch_linux(f.syscall_nr, &args),
    };

    // On the way back to userspace, deliver a pending signal if one is
    // ready and the process registered a trampoline. This rewrites the
    // frame to jump to the handler; the interrupted syscall's return
    // value (result.value) is preserved in the SignalContext for
    // SYS_SIGNAL_RETURN to restore.  When a handler frame is built the
    // restart sentinel (if any) is already resolved inside the delivery
    // path (rewind-to-restart or convert-to-EINTR baked into the saved
    // context), so we just return.
    if super::handlers::deliver_pending_signal(f, result.value) {
        return 0;
    }

    // No handler frame was built (no deliverable signal, or every pending
    // signal was ignored / had a non-fatal default).  This is the "no handler"
    // case of SA_RESTART: if the syscall returned a restart sentinel, restart
    // it transparently here (rewind RIP to the `syscall` instruction and reload
    // RAX with the original syscall number — the SYSRET path below reloads the
    // original argument registers from the frame).  Any sentinel that is not
    // restarted is converted to -EINTR so it can never leak to ring 3.
    let rax = super::linux::resolve_syscall_restart(f, result.value);

    // Deliver a second return value in RDX for two-value syscalls
    // (e.g. SYS_PIPE_CREATE / SYS_CHANNEL_CREATE, which return a pair of
    // handles).  The exit stub restores RDX from `f.arg2`, so overwriting
    // that slot places `value2` into the user's RDX.  We do this *after*
    // `resolve_syscall_restart` so a restart (which rewinds RIP and needs
    // the original RDX arg intact) is never corrupted — but `has_value2`
    // is only ever set by `ok2`, a success path that is never a restart
    // sentinel, so the two never collide in practice.  Single-value
    // syscalls leave `f.arg2` untouched, preserving the user's RDX across
    // the call as the SysV/musl calling convention requires.
    if result.has_value2 {
        #[allow(clippy::cast_sign_loss)]
        {
            f.arg2 = result.value2 as u64;
        }
    }

    rax
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Set up the SYSCALL/SYSRET MSRs.
///
/// Configures:
/// - `IA32_LSTAR` — syscall entry point
/// - `IA32_FMASK` — RFLAGS mask (clears IF, DF, TF, AC, NT, IOPL, RF, ID and
///   the arithmetic flags on entry)
/// - `IA32_KERNEL_GS_BASE` — per-CPU data for SWAPGS
///
/// `IA32_STAR` (segment selectors) is configured in `gdt::init()`.
///
/// # Safety
///
/// Must be called during boot after GDT is loaded.
pub unsafe fn init() {
    // Enable SYSCALL/SYSRET by setting IA32_EFER.SCE (bit 0).
    //
    // The bootloader sets LME (bit 8) and LMA (bit 10) for long mode,
    // but SCE is not set by default.  Without it, SYSCALL from ring 3
    // causes #UD.
    // SAFETY: IA32_EFER is valid on all x86_64 CPUs; setting SCE is required for SYSCALL.
    unsafe {
        let efer = cpu::rdmsr(IA32_EFER);
        cpu::wrmsr(IA32_EFER, efer | 1); // Set SCE (bit 0).
    }

    // Set LSTAR — the RIP loaded on SYSCALL.
    let entry_addr = syscall_entry as *const () as u64;
    // SAFETY: IA32_LSTAR is a valid MSR on all x86_64 CPUs.
    unsafe {
        cpu::wrmsr(IA32_LSTAR, entry_addr);
    }

    // Set FMASK — bits to clear in RFLAGS on SYSCALL entry.
    //
    // SYSCALL copies RFLAGS from ring 3 verbatim except for the bits set here,
    // so every bit we leave unmasked is ring-3-controlled kernel entry state.
    // We therefore mask the same set Linux does in `syscall_init()`
    // (arch/x86/kernel/cpu/common.c, MSR_SYSCALL_MASK) rather than the
    // minimum that happens to work today:
    //
    //   TF   (8)      single-step — prevent tracing into the kernel.
    //   IF   (9)      interrupts — off until we are on the kernel stack.
    //   DF   (10)     direction — `rep movsb`/`rep stosb` must run forwards.
    //                 See the "`cld` on entry" comment in idt.rs: with DF = 1 a
    //                 compiler-emitted memset writes [dst-n, dst) instead.
    //   AC   (18)     alignment check — doubles as the SMAP override. A user
    //                 process can set AC with `popfq`; if it survived into the
    //                 kernel, every SMAP check would be suppressed for the whole
    //                 syscall. Masking it now means SMAP cannot be silently
    //                 defeated on this path when it is eventually enabled
    //                 (see B-AC-INHERITED-AT-KERNEL-ENTRY in known-issues.md).
    //   NT   (14)     nested task — a stray NT makes a later `iret` attempt a
    //                 task-switch return through the TSS link field.
    //   IOPL (12,13)  I/O privilege level — never meaningful for kernel code.
    //   RF   (16)     resume flag — would suppress the next instruction breakpoint.
    //   ID   (21)     CPUID-available flag — no reason to inherit.
    //   CF PF AF ZF SF OF (0,2,4,6,7,11)
    //                 arithmetic flags — masked so the kernel never begins a
    //                 syscall with attacker-chosen condition codes, and so the
    //                 entry state is deterministic for debugging.
    let fmask: u64 = (1 << 0)   // CF
        | (1 << 2)              // PF
        | (1 << 4)              // AF
        | (1 << 6)              // ZF
        | (1 << 7)              // SF
        | (1 << 8)              // TF
        | (1 << 9)              // IF
        | (1 << 10)             // DF
        | (1 << 11)             // OF
        | (0b11 << 12)          // IOPL
        | (1 << 14)             // NT
        | (1 << 16)             // RF
        | (1 << 18)             // AC
        | (1 << 21); // ID
    // SAFETY: IA32_FMASK is a valid MSR on all x86_64 CPUs with SCE, and every
    // bit set above is a defined, maskable RFLAGS bit (reserved bits are left
    // clear).  Masking a flag can only clear it on entry, never set it.
    unsafe {
        cpu::wrmsr(IA32_FMASK, fmask);
    }

    // Set up per-CPU data for SWAPGS.
    let per_cpu_addr = core::ptr::addr_of!(PER_CPU) as u64;
    // SAFETY: IA32_KERNEL_GS_BASE sets the base for SWAPGS; per_cpu_addr is a valid static.
    unsafe {
        cpu::wrmsr(IA32_KERNEL_GS_BASE, per_cpu_addr);
    }

    serial_println!(
        "[syscall] LSTAR={:#x}, FMASK={:#x}, KERNEL_GS_BASE={:#x}, EFER.SCE=1",
        entry_addr,
        fmask,
        per_cpu_addr
    );
}

/// Update the kernel stack pointer in the per-CPU data.
///
/// Called by the scheduler on context switch so that SYSCALL entry
/// uses the correct kernel stack for the new task.
///
/// # Safety
///
/// Must be called with interrupts disabled (during context switch).
pub unsafe fn set_kernel_stack(stack_top: u64) {
    // SAFETY: Single-CPU, called during context switch with interrupts
    // disabled.  No concurrent access.
    unsafe {
        (*core::ptr::addr_of_mut!(PER_CPU)).kernel_rsp = stack_top;
    }
}
