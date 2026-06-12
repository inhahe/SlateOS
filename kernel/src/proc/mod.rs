//! Process and thread management.
//!
//! ## Features
//!
//! - Process Control Block (PCB) with per-process address space.
//! - ELF binary loader.
//! - Process creation/destruction (posix_spawn-style, no fork).
//! - Thread creation/destruction.
//! - Per-process capability table.
//! - Hardware exceptions → language-level exceptions (SEH-style).
//! - Structured shutdown via IPC (no Unix signals).
//!
//! ## Design
//!
//! Each process has:
//! - A unique `ProcessId`.
//! - An address space (PML4 page table root).
//! - A capability table (unforgeable handles to kernel objects).
//! - One or more threads (each an entry in the scheduler).
//! - A parent process (for capability inheritance).
//!
//! ## Current Scope
//!
//! This implementation provides:
//! - Process/thread ID types.
//! - Process Control Block structure.
//! - Process table (global registry of all processes).
//! - Basic create/destroy operations.
//! - ELF64 binary parser and segment loader.
//! - Thread management (spawn/exit within a process).
//! - High-level process spawn (posix_spawn-style, no fork).
//!
//! ## Lock Ordering
//!
//! `THREAD_OWNERS` → `PROCESS_TABLE` → `CAP_TABLE` → `SCHED`.

pub mod elf;
pub mod exception;
pub mod fork;
pub mod itimer;
pub mod linux_fd;
pub mod linux_stack;
pub mod pcb;
pub mod signal;
pub mod spawn;
pub mod thread;
pub mod thread_clone;

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run process management self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[proc] Running process management self-test...");

    pcb::self_test()?;
    serial_println!("[proc] Running ELF loader self-test...");
    elf::self_test()?;
    serial_println!("[proc] Running thread management self-test...");
    thread::self_test()?;
    serial_println!("[proc] Running process spawn self-test...");
    spawn::self_test()?;
    serial_println!("[proc] Running exception handling self-test...");
    exception::self_test()?;
    serial_println!("[proc] Running signal-shim self-test...");
    signal::self_test()?;
    serial_println!("[proc] Running ITIMER_REAL self-test...");
    itimer::self_test()?;
    serial_println!("[proc] Running fork self-test...");
    fork::self_test()?;
    serial_println!("[proc] Running Linux fd-table self-test...");
    linux_fd::self_test()?;
    serial_println!("[proc] Running Linux SysV-stack self-test...");
    linux_stack::self_test()?;
    serial_println!("[proc] Running thread-clone self-test...");
    thread_clone::self_test()?;

    serial_println!("[proc] Process management self-test PASSED");
    Ok(())
}
