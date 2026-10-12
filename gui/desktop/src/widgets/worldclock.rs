//! The world clock widget: the time in each of the user's other zones -- the
//! clocks the Date & Time settings keep beside the main one, which the clock
//! popup shows too -- each with its label, its time, the day it is there
//! when that is another day, and how far ahead or behind it is: "Tokyo,
//! 09:30, Tomorrow, 13 hours ahead" (`design-decisions.md` §1600).
//!
//! One list of other clocks on the desktop: a clock added, hidden or
//! renamed in the settings is so on the popup and on the widget alike. With
//! none to show, the widget says where they are added.
//!
//! # One layout
//!
//! [`rows`] places the clocks in the widget's content -- as many as fit, and
//! a last line saying how many more (`widgets::listing`) -- and the drawing
//! ([`render`]) and what tools are told (`widgets::accessible`) both read
//! it.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};

use super::listing::{self, Rows, more_text};
use crate::Rect;
use crate::calendar::WorldClockReading;

/// What the widget says with no other clock to show.
pub const NO_CLOCKS: &str = "No other clocks: add them in Date & Time settings";

/// A clock's label: its size and weight.
const LABEL_SIZE: f32 = 12.0;
const LABEL_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// Its time, at the right of the label's line.
const TIME_SIZE: f32 = 16.0;
const TIME_WEIGHT: FontWeightHint = FontWeightHint::Bold;
/// Its second line: the day and the offset.
const DETAIL_SIZE: f32 = 11.0;
/// Room between two clocks.
const GAP: f32 = 6.0;

fn first_line_height() -> f32 {
    guitk::text::line_height(TIME_SIZE, TIME_WEIGHT)
}

fn row_height() -> f32 {
    first_line_height() + guitk::text::line_height(DETAIL_SIZE, FontWeightHint::Regular)
}

/// The clocks of `clocks` placed in `content`, top to bottom: as many as
/// fit, and -- when some do not -- a last line saying how many more.
#[must_use]
pub fn rows(clocks: &[WorldClockReading], content: Rect) -> Rows {
    listing::stack(
        clocks.len(),
        row_height(),
        GAP,
        guitk::text::line_height(DETAIL_SIZE, FontWeightHint::Regular),
        content,
    )
}

/// Draw `clocks` in `content`, in `p` at `alpha`: each clock's label and
/// time on one line, its day and offset under them, how many more did not
/// fit -- or, with none, where they are added.
pub fn render(
    clocks: &[WorldClockReading],
    content: Rect,
    p: &Palette,
    alpha: u8,
    commands: &mut Vec<RenderCommand>,
) {
    let wash = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
    let mut text = |(x, y, width): (f32, f32, f32),
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
    if clocks.is_empty() {
        text(
            (content.x, content.y, content.w),
            NO_CLOCKS.to_owned(),
            DETAIL_SIZE,
            FontWeightHint::Regular,
            p.subtext0,
        );
        return;
    }
    let laid = rows(clocks, content);
    for &(index, bounds) in &laid.rows {
        let Some(clock) = clocks.get(index) else {
            continue;
        };
        let time_w = guitk::text::measure(&clock.time, TIME_SIZE, TIME_WEIGHT).min(bounds.w);
        // The label beside the time, at the line's foot with it.
        let label_drop = first_line_height() - guitk::text::line_height(LABEL_SIZE, LABEL_WEIGHT);
        text(
            (bounds.x, bounds.y + label_drop, bounds.w - time_w - 6.0),
            clock.label.clone(),
            LABEL_SIZE,
            LABEL_WEIGHT,
            p.text,
        );
        text(
            (bounds.x + bounds.w - time_w, bounds.y, time_w),
            clock.time.clone(),
            TIME_SIZE,
            TIME_WEIGHT,
            p.text,
        );
        text(
            (bounds.x, bounds.y + first_line_height(), bounds.w),
            clock.detail(),
            DETAIL_SIZE,
            FontWeightHint::Regular,
            p.subtext1,
        );
    }
    if let Some((more, bounds)) = laid.more {
        text(
            (bounds.x, bounds.y, bounds.w),
            more_text(more),
            DETAIL_SIZE,
            FontWeightHint::Regular,
            p.subtext0,
        );
    }
}

#[cfg(test)]
#[path = "worldclock_tests.rs"]
mod tests;
