//! Kernel entry point.
//!
//! This is a microkernel for x86_64 desktops.  Only the scheduler,
//! memory manager, IPC, capability enforcement, and interrupt routing
//! run in kernel space.  Everything else (drivers, filesystems,
//! networking) runs in userspace.
//!
//! ## Boot Sequence
//!
//! 1. Limine bootloader loads kernel ELF, sets up paging and long mode.
//! 2. `kmain()` — kernel entry point.
//! 3. Initialize serial console (debug output).
//! 4. Parse Limine boot info (memory map, HHDM offset).
//! 5. Set up GDT (with TSS for privilege transitions).
//! 6. Set up IDT (exception handlers).
//! 7. Initialize physical frame allocator from memory map.
//! 8. Initialize kernel heap.
//! 9. ... (scheduler, IPC, first userspace process)

#![no_std]
#![no_main]
// Lint configuration per CLAUDE.md coding standards.
#![deny(clippy::all, clippy::pedantic)]
#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
// Allow these lints in test code where panicking on bad data is expected.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )
)]

// Module declarations.
mod boot;
mod cap;
mod cpu;
mod error;
mod gdt;
mod idt;
mod ipc;
mod mm;
mod port;
mod proc;
mod sched;
mod security;
mod serial;
mod syscall;

// ---------------------------------------------------------------------------
// Kernel entry point
// ---------------------------------------------------------------------------

/// Kernel entry point, called by the Limine bootloader.
///
/// At this point we have:
/// - 64-bit long mode, paging enabled
/// - Identity map + HHDM for physical memory access
/// - Interrupts disabled
/// - BSS zeroed
/// - A temporary stack provided by the bootloader
#[no_mangle]
extern "C" fn kmain() -> ! {
    // Step 1: Initialize serial console for debug output.
    // This must be first so we can log everything that follows.
    //
    // SAFETY: COM1 is standard PC hardware, always present in QEMU.
    // Called exactly once.
    unsafe {
        serial::init();
    }

    serial_println!("=== Kernel booting ===");

    // Step 2: Parse boot information from Limine.
    let boot_info = match boot::parse_boot_info() {
        Some(info) => info,
        None => {
            serial_println!("FATAL: Failed to parse boot info from Limine");
            cpu::halt_loop();
        }
    };

    serial_println!("[boot] Boot info parsed successfully");
    serial_println!("[boot] HHDM offset: {:#x}", boot_info.hhdm_offset);

    // Step 3: Set up our own GDT (replacing the one Limine set up).
    //
    // SAFETY: We are in ring 0, interrupts are disabled, and this is
    // the only CPU running.
    unsafe {
        gdt::init();
    }
    serial_println!("[gdt] GDT and TSS initialized");

    // Step 4: Set up the IDT with exception handlers.
    //
    // SAFETY: GDT is loaded (the IDT references kernel CS).  We are
    // single-threaded during boot.
    unsafe {
        idt::init();
    }
    serial_println!("[idt] IDT initialized");

    // Step 5: Initialize the physical frame allocator.
    // TODO: Initialize buddy allocator from memory map.
    serial_println!("[mm] Physical frame allocator: TODO");

    // Step 6: Initialize the kernel heap.
    // TODO: Set up bump allocator (early boot) or slab allocator.
    serial_println!("[mm] Kernel heap allocator: TODO");

    // Step 7: Initialize the scheduler.
    // TODO: Set up run queues, timer interrupt.
    serial_println!("[sched] Scheduler: TODO");

    // Step 8: Initialize IPC subsystem.
    // TODO: Channel system, pipe system, eventfd.
    serial_println!("[ipc] IPC subsystem: TODO");

    // Step 9: Initialize capability system.
    // TODO: Root capability table.
    serial_println!("[cap] Capability system: TODO");

    // Boot success marker — the boot test script looks for this.
    serial_println!("BOOT_OK");
    serial_println!("=== Kernel boot complete ===");

    // Idle loop: halt until interrupt, repeat.
    // Once the scheduler is up, this becomes the idle task.
    loop {
        cpu::hlt();
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

/// Panic handler for the kernel.
///
/// Prints the panic info to serial and halts.  In a kernel, panics are
/// always fatal — there is no higher-level runtime to catch them.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Disable interrupts immediately — we don't want an interrupt
    // handler running on top of a panicking kernel.
    //
    // SAFETY: We're in a terminal error state; disabling interrupts
    // is the right thing to do.
    unsafe {
        cpu::cli();
    }

    serial_println!("!!! KERNEL PANIC !!!");
    serial_println!("{}", info);

    cpu::halt_loop();
}
