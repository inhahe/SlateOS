//! Tests for the calendar popup as tools see it: its controls, its days and
//! months, the chosen day's events, and that every control's box is where
//! the popup takes its press.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::calendar::{CalendarConfig, CalendarEvent, date_to_timestamp};

/// Where the popup is drawn, and at what scaling.
const X: f32 = 1500.0;
const Y: f32 = 400.0;
const SCALE: f32 = 1.0;

/// An open calendar on Tuesday 6 October 2026.
fn calendar() -> CalendarView {
    let mut view = CalendarView::new(CalendarConfig::default());
    view.set_today(2026, 10, 6);
    view.set_visible(true);
    view
}

/// An event titled `title` at `hour:minute` on a day of October 2026.
fn event(title: &str, day: u32, hour: u32, minute: u32) -> CalendarEvent {
    let start = date_to_timestamp(2026, 10, day, hour, minute, 0).unwrap();
    CalendarEvent {
        id: 0,
        title: title.to_owned(),
        start_timestamp: start,
        end_timestamp: start + 3600,
        all_day: false,
        repeat: None,
        color: None,
        description: String::new(),
    }
}

/// The popup as tools see it.
fn tree(view: &CalendarView, store: &EventStore) -> Node<CalendarPart> {
    view.automation(X, Y, SCALE, store)
}

/// The node of `part`.
fn node(view: &CalendarView, store: &EventStore, part: CalendarPart) -> Node<CalendarPart> {
    tree(view, store)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// The place in the month's grid of the day `day` of the month shown.
fn day_index(view: &CalendarView, day: u32) -> usize {
    view.generate_grid()
        .iter()
        .position(|cell| cell.current_month && cell.day == day)
        .unwrap()
}

/// Every control's box is where the popup's own hit test finds it.
fn controls_answer_where_they_are(view: &CalendarView, store: &EventStore) {
    for node in tree(view, store).walk() {
        if let CalendarPart::Control(hit) = node.id {
            let (x, y) = node.bounds.centre();
            assert_eq!(
                view.hit_test(X, Y, SCALE, x, y, store),
                Some(hit),
                "{} at {:?}",
                node.name,
                node.bounds
            );
        }
    }
}

/// **The month shows tools its controls and its days**: the arrows, the
/// title, the six weeks -- each day named by its date and said to be today
/// or another month's -- and no "Today" on today's month.
#[test]
fn the_month_shows_tools_its_controls_and_its_days() {
    let view = calendar();
    let store = EventStore::new();
    let root = tree(&view, &store);
    assert_eq!((root.role, root.name.as_str()), (Role::Dialog, "Calendar"));
    let names: Vec<(Role, &str)> = root
        .children
        .iter()
        .map(|n| (n.role, n.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (Role::Button, "Previous month"),
            (Role::Button, "October 2026"),
            (Role::Button, "Next month"),
            (Role::Grid, "October 2026"),
        ]
    );
    let grid = &root.children[3];
    assert_eq!(grid.children.len(), GRID_CELLS);
    let today = &grid.children[day_index(&view, 6)];
    assert_eq!(today.name, "Tuesday 6 October 2026");
    assert_eq!(today.role, Role::GridCell);
    assert_eq!(today.description.as_deref(), Some("today"));
    assert_eq!(today.value, Some(Value::Chosen(false)));
    let outside = grid
        .children
        .iter()
        .zip(view.generate_grid())
        .find(|(_, cell)| !cell.current_month)
        .map(|(node, _)| node)
        .expect("a day of another month");
    assert_eq!(outside.description.as_deref(), Some("another month"));
    controls_answer_where_they_are(&view, &store);
}

/// **A day with events says how many, and chosen, the card of its events
/// lists them**, each by its title and its time.
#[test]
fn a_chosen_days_events_are_listed() {
    let mut view = calendar();
    let mut store = EventStore::new();
    store.add_event(event("Dentist", 10, 9, 30)).unwrap();
    store.add_event(event("Call Mum", 10, 18, 0)).unwrap();
    store.add_event(event("Haircut", 12, 11, 0)).unwrap();
    let tenth = CalendarPart::Control(CalendarHit::Day(day_index(&view, 10)));
    assert_eq!(
        node(&view, &store, tenth).description.as_deref(),
        Some("2 events")
    );
    let twelfth = CalendarPart::Control(CalendarHit::Day(day_index(&view, 12)));
    assert_eq!(
        node(&view, &store, twelfth).description.as_deref(),
        Some("1 event")
    );

    assert!(view.apply(CalendarHit::Day(day_index(&view, 10))));
    let chosen = node(&view, &store, tenth);
    assert_eq!(chosen.value, Some(Value::Chosen(true)));
    assert!(chosen.focused);
    let card = node(&view, &store, CalendarPart::Events);
    assert_eq!(
        (card.role, card.name.as_str()),
        (Role::List, "Events on 10 October")
    );
    let rows: Vec<(&str, Option<&str>)> = card
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.description.as_deref()))
        .collect();
    assert_eq!(
        rows,
        [("Dentist", Some("09:30")), ("Call Mum", Some("18:00"))]
    );
    // The card takes a press as the popup's own, wherever on it.
    let (x, y) = card.children[1].bounds.centre();
    assert_eq!(
        view.hit_test(X, Y, SCALE, x, y, &store),
        Some(CalendarHit::Panel)
    );
    controls_answer_where_they_are(&view, &store);
}

/// **Away from today's month, "Today" is shown, and is where it is pressed.**
#[test]
fn today_is_shown_away_from_todays_month() {
    let mut view = calendar();
    let store = EventStore::new();
    assert_eq!(view.control_rect(CalendarHit::Today, X, Y, SCALE), None);
    view.apply(CalendarHit::NextPage);
    let names: Vec<String> = tree(&view, &store)
        .children
        .iter()
        .map(|n| n.name.clone())
        .collect();
    assert!(names.contains(&"Today".to_owned()), "{names:?}");
    assert!(names.contains(&"November 2026".to_owned()), "{names:?}");
    controls_answer_where_they_are(&view, &store);
}

/// **The year shows its arrows, its title and its twelve months**, the
/// month shown chosen; each where the popup takes its press.
#[test]
fn the_year_shows_its_months() {
    let mut view = calendar();
    let store = EventStore::new();
    view.apply(CalendarHit::Title);
    let root = tree(&view, &store);
    let names: Vec<&str> = root.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Previous year", "2026", "Next year", "2026"]);
    let months = &root.children[3];
    assert_eq!(months.role, Role::Grid);
    assert_eq!(months.children.len(), 12);
    assert_eq!(months.children[9].name, "October");
    assert_eq!(months.children[9].value, Some(Value::Chosen(true)));
    assert_eq!(months.children[0].value, Some(Value::Chosen(false)));
    controls_answer_where_they_are(&view, &store);
}

/// **What the popup does not show has no box**: a month in the month's
/// view, a day or "Today" in the year's, a day past the grid, anything of a
/// closed popup.
#[test]
fn what_the_popup_does_not_show_has_no_box() {
    let mut view = calendar();
    for hit in [
        CalendarHit::Month(3),
        CalendarHit::Today,
        CalendarHit::Day(GRID_CELLS),
    ] {
        assert_eq!(view.control_rect(hit, X, Y, SCALE), None, "{hit:?}");
    }
    view.apply(CalendarHit::Title);
    for hit in [
        CalendarHit::Day(0),
        CalendarHit::Today,
        CalendarHit::Month(0),
        CalendarHit::Month(13),
    ] {
        assert_eq!(view.control_rect(hit, X, Y, SCALE), None, "{hit:?}");
    }
    view.set_visible(false);
    assert_eq!(view.control_rect(CalendarHit::Title, X, Y, SCALE), None);
}
