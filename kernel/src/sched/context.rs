//! Context switch and task entry trampoline (assembly).
//!
//! ## `switch_context`
//!
//! Saves the current task's callee-saved registers, RFLAGS, and FPU/SSE/AVX
//! state into `old_ctx`/`old_fpu`, then restores them from
//! `new_ctx`/`new_fpu`.  The `ret` at the end pops the return address
//! from the **new** task's stack, resuming execution where that task last
//! called `switch_context` (or, for a new task, jumping to
//! `task_entry_trampoline`).
//!
//! ### FPU Strategy
//!
//! FPU/SSE/AVX save/restore dispatches at runtime based on a global
//! strategy byte set during boot:
//!
//! - Strategy 2 (XSAVEOPT64): only saves modified state components
//! - Strategy 1 (XSAVE64): saves all enabled components
//! - Strategy 0 (FXSAVE64): legacy x87 + SSE only
//!
//! The assembly reads `FPU_STRATEGY` and branches accordingly.  The
//! mask for XSAVE/XSAVEOPT is read from `XSAVE_MASK_LO`/`XSAVE_MASK_HI`.
//!
//! Callee-saved registers (`rbx`, `rbp`, `r12`–`r15`, `rsp`) plus
//! RFLAGS are saved.  Caller-saved registers are already handled by
//! the compiler's calling convention.
//!
//! ### Why RFLAGS?
//!
//! The interrupt flag (IF) in RFLAGS must be preserved per-task.
//! Without it, a task preempted while IF=0 (e.g., after softirq's
//! CLI) would context-switch to another task that then runs with
//! interrupts permanently disabled — losing timer ticks and device
//! IRQ delivery on that CPU.
//!
//! ## `task_entry_trampoline`
//!
//! For newly created tasks, `switch_context`'s `ret` pops the
//! trampoline address (placed on the stack by [`Task::prepare_context`]).
//! The trampoline reads the entry function from `rbx` and the argument
//! from `r12` (loaded by `switch_context` from the new context), calls
//! the entry function, then calls [`task_finished`] when it returns.
//!
//! [`Task::prepare_context`]: super::task::Task::prepare_context

use core::arch::global_asm;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use super::task::Context;

// ---------------------------------------------------------------------------
// Strategy dispatch variables (written once during boot, read by assembly)
// ---------------------------------------------------------------------------

/// FPU save strategy used by the context switch assembly.
///
/// - 0 = FXSAVE64/FXRSTOR64
/// - 1 = XSAVE64/XRSTOR64
/// - 2 = XSAVEOPT64/XRSTOR64
///
/// Set by `fpu::init_bsp()` during boot.  Read by the assembly on
/// every context switch.
#[unsafe(no_mangle)]
pub static FPU_STRATEGY: AtomicU8 = AtomicU8::new(0);

/// Low 32 bits of the XSAVE state-component bitmap (EDX:EAX mask).
/// Set by `fpu::init_bsp()`.
#[unsafe(no_mangle)]
pub static XSAVE_MASK_LO: AtomicU32 = AtomicU32::new(0x3); // x87 + SSE

/// High 32 bits of the XSAVE state-component bitmap.
/// Set by `fpu::init_bsp()`.
#[unsafe(no_mangle)]
pub static XSAVE_MASK_HI: AtomicU32 = AtomicU32::new(0);

/// Set the FPU strategy and mask (called from `fpu::init_bsp()`).
pub fn set_fpu_strategy(strategy: u8, mask_lo: u32, mask_hi: u32) {
    XSAVE_MASK_LO.store(mask_lo, Ordering::Release);
    XSAVE_MASK_HI.store(mask_hi, Ordering::Release);
    FPU_STRATEGY.store(strategy, Ordering::Release);
}

// ---------------------------------------------------------------------------
// switch_context assembly
// ---------------------------------------------------------------------------

global_asm!(
    // fn switch_context(old: &mut Context, new: &Context,
    //                   old_fpu: *mut FpuState, new_fpu: *const FpuState)
    //   rdi = old context pointer
    //   rsi = new context pointer
    //   rdx = old FPU state pointer (64-byte aligned)
    //   rcx = new FPU state pointer (64-byte aligned)
    //
    // Context layout (must match task.rs Context struct):
    //   offset 0x00: rbx
    //   offset 0x08: rbp
    //   offset 0x10: r12
    //   offset 0x18: r13
    //   offset 0x20: r14
    //   offset 0x28: r15
    //   offset 0x30: rsp
    //   offset 0x38: rflags
    ".global switch_context",
    "switch_context:",

    // --- Save old task's FPU/SSE/AVX state ---
    //
    // Dispatch based on FPU_STRATEGY global:
    //   0 → fxsave64, 1 → xsave64, 2 → xsaveopt64
    "movzx eax, byte ptr [rip + FPU_STRATEGY]",
    "cmp al, 2",
    "je 2f",
    "cmp al, 1",
    "je 1f",
    // Strategy 0: FXSAVE64 (legacy, 512 bytes)
    "fxsave64 [rdx]",
    "jmp 3f",
    "1:",
    // Strategy 1: XSAVE64
    "push rdx",       // save old_fpu pointer
    "push rcx",       // save new_fpu pointer
    "mov r8, rdx",    // save fpu pointer in r8
    "mov eax, dword ptr [rip + XSAVE_MASK_LO]",
    "mov edx, dword ptr [rip + XSAVE_MASK_HI]",
    "xsave64 [r8]",
    "pop rcx",
    "pop rdx",
    "jmp 3f",
    "2:",
    // Strategy 2: XSAVEOPT64
    "push rdx",
    "push rcx",
    "mov r8, rdx",
    "mov eax, dword ptr [rip + XSAVE_MASK_LO]",
    "mov edx, dword ptr [rip + XSAVE_MASK_HI]",
    "xsaveopt64 [r8]",
    "pop rcx",
    "pop rdx",
    "3:",

    // --- Save callee-saved GPRs to old context ---
    "mov [rdi + 0x00], rbx",
    "mov [rdi + 0x08], rbp",
    "mov [rdi + 0x10], r12",
    "mov [rdi + 0x18], r13",
    "mov [rdi + 0x20], r14",
    "mov [rdi + 0x28], r15",
    "mov [rdi + 0x30], rsp",
    // Save RFLAGS — preserves the interrupt flag (IF) per-task.
    "pushfq",
    "pop rax",
    "mov [rdi + 0x38], rax",

    // --- Restore callee-saved GPRs from new context ---
    //
    // After this sequence, we're on the NEW task's stack (rsp loaded
    // from new context).  rcx still holds new_fpu (untouched by the
    // GPR restore since we only load rbx, rbp, r12-r15, rsp).
    "mov rbx, [rsi + 0x00]",
    "mov rbp, [rsi + 0x08]",
    "mov r12, [rsi + 0x10]",
    "mov r13, [rsi + 0x18]",
    "mov r14, [rsi + 0x20]",
    "mov r15, [rsi + 0x28]",
    "mov rsp, [rsi + 0x30]",

    // --- Restore new task's FPU/SSE/AVX state ---
    //
    // Dispatch based on strategy (same as save, but always use XRSTOR
    // for strategies 1 and 2 — there's no xrstoropt).
    "movzx eax, byte ptr [rip + FPU_STRATEGY]",
    "cmp al, 0",
    "je 4f",
    // Strategy 1 or 2: XRSTOR64
    "mov r8, rcx",    // save new_fpu in r8 (rcx clobbered by mask load)
    "mov eax, dword ptr [rip + XSAVE_MASK_LO]",
    "mov edx, dword ptr [rip + XSAVE_MASK_HI]",
    "xrstor64 [r8]",
    "jmp 5f",
    "4:",
    // Strategy 0: FXRSTOR64
    "fxrstor64 [rcx]",
    "5:",

    // Restore RFLAGS from the target task's saved state.
    "mov rax, [rsi + 0x38]",
    "push rax",
    "popfq",

    // Return.  For an existing task, this returns to where it last
    // called switch_context.  For a new task, this pops the
    // trampoline address from the stack and jumps there.
    "ret",
);

global_asm!(
    // task_entry_trampoline — first code a new task executes.
    //
    // After switch_context restores registers and does `ret`:
    //   rbx = entry function pointer
    //   r12 = argument (u64)
    //
    // We set up the argument in rdi (System V ABI first parameter)
    // and call the entry function.  When it returns, we call
    // task_finished to mark the task as dead and yield.
    ".global task_entry_trampoline",
    "task_entry_trampoline:",
    "mov rdi, r12",       // arg → first parameter
    "call rbx",           // call entry(arg)
    "call task_finished", // entry returned — clean up
    "ud2",                // unreachable (task_finished never returns)
);

// Import the assembly symbols so Rust can reference them.
unsafe extern "C" {
    /// Switch CPU context from `old` to `new`, including FPU/SSE/AVX state.
    ///
    /// Saves `rbx`, `rbp`, `r12`–`r15`, `rsp`, RFLAGS, and FPU/SSE/AVX
    /// (via XSAVEOPT/XSAVE/FXSAVE) into `old`/`old_fpu`, then restores
    /// from `new`/`new_fpu`, and returns (into the new task).
    ///
    /// # Safety
    ///
    /// - Both `old` and `new` must point to valid `Context` structs.
    /// - `old_fpu` must point to a valid, writable, 64-byte-aligned
    ///   buffer for XSAVE/FXSAVE (size = `fpu::xsave_area_size()`).
    /// - `new_fpu` must point to a valid, readable, 64-byte-aligned
    ///   buffer containing a valid XSAVE/FXSAVE image.
    /// - `new.rsp` must point to a valid stack with a return address
    ///   at the top (either from a previous `switch_context` call or
    ///   from [`Task::prepare_context`]).
    /// - Interrupts should be disabled around the call to prevent
    ///   preemption during the switch.
    pub fn switch_context(
        old: &mut Context,
        new: &Context,
        old_fpu: *mut super::fpu::FpuState,
        new_fpu: *const super::fpu::FpuState,
    );

    /// Entry trampoline for newly created tasks.
    ///
    /// Not called directly — its address is placed on the new task's
    /// stack by [`Task::prepare_context`].
    #[allow(dead_code)] // Referenced by assembly; address taken for stack setup.
    pub fn task_entry_trampoline();
}

// ---------------------------------------------------------------------------
// task_finished — called when a task's entry function returns
// ---------------------------------------------------------------------------

/// Called by `task_entry_trampoline` when a task's entry function
/// returns.  Marks the current task as dead and yields to the
/// scheduler.  Never returns.
///
/// This is `#[unsafe(no_mangle)]` so the assembly trampoline can call
/// it by name.
#[unsafe(no_mangle)]
extern "C" fn task_finished() -> ! {
    super::task_exit();
    // task_exit halts if it somehow returns (it shouldn't).
    crate::cpu::halt_loop();
}
