//! A window one program lends so that another program's window can belong to
//! it (design-decisions 1387): the file explorer's Open and Save window,
//! which to the user is the asking program's dialog (§1415).
//!
//! Wayland's xdg-foreign, in this protocol's terms. The program that owns the
//! window asks the compositor to export it ([`RequestBody::ExportWindow`]) and
//! gets an [`ExportedWindow`] -- sixteen bytes the compositor drew -- which it
//! sends to the other program in its own request, as text if it likes
//! ([`ExportedWindow::to_text`]). The other program names it as its window's
//! parent ([`RequestBody::SetParent`] with [`Parent::Exported`]). Only a
//! program the owner chose to tell can do that: a window cannot be named by
//! its raw id across programs, so nobody can float a window of theirs over
//! someone else's without being asked.
//!
//! A program's own dialogs need no export: [`Parent::Own`] names one of its
//! own windows directly.
//!
//! [`RequestBody::ExportWindow`]: crate::control::RequestBody::ExportWindow
//! [`RequestBody::SetParent`]: crate::control::RequestBody::SetParent

use std::fmt;

use crate::hex16;

/// A window lent to another program: whoever holds these sixteen bytes may
/// make a window of theirs belong to it, and nobody else can name it. Good
/// while the window is open; exporting the same window again gives the same
/// bytes.
///
/// Opaque, like an activation token: its `Debug` form shows none of the
/// bytes.
#[derive(Clone, Copy, Eq)]
pub struct ExportedWindow([u8; ExportedWindow::LEN]);

impl ExportedWindow {
    /// Bytes in a handle.
    pub const LEN: usize = 16;

    /// A handle from its bytes -- as the compositor draws one, or the wire
    /// carries one.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    /// The handle's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::LEN] {
        self.0
    }

    /// The handle as text, to carry in another program's request: 32
    /// lowercase hexadecimal digits.
    #[must_use]
    pub fn to_text(self) -> String {
        hex16::to_text(self.0)
    }

    /// Read a handle's text form: exactly 32 hexadecimal digits, either case,
    /// and nothing else.
    #[must_use]
    pub fn from_text(text: &[u8]) -> Option<Self> {
        hex16::from_text(text).map(Self)
    }
}

impl PartialEq for ExportedWindow {
    /// Every byte compared whatever the first differing one.
    fn eq(&self, other: &Self) -> bool {
        hex16::same(&self.0, &other.0)
    }
}

impl fmt::Debug for ExportedWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExportedWindow(..)")
    }
}

/// What a window belongs to ([`RequestBody::SetParent`]): kept above it,
/// raised with it, placed over it, and its keyboard handed back to it when
/// the window closes.
///
/// [`RequestBody::SetParent`]: crate::control::RequestBody::SetParent
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    /// Nothing: an ordinary window again.
    None,
    /// Another of the sender's own windows, by its id: a program's own
    /// dialog.
    Own(u64),
    /// A window another program lent ([`ExportedWindow`]).
    Exported(ExportedWindow),
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly at the line that did
    // it. The defensive lints guard code that runs on a user's data, not this.
    #![allow(clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_handle_survives_its_text_form_and_shows_nothing_when_logged() {
        let handle = ExportedWindow::from_bytes(*b"0123456789abcdef");
        assert_eq!(handle.to_text(), "30313233343536373839616263646566");
        assert_eq!(
            ExportedWindow::from_text(handle.to_text().as_bytes()),
            Some(handle)
        );
        assert_eq!(
            ExportedWindow::from_text(b"+0313233343536373839616263646566"),
            None
        );
        assert_eq!(format!("{handle:?}"), "ExportedWindow(..)");
    }

    #[test]
    fn handles_are_equal_only_when_every_byte_is() {
        let handle = ExportedWindow::from_bytes([9; 16]);
        assert_eq!(handle, ExportedWindow::from_bytes([9; 16]));
        let mut other = [9; 16];
        other[15] = 8;
        assert_ne!(handle, ExportedWindow::from_bytes(other));
    }
}
