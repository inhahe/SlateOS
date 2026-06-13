//! System information pseudo-filesystem (`/sys`).
//!
//! Exposes kernel tunables, hardware information, and system state
//! through a read/write virtual filesystem.  Unlike procfs (which is
//! read-only), sysfs allows writing to tune kernel parameters at runtime.
//!
//! ## Layout
//!
//! ```text
//! /sys/
//! ├── kernel/
//! │   ├── version          Kernel version string (read-only)
//! │   ├── ostype           OS type identifier (read-only)
//! │   ├── osrelease        OS release string (read-only)
//! │   ├── hostname         System hostname (read/write)
//! │   └── ticks_per_sec    Timer tick rate (read-only)
//! ├── params/
//! │   ├── <name>           Sysctl parameters — one file per param (read/write)
//! │   └── ...              Values are decimal u64 strings
//! ├── devices/
//! │   ├── pci/
//! │   │   ├── BB:DD.F      PCI device info per BDF address
//! │   │   └── ...
//! │   └── system/
//! │       └── cpu/
//! │           ├── online    Online CPU range, e.g. "0-7" (read-only)
//! │           ├── present   Present (populated) CPU range (read-only)
//! │           ├── possible  Possible CPU range (read-only)
//! │           ├── kernel_max Highest addressable CPU index (read-only)
//! │           └── cpuN/                 One per present CPU
//! │               └── topology/
//! │                   ├── physical_package_id   Socket id (read-only)
//! │                   ├── core_id               Core id within socket
//! │                   ├── core_siblings[_list]  Threads in same socket
//! │                   └── thread_siblings[_list] Threads in same core
//! └── fs/
//!     ├── cache_sectors    Buffer cache capacity (read-only)
//!     ├── cache_stats      Buffer cache hit/miss stats (read-only)
//!     └── mount_count      Number of mounted filesystems (read-only)
//! ```
//!
//! ## Design
//!
//! Content is generated dynamically on read.  Writes to parameter files
//! under `/sys/params/` call through to `sysctl::set_by_name()`.  This
//! provides a filesystem-based alternative to the `SYS_SYSCTL_SET` syscall,
//! which is convenient for shell scripts and interactive tuning.
//!
//! The hostname is stored in this module (not via sysctl) since it's
//! a string, not a u64 — it doesn't fit the sysctl integer model.

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};

use spin::Mutex;

// ---------------------------------------------------------------------------
// Hostname state
// ---------------------------------------------------------------------------

/// System hostname.  Defaults to "mintos" until changed by writing
/// to `/sys/kernel/hostname`.
static HOSTNAME: Mutex<String> = Mutex::new(String::new());

/// Get the current hostname.
fn hostname() -> String {
    let h = HOSTNAME.lock();
    if h.is_empty() {
        String::from("mintos")
    } else {
        h.clone()
    }
}

/// Set the hostname.  Max 253 characters (DNS limit).
fn set_hostname(name: &str) -> KernelResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 253 {
        return Err(KernelError::InvalidArgument);
    }
    let mut h = HOSTNAME.lock();
    h.clear();
    h.push_str(trimmed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Path classification
// ---------------------------------------------------------------------------

/// Classified sysfs path.
enum SysPath<'a> {
    /// Root directory: /sys/
    Root,
    /// A subdirectory: /sys/kernel/, /sys/params/, etc.
    SubDir(&'a str),
    /// File in kernel/ subdir: /sys/kernel/version etc.
    KernelFile(&'a str),
    /// File in params/ subdir: /sys/params/mm.swappiness etc.
    ParamFile(&'a str),
    /// The devices/ directory.
    DevicesDir,
    /// The devices/pci/ directory.
    PciDir,
    /// A PCI device file: /sys/devices/pci/00:01.0
    PciDevice(&'a str),
    /// The devices/system/ directory.
    SystemDir,
    /// The devices/system/cpu/ directory.
    SystemCpuDir,
    /// A CPU range file: /sys/devices/system/cpu/online etc.
    CpuFile(&'a str),
    /// A per-CPU directory: /sys/devices/system/cpu/cpuN/
    CpuN(usize),
    /// A per-CPU topology directory: /sys/devices/system/cpu/cpuN/topology/
    CpuNTopologyDir(usize),
    /// A per-CPU topology file: /sys/devices/system/cpu/cpuN/topology/core_id etc.
    CpuTopoFile(usize, &'a str),
    /// File in fs/ subdir: /sys/fs/cache_sectors etc.
    FsFile(&'a str),
    /// Not found.
    NotFound,
}

/// Top-level subdirectories.
const TOP_DIRS: &[&str] = &["kernel", "params", "devices", "fs"];

/// Files in /sys/kernel/.
const KERNEL_FILES: &[&str] = &[
    "version",
    "ostype",
    "osrelease",
    "hostname",
    "ticks_per_sec",
];

/// Files in /sys/fs/.
const FS_FILES: &[&str] = &["cache_sectors", "cache_stats", "mount_count"];

/// CPU mask/range files in /sys/devices/system/cpu/.
///
/// These are the files glibc's `get_nprocs()`/`get_nprocs_conf()`, lscpu,
/// nproc, hwloc, and OpenMP/TBB runtimes consult to size thread pools — they
/// try this authoritative sysfs path *before* falling back to /proc/cpuinfo.
const CPU_FILES: &[&str] = &["online", "present", "possible", "kernel_max"];

/// Per-CPU topology files in /sys/devices/system/cpu/cpuN/topology/.
///
/// hwloc and lscpu read these to reconstruct the socket/core/thread layout:
/// `physical_package_id` (socket), `core_id`, and the sibling maps/lists.
/// Both the hex-mask (`*_siblings`) and CPU-list (`*_siblings_list`) forms are
/// provided because hwloc parses the mask form on older kernels and the list
/// form on newer ones.
const CPU_TOPOLOGY_FILES: &[&str] = &[
    "physical_package_id",
    "core_id",
    "core_siblings",
    "core_siblings_list",
    "thread_siblings",
    "thread_siblings_list",
];

/// Parse a `cpuN` directory name into its index, e.g. `"cpu3"` -> `Some(3)`.
/// Rejects names without the `cpu` prefix, with a non-numeric tail, or with
/// leading zeros (`cpu03`) to match Linux's exact `cpuN` naming.
fn parse_cpu_dir(name: &str) -> Option<usize> {
    let digits = name.strip_prefix("cpu")?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()
}

/// Classify the path tail beneath `/sys/devices/system/cpu/`.  `tail` is the
/// portion after `cpu/` (e.g. `""`, `"online"`, `"cpu0/topology/core_id"`).
fn classify_cpu_tail(tail: &str) -> SysPath<'_> {
    if tail.is_empty() {
        return SysPath::SystemCpuDir;
    }
    let (head, rest) = match tail.find('/') {
        Some(pos) => {
            let (a, b) = tail.split_at(pos);
            (a, b.get(1..).unwrap_or(""))
        }
        None => (tail, ""),
    };
    // A flat range file (online/present/possible/kernel_max).
    if rest.is_empty() && CPU_FILES.contains(&head) {
        return SysPath::CpuFile(head);
    }
    // A per-CPU directory cpuN — must index a present CPU.
    if let Some(idx) = parse_cpu_dir(head) {
        if idx >= crate::acpi::processor_count() {
            return SysPath::NotFound;
        }
        if rest.is_empty() {
            return SysPath::CpuN(idx);
        }
        let (sub, leaf) = match rest.find('/') {
            Some(pos) => {
                let (a, b) = rest.split_at(pos);
                (a, b.get(1..).unwrap_or(""))
            }
            None => (rest, ""),
        };
        if sub == "topology" {
            if leaf.is_empty() {
                return SysPath::CpuNTopologyDir(idx);
            } else if !leaf.contains('/') && CPU_TOPOLOGY_FILES.contains(&leaf) {
                return SysPath::CpuTopoFile(idx, leaf);
            }
        }
        return SysPath::NotFound;
    }
    SysPath::NotFound
}

fn classify_path(rel: &str) -> SysPath<'_> {
    if rel.is_empty() {
        return SysPath::Root;
    }

    let (first, rest) = match rel.find('/') {
        Some(pos) => {
            let (a, b) = rel.split_at(pos);
            (a, b.get(1..).unwrap_or(""))
        }
        None => (rel, ""),
    };

    match first {
        "kernel" => {
            if rest.is_empty() {
                SysPath::SubDir("kernel")
            } else if !rest.contains('/') && KERNEL_FILES.contains(&rest) {
                SysPath::KernelFile(rest)
            } else {
                SysPath::NotFound
            }
        }
        "params" => {
            if rest.is_empty() {
                SysPath::SubDir("params")
            } else if !rest.contains('/') {
                // Any non-empty name could be a parameter.
                SysPath::ParamFile(rest)
            } else {
                SysPath::NotFound
            }
        }
        "devices" => {
            if rest.is_empty() {
                SysPath::DevicesDir
            } else {
                let (second, tail) = match rest.find('/') {
                    Some(pos) => {
                        let (a, b) = rest.split_at(pos);
                        (a, b.get(1..).unwrap_or(""))
                    }
                    None => (rest, ""),
                };
                if second == "pci" {
                    if tail.is_empty() {
                        SysPath::PciDir
                    } else if !tail.contains('/') {
                        SysPath::PciDevice(tail)
                    } else {
                        SysPath::NotFound
                    }
                } else if second == "system" {
                    if tail.is_empty() {
                        SysPath::SystemDir
                    } else {
                        let (third, rest2) = match tail.find('/') {
                            Some(pos) => {
                                let (a, b) = tail.split_at(pos);
                                (a, b.get(1..).unwrap_or(""))
                            }
                            None => (tail, ""),
                        };
                        if third == "cpu" {
                            classify_cpu_tail(rest2)
                        } else {
                            SysPath::NotFound
                        }
                    }
                } else {
                    SysPath::NotFound
                }
            }
        }
        "fs" => {
            if rest.is_empty() {
                SysPath::SubDir("fs")
            } else if !rest.contains('/') && FS_FILES.contains(&rest) {
                SysPath::FsFile(rest)
            } else {
                SysPath::NotFound
            }
        }
        _ => SysPath::NotFound,
    }
}

// ---------------------------------------------------------------------------
// Content generators
// ---------------------------------------------------------------------------

fn gen_kernel_file(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        "version" => Ok(b"0.1.0\n".to_vec()),
        "ostype" => Ok(b"MintOS\n".to_vec()),
        "osrelease" => Ok(b"0.1.0-dev\n".to_vec()),
        "hostname" => {
            let h = hostname();
            Ok(format!("{h}\n").into_bytes())
        }
        "ticks_per_sec" => Ok(b"100\n".to_vec()),
        _ => Err(KernelError::NotFound),
    }
}

fn gen_param_file(name: &str) -> KernelResult<Vec<u8>> {
    match crate::sysctl::find_by_name(name) {
        Some(info) => {
            // Format: "value\n" — simple, machine-readable.
            Ok(format!("{}\n", info.value).into_bytes())
        }
        None => Err(KernelError::NotFound),
    }
}

fn gen_pci_device(bdf: &str) -> KernelResult<Vec<u8>> {
    // bdf is something like "00:01.0" — parse it.
    let devices = crate::pci::scan_bus0();
    for dev in &devices {
        let addr = format!(
            "{:02x}:{:02x}.{}",
            dev.address.bus, dev.address.device, dev.address.function
        );
        if addr == bdf {
            let mut s = String::with_capacity(128);
            s.push_str(&format!("address: {}\n", addr));
            s.push_str(&format!("vendor: {:04x}\n", dev.vendor_id));
            s.push_str(&format!("device: {:04x}\n", dev.device_id));
            s.push_str(&format!("class: {:02x}\n", dev.class));
            s.push_str(&format!("subclass: {:02x}\n", dev.subclass));
            return Ok(s.into_bytes());
        }
    }
    Err(KernelError::NotFound)
}

/// Format a contiguous CPU range the way Linux does: `0` for a single CPU,
/// `0-N` for N+1 CPUs.  CPUs are numbered contiguously from 0 in our model.
fn cpu_range(count: usize) -> String {
    let n = count.max(1);
    if n == 1 {
        String::from("0\n")
    } else {
        format!("0-{}\n", n.saturating_sub(1))
    }
}

fn gen_cpu_file(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        // CPUs currently online and schedulable (post-SMP-bringup).
        "online" => Ok(cpu_range(crate::smp::cpu_count()).into_bytes()),
        // CPUs present (populated) — enabled entries in the ACPI MADT.
        // We don't model hot-plug slots beyond the MADT, so possible ==
        // present (correct for non-hotplug hardware; never over-reported).
        "present" | "possible" => {
            Ok(cpu_range(crate::acpi::processor_count()).into_bytes())
        }
        // Highest CPU index the kernel can address (NR_CPUS - 1 in Linux).
        "kernel_max" => {
            let max = crate::sched::priority_rr::MAX_CPUS.saturating_sub(1);
            Ok(format!("{max}\n").into_bytes())
        }
        _ => Err(KernelError::NotFound),
    }
}

/// Format a set of CPU ids as a Linux CPU list (`0-2,4`), compressing
/// contiguous runs into ranges.  Input need not be sorted; duplicates are
/// collapsed.  Always terminated with a newline.
fn fmt_cpu_list(cpus: &[u32]) -> String {
    let mut ids: Vec<u32> = cpus.to_vec();
    ids.sort_unstable();
    ids.dedup();

    let mut out = String::new();
    // Current contiguous run [start, end]; flushed when the run breaks.
    let mut run: Option<(u32, u32)> = None;
    let flush = |out: &mut String, start: u32, end: u32| {
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&format!("{start}"));
        } else {
            out.push_str(&format!("{start}-{end}"));
        }
    };
    for &id in &ids {
        match run {
            // Extend the run when `id` immediately follows `end`.
            Some((start, end)) if id == end.saturating_add(1) => {
                run = Some((start, id));
            }
            Some((start, end)) => {
                flush(&mut out, start, end);
                run = Some((id, id));
            }
            None => run = Some((id, id)),
        }
    }
    if let Some((start, end)) = run {
        flush(&mut out, start, end);
    }
    out.push('\n');
    out
}

/// Format a set of CPU ids as a Linux hex CPU mask.  Our `MAX_CPUS` is 16, so
/// the mask always fits in a single 32-bit word, rendered zero-padded to eight
/// hex digits the way hwloc expects.  Always terminated with a newline.
fn fmt_cpu_mask(cpus: &[u32]) -> String {
    let mut mask: u32 = 0;
    for &c in cpus {
        if c < 32 {
            mask |= 1u32 << c;
        }
    }
    format!("{mask:08x}\n")
}

/// Generate a per-CPU topology file.  Topology is sourced from the real,
/// already-detected layout in `cputopo`; if detection has not populated an
/// entry we fall back to a single-thread core in package 0 (the same honest
/// "we don't know better" layout `cputopo::init_defaults` itself uses), never
/// fabricated data.
fn gen_cpu_topo_file(cpu_idx: usize, name: &str) -> KernelResult<Vec<u8>> {
    // Idempotent: populates the snapshot from cpu_topology on first call.
    crate::fs::cputopo::init_defaults();

    let id = cpu_idx as u32;
    let cpu = crate::fs::cputopo::get_cpu(id);
    let package_id = cpu.as_ref().map_or(0, |c| c.package_id);
    let core_id = cpu.as_ref().map_or(id, |c| c.core_id);

    let bytes = match name {
        "physical_package_id" => format!("{package_id}\n").into_bytes(),
        "core_id" => format!("{core_id}\n").into_bytes(),
        "core_siblings" | "core_siblings_list" => {
            // All logical CPUs in the same package (socket), including self.
            let mut sibs: Vec<u32> = crate::fs::cputopo::cpus_in_package(package_id)
                .iter()
                .map(|c| c.id)
                .collect();
            if sibs.is_empty() {
                sibs.push(id);
            }
            if name == "core_siblings" {
                fmt_cpu_mask(&sibs).into_bytes()
            } else {
                fmt_cpu_list(&sibs).into_bytes()
            }
        }
        "thread_siblings" | "thread_siblings_list" => {
            // Threads sharing this physical core, including self.
            let mut sibs = crate::fs::cputopo::thread_siblings(id);
            sibs.push(id);
            if name == "thread_siblings" {
                fmt_cpu_mask(&sibs).into_bytes()
            } else {
                fmt_cpu_list(&sibs).into_bytes()
            }
        }
        _ => return Err(KernelError::NotFound),
    };
    Ok(bytes)
}

fn gen_fs_file(name: &str) -> KernelResult<Vec<u8>> {
    match name {
        "cache_sectors" => {
            let stats = crate::fs::cache::stats();
            Ok(format!("{}\n", stats.capacity).into_bytes())
        }
        "cache_stats" => {
            let stats = crate::fs::cache::stats();
            let mut s = String::with_capacity(256);
            s.push_str(&format!("reads: {}\n", stats.reads));
            s.push_str(&format!("hits: {}\n", stats.hits));
            s.push_str(&format!("misses: {}\n", stats.misses));
            s.push_str(&format!("writes: {}\n", stats.writes));
            s.push_str(&format!("writebacks: {}\n", stats.writebacks));
            s.push_str(&format!("readaheads: {}\n", stats.readaheads));
            s.push_str(&format!("entries_used: {}\n", stats.entries_used));
            s.push_str(&format!("entries_dirty: {}\n", stats.entries_dirty));
            s.push_str(&format!("capacity: {}\n", stats.capacity));
            Ok(s.into_bytes())
        }
        "mount_count" => {
            let mounts = crate::fs::Vfs::mounts();
            Ok(format!("{}\n", mounts.len()).into_bytes())
        }
        _ => Err(KernelError::NotFound),
    }
}

// ---------------------------------------------------------------------------
// SysFs struct
// ---------------------------------------------------------------------------

/// Virtual filesystem exposing kernel configuration and hardware info.
pub struct SysFs;

impl SysFs {
    /// Create a new SysFs instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Strip leading "/" from a path to get relative path.
fn strip_root(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

impl FileSystem for SysFs {
    fn fs_type(&self) -> &'static str {
        "sysfs"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            SysPath::Root => {
                // Top-level: kernel/, params/, devices/, fs/.
                let entries = TOP_DIRS
                    .iter()
                    .map(|name| DirEntry {
                        name: String::from(*name),
                        entry_type: EntryType::Directory,
                        size: 0,
                    })
                    .collect();
                Ok(entries)
            }
            SysPath::SubDir("kernel") => {
                let entries = KERNEL_FILES
                    .iter()
                    .map(|name| {
                        let size = gen_kernel_file(name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            SysPath::SubDir("params") => {
                // One file per sysctl parameter.
                let params = crate::sysctl::list_all();
                let entries = params
                    .iter()
                    .map(|p| {
                        let val_str = format!("{}\n", p.value);
                        DirEntry {
                            name: String::from(p.name),
                            entry_type: EntryType::File,
                            size: val_str.len() as u64,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            SysPath::SubDir("fs") => {
                let entries = FS_FILES
                    .iter()
                    .map(|name| {
                        let size = gen_fs_file(name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            SysPath::DevicesDir => {
                // "pci/" (PCI devices) and "system/" (CPU/topology tree).
                Ok(vec![
                    DirEntry {
                        name: String::from("pci"),
                        entry_type: EntryType::Directory,
                        size: 0,
                    },
                    DirEntry {
                        name: String::from("system"),
                        entry_type: EntryType::Directory,
                        size: 0,
                    },
                ])
            }
            SysPath::SystemDir => {
                // Just "cpu/" for now (memory/node trees can follow).
                Ok(vec![DirEntry {
                    name: String::from("cpu"),
                    entry_type: EntryType::Directory,
                    size: 0,
                }])
            }
            SysPath::SystemCpuDir => {
                let mut entries: Vec<DirEntry> = CPU_FILES
                    .iter()
                    .map(|name| {
                        let size = gen_cpu_file(name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                // One cpuN directory per present CPU.
                for i in 0..crate::acpi::processor_count() {
                    entries.push(DirEntry {
                        name: format!("cpu{i}"),
                        entry_type: EntryType::Directory,
                        size: 0,
                    });
                }
                Ok(entries)
            }
            SysPath::CpuN(_) => {
                // Each cpuN exposes a topology/ subdir (cache/ is omitted: no
                // per-CPU cache share-maps are honestly available yet).
                Ok(vec![DirEntry {
                    name: String::from("topology"),
                    entry_type: EntryType::Directory,
                    size: 0,
                }])
            }
            SysPath::CpuNTopologyDir(idx) => {
                let entries = CPU_TOPOLOGY_FILES
                    .iter()
                    .map(|name| {
                        let size =
                            gen_cpu_topo_file(idx, name).map_or(0, |d| d.len() as u64);
                        DirEntry {
                            name: String::from(*name),
                            entry_type: EntryType::File,
                            size,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            SysPath::PciDir => {
                // List all PCI devices as files named by BDF address.
                let devices = crate::pci::scan_bus0();
                let entries = devices
                    .iter()
                    .map(|dev| {
                        let name = format!(
                            "{:02x}:{:02x}.{}",
                            dev.address.bus, dev.address.device, dev.address.function
                        );
                        DirEntry {
                            name,
                            entry_type: EntryType::File,
                            size: 0,
                        }
                    })
                    .collect();
                Ok(entries)
            }
            _ => Err(KernelError::NotADirectory),
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path);

        match classify_path(rel) {
            SysPath::Root
            | SysPath::SubDir(_)
            | SysPath::DevicesDir
            | SysPath::PciDir
            | SysPath::SystemDir
            | SysPath::SystemCpuDir
            | SysPath::CpuN(_)
            | SysPath::CpuNTopologyDir(_) => Err(KernelError::IsADirectory),

            SysPath::KernelFile(name) => gen_kernel_file(name),
            SysPath::ParamFile(name) => gen_param_file(name),
            SysPath::PciDevice(bdf) => gen_pci_device(bdf),
            SysPath::FsFile(name) => gen_fs_file(name),
            SysPath::CpuFile(name) => gen_cpu_file(name),
            SysPath::CpuTopoFile(idx, name) => gen_cpu_topo_file(idx, name),
            SysPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let rel = strip_root(path);

        match classify_path(rel) {
            SysPath::Root => Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::SubDir(name) => Ok(DirEntry {
                name: String::from(name),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::KernelFile(name) => {
                let size = gen_kernel_file(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::FsFile(name) => {
                let size = gen_fs_file(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::DevicesDir => Ok(DirEntry {
                name: String::from("devices"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::PciDir => Ok(DirEntry {
                name: String::from("pci"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::ParamFile(name) => {
                let size = gen_param_file(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::PciDevice(bdf) => {
                let size = gen_pci_device(bdf).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(bdf),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::SystemDir => Ok(DirEntry {
                name: String::from("system"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::SystemCpuDir => Ok(DirEntry {
                name: String::from("cpu"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::CpuFile(name) => {
                let size = gen_cpu_file(name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::CpuN(idx) => Ok(DirEntry {
                name: format!("cpu{idx}"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::CpuNTopologyDir(_) => Ok(DirEntry {
                name: String::from("topology"),
                entry_type: EntryType::Directory,
                size: 0,
            }),
            SysPath::CpuTopoFile(idx, name) => {
                let size = gen_cpu_topo_file(idx, name).map_or(0, |d| d.len() as u64);
                Ok(DirEntry {
                    name: String::from(name),
                    entry_type: EntryType::File,
                    size,
                })
            }
            SysPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let rel = strip_root(path);

        match classify_path(rel) {
            SysPath::KernelFile("hostname") => {
                let text = core::str::from_utf8(data)
                    .map_err(|_| KernelError::InvalidArgument)?;
                set_hostname(text)
            }
            SysPath::ParamFile(name) => {
                // Parse value as decimal u64.
                let text = core::str::from_utf8(data)
                    .map_err(|_| KernelError::InvalidArgument)?;
                let trimmed = text.trim();
                let value: u64 = trimmed
                    .parse()
                    .map_err(|_| KernelError::InvalidArgument)?;
                match crate::sysctl::set_by_name(name, value) {
                    Some(_old) => Ok(()),
                    None => Err(KernelError::InvalidArgument),
                }
            }
            SysPath::KernelFile(_)
            | SysPath::FsFile(_)
            | SysPath::PciDevice(_)
            | SysPath::CpuFile(_)
            | SysPath::CpuTopoFile(_, _) => {
                // Read-only files.
                Err(KernelError::NotSupported)
            }
            SysPath::Root
            | SysPath::SubDir(_)
            | SysPath::DevicesDir
            | SysPath::PciDir
            | SysPath::SystemDir
            | SysPath::SystemCpuDir
            | SysPath::CpuN(_)
            | SysPath::CpuNTopologyDir(_) => Err(KernelError::IsADirectory),
            SysPath::NotFound => Err(KernelError::NotFound),
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let entry = self.stat(path)?;
        let perms = if entry.entry_type == EntryType::Directory {
            0o555
        } else {
            // Writable for param files and hostname, read-only for others.
            let rel = strip_root(path);
            match classify_path(rel) {
                SysPath::ParamFile(_) | SysPath::KernelFile("hostname") => 0o644,
                _ => 0o444,
            }
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
        let param_count = crate::sysctl::count();
        Ok(FsInfo {
            fs_type: String::from("sysfs"),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            // Kernel files + param files + fs files + device entries.
            total_inodes: (KERNEL_FILES.len() + param_count + FS_FILES.len()) as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false, // Some files are writable.
        })
    }

    fn debug_stats(&self) -> String {
        let param_count = crate::sysctl::count();
        format!(
            "sysfs: {} kernel files, {} params, {} fs files",
            KERNEL_FILES.len(),
            param_count,
            FS_FILES.len()
        )
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount sysfs at the given path (typically `/sys`).
pub fn mount(mount_path: &str) -> KernelResult<()> {
    let fs = SysFs::new();
    crate::fs::Vfs::mount(mount_path, alloc::boxed::Box::new(fs))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Kshell integration: `sysctl` command
// ---------------------------------------------------------------------------

/// Get the current hostname for use by other kernel subsystems.
#[must_use]
pub fn get_hostname() -> String {
    hostname()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test sysfs read/write operations.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[sysfs] Running self-test...");

    let mut fs = SysFs::new();

    // 1. Read root directory — should contain our 4 subdirs.
    let root_entries = fs.readdir("/")?;
    assert!(
        root_entries.len() == TOP_DIRS.len(),
        "sysfs root should have {} entries, got {}",
        TOP_DIRS.len(),
        root_entries.len()
    );
    for dir_name in TOP_DIRS {
        assert!(
            root_entries.iter().any(|e| e.name == *dir_name),
            "sysfs root missing '{}'",
            dir_name
        );
    }
    serial_println!("[sysfs]   root directory: OK ({} entries)", root_entries.len());

    // 2. Read kernel files.
    let version = fs.read_file("/kernel/version")?;
    assert!(!version.is_empty(), "kernel/version should not be empty");
    serial_println!("[sysfs]   kernel/version: OK");

    let ostype = fs.read_file("/kernel/ostype")?;
    assert!(
        ostype.starts_with(b"MintOS"),
        "ostype should start with 'MintOS'"
    );
    serial_println!("[sysfs]   kernel/ostype: OK");

    // 3. Hostname read/write.
    let h1 = fs.read_file("/kernel/hostname")?;
    assert!(!h1.is_empty(), "hostname should not be empty");
    serial_println!("[sysfs]   hostname read: OK");

    fs.write_file("/kernel/hostname", b"test-host")?;
    let h2 = fs.read_file("/kernel/hostname")?;
    assert!(
        h2.starts_with(b"test-host"),
        "hostname should be 'test-host' after write"
    );
    serial_println!("[sysfs]   hostname write: OK");

    // Restore default.
    fs.write_file("/kernel/hostname", b"mintos")?;

    // 4. Parameter files.
    let params_dir = fs.readdir("/params")?;
    assert!(
        !params_dir.is_empty(),
        "params dir should have sysctl entries"
    );
    serial_println!("[sysfs]   params directory: OK ({} params)", params_dir.len());

    // Read one known parameter.
    let swappiness = fs.read_file("/params/mm.swappiness");
    if let Ok(data) = swappiness {
        let text = core::str::from_utf8(&data).unwrap_or("?");
        serial_println!("[sysfs]   mm.swappiness: {}", text.trim());
    }

    // 5. Read-only files should reject writes.
    let write_result = fs.write_file("/kernel/version", b"hacked");
    assert!(
        write_result.is_err(),
        "writing to kernel/version should fail"
    );
    serial_println!("[sysfs]   read-only enforcement: OK");

    // 6. Filesystem info files.
    let fs_dir = fs.readdir("/fs")?;
    assert!(
        fs_dir.len() == FS_FILES.len(),
        "fs dir should have {} entries",
        FS_FILES.len()
    );
    serial_println!("[sysfs]   fs directory: OK ({} entries)", fs_dir.len());

    // 7. Devices directory.
    let dev_dir = fs.readdir("/devices")?;
    assert!(
        dev_dir.iter().any(|e| e.name == "pci"),
        "devices dir should contain 'pci'"
    );
    assert!(
        dev_dir.iter().any(|e| e.name == "system"),
        "devices dir should contain 'system'"
    );
    serial_println!("[sysfs]   devices directory: OK");

    // 8. PCI device listing (may be empty if no PCI bus).
    let pci_entries = fs.readdir("/devices/pci");
    if let Ok(entries) = pci_entries {
        serial_println!("[sysfs]   devices/pci: {} devices", entries.len());
    }

    // 9. Stat on various paths.
    let root_stat = fs.stat("/")?;
    assert!(root_stat.entry_type == EntryType::Directory);

    let version_stat = fs.stat("/kernel/version")?;
    assert!(version_stat.entry_type == EntryType::File);
    serial_println!("[sysfs]   stat: OK");

    // 10. Metadata with permissions.
    let hostname_meta = fs.metadata("/kernel/hostname")?;
    assert!(hostname_meta.permissions == 0o644, "hostname should be rw-r--r--");
    let version_meta = fs.metadata("/kernel/version")?;
    assert!(version_meta.permissions == 0o444, "version should be r--r--r--");
    serial_println!("[sysfs]   metadata/permissions: OK");

    // 11. CPU topology tree (/sys/devices/system/cpu).  This is the
    // authoritative path glibc get_nprocs()/lscpu/nproc try before the
    // /proc/cpuinfo fallback, so the range files must be present, correctly
    // formatted ("0" or "0-N"), and consistent with the kernel CPU counts.
    {
        // system/ lists cpu/.
        let sys_dir = fs.readdir("/devices/system")?;
        assert!(
            sys_dir.iter().any(|e| e.name == "cpu" && e.entry_type == EntryType::Directory),
            "devices/system should contain 'cpu' directory"
        );

        // cpu/ lists exactly the range files.
        let cpu_dir = fs.readdir("/devices/system/cpu")?;
        for name in CPU_FILES {
            assert!(
                cpu_dir.iter().any(|e| e.name == *name && e.entry_type == EntryType::File),
                "devices/system/cpu missing '{}'",
                name
            );
        }

        // online must equal the SMP online count, formatted Linux-style.
        let online = fs.read_file("/devices/system/cpu/online")?;
        let online_txt = core::str::from_utf8(&online)
            .map_err(|_| KernelError::InternalError)?;
        let want_online = cpu_range(crate::smp::cpu_count());
        assert!(
            online_txt == want_online,
            "cpu/online = {:?}, want {:?}",
            online_txt, want_online
        );

        // present/possible must equal the ACPI present count.
        let present = fs.read_file("/devices/system/cpu/present")?;
        let present_txt = core::str::from_utf8(&present)
            .map_err(|_| KernelError::InternalError)?;
        let want_present = cpu_range(crate::acpi::processor_count());
        assert!(
            present_txt == want_present,
            "cpu/present = {:?}, want {:?}",
            present_txt, want_present
        );

        // kernel_max parses as a number and is >= any online index.
        let kmax = fs.read_file("/devices/system/cpu/kernel_max")?;
        let kmax_txt = core::str::from_utf8(&kmax)
            .map_err(|_| KernelError::InternalError)?;
        let kmax_val: usize = kmax_txt.trim().parse()
            .map_err(|_| KernelError::InternalError)?;
        assert!(
            kmax_val >= crate::smp::cpu_count().saturating_sub(1),
            "cpu/kernel_max {} < highest online index",
            kmax_val
        );

        // The range files are read-only.
        assert!(
            fs.write_file("/devices/system/cpu/online", b"0-1").is_err(),
            "cpu/online should reject writes"
        );

        // Stat reports a directory for cpu/ and a file for online.
        assert!(fs.stat("/devices/system/cpu")?.entry_type == EntryType::Directory);
        assert!(fs.stat("/devices/system/cpu/online")?.entry_type == EntryType::File);
        // An unknown cpu file is NotFound, not a phantom.
        assert!(fs.stat("/devices/system/cpu/bogus").is_err());

        serial_println!(
            "[sysfs]   devices/system/cpu: OK (online={}, present={})",
            online_txt.trim(), present_txt.trim()
        );
    }

    // 12. Per-CPU topology subtree (/sys/devices/system/cpu/cpuN/topology/).
    // hwloc and lscpu read these to reconstruct socket/core/thread layout.
    {
        let ncpu = crate::acpi::processor_count();
        // cpu/ lists one cpuN directory per present CPU.
        let cpu_dir = fs.readdir("/devices/system/cpu")?;
        for i in 0..ncpu {
            let want = format!("cpu{i}");
            assert!(
                cpu_dir.iter().any(|e| e.name == want
                    && e.entry_type == EntryType::Directory),
                "devices/system/cpu missing '{}'",
                want
            );
        }

        // cpu0 always exists; it exposes a topology/ subdir.
        let cpu0 = fs.readdir("/devices/system/cpu/cpu0")?;
        assert!(
            cpu0.iter().any(|e| e.name == "topology"
                && e.entry_type == EntryType::Directory),
            "cpu0 should contain 'topology'"
        );

        // topology/ lists exactly the topology files.
        let topo = fs.readdir("/devices/system/cpu/cpu0/topology")?;
        for name in CPU_TOPOLOGY_FILES {
            assert!(
                topo.iter().any(|e| e.name == *name && e.entry_type == EntryType::File),
                "cpu0/topology missing '{}'",
                name
            );
        }

        // physical_package_id and core_id parse as numbers.
        let pkg = fs.read_file("/devices/system/cpu/cpu0/topology/physical_package_id")?;
        let pkg_txt = core::str::from_utf8(&pkg).map_err(|_| KernelError::InternalError)?;
        let _pkg_val: u32 = pkg_txt.trim().parse().map_err(|_| KernelError::InternalError)?;
        let core = fs.read_file("/devices/system/cpu/cpu0/topology/core_id")?;
        let core_txt = core::str::from_utf8(&core).map_err(|_| KernelError::InternalError)?;
        let _core_val: u32 = core_txt.trim().parse().map_err(|_| KernelError::InternalError)?;

        // thread_siblings_list always includes self (cpu0).
        let tsl = fs.read_file("/devices/system/cpu/cpu0/topology/thread_siblings_list")?;
        let tsl_txt = core::str::from_utf8(&tsl).map_err(|_| KernelError::InternalError)?;
        assert!(
            tsl_txt.split([',', '-'])
                .any(|tok| tok.trim() == "0"),
            "thread_siblings_list {:?} should include cpu 0",
            tsl_txt
        );

        // core_siblings (hex mask) has bit 0 set (self is in its own socket).
        let csm = fs.read_file("/devices/system/cpu/cpu0/topology/core_siblings")?;
        let csm_txt = core::str::from_utf8(&csm).map_err(|_| KernelError::InternalError)?;
        let csm_val = u32::from_str_radix(csm_txt.trim(), 16)
            .map_err(|_| KernelError::InternalError)?;
        assert!(csm_val & 1 == 1, "core_siblings {:?} should set bit 0", csm_txt);

        // Topology files are read-only.
        assert!(
            fs.write_file("/devices/system/cpu/cpu0/topology/core_id", b"7").is_err(),
            "topology/core_id should reject writes"
        );

        // Out-of-range CPU and unknown topology file are NotFound.
        let bogus_cpu = format!("/devices/system/cpu/cpu{ncpu}");
        assert!(fs.stat(&bogus_cpu).is_err(), "cpu{} should not exist", ncpu);
        assert!(
            fs.stat("/devices/system/cpu/cpu0/topology/bogus").is_err(),
            "unknown topology file should be NotFound"
        );
        // cpuN with a leading zero is not a valid Linux name.
        assert!(fs.stat("/devices/system/cpu/cpu00").is_err());

        serial_println!(
            "[sysfs]   devices/system/cpu/cpuN/topology: OK ({} cpuN dirs)",
            ncpu
        );
    }

    serial_println!("[sysfs] Self-test passed.");
    Ok(())
}
