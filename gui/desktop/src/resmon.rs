//! Resource Monitor Widget — desktop shell overlay/panel.
//!
//! Provides real-time system resource monitoring graphs as a compact overlay
//! or expanded panel on the desktop. Designed for at-a-glance visibility of
//! CPU, memory, disk, and network activity without opening a full process
//! explorer.
//!
//! # Modes
//!
//! - **Compact**: a thin horizontal strip with four mini sparklines
//!   (CPU, RAM, disk, network) suitable for embedding in the taskbar tray
//!   area or a floating widget.
//! - **Expanded**: four stacked graph panels showing detailed time-series
//!   data with labeled axes, current values, and peak markers.
//!
//! # Data flow
//!
//! An external polling loop gathers system metrics and produces a
//! [`ResourceSnapshot`] at regular intervals (typically 1 Hz). The snapshot
//! is fed into [`ResourceMonitor::update`], which pushes samples into
//! circular buffers. Each call to [`ResourceMonitor::render`] reads the
//! buffers and produces a `Vec<RenderCommand>` the compositor can draw.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut monitor = ResourceMonitor::new(320.0, 400.0);
//!
//! // Toggle between compact and expanded:
//! monitor.toggle_mode();
//!
//! // Each polling tick:
//! monitor.update(&snapshot);
//!
//! // Each frame:
//! let commands = monitor.render();
//! ```

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT: Color = Color::from_hex(0xA6ADC8);

    pub const CPU: Color = Color::from_hex(0x89B4FA);
    pub const MEMORY: Color = Color::from_hex(0xA6E3A1);
    pub const DISK: Color = Color::from_hex(0xFAB387);
    pub const NETWORK: Color = Color::from_hex(0xCBA6F7);
    pub const TEMPERATURE: Color = Color::from_hex(0xF38BA8);
    pub const GPU: Color = Color::from_hex(0xB4BEFE);
}

// ============================================================================
// Constants
// ============================================================================

/// Number of samples retained in each circular buffer.
const GRAPH_BUFFER_SIZE: usize = 64;

/// Padding inside graph panels (expanded mode).
const PANEL_PADDING: f32 = 8.0;

/// Height of the label row above each expanded graph.
const LABEL_HEIGHT: f32 = 20.0;

/// Number of horizontal grid lines drawn in a graph area.
const GRID_LINE_COUNT: usize = 4;

/// Minimum sparkline width (compact mode) to avoid degenerate rendering.
const MIN_SPARKLINE_WIDTH: f32 = 20.0;

// ============================================================================
// Resource type enum
// ============================================================================

/// Categories of system resources that can be monitored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Temperature,
}

impl ResourceType {
    /// Display label for this resource type.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "RAM",
            Self::Disk => "Disk",
            Self::Network => "Net",
            Self::Gpu => "GPU",
            Self::Temperature => "Temp",
        }
    }

    /// Theme color for this resource type.
    pub fn color(self) -> Color {
        match self {
            Self::Cpu => theme::CPU,
            Self::Memory => theme::MEMORY,
            Self::Disk => theme::DISK,
            Self::Network => theme::NETWORK,
            Self::Gpu => theme::GPU,
            Self::Temperature => theme::TEMPERATURE,
        }
    }
}

// ============================================================================
// Circular graph data buffer
// ============================================================================

/// Fixed-size circular buffer of f32 samples for graph rendering.
///
/// Stores the most recent `GRAPH_BUFFER_SIZE` values. Older values are
/// silently overwritten when the buffer wraps.
#[derive(Clone, Debug)]
pub struct GraphData {
    /// Storage for samples. Always has length `GRAPH_BUFFER_SIZE`.
    samples: [f32; GRAPH_BUFFER_SIZE],
    /// Write cursor — next index to overwrite.
    write_pos: usize,
    /// Number of samples pushed so far (saturates at `GRAPH_BUFFER_SIZE`).
    count: usize,
}

impl GraphData {
    /// Create an empty graph buffer (all zeros).
    pub fn new() -> Self {
        Self {
            samples: [0.0; GRAPH_BUFFER_SIZE],
            write_pos: 0,
            count: 0,
        }
    }

    /// Push a new sample into the buffer, overwriting the oldest if full.
    pub fn push(&mut self, value: f32) {
        self.samples[self.write_pos] = value;
        self.write_pos = (self.write_pos + 1) % GRAPH_BUFFER_SIZE;
        if self.count < GRAPH_BUFFER_SIZE {
            self.count += 1;
        }
    }

    /// Number of valid samples currently in the buffer.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the buffer contains no samples.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Most recently pushed sample, or `0.0` if empty.
    pub fn latest(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        // write_pos points to the *next* slot, so the latest is one behind.
        let idx = if self.write_pos == 0 {
            GRAPH_BUFFER_SIZE - 1
        } else {
            self.write_pos - 1
        };
        self.samples[idx]
    }

    /// Arithmetic mean of all valid samples, or `0.0` if empty.
    pub fn average(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f32 = self.valid_samples().iter().copied().sum();
        sum / self.count as f32
    }

    /// Maximum value among all valid samples, or `0.0` if empty.
    pub fn peak(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        self.valid_samples()
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
    }

    /// Return valid samples in chronological order (oldest first).
    ///
    /// The returned `Vec` has exactly `self.count` elements.
    pub fn valid_samples(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.count);
        if self.count < GRAPH_BUFFER_SIZE {
            // Buffer hasn't wrapped yet — samples start at index 0.
            out.extend_from_slice(&self.samples[..self.count]);
        } else {
            // Wrapped — oldest sample is at write_pos.
            out.extend_from_slice(&self.samples[self.write_pos..]);
            out.extend_from_slice(&self.samples[..self.write_pos]);
        }
        out
    }
}

impl Default for GraphData {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Info structs — per-resource-type detailed metrics
// ============================================================================

/// Detailed CPU metrics for a single snapshot.
#[derive(Clone, Debug, Default)]
pub struct CpuInfo {
    /// Per-core usage as a percentage (0.0 .. 100.0).
    pub per_core_usage: Vec<f32>,
    /// Overall CPU usage as a percentage (0.0 .. 100.0).
    pub total_usage: f32,
    /// Current CPU frequency in MHz.
    pub frequency_mhz: u32,
    /// Number of running processes.
    pub process_count: u32,
    /// Number of running threads.
    pub thread_count: u32,
}

/// Detailed memory metrics for a single snapshot.
#[derive(Clone, Debug, Default)]
pub struct MemoryInfo {
    /// Total physical memory in MiB.
    pub total_mb: u64,
    /// Used physical memory in MiB.
    pub used_mb: u64,
    /// Cached/buffered memory in MiB.
    pub cached_mb: u64,
    /// Total swap space in MiB.
    pub swap_total_mb: u64,
    /// Used swap space in MiB.
    pub swap_used_mb: u64,
}

impl MemoryInfo {
    /// Memory usage as a percentage, clamped to 0..100.
    pub fn usage_pct(&self) -> f32 {
        if self.total_mb == 0 {
            return 0.0;
        }
        ((self.used_mb as f64 / self.total_mb as f64) * 100.0) as f32
    }
}

/// Detailed disk I/O metrics for a single snapshot.
#[derive(Clone, Debug, Default)]
pub struct DiskInfo {
    /// Bytes read per second.
    pub read_bytes_per_sec: u64,
    /// Bytes written per second.
    pub write_bytes_per_sec: u64,
    /// Disk busy percentage (0.0 .. 100.0).
    pub busy_pct: f32,
    /// Number of I/O operations in the sampling interval.
    pub io_count: u32,
}

/// Detailed network metrics for a single snapshot.
#[derive(Clone, Debug, Default)]
pub struct NetworkInfo {
    /// Bytes received per second.
    pub rx_bytes_per_sec: u64,
    /// Bytes transmitted per second.
    pub tx_bytes_per_sec: u64,
    /// Number of active connections.
    pub connections_count: u32,
    /// Packets received in the sampling interval.
    pub packets_in: u64,
    /// Packets transmitted in the sampling interval.
    pub packets_out: u64,
}

impl NetworkInfo {
    /// Total throughput in bytes per second (rx + tx).
    pub fn total_bytes_per_sec(&self) -> u64 {
        self.rx_bytes_per_sec.saturating_add(self.tx_bytes_per_sec)
    }
}

// ============================================================================
// Resource snapshot
// ============================================================================

/// A point-in-time snapshot of all monitored system resources.
///
/// Produced by the polling loop and consumed by [`ResourceMonitor::update`].
#[derive(Clone, Debug, Default)]
pub struct ResourceSnapshot {
    /// Monotonic timestamp in milliseconds (e.g., since boot).
    pub timestamp_ms: u64,
    /// CPU metrics.
    pub cpu: CpuInfo,
    /// Memory metrics.
    pub memory: MemoryInfo,
    /// Disk metrics.
    pub disk: DiskInfo,
    /// Network metrics.
    pub network: NetworkInfo,
    /// GPU usage percentage (0.0 .. 100.0).
    pub gpu_usage_pct: f32,
    /// Temperature in degrees Celsius.
    pub temperature_celsius: f32,
}

// ============================================================================
// Display mode
// ============================================================================

/// Display mode for the resource monitor widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// Single horizontal strip with four mini sparklines.
    Compact,
    /// Four stacked graph panels with full detail.
    Expanded,
}

// ============================================================================
// Resource monitor
// ============================================================================

/// Desktop resource monitor widget.
///
/// Maintains time-series data for each resource type and renders either
/// a compact sparkline strip or an expanded multi-panel graph view.
pub struct ResourceMonitor {
    /// Widget width in logical pixels.
    width: f32,
    /// Widget height in logical pixels.
    height: f32,
    /// Current display mode.
    mode: DisplayMode,
    /// Time-series data for CPU usage.
    cpu_data: GraphData,
    /// Time-series data for memory usage.
    mem_data: GraphData,
    /// Time-series data for disk busy percentage.
    disk_data: GraphData,
    /// Time-series data for network throughput (normalized 0..100).
    net_data: GraphData,
    /// Time-series data for GPU usage.
    gpu_data: GraphData,
    /// Time-series data for temperature.
    temp_data: GraphData,
    /// Most recent snapshot (for label display).
    last_snapshot: Option<ResourceSnapshot>,
    /// Recorded peak network throughput (bytes/sec) for normalization.
    net_peak_bps: u64,
}

impl ResourceMonitor {
    /// Create a new resource monitor widget with the given dimensions.
    ///
    /// Starts in compact mode. Call [`toggle_mode`] to switch.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            mode: DisplayMode::Compact,
            cpu_data: GraphData::new(),
            mem_data: GraphData::new(),
            disk_data: GraphData::new(),
            net_data: GraphData::new(),
            gpu_data: GraphData::new(),
            temp_data: GraphData::new(),
            last_snapshot: None,
            net_peak_bps: 1,
        }
    }

    /// Current display mode.
    pub fn mode(&self) -> DisplayMode {
        self.mode
    }

    /// Switch between compact and expanded mode.
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            DisplayMode::Compact => DisplayMode::Expanded,
            DisplayMode::Expanded => DisplayMode::Compact,
        };
    }

    /// Explicitly set the display mode.
    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
    }

    /// Resize the widget.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Widget width.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Widget height.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Push a new snapshot into all graph buffers.
    pub fn update(&mut self, snapshot: &ResourceSnapshot) {
        self.cpu_data.push(snapshot.cpu.total_usage);
        self.mem_data.push(snapshot.memory.usage_pct());
        self.disk_data.push(snapshot.disk.busy_pct);

        // Normalize network throughput to 0..100 using a running peak.
        let total_bps = snapshot.network.total_bytes_per_sec();
        if total_bps > self.net_peak_bps {
            self.net_peak_bps = total_bps;
        }
        let net_pct = if self.net_peak_bps > 0 {
            (total_bps as f64 / self.net_peak_bps as f64 * 100.0) as f32
        } else {
            0.0
        };
        self.net_data.push(net_pct);

        self.gpu_data.push(snapshot.gpu_usage_pct);
        self.temp_data.push(snapshot.temperature_celsius);

        self.last_snapshot = Some(snapshot.clone());
    }

    /// Access the graph data for a given resource type.
    pub fn graph_data(&self, resource: ResourceType) -> &GraphData {
        match resource {
            ResourceType::Cpu => &self.cpu_data,
            ResourceType::Memory => &self.mem_data,
            ResourceType::Disk => &self.disk_data,
            ResourceType::Network => &self.net_data,
            ResourceType::Gpu => &self.gpu_data,
            ResourceType::Temperature => &self.temp_data,
        }
    }

    /// Render the widget into a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        match self.mode {
            DisplayMode::Compact => self.render_compact(),
            DisplayMode::Expanded => self.render_expanded(),
        }
    }

    // ======================================================================
    // Compact mode rendering
    // ======================================================================

    /// Render compact mode: a single strip with four mini sparklines.
    fn render_compact(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: theme::BASE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: theme::SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Divide width into 4 equal sparkline slots with padding.
        let padding = 4.0;
        let slot_count = 4.0;
        let total_padding = padding * (slot_count + 1.0);
        let slot_w = (self.width - total_padding) / slot_count;
        let slot_h = self.height - padding * 2.0;

        if slot_w < MIN_SPARKLINE_WIDTH || slot_h < 4.0 {
            return cmds;
        }

        let resources = [
            ResourceType::Cpu,
            ResourceType::Memory,
            ResourceType::Disk,
            ResourceType::Network,
        ];

        for (i, &res) in resources.iter().enumerate() {
            let sx = padding + i as f32 * (slot_w + padding);
            let sy = padding;
            let data = self.graph_data(res);

            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, res.color());
        }

        cmds
    }

    // ======================================================================
    // Expanded mode rendering
    // ======================================================================

    /// Render expanded mode: four stacked graph panels.
    fn render_expanded(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Outer background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: theme::BASE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Outer border.
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: theme::SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PANEL_PADDING,
            y: PANEL_PADDING,
            text: "Resource Monitor".to_string(),
            color: theme::TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PANEL_PADDING * 2.0),
        });

        let panels = [
            ResourceType::Cpu,
            ResourceType::Memory,
            ResourceType::Disk,
            ResourceType::Network,
        ];
        let panel_count = panels.len() as f32;
        let title_area = PANEL_PADDING + 18.0;
        let gap = 6.0;
        let total_gap = gap * (panel_count - 1.0);
        let available_h = self.height - title_area - PANEL_PADDING - total_gap;
        let panel_h = available_h / panel_count;
        let panel_w = self.width - PANEL_PADDING * 2.0;

        for (i, &res) in panels.iter().enumerate() {
            let px = PANEL_PADDING;
            let py = title_area + i as f32 * (panel_h + gap);
            self.render_panel(&mut cmds, res, px, py, panel_w, panel_h);
        }

        cmds
    }

    /// Render a single graph panel (expanded mode).
    fn render_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        resource: ResourceType,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let color = resource.color();
        let data = self.graph_data(resource);

        // Panel background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: theme::SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Label row: resource name on the left, current value on the right.
        let label_y = y + 2.0;
        cmds.push(RenderCommand::Text {
            x: x + 6.0,
            y: label_y,
            text: resource.label().to_string(),
            color,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w * 0.4),
        });

        // Current value text.
        let value_text = self.format_value(resource, data.latest());
        cmds.push(RenderCommand::Text {
            x: x + w - 80.0,
            y: label_y,
            text: value_text,
            color: theme::TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(74.0),
        });

        // Peak value (dimmer).
        let peak_text = format!("peak {}", self.format_value(resource, data.peak()));
        cmds.push(RenderCommand::Text {
            x: x + 6.0,
            y: label_y + 12.0,
            text: peak_text,
            color: theme::SUBTEXT,
            font_size: 9.0,
            font_weight: FontWeightHint::Light,
            max_width: Some(w * 0.5),
        });

        // Graph area (below labels).
        let graph_x = x + 4.0;
        let graph_y = y + LABEL_HEIGHT + 6.0;
        let graph_w = w - 8.0;
        let graph_h = h - LABEL_HEIGHT - 10.0;

        if graph_w > 0.0 && graph_h > 0.0 {
            Self::render_grid_lines(cmds, graph_x, graph_y, graph_w, graph_h);
            Self::render_sparkline(cmds, data, graph_x, graph_y, graph_w, graph_h, color);
        }
    }

    // ======================================================================
    // Graph rendering primitives
    // ======================================================================

    /// Render a sparkline (line graph) for the given data.
    ///
    /// `x`, `y` define the top-left of the graph area; `w`, `h` its size.
    /// Values are normalized against 0..100 for percentage-based data.
    pub fn render_sparkline(
        cmds: &mut Vec<RenderCommand>,
        data: &GraphData,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    ) {
        let samples = data.valid_samples();
        let count = samples.len();
        if count < 2 || w < MIN_SPARKLINE_WIDTH || h < 2.0 {
            return;
        }

        let max_val = 100.0_f32; // percentage scale
        let step = w / (count as f32 - 1.0);

        for i in 1..count {
            let prev = samples.get(i - 1).copied().unwrap_or(0.0);
            let curr = samples.get(i).copied().unwrap_or(0.0);

            let x1 = x + (i as f32 - 1.0) * step;
            let y1 = y + h - (prev.clamp(0.0, max_val) / max_val * h);
            let x2 = x + i as f32 * step;
            let y2 = y + h - (curr.clamp(0.0, max_val) / max_val * h);

            cmds.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width: 1.5,
            });
        }
    }

    /// Render a bar graph for the given data.
    ///
    /// Each sample becomes one vertical bar. Bars are evenly spaced across `w`.
    pub fn render_bar_graph(
        cmds: &mut Vec<RenderCommand>,
        data: &GraphData,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    ) {
        let samples = data.valid_samples();
        let count = samples.len();
        if count == 0 || w < 1.0 || h < 1.0 {
            return;
        }

        let max_val = 100.0_f32;
        let bar_gap = 1.0_f32;
        let bar_w = ((w - bar_gap * count as f32) / count as f32).max(1.0);

        for (i, &val) in samples.iter().enumerate() {
            let bar_h = (val.clamp(0.0, max_val) / max_val * h).max(0.0);
            let bx = x + i as f32 * (bar_w + bar_gap);
            let by = y + h - bar_h;

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: bar_w,
                height: bar_h,
                color,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    /// Render subtle horizontal grid lines across a graph area.
    fn render_grid_lines(
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let grid_color = theme::SURFACE1;
        for i in 1..=GRID_LINE_COUNT {
            let gy = y + (i as f32 / (GRID_LINE_COUNT as f32 + 1.0)) * h;
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: gy,
                x2: x + w,
                y2: gy,
                color: grid_color,
                width: 0.5,
            });
        }
    }

    // ======================================================================
    // Value formatting
    // ======================================================================

    /// Format a sample value as a human-readable string.
    fn format_value(&self, resource: ResourceType, value: f32) -> String {
        match resource {
            ResourceType::Cpu | ResourceType::Memory | ResourceType::Disk | ResourceType::Gpu => {
                format!("{:.1}%", value.clamp(0.0, 100.0))
            }
            ResourceType::Network => {
                // Convert normalized 0..100 back to bytes/sec using peak.
                let bps = (value / 100.0 * self.net_peak_bps as f32) as u64;
                format_bytes_per_sec(bps)
            }
            ResourceType::Temperature => {
                format!("{:.0}\u{00B0}C", value)
            }
        }
    }
}

/// Format a bytes-per-second value into a compact human-readable string.
fn format_bytes_per_sec(bps: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bps >= GB {
        format!("{:.1} GB/s", bps as f64 / GB as f64)
    } else if bps >= MB {
        format!("{:.1} MB/s", bps as f64 / MB as f64)
    } else if bps >= KB {
        format!("{:.1} KB/s", bps as f64 / KB as f64)
    } else {
        format!("{bps} B/s")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ======================================================================
    // GraphData tests
    // ======================================================================

    #[test]
    fn test_graph_data_new_is_empty() {
        let data = GraphData::new();
        assert!(data.is_empty());
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_graph_data_push_single() {
        let mut data = GraphData::new();
        data.push(42.0);
        assert_eq!(data.len(), 1);
        assert!(!data.is_empty());
        assert_eq!(data.latest(), 42.0);
    }

    #[test]
    fn test_graph_data_push_multiple() {
        let mut data = GraphData::new();
        data.push(10.0);
        data.push(20.0);
        data.push(30.0);
        assert_eq!(data.len(), 3);
        assert_eq!(data.latest(), 30.0);
    }

    #[test]
    fn test_graph_data_wraps_at_capacity() {
        let mut data = GraphData::new();
        for i in 0..GRAPH_BUFFER_SIZE + 10 {
            data.push(i as f32);
        }
        assert_eq!(data.len(), GRAPH_BUFFER_SIZE);
        assert_eq!(data.latest(), (GRAPH_BUFFER_SIZE + 9) as f32);
    }

    #[test]
    fn test_graph_data_latest_empty() {
        let data = GraphData::new();
        assert_eq!(data.latest(), 0.0);
    }

    #[test]
    fn test_graph_data_average_empty() {
        let data = GraphData::new();
        assert_eq!(data.average(), 0.0);
    }

    #[test]
    fn test_graph_data_average_single() {
        let mut data = GraphData::new();
        data.push(50.0);
        assert_eq!(data.average(), 50.0);
    }

    #[test]
    fn test_graph_data_average_multiple() {
        let mut data = GraphData::new();
        data.push(10.0);
        data.push(20.0);
        data.push(30.0);
        let avg = data.average();
        assert!((avg - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_data_peak_empty() {
        let data = GraphData::new();
        assert_eq!(data.peak(), 0.0);
    }

    #[test]
    fn test_graph_data_peak() {
        let mut data = GraphData::new();
        data.push(10.0);
        data.push(99.0);
        data.push(30.0);
        assert_eq!(data.peak(), 99.0);
    }

    #[test]
    fn test_graph_data_peak_after_wrap() {
        let mut data = GraphData::new();
        // Fill buffer with 50, then overwrite with lower values.
        for _ in 0..GRAPH_BUFFER_SIZE {
            data.push(50.0);
        }
        // Overwrite the first 10 with 10.0.
        for _ in 0..10 {
            data.push(10.0);
        }
        // The remaining 54 values of 50.0 should still yield peak 50.0.
        assert_eq!(data.peak(), 50.0);
    }

    #[test]
    fn test_graph_data_valid_samples_chronological() {
        let mut data = GraphData::new();
        data.push(1.0);
        data.push(2.0);
        data.push(3.0);
        let samples = data.valid_samples();
        assert_eq!(samples, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_graph_data_valid_samples_after_wrap() {
        let mut data = GraphData::new();
        for i in 0..(GRAPH_BUFFER_SIZE + 5) {
            data.push(i as f32);
        }
        let samples = data.valid_samples();
        assert_eq!(samples.len(), GRAPH_BUFFER_SIZE);
        // Oldest should be 5, newest should be GRAPH_BUFFER_SIZE + 4.
        assert_eq!(samples[0], 5.0);
        assert_eq!(samples[GRAPH_BUFFER_SIZE - 1], (GRAPH_BUFFER_SIZE + 4) as f32);
    }

    #[test]
    fn test_graph_data_default() {
        let data = GraphData::default();
        assert!(data.is_empty());
    }

    // ======================================================================
    // ResourceType tests
    // ======================================================================

    #[test]
    fn test_resource_type_labels() {
        assert_eq!(ResourceType::Cpu.label(), "CPU");
        assert_eq!(ResourceType::Memory.label(), "RAM");
        assert_eq!(ResourceType::Disk.label(), "Disk");
        assert_eq!(ResourceType::Network.label(), "Net");
        assert_eq!(ResourceType::Gpu.label(), "GPU");
        assert_eq!(ResourceType::Temperature.label(), "Temp");
    }

    #[test]
    fn test_resource_type_colors_distinct() {
        let types = [
            ResourceType::Cpu,
            ResourceType::Memory,
            ResourceType::Disk,
            ResourceType::Network,
            ResourceType::Gpu,
            ResourceType::Temperature,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i].color(),
                    types[j].color(),
                    "{:?} and {:?} should have different colors",
                    types[i],
                    types[j],
                );
            }
        }
    }

    // ======================================================================
    // MemoryInfo tests
    // ======================================================================

    #[test]
    fn test_memory_info_usage_pct() {
        let info = MemoryInfo {
            total_mb: 16384,
            used_mb: 8192,
            ..Default::default()
        };
        assert!((info.usage_pct() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_memory_info_usage_pct_zero_total() {
        let info = MemoryInfo::default();
        assert_eq!(info.usage_pct(), 0.0);
    }

    // ======================================================================
    // NetworkInfo tests
    // ======================================================================

    #[test]
    fn test_network_info_total_bytes() {
        let info = NetworkInfo {
            rx_bytes_per_sec: 1000,
            tx_bytes_per_sec: 500,
            ..Default::default()
        };
        assert_eq!(info.total_bytes_per_sec(), 1500);
    }

    #[test]
    fn test_network_info_total_bytes_overflow_safe() {
        let info = NetworkInfo {
            rx_bytes_per_sec: u64::MAX,
            tx_bytes_per_sec: 1,
            ..Default::default()
        };
        assert_eq!(info.total_bytes_per_sec(), u64::MAX);
    }

    // ======================================================================
    // ResourceMonitor construction and mode tests
    // ======================================================================

    #[test]
    fn test_monitor_new_defaults_to_compact() {
        let monitor = ResourceMonitor::new(320.0, 40.0);
        assert_eq!(monitor.mode(), DisplayMode::Compact);
        assert_eq!(monitor.width(), 320.0);
        assert_eq!(monitor.height(), 40.0);
    }

    #[test]
    fn test_monitor_toggle_mode() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        assert_eq!(monitor.mode(), DisplayMode::Compact);
        monitor.toggle_mode();
        assert_eq!(monitor.mode(), DisplayMode::Expanded);
        monitor.toggle_mode();
        assert_eq!(monitor.mode(), DisplayMode::Compact);
    }

    #[test]
    fn test_monitor_set_mode() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        assert_eq!(monitor.mode(), DisplayMode::Expanded);
        monitor.set_mode(DisplayMode::Compact);
        assert_eq!(monitor.mode(), DisplayMode::Compact);
    }

    #[test]
    fn test_monitor_resize() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.resize(640.0, 800.0);
        assert_eq!(monitor.width(), 640.0);
        assert_eq!(monitor.height(), 800.0);
    }

    // ======================================================================
    // Update / data flow tests
    // ======================================================================

    fn make_snapshot(cpu: f32, mem_used: u64, disk_busy: f32, net_rx: u64) -> ResourceSnapshot {
        ResourceSnapshot {
            timestamp_ms: 1000,
            cpu: CpuInfo {
                total_usage: cpu,
                ..Default::default()
            },
            memory: MemoryInfo {
                total_mb: 16384,
                used_mb: mem_used,
                ..Default::default()
            },
            disk: DiskInfo {
                busy_pct: disk_busy,
                ..Default::default()
            },
            network: NetworkInfo {
                rx_bytes_per_sec: net_rx,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_monitor_update_pushes_cpu() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);
        monitor.update(&make_snapshot(75.0, 8192, 50.0, 1_000_000));
        assert_eq!(monitor.cpu_data.latest(), 75.0);
    }

    #[test]
    fn test_monitor_update_pushes_memory() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);
        monitor.update(&make_snapshot(50.0, 8192, 50.0, 1_000_000));
        // 8192 / 16384 * 100 = 50.0
        assert!((monitor.mem_data.latest() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_monitor_update_pushes_disk() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);
        monitor.update(&make_snapshot(50.0, 8192, 33.3, 1_000_000));
        assert!((monitor.disk_data.latest() - 33.3).abs() < 0.1);
    }

    #[test]
    fn test_monitor_update_network_normalization() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);

        // First snapshot sets the peak.
        monitor.update(&make_snapshot(50.0, 8192, 50.0, 1_000_000));
        assert!((monitor.net_data.latest() - 100.0).abs() < 0.1);

        // Second snapshot at half the peak.
        let mut snap = make_snapshot(50.0, 8192, 50.0, 500_000);
        snap.network.rx_bytes_per_sec = 500_000;
        monitor.update(&snap);
        assert!((monitor.net_data.latest() - 50.0).abs() < 0.5);
    }

    #[test]
    fn test_monitor_graph_data_accessor() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);
        monitor.update(&make_snapshot(80.0, 4096, 10.0, 0));

        assert_eq!(monitor.graph_data(ResourceType::Cpu).latest(), 80.0);
        assert!((monitor.graph_data(ResourceType::Memory).latest() - 25.0).abs() < 0.1);
        assert_eq!(monitor.graph_data(ResourceType::Disk).latest(), 10.0);
    }

    // ======================================================================
    // Rendering tests — compact mode
    // ======================================================================

    #[test]
    fn test_render_compact_empty_produces_background() {
        let monitor = ResourceMonitor::new(320.0, 40.0);
        let cmds = monitor.render();

        // Should have at least background + border.
        assert!(cmds.len() >= 2);
        match &cmds[0] {
            RenderCommand::FillRect { color, corner_radii, .. } => {
                assert_eq!(*color, theme::BASE);
                assert_eq!(*corner_radii, CornerRadii::all(4.0));
            }
            other => panic!("Expected FillRect, got {other:?}"),
        }
    }

    #[test]
    fn test_render_compact_with_data_has_lines() {
        let mut monitor = ResourceMonitor::new(320.0, 40.0);
        for i in 0..10 {
            monitor.update(&make_snapshot(i as f32 * 10.0, 8192, 50.0, 1_000_000));
        }
        let cmds = monitor.render();

        let line_count = cmds.iter().filter(|c| matches!(c, RenderCommand::Line { .. })).count();
        // 4 sparklines, each with 9 line segments (10 points) = 36 lines.
        assert!(
            line_count > 0,
            "Expected line commands for sparklines, found none",
        );
    }

    #[test]
    fn test_render_compact_too_narrow_skips_sparklines() {
        let monitor = ResourceMonitor::new(20.0, 40.0);
        let cmds = monitor.render();
        // Only background and border when too narrow for sparklines.
        assert_eq!(cmds.len(), 2);
    }

    // ======================================================================
    // Rendering tests — expanded mode
    // ======================================================================

    #[test]
    fn test_render_expanded_has_title() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        let cmds = monitor.render();

        let has_title = cmds.iter().any(|c| matches!(c,
            RenderCommand::Text { text, font_weight: FontWeightHint::Bold, .. }
            if text == "Resource Monitor"
        ));
        assert!(has_title, "Expected 'Resource Monitor' title in expanded view");
    }

    #[test]
    fn test_render_expanded_has_panel_backgrounds() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        let cmds = monitor.render();

        // Should have 4 panel backgrounds (Surface0 fill rects) plus the outer bg.
        let surface0_rects = cmds.iter().filter(|c| matches!(c,
            RenderCommand::FillRect { color, .. } if *color == theme::SURFACE0
        )).count();
        assert_eq!(surface0_rects, 4, "Expected 4 panel backgrounds");
    }

    #[test]
    fn test_render_expanded_has_resource_labels() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        monitor.update(&make_snapshot(50.0, 8192, 25.0, 500_000));
        let cmds = monitor.render();

        let labels: Vec<&str> = cmds.iter().filter_map(|c| match c {
            RenderCommand::Text { text, color, .. } if *color == theme::CPU
                || *color == theme::MEMORY
                || *color == theme::DISK
                || *color == theme::NETWORK => Some(text.as_str()),
            _ => None,
        }).collect();
        assert!(labels.contains(&"CPU"), "Missing CPU label");
        assert!(labels.contains(&"RAM"), "Missing RAM label");
        assert!(labels.contains(&"Disk"), "Missing Disk label");
        assert!(labels.contains(&"Net"), "Missing Net label");
    }

    #[test]
    fn test_render_expanded_has_grid_lines() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        let cmds = monitor.render();

        let grid_lines = cmds.iter().filter(|c| matches!(c,
            RenderCommand::Line { color, width, .. }
            if *color == theme::SURFACE1 && (*width - 0.5).abs() < f32::EPSILON
        )).count();
        // 4 panels * GRID_LINE_COUNT grid lines each.
        assert_eq!(
            grid_lines,
            4 * GRID_LINE_COUNT,
            "Expected {} grid lines, got {grid_lines}",
            4 * GRID_LINE_COUNT,
        );
    }

    // ======================================================================
    // Bar graph rendering test
    // ======================================================================

    #[test]
    fn test_render_bar_graph_produces_rects() {
        let mut data = GraphData::new();
        data.push(25.0);
        data.push(50.0);
        data.push(75.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_bar_graph(
            &mut cmds, &data, 0.0, 0.0, 100.0, 50.0, theme::CPU,
        );

        let fill_count = cmds.iter().filter(|c| matches!(c, RenderCommand::FillRect { .. })).count();
        assert_eq!(fill_count, 3, "Expected 3 bars for 3 data points");
    }

    #[test]
    fn test_render_bar_graph_empty_data() {
        let data = GraphData::new();
        let mut cmds = Vec::new();
        ResourceMonitor::render_bar_graph(
            &mut cmds, &data, 0.0, 0.0, 100.0, 50.0, theme::CPU,
        );
        assert!(cmds.is_empty(), "Empty data should produce no bar commands");
    }

    // ======================================================================
    // Sparkline edge cases
    // ======================================================================

    #[test]
    fn test_sparkline_single_sample_no_lines() {
        let mut data = GraphData::new();
        data.push(50.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds, &data, 0.0, 0.0, 100.0, 50.0, theme::CPU,
        );
        // Need at least 2 points to draw a line.
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_sparkline_two_samples_one_line() {
        let mut data = GraphData::new();
        data.push(25.0);
        data.push(75.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds, &data, 0.0, 0.0, 100.0, 50.0, theme::CPU,
        );
        assert_eq!(cmds.len(), 1);
        assert!(matches!(&cmds[0], RenderCommand::Line { .. }));
    }

    #[test]
    fn test_sparkline_values_clamped() {
        let mut data = GraphData::new();
        data.push(-10.0);
        data.push(150.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds, &data, 0.0, 0.0, 100.0, 50.0, theme::CPU,
        );

        // Should produce a line; the clamp should prevent y from escaping bounds.
        assert_eq!(cmds.len(), 1);
        if let RenderCommand::Line { y1, y2, .. } = &cmds[0] {
            // -10 clamped to 0 => y1 = 0 + 50 - 0 = 50 (bottom)
            // 150 clamped to 100 => y2 = 0 + 50 - 50 = 0 (top)
            assert!((*y1 - 50.0).abs() < f32::EPSILON, "y1 should be at bottom");
            assert!((*y2 - 0.0).abs() < f32::EPSILON, "y2 should be at top");
        } else {
            panic!("Expected Line command");
        }
    }

    // ======================================================================
    // Format helpers
    // ======================================================================

    #[test]
    fn test_format_bytes_per_sec_bytes() {
        assert_eq!(format_bytes_per_sec(0), "0 B/s");
        assert_eq!(format_bytes_per_sec(512), "512 B/s");
    }

    #[test]
    fn test_format_bytes_per_sec_kilobytes() {
        assert_eq!(format_bytes_per_sec(1024), "1.0 KB/s");
        assert_eq!(format_bytes_per_sec(2560), "2.5 KB/s");
    }

    #[test]
    fn test_format_bytes_per_sec_megabytes() {
        assert_eq!(format_bytes_per_sec(1_048_576), "1.0 MB/s");
    }

    #[test]
    fn test_format_bytes_per_sec_gigabytes() {
        assert_eq!(format_bytes_per_sec(1_073_741_824), "1.0 GB/s");
    }
}
