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
//! │   └── pci/
//! │       ├── BB:DD.F      PCI device info per BDF address
//! │       └── ...
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
                // Just "pci/" for now.
                Ok(vec![DirEntry {
                    name: String::from("pci"),
                    entry_type: EntryType::Directory,
                    size: 0,
                }])
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
            | SysPath::PciDir => Err(KernelError::IsADirectory),

            SysPath::KernelFile(name) => gen_kernel_file(name),
            SysPath::ParamFile(name) => gen_param_file(name),
            SysPath::PciDevice(bdf) => gen_pci_device(bdf),
            SysPath::FsFile(name) => gen_fs_file(name),
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
            SysPath::KernelFile(_) | SysPath::FsFile(_) | SysPath::PciDevice(_) => {
                // Read-only files.
                Err(KernelError::NotSupported)
            }
            SysPath::Root
            | SysPath::SubDir(_)
            | SysPath::DevicesDir
            | SysPath::PciDir => Err(KernelError::IsADirectory),
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

    serial_println!("[sysfs] Self-test passed.");
    Ok(())
}
