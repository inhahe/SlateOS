//! The character picker over the shell's own fields: opened from a field's
//! menu or by Ctrl+. in the field, what is picked is typed into the field --
//! with what follows a typed change there -- and the picker goes; Escape or a
//! press away takes it down with nothing typed; and what it learned is there
//! the next time it opens.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{beside, menu_row};
use crate::widgets::{WidgetInstanceId, WidgetKind};
use crate::{
    DesktopShell, Key, KeyEvent, MenuField, Modifiers, MouseButton, MouseEvent, MouseEventKind,
    Palette, Rect, ShellAction, click,
};
use charpicker::{SkinTone, Target};
use guitk::menu::MenuItem;

fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
    MouseEvent { x, y, kind }
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

/// Ctrl+., as a keyboard sends it.
fn ctrl_period() -> KeyEvent {
    KeyEvent {
        key: Key::Period,
        pressed: true,
        modifiers: Modifiers::ctrl(),
        text: String::new(),
    }
}

/// A key as the session delivers it: to the shell's hot keys, and on to the
/// desktop's own keys -- a note's, a rename's -- when they did not claim it.
fn key(shell: &mut DesktopShell, k: &KeyEvent) {
    if !shell.handle_hotkey(k).consumed
        && (shell.icons.renaming().is_some() || shell.widgets.writing_note().is_some())
    {
        let _ = shell.handle_desktop_key(k);
    }
}

/// Type `text` a character at a time, as the session delivers keys.
fn type_text(shell: &mut DesktopShell, text: &str) {
    for c in text.chars() {
        key(shell, &typed(&c.to_string()));
    }
}

/// A shell with the Run box up and `line` typed into it.
fn run_box_with(line: &str) -> DesktopShell {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.toggle_run_dialog();
    type_text(&mut shell, line);
    assert_eq!(shell.run_dialog.line(), line);
    shell
}

/// Choose the open field menu's row that opens the picker, with a click.
fn choose_picker_row(shell: &mut DesktopShell) {
    let (menu, _) = shell.field_menu.as_ref().expect("the field's menu is open");
    let index = menu
        .items()
        .iter()
        .position(
            |i| matches!(i, MenuItem::Action { label, .. } if label == charpicker::MENU_LABEL),
        )
        .expect("the menu offers the picker");
    let r = menu.item_rect(index).expect("the row is shown");
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    let _released = shell.handle_mouse(&at(x, y, MouseEventKind::Release(MouseButton::Left)));
}

/// Where on the screen the open picker drew `target`'s middle.
fn picker_spot(shell: &DesktopShell, target: Target) -> (f32, f32) {
    let open = shell.char_picker.as_ref().expect("the picker is up");
    let frame = open
        .picker
        .frame(&Palette::for_mode(false), open.rect.w, open.rect.h);
    let r = frame
        .rect_of(|t| *t == target)
        .unwrap_or_else(|| panic!("the picker drew no {target:?}"));
    (open.rect.x + r.x + r.w / 2.0, open.rect.y + r.y + r.h / 2.0)
}

/// Add a note from the desktop menu and open it for writing, answering it.
fn writing_note(shell: &mut DesktopShell) -> WidgetInstanceId {
    let _ = shell.activate_desktop_menu_item(DesktopShell::MENU_ADD_NOTE);
    let id = shell
        .widgets
        .all_widgets()
        .iter()
        .find(|w| matches!(w.kind, WidgetKind::Notes))
        .map(|w| w.id)
        .expect("a note on the desktop");
    let (x, y, w, h) = shell.widgets.content_rect(id).expect("placed");
    let _ = shell.handle_mouse(&click(x + w / 2.0, y + h / 2.0));
    let _ = shell.handle_mouse(&at(
        x + w / 2.0,
        y + h / 2.0,
        MouseEventKind::Release(MouseButton::Left),
    ));
    assert_eq!(shell.widgets.writing_note(), Some(id));
    id
}

/// **The Run box's menu opens the picker over the box, and a pick is typed
/// into the line** -- the keys that searched for it go no further, and the
/// picker goes when it has typed.
#[test]
fn the_run_boxs_menu_opens_the_picker_and_a_pick_is_typed_into_the_line() {
    let mut shell = run_box_with("echo ");
    let f = shell.run_dialog.field_rect();
    let _ = shell.handle_mouse(&at(
        f.x + f.w / 2.0,
        f.y + f.h / 2.0,
        MouseEventKind::Press(MouseButton::Right),
    ));
    choose_picker_row(&mut shell);
    assert!(shell.field_menu.is_none());
    assert!(shell.char_picker_open());
    assert!(shell.any_popup_open(), "Escape must be held while it is up");
    assert!(shell.render_char_picker().is_some());
    assert!(shell.run_dialog.is_visible(), "the picker closed the box");

    type_text(&mut shell, "U+00E9");
    assert_eq!(
        shell.run_dialog.line(),
        "echo ",
        "a search key reached the line"
    );
    key(&mut shell, &pressed(Key::Enter));
    assert!(!shell.char_picker_open(), "a pick left the picker up");
    assert!(shell.render_char_picker().is_none());
    assert_eq!(shell.run_dialog.line(), "echo \u{E9}");
    assert!(shell.run_dialog.is_visible(), "the pick ran the line");
    // And the typing goes on in the line.
    type_text(&mut shell, "!");
    assert_eq!(shell.run_dialog.line(), "echo \u{E9}!");
}

/// **Ctrl+. in the Run box's line opens the picker; Escape takes it down
/// and leaves the box, and the line as it was.**
#[test]
fn ctrl_period_opens_the_picker_and_escape_takes_it_down() {
    let mut shell = run_box_with("ls");
    key(&mut shell, &ctrl_period());
    assert!(shell.char_picker_open());
    let picker = shell.char_picker.as_ref().unwrap();
    let field = shell.run_dialog.field_rect();
    assert!(
        picker.rect.y >= field.bottom() || picker.rect.bottom() <= field.y,
        "over the field"
    );
    key(&mut shell, &pressed(Key::Escape));
    assert!(!shell.char_picker_open());
    assert!(
        shell.run_dialog.is_visible(),
        "Escape closed the box under the picker"
    );
    assert_eq!(shell.run_dialog.line(), "ls");
}

/// **A click on a cell types it into the start menu's search, which searches
/// as for a typed one.**
#[test]
fn a_click_types_into_the_start_menus_search() {
    let mut shell = DesktopShell::new(1600, 1000);
    shell.toggle_start_menu();
    key(&mut shell, &ctrl_period());
    assert!(shell.char_picker_open());
    type_text(&mut shell, "U+00E9");
    let (x, y) = picker_spot(&shell, Target::Cell(0));
    assert_eq!(shell.handle_mouse(&click(x, y)), ShellAction::Consumed);
    assert!(!shell.char_picker_open());
    assert_eq!(shell.start_query.text(), "\u{E9}");
    assert!(shell.start_menu_open);
}

/// **A press away from the picker takes it down, is spent doing so, and
/// types nothing.**
#[test]
fn a_press_away_takes_the_picker_down() {
    let mut shell = run_box_with("ls");
    key(&mut shell, &ctrl_period());
    let r = shell.char_picker.as_ref().unwrap().rect;
    let away = if r.x > 10.0 {
        (5.0, 5.0)
    } else {
        (1590.0, 990.0)
    };
    assert!(!r.contains(away.0, away.1));
    assert_eq!(
        shell.handle_mouse(&click(away.0, away.1)),
        ShellAction::Consumed
    );
    assert!(!shell.char_picker_open());
    assert!(
        shell.run_dialog.is_visible(),
        "the press closed the box too"
    );
    assert_eq!(shell.run_dialog.line(), "ls");
    // A move away is the picker's to ignore, and leaves it up.
    key(&mut shell, &ctrl_period());
    assert_eq!(
        shell.handle_mouse(&at(away.0, away.1, MouseEventKind::Move)),
        ShellAction::Consumed
    );
    assert!(shell.char_picker_open());
}

/// **A note takes a pick as it takes a typed character: in its words, and
/// saved with the layout.**
#[test]
fn a_note_takes_a_pick() {
    let mut shell = DesktopShell::new(1600, 1000);
    let id = writing_note(&mut shell);
    type_text(&mut shell, "tea ");
    shell.widgets_dirty = false;
    key(&mut shell, &ctrl_period());
    assert!(shell.char_picker_open());
    type_text(&mut shell, "hot beverage");
    key(&mut shell, &pressed(Key::Enter));
    assert!(!shell.char_picker_open());
    assert_eq!(
        shell.widgets.get(id).expect("placed").state_text,
        "tea \u{2615}"
    );
    assert!(shell.widgets_dirty, "the note's change was not saved");
    assert_eq!(
        shell.widgets.writing_note(),
        Some(id),
        "the pick put the note down"
    );
}

/// **A name being edited takes a pick, and keeps it when the rename ends.**
#[test]
fn a_rename_takes_a_pick() {
    let mut shell = DesktopShell::new(1600, 1000);
    let (icon, added) = shell.icons.add_shortcut_at(
        "Notes",
        crate::icons::IconType::Executable,
        crate::icons::IconAction::OpenPath(std::path::PathBuf::from("/usr/bin/notes")),
        1_400.0,
        100.0,
    );
    assert!(added);
    assert!(shell.icons.begin_rename(icon));
    key(&mut shell, &pressed(Key::End));
    key(&mut shell, &typed(" "));
    key(&mut shell, &ctrl_period());
    assert!(shell.char_picker_open());
    type_text(&mut shell, "U+2605");
    key(&mut shell, &pressed(Key::Enter));
    assert!(!shell.char_picker_open());
    assert_eq!(
        shell.icons.renaming(),
        Some(icon),
        "the pick ended the rename"
    );
    key(&mut shell, &pressed(Key::Enter));
    assert_eq!(
        shell.icons.get_icon(icon).expect("still there").label,
        "Notes \u{2605}"
    );
}

/// **The picker remembers, from one opening to the next, what was picked
/// lately and the skin tone** -- and opens on what was picked.
#[test]
fn the_picker_remembers_its_picks_and_tone() {
    let mut shell = run_box_with("");
    key(&mut shell, &ctrl_period());
    let (x, y) = picker_spot(&shell, Target::Tone(Some(SkinTone::Medium)));
    let _ = shell.handle_mouse(&click(x, y));
    type_text(&mut shell, "waving hand");
    key(&mut shell, &pressed(Key::Enter));
    assert_eq!(shell.run_dialog.line(), "\u{1F44B}\u{1F3FD}");
    assert_eq!(shell.char_recent, ["\u{1F44B}"]);
    assert_eq!(shell.char_tone, Some(SkinTone::Medium));

    key(&mut shell, &ctrl_period());
    let open = shell.char_picker.as_ref().unwrap();
    assert_eq!(open.picker.category(), charpicker::Category::Recent);
    assert_eq!(open.picker.tone(), Some(SkinTone::Medium));
    assert_eq!(
        open.picker.shown().collect::<Vec<_>>(),
        ["\u{1F44B}\u{1F3FD}"]
    );
    // Turned down, it still keeps what it learned.
    key(&mut shell, &pressed(Key::Escape));
    assert_eq!(shell.char_recent, ["\u{1F44B}"]);
}

/// **Whatever takes the popups down takes the picker with them**, so it is
/// never left over a field that has gone.
#[test]
fn dismissing_the_popups_takes_the_picker_down() {
    let mut shell = run_box_with("ls");
    key(&mut shell, &ctrl_period());
    assert!(shell.dismiss_popups());
    assert!(!shell.char_picker_open());
    assert!(!shell.any_popup_open());
}

/// **Nothing opens over a field that is not up.**
#[test]
fn nothing_opens_over_a_field_that_is_not_up() {
    let mut shell = DesktopShell::new(1600, 1000);
    for field in [MenuField::RunBox, MenuField::StartSearch, MenuField::Rename] {
        shell.open_char_picker(field);
        assert!(!shell.char_picker_open(), "{field:?}");
    }
    let id = writing_note(&mut shell);
    let _ = shell.dismiss_popups();
    assert!(shell.widgets.end_note());
    shell.open_char_picker(MenuField::Note(id));
    assert!(!shell.char_picker_open(), "a note nobody is writing in");
    // Ctrl+. on the desktop itself is nobody's.
    assert!(!shell.handle_hotkey(&ctrl_period()).consumed);
    assert_eq!(shell.handle_desktop_key(&ctrl_period()), ShellAction::Pass);
    assert!(!shell.char_picker_open());
}

/// **Every field's menu offers the picker, with its chord.**
#[test]
fn the_menu_row_names_the_picker_and_its_chord() {
    let MenuItem::Action {
        label,
        shortcut,
        enabled,
        ..
    } = menu_row()
    else {
        panic!("not a row");
    };
    assert_eq!(label, charpicker::MENU_LABEL);
    assert_eq!(shortcut.as_deref(), Some(charpicker::SHORTCUT_LABEL));
    assert!(enabled);
}

/// **The picker goes below its field, above it where there is no room
/// below, and stays on the screen.**
#[test]
fn the_picker_goes_beside_its_field() {
    let screen = (1600.0, 1000.0);
    let size = (560.0, 460.0);
    let field = Rect::new(100.0, 100.0, 300.0, 28.0);
    assert_eq!(
        beside(field, size, screen),
        Rect::new(100.0, 128.0, 560.0, 460.0)
    );
    // No room below: above.
    let low = Rect::new(100.0, 900.0, 300.0, 28.0);
    assert_eq!(
        beside(low, size, screen),
        Rect::new(100.0, 440.0, 560.0, 460.0)
    );
    // Near the right edge: moved left onto the screen.
    let right = Rect::new(1500.0, 100.0, 90.0, 28.0);
    assert_eq!(beside(right, size, screen).x, 1040.0);
    // Room neither below nor above: at the bottom of the screen.
    let tall = Rect::new(0.0, 300.0, 300.0, 400.0);
    let placed = beside(tall, size, screen);
    assert_eq!(placed.bottom(), 1000.0);
    // Larger than the screen: the screen, from its corner.
    let placed = beside(field, size, (400.0, 300.0));
    assert_eq!(placed, Rect::new(0.0, 0.0, 400.0, 300.0));
}
