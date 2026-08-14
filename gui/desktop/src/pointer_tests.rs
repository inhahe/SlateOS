//! Tests for the shell's pointer handling — hit testing, the start menu, the
//! taskbar and the window decorations.
//!
//! In its own file rather than inline in `main.rs` only because of size: the
//! shell's chrome has more clickable parts than it has palette rules, and the
//! two test modules were beginning to bury the code between them.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::launcher::{self, Category};
use crate::{
    DesktopShell, Hit, MouseButton, MouseEvent, MouseEventKind, Rect, START_MENU_ROW_HEIGHT,
    ShellAction, WindowState, click, scroll, scroll_rows,
};

fn shell() -> DesktopShell {
    DesktopShell::new(1000, 800)
}

/// The centre of a rectangle — where a user aiming at a control clicks.
fn centre(rect: Rect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

fn click_at(shell: &mut DesktopShell, rect: Rect) -> ShellAction {
    let (x, y) = centre(rect);
    shell.handle_mouse(&click(x, y))
}

// ---- the start menu -------------------------------------------------------

#[test]
fn the_start_button_opens_and_closes_the_menu() {
    let mut shell = shell();
    let start = shell.start_button_rect();
    assert!(!shell.start_menu_open);
    assert_eq!(click_at(&mut shell, start), ShellAction::Consumed);
    assert!(shell.start_menu_open);
    assert_eq!(click_at(&mut shell, start), ShellAction::Consumed);
    assert!(!shell.start_menu_open);
}

/// The point of the whole exercise: the Settings entry a user sees in the start
/// menu has to actually start the Settings application.
#[test]
fn clicking_settings_in_the_start_menu_asks_for_the_settings_program() {
    let mut shell = shell();
    shell.toggle_start_menu();

    let row = shell
        .start_menu_entries()
        .iter()
        .position(|entry| entry.name == "Settings")
        .expect("the start menu must offer Settings");
    assert!(
        row < shell.start_menu_visible_rows(),
        "Settings must be reachable without scrolling first"
    );

    let rect = shell.start_menu_row_rect(row);
    let action = click_at(&mut shell, rect);
    assert_eq!(action, ShellAction::Launch("/usr/bin/settings".to_string()));
    // Picking something dismisses the menu; a menu still open over the program
    // it just started is nobody's idea of a launcher.
    assert!(!shell.start_menu_open);
}

/// One list of applications, two front ends. The start menu used to carry its
/// own six names, one of which ("System Monitor") named no program the launcher
/// had ever heard of.
#[test]
fn the_start_menu_offers_only_programs_the_launcher_knows() {
    let shell = shell();
    let database = launcher::builtin_app_database();
    assert!(!shell.start_menu_entries().is_empty());
    for entry in shell.start_menu_entries() {
        assert!(
            database
                .iter()
                .any(|known| known.executable_path == entry.executable_path),
            "{} is offered by the start menu but is in no database",
            entry.name
        );
        assert_ne!(
            entry.category,
            Category::System,
            "{} is a power action, not an application",
            entry.name
        );
    }
}

/// Every drawn row must launch the program whose name is on it — the one
/// property a shared geometry exists to guarantee.
#[test]
fn every_visible_row_launches_the_program_named_on_it() {
    let mut shell = shell();
    for row in 0..shell.start_menu_visible_rows() {
        shell.toggle_start_menu();
        let expected = shell.start_menu_entries()[row].executable_path.clone();
        let rect = shell.start_menu_row_rect(row);
        let action = click_at(&mut shell, rect);
        assert_eq!(action, ShellAction::Launch(expected), "row {row}");
    }
}

/// And must go on doing so once the list has been scrolled, which is where a
/// renderer and a hit test that each tracked the offset would part company.
#[test]
fn a_scrolled_row_launches_the_program_named_on_it() {
    let mut shell = shell();
    shell.toggle_start_menu();
    assert!(
        shell.start_menu_max_scroll() >= 3,
        "the fixture needs more programs than the menu can show"
    );

    let (x, y) = centre(shell.start_menu_row_rect(0));
    shell.handle_mouse(&scroll(x, y, -3.0 * START_MENU_ROW_HEIGHT));
    assert_eq!(shell.start_menu_scroll, 3);

    let expected = shell.start_menu_entries()[3].executable_path.clone();
    let rect = shell.start_menu_row_rect(0);
    let action = click_at(&mut shell, rect);
    assert_eq!(action, ShellAction::Launch(expected));
}

#[test]
fn the_list_cannot_scroll_past_either_end() {
    let mut shell = shell();
    shell.toggle_start_menu();

    shell.scroll_start_menu(-1_000);
    assert_eq!(shell.start_menu_scroll, shell.start_menu_max_scroll());
    // The last row must still show the last program rather than blank space
    // past the end of the list.
    let last_row = shell.start_menu_visible_rows() - 1;
    assert_eq!(
        shell.start_menu_entry_at(last_row),
        Some(shell.start_menu_entries().len() - 1)
    );

    shell.scroll_start_menu(1_000);
    assert_eq!(shell.start_menu_scroll, 0);
}

/// A wheel that reports less than a row per event still has to move the list,
/// or the menu simply cannot be scrolled with that mouse.
#[test]
fn a_small_wheel_delta_still_moves_a_row() {
    assert_eq!(scroll_rows(-1.0), -1);
    assert_eq!(scroll_rows(1.0), 1);
    assert_eq!(scroll_rows(0.0), 0);
    assert_eq!(scroll_rows(-2.0 * START_MENU_ROW_HEIGHT), -2);
}

#[test]
fn reopening_the_menu_rewinds_the_list() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.scroll_start_menu(-2);
    assert_eq!(shell.start_menu_scroll, 2);
    shell.toggle_start_menu();
    shell.toggle_start_menu();
    assert_eq!(shell.start_menu_scroll, 0);
}

#[test]
fn a_click_outside_an_open_menu_dismisses_it_and_is_spent() {
    let mut shell = shell();
    let id = shell.add_window("Terminal", 400, 100, 500, 400, 1);
    shell.toggle_start_menu();

    let (x, y) = centre(shell.windows[&id].content_rect());
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert!(!shell.start_menu_open);
    // Spent means spent: the window under it must not also have seen the click
    // that closed the menu.
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Pass);
}

#[test]
fn a_click_on_the_menu_but_not_on_a_row_does_nothing_but_stay_open() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let menu = shell.start_menu_rect();
    // The heading strip, above the first row.
    let action = shell.handle_mouse(&click(menu.x + menu.w / 2.0, menu.y + 8.0));
    assert_eq!(action, ShellAction::Consumed);
    assert!(shell.start_menu_open);
}

// ---- the taskbar ----------------------------------------------------------

#[test]
fn a_taskbar_button_is_clickable_where_it_is_drawn() {
    let mut shell = shell();
    shell.add_window("A", 0, 0, 100, 100, 1);
    shell.add_window("B", 0, 0, 100, 100, 2);
    shell.add_window("C", 0, 0, 100, 100, 3);

    for index in 0..shell.visible_windows().len() {
        let (x, y) = centre(shell.taskbar_button_rect(index));
        assert_eq!(shell.hit_test(x, y), Hit::TaskbarButton(index));
    }
}

#[test]
fn a_taskbar_button_focuses_an_unfocused_window_and_minimizes_a_focused_one() {
    let mut shell = shell();
    let a = shell.add_window("A", 0, 0, 100, 100, 1);
    let b = shell.add_window("B", 0, 0, 100, 100, 2);
    assert_eq!(shell.focused_window, Some(b));

    // A is at index 0 — `visible_windows` is in Z order, and B was raised when
    // it was added.
    let first = shell.taskbar_button_rect(0);
    assert_eq!(click_at(&mut shell, first), ShellAction::Consumed);
    assert_eq!(shell.focused_window, Some(a));

    let index = shell
        .visible_windows()
        .iter()
        .position(|w| w.id == a)
        .unwrap();
    let button = shell.taskbar_button_rect(index);
    click_at(&mut shell, button);
    assert_eq!(shell.windows[&a].state, WindowState::Minimized);
}

#[test]
fn the_taskbar_panel_swallows_clicks_that_hit_nothing() {
    let mut shell = shell();
    let bar = shell.taskbar_rect();
    let action = shell.handle_mouse(&click(bar.w - 40.0, bar.y + bar.h / 2.0));
    assert_eq!(action, ShellAction::Consumed);
}

// ---- windows --------------------------------------------------------------

#[test]
fn the_title_bar_buttons_sit_inside_the_title_bar_and_do_not_overlap() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    let window = &shell.windows[&id];
    let bar = window.title_bar_rect();
    let buttons = [
        window.close_button_rect(),
        window.maximize_button_rect(),
        window.minimize_button_rect(),
    ];
    for button in buttons {
        assert!(bar.contains(button.x, button.y));
        assert!(bar.contains(button.x + button.w - 1.0, button.y + button.h - 1.0));
    }
    assert!(buttons[1].x + buttons[1].w <= buttons[0].x);
    assert!(buttons[2].x + buttons[2].w <= buttons[1].x);

    for (button, expected) in buttons.into_iter().zip([
        Hit::WindowClose(id),
        Hit::WindowMaximize(id),
        Hit::WindowMinimize(id),
    ]) {
        let (x, y) = centre(button);
        assert_eq!(shell.hit_test(x, y), expected);
    }
}

#[test]
fn the_close_button_closes_the_window() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    let rect = shell.windows[&id].close_button_rect();
    assert_eq!(click_at(&mut shell, rect), ShellAction::Consumed);
    assert!(!shell.windows.contains_key(&id));
}

#[test]
fn the_minimize_button_minimizes_the_window() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    let rect = shell.windows[&id].minimize_button_rect();
    click_at(&mut shell, rect);
    assert_eq!(shell.windows[&id].state, WindowState::Minimized);
}

/// Restoring has to put the window back where it was. Before the geometry was
/// remembered, "restore" only changed the state flag and left the window
/// filling the screen — a button that looked broken.
#[test]
fn maximizing_and_restoring_returns_the_window_to_where_it_was() {
    let mut shell = shell();
    let id = shell.add_window("A", 120, 90, 400, 300, 1);

    let rect = shell.windows[&id].maximize_button_rect();
    click_at(&mut shell, rect);
    let (_, _, work_w, work_h) = shell.work_area();
    assert_eq!(shell.windows[&id].state, WindowState::Maximized);
    assert_eq!(shell.windows[&id].width, work_w);
    assert_eq!(shell.windows[&id].height, work_h);

    // The button has moved with the window, so ask where it is now.
    let rect = shell.windows[&id].maximize_button_rect();
    click_at(&mut shell, rect);
    let window = &shell.windows[&id];
    assert_eq!(window.state, WindowState::Normal);
    assert_eq!((window.x, window.y), (120, 90));
    assert_eq!((window.width, window.height), (400, 300));
}

/// Maximizing twice must not record the maximized geometry as the one to spring
/// back to.
#[test]
fn maximizing_an_already_maximized_window_keeps_the_original_geometry() {
    let mut shell = shell();
    let id = shell.add_window("A", 120, 90, 400, 300, 1);
    shell.maximize_window(id);
    shell.maximize_window(id);
    shell.restore_window(id);
    let window = &shell.windows[&id];
    assert_eq!(
        (window.x, window.y, window.width, window.height),
        (120, 90, 400, 300)
    );
}

/// Once the user has placed the window themselves there is nothing to spring
/// back to, and springing back would move a window they just put where they
/// wanted it.
#[test]
fn moving_a_maximized_window_forgets_where_it_came_from() {
    let mut shell = shell();
    let id = shell.add_window("A", 120, 90, 400, 300, 1);
    shell.maximize_window(id);
    shell.move_window(id, 10, 10);
    shell.restore_window(id);
    let window = &shell.windows[&id];
    assert_eq!((window.x, window.y), (10, 10));
}

#[test]
fn a_double_click_on_the_title_bar_toggles_maximize() {
    let mut shell = shell();
    let id = shell.add_window("A", 120, 90, 400, 300, 1);
    let (x, y) = centre(shell.windows[&id].title_bar_rect());
    let event = MouseEvent {
        x,
        y,
        kind: MouseEventKind::DoubleClick(MouseButton::Left),
    };
    assert_eq!(shell.handle_mouse(&event), ShellAction::Consumed);
    assert_eq!(shell.windows[&id].state, WindowState::Maximized);
}

/// Click-to-focus raises the window and still lets the click reach it, so that
/// the first click on a background window presses what it landed on.
#[test]
fn a_click_in_a_window_focuses_it_and_is_passed_on() {
    let mut shell = shell();
    let a = shell.add_window("A", 0, 0, 400, 300, 1);
    let b = shell.add_window("B", 500, 0, 400, 300, 2);
    assert_eq!(shell.focused_window, Some(b));

    let (x, y) = centre(shell.windows[&a].content_rect());
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Pass);
    assert_eq!(shell.focused_window, Some(a));
}

/// A title-bar click focuses too, but is the shell's — it is where a drag
/// begins, and the client has no business seeing it.
#[test]
fn a_click_on_a_title_bar_focuses_but_is_consumed() {
    let mut shell = shell();
    let a = shell.add_window("A", 0, 0, 400, 300, 1);
    let _b = shell.add_window("B", 500, 0, 400, 300, 2);
    let (x, y) = centre(shell.windows[&a].title_bar_rect());
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert_eq!(shell.focused_window, Some(a));
}

#[test]
fn the_topmost_window_takes_an_overlapping_click() {
    let mut shell = shell();
    let a = shell.add_window("A", 100, 100, 400, 300, 1);
    let b = shell.add_window("B", 150, 150, 400, 300, 2);
    // A point inside both.
    assert_eq!(shell.hit_test(300.0, 300.0), Hit::WindowContent(b));
    shell.focus_window(a);
    assert_eq!(shell.hit_test(300.0, 300.0), Hit::WindowContent(a));
}

#[test]
fn a_minimized_window_is_not_under_the_pointer() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    assert_eq!(shell.hit_test(300.0, 300.0), Hit::WindowContent(id));
    shell.minimize_window(id);
    assert_eq!(shell.hit_test(300.0, 300.0), Hit::Desktop);
}

#[test]
fn the_bare_desktop_is_not_the_shells_to_consume() {
    let mut shell = shell();
    assert_eq!(shell.hit_test(500.0, 300.0), Hit::Desktop);
    assert_eq!(shell.handle_mouse(&click(500.0, 300.0)), ShellAction::Pass);
}

// ---- event kinds ----------------------------------------------------------

/// A client that saw a release with no matching press would read a click on the
/// shell's chrome as a click on itself.
#[test]
fn a_release_over_chrome_is_swallowed_with_the_press() {
    let mut shell = shell();
    let (x, y) = centre(shell.start_button_rect());
    let release = MouseEvent {
        x,
        y,
        kind: MouseEventKind::Release(MouseButton::Left),
    };
    assert_eq!(shell.handle_mouse(&release), ShellAction::Consumed);

    let release = MouseEvent {
        x: 500.0,
        y: 300.0,
        kind: MouseEventKind::Release(MouseButton::Left),
    };
    assert_eq!(shell.handle_mouse(&release), ShellAction::Pass);
}

/// The secondary buttons have no shell bindings yet, but must still not fall
/// through the taskbar onto whatever is behind it.
#[test]
fn a_right_click_on_chrome_acts_on_nothing_but_is_still_consumed() {
    let mut shell = shell();
    let (x, y) = centre(shell.start_button_rect());
    let event = MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Right),
    };
    assert_eq!(shell.handle_mouse(&event), ShellAction::Consumed);
    assert!(!shell.start_menu_open);
}

#[test]
fn motion_is_left_to_the_client() {
    let mut shell = shell();
    let (x, y) = centre(shell.start_button_rect());
    let event = MouseEvent {
        x,
        y,
        kind: MouseEventKind::Move,
    };
    assert_eq!(shell.handle_mouse(&event), ShellAction::Pass);
}

// ---- geometry -------------------------------------------------------------

/// Adjacent rows must not both claim the pixel on the boundary between them, or
/// a click near a row edge lands on either of two programs.
#[test]
fn adjacent_rows_do_not_share_a_pixel() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let first = shell.start_menu_row_rect(0);
    let boundary = first.y + first.h;
    assert!(!first.contains(first.x + 4.0, boundary));
    assert!(shell.start_menu_row_rect(1).contains(first.x + 4.0, boundary));
}

/// A screen too small for the chrome must produce empty rectangles, not
/// rectangles of negative size that swallow every click on the desktop.
#[test]
fn a_screen_smaller_than_the_taskbar_does_not_invert_the_geometry() {
    let shell = DesktopShell::new(200, 20);
    let bar = shell.taskbar_rect();
    assert_eq!(bar.y, 0.0);
    assert!(shell.taskbar_button_width() >= 0.0);
    assert_eq!(shell.work_area().3, 0);
    assert!(!Rect::new(0.0, 0.0, 0.0, 0.0).contains(0.0, 0.0));
}
