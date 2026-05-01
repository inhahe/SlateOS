//! Symmetric Multi-Processing (SMP) bootstrap.
//!
//! Wakes Application Processors (APs) from their INIT state using the
//! standard INIT-SIPI-SIPI IPI sequence, and brings each AP through
//! 16-bit real mode → 32-bit protected mode → 64-bit long mode into
//! the kernel.
//!
//! ## AP Bootstrap Sequence
//!
//! 1. BSP copies a small trampoline to physical address 0x8000.
//! 2. BSP temporarily identity-maps 0x0..0x200000 so the trampoline
//!    code can execute while paging is being set up.
//! 3. For each AP discovered in the MADT:
//!    a. BSP patches the trampoline data area with the AP's stack, PML4,
//!       entry point, and CPU index.
//!    b. BSP sends INIT IPI → 10 ms delay → SIPI → 200 µs → SIPI.
//!    c. BSP spins waiting for the AP to set its "started" flag.
//!    d. AP executes trampoline: real → protected → long mode, jumps
//!       to `ap_entry()` in the kernel.
//!    e. AP loads GDT, IDT, enables APIC timer, enters scheduler.
//! 4. BSP removes the identity mapping and updates the scheduler with
//!    the actual CPU count.
//!
//! ## Per-CPU Data
//!
//! Each CPU has a `PerCpuData` record stored in a static array indexed
//! by sequential CPU number (0 = BSP, 1+ = APs).  The LAPIC ID → CPU
//! index mapping is stored in a separate lookup table.
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §8.4 "Multiple-Processor (MP) Initialization"
//! - OSDev wiki: <https://wiki.osdev.org/Symmetric_Multiprocessing>
//! - Based on Linux `arch/x86/kernel/smpboot.c` AP bootstrap pattern.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum CPUs supported (matches `PerCpuScheduler::MAX_CPUS`).
pub const MAX_CPUS: usize = 16;

/// IA32_TSC_AUX MSR — stores per-CPU data readable by `rdtscp`.
///
/// We write the CPU index here during SMP init, enabling O(1) CPU
/// identification without APIC MMIO on the heap allocator hot path.
/// Based on Linux's use of IA32_TSC_AUX for `__getcpu()`.
const IA32_TSC_AUX: u32 = 0xC000_0103;

/// Whether rdtscp is available on this CPU (CPUID.80000001H:EDX[27]).
static RDTSCP_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Whether the RDPID instruction is available (CPUID.07H.0H:ECX[22]).
///
/// RDPID reads IA32_TSC_AUX directly into a GP register without touching
/// the TSC, avoiding the serialization and TSC-read overhead of rdtscp.
/// ~10 cycles vs ~30-40 cycles for rdtscp on Coffee Lake.
static RDPID_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Physical address where the AP trampoline is copied.
/// Must be below 1 MiB, 4 KiB aligned.  0x8000 is conventionally used
/// and avoids conflicts with BDA (0x400), EBDA, and VGA memory.
const TRAMPOLINE_PHYS: u64 = 0x8000;

/// SIPI vector = TRAMPOLINE_PHYS / 0x1000.
#[allow(clippy::cast_possible_truncation)]
const SIPI_VECTOR: u8 = (TRAMPOLINE_PHYS / 0x1000) as u8;

/// Size of per-AP kernel stack (32 KiB — 2 of our 16 KiB frames).
const AP_STACK_SIZE: usize = 32 * 1024;

/// Timeout waiting for an AP to start (in PIT-calibrated busy-loop
/// iterations).  ~200 ms under QEMU.
const AP_STARTUP_TIMEOUT: u64 = 200_000_000;

// ---------------------------------------------------------------------------
// Trampoline data area offsets (from trampoline base at 0x8000)
// ---------------------------------------------------------------------------

/// Offset of PML4 physical address (8 bytes).
const DATA_PML4: usize = 0x300;
/// Offset of AP entry point virtual address (8 bytes).
const DATA_ENTRY: usize = 0x308;
/// Offset of AP kernel stack top virtual address (8 bytes).
const DATA_STACK: usize = 0x310;
/// Offset of CPU index for this AP (4 bytes).
const DATA_CPU_IDX: usize = 0x318;
/// Offset of "AP started" flag (4 bytes, set to 1 by AP).
const DATA_STARTED: usize = 0x31C;

// ---------------------------------------------------------------------------
// Per-CPU data
// ---------------------------------------------------------------------------

/// Number of online CPUs (starts at 1 for the BSP).
static NUM_CPUS_ONLINE: AtomicU32 = AtomicU32::new(1);

/// LAPIC ID → sequential CPU index mapping (lock-free).
///
/// Index: APIC ID (0–255).  Value: CPU index (0xFF = unmapped).
///
/// Lock-free to avoid deadlock: `current_cpu_index()` is called from
/// the timer ISR (via `sched::timer_tick()` → `current_cpu_id()`).
/// If this were behind a spinlock and the ISR fired while the lock was
/// held on the same CPU, the non-reentrant spinlock would deadlock.
///
/// Writes happen only during SMP bootstrap (before APs start their
/// timers).  `SMP_INITIALIZED` (Release/Acquire) provides the
/// happens-before guarantee: all `store(Relaxed)` writes to this
/// table are visible to any reader that sees `SMP_INITIALIZED == true`
/// via `load(Acquire)`.
static APIC_TO_CPU: [AtomicU8; 256] = {
    const UNMAPPED: AtomicU8 = AtomicU8::new(0xFF);
    [UNMAPPED; 256]
};

/// Reverse mapping: CPU index → APIC ID.
///
/// Index: sequential CPU number (0 = BSP).  Value: APIC ID (0xFF = unmapped).
/// Populated during SMP bootstrap alongside `APIC_TO_CPU`.
/// Used by `send_fixed_ipi` to target a specific CPU.
static CPU_TO_APIC: [AtomicU8; MAX_CPUS] = {
    const UNMAPPED: AtomicU8 = AtomicU8::new(0xFF);
    [UNMAPPED; MAX_CPUS]
};

/// Whether SMP has been initialized.
static SMP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// BSP's sequential CPU index (always 0).
const BSP_CPU_INDEX: usize = 0;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the number of online CPUs.
#[must_use]
pub fn cpu_count() -> usize {
    NUM_CPUS_ONLINE.load(Ordering::Relaxed) as usize
}

/// Get the APIC ID for a CPU index.
///
/// Returns `None` if the CPU index is out of range or not yet online.
/// Used by the reschedule IPI mechanism to target a specific CPU.
#[must_use]
pub fn cpu_apic_id(cpu_index: usize) -> Option<u8> {
    let id = CPU_TO_APIC.get(cpu_index)?.load(Ordering::Relaxed);
    if id == 0xFF { None } else { Some(id) }
}

/// Get the current CPU's sequential index.
///
/// Returns 0 (BSP) if SMP has not been initialized.
///
/// # Performance
///
/// OPT: When `rdtscp` is available (set at init), reads the CPU index
/// directly from IA32_TSC_AUX (~20 cycles) instead of APIC MMIO
/// (~100+ cycles under virtualization).  This is the hot path for the
/// per-CPU heap slab cache, per-CPU frame cache, and timer ISR.
/// Based on Linux's use of IA32_TSC_AUX for fast CPU identification.
///
/// This function is lock-free and safe to call from ISR context
/// (timer interrupt, IPI handlers, etc.).
#[must_use]
#[inline]
pub fn current_cpu_index() -> usize {
    if !SMP_INITIALIZED.load(Ordering::Acquire) {
        return 0;
    }

    // Fast path: read CPU index from IA32_TSC_AUX via rdtscp.
    // rdtscp returns TSC in EDX:EAX (discarded) and IA32_TSC_AUX in ECX.
    if RDTSCP_AVAILABLE.load(Ordering::Relaxed) {
        let cpu_idx: u32;
        // SAFETY: rdtscp is available (checked above).  Reading TSC_AUX
        // is always safe.  We wrote the CPU index there during SMP init.
        unsafe {
            core::arch::asm!(
                "rdtscp",
                out("ecx") cpu_idx,
                out("eax") _,    // TSC low — discard
                out("edx") _,    // TSC high — discard
                options(nomem, nostack, preserves_flags),
            );
        }
        return cpu_idx as usize;
    }

    // Fallback: APIC MMIO read (slower, always works).
    let apic_id = crate::apic::read_id();
    let idx = APIC_TO_CPU[apic_id as usize].load(Ordering::Relaxed);
    if idx == 0xFF { 0 } else { idx as usize }
}

/// Fast CPU index for hot paths (heap allocator, frame allocator).
///
/// **Must only be called when per-CPU infrastructure is enabled**
/// (i.e., after SMP init completes), which is guaranteed by the
/// `PCPU_SLAB_ENABLED` / `PCPU_ENABLED` guards in the callers.
/// Skips the `SMP_INITIALIZED` check that `current_cpu_index()` does.
///
/// # Performance
///
/// OPT: Saves ~10-20 cycles per call vs `current_cpu_index()` by
/// eliminating the SMP_INITIALIZED atomic load + branch.  When RDPID
/// is available (Coffee Lake+), saves ~20-30 more cycles by reading
/// IA32_TSC_AUX without touching the TSC.  On the heap alloc+dealloc
/// hot path (called twice), the combined savings are ~40-100 cycles.
///
/// Tiered fast paths:
/// 1. RDPID available → ~10 cycles (no TSC read, no serialization)
/// 2. rdtscp available → ~30-40 cycles (reads TSC too, but no APIC MMIO)
/// 3. APIC MMIO fallback → ~100+ cycles (always works)
#[must_use]
#[inline(always)]
pub fn fast_cpu_index() -> usize {
    // Tier 1: RDPID — reads IA32_TSC_AUX directly into a GP register.
    // Cheapest option: no TSC read, no serialization.
    if RDPID_AVAILABLE.load(Ordering::Relaxed) {
        let cpu_idx: u64;
        // SAFETY: RDPID is available (CPUID check at boot).
        // IA32_TSC_AUX was written with the CPU index during SMP init.
        unsafe {
            core::arch::asm!(
                // RDPID r64: opcode F3 0F C7 /7 (mod=11, reg=7, rm=reg)
                // Encoded as REP RDPID using the F3 prefix.
                ".byte 0xF3, 0x0F, 0xC7, 0xF8",  // rdpid rax
                out("rax") cpu_idx,
                options(nomem, nostack, preserves_flags),
            );
        }
        return cpu_idx as usize;
    }

    // Tier 2: rdtscp — reads IA32_TSC_AUX + TSC.  We discard the TSC
    // but can't avoid reading it.
    if RDTSCP_AVAILABLE.load(Ordering::Relaxed) {
        let cpu_idx: u32;
        // SAFETY: rdtscp is available (checked above).
        unsafe {
            core::arch::asm!(
                "rdtscp",
                out("ecx") cpu_idx,
                out("eax") _,    // TSC low — discard
                out("edx") _,    // TSC high — discard
                options(nomem, nostack, preserves_flags),
            );
        }
        return cpu_idx as usize;
    }

    // Tier 3: APIC MMIO — slowest but always works.
    let apic_id = crate::apic::read_id();
    let idx = APIC_TO_CPU[apic_id as usize].load(Ordering::Relaxed);
    if idx == 0xFF { 0 } else { idx as usize }
}

/// Check if SMP bootstrap has completed.
#[must_use]
#[allow(dead_code)] // Will be used by per-CPU data accessors.
pub fn is_initialized() -> bool {
    SMP_INITIALIZED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// AP trampoline binary
// ---------------------------------------------------------------------------

/// Build the AP trampoline code as a byte array.
///
/// The trampoline runs at physical address TRAMPOLINE_PHYS (0x8000).
/// It transitions from 16-bit real mode → 32-bit protected mode →
/// 64-bit long mode, then jumps to the AP kernel entry point.
///
/// Layout:
///   0x000 – 0x0FF: 16-bit real mode code
///   0x100 – 0x1FF: 32-bit protected mode code
///   0x200 – 0x2FF: 64-bit long mode code
///   0x300 – 0x3FF: Data area (patched per-AP by BSP)
fn build_trampoline() -> [u8; 1024] {
    let mut buf = [0u8; 1024];
    let base = TRAMPOLINE_PHYS;

    // ===== 16-bit real mode code (offset 0x000) =====
    let mut p = 0usize;

    // cli
    buf[p] = 0xFA; p += 1;
    // cld
    buf[p] = 0xFC; p += 1;
    // xor ax, ax
    buf[p] = 0x31; buf[p+1] = 0xC0; p += 2;
    // mov ds, ax
    buf[p] = 0x8E; buf[p+1] = 0xD8; p += 2;
    // mov es, ax
    buf[p] = 0x8E; buf[p+1] = 0xC0; p += 2;
    // mov ss, ax
    buf[p] = 0x8E; buf[p+1] = 0xD0; p += 2;
    // mov sp, 0x7C00  (temporary stack below trampoline)
    buf[p] = 0xBC; p += 1;
    buf[p] = 0x00; buf[p+1] = 0x7C; p += 2;

    // lgdt [gdt16_ptr]  — 16-bit absolute address mode
    // The GDT pointer is at base + 0x340 (trampoline data area).
    // In 16-bit mode with DS=0: lgdt [disp16]
    // Opcode: 0F 01 16 <disp16>
    let gdt16_ptr = (base + 0x340) as u16;
    buf[p] = 0x0F; buf[p+1] = 0x01; buf[p+2] = 0x16; p += 3;
    buf[p] = (gdt16_ptr & 0xFF) as u8; buf[p+1] = (gdt16_ptr >> 8) as u8; p += 2;

    // mov eax, cr0  (operand-size prefix + mov eax, cr0)
    // In 16-bit: 66 0F 20 C0
    buf[p] = 0x66; buf[p+1] = 0x0F; buf[p+2] = 0x20; buf[p+3] = 0xC0; p += 4;
    // or al, 1  (set PE bit)
    buf[p] = 0x0C; buf[p+1] = 0x01; p += 2;
    // mov cr0, eax  (operand-size prefix)
    // 66 0F 22 C0
    buf[p] = 0x66; buf[p+1] = 0x0F; buf[p+2] = 0x22; buf[p+3] = 0xC0; p += 4;

    // Far jump to 32-bit code at base+0x100, selector 0x08
    // In 16-bit: EA <offset16> <selector16>
    // But our target address is 0x8100 which is > 0xFFFF in 16-bit offset.
    // We need an operand-size prefix for 32-bit offset: 66 EA <offset32> <sel16>
    let target32 = (base + 0x100) as u32;
    buf[p] = 0x66; p += 1;  // operand-size prefix for 32-bit offset
    buf[p] = 0xEA; p += 1;  // far jmp
    // offset32 (little-endian)
    buf[p]   = (target32 & 0xFF) as u8;
    buf[p+1] = ((target32 >> 8) & 0xFF) as u8;
    buf[p+2] = ((target32 >> 16) & 0xFF) as u8;
    buf[p+3] = ((target32 >> 24) & 0xFF) as u8;
    p += 4;
    // selector16 (little-endian)
    buf[p] = 0x08; buf[p+1] = 0x00;

    // ===== 32-bit protected mode code (offset 0x100) =====
    p = 0x100;

    // mov ax, 0x10  (data segment selector)
    // In 32-bit: 66 B8 10 00
    buf[p] = 0x66; buf[p+1] = 0xB8; buf[p+2] = 0x10; buf[p+3] = 0x00; p += 4;
    // mov ds, ax
    buf[p] = 0x8E; buf[p+1] = 0xD8; p += 2;
    // mov es, ax
    buf[p] = 0x8E; buf[p+1] = 0xC0; p += 2;
    // mov ss, ax
    buf[p] = 0x8E; buf[p+1] = 0xD0; p += 2;

    // Enable PAE in CR4 (required for long mode)
    // mov eax, cr4: 0F 20 E0
    buf[p] = 0x0F; buf[p+1] = 0x20; buf[p+2] = 0xE0; p += 3;
    // or eax, 0x20: 83 C8 20  (or eax, imm8)
    // Actually: or eax, imm32 = 0D 20 00 00 00
    buf[p] = 0x0D; p += 1;
    buf[p] = 0x20; buf[p+1] = 0x00; buf[p+2] = 0x00; buf[p+3] = 0x00; p += 4;
    // mov cr4, eax: 0F 22 E0
    buf[p] = 0x0F; buf[p+1] = 0x22; buf[p+2] = 0xE0; p += 3;

    // Load PML4 into CR3 from trampoline data area
    // mov eax, [base + DATA_PML4]: A1 <addr32>
    let pml4_data_addr = (base as u32) + (DATA_PML4 as u32);
    buf[p] = 0xA1; p += 1;
    buf[p]   = (pml4_data_addr & 0xFF) as u8;
    buf[p+1] = ((pml4_data_addr >> 8) & 0xFF) as u8;
    buf[p+2] = ((pml4_data_addr >> 16) & 0xFF) as u8;
    buf[p+3] = ((pml4_data_addr >> 24) & 0xFF) as u8;
    p += 4;
    // mov cr3, eax: 0F 22 D8
    buf[p] = 0x0F; buf[p+1] = 0x22; buf[p+2] = 0xD8; p += 3;

    // Enable long mode + NX support via IA32_EFER MSR.
    // Bit 8 (LME) = Long Mode Enable.
    // Bit 11 (NXE) = No-Execute Enable — required for NX bit in page tables.
    // Without NXE, bit 63 in PTEs is reserved and will cause #PF(RSVD).
    // mov ecx, 0xC0000080: B9 80 00 00 C0
    buf[p] = 0xB9; p += 1;
    buf[p] = 0x80; buf[p+1] = 0x00; buf[p+2] = 0x00; buf[p+3] = 0xC0; p += 4;
    // rdmsr: 0F 32
    buf[p] = 0x0F; buf[p+1] = 0x32; p += 2;
    // or eax, 0x900 (set LME bit 8 + NXE bit 11)
    // 0D 00 09 00 00
    buf[p] = 0x0D; p += 1;
    buf[p] = 0x00; buf[p+1] = 0x09; buf[p+2] = 0x00; buf[p+3] = 0x00; p += 4;
    // wrmsr: 0F 30
    buf[p] = 0x0F; buf[p+1] = 0x30; p += 2;

    // Enable paging (activates long mode since LME is set)
    // mov eax, cr0: 0F 20 C0
    buf[p] = 0x0F; buf[p+1] = 0x20; buf[p+2] = 0xC0; p += 3;
    // or eax, 0x80000000 (set PG bit): 0D 00 00 00 80
    buf[p] = 0x0D; p += 1;
    buf[p] = 0x00; buf[p+1] = 0x00; buf[p+2] = 0x00; buf[p+3] = 0x80; p += 4;
    // mov cr0, eax: 0F 22 C0
    buf[p] = 0x0F; buf[p+1] = 0x22; buf[p+2] = 0xC0; p += 3;

    // Load 64-bit GDT
    // lgdt [gdt64_ptr]: 0F 01 15 <disp32>
    let gdt64_ptr = (base as u32) + 0x368;
    buf[p] = 0x0F; buf[p+1] = 0x01; buf[p+2] = 0x15; p += 3;
    buf[p]   = (gdt64_ptr & 0xFF) as u8;
    buf[p+1] = ((gdt64_ptr >> 8) & 0xFF) as u8;
    buf[p+2] = ((gdt64_ptr >> 16) & 0xFF) as u8;
    buf[p+3] = ((gdt64_ptr >> 24) & 0xFF) as u8;
    p += 4;

    // Far jump to 64-bit code at base+0x200, selector 0x08
    // In 32-bit compatibility mode: EA <offset32> <selector16>
    let target64 = (base + 0x200) as u32;
    buf[p] = 0xEA; p += 1;
    buf[p]   = (target64 & 0xFF) as u8;
    buf[p+1] = ((target64 >> 8) & 0xFF) as u8;
    buf[p+2] = ((target64 >> 16) & 0xFF) as u8;
    buf[p+3] = ((target64 >> 24) & 0xFF) as u8;
    p += 4;
    buf[p] = 0x08; buf[p+1] = 0x00;

    // ===== 64-bit long mode code (offset 0x200) =====
    p = 0x200;

    // mov ax, 0x10  (data segment)
    // In 64-bit: 66 B8 10 00
    buf[p] = 0x66; buf[p+1] = 0xB8; buf[p+2] = 0x10; buf[p+3] = 0x00; p += 4;
    // mov ds, ax: 8E D8
    buf[p] = 0x8E; buf[p+1] = 0xD8; p += 2;
    // mov es, ax: 8E C0
    buf[p] = 0x8E; buf[p+1] = 0xC0; p += 2;
    // mov ss, ax: 8E D0
    buf[p] = 0x8E; buf[p+1] = 0xD0; p += 2;
    // xor ax, ax: 66 31 C0
    buf[p] = 0x66; buf[p+1] = 0x31; buf[p+2] = 0xC0; p += 3;
    // mov fs, ax: 8E E0
    buf[p] = 0x8E; buf[p+1] = 0xE0; p += 2;
    // mov gs, ax: 8E E8
    buf[p] = 0x8E; buf[p+1] = 0xE8; p += 2;

    // Load stack pointer from trampoline data area.
    // Since we have identity mapping AND the HHDM, the data is at the
    // identity-mapped address (low physical).
    // mov rsp, [base + DATA_STACK]
    // REX.W + MOV rsp, [rip+disp32] or REX.W + MOV rsp, [disp32]
    // Using absolute addressing: 48 8B 24 25 <disp32>
    let stack_addr = (base as u32) + (DATA_STACK as u32);
    // REX.W mov rsp, [disp32]: 48 8B 24 25 <addr32>
    // Actually: mov rsp, qword [addr]
    // 48 A1 <addr64> would be "mov rax, [moffs64]" — only rax.
    // For rsp: 48 8B 24 25 <disp32> = mov rsp, [sib] with base=none, index=none
    buf[p] = 0x48; buf[p+1] = 0x8B; buf[p+2] = 0x24; buf[p+3] = 0x25; p += 4;
    buf[p]   = (stack_addr & 0xFF) as u8;
    buf[p+1] = ((stack_addr >> 8) & 0xFF) as u8;
    buf[p+2] = ((stack_addr >> 16) & 0xFF) as u8;
    buf[p+3] = ((stack_addr >> 24) & 0xFF) as u8;
    p += 4;

    // Set the "AP started" flag to 1.
    // mov dword [base + DATA_STARTED], 1
    // C7 04 25 <addr32> 01 00 00 00
    let started_addr = (base as u32) + (DATA_STARTED as u32);
    buf[p] = 0xC7; buf[p+1] = 0x04; buf[p+2] = 0x25; p += 3;
    buf[p]   = (started_addr & 0xFF) as u8;
    buf[p+1] = ((started_addr >> 8) & 0xFF) as u8;
    buf[p+2] = ((started_addr >> 16) & 0xFF) as u8;
    buf[p+3] = ((started_addr >> 24) & 0xFF) as u8;
    p += 4;
    buf[p] = 0x01; buf[p+1] = 0x00; buf[p+2] = 0x00; buf[p+3] = 0x00; p += 4;

    // Load AP entry point and jump to it.
    // mov rax, [base + DATA_ENTRY]: 48 A1 <addr64>
    // Actually 48 A1 is only for moffs64 which is 8 bytes address in 64-bit.
    // In 64-bit mode, "MOV RAX, moffs64" = A1 + 8-byte absolute address.
    // But with REX.W: 48 A1 <addr64>
    let entry_addr = (base as u64) + (DATA_ENTRY as u64);
    buf[p] = 0x48; buf[p+1] = 0xA1; p += 2;
    buf[p]   = (entry_addr & 0xFF) as u8;
    buf[p+1] = ((entry_addr >> 8) & 0xFF) as u8;
    buf[p+2] = ((entry_addr >> 16) & 0xFF) as u8;
    buf[p+3] = ((entry_addr >> 24) & 0xFF) as u8;
    buf[p+4] = ((entry_addr >> 32) & 0xFF) as u8;
    buf[p+5] = ((entry_addr >> 40) & 0xFF) as u8;
    buf[p+6] = ((entry_addr >> 48) & 0xFF) as u8;
    buf[p+7] = ((entry_addr >> 56) & 0xFF) as u8;
    p += 8;

    // jmp rax: FF E0
    buf[p] = 0xFF; buf[p+1] = 0xE0;
    // p += 2;

    // ===== Data area (offset 0x300) =====
    // Patched per-AP by the BSP before sending SIPI.

    // 0x300: PML4 physical address (8 bytes)  — filled by patch_trampoline
    // 0x308: AP entry point virtual addr (8 bytes) — filled by patch_trampoline
    // 0x310: AP stack top virtual addr (8 bytes) — filled by patch_trampoline
    // 0x318: CPU index (4 bytes) — filled by patch_trampoline
    // 0x31C: AP started flag (4 bytes) — set to 1 by AP code above

    // 0x320: 32-bit temporary GDT (3 entries)
    p = 0x320;
    // Entry 0: null descriptor
    let null_desc: u64 = 0;
    write_u64(&mut buf, p, null_desc); p += 8;
    // Entry 1 (selector 0x08): 32-bit code segment
    // P=1, DPL=0, S=1, type=0xA (code, exec/read), G=1, D=1
    let code32: u64 = 0x00CF_9A00_0000_FFFF;
    write_u64(&mut buf, p, code32); p += 8;
    // Entry 2 (selector 0x10): 32-bit data segment
    // P=1, DPL=0, S=1, type=0x2 (data, read/write), G=1, D=1
    let data32: u64 = 0x00CF_9200_0000_FFFF;
    write_u64(&mut buf, p, data32);
    // p += 8;

    // 0x340: GDT pointer for 16-bit lgdt (6 bytes: limit16 + base32)
    p = 0x340;
    let gdt32_base = (base + 0x320) as u32;
    // limit = 3*8 - 1 = 23
    buf[p] = 23; buf[p+1] = 0; p += 2;
    // base (32-bit, little-endian)
    buf[p]   = (gdt32_base & 0xFF) as u8;
    buf[p+1] = ((gdt32_base >> 8) & 0xFF) as u8;
    buf[p+2] = ((gdt32_base >> 16) & 0xFF) as u8;
    buf[p+3] = ((gdt32_base >> 24) & 0xFF) as u8;
    // p += 4;

    // 0x350: 64-bit GDT (3 entries)
    p = 0x350;
    // Entry 0: null
    write_u64(&mut buf, p, 0); p += 8;
    // Entry 1 (selector 0x08): 64-bit code segment
    // P=1, DPL=0, S=1, type=0xA (code, exec/read), L=1, D=0
    let code64: u64 = 0x00AF_9A00_0000_FFFF;
    write_u64(&mut buf, p, code64); p += 8;
    // Entry 2 (selector 0x10): 64-bit data segment
    let data64: u64 = 0x00CF_9200_0000_FFFF;
    write_u64(&mut buf, p, data64);
    // p += 8;

    // 0x368: GDT pointer for 64-bit lgdt (6 bytes in 32-bit mode)
    // NOTE: The 64-bit GDT entries occupy 0x350-0x367 (3 × 8 bytes).
    // The GDT pointer must NOT overlap with the GDT entries.
    // In 32-bit compatibility mode, lgdt loads a 6-byte pseudo-descriptor:
    // 2 bytes limit + 4 bytes base.  Since we're still identity-mapped,
    // the base is the physical address of the 64-bit GDT.
    p = 0x368;
    let gdt64_base = (base + 0x350) as u32;
    buf[p] = 23; buf[p+1] = 0; p += 2;
    buf[p]   = (gdt64_base & 0xFF) as u8;
    buf[p+1] = ((gdt64_base >> 8) & 0xFF) as u8;
    buf[p+2] = ((gdt64_base >> 16) & 0xFF) as u8;
    buf[p+3] = ((gdt64_base >> 24) & 0xFF) as u8;

    buf
}

/// Write a u64 in little-endian to the buffer at the given offset.
#[allow(clippy::indexing_slicing)]
fn write_u64(buf: &mut [u8], off: usize, val: u64) {
    let bytes = val.to_le_bytes();
    buf[off..off + 8].copy_from_slice(&bytes);
}

/// Patch the trampoline data area for a specific AP.
fn patch_trampoline(
    tramp_virt: *mut u8,
    pml4_phys: u64,
    entry_virt: u64,
    stack_top: u64,
    cpu_index: u32,
) {
    // SAFETY: tramp_virt points to a mapped page with at least 1024 bytes.
    unsafe {
        // PML4 physical address
        let dst = tramp_virt.add(DATA_PML4);
        core::ptr::write_volatile(dst.cast::<u64>(), pml4_phys);

        // AP entry point (kernel virtual address)
        let dst = tramp_virt.add(DATA_ENTRY);
        core::ptr::write_volatile(dst.cast::<u64>(), entry_virt);

        // AP stack top (kernel virtual address)
        let dst = tramp_virt.add(DATA_STACK);
        core::ptr::write_volatile(dst.cast::<u64>(), stack_top);

        // CPU index
        let dst = tramp_virt.add(DATA_CPU_IDX);
        core::ptr::write_volatile(dst.cast::<u32>(), cpu_index);

        // Clear the started flag
        let dst = tramp_virt.add(DATA_STARTED);
        core::ptr::write_volatile(dst.cast::<u32>(), 0);
    }
}

/// Read the AP started flag from the trampoline data area.
fn read_started_flag(tramp_virt: *const u8) -> u32 {
    // SAFETY: tramp_virt points to mapped memory.
    unsafe {
        core::ptr::read_volatile(tramp_virt.add(DATA_STARTED).cast::<u32>())
    }
}

// ---------------------------------------------------------------------------
// Identity mapping for trampoline
// ---------------------------------------------------------------------------

/// Add an identity mapping for the trampoline page(s) so the AP can
/// execute the trampoline with paging enabled.
///
/// Maps physical 0x0000..0x10000 (64 KiB = 16 hardware pages) to
/// virtual 0x0000..0x10000 in the kernel's PML4.
///
/// Returns `Ok(())` on success.
///
/// # Safety
///
/// PML4 must be the active page table.  Must be called with interrupts
/// disabled.
unsafe fn setup_identity_mapping(pml4_phys: u64) -> KernelResult<()> {
    use crate::mm::page_table::{PageFlags, VirtAddr, map_4k_if_absent};

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;

    // Map 16 hardware pages (0x0000..0x10000) identity.
    // This covers the trampoline at 0x8000 and its temporary stack area.
    for page_idx in 0..16u64 {
        let phys = page_idx * 4096;
        let virt = VirtAddr::new(phys);
        // SAFETY: We're adding an identity mapping for a known physical
        // range that contains the AP trampoline code.
        if let Err(e) = unsafe { map_4k_if_absent(pml4_phys, virt, phys, flags) } {
            serial_println!(
                "[smp] WARNING: identity map for {:#x} failed: {:?}",
                phys, e
            );
            return Err(e);
        }
    }

    // Flush TLB for the mapped range.
    for page_idx in 0..16u64 {
        let virt = page_idx * 4096;
        // SAFETY: Standard TLB invalidation.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) virt,
                options(nostack, preserves_flags),
            );
        }
    }

    serial_println!("[smp] Identity mapping 0x0..0x10000 established");
    Ok(())
}

/// Remove the identity mapping added by `setup_identity_mapping`.
///
/// We only need to clear the PML4[0] entry (which covers the entire
/// lower 512 GiB).  The intermediate tables (PDPT, PD, PT) are leaked
/// (a few KiB) — acceptable for a one-time operation.
///
/// # Safety
///
/// Must be called after all APs have started and left the trampoline.
unsafe fn remove_identity_mapping(pml4_phys: u64) {
    let hhdm = crate::mm::page_table::hhdm().unwrap_or(0);
    let pml4_virt = pml4_phys + hhdm;

    // Clear PML4 entry 0 (covers virtual 0x0..0x80_0000_0000).
    // SAFETY: pml4_virt is valid, entry 0 was set by our identity mapping.
    unsafe {
        let entry_ptr = pml4_virt as *mut u64;
        core::ptr::write_volatile(entry_ptr, 0);
    }

    // Flush TLB.
    // SAFETY: Standard TLB invalidation for the low memory range.
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) 0u64,
            options(nostack, preserves_flags),
        );
        // Full TLB flush via CR3 reload is more thorough.
        let cr3 = crate::mm::page_table::read_cr3();
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) cr3,
            options(nostack, preserves_flags),
        );
    }

    serial_println!("[smp] Identity mapping removed");
}

// ---------------------------------------------------------------------------
// AP kernel entry point
// ---------------------------------------------------------------------------

/// AP entry point — called by the trampoline after entering 64-bit mode.
///
/// At this point:
/// - We're in 64-bit mode with identity mapping + HHDM active
/// - RSP points to a per-AP kernel stack (in HHDM space)
/// - The trampoline data area has our CPU index
///
/// We need to:
/// 1. Load the kernel's GDT (with a per-AP TSS)
/// 2. Load the kernel's IDT
/// 3. Initialize the local APIC
/// 4. Register with the scheduler
/// 5. Enable interrupts
/// 6. Enter the idle loop
#[unsafe(no_mangle)]
extern "C" fn ap_entry() -> ! {
    // Read our CPU index from the trampoline data area.
    // This is still identity-mapped, so we can access it directly.
    let cpu_index = unsafe {
        let addr = (TRAMPOLINE_PHYS + DATA_CPU_IDX as u64) as *const u32;
        core::ptr::read_volatile(addr) as usize
    };

    serial_println!("[smp] AP {} entered kernel (64-bit mode)", cpu_index);

    // Initialize this AP's own GDT and TSS.
    //
    // Each CPU gets its own GDT (with a TSS descriptor pointing to its
    // own TSS) and its own TSS (with independent RSP0 and IST stacks).
    // This ensures concurrent interrupts on different CPUs don't corrupt
    // each other's stacks.
    //
    // SAFETY: cpu_index is our CPU index, interrupts are disabled.
    unsafe {
        crate::gdt::init_for_ap(cpu_index);
    }

    // Load the kernel's IDT.
    // All CPUs share the same IDT — interrupt handlers are the same.
    // SAFETY: IDT was set up by BSP.
    unsafe {
        crate::idt::load();
    }

    // Initialize the local APIC on this AP.
    // Reuses the BSP's calibrated timer value.
    // SAFETY: GDT and IDT are loaded, we're in a valid kernel context.
    unsafe {
        crate::apic::init_ap();
    }

    // Register this AP's APIC ID ↔ CPU index bidirectional mapping (lock-free).
    let apic_id = crate::apic::read_id();
    #[allow(clippy::cast_possible_truncation)]
    APIC_TO_CPU[apic_id as usize].store(cpu_index as u8, Ordering::Relaxed);
    CPU_TO_APIC[cpu_index].store(apic_id, Ordering::Relaxed);

    // Write CPU index to IA32_TSC_AUX for fast rdtscp-based lookup.
    if RDTSCP_AVAILABLE.load(Ordering::Relaxed) {
        // SAFETY: IA32_TSC_AUX exists when rdtscp is supported.
        unsafe { crate::cpu::wrmsr(IA32_TSC_AUX, cpu_index as u64); }
    }

    // Bump the online CPU count.
    NUM_CPUS_ONLINE.fetch_add(1, Ordering::Release);

    serial_println!(
        "[smp] AP {} online (LAPIC ID={}, {} CPUs total)",
        cpu_index, apic_id, NUM_CPUS_ONLINE.load(Ordering::Relaxed)
    );

    // Register this AP's idle task with the scheduler.
    //
    // Each CPU needs its own idle task — a fallback task that runs when
    // nothing else is ready.  Without this, CURRENT_TASK_IDS[cpu] would
    // default to 0 (the BSP's idle task), causing both CPUs to think
    // they're running the same task: schedule_inner would corrupt task 0's
    // saved context, and reap_dead_tasks could free task 0's stack while
    // the BSP is using it.
    //
    // Must be before sti() so the timer ISR sees a valid current task.
    let _idle_id = crate::sched::register_ap_idle(cpu_index);

    // Enable interrupts.  The APIC timer will start firing.
    // SAFETY: IDT is loaded, APIC is configured, idle task registered.
    unsafe {
        crate::cpu::sti();
    }

    // Enter the AP idle loop.
    //
    // The timer ISR calls preempt() on every tick, which runs
    // schedule_inner and switches to any ready task.  We do NOT call
    // yield_now() here — it would redundantly acquire the SCHED lock
    // (spinlock contention with other CPUs), re-enqueue the idle task,
    // pick it right back, and return.  That contention was measured at
    // ~4x regression on the context switch benchmark.
    //
    // Maintenance tasks (reap + refill) run at reduced frequency to
    // avoid lock thrashing: reap every ~1 second (100 ticks), refill
    // on every wake.
    let mut tick_counter = 0u32;
    loop {
        crate::cpu::hlt(); // Sleep until next interrupt (timer tick or IPI).

        tick_counter = tick_counter.wrapping_add(1);

        // If a reschedule IPI woke us (someone enqueued work for this
        // CPU), yield immediately to pick up the new task.  This gives
        // microsecond-level latency vs the 10ms timer tick interval.
        if crate::sched::reschedule_pending(cpu_index) {
            crate::sched::yield_now();
        }

        // Reap dead tasks once per second (~100 ticks at 100 Hz).
        // reap_dead_tasks allocates Vecs and acquires the SCHED lock
        // even when nothing is dead, so throttling reduces contention.
        if tick_counter.is_multiple_of(100) {
            crate::sched::reap_dead_tasks();
        }

        // Refill the pre-zeroed frame pool.  This doesn't contend on
        // the SCHED lock, only the frame allocator (per-CPU fast path).
        crate::mm::frame::refill_zero_pool();
    }
}

// ---------------------------------------------------------------------------
// SMP bootstrap (main init function)
// ---------------------------------------------------------------------------

/// Initialize SMP: discover APs via ACPI MADT and boot them.
///
/// Must be called after:
/// - ACPI tables parsed (for CPU discovery)
/// - APIC initialized (for IPI sending)
/// - Scheduler initialized (for per-CPU queues)
/// - Page tables initialized (for identity mapping)
///
/// Interrupts should be enabled (the BSP's timer is running).
pub fn init() {
    serial_println!("[smp] Starting SMP bootstrap...");

    // Discover processors from ACPI MADT.
    let processors = crate::acpi::processors();
    let bsp_apic = crate::apic::bsp_id();

    // Filter to enabled APs (exclude BSP).
    let aps: alloc::vec::Vec<_> = processors.iter()
        .filter(|p| p.enabled && p.apic_id != bsp_apic)
        .collect();

    let ap_count = aps.len();
    if ap_count == 0 {
        serial_println!("[smp] No APs found — single CPU system");
        register_bsp(bsp_apic);
        SMP_INITIALIZED.store(true, Ordering::Release);
        return;
    }

    if ap_count + 1 > MAX_CPUS {
        serial_println!(
            "[smp] WARNING: {} CPUs found but MAX_CPUS={}, limiting to {}",
            ap_count + 1, MAX_CPUS, MAX_CPUS
        );
    }
    let ap_count = ap_count.min(MAX_CPUS - 1);

    serial_println!(
        "[smp] BSP APIC ID={}, {} AP(s) to boot",
        bsp_apic, ap_count
    );

    // Register BSP in the APIC→CPU mapping.
    register_bsp(bsp_apic);

    // Get the PML4 physical address for the APs.
    let pml4_phys = crate::mm::page_table::cr3_to_pml4(
        crate::mm::page_table::read_cr3()
    );

    // Set up identity mapping so the trampoline can execute.
    // SAFETY: We're the BSP, pml4_phys is valid.
    if unsafe { setup_identity_mapping(pml4_phys) }.is_err() {
        serial_println!("[smp] FAILED to set up identity mapping — aborting SMP");
        SMP_INITIALIZED.store(true, Ordering::Release);
        return;
    }

    // Build the trampoline code.
    let trampoline = build_trampoline();

    // Copy trampoline to the target physical address via HHDM.
    let hhdm = crate::mm::page_table::hhdm().unwrap_or(0);
    let tramp_virt = (TRAMPOLINE_PHYS + hhdm) as *mut u8;

    // SAFETY: tramp_virt is a valid HHDM mapping of the trampoline page.
    unsafe {
        core::ptr::copy_nonoverlapping(
            trampoline.as_ptr(),
            tramp_virt,
            trampoline.len(),
        );
    }

    serial_println!("[smp] Trampoline copied to phys={:#x}", TRAMPOLINE_PHYS);

    // Get the AP entry point address.
    let ap_entry_virt = ap_entry as *const () as u64;

    // Boot each AP.
    let mut booted_count: usize = 0;
    for (i, ap) in aps.iter().take(ap_count).enumerate() {
        let cpu_index = (i + 1) as u32; // BSP = 0, first AP = 1

        // Allocate a kernel stack for this AP.
        let stack = alloc::vec![0u8; AP_STACK_SIZE];
        let stack_top = stack.as_ptr() as u64 + AP_STACK_SIZE as u64;

        // Patch the trampoline data area for this AP.
        patch_trampoline(
            tramp_virt,
            pml4_phys,
            ap_entry_virt,
            stack_top,
            cpu_index,
        );

        serial_println!(
            "[smp] Booting AP {} (APIC ID={}, stack_top={:#x})",
            cpu_index, ap.apic_id, stack_top
        );

        // Send INIT-SIPI-SIPI sequence.
        // SAFETY: APIC is initialized, trampoline is in place.
        unsafe {
            // INIT IPI.
            crate::apic::send_init_ipi(ap.apic_id);

            // Wait 10 ms (Intel SDM §8.4.4.1 recommends 10 ms after INIT).
            crate::cpu::delay_us(10_000);

            // First SIPI.
            crate::apic::send_sipi(ap.apic_id, SIPI_VECTOR);

            // Wait 200 µs.
            crate::cpu::delay_us(200);

            // Second SIPI (in case the first was lost — per Intel spec).
            crate::apic::send_sipi(ap.apic_id, SIPI_VECTOR);
        }

        // Wait for the AP to set its started flag.
        let mut started = false;
        for _ in 0..AP_STARTUP_TIMEOUT {
            if read_started_flag(tramp_virt as *const u8) != 0 {
                started = true;
                break;
            }
            core::hint::spin_loop();
        }

        if started {
            booted_count += 1;
            serial_println!("[smp] AP {} responded (APIC ID={})", cpu_index, ap.apic_id);
        } else {
            serial_println!(
                "[smp] WARNING: AP {} (APIC ID={}) did not respond — skipping",
                cpu_index, ap.apic_id
            );
        }

        if started {
            // Leak the stack allocation — the AP will use it for the
            // kernel's lifetime.  Without leak, the Vec would be dropped
            // when this iteration ends.
            core::mem::forget(stack);
        }
        // If the AP didn't start, the stack Vec is dropped normally.
    }

    // Remove the identity mapping now that all APs are started.
    // SAFETY: All APs have left the trampoline (identity-mapped) code
    // and are executing in the kernel's higher-half virtual address space.
    unsafe {
        remove_identity_mapping(pml4_phys);
    }

    // Wait for all APs to fully initialize and increment NUM_CPUS_ONLINE.
    //
    // The "started" flag is set early in the trampoline (before jumping to
    // ap_entry), but APs don't bump NUM_CPUS_ONLINE until after GDT/IDT/APIC
    // init.  We spin briefly here so the scheduler gets the correct count.
    let expected_cpus = (booted_count + 1) as u32; // +1 for BSP
    let wait_limit: u64 = 50_000_000; // ~50 ms
    for _ in 0..wait_limit {
        if NUM_CPUS_ONLINE.load(Ordering::Acquire) >= expected_cpus {
            break;
        }
        core::hint::spin_loop();
    }

    // Update the scheduler with the actual CPU count.
    let total_cpus = NUM_CPUS_ONLINE.load(Ordering::Acquire) as usize;
    // Re-initialize the per-CPU scheduler with the real CPU count.
    // This is safe because APs are in their idle loops (not touching
    // the scheduler yet) and the BSP is the only one calling this.
    crate::sched::update_cpu_count(total_cpus);

    SMP_INITIALIZED.store(true, Ordering::Release);

    serial_println!(
        "[smp] SMP bootstrap complete: {} CPU(s) online",
        total_cpus
    );
}

/// Register the BSP in the APIC→CPU index mapping, and detect/enable
/// rdtscp-based fast CPU identification.
fn register_bsp(bsp_apic_id: u8) {
    #[allow(clippy::cast_possible_truncation)]
    APIC_TO_CPU[bsp_apic_id as usize].store(BSP_CPU_INDEX as u8, Ordering::Relaxed);
    CPU_TO_APIC[BSP_CPU_INDEX].store(bsp_apic_id, Ordering::Relaxed);

    // Detect rdtscp support: CPUID.80000001H:EDX bit 27.
    let has_rdtscp = detect_rdtscp();
    if has_rdtscp {
        // Write BSP's CPU index to IA32_TSC_AUX so rdtscp returns it.
        // SAFETY: IA32_TSC_AUX is writable when rdtscp is supported.
        unsafe { crate::cpu::wrmsr(IA32_TSC_AUX, BSP_CPU_INDEX as u64); }
        RDTSCP_AVAILABLE.store(true, Ordering::Release);
        serial_println!("[smp] rdtscp available — fast CPU index via IA32_TSC_AUX");
    } else {
        serial_println!("[smp] rdtscp not available — using APIC MMIO for CPU index");
    }

    // Detect RDPID: CPUID.07H.0H:ECX bit 22.
    // RDPID reads IA32_TSC_AUX without touching the TSC (~10 vs ~30-40
    // cycles for rdtscp).  Available on Coffee Lake+ and Goldmont Plus+.
    if has_rdtscp {
        let has_rdpid = detect_rdpid();
        if has_rdpid {
            RDPID_AVAILABLE.store(true, Ordering::Release);
            serial_println!("[smp] rdpid available — ultra-fast CPU index (no TSC read)");
        }
    }
}

/// Detect rdtscp instruction support via CPUID.
///
/// Returns `true` if CPUID.80000001H:EDX bit 27 is set.
fn detect_rdtscp() -> bool {
    let edx: u32;
    // SAFETY: CPUID is always safe to execute in ring 0.
    // Note: CPUID clobbers EBX but LLVM reserves RBX, so we must
    // save/restore it manually via xchg with a spare register.
    unsafe {
        core::arch::asm!(
            "xchg rbx, {tmp}",   // save RBX
            "mov eax, 0x80000001",
            "cpuid",
            "xchg rbx, {tmp}",   // restore RBX
            tmp = out(reg) _,
            out("edx") edx,
            out("eax") _,
            out("ecx") _,
            options(nomem, nostack, preserves_flags),
        );
    }
    edx & (1 << 27) != 0
}

/// Detect RDPID instruction support via CPUID.
///
/// Returns `true` if CPUID.07H.0H:ECX bit 22 is set.
/// RDPID reads IA32_TSC_AUX into a GP register without touching the TSC.
/// Available on Intel Coffee Lake+, Goldmont Plus+, and AMD Zen2+.
fn detect_rdpid() -> bool {
    let ecx: u32;
    // SAFETY: CPUID is always safe to execute in ring 0.
    unsafe {
        core::arch::asm!(
            "xchg rbx, {tmp}",   // save RBX
            "mov eax, 7",        // leaf 7
            "xor ecx, ecx",     // subleaf 0
            "cpuid",
            "xchg rbx, {tmp}",   // restore RBX
            tmp = out(reg) _,
            out("ecx") ecx,
            out("eax") _,
            out("edx") _,
            options(nomem, nostack, preserves_flags),
        );
    }
    ecx & (1 << 22) != 0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify SMP infrastructure is working.
pub fn self_test() {
    serial_println!("[smp] Running self-test...");

    let total = cpu_count();
    serial_println!("[smp]   Online CPUs: {}", total);

    // Verify BSP can read its own CPU index.
    let my_idx = current_cpu_index();
    serial_println!("[smp]   BSP CPU index: {}", my_idx);
    assert!(my_idx == 0, "BSP should be CPU 0");

    // Verify BSP APIC ID is mapped.
    let bsp_apic = crate::apic::bsp_id();
    let mapped_idx = APIC_TO_CPU[bsp_apic as usize].load(Ordering::Relaxed);
    assert!(mapped_idx == 0, "BSP APIC ID should map to CPU 0");

    serial_println!("[smp] Self-test PASSED");
}
