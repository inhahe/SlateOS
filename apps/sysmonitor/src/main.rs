//! `OurOS` System Monitor / Dashboard
//!
//! A comprehensive system monitoring application combining Task Manager
//! and Resource Monitor functionality. Features:
//!
//! - CPU monitoring: per-core usage, frequency, temperature, load average
//! - Memory monitoring: total/used/free/cached/buffers, swap usage
//! - Disk monitoring: per-disk I/O rates, IOPS, usage percentage
//! - Network monitoring: per-interface traffic, connection count
//! - Process list: sortable, filterable, with kill/signal capability
//! - Historical graphs: ring buffers for time-series data
//! - Alert thresholds: configurable CPU/memory/disk limits
//! - Tab views: Overview, Processes, CPU, Memory, Disk, Network
//! - Auto-refresh with configurable interval
//!
//! Uses the guitk library for UI rendering. All data is gathered
//! through `OurOS` syscalls; the structs here define the presentation
//! layer while the OS provides the actual system information.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::fn_params_excessive_bools)]

use guitk::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const PINK: Color = Color::from_hex(0xF5C2E7);

// ============================================================================
// Layout constants
// ============================================================================

/// Height of the tab bar at the top.
const TAB_BAR_HEIGHT: f32 = 32.0;
/// Height of the status bar at the bottom.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Height of a single row in tables.
const ROW_HEIGHT: f32 = 22.0;
/// Height of column headers.
const HEADER_HEIGHT: f32 = 24.0;
/// Number of historical samples stored per metric.
const GRAPH_HISTORY_LEN: usize = 120;
/// Corner radius for cards/panels.
const CARD_RADIUS: f32 = 6.0;
/// Padding inside content area.
const CONTENT_PAD: f32 = 12.0;
/// Gap between dashboard cards.
const CARD_GAP: f32 = 10.0;

// ============================================================================
// Ring buffer for time-series data
// ============================================================================

/// Ring buffer holding the last `GRAPH_HISTORY_LEN` samples for a
/// time-series value (CPU %, bandwidth, etc.).
#[derive(Clone, Debug)]
pub struct GraphHistory {
    samples: Vec<f32>,
    cursor: usize,
    count: usize,
}

impl GraphHistory {
    /// Create a new history buffer pre-filled with zeroes.
    pub fn new() -> Self {
        Self {
            samples: vec![0.0; GRAPH_HISTORY_LEN],
            cursor: 0,
            count: 0,
        }
    }

    /// Push a new sample, overwriting the oldest if full.
    pub fn push(&mut self, value: f32) {
        if let Some(slot) = self.samples.get_mut(self.cursor) {
            *slot = value;
        }
        self.cursor = (self.cursor.wrapping_add(1)) % GRAPH_HISTORY_LEN;
        if self.count < GRAPH_HISTORY_LEN {
            self.count = self.count.saturating_add(1);
        }
    }

    /// Iterate over samples from oldest to newest.
    pub fn iter_oldest_first(&self) -> impl Iterator<Item = f32> + '_ {
        let start = if self.count < GRAPH_HISTORY_LEN {
            0
        } else {
            self.cursor
        };
        let len = self.count;
        (0..len).map(move |i| {
            let idx = (start.wrapping_add(i)) % GRAPH_HISTORY_LEN;
            self.samples.get(idx).copied().unwrap_or(0.0)
        })
    }

    /// Number of recorded samples (up to `GRAPH_HISTORY_LEN`).
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Latest pushed sample.
    pub fn last(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let idx = if self.cursor == 0 {
            GRAPH_HISTORY_LEN.saturating_sub(1)
        } else {
            self.cursor.saturating_sub(1)
        };
        self.samples.get(idx).copied().unwrap_or(0.0)
    }

    /// Maximum value in the buffer.
    pub fn max_value(&self) -> f32 {
        self.iter_oldest_first()
            .fold(0.0f32, |acc, v| if v > acc { v } else { acc })
    }
}

impl Default for GraphHistory {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tab definitions
// ============================================================================

/// Application tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Processes,
    Cpu,
    Memory,
    Disk,
    Network,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Processes => "Processes",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::Network => "Network",
        }
    }

    pub const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Processes,
        Tab::Cpu,
        Tab::Memory,
        Tab::Disk,
        Tab::Network,
    ];
}

// ============================================================================
// Process management types
// ============================================================================

/// Process execution state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Stopped,
    Zombie,
    Idle,
}

impl ProcessStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Sleeping => "Sleeping",
            Self::Stopped => "Stopped",
            Self::Zombie => "Zombie",
            Self::Idle => "Idle",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Running => GREEN,
            Self::Sleeping => BLUE,
            Self::Stopped => YELLOW,
            Self::Zombie => RED,
            Self::Idle => OVERLAY0,
        }
    }
}

/// Information about a single process.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub status: ProcessStatus,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub thread_count: u32,
    pub uptime_secs: u64,
}

/// Columns in the process table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessColumn {
    Pid,
    Name,
    Status,
    Cpu,
    Memory,
    Threads,
    Uptime,
}

impl ProcessColumn {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Cpu => "CPU%",
            Self::Memory => "Memory",
            Self::Threads => "Threads",
            Self::Uptime => "Uptime",
        }
    }

    pub fn width(self) -> f32 {
        match self {
            Self::Pid => 60.0,
            Self::Name => 180.0,
            Self::Status => 80.0,
            Self::Cpu => 65.0,
            Self::Memory => 90.0,
            Self::Threads => 65.0,
            Self::Uptime => 90.0,
        }
    }

    pub const ALL: [ProcessColumn; 7] = [
        ProcessColumn::Pid,
        ProcessColumn::Name,
        ProcessColumn::Status,
        ProcessColumn::Cpu,
        ProcessColumn::Memory,
        ProcessColumn::Threads,
        ProcessColumn::Uptime,
    ];
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

// ============================================================================
// CPU information
// ============================================================================

/// Per-core CPU information.
#[derive(Clone, Debug)]
pub struct CpuCoreInfo {
    pub core_id: u32,
    pub usage_percent: f32,
    pub frequency_mhz: u32,
    pub temperature_c: f32,
    pub history: GraphHistory,
}

// ============================================================================
// Disk information
// ============================================================================

/// Information about a single disk/partition.
#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub read_bytes_sec: u64,
    pub write_bytes_sec: u64,
    pub read_iops: u32,
    pub write_iops: u32,
    pub read_history: GraphHistory,
    pub write_history: GraphHistory,
}

impl DiskInfo {
    /// Usage as a fraction (0.0 to 1.0).
    pub fn usage_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        self.used_bytes as f32 / self.total_bytes as f32
    }
}

// ============================================================================
// Network information
// ============================================================================

/// Information about a single network interface.
#[derive(Clone, Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub tx_bytes_sec: u64,
    pub rx_bytes_sec: u64,
    pub tx_packets_sec: u32,
    pub rx_packets_sec: u32,
    pub connection_count: u32,
    pub tx_history: GraphHistory,
    pub rx_history: GraphHistory,
}

// ============================================================================
// System information snapshot
// ============================================================================

/// Snapshot of overall system resource usage.
#[derive(Clone, Debug)]
pub struct SystemInfo {
    pub hostname: String,
    pub os_version: String,
    pub kernel_version: String,
    pub cpu_model: String,
    pub uptime_secs: u64,
    pub total_memory: u64,
    pub used_memory: u64,
    pub free_memory: u64,
    pub cached_memory: u64,
    pub buffers: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub cpu_overall: f32,
    pub load_avg: [f32; 3],
    pub process_count: u32,
    pub running_count: u32,
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
}

// ============================================================================
// Alert thresholds
// ============================================================================

/// Configurable alert thresholds for system resource usage.
#[derive(Clone, Debug)]
pub struct AlertThresholds {
    pub cpu_warn_percent: f32,
    pub cpu_crit_percent: f32,
    pub mem_warn_percent: f32,
    pub mem_crit_percent: f32,
    pub disk_warn_percent: f32,
    pub disk_crit_percent: f32,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            cpu_warn_percent: 70.0,
            cpu_crit_percent: 90.0,
            mem_warn_percent: 75.0,
            mem_crit_percent: 90.0,
            disk_warn_percent: 80.0,
            disk_crit_percent: 95.0,
        }
    }
}

impl AlertThresholds {
    /// Get the color for a usage value based on thresholds.
    pub fn color_for_value(&self, value: f32, warn: f32, crit: f32) -> Color {
        if value >= crit {
            RED
        } else if value >= warn {
            YELLOW
        } else {
            GREEN
        }
    }

    /// Get CPU color for a given usage percentage.
    pub fn cpu_color(&self, percent: f32) -> Color {
        self.color_for_value(percent, self.cpu_warn_percent, self.cpu_crit_percent)
    }

    /// Get memory color for a given usage percentage.
    pub fn mem_color(&self, percent: f32) -> Color {
        self.color_for_value(percent, self.mem_warn_percent, self.mem_crit_percent)
    }

    /// Get disk color for a given usage percentage.
    pub fn disk_color(&self, percent: f32) -> Color {
        self.color_for_value(percent, self.disk_warn_percent, self.disk_crit_percent)
    }
}

// ============================================================================
// Auto-refresh interval
// ============================================================================

/// Auto-refresh interval options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshInterval {
    HalfSecond,
    OneSecond,
    TwoSeconds,
    FiveSeconds,
}

impl RefreshInterval {
    pub fn ms(self) -> u64 {
        match self {
            Self::HalfSecond => 500,
            Self::OneSecond => 1000,
            Self::TwoSeconds => 2000,
            Self::FiveSeconds => 5000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::HalfSecond => "0.5s",
            Self::OneSecond => "1s",
            Self::TwoSeconds => "2s",
            Self::FiveSeconds => "5s",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::HalfSecond => Self::OneSecond,
            Self::OneSecond => Self::TwoSeconds,
            Self::TwoSeconds => Self::FiveSeconds,
            Self::FiveSeconds => Self::HalfSecond,
        }
    }
}

// ============================================================================
// Context menu for processes
// ============================================================================

/// Context menu action for a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAction {
    Kill,
    Stop,
    Continue,
    SetHighPriority,
    SetNormalPriority,
    SetLowPriority,
}

impl ContextAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kill => "Kill Process",
            Self::Stop => "Stop Process",
            Self::Continue => "Continue Process",
            Self::SetHighPriority => "Set High Priority",
            Self::SetNormalPriority => "Set Normal Priority",
            Self::SetLowPriority => "Set Low Priority",
        }
    }

    pub const ALL: [ContextAction; 6] = [
        ContextAction::Kill,
        ContextAction::Stop,
        ContextAction::Continue,
        ContextAction::SetHighPriority,
        ContextAction::SetNormalPriority,
        ContextAction::SetLowPriority,
    ];
}

/// State for the right-click context menu.
#[derive(Clone, Debug)]
pub struct ContextMenu {
    pub x: f32,
    pub y: f32,
    pub target_pid: u32,
    pub hover_index: Option<usize>,
}

// ============================================================================
// Active alerts
// ============================================================================

/// A triggered alert for display in the status bar / overview.
#[derive(Clone, Debug)]
pub struct Alert {
    pub message: String,
    pub severity: AlertSeverity,
}

/// Severity level for alerts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSeverity {
    Warning,
    Critical,
}

impl AlertSeverity {
    pub fn color(self) -> Color {
        match self {
            Self::Warning => YELLOW,
            Self::Critical => RED,
        }
    }
}

// ============================================================================
// Application state
// ============================================================================

/// Top-level state for the system monitor application.
pub struct SysMonitorState {
    // -- Window --
    pub window_width: u32,
    pub window_height: u32,

    // -- Navigation --
    pub active_tab: Tab,

    // -- System info --
    pub system_info: SystemInfo,
    pub cpu_history: GraphHistory,
    pub mem_history: GraphHistory,
    pub cores: Vec<CpuCoreInfo>,
    pub disks: Vec<DiskInfo>,
    pub interfaces: Vec<NetworkInterface>,

    // -- Processes --
    pub processes: Vec<ProcessInfo>,
    pub visible_indices: Vec<usize>,
    pub selected_index: Option<usize>,
    pub hovered_index: Option<usize>,
    pub sort_column: ProcessColumn,
    pub sort_direction: SortDirection,
    pub scroll_offset: usize,
    pub filter_text: String,
    pub filter_focused: bool,
    pub context_menu: Option<ContextMenu>,

    // -- Alert thresholds --
    pub thresholds: AlertThresholds,
    pub active_alerts: Vec<Alert>,

    // -- Refresh --
    pub refresh_interval: RefreshInterval,
    pub ms_since_refresh: u64,

    // -- Status --
    pub status_message: String,
}

impl SysMonitorState {
    /// Create a new system monitor with default state.
    pub fn new() -> Self {
        let system_info = SystemInfo {
            hostname: String::new(),
            os_version: String::new(),
            kernel_version: String::new(),
            cpu_model: String::new(),
            uptime_secs: 0,
            total_memory: 0,
            used_memory: 0,
            free_memory: 0,
            cached_memory: 0,
            buffers: 0,
            swap_total: 0,
            swap_used: 0,
            cpu_overall: 0.0,
            load_avg: [0.0; 3],
            process_count: 0,
            running_count: 0,
            battery_percent: None,
            battery_charging: false,
        };

        Self {
            window_width: 1024,
            window_height: 720,
            active_tab: Tab::Overview,
            system_info,
            cpu_history: GraphHistory::new(),
            mem_history: GraphHistory::new(),
            cores: Vec::new(),
            disks: Vec::new(),
            interfaces: Vec::new(),
            processes: Vec::new(),
            visible_indices: Vec::new(),
            selected_index: None,
            hovered_index: None,
            sort_column: ProcessColumn::Cpu,
            sort_direction: SortDirection::Descending,
            scroll_offset: 0,
            filter_text: String::new(),
            filter_focused: false,
            context_menu: None,
            thresholds: AlertThresholds::default(),
            active_alerts: Vec::new(),
            refresh_interval: RefreshInterval::TwoSeconds,
            ms_since_refresh: 0,
            status_message: String::new(),
        }
    }

    // ========================================================================
    // Data refresh
    // ========================================================================

    /// Refresh all data from the OS.
    ///
    /// In production this calls `OurOS` syscalls. The data vectors
    /// are populated externally or via `load_demo_data()` for testing.
    pub fn refresh(&mut self) {
        self.rebuild_visible_list();
        self.update_histories();
        self.check_alerts();
        self.update_status();
    }

    /// Rebuild the filtered and sorted visible process index list.
    pub fn rebuild_visible_list(&mut self) {
        self.visible_indices.clear();
        let filter_lower = self.filter_text.to_lowercase();

        for (i, proc) in self.processes.iter().enumerate() {
            if !filter_lower.is_empty()
                && !proc.name.to_lowercase().contains(&filter_lower)
                && !proc.pid.to_string().contains(&filter_lower)
            {
                continue;
            }
            self.visible_indices.push(i);
        }

        let processes = &self.processes;
        let col = self.sort_column;
        let dir = self.sort_direction;

        self.visible_indices.sort_by(|&a, &b| {
            let pa = match processes.get(a) {
                Some(p) => p,
                None => return std::cmp::Ordering::Equal,
            };
            let pb = match processes.get(b) {
                Some(p) => p,
                None => return std::cmp::Ordering::Equal,
            };

            let ord = match col {
                ProcessColumn::Pid => pa.pid.cmp(&pb.pid),
                ProcessColumn::Name => pa.name.to_lowercase().cmp(&pb.name.to_lowercase()),
                ProcessColumn::Status => pa.status.label().cmp(pb.status.label()),
                ProcessColumn::Cpu => pa
                    .cpu_percent
                    .partial_cmp(&pb.cpu_percent)
                    .unwrap_or(std::cmp::Ordering::Equal),
                ProcessColumn::Memory => pa.memory_bytes.cmp(&pb.memory_bytes),
                ProcessColumn::Threads => pa.thread_count.cmp(&pb.thread_count),
                ProcessColumn::Uptime => pa.uptime_secs.cmp(&pb.uptime_secs),
            };

            match dir {
                SortDirection::Ascending => ord,
                SortDirection::Descending => ord.reverse(),
            }
        });

        // Clamp selection.
        if let Some(sel) = self.selected_index
            && sel >= self.visible_indices.len()
        {
            self.selected_index = if self.visible_indices.is_empty() {
                None
            } else {
                Some(self.visible_indices.len().saturating_sub(1))
            };
        }
    }

    /// Push latest values into history ring buffers.
    fn update_histories(&mut self) {
        self.cpu_history.push(self.system_info.cpu_overall);

        let mem_percent = if self.system_info.total_memory > 0 {
            self.system_info.used_memory as f32 / self.system_info.total_memory as f32 * 100.0
        } else {
            0.0
        };
        self.mem_history.push(mem_percent);

        for core in &mut self.cores {
            core.history.push(core.usage_percent);
        }
        for disk in &mut self.disks {
            disk.read_history.push(disk.read_bytes_sec as f32);
            disk.write_history.push(disk.write_bytes_sec as f32);
        }
        for iface in &mut self.interfaces {
            iface.rx_history.push(iface.rx_bytes_sec as f32);
            iface.tx_history.push(iface.tx_bytes_sec as f32);
        }
    }

    /// Check alert thresholds and generate alerts.
    fn check_alerts(&mut self) {
        self.active_alerts.clear();

        // CPU alert
        if self.system_info.cpu_overall >= self.thresholds.cpu_crit_percent {
            self.active_alerts.push(Alert {
                message: format!("CPU usage critical: {:.1}%", self.system_info.cpu_overall),
                severity: AlertSeverity::Critical,
            });
        } else if self.system_info.cpu_overall >= self.thresholds.cpu_warn_percent {
            self.active_alerts.push(Alert {
                message: format!("CPU usage high: {:.1}%", self.system_info.cpu_overall),
                severity: AlertSeverity::Warning,
            });
        }

        // Memory alert
        let mem_percent = if self.system_info.total_memory > 0 {
            self.system_info.used_memory as f32 / self.system_info.total_memory as f32 * 100.0
        } else {
            0.0
        };
        if mem_percent >= self.thresholds.mem_crit_percent {
            self.active_alerts.push(Alert {
                message: format!("Memory usage critical: {mem_percent:.1}%"),
                severity: AlertSeverity::Critical,
            });
        } else if mem_percent >= self.thresholds.mem_warn_percent {
            self.active_alerts.push(Alert {
                message: format!("Memory usage high: {mem_percent:.1}%"),
                severity: AlertSeverity::Warning,
            });
        }

        // Disk alerts
        for disk in &self.disks {
            let usage_pct = disk.usage_fraction() * 100.0;
            if usage_pct >= self.thresholds.disk_crit_percent {
                self.active_alerts.push(Alert {
                    message: format!("Disk {} critical: {usage_pct:.1}%", disk.name),
                    severity: AlertSeverity::Critical,
                });
            } else if usage_pct >= self.thresholds.disk_warn_percent {
                self.active_alerts.push(Alert {
                    message: format!("Disk {} usage high: {usage_pct:.1}%", disk.name),
                    severity: AlertSeverity::Warning,
                });
            }
        }
    }

    /// Update status bar text.
    fn update_status(&mut self) {
        let total = self.processes.len();
        let running = self
            .processes
            .iter()
            .filter(|p| p.status == ProcessStatus::Running)
            .count();
        self.system_info.process_count = total as u32;
        self.system_info.running_count = running as u32;

        self.status_message = format!(
            "{total} processes ({running} running) | CPU: {:.1}% | Mem: {} / {} | Refresh: {}",
            self.system_info.cpu_overall,
            format_bytes(self.system_info.used_memory),
            format_bytes(self.system_info.total_memory),
            self.refresh_interval.label(),
        );
    }

    // ========================================================================
    // Actions
    // ========================================================================

    /// Kill the selected process.
    pub fn kill_selected(&mut self) {
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get(proc_idx)
        {
            let pid = proc.pid;
            let name = proc.name.clone();
            // In production: sys_process_kill(pid)
            self.status_message = format!("Killed process {name} (PID {pid})");
            self.processes.remove(proc_idx);
            self.rebuild_visible_list();
        }
    }

    /// Stop (pause) the selected process.
    pub fn stop_selected(&mut self) {
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get_mut(proc_idx)
        {
            proc.status = ProcessStatus::Stopped;
            self.status_message = format!("Stopped {} (PID {})", proc.name, proc.pid);
        }
    }

    /// Continue (resume) the selected process.
    pub fn continue_selected(&mut self) {
        if let Some(sel) = self.selected_index
            && let Some(&proc_idx) = self.visible_indices.get(sel)
            && let Some(proc) = self.processes.get_mut(proc_idx)
        {
            proc.status = ProcessStatus::Running;
            self.status_message = format!("Resumed {} (PID {})", proc.name, proc.pid);
        }
    }

    /// Set sort column; toggle direction if same column clicked again.
    pub fn set_sort_column(&mut self, col: ProcessColumn) {
        if self.sort_column == col {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = col;
            self.sort_direction = SortDirection::Ascending;
        }
        self.rebuild_visible_list();
    }

    /// Cycle the auto-refresh interval.
    pub fn cycle_refresh_interval(&mut self) {
        self.refresh_interval = self.refresh_interval.next();
        self.update_status();
    }

    /// Get the currently selected process.
    pub fn selected_process(&self) -> Option<&ProcessInfo> {
        let sel = self.selected_index?;
        let &proc_idx = self.visible_indices.get(sel)?;
        self.processes.get(proc_idx)
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse_ev) => self.handle_mouse(mouse_ev),
            Event::Resize { width, height } => {
                self.window_width = *width;
                self.window_height = *height;
                EventResult::Consumed
            }
            Event::Tick { elapsed_ms } => {
                self.ms_since_refresh = self.ms_since_refresh.saturating_add(*elapsed_ms);
                if self.ms_since_refresh >= self.refresh_interval.ms() {
                    self.ms_since_refresh = 0;
                    self.refresh();
                }
                EventResult::Consumed
            }
            Event::CloseRequested => EventResult::Ignored,
            _ => EventResult::Ignored,
        }
    }

    /// Handle a keyboard event.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        if self.filter_focused {
            return self.handle_filter_key(key);
        }

        match key.key {
            // Tab cycling
            Key::Num1 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Overview;
                EventResult::Consumed
            }
            Key::Num2 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Processes;
                EventResult::Consumed
            }
            Key::Num3 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Cpu;
                EventResult::Consumed
            }
            Key::Num4 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Memory;
                EventResult::Consumed
            }
            Key::Num5 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Disk;
                EventResult::Consumed
            }
            Key::Num6 if key.modifiers == Modifiers::NONE => {
                self.active_tab = Tab::Network;
                EventResult::Consumed
            }
            Key::Tab if key.modifiers == Modifiers::NONE => {
                self.cycle_tab_forward();
                EventResult::Consumed
            }
            Key::Tab if key.modifiers.shift => {
                self.cycle_tab_backward();
                EventResult::Consumed
            }
            // Process actions
            Key::Delete if key.modifiers == Modifiers::NONE => {
                self.kill_selected();
                EventResult::Consumed
            }
            Key::F5 => {
                self.refresh();
                self.status_message = "Refreshed".to_string();
                EventResult::Consumed
            }
            Key::F if key.modifiers.ctrl => {
                self.filter_focused = true;
                EventResult::Consumed
            }
            Key::R if key.modifiers.ctrl => {
                self.cycle_refresh_interval();
                EventResult::Consumed
            }
            // Navigation
            Key::Up if key.modifiers == Modifiers::NONE => {
                self.move_selection(-1);
                EventResult::Consumed
            }
            Key::Down if key.modifiers == Modifiers::NONE => {
                self.move_selection(1);
                EventResult::Consumed
            }
            Key::PageUp => {
                self.move_selection(-10);
                EventResult::Consumed
            }
            Key::PageDown => {
                self.move_selection(10);
                EventResult::Consumed
            }
            Key::Home => {
                self.selected_index = if self.visible_indices.is_empty() {
                    None
                } else {
                    Some(0)
                };
                self.scroll_offset = 0;
                EventResult::Consumed
            }
            Key::End => {
                self.selected_index = if self.visible_indices.is_empty() {
                    None
                } else {
                    Some(self.visible_indices.len().saturating_sub(1))
                };
                EventResult::Consumed
            }
            Key::Escape => {
                if self.context_menu.is_some() {
                    self.context_menu = None;
                } else if !self.filter_text.is_empty() {
                    self.filter_text.clear();
                    self.rebuild_visible_list();
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle keyboard input when the filter box is focused.
    fn handle_filter_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape | Key::Enter => {
                self.filter_focused = false;
                EventResult::Consumed
            }
            Key::Backspace => {
                self.filter_text.pop();
                self.rebuild_visible_list();
                EventResult::Consumed
            }
            _ => {
                if let Some(ch) = key.text
                    && (ch.is_ascii_graphic() || ch == ' ')
                {
                    self.filter_text.push(ch);
                    self.rebuild_visible_list();
                }
                EventResult::Consumed
            }
        }
    }

    /// Handle a mouse event.
    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        let mx = mouse.x;
        let my = mouse.y;

        // Context menu handling
        if let Some(menu) = self.context_menu.clone()
            && let MouseEventKind::Press(MouseButton::Left) = &mouse.kind
        {
            let menu_w = 180.0;
            let item_h = 24.0;
            let item_count = ContextAction::ALL.len() as f32;

            if mx >= menu.x
                && mx <= menu.x + menu_w
                && my >= menu.y
                && my <= menu.y + item_h * item_count
            {
                let index = ((my - menu.y) / item_h) as usize;
                if let Some(&action) = ContextAction::ALL.get(index) {
                    self.execute_context_action(action, menu.target_pid);
                }
            }
            self.context_menu = None;
            return EventResult::Consumed;
        }

        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.context_menu = None;

                // Tab bar click
                if my < TAB_BAR_HEIGHT {
                    let mut tab_x = 0.0f32;
                    for tab in &Tab::ALL {
                        let tab_w = tab.label().len() as f32 * 9.0 + 24.0;
                        if mx >= tab_x && mx < tab_x + tab_w {
                            self.active_tab = *tab;
                            return EventResult::Consumed;
                        }
                        tab_x += tab_w;
                    }
                    return EventResult::Consumed;
                }

                // Process tab: column header click
                if self.active_tab == Tab::Processes {
                    let content_y = TAB_BAR_HEIGHT;
                    if my >= content_y && my < content_y + HEADER_HEIGHT {
                        let mut col_x = 0.0f32;
                        for col in &ProcessColumn::ALL {
                            let cw = col.width();
                            if mx >= col_x && mx < col_x + cw {
                                self.set_sort_column(*col);
                                return EventResult::Consumed;
                            }
                            col_x += cw;
                        }
                        return EventResult::Consumed;
                    }

                    // Process row click
                    let rows_start = content_y + HEADER_HEIGHT;
                    if my >= rows_start {
                        let row_f = (my - rows_start) / ROW_HEIGHT;
                        let row_idx = (row_f as usize).saturating_add(self.scroll_offset);
                        if row_idx < self.visible_indices.len() {
                            self.selected_index = Some(row_idx);
                        }
                        return EventResult::Consumed;
                    }
                }

                EventResult::Consumed
            }

            MouseEventKind::Press(MouseButton::Right) => {
                if self.active_tab == Tab::Processes {
                    let content_y = TAB_BAR_HEIGHT + HEADER_HEIGHT;
                    if my >= content_y {
                        let row_f = (my - content_y) / ROW_HEIGHT;
                        let row_idx = (row_f as usize).saturating_add(self.scroll_offset);
                        if row_idx < self.visible_indices.len() {
                            self.selected_index = Some(row_idx);
                            if let Some(&proc_idx) = self.visible_indices.get(row_idx) {
                                let pid = self.processes.get(proc_idx).map_or(0, |p| p.pid);
                                self.context_menu = Some(ContextMenu {
                                    x: mx,
                                    y: my,
                                    target_pid: pid,
                                    hover_index: None,
                                });
                            }
                        }
                    }
                }
                EventResult::Consumed
            }

            MouseEventKind::Scroll { dy, .. } => {
                if *dy < 0.0 {
                    self.scroll_offset = self.scroll_offset.saturating_add(3);
                } else if *dy > 0.0 {
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                }
                let max_scroll = self.visible_indices.len().saturating_sub(1);
                if self.scroll_offset > max_scroll {
                    self.scroll_offset = max_scroll;
                }
                EventResult::Consumed
            }

            MouseEventKind::Move => {
                if self.active_tab == Tab::Processes {
                    let content_y = TAB_BAR_HEIGHT + HEADER_HEIGHT;
                    if my >= content_y {
                        let row_f = (my - content_y) / ROW_HEIGHT;
                        let row_idx = (row_f as usize).saturating_add(self.scroll_offset);
                        self.hovered_index = if row_idx < self.visible_indices.len() {
                            Some(row_idx)
                        } else {
                            None
                        };
                    } else {
                        self.hovered_index = None;
                    }
                }

                if let Some(menu) = &mut self.context_menu {
                    let menu_w = 180.0;
                    let item_h = 24.0;
                    let item_count = ContextAction::ALL.len() as f32;
                    if mx >= menu.x
                        && mx <= menu.x + menu_w
                        && my >= menu.y
                        && my <= menu.y + item_h * item_count
                    {
                        menu.hover_index = Some(((my - menu.y) / item_h) as usize);
                    } else {
                        menu.hover_index = None;
                    }
                }

                EventResult::Consumed
            }

            _ => EventResult::Ignored,
        }
    }

    /// Execute a context menu action on a target process.
    fn execute_context_action(&mut self, action: ContextAction, target_pid: u32) {
        let proc_idx = self.processes.iter().position(|p| p.pid == target_pid);
        match action {
            ContextAction::Kill => {
                if let Some(idx) = proc_idx {
                    let name = self
                        .processes
                        .get(idx)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.processes.remove(idx);
                    self.rebuild_visible_list();
                    self.status_message = format!("Killed {name} (PID {target_pid})");
                }
            }
            ContextAction::Stop => {
                if let Some(idx) = proc_idx
                    && let Some(proc) = self.processes.get_mut(idx)
                {
                    proc.status = ProcessStatus::Stopped;
                    self.status_message = format!("Stopped {} (PID {target_pid})", proc.name);
                }
            }
            ContextAction::Continue => {
                if let Some(idx) = proc_idx
                    && let Some(proc) = self.processes.get_mut(idx)
                {
                    proc.status = ProcessStatus::Running;
                    self.status_message = format!("Resumed {} (PID {target_pid})", proc.name);
                }
            }
            ContextAction::SetHighPriority
            | ContextAction::SetNormalPriority
            | ContextAction::SetLowPriority => {
                let level = action.label();
                self.status_message = format!("{level} for PID {target_pid} (not yet implemented)");
            }
        }
    }

    /// Cycle forward through tabs.
    fn cycle_tab_forward(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Overview => Tab::Processes,
            Tab::Processes => Tab::Cpu,
            Tab::Cpu => Tab::Memory,
            Tab::Memory => Tab::Disk,
            Tab::Disk => Tab::Network,
            Tab::Network => Tab::Overview,
        };
    }

    /// Cycle backward through tabs.
    fn cycle_tab_backward(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Overview => Tab::Network,
            Tab::Processes => Tab::Overview,
            Tab::Cpu => Tab::Processes,
            Tab::Memory => Tab::Cpu,
            Tab::Disk => Tab::Memory,
            Tab::Network => Tab::Disk,
        };
    }

    /// Move the selection by delta rows.
    fn move_selection(&mut self, delta: i32) {
        if self.visible_indices.is_empty() {
            return;
        }
        let current = self.selected_index.unwrap_or(0) as i32;
        let max_idx = (self.visible_indices.len() as i32).saturating_sub(1);
        let new_idx = (current.saturating_add(delta)).clamp(0, max_idx) as usize;
        self.selected_index = Some(new_idx);

        let visible_rows = self.visible_row_count();
        if new_idx < self.scroll_offset {
            self.scroll_offset = new_idx;
        } else if new_idx >= self.scroll_offset.saturating_add(visible_rows) {
            self.scroll_offset = new_idx.saturating_sub(visible_rows.saturating_sub(1));
        }
    }

    /// Number of process rows visible in the current window.
    fn visible_row_count(&self) -> usize {
        let content_h =
            self.window_height as f32 - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT - HEADER_HEIGHT;
        if content_h <= 0.0 {
            return 0;
        }
        (content_h / ROW_HEIGHT) as usize
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the complete system monitor UI.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();
        let w = self.window_width as f32;
        let h = self.window_height as f32;

        // Background
        tree.fill_rect(0.0, 0.0, w, h, BASE);

        // Tab bar
        self.render_tab_bar(&mut tree);

        // Content area (depends on active tab)
        match self.active_tab {
            Tab::Overview => self.render_overview_tab(&mut tree),
            Tab::Processes => self.render_processes_tab(&mut tree),
            Tab::Cpu => self.render_cpu_tab(&mut tree),
            Tab::Memory => self.render_memory_tab(&mut tree),
            Tab::Disk => self.render_disk_tab(&mut tree),
            Tab::Network => self.render_network_tab(&mut tree),
        }

        // Status bar
        self.render_status_bar(&mut tree);

        // Context menu overlay
        self.render_context_menu(&mut tree);

        tree
    }

    // -- Tab bar ----------------------------------------------------------------

    fn render_tab_bar(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        tree.fill_rect(0.0, 0.0, w, TAB_BAR_HEIGHT, MANTLE);

        let mut tx = 0.0f32;
        for tab in &Tab::ALL {
            let label = tab.label();
            let tab_w = label.len() as f32 * 9.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                tree.push(RenderCommand::FillRect {
                    x: tx,
                    y: 0.0,
                    width: tab_w,
                    height: TAB_BAR_HEIGHT,
                    color: BASE,
                    corner_radii: CornerRadii {
                        top_left: 4.0,
                        top_right: 4.0,
                        bottom_left: 0.0,
                        bottom_right: 0.0,
                    },
                });
                // Accent underline
                tree.fill_rect(tx, TAB_BAR_HEIGHT - 2.0, tab_w, 2.0, BLUE);
            }

            let text_color = if is_active { TEXT } else { SUBTEXT0 };
            let font_weight = if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            tree.push(RenderCommand::Text {
                x: tx + 12.0,
                y: 9.0,
                text: label.to_string(),
                color: text_color,
                font_size: 12.0,
                font_weight,
                max_width: None,
            });
            tx += tab_w;
        }

        // Refresh interval indicator (right-aligned)
        let ri_label = format!("Refresh: {}", self.refresh_interval.label());
        tree.push(RenderCommand::Text {
            x: w - 100.0,
            y: 9.0,
            text: ri_label,
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // -- Status bar ---------------------------------------------------------------

    fn render_status_bar(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let y = self.window_height as f32 - STATUS_BAR_HEIGHT;

        tree.fill_rect(0.0, y, w, STATUS_BAR_HEIGHT, CRUST);

        // Status message
        tree.push(RenderCommand::Text {
            x: 8.0,
            y: y + 5.0,
            text: self.status_message.clone(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w * 0.6),
        });

        // Alert indicator on the right
        if let Some(alert) = self.active_alerts.first() {
            let alert_x = w - 300.0;
            tree.push(RenderCommand::FillRect {
                x: alert_x - 4.0,
                y: y + 4.0,
                width: 8.0,
                height: 8.0,
                color: alert.severity.color(),
                corner_radii: CornerRadii::all(4.0),
            });
            tree.push(RenderCommand::Text {
                x: alert_x + 8.0,
                y: y + 5.0,
                text: alert.message.clone(),
                color: alert.severity.color(),
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(280.0),
            });
        }
    }

    // -- Overview tab ---------------------------------------------------------------

    fn render_overview_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT + CONTENT_PAD;
        let content_w = w - CONTENT_PAD * 2.0;

        // System info header card
        let header_h = 60.0;
        self.render_card(tree, CONTENT_PAD, content_y, content_w, header_h);
        render_bold_text(
            tree,
            CONTENT_PAD + 12.0,
            content_y + 8.0,
            &self.system_info.hostname,
            TEXT,
            14.0,
        );
        let info_line = format!(
            "{} | Kernel {} | {} | Up: {}",
            self.system_info.os_version,
            self.system_info.kernel_version,
            self.system_info.cpu_model,
            format_uptime(self.system_info.uptime_secs),
        );
        tree.push(RenderCommand::Text {
            x: CONTENT_PAD + 12.0,
            y: content_y + 28.0,
            text: info_line,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w - 24.0),
        });

        // Battery status if available
        if let Some(pct) = self.system_info.battery_percent {
            let batt_icon = if self.system_info.battery_charging {
                "Charging"
            } else {
                "Battery"
            };
            let batt_label = format!("{batt_icon}: {pct}%");
            let batt_color = if pct < 20 {
                RED
            } else if pct < 50 {
                YELLOW
            } else {
                GREEN
            };
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 12.0,
                y: content_y + 44.0,
                text: batt_label,
                color: batt_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        let mut cur_y = content_y + header_h + CARD_GAP;

        // Top row: CPU gauge + Memory gauge side by side
        let half_w = (content_w - CARD_GAP) / 2.0;
        let gauge_h = 120.0;

        // CPU gauge card
        self.render_card(tree, CONTENT_PAD, cur_y, half_w, gauge_h);
        render_bold_text(tree, CONTENT_PAD + 12.0, cur_y + 8.0, "CPU", TEXT, 13.0);
        let cpu_pct = self.system_info.cpu_overall;
        let cpu_color = self.thresholds.cpu_color(cpu_pct);
        let cpu_label = format!("{cpu_pct:.1}%");
        tree.push(RenderCommand::Text {
            x: CONTENT_PAD + 50.0,
            y: cur_y + 8.0,
            text: cpu_label,
            color: cpu_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // CPU mini graph
        let cpu_graph_x = CONTENT_PAD + 12.0;
        let cpu_graph_y = cur_y + 28.0;
        let cpu_graph_w = half_w - 24.0;
        let cpu_graph_h = gauge_h - 40.0;
        self.render_mini_graph(
            tree,
            cpu_graph_x,
            cpu_graph_y,
            cpu_graph_w,
            cpu_graph_h,
            &self.cpu_history,
            cpu_color,
            100.0,
        );

        // Load average below the graph
        let load_label = format!(
            "Load: {:.2} / {:.2} / {:.2}",
            self.system_info.load_avg[0],
            self.system_info.load_avg[1],
            self.system_info.load_avg[2],
        );
        tree.push(RenderCommand::Text {
            x: cpu_graph_x,
            y: cur_y + gauge_h - 14.0,
            text: load_label,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Memory gauge card
        let mem_card_x = CONTENT_PAD + half_w + CARD_GAP;
        self.render_card(tree, mem_card_x, cur_y, half_w, gauge_h);
        render_bold_text(tree, mem_card_x + 12.0, cur_y + 8.0, "Memory", TEXT, 13.0);

        let mem_pct = if self.system_info.total_memory > 0 {
            self.system_info.used_memory as f32 / self.system_info.total_memory as f32 * 100.0
        } else {
            0.0
        };
        let mem_color = self.thresholds.mem_color(mem_pct);
        let mem_label = format!(
            "{} / {}  ({mem_pct:.1}%)",
            format_bytes(self.system_info.used_memory),
            format_bytes(self.system_info.total_memory),
        );
        tree.push(RenderCommand::Text {
            x: mem_card_x + 80.0,
            y: cur_y + 8.0,
            text: mem_label,
            color: mem_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Memory mini graph
        let mem_graph_x = mem_card_x + 12.0;
        let mem_graph_y = cur_y + 28.0;
        let mem_graph_w = half_w - 24.0;
        let mem_graph_h = gauge_h - 40.0;
        self.render_mini_graph(
            tree,
            mem_graph_x,
            mem_graph_y,
            mem_graph_w,
            mem_graph_h,
            &self.mem_history,
            mem_color,
            100.0,
        );

        // Swap info
        let swap_label = format!(
            "Swap: {} / {}",
            format_bytes(self.system_info.swap_used),
            format_bytes(self.system_info.swap_total),
        );
        tree.push(RenderCommand::Text {
            x: mem_graph_x,
            y: cur_y + gauge_h - 14.0,
            text: swap_label,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cur_y += gauge_h + CARD_GAP;

        // Bottom row: Disk + Network summaries
        let disk_card_h = 90.0;

        // Disk summary card
        self.render_card(tree, CONTENT_PAD, cur_y, half_w, disk_card_h);
        render_bold_text(tree, CONTENT_PAD + 12.0, cur_y + 8.0, "Disks", TEXT, 13.0);
        let mut disk_y = cur_y + 26.0;
        for disk in self.disks.iter().take(3) {
            let usage_pct = disk.usage_fraction() * 100.0;
            let disk_color = self.thresholds.disk_color(usage_pct);
            let label = format!(
                "{}: {usage_pct:.0}% ({} / {})",
                disk.name,
                format_bytes(disk.used_bytes),
                format_bytes(disk.total_bytes),
            );
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 16.0,
                y: disk_y,
                text: label,
                color: disk_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(half_w - 32.0),
            });
            disk_y += 16.0;
        }

        // Network summary card
        let net_card_x = CONTENT_PAD + half_w + CARD_GAP;
        self.render_card(tree, net_card_x, cur_y, half_w, disk_card_h);
        render_bold_text(tree, net_card_x + 12.0, cur_y + 8.0, "Network", TEXT, 13.0);
        let mut net_y = cur_y + 26.0;
        for iface in self.interfaces.iter().take(3) {
            let label = format!(
                "{}: RX {} TX {} | {} conn",
                iface.name,
                format_rate(iface.rx_bytes_sec),
                format_rate(iface.tx_bytes_sec),
                iface.connection_count,
            );
            tree.push(RenderCommand::Text {
                x: net_card_x + 16.0,
                y: net_y,
                text: label,
                color: SUBTEXT1,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(half_w - 32.0),
            });
            net_y += 16.0;
        }

        cur_y += disk_card_h + CARD_GAP;

        // Alert panel
        if !self.active_alerts.is_empty() {
            let alert_h = 16.0f32
                .mul_add(self.active_alerts.len() as f32, 28.0)
                .min(120.0);
            self.render_card(tree, CONTENT_PAD, cur_y, content_w, alert_h);
            render_bold_text(tree, CONTENT_PAD + 12.0, cur_y + 8.0, "Alerts", RED, 13.0);
            let mut alert_y = cur_y + 26.0;
            for alert in self.active_alerts.iter().take(5) {
                tree.push(RenderCommand::FillRect {
                    x: CONTENT_PAD + 16.0,
                    y: alert_y + 3.0,
                    width: 6.0,
                    height: 6.0,
                    color: alert.severity.color(),
                    corner_radii: CornerRadii::all(3.0),
                });
                tree.push(RenderCommand::Text {
                    x: CONTENT_PAD + 28.0,
                    y: alert_y,
                    text: alert.message.clone(),
                    color: alert.severity.color(),
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w - 48.0),
                });
                alert_y += 16.0;
            }
        }
    }

    // -- Processes tab ---------------------------------------------------------------

    fn render_processes_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT;
        let content_h = self.window_height as f32 - content_y - STATUS_BAR_HEIGHT;

        // Filter box
        let filter_w = 220.0;
        let filter_x = w - filter_w - 8.0;
        let filter_h = HEADER_HEIGHT - 2.0;
        let filter_border = if self.filter_focused { BLUE } else { SURFACE1 };
        tree.push(RenderCommand::StrokeRect {
            x: filter_x,
            y: content_y + 1.0,
            width: filter_w,
            height: filter_h,
            color: filter_border,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::FillRect {
            x: filter_x + 1.0,
            y: content_y + 2.0,
            width: filter_w - 2.0,
            height: filter_h - 2.0,
            color: CRUST,
            corner_radii: CornerRadii::all(2.0),
        });

        let filter_display = if self.filter_text.is_empty() {
            "Filter (Ctrl+F)"
        } else {
            &self.filter_text
        };
        let filter_text_color = if self.filter_text.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };
        tree.push(RenderCommand::Text {
            x: filter_x + 8.0,
            y: content_y + 6.0,
            text: filter_display.to_string(),
            color: filter_text_color,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(filter_w - 16.0),
        });
        if self.filter_focused {
            let cursor_x = filter_x + 8.0 + self.filter_text.len() as f32 * 7.0;
            tree.fill_rect(cursor_x, content_y + 5.0, 1.0, filter_h - 8.0, TEXT);
        }

        // Column headers
        tree.fill_rect(0.0, content_y, w - filter_w - 16.0, HEADER_HEIGHT, MANTLE);
        let mut col_x = 0.0f32;
        for col in &ProcessColumn::ALL {
            let cw = col.width();
            let label = col.label();

            let display = if *col == self.sort_column {
                let arrow = match self.sort_direction {
                    SortDirection::Ascending => " \u{25B2}",
                    SortDirection::Descending => " \u{25BC}",
                };
                format!("{label}{arrow}")
            } else {
                label.to_string()
            };

            let label_color = if *col == self.sort_column {
                BLUE
            } else {
                SUBTEXT0
            };
            tree.push(RenderCommand::Text {
                x: col_x + 6.0,
                y: content_y + 5.0,
                text: display,
                color: label_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            tree.fill_rect(
                col_x + cw - 1.0,
                content_y + 2.0,
                1.0,
                HEADER_HEIGHT - 4.0,
                SURFACE0,
            );
            col_x += cw;
        }

        // Process rows
        let rows_y = content_y + HEADER_HEIGHT;
        let row_area_h = content_h - HEADER_HEIGHT;
        let visible_rows = if row_area_h > 0.0 {
            (row_area_h / ROW_HEIGHT) as usize
        } else {
            0
        };

        tree.clip(0.0, rows_y, w, row_area_h);

        for vis_i in 0..visible_rows {
            let row_idx = vis_i.saturating_add(self.scroll_offset);
            let proc_vec_idx = match self.visible_indices.get(row_idx) {
                Some(&idx) => idx,
                None => break,
            };
            let proc = match self.processes.get(proc_vec_idx) {
                Some(p) => p,
                None => continue,
            };

            let ry = rows_y + vis_i as f32 * ROW_HEIGHT;

            let bg = if self.selected_index == Some(row_idx) {
                SURFACE1
            } else if self.hovered_index == Some(row_idx) {
                SURFACE0
            } else if row_idx % 2 == 0 {
                BASE
            } else {
                Color::from_hex(0x1A1A2E)
            };
            tree.fill_rect(0.0, ry, w, ROW_HEIGHT, bg);

            let mut cx = 0.0f32;
            for col in &ProcessColumn::ALL {
                let cw = col.width();
                match col {
                    ProcessColumn::Pid => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: proc.pid.to_string(),
                            color: OVERLAY0,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                    ProcessColumn::Name => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: proc.name.clone(),
                            color: TEXT,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(cw - 12.0),
                        });
                    }
                    ProcessColumn::Status => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: proc.status.label().to_string(),
                            color: proc.status.color(),
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                    ProcessColumn::Cpu => {
                        let cpu_str = format!("{:.1}", proc.cpu_percent);
                        let cpu_color = self.thresholds.cpu_color(proc.cpu_percent);
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: cpu_str,
                            color: cpu_color,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                    ProcessColumn::Memory => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: format_bytes(proc.memory_bytes),
                            color: TEXT,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                    ProcessColumn::Threads => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: proc.thread_count.to_string(),
                            color: OVERLAY0,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                    ProcessColumn::Uptime => {
                        tree.push(RenderCommand::Text {
                            x: cx + 6.0,
                            y: ry + 4.0,
                            text: format_uptime_short(proc.uptime_secs),
                            color: OVERLAY0,
                            font_size: 11.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                }
                cx += cw;
            }
        }

        tree.unclip();
    }

    // -- CPU tab ---------------------------------------------------------------

    fn render_cpu_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT + CONTENT_PAD;
        let content_w = w - CONTENT_PAD * 2.0;

        // Overall CPU graph
        let graph_h = 160.0;
        self.render_card(tree, CONTENT_PAD, content_y, content_w, graph_h + 30.0);
        render_bold_text(
            tree,
            CONTENT_PAD + 12.0,
            content_y + 8.0,
            "CPU Usage",
            TEXT,
            13.0,
        );
        let cpu_label = format!("{:.1}%", self.system_info.cpu_overall);
        let cpu_color = self.thresholds.cpu_color(self.system_info.cpu_overall);
        tree.push(RenderCommand::Text {
            x: CONTENT_PAD + 100.0,
            y: content_y + 8.0,
            text: cpu_label,
            color: cpu_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let chart_x = CONTENT_PAD + 12.0;
        let chart_y = content_y + 28.0;
        let chart_w = content_w - 24.0;
        self.render_graph_area(
            tree,
            chart_x,
            chart_y,
            chart_w,
            graph_h,
            &self.cpu_history,
            cpu_color,
            100.0,
        );

        let cur_y = content_y + graph_h + 30.0 + CARD_GAP;

        // Per-core details
        let core_card_h = 24.0f32.mul_add(self.cores.len() as f32, 30.0).min(300.0);
        self.render_card(tree, CONTENT_PAD, cur_y, content_w, core_card_h);
        render_bold_text(
            tree,
            CONTENT_PAD + 12.0,
            cur_y + 8.0,
            "Per-Core Details",
            TEXT,
            13.0,
        );

        let bar_start_x = CONTENT_PAD + 120.0;
        let bar_w = content_w - 300.0;
        let mut core_y = cur_y + 28.0;

        for core in &self.cores {
            // Core label
            let core_label = format!("Core {}", core.core_id);
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 16.0,
                y: core_y + 2.0,
                text: core_label,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Usage bar background
            tree.push(RenderCommand::FillRect {
                x: bar_start_x,
                y: core_y,
                width: bar_w,
                height: 16.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(3.0),
            });

            // Usage bar fill
            let fill_w = bar_w * (core.usage_percent / 100.0);
            let bar_color = self.thresholds.cpu_color(core.usage_percent);
            if fill_w > 0.5 {
                tree.push(RenderCommand::FillRect {
                    x: bar_start_x,
                    y: core_y,
                    width: fill_w,
                    height: 16.0,
                    color: bar_color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Usage percentage + frequency + temperature
            let detail = format!(
                "{:.0}%  {} MHz  {:.0}\u{00B0}C",
                core.usage_percent, core.frequency_mhz, core.temperature_c,
            );
            tree.push(RenderCommand::Text {
                x: bar_start_x + bar_w + 12.0,
                y: core_y + 2.0,
                text: detail,
                color: SUBTEXT1,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            core_y += 24.0;
        }
    }

    // -- Memory tab ---------------------------------------------------------------

    fn render_memory_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT + CONTENT_PAD;
        let content_w = w - CONTENT_PAD * 2.0;

        // Memory usage graph
        let graph_h = 160.0;
        self.render_card(tree, CONTENT_PAD, content_y, content_w, graph_h + 30.0);

        let mem_pct = if self.system_info.total_memory > 0 {
            self.system_info.used_memory as f32 / self.system_info.total_memory as f32 * 100.0
        } else {
            0.0
        };
        render_bold_text(
            tree,
            CONTENT_PAD + 12.0,
            content_y + 8.0,
            "Memory Usage",
            TEXT,
            13.0,
        );
        let mem_label = format!("{mem_pct:.1}%");
        let mem_color = self.thresholds.mem_color(mem_pct);
        tree.push(RenderCommand::Text {
            x: CONTENT_PAD + 120.0,
            y: content_y + 8.0,
            text: mem_label,
            color: mem_color,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let chart_x = CONTENT_PAD + 12.0;
        let chart_y = content_y + 28.0;
        let chart_w = content_w - 24.0;
        self.render_graph_area(
            tree,
            chart_x,
            chart_y,
            chart_w,
            graph_h,
            &self.mem_history,
            mem_color,
            100.0,
        );

        let cur_y = content_y + graph_h + 30.0 + CARD_GAP;

        // Memory breakdown card
        let breakdown_h = 130.0;
        self.render_card(tree, CONTENT_PAD, cur_y, content_w, breakdown_h);
        render_bold_text(
            tree,
            CONTENT_PAD + 12.0,
            cur_y + 8.0,
            "Breakdown",
            TEXT,
            13.0,
        );

        // Stacked bar
        let bar_x = CONTENT_PAD + 16.0;
        let bar_y = cur_y + 30.0;
        let bar_w = content_w - 32.0;
        let bar_h = 24.0;
        let total = self.system_info.total_memory.max(1) as f32;

        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        let segments: &[(&str, u64, Color)] = &[
            ("Used", self.system_info.used_memory, BLUE),
            ("Cached", self.system_info.cached_memory, TEAL),
            ("Buffers", self.system_info.buffers, MAUVE),
            ("Free", self.system_info.free_memory, SURFACE1),
        ];

        let mut fill_x = bar_x;
        for &(_label, amount, color) in segments {
            let frac = amount as f32 / total;
            let fill_w = bar_w * frac;
            if fill_w > 0.5 {
                tree.push(RenderCommand::FillRect {
                    x: fill_x,
                    y: bar_y,
                    width: fill_w,
                    height: bar_h,
                    color,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            fill_x += fill_w;
        }

        // Legend
        let legend_y = bar_y + bar_h + 10.0;
        let mut legend_x = bar_x;
        for &(label, amount, color) in segments {
            tree.push(RenderCommand::FillRect {
                x: legend_x,
                y: legend_y + 2.0,
                width: 10.0,
                height: 10.0,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
            let entry = format!("{label}: {}", format_bytes(amount));
            tree.push(RenderCommand::Text {
                x: legend_x + 14.0,
                y: legend_y,
                text: entry,
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            legend_x += 160.0;
        }

        // Swap section
        let swap_y = legend_y + 24.0;
        let swap_total = self.system_info.swap_total.max(1) as f32;
        let swap_frac = self.system_info.swap_used as f32 / swap_total;
        tree.push(RenderCommand::Text {
            x: bar_x,
            y: swap_y,
            text: format!(
                "Swap: {} / {}",
                format_bytes(self.system_info.swap_used),
                format_bytes(self.system_info.swap_total),
            ),
            color: SUBTEXT1,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let swap_bar_y = swap_y + 16.0;
        tree.push(RenderCommand::FillRect {
            x: bar_x,
            y: swap_bar_y,
            width: bar_w,
            height: 12.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        let swap_fill = bar_w * swap_frac;
        if swap_fill > 0.5 {
            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: swap_bar_y,
                width: swap_fill,
                height: 12.0,
                color: PEACH,
                corner_radii: CornerRadii::all(3.0),
            });
        }
    }

    // -- Disk tab ---------------------------------------------------------------

    fn render_disk_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT + CONTENT_PAD;
        let content_w = w - CONTENT_PAD * 2.0;
        let mut cur_y = content_y;

        for disk in &self.disks {
            let card_h = 140.0;
            self.render_card(tree, CONTENT_PAD, cur_y, content_w, card_h);

            // Disk name and mount point
            let title = format!("{} ({})", disk.name, disk.mount_point);
            render_bold_text(tree, CONTENT_PAD + 12.0, cur_y + 8.0, &title, TEXT, 13.0);

            // Usage bar
            let bar_x = CONTENT_PAD + 16.0;
            let bar_y = cur_y + 28.0;
            let bar_w = content_w - 200.0;
            let bar_h = 18.0;
            let usage_pct = disk.usage_fraction() * 100.0;
            let disk_color = self.thresholds.disk_color(usage_pct);

            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: bar_w,
                height: bar_h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            let fill_w = bar_w * disk.usage_fraction();
            if fill_w > 0.5 {
                tree.push(RenderCommand::FillRect {
                    x: bar_x,
                    y: bar_y,
                    width: fill_w,
                    height: bar_h,
                    color: disk_color,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let usage_label = format!(
                "{usage_pct:.1}%  ({} / {})",
                format_bytes(disk.used_bytes),
                format_bytes(disk.total_bytes),
            );
            tree.push(RenderCommand::Text {
                x: bar_x + bar_w + 12.0,
                y: bar_y + 2.0,
                text: usage_label,
                color: disk_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // I/O rates
            let io_y = bar_y + bar_h + 10.0;
            let io_label = format!(
                "Read: {}/s ({} IOPS)  |  Write: {}/s ({} IOPS)",
                format_bytes(disk.read_bytes_sec),
                disk.read_iops,
                format_bytes(disk.write_bytes_sec),
                disk.write_iops,
            );
            tree.push(RenderCommand::Text {
                x: bar_x,
                y: io_y,
                text: io_label,
                color: SUBTEXT1,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 32.0),
            });

            // I/O graph
            let io_graph_y = io_y + 18.0;
            let io_graph_w = (content_w - 40.0) / 2.0;
            let io_graph_h = 50.0;

            let max_io = disk
                .read_history
                .max_value()
                .max(disk.write_history.max_value())
                .max(1.0);

            self.render_mini_graph(
                tree,
                bar_x,
                io_graph_y,
                io_graph_w,
                io_graph_h,
                &disk.read_history,
                GREEN,
                max_io,
            );
            self.render_mini_graph(
                tree,
                bar_x + io_graph_w + 8.0,
                io_graph_y,
                io_graph_w,
                io_graph_h,
                &disk.write_history,
                PEACH,
                max_io,
            );

            // Legend under graphs
            tree.push(RenderCommand::FillRect {
                x: bar_x,
                y: io_graph_y + io_graph_h + 2.0,
                width: 8.0,
                height: 8.0,
                color: GREEN,
                corner_radii: CornerRadii::all(2.0),
            });
            tree.push(RenderCommand::Text {
                x: bar_x + 12.0,
                y: io_graph_y + io_graph_h,
                text: "Read".to_string(),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::FillRect {
                x: bar_x + io_graph_w + 8.0,
                y: io_graph_y + io_graph_h + 2.0,
                width: 8.0,
                height: 8.0,
                color: PEACH,
                corner_radii: CornerRadii::all(2.0),
            });
            tree.push(RenderCommand::Text {
                x: bar_x + io_graph_w + 20.0,
                y: io_graph_y + io_graph_h,
                text: "Write".to_string(),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cur_y += card_h + CARD_GAP;
        }

        if self.disks.is_empty() {
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 16.0,
                y: content_y + 20.0,
                text: "No disk information available.".to_string(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // -- Network tab ---------------------------------------------------------------

    fn render_network_tab(&self, tree: &mut RenderTree) {
        let w = self.window_width as f32;
        let content_y = TAB_BAR_HEIGHT + CONTENT_PAD;
        let content_w = w - CONTENT_PAD * 2.0;
        let mut cur_y = content_y;

        for iface in &self.interfaces {
            let card_h = 130.0;
            self.render_card(tree, CONTENT_PAD, cur_y, content_w, card_h);

            // Interface name and connection count
            let title = format!("{} ({} connections)", iface.name, iface.connection_count);
            render_bold_text(tree, CONTENT_PAD + 12.0, cur_y + 8.0, &title, TEXT, 13.0);

            // Traffic stats
            let stats_y = cur_y + 28.0;
            let stats = format!(
                "RX: {}/s ({} pkt/s)  |  TX: {}/s ({} pkt/s)",
                format_rate(iface.rx_bytes_sec),
                iface.rx_packets_sec,
                format_rate(iface.tx_bytes_sec),
                iface.tx_packets_sec,
            );
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 16.0,
                y: stats_y,
                text: stats,
                color: SUBTEXT1,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 32.0),
            });

            // Traffic graph
            let graph_y = stats_y + 20.0;
            let graph_w = (content_w - 40.0) / 2.0;
            let graph_h = 60.0;

            let max_traffic = iface
                .rx_history
                .max_value()
                .max(iface.tx_history.max_value())
                .max(1.0);

            self.render_mini_graph(
                tree,
                CONTENT_PAD + 16.0,
                graph_y,
                graph_w,
                graph_h,
                &iface.rx_history,
                SKY,
                max_traffic,
            );
            self.render_mini_graph(
                tree,
                CONTENT_PAD + 24.0 + graph_w,
                graph_y,
                graph_w,
                graph_h,
                &iface.tx_history,
                PINK,
                max_traffic,
            );

            // Legend
            tree.push(RenderCommand::FillRect {
                x: CONTENT_PAD + 16.0,
                y: graph_y + graph_h + 2.0,
                width: 8.0,
                height: 8.0,
                color: SKY,
                corner_radii: CornerRadii::all(2.0),
            });
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 28.0,
                y: graph_y + graph_h,
                text: "RX".to_string(),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::FillRect {
                x: CONTENT_PAD + 24.0 + graph_w,
                y: graph_y + graph_h + 2.0,
                width: 8.0,
                height: 8.0,
                color: PINK,
                corner_radii: CornerRadii::all(2.0),
            });
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 36.0 + graph_w,
                y: graph_y + graph_h,
                text: "TX".to_string(),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cur_y += card_h + CARD_GAP;
        }

        if self.interfaces.is_empty() {
            tree.push(RenderCommand::Text {
                x: CONTENT_PAD + 16.0,
                y: content_y + 20.0,
                text: "No network interface information available.".to_string(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // -- Context menu ---------------------------------------------------------------

    fn render_context_menu(&self, tree: &mut RenderTree) {
        let menu = match &self.context_menu {
            Some(m) => m,
            None => return,
        };

        let menu_w = 180.0;
        let item_h = 24.0;
        let item_count = ContextAction::ALL.len() as f32;
        let menu_h = item_h * item_count;

        // Shadow
        tree.push(RenderCommand::FillRect {
            x: menu.x + 2.0,
            y: menu.y + 2.0,
            width: menu_w,
            height: menu_h,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(4.0),
        });

        // Background
        tree.push(RenderCommand::FillRect {
            x: menu.x,
            y: menu.y,
            width: menu_w,
            height: menu_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: menu.x,
            y: menu.y,
            width: menu_w,
            height: menu_h,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        for (i, action) in ContextAction::ALL.iter().enumerate() {
            let iy = menu.y + i as f32 * item_h;

            if menu.hover_index == Some(i) {
                tree.push(RenderCommand::FillRect {
                    x: menu.x + 2.0,
                    y: iy,
                    width: menu_w - 4.0,
                    height: item_h,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            let text_color = if *action == ContextAction::Kill {
                RED
            } else {
                TEXT
            };
            tree.push(RenderCommand::Text {
                x: menu.x + 12.0,
                y: iy + 5.0,
                text: action.label().to_string(),
                color: text_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // ========================================================================
    // Drawing helpers
    // ========================================================================

    /// Render a card (rounded rect with dark background + border).
    fn render_card(&self, tree: &mut RenderTree, x: f32, y: f32, w: f32, h: f32) {
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MANTLE,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CARD_RADIUS),
        });
    }

    /// Render a mini graph (no grid, just the line) within an area.
    fn render_mini_graph(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        history: &GraphHistory,
        color: Color,
        max_value: f32,
    ) {
        // Background
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: CRUST,
            corner_radii: CornerRadii::all(3.0),
        });

        render_line_graph(tree, x, y, w, h, history, color, max_value);
    }

    /// Render a full graph area with grid lines and labels.
    fn render_graph_area(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        history: &GraphHistory,
        color: Color,
        max_value: f32,
    ) {
        // Background
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Grid lines at 25%, 50%, 75%
        for &pct in &[25.0f32, 50.0, 75.0] {
            let gy = y + h * (1.0 - pct / 100.0);
            render_dashed_hline(tree, x + 1.0, gy, w - 2.0, SURFACE0);
            let pct_label = format!("{:.0}%", pct / 100.0 * max_value);
            tree.push(RenderCommand::Text {
                x: x + 4.0,
                y: gy - 10.0,
                text: pct_label,
                color: OVERLAY0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        render_line_graph(tree, x, y, w, h, history, color, max_value);
    }

    // ========================================================================
    // Demo data
    // ========================================================================

    /// Populate with sample data for UI testing.
    pub fn load_demo_data(&mut self) {
        self.system_info = SystemInfo {
            hostname: "ouros-desktop".to_string(),
            os_version: "OurOS 0.1.0".to_string(),
            kernel_version: "0.1.0-alpha".to_string(),
            cpu_model: "x86_64 4-core @ 3.6GHz".to_string(),
            uptime_secs: 86472,
            total_memory: 8_589_934_592,
            used_memory: 3_435_973_837,
            free_memory: 3_221_225_472,
            cached_memory: 1_610_612_736,
            buffers: 322_122_547,
            swap_total: 2_147_483_648,
            swap_used: 104_857_600,
            cpu_overall: 33.0,
            load_avg: [1.23, 0.98, 0.87],
            process_count: 0,
            running_count: 0,
            battery_percent: Some(72),
            battery_charging: false,
        };

        self.cores = vec![
            CpuCoreInfo {
                core_id: 0,
                usage_percent: 12.0,
                frequency_mhz: 3600,
                temperature_c: 52.0,
                history: GraphHistory::new(),
            },
            CpuCoreInfo {
                core_id: 1,
                usage_percent: 45.0,
                frequency_mhz: 3600,
                temperature_c: 58.0,
                history: GraphHistory::new(),
            },
            CpuCoreInfo {
                core_id: 2,
                usage_percent: 8.0,
                frequency_mhz: 2400,
                temperature_c: 48.0,
                history: GraphHistory::new(),
            },
            CpuCoreInfo {
                core_id: 3,
                usage_percent: 67.0,
                frequency_mhz: 3600,
                temperature_c: 64.0,
                history: GraphHistory::new(),
            },
        ];

        self.disks = vec![
            DiskInfo {
                name: "sda".to_string(),
                mount_point: "/".to_string(),
                total_bytes: 256_000_000_000,
                used_bytes: 178_000_000_000,
                read_bytes_sec: 52_428_800,
                write_bytes_sec: 10_485_760,
                read_iops: 1200,
                write_iops: 350,
                read_history: GraphHistory::new(),
                write_history: GraphHistory::new(),
            },
            DiskInfo {
                name: "sdb".to_string(),
                mount_point: "/home".to_string(),
                total_bytes: 1_000_000_000_000,
                used_bytes: 420_000_000_000,
                read_bytes_sec: 5_242_880,
                write_bytes_sec: 2_621_440,
                read_iops: 200,
                write_iops: 80,
                read_history: GraphHistory::new(),
                write_history: GraphHistory::new(),
            },
        ];

        self.interfaces = vec![
            NetworkInterface {
                name: "eth0".to_string(),
                tx_bytes_sec: 125_000,
                rx_bytes_sec: 2_500_000,
                tx_packets_sec: 450,
                rx_packets_sec: 3200,
                connection_count: 42,
                tx_history: GraphHistory::new(),
                rx_history: GraphHistory::new(),
            },
            NetworkInterface {
                name: "lo".to_string(),
                tx_bytes_sec: 50_000,
                rx_bytes_sec: 50_000,
                tx_packets_sec: 120,
                rx_packets_sec: 120,
                connection_count: 8,
                tx_history: GraphHistory::new(),
                rx_history: GraphHistory::new(),
            },
        ];

        self.processes = vec![
            make_demo_process(1, "init", ProcessStatus::Running, 0.1, 4_194_304, 2, 86472),
            make_demo_process(2, "kthread", ProcessStatus::Sleeping, 0.0, 0, 1, 86470),
            make_demo_process(
                100,
                "compositor",
                ProcessStatus::Running,
                8.5,
                67_108_864,
                6,
                85000,
            ),
            make_demo_process(
                101,
                "netd",
                ProcessStatus::Sleeping,
                0.3,
                12_582_912,
                4,
                85000,
            ),
            make_demo_process(
                200,
                "desktop",
                ProcessStatus::Running,
                3.2,
                104_857_600,
                12,
                82000,
            ),
            make_demo_process(
                201,
                "explorer",
                ProcessStatus::Running,
                1.1,
                52_428_800,
                4,
                80000,
            ),
            make_demo_process(
                202,
                "terminal",
                ProcessStatus::Sleeping,
                0.4,
                20_971_520,
                3,
                70000,
            ),
            make_demo_process(
                203,
                "editor",
                ProcessStatus::Running,
                12.7,
                157_286_400,
                8,
                50000,
            ),
            make_demo_process(
                300,
                "httpd",
                ProcessStatus::Running,
                2.1,
                33_554_432,
                16,
                60000,
            ),
            make_demo_process(
                301,
                "httpd-worker",
                ProcessStatus::Running,
                5.4,
                16_777_216,
                1,
                60000,
            ),
            make_demo_process(
                302,
                "httpd-worker",
                ProcessStatus::Sleeping,
                0.0,
                16_777_216,
                1,
                60000,
            ),
            make_demo_process(
                400,
                "sshd",
                ProcessStatus::Sleeping,
                0.0,
                8_388_608,
                1,
                85000,
            ),
            make_demo_process(
                500,
                "sysmonitor",
                ProcessStatus::Running,
                1.8,
                28_311_552,
                3,
                1200,
            ),
            make_demo_process(501, "zombie_proc", ProcessStatus::Zombie, 0.0, 0, 0, 30000),
            make_demo_process(
                600,
                "dhcpd",
                ProcessStatus::Sleeping,
                0.1,
                6_291_456,
                2,
                85000,
            ),
        ];

        // Push initial history data
        let cpu_samples = [
            20.0, 25.0, 22.0, 30.0, 35.0, 28.0, 40.0, 38.0, 33.0, 36.0, 42.0, 38.0, 35.0, 30.0,
            28.0, 25.0, 30.0, 33.0, 37.0, 33.0,
        ];
        for &s in &cpu_samples {
            self.cpu_history.push(s);
        }

        let mem_samples = [
            35.0, 36.0, 38.0, 37.0, 39.0, 40.0, 42.0, 41.0, 40.0, 39.0, 38.0, 39.0, 40.0, 41.0,
            42.0, 40.0, 39.0, 38.0, 39.0, 40.0,
        ];
        for &s in &mem_samples {
            self.mem_history.push(s);
        }

        let net_rx_samples = [
            100_000.0, 150_000.0, 200_000.0, 180_000.0, 300_000.0, 500_000.0, 450_000.0, 350_000.0,
            200_000.0, 150_000.0,
        ];
        let net_tx_samples = [
            50_000.0, 80_000.0, 120_000.0, 100_000.0, 200_000.0, 180_000.0, 160_000.0, 130_000.0,
            90_000.0, 70_000.0,
        ];
        if let Some(iface) = self.interfaces.get_mut(0) {
            for &s in &net_rx_samples {
                iface.rx_history.push(s);
            }
            for &s in &net_tx_samples {
                iface.tx_history.push(s);
            }
        }

        let disk_read_samples = [
            50_000_000.0,
            45_000_000.0,
            52_000_000.0,
            48_000_000.0,
            55_000_000.0,
            60_000_000.0,
            52_000_000.0,
            47_000_000.0,
        ];
        let disk_write_samples = [
            10_000_000.0,
            12_000_000.0,
            8_000_000.0,
            15_000_000.0,
            11_000_000.0,
            9_000_000.0,
            13_000_000.0,
            10_000_000.0,
        ];
        if let Some(disk) = self.disks.get_mut(0) {
            for &s in &disk_read_samples {
                disk.read_history.push(s);
            }
            for &s in &disk_write_samples {
                disk.write_history.push(s);
            }
        }

        self.refresh();
    }
}

impl Default for SysMonitorState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Free-standing drawing helpers
// ============================================================================

/// Render bold text.
fn render_bold_text(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    text: &str,
    color: Color,
    font_size: f32,
) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: text.to_string(),
        color,
        font_size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Render a line graph from a `GraphHistory` into a rectangular area.
fn render_line_graph(
    tree: &mut RenderTree,
    area_x: f32,
    area_y: f32,
    area_w: f32,
    area_h: f32,
    history: &GraphHistory,
    color: Color,
    max_value: f32,
) {
    let count = history.len();
    if count < 2 {
        return;
    }

    let max_val = if max_value > 0.0 { max_value } else { 1.0 };
    let samples: Vec<f32> = history.iter_oldest_first().collect();
    let step_x = area_w / (GRAPH_HISTORY_LEN as f32 - 1.0);

    let first_sample = samples.first().copied().unwrap_or(0.0);
    let mut prev_x = area_x;
    let mut prev_y = area_y + area_h - (first_sample / max_val * area_h).clamp(0.0, area_h);

    for (i, &sample) in samples.iter().enumerate().skip(1) {
        let sx = area_x + i as f32 * step_x;
        let sy = area_y + area_h - (sample / max_val * area_h).clamp(0.0, area_h);

        tree.push(RenderCommand::Line {
            x1: prev_x,
            y1: prev_y,
            x2: sx,
            y2: sy,
            color,
            width: 1.5,
        });

        prev_x = sx;
        prev_y = sy;
    }
}

/// Render a horizontal dashed line.
fn render_dashed_hline(tree: &mut RenderTree, x: f32, y: f32, total_w: f32, color: Color) {
    let dash_len = 4.0;
    let gap_len = 4.0;
    let mut cx = x;
    while cx < x + total_w {
        let seg_end = (cx + dash_len).min(x + total_w);
        tree.push(RenderCommand::Line {
            x1: cx,
            y1: y,
            x2: seg_end,
            y2: y,
            color,
            width: 1.0,
        });
        cx += dash_len + gap_len;
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Create a demo `ProcessInfo`.
fn make_demo_process(
    pid: u32,
    name: &str,
    status: ProcessStatus,
    cpu: f32,
    mem: u64,
    threads: u32,
    uptime: u64,
) -> ProcessInfo {
    ProcessInfo {
        pid,
        name: name.to_string(),
        status,
        cpu_percent: cpu,
        memory_bytes: mem,
        thread_count: threads,
        uptime_secs: uptime,
    }
}

/// Format a byte count for human-readable display.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format a byte rate for human-readable display (e.g. "1.5 MiB/s").
fn format_rate(bytes_per_sec: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes_per_sec >= GIB {
        format!("{:.1} GiB/s", bytes_per_sec as f64 / GIB as f64)
    } else if bytes_per_sec >= MIB {
        format!("{:.1} MiB/s", bytes_per_sec as f64 / MIB as f64)
    } else if bytes_per_sec >= KIB {
        format!("{:.1} KiB/s", bytes_per_sec as f64 / KIB as f64)
    } else {
        format!("{bytes_per_sec} B/s")
    }
}

/// Format uptime as "Xd Xh Xm Xs".
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else {
        format!("{minutes}m {seconds}s")
    }
}

/// Format uptime in a shorter form for table cells.
fn format_uptime_short(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let mut monitor = SysMonitorState::new();
    monitor.load_demo_data();

    let render_tree = monitor.render();
    println!("System Monitor initialized");
    println!("  {} processes loaded", monitor.processes.len());
    println!("  {} visible (after filter)", monitor.visible_indices.len());
    println!("  {} render commands", render_tree.len());
    println!("  {} active alerts", monitor.active_alerts.len());
    println!("  Status: {}", monitor.status_message);

    // Demonstrate tab switching
    for tab in &Tab::ALL {
        monitor.active_tab = *tab;
        let tab_tree = monitor.render();
        println!("  {} tab: {} render commands", tab.label(), tab_tree.len());
    }

    // Demonstrate sorting
    monitor.active_tab = Tab::Processes;
    monitor.set_sort_column(ProcessColumn::Memory);
    println!(
        "\nSorted by Memory ({}): first visible = {}",
        match monitor.sort_direction {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        },
        monitor
            .visible_indices
            .first()
            .and_then(|&i| monitor.processes.get(i))
            .map_or("(none)", |p| p.name.as_str()),
    );

    // Demonstrate filtering
    monitor.filter_text = "http".to_string();
    monitor.rebuild_visible_list();
    println!("Filter 'http': {} matches", monitor.visible_indices.len());

    // Demonstrate alert checking
    monitor.system_info.cpu_overall = 95.0;
    monitor.check_alerts();
    println!(
        "After CPU spike to 95%: {} alerts",
        monitor.active_alerts.len()
    );
    for alert in &monitor.active_alerts {
        println!("  [{:?}] {}", alert.severity, alert.message);
    }

    println!("\nSystem Monitor ready.");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::event::{Event, KeyEvent, Modifiers, MouseEvent, MouseEventKind};

    // -- GraphHistory tests --

    #[test]
    fn test_graph_history_new_is_empty() {
        let h = GraphHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn test_graph_history_push_and_len() {
        let mut h = GraphHistory::new();
        h.push(1.0);
        h.push(2.0);
        assert_eq!(h.len(), 2);
        assert!(!h.is_empty());
    }

    #[test]
    fn test_graph_history_last() {
        let mut h = GraphHistory::new();
        assert!((h.last() - 0.0).abs() < f32::EPSILON);
        h.push(42.0);
        assert!((h.last() - 42.0).abs() < f32::EPSILON);
        h.push(99.0);
        assert!((h.last() - 99.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_history_wraps_around() {
        let mut h = GraphHistory::new();
        for i in 0..GRAPH_HISTORY_LEN + 10 {
            h.push(i as f32);
        }
        assert_eq!(h.len(), GRAPH_HISTORY_LEN);
        let last = h.last();
        assert!((last - (GRAPH_HISTORY_LEN as f32 + 9.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_history_iter_oldest_first() {
        let mut h = GraphHistory::new();
        h.push(1.0);
        h.push(2.0);
        h.push(3.0);
        let values: Vec<f32> = h.iter_oldest_first().collect();
        assert_eq!(values, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_graph_history_max_value() {
        let mut h = GraphHistory::new();
        h.push(10.0);
        h.push(50.0);
        h.push(30.0);
        assert!((h.max_value() - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_history_max_value_empty() {
        let h = GraphHistory::new();
        assert!((h.max_value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_history_default() {
        let h = GraphHistory::default();
        assert!(h.is_empty());
    }

    // -- Tab tests --

    #[test]
    fn test_tab_labels() {
        assert_eq!(Tab::Overview.label(), "Overview");
        assert_eq!(Tab::Processes.label(), "Processes");
        assert_eq!(Tab::Cpu.label(), "CPU");
        assert_eq!(Tab::Memory.label(), "Memory");
        assert_eq!(Tab::Disk.label(), "Disk");
        assert_eq!(Tab::Network.label(), "Network");
    }

    #[test]
    fn test_tab_all_count() {
        assert_eq!(Tab::ALL.len(), 6);
    }

    // -- ProcessStatus tests --

    #[test]
    fn test_process_status_labels() {
        assert_eq!(ProcessStatus::Running.label(), "Running");
        assert_eq!(ProcessStatus::Sleeping.label(), "Sleeping");
        assert_eq!(ProcessStatus::Stopped.label(), "Stopped");
        assert_eq!(ProcessStatus::Zombie.label(), "Zombie");
        assert_eq!(ProcessStatus::Idle.label(), "Idle");
    }

    #[test]
    fn test_process_status_colors_differ() {
        assert_ne!(
            ProcessStatus::Running.color(),
            ProcessStatus::Zombie.color()
        );
    }

    // -- ProcessColumn tests --

    #[test]
    fn test_process_column_labels() {
        assert_eq!(ProcessColumn::Pid.label(), "PID");
        assert_eq!(ProcessColumn::Cpu.label(), "CPU%");
        assert_eq!(ProcessColumn::Uptime.label(), "Uptime");
    }

    #[test]
    fn test_process_column_widths_positive() {
        for col in &ProcessColumn::ALL {
            assert!(col.width() > 0.0);
        }
    }

    #[test]
    fn test_process_column_all_count() {
        assert_eq!(ProcessColumn::ALL.len(), 7);
    }

    // -- SortDirection tests --

    #[test]
    fn test_sort_direction_equality() {
        assert_eq!(SortDirection::Ascending, SortDirection::Ascending);
        assert_ne!(SortDirection::Ascending, SortDirection::Descending);
    }

    // -- RefreshInterval tests --

    #[test]
    fn test_refresh_interval_ms() {
        assert_eq!(RefreshInterval::HalfSecond.ms(), 500);
        assert_eq!(RefreshInterval::OneSecond.ms(), 1000);
        assert_eq!(RefreshInterval::TwoSeconds.ms(), 2000);
        assert_eq!(RefreshInterval::FiveSeconds.ms(), 5000);
    }

    #[test]
    fn test_refresh_interval_cycles() {
        let start = RefreshInterval::HalfSecond;
        let second = start.next();
        assert_eq!(second, RefreshInterval::OneSecond);
        let third = second.next();
        assert_eq!(third, RefreshInterval::TwoSeconds);
        let fourth = third.next();
        assert_eq!(fourth, RefreshInterval::FiveSeconds);
        let back = fourth.next();
        assert_eq!(back, RefreshInterval::HalfSecond);
    }

    #[test]
    fn test_refresh_interval_labels() {
        assert_eq!(RefreshInterval::HalfSecond.label(), "0.5s");
        assert_eq!(RefreshInterval::TwoSeconds.label(), "2s");
    }

    // -- AlertThresholds tests --

    #[test]
    fn test_alert_thresholds_default() {
        let t = AlertThresholds::default();
        assert!((t.cpu_warn_percent - 70.0).abs() < f32::EPSILON);
        assert!((t.cpu_crit_percent - 90.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_alert_thresholds_cpu_color_green() {
        let t = AlertThresholds::default();
        assert_eq!(t.cpu_color(30.0), GREEN);
    }

    #[test]
    fn test_alert_thresholds_cpu_color_yellow() {
        let t = AlertThresholds::default();
        assert_eq!(t.cpu_color(75.0), YELLOW);
    }

    #[test]
    fn test_alert_thresholds_cpu_color_red() {
        let t = AlertThresholds::default();
        assert_eq!(t.cpu_color(95.0), RED);
    }

    #[test]
    fn test_alert_thresholds_mem_color() {
        let t = AlertThresholds::default();
        assert_eq!(t.mem_color(50.0), GREEN);
        assert_eq!(t.mem_color(80.0), YELLOW);
        assert_eq!(t.mem_color(95.0), RED);
    }

    #[test]
    fn test_alert_thresholds_disk_color() {
        let t = AlertThresholds::default();
        assert_eq!(t.disk_color(50.0), GREEN);
        assert_eq!(t.disk_color(85.0), YELLOW);
        assert_eq!(t.disk_color(96.0), RED);
    }

    // -- DiskInfo tests --

    #[test]
    fn test_disk_usage_fraction() {
        let disk = DiskInfo {
            name: "sda".to_string(),
            mount_point: "/".to_string(),
            total_bytes: 1000,
            used_bytes: 700,
            read_bytes_sec: 0,
            write_bytes_sec: 0,
            read_iops: 0,
            write_iops: 0,
            read_history: GraphHistory::new(),
            write_history: GraphHistory::new(),
        };
        assert!((disk.usage_fraction() - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_disk_usage_fraction_zero_total() {
        let disk = DiskInfo {
            name: "sda".to_string(),
            mount_point: "/".to_string(),
            total_bytes: 0,
            used_bytes: 0,
            read_bytes_sec: 0,
            write_bytes_sec: 0,
            read_iops: 0,
            write_iops: 0,
            read_history: GraphHistory::new(),
            write_history: GraphHistory::new(),
        };
        assert!((disk.usage_fraction() - 0.0).abs() < f32::EPSILON);
    }

    // -- ContextAction tests --

    #[test]
    fn test_context_action_labels() {
        assert_eq!(ContextAction::Kill.label(), "Kill Process");
        assert_eq!(ContextAction::Stop.label(), "Stop Process");
        assert_eq!(ContextAction::Continue.label(), "Continue Process");
    }

    #[test]
    fn test_context_action_all_count() {
        assert_eq!(ContextAction::ALL.len(), 6);
    }

    // -- AlertSeverity tests --

    #[test]
    fn test_alert_severity_colors() {
        assert_eq!(AlertSeverity::Warning.color(), YELLOW);
        assert_eq!(AlertSeverity::Critical.color(), RED);
    }

    // -- SysMonitorState tests --

    #[test]
    fn test_state_new_defaults() {
        let s = SysMonitorState::new();
        assert_eq!(s.active_tab, Tab::Overview);
        assert!(s.processes.is_empty());
        assert!(s.visible_indices.is_empty());
        assert_eq!(s.sort_column, ProcessColumn::Cpu);
        assert_eq!(s.sort_direction, SortDirection::Descending);
        assert!(s.filter_text.is_empty());
        assert!(!s.filter_focused);
        assert!(s.context_menu.is_none());
    }

    #[test]
    fn test_state_default_matches_new() {
        let a = SysMonitorState::new();
        let b = SysMonitorState::default();
        assert_eq!(a.active_tab, b.active_tab);
        assert_eq!(a.processes.len(), b.processes.len());
    }

    #[test]
    fn test_load_demo_data_populates() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        assert!(!s.processes.is_empty());
        assert!(!s.visible_indices.is_empty());
        assert!(!s.cores.is_empty());
        assert!(!s.disks.is_empty());
        assert!(!s.interfaces.is_empty());
        assert!(!s.status_message.is_empty());
    }

    #[test]
    fn test_rebuild_visible_list_all_visible() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        let total = s.processes.len();
        assert_eq!(s.visible_indices.len(), total);
    }

    #[test]
    fn test_filter_narrows_list() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.filter_text = "http".to_string();
        s.rebuild_visible_list();
        assert!(s.visible_indices.len() < s.processes.len());
        assert!(!s.visible_indices.is_empty());
    }

    #[test]
    fn test_filter_no_match() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.filter_text = "zzz_nonexistent_zzz".to_string();
        s.rebuild_visible_list();
        assert!(s.visible_indices.is_empty());
    }

    #[test]
    fn test_sort_ascending() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.sort_column = ProcessColumn::Pid;
        s.sort_direction = SortDirection::Ascending;
        s.rebuild_visible_list();
        let pids: Vec<u32> = s
            .visible_indices
            .iter()
            .filter_map(|&i| s.processes.get(i).map(|p| p.pid))
            .collect();
        for window in pids.windows(2) {
            assert!(window.first().copied().unwrap_or(0) <= window.get(1).copied().unwrap_or(0));
        }
    }

    #[test]
    fn test_sort_descending_cpu() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.sort_column = ProcessColumn::Cpu;
        s.sort_direction = SortDirection::Descending;
        s.rebuild_visible_list();
        let cpus: Vec<f32> = s
            .visible_indices
            .iter()
            .filter_map(|&i| s.processes.get(i).map(|p| p.cpu_percent))
            .collect();
        for window in cpus.windows(2) {
            assert!(window.first().copied().unwrap_or(0.0) >= window.get(1).copied().unwrap_or(0.0));
        }
    }

    #[test]
    fn test_set_sort_column_toggles_direction() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.set_sort_column(ProcessColumn::Pid);
        assert_eq!(s.sort_column, ProcessColumn::Pid);
        assert_eq!(s.sort_direction, SortDirection::Ascending);
        s.set_sort_column(ProcessColumn::Pid);
        assert_eq!(s.sort_direction, SortDirection::Descending);
    }

    #[test]
    fn test_kill_selected() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        let initial = s.processes.len();
        s.selected_index = Some(0);
        s.kill_selected();
        assert_eq!(s.processes.len(), initial - 1);
    }

    #[test]
    fn test_stop_selected() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = Some(0);
        s.stop_selected();
        let proc_idx = s.visible_indices.first().copied().unwrap_or(0);
        assert_eq!(
            s.processes.get(proc_idx).map(|p| p.status),
            Some(ProcessStatus::Stopped)
        );
    }

    #[test]
    fn test_continue_selected() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = Some(0);
        s.stop_selected();
        s.continue_selected();
        let proc_idx = s.visible_indices.first().copied().unwrap_or(0);
        assert_eq!(
            s.processes.get(proc_idx).map(|p| p.status),
            Some(ProcessStatus::Running)
        );
    }

    #[test]
    fn test_cycle_tab_forward() {
        let mut s = SysMonitorState::new();
        s.active_tab = Tab::Overview;
        s.cycle_tab_forward();
        assert_eq!(s.active_tab, Tab::Processes);
        s.cycle_tab_forward();
        assert_eq!(s.active_tab, Tab::Cpu);
    }

    #[test]
    fn test_cycle_tab_backward() {
        let mut s = SysMonitorState::new();
        s.active_tab = Tab::Overview;
        s.cycle_tab_backward();
        assert_eq!(s.active_tab, Tab::Network);
    }

    #[test]
    fn test_cycle_refresh_interval() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        let initial = s.refresh_interval;
        s.cycle_refresh_interval();
        assert_ne!(s.refresh_interval, initial);
    }

    #[test]
    fn test_move_selection_down() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = Some(0);
        s.move_selection(1);
        assert_eq!(s.selected_index, Some(1));
    }

    #[test]
    fn test_move_selection_clamps() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = Some(0);
        s.move_selection(-10);
        assert_eq!(s.selected_index, Some(0));
    }

    #[test]
    fn test_check_alerts_cpu_critical() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.system_info.cpu_overall = 95.0;
        s.check_alerts();
        assert!(
            s.active_alerts
                .iter()
                .any(|a| a.severity == AlertSeverity::Critical)
        );
    }

    #[test]
    fn test_check_alerts_cpu_warning() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.system_info.cpu_overall = 75.0;
        s.check_alerts();
        assert!(
            s.active_alerts
                .iter()
                .any(|a| a.severity == AlertSeverity::Warning)
        );
    }

    #[test]
    fn test_check_alerts_none_when_healthy() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.system_info.cpu_overall = 20.0;
        s.system_info.used_memory = 1_000_000;
        s.system_info.total_memory = 8_000_000_000;
        // Reset disks to low usage
        for disk in &mut s.disks {
            disk.used_bytes = disk.total_bytes / 10;
        }
        s.check_alerts();
        assert!(s.active_alerts.is_empty());
    }

    #[test]
    fn test_render_overview_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Overview;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_processes_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Processes;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_cpu_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Cpu;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_memory_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Memory;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_disk_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Disk;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_network_produces_commands() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Network;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_empty_disks_tab() {
        let mut s = SysMonitorState::new();
        s.active_tab = Tab::Disk;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_empty_network_tab() {
        let mut s = SysMonitorState::new();
        s.active_tab = Tab::Network;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_handle_resize() {
        let mut s = SysMonitorState::new();
        let result = s.handle_event(&Event::Resize {
            width: 1920,
            height: 1080,
        });
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(s.window_width, 1920);
        assert_eq!(s.window_height, 1080);
    }

    #[test]
    fn test_handle_tick_triggers_refresh() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.refresh_interval = RefreshInterval::OneSecond;
        s.ms_since_refresh = 999;
        let result = s.handle_event(&Event::Tick { elapsed_ms: 10 });
        assert_eq!(result, EventResult::Consumed);
        // Should have triggered refresh, resetting ms_since_refresh
        assert_eq!(s.ms_since_refresh, 0);
    }

    #[test]
    fn test_handle_key_tab_switch() {
        let mut s = SysMonitorState::new();
        let key = KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        s.handle_key(&key);
        assert_eq!(s.active_tab, Tab::Cpu);
    }

    #[test]
    fn test_handle_key_filter_focus() {
        let mut s = SysMonitorState::new();
        let key = KeyEvent {
            key: Key::F,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        s.handle_key(&key);
        assert!(s.filter_focused);
    }

    #[test]
    fn test_filter_key_typing() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.filter_focused = true;
        let key = KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        };
        s.handle_filter_key(&key);
        assert_eq!(s.filter_text, "a");
    }

    #[test]
    fn test_filter_key_backspace() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.filter_text = "abc".to_string();
        s.filter_focused = true;
        let key = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        s.handle_filter_key(&key);
        assert_eq!(s.filter_text, "ab");
    }

    #[test]
    fn test_filter_key_escape_unfocuses() {
        let mut s = SysMonitorState::new();
        s.filter_focused = true;
        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        s.handle_filter_key(&key);
        assert!(!s.filter_focused);
    }

    #[test]
    fn test_context_menu_click() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Processes;
        // Simulate right-click on first row
        let mouse = MouseEvent {
            x: 50.0,
            y: TAB_BAR_HEIGHT + HEADER_HEIGHT + 5.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        };
        s.handle_mouse(&mouse);
        assert!(s.context_menu.is_some());
    }

    #[test]
    fn test_execute_context_action_kill() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        let initial = s.processes.len();
        let pid = s.processes.first().map(|p| p.pid).unwrap_or(0);
        s.execute_context_action(ContextAction::Kill, pid);
        assert_eq!(s.processes.len(), initial - 1);
    }

    #[test]
    fn test_selected_process_returns_some() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = Some(0);
        assert!(s.selected_process().is_some());
    }

    #[test]
    fn test_selected_process_returns_none_no_selection() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.selected_index = None;
        assert!(s.selected_process().is_none());
    }

    #[test]
    fn test_visible_row_count() {
        let s = SysMonitorState::new();
        let count = s.visible_row_count();
        assert!(count > 0);
    }

    // -- Format helper tests --

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
        assert_eq!(format_bytes(1_099_511_627_776), "1.0 TiB");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(format_rate(500), "500 B/s");
        assert_eq!(format_rate(1024), "1.0 KiB/s");
        assert_eq!(format_rate(1_048_576), "1.0 MiB/s");
    }

    #[test]
    fn test_format_uptime() {
        assert_eq!(format_uptime(90061), "1d 1h 1m");
        assert_eq!(format_uptime(3661), "1h 1m 1s");
        assert_eq!(format_uptime(61), "1m 1s");
    }

    #[test]
    fn test_format_uptime_short() {
        assert_eq!(format_uptime_short(3661), "1:01:01");
        assert_eq!(format_uptime_short(61), "1:01");
    }

    #[test]
    fn test_make_demo_process() {
        let p = make_demo_process(42, "test", ProcessStatus::Running, 5.0, 1000, 2, 300);
        assert_eq!(p.pid, 42);
        assert_eq!(p.name, "test");
        assert_eq!(p.status, ProcessStatus::Running);
    }

    #[test]
    fn test_render_with_context_menu() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.active_tab = Tab::Processes;
        s.context_menu = Some(ContextMenu {
            x: 100.0,
            y: 200.0,
            target_pid: 1,
            hover_index: Some(0),
        });
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_with_alerts() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.system_info.cpu_overall = 95.0;
        s.check_alerts();
        s.active_tab = Tab::Overview;
        let tree = s.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_scroll_offset_clamped() {
        let mut s = SysMonitorState::new();
        s.load_demo_data();
        s.scroll_offset = 1000;
        let mouse = MouseEvent {
            x: 50.0,
            y: 50.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        };
        s.handle_mouse(&mouse);
        assert!(s.scroll_offset <= s.visible_indices.len());
    }
}
