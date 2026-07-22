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
//!     (self-test: verify timer ticks are observed).  This happens *before*
//!     the ring-3 integration self-test battery so those real Linux-ABI
//!     processes run preemptively with interrupts on — the way userspace
//!     actually runs — instead of monopolizing the BSP with IF=0 (which
//!     starves the watchdog kick; see known-issues.md B-PTHREAD-YIELDBUDGET).
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
mod audio_alsa;
mod audio_alsa_ctl;
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
mod hardlockup;
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
mod tty;
mod udriver;
mod unicode;
mod userns;
mod virtio;
mod vmguest;
mod watchdog;
#[allow(dead_code)]
mod xhci;
mod cnetwork;
mod volume;
mod watchpoint;
mod wchan;
mod workqueue;

// ---------------------------------------------------------------------------
// Kernel entry point
// ---------------------------------------------------------------------------

/// Size of the dedicated kernel boot stack (2 MiB).
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
///
/// **Sized at 2 MiB** because the boot-time self-tests run directly on this
/// stack and one of them — `syscall::linux::self_test()` — is a single
/// monolithic ~1.4 MB function whose unoptimized (`opt-level=0`, no
/// stack-slot coloring) frame is already ~480 KiB and grows by ~1 KiB with
/// every Linux-ABI fidelity batch.  At 512 KiB that frame overflowed the
/// boot stack and silently scribbled the adjacent `.bss` (it flipped
/// `sched::context::FPU_STRATEGY` from FXSAVE→XSAVE, which then `#UD`-faulted
/// the first post-self-test context switch — a baffling crash far from the
/// real cause).  See `known-issues.md`: the proper long-term fix is to split
/// that monolithic function so no single frame is large; until then this
/// stack is generously sized and the [`BOOT_STACK_REDZONE`] canary below
/// turns any future overflow into a clear FATAL rather than silent
/// corruption.
const KERNEL_BOOT_STACK_SIZE: usize = 2 * 1024 * 1024;

/// Bytes at the *bottom* (lowest addresses) of [`KERNEL_BOOT_STACK`] reserved
/// as an overflow-detection redzone.  The usable stack is everything above
/// this; the redzone is filled with [`BOOT_STACK_CANARY`] at boot and
/// verified by [`check_boot_stack_canary`].  Because the unoptimized
/// stack-probe prologue writes a zero to every 4 KiB page it descends
/// through, any frame that reaches the redzone is guaranteed to clobber the
/// canary pattern and be detected before the overflow can corrupt the
/// `.bss` that lies below the stack array.
const BOOT_STACK_REDZONE: usize = 64 * 1024;

/// Magic byte written across the boot-stack redzone.  Chosen to be unlikely
/// to appear as a legitimate stack value and easy to spot in a memory dump.
const BOOT_STACK_CANARY: u8 = 0xC7;

/// Usable boot-stack size in KiB (everything above the redzone), precomputed
/// in a const context so the runtime diagnostic avoids a checked-arithmetic
/// lint.
const BOOT_STACK_USABLE_KIB: usize = (KERNEL_BOOT_STACK_SIZE - BOOT_STACK_REDZONE) / 1024;

/// The dedicated kernel boot stack.  16-byte aligned for the System V ABI.
#[repr(C, align(16))]
struct KernelBootStack([u8; KERNEL_BOOT_STACK_SIZE]);

static mut KERNEL_BOOT_STACK: KernelBootStack = KernelBootStack([0; KERNEL_BOOT_STACK_SIZE]);

/// Return the `[base, top)` virtual-address bounds of [`KERNEL_BOOT_STACK`].
///
/// Used by the backtrace validator to distinguish a genuine frame pointer that
/// lives on the boot stack (which is a static in the kernel image, so its
/// addresses fall in the `0xFFFF_FFFF_8000_0000+` image range) from a bogus
/// "frame pointer" that is actually a pointer into general kernel `.text`/
/// `.data`.  Without this, any higher-half image address passes the frame-ptr
/// check and the walker happily interprets static data as a stack frame chain,
/// producing a misleading garbage backtrace (observed in the iter19 liveness
/// dump: a sampled RBP of `0xffffffff824ca080` — kernel `.data`, not a stack —
/// yielded four bogus frames).
#[must_use]
pub(crate) fn boot_stack_bounds() -> (u64, u64) {
    // SAFETY: We only take the address and size of the static; we never
    // dereference it here.  Address-of on a `static mut` is sound.
    let base = core::ptr::addr_of!(KERNEL_BOOT_STACK) as u64;
    let top = base.saturating_add(KERNEL_BOOT_STACK_SIZE as u64);
    (base, top)
}

/// Fill the boot-stack redzone with the canary pattern.
///
/// Called once, very early in [`kernel_main`], while `RSP` is near the top
/// of the stack so the bottom redzone is guaranteed unused.  Subsequent
/// deep self-tests that overflow toward the bottom will overwrite this
/// pattern, which [`check_boot_stack_canary`] then detects.
///
/// # Safety
///
/// Must be called with `RSP` above the redzone (i.e. before any deep call
/// chain), so that writing the redzone does not clobber a live frame.
unsafe fn init_boot_stack_canary() {
    // SAFETY: `KERNEL_BOOT_STACK` is a dedicated static; we write only its
    // lowest `BOOT_STACK_REDZONE` bytes, which are unused while RSP is near
    // the top.  The range is in-bounds (`BOOT_STACK_REDZONE < SIZE`).
    unsafe {
        let base = core::ptr::addr_of_mut!(KERNEL_BOOT_STACK).cast::<u8>();
        core::ptr::write_bytes(base, BOOT_STACK_CANARY, BOOT_STACK_REDZONE);
    }
}

/// Verify the boot-stack redzone canary is intact; FATAL-halt if not.
///
/// A clobbered canary means a frame descended into the reserved redzone —
/// i.e. the boot stack overflowed (or came within [`BOOT_STACK_REDZONE`]
/// bytes of doing so).  Continuing would risk the exact silent-`.bss`-
/// corruption class this guard exists to catch, so we report and halt.
fn check_boot_stack_canary() {
    // SAFETY: read-only scan of the dedicated static's lowest redzone bytes.
    // `read_volatile` is required so the optimizer cannot prove the region
    // still holds the canary (a stack overflow's writes are invisible to it)
    // and elide the check.  `add`/range iteration avoid checked-arithmetic
    // lints.
    let corrupted = unsafe {
        let base = core::ptr::addr_of!(KERNEL_BOOT_STACK).cast::<u8>();
        (0..BOOT_STACK_REDZONE).any(|i| base.add(i).read_volatile() != BOOT_STACK_CANARY)
    };
    if corrupted {
        serial_println!(
            "FATAL: boot stack overflow detected — redzone canary clobbered. \
             A boot-time self-test frame exceeded the {} KiB usable boot \
             stack. Increase KERNEL_BOOT_STACK_SIZE or split the offending \
             self-test (see known-issues.md).",
            BOOT_STACK_USABLE_KIB,
        );
        cpu::halt_loop();
    }
}

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

    // Lay down the boot-stack overflow canary now, while RSP is near the top
    // of the stack and the bottom redzone is guaranteed unused.  The deep
    // boot-time self-tests run later on this same stack; if any frame grows
    // into the redzone, `check_boot_stack_canary()` (called after the heavy
    // self-tests) will catch it as a clean FATAL instead of letting the
    // overflow silently corrupt the adjacent `.bss`.
    // SAFETY: we are at the top of `kernel_main`'s frame on the boot stack,
    // so RSP is far above the redzone — writing it cannot clobber live data.
    unsafe {
        init_boot_stack_canary();
    }

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

    // Enable slab poisoning immediately, before the first heap allocation.
    //
    // Poisoning fills every freed slot with a poison pattern (UAF/double-free
    // detection) and every freshly-allocated slot with ALLOC_POISON, so the
    // red-zone overflow check can rely on "every byte past the requested size
    // is 0xCD".  That invariant ONLY holds if a slot was alloc-poisoned at the
    // time it was handed out.  If poisoning were enabled later in boot, every
    // allocation made in the pre-enable window would be unpoisoned; freeing
    // such a slot after poisoning came online made check_redzone scan stale
    // (or zeroed) bytes and report spurious "BUFFER OVERFLOW" false positives
    // (see known-issues B-HEAP1).  Enabling here — before any allocation —
    // closes that window entirely.  Poison is only ever toggled OFF for the
    // duration of the heap benchmarks (deferred_bench_task), which free their
    // own allocations within that window, then back ON afterwards.
    mm::heap::enable_poison();

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

    // Install the REAL physical memory map into the memlayout diagnostic table
    // straight from the Limine memmap response (needs the heap, just brought
    // up). This is the single source of truth for /proc/memlayout, total_ram()
    // and the `memlayout` kshell command — without it those would report a
    // fabricated layout. Done here, right after the heap, so the table reflects
    // the machine's actual RAM as early as possible.
    fs::memlayout::populate_from_memmap(boot_info.memory_map);

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

    // Step 9b′: Populate the kernel-parameter store from the Limine command
    // line. This MUST run during boot (previously it was only invoked lazily
    // from the `kernparam` shell command), because boot-time consumers —
    // notably the `net.userspace` cutover switch that decides whether the
    // userspace netstack daemon owns the NIC — call `kernparam::is_set()`
    // long before any shell exists. Without this, the param store stays
    // `None` at boot and every `is_set()` returns false regardless of what
    // the bootloader actually passed, making the switch unusable at runtime.
    // Idempotent: a no-op if the store was already populated.
    fs::kernparam::init_defaults();

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
    // The translation self-test is the deepest single frame that runs on the
    // boot stack (a monolithic function whose unoptimized frame is ~480 KiB
    // and grows with each ABI batch).  Verify it did not breach the redzone;
    // a clobbered canary means the boot stack overflowed and adjacent `.bss`
    // may be corrupt, so halt with a clear diagnostic rather than limp on.
    check_boot_stack_canary();

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

    // Step 15a: Epoll subsystem.
    // Epoll instances hold an interest set (fd -> events + user data) and
    // serve epoll_wait via the shared poll-readiness engine.  This self-test
    // exercises create/dup/close refcounting and ctl add/mod/del semantics.
    if let Err(e) = ipc::epoll::self_test() {
        serial_println!("FATAL: Epoll self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 15a (cont.): Signalfd subsystem.
    // A signalfd object holds an acceptance mask; reads drain masked pending
    // signals from the owning process.  This self-test exercises create with
    // SIGKILL/SIGSTOP mask sanitization, mask get/set, and dup/close
    // refcounting with a shared mask.
    if let Err(e) = ipc::signalfd::self_test() {
        serial_println!("FATAL: Signalfd self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 15a (cont.): Timerfd subsystem.
    // A timerfd object holds an armed-timer state (clock id, next expiry,
    // interval); reads return the lazily-computed expiration count.  This
    // self-test exercises the pure expiry math (one-shot/periodic/overdue),
    // arm/disarm/query, dup/close refcounting with shared armed state, and
    // stale-handle safety.
    if let Err(e) = ipc::timerfd::self_test() {
        serial_println!("FATAL: Timerfd self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Step 15a (cont.): Inotify subsystem.
    // An inotify instance multiplexes a set of path watches over the
    // native fs::notify subsystem, translating Linux IN_* masks and
    // serializing native FsEvents into inotify_event records.  This
    // self-test exercises mask translation, record sizing, add_watch +
    // event emission/read, move-pair cookie pairing, buffer-too-small
    // EINVAL, rm_watch IN_IGNORED, and dup/close refcounting.
    if let Err(e) = ipc::inotify::self_test() {
        serial_println!("FATAL: Inotify self-test failed: {}", e);
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

    // Capture the real boot timestamp now that HPET is running, so the
    // `sysuptime` command reports uptime since actual boot rather than since
    // the first time someone runs the command.  init_defaults() records
    // hpet::elapsed_ns() as the boot instant; doing it here (early, right after
    // the HPET clock starts) is the closest honest approximation of boot time
    // available to a kernel that does not yet persist a wall-clock boot record.
    // It is idempotent, so the lazy init in the kshell handler is a harmless
    // no-op afterwards.
    fs::sysuptime::init_defaults();

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

    // Step 20d-2d: Program the i6300esb hard-lockup watchdog if the boot
    // harness supplied one (opt-in `--hard-lockup-watchdog`). Absent on a
    // normal boot, in which case this is a no-op. Left disarmed here; armed
    // only around the ring-3 container self-tests. See kernel/src/hardlockup.rs.
    hardlockup::init(boot_info.hhdm_offset);

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
        // Never claim a device that holds a filesystem: `init_disk` writes a
        // raw swap area over whatever is there, so probing first is the
        // difference between "use the spare disk" and "destroy the rootfs".
        // The Path-Z glibc rootfs (rootfs.ext4) is attached as a virtio-blk
        // device (typically vdb) and must survive untouched for the /mnt mount
        // below — skip any device that already contains an ext4 superblock.
        // A raw swap image (swap.img, all zeros) has no ext4 magic and is still
        // selected here.
        if fs::ext4::probe(swap_dev) {
            serial_println!(
                "[boot] Skipping {} for swap: holds an ext4 filesystem (reserved for rootfs)",
                swap_dev
            );
            continue;
        }
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
    // File handle self-test — exercises open/read/write/seek/dup/dir-handle and
    // O_EXCL exclusive-create semantics against the VFS root.  It self-guards
    // (skips if "/" is not writable), so it runs on a diskless memfs boot too;
    // gating it on a FAT root would leave the whole handle layer — and O_EXCL —
    // untested on the common in-memory boot path (e.g. the CI boot test).
    if let Err(e) = fs::handle::self_test() {
        serial_println!("WARNING: File handle self-test failed: {:?}", e);
    }
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
    // Mount/unmount self-test (exercises the SYS_FS_MOUNT/SYS_FS_UMOUNT
    // backend dispatch on a scratch tmpfs mount — runs on any root).
    if let Err(e) = fs::vfs::mount_self_test() {
        serial_println!("WARNING: VFS mount/unmount self-test failed: {:?}", e);
    }
    // Stable file-identity self-test (the page-cache key precursor — §23/§36).
    if let Err(e) = fs::vfs::file_identity_self_test() {
        serial_println!("WARNING: VFS file-identity self-test failed: {:?}", e);
    }
    // Read-only shared page-cache self-test (C-lite storage core — §23/§36).
    if let Err(e) = mm::page_cache::self_test() {
        serial_println!("WARNING: page-cache self-test failed: {:?}", e);
    }
    // Register the page-cache shrinker so idle cached pages are reclaimed under
    // memory pressure instead of pinning frames resident without bound (§36).
    mm::page_cache::init();
    // mkfs/format self-test (exercises the SYS_FS_FORMAT backend on a RAM disk).
    if let Err(e) = fs::fat::format_self_test() {
        serial_println!("WARNING: FAT mkfs/format self-test failed: {:?}", e);
    }
    // fsck self-test (exercises the SYS_FS_CHECK backend on a RAM disk).
    if let Err(e) = fs::fat::fsck_self_test() {
        serial_println!("WARNING: FAT fsck self-test failed: {:?}", e);
    }
    // Block-layer discard (TRIM) primitive self-test on a scratch RAM disk.
    if let Err(e) = blkdev::self_test_discard() {
        serial_println!("WARNING: blkdev discard self-test failed: {:?}", e);
    }
    // FAT fstrim (free-space discard) self-test on a scratch RAM disk.
    if let Err(e) = fs::fat::trim_self_test() {
        serial_println!("WARNING: FAT fstrim self-test failed: {:?}", e);
    }

    // Step 21: Enable hardware interrupts — BEFORE the ring-3 self-test battery.
    //
    // Everything above this point is deterministic kernel/subsystem init and
    // in-kernel self-tests that neither spawn ring-3 processes nor perform
    // multi-second, data-proportional work.  Everything BELOW is the ring-3
    // integration battery: dozens of real Linux-ABI processes that fork,
    // CoW-clone, exec, demand-page file-backed mappings, run glibc/dash, and
    // even compile C with gcc/make.  In a debug build (with heap poisoning)
    // those operations are seconds-long and O(n)-over-large-data.
    //
    // Historically `sti()` was deferred until *after* the whole battery, so the
    // battery ran with IF=0.  That is the "long operation under IRQs-disabled"
    // anti-pattern (CLAUDE.md): with IF=0 the BSP takes no timer ticks, so the
    // scheduler cannot preempt, the timer-driven liveness / hung-task watchdogs
    // are blind, and the BSP-only hard-lockup watchdog kick
    // (`sched::timer_tick` → `hardlockup::kick`) is starved — so a slow-but-live
    // boot occasionally crossed the ~9.8 s watchdog / harness-timeout threshold
    // and presented as the intermittent "BSP-dead total-silence hang"
    // (known-issues.md B-PTHREAD-YIELDBUDGET).  Fixing each seconds-long IF=0
    // operation one at a time (SHA-256 auto-versioning, page-fault file reads,
    // heap poisoning …) was band-aid accumulation; the structural fix is to run
    // the battery the way userspace actually runs — with interrupts enabled and
    // preemption live.  This also makes the timer-driven watchdogs able to
    // catch a *genuine* clone/CoW/reap deadlock during the battery instead of
    // going silent.
    //
    // SAFETY: The IDT is fully populated (exceptions, timer vector 32, spurious
    // vector 255), the Local APIC + I/O APIC are initialized and the timer is
    // running (Step 20), and the scheduler is ready (Step 9).  The per-CPU IRQ
    // stack is installed first so hardware IRQs never push their frame onto a
    // near-full kernel task stack (B-DF1 / open-questions Q7, option A).
    console::boot_step(console::BootStatus::Running, "Preemptive scheduling");
    idt::init_irq_stack(0);
    // SAFETY: see the paragraph above — all interrupt infrastructure is ready.
    unsafe {
        cpu::sti();
    }
    serial_println!("[boot] Interrupts enabled — preemptive scheduling active");

    // Verify the APIC timer is actually firing before the battery relies on it
    // for preemption and watchdog kicks.  (Runs here, immediately after enable,
    // rather than at its old post-battery location.)
    if let Err(e) = apic::self_test() {
        serial_println!("FATAL: APIC timer self-test failed: {}", e);
        cpu::halt_loop();
    }
    console::boot_step_update(console::BootStatus::Ok, "Preemptive scheduling");

    // End-to-end dynamically-linked Linux launch test (needs a writable VFS,
    // so it runs here rather than in proc::self_test() which precedes VFS
    // init).  Places a minimal interpreter ("ld.so" stand-in) on the
    // filesystem and verifies the kernel loads + enters it for a
    // dynamically-linked Linux binary.  See proc::spawn for details.
    if let Err(e) = proc::spawn::self_test_linux_dynamic_interp() {
        serial_println!("WARNING: Linux dynamic-interpreter self-test failed: {:?}", e);
    }

    // Atomic RENAME_NOREPLACE test (needs the writable /tmp memfs to exercise
    // the same-mount branch, so it runs here rather than in
    // syscall::linux::self_test() which only sees a read-only root).
    if let Err(e) = syscall::linux::self_test_rename_noreplace() {
        serial_println!("WARNING: rename_noreplace self-test failed: {:?}", e);
    }

    // statfs(2) against the real mounted root (the in-self_test() version can
    // only check error paths since it runs before any filesystem is mounted).
    if let Err(e) = syscall::linux::self_test_statfs_root() {
        serial_println!("WARNING: statfs(/) self-test failed: {:?}", e);
    }

    // sendfile(2) data-transfer test (needs a writable VFS to stage files;
    // the syscall entry can't run in kernel context since it dereferences the
    // per-process Linux fd table, so this drives the sendfile_core copy path
    // against kernel-opened handles directly).
    if let Err(e) = syscall::linux::self_test_sendfile() {
        serial_println!("WARNING: sendfile self-test failed: {:?}", e);
    }

    // copy_file_range(2) data-transfer test — same kernel-context constraint as
    // sendfile, so it drives the copy_file_range_core / overlap path against
    // kernel-opened handles directly.
    if let Err(e) = syscall::linux::self_test_copy_file_range() {
        serial_println!("WARNING: copy_file_range self-test failed: {:?}", e);
    }

    // fallocate(2) PUNCH_HOLE / ZERO_RANGE zeroing test — drives the
    // fallocate_zero_vfs / fallocate_zero_memfd path against /tmp files and a
    // kernel-created memfd (the syscall entry needs a per-process fd table).
    if let Err(e) = syscall::linux::self_test_fallocate_range() {
        serial_println!("WARNING: fallocate range self-test failed: {:?}", e);
    }

    // splice(2) data-transfer test — drives splice_core against kernel-opened
    // file handles and kernel-created pipes (non-blocking) since the syscall
    // entry needs a per-process Linux fd table absent in kernel context.
    if let Err(e) = syscall::linux::self_test_splice() {
        serial_println!("WARNING: splice self-test failed: {:?}", e);
    }

    // tee(2) data-transfer test — drives tee_core against kernel-created pipes
    // (non-blocking); needs no VFS but runs here alongside its splice sibling.
    if let Err(e) = syscall::linux::self_test_tee() {
        serial_println!("WARNING: tee self-test failed: {:?}", e);
    }

    // vmsplice(2) data-transfer test — drives vmsplice_core and the
    // cross-address-space copy primitives against a throwaway process's page
    // table (the boot address space has no user mappings), so it must run after
    // process/paging init.
    if let Err(e) = syscall::linux::self_test_vmsplice() {
        serial_println!("WARNING: vmsplice self-test failed: {:?}", e);
    }

    // File-backed Linux mmap test (needs a writable VFS to stage a file, so
    // it runs here rather than in syscall::linux::self_test() which precedes
    // VFS init).  Exercises the path ld.so uses to map shared objects.
    if let Err(e) = syscall::linux::self_test_file_mmap() {
        serial_println!("WARNING: Linux file-backed mmap self-test failed: {:?}", e);
    }

    // Arm the system-wide liveness watchdog for the boot-time ring-3 phase.
    // From here until BOOT_OK the kernel spawns ring-3 processes that fork,
    // CoW-clone their address spaces, exec, and reap — the exact window in
    // which the intermittent total-hang (known-issues.md B-PTHREAD-
    // YIELDBUDGET) has been observed.  The watchdog dumps the full task
    // table to serial if every CPU goes idle while a thread is lost, giving
    // us the breadcrumb the soft-lockup watchdog structurally cannot.  It is
    // disarmed at BOOT_OK, before the system may legitimately idle at a
    // prompt.
    sched::liveness_arm();

    // Arm the hard-lockup watchdog (i6300esb NMI) for the same window. The
    // liveness watchdog above is timer-driven and therefore blind to a
    // BSP-dead total-silence wedge (spin with IF=0 in interrupt context); the
    // NMI watchdog is not. No-op unless the harness supplied the device.
    hardlockup::arm();

    // Ring-3 end-to-end counterpart of the above: a real Linux-ABI process
    // issues open(2)+mmap(2) itself and exits with a mapped second-frame byte,
    // proving the whole syscall path (fd install, caller_pid, ring-3 read).
    if let Err(e) = proc::spawn::self_test_linux_file_mmap() {
        serial_println!("WARNING: Linux file-backed mmap (ring 3) self-test failed: {:?}", e);
    }

    // Net→userspace migration (Path B, §63/§66). The `net.userspace` boot switch
    // selects NIC ownership at boot (§64):
    //
    //   * switch OFF (default): the in-kernel resident stack owns the NIC. Run the
    //     bounded daemon self-tests here — each briefly claims the NIC, proves a
    //     path, then releases it back so the kernel stack's RX resumes.
    //   * switch ON (Phase-5 cutover): the PERSISTENT userspace daemon owns the
    //     NIC for the system's lifetime and serves all AF_INET socket traffic
    //     (increment 5.5/5.6). Its spawn is DEFERRED to just before BOOT_OK
    //     (alongside the container health monitor) — a lifetime service that
    //     runs continuously would perturb the timing-sensitive timeout
    //     self-tests below (channel/futex/eventfd recv-with-timeout) and the
    //     hrtimer self-test's pending-count assertions. Kernel POST must run in
    //     a quiet system; services start only once self-verification is done.
    //     The bounded self-tests are also skipped under the switch — they would
    //     contend for the exclusive raw-NIC claim the daemon will hold (§64).
    if !crate::net::netstack_client::userspace_enabled() {
        // Phase 2: spawn the real `services/netstack` daemon (ring 3), which
        // claims the NIC via the capability-gated SYS_NET_RAW_* syscalls and
        // proves the raw-frame TX/RX path end-to-end with an ARP round-trip.
        // Skips gracefully when there's no network.
        if let Err(e) = proc::spawn::self_test_userspace_netstack() {
            serial_println!("WARNING: userspace netstack daemon (ring 3) self-test failed: {:?}", e);
        }

        // Phase 4: forward a DNS resolve from the kernel to the userspace
        // `netstack` daemon over the Service Registry (`net.stack`), proving the
        // socket-syscall → IPC path end-to-end. Bounded self-test (the daemon
        // owns the NIC only briefly); skips gracefully with no network.
        if let Err(e) = proc::spawn::self_test_netstack_dns_ipc() {
            serial_println!("WARNING: netstack DNS-over-IPC (ring 3) self-test failed: {:?}", e);
        }
    }

    // Ring-3 end-to-end test of the Linux brk(2) heap: a real Linux-ABI
    // process queries its program break, grows the heap by 32 KiB, writes a
    // sentinel into the second frame, reads it back, and exits with that byte
    // — proving set_brk_region at load, sys_brk's grow path, and demand-paging
    // of the new heap frames.
    if let Err(e) = proc::spawn::self_test_linux_brk() {
        serial_println!("WARNING: Linux brk(2) heap (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 end-to-end test of the SA_RESTART transparent-restart path — the
    // capstone for the slow-object signal-interruptibility work.  A real
    // Linux-ABI process blocks in read() on its own empty pipe with an
    // SA_RESTART SIGUSR1 handler installed; the kernel posts SIGUSR1, the
    // interrupted read returns ERESTARTSYS, the handler writes a byte into the
    // pipe, and the read is transparently restarted to return it.  Proves the
    // park is interruptible AND that SA_RESTART resumes the syscall.
    if let Err(e) = proc::spawn::self_test_linux_sa_restart() {
        serial_println!("WARNING: Linux SA_RESTART (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that a blocking signalfd read is interrupted by a signal NOT
    // in the fd's acceptance mask (the signalfd analogue of the slow-object
    // interruptibility fixes): a Linux-ABI process blocks in read() on a
    // signalfd watching only SIGUSR2 with a non-SA_RESTART SIGUSR1 handler
    // installed; the kernel posts SIGUSR1, the read wakes and returns -EINTR.
    // Before the fix the read parked forever for the out-of-mask signal.
    if let Err(e) = proc::spawn::self_test_linux_signalfd_interrupt() {
        serial_println!("WARNING: Linux signalfd-read interruptibility (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 eventfd-read signal-interruptibility test: a child blocks in
    // read() on an eventfd whose counter is 0, with a non-SA_RESTART SIGUSR1
    // handler installed; the kernel posts SIGUSR1, the read wakes and returns
    // -EINTR.  Before the fix the read parked forever (single-slot waiter only
    // wakeable by a writer), the same hang-bug class as pipe/signalfd.
    if let Err(e) = proc::spawn::self_test_linux_eventfd_interrupt() {
        serial_println!("WARNING: Linux eventfd-read interruptibility (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 timerfd-read signal-interruptibility test: a child blocks in
    // read() on a disarmed timerfd (blocks indefinitely), with a non-SA_RESTART
    // SIGUSR1 handler installed; the kernel posts SIGUSR1, the read wakes and
    // returns -EINTR.  Before the fix the read parked forever (single-slot
    // reader waiter only wakeable by settime/the expiry hrtimer), the same
    // hang-bug class as pipe/signalfd/eventfd.
    if let Err(e) = proc::spawn::self_test_linux_timerfd_interrupt() {
        serial_println!("WARNING: Linux timerfd-read interruptibility (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 inotify-read signal-interruptibility test: a child blocks in
    // read() on an inotify fd with no events queued (blocks indefinitely), with
    // a non-SA_RESTART SIGUSR1 handler installed; the kernel posts SIGUSR1, the
    // read wakes and returns -EINTR.  Before the fix the read registered only a
    // notify-waiter and parked uninterruptibly, the same hang-bug class as
    // pipe/signalfd/eventfd/timerfd.
    if let Err(e) = proc::spawn::self_test_linux_inotify_interrupt() {
        serial_println!("WARNING: Linux inotify-read interruptibility (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that a blocking poll() is signal-interruptible and surfaces
    // -EINTR (the "always-EINTR, never restarted" branch of the SA_RESTART
    // taxonomy).  Before the fix, poll_core busy-polled in sleep_ms slices and
    // never checked for a pending signal, so the thread parked forever.
    if let Err(e) = proc::spawn::self_test_linux_poll_interrupt() {
        serial_println!("WARNING: Linux poll() interruptibility (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that poll(NULL, 0, -1) (no fds, infinite timeout) BLOCKS
    // until a signal and returns -EINTR, rather than returning 0 immediately
    // (the empty-set infinite-wait quick-path bug fixed alongside the
    // poll/select/epoll signal-interruptibility work).
    if let Err(e) = proc::spawn::self_test_linux_poll_empty_infinite() {
        serial_println!("WARNING: Linux poll(NULL,0,-1) empty-set infinite-wait (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that the SysV stack builder's argv *pointers* (not just the
    // scalar argc) are valid in the mapped user stack: a real Linux-ABI
    // process dereferences argv[0] and exits with its first byte.
    if let Err(e) = proc::spawn::self_test_linux_argv0_deref() {
        serial_println!("WARNING: Linux argv[0] deref (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that the SysV stack builder places the envp array at the
    // correct *variable* offset (rsp + 16 + argc*8): a real Linux-ABI process
    // computes envp[0] from argc, dereferences it, and exits with its first
    // byte.  Distinct from the argv[0] test (fixed offset) — catches a
    // misplaced envp array that getenv()-dependent toolchains would fault on.
    if let Err(e) = proc::spawn::self_test_linux_envp0_deref() {
        serial_println!("WARNING: Linux envp[0] deref (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 end-to-end test of a native fastpy-compiled binary (initiative
    // F's "first real component" milestone).  Spawns a real fastpy AOT
    // executable linked against our posix libc and runs it to exit(0),
    // proving the crt sets up main-thread ELF TLS (SYS_SET_FS_BASE) so the
    // fastpy runtime's `__thread` accesses don't fault.  Bounded yield loop,
    // so it can never hang the boot.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_tls() {
        serial_println!("WARNING: fastpy-on-SlateOS TLS (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 end-to-end test of fastpy pure-mode FILE I/O on-target: a native
    // fastpy binary opens/writes/closes then reopens/reads a file on the /tmp
    // memfs and exits with the byte count read back, proving the full path
    // fastpy open/write/read/close -> C stdio -> SYS_FS_* -> kernel VFS. The
    // process is granted a File capability so sys_fs_open's cap check passes.
    // Bounded yield loop; can never hang the boot.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_fileio() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS pure-mode file I/O (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 end-to-end test of the FIRST SHIPPING fastpy SlateOS utility:
    // `fastpy-cat`, a real `cat`(1) that reads its argv[1] file and echoes it
    // to stdout.  Ties together argv delivery + pure-mode file I/O + stdout
    // (SYS_CONSOLE_WRITE) in one native Python-via-fastpy binary.  Bounded
    // yield loop; can never hang the boot.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_cat() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `cat` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-grep` utility: a fixed-string grep(1) that
    // reads argv[2], prints lines containing argv[1] via the native
    // contains_sub matcher, and exits 0/1 per grep semantics.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_grep() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `grep` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-wc` utility: reads argv[1], counts
    // lines/words/bytes in a str-typed helper, and prints "<lines> <words>
    // <bytes>". First fastpy tool that computes over the whole file.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_wc() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `wc` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-head` utility: `head <n> <file>` parses an
    // integer from argv[1] and prints the first n lines of argv[2] with
    // early-stop line iteration.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_head() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `head` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-uniq` utility: `uniq <file>` drops adjacent
    // duplicate lines via line-to-line string comparison (line == prev).
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_uniq() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `uniq` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-tail` utility: `tail <n> <file>` prints the
    // last n lines via a two-pass scan (count total lines, then re-scan and
    // print only those at index >= total-n) — the first two-pass fastpy tool.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_tail() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `tail` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-sort` utility: `sort <file>` collects lines
    // into an in-memory list, sorts them ascending (native list + `<` string
    // ordering), and prints them — the first fastpy list-of-strings tool.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_sort() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `sort` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-freq` utility: `freq <file>` counts each
    // distinct line's occurrences in a dict[str,int] (native dict construct/
    // membership/get/set + key iteration) — the first fastpy dict tool.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_freq() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `freq` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the `fastpy-ls` utility: `ls <dir>` enumerates a directory
    // via os.listdir (SYS_FS_LIST_DIR) — the first fastpy tool to list a
    // directory rather than read a file's contents.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_ls() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `ls` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of `fastpy-rm`: deletes a staged file via os.remove
    // (SYS_FS_DELETE) — the first fastpy tool to delete a filesystem entry
    // rather than read a file's contents or enumerate a directory.  The
    // primitive that unblocks a package-manager `gc` subcommand.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_rm() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `rm` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of `fastpy-mv`: renames a staged file via os.rename
    // (SYS_FS_RENAME) — the first fastpy tool to rename a filesystem entry.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_mv() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `mv` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of `fastpy-mkdir`: creates a directory via os.mkdir
    // (SYS_FS_MKDIR) — the first fastpy tool to create a directory.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_mkdir() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `mkdir` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of `fastpy-rmdir`: removes a directory via os.rmdir
    // (SYS_FS_RMDIR) — completes the mkdir/rmdir/rename trilogy.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_rmdir() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `rmdir` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy `size` utility: the first fastpy tool to read a
    // file's *metadata* (os.path.getsize → SYS_FS_STAT, gated on Rights::METADATA)
    // rather than its contents/listing. It exits with the byte size as its exit
    // code, so the size flows through and the exact byte count is asserted.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_size() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `size` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy `ftype` utility: reads st_mode's file-type bits
    // (os.path.isfile/isdir → SYS_FS_STAT) rather than st_size, and doubles as
    // an on-target regression test that chained os.path.X(...) lowers natively
    // in assignment form (the codegen fix that shipped with fastpy-size).
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_ftype() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `ftype` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the second shipping fastpy utility: `fastpy-sysinfo` reads
    // the kernel's procfs (/proc/version, /proc/uptime, /proc/meminfo) — files
    // generated on the fly with no fixed size — and prints a report. Proves
    // fastpy pure-mode reads stream generated kernel content correctly.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_sysinfo() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `sysinfo` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the third shipping fastpy utility: `fastpy-store`, the
    // package manager's core primitive — a content-addressed store. It hashes
    // argv[1] with a 32-bit FNV-1a (kept inside i64 — no bigint), writes the
    // bytes to /tmp/store-<digest>.blob, and verifies the read-back. Exit 0
    // proves the store round-trip end-to-end.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_store() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `store` utility (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 CLI-lifecycle test of the fastpy-built package manager front-end:
    // the registry layer atop the content-addressed store. Across six spawns it
    // drives install x2 / query / remove / query-gone over the persistent
    // /tmp/pkgdb.txt registry, then the kernel reads the registry back and
    // asserts the final state (installed record present, removed record gone).
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` manager (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 generations/rollback test of the fastpy package manager: commit
    // immutable registry snapshots (install foo/commit, install bar/commit) and
    // atomically roll back to the previous generation, then assert the live
    // registry was reverted to gen 1's snapshot (foo present, bar gone).
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_gen() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` generations/rollback (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 content-integrity test of the fastpy package manager: install a
    // package, `verify` its store blob hashes to the recorded digest, then
    // tamper with the blob and assert `verify` detects the corruption.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_verify() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` content-integrity verify (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy package manager's `gc` subcommand: seed a
    // registry referencing one blob, stage a referenced blob + an orphan,
    // run `pkg gc`, and assert (via the VFS) the referenced blob survives and
    // the orphan is reclaimed — os.listdir + os.remove combined into the
    // content-addressed store's garbage collector (unblocked by native
    // os.remove).
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_gc() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` store gc (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy package manager's `search` subcommand: seed a
    // registry, run substring queries, and assert grep-style exit codes
    // (0 = matched, 1 = no match) distinguish match from no-match.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_search() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` search (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy package manager's `upgrade` subcommand: reject
    // upgrading an uninstalled package (exit 1), then install and upgrade in
    // place, asserting the record's digest/deps and content blob were replaced.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_upgrade() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` upgrade (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 test of the fastpy package manager's transactional `batch`
    // subcommand: a satisfiable reverse-order manifest installs all packages
    // (exit 0), and an unsatisfiable one is rejected leaving the registry
    // untouched (exit 1) — the all-or-nothing guarantee.
    if let Err(e) = proc::spawn::self_test_fastpy_slateos_pkg_batch() {
        serial_println!(
            "WARNING: fastpy-on-SlateOS `pkg` batch (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 end-to-end test of the fork()+wait4() reap cycle — the core
    // process-lifecycle primitive every toolchain (make→gcc→cc1/as/ld) needs.
    // The launcher reaps with a non-blocking WNOHANG retry loop and the
    // harness drives the scheduler with a bounded yield loop, so this can
    // never hang the boot: worst case is a clean failed assertion.
    if let Err(e) = proc::spawn::self_test_linux_fork_wait() {
        serial_println!("WARNING: Linux fork()+wait4() (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 end-to-end test of the full fork → child execve → parent wait4
    // subprocess cycle (the make/gcc pattern): the child execs a staged target
    // and the parent reaps the *target's* exit status.  Same bounded, hang-safe
    // harness as the fork+wait4 test above.
    if let Err(e) = proc::spawn::self_test_linux_fork_execve_wait() {
        serial_println!(
            "WARNING: Linux fork()+execve()+wait4() (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 end-to-end test of the canonical shell-pipeline primitive
    // (cmd1 | cmd2): pipe2 + fork + dup2 + execve + blocking read.  Proves
    // fd-table inheritance across fork, dup2 onto stdout, execve preserving
    // the fd, and the pipe IPC path all compose end to end.  Same bounded,
    // hang-safe harness.
    if let Err(e) = proc::spawn::self_test_linux_pipe_fork_dup2_exec() {
        serial_println!(
            "WARNING: Linux pipe2()+fork()+dup2()+execve()+read() (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 round-trip test for the symlink()/readlink() syscalls, which were
    // stale EROFS/EINVAL stubs until wired to the VFS.  Creates a symlink and
    // reads its target back from ring 3, then confirms it kernel-side.
    if let Err(e) = proc::spawn::self_test_linux_symlink_readlink() {
        serial_println!(
            "WARNING: Linux symlink()+readlink() (ring 3) self-test failed: {:?}",
            e
        );
    }

    // Ring-3 round-trip test for the link()/linkat() hard-link syscalls, which
    // were stale EROFS stubs until wired to Vfs::link.
    if let Err(e) = proc::spawn::self_test_linux_link() {
        serial_println!("WARNING: Linux link() (ring 3) self-test failed: {:?}", e);
    }

    // Kernel-context test that link(2)/linkat honour the no-follow contract:
    // a symlink oldpath is hard-linked as the symlink itself (no-follow) vs the
    // target file (AT_SYMLINK_FOLLOW).  Runs on ext4 /mnt (memfs lacks links).
    if let Err(e) = proc::spawn::self_test_ext4_link_no_follow() {
        serial_println!("WARNING: link no-follow (ext4) self-test failed: {:?}", e);
    }

    // Ring-3 test that utimensat() applies file timestamps (the utimensat/
    // utimes/utime family now performs real Vfs::set_times for ring-3 callers
    // instead of returning EROFS).  Runs on the memfs root and verifies the
    // kernel-side metadata matches the requested atime/mtime exactly.
    if let Err(e) = proc::spawn::self_test_linux_utimensat() {
        serial_println!("WARNING: Linux utimensat() (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that chmod()/chown() mutate file metadata (the chmod/chown
    // family now routes to Vfs::set_permissions/set_owner for ring-3 callers
    // instead of returning EROFS).  Verifies kernel-side mode + owner.
    if let Err(e) = proc::spawn::self_test_linux_chmod_chown() {
        serial_println!("WARNING: Linux chmod()/chown() (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that truncate()/ftruncate() resize files (both now route to
    // Vfs::truncate for ring-3 callers instead of returning EROFS).  Shrinks
    // via the path syscall, grows via a writable fd, and verifies the
    // kernel-side final length + zero-fill.
    if let Err(e) = proc::spawn::self_test_linux_truncate() {
        serial_println!("WARNING: Linux truncate()/ftruncate() (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 test that fchmodat2(AT_EMPTY_PATH) chmods the file an O_RDWR fd
    // points to (the genuinely new path-resolution branch in the fchmodat2
    // wiring: dirfd -> handle_path -> Vfs::set_permissions).
    if let Err(e) = proc::spawn::self_test_linux_fchmodat2() {
        serial_println!("WARNING: Linux fchmodat2(AT_EMPTY_PATH) (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 regression test for fallocate(mode=0) growing a file via the
    // posix_fallocate path (fd -> handle_path -> Vfs::file_size/Vfs::truncate).
    if let Err(e) = proc::spawn::self_test_linux_fallocate() {
        serial_println!("WARNING: Linux fallocate(mode=0 grow) (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 regression test for the virtio-gpu GETPARAM render ioctl on
    // /dev/dri/renderD128 (honest no-3D reporting; Q18/§59). Skips cleanly when
    // no DRM device is bound.
    if let Err(e) = proc::spawn::self_test_linux_virtgpu_getparam() {
        serial_println!("WARNING: virtio-gpu GETPARAM (ring 3) self-test failed: {:?}", e);
    }

    // Ring-3 regression test that IA32_FS_BASE (the glibc %fs/TLS pointer) is
    // saved/restored per task across context switches.  Two concurrent Linux
    // procs install distinct FS bases and assert they survive cooperative
    // yields; without per-task FS-base save/restore they'd clobber each
    // other's TLS (fatal for any multi-process glibc workload, e.g. a real
    // toolchain).  Same bounded, hang-safe harness.
    if let Err(e) = proc::spawn::self_test_linux_fs_tls_switch() {
        serial_println!(
            "WARNING: Linux %fs/TLS-base context-switch self-test failed: {:?}",
            e
        );
    }

    // Sibling regression test for the userspace %gs base (the active
    // IA32_GS_BASE under Slate's entry-stub convention, installed by
    // arch_prctl(ARCH_SET_GS)): two concurrent Linux procs install distinct
    // %gs bases and assert they survive cooperative yields.  Without per-task
    // %gs-base save/restore they'd clobber each other's GS base.
    if let Err(e) = proc::spawn::self_test_linux_gs_tls_switch() {
        serial_println!(
            "WARNING: Linux %gs-base context-switch self-test failed: {:?}",
            e
        );
    }

    // Ring-3 end-to-end test of execveat(2) in both forms: a real Linux-ABI
    // launcher execs a target by path (AT_FDCWD) and by open-fd
    // (AT_EMPTY_PATH / fexecve), proving execveat replaces the image and
    // transfers control to the target (which exits with a sentinel).
    if let Err(e) = proc::spawn::self_test_linux_execveat() {
        serial_println!("WARNING: Linux execveat(2) (ring 3) self-test failed: {:?}", e);
    }

    // Path Z: run a REAL, prebuilt, dynamically-linked glibc binary
    // (/bin/hello, PT_INTERP=ld-linux-x86-64.so.2) end-to-end.  Self-stages the
    // glibc tree from the read-only ext4 rootfs at /mnt into the active root and
    // asserts the child exits 42 through the full ld.so + libc startup.  No-ops
    // when rootfs.ext4 is absent (the image is git-ignored).  Must run after the
    // /mnt ext4 probe above.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc() {
        serial_println!(
            "WARNING: Path-Z real glibc dynamic-execution self-test failed: {:?}",
            e
        );
    }

    // Path Z, part 2: run a REAL glibc binary that produces output
    // (/bin/stdio → printf), redirecting its fd 1 to a capture file and
    // asserting the exact bytes glibc's full-buffered stdio flushes via
    // write(2).  Proves the real-glibc output path, not just exit().  No-ops
    // when rootfs.ext4 is absent.  Must run after self_test_linux_real_glibc
    // (which stages the glibc tree) and the /mnt ext4 probe.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_stdio() {
        serial_println!(
            "WARNING: Path-Z real glibc stdio-output self-test failed: {:?}",
            e
        );
    }

    // Path Z, part 3: run a REAL glibc binary (/bin/full) that exercises argv,
    // getenv, a stdin fgets(), and 64 rounds of mixed brk/mmap malloc-free.
    // fd 0 is redirected from a pre-populated input file and fd 1 to a capture
    // file; we assert the exact deterministic output line and exit code (11).
    // Proves the real-glibc argv/env/input/heap paths.  No-ops when
    // rootfs.ext4 is absent.  Must run after the glibc tree is staged.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_full() {
        serial_println!(
            "WARNING: Path-Z real glibc argv/env/stdin/heap self-test failed: {:?}",
            e
        );
    }

    // Path Z, part 4: run a REAL glibc binary (/bin/pthread) that creates 4
    // worker threads via pthread_create, hammers a shared mutex 40000 times,
    // and pthread_joins them — exercising clone(CLONE_VM|CLONE_THREAD|SETTLS),
    // per-thread TLS, the futex wait/wake path, and join's child-tid futex.
    // fd 1 is captured and the exact deterministic output + exit code (13) are
    // asserted.  This is the multithreading integration coverage thread_clone.rs
    // cannot self-test.  No-ops when rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_pthread() {
        serial_println!(
            "WARNING: Path-Z real glibc pthread self-test failed: {:?}",
            e
        );
    }

    // Path Z, part 5: run a REAL glibc binary (/bin/signal) that installs an
    // SA_SIGINFO handler for SIGUSR1, raise()s it, and (in the handler) reads
    // the siginfo before returning via glibc's __restore_rt -> rt_sigreturn.
    // Exercises the kernel's byte-exact Linux rt_sigframe delivery
    // (build_linux_rt_frame) and the rt_sigreturn restore path.  fd 1 is
    // captured; the exact deterministic output + exit code (17) are asserted.
    // This is the real-glibc signal integration coverage the in-kernel
    // signal-shim self-tests cannot provide.  No-ops when rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_signal() {
        serial_println!(
            "WARNING: Path-Z real glibc signal self-test failed: {:?}",
            e
        );
    }

    // Path Z: synchronous-fault signal delivery. Proves an AbiMode::Linux
    // process that installs a SIGSEGV handler, dereferences a bad pointer
    // (#PF), reads a faithful siginfo (si_addr = bad address, si_code =
    // SEGV_MAPERR) and recovers via siglongjmp — the kernel delivers a
    // byte-exact rt_sigframe straight from the page-fault ISR. No-op when
    // rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_fault() {
        serial_println!(
            "WARNING: Path-Z real glibc fault-signal self-test failed: {:?}",
            e
        );
    }

    // Path Z: SI_QUEUE payload delivery. Proves an AbiMode::Linux process that
    // sigqueue()s itself with a sival_int receives si_code = SI_QUEUE, the
    // user-supplied si_value (stamped at the correct ABI offset), and a
    // faithful si_pid (the real caller). No-op when rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_sigqueue() {
        serial_println!(
            "WARNING: Path-Z real glibc SI_QUEUE-payload self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real glibc program fork()s, execl()s the silent /bin/hello
    // child, and waitpid()s it — proving glibc's fork (CoW)/exec (child
    // re-runs ld.so)/wait wrappers work end-to-end, the foundation for a
    // shell. No-op when rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_forkexec() {
        serial_println!(
            "WARNING: Path-Z real glibc fork/exec/wait self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real glibc program builds a `cmd1 | cmd2` pipeline —
    // pipe() + fork() + the child dup2()s the write end onto fd 1 and
    // execl()s /bin/emit, the parent read()s the pipe to EOF and
    // waitpid()s. Proves pipe-fd inheritance across fork, dup2, and an
    // open fd surviving execve. No-op when rootfs.ext4 is absent.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_pipe() {
        serial_println!(
            "WARNING: Path-Z real glibc pipe self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real glibc program performs its OWN `cmd > file` output
    // redirection — open(O_WRONLY|O_CREAT|O_TRUNC) + dup2(fd, 1) + printf.
    // Proves dup2 of a self-open()ed File handle onto stdout (vs Part 7's
    // dup2 onto a pipe) and the displaced-console close. No-op without
    // rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_redir() {
        serial_println!(
            "WARNING: Path-Z real glibc redir self-test failed: {:?}",
            e
        );
    }

    // Path Z: the mirror image — a real glibc program performs its OWN
    // `cmd < file` input redirection: open(O_RDONLY) + dup2(fd, 0) + fgets.
    // Proves dup2 of a self-open()ed read-only File handle onto stdin and
    // glibc's buffered input path reading from a real file. No-op without
    // rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_redirin() {
        serial_println!(
            "WARNING: Path-Z real glibc redirin self-test failed: {:?}",
            e
        );
    }

    // Path Z: the culmination — a real prebuilt POSIX shell (dash) runs and
    // performs its OWN `echo > file` redirection. Proves ld.so loads dash,
    // dash parses the command + `>` redirection, and drives open()/dup2()
    // itself. No-op without rootfs.ext4 / /bin/dash.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_redir() {
        serial_println!(
            "WARNING: Path-Z real dash shell redir self-test failed: {:?}",
            e
        );
    }

    // Path Z: the full shell-orchestration proof — dash forks + exec's an
    // EXTERNAL real-glibc binary (/bin/emit) with output redirection. Proves
    // dash parses `cmd > file`, fork()s, the child redirects fd 1 + execve()s
    // the external binary, and the parent wait4()s. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_exec() {
        serial_println!(
            "WARNING: Path-Z real dash shell fork+exec self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell builds a full PIPELINE — `cmd1 | cmd2 > file`.
    // Proves dash pipe()s, double-forks, dup2s both pipe ends, exec's two
    // external glibc binaries, and wait4s both; the downstream counts the
    // piped bytes. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_pipe() {
        serial_println!(
            "WARNING: Path-Z real dash shell pipeline self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell runs a `for` LOOP that fork+exec's an external
    // glibc binary every iteration (`for i in a b c; do /bin/emit; done > file`).
    // Three back-to-back CoW fork→exec→reap cycles in one parent — the exact
    // path that surfaced the F18 CoW double-free, so this is both a control-flow
    // capability proof and a regression guard. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_loop() {
        serial_println!(
            "WARNING: Path-Z real dash shell loop self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell reads a multi-command SCRIPT from stdin (no -c,
    // fd 0 redirected from a file), driving its main read-eval loop — two
    // sequential external execs + a builtin, EOF→exit 0. No-op without
    // rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_script_stdin() {
        serial_println!(
            "WARNING: Path-Z real dash shell script-from-stdin self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell performs pathname expansion (globbing) —
    // `echo /globdir/* > file` — driving its own opendir/getdents64 directory
    // read, the first end-to-end exercise of glibc readdir. No-op without
    // rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_glob() {
        serial_println!(
            "WARNING: Path-Z real dash shell glob self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell performs command substitution —
    // `echo [$(/bin/emit)] > file` — where dash itself reads the substituted
    // command's stdout from a pipe and splices it into the command line. No-op
    // without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_cmdsub() {
        serial_println!(
            "WARNING: Path-Z real dash shell cmdsub self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell evaluates a conditional compound command —
    // `x=hello; if [ "$x" = hello ]; then echo EQ; else echo NE; fi > file`
    // — exercising variable assignment, parameter expansion, the `[`/`test`
    // builtin, and if/then/else/fi control flow. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_cond() {
        serial_println!(
            "WARNING: Path-Z real dash shell conditional self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell evaluates an arithmetic expansion —
    // `x=3; y=4; echo $((x * y + 2)) > file` — exercising dash's arithmetic
    // evaluator (variable lookup in the arithmetic context, `*` before `+`).
    // No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_arith() {
        serial_println!(
            "WARNING: Path-Z real dash shell arithmetic self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell processes a here-document — `read a <<EOF /
    // HELLO / EOF / echo "$a" > file` — feeding the heredoc body onto fd 0
    // via the kernel's pipe machinery, then the `read` builtin consumes it.
    // No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_heredoc() {
        serial_println!(
            "WARNING: Path-Z real dash shell heredoc self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell runs a background job and reaps it —
    // `/bin/emit > file & wait` — exercising the async-child + waitpid path
    // (the `wait` builtin) driven from the shell. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_bgjob() {
        serial_println!(
            "WARNING: Path-Z real dash shell background-job self-test failed: {:?}",
            e
        );
    }

    // Path Z: a real dash shell runs a two-stage pipeline connecting an
    // external program to a shell-internal reader — `/bin/emit | while read
    // l; do echo "<$l>"; done > file` — exercising concurrent pipeline
    // stages joined by a kernel pipe. No-op without rootfs.ext4.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_pipeline() {
        serial_println!(
            "WARNING: Path-Z real dash shell pipeline self-test failed: {:?}",
            e
        );
    }

    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_cwd() {
        serial_println!(
            "WARNING: Path-Z real dash shell cwd self-test failed: {:?}",
            e
        );
    }

    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_relpath() {
        serial_println!(
            "WARNING: Path-Z real dash shell relpath self-test failed: {:?}",
            e
        );
    }

    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_statpath() {
        serial_println!(
            "WARNING: Path-Z real dash shell statpath self-test failed: {:?}",
            e
        );
    }

    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_dirstat() {
        serial_println!(
            "WARNING: Path-Z real dash shell dirstat self-test failed: {:?}",
            e
        );
    }

    if let Err(e) = proc::spawn::self_test_linux_real_glibc_shell_append() {
        serial_println!(
            "WARNING: Path-Z real dash shell append self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 34: run an unmodified prebuilt GNU make that parses a
    // Makefile and dispatches a recipe via /bin/sh (which fork/execs the
    // external /bin/emit) — the first rung of the GCC/Make toolchain.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_make() {
        serial_println!("WARNING: Path-Z real GNU make self-test failed: {:?}", e);
    }

    // Path Z Part 35: run an unmodified prebuilt C compiler (TinyCC) that
    // compiles a C source into a native ELF, then run that freshly-compiled
    // program — both in ring 3.  The next rung after make: the OS hosts a
    // real toolchain, not merely runs prebuilt binaries.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc() {
        serial_println!("WARNING: Path-Z real C compiler (tcc) self-test failed: {:?}", e);
    }

    // Path Z Part 36: the *hosted* compile rung — tcc links a C program against
    // real glibc (crt startup -> __libc_start_main -> main, calling puts), then
    // that freshly-built *dynamic* binary runs through ld.so in ring 3.  This is
    // the realistic compile mode (vs Part 35's freestanding -nostdlib -static).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_hosted() {
        serial_println!("WARNING: Path-Z hosted C compiler (tcc) self-test failed: {:?}", e);
    }

    // Path Z Part 37: the hosted compile rung exercising more of the glibc ABI
    // through a freshly-tcc-built dynamic binary — a malloc/free heap round-trip
    // plus printf's variadic format machinery (%s pointer arg, %d int format).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_hosted_stdio() {
        serial_println!(
            "WARNING: Path-Z hosted C compiler (tcc, printf/malloc) self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 38: separate compilation — `tcc -c` emits two relocatable ELF
    // objects (a defines slate_add, b's main calls it across the TU boundary),
    // then `tcc -o prog a.o b.o` links both + crt + glibc into one dynamic exe,
    // resolving the cross-TU reference at link time, and the binary runs in ring
    // 3. Exercises object emission + tcc-as-linker over multiple inputs.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_separate() {
        serial_println!(
            "WARNING: Path-Z separate-compilation C compiler (tcc) self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 39: the §4.4 toolchain capstone — real GNU make drives tcc to
    // build a multi-file C program. make parses a 3-target Makefile, fork/exec's
    // tcc to compile two TUs to objects and link them into a dynamic ELF, which
    // then runs in ring 3. Composes Part 34 (make) with Part 38 (separate
    // compilation) into the realistic "build a C project" flow.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_make_cc() {
        serial_println!(
            "WARNING: Path-Z make-drives-tcc build self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 40: a multi-TU C project that #includes its own project header
    // via `#include "..."` (the project-relative quote form, distinct from the
    // still-blocked <system_header.h> glibc-tree form). tcc's preprocessor must
    // resolve the quote-include to a sibling header from two TUs, expand a macro
    // it defines, and honor a prototype it declares across the TU boundary; the
    // linked dynamic binary then runs in ring 3 and prints SLATE-HDR-42. Fills
    // the header-include gap left by Parts 36-39 (which used bare `extern`s).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_project_header() {
        serial_println!(
            "WARNING: Path-Z project-header C build self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 41: the first rung to exercise the C runtime's constructor/
    // destructor machinery. tcc compiles a program with __attribute__((constructor))
    // and __attribute__((destructor)) into .init_array/.fini_array; glibc's csu
    // init runs the ctor before main, and _dl_fini runs the dtor at exit. The
    // three markers use raw write(2) (unbuffered) so the captured file's byte
    // order is the exact temporal order: CTOR then MAIN then DTOR.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_ctor_dtor() {
        serial_println!(
            "WARNING: Path-Z ctor/dtor C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 42: ELF thread-local storage (__thread) in a tcc-built dynamic
    // glibc binary. tcc emits a .tdata/PT_TLS segment + local-exec TLS relocs;
    // glibc's __libc_setup_tls copies the init image into the main thread's TLS
    // block and %fs-relative access reads/writes it. First compiled-program TLS
    // test; also end-to-end coverage of the per-task %fs-base save/restore (F13/F14).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_tls() {
        serial_println!(
            "WARNING: Path-Z TLS (__thread) C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 43: POSIX signal delivery in a tcc-built dynamic glibc binary.
    // The program installs a SIGUSR1 (10) handler via signal(), raise(10)s to
    // itself, and the kernel delivers the signal synchronously on the syscall
    // return path (tgkill self-signal) so the handler runs between the "A" and
    // "B" markers. Exercises glibc's sigaction wrapper, the kernel's asynchronous
    // signal-frame setup, and rt_sigreturn — end-to-end from compiled source.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_signal() {
        serial_println!(
            "WARNING: Path-Z signal C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 44: non-local control flow (setjmp/longjmp) in a tcc-built
    // dynamic glibc binary. setjmp snapshots the callee-saved registers + rsp/
    // rip into a jmp_buf; a longjmp from a deeper frame restores it so control
    // resumes at the setjmp site (setjmp "returns" a second time with the
    // longjmp value). Uses glibc's exported _setjmp/_longjmp symbols. Proves
    // tcc's call sequence + glibc's register save/restore work in ring 3.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_setjmp() {
        serial_println!(
            "WARNING: Path-Z setjmp/longjmp C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 45: user-defined variadic function (SysV varargs ABI codegen)
    // in a tcc-built dynamic glibc binary. Exercises tcc's own lowering of the
    // x86_64 variadic ABI (register save area, %al vector count, va_start/
    // va_arg/va_end) for a user-authored isum(int, ...) — a path glibc's printf
    // never covers (its va_arg walk lives inside libc). Purely userspace/codegen.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_vararg() {
        serial_println!(
            "WARNING: Path-Z variadic-function C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 46: floating-point / SSE codegen + the x86_64 SysV FP ABI in a
    // tcc-built dynamic glibc binary. No prior rung touched an XMM register, so
    // tcc's double codegen (mulsd/addsd), the FP calling convention (args/return
    // in %xmm0/%xmm1), and the truncating double->int cast (cvttsd2si) were
    // untested from compiled code. A volatile input defeats constant folding so
    // real SSE + the FP-ABI call sequence run. Purely userspace/codegen.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_float() {
        serial_println!(
            "WARNING: Path-Z floating-point C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 47: struct-by-value argument passing + return (the x86_64 SysV
    // aggregate ABI) in a tcc-built dynamic glibc binary. Prior rungs passed only
    // scalars, so the compiler's aggregate calling convention (eightbyte class-
    // ification, small-struct register-pair packing, ≤16B all-INTEGER return in
    // RAX:RDX) was untested from compiled code. A 16-byte struct passes in GP
    // register pairs and returns in RAX:RDX; a volatile seed defeats constant
    // folding so the real by-value pack/call/return sequence runs. Userspace-only.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_struct() {
        serial_println!(
            "WARNING: Path-Z struct-by-value C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 48: long double / x87 80-bit extended-precision FP in a tcc-
    // built dynamic glibc binary. Distinct from Part 46's SSE double: long double
    // uses the x87 register stack (st0..st7, not XMM) and a separate ABI (args
    // passed in memory, result in st0), so tcc must emit fldt/fstpt + x87 fmul/
    // fadd + fisttp truncation — an untested codegen path. A volatile input
    // defeats constant folding so real x87 + the memory-passing call sequence
    // run. Only undefined symbol is write (no memset → avoids B-TCC-LIBTCC1-MAIN).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_longdouble() {
        serial_println!(
            "WARNING: Path-Z long-double (x87) C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 49: bitfield layout + extract/insert codegen in a tcc-built
    // dynamic glibc binary. No prior rung used bitfields: packing three members
    // into one 32-bit unit exercises tcc's shift+mask extract and load/mask/
    // shift/store RMW insert (leaving neighbours intact) — a distinct codegen
    // path from Part 47's plain struct fields. A volatile seed defeats folding.
    // Only undefined symbol is write (no memset → avoids B-TCC-LIBTCC1-MAIN).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_bitfield() {
        serial_println!(
            "WARNING: Path-Z bitfield C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 50: indirect call through a function-pointer dispatch table in
    // a tcc-built dynamic glibc binary. Prior rungs called by name (direct call);
    // this calls through a runtime-selected function pointer, exercising tcc's
    // indirect-call codegen (call *reg) plus per-slot function-address
    // relocations in a static const table that ld.so fixes up at load. A
    // volatile selector forces the real indirect call. Only undefined sym: write.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_funcptr() {
        serial_println!(
            "WARNING: Path-Z function-pointer C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 51: computed goto (GNU labels-as-values, &&label + goto *p) in
    // a tcc-built dynamic glibc binary. Sibling to Part 50's indirect call: this
    // is the indirect *jump* path (jmp *reg, no call/return), the mechanism real
    // interpreters use for threaded bytecode dispatch. A static const table of
    // label addresses (rodata + per-slot relocation) is indexed by a volatile
    // selector. Only undefined sym: write (no memset → avoids B-TCC-LIBTCC1-MAIN).
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_computed_goto() {
        serial_println!(
            "WARNING: Path-Z computed-goto C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 52: union type-punning (overlapping-member storage aliasing) in
    // a tcc-built dynamic glibc binary. No prior rung used a union: writing one
    // member and reading an overlapping member forces the compiler to lay them
    // at the same offset and round-trip through memory (no register caching
    // across the aliasing read) — the standard byte-reinterpretation idiom,
    // distinct from Part 47's disjoint struct fields. Only undefined sym: write.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_union() {
        serial_println!(
            "WARNING: Path-Z union type-punning C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 53: function-local static variable (persistent, once-init
    // mutable state) in a tcc-built dynamic glibc binary. Prior rungs used only
    // stack automatics + static const tables; a mutable function-local static
    // must live in .data (function scope, static storage), be initialised once
    // at load, and persist across calls. bump() returns ++counter (40->41->42);
    // a volatile rep count forces two real calls. Only undefined sym: write.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_func_static() {
        serial_println!(
            "WARNING: Path-Z function-local-static C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 54: variable-length array (C99 VLA -> runtime-sized stack
    // frame) in a tcc-built dynamic glibc binary. Prior automatic arrays had
    // compile-time-constant sizes (fixed sub rsp,imm); a VLA computes its size at
    // runtime, carves it off rsp (the alloca mechanism), and unwinds on return --
    // a distinct, easily-mis-lowered codegen path. A volatile size defeats
    // constant folding; sum(1..=8)=36 +6 = 42. Only undefined sym: write.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_vla() {
        serial_println!(
            "WARNING: Path-Z VLA (dynamic-stack) C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 55: GCC-style inline assembly with operand constraints in a
    // tcc-built dynamic glibc binary. Exercises tcc's inline-assembler (a
    // separate subsystem from C codegen): parsing the constraint list, allocating
    // registers for =r/r/tied-0 operands, and substituting them into the %0/%2
    // template -- the mechanism real libc/drivers use for syscall/cpuid/atomics/
    // MMIO. asm_add(20,22)=42 via a single addl. Only undefined sym: write.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_inline_asm() {
        serial_println!(
            "WARNING: Path-Z inline-asm C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 56: aggregate brace-initializer (runtime value → tcc-
    // synthesised memset) compiled + glibc-linked + run in ring 3. Regression
    // guard for B-TCC-LIBTCC1-MAIN (the once-observed "unresolved reference to
    // 'main'" link failure that on-target instrumentation could not reproduce).
    // seed(40)+1+1+0 = 42.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_brace_memset() {
        serial_println!(
            "WARNING: Path-Z brace-init/memset C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 57: C11 `_Atomic` + `__atomic_fetch_add` builtin compiled +
    // glibc-linked + run in ring 3. Proves atomic codegen AND that the sized
    // atomic helper `__atomic_fetch_add_4` links out of tcc's libtcc1.a (glibc
    // does not provide it). 21 iterations of += 2 = 42.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_atomic() {
        serial_println!(
            "WARNING: Path-Z C11 atomic C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 58: GNU statement expressions (`({ ... })`) + `__typeof__`
    // compiled + glibc-linked + run in ring 3. Proves the on-target tcc lowers
    // the once-eval type-generic macro idiom (min/max, container_of) that glibc
    // and Linux headers depend on. MAX(42, MAX(17, 37)) = 42.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_stmt_expr() {
        serial_println!(
            "WARNING: Path-Z statement-expression C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 59: C11 `_Generic` type-generic selection compiled + glibc-
    // linked + run in ring 3. Proves the on-target tcc resolves the tgmath.h /
    // type-generic-macro selection primitive at translation time. int+long+
    // double+char weights 10+20+5+7 = 42.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_generic() {
        serial_println!(
            "WARNING: Path-Z C11 _Generic C runtime self-test failed: {:?}",
            e
        );
    }

    // Path Z Part 60: a dense `switch` (lowered to an indexed jump table)
    // compiled + glibc-linked + run in ring 3. Proves the on-target tcc builds
    // and executes a switch jump table — the canonical option/argument-parser
    // codegen shape — de-risking real coreutils/bash parser code. Sum = 42.
    if let Err(e) = proc::spawn::self_test_linux_real_glibc_cc_switch() {
        serial_println!(
            "WARNING: Path-Z dense-switch C runtime self-test failed: {:?}",
            e
        );
    }

    // madvise(MADV_DONTNEED) reclaim test: faults in an anonymous range,
    // reclaims it, and verifies the frames are freed, the VMA persists, and a
    // re-fault zero-fills (Linux anonymous DONTNEED contract).  Needs a live
    // process + page tables, so it runs here alongside the other MM tests.
    if let Err(e) = syscall::linux::self_test_madvise_dontneed() {
        serial_println!("WARNING: Linux madvise(MADV_DONTNEED) self-test failed: {:?}", e);
    }

    // Q6 / design-decisions §24: cross-address-space process_vm_readv/writev
    // introspection gated by a Process capability carrying the DEBUG right.
    // Exercises the authorization predicate + the remote read/write transfer
    // mechanism against a throwaway target process's page table.
    if let Err(e) = syscall::linux::self_test_process_vm_cross_as() {
        serial_println!("WARNING: Linux process_vm cross-AS self-test failed: {:?}", e);
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
    // wqstat backs /proc/wqstat (kernel workqueue work-item stats); like its
    // siblings the self-test now builds fixtures via the real register/enqueue/
    // activate/complete/cancel API and resets the table afterward (leaving no
    // fabricated workqueues), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `wqstat test` kshell subcommand).
    fs::wqstat::self_test();
    // numastat backs /proc/numastat (per-NUMA-node memory placement stats);
    // like its siblings the self-test now builds fixtures via the real
    // register_node/set_distance/record_* API and resets the table afterward
    // (leaving no fabricated nodes), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `numastat test` kshell subcommand).
    fs::numastat::self_test();
    // cpustat backs /proc/cpustat (per-CPU user/system/idle/irq time breakdown);
    // like its siblings the self-test now builds fixtures via the real
    // register_cpu/record_time/record_context_switch/record_interrupt API and
    // resets the table afterward (leaving no fabricated rows), so it is safe at
    // boot and gives the module automated coverage it previously lacked (it was
    // only reachable via the `cpustat test` kshell subcommand).
    fs::cpustat::self_test();
    // irqstat backs /proc/irqstat (per-IRQ-line counts + per-CPU interrupt
    // totals and ISR latency); like its siblings the self-test now builds
    // fixtures via the real register_irq/register_cpu/record/record_latency/
    // mark_spurious API and resets the table afterward (leaving no fabricated
    // rows), so it is safe at boot and gives the module automated coverage it
    // previously lacked (it was only reachable via the `irqstat test` kshell
    // subcommand).
    fs::irqstat::self_test();
    // diskstat backs /proc/diskstat (per-block-device read/write IOPS, bytes,
    // latency, queue depth, merges); like its siblings the self-test now builds
    // fixtures via the real register/record_read/record_write/record_discard/
    // record_flush/record_merge API and resets the table afterward (leaving no
    // fabricated rows), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the
    // `diskstat test` kshell subcommand).
    fs::diskstat::self_test();
    // acpistat backs /proc/acpistat (ACPI event counts, GPE firings, S-state
    // suspend/resume); like its siblings the self-test now builds fixtures via
    // the real register_gpe/record_event/record_gpe/set_s_state API and resets
    // the table afterward (leaving no fabricated rows), so it is safe at boot
    // and gives the module automated coverage it previously lacked (it was only
    // reachable via the `acpistat test` kshell subcommand).
    fs::acpistat::self_test();
    // bpfstat backs /proc/bpfstat (loaded eBPF programs, maps, run counts,
    // verifier errors); like its siblings the self-test now builds fixtures via
    // the real load_program/unload_program/record_run/create_map/
    // record_verifier_error API and resets the table afterward (leaving no
    // fabricated rows), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the
    // `bpfstat test` kshell subcommand).
    fs::bpfstat::self_test();
    // budstat backs /proc/buddyinfo (per-zone buddy-allocator free counts and
    // split/coalesce activity); like its siblings the self-test now builds
    // fixtures via the real register_zone/update_free/record_split/
    // record_coalesce API and resets the table afterward (leaving no fabricated
    // rows), so it is safe at boot and gives the module automated coverage it
    // previously lacked (it was only reachable via the `budstat test` kshell
    // subcommand).
    fs::budstat::self_test();
    // cgiostat backs /proc/cgiostat (per-cgroup disk I/O bytes, IOPS, throttle
    // events, I/O wait); like its siblings the self-test now builds fixtures via
    // the real create_cgroup/remove_cgroup/record_read/record_write/
    // record_throttle/record_io_wait API and resets the table afterward
    // (leaving no fabricated rows), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `cgiostat test` kshell subcommand).
    fs::cgiostat::self_test();
    // compstat backs /proc/compstat (per-zone memory compaction attempts, page
    // migrations, scan activity, stalls); like its siblings the self-test now
    // builds fixtures via the real register_zone/start_compaction/
    // finish_compaction/record_stall API and resets the table afterward
    // (leaving no fabricated rows), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `compstat test` kshell subcommand).
    fs::compstat::self_test();
    // dmastat backs /proc/dmastat (per-device DMA mappings, transfers, IOMMU
    // faults); like its siblings the self-test now builds fixtures via the real
    // register_device/record_map/record_unmap/record_transfer/record_fault API
    // and resets the table afterward (leaving no fabricated rows), so it is safe
    // at boot and gives the module automated coverage it previously lacked (it
    // was only reachable via the `dmastat test` kshell subcommand).
    fs::dmastat::self_test();
    // inodestat backs /proc/inodestat (per-filesystem inode counts + dcache
    // hit/miss); like its siblings the self-test now builds fixtures via the
    // real register_fs/alloc_inode/free_inode/evict/dcache_lookup API and resets
    // the table afterward (leaving no fabricated rows), so it is safe at boot and
    // gives the module automated coverage it previously lacked (it was only
    // reachable via the `inodestat test` kshell subcommand).
    fs::inodestat::self_test();
    // ksmstat backs /proc/ksmstat (Kernel Same-page Merging: per-process
    // sharing, merges/unmerges, scan progress, bytes saved); like its siblings
    // the self-test now builds fixtures via the real register_process/
    // record_merge/record_unmerge/record_scan/update_process API and resets the
    // table afterward (leaving no fabricated rows), so it is safe at boot and
    // gives the module automated coverage it previously lacked (it was only
    // reachable via the `ksmstat test` kshell subcommand).
    fs::ksmstat::self_test();
    // mmapstat backs /proc/mmapstat (per-process mmap/munmap/mprotect counts,
    // per-type breakdown, total bytes mapped); like its siblings the self-test
    // now builds fixtures via the real register_process/record_map/record_unmap/
    // record_protect API and resets the table afterward (leaving no fabricated
    // rows), so it is safe at boot and gives the module automated coverage it
    // previously lacked (it was only reachable via the `mmapstat test` kshell
    // subcommand).
    fs::mmapstat::self_test();
    // pagestat backs /proc/pagestat (per-zone page allocator stats, per-order
    // histogram, huge-page pools); like its siblings the self-test now builds
    // fixtures via the real register_zone/record_alloc/record_free/
    // record_reclaim/set_hugepages API and resets the table afterward (leaving no
    // fabricated rows), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the `pagestat
    // test` kshell subcommand).
    fs::pagestat::self_test();
    // pidstat backs /proc/pidstat (per-PID-namespace allocation counts, reuse
    // rate, high-watermark); like its siblings the self-test now builds fixtures
    // via the real alloc_pid/free_pid/create_ns API and resets the table
    // afterward (leaving only the structural root namespace with zeroed
    // counters, no fabricated activity), so it is safe at boot and gives the
    // module automated coverage it previously lacked (it was only reachable via
    // the `pidstat test` kshell subcommand).
    fs::pidstat::self_test();
    // pmcstat backs /proc/pmcstat (per-CPU hardware performance counters,
    // derived IPC + cache-miss rate, event multiplexing); like its siblings the
    // self-test now builds fixtures via the real register_cpu/record_sample/
    // configure_event/record_multiplex API and resets the table afterward
    // (leaving no fabricated rows), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `pmcstat test` kshell subcommand).
    fs::pmcstat::self_test();
    // powerstat backs /proc/powerstat (per-domain power state, energy in uJ,
    // transitions, wake-event log); like its siblings the self-test now builds
    // fixtures via the real register_domain/record_transition/update_energy/
    // record_wake API and resets the table afterward (leaving no fabricated
    // rows), so it is safe at boot and gives the module automated coverage it
    // previously lacked (it was only reachable via the `powerstat test` kshell
    // subcommand).
    fs::powerstat::self_test();
    // procstat backs /proc/procstat (per-process CPU/memory/IO/fault/ctx-switch
    // accounting, top-CPU/top-mem views); like its siblings the self-test now
    // builds fixtures via the real register/update_cpu/update_memory/unregister
    // API and resets the table afterward (leaving no fabricated rows), so it is
    // safe at boot and gives the module automated coverage it previously lacked
    // (it was only reachable via the `procstat test` kshell subcommand).
    fs::procstat::self_test();
    // ratestat backs /proc/ratestat (per-limiter token-bucket rate-limiting
    // stats: allow/deny counts, current bucket level, burst-exhaustion events);
    // the self-test now builds fixtures via the real register/record_allow/
    // record_deny/refill API and resets the table afterward (leaving no
    // fabricated rows), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the `ratestat
    // test` kshell subcommand).
    fs::ratestat::self_test();
    // rqstat backs /proc/rqstat (per-CPU runqueue depth/wait/load-balance stats);
    // record functions return NotFound for unknown CPUs and there was no register
    // API, so added register_cpu(cpu_id) (zeroed counters) — the proper fix is to
    // register real topology rather than seed fake rows. The self-test now builds
    // fixtures via register_cpu/enqueue/dequeue/record_balance/record_wait and
    // resets the table afterward (leaving no fabricated rows), so it is safe at
    // boot and gives the module automated coverage it previously lacked (it was
    // only reachable via the `rqstat test` kshell subcommand).
    fs::rqstat::self_test();
    // schedlat backs /proc/schedlat (per-CPU scheduling-latency stats: wakeup-to-
    // run / runqueue-wait / preemption latencies + per-CPU latency histograms);
    // record functions return NotFound for unknown CPUs and there was no register
    // API, so added register_cpu(cpu_id) (zeroed counters + empty histogram). The
    // self-test now builds fixtures via register_cpu/record_wakeup/
    // record_runq_wait/record_preempt with exact bucket-placement assertions and
    // resets the table afterward (leaving no fabricated rows), so it is safe at
    // boot and gives the module automated coverage it previously lacked (it was
    // only reachable via the `schedlat test` kshell subcommand).
    fs::schedlat::self_test();
    // ttystat backs /proc/ttystat (per-TTY read/write bytes+ops, line-discipline
    // signals, buffer overruns, buffer usage); already had a full register/
    // record_read/record_write/record_signal/record_overrun/set_buf_used API, so
    // just emptied init_defaults. The self-test now builds fixtures via that real
    // API with exact byte/op assertions and resets the table afterward (leaving no
    // fabricated rows), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the `ttystat test`
    // kshell subcommand).
    fs::ttystat::self_test();
    // zramstat backs /proc/zramstat (per-ZRAM-device compressed-swap stats:
    // original/compressed sizes, mem used, read/write/discard ops, compression
    // ratio); already had a full create_device/remove_device/record_write/
    // record_read/record_discard API, so just emptied init_defaults. The
    // self-test now builds fixtures via that real API with exact size/ratio
    // assertions (incl. saturating mem_used on over-discard) and resets the table
    // afterward (leaving no fabricated rows), so it is safe at boot and gives the
    // module automated coverage it previously lacked (it was only reachable via
    // the `zramstat test` kshell subcommand).
    fs::zramstat::self_test();
    // thpstat backs /proc/thpstat (transparent-huge-page promotion/demotion/
    // split/compaction/khugepaged stats per size class). The two size-class rows
    // (PMD 2MiB, PUD 1GiB) are real fixed structure kept with zeroed counters; the
    // self-test now builds fixtures via record_promotion/record_demotion/
    // record_split/record_alloc_failure/record_compaction/record_khugepaged_scan
    // with exact byte-credit assertions and resets the table afterward (leaving no
    // fabricated activity), so it is safe at boot and gives the module automated
    // coverage it previously lacked (it was only reachable via the `thpstat test`
    // kshell subcommand).
    fs::thpstat::self_test();
    // swapact backs /proc/swapact (per-swap-area swap-in/out counts, pages, and
    // latencies); already had a full register/record_in/record_out API, so just
    // emptied init_defaults. The self-test now builds fixtures via that real API
    // with exact page/latency assertions (incl. swap-in underflow guard and
    // used_pages clamped to total on swap-out) and resets the table afterward
    // (leaving no fabricated rows), so it is safe at boot and gives the module
    // automated coverage it previously lacked (it was only reachable via the
    // `swapact test` kshell subcommand).
    fs::swapact::self_test();
    // writeback backs /proc/writeback (per-device dirty/writeback/written page
    // counts + flusher-thread state). Record functions returned NotFound for
    // unknown devices and there was no register API, so added register_device(dev)
    // (creates a zeroed device row + an idle flusher thread, monotonic id). The
    // real default dirty threshold (DEFAULT_DIRTY_THRESHOLD_PCT) is kept as a
    // legitimate config default. The self-test now builds fixtures via
    // register_device/record_dirty/record_written/start_flush with exact
    // assertions and resets the table afterward (leaving no fabricated rows), so
    // it is safe at boot and gives the module automated coverage it previously
    // lacked (it was only reachable via the `writeback test` kshell subcommand).
    fs::writeback::self_test();
    // blkqueue backs /proc/blkqueue (per-device block I/O queue depth, request
    // merges, plug/unplug events).  Its init_defaults() previously seeded two
    // fictional devices (sda/nvme0n1) with ~60M fabricated submitted I/Os; that
    // demo data was removed (real queues are wired via register_device + the
    // submit/complete/merge/plug/unplug record functions).  The residue-free
    // self_test builds its fixtures via the real API with exact assertions and
    // resets the table afterward, so it is safe at boot.
    fs::blkqueue::self_test();
    // netqueue backs /proc/netqueue (per-NIC-queue TX/RX packets, drops, NAPI
    // poll/budget-exhaustion).  Its init_defaults() previously seeded four
    // fictional eth0 queues with 190M/150M fabricated RX/TX packets; that demo
    // data was removed (real queues are wired via register_queue + the
    // record_packets/record_drop/record_napi_poll functions).  The residue-free
    // self_test builds its fixtures via the real API with exact assertions and
    // resets the table afterward, so it is safe at boot.
    fs::netqueue::self_test();
    // pagecache backs /proc/pagecache (per-device file-cache hits/misses/
    // evictions/readahead and derived hit-rate/readahead-rate).  Its
    // init_defaults() previously seeded two fictional devices (sda/nvme0n1) with
    // 600M fabricated hits and a conjured 97.5% hit rate; that demo data was
    // removed (real devices are wired via register_device + the record_hit/
    // record_miss/record_eviction/record_readahead functions).  The residue-free
    // self_test builds its fixtures via the real API with exact assertions
    // (including hit_rate/readahead_rate) and resets the table afterward, so it
    // is safe at boot.
    fs::pagecache::self_test();
    // taskio backs /proc/taskio (per-process read/write bytes, syscall counts,
    // cancelled writes, io-wait time, major faults).  Its init_defaults()
    // previously seeded three fictional tasks (pid 1/100/200) with 2.6GB/1.25GB
    // fabricated read/write bytes; that demo data was removed (real tasks are
    // wired via register + the record_read/record_write/record_cancelled/
    // record_io_wait/record_page_fault_io functions).  The residue-free
    // self_test builds its fixtures via the real API with exact assertions and
    // resets the table afterward, so it is safe at boot.
    fs::taskio::self_test();
    // netspeed backs /proc/netspeed (per-interface bandwidth snapshots + speed
    // test history).  Its init_defaults() previously seeded a placeholder "eth0"
    // snapshot (fabricating an interface's existence) and its run_test()
    // fabricated ~100 Mbps download speeds from the HPET clock and showed them as
    // a real measurement.  Both were removed: init_defaults is empty (interfaces
    // appear only via update_bandwidth from real net-stack counters) and run_test
    // now honestly returns NotSupported until a real measurement backend exists.
    // The residue-free self_test verifies both and resets the table afterward.
    fs::netspeed::self_test();
    // diskhealth backs /proc/diskhealth (per-drive S.M.A.R.T. health, temp,
    // error rates, failure prediction).  Its init_defaults() previously seeded
    // two fictional disks with INVENTED model/serial numbers ("WDC WD10EZEX",
    // "Samsung 970 EVO") presented as real attached hardware; that demo data was
    // removed (real drives are wired via add_disk + update_attrs from the SMART
    // layer).  The residue-free self_test builds its fixtures via the real API,
    // exercises the compute_health grading (Excellent/Poor/Critical) with exact
    // assertions, and resets the table afterward, so it is safe at boot.
    fs::diskhealth::self_test();
    // netdev backs /proc/netdev (per-NIC packet/byte/error/drop counters + link
    // state, like Linux /proc/net/dev).  Its init_defaults() previously seeded
    // three fictional interfaces (lo/eth0/wlan0) with 51GB/11GB fabricated rx/tx
    // bytes and invented error/drop totals; that demo data was removed (real
    // interfaces are wired via register_iface + the record_rx/record_tx/
    // record_error/record_drop functions).  The residue-free self_test builds its
    // fixtures via the real API with exact assertions and resets the table
    // afterward, so it is safe at boot.
    fs::netdev::self_test();
    // netfilter backs /proc/netfilter (firewall rules + connection tracking +
    // packet accept/drop/reject totals).  Its init_defaults() previously seeded
    // four fictional rules ("allow established" 5M matches/10GB, "allow ssh",
    // "default deny", "allow all out" 4M matches/8GB), two fabricated conntrack
    // entries (192.168.0.1→…:22 and 192.168.0.1→8.8.8.8:443) and invented
    // totals (9.15M packets / 9.05M accepted / 100k dropped); that demo data was
    // removed.  Rules are registered via add_rule, connections via the new
    // track_connection/update_connection/set_conn_state/untrack_connection API,
    // and totals advance only on real record_match calls.  The residue-free
    // self_test builds its fixtures via the real API with exact assertions and
    // resets the table afterward, so it is safe at boot.
    fs::netfilter::self_test();
    // mempress backs the PSI-style /proc/pressure/memory view (memory stall
    // times, reclaim activity, OOM proximity).  Its init_defaults() previously
    // seeded fictional pressure — level Low, 5.5s total stall, 10M reclaim
    // pages, 100k stall events, 50k reclaim events, OOM proximity 15, 5000 level
    // changes; that demo data was removed (the state now starts at level None
    // with all counters zero, advanced only by real record_stall/record_reclaim/
    // update_level/set_oom_proximity calls from the reclaim/OOM paths).  The
    // residue-free self_test builds its fixtures via the real API with exact
    // assertions and resets the table afterward, so it is safe at boot.
    fs::mempress::self_test();
    // devfreq backs /proc/devfreq (per-device frequency governors, current
    // frequency, transition counts, time-in-state).  Its init_defaults()
    // previously seeded two fictional devices — "gpu0" 200MHz-2GHz OnDemand with
    // 500k transitions and "membus" 400MHz-3.2GHz Performance with 10k
    // transitions — plus invented time-in-state buckets and a 510k total; that
    // demo data was removed (devices are registered via register() by the power-
    // management subsystem and counters advance only on real record_transition
    // calls).  The residue-free self_test builds its fixtures via the real API
    // with exact assertions and resets the table afterward, so it is safe at
    // boot.
    fs::devfreq::self_test();
    // memcg backs /proc/memcg (per-cgroup memory usage, limits, swap, failcnt,
    // OOM kills, charge/uncharge counts).  Its init_defaults() previously seeded
    // three fictional cgroups — "/" 2GiB usage / 500k charges, "/system" 512MiB
    // usage / 1GiB limit, "/user" 1GiB usage / 4GiB limit / 128MiB swap — plus
    // invented totals (900k charges, 865k uncharges, 2 failures); that demo data
    // was removed (the cgroup hierarchy is built via create() by the cgroupfs
    // subsystem and usage is accounted only through real charge/uncharge calls).
    // The residue-free self_test builds its fixtures via the real API with exact
    // assertions and resets the table afterward, so it is safe at boot.
    fs::memcg::self_test();
    // cgmem backs /proc/cgmem (per-cgroup page-level memory stats: usage/RSS/
    // cache/swap pages, charges, uncharges, OOM kills, high-watermark events).
    // Its init_defaults() previously seeded three fictional cgroups — "root"
    // 500k usage pages / 10M charges, "system" 1M limit / 5M charges / 2 OOM,
    // "user" 2M limit / 20M charges / 5 OOM — plus invented totals (35M charges,
    // 33.3M uncharges, 7 OOM kills); that demo data was removed (cgroups are
    // created via create() and pages accounted only through real record_charge/
    // record_uncharge calls).  The residue-free self_test builds its fixtures via
    // the real API with exact assertions and resets the table afterward, so it is
    // safe at boot.
    fs::cgmem::self_test();
    // vmzone backs /proc/vmzone (per-zone page totals, watermarks, free/active/
    // inactive pages, alloc/free/reclaim activity).  Its init_defaults()
    // previously seeded four fictional zones — DMA 4096 pages / 10k allocs,
    // DMA32 262k pages / 1M allocs, Normal 2M pages / 50M allocs / 100k reclaims,
    // Movable 500k pages / 5M allocs — plus invented totals (56.01M allocs,
    // 54.76M frees, 125.05k reclaims); that demo data was removed (the page
    // allocator registers its real zones via register() with their actual page
    // totals and watermarks, and publishes activity only through real
    // record_alloc/record_free/record_reclaim calls).  The residue-free self_test
    // builds its fixtures via the real API with exact assertions and resets the
    // table afterward, so it is safe at boot.
    fs::vmzone::self_test();
    // vmballoon backs /proc/vmballoon (VM memory-balloon status: current/target/
    // max pages, inflate/deflate counts and page totals, OOM events, free-page
    // hints).  Its init_defaults() previously seeded a fictional balloon — 100k
    // current/target pages, 1M max, 500 inflates / 300 deflates, 5M/4.9M
    // inflate/deflate page totals, 2 OOM events, 10k free-page hints; that demo
    // data was removed (the balloon driver advertises its capacity via the new
    // configure() API on attach, and counters advance only on real inflate/
    // deflate/record_oom/record_free_hint calls).  The residue-free self_test
    // builds its fixtures via the real API with exact assertions and resets the
    // status afterward, so it is safe at boot.
    fs::vmballoon::self_test();
    // softirq backs /proc/softirq (deferred-interrupt stats: per-CPU softirq/
    // tasklet/ksoftirqd counts and per-type raised/executed/time).  Its
    // init_defaults() previously seeded four fictional CPUs with invented
    // per-type counts (Timer 500k+, NetRx 200k+, Block 100k+, RCU 300k+ each)
    // and ten type rows with invented bases (Timer 2.6M executed, NetRx 1M, RCU
    // 1.5M) plus totals (5.71M raised, 5.7M executed, 5200 tasklets); that demo
    // data was removed.  The ten softirq-vector rows are a fixed kernel taxonomy
    // so they are kept with ZEROED counters, while per-CPU state is created as
    // each CPU comes online via the new register_cpu() API; counters advance
    // only on real raise/run/tasklet_run/ksoftirqd_wakeup calls.  The residue-
    // free self_test builds its fixtures via the real API with exact assertions
    // and resets the tables afterward, so it is safe at boot.
    fs::softirq::self_test();
    // timerq backs /proc/timerq (kernel timer queue: per-timer id/name/type/
    // state/deadline/interval/fire-count/overruns plus created/fired/cancelled/
    // overrun totals).  Its init_defaults() previously seeded three fictional
    // pending timers — "tick" (periodic 10ms), "watchdog" (periodic 1s), and
    // "rcu_callback" (deferrable 50ms) — claiming timers were scheduled in the
    // queue that no subsystem actually armed; that phantom data was removed.
    // Timers are scheduled through the existing add() API and counters advance
    // only on real fire/fire_expired/cancel calls.  The residue-free self_test
    // builds its fixtures via the real API with exact assertions (using a far-
    // future periodic deadline so fire_expired is deterministic) and resets the
    // queue afterward, so it is safe at boot.
    fs::timerq::self_test();
    // schedclass backs /proc/schedclass (scheduler-class diagnostics: per-task
    // pid/class/priority/runtime/switches/migrations and per-class task counts,
    // context switches, runtime, slices, and migrations).  Its init_defaults()
    // previously seeded three fictional tasks — pid 0 Idle (50s runtime, 100k
    // switches), pid 1 Normal (10s runtime, 500k switches, 1000 migrations),
    // pid 2 RealTime (1s runtime, 200k switches, 50 migrations) — with matching
    // invented per-class stats and totals of 800000 switches / 1050 migrations;
    // that phantom data was removed.  The five scheduler-class rows (RealTime/
    // Deadline/Normal/Batch/Idle) are a fixed taxonomy so they are kept with
    // ZEROED counters, while tasks are tracked as they register via the existing
    // register_task() API and counters advance only on real record_switch/
    // record_slice/record_migration calls.  The residue-free self_test builds
    // its fixtures via the real API with exact assertions and resets the tables
    // afterward, so it is safe at boot.
    fs::schedclass::self_test();
    // schedwait backs /proc/schedwait (scheduler-wait diagnostics: per-reason
    // wait counts/total-ns/max-ns across runqueue/iowait/lock/sleep/ipc/pgfault
    // plus a six-bucket latency histogram and global wait totals).  Its
    // init_defaults() previously seeded fabricated activity — per-reason counts
    // of 50M/10M/5M/20M/3M/2M waits, hundreds of billions of ns per reason, a
    // populated histogram, and global totals of 90M waits over 920s; that demo
    // data was removed.  The six reason slots and six histogram buckets are a
    // fixed structure so they are kept ZEROED, and counters advance only on real
    // record_wait calls.  The residue-free self_test builds its fixtures via the
    // real API with exact assertions (including exact histogram-bucket placement)
    // and resets the tables afterward, so it is safe at boot.
    fs::schedwait::self_test();
    // kthread backs /proc/kthread (kernel-thread lifecycle: per-thread id/name/
    // cpu/state/cpu-time/wakeups plus created/exited totals).  Its
    // init_defaults() previously seeded five fictional kernel threads —
    // "kswapd0", "ksoftirqd/0", "kworker/0:0", "kworker/1:0", and "writeback" —
    // with invented CPU times and wakeup counts, plus totals of 100 created / 95
    // exited; that phantom data was removed.  Kernel threads are dynamic (no
    // fixed taxonomy) so the list starts empty, with threads tracked as they
    // register via the existing register()/unregister() API and activity
    // advancing only on real set_state/record_cpu_time calls.  The residue-free
    // self_test builds its fixtures via the real API with exact assertions and
    // resets the list afterward, so it is safe at boot.
    fs::kthread::self_test();
    // kstack backs /proc/kstack (kernel-stack diagnostics: per-CPU stack size,
    // current/high-water usage, overflow + guard-page-hit counts, and usage
    // samples, plus global overflow/guard/sample totals).  Its init_defaults()
    // previously seeded four fictional CPUs — cpu 0..3 with 16KiB stacks,
    // invented current/high-water usage, 1,000,000 samples each and
    // total_used_samples in the billions, plus 1 overflow and 3 guard hits;
    // that demo data was removed.  Per-CPU stack stats are dynamic so the table
    // starts empty, with CPUs added as they come online via the existing
    // register_cpu() API and counters advancing only on real record_usage/
    // record_overflow/record_guard_hit calls.  The residue-free self_test builds
    // its fixtures via the real API with exact assertions and resets the table
    // afterward, so it is safe at boot.
    fs::kstack::self_test();
    // kprobes backs /proc/kprobes (dynamic-instrumentation diagnostics: per-probe
    // id/type/name/address/hits/misses/enabled/overhead plus global hit/miss/
    // overhead totals).  Its init_defaults() previously seeded three fictional
    // probes — a "do_page_fault" kprobe (500k hits), a "sys_read" kretprobe (2M
    // hits, 100 misses), and a "sched:sched_switch" tracepoint (10M hits) — plus
    // totals of 12.5M hits / 100 misses / 625ms overhead; that phantom data was
    // removed.  Probes are dynamic so the list starts empty, with probes
    // installed via the existing register()/unregister() API and counters
    // advancing only on real record_hit calls.  The residue-free self_test builds
    // its fixtures via the real API with exact assertions (incl. disabled-probe
    // miss counting and by_type filtering) and resets the list afterward, so it
    // is safe at boot.
    fs::kprobes::self_test();
    // ftrace backs /proc/ftrace (function-trace diagnostics: per-probe func name/
    // kind/hits/misses/total-ns/max-ns/enabled plus global hit/miss/overhead
    // totals and a global tracing on/off flag).  Its init_defaults() previously
    // seeded four fictional probes — "schedule" (50M hits), "do_page_fault" (10M
    // hits, 100 misses), "sys_read" (30M hits), and "tcp_sendmsg" (5M hits, 50
    // misses, disabled) — plus totals of 95M hits / 150 misses / 1.1s overhead
    // with tracing enabled; that phantom data was removed.  Probes are dynamic so
    // the list starts empty and global tracing starts OFF (the honest default),
    // with probes installed via the existing add_probe()/remove_probe() API and
    // counters advancing only on real record_hit calls.  The residue-free
    // self_test builds its fixtures via the real API with exact assertions (incl.
    // disabled-probe miss counting, max-ns tracking, and the global toggle) and
    // resets the list afterward, so it is safe at boot.
    fs::ftrace::self_test();
    // sockbuf backs /proc/sockbuf (socket-buffer pool diagnostics: per-pool
    // active-buffers/bytes/allocs/frees/drops/peak across tcp/udp/raw/icmp/mcast/
    // general plus global alloc/free/drop/byte totals).  Its init_defaults()
    // previously seeded fabricated activity across all six pools — e.g. TCP with
    // 50,000 active buffers, 100M allocs and 200MB in flight — with global totals
    // of 176.5M allocs / 176.4M frees / 6,660 drops / 231.6MB; that demo data was
    // removed.  The six buffer pools are a fixed taxonomy so they are kept with
    // ZEROED counters, and counters advance only on real alloc/free/record_drop
    // calls.  The residue-free self_test builds its fixtures via the real API
    // with exact assertions (incl. peak high-water tracking across frees and
    // cumulative byte accounting) and resets the table afterward, so it is safe
    // at boot.
    fs::sockbuf::self_test();
    // msivec backs /proc/msivec (MSI/MSI-X interrupt-vector diagnostics:
    // per-device allocated/active vector counts, delivered-interrupt counts and
    // target CPU, plus global vector/interrupt/alloc/free totals).  Its
    // init_defaults() previously seeded four fictional PCIe devices — nvme0
    // (8 MSI-X vectors, 50M interrupts), eth0 (4 MSI-X, 100M), gpu0 (1 MSI, 5M)
    // and ahci0 (1 MSI, 10M) — plus totals of 14 vectors / 165M interrupts /
    // 100 allocs / 86 frees, all surfaced as if real MSI vectors had been
    // programmed into hardware.  That demo data was removed; the device list now
    // starts empty and fills only when a driver actually configures an MSI
    // capability via alloc_vectors().  The residue-free self_test builds its
    // fixtures via the real API with exact assertions (incl. cumulative
    // interrupt accounting that is not decremented on free) and resets the table
    // afterward, so it is safe at boot.
    fs::msivec::self_test();
    // clocksrc backs /proc/clocksrc (clock-source diagnostics: per-source
    // frequency, quality rating, current flag, read count, skew corrections,
    // total/max skew and read latency, plus global read/skew totals).  Its
    // init_defaults() previously seeded three fictional clock sources — tsc
    // (3GHz, Ideal, current, 1B reads, 100 skew corrections), hpet (14.3MHz,
    // Good, 500K reads) and acpi_pm (3.58MHz, Medium, 10K reads, 200 skews) —
    // plus totals of 1,000,510,000 reads / 350 skew corrections, surfaced as if
    // real timekeeping hardware had been calibrated and read.  Clock sources are
    // discovered hardware, so that demo data was removed; the list now starts
    // empty and fills only when the timekeeping subsystem actually registers a
    // calibrated source via register().  The residue-free self_test builds its
    // fixtures via the real API with exact assertions (incl. total/max skew
    // accumulation and latest-latency tracking) and resets the table afterward,
    // so it is safe at boot.
    fs::clocksrc::self_test();
    // cpuidle backs /proc/cpuidle (CPU idle / C-state diagnostics: per-CPU
    // current C-state, per-C-state entry counts and residency times, total
    // idle/active time, plus global transition/idle totals).  Its
    // init_defaults() previously seeded four fictional CPUs with invented
    // C-state entry counts (e.g. CPU0 with 1M C1 / 500K C1E / 100K C3 / 10K C6
    // entries) and multi-second residency times scaled per core, plus a global
    // total of 6,480,000 transitions — surfaced as if real C-state residency
    // had been measured.  CPUs are discovered hardware, so that demo data was
    // removed; a new register_cpu() API adds each core as SMP brings it online,
    // and the per-CPU table fills only through real enter_state/exit_state
    // calls.  The residue-free self_test builds its fixtures via the real API
    // with exact assertions (entry-counter increments by C-state depth,
    // transition counting) and resets the table afterward, so it is safe at
    // boot.
    fs::cpuidle::self_test();
    // cpucache backs /proc/cpucache (CPU cache-hierarchy diagnostics: per-level
    // geometry — size / line size / ways / sets / shared-CPU count — and
    // hit/miss/eviction counters across L1d/L1i/L2/L3, plus global hit/miss
    // totals and hit-rate percentages).  Its init_defaults() previously seeded a
    // plausible-looking but unprobed hierarchy (32KB 8-way L1d/L1i, 256KB L2,
    // 8MB 16-way L3 shared by 4 CPUs) with fabricated activity of 25,000,000,000
    // total hits / 850,000,000 misses — surfaced as if the cache topology had
    // been read from CPUID and its activity measured.  The four levels are a
    // fixed taxonomy so the rows are kept, but with ZEROED geometry and
    // counters; a new set_geometry() API lets a CPUID probe fill in real
    // geometry, and the counters advance only on real record_hit/miss/eviction
    // calls.  The residue-free self_test builds its fixtures via the real API
    // with exact assertions (geometry persistence, per-level + overall hit-rate
    // math, level→index mapping) and resets the table afterward, so it is safe
    // at boot.
    fs::cpucache::self_test();
    // userfault backs /proc/userfault (userfaultfd diagnostics: per-process
    // registered ranges, missing/write-protect/minor fault counts, resolves,
    // total/max resolve latency, copy/zero page counts, plus global fault/
    // resolve/copy/zero totals).  Its init_defaults() previously seeded one
    // fictional handler — pid 1 with 5 ranges, 100K missing / 50K wp / 10K minor
    // faults, 160K resolves, 100K copy pages and 60K zero pages — plus global
    // totals of 160K faults / 160K resolves / 100K copies / 60K zeros, surfaced
    // as if a process were actually handling page faults in userspace.  That
    // demo data was removed; the handler list now fills only when a process
    // really creates a userfaultfd via register().  The residue-free self_test
    // builds its fixtures via the real API with exact assertions (per-fault-type
    // counters, copy vs zero resolve accounting, max-latency tracking, cumulative
    // global totals not decremented on unregister) and resets the table
    // afterward, so it is safe at boot.
    fs::userfault::self_test();
    // iomem backs /proc/iomem (MMIO region diagnostics: per-region name, base,
    // size, cacheable/prefetchable attributes and read/write access counts,
    // plus global read/write totals).  Its init_defaults() previously seeded
    // five fictional regions — LAPIC (50M reads / 10M writes), IOAPIC (1M /
    // 500K), HPET (5M / 100K), GPU_FB (100M writes) and NVMe_BAR (20M / 15M) —
    // plus global totals of 76,000,000 reads / 125,600,000 writes, surfaced as
    // if device memory had been mapped and accessed.  That demo data was
    // removed; the region list now fills only when the kernel actually maps an
    // MMIO region via register().  The residue-free self_test builds its
    // fixtures via the real API with exact assertions (attribute persistence,
    // per-region + global read/write counting, duplicate-base AlreadyExists,
    // cumulative totals not decremented on unregister) and resets the table
    // afterward, so it is safe at boot.
    fs::iomem::self_test();
    // pgtable backs /proc/pgtable (page-table diagnostics: per-level (PML4/PDPT/
    // PD/PT) allocated/freed/active page-table page counts, page-walk count and
    // average depth, TLB-flush counts by scope (single/range/full/global), plus
    // the active page-table-page total).  Its init_defaults() previously seeded
    // fabricated activity — per-level allocs [1, 512, 50K, 2M] / frees
    // [0, 10, 5K, 500K], 100,000,000 page walks summing 350,000,000 levels, TLB
    // flushes of 50M single / 1M range / 500K full / 100K global, and 1,550,503
    // active pages — surfaced as if real paging activity had been measured.  The
    // four levels are a fixed dimension so per_level always returns four rows,
    // but with ZEROED counters; they advance only on real record_alloc/free/
    // walk/tlb_flush calls.  The residue-free self_test builds its fixtures via
    // the real API with exact assertions (per-level alloc/free/active accounting,
    // active-page total tracking, average walk depth 366 from 11 levels / 3
    // walks, per-scope flush counts) and resets the table afterward, so it is
    // safe at boot.
    fs::pgtable::self_test();
    // Memory diagnostics self-test (memdiag) — exercises the RAM-test log and
    // ECC-error tracking surfaced by the `memdiag` kshell command (test runs
    // with pass/fail results, correctable/uncorrectable ECC error counts, and
    // a memory-health summary).  Its init_defaults() previously seeded a
    // hardcoded total_memory_kb of 1,048,576 (a 1 GB placeholder), so
    // `memdiag show` always reported "Total memory: 1024 MB" regardless of the
    // machine's actual RAM.  The size now starts at 0 ("unknown until
    // detected") and advances only on a real set_total_memory() call by RAM
    // detection.  The residue-free self_test builds its fixtures via the real
    // API with exact assertions (test pass/fail accounting, ECC correctable/
    // uncorrectable counts, and a 2 GB size set explicitly via
    // set_total_memory) and resets the table afterward, so it is safe at boot.
    fs::memdiag::self_test();
    // IPC-namespace statistics self-test (ipcns) — exercises the System V IPC
    // namespace table surfaced by /proc/ipcns and the `ipcns` kshell command
    // (per-namespace shared-memory segment / semaphore-set / message-queue
    // counts and byte totals).  Its init_defaults() previously seeded two
    // fictional namespaces — "init" with 50 shm segments / 500 MB and
    // "container-1" with 10 shm / 100 MB, plus global totals of 60 shm / 25 sem
    // / 13 msg — claiming containers and shared memory existed when nothing
    // created them.  init_defaults now starts empty; namespaces appear only on
    // real create_ns() calls and their counters advance only on real
    // record_shm/sem/msg calls.  The residue-free self_test builds its fixtures
    // via the real API with exact assertions (first namespace gets id 1, per-
    // namespace + global SHM/SEM/MSG accounting, cumulative totals not
    // decremented on destroy) and resets the table afterward, so it is safe at
    // boot and /proc/ipcns reads as a truthful empty table.
    fs::ipcns::self_test();
    // I/O-port statistics self-test (ioport) — exercises the x86 port-I/O
    // region table surfaced by /proc/ioport and the `ioport` kshell command
    // (per-region in/out counts + byte totals, plus untracked-access counters).
    // Its init_defaults() previously seeded five fictional regions — PIC
    // (1M/500K), PIT (100K/50K), KBD (5M/1M), RTC (500K/200K) and COM1 (10M/8M)
    // — plus global totals of 16.6M reads / 9.75M writes and 50K/20K untracked,
    // claiming millions of port accesses that never happened (the kernel does
    // not instrument in/out yet — nothing calls record_in/out or
    // register_region outside the kshell).  init_defaults now starts empty;
    // regions appear only on real register_region() calls and counters advance
    // only on real record_in/out calls.  The residue-free self_test builds its
    // fixtures via the real API with exact assertions (region registration,
    // tracked vs untracked accounting, range-boundary matching at 0x103/0x104,
    // cumulative totals) and resets the table afterward, so it is safe at boot
    // and /proc/ioport reads as a truthful empty table.
    fs::ioport::self_test();
    // Hardware-RNG statistics self-test (hwrng) — exercises the entropy pool
    // status and per-source breakdown surfaced by /proc/hwrng and the `hwrng`
    // kshell command (RDRAND/RDSEED/interrupt/disk/input/jitter byte counts and
    // failures, pool fill level, reseed count).  Its init_defaults() previously
    // seeded fabricated activity — per-source bytes [500M, 100M, 50M, 10M, 5M,
    // 20M], 685M generated / 600M requested, a FULL 4096-bit pool reporting
    // "ready", and 100K reseeds — claiming cryptographic-quality entropy had
    // been gathered when none has (a dangerous lie for a security surface).
    // init_defaults now starts with an EMPTY pool (0 bits, not ready) and all
    // source/total counters zeroed; only the pool CAPACITY (4096 bits, a
    // structural constant) is non-zero.  The residue-free self_test builds its
    // fixtures via the real API with exact assertions (generation adds to the
    // pool, requests drain it, per-source failure tracking, reseed refills to
    // capacity, six-source breakdown) and resets afterward, so it is safe at
    // boot and /proc/hwrng reads as a truthful empty pool.
    fs::hwrng::self_test();
    // Certificate-manager self-test.  certmgr previously seeded five well-known
    // root CAs (ISRG Root X1, DigiCert Global Root G2, GlobalSign, Baltimore
    // CyberTrust, Amazon Root CA 1) into init_defaults, each marked Root/System/
    // Valid/pinned but carrying FABRICATED cryptographic material — FNV-hash
    // "fingerprints" instead of real SHA-256 digests, made-up serials, a
    // synthetic 10-year validity window, and no backing PEM file.  /proc/certmgr
    // and the certmgr kshell command surface that list as the real trust store,
    // so presenting phantom Valid/trusted roots is a dangerous fabrication on a
    // security surface.  init_defaults now starts with an EMPTY trust store; a
    // certificate enters only through a real import_cert (a bundled CA PEM) or
    // the ACME path.  The residue-free self_test (clear_all at start and end,
    // returns KernelResult) imports/looks-up/renews via the real API with exact
    // assertions and verifies the empty default, so it is safe at boot.
    if let Err(e) = fs::certmgr::self_test() {
        serial_println!("WARNING: Certificate manager self-test failed: {:?}", e);
    }
    // Auth-broker self-test.  authbroker previously seeded three fictional
    // credentials into init_defaults — "root" (Password), "admin" (PublicKey)
    // and "service_acct" (Token), each with a placeholder hash/key and no real
    // provisioned account behind it.  /proc/authbroker and the authbroker kshell
    // command surface the credential list as the real credential store, so
    // presenting phantom verified credentials for privileged principals is a
    // dangerous fabrication on a security surface.  init_defaults now starts
    // with an EMPTY credential store; credentials enter only through a real
    // store_credential (account provisioning) and grants through
    // grant_capability.  The residue-free self_test builds its fixtures via the
    // real API with exact assertions (store/authenticate/lockout/unlock/grant/
    // revoke) and clears the state afterward, so it is safe at boot.
    fs::authbroker::self_test();
    // Security-module (LSM) statistics self-test.  secmod previously seeded two
    // fictional modules into init_defaults — "capability" (88.8M checks /
    // 168,700 denials / 50K audits) and "apparmor" (71.93M checks / 338,500
    // denials / 100K audits), with fabricated per-hook check/denial arrays and
    // global totals of 160,730,000 checks / 507,200 denials / 150,000 audits.
    // /proc/secmod and the secmod kshell command surface the module list as the
    // real security-enforcement activity, so seeding hundreds of millions of
    // policy checks that never happened is fabricated procfs data.  init_defaults
    // now starts with NO modules and zeroed totals; a module enters only through
    // a real register_module and its counters advance via record_check/
    // record_deny/record_audit on the security-hook path.  The residue-free
    // self_test builds its fixtures via the real API with exact assertions
    // (register/check/deny/audit/enable) and clears the state afterward.
    fs::secmod::self_test();
    // Binary-format loader statistics self-test.  binfmt previously seeded three
    // fictional formats into init_defaults — Elf64 (500K loads / 1K errors),
    // Script (100K / 5K) and Elf32 (10K / 200) — plus an error breakdown
    // [3000, 500, 200, 1500, 800, 200] and global totals of 610,000 loads /
    // 6,200 errors.  /proc/binfmt and the binfmt kshell command surface the
    // format list as the real loader activity, so claiming hundreds of thousands
    // of executions that never happened is fabricated procfs data.  init_defaults
    // now starts with NO formats and zeroed totals; a format enters only through
    // the new register_format API (the real ELF/script/etc. loaders call it) and
    // its counters advance via record_load/record_error on the exec path.  The
    // residue-free self_test builds its fixtures via the real API with exact
    // assertions (register/load/average/max/error/breakdown) and clears the
    // state afterward.
    fs::binfmt::self_test();
    // Recovery-partition self-test.  recoverypart previously seeded a fabricated
    // 500 MB "Healthy" recovery partition (85 MB used) with four pre-installed
    // tools — System Repair, Boot Repair, Memory Test, Command Shell — into
    // init_defaults, none backed by a real partition or tool image.
    // /proc/recoverypart and the recoverypart kshell command surface the
    // partition status, tool list and space usage as a real recovery
    // environment, so claiming recovery tools exist when none are installed
    // could mislead an operator into thinking recovery is available.
    // init_defaults now starts with NO partition (status Missing, no tools, zero
    // space); a real partition is registered via the new register_partition API
    // (when one is detected on disk) and tools via add_tool.  The residue-free
    // self_test builds its fixtures via the real API with exact assertions
    // (register/verify/add/repair/boot/remove) and clears the state afterward.
    fs::recoverypart::self_test();
    // Log-rotation self-test.  Unlike the statistics modules above, logrotate's
    // init_defaults seeds CONFIGURATION (three default rotation rules for
    // syslog/kern.log/auth.log — the analogue of a shipped /etc/logrotate.d
    // policy) with ZEROED activity, so it is a legitimate settings module and the
    // default rules are deliberately kept.  The self_test, however, performs
    // simulated rotations (rotate/check_all), so it is made residue-free (clears
    // STATE and restores the clean default policy at the end) to ensure those
    // simulated rotations never leak into the live /proc/logrotate counters.  It
    // is wired here so its exact assertions (rule ids, rotation/byte totals) are
    // actually exercised at boot.
    fs::logrotate::self_test();
    // Disk-cleanup self-test.  diskclean's init_defaults was already empty, but
    // the fabrication lived in scan(): it used to inject nine hardcoded phantom
    // reclaimable items (~1.46 GB of fake /tmp, /var/cache, trash and crash-dump
    // junk) that the /proc/diskclean generator and the diskclean kshell command
    // surfaced as if real, so an operator would believe gigabytes of junk existed
    // and that cleaning them freed real space.  scan() now honestly finds NOTHING
    // (no filesystem-walk backend exists yet), reporting each real reclaimable
    // file via add_item only when a real scanner is implemented.  The residue-free
    // self_test exercises the honest empty scan plus the real add_item/summarize/
    // estimate/clean primitives with exact assertions and clears STATE afterward.
    fs::diskclean::self_test();
    // Game-mode self-test.  Like logrotate, gamemode is a legitimate SETTINGS
    // module: its init_defaults seeds only configuration (the default
    // optimization set, auto_detect flag and F12 capture hotkey) with NO
    // fabricated games, sessions or activation counts, so /proc/gamemode
    // honestly reports 0 games / 0 activations at boot.  The self_test, however,
    // registers a game, runs an activate/deactivate session and flips config
    // toggles, so it is made residue-free (clears STATE and restores the clean
    // default config at the end) to ensure the kshell `gamemode test` subcommand
    // cannot leak those fixtures into the live /proc/gamemode table.  Wired here
    // so its exact assertions (game id, session/activation totals) are exercised
    // at boot now that it is safe.
    fs::gamemode::self_test();
    // Usage-time self-test.  usagetime is a per-app foreground-time TRACKER, not
    // a fabricator: its init_defaults seeds only the tracking_enabled flag with
    // NO fabricated app-usage records (apps empty, all counters 0), so
    // /proc/usagetime honestly reports 0 tracked apps at boot.  The self_test,
    // however, tracks "browser"/"editor" sessions and sets a limit + a category,
    // and never cleared STATE — so the kshell `usagetime test` subcommand would
    // leak those fabricated usage records into the live /proc/usagetime listing
    // (which prints per-app foreground hours, making the leak look like real
    // usage).  It is now residue-free (clears STATE and restores clean defaults
    // at the end) and wired here so its exact assertions are exercised at boot.
    fs::usagetime::self_test();
    // Screen-time self-test.  screentime is a user-activity / app-focus TRACKER,
    // not a fabricator: its init_defaults seeds only the enabled flag, the
    // initial Active state and default (unlimited) usage limits, with NO
    // fabricated app records or daily history (apps empty, counters 0), so
    // /proc/screentime honestly reports 0 tracked apps at boot.  The self_test,
    // however, records org.editor/org.browser focus events, adds active time,
    // sets a daily limit and runs reset_daily (which creates a history entry),
    // and never cleared STATE — so the kshell `screentime test` subcommand would
    // leak those fabricated activity records into the live /proc/screentime
    // table.  It is now residue-free (clears STATE and restores clean defaults
    // at the end) and wired here so its exact assertions are exercised at boot.
    fs::screentime::self_test();
    // Startup-optimization self-test.  startupopt is a boot PROFILER, not a
    // fabricator: its init_defaults seeds NO boot profile (stages/suggestions
    // empty, all counters 0, fastest_boot_ms a u64::MAX sentinel reported as 0),
    // so /proc/startupopt honestly reports 0 boots at boot.  The self_test,
    // however, records boot stages, runs record_boot() (boot_count → 1) and
    // analyze() (total_analyses → 1), and never cleared STATE — so the kshell
    // `startupopt test` subcommand would leak a fabricated boot profile into the
    // live /proc/startupopt table.  It is now residue-free (clears STATE and
    // restores clean defaults at the end) and wired here so its exact assertions
    // (stage counts, 0 suggestions for sub-second stages, boot/analysis totals)
    // are exercised at boot.
    fs::startupopt::self_test();
    // Eye-protection self-test.  Like logrotate/gamemode, eyeprotect is a
    // legitimate SETTINGS module: its init_defaults seeds two break-reminder
    // PROFILES (the 20-20-20 rule and an Hourly preset) — the analogue of shipped
    // default config — with ZEROED activity counters (total_breaks/snoozes/skips
    // all 0), so /proc/eyeprotect honestly reports no break activity at boot.  The
    // default profiles are deliberately KEPT.  The self_test, however, runs
    // breaks/snooze/skip (bumping the activity counters) and changes the 20-20-20
    // profile's interval, and never cleared STATE — so the kshell `eyeprotect
    // test` subcommand would leak fabricated break activity into /proc/eyeprotect
    // AND corrupt the shipped default profile.  It is now residue-free (clears
    // STATE and restores the clean default profiles at the end) and wired here so
    // its exact assertions are exercised at boot.
    fs::eyeprotect::self_test();
    // File-notification statistics self-test.  fnotify's init_defaults used to
    // fabricate observed activity — inotify 500 watches / 10,000,000 events / 5
    // overflows, fanotify 50 watches / 5,000,000 events, dnotify 10 watches /
    // 100,000 events (15,100,000 phantom events total, with invented per-event-
    // kind breakdowns) — surfaced via /proc/fnotify and the `fnotify` kshell
    // command AS IF REAL, when the inotify/fanotify/dnotify subsystems are not
    // even implemented.  It now seeds only the real three-type taxonomy and the
    // sysctl-style CAPACITY limits (max_watches / max_queue_depth) with ALL
    // activity counters ZEROED (case c), so /proc/fnotify honestly reports 0
    // watches / 0 events.  The residue-free self_test exercises add_watch /
    // record_event / drain_events with exact assertions and restores the zeroed
    // baseline afterward.
    fs::fnotify::self_test();
    // memlayout previously seeded a FABRICATED physical memory layout in
    // init_defaults() — a hand-invented ~1 GiB "Main memory" block plus fixed
    // kernel/heap/APIC ranges — so /proc/memlayout, total_ram() and the
    // `memlayout` kshell command reported RAM totals with NO relation to the
    // machine's actual memory.  It is now populated from the REAL Limine memmap
    // response via populate_from_memmap() (called right after the heap during
    // boot, above).  This residue-free self_test snapshots the live (real) map,
    // exercises populate_from_memmap / add_region / the totals with exact
    // assertions against a synthetic map, then restores the real map so no test
    // fixtures leak into the live /proc/memlayout table.
    fs::memlayout::self_test();
    // netmon backs /proc/netmon and the `netmon` kshell command.  Its
    // init_defaults() previously seeded three FABRICATED connections (sshd
    // LISTEN :22, a browser ESTABLISHED to 93.184.216.34:443 with real-looking
    // byte counts, resolved to 8.8.8.8:53) plus invented aggregate totals, which
    // the kshell command surfaced as if they were live sockets.  It now seeds an
    // EMPTY table (connections are tracked via add_connection / record_traffic /
    // close_connection once the network stack is wired).  This residue-free
    // self_test builds its own fixtures via the real API with exact assertions
    // and resets to empty afterward so no test connections leak into
    // /proc/netmon.
    fs::netmon::self_test();
    // swapmon backs /proc/swapmon and the `swapmon` kshell command.  Its
    // init_defaults() previously seeded a FABRICATED default swap device (a
    // fictional 4 GiB /dev/sda2 partition shown ~500 MiB used) plus invented
    // swap-in/out rate counters, which the kshell command and /proc/swapmon
    // displayed as if they were real swap usage — and none of its mutation APIs
    // had a single real caller.  Meanwhile the kernel already has a real swap
    // subsystem (crate::mm::swap, which also backs /proc/swaps).  swapmon is now
    // a pure read-through over mm::swap + mm::fault with no state of its own, so
    // this self_test asserts the reporting views are exactly consistent with the
    // real subsystem (no fabricated fixtures, nothing to leak).
    fs::swapmon::self_test();
    // netusage backs /proc/netusage and the `netusage` kshell command.  Its
    // init_defaults() previously seeded three FABRICATED interfaces (eth0
    // Ethernet, wlan0 Wi-Fi, lo loopback) with zeroed counters, which the
    // `netusage interfaces` view displayed as if those NICs existed — presuming
    // a wired+wifi machine and inconsistent with the real interface registry
    // (fs::netdev), which seeds empty and registers interfaces only as they come
    // up.  netusage now seeds an EMPTY table (interfaces appear via
    // add_interface; per-app usage via record_traffic).  This residue-free
    // self_test builds its own fixtures via the real API with exact assertions
    // (including the cap-warning counter, which the old test asserted loosely)
    // and resets to empty afterward so nothing leaks into /proc/netusage.
    fs::netusage::self_test();
    // taskmon is the kernel-side process registry behind /proc/taskmon and the
    // `taskmon` kshell command.  Its init_defaults() previously seeded three
    // FABRICATED bootstrap tasks (kernel/init/kshell with invented CPU%, memory,
    // and thread counts) plus an invented SystemResources snapshot (100% CPU,
    // 64 MiB used of 1 GiB), which the `taskmon` command displayed as if they
    // were real processes — while the authoritative live process list is
    // crate::sched::task_list().  taskmon now seeds an EMPTY registry (tasks
    // arrive via register_task as proc::spawn / scheduler accounting wire it; see
    // the DEFERRED PROPER FIX note in todo.txt).  This self_test (never wired
    // before) builds its own fixtures via the real API with exact assertions and
    // resets STATE afterward — the old test left `testapp`/`daemon` behind, which
    // would have leaked into /proc/taskmon now that it runs at boot.
    fs::taskmon::self_test();
    // vmmap is the kernel-side VMA monitor behind /proc/vmmap and the `vmmap`
    // kshell command.  Its init_defaults() previously seeded a FABRICATED pid-1
    // address space — three invented VMAs ([text] r-x, [heap] rw, [stack] rw)
    // with made-up resident/dirty page counts and totals — which /proc/vmmap and
    // the `vmmap` command displayed as if pid 1 were a real process.  The
    // authoritative per-process VMA list is crate::proc::pcb::list_vmas (already
    // backing /proc/<pid>/maps).  vmmap now seeds an EMPTY table (VMAs arrive via
    // create_vma / remove_vma once the memory manager wires mmap/munmap; see the
    // DEFERRED PROPER FIX note in todo.txt for reading the aggregate view from
    // pcb::list_vmas).  This self_test (never wired before, and whose old version
    // relied on the fabricated pid 1 with no end-reset) builds its own fixtures
    // via the real API with exact assertions and resets STATE afterward so
    // nothing leaks into /proc/vmmap.
    fs::vmmap::self_test();
    // pftrack is the kernel-side page-fault tracker behind /proc/pftrack and the
    // `pftrack` kshell command.  Its init_defaults() previously seeded three
    // FABRICATED processes (init pid 1, sshd pid 100, browser pid 200) with
    // invented minor/major/cow fault counts plus invented system totals
    // (total_minor 15570, total_major 525, total_faults 16937), which
    // /proc/pftrack and the hotspots/top_faulters views displayed as if they were
    // real measured fault activity.  record() has NO real callers — the page-fault
    // handler does not call it — so the module is entirely unwired; the system-wide
    // aggregate lives in crate::mm::fault::fault_stats().  pftrack now seeds an
    // EMPTY table (faults arrive via record once the fault handler wires it; see
    // the DEFERRED PROPER FIX note in todo.txt).  This self_test (never wired
    // before, and whose old version relied on the fabricated processes with no
    // end-reset) builds its own fixtures via the real API with exact assertions
    // and resets STATE afterward so nothing leaks into /proc/pftrack.
    fs::pftrack::self_test();
    // vmfrag is the kernel-side VM-fragmentation monitor behind /proc/vmfrag and
    // the `vmfrag` kshell command.  Its init_defaults() previously seeded two
    // FABRICATED zones — `DMA32` and `Normal` (Linux zone names) — with invented
    // per-order fragmentation indices and compaction counts (5000/50000
    // compactions) plus invented totals (55000 compactions / 44000 success /
    // 11000 fail), which /proc/vmfrag displayed as if real.  This kernel has a
    // single global buddy allocator (crate::mm::frame) with no named-zone taxonomy
    // and no memory-compaction subsystem, so register_zone/update_index/
    // record_compaction have NO real callers — the module is entirely unwired.
    // vmfrag now seeds an EMPTY table (zones arrive via register_zone; see the
    // DEFERRED PROPER FIX note in todo.txt for computing real indices from the
    // buddy allocator's per-order free counts).  This self_test (never wired
    // before, and whose old version relied on the fabricated zones with no
    // end-reset) builds its own fixtures via the real API with exact assertions
    // and resets STATE afterward so nothing leaks into /proc/vmfrag.
    fs::vmfrag::self_test();
    // ipclog is the kernel-side IPC message log behind /proc/ipclog and the
    // `ipclog` kshell command.  Its init_defaults() previously seeded three
    // FABRICATED channels — `system_bus`, `vfs_channel`, `gui_events` — with
    // invented message/byte/latency/error counts (inconsistent with the system
    // totals, which were seeded at 0), which /proc/ipclog and the list_channels
    // view displayed as if real IPC traffic.  record() has NO real callers — the
    // IPC subsystem (crate::ipc) does not call it — so the module is entirely
    // unwired.  ipclog now seeds an EMPTY log (channels/messages arrive via record
    // once the IPC layer wires it; see the DEFERRED PROPER FIX note in todo.txt).
    // This self_test (never wired before, and whose old version relied on the
    // fabricated channels with no end-reset) builds its own fixtures via the real
    // API with exact assertions and resets STATE afterward so nothing leaks into
    // /proc/ipclog.
    fs::ipclog::self_test();
    // telemetry is the kernel-side metric registry behind /proc/telemetry and the
    // `telemetry` kshell command.  Its init_defaults() previously seeded four
    // FABRICATED metrics with invented OBSERVED values — cpu.usage_pct 15%,
    // mem.used_mb 512, disk.iops 1200, net.rx_bytes 1048576 — plus a fabricated
    // total_samples of 4, which /proc/telemetry and the list_metrics/by_category
    // views displayed as if real measured telemetry.  record()/register_metric()
    // have NO real callers — no subsystem publishes telemetry yet — so the registry
    // is entirely unwired.  telemetry now seeds an EMPTY registry (metrics arrive
    // via register_metric + record once producers wire it; collection_enabled /
    // interval are real settings, preserved; see the DEFERRED PROPER FIX note in
    // todo.txt).  This self_test (never wired before, and whose old version relied
    // on the fabricated metrics with no end-reset) builds its own fixtures via the
    // real API with exact assertions and resets STATE afterward so nothing leaks
    // into /proc/telemetry.
    fs::telemetry::self_test();
    // fdtable is the kernel-side FD-table tracker behind /proc/fdtable and the
    // `fdtable` kshell command.  Its init_defaults() previously seeded two
    // FABRICATED process FD tables — pid 1 (/dev/console ×3 + /etc/init.conf) and
    // pid 100 (pipe ×2, /dev/null, socket, /var/log/sshd.log) — plus an invented
    // total_opens of 9, which /proc/fdtable and the list_tables view displayed as
    // if real open file descriptors.  The authoritative per-process FD table is the
    // PCB's linux_fd_table (crate::proc::linux_fd::KernelFdTable); open/close/dup
    // have NO real callers — the VFS does not call this parallel tracker — so it is
    // entirely unwired.  fdtable now seeds an EMPTY table (FDs arrive via open/dup
    // once the VFS wires it; see the DEFERRED PROPER FIX note in todo.txt for
    // reading the aggregate view from the PCB).  This self_test (never wired before,
    // and whose old version relied on the fabricated tables with no end-reset)
    // builds its own fixtures via the real API with exact assertions and resets
    // STATE afterward so nothing leaks into /proc/fdtable.
    fs::fdtable::self_test();
    // sysprofiler is the detailed hardware/software inventory behind
    // /proc/sysprofiler and the `sysprofiler` kshell command.  Its
    // init_defaults() previously seeded entirely FABRICATED hardware specs — a
    // "4-core / 8-thread 3.60 GHz" CPU with invented 256 KB / 1 MB / 8 MB
    // caches, "8192 MB DDR4 3200 MHz" memory in "2 / 4" slots, an "NVMe SSD
    // 512 GB / PCIe 4.0 x4" drive, "Integrated Graphics / 512 MB shared", and
    // "UEFI / Secure Boot Disabled" firmware — none measured, which the
    // `sysprofiler all`/`summary`/`section` views displayed as if real.  Unlike
    // the other procfs fabricators in this sweep, REAL sources exist: the CPU
    // section is now built from live CPUID + topology (crate::cpu /
    // crate::cpu_topology — vendor/brand/family/cache + logical/physical/SMT
    // counts) and the Memory section from the real buddy-allocator total
    // (crate::mm::frame::stats).  Device-dependent sections (Storage, Graphics,
    // Firmware, …) are left ABSENT rather than fabricated until those
    // subsystems expose enumeration (see the DEFERRED PROPER FIX note in
    // todo.txt).  This self_test (never wired before, and whose old version
    // relied on the fabricated defaults with no end-reset) exercises the real
    // builders, then rebuilds the real snapshot so /proc/sysprofiler reflects
    // actual CPU + Memory and not its scratch entries.
    fs::sysprofiler::self_test();
    // fs::eventlog is a (redundant, unwired) structured event log behind
    // /proc/eventlog and the `eventlog` kshell command.  Its init_defaults()
    // previously seeded two FABRICATED entries — an Info/System/"kernel" "System
    // boot completed" and "Event log initialized", both stamped with the current
    // time — plus a fabricated total_logged of 2 and counts_by_severity of
    // [0, 2, 0, 0, 0], which /proc/eventlog and the query/recent views displayed
    // as if real logged events.  No subsystem calls log_event(): the kernel's
    // REAL system event log is the separate crate::eventlog module behind
    // /proc/sysevents, so this fs::eventlog is an entirely unwired parallel
    // tracker.  init_defaults now starts EMPTY (no events, zero counters);
    // entries appear only via log_event once a producer wires it.  This self_test
    // (never wired before, and whose old version relied on the fabricated entries
    // with no end-reset) builds its own fixtures via the real API with exact
    // assertions and resets STATE afterward so nothing leaks into /proc/eventlog.
    fs::eventlog::self_test();
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

    // Interrupts were already enabled earlier (Step 21, just before the ring-3
    // self-test battery) so the battery runs preemptively.  The two validations
    // below genuinely require interrupts to be live but do NOT need to run
    // before the battery, so they stay here at the tail of boot.

    // Test sleep_ns (requires interrupts for hrtimer-based wake).
    // Runs after interrupts are enabled because the hrtimer callback fires from
    // the APIC timer ISR.
    if let Err(e) = sched::test_sleep_ns_postboot() {
        serial_println!("FATAL: sleep_ns self-test failed: {}", e);
        cpu::halt_loop();
    }

    // Softirq self-test — verify raise/process/reentry-guard work.
    // Requires interrupts enabled (softirq processing does STI/CLI internally).
    if let Err(e) = softirq::self_test() {
        serial_println!("FATAL: Softirq self-test failed: {}", e);
        cpu::halt_loop();
    }

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

    // Verify the root-namespace routing table feeds resolve_next_hop (the
    // SYS_NET_ROUTE_ADD path). Runs here because it needs netns::init().
    if let Err(e) = net::ipv4::root_route_next_hop_self_test() {
        serial_println!("[WARN] IPv4 root route next-hop self-test failed: {:?}", e);
    }

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
    // Named-volume registry self-test (Docker `docker volume`). Runs after the
    // container self-test; exercises the registry against real backing dirs.
    volume::self_test();
    // Container-network registry + IPAM self-test (Docker `docker network`).
    cnetwork::self_test();
    // Pure parser self-test for the `oci run --memory`/`--cpus` CLI helpers.
    kshell::cli_resource_parser_self_test();

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

    // ALSA PCM ABI self-test (Linux audio-compat foundation).
    if let Err(e) = audio_alsa::self_test() {
        serial_println!("FATAL: ALSA PCM ABI self-test failed: {}", e);
        cpu::halt_loop();
    }

    // ALSA PCM instance-object lifecycle self-test (per-open substream
    // refcounting behind a /dev/snd/pcmC0D0p fd).
    if let Err(e) = ipc::alsa_pcm::self_test() {
        serial_println!("FATAL: ALSA PCM instance self-test failed: {}", e);
        cpu::halt_loop();
    }

    // ALSA control-device ABI self-test (card enumeration foundation behind
    // /dev/snd/controlC0).
    if let Err(e) = audio_alsa_ctl::self_test() {
        serial_println!("FATAL: ALSA control ABI self-test failed: {}", e);
        cpu::halt_loop();
    }

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

    // DRM Linux-uAPI ABI self-test (Linux graphics-compat foundation).
    if let Err(e) = drm::uapi::self_test() {
        serial_println!("FATAL: DRM uAPI ABI self-test failed: {:?}", e);
        cpu::halt_loop();
    }

    // virtio-gpu DRM driver-specific uAPI ABI self-test (3D/virgl foundation
    // for the Vulkan/OpenGL Mesa port).
    if let Err(e) = drm::virtgpu_uapi::self_test() {
        serial_println!("FATAL: virtio-gpu DRM uAPI ABI self-test failed: {:?}", e);
        cpu::halt_loop();
    }

    // DRM card client-instance lifecycle self-test (the /dev/dri fd family).
    if let Err(e) = drm::card_fd::self_test() {
        serial_println!("FATAL: DRM card client self-test failed: {:?}", e);
        cpu::halt_loop();
    }

    // Console VT100/ANSI escape sequence self-test.
    console::self_test();

    // TTY/termios layer self-test (depends on the console being up so that
    // TIOCGWINSZ can report live dimensions).
    tty::self_test();

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

    // Step 22f-3: Run the slab-poisoning self-test.
    // Poisoning itself was enabled right after heap init (see above) so that
    // there is no pre-poison allocation window that would produce spurious
    // red-zone overflow reports (B-HEAP1).  Here we just exercise the
    // UAF/double-free/red-zone detectors to confirm they fire correctly.
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

    // End-to-end cgroup memory-charging test.  Runs as a live scheduler
    // task (not an inline kmain self-test) so `current_task_cgroup()`
    // resolves to a real task and the ambient frame-allocator charging
    // path is exercised — the piece D-CGROUP-TASK-UNASSIGNED said was
    // untestable in the no-task kmain self-test context.  Spawned and
    // awaited *before* BOOT_OK so its PASS/FAIL line lands on the serial
    // log before the boot harness tears QEMU down.
    {
        let e2e_pml4 = mm::page_table::active_pml4_phys();
        match sched::spawn(
            b"cgroup-e2e",
            sched::task::DEFAULT_PRIORITY,
            cgroup_e2e_test_task,
            0,
            e2e_pml4,
        ) {
            Ok(tid) => {
                serial_println!("[boot] cgroup e2e test task spawned (tid={})", tid);
                // Bounded wait: yield until the task signals completion.
                // Capped so a hung test can never wedge boot — if the cap
                // is hit we log a warning and proceed (the boot still
                // succeeds; the test result is simply absent).
                let mut spins: u32 = 0;
                while !CGROUP_E2E_DONE.load(core::sync::atomic::Ordering::Acquire) {
                    sched::yield_now();
                    spins = spins.saturating_add(1);
                    if spins >= 2_000_000 {
                        serial_println!(
                            "[boot] WARNING: cgroup e2e task did not finish in time"
                        );
                        break;
                    }
                }
            }
            Err(e) => {
                serial_println!("[boot] WARNING: failed to spawn cgroup e2e task: {:?}", e);
            }
        }
    }

    // Net→userspace cutover (§63/§66), switch-ON branch. Deferred to here — past
    // every boot self-test — for the same reason as the health monitor below:
    // the persistent daemon owns the NIC and runs continuously, which would
    // perturb the timing-sensitive timeout self-tests (channel/futex/eventfd
    // recv-with-timeout) and the hrtimer pending-count assertions if it were
    // already running while they execute. Spawning it here — after POST, before
    // BOOT_OK and before any userspace process — means the NIC is owned for the
    // system's lifetime by the time init/shell come up, while kernel
    // self-verification still ran in a quiet system. Skipped when the switch is
    // off (the in-kernel resident stack owns the NIC; bounded net self-tests
    // already ran earlier).
    if crate::net::netstack_client::userspace_enabled() {
        if let Err(e) = proc::spawn::run_persistent_netstack() {
            serial_println!(
                "WARNING: persistent userspace netstack (ring 3) startup failed: {:?}",
                e
            );
        }
    }

    // Arm the container healthcheck supervisor. Deferred to here — after every
    // timer self-test (the hrtimer self-test asserts an exact `pending_count`,
    // which a persistent repeating timer would break) — so it only goes live
    // once the system is otherwise fully initialised. The periodic tick fires in
    // ISR context and hands off to the (already-live) workqueue worker.
    container::start_health_monitor();

    // Boot success marker — the boot test script greps for this.
    // Printed synchronously so it appears within seconds of power-on,
    // regardless of how long deferred benchmarks take.
    serial_println!("=== Kernel boot complete ===");
    serial_println!("BOOT_OK");
    boot_timing::mark(boot_timing::Milestone::ShellReady);

    // Disarm the boot-window liveness watchdog: past BOOT_OK the system may
    // legitimately go idle at an interactive prompt (all tasks blocked on the
    // keyboard), which — without a per-task block-reason field — is
    // indistinguishable from the hang the watchdog looks for.  Its job (guard
    // the continuous-progress boot window) is done.
    sched::liveness_disarm();

    // Disarm the hard-lockup NMI watchdog too: past BOOT_OK the BSP may
    // legitimately go long stretches without a timer tick (idle at a prompt),
    // which would otherwise trip the watchdog. No-op if it was never present.
    hardlockup::disarm();

    // Enable file-history auto-versioning now that boot is complete. It starts
    // disabled (see fs::history's static HISTORY init) so that the boot-time
    // staging of OS system files — which runs with interrupts disabled before
    // "Step 21: Enable hardware interrupts" — does not trigger multi-megabyte
    // SHA-256 hashes of overwritten content under IF=0. Such a hash starved the
    // timer-driven hard-lockup watchdog kick and tripped a false-positive NMI
    // that looked like an intermittent BSP-dead hang (known-issues.md
    // B-PTHREAD-YIELDBUDGET). Past BOOT_OK the BSP is preemptible (IF=1) and OS
    // staging is done, so auto-versioning of real user-data writes is safe.
    fs::history::set_auto_version(true);

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
        cwd: None,
        uid_gid: None,
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

/// End-to-end verification that the live frame allocator charges the
/// **current task's** cgroup on alloc and uncharges on free.
///
/// This closes the loop left open by `D-CGROUP-TASK-UNASSIGNED`: the
/// existing frame-allocator self-tests exercise `charge_cgroup_alloc_to`
/// with an *explicit* cgroup because kmain self-tests run with no
/// scheduled task (`current_task_cgroup()` returns root). Running here,
/// as a genuine scheduler task, lets us assign *this* task to a
/// memory-limited cgroup and confirm that the ordinary `alloc_frame`
/// path bills that group.
///
/// Protocol: create a limited child cgroup, join it, allocate N frames
/// via the live path (into a stack array — no heap growth to perturb the
/// count), leave the cgroup, and assert the group's `mem_usage` rose by
/// exactly N. Then free the frames and assert usage returns to baseline
/// (uncharge follows the per-frame record, independent of which cgroup
/// the freeing task is in).
extern "C" fn cgroup_e2e_test_task(_arg: u64) {
    const N: usize = 32;

    let cg = match cgroup::create(cgroup::ROOT_CGROUP) {
        Ok(id) => id,
        Err(e) => {
            serial_println!("[cgroup-e2e] SKIP: cgroup create failed: {:?}", e);
            return;
        }
    };

    // Limit comfortably above N so the test's own allocations succeed;
    // over-limit rejection is covered by the frame-allocator self-tests.
    if let Err(e) = cgroup::set_mem_limit(cg, cgroup::MemLimit::frames((N as u64).saturating_mul(4)))
    {
        serial_println!("[cgroup-e2e] SKIP: set_mem_limit failed: {:?}", e);
        let _ = cgroup::delete(cg);
        return;
    }

    let task_id = sched::current_task_id();
    let base = cgroup::stats(cg).map_or(0, |s| s.mem_usage);

    // Join the memory-limited cgroup. From here, every alloc_frame on this
    // task charges `cg` via the ambient `current_task_cgroup()` path.
    if let Err(e) = sched::set_task_cgroup(task_id, cg) {
        serial_println!("[cgroup-e2e] SKIP: set_task_cgroup failed: {:?}", e);
        let _ = cgroup::delete(cg);
        return;
    }

    // Allocate N frames. Stack array (no heap) so nothing else on this task
    // allocates — and therefore charges the cgroup — between join and measure.
    let mut frames: [Option<mm::frame::PhysFrame>; N] = [None; N];
    let mut allocated = 0usize;
    for slot in frames.iter_mut() {
        match mm::frame::alloc_frame() {
            Ok(f) => {
                *slot = Some(f);
                allocated = allocated.saturating_add(1);
            }
            Err(_) => break,
        }
    }

    // Leave the cgroup *before* any allocating operation (serial print, etc.)
    // so stray allocations don't inflate the measured usage.
    let _ = sched::set_task_cgroup(task_id, cgroup::ROOT_CGROUP);

    let after_alloc = cgroup::stats(cg).map_or(0, |s| s.mem_usage);

    // Free everything. Uncharge follows the per-frame FRAME_CGROUP record,
    // so it debits `cg` correctly even though this task is now back in root.
    for slot in frames.iter_mut() {
        if let Some(f) = slot.take() {
            // SAFETY: each frame came from alloc_frame just above and is
            // freed exactly once (the slot is take()n so no double free).
            let _ = unsafe { mm::frame::free_frame(f) };
        }
    }

    let after_free = cgroup::stats(cg).map_or(0, |s| s.mem_usage);

    let charged = after_alloc.saturating_sub(base);
    let ok = allocated == N && charged == N as u64 && after_free == base;
    if ok {
        serial_println!(
            "[cgroup-e2e] PASS: alloc_frame charged {} frames to cgroup {} (usage {}->{}->{}), uncharge balanced",
            charged, cg, base, after_alloc, after_free
        );
    } else {
        serial_println!(
            "[cgroup-e2e] FAIL: allocated={} charged={} (want {}) base={} after_free={}",
            allocated, charged, N, base, after_free
        );
    }

    let _ = cgroup::delete(cg);

    // Signal kmain (which is blocked in a bounded yield loop before it
    // prints BOOT_OK) that the test has finished and its result is on the
    // serial log.
    CGROUP_E2E_DONE.store(true, core::sync::atomic::Ordering::Release);
}

/// Set true by [`cgroup_e2e_test_task`] when the end-to-end cgroup test
/// completes, so kmain can wait for its serial output before the boot
/// harness observes `BOOT_OK` and tears down QEMU.
static CGROUP_E2E_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

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
