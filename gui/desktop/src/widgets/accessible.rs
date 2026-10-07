//! The desktop's widgets as tools see them -- a screen reader, a script:
//! each widget a group named by its title, holding what it shows.
//!
//! | widget | what is in it |
//! |---|---|
//! | clock | its time and its date, each a label, read as the taskbar's clock reads them |
//! | system monitor | a progress bar for each meter -- CPU, memory, disk -- or, for one nothing has measured, no value and "not measured" |
//! | note | a text area holding its text, the one being written in having the keyboard |
//! | photo frame | the picture up on it, named by its file, or what it says in a picture's place |
//! | battery | its charge, or "No battery", described by what its icon shows and the time left |
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

use super::{
    BATTERY_CHARGE, CLOCK_TIME, DesktopWidgetManager, LiveReadings, METER_PITCH,
    METER_TROUGH_HEIGHT, METER_TROUGH_TOP, Meter, NOT_MEASURED, PLACEHOLDER_SIZE, PROBLEM_SIZE,
    WidgetInstance, WidgetInstanceId, WidgetKind, charge_text, clock_date, estimate_text,
    fit_within, placeholder_line, problem_line,
};
use crate::Rect;
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
    /// A note's writing area: written in as its user writes in it.
    Note(WidgetInstanceId),
    /// A photo frame's picture, or what the frame says in a picture's place.
    Picture(WidgetInstanceId),
    /// A battery widget's charge.
    Charge(WidgetInstanceId),
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
                let mut top = cy;
                let mut meters = Vec::new();
                for meter in Meter::ALL {
                    let mut node = Node::new(
                        WidgetPart::Meter(id, meter),
                        Role::ProgressBar,
                        meter.label(),
                        Rect::new(cx, top, cw, METER_TROUGH_TOP + METER_TROUGH_HEIGHT),
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
                    meters.push(node);
                    top += METER_PITCH;
                }
                meters
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
