//! Global Descriptor Table (GDT) and Task State Segment (TSS) setup.
//!
//! In 64-bit long mode the GDT is mostly vestigial --- segment bases and
//! limits are ignored for code and data segments.  But three things still
//! require it:
//!
//! 1. The CPU needs valid CS/DS/SS selectors (privilege level encoded in
//!    the selector, not the descriptor).
//! 2. The `syscall`/`sysret` instructions compute their target selectors
//!    from the `IA32_STAR` MSR, which indexes into the GDT.
//! 3. The TSS (loaded via `ltr`) provides the kernel stack pointer for
//!    privilege transitions and the Interrupt Stack Table (IST) for
//!    per-interrupt stacks.
//!
//! ## Per-CPU Architecture
//!
//! Each CPU gets its own GDT and TSS.  The TSS contains per-CPU stacks:
//! - **RSP0**: kernel stack for ring 3→0 transitions (interrupts, syscalls)
//! - **IST1**: dedicated double-fault stack (catches kernel stack overflow)
//! - **IST2**: dedicated NMI stack (hard-lockup watchdog — always usable even
//!   when the interrupted context's stack is exhausted or wedged)
//!
//! The GDT code/data segments are identical across CPUs, but the TSS
//! descriptor must point to each CPU's own TSS, so we need per-CPU GDTs.
//!
//! Based on Linux `arch/x86/kernel/cpu/common.c` per-CPU GDT approach.
//!
//! ## GDT Layout (per CPU)
//!
//! | Index | Offset | Segment           | DPL | Notes                       |
//! |-------|--------|-------------------|-----|-----------------------------|
//! | 0     | 0x00   | Null              | —   | Required by CPU             |
//! | 1     | 0x08   | Kernel Code       | 0   | SYSCALL target CS           |
//! | 2     | 0x10   | Kernel Data       | 0   | SYSCALL target SS           |
//! | 3     | 0x18   | User Data         | 3   | SYSRET SS = STAR[63:48]+8   |
//! | 4     | 0x20   | User Code         | 3   | SYSRET CS = STAR[63:48]+16  |
//! | 5–6   | 0x28   | TSS (16 bytes)    | 0   | Loaded via `ltr`            |

use core::mem::size_of;
use core::ptr::addr_of;
use core::ptr::addr_of_mut;

use crate::cpu;
use crate::smp::MAX_CPUS;

// ---------------------------------------------------------------------------
// Segment selectors (byte offsets into GDT, with RPL bits)
// ---------------------------------------------------------------------------

/// Kernel code segment selector.
pub const KERNEL_CS: u16 = 0x08;
/// Kernel data segment selector.
pub const KERNEL_DS: u16 = 0x10;
/// User data segment selector (RPL=3).
pub const USER_DS: u16 = 0x18 | 3;
/// User code segment selector (RPL=3).
pub const USER_CS: u16 = 0x20 | 3;
/// TSS segment selector.
pub const TSS_SEL: u16 = 0x28;

// ---------------------------------------------------------------------------
// GDT entries (pre-computed raw u64 values)
// ---------------------------------------------------------------------------

/// Null descriptor — required as entry 0.
const GDT_NULL: u64 = 0;

/// Kernel code: Present, DPL=0, Code segment, Execute/Read, Long mode.
///
/// Bits: base=0, limit=0xFFFFF, access=0x9A (P=1, DPL=0, S=1, type=0xA),
///       flags=0xA (G=1, L=1, D=0).
const GDT_KERNEL_CODE: u64 = 0x00AF_9A00_0000_FFFF;

/// Kernel data: Present, DPL=0, Data segment, Read/Write.
///
/// Bits: base=0, limit=0xFFFFF, access=0x92 (P=1, DPL=0, S=1, type=0x2),
///       flags=0xC (G=1, D=1, L=0).
const GDT_KERNEL_DATA: u64 = 0x00CF_9200_0000_FFFF;

/// User data: Present, DPL=3, Data segment, Read/Write.
///
/// Same as kernel data but with DPL=3.  access=0xF2.
const GDT_USER_DATA: u64 = 0x00CF_F200_0000_FFFF;

/// User code: Present, DPL=3, Code segment, Execute/Read, Long mode.
///
/// Same as kernel code but with DPL=3.  access=0xFA.
const GDT_USER_CODE: u64 = 0x00AF_FA00_0000_FFFF;

// ---------------------------------------------------------------------------
// TSS
// ---------------------------------------------------------------------------

/// Size of the per-CPU interrupt stack (16 KiB — one of our base pages).
const INTERRUPT_STACK_SIZE: usize = 16 * 1024;

/// 64-bit Task State Segment.
///
/// In long mode the TSS holds:
/// - RSP0–RSP2: stack pointers loaded on privilege transitions
/// - IST1–IST7: Interrupt Stack Table entries for per-interrupt stacks
/// - I/O permission bitmap base address
#[repr(C, packed)]
pub struct TaskStateSegment {
    _reserved0: u32,
    /// Stack pointer loaded on transition to ring 0.
    pub rsp0: u64,
    /// Stack pointer loaded on transition to ring 1 (unused).
    pub rsp1: u64,
    /// Stack pointer loaded on transition to ring 2 (unused).
    pub rsp2: u64,
    _reserved1: u64,
    /// Interrupt Stack Table entry 1 (double fault).
    pub ist1: u64,
    /// Interrupt Stack Table entry 2 (NMI — hard-lockup watchdog).
    ///
    /// The NMI vector uses a dedicated IST stack so a non-maskable interrupt
    /// delivered while the interrupted context is on a nearly-exhausted or
    /// otherwise unusable stack (a stack-overflow wedge, or an IF=0 spin deep
    /// in an IRQ handler) does not push its frame onto that bad stack and
    /// triple-fault.  Hardware IST switches the stack *before* the CPU pushes
    /// the interrupt frame, which a stub-level RSP switch (used by ordinary
    /// IRQs) cannot do.  This is what lets `handle_nmi` reliably report the
    /// hard-lockup wedge instead of vanishing into a triple fault.
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    _reserved2: u64,
    _reserved3: u16,
    /// Offset from TSS base to the I/O permission bitmap.
    pub iomap_base: u16,
}

impl TaskStateSegment {
    // TSS struct is 104 bytes — the iomap_base cast always fits in u16.
    #[allow(clippy::cast_possible_truncation)]
    const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            _reserved1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: size_of::<Self>() as u16,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-CPU GDT and TSS arrays
// ---------------------------------------------------------------------------

/// Per-CPU stacks for RSP0 (ring 3→0 transitions).
///
/// Each CPU needs its own interrupt/syscall stack so concurrent interrupts
/// on different CPUs don't stomp each other's stacks.
///
/// # Safety
///
/// Mutable statics — each CPU accesses only its own slot, initialized
/// during sequential bootstrap.
static mut PRIVILEGE_STACKS: [[u8; INTERRUPT_STACK_SIZE]; MAX_CPUS] =
    [[0; INTERRUPT_STACK_SIZE]; MAX_CPUS];

/// Per-CPU stacks for IST1 (double-fault handling).
///
/// Same per-CPU isolation rationale as `PRIVILEGE_STACKS`.
static mut DOUBLE_FAULT_STACKS: [[u8; INTERRUPT_STACK_SIZE]; MAX_CPUS] =
    [[0; INTERRUPT_STACK_SIZE]; MAX_CPUS];

/// Per-CPU stacks for IST2 (NMI / hard-lockup watchdog handling).
///
/// A dedicated NMI stack (separate from the double-fault stack) so the NMI
/// handler always has a known-good stack even when the interrupted context's
/// stack is exhausted or wedged — see `TaskStateSegment::ist2`.  Keeping it
/// distinct from IST1 also means an NMI that arrives while a double fault is
/// being handled (or vice versa) does not reuse the other's stack.
static mut NMI_STACKS: [[u8; INTERRUPT_STACK_SIZE]; MAX_CPUS] =
    [[0; INTERRUPT_STACK_SIZE]; MAX_CPUS];

/// Per-CPU TSS instances.
///
/// # Safety
///
/// Each CPU accesses only its own TSS.  The BSP initializes TSS[0] during
/// early boot; each AP initializes its own during SMP bootstrap.  After
/// init, only the scheduler writes to `rsp0` (under a critical section
/// with interrupts disabled on the local CPU).
static mut TSS_ARRAY: [TaskStateSegment; MAX_CPUS] =
    [const { TaskStateSegment::new() }; MAX_CPUS];

/// The GDT itself: 5 normal 8-byte entries + 1 TSS entry (16 bytes) = 7 u64s.
#[repr(C, align(16))]
struct Gdt {
    entries: [u64; 7],
}

/// The GDT pointer (used by `lgdt`).
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Per-CPU GDT instances.
///
/// Each CPU needs its own GDT because the TSS descriptor (entries 5–6)
/// encodes the base address of that CPU's TSS.  The code/data segments
/// (entries 0–4) are identical across all CPUs.
///
/// # Safety
///
/// Each CPU accesses only its own GDT, initialized during sequential boot.
static mut GDT_ARRAY: [Gdt; MAX_CPUS] = [const {
    Gdt {
        entries: [
            GDT_NULL,
            GDT_KERNEL_CODE,
            GDT_KERNEL_DATA,
            GDT_USER_DATA,
            GDT_USER_CODE,
            0, // TSS low  — filled at runtime
            0, // TSS high — filled at runtime
        ],
    }
}; MAX_CPUS];

/// Build the two u64 halves of a 64-bit TSS descriptor from the TSS
/// base address and size.
fn make_tss_descriptor(base: u64, limit: u32) -> (u64, u64) {
    let mut low: u64 = 0;

    // Limit [15:0]
    low |= u64::from(limit) & 0xFFFF;
    // Base [23:0]
    low |= (base & 0xFF_FFFF) << 16;
    // Access byte: Present, DPL=0, Type=0x9 (64-bit TSS, available)
    low |= 0x89_u64 << 40;
    // Limit [19:16]
    low |= (u64::from(limit) >> 16 & 0xF) << 48;
    // Base [31:24]
    low |= ((base >> 24) & 0xFF) << 56;

    // High u64: base [63:32], rest reserved.
    let high = base >> 32;

    (low, high)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Initialize GDT entry 5–6 with the TSS descriptor for the given CPU,
/// then load the GDT and TSS.
///
/// # Safety
///
/// - `cpu` must be a valid CPU index (0..MAX_CPUS).
/// - The caller must be running on the CPU identified by `cpu`.
/// - Must be called with interrupts disabled.
unsafe fn init_cpu_gdt_tss(cpu: usize) {
    // SAFETY: We're initializing our own CPU's data, no concurrent access.
    unsafe {
        // Set up this CPU's TSS stack pointers.
        let priv_stack_top = addr_of!(PRIVILEGE_STACKS[cpu])
            .cast::<u8>()
            .add(INTERRUPT_STACK_SIZE);
        (*addr_of_mut!(TSS_ARRAY[cpu])).rsp0 = priv_stack_top as u64;

        let df_stack_top = addr_of!(DOUBLE_FAULT_STACKS[cpu])
            .cast::<u8>()
            .add(INTERRUPT_STACK_SIZE);
        (*addr_of_mut!(TSS_ARRAY[cpu])).ist1 = df_stack_top as u64;

        // IST2: dedicated NMI stack (hard-lockup watchdog).  Must be set up
        // before the IDT points the NMI vector at ist=2.
        let nmi_stack_top = addr_of!(NMI_STACKS[cpu])
            .cast::<u8>()
            .add(INTERRUPT_STACK_SIZE);
        (*addr_of_mut!(TSS_ARRAY[cpu])).ist2 = nmi_stack_top as u64;

        // Build TSS descriptor and write into this CPU's GDT.
        let tss_base = addr_of!(TSS_ARRAY[cpu]) as u64;
        #[allow(clippy::cast_possible_truncation)]
        let tss_limit = (size_of::<TaskStateSegment>() - 1) as u32;
        let (tss_low, tss_high) = make_tss_descriptor(tss_base, tss_limit);

        (*addr_of_mut!(GDT_ARRAY[cpu])).entries[5] = tss_low;
        (*addr_of_mut!(GDT_ARRAY[cpu])).entries[6] = tss_high;

        // Load this CPU's GDT.
        #[allow(clippy::cast_possible_truncation)]
        let gdt_ptr = GdtPointer {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: addr_of!(GDT_ARRAY[cpu]) as u64,
        };

        core::arch::asm!(
            "lgdt [{}]",
            in(reg) &raw const gdt_ptr,
            options(readonly, nostack, preserves_flags),
        );

        // Reload segment registers.
        reload_segments();

        // Load this CPU's TSS.
        // No need to clear busy bit — each CPU has its own fresh TSS descriptor.
        core::arch::asm!(
            "ltr {:x}",
            in(reg) TSS_SEL,
            options(nostack, preserves_flags),
        );
    }
}

/// Set up the STAR MSR for syscall/sysret on the current CPU.
///
/// This is a per-CPU MSR — each CPU must set it independently.
fn setup_star_msr() {
    // STAR[47:32] = kernel CS for SYSCALL  (0x08)
    // STAR[63:48] = base for SYSRET       (0x10)
    //   → SYSRET SS = 0x10 + 8 = 0x18 (user data)
    //   → SYSRET CS = 0x10 + 16 = 0x20 (user code)
    const IA32_STAR: u32 = 0xC000_0081;
    let star_value: u64 = (u64::from(KERNEL_CS) << 32) | (0x10_u64 << 48);
    // SAFETY: IA32_STAR is a valid MSR on all x86_64 CPUs.
    unsafe {
        cpu::wrmsr(IA32_STAR, star_value);
    }
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

/// Initialize the BSP's GDT and TSS, then load them.
///
/// Must be called exactly once during early boot, on the BSP (CPU 0).
///
/// # Safety
///
/// - Must be called in ring 0 with interrupts disabled.
/// - The stacks referenced by the TSS must remain valid for the lifetime
///   of the system.
pub unsafe fn init() {
    // SAFETY: CPU 0 is the BSP, we're the only code running.
    unsafe {
        init_cpu_gdt_tss(0);
    }
    setup_star_msr();
}

/// Update RSP0 in the current CPU's TSS (called by the scheduler on
/// context switch).
///
/// # Safety
///
/// Must be called with interrupts disabled or from within an interrupt
/// handler (otherwise a nested interrupt could see a half-written RSP0).
pub unsafe fn set_kernel_stack(stack_top: u64) {
    let cpu = crate::smp::current_cpu_index();
    // SAFETY: Called under a critical section; no concurrent access to
    // this CPU's TSS.  Using addr_of_mut! to avoid references to mutable
    // statics.
    unsafe {
        (*addr_of_mut!(TSS_ARRAY[cpu])).rsp0 = stack_top;
    }
}

/// Initialize and load the GDT and TSS for an Application Processor.
///
/// Each AP gets its own GDT and TSS with independent RSP0 and IST stacks.
/// This eliminates the shared-TSS race condition where concurrent interrupts
/// on different CPUs would corrupt each other's stacks.
///
/// Also sets up the STAR MSR for syscall/sysret, which is per-CPU.
///
/// # Safety
///
/// Must be called exactly once per AP during SMP bootstrap, with
/// interrupts disabled.  `cpu` must be the caller's CPU index.
pub unsafe fn init_for_ap(cpu: usize) {
    // SAFETY: Each AP initializes only its own GDT/TSS slot.
    unsafe {
        init_cpu_gdt_tss(cpu);
    }
    setup_star_msr();
}

/// Reload CS, DS, ES, SS, FS, GS after loading a new GDT.
///
/// CS requires a far return; the data segments are loaded with `mov`.
///
/// # Safety
///
/// The GDT must already be loaded with valid descriptors at the offsets
/// used here.
unsafe fn reload_segments() {
    // SAFETY: GDT was just loaded with valid kernel code/data selectors.
    unsafe {
        core::arch::asm!(
            // Push the new CS selector and the return address, then far-return.
            "push {kcs:r}",       // New CS
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",         // Return address
            "retfq",              // Far return → loads CS
            "2:",
            // Reload data segment registers.
            "mov ds, {kds:x}",
            "mov es, {kds:x}",
            "mov ss, {kds:x}",
            "xor {zero:r}, {zero:r}",
            "mov fs, {zero:x}",
            "mov gs, {zero:x}",
            kcs = in(reg) u64::from(KERNEL_CS),
            kds = in(reg) KERNEL_DS & !3_u16, // strip RPL bits (0x10 for kernel data)
            tmp = out(reg) _,
            zero = out(reg) _,
            options(preserves_flags),
        );
    }
}
