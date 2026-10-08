//! The network widget: the link the machine is on -- connected or not, the
//! address it has, the traffic over it now, the gateway and the name
//! server -- and, given the room, the last minute's traffic as the system
//! monitor graphs it (`design-decisions.md` §1502).
//!
//! The link is the kernel's (`crate::netlink`), read by the shell no oftener
//! than [`LINK_SPACING_MS`] while a network widget is out; the traffic is the
//! system monitor's own sample (`monitor::Sampler`), taken each second while
//! either is out, so the two widgets cannot disagree about it. On SlateOS the
//! traffic is not measured until the kernel serves Linux's counters
//! (`requests/c-a-the-system-monitor-reads-disk-and-network-counters-the-kernel-does-not-serve.md`),
//! and the widget says so.
//!
//! # One layout
//!
//! [`layout`] places the lines -- as many as fit, most wanted first -- and
//! the graph, and the drawing ([`render`]) and what tools are told
//! (`widgets::accessible`) both read it.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};

use super::monitor::{self, Graph, Graphs, Panel};
use crate::Rect;
use crate::netlink::{Link, LinkState};

/// How long the shell waits between two readings of the link.
pub const LINK_SPACING_MS: u64 = 5_000;

/// A line's size: a label at the left, its value at the right.
const LINE_SIZE: f32 = 12.0;
/// The first line -- whether the machine is connected -- is bolder.
const STATUS_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// Room between two lines.
const LINE_GAP: f32 = 3.0;
/// Room between the lines and the graph.
const GRAPH_GAP: f32 = 8.0;
/// The shortest graph drawn, heading and plot: less is a smear.
const MIN_GRAPH: f32 = 48.0;
/// Room between a label and its value.
const LABEL_ROOM: f32 = 8.0;

fn line_height() -> f32 {
    guitk::text::line_height(LINE_SIZE, STATUS_WEIGHT)
}

/// What the network widget is told.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkReading {
    /// The link as last read; `None` before the first reading. `Err` holds
    /// why it could not be read.
    pub link: Option<Result<LinkState, String>>,
    /// The traffic: the system monitor's sample, whose network graph and
    /// last rate are what this widget shows.
    pub graphs: Graphs,
}

/// One of the widget's lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What it is: "Address", "Traffic".
    pub label: &'static str,
    /// What it says.
    pub value: String,
    /// Whether it is the status line, drawn bolder and in the colour of
    /// the state.
    pub status: Option<Status>,
}

/// The state the status line reports, which decides its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The link is up: green.
    Connected,
    /// Anything else -- down, no adapter, not known: the quiet ink.
    Other,
}

/// The status line's label: the widget's first line names the network.
pub const STATUS_LABEL: &str = "Network";

/// Where the shell reads the link from.
#[derive(Clone, Debug)]
enum Source {
    /// This machine's (`netlink::current`).
    System,
    /// A directory laid out as the kernel's: a test's own.
    Dir(std::path::PathBuf),
}

/// What the shell keeps of the link: the last reading, and when.
#[derive(Clone, Debug)]
pub struct LinkSampler {
    /// Where it is read.
    source: Source,
    /// When the last reading was taken, on the session's clock.
    last_ms: Option<u64>,
    /// What it found; `None` before the first reading.
    link: Option<Result<LinkState, String>>,
}

impl Default for LinkSampler {
    fn default() -> Self {
        Self {
            source: Source::System,
            last_ms: None,
            link: None,
        }
    }
}

impl LinkSampler {
    /// A sampler reading the link from `dir`, laid out as the kernel's.
    #[must_use]
    pub fn of_dir(dir: std::path::PathBuf) -> Self {
        Self {
            source: Source::Dir(dir),
            ..Self::default()
        }
    }

    /// Read the link at `now_ms` on the session's clock, unless the last
    /// reading was taken less than [`LINK_SPACING_MS`] before; whether it
    /// read.
    pub fn sample(&mut self, now_ms: u64) -> bool {
        if self
            .last_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < LINK_SPACING_MS)
        {
            return false;
        }
        self.last_ms = Some(now_ms);
        let read = match &self.source {
            Source::System => crate::netlink::current(),
            Source::Dir(dir) => crate::netlink::read(dir),
        };
        self.link = Some(read.map_err(|e| e.to_string()));
        true
    }

    /// What the last reading found; `None` before the first.
    #[must_use]
    pub fn link(&self) -> Option<&Result<LinkState, String>> {
        self.link.as_ref()
    }
}

/// What the traffic line says while nothing measures the traffic.
pub const NOT_MEASURED: &str = "Not measured";

/// The lines `reading` says, most wanted first: whether the machine is
/// connected, the address it has, the traffic, the gateway and the name
/// server -- the last four only for a link there is.
#[must_use]
pub fn lines(reading: &NetworkReading) -> Vec<Line> {
    let status = |value: &str, state: Status| Line {
        label: STATUS_LABEL,
        value: value.to_owned(),
        status: Some(state),
    };
    let link = match &reading.link {
        None => return Vec::new(),
        Some(Err(_)) => return vec![status("Could not be read", Status::Other)],
        Some(Ok(LinkState::Unknown)) => return vec![status("Not known here", Status::Other)],
        Some(Ok(LinkState::NoAdapter)) => {
            return vec![status("No network adapter", Status::Other)];
        }
        Some(Ok(LinkState::Link(link))) => link,
    };
    let mut said = vec![if link.up {
        status("Connected", Status::Connected)
    } else {
        status("Not connected", Status::Other)
    }];
    let fact = |label: &'static str, value: String| Line {
        label,
        value,
        status: None,
    };
    said.push(fact("Address", address(link)));
    said.push(fact("Traffic", traffic(&reading.graphs)));
    let or_none = |a: Option<std::net::Ipv4Addr>| {
        a.as_ref()
            .map_or_else(|| "None".to_owned(), ToString::to_string)
    };
    said.push(fact("Gateway", or_none(link.gateway)));
    said.push(fact("DNS", or_none(link.dns)));
    said
}

/// The address with its network's prefix -- `10.0.2.15/24` -- or that it has
/// none yet.
fn address(link: &Link) -> String {
    match (link.address, link.prefix) {
        (Some(address), Some(prefix)) => format!("{address}/{prefix}"),
        (Some(address), None) => address.to_string(),
        (None, _) => "None yet".to_owned(),
    }
}

/// The traffic now, in and out -- decimal, as a network counts -- or that
/// nothing measures it.
fn traffic(graphs: &Graphs) -> String {
    graphs.network_now.map_or_else(
        || NOT_MEASURED.to_owned(),
        |rate| {
            format!(
                "{} in, {} out",
                guitk::bytes::si_rate(rate.down),
                guitk::bytes::si_rate(rate.up)
            )
        },
    )
}

/// The widget as laid out in its content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Each line that fits, by its place among [`lines`], and its box.
    pub lines: Vec<(usize, Rect)>,
    /// The traffic graph, when there is room for one under the lines.
    pub graph: Option<Panel>,
}

/// `count` lines placed down `content`, as many as fit, and under them the
/// network's graph when the rest of the content is tall enough for one.
#[must_use]
pub fn layout(count: usize, content: Rect) -> Layout {
    let laid = super::listing::stack(count, line_height(), LINE_GAP, 0.0, content);
    let bottom = laid.rows.last().map_or(content.y, |(_, b)| b.y + b.h);
    let room = content.y + content.h - bottom - GRAPH_GAP;
    let graph = (laid.more.is_none() && count > 1 && room >= MIN_GRAPH).then(|| {
        let area = Rect::new(content.x, bottom + GRAPH_GAP, content.w, room);
        monitor::panels_of(&[Graph::Network], area)
            .into_iter()
            .next()
    });
    Layout {
        lines: laid.rows,
        graph: graph.flatten(),
    }
}

/// Draw `reading` in `content`, in `p` at `alpha`: each line's label and
/// value -- the status bolder, green when connected -- and the graph when
/// it fits.
pub fn render(
    reading: &NetworkReading,
    content: Rect,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let wash = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
    let said = lines(reading);
    let laid = layout(said.len(), content);
    for &(index, bounds) in &laid.lines {
        let Some(line) = said.get(index) else {
            continue;
        };
        let weight = if line.status.is_some() {
            STATUS_WEIGHT
        } else {
            FontWeightHint::Regular
        };
        let value_ink = match line.status {
            Some(Status::Connected) => p.ink(p.green),
            Some(Status::Other) => p.subtext0,
            None => p.text,
        };
        let value_w = guitk::text::measure(&line.value, LINE_SIZE, weight).min(bounds.w * 0.7);
        let mut text = |x: f32, width: f32, said: String, ink: Color| {
            commands.push(RenderCommand::Text {
                x,
                y: bounds.y,
                text: said,
                font_size: LINE_SIZE,
                color: wash(ink),
                font_weight: weight,
                max_width: Some(width.max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        };
        text(
            bounds.x,
            bounds.w - value_w - LABEL_ROOM,
            line.label.to_owned(),
            p.subtext0,
        );
        text(
            bounds.x + bounds.w - value_w,
            value_w,
            line.value.clone(),
            value_ink,
        );
    }
    if let Some(panel) = laid.graph {
        monitor::render(&[panel], &reading.graphs, p, alpha, commands);
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
