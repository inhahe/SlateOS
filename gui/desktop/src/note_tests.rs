//! Tests for the desktop's notes as a user meets them: offered by the desktop
//! menu, opened by a click, written in from the keyboard, put down by Escape
//! or a click away, and still moved by the title bar.
//!
//! The widget layer's own tests hold the note's parts; these hold the shell's
//! routing -- which is where a press meant for a note could instead start a
//! drag, or a keystroke meant for it reach an icon.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::widgets::{WidgetInstanceId, WidgetKind};
use crate::{
    DesktopShell, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind, ShellAction,
    click,
};

fn shell() -> DesktopShell {
    DesktopShell::new(1600, 1000)
}

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

fn release(x: f32, y: f32) -> MouseEvent {
    at(x, y, MouseEventKind::Release(MouseButton::Left))
}

fn typed(text: &str) -> KeyEvent {
    KeyEvent {
        key: Key::A,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: text.to_string(),
    }
}

fn pressed(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

/// Add a note the way a user does -- the desktop menu's "Add widget" -- and
/// answer it and the middle of its writing area.
fn add_note(shell: &mut DesktopShell) -> (WidgetInstanceId, (f32, f32)) {
    shell.open_desktop_menu(800.0, 500.0);
    let action = shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_NOTE);
    assert!(action.changed(), "the menu added nothing");
    // Chosen by id rather than by a click on the row, so the menu is still up;
    // a click on the row would have closed it.
    shell.dismiss_popups();
    let id = shell
        .widgets
        .all_widgets()
        .iter()
        .find(|w| matches!(w.kind, WidgetKind::Notes))
        .map(|w| w.id)
        .expect("a note on the desktop");
    let (x, y, w, h) = shell.widgets.content_rect(id).expect("placed");
    (id, (x + w / 2.0, y + h / 2.0))
}

/// Open the note with a click on its writing area.
fn open(shell: &mut DesktopShell, (x, y): (f32, f32)) {
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert_eq!(shell.handle_mouse(&release(x, y)), ShellAction::Consumed);
}

/// The desktop menu offers a note -- the widget kind that was defined, drawn
/// and never offered.
#[test]
fn the_desktop_menu_offers_a_note() {
    let mut shell = shell();
    let (id, _) = add_note(&mut shell);
    assert!(shell.widgets.get(id).is_some());
    assert!(shell.take_widgets_dirty(), "a new note is a layout to save");
}

/// A click opens the note, what is typed goes into it -- not to the icons --
/// and every change marks the layout for saving.
#[test]
fn a_click_opens_a_note_and_typing_writes_in_it() {
    let mut shell = shell();
    let (id, body) = add_note(&mut shell);
    let _ = shell.take_widgets_dirty();
    open(&mut shell, body);
    assert_eq!(shell.widgets.writing_note(), Some(id));

    assert_eq!(shell.handle_desktop_key(&typed("m")), ShellAction::Consumed);
    assert_eq!(
        shell.handle_desktop_key(&pressed(Key::Enter)),
        ShellAction::Consumed
    );
    assert_eq!(shell.handle_desktop_key(&typed("e")), ShellAction::Consumed);
    assert_eq!(shell.widgets.get(id).expect("placed").state_text, "m\ne");
    assert!(shell.take_widgets_dirty(), "typing in a note is saved");

    // A Delete is the note's too: it must not reach an icon. A shortcut the
    // user added is the kind Delete takes off the desktop, so one is placed and
    // selected here to be the thing that would be lost.
    let (shortcut, added) = shell.icons.add_shortcut_at(
        "Editor",
        crate::icons::IconType::Executable,
        crate::icons::IconAction::OpenPath(std::path::PathBuf::from("/usr/bin/editor")),
        1_400.0,
        100.0,
    );
    assert!(added);
    shell.icons.select_single(shortcut);
    assert_eq!(
        shell.handle_desktop_key(&pressed(Key::Delete)),
        ShellAction::Consumed
    );
    assert!(
        shell.icons.icon_ids().contains(&shortcut),
        "the Delete reached the icon"
    );
}

/// Escape puts the note down, keeping what was written, and the keys go
/// back to the desktop.
#[test]
fn escape_puts_the_note_down() {
    let mut shell = shell();
    let (id, body) = add_note(&mut shell);
    open(&mut shell, body);
    shell.handle_desktop_key(&typed("x"));
    assert_eq!(
        shell.handle_desktop_key(&pressed(Key::Escape)),
        ShellAction::Consumed
    );
    assert_eq!(shell.widgets.writing_note(), None);
    assert_eq!(shell.widgets.get(id).expect("placed").state_text, "x");
    let _ = shell.handle_desktop_key(&typed("y"));
    assert_eq!(shell.widgets.get(id).expect("placed").state_text, "x");
}

/// A click anywhere else puts the note down too, as a click away does on
/// every desktop.
#[test]
fn a_click_away_puts_the_note_down() {
    let mut shell = shell();
    let (_, body) = add_note(&mut shell);
    open(&mut shell, body);
    let (far_x, far_y) = (1500.0, 900.0);
    assert!(shell.widgets.note_body_at(far_x, far_y).is_none());
    let _ = shell.handle_mouse(&click(far_x, far_y));
    let _ = shell.handle_mouse(&release(far_x, far_y));
    assert_eq!(shell.widgets.writing_note(), None);
}

/// The title bar still takes hold of the note to move it, and does not open
/// it for writing.
#[test]
fn the_title_bar_still_moves_a_note() {
    let mut shell = shell();
    let (id, _) = add_note(&mut shell);
    let (x, y, w, _) = shell.widgets.content_rect(id).expect("placed");
    // Above the writing area is the title bar.
    let title = (x + w / 2.0, y - 12.0);
    assert!(shell.widgets.note_body_at(title.0, title.1).is_none());
    assert_eq!(
        shell.handle_mouse(&click(title.0, title.1)),
        ShellAction::Consumed
    );
    assert!(
        shell.widget_drag.is_some(),
        "a press on the title bar did not take hold"
    );
    assert_eq!(shell.widgets.writing_note(), None);
}

/// Dragging across the writing area with the button down selects, rather
/// than moving the note.
#[test]
fn a_drag_in_a_note_selects_rather_than_moving_it() {
    let mut shell = shell();
    let (id, (bx, by)) = add_note(&mut shell);
    open(&mut shell, (bx, by));
    for ch in "some words".chars() {
        shell.handle_desktop_key(&typed(&ch.to_string()));
    }
    let before = shell.widgets.get(id).expect("placed").position;
    let (x, y, _, _) = shell.widgets.content_rect(id).expect("placed");
    assert_eq!(
        shell.handle_mouse(&click(x, y + 2.0)),
        ShellAction::Consumed
    );
    assert_eq!(
        shell.handle_mouse(&at(x + 40.0, y + 2.0, MouseEventKind::Move)),
        ShellAction::Consumed
    );
    assert_eq!(
        shell.handle_mouse(&release(x + 40.0, y + 2.0)),
        ShellAction::Consumed
    );
    assert!(shell.widget_drag.is_none());
    assert_eq!(shell.widgets.get(id).expect("placed").position, before);
    // Typing now replaces the selection, which is how a selection is seen
    // from the keyboard.
    shell.handle_desktop_key(&typed("S"));
    let text = &shell.widgets.get(id).expect("placed").state_text;
    assert!(
        text.starts_with('S') && text.len() < "some words".len(),
        "{text:?}"
    );
}

/// The wheel over the open note is the note's.
#[test]
fn the_wheel_over_the_open_note_is_the_notes() {
    let mut shell = shell();
    let (_, body) = add_note(&mut shell);
    open(&mut shell, body);
    assert_eq!(
        shell.handle_mouse(&crate::scroll(body.0, body.1, -1.0)),
        ShellAction::Consumed
    );
}
