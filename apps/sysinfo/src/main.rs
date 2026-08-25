//! Slate OS System Information Explorer
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
//! through Slate OS syscalls; stubbed with representative data for initial
//! development.

pub mod hwquery;

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::fold;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::{scroll_window, wheel};

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
/// Gap between the top of the detail pane and its category heading.
const DETAIL_HEADING_TOP: f32 = 8.0;
/// Distance from the heading's top down to the separator below it.
const DETAIL_HEADING_HEIGHT: f32 = 22.0;
/// Gap between that separator and the top of the property table.
const DETAIL_SEPARATOR_GAP: f32 = 8.0;

/// Distance from the top of a scroll window down to the `slot`-th drawn row.
///
/// `slot` counts from the first row *on screen*, so it is bounded by the pane
/// height divided by the row height — a few dozen. The saturating cast can
/// therefore never reach a slot that is actually drawn, and exists only so this
/// is total for a nonsense argument.
fn slot_offset(slot: usize, row_h: f32) -> f32 {
    f32::from(u16::try_from(slot).unwrap_or(u16::MAX)) * row_h
}
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

/// Whether a row is structure the report wrote, or data the report is showing.
///
/// Both consumers of a [`Property`] need to know which, and before this
/// existed both *guessed* -- from the strings, which is the one place the
/// answer cannot be:
///
/// - [`SysInfoApp::export_text`] treated an empty value as "this row is a
///   heading", and wrote its name at column 0, where the report's own
///   `--- Display Outputs ---` headings live.
/// - The detail-pane renderer treated a name beginning `---` as "this row is
///   a heading", and drew it bold and in the accent colour.
///
/// Neither guess is answerable from a string, because the strings are not
/// ours: they are environment variables, PCI vendor names and process names.
/// An environment variable set to the empty string -- `FOO=`, which is legal
/// and ordinary -- was enough to satisfy the first guess and print its own
/// name as a section header of the system report. A variable named `---x`
/// satisfied the second.
///
/// So the distinction is recorded at construction, by the code that knows the
/// answer, rather than re-derived by each consumer from data that cannot
/// carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    /// A heading this report wrote for itself. May occupy column 0.
    Heading,
    /// Vertical space this report inserted between groups.
    Blank,
    /// A name/value pair derived from data. Always indented, never a heading.
    Field,
}

/// A name-value property displayed in the detail pane.
#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub kind: PropertyKind,
}

impl Property {
    /// A row derived from data.
    ///
    /// Both halves are folded to a single line. They may come from a PCI
    /// descriptor, an environment variable or a process name, and a newline in
    /// any of them would put that data on a line of its own in the text
    /// export -- which is exactly where a heading would be. See
    /// [`guitk::fold`].
    fn new(name: &str, value: &str) -> Self {
        Self {
            name: fold::line(name),
            value: fold::line(value),
            kind: PropertyKind::Field,
        }
    }

    /// A heading this report writes for itself.
    ///
    /// `text` carries its own `--- ... ---` decoration rather than having it
    /// added here, because the two consumers decorate differently: the export
    /// prints it literally and the detail pane draws it bold. Callers pass a
    /// string literal, so there is nothing to fold.
    fn heading(text: &str) -> Self {
        Self {
            name: text.to_string(),
            value: String::new(),
            kind: PropertyKind::Heading,
        }
    }

    /// A blank separator row.
    fn blank() -> Self {
        Self {
            name: String::new(),
            value: String::new(),
            kind: PropertyKind::Blank,
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
///
/// Sizes are raw byte counts, not pre-scaled gigabytes. They used to be
/// `f32` fields named `*_gb` holding values divided by 1024³ and displayed
/// with a `GB` label — a 2 TB disk read `1863.0 GB`, which is neither its
/// capacity in GB (2000) nor a unit anyone sells. Keeping bytes and scaling
/// at the point of display means the divisor and the unit name are chosen
/// together, by `guitk::bytes`. See design-decisions.md §489.
#[derive(Clone, Debug)]
pub struct PartitionInfo {
    pub label: String,
    pub filesystem: String,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub mount_point: String,
}

/// Disk information.
#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub model: String,
    /// Raw byte count; see [`PartitionInfo`] for why this is not pre-scaled.
    pub capacity_bytes: u64,
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
    /// First visible property row of the detail pane, as an index into
    /// [`current_properties`](SysInfoState::current_properties).
    ///
    /// A row index rather than a pixel offset: the pane draws whole
    /// `PROPERTY_ROW_HEIGHT` rows and nothing else, so a pixel offset could
    /// only ever express positions the renderer rounds away. It used to be an
    /// `f32` that the wheel moved by `dy * 20.0` — misreading a count of wheel
    /// notches as a pixel distance — and that nothing bounded at the far end,
    /// so scrolling past the last property kept climbing while the table stood
    /// still and the same distance had to be scrolled back before anything
    /// moved.
    pub detail_scroll: usize,
    /// First visible row of the sidebar tree, as an index into
    /// [`visible_tree_rows`](SysInfoState::visible_tree_rows).
    ///
    /// Same units and the same history as [`Self::detail_scroll`].
    pub tree_scroll: usize,
    /// Wheel remainder for the sidebar; see [`wheel::Accumulator`].
    ///
    /// One per pane. Sharing a single accumulator would let a half-notch banked
    /// over the tree come out later as a step in the property table.
    tree_wheel: wheel::Accumulator,
    /// Wheel remainder for the detail pane.
    detail_wheel: wheel::Accumulator,
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

impl Default for SysInfoState {
    fn default() -> Self {
        Self::new()
    }
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
            detail_scroll: 0,
            tree_scroll: 0,
            tree_wheel: wheel::Accumulator::default(),
            detail_wheel: wheel::Accumulator::default(),
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
            brand: "Slate OS Virtual CPU @ 3.60GHz".to_string(),
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
            // Byte counts as a drive actually reports them: a "2 TB" NVMe is
            // 2.0×10¹² bytes, which is 1.82 TiB. The partitions below sum
            // exactly to the disk's capacity, which the pre-scaled gigabyte
            // figures they replaced did not.
            DiskInfo {
                model: "Samsung 990 Pro 2TB".to_string(),
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
                    PartitionInfo {
                        label: "Home".to_string(),
                        filesystem: "ext4".to_string(),
                        capacity_bytes: 1_462_991_191_104,
                        used_bytes: 905_000_000_000,
                        free_bytes: 557_991_191_104,
                        mount_point: "/home".to_string(),
                    },
                ],
            },
            DiskInfo {
                model: "WD Blue SN580 1TB".to_string(),
                capacity_bytes: 1_000_204_886_016,
                interface: "NVMe".to_string(),
                serial: "WD-WX32A0987654".to_string(),
                smart_status: "Healthy".to_string(),
                partitions: vec![PartitionInfo {
                    label: "Data".to_string(),
                    filesystem: "ext4".to_string(),
                    capacity_bytes: 1_000_204_886_016,
                    used_bytes: 443_000_000_000,
                    free_bytes: 557_204_886_016,
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
            ProcessEntry {
                pid: 5,
                name: "device-manager".to_string(),
                memory_kb: 45000,
                cpu_percent: 0.5,
            },
            ProcessEntry {
                pid: 8,
                name: "network-manager".to_string(),
                memory_kb: 32000,
                cpu_percent: 0.1,
            },
            ProcessEntry {
                pid: 12,
                name: "audio-mixer".to_string(),
                memory_kb: 24000,
                cpu_percent: 1.0,
            },
            ProcessEntry {
                pid: 15,
                name: "window-manager".to_string(),
                memory_kb: 86000,
                cpu_percent: 2.4,
            },
            ProcessEntry {
                pid: 20,
                name: "file-explorer".to_string(),
                memory_kb: 64000,
                cpu_percent: 0.8,
            },
            ProcessEntry {
                pid: 25,
                name: "terminal".to_string(),
                memory_kb: 18000,
                cpu_percent: 0.2,
            },
            ProcessEntry {
                pid: 30,
                name: "ssh-server".to_string(),
                memory_kb: 8000,
                cpu_percent: 0.0,
            },
            ProcessEntry {
                pid: 42,
                name: "sysinfo".to_string(),
                memory_kb: 52000,
                cpu_percent: 1.5,
            },
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
            (
                "PATH".to_string(),
                "/bin:/sbin:/usr/bin:/usr/local/bin".to_string(),
            ),
            ("HOME".to_string(), "/home/user".to_string()),
            ("SHELL".to_string(), "/bin/osh".to_string()),
            ("TERM".to_string(), "slateos-256color".to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string()),
            ("DISPLAY".to_string(), ":0".to_string()),
            ("EDITOR".to_string(), "/usr/bin/oedit".to_string()),
        ]
    }

    fn populate_irqs() -> Vec<IrqInfo> {
        vec![
            IrqInfo {
                irq_number: 0,
                device: "Timer".to_string(),
                irq_type: "Edge".to_string(),
            },
            IrqInfo {
                irq_number: 1,
                device: "Keyboard".to_string(),
                irq_type: "Edge".to_string(),
            },
            IrqInfo {
                irq_number: 8,
                device: "RTC".to_string(),
                irq_type: "Edge".to_string(),
            },
            IrqInfo {
                irq_number: 12,
                device: "Mouse".to_string(),
                irq_type: "Edge".to_string(),
            },
            IrqInfo {
                irq_number: 14,
                device: "NVMe SSD".to_string(),
                irq_type: "MSI-X".to_string(),
            },
            IrqInfo {
                irq_number: 16,
                device: "GPU".to_string(),
                irq_type: "MSI-X".to_string(),
            },
            IrqInfo {
                irq_number: 18,
                device: "Ethernet".to_string(),
                irq_type: "MSI".to_string(),
            },
            IrqInfo {
                irq_number: 19,
                device: "USB xHCI".to_string(),
                irq_type: "MSI".to_string(),
            },
            IrqInfo {
                irq_number: 22,
                device: "HD Audio".to_string(),
                irq_type: "MSI".to_string(),
            },
        ]
    }

    fn populate_io_ports() -> Vec<IoPortInfo> {
        vec![
            IoPortInfo {
                start: 0x0000,
                end: 0x001F,
                device: "DMA Controller".to_string(),
            },
            IoPortInfo {
                start: 0x0020,
                end: 0x0021,
                device: "PIC Master".to_string(),
            },
            IoPortInfo {
                start: 0x0040,
                end: 0x0043,
                device: "PIT Timer".to_string(),
            },
            IoPortInfo {
                start: 0x0060,
                end: 0x0064,
                device: "Keyboard Controller".to_string(),
            },
            IoPortInfo {
                start: 0x0070,
                end: 0x0071,
                device: "RTC/CMOS".to_string(),
            },
            IoPortInfo {
                start: 0x00A0,
                end: 0x00A1,
                device: "PIC Slave".to_string(),
            },
            IoPortInfo {
                start: 0x03F8,
                end: 0x03FF,
                device: "COM1 (Serial)".to_string(),
            },
            IoPortInfo {
                start: 0x0CF8,
                end: 0x0CFF,
                device: "PCI Configuration".to_string(),
            },
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
            DmaInfo {
                channel: 0,
                device: "Available".to_string(),
                mode: "N/A".to_string(),
            },
            DmaInfo {
                channel: 1,
                device: "Available".to_string(),
                mode: "N/A".to_string(),
            },
            DmaInfo {
                channel: 2,
                device: "Floppy (legacy)".to_string(),
                mode: "Single".to_string(),
            },
            DmaInfo {
                channel: 4,
                device: "Cascade".to_string(),
                mode: "Cascade".to_string(),
            },
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
            Property::new("OS Name", "Slate OS"),
            Property::new("OS Version", "1.0.0"),
            Property::new("OS Build", "2026.05.17-nightly"),
            Property::new("Kernel Version", "0.1.0-slateos"),
            Property::new("System Manufacturer", "SMBIOS: To Be Filled By O.E.M."),
            Property::new("Processor", &cpu.brand),
            Property::new(
                "Cores / Threads",
                &format!("{} / {}", cpu.physical_cores, cpu.logical_processors),
            ),
            Property::new("Base Frequency", &format!("{} MHz", cpu.base_clock_mhz)),
            Property::new(
                "Total Physical Memory",
                &format!(
                    "{} MiB ({:.1} GiB)",
                    mem.total_mb,
                    mem.total_mb as f64 / 1024.0
                ),
            ),
            Property::new(
                "Available Physical Memory",
                &format!(
                    "{} MiB ({:.1} GiB)",
                    mem.available_mb,
                    mem.available_mb as f64 / 1024.0
                ),
            ),
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
            Property::new(
                "L1 Data Cache",
                &format!("{} KiB (per core)", cpu.l1_data_kb),
            ),
            Property::new(
                "L1 Instruction Cache",
                &format!("{} KiB (per core)", cpu.l1_inst_kb),
            ),
            Property::new("L2 Cache", &format!("{} KiB (per core)", cpu.l2_kb)),
            Property::new("L3 Cache", &format!("{} KiB (shared)", cpu.l3_kb)),
            Property::new("Architecture", "x86_64"),
            Property::blank(),
            Property::heading("--- CPU Features ---"),
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
            Property::new(
                "Total Installed",
                &format!(
                    "{} MiB ({:.1} GiB)",
                    mem.total_mb,
                    mem.total_mb as f64 / 1024.0
                ),
            ),
            Property::new(
                "Available",
                &format!(
                    "{} MiB ({:.1} GiB)",
                    mem.available_mb,
                    mem.available_mb as f64 / 1024.0
                ),
            ),
            Property::new("Memory Type", &mem.mem_type),
            Property::new("Speed", &format!("{} MHz", mem.speed_mhz)),
            Property::new(
                "Slots Used / Total",
                &format!("{} / {}", mem.slots_used, mem.slots_total),
            ),
            Property::blank(),
            Property::heading("--- Per-Slot Details ---"),
        ];
        for slot in &mem.slots {
            props.push(Property::blank());
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
                props.push(Property::blank());
            }
            props.push(Property::new(&format!("--- Disk {} ---", idx), ""));
            props.push(Property::new("Model", &disk.model));
            props.push(Property::new(
                "Capacity",
                &guitk::bytes::iec(disk.capacity_bytes),
            ));
            props.push(Property::new("Interface", &disk.interface));
            props.push(Property::new("Serial", &disk.serial));
            props.push(Property::new("S.M.A.R.T. Status", &disk.smart_status));
            for part in &disk.partitions {
                props.push(Property::blank());
                props.push(Property::new("  Partition", &part.label));
                props.push(Property::new("  Filesystem", &part.filesystem));
                props.push(Property::new(
                    "  Capacity",
                    &guitk::bytes::iec(part.capacity_bytes),
                ));
                props.push(Property::new("  Used", &guitk::bytes::iec(part.used_bytes)));
                props.push(Property::new("  Free", &guitk::bytes::iec(part.free_bytes)));
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
            Property::new(
                "VRAM",
                &format!("{} MiB ({:.1} GiB)", d.vram_mb, d.vram_mb as f64 / 1024.0),
            ),
            Property::new("Resolution", &d.resolution),
            Property::new("Refresh Rate", &format!("{} Hz", d.refresh_rate_hz)),
            Property::new("Driver Version", &d.driver_version),
            Property::blank(),
            Property::heading("--- Display Outputs ---"),
        ];
        for (output, connected) in &d.outputs {
            let status = if *connected {
                "Connected"
            } else {
                "Disconnected"
            };
            props.push(Property::new(output, status));
        }
        props
    }

    fn props_sound(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, snd) in self.sound_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::blank());
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
                props.push(Property::blank());
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
            props.push(Property::new(
                "Speed",
                &format!("{} Mbps", adapter.speed_mbps),
            ));
            props.push(Property::new("Duplex", &adapter.duplex));
            props.push(Property::new(
                "Bytes Sent",
                &format_bytes(adapter.bytes_sent),
            ));
            props.push(Property::new(
                "Bytes Received",
                &format_bytes(adapter.bytes_received),
            ));
        }
        props
    }

    fn props_usb(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, dev) in self.usb_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::blank());
            }
            props.push(Property::new("Port", &dev.port));
            props.push(Property::new("Description", &dev.description));
            props.push(Property::new(
                "Vendor:Product",
                &format!("{:04X}:{:04X}", dev.vendor_id, dev.product_id),
            ));
            props.push(Property::new("Speed", &dev.speed));
        }
        props
    }

    fn props_pci(&self) -> Vec<Property> {
        let mut props = Vec::new();
        for (idx, dev) in self.pci_devices.iter().enumerate() {
            if idx > 0 {
                props.push(Property::blank());
            }
            props.push(Property::new(
                "BDF",
                &format!("{:02X}:{:02X}.{}", dev.bus, dev.device, dev.function),
            ));
            props.push(Property::new(
                "Vendor:Device",
                &format!("{:04X}:{:04X}", dev.vendor_id, dev.device_id),
            ));
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
                &format!(
                    "{} KiB / {:.1}%",
                    proc_entry.memory_kb, proc_entry.cpu_percent
                ),
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
            props.push(Property::new(
                &drv.name,
                &format!("{} [{}]", drv.path, drv.status),
            ));
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
            props.push(Property::new(
                &entry.name,
                &format!("{} ({})", entry.path, entry.source),
            ));
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

    // ========================================================================
    // Layout
    //
    // The sidebar rectangle and the property table's top edge each used to be
    // recomputed from the same four constants at every site that needed them —
    // the renderer, the click handler and the hover handler each carried their
    // own copy. That is the divergence class in `known-issues.md`
    // (`C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY`): three
    // copies that agree until one is edited. They are derived once here.
    // ========================================================================

    /// Top edge of both panes: below the title bar and toolbar.
    pub fn pane_top(&self) -> f32 {
        TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT
    }

    /// Bottom edge of both panes: above the status bar.
    pub fn pane_bottom(&self) -> f32 {
        self.window_height - STATUS_BAR_HEIGHT
    }

    /// Height of both panes.
    pub fn pane_height(&self) -> f32 {
        (self.pane_bottom() - self.pane_top()).max(0.0)
    }

    /// The window of tree rows the sidebar draws.
    fn tree_window(&self) -> scroll_window::Rows {
        scroll_window::visible(
            self.visible_tree_rows().len(),
            TREE_ROW_HEIGHT,
            self.pane_height(),
            self.tree_scroll,
        )
    }

    /// The largest [`Self::tree_scroll`] that still shows a full pane of rows.
    ///
    /// `usize::MAX` asks `scroll_window` for the last page: it clamps, and the
    /// start of the clamped window is by definition the furthest the list can
    /// usefully go.
    pub fn max_tree_scroll(&self) -> usize {
        scroll_window::visible(
            self.visible_tree_rows().len(),
            TREE_ROW_HEIGHT,
            self.pane_height(),
            usize::MAX,
        )
        .start
    }

    /// Top edge of the detail pane's first property row.
    ///
    /// Below the category heading, its separator, and the table's column
    /// header — the same stack of furniture `render_detail_pane` walks, stated
    /// once so the two cannot part company.
    pub fn property_rows_top(&self) -> f32 {
        self.pane_top()
            + DETAIL_HEADING_TOP
            + DETAIL_HEADING_HEIGHT
            + DETAIL_SEPARATOR_GAP
            + PROPERTY_HEADER_HEIGHT
    }

    /// Height available to property rows, below the table header.
    pub fn property_rows_height(&self) -> f32 {
        (self.pane_bottom() - self.property_rows_top()).max(0.0)
    }

    /// The window of property rows the detail pane draws.
    fn property_window(&self) -> scroll_window::Rows {
        scroll_window::visible(
            self.current_properties().len(),
            PROPERTY_ROW_HEIGHT,
            self.property_rows_height(),
            self.detail_scroll,
        )
    }

    /// The largest [`Self::detail_scroll`] that still shows a full pane of rows.
    pub fn max_detail_scroll(&self) -> usize {
        scroll_window::visible(
            self.current_properties().len(),
            PROPERTY_ROW_HEIGHT,
            self.property_rows_height(),
            usize::MAX,
        )
        .start
    }

    /// How many property rows fit in the pane — one PageUp/PageDown.
    ///
    /// A page of *whatever is on screen*, not a fixed 200 px. The old constant
    /// paged past three rows of a short window and left half a screen unread in
    /// a tall one.
    fn property_page(&self) -> usize {
        self.property_window().count.max(1)
    }

    /// Which visible tree row the sidebar drew at window y-coordinate `my`.
    ///
    /// Takes a *window* coordinate so that subtracting the pane top and adding
    /// the scroll offset happen in exactly one place. The old form added the
    /// scroll offset as **pixels before dividing**, so any list scrolled to a
    /// position that was not a whole multiple of `TREE_ROW_HEIGHT` selected the
    /// row above or below the one drawn under the pointer — and once the offset
    /// became a row index that arithmetic could not even be written.
    pub fn tree_hit_test(&self, my: f32) -> Option<usize> {
        let offset = my - self.pane_top();
        if !offset.is_finite() || offset < 0.0 || offset >= self.pane_height() {
            return None;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let slot = (offset / TREE_ROW_HEIGHT) as usize;
        let row = self.tree_scroll.checked_add(slot)?;
        // Below the last row is not the last row: returning `None` here is what
        // stops a click on the sidebar's empty tail selecting whatever happens
        // to sit at the bottom.
        if row < self.visible_tree_rows().len() {
            Some(row)
        } else {
            None
        }
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

    /// Scroll the sidebar so the selected row is on screen.
    ///
    /// Keyboard navigation used to move the selection without touching
    /// `tree_scroll` at all, so arrowing down past the last drawn row selected
    /// something the user could not see — and, because the wheel was the only
    /// thing that moved the sidebar, there was no way to find out what.
    /// Also clamps `tree_scroll` to the shortened list, which is why the clamp
    /// is outside the `if let`: collapsing a node removes rows whether or not
    /// the selection is one of the survivors, and an offset left pointing past
    /// the new end would draw an empty sidebar.
    fn scroll_selection_into_view(&mut self) {
        let rows = self.visible_tree_rows();
        if let Some(pos) = rows.iter().position(|c| *c == self.selected_category) {
            let window = self.tree_window();
            let last_slot = window.start.saturating_add(window.count);
            if pos < window.start {
                self.tree_scroll = pos;
            } else if pos >= last_slot {
                // Put it on the bottom slot rather than the top: scrolling down
                // by one row should move the list by one row, not jump a
                // screenful.
                self.tree_scroll = pos.saturating_sub(window.count.saturating_sub(1));
            }
        }
        self.tree_scroll = self.tree_scroll.min(self.max_tree_scroll());
    }

    /// Select the next visible tree row.
    pub fn select_next(&mut self) {
        let rows = self.visible_tree_rows();
        if let Some(pos) = rows.iter().position(|c| *c == self.selected_category)
            && let Some(&next) = pos.checked_add(1).and_then(|n| rows.get(n))
        {
            self.selected_category = next;
            self.detail_scroll = 0;
        }
        self.scroll_selection_into_view();
    }

    /// Select the previous visible tree row.
    pub fn select_prev(&mut self) {
        let rows = self.visible_tree_rows();
        if let Some(pos) = rows.iter().position(|c| *c == self.selected_category)
            && let Some(&prev) = pos.checked_sub(1).and_then(|n| rows.get(n))
        {
            self.selected_category = prev;
            self.detail_scroll = 0;
        }
        self.scroll_selection_into_view();
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
                    self.detail_scroll = 0;
                }
            }
        }
        self.scroll_selection_into_view();
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
            self.detail_scroll = 0;
        }
        self.scroll_selection_into_view();
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
                if prop.name.to_lowercase().contains(&q) || prop.value.to_lowercase().contains(&q) {
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
        out.push_str("=== Slate OS System Information Report ===\n\n");

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
            // Column 0 belongs to the report. A `Field` is data, so it is
            // always indented -- including when its value is empty, which is
            // an ordinary thing for an environment variable to be and no
            // longer means "this row is a heading".
            for prop in &props {
                match prop.kind {
                    PropertyKind::Blank => out.push('\n'),
                    PropertyKind::Heading => out.push_str(&format!("{}\n", prop.name)),
                    PropertyKind::Field => {
                        out.push_str(&format!("  {}: {}\n", prop.name, prop.value));
                    }
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
            // Scroll the detail view by a screenful of rows. Clamped at both
            // ends: paging past the last property used to keep climbing an
            // unbounded pixel offset while the table stood still, so the same
            // distance had to be paged back before anything moved.
            Key::PageDown => {
                let page = self.property_page();
                self.detail_scroll = self
                    .detail_scroll
                    .saturating_add(page)
                    .min(self.max_detail_scroll());
                EventResult::Consumed
            }
            Key::PageUp => {
                let page = self.property_page();
                self.detail_scroll = self.detail_scroll.saturating_sub(page);
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
                    if let Some(parent) = cat.parent()
                        && !self.expanded.contains(&parent)
                    {
                        self.expanded.push(parent);
                    }
                    self.detail_scroll = 0;
                    // Expanding a parent above the view pushes every row below
                    // it down, so the hit row can land off-screen even when the
                    // sidebar had not been scrolled at all.
                    self.scroll_selection_into_view();
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
                self.search_text.extend(key.typed());
                EventResult::Consumed
            }
        }
    }

    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) if mouse.x < SIDEBAR_WIDTH => {
                if let Some(row) = self.tree_hit_test(mouse.y) {
                    let rows = self.visible_tree_rows();
                    if let Some(&cat) = rows.get(row) {
                        if cat.is_parent() {
                            self.toggle_expand(cat);
                        }
                        self.selected_category = cat;
                        self.detail_scroll = 0;
                        // Folding a parent removes rows from under the view,
                        // which can leave the offset past the end of the
                        // shortened list. The same call the keyboard uses does
                        // that clamp, so the bound is derived in one place.
                        self.scroll_selection_into_view();
                    }
                }
                return EventResult::Consumed;
            }
            // `dy` counts wheel *notches*, not pixels — see
            // `MouseEventKind::Scroll`. Both branches used to multiply it by
            // 20.0 on the assumption it was a distance, which moved 20 px per
            // detent (most of a row, never a whole one) and discarded a
            // trackpad's fractions entirely. The accumulators bank those
            // fractions so a slow trackpad eventually steps a row.
            MouseEventKind::Scroll { dy, .. } => {
                if mouse.x < SIDEBAR_WIDTH {
                    let rows = self.tree_wheel.rows(*dy);
                    self.tree_scroll =
                        scroll_window::shift(self.tree_scroll, rows).min(self.max_tree_scroll());
                } else {
                    let rows = self.detail_wheel.rows(*dy);
                    self.detail_scroll = scroll_window::shift(self.detail_scroll, rows)
                        .min(self.max_detail_scroll());
                }
                return EventResult::Consumed;
            }
            MouseEventKind::Move => {
                self.hovered_tree_row = if mouse.x < SIDEBAR_WIDTH {
                    // The same hit-test the click uses. Two derivations of
                    // "which row is under the pointer" is how an app comes to
                    // highlight one row and select another.
                    self.tree_hit_test(mouse.y)
                } else {
                    None
                };
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
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width,
            TITLE_BAR_HEIGHT,
            COLOR_TITLE_BG,
        );

        // Title text.
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "System Information".to_string(),
            color: COLOR_TEXT,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
        let top = self.pane_top();
        let height = self.pane_height();

        // Sidebar background.
        tree.fill_rect(0.0, top, SIDEBAR_WIDTH, height, COLOR_SIDEBAR_BG);

        // Clip to sidebar area.
        tree.clip(0.0, top, SIDEBAR_WIDTH, height);

        // Only the rows in the window are drawn, positioned by their *slot* on
        // screen. Drawing the whole list under a translate meant a thousand
        // commands the clip then threw away, and made the drawn position and
        // the hit-tested position two different calculations.
        let rows = self.visible_tree_rows();
        let window = self.tree_window();
        for (slot, (idx, &cat)) in rows
            .iter()
            .enumerate()
            .skip(window.start)
            .take(window.count)
            .enumerate()
        {
            let row_y = top + slot_offset(slot, TREE_ROW_HEIGHT);
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
                    overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
            });
        }

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
        let top = self.pane_top();
        let left = SIDEBAR_WIDTH;
        let width = self.window_width - SIDEBAR_WIDTH;
        let height = self.pane_height();

        // Background.
        tree.fill_rect(left, top, width, height, COLOR_SURFACE0);

        // Clip to detail area.
        tree.clip(left, top, width, height);

        // Category heading.
        let heading_y = top + DETAIL_HEADING_TOP;
        tree.push(RenderCommand::Text {
            x: left + 16.0,
            y: heading_y,
            text: self.selected_category.label().to_string(),
            color: COLOR_LAVENDER,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Separator below heading.
        let sep_y = heading_y + DETAIL_HEADING_HEIGHT;
        tree.push(RenderCommand::Line {
            x1: left + 16.0,
            y1: sep_y,
            x2: left + width - 16.0,
            y2: sep_y,
            color: COLOR_SEPARATOR,
            width: 1.0,
        });

        // Property table.
        let table_top = sep_y + DETAIL_SEPARATOR_GAP;
        debug_assert!(
            (table_top + PROPERTY_HEADER_HEIGHT - self.property_rows_top()).abs() < 0.01,
            "the furniture this renderer stacks must be the same stack \
             `property_rows_top` adds up, or the scroll bound belongs to a \
             table that is not the one on screen"
        );
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
            overflow: TextOverflow::Ellipsis,
        });
        tree.push(RenderCommand::Text {
            x: left + name_col_width + 8.0,
            y: table_top + 6.0,
            text: "Value".to_string(),
            color: COLOR_SUBTEXT,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - name_col_width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Property rows. Only the window is drawn, positioned by its slot on
        // screen — `scroll_window` decides which rows those are, so the
        // renderer no longer needs skip/break tests of its own that could
        // disagree with the bound the wheel is clamped to.
        let props = self.current_properties();
        let content_top = self.property_rows_top();
        let window = self.property_window();

        for (slot, (idx, prop)) in props
            .iter()
            .enumerate()
            .skip(window.start)
            .take(window.count)
            .enumerate()
        {
            let row_y = content_top + slot_offset(slot, PROPERTY_ROW_HEIGHT);

            // Alternating row color. Keyed on the row's index in the whole
            // table, not its slot on screen, so the stripes do not invert as
            // the table scrolls.
            let row_bg = if idx % 2 == 0 {
                COLOR_ROW_EVEN
            } else {
                COLOR_ROW_ODD
            };
            tree.fill_rect(left, row_y, width, PROPERTY_ROW_HEIGHT, row_bg);

            // Section headers get different styling. Asked of the row's kind,
            // not of its text: a name beginning `---` is a heading only if we
            // wrote it, and an environment variable may be called anything.
            let is_section = prop.kind == PropertyKind::Heading;
            let name_color = if is_section {
                COLOR_PEACH
            } else {
                COLOR_SUBTEXT
            };
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
                    overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        tree.unclip();
    }

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        tree.fill_rect(
            0.0,
            y,
            self.window_width,
            STATUS_BAR_HEIGHT,
            COLOR_STATUS_BG,
        );

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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format a byte count in human-readable form.
fn format_bytes(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is what a test is for: an `expect` that fires here
    // *is* the failure report, and rewriting it as a `match` would only bury
    // the message. CLAUDE.md scopes the defensive panic lints to non-test code
    // for exactly this reason.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;

    /// Every line of the export that sits at column 0 and is not blank.
    ///
    /// This is the set the report claims sole authorship of. The test below
    /// asserts membership of it, rather than asserting on the *text* of any
    /// particular line: a folded value may legitimately still contain the
    /// characters `--- Display Outputs ---` in the middle of its own row, and
    /// a `contains` assertion would fail against that correct output.
    fn column_zero_lines(report: &str) -> Vec<&str> {
        report
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with(' '))
            .collect()
    }

    fn app_with_env(vars: &[(&str, &str)]) -> SysInfoState {
        let mut app = SysInfoState::new();
        app.env_vars = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        app
    }

    #[test]
    fn an_empty_environment_variable_is_not_a_section_heading() {
        // The original bug, exactly. `FOO=` is legal and ordinary, and an
        // empty value used to mean "print this name at column 0" -- so a
        // variable named `--- Display Outputs ---` printed itself as one of
        // the report's own headings, with no control characters involved.
        let clean = SysInfoState::new().export_text();
        let hostile = app_with_env(&[
            ("--- Display Outputs ---", ""),
            ("--- CPU Features ---", ""),
            ("PATH", "/bin"),
        ])
        .export_text();

        // Compared as a multiset, not a set. The forgeries above deliberately
        // duplicate headings the clean report already contains, because that
        // is the strongest form of the attack -- a forged heading that is
        // *identical* to a real one cannot be told apart by its text. An
        // assertion that merely asked "is this line one the clean report also
        // produced?" would answer yes and pass, which is how the first draft
        // of this test let the bug through.
        let mut got = column_zero_lines(&hostile);
        let mut want = column_zero_lines(&clean);
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(
            got, want,
            "environment variables changed which lines occupy column 0",
        );
    }

    #[test]
    fn an_empty_environment_variable_is_still_reported() {
        // Indenting it must not amount to hiding it.
        let report = app_with_env(&[("EMPTY_VAR", "")]).export_text();
        assert!(
            report.contains("  EMPTY_VAR: \n"),
            "an empty variable vanished from the report: {report:?}",
        );
    }

    #[test]
    fn a_newline_in_an_environment_variable_cannot_add_a_line() {
        let hostile = app_with_env(&[
            ("A", "one\n--- CPU Features ---"),
            ("B\nC", "two"),
            ("D", "three\r\nfour"),
        ]);
        let report = hostile.export_text();
        let clean = app_with_env(&[("A", "one"), ("B", "two"), ("D", "three")]).export_text();
        assert_eq!(
            report.lines().count(),
            clean.lines().count(),
            "a hostile environment variable added lines to the report",
        );
    }

    #[test]
    fn a_property_derived_from_data_is_never_a_heading() {
        for (name, value) in [("--- x ---", ""), ("", ""), ("plain", "v")] {
            assert_eq!(
                Property::new(name, value).kind,
                PropertyKind::Field,
                "Property::new({name:?}, {value:?}) claimed to be structure",
            );
        }
        assert_eq!(Property::heading("--- x ---").kind, PropertyKind::Heading);
        assert_eq!(Property::blank().kind, PropertyKind::Blank);
    }

    #[test]
    fn a_data_property_is_folded_on_construction() {
        let p = Property::new("a\nb", "c\r\nd");
        assert_eq!(p.name, "a b");
        assert_eq!(p.value, "c d");
    }

    #[test]
    fn the_reports_own_headings_survive() {
        // The fix must not cost the report the structure it legitimately has.
        let report = SysInfoState::new().export_text();
        let at_zero = column_zero_lines(&report);
        assert!(
            at_zero.contains(&"--- CPU Features ---"),
            "the report lost its own sub-heading: {at_zero:?}",
        );
        assert!(
            at_zero.contains(&"=== Slate OS System Information Report ==="),
            "the report lost its title: {at_zero:?}",
        );
    }

    #[test]
    fn every_category_section_is_present_once() {
        let report = SysInfoState::new().export_text();
        for heading in ["--- CPU ---", "--- Memory ---", "--- PCI Devices ---"] {
            assert_eq!(
                report.lines().filter(|l| *l == heading).count(),
                1,
                "expected exactly one {heading}",
            );
        }
    }

    // ========================================================================
    // Scrolling
    //
    // Both panes are lists of uniform rows, so both offsets are row indices
    // driven by `scroll_window`, and the wheel reaches them through a
    // `wheel::Accumulator`.
    //
    // These are written in *rows actually moved* — the unit of the bug — and
    // not in "the offset changed". The offset changed under the old
    // `dy * 20.0` code too: a notch moved 20 px of a 24 px row, so the number
    // grew every event and the list still never landed on a row boundary. A
    // test that asserted `scroll > 0` would have passed against it.
    // ========================================================================

    /// A state whose sidebar *and* property table both overflow their panes.
    ///
    /// The two `assert!`s are the fixture checking that it can fail. A scroll
    /// test run against a list that already fits on screen passes no matter
    /// what the handler does, because zero is the correct answer either way.
    fn overflowing_app() -> SysInfoState {
        let mut app = SysInfoState::new();
        for &root in TREE_ROOT_ITEMS {
            if root.is_parent() && !app.expanded.contains(&root) {
                app.expanded.push(root);
            }
        }
        app.window_height = 300.0;
        assert!(
            app.max_tree_scroll() > 0,
            "fixture's sidebar fits on screen: {} rows in {} px",
            app.visible_tree_rows().len(),
            app.pane_height(),
        );
        assert!(
            app.max_detail_scroll() > 0,
            "fixture's property table fits on screen: {} rows in {} px",
            app.current_properties().len(),
            app.property_rows_height(),
        );
        app
    }

    /// `ROWS_PER_NOTCH` as a row count.
    ///
    /// Taken from the constant so that retuning the platform default retunes
    /// the tests — but deliberately *not* from `Accumulator::rows`, which
    /// would make every expectation below agree with the converter by
    /// construction and so pass even if this app never called it.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn rows_per_notch() -> usize {
        wheel::ROWS_PER_NOTCH as usize
    }

    /// Horizontal probe inside the sidebar.
    const SIDEBAR_X: f32 = SIDEBAR_WIDTH / 2.0;
    /// Horizontal probe inside the detail pane.
    const DETAIL_X: f32 = SIDEBAR_WIDTH + 10.0;

    fn scroll_at(x: f32, dy: f32) -> Event {
        Event::Mouse(guitk::event::MouseEvent {
            x,
            y: 100.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })
    }

    fn move_to(x: f32, y: f32) -> Event {
        Event::Mouse(guitk::event::MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        })
    }

    fn press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// The label text and `y` of every tree row the sidebar actually draws.
    ///
    /// Rows are found by matching the drawn string against the labels of the
    /// visible categories, not by filtering on a coordinate range. A
    /// coordinate filter derived from the layout would be asking the code
    /// under test to mark its own homework: move the pane's top edge and both
    /// the renderer and the filter move with it, and the test keeps passing.
    fn drawn_sidebar_rows(app: &SysInfoState) -> Vec<(String, f32)> {
        let labels: Vec<&str> = app.visible_tree_rows().iter().map(|c| c.label()).collect();
        let mut t = RenderTree::new();
        app.render_sidebar(&mut t);
        t.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, y, .. } if labels.contains(&text.as_str()) => {
                    Some((text.clone(), *y))
                }
                _ => None,
            })
            .collect()
    }

    /// The top edge of every property row the detail pane actually draws.
    ///
    /// Identified by the alternating row stripe: a fill one row high in one of
    /// the two stripe colours. Structural rather than positional, for the
    /// reason given on [`drawn_sidebar_rows`].
    ///
    /// The height test is not belt-and-braces. `COLOR_ROW_EVEN` and
    /// `COLOR_SURFACE0` are the same RGB, so a colour-only filter also matches
    /// the pane's own background and reports one row more than the table
    /// drew — which is exactly how the first draft of this helper made two
    /// correct page-step assertions fail by one. A helper filtered on the
    /// wrong property is as wrong as the code it is checking.
    fn drawn_property_rows(app: &SysInfoState) -> Vec<(f32, Color)> {
        let mut t = RenderTree::new();
        app.render_detail_pane(&mut t);
        t.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y, height, color, ..
                } if (*color == COLOR_ROW_EVEN || *color == COLOR_ROW_ODD)
                    && (*height - PROPERTY_ROW_HEIGHT).abs() < 0.01 =>
                {
                    Some((*y, *color))
                }
                _ => None,
            })
            .collect()
    }

    fn drawn_property_row_tops(app: &SysInfoState) -> Vec<f32> {
        drawn_property_rows(app)
            .into_iter()
            .map(|(y, _)| y)
            .collect()
    }

    #[test]
    fn one_wheel_notch_scrolls_the_sidebar_by_exactly_three_rows() {
        let mut app = overflowing_app();
        let before = drawn_sidebar_rows(&app);
        app.handle_event(&scroll_at(SIDEBAR_X, -1.0));
        let after = drawn_sidebar_rows(&app);

        let step = rows_per_notch();
        assert_eq!(app.tree_scroll, step, "one notch is not one notch of rows");
        assert_eq!(
            after.first().map(|(t, _)| t.as_str()),
            before.get(step).map(|(t, _)| t.as_str()),
            "the offset moved but the drawn list did not follow it",
        );
        assert_eq!(
            after.first().map(|(_, y)| *y),
            before.first().map(|(_, y)| *y),
            "row 0's slot moved; the list should scroll under a fixed grid",
        );
    }

    #[test]
    fn one_wheel_notch_scrolls_the_property_table_by_exactly_three_rows() {
        let mut app = overflowing_app();
        let before = drawn_property_row_tops(&app);
        app.handle_event(&scroll_at(DETAIL_X, -1.0));
        let after = drawn_property_row_tops(&app);

        assert_eq!(app.detail_scroll, rows_per_notch());
        assert_eq!(
            before, after,
            "the drawn slots moved; only which rows occupy them should change",
        );
    }

    #[test]
    fn a_trackpads_fractions_add_up_instead_of_vanishing() {
        // Five fifths of a notch is one notch, and must move the same three
        // rows a single detent does. Rounding each event on its own would
        // return zero five times and the pane would be dead to a trackpad —
        // which is the same bug as `dy * 20.0`, just silent instead of wrong.
        let mut app = overflowing_app();
        for _ in 0..5 {
            app.handle_event(&scroll_at(SIDEBAR_X, -0.2));
        }
        assert_eq!(app.tree_scroll, rows_per_notch());
    }

    #[test]
    fn the_two_panes_bank_their_wheel_fractions_separately() {
        // A fifth of a notch over each pane is 0.6 of a row each: neither
        // moves. A single shared accumulator would have added them into 1.2
        // rows and stepped one of the two panes by a row it never received.
        let mut app = overflowing_app();
        app.handle_event(&scroll_at(SIDEBAR_X, -0.2));
        app.handle_event(&scroll_at(DETAIL_X, -0.2));
        assert_eq!(app.tree_scroll, 0, "the sidebar spent the table's fraction");
        assert_eq!(
            app.detail_scroll, 0,
            "the table spent the sidebar's fraction"
        );
    }

    #[test]
    fn scrolling_one_pane_leaves_the_other_alone() {
        let mut app = overflowing_app();
        app.handle_event(&scroll_at(SIDEBAR_X, -3.0));
        assert!(app.tree_scroll > 0);
        assert_eq!(app.detail_scroll, 0);

        let tree_before = app.tree_scroll;
        app.handle_event(&scroll_at(DETAIL_X, -3.0));
        assert!(app.detail_scroll > 0);
        assert_eq!(app.tree_scroll, tree_before);
    }

    #[test]
    fn the_wheel_stops_at_the_last_row_of_each_pane() {
        // The old `f32` offsets had no far-end bound at all, so scrolling past
        // the end kept climbing while the list stood still — and the same
        // distance had to be scrolled back before anything moved again.
        let mut app = overflowing_app();
        for _ in 0..200 {
            app.handle_event(&scroll_at(SIDEBAR_X, -1.0));
            app.handle_event(&scroll_at(DETAIL_X, -1.0));
        }
        assert_eq!(app.tree_scroll, app.max_tree_scroll());
        assert_eq!(app.detail_scroll, app.max_detail_scroll());

        let rows = app.visible_tree_rows();
        let drawn = drawn_sidebar_rows(&app);
        assert!(!drawn.is_empty(), "the sidebar drew nothing at the far end");
        assert_eq!(
            drawn.last().map(|(t, _)| t.as_str()),
            rows.last().map(|c| c.label()),
            "the last row of the list never reached the screen",
        );

        // And back: the very next notch upwards must move the list, not spend
        // a debt run up by the events that had nowhere to go.
        let first = drawn.first().map(|(t, _)| t.clone());
        app.handle_event(&scroll_at(SIDEBAR_X, 1.0));
        assert_ne!(
            drawn_sidebar_rows(&app).first().map(|(t, _)| t.clone()),
            first,
            "scrolling back up did nothing",
        );
    }

    #[test]
    fn every_drawn_sidebar_row_hit_tests_to_itself() {
        let mut app = overflowing_app();
        app.handle_event(&scroll_at(SIDEBAR_X, -1.0));
        let rows = app.visible_tree_rows();
        let drawn = drawn_sidebar_rows(&app);
        assert!(drawn.len() > 1, "nothing drawn to hit-test");

        for (label, y) in &drawn {
            // The label's own `y` is inside the row it belongs to, so it is a
            // probe the renderer supplies rather than one the test derives
            // from the same constants the hit test uses.
            let idx = app
                .tree_hit_test(y + 1.0)
                .expect("a row that is drawn must be clickable");
            assert_eq!(
                rows.get(idx).map(|c| c.label()),
                Some(label.as_str()),
                "clicking the row labelled {label:?} selects a different one",
            );
        }
    }

    #[test]
    fn nothing_outside_the_drawn_rows_hit_tests_to_a_row() {
        let app = overflowing_app();
        assert_eq!(app.tree_hit_test(app.pane_top() - 1.0), None, "toolbar");
        assert_eq!(
            app.tree_hit_test(app.pane_bottom() + 1.0),
            None,
            "status bar"
        );
        assert_eq!(app.tree_hit_test(f32::NAN), None, "NaN");

        // Empty space below a list too short to fill the pane is not a row
        // either — the old handler divided the offset and trusted the quotient.
        let short = SysInfoState::new();
        assert_eq!(short.max_tree_scroll(), 0, "fixture is not a short list");
        let below =
            short.pane_top() + short.visible_tree_rows().len() as f32 * TREE_ROW_HEIGHT + 1.0;
        assert!(below < short.pane_bottom(), "fixture has no empty space");
        assert_eq!(
            short.tree_hit_test(below),
            None,
            "empty space below the list"
        );
    }

    #[test]
    fn the_hover_row_is_cleared_off_the_sidebar() {
        let mut app = overflowing_app();
        app.handle_event(&move_to(SIDEBAR_X, app.pane_top() + 1.0));
        assert!(app.hovered_tree_row.is_some(), "no row hovered over a row");

        app.handle_event(&move_to(DETAIL_X, app.pane_top() + 1.0));
        assert_eq!(
            app.hovered_tree_row, None,
            "hover stuck over the other pane"
        );

        app.handle_event(&move_to(SIDEBAR_X, app.pane_top() + 1.0));
        app.handle_event(&move_to(SIDEBAR_X, app.pane_bottom() + 1.0));
        assert_eq!(app.hovered_tree_row, None, "hover stuck below the pane");
    }

    #[test]
    fn a_page_down_moves_the_table_by_the_screenful_that_was_showing() {
        let mut app = overflowing_app();
        let showing = drawn_property_row_tops(&app).len();
        assert!(showing > 1, "fixture shows no page to move by");
        app.handle_event(&press(Key::PageDown));
        assert_eq!(
            app.detail_scroll, showing,
            "a page is the screenful on display, not a fixed row count",
        );
    }

    #[test]
    fn paging_past_either_end_of_the_table_is_bounded() {
        let mut app = overflowing_app();
        for _ in 0..50 {
            app.handle_event(&press(Key::PageDown));
        }
        assert_eq!(app.detail_scroll, app.max_detail_scroll());

        // One page back must move by a page, not unwind an overshoot.
        let showing = drawn_property_row_tops(&app).len();
        let at_end = app.detail_scroll;
        app.handle_event(&press(Key::PageUp));
        assert_eq!(app.detail_scroll, at_end.saturating_sub(showing));

        for _ in 0..50 {
            app.handle_event(&press(Key::PageUp));
        }
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn arrowing_through_the_tree_keeps_the_selection_on_screen() {
        // Keyboard navigation used to move the selection without touching
        // `tree_scroll`, so arrowing past the last drawn row selected a
        // category the user could not see — and the wheel was the only thing
        // that moved the sidebar, so there was no way to find out which.
        let mut app = overflowing_app();
        let total = app.visible_tree_rows().len();

        for _ in 0..total + 5 {
            let before = app.tree_scroll;
            app.handle_event(&press(Key::Down));
            assert!(
                app.tree_scroll <= before + 1,
                "one row of selection jumped {} rows of list",
                app.tree_scroll - before,
            );
            let drawn = drawn_sidebar_rows(&app);
            assert!(
                drawn
                    .iter()
                    .any(|(t, _)| t == app.selected_category.label()),
                "selection {:?} is off screen; drawn: {drawn:?}",
                app.selected_category.label(),
            );
        }
        assert_eq!(
            app.tree_scroll,
            app.max_tree_scroll(),
            "never reached the end"
        );

        for _ in 0..total + 5 {
            app.handle_event(&press(Key::Up));
            let drawn = drawn_sidebar_rows(&app);
            assert!(
                drawn
                    .iter()
                    .any(|(t, _)| t == app.selected_category.label()),
                "selection {:?} is off screen on the way back up",
                app.selected_category.label(),
            );
        }
        assert_eq!(app.tree_scroll, 0, "never came back to the top");
    }

    #[test]
    fn collapsing_a_node_does_not_leave_the_sidebar_scrolled_past_its_end() {
        let mut app = overflowing_app();
        for _ in 0..200 {
            app.handle_event(&scroll_at(SIDEBAR_X, -1.0));
        }
        assert!(app.tree_scroll > 0, "fixture never scrolled");

        for &root in TREE_ROOT_ITEMS {
            if root.is_parent() {
                app.selected_category = root;
                app.handle_event(&press(Key::Left));
            }
        }
        assert!(app.tree_scroll <= app.max_tree_scroll());
        assert!(
            !drawn_sidebar_rows(&app).is_empty(),
            "the sidebar went blank after collapsing every node",
        );
    }

    #[test]
    fn the_row_stripes_do_not_invert_when_the_table_scrolls() {
        // The stripe has to be keyed on the row's index in the whole table,
        // not on its slot on screen. Keyed on the slot, the top row is always
        // the "even" colour and the whole table flickers between two
        // colourings as it scrolls by an odd number of rows.
        let mut app = overflowing_app();
        let top_colour = |a: &SysInfoState| drawn_property_rows(a).first().map(|(_, c)| *c);

        app.detail_scroll = 0;
        let even = top_colour(&app).expect("no rows drawn at the top");
        app.detail_scroll = 1;
        let odd = top_colour(&app).expect("no rows drawn one row down");
        assert_ne!(
            even, odd,
            "row 0 and row 1 got the same stripe in the top slot",
        );
        app.detail_scroll = 2;
        assert_eq!(top_colour(&app), Some(even), "the stripe lost its period");
    }

    #[test]
    fn selecting_a_category_returns_the_property_table_to_its_top() {
        let mut app = overflowing_app();
        app.handle_event(&scroll_at(DETAIL_X, -3.0));
        assert!(app.detail_scroll > 0);
        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.detail_scroll, 0,
            "a different category kept the last one's scroll position",
        );
    }
}
