//! Interrupt Descriptor Table (IDT) setup and exception handling.
//!
//! The IDT maps interrupt vectors (0--255) to handler functions.  The
//! first 32 vectors are CPU exceptions; the rest are available for
//! hardware IRQs and software interrupts.
//!
//! ## Design
//!
//! - Each exception gets a dedicated assembly stub that saves all
//!   registers, calls a Rust handler, restores registers, and executes
//!   `iretq`.
//! - Assembly stubs are generated via `global_asm!` (stable Rust --- no
//!   nightly features required).
//! - The double-fault handler (#8) uses `IST1` (a separate stack) so it
//!   can fire even if the kernel stack itself overflowed.
//! - IRQ handlers (vectors 32+) will be wired up when the APIC driver
//!   is initialized.

use core::arch::global_asm;
use core::ptr::addr_of;
use core::ptr::addr_of_mut;

use crate::cpu;
use crate::gdt;
use crate::mm;
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::proc::spawn::{USER_STACK_TOP, USER_STACK_GUARD};
use crate::sched;
use crate::serial_println;

// ---------------------------------------------------------------------------
// IDT entry
// ---------------------------------------------------------------------------

/// Number of entries in the IDT (full `x86_64` range).
const IDT_ENTRIES: usize = 256;

/// A single 64-bit IDT gate descriptor (16 bytes).
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct IdtEntry {
    /// Handler address bits [15:0].
    offset_low: u16,
    /// Code segment selector.
    selector: u16,
    /// IST index (bits [2:0]) — 0 means no IST.
    ist: u8,
    /// Type and attributes.
    ///   bit 7:    Present
    ///   bits 6-5: DPL
    ///   bit 4:    0 (must be zero for interrupt/trap gate)
    ///   bits 3-0: type (0xE = 64-bit interrupt gate, 0xF = 64-bit trap gate)
    type_attr: u8,
    /// Handler address bits [31:16].
    offset_mid: u16,
    /// Handler address bits [63:32].
    offset_high: u32,
    /// Reserved, must be zero.
    _reserved: u32,
}

impl IdtEntry {
    /// An empty (not-present) IDT entry.
    const EMPTY: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        type_attr: 0,
        offset_mid: 0,
        offset_high: 0,
        _reserved: 0,
    };

    /// Create an interrupt gate entry pointing to `handler`.
    ///
    /// - `selector`: code segment selector (always kernel CS).
    /// - `ist`: IST index (0 = no IST, 1--7 = use that IST stack).
    /// - `dpl`: descriptor privilege level (0 for kernel-only, 3 for
    ///   user-callable via `int` instruction).
    // The casts here intentionally extract 16-bit and 32-bit slices from a
    // 64-bit virtual address to fill the IDT gate's split offset fields.
    // Truncation is the desired behaviour.
    #[allow(clippy::cast_possible_truncation)]
    fn new(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            // Mask to IST field width (bits [2:0]).
            ist: ist & 0b111,
            // Present | DPL | Interrupt Gate (type 0xE)
            type_attr: 0x80 | ((dpl & 3) << 5) | 0x0E,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _reserved: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// IDT pointer (for `lidt`)
// ---------------------------------------------------------------------------

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

// ---------------------------------------------------------------------------
// The IDT itself
// ---------------------------------------------------------------------------

/// The IDT, aligned to 16 bytes per Intel recommendation.
#[repr(C, align(16))]
struct Idt {
    entries: [IdtEntry; IDT_ENTRIES],
}

static mut IDT: Idt = Idt {
    entries: [IdtEntry::EMPTY; IDT_ENTRIES],
};

// ---------------------------------------------------------------------------
// Interrupt stack frame
// ---------------------------------------------------------------------------

/// The stack frame pushed by the CPU when taking an interrupt in 64-bit
/// mode.  Interrupt handlers receive a pointer to this.
#[derive(Debug)]
#[repr(C)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// ---------------------------------------------------------------------------
// Assembly stubs via global_asm!
//
// Each stub:
//   1. Pushes a dummy error code (if the CPU didn't push one)
//   2. Saves all 15 general-purpose registers
//   3. Loads RDI = pointer to InterruptStackFrame, RSI = error code
//   4. Calls the Rust handler
//   5. Restores all registers
//   6. Pops the error code
//   7. Returns via `iretq`
//
// The Rust handlers are #[unsafe(no_mangle)] extern "C" so the assembler can
// reference them by name.
// ---------------------------------------------------------------------------

/// Generate an assembly stub for an exception WITHOUT a CPU error code.
macro_rules! isr_stub_no_error {
    ($stub:ident, $handler:ident) => {
        global_asm!(
            concat!(".global ", stringify!($stub)),
            concat!(stringify!($stub), ":"),
            "push 0",              // dummy error code
            "push rax",
            "push rcx",
            "push rdx",
            "push rbx",
            "push rbp",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "lea rdi, [rsp + 128]", // 16 pushes × 8 bytes = 128
            "xor esi, esi",         // error code = 0
            concat!("call ", stringify!($handler)),
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rbx",
            "pop rdx",
            "pop rcx",
            "pop rax",
            "add rsp, 8",          // pop dummy error code
            "iretq",
        );
        unsafe extern "C" { fn $stub(); }
    };
}

/// Generate an assembly stub for an exception WITH a CPU error code.
macro_rules! isr_stub_with_error {
    ($stub:ident, $handler:ident) => {
        global_asm!(
            concat!(".global ", stringify!($stub)),
            concat!(stringify!($stub), ":"),
            // CPU already pushed error code.
            "push rax",
            "push rcx",
            "push rdx",
            "push rbx",
            "push rbp",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "lea rdi, [rsp + 128]", // frame is at 15 GPRs + error code = 128 bytes
            "mov rsi, [rsp + 120]", // error code is at 15 GPRs × 8 = 120
            concat!("call ", stringify!($handler)),
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rbx",
            "pop rdx",
            "pop rcx",
            "pop rax",
            "add rsp, 8",          // pop error code
            "iretq",
        );
        unsafe extern "C" { fn $stub(); }
    };
}

// Generate all exception stubs.
isr_stub_no_error!(isr_divide_error, handle_divide_error);
isr_stub_no_error!(isr_debug, handle_debug);
isr_stub_no_error!(isr_nmi, handle_nmi);
isr_stub_no_error!(isr_breakpoint, handle_breakpoint);
isr_stub_no_error!(isr_overflow, handle_overflow);
isr_stub_no_error!(isr_bound_range, handle_bound_range);
isr_stub_no_error!(isr_invalid_opcode, handle_invalid_opcode);
isr_stub_no_error!(isr_device_not_avail, handle_device_not_avail);
isr_stub_with_error!(isr_double_fault, handle_double_fault);
isr_stub_with_error!(isr_invalid_tss, handle_invalid_tss);
isr_stub_with_error!(isr_seg_not_present, handle_seg_not_present);
isr_stub_with_error!(isr_stack_segment, handle_stack_segment);
isr_stub_with_error!(isr_general_protection, handle_general_protection);
isr_stub_with_error!(isr_page_fault, handle_page_fault);
isr_stub_no_error!(isr_x87_fp, handle_x87_fp);
isr_stub_with_error!(isr_alignment_check, handle_alignment_check);
isr_stub_no_error!(isr_machine_check, handle_machine_check);
isr_stub_no_error!(isr_simd_fp, handle_simd_fp);

// Default handler for unregistered vectors.
isr_stub_no_error!(isr_default, handle_default);

// Hardware IRQ handlers (vectors 32+).
// Timer (vector 32) — driven by the Local APIC timer.
isr_stub_no_error!(isr_timer, handle_timer_irq);
// Spurious (vector 255) — APIC spurious interrupts.
isr_stub_no_error!(isr_spurious, handle_spurious_irq);

// ---------------------------------------------------------------------------
// External device IRQ stubs (IOAPIC inputs 0–23 → vectors 33–56)
//
// Each stub saves all registers, passes the IRQ number in EDI (first
// argument, System V ABI), calls the common `handle_device_irq` handler
// in ioapic.rs, restores registers, and returns via IRETQ.
// ---------------------------------------------------------------------------

/// Generate an assembly stub for an external device IRQ.
///
/// The stub passes `$irq` (the IOAPIC input number) to the Rust
/// handler `handle_device_irq(irq: u32)` defined in `ioapic.rs`.
macro_rules! isr_irq_stub {
    ($stub:ident, $irq:literal) => {
        global_asm!(
            concat!(".global ", stringify!($stub)),
            concat!(stringify!($stub), ":"),
            "push 0",              // dummy error code
            "push rax",
            "push rcx",
            "push rdx",
            "push rbx",
            "push rbp",
            "push rsi",
            "push rdi",
            "push r8",
            "push r9",
            "push r10",
            "push r11",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            concat!("mov edi, ", stringify!($irq)),
            "call handle_device_irq",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r11",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdi",
            "pop rsi",
            "pop rbp",
            "pop rbx",
            "pop rdx",
            "pop rcx",
            "pop rax",
            "add rsp, 8",          // pop dummy error code
            "iretq",
        );
        unsafe extern "C" { fn $stub(); }
    };
}

isr_irq_stub!(isr_irq0, 0);
isr_irq_stub!(isr_irq1, 1);
isr_irq_stub!(isr_irq2, 2);
isr_irq_stub!(isr_irq3, 3);
isr_irq_stub!(isr_irq4, 4);
isr_irq_stub!(isr_irq5, 5);
isr_irq_stub!(isr_irq6, 6);
isr_irq_stub!(isr_irq7, 7);
isr_irq_stub!(isr_irq8, 8);
isr_irq_stub!(isr_irq9, 9);
isr_irq_stub!(isr_irq10, 10);
isr_irq_stub!(isr_irq11, 11);
isr_irq_stub!(isr_irq12, 12);
isr_irq_stub!(isr_irq13, 13);
isr_irq_stub!(isr_irq14, 14);
isr_irq_stub!(isr_irq15, 15);
isr_irq_stub!(isr_irq16, 16);
isr_irq_stub!(isr_irq17, 17);
isr_irq_stub!(isr_irq18, 18);
isr_irq_stub!(isr_irq19, 19);
isr_irq_stub!(isr_irq20, 20);
isr_irq_stub!(isr_irq21, 21);
isr_irq_stub!(isr_irq22, 22);
isr_irq_stub!(isr_irq23, 23);

// ---------------------------------------------------------------------------
// Ring 3 exception handling
// ---------------------------------------------------------------------------

/// The CPL (current privilege level) is stored in bits [1:0] of CS.
/// Ring 3 = user mode.
const RING_3: u64 = 3;

/// Returns `true` if the exception occurred in ring 3 (userspace).
fn is_userspace_exception(frame: &InterruptStackFrame) -> bool {
    (frame.cs & RING_3) == RING_3
}

/// Saved general-purpose registers on the kernel stack.
///
/// Layout matches the push order in the ISR assembly stubs.  The struct
/// lives immediately below the error code on the kernel stack (and the
/// `InterruptStackFrame` is immediately above the error code).
///
/// ```text
/// high address
///   [InterruptStackFrame: rip, cs, rflags, rsp, ss]
///   error_code
///   rax, rcx, rdx, rbx, rbp, rsi, rdi, r8, r9, r10, r11, r12, r13, r14, r15
/// low address ← RSP
/// ```
#[repr(C)]
struct SavedRegisters {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rdi: u64,
    rsi: u64,
    rbp: u64,
    rbx: u64,
    rdx: u64,
    rcx: u64,
    rax: u64,
}

/// Get a mutable pointer to the saved registers on the kernel stack.
///
/// The ISR stub layout is: [saved GPRs][error_code][InterruptStackFrame].
/// `frame_ptr` points to the InterruptStackFrame.  The saved GPRs start
/// at `frame_ptr - 8 (error code) - 15*8 (GPRs) = frame_ptr - 128`.
///
/// # Safety
///
/// `frame_ptr` must point to a valid `InterruptStackFrame` on the
/// kernel stack, created by an ISR assembly stub.
unsafe fn saved_registers_from_frame(frame_ptr: *const InterruptStackFrame) -> *mut SavedRegisters {
    // frame_ptr - 8 (error code) - 120 (15 GPRs × 8) = frame_ptr - 128.
    // SAFETY: ISR stubs push exactly 15 GPRs + error code below the frame.
    unsafe { (frame_ptr as *mut u8).sub(128) as *mut SavedRegisters }
}

/// Try to dispatch a ring 3 exception to the process's registered
/// exception handler (SEH-style).
///
/// If successful, modifies the `InterruptStackFrame` and saved registers
/// on the kernel stack so that IRETQ returns to the user exception
/// handler with an `ExceptionContext` on the user stack.
///
/// Returns `true` if the exception was dispatched (the ISR should just
/// return and let IRETQ do its thing).  Returns `false` if no handler
/// is registered or dispatch failed (caller should kill the task).
///
/// # Why raw pointers + volatile
///
/// The ISR assembly stubs pass the frame pointer through Rust handler
/// signatures typed as `&InterruptStackFrame` (immutable reference).
/// Creating `&mut` from `&` is undefined behavior — the compiler may
/// assume memory behind `&` is never mutated and optimize away our
/// writes.  In release builds this caused an infinite exception loop
/// because `frame.rip` was never actually updated on the stack.
///
/// We avoid all `&`/`&mut` to the frame and saved registers here.
/// Instead we use `addr_of!`/`addr_of_mut!` (which yield raw pointers
/// without creating references) combined with `read_volatile`/
/// `write_volatile` to guarantee the loads and stores reach memory.
///
/// # Safety
///
/// `frame` and `saved` must point to valid structs on the kernel stack,
/// exclusively owned by this ISR invocation (interrupts disabled).
fn try_dispatch_user_exception(
    frame: *mut InterruptStackFrame,
    saved: *mut SavedRegisters,
    code: crate::proc::exception::ExceptionCode,
    aux: u64,
) -> bool {
    use crate::proc::exception::{ExceptionContext, EXCEPTION_CONTEXT_SIZE};
    use crate::proc::thread;
    use core::ptr::{read_volatile, write_volatile};

    // Look up the process's exception handler.
    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) => pid,
        None => return false,
    };

    let handler_addr = match crate::proc::exception::get_handler(pid) {
        Some(addr) => addr,
        None => return false, // No handler → kill the process.
    };

    // Read current frame values via volatile through raw pointers.
    // `addr_of!` on a raw-pointer deref yields `*const field_ty` without
    // ever creating an intermediate `&InterruptStackFrame` reference.
    //
    // SAFETY: frame points to a valid InterruptStackFrame on the kernel
    // stack, exclusively owned by this ISR invocation.
    let (rip, rsp, rflags) = unsafe {
        (
            read_volatile(addr_of!((*frame).rip)),
            read_volatile(addr_of!((*frame).rsp)),
            read_volatile(addr_of!((*frame).rflags)),
        )
    };

    // Read saved general-purpose register values.
    //
    // SAFETY: saved points to valid SavedRegisters on the kernel stack.
    let (rax, rbx, rcx, rdx, rsi, rdi, rbp) = unsafe {
        (
            read_volatile(addr_of!((*saved).rax)),
            read_volatile(addr_of!((*saved).rbx)),
            read_volatile(addr_of!((*saved).rcx)),
            read_volatile(addr_of!((*saved).rdx)),
            read_volatile(addr_of!((*saved).rsi)),
            read_volatile(addr_of!((*saved).rdi)),
            read_volatile(addr_of!((*saved).rbp)),
        )
    };
    let (r8, r9, r10, r11, r12, r13, r14, r15) = unsafe {
        (
            read_volatile(addr_of!((*saved).r8)),
            read_volatile(addr_of!((*saved).r9)),
            read_volatile(addr_of!((*saved).r10)),
            read_volatile(addr_of!((*saved).r11)),
            read_volatile(addr_of!((*saved).r12)),
            read_volatile(addr_of!((*saved).r13)),
            read_volatile(addr_of!((*saved).r14)),
            read_volatile(addr_of!((*saved).r15)),
        )
    };

    // Build the ExceptionContext on the user stack.
    //
    // We need space for the context struct plus an 8-byte slot for a
    // "return address" that the handler's RET will pop.  We use 0
    // (which will fault if the handler tries to return without calling
    // SYS_EXIT or SYS_EXCEPTION_RETURN — this is intentional).
    #[allow(clippy::arithmetic_side_effects)]
    let ctx_size = EXCEPTION_CONTEXT_SIZE as u64;
    #[allow(clippy::arithmetic_side_effects)]
    let new_rsp = (rsp - ctx_size - 8) & !0xF; // 16-byte align

    let ctx_addr = new_rsp;

    // Build the context from the faulting state.
    let ctx = ExceptionContext {
        code: code as u64,
        aux,
        rip,
        rsp,
        rflags,
        rax,
        rbx,
        rcx,
        rdx,
        rsi,
        rdi,
        rbp,
        r8,
        r9,
        r10,
        r11,
        r12,
        r13,
        r14,
        r15,
    };

    // Write the context to the user stack.
    //
    // Since CR3 is still the process's PML4, the user stack is
    // accessible.  But we need to ensure the stack page is mapped
    // (it should be — we only need the page that was already in use
    // by the faulting code, unless the stack is very small).
    //
    // SAFETY: ctx_addr is within the user stack region, which the
    // process had mapped (it was just executing with this RSP).
    // The context struct is safely sized.
    let ctx_ptr = ctx_addr as *mut ExceptionContext;
    // Write a null return address above the context so the handler
    // sees a proper call frame.
    let ret_addr_ptr = (ctx_addr.wrapping_add(ctx_size)) as *mut u64;

    // SAFETY: These addresses are in the user's mapped stack region.
    unsafe {
        core::ptr::write(ctx_ptr, ctx);
        core::ptr::write(ret_addr_ptr, 0u64); // Null return address.
    }

    // Redirect execution to the handler via volatile writes.
    //
    // These writes MUST be volatile: the ISR assembly stubs expose the
    // frame pointer through `&InterruptStackFrame` (immutable), so the
    // compiler is entitled to assume the underlying memory doesn't
    // change.  Volatile writes force the stores to actually reach the
    // kernel stack, where the assembly stub's register-restore sequence
    // and IRETQ will pick them up.
    //
    // SAFETY: frame and saved are valid, exclusively ours (ISR context,
    // interrupts disabled on this CPU).
    unsafe {
        // Jump to the user's exception handler on return from ISR.
        write_volatile(addr_of_mut!((*frame).rip), handler_addr);
        // Use the new stack position (below the ExceptionContext).
        write_volatile(addr_of_mut!((*frame).rsp), new_rsp);
        // CS, SS, RFLAGS stay the same (ring 3, interrupts enabled).

        // Set RDI = pointer to ExceptionContext (first arg, SysV ABI).
        write_volatile(addr_of_mut!((*saved).rdi), ctx_addr);

        // Zero other argument registers for cleanliness.
        write_volatile(addr_of_mut!((*saved).rsi), 0);
        write_volatile(addr_of_mut!((*saved).rdx), 0);
        write_volatile(addr_of_mut!((*saved).rcx), 0);
        write_volatile(addr_of_mut!((*saved).r8), 0);
        write_volatile(addr_of_mut!((*saved).r9), 0);
    }

    serial_println!(
        "[exception] Dispatching {:?} to handler {:#x} for process {} (ctx at {:#x})",
        code, handler_addr, pid, ctx_addr
    );

    true
}

/// Kill the current task because it caused an unrecoverable exception
/// while running in ring 3 (no exception handler registered).
///
/// This function never returns.
fn kill_userspace_task(exception_name: &str, frame: &InterruptStackFrame) -> ! {
    let task_id = sched::current_task_id();
    serial_println!(
        "[exception] Killing task {} — {} at {:#x} (ring 3)",
        task_id, exception_name, frame.rip
    );
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
        frame.cs, frame.rflags, frame.rsp, frame.ss
    );

    crate::proc::thread::on_thread_exit(task_id);
    sched::task_exit();
    cpu::halt_loop();
}

/// Try to dispatch a ring 3 exception to the user handler.  If no handler
/// is registered, kill the task.
///
/// Unlike `kill_userspace_task` (which diverges unconditionally), this
/// function returns if the exception was dispatched to a user handler
/// (the ISR stub will IRETQ to the handler).  If no handler exists,
/// it diverges by killing the task.
///
/// # Safety Model
///
/// We need mutable access to the `InterruptStackFrame` and saved
/// registers on the kernel stack.  The `frame` parameter is `&`
/// because that's what the assembly stubs provide, but the underlying
/// memory IS mutable (it's on the kernel stack, exclusively owned by
/// this ISR invocation).  We recover mutability through pointer
/// arithmetic from the frame's address, not through `&` → `&mut`
/// casting.
/// Raw-pointer variant that avoids `&T` → `&mut T` UB.
///
/// # Safety
///
/// `frame_ptr` must point to a valid `InterruptStackFrame` on the
/// kernel stack, created by an ISR assembly stub.  The stack must
/// be exclusively owned (ISR context, interrupts disabled).
unsafe fn dispatch_or_kill_userspace_raw(
    exception_name: &str,
    frame_ptr: *mut InterruptStackFrame,
    code: crate::proc::exception::ExceptionCode,
    aux: u64,
) {
    // Pass raw pointers directly — never create `&mut` from the frame
    // pointer.  See `try_dispatch_user_exception` docs for the full
    // rationale (TL;DR: `&` → `&mut` is UB and the compiler elides
    // writes in release builds).
    //
    // SAFETY: frame_ptr is valid and exclusive (caller guarantee).
    // saved_registers_from_frame computes the GPR save area from the
    // frame pointer using the ISR stub's known layout.
    let saved_ptr = unsafe { saved_registers_from_frame(frame_ptr) };

    if try_dispatch_user_exception(frame_ptr, saved_ptr, code, aux) {
        // Dispatched — the ISR stub will IRETQ to the handler.
        return;
    }

    // No handler — kill.
    // SAFETY: frame_ptr is valid; creating `&` for reading is fine.
    kill_userspace_task(exception_name, unsafe { &*frame_ptr });
}

/// Convenience wrapper: casts `&InterruptStackFrame` to `*mut` and
/// calls the raw version.
///
/// Sound because ISR handlers run with interrupts disabled on the
/// kernel stack — the frame is exclusively ours and the assembly stub
/// will read the (possibly modified) values after we return.
fn dispatch_or_kill_userspace(
    exception_name: &str,
    frame: &InterruptStackFrame,
    code: crate::proc::exception::ExceptionCode,
    aux: u64,
) {
    // The frame was passed to us from assembly as a pointer in RDI.
    // Rust's `&` is more restrictive than what the assembly intended.
    // We recover the raw pointer the assembly originally computed.
    //
    // SAFETY: ISR context, single CPU, interrupts disabled, exclusive
    // access to the kernel stack.
    let frame_ptr = (frame as *const InterruptStackFrame).cast_mut();
    unsafe {
        dispatch_or_kill_userspace_raw(exception_name, frame_ptr, code, aux);
    }
}

// ---------------------------------------------------------------------------
// Rust exception handlers
//
// These are called from the assembly stubs above.  They MUST be
// #[unsafe(no_mangle)] extern "C" so the assembler can reference them.
//
// Each handler that can be triggered from user code checks the CS
// privilege level.  Ring 3 faults kill the offending task; ring 0
// faults are unrecoverable kernel bugs and halt the system.
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn handle_divide_error(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Divide Error (#DE)", frame, ExceptionCode::DivideError, 0);
        return; // Handler dispatched — IRETQ to user handler.
    }
    serial_println!("EXCEPTION: Divide Error (#DE) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_debug(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Debug (#DB) at {:#x}", frame.rip);
}

#[unsafe(no_mangle)]
extern "C" fn handle_nmi(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: NMI at {:#x}", frame.rip);
}

#[unsafe(no_mangle)]
extern "C" fn handle_breakpoint(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Breakpoint (#BP) at {:#x}", frame.rip);
}

#[unsafe(no_mangle)]
extern "C" fn handle_overflow(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Overflow (#OF)", frame, ExceptionCode::Overflow, 0);
        return;
    }
    serial_println!("EXCEPTION: Overflow (#OF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_bound_range(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Bound Range Exceeded (#BR)", frame, ExceptionCode::BoundRangeExceeded, 0);
        return;
    }
    serial_println!("EXCEPTION: Bound Range Exceeded (#BR) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_invalid_opcode(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Invalid Opcode (#UD)", frame, ExceptionCode::InvalidOpcode, 0);
        return;
    }
    serial_println!("EXCEPTION: Invalid Opcode (#UD) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_device_not_avail(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        // #NM typically means the FPU context isn't available.
        // Dispatch as invalid opcode (closest match).
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Device Not Available (#NM)", frame, ExceptionCode::InvalidOpcode, 0);
        return;
    }
    serial_println!("EXCEPTION: Device Not Available (#NM) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_double_fault(frame: &InterruptStackFrame, error: u64) {
    // Double faults are always unrecoverable — even from ring 3.
    // By the time we get a #DF, the CPU has already failed to handle
    // the original exception AND the secondary fault.
    serial_println!(
        "EXCEPTION: Double Fault (#DF) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
        frame.cs, frame.rflags, frame.rsp, frame.ss
    );
    serial_println!("FATAL: Double fault is unrecoverable. Halting.");
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_invalid_tss(frame: &InterruptStackFrame, error: u64) {
    serial_println!(
        "EXCEPTION: Invalid TSS (#TS) at {:#x}, error={:#x}",
        frame.rip, error
    );
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_seg_not_present(frame: &InterruptStackFrame, error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Segment Not Present (#NP)", frame, ExceptionCode::SegmentNotPresent, error);
        return;
    }
    serial_println!(
        "EXCEPTION: Segment Not Present (#NP) at {:#x}, selector={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_stack_segment(frame: &InterruptStackFrame, error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Stack-Segment Fault (#SS)", frame, ExceptionCode::StackSegmentFault, error);
        return;
    }
    serial_println!(
        "EXCEPTION: Stack-Segment Fault (#SS) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_general_protection(frame: &InterruptStackFrame, error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("General Protection Fault (#GP)", frame, ExceptionCode::GeneralProtectionFault, error);
        return;
    }
    serial_println!(
        "EXCEPTION: General Protection Fault (#GP) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
        frame.cs, frame.rflags, frame.rsp, frame.ss
    );
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_page_fault(frame: &InterruptStackFrame, error: u64) {
    // CR2 contains the faulting virtual address.
    let cr2: u64;
    // SAFETY: Reading CR2 is safe in a page fault handler — it contains
    // the address that caused the fault.
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }

    // Attempt to resolve the fault via the memory manager (demand
    // paging for kernel VMAs).  If resolution succeeds, the CPU will
    // retry the faulting instruction after iretq.
    if mm::fault::resolve(cr2, error).is_ok() {
        return;
    }

    // For user-mode page faults, try demand paging, stack growth, then SEH.
    let is_user = error & 4 != 0;
    if is_user {
        // First, try resolving via per-process VMAs (lazy/demand-paged
        // regions created by SYS_MMAP with MAP_LAZY).
        let task_id = sched::current_task_id();
        let pid = crate::proc::thread::owner_process(task_id).unwrap_or(0);
        if pid != 0 && crate::proc::pcb::try_resolve_fault(pid, cr2, error) {
            return; // Demand-paged successfully — retry the instruction.
        }

        // Second, try stack growth (stack VMAs are handled separately
        // because they pre-date the per-process VMA system and have
        // their own growth logic with guard page detection).
        if try_grow_user_stack(cr2, error) {
            return; // Stack grew successfully — retry the instruction.
        }

        // Unresolvable user fault — try SEH handler, then kill.
        let present = if error & 1 != 0 { "present" } else { "not-present" };
        let write = if error & 2 != 0 { "write" } else { "read" };
        serial_println!(
            "[exception] User page fault (task {}) at {:#x}, addr={:#x} ({}, {}) — trying SEH",
            sched::current_task_id(), frame.rip, cr2, present, write
        );

        // Try SEH dispatch with AccessViolation code and CR2 as aux data.
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Page Fault (#PF)", frame, ExceptionCode::AccessViolation, cr2);
        return; // Handler dispatched.
    }

    // Unresolvable kernel page fault — halt.
    serial_println!(
        "EXCEPTION: Page Fault (#PF) at {:#x}, address={:#x}, error={:#x}",
        frame.rip,
        cr2,
        error
    );

    let present = if error & 1 != 0 { "present" } else { "not-present" };
    let write = if error & 2 != 0 { "write" } else { "read" };
    let user = if error & 4 != 0 { "user" } else { "kernel" };
    serial_println!("  Cause: {present}, {write}, {user}");
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
        frame.cs, frame.rflags, frame.rsp, frame.ss
    );

    cpu::halt_loop();
}

/// Attempt to grow the user stack to cover the faulting address.
///
/// The user stack occupies `[USER_STACK_GUARD, USER_STACK_TOP)` and
/// grows downward on demand.  If `cr2` is within this region and the
/// page is not yet mapped, we allocate a frame and map it with
/// user read/write/no-execute permissions.
///
/// Returns `true` if the stack was successfully grown, `false` if the
/// address is outside the stack region or allocation failed.
fn try_grow_user_stack(cr2: u64, error: u64) -> bool {
    // Only handle not-present faults (bit 0 clear = page not mapped).
    // A present-page violation (protection fault) is not stack growth.
    if error & 1 != 0 {
        return false;
    }

    // Check if the address is in the growable stack region.
    if cr2 < USER_STACK_GUARD || cr2 >= USER_STACK_TOP {
        return false;
    }

    // Align down to the page (frame) boundary.
    #[allow(clippy::arithmetic_side_effects)]
    let page_addr = cr2 & !(FRAME_SIZE as u64 - 1);

    // Read the current PML4 from CR3 — this is the faulting process's
    // page table (the scheduler loaded it on context switch).
    let pml4_phys: u64;
    // SAFETY: Reading CR3 is always safe in ring 0.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) pml4_phys, options(nomem, nostack, preserves_flags));
    }

    // Allocate a physical frame.
    let phys_frame = match frame::alloc_frame() {
        Ok(f) => f,
        Err(_) => return false, // OOM — can't grow stack.
    };

    // Zero the frame (stack pages must be zeroed).
    let Some(hhdm) = page_table::hhdm() else {
        // No HHDM — can't zero the frame.  Free it and fail.
        // SAFETY: phys_frame was just allocated and is exclusively ours.
        let _ = unsafe { frame::free_frame(phys_frame) };
        return false;
    };
    let frame_virt = phys_frame.to_virt(hhdm);
    // SAFETY: frame_virt is the HHDM mapping of a freshly
    // allocated, exclusively owned frame.
    unsafe {
        core::ptr::write_bytes(frame_virt as *mut u8, 0, FRAME_SIZE);
    }

    // Map the frame with user read/write/no-execute permissions.
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;

    let virt = VirtAddr::new(page_addr);
    // SAFETY: pml4_phys is the current CR3 (valid), phys_frame is
    // freshly allocated and exclusively ours, virt is in user space
    // within the stack region.
    match unsafe { page_table::map_frame(pml4_phys, virt, phys_frame, flags) } {
        Ok(()) => true,
        Err(_) => {
            // Mapping failed (e.g., OOM for page table allocation).
            // SAFETY: phys_frame was just allocated and is exclusively ours.
            let _ = unsafe { frame::free_frame(phys_frame) };
            false
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn handle_x87_fp(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("x87 Floating-Point (#MF)", frame, ExceptionCode::FloatingPointError, 0);
        return;
    }
    serial_println!("EXCEPTION: x87 Floating-Point (#MF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_alignment_check(frame: &InterruptStackFrame, error: u64) {
    // #AC can only occur in ring 3 (when CR0.AM and RFLAGS.AC are set).
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Alignment Check (#AC)", frame, ExceptionCode::AlignmentCheck, error);
        return;
    }
    serial_println!(
        "EXCEPTION: Alignment Check (#AC) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_machine_check(frame: &InterruptStackFrame, _error: u64) {
    // Machine check is a hardware error — always fatal.
    serial_println!("EXCEPTION: Machine Check (#MC) at {:#x}", frame.rip);
    serial_println!("FATAL: Machine check is unrecoverable. Halting.");
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_simd_fp(frame: &InterruptStackFrame, _error: u64) {
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("SIMD Floating-Point (#XM)", frame, ExceptionCode::SimdFloatingPoint, 0);
        return;
    }
    serial_println!("EXCEPTION: SIMD Floating-Point (#XM) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[unsafe(no_mangle)]
extern "C" fn handle_default(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("INTERRUPT: Unhandled vector at {:#x}", frame.rip);
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Install exception handlers and load the IDT via `lidt`.
///
/// # Safety
///
/// Must be called exactly once during early boot, with the GDT already
/// loaded (the IDT entries reference the kernel CS selector).
pub unsafe fn init() {
    // SAFETY: Single-threaded boot, no other CPU accesses the IDT yet.
    // We use addr_of_mut! to avoid creating references to a mutable
    // static (Rust 2024 requirement).
    unsafe {
        let cs = gdt::KERNEL_CS;
        let idt = &mut *addr_of_mut!(IDT);

        // CPU exceptions (vectors 0–19).  Double fault uses IST1.
        idt.entries[0] = IdtEntry::new(isr_divide_error as *const () as u64, cs, 0, 0);
        idt.entries[1] = IdtEntry::new(isr_debug as *const () as u64, cs, 0, 0);
        idt.entries[2] = IdtEntry::new(isr_nmi as *const () as u64, cs, 0, 0);
        idt.entries[3] = IdtEntry::new(isr_breakpoint as *const () as u64, cs, 0, 0);
        idt.entries[4] = IdtEntry::new(isr_overflow as *const () as u64, cs, 0, 0);
        idt.entries[5] = IdtEntry::new(isr_bound_range as *const () as u64, cs, 0, 0);
        idt.entries[6] = IdtEntry::new(isr_invalid_opcode as *const () as u64, cs, 0, 0);
        idt.entries[7] = IdtEntry::new(isr_device_not_avail as *const () as u64, cs, 0, 0);
        // Double fault uses IST1 for a separate stack.
        idt.entries[8] = IdtEntry::new(isr_double_fault as *const () as u64, cs, 1, 0);
        // Vector 9 (coprocessor segment overrun) is legacy, not used.
        idt.entries[10] = IdtEntry::new(isr_invalid_tss as *const () as u64, cs, 0, 0);
        idt.entries[11] = IdtEntry::new(isr_seg_not_present as *const () as u64, cs, 0, 0);
        idt.entries[12] = IdtEntry::new(isr_stack_segment as *const () as u64, cs, 0, 0);
        idt.entries[13] = IdtEntry::new(isr_general_protection as *const () as u64, cs, 0, 0);
        idt.entries[14] = IdtEntry::new(isr_page_fault as *const () as u64, cs, 0, 0);
        // Vector 15 is reserved.
        idt.entries[16] = IdtEntry::new(isr_x87_fp as *const () as u64, cs, 0, 0);
        idt.entries[17] = IdtEntry::new(isr_alignment_check as *const () as u64, cs, 0, 0);
        idt.entries[18] = IdtEntry::new(isr_machine_check as *const () as u64, cs, 0, 0);
        idt.entries[19] = IdtEntry::new(isr_simd_fp as *const () as u64, cs, 0, 0);

        // Hardware IRQ vectors.
        // Vector 32: APIC timer interrupt.
        idt.entries[32] = IdtEntry::new(isr_timer as *const () as u64, cs, 0, 0);
        // Vector 255: APIC spurious interrupt.
        idt.entries[255] = IdtEntry::new(isr_spurious as *const () as u64, cs, 0, 0);

        // Vectors 33–56: External device IRQs (IOAPIC inputs 0–23).
        // Each stub calls handle_device_irq(irq_number) in ioapic.rs.
        let irq_stubs: [u64; 24] = [
            isr_irq0  as *const () as u64,
            isr_irq1  as *const () as u64,
            isr_irq2  as *const () as u64,
            isr_irq3  as *const () as u64,
            isr_irq4  as *const () as u64,
            isr_irq5  as *const () as u64,
            isr_irq6  as *const () as u64,
            isr_irq7  as *const () as u64,
            isr_irq8  as *const () as u64,
            isr_irq9  as *const () as u64,
            isr_irq10 as *const () as u64,
            isr_irq11 as *const () as u64,
            isr_irq12 as *const () as u64,
            isr_irq13 as *const () as u64,
            isr_irq14 as *const () as u64,
            isr_irq15 as *const () as u64,
            isr_irq16 as *const () as u64,
            isr_irq17 as *const () as u64,
            isr_irq18 as *const () as u64,
            isr_irq19 as *const () as u64,
            isr_irq20 as *const () as u64,
            isr_irq21 as *const () as u64,
            isr_irq22 as *const () as u64,
            isr_irq23 as *const () as u64,
        ];
        for (i, &addr) in irq_stubs.iter().enumerate() {
            idt.entries[33 + i] = IdtEntry::new(addr, cs, 0, 0);
        }

        // Fill remaining vectors with the default handler.
        let default_addr = isr_default as *const () as u64;
        for entry in &mut idt.entries[20..] {
            if entry.type_attr == 0 {
                *entry = IdtEntry::new(default_addr, cs, 0, 0);
            }
        }

        // Load the IDT.
        // IDT size is 256 * 16 = 4096 bytes; limit (4095) always fits in u16.
        #[allow(clippy::cast_possible_truncation)]
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<Idt>() - 1) as u16,
            base: addr_of!(IDT) as u64,
        };

        core::arch::asm!(
            "lidt [{}]",
            in(reg) &raw const idt_ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    serial_println!("[idt] IDT loaded with {} entries", IDT_ENTRIES);
}
