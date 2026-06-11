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
mod ac97;
mod acpi;
mod apic;
mod audio_history;
mod audio_mixer;
mod audio_notify;
mod backtrace;
mod bench;
mod blkdev;
mod ahci;
mod boot;
mod cap;
mod cet;
mod cgroup;
mod compositor;
mod container;
mod console;
mod cpu;
mod cpu_hotplug;
mod cpu_topology;
mod cpufreq;
mod e1000;
mod fb;
mod cputime;
mod crypto;
mod devhotplug;
mod devpower;
mod drm;
mod drvmon;
mod error;
mod eventlog;
mod font;
mod fs;
mod gdt;
mod hda;
mod hpet;
mod hrtimer;
mod hypervisor;
mod idt;
mod idle;
mod initproc;
mod iommu;
mod iommu_remap;
mod invariant;
mod ioapic;
mod irq_storm;
mod irqbalance;
mod ipc;
mod json;
mod kcounters;
mod kdiag;
mod kevent;
mod keyboard;
mod kobject;
mod mouse;
mod klog;
mod kshell;
mod ksnapshot;
mod ksyms;
mod boot_timing;
mod kprofile;
mod kstat;
mod kwarn;
mod ktimer;
mod loadavg;
mod ktrace;
mod limine;
mod lockdep;
mod logpersist;
mod mm;
mod msi;
mod net;
mod netns;
mod numa;
mod nvme;
mod oci;
mod pci;
mod pciids;
mod pcspk;
mod pacct;
mod pidns;
mod pmc;
mod port;
mod power;
mod proc;
mod ratelimit;
mod reslimit;
mod rcu;
mod rip_sample;
mod rng;
mod rtc;
mod rtl8139;
mod sched;
mod sched_fairness;
mod scfilter;
mod sched_migrate;
mod sclatency;
mod security;
mod selftest;
mod serial;
mod smep_smap;
mod smp;
mod spectre;
mod sockact;
mod softirq;
mod svcstart;
mod sync;
mod syscall;
mod sysctl;
mod syshealth;
mod termsession;
mod thermal;
mod timekeeping;
mod tlb;
mod udriver;
mod unicode;
mod userns;
mod virtio;
mod vmguest;
mod watchdog;
#[allow(dead_code)]
mod xhci;
mod watchpoint;
mod wchan;
mod workqueue;

// ---------------------------------------------------------------------------
// Kernel entry point
// ---------------------------------------------------------------------------

/// Size of the dedicated kernel boot stack (512 KiB).
///
/// Limine's default boot stack is only 64 KiB and lives in
/// bootloader-reclaimable memory immediately above the page tables Limine
/// set up (the kernel reuses Limine's PML4 rather than building its own).
/// Running the kernel's full boot-time self-test suite on that stack —
/// deep call chains with large per-frame locals in the unoptimized debug
/// build (e.g. `Task::new_kernel` reserves a 4 KiB `FpuState` frame) — grew
/// the stack down across the active PML4 and silently corrupted it (writes
/// into the HHDM-mapped page tables do not fault; the stale TLB hid the
/// damage until a later walk wedged the CPU).  We therefore switch to this
/// dedicated stack, in the kernel's own `.bss` and far from any bootloader
/// structures, before doing any real work.
const KERNEL_BOOT_STACK_SIZE: usize = 512 * 1024;

/// The dedicated kernel boot stack.  16-byte aligned for the System V ABI.
#[repr(C, align(16))]
struct KernelBootStack([u8; KERNEL_BOOT_STACK_SIZE]);

static mut KERNEL_BOOT_STACK: KernelBootStack = KernelBootStack([0; KERNEL_BOOT_STACK_SIZE]);

/// Kernel entry point, called by the Limine bootloader.
///
/// At this point we have:
/// - 64-bit long mode, paging enabled
/// - Identity map + HHDM for physical memory access
/// - Interrupts disabled
/// - BSS zeroed
/// - A temporary, small stack provided by the bootloader
///
/// This is a minimal trampoline: it immediately switches `RSP` to the top
/// of the dedicated [`KERNEL_BOOT_STACK`] and tail-calls [`kernel_main`], so
/// that no meaningful work — and in particular none of the deep boot-time
/// self-tests — ever runs on Limine's small reclaimable-memory stack.
#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    // SAFETY: `KERNEL_BOOT_STACK` is a dedicated 512 KiB static used only as
    // the kernel boot stack and touched nowhere else, so there is no
    // aliasing concern in computing its top address.  We load the top
    // (highest address; the static's size is a multiple of 16 and it is
    // 16-byte aligned, so the top is 16-byte aligned as the ABI requires
    // before a `call`) into RSP, zero RBP to terminate stack-frame chains,
    // then `call kernel_main`.  The old Limine stack frame is abandoned;
    // `kernel_main` is `-> !` and never returns, so the discarded frame is
    // never referenced again.
    unsafe {
        let stack_top = core::ptr::addr_of_mut!(KERNEL_BOOT_STACK)
            .cast::<u8>()
            .add(KERNEL_BOOT_STACK_SIZE) as u64;
        core::arch::asm!(
            "mov rsp, {top}",
            "xor rbp, rbp",
            "call {main}",
            top = in(reg) stack_top,
            main = sym kernel_main,
            options(noreturn),
        );
    }
}

/// The real kernel entry, running on the dedicated [`KERNEL_BOOT_STACK`].
#[unsafe(no_mangle)]
extern "C" fn kernel_main() -> ! {
    // Step 1: Initialize serial console for debug output.
    // This must be first so we can log everything that follows.
    //
    // SAFETY: COM1 is standard PC hardware, always present in QEMU.
    // Called exactly once.
    unsafe {
        serial::init();
    }

    serial_println!("=== Kernel booting ===");
    boot_timing::mark(boot_timing::Milestone::KernelEntry);

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

    // Initialize framebuffer 2D graphics primitives (after console::init
    // makes the framebuffer parameters available).
    fb::init();

    // Step 3: Set up our own GDT (replacing the one Limine set up).
    //
    // SAFETY: We are in ring 0, interrupts are disabled, and this is
    // the only CPU running.
    console::boot_step(console::BootStatus::Running, "CPU tables (GDT/IDT)");
    unsafe {
        gdt::init();
    }
    serial_println!("[gdt] GDT and TSS initialized");
    boot_timing::mark(boot_timing::Milestone::GdtIdt);

    // Step 4: Set up the IDT with exception handlers.
    //
    // SAFETY: GDT is loaded (the IDT references kernel CS).  We are
    // single-threaded during boot.
    unsafe {
        idt::init();
    }
    serial_println!("[idt] IDT initialized");

    // Step 4a: Detect CPU features via CPUID and cache them globally.
    // Must be done before FPU init so subsystems can query feature flags.
    cpu::detect_features();
    cpu::log_features();
    // Detect virtualization environment via CPUID.  This must happen
    // early because hypervisor type affects TSC behavior and driver
    // selection (e.g., prefer virtio under KVM/QEMU).
    hypervisor::detect();
    // Detect Intel CET (Control-flow Enforcement Technology) support.
    // On hardware with CET, this enables shadow stacks and IBT for kernel protection.
    cet::detect();
    // Enable SMEP/SMAP — hardware protection against kernel accidentally
    // accessing or executing user-space memory.  Critical for security.
    smep_smap::init();
    // Spectre/Meltdown mitigations — enable IBRS, STIBP, SSBD based on
    // CPU capabilities.  Issues initial IBPB to flush stale predictions.
    spectre::init();
    // Cache topology detection uses only CPUID (no heap needed), but
    // logging uses alloc::format, so we detect now and log later.
    cpu::detect_cache_topology();

    // Step 4b: Initialize FPU/SSE hardware on the BSP.
    //
    // Ensures CR0 and CR4 are configured for SSE operation (clear EM/TS,
    // set OSFXSR/OSXMMEXCPT).  While Limine typically sets these, we
    // configure them explicitly so the state is deterministic.  Must be
    // done before any code that might use XMM registers (e.g., the heap
    // allocator's memcpy, auto-vectorized loops).
    sched::fpu::init_bsp();
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

    boot_timing::mark(boot_timing::Milestone::FrameAlloc);

    // Step 6: Initialize the kernel heap.
    // The slab allocator uses the frame allocator for backing memory.
    mm::heap::init(boot_info.hhdm_offset);

    // Now that the heap is available, tell the console it can allocate
    // its screen text buffer and scrollback ring.
    console::notify_heap_available();

    // Verify heap allocations work.
    if let Err(e) = mm::heap::self_test() {
        serial_println!("FATAL: Heap allocator self-test failed: {}", e);
        cpu::halt_loop();
    }
    boot_timing::mark(boot_timing::Milestone::Heap);
    console::boot_step_update(console::BootStatus::Ok, "Memory manager");

    // Load kernel symbol table from ELF .symtab for backtrace resolution.
    // Needs heap (Vec allocation).  Best done early so symbols are available
    // for any crash during the rest of boot.
    ksyms::init();

    // Log cache topology (deferred from early boot — needs heap for formatting).
    cpu::log_cache_topology();

    // Step 6b: Calibrate TSC frequency using PIT for benchmark timing.
    // Must be after serial (for output) and before subsystem benchmarks.
    // PIT channel 2 is always available on x86_64 hardware.
    bench::calibrate_tsc();
    cputime::init();
    timekeeping::init();

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

    // Step 8c: Initialize kernel stack allocator with hardware guard pages.
    // Must be after fault::init() since it registers Guard VMAs.
    mm::kstack::init();
    if let Err(e) = mm::kstack::self_test() {
        serial_println!("FATAL: Kernel stack guard page self-test failed: {}", e);
        cpu::halt_loop();
    }

    boot_timing::mark(boot_timing::Milestone::PageTable);
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

    // Process accounting (after self-test, which fills/empties the hook table).
    pacct::init();

    // Verify FPU/SSE state save/restore works correctly.
    // This tests the fxsave64/fxrstor64 path that the context switch uses.
    sched::fpu::self_test();

    // Multi-task stress test: verify XMM state isolation across context switches.
    // Spawns 4 tasks writing unique patterns to XMM1, yields 50 times each,
    // verifies no cross-task XMM leakage.
    sched::fpu::stress_test();

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

    boot_timing::mark(boot_timing::Milestone::Scheduler);
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
    if let Err(e) = syscall::linux::self_test() {
        serial_println!("FATAL: Linux ABI translation self-test failed: {}", e);
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

    // Step 13b: Initialize stream socket subsystem.
    // Stream sockets are bidirectional kernel-buffered byte streams — the
    // primitive backing POSIX socketpair(AF_UNIX, SOCK_STREAM, ...).
    //
    // This self-test was previously disabled because its heap-allocation
    // churn appeared to trigger a "later" boot hang during ring-3 process
    // spawns.  That hang was in fact the boot-stack-vs-PML4 collision (the
    // kernel ran on Limine's small reclaimable-memory stack, which grew down
    // into the active page tables); the churn only shifted allocation/timing
    // enough to expose it.  With the kernel now switched to a dedicated
    // 512 KiB boot stack (see `KERNEL_BOOT_STACK`), the underlying bug is
    // fixed and the self-test runs normally at boot.
    if let Err(e) = ipc::stream_socket::self_test() {
        serial_println!("FATAL: Stream socket self-test failed: {}", e);
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

    // Step 15b: Memfd subsystem.
    // Anonymous in-memory regular file backing memfd_create(2).  Exercises
    // create/close/dup refcounting, read/write/seek, pread/pwrite,
    // truncate grow/shrink, F_SEAL_* enforcement, and poll readiness.
    if let Err(e) = ipc::memfd::self_test() {
        serial_println!("FATAL: Memfd self-test failed: {}", e);
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

    // Step 16b½: IPC semaphore self-test.
    if let Err(e) = ipc::semaphore::self_test() {
        serial_println!("FATAL: IPC semaphore self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 16c: io_ring (io_uring-style batch I/O) self-test.
    if let Err(e) = ipc::io_ring::self_test() {
        serial_println!("FATAL: io_ring self-test failed: {}", e);
        cpu::halt_loop();
    }

    boot_timing::mark(boot_timing::Milestone::Ipc);
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

    // Step 17a¼: Initialize named capability groups.
    // Built-in groups (admin, network, filesystem, driver, process, ipc)
    // are created with root (gid=0) as default member.
    cap::groups::init();

    // Step 17a½: Capability audit log self-test.
    cap::audit::self_test();

    // Step 17a¾: Capability groups self-test.
    if let Err(e) = cap::groups::self_test() {
        serial_println!("[WARN] Capability groups self-test failed: {:?}", e);
    }

    // Step 17a⅞: File capability tags self-test.
    if let Err(e) = cap::file_tags::self_test() {
        serial_println!("[WARN] File capability tags self-test failed: {:?}", e);
    }

    // Step 17a⅞+: Capability request broker self-test.
    if let Err(e) = cap::request::self_test() {
        serial_println!("[WARN] Capability request broker self-test failed: {:?}", e);
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
        // SAFETY: rsdp=0 triggers memory scanning; HHDM maps all physical
        // memory.  Heap is initialized for Vec allocation.
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

    // Step 19c¼: Detect IOMMU hardware (Intel VT-d / AMD-Vi).
    // Probes for DMAR/IVRS ACPI tables.  Detection only — actual DMA
    // remapping page tables are set up below.
    // Must be after ACPI init (uses acpi::find_table).
    iommu::init();

    // Step 19c¼: Initialize IOMMU DMA remapping page tables.
    // Programs root/context tables and enables translation on each
    // detected IOMMU unit.  After this, all PCI DMA is sandboxed.
    // Must be after iommu::init() (detection) and frame allocator.
    if let Err(e) = iommu_remap::init() {
        serial_println!("[boot] WARNING: IOMMU remap init failed: {:?}", e);
    }

    // Step 19c½: Initialize high-resolution timer subsystem.
    // Uses HPET as the clock source for nanosecond-precision timers.
    // Must be after HPET init.
    hrtimer::init();

    // Step 19d: Initialize kernel CSPRNG.
    // Seeds ChaCha20 from RDRAND/RDSEED (if available), HPET counter,
    // and TSC jitter.  Must be after HPET init for timer-based entropy.
    rng::init();
    // Randomize the stack canary using the now-initialized CSPRNG.
    // Must be after rng::init() and before any task creation.
    sched::task::init_canary();

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

    boot_timing::mark(boot_timing::Milestone::ApicTimer);
    console::boot_step_update(console::BootStatus::Ok, "Interrupt controllers (APIC)");

    // Step 20c: Scan PCI bus for device discovery.
    // This finds virtio, USB, NVMe, and other PCI devices.
    console::boot_step(console::BootStatus::Running, "PCI & device drivers");
    if let Err(e) = pci::self_test() {
        serial_println!("WARNING: PCI scan failed: {}", e);
    }

    // Step 20d: virtio-net probe is done first (it doesn't need the
    // blkdev registry).  virtio-blk devices are discovered in the
    // multi-device init below (step 20e).

    // Step 20d-2: Probe for virtio-net network device.
    // Uses legacy PCI transport (I/O port BAR0) with polling.
    // Non-fatal if no NIC is present.
    virtio::net::init(boot_info.hhdm_offset);

    // Step 20d-2b: Initialize Intel e1000 NIC (if present).
    // Provides native NIC support for QEMU/VirtualBox without virtio.
    // Falls back gracefully if no Intel NIC is found.
    e1000::init(boot_info.hhdm_offset);

    // Step 20d-2c: Initialize Realtek RTL8139 NIC (if present).
    // Common on older hardware and available as a QEMU option.
    rtl8139::init(boot_info.hhdm_offset);

    // Step 20d-2d: Initialize Intel HD Audio controller (if present).
    // Discovers codecs, sets up CORB/RIRB command buffers, probes audio
    // topology for output path (DAC → Pin).  QEMU: `-device intel-hda
    // -device hda-duplex`.
    hda::init(boot_info.hhdm_offset);

    // Virtio-sound driver: modern VM audio via virtio.
    // QEMU: `-device virtio-sound-pci,audiodev=a0 -audiodev sdl,id=a0`
    if let Err(e) = virtio::sound::init(boot_info.hhdm_offset) {
        serial_println!("[virtio-snd] Init: {:?} (non-fatal)", e);
    }

    // AC97 audio controller: legacy audio for older hardware/VMs.
    // QEMU: `-device AC97,audiodev=a0 -audiodev sdl,id=a0`
    if let Err(e) = ac97::init(boot_info.hhdm_offset) {
        serial_println!("[ac97] Init: {:?} (non-fatal)", e);
    }

    // Virtio-GPU driver: 2D framebuffer for VMs.
    // QEMU: `-device virtio-gpu-pci`
    if let Err(e) = virtio::gpu::init(boot_info.hhdm_offset) {
        serial_println!("[virtio-gpu] Init: {:?} (non-fatal)", e);
    }

    // DRM/KMS subsystem: abstracts display hardware for the compositor.
    // Must come after both fb::init() and virtio-gpu init so that both
    // backends are available.
    drm::init();

    console::boot_step_update(console::BootStatus::Ok, "PCI & device drivers");

    // Step 20d-3: Initialize networking stack.
    // Sets up the network interface from the active NIC (virtio-net or e1000)
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
    // Discovers ALL virtio-blk devices on the PCI bus and registers
    // them as vda, vdb, vdc, etc.  QEMU can present multiple devices
    // (disk.img, ext4_test.img, swap.img).
    console::boot_step(console::BootStatus::Running, "Storage & filesystems");
    blkdev::init_multi(boot_info.hhdm_offset);

    // AHCI/SATA driver: detect and initialize SATA disks on real hardware.
    // Registers devices as sda, sdb, etc.  No-op in QEMU without SATA.
    ahci::init(boot_info.hhdm_offset);

    // NVMe driver: detect and initialize NVMe SSDs.
    // Registers devices as nvme0n1, nvme1n1, etc.  No-op without NVMe hardware.
    nvme::init(boot_info.hhdm_offset);

    // xHCI USB host controller: detect and enumerate USB devices.
    // No-op without xHCI hardware (common in QEMU unless -device qemu-xhci).
    xhci::init(boot_info.hhdm_offset);

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
    // Prefer a real FAT filesystem on vda (auto-detects FAT16/FAT32).  If no
    // on-disk filesystem is present, fall back to a volatile in-memory root so
    // the system *always* has a usable "/" — the virtual filesystems
    // (/proc, /dev, /sys, /tmp) mount underneath it and userspace sees a
    // working namespace even on a diskless boot.  (This also means the
    // boot-test, whose vda is a raw swap disk with no FAT, still exercises the
    // virtual-filesystem layer instead of silently skipping it.)
    let fat_ok = match fs::fat::init("vda") {
        Ok(()) => true,
        Err(e) => {
            serial_println!(
                "[fs] No FAT filesystem on vda: {:?} — using in-memory root (non-fatal)",
                e
            );
            if let Err(me) = fs::memfs::mount("/") {
                serial_println!(
                    "[boot] WARNING: failed to mount fallback in-memory root: {:?}",
                    me
                );
            }
            false
        }
    };

    // --- Virtual filesystem mounts (independent of which root we have) ---

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
    // Mount sysfs at /sys for kernel configuration and hardware info.
    // Writable for tunables (hostname, sysctl params), read-only for
    // system info (kernel version, PCI devices, cache stats).
    if let Err(e) = fs::sysfs::mount("/sys") {
        serial_println!("[boot] WARNING: failed to mount sysfs at /sys: {:?}", e);
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

    // --- Disk-backed filesystem self-tests ---
    // These exercise the on-disk FAT root (and its buffer-cache / journal /
    // recycle-bin layers), so they only run when a real FAT filesystem
    // mounted.  On a diskless (in-memory root) boot they are skipped; the
    // virtual-filesystem self-tests below still run unconditionally.
    if fat_ok {
        if let Err(e) = fs::fat::self_test() {
            serial_println!("WARNING: FAT self-test failed: {:?}", e);
        }
        // File handle self-test (requires mounted filesystem).
        if let Err(e) = fs::handle::self_test() {
            serial_println!("WARNING: File handle self-test failed: {:?}", e);
        }
        // io_ring file handle test (requires mounted /tmp).
        if let Err(e) = ipc::io_ring::self_test_fh() {
            serial_println!("WARNING: io_ring file handle self-test failed: {:?}", e);
        }
        // Buffer cache self-test (validates caching, write-back, LRU).
        if let Err(e) = fs::cache::self_test() {
            serial_println!("WARNING: Buffer cache self-test failed: {:?}", e);
        }
        // Recycle bin self-test (trash, list, restore, empty).
        if let Err(e) = fs::trash::self_test() {
            serial_println!("WARNING: Recycle bin self-test failed: {:?}", e);
        }
        // Change notification self-test (watch, emit, read, close).
        if let Err(e) = fs::notify::self_test() {
            serial_println!("WARNING: Change notification self-test failed: {:?}", e);
        }
        // Change journal self-test (persistent change tracking).
        if let Err(e) = fs::journal::self_test() {
            serial_println!("WARNING: Change journal self-test failed: {:?}", e);
        }
        // ext4 self-test (reads directory listing and files if mounted).
        if let Err(e) = fs::ext4::self_test() {
            serial_println!("WARNING: ext4 self-test failed: {:?}", e);
        }
        // VFS-level self-test (symlinks, cross-mount resolution) — relies on
        // the FAT root for cross-mount cases.
        if let Err(e) = fs::vfs::self_test() {
            serial_println!("WARNING: VFS self-test failed: {:?}", e);
        }
        // Flush buffer cache to disk so data survives power loss / QEMU kill.
        if let Err(e) = fs::cache::flush_all() {
            serial_println!("WARNING: Buffer cache flush failed: {:?}", e);
        }
    }

    // --- Virtual filesystem self-tests (run on any root) ---
    // These construct their own filesystem instances and do not depend on a
    // real on-disk FAT root, so they run regardless of how "/" was mounted.
    // In-memory filesystem self-test (standalone, doesn't touch VFS mount).
    if let Err(e) = fs::memfs::self_test() {
        serial_println!("WARNING: MemFs self-test failed: {:?}", e);
    }
    // devfs self-test (validates device file operations).
    if let Err(e) = fs::devfs::self_test() {
        serial_println!("WARNING: DevFs self-test failed: {:?}", e);
    }
    // sysfs self-test (validates kernel tunables, hostname, PCI).
    if let Err(e) = fs::sysfs::self_test() {
        serial_println!("WARNING: SysFs self-test failed: {:?}", e);
    }

    boot_timing::mark(boot_timing::Milestone::Filesystem);

    // ProcFs self-test — constructs its own `ProcFs::new()` and reads live
    // scheduler/PCB state directly, so it needs no mounted backing store and
    // must run unconditionally.  (It was previously gated behind a successful
    // FAT mount on vda, which meant the boot-test — whose vda is a raw swap
    // disk with no FAT — never exercised procfs at all.)
    if let Err(e) = fs::procfs::self_test() {
        serial_println!("WARNING: ProcFs self-test failed: {:?}", e);
    }

    // Compression self-tests — pure in-memory, no mounted FS required.
    if let Err(e) = fs::compress::self_test() {
        serial_println!("WARNING: Compression self-test failed: {:?}", e);
    }
    if let Err(e) = fs::bzip2::self_test() {
        serial_println!("WARNING: Bzip2 self-test failed: {:?}", e);
    }
    if let Err(e) = fs::xz::self_test() {
        serial_println!("WARNING: XZ self-test failed: {:?}", e);
    }
    if let Err(e) = fs::zstd::self_test() {
        serial_println!("WARNING: Zstd self-test failed: {:?}", e);
    }
    if let Err(e) = fs::sevenz::self_test() {
        serial_println!("WARNING: 7z self-test failed: {:?}", e);
    }
    if let Err(e) = fs::cpio::self_test() {
        serial_println!("WARNING: CPIO self-test failed: {:?}", e);
    }
    if let Err(e) = fs::ar::self_test() {
        serial_println!("WARNING: ar self-test failed: {:?}", e);
    }
    if let Err(e) = fs::zip::self_test() {
        serial_println!("WARNING: ZIP self-test failed: {:?}", e);
    }
    if let Err(e) = fs::tar::self_test() {
        serial_println!("WARNING: tar self-test failed: {:?}", e);
    }
    if let Err(e) = fs::lz4::self_test() {
        serial_println!("WARNING: LZ4 self-test failed: {:?}", e);
    }
    if let Err(e) = fs::rar::self_test() {
        serial_println!("WARNING: RAR self-test failed: {:?}", e);
    }
    if let Err(e) = fs::index::self_test() {
        serial_println!("WARNING: File index self-test failed: {:?}", e);
    }
    if let Err(e) = fs::cas::self_test() {
        serial_println!("WARNING: CAS self-test failed: {:?}", e);
    }
    if let Err(e) = fs::integrity::self_test() {
        serial_println!("WARNING: Integrity monitoring self-test failed: {:?}", e);
    }
    if let Err(e) = fs::history::self_test() {
        serial_println!("WARNING: File history self-test failed: {:?}", e);
    }
    if let Err(e) = fs::mime::self_test() {
        serial_println!("WARNING: MIME detection self-test failed: {:?}", e);
    }
    // taskstats backs /proc/taskstats; its self-test builds fixtures via the
    // real accounting API and resets the table afterward (leaving no
    // fabricated rows), so it is safe to run during boot and gives the
    // module automated coverage it otherwise lacks (it was previously only
    // reachable via the `taskstats test` kshell subcommand).
    fs::taskstats::self_test();
    // iolatency backs /proc/iolatency; like taskstats its self-test now builds
    // fixtures via the real register_device/record API and resets the table
    // afterward (leaving no fabricated devices), so it is safe at boot and
    // gives the module automated coverage it previously lacked (it was only
    // reachable via the `iolatency test` kshell subcommand).
    fs::iolatency::self_test();
    // netsock backs /proc/netsock; like taskstats/iolatency its self-test now
    // builds fixtures via the real open/close/record API and resets the table
    // afterward (leaving no fabricated sockets), so it is safe at boot and
    // gives the module automated coverage it previously lacked (it was only
    // reachable via the `netsock test` kshell subcommand).
    fs::netsock::self_test();
    // slabstat backs /proc/slabstat; like taskstats/iolatency/netsock its
    // self-test now builds fixtures via the real create_cache/alloc/free API
    // and resets the table afterward (leaving no fabricated caches), so it is
    // safe at boot and gives the module automated coverage it previously
    // lacked (it was only reachable via the `slabstat test` kshell subcommand).
    fs::slabstat::self_test();
    // futexstat backs /proc/futexstat; like its siblings the self-test now
    // builds fixtures via the real record_wait/record_wake API and resets the
    // table afterward (leaving no fabricated futex/process rows), so it is
    // safe at boot and gives the module automated coverage it previously
    // lacked (it was only reachable via the `futexstat test` kshell subcommand).
    fs::futexstat::self_test();
    // pipestat backs /proc/pipestat; like its siblings the self-test now
    // builds fixtures via the real create/destroy/record_write/record_read API
    // and resets the table afterward (leaving no fabricated pipes), so it is
    // safe at boot and gives the module automated coverage it previously
    // lacked (it was only reachable via the `pipestat test` kshell subcommand).
    fs::pipestat::self_test();
    // epollstat backs /proc/epollstat; like its siblings the self-test now
    // builds fixtures via the real create_instance/add_fd/record_wait API and
    // resets the table afterward (leaving no fabricated instances), so it is
    // safe at boot and gives the module automated coverage it previously
    // lacked (it was only reachable via the `epollstat test` kshell subcommand).
    fs::epollstat::self_test();
    // aiostat backs /proc/aiostat (io_uring-style submission-queue monitoring);
    // like its siblings the self-test now builds fixtures via the real
    // create_ring/submit/complete/overflow API and resets the table afterward
    // (leaving no fabricated rings), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `aiostat test` kshell subcommand).
    fs::aiostat::self_test();
    // netlat backs /proc/netlat (per-interface network RTT/processing latency);
    // like its siblings the self-test now builds fixtures via the real
    // register_iface/record_rtt/record_processing API and resets the table
    // afterward (leaving no fabricated interfaces), so it is safe at boot and
    // gives the module automated coverage it previously lacked (it was only
    // reachable via the `netlat test` kshell subcommand).
    fs::netlat::self_test();
    // migstat backs /proc/migstat (per-CPU/per-task scheduler migration stats);
    // like its siblings the self-test now builds fixtures via the real
    // register_cpu/register_task/record API and resets the table afterward
    // (leaving no fabricated rows), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `migstat test` kshell subcommand).
    fs::migstat::self_test();
    // rcustat backs /proc/rcustat (RCU grace-period/callback/per-CPU stats);
    // like its siblings the self-test now builds fixtures via the real
    // register_cpu/begin_gp/end_gp/queue_callback API and resets the table
    // afterward (leaving no fabricated rows), so it is safe at boot and gives
    // the module automated coverage it previously lacked (it was only reachable
    // via the `rcustat test` kshell subcommand).
    fs::rcustat::self_test();
    // tlbstat backs /proc/tlbstat (per-CPU TLB hit/miss/shootdown/flush stats);
    // like its siblings the self-test now builds fixtures via the real
    // register_cpu/record_hit/record_miss/record_shootdown/record_flush API and
    // resets the table afterward (leaving no fabricated rows), so it is safe at
    // boot and gives the module automated coverage it previously lacked (it was
    // only reachable via the `tlbstat test` kshell subcommand).
    fs::tlbstat::self_test();
    // Register default file type associations, then self-test.
    fs::associations::register_defaults();
    if let Err(e) = fs::associations::self_test() {
        serial_println!("WARNING: File associations self-test failed: {:?}", e);
    }

    // Filesystem quota self-test.
    if let Err(e) = fs::quota::self_test() {
        serial_println!("WARNING: Filesystem quota self-test failed: {:?}", e);
    }
    // ACL self-test.
    if let Err(e) = fs::acl::self_test() {
        serial_println!("WARNING: ACL self-test failed: {:?}", e);
    }
    // Filesystem interceptor self-test.
    if let Err(e) = fs::intercept::self_test() {
        serial_println!("WARNING: FS interceptor self-test failed: {:?}", e);
    }
    // Symlink/hardlink security self-test.
    if let Err(e) = fs::symlink_security::self_test() {
        serial_println!("WARNING: Symlink security self-test failed: {:?}", e);
    }
    // Resource limits self-test.
    if let Err(e) = fs::rlimit::self_test() {
        serial_println!("WARNING: Resource limits self-test failed: {:?}", e);
    }
    // Overlay filesystem self-test.
    if let Err(e) = fs::overlay::self_test() {
        serial_println!("WARNING: Overlay filesystem self-test failed: {:?}", e);
    }
    // Named pipe self-test.
    if let Err(e) = fs::pipe::self_test() {
        serial_println!("WARNING: Named pipe self-test failed: {:?}", e);
    }
    // Tmpwatch self-test.
    if let Err(e) = fs::tmpwatch::self_test() {
        serial_println!("WARNING: Tmpwatch self-test failed: {:?}", e);
    }
    // Filesystem audit self-test.
    if let Err(e) = fs::audit::self_test() {
        serial_println!("WARNING: Filesystem audit self-test failed: {:?}", e);
    }
    // Mount namespace self-test.
    if let Err(e) = fs::mount_ns::self_test() {
        serial_println!("WARNING: Mount namespace self-test failed: {:?}", e);
    }

    // Run cryptographic self-tests.
    if let Err(e) = crypto::self_test() {
        serial_println!("WARNING: SHA-256 self-test failed: {:?}", e);
    }
    if let Err(e) = crypto::self_test_crc32c() {
        serial_println!("WARNING: CRC32C self-test failed: {:?}", e);
    }
    if let Err(e) = crypto::self_test_tls_crypto() {
        serial_println!("WARNING: TLS crypto self-test failed: {:?}", e);
    }
    if let Err(e) = crypto::self_test_ed25519() {
        serial_println!("WARNING: Ed25519/SHA-512 self-test failed: {:?}", e);
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

    // Test sleep_ns (requires interrupts for hrtimer-based wake).
    // Must run after interrupts are enabled because the hrtimer callback
    // fires from the APIC timer ISR.
    if let Err(e) = sched::test_sleep_ns_postboot() {
        serial_println!("FATAL: sleep_ns self-test failed: {}", e);
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

    // Initialize PS/2 mouse on port 2 (IRQ 12).
    // Must be after keyboard init (which sets up the i8042 controller).
    //
    // SAFETY: IOAPIC and IDT initialized, keyboard already set up the
    // controller.  Called exactly once.
    unsafe {
        mouse::init();
    }

    if let Err(e) = mouse::self_test() {
        serial_println!("[mouse] Self-test failed: {} (non-fatal)", e);
        // Non-fatal: system can boot without a mouse.
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

    boot_timing::mark(boot_timing::Milestone::Smp);

    // Step 22b¾: CPU topology detection.
    // Now that all CPUs are online, decode APIC ID hierarchy to determine
    // package/core/SMT relationships for topology-aware scheduling.
    cpu_topology::detect();

    // CPU hotplug framework initialization — marks all online CPUs.
    cpu_hotplug::init();
    cpu_hotplug::self_test();

    // NUMA topology detection — parse SRAT or default to UMA.
    // Requires ACPI tables and SMP to be initialized.
    numa::init();
    numa::self_test();

    // Kernel symbol table self-test.
    ksyms::self_test();

    // Kernel event bus self-test.
    kevent::self_test();

    // RCU (Read-Copy-Update) self-test.
    // Must be after scheduler (needs yield_now for synchronize).
    rcu::self_test();

    // MSI (Message Signaled Interrupts) self-test.
    // Vector allocation pool and address/data formatting.
    // Must be after PCI init (uses config space accessors).
    msi::self_test();

    // IRQ balancer initialization — distributes interrupts across CPUs.
    // Requires IOAPIC, SMP, and cpu_hotplug to be initialized.
    irqbalance::init();
    irqbalance::self_test();

    // Step 22b⅞: CPU frequency scaling initialization.
    // Detect HWP or EIST support and set default governor (performance).
    cpufreq::init();

    // Step 22b⅞+: Thermal monitoring initialization.
    // Detect DTS support, read Tj_max, take initial temperature reading.
    thermal::init();

    // Step 22b⅞++: Power management self-test.
    // Verifies ACPI shutdown/reboot capability reporting (FADT parsed in acpi::init).
    power::self_test();

    // Step 22c: TLB shootdown self-test.
    // Now that all CPUs are online, verify the TLB shootdown IPI works.
    tlb::self_test();

    // Step 22d: DMA buffer management self-test.
    // Verifies contiguous physical allocation and free for device DMA.
    mm::dma::self_test();

    // Step 22e: Copy-on-Write self-test.
    // Verifies refcount API and COW PTE flag manipulation.
    mm::cow::self_test();

    // Step 22e½: Huge page (2 MiB) self-test.
    // Verifies alloc/map/read/write/unmap/free of 2 MiB huge pages.
    mm::hugepage::self_test();

    // Step 22e¾: vmalloc self-test.
    // Verifies virtual-contiguous allocations backed by discontiguous frames.
    mm::vmalloc::self_test();

    // Step 22e⅞: Reverse mapping (rmap) self-test.
    // Verifies add/remove/lookup of physical frame → virtual address mappings.
    mm::rmap::self_test();

    // Step 22e⅞+: Kernel virtual address space validation.
    // Ensures no VA regions overlap (catches configuration bugs at boot).
    mm::kvspace::self_test();

    // Step 22e⅞+: Memory poison self-test.
    // Verifies poison fill/verify for use-after-free and overflow detection.
    mm::poison::self_test();

    // Step 22e⅞+: Memory watermark self-test.
    // Verifies per-subsystem peak usage tracking.
    mm::watermark::self_test();

    // Step 22e⅞+++: TLB flush gather self-test.
    // Verifies batched TLB shootdown and deferred frame free.
    mm::tlb_gather::self_test();

    // Step 22e⅞++++a: Migration type system initialization and self-test.
    // Classifies frames as unmovable/movable/reclaimable for compaction.
    mm::migrate_type::init();
    mm::migrate_type::self_test();

    // Step 22e⅞++++b: Page aging self-test.
    // Tracks page access patterns for intelligent reclaim decisions.
    mm::page_age::self_test();

    // Step 22e⅞++++c: Page table walker self-test.
    // Generic page table iteration for RSS counting, fork, etc.
    mm::pt_walk::self_test();

    // Step 22e⅞++++d: Memory scrubber self-test.
    // Proactive ECC error detection via background memory reads.
    mm::scrub::self_test();

    // Step 22e⅞++++e: Memory fault injection self-test.
    // Verifies controlled allocation failure simulation for testing error paths.
    mm::fault_inject::self_test();

    // Step 22e⅞++++g: Frame ownership tracker self-test.
    // Per-frame subsystem tagging for "who allocated this memory?" diagnostics.
    mm::frame_owner::self_test();

    // Step 22e⅞++++h: Allocation trace ring buffer self-test.
    // Records recent alloc/free events for post-mortem debugging.
    mm::alloc_trace::self_test();

    // Step 22e⅞++++i: Allocation latency histogram self-test.
    // Measures and profiles alloc/free timing for performance analysis.
    mm::alloc_lat::self_test();

    // Step 22e⅞++++j: Heap allocation profiler self-test.
    // Tracks allocation size distribution for slab tuning.
    mm::heap_profile::self_test();

    // Step 22e⅞++++k: Syscall profiler self-test.
    // Per-syscall invocation count and latency tracking.
    syscall::profile::self_test();

    // Step 22e⅞++++l: Allocation checkpoint self-test.
    // Memory state checkpoints for leak detection (save/diff).
    mm::alloc_checkpoint::self_test();

    // Step 22e⅞++++m: Syscall tracer self-test.
    // Per-event syscall capture for strace-like debugging.
    syscall::trace::self_test();

    // Step 22e⅞++++n: IPC statistics self-test.
    // Per-mechanism usage and performance counters.
    ipc::stats::self_test();

    // Step 22e⅞++++n2: TCP server (bind/listen/accept) self-test.
    // Validates listener lifecycle without needing network hardware.
    if let Err(e) = net::tcp::self_test() {
        serial_println!("[WARN] TCP self-test failed: {:?}", e);
    }

    // Step 22e⅞++++n3: Firewall self-test.
    // Stateful packet filtering with rules and connection tracking.
    if let Err(e) = net::firewall::self_test() {
        serial_println!("[WARN] Firewall self-test failed: {:?}", e);
    }

    // Step 22e⅞++++n4: Network stack per-module self-tests.
    // Exercises protocol parsing/building for ethernet, IPv4, ICMP, ARP,
    // UDP, DNS, DHCP, fragmentation, and interface modules.
    if let Err(e) = net::self_test() {
        serial_println!("[WARN] Network self-test failed: {:?}", e);
    }

    // Step 22e⅞++++o: Kernel object tracking self-test.
    // Lifecycle counters for all kernel object types.
    kobject::self_test();

    // Step 22e⅞++++p: Fragmentation history self-test.
    // Tracks memory fragmentation over time for trend analysis.
    mm::frag_history::self_test();

    // Step 22e⅞++++p2: Memory type accounting self-test.
    // Verifies charge/uncharge/peak tracking for memory usage breakdown.
    mm::memtype::self_test();

    // Step 22e⅞++++p3: Memory compaction self-test.
    // Verifies fragmentation analysis, rmap iteration, and migration API.
    mm::compact::self_test();

    // Step 22e⅞++++p4: VMA management self-test.
    // Verifies add/remove/find/overlap/alignment checks for address spaces.
    mm::vma::self_test();

    // Step 22e⅞++++p5: Resource control groups (cgroup) self-test.
    // Verifies hierarchy, CPU/memory controllers, charge/uncharge, limits.
    cgroup::self_test();

    // Step 22e⅞++++p6: PID namespace subsystem init + self-test.
    pidns::init();
    pidns::self_test();

    // Step 22e⅞++++p7: User namespace subsystem init + self-test.
    // UID/GID remapping for rootless containers.  Supports hierarchical
    // mappings with up to 16 ranges per namespace, process tracking,
    // and full host UID resolution through nested namespaces.
    userns::init();
    userns::self_test();

    // Step 22e⅞++++p8: Network namespace subsystem init + self-test.
    // Per-container network isolation with independent interface config,
    // routing tables (longest-prefix match + metric tie-breaking), and
    // process tracking.
    netns::init();
    netns::self_test();

    // Step 22e⅞++++p8b: Virtual Ethernet (veth) pairs init + self-test.
    // Connected virtual links between namespaces — frame sent on one
    // end appears on the peer's RX queue.  Required for container
    // networking isolation (per-namespace ARP, independent routing).
    // Runs after netns::init() because veth tests create child namespaces.
    net::veth::init();
    if let Err(e) = net::veth::self_test() {
        serial_println!("[WARN] Veth self-test failed: {:?}", e);
    }

    // Step 22e⅞++++p8c: Per-namespace ARP cache self-test.
    // Isolated MAC resolution per namespace — requires netns::init().
    if let Err(e) = net::arp::ns_self_test() {
        serial_println!("[WARN] Per-namespace ARP self-test failed: {:?}", e);
    }

    // Step 22e⅞++++p8d: NAT/masquerade self-test.
    // Source NAT for container traffic traversing namespace boundaries.
    if let Err(e) = net::nat::self_test() {
        serial_println!("[WARN] NAT self-test failed: {:?}", e);
    }

    // SSH server self-test (binary packet protocol, encryption, key derivation).
    if let Err(e) = net::ssh::self_test() {
        serial_println!("[WARN] SSH self-test failed: {:?}", e);
    }

    // Step 22e⅞++++p9: Container lifecycle manager init + self-test.
    // Unified container abstraction tying PID/user/network namespaces + cgroup
    // into a single lifecycle with create/start/stop/delete state machine.
    container::init();
    container::self_test();

    // Step 22e⅞++++p10a: JSON parser self-test.
    // Minimal recursive-descent JSON parser for OCI image manifests.
    if let Err(e) = json::self_test() {
        serial_println!("[WARN] JSON self-test failed: {:?}", e);
    }

    // Step 22e⅞++++p10b: OCI image format parser self-test.
    // Parses OCI image index, manifest, config, and verifies digests.
    if let Err(e) = oci::self_test() {
        serial_println!("[WARN] OCI self-test failed: {:?}", e);
    }

    // Step 22e⅞++++p10: Syscall filter (seccomp-equivalent) init + self-test.
    // Per-process bitmap-based syscall allow/deny lists for container
    // sandboxing.  O(1) check per syscall, fork-inheritable, tighten-only.
    scfilter::init();
    scfilter::self_test();

    // Step 22e⅞++++q: Self-test runner infrastructure test.
    // Verifies the centralized test runner can enumerate suites.
    selftest::self_test();

    // Step 22e⅞++++r: Watchpoint self-test.
    // Software memory watchpoints for debugging value changes.
    watchpoint::self_test();

    // Step 22e⅞++++s: Kernel snapshot self-test.
    // Comprehensive system state capture for before/after comparison.
    ksnapshot::self_test();

    // Step 22e⅞++++t: RIP sampler self-test.
    // Statistical profiler — samples instruction pointer on timer ticks.
    rip_sample::self_test();

    // Step 22e⅞++++u: Invariant checker self-test.
    // Verifies system-wide consistency properties (memory, scheduler, objects).
    invariant::self_test();

    // Step 22e⅞++++v: Scheduler migration tracker self-test.
    // Records and analyzes task migration events between CPUs.
    sched_migrate::self_test();

    // Step 22e⅞++++w: Wait channel tracker self-test.
    // Tracks what blocked tasks are waiting on (WCHAN for ps/top).
    wchan::self_test();

    // Step 22e⅞++++x: Diagnostic report generator self-test.
    // Comprehensive system state collection for bug reports.
    kdiag::self_test();

    // Step 22e⅞++++y: Hypervisor detection self-test.
    // Verifies CPUID-based VM detection and signature matching.
    hypervisor::self_test();

    // Step 22e⅞++++z: Scheduler fairness measurement self-test.
    // Computes Jain's Fairness Index for CPU time distribution.
    sched_fairness::self_test();

    // Step 22e⅞+++++a: Intel CET self-test.
    // Verifies CET detection, error code parsing, and CR4 state.
    cet::self_test();

    // Step 22e⅞+++++b: SMEP/SMAP self-test.
    // Verifies hardware execution/access prevention is enabled.
    smep_smap::self_test();

    // Step 22e⅞+++++c: Spectre/Meltdown mitigation self-test.
    // Verifies IBRS/STIBP/SSBD MSRs are set and IBPB barrier works.
    spectre::self_test();

    // Step 22e⅞+++++d: IOMMU detection self-test.
    // Verifies API consistency (available ↔ vendor ↔ unit_count).
    if let Err(e) = iommu::self_test() {
        serial_println!("[WARN] IOMMU self-test failed: {:?}", e);
    }

    // Step 22e⅞+++++d½: IOMMU DMA remapping self-test.
    // Tests page table manipulation (domain create/map/unmap/destroy).
    if let Err(e) = iommu_remap::self_test() {
        serial_println!("[WARN] IOMMU remap self-test failed: {:?}", e);
    }

    // AHCI/SATA driver self-test.
    ahci::self_test();

    // NVMe driver self-test.
    nvme::self_test();

    // xHCI USB host controller self-test.
    xhci::self_test();

    // Intel e1000 NIC self-test.
    e1000::self_test();

    // Realtek RTL8139 NIC self-test.
    rtl8139::self_test();

    // Intel HD Audio self-test.
    if let Err(e) = hda::self_test() {
        serial_println!("[hda] Self-test failed: {:?} (non-fatal)", e);
    }

    // PC speaker self-test.
    pcspk::self_test();

    // Virtio-sound self-test.
    virtio::sound::self_test();

    // AC97 audio self-test.
    ac97::self_test();

    // Virtio-GPU self-test.
    virtio::gpu::self_test();

    // Audio mixer self-test.
    audio_mixer::self_test();

    // System notification sounds self-test.
    audio_notify::self_test();

    // Sound history self-test.
    audio_history::self_test();

    // Framebuffer graphics self-test.
    if let Err(e) = fb::self_test() {
        serial_println!("[fb] Self-test failed: {} (non-fatal)", e);
    }

    // DRM/KMS subsystem self-test.
    if let Err(e) = drm::self_test() {
        serial_println!("[drm] Self-test failed: {:?} (non-fatal)", e);
    }

    // Console VT100/ANSI escape sequence self-test.
    console::self_test();

    // Terminal session multiplexer init + self-test.
    termsession::init();
    if let Err(e) = termsession::self_test() {
        serial_println!("[termsession] Self-test failed: {:?} (non-fatal)", e);
    }

    // Unicode support self-test (UTF-8 decoding, box drawing, block elements).
    unicode::self_test();

    // Step 22e⅞++++f: Memory subsystem integration tests.
    // End-to-end tests exercising alloc→map→access→unmap→free pipeline.
    mm::integ_test::self_test();

    // Step 22e⅞++++: PCID (Process Context Identifiers) initialization.
    // Enables TLB tagging to avoid full flushes on context switch.
    mm::pcid::detect();
    mm::pcid::enable_on_this_cpu();
    mm::pcid::self_test();

    // Step 22f: Initialize the soft lockup detector (watchdog).
    // Must be after SMP bootstrap so cpu_count() is accurate.
    // Monitors per-CPU heartbeats and warns if any CPU stops responding.
    watchdog::init();
    watchdog::self_test();

    // Step 22f1.5: Initialize MWAIT-based idle (power-efficient CPU sleep).
    idle::init();
    idle::self_test();

    // Step 22f2: Stack backtrace self-test.
    // Verifies frame pointer chain walking works (requires -C force-frame-pointers=yes).
    // Gracefully skips if frame pointers are missing (e.g., optimized-out in release).
    backtrace::self_test();

    // Step 22f3: Initialize lock order validator (lockdep).
    // Detects potential deadlocks via dependency graph cycle detection.
    // Must be after SMP init so current_cpu_index() works on all CPUs.
    lockdep::init();
    lockdep::self_test();

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

    // Step 22f-3: Enable slab poisoning and run its self-test.
    // Fills freed heap memory with a poison pattern and checks integrity
    // on reallocation — catches use-after-free bugs automatically.
    // Enabled during boot self-tests, disabled before benchmarks for speed.
    mm::heap::enable_poison();
    mm::heap::poison_self_test();

    // Step 22g: I/O scheduler self-test.
    // BFQ-style budget fair queueing with per-process queues,
    // priority classes, elevator ordering, and request merging.
    sched::io_sched::self_test();

    // Step 22h: Wait queue self-test.
    sched::waitqueue::self_test();

    // Step 22i: Sleeping mutex self-test.
    sched::kmutex::self_test();

    // Step 22j: Counting semaphore self-test.
    sched::semaphore::self_test();

    // Step 22k: Condition variable self-test.
    sched::condvar::self_test();

    // Step 22k2: Reader-writer lock self-test.
    sched::krwlock::self_test();

    // Step 22k3: Barrier self-test.
    sched::barrier::self_test();

    // Step 22k4: One-shot event self-test.
    sched::once_event::self_test();

    // Step 22k5: Kernel channel self-test.
    sched::kchannel::self_test();

    // Step 22k6: Kernel trace buffer self-test.
    ktrace::self_test();

    // Step 22k7: EEVDF scheduler self-test.
    if let Err(e) = sched::eevdf::self_test() {
        serial_println!("EEVDF self-test FAILED: {:?}", e);
    }

    // Step 22k8: Deadline scheduler self-test.
    if let Err(e) = sched::deadline::self_test() {
        serial_println!("Deadline scheduler self-test FAILED: {:?}", e);
    }

    // Step 22k9: Scheduler backend enum self-test.
    sched::backend::self_test();

    // Step 22l: Task supervisor self-test.
    sched::supervisor::self_test();

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

    // Self-test hardware performance counters (PMU).
    // Must be after CPU feature detection (uses pmu_version/counters from
    // cpu::features()).  If PMU is unavailable (QEMU without -cpu host),
    // the test gracefully skips.
    pmc::self_test();

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
    mm::accounting::self_test();
    mm::rlimits::self_test();
    mm::pressure::self_test();
    mm::mempool::self_test();

    // Step 22c: Spawn workqueue worker task.
    // Provides deferred work execution in full process context (can sleep,
    // allocate, take locks).  Must be after scheduler (Step 10).
    match workqueue::init() {
        Ok(()) => {}
        Err(e) => {
            serial_println!("[boot] WARNING: failed to spawn workqueue worker: {:?}", e);
        }
    }
    workqueue::self_test();

    // Step 22d: Kernel timers self-test.
    // ktimer fires callbacks via the workqueue after a tick-based delay.
    // Requires both the workqueue worker and TIMER softirq to be active.
    ktimer::self_test();

    // Step 22d½: High-resolution timer self-test.
    // Verifies scheduling, cancellation, ordering, and repeating timers.
    hrtimer::self_test();

    // Channel recv_timeout self-test (requires hrtimer for sleep_ms).
    if let Err(e) = ipc::channel::self_test_timeout() {
        serial_println!("[FATAL] Channel timeout self-test failed: {:?}", e);
    }

    // Futex wait_timeout self-test (requires hrtimer).
    if let Err(e) = ipc::futex::self_test_timeout() {
        serial_println!("[FATAL] Futex timeout self-test failed: {:?}", e);
    }

    // Eventfd read_timeout self-test (requires hrtimer).
    if let Err(e) = ipc::eventfd::self_test_timeout() {
        serial_println!("[FATAL] Eventfd timeout self-test failed: {:?}", e);
    }

    // Service registry self-test (requires scheduler + channels).
    if let Err(e) = ipc::service::self_test() {
        serial_println!("[FATAL] Service registry self-test failed: {:?}", e);
    }

    // Service limits self-test.
    if let Err(e) = ipc::service_limits::self_test() {
        serial_println!("[FATAL] Service limits self-test failed: {:?}", e);
    }

    // Namespace self-test (pure in-memory, no dependencies beyond alloc).
    if let Err(e) = ipc::namespace::self_test() {
        serial_println!("[FATAL] Namespace self-test failed: {:?}", e);
    }

    // Step 22e: CSPRNG self-test.
    // Verifies output quality now that we've accumulated some interrupt
    // entropy during the boot process (ISR timing mixed in).
    rng::self_test();

    // Zero-on-free test — runs here because it needs HHDM + per-CPU
    // caches, which aren't available during the early frame allocator
    // self-test (test 7 skips there with "HHDM not ready").
    if let Err(e) = mm::frame::test_zero_on_free() {
        serial_println!("[FATAL] Zero-on-free self-test failed: {:?}", e);
    }

    boot_timing::mark(boot_timing::Milestone::SelfTests);

    // Security posture summary — one consolidated view of active protections.
    print_security_posture();

    // Boot success marker — the boot test script greps for this.
    // Printed synchronously so it appears within seconds of power-on,
    // regardless of how long deferred benchmarks take.
    serial_println!("=== Kernel boot complete ===");
    serial_println!("BOOT_OK");
    boot_timing::mark(boot_timing::Milestone::ShellReady);

    // Show boot-complete on the framebuffer console too.
    console_println!();
    console_println!("=== Kernel boot complete ===");

    // Play the boot-complete notification sound (uses mixer if available,
    // falls back to PC speaker chime).
    audio_notify::play(audio_notify::NotifySound::BootComplete);

    // Spawn the background mouse cursor task.
    // Continuously drains mouse events and updates the framebuffer cursor,
    // keeping cursor movement smooth even under load.
    mouse::spawn_cursor_task();

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
        "../../services/init/target/x86_64-unknown-none/release/init"
    );
    static HELLO_ELF: &[u8] = include_bytes!(
        "../../services/hello/target/x86_64-unknown-none/release/hello"
    );
    static TICKER_ELF: &[u8] = include_bytes!(
        "../../services/ticker/target/x86_64-unknown-none/release/ticker"
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

    // Emit boot-complete event to the system event log.
    syslog!("system.boot", Info, "Kernel boot complete, spawning init");

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
        fd_map: &[],
        argv: &[],
        envp: &[],
        // init is a compiled-in (INIT_ELF) binary with no on-disk path, so
        // we record no exe path rather than fabricate one to a file that
        // may not exist; /proc/1/exe therefore reports NotFound.
        exe_path: None,
    };
    match proc::spawn::spawn_process(INIT_ELF, &spawn_opts) {
        Ok(result) => {
            serial_println!(
                "[init] Init process spawned: pid={}, tid={}, entry={:#x}",
                result.pid, result.task_id, result.entry_point
            );
            syslog!("process.launch", Info, "Init process spawned (pid={})", result.pid);
            // The init process is now in the scheduler's run queue.
            // Drop into the idle loop — the scheduler will switch to
            // init when it gets a time slice.
            idle_loop();
        }
        Err(e) => {
            serial_println!("[init] FAILED to spawn init: {:?}", e);
            serial_println!("[init] Falling back to kernel debug shell");
            syslog!("process.launch", Error, "Init spawn failed: {:?}, falling back to kshell", e);
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
    // Disable slab poisoning during benchmarks — the memset/memcmp overhead
    // would skew heap allocator measurements by ~10-50ns per operation.
    mm::heap::disable_poison();
    bench::run_all();
    // Re-enable poisoning after benchmarks for continued UAF detection
    // during normal kernel operation.
    mm::heap::enable_poison();
    serial_println!("BENCH_OK");
}

// ---------------------------------------------------------------------------
// Idle loop
// ---------------------------------------------------------------------------

/// The kernel idle loop.
///
/// Print a consolidated security posture summary near end-of-boot.
///
/// Shows all hardware security features (active vs deferred vs unavailable)
/// in a compact format for quick auditing of the boot serial log.
fn print_security_posture() {
    use alloc::string::String;

    let mut active: String = String::new();
    let mut deferred: String = String::new();

    // SMEP — supervisor mode execution prevention.
    if smep_smap::smep_active() {
        active.push_str(" SMEP");
    }

    // SMAP — supervisor mode access prevention (deferred until STAC/CLAC paths).
    let smap_status = smep_smap::status();
    if smap_status.smap_active {
        active.push_str(" SMAP");
    } else if smap_status.hw_smap {
        deferred.push_str(" SMAP(needs-STAC/CLAC)");
    }

    // UMIP — user-mode instruction prevention.
    if smap_status.umip_active {
        active.push_str(" UMIP");
    }

    // NX — no-execute (always enabled on x86_64 long mode via IA32_EFER.NXE).
    active.push_str(" NX");

    // Stack canary — randomized per-boot via CSPRNG.
    // If the canary differs from the fallback constant, it was randomized.
    let canary = sched::task::stack_canary();
    if canary != 0xDEAD_BEEF_CAFE_BABE {
        active.push_str(" StackCanary(random)");
    } else {
        active.push_str(" StackCanary(fixed)");
    }

    // Guard pages — always present (kernel stack allocator inserts them).
    active.push_str(" GuardPages");

    // PCID — process-context identifiers (TLB optimization).
    if mm::pcid::is_enabled() {
        active.push_str(" PCID");
    }

    // Spectre/Meltdown mitigations.
    let spectre_s = spectre::status();
    if spectre_s.ibrs_active {
        if spectre_s.enhanced_ibrs {
            active.push_str(" IBRS(enhanced)");
        } else {
            active.push_str(" IBRS");
        }
    }
    if spectre_s.stibp_active {
        active.push_str(" STIBP");
    }
    if spectre_s.ssbd_active {
        active.push_str(" SSBD");
    }
    if spectre_s.meltdown_immune {
        active.push_str(" Meltdown-immune");
    }

    // CET — control-flow enforcement (deferred until toolchain support).
    let cet_status = cet::status();
    if cet_status.supervisor_shstk {
        active.push_str(" CET-SS");
    } else if cet_status.hw_shstk {
        deferred.push_str(" CET-SS(needs-toolchain)");
    }
    if cet_status.supervisor_ibt {
        active.push_str(" CET-IBT");
    } else if cet_status.hw_ibt {
        deferred.push_str(" CET-IBT(needs-toolchain)");
    }

    // IOMMU — DMA sandboxing for driver isolation.
    if iommu::is_available() {
        let vendor_str = match iommu::vendor() {
            iommu::IommuVendor::IntelVtd => "VT-d",
            iommu::IommuVendor::AmdVi => "AMD-Vi",
            iommu::IommuVendor::None => "?",
        };
        active.push_str(" IOMMU(");
        active.push_str(vendor_str);
        active.push(')');
    } else {
        deferred.push_str(" IOMMU(not-detected)");
    }

    serial_println!("[security] Active:{}", active);
    if !deferred.is_empty() {
        serial_println!("[security] Deferred:{}", deferred);
    }
}

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
        // Notify RCU that this CPU is entering idle.  An idle CPU is
        // inherently quiescent — no RCU read-side critical section can
        // be active.  This lets rcu::synchronize() skip this CPU
        // instead of waiting for a timer-tick-driven quiescent state.
        rcu::mark_idle();

        // Sleep until next interrupt or MWAIT cache-line wake.
        // Uses MWAIT (power-efficient C-state idle) if supported,
        // falls back to HLT otherwise.
        idle::idle_once();

        // Mark this CPU as active before executing any code that might
        // enter an RCU read-side critical section.
        rcu::mark_active();

        tick_counter = tick_counter.wrapping_add(1);

        // If a reschedule IPI woke us (someone enqueued work for CPU 0),
        // yield immediately to pick up the new task.  This avoids the
        // up-to-10ms latency of waiting for the next timer tick.
        if sched::reschedule_pending(0) || idle::resched_pending() {
            idle::clear_resched();
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
/// Guard against re-entrant panics (panic inside panic handler).
///
/// If a panic occurs while we're already in the panic handler (e.g., from
/// formatting code, lock poisoning, or a bug in the diagnostics), we must
/// not recurse infinitely.  This counter tracks nesting depth:
/// - 0: normal code, first panic is fine
/// - 1: inside panic handler, another panic is a double-panic
/// - 2+: triple-panic — halt immediately with minimal output
static PANIC_NESTING: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

#[panic_handler]
#[allow(clippy::unwrap_used)] // Panic handler uses unwrap_or for best-effort diagnostics.
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Double-panic guard: detect re-entrant panics and short-circuit.
    let nesting = PANIC_NESTING.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    if nesting >= 2 {
        // Triple+ panic: absolutely nothing is safe. Halt immediately.
        // SAFETY: cli is valid in ring 0; we're about to halt forever.
        unsafe { cpu::cli(); }
        cpu::halt_loop();
    }
    if nesting == 1 {
        // Double panic: we're panicking inside the panic handler.
        // Print a minimal message and halt — don't attempt full diagnostics
        // since they may trigger yet another panic.
        // SAFETY: cli is valid in ring 0; we're about to halt forever.
        unsafe { cpu::cli(); }
        serial_println!("!!! DOUBLE PANIC (panic inside panic handler) !!!");
        serial_println!("{}", info);
        cpu::halt_loop();
    }

    // Capture volatile state before disabling interrupts.
    let rsp = cpu::read_rsp();
    let cr2 = cpu::read_cr2();
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
    // CR2 holds the last page fault address — useful if the panic
    // was triggered by a page fault handler or UAF in paged memory.
    if cr2 != 0 {
        serial_println!("  CR2 (last page fault addr): {:#018x}", cr2);
    }

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

    // --- Stack backtrace (with symbol resolution) ---
    serial_println!("  Backtrace:");
    let bt = backtrace::capture();
    if bt.count == 0 {
        serial_println!("    <no frames captured (frame pointers may be absent)>");
    } else {
        for i in 0..bt.count {
            let f = &bt.frames[i];
            if ksyms::is_loaded() {
                let sym = ksyms::format_addr(f.return_addr);
                serial_println!("    #{:2}: {} (rbp={:#018x})", i, sym, f.frame_ptr);
            } else {
                serial_println!("    #{:2}: {:#018x} (rbp={:#018x})", i, f.return_addr, f.frame_ptr);
            }
        }
    }

    serial_println!("--- end panic ---");

    cpu::halt_loop();
}
