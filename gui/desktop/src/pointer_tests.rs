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
use crate::icons;
use crate::launcher::{self, Category};
use crate::snap;
use crate::{
    DesktopShell, Hit, Key, KeyEvent, Layer, ManagedWindow, Modifiers, MouseButton, MouseEvent,
    MouseEventKind, Rect, START_BUTTON_WIDTH, START_MENU_ROW_HEIGHT, ShellAction,
    ShellControlAction, ShellRequest, TextRole, WindowId, WindowInfo, WindowList, WindowState,
    click, scroll, scroll_rows,
};
use appearance::{AppearanceSettings, WindowCorners};
use guitk::render::{RenderCommand, RenderTree};
use guitk::style::CornerRadii;
use guitk::wheel;

fn shell() -> DesktopShell {
    DesktopShell::new(1000, 800)
}

/// A shell whose start menu lists more programs than it can show, so that
/// scrolling has somewhere to go.
///
/// Made by adding programs rather than by counting on how many the built-in
/// database holds. The scrolling tests used to take "more than the menu can
/// show" from the database's size, and on 2026-09-25 the database lost three
/// entries that had never started anything -- and two of those tests lost
/// their scroll with them, while two more went on passing because a list that
/// cannot scroll does not move either way.
fn shell_with_a_long_menu() -> DesktopShell {
    let mut shell = shell();
    for n in 0..12 {
        shell.apps.push(launcher::AppEntry {
            name: format!("Program {n:02}"),
            description: String::new(),
            executable_path: format!("/opt/fixture/program-{n:02}"),
            keywords: Vec::new(),
            category: Category::Application,
            launch_count: 0,
        });
    }
    assert!(
        shell.start_menu_max_scroll() >= 6,
        "the fixture must list more programs than the menu can show"
    );
    shell
}

/// A window list on the desktop the user is looking at.
///
/// Almost every test in this file is about a click landing somewhere, and none
/// is about virtual desktops — so they all say desktop 0 and mean "the one
/// showing". The tests that *are* about desktops build the list themselves,
/// with numbers that differ, which is the only way the difference can matter.
fn here(windows: &[WindowInfo]) -> WindowList {
    WindowList::new(0, windows.to_vec())
}

/// The centre of a rectangle — where a user aiming at a control clicks.
fn centre(rect: Rect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

fn click_at(shell: &mut DesktopShell, rect: Rect) -> ShellAction {
    let (x, y) = centre(rect);
    shell.handle_mouse(&click(x, y))
}

/// Press and let go in the middle of `rect`, answering what the *release*
/// asked for -- for what acts on the release: a start-menu row, which does
/// not know it was a click rather than the start of a drag until then.
fn choose_at(shell: &mut DesktopShell, rect: Rect) -> ShellAction {
    let (x, y) = centre(rect);
    assert_eq!(
        shell.handle_mouse(&click(x, y)),
        ShellAction::Consumed,
        "the press acted before the release could say whether it was a drag"
    );
    shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Release(MouseButton::Left),
    })
}

/// An ordinary application window as the compositor would describe it. Nothing
/// here has geometry: where a window is is not something the shell is told,
/// because it is not something the shell decides.
fn app(id: u64, title: &str) -> WindowInfo {
    WindowInfo::new(id, id, title)
}

/// What the shell currently believes, in the compositor's own bottom-to-top
/// order, ready to be handed back with one thing changed.
///
/// Every helper below builds on this rather than calling a method on the shell:
/// there is no longer a method to call, and a live session never had one. A
/// window list is the only thing that moves the shell's idea of the desktop.
fn as_list(shell: &DesktopShell) -> Vec<WindowInfo> {
    let mut windows: Vec<&ManagedWindow> = shell.windows.values().collect();
    windows.sort_by_key(|window| window.z_order);
    windows
        .into_iter()
        .map(|window| {
            let mut info = app(window.id.0, &window.title);
            info.minimized = window.state == WindowState::Minimized;
            info.maximized = window.state == WindowState::Maximized;
            info.focused = window.focused;
            info
        })
        .collect()
}

/// A window opens: one more entry in the next list, on top and holding the
/// focus, which is what a newly-mapped window is. The id is the compositor's to
/// choose; this stands in for it by taking the next one the shell has not seen.
fn open(shell: &mut DesktopShell, title: &str) -> WindowId {
    let id = shell
        .windows
        .keys()
        .map(|id| id.0)
        .max()
        .map_or(1, |top| top + 1);
    let mut list = as_list(shell);
    for other in &mut list {
        other.focused = false;
    }
    let mut fresh = app(id, title);
    fresh.focused = true;
    list.push(fresh);
    shell.apply_window_list(&here(&list));
    WindowId(id)
}

/// The compositor minimized a window — the shell finds out the only way it
/// can, which is the next list.
fn minimize(shell: &mut DesktopShell, id: WindowId) {
    let mut list = as_list(shell);
    for info in &mut list {
        if info.id == id.0 {
            info.minimized = true;
            info.focused = false;
        }
    }
    shell.apply_window_list(&here(&list));
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
    let action = choose_at(&mut shell, rect);
    assert_eq!(
        action,
        ShellAction::Launch(crate::hotkeys::Launch::program("/usr/bin/settings"))
    );
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
    // More programs than rows, so that every visible row has one to launch.
    let mut shell = shell_with_a_long_menu();
    for row in 0..shell.start_menu_visible_rows() {
        shell.toggle_start_menu();
        let expected = shell.start_menu_entries()[row].executable_path.clone();
        let rect = shell.start_menu_row_rect(row);
        let action = choose_at(&mut shell, rect);
        assert_eq!(
            action,
            ShellAction::Launch(crate::hotkeys::Launch::program(&expected)),
            "row {row}"
        );
    }
}

/// And must go on doing so once the list has been scrolled, which is where a
/// renderer and a hit test that each tracked the offset would part company.
#[test]
fn a_scrolled_row_launches_the_program_named_on_it() {
    let mut shell = shell_with_a_long_menu();
    shell.toggle_start_menu();

    let (x, y) = centre(shell.start_menu_row_rect(0));
    // One detent, which is three rows. This used to read
    // `-3.0 * START_MENU_ROW_HEIGHT` — a delta in pixels, matching the units
    // the handler wrongly assumed rather than the notches it is actually sent.
    shell.handle_mouse(&scroll(x, y, -1.0));
    assert_eq!(shell.start_menu_scroll, 3);

    let expected = shell.start_menu_entries()[3].executable_path.clone();
    let rect = shell.start_menu_row_rect(0);
    let action = choose_at(&mut shell, rect);
    assert_eq!(
        action,
        ShellAction::Launch(crate::hotkeys::Launch::program(&expected))
    );
}

#[test]
fn the_list_cannot_scroll_past_either_end() {
    let mut shell = shell_with_a_long_menu();
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
    let mut shell = shell_with_a_long_menu();
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
    let mut shell = shell_with_a_long_menu();
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
    let mut shell = shell_with_a_long_menu();
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
    shell.toggle_start_menu();

    // Somewhere the shell draws nothing: the top-right of a 1000x800 screen,
    // with the taskbar along the bottom and the menu above the start button at
    // the left. Whatever is there belongs to somebody else — which is the
    // point, since the rule under test is that the click never reaches them.
    let (x, y) = (900.0, 40.0);
    assert_eq!(shell.hit_test(x, y), Hit::Desktop);
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert!(!shell.start_menu_open);
    // Spent means spent: with the menu shut, the same click is passed straight
    // through, so the one that closed it was withheld deliberately.
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Pass);
}

#[test]
fn a_click_on_the_menu_but_not_on_a_row_does_nothing_but_stay_open() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let menu = shell.start_menu_rect();
    // The places column's top, where the user is: the menu, and not a row
    // or a place.
    let user = shell.start_user_rect();
    assert!(menu.contains(user.x + user.w / 2.0, user.y + user.h / 2.0));
    let action = shell.handle_mouse(&click(user.x + user.w / 2.0, user.y + user.h / 2.0));
    assert_eq!(action, ShellAction::Consumed);
    assert!(shell.start_menu_open);
}

// ---- the power menu -------------------------------------------------------

/// The whole point: the machine can be shut down from the desktop. Before the
/// power menu the foot of the start menu drew the word "Power" in grey and did
/// nothing; before 2026-09-25 the menu launched `/sbin/shutdown` and its
/// neighbours, which SlateOS has never had.
#[test]
fn the_power_menu_offers_every_power_action_and_carries_each_out() {
    let labels: Vec<&str> = shell()
        .power_menu_choices()
        .iter()
        .map(|choice| choice.label())
        .collect();
    assert_eq!(
        labels,
        [
            "Shut down",
            "Restart",
            "Sleep",
            "Hibernate",
            "Lock",
            "Log out"
        ]
    );

    for (row, choice) in crate::power::PowerChoice::ALL.iter().enumerate() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let button = shell.power_button_rect();
        assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
        assert!(shell.power_menu_open);

        let rect = shell.power_menu_row_rect(row);
        let expected = choice
            .command()
            .map_or(ShellAction::LogOut, ShellAction::Launch);
        assert_eq!(click_at(&mut shell, rect), expected, "{choice:?}");
        // Both menus go: the machine is about to change state behind them.
        assert!(!shell.power_menu_open);
        assert!(!shell.start_menu_open);
    }
}

/// The machine is turned off, restarted, put to sleep and hibernated by
/// `powerctl`, a program SlateOS has, each with its own subcommand; the lock
/// is the lock screen's; log out is the shell's own.
#[test]
fn the_power_actions_are_carried_out_by_programs_that_exist() {
    use crate::power::{POWERCTL, PowerChoice};
    for (choice, subcommand) in [
        (PowerChoice::ShutDown, "shutdown"),
        (PowerChoice::Restart, "reboot"),
        (PowerChoice::Sleep, "suspend"),
        (PowerChoice::Hibernate, "hibernate"),
    ] {
        let launch = choice.command().expect("a program carries it out");
        assert_eq!(
            launch.program,
            std::path::PathBuf::from(POWERCTL),
            "{choice:?}"
        );
        assert_eq!(
            launch.args,
            [std::ffi::OsString::from(subcommand)],
            "{choice:?}"
        );
    }
    assert_eq!(
        PowerChoice::Lock.command(),
        Some(crate::hotkeys::Launch::program(
            crate::hotkeys::LOCK_COMMAND
        ))
    );
    assert_eq!(PowerChoice::LogOut.command(), None);
}

/// The login screen's power buttons do exactly what the start menu's do.
#[test]
fn the_login_screens_power_buttons_do_what_the_start_menus_do() {
    use crate::login_screen::LoginPowerAction as Login;
    use crate::power::PowerChoice as Menu;
    for (button, row) in [
        (Login::Shutdown, Menu::ShutDown),
        (Login::Reboot, Menu::Restart),
        (Login::Sleep, Menu::Sleep),
        (Login::Hibernate, Menu::Hibernate),
    ] {
        assert_eq!(Some(button.command()), row.command(), "{button:?}");
    }
}

/// The start menu lists every program in the database, once -- and the
/// database holds no power actions, which are the power menu's own list and
/// not programs at all. They were database entries until 2026-09-25, which is
/// how three of them came to name programs that do not exist.
#[test]
fn the_start_menu_offers_every_program_once_and_no_power_action() {
    let shell = shell();
    let database = launcher::builtin_app_database();
    let mut offered: Vec<&str> = shell
        .start_menu_entries()
        .iter()
        .map(|entry| entry.executable_path.as_str())
        .collect();
    offered.sort_unstable();
    let before = offered.len();
    offered.dedup();
    assert_eq!(before, offered.len(), "a program is listed twice");
    assert_eq!(offered.len(), database.len(), "a program is missing");
    for entry in &database {
        assert_ne!(
            entry.category,
            Category::System,
            "{} is back in the database",
            entry.name
        );
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

/// A submenu is allowed to cover the menu it opened from -- but then a click
/// anywhere in it has to reach the popup, not the place or the row buried
/// under it. Swept over the popup at three scales rather than tried at one
/// point: where the popup lands on the places column depends on the scale,
/// and at 100% it covers nothing at all.
#[test]
fn the_power_menu_takes_every_click_inside_it() {
    let mut covered_something = false;
    for percent in [100, 150, 200] {
        let mut shell = scaled(percent);
        shell.toggle_start_menu();
        shell.toggle_power_menu();
        let popup = shell.power_menu_rect();
        for i in 0..=8 {
            for j in 0..=8 {
                let x = popup.x + popup.w * (0.02 + 0.96 * i as f32 / 8.0);
                let y = popup.y + popup.h * (0.02 + 0.96 * j as f32 / 8.0);
                covered_something |= crate::StartShortcut::ALL
                    .iter()
                    .any(|w| shell.start_shortcut_rect(*w).contains(x, y))
                    || (0..shell.start_menu_visible_rows())
                        .any(|row| shell.start_menu_row_rect(row).contains(x, y));
                assert!(
                    matches!(
                        shell.hit_test(x, y),
                        Hit::PowerMenuEntry(_) | Hit::PowerMenuPanel
                    ),
                    "({x}, {y}) inside the popup at {percent}% went to {:?}",
                    shell.hit_test(x, y)
                );
            }
        }
    }
    assert!(
        covered_something,
        "the popup never covered anything, so this proves nothing"
    );
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
        choose_at(&mut by_launching, row),
        ShellAction::Launch(_)
    ));
    assert!(!by_launching.power_menu_open, "launching a program");
    assert!(!by_launching.start_menu_open);
}

#[test]
fn a_wheel_over_the_power_menu_does_not_scroll_the_list_behind_it() {
    let mut shell = shell_with_a_long_menu();
    shell.toggle_start_menu();
    shell.toggle_power_menu();

    let (x, y) = centre(shell.power_menu_row_rect(0));
    assert_eq!(
        shell.handle_mouse(&scroll(x, y, -3.0 * START_MENU_ROW_HEIGHT)),
        ShellAction::Consumed
    );
    assert_eq!(shell.start_menu_scroll, 0, "the hidden rows moved");
}

/// Scaling must not put a power action off the screen or out from under the
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
            shell.power_menu_choices().len(),
            "a power action was dropped at {percent}%"
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

    // Every power action is on screen, under its own label.
    let drawn: Vec<String> = tree
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for choice in shell.power_menu_choices() {
        assert!(
            drawn.iter().any(|text| text == choice.label()),
            "{choice:?} was not drawn"
        );
    }
}

// ---- the taskbar ----------------------------------------------------------

/// The button drawn in slot *n* must name the window listed in slot *n*.
///
/// Stated with the ids captured on the way in rather than read back out of
/// `taskbar_windows()`: `hit_test(centre(taskbar_button_rect(i))) ==
/// TaskbarButton(i)` — which is what this test used to say — compares the slot
/// number with itself, and no arrangement of windows can make it false. The
/// ids make it a claim about the *pairing*, which is the thing that would
/// actually strand a click on the wrong program.
#[test]
fn a_taskbar_button_is_clickable_where_it_is_drawn() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let c = open(&mut shell, "C");

    // Left to right in the order the compositor listed them, which `open`
    // appends to.
    for (index, id) in [a, b, c].into_iter().enumerate() {
        let rect = shell.taskbar_button_rect(index);
        let (x, y) = centre(rect);
        assert_eq!(
            shell.hit_test(x, y),
            Hit::TaskbarButton(id),
            "the button in slot {index} does not belong to the window there"
        );

        // The buttons are on the bar and in order, which is the part that no
        // amount of agreement between the hit test and the accessor can show.
        let bar = shell.taskbar_rect();
        assert!(
            rect.x >= bar.x + shell.start_button_rect().w,
            "button {index} is drawn over the start button"
        );
        assert!(
            rect.x + rect.w <= shell.tray_x(),
            "button {index} runs into the system tray"
        );
        assert!(
            rect.y >= bar.y && rect.y + rect.h <= bar.y + bar.h,
            "button {index} escapes the bar vertically"
        );
        if index > 0 {
            let previous = shell.taskbar_button_rect(index - 1);
            assert!(
                rect.x >= previous.x + previous.w,
                "buttons {} and {index} overlap",
                index - 1
            );
        }
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
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    assert_eq!(shell.focused_window, Some(b));

    // A is at index 0: the buttons stand in the order the windows opened.
    let first = shell.taskbar_button_rect(0);
    assert_eq!(
        choose_at(&mut shell, first),
        ShellAction::Control(ShellRequest::window(a, ShellControlAction::Activate)),
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
        .taskbar_windows()
        .iter()
        .position(|w| w.id == b)
        .unwrap();
    let button = shell.taskbar_button_rect(index);
    assert_eq!(
        choose_at(&mut shell, button),
        ShellAction::Control(ShellRequest::window(b, ShellControlAction::Minimize)),
        "the button of the focused window must put it away"
    );
    assert_eq!(
        shell.windows[&b].state,
        WindowState::Normal,
        "the shell minimized a window on its own authority"
    );
}

/// The window closed between the frame its button was drawn in and the press.
/// The click lands on bare taskbar, and bare taskbar keeps it: a bar that let
/// it through would raise whatever happened to be behind.
///
/// Named for the panel and not for the button, because the panel is what
/// answers. It was called `a_taskbar_button_whose_window_has_gone_swallows_the
/// _click`, which described a code path that did not exist —
/// [`Hit::TaskbarButton`] is resolved to a window by the same call that
/// produced it, so it never names one that has gone, and the `None` arm the
/// old name pointed at was unreachable. The behaviour is real; only the route
/// to it was misdescribed.
#[test]
fn a_click_where_a_button_used_to_be_is_still_the_taskbars() {
    let mut shell = shell();
    open(&mut shell, "A");
    let button = shell.taskbar_button_rect(0);
    shell.apply_window_list(&here(&[]));

    assert_eq!(
        shell.hit_test(button.x + 4.0, button.y + 4.0),
        Hit::TaskbarPanel
    );
    assert_eq!(click_at(&mut shell, button), ShellAction::Consumed);
}

// ---- the window list the taskbar is drawn from ----------------------------

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
    shell.apply_window_list(&here(&[app(9, "closed since")]));
    assert!(shell.windows.contains_key(&WindowId(9)));

    let mut second = app(2, "Editor");
    second.focused = true;
    shell.apply_window_list(&here(&[app(1, "Terminal"), second]));

    assert!(
        !shell.windows.contains_key(&WindowId(9)),
        "a window absent from the list kept its taskbar button"
    );
    let titles: Vec<&str> = shell
        .taskbar_windows()
        .iter()
        .map(|w| w.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Terminal", "Editor"],
        "in the order sent, bottom up"
    );
    assert_eq!(shell.focused_window, Some(WindowId(2)));
    assert_eq!(
        shell.taskbar_windows().last().map(|w| w.id),
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

    shell.apply_window_list(&here(&[wallpaper, app(3, "Editor"), bar]));

    let ids: Vec<WindowId> = shell.taskbar_windows().iter().map(|w| w.id).collect();
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

    shell.apply_window_list(&here(&[away, hidden, app(3, "Ordinary")]));

    assert_eq!(shell.windows[&WindowId(1)].state, WindowState::Minimized);
    assert!(
        shell.windows[&WindowId(1)].mapped,
        "a minimized window is still mapped -- the user put it away, it did not go away"
    );
    assert!(
        !shell.windows[&WindowId(1)].on_glass(),
        "a minimized window is not being drawn"
    );
    assert!(
        shell.windows.contains_key(&WindowId(2)),
        "an unmapped window keeps its id and its place in the stack"
    );

    // The assertion this test is named for, and which it did not used to make:
    // it asserted `[WindowId(3)]` -- that the minimized window had lost its
    // button -- which is the bug itself, written down as the expectation. A
    // test whose name promises one behaviour and whose body pins the opposite
    // is worse than no test, because a reader who greps for the promise finds
    // it and stops looking.
    let listed: Vec<WindowId> = shell.taskbar_windows().iter().map(|w| w.id).collect();
    assert_eq!(
        listed,
        [WindowId(1), WindowId(3)],
        "the minimized window keeps its button; only the unmapped one loses it"
    );
}

#[test]
fn a_minimized_window_can_be_got_back_from_its_taskbar_button() {
    // The other half of the same rule, and the reason it matters. Clicking the
    // button of a window that is not focused asks for `Activate` -- which the
    // compositor implements as un-minimize-then-focus precisely so that this
    // click works. That care was unreachable code for as long as minimizing a
    // window deleted its button: there was no way left to click.
    let mut shell = shell();
    let id = open(&mut shell, "Editor");
    minimize(&mut shell, id);

    assert_eq!(
        shell.taskbar_windows().len(),
        1,
        "the button must still be there to be clicked"
    );
    let button = shell.taskbar_button_rect(0);
    assert_eq!(
        choose_at(&mut shell, button),
        ShellAction::Control(ShellRequest::window(id, ShellControlAction::Activate)),
        "the click must ask for the window back, not minimize it again"
    );
}

#[test]
fn alt_tab_reaches_a_minimized_window() {
    // The switcher lists the same set for the same reason: Alt+Tab is the other
    // standing way to reach a window you put away, and a switcher that silently
    // skipped minimized windows made the last one you minimized unreachable by
    // either route at once.
    let mut shell = shell();
    let first = open(&mut shell, "Editor");
    open(&mut shell, "Terminal");
    minimize(&mut shell, first);

    assert_eq!(shell.taskbar_windows().len(), 2, "both are still listed");
    shell.start_alt_tab();
    assert!(
        shell.alt_tab_active,
        "two windows is enough to switch between"
    );
    assert!(
        shell
            .taskbar_windows()
            .iter()
            .any(|w| w.id == first && w.state == WindowState::Minimized),
        "the minimized window must be among the rows the switcher steps through"
    );
}

#[test]
fn show_desktop_does_not_ask_an_already_minimized_window_to_minimize() {
    // The one caller that wants the *narrow* question. Super+D means "clear the
    // screen", so it has nothing to say to a window that is already away --
    // asking would be a request the compositor must ignore and, worse, one the
    // user would have to undo twice to get back where they were. This is why
    // the fix is two accessors rather than one widened one.
    //
    // This test used to build `asked` by copying `run_desktop_action`'s filter
    // into its own body and asserting against the copy. That proved the copy:
    // widening the production predicate from `on_glass()` to `mapped` left it
    // green, because the test was no longer reading the code it was named for.
    // A test that re-derives the answer is worse than no test, because it looks
    // like coverage. So it presses the chord and reads what the shell asked for.
    let mut shell = shell();
    let away = open(&mut shell, "Editor");
    let still_here = open(&mut shell, "Terminal");
    minimize(&mut shell, away);

    let super_d = KeyEvent {
        key: Key::D,
        pressed: true,
        modifiers: Modifiers {
            super_key: true,
            ..Modifiers::default()
        },
        text: String::new(),
    };
    let outcome = shell.handle_hotkey(&super_d);
    assert!(outcome.consumed);

    let asked: Vec<WindowId> = outcome
        .requests
        .iter()
        .map(|request| match request {
            ShellRequest::Window(w) => {
                assert_eq!(w.action, ShellControlAction::Minimize);
                w.window
            }
            other => panic!("Super+D names windows and nothing else, got {other:?}"),
        })
        .collect();
    assert_eq!(
        asked,
        [still_here],
        "only the window still on the glass should be asked to go away"
    );
}

#[test]
fn a_retitle_reaches_the_button_without_disturbing_shell_local_state() {
    // The update a taskbar exists to show, and the one most likely to be lost:
    // a window that is already known must be updated in place, not rebuilt,
    // because the shell holds per-window state the compositor knows nothing
    // about and cannot send back.
    //
    // The virtual desktop used to be on that list and is not any more: it is
    // the compositor's field now, arrives with every window, and a shell that
    // preserved its own copy across an update would be preserving a stale one.
    let mut shell = shell();
    shell.apply_window_list(&here(&[app(1, "untitled")]));
    shell.windows.get_mut(&WindowId(1)).unwrap().icon_id = 42;

    let mut retitled = app(1, "notes.txt — saved");
    retitled.workspace = 3;
    shell.apply_window_list(&here(&[retitled]));

    let win = &shell.windows[&WindowId(1)];
    assert_eq!(win.title, "notes.txt — saved");
    assert_eq!(win.icon_id, 42, "the icon was rebuilt from nothing");
    assert_eq!(
        win.desktop, 3,
        "the desktop the compositor named was ignored in favour of a local copy"
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
    shell.apply_window_list(&here(&[only]));
    assert_eq!(shell.focused_window, Some(WindowId(1)));

    shell.apply_window_list(&here(&[]));
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
    shell.apply_window_list(&here(&[focused]));

    let button = shell.taskbar_button_rect(0);
    let action = choose_at(&mut shell, button);
    assert_eq!(
        action,
        ShellAction::Control(ShellRequest::window(
            WindowId(1),
            ShellControlAction::Minimize
        ))
    );
    assert!(
        shell.taskbar_windows().len() == 1,
        "the shell acted on the request itself"
    );

    // The compositor did as it was asked and said so.
    let mut away = app(1, "Editor");
    away.minimized = true;
    shell.apply_window_list(&here(&[away]));
    // The button survives the minimize -- this used to assert
    // `taskbar_windows().is_empty()`, which is the stranded-window bug stated
    // as an expectation for the second time in this file. What the round trip
    // actually shows is that the *state* moved and the listing did not.
    assert_eq!(shell.taskbar_windows().len(), 1);
    assert_eq!(shell.windows[&WindowId(1)].state, WindowState::Minimized);
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

/// The clock's target has to cover the reading the taskbar actually draws, at
/// every display scaling.
///
/// This is the one control on the taskbar whose drawn position and whose
/// clickable target are computed *independently*: `clock_rect` right-aligns a
/// slot to the display edge, and `render_taskbar` right-aligns the text to the
/// same edge with its own arithmetic. Every other control is drawn from the
/// very accessor `hit_test` resolves against, so asserting those two agree
/// would assert nothing at all — which is exactly what a first draft of a
/// scaling test here did, and it passed with the scaling deleted from the
/// taskbar's button spacing.
///
/// Both halves scale, and they must scale together. A target measured with one
/// unscaled term leaves the last few characters of the clock inert, which reads
/// as "the clock is not clickable" rather than as an off-by-a-few-pixels
/// rectangle — and at 100%, where everyone develops, the error is zero.
#[test]
fn the_clocks_target_covers_the_reading_that_is_drawn_at_every_scaling() {
    for percent in [100, 125, 150, 200] {
        let shell = scaled(percent);
        let target = shell.clock_rect();
        // The clock is the rightmost thing on the taskbar, so the rightmost
        // text command is it — the desktop indicator sits at the tray's left
        // edge.
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

        // The slot is sized for the *widest* reading the switches allow, so
        // check that one rather than the current second.
        let widest = shell.clock_width();
        assert!(
            x >= target.x,
            "at {percent}% the reading starts left of its target"
        );
        assert!(
            x + widest <= target.x + target.w + 0.5,
            "at {percent}% the reading runs past its target"
        );
        assert!(
            y >= target.y && y + size <= target.y + target.h,
            "at {percent}% the reading is drawn outside its target vertically"
        );
        // And the pixel it is drawn on is the pixel that opens the calendar.
        assert_eq!(
            shell.hit_test(x, y + size / 2.0),
            Hit::Clock,
            "at {percent}% the clock is drawn somewhere it cannot be clicked"
        );
    }
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
    let id = open(&mut shell, "Terminal");
    minimize(&mut shell, id);
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
        text: String::new(),
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

/// A double-click is not the shell's gesture — the compositor owns the title
/// bar and resolves the timing itself — so the shell must treat the two alike
/// rather than keep a second, divergent answer for one of them.
///
/// "Alike" is asserted as *doing the same thing*, not as declining to do the old
/// thing. A `DoubleClick` arm that quietly returned `Pass` without dispatching
/// would satisfy "it no longer maximizes" perfectly while silently dropping
/// every menu the shell opens on a press and every request its taskbar makes —
/// the first draft of this test asserted exactly that and caught exactly
/// nothing. So each half below checks an effect the do-nothing arm loses, on
/// both kinds of control the shell hit-tests: one that asks the compositor for
/// something, and one that only moves the shell's own state.
#[test]
fn a_double_click_is_the_same_event_to_this_shell_as_a_single_one() {
    let doubled = |x: f32, y: f32| MouseEvent {
        x,
        y,
        kind: MouseEventKind::DoubleClick(MouseButton::Left),
    };

    // On a taskbar button: the same request a single click makes.
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    assert_eq!(shell.focused_window, Some(b));
    let (x, y) = centre(shell.taskbar_button_rect(0));
    // Taken hold of, like a press -- and the release asks, as it does after a
    // single click. A `DoubleClick` arm that did nothing would leave the
    // release nothing to finish.
    assert_eq!(shell.handle_mouse(&doubled(x, y)), ShellAction::Consumed);
    assert_eq!(
        shell.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        }),
        ShellAction::Control(ShellRequest::window(a, ShellControlAction::Activate)),
        "double-click on a taskbar button asked for nothing"
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

// ---- desktop icons --------------------------------------------------------
//
// These were drawn on 2026-09-14 with nothing able to click them --
// `TD-C-THE-DESKTOP-ICONS-ARE-DRAWN-AND-NOTHING-CAN-CLICK-THEM`, filed by this
// lane against itself. Every test below goes through `handle_mouse`, the route
// a real pointer takes, because the interactions were all implemented and
// tested *inside* `icons.rs` while the route to them did not exist. Testing the
// layer again would have proved what was never in doubt.

/// A shell whose desktop has icons on it.
///
/// `populate_icons` reads the saved positions, so the caller must be holding a
/// scratch configuration directory -- which is why every test below is wrapped.
fn shell_with_icons() -> DesktopShell {
    let mut shell = shell();
    shell.populate_icons();
    shell
}

/// The middle of the first icon, in screen coordinates.
fn first_icon_point(shell: &DesktopShell) -> (f32, f32) {
    let id = *shell.icons.icon_ids().first().expect("no icons");
    let icon = shell
        .icons
        .get_icon(id)
        .expect("the id just came from here");
    #[allow(clippy::cast_precision_loss)]
    (icon.x as f32 + 8.0, icon.y as f32 + 8.0)
}

/// **A press on an icon selects it.**
#[test]
fn pressing_an_icon_selects_it() {
    settingsfile::testing::with_scratch_config("icons-press-selects", |_root| {
        let mut shell = shell_with_icons();
        assert!(shell.icons.selected_ids().is_empty());
        let (x, y) = first_icon_point(&shell);
        assert_eq!(shell.hit_test(x, y), Hit::Desktop, "not over bare desktop");

        assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);

        assert_eq!(
            shell.icons.selected_ids().len(),
            1,
            "the press did not reach the icon layer"
        );
    });
}

/// **A double-click on a folder icon opens the folder in the file manager.**
///
/// Until 2026-09-25 it asked for the folder to be *run*: the action named the
/// icon's path as the program, and `Command::new` on a directory fails. The
/// test that stood here asserted exactly that request, against a path that did
/// not exist, so it passed while the thing it described could never happen.
#[test]
fn double_clicking_a_folder_icon_opens_it_in_the_file_manager() {
    settingsfile::testing::with_scratch_config("icons-double-click", |root| {
        let mut shell = shell_with_icons();
        // A folder that exists, since opening one is a question about the
        // filesystem. Added here rather than found among the defaults: since
        // 2026-09-15 Home and Documents appear only when `HOME` is set, and a
        // test should not depend on the environment the runner has.
        let folder = root.join("Somewhere");
        std::fs::create_dir(&folder).expect("the scratch root is writable");
        let id = shell.icons.add_icon(
            "Somewhere",
            icons::IconType::Folder,
            icons::IconAction::OpenPath(folder.clone()),
            40,
            40,
        );
        let (x, y) = {
            let icon = shell.icons.get_icon(id).expect("just added");
            #[allow(clippy::cast_precision_loss)]
            (icon.x as f32 + 8.0, icon.y as f32 + 8.0)
        };

        let event = MouseEvent {
            x,
            y,
            kind: MouseEventKind::DoubleClick(MouseButton::Left),
        };
        assert_eq!(
            shell.handle_mouse(&event),
            ShellAction::Launch(crate::hotkeys::Launch::opening(
                crate::launcher::FILE_MANAGER,
                &folder
            )),
            "a double-click on a folder did not open it in the file manager"
        );
    });
}

/// Double-click the icon `id`, through the route a real pointer takes.
fn double_click_icon(shell: &mut DesktopShell, id: icons::IconId) -> ShellAction {
    let (x, y) = {
        let icon = shell.icons.get_icon(id).expect("the icon exists");
        #[allow(clippy::cast_precision_loss)]
        (icon.x as f32 + 8.0, icon.y as f32 + 8.0)
    };
    shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::DoubleClick(MouseButton::Left),
    })
}

/// The "cannot open" notices posted so far, as `(title, body)`.
fn cannot_open_notices(shell: &DesktopShell) -> Vec<(String, String)> {
    shell
        .notifications
        .notifications()
        .iter()
        .filter(|n| n.title.starts_with("Cannot open"))
        .map(|n| (n.title.clone(), n.body.clone()))
        .collect()
}

/// An icon on the desktop for `path`.
fn icon_for(shell: &mut DesktopShell, label: &str, path: &std::path::Path) -> icons::IconId {
    shell.icons.add_icon(
        label,
        icons::IconType::File,
        icons::IconAction::OpenPath(path.to_path_buf()),
        600,
        400,
    )
}

/// **A document opens in the program File Associations chose for its kind**
/// -- the lookup the file manager makes, so the desktop and the file manager
/// cannot disagree about what a file opens in.
#[test]
fn double_clicking_a_document_opens_it_in_its_chosen_program() {
    settingsfile::testing::with_scratch_config("icons-open-document", |root| {
        let mut doc = yamldoc::Document::new();
        doc.set_str(&[associations::ASSOCIATIONS, "txt"], "/usr/bin/editor");
        appearance::config::store(associations::CONFIG_NAME, &doc)
            .expect("the scratch config directory is writable");
        let file = root.join("notes.TXT");
        std::fs::write(&file, b"hello").expect("the scratch root is writable");

        let mut shell = shell_with_icons();
        let id = icon_for(&mut shell, "notes", &file);
        assert_eq!(
            double_click_icon(&mut shell, id),
            ShellAction::Launch(crate::hotkeys::Launch::opening("/usr/bin/editor", &file)),
            "an upper-case extension is the same kind of file"
        );
        assert!(cannot_open_notices(&shell).is_empty());
    });
}

/// **A document nothing is set to open says so**, and points at where to
/// choose something, rather than doing nothing the user can see.
#[test]
fn a_document_nothing_opens_says_so() {
    settingsfile::testing::with_scratch_config("icons-open-nothing", |root| {
        let file = root.join("mystery.xyz");
        std::fs::write(&file, b"?").expect("the scratch root is writable");
        let mut shell = shell_with_icons();
        let id = icon_for(&mut shell, "mystery", &file);

        assert_eq!(double_click_icon(&mut shell, id), ShellAction::Consumed);
        let notices = cannot_open_notices(&shell);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert_eq!(notices[0].0, "Cannot open mystery");
        assert!(notices[0].1.contains("File Associations"), "{notices:?}");
    });
}

/// **An icon whose file has gone says so**, naming the path.
#[test]
fn an_icon_whose_file_is_gone_says_so() {
    settingsfile::testing::with_scratch_config("icons-open-missing", |root| {
        let gone = root.join("gone.txt");
        let mut shell = shell_with_icons();
        let id = icon_for(&mut shell, "gone", &gone);

        assert_eq!(double_click_icon(&mut shell, id), ShellAction::Consumed);
        let notices = cannot_open_notices(&shell);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].1.contains("is not there any more"),
            "{notices:?}"
        );
        assert!(notices[0].1.contains("gone.txt"), "{notices:?}");
    });
}

/// **"This PC" opens the machine's files, from the top, in the file manager.**
/// It used to do nothing: nothing said what its destination was.
#[test]
fn this_pc_opens_the_root_in_the_file_manager() {
    settingsfile::testing::with_scratch_config("icons-open-this-pc", |_root| {
        let mut shell = shell_with_icons();
        let id = shell
            .icons
            .icon_ids()
            .into_iter()
            .find(|id| {
                shell
                    .icons
                    .get_icon(*id)
                    .is_some_and(|i| i.label == "This PC")
            })
            .expect("This PC is a default icon");

        assert_eq!(
            double_click_icon(&mut shell, id),
            ShellAction::Launch(crate::hotkeys::Launch::opening(
                launcher::FILE_MANAGER,
                std::path::Path::new("/")
            ))
        );
    });
}

/// **The Recycle Bin says it cannot show its contents yet**, rather than
/// opening its storage -- internal folders named by ids -- or nothing at all.
#[test]
fn the_recycle_bin_says_it_cannot_show_its_contents_yet() {
    settingsfile::testing::with_scratch_config("icons-open-bin", |_root| {
        let mut shell = shell_with_icons();
        let id = shell
            .icons
            .icon_ids()
            .into_iter()
            .find(|id| {
                shell
                    .icons
                    .get_icon(*id)
                    .is_some_and(|i| i.label == "Recycle Bin")
            })
            .expect("the Recycle Bin is a default icon");

        assert_eq!(double_click_icon(&mut shell, id), ShellAction::Consumed);
        let notices = cannot_open_notices(&shell);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert_eq!(notices[0].0, "Cannot open Recycle Bin");
    });
}

/// **A program on the desktop runs** -- what a program shortcut is. And a
/// program whose kind *has* a chosen opener is opened by it, not run: a disk
/// that marks every file executable must not turn every document into a
/// program.
#[cfg(unix)]
#[test]
fn a_program_on_the_desktop_runs_and_a_document_marked_executable_does_not() {
    use std::os::unix::fs::PermissionsExt;
    settingsfile::testing::with_scratch_config("icons-open-program", |root| {
        let tool = root.join("tool");
        std::fs::write(&tool, b"#!/bin/sh\n").expect("the scratch root is writable");
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut doc = yamldoc::Document::new();
        doc.set_str(&[associations::ASSOCIATIONS, "txt"], "/usr/bin/editor");
        appearance::config::store(associations::CONFIG_NAME, &doc).expect("store");
        let text = root.join("readme.txt");
        std::fs::write(&text, b"hi").expect("write");
        std::fs::set_permissions(&text, std::fs::Permissions::from_mode(0o777)).expect("chmod");

        let mut shell = shell_with_icons();
        let id = icon_for(&mut shell, "tool", &tool);
        assert_eq!(
            double_click_icon(&mut shell, id),
            ShellAction::Launch(crate::hotkeys::Launch::program(&tool))
        );
        let id = icon_for(&mut shell, "readme", &text);
        assert_eq!(
            double_click_icon(&mut shell, id),
            ShellAction::Launch(crate::hotkeys::Launch::opening("/usr/bin/editor", &text))
        );
    });
}

/// **Dragging an icon moves it, and the release marks the layout for saving.**
///
/// The session does the write, at the end of the pump -- see
/// `an_icon_dragged_in_a_session_is_where_it_was_left_after_a_restart` in the
/// session's tests for that half. This one holds the shell to its part: a
/// move that is only on screen, and not marked, is lost at the next restart.
#[test]
fn dragging_an_icon_moves_it_and_the_move_is_kept() {
    settingsfile::testing::with_scratch_config("icons-drag", |_root| {
        let mut shell = shell_with_icons();
        let id = *shell.icons.icon_ids().first().expect("no icons");
        let (x, y) = first_icon_point(&shell);
        let before = shell
            .icons
            .get_icon(id)
            .map(|i| (i.x, i.y))
            .expect("an icon");

        shell.handle_mouse(&click(x, y));
        shell.handle_mouse(&MouseEvent {
            x: x + 240.0,
            y: y + 160.0,
            kind: MouseEventKind::Move,
        });
        shell.handle_mouse(&MouseEvent {
            x: x + 240.0,
            y: y + 160.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });

        let after = shell
            .icons
            .get_icon(id)
            .map(|i| (i.x, i.y))
            .expect("an icon");
        assert_ne!(after, before, "the drag did not move the icon");
        assert!(
            !shell.icons.is_interacting(),
            "the release did not end the gesture, so the layer is stranded"
        );

        // And it is marked, and what is written is what was dropped.
        assert!(
            shell.take_icons_dirty(),
            "the move was not marked, so nothing will save it"
        );
        shell
            .save_icon_layout()
            .expect("the scratch config directory should be writable");
        let mut restarted = shell_with_icons();
        assert_eq!(
            restarted.icons.get_icon(id).map(|i| (i.x, i.y)),
            Some(after),
            "the dragged position did not survive"
        );
        restarted.icons.deselect_all();
    });
}

/// A press on empty desktop clears a selection, and says it changed something.
#[test]
fn pressing_empty_desktop_clears_the_selection() {
    settingsfile::testing::with_scratch_config("icons-clear", |_root| {
        let mut shell = shell_with_icons();
        shell.icons.select_all();
        assert!(!shell.icons.selected_ids().is_empty());

        // Far from any icon, but still bare desktop.
        let (x, y) = (900.0, 700.0);
        assert_eq!(shell.hit_test(x, y), Hit::Desktop);
        assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);

        assert!(
            shell.icons.selected_ids().is_empty(),
            "the press did not clear the selection"
        );
    });
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
    let rows = shell.start_menu_visible_rows();
    assert!(rows >= 3, "the fixture must draw rows to compare");

    for row in 0..3 {
        let this = shell.start_menu_row_rect(row);
        let next = shell.start_menu_row_rect(row + 1);
        let boundary = this.y + this.h;
        let x = this.x + 4.0;

        // Where the seam is. This used to be the whole test, and it is only
        // half the property: it says the rows meet with no gap, and says
        // nothing at all about their meeting *twice*. Rows laid out one pixel
        // short still cover the boundary -- they simply cover the pixel before
        // it as well -- so a defect that made every row start early passed the
        // test the row-sharing was named after.
        assert!(!this.contains(x, boundary), "row {row} claims its own end");
        assert!(
            next.contains(x, boundary),
            "row {} does not start where row {row} ends",
            row + 1
        );

        // And that the seam is only one. The pixel before the boundary belongs
        // to this row and to nothing else.
        assert!(this.contains(x, boundary - 0.5));
        assert!(
            !next.contains(x, boundary - 0.5),
            "rows {row} and {} both claim the pixel above their boundary",
            row + 1
        );

        // Stated once more as arithmetic, because the two `contains` pairs
        // above still admit a pitch that is wrong by less than half a pixel,
        // which over a long list walks the last row off the menu.
        assert!(
            (next.y - boundary).abs() < 0.001,
            "row {} starts at {} but row {row} ends at {boundary}",
            row + 1,
            next.y
        );
    }
}

/// A screen too small for the chrome must produce empty rectangles, not
/// rectangles of negative size that swallow every click on the desktop.
#[test]
fn a_screen_smaller_than_the_taskbar_does_not_invert_the_geometry() {
    let shell = DesktopShell::new(200, 20);
    let bar = shell.taskbar_rect();
    assert_eq!(bar.y, 0.0);
    // The bar is clamped to the screen it is on. Checking only that it starts
    // at 0 leaves the far edge unstated, and the far edge is the half that can
    // run away: the bar's thickness is what the *user* asked for (48 logical
    // pixels by default, more at 200% scaling), so on a screen shorter than
    // that an unclamped bar hangs 28 pixels off the bottom -- a rectangle that
    // answers `contains` for coordinates the display does not have.
    assert!(
        bar.h <= shell.screen_height as f32,
        "the taskbar is {} tall on a {}-tall screen",
        bar.h,
        shell.screen_height
    );
    assert_eq!(
        bar.y + bar.h,
        shell.screen_height as f32,
        "the taskbar must sit on the bottom edge on a small screen too"
    );
    assert!(shell.taskbar_button_width() >= 0.0);
    assert_eq!(shell.work_area().3, 0);
    assert!(!Rect::new(0.0, 0.0, 0.0, 0.0).contains(0.0, 0.0));

    let menu = shell.start_menu_rect();
    assert!(menu.w >= 0.0 && menu.h >= 0.0);
    assert_eq!(shell.start_menu_visible_rows(), 0);
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

        // Hand-written arithmetic, not a second reading of the shell. Aiming
        // at the middle of `start_button_rect()` and finding the start button
        // there proves only that the hit test and the accessor agree -- and
        // they cannot disagree, because the hit test *is* a call to the
        // accessor. A button that stayed its logical width on a 200% display
        // would shrink the test's aim along with itself and pass. So the size
        // has to be asserted against a number written here.
        let expected = START_BUTTON_WIDTH * f32::from(percent) / 100.0;
        assert!(
            (start.w - expected).abs() < 0.01,
            "the start button is {} wide at {percent}% scaling, expected {expected}",
            start.w
        );
        assert!(
            (start.h - shell.taskbar_thickness()).abs() < 0.01,
            "the start button is not the full height of the bar at {percent}%"
        );

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
    open(&mut shell, "A");
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
        open(shell, "Terminal");
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

// ---- the zone-tiling chooser ----------------------------------------------

/// Super+Z, the chord that opens the chooser.
fn zone_key() -> KeyEvent {
    KeyEvent {
        key: Key::Z,
        pressed: true,
        modifiers: Modifiers {
            super_key: true,
            ..Modifiers::default()
        },
        text: String::new(),
    }
}

/// A 1000x800 shell with one focused window, the chooser open over it, and
/// `preset` selected.
fn chooser(preset: snap::SnapLayoutPreset) -> (DesktopShell, WindowId) {
    let mut shell = shell();
    let id = open(&mut shell, "Editor");
    assert!(shell.handle_hotkey(&zone_key()).consumed);
    assert!(shell.snap.is_overlay_visible(), "Super+Z did not open it");
    shell.snap.set_layout(preset);
    (shell, id)
}

/// Where the chooser drew each zone of the selected layout: id and centre.
fn drawn_zones(shell: &DesktopShell) -> Vec<(snap::ZoneId, f32, f32)> {
    shell
        .snap
        .layout()
        .zones
        .iter()
        .map(|zone| {
            let (cx, cy) = zone.center();
            (zone.id, cx, cy)
        })
        .collect()
}

fn motion(x: f32, y: f32) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind: MouseEventKind::Move,
    }
}

/// The whole point of the feature, end to end: a click on a drawn zone becomes
/// a request naming that zone, aimed at the focused window.
///
/// Every zone of every layout the picker offers, clicked at the centre of the
/// rectangle the overlay actually drew -- so a click can only pass by landing
/// where the user sees the zone, and the hit test and the renderer cannot drift
/// apart without this failing.
///
/// What it replaced: the shell used to *compute the snapped rectangle* and
/// return it to a caller with nothing to do with it. The shell moves no
/// windows, so the numbers were returned and dropped while the window stayed
/// where it was, and no assertion about the returned numbers could tell.
#[test]
fn clicking_a_zone_asks_for_the_focused_window_to_be_tiled_into_it() {
    for &preset in snap::SnapLayoutPreset::all() {
        let (probe, _) = chooser(preset);
        let zones = drawn_zones(&probe);
        assert_eq!(
            zones.len(),
            preset.zone_count() as usize,
            "{preset:?} drew a different number of zones than it claims"
        );

        for (zone_id, cx, cy) in zones {
            let (mut shell, id) = chooser(preset);
            let slot = snap::SnapSlot::new(preset, u8::try_from(zone_id).unwrap())
                .expect("every drawn zone must name a slot");
            assert_eq!(
                shell.handle_mouse(&click(cx, cy)),
                ShellAction::Control(ShellRequest::window(
                    id,
                    ShellControlAction::SnapToZone(slot)
                )),
                "clicking zone {zone_id} of {preset:?} at ({cx}, {cy})"
            );
            assert!(
                !shell.snap.is_overlay_visible(),
                "the chooser stayed up after the choice was made"
            );
        }
    }
}

/// The chooser places the window the user is looking at, not the one that was
/// focused when the layout was last changed.
#[test]
fn the_tiled_window_is_whichever_one_is_focused_now() {
    let (mut shell, first) = chooser(snap::SnapLayoutPreset::TwoEqualHalves);
    let second = open(&mut shell, "Terminal");
    assert_ne!(first, second);
    assert_eq!(shell.focused_window, Some(second));

    let (_, cx, cy) = drawn_zones(&shell)[0];
    match shell.handle_mouse(&click(cx, cy)) {
        ShellAction::Control(ShellRequest::Window(request)) => {
            assert_eq!(request.window, second);
        }
        other => panic!("expected a request for the focused window, got {other:?}"),
    }
}

/// With nothing focused there is nothing to place, so the chord does not put a
/// chooser on screen whose every zone would decline the click. It still claims
/// the key: a shortcut that sometimes types a `z` is worse than one that
/// sometimes does nothing.
#[test]
fn the_chooser_does_not_open_over_an_empty_desktop() {
    let mut shell = shell();
    assert_eq!(shell.focused_window, None);
    assert!(shell.handle_hotkey(&zone_key()).consumed);
    assert!(!shell.snap.is_overlay_visible());
    assert!(shell.render_zone_overlay().is_none());
}

/// Super+Z is a toggle, Escape closes the chooser like every other popup, and a
/// press on the scrim abandons the choice without asking for anything.
#[test]
fn the_chooser_closes_the_ways_a_popup_closes() {
    let (mut shell, _) = chooser(snap::SnapLayoutPreset::FourQuadrants);
    assert!(shell.handle_hotkey(&zone_key()).consumed);
    assert!(!shell.snap.is_overlay_visible(), "Super+Z did not toggle");

    let (mut shell, _) = chooser(snap::SnapLayoutPreset::FourQuadrants);
    let escape = KeyEvent {
        key: Key::Escape,
        pressed: true,
        modifiers: Modifiers::default(),
        text: String::new(),
    };
    assert!(shell.handle_hotkey(&escape).consumed);
    assert!(!shell.snap.is_overlay_visible(), "Escape did not close it");

    // The gutter between two zones is the overlay's own space: it is inside the
    // work area and inside no zone, which is exactly what `SnapOverlay` means.
    let (mut shell, _) = chooser(snap::SnapLayoutPreset::FourQuadrants);
    let area = shell.snap.work_area();
    let (x, y) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    assert_eq!(shell.hit_test(x, y), Hit::SnapOverlay);
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert!(!shell.snap.is_overlay_visible());
}

/// The chooser is drawn only while it is open, and what it draws follows the
/// layout the picker has selected.
#[test]
fn the_chooser_draws_the_layout_it_will_place_into() {
    let mut shell = shell();
    open(&mut shell, "Editor");
    assert!(shell.render_zone_overlay().is_none(), "drawn while closed");

    assert!(shell.handle_hotkey(&zone_key()).consumed);
    shell
        .snap
        .set_layout(snap::SnapLayoutPreset::TwoEqualHalves);
    let halves = shell.render_zone_overlay().expect("open, so drawn");
    shell.snap.set_layout(snap::SnapLayoutPreset::SixGrid);
    let grid = shell.render_zone_overlay().expect("still open");
    assert!(
        grid.commands.len() > halves.commands.len(),
        "six zones did not draw more than two"
    );
}

/// Hovering a zone lights it, and what is lit is what a press would place: the
/// highlight and the hit test are one answer, not two.
#[test]
fn the_lit_zone_is_the_one_a_press_would_place_into() {
    const PRESET: snap::SnapLayoutPreset = snap::SnapLayoutPreset::ThreeColumns;

    let (unlit, _) = chooser(PRESET);
    assert_eq!(unlit.snap.hovered_zone(), None);
    let plain = unlit
        .render_zone_overlay()
        .expect("open, so drawn")
        .commands
        .len();

    for (zone_id, cx, cy) in drawn_zones(&unlit) {
        let (mut shell, id) = chooser(PRESET);
        assert_eq!(
            shell.handle_mouse(&motion(cx, cy)),
            ShellAction::Consumed,
            "motion over the chooser reached a window"
        );
        assert_eq!(shell.snap.hovered_zone(), Some(zone_id));
        assert!(
            shell.render_zone_overlay().expect("open").commands.len() > plain,
            "the hovered zone drew no highlight"
        );

        let slot = snap::SnapSlot::new(PRESET, u8::try_from(zone_id).unwrap()).unwrap();
        assert_eq!(
            shell.handle_mouse(&click(cx, cy)),
            ShellAction::Control(ShellRequest::window(
                id,
                ShellControlAction::SnapToZone(slot)
            )),
            "the zone that was lit is not the zone the press placed into"
        );
    }
}

/// The layout picker rises from the top band of the *work area* and is clicked
/// against the same rectangle it is drawn from -- so every thumbnail selects
/// the preset it shows, and a press on the panel never places a window.
#[test]
fn each_thumbnail_selects_the_layout_it_pictures() {
    let summon = |shell: &DesktopShell| {
        let area = shell.snap.work_area();
        motion(area.x + area.width / 2.0, area.y + 1.0)
    };

    let (mut shell, _) = chooser(snap::SnapLayoutPreset::TwoEqualHalves);
    assert!(!shell.snap.is_picker_visible(), "up before it was summoned");
    let band = summon(&shell);
    shell.handle_mouse(&band);
    assert!(
        shell.snap.is_picker_visible(),
        "the top band did not summon the picker"
    );
    let (px, py, w, h) = shell.snap.picker_rect();
    assert_eq!(
        shell.hit_test(px + w / 2.0, py + h / 2.0),
        Hit::SnapPicker,
        "the panel does not claim its own middle"
    );

    for &preset in snap::SnapLayoutPreset::all() {
        let (mut shell, _) = chooser(snap::SnapLayoutPreset::TwoEqualHalves);
        let band = summon(&shell);
        shell.handle_mouse(&band);
        let (tx, ty, size) = shell
            .snap
            .thumbnail_rect(preset)
            .expect("every preset has a thumbnail");
        assert_eq!(
            shell.handle_mouse(&click(tx + size / 2.0, ty + size / 2.0)),
            ShellAction::Consumed,
            "a press on the picker asked the compositor for something"
        );
        assert_eq!(
            shell.snap.active_preset(),
            preset,
            "the thumbnail selected a different layout than it pictures"
        );
        assert!(
            shell.snap.is_overlay_visible(),
            "choosing a layout closed the chooser it was chosen in"
        );
    }
}

/// The zones follow the taskbar. A chooser measured against the whole screen
/// would draw its bottom row under the bar, and place a window there.
#[test]
fn the_chooser_tiles_the_work_area_and_not_the_screen() {
    let (mut shell, _) = chooser(snap::SnapLayoutPreset::SixGrid);
    let bar = shell.taskbar_rect();
    assert_eq!(shell.snap.work_area().bottom(), bar.y);
    for zone in &shell.snap.layout().zones {
        assert!(
            zone.y + zone.height <= bar.y,
            "zone {} runs under the taskbar",
            zone.id
        );
    }

    // And they follow it when it moves: a taller bar is a shorter work area,
    // re-derived at the next gesture rather than cached from the last one.
    shell.taskbar_height = 120;
    let taller = shell.taskbar_rect();
    shell.handle_mouse(&motion(10.0, 10.0));
    assert_eq!(shell.snap.work_area().bottom(), taller.y);
    for zone in &shell.snap.layout().zones {
        assert!(zone.y + zone.height <= taller.y);
    }
}

// ---- the order of the running programs' buttons ----------------------------------

/// The window `id` is raised and focused: moved to the top of the next list,
/// the others unfocused -- what a click on it, or Alt+Tab to it, produces.
fn raise(shell: &mut DesktopShell, id: WindowId) {
    let mut list = as_list(shell);
    let at = list
        .iter()
        .position(|info| info.id == id.0)
        .expect("raising a window the shell does not hold");
    let mut raised = list.remove(at);
    for other in &mut list {
        other.focused = false;
    }
    raised.focused = true;
    list.push(raised);
    shell.apply_window_list(&here(&list));
}

/// The windows the taskbar's buttons stand for, left to right.
fn buttons(shell: &DesktopShell) -> Vec<WindowId> {
    shell
        .taskbar_slots()
        .into_iter()
        .filter_map(|slot| match slot {
            crate::TaskbarSlot::Window(id) => Some(id),
            crate::TaskbarSlot::Pinned(_) => None,
        })
        .collect()
}

/// The middle of the button standing for `id`.
fn button_of(shell: &DesktopShell, id: WindowId) -> (f32, f32) {
    let slot = shell
        .taskbar_slots()
        .iter()
        .position(|slot| *slot == crate::TaskbarSlot::Window(id))
        .expect("the window has no button");
    centre(shell.taskbar_button_rect(slot))
}

fn release(shell: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
    shell.handle_mouse(&MouseEvent {
        x: at.0,
        y: at.1,
        kind: MouseEventKind::Release(MouseButton::Left),
    })
}

fn move_to(shell: &mut DesktopShell, at: (f32, f32)) {
    shell.handle_mouse(&MouseEvent {
        x: at.0,
        y: at.1,
        kind: MouseEventKind::Move,
    });
}

/// **A button stays where it is when its window is raised.** The bar used to
/// be drawn in stacking order, so every click moved the clicked button to the
/// end -- no button was ever where the user had last seen it.
#[test]
fn a_raised_window_keeps_its_button_where_it_was() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let c = open(&mut shell, "C");
    assert_eq!(buttons(&shell), [a, b, c]);

    raise(&mut shell, a);
    assert_eq!(buttons(&shell), [a, b, c], "raising A moved its button");
    // The stacking order did change, and Alt+Tab still follows it.
    let stacked: Vec<WindowId> = shell.taskbar_windows().iter().map(|w| w.id).collect();
    assert_eq!(stacked, [b, c, a]);
}

/// A window that goes takes its button; one that arrives joins the end.
#[test]
fn a_closed_window_leaves_the_row_and_a_new_one_joins_its_end() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let c = open(&mut shell, "C");
    raise(&mut shell, a);

    let without_b: Vec<WindowInfo> = as_list(&shell)
        .into_iter()
        .filter(|info| info.id != b.0)
        .collect();
    shell.apply_window_list(&here(&without_b));
    assert_eq!(buttons(&shell), [a, c]);

    let d = open(&mut shell, "D");
    assert_eq!(buttons(&shell), [a, c, d]);
}

/// A click acts on the release now -- so that a press can become a drag --
/// and is the same toggle it was: summon the window behind, put away the one
/// in front.
#[test]
fn a_window_button_acts_on_the_release() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");

    let at = button_of(&shell, a);
    assert_eq!(
        shell.handle_mouse(&click(at.0, at.1)),
        ShellAction::Consumed
    );
    assert_eq!(
        release(&mut shell, at),
        ShellAction::Control(ShellRequest::window(a, ShellControlAction::Activate))
    );

    let at = button_of(&shell, b);
    shell.handle_mouse(&click(at.0, at.1));
    assert_eq!(
        release(&mut shell, at),
        ShellAction::Control(ShellRequest::window(b, ShellControlAction::Minimize))
    );
}

/// Pressing the bar can move the focus before the release arrives. The toggle
/// is decided by what was in front at the press, so a click on the front
/// window's button still puts it away.
#[test]
fn what_was_in_front_at_the_press_decides_the_toggle() {
    let mut shell = shell();
    open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let at = button_of(&shell, b);
    shell.handle_mouse(&click(at.0, at.1));

    // The panel took the focus: no application window holds it now.
    let mut list = as_list(&shell);
    for info in &mut list {
        info.focused = false;
    }
    shell.apply_window_list(&here(&list));
    assert_eq!(shell.focused_window, None);

    assert_eq!(
        release(&mut shell, at),
        ShellAction::Control(ShellRequest::window(b, ShellControlAction::Minimize))
    );
}

/// A window that closes while its button is held asks for nothing.
#[test]
fn a_window_closed_while_its_button_is_held_asks_for_nothing() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let at = button_of(&shell, a);
    shell.handle_mouse(&click(at.0, at.1));
    shell.apply_window_list(&here(&[]));
    assert_eq!(release(&mut shell, at), ShellAction::Consumed);
}

/// **Dragging a window's button moves it along the row**, and the release
/// that ends the drag summons nothing.
#[test]
fn dragging_a_window_button_moves_it_along_the_row() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let c = open(&mut shell, "C");

    let from = button_of(&shell, a);
    let last = shell.taskbar_button_rect(2);
    let to = (last.x + last.w * 0.75, from.1);
    shell.handle_mouse(&click(from.0, from.1));
    // Past its own middle, short of its neighbour's: nothing moves yet.
    move_to(&mut shell, (from.0 + 20.0, from.1));
    assert_eq!(
        buttons(&shell),
        [a, b, c],
        "a nudge swapped it with its neighbour"
    );
    move_to(&mut shell, to);
    assert_eq!(
        buttons(&shell),
        [b, c, a],
        "the row did not follow the drag"
    );
    assert_eq!(release(&mut shell, to), ShellAction::Consumed);
    assert_eq!(buttons(&shell), [b, c, a]);

    // And the new order holds when the stacking changes.
    raise(&mut shell, b);
    assert_eq!(buttons(&shell), [b, c, a]);
}

/// Carried up off the bar, a window's button stays where it last was: the
/// row rearranges only under a pointer that is on it.
#[test]
fn a_window_button_carried_off_the_bar_stays_put() {
    let mut shell = shell();
    let a = open(&mut shell, "A");
    let b = open(&mut shell, "B");
    let c = open(&mut shell, "C");

    let from = button_of(&shell, a);
    let last = shell.taskbar_button_rect(2);
    let above_the_last = (last.x + last.w * 0.75, 500.0);
    shell.handle_mouse(&click(from.0, from.1));
    move_to(&mut shell, (from.0, 500.0));
    move_to(&mut shell, above_the_last);
    assert_eq!(
        buttons(&shell),
        [a, b, c],
        "the row followed a pointer off the bar"
    );
    assert_eq!(release(&mut shell, above_the_last), ShellAction::Consumed);
    assert_eq!(buttons(&shell), [a, b, c]);
}

/// A window's button stays among the window buttons: dragged over the pins
/// it goes to the front of its own row, and the pins do not move.
#[test]
fn a_window_button_cannot_be_dragged_among_the_pins() {
    appearance::config::testing::with_scratch_config("window-button-not-among-pins", |_root| {
        let mut shell = shell();
        let entry = shell.start_menu_entries()[0];
        let (exec, name) = (entry.executable_path.clone(), entry.name.clone());
        shell.pin_app(&exec, &name);
        let a = open(&mut shell, "A");
        let b = open(&mut shell, "B");

        let from = button_of(&shell, b);
        let pin = shell.taskbar_button_rect(0);
        let to = (pin.x + 2.0, from.1);
        shell.handle_mouse(&click(from.0, from.1));
        move_to(&mut shell, to);
        release(&mut shell, to);

        assert_eq!(buttons(&shell), [b, a]);
        assert_eq!(shell.pinned_apps().len(), 1);
        assert_eq!(shell.taskbar_slots()[0], crate::TaskbarSlot::Pinned(0));
    });
}

/// A window on another desktop keeps its place in the order while the user
/// rearranges this desktop's buttons around it.
#[test]
fn a_window_on_another_desktop_keeps_its_place() {
    let mut shell = shell();
    shell.num_desktops = 2;
    let mut away = app(2, "X");
    away.workspace = 1;
    shell.apply_window_list(&here(&[app(1, "A"), away, app(3, "B")]));
    let (a, x, b) = (WindowId(1), WindowId(2), WindowId(3));
    assert_eq!(buttons(&shell), [a, b]);

    // B dragged in front of A on this desktop.
    let from = button_of(&shell, b);
    let first = shell.taskbar_button_rect(0);
    let to = (first.x + 2.0, from.1);
    shell.handle_mouse(&click(from.0, from.1));
    move_to(&mut shell, to);
    release(&mut shell, to);
    assert_eq!(buttons(&shell), [b, a]);

    // X comes to this desktop, and stands where it stood: between them.
    let mut here_now = app(2, "X");
    here_now.workspace = 0;
    shell.apply_window_list(&here(&[app(1, "A"), here_now, app(3, "B")]));
    assert_eq!(buttons(&shell), [b, x, a]);
}

/// A pin nudged past its own middle stays where it is: a button moves only
/// when the pointer crosses a *neighbour's* middle. The rule used to count the
/// dragged button's own middle, so a drag to the right swapped it with its
/// neighbour a few pixels in -- and a drag to the left did not, which is how
/// it went unnoticed.
#[test]
fn a_pin_nudged_past_its_own_middle_stays_put() {
    appearance::config::testing::with_scratch_config("pin-nudge", |_root| {
        let mut shell = shell();
        let apps: Vec<String> = shell
            .start_menu_entries()
            .iter()
            .take(3)
            .map(|entry| entry.executable_path.clone())
            .collect();
        for exec in &apps {
            shell.pin_app(exec, exec);
        }
        let order = |shell: &DesktopShell| -> Vec<String> {
            shell
                .pinned_apps()
                .iter()
                .map(|app| app.exec_path.clone())
                .collect()
        };

        let first = shell.taskbar_button_rect(0);
        let y = first.y + first.h / 2.0;
        let (pressed, nudged) = (first.x + first.w * 0.25, first.x + first.w * 0.75);
        shell.handle_mouse(&click(pressed, y));
        move_to(&mut shell, (nudged, y));
        assert_eq!(order(&shell), apps, "a nudge swapped it with its neighbour");
        release(&mut shell, (nudged, y));
        assert_eq!(order(&shell), apps);

        // Past the neighbour's middle, it does move.
        let second = shell.taskbar_button_rect(1);
        let past = second.x + second.w * 0.75;
        shell.handle_mouse(&click(pressed, y));
        move_to(&mut shell, (past, y));
        release(&mut shell, (past, y));
        assert_eq!(
            order(&shell),
            [apps[1].clone(), apps[0].clone(), apps[2].clone()]
        );
    });
}

// ---- the space and the divider between the two sections --------------------------

/// A shell with one pinned program and `windows` windows open.
fn pinned_and_open(windows: usize) -> DesktopShell {
    let mut shell = shell();
    let entry = shell.start_menu_entries()[0];
    let (exec, name) = (entry.executable_path.clone(), entry.name.clone());
    shell.pin_app(&exec, &name);
    for n in 0..windows {
        open(&mut shell, &format!("window {n}"));
    }
    shell
}

/// `design.txt`: pinned programs on the left, running ones to their right,
/// "with a small space and a divider between the two sections". The gap
/// between the sections is wider than the gap between two buttons, and the
/// divider stands in its middle, drawn.
#[test]
fn a_space_and_a_divider_set_the_pins_apart_from_the_windows() {
    appearance::config::testing::with_scratch_config("taskbar-divider", |_root| {
        let shell = pinned_and_open(2);
        let (pin, first, second) = (
            shell.taskbar_button_rect(0),
            shell.taskbar_button_rect(1),
            shell.taskbar_button_rect(2),
        );
        let between_sections = first.x - (pin.x + pin.w);
        let between_buttons = second.x - (first.x + first.w);
        assert!(
            between_sections > between_buttons,
            "the sections are {between_sections}px apart, the buttons {between_buttons}px"
        );

        let divider = shell.taskbar_divider_rect().expect("no divider");
        assert!(divider.x >= pin.x + pin.w && divider.x + divider.w <= first.x);
        let (dx, dy) = centre(divider);
        assert_eq!(
            shell.hit_test(dx, dy),
            Hit::TaskbarPanel,
            "the divider is not a button"
        );
        let drawn = shell.render_taskbar().commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::FillRect { x, y, width, height, .. }
                if (*x, *y, *width, *height) == (divider.x, divider.y, divider.w, divider.h))
        });
        assert!(drawn, "the divider is not drawn");
    });
}

/// A divider with nothing on one side divides nothing: no pins, or no
/// windows, and the buttons are evenly spaced with no line.
#[test]
fn one_section_alone_has_no_divider() {
    appearance::config::testing::with_scratch_config("taskbar-no-divider", |_root| {
        let only_pinned = pinned_and_open(0);
        assert_eq!(only_pinned.taskbar_divider_rect(), None);

        let mut only_windows = shell();
        open(&mut only_windows, "A");
        open(&mut only_windows, "B");
        assert_eq!(only_windows.taskbar_divider_rect(), None);
        let (a, b) = (
            only_windows.taskbar_button_rect(0),
            only_windows.taskbar_button_rect(1),
        );
        assert!((b.x - (a.x + a.w) - only_windows.scale(crate::TASKBAR_BUTTON_GAP)).abs() < 0.01);
    });
}

/// The extra space comes out of the buttons' share, not the tray's: a full
/// bar still stops short of the tray.
#[test]
fn a_full_bar_with_a_divider_still_stops_short_of_the_tray() {
    appearance::config::testing::with_scratch_config("taskbar-divider-full", |_root| {
        let shell = pinned_and_open(30);
        let last = shell.taskbar_button_rect(shell.taskbar_slots().len() - 1);
        assert!(
            last.x + last.w <= shell.tray_x(),
            "the last button ends at {}, past the tray at {}",
            last.x + last.w,
            shell.tray_x()
        );
    });
}

// ---- the start menu's places column ------------------------------------------------

/// `design.txt` line 721: the start menu contains a "settings icon" and a
/// "terminal"; the Aero reference's places column lists the user's folders
/// above them. Each place starts what it names and closes the menu: the
/// file manager on a folder, or the program.
#[test]
fn the_start_menus_places_open_folders_and_start_settings_and_the_terminal() {
    for which in crate::StartShortcut::ALL {
        let mut shell = shell();
        shell.toggle_start_menu();
        let rect = shell.start_shortcut_rect(*which);
        let (x, y) = centre(rect);
        assert_eq!(shell.hit_test(x, y), Hit::StartMenuShortcut(*which));
        let ShellAction::Launch(launch) = click_at(&mut shell, rect) else {
            panic!("{which:?} started nothing");
        };
        assert_eq!(
            launch.program,
            std::path::PathBuf::from(which.program()),
            "{which:?}"
        );
        // Whatever the home is here -- the variable is the process's -- a
        // folder opens *its* folder, and a program is started with nothing.
        match which.folder() {
            Some("") | None => assert!(launch.args.len() <= 1, "{which:?}: {launch:?}"),
            Some(sub) => assert!(
                launch.args.is_empty() || std::path::Path::new(&launch.args[0]).ends_with(sub),
                "{which:?} opened {launch:?}"
            ),
        }
        assert!(!shell.start_menu_open, "{which:?} left the menu open");
    }
    assert_eq!(
        crate::StartShortcut::Settings.program(),
        "/usr/bin/settings"
    );
    assert_eq!(
        crate::StartShortcut::Terminal.program(),
        "/usr/bin/terminal"
    );
}

/// A folder is found under the home it is given; with no home, the file
/// manager opens where it opens by itself.
#[test]
fn a_place_opens_its_folder_under_the_home() {
    use crate::StartShortcut as Place;
    use std::ffi::OsString;
    let home = std::path::Path::new("/home/ann");
    let arg = |place: Place| place.launch(Some(home)).args;
    assert_eq!(arg(Place::Home), [OsString::from("/home/ann")]);
    assert_eq!(
        arg(Place::Documents),
        [home.join("Documents").into_os_string()]
    );
    assert_eq!(
        arg(Place::Downloads),
        [home.join("Downloads").into_os_string()]
    );
    assert!(arg(Place::Settings).is_empty());
    assert!(Place::Pictures.launch(None).args.is_empty());
    assert_eq!(
        Place::Pictures.launch(None).program,
        std::path::PathBuf::from(crate::launcher::FILE_MANAGER)
    );
}

/// Every place fits the places column at every scale -- below the user,
/// above the power button, one under another -- and is drawn where it is hit.
#[test]
fn the_places_fit_their_column_at_every_scale() {
    for percent in [100, 150, 200] {
        let mut shell = scaled(percent);
        shell.toggle_start_menu();
        let column = shell.start_menu_right_rect();
        let user = shell.start_user_rect();
        let power = shell.power_button_rect();
        let mut below = user.y + user.h;
        for which in crate::StartShortcut::ALL {
            let rect = shell.start_shortcut_rect(*which);
            assert!(
                rect.w > 0.0 && rect.h > 0.0,
                "{which:?} is empty at {percent}%"
            );
            assert!(
                rect.x >= column.x && rect.x + rect.w <= column.x + column.w,
                "{which:?} runs out of its column at {percent}%"
            );
            assert!(
                rect.y >= below,
                "{which:?} overlaps the one above at {percent}%"
            );
            assert!(
                rect.y + rect.h <= power.y,
                "{which:?} reaches the power button at {percent}%"
            );
            below = rect.y + rect.h;
        }
        let drawn = format!("{:?}", shell.render_start_menu().expect("the menu is open"));
        for which in crate::StartShortcut::ALL {
            assert!(
                drawn.contains(&format!("\"{}\"", which.label())),
                "{which:?} is not drawn at {percent}%"
            );
        }
    }
}

/// The two columns keep to themselves at every scale: the rows and the
/// search field in the programs column, the user, the places and the power
/// button in the places column, and the two columns side by side filling the
/// menu. A row as wide as the menu would highlight across the places, and a
/// power button in the programs column would sit on the search field.
#[test]
fn the_programs_and_the_places_keep_to_their_columns() {
    let inside = |outer: Rect, inner: Rect| {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w + 0.01
            && inner.y + inner.h <= outer.y + outer.h + 0.01
    };
    for percent in [100, 150, 200] {
        let mut shell = scaled(percent);
        shell.toggle_start_menu();
        let menu = shell.start_menu_rect();
        let left = shell.start_menu_left_rect();
        let right = shell.start_menu_right_rect();
        assert_eq!(left.x, menu.x);
        assert!(
            (left.w + right.w - menu.w).abs() < 0.01,
            "the columns do not fill the menu"
        );
        assert!(
            (right.x - (left.x + left.w)).abs() < 0.01,
            "a gap between the columns"
        );
        for row in 0..shell.start_menu_visible_rows() {
            let rect = shell.start_menu_row_rect(row);
            assert!(
                inside(left, rect),
                "row {row} leaves the programs at {percent}%"
            );
        }
        let search = shell.start_search_rect();
        assert!(
            inside(left, search),
            "the search field leaves the programs at {percent}%"
        );
        let last = shell.start_menu_row_rect(shell.start_menu_visible_rows().saturating_sub(1));
        assert!(
            last.y + last.h <= search.y + 0.01,
            "a row covers the search at {percent}%"
        );
        assert!(
            inside(right, shell.start_user_rect()),
            "the user at {percent}%"
        );
        assert!(
            inside(right, shell.power_button_rect()),
            "the power button at {percent}%"
        );
    }
}

/// A menu clamped too short for every place leaves the lower ones out rather
/// than drawing them over the power button, where they would take its press.
#[test]
fn a_menu_too_short_for_every_place_leaves_the_lower_ones_out() {
    let mut shell = DesktopShell::new(1000, 360);
    shell.toggle_start_menu();
    let power = shell.power_button_rect();
    assert!(power.h > 0.0);
    let shown: Vec<_> = crate::StartShortcut::ALL
        .iter()
        .filter(|w| shell.start_shortcut_rect(**w).w > 0.0)
        .collect();
    assert!(
        shown.len() < crate::StartShortcut::ALL.len(),
        "the fixture is not short enough"
    );
    for which in crate::StartShortcut::ALL {
        let rect = shell.start_shortcut_rect(*which);
        assert!(rect.w == 0.0 || rect.y + rect.h <= power.y, "{which:?}");
    }
    let (x, y) = centre(power);
    assert_eq!(shell.hit_test(x, y), Hit::PowerButton);
}

/// Every place, and the power button, draws its icon to the left of its
/// words; the id each is drawn under names that icon, at that size, in the
/// colour of the words -- the same id every frame, and forgotten when the
/// appearance changes, when every icon is drawn again in new colours.
#[test]
fn every_place_and_the_power_button_draws_its_icon() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let tree = shell.render_start_menu().expect("open");
    let images: Vec<(f32, f32, f32, u64)> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Image {
                x,
                y,
                width,
                image_id,
                ..
            } => Some((*x, *y, *width, *image_id)),
            _ => None,
        })
        .collect();
    let text_x = |label: &str| {
        tree.commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { x, text, .. } if text == label => Some(*x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} is not drawn"))
    };
    let mut targets: Vec<(Rect, &str, &str)> = crate::StartShortcut::ALL
        .iter()
        .map(|w| (shell.start_shortcut_rect(*w), w.icon_name(), w.label()))
        .collect();
    targets.push((shell.power_button_rect(), "system-shutdown", "Power"));
    for (rect, name, label) in targets {
        let &(x, _, width, id) = images
            .iter()
            .find(|(x, y, ..)| rect.contains(*x + 1.0, *y + 1.0))
            .unwrap_or_else(|| panic!("{label} has no icon"));
        assert_ne!(
            id & crate::ICON_ID_TAG,
            0,
            "{label}'s icon is outside the icons' ids"
        );
        let request = shell.icon_request(id).expect("the request was not kept");
        assert_eq!(request.name, name, "{label}");
        assert_eq!(request.color, shell.theme.start_menu_fg, "{label}");
        assert_eq!(request.px as f32, width, "{label}");
        assert!(
            text_x(label) >= x + width,
            "{label}'s words overlap its icon"
        );
    }

    let again: Vec<u64> = shell
        .render_start_menu()
        .expect("open")
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Image { image_id, .. } => Some(*image_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        again,
        images.iter().map(|i| i.3).collect::<Vec<_>>(),
        "ids change between frames"
    );

    shell.set_appearance(AppearanceSettings::default());
    assert!(
        shell.icon_request(images[0].3).is_none(),
        "a request outlived the appearance it was drawn in"
    );
}

/// The places column says who is using the desktop, once somebody is known:
/// their name, and its first letter as their picture.
#[test]
fn the_places_column_names_the_user_once_known() {
    let mut shell = shell();
    shell.toggle_start_menu();
    let drawn = format!("{:?}", shell.render_start_menu().expect("open"));
    assert!(!drawn.contains("\"ann\""));
    shell.set_user_name("ann");
    let drawn = format!("{:?}", shell.render_start_menu().expect("open"));
    assert!(drawn.contains("\"ann\""), "the name is not drawn");
    assert!(drawn.contains("\"A\""), "the initial is not drawn");
    let user = shell.start_user_rect();
    let (x, y) = centre(user);
    assert_eq!(shell.hit_test(x, y), Hit::StartMenuPanel);
}

/// With the power menu open, a press on a place closes the power menu and
/// nothing else -- the same as a press anywhere else in the start menu.
#[test]
fn a_press_on_a_place_first_closes_the_power_menu() {
    let mut shell = shell();
    shell.toggle_start_menu();
    shell.toggle_power_menu();
    let rect = shell.start_shortcut_rect(crate::StartShortcut::Home);
    // Not under the power menu, which rises from the foot of the column.
    let (x, y) = centre(rect);
    assert!(
        !shell.power_menu_rect().contains(x, y),
        "the fixture is covered"
    );
    assert_eq!(click_at(&mut shell, rect), ShellAction::Consumed);
    assert!(!shell.power_menu_open);
    assert!(shell.start_menu_open, "the start menu closed as well");
}
