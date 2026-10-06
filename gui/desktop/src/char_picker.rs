//! The character picker over the shell's own text fields -- the Run box's
//! command line, the start menu's search, a note's writing area and an icon's
//! name being edited. [`charpicker::MENU_LABEL`] on a field's menu, or
//! [`charpicker::SHORTCUT_LABEL`] in the field, puts the one picker every
//! surface opens ([`charpicker::CharPicker`]) up beside the field, and what is
//! picked is typed into it.
//!
//! # Typed, not inserted
//!
//! A pick reaches its field as a key that types its text
//! ([`DesktopShell::type_into_field`]). Each field already does what follows
//! a typed change -- the Run box suggests, the start menu searches, a note is
//! saved with the layout, a name is kept when the rename ends -- and a second
//! way in would have to repeat each of those, and could drift from them. It
//! is also the path a pick will take into other programs' fields once the
//! window system can carry one there
//! (`requests/c-f-let-the-shell-type-into-the-focused-window.md`).
//!
//! # One pick, then gone
//!
//! The picker closes when a character is picked, as a menu does when a row
//! is chosen: it was asked for over a field the user was typing in, and the
//! typing goes on there. Escape, or a press anywhere outside it, closes it
//! with nothing typed.
//!
//! # What it learns
//!
//! The recent picks and the skin tone are handed back each time the picker
//! opens, and kept from one login to the next in `charpicker.yaml`, the file
//! every host of the picker shares ([`charpicker::Remembered`]): the session
//! reads it at the start ([`DesktopShell::load_char_picker`]) and writes it
//! when the picker comes down having learned something
//! ([`DesktopShell::save_char_picker`]).

use charpicker::{CharPicker, CharPickerEvent};
use guitk::event::{Key, KeyEvent, Modifiers, MouseEvent, MouseEventKind};
use guitk::menu::{MenuItem, MenuItemId};
use guitk::render::RenderTree;

use crate::{DesktopShell, HotkeyOutcome, MenuField, Palette, Rect, ShellAction};

/// The picker up over a field: the field it types into, and where it is.
pub(crate) struct FieldPicker {
    picker: CharPicker,
    field: MenuField,
    /// Its box on the screen.
    rect: Rect,
}

/// The id of a field menu's row that opens the picker: far from the edit
/// rows' (`guitk::editmenu`) and from the widget rows a note's menu adds.
pub(crate) const MENU_ROW: MenuItemId = 0xC4A2_0000_0000_0001;

/// A field menu's row that opens the picker, with the chord that does too.
pub(crate) fn menu_row() -> MenuItem {
    MenuItem::Action {
        id: MENU_ROW,
        label: charpicker::MENU_LABEL.to_string(),
        shortcut: Some(charpicker::SHORTCUT_LABEL.to_string()),
        icon: None,
        enabled: true,
        checked: None,
    }
}

/// Where a box `size` goes beside `field` on a `screen`-sized display: below
/// the field, or above it where there is no room below, and moved left as far
/// as it must be to stay on the screen. A box larger than the screen is
/// placed at its top-left corner, cut by the screen's edges rather than hung
/// off its top or left.
fn beside(field: Rect, size: (f32, f32), screen: (f32, f32)) -> Rect {
    let (w, h) = (size.0.min(screen.0).max(0.0), size.1.min(screen.1).max(0.0));
    let x = field.x.min(screen.0 - w).max(0.0);
    let below = field.bottom();
    let y = if below + h <= screen.1 {
        below
    } else if field.y - h >= 0.0 {
        field.y - h
    } else {
        (screen.1 - h).max(0.0)
    };
    Rect::new(x, y, w, h)
}

impl DesktopShell {
    /// Where `field` is on the screen, while it is up.
    fn field_rect_of(&self, field: MenuField) -> Option<Rect> {
        match field {
            MenuField::RunBox => self
                .run_dialog
                .is_visible()
                .then(|| self.run_dialog.field_rect()),
            MenuField::StartSearch => self.start_menu_open.then(|| self.start_search_rect()),
            MenuField::Note(note) => (self.widgets.writing_note() == Some(note))
                .then(|| self.widgets.writing_note_rect())
                .flatten(),
            MenuField::Rename => self
                .icons
                .rename_field()
                .map(|(x, y, w, h)| Rect::new(x, y, w, h)),
        }
    }

    /// Put the picker up beside `field`, if the field is up: the recent picks
    /// and the skin tone it learned last time, in the user's focus ring and
    /// caret.
    pub(crate) fn open_char_picker(&mut self, field: MenuField) {
        let Some(at) = self.field_rect_of(field) else {
            return;
        };
        #[allow(
            clippy::cast_precision_loss,
            reason = "a display's dimensions are exact in f32 for every size hardware produces"
        )]
        let screen = (self.screen_width as f32, self.screen_height as f32);
        let picker = CharPicker::new()
            .with_remembered(self.char_remembered.clone())
            .with_focus_ring(self.appearance.focus_ring_width())
            .with_caret_width(self.appearance.caret_width());
        self.char_picker = Some(FieldPicker {
            picker,
            field,
            rect: beside(at, CharPicker::preferred_size(), screen),
        });
    }

    /// Whether the picker is up.
    #[must_use]
    pub fn char_picker_open(&self) -> bool {
        self.char_picker.is_some()
    }

    /// Take the picker down, keeping what it learned for the next time --
    /// and for the next login, when it learned something.
    pub(crate) fn close_char_picker(&mut self) {
        if let Some(open) = self.char_picker.take() {
            let now = open.picker.remembered();
            if now != self.char_remembered {
                self.char_remembered = now;
                self.char_picker_dirty = true;
            }
        }
    }

    /// Read what the picker remembers from `charpicker.yaml`: for the
    /// session, at its start.
    pub fn load_char_picker(&mut self) {
        self.char_remembered = charpicker::Remembered::load();
    }

    /// Whether what the picker remembers needs writing, clearing the flag.
    pub fn take_char_picker_dirty(&mut self) -> bool {
        core::mem::take(&mut self.char_picker_dirty)
    }

    /// Write what the picker remembers to `charpicker.yaml`.
    ///
    /// # Errors
    ///
    /// The write's own error. What it remembers still holds for this
    /// session.
    pub fn save_char_picker(&self) -> std::io::Result<()> {
        self.char_remembered.save()
    }

    /// Ctrl+. in `field`: the picker, beside it. Answers whether `key` was
    /// the chord.
    pub(crate) fn char_picker_chord(&mut self, field: MenuField, key: &KeyEvent) -> bool {
        if !charpicker::is_shortcut(key) {
            return false;
        }
        self.open_char_picker(field);
        true
    }

    /// A key while the picker is up. Every key is the picker's: it is over
    /// the field, and a key it had no use for typed into the field under it
    /// would type into something the user cannot see.
    pub(crate) fn key_on_char_picker(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        let Some(open) = self.char_picker.as_mut() else {
            return HotkeyOutcome::default();
        };
        let event = open.picker.handle_key(key, open.rect.w, open.rect.h);
        // What a pick typed into its field asks nothing of the session: a
        // typed character launches no program and asks the compositor for
        // nothing, in any of the four fields. The key itself is spent.
        let _typed = self.char_picker_said(event);
        HotkeyOutcome::consumed()
    }

    /// A pointer event while the picker is up: inside it, the picker's; a
    /// press outside it closes it, and is spent doing so, as a menu's is.
    pub(crate) fn mouse_on_char_picker(&mut self, event: &MouseEvent) -> ShellAction {
        let Some(open) = self.char_picker.as_mut() else {
            return ShellAction::Pass;
        };
        let rect = open.rect;
        let outside = !rect.contains(event.x, event.y);
        if outside
            && matches!(
                event.kind,
                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_)
            )
        {
            self.close_char_picker();
            return ShellAction::Consumed;
        }
        let local = MouseEvent {
            x: event.x - rect.x,
            y: event.y - rect.y,
            kind: event.kind.clone(),
        };
        let said = open.picker.handle_mouse(&local, rect.w, rect.h);
        self.char_picker_said(said)
    }

    /// What the picker said: a pick is typed into its field, and a pick or
    /// Escape takes it down.
    fn char_picker_said(&mut self, event: Option<CharPickerEvent>) -> ShellAction {
        match event {
            Some(CharPickerEvent::Picked(text)) => {
                let field = self.char_picker.as_ref().map(|open| open.field);
                self.close_char_picker();
                match field {
                    Some(field) => self.type_into_field(field, &text),
                    None => ShellAction::Consumed,
                }
            }
            Some(CharPickerEvent::Cancelled) => {
                self.close_char_picker();
                ShellAction::Consumed
            }
            None => ShellAction::Consumed,
        }
    }

    /// Type `text` into `field` as a key that typed it would -- with all that
    /// follows a typed change there (the module's "Typed, not inserted").
    /// Nothing, for a field no longer up.
    pub(crate) fn type_into_field(&mut self, field: MenuField, text: &str) -> ShellAction {
        // No key of the keyboard's: a key that types and is nothing else, as
        // a key that came from no keyboard is.
        let key = KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_string(),
        };
        match field {
            MenuField::RunBox if self.run_dialog.is_visible() => {
                // Typing launches nothing; what the box suggests follows the
                // line, and the box redraws either way.
                let _outcome = self.key_on_run_dialog(&key);
                ShellAction::Consumed
            }
            MenuField::StartSearch if self.start_menu_open => {
                let _outcome = self.key_on_start_menu(&key);
                ShellAction::Consumed
            }
            MenuField::Note(note) if self.widgets.writing_note() == Some(note) => {
                self.handle_desktop_key(&key)
            }
            MenuField::Rename if self.icons.renaming().is_some() => self.handle_desktop_key(&key),
            MenuField::RunBox | MenuField::StartSearch | MenuField::Note(_) | MenuField::Rename => {
                ShellAction::Consumed
            }
        }
    }

    /// The picker's draw commands, `None` when it is down.
    #[must_use]
    pub fn render_char_picker(&self) -> Option<RenderTree> {
        let open = self.char_picker.as_ref()?;
        let mut tree = RenderTree::new();
        tree.translate(open.rect.x, open.rect.y);
        tree.commands.extend(open.picker.render(
            &Palette::from_settings(&self.appearance),
            open.rect.w,
            open.rect.h,
        ));
        tree.untranslate();
        Some(tree)
    }
}

#[cfg(test)]
mod tests;
