//! Slate OS Device Manager
//!
//! Graphical hardware device manager inspired by Windows Device Manager.
//! Features:
//! - Device tree with expandable categories (Display, Audio, Network, etc.)
//! - Device status indicators (working, warning, error, disabled, unknown)
//! - Detailed properties panel (name, type, vendor, driver, IRQ, MMIO)
//! - Driver information (name, version, provider, date)
//! - Enable/disable devices and driver uninstall
//! - Scan for hardware changes
//! - Device search/filter
//! - Toolbar with common actions
//! - Resource view (IRQ assignments, MMIO ranges, DMA channels)
//! - Problem device highlighting
//! - Driver update check model
//! - Device event history (connected, disconnected, error)
//! - Export hardware report
//!
//! Uses the guitk library for UI rendering. Hardware data is gathered
//! through Slate OS syscalls; stubbed with representative data for initial
//! development.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Constants -- layout dimensions
// ============================================================================

/// Width of the device tree sidebar.
const SIDEBAR_WIDTH: f32 = 280.0;
/// Height of the title bar.
const TITLE_BAR_HEIGHT: f32 = 36.0;
/// Height of the toolbar.
const TOOLBAR_HEIGHT: f32 = 38.0;
/// Height of the status bar at the bottom.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of each tree node row.
const TREE_ROW_HEIGHT: f32 = 24.0;
/// Indentation per tree level.
const TREE_INDENT: f32 = 20.0;
/// Height of each property row in the detail panels.
const PROPERTY_ROW_HEIGHT: f32 = 22.0;
/// Height of property/resource section headers.
const SECTION_HEADER_HEIGHT: f32 = 28.0;
/// Height of tab bar in properties panel.
const TAB_BAR_HEIGHT: f32 = 28.0;
/// Default window width.
const DEFAULT_WIDTH: f32 = 1100.0;
/// Default window height.
const DEFAULT_HEIGHT: f32 = 720.0;
/// Height of search bar.
const SEARCH_BAR_HEIGHT: f32 = 30.0;
/// Height of each event history row.
const EVENT_ROW_HEIGHT: f32 = 20.0;
/// Maximum number of event history entries retained.
const MAX_EVENT_HISTORY: usize = 200;
/// Toolbar button width.
const TOOLBAR_BTN_WIDTH: f32 = 90.0;
/// Toolbar button height.
const TOOLBAR_BTN_HEIGHT: f32 = 26.0;

// ============================================================================
// Color palette -- Catppuccin Mocha
// ============================================================================

/// Base background (Crust).
const COLOR_BASE: Color = Color::rgb(17, 17, 27);
/// Slightly lighter surface (Mantle).
const COLOR_MANTLE: Color = Color::rgb(24, 24, 37);
/// Surface for panels (Surface0).
const COLOR_SURFACE0: Color = Color::rgb(30, 30, 46);
/// Lighter surface for selected items (Surface1).
const COLOR_SURFACE1: Color = Color::rgb(49, 50, 68);
/// Overlay surface (Surface2).
const COLOR_SURFACE2: Color = Color::rgb(69, 71, 90);
/// Primary text (Text).
const COLOR_TEXT: Color = Color::rgb(205, 214, 244);
/// Secondary/dimmed text (Subtext0).
const COLOR_SUBTEXT: Color = Color::rgb(166, 173, 200);
/// Overlay text (Overlay1).
const COLOR_OVERLAY: Color = Color::rgb(147, 153, 178);
/// Blue accent.
const COLOR_BLUE: Color = Color::rgb(137, 180, 250);
/// Lavender accent.
const COLOR_LAVENDER: Color = Color::rgb(180, 190, 254);
/// Green (success/working).
const COLOR_GREEN: Color = Color::rgb(166, 227, 161);
/// Yellow (warning).
const COLOR_YELLOW: Color = Color::rgb(249, 226, 175);
/// Red (error/danger).
const COLOR_RED: Color = Color::rgb(243, 139, 168);
/// Peach.
const COLOR_PEACH: Color = Color::rgb(250, 179, 135);
/// Mauve.
const COLOR_MAUVE: Color = Color::rgb(203, 166, 247);
/// Teal.
const COLOR_TEAL: Color = Color::rgb(148, 226, 213);
/// Sapphire.
const COLOR_SAPPHIRE: Color = Color::rgb(116, 199, 236);

// ============================================================================
// Device categories
// ============================================================================

/// Hardware device category for the tree view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceCategory {
    Display,
    Audio,
    Network,
    Storage,
    Usb,
    Input,
    System,
    Other,
}

impl DeviceCategory {
    /// Human-readable label for this category.
    pub fn label(self) -> &'static str {
        match self {
            Self::Display => "Display Adapters",
            Self::Audio => "Audio Devices",
            Self::Network => "Network Adapters",
            Self::Storage => "Storage Controllers",
            Self::Usb => "USB Controllers",
            Self::Input => "Input Devices",
            Self::System => "System Devices",
            Self::Other => "Other Devices",
        }
    }

    /// Icon character for this category (simple ASCII marker).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Display => "[D]",
            Self::Audio => "[A]",
            Self::Network => "[N]",
            Self::Storage => "[S]",
            Self::Usb => "[U]",
            Self::Input => "[I]",
            Self::System => "[Y]",
            Self::Other => "[?]",
        }
    }

    /// Color accent for this category.
    pub fn color(self) -> Color {
        match self {
            Self::Display => COLOR_BLUE,
            Self::Audio => COLOR_MAUVE,
            Self::Network => COLOR_TEAL,
            Self::Storage => COLOR_PEACH,
            Self::Usb => COLOR_SAPPHIRE,
            Self::Input => COLOR_LAVENDER,
            Self::System => COLOR_GREEN,
            Self::Other => COLOR_OVERLAY,
        }
    }

    /// All categories in display order.
    pub fn all() -> &'static [DeviceCategory] {
        &[
            Self::Display,
            Self::Audio,
            Self::Network,
            Self::Storage,
            Self::Usb,
            Self::Input,
            Self::System,
            Self::Other,
        ]
    }
}

// ============================================================================
// Device status
// ============================================================================

/// Operating status of a hardware device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceStatus {
    /// Device is functioning correctly.
    Working,
    /// Device has a non-critical issue.
    Warning,
    /// Device has a critical error.
    Error,
    /// Device is administratively disabled.
    Disabled,
    /// Device status cannot be determined.
    Unknown,
}

impl DeviceStatus {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Disabled => "Disabled",
            Self::Unknown => "Unknown",
        }
    }

    /// Color for this status indicator.
    pub fn color(self) -> Color {
        match self {
            Self::Working => COLOR_GREEN,
            Self::Warning => COLOR_YELLOW,
            Self::Error => COLOR_RED,
            Self::Disabled => COLOR_OVERLAY,
            Self::Unknown => COLOR_SUBTEXT,
        }
    }

    /// Status icon marker.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Working => "+",
            Self::Warning => "!",
            Self::Error => "X",
            Self::Disabled => "-",
            Self::Unknown => "?",
        }
    }

    /// Whether this status indicates a problem.
    pub fn is_problem(self) -> bool {
        matches!(self, Self::Warning | Self::Error)
    }
}

// ============================================================================
// Device event types
// ============================================================================

/// Type of device event in the history log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEventKind {
    Connected,
    Disconnected,
    Error,
    DriverLoaded,
    DriverUnloaded,
    Enabled,
    Disabled,
    Reset,
}

impl DeviceEventKind {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Error => "Error",
            Self::DriverLoaded => "Driver Loaded",
            Self::DriverUnloaded => "Driver Unloaded",
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
            Self::Reset => "Reset",
        }
    }

    /// Color for this event kind.
    pub fn color(self) -> Color {
        match self {
            Self::Connected | Self::DriverLoaded | Self::Enabled => COLOR_GREEN,
            Self::Disconnected | Self::DriverUnloaded | Self::Disabled => COLOR_OVERLAY,
            Self::Error => COLOR_RED,
            Self::Reset => COLOR_YELLOW,
        }
    }
}

// ============================================================================
// Device event
// ============================================================================

/// A recorded event in device history.
#[derive(Clone, Debug)]
pub struct DeviceEvent {
    /// Unique device ID this event pertains to.
    pub device_id: u32,
    /// Kind of event.
    pub kind: DeviceEventKind,
    /// Timestamp as a formatted string (e.g. "2026-05-18 14:32:01").
    pub timestamp: String,
    /// Optional detail message.
    pub detail: String,
}

impl DeviceEvent {
    /// Create a new event.
    pub fn new(device_id: u32, kind: DeviceEventKind, timestamp: &str, detail: &str) -> Self {
        Self {
            device_id,
            kind,
            timestamp: timestamp.to_string(),
            detail: detail.to_string(),
        }
    }
}

// ============================================================================
// Driver info
// ============================================================================

/// Information about the driver backing a device.
#[derive(Clone, Debug)]
pub struct DriverInfo {
    /// Driver name.
    pub name: String,
    /// Driver version string.
    pub version: String,
    /// Driver provider (vendor).
    pub provider: String,
    /// Driver release date string.
    pub date: String,
    /// Whether a newer version is available.
    pub update_available: bool,
}

impl DriverInfo {
    /// Create a new driver info record.
    pub fn new(
        name: &str,
        version: &str,
        provider: &str,
        date: &str,
        update_available: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            provider: provider.to_string(),
            date: date.to_string(),
            update_available,
        }
    }
}

// ============================================================================
// Resource types
// ============================================================================

/// An IRQ (interrupt request) assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrqAssignment {
    /// IRQ number.
    pub irq: u32,
    /// Device name using this IRQ.
    pub device_name: String,
    /// Whether the IRQ is shared with other devices.
    pub shared: bool,
}

/// An MMIO (Memory-Mapped I/O) range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MmioRange {
    /// Start address.
    pub start: u64,
    /// End address (inclusive).
    pub end: u64,
    /// Device name using this range.
    pub device_name: String,
}

impl MmioRange {
    /// Format the address range as a hex string.
    pub fn format_range(&self) -> String {
        format!("0x{:08X} - 0x{:08X}", self.start, self.end)
    }

    /// Size of the MMIO region in bytes.
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

/// A DMA channel assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DmaChannel {
    /// DMA channel number.
    pub channel: u32,
    /// Device name using this channel.
    pub device_name: String,
}

/// System resource view aggregating IRQs, MMIO, and DMA.
#[derive(Clone, Debug, Default)]
pub struct ResourceView {
    pub irqs: Vec<IrqAssignment>,
    pub mmio_ranges: Vec<MmioRange>,
    pub dma_channels: Vec<DmaChannel>,
}

impl ResourceView {
    /// Create from a list of devices.
    pub fn from_devices(devices: &[DeviceInfo]) -> Self {
        let mut irqs = Vec::new();
        let mut mmio_ranges = Vec::new();
        let mut dma_channels = Vec::new();

        for dev in devices {
            if let Some(irq) = dev.irq {
                irqs.push(IrqAssignment {
                    irq,
                    device_name: dev.name.clone(),
                    shared: false,
                });
            }
            if let Some((start, end)) = dev.mmio_range {
                mmio_ranges.push(MmioRange {
                    start,
                    end,
                    device_name: dev.name.clone(),
                });
            }
            if let Some(ch) = dev.dma_channel {
                dma_channels.push(DmaChannel {
                    channel: ch,
                    device_name: dev.name.clone(),
                });
            }
        }

        // Mark shared IRQs.
        let mut irq_counts: HashMap<u32, usize> = HashMap::new();
        for a in &irqs {
            *irq_counts.entry(a.irq).or_insert(0) += 1;
        }
        for a in &mut irqs {
            if irq_counts.get(&a.irq).copied().unwrap_or(0) > 1 {
                a.shared = true;
            }
        }

        irqs.sort_by_key(|a| a.irq);
        mmio_ranges.sort_by_key(|r| r.start);
        dma_channels.sort_by_key(|c| c.channel);

        Self {
            irqs,
            mmio_ranges,
            dma_channels,
        }
    }

    /// Total number of resource entries.
    pub fn total_count(&self) -> usize {
        self.irqs.len() + self.mmio_ranges.len() + self.dma_channels.len()
    }
}

// ============================================================================
// Device info
// ============================================================================

/// Complete information about a hardware device.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    /// Unique device identifier.
    pub id: u32,
    /// Device display name.
    pub name: String,
    /// Category this device belongs to.
    pub category: DeviceCategory,
    /// Current operating status.
    pub status: DeviceStatus,
    /// Hardware vendor name.
    pub vendor: String,
    /// Device type/model description.
    pub device_type: String,
    /// PCI vendor:device ID if applicable (e.g. "8086:1234").
    pub hw_id: Option<String>,
    /// IRQ number if assigned.
    pub irq: Option<u32>,
    /// MMIO address range (start, end) if assigned.
    pub mmio_range: Option<(u64, u64)>,
    /// DMA channel if assigned.
    pub dma_channel: Option<u32>,
    /// Driver information.
    pub driver: Option<DriverInfo>,
    /// Whether the device is currently enabled.
    pub enabled: bool,
    /// Location path in the device tree (e.g. "PCI Bus 0, Device 2, Function 0").
    pub location: String,
    /// Status detail message (e.g. error description).
    pub status_detail: String,
}

impl DeviceInfo {
    /// Whether this device has any problem.
    pub fn has_problem(&self) -> bool {
        self.status.is_problem()
    }

    /// Whether a driver update is available.
    pub fn has_driver_update(&self) -> bool {
        self.driver
            .as_ref()
            .is_some_and(|d| d.update_available)
    }

    /// Format the MMIO range for display.
    pub fn format_mmio(&self) -> String {
        match self.mmio_range {
            Some((start, end)) => format!("0x{start:08X} - 0x{end:08X}"),
            None => "N/A".to_string(),
        }
    }

    /// Format the IRQ for display.
    pub fn format_irq(&self) -> String {
        match self.irq {
            Some(irq) => format!("IRQ {irq}"),
            None => "N/A".to_string(),
        }
    }
}

// ============================================================================
// Driver update check model
// ============================================================================

/// Status of a driver update check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateCheckStatus {
    /// Not yet checked.
    NotChecked,
    /// Check in progress.
    Checking,
    /// Up to date.
    UpToDate,
    /// Update available.
    UpdateAvailable,
    /// Check failed.
    Failed,
}

impl UpdateCheckStatus {
    /// Label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotChecked => "Not Checked",
            Self::Checking => "Checking...",
            Self::UpToDate => "Up to Date",
            Self::UpdateAvailable => "Update Available",
            Self::Failed => "Check Failed",
        }
    }

    /// Display color.
    pub fn color(self) -> Color {
        match self {
            Self::NotChecked => COLOR_SUBTEXT,
            Self::Checking => COLOR_BLUE,
            Self::UpToDate => COLOR_GREEN,
            Self::UpdateAvailable => COLOR_YELLOW,
            Self::Failed => COLOR_RED,
        }
    }
}

/// Result of checking for driver updates for a specific device.
#[derive(Clone, Debug)]
pub struct DriverUpdateCheck {
    /// Device ID.
    pub device_id: u32,
    /// Current check status.
    pub status: UpdateCheckStatus,
    /// Available version (if any).
    pub available_version: Option<String>,
    /// Last checked timestamp.
    pub last_checked: Option<String>,
}

impl DriverUpdateCheck {
    /// Create a new unchecked entry.
    pub fn new(device_id: u32) -> Self {
        Self {
            device_id,
            status: UpdateCheckStatus::NotChecked,
            available_version: None,
            last_checked: None,
        }
    }

    /// Mark as checking.
    pub fn start_check(&mut self) {
        self.status = UpdateCheckStatus::Checking;
    }

    /// Complete the check with a result.
    pub fn finish_check(&mut self, available: Option<String>, timestamp: &str) {
        self.last_checked = Some(timestamp.to_string());
        if let Some(ver) = available {
            self.status = UpdateCheckStatus::UpdateAvailable;
            self.available_version = Some(ver);
        } else {
            self.status = UpdateCheckStatus::UpToDate;
            self.available_version = None;
        }
    }

    /// Mark the check as failed.
    pub fn fail_check(&mut self, timestamp: &str) {
        self.status = UpdateCheckStatus::Failed;
        self.last_checked = Some(timestamp.to_string());
    }
}

// ============================================================================
// Tree node (for the sidebar)
// ============================================================================

/// A node in the device tree sidebar.
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Category this node represents (for category-level nodes).
    pub category: Option<DeviceCategory>,
    /// Device ID (for device-level nodes).
    pub device_id: Option<u32>,
    /// Display label.
    pub label: String,
    /// Tree depth (0 = root category, 1 = device).
    pub depth: u32,
    /// Whether this node is expanded (category nodes only).
    pub expanded: bool,
    /// Whether this node is visible (after filtering).
    pub visible: bool,
}

impl TreeNode {
    /// Create a category node.
    pub fn category(cat: DeviceCategory) -> Self {
        Self {
            category: Some(cat),
            device_id: None,
            label: cat.label().to_string(),
            depth: 0,
            expanded: true,
            visible: true,
        }
    }

    /// Create a device node.
    pub fn device(id: u32, name: &str) -> Self {
        Self {
            category: None,
            device_id: Some(id),
            label: name.to_string(),
            depth: 1,
            expanded: false,
            visible: true,
        }
    }
}

// ============================================================================
// Properties panel tab
// ============================================================================

/// Tab in the properties/detail panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertiesTab {
    /// General device properties.
    General,
    /// Driver information.
    Driver,
    /// Hardware resources (IRQ, MMIO, DMA).
    Resources,
    /// Event history for this device.
    Events,
}

impl PropertiesTab {
    /// Label for the tab.
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Driver => "Driver",
            Self::Resources => "Resources",
            Self::Events => "Events",
        }
    }

    /// All tabs in order.
    pub fn all() -> &'static [PropertiesTab] {
        &[Self::General, Self::Driver, Self::Resources, Self::Events]
    }
}

// ============================================================================
// Toolbar action
// ============================================================================

/// Action button in the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Scan,
    Properties,
    Enable,
    Disable,
    Uninstall,
    Export,
}

impl ToolbarAction {
    /// Label for the toolbar button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Scan => "Scan",
            Self::Properties => "Properties",
            Self::Enable => "Enable",
            Self::Disable => "Disable",
            Self::Uninstall => "Uninstall",
            Self::Export => "Export",
        }
    }

    /// All toolbar actions in display order.
    pub fn all() -> &'static [ToolbarAction] {
        &[
            Self::Scan,
            Self::Properties,
            Self::Enable,
            Self::Disable,
            Self::Uninstall,
            Self::Export,
        ]
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// Top-level state for the device manager application.
pub struct DeviceManagerState {
    /// Window width.
    pub width: f32,
    /// Window height.
    pub height: f32,
    /// All known devices.
    pub devices: Vec<DeviceInfo>,
    /// Flattened tree nodes for the sidebar.
    pub tree_nodes: Vec<TreeNode>,
    /// Index of the selected tree node.
    pub selected_tree_index: Option<usize>,
    /// Currently active properties tab.
    pub active_tab: PropertiesTab,
    /// Search/filter query string.
    pub search_query: String,
    /// Whether the search bar is focused.
    pub search_focused: bool,
    /// Event history log.
    pub event_history: Vec<DeviceEvent>,
    /// Resource view (computed from devices).
    pub resource_view: ResourceView,
    /// Driver update check results, keyed by device ID.
    pub update_checks: HashMap<u32, DriverUpdateCheck>,
    /// Scroll offset for the tree sidebar.
    pub tree_scroll: f32,
    /// Scroll offset for the properties panel.
    pub properties_scroll: f32,
    /// Hovered tree node index.
    pub hovered_tree_index: Option<usize>,
    /// Hovered toolbar action index.
    pub hovered_toolbar_action: Option<usize>,
    /// Whether we are showing the resource view (instead of per-device).
    pub show_resource_view: bool,
    /// Hovered properties tab index.
    pub hovered_tab_index: Option<usize>,
}

impl DeviceManagerState {
    /// Create a new device manager with sample data.
    pub fn new() -> Self {
        let devices = sample_devices();
        let resource_view = ResourceView::from_devices(&devices);
        let tree_nodes = build_tree_nodes(&devices);
        let mut update_checks = HashMap::new();
        for dev in &devices {
            update_checks.insert(dev.id, DriverUpdateCheck::new(dev.id));
        }

        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            devices,
            tree_nodes,
            selected_tree_index: None,
            active_tab: PropertiesTab::General,
            search_query: String::new(),
            search_focused: false,
            event_history: sample_events(),
            resource_view,
            update_checks,
            tree_scroll: 0.0,
            properties_scroll: 0.0,
            hovered_tree_index: None,
            hovered_toolbar_action: None,
            show_resource_view: false,
            hovered_tab_index: None,
        }
    }

    /// Get the currently selected device, if any.
    pub fn selected_device(&self) -> Option<&DeviceInfo> {
        let idx = self.selected_tree_index?;
        let node = self.tree_nodes.get(idx)?;
        let dev_id = node.device_id?;
        self.devices.iter().find(|d| d.id == dev_id)
    }

    /// Get the category of the selected tree node.
    pub fn selected_category(&self) -> Option<DeviceCategory> {
        let idx = self.selected_tree_index?;
        let node = self.tree_nodes.get(idx)?;
        node.category
    }

    /// Count devices with problems.
    pub fn problem_device_count(&self) -> usize {
        self.devices.iter().filter(|d| d.has_problem()).count()
    }

    /// Count devices with driver updates available.
    pub fn update_available_count(&self) -> usize {
        self.devices.iter().filter(|d| d.has_driver_update()).count()
    }

    /// Count total enabled devices.
    pub fn enabled_device_count(&self) -> usize {
        self.devices.iter().filter(|d| d.enabled).count()
    }

    /// Count devices matching the search query.
    pub fn matching_device_count(&self) -> usize {
        if self.search_query.is_empty() {
            return self.devices.len();
        }
        let q = self.search_query.to_lowercase();
        self.devices
            .iter()
            .filter(|d| device_matches_query(d, &q))
            .count()
    }

    /// Toggle the expanded state of a category node.
    pub fn toggle_category(&mut self, index: usize) {
        if let Some(node) = self.tree_nodes.get_mut(index)
            && node.category.is_some() {
                node.expanded = !node.expanded;
            }
    }

    /// Select a tree node by index.
    pub fn select_tree_node(&mut self, index: usize) {
        if index < self.tree_nodes.len() {
            self.selected_tree_index = Some(index);
            self.properties_scroll = 0.0;
            // If selecting a category, show resource view mode.
            if let Some(node) = self.tree_nodes.get(index) {
                self.show_resource_view = node.category.is_some();
            }
        }
    }

    /// Apply search filter to tree nodes.
    pub fn apply_search_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        for node in &mut self.tree_nodes {
            if q.is_empty() {
                node.visible = true;
            } else if node.category.is_some() {
                // Category nodes: visible if any child matches.
                let cat = node.category.expect("checked above");
                node.visible = self.devices.iter().any(|d| {
                    d.category == cat && device_matches_query(d, &q)
                });
            } else if let Some(dev_id) = node.device_id {
                node.visible = self.devices.iter().any(|d| {
                    d.id == dev_id && device_matches_query(d, &q)
                });
            }
        }
    }

    /// Enable or disable a device by ID.
    pub fn set_device_enabled(&mut self, device_id: u32, enabled: bool) {
        if let Some(dev) = self.devices.iter_mut().find(|d| d.id == device_id) {
            dev.enabled = enabled;
            if enabled {
                if dev.status == DeviceStatus::Disabled {
                    dev.status = DeviceStatus::Working;
                }
            } else {
                dev.status = DeviceStatus::Disabled;
            }
        }
    }

    /// Uninstall the driver for a device by ID.
    pub fn uninstall_driver(&mut self, device_id: u32) {
        if let Some(dev) = self.devices.iter_mut().find(|d| d.id == device_id) {
            dev.driver = None;
            dev.status = DeviceStatus::Warning;
            dev.status_detail = "Driver uninstalled".to_string();
        }
    }

    /// Simulate scanning for hardware changes (re-builds tree).
    pub fn scan_hardware(&mut self) {
        self.resource_view = ResourceView::from_devices(&self.devices);
        self.tree_nodes = build_tree_nodes(&self.devices);
        self.apply_search_filter();
    }

    /// Add an event to the history.
    pub fn add_event(&mut self, event: DeviceEvent) {
        self.event_history.push(event);
        if self.event_history.len() > MAX_EVENT_HISTORY {
            self.event_history.remove(0);
        }
    }

    /// Get events for a specific device.
    pub fn events_for_device(&self, device_id: u32) -> Vec<&DeviceEvent> {
        self.event_history
            .iter()
            .filter(|e| e.device_id == device_id)
            .collect()
    }

    /// Generate a hardware report as text.
    pub fn export_report(&self) -> String {
        let mut report = String::new();
        report.push_str("=== Slate OS Hardware Report ===\n\n");
        report.push_str(&format!("Total Devices: {}\n", self.devices.len()));
        report.push_str(&format!(
            "Problem Devices: {}\n",
            self.problem_device_count()
        ));
        report.push_str(&format!(
            "Enabled: {}\n\n",
            self.enabled_device_count()
        ));

        for cat in DeviceCategory::all() {
            let cat_devices: Vec<&DeviceInfo> =
                self.devices.iter().filter(|d| d.category == *cat).collect();
            if cat_devices.is_empty() {
                continue;
            }
            report.push_str(&format!("--- {} ---\n", cat.label()));
            for dev in &cat_devices {
                report.push_str(&format!(
                    "  {} [{}] ({})\n",
                    dev.name,
                    dev.status.label(),
                    dev.vendor,
                ));
                report.push_str(&format!("    Type: {}\n", dev.device_type));
                if let Some(ref hw_id) = dev.hw_id {
                    report.push_str(&format!("    HW ID: {hw_id}\n"));
                }
                report.push_str(&format!("    IRQ: {}\n", dev.format_irq()));
                report.push_str(&format!("    MMIO: {}\n", dev.format_mmio()));
                report.push_str(&format!(
                    "    Location: {}\n",
                    dev.location,
                ));
                if let Some(ref drv) = dev.driver {
                    report.push_str(&format!(
                        "    Driver: {} v{} ({})\n",
                        drv.name, drv.version, drv.provider,
                    ));
                }
                report.push('\n');
            }
        }

        // Resources section
        report.push_str("--- IRQ Assignments ---\n");
        for irq in &self.resource_view.irqs {
            report.push_str(&format!(
                "  IRQ {:>3}: {} {}\n",
                irq.irq,
                irq.device_name,
                if irq.shared { "(shared)" } else { "" },
            ));
        }
        report.push('\n');

        report.push_str("--- MMIO Ranges ---\n");
        for mmio in &self.resource_view.mmio_ranges {
            report.push_str(&format!(
                "  {} : {}\n",
                mmio.format_range(),
                mmio.device_name,
            ));
        }
        report.push('\n');

        report.push_str("--- DMA Channels ---\n");
        for dma in &self.resource_view.dma_channels {
            report.push_str(&format!(
                "  Channel {:>2}: {}\n",
                dma.channel, dma.device_name,
            ));
        }

        report
    }

    /// Process a toolbar action.
    pub fn handle_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::Scan => self.scan_hardware(),
            ToolbarAction::Properties => {
                // Switch to general tab if a device is selected.
                if self.selected_device().is_some() {
                    self.active_tab = PropertiesTab::General;
                    self.show_resource_view = false;
                }
            }
            ToolbarAction::Enable => {
                if let Some(dev) = self.selected_device() {
                    let id = dev.id;
                    self.set_device_enabled(id, true);
                }
            }
            ToolbarAction::Disable => {
                if let Some(dev) = self.selected_device() {
                    let id = dev.id;
                    self.set_device_enabled(id, false);
                }
            }
            ToolbarAction::Uninstall => {
                if let Some(dev) = self.selected_device() {
                    let id = dev.id;
                    self.uninstall_driver(id);
                }
            }
            ToolbarAction::Export => {
                let _report = self.export_report();
                // In a real app, we would save to a file or show in a dialog.
            }
        }
    }
}

impl Default for DeviceManagerState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper: check if a device matches a search query
// ============================================================================

/// Check if a device matches the given lowercase query string.
fn device_matches_query(dev: &DeviceInfo, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    dev.name.to_lowercase().contains(query)
        || dev.vendor.to_lowercase().contains(query)
        || dev.device_type.to_lowercase().contains(query)
        || dev.category.label().to_lowercase().contains(query)
        || dev.status.label().to_lowercase().contains(query)
        || dev.hw_id.as_deref().unwrap_or("").to_lowercase().contains(query)
        || dev.location.to_lowercase().contains(query)
        || dev
            .driver
            .as_ref()
            .is_some_and(|d| d.name.to_lowercase().contains(query))
}

// ============================================================================
// Build tree nodes
// ============================================================================

/// Build the flattened tree node list from a set of devices.
fn build_tree_nodes(devices: &[DeviceInfo]) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    for cat in DeviceCategory::all() {
        let cat_devices: Vec<&DeviceInfo> =
            devices.iter().filter(|d| d.category == *cat).collect();
        if cat_devices.is_empty() {
            continue;
        }
        nodes.push(TreeNode::category(*cat));
        for dev in &cat_devices {
            nodes.push(TreeNode::device(dev.id, &dev.name));
        }
    }
    nodes
}

// ============================================================================
// Sample data
// ============================================================================

/// Generate sample device data for development/testing.
fn sample_devices() -> Vec<DeviceInfo> {
    vec![
        DeviceInfo {
            id: 1,
            name: "Virtio GPU Display".to_string(),
            category: DeviceCategory::Display,
            status: DeviceStatus::Working,
            vendor: "Red Hat".to_string(),
            device_type: "VGA Compatible Controller".to_string(),
            hw_id: Some("1AF4:1050".to_string()),
            irq: Some(11),
            mmio_range: Some((0xFD00_0000, 0xFDFF_FFFF)),
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "virtio-gpu",
                "1.2.0",
                "Slate OS Project",
                "2026-04-01",
                false,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 2, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 2,
            name: "Intel HD Audio Controller".to_string(),
            category: DeviceCategory::Audio,
            status: DeviceStatus::Working,
            vendor: "Intel".to_string(),
            device_type: "Audio Controller".to_string(),
            hw_id: Some("8086:2668".to_string()),
            irq: Some(5),
            mmio_range: Some((0xFE80_0000, 0xFE80_3FFF)),
            dma_channel: Some(1),
            driver: Some(DriverInfo::new(
                "hda-intel",
                "3.1.4",
                "Slate OS Project",
                "2026-03-15",
                true,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 27, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 3,
            name: "Virtio Network Adapter".to_string(),
            category: DeviceCategory::Network,
            status: DeviceStatus::Working,
            vendor: "Red Hat".to_string(),
            device_type: "Ethernet Controller".to_string(),
            hw_id: Some("1AF4:1000".to_string()),
            irq: Some(10),
            mmio_range: Some((0xFE00_0000, 0xFE00_0FFF)),
            dma_channel: Some(2),
            driver: Some(DriverInfo::new(
                "virtio-net",
                "2.0.1",
                "Slate OS Project",
                "2026-04-10",
                false,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 3, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 4,
            name: "Virtio Block Storage".to_string(),
            category: DeviceCategory::Storage,
            status: DeviceStatus::Working,
            vendor: "Red Hat".to_string(),
            device_type: "SCSI Storage Controller".to_string(),
            hw_id: Some("1AF4:1001".to_string()),
            irq: Some(9),
            mmio_range: Some((0xFE01_0000, 0xFE01_0FFF)),
            dma_channel: Some(3),
            driver: Some(DriverInfo::new(
                "virtio-blk",
                "1.5.0",
                "Slate OS Project",
                "2026-02-20",
                false,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 4, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 5,
            name: "XHCI USB 3.0 Host Controller".to_string(),
            category: DeviceCategory::Usb,
            status: DeviceStatus::Working,
            vendor: "Intel".to_string(),
            device_type: "USB Controller".to_string(),
            hw_id: Some("8086:A12F".to_string()),
            irq: Some(16),
            mmio_range: Some((0xFE20_0000, 0xFE20_FFFF)),
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "xhci-hcd",
                "1.0.3",
                "Slate OS Project",
                "2026-01-10",
                false,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 20, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 6,
            name: "PS/2 Keyboard".to_string(),
            category: DeviceCategory::Input,
            status: DeviceStatus::Working,
            vendor: "Generic".to_string(),
            device_type: "Keyboard".to_string(),
            hw_id: None,
            irq: Some(1),
            mmio_range: None,
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "i8042-kbd",
                "1.0.0",
                "Slate OS Project",
                "2025-12-01",
                false,
            )),
            enabled: true,
            location: "ISA, Port 0x60".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 7,
            name: "PS/2 Mouse".to_string(),
            category: DeviceCategory::Input,
            status: DeviceStatus::Working,
            vendor: "Generic".to_string(),
            device_type: "Mouse".to_string(),
            hw_id: None,
            irq: Some(12),
            mmio_range: None,
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "i8042-mouse",
                "1.0.0",
                "Slate OS Project",
                "2025-12-01",
                false,
            )),
            enabled: true,
            location: "ISA, Port 0x60".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 8,
            name: "ACPI Power Management".to_string(),
            category: DeviceCategory::System,
            status: DeviceStatus::Working,
            vendor: "ACPI".to_string(),
            device_type: "System Device".to_string(),
            hw_id: Some("PNP0C0A".to_string()),
            irq: Some(9),
            mmio_range: None,
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "acpi-pm",
                "1.0.0",
                "Slate OS Project",
                "2025-11-15",
                false,
            )),
            enabled: true,
            location: "ACPI".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 9,
            name: "PCI-to-ISA Bridge".to_string(),
            category: DeviceCategory::System,
            status: DeviceStatus::Working,
            vendor: "Intel".to_string(),
            device_type: "ISA Bridge".to_string(),
            hw_id: Some("8086:7000".to_string()),
            irq: None,
            mmio_range: None,
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "piix3-isa",
                "1.0.0",
                "Slate OS Project",
                "2025-11-15",
                false,
            )),
            enabled: true,
            location: "PCI Bus 0, Device 1, Function 0".to_string(),
            status_detail: "Device is functioning correctly".to_string(),
        },
        DeviceInfo {
            id: 10,
            name: "Unknown PCI Device".to_string(),
            category: DeviceCategory::Other,
            status: DeviceStatus::Warning,
            vendor: "Unknown".to_string(),
            device_type: "PCI Device".to_string(),
            hw_id: Some("DEAD:BEEF".to_string()),
            irq: Some(11),
            mmio_range: Some((0xFE30_0000, 0xFE30_0FFF)),
            dma_channel: None,
            driver: None,
            enabled: true,
            location: "PCI Bus 0, Device 31, Function 0".to_string(),
            status_detail: "No compatible driver found".to_string(),
        },
        DeviceInfo {
            id: 11,
            name: "Realtek RTL8139 (Broken)".to_string(),
            category: DeviceCategory::Network,
            status: DeviceStatus::Error,
            vendor: "Realtek".to_string(),
            device_type: "Ethernet Controller".to_string(),
            hw_id: Some("10EC:8139".to_string()),
            irq: Some(10),
            mmio_range: Some((0xFE40_0000, 0xFE40_00FF)),
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "rtl8139",
                "0.9.0",
                "Community",
                "2025-06-01",
                true,
            )),
            enabled: false,
            location: "PCI Bus 0, Device 5, Function 0".to_string(),
            status_detail: "Device reported a hardware error (code 10)".to_string(),
        },
        DeviceInfo {
            id: 12,
            name: "USB Mass Storage".to_string(),
            category: DeviceCategory::Usb,
            status: DeviceStatus::Disabled,
            vendor: "SanDisk".to_string(),
            device_type: "USB Storage Device".to_string(),
            hw_id: Some("0781:5567".to_string()),
            irq: None,
            mmio_range: None,
            dma_channel: None,
            driver: Some(DriverInfo::new(
                "usb-storage",
                "1.1.0",
                "Slate OS Project",
                "2026-01-20",
                false,
            )),
            enabled: false,
            location: "USB Bus 1, Port 2".to_string(),
            status_detail: "Device is disabled by administrator".to_string(),
        },
    ]
}

/// Generate sample event history for development/testing.
fn sample_events() -> Vec<DeviceEvent> {
    vec![
        DeviceEvent::new(1, DeviceEventKind::Connected, "2026-05-18 08:00:12", "Virtio GPU enumerated at boot"),
        DeviceEvent::new(1, DeviceEventKind::DriverLoaded, "2026-05-18 08:00:13", "virtio-gpu v1.2.0 loaded"),
        DeviceEvent::new(3, DeviceEventKind::Connected, "2026-05-18 08:00:14", "Virtio NIC enumerated"),
        DeviceEvent::new(3, DeviceEventKind::DriverLoaded, "2026-05-18 08:00:15", "virtio-net v2.0.1 loaded"),
        DeviceEvent::new(11, DeviceEventKind::Connected, "2026-05-18 08:00:16", "RTL8139 detected"),
        DeviceEvent::new(11, DeviceEventKind::Error, "2026-05-18 08:00:17", "Hardware error code 10"),
        DeviceEvent::new(11, DeviceEventKind::Disabled, "2026-05-18 08:01:00", "Disabled by user"),
        DeviceEvent::new(12, DeviceEventKind::Connected, "2026-05-18 09:15:30", "USB device plugged in"),
        DeviceEvent::new(12, DeviceEventKind::DriverLoaded, "2026-05-18 09:15:31", "usb-storage v1.1.0 loaded"),
        DeviceEvent::new(12, DeviceEventKind::Disabled, "2026-05-18 09:20:00", "Disabled by administrator"),
        DeviceEvent::new(10, DeviceEventKind::Connected, "2026-05-18 08:00:18", "Unknown PCI device found"),
    ]
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the complete device manager UI into render commands.
pub fn render(state: &DeviceManagerState) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    // Background
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: state.width,
        height: state.height,
        color: COLOR_BASE,
        corner_radii: CornerRadii::ZERO,
    });

    render_title_bar(state, &mut cmds);
    render_toolbar(state, &mut cmds);
    render_search_bar(state, &mut cmds);
    render_sidebar(state, &mut cmds);
    render_properties_panel(state, &mut cmds);
    render_status_bar(state, &mut cmds);

    cmds
}

/// Render the title bar.
fn render_title_bar(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: state.width,
        height: TITLE_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    cmds.push(RenderCommand::Text {
        x: 12.0,
        y: 10.0,
        text: "Device Manager".to_string(),
        font_size: 15.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Problem count badge
    let problems = state.problem_device_count();
    if problems > 0 {
        let badge_text = format!("{problems} problem(s)");
        let badge_x = 160.0;
        cmds.push(RenderCommand::FillRect {
            x: badge_x,
            y: 8.0,
            width: 100.0,
            height: 20.0,
            color: COLOR_RED,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: badge_x + 8.0,
            y: 11.0,
            text: badge_text,
            font_size: 11.0,
            color: COLOR_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(84.0),
        });
    }

    // Separator line
    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: TITLE_BAR_HEIGHT,
        x2: state.width,
        y2: TITLE_BAR_HEIGHT,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

/// Render the toolbar with action buttons.
fn render_toolbar(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    let y = TITLE_BAR_HEIGHT;

    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: state.width,
        height: TOOLBAR_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let actions = ToolbarAction::all();
    for (i, action) in actions.iter().enumerate() {
        let btn_x = 8.0 + i as f32 * (TOOLBAR_BTN_WIDTH + 6.0);
        let btn_y = y + (TOOLBAR_HEIGHT - TOOLBAR_BTN_HEIGHT) / 2.0;

        let is_hovered = state.hovered_toolbar_action == Some(i);
        let bg = if is_hovered { COLOR_SURFACE2 } else { COLOR_SURFACE1 };

        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: TOOLBAR_BTN_WIDTH,
            height: TOOLBAR_BTN_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: btn_x + 8.0,
            y: btn_y + 6.0,
            text: action.label().to_string(),
            font_size: 12.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(TOOLBAR_BTN_WIDTH - 16.0),
        });
    }

    // Separator line
    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: y + TOOLBAR_HEIGHT,
        x2: state.width,
        y2: y + TOOLBAR_HEIGHT,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

/// Render the search bar below the toolbar.
fn render_search_bar(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    let y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;

    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: SIDEBAR_WIDTH,
        height: SEARCH_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Search input background
    let input_x = 8.0;
    let input_y = y + 4.0;
    let input_w = SIDEBAR_WIDTH - 16.0;
    let input_h = SEARCH_BAR_HEIGHT - 8.0;

    let border_color = if state.search_focused {
        COLOR_BLUE
    } else {
        COLOR_SURFACE2
    };

    cmds.push(RenderCommand::FillRect {
        x: input_x,
        y: input_y,
        width: input_w,
        height: input_h,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::all(3.0),
    });
    cmds.push(RenderCommand::StrokeRect {
        x: input_x,
        y: input_y,
        width: input_w,
        height: input_h,
        color: border_color,
        line_width: 1.0,
        corner_radii: CornerRadii::all(3.0),
    });

    let display_text = if state.search_query.is_empty() {
        "Search devices...".to_string()
    } else {
        state.search_query.clone()
    };
    let text_color = if state.search_query.is_empty() {
        COLOR_OVERLAY
    } else {
        COLOR_TEXT
    };

    cmds.push(RenderCommand::Text {
        x: input_x + 8.0,
        y: input_y + 4.0,
        text: display_text,
        font_size: 11.0,
        color: text_color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(input_w - 16.0),
    });
}

/// Render the device tree sidebar.
fn render_sidebar(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
    let bottom = state.height - STATUS_BAR_HEIGHT;
    let sidebar_height = bottom - top;

    // Sidebar background
    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y: top,
        width: SIDEBAR_WIDTH,
        height: sidebar_height,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Clip to sidebar area
    cmds.push(RenderCommand::PushClip {
        x: 0.0,
        y: top,
        width: SIDEBAR_WIDTH,
        height: sidebar_height,
    });

    let mut y_offset = top - state.tree_scroll;
    let mut parent_expanded = true;

    for (i, node) in state.tree_nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }

        // Device nodes are only shown if their parent category is expanded.
        if node.depth == 1 && !parent_expanded {
            continue;
        }

        if node.depth == 0 {
            parent_expanded = node.expanded;
        }

        // Only render visible rows.
        if y_offset + TREE_ROW_HEIGHT > top && y_offset < bottom {
            let is_selected = state.selected_tree_index == Some(i);
            let is_hovered = state.hovered_tree_index == Some(i);

            // Row background
            let row_bg = if is_selected {
                COLOR_SURFACE1
            } else if is_hovered {
                COLOR_SURFACE0
            } else {
                Color::TRANSPARENT
            };

            if row_bg != Color::TRANSPARENT {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: y_offset,
                    width: SIDEBAR_WIDTH,
                    height: TREE_ROW_HEIGHT,
                    color: row_bg,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Selection indicator
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: y_offset,
                    width: 3.0,
                    height: TREE_ROW_HEIGHT,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            let indent = node.depth as f32 * TREE_INDENT + 8.0;

            if let Some(cat) = node.category {
                // Category node: show expand/collapse arrow and category icon
                let arrow = if node.expanded { "v" } else { ">" };
                cmds.push(RenderCommand::Text {
                    x: indent,
                    y: y_offset + 5.0,
                    text: arrow.to_string(),
                    font_size: 11.0,
                    color: COLOR_OVERLAY,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });


                cmds.push(RenderCommand::Text {
                    x: indent + 14.0,
                    y: y_offset + 5.0,
                    text: cat.icon().to_string(),
                    font_size: 11.0,
                    color: cat.color(),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                cmds.push(RenderCommand::Text {
                    x: indent + 40.0,
                    y: y_offset + 5.0,
                    text: node.label.clone(),
                    font_size: 12.0,
                    color: COLOR_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(SIDEBAR_WIDTH - indent - 48.0),
                });
            } else if let Some(dev_id) = node.device_id {
                // Device node: show status icon and name
                let dev = state.devices.iter().find(|d| d.id == dev_id);
                if let Some(dev) = dev {
                    // Status icon
                    cmds.push(RenderCommand::Text {
                        x: indent + 4.0,
                        y: y_offset + 5.0,
                        text: dev.status.icon().to_string(),
                        font_size: 11.0,
                        color: dev.status.color(),
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });

                    // Device name
                    let name_color = if dev.has_problem() {
                        dev.status.color()
                    } else if !dev.enabled {
                        COLOR_OVERLAY
                    } else {
                        COLOR_SUBTEXT
                    };

                    cmds.push(RenderCommand::Text {
                        x: indent + 18.0,
                        y: y_offset + 5.0,
                        text: node.label.clone(),
                        font_size: 11.0,
                        color: name_color,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - indent - 26.0),
                    });

                    // Driver update indicator
                    if dev.has_driver_update() {
                        cmds.push(RenderCommand::FillRect {
                            x: SIDEBAR_WIDTH - 16.0,
                            y: y_offset + 8.0,
                            width: 8.0,
                            height: 8.0,
                            color: COLOR_YELLOW,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }
                }
            }
        }

        y_offset += TREE_ROW_HEIGHT;
    }

    cmds.push(RenderCommand::PopClip);

    // Vertical separator between sidebar and properties panel
    cmds.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: top,
        x2: SIDEBAR_WIDTH,
        y2: bottom,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
}

/// Render the properties panel (right side).
fn render_properties_panel(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    let top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
    let bottom = state.height - STATUS_BAR_HEIGHT;
    let panel_x = SIDEBAR_WIDTH + 1.0;
    let panel_width = state.width - panel_x;
    let panel_height = bottom - top;

    // Panel background
    cmds.push(RenderCommand::FillRect {
        x: panel_x,
        y: top,
        width: panel_width,
        height: panel_height,
        color: COLOR_BASE,
        corner_radii: CornerRadii::ZERO,
    });

    if state.show_resource_view {
        render_resource_view(state, cmds, panel_x, top, panel_width, panel_height);
        return;
    }

    let dev = match state.selected_device() {
        Some(d) => d,
        None => {
            // No device selected: show placeholder
            cmds.push(RenderCommand::Text {
                x: panel_x + 20.0,
                y: top + panel_height / 2.0 - 10.0,
                text: "Select a device to view its properties".to_string(),
                font_size: 14.0,
                color: COLOR_OVERLAY,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 40.0),
            });
            return;
        }
    };

    // Tab bar
    render_tab_bar(state, cmds, panel_x, top, panel_width);

    let content_top = top + TAB_BAR_HEIGHT;
    let content_height = panel_height - TAB_BAR_HEIGHT;

    // Clip to content area
    cmds.push(RenderCommand::PushClip {
        x: panel_x,
        y: content_top,
        width: panel_width,
        height: content_height,
    });

    match state.active_tab {
        PropertiesTab::General => {
            render_general_tab(state, dev, cmds, panel_x, content_top, panel_width);
        }
        PropertiesTab::Driver => {
            render_driver_tab(state, dev, cmds, panel_x, content_top, panel_width);
        }
        PropertiesTab::Resources => {
            render_resources_tab(dev, cmds, panel_x, content_top, panel_width);
        }
        PropertiesTab::Events => {
            render_events_tab(state, dev, cmds, panel_x, content_top, panel_width);
        }
    }

    cmds.push(RenderCommand::PopClip);
}

/// Render the tab bar for the properties panel.
fn render_tab_bar(
    state: &DeviceManagerState,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) {
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: TAB_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    let tabs = PropertiesTab::all();
    let tab_width = width / tabs.len() as f32;

    for (i, tab) in tabs.iter().enumerate() {
        let tab_x = x + i as f32 * tab_width;
        let is_active = *tab == state.active_tab;
        let is_hovered = state.hovered_tab_index == Some(i);

        if is_active {
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: y + TAB_BAR_HEIGHT - 2.0,
                width: tab_width,
                height: 2.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        } else if is_hovered {
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y,
                width: tab_width,
                height: TAB_BAR_HEIGHT,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        let text_color = if is_active {
            COLOR_BLUE
        } else {
            COLOR_SUBTEXT
        };

        cmds.push(RenderCommand::Text {
            x: tab_x + tab_width / 2.0 - 20.0,
            y: y + 7.0,
            text: tab.label().to_string(),
            font_size: 12.0,
            color: text_color,
            font_weight: if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(tab_width - 8.0),
        });
    }
}

/// Render the General properties tab for a device.
fn render_general_tab(
    _state: &DeviceManagerState,
    dev: &DeviceInfo,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) {
    let mut row_y = y + 8.0;
    let label_x = x + 16.0;
    let value_x = x + 140.0;
    let max_val_w = width - 156.0;

    // Device name header
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: dev.name.clone(),
        font_size: 15.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += 24.0;

    // Status indicator
    let status_color = dev.status.color();
    cmds.push(RenderCommand::FillRect {
        x: label_x,
        y: row_y + 2.0,
        width: 10.0,
        height: 10.0,
        color: status_color,
        corner_radii: CornerRadii::all(5.0),
    });
    cmds.push(RenderCommand::Text {
        x: label_x + 16.0,
        y: row_y,
        text: dev.status.label().to_string(),
        font_size: 12.0,
        color: status_color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 48.0),
    });
    row_y += 20.0;

    if !dev.status_detail.is_empty() {
        cmds.push(RenderCommand::Text {
            x: label_x + 16.0,
            y: row_y,
            text: dev.status_detail.clone(),
            font_size: 11.0,
            color: COLOR_SUBTEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 48.0),
        });
        row_y += 18.0;
    }

    row_y += 8.0;

    // Separator
    cmds.push(RenderCommand::Line {
        x1: label_x,
        y1: row_y,
        x2: x + width - 16.0,
        y2: row_y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
    row_y += 12.0;

    // Property rows
    let properties: Vec<(&str, String)> = vec![
        ("Type", dev.device_type.clone()),
        ("Vendor", dev.vendor.clone()),
        ("Category", dev.category.label().to_string()),
        ("HW ID", dev.hw_id.clone().unwrap_or_else(|| "N/A".to_string())),
        ("IRQ", dev.format_irq()),
        ("MMIO", dev.format_mmio()),
        ("DMA", dev.dma_channel.map_or("N/A".to_string(), |c| format!("Channel {c}"))),
        ("Location", dev.location.clone()),
        ("Enabled", if dev.enabled { "Yes" } else { "No" }.to_string()),
    ];

    for (label, value) in &properties {
        // Alternating row backgrounds
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: (*label).to_string(),
            font_size: 11.0,
            color: COLOR_OVERLAY,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });
        cmds.push(RenderCommand::Text {
            x: value_x,
            y: row_y,
            text: value.clone(),
            font_size: 11.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_val_w),
        });
        row_y += PROPERTY_ROW_HEIGHT;
    }
}

/// Render the Driver properties tab for a device.
fn render_driver_tab(
    state: &DeviceManagerState,
    dev: &DeviceInfo,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) {
    let mut row_y = y + 8.0;
    let label_x = x + 16.0;
    let value_x = x + 140.0;
    let max_val_w = width - 156.0;

    // Section header
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Driver Information".to_string(),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    match &dev.driver {
        Some(drv) => {
            let driver_props: Vec<(&str, &str)> = vec![
                ("Name", &drv.name),
                ("Version", &drv.version),
                ("Provider", &drv.provider),
                ("Date", &drv.date),
            ];

            for (label, value) in &driver_props {
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: row_y,
                    text: (*label).to_string(),
                    font_size: 11.0,
                    color: COLOR_OVERLAY,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
                cmds.push(RenderCommand::Text {
                    x: value_x,
                    y: row_y,
                    text: (*value).to_string(),
                    font_size: 11.0,
                    color: COLOR_TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(max_val_w),
                });
                row_y += PROPERTY_ROW_HEIGHT;
            }

            // Update status
            row_y += 8.0;
            cmds.push(RenderCommand::Line {
                x1: label_x,
                y1: row_y,
                x2: x + width - 16.0,
                y2: row_y,
                color: COLOR_SURFACE1,
                width: 1.0,
            });
            row_y += 12.0;

            cmds.push(RenderCommand::Text {
                x: label_x,
                y: row_y,
                text: "Update Status".to_string(),
                font_size: 13.0,
                color: COLOR_LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 32.0),
            });
            row_y += SECTION_HEADER_HEIGHT;

            if let Some(check) = state.update_checks.get(&dev.id) {
                cmds.push(RenderCommand::Text {
                    x: label_x,
                    y: row_y,
                    text: "Status".to_string(),
                    font_size: 11.0,
                    color: COLOR_OVERLAY,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(120.0),
                });
                cmds.push(RenderCommand::Text {
                    x: value_x,
                    y: row_y,
                    text: check.status.label().to_string(),
                    font_size: 11.0,
                    color: check.status.color(),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(max_val_w),
                });
                row_y += PROPERTY_ROW_HEIGHT;

                if let Some(ref ver) = check.available_version {
                    cmds.push(RenderCommand::Text {
                        x: label_x,
                        y: row_y,
                        text: "Available".to_string(),
                        font_size: 11.0,
                        color: COLOR_OVERLAY,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(120.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: value_x,
                        y: row_y,
                        text: ver.clone(),
                        font_size: 11.0,
                        color: COLOR_YELLOW,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_val_w),
                    });
                    row_y += PROPERTY_ROW_HEIGHT;
                }

                if let Some(ref ts) = check.last_checked {
                    cmds.push(RenderCommand::Text {
                        x: label_x,
                        y: row_y,
                        text: "Last Check".to_string(),
                        font_size: 11.0,
                        color: COLOR_OVERLAY,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(120.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: value_x,
                        y: row_y,
                        text: ts.clone(),
                        font_size: 11.0,
                        color: COLOR_TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(max_val_w),
                    });
                }
            }

            if drv.update_available {
                row_y += PROPERTY_ROW_HEIGHT + 8.0;
                cmds.push(RenderCommand::FillRect {
                    x: label_x,
                    y: row_y,
                    width: 140.0,
                    height: 24.0,
                    color: COLOR_YELLOW,
                    corner_radii: CornerRadii::all(4.0),
                });
                cmds.push(RenderCommand::Text {
                    x: label_x + 10.0,
                    y: row_y + 5.0,
                    text: "Update Available".to_string(),
                    font_size: 11.0,
                    color: COLOR_BASE,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                });
            }
        }
        None => {
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: row_y,
                text: "No driver installed".to_string(),
                font_size: 12.0,
                color: COLOR_OVERLAY,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
            });
        }
    }
}

/// Render the Resources tab for a device.
fn render_resources_tab(
    dev: &DeviceInfo,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) {
    let mut row_y = y + 8.0;
    let label_x = x + 16.0;
    let value_x = x + 140.0;
    let max_val_w = width - 156.0;

    // IRQ section
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Interrupt Request (IRQ)".to_string(),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "IRQ".to_string(),
        font_size: 11.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Regular,
        max_width: Some(120.0),
    });
    cmds.push(RenderCommand::Text {
        x: value_x,
        y: row_y,
        text: dev.format_irq(),
        font_size: 11.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_val_w),
    });
    row_y += PROPERTY_ROW_HEIGHT + 8.0;

    // MMIO section
    cmds.push(RenderCommand::Line {
        x1: label_x,
        y1: row_y,
        x2: x + width - 16.0,
        y2: row_y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
    row_y += 12.0;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Memory-Mapped I/O (MMIO)".to_string(),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Range".to_string(),
        font_size: 11.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Regular,
        max_width: Some(120.0),
    });
    cmds.push(RenderCommand::Text {
        x: value_x,
        y: row_y,
        text: dev.format_mmio(),
        font_size: 11.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_val_w),
    });
    row_y += PROPERTY_ROW_HEIGHT;

    if let Some((start, end)) = dev.mmio_range {
        let size = end.saturating_sub(start).saturating_add(1);
        let size_str = if size >= 1024 * 1024 {
            format!("{} MiB", size / (1024 * 1024))
        } else if size >= 1024 {
            format!("{} KiB", size / 1024)
        } else {
            format!("{size} B")
        };
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: "Size".to_string(),
            font_size: 11.0,
            color: COLOR_OVERLAY,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });
        cmds.push(RenderCommand::Text {
            x: value_x,
            y: row_y,
            text: size_str,
            font_size: 11.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_val_w),
        });
        row_y += PROPERTY_ROW_HEIGHT;
    }

    row_y += 8.0;

    // DMA section
    cmds.push(RenderCommand::Line {
        x1: label_x,
        y1: row_y,
        x2: x + width - 16.0,
        y2: row_y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
    row_y += 12.0;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "DMA Channel".to_string(),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Channel".to_string(),
        font_size: 11.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Regular,
        max_width: Some(120.0),
    });
    cmds.push(RenderCommand::Text {
        x: value_x,
        y: row_y,
        text: dev.dma_channel.map_or("N/A".to_string(), |c| format!("{c}")),
        font_size: 11.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_val_w),
    });
}

/// Render the Events tab for a device.
fn render_events_tab(
    state: &DeviceManagerState,
    dev: &DeviceInfo,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
) {
    let label_x = x + 16.0;
    let mut row_y = y + 8.0;

    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "Event History".to_string(),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    let events = state.events_for_device(dev.id);
    if events.is_empty() {
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: "No events recorded".to_string(),
            font_size: 11.0,
            color: COLOR_OVERLAY,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 32.0),
        });
        return;
    }

    // Column headers
    cmds.push(RenderCommand::FillRect {
        x: x + 8.0,
        y: row_y,
        width: width - 16.0,
        height: EVENT_ROW_HEIGHT,
        color: COLOR_SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y + 3.0,
        text: "Time".to_string(),
        font_size: 10.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Bold,
        max_width: Some(140.0),
    });
    cmds.push(RenderCommand::Text {
        x: x + 170.0,
        y: row_y + 3.0,
        text: "Event".to_string(),
        font_size: 10.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Bold,
        max_width: Some(100.0),
    });
    cmds.push(RenderCommand::Text {
        x: x + 280.0,
        y: row_y + 3.0,
        text: "Detail".to_string(),
        font_size: 10.0,
        color: COLOR_OVERLAY,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 296.0),
    });
    row_y += EVENT_ROW_HEIGHT;

    for (i, event) in events.iter().enumerate() {
        let bg = if i % 2 == 0 {
            Color::TRANSPARENT
        } else {
            COLOR_MANTLE
        };

        if bg != Color::TRANSPARENT {
            cmds.push(RenderCommand::FillRect {
                x: x + 8.0,
                y: row_y,
                width: width - 16.0,
                height: EVENT_ROW_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y + 3.0,
            text: event.timestamp.clone(),
            font_size: 10.0,
            color: COLOR_SUBTEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 170.0,
            y: row_y + 3.0,
            text: event.kind.label().to_string(),
            font_size: 10.0,
            color: event.kind.color(),
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 280.0,
            y: row_y + 3.0,
            text: event.detail.clone(),
            font_size: 10.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 296.0),
        });
        row_y += EVENT_ROW_HEIGHT;
    }
}

/// Render the full resource view (IRQs, MMIO, DMA system-wide).
fn render_resource_view(
    state: &DeviceManagerState,
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    width: f32,
    _height: f32,
) {
    let label_x = x + 16.0;
    let mut row_y = y + 8.0;

    // Title
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: "System Resource Overview".to_string(),
        font_size: 15.0,
        color: COLOR_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += 28.0;

    // IRQ section
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: format!("IRQ Assignments ({})", state.resource_view.irqs.len()),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    for irq in &state.resource_view.irqs {
        let shared_tag = if irq.shared { " (shared)" } else { "" };
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: format!("IRQ {:>3}", irq.irq),
            font_size: 11.0,
            color: COLOR_TEAL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 110.0,
            y: row_y,
            text: format!("{}{shared_tag}", irq.device_name),
            font_size: 11.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 126.0),
        });
        row_y += PROPERTY_ROW_HEIGHT;
    }

    row_y += 8.0;
    cmds.push(RenderCommand::Line {
        x1: label_x,
        y1: row_y,
        x2: x + width - 16.0,
        y2: row_y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
    row_y += 12.0;

    // MMIO section
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: format!("MMIO Ranges ({})", state.resource_view.mmio_ranges.len()),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    for mmio in &state.resource_view.mmio_ranges {
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: mmio.format_range(),
            font_size: 11.0,
            color: COLOR_PEACH,
            font_weight: FontWeightHint::Regular,
            max_width: Some(220.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 250.0,
            y: row_y,
            text: mmio.device_name.clone(),
            font_size: 11.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 266.0),
        });
        row_y += PROPERTY_ROW_HEIGHT;
    }

    row_y += 8.0;
    cmds.push(RenderCommand::Line {
        x1: label_x,
        y1: row_y,
        x2: x + width - 16.0,
        y2: row_y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });
    row_y += 12.0;

    // DMA section
    cmds.push(RenderCommand::Text {
        x: label_x,
        y: row_y,
        text: format!("DMA Channels ({})", state.resource_view.dma_channels.len()),
        font_size: 13.0,
        color: COLOR_LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: Some(width - 32.0),
    });
    row_y += SECTION_HEADER_HEIGHT;

    for dma in &state.resource_view.dma_channels {
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: row_y,
            text: format!("Ch {:>2}", dma.channel),
            font_size: 11.0,
            color: COLOR_SAPPHIRE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 90.0,
            y: row_y,
            text: dma.device_name.clone(),
            font_size: 11.0,
            color: COLOR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 106.0),
        });
        row_y += PROPERTY_ROW_HEIGHT;
    }
}

/// Render the status bar at the bottom of the window.
fn render_status_bar(state: &DeviceManagerState, cmds: &mut Vec<RenderCommand>) {
    let y = state.height - STATUS_BAR_HEIGHT;

    cmds.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: state.width,
        height: STATUS_BAR_HEIGHT,
        color: COLOR_MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    cmds.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: state.width,
        y2: y,
        color: COLOR_SURFACE1,
        width: 1.0,
    });

    let total = state.devices.len();
    let problems = state.problem_device_count();
    let enabled = state.enabled_device_count();
    let matching = state.matching_device_count();

    let status_text = if state.search_query.is_empty() {
        format!("{total} devices | {enabled} enabled | {problems} problem(s)")
    } else {
        format!("{matching}/{total} matching | {enabled} enabled | {problems} problem(s)")
    };

    cmds.push(RenderCommand::Text {
        x: 12.0,
        y: y + 5.0,
        text: status_text,
        font_size: 11.0,
        color: COLOR_SUBTEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(state.width - 24.0),
    });
}

// ============================================================================
// Event handling
// ============================================================================

/// Handle an event and return the result.
pub fn handle_event(state: &mut DeviceManagerState, event: &Event) -> EventResult {
    match event {
        Event::Resize { width, height } => {
            state.width = *width as f32;
            state.height = *height as f32;
            EventResult::Consumed
        }
        Event::Key(key_event) => handle_key_event(state, key_event),
        Event::Mouse(mouse_event) => handle_mouse_event(state, mouse_event),
        _ => EventResult::Ignored,
    }
}

/// Handle keyboard events.
fn handle_key_event(state: &mut DeviceManagerState, key: &KeyEvent) -> EventResult {
    if !key.pressed {
        return EventResult::Ignored;
    }

    // Search bar text input
    if state.search_focused {
        match key.key {
            Key::Escape => {
                state.search_focused = false;
                return EventResult::Consumed;
            }
            Key::Backspace => {
                state.search_query.pop();
                state.apply_search_filter();
                return EventResult::Consumed;
            }
            Key::Enter => {
                state.search_focused = false;
                return EventResult::Consumed;
            }
            _ => {
                if let Some(ch) = key.text
                    && !ch.is_control() {
                        state.search_query.push(ch);
                        state.apply_search_filter();
                        return EventResult::Consumed;
                    }
            }
        }
        return EventResult::Consumed;
    }

    // Global shortcuts
    if key.modifiers.ctrl {
        match key.key {
            Key::F => {
                state.search_focused = true;
                return EventResult::Consumed;
            }
            Key::E => {
                state.handle_toolbar_action(ToolbarAction::Export);
                return EventResult::Consumed;
            }
            Key::R => {
                state.handle_toolbar_action(ToolbarAction::Scan);
                return EventResult::Consumed;
            }
            _ => {}
        }
    }

    match key.key {
        Key::Up => {
            if let Some(idx) = state.selected_tree_index {
                // Find previous visible node
                let mut new_idx = idx;
                while new_idx > 0 {
                    new_idx -= 1;
                    if is_node_visible(state, new_idx) {
                        state.select_tree_node(new_idx);
                        break;
                    }
                }
            } else if !state.tree_nodes.is_empty() {
                state.select_tree_node(0);
            }
            EventResult::Consumed
        }
        Key::Down => {
            if let Some(idx) = state.selected_tree_index {
                let mut new_idx = idx;
                while new_idx + 1 < state.tree_nodes.len() {
                    new_idx += 1;
                    if is_node_visible(state, new_idx) {
                        state.select_tree_node(new_idx);
                        break;
                    }
                }
            } else if !state.tree_nodes.is_empty() {
                state.select_tree_node(0);
            }
            EventResult::Consumed
        }
        Key::Left => {
            // Collapse category or move to parent category
            if let Some(idx) = state.selected_tree_index
                && let Some(node) = state.tree_nodes.get(idx) {
                    if node.category.is_some() && node.expanded {
                        state.toggle_category(idx);
                    } else if node.device_id.is_some() {
                        // Move to parent category
                        for i in (0..idx).rev() {
                            if state.tree_nodes.get(i).is_some_and(|n| n.category.is_some()) {
                                state.select_tree_node(i);
                                break;
                            }
                        }
                    }
                }
            EventResult::Consumed
        }
        Key::Right => {
            // Expand category
            if let Some(idx) = state.selected_tree_index
                && let Some(node) = state.tree_nodes.get(idx)
                    && node.category.is_some() && !node.expanded {
                        state.toggle_category(idx);
                    }
            EventResult::Consumed
        }
        Key::Enter | Key::Space => {
            if let Some(idx) = state.selected_tree_index
                && let Some(node) = state.tree_nodes.get(idx)
                    && node.category.is_some() {
                        state.toggle_category(idx);
                    }
            EventResult::Consumed
        }
        Key::Tab => {
            // Cycle through properties tabs
            let tabs = PropertiesTab::all();
            if let Some(pos) = tabs.iter().position(|t| *t == state.active_tab) {
                let next = (pos + 1) % tabs.len();
                state.active_tab = tabs[next];
            }
            EventResult::Consumed
        }
        Key::F5 => {
            state.handle_toolbar_action(ToolbarAction::Scan);
            EventResult::Consumed
        }
        Key::Delete => {
            state.handle_toolbar_action(ToolbarAction::Uninstall);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Handle mouse events.
fn handle_mouse_event(
    state: &mut DeviceManagerState,
    mouse: &guitk::event::MouseEvent,
) -> EventResult {
    let mx = mouse.x;
    let my = mouse.y;

    match &mouse.kind {
        MouseEventKind::Press(MouseButton::Left) => {
            // Check toolbar buttons
            let toolbar_y = TITLE_BAR_HEIGHT;
            if my >= toolbar_y && my < toolbar_y + TOOLBAR_HEIGHT {
                let actions = ToolbarAction::all();
                for (i, _action) in actions.iter().enumerate() {
                    let btn_x = 8.0 + i as f32 * (TOOLBAR_BTN_WIDTH + 6.0);
                    let btn_y = toolbar_y + (TOOLBAR_HEIGHT - TOOLBAR_BTN_HEIGHT) / 2.0;
                    if mx >= btn_x
                        && mx < btn_x + TOOLBAR_BTN_WIDTH
                        && my >= btn_y
                        && my < btn_y + TOOLBAR_BTN_HEIGHT
                    {
                        state.handle_toolbar_action(actions[i]);
                        return EventResult::Consumed;
                    }
                }
            }

            // Check search bar
            let search_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
            if mx < SIDEBAR_WIDTH
                && my >= search_y
                && my < search_y + SEARCH_BAR_HEIGHT
            {
                state.search_focused = true;
                return EventResult::Consumed;
            }

            state.search_focused = false;

            // Check tree sidebar
            let tree_top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
            let tree_bottom = state.height - STATUS_BAR_HEIGHT;
            if mx < SIDEBAR_WIDTH && my >= tree_top && my < tree_bottom {
                let click_y = my - tree_top + state.tree_scroll;
                if let Some(idx) = tree_hit_test(state, click_y) {
                    if let Some(node) = state.tree_nodes.get(idx)
                        && node.category.is_some() {
                            state.toggle_category(idx);
                        }
                    state.select_tree_node(idx);
                    return EventResult::Consumed;
                }
            }

            // Check tab bar
            let tab_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
            let panel_x = SIDEBAR_WIDTH + 1.0;
            let panel_width = state.width - panel_x;
            if mx > panel_x
                && my >= tab_y
                && my < tab_y + TAB_BAR_HEIGHT
                && state.selected_device().is_some()
            {
                let tabs = PropertiesTab::all();
                let tab_width = panel_width / tabs.len() as f32;
                let tab_idx = ((mx - panel_x) / tab_width) as usize;
                if tab_idx < tabs.len() {
                    state.active_tab = tabs[tab_idx];
                    state.properties_scroll = 0.0;
                    return EventResult::Consumed;
                }
            }

            EventResult::Consumed
        }
        MouseEventKind::Move => {
            // Update hover states
            state.hovered_tree_index = None;
            state.hovered_toolbar_action = None;
            state.hovered_tab_index = None;

            // Toolbar hover
            let toolbar_y = TITLE_BAR_HEIGHT;
            if my >= toolbar_y && my < toolbar_y + TOOLBAR_HEIGHT {
                let actions = ToolbarAction::all();
                for (i, _) in actions.iter().enumerate() {
                    let btn_x = 8.0 + i as f32 * (TOOLBAR_BTN_WIDTH + 6.0);
                    let btn_y = toolbar_y + (TOOLBAR_HEIGHT - TOOLBAR_BTN_HEIGHT) / 2.0;
                    if mx >= btn_x
                        && mx < btn_x + TOOLBAR_BTN_WIDTH
                        && my >= btn_y
                        && my < btn_y + TOOLBAR_BTN_HEIGHT
                    {
                        state.hovered_toolbar_action = Some(i);
                        break;
                    }
                }
            }

            // Tree hover
            let tree_top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
            let tree_bottom = state.height - STATUS_BAR_HEIGHT;
            if mx < SIDEBAR_WIDTH && my >= tree_top && my < tree_bottom {
                let hover_y = my - tree_top + state.tree_scroll;
                state.hovered_tree_index = tree_hit_test(state, hover_y);
            }

            // Tab hover
            let tab_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
            let panel_x = SIDEBAR_WIDTH + 1.0;
            let panel_width = state.width - panel_x;
            if mx > panel_x && my >= tab_y && my < tab_y + TAB_BAR_HEIGHT {
                let tabs = PropertiesTab::all();
                let tab_width = panel_width / tabs.len() as f32;
                let tab_idx = ((mx - panel_x) / tab_width) as usize;
                if tab_idx < tabs.len() {
                    state.hovered_tab_index = Some(tab_idx);
                }
            }

            EventResult::Consumed
        }
        MouseEventKind::Scroll { dy, .. } => {
            // Scroll the tree sidebar
            let tree_top = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
            let tree_bottom = state.height - STATUS_BAR_HEIGHT;
            if mx < SIDEBAR_WIDTH && my >= tree_top && my < tree_bottom {
                state.tree_scroll = (state.tree_scroll - dy * 20.0).max(0.0);
                return EventResult::Consumed;
            }

            // Scroll the properties panel
            if mx >= SIDEBAR_WIDTH {
                state.properties_scroll = (state.properties_scroll - dy * 20.0).max(0.0);
                return EventResult::Consumed;
            }

            EventResult::Ignored
        }
        _ => EventResult::Ignored,
    }
}

/// Determine which tree node index is at a given y-offset within the tree.
fn tree_hit_test(state: &DeviceManagerState, y: f32) -> Option<usize> {
    let mut accumulated_y: f32 = 0.0;
    let mut parent_expanded = true;

    for (i, node) in state.tree_nodes.iter().enumerate() {
        if !node.visible {
            continue;
        }
        if node.depth == 1 && !parent_expanded {
            continue;
        }
        if node.depth == 0 {
            parent_expanded = node.expanded;
        }

        if y >= accumulated_y && y < accumulated_y + TREE_ROW_HEIGHT {
            return Some(i);
        }
        accumulated_y += TREE_ROW_HEIGHT;
    }
    None
}

/// Check if a tree node at the given index is currently visible.
fn is_node_visible(state: &DeviceManagerState, index: usize) -> bool {
    let Some(node) = state.tree_nodes.get(index) else {
        return false;
    };
    if !node.visible {
        return false;
    }
    if node.depth == 0 {
        return true;
    }
    // Device node: check if parent category is expanded.
    for i in (0..index).rev() {
        if let Some(parent) = state.tree_nodes.get(i)
            && parent.category.is_some() {
                return parent.expanded;
            }
    }
    true
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // Placeholder: in a real Slate OS environment, this would create a window,
    // enter the event loop, and call render() / handle_event() each frame.
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- DeviceCategory tests ------------------------------------------------

    #[test]
    fn test_category_label() {
        assert_eq!(DeviceCategory::Display.label(), "Display Adapters");
        assert_eq!(DeviceCategory::Audio.label(), "Audio Devices");
        assert_eq!(DeviceCategory::Network.label(), "Network Adapters");
        assert_eq!(DeviceCategory::Storage.label(), "Storage Controllers");
        assert_eq!(DeviceCategory::Usb.label(), "USB Controllers");
        assert_eq!(DeviceCategory::Input.label(), "Input Devices");
        assert_eq!(DeviceCategory::System.label(), "System Devices");
        assert_eq!(DeviceCategory::Other.label(), "Other Devices");
    }

    #[test]
    fn test_category_icon() {
        assert_eq!(DeviceCategory::Display.icon(), "[D]");
        assert_eq!(DeviceCategory::Audio.icon(), "[A]");
        assert_eq!(DeviceCategory::Network.icon(), "[N]");
        assert_eq!(DeviceCategory::Storage.icon(), "[S]");
        assert_eq!(DeviceCategory::Usb.icon(), "[U]");
        assert_eq!(DeviceCategory::Input.icon(), "[I]");
        assert_eq!(DeviceCategory::System.icon(), "[Y]");
        assert_eq!(DeviceCategory::Other.icon(), "[?]");
    }

    #[test]
    fn test_category_color_is_distinct() {
        let cats = DeviceCategory::all();
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i].color(), cats[j].color(), "Categories {:?} and {:?} share a color", cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_category_all_returns_eight() {
        assert_eq!(DeviceCategory::all().len(), 8);
    }

    // -- DeviceStatus tests --------------------------------------------------

    #[test]
    fn test_status_label() {
        assert_eq!(DeviceStatus::Working.label(), "Working");
        assert_eq!(DeviceStatus::Warning.label(), "Warning");
        assert_eq!(DeviceStatus::Error.label(), "Error");
        assert_eq!(DeviceStatus::Disabled.label(), "Disabled");
        assert_eq!(DeviceStatus::Unknown.label(), "Unknown");
    }

    #[test]
    fn test_status_is_problem() {
        assert!(!DeviceStatus::Working.is_problem());
        assert!(DeviceStatus::Warning.is_problem());
        assert!(DeviceStatus::Error.is_problem());
        assert!(!DeviceStatus::Disabled.is_problem());
        assert!(!DeviceStatus::Unknown.is_problem());
    }

    #[test]
    fn test_status_icon() {
        assert_eq!(DeviceStatus::Working.icon(), "+");
        assert_eq!(DeviceStatus::Error.icon(), "X");
        assert_eq!(DeviceStatus::Disabled.icon(), "-");
    }

    #[test]
    fn test_status_color_nonzero_alpha() {
        for status in &[DeviceStatus::Working, DeviceStatus::Warning, DeviceStatus::Error, DeviceStatus::Disabled, DeviceStatus::Unknown] {
            assert_eq!(status.color().a, 255);
        }
    }

    // -- DeviceEventKind tests -----------------------------------------------

    #[test]
    fn test_event_kind_label() {
        assert_eq!(DeviceEventKind::Connected.label(), "Connected");
        assert_eq!(DeviceEventKind::Disconnected.label(), "Disconnected");
        assert_eq!(DeviceEventKind::Error.label(), "Error");
        assert_eq!(DeviceEventKind::DriverLoaded.label(), "Driver Loaded");
        assert_eq!(DeviceEventKind::DriverUnloaded.label(), "Driver Unloaded");
        assert_eq!(DeviceEventKind::Enabled.label(), "Enabled");
        assert_eq!(DeviceEventKind::Disabled.label(), "Disabled");
        assert_eq!(DeviceEventKind::Reset.label(), "Reset");
    }

    #[test]
    fn test_event_kind_color_is_opaque() {
        let kinds = [
            DeviceEventKind::Connected,
            DeviceEventKind::Disconnected,
            DeviceEventKind::Error,
            DeviceEventKind::Reset,
        ];
        for k in &kinds {
            assert_eq!(k.color().a, 255);
        }
    }

    // -- DeviceEvent tests ---------------------------------------------------

    #[test]
    fn test_device_event_new() {
        let ev = DeviceEvent::new(42, DeviceEventKind::Connected, "2026-01-01 00:00:00", "test detail");
        assert_eq!(ev.device_id, 42);
        assert_eq!(ev.kind, DeviceEventKind::Connected);
        assert_eq!(ev.timestamp, "2026-01-01 00:00:00");
        assert_eq!(ev.detail, "test detail");
    }

    // -- DriverInfo tests ----------------------------------------------------

    #[test]
    fn test_driver_info_new() {
        let drv = DriverInfo::new("test-drv", "1.0.0", "TestCo", "2026-01-01", true);
        assert_eq!(drv.name, "test-drv");
        assert_eq!(drv.version, "1.0.0");
        assert_eq!(drv.provider, "TestCo");
        assert_eq!(drv.date, "2026-01-01");
        assert!(drv.update_available);
    }

    #[test]
    fn test_driver_info_no_update() {
        let drv = DriverInfo::new("drv", "2.0", "X", "2026-05-01", false);
        assert!(!drv.update_available);
    }

    // -- MmioRange tests -----------------------------------------------------

    #[test]
    fn test_mmio_range_format() {
        let r = MmioRange {
            start: 0xFD00_0000,
            end: 0xFDFF_FFFF,
            device_name: "GPU".to_string(),
        };
        assert_eq!(r.format_range(), "0xFD000000 - 0xFDFFFFFF");
    }

    #[test]
    fn test_mmio_range_size() {
        let r = MmioRange {
            start: 0x1000,
            end: 0x1FFF,
            device_name: "Test".to_string(),
        };
        assert_eq!(r.size(), 0x1000);
    }

    #[test]
    fn test_mmio_range_size_zero() {
        let r = MmioRange {
            start: 0x5000,
            end: 0x5000,
            device_name: "Test".to_string(),
        };
        assert_eq!(r.size(), 1);
    }

    // -- ResourceView tests --------------------------------------------------

    #[test]
    fn test_resource_view_from_empty() {
        let rv = ResourceView::from_devices(&[]);
        assert!(rv.irqs.is_empty());
        assert!(rv.mmio_ranges.is_empty());
        assert!(rv.dma_channels.is_empty());
        assert_eq!(rv.total_count(), 0);
    }

    #[test]
    fn test_resource_view_from_devices() {
        let devices = sample_devices();
        let rv = ResourceView::from_devices(&devices);
        assert!(!rv.irqs.is_empty());
        assert!(!rv.mmio_ranges.is_empty());
        assert!(rv.total_count() > 0);
    }

    #[test]
    fn test_resource_view_irq_sorted() {
        let devices = sample_devices();
        let rv = ResourceView::from_devices(&devices);
        for i in 1..rv.irqs.len() {
            assert!(rv.irqs[i].irq >= rv.irqs[i - 1].irq, "IRQs not sorted");
        }
    }

    #[test]
    fn test_resource_view_mmio_sorted() {
        let devices = sample_devices();
        let rv = ResourceView::from_devices(&devices);
        for i in 1..rv.mmio_ranges.len() {
            assert!(
                rv.mmio_ranges[i].start >= rv.mmio_ranges[i - 1].start,
                "MMIO ranges not sorted"
            );
        }
    }

    #[test]
    fn test_resource_view_dma_sorted() {
        let devices = sample_devices();
        let rv = ResourceView::from_devices(&devices);
        for i in 1..rv.dma_channels.len() {
            assert!(
                rv.dma_channels[i].channel >= rv.dma_channels[i - 1].channel,
                "DMA channels not sorted"
            );
        }
    }

    #[test]
    fn test_resource_view_shared_irq_detection() {
        let devices = sample_devices();
        let rv = ResourceView::from_devices(&devices);
        // IRQ 9 and IRQ 10 are each used by two devices in sample data.
        let shared_irqs: Vec<&IrqAssignment> = rv.irqs.iter().filter(|a| a.shared).collect();
        assert!(
            !shared_irqs.is_empty(),
            "Should detect shared IRQs in sample data"
        );
    }

    // -- DeviceInfo tests ----------------------------------------------------

    #[test]
    fn test_device_has_problem() {
        let devices = sample_devices();
        let warning_dev = devices.iter().find(|d| d.status == DeviceStatus::Warning);
        assert!(warning_dev.is_some());
        assert!(warning_dev.expect("checked").has_problem());

        let working_dev = devices.iter().find(|d| d.status == DeviceStatus::Working);
        assert!(working_dev.is_some());
        assert!(!working_dev.expect("checked").has_problem());
    }

    #[test]
    fn test_device_has_driver_update() {
        let devices = sample_devices();
        // Intel HD Audio has update_available = true
        let audio = devices.iter().find(|d| d.id == 2);
        assert!(audio.is_some());
        assert!(audio.expect("checked").has_driver_update());

        // Virtio GPU has update_available = false
        let gpu = devices.iter().find(|d| d.id == 1);
        assert!(gpu.is_some());
        assert!(!gpu.expect("checked").has_driver_update());
    }

    #[test]
    fn test_device_format_irq() {
        let devices = sample_devices();
        let dev = &devices[0]; // Has IRQ 11
        assert!(dev.format_irq().contains("11"));

        // PCI-to-ISA bridge (id=9) has no IRQ
        let bridge = devices.iter().find(|d| d.id == 9).expect("exists");
        assert_eq!(bridge.format_irq(), "N/A");
    }

    #[test]
    fn test_device_format_mmio() {
        let devices = sample_devices();
        let dev = &devices[0]; // Has MMIO
        assert!(dev.format_mmio().contains("0x"));

        // PS/2 Keyboard has no MMIO
        let kbd = devices.iter().find(|d| d.id == 6).expect("exists");
        assert_eq!(kbd.format_mmio(), "N/A");
    }

    // -- TreeNode tests ------------------------------------------------------

    #[test]
    fn test_tree_node_category() {
        let node = TreeNode::category(DeviceCategory::Display);
        assert!(node.category.is_some());
        assert!(node.device_id.is_none());
        assert_eq!(node.depth, 0);
        assert!(node.expanded);
        assert!(node.visible);
    }

    #[test]
    fn test_tree_node_device() {
        let node = TreeNode::device(42, "Test Device");
        assert!(node.category.is_none());
        assert_eq!(node.device_id, Some(42));
        assert_eq!(node.depth, 1);
        assert!(!node.expanded);
        assert!(node.visible);
    }

    // -- build_tree_nodes tests ----------------------------------------------

    #[test]
    fn test_build_tree_nodes_nonempty() {
        let devices = sample_devices();
        let nodes = build_tree_nodes(&devices);
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_build_tree_nodes_categories_first() {
        let devices = sample_devices();
        let nodes = build_tree_nodes(&devices);
        // First node should be a category
        assert!(nodes[0].category.is_some());
    }

    #[test]
    fn test_build_tree_nodes_device_follows_category() {
        let devices = sample_devices();
        let nodes = build_tree_nodes(&devices);
        for i in 1..nodes.len() {
            if nodes[i].device_id.is_some() {
                // There must be a category before it
                let found_cat = nodes[..i].iter().rev().any(|n| n.category.is_some());
                assert!(found_cat, "Device node at index {i} has no preceding category");
            }
        }
    }

    #[test]
    fn test_build_tree_nodes_empty_category_excluded() {
        let nodes = build_tree_nodes(&[]);
        assert!(nodes.is_empty());
    }

    // -- PropertiesTab tests -------------------------------------------------

    #[test]
    fn test_properties_tab_all() {
        let tabs = PropertiesTab::all();
        assert_eq!(tabs.len(), 4);
        assert_eq!(tabs[0], PropertiesTab::General);
        assert_eq!(tabs[1], PropertiesTab::Driver);
        assert_eq!(tabs[2], PropertiesTab::Resources);
        assert_eq!(tabs[3], PropertiesTab::Events);
    }

    #[test]
    fn test_properties_tab_label() {
        assert_eq!(PropertiesTab::General.label(), "General");
        assert_eq!(PropertiesTab::Driver.label(), "Driver");
        assert_eq!(PropertiesTab::Resources.label(), "Resources");
        assert_eq!(PropertiesTab::Events.label(), "Events");
    }

    // -- ToolbarAction tests -------------------------------------------------

    #[test]
    fn test_toolbar_action_all() {
        let actions = ToolbarAction::all();
        assert_eq!(actions.len(), 6);
    }

    #[test]
    fn test_toolbar_action_labels() {
        assert_eq!(ToolbarAction::Scan.label(), "Scan");
        assert_eq!(ToolbarAction::Properties.label(), "Properties");
        assert_eq!(ToolbarAction::Enable.label(), "Enable");
        assert_eq!(ToolbarAction::Disable.label(), "Disable");
        assert_eq!(ToolbarAction::Uninstall.label(), "Uninstall");
        assert_eq!(ToolbarAction::Export.label(), "Export");
    }

    // -- UpdateCheckStatus tests ---------------------------------------------

    #[test]
    fn test_update_check_status_label() {
        assert_eq!(UpdateCheckStatus::NotChecked.label(), "Not Checked");
        assert_eq!(UpdateCheckStatus::Checking.label(), "Checking...");
        assert_eq!(UpdateCheckStatus::UpToDate.label(), "Up to Date");
        assert_eq!(UpdateCheckStatus::UpdateAvailable.label(), "Update Available");
        assert_eq!(UpdateCheckStatus::Failed.label(), "Check Failed");
    }

    #[test]
    fn test_update_check_status_color_opaque() {
        for s in &[
            UpdateCheckStatus::NotChecked,
            UpdateCheckStatus::Checking,
            UpdateCheckStatus::UpToDate,
            UpdateCheckStatus::UpdateAvailable,
            UpdateCheckStatus::Failed,
        ] {
            assert_eq!(s.color().a, 255);
        }
    }

    // -- DriverUpdateCheck tests ---------------------------------------------

    #[test]
    fn test_driver_update_check_new() {
        let c = DriverUpdateCheck::new(5);
        assert_eq!(c.device_id, 5);
        assert_eq!(c.status, UpdateCheckStatus::NotChecked);
        assert!(c.available_version.is_none());
        assert!(c.last_checked.is_none());
    }

    #[test]
    fn test_driver_update_check_start() {
        let mut c = DriverUpdateCheck::new(1);
        c.start_check();
        assert_eq!(c.status, UpdateCheckStatus::Checking);
    }

    #[test]
    fn test_driver_update_check_finish_with_update() {
        let mut c = DriverUpdateCheck::new(1);
        c.start_check();
        c.finish_check(Some("2.0.0".to_string()), "2026-05-18");
        assert_eq!(c.status, UpdateCheckStatus::UpdateAvailable);
        assert_eq!(c.available_version, Some("2.0.0".to_string()));
        assert_eq!(c.last_checked, Some("2026-05-18".to_string()));
    }

    #[test]
    fn test_driver_update_check_finish_up_to_date() {
        let mut c = DriverUpdateCheck::new(1);
        c.start_check();
        c.finish_check(None, "2026-05-18");
        assert_eq!(c.status, UpdateCheckStatus::UpToDate);
        assert!(c.available_version.is_none());
    }

    #[test]
    fn test_driver_update_check_fail() {
        let mut c = DriverUpdateCheck::new(1);
        c.start_check();
        c.fail_check("2026-05-18");
        assert_eq!(c.status, UpdateCheckStatus::Failed);
        assert_eq!(c.last_checked, Some("2026-05-18".to_string()));
    }

    // -- DeviceManagerState tests --------------------------------------------

    #[test]
    fn test_state_new_has_devices() {
        let state = DeviceManagerState::new();
        assert!(!state.devices.is_empty());
    }

    #[test]
    fn test_state_new_has_tree_nodes() {
        let state = DeviceManagerState::new();
        assert!(!state.tree_nodes.is_empty());
    }

    #[test]
    fn test_state_new_has_events() {
        let state = DeviceManagerState::new();
        assert!(!state.event_history.is_empty());
    }

    #[test]
    fn test_state_new_has_resource_view() {
        let state = DeviceManagerState::new();
        assert!(state.resource_view.total_count() > 0);
    }

    #[test]
    fn test_state_default_selection() {
        let state = DeviceManagerState::new();
        assert!(state.selected_tree_index.is_none());
        assert!(state.selected_device().is_none());
    }

    #[test]
    fn test_state_select_device() {
        let mut state = DeviceManagerState::new();
        // Find a device node
        let dev_idx = state.tree_nodes.iter().position(|n| n.device_id.is_some()).expect("has device nodes");
        state.select_tree_node(dev_idx);
        assert_eq!(state.selected_tree_index, Some(dev_idx));
        assert!(state.selected_device().is_some());
    }

    #[test]
    fn test_state_select_category() {
        let mut state = DeviceManagerState::new();
        state.select_tree_node(0); // First node is always a category
        assert!(state.selected_category().is_some());
        assert!(state.show_resource_view);
    }

    #[test]
    fn test_state_problem_device_count() {
        let state = DeviceManagerState::new();
        let count = state.problem_device_count();
        // Sample data has at least 1 warning and 1 error device
        assert!(count >= 2, "Expected at least 2 problem devices, got {count}");
    }

    #[test]
    fn test_state_enabled_device_count() {
        let state = DeviceManagerState::new();
        let count = state.enabled_device_count();
        assert!(count > 0);
        assert!(count <= state.devices.len());
    }

    #[test]
    fn test_state_update_available_count() {
        let state = DeviceManagerState::new();
        let count = state.update_available_count();
        // Sample data has some devices with update_available
        assert!(count > 0);
    }

    #[test]
    fn test_state_toggle_category() {
        let mut state = DeviceManagerState::new();
        let first_cat = state.tree_nodes.iter().position(|n| n.category.is_some()).expect("has categories");
        assert!(state.tree_nodes[first_cat].expanded);
        state.toggle_category(first_cat);
        assert!(!state.tree_nodes[first_cat].expanded);
        state.toggle_category(first_cat);
        assert!(state.tree_nodes[first_cat].expanded);
    }

    #[test]
    fn test_state_set_device_enabled() {
        let mut state = DeviceManagerState::new();
        let id = state.devices[0].id;
        assert!(state.devices[0].enabled);

        state.set_device_enabled(id, false);
        let dev = state.devices.iter().find(|d| d.id == id).expect("exists");
        assert!(!dev.enabled);
        assert_eq!(dev.status, DeviceStatus::Disabled);

        state.set_device_enabled(id, true);
        let dev = state.devices.iter().find(|d| d.id == id).expect("exists");
        assert!(dev.enabled);
        assert_eq!(dev.status, DeviceStatus::Working);
    }

    #[test]
    fn test_state_uninstall_driver() {
        let mut state = DeviceManagerState::new();
        let id = state.devices[0].id;
        assert!(state.devices[0].driver.is_some());

        state.uninstall_driver(id);
        let dev = state.devices.iter().find(|d| d.id == id).expect("exists");
        assert!(dev.driver.is_none());
        assert_eq!(dev.status, DeviceStatus::Warning);
    }

    #[test]
    fn test_state_scan_hardware() {
        let mut state = DeviceManagerState::new();
        let old_node_count = state.tree_nodes.len();
        state.scan_hardware();
        // After scan, tree should be rebuilt with same data
        assert_eq!(state.tree_nodes.len(), old_node_count);
    }

    #[test]
    fn test_state_add_event() {
        let mut state = DeviceManagerState::new();
        let initial_count = state.event_history.len();
        state.add_event(DeviceEvent::new(1, DeviceEventKind::Reset, "2026-05-18 12:00:00", "test"));
        assert_eq!(state.event_history.len(), initial_count + 1);
    }

    #[test]
    fn test_state_add_event_max_cap() {
        let mut state = DeviceManagerState::new();
        state.event_history.clear();
        for i in 0..MAX_EVENT_HISTORY + 50 {
            state.add_event(DeviceEvent::new(
                1,
                DeviceEventKind::Reset,
                &format!("2026-05-18 12:{i:02}:00"),
                "overflow test",
            ));
        }
        assert_eq!(state.event_history.len(), MAX_EVENT_HISTORY);
    }

    #[test]
    fn test_state_events_for_device() {
        let state = DeviceManagerState::new();
        let events = state.events_for_device(1);
        assert!(!events.is_empty());
        for ev in &events {
            assert_eq!(ev.device_id, 1);
        }
    }

    #[test]
    fn test_state_events_for_nonexistent_device() {
        let state = DeviceManagerState::new();
        let events = state.events_for_device(99999);
        assert!(events.is_empty());
    }

    // -- Search/filter tests -------------------------------------------------

    #[test]
    fn test_search_empty_matches_all() {
        let state = DeviceManagerState::new();
        assert_eq!(state.matching_device_count(), state.devices.len());
    }

    #[test]
    fn test_search_filter_applies() {
        let mut state = DeviceManagerState::new();
        state.search_query = "virtio".to_string();
        state.apply_search_filter();
        let count = state.matching_device_count();
        assert!(count > 0);
        assert!(count < state.devices.len());
    }

    #[test]
    fn test_search_filter_no_match() {
        let mut state = DeviceManagerState::new();
        state.search_query = "xyznonexistent123".to_string();
        state.apply_search_filter();
        assert_eq!(state.matching_device_count(), 0);
    }

    #[test]
    fn test_search_matches_vendor() {
        let q = "intel";
        let devices = sample_devices();
        let matches: Vec<&DeviceInfo> = devices.iter().filter(|d| device_matches_query(d, q)).collect();
        assert!(!matches.is_empty());
        // All matched devices should have "Intel" somewhere
        for m in &matches {
            let combined = format!(
                "{} {} {} {} {} {}",
                m.name, m.vendor, m.device_type,
                m.category.label(), m.status.label(),
                m.hw_id.as_deref().unwrap_or(""),
            )
            .to_lowercase();
            assert!(combined.contains(q), "Device {:?} should match '{q}'", m.name);
        }
    }

    #[test]
    fn test_search_matches_hw_id() {
        assert!(device_matches_query(&sample_devices()[0], "1af4"));
    }

    #[test]
    fn test_search_matches_driver_name() {
        assert!(device_matches_query(&sample_devices()[0], "virtio-gpu"));
    }

    #[test]
    fn test_search_matches_location() {
        assert!(device_matches_query(&sample_devices()[0], "pci bus"));
    }

    // -- Export report tests -------------------------------------------------

    #[test]
    fn test_export_report_nonempty() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(!report.is_empty());
    }

    #[test]
    fn test_export_report_contains_header() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(report.contains("Hardware Report"));
    }

    #[test]
    fn test_export_report_contains_devices() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(report.contains("Virtio GPU"));
        assert!(report.contains("Intel HD Audio"));
    }

    #[test]
    fn test_export_report_contains_irqs() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(report.contains("IRQ Assignments"));
    }

    #[test]
    fn test_export_report_contains_mmio() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(report.contains("MMIO Ranges"));
    }

    #[test]
    fn test_export_report_contains_dma() {
        let state = DeviceManagerState::new();
        let report = state.export_report();
        assert!(report.contains("DMA Channels"));
    }

    // -- Render tests --------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let state = DeviceManagerState::new();
        let cmds = render(&state);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selected_device() {
        let mut state = DeviceManagerState::new();
        let dev_idx = state.tree_nodes.iter().position(|n| n.device_id.is_some()).expect("has devices");
        state.select_tree_node(dev_idx);
        let cmds = render(&state);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_resource_view() {
        let mut state = DeviceManagerState::new();
        state.select_tree_node(0); // Select a category
        assert!(state.show_resource_view);
        let cmds = render(&state);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_all_tabs() {
        let mut state = DeviceManagerState::new();
        let dev_idx = state.tree_nodes.iter().position(|n| n.device_id.is_some()).expect("has devices");
        state.select_tree_node(dev_idx);
        state.show_resource_view = false;

        for tab in PropertiesTab::all() {
            state.active_tab = *tab;
            let cmds = render(&state);
            assert!(!cmds.is_empty(), "Tab {:?} produced no commands", tab);
        }
    }

    // -- Event handling tests ------------------------------------------------

    #[test]
    fn test_handle_resize() {
        let mut state = DeviceManagerState::new();
        let result = handle_event(&mut state, &Event::Resize { width: 800, height: 600 });
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.width, 800.0);
        assert_eq!(state.height, 600.0);
    }

    #[test]
    fn test_handle_key_down() {
        let mut state = DeviceManagerState::new();
        state.select_tree_node(0);
        let result = handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Down,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(result, EventResult::Consumed);
    }

    #[test]
    fn test_handle_key_up() {
        let mut state = DeviceManagerState::new();
        state.select_tree_node(1);
        let result = handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Up,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(result, EventResult::Consumed);
    }

    #[test]
    fn test_handle_key_tab_cycles_tabs() {
        let mut state = DeviceManagerState::new();
        assert_eq!(state.active_tab, PropertiesTab::General);
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Tab,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(state.active_tab, PropertiesTab::Driver);
    }

    #[test]
    fn test_handle_key_f5_scans() {
        let mut state = DeviceManagerState::new();
        let result = handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::F5,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(result, EventResult::Consumed);
    }

    #[test]
    fn test_handle_ctrl_f_focuses_search() {
        let mut state = DeviceManagerState::new();
        assert!(!state.search_focused);
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::F,
                pressed: true,
                modifiers: Modifiers::ctrl(),
                text: None,
            }),
        );
        assert!(state.search_focused);
    }

    #[test]
    fn test_handle_search_typing() {
        let mut state = DeviceManagerState::new();
        state.search_focused = true;
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: Some('a'),
            }),
        );
        assert_eq!(state.search_query, "a");
    }

    #[test]
    fn test_handle_search_backspace() {
        let mut state = DeviceManagerState::new();
        state.search_focused = true;
        state.search_query = "abc".to_string();
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Backspace,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(state.search_query, "ab");
    }

    #[test]
    fn test_handle_search_escape() {
        let mut state = DeviceManagerState::new();
        state.search_focused = true;
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Escape,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert!(!state.search_focused);
    }

    #[test]
    fn test_handle_left_collapses_category() {
        let mut state = DeviceManagerState::new();
        let cat_idx = state.tree_nodes.iter().position(|n| n.category.is_some()).expect("has categories");
        state.select_tree_node(cat_idx);
        assert!(state.tree_nodes[cat_idx].expanded);
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Left,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert!(!state.tree_nodes[cat_idx].expanded);
    }

    #[test]
    fn test_handle_right_expands_category() {
        let mut state = DeviceManagerState::new();
        let cat_idx = state.tree_nodes.iter().position(|n| n.category.is_some()).expect("has categories");
        state.select_tree_node(cat_idx);
        state.tree_nodes[cat_idx].expanded = false;
        handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::Right,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert!(state.tree_nodes[cat_idx].expanded);
    }

    #[test]
    fn test_handle_key_release_ignored() {
        let mut state = DeviceManagerState::new();
        let result = handle_event(
            &mut state,
            &Event::Key(KeyEvent {
                key: Key::A,
                pressed: false,
                modifiers: Modifiers::NONE,
                text: None,
            }),
        );
        assert_eq!(result, EventResult::Ignored);
    }

    // -- tree_hit_test / is_node_visible tests -------------------------------

    #[test]
    fn test_tree_hit_test_first_node() {
        let state = DeviceManagerState::new();
        let hit = tree_hit_test(&state, 5.0);
        assert_eq!(hit, Some(0));
    }

    #[test]
    fn test_tree_hit_test_out_of_bounds() {
        let state = DeviceManagerState::new();
        let hit = tree_hit_test(&state, 100_000.0);
        assert!(hit.is_none());
    }

    #[test]
    fn test_is_node_visible_category() {
        let state = DeviceManagerState::new();
        assert!(is_node_visible(&state, 0));
    }

    #[test]
    fn test_is_node_visible_device_under_expanded() {
        let state = DeviceManagerState::new();
        let dev_idx = state.tree_nodes.iter().position(|n| n.device_id.is_some()).expect("has devices");
        assert!(is_node_visible(&state, dev_idx));
    }

    #[test]
    fn test_is_node_visible_device_under_collapsed() {
        let mut state = DeviceManagerState::new();
        // Collapse first category
        state.tree_nodes[0].expanded = false;
        // The device right after should not be visible
        if let Some(dev_idx) = state.tree_nodes.iter().position(|n| n.device_id.is_some()) {
            assert!(!is_node_visible(&state, dev_idx));
        }
    }

    #[test]
    fn test_is_node_visible_out_of_bounds() {
        let state = DeviceManagerState::new();
        assert!(!is_node_visible(&state, 99999));
    }

    // -- Toolbar action handling tests ---------------------------------------

    #[test]
    fn test_toolbar_enable_action() {
        let mut state = DeviceManagerState::new();
        // Find a disabled device
        let disabled_idx = state.tree_nodes.iter().position(|n| {
            n.device_id.is_some_and(|id| {
                state.devices.iter().any(|d| d.id == id && !d.enabled)
            })
        });
        if let Some(idx) = disabled_idx {
            state.select_tree_node(idx);
            state.show_resource_view = false;
            state.handle_toolbar_action(ToolbarAction::Enable);
            let dev_id = state.tree_nodes[idx].device_id.expect("device node");
            let dev = state.devices.iter().find(|d| d.id == dev_id).expect("exists");
            assert!(dev.enabled);
        }
    }

    #[test]
    fn test_toolbar_disable_action() {
        let mut state = DeviceManagerState::new();
        let dev_idx = state.tree_nodes.iter().position(|n| {
            n.device_id.is_some_and(|id| {
                state.devices.iter().any(|d| d.id == id && d.enabled)
            })
        }).expect("has enabled device");
        state.select_tree_node(dev_idx);
        state.show_resource_view = false;
        state.handle_toolbar_action(ToolbarAction::Disable);
        let dev_id = state.tree_nodes[dev_idx].device_id.expect("device node");
        let dev = state.devices.iter().find(|d| d.id == dev_id).expect("exists");
        assert!(!dev.enabled);
    }

    #[test]
    fn test_toolbar_properties_action() {
        let mut state = DeviceManagerState::new();
        let dev_idx = state.tree_nodes.iter().position(|n| n.device_id.is_some()).expect("has devices");
        state.select_tree_node(dev_idx);
        state.show_resource_view = false;
        state.active_tab = PropertiesTab::Events;
        state.handle_toolbar_action(ToolbarAction::Properties);
        assert_eq!(state.active_tab, PropertiesTab::General);
    }

    // -- device_matches_query exhaustive tests --------------------------------

    #[test]
    fn test_match_query_empty() {
        let dev = &sample_devices()[0];
        assert!(device_matches_query(dev, ""));
    }

    #[test]
    fn test_match_query_category_label() {
        let dev = &sample_devices()[0]; // Display category
        assert!(device_matches_query(dev, "display"));
    }

    #[test]
    fn test_match_query_status_label() {
        let dev = &sample_devices()[0]; // Working status
        assert!(device_matches_query(dev, "working"));
    }

    // -- Default trait -------------------------------------------------------

    #[test]
    fn test_state_default() {
        let state = DeviceManagerState::default();
        assert!(!state.devices.is_empty());
    }

    // -- Constant sanity checks ----------------------------------------------

    #[test]
    fn test_layout_constants_positive() {
        const {
            assert!(SIDEBAR_WIDTH > 0.0);
            assert!(TITLE_BAR_HEIGHT > 0.0);
            assert!(TOOLBAR_HEIGHT > 0.0);
            assert!(STATUS_BAR_HEIGHT > 0.0);
            assert!(TREE_ROW_HEIGHT > 0.0);
            assert!(TREE_INDENT > 0.0);
            assert!(PROPERTY_ROW_HEIGHT > 0.0);
            assert!(DEFAULT_WIDTH > 0.0);
            assert!(DEFAULT_HEIGHT > 0.0);
            assert!(TOOLBAR_BTN_WIDTH > 0.0);
            assert!(TOOLBAR_BTN_HEIGHT > 0.0);
        };
    }

    #[test]
    fn test_color_constants_opaque() {
        let colors = [
            COLOR_BASE, COLOR_MANTLE, COLOR_SURFACE0, COLOR_SURFACE1,
            COLOR_SURFACE2, COLOR_TEXT, COLOR_SUBTEXT, COLOR_OVERLAY,
            COLOR_BLUE, COLOR_LAVENDER, COLOR_GREEN, COLOR_YELLOW,
            COLOR_RED, COLOR_PEACH, COLOR_MAUVE, COLOR_TEAL, COLOR_SAPPHIRE,
        ];
        for c in &colors {
            assert_eq!(c.a, 255, "Color constant has non-opaque alpha");
        }
    }
}
