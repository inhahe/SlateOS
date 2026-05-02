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
//! 24. Spawn the userspace init process (PID 1) from an embedded ELF binary.
//!     The init process runs in ring 3 with a minimal interactive shell.
//!     The boot thread enters an idle loop after spawning init.

#![no_std]
#![no_main]
// Lint configuration is in workspace Cargo.toml ([workspace.lints.clippy])
// and inherited via [lints] workspace = true in kernel/Cargo.toml.

extern crate alloc;

// Module declarations.
mod acpi;
mod apic;
mod bench;
mod blkdev;
mod boot;
mod cap;
mod console;
mod cpu;
mod error;
mod font;
mod fs;
mod gdt;
mod hpet;
mod idt;
mod ioapic;
mod ipc;
mod keyboard;
mod klog;
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
mod smp;
mod softirq;
mod syscall;
mod sysctl;
mod tlb;
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
        console_println!(); // blank line before boot steps
    }

    // Step 3: Set up our own GDT (replacing the one Limine set up).
    //
    // SAFETY: We are in ring 0, interrupts are disabled, and this is
    // the only CPU running.
    console::boot_step(console::BootStatus::Running, "CPU tables (GDT/IDT)");
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
    console::boot_step_update(console::BootStatus::Ok, "CPU tables (GDT/IDT)");

    // Step 5: Initialize the physical frame allocator.
    //
    // SAFETY: Boot info contains a valid memory map and HHDM offset from
    // Limine.  This is the first and only call to frame::init.  We are
    // single-threaded with interrupts disabled.
    console::boot_step(console::BootStatus::Running, "Memory manager");
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
    console::boot_step_update(console::BootStatus::Ok, "Memory manager");

    // Step 6b: Calibrate TSC frequency using PIT for benchmark timing.
    // Must be after serial (for output) and before subsystem benchmarks.
    // PIT channel 2 is always available on x86_64 hardware.
    bench::calibrate_tsc();

    console::boot_step(console::BootStatus::Running, "Virtual memory");

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

    // Step 8b: Verify userspace pointer validation logic.
    // Validates that kernel rejects null, kernel-space, wrapping, and
    // unmapped user-space pointers before any syscall handler uses them.
    if let Err(e) = mm::user::self_test() {
        serial_println!("FATAL: User memory validation self-test failed: {}", e);
        cpu::halt_loop();
    }

    console::boot_step_update(console::BootStatus::Ok, "Virtual memory");

    // Step 9: Initialize the scheduler.
    // Creates the idle task (the current execution context) and sets up
    // the priority round-robin scheduler.  Timer-based preemption will
    // be added when the APIC timer is wired up (§2.2).
    console::boot_step(console::BootStatus::Running, "Scheduler");
    sched::init();

    // Verify cooperative scheduling works (spawn tasks, yield, verify).
    if let Err(e) = sched::self_test() {
        serial_println!("FATAL: Scheduler self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 9b: Initialize sysctl parameter registry.
    // Registers tunable kernel parameters for memory management,
    // scheduling, and other subsystems.
    sysctl::init();
    sysctl::self_test();

    // Step 9c: Initialize swap subsystem (zram initially).
    // The in-memory compressed (zram) backend is always available.
    // We'll try to upgrade to disk-backed swap after virtio-blk
    // and blkdev init (Step 20e).
    mm::swap::init(256);
    mm::swap::self_test();
    mm::compress::self_test();

    console::boot_step_update(console::BootStatus::Ok, "Scheduler");

    // Step 10: Initialize IPC subsystem.
    // Channels are the primary IPC mechanism — structured message
    // passing between tasks/processes.  No explicit init needed (the
    // global channel table is lazily populated).  Run self-tests to
    // verify send, recv, blocking, close detection, and backpressure.
    console::boot_step(console::BootStatus::Running, "IPC subsystem");
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

    // Step 16b: Timer subsystem self-test.
    if let Err(e) = ipc::timer::self_test() {
        serial_println!("FATAL: Timer self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 16c: io_ring (io_uring-style batch I/O) self-test.
    if let Err(e) = ipc::io_ring::self_test() {
        serial_println!("FATAL: io_ring self-test failed: {}", e);
        cpu::halt_loop();
    }

    console::boot_step_update(console::BootStatus::Ok, "IPC subsystem");

    // Step 17: Initialize capability system.
    // Capability tables store unforgeable handles to kernel objects.
    // Every resource access goes through capability checks — no
    // ambient authority.
    console::boot_step(console::BootStatus::Running, "Capabilities & logging");
    if let Err(e) = cap::self_test() {
        serial_println!("FATAL: Capability system self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 17b: Initialize structured logging subsystem.
    // JSON-lines log entries go to serial and a kernel ring buffer.
    // Must be after APIC init (uses tick_count for timestamps).
    if let Err(e) = klog::self_test() {
        serial_println!("FATAL: Structured logging self-test failed: {}", e);
        cpu::halt_loop();
    }

    console::boot_step_update(console::BootStatus::Ok, "Capabilities & logging");

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
    console::boot_step(console::BootStatus::Running, "Process management");
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

    console::boot_step_update(console::BootStatus::Ok, "Process management");

    // Step 19b: Parse ACPI tables for hardware discovery.
    // Locates the MADT to discover I/O APIC addresses, processor Local
    // APICs, and interrupt source overrides.  Must run after heap init
    // (allocates Vecs) and before APIC/IOAPIC init (which uses the data).
    console::boot_step(console::BootStatus::Running, "Hardware tables (ACPI/HPET)");
    if let Some(rsdp) = boot_info.rsdp_address {
        // SAFETY: rsdp is a valid RSDP address from Limine, HHDM maps
        // all physical memory.  Heap is initialized for Vec allocation.
        // Memory map is passed for fallback RSDP scanning.
        unsafe {
            acpi::init(rsdp, boot_info.hhdm_offset, boot_info.memory_map);
        }

        if let Err(e) = acpi::self_test() {
            serial_println!("WARNING: ACPI self-test failed: {} — using defaults", e);
        }
    } else {
        // No RSDP from Limine — try scanning memory directly.
        serial_println!("[acpi] No RSDP from bootloader — scanning memory...");
        unsafe {
            acpi::init(0, boot_info.hhdm_offset, boot_info.memory_map);
        }

        if let Err(e) = acpi::self_test() {
            serial_println!("WARNING: ACPI self-test failed: {} — using defaults", e);
        }
    }

    // Step 19c: Initialize HPET (High Precision Event Timer).
    // Provides a high-resolution monotonic counter (~10-25 MHz) for
    // precise time measurement.  The ACPI table gives us the MMIO base
    // address.  Must run after ACPI parsing and page table init (needs
    // HHDM and MMIO mapping).
    //
    // SAFETY: ACPI tables parsed, page tables initialized, single-threaded
    // early boot with interrupts disabled.
    unsafe {
        hpet::init();
    }
    if let Err(e) = hpet::self_test() {
        serial_println!("[hpet] WARNING: Self-test failed: {:?}", e);
    }

    console::boot_step_update(console::BootStatus::Ok, "Hardware tables (ACPI/HPET)");

    // Step 20: Initialize Local APIC and start the timer.
    // The APIC timer provides periodic interrupts for preemptive
    // scheduling.  Before this point, scheduling is purely cooperative.
    //
    // SAFETY: GDT, IDT, and heap are initialized.  We are single-threaded
    // with interrupts disabled.  Called exactly once.
    console::boot_step(console::BootStatus::Running, "Interrupt controllers (APIC)");
    if let Err(e) = unsafe { apic::init() } {
        serial_println!("FATAL: APIC init failed: {:?}", e);
        cpu::halt_loop();
    }

    // Step 20b: Initialize I/O APIC for external device interrupts.
    // Disables the legacy 8259 PIC, maps the IOAPIC MMIO registers,
    // and programs all 24 redirection entries (masked).  Drivers unmask
    // their IRQ lines individually when ready.  Uses I/O APIC address
    // from ACPI MADT if available, otherwise falls back to the standard
    // default (0xFEC0_0000).
    //
    // SAFETY: LAPIC is initialized (required for EOI routing).
    // Interrupts are disabled.  Called exactly once.
    if let Err(e) = unsafe { ioapic::init() } {
        serial_println!("FATAL: IOAPIC init failed: {:?}", e);
        cpu::halt_loop();
    }

    // Verify IOAPIC configuration.
    if let Err(e) = ioapic::self_test() {
        serial_println!("FATAL: IOAPIC self-test failed: {}", e);
        cpu::halt_loop();
    }

    console::boot_step_update(console::BootStatus::Ok, "Interrupt controllers (APIC)");

    // Step 20c: Scan PCI bus for device discovery.
    // This finds virtio, USB, NVMe, and other PCI devices.
    console::boot_step(console::BootStatus::Running, "PCI & device drivers");
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

    console::boot_step_update(console::BootStatus::Ok, "PCI & device drivers");

    // Step 20d-3: Initialize networking stack.
    // Sets up the network interface from the virtio-net device
    // and attempts DHCP to obtain an IP address.
    console::boot_step(console::BootStatus::Running, "Network stack");
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

    console::boot_step_update(console::BootStatus::Ok, "Network stack");

    // Step 20e: Initialize block device abstraction layer.
    // Moves driver instances from their module globals into the
    // unified block device registry.
    console::boot_step(console::BootStatus::Running, "Storage & filesystems");
    blkdev::init();

    // Step 20e-2: Add disk-backed swap alongside zram.
    // Multi-device swap: zram (priority 100) handles most evictions with
    // zero I/O latency; disk (priority 0) catches overflow when zram is full.
    // In QEMU, a second virtio-blk disk is available as "vda" (or "vdb"
    // if the boot disk is also virtio).  Try known swap device names.
    //
    // Each slot = 16 KiB = 32 sectors.  512 slots = 16 MiB.
    for swap_dev in &["vdb", "vda"] {
        if mm::swap::init_disk(swap_dev, 0, 512).is_ok() {
            serial_println!("[boot] Disk swap added on {} (zram + disk tiered)", swap_dev);
            // Run the disk-specific self-test now that a disk backend
            // is active.
            mm::swap::self_test_disk();
            break;
        }
    }

    // Step 20f: Mount root filesystem.
    // Try to mount a FAT filesystem from the first block device.
    // Auto-detects FAT16 or FAT32.  Non-fatal if no filesystem is present.
    match fs::fat::init("vda") {
        Ok(()) => {
            // Mount an in-memory filesystem at /tmp for temporary files.
            // This is volatile (lost on reboot) and heap-backed.
            if let Err(e) = fs::memfs::mount("/tmp") {
                serial_println!("[boot] WARNING: failed to mount memfs at /tmp: {:?}", e);
            }

            // Mount procfs at /proc for system information.
            // Read-only virtual filesystem — content generated on the fly.
            if let Err(e) = fs::procfs::mount("/proc") {
                serial_println!("[boot] WARNING: failed to mount procfs at /proc: {:?}", e);
            }

            // Mount devfs at /dev for standard device files.
            // Provides /dev/null, /dev/zero, /dev/random, /dev/console.
            if let Err(e) = fs::devfs::mount("/dev") {
                serial_println!("[boot] WARNING: failed to mount devfs at /dev: {:?}", e);
            }

            // Probe secondary block devices for ext4 filesystems.
            // Try common virtio-blk device names.  The first ext4 partition
            // found is mounted at /mnt.  Non-fatal if none found.
            for ext4_dev in &["vdb", "vdc"] {
                if fs::ext4::probe(ext4_dev) {
                    match fs::ext4::mount(ext4_dev, "/mnt") {
                        Ok(()) => break,
                        Err(e) => {
                            serial_println!(
                                "[boot] WARNING: ext4 detected on {} but mount failed: {:?}",
                                ext4_dev, e
                            );
                        }
                    }
                }
            }

            // Probe for ISO 9660 filesystems (CD-ROM images).
            // In QEMU, an ISO image can be attached as a virtio-blk device.
            for iso_dev in &["vdb", "vdc", "vdd"] {
                if fs::iso9660::probe(iso_dev) {
                    match fs::iso9660::mount(iso_dev, "/cdrom") {
                        Ok(()) => break,
                        Err(e) => {
                            serial_println!(
                                "[boot] WARNING: ISO 9660 detected on {} but mount failed: {:?}",
                                iso_dev, e
                            );
                        }
                    }
                }
            }

            // Initialize the change journal (persistent change tracking).
            // Must happen before self-tests so all VFS operations are captured.
            fs::journal::init();

            // Run filesystem self-test.
            if let Err(e) = fs::fat::self_test() {
                serial_println!("WARNING: FAT self-test failed: {:?}", e);
            }
            // Run file handle self-test (requires mounted filesystem).
            if let Err(e) = fs::handle::self_test() {
                serial_println!("WARNING: File handle self-test failed: {:?}", e);
            }
            // Run buffer cache self-test (validates caching, write-back, LRU).
            if let Err(e) = fs::cache::self_test() {
                serial_println!("WARNING: Buffer cache self-test failed: {:?}", e);
            }
            // Run recycle bin self-test (trash, list, restore, empty).
            if let Err(e) = fs::trash::self_test() {
                serial_println!("WARNING: Recycle bin self-test failed: {:?}", e);
            }
            // Run change notification self-test (watch, emit, read, close).
            if let Err(e) = fs::notify::self_test() {
                serial_println!("WARNING: Change notification self-test failed: {:?}", e);
            }
            // Initialize and self-test the change journal (persistent change tracking).
            if let Err(e) = fs::journal::self_test() {
                serial_println!("WARNING: Change journal self-test failed: {:?}", e);
            }
            // Run in-memory filesystem self-test (standalone, doesn't touch VFS mount).
            if let Err(e) = fs::memfs::self_test() {
                serial_println!("WARNING: MemFs self-test failed: {:?}", e);
            }
            // Run procfs self-test (validates virtual file generation).
            if let Err(e) = fs::procfs::self_test() {
                serial_println!("WARNING: ProcFs self-test failed: {:?}", e);
            }
            // Run devfs self-test (validates device file operations).
            if let Err(e) = fs::devfs::self_test() {
                serial_println!("WARNING: DevFs self-test failed: {:?}", e);
            }
            // Run ext4 self-test (reads directory listing and files if mounted).
            if let Err(e) = fs::ext4::self_test() {
                serial_println!("WARNING: ext4 self-test failed: {:?}", e);
            }
            // Flush buffer cache to disk so data survives power loss / QEMU kill.
            if let Err(e) = fs::cache::flush_all() {
                serial_println!("WARNING: Buffer cache flush failed: {:?}", e);
            }
        }
        Err(e) => {
            serial_println!("[fs] No FAT filesystem on vda: {:?} (non-fatal)", e);
        }
    }

    console::boot_step_update(console::BootStatus::Ok, "Storage & filesystems");

    // Step 21: Enable hardware interrupts.
    // From this point forward, the APIC timer fires periodically and
    // the scheduler enforces time slices preemptively.
    //
    // SAFETY: The IDT is fully set up with handlers for exceptions,
    // the timer (vector 32), and spurious interrupts (vector 255).
    // The APIC is configured and the scheduler is ready.
    console::boot_step(console::BootStatus::Running, "Preemptive scheduling");
    unsafe {
        cpu::sti();
    }
    serial_println!("[boot] Interrupts enabled — preemptive scheduling active");

    // Verify the APIC timer is actually firing.
    if let Err(e) = apic::self_test() {
        serial_println!("FATAL: APIC timer self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Softirq self-test — verify raise/process/reentry-guard work.
    // Must be after interrupts are enabled (softirq processing does
    // STI/CLI internally).
    if let Err(e) = softirq::self_test() {
        serial_println!("FATAL: Softirq self-test failed: {}", e);
        cpu::halt_loop();
    }

    console::boot_step_update(console::BootStatus::Ok, "Preemptive scheduling");

    // Step 22: Initialize PS/2 keyboard.
    // Unmasks IRQ 1, enables scan code translation.  Keypresses now
    // appear in the keyboard ring buffer and echo to the console.
    //
    // SAFETY: IOAPIC and IDT are initialized, interrupts are enabled.
    // Called exactly once.
    console::boot_step(console::BootStatus::Running, "Keyboard & multi-core");
    unsafe {
        keyboard::init();
    }

    if let Err(e) = keyboard::self_test() {
        serial_println!("FATAL: Keyboard self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 21b: Bootstrap Application Processors (SMP).
    // Discovers APs via ACPI MADT, copies the real-mode trampoline to
    // low memory, and sends INIT-SIPI-SIPI to each AP.  Each AP loads
    // the kernel's GDT/IDT, enables its local APIC timer, and enters
    // the scheduler's idle loop.
    //
    // Must be after: ACPI (CPU discovery), APIC (IPI sending),
    //                scheduler (per-CPU queues), page tables (identity mapping).
    smp::init();
    smp::self_test();

    // Step 22b½: Validate SMP scheduler invariants.
    // Now that all APs are online with their idle tasks, verify
    // per-CPU current tasks are distinct and reap is SMP-safe.
    if let Err(e) = sched::smp_self_test() {
        serial_println!("FATAL: Scheduler SMP self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 22c: TLB shootdown self-test.
    // Now that all CPUs are online, verify the TLB shootdown IPI works.
    tlb::self_test();

    // Step 22d: DMA buffer management self-test.
    // Verifies contiguous physical allocation and free for device DMA.
    mm::dma::self_test();

    // Step 22e: Copy-on-Write self-test.
    // Verifies refcount API and COW PTE flag manipulation.
    mm::cow::self_test();

    console::boot_step_update(console::BootStatus::Ok, "Keyboard & multi-core");

    // Step 22e2: Harden page permissions — set NX on HHDM and fix kernel
    // section permissions (W^X enforcement for kernel's own pages).
    console::boot_step(console::BootStatus::Running, "Security hardening");
    {
        let pml4 = mm::page_table::active_pml4_phys();

        let hhdm_hardened = mm::protect::harden_hhdm_nx(pml4);
        serial_println!(
            "[protect] HHDM NX hardened: {} PML4 entries updated",
            hhdm_hardened
        );

        let (sections_hardened, section_errors) =
            mm::protect::harden_kernel_sections(pml4);
        serial_println!(
            "[protect] Kernel section permissions hardened: {} PTEs updated, {} errors",
            sections_hardened, section_errors
        );
    }

    // Step 22e3: Memory protection (mprotect / W^X) self-test.
    // Verifies mprotect flag changes, W^X enforcement, JIT capability gate,
    // and audits kernel page tables for write+execute violations.
    // Runs AFTER hardening so the audit reflects the fixed state.
    if let Err(e) = mm::protect::self_test() {
        serial_println!("[FATAL] Memory protection self-test failed: {:?}", e);
    }

    console::boot_step_update(console::BootStatus::Ok, "Security hardening");

    // Step 22f: Enable per-CPU frame caches.
    // Now that all CPUs are online and current_cpu_index() works,
    // enable the per-CPU frame cache for lock-free order-0 allocation.
    console::boot_step(console::BootStatus::Running, "Performance tuning");
    mm::frame::enable_pcpu_caches();
    mm::heap::enable_pcpu_slab_caches();

    // Step 22f-2: Enable the pre-zeroed frame pool.
    // The idle loop will refill this pool in the background so
    // alloc_frame_zeroed() can skip the 16 KiB memset on the
    // page fault hot path.
    mm::frame::enable_zero_pool();

    // Step 22g: I/O scheduler self-test.
    // BFQ-style budget fair queueing with per-process queues,
    // priority classes, elevator ordering, and request merging.
    sched::io_sched::self_test();

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

    // Step 23b: Run benchmark infrastructure self-test (fast, validates runner).
    // The actual micro-benchmarks (bench::run_all) are deferred to a
    // background kernel task so init can start immediately.  This shaves
    // ~15-20s off the time-to-usable under QEMU TCG.
    bench::self_test();

    // Print a boot-time memory summary via the unified MemoryInfo API.
    {
        let info = mm::memory_info();
        serial_println!("=== Memory summary ===");
        serial_println!("{}", info);
    }

    console::boot_step_update(console::BootStatus::Ok, "Performance tuning");

    // Step 22b: Spawn kswapd (background page reclaimer).
    // Must be after swap init (Step 9c/20e) and scheduler (Step 10).
    // kswapd proactively reclaims pages when free memory drops below
    // the low watermark, preventing allocation stalls under memory
    // pressure.
    match mm::kswapd::spawn() {
        Ok(()) => {}
        Err(e) => {
            serial_println!("[boot] WARNING: failed to spawn kswapd: {:?}", e);
            // Non-fatal — the system will fall back to synchronous
            // reclamation in alloc_order().
        }
    }
    mm::kswapd::self_test();
    mm::oom::self_test();

    // Boot success marker — the boot test script greps for this.
    // Printed synchronously so it appears within seconds of power-on,
    // regardless of how long deferred benchmarks take.
    serial_println!("=== Kernel boot complete ===");
    serial_println!("BOOT_OK");

    // Show boot-complete on the framebuffer console too.
    console_println!();
    console_println!("=== Kernel boot complete ===");

    // Spawn a low-priority kernel task to run micro-benchmarks in the
    // background.  This lets init start immediately while benchmarks
    // run interleaved with normal scheduling.
    let pml4 = mm::page_table::active_pml4_phys();
    match sched::spawn(
        b"bench",
        sched::task::DEFAULT_PRIORITY.saturating_add(2), // slightly below default
        deferred_bench_task,
        0,
        pml4,
    ) {
        Ok(tid) => {
            serial_println!("[boot] Deferred benchmark task spawned (tid={})", tid);
        }
        Err(e) => {
            serial_println!("[boot] WARNING: failed to spawn bench task: {:?}", e);
            // Fall back to inline benchmarks so we still get numbers.
            bench::run_all();
            serial_println!("BENCH_OK");
        }
    }

    // Step 24: Spawn the userspace init process (PID 1).
    //
    // The init binary is embedded in the kernel image at compile time.
    // It runs in ring 3 and provides a minimal user-mode shell.  This
    // is the first step toward Phase 2 (boot to a shell prompt).
    //
    // If init spawning fails, fall back to the kernel debug shell.
    static INIT_ELF: &[u8] = include_bytes!(
        "../../userspace/init/target/x86_64-unknown-none/release/init"
    );
    static HELLO_ELF: &[u8] = include_bytes!(
        "../../userspace/hello/target/x86_64-unknown-none/release/hello"
    );
    static TICKER_ELF: &[u8] = include_bytes!(
        "../../userspace/ticker/target/x86_64-unknown-none/release/ticker"
    );

    // Write embedded binaries to the VFS so init can spawn them.
    if let Err(e) = fs::Vfs::mkdir("/bin") {
        serial_println!("[init] Note: /bin mkdir: {:?} (may already exist)", e);
    }
    if let Err(e) = fs::Vfs::write_file("/bin/hello", HELLO_ELF) {
        serial_println!("[init] WARNING: failed to write /bin/hello: {:?}", e);
    } else {
        serial_println!("[init] Installed /bin/hello ({} bytes)", HELLO_ELF.len());
    }
    if let Err(e) = fs::Vfs::write_file("/bin/ticker", TICKER_ELF) {
        serial_println!("[init] WARNING: failed to write /bin/ticker: {:?}", e);
    } else {
        serial_println!("[init] Installed /bin/ticker ({} bytes)", TICKER_ELF.len());
    }

    // Create /etc directory and write a default service list.
    // Init reads this at startup to auto-register services.
    if let Err(e) = fs::Vfs::mkdir("/etc") {
        serial_println!("[init] Note: /etc mkdir: {:?} (may already exist)", e);
    }
    if let Err(e) = fs::Vfs::write_file(
        "/etc/services",
        b"# Startup services (one per line)\n# Format: /path/to/elf [depends:dep1,dep2]\n/bin/ticker\n",
    ) {
        serial_println!("[init] Note: /etc/services write: {:?}", e);
    } else {
        serial_println!("[init] Created /etc/services");
    }

    serial_println!("[init] Spawning init process ({} bytes ELF)", INIT_ELF.len());

    // Init process capabilities — init is the root userspace process
    // and needs broad access.  Child processes will receive restricted
    // subsets of these capabilities.
    let init_caps: &[(cap::ResourceType, u64, cap::Rights)] = &[
        // Filesystem: full access (read, write, create, delete, metadata).
        (cap::ResourceType::File, 0, cap::Rights::ALL),
        // Network: full access (connect, bind, send, recv).
        (cap::ResourceType::Socket, 0, cap::Rights::ALL),
        // Process management: spawn and manage children.
        (cap::ResourceType::Process, 0, cap::Rights::ALL),
    ];
    let spawn_opts = proc::spawn::SpawnOptions {
        name: "init",
        parent: 0,
        priority: sched::task::DEFAULT_PRIORITY,
        capabilities: init_caps,
    };
    match proc::spawn::spawn_process(INIT_ELF, &spawn_opts) {
        Ok(result) => {
            serial_println!(
                "[init] Init process spawned: pid={}, tid={}, entry={:#x}",
                result.pid, result.task_id, result.entry_point
            );
            // The init process is now in the scheduler's run queue.
            // Drop into the idle loop — the scheduler will switch to
            // init when it gets a time slice.
            idle_loop();
        }
        Err(e) => {
            serial_println!("[init] FAILED to spawn init: {:?}", e);
            serial_println!("[init] Falling back to kernel debug shell");
            kshell::run();
        }
    }
}

// ---------------------------------------------------------------------------
// Deferred benchmark task
// ---------------------------------------------------------------------------

/// Kernel task that runs micro-benchmarks after boot completes.
///
/// By deferring benchmarks to a background task, the init process
/// can start immediately.  Under QEMU TCG, benchmarks take 15-20s;
/// running them in parallel with init gets the user to a shell prompt
/// in ~1s instead of ~20s.
///
/// The task prints `BENCH_OK` to serial after all benchmarks complete.
/// `BOOT_OK` is printed synchronously by `kmain()` before this task
/// starts, so the boot test script sees success within seconds.
extern "C" fn deferred_bench_task(_arg: u64) {
    bench::run_all();
    serial_println!("BENCH_OK");
}

// ---------------------------------------------------------------------------
// Idle loop
// ---------------------------------------------------------------------------

/// The kernel idle loop.
///
/// After spawning the init process, the boot thread enters this loop.
/// Each iteration performs lightweight housekeeping before yielding
/// and halting:
///
/// 1. **Reap dead tasks** — free kernel stacks for tasks that have
///    exited.  Without this, each dead task leaks 32 KiB of stack
///    memory permanently.
/// 2. **Refill the pre-zeroed frame pool** — zero a small batch of
///    frames in the background so page faults can grab them instantly.
///    This moves the 16 KiB memset cost from the page fault hot path
///    to idle time.
///
/// The APIC timer wakes the CPU from HLT for scheduling decisions.
///
/// The timer ISR calls `preempt()` on every tick, which runs
/// `schedule_inner` and switches to any ready task.  We do NOT
/// call `yield_now()` here — it would redundantly acquire the
/// global SCHED spinlock, re-enqueue the idle task, pick it right
/// back, and return.  On SMP, this contention was measured at ~4x
/// regression on the context switch benchmark due to cross-CPU
/// spinlock thrashing.
///
/// Maintenance (reap + refill) runs at reduced frequency to keep
/// lock pressure low.
fn idle_loop() -> ! {
    let mut tick_counter = 0u32;
    loop {
        cpu::hlt(); // Sleep until next interrupt (timer tick or IPI).

        tick_counter = tick_counter.wrapping_add(1);

        // If a reschedule IPI woke us (someone enqueued work for CPU 0),
        // yield immediately to pick up the new task.  This avoids the
        // up-to-10ms latency of waiting for the next timer tick.
        if sched::reschedule_pending(0) {
            sched::yield_now();
        }

        // Reap dead tasks once per second (~100 ticks at 100 Hz).
        // reap_dead_tasks allocates Vecs and acquires the SCHED lock
        // even when nothing is dead, so throttling reduces contention.
        if tick_counter.is_multiple_of(100) {
            sched::reap_dead_tasks();
        }

        // Refill the pre-zeroed frame pool.  Each call zeros up to
        // ZERO_POOL_REFILL_BATCH frames (8 × 16 KiB = 128 KiB), which
        // takes ~50-100µs on real hardware.  We do this during idle
        // time so page faults can skip the inline zeroing.
        mm::frame::refill_zero_pool();
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

/// Panic handler for the kernel.
///
/// Prints the panic info to serial along with diagnostic context:
/// current task, CPU, stack usage, and memory statistics.  All lock
/// acquisitions use `try_lock` to avoid deadlock if the panic occurred
/// while holding a lock.
///
/// In a kernel, panics are always fatal — there is no higher-level
/// runtime to catch them.
#[panic_handler]
#[allow(clippy::unwrap_used)] // Panic handler uses unwrap_or for best-effort diagnostics.
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Capture volatile state before disabling interrupts.
    let rsp = cpu::read_rsp();
    let interrupts_were_enabled = cpu::interrupts_enabled();

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

    // --- Task context ---
    let sched_info = sched::panic_diagnostics();
    let name_slice = sched_info.name.get(..sched_info.name_len).unwrap_or(&[]);
    let task_name = core::str::from_utf8(name_slice).unwrap_or("<invalid utf8>");
    serial_println!(
        "  Task: {} ({:?}), priority {}, cpu {}",
        sched_info.current_task_id,
        task_name,
        sched_info.priority,
        sched::current_cpu_id(),
    );

    // Stack usage estimate: compare RSP to the task's stack region.
    if sched_info.stack_bottom != 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let stack_top = sched_info.stack_bottom + sched::task::TASK_STACK_SIZE as u64;
        let used = stack_top.saturating_sub(rsp);
        serial_println!(
            "  Stack: bottom={:#x}, top={:#x}, rsp={:#x}, used={} / {} bytes",
            sched_info.stack_bottom,
            stack_top,
            rsp,
            used,
            sched::task::TASK_STACK_SIZE,
        );
    } else {
        serial_println!("  Stack: rsp={:#x} (idle task, bootloader stack)", rsp);
    }

    serial_println!(
        "  Interrupts were {}",
        if interrupts_were_enabled { "enabled" } else { "disabled" },
    );

    // --- Scheduler summary ---
    if sched_info.lock_acquired {
        let [ready, running, blocked, suspended, dead] = sched_info.state_counts;
        serial_println!(
            "  Tasks: {} total (ready={}, running={}, blocked={}, suspended={}, dead={})",
            sched_info.total_tasks,
            ready, running, blocked, suspended, dead,
        );
    } else {
        serial_println!("  Tasks: <scheduler lock held, cannot inspect>");
    }

    // --- Memory summary ---
    if let Some(stats) = mm::frame::try_stats() {
        let used = stats.total_frames.saturating_sub(stats.free_frames);
        let total_kb = stats.total_frames.saturating_mul(mm::frame::FRAME_SIZE) / 1024;
        let free_kb = stats.free_frames.saturating_mul(mm::frame::FRAME_SIZE) / 1024;
        let used_kb = used.saturating_mul(mm::frame::FRAME_SIZE) / 1024;
        serial_println!(
            "  Memory: {} KiB total, {} KiB used, {} KiB free ({} frames free)",
            total_kb, used_kb, free_kb, stats.free_frames,
        );
    } else {
        serial_println!("  Memory: <allocator lock held or not initialized>");
    }

    // --- Heap summary (lock-free, always available) ---
    let heap = mm::heap::stats();
    serial_println!(
        "  Heap: slab={}/{} allocs/frees, large={}/{}, refills={}, failures={}",
        heap.slab_allocs, heap.slab_frees,
        heap.large_allocs, heap.large_frees,
        heap.slab_refills, heap.alloc_failures,
    );

    serial_println!("--- end panic ---");

    cpu::halt_loop();
}
