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
//! ├── mounts         Mounted filesystems
//! ├── stat           System-wide scheduler statistics
//! └── <pid>/         Per-process directories (future)
//!     ├── status     Process name, state, priority
//!     └── ...
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
use crate::fs::vfs::{DirEntry, EntryType, FileSystem};

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
    "mounts",
    "stat",
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

/// `/proc/cpuinfo` — CPU topology.
fn gen_cpuinfo() -> Vec<u8> {
    let count = crate::acpi::processor_count();
    let processors = crate::acpi::processors();

    let mut s = String::with_capacity(256);
    s.push_str(&format!("processors: {count}\n\n"));

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

/// `/proc/mounts` — mounted filesystems.
///
/// Format: `<mount_path> <fs_type>` per line (similar to Linux `/proc/mounts`
/// but simplified — we don't have mount options yet).
fn gen_mounts() -> Vec<u8> {
    let mounts = crate::fs::Vfs::mounts();
    let mut s = String::with_capacity(128);

    for (path, fs_type) in &mounts {
        s.push_str(&format!("{path} {fs_type}\n"));
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

// ---------------------------------------------------------------------------
// Path resolution helpers
// ---------------------------------------------------------------------------

/// Strip leading "/" to get the relative path within procfs.
fn strip_root(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

/// Generate content for a virtual file by name.
fn generate(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        "version" => Ok(gen_version()),
        "uptime" => Ok(gen_uptime()),
        "meminfo" => Ok(gen_meminfo()),
        "cpuinfo" => Ok(gen_cpuinfo()),
        "mounts" => Ok(gen_mounts()),
        "stat" => Ok(gen_stat()),
        _ => Err(KernelError::NotFound),
    }
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

        if rel.is_empty() {
            // Root directory: list all virtual files.
            let entries: Vec<DirEntry> = ROOT_FILES
                .iter()
                .map(|name| {
                    // Generate each file to get its actual size.
                    let size = generate(name).map_or(0, |d| d.len() as u64);
                    DirEntry {
                        name: String::from(*name),
                        entry_type: EntryType::File,
                        size,
                    }
                })
                .collect();
            Ok(entries)
        } else {
            // No subdirectories yet (per-PID dirs are future work).
            Err(KernelError::NotADirectory)
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path);

        if rel.is_empty() {
            return Err(KernelError::IsADirectory);
        }

        generate(rel)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let rel = strip_root(path);

        if rel.is_empty() {
            // Root directory.
            return Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        // Check if the name is a known virtual file.
        if ROOT_FILES.contains(&rel) {
            let size = generate(rel).map_or(0, |d| d.len() as u64);
            Ok(DirEntry {
                name: String::from(rel),
                entry_type: EntryType::File,
                size,
            })
        } else {
            Err(KernelError::NotFound)
        }
    }

    fn debug_stats(&self) -> String {
        format!("procfs: {} virtual files", ROOT_FILES.len())
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

    // Test root readdir.
    let entries = fs.readdir("/")?;
    if entries.len() != ROOT_FILES.len() {
        serial_println!(
            "[procfs]   FAIL: readdir returned {} entries, expected {}",
            entries.len(),
            ROOT_FILES.len()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[procfs]   readdir /: {} entries OK", entries.len());

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

    serial_println!("[procfs] Self-test PASSED");
    Ok(())
}
