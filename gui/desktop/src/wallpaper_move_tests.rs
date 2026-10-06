//! Tests for moving the wallpaper on the desktop: "Move wallpaper" on the
//! desktop's menu, where there is room to move the picture, and then the
//! pointer, the wheel and the arrow keys choosing which part of it shows --
//! `design.txt`'s "let the user scroll the image up/down or right/left to
//! center it on the desktop how they want" -- kept with Enter, put back with
//! Escape.
//!
//! The session's tests hold the picture being drawn where the setting says;
//! these hold the shell's part.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use crate::{
    DesktopShell, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind, ShellAction,
    click,
};
use appearance::config::testing::with_scratch_config;
use guitk::menu::MenuItem;

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

fn pressed(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

/// A shell whose wallpaper overflows the screen by 1000 pixels across and
/// fits it down -- a panorama filling the screen.
fn with_a_panorama() -> DesktopShell {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.wallpaper_room = Some((-1000.0, 0.0));
    shell
}

/// The labels of the desktop menu opened on bare desktop.
fn desktop_menu_labels(shell: &mut DesktopShell) -> Vec<String> {
    shell.open_desktop_menu(800.0, 500.0);
    let labels = shell
        .desktop_menu
        .items()
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { label, .. } | MenuItem::Submenu { label, .. } => Some(label.clone()),
            MenuItem::Separator => None,
        })
        .collect();
    shell.dismiss_popups();
    labels
}

fn begin(shell: &mut DesktopShell) {
    assert_eq!(
        shell.activate_desktop_menu_item(DesktopShell::MENU_MOVE_WALLPAPER),
        ShellAction::Consumed
    );
    assert!(shell.moving_wallpaper());
}

/// **The desktop's menu offers to move the wallpaper only where there is
/// somewhere to move it**: a picture that overflows the screen, or leaves
/// room beside it -- not one that fills it exactly, or no picture.
#[test]
fn the_menu_offers_to_move_a_picture_with_room() {
    let mut shell = DesktopShell::new(1600, 1000);
    assert!(!desktop_menu_labels(&mut shell).contains(&"Move wallpaper".to_owned()));
    assert_eq!(
        shell.activate_desktop_menu_item(DesktopShell::MENU_MOVE_WALLPAPER),
        ShellAction::Pass,
        "a move began with nothing to move"
    );
    assert!(!shell.moving_wallpaper());

    shell.wallpaper_room = Some((0.0, 200.0));
    assert!(desktop_menu_labels(&mut shell).contains(&"Move wallpaper".to_owned()));
}

/// **Dragging the picture moves the part that shows**: dragged 200 pixels
/// right, a picture 1000 wider than the screen shows a fifth more of its
/// left; it stops at its edges, and an axis with no room does not move.
#[test]
fn dragging_moves_the_part_that_shows() {
    let mut shell = with_a_panorama();
    begin(&mut shell);
    assert_eq!(
        shell.handle_mouse(&click(500.0, 500.0)),
        ShellAction::Consumed
    );
    shell.handle_mouse(&at(700.0, 650.0, MouseEventKind::Move));
    let (x, y) = shell.appearance.wallpaper_position;
    assert!((x - 0.3).abs() < 1e-5, "{x}");
    assert_eq!(y, 0.5, "the axis with no room moved");

    // Past its left edge: it stops there.
    shell.handle_mouse(&at(5000.0, 500.0, MouseEventKind::Move));
    assert_eq!(shell.appearance.wallpaper_position.0, 0.0);
    shell.handle_mouse(&at(
        5000.0,
        500.0,
        MouseEventKind::Release(MouseButton::Left),
    ));

    // After the button is up, motion moves nothing.
    shell.handle_mouse(&at(0.0, 500.0, MouseEventKind::Move));
    assert_eq!(shell.appearance.wallpaper_position.0, 0.0);
}

/// **Escape puts it back, Enter keeps it** -- written to `appearance.yaml`,
/// so it is there after a login.
#[test]
fn escape_puts_it_back_and_enter_keeps_it() {
    with_scratch_config("wallpaper-move-keep", |_root| {
        let mut shell = with_a_panorama();
        begin(&mut shell);
        shell.handle_mouse(&click(500.0, 500.0));
        shell.handle_mouse(&at(400.0, 500.0, MouseEventKind::Move));
        assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
        assert!(!shell.moving_wallpaper());
        assert_eq!(shell.appearance.wallpaper_position, (0.5, 0.5));

        begin(&mut shell);
        shell.handle_mouse(&click(500.0, 500.0));
        shell.handle_mouse(&at(400.0, 500.0, MouseEventKind::Move));
        assert!(shell.handle_hotkey(&pressed(Key::Enter)).consumed);
        assert!(!shell.moving_wallpaper());
        let kept = shell.appearance.wallpaper_position;
        assert!((kept.0 - 0.6).abs() < 1e-5, "{kept:?}");
        let saved = appearance::AppearanceFile::load()
            .settings
            .wallpaper_position;
        assert!((saved.0 - 0.6).abs() < 1e-5, "not saved: {saved:?}");
    });
}

/// **The arrow keys and the wheel move it too**, and every other key goes
/// no further while it is being moved.
#[test]
fn the_keys_and_the_wheel_move_it() {
    let mut shell = with_a_panorama();
    begin(&mut shell);
    assert!(shell.handle_hotkey(&pressed(Key::Right)).consumed);
    // 24 pixels right of a 1000-pixel overflow.
    assert!((shell.appearance.wallpaper_position.0 - (0.5 - 0.024)).abs() < 1e-5);
    assert!(shell.handle_hotkey(&pressed(Key::Left)).consumed);
    assert!((shell.appearance.wallpaper_position.0 - 0.5).abs() < 1e-5);
    shell.handle_mouse(&at(
        800.0,
        500.0,
        MouseEventKind::Scroll { dx: 2.0, dy: 0.0 },
    ));
    assert!((shell.appearance.wallpaper_position.0 - (0.5 - 0.048)).abs() < 1e-5);
    // A letter is the move's, not the desktop's.
    assert!(shell.handle_hotkey(&pressed(Key::A)).consumed);
    assert!(shell.moving_wallpaper());
}

/// **A press on the taskbar keeps the picture where it is and ends the
/// move**, as does anything that dismisses the popups -- neither is the user
/// taking the move back.
#[test]
fn leaving_by_the_taskbar_or_another_popup_keeps_it() {
    with_scratch_config("wallpaper-move-leave", |_root| {
        let mut shell = with_a_panorama();
        begin(&mut shell);
        shell.handle_mouse(&click(500.0, 500.0));
        shell.handle_mouse(&at(300.0, 500.0, MouseEventKind::Move));
        let bar = shell.taskbar_rect();
        shell.handle_mouse(&click(bar.x + 200.0, bar.y + bar.h / 2.0));
        assert!(!shell.moving_wallpaper());
        assert!((shell.appearance.wallpaper_position.0 - 0.7).abs() < 1e-5);

        begin(&mut shell);
        shell.handle_mouse(&click(500.0, 500.0));
        shell.handle_mouse(&at(400.0, 500.0, MouseEventKind::Move));
        assert!(shell.dismiss_popups(), "the move did not count as open");
        assert!(!shell.moving_wallpaper());
        assert!((shell.appearance.wallpaper_position.0 - 0.8).abs() < 1e-5);
    });
}

/// **While it is moved the move owns the pointer and the keyboard, and the
/// screen says how to finish**: a right-click opens no menu, Escape is held
/// (the move counts as a popup), and the card is drawn until it ends.
#[test]
fn the_move_owns_the_pointer_and_says_how_to_finish() {
    let mut shell = with_a_panorama();
    assert!(shell.render_wallpaper_move().is_none());
    begin(&mut shell);
    assert!(shell.any_popup_open());
    let card = shell.render_wallpaper_move().expect("the card is drawn");
    let said: Vec<String> = card
        .commands
        .iter()
        .filter_map(|c| match c {
            guitk::render::RenderCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(said.iter().any(|t| t.contains("Esc")), "{said:?}");

    shell.handle_mouse(&at(800.0, 500.0, MouseEventKind::Press(MouseButton::Right)));
    assert!(
        !shell.desktop_menu.is_visible(),
        "a menu opened over the move"
    );
    assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
    assert!(shell.render_wallpaper_move().is_none());
    assert!(!shell.any_popup_open());
}
