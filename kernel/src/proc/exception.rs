//! Structured exception handling (SEH-style) for userspace processes.
//!
//! When a hardware exception (divide error, invalid opcode, access violation,
//! etc.) occurs in ring 3, the kernel can deliver it to a user-registered
//! exception handler instead of killing the process.
//!
//! ## Model
//!
//! Each process can register a single exception handler function via
//! `SYS_SET_EXCEPTION_HANDLER(addr)`.  When an exception occurs:
//!
//! 1. Kernel saves the full CPU context (registers, flags, faulting address).
//! 2. Kernel pushes an [`ExceptionRecord`] onto the user stack.
//! 3. Kernel redirects execution to the handler.
//! 4. The handler examines the exception and either:
//!    - Fixes the issue (e.g., guard page commit) and calls
//!      `SYS_EXCEPTION_RETURN(&context)` to resume at the faulting
//!      instruction (or the next one).
//!    - Calls `SYS_EXIT(code)` to terminate the process.
//!
//! If no handler is registered, the process is killed (the default behavior).
//!
//! ## Design Rationale
//!
//! Unlike Unix signals (which interrupt asynchronously and have complex
//! masking/queueing semantics), our exception delivery is synchronous:
//! it only happens at the point of the faulting instruction, and the
//! handler runs on the same thread with a well-defined stack frame.
//! This is much closer to Windows SEH or C++ exceptions, and is
//! easier to reason about for application developers.
//!
//! ## References
//!
//! - Windows SEH: `_EXCEPTION_RECORD`, `RtlDispatchException`
//! - Linux signals: `sigaction`, `sigreturn` (what we're deliberately avoiding)

use crate::proc::pcb::ProcessId;
use crate::serial_println;
use alloc::collections::BTreeMap;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Exception codes — hardware exceptions mapped to stable numeric codes
// ---------------------------------------------------------------------------

/// Exception codes delivered to user exception handlers.
///
/// These are stable ABI values — they never change once defined.
/// Applications match on these values to determine what happened.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionCode {
    /// Integer divide by zero or divide overflow (#DE, vector 0).
    DivideError = 1,
    /// Overflow (INTO instruction, #OF, vector 4).
    Overflow = 2,
    /// BOUND range exceeded (#BR, vector 5).
    BoundRangeExceeded = 3,
    /// Invalid opcode (#UD, vector 6).
    InvalidOpcode = 4,
    /// Segment not present (#NP, vector 11).
    SegmentNotPresent = 5,
    /// Stack-segment fault (#SS, vector 12).
    StackSegmentFault = 6,
    /// General protection fault (#GP, vector 13).
    GeneralProtectionFault = 7,
    /// Access violation — page fault from genuine invalid access
    /// (#PF, vector 14, after demand paging and stack growth are ruled out).
    AccessViolation = 8,
    /// x87 floating-point error (#MF, vector 16).
    FloatingPointError = 9,
    /// Alignment check (#AC, vector 17).
    AlignmentCheck = 10,
    /// SIMD floating-point exception (#XM, vector 19).
    SimdFloatingPoint = 11,
}

// ---------------------------------------------------------------------------
// Exception context — saved CPU state at the point of the fault
// ---------------------------------------------------------------------------

/// Saved CPU context at the point of the exception.
///
/// This is laid out on the user stack by the kernel before dispatching
/// to the exception handler.  The handler can inspect it to understand
/// what happened, and pass it to `SYS_EXCEPTION_RETURN` to resume.
///
/// # ABI
///
/// This struct is part of the userspace ABI.  Fields must not be
/// reordered or resized.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExceptionContext {
    /// Exception code (one of [`ExceptionCode`] values).
    pub code: u64,
    /// Auxiliary data depending on exception type:
    /// - For `AccessViolation`: the faulting virtual address (CR2).
    /// - For `GeneralProtectionFault`: the error code.
    /// - For others: 0.
    pub aux: u64,
    /// Instruction pointer at the point of the fault.
    pub rip: u64,
    /// Stack pointer at the point of the fault.
    pub rsp: u64,
    /// RFLAGS at the point of the fault.
    pub rflags: u64,
    /// General-purpose registers.
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

/// Size of the exception context in bytes.
pub const EXCEPTION_CONTEXT_SIZE: usize = core::mem::size_of::<ExceptionContext>();

// ---------------------------------------------------------------------------
// Per-process exception handler registry
// ---------------------------------------------------------------------------

/// Per-process exception handler address.
///
/// When set, the kernel delivers exceptions to this function instead
/// of killing the process.  The handler receives a pointer to an
/// [`ExceptionContext`] on the stack.
///
/// Signature expected by the kernel:
///
/// ```c
/// void exception_handler(ExceptionContext *ctx);
/// ```
static EXCEPTION_HANDLERS: Mutex<BTreeMap<ProcessId, u64>> =
    Mutex::new(BTreeMap::new());

/// Register an exception handler for a process.
///
/// `handler_addr` is the virtual address of a userspace function.
/// Pass 0 to unregister (revert to default kill-on-exception).
pub fn set_handler(pid: ProcessId, handler_addr: u64) {
    let mut handlers = EXCEPTION_HANDLERS.lock();
    if handler_addr == 0 {
        handlers.remove(&pid);
        serial_println!("[exception] Process {} unregistered exception handler", pid);
    } else {
        handlers.insert(pid, handler_addr);
        serial_println!(
            "[exception] Process {} registered exception handler at {:#x}",
            pid, handler_addr
        );
    }
}

/// Get the exception handler address for a process, if registered.
pub fn get_handler(pid: ProcessId) -> Option<u64> {
    let handlers = EXCEPTION_HANDLERS.lock();
    handlers.get(&pid).copied()
}

/// Remove exception handler registration for a process.
///
/// Called when a process is destroyed.
pub fn remove_handler(pid: ProcessId) {
    let mut handlers = EXCEPTION_HANDLERS.lock();
    handlers.remove(&pid);
}
