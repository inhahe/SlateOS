//! Tests for a widget's own menu as a user meets it: a right-click on the
//! widget offers "Size" -- the sizes its kind can be drawn at, the one it is
//! ticked, one it would not fit at greyed -- and choosing one resizes it in
//! place, the layout to be saved.
//!
//! The widget layer's tests hold which sizes a kind has and whether one
//! fits; these hold the shell's part: the menu built from them and the
//! choice carried out.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::DesktopShell;
use crate::widgets::{GridPos, WidgetInstanceId, WidgetKind, WidgetSize};
use guitk::menu::MenuItem;

/// A shell with a widget of `kind` at the grid's cell `at`.
fn shell_with(kind: WidgetKind, at: GridPos) -> (DesktopShell, WidgetInstanceId) {
    let mut shell = DesktopShell::new(1600, 1000);
    let id = shell.widgets.add_widget(kind, at).expect("placed");
    (shell, id)
}

/// The desktop menu opened on the middle of widget `id`: its "Size" rows,
/// as `(label, ticked, usable)`, or `None` with no "Size".
fn sizes_on(shell: &mut DesktopShell, id: WidgetInstanceId) -> Option<Vec<(String, bool, bool)>> {
    let (x, y, w, h) = shell.widgets.content_rect(id).expect("placed");
    shell.open_desktop_menu(x + w / 2.0, y + h / 2.0);
    shell
        .desktop_menu
        .items()
        .iter()
        .find_map(|item| match item {
            MenuItem::Submenu {
                label, children, ..
            } if label == "Size" => Some(
                children
                    .iter()
                    .filter_map(|row| match row {
                        MenuItem::Action {
                            label,
                            checked,
                            enabled,
                            ..
                        } => Some((label.clone(), *checked == Some(true), *enabled)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
}

/// **A clock's menu offers its sizes, the one it is ticked**, and choosing
/// the other draws it there, in place, the layout to be saved.
#[test]
fn a_widget_is_resized_from_its_menu() {
    let (mut shell, clock) = shell_with(WidgetKind::Clock, GridPos::new(0, 0));
    let sizes = sizes_on(&mut shell, clock).expect("no Size on a clock's menu");
    assert_eq!(
        sizes,
        [
            ("Small".to_owned(), true, true),
            ("Wide".to_owned(), false, true)
        ]
    );
    let _ = shell.take_widgets_dirty();
    let wide = DesktopShell::widget_size_menu_id(WidgetSize::WIDE).expect("a row for Wide");
    assert!(shell.activate_desktop_menu_item(wide).changed());
    let placed = shell.widgets.get(clock).expect("still out");
    assert_eq!(
        (placed.size, placed.position),
        (WidgetSize::WIDE, GridPos::new(0, 0))
    );
    assert!(shell.take_widgets_dirty(), "the new size is not saved");

    // The size it is already: nothing to do, and nothing to save.
    let sizes = sizes_on(&mut shell, clock).expect("Size");
    assert_eq!(sizes[1], ("Wide".to_owned(), true, true), "{sizes:?}");
    assert!(!shell.activate_desktop_menu_item(wide).changed());
    assert!(!shell.take_widgets_dirty(), "no change was saved");
}

/// **A size the widget would not fit at is greyed** -- it would cover its
/// neighbour, or run off the grid -- and choosing it anyway changes nothing.
#[test]
fn a_size_that_does_not_fit_is_greyed() {
    let (mut shell, clock) = shell_with(WidgetKind::Clock, GridPos::new(0, 0));
    shell
        .widgets
        .add_widget(WidgetKind::Clock, GridPos::new(1, 0))
        .expect("free");
    let sizes = sizes_on(&mut shell, clock).expect("Size");
    assert_eq!(sizes[1], ("Wide".to_owned(), false, false), "{sizes:?}");
    let wide = DesktopShell::widget_size_menu_id(WidgetSize::WIDE).expect("a row for Wide");
    assert!(!shell.activate_desktop_menu_item(wide).changed());
    assert_eq!(
        shell.widgets.get(clock).map(|w| w.size),
        Some(WidgetSize::SMALL)
    );

    // At the grid's last column, Wide would run off it.
    let (mut edge, last) = shell_with(WidgetKind::Clock, GridPos::new(7, 0));
    let sizes = sizes_on(&mut edge, last).expect("Size");
    assert_eq!(sizes[1], ("Wide".to_owned(), false, false), "{sizes:?}");
}

/// **A widget drawn at one size only has no "Size"**: a kind with nothing
/// of its own to show yet.
#[test]
fn a_widget_of_one_size_offers_none() {
    let (mut shell, weather) = shell_with(WidgetKind::Weather, GridPos::new(0, 0));
    assert_eq!(sizes_on(&mut shell, weather), None);
}

/// **The calendar is offered the sizes a month fits in**, and no smaller.
#[test]
fn a_calendar_is_offered_the_sizes_a_month_fits() {
    let (mut shell, calendar) = shell_with(WidgetKind::Calendar, GridPos::new(0, 0));
    let labels: Vec<String> = sizes_on(&mut shell, calendar)
        .expect("Size")
        .into_iter()
        .map(|(label, _, _)| label)
        .collect();
    assert_eq!(labels, ["Large", "Extra large"]);
}
