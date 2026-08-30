//! Slate OS Network Speed Test
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
//! performed through Slate OS syscalls; simulated with representative
//! data for initial development.

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::num::NonZeroU64;
use std::process::ExitCode;
use std::time::Duration;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::fold;
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

/// Seed used when the system has no entropy to offer.
///
/// A simulated speed test is novelty randomness, not a secret, so losing
/// entropy must not stop the app from running. The constant is per-crate
/// ("SPEEDTST") so that two programs falling back on the same boot do not
/// then agree with each other.
const FALLBACK_SEED: u64 = 0x5350_4545_4454_5354;

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
//
// Sizes, not positions. Every *position* is computed in `Layout` from the
// window's actual size; the constants here are the things that do not depend
// on it -- how tall a row is, how wide a button wants to be.
// ============================================================================

/// The size the window asks for when it opens.
const WINDOW_WIDTH: f32 = 900.0;
/// The height the window asks for when it opens.
const WINDOW_HEIGHT: f32 = 720.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
/// Space between the window's edge and the panels along the bottom.
const MARGIN: f32 = 20.0;
/// Space between the graph and the history panel.
const GAP: f32 = 20.0;
/// The largest the speedometer is ever drawn, however big the window is.
///
/// A dial that grew without limit would be a 400 px needle on a maximised
/// screen with the numbers it exists to show stranded in the middle of it.
const GAUGE_MAX_RADIUS: f32 = 150.0;
/// Ring outside the arc for the tick marks and their labels.
const GAUGE_LABEL_RING: f32 = 24.0;
/// The arc is 270 degrees with its opening at the bottom, so the dial's height
/// is not `2 * radius`: it runs from the top of the circle down to the ends of
/// the arc at `sin(45 degrees)`, plus the label ring at both extremes.
const GAUGE_HEIGHT_PER_RADIUS: f32 = 1.75;
const GAUGE_ARC_SEGMENTS: usize = 60;
const GAUGE_ARC_START_ANGLE: f32 = 135.0;
const GAUGE_ARC_SWEEP: f32 = 270.0;
const HISTORY_ROW_HEIGHT: f32 = 26.0;
/// Height of the history panel's title bar, above the list.
const HISTORY_HEADER_HEIGHT: f32 = 30.0;
/// Height of the opaque avg/best/worst strip at the foot of the history panel.
///
/// It is painted *over* the bottom of the list area, so it is part of the
/// list's geometry whether the list likes it or not: a row underneath it is
/// drawn nowhere, and so must not be reachable or scrollable-to either. Both
/// were literals before, which is how the hit test came to accept clicks on a
/// strip of panel where nothing is visible.
///
/// Reserved whether or not there is a result to put in it, so that the first
/// completed test does not shorten the list under a pointer already in it.
const HISTORY_STATS_HEIGHT: f32 = 24.0;
const BUTTON_WIDTH: f32 = 140.0;
const BUTTON_HEIGHT: f32 = 36.0;
const EXPORT_WIDTH: f32 = 100.0;
const EXPORT_HEIGHT: f32 = 28.0;
const SERVER_WIDTH: f32 = 200.0;
const SERVER_HEIGHT: f32 = 28.0;
/// Height of the download/upload/latency strip under the Start button.
const SUMMARY_HEIGHT: f32 = 32.0;
/// Height of the Latency -> Download -> Upload strip.
const PHASE_ROW_HEIGHT: f32 = 20.0;
/// Width the phase strip wants, when the window is wide enough to give it.
const PHASE_ROW_WIDTH: f32 = 360.0;
/// Vertical breathing room between the stacked pieces of the upper half.
const STACK_GAP: f32 = 8.0;
const MAX_HISTORY: usize = 20;
const MAX_GRAPH_POINTS: usize = 120;

/// Speed markers on the gauge (in Mbps).
const GAUGE_MARKERS: &[f32] = &[0.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0];

// ============================================================================
// Controls
// ============================================================================

/// Everything in the window that answers to a pointer.
///
/// The hit boxes are recorded by the renderer as it draws, so there is exactly
/// one description of where a control is. `handle_click` used to re-derive
/// each rectangle from the layout constants a second time; the two agreed only
/// because they were written from the same literals on the same afternoon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The server picker's button.
    ServerButton,
    /// One row of the open server list.
    ServerItem(usize),
    /// Start / Re-Test.
    Start,
    /// Write the history out as text.
    Export,
    /// One result in the history list.
    HistoryRow(usize),
    /// The history panel itself, under the rows: the wheel scrolls anywhere
    /// over the panel, including its header and its stats strip, but a *click*
    /// there selects nothing because no row is drawn there.
    HistoryPanel,
    /// Everything an open server list covers. A click here shuts the list
    /// instead of reaching the control underneath it.
    DropdownScrim,
}

/// This program's frame type: a render tree that also remembers where each
/// [`Target`] was drawn.
type Frame = guitk::frame::Frame<Target>;

/// Where every control is, for one window size.
///
/// Constructed fresh per frame and per hit test rather than cached, so there is
/// no chance of answering a click from a layout the user is not looking at.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// The window's width, after the guard against a degenerate size.
    width: f32,
    /// The window's height, after the same guard.
    height: f32,
    /// Where the title bar's own text ends, so the phase readout cannot be
    /// right-aligned on top of it.
    title_text_right: f32,
    /// The server picker's button.
    server: Rect,
    /// Centre of the speedometer.
    gauge_centre: (f32, f32),
    /// Radius of the speedometer's arc, excluding the label ring.
    gauge_radius: f32,
    /// The strip the three phase indicators are spread across.
    phase_row: Rect,
    /// The Start / Re-Test button.
    start: Rect,
    /// The download/upload/latency strip.
    summary: Rect,
    /// The speed-over-time panel.
    graph: Rect,
    /// The Export button, tucked under the graph.
    export: Rect,
    /// The history panel, whole.
    history: Rect,
    /// The part of the history panel the list is actually visible in: below
    /// the header, above the stats strip.
    history_list: Rect,
}

impl Layout {
    /// Lay the window out at `width` by `height`.
    ///
    /// Nothing is clamped up to a minimum size. A control positioned off the
    /// edge of a too-small window would still be recorded as a hit box and so
    /// would still be clickable while being invisible, which is worse than a
    /// cramped window; so every piece shrinks instead, down to nothing.
    fn new(width: f32, height: f32) -> Self {
        // A compositor mid-resize can report a zero-width window, and a NaN
        // would poison every comparison downstream -- once `history_scroll`
        // is NaN the list never moves again.
        let width = if width.is_finite() {
            width.max(1.0)
        } else {
            WINDOW_WIDTH
        };
        let height = if height.is_finite() {
            height.max(1.0)
        } else {
            WINDOW_HEIGHT
        };

        let server = Rect::new(
            ((width - SERVER_WIDTH) / 2.0).max(0.0),
            TITLE_BAR_HEIGHT + 8.0,
            SERVER_WIDTH.min(width),
            SERVER_HEIGHT,
        );

        // --- the band along the bottom: graph, export, history -------------
        let band_h = (height * 0.36)
            .clamp(0.0, 280.0)
            .min((height - server.bottom() - 100.0).max(0.0));
        let band_top = height - MARGIN - band_h;

        let inner_w = (width - 2.0 * MARGIN - GAP).max(0.0);
        let history_w = (inner_w * 0.36).min(320.0);
        let graph_w = inner_w - history_w;

        let graph_h = (band_h - EXPORT_HEIGHT - STACK_GAP).max(0.0);
        let graph = Rect::new(MARGIN, band_top, graph_w, graph_h);
        let export = Rect::new(
            MARGIN,
            band_top + graph_h + STACK_GAP,
            EXPORT_WIDTH.min(graph_w),
            EXPORT_HEIGHT.min(band_h),
        );
        let history = Rect::new(MARGIN + graph_w + GAP, band_top, history_w, band_h);
        let history_list = Rect::new(
            history.x,
            history.y + HISTORY_HEADER_HEIGHT,
            history.w,
            (history.h - HISTORY_HEADER_HEIGHT - HISTORY_STATS_HEIGHT).max(0.0),
        );

        // --- the upper half, stacked upwards from the band -----------------
        let upper_top = server.bottom() + STACK_GAP;

        // Each row of the stack is placed by its *bottom* edge and then
        // trimmed at `upper_top`, so a window too short to hold the stack
        // squeezes its rows away instead of pushing them up over the server
        // picker. That matters more than it looks: the picker is drawn first,
        // so an overlapping button would win the hit test and the picker would
        // be visible but unclickable -- the same "there but not there" fault
        // the no-clamping rule above exists to prevent.
        let stacked = |bottom: f32, height: f32, x: f32, w: f32| {
            let y = (bottom - height).max(upper_top);
            Rect::new(x, y, w, (bottom - y).max(0.0))
        };

        let summary = stacked(band_top - STACK_GAP, SUMMARY_HEIGHT, 0.0, width);
        let start = stacked(
            summary.y - STACK_GAP,
            BUTTON_HEIGHT,
            ((width - BUTTON_WIDTH) / 2.0).max(0.0),
            BUTTON_WIDTH.min(width),
        );
        let phase_w = PHASE_ROW_WIDTH.min(width);
        let phase_row = stacked(
            start.y - STACK_GAP,
            PHASE_ROW_HEIGHT,
            ((width - phase_w) / 2.0).max(0.0),
            phase_w,
        );

        // Whatever is left over is the dial's, capped so it does not swell to
        // fill a maximised screen.
        let gauge_room = (phase_row.y - STACK_GAP - upper_top).max(0.0);
        let gauge_radius = ((gauge_room - 2.0 * GAUGE_LABEL_RING) / GAUGE_HEIGHT_PER_RADIUS)
            .min(width / 2.0 - GAUGE_LABEL_RING - MARGIN)
            .clamp(0.0, GAUGE_MAX_RADIUS);
        let gauge_centre = (
            width / 2.0,
            upper_top
                + GAUGE_LABEL_RING
                + gauge_radius
                + (gauge_room - dial_height(gauge_radius)) / 2.0,
        );

        // The last word on every rectangle: nothing may extend past the window.
        //
        // `Frame` deliberately does not clip to the window -- it records what it
        // is told -- so a rectangle laid out off the edge would keep a hit box
        // and stay clickable while being invisible. Most of the arithmetic above
        // already shrinks rather than overflowing, but a few positions are fixed
        // offsets from the title bar (the server picker sits at y=48 whatever
        // the window's height is), and in a window shorter than that they land
        // entirely outside it. Trimming here catches all of them at once, and a
        // rectangle trimmed to nothing simply stops being a target.
        let window = Rect::new(0.0, 0.0, width, height);
        let trim = |r: Rect| r.intersect(window).unwrap_or(Rect::EMPTY);

        Self {
            width,
            height,
            title_text_right: 16.0
                + text::measure("Network Speed Test", 16.0, FontWeightHint::Bold)
                + 12.0,
            server: trim(server),
            gauge_centre,
            gauge_radius,
            phase_row: trim(phase_row),
            start: trim(start),
            summary: trim(summary),
            graph: trim(graph),
            export: trim(export),
            history: trim(history),
            history_list: trim(history_list),
        }
    }
}

/// How much vertical room a dial of this radius takes, labels included.
fn dial_height(radius: f32) -> f32 {
    radius * GAUGE_HEIGHT_PER_RADIUS + 2.0 * GAUGE_LABEL_RING
}

// ============================================================================
// Simulated Measurement Parameters
// ============================================================================
//
// Until there is a network stack to talk to, the numbers a run produces are
// synthesised. These constants were literals buried in the middle of the run
// itself; they are up here because the run is now driven a frame at a time
// from `Event::Tick`, so the same parameters are read from two places (the
// live path and the batch fixture the regression tests use) and a second copy
// would be a second answer.

/// Target rate the simulated download phase converges on, in Mbps.
const SIM_DOWNLOAD_MBPS: f64 = 450.0;
/// Target rate the simulated upload phase converges on, in Mbps.
const SIM_UPLOAD_MBPS: f64 = 120.0;
/// Round-trip time the simulated latency probes centre on, in milliseconds.
const SIM_LATENCY_BASE_MS: f64 = 12.5;
/// Half-width of the simulated latency jitter, in milliseconds.
const SIM_LATENCY_VARIANCE_MS: f64 = 3.0;
/// Wall-clock gap between latency probes, in seconds.
///
/// With the default 20 probes this makes the latency phase two seconds long --
/// long enough to see the phase strip light up, short enough that it is not
/// the part of the test you wait through.
const LATENCY_PROBE_INTERVAL_SECS: f32 = 0.1;
/// Fraction of a throughput phase spent ramping up to the target rate.
const THROUGHPUT_RAMP_FRACTION: f64 = 0.2;

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
    ///
    /// `server_name` is folded because it is the one field here that is not a
    /// number: it is the name a *test server* gave for itself, and this report
    /// is built from `--- ... ---` section headers that the reader assumes the
    /// report wrote. Every field is preceded on its line by its own label, so
    /// a folded value cannot begin a line and cannot be read as a header. See
    /// [`guitk::fold`].
    ///
    /// Today `servers` is filled from a hardcoded `default_servers()`, so this
    /// is defence in depth rather than a live bug -- but the field is
    /// *semantically* remote, and it will stop being hardcoded the moment
    /// server discovery is real.
    fn to_text_report(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("--- Speed Test Result ---\n");
        out.push_str(&format!(
            "Server:       {}\n",
            fold::line(&self.server_name)
        ));
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
            server_url: String::from("speedtest.slateos.local"),
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
            name: "Slate OS Central".into(),
            url: "speedtest.slateos.local".into(),
            location: "Local Network".into(),
            distance_km: 0,
        },
        TestServer {
            name: "Metro East".into(),
            url: "east.speedtest.slateos.net".into(),
            location: "New York, US".into(),
            distance_km: 50,
        },
        TestServer {
            name: "Metro West".into(),
            url: "west.speedtest.slateos.net".into(),
            location: "Los Angeles, US".into(),
            distance_km: 3800,
        },
        TestServer {
            name: "Europe".into(),
            url: "eu.speedtest.slateos.net".into(),
            location: "Frankfurt, DE".into(),
            distance_km: 6300,
        },
        TestServer {
            name: "Asia Pacific".into(),
            url: "apac.speedtest.slateos.net".into(),
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
        // Destructured rather than indexed: `windows(2)` does only yield pairs,
        // but a slice pattern says so to the compiler instead of to the reader,
        // so the guarantee is checked rather than commented.
        for &[a, b] in self
            .samples
            .windows(2)
            .filter_map(|w| <&[f64; 2]>::try_from(w).ok())
        {
            total_diff += (b - a).abs();
            count = count.saturating_add(1);
        }
        if count == 0 {
            return None;
        }
        Some(total_diff / count as f64)
    }

    /// Packet loss percentage (0.0 to 100.0). No probes sent is no loss.
    #[must_use]
    pub fn packet_loss_pct(&self) -> f64 {
        ratio::percent(self.probes_lost, self.probes_sent).unwrap_or(0.0)
    }

    /// Number of successful samples collected.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Send one simulated probe.
    ///
    /// One probe rather than a whole run, because the run is paced by the
    /// clock: [`SpeedTestUI::tick`] calls this once per
    /// [`LATENCY_PROBE_INTERVAL_SECS`] so the probe counter fills in visible
    /// time. A method that filled the whole tester in one call could only ever
    /// be reached from a frame the user does not get to see.
    ///
    /// The generator is a parameter rather than a field because a tester is a
    /// collector of measurements, not an owner of randomness: the app holds one
    /// stream and hands it to each phase in turn, so the three phases of a run
    /// are three stretches of one sequence instead of three replays of the same
    /// short one.
    ///
    /// The jitter this produces is now two-sided. It was not: the old code
    /// built its fraction as `(state >> 33) / u32::MAX`, and a 64-bit value
    /// shifted right by 33 has only 31 bits left, so dividing it by a 32-bit
    /// maximum yields a number that never reaches 0.5. `frac - 0.5` was
    /// therefore always negative and every simulated probe came in *under* the
    /// base latency -- measured, 0 of 20 samples above 12.5 ms with a stated
    /// variance of 3.0. The graph showed a line that only ever dipped.
    pub fn simulate_probe(&mut self, rng: &mut impl RandomSource, base_ms: f64, variance_ms: f64) {
        let frac = f64::from(rng.unit_f32());
        let rtt = base_ms + (frac - 0.5) * 2.0 * variance_ms;
        if rtt > 0.0 {
            self.record_sample(rtt);
        } else {
            self.record_loss();
        }
    }

    /// Fill the whole tester in one call, for tests that want a finished one.
    ///
    /// Deliberately test-only: the app reaches [`Self::simulate_probe`] from
    /// the clock, and a batch version on the production path would be a test
    /// that completes in a frame nothing renders.
    #[cfg(test)]
    fn simulate_all(&mut self, rng: &mut impl RandomSource, base_ms: f64, variance_ms: f64) {
        for _ in 0..self.probe_count {
            self.simulate_probe(rng, base_ms, variance_ms);
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

    /// Advance `delta_secs` worth of simulated traffic aimed at `target_mbps`.
    ///
    /// One step rather than a whole run, for the reason given on
    /// [`LatencyTester::simulate_probe`]: the run is paced by the clock, and
    /// this is what one frame of it looks like. The ramp is read off
    /// [`Self::progress`] *before* the step is applied, which is what the old
    /// batch loop's `i / steps` meant.
    ///
    /// The noise this produces is now two-sided. It carried the same defect as
    /// [`LatencyTester::simulate_probe`] and for the same reason: 0 of 60 steps
    /// drew a fraction at or above 0.5, so the simulated line never rose above
    /// the target rate, only sagged below it. See that method for the
    /// arithmetic.
    pub fn advance_simulated(
        &mut self,
        rng: &mut impl RandomSource,
        delta_secs: f32,
        target_mbps: f64,
    ) {
        let frac = f64::from(rng.unit_f32());
        // Ramp up over the first fifth of the test, then fluctuate.
        let ramp = (f64::from(self.progress()) / THROUGHPUT_RAMP_FRACTION)
            .min(1.0)
            .powi(2);
        let noise = (frac - 0.5) * 0.2 * target_mbps;
        let mbps = (target_mbps * ramp + noise).max(0.0);
        self.tick(delta_secs, mbps);

        // Simulate some bytes transferred.
        let bytes_this_tick = (mbps * 1_000_000.0 / 8.0 * f64::from(delta_secs)) as u64;
        // `NonZeroU64` rather than an `if conn_count > 0` guard: the guard
        // convinces a reader but not the compiler, so the division is still
        // a division by a value that could be zero. This way the type
        // carries the fact.
        let conn_count = self.num_connections as usize;
        if let Some(conns) = NonZeroU64::new(self.num_connections.into()) {
            let per_conn = bytes_this_tick / conns;
            for c in 0..conn_count {
                self.record_bytes(c, per_conn);
            }
        }
    }

    /// Run the whole phase in one call, for tests that want a finished tester.
    ///
    /// Test-only for the reason given on [`LatencyTester::simulate_all`]. The
    /// 60 steps are what the regression tests below count their samples in.
    #[cfg(test)]
    fn simulate_all(&mut self, rng: &mut impl RandomSource, target_mbps: f64) {
        let steps = 60u32;
        let dt = self.duration_secs / steps as f32;
        for _ in 0..steps {
            self.advance_simulated(rng, dt, target_mbps);
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
            out.push_str(&format!("\nTest #{}\n", i.saturating_add(1)));
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
    /// Time banked since the last latency probe went out, in seconds.
    ///
    /// The latency phase measures out in probes, not in seconds, so it needs
    /// somewhere to keep the fraction of a probe interval that a frame does
    /// not fill. Dropping it -- probing once per frame that crosses the
    /// interval and discarding the remainder -- would tie the probe rate to
    /// the frame rate, so the same test would take longer on a slower machine.
    probe_timer_secs: f32,
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
    /// The stream the simulated runs are drawn from.
    ///
    /// One per app rather than one per tester, and seeded once at startup
    /// rather than per run, so that pressing Start twice gives two different
    /// results. Both were hardcoded literals before -- 42 and 137 -- which
    /// made every simulated speed test on every machine byte-identical.
    rng: SeededRng,
}

impl SpeedTestUI {
    /// Create a new speed test UI with default configuration.
    pub fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// Create a UI whose simulated runs come from a known seed.
    #[cfg(test)]
    fn with_seed(seed: u64) -> Self {
        Self::with_rng(SeededRng::new(seed))
    }

    fn with_rng(rng: SeededRng) -> Self {
        Self {
            rng,
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
            probe_timer_secs: 0.0,
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
        self.probe_timer_secs = 0.0;

        // Begin with latency phase.
        self.phase = SpeedTestPhase::Testing(TestKind::Latency);
    }

    /// Advance a running test by `delta_secs` of wall clock.
    ///
    /// This is the whole test. Until 2026-08-25 there was no such method and
    /// Start called a `simulate_test` that ran latency, download and upload
    /// back to back inside one call: it assigned `Testing(Latency)`,
    /// `Testing(Download)` and `Testing(Upload)` in turn and returned in
    /// `Complete`, so no frame was ever drawn in a testing phase. Everything
    /// the app is built around was therefore unreachable -- the live graph
    /// (`graph_points` arrived full), the gauge sweep (`current_speed_mbps`
    /// jumped straight to the final average), the phase strip's
    /// currently-running highlight, and Escape's cancel, which is guarded on
    /// `phase.is_testing()` and so could never fire. A ten-second test
    /// finished in a frame and showed a plausible result. See
    /// known-issues.md lesson 47.
    ///
    /// Seconds rather than milliseconds because both testers measure in
    /// seconds; the conversion from [`Event::Tick`]'s `elapsed_ms` happens
    /// once, at the event.
    pub fn tick(&mut self, delta_secs: f32) {
        let SpeedTestPhase::Testing(kind) = self.phase else {
            return;
        };
        if !delta_secs.is_finite() || delta_secs <= 0.0 {
            return;
        }

        match kind {
            TestKind::Latency => self.tick_latency(delta_secs),
            TestKind::Download => {
                self.download_tester.advance_simulated(
                    &mut self.rng,
                    delta_secs,
                    SIM_DOWNLOAD_MBPS,
                );
                // The gauge wants the instantaneous rate while the test runs;
                // `finalize_test` replaces it with the average at the end.
                self.current_speed_mbps = self.download_tester.current_mbps();
                self.graph_points = self.download_tester.samples().to_vec();
                if self.download_tester.is_complete() {
                    self.phase = SpeedTestPhase::Testing(TestKind::Upload);
                }
            }
            TestKind::Upload => {
                self.upload_tester
                    .advance_simulated(&mut self.rng, delta_secs, SIM_UPLOAD_MBPS);
                self.current_speed_mbps = self.upload_tester.current_mbps();
                self.graph_points = self.upload_tester.samples().to_vec();
                if self.upload_tester.is_complete() {
                    self.finalize_test();
                }
            }
        }
    }

    /// Send however many probes `delta_secs` has paid for, then hand on.
    ///
    /// A loop rather than one probe per frame because a frame can be long --
    /// the window was unmapped, the machine stalled -- and a phase that only
    /// ever advances one probe per frame would stretch to fit the stall. The
    /// loop is bounded by the probe count, which `is_complete` caps.
    fn tick_latency(&mut self, delta_secs: f32) {
        self.probe_timer_secs += delta_secs;
        while self.probe_timer_secs >= LATENCY_PROBE_INTERVAL_SECS
            && !self.latency_tester.is_complete()
        {
            self.probe_timer_secs -= LATENCY_PROBE_INTERVAL_SECS;
            self.latency_tester.simulate_probe(
                &mut self.rng,
                SIM_LATENCY_BASE_MS,
                SIM_LATENCY_VARIANCE_MS,
            );
        }
        self.current_latency_ms = self.latency_tester.avg_rtt().unwrap_or(0.0);

        if self.latency_tester.is_complete() {
            // Whatever is left in the timer belongs to the download phase's
            // first frame, not to a probe that will never be sent.
            self.probe_timer_secs = 0.0;
            self.phase = SpeedTestPhase::Testing(TestKind::Download);
        }
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
        // The list just changed length -- and once it is at `MAX_HISTORY` a
        // push also *drops* the oldest, so the content can shrink back under
        // the offset. Re-clamping here is what stops a full history leaving
        // the view parked past its own end.
        self.clamp_history_scroll();
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

    // ========================================================================
    // Layout
    //
    // One description of where everything is, read by the renderer and by the
    // hit test through the same `Frame`. It used to be neither: the geometry
    // was a screen of absolute constants derived from `WINDOW_WIDTH`, and
    // `handle_click` re-derived every rectangle a second time from the same
    // constants. Three consequences, all of them shipped:
    //
    //  * the window did not resize. `self.width`/`self.height` were written
    //    once at construction and read by the background fill and the title
    //    bar; nothing else consulted them and no `Event::Resize` arm existed,
    //    so widening the window painted a wider background around a 900x720
    //    layout pinned to the top-left.
    //  * the Start button and the whole result summary were drawn *underneath*
    //    the graph panel. Start sat at y 410..446 and the summary at 455..;
    //    the graph's opaque background covered 420..600 and was pushed after
    //    both. So the numbers a speed test exists to produce -- download,
    //    upload, latency/jitter -- were painted every frame and never once
    //    visible, and the button you press to start was two thirds buried.
    //  * click and paint agreed only by being written twice from the same
    //    literals, which is the arrangement `history_row_at` was already the
    //    scar tissue from.
    // ========================================================================

    /// Where every control is, for one window size.
    fn layout(&self) -> Layout {
        Layout::new(self.width, self.height)
    }

    /// How far the list can scroll before its last row sits on the bottom of
    /// the viewport.
    ///
    /// `history_scroll` had no upper bound because nothing knew how tall the
    /// list was -- the row height and the panel geometry were literals spread
    /// across the renderer. Deriving it from the measured content is what lets
    /// the wheel stop rather than walking the list off into empty space.
    pub fn max_history_scroll(&self) -> f32 {
        self.max_history_scroll_in(&self.layout())
    }

    fn max_history_scroll_in(&self, l: &Layout) -> f32 {
        let content = self.history.len() as f32 * HISTORY_ROW_HEIGHT;
        (content - l.history_list.h).max(0.0)
    }

    /// Pull the offset back inside its bounds after the list or the panel
    /// changed shape under it.
    fn clamp_history_scroll(&mut self) {
        self.history_scroll = self.history_scroll.clamp(0.0, self.max_history_scroll());
    }

    /// Scroll the history by `delta` pixels, clamped at both ends.
    fn scroll_history_by(&mut self, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        self.history_scroll += delta;
        self.clamp_history_scroll();
    }

    /// Every history row in draw order, as `(screen y of its top, index into
    /// the history)`.
    ///
    /// The scroll offset is already applied, so the renderer and the hit test
    /// do not merely agree on the formula -- they read the same numbers.
    ///
    /// The index mapping is the point. The list is drawn newest-first, so draw
    /// position `i` is history index `len - 1 - i`. That reversal lived only
    /// in the renderer; the hit test stored the raw draw position in
    /// `history_hover`, which the renderer then matched against a history
    /// index. The list was therefore mirrored: hovering the newest result
    /// highlighted the oldest, and the middle row was the only one that
    /// highlighted itself.
    fn history_rows(&self, l: &Layout) -> Vec<(f32, usize)> {
        let top = l.history_list.y - self.history_scroll;
        // `.rev()` *is* the newest-first ordering, and `enumerate` then gives
        // the draw position -- so the reversal is one call rather than an
        // index subtraction that has to be trusted not to underflow.
        (0..self.history.len())
            .rev()
            .enumerate()
            .map(|(draw_pos, idx)| (top + draw_pos as f32 * HISTORY_ROW_HEIGHT, idx))
            .collect()
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// The control under `(x, y)`, or `None` for bare background.
    ///
    /// Drawn rather than remembered: the frame the user is looking at is the
    /// only authority on where anything is, and re-deriving it here is how the
    /// two stay one description. A frame is a few hundred pushes into a
    /// `Vec` -- cheaper than the mouse move that asked for it.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    /// Handle a UI event (keyboard or mouse).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            // A speed test is a measurement over time, so this is the event
            // that makes the app work at all. It was falling into the
            // `_ => EventResult::Ignored` arm below; see [`Self::tick`] for
            // what that cost.
            //
            // `Consumed` only while a test is running: an idle window has no
            // reason to claim the clock, and saying so lets a caller tell a
            // frame that changed something from one that did not.
            Event::Tick { elapsed_ms } => {
                if self.phase.is_testing() {
                    self.tick(*elapsed_ms as f32 / 1000.0);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // The layout is derived from these two numbers, so this arm is the
            // difference between an app that resizes and one that paints a
            // wider background around a picture of a 900x720 window.
            Event::Resize { width, height } => {
                self.resize(*width as f32, *height as f32);
                EventResult::Consumed
            }
            Event::Mouse(mouse_event) => {
                let x = mouse_event.x;
                let y = mouse_event.y;
                match mouse_event.kind {
                    MouseEventKind::Press(MouseButton::Left) => self.handle_click(x, y),
                    MouseEventKind::Move => self.handle_mouse_move(x, y),
                    // There was no wheel handler at all, and nothing else ever
                    // wrote `history_scroll` -- so with `MAX_HISTORY` of 20
                    // rows of 26 px in a 206 px viewport, twelve of the twenty
                    // results were drawn nowhere and reachable by nothing.
                    //
                    // `dy` is a *notch count*, not a distance (see
                    // `guitk::wheel`), and `pixels` rather than an
                    // `Accumulator` because `history_scroll` is an `f32` and
                    // can therefore hold a trackpad's fraction of a notch
                    // directly, with no need to bank it until it becomes a
                    // whole row.
                    MouseEventKind::Scroll { dy, .. } => {
                        // The panel target underlies the header and the stats
                        // strip as well as the rows, so the wheel works over
                        // the whole panel while a *click* still only reaches a
                        // row where a row is drawn.
                        if matches!(
                            self.target_at(x, y),
                            Some(Target::HistoryPanel | Target::HistoryRow(_))
                        ) {
                            self.scroll_history_by(wheel::pixels(dy, HISTORY_ROW_HEIGHT));
                            EventResult::Consumed
                        } else {
                            EventResult::Ignored
                        }
                    }
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Adopt a new window size and pull anything that hung off the old one
    /// back inside.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        // A taller panel shows more rows, which can leave the offset past the
        // end of a list that no longer reaches that far.
        self.clamp_history_scroll();
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter | Key::Space => {
                if self.phase.is_idle() || self.phase.is_complete() {
                    self.start_test();
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
        match self.target_at(x, y) {
            Some(Target::Start) => {
                // A running test's button says "Testing..." and does nothing;
                // Escape is the way out of it.
                if self.phase.is_testing() {
                    EventResult::Ignored
                } else {
                    self.start_test();
                    EventResult::Consumed
                }
            }
            Some(Target::Export) => {
                let _export = self.history.export_as_text();
                self.export_button_hover = true;
                EventResult::Consumed
            }
            Some(Target::ServerButton) => {
                self.server_dropdown_open = !self.server_dropdown_open;
                EventResult::Consumed
            }
            Some(Target::ServerItem(index)) => {
                self.select_server(index);
                EventResult::Consumed
            }
            // The open list covers the window, so a click anywhere else shuts
            // it instead of reaching what is behind it -- the frame records
            // that as a target rather than leaving it to a fall-through, so a
            // control added later cannot accidentally be clicked through an
            // open menu.
            Some(Target::DropdownScrim) => {
                self.server_dropdown_open = false;
                EventResult::Consumed
            }
            Some(Target::HistoryRow(index)) => {
                // Selecting a history item could show details; for now just
                // highlight it.
                self.history_hover = Some(index);
                EventResult::Consumed
            }
            Some(Target::HistoryPanel) | None => EventResult::Ignored,
        }
    }

    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        let target = self.target_at(x, y);
        let start = target == Some(Target::Start);
        let export = target == Some(Target::Export);
        let row = match target {
            Some(Target::HistoryRow(index)) => Some(index),
            _ => None,
        };

        // `Consumed` means "this changed what is on screen", which is what a
        // caller needs in order to decide whether to redraw. A move that
        // leaves every highlight where it was must not cost a frame, and at
        // pointer rates most moves are exactly that.
        let changed = start != self.start_button_hover
            || export != self.export_button_hover
            || row != self.history_hover;
        self.start_button_hover = start;
        self.export_button_hover = export;
        self.history_hover = row;

        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire UI at the size it last knew about.
    ///
    /// Kept for callers that have no size to offer; [`Self::frame`] is the
    /// real renderer and is what a window calls, with the size the compositor
    /// actually gave it.
    pub fn render(&self) -> RenderTree {
        self.frame(self.width, self.height).into_tree()
    }

    /// Draw the whole window at `width` by `height`, recording where every
    /// control ended up.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut frame = Frame::new(width, height);

        // Background.
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        self.draw_title_bar(&mut frame, &l);
        self.draw_server_selector(&mut frame, &l);
        self.draw_gauge(&mut frame, &l);
        self.draw_speed_label(&mut frame, &l);
        self.draw_phase_indicators(&mut frame, &l);
        self.draw_start_button(&mut frame, &l);
        self.draw_result_summary(&mut frame, &l);
        self.draw_graph(&mut frame, &l);
        self.draw_history_panel(&mut frame, &l);
        self.draw_export_button(&mut frame, &l);

        // Server dropdown overlay (drawn last so it draws on top).
        if self.server_dropdown_open {
            // An open menu is modal: everything under it keeps its pixels and
            // loses its clicks. Without this the Start button behind the list
            // still worked, so the menu only *looked* in front.
            frame.discard_hits();
            frame.hit(Target::DropdownScrim, Rect::new(0.0, 0.0, width, height));
            self.draw_server_dropdown(&mut frame, &l);
        }

        frame
    }

    fn draw_title_bar(&self, frame: &mut Frame, l: &Layout) {
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.width,
            height: TITLE_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        frame.push(RenderCommand::Text {
            x: 16.0,
            y: 12.0,
            text: "Network Speed Test".into(),
            color: TEXT_COLOR,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        // Right-aligned by measurement rather than by a 200 px guess, so a
        // long phase name -- "Error: server URL must not be empty" is one --
        // ends at the window's edge instead of past it.
        let phase = format!("Phase: {}", self.phase.label());
        let phase_w = text::measure(&phase, 12.0, FontWeightHint::Regular);
        frame.push(RenderCommand::Text {
            x: (l.width - 16.0 - phase_w).max(l.title_text_right),
            y: 14.0,
            text: phase,
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.width - 16.0 - l.title_text_right).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn draw_server_selector(&self, frame: &mut Frame, l: &Layout) {
        let r = l.server;

        frame.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.hit(Target::ServerButton, r);

        let server_name = self
            .servers
            .get(self.selected_server)
            .map_or("Select Server", |s| s.name.as_str());
        frame.push(RenderCommand::Text {
            x: r.x + 10.0,
            y: r.y + 7.0,
            text: server_name.into(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((r.w - 30.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Dropdown arrow.
        let arrow_text = if self.server_dropdown_open {
            "\u{25B2}"
        } else {
            "\u{25BC}"
        };
        frame.push(RenderCommand::Text {
            x: r.right() - 20.0,
            y: r.y + 7.0,
            text: arrow_text.into(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn draw_server_dropdown(&self, frame: &mut Frame, l: &Layout) {
        let item_h = SERVER_HEIGHT;
        let x = l.server.x;
        let w = l.server.w;
        let base_y = l.server.bottom() + 10.0;
        let total_h = self.servers.len() as f32 * item_h;

        frame.push(RenderCommand::FillRect {
            x,
            y: base_y,
            width: w,
            height: total_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
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
                frame.push(RenderCommand::FillRect {
                    x: x + 2.0,
                    y: iy + 1.0,
                    width: w - 4.0,
                    height: item_h - 2.0,
                    color: SURFACE1,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
            frame.push(RenderCommand::Text {
                x: x + 10.0,
                y: iy + 6.0,
                text: format!("{} ({})", server.name, server.location),
                color: if i == self.selected_server {
                    BLUE
                } else {
                    TEXT_COLOR
                },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((w - 20.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::ServerItem(i), Rect::new(x, iy, w, item_h));
        }
    }

    fn draw_gauge(&self, frame: &mut Frame, l: &Layout) {
        let (cx, cy) = l.gauge_centre;
        let outer_r = l.gauge_radius;
        let track = (outer_r * 0.107).max(3.0);
        let inner_r = outer_r - track;

        // Draw arc background (dark track).
        for seg in 0..GAUGE_ARC_SEGMENTS {
            let frac0 = seg as f32 / GAUGE_ARC_SEGMENTS as f32;
            let frac1 = seg.saturating_add(1) as f32 / GAUGE_ARC_SEGMENTS as f32;
            let a0 = gauge_fraction_to_angle(frac0);
            let a1 = gauge_fraction_to_angle(frac1);
            let (ox0, oy0) = point_on_circle(cx, cy, outer_r, a0);
            let (ox1, oy1) = point_on_circle(cx, cy, outer_r, a1);
            frame.push(RenderCommand::Line {
                x1: ox0,
                y1: oy0,
                x2: ox1,
                y2: oy1,
                color: SURFACE0,
                width: track,
            });
        }

        // Draw filled arc for current speed.
        let fill_frac = speed_to_gauge_fraction(self.current_speed_mbps);
        let fill_segments =
            ((fill_frac * GAUGE_ARC_SEGMENTS as f32).ceil() as usize).min(GAUGE_ARC_SEGMENTS);
        for seg in 0..fill_segments {
            let frac0 = seg as f32 / GAUGE_ARC_SEGMENTS as f32;
            let frac1 = (seg.saturating_add(1) as f32 / GAUGE_ARC_SEGMENTS as f32).min(fill_frac);
            let a0 = gauge_fraction_to_angle(frac0);
            let a1 = gauge_fraction_to_angle(frac1);
            let (ox0, oy0) = point_on_circle(cx, cy, outer_r - track / 2.0, a0);
            let (ox1, oy1) = point_on_circle(cx, cy, outer_r - track / 2.0, a1);
            let color = gauge_color_at(frac0);
            frame.push(RenderCommand::Line {
                x1: ox0,
                y1: oy0,
                x2: ox1,
                y2: oy1,
                color,
                width: track * 0.875,
            });
        }

        // Draw speed marker ticks and labels.
        for &marker in GAUGE_MARKERS {
            let frac = speed_to_gauge_fraction(f64::from(marker));
            let angle = gauge_fraction_to_angle(frac);
            let (tx0, ty0) = point_on_circle(cx, cy, outer_r + 2.0, angle);
            let (tx1, ty1) = point_on_circle(cx, cy, outer_r + 10.0, angle);
            frame.push(RenderCommand::Line {
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
            frame.push(RenderCommand::Text {
                x: lx - 10.0,
                y: ly - 5.0,
                text: label,
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Needle indicator.
        let needle_frac = speed_to_gauge_fraction(self.current_speed_mbps);
        let needle_angle = gauge_fraction_to_angle(needle_frac);
        let (nx, ny) = point_on_circle(cx, cy, (inner_r - 4.0).max(0.0), needle_angle);
        frame.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: nx,
            y2: ny,
            color: TEXT_COLOR,
            width: 2.0,
        });

        // Center dot.
        frame.push(RenderCommand::FillRect {
            x: cx - 6.0,
            y: cy - 6.0,
            width: 12.0,
            height: 12.0,
            color: TEXT_COLOR,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    fn draw_speed_label(&self, frame: &mut Frame, l: &Layout) {
        let (cx, cy) = l.gauge_centre;
        let r = l.gauge_radius;
        // The readout lives in the gauge's open bottom, so it is sized from
        // the gauge rather than from a literal: a small window shrinks the arc
        // and the number together instead of leaving a 32 px figure sprawled
        // across a 60 px dial.
        let value_size = (r * 0.21).clamp(10.0, 32.0);
        let unit_size = (r * 0.093).clamp(8.0, 14.0);
        let latency_size = (r * 0.073).clamp(7.0, 11.0);

        let speed_text = format!("{:.1}", self.current_speed_mbps);
        let speed_w = text::measure(&speed_text, value_size, FontWeightHint::Bold);
        frame.push(RenderCommand::Text {
            x: cx - speed_w / 2.0,
            y: cy + r * 0.23,
            text: speed_text,
            color: TEXT_COLOR,
            font_size: value_size,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let unit_w = text::measure("Mbps", unit_size, FontWeightHint::Regular);
        frame.push(RenderCommand::Text {
            x: cx - unit_w / 2.0,
            y: cy + r * 0.47,
            text: "Mbps".into(),
            color: SUBTEXT0,
            font_size: unit_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Latency readout below gauge.
        if self.current_latency_ms > 0.0 {
            let latency = format!("Latency: {:.1} ms", self.current_latency_ms);
            let latency_w = text::measure(&latency, latency_size, FontWeightHint::Regular);
            frame.push(RenderCommand::Text {
                x: cx - latency_w / 2.0,
                y: cy + r * 0.6,
                text: latency,
                color: SUBTEXT1,
                font_size: latency_size,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn draw_phase_indicators(&self, frame: &mut Frame, l: &Layout) {
        let phases = [TestKind::Latency, TestKind::Download, TestKind::Upload];
        let labels = ["Latency", "Download", "Upload"];
        let icons = [TEAL, BLUE, MAUVE];
        let step = (l.phase_row.w / 3.0).max(0.0);
        let y = l.phase_row.y;

        for (i, (kind, label)) in phases.iter().zip(labels.iter()).enumerate() {
            let x = l.phase_row.x + i as f32 * step;

            let (dot_color, text_color) = match &self.phase {
                SpeedTestPhase::Testing(active) if active == kind => {
                    (icons.get(i).copied().unwrap_or(TEXT_COLOR), TEXT_COLOR)
                }
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
            frame.push(RenderCommand::FillRect {
                x,
                y,
                width: 10.0,
                height: 10.0,
                color: dot_color,
                corner_radii: CornerRadii::all(5.0),
            });

            // Phase label.
            frame.push(RenderCommand::Text {
                x: x + 16.0,
                y: y - 1.0,
                text: (*label).into(),
                color: text_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((step - 26.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });

            // Arrow between phases, centred in the gap this label leaves.
            if i < 2 {
                let label_end = x + 16.0 + text::measure(label, 12.0, FontWeightHint::Regular);
                let arrow_x = (label_end + (x + step - label_end) / 2.0 - 4.0).max(label_end + 2.0);
                frame.push(RenderCommand::Text {
                    x: arrow_x,
                    y: y - 1.0,
                    text: "\u{2192}".into(),
                    color: SURFACE2,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }
    }

    fn draw_start_button(&self, frame: &mut Frame, l: &Layout) {
        let r = l.start;

        let (bg, label) = if self.phase.is_testing() {
            (SURFACE1, "Testing...")
        } else if self.start_button_hover {
            (SAPPHIRE, "Start Test")
        } else if self.phase.is_complete() {
            (BLUE, "Re-Test")
        } else {
            (BLUE, "Start Test")
        };

        frame.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: bg,
            corner_radii: CornerRadii::all(6.0),
        });
        let label_w = text::measure(label, 14.0, FontWeightHint::Bold);
        frame.push(RenderCommand::Text {
            x: r.x + (r.w - label_w).max(0.0) / 2.0,
            y: r.y + (r.h - 14.0) / 2.0,
            text: label.into(),
            color: if bg == SAPPHIRE { CRUST } else { TEXT_COLOR },
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(r.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Start, r);
    }

    fn draw_result_summary(&self, frame: &mut Frame, l: &Layout) {
        let r = l.summary;
        if !self.phase.is_complete() && !self.phase.is_testing() {
            if let SpeedTestPhase::Error(ref msg) = self.phase {
                let text = format!("Error: {msg}");
                let w = text::measure(&text, 12.0, FontWeightHint::Regular).min(r.w);
                frame.push(RenderCommand::Text {
                    x: r.x + (r.w - w).max(0.0) / 2.0,
                    y: r.y + 8.0,
                    text,
                    color: RED,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(r.w),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            return;
        }

        // Three equal columns across the summary strip. They used to be three
        // offsets from the gauge's centre, 140 px apart, which put the leftmost
        // at x=240 in a 900 px window and left the whole row overlapping the
        // graph panel below it.
        let col_w = (r.w / 3.0).max(0.0);
        let columns = [
            (
                "Download",
                format!("{:.1} Mbps", self.download_tester.avg_mbps()),
                BLUE,
            ),
            (
                "Upload",
                format!("{:.1} Mbps", self.upload_tester.avg_mbps()),
                MAUVE,
            ),
            (
                "Latency / Jitter",
                format!(
                    "{:.1} / {:.1} ms",
                    self.latency_tester.avg_rtt().unwrap_or(0.0),
                    self.latency_tester.jitter().unwrap_or(0.0),
                ),
                TEAL,
            ),
        ];

        for (i, (heading, value, color)) in columns.into_iter().enumerate() {
            let cx = r.x + (i as f32 + 0.5) * col_w;
            let heading_w = text::measure(heading, 10.0, FontWeightHint::Regular).min(col_w);
            frame.push(RenderCommand::Text {
                x: cx - heading_w / 2.0,
                y: r.y,
                text: heading.into(),
                color: SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_w),
                overflow: TextOverflow::Ellipsis,
            });
            let value_w = text::measure(&value, 14.0, FontWeightHint::Bold).min(col_w);
            frame.push(RenderCommand::Text {
                x: cx - value_w / 2.0,
                y: r.y + 14.0,
                text: value,
                color,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn draw_graph(&self, frame: &mut Frame, l: &Layout) {
        let panel = l.graph;

        frame.push(RenderCommand::FillRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        frame.push(RenderCommand::Text {
            x: panel.x + 12.0,
            y: panel.y + 8.0,
            text: "Speed Over Time".into(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some((panel.w - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        let plot_x = panel.x + 50.0;
        let plot_y = panel.y + 30.0;
        let plot_w = (panel.w - 60.0).max(0.0);
        let plot_h = (panel.h - 50.0).max(0.0);

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
            frame.push(RenderCommand::Line {
                x1: plot_x,
                y1: gy,
                x2: plot_x + plot_w,
                y2: gy,
                color: SURFACE0,
                width: 0.5,
            });
            let val = max_speed * f64::from(frac);
            frame.push(RenderCommand::Text {
                x: panel.x + 4.0,
                y: gy - 5.0,
                text: format!("{val:.0}"),
                color: SURFACE2,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(42.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Plot data.
        if samples.len() >= 2 {
            let max_time = samples.last().map_or(1.0, |s| s.elapsed_secs.max(0.1));

            // Slice pattern rather than indexing: see `avg_jitter`.
            for [s0, s1] in samples
                .windows(2)
                .filter_map(|w| <&[ThroughputSample; 2]>::try_from(w).ok())
            {
                let x0 = plot_x + (s0.elapsed_secs / max_time) * plot_w;
                let y0 = plot_y + plot_h - (s0.mbps / max_speed) as f32 * plot_h;
                let x1 = plot_x + (s1.elapsed_secs / max_time) * plot_w;
                let y1 = plot_y + plot_h - (s1.mbps / max_speed) as f32 * plot_h;
                frame.push(RenderCommand::Line {
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
                frame.push(RenderCommand::Text {
                    x: lx - 8.0,
                    y: plot_y + plot_h + 4.0,
                    text: format!("{t:.0}s"),
                    color: SURFACE2,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        } else {
            // No data placeholder.
            let w = text::measure("No data yet", 12.0, FontWeightHint::Regular);
            frame.push(RenderCommand::Text {
                x: plot_x + (plot_w - w) / 2.0,
                y: plot_y + plot_h / 2.0 - 5.0,
                text: "No data yet".into(),
                color: SURFACE2,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn draw_history_panel(&self, frame: &mut Frame, l: &Layout) {
        let panel = l.history;
        let list = l.history_list;

        frame.push(RenderCommand::FillRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: panel.x,
            y: panel.y,
            width: panel.w,
            height: panel.h,
            color: SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });
        // Recorded before the rows, so a row painted on top of it wins the hit
        // test and the bare panel only answers where no row is drawn. That is
        // what lets the wheel work over the header and the stats strip while a
        // click there still selects nothing.
        frame.hit(Target::HistoryPanel, panel);

        frame.push(RenderCommand::Text {
            x: panel.x + 12.0,
            y: panel.y + 8.0,
            text: format!("History ({})", self.history.len()),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some((panel.w - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Clip to history area. The frame trims every hit box recorded inside
        // to this rectangle and drops the ones it cuts to nothing, so a row
        // scrolled out of the viewport stops being clickable by construction
        // rather than by a bound the hit test remembers to apply.
        frame.clip(list);

        if self.history.is_empty() {
            frame.push(RenderCommand::Text {
                x: panel.x + 12.0,
                y: list.y + 10.0,
                text: "Run a test to see results".into(),
                color: SURFACE2,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((panel.w - 24.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            // Newest-first, and in the same order the hit test reads: both go
            // through `history_rows`, which is what stops the highlight
            // landing on a different row from the pointer.
            let results = self.history.results();
            for (ry, idx) in self.history_rows(l) {
                if ry + HISTORY_ROW_HEIGHT <= list.y || ry >= list.bottom() {
                    continue;
                }
                let Some(result) = results.get(idx) else {
                    continue;
                };

                // Hover highlight.
                if self.history_hover == Some(idx) {
                    frame.push(RenderCommand::FillRect {
                        x: panel.x + 4.0,
                        y: ry,
                        width: (panel.w - 8.0).max(0.0),
                        height: HISTORY_ROW_HEIGHT,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(3.0),
                    });
                }

                frame.push(RenderCommand::Text {
                    x: panel.x + 12.0,
                    y: ry + 6.0,
                    text: result.summary_line(),
                    color: SUBTEXT1,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some((panel.w - 24.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
                frame.hit(
                    Target::HistoryRow(idx),
                    Rect::new(panel.x, ry, panel.w, HISTORY_ROW_HEIGHT),
                );
            }
        }

        frame.unclip();

        // History stats at the bottom.
        if !self.history.is_empty() {
            frame.push(RenderCommand::FillRect {
                x: panel.x,
                y: list.bottom(),
                width: panel.w,
                height: HISTORY_STATS_HEIGHT,
                color: MANTLE,
                corner_radii: CornerRadii {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_right: 8.0,
                    bottom_left: 8.0,
                },
            });
            frame.push(RenderCommand::Text {
                x: panel.x + 8.0,
                y: list.bottom() + 6.0,
                text: format!(
                    "Avg: {:.0}/{:.0} Mbps, {:.0}ms",
                    self.history.avg_download(),
                    self.history.avg_upload(),
                    self.history.avg_latency(),
                ),
                color: SUBTEXT0,
                font_size: 9.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((panel.w - 16.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn draw_export_button(&self, frame: &mut Frame, l: &Layout) {
        let r = l.export;

        frame.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: if self.export_button_hover {
                SURFACE1
            } else {
                SURFACE0
            },
            corner_radii: CornerRadii::all(4.0),
        });
        let label_w = text::measure("Export", 11.0, FontWeightHint::Regular);
        frame.push(RenderCommand::Text {
            x: r.x + (r.w - label_w).max(0.0) / 2.0,
            y: r.y + (r.h - 11.0) / 2.0,
            text: "Export".into(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(r.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Export, r);
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
// The window
// ============================================================================

impl App for SpeedTestUI {
    fn title(&self) -> String {
        "Network Speed Test".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// The clock, but only while there is something to measure.
    ///
    /// A speed test is a measurement over time and the dial sweeps as it runs,
    /// so a running test asks for a frame's worth of clock. An idle window asks
    /// for none: `sync_clock` cancels the wake-up, and a window showing a
    /// finished result costs the compositor nothing until the user touches it.
    fn tick_interval(&self) -> Option<Duration> {
        self.phase.is_testing().then(|| Duration::from_millis(16))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Ctrl+Q closes the window. Escape does not: it cancels a running test,
        // which is the more useful answer to the key a user reaches for when a
        // ten-second measurement is halfway through.
        if let Event::Key(key) = event
            && key.pressed
            && key.key == Key::Q
            && key.modifiers.ctrl
        {
            return Response::Exit;
        }
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The remembered size is only ever a starting guess; this is the real
        // one, and the hit test reads it back through `handle_event`.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SpeedTestUI {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    app::launch("speedtest", &mut SpeedTestUI::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    // Not in the production imports above, because nothing outside the tests
    // names a modifier set: the app reads `key.modifiers.ctrl` off the event it
    // was handed and never constructs one.
    use guitk::event::Modifiers;
    // The free helpers -- `click`, `rect_of`, `press`. The production code
    // imports only the `Probe` trait it implements.
    use guitk::probe;

    /// A frame at roughly 60 Hz, the interval `oswindow` would hand us.
    const FRAME_MS: u64 = 16;

    /// Enough frames for the default run -- 2 s of probes plus two 10 s
    /// throughput phases -- with room to spare, so the bound is a deadlock
    /// guard rather than a second definition of how long a test takes.
    const FRAME_BUDGET: usize = 4000;

    /// Press Enter, then feed frames until the run finishes.
    ///
    /// Every test that wants a completed run goes through the keyboard and
    /// the clock rather than calling an internal, because a test that calls
    /// the internal cannot tell a wired app from an unwired one -- which is
    /// exactly how this app shipped with `Event::Tick` in its `_` arm.
    fn run_a_full_test(ui: &mut SpeedTestUI) {
        press(ui, Key::Enter);
        assert!(ui.phase().is_testing(), "Enter did not start a test");
        for _ in 0..FRAME_BUDGET {
            if ui.phase().is_complete() {
                return;
            }
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
        }
        panic!("the run did not finish within {FRAME_BUDGET} frames");
    }

    /// Send an unmodified key press.
    fn press(ui: &mut SpeedTestUI, key: Key) -> EventResult {
        ui.handle_event(&Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::default(),
            text: String::new(),
        }))
    }

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
        assert_eq!(
            SpeedTestPhase::Testing(TestKind::Download).label(),
            "Download"
        );
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
        let cfg = SpeedTestConfig {
            server_url: String::new(),
            ..SpeedTestConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_zero_duration_invalid() {
        let cfg = SpeedTestConfig {
            test_duration_secs: 0,
            ..SpeedTestConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_excessive_duration_invalid() {
        let cfg = SpeedTestConfig {
            test_duration_secs: 200,
            ..SpeedTestConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_zero_connections_invalid() {
        let cfg = SpeedTestConfig {
            num_connections: 0,
            ..SpeedTestConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_excessive_connections_invalid() {
        let cfg = SpeedTestConfig {
            num_connections: 64,
            ..SpeedTestConfig::default()
        };
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
        t.simulate_all(&mut SeededRng::new(1), 10.0, 2.0);
        assert!(t.is_complete());
        assert!(t.sample_count() > 0);
        assert!(t.avg_rtt().is_some());
    }

    /// The jitter has to go both ways.
    ///
    /// This is the regression test for the `>> 33` fraction: 31 bits over a
    /// 32-bit maximum can never reach 0.5, so `frac - 0.5` was always negative
    /// and every probe landed under the base. Measured on the old code with
    /// its hardcoded seed of 42: 0 of 20 samples above the base. The floor
    /// below is a quarter of the sample count in each direction, which a fair
    /// coin clears with room to spare and a one-sided one cannot clear at all.
    #[test]
    fn simulated_latency_varies_both_above_and_below_the_base() {
        const PROBES: u32 = 40;
        const BASE: f64 = 12.5;
        let mut t = LatencyTester::new(PROBES);
        t.simulate_all(&mut SeededRng::new(0x51EE_D7E5_7A11_C0DE), BASE, 3.0);

        let above = t.samples.iter().filter(|r| **r > BASE).count();
        let below = t.samples.iter().filter(|r| **r < BASE).count();
        assert!(
            above >= 10 && below >= 10,
            "jitter is one-sided: {above} probes above {BASE} ms and {below} below, of {PROBES}"
        );
    }

    /// Two runs of the same app must not produce the same numbers.
    ///
    /// The old simulators restarted from the literals 42 and 137 on every
    /// call, so pressing Start twice redrew an identical graph -- and so did
    /// every other machine running the app.
    #[test]
    fn two_simulated_runs_of_one_app_differ() {
        let mut app = SpeedTestUI::with_seed(0xA5A5_1234_5678_9ABC);
        run_a_full_test(&mut app);
        let first: Vec<f64> = app
            .download_tester
            .samples()
            .iter()
            .map(|s| s.mbps)
            .collect();
        run_a_full_test(&mut app);
        let second: Vec<f64> = app
            .download_tester
            .samples()
            .iter()
            .map(|s| s.mbps)
            .collect();
        assert_ne!(first, second, "the second run replayed the first");
    }

    /// A fresh app takes its seed from the system, not from a literal.
    ///
    /// Host `cargo test` has no SlateOS entropy source, so `seeded_from_system`
    /// returns the fallback and two fresh apps agree -- which is exactly what a
    /// hardcoded seed would also do. The test therefore asserts *which* seed:
    /// equal to a run from `FALLBACK_SEED`, and unequal to one from any other
    /// literal. Gated off Unix, where the host does have entropy and a fresh
    /// app is genuinely unpredictable.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_app_is_seeded_by_the_system_and_not_by_a_literal() {
        fn first_run(mut app: SpeedTestUI) -> Vec<f64> {
            run_a_full_test(&mut app);
            app.download_tester
                .samples()
                .iter()
                .map(|s| s.mbps)
                .collect()
        }
        let fresh = first_run(SpeedTestUI::new());
        assert_eq!(fresh, first_run(SpeedTestUI::with_seed(FALLBACK_SEED)));
        assert_ne!(fresh, first_run(SpeedTestUI::with_seed(42)));
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
        t.simulate_all(&mut SeededRng::new(2), 500.0);
        assert!(t.is_complete());
        assert!(t.avg_mbps() > 0.0);
        assert!(t.total_bytes() > 0);
    }

    /// The throughput line has to overshoot the target sometimes.
    ///
    /// Same defect, same regression test as
    /// `simulated_latency_varies_both_above_and_below_the_base`: measured on
    /// the old code, 0 of 60 steps drew a fraction at or above 0.5, so the
    /// simulated rate never once exceeded the target. Only the steps after the
    /// 20% ramp are examined -- during the ramp the rate is legitimately below
    /// target no matter which way the noise goes.
    #[test]
    fn simulated_throughput_overshoots_the_target_sometimes() {
        const TARGET: f64 = 500.0;
        let mut t = ThroughputTester::new(4, 10.0);
        t.simulate_all(&mut SeededRng::new(0x7B0E_4C11_9D2F_A063), TARGET);
        let after_ramp: Vec<f64> = t.samples().iter().skip(12).map(|s| s.mbps).collect();
        let above = after_ramp.iter().filter(|m| **m > TARGET).count();
        let below = after_ramp.iter().filter(|m| **m < TARGET).count();
        assert!(
            above >= 8 && below >= 8,
            "noise is one-sided: {above} of {} steps above {TARGET} Mbps, {below} below",
            after_ramp.len()
        );
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
    fn a_run_driven_by_the_clock_completes() {
        let mut ui = SpeedTestUI::new();
        run_a_full_test(&mut ui);
        assert!(ui.phase().is_complete());
        assert_eq!(ui.history().len(), 1);
    }

    #[test]
    fn a_completed_run_has_results() {
        let mut ui = SpeedTestUI::new();
        run_a_full_test(&mut ui);
        let result = ui.history().latest().unwrap();
        assert!(result.download_mbps > 0.0);
        assert!(result.upload_mbps > 0.0);
        assert!(result.latency_ms > 0.0);
    }

    // ====================================================================
    // The clock
    //
    // `handle_event` matched `Event::Key` and `Event::Mouse` and dropped
    // everything else, so a speed test -- a measurement over time -- had no
    // source of time. Start ran the whole thing inside one call instead.
    // These tests all go in through `handle_event`, because that is the only
    // way to tell a wired app from an unwired one: the phase machine below
    // was correct the whole time, and nothing but a test ever reached it.
    // ====================================================================

    /// The event that makes the app work.
    #[test]
    fn a_tick_event_advances_a_running_test() {
        let mut ui = SpeedTestUI::new();
        press(&mut ui, Key::Enter);
        assert_eq!(ui.latency_tester.probes_sent, 0);

        for _ in 0..20 {
            ui.handle_event(&Event::Tick { elapsed_ms: 50 });
        }

        assert!(
            ui.latency_tester.probes_sent > 0,
            "a second of ticks sent no probes"
        );
    }

    /// An idle window does not claim the clock.
    #[test]
    fn a_tick_while_idle_is_ignored() {
        let mut ui = SpeedTestUI::new();
        let res = ui.handle_event(&Event::Tick { elapsed_ms: 100 });
        assert_eq!(res, EventResult::Ignored);
        assert!(ui.phase().is_idle());
        assert_eq!(ui.latency_tester.probes_sent, 0);
    }

    /// Every phase gets frames of its own.
    ///
    /// The old Start assigned all three `Testing` phases inside one call, so
    /// the phase strip could only ever be drawn in `Idle` or `Complete` --
    /// its "currently running" highlight was unreachable code.
    #[test]
    fn the_run_is_drawn_in_every_phase_it_passes_through() {
        let mut ui = SpeedTestUI::new();
        press(&mut ui, Key::Enter);

        let mut seen = Vec::new();
        for _ in 0..FRAME_BUDGET {
            let now = ui.phase().clone();
            if seen.last() != Some(&now) {
                seen.push(now);
            }
            if ui.phase().is_complete() {
                break;
            }
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
        }

        assert_eq!(
            seen,
            vec![
                SpeedTestPhase::Testing(TestKind::Latency),
                SpeedTestPhase::Testing(TestKind::Download),
                SpeedTestPhase::Testing(TestKind::Upload),
                SpeedTestPhase::Complete,
            ]
        );
    }

    /// The graph fills in over the run rather than arriving complete.
    #[test]
    fn the_graph_grows_while_the_download_runs() {
        let mut ui = SpeedTestUI::new();
        press(&mut ui, Key::Enter);

        // Past the latency phase and a little way into the download.
        let mut early = 0;
        for _ in 0..FRAME_BUDGET {
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
            if *ui.phase() == SpeedTestPhase::Testing(TestKind::Download) {
                early = ui.graph_points.len();
                if early > 0 {
                    break;
                }
            }
        }
        assert!(early > 0, "the download phase drew no graph points");

        for _ in 0..100 {
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
        }
        assert!(
            ui.graph_points.len() > early,
            "the graph stopped growing at {early} points"
        );
    }

    /// The probe rate is the clock's, not the frame rate's.
    ///
    /// Two runs over the same 750 ms of wall clock, at 20 Hz and at 40 Hz.
    /// Both frame intervals are shorter than a probe interval, so an
    /// implementation that probed once per frame that crossed the interval
    /// and threw away the remainder would send no probes at all in either;
    /// one that probed at most once per frame would disagree between them.
    #[test]
    fn the_probe_rate_does_not_follow_the_frame_rate() {
        fn probes_after(frame_ms: u64, frames: usize) -> u32 {
            let mut ui = SpeedTestUI::new();
            press(&mut ui, Key::Enter);
            for _ in 0..frames {
                ui.handle_event(&Event::Tick {
                    elapsed_ms: frame_ms,
                });
            }
            ui.latency_tester.probes_sent
        }

        let slow = probes_after(50, 15);
        let fast = probes_after(25, 30);
        assert_eq!(slow, fast, "{slow} probes at 20 Hz but {fast} at 40 Hz");
        assert_eq!(slow, 7, "750 ms should pay for seven 100 ms probes");
    }

    /// Escape gets to cancel, now that there is a running test to cancel.
    ///
    /// The branch is guarded on `phase.is_testing()`, which was false at
    /// every moment a key could be pressed.
    #[test]
    fn escape_cancels_a_running_test() {
        let mut ui = SpeedTestUI::new();
        press(&mut ui, Key::Enter);
        ui.handle_event(&Event::Tick { elapsed_ms: 500 });
        assert!(ui.phase().is_testing());

        assert_eq!(press(&mut ui, Key::Escape), EventResult::Consumed);
        assert!(ui.phase().is_idle());

        // And a cancelled run records nothing.
        for _ in 0..FRAME_BUDGET {
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
        }
        assert!(ui.phase().is_idle());
        assert_eq!(ui.history().len(), 0);
    }

    /// A run takes about as long as it says it will.
    ///
    /// Two 10-second throughput phases plus 20 probes 100 ms apart is 22
    /// seconds. The tolerance is a frame either way; the point is that the
    /// duration is honoured at all, which it was not when the whole run
    /// happened inside one call.
    #[test]
    fn a_run_lasts_the_configured_duration() {
        let mut ui = SpeedTestUI::new();
        press(&mut ui, Key::Enter);
        let mut frames = 0u64;
        for _ in 0..FRAME_BUDGET {
            if ui.phase().is_complete() {
                break;
            }
            ui.handle_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
            frames += 1;
        }
        let secs = (frames * FRAME_MS) as f64 / 1000.0;
        let expected = 2.0 * f64::from(ui.config.test_duration_secs)
            + f64::from(ui.latency_tester.probe_count) * f64::from(LATENCY_PROBE_INTERVAL_SECS);
        assert!(
            (secs - expected).abs() < 0.5,
            "a {expected} s test took {secs} s"
        );
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
        run_a_full_test(&mut tested_ui);
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

    // --- Report forgery tests ---

    /// Server names shaped to redraw the report they are written into.
    const HOSTILE_SERVERS: &[&str] = &[
        "\n--- Speed Test Result ---",
        "Metro East\nDownload:     9999.00 Mbps",
        "\n--- Summary ---\n",
        "a\r\nb",
        "tab\there",
    ];

    /// Lines that look like one of the report's own section headers.
    ///
    /// Positional, not textual, on purpose: a correctly folded name may still
    /// *contain* the text `--- Summary ---` in the middle of its line, which
    /// is harmless. What must never happen is a line that *is* a header. A
    /// `contains` assertion here would fail against correct output.
    fn header_lines(report: &str) -> Vec<&str> {
        report
            .lines()
            .filter(|l| l.starts_with("--- ") && l.ends_with(" ---"))
            .collect()
    }

    fn result_named(name: &str) -> SpeedTestResult {
        let mut r = make_result(100.0, 50.0, 10.0);
        r.server_name = name.into();
        r
    }

    #[test]
    fn a_server_name_cannot_forge_a_report_section() {
        for &name in HOSTILE_SERVERS {
            let report = result_named(name).to_text_report();
            assert_eq!(
                header_lines(&report),
                vec!["--- Speed Test Result ---"],
                "server name {name:?} changed the report's section structure",
            );
        }
    }

    #[test]
    fn a_server_name_stays_on_its_own_field_line() {
        // A result report has exactly seven labelled fields under one header.
        for &name in HOSTILE_SERVERS {
            let report = result_named(name).to_text_report();
            assert_eq!(
                report.lines().count(),
                8,
                "server name {name:?} spilled onto extra lines: {report:?}",
            );
        }
    }

    #[test]
    fn a_hostile_server_name_cannot_forge_a_history_header() {
        let mut h = SpeedTestHistory::new(HOSTILE_SERVERS.len());
        for &name in HOSTILE_SERVERS {
            h.push(result_named(name));
        }
        let text = h.export_as_text();
        let headers = header_lines(&text);
        assert_eq!(
            headers.iter().filter(|l| **l == "--- Summary ---").count(),
            1,
            "a server name forged a Summary section: {headers:?}",
        );
        assert_eq!(
            headers
                .iter()
                .filter(|l| **l == "--- Speed Test Result ---")
                .count(),
            HOSTILE_SERVERS.len(),
            "one header per recorded result, no more: {headers:?}",
        );
    }

    #[test]
    fn an_ordinary_server_name_is_reported_verbatim() {
        let report = result_named("Metro East").to_text_report();
        assert!(
            report.contains("Server:       Metro East\n"),
            "folding altered a name that needed no folding: {report:?}",
        );
    }

    // ====================================================================
    // History list: layout, hit test and wheel
    //
    // The regression suite for a list whose renderer and hit test each had
    // their own copy of the layout arithmetic. Every position below is read
    // out of `render()`'s own commands rather than from the layout helpers,
    // because a test that asks the layout where a row is cannot catch the
    // drawing and the clicking disagreeing -- both would be wrong together
    // and the test would still pass. That is exactly how the mirrored
    // highlight survived a test suite for as long as it did.
    // ====================================================================

    /// The layout at the default window size, which is the size every test
    /// here runs at. A function rather than a `const`, because the geometry is
    /// now *derived* from the window size instead of being a table of literals
    /// -- which is the whole point of `Layout`, and the reason a test can no
    /// longer name a position without saying which window it means.
    fn layout() -> Layout {
        Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    /// An x well inside the history panel.
    fn hist_x() -> f32 {
        layout().history.x + 20.0
    }

    /// An app holding `n` results, each identifiable by its download speed
    /// and therefore by the summary line drawn for it.
    fn ui_with_history(n: usize) -> SpeedTestUI {
        let mut ui = SpeedTestUI::new();
        for i in 0..n {
            ui.history.push(make_result(i as f64, 50.0, 10.0));
        }
        ui
    }

    /// Where the renderer actually painted each history row, read out of the
    /// render commands: `(y of the row's top, the summary line drawn there)`.
    fn painted_rows(ui: &SpeedTestUI) -> Vec<(f32, String)> {
        ui.render()
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    ..
                } if *x == layout().history.x + 12.0 && *font_size == 10.0 => {
                    Some((*y - 6.0, text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Where the renderer painted the hover highlight, if it painted one.
    fn painted_highlight(ui: &SpeedTestUI) -> Option<f32> {
        ui.render().commands.iter().find_map(|cmd| match cmd {
            RenderCommand::FillRect { x, y, height, .. }
                if *x == layout().history.x + 4.0 && *height == HISTORY_ROW_HEIGHT =>
            {
                Some(*y)
            }
            _ => None,
        })
    }

    fn hover(ui: &mut SpeedTestUI, y: f32) {
        ui.handle_event(&Event::Mouse(MouseEvent {
            x: hist_x(),
            y,
            kind: MouseEventKind::Move,
        }));
    }

    fn wheel_over_history(ui: &mut SpeedTestUI, dy: f32) -> EventResult {
        ui.handle_event(&Event::Mouse(MouseEvent {
            x: hist_x(),
            y: layout().history_list.y + 20.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        }))
    }

    /// The regression test for a list drawn upside down relative to the way
    /// it was pointed at.
    ///
    /// The renderer reversed the history so the newest result was on top; the
    /// hit test stored the raw draw position, which the renderer then compared
    /// against a *history* index. Pointing at the top row therefore lit up the
    /// bottom one. Only the middle row of an odd-length list highlighted
    /// itself, which is why a spot check could miss it entirely.
    #[test]
    fn the_highlight_lands_on_the_row_the_pointer_is_over() {
        let mut ui = ui_with_history(5);
        let rows = painted_rows(&ui);
        assert_eq!(rows.len(), 5, "all five rows should fit");

        for (top, _) in rows {
            hover(&mut ui, top + HISTORY_ROW_HEIGHT / 2.0);
            assert_eq!(
                painted_highlight(&ui),
                Some(top),
                "hovering the row drawn at y={top} highlighted a different row",
            );
        }
    }

    /// Clicking must name the same row as hovering, and the row it draws.
    #[test]
    fn clicking_a_row_highlights_the_row_it_is_drawn_in() {
        let mut ui = ui_with_history(5);
        for (top, _) in painted_rows(&ui) {
            let y = top + HISTORY_ROW_HEIGHT / 2.0;
            ui.history_hover = None;
            assert_eq!(
                ui.handle_event(&Event::Mouse(MouseEvent {
                    x: hist_x(),
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })),
                EventResult::Consumed,
            );
            assert_eq!(painted_highlight(&ui), Some(top));
        }
    }

    /// A row's hit rectangle is the whole of the rectangle it is painted in
    /// and nothing beyond it. Probing the edges is what makes the hit test
    /// independent of the renderer; probing centres alone passes just as
    /// happily when both regions are shifted together.
    #[test]
    fn a_rows_hit_rectangle_is_exactly_the_rectangle_it_is_painted_in() {
        let mut ui = ui_with_history(5);
        for (top, _) in painted_rows(&ui) {
            hover(&mut ui, top);
            let at_top = ui.history_hover;
            hover(&mut ui, top + HISTORY_ROW_HEIGHT / 2.0);
            let middle = ui.history_hover;
            hover(&mut ui, top + HISTORY_ROW_HEIGHT - 0.5);
            let at_bottom = ui.history_hover;
            assert!(middle.is_some(), "nothing painted at y={top} is hoverable");
            assert_eq!(
                at_top, middle,
                "the row's top edge is not where it is drawn"
            );
            assert_eq!(
                at_bottom, middle,
                "the row's bottom edge is not where it is drawn"
            );
        }
    }

    /// The newest result goes on top -- the ordering the renderer's `rev()`
    /// was there to produce, now stated where a change to `history_rows` has
    /// to answer for it.
    #[test]
    fn the_newest_result_is_drawn_in_the_top_row() {
        let ui = ui_with_history(4);
        let rows = painted_rows(&ui);
        assert_eq!(rows[0].1, make_result(3.0, 50.0, 10.0).summary_line());
        assert_eq!(rows[3].1, make_result(0.0, 50.0, 10.0).summary_line());
    }

    /// The stats strip at the foot of the panel is opaque and painted over
    /// the list. A row underneath it is drawn nowhere, so it must not be
    /// hoverable -- the hit test used the panel's full height and happily
    /// selected an invisible result when the pointer was on "avg 84.2 Mbps".
    #[test]
    fn a_row_under_the_stats_strip_cannot_be_pointed_at() {
        let mut ui = ui_with_history(MAX_HISTORY);
        let strip_top = layout().history_list.bottom();
        // Inside the panel, below the visible list, above the panel's foot.
        hover(&mut ui, strip_top + HISTORY_STATS_HEIGHT / 2.0);
        assert_eq!(ui.history_hover, None);
        // No row begins under the strip either. A row *straddling* the edge is
        // fine -- the clip below cuts it -- but one starting past it would be
        // drawn nowhere at all.
        assert!(
            painted_rows(&ui).iter().all(|&(top, _)| top < strip_top),
            "a row was drawn entirely under the stats strip",
        );
    }

    /// The clip the renderer pushes and the bound the hit test enforces must
    /// be the same rectangle.
    ///
    /// This is the invariant the whole refactor rests on, stated directly:
    /// they were two separate expressions before, and the hit test's ran to
    /// the foot of the panel while the renderer's stopped at the stats strip.
    /// Reading the clip out of the render commands is what keeps the two
    /// honest, since neither test nor hit test may consult the other's copy.
    #[test]
    fn the_lists_clip_is_the_region_the_hit_test_accepts() {
        let ui = ui_with_history(MAX_HISTORY);
        let clip = ui
            .render()
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::PushClip { x, y, height, .. } if *x == layout().history.x => {
                    Some((*y, *height))
                }
                _ => None,
            })
            .expect("the history list is drawn under a clip");
        assert_eq!(clip.0, layout().history_list.y);
        assert_eq!(clip.1, layout().history_list.h);

        // And the hit test agrees at both edges of that rectangle.
        let mut ui = ui;
        hover(&mut ui, clip.0);
        assert!(ui.history_hover.is_some(), "the clip's top edge is dead");
        hover(&mut ui, clip.0 + clip.1 - 0.5);
        assert!(ui.history_hover.is_some(), "the clip's bottom edge is dead");
        hover(&mut ui, clip.0 + clip.1);
        assert_eq!(ui.history_hover, None, "the hit test runs past the clip");
    }

    /// `MouseEventKind::Scroll` carries notches. Three rows a notch is the
    /// toolkit's shared convention; a handler that treated `dy` as a pixel
    /// count would move a fortieth of this.
    #[test]
    fn one_wheel_notch_moves_three_history_rows() {
        let mut ui = ui_with_history(MAX_HISTORY);
        assert_eq!(wheel_over_history(&mut ui, -1.0), EventResult::Consumed);
        assert_eq!(ui.history_scroll, 3.0 * HISTORY_ROW_HEIGHT);
    }

    /// A trackpad sends fractions of a notch, and the offset is an `f32`, so
    /// they move the list now rather than being banked or truncated away.
    #[test]
    fn a_fraction_of_a_notch_moves_the_history_now() {
        let mut ui = ui_with_history(MAX_HISTORY);
        wheel_over_history(&mut ui, -0.2);
        assert!(ui.history_scroll > 0.0 && ui.history_scroll < HISTORY_ROW_HEIGHT);
    }

    /// The wheel stops with the last row on screen. `history_scroll` had no
    /// upper bound because nothing knew the list's height -- and no wheel
    /// handler at all, so twelve of twenty results were unreachable.
    #[test]
    fn the_wheel_stops_with_the_last_row_on_screen() {
        let mut ui = ui_with_history(MAX_HISTORY);
        for _ in 0..50 {
            wheel_over_history(&mut ui, -1.0);
        }
        assert_eq!(ui.history_scroll, ui.max_history_scroll());
        assert!(ui.max_history_scroll() > 0.0, "this list should scroll");

        let rows = painted_rows(&ui);
        let last = rows.last().expect("some rows are drawn");
        assert_eq!(last.1, make_result(0.0, 50.0, 10.0).summary_line());
        assert!(
            last.0 + HISTORY_ROW_HEIGHT <= list_bottom() + 0.01,
            "the oldest row hangs past the bottom of the viewport",
        );
    }

    /// The lowest y the list is visible at -- the top of the stats strip.
    fn list_bottom() -> f32 {
        layout().history_list.bottom()
    }

    /// A list shorter than its viewport does not scroll at all.
    #[test]
    fn a_short_history_does_not_scroll() {
        let mut ui = ui_with_history(3);
        assert_eq!(ui.max_history_scroll(), 0.0);
        wheel_over_history(&mut ui, -5.0);
        assert_eq!(ui.history_scroll, 0.0);
    }

    /// The wheel elsewhere in the window leaves the history where it is.
    #[test]
    fn scrolling_outside_the_panel_leaves_the_history_alone() {
        let mut ui = ui_with_history(MAX_HISTORY);
        let before = ui.history_scroll;
        let l = layout();
        let res = ui.handle_event(&Event::Mouse(MouseEvent {
            x: l.gauge_centre.0,
            y: l.gauge_centre.1,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
        }));
        assert_eq!(res, EventResult::Ignored);
        assert_eq!(ui.history_scroll, before);
    }

    /// A non-finite delta must not poison the offset -- once it is NaN every
    /// later comparison is false and the list never moves again.
    #[test]
    fn a_nonfinite_delta_does_not_freeze_the_history() {
        let mut ui = ui_with_history(MAX_HISTORY);
        wheel_over_history(&mut ui, f32::NAN);
        wheel_over_history(&mut ui, f32::INFINITY);
        assert!(ui.history_scroll.is_finite());
        wheel_over_history(&mut ui, -1.0);
        assert_eq!(ui.history_scroll, 3.0 * HISTORY_ROW_HEIGHT);
    }

    /// Scrolled to the end, a new result both lengthens the list and evicts
    /// the oldest once it is full -- so the offset has to be re-checked, or
    /// the view is left parked past the end of a list that no longer reaches.
    #[test]
    fn recording_a_result_pulls_a_scrolled_view_back_inside() {
        let mut ui = ui_with_history(MAX_HISTORY);
        for _ in 0..50 {
            wheel_over_history(&mut ui, -1.0);
        }
        let at_end = ui.history_scroll;
        ui.history.push(make_result(99.0, 50.0, 10.0));
        ui.clamp_history_scroll();
        assert!(ui.history_scroll <= ui.max_history_scroll());
        assert_eq!(ui.history_scroll, at_end, "a full list's height is fixed");
    }

    // ====================================================================
    // The window: layout, resize, modality and the clock
    //
    // Everything below drives the app the way `oswindow` does -- through
    // `App::on_event`, `App::render` and `Probe`'s hit test -- rather than
    // through its internals. Before this file had a `main`, none of that
    // existed: every control was drawn at a literal coordinate, and a test
    // could only ask the same literal back. That is how the whole result
    // readout came to be painted underneath the graph panel every frame and
    // seen in none of them.
    // ====================================================================

    /// The controls that are drawn in every state of the app, so a test may
    /// expect a hit box for each without setting anything up first.
    const ALWAYS_DRAWN: [Target; 4] = [
        Target::ServerButton,
        Target::Start,
        Target::Export,
        Target::HistoryPanel,
    ];

    /// Where the renderer put a given piece of text, if it put it anywhere.
    fn text_at(ui: &SpeedTestUI, needle: &str) -> Option<(f32, f32)> {
        ui.render().commands.iter().find_map(|cmd| match cmd {
            RenderCommand::Text { x, y, text, .. } if text == needle => Some((*x, *y)),
            _ => None,
        })
    }

    /// The regression test for a readout that was painted and then covered.
    ///
    /// `render` drew the Start button at y 410..446 and the whole
    /// download/upload/latency strip at y 455 and below -- and *then* filled
    /// the graph panel over x 40..560, y 420..600 with an opaque rectangle.
    /// The numbers this app exists to produce were painted every frame and
    /// visible in none of them, and the Start button was two-thirds buried.
    /// Nothing caught it because each half drew exactly where its own
    /// constants said it should; only their relationship was wrong, and no
    /// one had written the relationship down.
    #[test]
    fn the_start_button_and_the_readout_do_not_share_space_with_the_panels() {
        let l = layout();
        for (name, r) in [
            ("the Start button", l.start),
            ("the result strip", l.summary),
        ] {
            assert!(
                r.intersect(l.graph).is_none(),
                "{name} overlaps the graph panel, which is painted over it",
            );
            assert!(
                r.intersect(l.history).is_none(),
                "{name} overlaps the history panel, which is painted over it",
            );
        }
    }

    /// And the same thing said about the pixels rather than the rectangles:
    /// after a real run, the three headings are drawn clear of the band the
    /// two opaque panels occupy.
    #[test]
    fn a_finished_runs_numbers_are_painted_where_nothing_covers_them() {
        let mut ui = SpeedTestUI::with_seed(11);
        run_a_full_test(&mut ui);
        let l = layout();
        for heading in ["Download", "Upload", "Latency / Jitter"] {
            let (x, y) =
                text_at(&ui, heading).unwrap_or_else(|| panic!("{heading} is not drawn at all"));
            assert!(
                y >= 0.0 && y < l.graph.y,
                "{heading} is drawn at y={y}, inside the panel band that starts at {}",
                l.graph.y,
            );
            assert!(
                x >= 0.0 && x < l.width,
                "{heading} is drawn at x={x}, outside a {} px window",
                l.width,
            );
        }
    }

    /// Every control answers a click in the middle of where it is drawn. This
    /// is the property the hit test used to have its own second copy of.
    #[test]
    fn every_control_answers_where_the_frame_draws_it() {
        let ui = SpeedTestUI::new();
        for target in ALWAYS_DRAWN {
            let r = probe::rect_of(&ui, target)
                .unwrap_or_else(|| panic!("{target:?} records no hit box"));
            let (cx, cy) = r.centre();
            assert_eq!(
                ui.target_at(cx, cy),
                Some(target),
                "{target:?} is drawn at {r:?} but clicking its middle does not reach it",
            );
        }
    }

    /// A control laid out past the window's edge would keep its hit box and so
    /// stay clickable while being invisible -- which is why `Layout` shrinks
    /// rather than clamping. Stated here over a spread of sizes, including
    /// ones far smaller than the app's controls.
    #[test]
    fn no_size_puts_a_hit_box_outside_the_window() {
        for size in [
            (900.0, 720.0),
            (1600.0, 1000.0),
            (640.0, 480.0),
            (420.0, 360.0),
            (240.0, 200.0),
            (1.0, 1.0),
        ] {
            let ui = ui_with_history(MAX_HISTORY);
            for (target, r) in ui.frame(size.0, size.1).hits() {
                assert!(
                    r.x >= 0.0
                        && r.y >= 0.0
                        && r.right() <= size.0 + 0.01
                        && r.bottom() <= size.1 + 0.01,
                    "at {size:?} the hit box for {target:?} is {r:?}, outside the window",
                );
            }
        }
    }

    /// Resizing relays the window out: the controls move, and the hit test
    /// moves with them. Before `Event::Resize` was handled at all, `self.width`
    /// was written once at construction, so a widened window painted a wider
    /// background around a picture of a 900x720 one.
    #[test]
    fn a_resized_window_lays_out_again_and_the_hit_test_follows() {
        let size = (1280.0, 820.0);
        let mut ui = SpeedTestUI::new();
        let before = probe::rect_of(&ui, Target::Start).expect("the Start button is drawn");

        assert_eq!(
            ui.handle_event(&Event::Resize {
                width: size.0 as u32,
                height: size.1 as u32,
            }),
            EventResult::Consumed,
        );

        let after = probe::rect_of_sized(&ui, Target::Start, size).expect("still drawn");
        assert_ne!(
            before, after,
            "the Start button did not move with the window"
        );
        assert!(
            (after.centre().0 - size.0 / 2.0).abs() < 1.0,
            "the Start button is not centred in the window it was drawn for: {after:?}",
        );
        let (cx, cy) = after.centre();
        assert_eq!(ui.target_at(cx, cy), Some(Target::Start));
    }

    /// `App::render` must believe the size it is handed rather than a size it
    /// remembers, because the compositor can hand it one it has never seen an
    /// `Event::Resize` for.
    #[test]
    fn render_lays_out_at_the_size_it_is_handed() {
        let mut ui = SpeedTestUI::new();
        let tree = App::render(&mut ui, 1024.0, 680.0);
        let covered = tree.commands.iter().any(|cmd| {
            matches!(
                cmd,
                RenderCommand::FillRect { width, height, .. }
                    if *width == 1024.0 && *height == 680.0
            )
        });
        assert!(
            covered,
            "the background does not fill the window it was given"
        );
        let r = probe::rect_of_sized(&ui, Target::Start, (1024.0, 680.0)).expect("drawn");
        assert!((r.centre().0 - 512.0).abs() < 1.0, "{r:?} is not centred");
    }

    /// An open menu is modal: what it covers keeps its pixels and loses its
    /// clicks. Without the scrim the Start button behind the list still
    /// worked, so the menu only *looked* in front of it.
    #[test]
    fn an_open_server_list_takes_the_clicks_of_what_it_covers() {
        let mut ui = SpeedTestUI::new();
        let start = probe::rect_of(&ui, Target::Start).expect("the Start button is drawn");

        assert_eq!(
            probe::click(&mut ui, Target::ServerButton),
            EventResult::Consumed
        );
        assert!(ui.server_dropdown_open, "the picker did not open the list");

        let (cx, cy) = start.centre();
        assert_eq!(
            ui.target_at(cx, cy),
            Some(Target::DropdownScrim),
            "the Start button is still reachable underneath an open menu",
        );
        assert_eq!(ui.handle_click(cx, cy), EventResult::Consumed);
        assert!(!ui.server_dropdown_open, "the click did not shut the menu");
        assert!(
            ui.phase().is_idle(),
            "the click started a test through the menu covering the button",
        );
    }

    /// Choosing from the list selects that server and shuts the list.
    #[test]
    fn choosing_a_server_selects_it_and_shuts_the_list() {
        let mut ui = SpeedTestUI::new();
        let last = ui.servers.len() - 1;
        probe::click(&mut ui, Target::ServerButton);
        assert_eq!(
            probe::click(&mut ui, Target::ServerItem(last)),
            EventResult::Consumed,
        );
        assert_eq!(ui.selected_server, last);
        assert!(!ui.server_dropdown_open);
        assert_eq!(
            ui.config.server_url, ui.servers[last].url,
            "selecting a server did not point the config at it",
        );
    }

    /// The list's items only exist while it is open -- the closed picker must
    /// not leave nine invisible menu rows lying across the window.
    #[test]
    fn a_shut_server_list_has_no_items_to_click() {
        let ui = SpeedTestUI::new();
        assert_eq!(probe::rect_of(&ui, Target::ServerItem(0)), None);
        assert_eq!(probe::rect_of(&ui, Target::DropdownScrim), None);
    }

    /// The wheel works anywhere over the panel -- header and stats strip
    /// included -- while a *click* there still selects nothing, because no row
    /// is drawn there. Those are two different questions about the same pixel,
    /// and the frame answers both from one recording.
    #[test]
    fn the_wheel_works_over_the_panels_furniture_where_a_click_selects_nothing() {
        let l = layout();
        let dead_spots = [
            ("the header", l.history.y + HISTORY_HEADER_HEIGHT / 2.0),
            (
                "the stats strip",
                l.history_list.bottom() + HISTORY_STATS_HEIGHT / 2.0,
            ),
        ];
        for (name, y) in dead_spots {
            let mut ui = ui_with_history(MAX_HISTORY);
            let x = l.history.centre().0;
            assert_eq!(
                ui.target_at(x, y),
                Some(Target::HistoryPanel),
                "{name} is not part of the panel",
            );
            assert_eq!(
                ui.handle_click(x, y),
                EventResult::Ignored,
                "clicking {name} did something",
            );
            assert_eq!(ui.history_hover, None, "clicking {name} selected a row");
            assert_eq!(
                ui.handle_event(&Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
                })),
                EventResult::Consumed,
                "the wheel does not work over {name}",
            );
            assert_eq!(ui.history_scroll, 3.0 * HISTORY_ROW_HEIGHT);
        }
    }

    /// The clock is asked for only while there is something to measure. A
    /// window showing a finished result costs the compositor nothing.
    #[test]
    fn the_clock_is_only_armed_while_a_test_runs() {
        let mut ui = SpeedTestUI::with_seed(5);
        assert_eq!(ui.tick_interval(), None, "an idle window wants the clock");

        press(&mut ui, Key::Enter);
        assert!(ui.phase().is_testing());
        let interval = ui.tick_interval().expect("a running test needs the clock");
        assert!(
            interval <= Duration::from_millis(20),
            "a sweeping dial cannot be animated at {interval:?}",
        );

        for _ in 0..FRAME_BUDGET {
            if ui.phase().is_complete() {
                break;
            }
            ui.on_event(&Event::Tick {
                elapsed_ms: FRAME_MS,
            });
        }
        assert!(ui.phase().is_complete(), "the run never finished");
        assert_eq!(
            ui.tick_interval(),
            None,
            "a finished window keeps the clock"
        );
    }

    /// A tick is only worth a frame while it changes something.
    #[test]
    fn an_idle_tick_does_not_ask_for_a_redraw() {
        let mut ui = SpeedTestUI::new();
        assert!(matches!(
            ui.on_event(&Event::Tick {
                elapsed_ms: FRAME_MS
            }),
            Response::Idle,
        ));
        press(&mut ui, Key::Enter);
        assert!(matches!(
            ui.on_event(&Event::Tick {
                elapsed_ms: FRAME_MS
            }),
            Response::Redraw,
        ));
    }

    /// Pointer motion over dead space costs no frame; motion that lights a
    /// button costs exactly one.
    #[test]
    fn only_a_move_that_changes_something_asks_for_a_frame() {
        let mut ui = SpeedTestUI::new();
        let (bx, by) = probe::bare_point(&ui, SpeedTestUI::SIZE).expect("some background is bare");
        let mv = |x: f32, y: f32| {
            Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            })
        };
        assert!(matches!(ui.on_event(&mv(bx, by)), Response::Idle));

        let (sx, sy) = probe::rect_of(&ui, Target::Start).expect("drawn").centre();
        assert!(
            matches!(ui.on_event(&mv(sx, sy)), Response::Redraw),
            "moving onto the Start button did not light it",
        );
        assert!(
            matches!(ui.on_event(&mv(sx, sy)), Response::Idle),
            "a second move onto an already-lit button asked for a frame",
        );
    }

    /// Ctrl+Q and the close button end the app. Escape does not -- it cancels
    /// a running measurement, which is the more useful answer to the key a
    /// user reaches for ten seconds into one.
    #[test]
    fn ctrl_q_closes_the_window_and_escape_cancels_the_run() {
        let mut ui = SpeedTestUI::new();
        assert!(matches!(
            ui.on_event(&Event::Key(probe::ctrl(Key::Q))),
            Response::Exit,
        ));
        assert!(matches!(
            ui.on_event(&Event::CloseRequested),
            Response::Exit,
        ));

        press(&mut ui, Key::Enter);
        assert!(ui.phase().is_testing());
        assert!(
            !matches!(
                ui.on_event(&Event::Key(probe::press(Key::Escape))),
                Response::Exit,
            ),
            "Escape closed the window",
        );
        assert!(ui.phase().is_idle(), "Escape did not cancel the run");
    }

    /// Every recorded target is one the click handler answers. A hit box with
    /// no handler is a control that swallows a click and does nothing.
    #[test]
    fn every_target_the_frame_records_is_one_the_app_handles() {
        let mut ui = ui_with_history(3);
        probe::click(&mut ui, Target::ServerButton);
        let recorded: Vec<Target> = ui
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .hits()
            .iter()
            .map(|(t, _)| *t)
            .collect();
        assert!(
            recorded.contains(&Target::DropdownScrim) && recorded.contains(&Target::ServerItem(0)),
            "an open list records neither its scrim nor its items: {recorded:?}",
        );
        // Closed, the same walk records the ordinary controls instead.
        probe::key(&mut ui, &probe::press(Key::Escape));
        let names = probe::control_names(&ui);
        for target in ALWAYS_DRAWN {
            let name = probe::variant_name(target);
            assert!(
                names.iter().any(|n| n.starts_with(&name)),
                "{name} is missing from {names:?}",
            );
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
