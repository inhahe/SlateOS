//! The upcoming-events widget: the calendar's events from now over the next
//! week, a day at a time -- "Today", "Tomorrow", "Friday 9 October" -- each
//! with its time and the bar of its colour, as the clock popup's card shows
//! a day's.
//!
//! The events are the calendar program's, as the popup's and the calendar
//! widget's are (`design-decisions.md` §1497): the shell gathers them
//! ([`crate::calendar::Agenda::upcoming`]) while one of these is out, and
//! the widget turns its minute so that an event over drops away and
//! midnight moves the days along. The kind was declared as `Reminders`
//! ("upcoming events/reminders") from the start and drew only its name; its
//! spelling in `widgets.yaml` stays `reminders`.
//!
//! # One layout
//!
//! [`lines`] places the days and the events in the widget's content -- as
//! many as fit, and a last line saying how many more there are -- and the
//! drawing ([`render`]) and what tools are told (`widgets::accessible`) both
//! read it.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

use super::listing::more_text;
use crate::Rect;
use crate::calendar::Upcoming;

/// What the widget says with nothing coming up.
pub const NOTHING_COMING: &str = "Nothing in the next week";

/// A day's heading: its size and weight.
const DAY_SIZE: f32 = 11.0;
const DAY_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// An event's line: its size.
const EVENT_SIZE: f32 = 12.0;
/// Room above a day's heading, past the first.
const DAY_GAP: f32 = 4.0;
/// How wide an event's time is given, between its bar and its title: room
/// for "All day" at its size.
const TIME_WIDTH: f32 = 46.0;
/// The bar's width, and the room between it and the time.
const BAR_WIDTH: f32 = 3.0;
const BAR_ROOM: f32 = 6.0;

fn day_height() -> f32 {
    guitk::text::line_height(DAY_SIZE, DAY_WEIGHT)
}

fn event_height() -> f32 {
    guitk::text::line_height(EVENT_SIZE, FontWeightHint::Regular) + 2.0
}

/// A line of the list, where [`lines`] put it.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// Day `day`'s heading, its box.
    Day { day: usize, bounds: Rect },
    /// Event `event` of day `day` -- `index` among every event listed --
    /// its box.
    Event {
        day: usize,
        event: usize,
        index: usize,
        bounds: Rect,
    },
}

/// The list as laid out in a widget's content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    /// The lines that fit, top to bottom.
    pub lines: Vec<Line>,
    /// How many events did not fit, and the box of the line that says so;
    /// `None` when every one did.
    pub more: Option<(usize, Rect)>,
}

/// The days and events of `upcoming` placed in `content`: as many as fit,
/// each day's heading only with room for an event under it, and -- when
/// some do not fit -- a last line saying how many more.
#[must_use]
pub fn lines(upcoming: &Upcoming, content: Rect) -> Lines {
    let total = upcoming.events().count();
    let all = place(upcoming, content, content.y + content.h);
    let shown = all
        .iter()
        .filter(|line| matches!(line, Line::Event { .. }))
        .count();
    if shown == total {
        return Lines {
            lines: all,
            more: None,
        };
    }
    // Room for the last line, and the rest again above it.
    let more_h = event_height();
    let bottom = content.y + content.h - more_h;
    let lines = place(upcoming, content, bottom);
    let shown = lines
        .iter()
        .filter(|line| matches!(line, Line::Event { .. }))
        .count();
    let top = lines.last().map_or(content.y, |line| match line {
        Line::Day { bounds, .. } | Line::Event { bounds, .. } => bounds.y + bounds.h,
    });
    Lines {
        lines,
        more: Some((
            total.saturating_sub(shown),
            Rect::new(content.x, top, content.w, more_h),
        )),
    }
}

/// The lines of `upcoming` that fit in `content` above `bottom`.
fn place(upcoming: &Upcoming, content: Rect, bottom: f32) -> Vec<Line> {
    let (day_h, event_h) = (day_height(), event_height());
    let mut lines = Vec::new();
    let mut top = content.y;
    let mut index = 0_usize;
    'days: for (d, day) in upcoming.days.iter().enumerate() {
        let gap = if d == 0 { 0.0 } else { DAY_GAP };
        // A heading with no event under it says nothing.
        if top + gap + day_h + event_h > bottom {
            break;
        }
        top += gap;
        lines.push(Line::Day {
            day: d,
            bounds: Rect::new(content.x, top, content.w, day_h),
        });
        top += day_h;
        for e in 0..day.events.len() {
            if top + event_h > bottom {
                break 'days;
            }
            lines.push(Line::Event {
                day: d,
                event: e,
                index,
                bounds: Rect::new(content.x, top, content.w, event_h),
            });
            index = index.saturating_add(1);
            top += event_h;
        }
    }
    lines
}

/// Draw `upcoming` in `content`, in `p` at `alpha`: each day's heading,
/// each event's bar, time and title, how many more did not fit -- or, with
/// nothing coming, that there is nothing.
pub fn render(
    upcoming: &Upcoming,
    content: Rect,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let wash = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
    let text = |commands: &mut Vec<RenderCommand>,
                (x, y, width): (f32, f32, f32),
                said: String,
                size: f32,
                weight: FontWeightHint,
                ink: Color| {
        commands.push(RenderCommand::Text {
            x,
            y,
            text: said,
            font_size: size,
            color: wash(ink),
            font_weight: weight,
            max_width: Some(width.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    };
    if upcoming.days.is_empty() {
        text(
            commands,
            (content.x, content.y, content.w),
            NOTHING_COMING.to_owned(),
            EVENT_SIZE,
            FontWeightHint::Regular,
            p.subtext0,
        );
        return;
    }
    let laid = lines(upcoming, content);
    for line in &laid.lines {
        match *line {
            Line::Day { day, bounds } => {
                let Some(day) = upcoming.days.get(day) else {
                    continue;
                };
                text(
                    commands,
                    (bounds.x, bounds.y, bounds.w),
                    day.label.clone(),
                    DAY_SIZE,
                    DAY_WEIGHT,
                    p.subtext1,
                );
            }
            Line::Event {
                day, event, bounds, ..
            } => {
                let Some(event) = upcoming.days.get(day).and_then(|d| d.events.get(event)) else {
                    continue;
                };
                commands.push(RenderCommand::FillRect {
                    x: bounds.x,
                    y: bounds.y + 3.0,
                    width: BAR_WIDTH,
                    height: (bounds.h - 6.0).max(0.0),
                    color: wash(event.mark.color(p)),
                    corner_radii: CornerRadii::all(1.5),
                });
                let time_x = bounds.x + BAR_WIDTH + BAR_ROOM;
                text(
                    commands,
                    (time_x, bounds.y + 1.0, TIME_WIDTH),
                    event.time.clone(),
                    EVENT_SIZE - 1.0,
                    FontWeightHint::Regular,
                    p.subtext1,
                );
                let title_x = time_x + TIME_WIDTH;
                text(
                    commands,
                    (title_x, bounds.y + 1.0, bounds.x + bounds.w - title_x),
                    event.title.clone(),
                    EVENT_SIZE,
                    FontWeightHint::Regular,
                    p.text,
                );
            }
        }
    }
    if let Some((more, bounds)) = laid.more {
        text(
            commands,
            (bounds.x + BAR_WIDTH + BAR_ROOM, bounds.y + 1.0, bounds.w),
            more_text(more),
            EVENT_SIZE - 1.0,
            FontWeightHint::Regular,
            p.subtext0,
        );
    }
}

#[cfg(test)]
#[path = "upcoming_tests.rs"]
mod tests;
