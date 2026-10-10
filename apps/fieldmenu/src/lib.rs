//! The right-click menu of a program's text fields.
//!
//! A right-click in a text box offers Cut, Copy, Paste, Delete and Select
//! all -- Undo and Redo too in a box with a history -- each dimmed when it
//! would do nothing (`requests/c-e-text-fields-have-a-right-click-menu.md`).
//! The toolkit gives the rows (`TextInput::edit_menu`, `TextArea::edit_menu`)
//! and does what a chosen row says (`edit_command`); what is left to each
//! window is the same handful of lines, and [`FieldMenu`] is those lines:
//! the menu and which field it is for, the presses, keys, pointer, wheel and
//! clock routed to it while it is up, and its drawing.
//!
//! ```ignore
//! // A right press over a field: its menu, where the press landed.
//! self.field_menu.open(Field::Search, self.search.edit_menu(), x, y, viewport);
//!
//! // Every mouse event and key, before the window's own handling:
//! match self.field_menu.mouse(&event) {
//!     Outcome::NotUp => {}                        // the window's, as ever
//!     Outcome::Taken => return Consumed,
//!     Outcome::Chosen(field, id) => {             // the field does it
//!         self.input(field).edit_command(id);
//!         return Consumed;
//!     }
//!     Outcome::RightPressElsewhere(x, y) => {     // another field's menu?
//!         self.open_field_menu(x, y);
//!         return Consumed;
//!     }
//! }
//! ```
//!
//! **Modal while up**, as any menu is: every press goes to it -- a row
//! chosen with the left button is done, a press outside puts it away and
//! reaches nothing under it -- and every key: the arrows move through its
//! rows, Enter does the lit one, Escape puts it away, and none reaches the
//! field. A press on a dimmed row leaves it up: nothing happened.

use guitk::editmenu::EditCommand;
use guitk::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::menu::{ContextMenu, MenuAction, MenuItem, MenuItemId};
use guitk::palette::Palette;
use guitk::render::RenderCommand;

/// What the menu made of an event. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome<F> {
    /// The menu is not up: the event is the window's.
    NotUp,
    /// The menu took the event; there is nothing more to do but draw.
    Taken,
    /// The row `id` was chosen for the field: have the field do it
    /// (`edit_command`), then whatever a typed change there brings.
    Chosen(F, MenuItemId),
    /// A right press outside the menu, which put it away: the window opens
    /// the menu of the field at `(x, y)`, if there is one there.
    RightPressElsewhere(f32, f32),
}

/// A window's text-field menu, and which of its fields it is up for. `F`
/// names a field the window's way: an enum of its fields, an index.
pub struct FieldMenu<F> {
    up: Option<(ContextMenu, F)>,
}

/// The field the menu is up for, if it is: the toolkit's menu prints nothing
/// of itself.
impl<F: std::fmt::Debug> std::fmt::Debug for FieldMenu<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldMenu")
            .field("up_for", &self.up.as_ref().map(|(_, field)| field))
            .finish()
    }
}

impl<F> Default for FieldMenu<F> {
    fn default() -> Self {
        Self { up: None }
    }
}

impl<F: Copy> FieldMenu<F> {
    /// No menu up.
    #[must_use]
    pub const fn new() -> Self {
        Self { up: None }
    }

    /// Whether the menu is up.
    #[must_use]
    pub fn is_up(&self) -> bool {
        self.up.is_some()
    }

    /// The field the menu is up for.
    #[must_use]
    pub fn field(&self) -> Option<F> {
        self.up.as_ref().map(|(_, field)| *field)
    }

    /// The menu, while it is up: to ask which rows it holds, or where one
    /// is drawn.
    #[must_use]
    pub fn menu(&self) -> Option<&ContextMenu> {
        self.up.as_ref().map(|(menu, _)| menu)
    }

    /// The index of the row doing `command`, while the menu is up.
    fn row_of(&self, command: EditCommand) -> Option<usize> {
        self.menu()?
            .items()
            .iter()
            .position(|item| matches!(item, MenuItem::Action { id, .. } if *id == command.id()))
    }

    /// The middle of the row doing `command`, while the menu is up -- where a
    /// press chooses it, as a program's tests press it.
    #[must_use]
    pub fn row_middle(&self, command: EditCommand) -> Option<(f32, f32)> {
        let rect = self.menu()?.item_rect(self.row_of(command)?)?;
        Some((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0))
    }

    /// Whether the row doing `command` is lit -- it would do something --
    /// rather than dimmed, while the menu is up.
    #[must_use]
    pub fn row_lit(&self, command: EditCommand) -> Option<bool> {
        match self.menu()?.items().get(self.row_of(command)?)? {
            MenuItem::Action { enabled, .. } => Some(*enabled),
            MenuItem::Separator | MenuItem::Submenu { .. } => None,
        }
    }

    /// Put `menu` -- the field's `edit_menu`, with any rows of the window's
    /// own after it -- up for `field`, at `(x, y)` within `viewport`.
    pub fn open(&mut self, field: F, mut menu: ContextMenu, x: f32, y: f32, viewport: (f32, f32)) {
        menu.show(x, y, viewport);
        self.up = Some((menu, field));
    }

    /// Put the menu away.
    pub fn close(&mut self) {
        self.up = None;
    }

    /// A mouse event. See [`Outcome`] and the module docs.
    pub fn mouse(&mut self, event: &MouseEvent) -> Outcome<F> {
        let Some((menu, field)) = self.up.as_mut() else {
            return Outcome::NotUp;
        };
        let field = *field;
        match event.kind {
            MouseEventKind::Press(button) | MouseEventKind::DoubleClick(button) => {
                let inside = menu
                    .panel_rect()
                    .is_some_and(|panel| panel.contains(event.x, event.y));
                match button {
                    MouseButton::Left => match menu.handle_click(event.x, event.y) {
                        Some(id) => {
                            self.up = None;
                            Outcome::Chosen(field, id)
                        }
                        None => {
                            // On a dimmed row or a separator the menu stays
                            // up; anywhere else it has hidden itself.
                            if !menu.is_visible() {
                                self.up = None;
                            }
                            Outcome::Taken
                        }
                    },
                    _ if inside => Outcome::Taken,
                    MouseButton::Right => {
                        self.up = None;
                        Outcome::RightPressElsewhere(event.x, event.y)
                    }
                    _ => {
                        self.up = None;
                        Outcome::Taken
                    }
                }
            }
            MouseEventKind::Move => {
                menu.handle_mouse_move(event.x, event.y);
                Outcome::Taken
            }
            MouseEventKind::Scroll { dy, .. } => {
                menu.handle_scroll(event.x, event.y, dy);
                Outcome::Taken
            }
            _ => Outcome::Taken,
        }
    }

    /// A key. Every key is the menu's while it is up; see [`Outcome`].
    pub fn key(&mut self, key: &KeyEvent) -> Outcome<F> {
        let Some((menu, field)) = self.up.as_mut() else {
            return Outcome::NotUp;
        };
        let field = *field;
        match menu.handle_key(key) {
            Some(MenuAction::Selected(id)) => {
                self.up = None;
                Outcome::Chosen(field, id)
            }
            Some(MenuAction::Closed) => {
                self.up = None;
                Outcome::Taken
            }
            Some(MenuAction::None) | None => Outcome::Taken,
        }
    }

    /// The window's clock reads `now_ms`: a dimmed row the pointer rests on
    /// says why when its wait is over. Whether that changed what is drawn.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.up.as_mut().is_some_and(|(menu, _)| menu.tick(now_ms))
    }

    /// How long until the menu wants a tick, if it does: at least a
    /// millisecond, so a window's clock moves to start the wait.
    #[must_use]
    pub fn due_in(&self, now_ms: u64) -> Option<u64> {
        self.up
            .as_ref()
            .and_then(|(menu, _)| menu.due_in(now_ms))
            .map(|ms| ms.max(1))
    }

    /// The menu drawn, over everything else in the window: nothing while it
    /// is not up.
    #[must_use]
    pub fn render(&self, palette: &Palette) -> Vec<RenderCommand> {
        self.up
            .as_ref()
            .map_or_else(Vec::new, |(menu, _)| menu.render(palette))
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests;
