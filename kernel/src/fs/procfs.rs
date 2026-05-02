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
//! ├── filesystems    Available filesystem types
//! ├── cmdline        Kernel command line
//! ├── loadavg        Instantaneous system load
//! └── <pid>/         Per-process directories
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
    "mounts",
    "stat",
    "filesystems",
    "cmdline",
    "loadavg",
    "cacheinfo",
    "locks",
    "fdinfo",
];

/// Names of virtual files inside each `/proc/<pid>/` directory.
const PID_FILES: &[&str] = &[
    "status",
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

    let text = format!(
        "reads:        {}\n\
         hits:         {}\n\
         misses:       {}\n\
         hit_rate:     {:.1}%\n\
         writes:       {}\n\
         writebacks:   {}\n\
         readaheads:   {}\n\
         entries_used: {}/{}\n\
         entries_dirty:{}\n",
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

/// `/proc/<pid>/status` — per-task status information.
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

    let mut s = String::with_capacity(256);
    s.push_str(&format!("Name:     {name}\n"));
    s.push_str(&format!("Pid:      {}\n", task.id));
    s.push_str(&format!("State:    {state_str}\n"));
    s.push_str(&format!("Priority: {}\n", task.priority));
    s.push_str(&format!("CpuTime:  {cpu_ms} ms\n"));
    s.push_str(&format!("Scheduled:{}\n", task.schedule_count));
    s.push_str(&format!("LastCpu:  {}\n", task.last_cpu));

    Ok(s.into_bytes())
}

/// Generate content for a per-PID virtual file.
fn generate_pid(task_id: u64, file_name: &str) -> KernelResult<Vec<u8>> {
    match file_name {
        "status" => gen_pid_status(task_id),
        _ => Err(KernelError::NotFound),
    }
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
        "mounts" => Ok(gen_mounts()),
        "stat" => Ok(gen_stat()),
        "filesystems" => Ok(gen_filesystems()),
        "cmdline" => Ok(gen_cmdline()),
        "loadavg" => Ok(gen_loadavg()),
        "cacheinfo" => Ok(gen_cacheinfo()),
        "locks" => Ok(gen_locks()),
        "fdinfo" => Ok(gen_fdinfo()),
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

    // Try numeric PID directory.
    if let Ok(pid) = first.parse::<u64>() {
        if rest.is_empty() {
            return ProcPath::PidDir(pid);
        }
        // File inside PID directory (no nested subdirs).
        if !rest.contains('/') && PID_FILES.contains(&rest) {
            return ProcPath::PidFile(pid, rest);
        }
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
            ..FileMeta::minimal(entry.entry_type, entry.size)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let task_count = crate::sched::task_list().len();
        Ok(FsInfo {
            fs_type: String::from("procfs"),
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
