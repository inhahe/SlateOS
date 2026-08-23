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
//! let commands = monitor.render(&palette);
//! ```
//!
//! # Colour
//!
//! Every colour here is read from the [`Palette`] the caller supplies; this
//! module holds none of its own. Four judgements govern which role goes where,
//! and each is a test rather than a comment.
//!
//! 1. **A hue here names a measurement, never a state and never a position.**
//!    This is the one module in the shell that draws *no* accent at all. There
//!    is nothing to select, nothing to invite, nothing in force — only six
//!    quantities that have to be told apart. So the accent count is asserted to
//!    be **zero**, which is a claim that only holds up when the accent is moved
//!    off blue: `Cpu` is blue, and the stock accent is also blue, so under the
//!    shipped theme "no accent here" and "the CPU line is accented" draw the
//!    same pixels.
//! 2. **Six measurements are six distinct colours, each pinned to the role it
//!    names.** Distinctness alone is not enough — six distinct hues stay six
//!    distinct hues under a permutation, and a graph whose CPU line is drawn in
//!    the memory colour is wrong in a way no set can see.
//! 3. **The grid is furniture.** It is a surface role, dimmer than any reading
//!    drawn over it, and it must not collide with any of the six metric hues:
//!    a gridline the colour of a line is a reading the user did not take.
//! 4. **A metric's label, its sparkline and its bars are one colour said three
//!    times**, derived from [`ResourceType::color`] rather than named beside
//!    it — so adding a resource cannot leave a graph drawn in the old one's
//!    hue.

use appearance::Palette;
use guitk::color::Color;
use guitk::history::SampleHistory;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

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
    /// Every resource the monitor graphs, in the order it shows them.
    ///
    /// Written here so the tests and any future menu iterate the same list.
    /// Adding a variant without adding it here is caught by
    /// `every_resource_type_appears_in_all_exactly_once`, which matches
    /// exhaustively.
    pub const ALL: [Self; 6] = [
        Self::Cpu,
        Self::Memory,
        Self::Disk,
        Self::Network,
        Self::Gpu,
        Self::Temperature,
    ];

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

    /// The hue that stands for this resource.
    ///
    /// A *named hue*, never `p.accent`, even for `Cpu` — whose colour is the
    /// same pixel as the stock accent and so cannot be told apart from it until
    /// the user picks a different one. See judgement 1 in the module docs:
    /// these answer "which measurement", and this module has nothing at all to
    /// say about position.
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Cpu => p.blue,
            Self::Memory => p.green,
            Self::Disk => p.peach,
            Self::Network => p.mauve,
            Self::Gpu => p.lavender,
            Self::Temperature => p.red,
        }
    }
}

// ============================================================================
// Circular graph data buffer
// ============================================================================

/// The sample history behind each of this monitor's graphs.
///
/// This used to be a `GraphData` written out here: an array, a write cursor
/// wrapped with `% GRAPH_BUFFER_SIZE`, and a count that stopped climbing once
/// the array was full. The process explorer and the system monitor each had
/// their own copy of the same thing, and the three had already drifted apart in
/// how they advanced the cursor and what `peak` returned. It lives in
/// [`guitk::history`] now; only the length is this monitor's business.
pub type GraphData = SampleHistory;

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
    #[must_use]
    pub fn usage_pct(&self) -> f32 {
        ratio::percent(self.used_mb, self.total_mb).unwrap_or(0.0) as f32
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
            cpu_data: GraphData::new(GRAPH_BUFFER_SIZE),
            mem_data: GraphData::new(GRAPH_BUFFER_SIZE),
            disk_data: GraphData::new(GRAPH_BUFFER_SIZE),
            net_data: GraphData::new(GRAPH_BUFFER_SIZE),
            gpu_data: GraphData::new(GRAPH_BUFFER_SIZE),
            temp_data: GraphData::new(GRAPH_BUFFER_SIZE),
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
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        match self.mode {
            DisplayMode::Compact => self.render_compact(p),
            DisplayMode::Expanded => self.render_expanded(p),
        }
    }

    // ======================================================================
    // Compact mode rendering
    // ======================================================================

    /// Render compact mode: a single strip with four mini sparklines.
    fn render_compact(&self, p: &Palette) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: p.base,
            corner_radii: CornerRadii::all(4.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: p.surface0,
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

            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, res.color(p));
        }

        cmds
    }

    // ======================================================================
    // Expanded mode rendering
    // ======================================================================

    /// Render expanded mode: four stacked graph panels.
    fn render_expanded(&self, p: &Palette) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Outer background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: p.base,
            corner_radii: CornerRadii::all(6.0),
        });

        // Outer border.
        cmds.push(RenderCommand::StrokeRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: p.surface0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PANEL_PADDING,
            y: PANEL_PADDING,
            text: "Resource Monitor".to_string(),
            color: p.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.width - PANEL_PADDING * 2.0),
            overflow: TextOverflow::Ellipsis,
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
            self.render_panel(&mut cmds, p, res, px, py, panel_w, panel_h);
        }

        cmds
    }

    /// Render a single graph panel (expanded mode).
    fn render_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        resource: ResourceType,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let color = resource.color(p);
        let data = self.graph_data(resource);

        // Panel background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: p.surface0,
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
            overflow: TextOverflow::Ellipsis,
        });

        // Current value text.
        let value_text = self.format_value(resource, data.latest());
        cmds.push(RenderCommand::Text {
            x: x + w - 80.0,
            y: label_y,
            text: value_text,
            color: p.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(74.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Peak value (dimmer).
        let peak_text = format!("peak {}", self.format_value(resource, data.peak()));
        cmds.push(RenderCommand::Text {
            x: x + 6.0,
            y: label_y + 12.0,
            text: peak_text,
            color: p.subtext0,
            font_size: 9.0,
            font_weight: FontWeightHint::Light,
            max_width: Some(w * 0.5),
            overflow: TextOverflow::Ellipsis,
        });

        // Graph area (below labels).
        let graph_x = x + 4.0;
        let graph_y = y + LABEL_HEIGHT + 6.0;
        let graph_w = w - 8.0;
        let graph_h = h - LABEL_HEIGHT - 10.0;

        if graph_w > 0.0 && graph_h > 0.0 {
            Self::render_grid_lines(cmds, p, graph_x, graph_y, graph_w, graph_h);
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
        let samples = data.to_vec();
        let count = samples.len();
        if count < 2 || w < MIN_SPARKLINE_WIDTH || h < 2.0 {
            return;
        }

        let max_val = 100.0_f32; // percentage scale
        let step = w / (count as f32 - 1.0);

        // `windows(2)` yields the consecutive pairs directly. Indexing `i`
        // and `i - 1` needed a `.unwrap_or(0.0)` on each end, which would
        // have drawn a spike down to the floor from a sample that was merely
        // missing — a fabricated reading in a graph of real ones.
        for (segment, pair) in samples.windows(2).enumerate() {
            let [prev, curr] = *pair else { continue };

            let x1 = x + segment as f32 * step;
            let y1 = y + h - (prev.clamp(0.0, max_val) / max_val * h);
            let x2 = x + (segment as f32 + 1.0) * step;
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
        let samples = data.to_vec();
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
        p: &Palette,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let grid_color = p.surface1;
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
///
/// Only `ResourceType::Network` reaches here, so this is a link rate and is
/// decimal — the same convention as the tray indicator, which the user can see
/// at the same time as this graph. It used to divide by 1024 and write `KB/s`,
/// so the two disagreed by 2.4% while claiming the same unit. See
/// design-decisions.md §489.
fn format_bytes_per_sec(bps: u64) -> String {
    guitk::bytes::si_rate(bps)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check::assert_drawn_from;

    /// A palette whose accent belongs to no palette.
    ///
    /// The stock accent **is** `blue`, and `blue` is this module's colour
    /// for `Cpu` — so under the shipped theme "this module draws no accent"
    /// and "every CPU line is accented" are the same picture. The loop below
    /// proves the substitute really is outside the palette rather than
    /// coincidentally equal to a role a test then reads by accident.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        for (name, role) in p.roles() {
            if name == "accent" {
                continue;
            }
            assert_ne!(
                (role.r, role.g, role.b),
                (p.accent.r, p.accent.g, p.accent.b),
                "the substitute accent collides with {name}, so an \
                 accent assertion would be reading that role instead"
            );
        }
        p
    }

    // ======================================================================
    // GraphData tests
    // ======================================================================

    #[test]
    fn test_graph_data_new_is_empty() {
        let data = GraphData::new(GRAPH_BUFFER_SIZE);
        assert!(data.is_empty());
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_graph_data_push_single() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(42.0);
        assert_eq!(data.len(), 1);
        assert!(!data.is_empty());
        assert_eq!(data.latest(), 42.0);
    }

    #[test]
    fn test_graph_data_push_multiple() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(10.0);
        data.push(20.0);
        data.push(30.0);
        assert_eq!(data.len(), 3);
        assert_eq!(data.latest(), 30.0);
    }

    #[test]
    fn test_graph_data_wraps_at_capacity() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        for i in 0..GRAPH_BUFFER_SIZE + 10 {
            data.push(i as f32);
        }
        assert_eq!(data.len(), GRAPH_BUFFER_SIZE);
        assert_eq!(data.latest(), (GRAPH_BUFFER_SIZE + 9) as f32);
    }

    #[test]
    fn test_graph_data_latest_empty() {
        let data = GraphData::new(GRAPH_BUFFER_SIZE);
        assert_eq!(data.latest(), 0.0);
    }

    #[test]
    fn test_graph_data_average_empty() {
        let data = GraphData::new(GRAPH_BUFFER_SIZE);
        assert_eq!(data.average(), 0.0);
    }

    #[test]
    fn test_graph_data_average_single() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(50.0);
        assert_eq!(data.average(), 50.0);
    }

    #[test]
    fn test_graph_data_average_multiple() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(10.0);
        data.push(20.0);
        data.push(30.0);
        let avg = data.average();
        assert!((avg - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_graph_data_peak_empty() {
        let data = GraphData::new(GRAPH_BUFFER_SIZE);
        assert_eq!(data.peak(), 0.0);
    }

    #[test]
    fn test_graph_data_peak() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(10.0);
        data.push(99.0);
        data.push(30.0);
        assert_eq!(data.peak(), 99.0);
    }

    #[test]
    fn test_graph_data_peak_after_wrap() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
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
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(1.0);
        data.push(2.0);
        data.push(3.0);
        let samples = data.to_vec();
        assert_eq!(samples, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_graph_data_valid_samples_after_wrap() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        for i in 0..(GRAPH_BUFFER_SIZE + 5) {
            data.push(i as f32);
        }
        let samples = data.to_vec();
        assert_eq!(samples.len(), GRAPH_BUFFER_SIZE);
        // Oldest should be 5, newest should be GRAPH_BUFFER_SIZE + 4.
        assert_eq!(samples[0], 5.0);
        assert_eq!(
            samples[GRAPH_BUFFER_SIZE - 1],
            (GRAPH_BUFFER_SIZE + 4) as f32
        );
    }

    /// How the buffer behaves is `guitk::history`'s business and is tested
    /// there. The one thing this module still decides is how long it is, and
    /// every graph in the monitor has to agree on that or they would scroll at
    /// different rates across the same window of time.
    #[test]
    fn every_graph_in_the_monitor_keeps_the_same_length_of_history() {
        let monitor = ResourceMonitor::new(400.0, 300.0);
        for resource in ResourceType::ALL {
            let data = monitor.graph_data(resource);
            assert!(data.is_empty(), "{resource:?}");
            assert_eq!(data.capacity(), GRAPH_BUFFER_SIZE, "{resource:?}");
        }
    }

    /// `ALL` is a hand-written list, so something has to notice a variant that
    /// is added to the enum and not to it. The exhaustive match below does:
    /// adding a variant stops this compiling until the position is filled in.
    #[test]
    fn every_resource_type_appears_in_all_exactly_once() {
        fn position(resource: ResourceType) -> usize {
            match resource {
                ResourceType::Cpu => 0,
                ResourceType::Memory => 1,
                ResourceType::Disk => 2,
                ResourceType::Network => 3,
                ResourceType::Gpu => 4,
                ResourceType::Temperature => 5,
            }
        }

        let mut seen = [false; ResourceType::ALL.len()];
        for (index, resource) in ResourceType::ALL.iter().enumerate() {
            assert_eq!(position(*resource), index, "{resource:?} is out of order");
            seen[index] = true;
        }
        assert!(seen.iter().all(|s| *s), "every variant must be listed");
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
        let types = ResourceType::ALL;
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i].color(&accented(false)),
                    types[j].color(&accented(false)),
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
        let cmds = monitor.render(&accented(false));

        // Should have at least background + border.
        assert!(cmds.len() >= 2);
        match &cmds[0] {
            RenderCommand::FillRect {
                color,
                corner_radii,
                ..
            } => {
                assert_eq!(*color, accented(false).base);
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
        let cmds = monitor.render(&accented(false));

        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        // 4 sparklines, each with 9 line segments (10 points) = 36 lines.
        assert!(
            line_count > 0,
            "Expected line commands for sparklines, found none",
        );
    }

    #[test]
    fn test_render_compact_too_narrow_skips_sparklines() {
        let monitor = ResourceMonitor::new(20.0, 40.0);
        let cmds = monitor.render(&accented(false));
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
        let cmds = monitor.render(&accented(false));

        let has_title = cmds.iter().any(|c| {
            matches!(c,
                RenderCommand::Text { text, font_weight: FontWeightHint::Bold, .. }
                if text == "Resource Monitor"
            )
        });
        assert!(
            has_title,
            "Expected 'Resource Monitor' title in expanded view"
        );
    }

    #[test]
    fn test_render_expanded_has_panel_backgrounds() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        let cmds = monitor.render(&accented(false));

        // Should have 4 panel backgrounds (Surface0 fill rects) plus the outer bg.
        let surface0_rects = cmds
            .iter()
            .filter(|c| {
                matches!(c,
                    RenderCommand::FillRect { color, .. } if *color == accented(false).surface0
                )
            })
            .count();
        assert_eq!(surface0_rects, 4, "Expected 4 panel backgrounds");
    }

    #[test]
    fn test_render_expanded_has_resource_labels() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        monitor.update(&make_snapshot(50.0, 8192, 25.0, 500_000));
        let cmds = monitor.render(&accented(false));

        let labels: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. }
                    if *color == accented(false).blue
                        || *color == accented(false).green
                        || *color == accented(false).peach
                        || *color == accented(false).mauve =>
                {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"CPU"), "Missing CPU label");
        assert!(labels.contains(&"RAM"), "Missing RAM label");
        assert!(labels.contains(&"Disk"), "Missing Disk label");
        assert!(labels.contains(&"Net"), "Missing Net label");
    }

    #[test]
    fn test_render_expanded_has_grid_lines() {
        let mut monitor = ResourceMonitor::new(320.0, 400.0);
        monitor.set_mode(DisplayMode::Expanded);
        let cmds = monitor.render(&accented(false));

        let grid_lines = cmds
            .iter()
            .filter(|c| {
                matches!(c,
                    RenderCommand::Line { color, width, .. }
                    if *color == accented(false).surface1 && (*width - 0.5).abs() < f32::EPSILON
                )
            })
            .count();
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
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(25.0);
        data.push(50.0);
        data.push(75.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_bar_graph(
            &mut cmds,
            &data,
            0.0,
            0.0,
            100.0,
            50.0,
            accented(false).blue,
        );

        let fill_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert_eq!(fill_count, 3, "Expected 3 bars for 3 data points");
    }

    #[test]
    fn test_render_bar_graph_empty_data() {
        let data = GraphData::new(GRAPH_BUFFER_SIZE);
        let mut cmds = Vec::new();
        ResourceMonitor::render_bar_graph(
            &mut cmds,
            &data,
            0.0,
            0.0,
            100.0,
            50.0,
            accented(false).blue,
        );
        assert!(cmds.is_empty(), "Empty data should produce no bar commands");
    }

    // ======================================================================
    // Sparkline edge cases
    // ======================================================================

    #[test]
    fn test_sparkline_single_sample_no_lines() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(50.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds,
            &data,
            0.0,
            0.0,
            100.0,
            50.0,
            accented(false).blue,
        );
        // Need at least 2 points to draw a line.
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_sparkline_two_samples_one_line() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(25.0);
        data.push(75.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds,
            &data,
            0.0,
            0.0,
            100.0,
            50.0,
            accented(false).blue,
        );
        assert_eq!(cmds.len(), 1);
        assert!(matches!(&cmds[0], RenderCommand::Line { .. }));
    }

    #[test]
    fn test_sparkline_values_clamped() {
        let mut data = GraphData::new(GRAPH_BUFFER_SIZE);
        data.push(-10.0);
        data.push(150.0);

        let mut cmds = Vec::new();
        ResourceMonitor::render_sparkline(
            &mut cmds,
            &data,
            0.0,
            0.0,
            100.0,
            50.0,
            accented(false).blue,
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
        assert_eq!(format_bytes_per_sec(1024), "1.0 kB/s");
        assert_eq!(format_bytes_per_sec(2560), "2.6 kB/s");
    }

    #[test]
    fn test_format_bytes_per_sec_megabytes() {
        assert_eq!(format_bytes_per_sec(1_048_576), "1.0 MB/s");
    }

    #[test]
    fn test_format_bytes_per_sec_gigabytes() {
        assert_eq!(format_bytes_per_sec(1_073_741_824), "1.1 GB/s");
    }
    // ======================================================================
    // Colour — TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE
    // ======================================================================

    /// A monitor with enough history that every graph actually draws.
    ///
    /// `render_sparkline` returns early below two samples and `render_panel`
    /// draws its grid only when the graph area has positive size — so a monitor
    /// straight out of `new` renders a background, a border and nothing else.
    /// A sweep over *that* would check two commands and pass, which is the
    /// vacuous-coverage trap in its purest form.
    fn monitor(mode: DisplayMode) -> ResourceMonitor {
        let mut m = ResourceMonitor::new(320.0, 400.0);
        m.set_mode(mode);
        for i in 0..8 {
            let v = 10.0 + i as f32 * 7.0;
            m.update(&make_snapshot(v, 4096 + i * 512, v / 2.0, 1_000_000));
        }
        m
    }

    /// Every colour in `cmds`, whatever command carries it.
    fn all_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one `Text` command reading `s`.
    fn text_color(cmds: &[RenderCommand], s: &str) -> Color {
        let found: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == s => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one command reading {s:?}");
        found[0]
    }

    /// The colours of every `Line` of exactly `width` points.
    fn line_colors(cmds: &[RenderCommand], line_width: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Line { color, width, .. } if *width == line_width => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The four resources the widget actually graphs, in the order it draws
    /// them. `ResourceType::ALL` has six; GPU and temperature are collected but
    /// never plotted, which is a distinction a test that looped over `ALL`
    /// would silently paper over.
    const GRAPHED: [(ResourceType, &str); 4] = [
        (ResourceType::Cpu, "CPU"),
        (ResourceType::Memory, "RAM"),
        (ResourceType::Disk, "Disk"),
        (ResourceType::Network, "Net"),
    ];

    /// Nothing this monitor draws comes from outside the palette it was given.
    ///
    /// Both modes and both display modes: compact and expanded draw different
    /// trees, and a constant left behind in the one the test does not render is
    /// a constant the test cannot see.
    #[test]
    fn every_colour_this_monitor_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            for mode in [DisplayMode::Compact, DisplayMode::Expanded] {
                let cmds = monitor(mode).render(&p);
                assert!(
                    cmds.len() > 10,
                    "{mode:?} drew {} commands, which is not a graph",
                    cmds.len()
                );
                assert_drawn_from(&p, &cmds, &[], "resource monitor");
            }
        }
    }

    /// None of the eleven deleted constants is still drawn.
    ///
    /// Every value below is a Catppuccin **Mocha** colour, so a light render
    /// cannot legitimately produce one — which turns "a substitution was
    /// missed" from an invisible defect into a named failure.
    #[test]
    fn none_of_the_eleven_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 11] = [
            ("BASE", 0x001E_1E2E),
            ("SURFACE0", 0x0031_3244),
            ("SURFACE1", 0x0045_475A),
            ("TEXT", 0x00CD_D6F4),
            ("SUBTEXT", 0x00A6_ADC8),
            ("CPU", 0x0089_B4FA),
            ("MEMORY", 0x00A6_E3A1),
            ("DISK", 0x00FA_B387),
            ("NETWORK", 0x00CB_A6F7),
            ("TEMPERATURE", 0x00F3_8BA8),
            ("GPU", 0x00B4_BEFE),
        ];

        let p = accented(true);
        let mut cmds = monitor(DisplayMode::Expanded).render(&p);
        cmds.extend(monitor(DisplayMode::Compact).render(&p));
        for c in all_colors(&cmds) {
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, deleted) in DELETED {
                assert_ne!(
                    rgb, deleted,
                    "the monitor still draws the deleted constant {name} \
                     (#{deleted:06X}) in a light render"
                );
            }
        }
    }

    /// Every site, named one at a time, in the role this module claims for it.
    ///
    /// The sweep above proves only *membership*, and membership cannot see a
    /// swap: a panel painted `surface1` instead of `surface0` draws a legal
    /// colour, and so does a peak reading painted `subtext1`. n source sites
    /// need n assertions, so this is that table.
    #[test]
    fn every_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);

            // Compact: a background and a border, and nothing else with a fill.
            let compact = monitor(DisplayMode::Compact).render(&p);
            assert_eq!(
                compact.first().map(|c| match c {
                    RenderCommand::FillRect { color, .. } => *color,
                    _ => panic!("the compact strip does not start with its background"),
                }),
                Some(p.base),
                "the compact strip's background"
            );
            let strokes: Vec<Color> = compact
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(strokes, vec![p.surface0], "the compact strip's border");

            // Expanded: the same two, plus a title, four panels and their text.
            let cmds = monitor(DisplayMode::Expanded).render(&p);
            assert_eq!(
                cmds.first().map(|c| match c {
                    RenderCommand::FillRect { color, .. } => *color,
                    _ => panic!("the panel does not start with its background"),
                }),
                Some(p.base),
                "the expanded widget's background"
            );
            let strokes: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(strokes, vec![p.surface0], "the expanded widget's border");
            assert_eq!(
                text_color(&cmds, "Resource Monitor"),
                p.text,
                "the widget's own title is the brightest thing on it"
            );

            // One panel per graphed resource, all on the same rung.
            let panels: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { width, color, .. }
                        if *width == 320.0 - PANEL_PADDING * 2.0 =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(panels.len(), GRAPHED.len(), "one background per panel");
            assert!(
                panels.iter().all(|c| *c == p.surface0),
                "a panel is one rung above the widget it sits in"
            );

            // Each panel's three lines of text: the label in the metric's own
            // hue, the current reading in ordinary ink, the peak a rung dimmer.
            for (resource, label) in GRAPHED {
                assert_eq!(
                    text_color(&cmds, label),
                    resource.color(&p),
                    "the {label} label is not drawn in its own metric's hue"
                );
            }
            let values: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        color, font_size, ..
                    } if *font_size == 11.0 => Some(*color),
                    _ => None,
                })
                .collect();
            // Four labels and four readings share the 11pt row; the labels are
            // the metric hues, so what is left over is the readings.
            let readings = values.iter().filter(|c| **c == p.text).count();
            assert_eq!(readings, GRAPHED.len(), "one current reading per panel");
            let peaks: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        color, font_size, ..
                    } if *font_size == 9.0 => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(peaks.len(), GRAPHED.len(), "one peak reading per panel");
            assert!(
                peaks.iter().all(|c| *c == p.subtext0),
                "a peak is a rung dimmer than the reading it qualifies"
            );

            // The grid, which is the module's only half-point line.
            let grid = line_colors(&cmds, 0.5);
            assert_eq!(
                grid.len(),
                GRID_LINE_COUNT * GRAPHED.len(),
                "four grid lines per panel"
            );
            assert!(
                grid.iter().all(|c| *c == p.surface1),
                "the grid is furniture, drawn on the surface rungs"
            );
        }
    }

    /// Judgement 1: this module draws no accent at all.
    ///
    /// There is nothing here to select, nothing to invite and nothing in
    /// force — only quantities that have to be told apart. That claim is worth
    /// nothing at the shipped accent, which *is* `blue`, which is `Cpu`: every
    /// CPU line would answer to it and the count would read 40 rather than 0
    /// for a reason that has nothing to do with position. Hence the off-palette
    /// accent, without which this test would be asserting the opposite of what
    /// it says.
    #[test]
    fn no_colour_in_this_module_marks_a_position() {
        for light in [false, true] {
            let p = accented(light);
            for mode in [DisplayMode::Compact, DisplayMode::Expanded] {
                let cmds = monitor(mode).render(&p);
                let n = all_colors(&cmds).iter().filter(|c| **c == p.accent).count();
                assert_eq!(
                    n, 0,
                    "{mode:?} draws {n} accented commands, but a monitor has \
                     nothing to say about where you are"
                );
            }
        }
    }

    /// Judgement 2: six measurements are six distinct colours, each pinned to
    /// the role it names.
    ///
    /// The table is the point. `test_resource_type_colors_distinct` already
    /// checks the six are distinct, and distinctness survives a permutation
    /// untouched — six distinct hues rotated by one are still six distinct
    /// hues, and every graph in the widget is then drawn in its neighbour's
    /// colour. Only naming the pairs can fail that, and comparing a drawn
    /// command to `ResourceType::color` cannot, because that asks the code
    /// under test what it meant.
    #[test]
    fn each_measurement_is_pinned_to_the_role_it_names() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = monitor(DisplayMode::Expanded).render(&p);
            for (resource, expected, label) in [
                (ResourceType::Cpu, p.blue, "CPU"),
                (ResourceType::Memory, p.green, "RAM"),
                (ResourceType::Disk, p.peach, "Disk"),
                (ResourceType::Network, p.mauve, "Net"),
                (ResourceType::Gpu, p.lavender, "GPU"),
                (ResourceType::Temperature, p.red, "Temp"),
            ] {
                assert_eq!(resource.color(&p), expected, "{label} names the wrong role");
            }
            // And the four that reach the screen are drawn in those hues.
            for (resource, label) in GRAPHED {
                assert_eq!(
                    text_color(&cmds, label),
                    resource.color(&p),
                    "the {label} panel is drawn in another metric's hue"
                );
            }
        }
    }

    /// Judgement 3: the grid is furniture and never collides with a reading.
    ///
    /// A gridline the colour of a line is a reading the user did not take. The
    /// claim is checked against the six metric hues rather than the four drawn
    /// ones, so that plotting GPU or temperature later cannot quietly make the
    /// grid ambiguous.
    #[test]
    fn no_gridline_can_be_mistaken_for_a_reading() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = monitor(DisplayMode::Expanded).render(&p);

            let grid = line_colors(&cmds, 0.5);
            assert!(!grid.is_empty(), "no grid was drawn to check");
            for g in &grid {
                for resource in ResourceType::ALL {
                    assert_ne!(
                        *g,
                        resource.color(&p),
                        "a gridline is the same colour as the {} graph",
                        resource.label()
                    );
                }
                assert_ne!(*g, p.text, "a gridline is as bright as a reading");
            }
        }
    }

    /// Judgement 4: a metric's label, its sparkline and its bars are one
    /// colour said three times.
    ///
    /// Derived from [`ResourceType::color`] at each site rather than named
    /// beside it, so that adding a resource cannot leave a graph drawn in the
    /// old one's hue. Asserted as a *relationship* between three commands,
    /// which is a claim three hand-written constants would fail even if all
    /// three happened to be plausible colours.
    #[test]
    fn a_metric_is_one_colour_wherever_it_appears() {
        for light in [false, true] {
            let p = accented(light);
            // Both display modes, because the compact strip plots the same
            // four metrics from a *different* call site with its own copy of
            // the hue lookup — and the strip carries no labels at all, so
            // there is nothing else in the module that could notice its lines
            // being drawn in the wrong metric's colour.
            for mode in [DisplayMode::Compact, DisplayMode::Expanded] {
                let cmds = monitor(mode).render(&p);

                // The sparklines are the module's only 1.5-point lines.
                let plotted = line_colors(&cmds, 1.5);
                assert!(!plotted.is_empty(), "{mode:?} drew no sparkline to check");
                for (resource, _) in GRAPHED {
                    let hue = resource.color(&p);
                    assert!(
                        plotted.contains(&hue),
                        "{:?} plots nothing in the {} hue, so one metric is \
                         drawn in another's colour",
                        mode,
                        resource.label()
                    );
                }
                // And nothing is plotted in a colour that is not a metric's.
                for c in &plotted {
                    assert!(
                        ResourceType::ALL.iter().any(|r| r.color(&p) == *c),
                        "{mode:?} draws a line in {c:?}, which is not any \
                         metric's hue"
                    );
                }
            }

            // Expanded alone carries the labels, and each names the graph
            // beneath it: same hue, two commands, one claim.
            let cmds = monitor(DisplayMode::Expanded).render(&p);
            let plotted = line_colors(&cmds, 1.5);
            for (resource, label) in GRAPHED {
                let hue = resource.color(&p);
                assert_eq!(text_color(&cmds, label), hue, "the {label} label");
                assert!(
                    plotted.contains(&hue),
                    "the {label} label is drawn in a hue no line on the screen \
                     uses, so the label names a graph that is not there"
                );
            }

            // The bar graph takes the same hue at the same site.
            let mut bars = Vec::new();
            let data = monitor(DisplayMode::Expanded)
                .graph_data(ResourceType::Cpu)
                .clone();
            ResourceMonitor::render_bar_graph(
                &mut bars,
                &data,
                0.0,
                0.0,
                100.0,
                50.0,
                ResourceType::Cpu.color(&p),
            );
            assert!(!bars.is_empty(), "no bars were drawn to check");
            assert!(
                all_colors(&bars).iter().all(|c| *c == p.blue),
                "a CPU bar graph is not drawn in the CPU hue"
            );
        }
    }

    /// The widget reads as a stack of panels in either mode.
    ///
    /// Asserted as an *ordering* — the panel is nearer the foreground than the
    /// widget behind it — rather than as a pair of literals, so it fails on a
    /// palette whose surface rung was made darker than its base instead of
    /// passing because two particular hexes were typed correctly.
    #[test]
    fn a_panel_reads_as_raised_above_the_widget_behind_it() {
        for light in [false, true] {
            let p = accented(light);
            let luma =
                |c: Color| 0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b);
            let toward_fg = if light { -1.0 } else { 1.0 };
            assert!(
                (luma(p.surface0) - luma(p.base)) * toward_fg > 0.0,
                "a panel on the {} palette does not stand out from the widget \
                 it sits in",
                if light { "light" } else { "dark" }
            );
            assert!(
                (luma(p.surface1) - luma(p.surface0)) * toward_fg > 0.0,
                "the grid does not stand out from the panel it is drawn on"
            );
        }
    }
}
