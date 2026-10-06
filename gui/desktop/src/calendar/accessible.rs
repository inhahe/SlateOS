//! The calendar popup as tools see it -- held among the shell's own parts by
//! `crate::accessible`, which presses its controls as the user would.
//!
//! In the month: the arrows, the title -- the month and year, which shows
//! the year -- "Today" where it is shown, and the six weeks of days, each
//! named by its date, chosen while its events are the ones shown, and said
//! to be today, to have events, or to be another month's; below them, the
//! card of the chosen day's events, each named by its title and described by
//! its time. In the year: its arrows, its title -- which shows the month
//! again -- and its twelve months.
//!
//! Every box is the one the popup is drawn in and takes its press in
//! ([`MonthLayout`], [`YearLayout`]), so a press at the middle of a part's
//! box is a press on the part.

use guitk::widget::automation::{Node, Role, Value};

use crate::Rect;

use super::{
    CalendarHit, CalendarView, CalendarViewMode, EVENT_HEADER_HEIGHT, EVENT_ROW_HEIGHT, EventStore,
    GRID_CELLS, MAX_VISIBLE_EVENTS, MonthLayout, YearLayout, day_of_week, day_of_week_name,
    month_name, timestamp_to_date,
};

/// A part of the calendar popup, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarPart {
    /// The popup.
    Popup,
    /// The month's days, or the year's months.
    Grid,
    /// The card of the chosen day's events.
    Events,
    /// One of them, by its place on the card.
    Event(usize),
    /// A control the popup's hit test names: an arrow, the title, "Today",
    /// a day, a month.
    Control(CalendarHit),
}

/// What a day is said to be, beside its date: today, its events, another
/// month's.
fn day_description(today: bool, events: usize, this_month: bool) -> Option<String> {
    let mut said = Vec::new();
    if today {
        said.push("today".to_owned());
    }
    match events {
        0 => {}
        1 => said.push("1 event".to_owned()),
        n => said.push(format!("{n} events")),
    }
    if !this_month {
        said.push("another month".to_owned());
    }
    (!said.is_empty()).then(|| said.join(", "))
}

impl CalendarView {
    /// Where the control `hit` is drawn and takes a press, for a popup whose
    /// top-left corner is at `(x, y)` drawn at `scale` -- `None` for one it
    /// does not show now: "Today" on today's month, a day in the year's
    /// overview, a month in the month's, any while the popup is closed.
    #[must_use]
    pub fn control_rect(&self, hit: CalendarHit, x: f32, y: f32, scale: f32) -> Option<Rect> {
        if !self.visible {
            return None;
        }
        match self.mode {
            CalendarViewMode::Month => {
                let layout = MonthLayout::new(self, x, y, scale);
                match hit {
                    CalendarHit::PrevPage => Some(layout.prev_arrow()),
                    CalendarHit::NextPage => Some(layout.next_arrow()),
                    CalendarHit::Title => Some(layout.title()),
                    CalendarHit::Today => layout.today_button(),
                    CalendarHit::Day(index) => (index < GRID_CELLS).then(|| layout.cell(index)),
                    CalendarHit::Month(_) => None,
                    CalendarHit::Panel => Some(layout.frame),
                }
            }
            CalendarViewMode::Year => {
                let layout = YearLayout::new(x, y, scale);
                match hit {
                    CalendarHit::PrevPage => Some(layout.prev_arrow()),
                    CalendarHit::NextPage => Some(layout.next_arrow()),
                    CalendarHit::Title => Some(layout.title()),
                    CalendarHit::Month(month @ 1..=12) => {
                        let index = usize::try_from(month.saturating_sub(1)).ok()?;
                        Some(layout.month(index))
                    }
                    CalendarHit::Month(_) | CalendarHit::Today | CalendarHit::Day(_) => None,
                    CalendarHit::Panel => Some(layout.frame),
                }
            }
        }
    }

    /// The popup as tools see it, its top-left corner at `(x, y)` drawn at
    /// `scale`, its days' events from `store`.
    #[must_use]
    pub fn automation(&self, x: f32, y: f32, scale: f32, store: &EventStore) -> Node<CalendarPart> {
        let control = |hit: CalendarHit, role: Role, name: String| {
            let bounds = self
                .control_rect(hit, x, y, scale)
                .unwrap_or(Rect::new(x, y, 0.0, 0.0));
            Node::new(CalendarPart::Control(hit), role, name, bounds)
        };
        match self.mode {
            CalendarViewMode::Month => {
                let layout = MonthLayout::new(self, x, y, scale);
                let mut popup =
                    Node::new(CalendarPart::Popup, Role::Dialog, "Calendar", layout.frame);
                let title = format!("{} {}", month_name(self.view_month), self.view_year);
                popup.children.push(control(
                    CalendarHit::PrevPage,
                    Role::Button,
                    "Previous month".to_owned(),
                ));
                let mut heading = control(CalendarHit::Title, Role::Button, title.clone());
                heading.description = Some("shows the year".to_owned());
                popup.children.push(heading);
                popup.children.push(control(
                    CalendarHit::NextPage,
                    Role::Button,
                    "Next month".to_owned(),
                ));
                if layout.today_button().is_some() {
                    popup.children.push(control(
                        CalendarHit::Today,
                        Role::Button,
                        "Today".to_owned(),
                    ));
                }
                let first = layout.cell(0);
                let last = layout.cell(GRID_CELLS.saturating_sub(1));
                let grid_box = Rect::new(
                    first.x,
                    first.y,
                    last.right() - first.x,
                    last.bottom() - first.y,
                );
                let mut grid = Node::new(CalendarPart::Grid, Role::Grid, title, grid_box);
                for (index, cell) in self.generate_grid().into_iter().enumerate() {
                    let date = (cell.year, cell.month, cell.day);
                    let name = format!(
                        "{} {} {} {}",
                        day_of_week_name(day_of_week(cell.year, cell.month, cell.day)),
                        cell.day,
                        month_name(cell.month),
                        cell.year
                    );
                    let mut day = control(CalendarHit::Day(index), Role::GridCell, name);
                    let events = store.events_for_date(cell.year, cell.month, cell.day).len();
                    day.description =
                        day_description(date == self.today, events, cell.current_month);
                    day.value = Some(Value::Chosen(self.selected_date == Some(date)));
                    day.focused = self.selected_date == Some(date);
                    day.focusable = true;
                    grid.children.push(day);
                }
                popup.children.push(grid);
                if let Some(card) = self.events_node(&layout, store) {
                    popup.children.push(card);
                }
                popup
            }
            CalendarViewMode::Year => {
                let layout = YearLayout::new(x, y, scale);
                let mut popup =
                    Node::new(CalendarPart::Popup, Role::Dialog, "Calendar", layout.frame);
                let year = self.view_year.to_string();
                popup.children.push(control(
                    CalendarHit::PrevPage,
                    Role::Button,
                    "Previous year".to_owned(),
                ));
                let mut heading = control(CalendarHit::Title, Role::Button, year.clone());
                heading.description = Some("shows the month".to_owned());
                popup.children.push(heading);
                popup.children.push(control(
                    CalendarHit::NextPage,
                    Role::Button,
                    "Next year".to_owned(),
                ));
                let first = layout.month(0);
                let last = layout.month(11);
                let grid_box = Rect::new(
                    first.x,
                    first.y,
                    last.right() - first.x,
                    last.bottom() - first.y,
                );
                let mut grid = Node::new(CalendarPart::Grid, Role::Grid, year, grid_box);
                for month in 1..=12_u32 {
                    let mut cell = control(
                        CalendarHit::Month(month),
                        Role::GridCell,
                        month_name(month).to_owned(),
                    );
                    cell.value = Some(Value::Chosen(month == self.view_month));
                    cell.focusable = true;
                    grid.children.push(cell);
                }
                popup.children.push(grid);
                popup
            }
        }
    }

    /// The card of the chosen day's events, while it is shown: the events it
    /// shows, each named by its title and described by its time.
    fn events_node(&self, layout: &MonthLayout, store: &EventStore) -> Option<Node<CalendarPart>> {
        let (year, month, day) = self.selected_date?;
        let events = store.events_for_date(year, month, day);
        let card = self.detail_rect(layout, store)?;
        let mut node = Node::new(
            CalendarPart::Events,
            Role::List,
            format!("Events on {day} {}", month_name(month)),
            card,
        );
        if events.len() > MAX_VISIBLE_EVENTS {
            let more = events.len().saturating_sub(MAX_VISIBLE_EVENTS);
            node.description = Some(format!("{more} more not shown"));
        }
        let row_h = layout.px(EVENT_ROW_HEIGHT);
        let mut top = card.y + layout.px(EVENT_HEADER_HEIGHT);
        for (index, event) in events.iter().take(MAX_VISIBLE_EVENTS).enumerate() {
            let mut row = Node::new(
                CalendarPart::Event(index),
                Role::ListItem,
                event.title.clone(),
                Rect::new(card.x, top, card.w, row_h),
            );
            let (_, _, _, hour, minute, _) = timestamp_to_date(event.start_timestamp);
            row.description = Some(if event.all_day {
                "All day".to_owned()
            } else {
                format!("{hour:02}:{minute:02}")
            });
            node.children.push(row);
            top += row_h;
        }
        Some(node)
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
