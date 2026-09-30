//! Tests for choosing the folder a photo frame shows: "Choose folder…" on
//! its menu puts the shell's chooser up in its folder mode, and the folder
//! chosen there is the one the frame shows from then on.
//!
//! The widget layer's tests hold a frame's folder and how it steps through
//! it; the session's hold the pictures going up. These hold the shell's part
//! -- and, as the folder picker's first caller, that the picker works end to
//! end: opened on the right folder, a folder below opened rather than chosen,
//! an empty one chosen, Escape leaving everything as it was.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::widgets::{WidgetInstanceId, WidgetKind};
use crate::{DesktopShell, Key, KeyEvent, Modifiers, ShellAction};
use guitk::dialog::DirEntry;
use guitk::menu::MenuItem;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn key(k: Key) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    }
}

fn dir(name: &str) -> DirEntry {
    DirEntry {
        name: OsString::from(name),
        is_dir: true,
        size: 0,
        modified_timestamp: 0,
        extension: OsString::new(),
    }
}

/// A shell with a photo frame placed from the desktop menu, showing
/// `/pictures`, and the frame.
fn shell_with_a_frame() -> (DesktopShell, WidgetInstanceId) {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_PHOTO_FRAME);
    let frame = shell
        .widgets
        .all_widgets()
        .iter()
        .find(|w| matches!(w.kind, WidgetKind::PhotoFrame))
        .map(|w| w.id)
        .expect("the menu placed a frame");
    assert!(
        shell
            .widgets
            .set_frame_folder(frame, Path::new("/pictures"))
    );
    assert!(
        shell.take_widgets_dirty(),
        "placing a frame did not mark the layout for saving"
    );
    (shell, frame)
}

/// The labels of the desktop menu opened on the middle of widget `id`.
fn menu_on(shell: &mut DesktopShell, id: WidgetInstanceId) -> Vec<String> {
    let (x, y, w, h) = shell.widgets.content_rect(id).expect("placed");
    shell.open_desktop_menu(x + w / 2.0, y + h / 2.0);
    shell
        .desktop_menu
        .items()
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

/// **A photo frame's menu offers "Choose folder…", and choosing it puts the
/// chooser up at the frame's folder; a folder below opened -- not chosen --
/// and an empty one chosen is the frame's from then on, the layout marked
/// for saving.**
#[test]
fn a_frames_folder_is_chosen_in_the_shells_chooser() {
    let (mut shell, frame) = shell_with_a_frame();
    let labels = menu_on(&mut shell, frame);
    assert_eq!(labels[0], "Choose folder…", "{labels:?}");

    assert_eq!(
        shell.activate_desktop_menu_item(DesktopShell::MENU_FRAME_FOLDER),
        ShellAction::Consumed
    );
    shell.desktop_menu.hide();
    assert!(shell.chooser_open(), "no chooser came up");
    assert!(
        shell.any_popup_open(),
        "the chooser alone is not a popup, so Escape would not reach it"
    );
    assert!(
        !shell.take_widgets_dirty(),
        "asking for a folder changed the layout"
    );
    assert_eq!(shell.chooser_wants(), Some(Path::new("/pictures")));
    shell.set_chooser_entries(vec![dir("Trips"), dir("Work")]);

    // The first folder, highlighted and opened: opened, not chosen. Home
    // rather than Down, which would depend on where a listing leaves the
    // highlight.
    drop(shell.handle_hotkey(&key(Key::Home)));
    drop(shell.handle_hotkey(&key(Key::Enter)));
    assert!(shell.chooser_open(), "opening a folder chose it");
    assert_eq!(shell.chooser_wants(), Some(Path::new("/pictures/Trips")));
    // An empty folder, nothing highlighted: Enter chooses the folder shown.
    shell.set_chooser_entries(Vec::new());
    drop(shell.handle_hotkey(&key(Key::Enter)));

    assert!(!shell.chooser_open(), "the chooser stayed up");
    assert_eq!(
        shell.widgets.frame_folder(frame),
        Some(PathBuf::from("/pictures/Trips"))
    );
    assert!(shell.take_widgets_dirty(), "the folder chosen is not saved");
    // And the Run box, had it been open, was never touched.
    assert!(!shell.run_dialog.is_visible());
}

/// **Escape takes the chooser down and leaves the frame's folder as it
/// was.**
#[test]
fn escape_leaves_the_frames_folder_as_it_was() {
    let (mut shell, frame) = shell_with_a_frame();
    menu_on(&mut shell, frame);
    shell.activate_desktop_menu_item(DesktopShell::MENU_FRAME_FOLDER);
    shell.desktop_menu.hide();
    shell.set_chooser_entries(vec![dir("Trips")]);
    drop(shell.handle_hotkey(&key(Key::Escape)));
    assert!(!shell.chooser_open());
    assert_eq!(
        shell.widgets.frame_folder(frame),
        Some(PathBuf::from("/pictures"))
    );
    assert!(!shell.take_widgets_dirty());
}

/// **Only a photo frame's menu offers a folder**: a clock's does not.
#[test]
fn only_a_photo_frame_offers_a_folder() {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_CLOCK);
    let clock = shell.widgets.all_widgets()[0].id;
    let labels = menu_on(&mut shell, clock);
    assert!(labels.iter().all(|l| l != "Choose folder…"), "{labels:?}");
}
