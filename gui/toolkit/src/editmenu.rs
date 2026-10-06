//! The menu a right-click on a text field offers: Undo and Redo where the
//! field keeps a history, Cut, Copy, Paste, Delete and Select all -- the
//! commands its keys give (Ctrl+Z, Ctrl+X, Ctrl+C, Ctrl+V, Delete, Ctrl+A),
//! for a user who reaches for the pointer. Each row is there always, and
//! dimmed when it would do nothing, so the menu has one shape to learn.
//!
//! The toolkit's fields draw nothing and hold no menu, so this gives the
//! menu ([`menu`]: the rows of [`rows`], each dimmed one saying why while
//! the pointer rests on it, [`why_dimmed`]) and the fields do what a row
//! chosen says
//! ([`TextInput::edit_command`](crate::textinput::TextInput::edit_command),
//! [`TextArea::edit_command`](crate::textarea::TextArea::edit_command)); the
//! window that draws a field puts the menu up where the right-click landed,
//! and lets time pass for it ([`ContextMenu::tick`]) for a reason to appear.
//!
//! The rows' ids are far up the id space ([`EditCommand::id`]), so that they
//! can sit in a menu beside a window's own rows -- a field's menu with a
//! "Paste and go" of the window's below them -- without either side
//! renumbering.

use crate::menu::{ContextMenu, MenuItem, MenuItemId};

/// What a row of a field's menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    /// Take back the last change (a field with a history).
    Undo,
    /// Do again what Undo took back.
    Redo,
    /// Put the selection on the clipboard and delete it.
    Cut,
    /// Put the selection on the clipboard.
    Copy,
    /// Put the clipboard in place of the selection.
    Paste,
    /// Delete the selection.
    Delete,
    /// Select the whole text.
    SelectAll,
}

/// Where the rows' ids start: high enough that a window's own ids, counted
/// from nothing, do not reach them.
const BASE: MenuItemId = 0xED17_0000_0000_0000;

impl EditCommand {
    /// Every command, in the menu's order.
    pub const ALL: [Self; 7] = [
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Delete,
        Self::SelectAll,
    ];

    /// The id its row carries.
    #[must_use]
    pub const fn id(self) -> MenuItemId {
        BASE | self as MenuItemId
    }

    /// The command a row's id names, if it is one of these.
    #[must_use]
    pub fn from_id(id: MenuItemId) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Its row's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Delete => "Delete",
            Self::SelectAll => "Select all",
        }
    }

    /// The keys that do the same, shown beside the name.
    #[must_use]
    pub const fn keys(self) -> &'static str {
        match self {
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Y",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::Delete => "Del",
            Self::SelectAll => "Ctrl+A",
        }
    }
}

/// What a field is, for its menu: which rows can do something.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditState {
    /// Whether anything is selected.
    pub selected: bool,
    /// Whether the text can be changed -- a read-only field offers Copy and
    /// Select all alone.
    pub editable: bool,
    /// Whether there is any text to select.
    pub has_text: bool,
    /// Whether the field keeps a history, and so offers Undo and Redo at all.
    pub history: bool,
    /// Whether there is anything to undo.
    pub can_undo: bool,
    /// Whether there is anything to redo.
    pub can_redo: bool,
    /// Whether Cut and Copy with nothing selected take the caret's whole
    /// line, as a code editor's keys do -- so they are lit whenever there is
    /// text, selected or not.
    pub copies_line: bool,
    /// Whether a picture can be pasted in -- a rich field's text holds
    /// pictures -- so that Paste is lit for a picture copied alone.
    pub takes_pictures: bool,
}

/// Why the row for `command` is dimmed in a field in `state` -- what its
/// menu says while the pointer rests on it -- or `None` when it is not
/// dimmed: when it would do something now. The one place that decides, so
/// [`rows`] dims exactly the rows this explains.
///
/// Undo and Redo with no history are not dimmed but absent ([`rows`] leaves
/// them out), and so have nothing to explain. Paste asks the program's
/// clipboard ([`crate::clipboard`]).
#[must_use]
pub fn why_dimmed(command: EditCommand, state: EditState) -> Option<&'static str> {
    const READ_ONLY: &str = "This text cannot be changed";
    const SELECT: &str = "Select some text first";
    // What Cut and Copy would take: the selection, or the caret's line.
    let takes = state.selected || (state.copies_line && state.has_text);
    match command {
        EditCommand::Undo | EditCommand::Redo if !state.history => None,
        EditCommand::Undo
        | EditCommand::Redo
        | EditCommand::Cut
        | EditCommand::Paste
        | EditCommand::Delete
            if !state.editable =>
        {
            Some(READ_ONLY)
        }
        EditCommand::Undo => (!state.can_undo).then_some("There is nothing to undo"),
        EditCommand::Redo => (!state.can_redo).then_some("There is nothing to redo"),
        EditCommand::Cut | EditCommand::Copy => (!takes).then_some(SELECT),
        EditCommand::Paste => why_no_paste(state.takes_pictures),
        EditCommand::Delete => (!state.selected).then_some(SELECT),
        EditCommand::SelectAll => (!state.has_text).then_some("There is no text"),
    }
}

/// What a paste into a field would find nothing to take in, if so: nothing
/// copied, or -- where the field takes text alone (`pictures` false) -- a
/// picture copied with no text, which it cannot hold. `None` when the
/// clipboard has something the field can take.
#[must_use]
pub fn why_no_paste(pictures: bool) -> Option<&'static str> {
    if crate::clipboard::has_text() || (pictures && crate::clipboard::has_picture()) {
        None
    } else if crate::clipboard::has_picture() {
        Some(PICTURE_ONLY)
    } else {
        Some(NOTHING_COPIED)
    }
}

/// Why Paste is dimmed with nothing on the clipboard.
pub const NOTHING_COPIED: &str = "Nothing has been copied";

/// Why Paste is dimmed in a field that takes text when a picture alone was
/// copied.
pub const PICTURE_ONLY: &str = "What was copied is a picture, not text";

/// The rows of a field's menu: Undo and Redo where it keeps a history, then
/// Cut, Copy, Paste and Delete, then Select all -- each dimmed when it would
/// do nothing now ([`why_dimmed`]).
#[must_use]
pub fn rows(state: EditState) -> Vec<MenuItem> {
    let row = |command: EditCommand| MenuItem::Action {
        id: command.id(),
        label: command.label().to_owned(),
        shortcut: Some(command.keys().to_owned()),
        icon: None,
        enabled: why_dimmed(command, state).is_none(),
        checked: None,
    };
    let mut rows = Vec::new();
    if state.history {
        rows.push(row(EditCommand::Undo));
        rows.push(row(EditCommand::Redo));
        rows.push(MenuItem::Separator);
    }
    rows.push(row(EditCommand::Cut));
    rows.push(row(EditCommand::Copy));
    rows.push(row(EditCommand::Paste));
    rows.push(row(EditCommand::Delete));
    rows.push(MenuItem::Separator);
    rows.push(row(EditCommand::SelectAll));
    rows
}

/// The menu a right-click on a field in `state` puts up: its [`rows`], each
/// dimmed one saying why while the pointer rests on it ([`why_dimmed`],
/// [`ContextMenu::explain`]). The window shows it where the click landed,
/// and lets time pass for it ([`ContextMenu::tick`]) for the reasons to
/// appear.
#[must_use]
pub fn menu(state: EditState) -> ContextMenu {
    let mut menu = ContextMenu::new(rows(state));
    for command in EditCommand::ALL {
        if let Some(why) = why_dimmed(command, state) {
            menu.explain(command.id(), why);
        }
    }
    menu
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    fn enabled(rows: &[MenuItem]) -> Vec<(&str, bool)> {
        rows.iter()
            .filter_map(|r| match r {
                MenuItem::Action { label, enabled, .. } => Some((label.as_str(), *enabled)),
                _ => None,
            })
            .collect()
    }

    /// **Every row is there, dimmed when it would do nothing**: nothing
    /// selected and an empty clipboard leave Select all alone lit.
    #[test]
    fn each_row_is_lit_only_when_it_can_act() {
        crate::clipboard::set_text("");
        let state = EditState {
            editable: true,
            has_text: true,
            ..EditState::default()
        };
        assert_eq!(
            enabled(&rows(state)),
            [
                ("Cut", false),
                ("Copy", false),
                ("Paste", false),
                ("Delete", false),
                ("Select all", true),
            ]
        );
        crate::clipboard::set_text("x");
        let selected = EditState {
            selected: true,
            ..state
        };
        assert!(enabled(&rows(selected)).iter().all(|(_, on)| *on));
    }

    /// **A read-only field offers Copy and Select all alone**, whatever is
    /// on the clipboard.
    #[test]
    fn a_read_only_field_offers_copy_and_select_all() {
        crate::clipboard::set_text("x");
        let state = EditState {
            selected: true,
            has_text: true,
            ..EditState::default()
        };
        let offered = rows(state);
        let lit: Vec<&str> = enabled(&offered)
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(l, _)| l)
            .collect();
        assert_eq!(lit, ["Copy", "Select all"]);
    }

    /// **Undo and Redo come first where the field keeps a history**, lit when
    /// there is something to take back or do again.
    #[test]
    fn a_field_with_a_history_offers_undo_and_redo() {
        let state = EditState {
            editable: true,
            history: true,
            can_undo: true,
            ..EditState::default()
        };
        let rows = rows(state);
        assert_eq!(enabled(&rows)[..2], [("Undo", true), ("Redo", false)]);
        assert!(matches!(rows[2], MenuItem::Separator));
        assert!(
            !enabled(&super::rows(EditState::default()))
                .iter()
                .any(|(l, _)| *l == "Undo"),
            "a field with no history offered Undo"
        );
    }

    /// **A field whose Cut and Copy take the caret's line offers them with
    /// nothing selected** -- while there is text to take; Delete still wants
    /// a selection.
    #[test]
    fn a_field_that_copies_the_line_offers_cut_and_copy_unselected() {
        let state = EditState {
            editable: true,
            has_text: true,
            copies_line: true,
            ..EditState::default()
        };
        let offered = rows(state);
        let lit: Vec<(&str, bool)> = enabled(&offered)
            .into_iter()
            .filter(|(l, _)| matches!(*l, "Cut" | "Copy" | "Delete"))
            .collect();
        assert_eq!(lit, [("Cut", true), ("Copy", true), ("Delete", false)]);
        let empty = rows(EditState {
            has_text: false,
            ..state
        });
        assert!(
            enabled(&empty)
                .iter()
                .filter(|(l, _)| matches!(*l, "Cut" | "Copy"))
                .all(|(_, on)| !on),
            "an empty field offered a line to cut"
        );
        let read_only = rows(EditState {
            editable: false,
            ..state
        });
        assert_eq!(
            enabled(&read_only)
                .into_iter()
                .filter(|(l, _)| matches!(*l, "Cut" | "Copy"))
                .collect::<Vec<_>>(),
            [("Cut", false), ("Copy", true)]
        );
    }

    /// **Each row shows the keys that do the same**, so that the menu
    /// teaches them, in the menu's order.
    #[test]
    fn each_row_shows_its_keys() {
        let state = EditState {
            history: true,
            ..EditState::default()
        };
        let shown: Vec<(String, Option<String>)> = rows(state)
            .into_iter()
            .filter_map(|r| match r {
                MenuItem::Action {
                    label, shortcut, ..
                } => Some((label, shortcut)),
                _ => None,
            })
            .collect();
        let want = [
            ("Undo", "Ctrl+Z"),
            ("Redo", "Ctrl+Y"),
            ("Cut", "Ctrl+X"),
            ("Copy", "Ctrl+C"),
            ("Paste", "Ctrl+V"),
            ("Delete", "Del"),
            ("Select all", "Ctrl+A"),
        ]
        .map(|(label, keys)| (label.to_owned(), Some(keys.to_owned())));
        assert_eq!(shown, want);
    }

    /// **A row's id says which command it is, and no other id does** -- not
    /// a window's own, counted from nothing.
    #[test]
    fn a_rows_id_names_its_command() {
        for command in EditCommand::ALL {
            assert_eq!(EditCommand::from_id(command.id()), Some(command));
        }
        let ids: std::collections::BTreeSet<MenuItemId> =
            EditCommand::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), EditCommand::ALL.len());
        for id in [0, 1, 100, 10_000, u64::from(u32::MAX)] {
            assert_eq!(EditCommand::from_id(id), None);
        }
    }

    /// **Each dimmed row says why, and only a dimmed row does** -- one
    /// decision, so the menu cannot dim a row it does not explain, nor
    /// explain one it does not dim -- in words a user acts on.
    #[test]
    fn each_dimmed_row_says_why() {
        crate::clipboard::set_text("");
        let empty = EditState {
            editable: true,
            history: true,
            ..EditState::default()
        };
        let read_only = EditState {
            selected: true,
            has_text: true,
            history: true,
            can_undo: true,
            ..EditState::default()
        };
        let busy = EditState {
            editable: true,
            selected: true,
            has_text: true,
            history: true,
            can_undo: true,
            can_redo: true,
            ..EditState::default()
        };
        for state in [empty, read_only, busy] {
            for row in rows(state) {
                if let MenuItem::Action {
                    id, label, enabled, ..
                } = row
                {
                    let command = EditCommand::from_id(id).unwrap();
                    assert_eq!(
                        why_dimmed(command, state).is_none(),
                        enabled,
                        "{label} in {state:?}"
                    );
                }
            }
        }
        let said = |command, state| why_dimmed(command, state);
        assert_eq!(
            said(EditCommand::Undo, empty),
            Some("There is nothing to undo")
        );
        assert_eq!(
            said(EditCommand::Redo, empty),
            Some("There is nothing to redo")
        );
        assert_eq!(
            said(EditCommand::Cut, empty),
            Some("Select some text first")
        );
        assert_eq!(
            said(EditCommand::Copy, empty),
            Some("Select some text first")
        );
        assert_eq!(
            said(EditCommand::Paste, empty),
            Some("Nothing has been copied")
        );
        assert_eq!(
            said(EditCommand::Delete, empty),
            Some("Select some text first")
        );
        assert_eq!(
            said(EditCommand::SelectAll, empty),
            Some("There is no text")
        );
        for command in [
            EditCommand::Undo,
            EditCommand::Cut,
            EditCommand::Paste,
            EditCommand::Delete,
        ] {
            assert_eq!(
                said(command, read_only),
                Some("This text cannot be changed"),
                "{command:?}"
            );
        }
        assert_eq!(
            said(EditCommand::Copy, read_only),
            None,
            "copying is not changing"
        );
        assert_eq!(
            said(EditCommand::Undo, EditState::default()),
            None,
            "no history, so no row to explain"
        );
    }

    /// **The menu a field puts up explains its dimmed rows**, and nothing
    /// else.
    #[test]
    fn the_menu_explains_its_dimmed_rows() {
        crate::clipboard::set_text("");
        let state = EditState {
            editable: true,
            has_text: true,
            ..EditState::default()
        };
        let menu = menu(state);
        assert_eq!(menu.items().len(), rows(state).len());
        assert_eq!(
            menu.reason(EditCommand::Paste.id()),
            Some("Nothing has been copied")
        );
        assert_eq!(
            menu.reason(EditCommand::Cut.id()),
            Some("Select some text first")
        );
        assert_eq!(menu.reason(EditCommand::SelectAll.id()), None, "a lit row");
        assert_eq!(menu.reason(EditCommand::Undo.id()), None, "no history");
    }

    /// **A picture copied alone is pasted only where a picture can go**: a
    /// field of text dims Paste and says what was copied, a rich field
    /// lights it; text copied with the picture lights it everywhere.
    #[test]
    fn a_picture_alone_is_pasted_only_where_pictures_go() {
        let picture = crate::picture::Picture::new(imagecodec::Image {
            width: 1,
            height: 1,
            pixels: vec![0xFF00_0000],
        })
        .unwrap();
        crate::clipboard::set_picture(picture.clone());
        let text_only = EditState {
            editable: true,
            ..EditState::default()
        };
        let rich = EditState {
            takes_pictures: true,
            ..text_only
        };
        assert_eq!(
            why_dimmed(EditCommand::Paste, text_only),
            Some(PICTURE_ONLY)
        );
        assert_eq!(why_dimmed(EditCommand::Paste, rich), None);
        crate::clipboard::set("and words", Some(picture));
        assert_eq!(why_dimmed(EditCommand::Paste, text_only), None);
        crate::clipboard::set_text("");
        assert_eq!(why_dimmed(EditCommand::Paste, rich), Some(NOTHING_COPIED));
    }
}
