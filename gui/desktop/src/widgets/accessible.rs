//! The desktop's widgets as tools see them -- a screen reader, a script:
//! each widget a group named by its title, holding what it shows.
//!
//! | widget | what is in it |
//! |---|---|
//! | clock | its time and its date, each a label, read as the taskbar's clock reads them |
//! | system monitor | a progress bar for each meter -- CPU, memory, disk -- or, for one nothing has measured, no value and "not measured"; given two rows, a picture for each graph of the last minute, described by what it reads now and at most, and the disk's meter below them |
//! | note | a text area holding its text, the one being written in having the keyboard |
//! | photo frame | the picture up on it, named by its file, or what it says in a picture's place |
//! | battery | its charge, or "No battery", described by what its icon shows and the time left |
//! | calendar | the month today is in, a grid named by the month and year, each day named by its date and said to be today, to have events, or to be another month's |
//! | upcoming events | a list of the events it shows, each named by its title and described by its day and time, the list said to have nothing coming or how many more there are |
//! | world clock | a list of the user's other clocks, each named by its label and described by its time, its day and how far ahead or behind it is, the list said to have none or how many more there are |
//! | disk usage | a list of the disks mounted, each a progress bar named by where it is mounted, at how much of it is in use and described by what is free of what it holds -- and "nearly full" past nine tenths -- or, for one not measured, no value and "Not measured"; the list said to have none, that they could not be listed and why, or how many more there are |
//! | the rest | the name they show where their content will be |
//!
//! A widget not drawn is not here: one hidden, and every one while the layer
//! is. Each part's box is where it is drawn, from the numbers the drawing
//! uses.
//!
//! Writing in a note is the shell's: a tool's text goes in as the user's
//! does, the note opened with a click on it first, where nothing else takes
//! the click (`DesktopShell`'s `Accessible`).

use guitk::render::FontWeightHint;
use guitk::widget::automation::{Node, Role, Value};

use super::disks::{self, Disks};
use super::monitor::{Graph, Graphs, Panel};
use super::{
    BATTERY_CHARGE, CLOCK_TIME, DesktopWidgetManager, LiveReadings, METER_HEIGHT, Meter,
    NOT_MEASURED, PLACEHOLDER_SIZE, PROBLEM_SIZE, WidgetInstance, WidgetInstanceId, WidgetKind,
    calendar_cell, charge_text, clock_date, estimate_text, fit_within, monitor_layout,
    placeholder_line, problem_line,
};
use super::{listing, upcoming, worldclock};
use crate::Rect;
use crate::calendar::{MonthGlance, Upcoming, WorldClockReading};
use crate::power::{BatteryInfo, BatteryState};

/// A part of the widget layer, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetPart {
    /// The layer: every widget out on the desktop.
    Layer,
    /// A widget, named by its title.
    Widget(WidgetInstanceId),
    /// A clock's time.
    Time(WidgetInstanceId),
    /// A clock's date.
    Date(WidgetInstanceId),
    /// One of a system monitor's meters.
    Meter(WidgetInstanceId, Meter),
    /// One of a system monitor's graphs of the last minute.
    Graph(WidgetInstanceId, Graph),
    /// A note's writing area: written in as its user writes in it.
    Note(WidgetInstanceId),
    /// A photo frame's picture, or what the frame says in a picture's place.
    Picture(WidgetInstanceId),
    /// A battery widget's charge.
    Charge(WidgetInstanceId),
    /// A calendar's month: its days, named by the month and year.
    Month(WidgetInstanceId),
    /// A day of a calendar's month, by its place in the six weeks.
    Day(WidgetInstanceId, usize),
    /// An upcoming-events widget's list.
    Upcoming(WidgetInstanceId),
    /// One of the events it lists, by its place among them.
    UpcomingEvent(WidgetInstanceId, usize),
    /// A world clock widget's list of the user's other clocks.
    WorldClocks(WidgetInstanceId),
    /// One of them, by its place in the list.
    OtherClock(WidgetInstanceId, usize),
    /// A disk usage widget's list of the disks mounted.
    Disks(WidgetInstanceId),
    /// One of them, by its place in the list.
    Disk(WidgetInstanceId, usize),
    /// The name a widget with no content of its own yet shows in its place.
    Placeholder(WidgetInstanceId),
}

/// A line of text at `(x, y)`, `width` wide, written at `size`: its box.
fn line_box(x: f32, y: f32, width: f32, size: f32) -> Rect {
    Rect::new(
        x,
        y,
        width.max(0.0),
        guitk::text::line_height(size, FontWeightHint::Regular),
    )
}

/// A calendar's month in its content `(x, y, width, height)`, as tools see
/// it: a grid named by the month and year, each day a cell named by its date
/// and described as the popup's days are -- today, its events, another
/// month's -- boxed where it is drawn. `None` for an empty glance, which
/// draws nothing.
fn month_node(
    id: WidgetInstanceId,
    month: &MonthGlance,
    (x, y, width, height): (f32, f32, f32, f32),
) -> Option<Node<WidgetPart>> {
    let cells: Vec<Rect> = (0..month.days.len())
        .map(|index| calendar_cell(index, x, y, width, height))
        .collect();
    let bounds = cells.iter().copied().reduce(Rect::union)?;
    let mut grid = Node::new(
        WidgetPart::Month(id),
        Role::Grid,
        month.title.clone(),
        bounds,
    );
    for (index, (day, cell)) in month.days.iter().zip(cells).enumerate() {
        let mut node = Node::new(
            WidgetPart::Day(id, index),
            Role::GridCell,
            day.name.clone(),
            cell,
        );
        node.description = day.description();
        grid.children.push(node);
    }
    Some(grid)
}

/// An upcoming-events widget's list in its content `(x, y, width, height)`:
/// each event it shows, named by its title and described by its day and
/// time -- "Today, 09:00" -- boxed where it is drawn ([`upcoming::lines`]);
/// the list said to have nothing coming, or how many more it does not show.
fn upcoming_node(
    id: WidgetInstanceId,
    upcoming: &Upcoming,
    (x, y, width, height): (f32, f32, f32, f32),
) -> Node<WidgetPart> {
    let content = Rect::new(x, y, width, height);
    let mut list = Node::new(
        WidgetPart::Upcoming(id),
        Role::List,
        WidgetKind::Reminders.label(),
        content,
    );
    if upcoming.days.is_empty() {
        list.description = Some(upcoming::NOTHING_COMING.to_owned());
        return list;
    }
    let laid = upcoming::lines(upcoming, content);
    for line in &laid.lines {
        if let upcoming::Line::Event {
            day,
            event,
            index,
            bounds,
        } = *line
            && let Some((day, event)) = upcoming
                .days
                .get(day)
                .and_then(|d| d.events.get(event).map(|e| (d, e)))
        {
            let mut row = Node::new(
                WidgetPart::UpcomingEvent(id, index),
                Role::ListItem,
                event.title.clone(),
                bounds,
            );
            row.description = Some(format!("{}, {}", day.label, event.time));
            list.children.push(row);
        }
    }
    if let Some((more, _)) = laid.more {
        list.description = Some(format!("{} not shown", listing::more_text(more)));
    }
    list
}

/// A world clock widget's list in its content `(x, y, width, height)`: each
/// clock it shows, named by its label and described by its time, its day and
/// its offset -- "09:30, Tomorrow, 13 hours ahead" -- boxed where it is drawn
/// ([`worldclock::rows`]); the list said to have no clocks, or how many more
/// it does not show.
fn world_clocks_node(
    id: WidgetInstanceId,
    clocks: &[WorldClockReading],
    (x, y, width, height): (f32, f32, f32, f32),
) -> Node<WidgetPart> {
    let content = Rect::new(x, y, width, height);
    let mut list = Node::new(
        WidgetPart::WorldClocks(id),
        Role::List,
        WidgetKind::WorldClock.label(),
        content,
    );
    if clocks.is_empty() {
        list.description = Some(worldclock::NO_CLOCKS.to_owned());
        return list;
    }
    let laid = worldclock::rows(clocks, content);
    for &(index, bounds) in &laid.rows {
        if let Some(clock) = clocks.get(index) {
            let mut row = Node::new(
                WidgetPart::OtherClock(id, index),
                Role::ListItem,
                clock.label.clone(),
                bounds,
            );
            row.description = Some(format!("{}, {}", clock.time, clock.detail()));
            list.children.push(row);
        }
    }
    if let Some((more, _)) = laid.more {
        list.description = Some(format!("{} not shown", listing::more_text(more)));
    }
    list
}

/// A disk usage widget's list in its content `(x, y, width, height)`: each
/// disk it shows a progress bar, named by where it is mounted, at how much of
/// it is in use, and described by what is free of what it holds -- and as
/// nearly full when it is -- boxed where it is drawn ([`disks::rows`]); one
/// not measured with no value, and described so. In place of a list, the
/// list says why there is none -- with why, when the disks could not be
/// listed -- and under one, how many more it does not show.
fn disks_node(
    id: WidgetInstanceId,
    shown: &Disks,
    (x, y, width, height): (f32, f32, f32, f32),
) -> Node<WidgetPart> {
    let content = Rect::new(x, y, width, height);
    let mut list = Node::new(
        WidgetPart::Disks(id),
        Role::List,
        WidgetKind::DiskUsage.label(),
        content,
    );
    let listed = match shown {
        Disks::Listed(listed) if !listed.is_empty() => listed,
        Disks::Unlisted(why) => {
            list.description = Some(format!("{}: {why}", disks::UNLISTED));
            return list;
        }
        other => {
            list.description = disks::in_place_of_a_list(other).map(str::to_owned);
            return list;
        }
    };
    let laid = disks::rows(listed, content);
    for &(index, bounds) in &laid.rows {
        if let Some(disk) = listed.get(index) {
            let mut row = Node::new(
                WidgetPart::Disk(id, index),
                Role::ProgressBar,
                disk.name.clone(),
                bounds,
            );
            row.value = disk.fraction.map(|fraction| Value::Progress {
                value: fraction.clamp(0.0, 1.0) * 100.0,
                max: 100.0,
            });
            row.description = Some(if disk.nearly_full {
                format!("{}, {}", disk.detail, disks::NEARLY_FULL)
            } else {
                disk.detail.clone()
            });
            list.children.push(row);
        }
    }
    if let Some((more, _)) = laid.more {
        list.description = Some(format!("{} not shown", listing::more_text(more)));
    }
    list
}

/// A system monitor's `meter`, its heading's line at `(x, top)`, `width`
/// wide: a progress bar at its reading in `live`, or -- nothing having
/// measured it -- none, and said so.
fn meter_node(
    id: WidgetInstanceId,
    meter: Meter,
    (x, top, width): (f32, f32, f32),
    live: &LiveReadings,
) -> Node<WidgetPart> {
    let mut node = Node::new(
        WidgetPart::Meter(id, meter),
        Role::ProgressBar,
        meter.label(),
        Rect::new(x, top, width, METER_HEIGHT),
    );
    match meter.reading(live) {
        Some(fraction) => {
            node.value = Some(Value::Progress {
                value: fraction.clamp(0.0, 1.0) * 100.0,
                max: 100.0,
            });
        }
        None => node.description = Some(NOT_MEASURED.to_owned()),
    }
    node
}

/// A system monitor's graph, where `panel` places it: a picture named by
/// what it plots, described by what it reads now and at most in the minute
/// -- or that nothing measures it.
fn graph_node(id: WidgetInstanceId, panel: &Panel, graphs: &Graphs) -> Node<WidgetPart> {
    let mut node = Node::new(
        WidgetPart::Graph(id, panel.graph),
        Role::Image,
        panel.graph.label(),
        panel.bounds(),
    );
    node.description = Some(graphs.description(panel.graph));
    node
}

/// What else the battery widget says of the charge: what its icon shows --
/// charging, nearly flat -- and the time left, where something says. `None`
/// with nothing more to say.
fn charge_description(b: &BatteryInfo) -> Option<String> {
    let state = match b.state {
        BatteryState::Charging if b.present => Some("charging".to_owned()),
        BatteryState::Critical if b.present => Some("critically low".to_owned()),
        _ => None,
    };
    let said: Vec<String> = state
        .into_iter()
        .chain(b.time_remaining_secs.map(estimate_text))
        .collect();
    (!said.is_empty()).then(|| said.join(", "))
}

impl DesktopWidgetManager {
    /// The layer as tools see it, its readings `live`'s -- the clock's time,
    /// the meters, the battery, as [`render`](Self::render) is given them:
    /// `None` while no widget is drawn.
    #[must_use]
    pub fn automation(&self, live: &LiveReadings) -> Option<Node<WidgetPart>> {
        if !self.layer_visible {
            return None;
        }
        let widgets: Vec<Node<WidgetPart>> = self
            .widgets
            .iter()
            .filter(|w| w.visible)
            .map(|w| self.widget_node(w, live))
            .collect();
        let bounds = widgets.iter().map(|node| node.bounds).reduce(Rect::union)?;
        let mut layer = Node::new(WidgetPart::Layer, Role::Group, "Widgets", bounds);
        layer.children = widgets;
        Some(layer)
    }

    /// The widget `w`, and what it shows.
    fn widget_node(&self, w: &WidgetInstance, live: &LiveReadings) -> Node<WidgetPart> {
        let (x, y, width, height) = self.frame(w);
        let mut node = Node::new(
            WidgetPart::Widget(w.id),
            Role::Group,
            w.title(),
            Rect::new(x, y, width, height),
        );
        let (cx, cy, cw, ch) = self.content_of(w);
        let id = w.id;
        node.children = match &w.kind {
            WidgetKind::Clock => vec![
                Node::new(
                    WidgetPart::Time(id),
                    Role::Label,
                    live.clock_time.clone(),
                    CLOCK_TIME.bounds(cx, cy, cw),
                ),
                Node::new(
                    WidgetPart::Date(id),
                    Role::Label,
                    live.clock_date.clone(),
                    clock_date().bounds(cx, cy, cw),
                ),
            ],
            WidgetKind::SystemMonitor => {
                // Top to bottom, as they are drawn: the graphs, if it has
                // room for them, and then its meters.
                let layout = monitor_layout(w.size, (cx, cy, cw, ch));
                layout
                    .panels
                    .iter()
                    .map(|panel| graph_node(id, panel, &live.graphs))
                    .chain(
                        layout
                            .meters
                            .iter()
                            .map(|&(meter, top)| meter_node(id, meter, (cx, top, cw), live)),
                    )
                    .collect()
            }
            WidgetKind::Notes => {
                let mut note = Node::new(
                    WidgetPart::Note(id),
                    Role::TextArea,
                    w.title(),
                    Rect::new(cx, cy, cw, ch),
                );
                // Its text is the widget's at every change, so this is what
                // the open note holds too.
                note.value = Some(Value::Text(w.state_text.clone()));
                note.focused = self.writing_note() == Some(id);
                note.focusable = true;
                vec![note]
            }
            WidgetKind::PhotoFrame => self
                .picture_node(id, (cx, cy, cw, ch))
                .into_iter()
                .collect(),
            WidgetKind::Calendar => month_node(id, &live.month, (cx, cy, cw, ch))
                .into_iter()
                .collect(),
            WidgetKind::Reminders => vec![upcoming_node(id, &live.upcoming, (cx, cy, cw, ch))],
            WidgetKind::WorldClock => {
                vec![world_clocks_node(id, &live.world_clocks, (cx, cy, cw, ch))]
            }
            WidgetKind::DiskUsage => vec![disks_node(id, &live.disks, (cx, cy, cw, ch))],
            WidgetKind::BatteryStatus => {
                let mut charge = Node::new(
                    WidgetPart::Charge(id),
                    Role::Label,
                    charge_text(&live.battery),
                    BATTERY_CHARGE.bounds(cx, cy, cw),
                );
                charge.description = charge_description(&live.battery);
                vec![charge]
            }
            _ => {
                let (tx, ty, max_width) = placeholder_line(cx, cy, cw, ch);
                vec![Node::new(
                    WidgetPart::Placeholder(id),
                    Role::Label,
                    w.kind.label(),
                    line_box(tx, ty, max_width, PLACEHOLDER_SIZE),
                )]
            }
        };
        node
    }

    /// The photo frame `id`'s picture, in its content `(x, y, width,
    /// height)`: an image named by its file, fitted as it is drawn -- or,
    /// with none up, a label saying why, where the frame says it. Nothing
    /// while the first picture is on its way, as nothing is drawn.
    fn picture_node(
        &self,
        id: WidgetInstanceId,
        (x, y, width, height): (f32, f32, f32, f32),
    ) -> Option<Node<WidgetPart>> {
        let state = self.frames.get(&id)?;
        if let Some(picture) = &state.shown {
            let (px, py, pw, ph) = fit_within(picture.width, picture.height, x, y, width, height);
            let name = picture.path.file_name().map_or_else(
                || pathcodec::display_path(&picture.path),
                pathcodec::display_os,
            );
            let mut node = Node::new(
                WidgetPart::Picture(id),
                Role::Image,
                name,
                Rect::new(px, py, pw, ph),
            );
            node.description = Some(pathcodec::display_path(&picture.path));
            return Some(node);
        }
        let problem = state.problem.as_ref()?;
        let (tx, ty, max_width) = problem_line(x, y, width, height);
        Some(Node::new(
            WidgetPart::Picture(id),
            Role::Label,
            problem.clone(),
            line_box(tx, ty, max_width, PROBLEM_SIZE),
        ))
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
