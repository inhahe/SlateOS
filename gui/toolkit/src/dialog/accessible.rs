//! The file dialog as tools see it -- a screen reader, a script
//! ([`Accessible`]): a dialog holding Back, Forward and Up, the address, the
//! places in the sidebar, the column headings that sort, the files of the
//! folder shown, the name field of a Save dialog, and its Open, Save or
//! Select button and Cancel. Each is a part a tool can find by what it is
//! called and act on as its user would, answering the [`DialogAction`] a
//! click on the same part answers -- a navigation is the host's to list, as
//! after a click.
//!
//! The boxes are the ones [`FileDialog::frame`] draws and hit-tests with. A
//! file scrolled out of the list's well has its box above or below the well,
//! where it would be; a tool acting on it acts on the file, not on the
//! pixel.

use std::path::Path;

use crate::frame::Rect;
use crate::palette::Palette;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

use super::{
    DialogAction, DialogMode, DialogTarget, FileDialog, ROW_HEIGHT, SortColumn, format_size,
    format_timestamp, parent_path, scaled,
};

/// What tools call a column's heading.
const fn heading(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Name => "Name",
        SortColumn::Size => "Size",
        SortColumn::Modified => "Modified",
    }
}

/// An action's answer as a tool hears it: nothing to report, or the
/// dialog's own.
fn answered(action: DialogAction) -> Option<DialogAction> {
    (action != DialogAction::None).then_some(action)
}

impl FileDialog {
    /// What the dialog is, to a tool: its confirming button's word.
    const fn purpose(&self) -> &'static str {
        match self.mode {
            DialogMode::Open => "Open",
            DialogMode::Save => "Save",
            DialogMode::SelectFolder => "Select a folder",
        }
    }

    /// Whether Up has a folder to go to.
    fn has_parent(&self) -> bool {
        parent_path(self.current_path.as_os_str()) != self.current_path.as_os_str()
    }

    /// Where row `index` of the listing is, from where the frame drew the
    /// rows it drew -- above or below the well for one scrolled out of it.
    fn row_box(frame_rows: &[(usize, Rect)], index: usize, list: Rect) -> Rect {
        let Some(&(drawn, at)) = frame_rows.first() else {
            return Rect::new(list.x, list.y, list.w, 0.0);
        };
        let rows = count(index.abs_diff(drawn)) * scaled(ROW_HEIGHT);
        let offset = if index >= drawn { rows } else { -rows };
        Rect::new(at.x, at.y + offset, at.w, at.h)
    }
}

/// A count of rows as a length.
#[allow(
    clippy::cast_precision_loss,
    reason = "a listing's length, far below where f32 loses whole numbers"
)]
fn count(n: usize) -> f32 {
    n as f32
}

impl Accessible for FileDialog {
    type Part = DialogTarget;
    type Event = DialogAction;

    fn automation(&self, width: f32, height: f32) -> Node<DialogTarget> {
        // The boxes the dialog draws and is clicked in; any palette gives the
        // same ones, as `handle_mouse` says.
        let frame = self.frame(&Palette::for_mode(false), width, height);
        let at = |target: DialogTarget| {
            frame
                .rect_of(|t| *t == target)
                .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0))
        };
        let button = |target, name: &str, enabled| {
            let mut node = Node::new(target, Role::Button, name, at(target));
            node.enabled = enabled;
            node
        };

        let mut children = vec![
            button(DialogTarget::Back, "Back", !self.history_back.is_empty()),
            button(
                DialogTarget::Forward,
                "Forward",
                !self.history_forward.is_empty(),
            ),
            button(DialogTarget::Up, "Up", self.has_parent()),
        ];

        let mut address = Node::new(
            DialogTarget::AddressBar,
            Role::TextField,
            "Address",
            at(DialogTarget::AddressBar),
        );
        address.value = Some(Value::Text(self.address.typed_text().map_or_else(
            || pathcodec::display_path(&self.current_path),
            str::to_owned,
        )));
        address.focused = self.address.is_editing();
        address.focusable = true;
        children.push(address);

        let mut places = Node::new(
            DialogTarget::Places,
            Role::List,
            "Places",
            at(DialogTarget::Places),
        );
        for (i, place) in self.quick_access.iter().enumerate() {
            let mut item = Node::new(
                DialogTarget::Shortcut(i),
                Role::ListItem,
                place.label.clone(),
                at(DialogTarget::Shortcut(i)),
            );
            item.description = Some(pathcodec::display_path(&place.path));
            item.value = Some(Value::Chosen(place.path == self.current_path));
            places.children.push(item);
        }
        children.push(places);

        for column in [SortColumn::Name, SortColumn::Size, SortColumn::Modified] {
            let mut node = button(DialogTarget::Header(column), heading(column), true);
            if column == self.sort_by {
                node.description = Some(
                    if self.sort_ascending {
                        "sorted, ascending"
                    } else {
                        "sorted, descending"
                    }
                    .to_owned(),
                );
            }
            children.push(node);
        }

        let list_box = at(DialogTarget::List);
        let drawn: Vec<(usize, Rect)> = frame
            .hits()
            .iter()
            .filter_map(|(target, rect)| match target {
                DialogTarget::Entry(i) => Some((*i, *rect)),
                _ => None,
            })
            .collect();
        let mut files = Node::new(DialogTarget::List, Role::List, "Files", list_box);
        files.focusable = true;
        for (i, entry) in self.entries.iter().enumerate() {
            let mut item = Node::new(
                DialogTarget::Entry(i),
                Role::ListItem,
                pathcodec::display_os(&entry.name),
                Self::row_box(&drawn, i, list_box),
            );
            item.description = Some(if entry.is_dir {
                "folder".to_owned()
            } else {
                format!(
                    "{}, modified {}",
                    format_size(entry.size),
                    format_timestamp(entry.modified_timestamp, &self.timezone)
                )
            });
            item.value = Some(Value::Chosen(self.selected_index == Some(i)));
            item.focused = self.selected_index == Some(i);
            item.focusable = true;
            files.children.push(item);
        }
        children.push(files);

        if self.mode == DialogMode::Save {
            let mut name = Node::new(
                DialogTarget::FilenameInput,
                Role::TextField,
                "File name",
                at(DialogTarget::FilenameInput),
            );
            name.value = Some(Value::Text(self.filename_input.clone()));
            name.focused = !self.address.is_editing();
            name.focusable = true;
            children.push(name);
        }
        children.push(button(
            DialogTarget::Confirm,
            match self.mode {
                DialogMode::Open => "Open",
                DialogMode::Save => "Save",
                DialogMode::SelectFolder => "Select",
            },
            self.confirm().is_some(),
        ));
        children.push(button(DialogTarget::Cancel, "Cancel", true));

        let mut root = Node::new(
            DialogTarget::Chrome,
            Role::Dialog,
            self.purpose(),
            Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0)),
        );
        root.description = Some(pathcodec::display_path(&self.current_path));
        root.children = children;
        root
    }

    fn invoke(
        &mut self,
        part: &DialogTarget,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DialogAction>, Refusal> {
        let asked = action.name();
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: asked,
        };
        match (*part, action) {
            (DialogTarget::Back, Action::Press) => {
                if self.history_back.is_empty() {
                    return Err(Refusal::Disabled);
                }
                Ok(answered(self.navigated(Self::navigate_back)))
            }
            (DialogTarget::Forward, Action::Press) => {
                if self.history_forward.is_empty() {
                    return Err(Refusal::Disabled);
                }
                Ok(answered(self.navigated(Self::navigate_forward)))
            }
            (DialogTarget::Up, Action::Press) => {
                if !self.has_parent() {
                    return Err(Refusal::Disabled);
                }
                Ok(answered(self.navigated(Self::navigate_up)))
            }
            (DialogTarget::AddressBar, Action::SetText(text)) => {
                // A path typed and Enter pressed: gone to, as the bar goes,
                // the host listing it -- or refusing, which goes back.
                let path = Path::new(text.trim());
                Ok(answered(self.navigated(|dialog| dialog.navigate_to(path))))
            }
            (DialogTarget::Shortcut(i), Action::Press | Action::Choose) => {
                let path = self
                    .quick_access
                    .get(i)
                    .map(|place| place.path.clone())
                    .ok_or(Refusal::NoSuchWidget)?;
                Ok(answered(self.navigated(|dialog| dialog.navigate_to(&path))))
            }
            (DialogTarget::Header(column), Action::Press) => {
                self.toggle_sort(column);
                Ok(None)
            }
            (DialogTarget::Entry(i), Action::Choose | Action::Focus) => {
                if i >= self.entries.len() {
                    return Err(Refusal::NoSuchWidget);
                }
                self.select_entry(i);
                Ok(None)
            }
            (DialogTarget::Entry(i), Action::Press) => {
                // A double-click: chosen and opened -- a folder gone into, a
                // file opened, or a Save dialog's name filled.
                if i >= self.entries.len() {
                    return Err(Refusal::NoSuchWidget);
                }
                self.select_entry(i);
                Ok(answered(self.activate_entry(i)))
            }
            (DialogTarget::FilenameInput, Action::SetText(text)) => {
                if self.mode != DialogMode::Save {
                    return Err(Refusal::NoSuchWidget);
                }
                self.filename_input = text;
                self.edited_filename();
                Ok(None)
            }
            (DialogTarget::Confirm, Action::Press) => self
                .confirm()
                .map(|path| Some(DialogAction::Selected(path)))
                .ok_or(Refusal::Disabled),
            (DialogTarget::Cancel, Action::Press) => {
                self.cancel();
                Ok(Some(DialogAction::Cancelled))
            }
            (DialogTarget::Back | DialogTarget::Forward | DialogTarget::Up, _)
            | (DialogTarget::Header(_) | DialogTarget::Confirm | DialogTarget::Cancel, _) => {
                Err(not_for(Role::Button))
            }
            (DialogTarget::AddressBar | DialogTarget::FilenameInput, _) => {
                Err(not_for(Role::TextField))
            }
            (DialogTarget::Shortcut(_) | DialogTarget::Entry(_), _) => Err(not_for(Role::ListItem)),
            (DialogTarget::List | DialogTarget::Places, _) => Err(not_for(Role::List)),
            (DialogTarget::Chrome, _) => Err(not_for(Role::Dialog)),
            // Not parts a tool sees: the scrollbar is the list's, and the
            // completions are the address's.
            (
                DialogTarget::ScrollTrack
                | DialogTarget::ScrollThumb
                | DialogTarget::AddressCompletions,
                _,
            ) => Err(Refusal::NoSuchWidget),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
