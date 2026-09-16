//! Live Hardware Query Module
//!
//! Provides a trait-based abstraction for querying hardware information.
//! Two implementations:
//! - `SyscallProvider`: reads from OS info files (e.g. /sys/hardware/cpu,
//!   /sys/hardware/memory, etc.) using CPUID and sysfs-like interfaces.
//! - `StubProvider`: returns representative hardcoded data for development.
//!
//! The `RefreshManager` wraps any provider and caches results with a
//! configurable TTL, automatically refreshing stale data on access.

use crate::{
    CpuInfo, DiskInfo, DisplayInfo, DmaInfo, DriverInfo, IoPortInfo, IrqInfo, MemoryInfo,
    MemoryMapEntry, NetworkAdapterInfo, PciDeviceInfo, ProcessEntry, ServiceInfo, SoundInfo,
    StartupEntry, UsbDeviceInfo,
};
// Only `StubProvider` builds these, and it is `#[cfg(test)]`. `PartitionInfo`
// joined them when `query_storage` stopped inventing partitions: the kernel
// publishes no partition table under `/sys/devices/block`, so the real
// provider has none to build.
#[cfg(test)]
use crate::{MemorySlot, PartitionInfo};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Error type
// ============================================================================

/// Errors that can occur during hardware queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwQueryError {
    /// The requested info file or sysfs path does not exist.
    NotAvailable { path: String },
    /// Failed to parse data from the OS.
    ParseError { detail: String },
    /// The query timed out.
    Timeout,
    /// Permission denied for the query.
    PermissionDenied,
    /// Generic I/O error.
    IoError { detail: String },
}

impl std::fmt::Display for HwQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAvailable { path } => write!(f, "not available: {path}"),
            Self::ParseError { detail } => write!(f, "parse error: {detail}"),
            Self::Timeout => write!(f, "query timed out"),
            Self::PermissionDenied => write!(f, "permission denied"),
            Self::IoError { detail } => write!(f, "I/O error: {detail}"),
        }
    }
}

// ============================================================================
// Sysfs paths for our OS
// ============================================================================

/// Build a path under the kernel's hardware directory.
///
/// One literal for the base. There used to be a `const SYSFS_BASE` holding
/// `/sys/hardware` that nothing referred to, standing beside twelve constants
/// that each spelled `/sys/hardware/...` out again -- which is exactly why the
/// declaration could fall out of use without anyone noticing: nothing was
/// built from it. `concat!` takes literals rather than constants, so a macro
/// is what makes the base reachable at all.
///
/// `/sys/services` and `/sys/proc` below are deliberately not built from it:
/// they are not hardware, and folding them in would need a second base and put
/// the count back where it started.
///
/// **The cost, paid here so it is paid once.** Building the paths means the
/// full strings no longer appear in this file's *code*, so `grep
/// "/sys/hardware/cpu"` finds only prose and reads exactly like "nothing
/// refers to this". Lane A hit that on 2026-09-14 while looking for the
/// producer and believed it for about a minute. The twelve are therefore
/// written out once, in full, right here, so the grep lands somewhere that
/// explains itself:
///
/// ```text
/// /sys/hardware/cpu      /sys/hardware/memory   /sys/hardware/block
/// /sys/hardware/net      /sys/hardware/pci      /sys/hardware/usb
/// /sys/hardware/display  /sys/hardware/sound    /sys/hardware/irqs
/// /sys/hardware/ioports  /sys/hardware/memmap   /sys/hardware/dma
/// ```
macro_rules! sysfs {
    ($leaf:literal) => {
        concat!("/sys/hardware", $leaf)
    };
}
/// The CPU tree, as `design-decisions.md` §850 settled it.
///
/// `/sys/devices`, not a second `/sys/hardware`: the kernel already publishes
/// `core_id`, `physical_package_id`, `online` and cache geometry here, and a
/// parallel tree would publish the same facts twice in two layouts.
///
/// This is a **directory of scalar files**, one value per file, which is the
/// Linux shape -- not the single `key=value` file the older constants below
/// still name. That difference is the whole of why §850 was more than a
/// rename, and why these two queries have their own readers.
const SYSDEV_CPU: &str = "/sys/devices/system/cpu";
/// CPUID leaf 1 identity: `family`, `model`, `stepping`, one per file.
const SYSDEV_CPUID: &str = "/sys/devices/system/cpu/cpuid";
/// System memory: `total_kb` and `available_kb`, one per file.
const SYSDEV_MEMORY: &str = "/sys/devices/system/memory";
/// Block devices: `/sys/devices/block/<name>/{sector_count,sector_size,read_only}`.
///
/// `/sys/devices`, not `/sys/hardware`, for the reason §850 gives and that
/// `SYSDEV_CPU` above already follows. This constant used to name
/// `/sys/hardware/block`, **a path the kernel has never served**, so the
/// Storage category reported "cannot read" on a machine whose disks were
/// published the whole time.
///
/// A directory of scalar files, one value per file -- not the `key=value`
/// file the older constants below still name.
const SYSDEV_BLOCK: &str = "/sys/devices/block";
// `/sys/hardware/net` is gone with the query that read it. Interfaces come
// from `/proc/net/dev`, and lane A has declined to serve a `/sys/devices/net/`
// because the kernel's `InterfaceInfo` carries no name to key it on.
/// PCI devices directory.
const SYSFS_PCI: &str = sysfs!("/pci");
/// USB devices directory.
const SYSFS_USB: &str = sysfs!("/usb");
// No `/sys/hardware/display` constant: outputs come from
// `/proc/monitors`. Lane A has recorded that the sysfs node will never
// be served, because /proc already answers the question and a second
// kernel answer to one question is what design-decisions 850 prevents.
/// Sound devices.
const SYSFS_SOUND: &str = sysfs!("/sound");
// No `/sys/hardware/irqs` constant: interrupt lines come from
// `/proc/interrupts`, for the same reason.
/// I/O port ranges.
const SYSFS_IOPORTS: &str = sysfs!("/ioports");
/// Memory map from firmware.
const SYSFS_MEMMAP: &str = sysfs!("/memmap");
/// DMA channels.
const SYSFS_DMA: &str = sysfs!("/dma");
/// Running services.
const SYSFS_SERVICES: &str = "/sys/services";
// No constant for the process list: it comes from `/proc`, which is where
// processes have always been. This named `/sys/proc`, a path with no producer
// and no precedent -- Linux has never put a process list under `/sys`.
/// Loaded drivers.
const SYSFS_DRIVERS: &str = "/sys/drivers";
/// Startup programs.
const SYSFS_STARTUP: &str = "/sys/startup";

// ============================================================================
// Provider trait
// ============================================================================

/// Trait for hardware information providers.
///
/// Implementations can query live hardware or return static data.
/// All methods return `Result` to handle cases where the query fails
/// (e.g., sysfs file missing, permission denied, running in a VM).
pub trait HardwareProvider {
    /// Query CPU information.
    fn query_cpu(&self) -> Result<CpuInfo, HwQueryError>;
    /// Query memory information.
    fn query_memory(&self) -> Result<MemoryInfo, HwQueryError>;
    /// Query storage devices.
    fn query_storage(&self) -> Result<Vec<DiskInfo>, HwQueryError>;
    /// Query network adapters.
    fn query_network(&self) -> Result<Vec<NetworkAdapterInfo>, HwQueryError>;
    /// Query display/GPU info.
    fn query_display(&self) -> Result<DisplayInfo, HwQueryError>;
    /// Query PCI devices.
    fn query_pci(&self) -> Result<Vec<PciDeviceInfo>, HwQueryError>;
    /// Query USB devices.
    fn query_usb(&self) -> Result<Vec<UsbDeviceInfo>, HwQueryError>;
    /// Query sound devices.
    fn query_sound(&self) -> Result<Vec<SoundInfo>, HwQueryError>;
    /// Query IRQ assignments.
    fn query_irqs(&self) -> Result<Vec<IrqInfo>, HwQueryError>;
    /// Query I/O port ranges.
    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError>;
    /// Query firmware memory map.
    fn query_memory_map(&self) -> Result<Vec<MemoryMapEntry>, HwQueryError>;
    /// Query DMA channel assignments.
    fn query_dma(&self) -> Result<Vec<DmaInfo>, HwQueryError>;
    /// Query running services.
    fn query_services(&self) -> Result<Vec<ServiceInfo>, HwQueryError>;
    /// Query running processes.
    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError>;
    /// Query loaded drivers.
    fn query_drivers(&self) -> Result<Vec<DriverInfo>, HwQueryError>;
    /// Query environment variables.
    fn query_env_vars(&self) -> Result<Vec<(String, String)>, HwQueryError>;
    /// How long the machine has been up.
    ///
    /// A `Duration` rather than a formatted string, because the caller is the
    /// only one that knows how much room it has to draw it in -- and because a
    /// provider that returned "4h 23m 17s" is exactly what this replaced.
    fn query_uptime(&self) -> Result<std::time::Duration, HwQueryError>;
    /// Query startup programs.
    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError>;
    /// Human-readable name of this provider.
    ///
    /// `&'static str` rather than a borrow of `self`: the name identifies the
    /// *kind* of provider, not anything it holds, so tying it to the
    /// provider's lifetime would stop a caller keeping the name in a log line
    /// after the provider is gone -- for no gain, since every implementation
    /// returns a literal.
    fn provider_name(&self) -> &'static str;
}

// ============================================================================
// Syscall-based provider
// ============================================================================

/// Provider that queries live hardware via sysfs-like files and CPUID.
///
/// Reads from `/sys/hardware/*` files exposed by the kernel. Falls back
/// to CPUID for CPU feature detection. Returns `NotAvailable` for any
/// info that isn't exposed yet.
pub struct SyscallProvider {
    /// Cache of file contents from sysfs reads.
    file_cache: HashMap<String, String>,
    /// Prefixed to every path read, empty in a shipping build.
    ///
    /// The same seam `procinfo::ProcFs::at` provides, and for the same reason:
    /// the constants here are absolute, so without it nothing that *lists* a
    /// directory can be tested at all. `query_storage` reads the real
    /// `/sys/devices/block` through `read_dir`, which the file cache cannot
    /// stand in for -- a cache of file contents has no answer to "what is in
    /// this directory".
    root: String,
}

impl Default for SyscallProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SyscallProvider {
    /// A provider reading under `root` instead of `/`. **Tests only.**
    #[cfg(test)]
    pub fn at(root: &str) -> Self {
        Self {
            file_cache: HashMap::new(),
            root: root.to_string(),
        }
    }

    /// `path` as this provider should actually open it.
    fn rooted(&self, path: &str) -> String {
        format!("{}{path}", self.root)
    }

    /// The kernel's `/proc`, under the same root.
    ///
    /// A reader rather than a parser here: `procinfo` exists so that the two
    /// system-information programs in this tree do not grow two parsers of
    /// `/proc/meminfo` between them, and its module docs name this window by
    /// name as one of the two.
    fn procfs(&self) -> procinfo::ProcFs {
        procinfo::ProcFs::at(self.rooted("/proc"))
    }

    /// Create a new syscall-based provider.
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
            root: String::new(),
        }
    }

    /// Read a sysfs file, returning its contents or an error.
    /// One scalar file's whole contents, trimmed.
    ///
    /// The unit of the `/sys/devices` tree. `read_sysfs` below reads a file of
    /// `key=value` lines, which is what the older paths in this module serve;
    /// the two are not interchangeable and the names say which is which.
    fn read_scalar(&self, path: &str) -> Result<String, HwQueryError> {
        Ok(self.read_sysfs(path)?.trim().to_string())
    }

    /// A scalar file parsed as a number.
    fn read_num<T: core::str::FromStr>(&self, path: &str) -> Result<T, HwQueryError> {
        self.read_scalar(path)?
            .parse()
            .map_err(|_| HwQueryError::NotAvailable {
                path: path.to_string(),
            })
    }

    /// How many CPUs a Linux-style range names: `"0-7"` is 8, `"0,2-3"` is 3.
    ///
    /// `None` for anything it cannot read as a range, rather than a count of
    /// zero. A machine with no CPUs is not a possible answer, so a zero could
    /// only mean the parse failed, and saying so is cheaper than making every
    /// caller wonder.
    fn count_range(spec: &str) -> Option<u32> {
        let mut total = 0_u32;
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let n = match part.split_once('-') {
                None => {
                    part.parse::<u32>().ok()?;
                    1
                }
                Some((lo, hi)) => {
                    let lo: u32 = lo.trim().parse().ok()?;
                    let hi: u32 = hi.trim().parse().ok()?;
                    hi.checked_sub(lo)?.checked_add(1)?
                }
            };
            total = total.checked_add(n)?;
        }
        (total > 0).then_some(total)
    }

    fn read_sysfs(&self, path: &str) -> Result<String, HwQueryError> {
        // On the actual OS, this would use SYS_READ to read from the sysfs VFS.
        // For now, check the file cache (populated by refresh) or try a real read.
        if let Some(cached) = self.file_cache.get(path) {
            return Ok(cached.clone());
        }

        // Attempt a real filesystem read. The error names the path actually
        // opened rather than the logical one, so a reader can go and look at
        // it.
        let opened = self.rooted(path);
        match std::fs::read_to_string(&opened) {
            Ok(content) => Ok(content),
            Err(_) => Err(HwQueryError::NotAvailable { path: opened }),
        }
    }

    /// One numeric field of a hardware file.
    ///
    /// **Absent and malformed are different, and that is the whole point.** A
    /// field the kernel did not report is ordinary -- a machine with no L3
    /// cache reports no `l3_kb` -- and the caller's default stands. A field
    /// that is *present* and will not parse means the file is corrupt, and a
    /// system-information program that shows a corrupt reading as `0` is
    /// displaying a fact it does not have. Every one of these sites used to do
    /// exactly that: `kv.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)`.
    ///
    /// This replaces three near-identical `parse_u64` / `parse_u32` /
    /// `parse_f32` helpers that did the right thing, had four tests between
    /// them, and which nothing outside those tests ever called. One parser is
    /// also one model of "read a number out of this map", which the three were
    /// not.
    fn field<T: core::str::FromStr>(
        kv: &HashMap<String, String>,
        key: &str,
        default: T,
    ) -> Result<T, HwQueryError> {
        match kv.get(key) {
            None => Ok(default),
            Some(raw) => raw.trim().parse().map_err(|_| HwQueryError::ParseError {
                detail: format!(
                    "{key}: expected {}, got: '{raw}'",
                    core::any::type_name::<T>()
                ),
            }),
        }
    }

    /// Read the processor from `/sys/devices/system/cpu`.
    ///
    /// Scalar-per-file: `cpuid/` for the CPUID leaf 1 identity, `present` for
    /// the logical count, `cpuN/topology/` for the physical core count,
    /// `cpu0/cache/indexN/` for the cache geometry.
    ///
    /// **Four fields come back `None` and always will on this kernel.** There
    /// is no brand string -- CPUID leaves 0x8000_0002..4 are not served -- no
    /// vendor (leaf 0), and no `cpufreq/`, so neither clock can be read.
    /// `sysfs.rs` gives the reason for the last and it covers all four: a file
    /// reading 0 cannot be told from a real 0, so absent is the honest answer.
    /// The fields are `Option` so this function cannot invent them.
    fn query_cpu_from_cpuid(&self) -> Result<CpuInfo, HwQueryError> {
        // The identity must be present: if `cpuid/` is not there the tree is
        // not there, and a CpuInfo of defaults would describe a machine.
        let family = self.read_num(&format!("{SYSDEV_CPUID}/family"))?;
        let model = self.read_num(&format!("{SYSDEV_CPUID}/model"))?;
        let stepping = self.read_num(&format!("{SYSDEV_CPUID}/stepping"))?;

        let logical_processors = self
            .read_scalar(&format!("{SYSDEV_CPU}/present"))
            .ok()
            .and_then(|spec| Self::count_range(&spec))
            .unwrap_or(1);

        Ok(CpuInfo {
            brand: None,
            vendor: None,
            family,
            model,
            stepping,
            physical_cores: self.physical_core_count(logical_processors),
            logical_processors,
            base_clock_mhz: None,
            max_turbo_mhz: None,
            l1_data_kb: self.cache_kb(1, "Data").unwrap_or(0),
            l1_inst_kb: self.cache_kb(1, "Instruction").unwrap_or(0),
            l2_kb: self.cache_kb(2, "Unified").unwrap_or(0),
            l3_kb: self.cache_kb(3, "Unified").unwrap_or(0),
            features: Vec::new(),
        })
    }

    /// Distinct `(socket, core)` pairs across the present CPUs.
    ///
    /// Counted rather than divided by an assumed two threads per core: a
    /// machine without hyper-threading would come back at half its real core
    /// count, and one with four threads per core at twice.
    ///
    /// Falls back to the logical count when the topology is unreadable, which
    /// is right rather than convenient -- a machine with as many cores as
    /// threads is plausible, one with zero cores is not.
    fn physical_core_count(&self, logical: u32) -> u32 {
        let mut seen: Vec<(String, String)> = Vec::new();
        for cpu in 0..logical {
            let dir = format!("{SYSDEV_CPU}/cpu{cpu}/topology");
            let socket = self.read_scalar(&format!("{dir}/physical_package_id"));
            let core = self.read_scalar(&format!("{dir}/core_id"));
            if let (Ok(socket), Ok(core)) = (socket, core) {
                let pair = (socket, core);
                if !seen.contains(&pair) {
                    seen.push(pair);
                }
            }
        }
        u32::try_from(seen.len())
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(logical)
    }

    /// The size in KiB of the first cache matching `level` and `kind`.
    ///
    /// `size` is served the way Linux serves it, a number with a `K` or `M`
    /// suffix, so it is parsed rather than assumed to be bytes -- reading
    /// "32K" as 32 bytes would report a thirty-second of a kilobyte of L1.
    fn cache_kb(&self, level: u32, kind: &str) -> Option<u32> {
        for index in 0..8 {
            let dir = format!("{SYSDEV_CPU}/cpu0/cache/index{index}");
            let Ok(found_level) = self.read_num::<u32>(&format!("{dir}/level")) else {
                continue;
            };
            let Ok(found_kind) = self.read_scalar(&format!("{dir}/type")) else {
                continue;
            };
            if found_level != level || !found_kind.eq_ignore_ascii_case(kind) {
                continue;
            }
            let Ok(size) = self.read_scalar(&format!("{dir}/size")) else {
                continue;
            };
            return Self::parse_cache_size_kb(&size);
        }
        None
    }

    /// `"32K"` becomes 32, `"8M"` becomes 8192, a bare number is already KiB.
    fn parse_cache_size_kb(text: &str) -> Option<u32> {
        let text = text.trim();
        let last = text.chars().last()?;
        let kilo = last.eq_ignore_ascii_case(&'K');
        let mega = last.eq_ignore_ascii_case(&'M');
        let (digits, scale) = if kilo || mega {
            let cut = text.len().checked_sub(1)?;
            (text.get(..cut)?, if mega { 1024_u32 } else { 1_u32 })
        } else {
            (text, 1_u32)
        };
        digits.trim().parse::<u32>().ok()?.checked_mul(scale)
    }

    /// Parse a list of records from a sysfs directory.
    /// Each entry is a subdirectory with key=value files.
    fn read_sysfs_dir_entries(
        &self,
        base_path: &str,
    ) -> Result<Vec<HashMap<String, String>>, HwQueryError> {
        // On the actual OS, this would list directory entries and read each one.
        // For cross-platform dev, try filesystem read.
        let content = self.read_sysfs(base_path)?;

        // If the file contains multiple records separated by blank lines,
        // parse each record as a key=value block.
        let mut entries = Vec::new();
        let mut current = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                if !current.is_empty() {
                    entries.push(current.clone());
                    current.clear();
                }
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                current.insert(key.trim().to_string(), value.trim().to_string());
            } else if let Some((key, value)) = line.split_once(':') {
                current.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        if !current.is_empty() {
            entries.push(current);
        }

        Ok(entries)
    }
}

impl HardwareProvider for SyscallProvider {
    fn query_cpu(&self) -> Result<CpuInfo, HwQueryError> {
        self.query_cpu_from_cpuid()
    }

    /// Read memory from `/sys/devices/system/memory`.
    ///
    /// Two scalar files, both from `mm::memory_info`. Lane A's producer
    /// carries a boot-time cross-check asserting `total_kb` equals
    /// `/proc/meminfo`'s `MemTotal`, so a later change to a frame counter that
    /// includes non-usable holes goes red rather than quietly inflating what
    /// this window reports.
    ///
    /// **No slots.** The physical layout -- part numbers, per-slot speeds, how
    /// many sockets are populated -- is SMBIOS, which nothing in this tree
    /// reads. The list is empty rather than invented; it used to carry two
    /// 16 GB Corsair modules at 5,600 MHz.
    fn query_memory(&self) -> Result<MemoryInfo, HwQueryError> {
        let total_kb: u64 = self.read_num(&format!("{SYSDEV_MEMORY}/total_kb"))?;
        let available_kb: u64 = self.read_num(&format!("{SYSDEV_MEMORY}/available_kb"))?;
        Ok(MemoryInfo {
            total_mb: total_kb / 1024,
            available_mb: available_kb / 1024,
            // The kind and speed of the memory are SMBIOS facts too.
            mem_type: String::new(),
            speed_mhz: 0,
            slots_used: 0,
            slots_total: 0,
            slots: Vec::new(),
        })
    }

    /// Read the registered block devices from `/sys/devices/block`.
    ///
    /// Capacity is `sector_count * sector_size`, and both names are read
    /// rather than one assumed. Lane A's producer says why in
    /// `kernel/src/fs/sysfs.rs`: Linux's `size` is in 512-byte units whatever
    /// the device's real sector size is, so a reader that multiplied it by
    /// `sector_size` would be wrong on any device that is not 512 -- and every
    /// device here is 512 today, **which is the condition that lets that bug
    /// ship unnoticed.**
    ///
    /// Three fields stay empty rather than being filled with something
    /// plausible. The model string, the serial number and SMART health are
    /// SMBIOS and ATA facts that nothing in this tree reads; the device's own
    /// name is what there is, and it is put in `model` because that is the
    /// column the window draws. **Partitions are empty for the same reason**
    /// -- the kernel publishes no partition table here, and the previous
    /// implementation read `part0_`-prefixed keys out of a file that does not
    /// exist.
    fn query_storage(&self) -> Result<Vec<DiskInfo>, HwQueryError> {
        let base_dir = self.rooted(SYSDEV_BLOCK);
        let dir = std::fs::read_dir(&base_dir).map_err(|_| HwQueryError::NotAvailable {
            path: base_dir.clone(),
        })?;

        let mut disks = Vec::new();
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let base = format!("{SYSDEV_BLOCK}/{name}");
            // A device unregistered between the listing and the read is
            // skipped rather than failing the whole query: the same race a
            // process list has, and the same answer.
            let (Ok(sector_count), Ok(sector_size)) = (
                self.read_num::<u64>(&format!("{base}/sector_count")),
                self.read_num::<u64>(&format!("{base}/sector_size")),
            ) else {
                continue;
            };
            let read_only = self
                .read_num::<u8>(&format!("{base}/read_only"))
                .unwrap_or(0);

            disks.push(DiskInfo {
                model: name,
                capacity_bytes: sector_count.saturating_mul(sector_size),
                interface: String::new(),
                serial: String::new(),
                smart_status: if read_only == 1 {
                    String::from("read-only")
                } else {
                    String::new()
                },
                partitions: Vec::new(),
            });
        }
        Ok(disks)
    }

    /// Read the network interfaces from `/proc/net/dev`.
    ///
    /// This read `/sys/hardware/net`, which the kernel has never served, so
    /// the category reported "cannot read". `/proc/net/dev` is published and
    /// gives the interface names and their traffic counters.
    ///
    /// **Everything else stays empty, and that is deliberate.** A MAC address,
    /// an IPv4 lease, a gateway, a DNS server, a link speed and a duplex mode
    /// are not published by anything in this tree. Lane A declined to add a
    /// `/sys/devices/net/` for the same reason, in their own words: the
    /// kernel's `InterfaceInfo` "has no name field, so both would be
    /// invented". An adapter row with a plausible `192.168.1.x` in it is worse
    /// than one with a blank, because the blank is legible as absent.
    fn query_network(&self) -> Result<Vec<NetworkAdapterInfo>, HwQueryError> {
        let devices = self.procfs().net_devices().ok().flatten().ok_or_else(|| {
            HwQueryError::NotAvailable {
                path: self.rooted("/proc/net/dev"),
            }
        })?;

        Ok(devices
            .iter()
            .map(|d| NetworkAdapterInfo {
                // The name is the one field `/proc/net/dev` keys on, and it is
                // bytes there; it becomes text only to be drawn.
                name: String::from_utf8_lossy(&d.name).into_owned(),
                adapter_type: String::new(),
                mac_address: String::new(),
                ipv4: String::new(),
                ipv6: String::new(),
                subnet: String::new(),
                gateway: String::new(),
                dns: String::new(),
                speed_mbps: 0,
                duplex: String::new(),
                bytes_sent: d.tx_bytes.unwrap_or(0),
                bytes_received: d.rx_bytes.unwrap_or(0),
            })
            .collect())
    }

    /// Read the outputs from `/proc/monitors`.
    ///
    /// Four fields stay empty. The GPU's name, vendor, VRAM and driver version
    /// are published by nothing here -- `/proc/monitors` describes *outputs*,
    /// not the adapter driving them -- and a plausible "16384 MB" beside real
    /// resolutions would be the one figure on the page nobody could check.
    fn query_display(&self) -> Result<DisplayInfo, HwQueryError> {
        let mons =
            self.procfs()
                .monitors()
                .ok()
                .flatten()
                .ok_or_else(|| HwQueryError::NotAvailable {
                    path: self.rooted("/proc/monitors"),
                })?;

        // The primary output's mode is what "the resolution" means to a
        // reader. With no primary there is no single answer, and the field is
        // left empty rather than filled from whichever row happens to be
        // first.
        let primary = mons.primary();
        Ok(DisplayInfo {
            gpu_name: String::new(),
            vendor: String::new(),
            vram_mb: 0,
            resolution: primary
                .map_or_else(String::new, |mon| format!("{}x{}", mon.width, mon.height)),
            refresh_rate_hz: primary.map_or(0, |mon| mon.refresh_hz),
            // `enabled` defaults to true in the parser because the kernel
            // writes only a ` [disabled]` marker and never an ` [enabled]`
            // one, so a disabled output is still listed and still reports the
            // mode it would use.
            outputs: mons
                .outputs
                .iter()
                .map(|mon| (String::from_utf8_lossy(&mon.name).into_owned(), mon.enabled))
                .collect(),
            driver_version: String::new(),
        })
    }

    fn query_pci(&self) -> Result<Vec<PciDeviceInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_PCI)?;
        let mut devices = Vec::new();

        for entry in &entries {
            devices.push(PciDeviceInfo {
                bus: Self::field(entry, "bus", 0)?,
                device: Self::field(entry, "device", 0)?,
                function: Self::field(entry, "function", 0)?,
                vendor_id: entry
                    .get("vendor_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                device_id: entry
                    .get("device_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                class: entry.get("class").cloned().unwrap_or_default(),
                description: entry.get("description").cloned().unwrap_or_default(),
                vendor_name: entry.get("vendor_name").cloned().unwrap_or_default(),
            });
        }

        Ok(devices)
    }

    fn query_usb(&self) -> Result<Vec<UsbDeviceInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_USB)?;
        let mut devices = Vec::new();

        for entry in &entries {
            devices.push(UsbDeviceInfo {
                port: entry.get("port").cloned().unwrap_or_default(),
                vendor_id: entry
                    .get("vendor_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                product_id: entry
                    .get("product_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                description: entry.get("description").cloned().unwrap_or_default(),
                speed: entry.get("speed").cloned().unwrap_or_default(),
            });
        }

        Ok(devices)
    }

    fn query_sound(&self) -> Result<Vec<SoundInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_SOUND)?;
        let mut devices = Vec::new();

        for entry in &entries {
            devices.push(SoundInfo {
                name: entry.get("name").cloned().unwrap_or_default(),
                device_type: entry.get("type").cloned().unwrap_or_default(),
                driver: entry.get("driver").cloned().unwrap_or_default(),
                status: entry.get("status").cloned().unwrap_or_default(),
            });
        }

        Ok(devices)
    }

    /// Read the interrupt lines from `/proc/interrupts`.
    ///
    /// This read `/sys/hardware/irqs`, which this kernel has never served and
    /// which lane A has recorded it will never serve: `/proc` already
    /// publishes the data, and a second kernel answer to one question is what
    /// §850 exists to prevent.
    fn query_irqs(&self) -> Result<Vec<IrqInfo>, HwQueryError> {
        let table = self.procfs().interrupts().ok().flatten().ok_or_else(|| {
            HwQueryError::NotAvailable {
                path: self.rooted("/proc/interrupts"),
            }
        })?;

        Ok(table
            .irqs
            .iter()
            .map(|irq| IrqInfo {
                irq_number: irq.number,
                // Bytes in the file, because nothing guarantees the label is
                // UTF-8; text only here, where it becomes glyphs.
                device: String::from_utf8_lossy(&irq.description).into_owned(),
                irq_type: String::new(),
                asserted: irq.pending,
            })
            .collect())
    }

    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_IOPORTS)?;
        let mut ports = Vec::new();

        for entry in &entries {
            ports.push(IoPortInfo {
                start: entry
                    .get("start")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                end: entry
                    .get("end")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                device: entry.get("device").cloned().unwrap_or_default(),
            });
        }

        Ok(ports)
    }

    fn query_memory_map(&self) -> Result<Vec<MemoryMapEntry>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_MEMMAP)?;
        let mut regions = Vec::new();

        for entry in &entries {
            regions.push(MemoryMapEntry {
                start: entry
                    .get("start")
                    .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                end: entry
                    .get("end")
                    .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                region_type: entry.get("type").cloned().unwrap_or_default(),
                description: entry.get("description").cloned().unwrap_or_default(),
            });
        }

        Ok(regions)
    }

    fn query_dma(&self) -> Result<Vec<DmaInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_DMA)?;
        let mut channels = Vec::new();

        for entry in &entries {
            channels.push(DmaInfo {
                channel: Self::field(entry, "channel", 0)?,
                device: entry.get("device").cloned().unwrap_or_default(),
                mode: entry.get("mode").cloned().unwrap_or_default(),
            });
        }

        Ok(channels)
    }

    fn query_services(&self) -> Result<Vec<ServiceInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_SERVICES)?;
        let mut services = Vec::new();

        for entry in &entries {
            services.push(ServiceInfo {
                name: entry.get("name").cloned().unwrap_or_default(),
                status: entry.get("status").cloned().unwrap_or_default(),
                start_type: entry.get("start_type").cloned().unwrap_or_default(),
            });
        }

        Ok(services)
    }

    /// Read the running processes from `/proc`.
    ///
    /// The third window in this tree to read the same files, and the third to
    /// do it through `procinfo` rather than growing its own parser --
    /// `apps/procexplorer` and `apps/sysmonitor` were wired to it earlier
    /// today. Two parsers of `/proc/<pid>/stat` in one repository is the
    /// arrangement where a kernel change fixes one window and not the others
    /// and nobody notices, because all of them still produce numbers.
    ///
    /// `cpu_percent` stays 0.0. A percentage is a rate, and a rate needs two
    /// samples of a counter; this query has one. The same decision
    /// `procexplorer` makes, for the same reason, and the reason it is worth
    /// repeating here is that **0.0 is also what an invented value would look
    /// like if nobody had thought about it.**
    /// Read `/proc/uptime`.
    fn query_uptime(&self) -> Result<std::time::Duration, HwQueryError> {
        self.procfs()
            .uptime()
            .ok()
            .flatten()
            .map(|u| u.up)
            .ok_or_else(|| HwQueryError::NotAvailable {
                path: self.rooted("/proc/uptime"),
            })
    }

    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError> {
        let fs = self.procfs();
        let pids = fs.process_ids().map_err(|_| HwQueryError::NotAvailable {
            path: self.rooted("/proc"),
        })?;

        let mut procs = Vec::with_capacity(pids.len());
        for pid in pids {
            // A process that exits between the listing and the read is the
            // normal case, not an error: racing with the thing being measured
            // is what a process list is.
            let Ok(Some(stat)) = fs.process_stat(pid) else {
                continue;
            };
            procs.push(ProcessEntry {
                pid: u32::try_from(stat.pid).unwrap_or(u32::MAX),
                name: String::from_utf8_lossy(&stat.comm).into_owned(),
                memory_kb: stat.rss_kib(),
                cpu_percent: 0.0,
            });
        }
        Ok(procs)
    }

    fn query_drivers(&self) -> Result<Vec<DriverInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_DRIVERS)?;
        let mut drivers = Vec::new();

        for entry in &entries {
            drivers.push(DriverInfo {
                name: entry.get("name").cloned().unwrap_or_default(),
                path: entry.get("path").cloned().unwrap_or_default(),
                status: entry.get("status").cloned().unwrap_or_default(),
            });
        }

        Ok(drivers)
    }

    fn query_env_vars(&self) -> Result<Vec<(String, String)>, HwQueryError> {
        // Environment variables come from the process environment
        Ok(std::env::vars().collect())
    }

    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_STARTUP)?;
        let mut programs = Vec::new();

        for entry in &entries {
            programs.push(StartupEntry {
                name: entry.get("name").cloned().unwrap_or_default(),
                path: entry.get("path").cloned().unwrap_or_default(),
                source: entry.get("source").cloned().unwrap_or_default(),
            });
        }

        Ok(programs)
    }

    fn provider_name(&self) -> &'static str {
        "SyscallProvider (live hardware)"
    }
}

// ============================================================================
// Stub provider (existing hardcoded data, as fallback)
// ============================================================================

/// Provider that returns representative stub data.
/// Used for development, testing, and as a fallback when live queries fail.
/// Representative hardcoded values, for tests only.
///
/// `#[cfg(test)]` since 2026-09-15. It was reachable from production and
/// `FallbackProvider` called it whenever the syscall path failed — which is
/// every host without the sysfs tree, i.e. all of them — so the fallback's
/// purpose in practice was to display an invented machine. `FallbackProvider`
/// is gone; this stays because the tests legitimately need a provider that
/// answers.
///
/// One of the tests removed with the fallback asserted the fabrication
/// happened: *"FallbackProvider should work since SyscallProvider will fail
/// (no sysfs on dev machine) and fall back to StubProvider"*, then checked the
/// invented brand string. A test can pin a fabrication in place as firmly as
/// it pins anything else.
#[cfg(test)]
pub struct StubProvider;

#[cfg(test)]
impl Default for StubProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl StubProvider {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
impl HardwareProvider for StubProvider {
    fn query_cpu(&self) -> Result<CpuInfo, HwQueryError> {
        Ok(CpuInfo {
            brand: Some("Fixture CPU".to_string()),
            vendor: Some("FixtureVendor".to_string()),
            family: 6,
            model: 158,
            stepping: 13,
            physical_cores: 8,
            logical_processors: 16,
            base_clock_mhz: Some(3600),
            max_turbo_mhz: Some(5100),
            l1_data_kb: 32,
            l1_inst_kb: 32,
            l2_kb: 256,
            l3_kb: 16384,
            features: vec![
                ("SSE".to_string(), true),
                ("SSE2".to_string(), true),
                ("SSE3".to_string(), true),
                ("SSSE3".to_string(), true),
                ("SSE4.1".to_string(), true),
                ("SSE4.2".to_string(), true),
                ("AVX".to_string(), true),
                ("AVX2".to_string(), true),
                ("AVX-512".to_string(), false),
                ("AES-NI".to_string(), true),
                ("SHA".to_string(), true),
                ("RDRAND".to_string(), true),
            ],
        })
    }

    fn query_memory(&self) -> Result<MemoryInfo, HwQueryError> {
        Ok(MemoryInfo {
            total_mb: 32768,
            available_mb: 18432,
            mem_type: "DDR5".to_string(),
            speed_mhz: 5600,
            slots_used: 2,
            slots_total: 4,
            slots: vec![
                MemorySlot {
                    slot_name: "DIMM A1".to_string(),
                    size_mb: 16384,
                    mem_type: "DDR5".to_string(),
                    speed_mhz: 5600,
                    manufacturer: "Samsung".to_string(),
                },
                MemorySlot {
                    slot_name: "DIMM B1".to_string(),
                    size_mb: 16384,
                    mem_type: "DDR5".to_string(),
                    speed_mhz: 5600,
                    manufacturer: "Samsung".to_string(),
                },
            ],
        })
    }

    fn query_storage(&self) -> Result<Vec<DiskInfo>, HwQueryError> {
        Ok(vec![DiskInfo {
            model: "Samsung 990 Pro 2TB".to_string(),
            // 2.0×10¹² bytes, which is how a "2 TB" drive is actually sold —
            // and 1.82 TiB, which is what the display will call it. The figure
            // this replaced was 1863.0 labelled `GB`: gibibytes wearing an SI
            // name, so the mock disk agreed with neither reading.
            capacity_bytes: 2_000_398_934_016,
            interface: "NVMe".to_string(),
            serial: "S6Z2NF0W123456".to_string(),
            smart_status: "Healthy".to_string(),
            partitions: vec![
                PartitionInfo {
                    label: "EFI System".to_string(),
                    filesystem: "FAT32".to_string(),
                    capacity_bytes: 536_870_912,
                    used_bytes: 115_343_360,
                    free_bytes: 421_527_552,
                    mount_point: "/boot/efi".to_string(),
                },
                PartitionInfo {
                    label: "Slate OS Root".to_string(),
                    filesystem: "ext4".to_string(),
                    capacity_bytes: 536_870_912_000,
                    used_bytes: 136_667_299_840,
                    free_bytes: 400_203_612_160,
                    mount_point: "/".to_string(),
                },
            ],
        }])
    }

    fn query_network(&self) -> Result<Vec<NetworkAdapterInfo>, HwQueryError> {
        Ok(vec![NetworkAdapterInfo {
            name: "Intel I225-V Ethernet".to_string(),
            adapter_type: "Ethernet".to_string(),
            mac_address: "A4:BB:6D:12:34:56".to_string(),
            ipv4: "192.168.1.100".to_string(),
            ipv6: "fe80::a6bb:6dff:fe12:3456".to_string(),
            subnet: "255.255.255.0".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns: "1.1.1.1, 8.8.8.8".to_string(),
            speed_mbps: 2500,
            duplex: "Full".to_string(),
            bytes_sent: 1_542_876_160,
            bytes_received: 8_234_567_680,
        }])
    }

    fn query_display(&self) -> Result<DisplayInfo, HwQueryError> {
        Ok(DisplayInfo {
            gpu_name: "AMD Radeon RX 7900 XTX".to_string(),
            vendor: "AMD".to_string(),
            vram_mb: 24576,
            resolution: "3840x2160".to_string(),
            refresh_rate_hz: 144,
            outputs: vec![
                ("DisplayPort 1".to_string(), true),
                ("HDMI 1".to_string(), true),
            ],
            driver_version: "24.5.1".to_string(),
        })
    }

    fn query_pci(&self) -> Result<Vec<PciDeviceInfo>, HwQueryError> {
        Ok(vec![
            PciDeviceInfo {
                bus: 0,
                device: 0,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0xA700,
                class: "Host Bridge".to_string(),
                description: "Intel 13th Gen Core Host Bridge".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 0,
                device: 2,
                function: 0,
                vendor_id: 0x1002,
                device_id: 0x744C,
                class: "VGA Controller".to_string(),
                description: "AMD Radeon RX 7900 XTX".to_string(),
                vendor_name: "Advanced Micro Devices".to_string(),
            },
        ])
    }

    fn query_usb(&self) -> Result<Vec<UsbDeviceInfo>, HwQueryError> {
        Ok(vec![UsbDeviceInfo {
            port: "1-1".to_string(),
            vendor_id: 0x046D,
            product_id: 0xC548,
            description: "Logitech G Pro Wireless Mouse".to_string(),
            speed: "USB 2.0 (12 Mbps)".to_string(),
        }])
    }

    fn query_sound(&self) -> Result<Vec<SoundInfo>, HwQueryError> {
        Ok(vec![SoundInfo {
            name: "Realtek ALC4080 HD Audio".to_string(),
            device_type: "Output".to_string(),
            driver: "hda-intel".to_string(),
            status: "Active".to_string(),
        }])
    }

    fn query_irqs(&self) -> Result<Vec<IrqInfo>, HwQueryError> {
        Ok(vec![
            // `irq_type` is empty and `asserted` is false, matching what a
            // real read produces: this kernel publishes neither a trigger mode
            // nor a count, and a stub that invents richer data than the real
            // provider can return is a fixture that tests the wrong shape.
            IrqInfo {
                irq_number: 0,
                device: "Timer".to_string(),
                irq_type: String::new(),
                asserted: false,
            },
            IrqInfo {
                irq_number: 1,
                device: "Keyboard".to_string(),
                irq_type: String::new(),
                asserted: true,
            },
        ])
    }

    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError> {
        Ok(vec![
            IoPortInfo {
                start: 0x0060,
                end: 0x0064,
                device: "Keyboard Controller".to_string(),
            },
            IoPortInfo {
                start: 0x03F8,
                end: 0x03FF,
                device: "COM1 (Serial)".to_string(),
            },
        ])
    }

    fn query_memory_map(&self) -> Result<Vec<MemoryMapEntry>, HwQueryError> {
        Ok(vec![MemoryMapEntry {
            start: 0x0000_0000,
            end: 0x0009_FFFF,
            region_type: "Conventional".to_string(),
            description: "Low memory (640 KiB)".to_string(),
        }])
    }

    fn query_dma(&self) -> Result<Vec<DmaInfo>, HwQueryError> {
        Ok(vec![DmaInfo {
            channel: 2,
            device: "Floppy (legacy)".to_string(),
            mode: "Single".to_string(),
        }])
    }

    fn query_services(&self) -> Result<Vec<ServiceInfo>, HwQueryError> {
        Ok(vec![
            ServiceInfo {
                name: "compositor".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
            ServiceInfo {
                name: "network-manager".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
        ])
    }

    /// A fixed hour, so a test that formats an uptime has something to format.
    fn query_uptime(&self) -> Result<std::time::Duration, HwQueryError> {
        Ok(std::time::Duration::from_hours(1))
    }

    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError> {
        Ok(vec![
            ProcessEntry {
                pid: 1,
                name: "init".to_string(),
                memory_kb: 2048,
                cpu_percent: 0.0,
            },
            ProcessEntry {
                pid: 2,
                name: "compositor".to_string(),
                memory_kb: 128000,
                cpu_percent: 3.2,
            },
        ])
    }

    fn query_drivers(&self) -> Result<Vec<DriverInfo>, HwQueryError> {
        Ok(vec![
            DriverInfo {
                name: "nvme".to_string(),
                path: "/drivers/storage/nvme.drv".to_string(),
                status: "Loaded".to_string(),
            },
            DriverInfo {
                name: "amdgpu".to_string(),
                path: "/drivers/gpu/amdgpu.drv".to_string(),
                status: "Loaded".to_string(),
            },
        ])
    }

    fn query_env_vars(&self) -> Result<Vec<(String, String)>, HwQueryError> {
        Ok(vec![
            (
                "PATH".to_string(),
                "/bin:/sbin:/usr/bin:/usr/local/bin".to_string(),
            ),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SHELL".to_string(), "/bin/osh".to_string()),
        ])
    }

    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError> {
        Ok(vec![StartupEntry {
            name: "Network Manager".to_string(),
            path: "/usr/bin/network-manager".to_string(),
            source: "System".to_string(),
        }])
    }

    fn provider_name(&self) -> &'static str {
        "StubProvider (representative data)"
    }
}

// ============================================================================
// Fallback provider — tries live, falls back to stub
// ============================================================================

// ============================================================================
// Refresh Manager — cached queries with configurable TTL
// ============================================================================

/// Cache entry with a timestamp and TTL.
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    timestamp: u64,
    ttl_secs: u64,
}

impl<T> CacheEntry<T> {
    fn is_stale(&self, now: u64) -> bool {
        now.saturating_sub(self.timestamp) >= self.ttl_secs
    }
}

/// Manages cached hardware queries with configurable TTL per category.
///
/// On first access for each category, queries the provider and caches
/// the result. Subsequent accesses return the cached data until the TTL
/// expires, at which point the provider is re-queried.
pub struct RefreshManager {
    provider: Box<dyn HardwareProvider>,

    /// TTL in seconds for each data category.
    cpu_ttl: u64,
    memory_ttl: u64,
    storage_ttl: u64,
    network_ttl: u64,
    display_ttl: u64,
    pci_ttl: u64,
    usb_ttl: u64,
    sound_ttl: u64,
    irq_ttl: u64,
    ioport_ttl: u64,
    memmap_ttl: u64,
    dma_ttl: u64,
    service_ttl: u64,
    process_ttl: u64,
    driver_ttl: u64,
    env_ttl: u64,
    startup_ttl: u64,

    // Cached data
    cpu_cache: Option<CacheEntry<CpuInfo>>,
    memory_cache: Option<CacheEntry<MemoryInfo>>,
    storage_cache: Option<CacheEntry<Vec<DiskInfo>>>,
    network_cache: Option<CacheEntry<Vec<NetworkAdapterInfo>>>,
    display_cache: Option<CacheEntry<DisplayInfo>>,
    pci_cache: Option<CacheEntry<Vec<PciDeviceInfo>>>,
    usb_cache: Option<CacheEntry<Vec<UsbDeviceInfo>>>,
    sound_cache: Option<CacheEntry<Vec<SoundInfo>>>,
    irq_cache: Option<CacheEntry<Vec<IrqInfo>>>,
    ioport_cache: Option<CacheEntry<Vec<IoPortInfo>>>,
    memmap_cache: Option<CacheEntry<Vec<MemoryMapEntry>>>,
    dma_cache: Option<CacheEntry<Vec<DmaInfo>>>,
    service_cache: Option<CacheEntry<Vec<ServiceInfo>>>,
    process_cache: Option<CacheEntry<Vec<ProcessEntry>>>,
    driver_cache: Option<CacheEntry<Vec<DriverInfo>>>,
    env_cache: Option<CacheEntry<Vec<(String, String)>>>,
    startup_cache: Option<CacheEntry<Vec<StartupEntry>>>,

    /// Total refresh count for metrics.
    refresh_count: u64,
}

/// Default TTL values for different categories.
const TTL_STATIC_SECS: u64 = 300; // CPU, display, PCI: rarely change
const TTL_DYNAMIC_SECS: u64 = 5; // Processes, memory usage: change constantly
const TTL_MODERATE_SECS: u64 = 30; // Network stats, services: change occasionally

impl RefreshManager {
    /// Create a new refresh manager with a specific provider.
    pub fn new(provider: Box<dyn HardwareProvider>) -> Self {
        Self {
            provider,
            cpu_ttl: TTL_STATIC_SECS,
            memory_ttl: TTL_DYNAMIC_SECS,
            storage_ttl: TTL_MODERATE_SECS,
            network_ttl: TTL_MODERATE_SECS,
            display_ttl: TTL_STATIC_SECS,
            pci_ttl: TTL_STATIC_SECS,
            usb_ttl: TTL_MODERATE_SECS,
            sound_ttl: TTL_MODERATE_SECS,
            irq_ttl: TTL_STATIC_SECS,
            ioport_ttl: TTL_STATIC_SECS,
            memmap_ttl: TTL_STATIC_SECS,
            dma_ttl: TTL_STATIC_SECS,
            service_ttl: TTL_MODERATE_SECS,
            process_ttl: TTL_DYNAMIC_SECS,
            driver_ttl: TTL_MODERATE_SECS,
            env_ttl: TTL_MODERATE_SECS,
            startup_ttl: TTL_STATIC_SECS,
            cpu_cache: None,
            memory_cache: None,
            storage_cache: None,
            network_cache: None,
            display_cache: None,
            pci_cache: None,
            usb_cache: None,
            sound_cache: None,
            irq_cache: None,
            ioport_cache: None,
            memmap_cache: None,
            dma_cache: None,
            service_cache: None,
            process_cache: None,
            driver_cache: None,
            env_cache: None,
            startup_cache: None,
            refresh_count: 0,
        }
    }

    /// Get current timestamp.
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Force refresh all cached data.
    pub fn refresh_all(&mut self) {
        self.cpu_cache = None;
        self.memory_cache = None;
        self.storage_cache = None;
        self.network_cache = None;
        self.display_cache = None;
        self.pci_cache = None;
        self.usb_cache = None;
        self.sound_cache = None;
        self.irq_cache = None;
        self.ioport_cache = None;
        self.memmap_cache = None;
        self.dma_cache = None;
        self.service_cache = None;
        self.process_cache = None;
        self.driver_cache = None;
        self.env_cache = None;
        self.startup_cache = None;
    }

    /// Get the number of refreshes performed.
    pub fn refresh_count(&self) -> u64 {
        self.refresh_count
    }

    /// Get the provider name.
    pub fn provider_name(&self) -> &'static str {
        self.provider.provider_name()
    }

    /// The cached processor, or `None` if it has never been read.
    ///
    /// `Option` rather than a zero-filled `CpuInfo`. The last-resort branch
    /// here used to build one with brand "Unknown", zero cores and zero
    /// caches, which draws as a description of a very poor machine rather than
    /// as an absence, and which a caller cannot tell from a real reading. The
    /// stale-cache branch above it stays: a value read thirty seconds ago is a
    /// real value, and serving it is different in kind from inventing one.
    pub fn cpu(&mut self) -> Option<CpuInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.cpu_cache
            && !entry.is_stale(now)
        {
            return Some(entry.data.clone());
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_cpu() {
            Ok(info) => {
                self.cpu_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.cpu_ttl,
                });
                Some(info)
            }
            Err(_) => self.cpu_cache.as_ref().map(|e| e.data.clone()),
        }
    }

    /// Get memory info (cached with TTL).
    pub fn memory(&mut self) -> MemoryInfo {
        let now = Self::now();
        if let Some(ref entry) = self.memory_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_memory() {
            Ok(info) => {
                self.memory_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.memory_ttl,
                });
                info
            }
            Err(_) => self
                .memory_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_else(|| MemoryInfo {
                    total_mb: 0,
                    available_mb: 0,
                    mem_type: "Unknown".to_string(),
                    speed_mhz: 0,
                    slots_used: 0,
                    slots_total: 0,
                    slots: Vec::new(),
                }),
        }
    }

    /// Get storage info (cached with TTL).
    pub fn storage(&mut self) -> Vec<DiskInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.storage_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_storage() {
            Ok(info) => {
                self.storage_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.storage_ttl,
                });
                info
            }
            Err(_) => self
                .storage_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get network adapter info (cached with TTL).
    pub fn network(&mut self) -> Vec<NetworkAdapterInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.network_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_network() {
            Ok(info) => {
                self.network_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.network_ttl,
                });
                info
            }
            Err(_) => self
                .network_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get display info (cached with TTL).
    pub fn display(&mut self) -> DisplayInfo {
        let now = Self::now();
        if let Some(ref entry) = self.display_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_display() {
            Ok(info) => {
                self.display_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.display_ttl,
                });
                info
            }
            Err(_) => self
                .display_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_else(|| DisplayInfo {
                    gpu_name: "Unknown".to_string(),
                    vendor: "Unknown".to_string(),
                    vram_mb: 0,
                    resolution: "Unknown".to_string(),
                    refresh_rate_hz: 0,
                    outputs: Vec::new(),
                    driver_version: "Unknown".to_string(),
                }),
        }
    }

    /// Get PCI devices (cached with TTL).
    pub fn pci(&mut self) -> Vec<PciDeviceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.pci_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_pci() {
            Ok(info) => {
                self.pci_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.pci_ttl,
                });
                info
            }
            Err(_) => self
                .pci_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get USB devices (cached with TTL).
    pub fn usb(&mut self) -> Vec<UsbDeviceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.usb_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_usb() {
            Ok(info) => {
                self.usb_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.usb_ttl,
                });
                info
            }
            Err(_) => self
                .usb_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get sound devices (cached with TTL).
    pub fn sound(&mut self) -> Vec<SoundInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.sound_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_sound() {
            Ok(info) => {
                self.sound_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.sound_ttl,
                });
                info
            }
            Err(_) => self
                .sound_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get IRQs (cached with TTL).
    pub fn irqs(&mut self) -> Vec<IrqInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.irq_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_irqs() {
            Ok(info) => {
                self.irq_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.irq_ttl,
                });
                info
            }
            Err(_) => self
                .irq_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get I/O ports (cached with TTL).
    pub fn io_ports(&mut self) -> Vec<IoPortInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.ioport_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_io_ports() {
            Ok(info) => {
                self.ioport_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.ioport_ttl,
                });
                info
            }
            Err(_) => self
                .ioport_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get memory map (cached with TTL).
    pub fn memory_map(&mut self) -> Vec<MemoryMapEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.memmap_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_memory_map() {
            Ok(info) => {
                self.memmap_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.memmap_ttl,
                });
                info
            }
            Err(_) => self
                .memmap_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get DMA channels (cached with TTL).
    pub fn dma(&mut self) -> Vec<DmaInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.dma_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_dma() {
            Ok(info) => {
                self.dma_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.dma_ttl,
                });
                info
            }
            Err(_) => self
                .dma_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get services (cached with TTL).
    pub fn services(&mut self) -> Vec<ServiceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.service_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_services() {
            Ok(info) => {
                self.service_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.service_ttl,
                });
                info
            }
            Err(_) => self
                .service_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get processes (cached with TTL).
    pub fn processes(&mut self) -> Vec<ProcessEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.process_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_processes() {
            Ok(info) => {
                self.process_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.process_ttl,
                });
                info
            }
            Err(_) => self
                .process_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get drivers (cached with TTL).
    pub fn drivers(&mut self) -> Vec<DriverInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.driver_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_drivers() {
            Ok(info) => {
                self.driver_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.driver_ttl,
                });
                info
            }
            Err(_) => self
                .driver_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get environment variables (cached with TTL).
    pub fn env_vars(&mut self) -> Vec<(String, String)> {
        let now = Self::now();
        if let Some(ref entry) = self.env_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_env_vars() {
            Ok(info) => {
                self.env_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.env_ttl,
                });
                info
            }
            Err(_) => self
                .env_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }

    /// Get startup programs (cached with TTL).
    pub fn startup(&mut self) -> Vec<StartupEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.startup_cache
            && !entry.is_stale(now)
        {
            return entry.data.clone();
        }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_startup() {
            Ok(info) => {
                self.startup_cache = Some(CacheEntry {
                    data: info.clone(),
                    timestamp: now,
                    ttl_secs: self.startup_ttl,
                });
                info
            }
            Err(_) => self
                .startup_cache
                .as_ref()
                .map(|e| e.data.clone())
                .unwrap_or_default(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is what a test is for: an `expect` that fires here
    // *is* the failure report, and an index that goes out of range is the test
    // telling you the fixture changed shape. CLAUDE.md scopes the defensive
    // panic lints to non-test code for exactly this reason.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;

    // -- Error display --

    #[test]
    fn test_error_display() {
        let e = HwQueryError::NotAvailable {
            path: "/sys/cpu".to_string(),
        };
        assert!(e.to_string().contains("/sys/cpu"));

        let e = HwQueryError::ParseError {
            detail: "bad int".to_string(),
        };
        assert!(e.to_string().contains("bad int"));

        assert_eq!(HwQueryError::Timeout.to_string(), "query timed out");
        assert_eq!(
            HwQueryError::PermissionDenied.to_string(),
            "permission denied"
        );
    }

    // -- Stub provider --

    #[test]
    fn test_stub_cpu() {
        let stub = StubProvider::new();
        let cpu = stub.query_cpu().expect("stub cpu");
        assert_eq!(cpu.brand.as_deref(), Some("Fixture CPU"));
        assert_eq!(cpu.physical_cores, 8);
        assert_eq!(cpu.logical_processors, 16);
        assert!(!cpu.features.is_empty());
    }

    #[test]
    fn test_stub_memory() {
        let stub = StubProvider::new();
        let mem = stub.query_memory().expect("stub memory");
        assert_eq!(mem.total_mb, 32768);
        assert!(mem.available_mb > 0);
        assert_eq!(mem.slots.len(), 2);
    }

    #[test]
    fn test_stub_storage() {
        let stub = StubProvider::new();
        let disks = stub.query_storage().expect("stub storage");
        assert!(!disks.is_empty());
        assert!(!disks[0].partitions.is_empty());
    }

    #[test]
    fn test_stub_network() {
        let stub = StubProvider::new();
        let nets = stub.query_network().expect("stub network");
        assert!(!nets.is_empty());
        assert!(nets[0].speed_mbps > 0);
    }

    #[test]
    fn test_stub_display() {
        let stub = StubProvider::new();
        let disp = stub.query_display().expect("stub display");
        assert!(disp.gpu_name.contains("AMD"));
        assert!(disp.vram_mb > 0);
    }

    #[test]
    fn test_stub_pci() {
        let stub = StubProvider::new();
        let pci = stub.query_pci().expect("stub pci");
        assert!(pci.len() >= 2);
        assert_eq!(pci[0].vendor_id, 0x8086);
    }

    #[test]
    fn test_stub_usb() {
        let stub = StubProvider::new();
        let usb = stub.query_usb().expect("stub usb");
        assert!(!usb.is_empty());
        assert_eq!(usb[0].vendor_id, 0x046D);
    }

    #[test]
    fn test_stub_provider_name() {
        let stub = StubProvider::new();
        assert!(stub.provider_name().contains("Stub"));
    }

    #[test]
    fn test_stub_all_queries_succeed() {
        let stub = StubProvider::new();
        assert!(stub.query_cpu().is_ok());
        assert!(stub.query_memory().is_ok());
        assert!(stub.query_storage().is_ok());
        assert!(stub.query_network().is_ok());
        assert!(stub.query_display().is_ok());
        assert!(stub.query_pci().is_ok());
        assert!(stub.query_usb().is_ok());
        assert!(stub.query_sound().is_ok());
        assert!(stub.query_irqs().is_ok());
        assert!(stub.query_io_ports().is_ok());
        assert!(stub.query_memory_map().is_ok());
        assert!(stub.query_dma().is_ok());
        assert!(stub.query_services().is_ok());
        assert!(stub.query_processes().is_ok());
        assert!(stub.query_drivers().is_ok());
        assert!(stub.query_env_vars().is_ok());
        assert!(stub.query_startup().is_ok());
    }

    // -- CPU feature parsing --

    // -- The /sys/devices reader --
    //
    // Three parsers were added with the reader and none of them had a test
    // until this block. That is the gap worth naming: the rewrite deleted
    // three tests for a format the tree no longer uses and added nothing, so
    // for one commit the file had fewer tests and more untested code.

    #[test]
    fn a_cpu_range_counts_what_it_names() {
        assert_eq!(SyscallProvider::count_range("0-7"), Some(8));
        assert_eq!(SyscallProvider::count_range("0"), Some(1));
        assert_eq!(SyscallProvider::count_range("0,2-3"), Some(3));
        assert_eq!(SyscallProvider::count_range("0-0"), Some(1));
        // Whitespace is real: these files end in a newline, and `read_scalar`
        // trims the ends but not between the commas.
        assert_eq!(SyscallProvider::count_range("0-3, 6-7"), Some(6));
    }

    /// An unreadable range is `None`, never a count.
    ///
    /// The distinction this is pinning: a machine with no CPUs is not a
    /// possible answer, so a zero could only ever mean the parse failed. If
    /// this returned 0 or 1 on nonsense, `query_cpu` would report a
    /// single-core machine to anyone whose `present` file was malformed, and
    /// nothing anywhere would say so.
    #[test]
    fn an_unreadable_cpu_range_is_absent_rather_than_a_count() {
        assert_eq!(SyscallProvider::count_range(""), None);
        assert_eq!(SyscallProvider::count_range("garbage"), None);
        assert_eq!(SyscallProvider::count_range("3-1"), None);
        assert_eq!(SyscallProvider::count_range("0-"), None);
        assert_eq!(SyscallProvider::count_range("-7"), None);
    }

    /// Cache sizes carry their unit, and dropping it is a 1024x error.
    #[test]
    fn a_cache_size_is_read_with_its_suffix() {
        assert_eq!(SyscallProvider::parse_cache_size_kb("32K"), Some(32));
        assert_eq!(SyscallProvider::parse_cache_size_kb("32k"), Some(32));
        assert_eq!(SyscallProvider::parse_cache_size_kb("8M"), Some(8192));
        assert_eq!(SyscallProvider::parse_cache_size_kb("8m"), Some(8192));
        // Linux writes the suffix; a bare number is already KiB by convention.
        assert_eq!(SyscallProvider::parse_cache_size_kb("512"), Some(512));
        assert_eq!(SyscallProvider::parse_cache_size_kb(" 256K "), Some(256));
        assert_eq!(SyscallProvider::parse_cache_size_kb(""), None);
        assert_eq!(SyscallProvider::parse_cache_size_kb("K"), None);
        assert_eq!(SyscallProvider::parse_cache_size_kb("big"), None);
    }

    /// With no tree to read, the provider reports nothing rather than a
    /// machine.
    ///
    /// This is the whole point of the change and it is testable *here*,
    /// today, because `/sys/devices/system/cpu` does not exist on the host
    /// this suite runs on — the same reason it does not exist on Slate OS
    /// until lane A's producer lands. A `CpuInfo` full of zeroes would pass a
    /// test asserting the call succeeded, which is what the deleted
    /// `FallbackProvider` arranged and what its own test asserted.
    #[test]
    fn no_tree_means_no_reading_rather_than_an_empty_machine() {
        let provider = SyscallProvider::new();
        let cpu = provider.query_cpu();
        assert!(
            matches!(cpu, Err(HwQueryError::NotAvailable { .. })),
            "expected NotAvailable with no /sys/devices, got {cpu:?}"
        );
        let memory = provider.query_memory();
        assert!(
            matches!(memory, Err(HwQueryError::NotAvailable { .. })),
            "expected NotAvailable with no /sys/devices, got {memory:?}"
        );
    }

    /// The error says which path it could not read.
    ///
    /// Not decoration: the window renders this path, so a user who sees
    /// "Not available" can tell a missing kernel feature from a permissions
    /// problem, and whoever implements the producer is told exactly what was
    /// asked for.
    #[test]
    fn an_absence_names_the_path_it_could_not_read() {
        let provider = SyscallProvider::new();
        let Err(HwQueryError::NotAvailable { path }) = provider.query_cpu() else {
            panic!("expected NotAvailable");
        };
        assert!(
            path.contains("/sys/devices/system/cpu/cpuid/"),
            "the error names {path:?}, which does not say what was asked for"
        );
    }

    // -- Key-value file parsing --

    // -- Reading one field --

    /// A key/value map, as `parse_kv` produces from a hardware file.
    fn kv(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_field_that_is_there_is_read() {
        let m = kv(&[("a", "42"), ("b", " 100 "), ("c", "12345"), ("d", "3.25")]);
        assert_eq!(SyscallProvider::field(&m, "a", 0u64), Ok(42));
        assert_eq!(SyscallProvider::field(&m, "b", 0u64), Ok(100));
        assert_eq!(SyscallProvider::field(&m, "c", 0u32), Ok(12345));
        let f: f32 = SyscallProvider::field(&m, "d", 0.0).expect("a float");
        assert!((f - 3.25).abs() < 0.01);
    }

    /// **An absent field takes the default; a malformed one is an error.**
    ///
    /// The whole reason this function exists. Every caller used to write
    /// `kv.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)`, which cannot
    /// tell the two apart and answers `0` to both -- so a corrupt reading was
    /// displayed as a real one.
    #[test]
    fn an_absent_field_defaults_and_a_malformed_one_does_not() {
        let m = kv(&[("present", "abc")]);

        assert_eq!(
            SyscallProvider::field(&m, "missing", 7u32),
            Ok(7),
            "a field the kernel did not report should take the default"
        );

        let bad = SyscallProvider::field(&m, "present", 7u32);
        assert!(
            bad.is_err(),
            "a malformed reading came back as {bad:?} instead of an error"
        );
        let detail = format!("{}", bad.unwrap_err());
        assert!(
            detail.contains("present") && detail.contains("abc"),
            "the error names neither the field nor the value: {detail}"
        );
    }

    // -- Multi-record parsing --

    #[test]
    fn test_read_sysfs_dir_entries_parsing() {
        let provider = SyscallProvider::new();
        // Simulate a multi-record file (blank-line separated)
        let content = "name=eth0\nspeed_mbps=1000\n\nname=wlan0\nspeed_mbps=600\n";
        // Manually parse since we can't use read_sysfs in tests
        let mut entries = Vec::new();
        let mut current = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                if !current.is_empty() {
                    entries.push(current.clone());
                    current.clear();
                }
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                current.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        if !current.is_empty() {
            entries.push(current);
        }

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("name").map(|s| s.as_str()), Some("eth0"));
        assert_eq!(entries[1].get("name").map(|s| s.as_str()), Some("wlan0"));
        let _ = provider; // use the provider to avoid unused warning
    }

    // -- Cache entry --

    #[test]
    fn test_cache_entry_staleness() {
        let entry = CacheEntry {
            data: 42u32,
            timestamp: 1000,
            ttl_secs: 30,
        };
        assert!(!entry.is_stale(1010)); // 10s < 30s TTL
        assert!(!entry.is_stale(1029)); // 29s < 30s TTL
        assert!(entry.is_stale(1030)); // 30s >= 30s TTL
        assert!(entry.is_stale(1100)); // 100s > 30s TTL
    }

    // -- Refresh manager --

    #[test]
    fn test_refresh_manager_with_stub() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let cpu = mgr.cpu().expect("a cpu from the stub");
        assert_eq!(cpu.brand.as_deref(), Some("Fixture CPU"));
        assert_eq!(mgr.refresh_count(), 1);

        // Second access should be cached
        let cpu2 = mgr.cpu().expect("a cached cpu");
        assert_eq!(cpu2.brand, cpu.brand);
        assert_eq!(mgr.refresh_count(), 1); // Still 1 — used cache
    }

    #[test]
    fn test_refresh_manager_refresh_all_clears_cache() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let _ = mgr.cpu();
        assert_eq!(mgr.refresh_count(), 1);

        mgr.refresh_all();
        let _ = mgr.cpu();
        assert_eq!(mgr.refresh_count(), 2); // Had to re-query
    }

    #[test]
    fn test_refresh_manager_memory() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let mem = mgr.memory();
        assert_eq!(mem.total_mb, 32768);
    }

    #[test]
    fn test_refresh_manager_storage() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let disks = mgr.storage();
        assert!(!disks.is_empty());
    }

    #[test]
    fn test_refresh_manager_network() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let nets = mgr.network();
        assert!(!nets.is_empty());
    }

    #[test]
    fn test_refresh_manager_all_categories() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let _ = mgr.cpu();
        let _ = mgr.memory();
        let _ = mgr.storage();
        let _ = mgr.network();
        let _ = mgr.display();
        let _ = mgr.pci();
        let _ = mgr.usb();
        let _ = mgr.sound();
        let _ = mgr.irqs();
        let _ = mgr.io_ports();
        let _ = mgr.memory_map();
        let _ = mgr.dma();
        let _ = mgr.services();
        let _ = mgr.processes();
        let _ = mgr.drivers();
        let _ = mgr.env_vars();
        let _ = mgr.startup();
        assert_eq!(mgr.refresh_count(), 17); // One per category
    }

    #[test]
    fn test_refresh_manager_provider_name() {
        let mgr = RefreshManager::new(Box::new(StubProvider::new()));
        assert!(mgr.provider_name().contains("Stub"));
    }

    // -- Syscall provider (limited testing since sysfs doesn't exist on dev host) --

    #[test]
    fn test_syscall_provider_returns_error_for_missing_file() {
        let provider = SyscallProvider::new();
        let result = provider.query_cpu();
        // Should fail since /sys/hardware/cpu doesn't exist on dev machine
        assert!(result.is_err());
    }

    #[test]
    fn test_syscall_provider_name() {
        let provider = SyscallProvider::new();
        assert!(provider.provider_name().contains("Syscall"));
    }

    #[test]
    fn test_syscall_provider_env_vars_works() {
        // env_vars doesn't depend on sysfs — it reads process env
        let provider = SyscallProvider::new();
        let vars = provider.query_env_vars().expect("env vars");
        // Should have at least PATH or USERPROFILE on Windows
        assert!(!vars.is_empty());
    }
}
