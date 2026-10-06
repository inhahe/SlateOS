//! Tests for the dialogs as tools see them: a message box answered by its
//! buttons, an input dialog's field typed over and confirmed, a progress
//! dialog's bar and its Cancel, a floating dialog closed -- and a dialog
//! that has answered taking nothing more while it fades.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::palette::Palette;
use crate::render::RenderTree;

/// The size of what the dialogs are drawn over.
const W: f32 = 1024.0;
const H: f32 = 768.0;

/// `dialog` shown and drawn once.
fn shown(mut dialog: AlertDialog) -> AlertDialog {
    dialog.show();
    dialog.render(&Palette::for_mode(false), W, H, &mut RenderTree::new());
    dialog
}

/// `dialog` shown and drawn once.
fn prompt(mut dialog: InputDialog) -> InputDialog {
    dialog.show();
    redraw(&mut dialog);
    dialog
}

/// `dialog` drawn again, as the next frame draws it.
fn redraw(dialog: &mut InputDialog) {
    dialog.render(&Palette::for_mode(false), W, H, &mut RenderTree::new());
}

fn act<D: Accessible<Part = ModalPart, Event = DialogResult>>(
    dialog: &mut D,
    part: ModalPart,
    action: Action,
) -> Result<Option<DialogResult>, Refusal> {
    dialog.invoke(&part, action, W, H)
}

fn erase() -> AlertDialog {
    shown(
        AlertDialog::destructive("Erase disk", "Erase 'USB Drive'?", "Erase Disk")
            .with_detail("Everything on it will be lost."),
    )
}

/// **A message box shows tools its title, what it says, and its buttons**
/// -- each by its label, the one with the keyboard focused, the
/// destructive one said to be -- **and a button pressed is clicked**: the
/// dialog answers, keeps the answer for its host, and is on its way out.
#[test]
fn a_message_box_is_answered_by_its_buttons() {
    let mut dialog = erase();
    let root = dialog.automation(W, H);
    assert_eq!(
        (root.role, root.name.as_str()),
        (Role::Dialog, "Erase disk")
    );
    assert_eq!(
        root.description.as_deref(),
        Some("Erase 'USB Drive'?\nEverything on it will be lost.")
    );
    assert!(root.shown);
    let buttons: Vec<(&str, bool, Option<&str>)> = root
        .children
        .iter()
        .map(|n| (n.name.as_str(), n.focused, n.description.as_deref()))
        .collect();
    assert_eq!(
        buttons,
        [
            ("Erase Disk", false, Some("destructive")),
            ("Cancel", true, None),
        ]
    );
    for (index, node) in root.children.iter().enumerate() {
        assert_eq!(
            Some(node.bounds),
            dialog.button_rect(index).map(rect_of),
            "{}",
            node.name
        );
    }

    assert_eq!(
        act(&mut dialog, ModalPart::Button(1), Action::Press),
        Ok(Some(DialogResult::Cancel))
    );
    assert_eq!(dialog.result(), Some(&DialogResult::Cancel));
    assert!(dialog.is_active(), "fading");
    assert!(!dialog.automation(W, H).shown, "on its way out");
    assert_eq!(
        act(&mut dialog, ModalPart::Button(0), Action::Press),
        Err(Refusal::Hidden)
    );
    assert_eq!(dialog.result(), Some(&DialogResult::Cancel));
}

/// **A dialog that has answered takes nothing more while it fades.** A click
/// where "Erase Disk" was drawn, after "Cancel", turned the answer into the
/// erase; and a key typed into an input dialog that had handed its text
/// back went into it.
#[test]
fn an_answer_is_not_changed_while_the_dialog_fades() {
    let mut dialog = erase();
    let (cancel, erase) = (
        dialog.button_rect(1).map(rect_of).unwrap(),
        dialog.button_rect(0).map(rect_of).unwrap(),
    );
    // The keyboard on "Erase Disk", so Enter would answer it too.
    let _ = dialog.handle_event(&key(Key::Tab, Modifiers::NONE, ""));
    assert_eq!(dialog.focused_button(), 0);
    let _ = dialog.handle_event(&click(cancel.centre()));
    let _ = dialog.handle_event(&click(erase.centre()));
    assert_eq!(dialog.result(), Some(&DialogResult::Cancel), "a click");
    let _ = dialog.handle_event(&key(Key::Enter, Modifiers::NONE, ""));
    assert_eq!(dialog.result(), Some(&DialogResult::Cancel), "a key");
    let _ = dialog.handle_event(&Event::Tick { elapsed_ms: 1000 });
    assert!(!dialog.is_active(), "the fade still runs");

    let mut input = prompt(InputDialog::prompt("Rename", "New name:", ""));
    let _ = input.handle_event(&key(Key::Unknown(0), Modifiers::NONE, "a"));
    let _ = input.handle_event(&key(Key::Enter, Modifiers::NONE, ""));
    let _ = input.handle_event(&key(Key::Unknown(0), Modifiers::NONE, "b"));
    let _ = input.handle_event(&key(Key::Escape, Modifiers::NONE, ""));
    assert_eq!(input.result(), Some(&DialogResult::Text("a".to_owned())));
    assert_eq!(input.input_text(), "a");
}

/// **The keyboard is brought to a button with Tab**, round the row; and a
/// dialog's parts are refused what the user could not do to them -- a
/// button past the row, a part it has not got, a dialog not up or not yet
/// drawn.
#[test]
fn a_message_box_refuses_what_its_user_could_not_do() {
    let mut dialog = erase();
    assert_eq!(
        act(&mut dialog, ModalPart::Button(0), Action::Focus),
        Ok(None)
    );
    assert_eq!(dialog.focused_button(), 0);
    assert_eq!(dialog.result(), None, "focused, not pressed");
    let mut three = shown(AlertDialog::yes_no_cancel("Save", "Save changes?"));
    act(&mut three, ModalPart::Button(2), Action::Focus).unwrap();
    assert_eq!(three.focused_button(), 2, "two steps of Tab");
    assert_eq!(
        act(&mut dialog, ModalPart::Button(2), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut dialog, ModalPart::Field, Action::Press),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        act(&mut dialog, ModalPart::Dialog, Action::Press),
        Err(Refusal::NotApplicable {
            role: Role::Dialog,
            action: "press"
        })
    );
    assert_eq!(
        act(&mut dialog, ModalPart::Button(0), Action::Toggle),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "toggle"
        })
    );

    let mut unseen = AlertDialog::confirm("Quit", "Quit now?");
    assert!(!unseen.automation(W, H).shown);
    assert_eq!(
        act(&mut unseen, ModalPart::Button(0), Action::Press),
        Err(Refusal::Hidden),
        "never shown"
    );
    unseen.show();
    assert!(!unseen.automation(W, H).shown, "not drawn yet");
    assert_eq!(
        act(&mut unseen, ModalPart::Button(0), Action::Press),
        Err(Refusal::Hidden),
        "nothing on screen to aim at"
    );
}

/// **An input dialog's field is named by its prompt, typed over as the user
/// types, and confirmed with Enter**; OK presses as clicked; a field its
/// host has said is wrong says why, and OK does not take it.
#[test]
fn an_input_dialogs_field_is_typed_over_and_confirmed() {
    let mut dialog =
        prompt(InputDialog::prompt("Rename", "New name:", "Untitled").with_initial_text("old.txt"));
    let root = dialog.automation(W, H);
    assert_eq!(root.name, "Rename");
    let names: Vec<(ModalPart, &str)> = root
        .children
        .iter()
        .map(|n| (n.id, n.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            (ModalPart::Field, "New name:"),
            (ModalPart::Button(0), "OK"),
            (ModalPart::Button(1), "Cancel"),
        ]
    );
    let field = &root.children[0];
    assert_eq!(field.value, Some(Value::Text("old.txt".to_owned())));
    assert_eq!(field.description.as_deref(), Some("Untitled"));
    assert!(field.focused);

    // From Cancel, so the field is clicked into first.
    act(&mut dialog, ModalPart::Button(1), Action::Focus).unwrap();
    assert!(dialog.automation(W, H).children[2].focused);
    assert_eq!(
        act(
            &mut dialog,
            ModalPart::Field,
            Action::SetText("new.txt".to_owned())
        ),
        Ok(None)
    );
    assert_eq!(dialog.input_text(), "new.txt");
    dialog.set_validation_error(Some("That name is taken."));
    redraw(&mut dialog);
    assert_eq!(
        dialog.automation(W, H).children[0].description.as_deref(),
        Some("That name is taken.")
    );
    assert_eq!(
        act(&mut dialog, ModalPart::Button(0), Action::Press),
        Ok(None),
        "OK does not take a name said to be wrong"
    );
    assert!(dialog.automation(W, H).shown);
    act(
        &mut dialog,
        ModalPart::Field,
        Action::SetText(String::new()),
    )
    .unwrap();
    assert_eq!(dialog.input_text(), "");
    act(
        &mut dialog,
        ModalPart::Field,
        Action::SetText("newer.txt".to_owned()),
    )
    .unwrap();
    // From Cancel, where Enter would press Cancel: the field is clicked
    // into first, so Enter is the field's.
    act(&mut dialog, ModalPart::Button(1), Action::Focus).unwrap();
    assert_eq!(
        act(&mut dialog, ModalPart::Field, Action::Press),
        Ok(Some(DialogResult::Text("newer.txt".to_owned())))
    );
}

/// **A password typed into an input dialog is never shown to tools**,
/// though a tool may type one; and Cancel pressed answers Cancel.
#[test]
fn a_password_is_never_shown() {
    let mut dialog =
        prompt(InputDialog::prompt("Unlock", "Password:", "").with_password_mode(true));
    act(
        &mut dialog,
        ModalPart::Field,
        Action::SetText("hunter2".to_owned()),
    )
    .unwrap();
    assert_eq!(dialog.input_text(), "hunter2");
    let field = dialog.automation(W, H).children[0].clone();
    assert_eq!(field.value, None);
    assert_eq!(field.description, None);
    assert_eq!(
        act(&mut dialog, ModalPart::Button(1), Action::Press),
        Ok(Some(DialogResult::Cancel))
    );
}

/// **A progress dialog shows tools its bar -- how far, where that is known
/// -- and its Cancel, which pressed cancels the work**; one with no Cancel
/// has none to press.
#[test]
fn a_progress_dialog_shows_its_progress_and_is_cancelled() {
    let mut dialog = ProgressDialog::determinate("Copying", "Copying 3 files")
        .with_cancel()
        .with_detail("a.txt");
    dialog.show();
    dialog.set_progress(0.25);
    dialog.render(&Palette::for_mode(false), W, H, &mut RenderTree::new());
    let root = dialog.automation(W, H);
    assert_eq!(root.description.as_deref(), Some("Copying 3 files\na.txt"));
    let bar = &root.children[0];
    assert_eq!(
        (bar.role, bar.name.as_str()),
        (Role::ProgressBar, "Copying 3 files")
    );
    assert_eq!(
        bar.value,
        Some(Value::Progress {
            value: 0.25,
            max: 1.0
        })
    );
    let (bar_x, bar_y) = bar.bounds.centre();
    assert!(
        bar.bounds.w > 0.0 && root.bounds.contains(bar_x, bar_y),
        "{:?}",
        bar.bounds
    );
    assert_eq!(root.children[1].name, "Cancel");
    assert_eq!(
        act(&mut dialog, ModalPart::Button(0), Action::Press),
        Ok(Some(DialogResult::Cancel))
    );
    assert!(dialog.is_cancelled());

    let mut working = ProgressDialog::indeterminate("Scanning", "Looking for files");
    working.show();
    working.render(&Palette::for_mode(false), W, H, &mut RenderTree::new());
    let root = working.automation(W, H);
    assert_eq!(root.children.len(), 1, "no Cancel");
    assert_eq!(root.children[0].value, None, "how far is not known");
    assert_eq!(
        act(&mut working, ModalPart::Button(0), Action::Press),
        Err(Refusal::NoSuchWidget)
    );
}

/// **A floating dialog's close button pressed puts it away.**
#[test]
fn a_floating_dialog_is_closed() {
    let mut dialog = NonModalDialog::new("Find").with_position(50.0, 60.0);
    assert!(!dialog.automation(W, H).shown);
    assert_eq!(
        act(&mut dialog, ModalPart::Close, Action::Press),
        Err(Refusal::Hidden)
    );
    dialog.show();
    let root = dialog.automation(W, H);
    assert_eq!(
        (root.name.as_str(), root.bounds.x, root.bounds.y),
        ("Find", 50.0, 60.0)
    );
    let close = &root.children[0];
    assert_eq!((close.role, close.name.as_str()), (Role::Button, "Close"));
    let (_, middle) = close.bounds.centre();
    assert!(
        (middle - (60.0 + crate::text::scaled(super::super::TITLE_BAR_HEIGHT) / 2.0)).abs() < 0.01,
        "centred in the title bar: {:?}",
        close.bounds
    );
    assert_eq!(act(&mut dialog, ModalPart::Close, Action::Press), Ok(None));
    assert!(!dialog.is_visible());
}
