//! Kernel entry point.
//!
//! This is a microkernel for `x86_64` desktops.  Only the scheduler,
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
//! 9. Initialize page table subsystem.
//! 10. Initialize demand paging (page fault resolution).
//! 11. Initialize scheduler (priority round-robin, cooperative).
//! 12. Initialize IPC channels (self-test: send, recv, blocking, FIFO, backpressure).
//! 13. Initialize syscall dispatch (self-test: `yield`, `task_id`, channel roundtrip).
//! 14. Initialize futexes (self-test: value mismatch, no waiters, blocking wait/wake).
//! 15. Initialize pipes (self-test: basic IO, partial read, EOF, broken pipe, non-blocking, blocking).
//! 16. Initialize shared memory (self-test: create, write/read, zeroed, close frees frames).
//! 17. Initialize eventfd counters (self-test: initial value, accumulate, reset, non-blocking, blocking).
//! 18. Initialize completion port (self-test: create+poll, try-wait empty, notify+wake, unregister).
//! 19. Initialize capability system (self-test: insert, rights, duplicate, revoke, delegation).
//! 20. Set up SYSCALL/SYSRET MSRs (IA32_LSTAR entry point, IA32_FMASK,
//!     IA32_KERNEL_GS_BASE for per-CPU data via SWAPGS, IA32_EFER.SCE).
//! 21. Initialize process management (self-test: PCB create/lookup/destroy,
//!     thread lifecycle, capability integration, ELF parse/segments/BSS/flags/
//!     entry point, thread spawn/exit→zombie/reject zombie, process spawn from
//!     ELF/reject invalid/spawn with capabilities, ring 3 spawn+exit).
//! 22. Initialize Local APIC (calibrate timer via PIT, configure periodic mode
//!     at 100 Hz, register timer ISR on vector 32).
//! 23. Enable interrupts — preemptive scheduling is now active.
//!     (self-test: verify timer ticks are observed).
//! 24. ... (per-process address spaces, ring 3 transition, first userspace process)

#![no_std]
#![no_main]
// Lint configuration is in workspace Cargo.toml ([workspace.lints.clippy])
// and inherited via [lints] workspace = true in kernel/Cargo.toml.

extern crate alloc;

// Module declarations.
mod apic;
mod blkdev;
mod boot;
mod cap;
mod console;
mod cpu;
mod error;
mod font;
mod fs;
mod gdt;
mod idt;
mod ioapic;
mod ipc;
mod keyboard;
mod kshell;
mod limine;
mod mm;
mod net;
mod pci;
mod port;
mod proc;
mod rtc;
mod sched;
mod security;
mod serial;
mod syscall;
mod virtio;

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
#[unsafe(no_mangle)]
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
    let Some(boot_info) = boot::parse_boot_info() else {
        serial_println!("FATAL: Failed to parse boot info from Limine");
        cpu::halt_loop();
    };

    serial_println!("[boot] Boot info parsed successfully");
    serial_println!("[boot] HHDM offset: {:#x}", boot_info.hhdm_offset);

    // Step 2b: Initialize framebuffer console (if available).
    // The framebuffer is already mapped by Limine, so we can start
    // writing pixels immediately — no page tables or heap needed.
    // This gives us on-screen text output for the rest of boot.
    //
    // SAFETY: Limine guarantees the framebuffer address is a valid,
    // mapped virtual address covering at least height*pitch bytes.
    // Called exactly once.
    if let Some(ref fb) = boot_info.framebuffer {
        unsafe {
            console::init(fb.address, fb.width, fb.height, fb.pitch, fb.bpp);
        }
        console_println!("=== Framebuffer console active ===");
    }

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
    //
    // SAFETY: Boot info contains a valid memory map and HHDM offset from
    // Limine.  This is the first and only call to frame::init.  We are
    // single-threaded with interrupts disabled.
    if let Err(e) = unsafe {
        mm::frame::init(boot_info.hhdm_offset, boot_info.memory_map)
    } {
        serial_println!("FATAL: Frame allocator init failed: {}", e);
        cpu::halt_loop();
    }

    // Verify basic allocator functionality before proceeding.
    if let Err(e) = mm::frame::self_test() {
        serial_println!("FATAL: Frame allocator self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 6: Initialize the kernel heap.
    // The slab allocator uses the frame allocator for backing memory.
    mm::heap::init(boot_info.hhdm_offset);

    // Verify heap allocations work.
    if let Err(e) = mm::heap::self_test() {
        serial_println!("FATAL: Heap allocator self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 7: Initialize the page table subsystem.
    // This provides map/unmap/translate operations for managing virtual
    // address spaces.  Uses the HHDM to read/write page table entries.
    mm::page_table::init(boot_info.hhdm_offset);

    // Verify page table operations work (translate HHDM, map/unmap).
    if let Err(e) = mm::page_table::self_test() {
        serial_println!("FATAL: Page table self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 8: Initialize the page fault / demand paging subsystem.
    // This registers the kernel address space and enables the page
    // fault handler to resolve faults for demand-paged regions.
    mm::fault::init();

    // Verify demand paging works (register VMA, trigger fault, verify).
    if let Err(e) = mm::fault::self_test() {
        serial_println!("FATAL: Demand paging self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 9: Initialize the scheduler.
    // Creates the idle task (the current execution context) and sets up
    // the priority round-robin scheduler.  Timer-based preemption will
    // be added when the APIC timer is wired up (§2.2).
    sched::init();

    // Verify cooperative scheduling works (spawn tasks, yield, verify).
    if let Err(e) = sched::self_test() {
        serial_println!("FATAL: Scheduler self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 10: Initialize IPC subsystem.
    // Channels are the primary IPC mechanism — structured message
    // passing between tasks/processes.  No explicit init needed (the
    // global channel table is lazily populated).  Run self-tests to
    // verify send, recv, blocking, close detection, and backpressure.
    if let Err(e) = ipc::channel::self_test() {
        serial_println!("FATAL: IPC channel self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 11: Initialize syscall dispatch.
    // The versioned dispatch table maps syscall numbers to handlers.
    // No explicit init needed (table is a const static), but we run
    // self-tests to verify dispatch, yield, task_id, and IPC roundtrip
    // all work through the syscall interface.
    if let Err(e) = syscall::self_test() {
        serial_println!("FATAL: Syscall dispatch self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 12: Initialize futex subsystem.
    // Futexes enable fast userspace synchronization: the uncontended
    // path is pure atomic CAS (no syscall), the contended path uses
    // the kernel to block/wake tasks.
    if let Err(e) = ipc::futex::self_test() {
        serial_println!("FATAL: Futex self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 13: Initialize pipe subsystem.
    // Pipes provide one-way kernel-buffered byte streams — the classic
    // Unix pipe model but strictly unidirectional.
    if let Err(e) = ipc::pipe::self_test() {
        serial_println!("FATAL: Pipe self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 14: Initialize shared memory subsystem.
    // Shared memory regions let tasks (and future processes) map the
    // same physical pages into their address spaces for zero-copy IPC.
    if let Err(e) = ipc::shm::self_test() {
        serial_println!("FATAL: Shared memory self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 15: Initialize eventfd subsystem.
    // Eventfds are lightweight 64-bit counters for wake-up notifications.
    // Lighter than channels — ideal for "did something happen?" signaling.
    if let Err(e) = ipc::eventfd::self_test() {
        serial_println!("FATAL: Eventfd self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 16: Initialize completion port subsystem.
    // Completion ports provide unified wait on heterogeneous kernel
    // objects (channels, pipes, eventfds, future timers/process exit).
    // This is the IOCP-like multiplexer from the design spec.
    if let Err(e) = ipc::completion::self_test() {
        serial_println!("FATAL: Completion port self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 17: Initialize capability system.
    // Capability tables store unforgeable handles to kernel objects.
    // Every resource access goes through capability checks — no
    // ambient authority.
    if let Err(e) = cap::self_test() {
        serial_println!("FATAL: Capability system self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 18: Set up SYSCALL/SYSRET MSRs.
    // IA32_STAR (segment selectors) was already configured in gdt::init().
    // This sets up IA32_LSTAR (entry point RIP), IA32_FMASK (RFLAGS mask),
    // and IA32_KERNEL_GS_BASE (per-CPU data pointer for SWAPGS).
    //
    // Must be done before proc::self_test() because the spawn tests
    // transition to ring 3, and userspace code uses SYSCALL to exit.
    //
    // SAFETY: GDT is loaded (IA32_STAR is set), IDT is initialized.
    // Called exactly once.
    unsafe {
        syscall::entry::init();
    }

    // Step 19: Initialize process management subsystem.
    // Process control blocks track per-process state: address space,
    // capability table, thread list, parent relationship.
    // Spawn tests exercise the full ring 3 path: IRETQ → userspace →
    // SYSCALL(SYS_EXIT) → kernel, so SYSCALL MSRs must be ready.
    if let Err(e) = proc::self_test() {
        serial_println!("FATAL: Process management self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 20: Initialize Local APIC and start the timer.
    // The APIC timer provides periodic interrupts for preemptive
    // scheduling.  Before this point, scheduling is purely cooperative.
    //
    // SAFETY: GDT, IDT, and heap are initialized.  We are single-threaded
    // with interrupts disabled.  Called exactly once.
    unsafe {
        apic::init();
    }

    // Step 20b: Initialize I/O APIC for external device interrupts.
    // Disables the legacy 8259 PIC, maps the IOAPIC MMIO registers,
    // and programs all 24 redirection entries (masked).  Drivers unmask
    // their IRQ lines individually when ready.
    //
    // SAFETY: LAPIC is initialized (required for EOI routing).
    // Interrupts are disabled.  Called exactly once.
    unsafe {
        ioapic::init();
    }

    // Verify IOAPIC configuration.
    if let Err(e) = ioapic::self_test() {
        serial_println!("FATAL: IOAPIC self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 20c: Scan PCI bus for device discovery.
    // This finds virtio, USB, NVMe, and other PCI devices.
    if let Err(e) = pci::self_test() {
        serial_println!("WARNING: PCI scan failed: {}", e);
    }

    // Step 20d: Probe for virtio-blk storage device.
    // Uses legacy PCI transport (I/O port BAR0) with polling.
    // The device is stored temporarily in the virtio module.
    virtio::blk::init(boot_info.hhdm_offset);

    // Step 20d-2: Probe for virtio-net network device.
    // Uses legacy PCI transport (I/O port BAR0) with polling.
    // Non-fatal if no NIC is present.
    virtio::net::init(boot_info.hhdm_offset);

    // Step 20d-3: Initialize networking stack.
    // Sets up the network interface from the virtio-net device
    // and attempts DHCP to obtain an IP address.
    net::init();

    // Step 20d-4: Attempt DHCP to obtain an IP address.
    // Non-fatal — the system works without network connectivity.
    if net::interface::is_up() {
        match net::dhcp::discover() {
            Ok(ip) => {
                serial_println!("[net] DHCP assigned IP: {}", ip);
            }
            Err(e) => {
                serial_println!("[net] DHCP failed: {:?} (non-fatal)", e);
            }
        }

    }

    // Step 20e: Initialize block device abstraction layer.
    // Moves driver instances from their module globals into the
    // unified block device registry.
    blkdev::init();

    // Step 20f: Mount root filesystem.
    // Try to mount a FAT filesystem from the first block device.
    // Auto-detects FAT16 or FAT32.  Non-fatal if no filesystem is present.
    match fs::fat::init("vda") {
        Ok(()) => {
            // Run filesystem self-test.
            if let Err(e) = fs::fat::self_test() {
                serial_println!("WARNING: FAT self-test failed: {:?}", e);
            }
        }
        Err(e) => {
            serial_println!("[fs] No FAT filesystem on vda: {:?} (non-fatal)", e);
        }
    }

    // Step 21: Enable hardware interrupts.
    // From this point forward, the APIC timer fires periodically and
    // the scheduler enforces time slices preemptively.
    //
    // SAFETY: The IDT is fully set up with handlers for exceptions,
    // the timer (vector 32), and spurious interrupts (vector 255).
    // The APIC is configured and the scheduler is ready.
    unsafe {
        cpu::sti();
    }
    serial_println!("[boot] Interrupts enabled — preemptive scheduling active");

    // Verify the APIC timer is actually firing.
    if let Err(e) = apic::self_test() {
        serial_println!("FATAL: APIC timer self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 22: Initialize PS/2 keyboard.
    // Unmasks IRQ 1, enables scan code translation.  Keypresses now
    // appear in the keyboard ring buffer and echo to the console.
    //
    // SAFETY: IOAPIC and IDT are initialized, interrupts are enabled.
    // Called exactly once.
    unsafe {
        keyboard::init();
    }

    if let Err(e) = keyboard::self_test() {
        serial_println!("FATAL: Keyboard self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 22b: Enable interrupt-driven I/O for virtio devices.
    // Now that interrupts are globally enabled and the IOAPIC is
    // initialized, switch virtio drivers from polling to interrupt-
    // driven completion.  The PCI IRQ is configured as level-triggered
    // and unmasked.  Both devices may share the same IRQ line (IRQ 11
    // on QEMU q35); the handler reads each device's ISR status register
    // to acknowledge, which is correct for shared level-triggered IRQs.
    virtio::blk::enable_interrupts();
    virtio::net::enable_interrupts();

    // Step 23: Verify the CMOS Real-Time Clock.
    // No initialization needed — the RTC is always running on battery.
    // We just verify we can read a plausible date/time.
    if let Err(e) = rtc::self_test() {
        serial_println!("WARNING: RTC self-test failed: {}", e);
        // Non-fatal — the system can function without a correct clock.
    }

    // Boot success marker — the boot test script looks for this.
    serial_println!("BOOT_OK");
    serial_println!("=== Kernel boot complete ===");

    // Show boot-complete on the framebuffer console too.
    console_println!("=== Kernel boot complete ===");

    // Enter the kernel debug shell.
    // This replaces the idle loop — the shell blocks on keyboard input
    // using HLT, so it is equally power-efficient.  The APIC timer
    // still fires and drives the scheduler.
    kshell::run();
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
