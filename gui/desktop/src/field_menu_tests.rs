//! Tests for the menu a right-click on one of the shell's own text fields
//! offers -- the Run box's command line, the start menu's search, a note's
//! writing area, an icon's name being edited: Cut, Copy, Paste, Delete and
//! Select all (and Undo and Redo on a note, which keeps a history), each
//! dimmed when it would do nothing (`guitk::editmenu`).
//!
//! The toolkit's tests hold what each row does to a field. These hold the
//! shell's part: which presses open the menu and which do not, that it opens
//! over the Run box and the start menu without closing them, that it closes
//! as every menu does -- a row chosen, a press elsewhere, Escape -- and that
//! what follows a change made from it is what follows a typed one: the Run
//! box's suggestions, the start menu's list, a note saved with the layout.

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
    DesktopShell, Hit, Key, KeyEvent, MenuField, Modifiers, MouseButton, MouseEvent,
    MouseEventKind, ShellAction, click,
};
use guitk::menu::MenuItem;

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
}

/// A right press at `(x, y)`, answering what the shell made of it.
fn right_click(shell: &mut DesktopShell, x: f32, y: f32) -> ShellAction {
    shell.handle_mouse(&at(x, y, MouseEventKind::Press(MouseButton::Right)))
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

fn ctrl(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::ctrl(),
        text: String::new(),
    }
}

/// The open field menu's rows and whether each is lit, separators left out.
fn rows(shell: &DesktopShell) -> Vec<(String, bool)> {
    let (menu, _) = shell.field_menu.as_ref().expect("the field's menu is open");
    menu.items()
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action { label, enabled, .. } => Some((label.clone(), *enabled)),
            _ => None,
        })
        .collect()
}

/// `rows` spelled as the tests spell them.
fn lit(rows: &[(&str, bool)]) -> Vec<(String, bool)> {
    rows.iter().map(|(l, on)| ((*l).to_owned(), *on)).collect()
}

/// The middle of the field menu's row `label`, on the screen.
fn row_middle(shell: &DesktopShell, label: &str) -> (f32, f32) {
    let (menu, _) = shell.field_menu.as_ref().expect("the field's menu is open");
    let index = menu
        .items()
        .iter()
        .position(|i| matches!(i, MenuItem::Action { label: l, .. } if l == label))
        .unwrap_or_else(|| panic!("the menu has no {label:?} row"));
    let r = menu.item_rect(index).expect("the row is shown");
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// Choose the field menu's row `label` with a click on it, as a user does --
/// the press and the release -- answering what the press did.
fn choose(shell: &mut DesktopShell, label: &str) -> ShellAction {
    let (x, y) = row_middle(shell, label);
    let action = shell.handle_mouse(&click(x, y));
    let _released = shell.handle_mouse(&at(x, y, MouseEventKind::Release(MouseButton::Left)));
    assert!(
        shell.field_menu.is_none(),
        "choosing {label:?} left the menu up"
    );
    action
}

/// A shell with the Run box up and `line` typed into it.
fn run_box_with(line: &str) -> DesktopShell {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.toggle_run_dialog();
    assert!(shell.run_dialog.is_visible());
    for ch in line.chars() {
        let _ = shell.handle_hotkey(&typed(&ch.to_string()));
    }
    assert_eq!(shell.run_dialog.line(), line);
    shell
}

/// The middle of the Run box's command line.
fn run_line_middle(shell: &DesktopShell) -> (f32, f32) {
    let f = shell.run_dialog.field_rect();
    (f.x + f.w / 2.0, f.y + f.h / 2.0)
}

/// Add a note from the desktop menu, answering it and the middle of its
/// writing area.
fn add_note(shell: &mut DesktopShell) -> (WidgetInstanceId, (f32, f32)) {
    let _ = shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_NOTE);
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

fn note_text(shell: &DesktopShell, id: WidgetInstanceId) -> String {
    shell.widgets.get(id).expect("placed").state_text.clone()
}

// ---- the Run box ----

/// **A right-click on the Run box's line offers what its keys do, over the
/// box, which stays** -- every row there and lit only when it can act -- and
/// Select all, Cut and Paste do to the line what Ctrl+A, Ctrl+X and Ctrl+V
/// would.
#[test]
fn the_run_boxs_line_offers_its_menu_and_the_box_stays() {
    guitk::clipboard::set_text("");
    let mut shell = run_box_with("terminal");
    let (x, y) = run_line_middle(&shell);

    assert_eq!(right_click(&mut shell, x, y), ShellAction::Consumed);
    assert!(shell.run_dialog.is_visible(), "the menu closed the box");
    assert_eq!(
        shell.field_menu.as_ref().map(|(_, field)| *field),
        Some(MenuField::RunBox)
    );
    assert!(shell.render_field_menu().is_some(), "the menu is not drawn");
    assert_eq!(
        rows(&shell),
        lit(&[
            ("Cut", false),
            ("Copy", false),
            ("Paste", false),
            ("Delete", false),
            ("Select all", true),
            (charpicker::MENU_LABEL, true),
        ])
    );
    assert_eq!(choose(&mut shell, "Select all"), ShellAction::Consumed);

    right_click(&mut shell, x, y);
    assert_eq!(
        rows(&shell),
        lit(&[
            ("Cut", true),
            ("Copy", true),
            ("Paste", false),
            ("Delete", true),
            ("Select all", true),
            (charpicker::MENU_LABEL, true),
        ])
    );
    choose(&mut shell, "Cut");
    assert_eq!(shell.run_dialog.line(), "");
    assert_eq!(guitk::clipboard::text(), "terminal");

    right_click(&mut shell, x, y);
    choose(&mut shell, "Paste");
    assert_eq!(shell.run_dialog.line(), "terminal");
    assert!(shell.run_dialog.is_visible());
    assert!(
        shell.render_field_menu().is_none(),
        "a closed menu is drawn"
    );
}

/// **The menu closes as every menu does, and the box stays**: a press off
/// it is spent closing it, and Escape closes the menu before it closes the
/// box. A right-click on the box away from its line -- its label -- opens
/// nothing.
#[test]
fn a_press_away_or_escape_closes_the_menu_and_leaves_the_box() {
    let mut shell = run_box_with("ls");
    let (x, y) = run_line_middle(&shell);

    right_click(&mut shell, x, y);
    assert!(shell.any_popup_open());
    assert_eq!(shell.handle_mouse(&click(5.0, 5.0)), ShellAction::Consumed);
    assert!(shell.field_menu.is_none(), "a press away left the menu up");
    assert!(
        shell.run_dialog.is_visible(),
        "the press that closed the menu closed the box too"
    );

    right_click(&mut shell, x, y);
    assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
    assert!(shell.field_menu.is_none(), "Escape left the menu up");
    assert!(
        shell.run_dialog.is_visible(),
        "Escape closed the box under the menu"
    );
    assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
    assert!(!shell.run_dialog.is_visible());

    let mut shell = run_box_with("ls");
    let f = shell.run_dialog.field_rect();
    assert_eq!(
        right_click(&mut shell, f.x - 20.0, f.y + f.h / 2.0),
        ShellAction::Consumed
    );
    assert!(
        shell.field_menu.is_none(),
        "a right-click on the label opened the menu"
    );
    assert!(shell.run_dialog.is_visible());
}

/// **The keyboard works the menu, and the keys go no further while it is
/// up**: Down reaches the one lit row, Enter chooses it -- Select all -- and
/// the next key typed replaces the whole line.
#[test]
fn the_keyboard_works_the_menu() {
    guitk::clipboard::set_text("");
    let mut shell = run_box_with("ls");
    let (x, y) = run_line_middle(&shell);
    right_click(&mut shell, x, y);

    // A letter while the menu is up is the menu's, not the line's.
    assert!(shell.handle_hotkey(&typed("q")).consumed);
    assert_eq!(shell.run_dialog.line(), "ls");

    assert!(shell.handle_hotkey(&pressed(Key::Down)).consumed);
    assert!(shell.handle_hotkey(&pressed(Key::Enter)).consumed);
    assert!(shell.field_menu.is_none(), "Enter left the menu up");
    assert!(
        shell.run_dialog.is_visible(),
        "Enter ran the line under the menu"
    );
    let _ = shell.handle_hotkey(&typed("x"));
    assert_eq!(
        shell.run_dialog.line(),
        "x",
        "Select all did not select the line"
    );
}

/// **The pointer lights a row and Enter chooses the lit one**, as in every
/// menu: the pointer and the keys work the same menu.
#[test]
fn the_pointer_lights_a_row_and_enter_chooses_it() {
    guitk::clipboard::set_text("");
    let mut shell = run_box_with("ls");
    let (x, y) = run_line_middle(&shell);
    right_click(&mut shell, x, y);
    let (rx, ry) = row_middle(&shell, "Select all");

    assert_eq!(
        shell.handle_mouse(&at(rx, ry, MouseEventKind::Move)),
        ShellAction::Consumed
    );
    assert!(shell.handle_hotkey(&pressed(Key::Enter)).consumed);
    assert!(shell.field_menu.is_none(), "Enter left the menu up");
    let _ = shell.handle_hotkey(&typed("x"));
    assert_eq!(
        shell.run_dialog.line(),
        "x",
        "Enter did not choose the row the pointer lit"
    );
}

/// **Anything that dismisses the popups closes the field's menu with them**,
/// so it can never be left over a box that has gone.
#[test]
fn dismissing_the_popups_closes_the_menu() {
    let mut shell = run_box_with("ls");
    let (x, y) = run_line_middle(&shell);
    right_click(&mut shell, x, y);
    assert!(shell.dismiss_popups());
    assert!(shell.field_menu.is_none());
    assert!(!shell.run_dialog.is_visible());
    assert!(!shell.any_popup_open());
}

/// **Opening it closes every other menu**, as each menu's opening does --
/// whether or not a press could reach a field with one of them up.
#[test]
fn opening_it_closes_every_other_menu() {
    let mut shell = DesktopShell::new(1600, 1000);
    let (id, (x, y)) = add_note(&mut shell);
    assert!(shell.widgets.note_press(x, y, 1));
    let menu = || guitk::menu::ContextMenu::new(vec![MenuItem::Separator]);
    shell.open_desktop_menu(5.0, 5.0);
    assert!(shell.desktop_menu.is_visible());
    shell.tray_overflow_menu = Some((menu(), Vec::new()));
    shell.pin_menu = Some((menu(), crate::PinTarget::Pinned(0)));
    shell.taskbar_menu = Some(menu());
    shell.notification_menu = Some((menu(), "Chat".to_owned()));

    shell.open_field_menu(MenuField::Note(id), x, y);
    assert!(shell.field_menu.is_some());
    assert!(!shell.desktop_menu.is_visible(), "the desktop menu stayed");
    assert!(
        shell.tray_overflow_menu.is_none(),
        "the overflow list stayed"
    );
    assert!(shell.pin_menu.is_none(), "a pin menu stayed");
    assert!(shell.taskbar_menu.is_none(), "the taskbar's menu stayed");
    assert!(
        shell.notification_menu.is_none(),
        "a notification's menu stayed"
    );
}

/// **Nothing opens over a field with nothing to offer** -- a rename, or a
/// note being written, that went before the press arrived.
#[test]
fn nothing_opens_over_a_field_that_has_gone() {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.open_field_menu(MenuField::Rename, 100.0, 100.0);
    assert!(
        shell.field_menu.is_none(),
        "a menu opened over a rename that is not under way"
    );
    let (id, (x, y)) = add_note(&mut shell);
    shell.open_field_menu(MenuField::Note(id), x, y);
    assert!(
        shell.field_menu.is_none(),
        "a menu opened over a note nobody is writing in"
    );
}

// ---- the start menu's search ----

/// **The start menu's search offers its menu over the menu, which stays, and
/// a search pasted in is a search like a typed one**: the list is a new list,
/// so the keyboard's row through the old one is gone.
#[test]
fn a_search_can_be_pasted_into_the_start_menu() {
    guitk::clipboard::set_text("term");
    let mut shell = DesktopShell::new(1600, 1000);
    shell.toggle_start_menu();
    let f = shell.start_search_rect();
    let (x, y) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
    assert_eq!(
        shell.hit_test(x, y),
        Hit::StartMenuPanel,
        "the premise: the field is the menu's panel"
    );
    shell.start_selected = Some(1);

    assert_eq!(right_click(&mut shell, x, y), ShellAction::Consumed);
    assert!(
        shell.start_menu_open,
        "the field's menu closed the start menu"
    );
    assert_eq!(
        shell.field_menu.as_ref().map(|(_, field)| *field),
        Some(MenuField::StartSearch)
    );
    assert_eq!(
        rows(&shell),
        lit(&[
            ("Cut", false),
            ("Copy", false),
            ("Paste", true),
            ("Delete", false),
            ("Select all", false),
            (charpicker::MENU_LABEL, true),
        ])
    );
    choose(&mut shell, "Paste");
    assert_eq!(shell.start_query.text(), "term");
    assert_eq!(
        shell.start_selected, None,
        "the old list's row outlived the search"
    );
    assert!(
        shell.start_menu_open,
        "choosing Paste closed the start menu"
    );
}

/// **A right-click on the start menu off its search is not the field's**:
/// on the band round the field it opens nothing, and on a program's row it
/// is still that program's menu.
#[test]
fn a_right_click_on_the_start_menu_off_its_search_is_not_the_fields() {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.toggle_start_menu();
    let f = shell.start_search_rect();
    let (x, y) = (f.x + f.w / 2.0, f.y - 4.0);
    assert_eq!(
        shell.hit_test(x, y),
        Hit::StartMenuPanel,
        "the premise: the band above the field is the menu's panel"
    );
    assert_eq!(right_click(&mut shell, x, y), ShellAction::Consumed);
    assert!(
        shell.field_menu.is_none(),
        "the band beside the search offered the field's menu"
    );
    assert!(shell.start_menu_open);

    let first = shell.start_row_of_program(0).expect("a program");
    let row = shell.start_menu_row_rect(first);
    right_click(&mut shell, row.x + row.w / 2.0, row.y + row.h / 2.0);
    assert!(shell.field_menu.is_none());
    assert!(shell.pin_menu.is_some(), "the row's own menu did not open");
}

// ---- a note ----

/// **A right-click on a note's writing area opens the note and offers what
/// its keys do, with the widget's own rows below**; Escape closes the menu
/// and leaves the note open to type in.
#[test]
fn a_right_click_opens_a_note_and_offers_its_menu() {
    guitk::clipboard::set_text("");
    let mut shell = DesktopShell::new(1600, 1000);
    let (id, (x, y)) = add_note(&mut shell);
    assert_eq!(shell.widgets.writing_note(), None);

    assert_eq!(right_click(&mut shell, x, y), ShellAction::Consumed);
    assert_eq!(
        shell.widgets.writing_note(),
        Some(id),
        "the right-click did not open the note"
    );
    assert_eq!(
        shell.field_menu.as_ref().map(|(_, field)| *field),
        Some(MenuField::Note(id))
    );
    assert!(
        shell.any_popup_open(),
        "the menu is open and Escape is not held"
    );
    assert_eq!(
        rows(&shell),
        lit(&[
            ("Undo", false),
            ("Redo", false),
            ("Cut", false),
            ("Copy", false),
            ("Paste", false),
            ("Delete", false),
            ("Select all", false),
            (charpicker::MENU_LABEL, true),
            ("Remove this widget", true),
            ("Remove all widgets", true),
        ])
    );
    assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
    assert!(shell.field_menu.is_none());
    assert_eq!(
        shell.widgets.writing_note(),
        Some(id),
        "Escape put the note down"
    );
    assert_eq!(shell.handle_desktop_key(&typed("m")), ShellAction::Consumed);
    assert_eq!(note_text(&shell, id), "m");
}

/// **A note's Cut, Undo and Paste are changes saved with the layout, as
/// typed ones are** -- and a row chosen on the menu does not first put the
/// note down, though the menu is drawn over more than the note.
#[test]
fn a_notes_rows_change_it_and_are_saved() {
    guitk::clipboard::set_text("");
    let mut shell = DesktopShell::new(1600, 1000);
    let (id, (x, y)) = add_note(&mut shell);
    right_click(&mut shell, x, y);
    assert!(shell.handle_hotkey(&pressed(Key::Escape)).consumed);
    for ch in "milk".chars() {
        shell.handle_desktop_key(&typed(&ch.to_string()));
    }
    assert_eq!(
        shell.handle_desktop_key(&ctrl(Key::A)),
        ShellAction::Consumed
    );
    let _ = shell.take_widgets_dirty();

    right_click(&mut shell, x, y);
    let offered = rows(&shell);
    assert!(offered.contains(&("Cut".to_owned(), true)));
    assert!(offered.contains(&("Undo".to_owned(), true)));
    assert!(offered.contains(&("Redo".to_owned(), false)));
    choose(&mut shell, "Cut");
    assert_eq!(note_text(&shell, id), "");
    assert_eq!(guitk::clipboard::text(), "milk");
    assert_eq!(
        shell.widgets.writing_note(),
        Some(id),
        "the row put the note down"
    );
    assert!(
        shell.take_widgets_dirty(),
        "a cut from the menu was not saved"
    );

    right_click(&mut shell, x, y);
    choose(&mut shell, "Undo");
    assert_eq!(note_text(&shell, id), "milk");
    assert!(
        shell.take_widgets_dirty(),
        "an undo from the menu was not saved"
    );

    // Copy changes nothing, and nothing is saved for it.
    right_click(&mut shell, x, y);
    choose(&mut shell, "Copy");
    assert!(!shell.take_widgets_dirty(), "a copy was saved as a change");

    right_click(&mut shell, x, y);
    choose(&mut shell, "Paste");
    assert!(note_text(&shell, id).contains("milk"));
}

/// **The widget's rows on a note's menu are the widget menu's**: "Remove
/// this widget" takes the note away, and the note is no longer open.
#[test]
fn a_notes_menu_removes_the_note() {
    let mut shell = DesktopShell::new(1600, 1000);
    let (id, (x, y)) = add_note(&mut shell);
    let _ = shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_CLOCK);
    let _ = shell.take_widgets_dirty();
    right_click(&mut shell, x, y);
    choose(&mut shell, "Remove this widget");
    assert!(shell.widgets.get(id).is_none(), "the note is still there");
    assert_eq!(shell.widgets.writing_note(), None);
    assert_eq!(shell.widgets.count(), 1, "more than the note went");
    assert!(shell.take_widgets_dirty());

    let (_, (x, y)) = add_note(&mut shell);
    right_click(&mut shell, x, y);
    choose(&mut shell, "Remove all widgets");
    assert_eq!(shell.widgets.count(), 0);
}

/// **A note's title bar is still the widget's**: a right-click there offers
/// the widget menu, not the writing area's.
#[test]
fn a_notes_title_bar_offers_the_widget_menu() {
    let mut shell = DesktopShell::new(1600, 1000);
    let (id, _) = add_note(&mut shell);
    let (x, y, w, _) = shell.widgets.content_rect(id).expect("placed");
    right_click(&mut shell, x + w / 2.0, y - 12.0);
    assert!(
        shell.field_menu.is_none(),
        "the title bar offered the field's menu"
    );
    assert!(shell.desktop_menu.is_visible());
    assert_eq!(
        shell.widgets.writing_note(),
        None,
        "the title bar opened the note"
    );
}

// ---- an icon's name ----

/// **A right-click on an icon's name being edited offers its menu and keeps
/// the rename going**; what the rows do is in the name kept when it ends.
#[test]
fn a_rename_offers_its_menu_and_keeps_going() {
    guitk::clipboard::set_text("");
    let mut shell = DesktopShell::new(1600, 1000);
    let (icon, added) = shell.icons.add_shortcut_at(
        "Editor",
        crate::icons::IconType::Executable,
        crate::icons::IconAction::OpenPath(std::path::PathBuf::from("/usr/bin/editor")),
        1_400.0,
        100.0,
    );
    assert!(added);
    assert!(shell.icons.begin_rename(icon));
    let (fx, fy, fw, fh) = shell.icons.rename_field().expect("a rename under way");
    let (x, y) = (fx + fw / 2.0, fy + fh / 2.0);

    assert_eq!(right_click(&mut shell, x, y), ShellAction::Consumed);
    assert_eq!(
        shell.icons.renaming(),
        Some(icon),
        "the right-click ended the rename"
    );
    assert_eq!(
        rows(&shell),
        lit(&[
            ("Cut", true),
            ("Copy", true),
            ("Paste", false),
            ("Delete", true),
            ("Select all", true),
            (charpicker::MENU_LABEL, true),
        ])
    );
    choose(&mut shell, "Cut");
    assert_eq!(shell.icons.renaming(), Some(icon), "a row ended the rename");
    assert_eq!(guitk::clipboard::text(), "Editor");

    shell.handle_desktop_key(&typed("My "));
    right_click(&mut shell, x, y);
    choose(&mut shell, "Paste");
    assert_eq!(
        shell.handle_desktop_key(&pressed(Key::Enter)),
        ShellAction::Consumed
    );
    assert_eq!(shell.icons.renaming(), None);
    assert_eq!(
        shell.icons.get_icon(icon).expect("still there").label,
        "My Editor"
    );
}

// ---- why a row is greyed ----

/// **A greyed row says why once the pointer rests on it** -- Paste, with
/// nothing copied -- after the tooltip delay on the shell's overlay clock,
/// which the session is told to wake for; a lit row says nothing.
#[test]
fn a_greyed_row_says_why_once_the_pointer_rests_on_it() {
    guitk::clipboard::set_text("");
    let mut shell = run_box_with("terminal");
    let (x, y) = run_line_middle(&shell);
    let _ = right_click(&mut shell, x, y);
    let _ = shell.take_hover_changed();

    let (px, py) = row_middle(&shell, "Paste");
    let _ = shell.handle_mouse(&at(px, py, MouseEventKind::Move));
    assert!(shell.take_hover_changed(), "the session is told to wake");
    assert_eq!(
        shell.tooltip_due_in(),
        Some(0),
        "the wait starts at the next frame"
    );
    shell.advance_osd(16);
    let due = shell.tooltip_due_in().expect("a deadline to wake for");
    assert!(due > 0, "the delay is waited out, not skipped");
    assert!(!shell.take_hover_changed(), "nothing to draw yet");

    shell.advance_osd(due);
    assert!(shell.take_hover_changed(), "the reason appeared: repaint");
    let (menu, _) = shell.field_menu.as_ref().expect("still open");
    assert_eq!(menu.showing_reason(), Some("Nothing has been copied"));
    let drawn = format!("{:?}", shell.render_field_menu().expect("drawn"));
    assert!(drawn.contains("Nothing has been copied"), "{drawn}");

    let (sx, sy) = row_middle(&shell, "Select all");
    let _ = shell.handle_mouse(&at(sx, sy, MouseEventKind::Move));
    assert_eq!(shell.tooltip_due_in(), None, "a lit row has no reason");
    let (menu, _) = shell.field_menu.as_ref().expect("still open");
    assert_eq!(menu.showing_reason(), None);
}
