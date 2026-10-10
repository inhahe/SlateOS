//! A password field's state: what has been typed, kept as a [`Secret`], and
//! the caret.
//!
//! Not [`TextInput`](crate::textinput::TextInput) with a flag, though it
//! looks like one. A field for ordinary text copies its text to tell whether
//! a key changed it, grows its buffer as it is typed into, offers Cut and
//! Copy, and prints itself with `{:?}` -- four ways a password would be left
//! behind in memory or put where it can be read. This one keeps what is
//! typed in a [`Secret`] with its room reserved up front -- a keystroke that
//! would need more is refused, so what was typed is never moved -- offers no
//! way to copy or cut, and prints as `SecretInput { .. }`.
//!
//! It is drawn as one [`DOT`] per character, so its caret moves through dots
//! and has no direction of text to mind. Selection is all or nothing:
//! Ctrl+A selects what was typed, and the next key typed, deleted or pasted
//! replaces it -- the only selection anyone makes in a field whose
//! characters they cannot see.
//!
//! State only, as `TextInput` is: the caller draws the well, the dots and
//! the caret, and decides what Enter, Escape and Tab mean.
//!
//! Held by the credential service's prompt (`gui/credentials`); the lock
//! screen's and the password manager's fields are the next it is for.

use core::fmt;

use secret::Secret;

use crate::event::{Key, KeyEvent};
use crate::textinput::KeyEdit;

/// How many bytes a field holds unless its owner says otherwise: a passphrase
/// of a few hundred characters, in any script.
pub const SECRET_ROOM: usize = 1024;

/// What each typed character is drawn as.
pub const DOT: char = '\u{2022}';

/// A password field's state.
pub struct SecretInput {
    /// What was typed: always whole UTF-8 characters.
    typed: Secret,
    /// The caret, as a byte offset into `typed` on a character boundary.
    caret: usize,
    /// Whether everything typed is selected.
    all_selected: bool,
}

impl Default for SecretInput {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInput { .. }")
    }
}

impl SecretInput {
    /// An empty field with room for [`SECRET_ROOM`] bytes.
    #[must_use]
    pub fn new() -> Self {
        Self::with_room(SECRET_ROOM)
    }

    /// An empty field with room for `room` bytes of typing.
    #[must_use]
    pub fn with_room(room: usize) -> Self {
        Self {
            typed: Secret::with_room(room),
            caret: 0,
            all_selected: false,
        }
    }

    /// What was typed, as text.
    fn text(&self) -> &str {
        // Never empty for being unreadable: only whole characters go in, and
        // only whole characters come out, so the bytes are always UTF-8.
        std::str::from_utf8(self.typed.as_bytes()).unwrap_or_default()
    }

    /// How many characters were typed: how many dots to draw.
    #[must_use]
    pub fn chars(&self) -> usize {
        self.text().chars().count()
    }

    /// Where the caret is, as how many characters precede it: which gap
    /// between the dots to draw it in.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.text()
            .get(..self.caret)
            .map_or(0, |s| s.chars().count())
    }

    /// The dots that stand for what was typed.
    #[must_use]
    pub fn dots(&self) -> String {
        core::iter::repeat_n(DOT, self.chars()).collect()
    }

    /// Whether nothing was typed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.typed.is_empty()
    }

    /// Whether everything typed is selected, to be drawn so.
    #[must_use]
    pub const fn is_all_selected(&self) -> bool {
        self.all_selected
    }

    /// What was typed -- the very bytes, not a copy -- leaving the field
    /// empty, with as much room as before.
    pub fn take(&mut self) -> Secret {
        let room = self.typed.room();
        self.caret = 0;
        self.all_selected = false;
        core::mem::replace(&mut self.typed, Secret::with_room(room))
    }

    /// Empty the field, overwriting what was typed.
    pub fn clear(&mut self) {
        self.typed.clear();
        self.caret = 0;
        self.all_selected = false;
    }

    /// The boundary of the character before the caret, or the caret at the
    /// start.
    fn before_caret(&self) -> usize {
        self.text()
            .get(..self.caret)
            .and_then(|s| s.char_indices().next_back())
            .map_or(self.caret, |(at, _)| at)
    }

    /// The boundary just past the character after the caret, or the caret at
    /// the end.
    fn after_caret(&self) -> usize {
        self.text()
            .get(self.caret..)
            .and_then(|s| s.chars().next())
            .map_or(self.caret, |c| self.caret.saturating_add(c.len_utf8()))
    }

    /// Put the characters `chars` yields in at the caret -- in place of
    /// everything, when it is all selected -- leaving out control characters:
    /// all of them or, when there is not the room, none. A function rather
    /// than an iterator, to be walked twice -- once to measure, once to put
    /// in -- without collecting the characters into a copy.
    fn put<I: Iterator<Item = char>>(&mut self, chars: impl Fn() -> I) -> KeyEdit {
        let needed = chars()
            .filter(|c| !c.is_control())
            .map(char::len_utf8)
            .fold(0usize, usize::saturating_add);
        if needed == 0 {
            return KeyEdit::Handled;
        }
        let kept = if self.all_selected {
            0
        } else {
            self.typed.len()
        };
        if kept.saturating_add(needed) > self.typed.room() {
            return KeyEdit::Handled;
        }
        if self.all_selected {
            self.clear();
        }
        for c in chars().filter(|c| !c.is_control()) {
            let mut buf = [0u8; 4];
            // Room for all of them was found above, so none is refused.
            if self
                .typed
                .insert(self.caret, c.encode_utf8(&mut buf).as_bytes())
            {
                self.caret = self.caret.saturating_add(c.len_utf8());
            }
        }
        KeyEdit::Changed
    }

    /// Take out `start..end`, leaving the caret at `start`.
    fn cut_out(&mut self, start: usize, end: usize) -> KeyEdit {
        if start == end || !self.typed.remove(start..end) {
            return KeyEdit::Handled;
        }
        self.caret = start;
        KeyEdit::Changed
    }

    /// Paste `text` -- the clipboard's -- at the caret, as typing it would.
    pub fn paste_text(&mut self, text: &str) -> KeyEdit {
        self.put(|| text.chars())
    }

    /// Apply one keystroke's editing meaning, and say what it did -- as
    /// [`TextInput::edit_key`](crate::textinput::TextInput::edit_key) does,
    /// with one difference: Ctrl+C and Ctrl+X are taken and do nothing, as a
    /// password is not copied out of its field.
    ///
    /// An arrow, Home or End ends a selection -- an arrow at that end of it;
    /// Backspace or Delete empties the field; a key typed or pasted replaces
    /// it, unless there is not the room, when the selection stays.
    pub fn edit_key(&mut self, key: &KeyEvent) -> KeyEdit {
        if !key.pressed {
            return KeyEdit::Unhandled;
        }
        let chord = key.modifiers.is_ctrl_chord();
        match key.key {
            Key::A if chord => {
                self.all_selected = !self.typed.is_empty();
                KeyEdit::Handled
            }
            Key::C | Key::X if chord => KeyEdit::Handled,
            Key::V if chord => self.paste_text(&crate::clipboard::text()),
            Key::Left | Key::Right | Key::Home | Key::End => {
                self.caret = match key.key {
                    Key::Home => 0,
                    Key::End => self.typed.len(),
                    Key::Left if self.all_selected => 0,
                    Key::Right if self.all_selected => self.typed.len(),
                    Key::Left => self.before_caret(),
                    _ => self.after_caret(),
                };
                self.all_selected = false;
                KeyEdit::Handled
            }
            Key::Backspace | Key::Delete if self.all_selected => {
                self.clear();
                KeyEdit::Changed
            }
            Key::Backspace => self.cut_out(self.before_caret(), self.caret),
            Key::Delete => self.cut_out(self.caret, self.after_caret()),
            _ if key.types_text() => self.put(|| key.typed()),
            _ => KeyEdit::Unhandled,
        }
    }
}

#[cfg(test)]
#[path = "secretinput_tests.rs"]
mod tests;
