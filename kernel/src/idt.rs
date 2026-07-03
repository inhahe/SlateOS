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

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use core::arch::global_asm;
use core::ptr::addr_of;
use core::ptr::addr_of_mut;

use crate::cpu;
use crate::gdt;
use crate::mm;
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::proc::spawn::{USER_STACK_TOP, USER_STACK_GUARD, MAX_STACK_FRAMES};
use crate::sched;
use crate::serial_println;
use crate::emergency_println;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Exception / interrupt statistics
// ---------------------------------------------------------------------------

/// Per-vector exception/interrupt counts since boot.
///
/// Index = vector number (0–31 for CPU exceptions, 32+ for IRQs).
/// We only track the first 48 vectors (32 exceptions + 16 device IRQs)
/// to keep the array manageable.
const VECTOR_STATS_SIZE: usize = 48;

/// Counts of exception/interrupt firings per vector.
static VECTOR_COUNTS: [AtomicU64; VECTOR_STATS_SIZE] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; VECTOR_STATS_SIZE]
};

/// Increment the counter for a given vector.
#[inline]
fn count_vector(vector: usize) {
    if let Some(c) = VECTOR_COUNTS.get(vector) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Recent exception log (lock-free ring buffer)
// ---------------------------------------------------------------------------

/// Number of recent exception entries to keep.
const EXCEPTION_LOG_SIZE: usize = 32;

/// A logged exception event.
#[derive(Debug, Clone, Copy)]
pub struct ExceptionLogEntry {
    /// Vector number (0–31 for CPU exceptions).
    pub vector: u8,
    /// CPU that took the exception.
    pub cpu: u8,
    /// APIC tick count at the time of exception.
    pub tick: u64,
    /// Faulting instruction pointer (RIP).
    pub rip: u64,
    /// Auxiliary info: error code for #GP/#PF, CR2 for #PF, 0 otherwise.
    pub aux: u64,
}

/// Wrapper to make UnsafeCell<ExceptionLogEntry> usable in a static.
///
/// SAFETY: The ring buffer is accessed via atomic index only. Partial
/// reads are acceptable (all fields are Copy types with no invalid bit
/// patterns). We never hold references across call boundaries.
struct ExcLogSlot(core::cell::UnsafeCell<ExceptionLogEntry>);

// SAFETY: Access is serialized by the atomic write-index. Concurrent
// reads may see partial data but all bit patterns are valid.
unsafe impl Sync for ExcLogSlot {}

/// Ring buffer of recent exception events.
///
/// Lock-free: a single atomic write-index is bumped via fetch_add; each
/// slot stores one entry.  Races on slots are benign (worst case: a
/// partially-written entry is read, which is just slightly stale data).
static EXCEPTION_LOG: [ExcLogSlot; EXCEPTION_LOG_SIZE] = {
    const EMPTY: ExcLogSlot = ExcLogSlot(core::cell::UnsafeCell::new(
        ExceptionLogEntry { vector: 0, cpu: 0, tick: 0, rip: 0, aux: 0 }
    ));
    [EMPTY; EXCEPTION_LOG_SIZE]
};

/// Write index for the exception log ring buffer.
static EXCEPTION_LOG_IDX: AtomicU64 = AtomicU64::new(0);

/// Record an exception in the recent exception log.
///
/// Called from exception handlers for interesting events (not every
/// timer tick — only CPU exceptions vectors 0–31).
#[inline]
fn log_exception(vector: u8, rip: u64, aux: u64) {
    let idx = EXCEPTION_LOG_IDX.fetch_add(1, Ordering::Relaxed) as usize;
    let slot = idx % EXCEPTION_LOG_SIZE;
    let entry = ExceptionLogEntry {
        vector,
        cpu: crate::sched::current_cpu_id() as u8,
        tick: crate::apic::tick_count(),
        rip,
        aux,
    };
    // SAFETY: We own this slot by virtue of the atomic index bump.
    // Concurrent reads of a partially-written entry are safe (all fields
    // are POD with no invalid states).
    unsafe {
        core::ptr::write_volatile(EXCEPTION_LOG[slot].0.get(), entry);
    }
}

/// Get the recent exception log (most recent last).
///
/// Returns up to `EXCEPTION_LOG_SIZE` entries, ordered oldest-to-newest.
/// Entries with tick=0 are empty (never written).
#[must_use]
pub fn recent_exceptions() -> ([ExceptionLogEntry; EXCEPTION_LOG_SIZE], u64) {
    let total = EXCEPTION_LOG_IDX.load(Ordering::Relaxed);
    let mut result = [ExceptionLogEntry { vector: 0, cpu: 0, tick: 0, rip: 0, aux: 0 }; EXCEPTION_LOG_SIZE];

    // Read entries in chronological order.
    let count = total.min(EXCEPTION_LOG_SIZE as u64) as usize;
    let start = if total > EXCEPTION_LOG_SIZE as u64 {
        total as usize - EXCEPTION_LOG_SIZE
    } else {
        0
    };

    for i in 0..count {
        let slot = (start + i) % EXCEPTION_LOG_SIZE;
        // SAFETY: Reading a POD struct; partial reads produce valid (stale) data.
        result[i] = unsafe { core::ptr::read_volatile(EXCEPTION_LOG[slot].0.get()) };
    }

    (result, total)
}

/// Get the count for a specific vector.
#[must_use]
pub fn vector_count(vector: usize) -> u64 {
    VECTOR_COUNTS.get(vector).map_or(0, |c| c.load(Ordering::Relaxed))
}

/// Get all vector counts as an array snapshot.
#[must_use]
pub fn vector_counts() -> [u64; VECTOR_STATS_SIZE] {
    let mut result = [0u64; VECTOR_STATS_SIZE];
    for (i, c) in VECTOR_COUNTS.iter().enumerate() {
        result[i] = c.load(Ordering::Relaxed);
    }
    result
}

// ---------------------------------------------------------------------------
// Interrupt rate tracking (delta-based, snapshot-on-query)
// ---------------------------------------------------------------------------

/// Previous snapshot of vector counts — used to compute deltas.
///
/// Access is not atomic per-element because only a single kshell thread
/// queries rates.  A slight inconsistency during concurrent reads is
/// acceptable for diagnostic display.
static RATE_SNAPSHOT_COUNTS: [AtomicU64; VECTOR_STATS_SIZE] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; VECTOR_STATS_SIZE]
};

/// Tick at which the last rate snapshot was taken.
static RATE_SNAPSHOT_TICK: AtomicU64 = AtomicU64::new(0);

/// Interrupt rate snapshot: counts per second for each vector.
#[derive(Debug, Clone, Copy)]
pub struct InterruptRates {
    /// Per-vector interrupts/sec (fixed-point × 10 for 0.1 resolution).
    /// Value of 15 means 1.5 IRQ/sec.
    pub rates_x10: [u64; VECTOR_STATS_SIZE],
    /// Measurement window duration in ticks.
    pub window_ticks: u64,
}

/// Compute interrupt rates since the last call.
///
/// On the first call (or if no time has elapsed), returns all zeros
/// and takes the baseline snapshot.  Subsequent calls return the
/// delta divided by elapsed time.
#[must_use]
pub fn vector_rates() -> InterruptRates {
    let now_tick = crate::apic::tick_count();
    let prev_tick = RATE_SNAPSHOT_TICK.swap(now_tick, Ordering::Relaxed);
    let elapsed = now_tick.saturating_sub(prev_tick);

    let mut rates = InterruptRates {
        rates_x10: [0; VECTOR_STATS_SIZE],
        window_ticks: elapsed,
    };

    let tick_rate = u64::from(crate::apic::TICK_RATE_HZ);

    for i in 0..VECTOR_STATS_SIZE {
        let current = VECTOR_COUNTS.get(i)
            .map_or(0, |c| c.load(Ordering::Relaxed));
        let prev = RATE_SNAPSHOT_COUNTS.get(i)
            .map_or(0, |c| c.swap(current, Ordering::Relaxed));
        let delta = current.saturating_sub(prev);

        // Rate = delta * tick_rate * 10 / elapsed (× 10 for decimal place).
        if elapsed > 0 {
            rates.rates_x10[i] = delta
                .saturating_mul(tick_rate)
                .saturating_mul(10)
                .checked_div(elapsed)
                .unwrap_or(0);
        }
    }

    rates
}

/// Names for CPU exception vectors 0–31.
pub const EXCEPTION_NAMES: [&str; 32] = [
    "#DE Divide Error",
    "#DB Debug",
    "NMI",
    "#BP Breakpoint",
    "#OF Overflow",
    "#BR Bound Range",
    "#UD Invalid Opcode",
    "#NM Device N/A",
    "#DF Double Fault",
    "Coprocessor Overrun",
    "#TS Invalid TSS",
    "#NP Segment N/P",
    "#SS Stack Segment",
    "#GP General Protection",
    "#PF Page Fault",
    "(Reserved 15)",
    "#MF x87 FP",
    "#AC Alignment Check",
    "#MC Machine Check",
    "#XM SIMD FP",
    "#VE Virtualization",
    "#CP Control Protection",
    "(Reserved 22)",
    "(Reserved 23)",
    "(Reserved 24)",
    "(Reserved 25)",
    "(Reserved 26)",
    "(Reserved 27)",
    "(Reserved 28)",
    "(Reserved 29)",
    "#SX Security",
    "(Reserved 31)",
];

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
// Per-CPU hardware-IRQ stacks (B-DF1 / open-questions Q7, option A)
//
// Hardware IRQs (vectors 32–56, plus the 251/252/255 APIC IPIs) are
// configured with IST index 0, meaning the CPU does NOT switch stacks on
// entry — the interrupt frame is pushed onto whatever stack the interrupted
// code was using.  Heavy in-kernel code running on a near-full 64 KiB kernel
// task stack could therefore push an IRQ frame into the guard page, causing
// an unrecoverable double fault.
//
// To bound interrupt stack usage independently of task-stack depth, each CPU
// gets a dedicated IRQ stack.  The IRQ entry path (`irq_common_dispatch`)
// manually switches RSP to this stack for the duration of the handler, then
// switches back.  Unlike x86 hardware IST — which unconditionally resets RSP
// to the IST top on *every* interrupt and would clobber an outer handler's
// frame on a nested IRQ — the manual switch is performed only on the
// *outermost* IRQ.  A nested IRQ (the timer re-enables interrupts mid-handler
// for preemption) is detected by RSP already lying within the IRQ-stack range
// and continues to grow down the same IRQ stack.
//
// The IRQ stacks are allocated from the guard-page-protected kstack
// allocator, so an IRQ-stack overflow still faults on a guard page (clear
// diagnostic) instead of silently corrupting memory.
//
// The context switch that preemption performs must NOT run on the IRQ stack
// (it would record a transient IRQ-stack RSP as the task's resume point), so
// preemption is deferred: the timer ISR sets a flag via
// `sched::request_preempt()` and the outermost IRQ frame services it via
// `sched::do_deferred_preempt()` after RSP is back on the task stack.
// ---------------------------------------------------------------------------

/// Top (highest address, initial RSP) of each CPU's IRQ stack.  `0` = not yet
/// allocated — that CPU runs IRQs on its task stack as a safe fallback until
/// `init_irq_stack` runs.
static IRQ_STACK_TOP: [AtomicU64; crate::smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; crate::smp::MAX_CPUS]
};

/// Bottom (lowest usable address) of each CPU's IRQ stack.  Combined with the
/// top, used to detect whether the current RSP is already on the IRQ stack
/// (i.e. a nested IRQ).
static IRQ_STACK_BOTTOM: [AtomicU64; crate::smp::MAX_CPUS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; crate::smp::MAX_CPUS]
};

/// Allocate and install this CPU's dedicated hardware-IRQ stack.
///
/// Idempotent per CPU: a second call for an already-initialized CPU is a
/// no-op (the stack is never freed for the lifetime of the system).  Must be
/// called once per CPU *before* that CPU enables interrupts (`sti`).
///
/// On allocation failure the CPU keeps running IRQs on its task stack (the
/// pre-existing behaviour) — safe, but without the overflow-isolation
/// benefit; the failure is logged.
pub fn init_irq_stack(cpu: usize) {
    let Some(top_slot) = IRQ_STACK_TOP.get(cpu) else {
        serial_println!("[idt] init_irq_stack: cpu {} out of range", cpu);
        return;
    };
    if top_slot.load(Ordering::Acquire) != 0 {
        return; // Already initialized for this CPU.
    }
    match mm::kstack::alloc() {
        Ok(info) => {
            if let Some(b) = IRQ_STACK_BOTTOM.get(cpu) {
                b.store(info.stack_bottom, Ordering::Release);
            }
            // Publish the top last: `irq_common_dispatch` treats a non-zero
            // top as "IRQ stack ready" and reads the bottom only after.
            top_slot.store(info.stack_top, Ordering::Release);
            serial_println!(
                "[idt] CPU {} IRQ stack: {:#x}..{:#x}",
                cpu,
                info.stack_bottom,
                info.stack_top
            );
        }
        Err(e) => {
            serial_println!(
                "[idt] CPU {} IRQ stack alloc failed ({:?}); IRQs run on task stack",
                cpu,
                e
            );
        }
    }
}

/// Read the current stack pointer.
#[inline(always)]
fn read_rsp() -> u64 {
    let rsp: u64;
    // SAFETY: Reading RSP has no side effects and does not touch memory.
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
    }
    rsp
}

/// Dispatch a hardware IRQ to its Rust handler by vector number.
///
/// `frame` points to the `InterruptStackFrame` saved on the interrupted
/// task's kernel stack; it remains valid for the duration of the handler
/// regardless of which stack the handler executes on.
extern "C" fn dispatch_vector(frame: *mut InterruptStackFrame, vector: u64) {
    // SAFETY: `frame` was produced by `lea rdi, [rsp + 128]` in the IRQ entry
    // stub and points to a valid `InterruptStackFrame` that outlives this
    // call (it lives on the interrupted task's kernel stack).
    let frame_ref: &InterruptStackFrame = unsafe { &*frame };
    match vector {
        32 => crate::apic::handle_timer_irq(frame_ref, 0),
        251 => crate::tlb::handle_tlb_shootdown_irq(frame_ref, 0),
        252 => crate::apic::handle_reschedule_irq(frame_ref, 0),
        255 => crate::apic::handle_spurious_irq(frame_ref, 0),
        v @ 33..=56 => {
            // Vectors 33–56 map to IOAPIC inputs 0–23.
            #[allow(clippy::arithmetic_side_effects)] // v >= 33 in this arm.
            let irq = (v - 33) as u32;
            crate::ioapic::handle_device_irq(irq);
        }
        _ => {}
    }
}

/// Execute `dispatch_vector(frame, vector)` on the dedicated IRQ stack whose
/// top is `irq_top`, then switch RSP back to the caller's (task) stack.
///
/// # Safety
///
/// `irq_top` must be the 16-byte-aligned top of a valid, mapped, exclusively
/// owned IRQ stack with enough headroom for the handler's worst-case frame,
/// and the current RSP must NOT already be on that stack.  `frame` must point
/// to a valid `InterruptStackFrame`.
unsafe fn run_on_irq_stack(irq_top: u64, frame: *mut InterruptStackFrame, vector: u64) {
    // SAFETY: We save the caller's (task) RSP onto the new IRQ stack via the
    // `{saved}` operand, switch RSP to the IRQ stack, call `dispatch_vector`
    // (its System V arguments `frame`/`vector` are placed in RDI/RSI by the
    // `inout` operands), then restore the saved task RSP via `pop rsp`.  The
    // `push`/`sub rsp, 8` keeps RSP 16-byte aligned at the `call`.  RSP is
    // exactly restored, so the asm block preserves the stack pointer.
    //
    // Every register the template touches is a *named operand* — there are no
    // bare hardcoded registers that the compiler could also allocate to an
    // operand (the original bug: the compiler put `{f}` in RAX and a literal
    // `mov rax, rsp` clobbered it).  RDI/RSI carry the call args (`inout … =>
    // _`); RAX/RCX/RDX/R8–R11 are declared clobbered by the call, which forces
    // `top`/`f`/`saved` into callee-saved registers the compiler preserves.
    unsafe {
        core::arch::asm!(
            "mov {saved}, rsp",
            "mov rsp, {top}",
            "push {saved}",
            "sub rsp, 8",
            "call {f}",
            "add rsp, 8",
            "pop rsp",
            top = in(reg) irq_top,
            f = in(reg) dispatch_vector as extern "C" fn(*mut InterruptStackFrame, u64),
            saved = out(reg) _,
            inout("rdi") frame => _,
            inout("rsi") vector => _,
            lateout("rax") _,
            lateout("rcx") _,
            lateout("rdx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
        );
    }
}

/// Common Rust entry point for every hardware IRQ.
///
/// Called by every IRQ assembly stub after it has saved the 15 GPRs and the
/// interrupt frame on the interrupted task's kernel stack, with `frame`
/// pointing at the saved `InterruptStackFrame` and `vector` the IDT vector
/// number.  Switches to the per-CPU IRQ stack for the handler (outermost IRQ
/// only), then services any deferred preemption on the task stack.
#[unsafe(no_mangle)]
extern "C" fn irq_common_dispatch(frame: *mut InterruptStackFrame, vector: u64) {
    let cpu = crate::smp::current_cpu_index();
    let top = IRQ_STACK_TOP
        .get(cpu)
        .map_or(0, |t| t.load(Ordering::Acquire));

    if top == 0 {
        // IRQ stack not yet installed for this CPU — run on the task stack
        // (pre-existing behaviour: safe, but without overflow isolation).
        dispatch_vector(frame, vector);
        crate::sched::do_deferred_preempt();
        return;
    }

    let bottom = IRQ_STACK_BOTTOM
        .get(cpu)
        .map_or(0, |b| b.load(Ordering::Acquire));
    let rsp = read_rsp();

    if rsp > bottom && rsp <= top {
        // Nested IRQ: we are *already* on the IRQ stack (the timer re-enables
        // interrupts mid-handler).  Keep growing down the same stack; do NOT
        // re-switch and do NOT preempt here — the outermost frame owns
        // preemption, which must run on the task stack.
        dispatch_vector(frame, vector);
        return;
    }

    // Outermost IRQ: run the handler on the IRQ stack, then return to the
    // task stack and service any deferred preemption there.
    // SAFETY: `top` is the valid top of this CPU's IRQ stack (non-zero ⇒
    // installed by `init_irq_stack`), and the RSP range check above confirms
    // we are not already on it.
    unsafe {
        run_on_irq_stack(top, frame, vector);
    }
    crate::sched::do_deferred_preempt();
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

// ---------------------------------------------------------------------------
// Hardware IRQ stubs (vectors 32–56 + APIC IPIs 251/252/255)
//
// Every hardware IRQ stub saves the dummy error code + 15 GPRs on the
// interrupted task's kernel stack (so the GPR/IRETQ frame is preserved across
// a possible context switch), then calls the common Rust dispatcher
// `irq_common_dispatch(frame, vector)`.  The dispatcher switches to the
// per-CPU IRQ stack for the handler and routes by vector number.  Passing the
// vector (not a pre-bound handler) lets one stub shape serve every IRQ while
// keeping per-vector identity for the nesting-aware stack switch.
// ---------------------------------------------------------------------------

/// Generate an assembly stub for a hardware IRQ that dispatches via
/// `irq_common_dispatch` with the given IDT `$vector`.
macro_rules! irq_stub {
    ($stub:ident, $vector:literal) => {
        global_asm!(
            concat!(".global ", stringify!($stub)),
            concat!(stringify!($stub), ":"),
            "push 0",              // dummy error code (IRQs push none)
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
            "lea rdi, [rsp + 128]", // RDI = &InterruptStackFrame (16 × 8)
            concat!("mov esi, ", stringify!($vector)), // RSI = IDT vector
            "call irq_common_dispatch",
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

// Timer (vector 32) — driven by the Local APIC timer.
irq_stub!(isr_timer, 32);
// TLB shootdown IPI (vector 251) — sent by other CPUs to request TLB flush.
irq_stub!(isr_tlb_shootdown, 251);
// Reschedule IPI (vector 252) — sent to wake idle CPUs when work is enqueued.
irq_stub!(isr_reschedule, 252);
// Spurious (vector 255) — APIC spurious interrupts.
irq_stub!(isr_spurious, 255);

// External device IRQs (IOAPIC inputs 0–23 → vectors 33–56).  The dispatcher
// maps vector V in 33..=56 to IOAPIC input V-33 → handle_device_irq.
irq_stub!(isr_irq0, 33);
irq_stub!(isr_irq1, 34);
irq_stub!(isr_irq2, 35);
irq_stub!(isr_irq3, 36);
irq_stub!(isr_irq4, 37);
irq_stub!(isr_irq5, 38);
irq_stub!(isr_irq6, 39);
irq_stub!(isr_irq7, 40);
irq_stub!(isr_irq8, 41);
irq_stub!(isr_irq9, 42);
irq_stub!(isr_irq10, 43);
irq_stub!(isr_irq11, 44);
irq_stub!(isr_irq12, 45);
irq_stub!(isr_irq13, 46);
irq_stub!(isr_irq14, 47);
irq_stub!(isr_irq15, 48);
irq_stub!(isr_irq16, 49);
irq_stub!(isr_irq17, 50);
irq_stub!(isr_irq18, 51);
irq_stub!(isr_irq19, 52);
irq_stub!(isr_irq20, 53);
irq_stub!(isr_irq21, 54);
irq_stub!(isr_irq22, 55);
irq_stub!(isr_irq23, 56);

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
    kill_userspace_task_with_info(exception_name, frame, None);
}

/// Kill the current task with crash information recorded in the PCB.
///
/// The crash info (exception code, faulting address, etc.) is stored
/// in the process's PCB so the parent can retrieve it via
/// `SYS_PROCESS_CRASH_INFO`.  The exit code is set to a negative
/// value derived from the exception code (convention: crash = negative).
///
/// This function never returns.
fn kill_userspace_task_with_info(
    exception_name: &str,
    frame: &InterruptStackFrame,
    crash: Option<crate::proc::pcb::CrashInfo>,
) -> ! {
    let task_id = sched::current_task_id();
    serial_println!(
        "[exception] Killing task {} — {} at {:#x} (ring 3)",
        task_id, exception_name, frame.rip
    );
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x} SS={:#x}",
        frame.cs, frame.rflags, frame.rsp, frame.ss
    );
    // Record crash info in the PCB before killing the thread.
    // This allows the parent process (service manager) to distinguish
    // crashes from normal exits and get diagnostic details.
    if let Some(info) = crash {
        if let Some(pid) = crate::proc::thread::owner_process(task_id) {
            serial_println!(
                "[exception] Recording crash: pid={} exception={} rip={:#x} aux={:#x}",
                pid, info.exception_code, info.faulting_rip, info.aux
            );
            let _ = crate::proc::pcb::set_crash_info(pid, info);
        }
    }

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

    // No handler — kill, recording crash details for the parent.
    // SAFETY: frame_ptr is valid; creating `&` for reading is fine.
    let frame_ref = unsafe { &*frame_ptr };
    let task_id = sched::current_task_id();
    let crash = crate::proc::pcb::CrashInfo {
        exception_code: code as u64,
        faulting_rip: frame_ref.rip,
        aux,
        thread_id: task_id,
    };
    kill_userspace_task_with_info(exception_name, frame_ref, Some(crash));
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
    // For an AbiMode::Linux process with a handler installed for the
    // corresponding signal, a synchronous fault is delivered as a real Linux
    // signal (byte-exact rt_sigframe with a faithful fault siginfo) instead of
    // the native SEH trampoline.  #PF (AccessViolation) is handled separately
    // by handle_page_fault (it alone knows the present bit for the precise
    // SEGV_MAPERR/SEGV_ACCERR code), so it is skipped here.
    if let Some((sig, si_code)) = linux_fault_mapping(code) {
        // For non-#PF faults the architectural fault address is the trapping
        // instruction pointer (cr2 only applies to page faults).
        let addr = frame.rip;
        if try_deliver_linux_fault_signal(frame, sig, si_code, addr) {
            return;
        }
    }

    let frame_ptr = (frame as *const InterruptStackFrame).cast_mut();
    unsafe {
        dispatch_or_kill_userspace_raw(exception_name, frame_ptr, code, aux);
    }
}

/// Map a kernel [`ExceptionCode`](crate::proc::exception::ExceptionCode) to the
/// Linux `(signal, si_code)` pair used when delivering a synchronous fault to
/// an `AbiMode::Linux` process.
///
/// Returns `None` for `AccessViolation` (#PF) — delivered directly by
/// [`handle_page_fault`] with the precise `SEGV_MAPERR`/`SEGV_ACCERR` code,
/// which alone knows the page-fault present bit — and for any code with no
/// meaningful Linux signal mapping.
fn linux_fault_mapping(
    code: crate::proc::exception::ExceptionCode,
) -> Option<(u32, i32)> {
    use crate::proc::exception::ExceptionCode as E;
    use crate::proc::linux_sigframe::si_fault_code as F;
    use crate::proc::signal::si_code::SI_KERNEL;
    // x86_64 Linux signal numbers.
    const SIGILL: u32 = 4;
    const SIGFPE: u32 = 8;
    const SIGBUS: u32 = 7;
    const SIGSEGV: u32 = 11;
    Some(match code {
        E::DivideError => (SIGFPE, F::FPE_INTDIV),
        E::Overflow => (SIGFPE, F::FPE_INTOVF),
        E::InvalidOpcode => (SIGILL, F::ILL_ILLOPN),
        E::FloatingPointError | E::SimdFloatingPoint => (SIGFPE, F::FPE_FLTINV),
        E::AlignmentCheck => (SIGBUS, F::BUS_ADRALN),
        // #BR/#NP/#SS/#GP all surface as SIGSEGV with a kernel-origin si_code.
        E::BoundRangeExceeded
        | E::SegmentNotPresent
        | E::StackSegmentFault
        | E::GeneralProtectionFault => (SIGSEGV, SI_KERNEL),
        // #PF handled separately with the precise present-bit si_code.
        E::AccessViolation => return None,
    })
}

/// Attempt to deliver a synchronous CPU fault to a ring-3 `AbiMode::Linux`
/// process as a real Linux signal.
///
/// If the faulting process is Linux-ABI and has a handler installed for `sig`
/// (not `SIG_DFL`/`SIG_IGN`), this builds a byte-exact Linux `rt_sigframe`
/// carrying a faithful fault `siginfo` (`si_addr` = `addr`, `si_code` =
/// `si_code`) on the user stack and rewrites the interrupt frame + saved
/// registers so `IRETQ` enters the handler.  The trapped register state is
/// preserved in the frame's `uc_mcontext` so `rt_sigreturn` can resume the
/// faulting instruction (or the handler may `siglongjmp` away).
///
/// Returns `true` if delivered (the ISR should return and let `IRETQ` run the
/// handler).  Returns `false` if the process is native (keeps the SEH
/// trampoline, design-decision #4), has no handler for `sig`, or the user
/// stack is unusable — in which case the caller proceeds to native SEH
/// dispatch / terminate (the kernel default for an undelivered fault).
fn try_deliver_linux_fault_signal(
    frame: &InterruptStackFrame,
    sig: u32,
    si_code: i32,
    addr: u64,
) -> bool {
    use crate::proc::thread;
    use crate::syscall::linux::{self, LinuxDisposition, LinuxTrapRegs};
    use core::ptr::{read_volatile, write_volatile};

    let task_id = sched::current_task_id();
    let pid = match thread::owner_process(task_id) {
        Some(pid) if pid != 0 => pid,
        _ => return false,
    };

    // Only Linux-ABI processes use rt_sigframe delivery; native processes keep
    // the SEH-style trampoline (design-decision #4).
    if crate::proc::pcb::get_abi_mode(pid) != Some(crate::proc::pcb::AbiMode::Linux)
    {
        return false;
    }

    // Resolve the disposition; only a real handler diverts the fault. SIG_DFL /
    // SIG_IGN fall through to the kernel default (terminate for a fault).
    let act = match linux::linux_disposition(pid, sig) {
        LinuxDisposition::Handler(act) => act,
        LinuxDisposition::Ignore | LinuxDisposition::Default => return false,
    };

    // Recover raw pointers to the interrupt frame + saved GPRs without forming
    // an aliasing `&mut` (see `try_dispatch_user_exception` docs).
    let frame_ptr = (frame as *const InterruptStackFrame).cast_mut();
    // SAFETY: ISR context; frame_ptr is a valid InterruptStackFrame on the
    // kernel stack; the saved GPRs sit 128 bytes below it (ISR stub layout).
    let saved_ptr = unsafe { saved_registers_from_frame(frame_ptr) };

    // SAFETY: both pointers are valid and exclusively ours (interrupts disabled
    // on this CPU); volatile reads pick up the trapped register state.
    let regs = unsafe {
        LinuxTrapRegs {
            rax: read_volatile(addr_of!((*saved_ptr).rax)),
            rbx: read_volatile(addr_of!((*saved_ptr).rbx)),
            rcx: read_volatile(addr_of!((*saved_ptr).rcx)),
            rdx: read_volatile(addr_of!((*saved_ptr).rdx)),
            rsi: read_volatile(addr_of!((*saved_ptr).rsi)),
            rdi: read_volatile(addr_of!((*saved_ptr).rdi)),
            rbp: read_volatile(addr_of!((*saved_ptr).rbp)),
            r8: read_volatile(addr_of!((*saved_ptr).r8)),
            r9: read_volatile(addr_of!((*saved_ptr).r9)),
            r10: read_volatile(addr_of!((*saved_ptr).r10)),
            r11: read_volatile(addr_of!((*saved_ptr).r11)),
            r12: read_volatile(addr_of!((*saved_ptr).r12)),
            r13: read_volatile(addr_of!((*saved_ptr).r13)),
            r14: read_volatile(addr_of!((*saved_ptr).r14)),
            r15: read_volatile(addr_of!((*saved_ptr).r15)),
            rip: read_volatile(addr_of!((*frame_ptr).rip)),
            rsp: read_volatile(addr_of!((*frame_ptr).rsp)),
            rflags: read_volatile(addr_of!((*frame_ptr).rflags)),
        }
    };

    let siginfo = crate::proc::linux_sigframe::LinuxSiginfo::fault(
        #[allow(clippy::cast_possible_wrap)]
        {
            sig as i32
        },
        si_code,
        addr,
    );

    let entry = match linux::emit_linux_rt_frame(pid, sig, &act, &regs, siginfo) {
        Some(e) => e,
        None => return false, // user stack unusable — caller terminates.
    };

    // Redirect IRETQ into the handler.  Volatile writes guarantee the stores
    // reach the kernel stack the assembly stub restores from.
    // SAFETY: frame_ptr / saved_ptr are valid and exclusive (ISR context).
    unsafe {
        write_volatile(addr_of_mut!((*frame_ptr).rip), entry.rip);
        write_volatile(addr_of_mut!((*frame_ptr).rsp), entry.rsp);
        write_volatile(addr_of_mut!((*frame_ptr).rflags), entry.rflags);
        write_volatile(addr_of_mut!((*saved_ptr).rdi), entry.rdi);
        write_volatile(addr_of_mut!((*saved_ptr).rsi), entry.rsi);
        write_volatile(addr_of_mut!((*saved_ptr).rdx), entry.rdx);
        // Match the syscall delivery path: r10/r8/r9 cleared at handler entry.
        write_volatile(addr_of_mut!((*saved_ptr).r10), 0);
        write_volatile(addr_of_mut!((*saved_ptr).r8), 0);
        write_volatile(addr_of_mut!((*saved_ptr).r9), 0);
    }

    serial_println!(
        "[exception] Delivered Linux signal {} (si_code={}, addr={:#x}) to process {} handler {:#x}",
        sig, si_code, addr, pid, entry.rip
    );

    true
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

/// Handle #DE (Divide Error, vector 0).
///
/// Ring 3: dispatch to SEH handler or kill task.  Ring 0: halt (kernel bug).
#[unsafe(no_mangle)]
extern "C" fn handle_divide_error(frame: &InterruptStackFrame, _error: u64) {
    count_vector(0);
    log_exception(0, frame.rip, 0);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Divide Error (#DE)", frame, ExceptionCode::DivideError, 0);
        return; // Handler dispatched — IRETQ to user handler.
    }
    serial_println!("EXCEPTION: Divide Error (#DE) at {:#x}", frame.rip);
    cpu::halt_loop();
}

/// Handle #DB (Debug, vector 1).  Logged but non-fatal.
#[unsafe(no_mangle)]
extern "C" fn handle_debug(frame: &InterruptStackFrame, _error: u64) {
    count_vector(1);
    serial_println!("EXCEPTION: Debug (#DB) at {:#x}", frame.rip);
}

/// Handle NMI (Non-Maskable Interrupt, vector 2).
///
/// NMIs can be caused by:
/// - Memory parity error (System Control Port B, bit 7)
/// - I/O channel check (System Control Port B, bit 6)
/// - Watchdog hardware timeout
/// - Performance counter overflow (profiling)
/// - External debugger break (e.g., GDB connecting via QEMU)
///
/// We read port 0x61 (System Control Port B) to identify the source.
/// Non-fatal unless memory parity indicates hardware failure.
#[unsafe(no_mangle)]
extern "C" fn handle_nmi(frame: &InterruptStackFrame, _error: u64) {
    count_vector(2);
    // Read System Control Port B (port 0x61) to identify NMI source.
    // SAFETY: port 0x61 is always readable on PC-compatible hardware.
    let port_b: u8 = unsafe { crate::port::inb(0x61) };

    let parity_error = port_b & 0x80 != 0;
    let iochan_check = port_b & 0x40 != 0;

    if parity_error || iochan_check {
        // NMI context: use lock-free emergency output (the interrupted code may
        // hold the global serial spinlock).
        emergency_println!("EXCEPTION: NMI at {:#x} — hardware error", frame.rip);
        if parity_error {
            emergency_println!("  Memory parity error (possible bad RAM)");
        }
        if iochan_check {
            emergency_println!("  I/O channel check error");
        }
        crate::klog!(Error, "hw.nmi",
            "NMI hardware error at {:#x}: parity={}, iochan={}",
            frame.rip, parity_error, iochan_check
        );
    } else if crate::hardlockup::is_armed() {
        // No hardware-error bits and the hard-lockup watchdog is armed: the
        // i6300esb NMI fired because cpu0 stopped kicking it. QEMU's inject-nmi
        // broadcasts to every CPU, but this watchdog is driven *solely* by the
        // BSP timer tick (kick() lives at the top of `timer_tick` on cpu0), so
        // only a BSP that goes silent can trip it. That makes cpu0 the sole
        // authority on whether this is a real wedge — APs just record context.
        crate::hardlockup::note_fired();
        let cpu = crate::sched::current_cpu_id();

        if cpu != 0 {
            // AP context is informational only. Print a *non-greppable* line
            // (the harness looks for "NMI WATCHDOG FIRED", which we reserve for
            // real BSP wedges classified on cpu0). The classifier is now a pure
            // read of the BSP kick-staleness clock, so an AP taking the broadcast
            // NMI cannot corrupt it — but only cpu0 is the authority on a
            // BSP-driven watchdog, so APs still just record context and return.
            // Lock-free: the wedged BSP may hold the global serial spinlock.
            emergency_println!(
                "[hardlockup] NMI on AP cpu={} rip={:#x} rflags={:#x}",
                cpu, frame.rip, frame.rflags
            );
            return;
        }

        // cpu0: distinguish a genuine BSP-dead wedge (BSP timer stopped kicking
        // because a spin with IF=0 blocks `timer_tick`) from a spurious NMI
        // (QEMU/TCG virtual-clock-vs-APIC-timer divergence during a heavy
        // debug-build compute burst — the BSP is alive and still kicking). The
        // classifier reads a monotonic kick-staleness clock, so it fires on the
        // wedge's *first* NMI with no dependence on a prior baseline.
        // See hardlockup::classify_nmi and known-issues.md.
        let hb = crate::sched::bsp_heartbeat();
        let stale_ns = crate::hardlockup::kick_staleness_ns();
        let real = crate::hardlockup::classify_nmi();

        if real {
            // Real wedge. frame.rip is the wedged instruction we've been unable
            // to observe any other way. Always dump the backtrace + task table
            // here — unconditionally, ignoring the one-shot latch — so an earlier
            // spurious NMI that consumed the latch cannot rob the real wedge of
            // its stack trace (the exact failure mode observed in the make+tcc
            // soak: a mid-boot spurious NMI took the one-shot dump, and the later
            // wedge produced no diagnostic at all). Then emit the greppable
            // marker the soak harness keys on.
            HARDLOCKUP_DUMPED.store(true, core::sync::atomic::Ordering::Release);
            // Lock-free emergency output for the marker + backtrace: the whole
            // point of this watchdog is to report from a wedged machine, and the
            // wedged cpu0 may be holding the global serial spinlock (e.g. it
            // froze mid-`serial_println!`). A normal `serial_println!` here would
            // then spin forever on that held lock, producing the exact
            // total-silence-with-no-dump we observed. `dump_kernel_backtrace`
            // also uses emergency output internally.
            emergency_println!(
                "[hardlockup] NMI WATCHDOG FIRED cpu={} rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} ss={:#x} heartbeat={} kick_stale_ns={} — dumping backtrace + task table",
                cpu, frame.rip, frame.cs, frame.rflags, frame.rsp, frame.ss, hb, stale_ns
            );
            dump_kernel_backtrace(frame);
            // klog and the task-table dump take other locks (structured-log
            // buffer, scheduler) and are therefore best-effort — they run after
            // the RIP + backtrace above, which are the irreplaceable data. If the
            // wedge is holding one of those locks these may hang, but by then the
            // marker has already escaped via the lock-free path.
            crate::klog!(Error, "hw.nmi",
                "hardlockup NMI cpu={} rip={:#x} heartbeat={} kick_stale_ns={}",
                cpu, frame.rip, hb, stale_ns);
            crate::sched::dump_task_table();
        } else {
            // Spurious: the BSP is alive and still kicking. Take a one-shot
            // backtrace + task-table dump on the first such NMI (cheap, harmless,
            // and occasionally useful for early diagnostics), then re-kick the
            // watchdog and resume rather than latching a false catch. No greppable
            // marker.
            if !HARDLOCKUP_DUMPED.swap(true, core::sync::atomic::Ordering::AcqRel) {
                emergency_println!(
                    "[hardlockup] first (spurious) NMI on cpu={} rip={:#x} rflags={:#x} kick_stale_ns={} — dumping backtrace + task table",
                    cpu, frame.rip, frame.rflags, stale_ns
                );
                dump_kernel_backtrace(frame);
                crate::sched::dump_task_table();
            }
            emergency_println!(
                "[hardlockup] spurious NMI (BSP alive, heartbeat={} kick_stale_ns={}) at rip={:#x} — re-arming, resuming",
                hb, stale_ns, frame.rip
            );
            // Full re-arm, not a bare kick: QEMU's i6300esb resets (disables) the
            // counter when it fires its action, so a mere RELOAD_PING would not
            // restart it and the watchdog would be dead for the rest of the boot —
            // exactly why an earlier spurious NMI let a later real wedge hang
            // silently. rearm() re-enables the counter (NMI-safe, cached PCI addr).
            crate::hardlockup::rearm();
        }
    } else {
        // No hardware error bits — likely a software NMI (debugger, watchdog,
        // or performance monitoring).  Just log it.
        serial_println!("EXCEPTION: NMI at {:#x} (software/external)", frame.rip);
    }
}

/// One-shot latch so only the first CPU to take a hard-lockup watchdog NMI
/// dumps the (global) task table, avoiding N× serial spam when QEMU broadcasts
/// the injected NMI to every CPU.
static HARDLOCKUP_DUMPED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Lowest canonical higher-half address. Used to sanity-check stack/frame
/// pointers before dereferencing them in the NMI backtrace. Kernel *stacks* are
/// not confined to the top −2 GiB (where the kernel image / `.text` lives):
/// per-task kernel stacks are allocated from the HHDM/vmalloc region (observed
/// e.g. at `0xffffc10000…`), so the stack-validity floor must be the full
/// higher-half base, not the text base.
const HIGHER_HALF_MIN: u64 = 0xffff_8000_0000_0000;

// Linker-defined `.text` bounds, used to precisely classify a word as a real
// return address (points *into* executable kernel code) versus stale data.
unsafe extern "C" {
    static __text_start: u8;
    static __text_end: u8;
}

/// True iff `val` points into the kernel's executable `.text` section, i.e. it
/// is a plausible return address. Bounds come from the linker script, so this
/// rejects rodata/data/bss/stack pointers that a loose "any higher-half address"
/// scan would wrongly report.
fn is_kernel_text(val: u64) -> bool {
    let lo = core::ptr::addr_of!(__text_start) as u64;
    let hi = core::ptr::addr_of!(__text_end) as u64;
    val >= lo && val < hi
}

/// Best-effort kernel backtrace for the hard-lockup NMI dump.
///
/// The NMI is delivered with `ist=0`, so `frame.rsp` is the *interrupted*
/// kernel stack pointer and the stack is intact. We can't unwind DWARF in NMI
/// context, but every Rust kernel function compiled here keeps a frame pointer
/// (`push rbp; mov rbp, rsp`), so we can walk the RBP chain precisely:
///
///   [rbp+8] = return address into the caller
///   [rbp+0] = caller's saved RBP (the next frame, always higher on the stack)
///
/// The interrupted RBP is recovered from the ISR stub's register save area: the
/// `isr_stub_no_error!` macro pushes the dummy error code + all 15 GP registers
/// below the CPU-pushed interrupt frame, so the saved `rbp` sits 48 bytes (6
/// words: error, rax, rcx, rdx, rbx, rbp) below the `frame` pointer — see the
/// offset derivation at the read site below.
///
/// After the precise walk we also do the old conservative stack scan as a
/// backstop (some frames may be `#[naked]`/asm with no RBP link), filtering to
/// words that land inside `.text` so data pointers are not misreported.
///
/// Skipped entirely if the interrupt came from ring 3 (`cs & 3 != 0`), where
/// the stack is untrusted user memory.
fn dump_kernel_backtrace(frame: &InterruptStackFrame) {
    let rip = frame.rip;
    let rsp = frame.rsp;
    let cs = frame.cs;

    // Only meaningful for a ring-0 wedge; a ring-3 rsp is untrusted user memory.
    // All output here uses the lock-free `emergency_println!` — this runs in the
    // hard-lockup NMI path and the wedged code may hold the global serial lock.
    if cs & 0x3 != 0 {
        emergency_println!("[hardlockup] backtrace: ring-3 frame (cs={:#x}), skipped", cs);
        return;
    }
    if rsp < HIGHER_HALF_MIN {
        // A sane kernel stack is in the higher-half; a low rsp means we can't
        // trust it (or the wedge corrupted it) — don't risk a fault.
        emergency_println!("[hardlockup] backtrace: rsp={:#x} not in higher-half, skipped", rsp);
        return;
    }

    emergency_println!("[hardlockup] backtrace (rbp-chain walk):");
    emergency_println!("[hardlockup]   [rip]     {:#x}", rip);

    // Recover the interrupted RBP from the ISR stub's save area. The macro
    // `isr_stub_no_error!` pushes, in order below the CPU interrupt frame:
    //   dummy-error, rax, rcx, rdx, rbx, rbp, rsi, rdi, r8..r15
    // and sets `rdi = rsp + 128` (the `frame` pointer). Counting down from
    // `frame`: [frame-8]=error, [frame-16]=rax, [frame-24]=rcx, [frame-32]=rdx,
    // [frame-40]=rbx, [frame-48]=rbp. So the saved RBP is 6 words below `frame`.
    let frame_ptr = frame as *const InterruptStackFrame as *const u64;
    // SAFETY: `frame_ptr` was produced by the ISR stub as `rsp+128`, so the six
    // words below it are the pushed dummy-error + rax/rcx/rdx/rbx/rbp save slots
    // — mapped, valid kernel stack. The read is 8-byte aligned and volatile.
    let mut rbp = unsafe { core::ptr::read_volatile(frame_ptr.sub(6)) };

    // Walk the frame-pointer chain. Each iteration validates `rbp` before
    // dereferencing: it must be in the higher-half, 8-byte aligned, and (after
    // the first hop) strictly greater than the previous frame — a monotonic
    // increase toward the stack base guarantees termination and that every read
    // targets already-mapped older-frame memory.
    const MAX_DEPTH: u32 = 32;
    let mut depth: u32 = 0;
    let mut prev: u64 = 0;
    loop {
        if depth >= MAX_DEPTH {
            emergency_println!("[hardlockup]   … (depth cap {} reached)", MAX_DEPTH);
            break;
        }
        if rbp < HIGHER_HALF_MIN || rbp & 0x7 != 0 {
            break;
        }
        if prev != 0 && rbp <= prev {
            // Not monotonically increasing → chain is broken/corrupt; stop.
            break;
        }
        // `rbp` and `rbp+8` are two words at a valid higher-half, aligned frame
        // pointer; both lie in already-mapped stack. SAFETY as above.
        let ret = unsafe { core::ptr::read_volatile((rbp as *const u64).add(1)) };
        let next = unsafe { core::ptr::read_volatile(rbp as *const u64) };
        if is_kernel_text(ret) {
            emergency_println!("[hardlockup]   [{:#x}] ret {:#x}", rbp, ret);
            depth = depth.wrapping_add(1);
        }
        prev = rbp;
        rbp = next;
    }

    // Backstop: conservative stack scan, filtered to real `.text` addresses.
    // Catches callers whose frames the RBP walk skipped (e.g. asm/naked stubs).
    emergency_println!("[hardlockup] backtrace (stack scan, .text words from rsp={:#x}):", rsp);
    const MAX_WORDS: usize = 256;
    const MAX_HITS: u32 = 40;
    let mut hits: u32 = 0;
    for i in 0..MAX_WORDS {
        // `i` is bounded by MAX_WORDS so the multiply/add cannot overflow.
        #[allow(clippy::arithmetic_side_effects)]
        let addr = rsp + (i as u64) * 8;
        // SAFETY: `addr` is within a 2 KiB window at higher addresses than the
        // interrupted `rsp` (older, already-mapped stack frames). 8-byte aligned
        // by construction; volatile so the read is not reordered/elided.
        let val = unsafe { core::ptr::read_volatile(addr as *const u64) };
        if is_kernel_text(val) {
            #[allow(clippy::arithmetic_side_effects)]
            let off = addr - rsp;
            emergency_println!("[hardlockup]   [rsp+{:#05x}] {:#x}", off, val);
            hits = hits.wrapping_add(1);
            if hits >= MAX_HITS {
                emergency_println!("[hardlockup]   … (hit cap {} reached)", MAX_HITS);
                break;
            }
        }
    }
    emergency_println!("[hardlockup] backtrace end ({} scan candidate(s))", hits);
}

/// Handle #BP (Breakpoint, vector 3).  Logged but non-fatal.
#[unsafe(no_mangle)]
extern "C" fn handle_breakpoint(frame: &InterruptStackFrame, _error: u64) {
    count_vector(3);
    serial_println!("EXCEPTION: Breakpoint (#BP) at {:#x}", frame.rip);
}

/// Handle #OF (Overflow, vector 4).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_overflow(frame: &InterruptStackFrame, _error: u64) {
    count_vector(4);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Overflow (#OF)", frame, ExceptionCode::Overflow, 0);
        return;
    }
    serial_println!("EXCEPTION: Overflow (#OF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

/// Handle #BR (Bound Range Exceeded, vector 5).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_bound_range(frame: &InterruptStackFrame, _error: u64) {
    count_vector(5);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Bound Range Exceeded (#BR)", frame, ExceptionCode::BoundRangeExceeded, 0);
        return;
    }
    serial_println!("EXCEPTION: Bound Range Exceeded (#BR) at {:#x}", frame.rip);
    cpu::halt_loop();
}

/// Handle #UD (Invalid Opcode, vector 6).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_invalid_opcode(frame: &InterruptStackFrame, _error: u64) {
    count_vector(6);
    log_exception(6, frame.rip, 0);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Invalid Opcode (#UD)", frame, ExceptionCode::InvalidOpcode, 0);
        return;
    }
    serial_println!("EXCEPTION: Invalid Opcode (#UD) at {:#x}", frame.rip);
    serial_println!(
        "  CS={:#x} RFLAGS={:#x} RSP={:#x}",
        frame.cs, frame.rflags, frame.rsp
    );

    // Dump the bytes at the faulting instruction for post-mortem decode.
    if frame.rip >= 0xFFFF_8000_0000_0000 {
        let ptr = frame.rip as *const u8;
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: kernel text address, always mapped.
            *byte = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        }
        serial_println!(
            "  Instruction bytes: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15]
        );
        // Common causes: 0x0F 0x0B = `ud2` (intentional trap, e.g., unreachable),
        //                 0x0F 0x1F = nop variants that older CPUs reject.
        if bytes[0] == 0x0F && bytes[1] == 0x0B {
            serial_println!("  Likely cause: UD2 instruction (intentional trap / unreachable code)");
        }
    }

    let sched_info = sched::panic_diagnostics();
    let name_slice = sched_info.name.get(..sched_info.name_len).unwrap_or(&[]);
    let task_name = core::str::from_utf8(name_slice).unwrap_or("?");
    serial_println!(
        "  Task: {} ({:?}), cpu {}",
        sched_info.current_task_id, task_name, sched::current_cpu_id()
    );

    crate::backtrace::print_current();
    serial_println!("FATAL: Unrecoverable kernel #UD. Halting.");
    cpu::halt_loop();
}

/// Handle #NM (Device Not Available, vector 7).
///
/// Typically means FPU context isn't loaded.  Ring 3: SEH dispatch.
/// Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_device_not_avail(frame: &InterruptStackFrame, _error: u64) {
    count_vector(7);
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

/// Handle #DF (Double Fault, vector 8).  Always fatal.
///
/// Runs on IST1 (dedicated stack) so it works even if the kernel
/// stack is corrupted or overflowed.  Prints diagnostic context
/// (task, memory) using non-blocking lock acquisition.
#[unsafe(no_mangle)]
extern "C" fn handle_double_fault(frame: &InterruptStackFrame, error: u64) {
    count_vector(8);
    log_exception(8, frame.rip, error);
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

    // Print task context.  Use try_lock-based diagnostics to avoid
    // deadlock — the double fault may have been caused by a bug in
    // code that was holding the scheduler lock.
    let sched_info = sched::panic_diagnostics();
    let name_slice = sched_info.name.get(..sched_info.name_len).unwrap_or(&[]);
    let task_name = core::str::from_utf8(name_slice).unwrap_or("<invalid>");
    serial_println!(
        "  Task: {} ({:?}), priority {}, cpu {}",
        sched_info.current_task_id,
        task_name,
        sched_info.priority,
        sched::current_cpu_id(),
    );

    // Check if the faulting RSP was near a task stack boundary —
    // strong indicator of kernel stack overflow.
    if sched_info.stack_bottom != 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let stack_top = sched_info.stack_bottom + sched::task::TASK_STACK_SIZE as u64;
        if frame.rsp < sched_info.stack_bottom || frame.rsp > stack_top {
            serial_println!(
                "  RSP {:#x} is OUTSIDE task stack [{:#x}..{:#x}] — stack overflow likely",
                frame.rsp, sched_info.stack_bottom, stack_top
            );
        }
    }

    // Independent guard-page check against the kstack region.  This does NOT
    // depend on `sched_info` (whose `stack_bottom` is 0 whenever the scheduler
    // lock can't be acquired in the #DF path — common, since a #DF often
    // happens with a lock held), so it diagnoses kernel stack overflow even
    // when the per-task data above is unavailable.  `is_guard_page` /
    // `is_kstack_region` are pure address arithmetic (no locks).  This was
    // added after B-DF1, where a benchmark overflowed its 64 KiB kernel stack
    // into a kstack guard page and the only clue was a bare `atomic_load` PC.
    if crate::mm::kstack::is_guard_page(frame.rsp) {
        serial_println!(
            "  RSP {:#x} is in a kstack GUARD PAGE — KERNEL STACK OVERFLOW confirmed",
            frame.rsp
        );
    } else if crate::mm::kstack::is_kstack_region(frame.rsp) {
        // In a kstack slot but not (yet) the guard page — still worth noting,
        // since a #DF here usually means an interrupt frame push ran the stack
        // off the end during delivery.
        serial_println!(
            "  RSP {:#x} is within the kstack region (possible stack exhaustion)",
            frame.rsp
        );
    }

    // Print stack backtrace for crash diagnostics.
    crate::backtrace::print_current();

    serial_println!("FATAL: Double fault is unrecoverable. Halting.");
    cpu::halt_loop();
}

/// Handle #TS (Invalid TSS, vector 10).  Always fatal — broken task state.
#[unsafe(no_mangle)]
extern "C" fn handle_invalid_tss(frame: &InterruptStackFrame, error: u64) {
    count_vector(10);
    serial_println!(
        "EXCEPTION: Invalid TSS (#TS) at {:#x}, error={:#x}",
        frame.rip, error
    );
    cpu::halt_loop();
}

/// Handle #NP (Segment Not Present, vector 11).  Error code = selector index.
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_seg_not_present(frame: &InterruptStackFrame, error: u64) {
    count_vector(11);
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

/// Handle #SS (Stack-Segment Fault, vector 12).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_stack_segment(frame: &InterruptStackFrame, error: u64) {
    count_vector(12);
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

/// Handle #GP (General Protection Fault, vector 13).
///
/// Catches privilege violations, bad segment access, non-canonical addresses.
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_general_protection(frame: &InterruptStackFrame, error: u64) {
    count_vector(13);
    log_exception(13, frame.rip, error);
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

    // Decode the error code: for #GP, it's a selector index.
    // Bits [15:3] = selector index, bit [2] = TI (0=GDT, 1=LDT/IDT),
    // bit [1] = IDT flag, bit [0] = EXT (external event).
    if error != 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let selector_idx = (error >> 3) & 0x1FFF;
        let is_idt = error & 0x2 != 0;
        let is_ext = error & 0x1 != 0;
        let table = if is_idt { "IDT" } else if error & 0x4 != 0 { "LDT" } else { "GDT" };
        serial_println!(
            "  Error decode: {} index={}, ext={}",
            table, selector_idx, is_ext
        );
    } else {
        serial_println!("  Error decode: no selector (likely non-canonical address or privilege violation)");
    }

    // Try to read the faulting instruction bytes for diagnosis.
    // SAFETY: RIP points to kernel text (we checked this isn't userspace).
    // Reading a few bytes from kernel text is safe if the address is canonical.
    if frame.rip >= 0xFFFF_8000_0000_0000 {
        let ptr = frame.rip as *const u8;
        let mut bytes = [0u8; 8];
        for (i, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: kernel text is always mapped and readable.
            *byte = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        }
        serial_println!(
            "  Instruction bytes: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7]
        );
    }

    // Print task context for debugging (uses try_lock).
    let sched_info = sched::panic_diagnostics();
    let name_slice = sched_info.name.get(..sched_info.name_len).unwrap_or(&[]);
    let task_name = core::str::from_utf8(name_slice).unwrap_or("?");
    serial_println!(
        "  Task: {} ({:?}), priority {}, cpu {}",
        sched_info.current_task_id,
        task_name,
        sched_info.priority,
        sched::current_cpu_id(),
    );

    // Print stack backtrace for crash diagnostics.
    crate::backtrace::print_current();

    serial_println!("FATAL: Unrecoverable kernel #GP. Halting.");
    cpu::halt_loop();
}

/// Handle #PF (Page Fault, vector 14).  CR2 = faulting address.
///
/// Tries in order: kernel VMA resolve, swap-in, demand paging (VMAs),
/// stack growth, SEH dispatch.  Ring 0 faults halt.
#[unsafe(no_mangle)]
extern "C" fn handle_page_fault(frame: &InterruptStackFrame, error: u64) {
    count_vector(14);

    // CR2 contains the faulting virtual address.
    let cr2: u64;
    // SAFETY: Reading CR2 is safe in a page fault handler — it contains
    // the address that caused the fault.
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }

    // Make page-fault resolution preemptible.  #PF is dispatched through an
    // interrupt gate (IDT type 0xE), so we arrive here with IF=0.  Resolving a
    // fault can be *long*: demand-paging a subpaged file frame reads up to
    // 16 KiB through the VFS, and CoW/large-frame copies touch many pages —
    // and in debug builds heap poisoning makes every alloc/free O(size) with
    // per-byte volatile writes.  Holding IF=0 across all of that starves the
    // timer tick on this CPU (no preemption, no watchdog kick, no liveness
    // heartbeat) for the whole duration — the exact "IF=0 across a long
    // operation" anti-pattern the design forbids, and the residual cause of
    // the ~9.8 s hard-lockup NMI false-fires seen during the ring-3 battery.
    //
    // The fix mirrors Linux's `do_page_fault`, which calls `local_irq_enable()`
    // as soon as it is safe: re-enable interrupts here, but ONLY when the
    // faulting context itself had them enabled (saved RFLAGS.IF set).  Faults
    // taken from an already-IF=0 context (inside an ISR, the scheduler, or any
    // cli/raw-spin critical section) keep interrupts disabled, so we never
    // widen the interruptible window beyond what the interrupted code allowed.
    //
    // Safety w.r.t. CR2: it is captured into `cr2` above *before* this point,
    // so a nested page fault taken after re-enabling cannot clobber the value
    // we resolve against — the nested handler reads and consumes its own CR2.
    const RFLAGS_IF: u64 = 1 << 9;
    if frame.rflags & RFLAGS_IF != 0 {
        // SAFETY: the IDT is fully initialised (we are running its #PF
        // handler), and CR2 has already been captured, so re-enabling
        // interrupts here cannot lose fault state.  We only do so when the
        // interrupted context had IF=1, preserving its interruptibility.
        unsafe {
            cpu::sti();
        }
    }

    // Attempt to resolve the fault via the memory manager (demand
    // paging for kernel VMAs).  If resolution succeeds, the CPU will
    // retry the faulting instruction after iretq.
    if mm::fault::resolve(cr2, error).is_ok() {
        return;
    }

    // For user-mode page faults, try swap-in, demand paging, stack
    // growth, then SEH.
    let is_user = error & 4 != 0;
    if is_user {
        // First, try swap-in: if the PTE contains a swap entry, the
        // page was previously evicted to swap storage and needs to be
        // restored.  Only for not-present faults (bit 0 clear).
        if error & 1 == 0 {
            use mm::page_table::{VirtAddr, read_cr3, cr3_to_pml4};
            let pml4 = cr3_to_pml4(read_cr3());
            let frame_aligned = cr2 & !(mm::frame::FRAME_SIZE as u64 - 1);
            let virt = VirtAddr::new(frame_aligned);

            // SAFETY: pml4 is the current process's page table (from CR3).
            if unsafe { mm::swap::is_swapped(pml4, virt) } {
                // The page is swapped out — need to restore it.
                // Determine the flags from the VMA, or use a safe default.
                let flags = mm::page_table::PageFlags::PRESENT
                    | mm::page_table::PageFlags::WRITABLE
                    | mm::page_table::PageFlags::USER_ACCESSIBLE
                    | mm::page_table::PageFlags::NO_EXECUTE;

                // SAFETY: pml4 is valid, PTE contains a swap entry.
                if unsafe { mm::swap::swap_in_page(pml4, virt, flags) }.is_ok() {
                    // Re-register the restored page as reclaimable so it
                    // can be swapped out again if memory pressure returns.
                    mm::swap::register_reclaimable(pml4, virt.as_u64(), flags);
                    mm::fault::record_swap_in();
                    mm::fault::record_user_resolved();
                    // Major fault: resolution required I/O (swap-in).
                    sched::account_fault(sched::current_task_id(), true);
                    return; // Swap-in successful — retry the instruction.
                }
                // If swap-in fails (OOM, etc.), fall through to other
                // handlers or eventually kill the process.
            }
        }

        // Second, try resolving via per-process VMAs (lazy/demand-paged
        // regions created by SYS_MMAP with MAP_LAZY).
        let task_id = sched::current_task_id();
        let pid = crate::proc::thread::owner_process(task_id).unwrap_or(0);
        if pid != 0 && crate::proc::pcb::try_resolve_fault(pid, cr2, error) {
            mm::fault::record_user_resolved();
            // Minor fault: demand-zero / CoW resolved without I/O.
            sched::account_fault(task_id, false);
            return; // Demand-paged successfully — retry the instruction.
        }

        // Second, try stack growth (stack VMAs are handled separately
        // because they pre-date the per-process VMA system and have
        // their own growth logic with guard page detection).
        if try_grow_user_stack(cr2, error, pid) {
            mm::fault::record_stack_growth();
            mm::fault::record_user_resolved();
            // Minor fault: stack growth resolved without I/O.
            sched::account_fault(task_id, false);
            return; // Stack grew successfully — retry the instruction.
        }

        // Unresolvable user fault — try a Linux fault signal, then SEH, then
        // kill.
        mm::fault::record_fatal();
        log_exception(14, frame.rip, cr2);
        let present = if error & 1 != 0 { "present" } else { "not-present" };
        let write = if error & 2 != 0 { "write" } else { "read" };
        serial_println!(
            "[exception] User page fault (task {}) at {:#x}, addr={:#x} ({}, {}) — trying SEH",
            sched::current_task_id(), frame.rip, cr2, present, write
        );

        // For an AbiMode::Linux process with a SIGSEGV handler, deliver a real
        // Linux signal carrying si_addr = CR2 and the precise si_code: a
        // protection violation (present bit set) maps to SEGV_ACCERR, a
        // not-present access to SEGV_MAPERR.
        {
            use crate::proc::linux_sigframe::si_fault_code::{SEGV_ACCERR, SEGV_MAPERR};
            const SIGSEGV: u32 = 11;
            let si_code = if error & 1 != 0 { SEGV_ACCERR } else { SEGV_MAPERR };
            if try_deliver_linux_fault_signal(frame, SIGSEGV, si_code, cr2) {
                return; // Linux signal delivered — IRETQ into the handler.
            }
        }

        // Try SEH dispatch with AccessViolation code and CR2 as aux data.
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("Page Fault (#PF)", frame, ExceptionCode::AccessViolation, cr2);
        return; // Handler dispatched.
    }

    // Unresolvable kernel page fault — halt.
    mm::fault::record_fatal();
    log_exception(14, frame.rip, cr2);
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

    // Print task context for easier debugging (uses try_lock).
    let sched_info = sched::panic_diagnostics();
    let name_slice = sched_info.name.get(..sched_info.name_len).unwrap_or(&[]);
    let task_name = core::str::from_utf8(name_slice).unwrap_or("?");
    serial_println!(
        "  Task: {} ({:?}), priority {}, cpu {}",
        sched_info.current_task_id,
        task_name,
        sched_info.priority,
        sched::current_cpu_id(),
    );

    // Print stack backtrace for crash diagnostics.
    crate::backtrace::print_current();

    serial_println!("FATAL: Unrecoverable kernel page fault. Halting.");
    cpu::halt_loop();
}

/// Attempt to grow the user stack to cover the faulting address.
///
/// The user stack occupies a virtual region below `USER_STACK_TOP` and
/// grows downward on demand.  The maximum growth is bounded by:
///
/// 1. The compile-time `USER_STACK_GUARD` — absolute floor of the
///    reserved virtual address region.  Cannot grow below this even if
///    the sysctl value is higher.
/// 2. The runtime `mm.max_stack_frames` sysctl — allows administrators
///    to restrict stack growth below the compile-time maximum.
/// 3. The per-process `RLIMIT_STACK` soft limit (Linux ABI), looked up
///    via [`crate::proc::pcb::try_get_rlimit`] when `pid != 0`.  The
///    `try_lock`-based accessor returns `None` on contention; in that
///    case we silently skip the RLIMIT_STACK term, matching the
///    pre-enforcement behavior — better to occasionally allow a stack
///    page past the limit than to deadlock the page fault handler.
///
/// The effective guard is the maximum (i.e. most restrictive) of all
/// applicable terms.
///
/// If `cr2` is within the growable region and the page is not yet
/// mapped, we allocate a zeroed frame and map it with user
/// read/write/no-execute permissions.
///
/// Returns `true` if the stack was successfully grown, `false` if the
/// address is outside the stack region or allocation failed.
fn try_grow_user_stack(cr2: u64, error: u64, pid: u64) -> bool {
    // Only handle not-present faults (bit 0 clear = page not mapped).
    // A present-page violation (protection fault) is not stack growth.
    if error & 1 != 0 {
        return false;
    }

    // Fast reject: address must be below USER_STACK_TOP and above the
    // compile-time absolute floor.
    if cr2 < USER_STACK_GUARD || cr2 >= USER_STACK_TOP {
        return false;
    }

    // Dynamic limit: the sysctl mm.max_stack_frames may be lower than
    // the compile-time MAX_STACK_FRAMES, restricting growth further.
    // We compute the dynamic guard and use the more restrictive (higher)
    // of the two guards.
    #[allow(clippy::arithmetic_side_effects)]
    let dynamic_guard = {
        let max_frames = crate::sysctl::get(
            crate::sysctl::PARAM_MM_MAX_STACK_FRAMES,
        ).unwrap_or(MAX_STACK_FRAMES as u64);
        let max_bytes = max_frames.saturating_mul(FRAME_SIZE as u64);
        USER_STACK_TOP.saturating_sub(max_bytes)
    };

    // Per-process RLIMIT_STACK guard.  RLIM_INFINITY (u64::MAX) and a
    // `None` return (lock contention or unknown pid) both fall back to
    // USER_STACK_GUARD — i.e. the rlimit term contributes nothing in
    // either case, leaving only the compile-time and sysctl bounds.
    //
    // `try_get_rlimit` is the interrupt-safe accessor: it never blocks,
    // so the page fault handler cannot deadlock against a syscall that
    // happens to hold the process table.  See pcb.rs for rationale.
    let rlimit_guard = if pid != 0 {
        match crate::proc::pcb::try_get_rlimit(
            pid,
            crate::proc::pcb::RLIMIT_STACK_INDEX as u32,
        ) {
            Some((soft, _hard)) if soft != crate::proc::pcb::RLIM_INFINITY => {
                USER_STACK_TOP.saturating_sub(soft)
            }
            _ => USER_STACK_GUARD,
        }
    } else {
        USER_STACK_GUARD
    };

    let effective_guard = USER_STACK_GUARD
        .max(dynamic_guard)
        .max(rlimit_guard);

    if cr2 < effective_guard {
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

    // Allocate a zeroed physical frame for the new stack page.
    let phys_frame = match frame::alloc_frame_zeroed() {
        Ok(f) => f,
        Err(_) => return false, // OOM or HHDM unavailable — can't grow stack.
    };

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
        Ok(()) => {
            // Register the new page as reclaimable so the Clock algorithm
            // can swap it out under memory pressure.
            mm::swap::register_reclaimable(pml4_phys, virt.as_u64(), flags);
            true
        }
        Err(_) => {
            // Mapping failed (e.g., OOM for page table allocation).
            // SAFETY: phys_frame was just allocated and is exclusively ours.
            let _ = unsafe { frame::free_frame(phys_frame) };
            false
        }
    }
}

/// Handle #MF (x87 Floating-Point Error, vector 16).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_x87_fp(frame: &InterruptStackFrame, _error: u64) {
    count_vector(16);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("x87 Floating-Point (#MF)", frame, ExceptionCode::FloatingPointError, 0);
        return;
    }
    serial_println!("EXCEPTION: x87 Floating-Point (#MF) at {:#x}", frame.rip);
    cpu::halt_loop();
}

/// Handle #AC (Alignment Check, vector 17).  Only triggers from ring 3.
#[unsafe(no_mangle)]
extern "C" fn handle_alignment_check(frame: &InterruptStackFrame, error: u64) {
    count_vector(17);
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

/// Handle #MC (Machine Check, vector 18).  Hardware error — always fatal.
#[unsafe(no_mangle)]
extern "C" fn handle_machine_check(frame: &InterruptStackFrame, _error: u64) {
    count_vector(18);
    // Machine check is a hardware error — always fatal.
    // Read MCE bank MSRs for diagnostic information about what failed.
    serial_println!("EXCEPTION: Machine Check (#MC) at {:#x}", frame.rip);
    serial_println!("--- MCE Bank Status ---");

    // IA32_MCG_STATUS (MSR 0x17A): global MCE status.
    // SAFETY: Valid MSR on all x86_64 CPUs with MCE support.
    let mcg_status = unsafe { cpu::rdmsr(0x17A) };
    serial_println!("  MCG_STATUS: {:#018x}", mcg_status);
    if mcg_status & 1 != 0 {
        serial_println!("    RIPV: restart IP valid");
    }
    if mcg_status & 2 != 0 {
        serial_println!("    EIPV: error IP valid");
    }
    if mcg_status & 4 != 0 {
        serial_println!("    MCIP: machine check in progress");
    }

    // IA32_MCG_CAP (MSR 0x179): number of error-reporting banks.
    // SAFETY: MSR 0x179 is the MCG_CAP register, valid on x86_64 with MCE.
    let mcg_cap = unsafe { cpu::rdmsr(0x179) };
    let bank_count = (mcg_cap & 0xFF) as u32;
    let count = bank_count.min(8); // Limit to 8 banks to avoid huge output.

    for bank in 0..count {
        // Each bank has: STATUS at 0x401 + bank*4, ADDR at 0x402 + bank*4.
        let status_msr = 0x401u32.saturating_add(bank.saturating_mul(4));
        let addr_msr = 0x402u32.saturating_add(bank.saturating_mul(4));

        // SAFETY: MSR addresses are valid for x86_64 with MCE.
        let status = unsafe { cpu::rdmsr(status_msr) };
        if status & (1u64 << 63) != 0 {
            // VAL bit set — this bank has a logged error.
            let addr = unsafe { cpu::rdmsr(addr_msr) };
            serial_println!("  Bank {}: STATUS={:#018x} ADDR={:#018x}", bank, status, addr);
            if status & (1u64 << 61) != 0 {
                serial_println!("    UC: uncorrected error");
            }
            if status & (1u64 << 57) != 0 {
                serial_println!("    PCC: processor context corrupt");
            }
        }
    }

    serial_println!("FATAL: Machine check is unrecoverable. Halting.");
    crate::klog!(Error, "hw.mce", "Machine check exception at {:#x}, MCG_STATUS={:#x}", frame.rip, mcg_status);
    cpu::halt_loop();
}

/// Handle #XM (SIMD Floating-Point, vector 19).
///
/// Ring 3: SEH dispatch.  Ring 0: halt.
#[unsafe(no_mangle)]
extern "C" fn handle_simd_fp(frame: &InterruptStackFrame, _error: u64) {
    count_vector(19);
    if is_userspace_exception(frame) {
        use crate::proc::exception::ExceptionCode;
        dispatch_or_kill_userspace("SIMD Floating-Point (#XM)", frame, ExceptionCode::SimdFloatingPoint, 0);
        return;
    }
    serial_println!("EXCEPTION: SIMD Floating-Point (#XM) at {:#x}", frame.rip);
    cpu::halt_loop();
}

/// Catch-all for unhandled interrupt vectors.  Logged but non-fatal.
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
        // Vector 251: TLB shootdown IPI.
        idt.entries[251] = IdtEntry::new(isr_tlb_shootdown as *const () as u64, cs, 0, 0);
        // Vector 252: Reschedule IPI (wake idle CPU when work enqueued).
        idt.entries[252] = IdtEntry::new(isr_reschedule as *const () as u64, cs, 0, 0);
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

/// Load the IDT on an Application Processor.
///
/// APs share the same IDT as the BSP — all interrupt handlers are the
/// same across CPUs.  This just executes `lidt` to point this CPU's
/// IDTR at the shared table.
///
/// # Safety
///
/// The IDT must have been initialized by `init()` on the BSP.
/// Interrupts must be disabled.
pub unsafe fn load() {
    // SAFETY: IDT was initialized by BSP.
    #[allow(clippy::cast_possible_truncation)]
    let idt_ptr = IdtPointer {
        limit: (core::mem::size_of::<Idt>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };

    // SAFETY: idt_ptr references our static IDT which was initialized by
    // the BSP.  LIDT loads the IDT register from the pointer.
    unsafe {
        core::arch::asm!(
            "lidt [{}]",
            in(reg) &raw const idt_ptr,
            options(readonly, nostack, preserves_flags),
        );
    }
}
