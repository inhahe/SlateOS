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
use alloc::vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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
        uptime_secs: 0,
        boot_ns: crate::hpet::elapsed_ns(),
    };

    state.cpu = CpuInfo {
        model: String::from("Generic x86_64 Processor"),
        vendor: String::from("GenuineIntel"),
        cores: 8,
        threads: 16,
        base_freq_mhz: 3600,
        max_freq_mhz: 5000,
        l1d_cache_kib: 32,
        l1i_cache_kib: 32,
        l2_cache_kib: 256,
        l3_cache_kib: 16384,
        features: vec![
            String::from("SSE4.2"),
            String::from("AVX2"),
            String::from("AES-NI"),
            String::from("SHA"),
            String::from("BMI2"),
            String::from("FMA"),
        ],
        family: 6,
        model_num: 151,
        stepping: 2,
    };

    state.memory = MemoryInfo {
        total_bytes: 16 * 1024 * 1024 * 1024, // 16 GiB
        used_bytes: 4 * 1024 * 1024 * 1024,
        available_bytes: 12 * 1024 * 1024 * 1024,
        swap_total: 4 * 1024 * 1024 * 1024,
        swap_used: 0,
        dimm_count: 2,
        mem_type: String::from("DDR5"),
        speed_mts: 5600,
    };

    state.storage = vec![
        StorageDevice {
            device: String::from("/dev/nvme0n1p1"),
            model: String::from("Generic NVMe SSD"),
            capacity_bytes: 512 * 1024 * 1024 * 1024,
            free_bytes: 350 * 1024 * 1024 * 1024,
            fs_type: String::from("ext4"),
            mount_point: String::from("/"),
            network: false,
            removable: false,
            interface: String::from("NVMe"),
        },
        StorageDevice {
            device: String::from("/dev/nvme0n1p2"),
            model: String::from("Generic NVMe SSD"),
            capacity_bytes: 512 * 1024 * 1024 * 1024,
            free_bytes: 480 * 1024 * 1024 * 1024,
            fs_type: String::from("ext4"),
            mount_point: String::from("/home"),
            network: false,
            removable: false,
            interface: String::from("NVMe"),
        },
    ];

    state.gpus = vec![
        GpuInfo {
            name: String::from("Generic GPU"),
            vendor: String::from("Generic"),
            vram_mib: 8192,
            driver_version: String::from("1.0.0"),
            api_support: vec![
                String::from("Vulkan 1.3"),
                String::from("OpenGL 4.6"),
            ],
            resolution: String::from("1920x1080"),
            refresh_hz: 144,
        },
    ];

    state.net_ifaces = vec![
        NetIfaceInfo {
            name: String::from("eth0"),
            iface_type: String::from("Ethernet"),
            ip_address: String::from("192.168.1.100"),
            mac_address: String::from("52:54:00:12:34:56"),
            speed_mbps: 1000,
            connected: true,
        },
        NetIfaceInfo {
            name: String::from("wlan0"),
            iface_type: String::from("Wi-Fi"),
            ip_address: String::new(),
            mac_address: String::from("52:54:00:ab:cd:ef"),
            speed_mbps: 0,
            connected: false,
        },
    ];

    state.kernel_params = KernelParams {
        page_size: 16384,
        sched_model: String::from("PriorityRoundRobin"),
        preempt_model: String::from("Full"),
        alloc_model: String::from("Buddy"),
        overcommit_mode: String::from("Never"),
        max_cpus: 256,
        root_fs: String::from("ext4"),
        debug_assertions: false,
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

    // Test 1: init_defaults.
    serial_println!("sysinfo::self_test 1: defaults");
    init_defaults();
    let os = os_info();
    assert_eq!(os.name, "MintOS");
    assert_eq!(os.arch, "x86_64");

    // Test 2: CPU info.
    serial_println!("sysinfo::self_test 2: CPU");
    let cpu = cpu_info();
    assert_eq!(cpu.cores, 8);
    assert_eq!(cpu.threads, 16);
    assert!(!cpu.features.is_empty());

    // Test 3: memory info.
    serial_println!("sysinfo::self_test 3: memory");
    let mem = memory_info();
    assert!(mem.total_bytes > 0);
    assert!(mem.available_bytes <= mem.total_bytes);

    // Test 4: update memory.
    serial_println!("sysinfo::self_test 4: update memory");
    update_memory(8_000_000_000, 8_000_000_000, 100_000_000);
    let mem = memory_info();
    assert_eq!(mem.used_bytes, 8_000_000_000);
    assert_eq!(mem.swap_used, 100_000_000);

    // Test 5: storage.
    serial_println!("sysinfo::self_test 5: storage");
    let devs = storage_info();
    assert!(devs.len() >= 2);
    assert_eq!(devs[0].mount_point, "/");

    // Test 6: update storage free.
    serial_println!("sysinfo::self_test 6: update storage");
    update_storage_free("/dev/nvme0n1p1", 100_000_000_000)?;
    let devs = storage_info();
    assert_eq!(devs[0].free_bytes, 100_000_000_000);

    // Test 7: GPU.
    serial_println!("sysinfo::self_test 7: GPU");
    let gpus = gpu_info();
    assert_eq!(gpus.len(), 1);
    assert!(gpus[0].vram_mib > 0);

    // Test 8: network.
    serial_println!("sysinfo::self_test 8: network");
    let nets = network_info();
    assert!(nets.len() >= 2);
    assert!(nets[0].connected);

    // Test 9: kernel params.
    serial_println!("sysinfo::self_test 9: kernel params");
    let kp = kernel_params();
    assert_eq!(kp.page_size, 16384);
    assert_eq!(kp.sched_model, "PriorityRoundRobin");

    // Test 10: format_bytes helper.
    serial_println!("sysinfo::self_test 10: format_bytes");
    assert!(format_bytes(1024).contains("KiB"));
    assert!(format_bytes(1_073_741_824).contains("GiB"));

    // Test 11: add extra devices.
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
    assert_eq!(storage_info().len(), 3);

    clear_all();
    serial_println!("sysinfo::self_test: all 11 tests passed");
    Ok(())
}
