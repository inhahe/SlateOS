//! Context switch and task entry trampoline (assembly).
//!
//! ## `switch_context`
//!
//! Saves the current task's callee-saved registers into `old_ctx`,
//! then restores them from `new_ctx`.  The `ret` at the end pops the
//! return address from the **new** task's stack, resuming execution
//! where that task last called `switch_context` (or, for a new task,
//! jumping to `task_entry_trampoline`).
//!
//! Only callee-saved registers (`rbx`, `rbp`, `r12`–`r15`, `rsp`)
//! are saved because the caller's compiler-generated code already
//! saves caller-saved registers.  This keeps the context switch fast
//! (7 register saves + 7 restores ≈ 14 memory accesses).
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
    // fn switch_context(old: &mut Context, new: &Context)
    //   rdi = old context pointer
    //   rsi = new context pointer
    //
    // Context layout (must match task.rs Context struct):
    //   offset 0x00: rbx
    //   offset 0x08: rbp
    //   offset 0x10: r12
    //   offset 0x18: r13
    //   offset 0x20: r14
    //   offset 0x28: r15
    //   offset 0x30: rsp
    ".global switch_context",
    "switch_context:",
    // Save callee-saved registers to old context.
    "mov [rdi + 0x00], rbx",
    "mov [rdi + 0x08], rbp",
    "mov [rdi + 0x10], r12",
    "mov [rdi + 0x18], r13",
    "mov [rdi + 0x20], r14",
    "mov [rdi + 0x28], r15",
    "mov [rdi + 0x30], rsp",

    // Restore callee-saved registers from new context.
    "mov rbx, [rsi + 0x00]",
    "mov rbp, [rsi + 0x08]",
    "mov r12, [rsi + 0x10]",
    "mov r13, [rsi + 0x18]",
    "mov r14, [rsi + 0x20]",
    "mov r15, [rsi + 0x28]",
    "mov rsp, [rsi + 0x30]",

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
    /// Switch CPU context from `old` to `new`.
    ///
    /// Saves `rbx`, `rbp`, `r12`–`r15`, `rsp` into `old`, loads them
    /// from `new`, then returns (into the new task).
    ///
    /// # Safety
    ///
    /// - Both `old` and `new` must point to valid `Context` structs.
    /// - `new.rsp` must point to a valid stack with a return address
    ///   at the top (either from a previous `switch_context` call or
    ///   from [`Task::prepare_context`]).
    /// - Interrupts should be disabled around the call to prevent
    ///   preemption during the switch.
    pub fn switch_context(old: &mut Context, new: &Context);

    /// Entry trampoline for newly created tasks.
    ///
    /// Not called directly — its address is placed on the new task's
    /// stack by [`Task::prepare_context`].
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
