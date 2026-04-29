//! Interrupt Descriptor Table (IDT) setup and exception handling.
//!
//! The IDT maps interrupt vectors (0–255) to handler functions.  The
//! first 32 vectors are CPU exceptions; the rest are available for
//! hardware IRQs and software interrupts.
//!
//! ## Design
//!
//! - Each exception gets a dedicated assembly stub that saves all
//!   registers, calls a Rust handler, restores registers, and `iretq`s.
//! - Assembly stubs are generated via `global_asm!` (stable Rust — no
//!   nightly features required).
//! - The double-fault handler (#8) uses IST1 (a separate stack) so it
//!   can fire even if the kernel stack itself overflowed.
//! - IRQ handlers (vectors 32+) will be wired up when the APIC driver
//!   is initialized.

use core::arch::global_asm;

use crate::cpu;
use crate::gdt;

// ---------------------------------------------------------------------------
// IDT entry
// ---------------------------------------------------------------------------

/// Number of entries in the IDT (full x86_64 range).
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
    /// - `ist`: IST index (0 = no IST, 1–7 = use that IST stack).
    /// - `dpl`: descriptor privilege level (0 for kernel-only, 3 for
    ///   user-callable via `int` instruction).
    fn new(handler: u64, selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist & 0x7,
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
//   7. Returns via iretq
//
// The Rust handlers are #[no_mangle] extern "C" so the assembler can
// reference them by name.
// ---------------------------------------------------------------------------

/// Generate an assembly stub for an exception WITHOUT a CPU error code.
macro_rules! isr_stub_no_error {
    ($stub:ident, $handler:ident) => {
        global_asm!(
            ".intel_syntax noprefix",
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
        extern "C" { fn $stub(); }
    };
}

/// Generate an assembly stub for an exception WITH a CPU error code.
macro_rules! isr_stub_with_error {
    ($stub:ident, $handler:ident) => {
        global_asm!(
            ".intel_syntax noprefix",
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
        extern "C" { fn $stub(); }
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

// ---------------------------------------------------------------------------
// Rust exception handlers
//
// These are called from the assembly stubs above.  They MUST be
// #[no_mangle] extern "C" so the assembler can reference them.
// ---------------------------------------------------------------------------

#[no_mangle]
extern "C" fn handle_divide_error(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Divide Error (#DE) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_debug(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Debug (#DB) at {:#x}", frame.rip);
}

#[no_mangle]
extern "C" fn handle_nmi(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: NMI at {:#x}", frame.rip);
}

#[no_mangle]
extern "C" fn handle_breakpoint(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Breakpoint (#BP) at {:#x}", frame.rip);
}

#[no_mangle]
extern "C" fn handle_overflow(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Overflow (#OF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_bound_range(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Bound Range Exceeded (#BR) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_invalid_opcode(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Invalid Opcode (#UD) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_device_not_avail(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Device Not Available (#NM) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_double_fault(frame: &InterruptStackFrame, error: u64) {
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

#[no_mangle]
extern "C" fn handle_invalid_tss(frame: &InterruptStackFrame, error: u64) {
    serial_println!(
        "EXCEPTION: Invalid TSS (#TS) at {:#x}, error={:#x}",
        frame.rip, error
    );
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_seg_not_present(frame: &InterruptStackFrame, error: u64) {
    serial_println!(
        "EXCEPTION: Segment Not Present (#NP) at {:#x}, selector={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_stack_segment(frame: &InterruptStackFrame, error: u64) {
    serial_println!(
        "EXCEPTION: Stack-Segment Fault (#SS) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_general_protection(frame: &InterruptStackFrame, error: u64) {
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

#[no_mangle]
extern "C" fn handle_page_fault(frame: &InterruptStackFrame, error: u64) {
    // CR2 contains the faulting virtual address.
    let cr2: u64;
    // SAFETY: Reading CR2 is safe in a page fault handler — it contains
    // the address that caused the fault.
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }

    serial_println!(
        "EXCEPTION: Page Fault (#PF) at {:#x}, address={:#x}, error={:#x}",
        frame.rip,
        cr2,
        error
    );

    // Decode error code bits.
    let present = if error & 1 != 0 { "present" } else { "not-present" };
    let write = if error & 2 != 0 { "write" } else { "read" };
    let user = if error & 4 != 0 { "user" } else { "kernel" };
    serial_println!("  Cause: {present}, {write}, {user}");

    // TODO: Once the memory manager is up, attempt to resolve the fault
    // (demand paging, stack growth, CoW).  For now, halt.
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_x87_fp(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: x87 Floating-Point (#MF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_alignment_check(frame: &InterruptStackFrame, error: u64) {
    serial_println!(
        "EXCEPTION: Alignment Check (#AC) at {:#x}, error={:#x}",
        frame.rip,
        error
    );
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_machine_check(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: Machine Check (#MC) at {:#x}", frame.rip);
    serial_println!("FATAL: Machine check is unrecoverable. Halting.");
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_simd_fp(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("EXCEPTION: SIMD Floating-Point (#XM) at {:#x}", frame.rip);
    cpu::halt_loop();
}

#[no_mangle]
extern "C" fn handle_default(frame: &InterruptStackFrame, _error: u64) {
    serial_println!("INTERRUPT: Unhandled vector at {:#x}", frame.rip);
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Install exception handlers and load the IDT.
///
/// # Safety
///
/// Must be called exactly once during early boot, with the GDT already
/// loaded (the IDT entries reference the kernel CS selector).
pub unsafe fn init() {
    // SAFETY: Single-threaded boot, no other CPU accesses the IDT yet.
    unsafe {
        let cs = gdt::KERNEL_CS;

        // CPU exceptions (vectors 0–19).  Double fault uses IST1.
        IDT.entries[0] = IdtEntry::new(isr_divide_error as usize as u64, cs, 0, 0);
        IDT.entries[1] = IdtEntry::new(isr_debug as usize as u64, cs, 0, 0);
        IDT.entries[2] = IdtEntry::new(isr_nmi as usize as u64, cs, 0, 0);
        IDT.entries[3] = IdtEntry::new(isr_breakpoint as usize as u64, cs, 0, 0);
        IDT.entries[4] = IdtEntry::new(isr_overflow as usize as u64, cs, 0, 0);
        IDT.entries[5] = IdtEntry::new(isr_bound_range as usize as u64, cs, 0, 0);
        IDT.entries[6] = IdtEntry::new(isr_invalid_opcode as usize as u64, cs, 0, 0);
        IDT.entries[7] = IdtEntry::new(isr_device_not_avail as usize as u64, cs, 0, 0);
        // Double fault uses IST1 for a separate stack.
        IDT.entries[8] = IdtEntry::new(isr_double_fault as usize as u64, cs, 1, 0);
        // Vector 9 (coprocessor segment overrun) is legacy, not used.
        IDT.entries[10] = IdtEntry::new(isr_invalid_tss as usize as u64, cs, 0, 0);
        IDT.entries[11] = IdtEntry::new(isr_seg_not_present as usize as u64, cs, 0, 0);
        IDT.entries[12] = IdtEntry::new(isr_stack_segment as usize as u64, cs, 0, 0);
        IDT.entries[13] = IdtEntry::new(isr_general_protection as usize as u64, cs, 0, 0);
        IDT.entries[14] = IdtEntry::new(isr_page_fault as usize as u64, cs, 0, 0);
        // Vector 15 is reserved.
        IDT.entries[16] = IdtEntry::new(isr_x87_fp as usize as u64, cs, 0, 0);
        IDT.entries[17] = IdtEntry::new(isr_alignment_check as usize as u64, cs, 0, 0);
        IDT.entries[18] = IdtEntry::new(isr_machine_check as usize as u64, cs, 0, 0);
        IDT.entries[19] = IdtEntry::new(isr_simd_fp as usize as u64, cs, 0, 0);

        // Fill remaining vectors with the default handler.
        let default_addr = isr_default as usize as u64;
        for entry in &mut IDT.entries[20..] {
            if entry.type_attr == 0 {
                *entry = IdtEntry::new(default_addr, cs, 0, 0);
            }
        }

        // Load the IDT.
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<Idt>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };

        core::arch::asm!(
            "lidt [{}]",
            in(reg) &idt_ptr,
            options(readonly, nostack, preserves_flags),
        );
    }

    serial_println!("[idt] IDT loaded with {} entries", IDT_ENTRIES);
}
