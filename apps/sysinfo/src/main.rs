//! OurOS System Information Explorer
//!
//! Graphical application displaying hardware and OS information in a
//! tree-navigable layout similar to Windows msinfo32. Features:
//! - Tree navigation sidebar with expandable categories
//! - Detail view with property tables (name: value pairs)
//! - System summary, CPU, memory, storage, network, display, PCI info
//! - Software environment: services, processes, drivers, env vars
//! - Search across all categories (Ctrl+F)
//! - Copy individual values (Ctrl+C)
//! - Export all information to text
//!
//! Uses the guitk library for UI rendering. Hardware data is gathered
//! through OurOS syscalls; stubbed with representative data for initial
//! development.

#[allow(dead_code)]
pub mod hwquery;

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ============================================================================
// Constants — layout dimensions
// ============================================================================

/// Width of the tree sidebar.
const SIDEBAR_WIDTH: f32 = 260.0;
/// Height of the title bar.
const TITLE_BAR_HEIGHT: f32 = 36.0;
/// Height of the toolbar (search, export buttons).
const TOOLBAR_HEIGHT: f32 = 32.0;
/// Height of the status bar at the bottom.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of each tree node row.
const TREE_ROW_HEIGHT: f32 = 24.0;
/// Indentation per tree level.
const TREE_INDENT: f32 = 20.0;
/// Height of each property row in the detail view.
const PROPERTY_ROW_HEIGHT: f32 = 22.0;
/// Height of the property table header.
const PROPERTY_HEADER_HEIGHT: f32 = 26.0;
/// Default window width.
const DEFAULT_WIDTH: f32 = 1100.0;
/// Default window height.
const DEFAULT_HEIGHT: f32 = 700.0;

// ============================================================================
// Color palette — Catppuccin Mocha
// ============================================================================

/// Base background (Crust).
const COLOR_BASE: Color = Color::rgb(17, 17, 27);
/// Slightly lighter surface (Mantle).
const COLOR_MANTLE: Color = Color::rgb(24, 24, 37);
/// Surface for panels.
const COLOR_SURFACE0: Color = Color::rgb(30, 30, 46);
/// Lighter surface for selected items.
const COLOR_SURFACE1: Color = Color::rgb(49, 50, 68);
/// Overlay surface.
#[allow(dead_code)]
const COLOR_SURFACE2: Color = Color::rgb(69, 71, 90);
/// Primary text (Text).
const COLOR_TEXT: Color = Color::rgb(205, 214, 244);
/// Secondary/dimmed text (Subtext0).
const COLOR_SUBTEXT: Color = Color::rgb(166, 173, 200);
/// Overlay text (Overlay1).
const COLOR_OVERLAY: Color = Color::rgb(147, 153, 178);
/// Blue accent (Blue).
const COLOR_BLUE: Color = Color::rgb(137, 180, 250);
/// Lavender accent.
const COLOR_LAVENDER: Color = Color::rgb(180, 190, 254);
/// Green (success / checkmark).
const COLOR_GREEN: Color = Color::rgb(166, 227, 161);
/// Yellow (warning).
#[allow(dead_code)]
const COLOR_YELLOW: Color = Color::rgb(249, 226, 175);
/// Red (error / stopped).
const COLOR_RED: Color = Color::rgb(243, 139, 168);
/// Peach accent.
const COLOR_PEACH: Color = Color::rgb(250, 179, 135);
/// Teal accent.
#[allow(dead_code)]
const COLOR_TEAL: Color = Color::rgb(148, 226, 213);
/// Sidebar background.
const COLOR_SIDEBAR_BG: Color = Color::rgb(24, 24, 37);
/// Tree node hover.
const COLOR_TREE_HOVER: Color = Color::rgb(40, 40, 58);
/// Tree node selected.
const COLOR_TREE_SELECTED: Color = Color::rgb(49, 50, 68);
/// Title bar background.
const COLOR_TITLE_BG: Color = Color::rgb(17, 17, 27);
/// Toolbar background.
const COLOR_TOOLBAR_BG: Color = Color::rgb(24, 24, 37);
/// Status bar background.
const COLOR_STATUS_BG: Color = Color::rgb(24, 24, 37);
/// Property row alternating.
const COLOR_ROW_EVEN: Color = Color::rgb(30, 30, 46);
/// Property row alternating.
const COLOR_ROW_ODD: Color = Color::rgb(36, 36, 54);
/// Separator line color.
const COLOR_SEPARATOR: Color = Color::rgb(49, 50, 68);
/// Search box background.
const COLOR_SEARCH_BG: Color = Color::rgb(30, 30, 46);
/// Search box border.
const COLOR_SEARCH_BORDER: Color = Color::rgb(69, 71, 90);

// ============================================================================
// Category tree definitions
// ============================================================================

/// All navigable categories in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SysInfoCategory {
    SystemSummary,
    HardwareResources,
    HwIrqs,
    HwIoPorts,
    HwMemoryMap,
    HwDma,
    Components,
    CompCpu,
    CompMemory,
    CompStorage,
    CompDisplay,
    CompSound,
    CompNetwork,
    CompUsb,
    CompPci,
    SoftwareEnvironment,
    SwServices,
    SwProcesses,
    SwDrivers,
    SwEnvVars,
    SwStartupPrograms,
}

impl SysInfoCategory {
    /// Display label for the category.
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemSummary => "System Summary",
            Self::HardwareResources => "Hardware Resources",
            Self::HwIrqs => "IRQs",
            Self::HwIoPorts => "I/O Ports",
            Self::HwMemoryMap => "Memory Map",
            Self::HwDma => "DMA",
            Self::Components => "Components",
            Self::CompCpu => "CPU",
            Self::CompMemory => "Memory (RAM)",
            Self::CompStorage => "Storage",
            Self::CompDisplay => "Display",
            Self::CompSound => "Sound",
            Self::CompNetwork => "Network",
            Self::CompUsb => "USB",
            Self::CompPci => "PCI Devices",
            Self::SoftwareEnvironment => "Software Environment",
            Self::SwServices => "System Services",
            Self::SwProcesses => "Running Processes",
            Self::SwDrivers => "Loaded Drivers",
            Self::SwEnvVars => "Environment Variables",
            Self::SwStartupPrograms => "Startup Programs",
        }
    }

    /// Whether this category is a parent (expandable) node.
    pub fn is_parent(self) -> bool {
        matches!(
            self,
            Self::HardwareResources | Self::Components | Self::SoftwareEnvironment
        )
    }

    /// Children of this parent category.
    pub fn children(self) -> &'static [SysInfoCategory] {
        match self {
            Self::HardwareResources => &[
                Self::HwIrqs,
                Self::HwIoPorts,
                Self::HwMemoryMap,
                Self::HwDma,
            ],
            Self::Components => &[
                Self::CompCpu,
                Self::CompMemory,
                Self::CompStorage,
                Self::CompDisplay,
                Self::CompSound,
                Self::CompNetwork,
                Self::CompUsb,
                Self::CompPci,
            ],
            Self::SoftwareEnvironment => &[
                Self::SwServices,
                Self::SwProcesses,
                Self::SwDrivers,
                Self::SwEnvVars,
                Self::SwStartupPrograms,
            ],
            _ => &[],
        }
    }

    /// Tree depth (0 = top-level, 1 = child).
    pub fn depth(self) -> u32 {
        match self {
            Self::SystemSummary
            | Self::HardwareResources
            | Self::Components
            | Self::SoftwareEnvironment => 0,
            _ => 1,
        }
    }

    /// Parent of this category, if it is a child.
    pub fn parent(self) -> Option<SysInfoCategory> {
        match self {
            Self::HwIrqs | Self::HwIoPorts | Self::HwMemoryMap | Self::HwDma => {
                Some(Self::HardwareResources)
            }
            Self::CompCpu
            | Self::CompMemory
            | Self::CompStorage
            | Self::CompDisplay
            | Self::CompSound
            | Self::CompNetwork
            | Self::CompUsb
            | Self::CompPci => Some(Self::Components),
            Self::SwServices
            | Self::SwProcesses
            | Self::SwDrivers
            | Self::SwEnvVars
            | Self::SwStartupPrograms => Some(Self::SoftwareEnvironment),
            _ => None,
        }
    }
}

/// Top-level tree order.
const TREE_ROOT_ITEMS: &[SysInfoCategory] = &[
    SysInfoCategory::SystemSummary,
    SysInfoCategory::HardwareResources,
    SysInfoCategory::Components,
    SysInfoCategory::SoftwareEnvironment,
];

// ============================================================================
// Data structures for each category
// ============================================================================

/// A name-value property displayed in the detail pane.
#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub value: String,
}

impl Property {
    fn new(name: &str, value: &str) -> Self {
        Self {
            name: name.to_string(),
            value: value.to_string(),
        }
    }
}

/// CPU information.
#[derive(Clone, Debug)]
pub struct CpuInfo {
    pub brand: String,
    pub vendor: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub physical_cores: u32,
    pub logical_processors: u32,
    pub base_clock_mhz: u32,
    pub max_turbo_mhz: u32,
    pub l1_data_kb: u32,
    pub l1_inst_kb: u32,
    pub l2_kb: u32,
    pub l3_kb: u32,
    pub features: Vec<(String, bool)>,
}

/// Memory slot information.
#[derive(Clone, Debug)]
pub struct MemorySlot {
    pub slot_name: String,
    pub size_mb: u32,
    pub mem_type: String,
    pub speed_mhz: u32,
    pub manufacturer: String,
}

/// Overall memory information.
#[derive(Clone, Debug)]
pub struct MemoryInfo {
    pub total_mb: u64,
    pub available_mb: u64,
    pub mem_type: String,
    pub speed_mhz: u32,
    pub slots_used: u32,
    pub slots_total: u32,
    pub slots: Vec<MemorySlot>,
}

/// Partition information.
#[derive(Clone, Debug)]
pub struct PartitionInfo {
    pub label: String,
    pub filesystem: String,
    pub capacity_gb: f32,
    pub used_gb: f32,
    pub free_gb: f32,
    pub mount_point: String,
}

/// Disk information.
#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub model: String,
    pub capacity_gb: f32,
    pub interface: String,
    pub serial: String,
    pub smart_status: String,
    pub partitions: Vec<PartitionInfo>,
}

/// Network adapter information.
#[derive(Clone, Debug)]
pub struct NetworkAdapterInfo {
    pub name: String,
    pub adapter_type: String,
    pub mac_address: String,
    pub ipv4: String,
    pub ipv6: String,
    pub subnet: String,
    pub gateway: String,
    pub dns: String,
    pub speed_mbps: u32,
    pub duplex: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Display/GPU information.
#[derive(Clone, Debug)]
pub struct DisplayInfo {
    pub gpu_name: String,
    pub vendor: String,
    pub vram_mb: u32,
    pub resolution: String,
    pub refresh_rate_hz: u32,
    pub outputs: Vec<(String, bool)>,
    pub driver_version: String,
}

/// PCI device entry.
#[derive(Clone, Debug)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: String,
    pub description: String,
    pub vendor_name: String,
}

/// Service entry.
#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub status: String,
    pub start_type: String,
}

/// Process entry (for the sysinfo view).
#[derive(Clone, Debug)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub memory_kb: u64,
    pub cpu_percent: f32,
}

/// Driver entry.
#[derive(Clone, Debug)]
pub struct DriverInfo {
    pub name: String,
    pub path: String,
    pub status: String,
}

/// IRQ assignment.
#[derive(Clone, Debug)]
pub struct IrqInfo {
    pub irq_number: u32,
    pub device: String,
    pub irq_type: String,
}

/// I/O port range.
#[derive(Clone, Debug)]
pub struct IoPortInfo {
    pub start: u16,
    pub end: u16,
    pub device: String,
}

/// Memory map region.
#[derive(Clone, Debug)]
pub struct MemoryMapEntry {
    pub start: u64,
    pub end: u64,
    pub region_type: String,
    pub description: String,
}

/// DMA channel assignment.
#[derive(Clone, Debug)]
pub struct DmaInfo {
    pub channel: u8,
    pub device: String,
    pub mode: String,
}

/// USB device entry.
#[derive(Clone, Debug)]
pub struct UsbDeviceInfo {
    pub port: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub description: String,
    pub speed: String,
}

/// Sound device.
#[derive(Clone, Debug)]
pub struct SoundInfo {
    pub name: String,
    pub device_type: String,
    pub driver: String,
    pub status: String,
}

/// Startup program entry.
#[derive(Clone, Debug)]
pub struct StartupEntry {
    pub name: String,
    pub path: String,
    pub source: String,
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state for the System Information Explorer.
pub struct SysInfoState {
    /// Currently selected category in the tree.
    pub selected_category: SysInfoCategory,
    /// Which parent nodes are expanded.
    pub expanded: Vec<SysInfoCategory>,
    /// Scroll offset in the detail pane.
    pub detail_scroll: f32,
    /// Scroll offset in the tree.
    pub tree_scroll: f32,
    /// Window width.
    pub window_width: f32,
    /// Window height.
    pub window_height: f32,
    /// Hovered tree row index (visible index).
    pub hovered_tree_row: Option<usize>,
    /// Search query text.
    pub search_text: String,
    /// Whether search box is focused.
    pub search_focused: bool,
    /// Status message.
    pub status_message: String,

    // Data sources (populated from system or stubbed).
    pub cpu_info: CpuInfo,
    pub memory_info: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub network_adapters: Vec<NetworkAdapterInfo>,
    pub display_info: DisplayInfo,
    pub pci_devices: Vec<PciDeviceInfo>,
    pub services: Vec<ServiceInfo>,
    pub processes: Vec<ProcessEntry>,
    pub drivers: Vec<DriverInfo>,
    pub env_vars: Vec<(String, String)>,
    pub irqs: Vec<IrqInfo>,
    pub io_ports: Vec<IoPortInfo>,
    pub memory_map: Vec<MemoryMapEntry>,
    pub dma_channels: Vec<DmaInfo>,
    pub usb_devices: Vec<UsbDeviceInfo>,
    pub sound_devices: Vec<SoundInfo>,
    pub startup_programs: Vec<StartupEntry>,
}

impl SysInfoState {
    /// Create a new state with default values.
    pub fn new() -> Self {
        Self {
            selected_category: SysInfoCategory::SystemSummary,
            expanded: vec![
                SysInfoCategory::HardwareResources,
                SysInfoCategory::Components,
                SysInfoCategory::SoftwareEnvironment,
            ],
            detail_scroll: 0.0,
            tree_scroll: 0.0,
            window_width: DEFAULT_WIDTH,
            window_height: DEFAULT_HEIGHT,
            hovered_tree_row: None,
            search_text: String::new(),
            search_focused: false,
            status_message: String::from("Ready"),
            cpu_info: Self::populate_cpu(),
            memory_info: Self::populate_memory(),
            disks: Self::populate_storage(),
            network_adapters: Self::populate_network(),
            display_info: Self::populate_display(),
            pci_devices: Self::populate_pci(),
            services: Self::populate_services(),
            processes: Self::populate_processes(),
            drivers: Self::populate_drivers(),
            env_vars: Self::populate_env_vars(),
            irqs: Self::populate_irqs(),
            io_ports: Self::populate_io_ports(),
            memory_map: Self::populate_memory_map(),
            dma_channels: Self::populate_dma(),
            usb_devices: Self::populate_usb(),
            sound_devices: Self::populate_sound(),
            startup_programs: Self::populate_startup(),
        }
    }

    // ========================================================================
    // Data population (stubbed with representative data)
    // ========================================================================

    fn populate_cpu() -> CpuInfo {
        CpuInfo {
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
                ("FMA".to_string(), true),
                ("POPCNT".to_string(), true),
                ("RDRAND".to_string(), true),
                ("TSX".to_string(), false),
                ("SHA".to_string(), true),
                ("BMI1".to_string(), true),
                ("BMI2".to_string(), true),
            ],
        }
    }

    fn populate_memory() -> MemoryInfo {
        MemoryInfo {
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
        }
    }

    fn populate_storage() -> Vec<DiskInfo> {
        vec![
            DiskInfo {
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
                    PartitionInfo {
                        label: "Home".to_string(),
                        filesystem: "ext4".to_string(),
                        capacity_gb: 1362.0,
                        used_gb: 843.5,
                        free_gb: 518.5,
                        mount_point: "/home".to_string(),
                    },
                ],
            },
            DiskInfo {
                model: "WD Blue SN580 1TB".to_string(),
                capacity_gb: 931.0,
                interface: "NVMe".to_string(),
                serial: "WD-WX32A0987654".to_string(),
                smart_status: "Healthy".to_string(),
                partitions: vec![PartitionInfo {
                    label: "Data".to_string(),
                    filesystem: "ext4".to_string(),
                    capacity_gb: 931.0,
                    used_gb: 412.8,
                    free_gb: 518.2,
                    mount_point: "/mnt/data".to_string(),
                }],
            },
        ]
    }

    fn populate_network() -> Vec<NetworkAdapterInfo> {
        vec![
            NetworkAdapterInfo {
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
            },
            NetworkAdapterInfo {
                name: "Intel Wi-Fi 6E AX211".to_string(),
                adapter_type: "Wi-Fi".to_string(),
                mac_address: "B0:DC:EF:78:9A:BC".to_string(),
                ipv4: "192.168.1.101".to_string(),
                ipv6: "fe80::b2dc:efff:fe78:9abc".to_string(),
                subnet: "255.255.255.0".to_string(),
                gateway: "192.168.1.1".to_string(),
                dns: "1.1.1.1, 8.8.8.8".to_string(),
                speed_mbps: 1200,
                duplex: "N/A".to_string(),
                bytes_sent: 234_567_890,
                bytes_received: 1_876_543_210,
            },
        ]
    }

    fn populate_display() -> DisplayInfo {
        DisplayInfo {
            gpu_name: "AMD Radeon RX 7900 XTX".to_string(),
            vendor: "AMD".to_string(),
            vram_mb: 24576,
            resolution: "3840x2160".to_string(),
            refresh_rate_hz: 144,
            outputs: vec![
                ("DisplayPort 1".to_string(), true),
                ("DisplayPort 2".to_string(), false),
                ("HDMI 1".to_string(), true),
                ("HDMI 2".to_string(), false),
            ],
            driver_version: "24.5.1".to_string(),
        }
    }

    fn populate_pci() -> Vec<PciDeviceInfo> {
        vec![
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
                description: "AMD Radeon RX 7900 XTX (Navi 31)".to_string(),
                vendor_name: "Advanced Micro Devices".to_string(),
            },
            PciDeviceInfo {
                bus: 0,
                device: 14,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0x7AE8,
                class: "USB Controller".to_string(),
                description: "Intel USB 3.2 xHCI Host Controller".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 0,
                device: 31,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0x7A04,
                class: "ISA Bridge".to_string(),
                description: "Intel Z790 Chipset LPC/eSPI Controller".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 1,
                device: 0,
                function: 0,
                vendor_id: 0x144D,
                device_id: 0xA80A,
                class: "NVMe Controller".to_string(),
                description: "Samsung 990 Pro NVMe SSD".to_string(),
                vendor_name: "Samsung Electronics".to_string(),
            },
            PciDeviceInfo {
                bus: 2,
                device: 0,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0x125B,
                class: "Ethernet Controller".to_string(),
                description: "Intel I225-V 2.5G Ethernet".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 3,
                device: 0,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0x51F0,
                class: "Network Controller".to_string(),
                description: "Intel Wi-Fi 6E AX211 (Gig+)".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
            PciDeviceInfo {
                bus: 0,
                device: 31,
                function: 3,
                vendor_id: 0x8086,
                device_id: 0x7AD0,
                class: "Audio Device".to_string(),
                description: "Intel Alder Lake HD Audio Controller".to_string(),
                vendor_name: "Intel Corporation".to_string(),
            },
        ]
    }

    fn populate_services() -> Vec<ServiceInfo> {
        vec![
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
            ServiceInfo {
                name: "audio-mixer".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
            ServiceInfo {
                name: "device-manager".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
            ServiceInfo {
                name: "package-daemon".to_string(),
                status: "Stopped".to_string(),
                start_type: "Manual".to_string(),
            },
            ServiceInfo {
                name: "ssh-server".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
            ServiceInfo {
                name: "backup-scheduler".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
            ServiceInfo {
                name: "bluetooth".to_string(),
                status: "Running".to_string(),
                start_type: "Automatic".to_string(),
            },
        ]
    }

    fn populate_processes() -> Vec<ProcessEntry> {
        vec![
            ProcessEntry { pid: 1, name: "init".to_string(), memory_kb: 2048, cpu_percent: 0.0 },
            ProcessEntry { pid: 2, name: "compositor".to_string(), memory_kb: 128000, cpu_percent: 3.2 },
            ProcessEntry { pid: 5, name: "device-manager".to_string(), memory_kb: 45000, cpu_percent: 0.5 },
            ProcessEntry { pid: 8, name: "network-manager".to_string(), memory_kb: 32000, cpu_percent: 0.1 },
            ProcessEntry { pid: 12, name: "audio-mixer".to_string(), memory_kb: 24000, cpu_percent: 1.0 },
            ProcessEntry { pid: 15, name: "window-manager".to_string(), memory_kb: 86000, cpu_percent: 2.4 },
            ProcessEntry { pid: 20, name: "file-explorer".to_string(), memory_kb: 64000, cpu_percent: 0.8 },
            ProcessEntry { pid: 25, name: "terminal".to_string(), memory_kb: 18000, cpu_percent: 0.2 },
            ProcessEntry { pid: 30, name: "ssh-server".to_string(), memory_kb: 8000, cpu_percent: 0.0 },
            ProcessEntry { pid: 42, name: "sysinfo".to_string(), memory_kb: 52000, cpu_percent: 1.5 },
        ]
    }

    fn populate_drivers() -> Vec<DriverInfo> {
        vec![
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
            DriverInfo {
                name: "i225".to_string(),
                path: "/drivers/net/i225.drv".to_string(),
                status: "Loaded".to_string(),
            },
            DriverInfo {
                name: "iwlwifi".to_string(),
                path: "/drivers/net/iwlwifi.drv".to_string(),
                status: "Loaded".to_string(),
            },
            DriverInfo {
                name: "xhci-hcd".to_string(),
                path: "/drivers/usb/xhci.drv".to_string(),
                status: "Loaded".to_string(),
            },
            DriverInfo {
                name: "hda-intel".to_string(),
                path: "/drivers/audio/hda_intel.drv".to_string(),
                status: "Loaded".to_string(),
            },
            DriverInfo {
                name: "btusb".to_string(),
                path: "/drivers/bluetooth/btusb.drv".to_string(),
                status: "Loaded".to_string(),
            },
        ]
    }

    fn populate_env_vars() -> Vec<(String, String)> {
        vec![
            ("PATH".to_string(), "/bin:/sbin:/usr/bin:/usr/local/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SHELL".to_string(), "/bin/osh".to_string()),
            ("TERM".to_string(), "ouros-256color".to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string()),
            ("DISPLAY".to_string(), ":0".to_string()),
            ("EDITOR".to_string(), "/usr/bin/oedit".to_string()),
        ]
    }

    fn populate_irqs() -> Vec<IrqInfo> {
        vec![
            IrqInfo { irq_number: 0, device: "Timer".to_string(), irq_type: "Edge".to_string() },
            IrqInfo { irq_number: 1, device: "Keyboard".to_string(), irq_type: "Edge".to_string() },
            IrqInfo { irq_number: 8, device: "RTC".to_string(), irq_type: "Edge".to_string() },
            IrqInfo { irq_number: 12, device: "Mouse".to_string(), irq_type: "Edge".to_string() },
            IrqInfo { irq_number: 14, device: "NVMe SSD".to_string(), irq_type: "MSI-X".to_string() },
            IrqInfo { irq_number: 16, device: "GPU".to_string(), irq_type: "MSI-X".to_string() },
            IrqInfo { irq_number: 18, device: "Ethernet".to_string(), irq_type: "MSI".to_string() },
            IrqInfo { irq_number: 19, device: "USB xHCI".to_string(), irq_type: "MSI".to_string() },
            IrqInfo { irq_number: 22, device: "HD Audio".to_string(), irq_type: "MSI".to_string() },
        ]
    }

    fn populate_io_ports() -> Vec<IoPortInfo> {
        vec![
            IoPortInfo { start: 0x0000, end: 0x001F, device: "DMA Controller".to_string() },
            IoPortInfo { start: 0x0020, end: 0x0021, device: "PIC Master".to_string() },
            IoPortInfo { start: 0x0040, end: 0x0043, device: "PIT Timer".to_string() },
            IoPortInfo { start: 0x0060, end: 0x0064, device: "Keyboard Controller".to_string() },
            IoPortInfo { start: 0x0070, end: 0x0071, device: "RTC/CMOS".to_string() },
            IoPortInfo { start: 0x00A0, end: 0x00A1, device: "PIC Slave".to_string() },
            IoPortInfo { start: 0x03F8, end: 0x03FF, device: "COM1 (Serial)".to_string() },
            IoPortInfo { start: 0x0CF8, end: 0x0CFF, device: "PCI Configuration".to_string() },
        ]
    }

    fn populate_memory_map() -> Vec<MemoryMapEntry> {
        vec![
            MemoryMapEntry {
                start: 0x0000_0000,
                end: 0x0009_FFFF,
                region_type: "Conventional".to_string(),
                description: "Low memory (640 KiB)".to_string(),
            },
            MemoryMapEntry {
                start: 0x000A_0000,
                end: 0x000F_FFFF,
                region_type: "Reserved".to_string(),
                description: "Legacy video/ROM area".to_string(),
            },
            MemoryMapEntry {
                start: 0x0010_0000,
                end: 0x7FFF_FFFF,
                region_type: "Available".to_string(),
                description: "Main memory (2 GiB)".to_string(),
            },
            MemoryMapEntry {
                start: 0xFEC0_0000,
                end: 0xFEC0_0FFF,
                region_type: "MMIO".to_string(),
                description: "I/O APIC".to_string(),
            },
            MemoryMapEntry {
                start: 0xFEE0_0000,
                end: 0xFEE0_0FFF,
                region_type: "MMIO".to_string(),
                description: "Local APIC".to_string(),
            },
            MemoryMapEntry {
                start: 0x1_0000_0000,
                end: 0x8_7FFF_FFFF,
                region_type: "Available".to_string(),
                description: "Extended memory (30 GiB)".to_string(),
            },
        ]
    }

    fn populate_dma() -> Vec<DmaInfo> {
        vec![
            DmaInfo { channel: 0, device: "Available".to_string(), mode: "N/A".to_string() },
            DmaInfo { channel: 1, device: "Available".to_string(), mode: "N/A".to_string() },
            DmaInfo { channel: 2, device: "Floppy (legacy)".to_string(), mode: "Single".to_string() },
            DmaInfo { channel: 4, device: "Cascade".to_string(), mode: "Cascade".to_string() },
        ]
    }

    fn populate_usb() -> Vec<UsbDeviceInfo> {
        vec![
            UsbDeviceInfo {
                port: "1-1".to_string(),
                vendor_id: 0x046D,
                product_id: 0xC548,
                description: "Logitech G Pro Wireless Mouse".to_string(),
                speed: "USB 2.0 (12 Mbps)".to_string(),
            },
            UsbDeviceInfo {
                port: "1-2".to_string(),
                vendor_id: 0x046D,
                product_id: 0xC33A,
                description: "Logitech G915 Keyboard".to_string(),
                speed: "USB 2.0 (12 Mbps)".to_string(),
            },
            UsbDeviceInfo {
                port: "2-1".to_string(),
                vendor_id: 0x0BDA,
                product_id: 0x5411,
                description: "Realtek USB Hub".to_string(),
                speed: "USB 3.2 (5 Gbps)".to_string(),
            },
            UsbDeviceInfo {
                port: "3-1".to_string(),
                vendor_id: 0x8087,
                product_id: 0x0033,
                description: "Intel Bluetooth Adapter".to_string(),
                speed: "USB 2.0 (12 Mbps)".to_string(),
            },
        ]
    }

    fn populate_sound() -> Vec<SoundInfo> {
        vec![
            SoundInfo {
                name: "Realtek ALC4080 HD Audio".to_string(),
                device_type: "Output".to_string(),
                driver: "hda-intel".to_string(),
                status: "Active".to_string(),
            },
            SoundInfo {
                name: "Realtek ALC4080 Line In".to_string(),
                device_type: "Input".to_string(),
                driver: "hda-intel".to_string(),
                status: "Idle".to_string(),
            },
            SoundInfo {
                name: "AMD HDMI Audio (RX 7900 XTX)".to_string(),
                device_type: "Output".to_string(),
                driver: "amdgpu-audio".to_string(),
                status: "Idle".to_string(),
            },
        ]
    }

    fn populate_startup() -> Vec<StartupEntry> {
        vec![
            StartupEntry {
                name: "Network Manager".to_string(),
                path: "/usr/bin/network-manager".to_string(),
                source: "System".to_string(),
            },
            StartupEntry {
                name: "Bluetooth Service".to_string(),
                path: "/usr/bin/bluetoothd".to_string(),
                source: "System".to_string(),
            },
            StartupEntry {
                name: "Cloud Sync".to_string(),
                path: "/usr/bin/cloudsync".to_string(),
                source: "User".to_string(),
            },
        ]
    }

    // ========================================================================
    // Property generation for each category
    // ========================================================================

    /// Generate properties for the currently selected category.
    pub fn current_properties(&self) -> Vec<Property> {
        match self.selected_category {
            SysInfoCategory::SystemSummary => self.props_system_summary(),
            SysInfoCategory::CompCpu => self.props_cpu(),
            SysInfoCategory::CompMemory => self.props_memory(),
            SysInfoCategory::CompStorage => self.props_storage(),
            SysInfoCategory::CompDisplay => self.props_display(),
            SysInfoCategory::CompSound => self.props_sound(),
            SysInfoCategory::CompNetwork => self.props_network(),
            SysInfoCategory::CompUsb => self.props_usb(),
            SysInfoCategory::CompPci => self.props_pci(),
            SysInfoCategory::SwServices => self.props_services(),
            SysInfoCategory::SwProcesses => self.props_processes(),
            SysInfoCategory::SwDrivers => self.props_drivers(),
            SysInfoCategory::SwEnvVars => self.props_env_vars(),
            SysInfoCategory::SwStartupPrograms => self.props_startup(),
            SysInfoCategory::HwIrqs => self.props_irqs(),
            SysInfoCategory::HwIoPorts => self.props_io_ports(),
            SysInfoCategory::HwMemoryMap => self.props_memory_map(),
            SysInfoCategory::HwDma => self.props_dma(),
            SysInfoCategory::HardwareResources
            | SysInfoCategory::Components
            | SysInfoCategory::SoftwareEnvironment => {
                vec![Property::new(
                    "Info",
                    "Select a subcategory from the tree to view details.",
                )]
            }
        }
    }

    fn props_system_summary(&self) -> Vec<Property> {
        let cpu = &self.cpu_info;
        let mem = &self.memory_info;
        vec![
            Property::new("OS Name", "OurOS"),
            Property::new("OS Version", "1.0.0"),
            Property::new("OS Build", "2026.05.17-nightly"),
            Property::new("Kernel Version", "0.1.0-ouros"),
            Property::new("System Manufacturer", "SMBIOS: To Be Filled By O.E.M."),
            Property::new("Processor", &cpu.brand),
            Property::new("Cores / Threads", &format!("{} / {}", cpu.physical_cores, cpu.logical_processors)),
            Property::new("Base Frequency", &format!("{} MHz", cpu.base_clock_mhz)),
            Property::new("Total Physical Memory", &format!("{} MiB ({:.1} GiB)", mem.total_mb, mem.total_mb as f64 / 1024.0)),
            Property::new("Available Physical Memory", &format!("{} MiB ({:.1} GiB)", mem.available_mb, mem.available_mb as f64 / 1024.0)),
            Property::new("Total Virtual Memory", "65536 MiB (64.0 GiB)"),
            Property::new("Page Size", "16 KiB"),
            Property::new("System Uptime", "4h 23m 17s"),
            Property::new("Boot Time", "2026-05-17 08:14:02 UTC"),
            Property::new("Architecture", "x86_64"),
        ]
    }

    fn props_cpu(&self) -> Vec<Property> {
        let cpu = &self.cpu_info;
        let mut props = vec![
            Property::new("Processor Name", &cpu.brand),
            Property::new("Vendor", &cpu.vendor),
            Property::new("Family", &format!("{}", cpu.family)),
            Property::new("Model", &format!("{}", cpu.model)),
            Property::new("Stepping", &format!("{}", cpu.stepping)),
            Property::new("Physical Cores", &format!("{}", cpu.physical_cores)),
            Property::new("Logical Processors", &format!("{}", cpu.logical_processors)),
            Property::new("Base Clock", &format!("{} MHz", cpu.base_clock_mhz)),
            Property::new("Max Turbo Clock", &format!("{} MHz", cpu.max_turbo_mhz)),
            Property::new("L1 Data Cache", &format!("{} KiB (per core)", cpu.l1_data_kb)),
            Property::new("L1 Instruction Cache", &format!("{} KiB (per core)", cpu.l1_inst_kb)),
            Property::new("L2 Cache", &format!("{} KiB (per core)", cpu.l2_kb)),
            Property::new("L3 Cache", &format!("{} KiB (shared)", cpu.l3_kb)),
            Property::new("Architecture", "x86_64"),
            Property::new("", ""),
            Property::new("--- CPU Features ---", ""),
        ];
        for (feature, supported) in &cpu.features {
            let mark = if *supported { "\u{2713}" } else { "\u{2717}" };
            props.push(Property::new(feature, mark));
        }
        props
    }

    fn props_memory(&self) -> Vec<Property> {
        let mem = &self.memory_info;
        let mut props = vec![
            Property::new("Total Installed", &format!("{} MiB ({:.1} GiB)", mem.total_mb, mem.total_mb as f64 / 1024.0)),
            Property::new("Available", &format!("{} MiB ({:.1} GiB)", mem.available_mb, mem.available_mb as f64 / 1024.0)),
            Property::new("Memory Type", &mem.mem_type),
            Property::new("Speed", &format!("{} MHz", mem.speed_mhz)),
            Property::new("Slots Used / Total", &format!("{} / {}", mem.slots_used, mem.slots_total)),
            Property::new("", ""),
            Property::new("--- Per-Slot Details ---", ""),
        ];
        for slot in &mem.slots {
            props.push(Property::new("", ""));
            props.push(Property::new("Slot", &slot.slot_name));
            props.push(Property::new("  Size", &format!("{} MiB", slot.size_mb)));
            props.push(Property::new("  Type", &slot.mem_type));
            props.push(Property::new("  Speed", &format!("{} MHz", slot.speed_mhz)));
            props.push(Property::new("  Manufacturer", &slot.manufacturer));
        }
        props
    }

    fn props_storage(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, disk) in self.disks.iter().enumerate() {
            if idx > 0 {
                props.push(Property::new("", ""));
            }
            props.push(Property::new(&format!("--- Disk {} ---", idx), ""));
            props.push(Property::new("Model", &disk.model));
            props.push(Property::new("Capacity", &format!("{:.1} GB", disk.capacity_gb)));
            props.push(Property::new("Interface", &disk.interface));
            props.push(Property::new("Serial", &disk.serial));
            props.push(Property::new("S.M.A.R.T. Status", &disk.smart_status));
            for part in &disk.partitions {
                props.push(Property::new("", ""));
                props.push(Property::new("  Partition", &part.label));
                props.push(Property::new("  Filesystem", &part.filesystem));
                props.push(Property::new("  Capacity", &format!("{:.1} GB", part.capacity_gb)));
                props.push(Property::new("  Used", &format!("{:.1} GB", part.used_gb)));
                props.push(Property::new("  Free", &format!("{:.1} GB", part.free_gb)));
                props.push(Property::new("  Mount", &part.mount_point));
            }
        }
        props
    }

    fn props_display(&self) -> Vec<Property> {
        let d = &self.display_info;
        let mut props = vec![
            Property::new("GPU Name", &d.gpu_name),
            Property::new("Vendor", &d.vendor),
            Property::new("VRAM", &format!("{} MiB ({:.1} GiB)", d.vram_mb, d.vram_mb as f64 / 1024.0)),
            Property::new("Resolution", &d.resolution),
            Property::new("Refresh Rate", &format!("{} Hz", d.refresh_rate_hz)),
            Property::new("Driver Version", &d.driver_version),
            Property::new("", ""),
            Property::new("--- Display Outputs ---", ""),
        ];
        for (output, connected) in &d.outputs {
            let status = if *connected { "Connected" } else { "Disconnected" };
            props.push(Property::new(output, status));
        }
        props
    }

    fn props_sound(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, snd) in self.sound_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::new("", ""));
            }
            props.push(Property::new("Name", &snd.name));
            props.push(Property::new("Type", &snd.device_type));
            props.push(Property::new("Driver", &snd.driver));
            props.push(Property::new("Status", &snd.status));
        }
        props
    }

    fn props_network(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, adapter) in self.network_adapters.iter().enumerate() {
            if idx > 0 {
                props.push(Property::new("", ""));
            }
            props.push(Property::new(&format!("--- Adapter {} ---", idx), ""));
            props.push(Property::new("Name", &adapter.name));
            props.push(Property::new("Type", &adapter.adapter_type));
            props.push(Property::new("MAC Address", &adapter.mac_address));
            props.push(Property::new("IPv4 Address", &adapter.ipv4));
            props.push(Property::new("IPv6 Address", &adapter.ipv6));
            props.push(Property::new("Subnet Mask", &adapter.subnet));
            props.push(Property::new("Default Gateway", &adapter.gateway));
            props.push(Property::new("DNS Servers", &adapter.dns));
            props.push(Property::new("Speed", &format!("{} Mbps", adapter.speed_mbps)));
            props.push(Property::new("Duplex", &adapter.duplex));
            props.push(Property::new("Bytes Sent", &format_bytes(adapter.bytes_sent)));
            props.push(Property::new("Bytes Received", &format_bytes(adapter.bytes_received)));
        }
        props
    }

    fn props_usb(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, dev) in self.usb_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::new("", ""));
            }
            props.push(Property::new("Port", &dev.port));
            props.push(Property::new("Description", &dev.description));
            props.push(Property::new("Vendor:Product", &format!("{:04X}:{:04X}", dev.vendor_id, dev.product_id)));
            props.push(Property::new("Speed", &dev.speed));
        }
        props
    }

    fn props_pci(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, dev) in self.pci_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::new("", ""));
            }
            props.push(Property::new(
                "BDF",
                &format!("{:02X}:{:02X}.{}", dev.bus, dev.device, dev.function),
            ));
            props.push(Property::new("Vendor:Device", &format!("{:04X}:{:04X}", dev.vendor_id, dev.device_id)));
            props.push(Property::new("Vendor", &dev.vendor_name));
            props.push(Property::new("Class", &dev.class));
            props.push(Property::new("Description", &dev.description));
        }
        props
    }

    fn props_services(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("Name", "Status / Start Type"),
            Property::new("---", "---"),
        ];
        for svc in &self.services {
            props.push(Property::new(
                &svc.name,
                &format!("{} ({})", svc.status, svc.start_type),
            ));
        }
        props
    }

    fn props_processes(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("PID  Name", "Memory / CPU"),
            Property::new("---", "---"),
        ];
        for proc_entry in &self.processes {
            props.push(Property::new(
                &format!("{:<5} {}", proc_entry.pid, proc_entry.name),
                &format!("{} KiB / {:.1}%", proc_entry.memory_kb, proc_entry.cpu_percent),
            ));
        }
        props
    }

    fn props_drivers(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("Name", "Path / Status"),
            Property::new("---", "---"),
        ];
        for drv in &self.drivers {
            props.push(Property::new(&drv.name, &format!("{} [{}]", drv.path, drv.status)));
        }
        props
    }

    fn props_env_vars(&self) -> Vec<Property> {
        self.env_vars
            .iter()
            .map(|(k, v)| Property::new(k, v))
            .collect()
    }

    fn props_startup(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for entry in &self.startup_programs {
            props.push(Property::new(&entry.name, &format!("{} ({})", entry.path, entry.source)));
        }
        props
    }

    fn props_irqs(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("IRQ #", "Device / Type"),
            Property::new("---", "---"),
        ];
        for irq in &self.irqs {
            props.push(Property::new(
                &format!("IRQ {}", irq.irq_number),
                &format!("{} ({})", irq.device, irq.irq_type),
            ));
        }
        props
    }

    fn props_io_ports(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("Range", "Device"),
            Property::new("---", "---"),
        ];
        for port in &self.io_ports {
            props.push(Property::new(
                &format!("{:#06X}-{:#06X}", port.start, port.end),
                &port.device,
            ));
        }
        props
    }

    fn props_memory_map(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("Range", "Type / Description"),
            Property::new("---", "---"),
        ];
        for entry in &self.memory_map {
            props.push(Property::new(
                &format!("{:#012X}-{:#012X}", entry.start, entry.end),
                &format!("{}: {}", entry.region_type, entry.description),
            ));
        }
        props
    }

    fn props_dma(&self) -> Vec<Property> {
        let mut props = vec![
            Property::new("Channel", "Device / Mode"),
            Property::new("---", "---"),
        ];
        for dma in &self.dma_channels {
            props.push(Property::new(
                &format!("DMA {}", dma.channel),
                &format!("{} ({})", dma.device, dma.mode),
            ));
        }
        props
    }

    // ========================================================================
    // Tree navigation helpers
    // ========================================================================

    /// Build a flat list of visible tree rows (respecting expand/collapse).
    pub fn visible_tree_rows(&self) -> Vec<SysInfoCategory> {
        let mut rows = Vec::new();
        for &root in TREE_ROOT_ITEMS {
            rows.push(root);
            if root.is_parent() && self.expanded.contains(&root) {
                for &child in root.children() {
                    rows.push(child);
                }
            }
        }
        rows
    }

    /// Toggle expansion of a parent node.
    pub fn toggle_expand(&mut self, cat: SysInfoCategory) {
        if cat.is_parent() {
            if let Some(pos) = self.expanded.iter().position(|c| *c == cat) {
                self.expanded.remove(pos);
            } else {
                self.expanded.push(cat);
            }
        }
    }

    /// Select the next visible tree row.
    pub fn select_next(&mut self) {
        let rows = self.visible_tree_rows();
        if let Some(pos) = rows.iter().position(|c| *c == self.selected_category) {
            if pos + 1 < rows.len() {
                if let Some(&next) = rows.get(pos + 1) {
                    self.selected_category = next;
                    self.detail_scroll = 0.0;
                }
            }
        }
    }

    /// Select the previous visible tree row.
    pub fn select_prev(&mut self) {
        let rows = self.visible_tree_rows();
        if let Some(pos) = rows.iter().position(|c| *c == self.selected_category) {
            if pos > 0 {
                if let Some(&prev) = rows.get(pos - 1) {
                    self.selected_category = prev;
                    self.detail_scroll = 0.0;
                }
            }
        }
    }

    /// Expand the selected node (or select first child if already expanded).
    pub fn expand_selected(&mut self) {
        let cat = self.selected_category;
        if cat.is_parent() {
            if !self.expanded.contains(&cat) {
                self.expanded.push(cat);
            } else {
                // Already expanded: move to first child.
                let children = cat.children();
                if let Some(&first) = children.first() {
                    self.selected_category = first;
                    self.detail_scroll = 0.0;
                }
            }
        }
    }

    /// Collapse the selected node or move to parent.
    pub fn collapse_selected(&mut self) {
        let cat = self.selected_category;
        if cat.is_parent() && self.expanded.contains(&cat) {
            // Collapse it.
            if let Some(pos) = self.expanded.iter().position(|c| *c == cat) {
                self.expanded.remove(pos);
            }
        } else if let Some(parent) = cat.parent() {
            // Move to parent.
            self.selected_category = parent;
            self.detail_scroll = 0.0;
        }
    }

    /// Search all categories for a text match and return matching properties.
    pub fn search_all(&self, query: &str) -> Vec<(SysInfoCategory, Property)> {
        if query.is_empty() {
            return Vec::new();
        }
        let q = query.to_lowercase();
        let all_categories = [
            SysInfoCategory::SystemSummary,
            SysInfoCategory::HwIrqs,
            SysInfoCategory::HwIoPorts,
            SysInfoCategory::HwMemoryMap,
            SysInfoCategory::HwDma,
            SysInfoCategory::CompCpu,
            SysInfoCategory::CompMemory,
            SysInfoCategory::CompStorage,
            SysInfoCategory::CompDisplay,
            SysInfoCategory::CompSound,
            SysInfoCategory::CompNetwork,
            SysInfoCategory::CompUsb,
            SysInfoCategory::CompPci,
            SysInfoCategory::SwServices,
            SysInfoCategory::SwProcesses,
            SysInfoCategory::SwDrivers,
            SysInfoCategory::SwEnvVars,
            SysInfoCategory::SwStartupPrograms,
        ];

        let mut results = Vec::new();
        let old_cat = self.selected_category;
        for &cat in &all_categories {
            let props = match cat {
                SysInfoCategory::SystemSummary => self.props_system_summary(),
                SysInfoCategory::CompCpu => self.props_cpu(),
                SysInfoCategory::CompMemory => self.props_memory(),
                SysInfoCategory::CompStorage => self.props_storage(),
                SysInfoCategory::CompDisplay => self.props_display(),
                SysInfoCategory::CompSound => self.props_sound(),
                SysInfoCategory::CompNetwork => self.props_network(),
                SysInfoCategory::CompUsb => self.props_usb(),
                SysInfoCategory::CompPci => self.props_pci(),
                SysInfoCategory::SwServices => self.props_services(),
                SysInfoCategory::SwProcesses => self.props_processes(),
                SysInfoCategory::SwDrivers => self.props_drivers(),
                SysInfoCategory::SwEnvVars => self.props_env_vars(),
                SysInfoCategory::SwStartupPrograms => self.props_startup(),
                SysInfoCategory::HwIrqs => self.props_irqs(),
                SysInfoCategory::HwIoPorts => self.props_io_ports(),
                SysInfoCategory::HwMemoryMap => self.props_memory_map(),
                SysInfoCategory::HwDma => self.props_dma(),
                _ => Vec::new(),
            };
            for prop in props {
                if prop.name.to_lowercase().contains(&q)
                    || prop.value.to_lowercase().contains(&q)
                {
                    results.push((cat, prop));
                }
            }
        }
        let _ = old_cat; // suppress unused warning
        results
    }

    /// Export all system information as a text report.
    pub fn export_text(&self) -> String {
        let mut out = String::with_capacity(4096);
        out.push_str("=== OurOS System Information Report ===\n\n");

        let sections: &[(SysInfoCategory, &str)] = &[
            (SysInfoCategory::SystemSummary, "System Summary"),
            (SysInfoCategory::CompCpu, "CPU"),
            (SysInfoCategory::CompMemory, "Memory"),
            (SysInfoCategory::CompStorage, "Storage"),
            (SysInfoCategory::CompDisplay, "Display"),
            (SysInfoCategory::CompSound, "Sound"),
            (SysInfoCategory::CompNetwork, "Network"),
            (SysInfoCategory::CompUsb, "USB Devices"),
            (SysInfoCategory::CompPci, "PCI Devices"),
            (SysInfoCategory::HwIrqs, "IRQs"),
            (SysInfoCategory::HwIoPorts, "I/O Ports"),
            (SysInfoCategory::HwMemoryMap, "Memory Map"),
            (SysInfoCategory::HwDma, "DMA Channels"),
            (SysInfoCategory::SwServices, "Services"),
            (SysInfoCategory::SwProcesses, "Processes"),
            (SysInfoCategory::SwDrivers, "Drivers"),
            (SysInfoCategory::SwEnvVars, "Environment Variables"),
            (SysInfoCategory::SwStartupPrograms, "Startup Programs"),
        ];

        for (cat, heading) in sections {
            out.push_str(&format!("--- {} ---\n", heading));
            let props = match *cat {
                SysInfoCategory::SystemSummary => self.props_system_summary(),
                SysInfoCategory::CompCpu => self.props_cpu(),
                SysInfoCategory::CompMemory => self.props_memory(),
                SysInfoCategory::CompStorage => self.props_storage(),
                SysInfoCategory::CompDisplay => self.props_display(),
                SysInfoCategory::CompSound => self.props_sound(),
                SysInfoCategory::CompNetwork => self.props_network(),
                SysInfoCategory::CompUsb => self.props_usb(),
                SysInfoCategory::CompPci => self.props_pci(),
                SysInfoCategory::SwServices => self.props_services(),
                SysInfoCategory::SwProcesses => self.props_processes(),
                SysInfoCategory::SwDrivers => self.props_drivers(),
                SysInfoCategory::SwEnvVars => self.props_env_vars(),
                SysInfoCategory::SwStartupPrograms => self.props_startup(),
                SysInfoCategory::HwIrqs => self.props_irqs(),
                SysInfoCategory::HwIoPorts => self.props_io_ports(),
                SysInfoCategory::HwMemoryMap => self.props_memory_map(),
                SysInfoCategory::HwDma => self.props_dma(),
                _ => Vec::new(),
            };
            for prop in &props {
                if prop.name.is_empty() && prop.value.is_empty() {
                    out.push('\n');
                } else if prop.value.is_empty() {
                    out.push_str(&format!("{}\n", prop.name));
                } else {
                    out.push_str(&format!("  {}: {}\n", prop.name, prop.value));
                }
            }
            out.push('\n');
        }
        out
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Process an incoming event. Returns whether the event was consumed.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // Search box input handling.
        if self.search_focused {
            return self.handle_search_key(key);
        }

        match key.key {
            // Navigation
            Key::Up if key.modifiers == Modifiers::NONE => {
                self.select_prev();
                EventResult::Consumed
            }
            Key::Down if key.modifiers == Modifiers::NONE => {
                self.select_next();
                EventResult::Consumed
            }
            Key::Right if key.modifiers == Modifiers::NONE => {
                self.expand_selected();
                EventResult::Consumed
            }
            Key::Left if key.modifiers == Modifiers::NONE => {
                self.collapse_selected();
                EventResult::Consumed
            }
            // Scroll detail view
            Key::PageDown => {
                self.detail_scroll += 200.0;
                EventResult::Consumed
            }
            Key::PageUp => {
                self.detail_scroll = (self.detail_scroll - 200.0).max(0.0);
                EventResult::Consumed
            }
            // Ctrl+F = open search
            Key::F if key.modifiers.ctrl => {
                self.search_focused = true;
                EventResult::Consumed
            }
            // Ctrl+C = copy selected value (simulated)
            Key::C if key.modifiers.ctrl => {
                self.status_message = "Value copied to clipboard".to_string();
                EventResult::Consumed
            }
            // Ctrl+E = export
            Key::E if key.modifiers.ctrl => {
                let _report = self.export_text();
                self.status_message = "Exported system info to file".to_string();
                EventResult::Consumed
            }
            // Escape = close search
            Key::Escape => {
                if !self.search_text.is_empty() {
                    self.search_text.clear();
                }
                self.search_focused = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_search_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.search_focused = false;
                EventResult::Consumed
            }
            Key::Enter => {
                // Navigate to first search result.
                let results = self.search_all(&self.search_text);
                if let Some((cat, _)) = results.first() {
                    self.selected_category = *cat;
                    // Expand parent if needed.
                    if let Some(parent) = cat.parent() {
                        if !self.expanded.contains(&parent) {
                            self.expanded.push(parent);
                        }
                    }
                    self.detail_scroll = 0.0;
                    self.status_message = format!("{} results found", results.len());
                } else {
                    self.status_message = "No results found".to_string();
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                self.search_text.pop();
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text {
                    if !ch.is_control() {
                        self.search_text.push(ch);
                    }
                }
                EventResult::Consumed
            }
        }
    }

    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Check if click is in the sidebar.
                if mouse.x < SIDEBAR_WIDTH && mouse.y > TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT {
                    let row_y = mouse.y - TITLE_BAR_HEIGHT - TOOLBAR_HEIGHT + self.tree_scroll;
                    let row_idx = (row_y / TREE_ROW_HEIGHT) as usize;
                    let rows = self.visible_tree_rows();
                    if let Some(&cat) = rows.get(row_idx) {
                        if cat.is_parent() {
                            self.toggle_expand(cat);
                        }
                        self.selected_category = cat;
                        self.detail_scroll = 0.0;
                    }
                    return EventResult::Consumed;
                }
            }
            MouseEventKind::Scroll { dy, .. } => {
                if mouse.x < SIDEBAR_WIDTH {
                    self.tree_scroll = (self.tree_scroll - dy * 20.0).max(0.0);
                } else {
                    self.detail_scroll = (self.detail_scroll - dy * 20.0).max(0.0);
                }
                return EventResult::Consumed;
            }
            MouseEventKind::Move => {
                if mouse.x < SIDEBAR_WIDTH && mouse.y > TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT {
                    let row_y = mouse.y - TITLE_BAR_HEIGHT - TOOLBAR_HEIGHT + self.tree_scroll;
                    let row_idx = (row_y / TREE_ROW_HEIGHT) as usize;
                    self.hovered_tree_row = Some(row_idx);
                } else {
                    self.hovered_tree_row = None;
                }
                return EventResult::Consumed;
            }
            _ => {}
        }
        EventResult::Ignored
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Produce a full render tree for the current state.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background fill.
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, COLOR_BASE);

        // Title bar.
        self.render_title_bar(&mut tree);
        // Toolbar (search, export buttons).
        self.render_toolbar(&mut tree);
        // Sidebar tree.
        self.render_sidebar(&mut tree);
        // Detail pane.
        self.render_detail_pane(&mut tree);
        // Status bar.
        self.render_status_bar(&mut tree);

        tree
    }

    fn render_title_bar(&self, tree: &mut RenderTree) {
        tree.fill_rect(0.0, 0.0, self.window_width, TITLE_BAR_HEIGHT, COLOR_TITLE_BG);

        // Title text.
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "System Information".to_string(),
            color: COLOR_TEXT,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: TITLE_BAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: TITLE_BAR_HEIGHT - 1.0,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });
    }

    fn render_toolbar(&self, tree: &mut RenderTree) {
        let y = TITLE_BAR_HEIGHT;
        tree.fill_rect(0.0, y, self.window_width, TOOLBAR_HEIGHT, COLOR_TOOLBAR_BG);

        // Search box.
        let search_x = 8.0;
        let search_y = y + 5.0;
        let search_w = 220.0;
        let search_h = 22.0;

        tree.push(RenderCommand::FillRect {
            x: search_x,
            y: search_y,
            width: search_w,
            height: search_h,
            color: COLOR_SEARCH_BG,
            corner_radii: CornerRadii::all(3.0),
        });

        let border_color = if self.search_focused {
            COLOR_BLUE
        } else {
            COLOR_SEARCH_BORDER
        };
        tree.push(RenderCommand::StrokeRect {
            x: search_x,
            y: search_y,
            width: search_w,
            height: search_h,
            color: border_color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });

        let search_display = if self.search_text.is_empty() {
            "Search (Ctrl+F)..."
        } else {
            &self.search_text
        };
        let search_color = if self.search_text.is_empty() {
            COLOR_OVERLAY
        } else {
            COLOR_TEXT
        };
        tree.push(RenderCommand::Text {
            x: search_x + 6.0,
            y: search_y + 4.0,
            text: search_display.to_string(),
            color: search_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(search_w - 12.0),
        });

        // Export button.
        let export_x = search_x + search_w + 16.0;
        let btn_w = 70.0;
        tree.push(RenderCommand::FillRect {
            x: export_x,
            y: search_y,
            width: btn_w,
            height: search_h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::Text {
            x: export_x + 10.0,
            y: search_y + 4.0,
            text: "Export".to_string(),
            color: COLOR_SUBTEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Copy button.
        let copy_x = export_x + btn_w + 8.0;
        tree.push(RenderCommand::FillRect {
            x: copy_x,
            y: search_y,
            width: btn_w,
            height: search_h,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::Text {
            x: copy_x + 14.0,
            y: search_y + 4.0,
            text: "Copy".to_string(),
            color: COLOR_SUBTEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TOOLBAR_HEIGHT - 1.0,
            x2: self.window_width,
            y2: y + TOOLBAR_HEIGHT - 1.0,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });
    }

    fn render_sidebar(&self, tree: &mut RenderTree) {
        let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
        let height = self.window_height - top - STATUS_BAR_HEIGHT;

        // Sidebar background.
        tree.fill_rect(0.0, top, SIDEBAR_WIDTH, height, COLOR_SIDEBAR_BG);

        // Clip to sidebar area.
        tree.clip(0.0, top, SIDEBAR_WIDTH, height);
        tree.translate(0.0, -self.tree_scroll);

        let rows = self.visible_tree_rows();
        for (idx, &cat) in rows.iter().enumerate() {
            let row_y = top + idx as f32 * TREE_ROW_HEIGHT;
            let depth = cat.depth();
            let indent = 12.0 + depth as f32 * TREE_INDENT;

            // Row background (selected or hovered).
            let bg = if cat == self.selected_category {
                COLOR_TREE_SELECTED
            } else if self.hovered_tree_row == Some(idx) {
                COLOR_TREE_HOVER
            } else {
                Color::TRANSPARENT
            };

            if bg != Color::TRANSPARENT {
                tree.fill_rect(0.0, row_y, SIDEBAR_WIDTH, TREE_ROW_HEIGHT, bg);
            }

            // Expand/collapse indicator for parent nodes.
            if cat.is_parent() {
                let arrow = if self.expanded.contains(&cat) {
                    "\u{25BC}" // down triangle
                } else {
                    "\u{25B6}" // right triangle
                };
                tree.push(RenderCommand::Text {
                    x: indent - 14.0,
                    y: row_y + 5.0,
                    text: arrow.to_string(),
                    color: COLOR_OVERLAY,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Label.
            let text_color = if cat == self.selected_category {
                COLOR_BLUE
            } else {
                COLOR_TEXT
            };
            tree.push(RenderCommand::Text {
                x: indent,
                y: row_y + 5.0,
                text: cat.label().to_string(),
                color: text_color,
                font_size: 13.0,
                font_weight: if cat == self.selected_category {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_WIDTH - indent - 8.0),
            });
        }

        tree.untranslate();
        tree.unclip();

        // Sidebar right border.
        tree.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH - 1.0,
            y1: top,
            x2: SIDEBAR_WIDTH - 1.0,
            y2: top + height,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });
    }

    fn render_detail_pane(&self, tree: &mut RenderTree) {
        let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
        let left = SIDEBAR_WIDTH;
        let width = self.window_width - SIDEBAR_WIDTH;
        let height = self.window_height - top - STATUS_BAR_HEIGHT;

        // Background.
        tree.fill_rect(left, top, width, height, COLOR_SURFACE0);

        // Clip to detail area.
        tree.clip(left, top, width, height);

        // Category heading.
        let heading_y = top + 8.0;
        tree.push(RenderCommand::Text {
            x: left + 16.0,
            y: heading_y,
            text: self.selected_category.label().to_string(),
            color: COLOR_LAVENDER,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
        });

        // Separator below heading.
        let sep_y = heading_y + 22.0;
        tree.push(RenderCommand::Line {
            x1: left + 16.0,
            y1: sep_y,
            x2: left + width - 16.0,
            y2: sep_y,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });

        // Property table.
        let table_top = sep_y + 8.0;
        let name_col_width = width * 0.38;

        // Header row.
        tree.fill_rect(left, table_top, width, PROPERTY_HEADER_HEIGHT, COLOR_MANTLE);
        tree.push(RenderCommand::Text {
            x: left + 16.0,
            y: table_top + 6.0,
            text: "Property".to_string(),
            color: COLOR_SUBTEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(name_col_width - 20.0),
        });
        tree.push(RenderCommand::Text {
            x: left + name_col_width + 8.0,
            y: table_top + 6.0,
            text: "Value".to_string(),
            color: COLOR_SUBTEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - name_col_width - 24.0),
        });

        // Property rows.
        let props = self.current_properties();
        let content_top = table_top + PROPERTY_HEADER_HEIGHT;

        tree.translate(0.0, -self.detail_scroll);

        for (idx, prop) in props.iter().enumerate() {
            let row_y = content_top + idx as f32 * PROPERTY_ROW_HEIGHT;

            // Skip rows above clip area (accounting for scroll).
            if row_y + PROPERTY_ROW_HEIGHT - self.detail_scroll < content_top {
                continue;
            }
            // Stop rendering below visible area.
            if row_y - self.detail_scroll > top + height {
                break;
            }

            // Alternating row color.
            let row_bg = if idx % 2 == 0 { COLOR_ROW_EVEN } else { COLOR_ROW_ODD };
            tree.fill_rect(left, row_y, width, PROPERTY_ROW_HEIGHT, row_bg);

            // Section headers get different styling.
            let is_section = prop.name.starts_with("---");
            let name_color = if is_section { COLOR_PEACH } else { COLOR_SUBTEXT };
            let value_color = if is_section { COLOR_PEACH } else { COLOR_TEXT };

            // Name.
            if !prop.name.is_empty() {
                tree.push(RenderCommand::Text {
                    x: left + 16.0,
                    y: row_y + 4.0,
                    text: prop.name.clone(),
                    color: name_color,
                    font_size: 12.0,
                    font_weight: if is_section {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(name_col_width - 20.0),
                });
            }

            // Value.
            if !prop.value.is_empty() {
                // Color checkmarks green and X marks red.
                let val_color = if prop.value == "\u{2713}" {
                    COLOR_GREEN
                } else if prop.value == "\u{2717}" {
                    COLOR_RED
                } else {
                    value_color
                };
                tree.push(RenderCommand::Text {
                    x: left + name_col_width + 8.0,
                    y: row_y + 4.0,
                    text: prop.value.clone(),
                    color: val_color,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - name_col_width - 24.0),
                });
            }
        }

        tree.untranslate();
        tree.unclip();
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        tree.fill_rect(0.0, y, self.window_width, STATUS_BAR_HEIGHT, COLOR_STATUS_BG);

        // Top separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });

        // Status text.
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: y + 5.0,
            text: self.status_message.clone(),
            color: COLOR_SUBTEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width * 0.5),
        });

        // Category indicator on the right.
        let cat_text = format!("Category: {}", self.selected_category.label());
        tree.push(RenderCommand::Text {
            x: self.window_width - 300.0,
            y: y + 5.0,
            text: cat_text,
            color: COLOR_OVERLAY,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(280.0),
        });
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a byte count in human-readable form.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut app = SysInfoState::new();

    // Render the initial view.
    let render_tree = app.render();
    println!("System Information Explorer initialized");
    println!("  Selected: {}", app.selected_category.label());
    println!("  Tree rows visible: {}", app.visible_tree_rows().len());
    println!("  Render commands: {}", render_tree.len());

    // Demonstrate navigation.
    app.select_next(); // Hardware Resources
    app.select_next(); // IRQs (expanded)
    println!("\nNavigated to: {}", app.selected_category.label());

    // Demonstrate expand/collapse.
    app.selected_category = SysInfoCategory::Components;
    app.collapse_selected();
    println!(
        "Collapsed Components: {} tree rows",
        app.visible_tree_rows().len()
    );
    app.expand_selected();
    println!(
        "Expanded Components: {} tree rows",
        app.visible_tree_rows().len()
    );

    // Render CPU page.
    app.selected_category = SysInfoCategory::CompCpu;
    let cpu_tree = app.render();
    println!("\nCPU page: {} render commands", cpu_tree.len());
    println!("  Properties: {}", app.current_properties().len());

    // Demonstrate search.
    let results = app.search_all("NVMe");
    println!("\nSearch 'NVMe': {} results", results.len());
    for (cat, prop) in results.iter().take(3) {
        println!("  [{:?}] {} = {}", cat, prop.name, prop.value);
    }

    // Export demo.
    let report = app.export_text();
    println!("\nExport report: {} bytes", report.len());

    println!("\nSystem Information Explorer ready.");
}
