//! System information explorer.
//!
//! Comprehensive hardware and OS information display, inspired by
//! tools like HWiNFO, CPU-Z, and the Windows System Information panel.
//!
//! ## Design Reference
//!
//! design.txt line 886: "comprehensive system information explorer
//!   (for hardware) - also includes a page about the OS - name, version,
//!   web link, maybe certain options, especially ones that require
//!   recompilation like paging model/params, scheduling model/params,
//!   filesystem params, and memory management model/params, maybe
//!   mounted drives and network drives and show capacity and free space"
//!
//! ## Architecture
//!
//! ```text
//! Settings / System Info panel
//!   → sysinfo::os_info()
//!   → sysinfo::cpu_info()
//!   → sysinfo::memory_info()
//!   → sysinfo::storage_info()
//!   → sysinfo::network_info()
//!   → sysinfo::gpu_info()
//!   → sysinfo::kernel_params()
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// OS version and identity.
#[derive(Debug, Clone)]
pub struct OsInfo {
    /// OS name.
    pub name: String,
    /// Version string (e.g. "1.0.0").
    pub version: String,
    /// Build number.
    pub build_number: u64,
    /// Build date.
    pub build_date: String,
    /// Codename (e.g. "Mint").
    pub codename: String,
    /// Architecture (e.g. "x86_64").
    pub arch: String,
    /// Kernel version.
    pub kernel_version: String,
    /// Website URL.
    pub website: String,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Boot timestamp (ns).
    pub boot_ns: u64,
}

/// CPU information.
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// Processor name / model.
    pub model: String,
    /// Vendor string.
    pub vendor: String,
    /// Number of physical cores.
    pub cores: u32,
    /// Number of logical processors (threads).
    pub threads: u32,
    /// Base frequency (MHz).
    pub base_freq_mhz: u32,
    /// Max boost frequency (MHz).
    pub max_freq_mhz: u32,
    /// L1 data cache size (KiB).
    pub l1d_cache_kib: u32,
    /// L1 instruction cache size (KiB).
    pub l1i_cache_kib: u32,
    /// L2 cache size (KiB).
    pub l2_cache_kib: u32,
    /// L3 cache size (KiB).
    pub l3_cache_kib: u32,
    /// Supported features (e.g. SSE4.2, AVX2, AES-NI).
    pub features: Vec<String>,
    /// CPU family.
    pub family: u32,
    /// CPU model number.
    pub model_num: u32,
    /// Stepping.
    pub stepping: u32,
}

/// Memory information.
#[derive(Debug, Clone)]
pub struct MemoryInfo {
    /// Total physical RAM (bytes).
    pub total_bytes: u64,
    /// Used physical RAM (bytes).
    pub used_bytes: u64,
    /// Available for allocation (bytes).
    pub available_bytes: u64,
    /// Total swap (bytes).
    pub swap_total: u64,
    /// Used swap (bytes).
    pub swap_used: u64,
    /// Number of DIMM slots populated.
    pub dimm_count: u32,
    /// Memory type (e.g. "DDR5").
    pub mem_type: String,
    /// Speed (MT/s).
    pub speed_mts: u32,
}

/// Storage device / partition info.
#[derive(Debug, Clone)]
pub struct StorageDevice {
    /// Device name (e.g. "/dev/sda").
    pub device: String,
    /// Model / product name.
    pub model: String,
    /// Total capacity (bytes).
    pub capacity_bytes: u64,
    /// Free space (bytes).
    pub free_bytes: u64,
    /// Filesystem type (e.g. "ext4").
    pub fs_type: String,
    /// Mount point.
    pub mount_point: String,
    /// Whether this is a network drive.
    pub network: bool,
    /// Whether this is removable.
    pub removable: bool,
    /// Interface type (e.g. "NVMe", "SATA", "USB").
    pub interface: String,
}

/// GPU information.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// GPU name.
    pub name: String,
    /// Vendor.
    pub vendor: String,
    /// VRAM size (MiB).
    pub vram_mib: u32,
    /// Driver version.
    pub driver_version: String,
    /// API support (e.g. "Vulkan 1.3", "OpenGL 4.6").
    pub api_support: Vec<String>,
    /// Current resolution.
    pub resolution: String,
    /// Refresh rate (Hz).
    pub refresh_hz: u32,
}

/// Network interface info (summary for sysinfo).
#[derive(Debug, Clone)]
pub struct NetIfaceInfo {
    /// Interface name.
    pub name: String,
    /// Type description.
    pub iface_type: String,
    /// IP address.
    pub ip_address: String,
    /// MAC address.
    pub mac_address: String,
    /// Link speed (Mbps).
    pub speed_mbps: u32,
    /// Whether connected.
    pub connected: bool,
}

/// Kernel compile-time parameters (per design.txt: "options that
/// require recompilation").
#[derive(Debug, Clone)]
pub struct KernelParams {
    /// Page size (bytes).
    pub page_size: u32,
    /// Scheduling model.
    pub sched_model: String,
    /// Preemption model.
    pub preempt_model: String,
    /// Memory allocator model.
    pub alloc_model: String,
    /// Overcommit mode.
    pub overcommit_mode: String,
    /// Max CPU count.
    pub max_cpus: u32,
    /// Filesystem type for root.
    pub root_fs: String,
    /// Debug assertions enabled.
    pub debug_assertions: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    os: OsInfo,
    cpu: CpuInfo,
    memory: MemoryInfo,
    storage: Vec<StorageDevice>,
    gpus: Vec<GpuInfo>,
    net_ifaces: Vec<NetIfaceInfo>,
    kernel_params: KernelParams,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    os: OsInfo {
        name: String::new(),
        version: String::new(),
        build_number: 0,
        build_date: String::new(),
        codename: String::new(),
        arch: String::new(),
        kernel_version: String::new(),
        website: String::new(),
        uptime_secs: 0,
        boot_ns: 0,
    },
    cpu: CpuInfo {
        model: String::new(),
        vendor: String::new(),
        cores: 0,
        threads: 0,
        base_freq_mhz: 0,
        max_freq_mhz: 0,
        l1d_cache_kib: 0,
        l1i_cache_kib: 0,
        l2_cache_kib: 0,
        l3_cache_kib: 0,
        features: Vec::new(),
        family: 0,
        model_num: 0,
        stepping: 0,
    },
    memory: MemoryInfo {
        total_bytes: 0,
        used_bytes: 0,
        available_bytes: 0,
        swap_total: 0,
        swap_used: 0,
        dimm_count: 0,
        mem_type: String::new(),
        speed_mts: 0,
    },
    storage: Vec::new(),
    gpus: Vec::new(),
    net_ifaces: Vec::new(),
    kernel_params: KernelParams {
        page_size: 16384,
        sched_model: String::new(),
        preempt_model: String::new(),
        alloc_model: String::new(),
        overcommit_mode: String::new(),
        max_cpus: 256,
        root_fs: String::new(),
        debug_assertions: false,
    },
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Getters
// ---------------------------------------------------------------------------

/// Get OS information.
pub fn os_info() -> OsInfo {
    let mut info = STATE.lock().os.clone();
    // Compute uptime from boot timestamp.
    if info.boot_ns > 0 {
        let now = crate::hpet::elapsed_ns();
        if now > info.boot_ns {
            info.uptime_secs = (now - info.boot_ns) / 1_000_000_000;
        }
    }
    info
}

/// Get CPU information.
pub fn cpu_info() -> CpuInfo {
    STATE.lock().cpu.clone()
}

/// Get memory information.
pub fn memory_info() -> MemoryInfo {
    STATE.lock().memory.clone()
}

/// Get storage devices.
pub fn storage_info() -> Vec<StorageDevice> {
    STATE.lock().storage.clone()
}

/// Get GPU information.
pub fn gpu_info() -> Vec<GpuInfo> {
    STATE.lock().gpus.clone()
}

/// Get network interfaces.
pub fn network_info() -> Vec<NetIfaceInfo> {
    STATE.lock().net_ifaces.clone()
}

/// Get kernel parameters.
pub fn kernel_params() -> KernelParams {
    STATE.lock().kernel_params.clone()
}

// ---------------------------------------------------------------------------
// Setters (populated by kernel/drivers at boot)
// ---------------------------------------------------------------------------

/// Set OS information.
pub fn set_os_info(info: OsInfo) {
    let mut state = STATE.lock();
    state.os = info;
    state.changes += 1;
}

/// Set CPU information.
pub fn set_cpu_info(info: CpuInfo) {
    let mut state = STATE.lock();
    state.cpu = info;
    state.changes += 1;
}

/// Set memory information.
pub fn set_memory_info(info: MemoryInfo) {
    let mut state = STATE.lock();
    state.memory = info;
    state.changes += 1;
}

/// Add a storage device.
pub fn add_storage(dev: StorageDevice) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.storage.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    state.storage.push(dev);
    state.changes += 1;
    Ok(())
}

/// Add a GPU.
pub fn add_gpu(gpu: GpuInfo) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.gpus.len() >= 8 {
        return Err(KernelError::ResourceExhausted);
    }
    state.gpus.push(gpu);
    state.changes += 1;
    Ok(())
}

/// Add a network interface.
pub fn add_net_iface(iface: NetIfaceInfo) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.net_ifaces.len() >= 32 {
        return Err(KernelError::ResourceExhausted);
    }
    state.net_ifaces.push(iface);
    state.changes += 1;
    Ok(())
}

/// Set kernel parameters.
pub fn set_kernel_params(params: KernelParams) {
    let mut state = STATE.lock();
    state.kernel_params = params;
    state.changes += 1;
}

/// Update memory usage (called periodically).
pub fn update_memory(used: u64, available: u64, swap_used: u64) {
    let mut state = STATE.lock();
    state.memory.used_bytes = used;
    state.memory.available_bytes = available;
    state.memory.swap_used = swap_used;
}

/// Update storage free space.
pub fn update_storage_free(device: &str, free_bytes: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let dev = state.storage.iter_mut().find(|s| s.device == device)
        .ok_or(KernelError::NotFound)?;
    dev.free_bytes = free_bytes;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_099_511_627_776 {
        format!("{:.1} TiB", bytes as f64 / 1_099_511_627_776.0)
    } else if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with simulated hardware information.
/// Convert a fixed CPUID byte buffer (vendor / brand string) to a trimmed
/// UTF-8 `String`, stopping at the first NUL.  Returns an empty string when the
/// buffer is not valid UTF-8 (never fabricates a placeholder).
fn cpuid_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = bytes
        .get(..end)
        .and_then(|sl| core::str::from_utf8(sl).ok())
        .unwrap_or("");
    String::from(s.trim())
}

/// Build [`CpuInfo`] from **live** CPUID + topology detection
/// (`crate::cpu`, `crate::cpu_topology`, `crate::smp`).
///
/// Fields with no reliable detection source (base/boost frequency — CPUID
/// leaf 0x16 is absent under TCG and many hypervisors) are left ZERO rather
/// than fabricated.  The feature list contains only flags the detector
/// actually observed.
fn detect_cpu() -> CpuInfo {
    let vendor = cpuid_str(&crate::cpu::vendor_string());
    let model = cpuid_str(&crate::cpu::brand_string());
    let (family, model_num, stepping) = crate::cpu::cpu_family_model_stepping();
    let cores = u32::try_from(crate::cpu_topology::num_physical_cores()).unwrap_or(0);
    let threads = u32::try_from(crate::smp::cpu_count().max(1)).unwrap_or(1);

    // Map the detected cache topology onto the L1d/L1i/L2/L3 slots (bytes → KiB).
    let (mut l1d, mut l1i, mut l2, mut l3) = (0u32, 0u32, 0u32, 0u32);
    for c in crate::cpu::cache_topology() {
        let kib = c.size / 1024;
        match (c.level, c.cache_type) {
            (1, 1) => l1d = kib,            // L1 data.
            (1, 2) => l1i = kib,            // L1 instruction.
            (1, 3) => { l1d = kib; l1i = kib; } // Unified L1 (rare).
            (2, _) => l2 = kib,
            (3, _) => l3 = kib,
            _ => {}
        }
    }

    // Surface only the ISA features the detector actually saw.
    let mut features = Vec::new();
    if let Some(f) = crate::cpu::features() {
        let flags: &[(bool, &str)] = &[
            (f.sse3, "SSE3"), (f.ssse3, "SSSE3"), (f.sse4_1, "SSE4.1"),
            (f.sse4_2, "SSE4.2"), (f.popcnt, "POPCNT"), (f.avx, "AVX"),
            (f.avx2, "AVX2"), (f.avx512f, "AVX-512F"), (f.aes_ni, "AES-NI"),
            (f.sha, "SHA"), (f.rdrand, "RDRAND"), (f.rdseed, "RDSEED"),
            (f.rdtscp, "RDTSCP"), (f.smep, "SMEP"), (f.smap, "SMAP"),
        ];
        for &(present, name) in flags {
            if present { features.push(String::from(name)); }
        }
    }

    CpuInfo {
        model, vendor, cores, threads,
        base_freq_mhz: 0,
        max_freq_mhz: 0,
        l1d_cache_kib: l1d, l1i_cache_kib: l1i, l2_cache_kib: l2, l3_cache_kib: l3,
        features, family, model_num, stepping,
    }
}

/// Build [`MemoryInfo`] from the **real** buddy-allocator statistics
/// (`crate::mm::frame::stats`).  Swap and DIMM/SMBIOS details are left zero/empty
/// because no swap subsystem or SMBIOS parser is wired for inventory yet — they
/// are NOT fabricated.
fn detect_memory() -> MemoryInfo {
    let (total, available) = match crate::mm::frame::stats() {
        Some(s) => {
            let total = (s.total_frames as u64)
                .saturating_mul(crate::mm::frame::FRAME_SIZE as u64);
            (total, s.free_bytes as u64)
        }
        None => (0, 0),
    };
    MemoryInfo {
        total_bytes: total,
        used_bytes: total.saturating_sub(available),
        available_bytes: available,
        swap_total: 0,
        swap_used: 0,
        dimm_count: 0,
        mem_type: String::new(),
        speed_mts: 0,
    }
}

/// Populate the system-info snapshot with **real** boot-time facts.
///
/// - **OS** identity (name/version/codename/arch/website) is the OS build's own
///   declared identity — legitimate configuration, analogous to `/etc/os-release`.
/// - **CPU** and **Memory** are read through to live detection ([`detect_cpu`],
///   [`detect_memory`]) — no invented core counts, caches, or RAM totals.
/// - **Kernel params** are true kernel facts (page size from
///   `frame::FRAME_SIZE`, the real scheduler / allocator / overcommit models,
///   `debug_assertions` from the actual build profile).
/// - **Storage / GPU / Network** are left EMPTY rather than fabricated; they
///   will be filled in once the block layer, GPU driver, and NIC stack expose
///   real enumeration (see DEFERRED PROPER FIX in todo.txt).
///
/// (Previously this seeded an entirely FABRICATED machine spec that
/// `/proc/sysinfo` and the `sysinfo` kshell command surfaced as the real
/// hardware: a "Generic x86_64 Processor" with a hard-coded 8 cores / 16
/// threads, 3.6/5.0 GHz, 32/32/256/16384 KiB caches and a fixed feature list;
/// 16 GiB / 4 GiB-used DDR5-5600 across "2 DIMMs"; two "Generic NVMe SSD"
/// partitions (`/dev/nvme0n1p1` at `/`, `p2` at `/home`); a "Generic GPU" with
/// 8 GiB VRAM at 1920x1080 144 Hz; and `eth0` at `192.168.1.100` with a fake
/// MAC plus a `wlan0`.  All of it was fiction — a direct violation of the
/// kernel's "never invent data in procfs" rule for a panel whose entire purpose
/// is to report the user's actual hardware.)
pub fn init_defaults() {
    let mut state = STATE.lock();

    state.os = OsInfo {
        name: String::from("MintOS"),
        version: String::from("1.0.0"),
        build_number: 1,
        build_date: String::from("2026-05-06"),
        codename: String::from("Mint"),
        arch: String::from("x86_64"),
        kernel_version: String::from("1.0.0"),
        website: String::from("https://mintos.dev"),
        uptime_secs: crate::hpet::elapsed_ns() / 1_000_000_000,
        boot_ns: crate::hpet::elapsed_ns(),
    };

    state.cpu = detect_cpu();
    state.memory = detect_memory();

    // Storage / GPU / Network inventory is not yet wired — leave empty rather
    // than fabricate. (DEFERRED PROPER FIX: read storage from the mount table /
    // block layer, GPUs from PCI display-class devices, NICs from the net stack.)
    state.storage = Vec::new();
    state.gpus = Vec::new();
    state.net_ifaces = Vec::new();

    state.kernel_params = KernelParams {
        page_size: crate::mm::frame::FRAME_SIZE as u32,
        sched_model: String::from("PriorityRoundRobin"),
        preempt_model: String::from("Full"),
        alloc_model: String::from("Buddy"),
        overcommit_mode: String::from("Never"),
        max_cpus: 256,
        root_fs: String::from("ext4"),
        debug_assertions: cfg!(debug_assertions),
    };

    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (storage_count, gpu_count, net_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    (state.storage.len(),
     state.gpus.len(),
     state.net_ifaces.len(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.os = OsInfo {
        name: String::new(), version: String::new(), build_number: 0,
        build_date: String::new(), codename: String::new(), arch: String::new(),
        kernel_version: String::new(), website: String::new(),
        uptime_secs: 0, boot_ns: 0,
    };
    state.cpu = CpuInfo {
        model: String::new(), vendor: String::new(),
        cores: 0, threads: 0, base_freq_mhz: 0, max_freq_mhz: 0,
        l1d_cache_kib: 0, l1i_cache_kib: 0, l2_cache_kib: 0, l3_cache_kib: 0,
        features: Vec::new(), family: 0, model_num: 0, stepping: 0,
    };
    state.memory = MemoryInfo {
        total_bytes: 0, used_bytes: 0, available_bytes: 0,
        swap_total: 0, swap_used: 0, dimm_count: 0,
        mem_type: String::new(), speed_mts: 0,
    };
    state.storage.clear();
    state.gpus.clear();
    state.net_ifaces.clear();
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: init_defaults — OS identity is legitimate build config.
    serial_println!("sysinfo::self_test 1: defaults");
    init_defaults();
    let os = os_info();
    assert_eq!(os.name, "MintOS");
    assert_eq!(os.arch, "x86_64");

    // Test 2: CPU info — READ-THROUGH from live CPUID/topology. Core/thread
    // counts and the feature list are hardware-dependent (and may be empty under
    // a minimal hypervisor CPU), so assert INVARIANTS, not the old fabricated
    // 8-core/16-thread magic numbers: at least one logical CPU, and no invented
    // base/boost frequency.
    serial_println!("sysinfo::self_test 2: CPU");
    let cpu = cpu_info();
    assert!(cpu.threads >= 1);
    assert!(cpu.cores <= cpu.threads || cpu.cores == 0);
    assert_eq!(cpu.base_freq_mhz, 0); // No reliable source — not fabricated.
    assert_eq!(cpu.max_freq_mhz, 0);

    // Test 3: memory info — READ-THROUGH from the real buddy allocator.
    serial_println!("sysinfo::self_test 3: memory");
    let mem = memory_info();
    assert!(mem.total_bytes > 0); // Frame allocator is up by boot.
    assert!(mem.available_bytes <= mem.total_bytes);
    assert_eq!(mem.used_bytes, mem.total_bytes - mem.available_bytes);
    assert_eq!(mem.swap_total, 0); // No swap inventory — not fabricated.
    assert_eq!(mem.mem_type, ""); // No SMBIOS — not a fabricated "DDR5".

    // Test 4: update memory — mutator still works over the real snapshot.
    serial_println!("sysinfo::self_test 4: update memory");
    update_memory(8_000_000_000, 8_000_000_000, 100_000_000);
    let mem = memory_info();
    assert_eq!(mem.used_bytes, 8_000_000_000);
    assert_eq!(mem.swap_used, 100_000_000);

    // Test 5: storage — EMPTY by default now (no fabricated NVMe drives).
    serial_println!("sysinfo::self_test 5: storage");
    assert!(storage_info().is_empty());

    // Test 6: add a storage device, then update its free space.
    serial_println!("sysinfo::self_test 6: add + update storage");
    add_storage(StorageDevice {
        device: String::from("/dev/nvme0n1p1"),
        model: String::from("Test SSD"),
        capacity_bytes: 512 * 1024 * 1024 * 1024,
        free_bytes: 350 * 1024 * 1024 * 1024,
        fs_type: String::from("ext4"),
        mount_point: String::from("/"),
        network: false,
        removable: false,
        interface: String::from("NVMe"),
    })?;
    update_storage_free("/dev/nvme0n1p1", 100_000_000_000)?;
    let devs = storage_info();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].free_bytes, 100_000_000_000);

    // Test 7: GPU — EMPTY by default now (no fabricated "Generic GPU").
    serial_println!("sysinfo::self_test 7: GPU");
    assert!(gpu_info().is_empty());

    // Test 8: network — EMPTY by default now (no fabricated eth0/wlan0).
    serial_println!("sysinfo::self_test 8: network");
    assert!(network_info().is_empty());

    // Test 9: kernel params — true kernel facts.
    serial_println!("sysinfo::self_test 9: kernel params");
    let kp = kernel_params();
    assert_eq!(kp.page_size, 16384);
    assert_eq!(kp.sched_model, "PriorityRoundRobin");

    // Test 10: format_bytes helper.
    serial_println!("sysinfo::self_test 10: format_bytes");
    assert!(format_bytes(1024).contains("KiB"));
    assert!(format_bytes(1_073_741_824).contains("GiB"));

    // Test 11: add extra device — count rises to exactly 2.
    serial_println!("sysinfo::self_test 11: add devices");
    add_storage(StorageDevice {
        device: String::from("/dev/sdb1"),
        model: String::from("USB Drive"),
        capacity_bytes: 32 * 1024 * 1024 * 1024,
        free_bytes: 20 * 1024 * 1024 * 1024,
        fs_type: String::from("fat32"),
        mount_point: String::from("/mnt/usb"),
        network: false,
        removable: true,
        interface: String::from("USB"),
    })?;
    assert_eq!(storage_info().len(), 2);

    // Reset so the test leaves the pristine empty state (sysinfo is not
    // boot-wired, so uninitialised is the natural state for /proc/sysinfo).
    clear_all();
    serial_println!("sysinfo::self_test: all 11 tests passed");
    Ok(())
}
