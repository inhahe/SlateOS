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
    ShellAction, TextRole, WindowChrome, WindowId, WindowState, click, scroll, scroll_rows,
};
use appearance::{AppearanceSettings, WindowCorners};
use guitk::render::{RenderCommand, RenderTree};
use guitk::style::CornerRadii;

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

/// Where one window's decorations are, as the shell would draw them.
fn chrome(shell: &DesktopShell, id: WindowId) -> WindowChrome {
    shell.window_chrome(&shell.windows[&id])
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

    let (x, y) = centre(chrome(&shell, id).content);
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
    let decorations = chrome(&shell, id);
    let bar = decorations.title_bar;
    let buttons = [
        decorations.close,
        decorations.maximize,
        decorations.minimize,
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
    let rect = chrome(&shell, id).close;
    assert_eq!(click_at(&mut shell, rect), ShellAction::Consumed);
    assert!(!shell.windows.contains_key(&id));
}

#[test]
fn the_minimize_button_minimizes_the_window() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    let rect = chrome(&shell, id).minimize;
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

    let rect = chrome(&shell, id).maximize;
    click_at(&mut shell, rect);
    let (_, _, work_w, work_h) = shell.work_area();
    assert_eq!(shell.windows[&id].state, WindowState::Maximized);
    assert_eq!(shell.windows[&id].width, work_w);
    assert_eq!(shell.windows[&id].height, work_h);

    // The button has moved with the window, so ask where it is now.
    let rect = chrome(&shell, id).maximize;
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
    let (x, y) = centre(chrome(&shell, id).title_bar);
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

    let (x, y) = centre(chrome(&shell, a).content);
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
    let (x, y) = centre(chrome(&shell, a).title_bar);
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

    let menu = shell.start_menu_rect();
    assert!(menu.w >= 0.0 && menu.h >= 0.0);
    assert_eq!(shell.start_menu_visible_rows(), 0);
}

/// A window shorter than the title bar it would be given must not be handed a
/// content area of negative height.
#[test]
fn a_window_shorter_than_its_own_title_bar_has_an_empty_content_area() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 200, 10, 1);
    let decorations = chrome(&shell, id);
    assert_eq!(decorations.title_bar.h, 10.0);
    assert_eq!(decorations.content.h, 0.0);
    let (x, y) = centre(decorations.frame);
    assert_eq!(shell.hit_test(x, y), Hit::WindowTitleBar(id));
}

// ---- display scaling ------------------------------------------------------

/// A shell whose user has chosen a display scaling.
fn scaled(percent: u16) -> DesktopShell {
    let mut shell = shell();
    let mut appearance = AppearanceSettings::default();
    appearance.scaling_percent = percent;
    shell.set_appearance(appearance);
    shell
}

/// The chrome is authored in logical pixels and drawn in physical ones. The
/// window itself is the compositor's and arrives already physical, so it is the
/// one measurement that must *not* be scaled again.
#[test]
fn the_window_chrome_grows_with_the_display_scaling_but_the_window_does_not() {
    let mut hundred = shell();
    let a = hundred.add_window("A", 100, 100, 400, 300, 1);
    let mut two_hundred = scaled(200);
    let b = two_hundred.add_window("A", 100, 100, 400, 300, 1);

    let small = chrome(&hundred, a);
    let large = chrome(&two_hundred, b);

    assert_eq!(small.frame, large.frame);
    assert!((large.title_bar.h - small.title_bar.h * 2.0).abs() < 0.01);
    assert!((large.close.w - small.close.w * 2.0).abs() < 0.01);
    assert!((large.content.y - large.frame.y) > (small.content.y - small.frame.y));
}

#[test]
fn the_title_bar_buttons_are_clickable_where_they_are_drawn_at_every_scale() {
    for percent in [100, 125, 150, 200] {
        let mut shell = scaled(percent);
        let id = shell.add_window("A", 100, 100, 600, 400, 1);
        let decorations = chrome(&shell, id);
        for (rect, expected) in [
            (decorations.close, Hit::WindowClose(id)),
            (decorations.maximize, Hit::WindowMaximize(id)),
            (decorations.minimize, Hit::WindowMinimize(id)),
        ] {
            assert!(
                decorations.title_bar.contains(rect.x, rect.y),
                "at {percent}% a button escaped the title bar"
            );
            let (x, y) = centre(rect);
            assert_eq!(shell.hit_test(x, y), expected, "at {percent}% scaling");
        }
    }
}

/// A taskbar that stayed its logical height while the buttons on it doubled
/// would clip them; a work area that ignored the scaling would leave maximized
/// windows underneath the bar.
#[test]
fn the_taskbar_grows_with_the_scaling_and_takes_the_room_from_the_work_area() {
    let hundred = shell();
    let two_hundred = scaled(200);

    assert!((two_hundred.taskbar_thickness() - hundred.taskbar_thickness() * 2.0).abs() < 0.01);
    assert!(two_hundred.work_area().3 < hundred.work_area().3);

    for (shell, name) in [(&hundred, "100%"), (&two_hundred, "200%")] {
        let bar = shell.taskbar_rect();
        assert!(
            (bar.y + bar.h - f32::from(u16::try_from(shell.screen_height).unwrap())).abs() < 0.01,
            "the taskbar must sit on the bottom edge at {name}"
        );
        assert!((bar.y - shell.work_area().3 as f32).abs() < 1.0);
    }
}

#[test]
fn the_start_button_is_clickable_where_it_is_drawn_at_every_scale() {
    for percent in [100, 125, 150, 200] {
        let mut shell = scaled(percent);
        let start = shell.start_button_rect();
        assert_eq!(click_at(&mut shell, start), ShellAction::Consumed);
        assert!(shell.start_menu_open, "at {percent}% scaling");
    }
}

/// Scaling makes the rows larger, and so fewer of them fit — but every program
/// must still be reachable, which is what the scroll is for.
#[test]
fn a_scaled_start_menu_still_reaches_every_program() {
    for percent in [100, 150, 200] {
        let mut shell = scaled(percent);
        shell.toggle_start_menu();

        let menu = shell.start_menu_rect();
        assert!(menu.y >= 0.0, "the menu ran off the top at {percent}%");
        assert!(
            menu.w <= shell.screen_width as f32,
            "the menu ran off the side at {percent}%"
        );

        let row = shell.start_menu_row_rect(0);
        assert!((row.h - shell.scale(START_MENU_ROW_HEIGHT)).abs() < 0.01);

        let total = shell.start_menu_entries().len();
        let mut seen = std::collections::BTreeSet::new();
        // Scroll to the far end a row at a time, collecting what is on screen.
        for _ in 0..=total {
            for visible in 0..shell.start_menu_visible_rows() {
                let rect = shell.start_menu_row_rect(visible);
                let (x, y) = centre(rect);
                if let Hit::StartMenuEntry(index) = shell.hit_test(x, y) {
                    seen.insert(index);
                }
            }
            shell.scroll_start_menu(-1);
        }
        assert_eq!(seen.len(), total, "unreachable programs at {percent}%");
    }
}

// ---- corners, shadows and type --------------------------------------------

/// The radii of every filled rectangle in a tree, in paint order.
fn fill_radii(tree: &RenderTree) -> Vec<CornerRadii> {
    tree.commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::FillRect { corner_radii, .. } => Some(*corner_radii),
            _ => None,
        })
        .collect()
}

fn shadow_count(tree: &RenderTree) -> usize {
    tree.commands
        .iter()
        .filter(|cmd| matches!(cmd, RenderCommand::BoxShadow { .. }))
        .count()
}

/// The font size of every piece of text in a tree, in paint order.
fn text_sizes(tree: &RenderTree) -> Vec<f32> {
    tree.commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::Text { font_size, .. } => Some(*font_size),
            _ => None,
        })
        .collect()
}

fn with_corners(corners: WindowCorners) -> DesktopShell {
    let mut shell = shell();
    let mut appearance = AppearanceSettings::default();
    appearance.window_corners = corners;
    shell.set_appearance(appearance);
    shell
}

#[test]
fn the_corner_setting_reaches_the_windows_the_shell_draws() {
    for corners in [
        WindowCorners::Square,
        WindowCorners::Subtle,
        WindowCorners::Rounded,
        WindowCorners::ExtraRounded,
    ] {
        let mut shell = with_corners(corners);
        shell.add_window("A", 100, 100, 400, 300, 1);
        let tree = shell.render_window_decorations();
        let title_bar = fill_radii(&tree)[0];
        assert_eq!(title_bar.top_left, corners.radius(), "{corners:?}");
        // Rounded across the top only — rounding the lower corners would cut a
        // notch out of the join with the client area.
        assert_eq!(title_bar.bottom_left, 0.0, "{corners:?}");
    }
}

#[test]
fn the_corner_setting_reaches_the_start_menu_and_the_taskbar_buttons() {
    let mut shell = with_corners(WindowCorners::ExtraRounded);
    shell.add_window("A", 100, 100, 400, 300, 1);
    shell.toggle_start_menu();

    let menu = shell.render_start_menu().expect("the menu is open");
    assert_eq!(fill_radii(&menu)[0], CornerRadii::all(16.0));

    // The taskbar panel itself is square — it has no free edge to round — but
    // the buttons on it follow the setting.
    let taskbar = shell.render_taskbar();
    let radii = fill_radii(&taskbar);
    assert_eq!(radii[0], CornerRadii::ZERO, "the panel spans the screen");
    assert!(
        radii.iter().any(|r| *r == CornerRadii::all(16.0)),
        "the window buttons must follow the corner setting"
    );
}

/// A radius that stayed 8px while everything around it doubled would read as a
/// sharper corner, not the same one.
#[test]
fn the_corner_radius_grows_with_the_display_scaling() {
    let mut shell = scaled(200);
    shell.add_window("A", 100, 100, 400, 300, 1);
    assert_eq!(fill_radii(&shell.render_window_decorations())[0].top_left, 16.0);
}

#[test]
fn drop_shadows_are_drawn_only_when_the_user_asks_for_them() {
    for (wanted, expected) in [(true, 1), (false, 0)] {
        let mut shell = shell();
        let mut appearance = AppearanceSettings::default();
        appearance.drop_shadows = wanted;
        shell.set_appearance(appearance);
        shell.add_window("A", 100, 100, 400, 300, 1);
        shell.toggle_start_menu();

        assert_eq!(shadow_count(&shell.render_window_decorations()), expected);
        assert_eq!(
            shadow_count(&shell.render_start_menu().expect("the menu is open")),
            expected
        );
    }
}

/// A shadow drawn after the surface it belongs to lands on top of the thing it
/// is supposed to sit behind.
#[test]
fn a_shadow_is_painted_before_the_surface_that_casts_it() {
    let mut shell = shell();
    shell.add_window("A", 100, 100, 400, 300, 1);
    let tree = shell.render_window_decorations();
    let shadow = tree
        .commands
        .iter()
        .position(|cmd| matches!(cmd, RenderCommand::BoxShadow { .. }))
        .expect("a shadow");
    let surface = tree
        .commands
        .iter()
        .position(|cmd| matches!(cmd, RenderCommand::FillRect { .. }))
        .expect("the title bar");
    assert!(shadow < surface);
}

/// There is nothing beside a maximized window for a shadow to fall on, and one
/// drawn anyway bleeds over the screen edge.
#[test]
fn a_maximized_window_casts_no_shadow() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 400, 300, 1);
    assert_eq!(shadow_count(&shell.render_window_decorations()), 1);
    shell.maximize_window(id);
    assert_eq!(shadow_count(&shell.render_window_decorations()), 0);
    shell.restore_window(id);
    assert_eq!(shadow_count(&shell.render_window_decorations()), 1);
}

#[test]
fn text_grows_with_both_the_font_size_and_the_display_scaling() {
    let base = shell();
    let body = base.font_size(TextRole::Body);
    assert_eq!(body, AppearanceSettings::default().fonts.ui_size);
    assert!(base.font_size(TextRole::Heading) > body);
    assert!(base.font_size(TextRole::Caption) < body);

    let mut bigger = shell();
    let mut appearance = AppearanceSettings::default();
    appearance.fonts.ui_size *= 2.0;
    bigger.set_appearance(appearance);
    assert_eq!(bigger.font_size(TextRole::Body), body * 2.0);

    assert_eq!(scaled(200).font_size(TextRole::Body), body * 2.0);
}

/// The sizes have to reach the draw calls: a `13.0` left in one of them ignores
/// both settings at once, and enlarges the chrome around text that stays put.
#[test]
fn every_drawn_string_follows_the_users_font_size() {
    let render = |shell: &mut DesktopShell| {
        shell.add_window("Terminal", 100, 100, 400, 300, 1);
        shell.start_menu_open = true;
        shell.alt_tab_active = true;
        let mut sizes = text_sizes(&shell.render_taskbar());
        sizes.extend(text_sizes(&shell.render_window_decorations()));
        sizes.extend(text_sizes(
            &shell.render_start_menu().expect("the menu is open"),
        ));
        sizes.extend(text_sizes(&shell.render_alt_tab().expect("alt-tab is up")));
        sizes
    };

    let mut base = shell();
    let plain = render(&mut base);
    assert!(!plain.is_empty());

    let mut bigger = shell();
    let mut appearance = AppearanceSettings::default();
    appearance.fonts.ui_size *= 2.0;
    bigger.set_appearance(appearance);
    let enlarged = render(&mut bigger);

    assert_eq!(plain.len(), enlarged.len());
    for (small, large) in plain.iter().zip(&enlarged) {
        assert!(
            (large - small * 2.0).abs() < 0.01,
            "a draw call ignored the font size: {small} did not become {large}"
        );
    }
}
