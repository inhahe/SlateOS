//! OurOS Network Speed Test
//!
//! Graphical network speed test utility with:
//! - Download, upload, and latency measurement
//! - Large speedometer-style arc gauge with speed markers
//! - Live throughput graph over test duration
//! - Phase indicators (Latency -> Download -> Upload)
//! - History of last 20 results with avg/best/worst stats
//! - Server selection
//! - Export results as text
//! - Dark theme (Catppuccin Mocha)
//!
//! Uses the guitk library for UI rendering. Network I/O is
//! performed through OurOS syscalls; simulated with representative
//! data for initial development.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;
use std::f32::consts::PI;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::rgb(30, 30, 46);
const MANTLE: Color = Color::rgb(24, 24, 37);
const CRUST: Color = Color::rgb(17, 17, 27);
const SURFACE0: Color = Color::rgb(49, 50, 68);
const SURFACE1: Color = Color::rgb(69, 71, 90);
const SURFACE2: Color = Color::rgb(88, 91, 112);
const TEXT_COLOR: Color = Color::rgb(205, 214, 244);
const SUBTEXT0: Color = Color::rgb(166, 173, 200);
const SUBTEXT1: Color = Color::rgb(186, 194, 222);
const BLUE: Color = Color::rgb(137, 180, 250);
const SAPPHIRE: Color = Color::rgb(116, 199, 236);
const GREEN: Color = Color::rgb(166, 227, 161);
const PEACH: Color = Color::rgb(250, 179, 135);
const RED: Color = Color::rgb(243, 139, 168);
const MAUVE: Color = Color::rgb(203, 166, 247);
const YELLOW: Color = Color::rgb(249, 226, 175);
const TEAL: Color = Color::rgb(148, 226, 213);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 720.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const GAUGE_RADIUS: f32 = 150.0;
const GAUGE_CENTER_X: f32 = WINDOW_WIDTH / 2.0;
const GAUGE_CENTER_Y: f32 = 240.0;
const GAUGE_ARC_SEGMENTS: usize = 60;
const GAUGE_ARC_START_ANGLE: f32 = 135.0;
const GAUGE_ARC_SWEEP: f32 = 270.0;
const GRAPH_X: f32 = 40.0;
const GRAPH_Y: f32 = 420.0;
const GRAPH_WIDTH: f32 = 520.0;
const GRAPH_HEIGHT: f32 = 180.0;
const HISTORY_X: f32 = 580.0;
const HISTORY_Y: f32 = 420.0;
const HISTORY_WIDTH: f32 = 300.0;
const HISTORY_HEIGHT: f32 = 260.0;
const HISTORY_ROW_HEIGHT: f32 = 26.0;
const BUTTON_WIDTH: f32 = 140.0;
const BUTTON_HEIGHT: f32 = 36.0;
const PHASE_INDICATOR_Y: f32 = 370.0;
const MAX_HISTORY: usize = 20;
const MAX_GRAPH_POINTS: usize = 120;

/// Speed markers on the gauge (in Mbps).
const GAUGE_MARKERS: &[f32] = &[0.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0];

// ============================================================================
// Speed Test Phase
// ============================================================================

/// Sub-phase of an active test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestKind {
    Download,
    Upload,
    Latency,
}

impl TestKind {
    fn label(self) -> &'static str {
        match self {
            Self::Download => "Download",
            Self::Upload => "Upload",
            Self::Latency => "Latency",
        }
    }
}

/// Overall phase of the speed test application.
#[derive(Clone, Debug, PartialEq)]
pub enum SpeedTestPhase {
    /// No test running; waiting for user action.
    Idle,
    /// Actively running a specific test kind.
    Testing(TestKind),
    /// All phases complete; results available.
    Complete,
    /// An error occurred during testing.
    Error(String),
}

impl SpeedTestPhase {
    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    fn is_testing(&self) -> bool {
        matches!(self, Self::Testing(_))
    }

    fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    fn label(&self) -> &str {
        match self {
            Self::Idle => "Ready",
            Self::Testing(kind) => kind.label(),
            Self::Complete => "Complete",
            Self::Error(_) => "Error",
        }
    }
}

// ============================================================================
// Speed Test Result
// ============================================================================

/// Complete result from a single speed test run.
#[derive(Clone, Debug)]
pub struct SpeedTestResult {
    /// Download speed in megabits per second.
    pub download_mbps: f64,
    /// Upload speed in megabits per second.
    pub upload_mbps: f64,
    /// Average round-trip latency in milliseconds.
    pub latency_ms: f64,
    /// Jitter (variation in latency) in milliseconds.
    pub jitter_ms: f64,
    /// Name of the server used for testing.
    pub server_name: String,
    /// Unix timestamp when the test completed.
    pub timestamp: u64,
    /// Percentage of packets lost during the test (0.0-100.0).
    pub packet_loss_pct: f64,
}

impl SpeedTestResult {
    /// Format the result as a human-readable summary line.
    fn summary_line(&self) -> String {
        format!(
            "D:{:.1} U:{:.1} L:{:.1}ms",
            self.download_mbps, self.upload_mbps, self.latency_ms,
        )
    }

    /// Format the full result as a multi-line text report.
    fn to_text_report(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("--- Speed Test Result ---\n");
        out.push_str(&format!("Server:       {}\n", self.server_name));
        out.push_str(&format!("Download:     {:.2} Mbps\n", self.download_mbps));
        out.push_str(&format!("Upload:       {:.2} Mbps\n", self.upload_mbps));
        out.push_str(&format!("Latency:      {:.2} ms\n", self.latency_ms));
        out.push_str(&format!("Jitter:       {:.2} ms\n", self.jitter_ms));
        out.push_str(&format!("Packet loss:  {:.1}%\n", self.packet_loss_pct));
        out.push_str(&format!("Timestamp:    {}\n", self.timestamp));
        out
    }
}

// ============================================================================
// Speed Test Configuration
// ============================================================================

/// Configuration for a speed test run.
#[derive(Clone, Debug)]
pub struct SpeedTestConfig {
    /// URL of the test server.
    pub server_url: String,
    /// Duration of each test phase in seconds.
    pub test_duration_secs: u32,
    /// Number of parallel connections for throughput tests.
    pub num_connections: u32,
    /// Size of data to transfer for download test in megabytes.
    pub download_size_mb: u32,
}

impl Default for SpeedTestConfig {
    fn default() -> Self {
        Self {
            server_url: String::from("speedtest.ouros.local"),
            test_duration_secs: 10,
            num_connections: 4,
            download_size_mb: 25,
        }
    }
}

impl SpeedTestConfig {
    /// Validate that configuration values are within sane ranges.
    fn validate(&self) -> Result<(), String> {
        if self.server_url.is_empty() {
            return Err("Server URL cannot be empty".into());
        }
        if self.test_duration_secs == 0 || self.test_duration_secs > 120 {
            return Err("Test duration must be between 1 and 120 seconds".into());
        }
        if self.num_connections == 0 || self.num_connections > 32 {
            return Err("Connection count must be between 1 and 32".into());
        }
        if self.download_size_mb == 0 || self.download_size_mb > 1000 {
            return Err("Download size must be between 1 and 1000 MB".into());
        }
        Ok(())
    }
}

// ============================================================================
// Available Test Servers
// ============================================================================

/// A server that can be used for speed testing.
#[derive(Clone, Debug)]
pub struct TestServer {
    /// Human-readable name.
    pub name: String,
    /// URL or address.
    pub url: String,
    /// Geographic location description.
    pub location: String,
    /// Estimated distance in kilometers (for display).
    pub distance_km: u32,
}

/// Returns the default list of available test servers.
fn default_servers() -> Vec<TestServer> {
    vec![
        TestServer {
            name: "OurOS Central".into(),
            url: "speedtest.ouros.local".into(),
            location: "Local Network".into(),
            distance_km: 0,
        },
        TestServer {
            name: "Metro East".into(),
            url: "east.speedtest.ouros.net".into(),
            location: "New York, US".into(),
            distance_km: 50,
        },
        TestServer {
            name: "Metro West".into(),
            url: "west.speedtest.ouros.net".into(),
            location: "Los Angeles, US".into(),
            distance_km: 3800,
        },
        TestServer {
            name: "Europe".into(),
            url: "eu.speedtest.ouros.net".into(),
            location: "Frankfurt, DE".into(),
            distance_km: 6300,
        },
        TestServer {
            name: "Asia Pacific".into(),
            url: "apac.speedtest.ouros.net".into(),
            location: "Tokyo, JP".into(),
            distance_km: 10800,
        },
    ]
}

// ============================================================================
// Latency Tester
// ============================================================================

/// Measures network latency by sending probe packets and collecting RTT data.
#[derive(Clone, Debug)]
pub struct LatencyTester {
    /// Collected round-trip times in milliseconds.
    samples: Vec<f64>,
    /// Number of probes to send.
    probe_count: u32,
    /// Number of probes sent so far.
    probes_sent: u32,
    /// Number of probes that timed out.
    probes_lost: u32,
}

impl LatencyTester {
    /// Create a new latency tester with the specified probe count.
    pub fn new(probe_count: u32) -> Self {
        Self {
            samples: Vec::with_capacity(probe_count as usize),
            probe_count,
            probes_sent: 0,
            probes_lost: 0,
        }
    }

    /// Record a successful probe with the given RTT in milliseconds.
    pub fn record_sample(&mut self, rtt_ms: f64) {
        self.probes_sent = self.probes_sent.saturating_add(1);
        if rtt_ms >= 0.0 && rtt_ms.is_finite() {
            self.samples.push(rtt_ms);
        }
    }

    /// Record a lost (timed-out) probe.
    pub fn record_loss(&mut self) {
        self.probes_sent = self.probes_sent.saturating_add(1);
        self.probes_lost = self.probes_lost.saturating_add(1);
    }

    /// Fraction of probes completed (0.0 to 1.0).
    pub fn progress(&self) -> f32 {
        if self.probe_count == 0 {
            return 1.0;
        }
        (self.probes_sent as f32 / self.probe_count as f32).min(1.0)
    }

    /// Whether all probes have been sent.
    pub fn is_complete(&self) -> bool {
        self.probes_sent >= self.probe_count
    }

    /// Minimum observed RTT, or `None` if no samples.
    pub fn min_rtt(&self) -> Option<f64> {
        self.samples.iter().copied().reduce(f64::min)
    }

    /// Maximum observed RTT, or `None` if no samples.
    pub fn max_rtt(&self) -> Option<f64> {
        self.samples.iter().copied().reduce(f64::max)
    }

    /// Average RTT across all successful probes.
    pub fn avg_rtt(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: f64 = self.samples.iter().sum();
        Some(sum / self.samples.len() as f64)
    }

    /// Jitter: average absolute difference between consecutive samples.
    pub fn jitter(&self) -> Option<f64> {
        if self.samples.len() < 2 {
            return None;
        }
        let mut total_diff = 0.0_f64;
        let mut count = 0u64;
        for pair in self.samples.windows(2) {
            // windows(2) always yields slices of length 2
            let (a, b) = (pair[0], pair[1]);
            total_diff += (b - a).abs();
            count = count.saturating_add(1);
        }
        if count == 0 {
            return None;
        }
        Some(total_diff / count as f64)
    }

    /// Packet loss percentage (0.0 to 100.0).
    pub fn packet_loss_pct(&self) -> f64 {
        if self.probes_sent == 0 {
            return 0.0;
        }
        (self.probes_lost as f64 / self.probes_sent as f64) * 100.0
    }

    /// Number of successful samples collected.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Populate with simulated latency data for development.
    pub fn simulate(&mut self, base_ms: f64, variance_ms: f64) {
        // Simple deterministic simulation using a linear congruential pattern.
        let mut pseudo = 42u64;
        for _ in 0..self.probe_count {
            pseudo = pseudo.wrapping_mul(6364136223846793005).wrapping_add(1);
            let frac = ((pseudo >> 33) as f64) / (u32::MAX as f64);
            let rtt = base_ms + (frac - 0.5) * 2.0 * variance_ms;
            if rtt > 0.0 {
                self.record_sample(rtt);
            } else {
                self.record_loss();
            }
        }
    }
}

// ============================================================================
// Throughput Tester
// ============================================================================

/// A single data point for the throughput time series.
#[derive(Clone, Copy, Debug)]
pub struct ThroughputSample {
    /// Elapsed seconds since test start.
    pub elapsed_secs: f32,
    /// Instantaneous speed in Mbps at this sample.
    pub mbps: f64,
}

/// Measures throughput by tracking bytes transferred over time across
/// multiple simulated connections.
#[derive(Clone, Debug)]
pub struct ThroughputTester {
    /// Number of parallel connections.
    num_connections: u32,
    /// Total bytes transferred so far.
    total_bytes: u64,
    /// Elapsed time of the test in seconds.
    elapsed_secs: f32,
    /// Target duration of the test in seconds.
    duration_secs: f32,
    /// Time-series data for graphing.
    samples: Vec<ThroughputSample>,
    /// Per-connection byte counts (for tracking individual connections).
    connection_bytes: Vec<u64>,
}

impl ThroughputTester {
    /// Create a new throughput tester.
    pub fn new(num_connections: u32, duration_secs: f32) -> Self {
        let conns = num_connections.max(1) as usize;
        Self {
            num_connections: num_connections.max(1),
            total_bytes: 0,
            elapsed_secs: 0.0,
            duration_secs,
            samples: Vec::with_capacity(MAX_GRAPH_POINTS),
            connection_bytes: vec![0u64; conns],
        }
    }

    /// Record bytes transferred on a specific connection.
    pub fn record_bytes(&mut self, connection: usize, bytes: u64) {
        if let Some(slot) = self.connection_bytes.get_mut(connection) {
            *slot = slot.saturating_add(bytes);
            self.total_bytes = self.total_bytes.saturating_add(bytes);
        }
    }

    /// Advance the elapsed time and record a throughput sample.
    pub fn tick(&mut self, delta_secs: f32, current_mbps: f64) {
        self.elapsed_secs += delta_secs;
        if current_mbps >= 0.0 && current_mbps.is_finite() {
            self.samples.push(ThroughputSample {
                elapsed_secs: self.elapsed_secs,
                mbps: current_mbps,
            });
        }
        // Cap sample count to prevent unbounded growth.
        while self.samples.len() > MAX_GRAPH_POINTS {
            self.samples.remove(0);
        }
    }

    /// Fraction of the test completed (0.0 to 1.0).
    pub fn progress(&self) -> f32 {
        if self.duration_secs <= 0.0 {
            return 1.0;
        }
        (self.elapsed_secs / self.duration_secs).min(1.0)
    }

    /// Whether the test duration has elapsed.
    pub fn is_complete(&self) -> bool {
        self.elapsed_secs >= self.duration_secs
    }

    /// Average throughput in Mbps over all samples.
    pub fn avg_mbps(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|s| s.mbps).sum();
        sum / self.samples.len() as f64
    }

    /// Peak observed throughput in Mbps.
    pub fn peak_mbps(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.mbps)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    /// Current (most recent) throughput in Mbps.
    pub fn current_mbps(&self) -> f64 {
        self.samples.last().map_or(0.0, |s| s.mbps)
    }

    /// Reference to the time-series data for graphing.
    pub fn samples(&self) -> &[ThroughputSample] {
        &self.samples
    }

    /// Total bytes transferred across all connections.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Populate with simulated throughput data for development.
    pub fn simulate(&mut self, target_mbps: f64) {
        let steps = 60u32;
        let dt = self.duration_secs / steps as f32;
        let mut pseudo = 137u64;
        for i in 0..steps {
            pseudo = pseudo.wrapping_mul(6364136223846793005).wrapping_add(1);
            let frac = ((pseudo >> 33) as f64) / (u32::MAX as f64);
            // Ramp up over the first 20% of the test, then fluctuate.
            let ramp = ((i as f64 / (steps as f64 * 0.2)).min(1.0)).powi(2);
            let noise = (frac - 0.5) * 0.2 * target_mbps;
            let mbps = (target_mbps * ramp + noise).max(0.0);
            self.tick(dt, mbps);
            // Simulate some bytes transferred.
            let bytes_this_tick = (mbps * 1_000_000.0 / 8.0 * dt as f64) as u64;
            let conn_count = self.num_connections as usize;
            if conn_count > 0 {
                let per_conn = bytes_this_tick / conn_count as u64;
                for c in 0..conn_count {
                    self.record_bytes(c, per_conn);
                }
            }
        }
    }
}

// ============================================================================
// Speed Test History
// ============================================================================

/// Stores and aggregates historical speed test results.
#[derive(Clone, Debug)]
pub struct SpeedTestHistory {
    results: VecDeque<SpeedTestResult>,
    max_entries: usize,
}

impl SpeedTestHistory {
    /// Create a new history with the given capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            results: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    /// Add a result to the history, evicting the oldest if full.
    pub fn push(&mut self, result: SpeedTestResult) {
        if self.results.len() >= self.max_entries {
            self.results.pop_front();
        }
        self.results.push_back(result);
    }

    /// Number of stored results.
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// Get all results (newest last).
    pub fn results(&self) -> &VecDeque<SpeedTestResult> {
        &self.results
    }

    /// Get the most recent result, if any.
    pub fn latest(&self) -> Option<&SpeedTestResult> {
        self.results.back()
    }

    /// Average download speed across all results.
    pub fn avg_download(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.results.iter().map(|r| r.download_mbps).sum();
        sum / self.results.len() as f64
    }

    /// Average upload speed across all results.
    pub fn avg_upload(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.results.iter().map(|r| r.upload_mbps).sum();
        sum / self.results.len() as f64
    }

    /// Average latency across all results.
    pub fn avg_latency(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.results.iter().map(|r| r.latency_ms).sum();
        sum / self.results.len() as f64
    }

    /// Best (highest) download speed.
    pub fn best_download(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.download_mbps)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    /// Best (highest) upload speed.
    pub fn best_upload(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.upload_mbps)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    /// Best (lowest) latency.
    pub fn best_latency(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.latency_ms)
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    /// Worst (lowest) download speed.
    pub fn worst_download(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.download_mbps)
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    /// Worst (lowest) upload speed.
    pub fn worst_upload(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.upload_mbps)
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    /// Worst (highest) latency.
    pub fn worst_latency(&self) -> f64 {
        self.results
            .iter()
            .map(|r| r.latency_ms)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    /// Export entire history as a formatted text report.
    pub fn export_as_text(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("=== Speed Test History ===\n\n");

        if self.results.is_empty() {
            out.push_str("No results recorded.\n");
            return out;
        }

        out.push_str(&format!("Total tests: {}\n\n", self.results.len()));

        out.push_str("--- Summary ---\n");
        out.push_str(&format!(
            "Download (avg/best/worst): {:.1} / {:.1} / {:.1} Mbps\n",
            self.avg_download(),
            self.best_download(),
            self.worst_download(),
        ));
        out.push_str(&format!(
            "Upload   (avg/best/worst): {:.1} / {:.1} / {:.1} Mbps\n",
            self.avg_upload(),
            self.best_upload(),
            self.worst_upload(),
        ));
        out.push_str(&format!(
            "Latency  (avg/best/worst): {:.1} / {:.1} / {:.1} ms\n\n",
            self.avg_latency(),
            self.best_latency(),
            self.worst_latency(),
        ));

        out.push_str("--- Individual Results ---\n");
        for (i, r) in self.results.iter().enumerate() {
            out.push_str(&format!("\nTest #{}\n", i + 1));
            out.push_str(&r.to_text_report());
        }

        out
    }

    /// Clear all stored results.
    pub fn clear(&mut self) {
        self.results.clear();
    }
}

// ============================================================================
// Gauge Math — Arc Rendering Helpers
// ============================================================================

/// Map a speed value (Mbps) to an angle on the gauge arc.
/// Uses a logarithmic scale to handle the wide range (0-1000 Mbps).
/// Returns angle in degrees where 0 is the arc start.
fn speed_to_gauge_fraction(mbps: f64) -> f32 {
    if mbps <= 0.0 {
        return 0.0;
    }
    // Log scale: map [0, 1000] to [0.0, 1.0] using log10.
    // log10(1) = 0, log10(1000) = 3.
    let clamped = mbps.clamp(1.0, 1000.0);
    let log_val = clamped.log10(); // 0.0 .. 3.0
    (log_val / 3.0) as f32
}

/// Convert a gauge fraction (0.0-1.0) to an absolute angle in degrees.
fn gauge_fraction_to_angle(fraction: f32) -> f32 {
    GAUGE_ARC_START_ANGLE + fraction * GAUGE_ARC_SWEEP
}

/// Convert degrees to radians.
fn deg_to_rad(deg: f32) -> f32 {
    deg * PI / 180.0
}

/// Point on a circle given center, radius, and angle in degrees.
fn point_on_circle(cx: f32, cy: f32, radius: f32, angle_deg: f32) -> (f32, f32) {
    let rad = deg_to_rad(angle_deg);
    (cx + radius * rad.cos(), cy + radius * rad.sin())
}

/// Color for a given gauge fraction (gradient from green to yellow to red).
fn gauge_color_at(fraction: f32) -> Color {
    if fraction < 0.33 {
        GREEN
    } else if fraction < 0.66 {
        YELLOW
    } else {
        PEACH
    }
}

// ============================================================================
// Speed Test UI
// ============================================================================

/// Main application state for the speed test utility.
pub struct SpeedTestUI {
    /// Current phase.
    phase: SpeedTestPhase,
    /// Configuration for the test.
    config: SpeedTestConfig,
    /// Current live speed value being displayed on the gauge.
    current_speed_mbps: f64,
    /// Current latency being displayed.
    current_latency_ms: f64,
    /// Latency tester for the current/last run.
    latency_tester: LatencyTester,
    /// Download throughput tester for the current/last run.
    download_tester: ThroughputTester,
    /// Upload throughput tester for the current/last run.
    upload_tester: ThroughputTester,
    /// Historical results.
    history: SpeedTestHistory,
    /// Available test servers.
    servers: Vec<TestServer>,
    /// Index of the selected server.
    selected_server: usize,
    /// Whether the server dropdown is open.
    server_dropdown_open: bool,
    /// Graph data points (speed over time for the current test phase).
    graph_points: Vec<ThroughputSample>,
    /// Index of the history item being hovered.
    history_hover: Option<usize>,
    /// Scroll offset for history list.
    history_scroll: f32,
    /// Window dimensions.
    width: f32,
    height: f32,
    /// Whether the start button is hovered.
    start_button_hover: bool,
    /// Whether the export button is hovered.
    export_button_hover: bool,
}

impl SpeedTestUI {
    /// Create a new speed test UI with default configuration.
    pub fn new() -> Self {
        Self {
            phase: SpeedTestPhase::Idle,
            config: SpeedTestConfig::default(),
            current_speed_mbps: 0.0,
            current_latency_ms: 0.0,
            latency_tester: LatencyTester::new(20),
            download_tester: ThroughputTester::new(4, 10.0),
            upload_tester: ThroughputTester::new(4, 10.0),
            history: SpeedTestHistory::new(MAX_HISTORY),
            servers: default_servers(),
            selected_server: 0,
            server_dropdown_open: false,
            graph_points: Vec::with_capacity(MAX_GRAPH_POINTS),
            history_hover: None,
            history_scroll: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            start_button_hover: false,
            export_button_hover: false,
        }
    }

    /// Start a full speed test (latency -> download -> upload).
    pub fn start_test(&mut self) {
        if let Err(msg) = self.config.validate() {
            self.phase = SpeedTestPhase::Error(msg);
            return;
        }

        // Reset testers.
        self.latency_tester = LatencyTester::new(20);
        self.download_tester = ThroughputTester::new(
            self.config.num_connections,
            self.config.test_duration_secs as f32,
        );
        self.upload_tester = ThroughputTester::new(
            self.config.num_connections,
            self.config.test_duration_secs as f32,
        );
        self.graph_points.clear();
        self.current_speed_mbps = 0.0;
        self.current_latency_ms = 0.0;

        // Begin with latency phase.
        self.phase = SpeedTestPhase::Testing(TestKind::Latency);
    }

    /// Simulate a complete speed test run (for development without network).
    pub fn simulate_test(&mut self) {
        self.start_test();
        if matches!(self.phase, SpeedTestPhase::Error(_)) {
            return;
        }

        // Simulate latency phase.
        self.latency_tester.simulate(12.5, 3.0);
        self.current_latency_ms = self.latency_tester.avg_rtt().unwrap_or(0.0);

        // Simulate download phase.
        self.phase = SpeedTestPhase::Testing(TestKind::Download);
        self.download_tester.simulate(450.0);
        self.current_speed_mbps = self.download_tester.avg_mbps();
        self.graph_points = self.download_tester.samples().to_vec();

        // Simulate upload phase.
        self.phase = SpeedTestPhase::Testing(TestKind::Upload);
        self.upload_tester.simulate(120.0);

        // Complete.
        self.finalize_test();
    }

    /// Finalize the test and record results.
    fn finalize_test(&mut self) {
        let server_name = self
            .servers
            .get(self.selected_server)
            .map_or_else(|| "Unknown".into(), |s| s.name.clone());

        let result = SpeedTestResult {
            download_mbps: self.download_tester.avg_mbps(),
            upload_mbps: self.upload_tester.avg_mbps(),
            latency_ms: self.latency_tester.avg_rtt().unwrap_or(0.0),
            jitter_ms: self.latency_tester.jitter().unwrap_or(0.0),
            server_name,
            timestamp: 1747573200, // Placeholder; real impl uses system clock.
            packet_loss_pct: self.latency_tester.packet_loss_pct(),
        };

        self.current_speed_mbps = result.download_mbps;
        self.history.push(result);
        self.phase = SpeedTestPhase::Complete;
    }

    /// Get the current phase.
    pub fn phase(&self) -> &SpeedTestPhase {
        &self.phase
    }

    /// Get the test history.
    pub fn history(&self) -> &SpeedTestHistory {
        &self.history
    }

    /// Set the selected server by index.
    pub fn select_server(&mut self, index: usize) {
        if index < self.servers.len() {
            self.selected_server = index;
            if let Some(server) = self.servers.get(index) {
                self.config.server_url = server.url.clone();
            }
        }
        self.server_dropdown_open = false;
    }

    /// Handle a UI event (keyboard or mouse).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            Event::Mouse(mouse_event) => {
                let x = mouse_event.x;
                let y = mouse_event.y;
                match mouse_event.kind {
                    MouseEventKind::Press(MouseButton::Left) => self.handle_click(x, y),
                    MouseEventKind::Move => self.handle_mouse_move(x, y),
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter | Key::Space => {
                if self.phase.is_idle() || self.phase.is_complete() {
                    self.simulate_test();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Key::Escape => {
                if self.server_dropdown_open {
                    self.server_dropdown_open = false;
                    return EventResult::Consumed;
                }
                if self.phase.is_testing() {
                    self.phase = SpeedTestPhase::Idle;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Key::E if key.modifiers.ctrl => {
                // Ctrl+E: export history.
                // In a real app this would open a save dialog; for now it just
                // builds the text (could be copied to clipboard).
                let _export = self.history.export_as_text();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        // Start/Restart button.
        let btn_x = GAUGE_CENTER_X - BUTTON_WIDTH / 2.0;
        let btn_y = GAUGE_CENTER_Y + GAUGE_RADIUS + 20.0;
        if x >= btn_x
            && x <= btn_x + BUTTON_WIDTH
            && y >= btn_y
            && y <= btn_y + BUTTON_HEIGHT
        {
            if self.phase.is_idle() || self.phase.is_complete() || matches!(self.phase, SpeedTestPhase::Error(_)) {
                self.simulate_test();
                return EventResult::Consumed;
            }
        }

        // Export button.
        let export_x = GRAPH_X;
        let export_y = GRAPH_Y + GRAPH_HEIGHT + 10.0;
        if x >= export_x
            && x <= export_x + 100.0
            && y >= export_y
            && y <= export_y + 28.0
        {
            let _export = self.history.export_as_text();
            self.export_button_hover = true;
            return EventResult::Consumed;
        }

        // Server dropdown toggle.
        let dd_x = GAUGE_CENTER_X - 100.0;
        let dd_y = TITLE_BAR_HEIGHT + 8.0;
        if x >= dd_x && x <= dd_x + 200.0 && y >= dd_y && y <= dd_y + 28.0 {
            self.server_dropdown_open = !self.server_dropdown_open;
            return EventResult::Consumed;
        }

        // Server dropdown items.
        if self.server_dropdown_open {
            let item_y_start = dd_y + 30.0;
            for (i, _server) in self.servers.iter().enumerate() {
                let item_y = item_y_start + i as f32 * 28.0;
                if x >= dd_x && x <= dd_x + 200.0 && y >= item_y && y <= item_y + 28.0 {
                    self.select_server(i);
                    return EventResult::Consumed;
                }
            }
            // Click outside closes dropdown.
            self.server_dropdown_open = false;
            return EventResult::Consumed;
        }

        // History items.
        let hist_content_y = HISTORY_Y + 30.0;
        if x >= HISTORY_X
            && x <= HISTORY_X + HISTORY_WIDTH
            && y >= hist_content_y
            && y <= hist_content_y + HISTORY_HEIGHT - 30.0
        {
            let idx = ((y - hist_content_y + self.history_scroll) / HISTORY_ROW_HEIGHT) as usize;
            if idx < self.history.len() {
                // Selecting a history item could show details; for now just
                // highlight it.
                self.history_hover = Some(idx);
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        // Update button hover states.
        let btn_x = GAUGE_CENTER_X - BUTTON_WIDTH / 2.0;
        let btn_y = GAUGE_CENTER_Y + GAUGE_RADIUS + 20.0;
        self.start_button_hover = x >= btn_x
            && x <= btn_x + BUTTON_WIDTH
            && y >= btn_y
            && y <= btn_y + BUTTON_HEIGHT;

        let export_x = GRAPH_X;
        let export_y = GRAPH_Y + GRAPH_HEIGHT + 10.0;
        self.export_button_hover =
            x >= export_x && x <= export_x + 100.0 && y >= export_y && y <= export_y + 28.0;

        // History hover.
        let hist_content_y = HISTORY_Y + 30.0;
        if x >= HISTORY_X
            && x <= HISTORY_X + HISTORY_WIDTH
            && y >= hist_content_y
            && y <= hist_content_y + HISTORY_HEIGHT - 30.0
        {
            let idx = ((y - hist_content_y + self.history_scroll) / HISTORY_ROW_HEIGHT) as usize;
            self.history_hover = if idx < self.history.len() {
                Some(idx)
            } else {
                None
            };
        } else {
            self.history_hover = None;
        }

        EventResult::Ignored
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background.
        tree.fill_rect(0.0, 0.0, self.width, self.height, CRUST);

        self.render_title_bar(&mut tree);
        self.render_server_selector(&mut tree);
        self.render_gauge(&mut tree);
        self.render_speed_label(&mut tree);
        self.render_phase_indicators(&mut tree);
        self.render_start_button(&mut tree);
        self.render_result_summary(&mut tree);
        self.render_graph(&mut tree);
        self.render_history_panel(&mut tree);
        self.render_export_button(&mut tree);

        // Server dropdown overlay (rendered last so it draws on top).
        if self.server_dropdown_open {
            self.render_server_dropdown(&mut tree);
        }

        tree
    }

    fn render_title_bar(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: TITLE_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        tree.push(RenderCommand::Text {
            x: 16.0,
            y: 12.0,
            text: "Network Speed Test".into(),
            color: TEXT_COLOR,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: self.width - 200.0,
            y: 14.0,
            text: format!("Phase: {}", self.phase.label()),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_server_selector(&self, tree: &mut RenderTree) {
        let x = GAUGE_CENTER_X - 100.0;
        let y = TITLE_BAR_HEIGHT + 8.0;
        let w = 200.0_f32;
        let h = 28.0_f32;

        // Dropdown button background.
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        let server_name = self
            .servers
            .get(self.selected_server)
            .map_or("Select Server", |s| s.name.as_str());
        tree.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 7.0,
            text: server_name.into(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 30.0),
        });

        // Dropdown arrow.
        let arrow_text = if self.server_dropdown_open { "\u{25B2}" } else { "\u{25BC}" };
        tree.push(RenderCommand::Text {
            x: x + w - 20.0,
            y: y + 7.0,
            text: arrow_text.into(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_server_dropdown(&self, tree: &mut RenderTree) {
        let x = GAUGE_CENTER_X - 100.0;
        let base_y = TITLE_BAR_HEIGHT + 38.0;
        let w = 200.0_f32;
        let item_h = 28.0_f32;
        let total_h = self.servers.len() as f32 * item_h;

        // Dropdown background.
        tree.push(RenderCommand::FillRect {
            x,
            y: base_y,
            width: w,
            height: total_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y: base_y,
            width: w,
            height: total_h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        for (i, server) in self.servers.iter().enumerate() {
            let iy = base_y + i as f32 * item_h;
            if i == self.selected_server {
                tree.push(RenderCommand::FillRect {
                    x: x + 2.0,
                    y: iy + 1.0,
                    width: w - 4.0,
                    height: item_h - 2.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
            tree.push(RenderCommand::Text {
                x: x + 10.0,
                y: iy + 6.0,
                text: format!("{} ({})", server.name, server.location),
                color: if i == self.selected_server { BLUE } else { TEXT_COLOR },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 20.0),
            });
        }
    }

    fn render_gauge(&self, tree: &mut RenderTree) {
        let cx = GAUGE_CENTER_X;
        let cy = GAUGE_CENTER_Y;
        let outer_r = GAUGE_RADIUS;
        let inner_r = GAUGE_RADIUS - 16.0;

        // Draw arc background (dark track).
        for seg in 0..GAUGE_ARC_SEGMENTS {
            let frac0 = seg as f32 / GAUGE_ARC_SEGMENTS as f32;
            let frac1 = (seg + 1) as f32 / GAUGE_ARC_SEGMENTS as f32;
            let a0 = gauge_fraction_to_angle(frac0);
            let a1 = gauge_fraction_to_angle(frac1);
            let (ox0, oy0) = point_on_circle(cx, cy, outer_r, a0);
            let (ox1, oy1) = point_on_circle(cx, cy, outer_r, a1);
            tree.push(RenderCommand::Line {
                x1: ox0,
                y1: oy0,
                x2: ox1,
                y2: oy1,
                color: SURFACE0,
                width: 16.0,
            });
        }

        // Draw filled arc for current speed.
        let fill_frac = speed_to_gauge_fraction(self.current_speed_mbps);
        let fill_segments =
            ((fill_frac * GAUGE_ARC_SEGMENTS as f32).ceil() as usize).min(GAUGE_ARC_SEGMENTS);
        for seg in 0..fill_segments {
            let frac0 = seg as f32 / GAUGE_ARC_SEGMENTS as f32;
            let frac1 = ((seg + 1) as f32 / GAUGE_ARC_SEGMENTS as f32).min(fill_frac);
            let a0 = gauge_fraction_to_angle(frac0);
            let a1 = gauge_fraction_to_angle(frac1);
            let (ox0, oy0) = point_on_circle(cx, cy, outer_r - 8.0, a0);
            let (ox1, oy1) = point_on_circle(cx, cy, outer_r - 8.0, a1);
            let color = gauge_color_at(frac0);
            tree.push(RenderCommand::Line {
                x1: ox0,
                y1: oy0,
                x2: ox1,
                y2: oy1,
                color,
                width: 14.0,
            });
        }

        // Draw speed marker ticks and labels.
        for &marker in GAUGE_MARKERS {
            let frac = speed_to_gauge_fraction(marker as f64);
            let angle = gauge_fraction_to_angle(frac);
            let (tx0, ty0) = point_on_circle(cx, cy, outer_r + 2.0, angle);
            let (tx1, ty1) = point_on_circle(cx, cy, outer_r + 10.0, angle);
            tree.push(RenderCommand::Line {
                x1: tx0,
                y1: ty0,
                x2: tx1,
                y2: ty1,
                color: SUBTEXT0,
                width: 1.5,
            });
            let (lx, ly) = point_on_circle(cx, cy, outer_r + 20.0, angle);
            let label = if marker >= 1000.0 {
                format!("{}G", (marker / 1000.0) as u32)
            } else {
                format!("{}", marker as u32)
            };
            tree.push(RenderCommand::Text {
                x: lx - 10.0,
                y: ly - 5.0,
                text: label,
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Needle indicator.
        let needle_frac = speed_to_gauge_fraction(self.current_speed_mbps);
        let needle_angle = gauge_fraction_to_angle(needle_frac);
        let (nx, ny) = point_on_circle(cx, cy, inner_r - 4.0, needle_angle);
        tree.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: nx,
            y2: ny,
            color: TEXT_COLOR,
            width: 2.0,
        });

        // Center dot.
        tree.push(RenderCommand::FillRect {
            x: cx - 6.0,
            y: cy - 6.0,
            width: 12.0,
            height: 12.0,
            color: TEXT_COLOR,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    fn render_speed_label(&self, tree: &mut RenderTree) {
        let cx = GAUGE_CENTER_X;
        let cy = GAUGE_CENTER_Y;

        // Speed value text.
        let speed_text = format!("{:.1}", self.current_speed_mbps);
        tree.push(RenderCommand::Text {
            x: cx - 50.0,
            y: cy + 35.0,
            text: speed_text,
            color: TEXT_COLOR,
            font_size: 32.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });

        // Unit label.
        tree.push(RenderCommand::Text {
            x: cx - 20.0,
            y: cy + 70.0,
            text: "Mbps".into(),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Latency readout below gauge.
        if self.current_latency_ms > 0.0 {
            tree.push(RenderCommand::Text {
                x: cx - 40.0,
                y: cy + 90.0,
                text: format!("Latency: {:.1} ms", self.current_latency_ms),
                color: SUBTEXT1,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_phase_indicators(&self, tree: &mut RenderTree) {
        let phases = [TestKind::Latency, TestKind::Download, TestKind::Upload];
        let labels = ["Latency", "Download", "Upload"];
        let icons = [TEAL, BLUE, MAUVE];
        let total_width = 360.0_f32;
        let start_x = GAUGE_CENTER_X - total_width / 2.0;
        let y = PHASE_INDICATOR_Y;

        for (i, (kind, label)) in phases.iter().zip(labels.iter()).enumerate() {
            let x = start_x + i as f32 * 120.0;

            let (dot_color, text_color) = match &self.phase {
                SpeedTestPhase::Testing(active) if active == kind => (icons[i], TEXT_COLOR),
                SpeedTestPhase::Complete => (GREEN, SUBTEXT0),
                _ => {
                    // Check if this phase has already been completed in the
                    // current test sequence.
                    let done = match kind {
                        TestKind::Latency => {
                            self.latency_tester.is_complete()
                                && !matches!(self.phase, SpeedTestPhase::Idle)
                        }
                        TestKind::Download => {
                            self.download_tester.is_complete()
                                && !matches!(self.phase, SpeedTestPhase::Idle)
                        }
                        TestKind::Upload => {
                            self.upload_tester.is_complete()
                                && !matches!(self.phase, SpeedTestPhase::Idle)
                        }
                    };
                    if done {
                        (GREEN, SUBTEXT0)
                    } else {
                        (SURFACE2, SURFACE2)
                    }
                }
            };

            // Phase dot.
            tree.push(RenderCommand::FillRect {
                x,
                y,
                width: 10.0,
                height: 10.0,
                color: dot_color,
                corner_radii: CornerRadii::all(5.0),
            });

            // Phase label.
            tree.push(RenderCommand::Text {
                x: x + 16.0,
                y: y - 1.0,
                text: (*label).into(),
                color: text_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Arrow between phases.
            if i < 2 {
                tree.push(RenderCommand::Text {
                    x: x + 90.0,
                    y: y - 1.0,
                    text: "\u{2192}".into(),
                    color: SURFACE2,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    fn render_start_button(&self, tree: &mut RenderTree) {
        let x = GAUGE_CENTER_X - BUTTON_WIDTH / 2.0;
        let y = GAUGE_CENTER_Y + GAUGE_RADIUS + 20.0;

        let (bg, label) = if self.phase.is_testing() {
            (SURFACE1, "Testing...")
        } else if self.start_button_hover {
            (SAPPHIRE, "Start Test")
        } else if self.phase.is_complete() {
            (BLUE, "Re-Test")
        } else {
            (BLUE, "Start Test")
        };

        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(6.0),
        });
        tree.push(RenderCommand::Text {
            x: x + 30.0,
            y: y + 10.0,
            text: label.into(),
            color: if bg == SAPPHIRE { CRUST } else { TEXT_COLOR },
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_result_summary(&self, tree: &mut RenderTree) {
        if !self.phase.is_complete() && !self.phase.is_testing() {
            if let SpeedTestPhase::Error(ref msg) = self.phase {
                tree.push(RenderCommand::Text {
                    x: GAUGE_CENTER_X - 120.0,
                    y: GAUGE_CENTER_Y + GAUGE_RADIUS + 65.0,
                    text: format!("Error: {msg}"),
                    color: RED,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(240.0),
                });
            }
            return;
        }

        let y_base = GAUGE_CENTER_Y + GAUGE_RADIUS + 65.0;
        let col_width = 140.0_f32;

        // Download.
        let dl_x = GAUGE_CENTER_X - col_width * 1.5;
        tree.push(RenderCommand::Text {
            x: dl_x,
            y: y_base,
            text: "Download".into(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: dl_x,
            y: y_base + 14.0,
            text: format!("{:.1} Mbps", self.download_tester.avg_mbps()),
            color: BLUE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Upload.
        let ul_x = GAUGE_CENTER_X - col_width * 0.5;
        tree.push(RenderCommand::Text {
            x: ul_x,
            y: y_base,
            text: "Upload".into(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: ul_x,
            y: y_base + 14.0,
            text: format!("{:.1} Mbps", self.upload_tester.avg_mbps()),
            color: MAUVE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Latency / Jitter.
        let lat_x = GAUGE_CENTER_X + col_width * 0.5;
        tree.push(RenderCommand::Text {
            x: lat_x,
            y: y_base,
            text: "Latency / Jitter".into(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: lat_x,
            y: y_base + 14.0,
            text: format!(
                "{:.1} / {:.1} ms",
                self.latency_tester.avg_rtt().unwrap_or(0.0),
                self.latency_tester.jitter().unwrap_or(0.0),
            ),
            color: TEAL,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_graph(&self, tree: &mut RenderTree) {
        // Graph panel background.
        tree.push(RenderCommand::FillRect {
            x: GRAPH_X,
            y: GRAPH_Y,
            width: GRAPH_WIDTH,
            height: GRAPH_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: GRAPH_X,
            y: GRAPH_Y,
            width: GRAPH_WIDTH,
            height: GRAPH_HEIGHT,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Graph title.
        tree.push(RenderCommand::Text {
            x: GRAPH_X + 12.0,
            y: GRAPH_Y + 8.0,
            text: "Speed Over Time".into(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let plot_x = GRAPH_X + 50.0;
        let plot_y = GRAPH_Y + 30.0;
        let plot_w = GRAPH_WIDTH - 60.0;
        let plot_h = GRAPH_HEIGHT - 50.0;

        // Grid lines and Y-axis labels.
        let y_steps = 4u32;
        let samples = self.active_graph_samples();
        let max_speed = samples
            .iter()
            .map(|s| s.mbps)
            .reduce(f64::max)
            .unwrap_or(100.0)
            .max(10.0);

        for i in 0..=y_steps {
            let frac = i as f32 / y_steps as f32;
            let gy = plot_y + plot_h - frac * plot_h;
            // Grid line.
            tree.push(RenderCommand::Line {
                x1: plot_x,
                y1: gy,
                x2: plot_x + plot_w,
                y2: gy,
                color: SURFACE0,
                width: 0.5,
            });
            // Y label.
            let val = max_speed * frac as f64;
            tree.push(RenderCommand::Text {
                x: GRAPH_X + 4.0,
                y: gy - 5.0,
                text: format!("{:.0}", val),
                color: SURFACE2,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(42.0),
            });
        }

        // Plot data.
        if samples.len() >= 2 {
            let max_time = samples
                .last()
                .map_or(1.0, |s| s.elapsed_secs.max(0.1));

            for pair in samples.windows(2) {
                let s0 = &pair[0];
                let s1 = &pair[1];
                let x0 = plot_x + (s0.elapsed_secs / max_time) * plot_w;
                let y0 = plot_y + plot_h - (s0.mbps / max_speed) as f32 * plot_h;
                let x1 = plot_x + (s1.elapsed_secs / max_time) * plot_w;
                let y1 = plot_y + plot_h - (s1.mbps / max_speed) as f32 * plot_h;
                tree.push(RenderCommand::Line {
                    x1: x0,
                    y1: y0,
                    x2: x1,
                    y2: y1,
                    color: BLUE,
                    width: 2.0,
                });
            }

            // X-axis time labels.
            let label_count = 5u32;
            for i in 0..=label_count {
                let frac = i as f32 / label_count as f32;
                let t = max_time * frac;
                let lx = plot_x + frac * plot_w;
                tree.push(RenderCommand::Text {
                    x: lx - 8.0,
                    y: plot_y + plot_h + 4.0,
                    text: format!("{:.0}s", t),
                    color: SURFACE2,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        } else {
            // No data placeholder.
            tree.push(RenderCommand::Text {
                x: plot_x + plot_w / 2.0 - 40.0,
                y: plot_y + plot_h / 2.0 - 5.0,
                text: "No data yet".into(),
                color: SURFACE2,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_history_panel(&self, tree: &mut RenderTree) {
        // Panel background.
        tree.push(RenderCommand::FillRect {
            x: HISTORY_X,
            y: HISTORY_Y,
            width: HISTORY_WIDTH,
            height: HISTORY_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: HISTORY_X,
            y: HISTORY_Y,
            width: HISTORY_WIDTH,
            height: HISTORY_HEIGHT,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Header.
        tree.push(RenderCommand::Text {
            x: HISTORY_X + 12.0,
            y: HISTORY_Y + 8.0,
            text: format!("History ({})", self.history.len()),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let content_y = HISTORY_Y + 30.0;
        let content_h = HISTORY_HEIGHT - 30.0;

        // Clip to history area.
        tree.push(RenderCommand::PushClip {
            x: HISTORY_X,
            y: content_y,
            width: HISTORY_WIDTH,
            height: content_h,
        });

        if self.history.is_empty() {
            tree.push(RenderCommand::Text {
                x: HISTORY_X + 12.0,
                y: content_y + 10.0,
                text: "Run a test to see results".into(),
                color: SURFACE2,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(HISTORY_WIDTH - 24.0),
            });
        } else {
            // Show results newest-first.
            let results: Vec<&SpeedTestResult> = self.history.results().iter().rev().collect();
            for (i, result) in results.iter().enumerate() {
                let ry = content_y + i as f32 * HISTORY_ROW_HEIGHT - self.history_scroll;
                if ry + HISTORY_ROW_HEIGHT < content_y || ry > content_y + content_h {
                    continue;
                }

                // Hover highlight.
                let rev_idx = self.history.len().saturating_sub(1).saturating_sub(i);
                if self.history_hover == Some(rev_idx) {
                    tree.push(RenderCommand::FillRect {
                        x: HISTORY_X + 4.0,
                        y: ry,
                        width: HISTORY_WIDTH - 8.0,
                        height: HISTORY_ROW_HEIGHT,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(3.0),
                    });
                }

                tree.push(RenderCommand::Text {
                    x: HISTORY_X + 12.0,
                    y: ry + 6.0,
                    text: result.summary_line(),
                    color: SUBTEXT1,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(HISTORY_WIDTH - 24.0),
                });
            }
        }

        tree.push(RenderCommand::PopClip);

        // History stats at the bottom.
        if !self.history.is_empty() {
            let stats_y = HISTORY_Y + HISTORY_HEIGHT - 22.0;
            tree.push(RenderCommand::FillRect {
                x: HISTORY_X,
                y: stats_y - 2.0,
                width: HISTORY_WIDTH,
                height: 24.0,
                color: MANTLE,
                corner_radii: CornerRadii {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_right: 8.0,
                    bottom_left: 8.0,
                },
            });
            tree.push(RenderCommand::Text {
                x: HISTORY_X + 8.0,
                y: stats_y + 2.0,
                text: format!(
                    "Avg: {:.0}/{:.0} Mbps, {:.0}ms",
                    self.history.avg_download(),
                    self.history.avg_upload(),
                    self.history.avg_latency(),
                ),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(HISTORY_WIDTH - 16.0),
            });
        }
    }

    fn render_export_button(&self, tree: &mut RenderTree) {
        let x = GRAPH_X;
        let y = GRAPH_Y + GRAPH_HEIGHT + 10.0;

        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: 100.0,
            height: 28.0,
            color: if self.export_button_hover { SURFACE1 } else { SURFACE0 },
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: x + 14.0,
            y: y + 7.0,
            text: "Export".into(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Return the graph samples for the currently active or last completed phase.
    fn active_graph_samples(&self) -> &[ThroughputSample] {
        match &self.phase {
            SpeedTestPhase::Testing(TestKind::Download) | SpeedTestPhase::Complete => {
                self.download_tester.samples()
            }
            SpeedTestPhase::Testing(TestKind::Upload) => self.upload_tester.samples(),
            _ => {
                if !self.graph_points.is_empty() {
                    &self.graph_points
                } else {
                    self.download_tester.samples()
                }
            }
        }
    }
}

impl Default for SpeedTestUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Entry point (placeholder for OurOS)
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- SpeedTestPhase tests ---

    #[test]
    fn phase_idle_is_idle() {
        assert!(SpeedTestPhase::Idle.is_idle());
    }

    #[test]
    fn phase_testing_is_testing() {
        assert!(SpeedTestPhase::Testing(TestKind::Download).is_testing());
    }

    #[test]
    fn phase_complete_is_complete() {
        assert!(SpeedTestPhase::Complete.is_complete());
    }

    #[test]
    fn phase_idle_is_not_testing() {
        assert!(!SpeedTestPhase::Idle.is_testing());
    }

    #[test]
    fn phase_label_idle() {
        assert_eq!(SpeedTestPhase::Idle.label(), "Ready");
    }

    #[test]
    fn phase_label_testing_download() {
        assert_eq!(SpeedTestPhase::Testing(TestKind::Download).label(), "Download");
    }

    #[test]
    fn phase_label_error() {
        assert_eq!(SpeedTestPhase::Error("fail".into()).label(), "Error");
    }

    // --- SpeedTestConfig tests ---

    #[test]
    fn config_default_valid() {
        assert!(SpeedTestConfig::default().validate().is_ok());
    }

    #[test]
    fn config_empty_url_invalid() {
        let mut cfg = SpeedTestConfig::default();
        cfg.server_url = String::new();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_zero_duration_invalid() {
        let mut cfg = SpeedTestConfig::default();
        cfg.test_duration_secs = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_excessive_duration_invalid() {
        let mut cfg = SpeedTestConfig::default();
        cfg.test_duration_secs = 200;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_zero_connections_invalid() {
        let mut cfg = SpeedTestConfig::default();
        cfg.num_connections = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_excessive_connections_invalid() {
        let mut cfg = SpeedTestConfig::default();
        cfg.num_connections = 64;
        assert!(cfg.validate().is_err());
    }

    // --- LatencyTester tests ---

    #[test]
    fn latency_tester_empty() {
        let t = LatencyTester::new(10);
        assert_eq!(t.sample_count(), 0);
        assert!(t.min_rtt().is_none());
        assert!(t.max_rtt().is_none());
        assert!(t.avg_rtt().is_none());
        assert!(t.jitter().is_none());
    }

    #[test]
    fn latency_tester_single_sample() {
        let mut t = LatencyTester::new(5);
        t.record_sample(10.0);
        assert_eq!(t.sample_count(), 1);
        assert!((t.avg_rtt().unwrap() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn latency_tester_min_max() {
        let mut t = LatencyTester::new(10);
        t.record_sample(5.0);
        t.record_sample(15.0);
        t.record_sample(10.0);
        assert!((t.min_rtt().unwrap() - 5.0).abs() < f64::EPSILON);
        assert!((t.max_rtt().unwrap() - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn latency_tester_avg() {
        let mut t = LatencyTester::new(10);
        t.record_sample(10.0);
        t.record_sample(20.0);
        t.record_sample(30.0);
        assert!((t.avg_rtt().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn latency_tester_jitter() {
        let mut t = LatencyTester::new(10);
        t.record_sample(10.0);
        t.record_sample(20.0);
        t.record_sample(10.0);
        // jitter = avg(|20-10|, |10-20|) = avg(10, 10) = 10
        assert!((t.jitter().unwrap() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn latency_tester_packet_loss() {
        let mut t = LatencyTester::new(10);
        t.record_sample(5.0);
        t.record_loss();
        t.record_sample(10.0);
        t.record_loss();
        // 2 lost out of 4 sent = 50%
        assert!((t.packet_loss_pct() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn latency_tester_progress() {
        let mut t = LatencyTester::new(4);
        assert!((t.progress() - 0.0).abs() < f32::EPSILON);
        t.record_sample(5.0);
        assert!((t.progress() - 0.25).abs() < f32::EPSILON);
        t.record_sample(5.0);
        t.record_sample(5.0);
        t.record_sample(5.0);
        assert!((t.progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn latency_tester_is_complete() {
        let mut t = LatencyTester::new(2);
        assert!(!t.is_complete());
        t.record_sample(1.0);
        assert!(!t.is_complete());
        t.record_sample(2.0);
        assert!(t.is_complete());
    }

    #[test]
    fn latency_tester_simulate() {
        let mut t = LatencyTester::new(50);
        t.simulate(10.0, 2.0);
        assert!(t.is_complete());
        assert!(t.sample_count() > 0);
        assert!(t.avg_rtt().is_some());
    }

    #[test]
    fn latency_tester_negative_rtt_ignored() {
        let mut t = LatencyTester::new(5);
        t.record_sample(-1.0);
        // Negative value should not be recorded as a sample.
        assert_eq!(t.sample_count(), 0);
    }

    // --- ThroughputTester tests ---

    #[test]
    fn throughput_tester_initial_state() {
        let t = ThroughputTester::new(4, 10.0);
        assert_eq!(t.total_bytes(), 0);
        assert!((t.avg_mbps() - 0.0).abs() < f64::EPSILON);
        assert!((t.peak_mbps() - 0.0).abs() < f64::EPSILON);
        assert!((t.current_mbps() - 0.0).abs() < f64::EPSILON);
        assert!(!t.is_complete());
    }

    #[test]
    fn throughput_tester_record_bytes() {
        let mut t = ThroughputTester::new(2, 10.0);
        t.record_bytes(0, 1000);
        t.record_bytes(1, 2000);
        assert_eq!(t.total_bytes(), 3000);
    }

    #[test]
    fn throughput_tester_out_of_bounds_connection() {
        let mut t = ThroughputTester::new(2, 10.0);
        // Connection index 5 is out of bounds; should be silently ignored.
        t.record_bytes(5, 1000);
        assert_eq!(t.total_bytes(), 0);
    }

    #[test]
    fn throughput_tester_tick_and_avg() {
        let mut t = ThroughputTester::new(1, 10.0);
        t.tick(1.0, 100.0);
        t.tick(1.0, 200.0);
        assert!((t.avg_mbps() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn throughput_tester_peak() {
        let mut t = ThroughputTester::new(1, 10.0);
        t.tick(1.0, 50.0);
        t.tick(1.0, 300.0);
        t.tick(1.0, 100.0);
        assert!((t.peak_mbps() - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn throughput_tester_progress_and_complete() {
        let mut t = ThroughputTester::new(1, 4.0);
        t.tick(2.0, 100.0);
        assert!((t.progress() - 0.5).abs() < f32::EPSILON);
        assert!(!t.is_complete());
        t.tick(2.0, 100.0);
        assert!((t.progress() - 1.0).abs() < f32::EPSILON);
        assert!(t.is_complete());
    }

    #[test]
    fn throughput_tester_simulate() {
        let mut t = ThroughputTester::new(4, 10.0);
        t.simulate(500.0);
        assert!(t.is_complete());
        assert!(t.avg_mbps() > 0.0);
        assert!(t.total_bytes() > 0);
    }

    #[test]
    fn throughput_tester_samples_capped() {
        let mut t = ThroughputTester::new(1, 1000.0);
        for i in 0..200 {
            t.tick(1.0, i as f64);
        }
        assert!(t.samples().len() <= MAX_GRAPH_POINTS);
    }

    // --- SpeedTestHistory tests ---

    #[test]
    fn history_empty() {
        let h = SpeedTestHistory::new(5);
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(h.latest().is_none());
    }

    #[test]
    fn history_push_and_len() {
        let mut h = SpeedTestHistory::new(5);
        h.push(make_result(100.0, 50.0, 10.0));
        assert_eq!(h.len(), 1);
        assert!(h.latest().is_some());
    }

    #[test]
    fn history_eviction() {
        let mut h = SpeedTestHistory::new(3);
        h.push(make_result(100.0, 50.0, 10.0));
        h.push(make_result(200.0, 60.0, 8.0));
        h.push(make_result(300.0, 70.0, 6.0));
        h.push(make_result(400.0, 80.0, 5.0));
        assert_eq!(h.len(), 3);
        // The first result (100 Mbps) should have been evicted.
        assert!((h.results().front().unwrap().download_mbps - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_avg_download() {
        let mut h = SpeedTestHistory::new(10);
        h.push(make_result(100.0, 50.0, 10.0));
        h.push(make_result(200.0, 70.0, 8.0));
        assert!((h.avg_download() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_best_worst_download() {
        let mut h = SpeedTestHistory::new(10);
        h.push(make_result(100.0, 50.0, 10.0));
        h.push(make_result(500.0, 70.0, 8.0));
        h.push(make_result(200.0, 60.0, 12.0));
        assert!((h.best_download() - 500.0).abs() < f64::EPSILON);
        assert!((h.worst_download() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_best_worst_latency() {
        let mut h = SpeedTestHistory::new(10);
        h.push(make_result(100.0, 50.0, 15.0));
        h.push(make_result(200.0, 60.0, 5.0));
        h.push(make_result(150.0, 55.0, 10.0));
        // Best latency = lowest.
        assert!((h.best_latency() - 5.0).abs() < f64::EPSILON);
        // Worst latency = highest.
        assert!((h.worst_latency() - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_export_empty() {
        let h = SpeedTestHistory::new(5);
        let text = h.export_as_text();
        assert!(text.contains("No results recorded"));
    }

    #[test]
    fn history_export_with_results() {
        let mut h = SpeedTestHistory::new(5);
        h.push(make_result(200.0, 80.0, 10.0));
        let text = h.export_as_text();
        assert!(text.contains("Total tests: 1"));
        assert!(text.contains("Download"));
    }

    #[test]
    fn history_clear() {
        let mut h = SpeedTestHistory::new(5);
        h.push(make_result(100.0, 50.0, 10.0));
        h.clear();
        assert!(h.is_empty());
    }

    // --- SpeedTestResult tests ---

    #[test]
    fn result_summary_line() {
        let r = make_result(100.5, 45.3, 12.7);
        let s = r.summary_line();
        assert!(s.contains("100.5"));
        assert!(s.contains("45.3"));
        assert!(s.contains("12.7"));
    }

    #[test]
    fn result_text_report() {
        let r = make_result(200.0, 80.0, 10.0);
        let report = r.to_text_report();
        assert!(report.contains("200.00 Mbps"));
        assert!(report.contains("80.00 Mbps"));
        assert!(report.contains("Server:"));
    }

    // --- Gauge math tests ---

    #[test]
    fn gauge_fraction_zero_speed() {
        assert!((speed_to_gauge_fraction(0.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn gauge_fraction_max_speed() {
        let f = speed_to_gauge_fraction(1000.0);
        assert!((f - 1.0).abs() < 0.01);
    }

    #[test]
    fn gauge_fraction_mid_speed() {
        // 10 Mbps: log10(10) = 1.0, fraction = 1/3
        let f = speed_to_gauge_fraction(10.0);
        assert!((f - 0.333).abs() < 0.01);
    }

    #[test]
    fn gauge_fraction_monotonic() {
        let f1 = speed_to_gauge_fraction(10.0);
        let f2 = speed_to_gauge_fraction(100.0);
        let f3 = speed_to_gauge_fraction(500.0);
        assert!(f1 < f2);
        assert!(f2 < f3);
    }

    #[test]
    fn point_on_circle_right() {
        let (x, y) = point_on_circle(0.0, 0.0, 10.0, 0.0);
        assert!((x - 10.0).abs() < 0.01);
        assert!(y.abs() < 0.01);
    }

    #[test]
    fn deg_to_rad_conversion() {
        assert!((deg_to_rad(180.0) - PI).abs() < 0.001);
        assert!((deg_to_rad(90.0) - PI / 2.0).abs() < 0.001);
    }

    // --- SpeedTestUI tests ---

    #[test]
    fn ui_new_starts_idle() {
        let ui = SpeedTestUI::new();
        assert!(ui.phase().is_idle());
    }

    #[test]
    fn ui_default_same_as_new() {
        let ui = SpeedTestUI::default();
        assert!(ui.phase().is_idle());
    }

    #[test]
    fn ui_simulate_test_completes() {
        let mut ui = SpeedTestUI::new();
        ui.simulate_test();
        assert!(ui.phase().is_complete());
        assert_eq!(ui.history().len(), 1);
    }

    #[test]
    fn ui_simulate_test_has_results() {
        let mut ui = SpeedTestUI::new();
        ui.simulate_test();
        let result = ui.history().latest().unwrap();
        assert!(result.download_mbps > 0.0);
        assert!(result.upload_mbps > 0.0);
        assert!(result.latency_ms > 0.0);
    }

    #[test]
    fn ui_select_server() {
        let mut ui = SpeedTestUI::new();
        ui.select_server(2);
        assert_eq!(ui.selected_server, 2);
    }

    #[test]
    fn ui_select_server_out_of_range() {
        let mut ui = SpeedTestUI::new();
        ui.select_server(999);
        // Should not change.
        assert_eq!(ui.selected_server, 0);
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = SpeedTestUI::new();
        let tree = ui.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn ui_render_after_test_has_more_commands() {
        let idle_ui = SpeedTestUI::new();
        let idle_cmds = idle_ui.render().len();

        let mut tested_ui = SpeedTestUI::new();
        tested_ui.simulate_test();
        let tested_cmds = tested_ui.render().len();

        // After a test, we should have more render commands due to
        // result data, graph data, and history.
        assert!(tested_cmds > idle_cmds);
    }

    #[test]
    fn ui_invalid_config_shows_error() {
        let mut ui = SpeedTestUI::new();
        ui.config.server_url = String::new();
        ui.start_test();
        assert!(matches!(ui.phase(), SpeedTestPhase::Error(_)));
    }

    // --- Server list tests ---

    #[test]
    fn default_servers_not_empty() {
        let servers = default_servers();
        assert!(!servers.is_empty());
    }

    #[test]
    fn default_servers_have_names() {
        for server in default_servers() {
            assert!(!server.name.is_empty());
            assert!(!server.url.is_empty());
        }
    }

    // --- Test helper ---

    fn make_result(dl: f64, ul: f64, lat: f64) -> SpeedTestResult {
        SpeedTestResult {
            download_mbps: dl,
            upload_mbps: ul,
            latency_ms: lat,
            jitter_ms: 1.5,
            server_name: "TestServer".into(),
            timestamp: 1000000,
            packet_loss_pct: 0.0,
        }
    }
}
