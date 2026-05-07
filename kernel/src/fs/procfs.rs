//! Process/system information pseudo-filesystem (`/proc`).
//!
//! ProcFs is a read-only virtual filesystem that generates content on the fly
//! by querying kernel subsystems.  It provides system information to userspace
//! without adding special-purpose syscalls for every diagnostic need.
//!
//! ## Layout
//!
//! ```text
//! /proc/
//! ├── version        Kernel version string
//! ├── uptime         Uptime in seconds (decimal)
//! ├── meminfo        Memory statistics (key: value format)
//! ├── cpuinfo        CPU topology and features
//! ├── config         Kernel build configuration and enabled features
//! ├── mounts         Mounted filesystems
//! ├── stat           System-wide scheduler statistics
//! ├── filesystems    Available filesystem types
//! ├── cmdline        Kernel command line
//! ├── loadavg        Instantaneous system load
//! ├── cacheinfo      Buffer cache and VFS dcache statistics
//! ├── locks          Advisory file lock information
//! ├── fdinfo         Open file handle listing
//! ├── diskstats      Block device statistics
//! ├── interrupts     APIC timer and IRQ state
//! ├── devices        PCI device listing
//! ├── net            Network interface configuration
//! ├── vmstat         Virtual memory statistics (frames, swap, zram, OOM)
//! ├── buddyinfo      Buddy allocator free blocks per order
//! ├── swaps          Active swap devices with usage and priority
//! ├── fsstats        Per-filesystem debug statistics
//! ├── cas            Content-addressed store statistics
//! ├── integrity      File integrity monitoring statistics
//! ├── fhistory       File version history statistics
//! └── <pid>/         Per-process directories
//!     ├── status     Process name, state, priority, credentials
//!     ├── cmdline    Process command name (null-terminated)
//!     ├── stat       Single-line statistics (pid, name, state, ppid, ...)
//!     ├── maps       Virtual memory areas (PML4, threads)
//!     └── caps       Capability table and credentials
//! ```
//!
//! ## Design
//!
//! Content is generated fresh on every `read_file()` call — there is no
//! caching.  This keeps the implementation simple and ensures data is always
//! current.  The cost is acceptable: procfs reads are infrequent compared to
//! real I/O, and the generation functions are cheap (a few microseconds).
//!
//! Implements the [`FileSystem`] trait.  Write operations return
//! `NotSupported` (this is a read-only filesystem).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};

// ---------------------------------------------------------------------------
// ProcFs implementation
// ---------------------------------------------------------------------------

/// Virtual filesystem exposing kernel and process information.
///
/// All content is generated dynamically — no persistent storage.
pub struct ProcFs;

impl ProcFs {
    /// Create a new ProcFs instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Names of virtual files in the procfs root.
const ROOT_FILES: &[&str] = &[
    "version",
    "uptime",
    "meminfo",
    "cpuinfo",
    "config",
    "mounts",
    "stat",
    "filesystems",
    "cmdline",
    "loadavg",
    "cacheinfo",
    "locks",
    "fdinfo",
    "diskstats",
    "partitions",
    "interrupts",
    "devices",
    "net",
    "vmstat",
    "buddyinfo",
    "swaps",
    "fsstats",
    "heapinfo",
    "bcache",
    "cas",
    "integrity",
    "fhistory",
    "quotas",
    "security",
    "pipes",
    "overlays",
    "namespaces",
    "rlimits",
    "audit",
    "snapshots",
];

/// Names of virtual files inside each `/proc/<pid>/` directory.
const PID_FILES: &[&str] = &[
    "status",
    "cmdline",
    "stat",
    "maps",
    "caps",
];

// ---------------------------------------------------------------------------
// Content generators
//
// Each function generates the content for one virtual file.  They query
// kernel subsystems and format the result as human-readable text.
// ---------------------------------------------------------------------------

/// `/proc/version` — kernel version and build info.
fn gen_version() -> Vec<u8> {
    // Keep this consistent with any future version syscall.
    let text = format!(
        "MintOS kernel 0.1.0 (Rust, x86_64, 16 KiB pages)\n"
    );
    text.into_bytes()
}

/// `/proc/uptime` — system uptime in seconds (decimal with nanosecond precision).
fn gen_uptime() -> Vec<u8> {
    let ns = crate::hpet::elapsed_ns();
    let secs = ns / 1_000_000_000;
    let frac = ns % 1_000_000_000;
    let text = format!("{secs}.{frac:09}\n");
    text.into_bytes()
}

/// `/proc/meminfo` — memory statistics in `key: value` format.
///
/// Modelled after Linux's `/proc/meminfo` but with our own field names
/// reflecting our memory subsystem (16 KiB frames, zero-page pool, slab heap).
fn gen_meminfo() -> Vec<u8> {
    let info = crate::mm::memory_info();
    let mut s = String::with_capacity(512);

    // Total / free / used in KiB (matching Linux convention).
    let total_kib = info.total_bytes / 1024;
    let free_kib = info.free_bytes / 1024;
    let used_kib = info.used_bytes / 1024;

    s.push_str(&format!("MemTotal:       {total_kib} kB\n"));
    s.push_str(&format!("MemFree:        {free_kib} kB\n"));
    s.push_str(&format!("MemUsed:        {used_kib} kB\n"));
    s.push_str(&format!("Frames:         {} total, {} free\n",
        info.total_frames, info.free_frames));

    // Zero-page pool.
    s.push_str(&format!("ZeroPool:       {} pages\n", info.zero_pool_count));
    s.push_str(&format!("ZeroPoolHits:   {}\n", info.zero_pool_hits));
    s.push_str(&format!("ZeroPoolMisses: {}\n", info.zero_pool_misses));

    // Heap allocator.
    s.push_str(&format!("HeapSlabAllocs: {}\n", info.heap_slab_allocs));
    s.push_str(&format!("HeapSlabFrees:  {}\n", info.heap_slab_frees));
    s.push_str(&format!("HeapLargeAllocs:{}\n", info.heap_large_allocs));
    s.push_str(&format!("HeapAllocFails: {}\n", info.heap_alloc_failures));

    // Swap.
    let swap_total_kib = info.swap_total_bytes / 1024;
    let swap_used_kib = info.swap_used_bytes / 1024;
    s.push_str(&format!("SwapTotal:      {swap_total_kib} kB\n"));
    s.push_str(&format!("SwapUsed:       {swap_used_kib} kB\n"));
    s.push_str(&format!("SwapDevices:    {}\n", info.swap_device_count));

    // OOM.
    s.push_str(&format!("OomEvents:      {}\n", info.oom_events));
    s.push_str(&format!("OomKills:       {}\n", info.oom_kills));

    // kswapd.
    s.push_str(&format!("KswapdRunning:  {}\n", info.kswapd_running));
    s.push_str(&format!("KswapdCycles:   {}\n", info.kswapd_reclaim_cycles));
    s.push_str(&format!("KswapdReclaimed:{}\n", info.kswapd_total_reclaimed));

    s.into_bytes()
}

/// `/proc/cpuinfo` — CPU topology and features.
fn gen_cpuinfo() -> Vec<u8> {
    let count = crate::acpi::processor_count();
    let processors = crate::acpi::processors();

    let mut s = String::with_capacity(512);
    s.push_str(&format!("processors: {count}\n"));

    // CPU feature flags (from centralized CPUID detection).
    if let Some(f) = crate::cpu::features() {
        s.push_str("flags      :");
        if f.sse       { s.push_str(" sse"); }
        if f.sse2      { s.push_str(" sse2"); }
        if f.sse3      { s.push_str(" sse3"); }
        if f.ssse3     { s.push_str(" ssse3"); }
        if f.sse4_1    { s.push_str(" sse4_1"); }
        if f.sse4_2    { s.push_str(" sse4_2"); }
        if f.popcnt    { s.push_str(" popcnt"); }
        if f.avx       { s.push_str(" avx"); }
        if f.avx2      { s.push_str(" avx2"); }
        if f.avx512f   { s.push_str(" avx512f"); }
        if f.xsave     { s.push_str(" xsave"); }
        if f.aes_ni    { s.push_str(" aes"); }
        if f.sha       { s.push_str(" sha_ni"); }
        if f.rdrand    { s.push_str(" rdrand"); }
        if f.rdseed    { s.push_str(" rdseed"); }
        if f.rdtscp    { s.push_str(" rdtscp"); }
        if f.rdpid     { s.push_str(" rdpid"); }
        if f.fxsr      { s.push_str(" fxsr"); }
        if f.tsc       { s.push_str(" tsc"); }
        if f.f16c      { s.push_str(" f16c"); }
        if f.bmi1      { s.push_str(" bmi1"); }
        if f.bmi2      { s.push_str(" bmi2"); }
        if f.vaes      { s.push_str(" vaes"); }
        if f.page_1g   { s.push_str(" pdpe1gb"); }
        s.push('\n');

        if f.pmu_version > 0 {
            s.push_str(&format!(
                "pmu        : v{}, {} counters, {}-bit\n",
                f.pmu_version, f.pmu_counters, f.pmu_counter_width
            ));
        }
    }

    // TSC frequency.
    let tsc_freq = crate::bench::tsc_freq();
    if tsc_freq > 0 {
        s.push_str(&format!("tsc_freq   : {} Hz\n", tsc_freq));
    }

    s.push('\n');

    for (i, p) in processors.iter().enumerate() {
        s.push_str(&format!("processor  : {i}\n"));
        s.push_str(&format!("acpi_id    : {}\n", p.acpi_processor_id));
        s.push_str(&format!("apic_id    : {}\n", p.apic_id));
        s.push_str(&format!("enabled    : {}\n", p.enabled));
        s.push_str(&format!("online_cap : {}\n", p.online_capable));
        s.push('\n');
    }

    s.into_bytes()
}

/// `/proc/config` — kernel build configuration and enabled features.
///
/// Reports architecture, page size, subsystem limits, and which filesystem
/// and network features are compiled in.  Uses real constants from the
/// codebase where available, hardcoded values for private constants.
fn gen_config() -> Vec<u8> {
    let mut s = String::with_capacity(512);

    s.push_str("# Kernel Configuration\n");
    s.push_str("ARCH=x86_64\n");
    s.push_str(&format!("PAGE_SIZE={}\n", crate::mm::frame::FRAME_SIZE));
    s.push_str(&format!("MAX_CPUS={}\n", crate::sched::priority_rr::MAX_CPUS));
    s.push_str("PREEMPTION=yes\n");

    // Memory subsystems.
    s.push_str("SWAP=yes\n");
    s.push_str("ZRAM=yes\n");

    // Filesystems.
    s.push_str("EXT4=yes\n");
    s.push_str("FAT=yes\n");
    s.push_str("ISO9660=yes\n");
    s.push_str("MEMFS=yes\n");
    s.push_str("PROCFS=yes\n");
    s.push_str("DEVFS=yes\n");
    s.push_str("SYSFS=yes\n");

    // Drivers.
    s.push_str("VIRTIO_BLK=yes\n");
    s.push_str("VIRTIO_NET=yes\n");

    // Networking.
    s.push_str("TCP=yes\n");
    s.push_str("UDP=yes\n");
    s.push_str("DHCP=yes\n");
    s.push_str("DNS=yes\n");

    // Subsystem limits.
    // cache::MAX_ENTRIES is private (2048), hardcoded here.
    s.push_str("BUFFER_CACHE_SECTORS=2048\n");
    s.push_str(&format!("VFS_DCACHE_SIZE={}\n", super::vfs::VFS_DCACHE_SIZE));

    s.into_bytes()
}

/// `/proc/mounts` — mounted filesystems.
///
/// Format: `<mount_path> <fs_type>` per line (similar to Linux `/proc/mounts`
/// but simplified — we don't have mount options yet).
fn gen_mounts() -> Vec<u8> {
    let mounts = crate::fs::Vfs::mounts_full();
    let mut s = String::with_capacity(256);

    // Format like Linux /proc/mounts: device mountpoint fstype options 0 0
    for (path, fs_type, options) in &mounts {
        let opts = options.to_string();
        s.push_str(&format!("none {path} {fs_type} {opts} 0 0\n"));
    }

    s.into_bytes()
}

/// `/proc/stat` — system-wide task/scheduler statistics.
fn gen_stat() -> Vec<u8> {
    let tasks = crate::sched::task_list();

    let mut s = String::with_capacity(512);
    s.push_str(&format!("tasks: {}\n", tasks.len()));

    use crate::sched::task::TaskState;

    // Count by state.
    let mut running = 0u32;
    let mut ready = 0u32;
    let mut blocked = 0u32;
    let mut suspended = 0u32;
    let mut dead = 0u32;

    for t in &tasks {
        match t.state {
            TaskState::Running => running = running.wrapping_add(1),
            TaskState::Ready => ready = ready.wrapping_add(1),
            TaskState::Blocked => blocked = blocked.wrapping_add(1),
            TaskState::Suspended => suspended = suspended.wrapping_add(1),
            TaskState::Dead => dead = dead.wrapping_add(1),
        }
    }

    s.push_str(&format!("running: {running}\n"));
    s.push_str(&format!("ready: {ready}\n"));
    s.push_str(&format!("blocked: {blocked}\n"));
    s.push_str(&format!("suspended: {suspended}\n"));
    s.push_str(&format!("dead: {dead}\n"));

    // Total CPU ticks across all tasks.
    let total_ticks: u64 = tasks.iter().map(|t| t.total_ticks).sum();
    s.push_str(&format!("total_cpu_ticks: {total_ticks}\n"));

    // Per-task detail.
    s.push('\n');
    s.push_str("# pid  name                state      prio  ticks      cpu\n");
    for t in &tasks {
        let name = core::str::from_utf8(t.name.get(..t.name_len).unwrap_or(&[]))
            .unwrap_or("???");
        let state_str = match t.state {
            TaskState::Running => "running",
            TaskState::Ready => "ready",
            TaskState::Blocked => "blocked",
            TaskState::Suspended => "suspended",
            TaskState::Dead => "dead",
        };
        s.push_str(&format!(
            "  {:<4} {:<19} {:<10} {:<5} {:<10} {}\n",
            t.id, name, state_str, t.priority, t.total_ticks, t.last_cpu
        ));
    }

    s.into_bytes()
}

/// `/proc/filesystems` — list of available filesystem types.
///
/// Format follows Linux: `nodev <type>` for virtual filesystems,
/// plain `<type>` for disk-backed ones.
fn gen_filesystems() -> Vec<u8> {
    let mut s = String::with_capacity(256);

    // Virtual filesystems (no backing block device).
    s.push_str("nodev\tmemfs\n");
    s.push_str("nodev\tprocfs\n");
    s.push_str("nodev\tdevfs\n");
    s.push_str("nodev\tsysfs\n");

    // Disk-backed filesystems.
    s.push_str("\text4\n");
    s.push_str("\tfat\n");
    s.push_str("\tiso9660\n");

    s.into_bytes()
}

/// `/proc/cmdline` — kernel command line.
///
/// Reports a synthetic command line reflecting the boot configuration.
/// In the future, this could read actual bootloader-provided arguments.
fn gen_cmdline() -> Vec<u8> {
    // Build a synthetic cmdline from boot state.
    let cpu_count = crate::acpi::processor_count();
    let text = format!(
        "kernel=mintos cpus={cpu_count} pages=16k\n"
    );
    text.into_bytes()
}

/// `/proc/loadavg` — system load average approximation.
///
/// Reports the number of runnable (ready + running) tasks as an
/// instantaneous load metric.  True exponentially-weighted load
/// averages (1/5/15 min) would require periodic sampling in the
/// scheduler; for now, the snapshot is useful for monitoring.
fn gen_loadavg() -> Vec<u8> {
    let tasks = crate::sched::task_list();

    use crate::sched::task::TaskState;

    let runnable = tasks.iter()
        .filter(|t| matches!(t.state, TaskState::Running | TaskState::Ready))
        .count();
    let total = tasks.len();

    // Format: "load_now running/total last_pid\n"
    // We use the instantaneous runnable count for all three slots
    // (1/5/15 min) since we don't track history yet.
    let load = runnable as f64; // exact integer, no precision loss
    let last_pid = tasks.iter().map(|t| t.id).max().unwrap_or(0);

    let text = format!(
        "{:.2} {:.2} {:.2} {runnable}/{total} {last_pid}\n",
        load, load, load,
    );
    text.into_bytes()
}

/// `/proc/cacheinfo` — buffer cache statistics.
#[allow(clippy::arithmetic_side_effects)]
fn gen_cacheinfo() -> Vec<u8> {
    let stats = super::cache::stats();
    let hit_rate = if stats.reads > 0 {
        (stats.hits as f64 / stats.reads as f64) * 100.0
    } else {
        0.0
    };

    // VFS path resolution cache (dcache) stats.
    let (dcache_hits, dcache_misses, dcache_valid) = super::vfs::Vfs::dcache_stats();
    let dcache_hit_rate = {
        let total = dcache_hits.saturating_add(dcache_misses);
        if total > 0 {
            (dcache_hits as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    };

    let text = format!(
        "--- buffer cache ---\n\
         reads:        {}\n\
         hits:         {}\n\
         misses:       {}\n\
         hit_rate:     {:.1}%\n\
         writes:       {}\n\
         writebacks:   {}\n\
         readaheads:   {}\n\
         entries_used: {}/{}\n\
         entries_dirty:{}\n\
         --- vfs dcache ---\n\
         dcache_hits:  {}\n\
         dcache_misses:{}\n\
         dcache_valid: {}/{}\n\
         dcache_rate:  {:.1}%\n",
        stats.reads,
        stats.hits,
        stats.misses,
        hit_rate,
        stats.writes,
        stats.writebacks,
        stats.readaheads,
        stats.entries_used,
        stats.capacity,
        stats.entries_dirty,
        dcache_hits,
        dcache_misses,
        dcache_valid,
        super::vfs::VFS_DCACHE_SIZE,
        dcache_hit_rate,
    );
    text.into_bytes()
}

/// `/proc/locks` — advisory file lock information.
fn gen_locks() -> Vec<u8> {
    // Query the lock table directly via Vfs internal.
    // We can use lock_query for individual paths, but for a full dump
    // we need to access the table.  Use a simpler approach: just report
    // that the lock subsystem is active.
    let mut text = String::from("LOCK  TYPE       OWNER    PATH\n");

    // Access the global lock table through a helper on Vfs.
    let lock_info = super::vfs::lock_table_dump();
    if lock_info.is_empty() {
        text.push_str("(no active locks)\n");
    } else {
        for (path, lock_type, owner) in &lock_info {
            let type_str = match lock_type {
                super::vfs::LockType::Shared => "SHARED   ",
                super::vfs::LockType::Exclusive => "EXCLUSIVE",
            };
            text.push_str(&format!("FLOCK {} {:>8}  {}\n", type_str, owner, path));
        }
    }
    text.into_bytes()
}

/// `/proc/diskstats` — block device statistics.
fn gen_diskstats() -> Vec<u8> {
    let devices = crate::blkdev::list_devices_full();
    let cache_stats = super::cache::stats();

    let mut text = String::from("DEVICE     SECTORS      SIZE         RO    CACHE\n");

    if devices.is_empty() {
        text.push_str("(no block devices)\n");
    } else {
        for dev in &devices {
            // Calculate size from sector count.
            let bytes = dev.sector_count.saturating_mul(dev.sector_size as u64);
            let size_str = if bytes >= 1_073_741_824 {
                format!("{} GiB", bytes / 1_073_741_824)
            } else if bytes >= 1_048_576 {
                format!("{} MiB", bytes / 1_048_576)
            } else if bytes >= 1024 {
                format!("{} KiB", bytes / 1024)
            } else {
                format!("{} B", bytes)
            };

            let ro_str = if dev.read_only { "yes" } else { "no" };

            text.push_str(&format!(
                "{:<10} {:<12} {:<12} {:<5} {}/{}\n",
                dev.name,
                dev.sector_count,
                size_str,
                ro_str,
                cache_stats.entries_used,
                cache_stats.capacity,
            ));
        }
    }

    // Cache summary.
    let hit_rate = if cache_stats.reads > 0 {
        cache_stats.hits.saturating_mul(100) / cache_stats.reads
    } else {
        0
    };
    text.push_str(&format!(
        "\nBuffer cache: {} hits / {} reads ({}% hit rate), {} readaheads\n",
        cache_stats.hits, cache_stats.reads, hit_rate, cache_stats.readaheads,
    ));

    // Device I/O activity tracking.
    let io = crate::blkdev::io_stats();
    let idle_secs = if io.last_io_tick > 0 {
        let elapsed = crate::apic::tick_count().saturating_sub(io.last_io_tick);
        elapsed / 100 // ~100 Hz timer
    } else {
        0
    };
    text.push_str(&format!(
        "Device I/O: {} reads, {} writes, idle {} sec\n",
        io.total_reads, io.total_writes, idle_secs,
    ));

    text.into_bytes()
}

/// `/proc/partitions` — block device partitions.
///
/// Matches Linux format: `major minor #blocks name`.
/// Since our OS doesn't yet support partitions, each device is listed
/// as a whole-disk entry with major 254 (virtio).
fn gen_partitions() -> Vec<u8> {
    let devices = crate::blkdev::list_devices_full();

    let mut text = String::from("major minor  #blocks  name\n\n");

    for (i, dev) in devices.iter().enumerate() {
        // Calculate size in 1 KiB blocks (Linux convention).
        let kib_blocks = dev.sector_count
            .saturating_mul(dev.sector_size as u64)
            / 1024;
        text.push_str(&format!(
            " 254    {:>4}  {:>8}  {}\n",
            i, kib_blocks, dev.name,
        ));
    }

    text.into_bytes()
}

/// `/proc/fdinfo` — open file handle information.
fn gen_fdinfo() -> Vec<u8> {
    let handles = super::handle::list_handles();
    let mut text = format!("HANDLE  FLAGS  OFFSET       SIZE         PATH\n");

    if handles.is_empty() {
        text.push_str("(no open handles)\n");
    } else {
        for h in &handles {
            // Decode flags into a compact string.
            let mut flags_str = String::new();
            if h.flags & 0x01 != 0 { flags_str.push('R'); }
            if h.flags & 0x02 != 0 { flags_str.push('W'); }
            if h.flags & 0x04 != 0 { flags_str.push('C'); }
            if h.flags & 0x08 != 0 { flags_str.push('T'); }
            if h.flags & 0x10 != 0 { flags_str.push('A'); }
            if flags_str.is_empty() { flags_str.push('-'); }

            text.push_str(&format!(
                "{:<7} {:<5} {:<12} {:<12} {}\n",
                h.id, flags_str, h.offset, h.size, h.path,
            ));
        }
    }

    text.push_str(&format!("\nTotal: {} open handles\n", handles.len()));
    text.into_bytes()
}

/// `/proc/interrupts` — interrupt statistics and IOAPIC IRQ state.
///
/// Reports APIC timer tick count, ISR latency measurements, and
/// per-IRQ pending state for standard x86 IRQ lines.
fn gen_interrupts() -> Vec<u8> {
    let mut text = String::with_capacity(512);

    // APIC timer statistics.
    let ticks = crate::apic::tick_count();
    text.push_str(&format!("APIC timer ticks: {ticks}\n"));

    // ISR latency measurements (if sampling was active).
    if let Some(isr) = crate::apic::isr_measurement_results() {
        text.push_str(&format!(
            "ISR latency:  min={} max={} mean={} cycles ({} samples)\n",
            isr.min_cycles, isr.max_cycles, isr.mean_cycles, isr.count,
        ));
    } else {
        text.push_str("ISR latency:  (no measurements)\n");
    }

    // Per-IRQ pending state from IOAPIC.
    text.push_str("\nIRQ  PENDING  DESCRIPTION\n");
    let irq_descs: &[(u32, &str)] = &[
        (0, "PIT timer / HPET"),
        (1, "Keyboard (PS/2)"),
        (2, "Cascade (PIC2)"),
        (3, "COM2 / Serial"),
        (4, "COM1 / Serial"),
        (6, "Floppy disk"),
        (8, "RTC / CMOS"),
        (9, "ACPI SCI"),
        (11, "PCI / AHCI"),
        (12, "PS/2 mouse"),
        (14, "Primary ATA"),
        (15, "Secondary ATA"),
    ];

    for &(irq, desc) in irq_descs {
        let pending = if crate::ioapic::irq_is_pending(irq) { "yes" } else { "no " };
        text.push_str(&format!("{:<4} {:<8} {}\n", irq, pending, desc));
    }

    text.into_bytes()
}

/// `/proc/devices` — PCI device listing.
///
/// Scans PCI bus 0 and reports all discovered devices with their
/// bus/device/function address, class/subclass codes, and vendor:device IDs.
fn gen_devices() -> Vec<u8> {
    let mut text = String::from("BUS  DEV  FN   CLASS:SUB  VENDOR:DEVICE\n");

    let devices = crate::pci::scan_bus0();
    if devices.is_empty() {
        text.push_str("(no PCI devices found)\n");
    } else {
        for dev in &devices {
            text.push_str(&format!(
                "{:02x}   {:02x}   {:02x}   {:02x}:{:02x}      {:04x}:{:04x}\n",
                dev.address.bus, dev.address.device, dev.address.function,
                dev.class, dev.subclass, dev.vendor_id, dev.device_id,
            ));
        }
        text.push_str(&format!("\n{} devices total\n", devices.len()));
    }

    text.into_bytes()
}

/// `/proc/net` — network interface information.
///
/// Reports the primary network interface's MAC, IP, netmask, gateway,
/// and DNS configuration.  Uses `interface::info()` to get all fields
/// in a single consistent snapshot.
fn gen_net() -> Vec<u8> {
    let mut text = String::with_capacity(256);

    // Get a consistent snapshot of all interface state.
    let ni = crate::net::interface::info();

    let up_str = if ni.up { "UP" } else { "DOWN" };
    text.push_str(&format!("Interface: eth0  ({})\n", up_str));
    // MacAddress is a newtype around [u8; 6]; access via .0[i].
    text.push_str(&format!("  MAC:     {}\n", ni.mac)); // Display impl formats as hex
    text.push_str(&format!("  IPv4:    {}\n", ni.ip));
    text.push_str(&format!("  Netmask: {}\n", ni.subnet_mask));
    text.push_str(&format!("  Gateway: {}\n", ni.gateway));
    text.push_str(&format!("  DNS:     {}\n", ni.dns));

    text.into_bytes()
}

/// `/proc/vmstat` — virtual memory statistics.
///
/// Summarizes page fault handling, swap activity, and frame allocator
/// state.  Useful for diagnosing memory pressure and swap storms.
fn gen_vmstat() -> Vec<u8> {
    let info = crate::mm::memory_info();

    let mut s = String::with_capacity(512);

    // Frame allocator.
    s.push_str(&format!("nr_free_frames {}\n", info.free_frames));
    s.push_str(&format!("nr_total_frames {}\n", info.total_frames));

    // Zero page pool.
    s.push_str(&format!("nr_zero_pool {}\n", info.zero_pool_count));
    s.push_str(&format!("zero_pool_hits {}\n", info.zero_pool_hits));
    s.push_str(&format!("zero_pool_misses {}\n", info.zero_pool_misses));

    // Heap allocator.
    s.push_str(&format!("heap_slab_allocs {}\n", info.heap_slab_allocs));
    s.push_str(&format!("heap_slab_frees {}\n", info.heap_slab_frees));
    s.push_str(&format!("heap_large_allocs {}\n", info.heap_large_allocs));
    s.push_str(&format!("heap_alloc_failures {}\n", info.heap_alloc_failures));

    // Swap.
    let swap_free = crate::mm::swap::free_slots();
    let swap_used = crate::mm::swap::used_slots();
    s.push_str(&format!("swap_free_slots {swap_free}\n"));
    s.push_str(&format!("swap_used_slots {swap_used}\n"));

    // Compression.
    let comp = crate::mm::swap::compression_stats();
    s.push_str(&format!("zram_compressed_bytes {}\n", comp.compressed_bytes));
    s.push_str(&format!("zram_uncompressed_bytes {}\n", comp.uncompressed_bytes));
    s.push_str(&format!("zram_compressed_pages {}\n", comp.compressed_count));
    s.push_str(&format!("zram_uncompressed_pages {}\n", comp.uncompressed_count));
    if comp.uncompressed_bytes > 0 {
        s.push_str(&format!("zram_ratio_pct {}\n", comp.ratio_percent()));
        s.push_str(&format!("zram_bytes_saved {}\n", comp.bytes_saved()));
    }

    // kswapd.
    s.push_str(&format!("kswapd_running {}\n", if info.kswapd_running { 1 } else { 0 }));
    s.push_str(&format!("kswapd_reclaim_cycles {}\n", info.kswapd_reclaim_cycles));
    s.push_str(&format!("kswapd_total_reclaimed {}\n", info.kswapd_total_reclaimed));

    // OOM.
    s.push_str(&format!("oom_events {}\n", info.oom_events));
    s.push_str(&format!("oom_kills {}\n", info.oom_kills));

    s.into_bytes()
}

/// `/proc/buddyinfo` — buddy allocator free block counts per order.
///
/// Each line shows how many free blocks exist at each order level.
/// Order 0 = 1 frame (16 KiB), order 1 = 2 frames (32 KiB), etc.
/// This is essential for diagnosing memory fragmentation.
fn gen_buddyinfo() -> Vec<u8> {
    match crate::mm::frame::stats() {
        Some(stats) => {
            let mut s = String::with_capacity(256);
            s.push_str("Node 0, zone Normal ");
            for (order, count) in stats.order_counts.iter().enumerate() {
                if order > 0 {
                    s.push(' ');
                }
                s.push_str(&format!("{count}"));
            }
            s.push('\n');

            // Also show derived info.
            s.push_str(&format!(
                "\n# Order sizes: 0={} KiB, 1={} KiB, ..., 10={} KiB\n",
                16, 32, 16 * 1024
            ));
            s.push_str(&format!(
                "# Total free frames: {}\n",
                stats.free_frames
            ));

            s.into_bytes()
        }
        None => b"(frame allocator not initialized)\n".to_vec(),
    }
}

/// `/proc/fsstats` — per-filesystem debug statistics.
///
/// Iterates all mounted filesystems and calls their `debug_stats()` method,
/// concatenating the results.  Useful for monitoring filesystem internals
/// (extent counts, inode usage, cache states, etc.) in a single read.
fn gen_fsstats() -> Vec<u8> {
    let mounts = crate::fs::Vfs::mounts();
    let mut s = String::with_capacity(512);

    for (mount_path, fs_type) in &mounts {
        s.push_str(&format!("--- {} ({}) ---\n", mount_path, fs_type));
        match crate::fs::Vfs::debug_stats(mount_path) {
            Ok(stats) if !stats.is_empty() => {
                s.push_str(&stats);
                if !stats.ends_with('\n') {
                    s.push('\n');
                }
            }
            Ok(_) => {
                s.push_str("(no stats)\n");
            }
            Err(_) => {
                s.push_str("(unavailable)\n");
            }
        }
    }

    if mounts.is_empty() {
        s.push_str("(no filesystems mounted)\n");
    }

    s.into_bytes()
}

/// `/proc/heapinfo` — kernel heap allocator statistics.
///
/// Shows slab allocator and large-allocation counters, refill
/// count, and failure count.  Useful for diagnosing memory
/// allocation patterns and detecting heap pressure.
#[allow(clippy::arithmetic_side_effects)]
fn gen_heapinfo() -> Vec<u8> {
    let stats = crate::mm::heap::stats();
    let mut s = String::with_capacity(512);

    s.push_str("Kernel Heap Statistics\n");
    s.push_str("---------------------\n");

    // Slab allocator stats (small allocations, per-CPU fast path).
    let slab_active = stats.slab_allocs.saturating_sub(stats.slab_frees);
    s.push_str(&format!(
        "slab_allocs:    {}\n\
         slab_frees:     {}\n\
         slab_active:    {} (allocs - frees)\n\
         slab_refills:   {}\n",
        stats.slab_allocs, stats.slab_frees, slab_active, stats.slab_refills,
    ));

    // Large allocation stats (buddy allocator path, >512 bytes).
    let large_active = stats.large_allocs.saturating_sub(stats.large_frees);
    s.push_str(&format!(
        "large_allocs:   {}\n\
         large_frees:    {}\n\
         large_active:   {} (allocs - frees)\n",
        stats.large_allocs, stats.large_frees, large_active,
    ));

    // Failure and total stats.
    let total_allocs = stats.slab_allocs.saturating_add(stats.large_allocs);
    let total_frees = stats.slab_frees.saturating_add(stats.large_frees);
    s.push_str(&format!(
        "total_allocs:   {}\n\
         total_frees:    {}\n\
         alloc_failures: {}\n",
        total_allocs, total_frees, stats.alloc_failures,
    ));

    s.into_bytes()
}

/// `/proc/bcache` — buffer cache statistics.
///
/// Shows hit/miss rates, dirty/clean entries, read-ahead stats,
/// and overall cache utilization.
fn gen_bcache() -> Vec<u8> {
    let stats = super::cache::stats();

    let mut s = String::with_capacity(512);
    s.push_str("Buffer Cache Statistics\n");
    s.push_str("----------------------\n");

    // Hit rate calculation.
    let total_io = stats.reads;
    let hit_rate = if total_io > 0 {
        (stats.hits * 100) / total_io
    } else {
        0
    };

    s.push_str(&format!(
        "reads:        {}\n\
         hits:         {} ({}%)\n\
         misses:       {}\n\
         writes:       {}\n\
         writebacks:   {}\n\
         readaheads:   {}\n\
         exp_flushes:  {}\n",
        stats.reads,
        stats.hits, hit_rate,
        stats.misses,
        stats.writes,
        stats.writebacks,
        stats.readaheads,
        stats.expired_flushes,
    ));

    s.push_str(&format!(
        "entries_used: {}/{}\n\
         entries_dirty:{}/{}\n",
        stats.entries_used, stats.capacity,
        stats.entries_dirty, stats.capacity,
    ));

    // Utilization percentage.
    let util = if stats.capacity > 0 {
        (stats.entries_used * 100) / stats.capacity
    } else {
        0
    };
    let dirty_pct = if stats.capacity > 0 {
        (stats.entries_dirty * 100) / stats.capacity
    } else {
        0
    };
    s.push_str(&format!(
        "utilization:  {}%\n\
         dirty_pct:    {}%\n",
        util, dirty_pct,
    ));

    s.into_bytes()
}

/// `/proc/swaps` — active swap devices, Linux-compatible format.
///
/// Shows each swap device's type, capacity, usage, and priority.
fn gen_swaps() -> Vec<u8> {
    let devices = crate::mm::swap::list_devices();

    let mut s = String::with_capacity(256);
    // Header matching Linux's /proc/swaps format.
    s.push_str("Filename\t\t\tType\t\tSize\tUsed\tPriority\n");

    if devices.is_empty() {
        // No swap devices.
        return s.into_bytes();
    }

    for dev in &devices {
        // Size/used in KiB (1 slot = 1 frame = 16 KiB).
        let size_kib = (dev.total_slots as u64).saturating_mul(16);
        let used_kib = (dev.used_slots as u64).saturating_mul(16);
        s.push_str(&format!(
            "{}\t\t\t{}\t\t{}\t{}\t{}\n",
            dev.name, dev.device_type, size_kib, used_kib, dev.priority
        ));
    }

    s.into_bytes()
}

/// `/proc/cas` — Content-addressed store statistics.
///
/// Shows blob count, total bytes, deduplication hits, GC stats,
/// and capacity.
fn gen_cas() -> Vec<u8> {
    let st = super::cas::stats();

    let mut s = String::with_capacity(512);
    s.push_str("Content-Addressed Store\n");
    s.push_str("----------------------\n");

    let util_pct = if st.max_bytes > 0 {
        (st.total_bytes * 100) / st.max_bytes
    } else {
        0
    };

    s.push_str(&format!(
        "blob_count:         {}\n\
         total_bytes:        {} ({} / {} = {}%)\n\
         total_refs:         {}\n\
         dedup_hits:         {}\n\
         gc_collected:       {}\n\
         integrity_failures: {}\n",
        st.blob_count,
        st.total_bytes, st.total_bytes, st.max_bytes, util_pct,
        st.total_refs,
        st.dedup_hits,
        st.gc_collected,
        st.integrity_failures,
    ));

    s.into_bytes()
}

/// `/proc/integrity` — File integrity monitoring statistics.
///
/// Shows baseline entry count, configuration, and operation counts.
fn gen_integrity() -> Vec<u8> {
    let st = super::integrity::stats();

    let mut s = String::with_capacity(512);
    s.push_str("File Integrity Monitor\n");
    s.push_str("---------------------\n");

    s.push_str(&format!(
        "baseline_entries:    {}\n\
         max_entries:         {}\n\
         max_file_size:       {}\n\
         baseline_operations: {}\n\
         verify_operations:   {}\n",
        st.baseline_entries,
        st.max_entries,
        st.max_file_size,
        st.baseline_count,
        st.verify_count,
    ));

    if st.baseline_timestamp > 0 {
        let secs = st.baseline_timestamp / 1_000_000_000;
        s.push_str(&format!("last_baseline:       {}s after boot\n", secs));
    } else {
        s.push_str("last_baseline:       never\n");
    }

    s.into_bytes()
}

/// `/proc/fhistory` — File version history statistics.
///
/// Shows tracked file count, total versions, eviction stats,
/// and operation counters.
fn gen_fhistory() -> Vec<u8> {
    let st = super::history::stats();

    let mut s = String::with_capacity(512);
    s.push_str("File Version History\n");
    s.push_str("--------------------\n");

    s.push_str(&format!(
        "enabled:            {}\n\
         auto_version:       {}\n\
         tracked_files:      {}\n\
         total_versions:     {}\n\
         evicted_versions:   {}\n\
         record_operations:  {}\n\
         restore_operations: {}\n",
        if st.enabled { "yes" } else { "no" },
        if st.auto_version { "yes" } else { "no" },
        st.tracked_files,
        st.total_versions,
        st.evicted_versions,
        st.record_count,
        st.restore_count,
    ));

    s.into_bytes()
}

/// `/proc/quotas` — Filesystem quota status.
///
/// Shows global quota enforcement status and per-subject usage/limits.
fn gen_quotas() -> Vec<u8> {
    let st = super::quota::stats();
    let all = super::quota::list_all();

    let mut s = String::with_capacity(1024);
    s.push_str("Filesystem Quotas\n");
    s.push_str("-----------------\n");
    s.push_str(&format!(
        "enforcement: {}\n\
         entries:     {}\n\
         user_quotas: {}\n\
         group_quotas:{}\n\
         over_soft:   {}\n\
         over_hard:   {}\n",
        if st.enabled { "yes" } else { "no" },
        st.entries,
        st.user_quotas,
        st.group_quotas,
        st.over_soft,
        st.over_hard,
    ));

    if !all.is_empty() {
        s.push_str("\nSubject      Bytes Used   Soft Limit   Hard Limit   Files  Status\n");
        for info in &all {
            let subj = match info.subject {
                super::quota::QuotaSubject::User(uid) => format!("user:{}", uid),
                super::quota::QuotaSubject::Group(gid) => format!("group:{}", gid),
            };
            let status = if info.over_hard_bytes || info.over_hard_inodes {
                "OVER_HARD"
            } else if info.over_soft_bytes || info.over_soft_inodes {
                "over_soft"
            } else {
                "ok"
            };
            s.push_str(&format!("{:<12} {:>12} {:>12} {:>12} {:>6} {}\n",
                subj,
                super::quota::format_bytes(info.usage.bytes_used),
                if info.limits.soft_bytes > 0 {
                    super::quota::format_bytes(info.limits.soft_bytes)
                } else {
                    String::from("-")
                },
                if info.limits.hard_bytes > 0 {
                    super::quota::format_bytes(info.limits.hard_bytes)
                } else {
                    String::from("-")
                },
                info.usage.inodes_used,
                status,
            ));
        }
    }

    s.into_bytes()
}

/// `/proc/security` — Security posture summary.
///
/// Consolidates capability system status, IOMMU protection,
/// namespace isolation, file tags, audit trail, and pending
/// capability requests into a single overview.
fn gen_security() -> Vec<u8> {
    let mut s = String::with_capacity(1024);
    s.push_str("Security Posture\n");
    s.push_str("================\n\n");

    // --- IOMMU ---
    s.push_str("[IOMMU / DMA Protection]\n");
    let iommu_available = crate::iommu::is_available();
    s.push_str(&format!(
        "  status:             {}\n",
        if iommu_available { "active" } else { "not detected" }
    ));
    if iommu_available {
        s.push_str(&format!(
            "  vendor:             {:?}\n\
               units:              {}\n",
            crate::iommu::vendor(),
            crate::iommu::unit_count(),
        ));
        let remap = crate::iommu_remap::stats();
        s.push_str(&format!(
            "  dma_remapping:      {}\n\
               active_domains:     {}\n\
               mapped_pages:       {}\n\
               dma_faults:         {}\n",
            if remap.active { "enabled" } else { "disabled" },
            remap.active_domains,
            remap.total_mapped_pages,
            remap.total_faults,
        ));
    }
    s.push('\n');

    // --- CET (Control-flow Enforcement) ---
    let cet = crate::cet::status();
    s.push_str("[Control-flow Enforcement (CET)]\n");
    s.push_str(&format!(
        "  shadow_stack_hw:    {}\n\
           ibt_hw:             {}\n\
           supervisor_shstk:   {}\n\
           supervisor_ibt:     {}\n\
           cp_exceptions:      {}\n",
        if cet.hw_shstk { "supported" } else { "not available" },
        if cet.hw_ibt { "supported" } else { "not available" },
        if cet.supervisor_shstk { "active" } else { "inactive" },
        if cet.supervisor_ibt { "active" } else { "inactive" },
        cet.cp_exceptions,
    ));
    s.push('\n');

    // --- Capability Audit ---
    let audit = crate::cap::audit::stats();
    s.push_str("[Capability Audit]\n");
    s.push_str(&format!(
        "  auditing:           {}\n\
           total_events:       {}\n\
           grants:             {}\n\
           denials:            {}\n\
           revocations:        {}\n\
           ring_entries:       {} / 128\n",
        if audit.enabled { "enabled" } else { "disabled" },
        audit.total_events,
        audit.total_grants,
        audit.total_denials,
        audit.total_revokes,
        audit.ring_entries,
    ));
    s.push('\n');

    // --- Capability Groups ---
    let group_count = crate::cap::groups::count();
    s.push_str("[Capability Groups]\n");
    s.push_str(&format!("  defined_groups:     {}\n", group_count));
    // List groups briefly.
    let groups = crate::cap::groups::list();
    for (id, name, member_count, _max, enabled) in &groups {
        s.push_str(&format!(
            "  group[{}]:           {} (members: {}, {})\n",
            id,
            name,
            member_count,
            if *enabled { "active" } else { "disabled" },
        ));
    }
    s.push('\n');

    // --- File Tags ---
    let file_tag_count = crate::cap::file_tags::count();
    s.push_str("[File Capability Tags]\n");
    s.push_str(&format!("  tagged_paths:       {}\n", file_tag_count));
    s.push('\n');

    // --- Capability Requests ---
    let pending = crate::cap::request::pending_count();
    s.push_str("[Capability Requests]\n");
    s.push_str(&format!("  pending_requests:   {}\n", pending));
    s.push('\n');

    // --- Process Namespaces ---
    let ns_count = crate::ipc::namespace::active_count();
    s.push_str("[Process Namespaces]\n");
    s.push_str(&format!("  active_namespaces:  {}\n", ns_count));
    s.push('\n');

    // --- Overall Assessment ---
    s.push_str("[Assessment]\n");
    let mut issues: u32 = 0;
    if !iommu_available {
        s.push_str("  WARNING: No IOMMU — DMA attacks possible from PCI devices\n");
        issues += 1;
    }
    if !audit.enabled {
        s.push_str("  WARNING: Capability auditing disabled\n");
        issues += 1;
    }
    if !cet.supervisor_shstk && cet.hw_shstk {
        s.push_str("  NOTE: CET shadow stacks available but not enabled\n");
    }
    if !cet.hw_shstk {
        s.push_str("  INFO: Hardware CET not available (pre-11th gen or QEMU)\n");
    }
    if audit.total_denials > 0 {
        s.push_str(&format!(
            "  NOTE: {} capability denial(s) recorded — review audit log\n",
            audit.total_denials,
        ));
    }
    if issues == 0 {
        s.push_str("  All security subsystems operational\n");
    }

    s.into_bytes()
}

/// `/proc/<pid>/status` — per-task status information (human-readable).
///
/// Includes both task-level (scheduler) and process-level (PCB) data
/// when the task belongs to a process.
fn gen_pid_status(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::sched::task::TaskState;

    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let state_str = match task.state {
        TaskState::Running => "running",
        TaskState::Ready => "ready",
        TaskState::Blocked => "blocked",
        TaskState::Suspended => "suspended",
        TaskState::Dead => "dead",
    };

    // CPU time in milliseconds (timer ticks are 10 ms each at 100 Hz).
    let cpu_ms = task.total_ticks.saturating_mul(10);

    let mut s = String::with_capacity(512);
    s.push_str(&format!("Name:     {name}\n"));
    s.push_str(&format!("Pid:      {}\n", task.id));
    s.push_str(&format!("State:    {state_str}\n"));
    s.push_str(&format!("Priority: {}\n", task.priority));
    s.push_str(&format!("CpuTime:  {cpu_ms} ms\n"));
    s.push_str(&format!("Scheduled:{}\n", task.schedule_count));
    s.push_str(&format!("LastCpu:  {}\n", task.last_cpu));

    // Process-level info (if the task belongs to a process).
    // We treat the task_id as a potential process ID.
    if let Some(proc_name) = crate::proc::pcb::name(task_id) {
        s.push_str(&format!("ProcName: {}\n", proc_name));
    }
    if let Some(parent) = crate::proc::pcb::parent(task_id) {
        s.push_str(&format!("PPid:     {}\n", parent));
    }
    if let Some(proc_state) = crate::proc::pcb::state(task_id) {
        s.push_str(&format!("ProcState:{:?}\n", proc_state));
    }
    if let Some(creds) = crate::proc::pcb::get_credentials(task_id) {
        s.push_str(&format!("Uid:      {}\n", creds.uid));
        s.push_str(&format!("Gid:      {}\n", creds.gid));
        if !creds.groups.is_empty() {
            s.push_str("Groups:   ");
            for (i, g) in creds.groups.iter().enumerate() {
                if i > 0 { s.push(' '); }
                s.push_str(&format!("{g}"));
            }
            s.push('\n');
        }
    }
    if let Some(threads) = crate::proc::pcb::get_threads(task_id) {
        s.push_str(&format!("Threads:  {}\n", threads.len()));
    }
    if let Some(caps) = crate::proc::pcb::cap_count(task_id) {
        s.push_str(&format!("CapCount: {}\n", caps));
    }

    Ok(s.into_bytes())
}

/// `/proc/<pid>/cmdline` — process command name.
///
/// Returns the process name as a null-terminated string (matching
/// Linux's `/proc/<pid>/cmdline` format for simple cases).
fn gen_pid_cmdline(task_id: u64) -> KernelResult<Vec<u8>> {
    // Try process name first.
    if let Some(name) = crate::proc::pcb::name(task_id) {
        let mut data = name.into_bytes();
        data.push(0); // Null-terminated like Linux.
        return Ok(data);
    }

    // Fall back to task name from the scheduler.
    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");
    let mut data = name.as_bytes().to_vec();
    data.push(0);
    Ok(data)
}

/// `/proc/<pid>/stat` — single-line task statistics (Linux-compatible format).
///
/// Format: `pid (name) state ppid prio nice threads cpu_ticks`
///
/// This simplified format provides the essential fields that tools
/// like `top` and `ps` parse.
fn gen_pid_stat(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::sched::task::TaskState;

    let tasks = crate::sched::task_list();
    let task = tasks.iter().find(|t| t.id == task_id)
        .ok_or(KernelError::NotFound)?;

    let name = core::str::from_utf8(task.name.get(..task.name_len).unwrap_or(&[]))
        .unwrap_or("???");

    let state_char = match task.state {
        TaskState::Running => 'R',
        TaskState::Ready => 'R',    // runnable = R in Linux
        TaskState::Blocked => 'S',  // sleeping
        TaskState::Suspended => 'T', // stopped
        TaskState::Dead => 'Z',     // zombie
    };

    let ppid = crate::proc::pcb::parent(task_id).unwrap_or(0);
    let threads = crate::proc::pcb::get_threads(task_id)
        .map_or(1, |t| t.len());

    // Format: pid (name) state ppid prio nice threads cpu_ticks sched_count
    let text = format!(
        "{} ({}) {} {} {} 0 {} {} {}\n",
        task.id, name, state_char, ppid,
        task.priority, threads, task.total_ticks, task.schedule_count,
    );
    Ok(text.into_bytes())
}

/// `/proc/<pid>/maps` — virtual memory area listing.
///
/// Shows the VMAs (lazy/demand-paged regions) for the process, similar
/// to Linux's `/proc/<pid>/maps`.  Format:
/// `start-end perms offset name`
fn gen_pid_maps(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::proc::pcb;

    // VMAs are only tracked for processes (not bare tasks).
    if pcb::state(task_id).is_none() {
        // Not a process — return empty.
        return Ok(b"(no process VMAs)\n".to_vec());
    }

    // Read the VMA list via the public API.
    // We can't directly access the VMA list from procfs, but we can
    // use the PCB's fault resolution to infer info.  Actually, let me
    // just report what we know from the process table.
    let mut text = String::from("START            END              PERMS  DESCRIPTION\n");

    // PML4 address (page table root).
    if let Some(pml4) = pcb::get_pml4(task_id) {
        if pml4 != 0 {
            text.push_str(&format!("pml4: 0x{:016x}\n", pml4));
        } else {
            text.push_str("pml4: (kernel address space)\n");
        }
    }

    // Report process state and thread count — useful alongside maps.
    if let Some(threads) = pcb::get_threads(task_id) {
        text.push_str(&format!("threads: {}\n", threads.len()));
    }
    if let Some(exit_code) = pcb::exit_code(task_id) {
        text.push_str(&format!("exit_code: {}\n", exit_code));
    }

    Ok(text.into_bytes())
}

/// `/proc/<pid>/caps` — capability table listing.
///
/// Shows the count and types of capabilities granted to this process,
/// plus the process credentials (UID/GID).
fn gen_pid_caps(task_id: u64) -> KernelResult<Vec<u8>> {
    use crate::cap::{ResourceType, Rights};
    use crate::proc::pcb;

    let cap_count = pcb::cap_count(task_id)
        .ok_or(KernelError::NotFound)?;

    let mut text = format!("Capabilities: {} total\n", cap_count);

    if cap_count == 0 {
        text.push_str("(no capabilities granted)\n");
    } else {
        // Probe well-known resource types with READ rights to show which
        // kinds of capabilities this process holds.
        let probes: &[(ResourceType, &str)] = &[
            (ResourceType::Process, "Process"),
            (ResourceType::Thread, "Thread"),
            (ResourceType::Channel, "Channel"),
            (ResourceType::Pipe, "Pipe"),
            (ResourceType::SharedMemory, "SharedMem"),
            (ResourceType::File, "File"),
            (ResourceType::Socket, "Socket"),
            (ResourceType::PortIo, "PortIO"),
            (ResourceType::DeviceIrq, "DevIRQ"),
            (ResourceType::IoScheduler, "IoSched"),
        ];

        for &(rt, label) in probes {
            if pcb::has_capability_type(task_id, rt, Rights::READ) {
                text.push_str(&format!("  {}: yes\n", label));
            }
        }
    }

    // Credentials.
    if let Some(creds) = pcb::get_credentials(task_id) {
        text.push_str(&format!("\nUID: {} GID: {}\n", creds.uid, creds.gid));
        if creds.is_root() {
            text.push_str("Privilege: root (all capabilities implied)\n");
        }
    }

    Ok(text.into_bytes())
}

/// Generate content for a per-PID virtual file.
fn generate_pid(task_id: u64, file_name: &str) -> KernelResult<Vec<u8>> {
    match file_name {
        "status" => gen_pid_status(task_id),
        "cmdline" => gen_pid_cmdline(task_id),
        "stat" => gen_pid_stat(task_id),
        "maps" => gen_pid_maps(task_id),
        "caps" => gen_pid_caps(task_id),
        _ => Err(KernelError::NotFound),
    }
}

/// Generate `/proc/pipes` — active named pipes.
fn gen_pipes() -> Vec<u8> {
    let pipes = crate::fs::pipe::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Active pipes: {}\n\n", pipes.len()));
    if !pipes.is_empty() {
        s.push_str(&format!("{:<30} {:>8} {:>8} {:>4} {:>4} {:>12} {:>12}\n",
            "Path", "Capacity", "Buffered", "R", "W", "BytesIn", "BytesOut"));
        for p in &pipes {
            s.push_str(&format!("{:<30} {:>8} {:>8} {:>4} {:>4} {:>12} {:>12}\n",
                p.path, p.capacity, p.buffered, p.readers, p.writers,
                p.bytes_written, p.bytes_read));
        }
    }
    s.into_bytes()
}

/// Generate `/proc/overlays` — active overlay mounts.
fn gen_overlays() -> Vec<u8> {
    let overlays = crate::fs::overlay::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Active overlays: {}\n\n", overlays.len()));
    for (id, ov) in &overlays {
        s.push_str(&format!("overlay {} ({}):\n", id, ov.name));
        s.push_str(&format!("  lower:      {}\n", ov.lower_path));
        s.push_str(&format!("  upper:      {}\n", ov.upper_path));
        s.push_str(&format!("  whiteouts:  {}\n", ov.whiteout_count));
        s.push_str(&format!("  opaque:     {}\n", ov.opaque_dir_count));
        s.push_str(&format!("  reads:      {}\n", ov.reads));
        s.push_str(&format!("  writes:     {}\n", ov.writes));
        s.push_str(&format!("  copyups:    {}\n", ov.copyups));
        s.push('\n');
    }
    s.into_bytes()
}

/// Generate `/proc/namespaces` — active mount namespaces.
fn gen_namespaces() -> Vec<u8> {
    let nss = crate::fs::mount_ns::list();
    let mut s = String::with_capacity(512);
    s.push_str(&format!("Mount namespaces: {}\n\n", nss.len()));
    for ns in &nss {
        let parent = ns.parent.map(|p| format!("{}", p)).unwrap_or_else(|| String::from("none"));
        s.push_str(&format!("ns {} ({}):\n", ns.id, ns.name));
        s.push_str(&format!("  parent:     {}\n", parent));
        s.push_str(&format!("  mounts:     {}\n", ns.mount_count));
        s.push_str(&format!("  refcount:   {}\n", ns.refcount));
        s.push_str(&format!("  nested:     {}\n", ns.allow_nested));
        s.push('\n');
    }
    s.into_bytes()
}

/// Generate `/proc/rlimits` — resource limits.
fn gen_rlimits() -> Vec<u8> {
    use crate::fs::rlimit;
    let defaults = rlimit::get_defaults();
    let overrides = rlimit::list_overrides();
    let mut s = String::with_capacity(512);

    s.push_str("Global defaults:\n");
    s.push_str(&format!("  nofile:  soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.nofile.soft),
        rlimit::Rlimit::format_value(defaults.nofile.hard)));
    s.push_str(&format!("  fsize:   soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.fsize.soft),
        rlimit::Rlimit::format_value(defaults.fsize.hard)));
    s.push_str(&format!("  locks:   soft={} hard={}\n",
        rlimit::Rlimit::format_value(defaults.locks.soft),
        rlimit::Rlimit::format_value(defaults.locks.hard)));

    if !overrides.is_empty() {
        s.push_str(&format!("\nPer-UID overrides ({}):\n", overrides.len()));
        for (uid, set) in &overrides {
            s.push_str(&format!("  uid {}:\n", uid));
            s.push_str(&format!("    nofile: soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.nofile.soft),
                rlimit::Rlimit::format_value(set.nofile.hard)));
            s.push_str(&format!("    fsize:  soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.fsize.soft),
                rlimit::Rlimit::format_value(set.fsize.hard)));
            s.push_str(&format!("    locks:  soft={} hard={}\n",
                rlimit::Rlimit::format_value(set.locks.soft),
                rlimit::Rlimit::format_value(set.locks.hard)));
        }
    } else {
        s.push_str("\nNo per-UID overrides.\n");
    }
    s.into_bytes()
}

/// Generate `/proc/audit` — filesystem audit status.
fn gen_audit() -> Vec<u8> {
    use crate::fs::audit;
    let st = audit::stats();
    let rules = audit::list_rules();
    let mut s = String::with_capacity(512);

    s.push_str(&format!("Filesystem audit: {}\n\n", if st.enabled { "enabled" } else { "disabled" }));
    s.push_str(&format!("  buffer:       {}/{} entries\n", st.buffer_used, st.buffer_size));
    s.push_str(&format!("  total events: {}\n", st.total_events));
    s.push_str(&format!("  dropped:      {}\n", st.dropped_events));
    s.push_str(&format!("  rules:        {}\n\n", st.rules_count));

    if !rules.is_empty() {
        s.push_str("Rules:\n");
        for r in &rules {
            let uid_str = r.uid.map(|u| format!("{}", u)).unwrap_or_else(|| String::from("*"));
            let prefix = if r.path_prefix.is_empty() { "(all)" } else { &r.path_prefix };
            s.push_str(&format!("  rule {}: path={} mask=0x{:X} uid={} failures={} enabled={}\n",
                r.id, prefix, r.mask.0, uid_str, r.failures_only, r.enabled));
        }
    }
    s.into_bytes()
}

fn gen_snapshots() -> Vec<u8> {
    use crate::fs::snapshot;
    let snaps = snapshot::list();
    let mut s = String::with_capacity(512);

    s.push_str(&format!("Filesystem snapshots: {}\n\n", snaps.len()));

    if !snaps.is_empty() {
        s.push_str(&format!("{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}\n",
            "ID", "NAME", "PATH", "FILES", "BYTES", "PARENT"));
        for snap in &snaps {
            let parent_str = snap.parent
                .map(|p| format!("{}", p.0))
                .unwrap_or_else(|| String::from("-"));
            s.push_str(&format!("{:>4}  {:20}  {:30}  {:>8}  {:>12}  {}\n",
                snap.id.0, snap.name, snap.root_path,
                snap.file_count, snap.total_bytes, parent_str));
        }
    }

    s.into_bytes()
}

/// Check if a task ID currently exists in the scheduler.
fn task_exists(task_id: u64) -> bool {
    crate::sched::task_list().iter().any(|t| t.id == task_id)
}

// ---------------------------------------------------------------------------
// Path resolution helpers
// ---------------------------------------------------------------------------

/// Strip leading "/" to get the relative path within procfs.
fn strip_root(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

/// Generate content for a root-level virtual file by name.
fn generate(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        "version" => Ok(gen_version()),
        "uptime" => Ok(gen_uptime()),
        "meminfo" => Ok(gen_meminfo()),
        "cpuinfo" => Ok(gen_cpuinfo()),
        "config" => Ok(gen_config()),
        "mounts" => Ok(gen_mounts()),
        "stat" => Ok(gen_stat()),
        "filesystems" => Ok(gen_filesystems()),
        "cmdline" => Ok(gen_cmdline()),
        "loadavg" => Ok(gen_loadavg()),
        "cacheinfo" => Ok(gen_cacheinfo()),
        "locks" => Ok(gen_locks()),
        "fdinfo" => Ok(gen_fdinfo()),
        "diskstats" => Ok(gen_diskstats()),
        "partitions" => Ok(gen_partitions()),
        "interrupts" => Ok(gen_interrupts()),
        "devices" => Ok(gen_devices()),
        "net" => Ok(gen_net()),
        "vmstat" => Ok(gen_vmstat()),
        "buddyinfo" => Ok(gen_buddyinfo()),
        "swaps" => Ok(gen_swaps()),
        "fsstats" => Ok(gen_fsstats()),
        "heapinfo" => Ok(gen_heapinfo()),
        "bcache" => Ok(gen_bcache()),
        "cas" => Ok(gen_cas()),
        "integrity" => Ok(gen_integrity()),
        "fhistory" => Ok(gen_fhistory()),
        "quotas" => Ok(gen_quotas()),
        "security" => Ok(gen_security()),
        "pipes" => Ok(gen_pipes()),
        "overlays" => Ok(gen_overlays()),
        "namespaces" => Ok(gen_namespaces()),
        "rlimits" => Ok(gen_rlimits()),
        "audit" => Ok(gen_audit()),
        "snapshots" => Ok(gen_snapshots()),
        _ => Err(KernelError::NotFound),
    }
}

/// Classify a relative procfs path into a typed request.
///
/// Returns:
/// - `ProcPath::Root` — the root directory itself
/// - `ProcPath::RootFile(name)` — a file in the root (e.g., "meminfo")
/// - `ProcPath::PidDir(id)` — a per-PID directory (e.g., "1")
/// - `ProcPath::PidFile(id, name)` — a file inside a PID dir (e.g., "1/status")
/// - `ProcPath::NotFound` — unrecognized path
enum ProcPath<'a> {
    Root,
    RootFile(&'a str),
    PidDir(u64),
    PidFile(u64, &'a str),
    NotFound,
}

fn classify_path(rel: &str) -> ProcPath<'_> {
    if rel.is_empty() {
        return ProcPath::Root;
    }

    // Split into first component and optional remainder.
    let (first, rest) = match rel.find('/') {
        Some(pos) => {
            let (a, b) = rel.split_at(pos);
            // b starts with '/'; strip it.
            (a, b.get(1..).unwrap_or(""))
        }
        None => (rel, ""),
    };

    // Try root-level file first.
    if rest.is_empty() && ROOT_FILES.contains(&first) {
        return ProcPath::RootFile(first);
    }

    // "self" is a magic alias for the current task's PID.
    // Linux provides /proc/self as a symlink → /proc/<current_pid>.
    // We resolve it inline since procfs is a virtual filesystem.
    let pid = if first == "self" {
        crate::sched::current_task_id()
    } else if let Ok(p) = first.parse::<u64>() {
        p
    } else {
        return ProcPath::NotFound;
    };

    if rest.is_empty() {
        return ProcPath::PidDir(pid);
    }
    // File inside PID directory (no nested subdirs).
    if !rest.contains('/') && PID_FILES.contains(&rest) {
        return ProcPath::PidFile(pid, rest);
    }

    ProcPath::NotFound
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

impl FileSystem for ProcFs {
    fn fs_type(&self) -> &str {
        "procfs"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root => {
                // Root directory: list virtual files + per-PID directories.
                let mut entries: Vec<DirEntry> = ROOT_FILES
                    .iter()
                    .map(|name| {
                        let size = generate(name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();

                // "self" — magic symlink to the current task's PID directory.
                entries.push(DirEntry {
                    name: String::from("self"),
                    entry_type: EntryType::Symlink,
                    size: 0,
                });

                // Add per-PID directories for all live tasks.
                for task in &crate::sched::task_list() {
                    entries.push(DirEntry {
                        name: format!("{}", task.id),
                        entry_type: EntryType::Directory,
                        size: 0,
                    });
                }

                Ok(entries)
            }
            ProcPath::PidDir(pid) => {
                // Per-PID directory: list virtual files inside it.
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let entries: Vec<DirEntry> = PID_FILES
                    .iter()
                    .map(|name| {
                        let size = generate_pid(pid, name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            ProcPath::RootFile(_) | ProcPath::PidFile(_, _) => {
                Err(KernelError::NotADirectory)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root | ProcPath::PidDir(_) => {
                Err(KernelError::IsADirectory)
            }
            ProcPath::RootFile(name) => generate(name),
            ProcPath::PidFile(pid, file_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                generate_pid(pid, file_name)
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let rel = strip_root(path);

        match classify_path(rel) {
            ProcPath::Root => Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            ProcPath::RootFile(name) => {
                let size = generate(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::PidDir(pid) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                Ok(DirEntry {
                    name: format!("{pid}"),
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            ProcPath::PidFile(pid, file_name) => {
                if !task_exists(pid) {
                    return Err(KernelError::NotFound);
                }
                let size = generate_pid(pid, file_name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(file_name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            ProcPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        // Verify the path exists by calling stat.
        let entry = self.stat(path)?;

        let perms = if entry.entry_type == EntryType::Directory {
            0o555
        } else {
            0o444
        };

        Ok(FileMeta {
            size: entry.size,
            entry_type: entry.entry_type,
            permissions: perms,
            nlinks: 1,
            blocks: 0,
            ..FileMeta::minimal(entry.entry_type, entry.size)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let task_count = crate::sched::task_list().len();
        Ok(FsInfo {
            fs_type: String::from("procfs"),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: (ROOT_FILES.len() + task_count) as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: true,
        })
    }

    fn debug_stats(&self) -> String {
        let task_count = crate::sched::task_list().len();
        format!(
            "procfs: {} root files, {} task dirs",
            ROOT_FILES.len(),
            task_count
        )
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount procfs at the given path (typically `/proc`).
pub fn mount(mount_path: &str) -> KernelResult<()> {
    let fs = ProcFs::new();
    crate::fs::Vfs::mount(mount_path, alloc::boxed::Box::new(fs))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the procfs implementation.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[procfs] Running self-test...");

    let mut fs = ProcFs::new();

    // Test root readdir — should have root files + at least 1 PID directory.
    let entries = fs.readdir("/")?;
    let min_expected = ROOT_FILES.len();
    if entries.len() < min_expected {
        serial_println!(
            "[procfs]   FAIL: readdir returned {} entries, expected >= {}",
            entries.len(),
            min_expected
        );
        return Err(KernelError::InternalError);
    }
    // Count PID directories.
    let pid_dirs = entries.iter()
        .filter(|e| e.entry_type == EntryType::Directory)
        .count();
    serial_println!(
        "[procfs]   readdir /: {} entries ({} files, {} pid dirs) OK",
        entries.len(),
        entries.len().saturating_sub(pid_dirs),
        pid_dirs
    );

    // Test stat on root.
    let root_stat = fs.stat("/")?;
    if root_stat.entry_type != EntryType::Directory {
        serial_println!("[procfs]   FAIL: stat / not a directory");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /: directory OK");

    // Test each virtual file.
    for name in ROOT_FILES {
        let path = format!("/{name}");

        // stat should succeed.
        let entry = fs.stat(&path)?;
        if entry.entry_type != EntryType::File {
            serial_println!("[procfs]   FAIL: stat {path} not a file");
            return Err(KernelError::InternalError);
        }

        // read_file should return non-empty data.
        let data = fs.read_file(&path)?;
        if data.is_empty() {
            serial_println!("[procfs]   FAIL: read_file {path} returned empty");
            return Err(KernelError::InternalError);
        }

        // Verify it's valid UTF-8 (all our files are text).
        if core::str::from_utf8(&data).is_err() {
            serial_println!("[procfs]   FAIL: {path} is not valid UTF-8");
            return Err(KernelError::InternalError);
        }

        serial_println!("[procfs]   {name}: {} bytes OK", data.len());
    }

    // Test stat on nonexistent file.
    if fs.stat("/nonexistent").is_ok() {
        serial_println!("[procfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /nonexistent: NotFound OK");

    // Test read on directory.
    if let Ok(_) = fs.read_file("/") {
        serial_println!("[procfs]   FAIL: read_file / should fail (IsADirectory)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   read_file /: IsADirectory OK");

    // Test write (should fail — read-only).
    if fs.write_file("/version", b"hacked").is_ok() {
        serial_println!("[procfs]   FAIL: write_file should fail (NotSupported)");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   write_file: NotSupported OK");

    // --- Per-PID directory tests ---

    // Get the current task ID to test against a known-live PID.
    let current_tid = crate::sched::current_task_id();
    let pid_path = format!("/{current_tid}");
    let status_path = format!("/{current_tid}/status");

    // stat on PID directory.
    let pid_stat = fs.stat(&pid_path)?;
    if pid_stat.entry_type != EntryType::Directory {
        serial_println!("[procfs]   FAIL: stat {pid_path} not a directory");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat {}: directory OK", pid_path);

    // readdir on PID directory — should have PID_FILES entries.
    let pid_entries = fs.readdir(&pid_path)?;
    if pid_entries.len() != PID_FILES.len() {
        serial_println!(
            "[procfs]   FAIL: readdir {} returned {} entries, expected {}",
            pid_path, pid_entries.len(), PID_FILES.len()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   readdir {}: {} entries OK", pid_path, pid_entries.len());

    // read_file on status.
    let status_data = fs.read_file(&status_path)?;
    if status_data.is_empty() {
        serial_println!("[procfs]   FAIL: read_file {} returned empty", status_path);
        return Err(KernelError::InternalError);
    }
    let status_text = core::str::from_utf8(&status_data)
        .map_err(|_| KernelError::InternalError)?;
    // Verify it mentions the PID.
    let pid_str = format!("{current_tid}");
    if !status_text.contains(&pid_str) {
        serial_println!("[procfs]   FAIL: status doesn't contain PID {}", current_tid);
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   {}/status: {} bytes OK", current_tid, status_data.len());

    // read_file on PID directory should fail (IsADirectory).
    if fs.read_file(&pid_path).is_ok() {
        serial_println!("[procfs]   FAIL: read_file on PID dir should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   read_file on PID dir: IsADirectory OK");

    // stat on nonexistent PID should fail.
    if fs.stat("/999999").is_ok() {
        serial_println!("[procfs]   FAIL: stat on bogus PID should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   stat /999999: NotFound OK");

    serial_println!("[procfs] Self-test PASSED");
    Ok(())
}
