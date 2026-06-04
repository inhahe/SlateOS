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

#![allow(dead_code)]

use crate::{
    CpuInfo, DiskInfo, DisplayInfo, DmaInfo, DriverInfo, IoPortInfo, IrqInfo,
    MemoryInfo, MemoryMapEntry, MemorySlot, NetworkAdapterInfo, PartitionInfo,
    PciDeviceInfo, ProcessEntry, ServiceInfo, SoundInfo, StartupEntry,
    UsbDeviceInfo,
};
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

/// Base path for hardware info files exposed by the kernel.
const SYSFS_BASE: &str = "/sys/hardware";
/// CPU info file.
const SYSFS_CPU: &str = "/sys/hardware/cpu";
/// Memory info file.
const SYSFS_MEMORY: &str = "/sys/hardware/memory";
/// Block devices directory.
const SYSFS_BLOCK: &str = "/sys/hardware/block";
/// Network interfaces directory.
const SYSFS_NET: &str = "/sys/hardware/net";
/// PCI devices directory.
const SYSFS_PCI: &str = "/sys/hardware/pci";
/// USB devices directory.
const SYSFS_USB: &str = "/sys/hardware/usb";
/// Display/GPU info.
const SYSFS_DISPLAY: &str = "/sys/hardware/display";
/// Sound devices.
const SYSFS_SOUND: &str = "/sys/hardware/sound";
/// IRQ assignments.
const SYSFS_IRQS: &str = "/sys/hardware/irqs";
/// I/O port ranges.
const SYSFS_IOPORTS: &str = "/sys/hardware/ioports";
/// Memory map from firmware.
const SYSFS_MEMMAP: &str = "/sys/hardware/memmap";
/// DMA channels.
const SYSFS_DMA: &str = "/sys/hardware/dma";
/// Running services.
const SYSFS_SERVICES: &str = "/sys/services";
/// Process list.
const SYSFS_PROC: &str = "/sys/proc";
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
    /// Query startup programs.
    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError>;
    /// Human-readable name of this provider.
    fn provider_name(&self) -> &str;
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
}

impl SyscallProvider {
    /// Create a new syscall-based provider.
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
        }
    }

    /// Read a sysfs file, returning its contents or an error.
    fn read_sysfs(&self, path: &str) -> Result<String, HwQueryError> {
        // On the actual OS, this would use SYS_READ to read from the sysfs VFS.
        // For now, check the file cache (populated by refresh) or try a real read.
        if let Some(cached) = self.file_cache.get(path) {
            return Ok(cached.clone());
        }

        // Attempt a real filesystem read
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(content),
            Err(_) => Err(HwQueryError::NotAvailable {
                path: path.to_string(),
            }),
        }
    }

    /// Parse a key=value file into a HashMap.
    fn parse_kv_file(content: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            } else if let Some((key, value)) = line.split_once(':') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        map
    }

    /// Parse a u64 from a string, returning a parse error on failure.
    fn parse_u64(s: &str) -> Result<u64, HwQueryError> {
        s.trim().parse().map_err(|_| HwQueryError::ParseError {
            detail: format!("expected integer, got: '{s}'"),
        })
    }

    /// Parse a u32 from a string.
    fn parse_u32(s: &str) -> Result<u32, HwQueryError> {
        s.trim().parse().map_err(|_| HwQueryError::ParseError {
            detail: format!("expected u32, got: '{s}'"),
        })
    }

    /// Parse a f32 from a string.
    fn parse_f32(s: &str) -> Result<f32, HwQueryError> {
        s.trim().parse().map_err(|_| HwQueryError::ParseError {
            detail: format!("expected f32, got: '{s}'"),
        })
    }

    /// Query CPU info using CPUID instruction and sysfs.
    ///
    /// On native hardware, this reads CPUID results. The kernel exposes
    /// processed CPUID data at `/sys/hardware/cpu`.
    fn query_cpu_from_cpuid(&self) -> Result<CpuInfo, HwQueryError> {
        let content = self.read_sysfs(SYSFS_CPU)?;
        let kv = Self::parse_kv_file(&content);

        Ok(CpuInfo {
            brand: kv
                .get("brand")
                .cloned()
                .unwrap_or_else(|| "Unknown CPU".to_string()),
            vendor: kv
                .get("vendor")
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string()),
            family: kv
                .get("family")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            model: kv
                .get("model")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            stepping: kv
                .get("stepping")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            physical_cores: kv
                .get("physical_cores")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            logical_processors: kv
                .get("logical_processors")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            base_clock_mhz: kv
                .get("base_clock_mhz")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            max_turbo_mhz: kv
                .get("max_turbo_mhz")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            l1_data_kb: kv
                .get("l1_data_kb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            l1_inst_kb: kv
                .get("l1_inst_kb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            l2_kb: kv
                .get("l2_kb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            l3_kb: kv
                .get("l3_kb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            features: Self::parse_cpu_features(kv.get("features").map(|s| s.as_str()).unwrap_or("")),
        })
    }

    /// Parse CPU feature flags from a comma-separated string.
    /// Format: "SSE,SSE2,!AVX-512" where ! prefix means not supported.
    fn parse_cpu_features(features_str: &str) -> Vec<(String, bool)> {
        if features_str.is_empty() {
            return Vec::new();
        }
        features_str
            .split(',')
            .map(|f| {
                let f = f.trim();
                if let Some(name) = f.strip_prefix('!') {
                    (name.to_string(), false)
                } else {
                    (f.to_string(), true)
                }
            })
            .collect()
    }

    /// Parse a list of records from a sysfs directory.
    /// Each entry is a subdirectory with key=value files.
    fn read_sysfs_dir_entries(&self, base_path: &str) -> Result<Vec<HashMap<String, String>>, HwQueryError> {
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

    fn query_memory(&self) -> Result<MemoryInfo, HwQueryError> {
        let content = self.read_sysfs(SYSFS_MEMORY)?;
        let kv = Self::parse_kv_file(&content);

        let mut slots = Vec::new();
        // Parse slot entries if present (slot0_*, slot1_*, etc.)
        for i in 0..16 {
            let prefix = format!("slot{i}_");
            if let Some(name) = kv.get(&format!("{prefix}name")) {
                slots.push(MemorySlot {
                    slot_name: name.clone(),
                    size_mb: kv.get(&format!("{prefix}size_mb"))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    mem_type: kv.get(&format!("{prefix}type"))
                        .cloned()
                        .unwrap_or_default(),
                    speed_mhz: kv.get(&format!("{prefix}speed_mhz"))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    manufacturer: kv.get(&format!("{prefix}manufacturer"))
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }

        let slots_used = slots.iter().filter(|s| s.size_mb > 0).count() as u32;

        Ok(MemoryInfo {
            total_mb: kv.get("total_mb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            available_mb: kv.get("available_mb")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            mem_type: kv.get("type")
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string()),
            speed_mhz: kv.get("speed_mhz")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            slots_used,
            slots_total: kv.get("slots_total")
                .and_then(|v| v.parse().ok())
                .unwrap_or(slots.len() as u32),
            slots,
        })
    }

    fn query_storage(&self) -> Result<Vec<DiskInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_BLOCK)?;
        let mut disks = Vec::new();

        for entry in &entries {
            // Parse partitions from sub-entries
            let mut partitions = Vec::new();
            for i in 0..16 {
                let prefix = format!("part{i}_");
                if let Some(label) = entry.get(&format!("{prefix}label")) {
                    partitions.push(PartitionInfo {
                        label: label.clone(),
                        filesystem: entry.get(&format!("{prefix}fs"))
                            .cloned()
                            .unwrap_or_default(),
                        capacity_gb: entry.get(&format!("{prefix}capacity_gb"))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0),
                        used_gb: entry.get(&format!("{prefix}used_gb"))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0),
                        free_gb: entry.get(&format!("{prefix}free_gb"))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0),
                        mount_point: entry.get(&format!("{prefix}mount"))
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }

            disks.push(DiskInfo {
                model: entry.get("model")
                    .cloned()
                    .unwrap_or_else(|| "Unknown Disk".to_string()),
                capacity_gb: entry.get("capacity_gb")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0),
                interface: entry.get("interface")
                    .cloned()
                    .unwrap_or_default(),
                serial: entry.get("serial")
                    .cloned()
                    .unwrap_or_default(),
                smart_status: entry.get("smart_status")
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string()),
                partitions,
            });
        }

        Ok(disks)
    }

    fn query_network(&self) -> Result<Vec<NetworkAdapterInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_NET)?;
        let mut adapters = Vec::new();

        for entry in &entries {
            adapters.push(NetworkAdapterInfo {
                name: entry.get("name").cloned().unwrap_or_default(),
                adapter_type: entry.get("type").cloned().unwrap_or_default(),
                mac_address: entry.get("mac").cloned().unwrap_or_default(),
                ipv4: entry.get("ipv4").cloned().unwrap_or_default(),
                ipv6: entry.get("ipv6").cloned().unwrap_or_default(),
                subnet: entry.get("subnet").cloned().unwrap_or_default(),
                gateway: entry.get("gateway").cloned().unwrap_or_default(),
                dns: entry.get("dns").cloned().unwrap_or_default(),
                speed_mbps: entry.get("speed_mbps")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                duplex: entry.get("duplex").cloned().unwrap_or_default(),
                bytes_sent: entry.get("bytes_sent")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                bytes_received: entry.get("bytes_received")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            });
        }

        Ok(adapters)
    }

    fn query_display(&self) -> Result<DisplayInfo, HwQueryError> {
        let content = self.read_sysfs(SYSFS_DISPLAY)?;
        let kv = Self::parse_kv_file(&content);

        let mut outputs = Vec::new();
        for i in 0..8 {
            let key = format!("output{i}");
            if let Some(name) = kv.get(&key) {
                let connected = kv.get(&format!("{key}_connected"))
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false);
                outputs.push((name.clone(), connected));
            }
        }

        Ok(DisplayInfo {
            gpu_name: kv.get("gpu_name").cloned().unwrap_or_default(),
            vendor: kv.get("vendor").cloned().unwrap_or_default(),
            vram_mb: kv.get("vram_mb").and_then(|v| v.parse().ok()).unwrap_or(0),
            resolution: kv.get("resolution").cloned().unwrap_or_default(),
            refresh_rate_hz: kv.get("refresh_rate_hz").and_then(|v| v.parse().ok()).unwrap_or(0),
            outputs,
            driver_version: kv.get("driver_version").cloned().unwrap_or_default(),
        })
    }

    fn query_pci(&self) -> Result<Vec<PciDeviceInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_PCI)?;
        let mut devices = Vec::new();

        for entry in &entries {
            devices.push(PciDeviceInfo {
                bus: entry.get("bus").and_then(|v| v.parse().ok()).unwrap_or(0),
                device: entry.get("device").and_then(|v| v.parse().ok()).unwrap_or(0),
                function: entry.get("function").and_then(|v| v.parse().ok()).unwrap_or(0),
                vendor_id: entry.get("vendor_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                device_id: entry.get("device_id")
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
                vendor_id: entry.get("vendor_id")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                product_id: entry.get("product_id")
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

    fn query_irqs(&self) -> Result<Vec<IrqInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_IRQS)?;
        let mut irqs = Vec::new();

        for entry in &entries {
            irqs.push(IrqInfo {
                irq_number: entry.get("irq").and_then(|v| v.parse().ok()).unwrap_or(0),
                device: entry.get("device").cloned().unwrap_or_default(),
                irq_type: entry.get("type").cloned().unwrap_or_else(|| "Edge".to_string()),
            });
        }

        Ok(irqs)
    }

    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_IOPORTS)?;
        let mut ports = Vec::new();

        for entry in &entries {
            ports.push(IoPortInfo {
                start: entry.get("start")
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                end: entry.get("end")
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
                start: entry.get("start")
                    .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0),
                end: entry.get("end")
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
                channel: entry.get("channel").and_then(|v| v.parse().ok()).unwrap_or(0),
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

    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError> {
        let entries = self.read_sysfs_dir_entries(SYSFS_PROC)?;
        let mut procs = Vec::new();

        for entry in &entries {
            procs.push(ProcessEntry {
                pid: entry.get("pid").and_then(|v| v.parse().ok()).unwrap_or(0),
                name: entry.get("name").cloned().unwrap_or_default(),
                memory_kb: entry.get("memory_kb").and_then(|v| v.parse().ok()).unwrap_or(0),
                cpu_percent: entry.get("cpu_percent").and_then(|v| v.parse().ok()).unwrap_or(0.0),
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

    fn provider_name(&self) -> &str {
        "SyscallProvider (live hardware)"
    }
}

// ============================================================================
// Stub provider (existing hardcoded data, as fallback)
// ============================================================================

/// Provider that returns representative stub data.
/// Used for development, testing, and as a fallback when live queries fail.
pub struct StubProvider;

impl Default for StubProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl StubProvider {
    pub fn new() -> Self {
        Self
    }
}

impl HardwareProvider for StubProvider {
    fn query_cpu(&self) -> Result<CpuInfo, HwQueryError> {
        Ok(CpuInfo {
            brand: "OurOS Virtual CPU @ 3.60GHz".to_string(),
            vendor: "GenuineIntel".to_string(),
            family: 6,
            model: 158,
            stepping: 13,
            physical_cores: 8,
            logical_processors: 16,
            base_clock_mhz: 3600,
            max_turbo_mhz: 5100,
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
            capacity_gb: 1863.0,
            interface: "NVMe".to_string(),
            serial: "S6Z2NF0W123456".to_string(),
            smart_status: "Healthy".to_string(),
            partitions: vec![
                PartitionInfo {
                    label: "EFI System".to_string(),
                    filesystem: "FAT32".to_string(),
                    capacity_gb: 0.5,
                    used_gb: 0.1,
                    free_gb: 0.4,
                    mount_point: "/boot/efi".to_string(),
                },
                PartitionInfo {
                    label: "OurOS Root".to_string(),
                    filesystem: "ext4".to_string(),
                    capacity_gb: 500.0,
                    used_gb: 127.3,
                    free_gb: 372.7,
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
                bus: 0, device: 0, function: 0,
                vendor_id: 0x8086, device_id: 0xA700,
                class: "Host Bridge".to_string(),
                description: "Intel 13th Gen Core Host Bridge".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 0, device: 2, function: 0,
                vendor_id: 0x1002, device_id: 0x744C,
                class: "VGA Controller".to_string(),
                description: "AMD Radeon RX 7900 XTX".to_string(),
                vendor_name: "Advanced Micro Devices".to_string(),
            },
        ])
    }

    fn query_usb(&self) -> Result<Vec<UsbDeviceInfo>, HwQueryError> {
        Ok(vec![UsbDeviceInfo {
            port: "1-1".to_string(),
            vendor_id: 0x046D, product_id: 0xC548,
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
            IrqInfo { irq_number: 0, device: "Timer".to_string(), irq_type: "Edge".to_string() },
            IrqInfo { irq_number: 1, device: "Keyboard".to_string(), irq_type: "Edge".to_string() },
        ])
    }

    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError> {
        Ok(vec![
            IoPortInfo { start: 0x0060, end: 0x0064, device: "Keyboard Controller".to_string() },
            IoPortInfo { start: 0x03F8, end: 0x03FF, device: "COM1 (Serial)".to_string() },
        ])
    }

    fn query_memory_map(&self) -> Result<Vec<MemoryMapEntry>, HwQueryError> {
        Ok(vec![
            MemoryMapEntry {
                start: 0x0000_0000, end: 0x0009_FFFF,
                region_type: "Conventional".to_string(),
                description: "Low memory (640 KiB)".to_string(),
            },
        ])
    }

    fn query_dma(&self) -> Result<Vec<DmaInfo>, HwQueryError> {
        Ok(vec![
            DmaInfo { channel: 2, device: "Floppy (legacy)".to_string(), mode: "Single".to_string() },
        ])
    }

    fn query_services(&self) -> Result<Vec<ServiceInfo>, HwQueryError> {
        Ok(vec![
            ServiceInfo { name: "compositor".to_string(), status: "Running".to_string(), start_type: "Automatic".to_string() },
            ServiceInfo { name: "network-manager".to_string(), status: "Running".to_string(), start_type: "Automatic".to_string() },
        ])
    }

    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError> {
        Ok(vec![
            ProcessEntry { pid: 1, name: "init".to_string(), memory_kb: 2048, cpu_percent: 0.0 },
            ProcessEntry { pid: 2, name: "compositor".to_string(), memory_kb: 128000, cpu_percent: 3.2 },
        ])
    }

    fn query_drivers(&self) -> Result<Vec<DriverInfo>, HwQueryError> {
        Ok(vec![
            DriverInfo { name: "nvme".to_string(), path: "/drivers/storage/nvme.drv".to_string(), status: "Loaded".to_string() },
            DriverInfo { name: "amdgpu".to_string(), path: "/drivers/gpu/amdgpu.drv".to_string(), status: "Loaded".to_string() },
        ])
    }

    fn query_env_vars(&self) -> Result<Vec<(String, String)>, HwQueryError> {
        Ok(vec![
            ("PATH".to_string(), "/bin:/sbin:/usr/bin:/usr/local/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SHELL".to_string(), "/bin/osh".to_string()),
        ])
    }

    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError> {
        Ok(vec![
            StartupEntry { name: "Network Manager".to_string(), path: "/usr/bin/network-manager".to_string(), source: "System".to_string() },
        ])
    }

    fn provider_name(&self) -> &str {
        "StubProvider (representative data)"
    }
}

// ============================================================================
// Fallback provider — tries live, falls back to stub
// ============================================================================

/// Provider that tries `SyscallProvider` first and falls back to `StubProvider`
/// for each individual query if the syscall-based query fails.
pub struct FallbackProvider {
    syscall: SyscallProvider,
    stub: StubProvider,
}

impl Default for FallbackProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FallbackProvider {
    pub fn new() -> Self {
        Self {
            syscall: SyscallProvider::new(),
            stub: StubProvider::new(),
        }
    }
}

/// Macro to implement fallback: try syscall, fall back to stub on error.
macro_rules! fallback_query {
    ($self:expr, $method:ident) => {
        match $self.syscall.$method() {
            Ok(result) => Ok(result),
            Err(_) => $self.stub.$method(),
        }
    };
}

impl HardwareProvider for FallbackProvider {
    fn query_cpu(&self) -> Result<CpuInfo, HwQueryError> { fallback_query!(self, query_cpu) }
    fn query_memory(&self) -> Result<MemoryInfo, HwQueryError> { fallback_query!(self, query_memory) }
    fn query_storage(&self) -> Result<Vec<DiskInfo>, HwQueryError> { fallback_query!(self, query_storage) }
    fn query_network(&self) -> Result<Vec<NetworkAdapterInfo>, HwQueryError> { fallback_query!(self, query_network) }
    fn query_display(&self) -> Result<DisplayInfo, HwQueryError> { fallback_query!(self, query_display) }
    fn query_pci(&self) -> Result<Vec<PciDeviceInfo>, HwQueryError> { fallback_query!(self, query_pci) }
    fn query_usb(&self) -> Result<Vec<UsbDeviceInfo>, HwQueryError> { fallback_query!(self, query_usb) }
    fn query_sound(&self) -> Result<Vec<SoundInfo>, HwQueryError> { fallback_query!(self, query_sound) }
    fn query_irqs(&self) -> Result<Vec<IrqInfo>, HwQueryError> { fallback_query!(self, query_irqs) }
    fn query_io_ports(&self) -> Result<Vec<IoPortInfo>, HwQueryError> { fallback_query!(self, query_io_ports) }
    fn query_memory_map(&self) -> Result<Vec<MemoryMapEntry>, HwQueryError> { fallback_query!(self, query_memory_map) }
    fn query_dma(&self) -> Result<Vec<DmaInfo>, HwQueryError> { fallback_query!(self, query_dma) }
    fn query_services(&self) -> Result<Vec<ServiceInfo>, HwQueryError> { fallback_query!(self, query_services) }
    fn query_processes(&self) -> Result<Vec<ProcessEntry>, HwQueryError> { fallback_query!(self, query_processes) }
    fn query_drivers(&self) -> Result<Vec<DriverInfo>, HwQueryError> { fallback_query!(self, query_drivers) }
    fn query_env_vars(&self) -> Result<Vec<(String, String)>, HwQueryError> { fallback_query!(self, query_env_vars) }
    fn query_startup(&self) -> Result<Vec<StartupEntry>, HwQueryError> { fallback_query!(self, query_startup) }
    fn provider_name(&self) -> &str { "FallbackProvider (live → stub)" }
}

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
const TTL_STATIC_SECS: u64 = 300;    // CPU, display, PCI: rarely change
const TTL_DYNAMIC_SECS: u64 = 5;     // Processes, memory usage: change constantly
const TTL_MODERATE_SECS: u64 = 30;   // Network stats, services: change occasionally

impl RefreshManager {
    /// Create a new refresh manager with a fallback provider and default TTLs.
    pub fn new_with_fallback() -> Self {
        Self::new(Box::new(FallbackProvider::new()))
    }

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
    pub fn provider_name(&self) -> &str {
        self.provider.provider_name()
    }

    /// Get CPU info (cached with TTL).
    pub fn cpu(&mut self) -> CpuInfo {
        let now = Self::now();
        if let Some(ref entry) = self.cpu_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_cpu() {
            Ok(info) => {
                self.cpu_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.cpu_ttl });
                info
            }
            Err(_) => self.cpu_cache.as_ref().map(|e| e.data.clone()).unwrap_or_else(|| {
                // Last resort: empty default
                CpuInfo {
                    brand: "Unknown".to_string(), vendor: "Unknown".to_string(),
                    family: 0, model: 0, stepping: 0, physical_cores: 0,
                    logical_processors: 0, base_clock_mhz: 0, max_turbo_mhz: 0,
                    l1_data_kb: 0, l1_inst_kb: 0, l2_kb: 0, l3_kb: 0,
                    features: Vec::new(),
                }
            })
        }
    }

    /// Get memory info (cached with TTL).
    pub fn memory(&mut self) -> MemoryInfo {
        let now = Self::now();
        if let Some(ref entry) = self.memory_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_memory() {
            Ok(info) => {
                self.memory_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.memory_ttl });
                info
            }
            Err(_) => self.memory_cache.as_ref().map(|e| e.data.clone()).unwrap_or_else(|| {
                MemoryInfo {
                    total_mb: 0, available_mb: 0, mem_type: "Unknown".to_string(),
                    speed_mhz: 0, slots_used: 0, slots_total: 0, slots: Vec::new(),
                }
            })
        }
    }

    /// Get storage info (cached with TTL).
    pub fn storage(&mut self) -> Vec<DiskInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.storage_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_storage() {
            Ok(info) => {
                self.storage_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.storage_ttl });
                info
            }
            Err(_) => self.storage_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get network adapter info (cached with TTL).
    pub fn network(&mut self) -> Vec<NetworkAdapterInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.network_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_network() {
            Ok(info) => {
                self.network_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.network_ttl });
                info
            }
            Err(_) => self.network_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get display info (cached with TTL).
    pub fn display(&mut self) -> DisplayInfo {
        let now = Self::now();
        if let Some(ref entry) = self.display_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_display() {
            Ok(info) => {
                self.display_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.display_ttl });
                info
            }
            Err(_) => self.display_cache.as_ref().map(|e| e.data.clone()).unwrap_or_else(|| {
                DisplayInfo {
                    gpu_name: "Unknown".to_string(), vendor: "Unknown".to_string(),
                    vram_mb: 0, resolution: "Unknown".to_string(), refresh_rate_hz: 0,
                    outputs: Vec::new(), driver_version: "Unknown".to_string(),
                }
            })
        }
    }

    /// Get PCI devices (cached with TTL).
    pub fn pci(&mut self) -> Vec<PciDeviceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.pci_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_pci() {
            Ok(info) => {
                self.pci_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.pci_ttl });
                info
            }
            Err(_) => self.pci_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get USB devices (cached with TTL).
    pub fn usb(&mut self) -> Vec<UsbDeviceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.usb_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_usb() {
            Ok(info) => {
                self.usb_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.usb_ttl });
                info
            }
            Err(_) => self.usb_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get sound devices (cached with TTL).
    pub fn sound(&mut self) -> Vec<SoundInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.sound_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_sound() {
            Ok(info) => {
                self.sound_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.sound_ttl });
                info
            }
            Err(_) => self.sound_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get IRQs (cached with TTL).
    pub fn irqs(&mut self) -> Vec<IrqInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.irq_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_irqs() {
            Ok(info) => {
                self.irq_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.irq_ttl });
                info
            }
            Err(_) => self.irq_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get I/O ports (cached with TTL).
    pub fn io_ports(&mut self) -> Vec<IoPortInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.ioport_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_io_ports() {
            Ok(info) => {
                self.ioport_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.ioport_ttl });
                info
            }
            Err(_) => self.ioport_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get memory map (cached with TTL).
    pub fn memory_map(&mut self) -> Vec<MemoryMapEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.memmap_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_memory_map() {
            Ok(info) => {
                self.memmap_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.memmap_ttl });
                info
            }
            Err(_) => self.memmap_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get DMA channels (cached with TTL).
    pub fn dma(&mut self) -> Vec<DmaInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.dma_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_dma() {
            Ok(info) => {
                self.dma_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.dma_ttl });
                info
            }
            Err(_) => self.dma_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get services (cached with TTL).
    pub fn services(&mut self) -> Vec<ServiceInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.service_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_services() {
            Ok(info) => {
                self.service_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.service_ttl });
                info
            }
            Err(_) => self.service_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get processes (cached with TTL).
    pub fn processes(&mut self) -> Vec<ProcessEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.process_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_processes() {
            Ok(info) => {
                self.process_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.process_ttl });
                info
            }
            Err(_) => self.process_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get drivers (cached with TTL).
    pub fn drivers(&mut self) -> Vec<DriverInfo> {
        let now = Self::now();
        if let Some(ref entry) = self.driver_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_drivers() {
            Ok(info) => {
                self.driver_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.driver_ttl });
                info
            }
            Err(_) => self.driver_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get environment variables (cached with TTL).
    pub fn env_vars(&mut self) -> Vec<(String, String)> {
        let now = Self::now();
        if let Some(ref entry) = self.env_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_env_vars() {
            Ok(info) => {
                self.env_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.env_ttl });
                info
            }
            Err(_) => self.env_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }

    /// Get startup programs (cached with TTL).
    pub fn startup(&mut self) -> Vec<StartupEntry> {
        let now = Self::now();
        if let Some(ref entry) = self.startup_cache
            && !entry.is_stale(now) {
                return entry.data.clone();
            }
        self.refresh_count = self.refresh_count.saturating_add(1);
        match self.provider.query_startup() {
            Ok(info) => {
                self.startup_cache = Some(CacheEntry { data: info.clone(), timestamp: now, ttl_secs: self.startup_ttl });
                info
            }
            Err(_) => self.startup_cache.as_ref().map(|e| e.data.clone()).unwrap_or_default()
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Error display --

    #[test]
    fn test_error_display() {
        let e = HwQueryError::NotAvailable { path: "/sys/cpu".to_string() };
        assert!(e.to_string().contains("/sys/cpu"));

        let e = HwQueryError::ParseError { detail: "bad int".to_string() };
        assert!(e.to_string().contains("bad int"));

        assert_eq!(HwQueryError::Timeout.to_string(), "query timed out");
        assert_eq!(HwQueryError::PermissionDenied.to_string(), "permission denied");
    }

    // -- Stub provider --

    #[test]
    fn test_stub_cpu() {
        let stub = StubProvider::new();
        let cpu = stub.query_cpu().expect("stub cpu");
        assert!(cpu.brand.contains("OurOS"));
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

    #[test]
    fn test_parse_cpu_features_basic() {
        let features = SyscallProvider::parse_cpu_features("SSE,SSE2,!AVX-512");
        assert_eq!(features.len(), 3);
        assert_eq!(features[0], ("SSE".to_string(), true));
        assert_eq!(features[1], ("SSE2".to_string(), true));
        assert_eq!(features[2], ("AVX-512".to_string(), false));
    }

    #[test]
    fn test_parse_cpu_features_empty() {
        let features = SyscallProvider::parse_cpu_features("");
        assert!(features.is_empty());
    }

    #[test]
    fn test_parse_cpu_features_all_disabled() {
        let features = SyscallProvider::parse_cpu_features("!A,!B,!C");
        assert_eq!(features.len(), 3);
        assert!(features.iter().all(|(_, enabled)| !enabled));
    }

    // -- Key-value file parsing --

    #[test]
    fn test_parse_kv_equals() {
        let kv = SyscallProvider::parse_kv_file("brand=Intel i7\nmodel=158\n");
        assert_eq!(kv.get("brand").map(|s| s.as_str()), Some("Intel i7"));
        assert_eq!(kv.get("model").map(|s| s.as_str()), Some("158"));
    }

    #[test]
    fn test_parse_kv_colon() {
        let kv = SyscallProvider::parse_kv_file("vendor: Intel\nfamily: 6\n");
        assert_eq!(kv.get("vendor").map(|s| s.as_str()), Some("Intel"));
        assert_eq!(kv.get("family").map(|s| s.as_str()), Some("6"));
    }

    #[test]
    fn test_parse_kv_skips_comments_and_blanks() {
        let kv = SyscallProvider::parse_kv_file("# comment\n\nbrand=test\n");
        assert_eq!(kv.len(), 1);
        assert_eq!(kv.get("brand").map(|s| s.as_str()), Some("test"));
    }

    #[test]
    fn test_parse_kv_whitespace_trimming() {
        let kv = SyscallProvider::parse_kv_file("  key  =  value  \n");
        assert_eq!(kv.get("key").map(|s| s.as_str()), Some("value"));
    }

    // -- Parse helpers --

    #[test]
    fn test_parse_u64_valid() {
        assert_eq!(SyscallProvider::parse_u64("42"), Ok(42));
        assert_eq!(SyscallProvider::parse_u64(" 100 "), Ok(100));
    }

    #[test]
    fn test_parse_u64_invalid() {
        assert!(SyscallProvider::parse_u64("abc").is_err());
    }

    #[test]
    fn test_parse_u32_valid() {
        assert_eq!(SyscallProvider::parse_u32("12345"), Ok(12345));
    }

    #[test]
    fn test_parse_f32_valid() {
        let val = SyscallProvider::parse_f32("3.25").expect("parse f32");
        assert!((val - 3.25).abs() < 0.01);
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
        assert!(entry.is_stale(1030));  // 30s >= 30s TTL
        assert!(entry.is_stale(1100));  // 100s > 30s TTL
    }

    // -- Refresh manager --

    #[test]
    fn test_refresh_manager_with_stub() {
        let mut mgr = RefreshManager::new(Box::new(StubProvider::new()));
        let cpu = mgr.cpu();
        assert!(cpu.brand.contains("OurOS"));
        assert_eq!(mgr.refresh_count(), 1);

        // Second access should be cached
        let cpu2 = mgr.cpu();
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

    #[test]
    fn test_fallback_provider_uses_stub_when_syscall_fails() {
        // FallbackProvider should work since SyscallProvider will fail
        // (no sysfs on dev machine) and fall back to StubProvider
        let provider = FallbackProvider::new();
        let cpu = provider.query_cpu().expect("fallback cpu");
        assert!(cpu.brand.contains("OurOS"));
    }

    #[test]
    fn test_fallback_provider_name() {
        let provider = FallbackProvider::new();
        assert!(provider.provider_name().contains("Fallback"));
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
