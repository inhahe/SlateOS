//! Tests for windows minimised to the system tray: a window rule's `tray`
//! action (`windowrules::RuleActions::to_tray`) sends a window, when it is
//! minimised, from the taskbar to an entry of the shell's own in the tray --
//! `design.txt`'s "User can override any app: always start in system tray,
//! always in taskbar, or neither".
//!
//! `windowrules`' tests hold that the action is read, written and merged;
//! these hold the shell's part: when a window has an entry, what the entry
//! shows, what a click on it does, and that it keeps its place and goes with
//! its window.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use crate::{
    DesktopShell, ManagedWindow, PinTarget, Rect, SHELL_TRAY_OWNER, ShellAction,
    ShellControlAction, ShellRequest, WindowId, WindowInfo, WindowList, launcher, tray_dnd,
    window_rules,
};
use guiremote::tray::TrayIcon;
use guitk::event::{MouseButton, MouseEvent, MouseEventKind};
use guitk::menu::MenuItem;
use guitk::render::RenderCommand;
use tray_dnd::TrayIconKey;

/// A shell with a rule sending `app`'s windows to the tray when they are
/// minimised, and `also` whatever else the test wants of the rule.
fn shell_with_rule(app: &str, also: impl Fn(&mut window_rules::RuleActions)) -> DesktopShell {
    let mut shell = DesktopShell::new(1920, 1080);
    let mut rule = window_rules::WindowRule::new(
        0,
        "to the tray",
        window_rules::MatchCriteria::AppId(app.to_owned()),
    );
    rule.actions.to_tray = Some(true);
    also(&mut rule.actions);
    assert!(shell.rules.add_rule(rule).is_some(), "the rule was refused");
    shell
}

/// A shell whose rule sends the player's windows to the tray.
fn shell() -> DesktopShell {
    shell_with_rule("player", |_| {})
}

/// A window of `app` titled `title`, on the first desktop.
fn window(id: u64, app: &str, title: &str, minimized: bool) -> WindowInfo {
    let mut info = WindowInfo::new(id, 40 + id, title);
    info.app_id = app.to_owned();
    info.minimized = minimized;
    info
}

/// The compositor's list: `windows`, the first desktop showing.
fn show(shell: &mut DesktopShell, windows: Vec<WindowInfo>) -> Vec<ShellRequest> {
    shell.apply_window_list(&WindowList::new(0, windows))
}

fn ids(windows: &[&ManagedWindow]) -> Vec<u64> {
    windows.iter().map(|w| w.id.0).collect()
}

/// The shell's own entries in the tray, as `(id, glyph, tooltip)`.
fn entries(shell: &DesktopShell) -> Vec<(u32, String, String)> {
    shell
        .tray_icons()
        .iter()
        .filter(|icon| icon.owner == SHELL_TRAY_OWNER)
        .map(|icon| (icon.id, icon.glyph.clone(), icon.tooltip.clone()))
        .collect()
}

/// The tray's icons as drawn, left to right, by their tooltips.
fn row(shell: &DesktopShell) -> Vec<String> {
    shell
        .ordered_tray_icons()
        .iter()
        .map(|icon| icon.tooltip.clone())
        .collect()
}

/// The key of the shell's entry for the window titled `title`.
fn entry_of(shell: &DesktopShell, title: &str) -> TrayIconKey {
    let (id, _, _) = entries(shell)
        .into_iter()
        .find(|(_, _, tooltip)| tooltip == title)
        .unwrap_or_else(|| panic!("no entry for {title:?}"));
    TrayIconKey {
        owner: SHELL_TRAY_OWNER,
        id,
    }
}

/// Press and let go of `button` in the middle of `rect`.
fn click(shell: &mut DesktopShell, rect: Rect, button: MouseButton) -> ShellAction {
    let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let pressed = shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(button),
    });
    assert_eq!(pressed, ShellAction::Consumed, "the press is the tray's");
    shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Release(button),
    })
}

fn activate(id: u64) -> ShellAction {
    ShellAction::Control(ShellRequest::window(
        WindowId(id),
        ShellControlAction::Activate,
    ))
}

// ─── Which windows ──────────────────────────────────────────────────────────

/// **A window minimised under the rule leaves the taskbar and the switcher
/// for the tray**, and comes back to them restored; a window the rule does
/// not name stays on the bar minimised.
#[test]
fn a_window_minimised_to_the_tray_leaves_the_taskbar_for_it() {
    let mut shell = shell();
    show(
        &mut shell,
        vec![
            window(1, "player", "Music", false),
            window(2, "editor", "notes.md", false),
        ],
    );
    assert!(
        entries(&shell).is_empty(),
        "in the tray before it was put away"
    );
    assert_eq!(ids(&shell.taskbar_windows()), [1, 2]);

    show(
        &mut shell,
        vec![
            window(1, "player", "Music", true),
            window(2, "editor", "notes.md", true),
        ],
    );
    assert_eq!(ids(&shell.taskbar_windows()), [2], "the editor has no rule");
    assert_eq!(ids(&shell.switcher_windows()), [2]);
    assert_eq!(entries(&shell), [(1, "M".to_owned(), "Music".to_owned())]);

    show(
        &mut shell,
        vec![
            window(1, "player", "Music", false),
            window(2, "editor", "notes.md", true),
        ],
    );
    assert!(
        entries(&shell).is_empty(),
        "restored, it is still in the tray"
    );
    assert_eq!(ids(&shell.taskbar_windows()), [1, 2]);
    // Most recently used first.
    assert_eq!(ids(&shell.switcher_windows()), [2, 1]);
}

/// **A rule can start a window in the tray**: `tray` with `state: minimized`
/// asks for the window to be minimised as it arrives, and minimised, it is
/// in the tray.
#[test]
fn a_rule_can_start_a_window_in_the_tray() {
    let mut shell = shell_with_rule("player", |a| {
        a.initial_state = Some(window_rules::InitialState::Minimized);
    });
    let asked = show(&mut shell, vec![window(1, "player", "Music", false)]);
    assert!(
        asked.contains(&ShellRequest::window(
            WindowId(1),
            ShellControlAction::Minimize
        )),
        "{asked:?}"
    );
    // Until the compositor has done it, the window is an open one.
    assert_eq!(ids(&shell.taskbar_windows()), [1]);
    assert!(entries(&shell).is_empty());

    show(&mut shell, vec![window(1, "player", "Music", true)]);
    assert!(shell.taskbar_windows().is_empty());
    assert_eq!(entries(&shell).len(), 1);
}

/// **The tray is every desktop's**, as a program's icon there is: a window
/// put away on another desktop is in it all the same.
#[test]
fn the_tray_holds_windows_from_every_desktop() {
    let mut shell = shell();
    let mut elsewhere = window(1, "player", "Music", true);
    elsewhere.workspace = 1;
    show(&mut shell, vec![elsewhere]);
    assert_eq!(entries(&shell).len(), 1);
}

/// **An entry is named by its window's title**, its glyph the title's first
/// letter or digit, capitalised -- or a square, for a title with none.
#[test]
fn an_entry_is_named_by_its_window() {
    let mut shell = shell();
    show(
        &mut shell,
        vec![
            window(1, "player", "élan vital", true),
            window(2, "player", "-- 3 songs --", true),
            window(3, "player", "...", true),
        ],
    );
    let glyphs: Vec<String> = entries(&shell).into_iter().map(|(_, g, _)| g).collect();
    assert_eq!(glyphs, ["É", "3", "\u{25A3}"]);
}

// ─── Ids and places ─────────────────────────────────────────────────────────

/// **Every entry has an id no other holds, and none is 0** -- not when the
/// count wraps, and not when it comes round to one still in use.
#[test]
fn every_entry_has_its_own_id() {
    let mut shell = shell();
    show(&mut shell, vec![window(1, "player", "One", true)]);
    assert_eq!(entries(&shell)[0].0, 1);

    // The count comes round to the id the first window still holds.
    shell.next_window_tray_id = 1;
    show(
        &mut shell,
        vec![
            window(1, "player", "One", true),
            window(2, "player", "Two", true),
        ],
    );
    let held: Vec<u32> = entries(&shell).into_iter().map(|(id, _, _)| id).collect();
    assert_eq!(held, [1, 2]);

    // And it wraps, past 0.
    shell.next_window_tray_id = u32::MAX;
    show(
        &mut shell,
        vec![
            window(1, "player", "One", true),
            window(2, "player", "Two", true),
            window(3, "player", "Three", true),
            window(4, "player", "Four", true),
        ],
    );
    let mut held: Vec<u32> = entries(&shell).into_iter().map(|(id, _, _)| id).collect();
    held.sort_unstable();
    assert_eq!(held, [1, 2, 3, u32::MAX]);
}

/// **An entry keeps its place while its window is in the tray**: a new
/// title relabels it where it is, a program's icon changing moves nothing,
/// and closing the window takes the entry and its id away.
#[test]
fn an_entry_keeps_its_place_and_goes_with_its_window() {
    let mut shell = shell();
    shell.apply_tray_icons(vec![TrayIcon::new(99, 1, "B", "Battery")]);
    show(
        &mut shell,
        vec![
            window(1, "player", "Music", true),
            window(2, "player", "Radio", true),
        ],
    );
    assert_eq!(row(&shell), ["Battery", "Music", "Radio"]);

    // The user puts the radio first.
    let radio = entry_of(&shell, "Radio");
    let battery = TrayIconKey { owner: 99, id: 1 };
    assert!(shell.tray_arrangement.move_before(radio, Some(battery)));
    assert_eq!(row(&shell), ["Radio", "Battery", "Music"]);

    show(
        &mut shell,
        vec![
            window(1, "player", "Music", true),
            window(2, "player", "Radio - Jazz", true),
        ],
    );
    assert_eq!(row(&shell), ["Radio - Jazz", "Battery", "Music"]);

    shell.apply_tray_icons(vec![TrayIcon::new(99, 1, "B", "Battery: 50%")]);
    assert_eq!(row(&shell), ["Radio - Jazz", "Battery: 50%", "Music"]);

    show(&mut shell, vec![window(1, "player", "Music", true)]);
    assert_eq!(row(&shell), ["Battery: 50%", "Music"]);
    assert_eq!(shell.window_tray_ids.len(), 1, "the closed window's id");
}

// ─── Clicks ─────────────────────────────────────────────────────────────────

/// **A click on a window's entry brings the window back** -- the shell's to
/// answer, so it asks for the window and tells no program -- while a click on
/// a program's icon beside it is still the program's to hear.
#[test]
fn a_click_on_an_entry_brings_its_window_back() {
    let mut shell = shell();
    shell.apply_tray_icons(vec![TrayIcon::new(99, 7, "B", "Battery")]);
    show(&mut shell, vec![window(1, "player", "Music", true)]);
    let rects = shell.tray_icon_rects();
    assert_eq!(row(&shell), ["Battery", "Music"]);

    assert_eq!(click(&mut shell, rects[1], MouseButton::Left), activate(1));
    assert_eq!(
        click(&mut shell, rects[0], MouseButton::Left),
        ShellAction::Control(ShellRequest::ClickTrayIcon {
            owner: 99,
            id: 7,
            button: MouseButton::Left,
        })
    );
}

/// **A right click on a window's entry opens the window's menu**, as on its
/// taskbar button; the middle button does nothing, as it does there.
#[test]
fn a_right_click_on_an_entry_opens_its_windows_menu() {
    let mut shell = shell();
    show(&mut shell, vec![window(1, "player", "Music", true)]);
    let rect = shell.tray_icon_rects()[0];

    assert_eq!(
        click(&mut shell, rect, MouseButton::Middle),
        ShellAction::Consumed
    );
    assert!(shell.pin_menu.is_none(), "the middle button opened a menu");

    assert_eq!(
        click(&mut shell, rect, MouseButton::Right),
        ShellAction::Consumed
    );
    assert!(
        matches!(shell.pin_menu, Some((_, PinTarget::Window(WindowId(1))))),
        "no menu for the window"
    );
}

/// **An entry whose window has gone does nothing when clicked**, though the
/// press began while it was there.
#[test]
fn a_click_on_an_entry_whose_window_went_does_nothing() {
    let mut shell = shell();
    show(&mut shell, vec![window(1, "player", "Music", true)]);
    let rect = shell.tray_icon_rects()[0];
    let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    });
    show(&mut shell, vec![]);
    let released = shell.handle_mouse(&MouseEvent {
        x,
        y,
        kind: MouseEventKind::Release(MouseButton::Left),
    });
    assert_eq!(released, ShellAction::Consumed);
}

/// **The overflow list names a window's entry by its title, shows its
/// program's picture, and brings the window back.**
#[test]
fn an_entry_in_the_overflow_list_brings_its_window_back() {
    let mut shell = shell();
    shell.apply_tray_icons((1..=80).map(|id| TrayIcon::new(99, id, "X", "x")).collect());
    show(&mut shell, vec![window(1, "player", "Music", true)]);
    let entry = entry_of(&shell, "Music");
    assert!(
        shell
            .overflowed_tray_icons()
            .iter()
            .any(|icon| TrayIconKey::of(icon) == entry),
        "the premise: the entry is past the bar's room"
    );

    shell.open_tray_overflow();
    let (menu, keys) = shell.tray_overflow_menu.as_ref().expect("the list opened");
    let at = keys.iter().position(|key| *key == entry).expect("listed");
    match &menu.items()[at] {
        MenuItem::Action { label, icon, .. } => {
            assert_eq!(label, "Music");
            assert_eq!(icon.as_deref(), Some(launcher::GENERIC_PROGRAM_ICON));
        }
        other => panic!("not a row: {other:?}"),
    }
    assert_eq!(
        shell.take_overflow_row(at as u64),
        Some(ShellRequest::window(
            WindowId(1),
            ShellControlAction::Activate
        ))
    );
    assert!(shell.tray_overflow_menu.is_none(), "the list stayed open");
}

// ─── Drawing ────────────────────────────────────────────────────────────────

/// **A window's entry shows its program's picture**, not its glyph.
#[test]
fn an_entry_is_drawn_as_its_programs_picture() {
    let mut shell = shell();
    show(&mut shell, vec![window(1, "player", "Music", true)]);
    let rect = shell.tray_icon_rects()[0];
    let tree = shell.render_taskbar();
    let pictures = tree
        .commands
        .iter()
        .filter(|c| match c {
            RenderCommand::Image {
                x,
                y,
                width,
                height,
                ..
            } => rect.contains(*x + *width / 2.0, *y + *height / 2.0),
            _ => false,
        })
        .count();
    assert_eq!(pictures, 1, "no picture in the entry's slot");
    assert!(
        !tree
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "M")),
        "the glyph was drawn as well"
    );
}
