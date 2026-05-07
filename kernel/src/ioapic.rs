//! I/O APIC driver — external device interrupt routing.
//!
//! The IOAPIC routes external device interrupts (keyboard, mouse, disk,
//! network, etc.) to Local APICs.  Each IOAPIC has up to 24 redirection
//! table entries, each mapping one IRQ pin to an interrupt vector
//! delivered to a specific LAPIC.
//!
//! ## Architecture
//!
//! ISA IRQs 0–15 correspond to IOAPIC inputs 0–15 on standard PC
//! hardware.  We map IOAPIC input N to IDT vector `IRQ_VECTOR_BASE + N`
//! (33 + N), keeping vector 32 reserved for the LAPIC timer.  All
//! entries start **masked**; drivers unmask individual IRQ lines when
//! they are ready to handle interrupts.
//!
//! ## IRQ Notification Model
//!
//! Device interrupts use a two-phase notification:
//!
//! 1. **ISR phase** (interrupt context, no locks): increment an atomic
//!    counter for the IRQ line, send EOI to the LAPIC.  Optionally
//!    attempt a lock-free wake of the registered driver task.
//! 2. **Deferred phase** (timer tick): the timer ISR scans for pending
//!    IRQ notifications and wakes any registered blocked tasks using
//!    `try_lock` (safe in ISR context).  This bounds worst-case wake
//!    latency to one timer period (~10 ms at 100 Hz).
//!
//! The driver task blocks via `SYS_IRQ_WAIT` (to be implemented),
//! which checks the pending counter.  If nonzero, it consumes the
//! count and returns immediately.  If zero, the task blocks.
//!
//! ## 8259 PIC Disable
//!
//! On initialization, the legacy 8259 PIC is remapped and fully masked
//! to prevent it from delivering interrupts that conflict with the
//! IOAPIC/LAPIC system.
//!
//! ## References
//!
//! - Intel 82093AA I/O APIC datasheet
//! - OSDev wiki: <https://wiki.osdev.org/IOAPIC>
//! - Based on Linux `arch/x86/kernel/apic/io_apic.c` register access
//!   patterns.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::PhysFrame;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::port;
use crate::serial_println;

// ---------------------------------------------------------------------------
// IOAPIC MMIO constants
// ---------------------------------------------------------------------------

/// Standard IOAPIC physical base address (82093AA default, same in QEMU).
///
/// Used as fallback when ACPI MADT is not available.
const IOAPIC_DEFAULT_PHYS: u64 = 0xFEC0_0000;

/// IOREGSEL register offset — write the indirect register index here.
const IOREGSEL: u32 = 0x00;

/// IOWIN register offset — read/write the selected indirect register.
const IOWIN: u32 = 0x10;

// ---------------------------------------------------------------------------
// IOAPIC indirect register indices
// ---------------------------------------------------------------------------

/// IOAPIC ID register (bits [27:24] = APIC ID).
const REG_ID: u8 = 0x00;

/// IOAPIC version register.
/// Bits [7:0]   = version number.
/// Bits [23:16] = maximum redirection entry index (0-based).
const REG_VER: u8 = 0x01;

/// Base index of the redirection table.  Entry N occupies two
/// consecutive 32-bit registers:
/// - Low  32 bits: `REG_REDTBL_BASE + 2*N`
/// - High 32 bits: `REG_REDTBL_BASE + 2*N + 1`
const REG_REDTBL_BASE: u8 = 0x10;

// ---------------------------------------------------------------------------
// Redirection table entry bits
// ---------------------------------------------------------------------------

/// Mask bit (bit 16) — when set, the interrupt is suppressed.
const REDIR_MASKED: u64 = 1 << 16;

/// Trigger mode (bit 15): 1 = level-triggered, 0 = edge-triggered.
#[allow(dead_code)]
const REDIR_LEVEL_TRIGGER: u64 = 1 << 15;

/// Pin polarity (bit 13): 1 = active-low, 0 = active-high.
#[allow(dead_code)]
const REDIR_ACTIVE_LOW: u64 = 1 << 13;

// Delivery mode (bits 10:8): 000 = Fixed.  Other modes (lowest
// priority, SMI, NMI, INIT, ExtINT) are not used.  Destination mode
// (bit 11): 0 = Physical.  Both default to 0, so we don't define
// separate constants — just leave those bits clear.

// ---------------------------------------------------------------------------
// Vector assignment
// ---------------------------------------------------------------------------

/// Base IDT vector for IOAPIC-routed external interrupts.
/// IOAPIC input N → vector `IRQ_VECTOR_BASE + N`.
///
/// Vector 32 is the LAPIC timer, so external IRQs start at 33.
pub const IRQ_VECTOR_BASE: u8 = 33;

/// Maximum number of IOAPIC redirection entries (standard 82093AA).
pub const MAX_IRQ: usize = 24;

// ---------------------------------------------------------------------------
// 8259 PIC I/O ports
// ---------------------------------------------------------------------------

/// Master PIC command port.
const PIC1_CMD: u16 = 0x20;
/// Master PIC data port.
const PIC1_DATA: u16 = 0x21;
/// Slave PIC command port.
const PIC2_CMD: u16 = 0xA0;
/// Slave PIC data port.
const PIC2_DATA: u16 = 0xA1;

/// ICW1: initialize + expect ICW4.
const ICW1_INIT_ICW4: u8 = 0x11;
/// ICW4: 8086/88 mode.
const ICW4_8086: u8 = 0x01;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Virtual address of the IOAPIC MMIO base (set during init).
static IOAPIC_BASE_VIRT: AtomicU64 = AtomicU64::new(0);

/// Number of usable redirection entries (read from hardware at init).
static NUM_REDIR_ENTRIES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// IRQ notification state (lock-free, safe from ISR context)
// ---------------------------------------------------------------------------

/// Per-IRQ pending interrupt counter.
///
/// The ISR increments this atomically.  A driver task reads and resets
/// it via [`irq_consume`] or the `SYS_IRQ_WAIT` syscall.
static IRQ_PENDING: [AtomicU64; MAX_IRQ] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_IRQ]
};

/// Per-IRQ registered task ID for deferred wakeup.
///
/// `u64::MAX` means no task is registered.  Set by
/// [`irq_register_task`], cleared by [`irq_unregister_task`].
static IRQ_WAIT_TASK: [AtomicU64; MAX_IRQ] = {
    const NONE: AtomicU64 = AtomicU64::new(u64::MAX);
    [NONE; MAX_IRQ]
};

// ---------------------------------------------------------------------------
// IOAPIC register access
// ---------------------------------------------------------------------------

/// Read a 32-bit IOAPIC register via the indirect register window.
///
/// # Safety
///
/// `IOAPIC_BASE_VIRT` must be initialized.  `reg` must be a valid
/// IOAPIC register index.
unsafe fn ioapic_read(reg: u8) -> u32 {
    let base = IOAPIC_BASE_VIRT.load(Ordering::Relaxed);
    debug_assert!(base != 0, "IOAPIC not initialized");
    // SAFETY: IOAPIC MMIO is mapped at `base`.  IOREGSEL and IOWIN
    // offsets are within the mapped 16 KiB frame.  Volatile access
    // is required for MMIO.
    unsafe {
        let sel = (base.wrapping_add(u64::from(IOREGSEL))) as *mut u32;
        let win = (base.wrapping_add(u64::from(IOWIN))) as *mut u32;
        core::ptr::write_volatile(sel, u32::from(reg));
        core::ptr::read_volatile(win)
    }
}

/// Write a 32-bit IOAPIC register via the indirect register window.
///
/// # Safety
///
/// `IOAPIC_BASE_VIRT` must be initialized.  `reg` must be a valid
/// IOAPIC register index.
unsafe fn ioapic_write(reg: u8, value: u32) {
    let base = IOAPIC_BASE_VIRT.load(Ordering::Relaxed);
    debug_assert!(base != 0, "IOAPIC not initialized");
    // SAFETY: Same as `ioapic_read` — MMIO register access.
    unsafe {
        let sel = (base.wrapping_add(u64::from(IOREGSEL))) as *mut u32;
        let win = (base.wrapping_add(u64::from(IOWIN))) as *mut u32;
        core::ptr::write_volatile(sel, u32::from(reg));
        core::ptr::write_volatile(win, value);
    }
}

/// Read a 64-bit redirection table entry.
///
/// # Safety
///
/// IOAPIC must be initialized.  `irq` must be < `NUM_REDIR_ENTRIES`.
unsafe fn read_redir_entry(irq: u8) -> u64 {
    // Each entry occupies two consecutive 32-bit indirect registers.
    // SAFETY: Caller guarantees irq is valid.
    unsafe {
        let lo_reg = REG_REDTBL_BASE.wrapping_add(irq.wrapping_mul(2));
        let hi_reg = lo_reg.wrapping_add(1);
        let lo = ioapic_read(lo_reg);
        let hi = ioapic_read(hi_reg);
        u64::from(lo) | (u64::from(hi) << 32)
    }
}

/// Write a 64-bit redirection table entry.
///
/// # Safety
///
/// IOAPIC must be initialized.  `irq` must be < `NUM_REDIR_ENTRIES`.
#[allow(clippy::cast_possible_truncation)]
unsafe fn write_redir_entry(irq: u8, entry: u64) {
    // Write high DWORD first (destination), then low DWORD (vector +
    // flags).  Writing low last ensures the entry is complete before
    // potentially unmasking.
    // SAFETY: Caller guarantees irq is valid.
    unsafe {
        let lo_reg = REG_REDTBL_BASE.wrapping_add(irq.wrapping_mul(2));
        let hi_reg = lo_reg.wrapping_add(1);
        // Intentional truncation: extracting the high and low 32-bit
        // halves of a 64-bit redirection entry.
        ioapic_write(hi_reg, (entry >> 32) as u32);
        ioapic_write(lo_reg, entry as u32);
    }
}

// ---------------------------------------------------------------------------
// 8259 PIC management
// ---------------------------------------------------------------------------

/// Disable the legacy 8259 PIC by remapping and fully masking all lines.
///
/// The PIC is remapped first (so any pending or spurious interrupts
/// don't alias CPU exception vectors 0–15), then all IRQs are masked.
///
/// # Safety
///
/// PIC I/O ports must be accessible (standard PC hardware).
unsafe fn disable_pic() {
    serial_println!("[ioapic] Disabling legacy 8259 PIC...");

    // SAFETY: Standard 8259 PIC programming sequence.  Port I/O
    // delays via io_wait() give the hardware time to process each
    // command word.
    unsafe {
        // ICW1: Initialize + expect ICW4.
        port::outb(PIC1_CMD, ICW1_INIT_ICW4);
        port::io_wait();
        port::outb(PIC2_CMD, ICW1_INIT_ICW4);
        port::io_wait();

        // ICW2: Vector offset.
        // Master PIC: vectors 0x20–0x27 (32–39).
        // Slave PIC:  vectors 0x28–0x2F (40–47).
        // These won't actually be used — all IRQs are masked below.
        port::outb(PIC1_DATA, 0x20);
        port::io_wait();
        port::outb(PIC2_DATA, 0x28);
        port::io_wait();

        // ICW3: Master/slave wiring.
        port::outb(PIC1_DATA, 0x04); // Slave on IRQ2.
        port::io_wait();
        port::outb(PIC2_DATA, 0x02); // Cascade identity.
        port::io_wait();

        // ICW4: 8086/88 mode.
        port::outb(PIC1_DATA, ICW4_8086);
        port::io_wait();
        port::outb(PIC2_DATA, ICW4_8086);
        port::io_wait();

        // Mask ALL IRQ lines on both PICs.
        port::outb(PIC1_DATA, 0xFF);
        port::outb(PIC2_DATA, 0xFF);
    }

    serial_println!("[ioapic] Legacy PIC disabled (all IRQs masked)");
}

// ---------------------------------------------------------------------------
// Public API — initialization
// ---------------------------------------------------------------------------

/// Initialize the I/O APIC.
///
/// 1. Disables the legacy 8259 PIC.
/// 2. Maps the IOAPIC MMIO registers into the kernel address space.
/// 3. Reads the IOAPIC version and max redirection entries.
/// 4. Programs all redirection entries: masked, edge-triggered,
///    active-high, fixed delivery to BSP (LAPIC ID 0), vector
///    `IRQ_VECTOR_BASE + N`.
///
/// After init, all IRQ lines are masked.  Call [`unmask_irq`] to
/// enable delivery for specific IRQ lines once a driver is ready.
///
/// # Errors
///
/// Returns [`KernelError::NotSupported`] if the HHDM is not initialized,
/// or [`KernelError::BadAlignment`] if the IOAPIC base address is not
/// frame-aligned.
///
/// # Safety
///
/// - Must be called exactly once during boot.
/// - The LAPIC must already be initialized (for EOI routing).
/// - Interrupts should be disabled during initialization.
pub unsafe fn init() -> KernelResult<()> {
    serial_println!("[ioapic] Initializing I/O APIC...");

    // Step 1: Disable the legacy 8259 PIC.
    // SAFETY: Standard PC hardware, I/O ports are accessible.
    unsafe {
        disable_pic();
    }

    // Step 2: Map the IOAPIC MMIO region.
    // Use the ACPI MADT if available, otherwise fall back to the
    // standard default (0xFEC0_0000).
    let ioapic_base_phys = crate::acpi::io_apic_address().unwrap_or_else(|| {
        serial_println!("[ioapic] No ACPI MADT — using default address {:#x}", IOAPIC_DEFAULT_PHYS);
        IOAPIC_DEFAULT_PHYS
    });

    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let ioapic_virt = ioapic_base_phys.wrapping_add(hhdm);

    let ioapic_frame = PhysFrame::from_addr(ioapic_base_phys)
        .ok_or(KernelError::BadAlignment)?;
    let ioapic_va = VirtAddr::new(ioapic_virt);
    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    // SAFETY: IOAPIC physical address is valid device MMIO at the
    // well-known address 0xFEC0_0000.  Mapping into the HHDM range.
    if let Err(e) = unsafe {
        page_table::map_frame(pml4_phys, ioapic_va, ioapic_frame, mmio_flags)
    } {
        serial_println!(
            "[ioapic] WARNING: MMIO map failed ({:?}), trying existing HHDM...",
            e,
        );
    } else {
        // Flush TLB for the new mapping.
        // SAFETY: Standard invlpg for the virtual address we just mapped.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) ioapic_virt,
                options(nostack, preserves_flags),
            );
        }
        serial_println!("[ioapic] MMIO mapped at {:#x}", ioapic_virt);
    }

    IOAPIC_BASE_VIRT.store(ioapic_virt, Ordering::Release);

    // Step 3: Read IOAPIC identification and version.
    // SAFETY: IOAPIC MMIO is mapped and accessible.
    let raw_id = unsafe { ioapic_read(REG_ID) };
    let id = raw_id >> 24;
    let ver_reg = unsafe { ioapic_read(REG_VER) };
    let version = ver_reg & 0xFF;
    let max_entry = (ver_reg >> 16) & 0xFF;
    // Count of entries = max index + 1.
    let num_entries = max_entry.wrapping_add(1);

    serial_println!(
        "[ioapic] ID={}, version={:#x}, {} redirection entries",
        id,
        version,
        num_entries,
    );

    // Clamp to our compile-time MAX_IRQ.
    let usable = if (num_entries as usize) > MAX_IRQ {
        MAX_IRQ
    } else {
        num_entries as usize
    };
    NUM_REDIR_ENTRIES.store(usable as u64, Ordering::Release);

    // Step 4: Program all redirection table entries.
    //
    // Default: vector = IRQ_VECTOR_BASE + N, fixed delivery,
    // physical destination mode, edge-triggered, active-high,
    // destination = LAPIC ID 0 (BSP), masked.
    //
    // ACPI interrupt source overrides may change the trigger mode
    // and polarity for specific IRQ lines (e.g., ISA IRQ 0 → GSI 2,
    // ISA IRQ 9 → level-triggered active-low).
    let overrides = crate::acpi::interrupt_overrides();

    for irq in 0..usable {
        #[allow(clippy::cast_possible_truncation)]
        let irq_u8 = irq as u8;
        let vector = u64::from(IRQ_VECTOR_BASE.wrapping_add(irq_u8));
        // Destination LAPIC ID 0 in bits [63:56].
        let mut entry = vector | REDIR_MASKED;

        // Check if this IOAPIC input has an ACPI interrupt source
        // override that changes the trigger mode or polarity.
        // The override GSI tells us which IOAPIC pin to configure,
        // but the trigger/polarity flags must be applied to the
        // redirection entry for that pin.
        for ovr in &overrides {
            if ovr.gsi as usize == irq {
                if ovr.is_level_triggered() {
                    entry |= REDIR_LEVEL_TRIGGER;
                }
                if ovr.is_active_low() {
                    entry |= REDIR_ACTIVE_LOW;
                }
                break;
            }
        }

        // SAFETY: IOAPIC is initialized, irq < num_entries.
        unsafe {
            write_redir_entry(irq_u8, entry);
        }
    }

    serial_println!(
        "[ioapic] {} redirection entries programmed (all masked)",
        usable,
    );
    if !overrides.is_empty() {
        serial_println!(
            "[ioapic] Applied {} ACPI interrupt source override(s)",
            overrides.len(),
        );
    }
    serial_println!(
        "[ioapic] IRQ vector range: {}–{}",
        IRQ_VECTOR_BASE,
        IRQ_VECTOR_BASE as usize + usable - 1,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — IRQ mask/unmask
// ---------------------------------------------------------------------------

/// Unmask (enable) an IOAPIC IRQ line.
///
/// The IRQ will be delivered to the BSP as vector
/// `IRQ_VECTOR_BASE + irq`.
///
/// # Safety
///
/// The corresponding IDT entry must have an ISR that handles this IRQ
/// and sends EOI.  Unmasking without a handler causes an unhandled
/// interrupt (double fault).
pub unsafe fn unmask_irq(irq: u8) {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    if u64::from(irq) >= num {
        serial_println!("[ioapic] WARNING: unmask_irq({}) out of range", irq);
        return;
    }

    // SAFETY: IOAPIC is initialized, irq < num_entries.
    unsafe {
        let entry = read_redir_entry(irq);
        write_redir_entry(irq, entry & !REDIR_MASKED);
    }

    serial_println!(
        "[ioapic] IRQ {} unmasked → vector {}",
        irq,
        IRQ_VECTOR_BASE.wrapping_add(irq),
    );
}

/// Mask (disable) an IOAPIC IRQ line.
pub fn mask_irq(irq: u8) {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    if u64::from(irq) >= num {
        return;
    }

    // SAFETY: IOAPIC is initialized, irq < num_entries.
    unsafe {
        let entry = read_redir_entry(irq);
        write_redir_entry(irq, entry | REDIR_MASKED);
    }
}

/// Configure a redirection entry for level-triggered, active-low
/// delivery (typical for PCI interrupts).
///
/// The entry remains masked; call [`unmask_irq`] separately.
///
/// # Safety
///
/// IOAPIC must be initialized.
pub unsafe fn set_level_triggered(irq: u8) {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    if u64::from(irq) >= num {
        return;
    }

    // SAFETY: IOAPIC is initialized, irq < num_entries.
    unsafe {
        let entry = read_redir_entry(irq);
        write_redir_entry(irq, entry | REDIR_LEVEL_TRIGGER | REDIR_ACTIVE_LOW);
    }
}

/// Set the CPU affinity for an IRQ — route it to a specific LAPIC.
///
/// This modifies the destination field (bits 63:56) of the redirection
/// table entry to route the interrupt to the specified LAPIC ID.  The
/// IRQ's mask/trigger/polarity settings are preserved.
///
/// Use this to:
/// - Spread interrupt load across CPUs (network, storage).
/// - Pin a device interrupt to the same CPU as its driver thread
///   (improved cache locality for ISR → driver wake path).
///
/// # Arguments
///
/// - `irq`: The IOAPIC input pin number (0..MAX_IRQ).
/// - `lapic_id`: The target Local APIC ID.  Use
///   [`crate::smp::cpu_apic_id(cpu_index)`] to convert a CPU index.
///
/// # Safety
///
/// The target LAPIC must be valid and online.  Routing to an offline
/// CPU causes lost interrupts.  IOAPIC must be initialized.
#[allow(dead_code)] // API for drivers zone; used when IRQ balancing is implemented.
pub unsafe fn set_irq_affinity(irq: u8, lapic_id: u8) {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    if u64::from(irq) >= num {
        serial_println!("[ioapic] WARNING: set_irq_affinity({}) out of range", irq);
        return;
    }

    // SAFETY: IOAPIC is initialized, irq < num_entries.
    // Destination LAPIC ID occupies bits 63:56 of the 64-bit entry.
    unsafe {
        let entry = read_redir_entry(irq);
        // Clear old destination (bits 63:56), set new one.
        let entry = (entry & 0x00FF_FFFF_FFFF_FFFF) | (u64::from(lapic_id) << 56);
        write_redir_entry(irq, entry);
    }

    serial_println!(
        "[ioapic] IRQ {} affinity → LAPIC ID {}",
        irq, lapic_id,
    );
}

/// Get the current CPU target (LAPIC ID) for an IRQ.
///
/// Returns the destination LAPIC ID from the redirection table entry,
/// or `None` if the IRQ is out of range.
#[must_use]
#[allow(dead_code)] // API for drivers zone; paired with set_irq_affinity.
pub fn get_irq_affinity(irq: u8) -> Option<u8> {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    if u64::from(irq) >= num {
        return None;
    }

    // SAFETY: IOAPIC is initialized, irq < num_entries.
    let entry = unsafe { read_redir_entry(irq) };
    // Destination LAPIC ID is in bits 63:56.
    #[allow(clippy::cast_possible_truncation)]
    Some((entry >> 56) as u8)
}

// ---------------------------------------------------------------------------
// IRQ notification — lock-free, safe from ISR context
// ---------------------------------------------------------------------------

/// Notify that an IRQ has fired.
///
/// Called from the device ISR assembly stub via [`handle_device_irq`].
/// This is the only operation performed in interrupt context — no
/// locks, just an atomic increment.
pub fn irq_notify(irq: u32) {
    if let Some(slot) = IRQ_PENDING.get(irq as usize) {
        slot.fetch_add(1, Ordering::Release);
    }
}

/// Consume (read and reset) the pending IRQ counter.
///
/// Returns the number of interrupts that fired since the last consume.
/// Used by the `SYS_IRQ_WAIT` syscall handler.
#[must_use]
pub fn irq_consume(irq: u32) -> u64 {
    if let Some(slot) = IRQ_PENDING.get(irq as usize) {
        slot.swap(0, Ordering::AcqRel)
    } else {
        0
    }
}

/// Check if an IRQ has pending interrupts (without consuming).
#[must_use]
#[allow(dead_code)] // Public API for polling-mode IRQ status checks.
pub fn irq_is_pending(irq: u32) -> bool {
    IRQ_PENDING
        .get(irq as usize)
        .is_some_and(|slot| slot.load(Ordering::Acquire) > 0)
}

/// Register a task to be woken when the specified IRQ fires.
///
/// Only one task can wait on each IRQ at a time.  Returns `false`
/// if the IRQ number is out of range.
pub fn irq_register_task(irq: u32, task_id: u64) -> bool {
    if let Some(slot) = IRQ_WAIT_TASK.get(irq as usize) {
        slot.store(task_id, Ordering::Release);
        true
    } else {
        false
    }
}

/// Unregister the waiting task for an IRQ.
pub fn irq_unregister_task(irq: u32) {
    if let Some(slot) = IRQ_WAIT_TASK.get(irq as usize) {
        slot.store(u64::MAX, Ordering::Release);
    }
}

/// Release all IRQ registrations belonging to a specific task.
///
/// Called during task cleanup (e.g., driver process exit) to ensure
/// no dangling registrations remain.  Scans all 24 IRQ lines — O(1)
/// for the fixed-size table.
///
/// For each IRQ owned by `task_id`, unmasks are NOT reversed (the
/// device may need re-initialization by the replacement driver).
/// Only the wait-task registration is cleared so the dead task won't
/// be spuriously woken.
pub fn release_irqs_for_task(task_id: u64) {
    for slot in &IRQ_WAIT_TASK {
        // CAS to avoid clearing a slot that was re-registered by
        // another task between load and store.
        let _ = slot.compare_exchange(
            task_id,
            u64::MAX,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }
}

/// Process deferred IRQ wake-ups.
///
/// Scans all IRQ lines for pending notifications and tries to wake
/// the registered task for each.  Uses [`crate::sched::try_wake`]
/// (try_lock) so this is safe in ISR context.
///
/// Called from the LAPIC timer ISR ([`crate::apic::handle_timer_irq`])
/// as a fallback for IRQs whose immediate wake attempt failed (e.g.,
/// because the scheduler lock was held by interrupted code).
///
/// On a single CPU at 100 Hz, scanning 24 atomic loads adds ~50 ns
/// per timer tick — negligible overhead.
pub fn process_deferred_wakes() {
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire) as usize;
    let limit = if num < MAX_IRQ { num } else { MAX_IRQ };

    for i in 0..limit {
        if IRQ_PENDING[i].load(Ordering::Acquire) > 0 {
            let task_id = IRQ_WAIT_TASK[i].load(Ordering::Acquire);
            if task_id != u64::MAX {
                // Best-effort wake.  If try_lock fails, the next
                // timer tick will retry.
                let _ = crate::sched::try_wake(task_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Device IRQ handler (called from ISR assembly stubs in idt.rs)
// ---------------------------------------------------------------------------

/// Common handler for all external device IRQs (vectors 33–56).
///
/// Called by the per-IRQ assembly stubs generated in `idt.rs`.  The
/// stub passes the IOAPIC input number (0–23) in RDI (first argument,
/// System V ABI).
///
/// This handler:
/// 1. Runs device-specific ISR work (keyboard scan code read, virtio ack).
/// 2. Increments the atomic pending counter for the IRQ.
/// 3. Tries to wake the registered task (lock-free attempt).
/// 4. If wake fails, raises `IRQ_POLL_SOFTIRQ` for deferred retry.
/// 5. Sends End-of-Interrupt (EOI) to the Local APIC.
/// 6. Processes pending softirqs with interrupts re-enabled.
///
/// ## Softirq integration
///
/// Previously a failed `try_wake` meant the driver task would sleep
/// until the next timer tick (~10 ms worst case) retried the wake.
/// Now the softirq mechanism retries immediately after EOI with
/// interrupts enabled, reducing worst-case wake latency from ~10 ms
/// to ~microseconds.
#[unsafe(no_mangle)]
pub extern "C" fn handle_device_irq(irq: u32) {
    // --- CPU time accounting: entering IRQ context ---
    crate::cputime::enter_irq();

    crate::ktrace::record(
        crate::ktrace::Category::Irq,
        crate::ktrace::event::IRQ_ENTER,
        irq as u64,
        crate::sched::current_cpu_id() as u64,
    );

    // 0. Device-specific handlers that must run in ISR context.
    //    The keyboard must read its scan code from port 0x60
    //    immediately — the data is lost after EOI.
    if irq == 1 {
        crate::keyboard::handle_scancode();
    } else if irq == 12 {
        crate::mouse::handle_irq();
    }

    // PCI shared IRQ handling: acknowledge all virtio devices that
    // may share this IRQ line.  Reading each device's ISR status
    // register deasserts its interrupt signal — required for level-
    // triggered PCI interrupts.  Each handler checks the IRQ line
    // first (two atomic loads, ~1 ns) and only performs the I/O port
    // read if this IRQ matches the device.
    crate::virtio::blk::handle_irq(irq);
    crate::virtio::net::handle_irq(irq);
    crate::rtl8139::handle_irq(irq);

    // 1. Record the interrupt (for IRQ storm detection and notification).
    irq_notify(irq);
    crate::irq_storm::record_irq(irq);

    // 2. Fast path: try immediate wake of the registered task.
    //    If the wake attempt fails (scheduler lock contention), raise
    //    a softirq so the wake is retried immediately after EOI with
    //    interrupts re-enabled (instead of waiting up to 10 ms for the
    //    next timer tick).
    if let Some(slot) = IRQ_WAIT_TASK.get(irq as usize) {
        let task_id = slot.load(Ordering::Acquire);
        if task_id != u64::MAX {
            if !crate::sched::try_wake(task_id) {
                crate::softirq::raise(crate::softirq::IRQ_POLL_SOFTIRQ);
            }
        }
    }

    // 3. EOI to the Local APIC.
    //
    // For edge-triggered ISA interrupts, LAPIC EOI is sufficient.
    // For level-triggered (PCI), the LAPIC forwards the EOI to the
    // IOAPIC via the directed-EOI mechanism.
    //
    // SAFETY: We're in an interrupt handler, LAPIC is initialized.
    unsafe {
        crate::apic::eoi();
    }

    // 4. Process pending softirqs (if any were raised above or by
    //    other ISRs).  This re-enables interrupts internally.
    //
    // SAFETY: EOI has been sent, assembly stub expects CLI on return.
    unsafe {
        crate::softirq::process_pending();
    }

    crate::ktrace::record(
        crate::ktrace::Category::Irq,
        crate::ktrace::event::IRQ_EXIT,
        irq as u64,
        crate::sched::current_cpu_id() as u64,
    );

    // --- CPU time accounting: leaving IRQ context ---
    crate::cputime::exit_irq();
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify IOAPIC initialization is correct.
///
/// Checks:
/// 1. IOAPIC MMIO base is set.
/// 2. All redirection entries are masked with correct vectors.
/// 3. IRQ notification counters are zeroed.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[ioapic] Running self-test...");

    // Test 1: IOAPIC is initialized.
    let base = IOAPIC_BASE_VIRT.load(Ordering::Acquire);
    if base == 0 {
        serial_println!("[ioapic]   FAIL: IOAPIC base not set");
        return Err(KernelError::InternalError);
    }
    serial_println!("[ioapic]   MMIO base: {:#x} (OK)", base);

    // Test 2: All entries are masked with correct vectors.
    let num = NUM_REDIR_ENTRIES.load(Ordering::Acquire);
    for i in 0..num {
        #[allow(clippy::cast_possible_truncation)]
        let irq = i as u8;
        // SAFETY: IOAPIC is initialized, irq < num_entries.
        let entry = unsafe { read_redir_entry(irq) };

        // Check masked.
        if entry & REDIR_MASKED == 0 {
            serial_println!("[ioapic]   FAIL: IRQ {} not masked", irq);
            return Err(KernelError::InternalError);
        }

        // Check vector.
        let expected_vec = u64::from(IRQ_VECTOR_BASE.wrapping_add(irq));
        if entry & 0xFF != expected_vec {
            serial_println!(
                "[ioapic]   FAIL: IRQ {} vector mismatch (got {}, want {})",
                irq,
                entry & 0xFF,
                expected_vec,
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!(
        "[ioapic]   {} redirection entries: all masked, correct vectors",
        num,
    );

    // Test 3: Notification counters are all zero.
    for i in 0..MAX_IRQ {
        if IRQ_PENDING[i].load(Ordering::Acquire) != 0 {
            serial_println!(
                "[ioapic]   FAIL: IRQ {} has nonzero pending count",
                i,
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[ioapic]   IRQ notification counters: all zero");

    serial_println!("[ioapic] Self-test PASSED");
    Ok(())
}
