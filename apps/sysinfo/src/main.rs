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
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::fold;
use guitk::frame::Rect;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::{scroll_window, wheel};
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

// The colours live in the user's palette, not here.
//
// 26 constants used to sit here -- the Catppuccin Mocha table, copied
// verbatim -- so a light desktop got a dark system-information window.
// design-decisions 822 added `App::theme_changed`; this was one of the 55
// crates never converted against it.
//
// Mapped to roles **by value, not by name**: the constant this module called
// SURFACE0 held rgb(30, 30, 46), which is the palette's `base`, and the ones
// it called SURFACE1 and SURFACE2 were likewise one rung off. Trusting the
// names would have darkened every background by a step.
//
// And nearest-value was not the last word either. Four constants had to be
// moved off their closest role because the closest role was *already taken*
// by a neighbour they were drawn to differ from: ROW_ODD would have become
// ROW_EVEN (no striping), TREE_HOVER would have become TREE_SELECTED
// (pointing at a row would look like choosing it), and OVERLAY would have
// become SUBTEXT (the muted ink stops being muted). Preserving a distinction
// the module draws on purpose outranks matching the nearest colour.

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
    /// Every category, in tree order.
    ///
    /// Distinct from [`TREE_ROOT_ITEMS`], which is the four *roots*; this is
    /// all twenty-one panes the right-hand side can show. Added so the theme
    /// sweep can visit each one -- it used to render whichever category the
    /// window opens on and no other, which is one pane in twenty-one.
    ///
    /// A variant missing from here is a pane the sweep never renders: a
    /// coverage gap rather than a wrong answer, which is the failure mode to
    /// prefer but is still worth knowing about.
    pub const ALL: [Self; 21] = [
        Self::SystemSummary,
        Self::HardwareResources,
        Self::HwIrqs,
        Self::HwIoPorts,
        Self::HwMemoryMap,
        Self::HwDma,
        Self::Components,
        Self::CompCpu,
        Self::CompMemory,
        Self::CompStorage,
        Self::CompDisplay,
        Self::CompSound,
        Self::CompNetwork,
        Self::CompUsb,
        Self::CompPci,
        Self::SoftwareEnvironment,
        Self::SwServices,
        Self::SwProcesses,
        Self::SwDrivers,
        Self::SwEnvVars,
        Self::SwStartupPrograms,
    ];

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
    /// The marketing name, if anything publishes one. Nothing does.
    ///
    /// The kernel serves CPUID leaf 1 -- family, model, stepping -- and not
    /// leaves 0x8000_0002..4, which are where a brand string lives. An empty
    /// `String` would draw as a processor with no name rather than as a
    /// question nobody answered, and the invented value it replaced was
    /// "Intel Core i7-13700K".
    pub brand: Option<String>,
    /// The vendor string, on the same terms as [`CpuInfo::brand`]: CPUID leaf
    /// 0, also not served.
    pub vendor: Option<String>,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub physical_cores: u32,
    pub logical_processors: u32,
    /// Base clock, if a frequency source exists. None does.
    ///
    /// Lane A's `sysfs.rs` says why there is no `cpufreq/`, and the reasoning
    /// covers this field: *"a file reading 0 cannot be told from a real 0 MHz.
    /// Absent is the honest answer until a CPUID leaf-16h reader exists."*
    pub base_clock_mhz: Option<u32>,
    /// Turbo clock, on the same terms as [`CpuInfo::base_clock_mhz`].
    pub max_turbo_mhz: Option<u32>,
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
    /// Left empty when read from `/proc/interrupts`: the kernel publishes a
    /// label and a flag, and nothing that is a *type*. It used to default to
    /// `"Edge"`, which is a trigger mode nobody reported.
    pub irq_type: String,
    /// Whether the IOAPIC showed the line asserted at the moment of the read.
    ///
    /// **A sample, not a total, and never to be drawn as activity.** Reading
    /// `/proc/interrupts` twice can show `false` both times while thousands of
    /// interrupts were serviced in between, so a count synthesised from this
    /// would agree with itself forever *and* agree with the real one. That is
    /// worse than a wrong constant, which at least fails on a second look:
    /// **the observation that would falsify it is the observation that
    /// confirms it.** Lane B declined to synthesise the count in `procinfo`
    /// for this reason; the same answer applies one layer up. A rate needs the
    /// counters to exist first.
    pub asserted: bool,
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
    /// The command line the item runs. `/proc/autostart`'s COMMAND column.
    pub path: String,
    /// **When** it runs -- boot, login, session -- not where it came from.
    ///
    /// This field was `source`, and `source` in a startup manager means the
    /// place the entry was registered: a folder, a registry key, a unit file.
    /// `/proc/autostart` publishes no such thing. It publishes a PHASE, and
    /// putting a phase in a column meaning origin is the same defect as
    /// putting a bus *type* in a column meaning bus *number* -- which is why
    /// `/proc/devicemgr` was left unwired. Renamed rather than repurposed.
    pub phase: String,
    /// Whether the item is set to run at all.
    ///
    /// A disabled entry listed like an enabled one is a claim that it runs.
    /// The file says which, so the window can too.
    pub enabled: bool,
}

// ============================================================================
// Application state
// ============================================================================

/// Main application state for the System Information Explorer.
/// Said when the user asks for a clipboard copy.
///
/// The old text was "Value copied to clipboard", printed by a handler whose
/// own comment read `(simulated)`. **A note to the next programmer was
/// standing in for a sentence addressed to the user.** Nothing in this
/// process can reach a clipboard -- `gui/clipboard` is a service this app does
/// not talk to -- so it names the thing that does work rather than denying
/// flatly.
const CLIPBOARD_UNAVAILABLE: &str =
    "Nothing here can reach the clipboard yet -- press Ctrl+E to write a file instead";

/// The toolbar's clickable geometry. See `SysInfoState::toolbar_layout`.
struct ToolbarLayout {
    search: Rect,
    export: Rect,
    copy: Rect,
}

pub struct SysInfoState {
    /// The save picker, shared with sixteen other applications.
    pub picker: FilePicker,
    /// The user's colours, handed over by the framework (§822).
    ///
    /// Defaulted rather than `Option`, so the first frame has *a* palette on
    /// a machine with no settings file.
    pub palette: Palette,

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
    /// What the hardware query returned for the processor, or `None` if it
    /// could not be read.
    ///
    /// An `Option` rather than a zero-filled `CpuInfo`, which would draw as a
    /// processor with no cores and a blank name -- a description of a machine
    /// rather than an admission that none was obtained.
    pub cpu_info: Option<CpuInfo>,
    /// As [`SysInfoApp::cpu_info`], for memory.
    pub memory_info: Option<MemoryInfo>,
    /// How long the machine has been up, re-read on every tick.
    ///
    /// `None` when `/proc/uptime` cannot be read, which is what the Summary
    /// then says. It used to be the string "4h 23m 17s" -- a number that was
    /// the same on every machine and at every moment, including the moment
    /// after you watched it for a minute.
    pub uptime: Option<Duration>,
    pub disks: Vec<DiskInfo>,
    pub network_adapters: Vec<NetworkAdapterInfo>,
    /// As [`SysInfoApp::cpu_info`], for the display.
    pub display_info: Option<DisplayInfo>,
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
    /// Build the window, asking the system what hardware it has.
    ///
    /// `SyscallProvider` directly, not `FallbackProvider`. The fallback tries
    /// the syscall path and drops to `StubProvider` on error, so on any host
    /// without `/sys/hardware` -- which today is every host -- it would show
    /// the same invented machine this replaced, through a longer call stack.
    /// It would also pass any test that only checked the app was using
    /// `hwquery`.
    ///
    /// **The paragraph that stood here was a promise, and it was wrong by
    /// 2026-09-15.** It said every query was expected to fail because nothing
    /// produced `/sys/hardware/*`, that this was the point rather than a
    /// defect, and that the window would start reporting real values "on the
    /// day a producer appears, with no change to this file".
    ///
    /// No producer of `/sys/hardware` ever appeared and none is coming. The
    /// kernel serves `/sys/devices/...`, `/sys/fs/` and `/sys/params/`, and
    /// lane A has recorded that `irqs` and `display` in particular will *never*
    /// be served there, because `/proc/interrupts` and `/proc/monitors` already
    /// publish them and a second kernel answer to one question is what §850
    /// exists to prevent. Each category moved by hand instead, so "with no
    /// change to this file" was the least accurate part.
    ///
    /// Where each category reads from now:
    ///
    /// | category | source |
    /// |---|---|
    /// | CPU, memory | `/sys/devices/system/{cpu,memory}` (§850) |
    /// | storage | `/sys/devices/block/<name>/` |
    /// | network, processes | `/proc/net/dev`, `/proc/<pid>/stat` |
    /// | IRQs, display | `/proc/{interrupts,monitors}` — **published, not yet read here** |
    /// | PCI, USB, sound, I/O ports, DMA, memory map, drivers, services, startup | nothing publishes these |
    ///
    /// The last row is the honest "cannot read", and is expected to stay that
    /// way. The row above it is the outstanding work, and it needs parsers in
    /// `procinfo` rather than here -- see
    /// `known-issues.md` →
    /// `TD-C-APPS-SYSINFO-WAITS-ON-A-FILESYSTEM-TREE-THAT-DOES-NOT-EXIST`.
    pub fn new() -> Self {
        use hwquery::HardwareProvider;
        let provider = hwquery::SyscallProvider::new();
        Self {
            picker: FilePicker::default(),
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
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
            cpu_info: provider.query_cpu().ok(),
            memory_info: provider.query_memory().ok(),
            uptime: provider.query_uptime().ok(),
            disks: provider.query_storage().unwrap_or_default(),
            network_adapters: provider.query_network().unwrap_or_default(),
            display_info: provider.query_display().ok(),
            pci_devices: provider.query_pci().unwrap_or_default(),
            services: provider.query_services().unwrap_or_default(),
            processes: provider.query_processes().unwrap_or_default(),
            drivers: provider.query_drivers().unwrap_or_default(),
            env_vars: provider.query_env_vars().unwrap_or_default(),
            irqs: provider.query_irqs().unwrap_or_default(),
            io_ports: provider.query_io_ports().unwrap_or_default(),
            memory_map: provider.query_memory_map().unwrap_or_default(),
            dma_channels: provider.query_dma().unwrap_or_default(),
            usb_devices: provider.query_usb().unwrap_or_default(),
            sound_devices: provider.query_sound().unwrap_or_default(),
            startup_programs: provider.query_startup().unwrap_or_default(),
        }
    }

    // ========================================================================
    // Data population (stubbed with representative data)
    // ========================================================================

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

    /// The single row shown for a category the system could not be asked about.
    ///
    /// Says what was not read and where it would have come from, rather than
    /// leaving the pane blank. A blank pane reads as "this machine has none of
    /// those", which is a claim; this is the absence of one.
    fn unreadable(what: &str, path: &str) -> Vec<Property> {
        vec![Property::new(
            what,
            &format!("Not available — nothing on this system provides {path}"),
        )]
    }

    /// A value the system did not report, said as an absence.
    ///
    /// "Not reported" rather than a blank, an "Unknown", or a zero. All three
    /// read as facts about the machine: a blank looks like an empty name, and
    /// a zero MHz looks like a stopped clock. This is the one phrasing that
    /// cannot be mistaken for a reading.
    const NOT_REPORTED: &'static str = "Not reported by this system";

    /// Render an optional value, or say it was not reported.
    /// An uptime as days, hours, minutes and seconds.
    ///
    /// Days are included because a machine that has been up for four days read
    /// `100h 0m 0s` in `apps/vpnmanager` until somebody noticed; the same
    /// arithmetic with the same missing field is how that happens twice.
    fn uptime_text(up: Option<Duration>) -> String {
        let Some(up) = up else {
            return Self::NOT_REPORTED.to_string();
        };
        let secs = up.as_secs();
        let (d, h, m, s) = (
            secs / 86_400,
            (secs / 3_600) % 24,
            (secs / 60) % 60,
            secs % 60,
        );
        if d > 0 {
            format!("{d}d {h}h {m}m {s}s")
        } else {
            format!("{h}h {m}m {s}s")
        }
    }

    /// When the machine booted: now, less how long it has been up.
    ///
    /// Two readings that can each be absent, and the answer needs both. The
    /// string this replaced was "2026-05-17 08:14:02 UTC", fixed, so a machine
    /// booted this morning reported a boot four months ago.
    fn boot_time_text(up: Option<Duration>) -> String {
        let Some(up) = up else {
            return Self::NOT_REPORTED.to_string();
        };
        let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return Self::NOT_REPORTED.to_string();
        };
        let Some(boot) = now.as_secs().checked_sub(up.as_secs()) else {
            // An uptime longer than the epoch means one of the two readings is
            // wrong, and there is no way to tell which.
            return Self::NOT_REPORTED.to_string();
        };
        let t = civildate::unix_to_datetime(i64::try_from(boot).unwrap_or(0));
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            t.year, t.month, t.day, t.hour, t.minute, t.second
        )
    }

    fn or_absent(value: Option<&str>) -> String {
        value.map_or_else(|| Self::NOT_REPORTED.to_string(), ToString::to_string)
    }

    /// Render an optional number with a unit, or say it was not reported.
    fn num_or_absent(value: Option<u32>, unit: &str) -> String {
        value.map_or_else(|| Self::NOT_REPORTED.to_string(), |n| format!("{n} {unit}"))
    }

    fn props_system_summary(&self) -> Vec<Property> {
        let (Some(cpu), Some(mem)) = (&self.cpu_info, &self.memory_info) else {
            // The OS rows below are this program's own constants and stay
            // truthful; only the hardware half is unobtainable.
            let mut props = vec![
                Property::new("OS Name", "Slate OS"),
                Property::new("OS Version", "1.0.0"),
            ];
            props.extend(Self::unreadable("Processor", "/sys/hardware/cpu"));
            props.extend(Self::unreadable("Memory", "/sys/hardware/memory"));
            return props;
        };
        vec![
            Property::new("OS Name", "Slate OS"),
            Property::new("OS Version", "1.0.0"),
            Property::new("OS Build", "2026.05.17-nightly"),
            Property::new("Kernel Version", "0.1.0-slateos"),
            Property::new("System Manufacturer", "SMBIOS: To Be Filled By O.E.M."),
            Property::new("Processor", &Self::or_absent(cpu.brand.as_deref())),
            Property::new(
                "Cores / Threads",
                &format!("{} / {}", cpu.physical_cores, cpu.logical_processors),
            ),
            Property::new(
                "Base Frequency",
                &Self::num_or_absent(cpu.base_clock_mhz, "MHz"),
            ),
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
            // A commit limit is what "total virtual memory" names, and
            // `procinfo::MemInfo` documents `commit_limit_kib` as never
            // published by this kernel. The figure here was 65536 MiB on every
            // machine.
            Property::new("Total Virtual Memory", Self::NOT_REPORTED),
            // A design constant, not a measurement: `design.txt` specifies
            // 16 KiB pages and the whole memory subsystem is built on it.
            Property::new("Page Size", "16 KiB"),
            Property::new("System Uptime", &Self::uptime_text(self.uptime)),
            Property::new("Boot Time", &Self::boot_time_text(self.uptime)),
            Property::new("Architecture", "x86_64"),
        ]
    }

    fn props_cpu(&self) -> Vec<Property> {
        let Some(cpu) = &self.cpu_info else {
            return Self::unreadable("Processor", "/sys/hardware/cpu");
        };
        let mut props = vec![
            Property::new("Processor Name", &Self::or_absent(cpu.brand.as_deref())),
            Property::new("Vendor", &Self::or_absent(cpu.vendor.as_deref())),
            Property::new("Family", &format!("{}", cpu.family)),
            Property::new("Model", &format!("{}", cpu.model)),
            Property::new("Stepping", &format!("{}", cpu.stepping)),
            Property::new("Physical Cores", &format!("{}", cpu.physical_cores)),
            Property::new("Logical Processors", &format!("{}", cpu.logical_processors)),
            Property::new(
                "Base Clock",
                &Self::num_or_absent(cpu.base_clock_mhz, "MHz"),
            ),
            Property::new(
                "Max Turbo Clock",
                &Self::num_or_absent(cpu.max_turbo_mhz, "MHz"),
            ),
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
        let Some(mem) = &self.memory_info else {
            return Self::unreadable("Memory", "/sys/hardware/memory");
        };
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
        let Some(d) = &self.display_info else {
            return Self::unreadable("Display", "/sys/hardware/display");
        };
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
            let state = if entry.enabled {
                String::new()
            } else {
                String::from("  [disabled]")
            };
            props.push(Property::new(
                &entry.name,
                &format!("{} ({}){state}", entry.path, entry.phase),
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
    /// Write the report to `path`, and say what happened.
    ///
    /// `export_text` was written, tested, and its result assigned to
    /// `let _report`. This is the half that was missing.
    pub fn write_report(&mut self, path: &std::path::Path) -> String {
        let text = self.export_text();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!("Wrote {} bytes to {}", text.len(), path.display()),
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

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
        // The picker takes input first while it is up, or a filename is typed
        // into the search box behind it. `Picked::Ignored` covers `Resize`, so
        // the app still learns its own size with a dialog open -- which it
        // needs, because it is the app that draws the dialog.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.status_message = self.write_report(&path);
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                EventResult::Consumed
            }
            // The arm `tick_interval` promised. Re-reads the two figures that
            // age: the uptime, and the memory the machine has left.
            //
            // The CPU is not re-read. Its model, family and cache geometry do
            // not change while a window is open, and polling them once a
            // second would be spending a `/sys` walk to confirm a constant.
            Event::Tick { .. } => {
                self.refresh_ageing_figures();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Re-read the figures that change while the window is open.
    ///
    /// A failed read leaves the previous value rather than blanking the row:
    /// `/proc` can be briefly unreadable, and a Summary that empties itself
    /// for one tick and fills back in is harder to read than one that holds.
    /// The first read is in `new`, so a value only ever goes missing here if
    /// it was missing to begin with.
    pub fn refresh_ageing_figures(&mut self) {
        use hwquery::HardwareProvider;
        let provider = hwquery::SyscallProvider::new();
        if let Ok(up) = provider.query_uptime() {
            self.uptime = Some(up);
        }
        if let Ok(mem) = provider.query_memory() {
            self.memory_info = Some(mem);
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
            Key::C if key.modifiers.ctrl => {
                self.status_message = CLIPBOARD_UNAVAILABLE.to_string();
                EventResult::Consumed
            }
            // This read `let _report = self.export_text();` and then said
            // "Exported system info to file". The report was composed in full,
            // dropped on the floor, and announced. **The status line is the
            // only evidence a user has that an export happened**, so a false
            // one is worse than no control at all.
            Key::E if key.modifiers.ctrl => {
                self.picker.open_to_write("system-info.txt");
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
        // The toolbar buttons, which were drawn and never hit-tested. Tested
        // ahead of the sidebar branch below: they sit well clear of
        // `SIDEBAR_WIDTH`, but putting the specific region before the general
        // one is what keeps that true if either moves.
        if let MouseEventKind::Press(MouseButton::Left) = &mouse.kind {
            let layout = Self::toolbar_layout();
            if layout.export.contains(mouse.x, mouse.y) {
                self.picker.open_to_write("system-info.txt");
                return EventResult::Consumed;
            }
            if layout.copy.contains(mouse.x, mouse.y) {
                self.status_message = CLIPBOARD_UNAVAILABLE.to_string();
                return EventResult::Consumed;
            }
            if layout.search.contains(mouse.x, mouse.y) {
                self.search_focused = true;
                return EventResult::Consumed;
            }
        }
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
    /// Named `render_tree` and not `render`: at equal arity an inherent method
    /// silently wins method lookup over `oswindow::app::App::render`, so an app
    /// that keeps the name draws nothing and reports no error.
    pub fn render_tree(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background fill.
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width,
            self.window_height,
            self.palette.crust,
        );

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

        // The picker last, so it draws over everything. A dialog that takes
        // input but is painted under the pane behind it is invisible and still
        // swallowing keys -- which looks exactly like an app that has frozen.
        tree.extend(
            self.picker
                .render(&self.palette, self.window_width, self.window_height),
        );

        tree
    }

    fn render_title_bar(&self, tree: &mut RenderTree) {
        tree.fill_rect(
            0.0,
            0.0,
            self.window_width,
            TITLE_BAR_HEIGHT,
            self.palette.crust,
        );

        // Title text.
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "System Information".to_string(),
            color: self.palette.text,
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
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    /// Where the toolbar's controls sit.
    ///
    /// These were locals inside `render_toolbar`, and `handle_mouse` never
    /// mentioned the buttons at all: Export and Copy were **pictures**.
    /// Clicking either did nothing -- no action, and not even the false status
    /// line the keyboard printed. Deriving the rectangles once and using them
    /// for both the drawing and the hit-test is what stops the drawn button
    /// and the clickable region drifting apart, and is the same discipline
    /// `tree_hit_test` already applies to the click and the hover.
    fn toolbar_layout() -> ToolbarLayout {
        let y = TITLE_BAR_HEIGHT + 5.0;
        let h = 22.0;
        let search = Rect {
            x: 8.0,
            y,
            w: 220.0,
            h,
        };
        let export = Rect {
            x: search.x + search.w + 16.0,
            y,
            w: 70.0,
            h,
        };
        let copy = Rect {
            x: export.x + export.w + 8.0,
            y,
            w: 70.0,
            h,
        };
        ToolbarLayout {
            search,
            export,
            copy,
        }
    }

    fn render_toolbar(&self, tree: &mut RenderTree) {
        let y = TITLE_BAR_HEIGHT;
        let layout = Self::toolbar_layout();
        tree.fill_rect(
            0.0,
            y,
            self.window_width,
            TOOLBAR_HEIGHT,
            self.palette.mantle,
        );

        // Search box.
        let search_x = layout.search.x;
        let search_y = layout.search.y;
        let search_w = layout.search.w;
        let search_h = layout.search.h;

        tree.push(RenderCommand::FillRect {
            x: search_x,
            y: search_y,
            width: search_w,
            height: search_h,
            color: self.palette.base,
            corner_radii: CornerRadii::all(3.0),
        });

        let border_color = if self.search_focused {
            self.palette.blue
        } else {
            self.palette.surface1
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
            self.palette.overlay0
        } else {
            self.palette.text
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
        let export_x = layout.export.x;
        let btn_w = layout.export.w;
        self.palette.push_surface(
            tree,
            export_x,
            search_y,
            btn_w,
            search_h,
            3.0,
            Surface::Card,
        );
        tree.push(RenderCommand::Text {
            x: export_x + 10.0,
            y: search_y + 4.0,
            text: "Export".to_string(),
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Copy button.
        let copy_x = layout.copy.x;
        self.palette
            .push_surface(tree, copy_x, search_y, btn_w, search_h, 3.0, Surface::Card);
        tree.push(RenderCommand::Text {
            x: copy_x + 14.0,
            y: search_y + 4.0,
            text: "Copy".to_string(),
            color: self.palette.subtext0,
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
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_sidebar(&self, tree: &mut RenderTree) {
        let top = self.pane_top();
        let height = self.pane_height();

        // Sidebar background.
        tree.fill_rect(0.0, top, SIDEBAR_WIDTH, height, self.palette.mantle);

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
                self.palette.surface1
            } else if self.hovered_tree_row == Some(idx) {
                self.palette.surface0
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
                    color: self.palette.subtext0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Label.
            let text_color = if cat == self.selected_category {
                self.palette.blue
            } else {
                self.palette.text
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
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    fn render_detail_pane(&self, tree: &mut RenderTree) {
        let top = self.pane_top();
        let left = SIDEBAR_WIDTH;
        let width = self.window_width - SIDEBAR_WIDTH;
        let height = self.pane_height();

        // Background.
        tree.fill_rect(left, top, width, height, self.palette.base);

        // Clip to detail area.
        tree.clip(left, top, width, height);

        // Category heading.
        let heading_y = top + DETAIL_HEADING_TOP;
        tree.push(RenderCommand::Text {
            x: left + 16.0,
            y: heading_y,
            text: self.selected_category.label().to_string(),
            color: self.palette.ink(self.palette.lavender),
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
            color: self.palette.surface0,
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
        tree.fill_rect(
            left,
            table_top,
            width,
            PROPERTY_HEADER_HEIGHT,
            self.palette.mantle,
        );
        tree.push(RenderCommand::Text {
            x: left + 16.0,
            y: table_top + 6.0,
            text: "Property".to_string(),
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(name_col_width - 20.0),
            overflow: TextOverflow::Ellipsis,
        });
        tree.push(RenderCommand::Text {
            x: left + name_col_width + 8.0,
            y: table_top + 6.0,
            text: "Value".to_string(),
            color: self.palette.subtext0,
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
                self.palette.base
            } else {
                self.palette.surface0
            };
            tree.fill_rect(left, row_y, width, PROPERTY_ROW_HEIGHT, row_bg);

            // Section headers get different styling. Asked of the row's kind,
            // not of its text: a name beginning `---` is a heading only if we
            // wrote it, and an environment variable may be called anything.
            let is_section = prop.kind == PropertyKind::Heading;
            let name_color = if is_section {
                self.palette.peach
            } else {
                self.palette.subtext0
            };
            let value_color = if is_section {
                self.palette.peach
            } else {
                self.palette.text
            };

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
                    self.palette.green
                } else if prop.value == "\u{2717}" {
                    self.palette.red
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
            self.palette.mantle,
        );

        // Top separator.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Status text.
        tree.push(RenderCommand::Text {
            x: 12.0,
            y: y + 5.0,
            text: self.status_message.clone(),
            color: self.palette.subtext0,
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
            color: self.palette.subtext0,
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

impl App for SysInfoState {
    /// Adopt the user's colours (§822).
    ///
    /// The trait's default does nothing, which is how this crate shipped a
    /// verbatim copy of Catppuccin Mocha: not overriding it is silent.
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "System Information".to_owned()
    }

    fn initial_size(&self) -> (u32, u32) {
        (self.window_width as u32, self.window_height as u32)
    }

    /// A second, because the uptime it draws is now read from `/proc`.
    ///
    /// This returned `None`, and said why: *"Some of what this app displays
    /// genuinely ages — uptime, available memory, free disk space. None of it
    /// is read from anywhere ... When a source exists this returns its poll
    /// interval and `handle_event` grows a `Tick` arm that re-reads."*
    ///
    /// The source exists. **An uptime that is read once and then drawn forever
    /// is worse than a constant**, because a constant is at least obviously
    /// not a clock: a figure that was true when the window opened and is
    /// wrong by however long it has been open is the frozen-clock defect this
    /// project has now found in five other windows.
    ///
    /// One second, because that is the resolution `/proc/uptime` reports and
    /// there is nothing to gain from asking more often than the number can
    /// change.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.window_width = width;
        self.window_height = height;
        self.render_tree()
    }
}

fn main() -> ExitCode {
    let mut info = SysInfoState::new();
    app::launch("sysinfo", &mut info)
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
    // `float_cmp` joins them for one assertion: that `cpu_percent` is exactly
    // 0.0, which is not an approximation of a measurement but the *absence* of
    // one. An epsilon comparison there would assert something weaker than what
    // actually holds, and the value it guards against is a rate invented from
    // a single sample.
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::float_cmp
    )]

    use super::*;

    // ------------------------------------------------------------------
    // The compositor wiring
    // ------------------------------------------------------------------

    /// The app asks for a clock, because the uptime it draws is now read.
    ///
    /// This asserted `None`, under the reason that "some of what this app
    /// shows genuinely ages, but none of it is read from anywhere, so a tick
    /// would redraw constants on a timer". True when written, and the
    /// condition it named has since been met: the uptime comes from
    /// `/proc/uptime`. **A figure read once and drawn forever is worse than a
    /// constant** -- a constant is at least obviously not a clock.
    #[test]
    fn the_app_asks_for_a_clock_because_the_uptime_is_read() {
        let app = SysInfoState::new();
        assert_eq!(app.tick_interval(), Some(Duration::from_secs(1)));
    }

    /// A tick re-reads rather than being accepted and ignored.
    ///
    /// Consuming `Tick` without re-reading is the frozen clock with extra
    /// steps, and it is what five other windows in this tree were doing.
    #[test]
    fn a_tick_re_reads_the_figures_that_age() {
        let mut app = SysInfoState::new();
        app.uptime = None;
        app.memory_info = None;

        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 1000 }),
            EventResult::Consumed
        );

        // On this host `/proc` is absent, so the read fails and the previous
        // value is kept -- which is the documented behaviour and is what makes
        // the assertion below the honest one to write. What is pinned is that
        // the tick reached `refresh_ageing_figures` at all: with no arm it
        // would have returned `Ignored`.
        let provider_can_read = {
            use hwquery::HardwareProvider;
            hwquery::SyscallProvider::new().query_uptime().is_ok()
        };
        assert_eq!(
            app.uptime.is_some(),
            provider_can_read,
            "the tick did not re-read: uptime is {:?} while the provider {} read one",
            app.uptime,
            if provider_can_read {
                "could"
            } else {
                "could not"
            }
        );
    }

    #[test]
    fn rendering_at_a_new_size_adopts_it_and_draws_something() {
        // The first frame is drawn before any `Resize` arrives, and a
        // compositor may grant a size that was never asked for.
        let mut app = SysInfoState::new();
        let tree = app.render(1600.0, 900.0);
        assert_eq!((app.window_width, app.window_height), (1600.0, 900.0));
        assert!(!tree.is_empty(), "the app drew nothing");
    }

    #[test]
    fn the_initial_size_is_the_size_the_app_is_holding() {
        let app = SysInfoState::new();
        assert_eq!(
            app.initial_size(),
            (app.window_width as u32, app.window_height as u32)
        );
    }

    #[test]
    fn a_close_request_exits_and_an_unwanted_key_does_not_redraw() {
        let mut app = SysInfoState::new();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        let unwanted = Event::Key(KeyEvent {
            key: Key::F9,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert!(matches!(app.on_event(&unwanted), Response::Idle));
    }

    #[test]
    fn every_category_renders_without_panicking_at_an_awkward_size() {
        // A category reachable by holding Down that panics when drawn is a
        // crash the user reaches by holding Down.
        let mut app = SysInfoState::new();
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        for _ in 0..40 {
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                assert!(
                    !app.render(w, h).is_empty(),
                    "{:?} drew nothing at {w}x{h}",
                    app.selected_category
                );
            }
            app.handle_event(&down);
        }
    }

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
        // The CPU section comes from a fixture now: the application no
        // longer invents a processor, so there are no features to head.
        let mut app = SysInfoState::new();
        app.cpu_info = Some(fixture_cpu());
        let report = app.export_text();
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

    /// A processor for the tests to describe, since the application no longer
    /// invents one.
    ///
    /// Deliberately not a plausible real part. `SysInfoState::new` used to
    /// carry a GenuineIntel with a Radeon RX 7900 XTX beside it, and the
    /// closer a fixture looks to a real machine the easier it is for it to
    /// reach production -- which is how that one got there. "Fixture CPU" says
    /// what it is in the one place a reader would see it.
    fn fixture_cpu() -> CpuInfo {
        CpuInfo {
            brand: Some("Fixture CPU".to_string()),
            vendor: Some("FixtureVendor".to_string()),
            family: 1,
            model: 2,
            stepping: 3,
            physical_cores: 4,
            logical_processors: 8,
            base_clock_mhz: Some(1000),
            max_turbo_mhz: Some(2000),
            l1_data_kb: 32,
            l1_inst_kb: 32,
            l2_kb: 512,
            l3_kb: 8192,
            features: (0..24)
                .map(|i| (format!("FEATURE_{i}"), i % 2 == 0))
                .collect(),
        }
    }

    /// Memory for the tests, on the same terms as [`fixture_cpu`].
    fn fixture_memory() -> MemoryInfo {
        MemoryInfo {
            total_mb: 4096,
            available_mb: 2048,
            mem_type: "FixtureRAM".to_string(),
            speed_mhz: 1600,
            slots_used: 1,
            slots_total: 2,
            slots: vec![MemorySlot {
                slot_name: "Slot 0".to_string(),
                size_mb: 4096,
                mem_type: "FixtureRAM".to_string(),
                speed_mhz: 1600,
                manufacturer: "Fixtures Inc".to_string(),
            }],
        }
    }

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

        // The property table's rows come from the fixture, not from the
        // application.
        //
        // Until 2026-09-15 they came from `SysInfoState::new`, which invented
        // a machine -- a GenuineIntel processor, a Radeon RX 7900 XTX, forty
        // PCI devices -- and every scroll test below was quietly resting on
        // it. When that went, sixteen tests failed at once.
        //
        // They failed *loudly*, and that is worth noticing: the two asserts
        // below are this fixture checking it can still fail, and they named
        // the problem in their message ("4 rows in 144 px") instead of leaving
        // sixteen scroll tests passing vacuously against a list that now fits
        // on screen. The same defect in `apps/settings` had no such guard and
        // surfaced only because a list became empty rather than merely
        // shorter.
        app.cpu_info = Some(fixture_cpu());
        app.memory_info = Some(fixture_memory());
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

    /// The disks the kernel publishes are read, and nothing else is invented.
    ///
    /// `query_storage` read `/sys/hardware/block`, **a path this kernel has
    /// never served**, and parsed `part0_`-prefixed keys out of it. Lane A
    /// publishes `/sys/devices/block/<name>/{sector_count,sector_size,
    /// read_only}` as scalar files, so the Storage category reported "cannot
    /// read" on a machine whose disks were there the whole time.
    ///
    /// Capacity multiplies the two names that are read rather than assuming
    /// 512-byte sectors -- lane A's own comment gives the reason, and it is
    /// the sharpest kind: every device here is 512 today, **which is the
    /// condition that lets a 512-assumption ship unnoticed.** The fixture
    /// below uses 4096 so the assumption cannot pass.
    #[test]
    fn the_disks_the_kernel_publishes_are_read() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-block-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join("sys/devices/block");
        std::fs::create_dir_all(base.join("vda")).expect("fixture");
        std::fs::create_dir_all(base.join("vdb")).expect("fixture");

        std::fs::write(base.join("vda/sector_count"), b"2048\n").unwrap();
        std::fs::write(base.join("vda/sector_size"), b"4096\n").unwrap();
        std::fs::write(base.join("vda/read_only"), b"0\n").unwrap();

        std::fs::write(base.join("vdb/sector_count"), b"100\n").unwrap();
        std::fs::write(base.join("vdb/sector_size"), b"512\n").unwrap();
        std::fs::write(base.join("vdb/read_only"), b"1\n").unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        let mut disks = {
            use hwquery::HardwareProvider;
            provider
                .query_storage()
                .expect("the fixture tree is readable")
        };
        disks.sort_by(|a, b| a.model.cmp(&b.model));

        assert_eq!(disks.len(), 2, "one entry per device directory");

        let vda = disks.first().expect("vda");
        assert_eq!(vda.model, "vda");
        assert_eq!(
            vda.capacity_bytes,
            2048 * 4096,
            "capacity is sector_count times the device's own sector_size"
        );
        assert!(
            vda.smart_status.is_empty(),
            "a writable disk is not annotated"
        );

        let vdb = disks.get(1).expect("vdb");
        assert_eq!(vdb.capacity_bytes, 100 * 512);
        assert_eq!(vdb.smart_status, "read-only");

        for d in &disks {
            assert!(d.partitions.is_empty(), "invented a partition table");
            assert!(d.serial.is_empty(), "invented a serial number");
            assert!(d.interface.is_empty(), "invented an interface");
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A device that vanishes between the listing and the read is skipped.
    #[test]
    fn a_block_device_with_no_scalars_is_skipped_not_fatal() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-block-gone-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join("sys/devices/block");
        std::fs::create_dir_all(base.join("vda")).expect("fixture");
        // A directory with no files: the shape of a device unregistered while
        // the list was being walked.
        std::fs::create_dir_all(base.join("gone")).expect("fixture");
        std::fs::write(base.join("vda/sector_count"), b"8\n").unwrap();
        std::fs::write(base.join("vda/sector_size"), b"512\n").unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let disks = provider.query_storage().expect("readable");

        assert_eq!(disks.len(), 1, "the half-gone device is skipped, not fatal");
        assert_eq!(disks.first().expect("vda").model, "vda");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With no such tree at all, it says so rather than reporting no disks.
    ///
    /// **Absent is not empty.** "This machine has no disks" and "I could not
    /// look" are different answers, and only one of them is ever true here.
    #[test]
    fn an_absent_block_tree_is_an_error_not_an_empty_list() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-block-absent-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_storage().is_err(),
            "an unreadable tree reported as an empty disk list"
        );
    }

    /// The interfaces come from `/proc/net/dev`, with nothing filled in around them.
    ///
    /// `query_network` read `/sys/hardware/net`, which the kernel has never
    /// served. `/proc/net/dev` is published and carries the names and the
    /// traffic counters.
    ///
    /// The empty fields are the point of the test as much as the full ones. A
    /// MAC address, an IPv4 lease, a gateway, a DNS server, a link speed and a
    /// duplex mode are published by nothing in this tree, and lane A declined
    /// to add a `/sys/devices/net/` in the same words: the kernel's
    /// `InterfaceInfo` "has no name field, so both would be invented". **A row
    /// carrying a plausible 192.168.1.x is worse than one carrying a blank**,
    /// because the blank is legible as absent.
    #[test]
    fn the_network_interfaces_are_read_and_nothing_is_filled_in_around_them() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-net-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc/net")).expect("fixture");
        std::fs::write(
            root.join("proc/net/dev"),
            b"Inter-|   Receive                    |  Transmit\n\
             face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets\n\
                lo:    1234       9    0    0    0     0          0         0     5678      11\n\
              eth0:  900000     700    0    0    0     0          0         0   400000     300\n",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let mut adapters = provider.query_network().expect("the fixture is readable");
        adapters.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(adapters.len(), 2, "one row per interface");
        let eth0 = adapters.first().expect("eth0");
        assert_eq!(eth0.name, "eth0");
        assert_eq!(eth0.bytes_received, 900_000, "rx is the receive column");
        assert_eq!(eth0.bytes_sent, 400_000, "tx is the transmit column");

        for a in &adapters {
            assert!(a.mac_address.is_empty(), "invented a MAC address");
            assert!(a.ipv4.is_empty(), "invented an address lease");
            assert!(a.gateway.is_empty(), "invented a gateway");
            assert!(a.dns.is_empty(), "invented a resolver");
            assert_eq!(a.speed_mbps, 0, "invented a link speed");
            assert!(a.duplex.is_empty(), "invented a duplex mode");
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With no `/proc/net/dev`, it says so rather than reporting no interfaces.
    #[test]
    fn an_absent_net_dev_is_an_error_not_an_empty_list() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-net-absent-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_network().is_err(),
            "an unreadable /proc/net/dev reported as a machine with no interfaces"
        );
    }

    /// The running processes are read from `/proc`, not invented.
    ///
    /// `query_processes` read `/sys/proc` -- a path with no producer and no
    /// precedent; Linux has never put a process list under `/sys`. This is the
    /// third window in the tree to read `/proc/<pid>/stat` and the third to do
    /// it through `procinfo`, after `apps/procexplorer` and
    /// `apps/sysmonitor` earlier today.
    #[test]
    fn the_running_processes_are_read_from_proc() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-proc-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc/41")).expect("fixture");
        // A directory with no `stat`: a process that exited while the list was
        // being walked.
        std::fs::create_dir_all(root.join("proc/42")).expect("fixture");
        std::fs::write(
            root.join("proc/41/stat"),
            b"41 (shell) R 1 41 41 0 -1 0 0 0 0 0 200 50 0 0 20 0 8 0 900 \
              4096000 300 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let procs = provider.query_processes().expect("the fixture is readable");

        assert_eq!(
            procs.len(),
            1,
            "the half-gone process is skipped, not fatal"
        );
        let p = procs.first().expect("one process");
        assert_eq!(p.pid, 41);
        assert_eq!(p.name, "shell");
        assert_eq!(
            p.memory_kb,
            300 * procinfo::PAGE_SIZE_KIB,
            "resident pages become KiB through the shared page size"
        );
        assert_eq!(
            p.cpu_percent, 0.0,
            "a rate needs two samples of a counter and this query has one"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With no `/proc`, it says so rather than reporting no processes.
    #[test]
    fn an_absent_proc_is_an_error_not_an_empty_process_list() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-proc-absent-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_processes().is_err(),
            "an unreadable /proc reported as a machine running nothing"
        );
    }

    /// The interrupt lines come from `/proc/interrupts`, and the flag is a flag.
    ///
    /// `query_irqs` read `/sys/hardware/irqs`, which this kernel has never
    /// served and which lane A has recorded it will never serve. The category
    /// has therefore said "cannot read" since it was written.
    ///
    /// **`irq_type` is empty, deliberately.** It used to default to `"Edge"` --
    /// a trigger mode nobody published. The kernel serves a label and a
    /// pending flag and nothing that is a type, and a plausible default in a
    /// column headed Type is the same defect as a plausible default anywhere
    /// else.
    #[test]
    fn the_interrupt_lines_are_read_and_the_flag_is_not_a_count() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-irq-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc")).expect("fixture");
        std::fs::write(
            root.join("proc/interrupts"),
            b"APIC timer ticks: 4210\nISR latency:  (no measurements)\n\n\
              IRQ  PENDING  DESCRIPTION\n\
              0    no       PIT timer / HPET\n\
              1    yes      Keyboard (PS/2)\n",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let irqs = provider.query_irqs().expect("the fixture is readable");

        assert_eq!(irqs.len(), 2);
        assert_eq!(irqs[0].irq_number, 0);
        assert_eq!(irqs[0].device, "PIT timer / HPET");
        assert!(!irqs[0].asserted);
        assert_eq!(irqs[1].irq_number, 1);
        assert!(irqs[1].asserted, "the asserted line was read as idle");
        for irq in &irqs {
            assert!(
                irq.irq_type.is_empty(),
                "invented a trigger mode the kernel does not publish: {:?}",
                irq.irq_type
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With no `/proc/interrupts`, it says so rather than reporting no lines.
    #[test]
    fn an_absent_interrupts_file_is_an_error_not_an_empty_table() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-irq-absent-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_irqs().is_err(),
            "an unreadable file reported as a machine with no interrupt lines"
        );
    }

    /// The outputs come from `/proc/monitors`, and the adapter stays empty.
    ///
    /// `/proc/monitors` describes *outputs*, not the card driving them, so the
    /// GPU name, vendor, VRAM and driver version have no source. A plausible
    /// "16384 MB" beside real resolutions would be the one figure on the page
    /// nobody could check.
    #[test]
    fn the_outputs_are_read_and_the_adapter_is_not_invented() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-mon-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc")).expect("fixture");
        std::fs::write(
            root.join("proc/monitors"),
            // Copied from `procinfo`'s own fixture rather than guessed. My
            // first version invented a plausible-looking layout -- `primary:`
            // for `primary_id:`, `at 0,0` for `pos=(0,0)` -- and the parser
            // read no primary from it, so the resolution came back empty. A
            // fixture written from memory tests the memory.
            //
            // **The primary is the SECOND row, and the rows carry different
            // modes.** procinfo's own fixture makes output 1 the primary,
            // which is the one arrangement where reading `outputs.first()`
            // and reading `primary()` cannot be told apart -- and that is
            // exactly the mistake this test exists to catch. Copy a fixture
            // for its format; choose its contents for what you are pinning.
            b"monitors: 2
              enabled: 1
              layout_mode: extended
              primary_id: 2
              ops: 14
              desktop: 3840x1200 at (-1920,0)
              1: HP-Z24 1920x1080@75Hz pos=(-1920,0) scale=125% HDMI [disabled]
              2: DELL U2412 1920x1200@60Hz pos=(0,0) scale=100% DisplayPort [primary]
",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let display = provider.query_display().expect("the fixture is readable");

        assert_eq!(
            display.resolution, "1920x1200",
            "the resolution should be the PRIMARY output's mode"
        );
        assert_eq!(
            display.refresh_rate_hz, 60,
            "the refresh rate should be the PRIMARY output's, not the first row's 75"
        );
        assert_eq!(
            display.outputs,
            vec![
                (String::from("HP-Z24"), false),
                (String::from("DELL U2412"), true),
            ],
            "a disabled output is still listed, and its state is read not assumed"
        );
        assert!(display.gpu_name.is_empty(), "invented an adapter name");
        assert!(display.vendor.is_empty(), "invented a vendor");
        assert_eq!(display.vram_mb, 0, "invented a VRAM size");
        assert!(
            display.driver_version.is_empty(),
            "invented a driver version"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// With no `/proc/monitors`, it says so rather than reporting no screens.
    #[test]
    fn an_absent_monitors_file_is_an_error_not_an_empty_list() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-mon-absent-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_display().is_err(),
            "an unreadable file reported as a machine with no outputs"
        );
    }

    /// The port ranges come from `/proc/ioport`.
    ///
    /// `query_io_ports` read `/sys/hardware/ioports`, which this kernel has
    /// never served. All three of `IoPortInfo`'s fields have a source in the
    /// real file, which is why this category is wired and PCI is not.
    ///
    /// **The summary lines are the hazard.** `Untracked R: 7` is a word then a
    /// number in exactly the places a region name and a counter occupy, so a
    /// position-based reader reports a region called `Untracked`. They are in
    /// this fixture for that reason -- a fixture holding only the rows the
    /// parser wants tests a file nobody serves.
    #[test]
    fn the_port_ranges_are_read_and_the_summary_is_not_a_region() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-port-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc")).expect("fixture");
        std::fs::write(
            root.join("proc/ioport"),
            b"=== I/O Port Stats ===\n\
              Regions: 2  Reads: 12  Writes: 48  Untracked R: 7  Untracked W: 3  Ops: 70\n\
              \n\
              Per-region:\n\
                COM1   0x03f8-0x03ff  reads=12  writes=48  rbytes=12  wbytes=48\n\
                PIT    0x0040-0x0043  reads=0  writes=9  rbytes=0  wbytes=9\n",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let ports = provider.query_io_ports().expect("the fixture is readable");

        assert_eq!(ports.len(), 2, "a summary line was read as a region");
        assert_eq!(ports[0].device, "COM1");
        assert_eq!(ports[0].start, 0x03f8);
        assert_eq!(ports[0].end, 0x03ff);
        assert_eq!(ports[1].device, "PIT");
        assert!(
            !ports.iter().any(|p| p.device == "Untracked"),
            "the untracked-counter line was read as a port range"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The drivers come from `/proc/kmod`, and none of them gains a path.
    ///
    /// `DriverInfo::path` has no source: `/proc/kmod` publishes a name, a
    /// version, a state, a kind, a size and a refcount, and on a machine with
    /// no module files there is no path for it to publish. Two of three fields
    /// are real and the third is empty, rather than filled with something that
    /// would read as a location on disk.
    #[test]
    fn the_modules_are_read_and_none_of_them_gains_a_path() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-kmod-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc")).expect("fixture");
        std::fs::write(
            root.join("proc/kmod"),
            b"=== Kernel Modules ===\n\
              live_modules: 2\n\
              total_loads: 5\n\
              total_unloads: 3\n\
              total_errors: 0\n\
              ops: 8\n\
                ext4 1.0.0 [live] filesystem 262144B refs=3\n\
                nvme 2.1.0 [live] driver 131072B refs=1\n",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let drivers = provider.query_drivers().expect("the fixture is readable");

        assert_eq!(drivers.len(), 2, "a counter line was read as a module");
        assert_eq!(drivers[0].name, "ext4");
        assert_eq!(drivers[0].status, "live");
        for d in &drivers {
            assert!(
                d.path.is_empty(),
                "invented a path for a module the kernel gives none for: {:?}",
                d.path
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The startup items come from `/proc/autostart`, header and all.
    ///
    /// **The header row is the control.** `ID NAME PHASE CONDITION ENABLED
    /// ORDER COMMAND` has seven tokens in exactly the places an item's seven
    /// fields occupy, so nothing about its *shape* excludes it -- a reader
    /// that checked shape alone would add a phantom item named `NAME` to
    /// every listing, and `Total items` above would agree with the extra row.
    ///
    /// `phase` is not `source`. A startup manager's "source" means where the
    /// entry was registered; `/proc/autostart` publishes when it runs. The
    /// field was renamed rather than repurposed, for the same reason
    /// `/proc/devicemgr`'s bus *type* was not wired into a bus *number*.
    #[test]
    fn the_startup_items_are_read_and_the_header_is_not_one() {
        let root =
            std::env::temp_dir().join(format!("sysinfo-auto-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc")).expect("fixture");
        std::fs::write(
            root.join("proc/autostart"),
            b"Autostart Items\n\
              ===============\n\
              \n\
              Total items:   2\n\
              Enabled:       1\n\
              System:        1\n\
              Operations:    4\n\
              \n\
              ID   NAME                 PHASE            CONDITION  ENABLED  ORDER  COMMAND\n\
              1    NetworkManager       Boot             Always     true     10     /usr/bin/nm\n\
              2    Backup               Login            OnAC       false    20     /usr/bin/backup --daily\n",
        )
        .unwrap();

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        let items = provider.query_startup().expect("the fixture is readable");

        assert_eq!(items.len(), 2, "the header row was read as an item");
        assert!(
            !items.iter().any(|i| i.name == "NAME"),
            "the header row became a startup item"
        );
        assert_eq!(items[0].name, "NetworkManager");
        assert_eq!(items[0].path, "/usr/bin/nm");
        assert_eq!(items[0].phase, "Boot");
        assert!(items[0].enabled);
        assert!(
            !items[1].enabled,
            "a disabled item was listed as one that runs"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Each of the three says so when its file is absent.
    #[test]
    fn an_absent_file_is_an_error_not_an_empty_list() {
        let root = std::env::temp_dir().join(format!(
            "sysinfo-absent3-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let provider = hwquery::SyscallProvider::at(&root.to_string_lossy());
        use hwquery::HardwareProvider;
        assert!(
            provider.query_io_ports().is_err(),
            "an unreadable file reported as a machine with no port ranges"
        );
        assert!(
            provider.query_drivers().is_err(),
            "an unreadable file reported as a machine with no drivers"
        );
        assert!(
            provider.query_startup().is_err(),
            "an unreadable file reported as a machine with nothing starting up"
        );
    }

    fn ctrl(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn door_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("slateos-sysinfo-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn click_at(x: f32, y: f32) -> Event {
        Event::Mouse(guitk::event::MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// The report reaches the disk.
    ///
    /// `export_text` was written and tested, and its one caller read
    /// `let _report = self.export_text();` followed by a status line saying
    /// "Exported system info to file". The report was composed, dropped, and
    /// announced. **This test is the half that was missing:** the bytes a
    /// reader gets back are the bytes the app composed.
    #[test]
    fn the_exported_report_reaches_the_disk() {
        let path = door_dir().join("system-info.txt");
        let _ = std::fs::remove_file(&path);

        let mut app = SysInfoState::new();
        let expected = app.export_text();
        let said = app.write_report(&path);

        let back = std::fs::read_to_string(&path).expect("the file the app said it wrote");
        assert_eq!(
            back, expected,
            "what was read back is not what was composed"
        );
        assert!(
            said.contains(&format!("{} bytes", expected.len())),
            "the status line should report the size actually written, said: {said}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Ctrl+E asks where to put it rather than claiming it already has.
    #[test]
    fn ctrl_e_opens_the_picker() {
        let mut app = SysInfoState::new();
        assert!(!app.picker.is_open(), "nothing should be open at rest");

        assert_eq!(app.handle_event(&ctrl(Key::E)), EventResult::Consumed);
        assert!(app.picker.is_open(), "Ctrl+E should ask for a destination");
        assert!(
            !app.status_message.contains("Exported"),
            "nothing has been exported yet, said: {}",
            app.status_message
        );
    }

    /// An open picker is drawn, not merely routed to.
    #[test]
    fn the_open_picker_is_drawn() {
        let mut app = SysInfoState::new();
        let closed = app.render_tree().commands.len();

        app.handle_event(&ctrl(Key::E));
        let open = app.render_tree().commands.len();

        let own = open.saturating_sub(closed);
        assert!(
            own > 0,
            "the open picker contributed {own} commands; it is not being drawn"
        );
    }

    /// A resize still reaches the app while the picker is up.
    ///
    /// The app is what draws the dialog, so a picker that swallowed `Resize`
    /// would leave itself being drawn at the old size.
    #[test]
    fn a_resize_reaches_the_app_under_an_open_picker() {
        let mut app = SysInfoState::new();
        app.handle_event(&ctrl(Key::E));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&Event::Resize {
            width: 1400,
            height: 900,
        });
        assert!(
            (app.window_width - 1400.0).abs() < f32::EPSILON,
            "the resize did not reach the app: width is {}",
            app.window_width
        );
        assert!(app.picker.is_open(), "a resize should not close the dialog");
    }

    /// Ctrl+C no longer reports an act it cannot perform.
    #[test]
    fn ctrl_c_does_not_claim_a_copy_that_did_not_happen() {
        let mut app = SysInfoState::new();
        app.handle_event(&ctrl(Key::C));
        assert!(
            !app.status_message.contains("copied"),
            "still claiming a copy: {}",
            app.status_message
        );
        assert!(
            app.status_message.contains("Ctrl+E"),
            "a denial should name what does work, said: {}",
            app.status_message
        );
    }

    /// The toolbar buttons are controls, not pictures.
    ///
    /// Neither appeared anywhere in `handle_mouse`: clicking either did
    /// nothing at all. The click here is aimed with the same `toolbar_layout`
    /// the drawing uses, so this cannot pass against a rectangle the renderer
    /// does not actually use.
    #[test]
    fn the_export_and_copy_buttons_respond_to_a_click() {
        let layout = SysInfoState::toolbar_layout();

        let mut app = SysInfoState::new();
        let mid = |r: Rect| click_at(r.x + r.w / 2.0, r.y + r.h / 2.0);

        assert_eq!(app.handle_event(&mid(layout.export)), EventResult::Consumed);
        assert!(app.picker.is_open(), "the Export button did nothing");

        let mut app = SysInfoState::new();
        app.handle_event(&mid(layout.copy));
        assert_eq!(
            app.status_message, CLIPBOARD_UNAVAILABLE,
            "the Copy button did nothing"
        );
    }

    /// The drawn buttons sit where the hit-test looks.
    #[test]
    fn the_drawn_buttons_sit_where_the_click_looks() {
        let app = SysInfoState::new();
        let layout = SysInfoState::toolbar_layout();
        let mut tree = RenderTree::new();
        app.render_toolbar(&mut tree);

        for (label, rect) in [("Export", layout.export), ("Copy", layout.copy)] {
            let drawn = tree
                .commands
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, x, y, .. } if text == label => Some((*x, *y)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{label} is not drawn at all"));
            assert!(
                rect.contains(drawn.0, drawn.1),
                "{label} is drawn at {drawn:?}, outside the rectangle the click tests: {rect:?}"
            );
        }
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
    /// The height test is not belt-and-braces. The even stripe and the search
    /// field share `base`, so a colour-only filter also matches
    /// the pane's own background and reports one row more than the table
    /// drew — which is exactly how the first draft of this helper made two
    /// correct page-step assertions fail by one. A helper filtered on the
    /// Every colour this app draws comes from the user's palette.
    ///
    /// The guard §822 expects each converted crate to adopt. It is also what
    /// finds the work a survey of `const COLOR_*` cannot see -- inline
    /// literals, and text hardcoded on a themed fill.
    #[test]
    fn every_colour_the_sysinfo_window_draws_comes_from_its_palette() {
        // Every category, not just the one the window opens on -- each is a
        // different detail pane, and twenty of the twenty-one were never
        // rendered here. See `known-issues.md`
        // `TD-C-EIGHT-THEME-GUARDS-CHECK-A-PROGRAM'S-OPENING-FRAME`.
        for light in [false, true] {
            for category in SysInfoCategory::ALL {
                let mut app = SysInfoState::new();
                app.palette = Palette::for_mode(light);
                app.selected_category = category;
                let tree = app.render_tree();
                // Not a formality: a guard that sweeps an empty command list passes
                // for the wrong reason, and this file's own `drawn_property_rows`
                // exists because a helper filtered on the wrong property once
                // already. A default `SysInfoState` draws its chrome, its sidebar
                // and its detail pane, so the floor is generous and still real.
                assert!(
                    tree.commands.len() > 50,
                    "the sweep examined only {} commands, which is not a render",
                    tree.commands.len()
                );
                appearance::palette_check::assert_drawn_from(
                    &app.palette,
                    &tree.commands,
                    &[],
                    &format!("sysinfo {category:?} (light={light})"),
                );
            }
        }
    }

    /// wrong property is as wrong as the code it is checking.
    fn drawn_property_rows(app: &SysInfoState) -> Vec<(f32, Color)> {
        let mut t = RenderTree::new();
        app.render_detail_pane(&mut t);
        t.commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y, height, color, ..
                } if (*color == app.palette.base || *color == app.palette.surface0)
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
