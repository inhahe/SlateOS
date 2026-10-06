#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;
use crate::dialog::DirEntry;
use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::widget::automation::Query;

const W: f32 = 600.0;
const H: f32 = 400.0;

/// A listed file, ten bytes long.
fn file(name: &str) -> DirEntry {
    DirEntry {
        name: OsString::from(name),
        is_dir: false,
        size: 10,
        modified_timestamp: 1_000,
        extension: name
            .rsplit_once('.')
            .map_or(OsString::new(), |(_, e)| OsString::from(e)),
    }
}

/// A listed folder.
fn folder(name: &str) -> DirEntry {
    DirEntry {
        name: OsString::from(name),
        is_dir: true,
        size: 0,
        modified_timestamp: 1_000,
        extension: OsString::new(),
    }
}

/// The node of `part`.
fn node(dialog: &FileDialog, part: DialogTarget) -> Node<DialogTarget> {
    dialog
        .automation(W, H)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// **The dialog shows tools every part**, in its order: Back, Forward and
/// Up -- each in use only with somewhere to go -- the address, the places,
/// the headings (the one sorting saying so), the files with what each is,
/// the name of a Save dialog, Save -- out of use with no name -- and Cancel.
#[test]
fn the_dialog_shows_tools_every_part() {
    let mut dialog = FileDialog::save().with_initial_path("/docs");
    dialog.set_entries(vec![folder("pics"), file("a.txt")]);
    let tree = dialog.automation(W, H);
    assert_eq!((tree.role, tree.name.as_str()), (Role::Dialog, "Save"));
    assert_eq!(tree.description.as_deref(), Some("/docs"));
    let parts: Vec<(Role, &str, bool)> = tree
        .children
        .iter()
        .map(|node| (node.role, node.name.as_str(), node.enabled))
        .collect();
    assert_eq!(
        parts,
        [
            (Role::Button, "Back", false),
            (Role::Button, "Forward", false),
            (Role::Button, "Up", true),
            (Role::TextField, "Address", true),
            (Role::List, "Places", true),
            (Role::Button, "Name", true),
            (Role::Button, "Size", true),
            (Role::Button, "Modified", true),
            (Role::List, "Files", true),
            (Role::TextField, "File name", true),
            (Role::Button, "Save", false),
            (Role::Button, "Cancel", true),
        ]
    );
    assert_eq!(
        node(&dialog, DialogTarget::AddressBar).value,
        Some(Value::Text("/docs".to_owned()))
    );
    let files = node(&dialog, DialogTarget::List);
    let names: Vec<&str> = files.children.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["pics", "a.txt"]);
    assert_eq!(files.children[0].description.as_deref(), Some("folder"));
    assert!(
        files.children[1]
            .description
            .as_deref()
            .is_some_and(|d| d.starts_with("10 B")),
        "{:?}",
        files.children[1].description
    );
    assert!(
        node(&dialog, DialogTarget::Header(SortColumn::Name))
            .description
            .is_some_and(|d| d.contains("ascending"))
    );
    let home = Query {
        role: Some(Role::ListItem),
        name: Some("home".to_owned()),
        ..Query::default()
    };
    assert_eq!(home.find_in(&tree), [DialogTarget::Shortcut(0)]);
}

/// **A file's box is where it is drawn and clicked**, and one scrolled out
/// of the list's well is below it, a row on from the last shown.
#[test]
fn a_files_box_is_where_it_is_clicked() {
    let mut dialog = FileDialog::open().with_initial_path("/docs");
    dialog.set_entries((0..30).map(|i| file(&format!("f{i:02}.txt"))).collect());
    let one = node(&dialog, DialogTarget::Entry(1)).bounds;
    let (x, y) = one.centre();
    dialog.handle_mouse(
        &MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        },
        W,
        H,
    );
    assert_eq!(dialog.selected_index(), Some(1));
    let list = node(&dialog, DialogTarget::List).bounds;
    let last = node(&dialog, DialogTarget::Entry(29)).bounds;
    assert!(last.y >= list.y + list.h, "{last:?} below {list:?}");
    let before = node(&dialog, DialogTarget::Entry(28)).bounds;
    assert!((last.y - before.y - one.h).abs() < 0.01, "a row on");
}

/// **Navigation as a user navigates**: Back and Forward refused with
/// nowhere to go, Up to the parent, a path set in the address gone to, a
/// place pressed -- each a navigation for the host to list.
#[test]
fn navigation_as_a_user_navigates() {
    let mut dialog = FileDialog::open().with_initial_path("/docs");
    let press = |dialog: &mut FileDialog, part| dialog.invoke(&part, Action::Press, W, H);
    assert_eq!(
        press(&mut dialog, DialogTarget::Back),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        press(&mut dialog, DialogTarget::Up),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/"))))
    );
    assert!(node(&dialog, DialogTarget::Back).enabled);
    assert_eq!(press(&mut dialog, DialogTarget::Up), Err(Refusal::Disabled));
    assert_eq!(
        press(&mut dialog, DialogTarget::Back),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/docs"))))
    );
    assert_eq!(
        press(&mut dialog, DialogTarget::Forward),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/"))))
    );
    assert_eq!(
        dialog.invoke(
            &DialogTarget::AddressBar,
            Action::SetText(" /srv/media ".to_owned()),
            W,
            H
        ),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/srv/media"))))
    );
    assert_eq!(
        press(&mut dialog, DialogTarget::Shortcut(0)),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/home/user"))))
    );
    assert_eq!(
        node(&dialog, DialogTarget::Shortcut(0)).value,
        Some(Value::Chosen(true)),
        "the place shown"
    );
    assert_eq!(
        press(&mut dialog, DialogTarget::Shortcut(0)),
        Ok(None),
        "already there: nothing to list"
    );
    assert_eq!(
        press(&mut dialog, DialogTarget::Shortcut(99)),
        Err(Refusal::NoSuchWidget)
    );
}

/// **Files chosen, opened and named as a user does**: a file chosen then
/// Open pressed is the file; a folder pressed is gone into; a Save
/// dialog's name set and Save pressed is that name here; Cancel cancels.
#[test]
fn files_chosen_opened_and_named() {
    let mut dialog = FileDialog::open().with_initial_path("/docs");
    dialog.set_entries(vec![folder("pics"), file("a.txt")]);
    assert_eq!(
        dialog.invoke(&DialogTarget::Confirm, Action::Press, W, H),
        Err(Refusal::Disabled),
        "nothing chosen"
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Entry(1), Action::Choose, W, H),
        Ok(None)
    );
    assert_eq!(
        node(&dialog, DialogTarget::Entry(1)).value,
        Some(Value::Chosen(true))
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Confirm, Action::Press, W, H),
        Ok(Some(DialogAction::Selected(PathBuf::from("/docs/a.txt"))))
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Entry(0), Action::Press, W, H),
        Ok(Some(DialogAction::NavigatedTo(PathBuf::from("/docs/pics"))))
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Entry(5), Action::Choose, W, H),
        Err(Refusal::NoSuchWidget),
        "the old listing's rows are gone with it"
    );

    let mut dialog = FileDialog::save().with_initial_path("/docs");
    assert_eq!(
        dialog.invoke(
            &DialogTarget::FilenameInput,
            Action::SetText("notes.txt".to_owned()),
            W,
            H
        ),
        Ok(None)
    );
    assert_eq!(
        node(&dialog, DialogTarget::FilenameInput).value,
        Some(Value::Text("notes.txt".to_owned()))
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Confirm, Action::Press, W, H),
        Ok(Some(DialogAction::Selected(PathBuf::from(
            "/docs/notes.txt"
        ))))
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Cancel, Action::Press, W, H),
        Ok(Some(DialogAction::Cancelled))
    );
    assert!(dialog.is_cancelled());
}

/// **A heading pressed sorts by it, and says so**; what is not a part, or
/// not a part of this dialog, is refused, as an action a part is not for.
#[test]
fn headings_sort_and_the_rest_refuse() {
    let mut dialog = FileDialog::open().with_initial_path("/docs");
    dialog.set_entries(vec![file("a.txt"), file("b.txt")]);
    assert_eq!(
        dialog.invoke(&DialogTarget::Header(SortColumn::Name), Action::Press, W, H),
        Ok(None)
    );
    assert!(
        node(&dialog, DialogTarget::Header(SortColumn::Name))
            .description
            .is_some_and(|d| d.contains("descending"))
    );
    assert_eq!(node(&dialog, DialogTarget::Entry(0)).name, "b.txt");
    assert_eq!(
        dialog.invoke(
            &DialogTarget::FilenameInput,
            Action::SetText("x".to_owned()),
            W,
            H
        ),
        Err(Refusal::NoSuchWidget),
        "an Open dialog has no name field"
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::ScrollThumb, Action::Press, W, H),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Up, Action::SetText("/".to_owned()), W, H),
        Err(Refusal::NotApplicable {
            role: Role::Button,
            action: "set the text of"
        })
    );
    assert_eq!(
        dialog.invoke(&DialogTarget::Places, Action::Press, W, H),
        Err(Refusal::NotApplicable {
            role: Role::List,
            action: "press"
        })
    );
}
