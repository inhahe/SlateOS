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
// These tests assert a float equals the exact literal the code under test was
// handed. That is the assertion meant: a tolerance would let a value that has
// drifted pass as one that has not.
#![allow(clippy::float_cmp)]

use crate::calendar;
use crate::datetime_settings::AdditionalClock;
use crate::launcher::{self, Category};
use crate::{
    DesktopShell, Hit, Key, KeyEvent, Layer, Modifiers, MouseButton, MouseEvent, MouseEventKind,
    Rect, START_MENU_ROW_HEIGHT, ShellAction, ShellControlAction, TextRole, WindowId, WindowInfo,
    WindowRequest, WindowState, click, scroll, scroll_rows,
};
use appearance::{AppearanceSettings, WindowCorners};
use guitk::render::{RenderCommand, RenderTree};
use guitk::style::CornerRadii;
use guitk::wheel;

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

/// Where a window is. The shell knows this much — the taskbar and the hit test
/// both need it — but not what its frame looks like; that is the compositor's.
fn frame(shell: &DesktopShell, id: WindowId) -> Rect {
    shell.windows[&id].frame_rect()
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
    // One detent, which is three rows. This used to read
    // `-3.0 * START_MENU_ROW_HEIGHT` — a delta in pixels, matching the units
    // the handler wrongly assumed rather than the notches it is actually sent.
    shell.handle_mouse(&scroll(x, y, -1.0));
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

    shell.scroll_start_menu(1_000);
    assert_eq!(shell.start_menu_scroll, shell.start_menu_max_scroll());
    // The last row must still show the last program rather than blank space
    // past the end of the list.
    let last_row = shell.start_menu_visible_rows() - 1;
    assert_eq!(
        shell.start_menu_entry_at(last_row),
        Some(shell.start_menu_entries().len() - 1)
    );

    shell.scroll_start_menu(-1_000);
    assert_eq!(shell.start_menu_scroll, 0);
}

/// One detent of an ordinary wheel is `1.0` notch, and a notch is three rows.
///
/// This is what the old `dy / START_MENU_ROW_HEIGHT` could never produce: it
/// read `dy` as pixels, so a notch came out as `1.0 / 36.0`, truncated to zero,
/// and every scroll fell through to a fallback that moved a single row no
/// matter how far the wheel was turned.
#[test]
fn one_notch_moves_three_rows() {
    let mut acc = wheel::Accumulator::default();
    // Positive `dy` is away from the user, which moves towards row 0.
    assert_eq!(scroll_rows(&mut acc, -1.0), 3);
    assert_eq!(scroll_rows(&mut acc, 1.0), -3);
    assert_eq!(scroll_rows(&mut acc, 0.0), 0);
    assert_eq!(scroll_rows(&mut acc, -2.0), 6);
}

/// A trackpad sends fractions of a notch. Rounding each one on its own would
/// discard all of them; the accumulator banks them until they make a row.
#[test]
fn a_trackpads_fractions_add_up_instead_of_being_discarded() {
    let mut acc = wheel::Accumulator::default();
    // A tenth of a notch is three tenths of a row, so the first delivery lands
    // on the fourth event and the ten together are worth exactly three rows.
    let mut total = 0;
    for _ in 0..10 {
        total += scroll_rows(&mut acc, -0.1);
    }
    assert_eq!(total, 3);
}

/// The menu must actually respond to a real wheel event, end to end — the old
/// code's failure was invisible to any test that called the helper directly
/// with a pixel-shaped number.
#[test]
fn a_wheel_notch_over_the_menu_scrolls_it() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let rect = shell.start_menu_row_rect(0);
    let (x, y) = centre(rect);
    assert_eq!(
        shell.handle_mouse(&scroll(x, y, -1.0)),
        ShellAction::Consumed
    );
    assert_eq!(
        shell.start_menu_scroll, 3,
        "one detent should cross three rows"
    );
}

/// A fraction left over from one visit to the menu must not move the next one.
#[test]
fn reopening_the_menu_forgets_the_leftover_fraction() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let rect = shell.start_menu_row_rect(0);
    let (x, y) = centre(rect);
    // Two tenths of a notch: six tenths of a row, so nothing moves yet.
    shell.handle_mouse(&scroll(x, y, -0.1));
    shell.handle_mouse(&scroll(x, y, -0.1));
    assert_eq!(shell.start_menu_scroll, 0);
    shell.toggle_start_menu();
    shell.toggle_start_menu();
    // Were the 0.6 rows still banked, this 0.6 would complete a row.
    shell.handle_mouse(&scroll(x, y, -0.1));
    shell.handle_mouse(&scroll(x, y, -0.1));
    assert_eq!(shell.start_menu_scroll, 0);
}

#[test]
fn reopening_the_menu_rewinds_the_list() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.scroll_start_menu(2);
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

    let (x, y) = centre(frame(&shell, id));
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

// ---- the power menu -------------------------------------------------------

/// The whole point: the machine can be shut down from the desktop. Before this
/// the foot of the start menu drew the word "Power" in grey and did nothing,
/// and the five system actions were in no menu at all.
#[test]
fn the_power_menu_offers_every_system_action_and_launches_them() {
    let names: Vec<String> = {
        let shell = shell();
        shell
            .power_menu_entries()
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    };
    assert!(
        names.iter().any(|name| name == "Shutdown"),
        "no way to shut the machine down: {names:?}"
    );

    for row in 0..names.len() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let button = shell.power_button_rect();
        assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
        assert!(shell.power_menu_open);

        let expected = shell.power_menu_entries()[row].executable_path.clone();
        let rect = shell.power_menu_row_rect(row);
        let action = click_at(&mut shell, rect);
        assert_eq!(action, ShellAction::Launch(expected), "row {row}");
        // Both menus go: the machine is about to shut down behind them.
        assert!(!shell.power_menu_open);
        assert!(!shell.start_menu_open);
    }
}

/// The two menus divide the one database between them. An entry in neither list
/// is an unreachable program; an entry in both puts "Shutdown" one mis-click
/// below "Screenshot", which is why the split exists.
#[test]
fn the_two_menus_between_them_offer_every_program_exactly_once() {
    let shell = shell();
    let database = launcher::builtin_app_database();
    let mut offered: Vec<&str> = shell
        .start_menu_entries()
        .iter()
        .chain(shell.power_menu_entries().iter())
        .map(|entry| entry.executable_path.as_str())
        .collect();
    offered.sort_unstable();
    let before = offered.len();
    offered.dedup();
    assert_eq!(before, offered.len(), "a program is in both menus");
    assert_eq!(offered.len(), database.len(), "a program is in neither");

    for entry in shell.power_menu_entries() {
        assert_eq!(entry.category, Category::System, "{}", entry.name);
    }
}

#[test]
fn the_power_button_toggles_its_menu_and_leaves_the_start_menu_open() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let button = shell.power_button_rect();

    assert_eq!(shell.hit_test(button.x, button.y), Hit::PowerButton);
    assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
    assert!(shell.power_menu_open);
    assert!(shell.start_menu_open);

    assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
    assert!(!shell.power_menu_open);
    assert!(
        shell.start_menu_open,
        "closing the submenu is not closing both"
    );
}

/// A submenu is allowed to cover the list it opened from — but then a click in
/// the overlap has to reach the popup, not the row buried under it.
#[test]
fn the_power_menu_takes_the_clicks_on_the_rows_it_covers() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.toggle_power_menu();

    let (x, y) = centre(shell.power_menu_row_rect(0));
    let covered = (0..shell.start_menu_visible_rows())
        .any(|row| shell.start_menu_row_rect(row).contains(x, y));
    assert!(covered, "the fixture must actually overlap a row");
    assert_eq!(shell.hit_test(x, y), Hit::PowerMenuEntry(0));
}

/// Clicking the list behind an open submenu dismisses the submenu and is spent
/// doing so — launching the program underneath as well would start something
/// the user could not see when they aimed.
#[test]
fn a_click_on_the_list_behind_the_power_menu_only_dismisses_it() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.toggle_power_menu();

    let row = shell.start_menu_row_rect(0);
    assert!(
        !shell
            .power_menu_rect()
            .contains(row.x + row.w / 2.0, row.y + row.h / 2.0),
        "the fixture must pick a row the popup does not cover"
    );
    assert_eq!(click_at(&mut shell, row), ShellAction::Consumed);
    assert!(!shell.power_menu_open);
    assert!(shell.start_menu_open);
}

/// The popup rises from a button inside the start menu, so it can never outlive
/// it: one left over a closed menu is a floating panel with no visible cause.
#[test]
fn closing_the_start_menu_any_way_at_all_takes_the_power_menu_with_it() {
    let open = |shell: &mut DesktopShell| {
        shell.start_menu_open = true;
        shell.power_menu_open = true;
    };

    let mut by_toggle = shell();
    open(&mut by_toggle);
    by_toggle.toggle_start_menu();
    assert!(!by_toggle.power_menu_open, "toggle");

    let mut by_start_button = shell();
    open(&mut by_start_button);
    let start = by_start_button.start_button_rect();
    click_at(&mut by_start_button, start);
    assert!(!by_start_button.power_menu_open, "start button");

    let mut by_click_away = shell();
    open(&mut by_click_away);
    by_click_away.handle_mouse(&click(900.0, 300.0));
    assert!(!by_click_away.power_menu_open, "click on the desktop");
    assert!(!by_click_away.start_menu_open);

    let mut by_launching = shell();
    open(&mut by_launching);
    let row = by_launching.start_menu_row_rect(0);
    // The first click dismisses the popup; the second launches.
    click_at(&mut by_launching, row);
    assert!(matches!(
        click_at(&mut by_launching, row),
        ShellAction::Launch(_)
    ));
    assert!(!by_launching.power_menu_open, "launching a program");
    assert!(!by_launching.start_menu_open);
}

#[test]
fn a_wheel_over_the_power_menu_does_not_scroll_the_list_behind_it() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.toggle_power_menu();

    let (x, y) = centre(shell.power_menu_row_rect(0));
    assert_eq!(
        shell.handle_mouse(&scroll(x, y, -3.0 * START_MENU_ROW_HEIGHT)),
        ShellAction::Consumed
    );
    assert_eq!(shell.start_menu_scroll, 0, "the hidden rows moved");
}

/// Scaling must not put a system action off the screen or out from under the
/// pointer: unlike the application list the power menu has no scroll to rescue
/// a row it fails to fit.
#[test]
fn every_power_action_is_clickable_where_it_is_drawn_at_every_scale() {
    for percent in [100, 125, 150, 200] {
        let mut shell = scaled(percent);
        shell.toggle_start_menu();
        shell.toggle_power_menu();

        let menu = shell.power_menu_rect();
        assert!(menu.y >= 0.0, "the popup ran off the top at {percent}%");
        assert!(
            menu.x + menu.w <= shell.screen_width as f32,
            "the popup ran off the side at {percent}%"
        );

        let button = shell.power_button_rect();
        let start = shell.start_menu_rect();
        assert!(
            button.x >= start.x && button.y >= start.y && button.y + button.h <= start.y + start.h,
            "the power button escaped the menu at {percent}%"
        );

        assert_eq!(
            shell.power_menu_visible_rows(),
            shell.power_menu_entries().len(),
            "a system action was dropped at {percent}%"
        );
        for row in 0..shell.power_menu_visible_rows() {
            let (x, y) = centre(shell.power_menu_row_rect(row));
            assert_eq!(
                shell.hit_test(x, y),
                Hit::PowerMenuEntry(row),
                "row {row} at {percent}% scaling"
            );
        }
    }
}

/// The popup is drawn by `power.rs` but themed by the shell: a menu in its own
/// hard-coded palette would stay dark on a light desktop.
#[test]
fn the_power_menu_follows_the_theme_and_the_corner_setting() {
    let mut shell = with_corners(WindowCorners::ExtraRounded);
    shell.toggle_start_menu();
    shell.toggle_power_menu();
    let tree = shell.render_start_menu().expect("the menu is open");

    let panel = tree
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::FillRect {
                color,
                corner_radii,
                width,
                ..
            } => Some((*color, *corner_radii, *width)),
            _ => None,
        })
        .find(|(_, _, width)| (*width - shell.power_menu_rect().w).abs() < 0.01)
        .expect("the popup's panel");
    assert_eq!(panel.0, shell.theme.start_menu_bg);
    assert_eq!(panel.1, CornerRadii::all(16.0));

    // Every system action is on screen, spelled as the database spells it.
    let drawn: Vec<String> = tree
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for entry in shell.power_menu_entries() {
        assert!(drawn.contains(&entry.name), "{} was not drawn", entry.name);
    }
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
fn a_taskbar_button_asks_to_activate_an_unfocused_window_and_to_minimize_a_focused_one() {
    // The taskbar button is a toggle, and the two halves are different
    // requests: the window you are not looking at is summoned, the one you are
    // is put away. Both are *asked for* — the shell does not minimise anything
    // itself, because the compositor owns whether a window is minimised and a
    // shell that decided for itself would hold a second answer.
    let mut shell = shell();
    let a = shell.add_window("A", 0, 0, 100, 100, 1);
    let b = shell.add_window("B", 0, 0, 100, 100, 2);
    assert_eq!(shell.focused_window, Some(b));

    // A is at index 0 — `visible_windows` is in Z order, and B was raised when
    // it was added.
    let first = shell.taskbar_button_rect(0);
    assert_eq!(
        click_at(&mut shell, first),
        ShellAction::Control(WindowRequest::new(a, ShellControlAction::Activate)),
        "the button of an unfocused window must summon it"
    );

    // And the click changed nothing locally. This is the half that used to be
    // wrong: the shell focused the window itself, so it believed a thing the
    // compositor had not been told and would not agree with.
    assert_eq!(
        shell.focused_window,
        Some(b),
        "the shell focused a window on its own authority"
    );

    let index = shell
        .visible_windows()
        .iter()
        .position(|w| w.id == b)
        .unwrap();
    let button = shell.taskbar_button_rect(index);
    assert_eq!(
        click_at(&mut shell, button),
        ShellAction::Control(WindowRequest::new(b, ShellControlAction::Minimize)),
        "the button of the focused window must put it away"
    );
    assert_eq!(
        shell.windows[&b].state,
        WindowState::Normal,
        "the shell minimized a window on its own authority"
    );
}

#[test]
fn a_taskbar_button_whose_window_has_gone_swallows_the_click() {
    // The window closed between the frame the button was drawn in and the
    // press. Nothing to ask for — but the click landed on the taskbar, and a
    // taskbar that let it through would raise whatever happened to be behind.
    let mut shell = shell();
    shell.add_window("A", 0, 0, 100, 100, 1);
    let button = shell.taskbar_button_rect(0);
    shell.apply_window_list(&[]);

    assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
}

// ---- the window list the taskbar is drawn from ----------------------------

/// An ordinary application window as the compositor would describe it.
fn app(id: u64, title: &str) -> WindowInfo {
    WindowInfo::new(id, u64::from(u32::try_from(id).unwrap()), title)
}

#[test]
fn the_window_list_replaces_what_the_shell_believed_rather_than_adding_to_it() {
    // The compositor's list is the whole truth about the desktop, not a stream
    // of changes: a window that is not in it has gone, and a shell that merged
    // instead of replacing would keep a button for a program that has exited.
    let mut shell = shell();
    // Seeded through the same door a live shell uses, and with an id that
    // appears in no later list — the point being missed otherwise. Seeding with
    // `add_window` gives the stale window id 1, which the next list happens to
    // reuse and therefore overwrites, so a shell that merged would pass anyway.
    shell.apply_window_list(&[app(9, "closed since")]);
    assert!(shell.windows.contains_key(&WindowId(9)));

    let mut second = app(2, "Editor");
    second.focused = true;
    shell.apply_window_list(&[app(1, "Terminal"), second]);

    assert!(
        !shell.windows.contains_key(&WindowId(9)),
        "a window absent from the list kept its taskbar button"
    );
    let titles: Vec<&str> = shell
        .visible_windows()
        .iter()
        .map(|w| w.title.as_str())
        .collect();
    assert_eq!(titles, ["Terminal", "Editor"], "in the order sent, bottom up");
    assert_eq!(shell.focused_window, Some(WindowId(2)));
    assert_eq!(
        shell.visible_windows().last().map(|w| w.id),
        Some(WindowId(2)),
        "the list is bottom-to-top, so the last entry is topmost"
    );
}

#[test]
fn the_taskbar_leaves_out_every_surface_that_is_not_an_application_window() {
    // The list describes the shell's own surfaces too — its taskbar, its
    // wallpaper, its start menu. A taskbar that listed them would be mostly
    // buttons for itself, and `Layer` is the only field that tells them apart.
    let mut shell = shell();
    let mut wallpaper = app(1, "Wallpaper");
    wallpaper.layer = Layer::Background;
    let mut bar = app(2, "Taskbar");
    bar.layer = Layer::Overlay;

    shell.apply_window_list(&[wallpaper, app(3, "Editor"), bar]);

    let ids: Vec<WindowId> = shell.visible_windows().iter().map(|w| w.id).collect();
    assert_eq!(ids, [WindowId(3)], "the shell listed its own surfaces");
}

#[test]
fn a_minimized_window_keeps_its_button_and_an_unmapped_one_does_not() {
    // The two are different states and the difference is exactly the taskbar:
    // a minimised window is one the user put away and can click to get back, so
    // it must keep its button. A window its own program unmapped is not there.
    let mut shell = shell();
    let mut away = app(1, "Minimized");
    away.minimized = true;
    let mut hidden = app(2, "Unmapped");
    hidden.visible = false;

    shell.apply_window_list(&[away, hidden, app(3, "Ordinary")]);

    assert_eq!(shell.windows[&WindowId(1)].state, WindowState::Minimized);
    assert!(
        !shell.windows[&WindowId(1)].visible,
        "a minimized window is not on the glass"
    );
    assert!(
        shell.windows.contains_key(&WindowId(2)),
        "an unmapped window keeps its id and its place in the stack"
    );
    let listed: Vec<WindowId> = shell.visible_windows().iter().map(|w| w.id).collect();
    assert_eq!(listed, [WindowId(3)]);
}

#[test]
fn a_retitle_reaches_the_button_without_disturbing_shell_local_state() {
    // The update a taskbar exists to show, and the one most likely to be lost:
    // a window that is already known must be updated in place, not rebuilt,
    // because the shell holds per-window state the compositor knows nothing
    // about and cannot send back.
    let mut shell = shell();
    shell.apply_window_list(&[app(1, "untitled")]);
    shell.windows.get_mut(&WindowId(1)).unwrap().icon_id = 42;
    shell.windows.get_mut(&WindowId(1)).unwrap().desktop = 3;

    shell.apply_window_list(&[app(1, "notes.txt — saved")]);

    let win = &shell.windows[&WindowId(1)];
    assert_eq!(win.title, "notes.txt — saved");
    assert_eq!(win.icon_id, 42, "the icon was rebuilt from nothing");
    assert_eq!(
        win.desktop, 3,
        "a retitle moved the window to another virtual desktop"
    );
}

#[test]
fn an_empty_desktop_leaves_nothing_focused() {
    // "Nobody is focused" is a state the compositor can genuinely be in — every
    // window minimised, or the last one closed — so the shell must be able to
    // hold it. Keeping the previous answer would leave a taskbar button lit for
    // a window that is gone.
    let mut shell = shell();
    let mut only = app(1, "Editor");
    only.focused = true;
    shell.apply_window_list(&[only]);
    assert_eq!(shell.focused_window, Some(WindowId(1)));

    shell.apply_window_list(&[]);
    assert_eq!(shell.focused_window, None);
    assert!(shell.windows.is_empty());
}

#[test]
fn the_window_list_is_the_only_thing_that_grows_the_shells_idea_of_the_desktop() {
    // A round trip in the shape a live session runs it: the taskbar button
    // produces a request, and the *only* way the result comes back is the next
    // list. Applying the compositor's answer is what moves the shell — nothing
    // the click did.
    let mut shell = shell();
    let mut focused = app(1, "Editor");
    focused.focused = true;
    shell.apply_window_list(&[focused]);

    let button = shell.taskbar_button_rect(0);
    let action = click_at(&mut shell, button);
    assert_eq!(
        action,
        ShellAction::Control(WindowRequest::new(WindowId(1), ShellControlAction::Minimize))
    );
    assert!(
        shell.visible_windows().len() == 1,
        "the shell acted on the request itself"
    );

    // The compositor did as it was asked and said so.
    let mut away = app(1, "Editor");
    away.minimized = true;
    shell.apply_window_list(&[away]);
    assert!(shell.visible_windows().is_empty());
    assert_eq!(shell.focused_window, None);
}

#[test]
fn the_taskbar_panel_swallows_clicks_that_hit_nothing() {
    let mut shell = shell();
    let bar = shell.taskbar_rect();
    // Between the start button and the tray, where there is nothing at all:
    // this used to aim 40 px in from the right edge, which the clock's target
    // now covers, so the test would have gone on passing while measuring the
    // clock instead of the bare panel.
    let x = f32::midpoint(shell.start_button_rect().w, shell.tray_x());
    let y = bar.y + bar.h / 2.0;
    assert_eq!(shell.hit_test(x, y), Hit::TaskbarPanel);
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
}

// ---- the calendar popup ---------------------------------------------------

/// Where the popup's month layout is, given where the shell would place it.
fn month_layout(shell: &DesktopShell) -> calendar::MonthLayout {
    let (x, y) = shell.calendar_origin();
    calendar::MonthLayout::new(&shell.calendar, x, y, shell.calendar_scale())
}

/// Click the tray clock — which opens the popup, or closes it again.
fn click_clock(shell: &mut DesktopShell) -> ShellAction {
    let clock = shell.clock_rect();
    click_at(shell, clock)
}

fn click_start(shell: &mut DesktopShell) -> ShellAction {
    let start = shell.start_button_rect();
    click_at(shell, start)
}

/// The clock toggles: a second click on it closes what the first opened.
///
/// It is the one target exempt from the shell's dismiss-first rule (see
/// `swapping_popups_costs_the_click_that_closes_the_first_one`), because it is
/// the popup's own button — applying the rule there would leave the calendar
/// impossible to close from the control that opened it.
#[test]
fn the_tray_clock_opens_and_closes_the_calendar() {
    let mut shell = shell();
    let clock = shell.clock_rect();
    assert_eq!(shell.hit_test(clock.x + 1.0, clock.y + 1.0), Hit::Clock);

    assert!(!shell.calendar.visible);
    assert_eq!(click_at(&mut shell, clock), ShellAction::Consumed);
    assert!(shell.calendar.visible);
    assert_eq!(click_at(&mut shell, clock), ShellAction::Consumed);
    assert!(!shell.calendar.visible);
}

/// The clock's target has to cover the reading the taskbar actually draws.
///
/// The two are derived from the same `clock_width`, and this is what holds
/// them to it: a target measured from a stale width leaves the last few
/// characters of the clock inert, which reads as "the clock is not clickable"
/// rather than as an off-by-a-few-pixels rectangle.
#[test]
fn the_clocks_target_covers_the_reading_that_is_drawn() {
    let shell = shell();
    let target = shell.clock_rect();
    // The clock is the rightmost thing on the taskbar, so the rightmost text
    // command is it — the desktop indicator sits at the tray's left edge.
    let (x, y, size) = shell
        .render_taskbar()
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text {
                x, y, font_size, ..
            } => Some((*x, *y, *font_size)),
            _ => None,
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .expect("the taskbar draws a clock");

    // The slot is sized for the *widest* reading the switches allow, so check
    // that one rather than the current second.
    let widest = shell.clock_width();
    assert!(x >= target.x, "the reading starts left of its target");
    assert!(
        x + widest <= target.x + target.w + 0.5,
        "the reading runs past its target"
    );
    assert!(y >= target.y && y + size <= target.y + target.h);
}

#[test]
fn every_calendar_control_is_clickable_where_it_is_drawn() {
    let mut shell = shell();
    click_clock(&mut shell);
    // Off today's month, so the Today button exists too.
    shell.calendar.next_month();

    let layout = month_layout(&shell);
    for (rect, want) in [
        (layout.prev_arrow(), calendar::CalendarHit::PrevPage),
        (layout.next_arrow(), calendar::CalendarHit::NextPage),
        (layout.title(), calendar::CalendarHit::Title),
        (
            layout.today_button().expect("off-month shows Today"),
            calendar::CalendarHit::Today,
        ),
    ] {
        let (x, y) = centre(rect);
        assert_eq!(
            shell.hit_test(x, y),
            Hit::CalendarControl(want),
            "{want:?} is not clickable where the shell draws it"
        );
    }
    for index in [0_usize, 20, 41] {
        let (x, y) = centre(layout.cell(index));
        assert_eq!(
            shell.hit_test(x, y),
            Hit::CalendarControl(calendar::CalendarHit::Day(index))
        );
    }
}

#[test]
fn a_calendar_arrow_pages_the_month_it_is_drawn_beside() {
    let mut shell = shell();
    click_clock(&mut shell);
    let (year, month) = (shell.calendar.view_year, shell.calendar.view_month);

    let next = month_layout(&shell).next_arrow();
    assert_eq!(click_at(&mut shell, next), ShellAction::Consumed);
    assert!(shell.calendar.visible, "paging must not close the popup");
    assert_ne!(
        (shell.calendar.view_year, shell.calendar.view_month),
        (year, month)
    );

    let prev = month_layout(&shell).prev_arrow();
    click_at(&mut shell, prev);
    assert_eq!(
        (shell.calendar.view_year, shell.calendar.view_month),
        (year, month)
    );
}

/// A click on the popup's own margin must not close it.
///
/// The popup is wider than its grid, so there is real inert space inside it;
/// dismissing on a press there is the single most irritating way for a popup
/// to behave, and the reason the hit test distinguishes "off the popup" from
/// "on the popup, on nothing".
#[test]
fn a_click_on_the_calendars_margin_leaves_it_open() {
    let mut shell = shell();
    click_clock(&mut shell);
    let frame = month_layout(&shell).frame;

    let action = shell.handle_mouse(&click(frame.x + 1.0, frame.y + frame.h - 1.0));
    assert_eq!(action, ShellAction::Consumed);
    assert!(shell.calendar.visible);
}

#[test]
fn a_click_off_the_calendar_closes_it_without_acting() {
    let mut shell = shell();
    let id = shell.add_window("Terminal", 100, 100, 400, 300, 1);
    shell.minimize_window(id);
    click_clock(&mut shell);

    // On the taskbar button of the minimised window: the click is spent
    // closing the popup, so the window stays minimised.
    let button = shell.taskbar_button_rect(0);
    assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
    assert!(!shell.calendar.visible);
    assert_eq!(shell.windows[&id].state, WindowState::Minimized);
}

/// One popup at a time — and the click that closes one does not open the other.
///
/// The shell's standing rule is that a press outside an open menu is *spent*
/// dismissing it (see `handle_press`): dismissing is what the user aimed at,
/// and acting as well would make one click do something they could not see
/// coming. The clock and the start button are outside each other's popups, so
/// they obey it like every other target does — the alternative would be two
/// buttons on one bar that quietly follow a different rule from their
/// neighbours. It costs a second click to swap popups, which is the price of
/// the rule and is paid identically by the taskbar's window buttons.
#[test]
fn swapping_popups_costs_the_click_that_closes_the_first_one() {
    let mut shell = shell();
    click_start(&mut shell);
    assert!(shell.start_menu_open);

    // First click on the clock: spent closing the start menu.
    click_clock(&mut shell);
    assert!(!shell.start_menu_open);
    assert!(
        !shell.calendar.visible,
        "the click that dismissed the start menu must not also open the calendar"
    );

    // Second click: nothing is open, so it opens the calendar.
    click_clock(&mut shell);
    assert!(shell.calendar.visible);

    // And symmetrically, with the calendar open.
    click_start(&mut shell);
    assert!(!shell.calendar.visible);
    assert!(!shell.start_menu_open);
    click_start(&mut shell);
    assert!(shell.start_menu_open);
}

/// Escape closes a popup, and is passed through when there is none.
///
/// A shell that claimed Escape unconditionally would be a shell in which no
/// window could ever close a dialog with it — far more often what the key is
/// for than closing the start menu.
#[test]
fn escape_closes_a_popup_and_is_otherwise_left_alone() {
    let escape = KeyEvent {
        key: Key::Escape,
        pressed: true,
        modifiers: Modifiers::default(),
        text: None,
    };

    let mut shell = shell();
    assert!(
        !shell.handle_hotkey(&escape).consumed,
        "Escape with nothing open must reach the focused window"
    );

    click_clock(&mut shell);
    assert!(shell.handle_hotkey(&escape).consumed);
    assert!(!shell.calendar.visible);
    assert!(!shell.handle_hotkey(&escape).consumed);

    click_start(&mut shell);
    assert!(shell.handle_hotkey(&escape).consumed);
    assert!(!shell.start_menu_open);
}

/// The popup stays on the display, at any scaling and any screen size, and
/// sits above the taskbar whenever there is room for it.
///
/// When there is not — 640×480 at 200% scaling gives 400 px above the bar for
/// a 480 px popup — it is pinned to the top of the display and overlaps the
/// bar rather than running off the screen. That is the right way round, and
/// the test asserts it rather than skipping the case: everything the popup is
/// steered by lives in its first eighty pixels, so losing the bottom of the
/// grid leaves it usable while losing the top would not.
#[test]
fn the_calendar_is_placed_on_screen_above_the_taskbar() {
    for percent in [100_u16, 150, 200] {
        for (w, h) in [(1000_u32, 800_u32), (640, 480), (3840, 2160)] {
            let mut shell = scaled(percent);
            shell.screen_width = w;
            shell.screen_height = h;
            click_clock(&mut shell);
            assert!(shell.calendar.visible);

            let frame = month_layout(&shell).frame;
            let bar = shell.taskbar_rect();
            assert!(frame.x >= 0.0 && frame.y >= 0.0, "{frame:?} is off screen");
            assert!(
                frame.x + frame.w <= shell.screen_width as f32 + 0.5,
                "{frame:?} runs off the right of a {w}x{h} display"
            );
            if frame.h <= bar.y {
                assert!(
                    frame.y + frame.h <= bar.y + 0.5,
                    "{frame:?} overlaps the taskbar with room to spare above it"
                );
            } else {
                assert!(
                    frame.y <= 0.5,
                    "{frame:?} is taller than the space above the taskbar, so it \
                     must be pinned to the top of the display"
                );
            }
        }
    }
}

/// The popup is drawn in the shell's pixels, not the toolkit's logical ones.
#[test]
fn the_calendar_grows_with_the_display_scaling() {
    let mut base = shell();
    click_clock(&mut base);
    let small = month_layout(&base).frame;

    let mut big = scaled(200);
    click_clock(&mut big);
    let large = month_layout(&big).frame;

    assert!((large.w - small.w * 2.0).abs() < 0.01, "{large:?}");
    assert!((large.h - small.h * 2.0).abs() < 0.01, "{large:?}");
}

/// The extra clocks the Date & Time panel can add finally reach a surface.
///
/// `AdditionalClock` has existed since that panel was written — up to four
/// zones, each named and each with a `visible` switch — and nothing anywhere
/// drew one, so `visible` was a flag whose only effect was to print "Hidden"
/// beside its own row in the panel that set it.
#[test]
fn the_extra_clocks_reach_the_calendars_header() {
    let mut shell = shell();
    assert!(shell.datetime.add_clock("Europe/London", "London"));
    assert!(shell.datetime.add_clock("Asia/Tokyo", "Tokyo"));
    assert!(shell.datetime.add_clock("Europe/Paris", "Paris"));
    // Hidden ones are configured but not drawn.
    shell.datetime.additional_clocks[1].visible = false;
    // A zone the table cannot resolve is dropped rather than shown at UTC
    // under its own label, which would be a wrong clock presented as a right
    // one.
    shell.datetime.additional_clocks.push(AdditionalClock {
        tz_id: "Mars/Olympus_Mons".to_string(),
        label: "Mars".to_string(),
        visible: true,
    });

    click_clock(&mut shell);
    let header = shell
        .calendar
        .header
        .as_ref()
        .expect("the popup has a band");
    let labels: Vec<&str> = header
        .clock
        .extra_timezones
        .iter()
        .map(|z| z.label.as_str())
        .collect();
    assert_eq!(labels, ["London", "Paris"]);

    // And the band's rows are actually drawn, below the popup's own top edge
    // and above its month grid.
    let layout = month_layout(&shell);
    let band = layout.clock_band().expect("a header band is laid out");
    let tree = shell.render_calendar().expect("the popup is open");
    for label in ["London", "Paris"] {
        assert!(
            tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, y, .. }
                    if text.starts_with(label) && *y >= band.y && *y <= band.y + band.h
            )),
            "{label} is not drawn in the header band"
        );
    }
    assert!(
        !tree
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.starts_with("Tokyo"))),
        "a hidden clock was drawn anyway"
    );
}

/// Reopening the popup rewinds it and re-reads the clock settings.
#[test]
fn reopening_the_calendar_rewinds_it() {
    let mut shell = shell();
    click_clock(&mut shell);
    let start = (shell.calendar.view_year, shell.calendar.view_month);

    let next = month_layout(&shell).next_arrow();
    click_at(&mut shell, next);
    click_clock(&mut shell);
    assert!(!shell.calendar.visible);

    // A clock added while it was shut is in the band when it reopens, because
    // the header is rebuilt on every open rather than cached at construction.
    assert!(shell.datetime.add_clock("Europe/London", "London"));
    click_clock(&mut shell);
    assert_eq!(
        (shell.calendar.view_year, shell.calendar.view_month),
        start,
        "the popup reopened where it was left"
    );
    assert_eq!(
        shell
            .calendar
            .header
            .as_ref()
            .map(|h| h.clock.extra_timezones.len()),
        Some(1)
    );
}

// ---- windows --------------------------------------------------------------

/// Restoring has to put the window back where it was. Before the geometry was
/// remembered, "restore" only changed the state flag and left the window
/// filling the screen — a button that looked broken.
///
/// Driven through `toggle_maximize` rather than through a click on a maximize
/// button, because the shell has no maximize button: the compositor draws and
/// hit-tests the title bar. What survives here is the state machine the
/// compositor drives, and that is what this asserts.
#[test]
fn maximizing_and_restoring_returns_the_window_to_where_it_was() {
    let mut shell = shell();
    let id = shell.add_window("A", 120, 90, 400, 300, 1);

    shell.toggle_maximize(id);
    let (_, _, work_w, work_h) = shell.work_area();
    assert_eq!(shell.windows[&id].state, WindowState::Maximized);
    assert_eq!(shell.windows[&id].width, work_w);
    assert_eq!(shell.windows[&id].height, work_h);

    shell.toggle_maximize(id);
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

/// A double-click is not the shell's gesture any more — the compositor owns the
/// title bar and resolves the timing itself — so the shell must treat the two
/// alike rather than keep a second, divergent answer for one of them.
///
/// "Alike" is asserted as *doing the same thing*, not as declining to do the old
/// thing. A `DoubleClick` arm that quietly returned `Pass` without dispatching
/// would satisfy "it no longer maximizes" perfectly while silently dropping
/// click-to-focus and every menu the shell opens on a press — the first draft of
/// this test asserted exactly that and caught exactly nothing. So each half
/// below checks an effect the do-nothing arm loses, on both kinds of surface the
/// shell hit-tests: a window, and its own chrome.
#[test]
fn a_double_click_is_the_same_event_to_this_shell_as_a_single_one() {
    let doubled = |x: f32, y: f32| MouseEvent {
        x,
        y,
        kind: MouseEventKind::DoubleClick(MouseButton::Left),
    };

    // On a window: it focuses, and it still does not maximize.
    let mut shell = shell();
    let a = shell.add_window("A", 120, 90, 400, 300, 1);
    let b = shell.add_window("B", 700, 90, 400, 300, 2);
    assert_eq!(shell.focused_window, Some(b));
    let (x, y) = centre(frame(&shell, a));
    assert_eq!(shell.handle_mouse(&doubled(x, y)), ShellAction::Pass);
    assert_eq!(shell.focused_window, Some(a), "double-click did not focus");
    assert_eq!(
        shell.windows[&a].state,
        WindowState::Normal,
        "double-click maximized — that gesture is the compositor's now"
    );

    // On the shell's own chrome: it opens the menu a press opens.
    let start = shell.start_button_rect();
    let (sx, sy) = (start.x + 8.0, start.y + 8.0);
    assert!(!shell.start_menu_open);
    shell.handle_mouse(&doubled(sx, sy));
    assert!(
        shell.start_menu_open,
        "double-click on the start button did nothing"
    );

    // And they are interchangeable in the other direction too: a press closes
    // what a double-click opened.
    shell.handle_mouse(&click(sx, sy));
    assert!(!shell.start_menu_open);
}

/// Click-to-focus raises the window and still lets the click reach it, so that
/// the first click on a background window presses what it landed on.
#[test]
fn a_click_in_a_window_focuses_it_and_is_passed_on() {
    let mut shell = shell();
    let a = shell.add_window("A", 0, 0, 400, 300, 1);
    let b = shell.add_window("B", 500, 0, 400, 300, 2);
    assert_eq!(shell.focused_window, Some(b));

    let (x, y) = centre(frame(&shell, a));
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Pass);
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
    assert!(
        shell
            .start_menu_row_rect(1)
            .contains(first.x + 4.0, boundary)
    );
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

/// A window can be any size the compositor gives it, including one too small to
/// have held the title bar the shell used to draw. It is still a window, and a
/// point on it still belongs to it.
#[test]
fn a_window_smaller_than_any_decoration_is_still_hit() {
    let mut shell = shell();
    let id = shell.add_window("A", 100, 100, 200, 10, 1);
    let rect = frame(&shell, id);
    assert_eq!(rect.h, 10.0);
    let (x, y) = centre(rect);
    assert_eq!(shell.hit_test(x, y), Hit::WindowContent(id));
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

/// The shell's own chrome is authored in logical pixels and drawn in physical
/// ones. A window is the compositor's and arrives already physical, so it is
/// the one measurement that must *not* be scaled again — a shell that scaled it
/// would hit-test a window somewhere other than where it is.
#[test]
fn the_display_scaling_moves_the_shells_chrome_and_leaves_the_windows_alone() {
    let mut hundred = shell();
    let a = hundred.add_window("A", 100, 100, 400, 300, 1);
    let mut two_hundred = scaled(200);
    let b = two_hundred.add_window("A", 100, 100, 400, 300, 1);

    assert_eq!(frame(&hundred, a), frame(&two_hundred, b));
    for percent in [100, 125, 150, 200] {
        let mut shell = scaled(percent);
        let id = shell.add_window("A", 100, 100, 600, 400, 1);
        let (x, y) = centre(frame(&shell, id));
        assert_eq!(
            shell.hit_test(x, y),
            Hit::WindowContent(id),
            "at {percent}% the window moved out from under itself"
        );
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
            shell.scroll_start_menu(1);
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

/// The setting has to reach every surface the shell rounds, not just the one
/// that happened to be checked. Windows are absent on purpose: the compositor
/// rounds those, and proves it in
/// `compositor::tests::the_users_corner_setting_reaches_the_window_frame`.
#[test]
fn the_corner_setting_reaches_the_surfaces_the_shell_draws() {
    for corners in [
        WindowCorners::Square,
        WindowCorners::Subtle,
        WindowCorners::Rounded,
        WindowCorners::ExtraRounded,
    ] {
        let mut shell = with_corners(corners);
        shell.toggle_start_menu();
        let menu = shell.render_start_menu().expect("the menu is open");
        assert_eq!(
            fill_radii(&menu)[0],
            CornerRadii::all(corners.radius()),
            "{corners:?}"
        );
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
    shell.toggle_start_menu();
    let menu = shell.render_start_menu().expect("the menu is open");
    assert_eq!(fill_radii(&menu)[0].top_left, 16.0);
}

#[test]
fn drop_shadows_are_drawn_only_when_the_user_asks_for_them() {
    for (wanted, expected) in [(true, 1), (false, 0)] {
        let mut shell = shell();
        let mut appearance = AppearanceSettings::default();
        appearance.drop_shadows = wanted;
        shell.set_appearance(appearance);
        shell.toggle_start_menu();

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
    shell.toggle_start_menu();
    let tree = shell.render_start_menu().expect("the menu is open");
    let shadow = tree
        .commands
        .iter()
        .position(|cmd| matches!(cmd, RenderCommand::BoxShadow { .. }))
        .expect("a shadow");
    let surface = tree
        .commands
        .iter()
        .position(|cmd| matches!(cmd, RenderCommand::FillRect { .. }))
        .expect("the menu panel");
    assert!(shadow < surface);
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
        shell.power_menu_open = true;
        shell.alt_tab_active = true;
        let mut sizes = text_sizes(&shell.render_taskbar());
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
