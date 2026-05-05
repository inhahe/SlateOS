//! Context switch and task entry trampoline (assembly).
//!
//! ## `switch_context`
//!
//! Saves the current task's callee-saved registers and RFLAGS into
//! `old_ctx`, then restores them from `new_ctx`.  The `ret` at the
//! end pops the return address from the **new** task's stack, resuming
//! execution where that task last called `switch_context` (or, for a
//! new task, jumping to `task_entry_trampoline`).
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

use super::task::Context;

// ---------------------------------------------------------------------------
// switch_context assembly
// ---------------------------------------------------------------------------

global_asm!(
    // fn switch_context(old: &mut Context, new: &Context,
    //                   old_fpu: *mut FpuState, new_fpu: *const FpuState)
    //   rdi = old context pointer
    //   rsi = new context pointer
    //   rdx = old FPU state pointer (16-byte aligned, 512 bytes)
    //   rcx = new FPU state pointer (16-byte aligned, 512 bytes)
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

    // --- Save old task's FPU/SSE state ---
    //
    // fxsave64 saves x87, MMX, and XMM0-XMM15 (512 bytes) to [rdx].
    // This is non-destructive: the FPU registers are unchanged after
    // the save.  We do this FIRST so the FPU state is captured before
    // any potential clobbering by subsequent instructions (though the
    // GPR save instructions don't touch FPU registers).
    "fxsave64 [rdx]",

    // --- Save callee-saved GPRs to old context ---
    "mov [rdi + 0x00], rbx",
    "mov [rdi + 0x08], rbp",
    "mov [rdi + 0x10], r12",
    "mov [rdi + 0x18], r13",
    "mov [rdi + 0x20], r14",
    "mov [rdi + 0x28], r15",
    "mov [rdi + 0x30], rsp",
    // Save RFLAGS — preserves the interrupt flag (IF) per-task.
    // Without this, a task preempted after CLI would leak IF=0
    // into the next task, permanently disabling interrupts on
    // that CPU.
    "pushfq",
    "pop rax",
    "mov [rdi + 0x38], rax",

    // --- Restore callee-saved GPRs from new context ---
    //
    // After this sequence, we're on the NEW task's stack (rsp loaded
    // from new context).  rdi and rsi are caller-saved, so they're
    // stale — but rcx still holds new_fpu (untouched by the restore
    // sequence since we only load rbx, rbp, r12-r15, rsp).
    "mov rbx, [rsi + 0x00]",
    "mov rbp, [rsi + 0x08]",
    "mov r12, [rsi + 0x10]",
    "mov r13, [rsi + 0x18]",
    "mov r14, [rsi + 0x20]",
    "mov r15, [rsi + 0x28]",
    "mov rsp, [rsi + 0x30]",

    // --- Restore new task's FPU/SSE state ---
    //
    // fxrstor64 loads x87, MMX, and XMM0-XMM15 from [rcx].
    // Done BEFORE restoring RFLAGS because popfq may enable interrupts
    // (IF=1).  By restoring FPU first, any interrupt that fires after
    // popfq sees the correct FPU state for the new task.
    //
    // rcx is still valid here: the GPR restore above only writes to
    // rbx, rbp, r12-r15, rsp — it does not touch rcx.
    "fxrstor64 [rcx]",

    // Restore RFLAGS from the target task's saved state.
    // For new tasks, rflags is set to 0x202 (IF=1, reserved bit 1=1)
    // by prepare_context(), ensuring interrupts are enabled when the
    // task first runs.
    //
    // WARNING: After popfq, interrupts may be enabled.  The CPU's
    // one-instruction interrupt deferral after STI does NOT apply to
    // popfq — an interrupt can fire between popfq and ret.  This is
    // safe because the new task's full state (GPRs + FPU) is already
    // restored at this point.
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
    /// Switch CPU context from `old` to `new`, including FPU/SSE state.
    ///
    /// Saves `rbx`, `rbp`, `r12`–`r15`, `rsp`, RFLAGS, and FPU/SSE
    /// (via `fxsave64`) into `old`/`old_fpu`, then restores them from
    /// `new`/`new_fpu` (via `fxrstor64`), and returns (into the new task).
    ///
    /// # Safety
    ///
    /// - Both `old` and `new` must point to valid `Context` structs.
    /// - `old_fpu` must point to a valid, writable, 16-byte-aligned
    ///   512-byte buffer for FXSAVE.
    /// - `new_fpu` must point to a valid, readable, 16-byte-aligned
    ///   512-byte buffer containing a valid FXSAVE image.
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
