//! Tests for the upcoming-events widget: where its days and events go, what
//! it says of those that do not fit or of nothing at all, and what it draws
//! them in.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::calendar::{EventMark, UpcomingDay, UpcomingEvent};
use appearance::palette_check::assert_drawn_from;

/// A colour a user gave an event, none of the palette's.
const CHOSEN: Color = Color::rgb(200, 40, 40);

/// A veil no default gives a widget, so no wash is right by accident.
const ODD_ALPHA: u8 = 137;

/// An event `title` at `time`, in its category's colour or `chosen`.
fn event(title: &str, time: &str, chosen: Option<Color>) -> UpcomingEvent {
    UpcomingEvent {
        title: title.to_owned(),
        time: time.to_owned(),
        mark: EventMark {
            chosen,
            category: calendarstore::EventCategory::Personal,
        },
    }
}

/// Days `labels`, each with `per_day` events.
fn week(labels: &[&str], per_day: usize) -> Upcoming {
    Upcoming {
        days: labels
            .iter()
            .map(|label| UpcomingDay {
                label: (*label).to_owned(),
                events: (0..per_day)
                    .map(|i| event(&format!("{label} {i}"), "09:00", None))
                    .collect(),
            })
            .collect(),
    }
}

/// A large widget's content, two cells by two less its title bar.
fn large() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 258.0)
}

/// A wide widget's content, one cell high.
fn wide() -> Rect {
    Rect::new(100.0, 200.0, 356.0, 96.0)
}

fn events_shown(laid: &Lines) -> usize {
    laid.lines
        .iter()
        .filter(|line| matches!(line, Line::Event { .. }))
        .count()
}

fn bounds_of(line: &Line) -> Rect {
    match line {
        Line::Day { bounds, .. } | Line::Event { bounds, .. } => *bounds,
    }
}

/// **What fits is listed a day at a time, top to bottom, in the content**:
/// each day's heading, then its events; nothing more to say.
#[test]
fn a_week_that_fits_is_listed_whole() {
    let upcoming = week(&["Today", "Tomorrow"], 2);
    let laid = lines(&upcoming, large());
    let kinds: Vec<(&str, usize, Option<usize>)> = laid
        .lines
        .iter()
        .map(|line| match *line {
            Line::Day { day, .. } => ("day", day, None),
            Line::Event { day, event, .. } => ("event", day, Some(event)),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("day", 0, None),
            ("event", 0, Some(0)),
            ("event", 0, Some(1)),
            ("day", 1, None),
            ("event", 1, Some(0)),
            ("event", 1, Some(1)),
        ]
    );
    assert_eq!(laid.more, None);
    // Stacked without a gap that overlaps, inside the content.
    let content = large();
    let mut bottom = content.y;
    for line in &laid.lines {
        let b = bounds_of(line);
        assert!(b.y >= bottom, "{line:?} overlaps the line above");
        assert!(
            b.y + b.h <= content.y + content.h,
            "{line:?} below the content"
        );
        bottom = b.y + b.h;
    }
    // Numbered through the list, for tools.
    let indices: Vec<usize> = laid
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Event { index, .. } => Some(*index),
            Line::Day { .. } => None,
        })
        .collect();
    assert_eq!(indices, [0, 1, 2, 3]);
}

/// **What does not fit is counted on a last line, inside the content**, and
/// a day's heading is never the last line: a heading with no event under it
/// says nothing.
#[test]
fn what_does_not_fit_is_counted() {
    let upcoming = week(&["Today", "Tomorrow", "Friday 9 October"], 3);
    let content = wide();
    let laid = lines(&upcoming, content);
    let shown = events_shown(&laid);
    assert!(shown > 0, "nothing fitted a wide widget");
    let (more, at) = laid.more.expect("nine events fitted a wide widget");
    assert_eq!(more, 9 - shown);
    assert!(
        at.y + at.h <= content.y + content.h,
        "the count is below the content"
    );
    let last = laid.lines.last().expect("some lines");
    assert!(
        matches!(last, Line::Event { .. }),
        "a heading with nothing under it"
    );
    assert!(
        bounds_of(last).y + bounds_of(last).h <= at.y,
        "the count overlaps"
    );
    assert_eq!(more_text(more), format!("{more} more"));
}

/// **Nothing coming says so** -- in words, not an empty box.
#[test]
fn nothing_coming_says_so() {
    let p = Palette::for_mode(false);
    let mut commands = Vec::new();
    render(&Upcoming::default(), large(), &p, 255, &mut commands);
    let said: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, [NOTHING_COMING]);
}

/// **Each event is drawn as listed** -- its bar in its colour, its time and
/// its title on its line -- and the days' headings and the count with them.
#[test]
fn each_event_is_drawn_on_its_line() {
    let p = Palette::for_mode(false);
    let mut upcoming = week(&["Today"], 1);
    upcoming.days[0]
        .events
        .push(event("Dentist", "14:30", Some(CHOSEN)));
    let mut commands = Vec::new();
    render(&upcoming, large(), &p, 255, &mut commands);
    let laid = lines(&upcoming, large());
    let texts: Vec<(&str, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text { text, y, .. } => Some((text.as_str(), *y)),
            _ => None,
        })
        .collect();
    let line_of = |index: usize| bounds_of(&laid.lines[index]);
    assert!(texts.contains(&("Today", line_of(0).y)), "{texts:?}");
    assert!(texts.contains(&("14:30", line_of(2).y + 1.0)), "{texts:?}");
    assert!(
        texts.contains(&("Dentist", line_of(2).y + 1.0)),
        "{texts:?}"
    );
    let bars: Vec<Color> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(bars, [p.green, CHOSEN], "a bar in another colour");
}

/// **Everything it draws is its palette's**, in both modes, at any veil --
/// but a colour the user gave an event, which is theirs.
#[test]
fn every_colour_it_draws_comes_from_its_palette() {
    for light in [false, true] {
        let mut p = Palette::for_mode(light);
        p.accent = appearance::PINK;
        let mut upcoming = week(&["Today", "Tomorrow", "Friday 9 October"], 3);
        upcoming.days[0].events[0].mark.chosen = Some(CHOSEN);
        for shown in [upcoming, Upcoming::default()] {
            for content in [large(), wide()] {
                let mut commands = Vec::new();
                render(&shown, content, &p, ODD_ALPHA, &mut commands);
                assert_drawn_from(
                    &p,
                    &commands,
                    &[CHOSEN],
                    &format!("the upcoming events (light={light})"),
                );
            }
        }
    }
}
