//! A system monitor's graphs: the last minute of each reading that changes
//! from second to second -- the processor, memory, the disks' activity and
//! the network -- sampled by the shell and drawn when the monitor is given
//! the room (two rows of the widget grid; its menu's "Size" offers it).
//!
//! These were the resource monitor's (`resmon.rs`), a panel of four graphs
//! nothing ever showed. The operator's rule for dead code with a live twin
//! (`design-decisions.md` §1423) is that the twin takes what only the dead
//! copy could do, and then the copy goes: the system monitor widget took
//! the graphs -- each with what it reads now and the most it read in the
//! minute -- and `resmon.rs` is gone (§1496).
//!
//! # Where the readings come from
//!
//! | graph | source |
//! |---|---|
//! | processor | `/proc/stat`, two reads differenced (`SystemSample::cpu_busy_fraction`) |
//! | memory | `/proc/meminfo`'s `MemAvailable` (`SystemSample::memory_used_fraction`) |
//! | disk activity | `/proc/diskstats`' time spent doing I/O, the busiest disk's ([`disk_busy_fraction`]) |
//! | network | `/proc/net/dev`'s bytes, every interface but the loopback ([`network_rate`]) |
//!
//! SlateOS serves the first two. Its `/proc/diskstats` is a table of its own
//! with no counters, and it has no `/proc/net/dev`, so there those two graphs
//! say they are not measured -- read through `procinfo` as on any Linux, so
//! they measure the day the kernel serves Linux's counters
//! (`requests/c-a-the-system-monitor-reads-disk-and-network-counters-the-kernel-does-not-serve.md`).
//!
//! # One layout
//!
//! [`panels`] places the graphs, and the drawing ([`render`]) and what tools
//! are told (`widgets::accessible`) both read it: a box drawn in one place
//! and reported in another is the defect
//! `C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY` names.

use appearance::Palette;
use guitk::color::Color;
use guitk::history::SampleHistory;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use procinfo::{DiskStats, NetDevice};

use crate::Rect;

/// How many samples a graph keeps: a minute, at the monitor's one a second.
pub const HISTORY_SAMPLES: usize = 60;

/// The least time between two samples the monitor takes.
///
/// The shell samples when a widget falls due -- a clock every second, a
/// photo frame every thirty -- so two widgets due a moment apart would give
/// two samples a moment apart: a graph's minute crowded at one end, and a
/// processor reading over an interval too short to say anything.
pub const MIN_SAMPLE_SPACING_MS: u64 = 500;

/// What a graph's heading says when nothing has measured it -- as a meter's
/// does (`widgets::NOT_MEASURED`).
pub const NOT_MEASURED: &str = "not measured";

/// One of a system monitor's graphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Graph {
    /// Processor use.
    Cpu,
    /// Memory in use.
    Memory,
    /// How much of the time the busiest disk spent working.
    DiskActivity,
    /// Bytes in and out over every network interface but the loopback.
    Network,
}

impl Graph {
    /// Every graph, top to bottom, as the monitor draws them.
    pub const ALL: [Self; 4] = [Self::Cpu, Self::Memory, Self::DiskActivity, Self::Network];

    /// What its heading calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::DiskActivity => "Disk activity",
            Self::Network => "Network",
        }
    }

    /// The hue its line is drawn in: one for each measurement, and none of
    /// them the accent -- the accent marks what the user chose, and this is
    /// what was measured. The processor and memory as their meters are;
    /// the disks' activity apart from the disk's space (the meter's peach),
    /// which is another measurement; the network as the resource monitor
    /// drew it.
    #[must_use]
    pub const fn hue(self, p: &Palette) -> Color {
        match self {
            Self::Cpu => p.blue,
            Self::Memory => p.green,
            Self::DiskActivity => p.teal,
            Self::Network => p.mauve,
        }
    }
}

/// A network's rate, in bytes a second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetRate {
    /// Received.
    pub down: u64,
    /// Sent.
    pub up: u64,
}

impl NetRate {
    /// In and out together: what the network graph plots.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.down.saturating_add(self.up)
    }
}

/// The graphs as a frame is given them: each reading's last minute, oldest
/// first -- an empty one is a reading nothing has measured -- and the
/// network's last rate, in and out apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graphs {
    /// Processor use, 0.0 to 1.0.
    pub cpu: Vec<f32>,
    /// Memory in use, 0.0 to 1.0.
    pub memory: Vec<f32>,
    /// The busiest disk's share of the time spent working, 0.0 to 1.0.
    pub disk_activity: Vec<f32>,
    /// Bytes a second, in and out together.
    pub network: Vec<f32>,
    /// The last rate, in and out apart; `None` while nothing has measured it.
    pub network_now: Option<NetRate>,
}

impl Graphs {
    /// `graph`'s last minute, oldest first.
    #[must_use]
    pub fn samples(&self, graph: Graph) -> &[f32] {
        match graph {
            Graph::Cpu => &self.cpu,
            Graph::Memory => &self.memory,
            Graph::DiskActivity => &self.disk_activity,
            Graph::Network => &self.network,
        }
    }

    /// The most `graph` read in its minute; `None` while nothing has
    /// measured it.
    #[must_use]
    pub fn peak(&self, graph: Graph) -> Option<f32> {
        self.samples(graph).iter().copied().reduce(f32::max)
    }

    /// What `graph`'s heading says it reads: now, and the most in its
    /// minute -- "23%, peak 80%", "1.2 MB/s, peak 3.4 MB/s"; `None` while
    /// nothing has measured it.
    #[must_use]
    pub fn reading(&self, graph: Graph) -> Option<String> {
        let now = *self.samples(graph).last()?;
        let peak = self.peak(graph)?;
        Some(format!(
            "{}, peak {}",
            amount(graph, now),
            amount(graph, peak)
        ))
    }

    /// What tools are told of `graph`: now -- the network's in and out
    /// apart -- and the most in its minute; or that nothing measures it.
    #[must_use]
    pub fn description(&self, graph: Graph) -> String {
        let (Some(&now), Some(peak)) = (self.samples(graph).last(), self.peak(graph)) else {
            return NOT_MEASURED.to_owned();
        };
        let now = match (graph, self.network_now) {
            (Graph::Network, Some(rate)) => format!(
                "{} in, {} out",
                guitk::bytes::si_rate(rate.down),
                guitk::bytes::si_rate(rate.up)
            ),
            _ => amount(graph, now),
        };
        format!(
            "{now} now, at most {} in the last minute",
            amount(graph, peak)
        )
    }
}

/// A sample of `graph` as words: a share as a percentage, the network's
/// bytes as a rate.
fn amount(graph: Graph, value: f32) -> String {
    match graph {
        Graph::Network => guitk::bytes::si_rate(whole_bytes(value)),
        Graph::Cpu | Graph::Memory | Graph::DiskActivity => {
            format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0)
        }
    }
}

/// A rate kept as `f32` for plotting, back as whole bytes a second: a cast
/// from a float saturates, so nothing below zero and nothing past what
/// `u64` holds comes out of it.
fn whole_bytes(value: f32) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a float's cast saturates, at zero and at u64::MAX; a rate has no fraction worth a word"
    )]
    let bytes = f64::from(value).max(0.0) as u64;
    bytes
}

/// A count as `f32`, for a graph's height: a rate of bytes a second, which
/// `f32` holds to seven figures -- a plotted line needs no more.
fn plotted(value: u64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a rate plotted as a line's height; seven figures are more than a pixel"
    )]
    let value = value as f32;
    value
}

/// `part` of `whole`, as `f32`.
fn ratio(part: u64, whole: u64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "milliseconds over a second or two: exact in f64, and a share needs no more than f32"
    )]
    let r = (part as f64 / whole as f64) as f32;
    r
}

/// `bytes` over `elapsed_ms`, as bytes a second; `None` over no time.
fn per_second(bytes: u64, elapsed_ms: u64) -> Option<u64> {
    let scaled = u128::from(bytes).saturating_mul(1000);
    let rate = scaled.checked_div(u128::from(elapsed_ms))?;
    Some(u64::try_from(rate).unwrap_or(u64::MAX))
}

/// The busiest disk's share of `elapsed_ms` spent doing I/O between two
/// reads of `/proc/diskstats`: each disk's time doing I/O differenced, over
/// the time between the reads.
///
/// **The busiest, not the average.** A graph of how hard the disks are
/// working reads one disk flat out as flat out; an average with an idle
/// second disk would halve it. A partition is never busier than its disk,
/// so the partitions listed beside the disks change nothing.
///
/// A disk only one read lists, or whose counter went back (the device was
/// replaced), is left out. `None` if no disk is left, or no time passed.
#[must_use]
pub fn disk_busy_fraction(prev: &[DiskStats], now: &[DiskStats], elapsed_ms: u64) -> Option<f32> {
    if elapsed_ms == 0 {
        return None;
    }
    now.iter()
        .filter_map(|disk| {
            let before = prev.iter().find(|p| p.name == disk.name)?;
            let busy = disk.ms_doing_io?.checked_sub(before.ms_doing_io?)?;
            Some(ratio(busy, elapsed_ms).clamp(0.0, 1.0))
        })
        .reduce(f32::max)
}

/// The rate in and out over every interface but the loopback between two
/// reads of `/proc/net/dev`: each interface's bytes differenced, summed, over
/// the time between the reads.
///
/// The loopback is a program talking to itself, which is traffic on no
/// network. An interface only one read lists, or whose counters went back
/// (it was taken down and brought up again), is left out. `None` if none is
/// left, or no time passed.
#[must_use]
pub fn network_rate(prev: &[NetDevice], now: &[NetDevice], elapsed_ms: u64) -> Option<NetRate> {
    if elapsed_ms == 0 {
        return None;
    }
    let mut total: Option<(u64, u64)> = None;
    for device in now.iter().filter(|d| d.name.as_slice() != b"lo") {
        let Some(before) = prev.iter().find(|p| p.name == device.name) else {
            continue;
        };
        let moved = |now: Option<u64>, then: Option<u64>| now?.checked_sub(then?);
        let (Some(down), Some(up)) = (
            moved(device.rx_bytes, before.rx_bytes),
            moved(device.tx_bytes, before.tx_bytes),
        ) else {
            continue;
        };
        let (all_down, all_up) = total.unwrap_or((0, 0));
        total = Some((all_down.saturating_add(down), all_up.saturating_add(up)));
    }
    let (down, up) = total?;
    Some(NetRate {
        down: per_second(down, elapsed_ms)?,
        up: per_second(up, elapsed_ms)?,
    })
}

/// What the shell keeps between samples: the counters last read and when,
/// and each graph's last minute.
#[derive(Debug)]
pub struct Sampler {
    /// When the last sample was taken, on the session's clock.
    last_ms: Option<u64>,
    /// The disks' counters as last read, and when.
    disks: Option<(u64, Vec<DiskStats>)>,
    /// The interfaces' counters as last read, and when.
    nets: Option<(u64, Vec<NetDevice>)>,
    /// Processor use.
    cpu: SampleHistory,
    /// Memory in use.
    memory: SampleHistory,
    /// The busiest disk's activity.
    disk_activity: SampleHistory,
    /// The network's bytes a second, in and out together.
    network: SampleHistory,
    /// The network's last rate, in and out apart.
    network_now: Option<NetRate>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    /// Nothing sampled yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_ms: None,
            disks: None,
            nets: None,
            cpu: SampleHistory::new(HISTORY_SAMPLES),
            memory: SampleHistory::new(HISTORY_SAMPLES),
            disk_activity: SampleHistory::new(HISTORY_SAMPLES),
            network: SampleHistory::new(HISTORY_SAMPLES),
            network_now: None,
        }
    }

    /// Whether a sample at `now_ms` would be one: the first, or one at least
    /// [`MIN_SAMPLE_SPACING_MS`] after the last.
    #[must_use]
    pub fn due(&self, now_ms: u64) -> bool {
        self.last_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= MIN_SAMPLE_SPACING_MS)
    }

    /// Take the sample read at `now_ms`: the processor's and memory's shares
    /// as the meters have them, and the disks' and interfaces' counters as
    /// read -- `None` where there was nothing to read.
    ///
    /// A reading nothing measured empties its graph rather than leaving the
    /// line where it stopped: a line that stops reads as one that held. The
    /// counters' first read measures nothing -- a rate is a difference --
    /// so the disks' and network's graphs begin a sample after the others.
    pub fn record(
        &mut self,
        now_ms: u64,
        cpu: Option<f32>,
        memory: Option<f32>,
        disks: Option<Vec<DiskStats>>,
        nets: Option<Vec<NetDevice>>,
    ) {
        self.last_ms = Some(now_ms);
        push_or_clear(&mut self.cpu, cpu);
        push_or_clear(&mut self.memory, memory);
        let activity = match (&self.disks, &disks) {
            (Some((then, prev)), Some(now)) => {
                disk_busy_fraction(prev, now, now_ms.saturating_sub(*then))
            }
            _ => None,
        };
        self.disks = disks.map(|now| (now_ms, now));
        push_or_clear(&mut self.disk_activity, activity);
        let rate = match (&self.nets, &nets) {
            (Some((then, prev)), Some(now)) => {
                network_rate(prev, now, now_ms.saturating_sub(*then))
            }
            _ => None,
        };
        self.nets = nets.map(|now| (now_ms, now));
        self.network_now = rate;
        push_or_clear(&mut self.network, rate.map(|r| plotted(r.total())));
    }

    /// The graphs, as a frame is given them.
    #[must_use]
    pub fn graphs(&self) -> Graphs {
        Graphs {
            cpu: self.cpu.to_vec(),
            memory: self.memory.to_vec(),
            disk_activity: self.disk_activity.to_vec(),
            network: self.network.to_vec(),
            network_now: self.network_now,
        }
    }
}

/// `value` onto `history`, or `history` emptied when there is none.
fn push_or_clear(history: &mut SampleHistory, value: Option<f32>) {
    match value {
        Some(value) => history.push(value),
        None => history.clear(),
    }
}

/// Where a graph is drawn in a monitor's content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    /// Which graph.
    pub graph: Graph,
    /// Its heading's line: the label at the left, the reading at the right.
    pub heading: Rect,
    /// Where its line is plotted, below the heading; `None` where the panel
    /// is too short for a line anyone could read a shape from.
    pub plot: Option<Rect>,
}

impl Panel {
    /// Its whole box: the heading, and the plot below it.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        self.plot
            .map_or(self.heading, |plot| self.heading.union(plot))
    }
}

/// The size a graph's heading is written at: a meter's.
pub const HEADING_SIZE: f32 = 10.0;
/// How far apart two graphs are.
const PANEL_GAP: f32 = 6.0;
/// How far below its heading a graph's plot begins.
const PLOT_GAP: f32 = 2.0;
/// The shortest plot drawn: a line in less is a row of pixels.
const MIN_PLOT: f32 = 12.0;
/// How thick a graph's line is.
const LINE_WIDTH: f32 = 1.5;
/// How thick a plot's guide lines are: furniture, thinner than any reading.
const GRID_WIDTH: f32 = 0.5;
/// The fractions of a plot's height its guide lines are drawn at.
const GRID_AT: [f32; 3] = [0.25, 0.5, 0.75];

/// The heading's line height.
fn heading_height() -> f32 {
    guitk::text::line_height(HEADING_SIZE, FontWeightHint::Bold)
}

/// Where the graphs go in a monitor's content `content`, top to bottom,
/// sharing the height above the last `foot` of it -- where the monitor draws
/// its disk-space meter -- and a gap.
#[must_use]
pub fn panels(content: Rect, foot: f32) -> Vec<Panel> {
    let heading = heading_height();
    let count = plotted(Graph::ALL.len() as u64);
    let room = (content.h - foot - PANEL_GAP).max(0.0);
    let each = ((room - PANEL_GAP * (count - 1.0)) / count).max(0.0);
    let mut top = content.y;
    Graph::ALL
        .into_iter()
        .map(|graph| {
            let plot_h = each - heading - PLOT_GAP;
            let panel = Panel {
                graph,
                heading: Rect::new(content.x, top, content.w, heading.min(each)),
                plot: (plot_h >= MIN_PLOT)
                    .then(|| Rect::new(content.x, top + heading + PLOT_GAP, content.w, plot_h)),
            };
            top += each + PANEL_GAP;
            panel
        })
        .collect()
}

/// Draw `panels` with `graphs`' readings, at `alpha`: each heading -- its
/// label, and what it reads now and at most or that nothing measures it --
/// and each plot's guide lines and line.
pub fn render(
    panels: &[Panel],
    graphs: &Graphs,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let wash = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
    let weight = FontWeightHint::Bold;
    for panel in panels {
        let label = panel.graph.label();
        let heading = panel.heading;
        commands.push(RenderCommand::Text {
            x: heading.x,
            y: heading.y,
            text: label.to_owned(),
            font_size: HEADING_SIZE,
            color: wash(p.subtext0),
            font_weight: weight,
            max_width: Some(heading.w),
            overflow: TextOverflow::Ellipsis,
        });
        // What it reads, against the heading's right edge, never back over
        // its label.
        let (reading, ink) = graphs
            .reading(panel.graph)
            .map_or((NOT_MEASURED.to_owned(), p.subtext0), |r| (r, p.text));
        let label_end = heading.x + guitk::text::measure(label, HEADING_SIZE, weight) + 8.0;
        let width = guitk::text::measure(&reading, HEADING_SIZE, FontWeightHint::Regular);
        let x = (heading.right() - width).max(label_end);
        commands.push(RenderCommand::Text {
            x,
            y: heading.y,
            text: reading,
            font_size: HEADING_SIZE,
            color: wash(ink),
            font_weight: FontWeightHint::Regular,
            max_width: Some((heading.right() - x).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if let Some(plot) = panel.plot {
            plot_lines(
                plot,
                graphs.samples(panel.graph),
                panel.graph,
                p,
                alpha,
                commands,
            );
        }
    }
}

/// `samples` plotted in `plot`, with its guide lines: the newest at the
/// right edge, a minute across, so the line moves left as it fills. A share
/// is plotted on its whole, the network on the most it carried in the
/// minute -- a rate has no whole, and plotted on a fixed one a quiet minute
/// would be a flat line along the floor.
fn plot_lines(
    plot: Rect,
    samples: &[f32],
    graph: Graph,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let grid = Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha);
    for at in GRID_AT {
        let y = plot.y + plot.h * at;
        commands.push(RenderCommand::Line {
            x1: plot.x,
            y1: y,
            x2: plot.right(),
            y2: y,
            color: grid,
            width: GRID_WIDTH,
        });
    }
    let whole = match graph {
        Graph::Network => samples.iter().copied().fold(0.0_f32, f32::max),
        Graph::Cpu | Graph::Memory | Graph::DiskActivity => 1.0,
    };
    let height = |value: f32| {
        let share = if whole > 0.0 {
            (value / whole).clamp(0.0, 1.0)
        } else {
            0.0
        };
        plot.bottom() - share * plot.h
    };
    let step = plot.w / plotted(HISTORY_SAMPLES.saturating_sub(1) as u64).max(1.0);
    let hue = graph.hue(p);
    let ink = Color::rgba(hue.r, hue.g, hue.b, alpha);
    let newest = samples.len().saturating_sub(1);
    let x_of = |index: usize| plot.right() - plotted(newest.saturating_sub(index) as u64) * step;
    for (index, pair) in samples.windows(2).enumerate() {
        let [before, after] = *pair else {
            continue;
        };
        commands.push(RenderCommand::Line {
            x1: x_of(index),
            y1: height(before),
            x2: x_of(index.saturating_add(1)),
            y2: height(after),
            color: ink,
            width: LINE_WIDTH,
        });
    }
}

#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
